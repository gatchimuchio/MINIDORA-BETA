use crate::scheduler::{GoalNode, GoalNodeKind, Scheduler};
use crate::{input, language, output, ports, relations, store::Store};
use minidora_domain::*;
use std::collections::BTreeMap;
use std::path::Path;
use std::time::Instant;

#[derive(serde::Serialize)]
struct CalculationSequenceStepArtifact {
    id: String,
    source_expression: SourceSpan,
    evaluated_expression: String,
    calculation: Calculation,
    consumed: Vec<language::CalculationReferenceUse>,
}

/// Trusted frontends supply an authenticated local principal. A caller-controlled
/// actor string is not remote authentication; the HTTP adapter binds it to its
/// configured authenticated identity before entering this API.
pub struct Engine {
    pub(crate) store: Store,
}
impl Engine {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        Ok(Self {
            store: Store::open(path)?,
        })
    }
    pub fn reconcile_cancelled_effects(
        &mut self,
        actor: &str,
        case: &str,
    ) -> Result<Vec<ActionRecord>> {
        crate::recovery::reconcile_cancelled_effects(&self.store, actor, case)
    }
    pub fn reconcile(&mut self, actor: &str, case: &str) -> Result<ResultRecord> {
        crate::recovery::reconcile(&self.store, actor, case)
    }
    pub fn latest(&self, actor: &str, case: &str) -> Result<Option<CaseRevision>> {
        self.store.load_case(actor, case)
    }
    pub fn execute(&mut self, actor: &str, request: RequestEnvelope) -> Result<ResultRecord> {
        let attempt = self.execute_inner(actor, request.clone());
        match attempt {
            Ok(value) => Ok(value),
            Err(error) => {
                // Preserve a typed terminal for the SAME accepted request only;
                // never repair a concurrently advanced request or hide integrity errors.
                if let Ok(Some(mut state)) = self.store.load_case(actor, &request.case_id)
                    && state.input_fingerprint.as_deref() == Some(identity(&request)?.as_str())
                    && matches!(
                        state.lifecycle,
                        Lifecycle::Received
                            | Lifecycle::Sealed
                            | Lifecycle::Running
                            | Lifecycle::Adopted
                    )
                {
                    if state.result.is_none() {
                        state.result = Some(base_result(&request));
                    }
                    let terminal = if matches!(error, Error::Contract { .. }) {
                        Lifecycle::Suspend
                    } else {
                        Lifecycle::Fail
                    };
                    if state.lifecycle.permits(terminal)
                        && let Ok(result) =
                            self.stop(&mut state, terminal, "EXECUTION_ERROR", &error.to_string())
                    {
                        return Ok(result);
                    }
                }
                Err(error)
            }
        }
    }
    fn execute_inner(&mut self, actor: &str, request: RequestEnvelope) -> Result<ResultRecord> {
        if actor != request.actor_id || actor.is_empty() {
            return Ok(rejected(
                &request,
                "ACTOR_MISMATCH",
                "認証された使用者と入力の主体が一致しません。",
            ));
        }
        if request.case_id.is_empty()
            || request.input_id.is_empty()
            || request.case_id.len() > 256
            || request.input_id.len() > 256
        {
            return Ok(rejected(
                &request,
                "INVALID_IDENTITY",
                "空または長すぎる識別子です。",
            ));
        }
        let mut latest = match self.store.load_case(actor, &request.case_id) {
            Ok(v) => v,
            Err(e) => return Ok(rejected(&request, "CASE_ACCESS_DENIED", &e.to_string())),
        };
        let fingerprint = identity(&request)?;
        let fresh = match self.store.register_input(actor, &request, &fingerprint) {
            Ok(v) => v,
            Err(e) => {
                return Ok(rejected(
                    &request,
                    "INPUT_IDENTITY_CONFLICT",
                    &e.to_string(),
                ));
            }
        };
        if !fresh {
            if let Some(mut saved) = self.store.get_input_result(
                actor,
                &request.case_id,
                &request.input_id,
                request.input_version,
            )? {
                if matches!(
                    saved.lifecycle,
                    Lifecycle::Received
                        | Lifecycle::Sealed
                        | Lifecycle::Running
                        | Lifecycle::Adopted
                ) {
                    saved.lifecycle = Lifecycle::Suspend;
                    saved.reason=Some("INCOMPLETE_PRIOR_ATTEMPT: no effect is retried; explicit reconciliation is required".into());
                }
                return Ok(saved);
            }
            let mut result = rejected(
                &request,
                "INCOMPLETE_PRIOR_ATTEMPT",
                "前回実行は未確定です。保存済み作用を照合するまで再実行しません。",
            );
            result.lifecycle = Lifecycle::Suspend;
            return Ok(result);
        }
        let mut early_bundle = None;
        let pending = latest.as_ref().is_some_and(|s| {
            matches!(
                s.lifecycle,
                Lifecycle::Received | Lifecycle::Sealed | Lifecycle::Running | Lifecycle::Adopted
            )
        });
        if pending {
            let interpreted =
                input::seal(&request, latest.as_ref().and_then(|s| s.result.as_ref()))?;
            if matches!(interpreted.meaning.intent, Intent::Cancel) {
                let mut old = latest.take().expect("pending case exists");
                self.advance(
                    &mut old,
                    Lifecycle::Cancel,
                    Some(format!(
                        "AUTHORIZED_CANCEL_REQUEST:{fingerprint}; past effects retained"
                    )),
                )?;
                latest = Some(old);
                early_bundle = Some(interpreted);
            }
        }
        if pending && early_bundle.is_none() {
            let mut r = rejected(
                &request,
                "CASE_BUSY_OR_RECOVERY_REQUIRED",
                "未確定のケースがあります。既存作用を照合してから再開してください。",
            );
            r.lifecycle = Lifecycle::Suspend;
            return Ok(r);
        }
        if latest.is_some()
            && self
                .store
                .actions(actor, &request.case_id)?
                .iter()
                .any(|a| matches!(a.phase, EffectPhase::Unknown | EffectPhase::Attempted))
        {
            let candidate = early_bundle.take().map_or_else(
                || input::seal(&request, latest.as_ref().and_then(|s| s.result.as_ref())),
                Ok,
            )?;
            if matches!(candidate.meaning.intent, Intent::Cancel) {
                if latest.as_ref().is_some_and(|old| {
                    old.lifecycle == Lifecycle::Suspend && old.goal_sha256.is_some()
                }) {
                    let mut old = latest.take().expect("suspended case exists");
                    self.advance(
                        &mut old,
                        Lifecycle::Running,
                        Some(format!("AUTHORIZED_CANCEL_REQUEST:{fingerprint}")),
                    )?;
                    self.advance(
                        &mut old,
                        Lifecycle::Cancel,
                        Some("CANCELLED_WITH_ORIGINAL_UNKNOWN_EFFECT_ACCOUNTING".into()),
                    )?;
                    latest = Some(old);
                }
                early_bundle = Some(candidate);
            } else {
                let mut r = rejected(
                    &request,
                    "EXTERNAL_EFFECT_UNKNOWN",
                    "既存作用の結果が未確定です。同一identityを再照合するまで新しい実行を開始しません。",
                );
                r.lifecycle = Lifecycle::Suspend;
                return Ok(r);
            }
        }
        let previous = if let Some(ref requested) = request.context_ref {
            latest.as_ref().and_then(|s| {
                s.result
                    .as_ref()
                    .filter(|r| &r.id == requested)
                    .or_else(|| s.prior_results.get(requested))
            })
        } else {
            latest.as_ref().and_then(|s| s.result.as_ref())
        };
        let mut prior_results = latest
            .as_ref()
            .map(|s| s.prior_results.clone())
            .unwrap_or_default();
        if let Some(ref s) = latest
            && let Some(ref r) = s.result
        {
            prior_results.insert(r.id.clone(), r.clone());
        }
        let mut state = CaseRevision {
            schema_version: SCHEMA_VERSION.into(),
            case_id: request.case_id.clone(),
            actor_id: actor.into(),
            revision: latest.as_ref().map_or(0, |s| s.revision + 1),
            parent_revision: latest.as_ref().map(|s| s.revision),
            lifecycle: Lifecycle::Received,
            goal_sha256: None,
            input_fingerprint: Some(fingerprint.clone()),
            bundle: None,
            result: None,
            reason: None,
            prior_results,
        };
        self.store.append_case(actor, &state)?;
        let started = Instant::now();
        let sealed = early_bundle.map_or_else(|| input::seal(&request, previous), Ok);
        let bundle = match sealed {
            Ok(v) => v,
            Err(e) => {
                state.result = Some(base_result(&request));
                return self.stop(&mut state, Lifecycle::Fail, "INPUT_FAILURE", &e.to_string());
            }
        };
        state.bundle = Some(bundle.clone());
        self.advance(&mut state, Lifecycle::Sealed, None)?;
        let mut nodes = Vec::new();
        let mut compute_requires = Vec::new();
        if let Intent::Retrieve { urls, .. } = &bundle.meaning.intent {
            for (index, _) in urls.iter().enumerate() {
                let id = format!("source-{index}");
                compute_requires.push(id.clone());
                nodes.push(GoalNode {
                    id,
                    kind: GoalNodeKind::Evidence,
                    requires: Vec::new(),
                });
            }
        }
        if let Intent::CalculateSequence { steps } = &bundle.meaning.intent {
            for step in steps {
                let id = format!("sequence-{}", step.id);
                nodes.push(GoalNode {
                    id: id.clone(),
                    kind: GoalNodeKind::ComputedValue,
                    requires: step
                        .requires
                        .iter()
                        .map(|required| format!("sequence-{required}"))
                        .collect(),
                });
                compute_requires.push(id);
            }
        }
        nodes.extend([
            GoalNode {
                id: "compute".into(),
                kind: GoalNodeKind::CoreResult,
                requires: compute_requires,
            },
            GoalNode {
                id: "content".into(),
                kind: GoalNodeKind::ContentPlan,
                requires: vec!["compute".into()],
            },
            GoalNode {
                id: "render".into(),
                kind: GoalNodeKind::RenderedArtifact,
                requires: vec!["content".into()],
            },
        ]);
        if request.output_path.is_some() {
            nodes.push(GoalNode {
                id: "file".into(),
                kind: GoalNodeKind::RequiredFile,
                requires: vec!["render".into()],
            });
        }
        let required_graph_ref = self.store.put_artifact(actor, &request.case_id, &nodes)?;
        let required_actions = nodes.len();
        let mut graph = Scheduler::new(nodes, request.budget.max_actions)?;
        let goal = GoalContract {
            input_fingerprint: fingerprint.clone(),
            intent: bundle.meaning.intent.clone(),
            output_path: request.output_path.clone(),
            permissions: request.permissions.clone(),
            budget: request.budget.clone(),
            required_graph_ref,
            required_predicates: if request.output_path.is_some() {
                vec![
                    "SUPPORTED_CONTENT".into(),
                    "EXACT_RENDER".into(),
                    "CONFIRMED_FILE".into(),
                ]
            } else {
                vec!["SUPPORTED_CONTENT".into(), "EXACT_RENDER".into()]
            },
        };
        state.goal_sha256 = Some(self.store.put_artifact(actor, &request.case_id, &goal)?);
        self.advance(&mut state, Lifecycle::Running, None)?;
        if matches!(bundle.meaning.intent, Intent::Cancel) {
            let mut cancellation = base_result(&request);
            cancellation.receipts = self
                .store
                .actions(actor, &request.case_id)?
                .into_iter()
                .map(|action| {
                    action.receipt.unwrap_or(DeliveryReceipt {
                        action_id: action.action_id,
                        target: action.target,
                        content_sha256: action.exact_request_sha256,
                        phase: action.phase,
                        observed_bytes: None,
                        reason: Some(
                            "actual prior effect accounting; cancellation does not undo it".into(),
                        ),
                    })
                })
                .collect();
            state.result = Some(cancellation);
            return self.stop(
                &mut state,
                Lifecycle::Cancel,
                "AUTHORIZED_CANCEL",
                "取消を記録しました。既発生・未確定の作用は消去せず保持しています。",
            );
        }
        if request.locale != "ja" {
            return self.stop(
                &mut state,
                Lifecycle::Suspend,
                "SEMANTIC_VERIFIER_UNAVAILABLE",
                "この版の表現検証は日本語の限定経路のみ対応しています。",
            );
        }
        if request.budget.max_actions == 0 || request.budget.deadline_ms == 0 {
            return self.stop(
                &mut state,
                Lifecycle::Suspend,
                "RESOURCE_LIMIT",
                "計算予算がありません。",
            );
        }
        if (request.budget.max_actions as usize) < required_actions {
            return self.stop(
                &mut state,
                Lifecycle::Suspend,
                "ACTION_BUDGET_EXHAUSTED",
                &format!("必須成果の残件: {:?}", graph.residuals()),
            );
        }
        let mut result = base_result(&request);
        let mut units = Vec::new();
        match &bundle.meaning.intent {
            Intent::Calculate { expression } => match language::calculate(expression) {
                Ok(calculation) => {
                    units.push(format!(
                        "{} = {}",
                        calculation.expression, calculation.exact_value
                    ));
                    result.calculation = Some(calculation);
                    result.guarantee = Guarantee::Computed;
                }
                Err(e) => {
                    let stop_code = match &e {
                        minidora_domain::Error::Contract { code, .. }
                            if code == "UNIT_MISMATCH" =>
                        {
                            "UNIT_MISMATCH"
                        }
                        _ => "CALCULATION_UNSUPPORTED",
                    };
                    return self.stop(&mut state, Lifecycle::Suspend, stop_code, &e.to_string());
                }
            },
            Intent::CalculateSequence { steps } => {
                let mut completed = BTreeMap::<String, language::SequenceValue>::new();
                let mut artifact_refs = Vec::with_capacity(steps.len());
                let mut final_value = None;
                for step in steps {
                    if started.elapsed().as_millis() >= u128::from(request.budget.deadline_ms) {
                        state.result = Some(result.clone());
                        return self.stop(
                            &mut state,
                            Lifecycle::Suspend,
                            "DEADLINE",
                            "計算手順の途中で要求全体の締切へ達しました。",
                        );
                    }
                    let node_id = format!("sequence-{}", step.id);
                    if !graph.ready().iter().any(|ready| ready == &node_id) {
                        state.result = Some(result.clone());
                        return self.stop(
                            &mut state,
                            Lifecycle::Suspend,
                            "UNOBSERVED_PREREQUISITE",
                            &format!("{} の必須入力artifactがまだ観測されていません。", step.id),
                        );
                    }
                    let resolved = match language::calculate_sequence_expression(
                        &step.expression.text,
                        &completed,
                    ) {
                        Ok(value) => value,
                        Err(error) => {
                            state.result = Some(result.clone());
                            let code = match &error {
                                minidora_domain::Error::Contract { code, .. } => code.as_str(),
                                _ => "CALCULATION_SEQUENCE_UNSUPPORTED",
                            };
                            return self.stop(
                                &mut state,
                                Lifecycle::Suspend,
                                code,
                                &error.to_string(),
                            );
                        }
                    };
                    let consumed: BTreeMap<String, String> = resolved
                        .consumed
                        .iter()
                        .map(|reference| {
                            (reference.step_id.clone(), reference.artifact_sha256.clone())
                        })
                        .collect();
                    let declared: std::collections::BTreeSet<_> =
                        step.requires.iter().cloned().collect();
                    let actual: std::collections::BTreeSet<_> = consumed.keys().cloned().collect();
                    if declared != actual {
                        state.result = Some(result.clone());
                        return self.stop(
                            &mut state,
                            Lifecycle::Fail,
                            "SEQUENCE_DEPENDENCY_MISMATCH",
                            "入力で宣言された参照集合と計算器が実際に消費した参照集合が一致しません。",
                        );
                    }
                    let artifact = CalculationSequenceStepArtifact {
                        id: step.id.clone(),
                        source_expression: step.expression.clone(),
                        evaluated_expression: resolved.evaluated_expression.clone(),
                        calculation: resolved.calculation.clone(),
                        consumed: resolved.consumed,
                    };
                    let artifact_sha256 =
                        self.store
                            .put_artifact(actor, &request.case_id, &artifact)?;
                    graph.observe(&node_id, &artifact_sha256, &consumed)?;
                    result.dependencies.push(artifact_sha256.clone());
                    units.push(format!(
                        "{}: {} = {}",
                        step.id, resolved.evaluated_expression, resolved.calculation.exact_value
                    ));
                    completed.insert(
                        step.id.clone(),
                        language::SequenceValue {
                            exact_value: resolved.calculation.exact_value.clone(),
                            artifact_sha256: artifact_sha256.clone(),
                        },
                    );
                    final_value = Some((step.id.clone(), resolved.calculation.clone()));
                    artifact_refs.push((step.id.clone(), artifact_sha256));
                }
                let Some((final_step_id, final_calculation)) = final_value else {
                    return self.stop(
                        &mut state,
                        Lifecycle::Suspend,
                        "CALCULATION_SEQUENCE_EMPTY",
                        "実行可能な計算手順がありません。",
                    );
                };
                let report = (
                    "minidora-calculation-sequence-report-v1",
                    &artifact_refs,
                    &final_step_id,
                    &final_calculation.exact_value,
                    "each declared downstream reference was substituted with the exact value of its observed upstream calculation artifact",
                );
                if serde_json::to_vec(&report)?.len() > request.budget.max_evidence_bytes {
                    state.result = Some(result.clone());
                    return self.stop(
                        &mut state,
                        Lifecycle::Suspend,
                        "RESOURCE_LIMIT",
                        "計算手順のartifact参照報告が出力予算を超えています。",
                    );
                }
                let report_sha256 = self.store.put_artifact(actor, &request.case_id, &report)?;
                result.dependencies.push(report_sha256);
                result.calculation = Some(final_calculation);
                result.guarantee = Guarantee::Computed;
            }
            Intent::CorrectCalculation {
                operand_index,
                replacement,
            } => {
                let Some(prior) = previous.filter(|prior| prior.lifecycle == Lifecycle::Commit)
                else {
                    return self.stop(
                        &mut state,
                        Lifecycle::Suspend,
                        "MISSING_ACTUAL_RESULT",
                        "訂正対象となる実際の計算結果がありません。",
                    );
                };
                let Some(prior_calculation) = prior.calculation.as_ref() else {
                    return self.stop(
                        &mut state,
                        Lifecycle::Suspend,
                        "MISSING_ACTUAL_CALCULATION",
                        "直前の採用済み成果に計算式がありません。",
                    );
                };
                match language::correct_binary_operand(
                    prior_calculation,
                    *operand_index,
                    replacement,
                ) {
                    Ok(calculation) => {
                        units.push(format!(
                            "{} = {}",
                            calculation.expression, calculation.exact_value
                        ));
                        result.dependencies.push(prior.id.clone());
                        result.calculation = Some(calculation);
                        result.guarantee = Guarantee::Computed;
                    }
                    Err(error) => {
                        let stop_code = match &error {
                            minidora_domain::Error::Contract { code, .. } => code.as_str(),
                            _ => "CORRECTION_UNSUPPORTED",
                        };
                        return self.stop(
                            &mut state,
                            Lifecycle::Suspend,
                            stop_code,
                            &error.to_string(),
                        );
                    }
                }
            }
            Intent::EvaluatePolynomial {
                expression,
                bindings,
            } => match crate::math::evaluate_polynomial_report(expression, bindings) {
                Ok(report) => {
                    units.push(serde_json::to_string_pretty(&report)?);
                    result.guarantee = Guarantee::Computed;
                }
                Err(error) => {
                    let code = match &error {
                        minidora_domain::Error::Contract { code, .. } => code.as_str(),
                        _ => "POLYNOMIAL_EVALUATION_UNSUPPORTED",
                    };
                    return self.stop(&mut state, Lifecycle::Suspend, code, &error.to_string());
                }
            },
            Intent::Symbolic {
                expression,
                operation,
                variable,
            } => {
                match crate::math::symbolic(expression, operation, variable) {
                    Ok(value) => units.push(serde_json::to_string_pretty(&value)?),
                    Err(e) => {
                        return self.stop(
                            &mut state,
                            Lifecycle::Suspend,
                            "SYMBOLIC_UNSUPPORTED",
                            &e.to_string(),
                        );
                    }
                }
                result.guarantee = Guarantee::Computed;
            }
            Intent::LinearSystem { coefficients, rhs } => {
                match crate::math::solve_linear(coefficients.clone(), rhs.clone()) {
                    Ok(solution) => units.push(serde_json::to_string_pretty(&solution)?),
                    Err(e) => {
                        return self.stop(
                            &mut state,
                            Lifecycle::Suspend,
                            "LINEAR_SYSTEM_UNSUPPORTED",
                            &e.to_string(),
                        );
                    }
                }
                result.guarantee = Guarantee::Computed;
            }
            Intent::Statistics {
                values,
                units: source_units,
            } => {
                match crate::math::summarize(values, source_units.as_deref()) {
                    Ok(statistics) => units.push(serde_json::to_string_pretty(&statistics)?),
                    Err(e) => {
                        return self.stop(
                            &mut state,
                            Lifecycle::Suspend,
                            "STATISTICS_UNSUPPORTED",
                            &e.to_string(),
                        );
                    }
                }
                result.guarantee = Guarantee::Computed;
            }
            Intent::NormalizeProbabilityWeights { weights } => {
                match crate::math::normalize_probability_weights(weights) {
                    Ok(distribution) => {
                        units.push(serde_json::to_string_pretty(&distribution)?);
                        result.guarantee = Guarantee::Computed;
                    }
                    Err(error) => {
                        let code = match &error {
                            minidora_domain::Error::Contract { code, .. } => code.as_str(),
                            _ => "PROBABILITY_NORMALIZATION_UNSUPPORTED",
                        };
                        return self.stop(&mut state, Lifecycle::Suspend, code, &error.to_string());
                    }
                }
            }
            Intent::FiniteProbabilityStatistics { outcomes, weights } => {
                match crate::math::finite_probability_statistics(outcomes, weights) {
                    Ok(statistics) => {
                        units.push(serde_json::to_string_pretty(&statistics)?);
                        result.guarantee = Guarantee::Computed;
                    }
                    Err(error) => {
                        let code = match &error {
                            minidora_domain::Error::Contract { code, .. } => code.as_str(),
                            _ => "PROBABILITY_STATISTICS_UNSUPPORTED",
                        };
                        return self.stop(&mut state, Lifecycle::Suspend, code, &error.to_string());
                    }
                }
            }
            Intent::FiniteEventProbability {
                outcomes,
                weights,
                event_values,
            } => match crate::math::finite_event_probability(outcomes, weights, event_values) {
                Ok(probability) => {
                    units.push(serde_json::to_string_pretty(&probability)?);
                    result.guarantee = Guarantee::Computed;
                }
                Err(error) => {
                    let code = match &error {
                        minidora_domain::Error::Contract { code, .. } => code.as_str(),
                        _ => "EVENT_PROBABILITY_UNSUPPORTED",
                    };
                    return self.stop(&mut state, Lifecycle::Suspend, code, &error.to_string());
                }
            },
            Intent::CsvSelect { document, column } => {
                let value = crate::document::csv_select(document, column)?;
                let artifact = self.store.put_artifact(actor, &request.case_id, &value)?;
                result.dependencies.push(artifact);
                units.push(value.text);
                result.guarantee = Guarantee::Computed;
            }
            Intent::CsvFilter {
                document,
                column,
                equals,
            } => {
                let value = match crate::document::csv_filter(document, column, equals) {
                    Ok(value) => value,
                    Err(error) => {
                        let code = match &error {
                            minidora_domain::Error::Contract { code, .. } => code.as_str(),
                            _ => "CSV_FILTER_UNSUPPORTED",
                        };
                        return self.stop(&mut state, Lifecycle::Suspend, code, &error.to_string());
                    }
                };
                let artifact = self.store.put_artifact(actor, &request.case_id, &value)?;
                result.dependencies.push(artifact);
                units.push(value.text);
                result.guarantee = Guarantee::Computed;
            }
            Intent::CsvSort {
                document,
                column,
                descending,
            } => {
                let value = match crate::document::csv_sort(document, column, *descending) {
                    Ok(value) => value,
                    Err(error) => {
                        let code = match &error {
                            minidora_domain::Error::Contract { code, .. } => code.as_str(),
                            _ => "CSV_SORT_UNSUPPORTED",
                        };
                        return self.stop(&mut state, Lifecycle::Suspend, code, &error.to_string());
                    }
                };
                let artifact = self.store.put_artifact(actor, &request.case_id, &value)?;
                result.dependencies.push(artifact);
                units.push(value.text);
                result.guarantee = Guarantee::Computed;
            }
            Intent::CsvGroupCount { document, column } => {
                let value = match crate::document::csv_group_count(document, column) {
                    Ok(value) => value,
                    Err(error) => {
                        let code = match &error {
                            minidora_domain::Error::Contract { code, .. } => code.as_str(),
                            _ => "CSV_GROUP_COUNT_UNSUPPORTED",
                        };
                        return self.stop(&mut state, Lifecycle::Suspend, code, &error.to_string());
                    }
                };
                let artifact = self.store.put_artifact(actor, &request.case_id, &value)?;
                result.dependencies.push(artifact);
                units.push(value.text);
                result.guarantee = Guarantee::Computed;
            }
            Intent::CsvGroupStatistics {
                document,
                group_column,
                value_column,
            } => {
                let value = match crate::document::csv_group_statistics(
                    document,
                    group_column,
                    value_column,
                ) {
                    Ok(value) => value,
                    Err(error) => {
                        let code = match &error {
                            minidora_domain::Error::Contract { code, .. } => code.as_str(),
                            _ => "CSV_GROUP_STATISTICS_UNSUPPORTED",
                        };
                        return self.stop(&mut state, Lifecycle::Suspend, code, &error.to_string());
                    }
                };
                let artifact = self.store.put_artifact(actor, &request.case_id, &value)?;
                result.dependencies.push(artifact);
                units.push(value.text);
                result.guarantee = Guarantee::Computed;
            }
            Intent::CsvCompare {
                document,
                key_column,
            } => {
                let value = match crate::document::csv_compare_by(document, key_column) {
                    Ok(value) => value,
                    Err(error) => {
                        let code = match &error {
                            minidora_domain::Error::Contract { code, .. } => code.as_str(),
                            _ => "CSV_TABLE_COMPARE_UNSUPPORTED",
                        };
                        return self.stop(&mut state, Lifecycle::Suspend, code, &error.to_string());
                    }
                };
                let artifact = self.store.put_artifact(actor, &request.case_id, &value)?;
                result.dependencies.push(artifact);
                units.push(value.text);
                result.guarantee = Guarantee::Computed;
            }
            Intent::CsvToJson {
                document,
                has_header,
            } => {
                let value = match crate::document::csv_to_json(document, *has_header) {
                    Ok(value) => value,
                    Err(error) => {
                        let code = match &error {
                            minidora_domain::Error::Contract { code, .. } => code.as_str(),
                            _ => "CSV_JSON_CONVERSION_UNSUPPORTED",
                        };
                        return self.stop(&mut state, Lifecycle::Suspend, code, &error.to_string());
                    }
                };
                let artifact = self.store.put_artifact(actor, &request.case_id, &value)?;
                result.dependencies.push(artifact);
                units.push(value.text);
                result.guarantee = Guarantee::Computed;
            }
            Intent::JsonToCsv { document } => {
                let value = match crate::document::json_to_csv(document) {
                    Ok(value) => value,
                    Err(error) => {
                        let code = match &error {
                            minidora_domain::Error::Contract { code, .. } => code.as_str(),
                            _ => "JSON_CSV_CONVERSION_UNSUPPORTED",
                        };
                        return self.stop(&mut state, Lifecycle::Suspend, code, &error.to_string());
                    }
                };
                let artifact = self.store.put_artifact(actor, &request.case_id, &value)?;
                result.dependencies.push(artifact);
                units.push(value.text);
                result.guarantee = Guarantee::Computed;
            }
            Intent::CsvStatistics { document, column } => {
                let value = match crate::document::csv_statistics(document, column) {
                    Ok(value) => value,
                    Err(error) => {
                        let code = match &error {
                            minidora_domain::Error::Contract { code, .. } => code.as_str(),
                            _ => "CSV_STATISTICS_UNSUPPORTED",
                        };
                        return self.stop(&mut state, Lifecycle::Suspend, code, &error.to_string());
                    }
                };
                let artifact = self.store.put_artifact(actor, &request.case_id, &value)?;
                result.dependencies.push(artifact);
                units.push(value.text);
                result.guarantee = Guarantee::Computed;
            }
            Intent::TextReplace {
                document,
                old,
                new,
                count,
            } => {
                let value = crate::document::replace_exact(document, old, new, *count)?;
                let artifact = self.store.put_artifact(actor, &request.case_id, &value)?;
                result.dependencies.push(artifact);
                units.push(value.text);
                result.guarantee = Guarantee::Computed;
            }
            Intent::Search { provider, query } => {
                use crate::search::SearchProvider;
                let selected = match provider.as_str() {
                    "wikipedia" => SearchProvider::Wikipedia,
                    "crossref" => SearchProvider::Crossref,
                    "europepmc" => SearchProvider::EuropePmc,
                    _ => {
                        return self.stop(
                            &mut state,
                            Lifecycle::Suspend,
                            "SEARCH_PROVIDER_UNAVAILABLE",
                            "指定された検索接続がありません。",
                        );
                    }
                };
                let mut remaining = request.budget.clone();
                remaining.deadline_ms = remaining.deadline_ms.saturating_sub(
                    u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
                );
                let observation = match selected.search(query, &request.permissions, &remaining) {
                    Ok(value) => value,
                    Err(error) => {
                        return self.stop(
                            &mut state,
                            Lifecycle::Suspend,
                            "SEARCH_UNAVAILABLE",
                            &error.to_string(),
                        );
                    }
                };
                let artifact = self
                    .store
                    .put_artifact(actor, &request.case_id, &observation)?;
                result.dependencies.push(artifact);
                result.evidence.push(observation.listing.clone());
                if let Some(error) = &observation.failure {
                    state.result = Some(result);
                    return self.stop(
                        &mut state,
                        Lifecycle::Suspend,
                        "SEARCH_OBSERVATION_FAILURE",
                        error,
                    );
                }
                units.push(
                    "検索一覧（本文は未取得。題名・URLは本文の事実根拠ではありません）:".into(),
                );
                for candidate in &observation.candidates {
                    units.push(format!(
                        "{}: {}\n{}",
                        candidate.rank, candidate.title, candidate.uri
                    ));
                }
                if observation.candidates.is_empty() {
                    units.push("検索候補がありません。".into());
                }
                result.guarantee = Guarantee::SourceReport;
            }
            Intent::ExplainPrevious => {
                let Some(prior) = previous.filter(|p| p.lifecycle == Lifecycle::Commit) else {
                    return self.stop(
                        &mut state,
                        Lifecycle::Suspend,
                        "MISSING_ACTUAL_RESULT",
                        "説明対象となる実際の採用済み成果がありません。",
                    );
                };
                if let Some(ref calculation) = prior.calculation {
                    units.push(format!("計算式: {}", calculation.expression));
                    units.extend(calculation.steps.clone());
                    units.push(format!("計算結果: {}", calculation.exact_value));
                    result.calculation = Some(calculation.clone());
                } else {
                    units.push(prior.text.clone());
                }
                result.dependencies.push(prior.id.clone());
                result.guarantee = prior.guarantee.clone();
            }
            Intent::Assert { proposition } => {
                let mut memory = self.store.load_memory(actor)?;
                let fact = relations::assert_fact(&mut memory, proposition.clone(), &fingerprint)?;
                self.store.save_memory(actor, &memory)?;
                units.push(format!(
                    "前提を保存しました: {}。",
                    describe(&fact.proposition)
                ));
                units.push(format!("前提ID: {}", fact.id));
                units.push(
                    "保証範囲: 使用者が提示した前提。現実世界の真偽を検証したものではありません。"
                        .into(),
                );
                result.dependencies.push(fact.id);
                result.guarantee = Guarantee::UserPremise;
            }
            Intent::AddRule {
                premise,
                conclusion,
            } => {
                let mut memory = self.store.load_memory(actor)?;
                let rule = relations::add_rule(
                    &mut memory,
                    premise.clone(),
                    conclusion.clone(),
                    &fingerprint,
                )?;
                self.store.save_memory(actor, &memory)?;
                units.push(format!(
                    "条件付き前提を保存しました: {} ならば {}。",
                    describe(premise),
                    describe(conclusion)
                ));
                units.push(format!("規則ID: {}", rule.id));
                units.push("保証範囲: 提示された条件付き前提の中でのみ利用します。普遍的事実として学習しません。".into());
                result.dependencies.push(rule.id);
                result.guarantee = Guarantee::UserPremise;
            }
            Intent::Ask { proposition } => {
                let memory = self.store.load_memory(actor)?;
                let answer = relations::query(&memory, proposition, request.budget.max_actions)?;
                match answer.status.as_str() {
                    "SUPPORTED"=>{
                        units.push(format!("提示された前提の範囲では、{}。",describe(proposition)));
                        result.dependencies.extend(answer.facts.iter().map(|f|f.id.clone()));
                        result.dependencies.extend(answer.used_rules.clone());
                        result.guarantee=if answer.used_rules.is_empty(){Guarantee::UserPremise}else{Guarantee::Derived};
                    },
                    "CONTRADICTED"=>{
                        let mut opposite=proposition.clone(); opposite.negative = !opposite.negative;
                        units.push(format!("提示された前提の範囲では、{}。",describe(&opposite)));
                        result.dependencies.extend(answer.facts.iter().map(|f|f.id.clone()));
                        result.dependencies.extend(answer.used_rules.clone());
                        result.guarantee=Guarantee::Derived;
                    },
                    "CONFLICT"=>return self.stop(&mut state,Lifecycle::Suspend,"CONFLICTING_PREMISES",
                        "同じ対象・範囲に対して肯定と否定の根拠があります。一方を恣意的に採用しません。"),
                    _=>return self.stop(&mut state,Lifecycle::Suspend,"MISSING_PREMISES",
                        "この問いに必要な有効な前提または導出経路がありません。"),
                }
            }
            Intent::Invalidate { id } => {
                let mut memory = self.store.load_memory(actor)?;
                let affected = relations::invalidate(&mut memory, id)?;
                self.store.save_memory(actor, &memory)?;
                units.push(format!(
                    "{}件の前提・依存を失効させ、元の記録を保持しました。",
                    affected.len()
                ));
                result.dependencies = affected;
                result.guarantee = Guarantee::Computed;
            }
            Intent::Retrieve { urls, question: _ } => {
                if urls.len() > request.budget.max_actions as usize {
                    return self.stop(
                        &mut state,
                        Lifecycle::Suspend,
                        "RESOURCE_LIMIT",
                        "取得数が作用予算を超えています。",
                    );
                }
                let mut bytes = 0usize;
                for (source_index, uri) in urls.iter().enumerate() {
                    if started.elapsed().as_millis() > u128::from(request.budget.deadline_ms) {
                        return self.stop(
                            &mut state,
                            Lifecycle::Suspend,
                            "DEADLINE",
                            "取得の締切です。",
                        );
                    }
                    let mut remaining = request.budget.clone();
                    remaining.deadline_ms = remaining.deadline_ms.saturating_sub(
                        u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
                    );
                    if remaining.deadline_ms == 0 {
                        state.result = Some(result.clone());
                        return self.stop(
                            &mut state,
                            Lifecycle::Suspend,
                            "DEADLINE",
                            "取得の総締切です。",
                        );
                    }
                    remaining.max_evidence_bytes =
                        remaining.max_evidence_bytes.saturating_sub(bytes);
                    let packet = match ports::fetch(uri, &request.permissions, &remaining) {
                        Ok(p) => p,
                        Err(e) => {
                            state.result = Some(result.clone());
                            return self.stop(
                                &mut state,
                                Lifecycle::Suspend,
                                "EVIDENCE_UNAVAILABLE",
                                &e.to_string(),
                            );
                        }
                    };
                    let observed = self.store.put_artifact(actor, &request.case_id, &packet)?;
                    graph.observe(
                        &format!("source-{source_index}"),
                        &observed,
                        &BTreeMap::new(),
                    )?;
                    bytes = bytes.saturating_add(packet.raw_body.len());
                    result.dependencies.push(packet.id.clone());
                    result.evidence.push(packet);
                }
                if result
                    .evidence
                    .iter()
                    .any(|p| p.failure.is_some() || p.extracted_text.is_none())
                {
                    state.result = Some(result);
                    return self.stop(
                        &mut state,
                        Lifecycle::Suspend,
                        "FETCH_OR_PARSER_FAILURE",
                        "本文の取得または対応形式の検証ができません。失敗packetを保持しました。",
                    );
                }
                // Exact quotations are source reports. No open-domain factual
                // comparison or unverified summary is fabricated from a URL.
                for packet in &result.evidence {
                    units.push(format!(
                        "出典: {}\n本文（出典報告）:\n{}",
                        packet.final_uri,
                        packet.extracted_text.as_deref().unwrap_or_default()
                    ));
                }
                result.guarantee = Guarantee::SourceReport;
            }
            Intent::JsonPointer { document, pointer } => {
                match ports::json_pointer(document, pointer) {
                    Ok(value) => units.push(value),
                    Err(e) => {
                        return self.stop(
                            &mut state,
                            Lifecycle::Suspend,
                            "DOCUMENT_OPERATION_FAILURE",
                            &e.to_string(),
                        );
                    }
                }
                result.guarantee = Guarantee::Computed;
            }
            Intent::JsonObjectProject { document, fields } => {
                let report = match ports::json_object_project(document, fields) {
                    Ok(report) => report,
                    Err(error) => {
                        let code = match &error {
                            minidora_domain::Error::Contract { code, .. } => code.as_str(),
                            _ => "JSON_OBJECT_PROJECT_UNSUPPORTED",
                        };
                        return self.stop(&mut state, Lifecycle::Suspend, code, &error.to_string());
                    }
                };
                if report.json.len() > request.budget.max_evidence_bytes {
                    return self.stop(
                        &mut state,
                        Lifecycle::Suspend,
                        "RESOURCE_LIMIT",
                        "JSON object projection exceeds the request output budget",
                    );
                }
                let artifact = self.store.put_artifact(actor, &request.case_id, &report)?;
                result.dependencies.push(artifact);
                units.push(report.json);
                result.guarantee = Guarantee::Computed;
            }
            Intent::JsonStatistics { document, pointer } => {
                let report = match ports::json_pointer_statistics(document, pointer) {
                    Ok(report) => report,
                    Err(error) => {
                        let code = match &error {
                            minidora_domain::Error::Contract { code, .. } => code.as_str(),
                            _ => "JSON_STATISTICS_UNSUPPORTED",
                        };
                        return self.stop(&mut state, Lifecycle::Suspend, code, &error.to_string());
                    }
                };
                let text = serde_json::to_string_pretty(&report)?;
                if text.len() > request.budget.max_evidence_bytes {
                    return self.stop(
                        &mut state,
                        Lifecycle::Suspend,
                        "RESOURCE_LIMIT",
                        "JSON statistics report exceeds the request output budget",
                    );
                }
                let artifact = self.store.put_artifact(actor, &request.case_id, &report)?;
                result.dependencies.push(artifact);
                units.push(text);
                result.guarantee = Guarantee::Computed;
            }
            Intent::JsonCompare { document } => {
                let report = match ports::json_compare(document) {
                    Ok(report) => report,
                    Err(error) => {
                        let code = match &error {
                            minidora_domain::Error::Contract { code, .. } => code.as_str(),
                            _ => "JSON_COMPARE_UNSUPPORTED",
                        };
                        return self.stop(&mut state, Lifecycle::Suspend, code, &error.to_string());
                    }
                };
                let text = serde_json::to_string_pretty(&report)?;
                if text.len() > request.budget.max_evidence_bytes {
                    return self.stop(
                        &mut state,
                        Lifecycle::Suspend,
                        "RESOURCE_LIMIT",
                        "JSON comparison report exceeds the request output budget",
                    );
                }
                let artifact = self.store.put_artifact(actor, &request.case_id, &report)?;
                result.dependencies.push(artifact);
                units.push(text);
                result.guarantee = Guarantee::Computed;
            }
            Intent::JsonTableCompare {
                document,
                key_column,
            } => {
                let report = match ports::json_table_compare(document, key_column) {
                    Ok(report) => report,
                    Err(error) => {
                        let code = match &error {
                            minidora_domain::Error::Contract { code, .. } => code.as_str(),
                            _ => "JSON_TABLE_COMPARE_UNSUPPORTED",
                        };
                        return self.stop(&mut state, Lifecycle::Suspend, code, &error.to_string());
                    }
                };
                let text = serde_json::to_string_pretty(&report)?;
                if text.len() > request.budget.max_evidence_bytes {
                    return self.stop(
                        &mut state,
                        Lifecycle::Suspend,
                        "RESOURCE_LIMIT",
                        "JSON table comparison report exceeds the request output budget",
                    );
                }
                let artifact = self.store.put_artifact(actor, &request.case_id, &report)?;
                result.dependencies.push(artifact);
                units.push(text);
                result.guarantee = Guarantee::Computed;
            }
            Intent::JsonTableFilter {
                document,
                column,
                equals,
            } => {
                let report = match ports::json_table_filter(document, column, equals) {
                    Ok(report) => report,
                    Err(error) => {
                        let code = match &error {
                            minidora_domain::Error::Contract { code, .. } => code.as_str(),
                            _ => "JSON_TABLE_FILTER_UNSUPPORTED",
                        };
                        return self.stop(&mut state, Lifecycle::Suspend, code, &error.to_string());
                    }
                };
                let text = serde_json::to_string_pretty(&report)?;
                if text.len() > request.budget.max_evidence_bytes {
                    return self.stop(
                        &mut state,
                        Lifecycle::Suspend,
                        "RESOURCE_LIMIT",
                        "JSON table filter report exceeds the request output budget",
                    );
                }
                let artifact = self.store.put_artifact(actor, &request.case_id, &report)?;
                result.dependencies.push(artifact);
                units.push(text);
                result.guarantee = Guarantee::Computed;
            }
            Intent::JsonTableSort {
                document,
                column,
                descending,
            } => {
                let report = match ports::json_table_sort(document, column, *descending) {
                    Ok(report) => report,
                    Err(error) => {
                        let code = match &error {
                            minidora_domain::Error::Contract { code, .. } => code.as_str(),
                            _ => "JSON_TABLE_SORT_UNSUPPORTED",
                        };
                        return self.stop(&mut state, Lifecycle::Suspend, code, &error.to_string());
                    }
                };
                let text = serde_json::to_string_pretty(&report)?;
                if text.len() > request.budget.max_evidence_bytes {
                    return self.stop(
                        &mut state,
                        Lifecycle::Suspend,
                        "RESOURCE_LIMIT",
                        "JSON table sort report exceeds the request output budget",
                    );
                }
                let artifact = self.store.put_artifact(actor, &request.case_id, &report)?;
                result.dependencies.push(artifact);
                units.push(text);
                result.guarantee = Guarantee::Computed;
            }
            Intent::JsonTableGroupCount { document, column } => {
                let report = match ports::json_table_group_count(document, column) {
                    Ok(report) => report,
                    Err(error) => {
                        let code = match &error {
                            minidora_domain::Error::Contract { code, .. } => code.as_str(),
                            _ => "JSON_TABLE_COUNT_UNSUPPORTED",
                        };
                        return self.stop(&mut state, Lifecycle::Suspend, code, &error.to_string());
                    }
                };
                let text = serde_json::to_string_pretty(&report)?;
                if text.len() > request.budget.max_evidence_bytes {
                    return self.stop(
                        &mut state,
                        Lifecycle::Suspend,
                        "RESOURCE_LIMIT",
                        "JSON table group-count report exceeds the request output budget",
                    );
                }
                let artifact = self.store.put_artifact(actor, &request.case_id, &report)?;
                result.dependencies.push(artifact);
                units.push(text);
                result.guarantee = Guarantee::Computed;
            }
            Intent::JsonTableSelect { document, column } => {
                let report = match ports::json_table_select(document, column) {
                    Ok(report) => report,
                    Err(error) => {
                        let code = match &error {
                            minidora_domain::Error::Contract { code, .. } => code.as_str(),
                            _ => "JSON_TABLE_SELECT_UNSUPPORTED",
                        };
                        return self.stop(&mut state, Lifecycle::Suspend, code, &error.to_string());
                    }
                };
                let text = serde_json::to_string_pretty(&report)?;
                if text.len() > request.budget.max_evidence_bytes {
                    return self.stop(
                        &mut state,
                        Lifecycle::Suspend,
                        "RESOURCE_LIMIT",
                        "JSON table select report exceeds the request output budget",
                    );
                }
                let artifact = self.store.put_artifact(actor, &request.case_id, &report)?;
                result.dependencies.push(artifact);
                units.push(text);
                result.guarantee = Guarantee::Computed;
            }
            Intent::JsonTableProject { document, columns } => {
                let report = match ports::json_table_project(document, columns) {
                    Ok(report) => report,
                    Err(error) => {
                        let code = match &error {
                            minidora_domain::Error::Contract { code, .. } => code.as_str(),
                            _ => "JSON_TABLE_PROJECT_UNSUPPORTED",
                        };
                        return self.stop(&mut state, Lifecycle::Suspend, code, &error.to_string());
                    }
                };
                let text = serde_json::to_string_pretty(&report)?;
                if text.len() > request.budget.max_evidence_bytes {
                    return self.stop(
                        &mut state,
                        Lifecycle::Suspend,
                        "RESOURCE_LIMIT",
                        "JSON table projection report exceeds the request output budget",
                    );
                }
                let artifact = self.store.put_artifact(actor, &request.case_id, &report)?;
                result.dependencies.push(artifact);
                units.push(text);
                result.guarantee = Guarantee::Computed;
            }
            Intent::EvidenceSynthesize { document } => {
                let report = match ports::synthesize_explicit_evidence(document) {
                    Ok(report) => report,
                    Err(error) => {
                        let code = match &error {
                            minidora_domain::Error::Contract { code, .. } => code.as_str(),
                            _ => "EVIDENCE_SYNTHESIS_UNSUPPORTED",
                        };
                        return self.stop(&mut state, Lifecycle::Suspend, code, &error.to_string());
                    }
                };
                let text = serde_json::to_string_pretty(&report)?;
                if text.len() > request.budget.max_evidence_bytes {
                    return self.stop(
                        &mut state,
                        Lifecycle::Suspend,
                        "RESOURCE_LIMIT",
                        "Evidence synthesis report exceeds the request output budget",
                    );
                }
                let artifact = self.store.put_artifact(actor, &request.case_id, &report)?;
                result.dependencies.push(artifact);
                units.push(text);
                result.guarantee = Guarantee::Computed;
            }
            Intent::MeaningCompare { a, b } => match crate::meaning_compare::compare(a, b) {
                Ok(report) => {
                    units.push(report);
                    result.guarantee = Guarantee::Computed;
                }
                Err(error) => {
                    return self.stop(
                        &mut state,
                        Lifecycle::Suspend,
                        "MEANING_COMPARE_UNRESOLVED",
                        &error.to_string(),
                    );
                }
            },
            Intent::Unknown { .. } => {
                return self.stop(
                    &mut state,
                    Lifecycle::Suspend,
                    "CAPABILITY_UNAVAILABLE",
                    "この版では入力の意味を確定できず、処理と回答を保留しました。正確な要件を保持していますが、実行や外部作用は行っていません。",
                );
            }
            Intent::Cancel => unreachable!("handled before computation"),
        }
        if started.elapsed().as_millis() > u128::from(request.budget.deadline_ms) {
            return self.stop(
                &mut state,
                Lifecycle::Suspend,
                "DEADLINE",
                "採用前に締切へ達しました。",
            );
        }
        state.result = Some(result.clone());
        let compute_artifact = self.store.put_artifact(
            actor,
            &request.case_id,
            &(
                &result.calculation,
                &units,
                &result.evidence,
                &result.dependencies,
            ),
        )?;
        let source_refs = graph
            .nodes()
            .iter()
            .find(|n| n.id == "compute")
            .map(|n| {
                n.requires
                    .iter()
                    .map(|id| (id.clone(), graph.observed()[id].clone()))
                    .collect()
            })
            .unwrap_or_default();
        graph.observe("compute", &compute_artifact, &source_refs)?;
        self.advance(
            &mut state,
            Lifecycle::Running,
            Some("ACTUAL_COMPUTATION_OBSERVED".into()),
        )?;
        let deadline = started
            .checked_add(std::time::Duration::from_millis(request.budget.deadline_ms))
            .ok_or_else(|| {
                failure(
                    "INVALID_DEADLINE",
                    "execution deadline overflows this clock",
                )
            })?;
        let model = match language::SequenceModel::train_until(&units, 2, 1, deadline) {
            Ok(model) => model,
            Err(error) => {
                return self.stop(
                    &mut state,
                    Lifecycle::Suspend,
                    "LANGUAGE_RESOURCE_OR_COVERAGE",
                    &error.to_string(),
                );
            }
        };
        let emission = match model.realize_supported_until(
            &[units.join("\n")],
            request.budget.max_evidence_bytes,
            deadline,
        ) {
            Ok(value) => value,
            Err(e) => {
                return self.stop(
                    &mut state,
                    Lifecycle::Suspend,
                    "LANGUAGE_RESOURCE_OR_COVERAGE",
                    &e.to_string(),
                );
            }
        };
        let model_artifact_ref = self.store.put_artifact(actor, &request.case_id, &model)?;
        let model_observation_ref = self
            .store
            .put_artifact(actor, &request.case_id, &emission)?;
        let mut plan = ContentPlan {
            id: String::new(),
            text: emission.text,
            locale: request.locale.clone(),
            guarantee: result.guarantee.clone(),
            required_units: units,
            dependencies: result.dependencies.clone(),
            model_revision: emission.model_revision,
            model_artifact_ref,
            model_observation_ref,
        };
        plan.dependencies.push(compute_artifact.clone());
        plan.id = identity(&plan)?;
        let content_artifact = self.store.put_artifact(actor, &request.case_id, &plan)?;
        graph.observe(
            "content",
            &content_artifact,
            &BTreeMap::from([("compute".into(), compute_artifact)]),
        )?;
        result.text = plan.text.clone();
        result.content_plan = Some(plan.clone());
        result.lifecycle = Lifecycle::Adopted;
        state.result = Some(result.clone());
        self.advance(&mut state, Lifecycle::Adopted, None)?;
        let render = output::render(&plan)?;
        if !validate_render(&plan, &render) {
            return self.stop(
                &mut state,
                Lifecycle::Fail,
                "RENDER_SEMANTIC_MISMATCH",
                "出力の実バイトがCoreの採用内容と一致しません。",
            );
        }
        let render_artifact = self.store.put_artifact(actor, &request.case_id, &render)?;
        graph.observe(
            "render",
            &render_artifact,
            &BTreeMap::from([("content".into(), content_artifact)]),
        )?;
        result.render = Some(render.clone());
        state.result = Some(result.clone());
        self.advance(
            &mut state,
            Lifecycle::Running,
            Some("ACTUAL_RENDER_VERIFIED".into()),
        )?;
        self.advance(&mut state, Lifecycle::Adopted, None)?;
        // Required file effect is performed once under a durable, fully bound
        // identity. Physical output never independently closes the case.
        if let Some(ref path) = request.output_path {
            if !graph.ready().iter().any(|id| id == "file") {
                return self.stop(
                    &mut state,
                    Lifecycle::Suspend,
                    "ACTION_BUDGET_EXHAUSTED",
                    "ファイル作用を実行する予算がありません。",
                );
            }
            if !request.permissions.write_paths.contains(path) {
                return self.stop(
                    &mut state,
                    Lifecycle::Suspend,
                    "WRITE_PERMISSION_MISSING",
                    "指定ファイルへの書込権限がありません。",
                );
            }
            let action_id = identity(&(
                &request.case_id,
                &fingerprint,
                "file",
                path,
                &render.actual_sha256,
            ))?;
            let auth = self
                .store
                .put_artifact(actor, &request.case_id, &request.permissions)?;
            let exact = self
                .store
                .put_bytes(actor, &request.case_id, &render.bytes)?;
            let mut action = ActionRecord {
                case_id: request.case_id.clone(),
                actor_id: actor.into(),
                action_id: action_id.clone(),
                input_fingerprint: fingerprint,
                idempotency_key: action_id,
                effect_class: "LOCAL_WRITE".into(),
                target: path.to_string_lossy().into_owned(),
                authorization_sha256: auth,
                exact_request_sha256: exact,
                phase: EffectPhase::Proposed,
                receipt: None,
            };
            self.store.save_action(actor, &action)?;
            for phase in [
                EffectPhase::Authorized,
                EffectPhase::Prepared,
                EffectPhase::Attempted,
            ] {
                action.phase = phase;
                self.store.save_action(actor, &action)?;
            }
            let receipt = match output::write_file(&action, &render.bytes) {
                Ok(receipt) => receipt,
                Err(e) => DeliveryReceipt {
                    action_id: action.action_id.clone(),
                    target: action.target.clone(),
                    content_sha256: render.actual_sha256.clone(),
                    phase: EffectPhase::Failed,
                    observed_bytes: None,
                    reason: Some(e.to_string()),
                },
            };
            action.phase = receipt.phase;
            action.receipt = Some(receipt.clone());
            self.store.save_action(actor, &action)?;
            result.receipts.push(receipt.clone());
            state.result = Some(result.clone());
            if receipt.phase != EffectPhase::Confirmed {
                return self.stop(
                    &mut state,
                    Lifecycle::Suspend,
                    "REQUIRED_EFFECT_UNCONFIRMED",
                    "内容は採用済みですが、要求されたファイル効果は未確認です。",
                );
            }
            if receipt.action_id != action.action_id
                || receipt.target != action.target
                || receipt.content_sha256 != render.actual_sha256
                || receipt.observed_bytes != Some(render.bytes.len() as u64)
                || !matches_artifact(path, &render.bytes)
            {
                return self.stop(
                    &mut state,
                    Lifecycle::Fail,
                    "EFFECT_RECEIPT_MISMATCH",
                    "実ファイルと作用受領の整合性を確認できません。",
                );
            }
            let file_artifact = self.store.put_artifact(actor, &request.case_id, &receipt)?;
            graph.observe(
                "file",
                &file_artifact,
                &BTreeMap::from([("render".into(), render_artifact)]),
            )?;
        }
        if !graph.is_complete() {
            return self.stop(
                &mut state,
                Lifecycle::Suspend,
                "UNMET_GOAL_NODES",
                &format!("{:?}", graph.residuals()),
            );
        }
        result.execution_graph_ref = Some(
            self.store.put_artifact(
                actor,
                &request.case_id,
                &(
                    graph.nodes(),
                    graph.observed(),
                    graph.consumed_by("content"),
                    graph.consumed_by("render"),
                    graph.consumed_by("file"),
                    graph
                        .nodes()
                        .iter()
                        .map(|node| (node.id.clone(), graph.consumed_by(&node.id).cloned()))
                        .collect::<BTreeMap<_, _>>(),
                ),
            )?,
        );
        result.lifecycle = Lifecycle::Commit;
        state.result = Some(result.clone());
        self.advance(&mut state, Lifecycle::Commit, None)?;
        Ok(result)
    }
    fn advance(
        &self,
        state: &mut CaseRevision,
        next: Lifecycle,
        reason: Option<String>,
    ) -> Result<()> {
        if !state.lifecycle.permits(next) {
            return Err(failure(
                "INVALID_TRANSITION",
                format!("{:?} -> {:?}", state.lifecycle, next),
            ));
        }
        let mut updated = state.clone();
        updated.parent_revision = Some(state.revision);
        updated.revision += 1;
        updated.lifecycle = next;
        updated.reason = reason;
        if let Some(ref mut result) = updated.result {
            result.lifecycle = next;
        }
        self.store.append_case(&state.actor_id, &updated)?;
        *state = updated;
        Ok(())
    }
    fn stop(
        &self,
        state: &mut CaseRevision,
        lifecycle: Lifecycle,
        code: &str,
        message: &str,
    ) -> Result<ResultRecord> {
        let request = state.bundle.as_ref().map(|b| b.request.clone());
        let mut result = state.result.take().unwrap_or_else(|| {
            request.as_ref().map(base_result).unwrap_or_else(|| {
                let req = RequestEnvelope::new(&state.case_id, "unsealed", &state.actor_id, "");
                base_result(&req)
            })
        });
        result.lifecycle = lifecycle;
        result.reason = Some(format!("{code}: {message}"));
        if result.text.is_empty() {
            result.text = message.into();
        }
        state.result = Some(result.clone());
        self.advance(state, lifecycle, result.reason.clone())?;
        Ok(result)
    }
}
fn base_result(request: &RequestEnvelope) -> ResultRecord {
    ResultRecord {
        id: identity(request).unwrap_or_default(),
        input_id: request.input_id.clone(),
        input_version: request.input_version,
        lifecycle: Lifecycle::Received,
        text: String::new(),
        reason: None,
        guarantee: Guarantee::Unknown,
        calculation: None,
        content_plan: None,
        render: None,
        receipts: Vec::new(),
        evidence: Vec::new(),
        dependencies: Vec::new(),
        execution_graph_ref: None,
    }
}
fn rejected(request: &RequestEnvelope, code: &str, message: &str) -> ResultRecord {
    let mut r = base_result(request);
    r.lifecycle = Lifecycle::Reject;
    r.reason = Some(format!("{code}: {message}"));
    r.text = message.into();
    r
}
fn describe(p: &Proposition) -> String {
    if p.predicate == "is" {
        format!(
            "{}は{}{}",
            p.subject,
            p.object,
            if p.negative {
                "ではない"
            } else {
                "である"
            }
        )
    } else {
        format!(
            "{} / {} / {} / {}",
            p.subject,
            p.predicate,
            p.object,
            if p.negative { "否定" } else { "肯定" }
        )
    }
}
/// This verifier covers exact UTF-8 rendering of an already adopted surface,
/// not arbitrary paraphrase equivalence or external-world truth.
pub fn validate_render(plan: &ContentPlan, render: &RenderObservation) -> bool {
    render.status == "RENDERED"
        && render.plan_id == plan.id
        && render.bytes == plan.text.as_bytes()
        && render.actual_sha256 == digest(&render.bytes)
}

fn matches_artifact(path: &Path, expected: &[u8]) -> bool {
    use std::io::Read;
    let checked = (|| -> std::io::Result<bool> {
        let metadata = std::fs::symlink_metadata(path)?;
        if !metadata.is_file() || metadata.len() != expected.len() as u64 {
            return Ok(false);
        }
        let mut bytes = Vec::with_capacity(expected.len());
        std::fs::File::open(path)?
            .take(expected.len() as u64 + 1)
            .read_to_end(&mut bytes)?;
        Ok(bytes == expected)
    })();
    checked.unwrap_or(false)
}
