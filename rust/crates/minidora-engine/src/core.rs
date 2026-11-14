use crate::scheduler::{GoalNode, GoalNodeKind, Scheduler};
use crate::{input, language, output, ports, relations, store::Store};
use minidora_domain::*;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::time::{Duration, Instant};

#[derive(serde::Serialize)]
struct CalculationSequenceStepArtifact {
    id: String,
    source_expression: SourceSpan,
    evaluated_expression: String,
    calculation: Calculation,
    consumed: Vec<language::CalculationReferenceUse>,
}

const CORE_COMPUTATION_ARTIFACT_KIND: &str = "minidora.core-computation.relation-meaning.v1";

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct CoreComputationArtifact {
    kind: String,
    calculation: Option<Calculation>,
    units: Vec<String>,
    evidence: Vec<EvidencePacket>,
    dependencies: Vec<String>,
    relation_meaning: Proposition,
    relation_proof_dependencies: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum PriorRelationMeaning {
    OriginalQuestion(Proposition),
    AdoptedConclusion {
        proposition: Proposition,
        proof_dependencies: Vec<String>,
    },
}

const COMMON_RELATIONS_REPORT_KIND: &str = "minidora.common-relations-report.v1";
const COMMON_RELATIONS_REPORT_HEADER: &str = "保存済みの前提・規則から、両対象に共通して支持される関係です。列挙は保存記録で確認できた範囲に限られ、網羅的な比較でも現実世界の真偽確認でもありません。";
const RELATION_COMPARISON_REPORT_KIND: &str = "minidora.relation-comparison-report.v1";
const RELATION_COMPARISON_REPORT_HEADER: &str = "保存済みの根拠を比較した記録です。片側のみの表示はその側の保存記録で支持され、他方では同じ関係の支持を確認できなかった意味です。保存記録の網羅性と現実世界の真偽は確認していません。";

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct RelationObjectsAnswer {
    pub(crate) text: String,
    pub(crate) dependencies: Vec<String>,
    pub(crate) guarantee: Guarantee,
}

pub(crate) fn relation_objects_answer(
    memory: &MemoryState,
    subject: &str,
    predicate: &str,
    negative: bool,
    scope: &str,
    max_actions: u32,
    max_evidence_bytes: usize,
) -> Result<RelationObjectsAnswer> {
    let scanned_records = memory
        .facts
        .len()
        .checked_add(memory.rules.len())
        .ok_or_else(|| {
            failure(
                "RESOURCE_LIMIT",
                "relation memory scan count overflowed its bounded representation",
            )
        })?;
    let action_budget = usize::try_from(max_actions).unwrap_or(usize::MAX);
    if scanned_records > action_budget {
        return Err(failure(
            "RESOURCE_LIMIT",
            "保存済み関係を候補として確認する走査量が要求の推論予算を超えました。",
        ));
    }
    let remaining_actions =
        max_actions.saturating_sub(u32::try_from(scanned_records).unwrap_or(u32::MAX));
    let mut candidates = BTreeSet::new();
    for fact in &memory.facts {
        if !fact.valid || matches!(fact.guarantee, Guarantee::Hypothesis | Guarantee::Unknown) {
            continue;
        }
        if fact.proposition.scope == scope
            && !fact.proposition.object.starts_with('$')
            && !fact.proposition.object.starts_with('?')
        {
            candidates.insert(fact.proposition.object.clone());
        }
    }
    for rule in memory.rules.iter().filter(|rule| rule.valid) {
        for proposition in std::iter::once(&rule.premise)
            .chain(rule.additional_premises.iter())
            .chain(std::iter::once(&rule.conclusion))
        {
            if proposition.scope == scope
                && !proposition.object.starts_with('$')
                && !proposition.object.starts_with('?')
            {
                candidates.insert(proposition.object.clone());
            }
        }
    }
    if candidates.is_empty() {
        return Err(failure(
            "MISSING_PREMISES",
            "同じ主体・scopeで確認できる関係値が保存されていません。",
        ));
    }
    let max_candidates = usize::try_from(remaining_actions).unwrap_or(usize::MAX);
    if candidates.len() > max_candidates {
        return Err(failure(
            "RESOURCE_LIMIT",
            "候補となる関係値が要求の推論予算を超えました。",
        ));
    }
    let candidate_count = u32::try_from(candidates.len()).map_err(|_| {
        failure(
            "RESOURCE_LIMIT",
            "candidate count exceeds the bounded relation query representation",
        )
    })?;
    let per_candidate_budget = remaining_actions / candidate_count;
    if per_candidate_budget == 0 {
        return Err(failure(
            "RESOURCE_LIMIT",
            "すべての候補関係を確認する推論予算がありません。",
        ));
    }

    let mut supported = BTreeMap::new();
    let mut dependencies = BTreeSet::new();
    let mut any_derived = false;
    for object in candidates {
        let proposition = Proposition {
            subject: subject.into(),
            predicate: predicate.into(),
            object,
            negative,
            scope: scope.into(),
        };
        let answer = relations::query(memory, &proposition, per_candidate_budget)?;
        match answer.status.as_str() {
            "SUPPORTED" => {
                let guarantee = relations::answer_guarantee(&answer);
                supported.insert(proposition, guarantee.clone());
                dependencies.extend(relations::answer_dependencies(&answer));
                any_derived |= guarantee == Guarantee::Derived;
            }
            "CONFLICT" => {
                return Err(failure(
                    "CONFLICTING_PREMISES",
                    format!(
                        "範囲「{scope}」の「{subject}」について、「{predicate}」関係に肯定と否定の根拠があります。選択せず停止します。"
                    ),
                ));
            }
            "CONTRADICTED" | "UNKNOWN" | "MISSING_PREMISES" => {}
            _ => {
                return Err(failure(
                    "UNSUPPORTED_RELATION_RESULT",
                    "関係値の照合が既知の終端状態を返しませんでした。",
                ));
            }
        }
    }
    if supported.is_empty() {
        return Err(failure(
            "MISSING_PREMISES",
            format!(
                "範囲「{scope}」で「{subject}」の「{predicate}」関係を支える有効な前提または導出がありません。"
            ),
        ));
    }
    let descriptions = supported
        .iter()
        .map(|(proposition, guarantee)| {
            let origin = match guarantee {
                Guarantee::UserPremise => "直接提示",
                Guarantee::Derived => "規則導出",
                _ => "根拠付き",
            };
            format!(
                "{origin}: {}（範囲: {}）",
                describe(proposition),
                proposition.scope
            )
        })
        .collect::<Vec<_>>()
        .join("；");
    let text = format!(
        "範囲「{scope}」の保存済み前提では、次の関係が支持されます: {descriptions}。保存済みの根拠にある対象のみを示し、対象全体の網羅性や現実世界の真偽は確認していません。"
    );
    if text.len() > max_evidence_bytes {
        return Err(failure(
            "RESOURCE_LIMIT",
            "関係回答が要求の証拠・出力予算を超えました。",
        ));
    }
    Ok(RelationObjectsAnswer {
        text,
        dependencies: dependencies.into_iter().collect(),
        guarantee: if any_derived {
            Guarantee::Derived
        } else {
            Guarantee::UserPremise
        },
    })
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct CommonRelationsReportArtifact {
    kind: String,
    result_id: String,
    report_sha256: String,
    subject_a: String,
    subject_b: String,
    scope: String,
    shared_proofs: Vec<CommonRelationProofArtifact>,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct CommonRelationProofArtifact {
    proposition_a: Proposition,
    proposition_b: Proposition,
    guarantee_a: Guarantee,
    guarantee_b: Guarantee,
    dependencies_a: Vec<String>,
    dependencies_b: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct RelationComparisonReportArtifact {
    kind: String,
    result_id: String,
    report_sha256: String,
    subject_a: String,
    subject_b: String,
    scope: String,
    inventory_a: relations::RelationInventory,
    inventory_b: relations::RelationInventory,
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
        let source_followup_indices = if matches!(
            bundle.meaning.intent,
            Intent::SummarizePreviousSource | Intent::SummarizePreviousSourceConstrained { .. }
        ) {
            previous
                .filter(|prior| prior.lifecycle == Lifecycle::Commit)
                .map(|prior| {
                    prior
                        .evidence
                        .iter()
                        .enumerate()
                        .filter(|(_, packet)| {
                            packet.packet_kind == "FETCHED_BODY"
                                && packet.failure.is_none()
                                && packet.extracted_text.is_some()
                        })
                        .map(|(index, _)| index)
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default()
        } else {
            Vec::new()
        };
        let previous_csv_statistics_explanation =
            if matches!(bundle.meaning.intent, Intent::ExplainPrevious) {
                match previous.filter(|prior| prior.lifecycle == Lifecycle::Commit) {
                    Some(prior) => crate::document::previous_csv_statistics_explanation(
                        &self.store,
                        actor,
                        &request.case_id,
                        prior,
                    )?,
                    None => None,
                }
            } else {
                None
            };
        let previous_json_table_group_statistics_explanation =
            if matches!(bundle.meaning.intent, Intent::ExplainPrevious) {
                match previous.filter(|prior| prior.lifecycle == Lifecycle::Commit) {
                    Some(prior) => {
                        crate::document::previous_json_table_group_statistics_explanation(
                            &self.store,
                            actor,
                            &request.case_id,
                            prior,
                        )?
                    }
                    None => None,
                }
            } else {
                None
            };
        let previous_json_table_operation_explanation =
            if matches!(bundle.meaning.intent, Intent::ExplainPrevious) {
                match previous.filter(|prior| prior.lifecycle == Lifecycle::Commit) {
                    Some(prior) => {
                        match crate::document::previous_json_table_operation_explanation(
                            &self.store,
                            actor,
                            &request.case_id,
                            prior,
                            request.budget.max_evidence_bytes,
                        ) {
                            Ok(value) => value,
                            Err(error) => {
                                let code = match &error {
                                    Error::Contract { code, .. } => code.as_str(),
                                    _ => "JSON_TABLE_CONTEXT_INVALID",
                                };
                                return self.stop(
                                    &mut state,
                                    Lifecycle::Suspend,
                                    code,
                                    &error.to_string(),
                                );
                            }
                        }
                    }
                    None => None,
                }
            } else {
                None
            };
        let previous_csv_table_comparison_explanation =
            if matches!(bundle.meaning.intent, Intent::ExplainPrevious) {
                match previous.filter(|prior| prior.lifecycle == Lifecycle::Commit) {
                    Some(prior) => {
                        match crate::document::previous_csv_table_comparison_explanation(
                            &self.store,
                            actor,
                            &request.case_id,
                            prior,
                            request.budget.max_evidence_bytes,
                        ) {
                            Ok(value) => value,
                            Err(error) => {
                                let code = match &error {
                                    Error::Contract { code, .. } => code.as_str(),
                                    _ => "CSV_TABLE_CONTEXT_INVALID",
                                };
                                return self.stop(
                                    &mut state,
                                    Lifecycle::Suspend,
                                    code,
                                    &error.to_string(),
                                );
                            }
                        }
                    }
                    None => None,
                }
            } else {
                None
            };
        let previous_json_group_mean_answer =
            if matches!(bundle.meaning.intent, Intent::AskPreviousGroupMean) {
                match previous.filter(|prior| prior.lifecycle == Lifecycle::Commit) {
                    Some(prior) => match crate::document::previous_json_table_highest_group_mean(
                        &self.store,
                        actor,
                        &request.case_id,
                        prior,
                    ) {
                        Ok(value) => value,
                        Err(error) => {
                            let code = match &error {
                                Error::Contract { code, .. } => code.as_str(),
                                _ => "JSON_GROUP_MEAN_UNSUPPORTED",
                            };
                            return self.stop(
                                &mut state,
                                Lifecycle::Suspend,
                                code,
                                &error.to_string(),
                            );
                        }
                    },
                    None => None,
                }
            } else {
                None
            };
        let previous_source_question_answer =
            if let Intent::AskPreviousSourceQuestion { query } = &bundle.meaning.intent {
                match previous.filter(|prior| prior.lifecycle == Lifecycle::Commit) {
                    Some(prior) => {
                        let mut source_packets = Vec::new();
                        for (index, packet) in
                            prior.evidence.iter().enumerate().filter(|(_, packet)| {
                                packet.packet_kind == "FETCHED_BODY"
                                    && packet.failure.is_none()
                                    && packet.extracted_text.is_some()
                            })
                        {
                            let packet_bytes = canonical(packet)?;
                            let packet_ref = digest(&packet_bytes);
                            match self.store.get_bytes(actor, &request.case_id, &packet_ref) {
                                Ok(stored) if stored == packet_bytes => {}
                                Ok(_) => {
                                    return self.stop(
                                        &mut state,
                                        Lifecycle::Fail,
                                        "SOURCE_QUESTION_PACKET_MISMATCH",
                                        "前回の取得本文artifactと保存済みpacketが一致しません。",
                                    );
                                }
                                Err(error) => {
                                    return self.stop(
                                        &mut state,
                                        Lifecycle::Suspend,
                                        "SOURCE_QUESTION_PACKET_UNAVAILABLE",
                                        &error.to_string(),
                                    );
                                }
                            }
                            source_packets.push((index, packet, packet_ref));
                        }
                        if source_packets.is_empty() {
                            return self.stop(
                                &mut state,
                                Lifecycle::Suspend,
                                "SOURCE_BODY_UNAVAILABLE",
                                "前回の採用結果に再検証できる取得本文がありません。",
                            );
                        }
                        match crate::text_summary::answer_fetched_source_question_v2(
                            &prior.id,
                            query,
                            &source_packets,
                        ) {
                            Ok(answer) => Some((
                                prior.id.clone(),
                                source_packets
                                    .iter()
                                    .map(|(index, _, reference)| (*index, reference.clone()))
                                    .collect::<Vec<_>>(),
                                answer,
                            )),
                            Err(error) => {
                                let code = match &error {
                                    Error::Contract { code, .. } => code.as_str(),
                                    _ => "SOURCE_QUESTION_FAILED",
                                };
                                return self.stop(
                                    &mut state,
                                    Lifecycle::Suspend,
                                    code,
                                    &error.to_string(),
                                );
                            }
                        }
                    }
                    None => None,
                }
            } else {
                None
            };
        let previous_text_replacement = if let Intent::ReplacePreviousText { old, new } =
            &bundle.meaning.intent
        {
            let Some(prior) = previous.filter(|prior| prior.lifecycle == Lifecycle::Commit) else {
                return self.stop(
                    &mut state,
                    Lifecycle::Suspend,
                    "MISSING_ACTUAL_RESULT",
                    "置換対象となる直前の採用済み回答がありません。",
                );
            };
            if bundle.meaning.context_result_ref.as_deref() != Some(prior.id.as_str()) {
                return self.stop(
                    &mut state,
                    Lifecycle::Fail,
                    "PREVIOUS_TEXT_CONTEXT_MISMATCH",
                    "解釈時に固定した前回結果と編集対象が一致しません。",
                );
            }
            let (content_ref, render_ref, artifact) =
                match crate::document::previous_text_replacement(prior, old, new) {
                    Ok(value) => value,
                    Err(error) => {
                        let code = match &error {
                            Error::Contract { code, .. } => code.as_str(),
                            _ => "PREVIOUS_TEXT_REPLACEMENT_FAILED",
                        };
                        let lifecycle = if code == "PREVIOUS_OUTPUT_INTEGRITY" {
                            Lifecycle::Fail
                        } else {
                            Lifecycle::Suspend
                        };
                        return self.stop(&mut state, lifecycle, code, &error.to_string());
                    }
                };
            for (reference, expected) in [
                (
                    content_ref.as_str(),
                    canonical(prior.content_plan.as_ref().expect("validated content plan"))?,
                ),
                (
                    render_ref.as_str(),
                    canonical(prior.render.as_ref().expect("validated render"))?,
                ),
            ] {
                match self
                    .store
                    .get_bytes_if_present(actor, &request.case_id, reference)?
                {
                    Some(stored) if stored == expected => {}
                    Some(_) => {
                        return self.stop(
                                &mut state,
                                Lifecycle::Fail,
                                "PREVIOUS_OUTPUT_INTEGRITY",
                                "前回の保存ContentPlanまたはrender artifactが保持済み実体と一致しません。",
                            );
                    }
                    None => {
                        return self.stop(
                            &mut state,
                            Lifecycle::Suspend,
                            "PREVIOUS_OUTPUT_UNAVAILABLE",
                            "前回の保存ContentPlanまたはrender artifactを取得できません。",
                        );
                    }
                }
            }
            let artifact_bytes = canonical(&artifact)?;
            let output_units = crate::document::previous_text_replacement_lines(&artifact);
            if artifact_bytes.len() > request.budget.max_evidence_bytes
                || output_units.join("\n").len() > request.budget.max_evidence_bytes
            {
                return self.stop(
                    &mut state,
                    Lifecycle::Suspend,
                    "RESOURCE_LIMIT",
                    "前回回答の編集artifactまたは出力が要求の証拠予算を超えました。",
                );
            }
            Some((prior.id.clone(), content_ref, render_ref, artifact))
        } else {
            None
        };
        let previous_result_summary = if let Intent::SummarizePreviousResult {
            max_sentences,
            max_chars,
        } = &bundle.meaning.intent
        {
            let Some(prior) = previous.filter(|prior| prior.lifecycle == Lifecycle::Commit) else {
                return self.stop(
                    &mut state,
                    Lifecycle::Suspend,
                    "MISSING_ACTUAL_RESULT",
                    "要約対象となる実際の直前結果がありません。",
                );
            };
            if bundle.meaning.context_result_ref.as_deref() != Some(prior.id.as_str()) {
                return self.stop(
                    &mut state,
                    Lifecycle::Suspend,
                    "MISSING_RESULT_SUMMARY_CONTEXT",
                    "要約対象として固定された直前結果がありません。",
                );
            }
            let (Some(plan), Some(render)) = (&prior.content_plan, &prior.render) else {
                return self.stop(
                    &mut state,
                    Lifecycle::Suspend,
                    "PREVIOUS_RESULT_UNSUMMARIZABLE",
                    "前回結果に採用本文または実表示記録がありません。",
                );
            };
            let mut unsigned_plan = plan.clone();
            unsigned_plan.id.clear();
            if plan.id != identity(&unsigned_plan)?
                || plan.text != prior.text
                || !validate_render(plan, render)
            {
                return self.stop(
                    &mut state,
                    Lifecycle::Fail,
                    "PREVIOUS_RESULT_INTEGRITY",
                    "前回の採用本文と実表示記録が一致しません。",
                );
            }
            let content_bytes = canonical(plan)?;
            let content_ref = digest(&content_bytes);
            let render_bytes = canonical(render)?;
            let render_ref = digest(&render_bytes);
            for (reference, expected) in [
                (content_ref.as_str(), content_bytes.as_slice()),
                (render_ref.as_str(), render_bytes.as_slice()),
            ] {
                match self
                    .store
                    .get_bytes_if_present(actor, &request.case_id, reference)?
                {
                    Some(stored) if stored == expected => {}
                    Some(_) => {
                        return self.stop(
                            &mut state,
                            Lifecycle::Fail,
                            "PREVIOUS_RESULT_INTEGRITY",
                            "前回の本文またはrender artifactが保存済み実体と一致しません。",
                        );
                    }
                    None => {
                        return self.stop(
                            &mut state,
                            Lifecycle::Suspend,
                            "PREVIOUS_RESULT_ARTIFACT_UNAVAILABLE",
                            "前回の本文またはrender artifactを同一ケースから取得できません。",
                        );
                    }
                }
            }
            let artifact = match crate::text_summary::summarize_previous_result_with_limits(
                &identity(&request)?,
                &prior.id,
                &plan.id,
                &render.actual_sha256,
                &prior.text,
                max_sentences.unwrap_or(3),
                *max_chars,
            ) {
                Ok(value) => value,
                Err(error) => {
                    let code = match &error {
                        Error::Contract { code, .. } => code.as_str(),
                        _ => "RESULT_SUMMARY_FAILED",
                    };
                    return self.stop(&mut state, Lifecycle::Suspend, code, &error.to_string());
                }
            };
            if artifact.summary.sentence_count
                > usize::try_from(request.budget.max_actions).unwrap_or(usize::MAX)
            {
                return self.stop(
                    &mut state,
                    Lifecycle::Suspend,
                    "RESOURCE_LIMIT",
                    "前回結果の候補文数が今回の推論予算を超えています。",
                );
            }
            let output_unit =
                crate::text_summary::previous_result_summary_unit(&prior.id, &artifact.summary);
            let artifact_bytes = canonical(&artifact)?;
            if artifact_bytes.len().saturating_add(output_unit.len())
                > request.budget.max_evidence_bytes
            {
                return self.stop(
                    &mut state,
                    Lifecycle::Suspend,
                    "RESOURCE_LIMIT",
                    "前回結果の要約artifactと出力が要求の証拠予算を超えました。",
                );
            }
            Some((
                prior.id.clone(),
                content_ref,
                render_ref,
                artifact_bytes,
                output_unit,
            ))
        } else {
            None
        };
        let previous_math_report_explanation =
            if matches!(bundle.meaning.intent, Intent::ExplainPrevious) {
                match previous.filter(|prior| prior.lifecycle == Lifecycle::Commit) {
                    Some(prior) => crate::math::previous_math_report_explanation(prior)?,
                    None => None,
                }
            } else {
                None
            };
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
        if let Intent::FetchPreviousSearchCandidate { rank } = &bundle.meaning.intent {
            let listing_id = "search-candidate-listing".to_string();
            let id = format!("search-candidate-body-{rank}");
            compute_requires.push(id.clone());
            nodes.push(GoalNode {
                id: listing_id.clone(),
                kind: GoalNodeKind::Evidence,
                requires: Vec::new(),
            });
            nodes.push(GoalNode {
                id,
                kind: GoalNodeKind::Evidence,
                requires: vec![listing_id],
            });
        }
        for index in &source_followup_indices {
            let id = format!("followup-source-{index}");
            nodes.push(GoalNode {
                id: id.clone(),
                kind: GoalNodeKind::Evidence,
                requires: Vec::new(),
            });
            compute_requires.push(id);
        }
        if previous_csv_statistics_explanation.is_some() {
            let id = "previous-csv-statistics".to_string();
            nodes.push(GoalNode {
                id: id.clone(),
                kind: GoalNodeKind::Evidence,
                requires: Vec::new(),
            });
            compute_requires.push(id);
        }
        if previous_json_table_group_statistics_explanation.is_some() {
            let source_id = "previous-json-group-statistics-source".to_string();
            let report_id = "previous-json-group-statistics-report".to_string();
            nodes.push(GoalNode {
                id: source_id.clone(),
                kind: GoalNodeKind::Evidence,
                requires: Vec::new(),
            });
            nodes.push(GoalNode {
                id: report_id.clone(),
                kind: GoalNodeKind::ComputedValue,
                requires: vec![source_id],
            });
            compute_requires.push(report_id);
        }
        if previous_json_table_operation_explanation.is_some() {
            let input_id = "previous-json-table-transform-input".to_string();
            let report_id = "previous-json-table-transform-report".to_string();
            nodes.push(GoalNode {
                id: input_id.clone(),
                kind: GoalNodeKind::Evidence,
                requires: Vec::new(),
            });
            nodes.push(GoalNode {
                id: report_id.clone(),
                kind: GoalNodeKind::ComputedValue,
                requires: vec![input_id],
            });
            compute_requires.push(report_id);
        }
        if previous_csv_table_comparison_explanation.is_some() {
            let input_id = "previous-csv-table-comparison-input".to_string();
            let report_id = "previous-csv-table-comparison-report".to_string();
            nodes.push(GoalNode {
                id: input_id.clone(),
                kind: GoalNodeKind::Evidence,
                requires: Vec::new(),
            });
            nodes.push(GoalNode {
                id: report_id.clone(),
                kind: GoalNodeKind::ComputedValue,
                requires: vec![input_id],
            });
            compute_requires.push(report_id);
        }
        if previous_json_group_mean_answer.is_some() {
            let source_id = "previous-json-group-statistics-source".to_string();
            let report_id = "previous-json-group-statistics-report".to_string();
            let answer_id = "previous-json-group-mean-answer".to_string();
            nodes.push(GoalNode {
                id: source_id.clone(),
                kind: GoalNodeKind::Evidence,
                requires: Vec::new(),
            });
            nodes.push(GoalNode {
                id: report_id.clone(),
                kind: GoalNodeKind::ComputedValue,
                requires: vec![source_id],
            });
            nodes.push(GoalNode {
                id: answer_id.clone(),
                kind: GoalNodeKind::ComputedValue,
                requires: vec![report_id],
            });
            compute_requires.push(answer_id);
        }
        if let Some((_, source_packet_refs, _)) = &previous_source_question_answer {
            let mut source_nodes = Vec::new();
            for (index, _) in source_packet_refs {
                let source_id = format!("previous-question-source-{index}");
                nodes.push(GoalNode {
                    id: source_id.clone(),
                    kind: GoalNodeKind::Evidence,
                    requires: Vec::new(),
                });
                source_nodes.push(source_id);
            }
            let answer_id = "previous-source-question-answer".to_string();
            nodes.push(GoalNode {
                id: answer_id.clone(),
                kind: GoalNodeKind::ComputedValue,
                requires: source_nodes,
            });
            compute_requires.push(answer_id);
        }
        if previous_text_replacement.is_some() {
            let content_id = "previous-text-content".to_string();
            let render_id = "previous-text-render".to_string();
            let edit_id = "previous-text-replacement".to_string();
            nodes.push(GoalNode {
                id: content_id.clone(),
                kind: GoalNodeKind::Evidence,
                requires: Vec::new(),
            });
            nodes.push(GoalNode {
                id: render_id.clone(),
                kind: GoalNodeKind::RenderedArtifact,
                requires: vec![content_id],
            });
            nodes.push(GoalNode {
                id: edit_id.clone(),
                kind: GoalNodeKind::ComputedValue,
                requires: vec![
                    "previous-text-content".into(),
                    "previous-text-render".into(),
                ],
            });
            compute_requires.push(edit_id);
        }
        if previous_result_summary.is_some() {
            let content_id = "previous-result-content".to_string();
            let render_id = "previous-result-render".to_string();
            let summary_id = "previous-result-summary".to_string();
            nodes.push(GoalNode {
                id: content_id.clone(),
                kind: GoalNodeKind::Evidence,
                requires: Vec::new(),
            });
            nodes.push(GoalNode {
                id: render_id.clone(),
                kind: GoalNodeKind::RenderedArtifact,
                requires: vec![content_id.clone()],
            });
            nodes.push(GoalNode {
                id: summary_id.clone(),
                kind: GoalNodeKind::ComputedValue,
                requires: vec![content_id, render_id],
            });
            compute_requires.push(summary_id);
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
        let mut approved_surfaces = None;
        let mut adopted_relation_meaning = None;
        let mut relation_proof_dependencies = None;
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
                    let graph_consumed = consumed
                        .into_iter()
                        .map(|(id, sha)| (format!("sequence-{id}"), sha))
                        .collect();
                    graph.observe(&node_id, &artifact_sha256, &graph_consumed)?;
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
            Intent::QuadraticEquation { left, right } => {
                match crate::math::solve_quadratic(left, right) {
                    Ok(solution) => units.push(serde_json::to_string_pretty(&solution)?),
                    Err(error) => {
                        let code = match &error {
                            minidora_domain::Error::Contract { code, .. } => code.as_str(),
                            _ => "QUADRATIC_SOLVE_UNSUPPORTED",
                        };
                        return self.stop(&mut state, Lifecycle::Suspend, code, &error.to_string());
                    }
                }
                result.guarantee = Guarantee::Computed;
            }
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
            Intent::LinearEquation { left, right } => {
                match crate::math::solve_linear_equation(left, right) {
                    Ok(solution) => units.push(serde_json::to_string_pretty(&solution)?),
                    Err(error) => {
                        let code = match &error {
                            minidora_domain::Error::Contract { code, .. } => code.as_str(),
                            _ => "LINEAR_EQUATION_UNSUPPORTED",
                        };
                        return self.stop(&mut state, Lifecycle::Suspend, code, &error.to_string());
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
            Intent::CsvInnerJoin {
                document,
                left_key_column,
                right_key_column,
            } => {
                let value = match crate::document::csv_inner_join(
                    document,
                    left_key_column,
                    right_key_column,
                ) {
                    Ok(value) => value,
                    Err(error) => {
                        let code = match &error {
                            minidora_domain::Error::Contract { code, .. } => code.as_str(),
                            _ => "CSV_INNER_JOIN_UNSUPPORTED",
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
            Intent::ReplacePreviousText { .. } => {
                let Some((prior_id, content_ref, render_ref, artifact)) =
                    &previous_text_replacement
                else {
                    return self.stop(
                        &mut state,
                        Lifecycle::Suspend,
                        "PREVIOUS_OUTPUT_UNAVAILABLE",
                        "編集対象となる前回の回答artifactがありません。",
                    );
                };
                let edit_bytes = canonical(artifact)?;
                let edit_ref = self.store.put_bytes(actor, &request.case_id, &edit_bytes)?;
                graph.observe("previous-text-content", content_ref, &BTreeMap::new())?;
                graph.observe(
                    "previous-text-render",
                    render_ref,
                    &BTreeMap::from([("previous-text-content".into(), content_ref.clone())]),
                )?;
                graph.observe(
                    "previous-text-replacement",
                    &edit_ref,
                    &BTreeMap::from([
                        ("previous-text-content".into(), content_ref.clone()),
                        ("previous-text-render".into(), render_ref.clone()),
                    ]),
                )?;
                result.dependencies.extend([
                    prior_id.clone(),
                    content_ref.clone(),
                    render_ref.clone(),
                    edit_ref,
                ]);
                units.extend(crate::document::previous_text_replacement_lines(artifact));
                result.guarantee = Guarantee::UserPremise;
            }
            Intent::ComposeText { document } => {
                let value = match crate::document::compose_explicit_text(document) {
                    Ok(value) => value,
                    Err(error) => {
                        let code = match &error {
                            minidora_domain::Error::Contract { code, .. } => code.as_str(),
                            _ => "TEXT_COMPOSITION_FAILED",
                        };
                        return self.stop(&mut state, Lifecycle::Suspend, code, &error.to_string());
                    }
                };
                let artifact = self.store.put_artifact(actor, &request.case_id, &value)?;
                result.dependencies.push(artifact);
                units.push(value.text);
                result.guarantee = Guarantee::UserPremise;
            }
            Intent::TextSummaryCandidates { document } => {
                let value = match crate::text_summary::extractive_summary_candidates(document) {
                    Ok(value) => value,
                    Err(error) => {
                        let code = match &error {
                            Error::Contract { code, .. } => code.as_str(),
                            _ => "TEXT_SUMMARY_FAILED",
                        };
                        return self.stop(&mut state, Lifecycle::Suspend, code, &error.to_string());
                    }
                };
                let report_bytes = canonical(&value)?;
                if report_bytes.len() > request.budget.max_evidence_bytes {
                    return self.stop(
                        &mut state,
                        Lifecycle::Suspend,
                        "RESOURCE_LIMIT",
                        "extractive summary candidate artifact exceeds request evidence budget",
                    );
                }
                let artifact = self.store.put_artifact(actor, &request.case_id, &value)?;
                result.dependencies.push(artifact);
                units.push(value.text);
                result.guarantee = Guarantee::Hypothesis;
            }
            Intent::ImportText { document } => {
                let value = match crate::text_editing::import_explicit_text(document) {
                    Ok(value) => value,
                    Err(error) => {
                        let code = match &error {
                            Error::Contract { code, .. } => code.as_str(),
                            _ => "TEXT_IMPORT_FAILED",
                        };
                        return self.stop(&mut state, Lifecycle::Suspend, code, &error.to_string());
                    }
                };
                if canonical(&value)?.len() > request.budget.max_evidence_bytes {
                    return self.stop(
                        &mut state,
                        Lifecycle::Suspend,
                        "RESOURCE_LIMIT",
                        "imported text artifact exceeds request evidence budget",
                    );
                }
                let artifact = self.store.put_artifact(actor, &request.case_id, &value)?;
                result.dependencies.push(artifact);
                units.push(value.text);
                result.guarantee = Guarantee::UserPremise;
            }
            Intent::EditText { document } => {
                let value = match crate::text_editing::edit_explicit_text(
                    document,
                    |sha| {
                        self.store.get_bytes(actor, &request.case_id, sha).map_err(|error| match error {
                        Error::Contract { .. } => failure("TEXT_EDIT_BASE_UNAVAILABLE", "a base artifact is unavailable or unverifiable in this actor/case scope"),
                        other => other,
                    })
                    },
                    started,
                    request.budget.deadline_ms,
                ) {
                    Ok(value) => value,
                    Err(error) => {
                        let code = match &error {
                            Error::Contract { code, .. } => code.as_str(),
                            _ => "TEXT_EDIT_FAILED",
                        };
                        return self.stop(&mut state, Lifecycle::Suspend, code, &error.to_string());
                    }
                };
                if canonical(&value)?.len() > request.budget.max_evidence_bytes {
                    return self.stop(
                        &mut state,
                        Lifecycle::Suspend,
                        "RESOURCE_LIMIT",
                        "edited text artifact exceeds request evidence budget",
                    );
                }
                let artifact = self.store.put_artifact(actor, &request.case_id, &value)?;
                result
                    .dependencies
                    .extend(value.consumed_artifacts.iter().cloned());
                result.dependencies.push(artifact);
                units.push(value.text);
                result.guarantee = Guarantee::UserPremise;
            }
            Intent::LocateTextReplacement { document } => {
                let value = match crate::text_editing::locate_explicit_replacement(
                    document,
                    |sha| {
                        self.store.get_bytes(actor, &request.case_id, sha).map_err(|error| match error {
                        Error::Contract { .. } => failure("TEXT_EDIT_BASE_UNAVAILABLE", "a base artifact is unavailable or unverifiable in this actor/case scope"),
                        other => other,
                    })
                    },
                    started,
                    request.budget.deadline_ms,
                ) {
                    Ok(value) => value,
                    Err(error) => {
                        let code = match &error {
                            Error::Contract { code, .. } => code.as_str(),
                            _ => "TEXT_EDIT_LOCATION_FAILED",
                        };
                        return self.stop(&mut state, Lifecycle::Suspend, code, &error.to_string());
                    }
                };
                let report_bytes = canonical(&value)?;
                if report_bytes.len() > request.budget.max_evidence_bytes {
                    return self.stop(
                        &mut state,
                        Lifecycle::Suspend,
                        "RESOURCE_LIMIT",
                        "text location artifact exceeds request evidence budget",
                    );
                }
                let display = std::str::from_utf8(&report_bytes)
                    .map_err(|error| failure("TEXT_LOCATION_ENCODING", error.to_string()))?
                    .to_owned();
                let artifact = self.store.put_artifact(actor, &request.case_id, &value)?;
                result
                    .dependencies
                    .extend(value.consumed_artifacts.iter().cloned());
                result.dependencies.push(artifact);
                units.push(display);
                result.guarantee = Guarantee::UserPremise;
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
            Intent::FetchPreviousSearchCandidate { rank } => {
                let Some(prior) = previous.filter(|prior| prior.lifecycle == Lifecycle::Commit)
                else {
                    return self.stop(
                        &mut state,
                        Lifecycle::Suspend,
                        "MISSING_SEARCH_RESULT_CONTEXT",
                        "本文取得の対象となる直前の検索結果がありません。",
                    );
                };
                let Some((search_ref, observation)) = crate::search::previous_observation(
                    &self.store,
                    actor,
                    &request.case_id,
                    prior,
                )?
                else {
                    return self.stop(
                        &mut state,
                        Lifecycle::Suspend,
                        "MISSING_SEARCH_RESULT_CONTEXT",
                        "直前の結果に再検証できる検索一覧がありません。",
                    );
                };
                let Some(candidate) = observation
                    .candidates
                    .iter()
                    .find(|candidate| candidate.rank == *rank)
                else {
                    return self.stop(
                        &mut state,
                        Lifecycle::Suspend,
                        "SEARCH_CANDIDATE_UNAVAILABLE",
                        "指定順位の有効な検索候補が直前の一覧にありません。",
                    );
                };
                let carried_evidence =
                    match crate::search::carry_forward_candidate_evidence(prior, &observation) {
                        Ok(evidence) => evidence,
                        Err(error) => {
                            let code = match &error {
                                Error::Contract { code, .. } => code.as_str(),
                                _ => "SEARCH_CONTEXT_INVALID",
                            };
                            let lifecycle = if code == "SOURCE_BODY_PACKET_LIMIT" {
                                Lifecycle::Suspend
                            } else {
                                Lifecycle::Fail
                            };
                            return self.stop(&mut state, lifecycle, code, &error.to_string());
                        }
                    };
                if let Err(error) =
                    crate::search::validate_next_candidate_body_capacity(&carried_evidence)
                {
                    return self.stop(
                        &mut state,
                        Lifecycle::Suspend,
                        "SOURCE_BODY_PACKET_LIMIT",
                        &error.to_string(),
                    );
                }
                if !graph
                    .ready()
                    .iter()
                    .any(|ready| ready == "search-candidate-listing")
                {
                    return self.stop(
                        &mut state,
                        Lifecycle::Suspend,
                        "UNOBSERVED_PREREQUISITE",
                        "検索候補の本文取得に必要な検索一覧が未観測です。",
                    );
                }
                graph.observe("search-candidate-listing", &search_ref, &BTreeMap::new())?;
                let body_id = format!("search-candidate-body-{rank}");
                if !graph.ready().iter().any(|ready| ready == &body_id) {
                    return self.stop(
                        &mut state,
                        Lifecycle::Suspend,
                        "UNOBSERVED_PREREQUISITE",
                        "本文取得前に検索一覧の依存関係を完了できません。",
                    );
                }
                result.evidence = carried_evidence;
                result.dependencies.push(prior.id.clone());
                result.dependencies.push(search_ref.clone());
                for packet in result
                    .evidence
                    .iter()
                    .filter(|packet| packet.packet_kind == "FETCHED_BODY")
                {
                    let packet_bytes = canonical(packet)?;
                    let packet_ref = digest(&packet_bytes);
                    match self.store.get_bytes(actor, &request.case_id, &packet_ref) {
                        Ok(stored) if stored == packet_bytes => {}
                        Ok(_) => {
                            return self.stop(
                                &mut state,
                                Lifecycle::Fail,
                                "SEARCH_CONTEXT_INVALID",
                                "保持中の本文packetと保存artifactの内容が一致しません。",
                            );
                        }
                        Err(error) => {
                            return self.stop(
                                &mut state,
                                Lifecycle::Fail,
                                "SEARCH_CONTEXT_INVALID",
                                &error.to_string(),
                            );
                        }
                    }
                    result.dependencies.push(packet_ref);
                }
                let elapsed = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
                let mut remaining = request.budget.clone();
                remaining.deadline_ms = remaining.deadline_ms.saturating_sub(elapsed);
                if remaining.deadline_ms == 0 {
                    return self.stop(
                        &mut state,
                        Lifecycle::Suspend,
                        "DEADLINE",
                        "本文取得の総締切です。",
                    );
                }
                let packet = match crate::search::fetch_candidate(
                    candidate,
                    &request.permissions,
                    &remaining,
                ) {
                    Ok(packet) => packet,
                    Err(error) => {
                        let code = match &error {
                            Error::Contract { code, .. } => code.as_str(),
                            _ => "EVIDENCE_UNAVAILABLE",
                        };
                        return self.stop(&mut state, Lifecycle::Suspend, code, &error.to_string());
                    }
                };
                if packet.raw_body.len() > request.budget.max_evidence_bytes {
                    return self.stop(
                        &mut state,
                        Lifecycle::Suspend,
                        "RESOURCE_LIMIT",
                        "検索候補の取得本文が要求の証拠予算を超えました。",
                    );
                }
                let body_ref = self.store.put_artifact(actor, &request.case_id, &packet)?;
                graph.observe(
                    &body_id,
                    &body_ref,
                    &BTreeMap::from([("search-candidate-listing".into(), search_ref)]),
                )?;
                result.dependencies.push(body_ref);
                if !result.evidence.iter().any(|existing| existing == &packet) {
                    result.evidence.push(packet.clone());
                }
                if packet.failure.is_some() || packet.extracted_text.is_none() {
                    state.result = Some(result);
                    return self.stop(
                        &mut state,
                        Lifecycle::Suspend,
                        "FETCH_OR_PARSER_FAILURE",
                        "選択した検索候補の本文を取得または検証できません。失敗packetを保持しました。",
                    );
                }
                units.extend(crate::search::candidate_body_lines(candidate, &packet));
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
                let prior_relation = if prior.calculation.is_none() {
                    previous_relation_meaning(&self.store, actor, &request.case_id, prior)?
                } else {
                    None
                };
                if let Some(ref calculation) = prior.calculation {
                    units.push(format!("計算式: {}", calculation.expression));
                    units.extend(calculation.steps.clone());
                    units.push(format!("計算結果: {}", calculation.exact_value));
                    approved_surfaces =
                        Some(language::calculation_explanation_surfaces(calculation));
                    result.calculation = Some(calculation.clone());
                    result.dependencies.push(prior.id.clone());
                    result.guarantee = prior.guarantee.clone();
                } else if let Some(PriorRelationMeaning::OriginalQuestion(proposition)) =
                    prior_relation.as_ref()
                {
                    let memory = self.store.load_memory(actor)?;
                    let answer =
                        relations::query(&memory, proposition, request.budget.max_actions)?;
                    let proof_dependencies = relations::answer_dependencies(&answer);
                    let answer_guarantee = relations::answer_guarantee(&answer);
                    let Some(explained) = relation_explanation_target(proposition, &answer) else {
                        return self.stop(
                            &mut state,
                            Lifecycle::Suspend,
                            "CONTEXT_PROOF_CHANGED",
                            "前回入力の型付き関係を、現在有効な根拠で再確認できません。",
                        );
                    };
                    if proof_dependencies != prior.dependencies
                        || answer_guarantee != prior.guarantee
                    {
                        return self.stop(
                            &mut state,
                            Lifecycle::Suspend,
                            "CONTEXT_PROOF_CHANGED",
                            "前回の関係回答を支えた現在有効な根拠が変わっています。新しい問いで確認してください。",
                        );
                    }
                    units.extend(relation_explanation_lines(
                        &memory,
                        &explained,
                        &answer,
                        "前回の結論",
                    ));
                    result.dependencies.push(prior.id.clone());
                    result.dependencies.extend(proof_dependencies);
                    result.guarantee = answer_guarantee;
                } else if let Some(PriorRelationMeaning::AdoptedConclusion {
                    proposition,
                    proof_dependencies: expected_proof_dependencies,
                }) = prior_relation.as_ref()
                {
                    let memory = self.store.load_memory(actor)?;
                    let answer =
                        relations::query(&memory, proposition, request.budget.max_actions)?;
                    let proof_dependencies = relations::answer_dependencies(&answer);
                    let answer_guarantee = relations::answer_guarantee(&answer);
                    if answer.status != "SUPPORTED"
                        || proof_dependencies != *expected_proof_dependencies
                        || proof_dependencies != prior.dependencies
                        || answer_guarantee != prior.guarantee
                    {
                        return self.stop(
                            &mut state,
                            Lifecycle::Suspend,
                            "CONTEXT_PROOF_CHANGED",
                            "前回の関係回答を支えた現在有効な根拠が変わっています。新しい問いで確認してください。",
                        );
                    }
                    units.extend(relation_explanation_lines(
                        &memory,
                        proposition,
                        &answer,
                        "前回の結論",
                    ));
                    result.dependencies.push(prior.id.clone());
                    result.dependencies.extend(proof_dependencies);
                    result.guarantee = answer_guarantee;
                } else if prior.text.starts_with(RELATION_COMPARISON_REPORT_HEADER) {
                    if bundle.meaning.context_result_ref.as_deref() != Some(prior.id.as_str()) {
                        return self.stop(
                            &mut state,
                            Lifecycle::Suspend,
                            "MISSING_RELATION_COMPARISON_CONTEXT",
                            "説明対象として固定された直前の保存関係比較がありません。",
                        );
                    }
                    let Some(report_ref) = prior.dependencies.first().cloned() else {
                        return self.stop(
                            &mut state,
                            Lifecycle::Suspend,
                            "RELATION_COMPARISON_ARTIFACT_UNAVAILABLE",
                            "前回の保存関係比較に再検証可能なartifactがありません。",
                        );
                    };
                    let Some(report_bytes) =
                        self.store
                            .get_bytes_if_present(actor, &request.case_id, &report_ref)?
                    else {
                        return self.stop(
                            &mut state,
                            Lifecycle::Suspend,
                            "RELATION_COMPARISON_ARTIFACT_UNAVAILABLE",
                            "前回の保存関係比較artifactを同一ケースから取得できません。",
                        );
                    };
                    if report_bytes.len() > request.budget.max_evidence_bytes {
                        return self.stop(
                            &mut state,
                            Lifecycle::Suspend,
                            "RESOURCE_LIMIT",
                            "前回の保存関係比較artifactが今回の証拠予算を超えています。",
                        );
                    }
                    let artifact = match serde_json::from_slice::<RelationComparisonReportArtifact>(
                        &report_bytes,
                    ) {
                        Ok(value) => value,
                        Err(_) => {
                            return self.stop(
                                &mut state,
                                Lifecycle::Suspend,
                                "RELATION_COMPARISON_ARTIFACT_INVALID",
                                "前回の保存関係比較artifactの形式を検証できません。",
                            );
                        }
                    };
                    let (Some(prior_plan), Some(prior_render)) =
                        (&prior.content_plan, &prior.render)
                    else {
                        return self.stop(
                            &mut state,
                            Lifecycle::Suspend,
                            "RELATION_COMPARISON_OUTPUT_UNAVAILABLE",
                            "前回比較の採用ContentPlanまたは実表示記録がありません。",
                        );
                    };
                    let mut unsigned_plan = prior_plan.clone();
                    unsigned_plan.id.clear();
                    if canonical(&artifact)? != report_bytes
                        || digest(&report_bytes) != report_ref
                        || artifact.kind != RELATION_COMPARISON_REPORT_KIND
                        || artifact.result_id != prior.id
                        || artifact.subject_a.is_empty()
                        || artifact.subject_b.is_empty()
                        || artifact.subject_a == artifact.subject_b
                        || artifact.scope.is_empty()
                        || prior.lifecycle != Lifecycle::Commit
                        || prior.guarantee != Guarantee::Hypothesis
                        || prior_plan.guarantee != Guarantee::Hypothesis
                        || prior_plan.id != identity(&unsigned_plan)?
                        || prior_plan.text != prior.text
                        || !validate_render(prior_plan, prior_render)
                        || prior.dependencies.first() != Some(&report_ref)
                    {
                        return self.stop(
                            &mut state,
                            Lifecycle::Suspend,
                            "RELATION_COMPARISON_ARTIFACT_INVALID",
                            "前回比較結果とartifactまたは表示記録の同一性が一致しません。",
                        );
                    }
                    let prior_content_bytes = canonical(prior_plan)?;
                    let prior_content_ref = digest(&prior_content_bytes);
                    let prior_render_bytes = canonical(prior_render)?;
                    let prior_render_ref = digest(&prior_render_bytes);
                    for (reference, expected) in [
                        (prior_content_ref.as_str(), prior_content_bytes.as_slice()),
                        (prior_render_ref.as_str(), prior_render_bytes.as_slice()),
                    ] {
                        match self
                            .store
                            .get_bytes_if_present(actor, &request.case_id, reference)?
                        {
                            Some(stored) if stored == expected => {}
                            Some(_) => {
                                return self.stop(
                                    &mut state,
                                    Lifecycle::Fail,
                                    "RELATION_COMPARISON_OUTPUT_INTEGRITY",
                                    "前回比較のContentPlanまたはrender artifactが保存済み実体と一致しません。",
                                );
                            }
                            None => {
                                return self.stop(
                                    &mut state,
                                    Lifecycle::Suspend,
                                    "RELATION_COMPARISON_OUTPUT_UNAVAILABLE",
                                    "前回比較のContentPlanまたはrender artifactを同一ケースから取得できません。",
                                );
                            }
                        }
                    }
                    let per_subject_actions = request.budget.max_actions / 2;
                    if per_subject_actions == 0 {
                        return self.stop(
                            &mut state,
                            Lifecycle::Suspend,
                            "RESOURCE_LIMIT",
                            "左右の保存関係を再検証する推論予算が不足しています。",
                        );
                    }
                    let memory = self.store.load_memory(actor)?;
                    let inventory_a = match relation_comparison_inventory(
                        &memory,
                        &artifact.subject_a,
                        &artifact.scope,
                        per_subject_actions,
                    ) {
                        Ok(value) => value,
                        Err(Error::Contract { code, message }) => {
                            let code = if code == "RELATION_BUDGET" {
                                "RESOURCE_LIMIT"
                            } else {
                                "RELATION_COMPARISON_ARTIFACT_INVALID"
                            };
                            return self.stop(&mut state, Lifecycle::Suspend, code, &message);
                        }
                        Err(error) => return Err(error),
                    };
                    let inventory_b = match relation_comparison_inventory(
                        &memory,
                        &artifact.subject_b,
                        &artifact.scope,
                        per_subject_actions,
                    ) {
                        Ok(value) => value,
                        Err(Error::Contract { code, message }) => {
                            let code = if code == "RELATION_BUDGET" {
                                "RESOURCE_LIMIT"
                            } else {
                                "RELATION_COMPARISON_ARTIFACT_INVALID"
                            };
                            return self.stop(&mut state, Lifecycle::Suspend, code, &message);
                        }
                        Err(error) => return Err(error),
                    };
                    if inventory_a != artifact.inventory_a || inventory_b != artifact.inventory_b {
                        return self.stop(
                            &mut state,
                            Lifecycle::Suspend,
                            "CONTEXT_PROOF_CHANGED",
                            "前回比較の左右いずれかの保存根拠・競合状態が変わっています。新しい比較を行ってください。",
                        );
                    }
                    let report = relation_comparison_report(
                        &artifact.subject_a,
                        &artifact.subject_b,
                        &artifact.scope,
                        &inventory_a,
                        &inventory_b,
                    );
                    if digest(report.as_bytes()) != artifact.report_sha256
                        || prior_plan.required_units != [report.clone()]
                        || prior.text != report
                        || !prior_plan.dependencies.starts_with(&prior.dependencies)
                    {
                        return self.stop(
                            &mut state,
                            Lifecycle::Suspend,
                            "RELATION_COMPARISON_ARTIFACT_INVALID",
                            "再生成した比較本文または出力依存記録が前回artifactと一致しません。",
                        );
                    }
                    let mut expected_dependencies = vec![report_ref];
                    expected_dependencies.extend(
                        inventory_a
                            .dependencies
                            .iter()
                            .chain(&inventory_b.dependencies)
                            .cloned()
                            .collect::<BTreeSet<_>>(),
                    );
                    if prior.dependencies != expected_dependencies {
                        return self.stop(
                            &mut state,
                            Lifecycle::Suspend,
                            "RELATION_COMPARISON_ARTIFACT_INVALID",
                            "前回比較の依存記録が左右の再検証済みinventoryと一致しません。",
                        );
                    }
                    let mut explanation = vec![format!(
                        "前回の保存関係比較を詳しく説明します（比較結果ID: {}）。同じactorの現在有効な記憶から左右のscope別inventoryを再計算し、前回と一致することを確認しました。",
                        prior.id
                    )];
                    explanation.extend(report.lines().skip(1).map(str::to_string));
                    explanation.push(
                        "比較分類は保存済みの前提・規則の範囲に限られます。片側で支持を確認できないことは、関係が偽または存在しない証明ではありません。記録の網羅性と現実世界の真偽は確認していません。".into(),
                    );
                    if explanation.join("\n").len() > request.budget.max_evidence_bytes {
                        return self.stop(
                            &mut state,
                            Lifecycle::Suspend,
                            "RESOURCE_LIMIT",
                            "再検証した保存関係比較の説明が今回の証拠予算を超えています。",
                        );
                    }
                    units.extend(explanation);
                    result.dependencies.push(prior.id.clone());
                    result
                        .dependencies
                        .extend(prior.dependencies.iter().cloned());
                    result.guarantee = Guarantee::Hypothesis;
                } else if prior.text.starts_with(COMMON_RELATIONS_REPORT_HEADER) {
                    if bundle.meaning.context_result_ref.as_deref() != Some(prior.id.as_str()) {
                        return self.stop(
                            &mut state,
                            Lifecycle::Suspend,
                            "MISSING_COMMON_RELATION_CONTEXT",
                            "説明対象として固定された直前の共通関係比較がありません。",
                        );
                    }
                    let Some(report_ref) = prior.dependencies.first().cloned() else {
                        return self.stop(
                            &mut state,
                            Lifecycle::Suspend,
                            "COMMON_RELATION_CONTEXT_UNAVAILABLE",
                            "前回の共通関係比較に再検証可能な証拠artifactがありません。",
                        );
                    };
                    let Some(report_bytes) =
                        self.store
                            .get_bytes_if_present(actor, &request.case_id, &report_ref)?
                    else {
                        return self.stop(
                            &mut state,
                            Lifecycle::Suspend,
                            "COMMON_RELATION_CONTEXT_UNAVAILABLE",
                            "前回の共通関係比較artifactを同一ケースから取得できません。",
                        );
                    };
                    if report_bytes.len() > request.budget.max_evidence_bytes {
                        return self.stop(
                            &mut state,
                            Lifecycle::Suspend,
                            "RESOURCE_LIMIT",
                            "前回の共通関係artifactが今回の証拠予算を超えています。",
                        );
                    }
                    let artifact = match serde_json::from_slice::<CommonRelationsReportArtifact>(
                        &report_bytes,
                    ) {
                        Ok(value) => value,
                        Err(_) => {
                            return self.stop(
                                &mut state,
                                Lifecycle::Suspend,
                                "COMMON_RELATION_CONTEXT_INVALID",
                                "前回の共通関係artifactの形式を検証できません。",
                            );
                        }
                    };
                    if canonical(&artifact)? != report_bytes
                        || artifact.kind != COMMON_RELATIONS_REPORT_KIND
                        || artifact.result_id != prior.id
                        || !prior.content_plan.as_ref().is_some_and(|plan| {
                            plan.required_units.len() == 1
                                && digest(plan.required_units[0].as_bytes())
                                    == artifact.report_sha256
                        })
                        || artifact.subject_a.is_empty()
                        || artifact.subject_b.is_empty()
                        || artifact.subject_a == artifact.subject_b
                        || artifact.scope.is_empty()
                        || artifact.shared_proofs.is_empty()
                        || prior.dependencies.first() != Some(&report_ref)
                    {
                        return self.stop(
                            &mut state,
                            Lifecycle::Suspend,
                            "COMMON_RELATION_CONTEXT_INVALID",
                            "前回結果とartifactの同一性または比較条件が一致しません。",
                        );
                    }
                    let query_count = match u32::try_from(artifact.shared_proofs.len())
                        .ok()
                        .and_then(|count| count.checked_mul(2))
                    {
                        Some(count) if count <= request.budget.max_actions => count,
                        _ => {
                            return self.stop(
                                &mut state,
                                Lifecycle::Suspend,
                                "RESOURCE_LIMIT",
                                "各共通関係を両対象について再検証する推論予算が不足しています。",
                            );
                        }
                    };
                    let per_query_budget = request.budget.max_actions / query_count;
                    let memory = self.store.load_memory(actor)?;
                    let mut explanation = vec![format!(
                        "前回比較の詳細説明（比較結果ID: {}）。候補となった各関係を、同一ケースの現在有効な記憶に対して両対象ごとに再照合しました。",
                        prior.id
                    )];
                    let mut expected_dependencies = BTreeSet::from([report_ref.clone()]);
                    let mut candidate_keys = BTreeSet::new();
                    let mut any_derived = false;
                    for proof in &artifact.shared_proofs {
                        let expected_b = Proposition {
                            subject: artifact.subject_b.clone(),
                            ..proof.proposition_a.clone()
                        };
                        let candidate_key = (
                            proof.proposition_a.predicate.clone(),
                            proof.proposition_a.object.clone(),
                            proof.proposition_a.negative,
                        );
                        if proof.proposition_a.subject != artifact.subject_a
                            || proof.proposition_a.scope != artifact.scope
                            || proof.proposition_a.predicate.starts_with(['$', '?'])
                            || proof.proposition_a.object.starts_with(['$', '?'])
                            || proof.proposition_b != expected_b
                            || !candidate_keys.insert(candidate_key)
                            || !matches!(
                                proof.guarantee_a,
                                Guarantee::UserPremise | Guarantee::Derived
                            )
                            || !matches!(
                                proof.guarantee_b,
                                Guarantee::UserPremise | Guarantee::Derived
                            )
                            || proof.dependencies_a.is_empty()
                            || proof.dependencies_b.is_empty()
                            || !proof
                                .dependencies_a
                                .windows(2)
                                .all(|pair| pair[0] < pair[1])
                            || !proof
                                .dependencies_b
                                .windows(2)
                                .all(|pair| pair[0] < pair[1])
                        {
                            return self.stop(
                                &mut state,
                                Lifecycle::Suspend,
                                "COMMON_RELATION_CONTEXT_INVALID",
                                "前回の比較条件または左右の証拠一覧に不整合があります。",
                            );
                        }
                        let answer_a =
                            relations::query(&memory, &proof.proposition_a, per_query_budget)?;
                        let answer_b =
                            relations::query(&memory, &proof.proposition_b, per_query_budget)?;
                        let dependencies_a = relations::answer_dependencies(&answer_a);
                        let dependencies_b = relations::answer_dependencies(&answer_b);
                        let guarantee_a = relations::answer_guarantee(&answer_a);
                        let guarantee_b = relations::answer_guarantee(&answer_b);
                        if answer_a.status != "SUPPORTED"
                            || answer_b.status != "SUPPORTED"
                            || dependencies_a != proof.dependencies_a
                            || dependencies_b != proof.dependencies_b
                            || guarantee_a != proof.guarantee_a
                            || guarantee_b != proof.guarantee_b
                        {
                            return self.stop(
                                &mut state,
                                Lifecycle::Suspend,
                                "CONTEXT_PROOF_CHANGED",
                                "前回比較の左右いずれかを支えた現在有効な根拠が変わっています。新しい比較を行ってください。",
                            );
                        }
                        expected_dependencies.extend(dependencies_a.iter().cloned());
                        expected_dependencies.extend(dependencies_b.iter().cloned());
                        any_derived |=
                            guarantee_a == Guarantee::Derived || guarantee_b == Guarantee::Derived;
                        explanation.push(format!(
                            "共通関係の照合: {} ／ {}（範囲: {}）。両側で同一の述語・対象・肯定否定・範囲が支持されています。",
                            describe(&proof.proposition_a),
                            describe(&proof.proposition_b),
                            artifact.scope
                        ));
                        explanation.push(format!(
                            "{}側: SUPPORTED（{}）；{}側: SUPPORTED（{}）。左右の証拠IDを別々に再確認しました。",
                            artifact.subject_a,
                            guarantee_label(&guarantee_a),
                            artifact.subject_b,
                            guarantee_label(&guarantee_b)
                        ));
                        explanation.extend(relation_explanation_lines(
                            &memory,
                            &proof.proposition_a,
                            &answer_a,
                            &format!("{}側の証明", artifact.subject_a),
                        ));
                        explanation.extend(relation_explanation_lines(
                            &memory,
                            &proof.proposition_b,
                            &answer_b,
                            &format!("{}側の証明", artifact.subject_b),
                        ));
                    }
                    if expected_dependencies
                        != prior.dependencies.iter().cloned().collect::<BTreeSet<_>>()
                        || prior.dependencies.len() != expected_dependencies.len()
                        || (any_derived && prior.guarantee != Guarantee::Derived)
                        || (!any_derived && prior.guarantee != Guarantee::UserPremise)
                    {
                        return self.stop(
                            &mut state,
                            Lifecycle::Suspend,
                            "COMMON_RELATION_CONTEXT_INVALID",
                            "前回比較の依存記録または全体保証がartifactと一致しません。",
                        );
                    }
                    explanation.push(
                        "一覧にない関係は「共通点がない」ことを意味しません。この説明は保存済みの前提・規則の範囲に限られ、現実世界の真偽を確認したものではありません。".into(),
                    );
                    if explanation.join("\n").len() > request.budget.max_evidence_bytes {
                        return self.stop(
                            &mut state,
                            Lifecycle::Suspend,
                            "RESOURCE_LIMIT",
                            "再検証した共通関係の説明が今回の証拠予算を超えています。",
                        );
                    }
                    units.extend(explanation);
                    result.dependencies.push(prior.id.clone());
                    result
                        .dependencies
                        .extend(prior.dependencies.iter().cloned());
                    result.guarantee = prior.guarantee.clone();
                } else if prior
                    .text
                    .starts_with(crate::text_summary::PREVIOUS_RESULT_SUMMARY_HEADER)
                {
                    if bundle.meaning.context_result_ref.as_deref() != Some(prior.id.as_str()) {
                        return self.stop(
                            &mut state,
                            Lifecycle::Suspend,
                            "MISSING_RESULT_SUMMARY_CONTEXT",
                            "説明対象として固定された直前の要約結果がありません。",
                        );
                    }
                    let Some(report_ref) = prior.dependencies.first().cloned() else {
                        return self.stop(
                            &mut state,
                            Lifecycle::Suspend,
                            "RESULT_SUMMARY_ARTIFACT_UNAVAILABLE",
                            "前回要約に再検証可能なartifactがありません。",
                        );
                    };
                    let Some(report_bytes) =
                        self.store
                            .get_bytes_if_present(actor, &request.case_id, &report_ref)?
                    else {
                        return self.stop(
                            &mut state,
                            Lifecycle::Suspend,
                            "RESULT_SUMMARY_ARTIFACT_UNAVAILABLE",
                            "前回要約artifactを同一ケースから取得できません。",
                        );
                    };
                    if report_bytes.len() > request.budget.max_evidence_bytes {
                        return self.stop(
                            &mut state,
                            Lifecycle::Suspend,
                            "RESOURCE_LIMIT",
                            "前回要約artifactが今回の証拠予算を超えています。",
                        );
                    }
                    let artifact = match serde_json::from_slice::<
                        crate::text_summary::PreviousResultSummaryArtifact,
                    >(&report_bytes)
                    {
                        Ok(value) => value,
                        Err(_) => {
                            return self.stop(
                                &mut state,
                                Lifecycle::Suspend,
                                "RESULT_SUMMARY_ARTIFACT_INVALID",
                                "前回要約artifactの形式を検証できません。",
                            );
                        }
                    };
                    let (Some(summary_plan), Some(summary_render)) =
                        (&prior.content_plan, &prior.render)
                    else {
                        return self.stop(
                            &mut state,
                            Lifecycle::Suspend,
                            "RESULT_SUMMARY_OUTPUT_UNAVAILABLE",
                            "前回要約の採用ContentPlanまたは実表示記録がありません。",
                        );
                    };
                    let summary_unit = crate::text_summary::previous_result_summary_unit(
                        &artifact.source_result_id,
                        &artifact.summary,
                    );
                    let mut unsigned_summary_plan = summary_plan.clone();
                    unsigned_summary_plan.id.clear();
                    if canonical(&artifact)? != report_bytes
                        || artifact.kind != crate::text_summary::PREVIOUS_RESULT_SUMMARY_KIND
                        || artifact.summary_result_id != prior.id
                        || prior.lifecycle != Lifecycle::Commit
                        || !prior.evidence.is_empty()
                        || summary_plan.id != identity(&unsigned_summary_plan)?
                        || summary_plan.text != prior.text
                        || summary_plan.guarantee != Guarantee::Hypothesis
                        || prior.guarantee != Guarantee::Hypothesis
                        || summary_plan.required_units != [summary_unit.clone()]
                        || prior.text != summary_unit
                        || digest(summary_unit.as_bytes()) != artifact.summary_unit_sha256
                        || !validate_render(summary_plan, summary_render)
                        || prior.dependencies.first() != Some(&report_ref)
                    {
                        return self.stop(
                            &mut state,
                            Lifecycle::Suspend,
                            "RESULT_SUMMARY_ARTIFACT_INVALID",
                            "前回要約結果とartifactの同一性が一致しません。",
                        );
                    }
                    let summary_content_bytes = canonical(summary_plan)?;
                    let summary_content_ref = digest(&summary_content_bytes);
                    let summary_render_bytes = canonical(summary_render)?;
                    let summary_render_ref = digest(&summary_render_bytes);
                    for (reference, expected) in [
                        (
                            summary_content_ref.as_str(),
                            summary_content_bytes.as_slice(),
                        ),
                        (summary_render_ref.as_str(), summary_render_bytes.as_slice()),
                    ] {
                        match self
                            .store
                            .get_bytes_if_present(actor, &request.case_id, reference)?
                        {
                            Some(stored) if stored == expected => {}
                            Some(_) => {
                                return self.stop(
                                    &mut state,
                                    Lifecycle::Fail,
                                    "RESULT_SUMMARY_OUTPUT_INTEGRITY",
                                    "前回要約のContentPlanまたはrender artifactが保存済み実体と一致しません。",
                                );
                            }
                            None => {
                                return self.stop(
                                    &mut state,
                                    Lifecycle::Suspend,
                                    "RESULT_SUMMARY_OUTPUT_UNAVAILABLE",
                                    "前回要約のContentPlanまたはrender artifactを同一ケースから取得できません。",
                                );
                            }
                        }
                    }
                    let Some(source_result) =
                        state.prior_results.get(&artifact.source_result_id).cloned()
                    else {
                        return self.stop(
                            &mut state,
                            Lifecycle::Suspend,
                            "RESULT_SUMMARY_SOURCE_UNAVAILABLE",
                            "要約元となった直前結果の履歴を取得できません。",
                        );
                    };
                    let (Some(source_plan), Some(source_render)) =
                        (&source_result.content_plan, &source_result.render)
                    else {
                        return self.stop(
                            &mut state,
                            Lifecycle::Suspend,
                            "RESULT_SUMMARY_SOURCE_UNAVAILABLE",
                            "要約元の確定本文または実表示記録がありません。",
                        );
                    };
                    if source_result.lifecycle != Lifecycle::Commit
                        || source_result.id != artifact.source_result_id
                        || source_result.text != source_plan.text
                        || !validate_render(source_plan, source_render)
                    {
                        return self.stop(
                            &mut state,
                            Lifecycle::Fail,
                            "RESULT_SUMMARY_SOURCE_INTEGRITY",
                            "要約元の採用本文と実表示記録が一致しません。",
                        );
                    }
                    let mut unsigned_source_plan = source_plan.clone();
                    unsigned_source_plan.id.clear();
                    if source_plan.id != identity(&unsigned_source_plan)? {
                        return self.stop(
                            &mut state,
                            Lifecycle::Fail,
                            "RESULT_SUMMARY_SOURCE_INTEGRITY",
                            "要約元ContentPlanの識別子を再検証できません。",
                        );
                    }
                    let source_content_bytes = canonical(source_plan)?;
                    let source_content_ref = digest(&source_content_bytes);
                    let source_render_bytes = canonical(source_render)?;
                    let source_render_ref = digest(&source_render_bytes);
                    for (reference, expected) in [
                        (source_content_ref.as_str(), source_content_bytes.as_slice()),
                        (source_render_ref.as_str(), source_render_bytes.as_slice()),
                    ] {
                        match self
                            .store
                            .get_bytes_if_present(actor, &request.case_id, reference)?
                        {
                            Some(stored) if stored == expected => {}
                            Some(_) => {
                                return self.stop(
                                    &mut state,
                                    Lifecycle::Fail,
                                    "RESULT_SUMMARY_SOURCE_INTEGRITY",
                                    "要約元のContentPlanまたはrender artifactが保存済み実体と一致しません。",
                                );
                            }
                            None => {
                                return self.stop(
                                    &mut state,
                                    Lifecycle::Suspend,
                                    "RESULT_SUMMARY_SOURCE_UNAVAILABLE",
                                    "要約元のContentPlanまたはrender artifactを同一ケースから取得できません。",
                                );
                            }
                        }
                    }
                    let source_text_sha256 = digest(source_result.text.as_bytes());
                    if source_plan.id != artifact.source_content_plan_id
                        || source_render.actual_sha256 != artifact.source_render_sha256
                        || source_text_sha256 != artifact.source_text_sha256
                        || artifact.summary.source_text != source_result.text
                        || artifact.summary.source_text_sha256 != source_text_sha256
                        || artifact.summary.output_sha256
                            != digest(artifact.summary.text.as_bytes())
                    {
                        return self.stop(
                            &mut state,
                            Lifecycle::Suspend,
                            "RESULT_SUMMARY_SOURCE_CHANGED",
                            "要約artifactと元結果の本文・ContentPlan・表示記録が一致しません。",
                        );
                    }
                    if artifact.summary.sentence_count
                        > usize::try_from(request.budget.max_actions).unwrap_or(usize::MAX)
                    {
                        return self.stop(
                            &mut state,
                            Lifecycle::Suspend,
                            "RESOURCE_LIMIT",
                            "要約選択を再検証する推論予算が不足しています。",
                        );
                    }
                    let recomputed = match crate::text_summary::extractive_summary_candidates(
                        &artifact.summary.source_document,
                    ) {
                        Ok(value) => value,
                        Err(_) => {
                            return self.stop(
                                &mut state,
                                Lifecycle::Suspend,
                                "RESULT_SUMMARY_RECOMPUTATION_FAILED",
                                "保存済み要約条件から同じ候補を再生成できません。",
                            );
                        }
                    };
                    if recomputed != artifact.summary {
                        return self.stop(
                            &mut state,
                            Lifecycle::Suspend,
                            "RESULT_SUMMARY_RECOMPUTATION_CHANGED",
                            "保存済みの抽出要約候補を同じ本文・条件から再現できません。",
                        );
                    }
                    let mut explanation = vec![format!(
                        "前回結果の要約説明（要約結果ID: {}; 元結果ID: {}）。元結果は当時の採用本文です。この説明は現在の事実性を再判定するものではありません。",
                        prior.id, source_result.id
                    )];
                    for sentence in &artifact.summary.sentences {
                        let source_span = source_result
                            .text
                            .get(sentence.source_start..sentence.source_end);
                        let output_span = artifact
                            .summary
                            .text
                            .get(sentence.output_start..sentence.output_end);
                        if source_span != Some(sentence.text.as_str())
                            || output_span != Some(sentence.text.as_str())
                        {
                            return self.stop(
                                &mut state,
                                Lifecycle::Suspend,
                                "RESULT_SUMMARY_SPAN_INVALID",
                                "選択文の元結果byte範囲または要約出力範囲が一致しません。",
                            );
                        }
                        explanation.push(format!(
                            "選択文{}（選択順位{}、元結果byte {}..{}）: {}",
                            sentence.sentence_index,
                            sentence.selection_rank,
                            sentence.source_start,
                            sentence.source_end,
                            sentence.text
                        ));
                    }
                    explanation.push(format!(
                        "候補文{}件のうち{}件を抽出しました。残り{}件は今回の選択に含まれていません。要約の網羅性・意味・条件保持は未検証です。",
                        artifact.summary.sentence_count,
                        artifact.summary.sentences.len(),
                        artifact
                            .summary
                            .sentence_count
                            .saturating_sub(artifact.summary.sentences.len())
                    ));
                    let mut expected_dependencies = BTreeSet::from([
                        report_ref.clone(),
                        source_result.id.clone(),
                        source_content_ref,
                        source_render_ref,
                    ]);
                    expected_dependencies.extend(source_result.dependencies.iter().cloned());
                    if prior.dependencies.len() != expected_dependencies.len()
                        || prior.dependencies.iter().cloned().collect::<BTreeSet<_>>()
                            != expected_dependencies
                    {
                        return self.stop(
                            &mut state,
                            Lifecycle::Suspend,
                            "RESULT_SUMMARY_ARTIFACT_INVALID",
                            "要約元の結果依存関係がartifactと一致しません。",
                        );
                    }
                    if explanation.join("\n").len() > request.budget.max_evidence_bytes {
                        return self.stop(
                            &mut state,
                            Lifecycle::Suspend,
                            "RESOURCE_LIMIT",
                            "要約の出典説明が今回の証拠予算を超えています。",
                        );
                    }
                    units.extend(explanation);
                    result.dependencies.push(prior.id.clone());
                    result
                        .dependencies
                        .extend(prior.dependencies.iter().cloned());
                    result.guarantee = Guarantee::Hypothesis;
                } else if let Some((report_ref, explanation)) =
                    crate::code_generation::previous_code_explanation(
                        &self.store,
                        actor,
                        &request.case_id,
                        prior,
                    )?
                {
                    units.extend(explanation);
                    result.dependencies.push(prior.id.clone());
                    result.dependencies.push(report_ref);
                    result.guarantee = prior.guarantee.clone();
                } else if let Some((report_ref, explanation)) = &previous_csv_statistics_explanation
                {
                    graph.observe("previous-csv-statistics", report_ref, &BTreeMap::new())?;
                    units.extend(explanation.clone());
                    result.dependencies.push(prior.id.clone());
                    result.dependencies.push(report_ref.clone());
                    result.guarantee = prior.guarantee.clone();
                } else if let Some((source_ref, report_ref, explanation)) =
                    &previous_json_table_group_statistics_explanation
                {
                    graph.observe(
                        "previous-json-group-statistics-source",
                        source_ref,
                        &BTreeMap::new(),
                    )?;
                    graph.observe(
                        "previous-json-group-statistics-report",
                        report_ref,
                        &BTreeMap::from([(
                            "previous-json-group-statistics-source".into(),
                            source_ref.clone(),
                        )]),
                    )?;
                    units.extend(explanation.clone());
                    result.dependencies.extend([
                        prior.id.clone(),
                        source_ref.clone(),
                        report_ref.clone(),
                    ]);
                    result.guarantee = prior.guarantee.clone();
                } else if let Some((bundle_ref, report_ref, explanation)) =
                    &previous_json_table_operation_explanation
                {
                    let input_id = "previous-json-table-transform-input".to_string();
                    let report_id = "previous-json-table-transform-report".to_string();
                    graph.observe(&input_id, bundle_ref, &BTreeMap::new())?;
                    graph.observe(
                        &report_id,
                        report_ref,
                        &BTreeMap::from([(input_id, bundle_ref.clone())]),
                    )?;
                    units.extend(explanation.clone());
                    result.dependencies.extend([
                        prior.id.clone(),
                        bundle_ref.clone(),
                        report_ref.clone(),
                    ]);
                    result.guarantee = prior.guarantee.clone();
                } else if let Some((bundle_ref, report_ref, explanation)) =
                    &previous_csv_table_comparison_explanation
                {
                    let input_id = "previous-csv-table-comparison-input".to_string();
                    let report_id = "previous-csv-table-comparison-report".to_string();
                    graph.observe(&input_id, bundle_ref, &BTreeMap::new())?;
                    graph.observe(
                        &report_id,
                        report_ref,
                        &BTreeMap::from([(input_id, bundle_ref.clone())]),
                    )?;
                    units.extend(explanation.clone());
                    result.dependencies.extend([
                        prior.id.clone(),
                        bundle_ref.clone(),
                        report_ref.clone(),
                    ]);
                    result.guarantee = prior.guarantee.clone();
                } else if let Some(explanation) = &previous_math_report_explanation {
                    units.extend(explanation.clone());
                    result.dependencies.push(prior.id.clone());
                    result.guarantee = prior.guarantee.clone();
                } else {
                    units.push(prior.text.clone());
                    result.dependencies.push(prior.id.clone());
                    result.guarantee = prior.guarantee.clone();
                }
            }
            Intent::AskPreviousGroupMean => {
                let Some(prior) = previous.filter(|result| result.lifecycle == Lifecycle::Commit)
                else {
                    return self.stop(
                        &mut state,
                        Lifecycle::Suspend,
                        "MISSING_ACTUAL_RESULT",
                        "前回の採用済み結果がありません。先にグループ別統計を計算してください。",
                    );
                };
                let Some((source_ref, report_ref, answer)) = &previous_json_group_mean_answer
                else {
                    return self.stop(
                        &mut state,
                        Lifecycle::Suspend,
                        "MISSING_GROUP_STATISTICS_CONTEXT",
                        "前回の結果に再検証できるJSON表グループ統計がありません。",
                    );
                };
                let answer_bytes = canonical(answer)?;
                if answer_bytes.len() > request.budget.max_evidence_bytes {
                    return self.stop(
                        &mut state,
                        Lifecycle::Suspend,
                        "RESOURCE_LIMIT",
                        "グループ平均の比較結果が要求の証拠予算を超えました。",
                    );
                }
                let answer_ref = self
                    .store
                    .put_bytes(actor, &request.case_id, &answer_bytes)?;
                graph.observe(
                    "previous-json-group-statistics-source",
                    source_ref,
                    &BTreeMap::new(),
                )?;
                graph.observe(
                    "previous-json-group-statistics-report",
                    report_ref,
                    &BTreeMap::from([(
                        "previous-json-group-statistics-source".into(),
                        source_ref.clone(),
                    )]),
                )?;
                graph.observe(
                    "previous-json-group-mean-answer",
                    &answer_ref,
                    &BTreeMap::from([(
                        "previous-json-group-statistics-report".into(),
                        report_ref.clone(),
                    )]),
                )?;
                result.dependencies.extend([
                    prior.id.clone(),
                    source_ref.clone(),
                    report_ref.clone(),
                    answer_ref,
                ]);
                units.extend(crate::document::json_group_mean_answer_lines(answer));
                result.guarantee = Guarantee::Computed;
            }
            Intent::SummarizePreviousSource | Intent::SummarizePreviousSourceConstrained { .. } => {
                let Some(prior) = previous.filter(|result| result.lifecycle == Lifecycle::Commit)
                else {
                    return self.stop(
                        &mut state,
                        Lifecycle::Suspend,
                        "MISSING_ACTUAL_RESULT",
                        "要約対象となる実際の採用済み出典結果がありません。",
                    );
                };
                if source_followup_indices.is_empty() {
                    return self.stop(
                        &mut state,
                        Lifecycle::Suspend,
                        "SOURCE_BODY_UNAVAILABLE",
                        "前回結果に要約できる取得本文がありません。検索一覧や題名だけから本文を補いません。",
                    );
                }
                let summary_limits =
                    crate::text_summary::constrained_source_summary_limits(&bundle.meaning.intent);
                if summary_limits.is_some() && source_followup_indices.len() != 1 {
                    return self.stop(
                        &mut state,
                        Lifecycle::Suspend,
                        "AMBIGUOUS_SOURCE_CONTEXT",
                        "複数の取得本文があるため、文字数・箇条書き数の制約を適用する対象を一つに決められません。",
                    );
                }
                result.dependencies.push(prior.id.clone());
                let mut source_reports = Vec::new();
                let mut report_bytes = 0usize;
                for source_index in &source_followup_indices {
                    let Some(packet) = prior.evidence.get(*source_index) else {
                        return self.stop(
                            &mut state,
                            Lifecycle::Fail,
                            "SOURCE_EVIDENCE_INDEX_MISMATCH",
                            "取得本文の保存位置が前回結果と一致しません。",
                        );
                    };
                    let packet_bytes = canonical(packet)?;
                    let packet_ref = digest(&packet_bytes);
                    let retained = match self.store.get_bytes(actor, &request.case_id, &packet_ref)
                    {
                        Ok(value) => value,
                        Err(error) => {
                            return self.stop(
                                &mut state,
                                Lifecycle::Suspend,
                                "SOURCE_EVIDENCE_UNAVAILABLE",
                                &error.to_string(),
                            );
                        }
                    };
                    if retained != packet_bytes {
                        return self.stop(
                            &mut state,
                            Lifecycle::Fail,
                            "SOURCE_EVIDENCE_INTEGRITY_FAILURE",
                            "前回の取得本文artifactが保存済みpacketと一致しません。",
                        );
                    }
                    let node_id = format!("followup-source-{source_index}");
                    graph.observe(&node_id, &packet_ref, &BTreeMap::new())?;
                    let summary_result = if let Some((max_sentences, max_chars)) = summary_limits {
                        crate::text_summary::summarize_fetched_source_with_limits(
                            &prior.id,
                            packet,
                            &packet_ref,
                            max_sentences,
                            max_chars,
                        )
                    } else {
                        crate::text_summary::summarize_fetched_source(
                            &prior.id,
                            packet,
                            &packet_ref,
                        )
                    };
                    let source_report = match summary_result {
                        Ok(report) => report,
                        Err(error) => {
                            let code = match &error {
                                Error::Contract { code, .. } => code.as_str(),
                                _ => "SOURCE_SUMMARY_FAILED",
                            };
                            return self.stop(
                                &mut state,
                                Lifecycle::Suspend,
                                code,
                                &error.to_string(),
                            );
                        }
                    };
                    let encoded = canonical(&source_report)?;
                    report_bytes = report_bytes.saturating_add(encoded.len());
                    if report_bytes > request.budget.max_evidence_bytes {
                        return self.stop(
                            &mut state,
                            Lifecycle::Suspend,
                            "RESOURCE_LIMIT",
                            "出典付き要約artifactが要求の証拠予算を超えました。",
                        );
                    }
                    units.push(format!(
                        "出典: {}\n出典packet: {}\n取得本文SHA-256: {}\nライセンス情報: 未記録。\n{}",
                        source_report.source_final_uri,
                        source_report.source_packet_id,
                        source_report.raw_body_sha256,
                        source_report.summary.text
                    ));
                    source_reports.push(encoded);
                    result.dependencies.push(packet_ref);
                }
                for encoded in source_reports {
                    let report_ref = self.store.put_bytes(actor, &request.case_id, &encoded)?;
                    result.dependencies.push(report_ref);
                }
                result.guarantee = Guarantee::Hypothesis;
            }
            Intent::SummarizePreviousResult { .. } => {
                let Some((prior_id, content_ref, render_ref, artifact_bytes, output_unit)) =
                    &previous_result_summary
                else {
                    return self.stop(
                        &mut state,
                        Lifecycle::Suspend,
                        "PREVIOUS_RESULT_UNSUMMARIZABLE",
                        "直前結果の再検証済み要約候補を作成できません。",
                    );
                };
                let Some(prior) = previous.filter(|prior| &prior.id == prior_id) else {
                    return self.stop(
                        &mut state,
                        Lifecycle::Suspend,
                        "MISSING_RESULT_SUMMARY_CONTEXT",
                        "要約artifactの元結果が直前の採用済み結果と一致しません。",
                    );
                };
                let report_ref = self
                    .store
                    .put_bytes(actor, &request.case_id, artifact_bytes)?;
                graph.observe("previous-result-content", content_ref, &BTreeMap::new())?;
                graph.observe(
                    "previous-result-render",
                    render_ref,
                    &BTreeMap::from([("previous-result-content".into(), content_ref.clone())]),
                )?;
                graph.observe(
                    "previous-result-summary",
                    &report_ref,
                    &BTreeMap::from([
                        ("previous-result-content".into(), content_ref.clone()),
                        ("previous-result-render".into(), render_ref.clone()),
                    ]),
                )?;
                result.dependencies.push(report_ref);
                result.dependencies.extend([
                    prior.id.clone(),
                    content_ref.clone(),
                    render_ref.clone(),
                ]);
                result
                    .dependencies
                    .extend(prior.dependencies.iter().cloned());
                units.push(output_unit.clone());
                result.guarantee = Guarantee::Hypothesis;
            }
            Intent::AskPreviousSourceQuestion { .. } => {
                let Some(prior) = previous.filter(|result| result.lifecycle == Lifecycle::Commit)
                else {
                    return self.stop(
                        &mut state,
                        Lifecycle::Suspend,
                        "MISSING_ACTUAL_RESULT",
                        "質問対象となる実際の採用済み出典結果がありません。",
                    );
                };
                let Some((prior_id, source_packet_refs, answer)) = &previous_source_question_answer
                else {
                    return self.stop(
                        &mut state,
                        Lifecycle::Suspend,
                        "SOURCE_BODY_UNAVAILABLE",
                        "前回の結果に質問照合できる実本文がありません。",
                    );
                };
                if &prior.id != prior_id {
                    return self.stop(
                        &mut state,
                        Lifecycle::Fail,
                        "SOURCE_QUESTION_CONTEXT_MISMATCH",
                        "解釈時に固定した前回結果と質問に使う出典証拠が一致しません。",
                    );
                }
                let answer_bytes = canonical(answer)?;
                let output_units = crate::text_summary::source_question_answer_lines_v2(answer);
                let output_bytes = output_units.join("\n").len();
                if answer_bytes.len() > request.budget.max_evidence_bytes
                    || output_bytes > request.budget.max_evidence_bytes
                {
                    return self.stop(
                        &mut state,
                        Lifecycle::Suspend,
                        "RESOURCE_LIMIT",
                        "出典質問の回答artifactまたは本文候補が要求の証拠予算を超えました。",
                    );
                }
                let answer_ref = self
                    .store
                    .put_bytes(actor, &request.case_id, &answer_bytes)?;
                let mut answer_inputs = BTreeMap::new();
                for (index, packet_ref) in source_packet_refs {
                    let source_id = format!("previous-question-source-{index}");
                    graph.observe(&source_id, packet_ref, &BTreeMap::new())?;
                    answer_inputs.insert(source_id, packet_ref.clone());
                }
                graph.observe(
                    "previous-source-question-answer",
                    &answer_ref,
                    &answer_inputs,
                )?;
                result.dependencies.push(prior_id.clone());
                result.dependencies.extend(
                    source_packet_refs
                        .iter()
                        .map(|(_, reference)| reference.clone()),
                );
                result.dependencies.push(answer_ref);
                units.extend(output_units);
                result.guarantee = Guarantee::Hypothesis;
            }
            Intent::CorrectPremise {
                superseded,
                replacement,
            } => {
                let mut memory = self.store.load_memory(actor)?;
                let targets = active_direct_premise_ids(&memory, superseded);
                if targets.is_empty() {
                    return self.stop(
                        &mut state,
                        Lifecycle::Suspend,
                        "MISSING_CORRECTION_TARGET",
                        "この訂正に一致する有効な同一範囲の提示前提がありません。記憶は変更していません。",
                    );
                }

                let (dependencies, invalidated_count) = apply_premise_correction(
                    &self.store,
                    actor,
                    &mut memory,
                    &targets,
                    superseded,
                    replacement,
                    &fingerprint,
                )?;
                let negative_superseded = Proposition {
                    negative: true,
                    ..superseded.clone()
                };
                units.push(format!(
                    "訂正を会話内の提示前提として記録しました: {}。",
                    describe(&negative_superseded)
                ));
                units.push(format!("置き換え後の提示前提: {}。", describe(replacement)));
                units.push(format!(
                    "以前の前提とその依存記録を{}件失効させ、元の記録は保持しました。",
                    invalidated_count
                ));
                units.push("現実世界の真偽を検証したものではありません。".into());
                result.dependencies = dependencies;
                result.guarantee = Guarantee::UserPremise;
            }
            Intent::CorrectPreviousPremise {
                predicate,
                superseded_object,
                replacement_object,
                scope,
            } => {
                let Some(prior) = previous.filter(|prior| prior.lifecycle == Lifecycle::Commit)
                else {
                    return self.stop(
                        &mut state,
                        Lifecycle::Suspend,
                        "MISSING_CORRECTION_CONTEXT",
                        "代名詞の対象を決める直前の同一ケース確定回答がありません。記憶は変更していません。",
                    );
                };
                if bundle.meaning.context_result_ref.as_deref() != Some(prior.id.as_str()) {
                    return self.stop(
                        &mut state,
                        Lifecycle::Suspend,
                        "MISSING_CORRECTION_CONTEXT",
                        "訂正の文脈参照が直前の同一ケース確定回答と一致しません。記憶は変更していません。",
                    );
                }

                let mut memory = self.store.load_memory(actor)?;
                let prior_dependencies: std::collections::BTreeSet<_> =
                    prior.dependencies.iter().map(String::as_str).collect();
                let candidates: std::collections::BTreeSet<_> = memory
                    .facts
                    .iter()
                    .filter(|fact| {
                        fact.valid
                            && fact.guarantee == Guarantee::UserPremise
                            && fact.dependencies.is_empty()
                            && !fact.source_input.is_empty()
                            && !fact.proposition.negative
                            && fact.proposition.predicate == *predicate
                            && fact.proposition.object == *superseded_object
                            && scope
                                .as_ref()
                                .is_none_or(|scope| fact.proposition.scope == *scope)
                            && prior_dependencies.contains(fact.id.as_str())
                    })
                    .map(|fact| fact.proposition.clone())
                    .collect();
                if candidates.is_empty() {
                    return self.stop(
                        &mut state,
                        Lifecycle::Suspend,
                        "MISSING_CORRECTION_TARGET",
                        "直前の回答が参照した有効な同一範囲の提示前提に訂正対象がありません。記憶は変更していません。",
                    );
                }
                if candidates.len() != 1 {
                    return self.stop(
                        &mut state,
                        Lifecycle::Suspend,
                        "AMBIGUOUS_CORRECTION_CONTEXT",
                        "直前の回答が同じ訂正文に一致する複数の対象を参照しています。範囲を明示して再質問してください。記憶は変更していません。",
                    );
                }
                let superseded = candidates.into_iter().next().expect("one candidate");
                let replacement = Proposition {
                    object: replacement_object.clone(),
                    negative: false,
                    ..superseded.clone()
                };
                let targets = active_direct_premise_ids(&memory, &superseded);
                if targets.is_empty() {
                    return self.stop(
                        &mut state,
                        Lifecycle::Suspend,
                        "MISSING_CORRECTION_TARGET",
                        "直前の回答が示した前提は既に有効な訂正対象ではありません。記憶は変更していません。",
                    );
                }

                let (dependencies, invalidated_count) = apply_premise_correction(
                    &self.store,
                    actor,
                    &mut memory,
                    &targets,
                    &superseded,
                    &replacement,
                    &fingerprint,
                )?;
                let negative_superseded = Proposition {
                    negative: true,
                    ..superseded.clone()
                };
                units.push(format!(
                    "直前の回答が参照した提示前提IDから訂正対象を特定しました: {}。",
                    describe(&superseded)
                ));
                units.push(format!(
                    "訂正を会話内の提示前提として記録しました: {}。",
                    describe(&negative_superseded)
                ));
                units.push(format!(
                    "置き換え後の提示前提: {}。",
                    describe(&replacement)
                ));
                units.push(format!(
                    "以前の前提とその依存記録を{}件失効させ、元の記録は保持しました。",
                    invalidated_count
                ));
                units.push("現実世界の真偽を検証したものではありません。".into());
                result.dependencies = dependencies;
                result.guarantee = Guarantee::UserPremise;
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
            Intent::AssertPreviousRelation {
                proposition,
                explicit_scope,
            } => {
                let Some(prior) = previous.filter(|prior| prior.lifecycle == Lifecycle::Commit)
                else {
                    return self.stop(
                        &mut state,
                        Lifecycle::Suspend,
                        "MISSING_RELATION_CONTEXT",
                        "代名詞主語を解決できる直前の同一ケース確定関係結果がありません。",
                    );
                };
                if bundle.meaning.context_result_ref.as_deref() != Some(prior.id.as_str()) {
                    return self.stop(
                        &mut state,
                        Lifecycle::Suspend,
                        "MISSING_RELATION_CONTEXT",
                        "前提追加の文脈参照が直前の同一ケース確定結果と一致しません。",
                    );
                }
                let mut memory = self.store.load_memory(actor)?;
                let previous_assertion_fact = previous_contextual_assertion_fact(
                    &self.store,
                    actor,
                    &request.case_id,
                    prior,
                    &memory,
                )?;
                let (anchor, expected_proof_dependencies, adopted_conclusion, assertion_fact_id) =
                    if let Some(fact) = previous_assertion_fact {
                        if !fact.valid {
                            return self.stop(
                                &mut state,
                                Lifecycle::Suspend,
                                "CONTEXT_PROOF_CHANGED",
                                "直前の代名詞前提が現在の記憶で無効です。新しい関係結果を確認してください。",
                            );
                        }
                        if input::singular_previous_subject(&fact.proposition.subject) {
                            return self.stop(
                                &mut state,
                                Lifecycle::Suspend,
                                "MISSING_RELATION_CONTEXT",
                                "直前の前提自体に未解決の代名詞主語が残っています。主語を推測しません。",
                            );
                        }
                        (
                            fact.proposition.clone(),
                            vec![fact.id.clone()],
                            false,
                            Some(fact.id),
                        )
                    } else {
                        let Some(prior_meaning) =
                            previous_relation_meaning(&self.store, actor, &request.case_id, prior)?
                        else {
                            return self.stop(
                                &mut state,
                                Lifecycle::Suspend,
                                "MISSING_RELATION_CONTEXT",
                                "直前の確定結果に一意の型付き関係主語がありません。記憶から主語を推測しません。",
                            );
                        };
                        match prior_meaning {
                            PriorRelationMeaning::OriginalQuestion(proposition) => {
                                (proposition, prior.dependencies.clone(), false, None)
                            }
                            PriorRelationMeaning::AdoptedConclusion {
                                proposition,
                                proof_dependencies,
                            } => (proposition, proof_dependencies, true, None),
                        }
                    };
                let answer = relations::query(&memory, &anchor, request.budget.max_actions)?;
                let proof_dependencies = relations::answer_dependencies(&answer);
                let answer_guarantee = relations::answer_guarantee(&answer);
                let relation_still_has_a_result =
                    relation_explanation_target(&anchor, &answer).is_some();
                let proof_lineage_matches = if let Some(fact_id) = assertion_fact_id {
                    answer.status == "SUPPORTED"
                        && proof_dependencies == [fact_id]
                        && answer_guarantee == Guarantee::UserPremise
                } else {
                    proof_dependencies == expected_proof_dependencies
                        && proof_dependencies == prior.dependencies
                        && answer_guarantee == prior.guarantee
                        && (!adopted_conclusion || answer.status == "SUPPORTED")
                };
                if !relation_still_has_a_result || !proof_lineage_matches {
                    return self.stop(
                        &mut state,
                        Lifecycle::Suspend,
                        "CONTEXT_PROOF_CHANGED",
                        "直前の関係結果を現在有効な根拠で再確認できません。新しい関係結果を確認してください。",
                    );
                }
                let resolved = Proposition {
                    subject: anchor.subject.clone(),
                    scope: explicit_scope
                        .clone()
                        .unwrap_or_else(|| anchor.scope.clone()),
                    ..proposition.clone()
                };
                let fact = relations::assert_fact(&mut memory, resolved.clone(), &fingerprint)?;
                self.store.save_memory(actor, &memory)?;
                units.push(format!(
                    "直前結果 {} の関係根拠を再確認し、主語「{}」とscope「{}」を復元しました。",
                    prior.id, resolved.subject, resolved.scope
                ));
                units.push(format!(
                    "前提を保存しました: {}。",
                    describe(&fact.proposition)
                ));
                units.push(format!("前提ID: {}", fact.id));
                units.push(
                    "保証範囲: 使用者が提示した前提。現実世界の真偽を検証したものではありません。"
                        .into(),
                );
                result.dependencies.push(prior.id.clone());
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
            Intent::AddConjunctiveRule {
                premises,
                conclusion,
            } => {
                if premises.len() != 2
                    || premises.iter().any(|premise| {
                        premise.subject != premises[0].subject || premise.scope != premises[0].scope
                    })
                {
                    return self.stop(
                        &mut state,
                        Lifecycle::Reject,
                        "INVALID_CONJUNCTIVE_RULE",
                        "二前提規則には同じ主語とscopeの関係節が必要です。",
                    );
                }
                let mut memory = self.store.load_memory(actor)?;
                let rule = relations::add_rule_with_premises(
                    &mut memory,
                    premises.clone(),
                    conclusion.clone(),
                    &fingerprint,
                )?;
                self.store.save_memory(actor, &memory)?;
                units.push(format!(
                    "二つの条件付き前提を保存しました: {} かつ {} ならば {}。",
                    describe(&premises[0]),
                    describe(&premises[1]),
                    describe(conclusion)
                ));
                units.push(format!("規則ID: {}", rule.id));
                units.push("保証範囲: 提示された二条件の組み合わせの中でのみ利用します。普遍的事実として学習しません。".into());
                result.dependencies.push(rule.id);
                result.guarantee = Guarantee::UserPremise;
            }
            Intent::AskConjunction { propositions } => {
                if !(2..=3).contains(&propositions.len()) {
                    return self.stop(
                        &mut state,
                        Lifecycle::Reject,
                        "INVALID_CONJUNCTION_QUERY",
                        "この質問経路は二つまたは三つの関係節を受け付けます。",
                    );
                }
                let relation_count = match propositions.len() {
                    2 => "二つ",
                    3 => "三つ",
                    _ => unreachable!("the conjunction length was bounded above"),
                };
                let first = &propositions[0];
                if propositions.iter().any(|proposition| {
                    proposition.subject != first.subject || proposition.scope != first.scope
                }) {
                    return self.stop(
                        &mut state,
                        Lifecycle::Reject,
                        "INVALID_CONJUNCTION_QUERY",
                        "すべての関係節で主語とscopeが一致しません。",
                    );
                }
                let clause_count =
                    u32::try_from(propositions.len()).expect("the conjunction length is bounded");
                let per_relation_budget = request.budget.max_actions / clause_count;
                if per_relation_budget == 0 {
                    return self.stop(
                        &mut state,
                        Lifecycle::Suspend,
                        "RESOURCE_LIMIT",
                        "すべての関係を確認するための推論予算が不足しています。",
                    );
                }
                let memory = self.store.load_memory(actor)?;
                let mut dependencies = BTreeSet::new();
                let mut all_supported = true;
                let mut any_derived = false;
                let mut conclusions = Vec::with_capacity(propositions.len());
                for proposition in propositions {
                    let answer = relations::query(&memory, proposition, per_relation_budget)?;
                    match answer.status.as_str() {
                        "SUPPORTED" => conclusions
                            .push(format!("- {}", relation_answer_surface(proposition, false))),
                        "CONTRADICTED" => {
                            all_supported = false;
                            conclusions
                                .push(format!("- {}", relation_answer_surface(proposition, true)));
                        }
                        "CONFLICT" => {
                            return self.stop(
                                &mut state,
                                Lifecycle::Suspend,
                                "CONFLICTING_PREMISES",
                                &format!(
                                    "範囲「{}」の「{}」に肯定・否定の競合があります。個別の根拠を確認してください。",
                                    proposition.scope,
                                    describe(proposition)
                                ),
                            );
                        }
                        _ => {
                            return self.stop(
                                &mut state,
                                Lifecycle::Suspend,
                                "MISSING_PREMISES",
                                &format!(
                                    "範囲「{}」の「{}」を判定する有効な前提または導出経路がありません。",
                                    proposition.scope,
                                    describe(proposition)
                                ),
                            );
                        }
                    }
                    dependencies.extend(relations::answer_dependencies(&answer));
                    any_derived |= relations::answer_guarantee(&answer) == Guarantee::Derived;
                }
                if all_supported {
                    units.push(format!(
                        "提示された前提の範囲では、尋ねた{relation_count}の関係がともに支持されます。"
                    ));
                } else {
                    units.push(format!(
                        "提示された根拠は、尋ねた{relation_count}の関係がともに成立するという組み合わせを支持しません。"
                    ));
                }
                units.extend(conclusions);
                result.dependencies = dependencies.into_iter().collect();
                result.guarantee = if any_derived {
                    Guarantee::Derived
                } else {
                    Guarantee::UserPremise
                };
            }
            Intent::CompareRelationInventories {
                subject_a,
                subject_b,
                scope,
            } => {
                let per_subject_actions = request.budget.max_actions / 2;
                if per_subject_actions == 0 {
                    return self.stop(
                        &mut state,
                        Lifecycle::Suspend,
                        "RESOURCE_LIMIT",
                        "二つの対象の関係を調べるための推論予算がありません。",
                    );
                }
                let memory = self.store.load_memory(actor)?;
                let inventory_a = match relation_comparison_inventory(
                    &memory,
                    subject_a,
                    scope,
                    per_subject_actions,
                ) {
                    Ok(value) => value,
                    Err(Error::Contract { code, message }) if code == "RELATION_BUDGET" => {
                        return self.stop(
                            &mut state,
                            Lifecycle::Suspend,
                            "RESOURCE_LIMIT",
                            &message,
                        );
                    }
                    Err(Error::Contract { code, message }) => {
                        return self.stop(&mut state, Lifecycle::Suspend, &code, &message);
                    }
                    Err(error) => return Err(error),
                };
                let inventory_b = match relation_comparison_inventory(
                    &memory,
                    subject_b,
                    scope,
                    per_subject_actions,
                ) {
                    Ok(value) => value,
                    Err(Error::Contract { code, message }) if code == "RELATION_BUDGET" => {
                        return self.stop(
                            &mut state,
                            Lifecycle::Suspend,
                            "RESOURCE_LIMIT",
                            &message,
                        );
                    }
                    Err(Error::Contract { code, message }) => {
                        return self.stop(&mut state, Lifecycle::Suspend, &code, &message);
                    }
                    Err(error) => return Err(error),
                };
                if inventory_a.facts.is_empty() && inventory_b.facts.is_empty() {
                    return self.stop(
                        &mut state,
                        Lifecycle::Suspend,
                        "MISSING_PREMISES",
                        "このscopeで二つの対象について比較できる支持記録がありません。共通点や差異がないとは断定しません。",
                    );
                }
                let report = relation_comparison_report(
                    subject_a,
                    subject_b,
                    scope,
                    &inventory_a,
                    &inventory_b,
                );
                let artifact = RelationComparisonReportArtifact {
                    kind: RELATION_COMPARISON_REPORT_KIND.into(),
                    result_id: result.id.clone(),
                    report_sha256: digest(report.as_bytes()),
                    subject_a: subject_a.clone(),
                    subject_b: subject_b.clone(),
                    scope: scope.clone(),
                    inventory_a: inventory_a.clone(),
                    inventory_b: inventory_b.clone(),
                };
                let artifact_bytes = canonical(&artifact)?;
                if report.len().saturating_add(artifact_bytes.len())
                    > request.budget.max_evidence_bytes
                {
                    return self.stop(
                        &mut state,
                        Lifecycle::Suspend,
                        "RESOURCE_LIMIT",
                        "関係比較の出力と再検証artifactが要求の証拠予算を超えました。",
                    );
                }
                let report_ref = self
                    .store
                    .put_bytes(actor, &request.case_id, &artifact_bytes)?;
                units.push(report);
                result.dependencies.push(report_ref);
                result.dependencies.extend(
                    inventory_a
                        .dependencies
                        .iter()
                        .chain(&inventory_b.dependencies)
                        .cloned()
                        .collect::<BTreeSet<_>>(),
                );
                // The report contrasts only the saved record set; omitted facts
                // and outside-world truth have not been checked.
                result.guarantee = Guarantee::Hypothesis;
            }
            Intent::AskCommonRelations {
                subject_a,
                subject_b,
                scope,
            } => {
                let memory = self.store.load_memory(actor)?;
                let scanned_records = memory
                    .facts
                    .len()
                    .checked_add(memory.rules.len())
                    .ok_or_else(|| {
                        failure(
                            "RESOURCE_LIMIT",
                            "common-relation memory scan count overflowed its bounded representation",
                        )
                    })?;
                let action_budget =
                    usize::try_from(request.budget.max_actions).unwrap_or(usize::MAX);
                if scanned_records > action_budget {
                    return self.stop(
                        &mut state,
                        Lifecycle::Suspend,
                        "RESOURCE_LIMIT",
                        "保存済みの関係を比較候補として走査する量が推論予算を超えました。",
                    );
                }
                let scanned_actions = u32::try_from(scanned_records).map_err(|_| {
                    failure(
                        "RESOURCE_LIMIT",
                        "common-relation scan count exceeds the bounded action representation",
                    )
                })?;
                let remaining_actions = request.budget.max_actions.saturating_sub(scanned_actions);
                let mut candidates = BTreeSet::new();
                let includes_subject = |subject: &str| {
                    subject == subject_a
                        || subject == subject_b
                        || subject.starts_with('$')
                        || subject.starts_with('?')
                };
                let mut include_candidate = |proposition: &Proposition| {
                    if proposition.scope == *scope
                        && includes_subject(&proposition.subject)
                        && !proposition.predicate.starts_with(['$', '?'])
                        && !proposition.object.starts_with(['$', '?'])
                    {
                        candidates.insert((
                            proposition.predicate.clone(),
                            proposition.object.clone(),
                            proposition.negative,
                        ));
                    }
                };
                for fact in &memory.facts {
                    if fact.valid
                        && !matches!(fact.guarantee, Guarantee::Hypothesis | Guarantee::Unknown)
                    {
                        include_candidate(&fact.proposition);
                    }
                }
                for rule in memory.rules.iter().filter(|rule| rule.valid) {
                    for proposition in std::iter::once(&rule.premise)
                        .chain(rule.additional_premises.iter())
                        .chain(std::iter::once(&rule.conclusion))
                    {
                        include_candidate(proposition);
                    }
                }
                drop(include_candidate);
                if candidates.is_empty() {
                    return self.stop(
                        &mut state,
                        Lifecycle::Suspend,
                        "MISSING_PREMISES",
                        "二つの対象について比較できる保存済み関係がありません。共通点がないとは断定しません。",
                    );
                }
                let candidate_count = u32::try_from(candidates.len()).map_err(|_| {
                    failure(
                        "RESOURCE_LIMIT",
                        "common-relation candidate count exceeds the bounded representation",
                    )
                })?;
                let query_count = candidate_count.checked_mul(2).ok_or_else(|| {
                    failure(
                        "RESOURCE_LIMIT",
                        "common-relation query count exceeds the bounded action representation",
                    )
                })?;
                if query_count > remaining_actions {
                    return self.stop(
                        &mut state,
                        Lifecycle::Suspend,
                        "RESOURCE_LIMIT",
                        "各比較候補を両方の対象について照合する推論予算が不足しています。",
                    );
                }
                let per_query_budget = remaining_actions / query_count;
                if per_query_budget == 0 {
                    return self.stop(
                        &mut state,
                        Lifecycle::Suspend,
                        "RESOURCE_LIMIT",
                        "関係比較を最後まで行う推論予算がありません。",
                    );
                }

                let mut common = Vec::new();
                let mut shared_proofs = Vec::new();
                let mut dependencies = BTreeSet::new();
                let mut any_derived = false;
                for (predicate, object, negative) in candidates {
                    let proposition_a = Proposition {
                        subject: subject_a.clone(),
                        predicate: predicate.clone(),
                        object: object.clone(),
                        negative,
                        scope: scope.clone(),
                    };
                    let proposition_b = Proposition {
                        subject: subject_b.clone(),
                        ..proposition_a.clone()
                    };
                    let answer_a = relations::query(&memory, &proposition_a, per_query_budget)?;
                    let answer_b = relations::query(&memory, &proposition_b, per_query_budget)?;
                    if answer_a.status == "CONFLICT" || answer_b.status == "CONFLICT" {
                        return self.stop(
                            &mut state,
                            Lifecycle::Suspend,
                            "CONFLICTING_PREMISES",
                            "比較対象のいずれかに肯定・否定の競合があります。共通関係を選択せず停止します。",
                        );
                    }
                    if !matches!(
                        answer_a.status.as_str(),
                        "SUPPORTED" | "CONTRADICTED" | "UNKNOWN"
                    ) || !matches!(
                        answer_b.status.as_str(),
                        "SUPPORTED" | "CONTRADICTED" | "UNKNOWN"
                    ) {
                        return self.stop(
                            &mut state,
                            Lifecycle::Suspend,
                            "UNSUPPORTED_RELATION_RESULT",
                            "比較対象の関係照合が既知の終端状態を返しませんでした。",
                        );
                    }
                    if answer_a.status != "SUPPORTED" || answer_b.status != "SUPPORTED" {
                        continue;
                    }
                    let guarantee_a = relations::answer_guarantee(&answer_a);
                    let guarantee_b = relations::answer_guarantee(&answer_b);
                    let dependencies_a = relations::answer_dependencies(&answer_a);
                    let dependencies_b = relations::answer_dependencies(&answer_b);
                    dependencies.extend(dependencies_a.iter().cloned());
                    dependencies.extend(dependencies_b.iter().cloned());
                    any_derived |=
                        guarantee_a == Guarantee::Derived || guarantee_b == Guarantee::Derived;
                    shared_proofs.push(CommonRelationProofArtifact {
                        proposition_a: proposition_a.clone(),
                        proposition_b: proposition_b.clone(),
                        guarantee_a: guarantee_a.clone(),
                        guarantee_b: guarantee_b.clone(),
                        dependencies_a,
                        dependencies_b,
                    });
                    common.push((
                        proposition_a,
                        proposition_b,
                        answer_a,
                        answer_b,
                        guarantee_a,
                        guarantee_b,
                    ));
                }
                if common.is_empty() {
                    return self.stop(
                        &mut state,
                        Lifecycle::Suspend,
                        "MISSING_PREMISES",
                        "保存済みの根拠から両対象に共通して支持される関係を確認できません。共通点が存在しないとは断定しません。",
                    );
                }
                let origin = |guarantee: &Guarantee| match guarantee {
                    Guarantee::UserPremise => "直接提示",
                    Guarantee::Derived => "規則導出",
                    _ => "根拠付き",
                };
                let mut report = vec![COMMON_RELATIONS_REPORT_HEADER.to_string()];
                for (proposition_a, proposition_b, answer_a, answer_b, guarantee_a, guarantee_b) in
                    common
                {
                    report.push(format!(
                        "共通関係: {}（{}）／{}（{}）",
                        describe(&proposition_a),
                        origin(&guarantee_a),
                        describe(&proposition_b),
                        origin(&guarantee_b)
                    ));
                    report.extend(relation_explanation_lines(
                        &memory,
                        &proposition_a,
                        &answer_a,
                        &format!("{}側の根拠", subject_a),
                    ));
                    report.extend(relation_explanation_lines(
                        &memory,
                        &proposition_b,
                        &answer_b,
                        &format!("{}側の根拠", subject_b),
                    ));
                }
                let report = report.join("\n");
                let overall_guarantee = if any_derived {
                    Guarantee::Derived
                } else {
                    Guarantee::UserPremise
                };
                let artifact = CommonRelationsReportArtifact {
                    kind: COMMON_RELATIONS_REPORT_KIND.into(),
                    result_id: result.id.clone(),
                    report_sha256: digest(report.as_bytes()),
                    subject_a: subject_a.clone(),
                    subject_b: subject_b.clone(),
                    scope: scope.clone(),
                    shared_proofs,
                };
                let artifact_bytes = canonical(&artifact)?;
                if report.len().saturating_add(artifact_bytes.len())
                    > request.budget.max_evidence_bytes
                {
                    return self.stop(
                        &mut state,
                        Lifecycle::Suspend,
                        "RESOURCE_LIMIT",
                        "共通関係の出力と再検証artifactが要求の証拠予算を超えました。",
                    );
                }
                let report_ref = self
                    .store
                    .put_bytes(actor, &request.case_id, &artifact_bytes)?;
                units.push(report);
                result.dependencies.push(report_ref);
                result.dependencies.extend(dependencies);
                result.guarantee = overall_guarantee;
            }
            Intent::AskPreviousRelation {
                proposition,
                explain,
                explicit_scope,
            } => {
                let Some(prior) = previous.filter(|prior| prior.lifecycle == Lifecycle::Commit)
                else {
                    return self.stop(
                        &mut state,
                        Lifecycle::Suspend,
                        "MISSING_RELATION_CONTEXT",
                        "代名詞の対象を解決できる直前の同一ケース確定回答がありません。actor記憶から対象を推測しません。",
                    );
                };
                if bundle.meaning.context_result_ref.as_deref() != Some(prior.id.as_str()) {
                    return self.stop(
                        &mut state,
                        Lifecycle::Suspend,
                        "MISSING_RELATION_CONTEXT",
                        "関係質問の文脈参照が直前の同一ケース確定回答と一致しません。",
                    );
                }

                let memory = self.store.load_memory(actor)?;
                let prior_dependencies: std::collections::BTreeSet<_> =
                    prior.dependencies.iter().map(String::as_str).collect();
                let candidates: Vec<_> = memory
                    .facts
                    .iter()
                    .filter(|fact| {
                        fact.valid
                            && fact.guarantee == Guarantee::UserPremise
                            && fact.dependencies.is_empty()
                            && !fact.source_input.is_empty()
                            && prior_dependencies.contains(fact.id.as_str())
                            && explicit_scope
                                .as_ref()
                                .is_none_or(|scope| fact.proposition.scope == *scope)
                    })
                    .collect();
                if candidates.is_empty() {
                    return self.stop(
                        &mut state,
                        Lifecycle::Suspend,
                        "MISSING_RELATION_CONTEXT",
                        "直前の回答が参照する有効な直接前提がありません。明示したscopeを含め、代名詞の対象を推測しません。",
                    );
                }
                if candidates.len() != 1 {
                    return self.stop(
                        &mut state,
                        Lifecycle::Suspend,
                        "AMBIGUOUS_RELATION_CONTEXT",
                        "直前の回答は複数の直接前提を参照しています。代名詞の対象を一意に決められません。",
                    );
                }
                let anchor = &candidates[0].proposition;
                let resolved = Proposition {
                    subject: anchor.subject.clone(),
                    scope: explicit_scope
                        .clone()
                        .unwrap_or_else(|| anchor.scope.clone()),
                    ..proposition.clone()
                };
                let answer = relations::query(&memory, &resolved, request.budget.max_actions)?;
                let answer_dependencies = relations::answer_dependencies(&answer);
                let answer_guarantee = relations::answer_guarantee(&answer);
                if *explain {
                    let explained = match answer.status.as_str() {
                        "SUPPORTED" => resolved.clone(),
                        "CONTRADICTED" => {
                            let mut opposite = resolved.clone();
                            opposite.negative = !opposite.negative;
                            units.push(relation_answer_surface(&resolved, true));
                            opposite
                        }
                        "CONFLICT" => {
                            return self.stop(
                                &mut state,
                                Lifecycle::Suspend,
                                "CONFLICTING_PREMISES",
                                &format!(
                                    "範囲「{}」の「{}」に肯定と否定の根拠があります。一方を恣意的に採用しません。",
                                    resolved.scope,
                                    describe(&resolved)
                                ),
                            );
                        }
                        _ => {
                            return self.stop(
                                &mut state,
                                Lifecycle::Suspend,
                                "MISSING_PREMISES",
                                &format!(
                                    "範囲「{}」の「{}」を説明する有効な前提または導出経路がありません。",
                                    resolved.scope,
                                    describe(&resolved)
                                ),
                            );
                        }
                    };
                    units.extend(relation_explanation_lines(
                        &memory,
                        &explained,
                        &answer,
                        "提示された証拠が支える結論",
                    ));
                } else {
                    match answer.status.as_str() {
                        "SUPPORTED" => units.push(relation_answer_surface(&resolved, false)),
                        "CONTRADICTED" => {
                            units.push(relation_answer_surface(&resolved, true));
                        }
                        "CONFLICT" => {
                            return self.stop(
                                &mut state,
                                Lifecycle::Suspend,
                                "CONFLICTING_PREMISES",
                                &format!(
                                    "範囲「{}」の「{}」に肯定と否定の根拠があります。一方を恣意的に採用しません。",
                                    resolved.scope,
                                    describe(&resolved)
                                ),
                            );
                        }
                        _ => {
                            return self.stop(
                                &mut state,
                                Lifecycle::Suspend,
                                "MISSING_PREMISES",
                                &format!(
                                    "範囲「{}」の「{}」を判定する有効な前提または導出経路がありません。",
                                    resolved.scope,
                                    describe(&resolved)
                                ),
                            );
                        }
                    }
                }
                result.dependencies = answer_dependencies;
                result.guarantee = answer_guarantee;
                let Some(adopted_meaning) = relation_explanation_target(&resolved, &answer) else {
                    return self.stop(
                        &mut state,
                        Lifecycle::Suspend,
                        "MISSING_PREMISES",
                        "採用した関係結論を型付き命題として確定できません。",
                    );
                };
                adopted_relation_meaning = Some(adopted_meaning);
                relation_proof_dependencies = Some(result.dependencies.clone());
            }
            Intent::Ask { proposition } => {
                let memory = self.store.load_memory(actor)?;
                let answer = relations::query(&memory, proposition, request.budget.max_actions)?;
                let answer_dependencies = relations::answer_dependencies(&answer);
                let answer_guarantee = relations::answer_guarantee(&answer);
                match answer.status.as_str() {
                    "SUPPORTED"=>{
                        units.push(relation_answer_surface(proposition, false));
                        result.dependencies.extend(answer_dependencies.iter().cloned());
                        result.guarantee=answer_guarantee;
                    },
                    "CONTRADICTED"=>{
                        units.push(relation_answer_surface(proposition, true));
                        result.dependencies.extend(answer_dependencies);
                        result.guarantee=answer_guarantee;
                    },
                    "CONFLICT"=>return self.stop(&mut state,Lifecycle::Suspend,"CONFLICTING_PREMISES",
                        &format!("範囲「{}」の「{}」に肯定と否定の根拠があります。一方を恣意的に採用しません。",proposition.scope,describe(proposition))),
                    _=>return self.stop(&mut state,Lifecycle::Suspend,"MISSING_PREMISES",
                        &format!("範囲「{}」の「{}」を判定する有効な前提または導出経路がありません。",proposition.scope,describe(proposition))),
                }
            }
            Intent::AskRelationObjects {
                subject,
                predicate,
                negative,
                scope,
            } => {
                let memory = self.store.load_memory(actor)?;
                let answer = match relation_objects_answer(
                    &memory,
                    subject,
                    predicate,
                    *negative,
                    scope,
                    request.budget.max_actions,
                    request.budget.max_evidence_bytes,
                ) {
                    Ok(answer) => answer,
                    Err(error) => {
                        let code = match &error {
                            Error::Contract { code, .. } => code.as_str(),
                            _ => "RELATION_QUERY_UNAVAILABLE",
                        };
                        return self.stop(&mut state, Lifecycle::Suspend, code, &error.to_string());
                    }
                };
                units.push(answer.text);
                result.dependencies = answer.dependencies;
                result.guarantee = answer.guarantee;
            }
            Intent::AskWhy { proposition } => {
                let memory = self.store.load_memory(actor)?;
                let answer = relations::query(&memory, proposition, request.budget.max_actions)?;
                let answer_dependencies = relations::answer_dependencies(&answer);
                let answer_guarantee = relations::answer_guarantee(&answer);
                match answer.status.as_str() {
                    "SUPPORTED" | "CONTRADICTED" => {}
                    "CONFLICT" => {
                        return self.stop(
                            &mut state,
                            Lifecycle::Suspend,
                            "CONFLICTING_PREMISES",
                            &format!(
                                "範囲「{}」の「{}」に肯定と否定の根拠があります。一方を恣意的に採用しません。",
                                proposition.scope,
                                describe(proposition)
                            ),
                        );
                    }
                    _ => {
                        return self.stop(
                            &mut state,
                            Lifecycle::Suspend,
                            "MISSING_PREMISES",
                            &format!(
                                "範囲「{}」の「{}」を説明する有効な前提または導出経路がありません。",
                                proposition.scope,
                                describe(proposition)
                            ),
                        );
                    }
                }
                let explanation_units =
                    relation_why_explanation_units(&memory, proposition, &answer)?;
                approved_surfaces =
                    Some(language::relation_explanation_surfaces(&explanation_units));
                units.extend(explanation_units);
                result.dependencies = answer_dependencies;
                result.guarantee = answer_guarantee;
            }
            Intent::InspectPremises { subject } => {
                let memory = self.store.load_memory(actor)?;
                let scanned_records = memory
                    .facts
                    .len()
                    .checked_add(memory.rules.len())
                    .ok_or_else(|| {
                        failure(
                            "RESOURCE_LIMIT",
                            "relation inventory scan count overflowed its bounded representation",
                        )
                    })?;
                let action_budget =
                    usize::try_from(request.budget.max_actions).unwrap_or(usize::MAX);
                if scanned_records > action_budget {
                    return self.stop(
                        &mut state,
                        Lifecycle::Suspend,
                        "RESOURCE_LIMIT",
                        "保存済み関係を列挙する走査量が要求の推論予算を超えました。",
                    );
                }
                let remaining_actions = request
                    .budget
                    .max_actions
                    .saturating_sub(u32::try_from(scanned_records).unwrap_or(u32::MAX));
                let query_shapes = match relations::query_shapes_about(
                    &memory,
                    subject,
                    usize::try_from(remaining_actions).unwrap_or(usize::MAX),
                ) {
                    Ok(shapes) => shapes,
                    Err(Error::Contract { code, .. }) if code == "RELATION_BUDGET" => {
                        return self.stop(
                            &mut state,
                            Lifecycle::Suspend,
                            "RESOURCE_LIMIT",
                            "対象について確認する関係候補が要求の推論予算を超えました。",
                        );
                    }
                    Err(error) => return Err(error),
                };
                if query_shapes.is_empty() {
                    return self.stop(
                        &mut state,
                        Lifecycle::Suspend,
                        "MISSING_PREMISES",
                        "この対象について有効な提示前提または規則導出はありません。",
                    );
                }
                let query_count = u32::try_from(query_shapes.len()).map_err(|_| {
                    failure(
                        "RESOURCE_LIMIT",
                        "relation query count exceeds the bounded representation",
                    )
                })?;
                let per_query_budget = remaining_actions / query_count;
                if per_query_budget == 0 {
                    return self.stop(
                        &mut state,
                        Lifecycle::Suspend,
                        "RESOURCE_LIMIT",
                        "対象のすべての関係を確認する推論予算がありません。",
                    );
                }

                let disclosure = format!(
                    "会話に保存された「{subject}」に関する有効な関係です。保存記録の範囲に限った列挙で、現実世界の真偽や全件網羅性を確認したものではありません。"
                );
                let mut evidence_bytes = disclosure.len();
                if evidence_bytes > request.budget.max_evidence_bytes {
                    return self.stop(
                        &mut state,
                        Lifecycle::Suspend,
                        "RESOURCE_LIMIT",
                        "保存済み関係の一覧が要求の証拠予算を超えました。",
                    );
                }
                let mut lines = vec![disclosure];
                let mut reported_facts = BTreeMap::new();
                let mut contested_fact_ids = BTreeSet::new();
                let mut dependencies = BTreeSet::new();
                let mut has_conflict = false;
                for shape in query_shapes {
                    let answer = match relations::query(&memory, &shape, per_query_budget) {
                        Ok(answer) => answer,
                        Err(Error::Contract { code, .. }) if code == "RELATION_BUDGET" => {
                            return self.stop(
                                &mut state,
                                Lifecycle::Suspend,
                                "RESOURCE_LIMIT",
                                "保存済み関係を導出する計算が要求の推論予算を超えました。",
                            );
                        }
                        Err(error) => return Err(error),
                    };
                    match answer.status.as_str() {
                        "SUPPORTED" | "CONTRADICTED" => {}
                        "CONFLICT" => has_conflict = true,
                        "UNKNOWN" | "MISSING_PREMISES" => continue,
                        _ => {
                            return self.stop(
                                &mut state,
                                Lifecycle::Suspend,
                                "UNSUPPORTED_RELATION_RESULT",
                                "関係の列挙が既知の終端状態を返しませんでした。",
                            );
                        }
                    }
                    dependencies.extend(relations::answer_dependencies(&answer));
                    if answer.status == "CONFLICT" {
                        contested_fact_ids.extend(answer.facts.iter().map(|fact| fact.id.clone()));
                    }
                    for fact in answer.facts {
                        reported_facts.insert(fact.id.clone(), fact);
                    }
                }
                if reported_facts.is_empty() {
                    return self.stop(
                        &mut state,
                        Lifecycle::Suspend,
                        "MISSING_PREMISES",
                        "この対象について有効な提示前提または規則導出はありません。",
                    );
                }
                if has_conflict {
                    let line = "一部の関係には肯定と否定の根拠が併存するため、どちらも確定せず根拠を併記します。".to_string();
                    evidence_bytes = evidence_bytes
                        .checked_add(line.len())
                        .and_then(|total| total.checked_add(1))
                        .unwrap_or(usize::MAX);
                    if evidence_bytes > request.budget.max_evidence_bytes {
                        return self.stop(
                            &mut state,
                            Lifecycle::Suspend,
                            "RESOURCE_LIMIT",
                            "保存済み関係の一覧が要求の証拠予算を超えました。",
                        );
                    }
                    lines.push(line);
                }
                let mut facts: Vec<_> = reported_facts.into_values().collect();
                facts.sort_by(|left, right| {
                    left.proposition
                        .cmp(&right.proposition)
                        .then_with(|| left.id.cmp(&right.id))
                });
                let mut any_derived = false;
                for fact in &facts {
                    let (category, origin) = match &fact.guarantee {
                        Guarantee::UserPremise => ("提示前提", format!("提示前提ID: {}", fact.id)),
                        Guarantee::Derived => {
                            any_derived = true;
                            (
                                "規則導出",
                                format!(
                                    "導出ID: {}; 依存ID: {}",
                                    fact.id,
                                    fact.dependencies.join(", ")
                                ),
                            )
                        }
                        Guarantee::Computed => ("計算記録", format!("計算記録ID: {}", fact.id)),
                        Guarantee::SourceReport => ("出典報告", format!("出典報告ID: {}", fact.id)),
                        Guarantee::Hypothesis | Guarantee::Unknown => {
                            ("未確定", format!("未確定記録ID: {}", fact.id))
                        }
                    };
                    let conflict = if contested_fact_ids.contains(&fact.id) {
                        "対立根拠として"
                    } else {
                        ""
                    };
                    let line = format!(
                        "・{conflict}{category}: {}（範囲: {}; {origin}）",
                        describe(&fact.proposition),
                        fact.proposition.scope
                    );
                    evidence_bytes = evidence_bytes
                        .checked_add(line.len())
                        .and_then(|total| total.checked_add(1))
                        .unwrap_or(usize::MAX);
                    if evidence_bytes > request.budget.max_evidence_bytes {
                        return self.stop(
                            &mut state,
                            Lifecycle::Suspend,
                            "RESOURCE_LIMIT",
                            "保存済み関係の一覧が要求の証拠予算を超えました。",
                        );
                    }
                    lines.push(line);
                }
                units.extend(lines);
                result.dependencies = dependencies.into_iter().collect();
                result.guarantee = if any_derived {
                    Guarantee::Derived
                } else if facts
                    .iter()
                    .any(|fact| matches!(&fact.guarantee, Guarantee::UserPremise))
                {
                    Guarantee::UserPremise
                } else if facts
                    .iter()
                    .any(|fact| matches!(&fact.guarantee, Guarantee::Computed))
                {
                    Guarantee::Computed
                } else if facts
                    .iter()
                    .any(|fact| matches!(&fact.guarantee, Guarantee::SourceReport))
                {
                    Guarantee::SourceReport
                } else {
                    Guarantee::Unknown
                };
            }
            Intent::DraftRelationProfile { subject } => {
                let memory = self.store.load_memory(actor)?;
                let inventory = match relations::inventory_about(
                    &memory,
                    subject,
                    request.budget.max_actions,
                ) {
                    Ok(inventory) => inventory,
                    Err(Error::Contract { code, .. }) if code == "RELATION_BUDGET" => {
                        return self.stop(
                            &mut state,
                            Lifecycle::Suspend,
                            "RESOURCE_LIMIT",
                            "保存済み関係を紹介文へまとめる走査・推論予算が不足しています。",
                        );
                    }
                    Err(Error::Contract { code, .. }) if code == "MISSING_PREMISES" => {
                        return self.stop(
                            &mut state,
                            Lifecycle::Suspend,
                            "MISSING_PREMISES",
                            "この対象について紹介文の根拠にできる有効な保存記録がありません。",
                        );
                    }
                    Err(Error::Contract { code, message }) => {
                        return self.stop(&mut state, Lifecycle::Suspend, &code, &message);
                    }
                    Err(error) => return Err(error),
                };
                let profile_units = relation_profile_units(subject, &inventory);
                let profile_bytes = profile_units.iter().try_fold(0usize, |total, unit| {
                    total
                        .checked_add(unit.len())
                        .and_then(|size| size.checked_add(1))
                });
                if profile_bytes.is_none_or(|size| size > request.budget.max_evidence_bytes) {
                    return self.stop(
                        &mut state,
                        Lifecycle::Suspend,
                        "RESOURCE_LIMIT",
                        "根拠付き紹介文案が要求の証拠予算を超えています。",
                    );
                }
                approved_surfaces = Some(language::relation_profile_surfaces(&profile_units));
                units.extend(profile_units);
                result.dependencies = inventory.dependencies;
                // The proof lineage remains attached, while completeness and
                // outside-world truth of a requested profile are unverified.
                result.guarantee = Guarantee::Hypothesis;
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
            Intent::GenerateCode { document } => {
                let report = match crate::code_generation::generate(document) {
                    Ok(report) => report,
                    Err(error) => {
                        let code = match &error {
                            minidora_domain::Error::Contract { code, .. } => code.as_str(),
                            _ => "CODE_GENERATION_UNSUPPORTED",
                        };
                        return self.stop(&mut state, Lifecycle::Suspend, code, &error.to_string());
                    }
                };
                if serde_json::to_vec(&report)?.len() > request.budget.max_evidence_bytes {
                    return self.stop(
                        &mut state,
                        Lifecycle::Suspend,
                        "RESOURCE_LIMIT",
                        "generated code report exceeds request evidence budget",
                    );
                }
                let artifact = self.store.put_artifact(actor, &request.case_id, &report)?;
                result.dependencies.push(artifact);
                units.push(report.source);
                result.guarantee = Guarantee::Computed;
            }
            Intent::VerifyPreviousCode { document } => {
                let Some(prior) = previous.filter(|prior| prior.lifecycle == Lifecycle::Commit)
                else {
                    return self.stop(
                        &mut state,
                        Lifecycle::Suspend,
                        "MISSING_ACTUAL_RESULT",
                        "直前に採用されたコード生成結果がありません。",
                    );
                };
                let Some((source_ref, source_report)) =
                    crate::code_generation::previous_code_report(
                        &self.store,
                        actor,
                        &request.case_id,
                        prior,
                    )?
                else {
                    return self.stop(
                        &mut state,
                        Lifecycle::Suspend,
                        "MISSING_CODE_GENERATION_CONTEXT",
                        "直前の採用結果に、同一ケースで再検証できる構造化コード生成レポートがありません。",
                    );
                };
                if source_report.source_sha256 != digest(source_report.source.as_bytes()) {
                    return self.stop(
                        &mut state,
                        Lifecycle::Suspend,
                        "CODE_GENERATION_CONTEXT_MISMATCH",
                        "直前の生成レポートのソースハッシュが保存済みコードと一致しません。",
                    );
                }
                let deadline =
                    started.checked_add(Duration::from_millis(request.budget.deadline_ms));
                let report = match crate::code_capability::verify_previous_source(
                    &source_report.source,
                    document,
                    deadline,
                ) {
                    Ok(report) => report,
                    Err(error) => {
                        let code = match &error {
                            minidora_domain::Error::Contract { code, .. } => code.as_str(),
                            _ => "CODE_CAPABILITY_UNSUPPORTED",
                        };
                        return self.stop(&mut state, Lifecycle::Suspend, code, &error.to_string());
                    }
                };
                let report_bytes = canonical(&report)?;
                if report_bytes.len() > request.budget.max_evidence_bytes {
                    return self.stop(
                        &mut state,
                        Lifecycle::Suspend,
                        "RESOURCE_LIMIT",
                        "generated-code verification report exceeds request evidence budget",
                    );
                }
                let verification_ref = self.store.put_artifact(actor, &request.case_id, &report)?;
                result
                    .dependencies
                    .extend([prior.id.clone(), source_ref, verification_ref]);
                units.extend(crate::code_capability::previous_code_verification_lines(
                    &report,
                )?);
                result.guarantee = Guarantee::Computed;
            }
            Intent::ReadCode { document }
            | Intent::EvaluateCode { document }
            | Intent::VerifyCode { document } => {
                let operation = match &bundle.meaning.intent {
                    Intent::ReadCode { .. } => crate::code_capability::CodeOperation::Read,
                    Intent::EvaluateCode { .. } => crate::code_capability::CodeOperation::Evaluate,
                    Intent::VerifyCode { .. } => crate::code_capability::CodeOperation::Verify,
                    _ => unreachable!("matched code capability intent"),
                };
                let deadline =
                    started.checked_add(Duration::from_millis(request.budget.deadline_ms));
                let report = match crate::code_capability::run(document, operation, deadline) {
                    Ok(report) => report,
                    Err(error) => {
                        let code = match &error {
                            minidora_domain::Error::Contract { code, .. } => code.as_str(),
                            _ => "CODE_CAPABILITY_UNSUPPORTED",
                        };
                        return self.stop(&mut state, Lifecycle::Suspend, code, &error.to_string());
                    }
                };
                if serde_json::to_vec(&report)?.len() > request.budget.max_evidence_bytes {
                    return self.stop(
                        &mut state,
                        Lifecycle::Suspend,
                        "RESOURCE_LIMIT",
                        "code capability report exceeds request evidence budget",
                    );
                }
                let artifact = self.store.put_artifact(actor, &request.case_id, &report)?;
                result.dependencies.push(artifact);
                units.push(serde_json::to_string_pretty(&report)?);
                result.guarantee = Guarantee::Computed;
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
            Intent::JsonTableInnerJoin {
                document,
                left_key_column,
                right_key_column,
            } => {
                let report =
                    match ports::json_table_inner_join(document, left_key_column, right_key_column)
                    {
                        Ok(report) => report,
                        Err(error) => {
                            let code = match &error {
                                minidora_domain::Error::Contract { code, .. } => code.as_str(),
                                _ => "JSON_TABLE_INNER_JOIN_UNSUPPORTED",
                            };
                            return self.stop(
                                &mut state,
                                Lifecycle::Suspend,
                                code,
                                &error.to_string(),
                            );
                        }
                    };
                let text = serde_json::to_string_pretty(&report)?;
                if text.len() > request.budget.max_evidence_bytes {
                    return self.stop(
                        &mut state,
                        Lifecycle::Suspend,
                        "RESOURCE_LIMIT",
                        "JSON table inner join report exceeds the request output budget",
                    );
                }
                let artifact = self.store.put_artifact(actor, &request.case_id, &report)?;
                result.dependencies.push(artifact);
                units.push(text);
                result.guarantee = Guarantee::SourceReport;
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
            Intent::JsonTableGroupStatistics {
                document,
                group_column,
                value_column,
            } => {
                let report = match ports::json_table_group_statistics(
                    document,
                    group_column,
                    value_column,
                ) {
                    Ok(report) => report,
                    Err(error) => {
                        let code = match &error {
                            minidora_domain::Error::Contract { code, .. } => code.as_str(),
                            _ => "JSON_TABLE_GROUP_STATISTICS_UNSUPPORTED",
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
                        "JSON table grouped statistics report exceeds the request output budget",
                    );
                }
                let report_bytes = canonical(&report)?;
                if document.len().saturating_add(report_bytes.len())
                    > request.budget.max_evidence_bytes
                {
                    return self.stop(
                        &mut state,
                        Lifecycle::Suspend,
                        "RESOURCE_LIMIT",
                        "JSON table grouped statistics source and report exceed the evidence budget",
                    );
                }
                let source_ref =
                    self.store
                        .put_bytes(actor, &request.case_id, document.as_bytes())?;
                let report_ref = self
                    .store
                    .put_bytes(actor, &request.case_id, &report_bytes)?;
                result.dependencies.extend([source_ref, report_ref]);
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
        let compute_artifact = if let (Some(relation_meaning), Some(relation_proof_dependencies)) =
            (adopted_relation_meaning, relation_proof_dependencies)
        {
            self.store.put_artifact(
                actor,
                &request.case_id,
                &CoreComputationArtifact {
                    kind: CORE_COMPUTATION_ARTIFACT_KIND.into(),
                    calculation: result.calculation.clone(),
                    units: units.clone(),
                    evidence: result.evidence.clone(),
                    dependencies: result.dependencies.clone(),
                    relation_meaning,
                    relation_proof_dependencies,
                },
            )?
        } else {
            self.store.put_artifact(
                actor,
                &request.case_id,
                &(
                    &result.calculation,
                    &units,
                    &result.evidence,
                    &result.dependencies,
                ),
            )?
        };
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
        let surfaces = approved_surfaces.unwrap_or_else(|| vec![units.join("\n")]);
        let emission = match model.realize_supported_until(
            &surfaces,
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
fn active_direct_premise_ids(memory: &MemoryState, proposition: &Proposition) -> Vec<String> {
    memory
        .facts
        .iter()
        .filter(|fact| {
            fact.valid
                && fact.guarantee == Guarantee::UserPremise
                && fact.dependencies.is_empty()
                && !fact.source_input.is_empty()
                && fact.proposition == *proposition
        })
        .map(|fact| fact.id.clone())
        .collect()
}
fn apply_premise_correction(
    store: &Store,
    actor: &str,
    memory: &mut MemoryState,
    targets: &[String],
    superseded: &Proposition,
    replacement: &Proposition,
    fingerprint: &str,
) -> Result<(Vec<String>, usize)> {
    let original_memory_revision = memory.revision;
    let mut invalidated = Vec::new();
    for id in targets {
        for affected in relations::invalidate(memory, id)? {
            if !invalidated.contains(&affected) {
                invalidated.push(affected);
            }
        }
    }
    let negative_superseded = Proposition {
        negative: true,
        ..superseded.clone()
    };
    let negative_fact = relations::assert_fact(memory, negative_superseded, fingerprint)?;
    let replacement_fact = relations::assert_fact(memory, replacement.clone(), fingerprint)?;
    // Several relation helpers mutate one in-memory candidate. Persist the
    // compound correction as one actor-memory CAS revision.
    memory.revision = original_memory_revision
        .checked_add(1)
        .ok_or_else(|| failure("MEMORY_REVISION_LIMIT", "memory revision overflow"))?;
    store.save_memory(actor, memory)?;
    let invalidated_count = invalidated.len();
    invalidated.push(negative_fact.id);
    invalidated.push(replacement_fact.id);
    Ok((invalidated, invalidated_count))
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
pub(crate) fn previous_relation_answer(text: &str) -> Result<Option<Proposition>> {
    let Some(mut statement) = text
        .strip_prefix("提示された前提の範囲では、")
        .and_then(|value| value.strip_suffix('。'))
    else {
        return Ok(None);
    };
    if let Some(value) = statement.strip_prefix("問われた関係「") {
        let Some((_, answer)) = value.split_once("」は支持されず、反対に、") else {
            return Ok(None);
        };
        statement = answer;
    }
    let (statement, scope) = statement
        .strip_suffix('）')
        .and_then(|value| value.rsplit_once("（範囲: "))
        .map_or((statement, None), |(statement, scope)| {
            (statement, Some(scope))
        });
    let Some(mut proposition) = input::parse_proposition(statement, false)? else {
        return Ok(None);
    };
    if let Some(scope) = scope {
        if scope.is_empty() {
            return Ok(None);
        }
        proposition.scope = scope.into();
    }
    Ok(Some(proposition))
}

pub(crate) fn relation_question_proposition(intent: &Intent) -> Option<&Proposition> {
    match intent {
        Intent::Ask { proposition } | Intent::AskWhy { proposition } => Some(proposition),
        _ => None,
    }
}

fn previous_contextual_assertion_fact(
    store: &Store,
    actor: &str,
    case: &str,
    previous: &ResultRecord,
    memory: &MemoryState,
) -> Result<Option<Fact>> {
    let Some((_, bundle)) =
        store.get_input_bundle(actor, case, &previous.input_id, previous.input_version)?
    else {
        return Ok(None);
    };
    if identity(&bundle.request)? != previous.id {
        return Err(failure(
            "PRIOR_INPUT_RESULT_MISMATCH",
            "the sealed prior assertion and committed result identities differ",
        ));
    }
    if !matches!(
        &bundle.meaning.intent,
        Intent::AssertPreviousRelation { .. }
    ) {
        return Ok(None);
    }

    let facts: Vec<_> = memory
        .facts
        .iter()
        .filter(|fact| {
            fact.source_input == previous.id
                && fact.guarantee == Guarantee::UserPremise
                && fact.dependencies.is_empty()
        })
        .collect();
    let [fact] = facts.as_slice() else {
        return Ok(None);
    };
    let Some(context_result_ref) = bundle.meaning.context_result_ref.as_ref() else {
        return Ok(None);
    };
    let expected_dependencies = vec![context_result_ref.clone(), fact.id.clone()];
    if previous.guarantee != Guarantee::UserPremise
        || previous.dependencies != expected_dependencies
    {
        return Ok(None);
    }
    Ok(Some((*fact).clone()))
}

pub(crate) fn previous_relation_meaning(
    store: &Store,
    actor: &str,
    case: &str,
    previous: &ResultRecord,
) -> Result<Option<PriorRelationMeaning>> {
    if previous.calculation.is_some() {
        return Ok(None);
    }
    if let Some((_, prior_bundle)) =
        store.get_input_bundle(actor, case, &previous.input_id, previous.input_version)?
    {
        if identity(&prior_bundle.request)? != previous.id {
            return Err(failure(
                "PRIOR_INPUT_RESULT_MISMATCH",
                "the sealed prior input and committed result identities differ",
            ));
        }
        if let Some(proposition) = relation_question_proposition(&prior_bundle.meaning.intent) {
            return Ok(Some(PriorRelationMeaning::OriginalQuestion(
                proposition.clone(),
            )));
        }
    }

    if let Some(plan) = previous.content_plan.as_ref()
        && let Some(compute_ref) = plan.dependencies.last()
    {
        let Some(bytes) = store.get_bytes_if_present(actor, case, compute_ref)? else {
            return Err(failure(
                "PRIOR_COMPUTATION_ARTIFACT_UNAVAILABLE",
                "the committed prior computation artifact is not available in this case",
            ));
        };
        let value: serde_json::Value = serde_json::from_slice(&bytes)?;
        let artifact_kind = value.get("kind").and_then(serde_json::Value::as_str);
        if artifact_kind.is_some_and(|kind| {
            kind.starts_with("minidora.core-computation.relation-meaning.")
                && kind != CORE_COMPUTATION_ARTIFACT_KIND
        }) {
            return Err(failure(
                "PRIOR_COMPUTATION_ARTIFACT_VERSION_UNSUPPORTED",
                "the prior relation meaning uses an unsupported computation artifact version",
            ));
        }
        if artifact_kind == Some(CORE_COMPUTATION_ARTIFACT_KIND) {
            let artifact: CoreComputationArtifact = serde_json::from_value(value)?;
            if artifact.kind != CORE_COMPUTATION_ARTIFACT_KIND
                || artifact.calculation != previous.calculation
                || artifact.units != plan.required_units
                || artifact.evidence != previous.evidence
                || artifact.dependencies != previous.dependencies
                || artifact.relation_proof_dependencies != previous.dependencies
            {
                return Err(failure(
                    "PRIOR_COMPUTATION_ARTIFACT_MISMATCH",
                    "the retained relation meaning does not match its committed computation",
                ));
            }
            return Ok(Some(PriorRelationMeaning::AdoptedConclusion {
                proposition: artifact.relation_meaning,
                proof_dependencies: artifact.relation_proof_dependencies,
            }));
        }
    }

    Ok(
        previous_relation_answer(&previous.text)?.map(|proposition| {
            PriorRelationMeaning::AdoptedConclusion {
                proposition,
                proof_dependencies: previous.dependencies.clone(),
            }
        }),
    )
}

pub(crate) fn relation_explanation_target(
    proposition: &Proposition,
    answer: &relations::QueryOutcome,
) -> Option<Proposition> {
    match answer.status.as_str() {
        "SUPPORTED" => Some(proposition.clone()),
        "CONTRADICTED" => {
            let mut opposite = proposition.clone();
            opposite.negative = !opposite.negative;
            Some(opposite)
        }
        _ => None,
    }
}

fn relation_answer_surface(proposition: &Proposition, contradicted: bool) -> String {
    if contradicted {
        let mut opposite = proposition.clone();
        opposite.negative = !opposite.negative;
        format!(
            "提示された前提の範囲では、問われた関係「{}」は支持されず、反対に、{}（範囲: {}）。",
            describe(proposition),
            describe(&opposite),
            proposition.scope
        )
    } else {
        format!(
            "提示された前提の範囲では、{}（範囲: {}）。",
            describe(proposition),
            proposition.scope
        )
    }
}
pub(crate) fn relation_why_explanation_units(
    memory: &MemoryState,
    proposition: &Proposition,
    answer: &relations::QueryOutcome,
) -> Result<Vec<String>> {
    let mut units = Vec::new();
    let explained = match answer.status.as_str() {
        "SUPPORTED" => proposition.clone(),
        "CONTRADICTED" => {
            let mut opposite = proposition.clone();
            opposite.negative = !opposite.negative;
            units.push(relation_answer_surface(proposition, true));
            opposite
        }
        _ => {
            return Err(failure(
                "RELATION_EXPLANATION_UNSUPPORTED",
                "only a supported or refuted relation has an explanation surface",
            ));
        }
    };
    units.extend(relation_explanation_lines(
        memory,
        &explained,
        answer,
        "提示された証拠が支える結論",
    ));
    Ok(units)
}

fn relation_inventory_in_scope(
    memory: &MemoryState,
    subject: &str,
    scope: &str,
    max_actions: u32,
) -> Result<relations::RelationInventory> {
    let total_records = memory
        .facts
        .len()
        .checked_add(memory.rules.len())
        .ok_or_else(|| failure("RELATION_BUDGET", "relation memory scan count overflowed"))?;
    let facts: Vec<_> = memory
        .facts
        .iter()
        .filter(|fact| fact.proposition.scope == scope)
        .cloned()
        .collect();
    let rules: Vec<_> = memory
        .rules
        .iter()
        .filter(|rule| {
            rule.premise.scope == scope
                && rule.conclusion.scope == scope
                && rule
                    .additional_premises
                    .iter()
                    .all(|premise| premise.scope == scope)
        })
        .cloned()
        .collect();
    let relevant_records = facts
        .len()
        .checked_add(rules.len())
        .ok_or_else(|| failure("RELATION_BUDGET", "scoped relation scan count overflowed"))?;
    let excluded_records = total_records.saturating_sub(relevant_records);
    let excluded_actions = u32::try_from(excluded_records).map_err(|_| {
        failure(
            "RELATION_BUDGET",
            "relation memory scan exceeds action range",
        )
    })?;
    let remaining_actions = max_actions.checked_sub(excluded_actions).ok_or_else(|| {
        failure(
            "RELATION_BUDGET",
            "scanning the actor memory exceeds the relation comparison budget",
        )
    })?;
    relations::inventory_about(
        &MemoryState {
            revision: memory.revision,
            facts,
            rules,
        },
        subject,
        remaining_actions,
    )
}

fn relation_comparison_inventory(
    memory: &MemoryState,
    subject: &str,
    scope: &str,
    max_actions: u32,
) -> Result<relations::RelationInventory> {
    match relation_inventory_in_scope(memory, subject, scope, max_actions) {
        Err(Error::Contract { code, .. }) if code == "MISSING_PREMISES" => {
            Ok(relations::RelationInventory {
                facts: Vec::new(),
                contested_fact_ids: BTreeSet::new(),
                has_conflict: false,
                dependencies: Vec::new(),
                guarantee: Guarantee::Unknown,
            })
        }
        other => other,
    }
}

type RelationComparisonKey = (String, String, bool, String);

fn relation_comparison_map(
    inventory: &relations::RelationInventory,
) -> BTreeMap<RelationComparisonKey, Vec<&Fact>> {
    let mut grouped = BTreeMap::new();
    for fact in &inventory.facts {
        let proposition = &fact.proposition;
        grouped
            .entry((
                proposition.predicate.clone(),
                proposition.object.clone(),
                proposition.negative,
                proposition.scope.clone(),
            ))
            .or_insert_with(Vec::new)
            .push(fact);
    }
    grouped
}

fn relation_comparison_report(
    subject_a: &str,
    subject_b: &str,
    scope: &str,
    inventory_a: &relations::RelationInventory,
    inventory_b: &relations::RelationInventory,
) -> String {
    let left = relation_comparison_map(inventory_a);
    let right = relation_comparison_map(inventory_b);
    let mut keys: BTreeSet<_> = left.keys().cloned().collect();
    keys.extend(right.keys().cloned());
    let mut lines = vec![
        RELATION_COMPARISON_REPORT_HEADER.to_string(),
        format!("比較対象: {subject_a}／{subject_b}（scope: {scope}）"),
    ];
    if inventory_a.has_conflict || inventory_b.has_conflict {
        lines.push(
            "同一対象・範囲内で肯定と否定の競合根拠があります。競合は解消せず、確認した記録として併記します。".into(),
        );
    }
    if inventory_a.facts.is_empty() {
        lines.push(format!(
            "{subject_a}について、このscopeで比較に使える支持記録はありません。記録がないことは、関係がない証明ではありません。"
        ));
    }
    if inventory_b.facts.is_empty() {
        lines.push(format!(
            "{subject_b}について、このscopeで比較に使える支持記録はありません。記録がないことは、関係がない証明ではありません。"
        ));
    }
    let mut common_count = 0usize;
    let mut one_sided_count = 0usize;
    let mut contested_count = 0usize;
    for key in keys {
        let facts_a = left.get(&key).map(Vec::as_slice).unwrap_or_default();
        let facts_b = right.get(&key).map(Vec::as_slice).unwrap_or_default();
        let contested_a = facts_a
            .iter()
            .any(|fact| inventory_a.contested_fact_ids.contains(&fact.id));
        let contested_b = facts_b
            .iter()
            .any(|fact| inventory_b.contested_fact_ids.contains(&fact.id));
        if contested_a || contested_b {
            contested_count += 1;
            let example = facts_a.first().or_else(|| facts_b.first()).unwrap();
            lines.push(format!(
                "競合根拠を含む関係（共通・片側のみと確定しません）: {}",
                describe(&example.proposition)
            ));
        } else if !facts_a.is_empty() && !facts_b.is_empty() {
            common_count += 1;
            lines.push(format!(
                "両側で確認した同じ関係: {} ／ {}",
                describe(&facts_a[0].proposition),
                describe(&facts_b[0].proposition)
            ));
        } else if !facts_a.is_empty() {
            one_sided_count += 1;
            lines.push(format!(
                "{subject_a}側で確認した関係（{subject_b}側では同じ関係の支持を確認できず）: {}",
                describe(&facts_a[0].proposition)
            ));
        } else {
            one_sided_count += 1;
            lines.push(format!(
                "{subject_b}側で確認した関係（{subject_a}側では同じ関係の支持を確認できず）: {}",
                describe(&facts_b[0].proposition)
            ));
        }
        for (side, facts, inventory) in [
            (subject_a, facts_a, inventory_a),
            (subject_b, facts_b, inventory_b),
        ] {
            for fact in facts {
                let contested = if inventory.contested_fact_ids.contains(&fact.id) {
                    "; 対立根拠あり"
                } else {
                    ""
                };
                let dependencies = if fact.dependencies.is_empty() {
                    "なし".to_string()
                } else {
                    fact.dependencies.join(", ")
                };
                lines.push(format!(
                    "根拠({side}): {}（{}; 記録ID: {}; 依存ID: {}{contested}）",
                    describe(&fact.proposition),
                    guarantee_label(&fact.guarantee),
                    fact.id,
                    dependencies
                ));
            }
        }
    }
    if common_count == 0 && contested_count == 0 {
        lines.push(
            "両側で同じ関係は確認できませんでした。これは共通関係が存在しないことの証明ではありません。".into(),
        );
    }
    if one_sided_count == 0 {
        lines.push(
            "確認できた範囲では、明確に片側のみと分類できる関係はありませんでした。保存記録が網羅的とは限りません。".into(),
        );
    }
    lines.join("\n")
}

pub(crate) fn relation_profile_units(
    subject: &str,
    inventory: &relations::RelationInventory,
) -> Vec<String> {
    let mut units = vec![format!("「{subject}」の紹介文案です。")];
    if inventory.has_conflict {
        units.push(
            "同じ範囲に肯定と否定の根拠があるため、対立する記録を確定せず併記します。".into(),
        );
    }
    for fact in &inventory.facts {
        let proposition = describe(&fact.proposition);
        let scope = &fact.proposition.scope;
        let origin = if inventory.contested_fact_ids.contains(&fact.id) {
            format!(
                "対立根拠の一方として保存された関係は「{proposition}」です（範囲: {scope}; 記録ID: {}; 依存ID: {}）。",
                fact.id,
                if fact.dependencies.is_empty() {
                    "なし".to_string()
                } else {
                    fact.dependencies.join(", ")
                }
            )
        } else {
            match &fact.guarantee {
                Guarantee::UserPremise => format!(
                    "会話で提示された前提は「{proposition}」です（範囲: {scope}; 前提ID: {}）。",
                    fact.id
                ),
                Guarantee::Derived => format!(
                    "保存された規則による導出は「{proposition}」です（範囲: {scope}; 導出ID: {}; 依存ID: {}）。",
                    fact.id,
                    fact.dependencies.join(", ")
                ),
                Guarantee::Computed => format!(
                    "保存された計算記録は「{proposition}」を示します（範囲: {scope}; 記録ID: {}）。",
                    fact.id
                ),
                Guarantee::SourceReport => format!(
                    "保存された出典報告には「{proposition}」が記録されています（範囲: {scope}; 報告ID: {}）。",
                    fact.id
                ),
                Guarantee::Hypothesis | Guarantee::Unknown => format!(
                    "未確定の記録には「{proposition}」が含まれています（範囲: {scope}; 記録ID: {}）。",
                    fact.id
                ),
            }
        };
        units.push(origin);
    }
    units.push(
        "この草稿はこの利用者の保存記録に基づきます。現実世界の真偽や記録の網羅性は確認していません。"
            .into(),
    );
    units
}

pub(crate) fn relation_explanation_lines(
    memory: &MemoryState,
    proposition: &Proposition,
    answer: &relations::QueryOutcome,
    heading: &str,
) -> Vec<String> {
    let mut lines = vec![format!(
        "{heading}: {}（範囲: {}）。",
        describe(proposition),
        proposition.scope
    )];
    let proof_dependencies = relations::answer_dependencies(answer);
    let mut displayed_fact_ids = Vec::new();
    for id in &proof_dependencies {
        if let Some(fact) = memory.facts.iter().find(|fact| {
            &fact.id == id
                && fact.valid
                && fact.guarantee == Guarantee::UserPremise
                && fact.dependencies.is_empty()
        }) {
            lines.push(format!(
                "直接提示された前提: {}（範囲: {}; 根拠ID: {}）",
                describe(&fact.proposition),
                fact.proposition.scope,
                fact.id
            ));
            displayed_fact_ids.push(fact.id.clone());
        }
    }
    for fact in &answer.facts {
        if displayed_fact_ids.contains(&fact.id) {
            continue;
        }
        let label = if fact.guarantee == Guarantee::Derived {
            "導出された事実"
        } else {
            "直接提示された前提"
        };
        let lineage = if fact.dependencies.is_empty() {
            format!("根拠ID: {}", fact.id)
        } else {
            format!(
                "導出ID: {}; 依存ID: {}",
                fact.id,
                fact.dependencies.join(", ")
            )
        };
        lines.push(format!(
            "{label}: {}（範囲: {}; {lineage}）",
            describe(&fact.proposition),
            fact.proposition.scope,
        ));
    }
    for id in &answer.used_rules {
        if let Some(rule) = memory.rules.iter().find(|rule| &rule.id == id) {
            let premises = std::iter::once(&rule.premise)
                .chain(rule.additional_premises.iter())
                .map(describe)
                .collect::<Vec<_>>()
                .join(" かつ ");
            lines.push(format!(
                "適用規則（範囲: {}）: {} ならば {}（規則ID: {}）",
                rule.conclusion.scope,
                premises,
                describe(&rule.conclusion),
                rule.id
            ));
        }
    }
    lines.push(format!(
        "この説明は範囲「{}」の有効な提示前提・規則に基づきます。現実世界での真偽確認ではありません。",
        proposition.scope
    ));
    lines
}
fn guarantee_label(guarantee: &Guarantee) -> &'static str {
    match guarantee {
        Guarantee::UserPremise => "直接提示された前提",
        Guarantee::Derived => "保存規則からの導出",
        Guarantee::Computed => "計算結果",
        Guarantee::SourceReport => "出典付き報告",
        Guarantee::Hypothesis => "仮説",
        Guarantee::Unknown => "未確定",
    }
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
    } else if p.predicate == "eat" {
        format!(
            "{}は{}を{}",
            p.subject,
            p.object,
            if p.negative {
                "食べない"
            } else {
                "食べる"
            }
        )
    } else if p.predicate == "use" {
        format!(
            "{}は{}を{}",
            p.subject,
            p.object,
            if p.negative { "使わない" } else { "使う" }
        )
    } else if ["う", "く", "ぐ", "す", "つ", "ぬ", "ぶ", "む", "る"]
        .iter()
        .any(|ending| p.predicate.ends_with(ending))
    {
        let predicate = if p.negative {
            let Some(predicate) = japanese_godan_negative_form(&p.predicate) else {
                return format!("{} / {} / {} / 否定", p.subject, p.predicate, p.object);
            };
            predicate
        } else {
            p.predicate.clone()
        };
        format!("{}は{}を{}", p.subject, p.object, predicate)
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

fn japanese_godan_negative_form(predicate: &str) -> Option<String> {
    let (ending, negative_ending) = if predicate.ends_with("う") {
        ("う", "わない")
    } else if predicate.ends_with("く") {
        ("く", "かない")
    } else if predicate.ends_with("ぐ") {
        ("ぐ", "がない")
    } else if predicate.ends_with("す") {
        ("す", "さない")
    } else if predicate.ends_with("つ") {
        ("つ", "たない")
    } else if predicate.ends_with("ぬ") {
        ("ぬ", "なない")
    } else if predicate.ends_with("ぶ") {
        ("ぶ", "ばない")
    } else if predicate.ends_with("む") {
        ("む", "まない")
    } else if predicate.ends_with("る") {
        ("る", "らない")
    } else {
        return None;
    };
    let stem = predicate.strip_suffix(ending)?;
    (!stem.is_empty()).then(|| format!("{stem}{negative_ending}"))
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

#[cfg(test)]
mod previous_relation_meaning_tests {
    use super::*;
    use std::fs;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT_TEMP_DIRECTORY: AtomicU64 = AtomicU64::new(0);

    #[test]
    fn previous_relation_meaning_reads_legacy_computation_artifact() {
        let serial = NEXT_TEMP_DIRECTORY.fetch_add(1, Ordering::Relaxed);
        let directory = std::env::temp_dir().join(format!(
            "minidora-legacy-relation-test-{}-{serial}",
            std::process::id()
        ));
        fs::create_dir(&directory).expect("unique legacy relation test directory");
        let store = Store::open(&directory).expect("open isolated store");
        let request = RequestEnvelope::new(
            "legacy-relation-case",
            "legacy-relation-input",
            "legacy-relation-actor",
            "それは動物ですか",
        );
        let fingerprint = identity(&request).expect("request identity");
        assert!(
            store
                .register_input(&request.actor_id, &request, &fingerprint)
                .expect("register legacy request")
        );

        let legacy_dependencies = vec!["legacy-direct-premise".to_owned()];
        let legacy_artifact = serde_json::json!({
            "calculation": null,
            "units": [],
            "evidence": [],
            "dependencies": legacy_dependencies,
        });
        let artifact_ref = store
            .put_artifact(&request.actor_id, &request.case_id, &legacy_artifact)
            .expect("store unversioned computation artifact");
        let previous_text = "提示された前提の範囲では、ココは動物（範囲: default）。".to_owned();
        let previous = ResultRecord {
            id: "legacy-prior-result".into(),
            input_id: request.input_id.clone(),
            input_version: request.input_version,
            lifecycle: Lifecycle::Commit,
            text: previous_text.clone(),
            reason: None,
            guarantee: Guarantee::Derived,
            calculation: None,
            content_plan: Some(ContentPlan {
                id: "legacy-prior-plan".into(),
                text: previous_text,
                locale: "ja".into(),
                guarantee: Guarantee::Derived,
                required_units: Vec::new(),
                dependencies: vec![artifact_ref],
                model_revision: "legacy-model".into(),
                model_artifact_ref: "legacy-model-artifact".into(),
                model_observation_ref: "legacy-model-observation".into(),
            }),
            render: None,
            receipts: Vec::new(),
            evidence: Vec::new(),
            dependencies: vec!["legacy-direct-premise".into()],
            execution_graph_ref: None,
        };

        let meaning =
            previous_relation_meaning(&store, &request.actor_id, &request.case_id, &previous)
                .expect("legacy prior relation is readable")
                .expect("legacy prior relation is recognized");
        assert_eq!(
            meaning,
            PriorRelationMeaning::AdoptedConclusion {
                proposition: Proposition {
                    subject: "ココ".into(),
                    predicate: "is".into(),
                    object: "動物".into(),
                    negative: false,
                    scope: "default".into(),
                },
                proof_dependencies: vec!["legacy-direct-premise".into()],
            }
        );

        drop(store);
        fs::remove_dir_all(directory).expect("remove isolated legacy relation store");
    }
}
