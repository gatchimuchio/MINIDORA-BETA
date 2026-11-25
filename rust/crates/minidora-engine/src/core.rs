use crate::scheduler::{GoalNode, GoalNodeKind, Scheduler};
use crate::{
    document::CsvInput,
    input, language, output, ports,
    recovery::PREVIOUS_DIRECT_RETRIEVAL_NODE,
    relations, search,
    store::{MemoryIndexSnapshot, Store, VerifiedCaseHistory},
};
use minidora_domain::*;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::time::{Duration, Instant};

pub(crate) const NO_COMPUTE_BUDGET: &str = "計算予算がありません。";
pub(crate) const ACTION_BUDGET_RESIDUALS: &str = "必須成果の残件:";
pub(crate) const NO_PENDING_DIRECT_URL: &str = "NO_PENDING_DIRECT_URL";
pub(crate) const NO_PENDING_DIRECT_URL_MESSAGE: &str =
    "前回の直接取得に、照合できる次の未接続URLがありません。";

#[derive(serde::Serialize)]
struct CalculationSequenceStepArtifact {
    id: String,
    source_expression: SourceSpan,
    evaluated_expression: String,
    calculation: Calculation,
    consumed: Vec<language::CalculationReferenceUse>,
}

pub(crate) fn calculation_sequence_nodes(steps: &[CalculationSequenceStep]) -> Vec<GoalNode> {
    steps
        .iter()
        .map(|step| GoalNode {
            id: format!("sequence-{}", step.id),
            kind: GoalNodeKind::ComputedValue,
            requires: step
                .requires
                .iter()
                .map(|id| format!("sequence-{id}"))
                .collect(),
        })
        .collect()
}

pub(crate) struct CalculationSequenceResult {
    pub(crate) calculation: Calculation,
    pub(crate) units: Vec<String>,
    steps: Vec<CalculationSequenceStepArtifact>,
}

impl CalculationSequenceResult {
    pub(crate) fn verify_content(&self, result: &ResultRecord, plan: &ContentPlan) -> Result<()> {
        if result.calculation.as_ref() != Some(&self.calculation)
            || result.guarantee != Guarantee::Computed
            || plan.guarantee != Guarantee::Computed
            || plan.required_units != self.units
            || plan.text != self.units.join("\n")
        {
            return Err(failure(
                "GOAL_CALCULATION_SEQUENCE_MISMATCH",
                "adopted calculation content differs from the required observed steps",
            ));
        }
        Ok(())
    }
}

/// Execute only the next ready node of the sealed Goal. The artifact callback
/// persists live results or authenticates the same bytes during reconciliation.
pub(crate) fn run_calculation_sequence<'a>(
    steps: &[CalculationSequenceStep],
    source_text: impl Fn(&str) -> &'a str,
    graph: &mut Scheduler,
    budget: &ResourceBudget,
    started: Instant,
    dependencies: &mut Vec<String>,
    mut artifact_ref: impl FnMut(&[u8]) -> Result<String>,
) -> Result<CalculationSequenceResult> {
    let final_step = steps.last().ok_or_else(|| {
        failure(
            "CALCULATION_SEQUENCE_EMPTY",
            "実行可能な計算手順がありません。",
        )
    })?;
    let by_node: BTreeMap<_, _> = steps
        .iter()
        .map(|step| (format!("sequence-{}", step.id), step))
        .collect();
    let mut completed = BTreeMap::<String, language::SequenceValue>::new();
    let mut artifact_refs = Vec::with_capacity(steps.len());
    let mut units = Vec::with_capacity(steps.len());
    let mut observed_steps = Vec::with_capacity(steps.len());
    let mut final_calculation = None;
    loop {
        let node_id = graph.ready().into_iter().next().ok_or_else(|| {
            failure(
                "UNOBSERVED_PREREQUISITE",
                format!("必須成果の残件を実行できません: {:?}", graph.residuals()),
            )
        })?;
        if node_id == "compute" {
            break;
        }
        let step = by_node.get(&node_id).ok_or_else(|| {
            failure(
                "SEQUENCE_DEPENDENCY_MISMATCH",
                "ready node is not a sealed calculation step",
            )
        })?;
        if started.elapsed().as_millis() >= u128::from(budget.deadline_ms) {
            return Err(failure(
                "DEADLINE",
                "計算手順の途中で要求全体の締切へ達しました。",
            ));
        }
        let span = &step.expression;
        if source_text(&step.id).get(span.start..span.end) != Some(span.text.as_str()) {
            return Err(failure(
                "SEQUENCE_DEPENDENCY_MISMATCH",
                "sealed calculation expression does not match the original input span",
            ));
        }
        let inputs = step
            .requires
            .iter()
            .map(|id| {
                completed
                    .get(id)
                    .cloned()
                    .map(|value| (id.clone(), value))
                    .ok_or_else(|| {
                        failure(
                            "UNOBSERVED_PREREQUISITE",
                            "sealed step requires an unobserved value",
                        )
                    })
            })
            .collect::<Result<BTreeMap<_, _>>>()?;
        let resolved = language::calculate_sequence_expression(&span.text, &inputs)?;
        let consumed: BTreeMap<_, _> = resolved
            .consumed
            .iter()
            .map(|reference| {
                (
                    format!("sequence-{}", reference.step_id),
                    reference.artifact_sha256.clone(),
                )
            })
            .collect();
        if consumed.len() != inputs.len() {
            return Err(failure(
                "SEQUENCE_DEPENDENCY_MISMATCH",
                "計算器が実際に消費した参照集合が封印済みの参照集合と一致しません。",
            ));
        }
        let artifact = CalculationSequenceStepArtifact {
            id: step.id.clone(),
            source_expression: span.clone(),
            evaluated_expression: resolved.evaluated_expression.clone(),
            calculation: resolved.calculation.clone(),
            consumed: resolved.consumed,
        };
        let reference = artifact_ref(&canonical(&artifact)?)?;
        observed_steps.push(artifact);
        graph.observe(&node_id, &reference, &consumed)?;
        dependencies.push(reference.clone());
        units.push(format!(
            "{}: {} = {}",
            step.id, resolved.evaluated_expression, resolved.calculation.exact_value
        ));
        completed.insert(
            step.id.clone(),
            language::SequenceValue {
                exact_value: resolved.calculation.exact_value.clone(),
                artifact_sha256: reference.clone(),
            },
        );
        if step.id == final_step.id {
            final_calculation = Some(resolved.calculation);
        }
        artifact_refs.push((step.id.clone(), reference));
    }
    let calculation = final_calculation.ok_or_else(|| {
        failure(
            "UNOBSERVED_PREREQUISITE",
            "the declared final calculation has not been observed",
        )
    })?;
    let report = canonical(&(
        "minidora-calculation-sequence-report-v1",
        &artifact_refs,
        &final_step.id,
        &calculation.exact_value,
        "each declared downstream reference was substituted with the exact value of its observed upstream calculation artifact",
    ))?;
    if report.len() > budget.max_evidence_bytes {
        return Err(failure(
            "RESOURCE_LIMIT",
            "計算手順のartifact参照報告が出力予算を超えています。",
        ));
    }
    dependencies.push(artifact_ref(&report)?);
    Ok(CalculationSequenceResult {
        calculation,
        units,
        steps: observed_steps,
    })
}

pub(crate) struct PreviousCalculationSequence<'a> {
    input: InputBundle,
    original: &'a ResultRecord,
    followups: Vec<CalculationSequenceFollowup<'a>>,
    edits: usize,
    context_actions: usize,
    effective_steps: Vec<CalculationSequenceStep>,
}

struct CalculationSequenceFollowup<'a> {
    bundle: InputBundle,
    result: &'a ResultRecord,
}

impl PreviousCalculationSequence<'_> {
    fn root_steps(&self) -> &[CalculationSequenceStep] {
        let Intent::CalculateSequence { steps } = &self.input.meaning.intent else {
            unreachable!("sequence history starts at a calculation sequence");
        };
        steps
    }

    pub(crate) fn effective_steps(&self) -> &[CalculationSequenceStep] {
        &self.effective_steps
    }

    pub(crate) fn predicate(&self) -> &'static str {
        if self.followups.iter().any(|entry| {
            matches!(
                entry.bundle.meaning.intent,
                Intent::InsertCalculationSequenceStep { .. }
            )
        }) {
            INSERTED_SEQUENCE_EXPLANATION_PREDICATE
        } else if self.followups.iter().any(|entry| {
            matches!(
                entry.bundle.meaning.intent,
                Intent::MoveCalculationSequenceStep { .. }
            )
        }) {
            MOVED_SEQUENCE_EXPLANATION_PREDICATE
        } else if self.followups.iter().any(|entry| {
            matches!(
                entry.bundle.meaning.intent,
                Intent::RemoveCalculationSequenceStep { .. }
            )
        }) {
            REMOVED_SEQUENCE_EXPLANATION_PREDICATE
        } else if self.followups.iter().any(|entry| {
            matches!(
                entry.bundle.meaning.intent,
                Intent::AppendCalculationSequenceStep { .. }
            )
        }) {
            APPENDED_SEQUENCE_EXPLANATION_PREDICATE
        } else if self.followups.iter().any(|entry| {
            entry.bundle.meaning.parser_version == input::SEQUENCE_REWIRE_PARSER_VERSION
        }) {
            REWIRED_SEQUENCE_EXPLANATION_PREDICATE
        } else if self.edits > 0 {
            REVISED_SEQUENCE_EXPLANATION_PREDICATE
        } else if self.followups.is_empty() {
            SEQUENCE_EXPLANATION_PREDICATE
        } else {
            CHAINED_SEQUENCE_EXPLANATION_PREDICATE
        }
    }

    pub(crate) fn context_actions(&self) -> usize {
        self.context_actions
    }
}

pub(crate) fn previous_calculation_sequence<'a>(
    store: &Store,
    followup: &InputBundle,
    previous: &'a ResultRecord,
    prior_results: &'a BTreeMap<String, ResultRecord>,
    started: Option<Instant>,
) -> Result<Option<PreviousCalculationSequence<'a>>> {
    if previous.lifecycle != Lifecycle::Commit || previous.calculation.is_none() {
        return Ok(None);
    }
    let request = &followup.request;
    store.with_verified_case_history(&request.actor_id, &request.case_id, |history| {
        let invalid = || {
            failure(
                "CALCULATION_CONTEXT_INVALID",
                "前回の計算を同じケースの封印入力と確定結果に結び付けられません。",
            )
        };
        if followup.meaning.context_result_ref.as_deref() != Some(previous.id.as_str())
            || followup.context_sha256 != identity(&Some(previous))?
            || request
                .context_ref
                .as_ref()
                .is_some_and(|id| id != &previous.id)
        {
            return Err(invalid());
        }
        let mut current = previous;
        let mut followups: Vec<CalculationSequenceFollowup<'_>> = Vec::new();
        let mut edits = 0;
        let mut visited = BTreeSet::new();
        loop {
            if started.is_some_and(|start| {
                start.elapsed().as_millis() >= u128::from(request.budget.deadline_ms)
            }) {
                return Err(failure(
                    "DEADLINE",
                    "前回の計算を照合中に締切へ達しました。",
                ));
            }
            let (_, input) = history
                .input_bundle(&current.input_id, current.input_version)?
                .ok_or_else(invalid)?;
            if identity(&input.request)? != current.id
                || current.lifecycle != Lifecycle::Commit
                || !visited.insert(current.id.as_str())
            {
                return Err(invalid());
            }
            if let Intent::CalculateSequence { steps } = &input.meaning.intent {
                if history
                    .input_result(&current.input_id, current.input_version)?
                    .as_ref()
                    != Some(current)
                {
                    return Err(invalid());
                }
                followups.reverse();
                let mut effective_steps = steps.clone();
                let mut context_actions = 0;
                for entry in &followups {
                    if let Some(started) = started {
                        sequence_history_time(&request.budget, started)?;
                    }
                    if input::sequence_edit_parser(&entry.bundle.meaning.intent).is_some() {
                        // The current graph already charges the latest edition.
                        context_actions += effective_steps.len();
                        sequence_step_edit(&effective_steps, &entry.bundle)?
                            .apply(&mut effective_steps);
                    } else {
                        context_actions += 1;
                    }
                }
                return Ok(Some(PreviousCalculationSequence {
                    input,
                    original: current,
                    followups,
                    edits,
                    context_actions,
                    effective_steps,
                }));
            }
            let edit_parser = input::sequence_edit_parser(&input.meaning.intent);
            let shows_history =
                matches!(input.meaning.intent, Intent::ShowCalculationSequenceHistory);
            let explains_step = matches!(
                input.meaning.intent,
                Intent::ExplainCalculationSequenceStep { .. }
            );
            let detail = matches!(input.meaning.intent, Intent::ExplainPrevious);
            if followups.is_empty()
                && edit_parser.is_none()
                && !shows_history
                && !explains_step
                && (!detail
                    || current.dependencies.len() == 1
                        && current.calculation.as_ref().is_some_and(|calculation| {
                            current.content_plan.as_ref().is_some_and(|plan| {
                                plan.required_units == calculation_detail_units(calculation)
                            })
                        }))
            {
                return Ok(None);
            }
            if !(detail
                || shows_history
                    && input.meaning.parser_version == input::SEQUENCE_HISTORY_PARSER_VERSION
                || explains_step
                    && input.meaning.parser_version
                        == input::SEQUENCE_STEP_EXPLANATION_PARSER_VERSION
                || edit_parser == Some(input.meaning.parser_version.as_str()))
                || history
                    .input_result(&current.input_id, current.input_version)?
                    .as_ref()
                    != Some(current)
            {
                return Err(invalid());
            }
            if followups.len() >= request.budget.max_actions as usize {
                return Err(failure(
                    "ACTION_BUDGET_EXHAUSTED",
                    "前回の計算と訂正履歴を照合するための行動予算が不足しています。",
                ));
            }
            let parent = input
                .meaning
                .context_result_ref
                .as_ref()
                .and_then(|reference| {
                    prior_results
                        .get(reference)
                        .filter(|parent| &parent.id == reference)
                })
                .ok_or_else(invalid)?;
            if input.context_sha256 != identity(&Some(parent))?
                || input
                    .request
                    .context_ref
                    .as_ref()
                    .is_some_and(|id| id != &parent.id)
            {
                return Err(invalid());
            }
            edits += usize::from(edit_parser.is_some());
            followups.push(CalculationSequenceFollowup {
                bundle: input,
                result: current,
            });
            current = parent;
        }
    })
}

fn calculation_detail_units(calculation: &Calculation) -> Vec<String> {
    let mut units = vec![format!("計算式: {}", calculation.expression)];
    units.extend(calculation.steps.iter().cloned());
    units.push(format!("計算結果: {}", calculation.exact_value));
    units
}

pub(crate) struct CalculationSequenceExplanation {
    pub(crate) calculation: Calculation,
    pub(crate) units: Vec<String>,
    pub(crate) dependencies: Vec<String>,
}

fn invalid_sequence_proof() -> Error {
    failure(
        "CALCULATION_SEQUENCE_CONTEXT_CHANGED",
        "前回の計算手順、訂正履歴または保存された成果物が一致しません。",
    )
}

fn sequence_proof_graph(
    steps: &[CalculationSequenceStep],
    budget: &ResourceBudget,
) -> Result<Scheduler> {
    let mut nodes = calculation_sequence_nodes(steps);
    nodes.push(GoalNode {
        id: "compute".into(),
        kind: GoalNodeKind::CoreResult,
        requires: nodes.iter().map(|node| node.id.clone()).collect(),
    });
    Scheduler::new(nodes, budget.max_actions)
}

fn sequence_history_time(budget: &ResourceBudget, started: Instant) -> Result<()> {
    if started.elapsed().as_millis() >= u128::from(budget.deadline_ms) {
        return Err(failure(
            "DEADLINE",
            "計算手順の履歴を検証中に締切へ達しました。",
        ));
    }
    Ok(())
}

fn debit_sequence_evidence(remaining: &mut usize, bytes: &[u8]) -> Result<()> {
    *remaining = remaining.checked_sub(bytes.len()).ok_or_else(|| {
        failure(
            "RESOURCE_LIMIT",
            "元の手順と訂正の成果物が証拠予算を超えています。",
        )
    })?;
    Ok(())
}

fn read_sequence_artifact(
    store: &Store,
    request: &RequestEnvelope,
    expected: &[u8],
) -> Result<String> {
    let reference = digest(expected);
    if store.get_bytes(&request.actor_id, &request.case_id, &reference)? != expected {
        return Err(invalid_sequence_proof());
    }
    Ok(reference)
}

fn append_sequence_step_units(units: &mut Vec<String>, step: &CalculationSequenceStepArtifact) {
    units.push(format!(
        "手順 {} の元の式: {}",
        step.id, step.source_expression.text
    ));
    if !step.consumed.is_empty() {
        let references = step
            .consumed
            .iter()
            .map(|reference| format!("{} = {}", reference.step_id, reference.exact_value))
            .collect::<Vec<_>>()
            .join("、");
        units.push(format!("代入した先行手順の値: {references}"));
    }
    units.push(format!(
        "手順 {} の結果: {} = {}",
        step.id, step.evaluated_expression, step.calculation.exact_value
    ));
}

fn sequence_explanation_units(
    observed: &CalculationSequenceResult,
    budget: &ResourceBudget,
) -> Result<Vec<String>> {
    let mut units = Vec::new();
    for step in &observed.steps {
        append_sequence_step_units(&mut units, step);
    }
    units.push(format!("最終結果: {}", observed.calculation.exact_value));
    if units
        .join(
            "
",
        )
        .len()
        > budget.max_evidence_bytes
    {
        return Err(failure(
            "RESOURCE_LIMIT",
            "計算手順の説明が要求の証拠予算を超えています。",
        ));
    }
    Ok(units)
}

fn revised_calculation_step(
    steps: &[CalculationSequenceStep],
    bundle: &InputBundle,
) -> Result<(usize, CalculationSequenceStep)> {
    let Intent::CorrectCalculationSequenceStep {
        step_id,
        expression,
        change_dependencies,
    } = &bundle.meaning.intent
    else {
        return Err(invalid_sequence_proof());
    };
    let target = steps
        .iter()
        .position(|step| &step.id == step_id)
        .ok_or_else(|| {
            failure(
                "SEQUENCE_CORRECTION_STEP_UNKNOWN",
                "指定した手順IDが元の連続計算にありません。",
            )
        })?;
    let references = input::calculation_step_references(&expression.text)
        .map_err(|message| failure("SEQUENCE_CORRECTION_REFERENCE_INVALID", message))?;
    let mut revised = steps[target].clone();
    if *change_dependencies {
        revised.requires.clear();
        for reference in references {
            let position = steps
                .iter()
                .position(|step| step.id == reference)
                .ok_or_else(|| {
                    failure(
                        "SEQUENCE_CORRECTION_REFERENCE_UNKNOWN",
                        format!("参照先の手順 {reference} はありません。"),
                    )
                })?;
            if position == target {
                return Err(failure(
                    "SEQUENCE_CORRECTION_SELF_REFERENCE",
                    format!("手順 {reference} は自分自身を参照できません。"),
                ));
            }
            if position > target {
                return Err(failure(
                    "SEQUENCE_CORRECTION_FORWARD_REFERENCE",
                    format!("手順 {step_id} より後に定義された手順 {reference} は参照できません。"),
                ));
            }
            if !revised.requires.contains(&reference) {
                revised.requires.push(reference);
            }
        }
    } else if references.iter().collect::<BTreeSet<_>>()
        != revised.requires.iter().collect::<BTreeSet<_>>()
    {
        return Err(failure(
            "SEQUENCE_CORRECTION_DEPENDENCIES_CHANGED",
            "参照先を変更するには「を参照先も含めて訂正」を指定してください。",
        ));
    }
    revised.expression = expression.clone();
    Ok((target, revised))
}

fn sequence_step_edit(
    steps: &[CalculationSequenceStep],
    bundle: &InputBundle,
) -> Result<SequenceStepEdit> {
    if let Intent::InsertCalculationSequenceStep {
        step_id,
        anchor_id,
        before,
        expression,
    } = &bundle.meaning.intent
    {
        let anchor = steps
            .iter()
            .position(|step| &step.id == anchor_id)
            .ok_or_else(|| {
                failure(
                    "SEQUENCE_INSERT_ANCHOR_UNKNOWN",
                    format!("挿入の基準となる手順 {anchor_id} は定義されていません。"),
                )
            })?;
        let position = anchor + usize::from(!*before);
        let step =
            new_calculation_sequence_step(steps, position, step_id, expression, "SEQUENCE_INSERT")?;
        return Ok(SequenceStepEdit::Insert { position, step });
    }
    if let Intent::MoveCalculationSequenceStep {
        step_id,
        anchor_id,
        before,
    } = &bundle.meaning.intent
    {
        return prepare_sequence_move(steps, step_id, anchor_id, *before);
    }
    if let Intent::RemoveCalculationSequenceStep { step_id } = &bundle.meaning.intent {
        let target = steps
            .iter()
            .position(|step| &step.id == step_id)
            .ok_or_else(|| {
                failure(
                    "SEQUENCE_REMOVE_STEP_UNKNOWN",
                    format!("手順 {step_id} は定義されていません。"),
                )
            })?;
        if steps.len() == 1 {
            return Err(failure(
                "SEQUENCE_REMOVE_LAST_STEP",
                "連続計算には少なくとも1手順が必要です。",
            ));
        }
        let consumers = steps
            .iter()
            .filter(|step| step.requires.contains(step_id))
            .map(|step| step.id.as_str())
            .collect::<Vec<_>>();
        if !consumers.is_empty() {
            return Err(failure(
                "SEQUENCE_REMOVE_STEP_IN_USE",
                format!(
                    "手順 {} が {step_id} を参照しています。先に参照先を訂正してください。",
                    consumers.join("、")
                ),
            ));
        }
        return Ok(SequenceStepEdit::Remove { target });
    }
    let Intent::AppendCalculationSequenceStep {
        step_id,
        expression,
    } = &bundle.meaning.intent
    else {
        let (target, step) = revised_calculation_step(steps, bundle)?;
        return Ok(SequenceStepEdit::Replace { target, step });
    };
    let step =
        new_calculation_sequence_step(steps, steps.len(), step_id, expression, "SEQUENCE_APPEND")?;
    Ok(SequenceStepEdit::Append { step })
}

fn new_calculation_sequence_step(
    steps: &[CalculationSequenceStep],
    position: usize,
    step_id: &str,
    expression: &SourceSpan,
    operation: &str,
) -> Result<CalculationSequenceStep> {
    let stop = |reason, message: String| failure(&format!("{operation}_{reason}"), message);
    if steps.iter().any(|step| step.id == step_id) {
        return Err(stop(
            "DUPLICATE_STEP",
            format!("手順 {step_id} は既に定義されています。"),
        ));
    }
    if steps.len() >= input::MAX_CALCULATION_SEQUENCE_STEPS {
        return Err(stop(
            "STEP_LIMIT",
            "連続計算には16手順まで定義できます。".into(),
        ));
    }
    let references = input::calculation_step_references(&expression.text)
        .map_err(|message| stop("REFERENCE_INVALID", message))?;
    let mut requires = Vec::new();
    for reference in references {
        if reference == step_id {
            return Err(stop(
                "SELF_REFERENCE",
                format!("手順 {step_id} は自分自身を参照できません。"),
            ));
        }
        let prerequisite = steps
            .iter()
            .position(|step| step.id == reference)
            .ok_or_else(|| {
                stop(
                    "REFERENCE_UNKNOWN",
                    format!("参照先の手順 {reference} はありません。"),
                )
            })?;
        if prerequisite >= position {
            return Err(stop(
                "FORWARD_REFERENCE",
                format!("挿入する手順より後に実行される手順 {reference} は参照できません。"),
            ));
        }
        if !requires.contains(&reference) {
            requires.push(reference);
        }
    }
    Ok(CalculationSequenceStep {
        id: step_id.to_owned(),
        expression: expression.clone(),
        requires,
    })
}

enum SequenceStepEdit {
    Replace {
        target: usize,
        step: CalculationSequenceStep,
    },
    Append {
        step: CalculationSequenceStep,
    },
    Insert {
        position: usize,
        step: CalculationSequenceStep,
    },
    Remove {
        target: usize,
    },
    Move {
        from: usize,
        to: usize,
    },
}

fn move_sequence_item<T>(items: &mut Vec<T>, from: usize, to: usize) {
    let item = items.remove(from);
    items.insert(to, item);
}

impl SequenceStepEdit {
    fn apply(self, steps: &mut Vec<CalculationSequenceStep>) {
        match self {
            Self::Replace { target, step } => steps[target] = step,
            Self::Append { step } => steps.push(step),
            Self::Insert { position, step } => steps.insert(position, step),
            Self::Remove { target } => {
                steps.remove(target);
            }
            Self::Move { from, to } => move_sequence_item(steps, from, to),
        }
    }
}

fn prepare_sequence_move(
    steps: &[CalculationSequenceStep],
    step_id: &str,
    anchor_id: &str,
    before: bool,
) -> Result<SequenceStepEdit> {
    let from = steps
        .iter()
        .position(|step| step.id == step_id)
        .ok_or_else(|| {
            failure(
                "SEQUENCE_MOVE_STEP_UNKNOWN",
                format!("手順 {step_id} は定義されていません。"),
            )
        })?;
    let anchor = steps
        .iter()
        .position(|step| step.id == anchor_id)
        .ok_or_else(|| {
            failure(
                "SEQUENCE_MOVE_ANCHOR_UNKNOWN",
                format!("移動の基準となる手順 {anchor_id} は定義されていません。"),
            )
        })?;
    if from == anchor {
        return Err(failure(
            "SEQUENCE_MOVE_SAME_STEP",
            "同じ手順を移動先の基準にできません。",
        ));
    }
    let to = anchor - usize::from(from < anchor) + usize::from(!before);
    if from == to {
        return Err(failure(
            "SEQUENCE_MOVE_UNCHANGED",
            "指定した位置に手順は既にあります。",
        ));
    }
    let mut ordered = steps.iter().collect::<Vec<_>>();
    move_sequence_item(&mut ordered, from, to);
    let mut seen = BTreeSet::new();
    for step in ordered {
        if let Some(prerequisite) = step.requires.iter().find(|id| !seen.contains(id.as_str())) {
            return Err(failure(
                "SEQUENCE_MOVE_DEPENDENCY_ORDER",
                format!(
                    "手順 {} が参照する手順 {prerequisite} を、先に実行できる順序にしてください。",
                    step.id
                ),
            ));
        }
        seen.insert(step.id.as_str());
    }
    Ok(SequenceStepEdit::Move { from, to })
}

#[derive(serde::Serialize)]
struct SequenceDependencyRevision<'a> {
    kind: &'static str,
    selected_result_id: &'a str,
    original_result_id: &'a str,
    previous_raw_sha256: &'a str,
    correction_input_id: &'a str,
    correction_raw_sha256: &'a str,
    step_id: &'a str,
    previous_expression: &'a SourceSpan,
    revised_expression: &'a SourceSpan,
    previous_requires: &'a [String],
    revised_requires: &'a [String],
    dependencies: &'a [String],
}

#[derive(serde::Serialize)]
struct SequenceAppendArtifact<'a> {
    kind: &'static str,
    selected_result_id: &'a str,
    original_result_id: &'a str,
    append_input_id: &'a str,
    append_raw_sha256: &'a str,
    previous_final_step_id: &'a str,
    appended_step: &'a CalculationSequenceStep,
    dependencies: &'a [String],
}

#[derive(serde::Serialize)]
struct SequenceInsertArtifact<'a> {
    kind: &'static str,
    selected_result_id: &'a str,
    original_result_id: &'a str,
    insert_input_id: &'a str,
    insert_raw_sha256: &'a str,
    anchor_step_id: &'a str,
    before: bool,
    inserted_step: &'a CalculationSequenceStep,
    previous_step_order: &'a [&'a str],
    revised_step_order: &'a [&'a str],
    dependencies: &'a [String],
}

#[derive(serde::Serialize)]
struct SequenceRemoveArtifact<'a> {
    kind: &'static str,
    selected_result_id: &'a str,
    original_result_id: &'a str,
    removal_input_id: &'a str,
    removal_raw_sha256: &'a str,
    removed_step_source_input_id: &'a str,
    removed_step_source_sha256: &'a str,
    removed_step: &'a CalculationSequenceStep,
    previous_final_step_id: &'a str,
    revised_final_step_id: &'a str,
    dependencies: &'a [String],
}

#[derive(serde::Serialize)]
struct SequenceMoveArtifact<'a> {
    kind: &'static str,
    selected_result_id: &'a str,
    original_result_id: &'a str,
    move_input_id: &'a str,
    move_raw_sha256: &'a str,
    step_id: &'a str,
    anchor_step_id: &'a str,
    before: bool,
    previous_step_order: &'a [&'a str],
    revised_step_order: &'a [&'a str],
    dependencies: &'a [String],
}

struct ReplayedCalculationSequence<'a> {
    original: &'a ResultRecord,
    selected: &'a ResultRecord,
    active: &'a ResultRecord,
    steps: Vec<CalculationSequenceStep>,
    sources: BTreeMap<String, &'a InputBundle>,
    remaining_bytes: usize,
    observed: CalculationSequenceResult,
    units: Vec<String>,
    history_units: Option<Vec<String>>,
}

impl<'a> ReplayedCalculationSequence<'a> {
    fn dependencies(&self) -> Vec<String> {
        let mut dependencies = vec![self.selected.id.clone()];
        if self.selected.id != self.active.id {
            dependencies.push(self.active.id.clone());
        }
        dependencies.extend(self.active.dependencies.iter().cloned());
        dependencies
    }

    fn explain_step(
        &self,
        step_id: &str,
        budget: &ResourceBudget,
    ) -> Result<(&Calculation, Vec<String>)> {
        let step = self
            .observed
            .steps
            .iter()
            .find(|step| step.id == step_id)
            .ok_or_else(|| {
                failure(
                    "SEQUENCE_EXPLANATION_STEP_UNKNOWN",
                    format!("選択した計算の版に手順 {step_id} はありません。"),
                )
            })?;
        let source = self
            .sources
            .get(step_id)
            .ok_or_else(invalid_sequence_proof)?;
        let source_id = identity(&source.request)?;
        let expression = &step.source_expression;
        let mut units = Vec::new();
        append_sequence_step_units(&mut units, step);
        units.push(format!(
            "式の出典: 結果ID {source_id}、原文SHA-256 {}、UTF-8バイト [{}, {})",
            source.raw_sha256, expression.start, expression.end,
        ));
        if units.join("\n").len() > budget.max_evidence_bytes {
            return Err(failure(
                "RESOURCE_LIMIT",
                "指定した計算手順の説明が要求の証拠予算を超えています。",
            ));
        }
        Ok((&step.calculation, units))
    }

    fn revise(
        &mut self,
        bundle: &'a InputBundle,
        graph: Option<&mut Scheduler>,
        budget: &ResourceBudget,
        started: Instant,
        dependencies: &mut Vec<String>,
        mut artifact_ref: impl FnMut(&[u8]) -> Result<String>,
    ) -> Result<CalculationSequenceResult> {
        let edit = sequence_step_edit(&self.steps, bundle)?;
        let proof = self.dependencies();
        let edit_id = identity(&bundle.request)?;
        let (revision, description) = match &edit {
            SequenceStepEdit::Replace { target, step } => {
                let preceding = &self.steps[*target];
                let id = &step.id;
                let source = self.sources[id];
                let revision =
                    if bundle.meaning.parser_version == input::SEQUENCE_REWIRE_PARSER_VERSION {
                        canonical(&SequenceDependencyRevision {
                            kind: "minidora-calculation-sequence-correction-v3",
                            selected_result_id: &self.selected.id,
                            original_result_id: &self.original.id,
                            previous_raw_sha256: &source.raw_sha256,
                            correction_input_id: &edit_id,
                            correction_raw_sha256: &bundle.raw_sha256,
                            step_id: id,
                            previous_expression: &preceding.expression,
                            revised_expression: &step.expression,
                            previous_requires: &preceding.requires,
                            revised_requires: &step.requires,
                            dependencies: &proof,
                        })?
                    } else {
                        let kind = if self.active.id == self.original.id {
                            "minidora-calculation-sequence-correction-v1"
                        } else {
                            "minidora-calculation-sequence-correction-v2"
                        };
                        canonical(&(
                            kind,
                            bundle.meaning.context_result_ref.as_deref(),
                            &self.original.id,
                            &source.raw_sha256,
                            &edit_id,
                            &bundle.raw_sha256,
                            id,
                            &preceding.expression,
                            &step.expression,
                            &proof,
                        ))?
                    };
                self.sources.insert(id.clone(), bundle);
                (revision, format!("訂正した手順: {id}"))
            }
            SequenceStepEdit::Append { step } => {
                let preceding = self.steps.last().ok_or_else(invalid_sequence_proof)?;
                let revision = canonical(&SequenceAppendArtifact {
                    kind: "minidora-calculation-sequence-append-v1",
                    selected_result_id: &self.selected.id,
                    original_result_id: &self.original.id,
                    append_input_id: &edit_id,
                    append_raw_sha256: &bundle.raw_sha256,
                    previous_final_step_id: &preceding.id,
                    appended_step: step,
                    dependencies: &proof,
                })?;
                self.sources.insert(step.id.clone(), bundle);
                (revision, format!("追加した手順: {}", step.id))
            }
            SequenceStepEdit::Insert { position, step } => {
                let Intent::InsertCalculationSequenceStep {
                    anchor_id, before, ..
                } = &bundle.meaning.intent
                else {
                    return Err(invalid_sequence_proof());
                };
                let previous = self
                    .steps
                    .iter()
                    .map(|step| step.id.as_str())
                    .collect::<Vec<_>>();
                let mut revised = previous.clone();
                revised.insert(*position, &step.id);
                let revision = canonical(&SequenceInsertArtifact {
                    kind: "minidora-calculation-sequence-insert-v1",
                    selected_result_id: &self.selected.id,
                    original_result_id: &self.original.id,
                    insert_input_id: &edit_id,
                    insert_raw_sha256: &bundle.raw_sha256,
                    anchor_step_id: anchor_id,
                    before: *before,
                    inserted_step: step,
                    previous_step_order: &previous,
                    revised_step_order: &revised,
                    dependencies: &proof,
                })?;
                self.sources.insert(step.id.clone(), bundle);
                (revision, format!("挿入した手順: {}", step.id))
            }
            SequenceStepEdit::Remove { target } => {
                let removed = &self.steps[*target];
                let source = self.sources[&removed.id];
                let preceding = self.steps.last().ok_or_else(invalid_sequence_proof)?;
                let revised = self
                    .steps
                    .iter()
                    .rev()
                    .find(|step| step.id != removed.id)
                    .ok_or_else(invalid_sequence_proof)?;
                let source_id = identity(&source.request)?;
                let revision = canonical(&SequenceRemoveArtifact {
                    kind: "minidora-calculation-sequence-remove-v1",
                    selected_result_id: &self.selected.id,
                    original_result_id: &self.original.id,
                    removal_input_id: &edit_id,
                    removal_raw_sha256: &bundle.raw_sha256,
                    removed_step_source_input_id: &source_id,
                    removed_step_source_sha256: &source.raw_sha256,
                    removed_step: removed,
                    previous_final_step_id: &preceding.id,
                    revised_final_step_id: &revised.id,
                    dependencies: &proof,
                })?;
                self.sources.remove(&removed.id);
                (revision, format!("削除した手順: {}", removed.id))
            }
            SequenceStepEdit::Move { from, to } => {
                let Intent::MoveCalculationSequenceStep {
                    step_id,
                    anchor_id,
                    before,
                } = &bundle.meaning.intent
                else {
                    return Err(invalid_sequence_proof());
                };
                let previous = self
                    .steps
                    .iter()
                    .map(|step| step.id.as_str())
                    .collect::<Vec<_>>();
                let mut revised = previous.clone();
                move_sequence_item(&mut revised, *from, *to);
                let revision = canonical(&SequenceMoveArtifact {
                    kind: "minidora-calculation-sequence-move-v1",
                    selected_result_id: &self.selected.id,
                    original_result_id: &self.original.id,
                    move_input_id: &edit_id,
                    move_raw_sha256: &bundle.raw_sha256,
                    step_id,
                    anchor_step_id: anchor_id,
                    before: *before,
                    previous_step_order: &previous,
                    revised_step_order: &revised,
                    dependencies: &proof,
                })?;
                (revision, format!("移動した手順: {step_id}"))
            }
        };
        edit.apply(&mut self.steps);
        let mut private_graph;
        let graph = if let Some(graph) = graph {
            graph
        } else {
            private_graph = sequence_proof_graph(&self.steps, budget)?;
            &mut private_graph
        };
        let mut retain = |bytes: &[u8]| {
            sequence_history_time(budget, started)?;
            debit_sequence_evidence(&mut self.remaining_bytes, bytes)?;
            artifact_ref(bytes)
        };
        dependencies.push(self.selected.id.clone());
        if self.selected.id != self.active.id {
            dependencies.push(self.active.id.clone());
        }
        dependencies.push(retain(&revision)?);
        let mut revised = run_calculation_sequence(
            &self.steps,
            |id| &self.sources[id].request.text,
            graph,
            budget,
            started,
            dependencies,
            retain,
        )?;
        revised.units.insert(0, description);
        Ok(revised)
    }
}

fn sequence_history_edit_label(intent: &Intent) -> Result<String> {
    let label = match intent {
        Intent::CorrectCalculationSequenceStep {
            step_id,
            change_dependencies,
            ..
        } => {
            let label = if *change_dependencies {
                "参照先も訂正"
            } else {
                "訂正"
            };
            format!("{label} {step_id}")
        }
        Intent::AppendCalculationSequenceStep { step_id, .. } => format!("追加 {step_id}"),
        Intent::RemoveCalculationSequenceStep { step_id } => format!("削除 {step_id}"),
        Intent::MoveCalculationSequenceStep {
            step_id,
            anchor_id,
            before,
        }
        | Intent::InsertCalculationSequenceStep {
            step_id,
            anchor_id,
            before,
            ..
        } => {
            let label = if matches!(intent, Intent::MoveCalculationSequenceStep { .. }) {
                "移動"
            } else {
                "挿入"
            };
            let side = if *before { "前" } else { "後" };
            format!("{label} {step_id} ({anchor_id} の{side})")
        }
        _ => return Err(invalid_sequence_proof()),
    };
    Ok(label)
}

fn append_sequence_history_row(
    units: &mut Vec<String>,
    remaining: &mut usize,
    label: &str,
    calculation: &Calculation,
    final_step: &CalculationSequenceStep,
    result: &ResultRecord,
) -> Result<()> {
    let row = format!(
        "{label}: 最終段 {} = {} [結果ID: {}]",
        final_step.id, calculation.exact_value, result.id,
    );
    *remaining = remaining
        .checked_sub(row.len() + usize::from(!units.is_empty()))
        .ok_or_else(|| {
            failure(
                "RESOURCE_LIMIT",
                "変更履歴の表示が要求の証拠予算を超えています。",
            )
        })?;
    units.push(row);
    Ok(())
}

fn recheck_calculation_sequence_origin(
    store: &Store,
    base: &InputBundle,
    original: &ResultRecord,
    graph: &mut Scheduler,
    budget: &ResourceBudget,
    started: Instant,
    remaining_bytes: &mut usize,
) -> Result<CalculationSequenceResult> {
    let Intent::CalculateSequence { steps } = &base.meaning.intent else {
        return Err(invalid_sequence_proof());
    };
    let mut dependencies = Vec::new();
    let observed = run_calculation_sequence(
        steps,
        |_| &base.request.text,
        graph,
        budget,
        started,
        &mut dependencies,
        |expected| {
            debit_sequence_evidence(remaining_bytes, expected)?;
            read_sequence_artifact(store, &base.request, expected)
        },
    )?;
    let plan = original
        .content_plan
        .as_ref()
        .ok_or_else(invalid_sequence_proof)?;
    if original.dependencies != dependencies || original.text != plan.text {
        return Err(invalid_sequence_proof());
    }
    observed.verify_content(original, plan)?;
    Ok(observed)
}

fn replay_calculation_sequence<'a>(
    store: &Store,
    context: &'a PreviousCalculationSequence<'_>,
    graph: &mut Scheduler,
    budget: &ResourceBudget,
    started: Instant,
    show_history: bool,
) -> Result<ReplayedCalculationSequence<'a>> {
    let original = context.original;
    let mut remaining_bytes = budget.max_evidence_bytes;
    let base = &context.input;
    let steps = context.root_steps();
    let mut prior_graph = (context.edits != 0)
        .then(|| sequence_proof_graph(steps, budget))
        .transpose()?;
    let observed = recheck_calculation_sequence_origin(
        store,
        base,
        original,
        prior_graph.as_mut().unwrap_or(&mut *graph),
        budget,
        started,
        &mut remaining_bytes,
    )?;
    let units = sequence_explanation_units(&observed, budget)?;
    let history_until = if show_history {
        Some(context.followups.len())
    } else {
        context.followups.iter().rposition(|entry| {
            matches!(
                entry.bundle.meaning.intent,
                Intent::ShowCalculationSequenceHistory
            )
        })
    };
    let mut remaining_history_bytes = budget.max_evidence_bytes;
    let history_units = if history_until.is_some() {
        let mut history = Vec::new();
        append_sequence_history_row(
            &mut history,
            &mut remaining_history_bytes,
            "元の計算",
            &observed.calculation,
            steps.last().ok_or_else(invalid_sequence_proof)?,
            original,
        )?;
        Some(history)
    } else {
        None
    };
    let mut replay = ReplayedCalculationSequence {
        original,
        selected: original,
        active: original,
        steps: steps.to_vec(),
        sources: steps.iter().map(|step| (step.id.clone(), base)).collect(),
        remaining_bytes,
        observed,
        units,
        history_units,
    };
    let mut edits_left = context.edits;
    for (index, followup) in context.followups.iter().enumerate() {
        sequence_history_time(budget, started)?;
        let result = followup.result;
        let plan = result
            .content_plan
            .as_ref()
            .ok_or_else(invalid_sequence_proof)?;
        if result.text != plan.text || !result.evidence.is_empty() {
            return Err(invalid_sequence_proof());
        }
        if input::sequence_edit_parser(&followup.bundle.meaning.intent).is_some() {
            edits_left -= 1;
            let mut dependencies = Vec::new();
            let observed = replay.revise(
                &followup.bundle,
                (edits_left == 0).then_some(&mut *graph),
                budget,
                started,
                &mut dependencies,
                |expected| read_sequence_artifact(store, &base.request, expected),
            )?;
            if result.dependencies != dependencies {
                return Err(invalid_sequence_proof());
            }
            observed.verify_content(result, plan)?;
            if history_until.is_some_and(|end| index < end)
                && let Some(history) = replay.history_units.as_mut()
            {
                let change = sequence_history_edit_label(&followup.bundle.meaning.intent)?;
                let label = format!("{}. {change}", history.len());
                append_sequence_history_row(
                    history,
                    &mut remaining_history_bytes,
                    &label,
                    &observed.calculation,
                    replay.steps.last().ok_or_else(invalid_sequence_proof)?,
                    result,
                )?;
            }
            replay.units = sequence_explanation_units(&observed, budget)?;
            replay.observed = observed;
            replay.active = result;
        } else {
            let step_units;
            let (calculation, units) = match &followup.bundle.meaning.intent {
                Intent::ShowCalculationSequenceHistory => (
                    &replay.observed.calculation,
                    replay
                        .history_units
                        .as_deref()
                        .ok_or_else(invalid_sequence_proof)?,
                ),
                Intent::ExplainCalculationSequenceStep { step_id } => {
                    let (calculation, units) = replay.explain_step(step_id, budget)?;
                    step_units = units;
                    (calculation, step_units.as_slice())
                }
                _ => (&replay.observed.calculation, replay.units.as_slice()),
            };
            if result.dependencies != replay.dependencies()
                || result.calculation.as_ref() != Some(calculation)
                || result.guarantee != Guarantee::Computed
                || plan.guarantee != Guarantee::Computed
                || plan.required_units != units
                || plan.text != units.join("\n")
            {
                return Err(invalid_sequence_proof());
            }
        }
        replay.selected = result;
    }
    Ok(replay)
}

pub(crate) fn calculation_sequence_report(
    store: &Store,
    context: &PreviousCalculationSequence<'_>,
    intent: &Intent,
    graph: &mut Scheduler,
    budget: &ResourceBudget,
    started: Instant,
) -> Result<CalculationSequenceExplanation> {
    let show_history = matches!(intent, Intent::ShowCalculationSequenceHistory);
    let replay = replay_calculation_sequence(store, context, graph, budget, started, show_history)?;
    let dependencies = replay.dependencies();
    let (calculation, units) = match intent {
        Intent::ExplainPrevious => (replay.observed.calculation, replay.units),
        Intent::ShowCalculationSequenceHistory => (
            replay.observed.calculation,
            replay.history_units.ok_or_else(invalid_sequence_proof)?,
        ),
        Intent::ExplainCalculationSequenceStep { step_id } => {
            let (calculation, units) = replay.explain_step(step_id, budget)?;
            (calculation.clone(), units)
        }
        _ => return Err(invalid_sequence_proof()),
    };
    Ok(CalculationSequenceExplanation {
        calculation,
        units,
        dependencies,
    })
}

pub(crate) struct SequenceEdit<'a> {
    baseline: PreviousCalculationSequence<'a>,
    pub(crate) steps: Vec<CalculationSequenceStep>,
    predicate: &'static str,
}

impl SequenceEdit<'_> {
    pub(crate) fn verification_actions(&self) -> usize {
        self.baseline.effective_steps().len() + self.baseline.context_actions()
    }

    pub(crate) fn predicate(&self) -> &'static str {
        self.predicate
    }
}

fn sequence_edit_context_requirement(intent: &Intent) -> (&'static str, &'static str) {
    match intent {
        Intent::AppendCalculationSequenceStep { .. } => (
            "SEQUENCE_APPEND_CONTEXT_REQUIRED",
            "追加対象となる同じケースの確定済み連続計算が必要です。",
        ),
        Intent::RemoveCalculationSequenceStep { .. } => (
            "SEQUENCE_REMOVE_CONTEXT_REQUIRED",
            "削除対象となる同じケースの確定済み連続計算が必要です。",
        ),
        Intent::MoveCalculationSequenceStep { .. } => (
            "SEQUENCE_MOVE_CONTEXT_REQUIRED",
            "移動対象となる同じケースの確定済み連続計算が必要です。",
        ),
        Intent::InsertCalculationSequenceStep { .. } => (
            "SEQUENCE_INSERT_CONTEXT_REQUIRED",
            "挿入対象となる同じケースの確定済み連続計算が必要です。",
        ),
        _ => (
            "SEQUENCE_CORRECTION_CONTEXT_REQUIRED",
            "訂正対象となる同じケースの確定済み連続計算が必要です。",
        ),
    }
}

pub(crate) fn prepare_sequence_edit<'a>(
    store: &Store,
    bundle: &InputBundle,
    previous: &'a ResultRecord,
    prior_results: &'a BTreeMap<String, ResultRecord>,
    started: Instant,
) -> Result<SequenceEdit<'a>> {
    if input::sequence_edit_parser(&bundle.meaning.intent)
        != Some(bundle.meaning.parser_version.as_str())
    {
        return Err(invalid_sequence_proof());
    }
    let baseline =
        previous_calculation_sequence(store, bundle, previous, prior_results, Some(started))?
            .ok_or_else(|| {
                let (code, message) = sequence_edit_context_requirement(&bundle.meaning.intent);
                failure(code, message)
            })?;
    let mut steps = baseline.effective_steps().to_vec();
    sequence_step_edit(&steps, bundle)?.apply(&mut steps);
    let predicate = match bundle.meaning.parser_version.as_str() {
        input::SEQUENCE_APPEND_PARSER_VERSION => SEQUENCE_APPEND_PREDICATE,
        input::SEQUENCE_REMOVE_PARSER_VERSION => SEQUENCE_REMOVE_PREDICATE,
        input::SEQUENCE_MOVE_PARSER_VERSION => SEQUENCE_MOVE_PREDICATE,
        input::SEQUENCE_INSERT_PARSER_VERSION => SEQUENCE_INSERT_PREDICATE,
        input::SEQUENCE_REWIRE_PARSER_VERSION => REWIRED_SEQUENCE_CORRECTION_PREDICATE,
        _ if baseline.edits > 0 => REPEATED_SEQUENCE_CORRECTION_PREDICATE,
        _ => SEQUENCE_CORRECTION_PREDICATE,
    };
    Ok(SequenceEdit {
        baseline,
        steps,
        predicate,
    })
}

pub(crate) fn run_sequence_edit(
    store: &Store,
    bundle: &InputBundle,
    context: &SequenceEdit<'_>,
    graph: &mut Scheduler,
    started: Instant,
    dependencies: &mut Vec<String>,
    artifact_ref: impl FnMut(&[u8]) -> Result<String>,
) -> Result<CalculationSequenceResult> {
    let budget = &bundle.request.budget;
    let mut prior_graph = sequence_proof_graph(context.baseline.effective_steps(), budget)?;
    let mut replay = replay_calculation_sequence(
        store,
        &context.baseline,
        &mut prior_graph,
        budget,
        started,
        false,
    )?;
    replay.revise(
        bundle,
        Some(graph),
        budget,
        started,
        dependencies,
        artifact_ref,
    )
}

pub(crate) fn store_execution_graph(
    store: &Store,
    actor: &str,
    case: &str,
    graph: &Scheduler,
) -> Result<String> {
    store.put_artifact(
        actor,
        case,
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
    )
}

const CORE_COMPUTATION_ARTIFACT_KIND_V1: &str = "minidora.core-computation.relation-meaning.v1";
const CORE_COMPUTATION_ARTIFACT_KIND_V2: &str = "minidora.core-computation.relation-meaning.v2";
const CORE_COMPUTATION_ARTIFACT_KIND_V3: &str = "minidora.core-computation.relation-meaning.v3";
const CORE_COMPUTATION_ARTIFACT_KIND_V4: &str = "minidora.core-computation.relation-meaning.v4";
const CORE_COMPUTATION_ARTIFACT_KIND_V5: &str = "minidora.core-computation.relation-meaning.v5";
const CORE_COMPUTATION_ARTIFACT_KIND_V6: &str = "minidora.core-computation.relation-meaning.v6";
const CORE_COMPUTATION_ARTIFACT_KIND_V7: &str = "minidora.core-computation.relation-meaning.v7";
const CORE_COMPUTATION_ARTIFACT_KIND: &str = CORE_COMPUTATION_ARTIFACT_KIND_V4;
const CORE_RELATION_MEANING_PLAN_SCHEMA: &str = "core-relation-meaning-plan-v1";
const DIRECT_ATOMIC_MEANING_KIND: &str = "CORE_TYPED_ATOMIC_MEANING_V1";
const DIRECT_ATOMIC_MEANING_PARSER: &str = "core-typed-atomic-meaning-v1";
const DIRECT_TYPED_RELATION_ASSERTION_KIND: &str = "CORE_TYPED_RELATION_ASSERTION_V1";
const DIRECT_TYPED_RELATION_ASSERTION_PARSER: &str = "core-typed-relation-assertion-v1";
const DIRECT_TYPED_RELATION_CORRECTION_KIND: &str = "CORE_TYPED_RELATION_CORRECTION_V1";
const DIRECT_TYPED_RELATION_CORRECTION_PARSER: &str = "core-typed-relation-correction-v1";
const DIRECT_TYPED_RELATION_RULE_KIND: &str = "CORE_TYPED_RELATION_RULE_V1";
const DIRECT_TYPED_RELATION_RULE_PARSER: &str = "core-typed-relation-rule-v1";
const DIRECT_TYPED_CONDITION_KIND: &str = "CORE_TYPED_CONDITION_DEFINITION_V1";
const DIRECT_TYPED_CONDITION_PARSER: &str = "core-typed-condition-definition-v1";
const DIRECT_RELATION_COMPARISON_KIND: &str = "CORE_TYPED_RELATION_COMPARISON_V1";
const DIRECT_RELATION_COMPARISON_PARSER: &str = "core-typed-relation-comparison-v1";
const EXACT_CALCULATION_PREDICATE: &str = "EXACT_CALCULATION_V1";
pub(crate) const GENERATED_CALCULATION_PREDICATE: &str = "EXACT_CALCULATION_V2";
pub(crate) const CALCULATION_CORRECTION_PREDICATE: &str =
    "EXACT_PREVIOUS_CALCULATION_CORRECTION_V1";
const PREVIOUS_LENGTH_CONVERSION_PREDICATE: &str = "EXACT_PREVIOUS_LENGTH_CONVERSION_V1";
const PREVIOUS_WEB_LENGTH_CONVERSION_PREDICATE: &str = "EXACT_PREVIOUS_WEB_LENGTH_CONVERSION_V1";
pub(crate) const SEQUENCE_CORRECTION_PREDICATE: &str = "EXACT_CALCULATION_SEQUENCE_CORRECTION_V1";
pub(crate) const REPEATED_SEQUENCE_CORRECTION_PREDICATE: &str =
    "EXACT_CALCULATION_SEQUENCE_CORRECTION_V2";
pub(crate) const REWIRED_SEQUENCE_CORRECTION_PREDICATE: &str =
    "EXACT_CALCULATION_SEQUENCE_CORRECTION_V3";
pub(crate) const SEQUENCE_APPEND_PREDICATE: &str = "EXACT_CALCULATION_SEQUENCE_APPEND_V1";
pub(crate) const SEQUENCE_REMOVE_PREDICATE: &str = "EXACT_CALCULATION_SEQUENCE_REMOVE_V1";
pub(crate) const SEQUENCE_MOVE_PREDICATE: &str = "EXACT_CALCULATION_SEQUENCE_MOVE_V1";
pub(crate) const SEQUENCE_INSERT_PREDICATE: &str = "EXACT_CALCULATION_SEQUENCE_INSERT_V1";
const GENERATED_CALCULATION_KIND: &str = "minidora.exact-calculation-content.v1";
pub(crate) const RELATION_EXPLANATION_PREDICATE: &str = "RELATION_PROOF_EXPLANATION_V1";
pub(crate) const GENERATED_RELATION_EXPLANATION_PREDICATE: &str = "RELATION_PROOF_EXPLANATION_V2";
const LEGACY_RELATION_GENERATION_BYTES: usize = 8192;
const GROUP_MEAN_HYPOTHESIS_PREDICATE: &str = "GROUP_MEAN_HYPOTHESIS_RESULT_V1";

pub(crate) const RELATION_COMPARISON_PREDICATE: &str = "GENERATED_RELATION_COMPARISON_V1";
pub(crate) const SEQUENCE_EXPLANATION_PREDICATE: &str = "EXACT_PREVIOUS_CALCULATION_SEQUENCE_V1";
pub(crate) const CHAINED_SEQUENCE_EXPLANATION_PREDICATE: &str =
    "EXACT_PREVIOUS_CALCULATION_SEQUENCE_V2";
pub(crate) const REVISED_SEQUENCE_EXPLANATION_PREDICATE: &str =
    "EXACT_PREVIOUS_CALCULATION_SEQUENCE_V3";
pub(crate) const REWIRED_SEQUENCE_EXPLANATION_PREDICATE: &str =
    "EXACT_PREVIOUS_CALCULATION_SEQUENCE_V4";
pub(crate) const APPENDED_SEQUENCE_EXPLANATION_PREDICATE: &str =
    "EXACT_PREVIOUS_CALCULATION_SEQUENCE_V5";
pub(crate) const REMOVED_SEQUENCE_EXPLANATION_PREDICATE: &str =
    "EXACT_PREVIOUS_CALCULATION_SEQUENCE_V6";
pub(crate) const MOVED_SEQUENCE_EXPLANATION_PREDICATE: &str =
    "EXACT_PREVIOUS_CALCULATION_SEQUENCE_V7";
pub(crate) const INSERTED_SEQUENCE_EXPLANATION_PREDICATE: &str =
    "EXACT_PREVIOUS_CALCULATION_SEQUENCE_V8";
pub(crate) const SEQUENCE_HISTORY_PREDICATE: &str = "EXACT_CALCULATION_SEQUENCE_HISTORY_V1";
pub(crate) const SEQUENCE_STEP_EXPLANATION_PREDICATE: &str =
    "EXACT_CALCULATION_SEQUENCE_STEP_EXPLANATION_V1";
pub(crate) const CONDITION_EVALUATION_PREDICATE: &str = "EXACT_CONDITION_EVALUATION_V1";
pub(crate) const CONDITION_EVALUATION_DETAIL_PREDICATE: &str =
    "EXACT_CONDITION_EVALUATION_DETAIL_V1";
pub(crate) const FACT_SOURCE_PREDICATE: &str = "EXACT_FACT_SOURCE_V1";
pub(crate) const CONDITION_SOURCE_PREDICATE: &str = "EXACT_CONDITION_SOURCE_V1";
pub(crate) const CONDITION_SOURCE_BY_DEFINITION_PREDICATE: &str =
    "EXACT_CONDITION_SOURCE_BY_DEFINITION_V1";
pub(crate) const RULE_SOURCE_PREDICATE: &str = "EXACT_RULE_SOURCE_V1";
pub(crate) const DECLARED_RULE_SOURCE_PREDICATE: &str = "EXACT_DECLARED_RULE_SOURCE_V1";
pub(crate) const PREVIOUS_RULE_SOURCE_PREDICATE: &str = "EXACT_PREVIOUS_RULE_SOURCE_V1";
pub(crate) const PREVIOUS_COMPOSED_RULE_SOURCE_PREDICATE: &str =
    "EXACT_PREVIOUS_COMPOSED_RULE_SOURCE_V1";
pub(crate) const PREVIOUS_RULE_SOURCE_BY_ID_PREDICATE: &str = "EXACT_PREVIOUS_RULE_SOURCE_BY_ID_V1";
pub(crate) const PREVIOUS_RULE_LIST_PREDICATE: &str = "EXACT_PREVIOUS_RULE_LIST_V1";
pub(crate) const LISTED_RULE_SOURCE_PREDICATE: &str = "EXACT_LISTED_RULE_SOURCE_V1";
pub(crate) const COMPOSED_RULE_SOURCE_PREDICATE: &str = "EXACT_COMPOSED_RULE_SOURCE_V1";
pub(crate) const GUARDED_RELATION_RULE_PREDICATE: &str = "EXACT_GUARDED_RELATION_RULE_V1";
pub(crate) const GUARDED_CONJUNCTIVE_RULE_PREDICATE: &str = "EXACT_GUARDED_CONJUNCTIVE_RULE_V1";
pub(crate) const MULTI_CONDITION_RULE_PREDICATE: &str = "EXACT_MULTI_CONDITION_RULE_V1";
pub(crate) const MULTI_CONDITION_PREMISE_PREDICATE: &str = "EXACT_MULTI_CONDITION_PREMISE_V1";

fn guarded_rule_predicate(meaning: &MeaningIR) -> Option<&'static str> {
    match (&meaning.intent, meaning.parser_version.as_str()) {
        (Intent::AddRule { .. }, input::GUARDED_RELATION_RULE_PARSER_VERSION) => {
            Some(GUARDED_RELATION_RULE_PREDICATE)
        }
        (Intent::AddConjunctiveRule { .. }, input::GUARDED_CONJUNCTIVE_RULE_PARSER_VERSION) => {
            Some(GUARDED_CONJUNCTIVE_RULE_PREDICATE)
        }
        (
            Intent::AddRule { .. } | Intent::AddConjunctiveRule { .. },
            input::MULTI_CONDITION_RULE_PARSER_VERSION,
        ) => Some(MULTI_CONDITION_RULE_PREDICATE),
        _ => None,
    }
}

pub(crate) fn rule_declaration(intent: &Intent) -> Option<(&[Proposition], &Proposition)> {
    match intent {
        Intent::AddRule {
            premise,
            conclusion,
        } => Some((std::slice::from_ref(premise), conclusion)),
        Intent::AddConjunctiveRule {
            premises,
            conclusion,
        } => Some((premises, conclusion)),
        _ => None,
    }
}

pub(crate) fn record_source_contract(intent: &Intent) -> Option<(&'static str, &'static str)> {
    match intent {
        Intent::InspectFactSource { .. } => {
            Some((input::FACT_SOURCE_PARSER_VERSION, FACT_SOURCE_PREDICATE))
        }
        Intent::InspectConditionSource { .. } => Some((
            input::CONDITION_SOURCE_PARSER_VERSION,
            CONDITION_SOURCE_PREDICATE,
        )),
        Intent::InspectConditionSourceByDefinition { .. } => Some((
            input::CONDITION_SOURCE_BY_DEFINITION_PARSER_VERSION,
            CONDITION_SOURCE_BY_DEFINITION_PREDICATE,
        )),
        Intent::InspectRuleSource { .. } => {
            Some((input::RULE_SOURCE_PARSER_VERSION, RULE_SOURCE_PREDICATE))
        }
        Intent::InspectDeclaredRuleSource { .. } => Some((
            input::DECLARED_RULE_SOURCE_PARSER_VERSION,
            DECLARED_RULE_SOURCE_PREDICATE,
        )),
        Intent::InspectPreviousRuleSource => Some((
            input::PREVIOUS_RULE_SOURCE_PARSER_VERSION,
            PREVIOUS_RULE_SOURCE_PREDICATE,
        )),
        Intent::InspectPreviousComposedRuleSource => Some((
            input::PREVIOUS_COMPOSED_RULE_SOURCE_PARSER_VERSION,
            PREVIOUS_COMPOSED_RULE_SOURCE_PREDICATE,
        )),
        Intent::InspectPreviousRuleSourceById { .. } => Some((
            input::PREVIOUS_RULE_SOURCE_BY_ID_PARSER_VERSION,
            PREVIOUS_RULE_SOURCE_BY_ID_PREDICATE,
        )),
        Intent::ListPreviousRules => Some((
            input::PREVIOUS_RULE_LIST_PARSER_VERSION,
            PREVIOUS_RULE_LIST_PREDICATE,
        )),
        Intent::AskListedRuleSource { .. } => Some((
            input::LISTED_RULE_SOURCE_PARSER_VERSION,
            LISTED_RULE_SOURCE_PREDICATE,
        )),
        Intent::InspectComposedRuleSource { .. } => Some((
            input::COMPOSED_RULE_SOURCE_PARSER_VERSION,
            COMPOSED_RULE_SOURCE_PREDICATE,
        )),
        _ => None,
    }
}

pub(crate) fn condition_evaluation_contract(
    intent: &Intent,
) -> Option<(&'static str, &'static str)> {
    match intent {
        Intent::EvaluateCondition { .. } => Some((
            input::CONDITION_EVALUATION_PARSER_VERSION,
            CONDITION_EVALUATION_PREDICATE,
        )),
        Intent::EvaluateConditionDetailed { .. } => Some((
            input::CONDITION_EVALUATION_DETAIL_PARSER_VERSION,
            CONDITION_EVALUATION_DETAIL_PREDICATE,
        )),
        _ => None,
    }
}

const WEB_ANSWER_PREDICATE: &str = "GROUNDED_WEB_ANSWER_V1";
const CODE_EXPLANATION_PREDICATE: &str = "SOURCE_CODE_EXPLANATION_V1";
const RETURN_FUNCTION_PREDICATE: &str = "SOURCE_RETURN_FUNCTION_V1";

pub(crate) fn goal_predicates(
    meaning: &MeaningIR,
    requires_file: bool,
    context_predicate: Option<&str>,
) -> Vec<String> {
    let mut predicates = Vec::new();
    match &meaning.intent {
        Intent::AskWeb { .. } => predicates.push(WEB_ANSWER_PREDICATE.into()),
        Intent::ExplainCode { .. } => predicates.push(CODE_EXPLANATION_PREDICATE.into()),
        Intent::GenerateReturnFunction { .. } => predicates.push(RETURN_FUNCTION_PREDICATE.into()),
        Intent::VerifyPreviousCode { .. }
            if meaning.parser_version == input::EXPLAINED_CODE_SINGLE_CASE_PARSER_VERSION =>
        {
            predicates.push(crate::code_generation::EXPLAINED_CODE_VERIFICATION_PREDICATE.into());
        }
        Intent::CompareRelationInventories { .. } if input::uses_generated_comparison(meaning) => {
            predicates.push(RELATION_COMPARISON_PREDICATE.into());
        }
        Intent::Calculate { .. } if meaning.parser_version == input::CALCULATION_PARSER_VERSION => {
            predicates.push(GENERATED_CALCULATION_PREDICATE.into());
        }
        Intent::Calculate { .. } => predicates.push(EXACT_CALCULATION_PREDICATE.into()),
        Intent::CorrectCalculation { .. }
            if meaning.parser_version == input::CALCULATION_CORRECTION_PARSER_VERSION =>
        {
            predicates.push(CALCULATION_CORRECTION_PREDICATE.into());
        }
        Intent::ConvertPreviousLength { .. }
            if meaning.parser_version == input::PREVIOUS_LENGTH_CONVERSION_PARSER_VERSION =>
        {
            predicates.push(PREVIOUS_LENGTH_CONVERSION_PREDICATE.into());
        }
        Intent::ConvertPreviousLength { .. }
            if meaning.parser_version == input::PREVIOUS_WEB_LENGTH_CONVERSION_PARSER_VERSION =>
        {
            predicates.push(PREVIOUS_WEB_LENGTH_CONVERSION_PREDICATE.into());
        }
        Intent::CalculateSequence { .. } => predicates.push("EXACT_CALCULATION_SEQUENCE_V1".into()),
        Intent::AddRule { .. } | Intent::AddConjunctiveRule { .. } => {
            if let Some(predicate) = guarded_rule_predicate(meaning) {
                predicates.push(predicate.into());
            }
        }
        Intent::Assert { .. }
            if meaning.parser_version == input::MULTI_CONDITION_RELATION_PARSER_VERSION =>
        {
            predicates.push(MULTI_CONDITION_PREMISE_PREDICATE.into());
        }
        Intent::AssertPreviousRelation { .. }
            if input::is_correction_context_assertion(&meaning.parser_version) =>
        {
            predicates.push(CORRECTION_ASSERTION_PREDICATE.into());
        }
        intent @ (Intent::InspectFactSource { .. }
        | Intent::InspectConditionSource { .. }
        | Intent::InspectConditionSourceByDefinition { .. }
        | Intent::InspectRuleSource { .. }
        | Intent::InspectDeclaredRuleSource { .. }
        | Intent::InspectPreviousRuleSource
        | Intent::InspectPreviousComposedRuleSource
        | Intent::InspectPreviousRuleSourceById { .. }
        | Intent::ListPreviousRules
        | Intent::AskListedRuleSource { .. }
        | Intent::InspectComposedRuleSource { .. }) => {
            if let Some((parser, predicate)) = record_source_contract(intent)
                && parser == meaning.parser_version
            {
                predicates.push(predicate.into());
            }
        }
        intent @ (Intent::EvaluateCondition { .. } | Intent::EvaluateConditionDetailed { .. }) => {
            if let Some((parser, predicate)) = condition_evaluation_contract(intent)
                && parser == meaning.parser_version
            {
                predicates.push(predicate.into());
            }
        }
        Intent::AppendCalculationSequenceStep { .. }
            if meaning.parser_version == input::SEQUENCE_APPEND_PARSER_VERSION =>
        {
            predicates.push(SEQUENCE_APPEND_PREDICATE.into());
        }
        Intent::RemoveCalculationSequenceStep { .. }
            if meaning.parser_version == input::SEQUENCE_REMOVE_PARSER_VERSION =>
        {
            predicates.push(SEQUENCE_REMOVE_PREDICATE.into());
        }
        Intent::MoveCalculationSequenceStep { .. }
            if meaning.parser_version == input::SEQUENCE_MOVE_PARSER_VERSION =>
        {
            predicates.push(SEQUENCE_MOVE_PREDICATE.into());
        }
        Intent::InsertCalculationSequenceStep { .. }
            if meaning.parser_version == input::SEQUENCE_INSERT_PARSER_VERSION =>
        {
            predicates.push(SEQUENCE_INSERT_PREDICATE.into());
        }
        Intent::CorrectCalculationSequenceStep { .. }
            if input::sequence_edit_parser(&meaning.intent)
                == Some(meaning.parser_version.as_str()) =>
        {
            let predicate = if meaning.parser_version == input::SEQUENCE_REWIRE_PARSER_VERSION {
                REWIRED_SEQUENCE_CORRECTION_PREDICATE
            } else {
                context_predicate.unwrap_or(SEQUENCE_CORRECTION_PREDICATE)
            };
            predicates.push(predicate.into());
        }
        Intent::ExplainPrevious
            if meaning.parser_version == input::PREVIOUS_CORRECTION_EXPLANATION_PARSER_VERSION =>
        {
            predicates.push(PREVIOUS_CORRECTION_EXPLANATION_PREDICATE.into());
        }
        Intent::ExplainPrevious => {
            predicates.extend(context_predicate.map(str::to_owned));
        }
        Intent::ShowCalculationSequenceHistory
            if meaning.parser_version == input::SEQUENCE_HISTORY_PARSER_VERSION =>
        {
            predicates.push(SEQUENCE_HISTORY_PREDICATE.into());
        }
        Intent::ExplainCalculationSequenceStep { .. }
            if meaning.parser_version == input::SEQUENCE_STEP_EXPLANATION_PARSER_VERSION =>
        {
            predicates.push(SEQUENCE_STEP_EXPLANATION_PREDICATE.into());
        }
        Intent::AskWhy { .. } if input::uses_generated_relation_explanation(meaning) => {
            predicates.push(GENERATED_RELATION_EXPLANATION_PREDICATE.into());
        }
        Intent::AskWhy { .. } => predicates.push(RELATION_EXPLANATION_PREDICATE.into()),
        Intent::RetainPreviousGroupMeanHypothesis
        | Intent::ExplainPreviousGroupMeanHypothesis
        | Intent::AssessPreviousGroupMeanHypothesis { .. } => {
            predicates.push(GROUP_MEAN_HYPOTHESIS_PREDICATE.into());
        }
        _ => {}
    }
    predicates.extend(["SUPPORTED_CONTENT".into(), "EXACT_RENDER".into()]);
    if requires_file {
        predicates.push("CONFIRMED_FILE".into());
    }
    predicates
}

struct ReportGoalContent {
    predicate: &'static str,
    guarantee: Guarantee,
    dependencies: Vec<String>,
    units: Vec<String>,
}

fn verify_report_goal_content(
    goal: &GoalContract,
    result: &ResultRecord,
    plan: &ContentPlan,
    compute_ref: &str,
    observed: Option<&ReportGoalContent>,
) -> Result<()> {
    let Some(predicate) = goal.required_predicates.iter().find(|predicate| {
        matches!(
            predicate.as_str(),
            RELATION_EXPLANATION_PREDICATE
                | GENERATED_RELATION_EXPLANATION_PREDICATE
                | GROUP_MEAN_HYPOTHESIS_PREDICATE
                | RELATION_COMPARISON_PREDICATE
                | GENERATED_CALCULATION_PREDICATE
                | CALCULATION_CORRECTION_PREDICATE
                | PREVIOUS_LENGTH_CONVERSION_PREDICATE
                | PREVIOUS_WEB_LENGTH_CONVERSION_PREDICATE
                | SEQUENCE_EXPLANATION_PREDICATE
                | CHAINED_SEQUENCE_EXPLANATION_PREDICATE
                | REVISED_SEQUENCE_EXPLANATION_PREDICATE
                | REWIRED_SEQUENCE_EXPLANATION_PREDICATE
                | APPENDED_SEQUENCE_EXPLANATION_PREDICATE
                | REMOVED_SEQUENCE_EXPLANATION_PREDICATE
                | MOVED_SEQUENCE_EXPLANATION_PREDICATE
                | INSERTED_SEQUENCE_EXPLANATION_PREDICATE
                | SEQUENCE_HISTORY_PREDICATE
                | SEQUENCE_STEP_EXPLANATION_PREDICATE
                | CONDITION_EVALUATION_PREDICATE
                | CONDITION_EVALUATION_DETAIL_PREDICATE
                | GUARDED_RELATION_RULE_PREDICATE
                | GUARDED_CONJUNCTIVE_RULE_PREDICATE
                | MULTI_CONDITION_RULE_PREDICATE
                | MULTI_CONDITION_PREMISE_PREDICATE
                | CORRECTION_ASSERTION_PREDICATE
                | WEB_ANSWER_PREDICATE
                | CODE_EXPLANATION_PREDICATE
                | RETURN_FUNCTION_PREDICATE
                | crate::code_generation::EXPLAINED_CODE_VERIFICATION_PREDICATE
                | PREVIOUS_ASSERTION_EXPLANATION_PREDICATE
                | PREVIOUS_CORRECTION_EXPLANATION_PREDICATE
                | crate::code_generation::VERIFICATION_EXPLANATION_PREDICATE
        ) || record_source_contract(&goal.intent)
            .is_some_and(|(_, required)| required == predicate.as_str())
    }) else {
        return Ok(());
    };
    let matches = observed.is_some_and(|report| {
        report.predicate == predicate
            && result.guarantee == report.guarantee
            && plan.guarantee == report.guarantee
            && result.dependencies == report.dependencies
            && plan.required_units == report.units
            && plan
                .dependencies
                .split_last()
                .is_some_and(|(last, prior)| last == compute_ref && prior == report.dependencies)
    });
    if !matches {
        return Err(failure(
            "GOAL_REPORT_MISMATCH",
            "元Goalが要求した報告成果と採用内容が一致しません。",
        ));
    }
    Ok(())
}

#[cfg(test)]
#[test]
fn rule_source_goal_rejects_an_adopted_plan_with_other_provenance() {
    let id = format!("rule:{}", "a".repeat(64));
    for label in ["規則の出典", "合成規則の出典"] {
        let request =
            RequestEnvelope::new("case", "rule-source", "actor", &format!("{label}: {id}"));
        let bundle = input::seal(&request, None).unwrap();
        let (_, predicate) = record_source_contract(&bundle.meaning.intent).unwrap();
        let goal = GoalContract {
            input_fingerprint: bundle.fingerprint.clone(),
            intent: bundle.meaning.intent.clone(),
            output_path: None,
            permissions: request.permissions.clone(),
            budget: request.budget.clone(),
            required_predicates: goal_predicates(&bundle.meaning, false, None),
            required_graph_ref: String::new(),
            renderer_version: None,
        };
        let report = ReportGoalContent {
            predicate,
            guarantee: Guarantee::Computed,
            dependencies: vec![id.clone(), "original-proof".into()],
            units: vec!["原文".into()],
        };
        let mut result = base_result(&request);
        result.guarantee = report.guarantee.clone();
        result.dependencies = report.dependencies.clone();
        let mut plan = ContentPlan {
            id: "plan".into(),
            text: report.units.join("\n"),
            locale: "ja".into(),
            guarantee: report.guarantee.clone(),
            required_units: report.units.clone(),
            dependencies: result.dependencies.clone(),
            model_revision: String::new(),
            model_artifact_ref: String::new(),
            model_observation_ref: String::new(),
        };
        plan.dependencies.push("compute".into());
        verify_report_goal_content(&goal, &result, &plan, "compute", Some(&report)).unwrap();
        plan.dependencies[1] = "different-proof".into();
        let error = verify_report_goal_content(&goal, &result, &plan, "compute", Some(&report))
            .unwrap_err();
        assert!(
            error.to_string().contains("GOAL_REPORT_MISMATCH"),
            "{error:?}"
        );
    }
}

pub(crate) fn exact_calculation_unit(calculation: &Calculation) -> String {
    format!("{} = {}", calculation.expression, calculation.exact_value)
}

pub(crate) fn calculation_from_context(
    store: &Store,
    bundle: &InputBundle,
    previous: &ResultRecord,
    prior_results: &BTreeMap<String, ResultRecord>,
    started: Instant,
) -> Result<Calculation> {
    enum Origin<'a> {
        Scalar(Calculation),
        Sequence(Box<InputBundle>, &'a ResultRecord),
    }

    let correcting = matches!(bundle.meaning.intent, Intent::CorrectCalculation { .. });
    let invalid = || {
        if correcting {
            failure(
                "CORRECTION_SOURCE_MISMATCH",
                "訂正対象を同じケースの封印済み計算と確定結果に結び付けられません。",
            )
        } else {
            failure(
                "LENGTH_CONVERSION_SOURCE_MISMATCH",
                "換算対象を同じ会話の封印済み計算と確定結果に結び付けられません。",
            )
        }
    };
    if !correcting && !matches!(bundle.meaning.intent, Intent::ConvertPreviousLength { .. }) {
        return Err(invalid());
    }
    let request = &bundle.request;
    let apply = |value: Calculation, intent: &Intent| match intent {
        Intent::ExplainPrevious => Ok(value),
        Intent::CorrectCalculation {
            operand_index,
            replacement,
        } => language::correct_binary_operand(&value, *operand_index, replacement),
        Intent::ConvertPreviousLength { unit } => language::convert_exact_length(&value, unit),
        _ => Err(invalid()),
    };
    let check_context = |input: &InputBundle, prior: &ResultRecord| -> Result<()> {
        if input.meaning.context_result_ref.as_deref() != Some(prior.id.as_str())
            || input.context_sha256 != identity(&Some(prior))?
            || input
                .request
                .context_ref
                .as_ref()
                .is_some_and(|id| id != &prior.id)
        {
            return Err(invalid());
        }
        Ok(())
    };
    let check_deadline = || {
        if started.elapsed().as_millis() >= u128::from(request.budget.deadline_ms) {
            return Err(failure(
                "DEADLINE",
                if correcting {
                    "訂正対象の確認中に締切へ達しました。"
                } else {
                    "換算対象の確認中に締切へ達しました。"
                },
            ));
        }
        Ok(())
    };
    check_context(bundle, previous)?;
    let insufficient = || {
        failure(
            "ACTION_BUDGET_EXHAUSTED",
            if correcting {
                "訂正に必要な計算と履歴を照合する行動予算が不足しています。"
            } else {
                "換算に必要な計算と履歴を照合する行動予算が不足しています。"
            },
        )
    };
    let context_actions = request
        .budget
        .max_actions
        .checked_sub(3 + u32::from(request.output_path.is_some()))
        .ok_or_else(insufficient)? as usize;
    let (origin, links) =
        store.with_verified_case_history(&request.actor_id, &request.case_id, |history| {
            let mut current = previous;
            let mut visited = BTreeSet::new();
            let mut links = Vec::new();
            let origin = loop {
                check_deadline()?;
                let (_, input) = history
                    .input_bundle(&current.input_id, current.input_version)?
                    .ok_or_else(invalid)?;
                if identity(&input.request)? != current.id
                    || current.lifecycle != Lifecycle::Commit
                    || current.guarantee != Guarantee::Computed
                    || !visited.insert(current.id.as_str())
                    || history.input_result(&current.input_id, current.input_version)?.as_ref()
                        != Some(current)
                {
                    return Err(invalid());
                }
                match &input.meaning.intent {
                    Intent::Calculate { expression } => {
                        let value = language::calculate(expression)?;
                        if current.calculation.as_ref() != Some(&value) {
                            return Err(invalid());
                        }
                        break Origin::Scalar(value);
                    }
                    Intent::CalculateSequence { steps } if !correcting => {
                        if steps.len() > context_actions.saturating_sub(links.len()) {
                            return Err(insufficient());
                        }
                        break Origin::Sequence(Box::new(input), current);
                    }
                    Intent::CalculateSequence { .. }
                    | Intent::CorrectCalculationSequenceStep { .. }
                    | Intent::AppendCalculationSequenceStep { .. }
                    | Intent::RemoveCalculationSequenceStep { .. }
                    | Intent::MoveCalculationSequenceStep { .. }
                    | Intent::InsertCalculationSequenceStep { .. }
                    | Intent::ShowCalculationSequenceHistory
                    | Intent::ExplainCalculationSequenceStep { .. } => {
                        if !correcting {
                            return Err(invalid());
                        }
                        return Err(failure(
                            "SEQUENCE_CORRECTION_REQUIRES_STEP",
                            "連続計算は項番号だけでは訂正できません。元の計算手順の対象段の式を変更して、計算手順を再入力してください。",
                        ));
                    }
                    Intent::ConvertPreviousLength { .. }
                        if input.meaning.parser_version == input::PREVIOUS_LENGTH_CONVERSION_PARSER_VERSION => {}
                    Intent::ExplainPrevious | Intent::CorrectCalculation { .. } => {}
                    _ => return Err(invalid()),
                }
                if links.len() >= context_actions {
                    return Err(insufficient());
                }
                let parent = input
                    .meaning
                    .context_result_ref
                    .as_ref()
                    .and_then(|id| prior_results.get(id).filter(|parent| &parent.id == id))
                    .ok_or_else(invalid)?;
                check_context(&input, parent)?;
                links.push((current, input.meaning.intent, &parent.id));
                current = parent;
            };
            Ok((origin, links))
        })?;
    let mut calculation = match origin {
        Origin::Scalar(value) => value,
        Origin::Sequence(input, original) => {
            let Intent::CalculateSequence { steps } = &input.meaning.intent else {
                return Err(invalid());
            };
            // Artifact reads use the Store after the verified history snapshot
            // closes. The original result and input were bound inside it.
            let mut graph = sequence_proof_graph(steps, &request.budget)?;
            let mut remaining_bytes = request.budget.max_evidence_bytes;
            recheck_calculation_sequence_origin(
                store,
                &input,
                original,
                &mut graph,
                &request.budget,
                started,
                &mut remaining_bytes,
            )?
            .calculation
        }
    };
    for (result, intent, parent_id) in links.into_iter().rev() {
        check_deadline()?;
        calculation = apply(calculation, &intent)?;
        if result.calculation.as_ref() != Some(&calculation)
            || result.dependencies.as_slice() != std::slice::from_ref(parent_id)
        {
            return Err(invalid());
        }
    }
    check_deadline()?;
    let result = apply(calculation, &bundle.meaning.intent)?;
    check_deadline()?;
    Ok(result)
}

pub(crate) fn verify_goal_content(
    goal: &GoalContract,
    result: &ResultRecord,
    plan: &ContentPlan,
) -> Result<()> {
    let Some(predicate) = goal.required_predicates.iter().find(|predicate| {
        matches!(
            predicate.as_str(),
            EXACT_CALCULATION_PREDICATE | GENERATED_CALCULATION_PREDICATE
        )
    }) else {
        return Ok(());
    };
    let Intent::Calculate { expression } = &goal.intent else {
        return Err(failure(
            "GOAL_CALCULATION_MISMATCH",
            "厳密計算の完了条件に封印済みの式がありません。",
        ));
    };
    let calculation = language::calculate(expression)?;
    let equation = exact_calculation_unit(&calculation);
    let matches_surface = if predicate == EXACT_CALCULATION_PREDICATE {
        plan.required_units.as_slice() == std::slice::from_ref(&equation) && plan.text == equation
    } else {
        plan.text
            .strip_prefix(&equation)
            .is_some_and(|tail| tail.is_empty() || tail.starts_with('。'))
    };
    if result.calculation.as_ref() != Some(&calculation)
        || result.guarantee != Guarantee::Computed
        || plan.guarantee != Guarantee::Computed
        || !matches_surface
    {
        return Err(failure(
            "GOAL_CALCULATION_MISMATCH",
            "式の厳密な計算結果と採用内容が、元の要求の完了条件を満たしません。",
        ));
    }
    Ok(())
}

#[derive(serde::Serialize)]
struct CalculationMeaningPlan {
    calculation_sha256: String,
    expression: String,
    rational: String,
    unit: Option<String>,
}

#[derive(serde::Serialize)]
struct GeneratedCalculationArtifact<'a> {
    kind: &'static str,
    meaning_plan: &'a CalculationMeaningPlan,
    content_generation: &'a language::MeaningGenerationReport,
}

impl CalculationMeaningPlan {
    fn from_calculation(calculation: &Calculation) -> Result<Self> {
        // The exact calculator emits a canonical rational or the same
        // rational followed by one supported unit.
        let (rational, unit) = calculation.exact_value.split_once(' ').map_or(
            (calculation.exact_value.as_str(), None),
            |(number, unit)| (number, Some(unit.to_owned())),
        );
        Ok(Self {
            calculation_sha256: identity(calculation)?,
            expression: calculation.expression.clone(),
            rational: rational.into(),
            unit,
        })
    }

    fn generation_request(&self, budget: usize) -> Result<language::MeaningGenerationRequest> {
        use language::{
            MeaningCompositionFormat, MeaningCompositionPurpose, MeaningCompositionStyle,
            MeaningGrammarFragment::{ExactField, Literal},
            MeaningGrammarUnit, MeaningRole,
        };

        let max_output_bytes = budget.min(language::MAX_EXACT_FIELD_GENERATION_BYTES);
        let mut fragments = vec![
            ExactField {
                value: self.expression.clone(),
            },
            Literal {
                value: " = ".into(),
            },
            ExactField {
                value: self.rational.clone(),
            },
        ];
        let mut equation_bytes = self.expression.len() + 3 + self.rational.len();
        if let Some(unit) = &self.unit {
            equation_bytes += 1 + unit.len();
            fragments.push(Literal { value: " ".into() });
            fragments.push(ExactField {
                value: unit.clone(),
            });
        }
        let sign = if self.rational == "0" {
            ""
        } else if self.rational.starts_with('-') {
            "負の"
        } else {
            "正の"
        };
        let classification = match (
            self.unit.is_some(),
            self.rational.contains('/'),
            self.rational == "0",
        ) {
            (false, _, true) => "ゼロです。".into(),
            (true, _, true) => "ゼロの数量です。".into(),
            (false, false, false) => format!("{sign}整数です。"),
            (false, true, false) => format!("{sign}分数です。"),
            (true, false, false) => format!("{sign}整数で表される数量です。"),
            (true, true, false) => format!("{sign}分数で表される数量です。"),
        };
        let intros = ["この値は", "結果は"];
        let max_intro_bytes = intros.iter().map(|intro| intro.len()).max().unwrap_or(0);
        // If the output budget is just enough for the exact equation, the
        // decoder still emits its typed fields and can end directly at EOS.
        if equation_bytes + "。".len() + max_intro_bytes + classification.len() <= max_output_bytes
        {
            fragments.extend([
                Literal {
                    value: "。".into()
                },
                grammar_choice("classification_intro", &intros),
                Literal {
                    value: classification,
                },
            ]);
        }
        Ok(language::MeaningGenerationRequest {
            purpose: MeaningCompositionPurpose::ShortComposition,
            format: MeaningCompositionFormat::Paragraph,
            style: MeaningCompositionStyle::Neutral,
            units: vec![MeaningGrammarUnit {
                id: "exact_equation".into(),
                role: MeaningRole::Body,
                semantic_sha256: identity(&("exact-calculation-meaning-v1", self))?,
                evidence_ids: Vec::new(),
                dependencies: Vec::new(),
                fragments,
            }],
            max_output_bytes,
            max_tokens: language::MAX_EXACT_FIELD_GENERATION_TOKENS,
        })
    }

    fn validate_generation(
        request: &language::MeaningGenerationRequest,
        report: &language::MeaningGenerationReport,
    ) -> Result<()> {
        validate_meaning_generation(request, &[], report)?;
        let mut steps = report.token_steps.iter().peekable();
        let unit = &request.units[0];
        for (index, fragment) in unit.fragments.iter().enumerate() {
            let mut tokens = Vec::new();
            while steps
                .peek()
                .is_some_and(|step| step.fragment_index == index && step.unit_id == unit.id)
            {
                tokens.push(steps.next().expect("peeked step"));
            }
            let text = tokens
                .iter()
                .map(|step| step.token.as_str())
                .collect::<String>();
            let matches = match fragment {
                language::MeaningGrammarFragment::ExactField { value } => {
                    text == *value
                        && tokens.len() == 1
                        && tokens[0].admissible_tokens.as_slice() == std::slice::from_ref(value)
                        && tokens[0].selected_probability == "1"
                        && tokens[0].distribution.as_slice()
                            == [language::MeaningGenerationDistributionEntry {
                                token: value.clone(),
                                probability: "1".into(),
                            }]
                }
                language::MeaningGrammarFragment::Literal { value } => text == *value,
                language::MeaningGrammarFragment::Choice { alternatives, .. } => {
                    alternatives.contains(&text)
                }
            };
            if !matches {
                return Err(failure(
                    "GOAL_CALCULATION_MISMATCH",
                    "the generated equation or classification does not retain its exact semantic fields",
                ));
            }
        }
        let emitted = report.token_steps[..report.token_steps.len() - 1]
            .iter()
            .map(|step| step.token.as_str())
            .collect::<String>();
        if steps.count() != 1 || emitted != report.text {
            return Err(failure(
                "GOAL_CALCULATION_MISMATCH",
                "the verified arithmetic tokens do not reproduce the adopted output",
            ));
        }
        Ok(())
    }
}

pub(crate) fn verify_generated_calculation_computation(
    store: &Store,
    original: &RequestEnvelope,
    goal: &GoalContract,
    result: &ResultRecord,
    plan: &ContentPlan,
) -> Result<String> {
    // The caller has already compared the full Calculation against the
    // sealed expression in verify_goal_content.
    let calculation = result
        .calculation
        .as_ref()
        .filter(|_| result.dependencies.is_empty() && result.evidence.is_empty())
        .ok_or_else(|| {
            failure(
                "GOAL_CALCULATION_MISMATCH",
                "the retained calculation or its provenance is invalid",
            )
        })?;
    let meaning_plan = CalculationMeaningPlan::from_calculation(calculation)?;
    let request = meaning_plan.generation_request(goal.budget.max_evidence_bytes)?;
    let mut replay = language::generate_semantic_meaning(&request)?;
    CalculationMeaningPlan::validate_generation(&request, &replay.report)?;
    replay.report.model_artifact_ref = Some(digest(&canonical(&replay.model)?));
    let artifact = GeneratedCalculationArtifact {
        kind: GENERATED_CALCULATION_KIND,
        meaning_plan: &meaning_plan,
        content_generation: &replay.report,
    };
    let compute_ref = digest(&canonical(&artifact)?);
    validate_generated_content_plan_adoption(
        result,
        &replay.report,
        &replay.emission,
        &[],
        &compute_ref,
        &original.locale,
        plan,
    )?;
    if result.text != replay.report.text {
        return Err(failure(
            "CONTENT_ADOPTION_MISMATCH",
            "the saved result differs from the replayed arithmetic sequence",
        ));
    }
    store.get_bytes(&original.actor_id, &original.case_id, &compute_ref)?;
    verify_generated_model_artifacts(store, &original.actor_id, &original.case_id, plan)?;
    Ok(compute_ref)
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct DirectAtomicMeaningInput {
    schema: String,
    purpose: String,
    proposition: Proposition,
    source_ids: Vec<String>,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct DirectTypedRelationAssertionInput {
    schema: String,
    proposition: Proposition,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct DirectTypedRelationCorrectionInput {
    schema: String,
    superseded: Proposition,
    replacement: Proposition,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct DirectTypedRelationRuleInput {
    schema: String,
    premises: Vec<Proposition>,
    conclusion: Proposition,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct DirectTypedConditionInput {
    schema: String,
    definition: ConditionDefinition,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct DirectRelationComparisonInput {
    schema: String,
    purpose: String,
    subject_a: String,
    subject_b: String,
    scope: String,
    source_ids_a: Vec<String>,
    source_ids_b: Vec<String>,
}

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

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct CoreComputationArtifactContent {
    kind: String,
    calculation: Option<Calculation>,
    units: Vec<String>,
    evidence: Vec<EvidencePacket>,
    dependencies: Vec<String>,
    relation_meaning: Proposition,
    relation_proof_dependencies: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    relation_meaning_plan: Option<CoreRelationMeaningPlan>,
    content_request: language::MeaningCompositionRequest,
    content_composition: language::MeaningCompositionReport,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct CoreComputationArtifactGeneratedContent {
    kind: String,
    calculation: Option<Calculation>,
    units: Vec<String>,
    evidence: Vec<EvidencePacket>,
    dependencies: Vec<String>,
    relation_meaning: Proposition,
    relation_proof_dependencies: Vec<String>,
    relation_meaning_plan: CoreRelationMeaningPlan,
    content_generation: language::MeaningGenerationReport,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    adaptation: Option<RuleGrammarAdaptation>,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(untagged, deny_unknown_fields)]
enum RuleGrammarAdaptation {
    PriorResult {
        source_result_id: String,
        source_compute_ref: String,
        rule_ids: Vec<String>,
    },
    ActorRules {
        rule_ids: Vec<String>,
    },
}

fn generated_artifact_kind(adaptation: Option<&RuleGrammarAdaptation>) -> &'static str {
    match adaptation {
        None => CORE_COMPUTATION_ARTIFACT_KIND_V5,
        Some(RuleGrammarAdaptation::PriorResult { .. }) => CORE_COMPUTATION_ARTIFACT_KIND_V6,
        Some(RuleGrammarAdaptation::ActorRules { .. }) => CORE_COMPUTATION_ARTIFACT_KIND_V7,
    }
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct CoreRelationMeaningPlan {
    schema: String,
    id: String,
    purpose: language::MeaningCompositionPurpose,
    format: language::MeaningCompositionFormat,
    style: language::MeaningCompositionStyle,
    asked: Proposition,
    conclusion: Proposition,
    answer_status: String,
    guarantee: Guarantee,
    condition_evaluations: Vec<ConditionEvaluation>,
    units: Vec<CoreSemanticMeaningUnit>,
    required_units: Vec<String>,
    dependencies: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct CoreSemanticMeaningUnit {
    id: String,
    role: language::MeaningRole,
    meaning: CoreSemanticMeaning,
    evidence_ids: Vec<String>,
    dependencies: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "kind", rename_all = "SCREAMING_SNAKE_CASE", deny_unknown_fields)]
enum CoreSemanticMeaning {
    Claim {
        asked: Proposition,
        conclusion: Proposition,
        answer_status: String,
        guarantee: Guarantee,
    },
    Condition {
        evaluation: ConditionEvaluation,
    },
    Fact {
        proposition: Proposition,
        guarantee: Guarantee,
        record_id: String,
        record_dependencies: Vec<String>,
    },
    Rule {
        premises: Vec<Proposition>,
        conclusion: Proposition,
        record_id: String,
        record_dependencies: Vec<String>,
        composed: bool,
    },
    ProofReference {
        record_id: String,
    },
    Limitation {
        scope: String,
    },
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
const RELATION_COMPARISON_CONTENT_KIND: &str = "minidora.relation-comparison-content.v2";
const RELATION_COMPARISON_CONTENT_KIND_V3: &str = "minidora.relation-comparison-content.v3";
const RELATION_COMPARISON_CONTENT_KIND_V4: &str = "minidora.relation-comparison-content.v4";
const RELATION_COMPARISON_CONTENT_KIND_V5: &str = "minidora.relation-comparison-content.v5";
const RELATION_COMPARISON_CONTENT_KIND_V6: &str = "minidora.relation-comparison-content.v6";
const RELATION_COMPARISON_GENERATED_HEADING: &str = "保存済みの関係を左右で比較します。";
const RELATION_COMPARISON_REPORT_HEADER: &str = "保存済みの根拠を比較した記録です。片側のみの表示はその側の保存記録で支持され、他方では同じ関係の支持を確認できなかった意味です。保存記録の網羅性と現実世界の真偽は確認していません。";
const RELATION_COMPARISON_CONTENT_HEADER: &str = "左右の保存関係根拠を照合した比較です。";

#[derive(Clone, Debug)]
pub(crate) struct RelationObjectsAnswer {
    pub(crate) text: String,
    pub(crate) dependencies: Vec<String>,
    pub(crate) guarantee: Guarantee,
    pub(crate) proofs: Vec<RelationObjectsProof>,
}

#[derive(Clone, Debug)]
pub(crate) struct RelationObjectsProof {
    pub(crate) proposition: Proposition,
    pub(crate) outcome: relations::QueryOutcome,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct RelationObjectsExplanationAnswer {
    pub(crate) units: Vec<String>,
    pub(crate) dependencies: Vec<String>,
    pub(crate) guarantee: Guarantee,
}

pub(crate) enum RelationObjectsExplanationEvaluation {
    Complete(RelationObjectsExplanationAnswer),
    Blocked { code: &'static str, message: String },
}

#[derive(Clone, Copy)]
pub(crate) struct RelationObjectsReplay<'a> {
    pub(crate) subject: &'a str,
    pub(crate) predicate: &'a str,
    pub(crate) negative: bool,
    pub(crate) scope: &'a str,
    pub(crate) max_actions: u32,
    pub(crate) max_evidence_bytes: usize,
}

struct RelationClauseEvaluation {
    answer: relations::QueryOutcome,
    out_of_scope_facts: Vec<Fact>,
}

struct ConjunctionRelationQueries {
    memory_revision: u64,
    outcomes: Result<Vec<RelationClauseEvaluation>>,
}

fn indexed_relation_evaluation(
    index: &crate::store::MemoryIndexSnapshot<'_, '_>,
    proposition: &Proposition,
    max_actions: u32,
) -> Result<(MemoryState, RelationClauseEvaluation)> {
    let (mut memory, mut answer) =
        relations::query_active_relation(index, proposition, max_actions)?;
    let mut actions = answer.steps;
    let mut out_of_scope_facts = Vec::new();
    if answer.status == "UNKNOWN"
        && answer
            .condition_status
            .is_none_or(|status| status == ConditionStatus::True)
    {
        let classification_budget = max_actions.checked_sub(actions).ok_or_else(|| {
            failure(
                "RELATION_BUDGET",
                "proof query exhausted the out-of-scope classification budget",
            )
        })?;
        let (facts, classification_actions) =
            relations::active_out_of_scope_facts(index, proposition, classification_budget)?;
        memory.facts.extend(facts.iter().cloned());
        out_of_scope_facts = facts;
        actions = actions
            .checked_add(classification_actions)
            .ok_or_else(|| failure("RELATION_BUDGET", "relation action count overflowed"))?;
    }
    answer.steps = actions;
    Ok((
        memory,
        RelationClauseEvaluation {
            answer,
            out_of_scope_facts,
        },
    ))
}

/// Re-evaluate one relation query from a revision-matched Store projection.
/// `None` means the index is cold or stale and the caller should use its
/// authenticated snapshot path.
pub(crate) fn indexed_relation_query_from_store(
    store: &Store,
    actor: &str,
    proposition: &Proposition,
    max_actions: u32,
) -> Result<Option<(MemoryState, Result<relations::QueryOutcome>)>> {
    store.with_relation_index(actor, |index| {
        let (memory, answer) = match indexed_relation_evaluation(index, proposition, max_actions) {
            Ok((memory, evaluation)) => (memory, Ok(evaluation.answer)),
            Err(error)
                if matches!(
                    &error,
                    Error::Contract { code, .. } if code == "RELATION_BUDGET"
                ) =>
            {
                (
                    MemoryState {
                        revision: index.revision(),
                        ..MemoryState::default()
                    },
                    Err(error),
                )
            }
            Err(error) => return Err(error),
        };
        Ok((memory, answer))
    })
}

/// Re-evaluate every clause of a conjunction from one revision-matched Store
/// index transaction. `None` means the index is cold or stale and the caller
/// should use its authenticated snapshot path.
pub(crate) fn indexed_conjunction_relation_answers_from_store(
    store: &Store,
    actor: &str,
    propositions: &[Proposition],
    per_relation_budget: u32,
) -> Result<Option<(u64, Result<Vec<relations::QueryOutcome>>)>> {
    let Some(queries) = indexed_conjunction_relation_queries_from_store(
        store,
        actor,
        propositions,
        per_relation_budget,
    )?
    else {
        return Ok(None);
    };
    let outcomes = queries.outcomes.map(|evaluations| {
        evaluations
            .into_iter()
            .map(|evaluation| evaluation.answer)
            .collect()
    });
    Ok(Some((queries.memory_revision, outcomes)))
}

fn indexed_conjunction_relation_queries_from_store(
    store: &Store,
    actor: &str,
    propositions: &[Proposition],
    per_relation_budget: u32,
) -> Result<Option<ConjunctionRelationQueries>> {
    store.with_relation_index(actor, |index| {
        let outcomes = propositions
            .iter()
            .map(|proposition| {
                indexed_relation_evaluation(index, proposition, per_relation_budget)
                    .map(|(_, evaluation)| evaluation)
            })
            .collect();
        Ok(ConjunctionRelationQueries {
            memory_revision: index.revision(),
            outcomes,
        })
    })
}

fn full_snapshot_relation_evaluation(
    memory: &MemoryState,
    proposition: &Proposition,
    max_actions: u32,
) -> Result<RelationClauseEvaluation> {
    let mut answer = relations::query(memory, proposition, max_actions)?;
    let mut actions = answer.steps;
    let out_of_scope_facts = if answer.status == "UNKNOWN"
        && answer
            .condition_status
            .is_none_or(|status| status == ConditionStatus::True)
    {
        let classification_budget = max_actions.checked_sub(actions).ok_or_else(|| {
            failure(
                "RELATION_BUDGET",
                "proof query exhausted the out-of-scope classification budget",
            )
        })?;
        let facts =
            relations::direct_out_of_scope_premises(memory, proposition, classification_budget)?;
        let scanned_facts = u32::try_from(memory.facts.len()).unwrap_or(u32::MAX);
        actions = actions
            .checked_add(scanned_facts)
            .ok_or_else(|| failure("RELATION_BUDGET", "relation action count overflowed"))?;
        facts
    } else {
        Vec::new()
    };
    answer.steps = actions;
    Ok(RelationClauseEvaluation {
        answer,
        out_of_scope_facts,
    })
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
            temporal_scope: None,
            condition_refs: Default::default(),
        };
        let answer = relations::query(memory, &proposition, per_candidate_budget)?;
        match answer.status.as_str() {
            "SUPPORTED" => {
                let guarantee = relations::answer_guarantee(&answer);
                dependencies.extend(relations::answer_dependencies(&answer));
                any_derived |= guarantee == Guarantee::Derived;
                supported.insert(proposition, answer);
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
        .map(|(proposition, answer)| {
            let guarantee = relations::answer_guarantee(answer);
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
        proofs: supported
            .into_iter()
            .map(|(proposition, outcome)| RelationObjectsProof {
                proposition,
                outcome,
            })
            .collect(),
    })
}

/// Evaluate a relation-object answer from one revision-matched Store index.
/// `None` means the index is cold or stale and lets the caller retain the
/// authenticated full-snapshot compatibility path.
pub(crate) fn indexed_relation_objects_answer_from_store(
    store: &Store,
    actor: &str,
    replay: RelationObjectsReplay<'_>,
) -> Result<Option<(MemoryState, Result<RelationObjectsAnswer>)>> {
    let candidate_shape = Proposition {
        subject: replay.subject.into(),
        predicate: replay.predicate.into(),
        object: "$relation_object".into(),
        negative: replay.negative,
        scope: replay.scope.into(),
        condition_refs: BTreeSet::new(),
        temporal_scope: None,
    };
    store.with_relation_index(actor, |index| {
        let (memory, answer) = match relations::active_relation_projection(
            index,
            &candidate_shape,
            replay.max_actions,
        ) {
            Ok((memory, projection_actions)) => {
                let answer = replay
                    .max_actions
                    .checked_sub(projection_actions)
                    .ok_or_else(|| {
                        failure(
                            "RELATION_BUDGET",
                            "active memory projection exhausted the relation-object action budget",
                        )
                    })
                    .and_then(|answer_budget| {
                        relation_objects_answer(
                            &memory,
                            replay.subject,
                            replay.predicate,
                            replay.negative,
                            replay.scope,
                            answer_budget,
                            replay.max_evidence_bytes,
                        )
                    });
                (memory, answer)
            }
            Err(error)
                if matches!(
                    &error,
                    Error::Contract { code, .. } if code == "RELATION_BUDGET"
                ) =>
            {
                (
                    MemoryState {
                        revision: index.revision(),
                        ..MemoryState::default()
                    },
                    Err(error),
                )
            }
            Err(error) => return Err(error),
        };
        Ok((memory, answer))
    })
}

/// Rebuild one committed relation-object list and explain each listed value
/// from the same actor-memory snapshot. Core and Recovery use this exact
/// consumer for proof lineage, dependency order, and evidence limits.
pub(crate) fn relation_objects_explanation(
    memory: &MemoryState,
    previous: &ResultRecord,
    replay: RelationObjectsReplay<'_>,
) -> Result<RelationObjectsExplanationEvaluation> {
    let answer = relation_objects_answer(
        memory,
        replay.subject,
        replay.predicate,
        replay.negative,
        replay.scope,
        replay.max_actions,
        replay.max_evidence_bytes,
    );
    relation_objects_explanation_with_answer(memory, previous, replay, answer)
}

/// Rebuild a saved relation-object explanation from a revision-matched Store
/// index. `None` means the index is cold or stale and the caller should use its
/// authenticated snapshot path.
pub(crate) fn indexed_relation_objects_explanation_from_store(
    store: &Store,
    actor: &str,
    previous: &ResultRecord,
    replay: RelationObjectsReplay<'_>,
) -> Result<Option<(u64, Result<RelationObjectsExplanationEvaluation>)>> {
    let Some((memory, answer)) = indexed_relation_objects_answer_from_store(store, actor, replay)?
    else {
        return Ok(None);
    };
    let revision = memory.revision;
    let explanation = relation_objects_explanation_with_answer(&memory, previous, replay, answer);
    Ok(Some((revision, explanation)))
}

fn relation_objects_explanation_with_answer(
    memory: &MemoryState,
    previous: &ResultRecord,
    replay: RelationObjectsReplay<'_>,
    answer: Result<RelationObjectsAnswer>,
) -> Result<RelationObjectsExplanationEvaluation> {
    let answer = match answer {
        Ok(answer) => answer,
        Err(error) => {
            let code = match &error {
                Error::Contract { code, .. } => match code.as_str() {
                    "RESOURCE_LIMIT" => "RESOURCE_LIMIT",
                    "CONFLICTING_PREMISES" => "CONFLICTING_PREMISES",
                    "MISSING_PREMISES" => "MISSING_PREMISES",
                    _ => "RELATION_QUERY_UNAVAILABLE",
                },
                _ => "RELATION_QUERY_UNAVAILABLE",
            };
            return Ok(RelationObjectsExplanationEvaluation::Blocked {
                // The result cannot be rebuilt under the sealed request's
                // budget or current memory, so preserve the original typed
                // stop when Core adopts this explanation.
                code,
                message: error.to_string(),
            });
        }
    };
    let Some(prior_plan) = previous.content_plan.as_ref() else {
        return Ok(RelationObjectsExplanationEvaluation::Blocked {
            code: "MISSING_RELATION_OBJECT_CONTEXT",
            message: "説明対象の前回関係一覧に採用済み出力計画がありません。".into(),
        });
    };
    if previous.lifecycle != Lifecycle::Commit
        || previous.calculation.is_some()
        || previous.text != answer.text
        || previous.dependencies != answer.dependencies
        || previous.guarantee != answer.guarantee
        || prior_plan.text != previous.text
        || prior_plan.guarantee != previous.guarantee
        || prior_plan.required_units != vec![previous.text.clone()]
    {
        return Ok(RelationObjectsExplanationEvaluation::Blocked {
            code: "CONTEXT_PROOF_CHANGED",
            message: "前回の関係一覧と現在有効な記憶の内容または根拠が変わっています。新しい問いで確認してください。".into(),
        });
    }
    let mut units = vec![format!(
        "前回の関係一覧を詳しく説明します（結果ID: {}）。各対象の直接前提と適用規則を、同じscopeの現在有効な記憶で再確認しました。",
        previous.id
    )];
    for proof in &answer.proofs {
        units.extend(relation_explanation_lines(
            memory,
            &proof.proposition,
            &proof.outcome,
            "前回回答に含まれる関係の根拠",
        ));
    }
    units.push(
        "一覧は保存済みの根拠にある対象のみを示し、対象全体の網羅性や現実世界の真偽を確認したものではありません。".into(),
    );
    if units.join("\n").len() > replay.max_evidence_bytes {
        return Ok(RelationObjectsExplanationEvaluation::Blocked {
            code: "RESOURCE_LIMIT",
            message: "再検証した関係一覧の説明が今回の証拠予算を超えています。".into(),
        });
    }
    let mut dependencies = vec![previous.id.clone()];
    dependencies.extend(previous.dependencies.iter().cloned());
    Ok(RelationObjectsExplanationEvaluation::Complete(
        RelationObjectsExplanationAnswer {
            units,
            dependencies,
            guarantee: previous.guarantee.clone(),
        },
    ))
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct RelationProfileAnswer {
    pub(crate) units: Vec<String>,
    pub(crate) dependencies: Vec<String>,
    pub(crate) guarantee: Guarantee,
}

struct RelationComparisonInventories {
    revision: u64,
    inventory_a: Result<relations::RelationInventory>,
    inventory_b: Result<relations::RelationInventory>,
    comparison_rules: Vec<RelationRule>,
}

pub(crate) fn relation_profile_answer(
    memory: &MemoryState,
    subject: &str,
    max_actions: u32,
    max_evidence_bytes: usize,
) -> Result<RelationProfileAnswer> {
    let inventory = relations::inventory_about(memory, subject, max_actions)?;
    relation_profile_answer_from_inventory(subject, inventory, max_evidence_bytes)
}

fn relation_profile_answer_from_index(
    index: &MemoryIndexSnapshot<'_, '_>,
    subject: &str,
    max_actions: u32,
    max_evidence_bytes: usize,
) -> Result<RelationProfileAnswer> {
    let (inventory, _) =
        relations::active_relation_inventory_projection(index, subject, max_actions)?;
    relation_profile_answer_from_inventory(subject, inventory, max_evidence_bytes)
}

/// Re-evaluate a relation profile from a revision-matched Store index.
/// `None` means the index is cold or stale and the caller should use its
/// authenticated snapshot path.
pub(crate) fn indexed_relation_profile_answer_from_store(
    store: &Store,
    actor: &str,
    subject: &str,
    max_actions: u32,
    max_evidence_bytes: usize,
) -> Result<Option<(u64, Result<RelationProfileAnswer>)>> {
    store.with_relation_index(actor, |index| {
        let revision = index.revision();
        let answer =
            relation_profile_answer_from_index(index, subject, max_actions, max_evidence_bytes);
        Ok((revision, answer))
    })
}

/// Re-evaluate a premise inventory from a revision-matched Store index.
/// `None` means the index is cold or stale and the caller should use its
/// authenticated snapshot path.
pub(crate) fn indexed_premise_inventory_from_store(
    store: &Store,
    actor: &str,
    subject: &str,
    max_actions: u32,
) -> Result<Option<(u64, Result<relations::RelationInventory>)>> {
    store.with_relation_index(actor, |index| {
        let revision = index.revision();
        let inventory =
            relations::active_relation_inventory_projection(index, subject, max_actions)
                .map(|(inventory, _actions)| inventory);
        Ok((revision, inventory))
    })
}

/// Render the exact units used by the saved premise inventory result.
/// Recovery uses this same projection when checking that a prior result still
/// matches its retained actor-memory revision.
pub(crate) fn premise_inventory_units<'a>(
    subject: &'a str,
    inventory: &'a relations::RelationInventory,
) -> impl Iterator<Item = String> + 'a {
    let disclosure = format!(
        "会話に保存された「{subject}」に関する有効な関係です。保存記録の範囲に限った列挙で、現実世界の真偽や全件網羅性を確認したものではありません。"
    );
    let conflict = std::iter::once_with(move || {
        inventory.has_conflict.then(|| {
            "一部の関係には肯定と否定の根拠が併存するため、どちらも確定せず根拠を併記します。"
                .to_string()
        })
    })
    .flatten();
    let fact_lines = std::iter::once_with(move || {
        let mut facts: Vec<_> = inventory.facts.iter().collect();
        facts.sort_by(|left, right| {
            left.proposition
                .cmp(&right.proposition)
                .then_with(|| left.id.cmp(&right.id))
        });
        facts
    })
    .flatten()
    .map(move |fact| {
        let (category, origin) = match &fact.guarantee {
            Guarantee::UserPremise => ("提示前提", format!("提示前提ID: {}", fact.id)),
            Guarantee::Derived => (
                "規則導出",
                format!(
                    "導出ID: {}; 依存ID: {}",
                    fact.id,
                    fact.dependencies.join(", ")
                ),
            ),
            Guarantee::Computed => ("計算記録", format!("計算記録ID: {}", fact.id)),
            Guarantee::SourceReport => ("出典報告", format!("出典報告ID: {}", fact.id)),
            Guarantee::Hypothesis | Guarantee::Unknown => {
                ("未確定", format!("未確定記録ID: {}", fact.id))
            }
        };
        let conflict = if inventory.contested_fact_ids.contains(&fact.id) {
            "対立根拠として"
        } else {
            ""
        };
        format!(
            "・{conflict}{category}: {}（範囲: {}; {origin}）",
            describe(&fact.proposition),
            fact.proposition.scope
        )
    });
    std::iter::once(disclosure)
        .chain(conflict)
        .chain(fact_lines)
}

fn relation_profile_answer_from_inventory(
    subject: &str,
    inventory: relations::RelationInventory,
    max_evidence_bytes: usize,
) -> Result<RelationProfileAnswer> {
    let units = relation_profile_units(subject, &inventory);
    let profile_bytes = units.iter().try_fold(0usize, |total, unit| {
        total
            .checked_add(unit.len())
            .and_then(|size| size.checked_add(1))
    });
    if profile_bytes.is_none_or(|size| size > max_evidence_bytes) {
        return Err(failure(
            "RESOURCE_LIMIT",
            "根拠付き紹介文案が要求の証拠予算を超えています。",
        ));
    }
    Ok(RelationProfileAnswer {
        units,
        dependencies: inventory.dependencies,
        guarantee: Guarantee::Hypothesis,
    })
}

pub(crate) fn previous_relation_profile_request(
    store: &Store,
    actor: &str,
    case: &str,
    previous: &ResultRecord,
) -> Result<Option<(String, u32, usize)>> {
    let prior = store.get_input_bundle(actor, case, &previous.input_id, previous.input_version)?;
    previous_relation_profile_from_input(actor, case, previous, prior.as_ref().map(|(_, b)| b))
}

fn previous_relation_profile_from_input(
    actor: &str,
    case: &str,
    previous: &ResultRecord,
    prior_bundle: Option<&InputBundle>,
) -> Result<Option<(String, u32, usize)>> {
    let Some(prior_bundle) = prior_bundle else {
        return Ok(None);
    };
    let Intent::DraftRelationProfile { subject } = &prior_bundle.meaning.intent else {
        return Ok(None);
    };
    if prior_bundle.request.actor_id != actor
        || prior_bundle.request.case_id != case
        || identity(&prior_bundle.request)? != previous.id
    {
        return Err(failure(
            "RELATION_PROFILE_CONTEXT_INVALID",
            "the saved profile input and committed result identities differ",
        ));
    }
    Ok(Some((
        subject.clone(),
        prior_bundle.request.budget.max_actions,
        prior_bundle.request.budget.max_evidence_bytes,
    )))
}

pub(crate) fn relation_profile_result_from_memory(
    store: &Store,
    actor: &str,
    case: &str,
    previous: &ResultRecord,
    memory: &MemoryState,
) -> Result<(String, RelationProfileAnswer)> {
    let Some((subject, max_actions, max_evidence_bytes)) =
        previous_relation_profile_request(store, actor, case, previous)?
    else {
        return Err(failure(
            "RECOVERY_MEMORY_PROOF_MISMATCH",
            "the exact prior result is not a typed relation profile",
        ));
    };
    if previous.lifecycle != Lifecycle::Commit
        || previous.calculation.is_some()
        || previous.actor_memory_revision.is_none()
        || previous.guarantee != Guarantee::Hypothesis
    {
        return Err(failure(
            "RECOVERY_MEMORY_PROOF_MISMATCH",
            "the prior relation profile is not a pinned committed hypothesis",
        ));
    }
    let answer = relation_profile_answer(memory, &subject, max_actions, max_evidence_bytes)?;
    let plan = previous.content_plan.as_ref().ok_or_else(|| {
        failure(
            "RECOVERY_MEMORY_PROOF_MISMATCH",
            "the prior relation profile has no retained content plan",
        )
    })?;
    let render = previous.render.as_ref().ok_or_else(|| {
        failure(
            "RECOVERY_MEMORY_PROOF_MISMATCH",
            "the prior relation profile has no retained render record",
        )
    })?;
    let mut unsigned_plan = plan.clone();
    unsigned_plan.id.clear();
    if previous.id.is_empty()
        || plan.id != identity(&unsigned_plan)?
        || plan.text != previous.text
        || plan.guarantee != previous.guarantee
        || plan.required_units != answer.units
        || plan.dependencies.get(..previous.dependencies.len())
            != Some(previous.dependencies.as_slice())
        || answer.dependencies != previous.dependencies
        || answer.guarantee != previous.guarantee
        || !validate_render(plan, render)
    {
        return Err(failure(
            "RECOVERY_MEMORY_PROOF_MISMATCH",
            "the saved relation profile differs from its actor-memory revision",
        ));
    }
    let plan_bytes = canonical(plan)?;
    let render_bytes = canonical(render)?;
    for (reference, expected) in [
        (digest(&plan_bytes), plan_bytes),
        (digest(&render_bytes), render_bytes),
    ] {
        match store.get_bytes_if_present(actor, case, &reference)? {
            Some(stored) if stored == expected => {}
            Some(_) => {
                return Err(failure(
                    "RECOVERY_MEMORY_PROOF_MISMATCH",
                    "the saved relation profile plan or render differs from its artifact",
                ));
            }
            None => {
                return Err(failure(
                    "RECOVERY_MEMORY_PROOF_MISMATCH",
                    "the saved relation profile plan or render artifact is missing",
                ));
            }
        }
    }
    Ok((subject, answer))
}

pub(crate) fn relation_profile_explanation_units(
    previous_result_id: &str,
    previous_memory_revision: u64,
    answer: &RelationProfileAnswer,
) -> Vec<String> {
    let mut units = vec![format!(
        "前回の保存済み紹介文案を説明します（結果ID: {previous_result_id}）。"
    )];
    units.push(format!(
        "採用時のactor-memory revision {previous_memory_revision}から再計算し、本文・依存ID・保証を照合しました。"
    ));
    units.push("前回の紹介文案と根拠付き記述:".into());
    units.extend(answer.units.iter().cloned());
    units.push(format!(
        "今回再確認した根拠依存ID: {}",
        if answer.dependencies.is_empty() {
            "なし".to_string()
        } else {
            answer.dependencies.join(", ")
        }
    ));
    units.push(
        "この草稿は同じ利用者の有効な保存記録に基づきます。現実世界の真偽や記録の網羅性は確認していません。".into(),
    );
    units
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

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct CommonRelationsAnswer {
    pub(crate) report: String,
    pub(crate) artifact_bytes: Vec<u8>,
    pub(crate) proof_dependencies: Vec<String>,
    pub(crate) guarantee: Guarantee,
}

pub(crate) enum CommonRelationsEvaluation {
    Complete(CommonRelationsAnswer),
    Blocked { code: &'static str, message: String },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct CommonRelationsExplanationAnswer {
    pub(crate) units: Vec<String>,
    pub(crate) dependencies: Vec<String>,
    pub(crate) guarantee: Guarantee,
}

pub(crate) enum CommonRelationsExplanationEvaluation {
    Complete(CommonRelationsExplanationAnswer),
    Blocked { code: &'static str, message: String },
}

#[derive(Clone, Copy)]
pub(crate) struct CommonRelationsExplanationReplay<'a> {
    pub(crate) subject_a: &'a str,
    pub(crate) subject_b: &'a str,
    pub(crate) scope: &'a str,
    pub(crate) max_actions: u32,
    pub(crate) max_evidence_bytes: usize,
}

#[derive(Clone, Copy)]
pub(crate) struct CommonRelationsReplay<'a> {
    pub(crate) subject_a: &'a str,
    pub(crate) subject_b: &'a str,
    pub(crate) scope: &'a str,
    pub(crate) max_actions: u32,
    pub(crate) max_evidence_bytes: usize,
    pub(crate) result_id: &'a str,
}

struct SupportedCommonRelation {
    proposition_a: Proposition,
    proposition_b: Proposition,
    guarantee_a: Guarantee,
    guarantee_b: Guarantee,
    dependencies_a: Vec<String>,
    dependencies_b: Vec<String>,
    explanation_lines_a: Vec<String>,
    explanation_lines_b: Vec<String>,
}

fn common_relations_answer_from_supported(
    subject_a: &str,
    subject_b: &str,
    scope: &str,
    result_id: &str,
    max_evidence_bytes: usize,
    supported: Vec<SupportedCommonRelation>,
) -> Result<CommonRelationsEvaluation> {
    if supported.is_empty() {
        return Ok(CommonRelationsEvaluation::Blocked {
            code: "MISSING_PREMISES",
            message: "保存済みの根拠から両対象に共通して支持される関係を確認できません。共通点が存在しないとは断定しません。".into(),
        });
    }

    let origin = |guarantee: &Guarantee| match guarantee {
        Guarantee::UserPremise => "直接提示",
        Guarantee::Derived => "規則導出",
        _ => "根拠付き",
    };
    let mut report = vec![COMMON_RELATIONS_REPORT_HEADER.to_string()];
    let mut shared_proofs = Vec::with_capacity(supported.len());
    let mut dependencies = BTreeSet::new();
    let mut any_derived = false;
    for relation in supported {
        report.push(format!(
            "共通関係: {}（{}）／{}（{}）",
            describe(&relation.proposition_a),
            origin(&relation.guarantee_a),
            describe(&relation.proposition_b),
            origin(&relation.guarantee_b)
        ));
        report.extend(relation.explanation_lines_a);
        report.extend(relation.explanation_lines_b);
        dependencies.extend(relation.dependencies_a.iter().cloned());
        dependencies.extend(relation.dependencies_b.iter().cloned());
        any_derived |= relation.guarantee_a == Guarantee::Derived
            || relation.guarantee_b == Guarantee::Derived;
        shared_proofs.push(CommonRelationProofArtifact {
            proposition_a: relation.proposition_a,
            proposition_b: relation.proposition_b,
            guarantee_a: relation.guarantee_a,
            guarantee_b: relation.guarantee_b,
            dependencies_a: relation.dependencies_a,
            dependencies_b: relation.dependencies_b,
        });
    }
    let report = report.join("\n");
    let guarantee = if any_derived {
        Guarantee::Derived
    } else {
        Guarantee::UserPremise
    };
    let artifact = CommonRelationsReportArtifact {
        kind: COMMON_RELATIONS_REPORT_KIND.into(),
        result_id: result_id.into(),
        report_sha256: digest(report.as_bytes()),
        subject_a: subject_a.into(),
        subject_b: subject_b.into(),
        scope: scope.into(),
        shared_proofs,
    };
    let artifact_bytes = canonical(&artifact)?;
    if report.len().saturating_add(artifact_bytes.len()) > max_evidence_bytes {
        return Ok(CommonRelationsEvaluation::Blocked {
            code: "RESOURCE_LIMIT",
            message: "共通関係の出力と再検証artifactが要求の証拠予算を超えました。".into(),
        });
    }
    Ok(CommonRelationsEvaluation::Complete(CommonRelationsAnswer {
        report,
        artifact_bytes,
        proof_dependencies: dependencies.into_iter().collect(),
        guarantee,
    }))
}

/// Rebuild the bounded common-relation report and its proof artifact from one
/// actor-memory snapshot. Core uses this when adopting the result; Recovery
/// uses the same computation to replay the retained revision and recheck the
/// current revision before confirming a pending file effect.
pub(crate) fn common_relations_answer(
    memory: &MemoryState,
    subject_a: &str,
    subject_b: &str,
    scope: &str,
    max_actions: u32,
    max_evidence_bytes: usize,
    result_id: &str,
) -> Result<CommonRelationsEvaluation> {
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
    let action_budget = usize::try_from(max_actions).unwrap_or(usize::MAX);
    if scanned_records > action_budget {
        return Ok(CommonRelationsEvaluation::Blocked {
            code: "RESOURCE_LIMIT",
            message: "保存済みの関係を比較候補として走査する量が推論予算を超えました。".into(),
        });
    }
    let scanned_actions = u32::try_from(scanned_records).map_err(|_| {
        failure(
            "RESOURCE_LIMIT",
            "common-relation scan count exceeds the bounded action representation",
        )
    })?;
    let remaining_actions = max_actions.saturating_sub(scanned_actions);
    let mut candidates = BTreeSet::new();
    let includes_subject = |subject: &str| {
        subject == subject_a
            || subject == subject_b
            || subject.starts_with('$')
            || subject.starts_with('?')
    };
    let mut include_candidate = |proposition: &Proposition| {
        if proposition.scope == scope
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
        if fact.valid && !matches!(fact.guarantee, Guarantee::Hypothesis | Guarantee::Unknown) {
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
        return Ok(CommonRelationsEvaluation::Blocked {
            code: "MISSING_PREMISES",
            message: "二つの対象について比較できる保存済み関係がありません。共通点がないとは断定しません。".into(),
        });
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
        return Ok(CommonRelationsEvaluation::Blocked {
            code: "RESOURCE_LIMIT",
            message: "各比較候補を両方の対象について照合する推論予算が不足しています。".into(),
        });
    }
    let per_query_budget = remaining_actions / query_count;
    if per_query_budget == 0 {
        return Ok(CommonRelationsEvaluation::Blocked {
            code: "RESOURCE_LIMIT",
            message: "関係比較を最後まで行う推論予算がありません。".into(),
        });
    }

    let mut supported = Vec::new();
    for (predicate, object, negative) in candidates {
        let proposition_a = Proposition {
            subject: subject_a.into(),
            predicate: predicate.clone(),
            object: object.clone(),
            negative,
            scope: scope.into(),
            temporal_scope: None,
            condition_refs: Default::default(),
        };
        let proposition_b = Proposition {
            subject: subject_b.into(),
            ..proposition_a.clone()
        };
        let answer_a = relations::query(memory, &proposition_a, per_query_budget)?;
        let answer_b = relations::query(memory, &proposition_b, per_query_budget)?;
        if answer_a.status == "CONFLICT" || answer_b.status == "CONFLICT" {
            return Ok(CommonRelationsEvaluation::Blocked {
                code: "CONFLICTING_PREMISES",
                message:
                    "比較対象のいずれかに肯定・否定の競合があります。共通関係を選択せず停止します。"
                        .into(),
            });
        }
        if !matches!(
            answer_a.status.as_str(),
            "SUPPORTED" | "CONTRADICTED" | "UNKNOWN"
        ) || !matches!(
            answer_b.status.as_str(),
            "SUPPORTED" | "CONTRADICTED" | "UNKNOWN"
        ) {
            return Ok(CommonRelationsEvaluation::Blocked {
                code: "UNSUPPORTED_RELATION_RESULT",
                message: "比較対象の関係照合が既知の終端状態を返しませんでした。".into(),
            });
        }
        if answer_a.status != "SUPPORTED" || answer_b.status != "SUPPORTED" {
            continue;
        }
        let guarantee_a = relations::answer_guarantee(&answer_a);
        let guarantee_b = relations::answer_guarantee(&answer_b);
        let dependencies_a = relations::answer_dependencies(&answer_a);
        let dependencies_b = relations::answer_dependencies(&answer_b);
        let explanation_lines_a = relation_explanation_lines(
            memory,
            &proposition_a,
            &answer_a,
            &format!("{}側の根拠", subject_a),
        );
        let explanation_lines_b = relation_explanation_lines(
            memory,
            &proposition_b,
            &answer_b,
            &format!("{}側の根拠", subject_b),
        );
        supported.push(SupportedCommonRelation {
            proposition_a,
            proposition_b,
            guarantee_a,
            guarantee_b,
            dependencies_a,
            dependencies_b,
            explanation_lines_a,
            explanation_lines_b,
        });
    }
    common_relations_answer_from_supported(
        subject_a,
        subject_b,
        scope,
        result_id,
        max_evidence_bytes,
        supported,
    )
}

fn indexed_common_relations_answer(
    index: &crate::store::MemoryIndexSnapshot<'_, '_>,
    subject_a: &str,
    subject_b: &str,
    scope: &str,
    max_actions: u32,
    max_evidence_bytes: usize,
    result_id: &str,
) -> Result<CommonRelationsEvaluation> {
    if max_actions == 0 {
        return Ok(CommonRelationsEvaluation::Blocked {
            code: "RESOURCE_LIMIT",
            message: "共通関係の候補を選ぶ推論予算がありません。".into(),
        });
    }
    let candidate_record_budget = usize::try_from(max_actions - 1).unwrap_or(usize::MAX);
    let candidate_records =
        index.common_relation_candidates(subject_a, subject_b, scope, candidate_record_budget)?;
    let candidate_record_actions = u32::try_from(candidate_records.len())
        .map_err(|_| failure("RESOURCE_LIMIT", "common-relation record count overflowed"))?;
    let selection_actions = candidate_record_actions.checked_add(1).ok_or_else(|| {
        failure(
            "RESOURCE_LIMIT",
            "common-relation selection action count overflowed",
        )
    })?;
    let remaining_actions = max_actions.checked_sub(selection_actions).ok_or_else(|| {
        failure(
            "RELATION_BUDGET",
            "common-relation candidate selection exhausted the action budget",
        )
    })?;

    let mut candidates = BTreeSet::new();
    let mut include_candidate = |proposition: &Proposition| {
        if proposition.scope == scope
            && (proposition.subject == subject_a
                || proposition.subject == subject_b
                || proposition.subject.starts_with('$')
                || proposition.subject.starts_with('?'))
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
    for record in &candidate_records {
        match &record.value {
            crate::store::IndexedMemoryValue::Fact(fact)
                if fact.valid
                    && !matches!(fact.guarantee, Guarantee::Hypothesis | Guarantee::Unknown) =>
            {
                include_candidate(&fact.proposition);
            }
            crate::store::IndexedMemoryValue::Rule(rule) if rule.valid => {
                for proposition in std::iter::once(&rule.premise)
                    .chain(rule.additional_premises.iter())
                    .chain(std::iter::once(&rule.conclusion))
                {
                    include_candidate(proposition);
                }
            }
            crate::store::IndexedMemoryValue::Fact(_)
            | crate::store::IndexedMemoryValue::Rule(_)
            | crate::store::IndexedMemoryValue::Condition(_) => {}
        }
    }
    drop(include_candidate);
    if candidates.is_empty() {
        return Ok(CommonRelationsEvaluation::Blocked {
            code: "MISSING_PREMISES",
            message: "二つの対象について比較できる保存済み関係がありません。共通点がないとは断定しません。".into(),
        });
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
        return Ok(CommonRelationsEvaluation::Blocked {
            code: "RESOURCE_LIMIT",
            message: "各比較候補を両方の対象について照合する推論予算が不足しています。".into(),
        });
    }
    let per_query_budget = remaining_actions / query_count;
    if per_query_budget == 0 {
        return Ok(CommonRelationsEvaluation::Blocked {
            code: "RESOURCE_LIMIT",
            message: "関係比較を最後まで行う推論予算がありません。".into(),
        });
    }

    let mut supported = Vec::new();
    for (predicate, object, negative) in candidates {
        let proposition_a = Proposition {
            subject: subject_a.into(),
            predicate,
            object,
            negative,
            scope: scope.into(),
            temporal_scope: None,
            condition_refs: Default::default(),
        };
        let proposition_b = Proposition {
            subject: subject_b.into(),
            ..proposition_a.clone()
        };
        let (memory_a, answer_a) =
            indexed_common_relation_query(index, &proposition_a, per_query_budget)?;
        let (memory_b, answer_b) =
            indexed_common_relation_query(index, &proposition_b, per_query_budget)?;
        if answer_a.status == "CONFLICT" || answer_b.status == "CONFLICT" {
            return Ok(CommonRelationsEvaluation::Blocked {
                code: "CONFLICTING_PREMISES",
                message:
                    "比較対象のいずれかに肯定・否定の競合があります。共通関係を選択せず停止します。"
                        .into(),
            });
        }
        if !matches!(
            answer_a.status.as_str(),
            "SUPPORTED" | "CONTRADICTED" | "UNKNOWN"
        ) || !matches!(
            answer_b.status.as_str(),
            "SUPPORTED" | "CONTRADICTED" | "UNKNOWN"
        ) {
            return Ok(CommonRelationsEvaluation::Blocked {
                code: "UNSUPPORTED_RELATION_RESULT",
                message: "比較対象の関係照合が既知の終端状態を返しませんでした。".into(),
            });
        }
        if answer_a.status != "SUPPORTED" || answer_b.status != "SUPPORTED" {
            continue;
        }
        let guarantee_a = relations::answer_guarantee(&answer_a);
        let guarantee_b = relations::answer_guarantee(&answer_b);
        let dependencies_a = relations::answer_dependencies(&answer_a);
        let dependencies_b = relations::answer_dependencies(&answer_b);
        let explanation_lines_a = relation_explanation_lines(
            &memory_a,
            &proposition_a,
            &answer_a,
            &format!("{}側の根拠", subject_a),
        );
        let explanation_lines_b = relation_explanation_lines(
            &memory_b,
            &proposition_b,
            &answer_b,
            &format!("{}側の根拠", subject_b),
        );
        supported.push(SupportedCommonRelation {
            proposition_a,
            proposition_b,
            guarantee_a,
            guarantee_b,
            dependencies_a,
            dependencies_b,
            explanation_lines_a,
            explanation_lines_b,
        });
    }
    common_relations_answer_from_supported(
        subject_a,
        subject_b,
        scope,
        result_id,
        max_evidence_bytes,
        supported,
    )
}

/// Evaluate common relations only from a revision-matched derived index.
/// `None` means the index is cold or stale; callers retain their authenticated
/// full-snapshot compatibility path in that case.
pub(crate) fn indexed_common_relations_answer_from_store(
    store: &Store,
    actor: &str,
    replay: CommonRelationsReplay<'_>,
) -> Result<Option<(u64, Result<CommonRelationsEvaluation>)>> {
    store.with_relation_index(actor, |index| {
        let revision = index.revision();
        let answer = match indexed_common_relations_answer(
            index,
            replay.subject_a,
            replay.subject_b,
            replay.scope,
            replay.max_actions,
            replay.max_evidence_bytes,
            replay.result_id,
        ) {
            Ok(answer) => Ok(answer),
            Err(error)
                if matches!(
                    &error,
                    Error::Contract { code, .. } if code == "RELATION_BUDGET"
                ) =>
            {
                Ok(CommonRelationsEvaluation::Blocked {
                    code: "RESOURCE_LIMIT",
                    message: error.to_string(),
                })
            }
            Err(error) => Err(error),
        };
        Ok((revision, answer))
    })
}

fn indexed_common_relation_query(
    index: &crate::store::MemoryIndexSnapshot<'_, '_>,
    proposition: &Proposition,
    max_actions: u32,
) -> Result<(MemoryState, relations::QueryOutcome)> {
    let (memory, projection_actions) =
        relations::active_relation_projection(index, proposition, max_actions)?;
    let query_budget = max_actions.checked_sub(projection_actions).ok_or_else(|| {
        failure(
            "RELATION_BUDGET",
            "common-relation proof exhausted its projection action budget",
        )
    })?;
    let mut answer = relations::query(&memory, proposition, query_budget)?;
    answer.steps = projection_actions
        .checked_add(answer.steps)
        .ok_or_else(|| failure("RELATION_BUDGET", "common-relation action count overflowed"))?;
    Ok((memory, answer))
}

/// Rebuild the detailed explanation of a retained common-relations report
/// against one actor-memory snapshot. Core and Recovery use the same proof,
/// dependency, guarantee, and output-budget checks.
pub(crate) fn common_relations_explanation(
    memory: &MemoryState,
    report_bytes: &[u8],
    report_ref: &str,
    prior: &ResultRecord,
    replay: CommonRelationsExplanationReplay<'_>,
) -> Result<CommonRelationsExplanationEvaluation> {
    common_relations_explanation_with_query(
        report_bytes,
        report_ref,
        prior,
        replay,
        |proposition, max_actions, heading| {
            let answer = relations::query(memory, proposition, max_actions)?;
            let lines = relation_explanation_lines(memory, proposition, &answer, heading);
            Ok((answer, lines))
        },
    )
}

/// Re-evaluate a saved common-relations explanation from the revision-matched
/// derived index. `None` means the index is cold or stale and the caller must
/// use its authenticated snapshot path.
pub(crate) fn indexed_common_relations_explanation_from_store(
    store: &Store,
    actor: &str,
    report_bytes: &[u8],
    report_ref: &str,
    prior: &ResultRecord,
    replay: CommonRelationsExplanationReplay<'_>,
) -> Result<Option<(u64, Result<CommonRelationsExplanationEvaluation>)>> {
    store.with_relation_index(actor, |index| {
        let revision = index.revision();
        let evaluation = common_relations_explanation_with_query(
            report_bytes,
            report_ref,
            prior,
            replay,
            |proposition, max_actions, heading| {
                let (memory, answer) =
                    indexed_common_relation_query(index, proposition, max_actions)?;
                let lines = relation_explanation_lines(&memory, proposition, &answer, heading);
                Ok((answer, lines))
            },
        );
        let evaluation = match evaluation {
            Err(error)
                if matches!(
                    &error,
                    Error::Contract { code, .. } if code == "RELATION_BUDGET"
                ) =>
            {
                Ok(CommonRelationsExplanationEvaluation::Blocked {
                    code: "RESOURCE_LIMIT",
                    message: error.to_string(),
                })
            }
            other => other,
        };
        Ok((revision, evaluation))
    })
}

fn common_relations_explanation_with_query(
    report_bytes: &[u8],
    report_ref: &str,
    prior: &ResultRecord,
    replay: CommonRelationsExplanationReplay<'_>,
    mut query: impl FnMut(&Proposition, u32, &str) -> Result<(relations::QueryOutcome, Vec<String>)>,
) -> Result<CommonRelationsExplanationEvaluation> {
    if report_bytes.len() > replay.max_evidence_bytes {
        return Ok(CommonRelationsExplanationEvaluation::Blocked {
            code: "RESOURCE_LIMIT",
            message: "前回の共通関係artifactが今回の証拠予算を超えています。".into(),
        });
    }
    let artifact = match serde_json::from_slice::<CommonRelationsReportArtifact>(report_bytes) {
        Ok(value) => value,
        Err(_) => {
            return Ok(CommonRelationsExplanationEvaluation::Blocked {
                code: "COMMON_RELATION_CONTEXT_INVALID",
                message: "前回の共通関係artifactの形式を検証できません。".into(),
            });
        }
    };
    let Some(prior_plan) = prior.content_plan.as_ref() else {
        return Ok(CommonRelationsExplanationEvaluation::Blocked {
            code: "COMMON_RELATION_CONTEXT_INVALID",
            message: "前回結果に共通関係の出力計画がありません。".into(),
        });
    };
    if canonical(&artifact)? != report_bytes
        || artifact.kind != COMMON_RELATIONS_REPORT_KIND
        || artifact.result_id != prior.id
        || artifact.report_sha256 != digest(prior.text.as_bytes())
        || !prior.text.starts_with(COMMON_RELATIONS_REPORT_HEADER)
        || prior.lifecycle != Lifecycle::Commit
        || prior.calculation.is_some()
        || prior.dependencies.first().map(String::as_str) != Some(report_ref)
        || prior_plan.text != prior.text
        || prior_plan.guarantee != prior.guarantee
        || prior_plan.required_units != vec![prior.text.clone()]
        || artifact.subject_a != replay.subject_a
        || artifact.subject_b != replay.subject_b
        || artifact.scope != replay.scope
        || replay.subject_a.is_empty()
        || replay.subject_b.is_empty()
        || replay.subject_a == replay.subject_b
        || replay.scope.is_empty()
        || artifact.shared_proofs.is_empty()
    {
        return Ok(CommonRelationsExplanationEvaluation::Blocked {
            code: "COMMON_RELATION_CONTEXT_INVALID",
            message: "前回結果とartifactの同一性または比較条件が一致しません。".into(),
        });
    }
    let query_count = match u32::try_from(artifact.shared_proofs.len())
        .ok()
        .and_then(|count| count.checked_mul(2))
    {
        Some(count) if count <= replay.max_actions => count,
        _ => {
            return Ok(CommonRelationsExplanationEvaluation::Blocked {
                code: "RESOURCE_LIMIT",
                message: "各共通関係を両対象について再検証する推論予算が不足しています。".into(),
            });
        }
    };
    let per_query_budget = replay.max_actions / query_count;
    let mut units = vec![format!(
        "前回比較の詳細説明（比較結果ID: {}）。候補となった各関係を、同一ケースの現在有効な記憶に対して両対象ごとに再照合しました。",
        prior.id
    )];
    let mut expected_proof_dependencies = BTreeSet::new();
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
            return Ok(CommonRelationsExplanationEvaluation::Blocked {
                code: "COMMON_RELATION_CONTEXT_INVALID",
                message: "前回の比較条件または左右の証拠一覧に不整合があります。".into(),
            });
        }
        let (answer_a, explanation_lines_a) = query(
            &proof.proposition_a,
            per_query_budget,
            &format!("{}側の証明", artifact.subject_a),
        )?;
        let (answer_b, explanation_lines_b) = query(
            &proof.proposition_b,
            per_query_budget,
            &format!("{}側の証明", artifact.subject_b),
        )?;
        if let Some((_, code, message)) = condition_block(&answer_a) {
            return Ok(CommonRelationsExplanationEvaluation::Blocked { code, message });
        }
        if let Some((_, code, message)) = condition_block(&answer_b) {
            return Ok(CommonRelationsExplanationEvaluation::Blocked { code, message });
        }
        if !matches!(answer_a.status.as_str(), "SUPPORTED")
            || !matches!(answer_b.status.as_str(), "SUPPORTED")
        {
            return Ok(CommonRelationsExplanationEvaluation::Blocked {
                code: "CONTEXT_PROOF_CHANGED",
                message: "前回比較の左右いずれかを支えた現在有効な根拠が変わっています。新しい比較を行ってください。".into(),
            });
        }
        let dependencies_a = relations::answer_dependencies(&answer_a);
        let dependencies_b = relations::answer_dependencies(&answer_b);
        let guarantee_a = relations::answer_guarantee(&answer_a);
        let guarantee_b = relations::answer_guarantee(&answer_b);
        if dependencies_a != proof.dependencies_a
            || dependencies_b != proof.dependencies_b
            || guarantee_a != proof.guarantee_a
            || guarantee_b != proof.guarantee_b
        {
            return Ok(CommonRelationsExplanationEvaluation::Blocked {
                code: "CONTEXT_PROOF_CHANGED",
                message: "前回比較の左右いずれかを支えた現在有効な根拠が変わっています。新しい比較を行ってください。".into(),
            });
        }
        expected_proof_dependencies.extend(dependencies_a.iter().cloned());
        expected_proof_dependencies.extend(dependencies_b.iter().cloned());
        any_derived |= guarantee_a == Guarantee::Derived || guarantee_b == Guarantee::Derived;
        units.push(format!(
            "共通関係の照合: {} ／ {}（範囲: {}）。両側で同一の述語・対象・肯定否定・範囲が支持されています。",
            describe(&proof.proposition_a),
            describe(&proof.proposition_b),
            artifact.scope
        ));
        units.push(format!(
            "{}側: SUPPORTED（{}）；{}側: SUPPORTED（{}）。左右の証拠IDを別々に再確認しました。",
            artifact.subject_a,
            guarantee_label(&guarantee_a),
            artifact.subject_b,
            guarantee_label(&guarantee_b)
        ));
        units.extend(explanation_lines_a);
        units.extend(explanation_lines_b);
    }
    let mut expected_prior_dependencies = vec![report_ref.to_string()];
    expected_prior_dependencies.extend(expected_proof_dependencies);
    let expected_guarantee = if any_derived {
        Guarantee::Derived
    } else {
        Guarantee::UserPremise
    };
    if prior.dependencies != expected_prior_dependencies || prior.guarantee != expected_guarantee {
        return Ok(CommonRelationsExplanationEvaluation::Blocked {
            code: "COMMON_RELATION_CONTEXT_INVALID",
            message: "前回比較の依存記録または全体保証がartifactと一致しません。".into(),
        });
    }
    units.push(
        "一覧にない関係は「共通点がない」ことを意味しません。この説明は保存済みの前提・規則の範囲に限られ、現実世界の真偽を確認したものではありません。".into(),
    );
    if units.join("\n").len() > replay.max_evidence_bytes {
        return Ok(CommonRelationsExplanationEvaluation::Blocked {
            code: "RESOURCE_LIMIT",
            message: "再検証した共通関係の説明が今回の証拠予算を超えています。".into(),
        });
    }
    let mut dependencies = vec![prior.id.clone()];
    dependencies.extend(prior.dependencies.iter().cloned());
    Ok(CommonRelationsExplanationEvaluation::Complete(
        CommonRelationsExplanationAnswer {
            units,
            dependencies,
            guarantee: prior.guarantee.clone(),
        },
    ))
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

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct RelationComparisonContentArtifact {
    kind: String,
    result_id: String,
    report_sha256: String,
    subject_a: String,
    subject_b: String,
    scope: String,
    inventory_a: relations::RelationInventory,
    inventory_b: relations::RelationInventory,
    content_request: language::MeaningCompositionRequest,
    content_composition: language::MeaningCompositionReport,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct RelationComparisonMeaningPlan {
    subject_a: String,
    subject_b: String,
    scope: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    format: Option<MeaningCompositionFormat>,
    rows: Vec<RelationComparisonMeaningRow>,
    proof_ids: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct RelationComparisonMeaningRow {
    class: RelationComparisonClass,
    left_fact_ids: Vec<String>,
    right_fact_ids: Vec<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
enum RelationComparisonClass {
    Common,
    LeftOnly,
    RightOnly,
    Contested,
}

#[derive(serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct RelationComparisonGeneratedArtifact {
    kind: String,
    result_id: String,
    report_sha256: String,
    subject_a: String,
    subject_b: String,
    scope: String,
    inventory_a: relations::RelationInventory,
    inventory_b: relations::RelationInventory,
    meaning_plan: RelationComparisonMeaningPlan,
    content_generation: language::MeaningGenerationReport,
}

struct RelationComparisonArtifactView {
    canonical_bytes: Vec<u8>,
    kind: String,
    result_id: String,
    report_sha256: String,
    subject_a: String,
    subject_b: String,
    scope: String,
    inventory_a: relations::RelationInventory,
    inventory_b: relations::RelationInventory,
    content: Option<(
        language::MeaningCompositionRequest,
        language::MeaningCompositionReport,
    )>,
    generated: Option<(
        RelationComparisonMeaningPlan,
        language::MeaningGenerationReport,
    )>,
}

/// Typed terminal of the direct, bounded counterfactual Core API. This API
/// constructs an isolated scenario from typed propositions; it does not add a
/// natural-language counterfactual route.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum CounterfactualRelationStatus {
    Supported,
    Contradicted,
    Conflict,
    Unknown,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CounterfactualRelationReport {
    pub status: CounterfactualRelationStatus,
    pub target: Proposition,
    pub guarantee: Guarantee,
    pub facts: Vec<Fact>,
    pub dependencies: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub condition_status: Option<ConditionStatus>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub condition_evaluations: Vec<ConditionEvaluation>,
    pub hypothesis_ids: Vec<String>,
    pub hypothesis_ids_used: Vec<String>,
    pub superseded_fact_ids: Vec<String>,
    pub invalidated_record_ids: Vec<String>,
    pub base_memory_revision: u64,
    pub scenario_memory_revision: u64,
    pub steps: u32,
}

/// Trusted frontends supply an authenticated local principal. A caller-controlled
/// actor string is not remote authentication; the HTTP adapter binds it to its
/// configured authenticated identity before entering this API.
pub struct Engine {
    pub(crate) store: Store,
    #[cfg(test)]
    pub(crate) expire_output_at: Option<OutputCheckpoint>,
}

#[cfg(test)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum OutputCheckpoint {
    Rendered,
    FilePhase(EffectPhase),
    FileDelivery,
    Commit,
    CommitPersistence,
}

impl Engine {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        Ok(Self {
            store: Store::open(path)?,
            #[cfg(test)]
            expire_output_at: None,
        })
    }

    #[cfg(test)]
    fn output_deadline_at(&self, checkpoint: OutputCheckpoint, deadline: Instant) -> Instant {
        if self.expire_output_at == Some(checkpoint) {
            deadline.min(Instant::now())
        } else {
            deadline
        }
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

    /// Read the durable result for an exact input and version of an actor-owned case.
    /// A missing result yields `None`.
    pub fn input_result(
        &self,
        actor: &str,
        case: &str,
        input: &str,
        version: u64,
    ) -> Result<Option<ResultRecord>> {
        self.store.get_input_result(actor, case, input, version)
    }

    /// The verified case head and its earlier result IDs, newest revision first.
    pub fn case_with_history_order(
        &self,
        actor: &str,
        case: &str,
    ) -> Result<Option<(CaseRevision, Vec<String>)>> {
        self.store.load_case_with_history_order(actor, case)
    }

    /// Build a bounded sequence from directly supplied typed semantic units.
    /// The API belongs to Core but deliberately does not parse natural language,
    /// verify the supplied evidence labels, mutate a case, or adopt content.
    pub fn compose_typed_meaning(
        &self,
        request: &language::MeaningCompositionRequest,
    ) -> Result<language::MeaningCompositionReport> {
        language::compose_typed_meaning(request)
    }

    /// Evaluate a bounded counterfactual relation in a disposable clone of
    /// actor memory. Only exact, active USER_PREMISE opposites explicitly
    /// named by `superseded_fact_ids` may be invalidated in that clone. The
    /// method performs no persistent write, effect, or content adoption.
    /// Natural-language parsing is intentionally outside this API.
    pub fn evaluate_counterfactual_relation(
        &self,
        actor: &str,
        case: &str,
        assumptions: &[Proposition],
        superseded_fact_ids: &[String],
        target: &Proposition,
        max_actions: u32,
    ) -> Result<CounterfactualRelationReport> {
        const MAX_ASSUMPTIONS: usize = 8;
        const MAX_SUPERSEDED_FACTS: usize = 16;
        const MAX_ACTIONS: u32 = 4096;

        if self.store.load_case(actor, case)?.is_none() {
            return Err(failure(
                "CASE_NOT_FOUND",
                "counterfactual evaluation requires an actor-owned case",
            ));
        }
        if assumptions.is_empty() || assumptions.len() > MAX_ASSUMPTIONS {
            return Err(failure(
                "COUNTERFACTUAL_ASSUMPTION_LIMIT",
                "counterfactual evaluation requires one to eight explicit assumptions",
            ));
        }
        if superseded_fact_ids.len() > MAX_SUPERSEDED_FACTS {
            return Err(failure(
                "COUNTERFACTUAL_OVERRIDE_LIMIT",
                "counterfactual override list exceeds its bounded limit",
            ));
        }
        if max_actions == 0 || max_actions > MAX_ACTIONS {
            return Err(failure(
                "COUNTERFACTUAL_BUDGET",
                "counterfactual action budget must be between one and 4096",
            ));
        }
        relations::validate(target)?;
        if !relations::is_ground(target) {
            return Err(failure(
                "UNBOUND_VARIABLE",
                "a counterfactual target requires bound subject and object",
            ));
        }

        let mut unique_assumptions = BTreeSet::new();
        for assumption in assumptions {
            relations::validate(assumption)?;
            if !relations::is_ground(assumption) || assumption.scope != target.scope {
                return Err(failure(
                    "COUNTERFACTUAL_SCOPE_MISMATCH",
                    "assumptions must be ground propositions in the target scope",
                ));
            }
            if !unique_assumptions.insert(assumption.clone())
                || unique_assumptions.contains(&relations::opposition(assumption))
            {
                return Err(failure(
                    "COUNTERFACTUAL_ASSUMPTION_CONFLICT",
                    "assumptions must be unique and may not contain exact opposites",
                ));
            }
        }

        let mut unique_superseded = BTreeSet::new();
        if superseded_fact_ids
            .iter()
            .any(|id| !unique_superseded.insert(id.clone()))
        {
            return Err(failure(
                "COUNTERFACTUAL_OVERRIDE_DUPLICATE",
                "a superseded fact identity may be listed only once",
            ));
        }

        let mut memory = self.store.load_memory(actor)?;
        let base_memory_revision = memory.revision;
        let record_count = memory
            .facts
            .len()
            .checked_add(memory.rules.len())
            .and_then(|count| count.checked_add(memory.conditions.len()))
            .ok_or_else(|| failure("COUNTERFACTUAL_BUDGET", "memory scan count overflowed"))?;
        let record_actions = u32::try_from(record_count).map_err(|_| {
            failure(
                "COUNTERFACTUAL_BUDGET",
                "actor memory exceeds the bounded scan representation",
            )
        })?;
        let preparation_actions = record_actions
            .checked_add(u32::try_from(superseded_fact_ids.len()).unwrap_or(u32::MAX))
            .and_then(|count| {
                count.checked_add(u32::try_from(assumptions.len()).unwrap_or(u32::MAX))
            })
            .ok_or_else(|| failure("COUNTERFACTUAL_BUDGET", "scenario preparation overflowed"))?;
        if preparation_actions >= max_actions {
            return Err(failure(
                "COUNTERFACTUAL_BUDGET",
                "memory scan and scenario setup consume the bounded action budget",
            ));
        }

        for id in superseded_fact_ids {
            let Some(fact) = memory.facts.iter().find(|fact| fact.id == *id) else {
                return Err(failure(
                    "COUNTERFACTUAL_OVERRIDE_INVALID",
                    "each override must identify an active direct actor-memory fact",
                ));
            };
            let exact_opposite = assumptions
                .iter()
                .any(|assumption| fact.proposition == relations::opposition(assumption));
            if !fact.valid
                || fact.guarantee != Guarantee::UserPremise
                || !fact.dependencies.is_empty()
                || fact.source_input.is_empty()
                || !exact_opposite
            {
                return Err(failure(
                    "COUNTERFACTUAL_OVERRIDE_INVALID",
                    "only an active direct USER_PREMISE exactly opposite to an assumption may be superseded",
                ));
            }
        }

        let scenario_identity = identity(&(
            "counterfactual-scenario-v1",
            actor,
            case,
            base_memory_revision,
            assumptions,
            superseded_fact_ids,
            target,
        ))?;
        let mut invalidated_record_ids = relations::invalidate_in_order(
            &mut memory,
            superseded_fact_ids.iter().map(String::as_str),
        )?;
        invalidated_record_ids.sort_unstable();
        let mut hypothesis_ids = Vec::with_capacity(assumptions.len());
        for (ordinal, assumption) in assumptions.iter().enumerate() {
            let hypothesis = relations::add_hypothesis(
                &mut memory,
                assumption.clone(),
                &scenario_identity,
                ordinal,
            )?;
            hypothesis_ids.push(hypothesis.id);
        }
        let allowed_hypotheses: BTreeSet<_> = hypothesis_ids.iter().cloned().collect();
        // The ordinary query path evaluates target conditions before inference.
        // Reuse it here so a scenario cannot let a conflicting assumption
        // bypass a FALSE, UNKNOWN, or CONFLICT condition boundary.
        let outcome = relations::query_hypothetical(
            &memory,
            target,
            max_actions - preparation_actions,
            &allowed_hypotheses,
        )?;
        let outcome_dependencies = relations::answer_dependencies(&outcome);
        let outcome_dependency_set: BTreeSet<_> = outcome_dependencies.iter().cloned().collect();
        let outcome_hypothesis_ids_used: Vec<_> = hypothesis_ids
            .iter()
            .filter(|id| outcome_dependency_set.contains(*id))
            .cloned()
            .collect();
        if outcome
            .condition_status
            .is_some_and(|status| status != ConditionStatus::True)
        {
            return Ok(CounterfactualRelationReport {
                status: CounterfactualRelationStatus::Unknown,
                target: target.clone(),
                guarantee: Guarantee::Unknown,
                facts: outcome.facts,
                dependencies: outcome_dependencies,
                condition_status: outcome.condition_status,
                condition_evaluations: outcome.condition_evaluations,
                hypothesis_ids,
                hypothesis_ids_used: outcome_hypothesis_ids_used,
                superseded_fact_ids: superseded_fact_ids.to_vec(),
                invalidated_record_ids,
                base_memory_revision,
                scenario_memory_revision: memory.revision,
                steps: outcome.steps.saturating_add(preparation_actions),
            });
        }
        let conflicting_premises: Vec<_> = memory
            .facts
            .iter()
            .filter(|fact| {
                fact.valid
                    && assumptions
                        .iter()
                        .any(|assumption| fact.proposition == relations::opposition(assumption))
            })
            .cloned()
            .collect();
        if !conflicting_premises.is_empty() {
            let mut facts = conflicting_premises;
            facts.extend(
                memory
                    .facts
                    .iter()
                    .filter(|fact| allowed_hypotheses.contains(&fact.id))
                    .cloned(),
            );
            facts.extend(outcome.facts.iter().cloned());
            facts.sort_by(|left, right| left.id.cmp(&right.id));
            facts.dedup_by(|left, right| left.id == right.id);
            let mut dependency_set: BTreeSet<_> = outcome_dependencies.into_iter().collect();
            dependency_set.extend(facts.iter().flat_map(|fact| {
                std::iter::once(fact.id.clone()).chain(fact.dependencies.iter().cloned())
            }));
            let dependencies: Vec<_> = dependency_set.iter().cloned().collect();
            let hypothesis_ids_used = hypothesis_ids
                .iter()
                .filter(|id| dependency_set.contains(*id))
                .cloned()
                .collect();
            return Ok(CounterfactualRelationReport {
                status: CounterfactualRelationStatus::Conflict,
                target: target.clone(),
                guarantee: Guarantee::Unknown,
                facts,
                dependencies,
                condition_status: outcome.condition_status,
                condition_evaluations: outcome.condition_evaluations,
                hypothesis_ids,
                hypothesis_ids_used,
                superseded_fact_ids: superseded_fact_ids.to_vec(),
                invalidated_record_ids,
                base_memory_revision,
                scenario_memory_revision: memory.revision,
                steps: outcome.steps.saturating_add(preparation_actions),
            });
        }
        let dependencies = outcome_dependencies;
        let hypothesis_ids_used = outcome_hypothesis_ids_used;
        let status = match outcome.status.as_str() {
            "SUPPORTED" => CounterfactualRelationStatus::Supported,
            "CONTRADICTED" => CounterfactualRelationStatus::Contradicted,
            "CONFLICT" => CounterfactualRelationStatus::Conflict,
            _ => CounterfactualRelationStatus::Unknown,
        };
        let guarantee = match status {
            CounterfactualRelationStatus::Conflict | CounterfactualRelationStatus::Unknown => {
                Guarantee::Unknown
            }
            _ if !hypothesis_ids_used.is_empty() => Guarantee::Hypothesis,
            _ => relations::answer_guarantee(&outcome),
        };

        Ok(CounterfactualRelationReport {
            status,
            target: target.clone(),
            guarantee,
            facts: outcome.facts,
            dependencies,
            condition_status: outcome.condition_status,
            condition_evaluations: outcome.condition_evaluations,
            hypothesis_ids,
            hypothesis_ids_used,
            superseded_fact_ids: superseded_fact_ids.to_vec(),
            invalidated_record_ids,
            base_memory_revision,
            scenario_memory_revision: memory.revision,
            steps: outcome.steps.saturating_add(preparation_actions),
        })
    }

    /// Build and adopt an explanation from one atomic, typed relation.
    /// Core resolves the supporting record IDs from actor memory, seals the
    /// typed input, then sends it through the same compute, content, render,
    /// and result lifecycle used by ordinary requests. This is a construction
    /// entry point; it does not parse natural language or accept finished prose.
    /// `request.text` must be empty; the method stores its canonical typed input
    /// there while retaining the envelope's identity, budget, and permissions.
    pub fn compose_atomic_explanation(
        &mut self,
        actor: &str,
        mut request: RequestEnvelope,
        proposition: Proposition,
    ) -> Result<ResultRecord> {
        if actor != request.actor_id {
            return Err(failure(
                "ACTOR_MISMATCH",
                "authenticated actor and request actor differ",
            ));
        }
        if request.context_ref.is_some() {
            return Err(failure(
                "DIRECT_CONTEXT_UNSUPPORTED",
                "atomic explanation accepts a proposition and resolves its proof from actor memory",
            ));
        }
        if !request.text.is_empty() {
            return Err(failure(
                "DIRECT_TEXT_FORBIDDEN",
                "direct atomic explanation accepts typed relation fields, not text",
            ));
        }
        if !supports_japanese_output_locale(&request.locale) {
            return Err(failure(
                "DIRECT_LOCALE_UNSUPPORTED",
                "the atomic explanation surface currently supports Japanese",
            ));
        }
        relations::validate(&proposition)?;
        if !relations::is_ground(&proposition) {
            return Err(failure(
                "UNBOUND_VARIABLE",
                "atomic explanation requires bound subject and object",
            ));
        }

        // Record the actual proof selected before execution. The normal Core
        // path rechecks these exact IDs immediately before generating content,
        // so a changed memory revision cannot silently substitute evidence.
        let mut source_ids = if request.budget.max_actions == 0 {
            Vec::new()
        } else {
            let memory = self.store.load_memory(actor)?;
            let answer = relations::query(&memory, &proposition, request.budget.max_actions)?;
            if matches!(answer.status.as_str(), "SUPPORTED" | "CONTRADICTED") {
                relations::answer_dependencies(&answer)
            } else {
                Vec::new()
            }
        };
        source_ids.sort();
        source_ids.dedup();
        let direct_input = DirectAtomicMeaningInput {
            schema: DIRECT_ATOMIC_MEANING_KIND.into(),
            purpose: "explain".into(),
            proposition: proposition.clone(),
            source_ids,
        };
        request.text = serde_json::to_string(&direct_input)?;
        if request.text.is_empty() || request.text.len() > request.budget.max_input_bytes {
            return Err(failure(
                "INPUT_LIMIT",
                "typed atomic meaning exceeds the request byte budget",
            ));
        }
        let bundle = direct_atomic_meaning_bundle(&request, &direct_input)?;
        self.execute_with_bundle(actor, request, Some(bundle))
    }

    /// Assert one typed relation through the Core-owned persistence path.
    /// This construction entry keeps temporal and world scopes in separate
    /// fields and does not parse or infer natural-language meaning.
    pub fn assert_atomic_relation(
        &mut self,
        actor: &str,
        mut request: RequestEnvelope,
        proposition: Proposition,
    ) -> Result<ResultRecord> {
        validate_direct_relation_request(actor, &request)?;
        relations::validate(&proposition)?;
        if !relations::is_ground(&proposition) {
            return Err(failure(
                "UNBOUND_VARIABLE",
                "a typed atomic premise requires a bound subject and object",
            ));
        }
        let input = DirectTypedRelationAssertionInput {
            schema: DIRECT_TYPED_RELATION_ASSERTION_KIND.into(),
            proposition,
        };
        request.text = serde_json::to_string(&input)?;
        if request.text.len() > request.budget.max_input_bytes {
            return Err(failure(
                "INPUT_LIMIT",
                "typed relation assertion exceeds the request byte budget",
            ));
        }
        let bundle = direct_typed_relation_assertion_bundle(&request, &input)?;
        self.execute_with_bundle(actor, request, Some(bundle))
    }

    /// Correct one exact typed premise through Core's versioned correction
    /// path. The superseded proposition is matched with its full identity,
    /// including world scope, temporal scope and condition references; descendants
    /// are invalidated before the replacement is adopted. This entry does not
    /// parse natural language.
    pub fn correct_atomic_relation(
        &mut self,
        actor: &str,
        mut request: RequestEnvelope,
        superseded: Proposition,
        replacement: Proposition,
    ) -> Result<ResultRecord> {
        validate_direct_relation_request(actor, &request)?;
        relations::validate(&superseded)?;
        relations::validate(&replacement)?;
        if !relations::is_ground(&superseded) || !relations::is_ground(&replacement) {
            return Err(failure(
                "UNBOUND_VARIABLE",
                "a typed relation correction requires bound subjects and objects",
            ));
        }
        if superseded.negative
            || replacement.negative
            || superseded.subject != replacement.subject
            || superseded.predicate != replacement.predicate
            || superseded == replacement
        {
            return Err(failure(
                "INVALID_RELATION_CORRECTION",
                "a typed relation correction needs distinct positive propositions for one subject and predicate",
            ));
        }
        let input = DirectTypedRelationCorrectionInput {
            schema: DIRECT_TYPED_RELATION_CORRECTION_KIND.into(),
            superseded,
            replacement,
        };
        request.text = serde_json::to_string(&input)?;
        if request.text.len() > request.budget.max_input_bytes {
            return Err(failure(
                "INPUT_LIMIT",
                "typed relation correction exceeds the request byte budget",
            ));
        }
        let bundle = direct_typed_relation_correction_bundle(&request, &input)?;
        self.execute_with_bundle(actor, request, Some(bundle))
    }

    /// Add a one- or two-premise typed rule through the same Core path used by
    /// parsed rules. Exact temporal-scope and condition-reference matching
    /// remain part of relation identity; Core evaluates condition references
    /// against actor-owned definitions and evidence. Chronology is not inferred.
    pub fn add_atomic_relation_rule(
        &mut self,
        actor: &str,
        mut request: RequestEnvelope,
        premises: Vec<Proposition>,
        conclusion: Proposition,
    ) -> Result<ResultRecord> {
        validate_direct_relation_request(actor, &request)?;
        if !(1..=2).contains(&premises.len()) {
            return Err(failure(
                "UNSUPPORTED_RULE_PREMISE_COUNT",
                "a typed relation rule requires one or two explicit premises",
            ));
        }
        for premise in &premises {
            relations::validate(premise)?;
        }
        relations::validate(&conclusion)?;
        let input = DirectTypedRelationRuleInput {
            schema: DIRECT_TYPED_RELATION_RULE_KIND.into(),
            premises,
            conclusion,
        };
        request.text = serde_json::to_string(&input)?;
        if request.text.len() > request.budget.max_input_bytes {
            return Err(failure(
                "INPUT_LIMIT",
                "typed relation rule exceeds the request byte budget",
            ));
        }
        let bundle = direct_typed_relation_rule_bundle(&request, &input)?;
        self.execute_with_bundle(actor, request, Some(bundle))
    }

    /// Define an immutable Core-owned condition record. Fact IDs remain
    /// evidence identities; `condition_id` is the separate reference used by
    /// propositions and expressions. Revised meaning requires a new ID.
    pub fn define_condition(
        &mut self,
        actor: &str,
        mut request: RequestEnvelope,
        definition: ConditionDefinition,
    ) -> Result<ResultRecord> {
        validate_direct_relation_request(actor, &request)?;
        relations::validate_condition_definition(&definition)?;
        let input = DirectTypedConditionInput {
            schema: DIRECT_TYPED_CONDITION_KIND.into(),
            definition,
        };
        request.text = serde_json::to_string(&input)?;
        if request.text.len() > request.budget.max_input_bytes {
            return Err(failure(
                "INPUT_LIMIT",
                "typed condition definition exceeds the request byte budget",
            ));
        }
        let bundle = direct_condition_bundle(&request, &input)?;
        self.execute_with_bundle(actor, request, Some(bundle))
    }

    /// Build and adopt a comparison from two typed actor-memory subjects and a
    /// scope. Core resolves each side's proof IDs, seals the typed request, then
    /// sends it through the existing comparison, content, render, and result
    /// lifecycle. It does not parse natural language or accept finished prose.
    /// `request.text` must be empty; the method stores its canonical typed input
    /// there while retaining the envelope's identity, budget, and permissions.
    pub fn compose_atomic_relation_comparison(
        &mut self,
        actor: &str,
        mut request: RequestEnvelope,
        subject_a: &str,
        subject_b: &str,
        scope: &str,
    ) -> Result<ResultRecord> {
        if actor != request.actor_id {
            return Err(failure(
                "ACTOR_MISMATCH",
                "authenticated actor and request actor differ",
            ));
        }
        if request.context_ref.is_some() {
            return Err(failure(
                "DIRECT_CONTEXT_UNSUPPORTED",
                "atomic relation comparison accepts two subjects and resolves their proofs from actor memory",
            ));
        }
        if !request.text.is_empty() {
            return Err(failure(
                "DIRECT_TEXT_FORBIDDEN",
                "direct relation comparison accepts typed subject and scope fields, not text",
            ));
        }
        if !supports_japanese_output_locale(&request.locale) {
            return Err(failure(
                "DIRECT_LOCALE_UNSUPPORTED",
                "the atomic relation comparison surface currently supports Japanese",
            ));
        }
        if subject_a.is_empty()
            || subject_b.is_empty()
            || subject_a == subject_b
            || scope.is_empty()
            || subject_a.len() > 512
            || subject_b.len() > 512
            || scope.len() > 512
        {
            return Err(failure(
                "INVALID_RELATION_COMPARISON",
                "relation comparison requires two distinct bounded subjects and one nonempty bounded scope",
            ));
        }

        // Capture both proof sets before execution. The normal Core comparison
        // recomputes them before generating content and suspends if either side
        // changed, so a stale preflight cannot silently replace its evidence.
        let per_subject_actions = request.budget.max_actions / 2;
        let (mut source_ids_a, mut source_ids_b) = if per_subject_actions == 0 {
            (Vec::new(), Vec::new())
        } else {
            let inventories = self.active_relation_comparison_inventories(
                actor,
                subject_a,
                subject_b,
                scope,
                per_subject_actions,
            )?;
            let resolve_source_ids = |result: Result<relations::RelationInventory>| match result {
                Ok(inventory) => Ok(inventory.dependencies),
                Err(Error::Contract { code, .. }) if code == "RELATION_BUDGET" => Ok(Vec::new()),
                Err(error) => Err(error),
            };
            (
                resolve_source_ids(inventories.inventory_a)?,
                resolve_source_ids(inventories.inventory_b)?,
            )
        };
        source_ids_a.sort();
        source_ids_a.dedup();
        source_ids_b.sort();
        source_ids_b.dedup();
        let direct_input = DirectRelationComparisonInput {
            schema: DIRECT_RELATION_COMPARISON_KIND.into(),
            purpose: "compare-relations".into(),
            subject_a: subject_a.into(),
            subject_b: subject_b.into(),
            scope: scope.into(),
            source_ids_a,
            source_ids_b,
        };
        request.text = serde_json::to_string(&direct_input)?;
        if request.text.is_empty() || request.text.len() > request.budget.max_input_bytes {
            return Err(failure(
                "INPUT_LIMIT",
                "typed relation comparison exceeds the request byte budget",
            ));
        }
        let bundle = direct_relation_comparison_bundle(&request, &direct_input)?;
        self.execute_with_bundle(actor, request, Some(bundle))
    }

    fn indexed_relation_query(
        &self,
        actor: &str,
        proposition: &Proposition,
        max_actions: u32,
    ) -> Result<Option<(MemoryState, relations::QueryOutcome)>> {
        self.store.with_relation_index(actor, |index| {
            let (memory, evaluation) =
                indexed_relation_evaluation(index, proposition, max_actions)?;
            Ok((memory, evaluation.answer))
        })
    }

    fn indexed_conjunction_relation_queries(
        &self,
        actor: &str,
        propositions: &[Proposition],
        per_relation_budget: u32,
    ) -> Result<Option<ConjunctionRelationQueries>> {
        indexed_conjunction_relation_queries_from_store(
            &self.store,
            actor,
            propositions,
            per_relation_budget,
        )
    }

    fn indexed_common_relations_answer(
        &self,
        actor: &str,
        replay: CommonRelationsReplay<'_>,
    ) -> Result<Option<(u64, Result<CommonRelationsEvaluation>)>> {
        indexed_common_relations_answer_from_store(&self.store, actor, replay)
    }

    fn active_common_relations_answer(
        &self,
        actor: &str,
        replay: CommonRelationsReplay<'_>,
    ) -> Result<(u64, Result<CommonRelationsEvaluation>)> {
        if let Some(answer) = self.indexed_common_relations_answer(actor, replay)? {
            return Ok(answer);
        }

        // Rebuild a cold or stale derived index from the authenticated current
        // snapshot once, then retry against one revision-matched transaction.
        let memory = self.store.load_memory(actor)?;
        self.store
            .rebuild_relation_index_if_current(actor, &memory)?;
        if let Some(answer) = self.indexed_common_relations_answer(actor, replay)? {
            return Ok(answer);
        }

        // If another writer advances the head during rebuild, use the
        // immutable snapshot already loaded for this request.
        let answer = common_relations_answer(
            &memory,
            replay.subject_a,
            replay.subject_b,
            replay.scope,
            replay.max_actions,
            replay.max_evidence_bytes,
            replay.result_id,
        );
        Ok((memory.revision, answer))
    }

    fn indexed_relation_profile_answer(
        &self,
        actor: &str,
        subject: &str,
        max_actions: u32,
        max_evidence_bytes: usize,
    ) -> Result<Option<(u64, Result<RelationProfileAnswer>)>> {
        indexed_relation_profile_answer_from_store(
            &self.store,
            actor,
            subject,
            max_actions,
            max_evidence_bytes,
        )
    }

    fn active_relation_profile_answer(
        &self,
        actor: &str,
        subject: &str,
        max_actions: u32,
        max_evidence_bytes: usize,
    ) -> Result<(u64, Result<RelationProfileAnswer>)> {
        if let Some(answer) =
            self.indexed_relation_profile_answer(actor, subject, max_actions, max_evidence_bytes)?
        {
            return Ok(answer);
        }

        // A cold or stale derived index is rebuilt from the authenticated
        // snapshot once, then the profile is retried against one matched revision.
        let memory = self.store.load_memory(actor)?;
        self.store
            .rebuild_relation_index_if_current(actor, &memory)?;
        if let Some(answer) =
            self.indexed_relation_profile_answer(actor, subject, max_actions, max_evidence_bytes)?
        {
            return Ok(answer);
        }

        // If a concurrent writer moves the head during rebuild, the loaded
        // immutable snapshot remains the safe fallback for this request.
        let answer = relation_profile_answer(&memory, subject, max_actions, max_evidence_bytes);
        Ok((memory.revision, answer))
    }

    fn indexed_premise_inventory(
        &self,
        actor: &str,
        subject: &str,
        max_actions: u32,
    ) -> Result<Option<(u64, Result<relations::RelationInventory>)>> {
        indexed_premise_inventory_from_store(&self.store, actor, subject, max_actions)
    }

    fn active_premise_inventory(
        &self,
        actor: &str,
        subject: &str,
        max_actions: u32,
    ) -> Result<(u64, Result<relations::RelationInventory>)> {
        if let Some(inventory) = self.indexed_premise_inventory(actor, subject, max_actions)? {
            return Ok(inventory);
        }

        // Rebuild a cold or stale derived index once from the authenticated
        // snapshot, then retry against one revision-matched transaction.
        let memory = self.store.load_memory(actor)?;
        self.store
            .rebuild_relation_index_if_current(actor, &memory)?;
        if let Some(inventory) = self.indexed_premise_inventory(actor, subject, max_actions)? {
            return Ok(inventory);
        }

        // A concurrent writer may advance the head during rebuild. Preserve
        // this request's immutable snapshot as its safe fallback.
        let inventory = relations::inventory_about(&memory, subject, max_actions);
        Ok((memory.revision, inventory))
    }

    fn indexed_relation_comparison_inventories(
        &self,
        actor: &str,
        subject_a: &str,
        subject_b: &str,
        scope: &str,
        per_subject_actions: u32,
    ) -> Result<Option<RelationComparisonInventories>> {
        self.store.with_relation_index(actor, |index| {
            let indexed_a = relation_comparison_inventory_from_index(
                index,
                subject_a,
                scope,
                per_subject_actions,
            );
            let indexed_b = relation_comparison_inventory_from_index(
                index,
                subject_b,
                scope,
                per_subject_actions,
            );
            let mut comparison_rules = BTreeMap::new();
            if let (Ok((_, rules_a)), Ok((_, rules_b))) = (&indexed_a, &indexed_b) {
                for rule in rules_a.iter().chain(rules_b) {
                    comparison_rules.insert(rule.id.clone(), rule.clone());
                }
            }
            Ok(RelationComparisonInventories {
                revision: index.revision(),
                inventory_a: indexed_a.map(|(inventory, _rules)| inventory),
                inventory_b: indexed_b.map(|(inventory, _rules)| inventory),
                comparison_rules: comparison_rules.into_values().collect(),
            })
        })
    }

    fn active_relation_comparison_inventories(
        &self,
        actor: &str,
        subject_a: &str,
        subject_b: &str,
        scope: &str,
        per_subject_actions: u32,
    ) -> Result<RelationComparisonInventories> {
        if let Some(inventories) = self.indexed_relation_comparison_inventories(
            actor,
            subject_a,
            subject_b,
            scope,
            per_subject_actions,
        )? {
            return Ok(inventories);
        }

        // Rebuild a cold or stale derived index once from the authenticated
        // snapshot, then retry both subjects against one indexed revision.
        let memory = self.store.load_memory(actor)?;
        self.store
            .rebuild_relation_index_if_current(actor, &memory)?;
        if let Some(inventories) = self.indexed_relation_comparison_inventories(
            actor,
            subject_a,
            subject_b,
            scope,
            per_subject_actions,
        )? {
            return Ok(inventories);
        }

        // A concurrent head advance preserves this request's immutable source
        // snapshot as the safe fallback for both inventories.
        let inventory_a =
            relation_comparison_inventory(&memory, subject_a, scope, per_subject_actions);
        let inventory_b =
            relation_comparison_inventory(&memory, subject_b, scope, per_subject_actions);
        let comparison_rules = match (&inventory_a, &inventory_b) {
            (Ok(inventory_a), Ok(inventory_b)) => {
                relation_comparison_supporting_rules(&memory.rules, inventory_a, inventory_b)
            }
            _ => Vec::new(),
        };
        Ok(RelationComparisonInventories {
            revision: memory.revision,
            inventory_a,
            inventory_b,
            comparison_rules,
        })
    }

    fn active_conjunction_relation_queries(
        &self,
        actor: &str,
        propositions: &[Proposition],
        per_relation_budget: u32,
    ) -> Result<ConjunctionRelationQueries> {
        if let Some(queries) =
            self.indexed_conjunction_relation_queries(actor, propositions, per_relation_budget)?
        {
            return Ok(queries);
        }

        // Rebuild a cold or stale derived index from the authenticated current
        // snapshot once, then retry all clauses against one indexed revision.
        let memory = self.store.load_memory(actor)?;
        self.store
            .rebuild_relation_index_if_current(actor, &memory)?;
        if let Some(queries) =
            self.indexed_conjunction_relation_queries(actor, propositions, per_relation_budget)?
        {
            return Ok(queries);
        }

        // A concurrent writer may advance the head during rebuild. Keep the
        // loaded immutable snapshot as the safe, bounded fallback for all
        // clauses in this request.
        let outcomes = propositions
            .iter()
            .map(|proposition| {
                full_snapshot_relation_evaluation(&memory, proposition, per_relation_budget)
            })
            .collect();
        Ok(ConjunctionRelationQueries {
            memory_revision: memory.revision,
            outcomes,
        })
    }

    fn active_relation_query(
        &self,
        actor: &str,
        proposition: &Proposition,
        max_actions: u32,
    ) -> Result<(MemoryState, relations::QueryOutcome)> {
        if let Some(result) = self.indexed_relation_query(actor, proposition, max_actions)? {
            return Ok(result);
        }

        // Older stores may predate the derived index. Rebuild it from the
        // authenticated current snapshot once, then use the bounded query.
        let memory = self.store.load_memory(actor)?;
        self.store
            .rebuild_relation_index_if_current(actor, &memory)?;
        if let Some(result) = self.indexed_relation_query(actor, proposition, max_actions)? {
            return Ok(result);
        }

        // A concurrent writer may have advanced the source while rebuilding;
        // the immutable snapshot loaded above remains a safe fallback.
        let answer = relations::query(&memory, proposition, max_actions)?;
        Ok((memory, answer))
    }

    fn indexed_previous_relation_query(
        &self,
        actor: &str,
        previous: &ResultRecord,
        proposition: &Proposition,
        explicit_scope: Option<&str>,
        max_actions: u32,
    ) -> Result<Option<PreviousRelationQuery>> {
        indexed_previous_relation_query_from_store(
            &self.store,
            actor,
            previous,
            proposition,
            explicit_scope,
            max_actions,
        )
    }

    fn active_previous_relation_query(
        &self,
        actor: &str,
        previous: &ResultRecord,
        proposition: &Proposition,
        explicit_scope: Option<&str>,
        max_actions: u32,
    ) -> Result<PreviousRelationQuery> {
        if let Some(result) = self.indexed_previous_relation_query(
            actor,
            previous,
            proposition,
            explicit_scope,
            max_actions,
        )? {
            return Ok(result);
        }

        // Rebuild an older index from its authenticated current snapshot once.
        let memory = self.store.load_memory(actor)?;
        self.store
            .rebuild_relation_index_if_current(actor, &memory)?;
        if let Some(result) = self.indexed_previous_relation_query(
            actor,
            previous,
            proposition,
            explicit_scope,
            max_actions,
        )? {
            return Ok(result);
        }

        // If a concurrent writer advances the head during rebuild, keep the
        // loaded immutable snapshot as the safe fallback for this request.
        let resolution =
            resolve_previous_relation_proposition(&memory, previous, proposition, explicit_scope);
        Ok(PreviousRelationQuery::from_snapshot(
            memory,
            resolution,
            max_actions,
        ))
    }

    fn indexed_relation_objects_answer(
        &self,
        actor: &str,
        replay: RelationObjectsReplay<'_>,
    ) -> Result<Option<(MemoryState, Result<RelationObjectsAnswer>)>> {
        indexed_relation_objects_answer_from_store(&self.store, actor, replay)
    }

    fn active_relation_objects_answer(
        &self,
        actor: &str,
        replay: RelationObjectsReplay<'_>,
    ) -> Result<(MemoryState, Result<RelationObjectsAnswer>)> {
        if let Some(result) = self.indexed_relation_objects_answer(actor, replay)? {
            return Ok(result);
        }

        // Older stores may predate the derived index. Rebuild it from the
        // authenticated current snapshot once, then use the bounded query.
        let memory = self.store.load_memory(actor)?;
        self.store
            .rebuild_relation_index_if_current(actor, &memory)?;
        if let Some(result) = self.indexed_relation_objects_answer(actor, replay)? {
            return Ok(result);
        }

        // A concurrent writer may have advanced the source while rebuilding;
        // the immutable snapshot loaded above remains a safe fallback.
        let answer = relation_objects_answer(
            &memory,
            replay.subject,
            replay.predicate,
            replay.negative,
            replay.scope,
            replay.max_actions,
            replay.max_evidence_bytes,
        );
        Ok((memory, answer))
    }

    pub fn execute(&mut self, actor: &str, request: RequestEnvelope) -> Result<ResultRecord> {
        self.execute_with_bundle(actor, request, None)
    }

    #[cfg(test)]
    pub(crate) fn execute_legacy_calculation_fixture(
        &mut self,
        actor: &str,
        request: RequestEnvelope,
    ) -> Result<ResultRecord> {
        let mut bundle = input::seal(&request, None)?;
        assert!(matches!(bundle.meaning.intent, Intent::Calculate { .. }));
        bundle.meaning.parser_version = input::PARSER_VERSION.into();
        bundle.semantic_signature = identity(&bundle.meaning)?;
        bundle.fingerprint = identity(&(
            &request,
            &bundle.raw_sha256,
            &bundle.context_sha256,
            input::PARSER_VERSION,
        ))?;
        self.execute_with_bundle(actor, request, Some(bundle))
    }

    fn execute_with_bundle(
        &mut self,
        actor: &str,
        request: RequestEnvelope,
        direct_bundle: Option<InputBundle>,
    ) -> Result<ResultRecord> {
        let attempt = self.execute_inner(actor, request.clone(), direct_bundle);
        match attempt {
            Ok(value) => Ok(value),
            Err(error) => {
                if (read_case_changed(&error)
                    || matches!(&error, Error::Contract { code, .. } if code == "REVISION_CONFLICT"))
                    && actor == request.actor_id
                    && let Some(saved) = self.store.get_input_result(
                        actor,
                        &request.case_id,
                        &request.input_id,
                        request.input_version,
                    )?
                    && saved.lifecycle == Lifecycle::Cancel
                    && saved.id == identity(&request)?
                    && saved.input_id == request.input_id
                    && saved.input_version == request.input_version
                {
                    return Ok(saved);
                }
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
    fn execute_inner(
        &mut self,
        actor: &str,
        request: RequestEnvelope,
        direct_bundle: Option<InputBundle>,
    ) -> Result<ResultRecord> {
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
        let stop_admission = |code: &str, message: &str| {
            let mut result = rejected(&request, code, message);
            result.lifecycle = Lifecycle::Suspend;
            self.store.record_admission_stop(actor, &request, &result)?;
            Ok(result)
        };
        let mut early_bundle = None;
        let pending = latest.as_ref().is_some_and(|s| {
            matches!(
                s.lifecycle,
                Lifecycle::Received | Lifecycle::Sealed | Lifecycle::Running | Lifecycle::Adopted
            )
        });
        if pending
            && direct_bundle.is_none()
            && let Ok(interpreted) =
                input::seal(&request, latest.as_ref().and_then(|s| s.result.as_ref()))
            && matches!(interpreted.meaning.intent, Intent::Cancel)
        {
            let mut old = latest.take().expect("pending case exists");
            self.cancel_prior_input(
                &mut old,
                format!("AUTHORIZED_CANCEL_REQUEST:{fingerprint}; past effects retained"),
            )?;
            latest = Some(old);
            early_bundle = Some(interpreted);
        }
        if pending && early_bundle.is_none() {
            return stop_admission(
                "CASE_BUSY_OR_RECOVERY_REQUIRED",
                "未確定のケースがあります。既存作用を照合してから再開してください。",
            );
        }
        let blocked_effect = if let Some(current) = latest.as_ref() {
            let actions = self.store.actions(actor, &request.case_id)?;
            if actions
                .iter()
                .any(|action| matches!(action.phase, EffectPhase::Unknown | EffectPhase::Attempted))
            {
                Some((
                    "EXTERNAL_EFFECT_UNKNOWN",
                    "既存作用の結果が未確定です。同一identityを再照合するまで新しい実行を開始しません。",
                    "CANCELLED_WITH_ORIGINAL_UNKNOWN_EFFECT_ACCOUNTING",
                ))
            } else if current.lifecycle == Lifecycle::Suspend
                && actions.iter().any(|action| {
                    current.input_fingerprint.as_ref() == Some(&action.input_fingerprint)
                        && crate::recovery::is_unattempted_local_write(action)
                })
            {
                Some((
                    "EFFECT_PREPARATION_UNRESOLVED",
                    "前回の書き込みは試行前で停止しています。取り消してから新しい実行を開始してください。",
                    "CANCELLED_WITH_ORIGINAL_PREPARATION_ACCOUNTING",
                ))
            } else {
                None
            }
        } else {
            None
        };
        if let Some((code, message, cancel_reason)) = blocked_effect {
            if direct_bundle.is_some() {
                return stop_admission(code, message);
            }
            let candidate = early_bundle.take().map_or_else(
                || input::seal(&request, latest.as_ref().and_then(|s| s.result.as_ref())),
                Ok,
            );
            if let Ok(candidate) = candidate
                && matches!(candidate.meaning.intent, Intent::Cancel)
            {
                if latest.as_ref().is_some_and(|old| {
                    old.lifecycle == Lifecycle::Suspend && old.goal_sha256.is_some()
                }) {
                    let mut old = latest.take().expect("suspended case exists");
                    self.advance(
                        &mut old,
                        Lifecycle::Running,
                        Some(format!("AUTHORIZED_CANCEL_REQUEST:{fingerprint}")),
                    )?;
                    self.cancel_prior_input(&mut old, cancel_reason.into())?;
                    latest = Some(old);
                }
                early_bundle = Some(candidate);
            } else {
                return stop_admission(code, message);
            }
        }
        if let Some(cancelled) = latest
            .as_ref()
            .filter(|prior| prior.lifecycle == Lifecycle::Cancel)
        {
            crate::recovery::close_cancelled_file_preparations(&self.store, cancelled)?;
        }
        let implicit_context = request.context_ref.is_none() && direct_bundle.is_none();
        let mut previous = if let Some(ref requested) = request.context_ref {
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
        if let Err(error) = self.store.append_case(actor, &state) {
            // An earlier cancellation may have already changed the predecessor.
            if early_bundle.is_none()
                && matches!(&error, Error::Contract { code, .. } if code == "REVISION_CONFLICT")
                && let Some(current) = self.store.load_case(actor, &request.case_id)?
                && current.revision >= state.revision
                && current
                    .input_fingerprint
                    .as_deref()
                    .is_some_and(|accepted| accepted != fingerprint)
            {
                return stop_admission(
                    "CASE_CHANGED_BEFORE_ADMISSION",
                    &format!(
                        "別入力の会話記録が先に進みました。競合を確認した保存版は {} です。この入力は実行されていません。状態を確認してから、新しい入力IDで送ってください。",
                        current.revision,
                    ),
                );
            }
            return Err(error);
        }
        let started = Instant::now();
        let has_compute_budget = request.budget.max_actions > 0 && request.budget.deadline_ms > 0;
        let sealed = if let Some(bundle) = early_bundle {
            Ok(bundle)
        } else if let Some(bundle) = direct_bundle {
            Ok(bundle)
        } else {
            let implicit_source_context = latest
                .as_ref()
                .filter(|case| {
                    implicit_context && has_compute_budget && case.lifecycle == Lifecycle::Commit
                })
                .and_then(|case| case.bundle.as_ref())
                .and_then(|bundle| match &bundle.meaning.intent {
                    Intent::ExplainPrevious
                        if bundle.meaning.parser_version
                            == input::DIRECT_SOURCE_EXPLANATION_PARSER_VERSION =>
                    {
                        Some(input::SourceContext::ExplainedDirectSource)
                    }
                    Intent::ExplainPrevious
                        if bundle.meaning.parser_version
                            == input::WEB_ANSWER_EXPLANATION_PARSER_VERSION =>
                    {
                        Some(input::SourceContext::WebAnswerExplanation)
                    }
                    Intent::ExplainPrevious => Some(input::SourceContext::Explanation),
                    _ if input::is_web_body_question_input(bundle) => {
                        Some(input::SourceContext::WebBodyQuestion)
                    }
                    intent if input::is_source_summary_intent(intent) => {
                        Some(input::SourceContext::Summary)
                    }
                    _ => None,
                });
            input::seal_with_source_context(&request, previous, implicit_source_context)
        };
        let mut resumed_summary_scope = None;
        let mut explicit_summary_question_scope = None;
        let sealed = sealed.and_then(|bundle| {
            if has_compute_budget
                && matches!(bundle.meaning.intent, Intent::AskPreviousRelation { .. })
                && let Some(prior) = previous
                && previous_assertion_context(
                    &self.store,
                    actor,
                    &request.case_id,
                    &state.prior_results,
                    prior,
                )?
                .is_some_and(|assertion| assertion.context.is_some())
            {
                return input::seal_with_source_context(
                    &request,
                    previous,
                    Some(input::SourceContext::ContextualAssertionRelationQuery),
                );
            }
            if has_compute_budget
                && matches!(bundle.meaning.intent, Intent::AskPreviousRelation { .. })
                && let Some(prior) = previous
                && let Some(source) =
                    previous_natural_correction_input(&self.store, actor, &request.case_id, prior)?
            {
                let context = if matches!(source.meaning.intent, Intent::CorrectPremise { .. }) {
                    input::SourceContext::NamedCorrectionRelationQuery
                } else {
                    input::SourceContext::ContextualCorrectionRelationQuery
                };
                return input::seal_with_source_context(&request, previous, Some(context));
            }
            if has_compute_budget
                && matches!(bundle.meaning.intent, Intent::AssertPreviousRelation { .. })
                && let Some(prior) = previous
                && let Some(context) =
                    correction_assertion_source(&self.store, actor, &request.case_id, prior)?
            {
                return input::seal_with_source_context(&request, previous, Some(context));
            }
            if has_compute_budget
                && matches!(bundle.meaning.intent, Intent::AskPreviousRelation { .. })
                && let Some(prior) = previous
                && previous_named_derived_relation_has_single_subject_and_multiple_premises(
                    &self.store,
                    actor,
                    &request.case_id,
                    prior,
                )?
            {
                return input::seal_with_source_context(
                    &request,
                    previous,
                    Some(input::SourceContext::NamedDerivedRelationQuery),
                );
            }
            if has_compute_budget
                && matches!(bundle.meaning.intent, Intent::AskPreviousRelation { .. })
                && let Some(prior) = previous
                && previous_derived_followup_question(&self.store, actor, &request.case_id, prior)?
                    .is_some()
            {
                return input::seal_with_source_context(
                    &request,
                    previous,
                    Some(input::SourceContext::DerivedRelationFollowup),
                );
            }
            if has_compute_budget
                && matches!(
                    bundle.meaning.intent,
                    Intent::FetchPreviousSearchCandidate { .. }
                )
                && previous.is_some_and(search::is_partial_web_candidate_context)
            {
                return input::seal_with_source_context(
                    &request,
                    previous,
                    Some(input::SourceContext::PartialWebCandidate),
                );
            }
            if has_compute_budget
                && matches!(bundle.meaning.intent, Intent::ConvertPreviousLength { .. })
                && let Some(prior) = previous.filter(|result| {
                    result.lifecycle == Lifecycle::Commit
                        && result.guarantee == Guarantee::SourceReport
                })
                && self
                    .store
                    .get_input_bundle(
                        actor,
                        &request.case_id,
                        &prior.input_id,
                        prior.input_version,
                    )?
                    .is_some_and(|(_, source)| search::is_web_elevation_query(&source))
            {
                return input::seal_with_source_context(
                    &request,
                    previous,
                    Some(input::SourceContext::PreviousWebLength),
                );
            }
            if has_compute_budget
                && matches!(bundle.meaning.intent, Intent::AskPreviousGroupMean)
                && let Some(prior) = previous.filter(|result| {
                    result.lifecycle == Lifecycle::Commit && result.guarantee == Guarantee::Computed
                })
                && self
                    .store
                    .get_input_bundle(
                        actor,
                        &request.case_id,
                        &prior.input_id,
                        prior.input_version,
                    )?
                    .is_some_and(|(_, source)| {
                        matches!(source.meaning.intent, Intent::CsvGroupStatistics { .. })
                    })
            {
                return input::seal_with_source_context(
                    &request,
                    previous,
                    Some(input::SourceContext::PreviousCsvGroupMean),
                );
            }
            if has_compute_budget
                && matches!(bundle.meaning.intent, Intent::ConvertPreviousJsonTableToCsv)
                && let Some(prior) = previous.filter(|result| {
                    result.lifecycle == Lifecycle::Commit && result.guarantee == Guarantee::Computed
                })
                && let Some(source_context) = self
                    .store
                    .get_input_bundle(
                        actor,
                        &request.case_id,
                        &prior.input_id,
                        prior.input_version,
                    )?
                    .and_then(|(_, source)| match source.meaning.intent {
                        Intent::CsvFilter { .. } => {
                            Some(input::SourceContext::PreviousFilteredCsvToCsv)
                        }
                        Intent::JsonTableFilter { .. } => {
                            Some(input::SourceContext::PreviousFilteredJsonToCsv)
                        }
                        _ => None,
                    })
            {
                return input::seal_with_source_context(&request, previous, Some(source_context));
            }
            if has_compute_budget
                && matches!(bundle.meaning.intent, Intent::ExplainPrevious)
                && let Some(context) = previous.filter(|result| {
                    result.lifecycle == Lifecycle::Commit
                        && matches!(
                            result.guarantee,
                            Guarantee::Computed | Guarantee::SourceReport | Guarantee::UserPremise
                        )
                })
                && let Some(source_context) = self
                    .store
                    .get_input_bundle(
                        actor,
                        &request.case_id,
                        &context.input_id,
                        context.input_version,
                    )?
                    .and_then(|(_, source)| match source.meaning.intent {
                        Intent::CorrectPremise { .. } | Intent::CorrectPreviousPremise { .. }
                            if is_natural_correction(&source)
                                && context.guarantee == Guarantee::UserPremise =>
                        {
                            Some(input::SourceContext::PreviousCorrectionExplanation)
                        }
                        Intent::VerifyCode { .. } if context.guarantee == Guarantee::Computed => {
                            Some(input::SourceContext::DirectCodeVerificationExplanation)
                        }
                        Intent::VerifyPreviousCode { .. }
                            if source.meaning.parser_version
                                == input::EXPLAINED_CODE_SINGLE_CASE_PARSER_VERSION
                                && context.guarantee == Guarantee::Computed =>
                        {
                            Some(input::SourceContext::ExplainedCodeVerificationExplanation)
                        }
                        Intent::TextReplace { .. }
                            if context.guarantee == Guarantee::Computed
                                && context.evidence.is_empty() =>
                        {
                            Some(input::SourceContext::PastedTextReplacementExplanation)
                        }
                        Intent::ConvertPreviousJsonTableToCsv
                            if source.meaning.parser_version
                                == input::PREVIOUS_JSON_TABLE_TO_CSV_PARSER_VERSION
                                && context.guarantee == Guarantee::Computed =>
                        {
                            Some(input::SourceContext::PreviousJsonTableConversionExplanation)
                        }
                        Intent::ConvertPreviousJsonTableToCsv
                            if source.meaning.parser_version
                                == input::PREVIOUS_FILTERED_CSV_TO_CSV_PARSER_VERSION
                                && context.guarantee == Guarantee::Computed =>
                        {
                            Some(input::SourceContext::PreviousFilteredCsvConversionExplanation)
                        }
                        Intent::Retrieve { .. }
                            if context.guarantee == Guarantee::SourceReport
                                && context
                                    .evidence
                                    .iter()
                                    .any(|p| p.packet_kind == "FETCHED_BODY") =>
                        {
                            Some(input::SourceContext::DirectSourceExplanation)
                        }
                        _ => None,
                    })
            {
                return input::seal_with_source_context(&request, previous, Some(source_context));
            }
            if has_compute_budget
                && request.context_ref.is_some()
                && matches!(
                    bundle.meaning.intent,
                    Intent::AskPreviousSourceQuestion { .. }
                        | Intent::SummarizePreviousSource
                        | Intent::SummarizePreviousSourceConstrained { .. }
                        | Intent::SummarizePreviousSourceLines { .. }
                        | Intent::SummarizePreviousSourcePacketLines { .. }
                )
                && let Some(context) = previous.filter(|result| {
                    result.lifecycle == Lifecycle::Commit
                        && matches!(
                            result.guarantee,
                            Guarantee::SourceReport | Guarantee::Hypothesis
                        )
                        && result.evidence.is_empty()
                })
            {
                let classify_and_prepare = |history: &VerifiedCaseHistory<'_>| {
                    let Some((_, source)) =
                        history.input_bundle(&context.input_id, context.input_version)?
                    else {
                        return Ok(None);
                    };
                    let source_context = match context.guarantee {
                        Guarantee::SourceReport
                            if matches!(source.meaning.intent, Intent::ExplainPrevious) =>
                        {
                            match source.meaning.parser_version.as_str() {
                                input::WEB_ANSWER_EXPLANATION_PARSER_VERSION => {
                                    Some(input::SourceContext::WebAnswerExplanation)
                                }
                                input::DIRECT_SOURCE_EXPLANATION_PARSER_VERSION => {
                                    Some(input::SourceContext::ExplainedDirectSource)
                                }
                                _ => None,
                            }
                        }
                        Guarantee::Hypothesis if input::is_web_body_question_input(&source) => {
                            Some(input::SourceContext::WebBodyQuestion)
                        }
                        Guarantee::Hypothesis if input::is_direct_body_question_input(&source) => {
                            Some(input::SourceContext::DirectBodyQuestion)
                        }
                        Guarantee::Hypothesis
                            if matches!(
                                bundle.meaning.intent,
                                Intent::AskPreviousSourceQuestion { .. }
                            ) && input::is_source_summary_intent(&source.meaning.intent) =>
                        {
                            Some(input::SourceContext::Summary)
                        }
                        Guarantee::Hypothesis => crate::text_summary::legacy_body_question_source(
                            &self.store,
                            &source,
                            &state.prior_results,
                            Some(history),
                        )?
                        .and_then(|(_, original)| match original.meaning.intent {
                            Intent::AskWeb { .. } => Some(input::SourceContext::WebBodyQuestion),
                            Intent::Retrieve { .. } => {
                                Some(input::SourceContext::DirectBodyQuestion)
                            }
                            _ => None,
                        }),
                        _ => None,
                    };
                    let Some(source_context) = source_context else {
                        return Ok(None);
                    };
                    let sealed =
                        input::seal_with_source_context(&request, previous, Some(source_context))?;
                    // A typed summary question can use the same verified
                    // history for routing and replaying the source proof.
                    // Process its outcome only after SEALED, as before.
                    if sealed.meaning.parser_version
                        == input::EXPLICIT_SOURCE_SUMMARY_QUESTION_PARSER_VERSION
                        && let Some(prior_case) = latest.as_ref()
                    {
                        explicit_summary_question_scope =
                            Some(crate::text_summary::source_followup_context_with_history(
                                &self.store,
                                &sealed,
                                &prior_case.prior_results,
                                context,
                                Some(history),
                            ));
                    }
                    Ok(Some(sealed))
                };
                if let Some(sealed) = self.store.with_verified_case_history(
                    actor,
                    &request.case_id,
                    classify_and_prepare,
                )? {
                    return Ok(sealed);
                }
            }
            if has_compute_budget
                && matches!(
                    bundle.meaning.parser_version.as_str(),
                    input::EXPLICIT_EXPLANATION_LINE_SUMMARY_PARSER_VERSION
                        | input::EXPLICIT_EXPLANATION_PACKET_LINE_SUMMARY_PARSER_VERSION
                )
                && let Some(context) = previous
                && self
                    .store
                    .get_input_bundle(
                        actor,
                        &request.case_id,
                        &context.input_id,
                        context.input_version,
                    )?
                    .is_some_and(|(_, source)| {
                        input::is_source_summary_intent(&source.meaning.intent)
                    })
            {
                return input::seal_with_source_context(
                    &request,
                    previous,
                    Some(input::SourceContext::Summary),
                );
            }
            if input::uses_implicit_source_context(&bundle.meaning.parser_version)
                || matches!(
                    bundle.meaning.parser_version.as_str(),
                    input::WEB_ANSWER_EXPLANATION_FOLLOWUP_PARSER_VERSION
                        | input::DIRECT_SOURCE_EXPLANATION_FOLLOWUP_PARSER_VERSION
                )
            {
                // Preserve the derived result itself before resolving its source.
                return Ok(bundle);
            }
            let Some(prior_case) = latest.as_ref().filter(|_| implicit_context) else {
                return Ok(bundle);
            };
            if has_compute_budget
                && matches!(
                    bundle.meaning.intent,
                    Intent::AskPreviousSourceQuestion { .. }
                )
                && let Some(scope) =
                    crate::recovery::resume_summary_question_scope(&self.store, prior_case)?
            {
                previous = Some(scope.base_context);
                let sealed = input::seal_with_source_context(
                    &request,
                    previous,
                    Some(input::SourceContext::Summary),
                )?;
                if !scope.matches_resumed_summary_question(&sealed)? {
                    return Err(failure(
                        "SOURCE_CONTEXT_INVALID",
                        "The resumed question does not preserve the verified summary scope",
                    ));
                }
                resumed_summary_scope = Some(scope);
                return Ok(sealed);
            }
            let requested_rank = match bundle.meaning.intent {
                Intent::FetchPreviousSearchCandidate { rank } => Some(rank),
                Intent::AskPreviousSourceQuestion { .. }
                | Intent::SummarizePreviousSource
                | Intent::SummarizePreviousSourceConstrained { .. }
                | Intent::SummarizePreviousSourceLines { .. }
                | Intent::SummarizePreviousSourcePacketLines { .. } => None,
                _ => return Ok(bundle),
            };
            let context = if has_compute_budget
                && let Some(source) = crate::recovery::source_followup_anchor(
                    &self.store,
                    prior_case,
                    requested_rank.is_none(),
                )?
                && (requested_rank.is_none()
                    || source
                        .evidence
                        .iter()
                        .any(|packet| packet.packet_kind == "SEARCH_LIST"))
            {
                if requested_rank.is_none()
                    && crate::recovery::is_web_body_question_no_match(prior_case)
                {
                    previous = Some(source);
                    return input::seal_with_source_context(
                        &request,
                        previous,
                        Some(input::SourceContext::ResumedWebAnswerExplanation),
                    );
                }
                Some(source)
            } else {
                search::committed_context_after_candidate_failure(
                    &self.store,
                    prior_case,
                    requested_rank,
                )?
            };
            if let Some(context) = context {
                previous = Some(context);
                if has_compute_budget
                    && matches!(
                        bundle.meaning.intent,
                        Intent::AskPreviousSourceQuestion { .. }
                    )
                    && context.lifecycle == Lifecycle::Commit
                    && context.guarantee == Guarantee::Hypothesis
                    && context.evidence.is_empty()
                    && self
                        .store
                        .get_input_bundle(
                            actor,
                            &request.case_id,
                            &context.input_id,
                            context.input_version,
                        )?
                        .is_some_and(|(_, source)| {
                            input::is_source_summary_intent(&source.meaning.intent)
                        })
                {
                    return input::seal_with_source_context(
                        &request,
                        previous,
                        Some(input::SourceContext::Summary),
                    );
                }
                return input::seal(&request, previous);
            }
            Ok(bundle)
        });
        let bundle = match sealed {
            Ok(v) => v,
            Err(e) => {
                state.result = Some(base_result(&request));
                return self.stop(&mut state, Lifecycle::Fail, "INPUT_FAILURE", &e.to_string());
            }
        };
        state.bundle = Some(bundle.clone());
        self.advance(&mut state, Lifecycle::Sealed, None)?;
        if let Some(reason) = bundle
            .meaning
            .unresolved
            .iter()
            .find(|reason| reason.starts_with("source role indexing stopped:"))
        {
            return self.stop(
                &mut state,
                Lifecycle::Suspend,
                "SOURCE_SPAN_INDEX_LIMIT",
                reason,
            );
        }
        let previous_source = if resumed_summary_scope.is_some() {
            // The predecessor's exact NO_MATCH and this turn's scope were
            // already authenticated together before sealing this question.
            resumed_summary_scope
        } else if matches!(
            bundle.meaning.intent,
            Intent::AskPreviousSourceQuestion { .. }
                | Intent::SummarizePreviousSource
                | Intent::SummarizePreviousSourceConstrained { .. }
                | Intent::SummarizePreviousSourceLines { .. }
                | Intent::SummarizePreviousSourcePacketLines { .. }
        ) && let Some(source) =
            previous.filter(|source| has_compute_budget || source.lifecycle == Lifecycle::Commit)
            && let Some(prior_case) = latest.as_ref()
        {
            match explicit_summary_question_scope.unwrap_or_else(|| {
                crate::text_summary::source_followup_context(
                    &self.store,
                    &bundle,
                    &prior_case.prior_results,
                    source,
                )
            }) {
                Ok(context) => context,
                Err(Error::Contract { code, message })
                    if matches!(
                        code.as_str(),
                        "AMBIGUOUS_SOURCE_CONTEXT" | "SOURCE_SUMMARY_PACKET_NOT_ACTIVE"
                    ) =>
                {
                    return self.stop(&mut state, Lifecycle::Suspend, &code, &message);
                }
                Err(error) => {
                    return self.stop(
                        &mut state,
                        Lifecycle::Fail,
                        "SOURCE_CONTEXT_INVALID",
                        &error.to_string(),
                    );
                }
            }
        } else {
            None
        };
        let direct_continuation =
            if matches!(bundle.meaning.intent, Intent::FetchNextUnobservedSource)
                && has_compute_budget
            {
                let latest_result = latest.as_ref().and_then(|case| case.result.as_ref());
                let previous = previous.filter(|source| Some(*source) == latest_result);
                match crate::recovery::next_direct_retrieval(
                    &self.store,
                    actor,
                    &bundle,
                    previous,
                    &state.prior_results,
                    None,
                ) {
                    Ok(Some(continuation)) => Some(continuation),
                    Ok(None) => {
                        return self.stop(
                            &mut state,
                            Lifecycle::Suspend,
                            NO_PENDING_DIRECT_URL,
                            NO_PENDING_DIRECT_URL_MESSAGE,
                        );
                    }
                    Err(error) => {
                        return self.stop(
                            &mut state,
                            Lifecycle::Fail,
                            "SOURCE_CONTEXT_INVALID",
                            &error.to_string(),
                        );
                    }
                }
            } else {
                None
            };
        let direct_urls = match &bundle.meaning.intent {
            Intent::Retrieve { urls, .. } => Some(urls.as_slice()),
            Intent::FetchNextUnobservedSource => direct_continuation
                .as_ref()
                .map(|continuation| std::slice::from_ref(&continuation.uri)),
            _ => None,
        };
        let mut nodes = Vec::new();
        let mut compute_requires = Vec::new();
        let source_followup_indices = if matches!(
            bundle.meaning.intent,
            Intent::SummarizePreviousSource
                | Intent::SummarizePreviousSourceConstrained { .. }
                | Intent::SummarizePreviousSourceLines { .. }
                | Intent::SummarizePreviousSourcePacketLines { .. }
        ) {
            previous_source
                .map(|context| context.bodies().map(|(index, _)| index).collect::<Vec<_>>())
                .unwrap_or_default()
        } else {
            Vec::new()
        };
        let previous_sequence = if has_compute_budget
            && matches!(
                bundle.meaning.intent,
                Intent::ExplainPrevious
                    | Intent::ShowCalculationSequenceHistory
                    | Intent::ExplainCalculationSequenceStep { .. }
            )
            && let (Some(prior), Some(prior_case)) = (previous, latest.as_ref())
        {
            match previous_calculation_sequence(
                &self.store,
                &bundle,
                prior,
                &prior_case.prior_results,
                Some(started),
            ) {
                Ok(context) => context,
                Err(error) => {
                    let code = match &error {
                        Error::Contract { code, .. }
                            if matches!(code.as_str(), "DEADLINE" | "ACTION_BUDGET_EXHAUSTED") =>
                        {
                            code.as_str()
                        }
                        _ => "CALCULATION_CONTEXT_INVALID",
                    };
                    return self.stop(&mut state, Lifecycle::Suspend, code, &error.to_string());
                }
            }
        } else {
            None
        };
        let sequence_edit = if has_compute_budget
            && input::sequence_edit_parser(&bundle.meaning.intent).is_some()
        {
            let Some((prior, prior_case)) = previous.zip(latest.as_ref()) else {
                let (code, message) = sequence_edit_context_requirement(&bundle.meaning.intent);
                return self.stop(&mut state, Lifecycle::Suspend, code, message);
            };
            match prepare_sequence_edit(
                &self.store,
                &bundle,
                prior,
                &prior_case.prior_results,
                started,
            ) {
                Ok(context) => Some(context),
                Err(Error::Contract { code, message }) => {
                    return self.stop(&mut state, Lifecycle::Suspend, &code, &message);
                }
                Err(error) => {
                    return self.stop(
                        &mut state,
                        Lifecycle::Suspend,
                        "CALCULATION_CONTEXT_INVALID",
                        &error.to_string(),
                    );
                }
            }
        } else {
            None
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
                            &bundle.meaning.parser_version,
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
        // The verified case head can route an immediate source-question
        // explanation without probing unrelated document and code producers.
        // The source proof must still match the registered prior input.
        let scoped_question_explanation_input = if has_compute_budget
            && matches!(bundle.meaning.intent, Intent::ExplainPrevious)
            && previous.is_some_and(|prior| {
                prior.lifecycle == Lifecycle::Commit && prior.guarantee == Guarantee::Hypothesis
            }) {
            latest
                .as_ref()
                .filter(|case| {
                    case.lifecycle == Lifecycle::Commit && case.result.as_ref() == previous
                })
                .and_then(|case| case.bundle.as_ref())
                .filter(|prior| {
                    input::is_source_summary_question_version(&prior.meaning.parser_version)
                        && matches!(
                            prior.meaning.intent,
                            Intent::AskPreviousSourceQuestion { .. }
                        )
                })
        } else {
            None
        };
        let previous_document_explanation = if scoped_question_explanation_input.is_none()
            && matches!(bundle.meaning.intent, Intent::ExplainPrevious)
        {
            match previous.filter(|prior| prior.lifecycle == Lifecycle::Commit) {
                Some(prior) => {
                    match crate::document::previous_document_explanation(
                        &self.store,
                        &state,
                        prior,
                        &request.budget,
                        &bundle.meaning.parser_version,
                        started,
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
        let csv_group_mean =
            bundle.meaning.parser_version == input::PREVIOUS_CSV_GROUP_MEAN_PARSER_VERSION;
        let [group_mean_source, group_mean_report, group_mean_answer] =
            crate::document::group_mean_graph_ids(csv_group_mean);
        let previous_group_mean_answer =
            if matches!(bundle.meaning.intent, Intent::AskPreviousGroupMean) {
                let deadline =
                    started.checked_add(Duration::from_millis(request.budget.deadline_ms));
                match previous.filter(|prior| prior.lifecycle == Lifecycle::Commit) {
                    Some(prior) => match crate::document::previous_group_mean(
                        &self.store,
                        &bundle,
                        prior,
                        deadline,
                    ) {
                        Ok(value) => value,
                        Err(error) => {
                            let code = match &error {
                                Error::Contract { code, .. } => code.as_str(),
                                _ if csv_group_mean => "CSV_GROUP_MEAN_UNSUPPORTED",
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
                match previous_source.filter(|_| has_compute_budget) {
                    Some(context) => {
                        let prior = context.source;
                        let mut source_packets = Vec::new();
                        for (index, packet) in context.bodies() {
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
                        match crate::text_summary::answer_fetched_source_question(
                            &prior.id,
                            query,
                            &source_packets,
                            request.budget.max_actions as usize,
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
                match crate::document::previous_text_replacement_for_bundle(
                    &self.store,
                    &bundle,
                    &state.prior_results,
                    prior,
                    old,
                    new,
                ) {
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
        let previous_result_summary = if let Some(limits) =
            crate::text_summary::PreviousResultSummaryLimits::from_intent(&bundle.meaning.intent)
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
                limits,
            ) {
                Ok(value) => value,
                Err(error) => {
                    let code = match &error {
                        Error::Contract { code, .. } => code.as_str(),
                        _ => "RESULT_SUMMARY_FAILED",
                    };
                    let message = match (limits, &error) {
                        (
                            crate::text_summary::PreviousResultSummaryLimits::Lines(_),
                            Error::Contract { message, .. },
                        ) => message.clone(),
                        _ => error.to_string(),
                    };
                    return self.stop(&mut state, Lifecycle::Suspend, code, &message);
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
            let output_unit = artifact.output_unit();
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
            if matches!(bundle.meaning.intent, Intent::ExplainPrevious)
                && let Some(prior) = previous
            {
                registered_math_report_explanation(&self.store, actor, &request.case_id, prior)?
            } else {
                None
            };
        let previous_assertion = if matches!(bundle.meaning.intent, Intent::ExplainPrevious)
            && let Some(prior) = previous
        {
            match previous_assertion_context(
                &self.store,
                actor,
                &request.case_id,
                &state.prior_results,
                prior,
            ) {
                Ok(context) => context,
                Err(Error::Contract { code, message }) if code == "PRIOR_INPUT_RESULT_MISMATCH" => {
                    return self.stop(&mut state, Lifecycle::Suspend, &code, &message);
                }
                Err(error) => return Err(error),
            }
        } else {
            None
        };
        let mut prior_explanation_input = scoped_question_explanation_input.cloned();
        let verification_explanation = if has_compute_budget
            && scoped_question_explanation_input.is_none()
            && matches!(bundle.meaning.intent, Intent::ExplainPrevious)
            && let Some(prior) = previous.filter(|prior| prior.lifecycle == Lifecycle::Commit)
        {
            let deadline = started.checked_add(Duration::from_millis(request.budget.deadline_ms));
            let explanation = if bundle.meaning.parser_version
                == input::DIRECT_CODE_VERIFICATION_EXPLANATION_PARSER_VERSION
            {
                crate::code_generation::direct_verification_explanation(
                    &self.store,
                    &state,
                    &bundle,
                    prior,
                    deadline,
                )
                .map(Some)
            } else if bundle.meaning.parser_version
                == input::EXPLAINED_CODE_VERIFICATION_EXPLANATION_PARSER_VERSION
            {
                crate::code_generation::explained_verification_explanation(
                    &self.store,
                    &state,
                    &bundle,
                    prior,
                    deadline,
                )
                .map(Some)
            } else {
                let stored = self.store.get_input_bundle(
                    actor,
                    &request.case_id,
                    &prior.input_id,
                    prior.input_version,
                );
                stored.and_then(|sealed| {
                    prior_explanation_input = sealed.map(|(_, input)| input);
                    crate::code_generation::previous_verification_explanation_from_input(
                        &self.store,
                        &state,
                        &bundle,
                        prior,
                        deadline,
                        prior_explanation_input.as_ref(),
                    )
                })
            };
            match explanation {
                Ok(explanation) => explanation,
                Err(Error::Contract { code, message }) => {
                    return self.stop(&mut state, Lifecycle::Suspend, &code, &message);
                }
                Err(error) => return Err(error),
            }
        } else {
            None
        };
        if direct_continuation.is_some() {
            nodes.push(GoalNode {
                id: PREVIOUS_DIRECT_RETRIEVAL_NODE.into(),
                kind: GoalNodeKind::Evidence,
                requires: Vec::new(),
            });
        }
        if let Some(urls) = direct_urls {
            for (index, _) in urls.iter().enumerate() {
                let id = format!("source-{index}");
                compute_requires.push(id.clone());
                nodes.push(GoalNode {
                    id,
                    kind: GoalNodeKind::Evidence,
                    requires: direct_continuation
                        .is_some()
                        .then(|| PREVIOUS_DIRECT_RETRIEVAL_NODE.into())
                        .into_iter()
                        .collect(),
                });
            }
        }
        if matches!(
            bundle.meaning.intent,
            Intent::Search { .. } | Intent::SearchWithFallback { .. }
        ) {
            let id = search::SEARCH_LISTING_NODE.to_string();
            compute_requires.push(id.clone());
            nodes.push(GoalNode {
                id,
                kind: GoalNodeKind::Evidence,
                requires: Vec::new(),
            });
        }
        if matches!(bundle.meaning.intent, Intent::AskWeb { .. }) {
            nodes.extend(crate::web_answer::nodes());
            compute_requires.push(crate::web_answer::ANSWER_NODE.into());
        }
        if let Intent::FetchPreviousSearchCandidate { rank } = &bundle.meaning.intent {
            let listing_id = search::SEARCH_CANDIDATE_LISTING_NODE.to_string();
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
        if let Some(explanation) = &previous_document_explanation {
            let input_id = explanation.input_node.to_string();
            let report_id = explanation.report_node.to_string();
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
        if previous_group_mean_answer.is_some() {
            let source_id = group_mean_source.to_string();
            let report_id = group_mean_report.to_string();
            let answer_id = group_mean_answer.to_string();
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
        let sequence_steps = sequence_edit
            .as_ref()
            .map(|context| context.steps.as_slice())
            .or_else(|| {
                previous_sequence
                    .as_ref()
                    .map(PreviousCalculationSequence::effective_steps)
            })
            .or(match &bundle.meaning.intent {
                Intent::CalculateSequence { steps } => Some(steps.as_slice()),
                _ => None,
            });
        if let Some(steps) = sequence_steps {
            let sequence_nodes = calculation_sequence_nodes(steps);
            compute_requires.extend(sequence_nodes.iter().map(|node| node.id.clone()));
            nodes.extend(sequence_nodes);
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
        let required_actions = nodes.len()
            + previous_sequence
                .as_ref()
                .map_or(0, PreviousCalculationSequence::context_actions)
            + sequence_edit
                .as_ref()
                .map_or(0, SequenceEdit::verification_actions);
        let mut graph = Scheduler::new(nodes, request.budget.max_actions)?;
        let goal = GoalContract {
            input_fingerprint: fingerprint.clone(),
            intent: bundle.meaning.intent.clone(),
            output_path: request.output_path.clone(),
            permissions: request.permissions.clone(),
            budget: request.budget.clone(),
            required_graph_ref,
            required_predicates: goal_predicates(
                &bundle.meaning,
                request.output_path.is_some(),
                previous_sequence
                    .as_ref()
                    .map(PreviousCalculationSequence::predicate)
                    .or(sequence_edit.as_ref().map(SequenceEdit::predicate))
                    .or(previous_assertion
                        .as_ref()
                        .map(|_| PREVIOUS_ASSERTION_EXPLANATION_PREDICATE))
                    .or(verification_explanation
                        .as_ref()
                        .map(|_| crate::code_generation::VERIFICATION_EXPLANATION_PREDICATE)),
            ),
            renderer_version: Some(output::RENDERER_VERSION.into()),
        };
        state.goal_sha256 = Some(self.store.put_artifact(actor, &request.case_id, &goal)?);
        let mut result = base_result(&request);
        if matches!(
            goal.intent,
            Intent::Retrieve { .. }
                | Intent::FetchNextUnobservedSource
                | Intent::Search { .. }
                | Intent::SearchWithFallback { .. }
                | Intent::FetchPreviousSearchCandidate { .. }
                | Intent::AskWeb { .. }
        ) {
            state.result = Some(result.clone());
        }
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
        if !supports_japanese_output_locale(&request.locale) {
            return self.stop(
                &mut state,
                Lifecycle::Suspend,
                "SEMANTIC_VERIFIER_UNAVAILABLE",
                "この版の表現検証は日本語の限定経路のみ対応しています。",
            );
        }
        if !has_compute_budget {
            return self.stop(
                &mut state,
                Lifecycle::Suspend,
                "RESOURCE_LIMIT",
                NO_COMPUTE_BUDGET,
            );
        }
        if (request.budget.max_actions as usize) < required_actions {
            return self.stop(
                &mut state,
                Lifecycle::Suspend,
                "ACTION_BUDGET_EXHAUSTED",
                &format!("{ACTION_BUDGET_RESIDUALS} {:?}", graph.residuals()),
            );
        }
        // These intents mutate shared actor memory. Refuse a known missing
        // grant before executing them. Read-only results can still retain
        // their computed content when the required file cannot be written.
        if matches!(
            goal.intent,
            Intent::Assert { .. }
                | Intent::AssertPreviousRelation { .. }
                | Intent::CorrectPremise { .. }
                | Intent::CorrectPreviousPremise { .. }
                | Intent::AddRule { .. }
                | Intent::AddConjunctiveRule { .. }
                | Intent::DefineCondition { .. }
                | Intent::Invalidate { .. }
        ) && let Some((code, message)) = required_file_permission_stop(&goal)
        {
            return self.stop(&mut state, Lifecycle::Suspend, code, message);
        }
        // Each explanation probe below consumes the same authenticated prior
        // input. Read it once, without rescanning the journal for each probe.
        let prior_context_input = if matches!(goal.intent, Intent::ExplainPrevious)
            && bundle.meaning.parser_version
                != input::DIRECT_CODE_VERIFICATION_EXPLANATION_PARSER_VERSION
        {
            prior_explanation_input
        } else if matches!(
            goal.intent,
            Intent::ExplainPrevious
                | Intent::AskPreviousRelation { .. }
                | Intent::AssertPreviousRelation { .. }
        ) && bundle.meaning.parser_version
            != input::DERIVED_RELATION_FOLLOWUP_PARSER_VERSION
            && let Some(prior) = previous.filter(|prior| prior.lifecycle == Lifecycle::Commit)
        {
            self.store
                .get_input_bundle(
                    actor,
                    &request.case_id,
                    &prior.input_id,
                    prior.input_version,
                )?
                .map(|(_, input)| input)
        } else {
            None
        };
        let contextual_query_memory = if bundle.meaning.parser_version
            != input::DERIVED_RELATION_FOLLOWUP_PARSER_VERSION
            && let (Some(prior), Some(prior_bundle)) = (previous, prior_context_input.as_ref())
            && input::is_registered_previous_relation_query(&prior_bundle.meaning.parser_version)
        {
            let memory = self.store.load_memory(actor)?;
            match crate::recovery::verify_current_registered_previous_relation_result(
                &self.store,
                &state,
                prior,
                &memory,
                request.budget.max_actions,
                prior_bundle,
            ) {
                Ok(()) => Some(memory),
                Err(Error::Contract { code, message }) if code == "RELATION_BUDGET" => {
                    return self.stop(&mut state, Lifecycle::Suspend, "RESOURCE_LIMIT", &message);
                }
                Err(Error::Contract { .. }) => {
                    return self.stop(
                        &mut state, Lifecycle::Suspend, "CONTEXT_PROOF_CHANGED",
                        "前回の関係回答で使った代名詞の前提と根拠を、現在の記憶で再確認できません。",
                    );
                }
                Err(error) => return Err(error),
            }
        } else {
            None
        };
        let mut units = Vec::new();
        let mut approved_surfaces = None;
        let mut report_goal_content = None;
        let mut adopted_relation_meaning = None;
        let mut relation_proof_dependencies = None;
        let mut relation_meaning_plan = None;
        let content_request = None;
        let content_composition = None;
        let mut content_generation = None;
        let mut generation_proof_ids = None;
        let mut calculation_meaning_plan = None;
        let mut applied_rule_adaptation = None;
        let mut generation_model = None;
        let mut generation_emission = None;
        let mut sequence_result = None;
        let retrieval_budget = |read_nodes, state: &CaseRevision, store: &Store| -> Result<_> {
            // Each live source node reserves one start. Non-read nodes keep
            // their original allowance, including the saved candidate listing.
            let reserved = (required_actions - read_nodes) as u32;
            let mut budget = ports::RetrievalBudget::new(
                started,
                &goal.budget,
                goal.budget.max_actions - reserved,
            )?;
            // The authenticated case contains both its latest result and its
            // older results. A semantic context_ref does not select history
            // for transport admission.
            for result in state.prior_results.values() {
                budget.time_remaining()?;
                for packet in &result.evidence {
                    if !matches!(result.lifecycle, Lifecycle::Suspend | Lifecycle::Commit)
                        && !(result.lifecycle == Lifecycle::Cancel
                            && matches!(
                                packet.packet_kind.as_str(),
                                "SEARCH_LIST" | "FETCH_FAILURE"
                            ))
                    {
                        continue;
                    }
                    budget.restore_retry_after(packet, &goal.permissions, || {
                        verify_retained_retry_source(store, state, result, packet, None)
                    })?;
                }
            }
            Ok(budget)
        };
        let read_admission = |state: &CaseRevision| -> Result<_> {
            let reference = digest(&canonical(state)?);
            let store = &self.store;
            let case = request.case_id.as_str();
            Ok(move || {
                if !store.is_case_head(actor, case, &reference)? {
                    return Err(failure(
                        READ_CASE_CHANGED,
                        "The case changed before the HTTP request could start",
                    ));
                }
                Ok(())
            })
        };
        match &goal.intent {
            Intent::Calculate { expression } => match language::calculate(expression) {
                Ok(calculation) => {
                    result.guarantee = Guarantee::Computed;
                    if goal
                        .required_predicates
                        .iter()
                        .any(|predicate| predicate == GENERATED_CALCULATION_PREDICATE)
                    {
                        let meaning_plan = CalculationMeaningPlan::from_calculation(&calculation)?;
                        let grammar =
                            meaning_plan.generation_request(goal.budget.max_evidence_bytes)?;
                        let deadline = started
                            .checked_add(Duration::from_millis(goal.budget.deadline_ms))
                            .ok_or_else(|| {
                                failure(
                                    "INVALID_DEADLINE",
                                    "execution deadline overflows this clock",
                                )
                            })?;
                        let mut generation =
                            match language::generate_semantic_meaning_until(&grammar, deadline) {
                                Ok(value) => value,
                                Err(error) => {
                                    return self.stop(
                                        &mut state,
                                        Lifecycle::Suspend,
                                        "LANGUAGE_RESOURCE_OR_COVERAGE",
                                        &error.to_string(),
                                    );
                                }
                            };
                        if let Err(error) = CalculationMeaningPlan::validate_generation(
                            &grammar,
                            &generation.report,
                        ) {
                            return self.stop(
                                &mut state,
                                Lifecycle::Suspend,
                                "CONTENT_PLAN_INVALID",
                                &error.to_string(),
                            );
                        }
                        units.extend(generation.report.units.iter().map(|unit| unit.text.clone()));
                        generation.report.model_artifact_ref = Some(self.store.put_artifact(
                            actor,
                            &request.case_id,
                            &generation.model,
                        )?);
                        report_goal_content = Some(ReportGoalContent {
                            predicate: GENERATED_CALCULATION_PREDICATE,
                            guarantee: Guarantee::Computed,
                            dependencies: Vec::new(),
                            units: units.clone(),
                        });
                        calculation_meaning_plan = Some(meaning_plan);
                        content_generation = Some(generation.report);
                        generation_model = Some(generation.model);
                        generation_emission = Some(generation.emission);
                    } else {
                        units.push(exact_calculation_unit(&calculation));
                    }
                    result.calculation = Some(calculation);
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
                let observed = match run_calculation_sequence(
                    steps,
                    |_| &request.text,
                    &mut graph,
                    &goal.budget,
                    started,
                    &mut result.dependencies,
                    |bytes| self.store.put_bytes(actor, &request.case_id, bytes),
                ) {
                    Ok(observed) => observed,
                    Err(Error::Contract { code, message }) => {
                        state.result = Some(result.clone());
                        let lifecycle = if code == "SEQUENCE_DEPENDENCY_MISMATCH" {
                            Lifecycle::Fail
                        } else {
                            Lifecycle::Suspend
                        };
                        return self.stop(&mut state, lifecycle, &code, &message);
                    }
                    Err(error) => return Err(error),
                };
                units.clone_from(&observed.units);
                result.calculation = Some(observed.calculation.clone());
                result.guarantee = Guarantee::Computed;
                sequence_result = Some(observed);
            }
            Intent::CorrectCalculationSequenceStep { .. }
            | Intent::AppendCalculationSequenceStep { .. }
            | Intent::RemoveCalculationSequenceStep { .. }
            | Intent::MoveCalculationSequenceStep { .. }
            | Intent::InsertCalculationSequenceStep { .. } => {
                let context = sequence_edit.as_ref().ok_or_else(|| {
                    let (code, message) = sequence_edit_context_requirement(&bundle.meaning.intent);
                    failure(code, message)
                })?;
                let observed = match run_sequence_edit(
                    &self.store,
                    &bundle,
                    context,
                    &mut graph,
                    started,
                    &mut result.dependencies,
                    |bytes| self.store.put_bytes(actor, &request.case_id, bytes),
                ) {
                    Ok(observed) => observed,
                    Err(Error::Contract { code, message }) => {
                        state.result = Some(result.clone());
                        let lifecycle = if code == "SEQUENCE_DEPENDENCY_MISMATCH" {
                            Lifecycle::Fail
                        } else {
                            Lifecycle::Suspend
                        };
                        return self.stop(&mut state, lifecycle, &code, &message);
                    }
                    Err(error) => return Err(error),
                };
                units.clone_from(&observed.units);
                result.calculation = Some(observed.calculation.clone());
                result.guarantee = Guarantee::Computed;
                sequence_result = Some(observed);
            }
            Intent::ConvertPreviousLength { .. }
                if bundle.meaning.parser_version
                    == input::PREVIOUS_WEB_LENGTH_CONVERSION_PARSER_VERSION =>
            {
                let Some(prior) = previous else {
                    return self.stop(
                        &mut state,
                        Lifecycle::Suspend,
                        "WEB_LENGTH_CONVERSION_CONTEXT_INVALID",
                        "同じ会話に確定した標高回答がありません。",
                    );
                };
                let conversion = match search::previous_web_length_conversion(
                    &self.store,
                    &bundle,
                    prior,
                    started,
                ) {
                    Ok(value) => value,
                    Err(Error::Contract { code, message }) => {
                        return self.stop(&mut state, Lifecycle::Suspend, &code, &message);
                    }
                    Err(error) => return Err(error),
                };
                result.calculation = Some(conversion.calculation);
                result.dependencies = conversion.dependencies;
                result.guarantee = Guarantee::Computed;
                units = conversion.units;
                report_goal_content = Some(ReportGoalContent {
                    predicate: PREVIOUS_WEB_LENGTH_CONVERSION_PREDICATE,
                    guarantee: Guarantee::Computed,
                    dependencies: result.dependencies.clone(),
                    units: units.clone(),
                });
            }
            Intent::CorrectCalculation { .. } | Intent::ConvertPreviousLength { .. } => {
                let correcting = matches!(goal.intent, Intent::CorrectCalculation { .. });
                let Some(prior) = previous.filter(|prior| prior.lifecycle == Lifecycle::Commit)
                else {
                    return self.stop(
                        &mut state,
                        Lifecycle::Suspend,
                        "MISSING_ACTUAL_RESULT",
                        if correcting {
                            "訂正対象となる実際の計算結果がありません。"
                        } else {
                            "換算対象となる実際の計算結果がありません。"
                        },
                    );
                };
                if prior.calculation.is_none() {
                    return self.stop(
                        &mut state,
                        Lifecycle::Suspend,
                        "MISSING_ACTUAL_CALCULATION",
                        "直前の採用済み成果に計算式がありません。",
                    );
                }
                match calculation_from_context(
                    &self.store,
                    &bundle,
                    prior,
                    &state.prior_results,
                    started,
                ) {
                    Ok(calculation) => {
                        units.push(exact_calculation_unit(&calculation));
                        result.dependencies.push(prior.id.clone());
                        result.calculation = Some(calculation);
                        result.guarantee = Guarantee::Computed;
                        report_goal_content = Some(ReportGoalContent {
                            predicate: if correcting {
                                CALCULATION_CORRECTION_PREDICATE
                            } else {
                                PREVIOUS_LENGTH_CONVERSION_PREDICATE
                            },
                            guarantee: Guarantee::Computed,
                            dependencies: result.dependencies.clone(),
                            units: units.clone(),
                        });
                    }
                    Err(Error::Contract { code, message }) => {
                        return self.stop(&mut state, Lifecycle::Suspend, &code, &message);
                    }
                    Err(error) => {
                        return self.stop(
                            &mut state,
                            Lifecycle::Suspend,
                            if correcting {
                                "CORRECTION_UNSUPPORTED"
                            } else {
                                "LENGTH_CONVERSION_UNSUPPORTED"
                            },
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
                let value = CsvInput::conversation(document).select(column)?;
                let artifact = self.store.put_artifact(actor, &request.case_id, &value)?;
                result.dependencies.push(artifact);
                units.push(value.text);
                result.guarantee = Guarantee::Computed;
            }
            Intent::CsvSelectIndex {
                document,
                column_index,
                has_header,
            } => {
                let value = match CsvInput::conversation(document)
                    .select_index(*column_index, *has_header)
                {
                    Ok(value) => value,
                    Err(error) => {
                        let code = match &error {
                            Error::Contract { code, .. } => code.as_str(),
                            _ => "CSV_SELECT_UNSUPPORTED",
                        };
                        return self.stop(&mut state, Lifecycle::Suspend, code, &error.to_string());
                    }
                };
                if value.text.len() > request.budget.max_evidence_bytes {
                    return self.stop(
                        &mut state,
                        Lifecycle::Suspend,
                        "RESOURCE_LIMIT",
                        "CSV positional selection exceeds the request output budget",
                    );
                }
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
                let value = match CsvInput::conversation(document).filter(column, equals) {
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
                let value = match CsvInput::conversation(document).sort(column, *descending) {
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
                let value = match CsvInput::conversation(document).group_count(column) {
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
                let value = match CsvInput::conversation(document)
                    .group_statistics(group_column, value_column)
                {
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
                let value = match CsvInput::conversation(document)
                    .compare_by_parser(key_column, &bundle.meaning.parser_version)
                {
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
                let value = match CsvInput::conversation(document).inner_join_by_parser(
                    left_key_column,
                    right_key_column,
                    &bundle.meaning.parser_version,
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
                let value = match CsvInput::conversation(document).to_json(*has_header) {
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
            Intent::ConvertPreviousJsonTableToCsv => {
                let Some(prior) = previous.filter(|result| result.lifecycle == Lifecycle::Commit)
                else {
                    return self.stop(
                        &mut state,
                        Lifecycle::Suspend,
                        "MISSING_ACTUAL_RESULT",
                        "同じ会話に確定した変換元がありません。",
                    );
                };
                let conversion = match crate::document::previous_json_table_to_csv(
                    &self.store,
                    &bundle,
                    prior,
                ) {
                    Ok(value) => value,
                    Err(Error::Contract { code, message }) => {
                        return self.stop(&mut state, Lifecycle::Suspend, &code, &message);
                    }
                    Err(error) => return Err(error),
                };
                let report_ref =
                    self.store
                        .put_artifact(actor, &request.case_id, &conversion.report)?;
                result
                    .dependencies
                    .extend(conversion.dependencies(report_ref));
                units.push(conversion.report.text);
                result.guarantee = Guarantee::Computed;
            }
            Intent::CsvStatistics { document, column } => {
                let value = match CsvInput::conversation(document).statistics(column) {
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
                let value = if bundle.meaning.parser_version
                    == input::PASTED_TEXT_REPLACEMENT_PARSER_VERSION
                {
                    match crate::document::replace_pasted_text(document, old, new, *count) {
                        Ok(value) => value,
                        Err(error) => {
                            let code = match &error {
                                minidora_domain::Error::Contract { code, .. } => code.as_str(),
                                _ => "TEXT_REPLACEMENT_FAILED",
                            };
                            return self.stop(
                                &mut state,
                                Lifecycle::Suspend,
                                code,
                                &error.to_string(),
                            );
                        }
                    }
                } else {
                    crate::document::replace_exact(document, old, new, *count)?
                };
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
                if canonical(&value)?.len() > request.budget.max_evidence_bytes {
                    return self.stop(
                        &mut state,
                        Lifecycle::Suspend,
                        "RESOURCE_LIMIT",
                        "text composition artifact exceeds request evidence budget",
                    );
                }
                let artifact = self.store.put_artifact(actor, &request.case_id, &value)?;
                result.dependencies.push(artifact);
                units.push(value.text);
                result.guarantee = Guarantee::UserPremise;
            }
            Intent::TextSummaryCandidates { .. } | Intent::SummarizePastedText { .. } => {
                let value = match crate::text_summary::direct_summary_candidates(&bundle) {
                    Ok(value) => value,
                    Err(Error::Contract { code, message })
                        if code == "RESULT_SUMMARY_LINE_COUNT_UNMET" =>
                    {
                        return self.stop(&mut state, Lifecycle::Suspend, &code, &message);
                    }
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
            Intent::AskWeb {
                topic,
                elevation_in_kilometres,
            } => {
                use crate::web_answer::{self, ANSWER_NODE, BODY_NODE, LISTING_NODE};

                let answer = (|| {
                    let mut budget = retrieval_budget(2, &state, &self.store)?;
                    let listed = search::search_with_budget(
                        search::SearchProvider::Wikipedia,
                        topic,
                        &goal.permissions,
                        &goal.budget,
                        &mut budget,
                        &read_admission(&state)?,
                    )?;
                    let observation = listed.observation;
                    let listing_ref =
                        self.store
                            .put_artifact(actor, &request.case_id, &observation)?;
                    result.evidence.push(observation.listing.clone());
                    result.dependencies.push(listing_ref.clone());
                    state.result = Some(result.clone());
                    if let Some(wait) = listed.redirect_wait {
                        return Err(failure(ports::RETRY_DEFERRED, wait));
                    }
                    if let Some(reason) = &observation.failure {
                        return Err(failure("WEB_SEARCH_UNAVAILABLE", reason));
                    }
                    let candidate = web_answer::select_candidate(&observation, &goal.permissions)?;
                    graph.observe(LISTING_NODE, &listing_ref, &BTreeMap::new())?;
                    self.advance(
                        &mut state,
                        Lifecycle::Running,
                        Some("WEB_SEARCH_OBSERVED".into()),
                    )?;

                    let fetched = search::fetch_candidate_with_budget(
                        candidate,
                        &goal.permissions,
                        &mut budget,
                        &read_admission(&state)?,
                    )?;
                    let packet = fetched.observation;
                    let body_ref = self.store.put_artifact(actor, &request.case_id, &packet)?;
                    result.evidence.push(packet.clone());
                    result.dependencies.push(body_ref.clone());
                    state.result = Some(result.clone());
                    if let Some(wait) = fetched.redirect_wait {
                        return Err(failure(ports::RETRY_DEFERRED, wait));
                    }
                    if let Some(reason) = &packet.failure {
                        return Err(failure("WEB_BODY_UNAVAILABLE", reason));
                    }
                    let answer = web_answer::answer(
                        topic,
                        *elevation_in_kilometres,
                        candidate,
                        &packet,
                        &goal.permissions,
                    )?;
                    let answer_units = answer.units();
                    let answer_bytes = canonical(&answer)?;
                    if answer_bytes
                        .len()
                        .saturating_add(answer_units.join("\n").len())
                        > goal.budget.max_evidence_bytes
                    {
                        return Err(failure("RESOURCE_LIMIT", "Web回答が出力予算を超えました。"));
                    }
                    graph.observe(
                        BODY_NODE,
                        &body_ref,
                        &BTreeMap::from([(LISTING_NODE.into(), listing_ref)]),
                    )?;
                    let answer_ref =
                        self.store
                            .put_bytes(actor, &request.case_id, &answer_bytes)?;
                    graph.observe(
                        ANSWER_NODE,
                        &answer_ref,
                        &BTreeMap::from([(BODY_NODE.into(), body_ref)]),
                    )?;
                    result.dependencies.push(answer_ref);
                    Ok(answer_units)
                })();
                units = match answer {
                    Ok(units) => units,
                    Err(error) if read_case_changed(&error) => return Err(error),
                    Err(Error::Contract { code, .. })
                        if code == ports::READ_PERMISSION_REQUIRED =>
                    {
                        return self.stop(
                            &mut state,
                            Lifecycle::Suspend,
                            &code,
                            "許可された接続先の範囲では、Web資料を取得できません。ローカルでの対話は続けられます。",
                        );
                    }
                    Err(Error::Contract { code, message }) => {
                        return self.stop(
                            &mut state,
                            Lifecycle::Suspend,
                            &code,
                            &format!("Web資料を使った回答を保留しました。{message}"),
                        );
                    }
                    Err(error) => return Err(error),
                };
                result.guarantee = Guarantee::SourceReport;
                report_goal_content = Some(ReportGoalContent {
                    predicate: WEB_ANSWER_PREDICATE,
                    guarantee: result.guarantee.clone(),
                    dependencies: result.dependencies.clone(),
                    units: units.clone(),
                });
            }
            Intent::Search { .. } | Intent::SearchWithFallback { .. } => {
                let search_plan = match search::search_plan(&goal.intent) {
                    Ok(Some(plan)) => plan,
                    Err(error) => {
                        return self.stop(
                            &mut state,
                            Lifecycle::Suspend,
                            "SEARCH_UNAVAILABLE",
                            &error.to_string(),
                        );
                    }
                    Ok(None) => unreachable!("search intent was selected"),
                };
                if !graph
                    .ready()
                    .iter()
                    .any(|id| id == search::SEARCH_LISTING_NODE)
                {
                    return self.stop(
                        &mut state,
                        Lifecycle::Suspend,
                        "UNOBSERVED_PREREQUISITE",
                        "検索一覧を取得する実行枠がありません。",
                    );
                }
                let mut budget = match retrieval_budget(1, &state, &self.store) {
                    Ok(budget) => budget,
                    Err(error) => {
                        return self.stop(
                            &mut state,
                            Lifecycle::Suspend,
                            "SEARCH_UNAVAILABLE",
                            &error.to_string(),
                        );
                    }
                };
                let mut provider = search_plan.primary;
                let mut failed_primary = None;
                loop {
                    let read = match search::search_with_budget(
                        provider,
                        search_plan.query,
                        &goal.permissions,
                        &goal.budget,
                        &mut budget,
                        &read_admission(&state)?,
                    ) {
                        Ok(read) => read,
                        Err(error) if read_case_changed(&error) => return Err(error),
                        Err(error) => {
                            state.result = Some(result);
                            return self.stop(
                                &mut state,
                                Lifecycle::Suspend,
                                "SEARCH_UNAVAILABLE",
                                &error.to_string(),
                            );
                        }
                    };
                    let observation = read.observation;
                    let artifact =
                        self.store
                            .put_artifact(actor, &request.case_id, &observation)?;
                    result.dependencies.push(artifact.clone());
                    result.evidence.push(observation.listing.clone());
                    if read.redirect_wait.is_some()
                        || observation.listing.failure.as_deref()
                            == Some(ports::REDIRECT_RETRY_DEFERRED)
                    {
                        return self.suspend_redirect_retry(
                            &mut state,
                            result,
                            read.redirect_wait.as_deref(),
                        );
                    }
                    if let Some(error) = &observation.failure {
                        state.result = Some(result.clone());
                        if let Some(fallback) = search_plan.fallback
                            && failed_primary.is_none()
                            && search::allows_fallback(&observation)
                        {
                            if let Err(error) = search::validate_failed_observation(
                                &observation,
                                Some(&goal.permissions),
                            ) {
                                return self.stop(
                                    &mut state,
                                    Lifecycle::Suspend,
                                    "SEARCH_OBSERVATION_FAILURE",
                                    &error.to_string(),
                                );
                            }
                            self.advance(
                                &mut state,
                                Lifecycle::Running,
                                Some("SEARCH_PRIMARY_FAILURE_OBSERVED".into()),
                            )?;
                            failed_primary = Some(observation);
                            provider = fallback;
                            continue;
                        }
                        return self.stop(
                            &mut state,
                            Lifecycle::Suspend,
                            "SEARCH_OBSERVATION_FAILURE",
                            error,
                        );
                    }
                    if let Err(error) =
                        search::validate_stored_observation(&observation, Some(&goal.permissions))
                    {
                        state.result = Some(result);
                        return self.stop(
                            &mut state,
                            Lifecycle::Suspend,
                            "SEARCH_OBSERVATION_FAILURE",
                            &error.to_string(),
                        );
                    }
                    let evidence = if search_plan.fallback.is_some() {
                        self.store.put_artifact(
                            actor,
                            &request.case_id,
                            &(search::SEARCH_SELECTION_KIND, &result.dependencies),
                        )?
                    } else {
                        artifact
                    };
                    graph.observe(search::SEARCH_LISTING_NODE, &evidence, &BTreeMap::new())?;
                    units = search::selected_listing_lines(
                        &observation,
                        failed_primary.as_ref(),
                        search_plan.fallback.is_some(),
                    );
                    result.guarantee = Guarantee::SourceReport;
                    state.result = Some(result.clone());
                    break;
                }
            }
            Intent::FetchPreviousSearchCandidate { rank } => {
                let resumes_partial = bundle.meaning.parser_version
                    == input::PARTIAL_WEB_CANDIDATE_FOLLOWUP_PARSER_VERSION;
                let Some(prior) = previous.filter(|prior| {
                    prior.lifecycle == Lifecycle::Commit
                        || (resumes_partial && search::is_partial_web_candidate_context(prior))
                }) else {
                    return self.stop(
                        &mut state,
                        Lifecycle::Suspend,
                        "MISSING_SEARCH_RESULT_CONTEXT",
                        "本文取得の対象となる直前の検索結果がありません。",
                    );
                };
                let observation = if resumes_partial {
                    match search::candidate_body_observation(
                        &self.store,
                        &bundle,
                        prior,
                        None,
                        "SEARCH_CONTEXT_INVALID",
                    ) {
                        Ok(observation) => observation,
                        Err(Error::Contract { code, message }) => {
                            return self.stop(&mut state, Lifecycle::Suspend, &code, &message);
                        }
                        Err(error) => return Err(error),
                    }
                } else {
                    search::previous_observation(&self.store, actor, &request.case_id, prior)?
                };
                let Some((search_ref, observation)) = observation else {
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
                    .any(|ready| ready == search::SEARCH_CANDIDATE_LISTING_NODE)
                {
                    return self.stop(
                        &mut state,
                        Lifecycle::Suspend,
                        "UNOBSERVED_PREREQUISITE",
                        "検索候補の本文取得に必要な検索一覧が未観測です。",
                    );
                }
                graph.observe(
                    search::SEARCH_CANDIDATE_LISTING_NODE,
                    &search_ref,
                    &BTreeMap::new(),
                )?;
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
                let read = match retrieval_budget(1, &state, &self.store).and_then(|mut budget| {
                    search::fetch_candidate_with_budget(
                        candidate,
                        &goal.permissions,
                        &mut budget,
                        &read_admission(&state)?,
                    )
                }) {
                    Ok(read) => read,
                    Err(error) if read_case_changed(&error) => return Err(error),
                    Err(error) => {
                        let code = match &error {
                            Error::Contract { code, .. } => code.as_str(),
                            _ => "EVIDENCE_UNAVAILABLE",
                        };
                        state.result = Some(result);
                        return self.stop(&mut state, Lifecycle::Suspend, code, &error.to_string());
                    }
                };
                let packet = read.observation;
                if packet.raw_body.len() > request.budget.max_evidence_bytes {
                    return self.stop(
                        &mut state,
                        Lifecycle::Suspend,
                        "RESOURCE_LIMIT",
                        "検索候補の取得本文が要求の証拠予算を超えました。",
                    );
                }
                let body_ref = self.store.put_artifact(actor, &request.case_id, &packet)?;
                result.dependencies.push(body_ref.clone());
                if !result.evidence.iter().any(|existing| existing == &packet) {
                    result.evidence.push(packet.clone());
                }
                if read.redirect_wait.is_some()
                    || packet.failure.as_deref() == Some(ports::REDIRECT_RETRY_DEFERRED)
                {
                    return self.suspend_redirect_retry(
                        &mut state,
                        result,
                        read.redirect_wait.as_deref(),
                    );
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
                graph.observe(
                    &body_id,
                    &body_ref,
                    &BTreeMap::from([(search::SEARCH_CANDIDATE_LISTING_NODE.into(), search_ref)]),
                )?;
                units.extend(crate::search::candidate_body_lines(candidate, &packet));
                result.guarantee = Guarantee::SourceReport;
                state.result = Some(result.clone());
            }
            Intent::ExplainPrevious
            | Intent::ShowCalculationSequenceHistory
            | Intent::ExplainCalculationSequenceStep { .. }
                if previous_sequence.is_some() =>
            {
                let context = previous_sequence
                    .as_ref()
                    .expect("sequence context checked");
                let explanation = match calculation_sequence_report(
                    &self.store,
                    context,
                    &bundle.meaning.intent,
                    &mut graph,
                    &goal.budget,
                    started,
                ) {
                    Ok(explanation) => explanation,
                    Err(Error::Contract { code, message }) => {
                        return self.stop(&mut state, Lifecycle::Suspend, &code, &message);
                    }
                    Err(error) => return Err(error),
                };
                units = explanation.units;
                result.calculation = Some(explanation.calculation);
                result.dependencies = explanation.dependencies;
                result.guarantee = Guarantee::Computed;
                report_goal_content = Some(ReportGoalContent {
                    predicate: match bundle.meaning.intent {
                        Intent::ShowCalculationSequenceHistory => SEQUENCE_HISTORY_PREDICATE,
                        Intent::ExplainCalculationSequenceStep { .. } => {
                            SEQUENCE_STEP_EXPLANATION_PREDICATE
                        }
                        _ => context.predicate(),
                    },
                    guarantee: Guarantee::Computed,
                    dependencies: result.dependencies.clone(),
                    units: units.clone(),
                });
            }
            Intent::ShowCalculationSequenceHistory => {
                return self.stop(
                    &mut state,
                    Lifecycle::Suspend,
                    "SEQUENCE_HISTORY_CONTEXT_REQUIRED",
                    "変更履歴を表示するには、同じケースの確定済み連続計算が必要です。",
                );
            }
            Intent::ExplainCalculationSequenceStep { .. } => {
                return self.stop(
                    &mut state,
                    Lifecycle::Suspend,
                    "SEQUENCE_STEP_EXPLANATION_CONTEXT_REQUIRED",
                    "手順を詳説するには、同じケースの確定済み連続計算が必要です。",
                );
            }
            Intent::ExplainPrevious
                if bundle.meaning.parser_version
                    == input::PREVIOUS_CORRECTION_EXPLANATION_PARSER_VERSION =>
            {
                let Some(prior) = previous else {
                    return self.stop(
                        &mut state,
                        Lifecycle::Suspend,
                        "PRIOR_INPUT_RESULT_MISMATCH",
                        "同じ会話の確定した訂正結果を確認できません。",
                    );
                };
                let memory = record_source_memory_at_head(&self.store, actor)?;
                let explanation = match previous_correction_explanation(
                    &self.store,
                    &state,
                    &bundle,
                    prior,
                    &memory,
                    started,
                ) {
                    Ok(explanation) => explanation,
                    Err(Error::Contract { code, message }) => {
                        return self.stop(&mut state, Lifecycle::Suspend, &code, &message);
                    }
                    Err(error) => return Err(error),
                };
                result.actor_memory_revision = Some(memory.revision);
                result.dependencies = explanation.dependencies;
                result.guarantee = Guarantee::UserPremise;
                units = explanation.units;
                report_goal_content = Some(ReportGoalContent {
                    predicate: PREVIOUS_CORRECTION_EXPLANATION_PREDICATE,
                    guarantee: Guarantee::UserPremise,
                    dependencies: result.dependencies.clone(),
                    units: units.clone(),
                });
            }
            Intent::ExplainPrevious
                if bundle.meaning.parser_version
                    == input::DIRECT_SOURCE_EXPLANATION_PARSER_VERSION =>
            {
                let Some(prior) = previous else {
                    return self.stop(
                        &mut state,
                        Lifecycle::Suspend,
                        "DIRECT_SOURCE_EXPLANATION_CONTEXT_INVALID",
                        "説明する取得結果が保存されていません。",
                    );
                };
                let explanation = match crate::recovery::previous_direct_source_explanation(
                    &self.store,
                    &bundle,
                    prior,
                    None,
                ) {
                    Ok(explanation) => explanation,
                    Err(Error::Contract { code, message }) => {
                        return self.stop(&mut state, Lifecycle::Suspend, &code, &message);
                    }
                    Err(error) => {
                        return self.stop(
                            &mut state,
                            Lifecycle::Suspend,
                            "DIRECT_SOURCE_EXPLANATION_CONTEXT_INVALID",
                            &error.to_string(),
                        );
                    }
                };
                units = explanation.units;
                result.dependencies = explanation.dependencies;
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
                let search_explanation = match search::previous_search_explanation(
                    &self.store,
                    &bundle,
                    &state.prior_results,
                    prior,
                ) {
                    Ok(value) => value,
                    Err(Error::Contract { code, message }) => {
                        return self.stop(&mut state, Lifecycle::Suspend, &code, &message);
                    }
                    Err(error) => {
                        return self.stop(
                            &mut state,
                            Lifecycle::Suspend,
                            "SEARCH_EXPLANATION_CONTEXT_INVALID",
                            &error.to_string(),
                        );
                    }
                };
                let summary_explanation = match crate::text_summary::previous_summary_explanation(
                    &self.store,
                    &bundle,
                    &state.prior_results,
                    prior,
                    scoped_question_explanation_input,
                ) {
                    Ok(value) => value,
                    Err(error) => {
                        let code = match &error {
                            Error::Contract { code, .. } => {
                                code.strip_prefix("RECOVERY_").unwrap_or(code)
                            }
                            _ => "RESULT_SUMMARY_ARTIFACT_INVALID",
                        };
                        let lifecycle = if code == "RESULT_SUMMARY_SOURCE_INTEGRITY" {
                            Lifecycle::Fail
                        } else {
                            Lifecycle::Suspend
                        };
                        let message = match &error {
                            Error::Contract { message, .. } => message.clone(),
                            _ => error.to_string(),
                        };
                        return self.stop(&mut state, lifecycle, code, &message);
                    }
                };
                let prior_relation = if prior.calculation.is_none() {
                    previous_relation_meaning_from_input(
                        &self.store,
                        actor,
                        &request.case_id,
                        prior,
                        prior_context_input.as_ref(),
                    )?
                } else {
                    None
                };
                let prior_relation_objects =
                    if prior.calculation.is_none() && prior_relation.is_none() {
                        match prior_context_input.as_ref() {
                            Some(prior_bundle) => {
                                if identity(&prior_bundle.request)? != prior.id {
                                    return self.stop(
                                        &mut state,
                                        Lifecycle::Suspend,
                                        "RELATION_OBJECT_CONTEXT_INVALID",
                                        "前回の関係一覧入力と採用結果の同一性が一致しません。",
                                    );
                                }
                                match &prior_bundle.meaning.intent {
                                    Intent::AskRelationObjects {
                                        subject,
                                        predicate,
                                        negative,
                                        scope,
                                    } => Some((
                                        subject.clone(),
                                        predicate.clone(),
                                        *negative,
                                        scope.clone(),
                                    )),
                                    _ => None,
                                }
                            }
                            None => None,
                        }
                    } else {
                        None
                    };
                let prior_relation_profile = if prior.calculation.is_none()
                    && prior_relation.is_none()
                    && prior_relation_objects.is_none()
                {
                    match previous_relation_profile_from_input(
                        actor,
                        &request.case_id,
                        prior,
                        prior_context_input.as_ref(),
                    ) {
                        Ok(profile) => profile,
                        Err(Error::Contract { code, message }) => {
                            return self.stop(&mut state, Lifecycle::Suspend, &code, &message);
                        }
                        Err(error) => return Err(error),
                    }
                } else {
                    None
                };
                if let Some(assertion) = &previous_assertion {
                    let memory = record_source_memory_at_head(&self.store, actor)?;
                    let explanation = match assertion.explain(&memory) {
                        Ok(explanation) => explanation,
                        Err(Error::Contract { code, message }) => {
                            return self.stop(&mut state, Lifecycle::Suspend, &code, &message);
                        }
                        Err(error) => return Err(error),
                    };
                    result.actor_memory_revision = Some(memory.revision);
                    result.dependencies = explanation.dependencies;
                    result.guarantee = Guarantee::UserPremise;
                    units = explanation.units;
                    report_goal_content = Some(ReportGoalContent {
                        predicate: PREVIOUS_ASSERTION_EXPLANATION_PREDICATE,
                        guarantee: Guarantee::UserPremise,
                        dependencies: result.dependencies.clone(),
                        units: units.clone(),
                    });
                } else if let Some(ref calculation) = prior.calculation {
                    units.extend(calculation_detail_units(calculation));
                    approved_surfaces =
                        Some(language::calculation_explanation_surfaces(calculation));
                    result.calculation = Some(calculation.clone());
                    result.dependencies.push(prior.id.clone());
                    result.guarantee = prior.guarantee.clone();
                } else if let Some(PriorRelationMeaning::OriginalQuestion(proposition)) =
                    prior_relation.as_ref()
                {
                    let memory = self.store.load_memory(actor)?;
                    result.actor_memory_revision = Some(memory.revision);
                    let answer =
                        relations::query(&memory, proposition, request.budget.max_actions)?;
                    retain_relation_answer_dependencies(&mut result, &answer);
                    state.result = Some(result.clone());
                    if let Some((lifecycle, code, message)) = condition_block(&answer) {
                        return self.stop(&mut state, lifecycle, code, &message);
                    }
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
                    result.dependencies = vec![prior.id.clone()];
                    result.dependencies.extend(proof_dependencies);
                    result.guarantee = answer_guarantee;
                } else if let Some(PriorRelationMeaning::AdoptedConclusion {
                    proposition,
                    proof_dependencies: expected_proof_dependencies,
                }) = prior_relation.as_ref()
                {
                    let memory = match contextual_query_memory {
                        Some(memory) => memory,
                        None => self.store.load_memory(actor)?,
                    };
                    result.actor_memory_revision = Some(memory.revision);
                    let answer =
                        relations::query(&memory, proposition, request.budget.max_actions)?;
                    retain_relation_answer_dependencies(&mut result, &answer);
                    state.result = Some(result.clone());
                    if let Some((lifecycle, code, message)) = condition_block(&answer) {
                        return self.stop(&mut state, lifecycle, code, &message);
                    }
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
                    result.dependencies = vec![prior.id.clone()];
                    result.dependencies.extend(proof_dependencies);
                    result.guarantee = answer_guarantee;
                } else if let Some((subject, predicate, negative, scope)) =
                    prior_relation_objects.as_ref()
                {
                    if bundle.meaning.context_result_ref.as_deref() != Some(prior.id.as_str()) {
                        return self.stop(
                            &mut state,
                            Lifecycle::Suspend,
                            "MISSING_RELATION_OBJECT_CONTEXT",
                            "説明対象として固定された直前の関係一覧がありません。",
                        );
                    }
                    let memory = self.store.load_memory(actor)?;
                    result.actor_memory_revision = Some(memory.revision);
                    match relation_objects_explanation(
                        &memory,
                        prior,
                        RelationObjectsReplay {
                            subject,
                            predicate,
                            negative: *negative,
                            scope,
                            max_actions: request.budget.max_actions,
                            max_evidence_bytes: request.budget.max_evidence_bytes,
                        },
                    )? {
                        RelationObjectsExplanationEvaluation::Complete(answer) => {
                            units.extend(answer.units);
                            result.dependencies.extend(answer.dependencies);
                            result.guarantee = answer.guarantee;
                        }
                        RelationObjectsExplanationEvaluation::Blocked { code, message } => {
                            return self.stop(&mut state, Lifecycle::Suspend, code, &message);
                        }
                    }
                } else if let Some((subject, _, _)) = prior_relation_profile.as_ref() {
                    if bundle.meaning.context_result_ref.as_deref() != Some(prior.id.as_str()) {
                        return self.stop(
                            &mut state,
                            Lifecycle::Suspend,
                            "MISSING_RELATION_PROFILE_CONTEXT",
                            "説明対象として固定された直前の紹介文案がありません。",
                        );
                    }
                    let Some(previous_memory_revision) = prior.actor_memory_revision else {
                        return self.stop(
                            &mut state,
                            Lifecycle::Suspend,
                            "RELATION_PROFILE_MEMORY_REVISION_MISSING",
                            "前回の紹介文案に採用時のactor-memory revisionがありません。",
                        );
                    };
                    let Some(original_memory) = self
                        .store
                        .load_memory_revision(actor, previous_memory_revision)?
                    else {
                        return self.stop(
                            &mut state,
                            Lifecycle::Suspend,
                            "RELATION_PROFILE_MEMORY_REVISION_MISSING",
                            "前回の紹介文案が参照した記憶版を取得できません。",
                        );
                    };
                    let (verified_subject, original_answer) =
                        match relation_profile_result_from_memory(
                            &self.store,
                            actor,
                            &request.case_id,
                            prior,
                            &original_memory,
                        ) {
                            Ok(value) => value,
                            Err(Error::Contract { code, message }) => {
                                return self.stop(&mut state, Lifecycle::Suspend, &code, &message);
                            }
                            Err(error) => return Err(error),
                        };
                    if &verified_subject != subject {
                        return self.stop(
                            &mut state,
                            Lifecycle::Suspend,
                            "RELATION_PROFILE_CONTEXT_INVALID",
                            "前回入力の対象と紹介文案の対象が一致しません。",
                        );
                    }
                    let current_memory = self.store.load_memory(actor)?;
                    result.actor_memory_revision = Some(current_memory.revision);
                    let current_answer = match relation_profile_answer(
                        &current_memory,
                        subject,
                        request.budget.max_actions,
                        request.budget.max_evidence_bytes,
                    ) {
                        Ok(answer) => answer,
                        Err(Error::Contract { code, message })
                            if code == "RELATION_BUDGET" || code == "RESOURCE_LIMIT" =>
                        {
                            return self.stop(
                                &mut state,
                                Lifecycle::Suspend,
                                "RESOURCE_LIMIT",
                                &message,
                            );
                        }
                        Err(Error::Contract { .. }) => {
                            return self.stop(
                                &mut state,
                                Lifecycle::Suspend,
                                "CONTEXT_PROOF_CHANGED",
                                "紹介文案の根拠を現在のactor memoryで再確認できません。",
                            );
                        }
                        Err(error) => return Err(error),
                    };
                    if current_answer != original_answer {
                        return self.stop(
                            &mut state,
                            Lifecycle::Suspend,
                            "CONTEXT_PROOF_CHANGED",
                            "紹介文案の内容または依存根拠が現在のactor memoryと一致しません。",
                        );
                    }
                    let explanation = relation_profile_explanation_units(
                        &prior.id,
                        previous_memory_revision,
                        &current_answer,
                    );
                    if explanation.join("\n").len() > request.budget.max_evidence_bytes {
                        return self.stop(
                            &mut state,
                            Lifecycle::Suspend,
                            "RESOURCE_LIMIT",
                            "紹介文案の説明が今回の証拠予算を超えています。",
                        );
                    }
                    units.extend(explanation);
                    result.dependencies = vec![prior.id.clone()];
                    result
                        .dependencies
                        .extend(prior.dependencies.iter().cloned());
                    result.guarantee = original_answer.guarantee;
                } else if is_saved_relation_comparison_result(&prior) {
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
                    let artifact = match decode_relation_comparison_artifact(&report_bytes) {
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
                    if artifact.canonical_bytes != report_bytes
                        || digest(&report_bytes) != report_ref
                        || !matches!(
                            artifact.kind.as_str(),
                            RELATION_COMPARISON_REPORT_KIND
                                | RELATION_COMPARISON_CONTENT_KIND
                                | RELATION_COMPARISON_CONTENT_KIND_V3
                                | RELATION_COMPARISON_CONTENT_KIND_V4
                                | RELATION_COMPARISON_CONTENT_KIND_V5
                                | RELATION_COMPARISON_CONTENT_KIND_V6
                        )
                        || matches!(
                            artifact.kind.as_str(),
                            RELATION_COMPARISON_CONTENT_KIND
                                | RELATION_COMPARISON_CONTENT_KIND_V3
                                | RELATION_COMPARISON_CONTENT_KIND_V4
                        ) != artifact.content.is_some()
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
                    let inventories = self.active_relation_comparison_inventories(
                        actor,
                        &artifact.subject_a,
                        &artifact.subject_b,
                        &artifact.scope,
                        per_subject_actions,
                    )?;
                    result.actor_memory_revision = Some(inventories.revision);
                    let inventory_a = match inventories.inventory_a {
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
                    let inventory_b = match inventories.inventory_b {
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
                    let (report, expected_content_units) = if artifact.generated.is_some() {
                        let prior_bundle = self.store.get_input_bundle(
                            actor,
                            &request.case_id,
                            &prior.input_id,
                            prior.input_version,
                        )?;
                        let Some((_, prior_bundle)) = prior_bundle else {
                            return self.stop(
                                &mut state,
                                Lifecycle::Suspend,
                                "RELATION_COMPARISON_ARTIFACT_UNAVAILABLE",
                                "前回比較の封印入力を取得できません。",
                            );
                        };
                        let Intent::CompareRelationInventories {
                            subject_a,
                            subject_b,
                            scope,
                            format,
                        } = &prior_bundle.meaning.intent
                        else {
                            return self.stop(
                                &mut state,
                                Lifecycle::Suspend,
                                "RELATION_COMPARISON_ARTIFACT_INVALID",
                                "前回比較の封印済みの目的を照合できません。",
                            );
                        };
                        if !input::uses_generated_comparison(&prior_bundle.meaning)
                            || subject_a != &artifact.subject_a
                            || subject_b != &artifact.subject_b
                            || scope != &artifact.scope
                            || identity(&prior_bundle.request)? != prior.id
                        {
                            return self.stop(
                                &mut state,
                                Lifecycle::Suspend,
                                "RELATION_COMPARISON_ARTIFACT_INVALID",
                                "前回比較の型付き入力が記録と一致しません。",
                            );
                        }
                        let replay = RelationComparisonReplay {
                            subject_a: &artifact.subject_a,
                            subject_b: &artifact.subject_b,
                            scope: &artifact.scope,
                            locale: &prior_bundle.request.locale,
                            format: *format,
                            max_actions: request.budget.max_actions,
                            max_evidence_bytes: prior_bundle.request.budget.max_evidence_bytes,
                            generated: true,
                        };
                        let verified = verify_generated_comparison(
                            &artifact,
                            &inventory_a,
                            &inventory_b,
                            &prior,
                            prior_plan,
                            replay,
                        )
                        .and_then(|answer| {
                            verify_generated_model_artifacts(
                                &self.store,
                                actor,
                                &request.case_id,
                                prior_plan,
                            )?;
                            Ok(answer)
                        });
                        match verified {
                            Ok(content) => content,
                            Err(error) => {
                                return self.stop(
                                    &mut state,
                                    Lifecycle::Suspend,
                                    "RELATION_COMPARISON_ARTIFACT_INVALID",
                                    &error.to_string(),
                                );
                            }
                        }
                    } else if let Some((content_request, content_composition)) = &artifact.content {
                        let regenerated_request = match artifact.kind.as_str() {
                            RELATION_COMPARISON_CONTENT_KIND => {
                                relation_comparison_content_request_v2(
                                    &artifact.subject_a,
                                    &artifact.subject_b,
                                    &artifact.scope,
                                    &inventory_a,
                                    &inventory_b,
                                    content_request.max_output_bytes,
                                )?
                            }
                            RELATION_COMPARISON_CONTENT_KIND_V3 => {
                                relation_comparison_content_request_v3(
                                    &artifact.subject_a,
                                    &artifact.subject_b,
                                    &artifact.scope,
                                    &inventory_a,
                                    &inventory_b,
                                    &inventories.comparison_rules,
                                    content_request.max_output_bytes,
                                )?
                            }
                            RELATION_COMPARISON_CONTENT_KIND_V4 => {
                                relation_comparison_content_request_v4(
                                    &artifact.subject_a,
                                    &artifact.subject_b,
                                    &artifact.scope,
                                    &inventory_a,
                                    &inventory_b,
                                    &inventories.comparison_rules,
                                    content_request.max_output_bytes,
                                )?
                            }
                            _ => None,
                        };
                        let expected_proofs =
                            relation_comparison_proof_ids(&inventory_a, &inventory_b);
                        let composed_proofs = content_composition
                            .evidence_ids
                            .iter()
                            .cloned()
                            .collect::<BTreeSet<_>>();
                        let replay = language::compose_typed_meaning(content_request).ok();
                        if regenerated_request.as_ref() != Some(content_request)
                            || replay.as_ref() != Some(content_composition)
                            || composed_proofs != expected_proofs
                        {
                            return self.stop(
                                    &mut state,
                                    Lifecycle::Suspend,
                                    "RELATION_COMPARISON_ARTIFACT_INVALID",
                                    "前回比較のCore内容要求、proof参照、再生結果が現在の記録と一致しません。",
                                );
                        }
                        let content_units =
                            match composed_content_units(content_request, content_composition) {
                                Ok(value) => value,
                                Err(error) => {
                                    return self.stop(
                                        &mut state,
                                        Lifecycle::Suspend,
                                        "RELATION_COMPARISON_ARTIFACT_INVALID",
                                        &error.to_string(),
                                    );
                                }
                            };
                        (content_composition.text.clone(), content_units)
                    } else {
                        let report = relation_comparison_report(
                            &artifact.subject_a,
                            &artifact.subject_b,
                            &artifact.scope,
                            &inventory_a,
                            &inventory_b,
                        );
                        (report.clone(), vec![report])
                    };
                    if digest(report.as_bytes()) != artifact.report_sha256
                        || prior_plan.required_units != expected_content_units
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
                    if let Some((plan, _)) = &artifact.generated {
                        expected_dependencies.extend(plan.proof_ids.iter().cloned());
                    } else {
                        expected_dependencies.extend(
                            inventory_a
                                .dependencies
                                .iter()
                                .chain(&inventory_b.dependencies)
                                .cloned()
                                .collect::<BTreeSet<_>>(),
                        );
                    }
                    if prior.dependencies != expected_dependencies {
                        return self.stop(
                            &mut state,
                            Lifecycle::Suspend,
                            "RELATION_COMPARISON_ARTIFACT_INVALID",
                            "前回比較の依存記録が左右の再検証済みinventoryと一致しません。",
                        );
                    }
                    let explanation = relation_comparison_explanation_units(&prior, prior_plan);
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
                    let Some((_, prior_bundle)) = self.store.get_input_bundle(
                        actor,
                        &request.case_id,
                        &prior.input_id,
                        prior.input_version,
                    )?
                    else {
                        return self.stop(
                            &mut state,
                            Lifecycle::Suspend,
                            "COMMON_RELATION_CONTEXT_UNAVAILABLE",
                            "前回の共通関係比較入力を同一ケースから取得できません。",
                        );
                    };
                    let Intent::AskCommonRelations {
                        subject_a,
                        subject_b,
                        scope,
                    } = &prior_bundle.meaning.intent
                    else {
                        return self.stop(
                            &mut state,
                            Lifecycle::Suspend,
                            "COMMON_RELATION_CONTEXT_INVALID",
                            "前回入力が型付きの共通関係比較ではありません。",
                        );
                    };
                    let memory = self.store.load_memory(actor)?;
                    result.actor_memory_revision = Some(memory.revision);
                    match common_relations_explanation(
                        &memory,
                        &report_bytes,
                        &report_ref,
                        prior,
                        CommonRelationsExplanationReplay {
                            subject_a,
                            subject_b,
                            scope,
                            max_actions: request.budget.max_actions,
                            max_evidence_bytes: request.budget.max_evidence_bytes,
                        },
                    )? {
                        CommonRelationsExplanationEvaluation::Complete(answer) => {
                            units.extend(answer.units);
                            result.dependencies.extend(answer.dependencies);
                            result.guarantee = answer.guarantee;
                        }
                        CommonRelationsExplanationEvaluation::Blocked { code, message } => {
                            return self.stop(&mut state, Lifecycle::Suspend, code, &message);
                        }
                    }
                } else if let Some(explanation) = search_explanation {
                    units.extend(explanation.units);
                    result.dependencies.extend(explanation.dependencies);
                    result.guarantee = Guarantee::SourceReport;
                } else if let Some(explanation) = summary_explanation {
                    units.extend(explanation.units);
                    result.dependencies.extend(explanation.dependencies);
                    result.guarantee = Guarantee::Hypothesis;
                } else if let Some(explanation) = verification_explanation {
                    units = explanation.units;
                    result.dependencies = explanation.dependencies;
                    result.guarantee = Guarantee::Computed;
                    report_goal_content = Some(ReportGoalContent {
                        predicate: crate::code_generation::VERIFICATION_EXPLANATION_PREDICATE,
                        guarantee: result.guarantee.clone(),
                        dependencies: result.dependencies.clone(),
                        units: units.clone(),
                    });
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
                } else if let Some(explanation) = &previous_document_explanation {
                    graph.observe(
                        explanation.input_node,
                        &explanation.bundle_ref,
                        &BTreeMap::new(),
                    )?;
                    graph.observe(
                        explanation.report_node,
                        &explanation.report_ref,
                        &BTreeMap::from([(
                            explanation.input_node.into(),
                            explanation.bundle_ref.clone(),
                        )]),
                    )?;
                    units.extend(explanation.lines.clone());
                    result.dependencies.extend([
                        prior.id.clone(),
                        explanation.bundle_ref.clone(),
                        explanation.report_ref.clone(),
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
                let Some((source_ref, report_ref, answer)) = &previous_group_mean_answer else {
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
                graph.observe(group_mean_source, source_ref, &BTreeMap::new())?;
                graph.observe(
                    group_mean_report,
                    report_ref,
                    &BTreeMap::from([(group_mean_source.into(), source_ref.clone())]),
                )?;
                graph.observe(
                    group_mean_answer,
                    &answer_ref,
                    &BTreeMap::from([(group_mean_report.into(), report_ref.clone())]),
                )?;
                result.dependencies.extend([
                    prior.id.clone(),
                    source_ref.clone(),
                    report_ref.clone(),
                    answer_ref,
                ]);
                units.extend(crate::document::group_mean_answer_lines(
                    answer,
                    csv_group_mean,
                ));
                result.guarantee = Guarantee::Computed;
            }
            Intent::RetainPreviousGroupMeanHypothesis
            | Intent::ExplainPreviousGroupMeanHypothesis
            | Intent::AssessPreviousGroupMeanHypothesis { .. } => {
                let hypothesis = match previous_group_mean_hypothesis(&self.store, &state) {
                    Ok(Some(hypothesis)) => hypothesis,
                    Ok(None) => unreachable!("group mean hypothesis intent already matched"),
                    Err(Error::Contract { code, message }) => {
                        return self.stop(&mut state, Lifecycle::Suspend, &code, &message);
                    }
                    Err(error) => return Err(error),
                };
                if !matches!(goal.intent, Intent::ExplainPreviousGroupMeanHypothesis) {
                    for bytes in &hypothesis.artifacts {
                        self.store.put_bytes(actor, &request.case_id, bytes)?;
                    }
                }
                report_goal_content = Some(ReportGoalContent {
                    predicate: GROUP_MEAN_HYPOTHESIS_PREDICATE,
                    guarantee: Guarantee::Hypothesis,
                    dependencies: hypothesis.dependencies.clone(),
                    units: hypothesis.units.clone(),
                });
                result.dependencies = hypothesis.dependencies;
                units = hypothesis.units;
                result.guarantee = Guarantee::Hypothesis;
            }
            intent @ (Intent::SummarizePreviousSource
            | Intent::SummarizePreviousSourceConstrained { .. }
            | Intent::SummarizePreviousSourceLines { .. }
            | Intent::SummarizePreviousSourcePacketLines { .. }) => {
                let Some(context) = previous_source else {
                    return self.stop(
                        &mut state,
                        Lifecycle::Suspend,
                        "MISSING_ACTUAL_RESULT",
                        "要約対象となる実際の採用済み出典結果がありません。",
                    );
                };
                let prior = context.source;
                let line_summary = input::source_line_summary_parser(intent).is_some();
                if line_summary && prior.lifecycle != Lifecycle::Commit {
                    return self.stop(
                        &mut state,
                        Lifecycle::Suspend,
                        "SOURCE_SUMMARY_PARTIAL_CONTEXT",
                        "行数を指定した出典要約には、取得が完了した本文が必要です。",
                    );
                }
                if source_followup_indices.is_empty() {
                    return self.stop(
                        &mut state,
                        Lifecycle::Suspend,
                        "SOURCE_BODY_UNAVAILABLE",
                        "前回結果に要約できる取得本文がありません。検索一覧や題名だけから本文を補いません。",
                    );
                }
                let constrained = line_summary
                    || crate::text_summary::constrained_source_summary_limits(intent).is_some();
                let mut source_packets = Vec::with_capacity(source_followup_indices.len());
                for (source_index, packet) in context.bodies() {
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
                    source_packets.push((source_index, packet, packet_ref));
                }
                let active_positions = match crate::text_summary::selected_source_summary_positions(
                    &source_packets,
                    &bundle.meaning,
                ) {
                    Ok(positions) => positions,
                    Err(Error::Contract { code, message }) => {
                        return self.stop(&mut state, Lifecycle::Suspend, &code, &message);
                    }
                    Err(error) => return Err(error),
                };
                if constrained && active_positions.len() != 1 {
                    return self.stop(
                        &mut state,
                        Lifecycle::Suspend,
                        "AMBIGUOUS_SOURCE_CONTEXT",
                        if line_summary {
                            "複数の出典があるため、行数を指定して要約する対象を一つに決められません。"
                        } else {
                            "複数の出典があるため、文字数・箇条書き数の制約を適用する対象を一つに決められません。"
                        },
                    );
                }
                result.dependencies.extend(context.dependencies());
                result.dependencies.extend(
                    source_packets
                        .iter()
                        .map(|(_, _, reference)| reference.clone()),
                );
                let mut source_reports = Vec::new();
                let mut report_bytes = 0usize;
                if let Some(stop) = crate::text_summary::partial_retrieval_stop(prior) {
                    units.push(stop.notice().into());
                }
                for position in active_positions {
                    let (_, packet, packet_ref) = &source_packets[position];
                    let summary_result = crate::text_summary::summarize_fetched_source_for_intent(
                        &bundle, &prior.id, packet, packet_ref,
                    );
                    let source_report = match summary_result {
                        Ok(report) => report,
                        Err(Error::Contract { code, message }) if line_summary => {
                            return self.stop(&mut state, Lifecycle::Suspend, &code, &message);
                        }
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
                    let unit = source_report.unit();
                    report_bytes = report_bytes.saturating_add(encoded.len());
                    if line_summary {
                        report_bytes = report_bytes.saturating_add(unit.len());
                        if source_report.sentence_count() > request.budget.max_actions as usize {
                            return self.stop(
                                &mut state,
                                Lifecycle::Suspend,
                                "RESOURCE_LIMIT",
                                "取得本文の候補文数が今回の推論予算を超えています。",
                            );
                        }
                    }
                    if report_bytes > request.budget.max_evidence_bytes {
                        return self.stop(
                            &mut state,
                            Lifecycle::Suspend,
                            "RESOURCE_LIMIT",
                            "出典付き要約artifactが要求の証拠予算を超えました。",
                        );
                    }
                    units.push(unit);
                    source_reports.push(encoded);
                }
                for encoded in source_reports {
                    let report_ref = self.store.put_bytes(actor, &request.case_id, &encoded)?;
                    result.dependencies.push(report_ref);
                }
                result.guarantee = Guarantee::Hypothesis;
            }
            Intent::SummarizePreviousResult { .. }
            | Intent::SummarizePreviousResultLines { .. } => {
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
                let Some(context) = previous_source else {
                    return self.stop(
                        &mut state,
                        Lifecycle::Suspend,
                        "MISSING_ACTUAL_RESULT",
                        "質問対象となる実際の採用済み出典結果がありません。",
                    );
                };
                let prior = context.source;
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
                let mut output_units = crate::text_summary::source_question_answer_lines_v2(answer);
                if let Some(stop) = crate::text_summary::partial_retrieval_stop(prior) {
                    output_units.insert(0, stop.notice().into());
                }
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
                result.dependencies.extend(context.dependencies());
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
                let _ = direct_typed_relation_correction_input(&bundle, superseded, replacement)?;
                let source_evidence = input::assertion_source_evidence(&bundle)?;
                if bundle.meaning.parser_version == input::PARSER_VERSION
                    && source_evidence.is_none()
                {
                    return self.stop(
                        &mut state,
                        Lifecycle::Suspend,
                        "CORRECTION_SOURCE_UNRESOLVED",
                        "訂正前後の値を原文に対応づけられないため、記憶の変更を保留しました。",
                    );
                }
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
                    (superseded, replacement),
                    &fingerprint,
                    source_evidence,
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

                let Some(source_evidence) = input::assertion_source_evidence(&bundle)? else {
                    return self.stop(
                        &mut state,
                        Lifecycle::Suspend,
                        "CORRECTION_SOURCE_EVIDENCE_UNAVAILABLE",
                        "訂正文の出典役割を一意に検証できません。記憶は変更していません。",
                    );
                };
                let (dependencies, invalidated_count) = apply_premise_correction(
                    &self.store,
                    actor,
                    &mut memory,
                    &targets,
                    (&superseded, &replacement),
                    &fingerprint,
                    Some(source_evidence),
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
                let multiple_conditions =
                    bundle.meaning.parser_version == input::MULTI_CONDITION_RELATION_PARSER_VERSION;
                if multiple_conditions
                    && let Some((code, message)) = relation_registration_stop(
                        &goal,
                        "前提の登録・表示と必要なファイル確認の予算が不足しています。",
                    )
                {
                    return self.stop(&mut state, Lifecycle::Suspend, code, message);
                }
                let _ = direct_typed_relation_assertion_input(&bundle, proposition)?;
                let mut memory = self.store.load_memory(actor)?;
                let source_evidence = input::assertion_source_evidence(&bundle)?;
                if multiple_conditions && source_evidence.is_none() {
                    return self.stop(
                        &mut state,
                        Lifecycle::Suspend,
                        "ASSERTION_SOURCE_EVIDENCE_UNAVAILABLE",
                        "条件付き前提の原文範囲を確認できません。",
                    );
                }
                let fact = relations::assert_fact_with_source_evidence(
                    &mut memory,
                    proposition.clone(),
                    &fingerprint,
                    source_evidence,
                )?;
                units = premise_registration_units(&fact);
                if multiple_conditions && units.join("\n").len() > goal.budget.max_evidence_bytes {
                    return self.stop(
                        &mut state,
                        Lifecycle::Suspend,
                        "RESOURCE_LIMIT",
                        "前提の登録結果が要求の証拠予算を超えています。",
                    );
                }
                self.store.save_memory(actor, &memory)?;
                result.dependencies.push(fact.id);
                result.guarantee = Guarantee::UserPremise;
                if multiple_conditions {
                    result.actor_memory_revision = Some(memory.revision);
                    report_goal_content = Some(ReportGoalContent {
                        predicate: MULTI_CONDITION_PREMISE_PREDICATE,
                        guarantee: Guarantee::UserPremise,
                        dependencies: result.dependencies.clone(),
                        units: units.clone(),
                    });
                }
            }
            Intent::AssertPreviousRelation {
                proposition,
                explicit_scope,
            } => {
                let after_correction =
                    input::is_correction_context_assertion(&bundle.meaning.parser_version);
                if after_correction
                    && let Some((code, message)) = relation_registration_stop(
                        &goal,
                        "前提の登録・表示と必要なファイル確認の予算が不足しています。",
                    )
                {
                    return self.stop(&mut state, Lifecycle::Suspend, code, message);
                }
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
                let mut memory = match contextual_query_memory {
                    Some(memory) => memory,
                    None => self.store.load_memory(actor)?,
                };
                let resolved = if after_correction {
                    match verified_correction_assertion_proposition(
                        &self.store,
                        &bundle,
                        &state.prior_results,
                        prior,
                        &memory,
                    ) {
                        Ok(proposition) => proposition,
                        Err(Error::Contract { code, message }) => {
                            let (lifecycle, code) = match code.as_str() {
                                "RELATION_BUDGET" => (Lifecycle::Suspend, "RESOURCE_LIMIT"),
                                "CONDITION_FALSE" => (Lifecycle::Reject, code.as_str()),
                                _ => (Lifecycle::Suspend, code.as_str()),
                            };
                            return self.stop(&mut state, lifecycle, code, &message);
                        }
                        Err(error) => return Err(error),
                    }
                } else {
                    let previous_assertion = match previous_contextual_assertion_fact(
                        &self.store,
                        actor,
                        &request.case_id,
                        &state.prior_results,
                        prior,
                        &memory,
                    ) {
                        Ok(assertion) => assertion,
                        Err(Error::Contract { code, message })
                            if matches!(
                                code.as_str(),
                                "CONTEXT_PROOF_CHANGED" | "PRIOR_INPUT_RESULT_MISMATCH"
                            ) =>
                        {
                            return self.stop(&mut state, Lifecycle::Suspend, &code, &message);
                        }
                        Err(error) => return Err(error),
                    };
                    let (
                        anchor,
                        expected_proof_dependencies,
                        adopted_conclusion,
                        assertion_fact_id,
                    ) = if let Some(previous_assertion) = previous_assertion {
                        let PreviousContextualAssertion {
                            fact,
                            has_contextual_assertion_parent,
                        } = previous_assertion;
                        if has_contextual_assertion_parent {
                            return self.stop(
                                &mut state,
                                Lifecycle::Suspend,
                                "CONTEXT_CHAIN_DEPTH_LIMIT",
                                "関係についての回答から続けて主語を引き継げる前提は二つまでです。新しい関係結果を確認してから続けてください。",
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
                    retain_relation_answer_dependencies(&mut result, &answer);
                    state.result = Some(result.clone());
                    if let Some((lifecycle, code, message)) = condition_block(&answer) {
                        return self.stop(&mut state, lifecycle, code, &message);
                    }
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
                    Proposition {
                        subject: anchor.subject.clone(),
                        scope: explicit_scope
                            .clone()
                            .unwrap_or_else(|| anchor.scope.clone()),
                        ..proposition.clone()
                    }
                };
                let source_evidence = input::assertion_source_evidence(&bundle)?;
                if after_correction && source_evidence.is_none() {
                    return self.stop(
                        &mut state,
                        Lifecycle::Suspend,
                        "ASSERTION_SOURCE_EVIDENCE_UNAVAILABLE",
                        "前提追加の原文範囲を確認できません。",
                    );
                }
                let base_revision = memory.revision;
                let fact = relations::assert_fact_with_source_evidence(
                    &mut memory,
                    resolved.clone(),
                    &fingerprint,
                    source_evidence,
                )?;
                units = contextual_assertion_units(&prior.id, &fact);
                if after_correction {
                    if base_revision.checked_add(1) != Some(memory.revision) || !fact.valid {
                        return Err(correction_context_mismatch());
                    }
                    if units.join("\n").len() > goal.budget.max_evidence_bytes {
                        return self.stop(
                            &mut state,
                            Lifecycle::Suspend,
                            "RESOURCE_LIMIT",
                            "前提の登録結果が要求の証拠予算を超えています。",
                        );
                    }
                    if started.elapsed() >= Duration::from_millis(goal.budget.deadline_ms) {
                        return self.stop(
                            &mut state,
                            Lifecycle::Suspend,
                            "DEADLINE",
                            "前提の保存前に元Goalの締切へ達しました。",
                        );
                    }
                }
                self.store.save_memory(actor, &memory)?;
                result.dependencies.push(prior.id.clone());
                result.dependencies.push(fact.id);
                result.guarantee = Guarantee::UserPremise;
                if after_correction {
                    result.actor_memory_revision = Some(memory.revision);
                    report_goal_content = Some(ReportGoalContent {
                        predicate: CORRECTION_ASSERTION_PREDICATE,
                        guarantee: Guarantee::UserPremise,
                        dependencies: result.dependencies.clone(),
                        units: units.clone(),
                    });
                }
            }
            intent @ (Intent::AddRule { .. } | Intent::AddConjunctiveRule { .. }) => {
                let predicate = guarded_rule_predicate(&bundle.meaning);
                if predicate.is_some()
                    && let Some((code, message)) = relation_registration_stop(
                        &goal,
                        "規則の登録・表示と必要なファイル確認の予算が不足しています。",
                    )
                {
                    return self.stop(&mut state, Lifecycle::Suspend, code, message);
                }
                let (premises, conclusion) = rule_declaration(intent).expect("rule declaration");
                let _ = direct_typed_relation_rule_input(&bundle, premises, conclusion)?;
                if matches!(intent, Intent::AddConjunctiveRule { .. })
                    && (premises.len() != 2
                        || premises.iter().any(|premise| {
                            premise.subject != premises[0].subject
                                || premise.scope != premises[0].scope
                                || premise.temporal_scope != premises[0].temporal_scope
                                || premise.condition_refs != premises[0].condition_refs
                        }))
                {
                    return self.stop(
                        &mut state,
                        Lifecycle::Reject,
                        "INVALID_CONJUNCTIVE_RULE",
                        "二前提規則には同じ主語・scope・時刻範囲の関係節が必要です。",
                    );
                }
                let mut memory = self.store.load_memory(actor)?;
                let rule = relations::add_rule_with_premises(
                    &mut memory,
                    premises.to_vec(),
                    conclusion.clone(),
                    &fingerprint,
                )?;
                units = rule_registration_units(&rule);
                if predicate.is_some() && units.join("\n").len() > goal.budget.max_evidence_bytes {
                    return self.stop(
                        &mut state,
                        Lifecycle::Suspend,
                        "RESOURCE_LIMIT",
                        "規則の登録結果が要求の証拠予算を超えています。",
                    );
                }
                self.store.save_memory(actor, &memory)?;
                result.dependencies.push(rule.id);
                result.guarantee = Guarantee::UserPremise;
                if let Some(predicate) = predicate {
                    result.actor_memory_revision = Some(memory.revision);
                    report_goal_content = Some(ReportGoalContent {
                        predicate,
                        guarantee: Guarantee::UserPremise,
                        dependencies: result.dependencies.clone(),
                        units: units.clone(),
                    });
                }
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
                let queries = self.active_conjunction_relation_queries(
                    actor,
                    propositions,
                    per_relation_budget,
                )?;
                result.actor_memory_revision = Some(queries.memory_revision);
                state.result = Some(result.clone());
                let evaluations = match queries.outcomes {
                    Ok(evaluations) => evaluations,
                    Err(Error::Contract { code, message }) if code == "RELATION_BUDGET" => {
                        return self.stop(
                            &mut state,
                            Lifecycle::Suspend,
                            "RESOURCE_LIMIT",
                            &message,
                        );
                    }
                    Err(error) => return Err(error),
                };
                let mut dependencies = BTreeSet::new();
                let mut all_supported = true;
                let mut any_derived = false;
                let mut conclusions = Vec::with_capacity(propositions.len());
                for (proposition, evaluation) in propositions.iter().zip(evaluations) {
                    let answer = evaluation.answer;
                    retain_relation_answer_dependencies(&mut result, &answer);
                    state.result = Some(result.clone());
                    if let Some((lifecycle, code, message)) = condition_block(&answer) {
                        result.dependencies.extend(dependencies.iter().cloned());
                        result.dependencies.sort();
                        result.dependencies.dedup();
                        state.result = Some(result.clone());
                        return self.stop(&mut state, lifecycle, code, &message);
                    }
                    match answer.status.as_str() {
                        "SUPPORTED" => conclusions
                            .push(format!("- {}", relation_answer_surface(proposition, false))),
                        "CONTRADICTED" => {
                            all_supported = false;
                            conclusions
                                .push(format!("- {}", relation_answer_surface(proposition, true)));
                        }
                        "CONFLICT" => {
                            result.dependencies.extend(dependencies.iter().cloned());
                            result.dependencies.sort();
                            result.dependencies.dedup();
                            state.result = Some(result.clone());
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
                            result.dependencies.extend(dependencies.iter().cloned());
                            result.dependencies.sort();
                            result.dependencies.dedup();
                            state.result = Some(result.clone());
                            return self.stop_unresolved_relation_with_facts(
                                &mut state,
                                &mut result,
                                proposition,
                                &evaluation.out_of_scope_facts,
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
                format,
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
                let direct_input =
                    direct_relation_comparison_input(&bundle, subject_a, subject_b, scope)?;
                let inventories = self.active_relation_comparison_inventories(
                    actor,
                    subject_a,
                    subject_b,
                    scope,
                    per_subject_actions,
                )?;
                result.actor_memory_revision = Some(inventories.revision);
                let inventory_a = match inventories.inventory_a {
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
                let inventory_b = match inventories.inventory_b {
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
                if let Some(direct_input) = &direct_input
                    && (direct_input.source_ids_a != inventory_a.dependencies
                        || direct_input.source_ids_b != inventory_b.dependencies)
                {
                    return self.stop(
                        &mut state,
                        Lifecycle::Suspend,
                        "RELATION_COMPARISON_EVIDENCE_CHANGED",
                        "内容生成前に左右いずれかの根拠集合が変化しました。元の証拠参照を差し替えず保留します。",
                    );
                }
                if inventory_a.facts.is_empty() && inventory_b.facts.is_empty() {
                    return self.stop(
                        &mut state,
                        Lifecycle::Suspend,
                        "MISSING_PREMISES",
                        "このscopeで二つの対象について比較できる支持記録がありません。共通点や差異がないとは断定しません。",
                    );
                }
                let generated = goal
                    .required_predicates
                    .iter()
                    .any(|predicate| predicate == RELATION_COMPARISON_PREDICATE);
                let (report, comparison_units, artifact_bytes) = if generated {
                    let meaning_plan = relation_comparison_meaning_plan(
                        subject_a,
                        subject_b,
                        scope,
                        *format,
                        &inventory_a,
                        &inventory_b,
                    );
                    let grammar = comparison_generation_request(
                        &meaning_plan,
                        &inventory_a,
                        &inventory_b,
                        request.budget.max_evidence_bytes,
                    )?;
                    let generation = started
                        .checked_add(Duration::from_millis(goal.budget.deadline_ms))
                        .ok_or_else(|| {
                            failure(
                                "INVALID_DEADLINE",
                                "execution deadline overflows this clock",
                            )
                        })
                        .and_then(|deadline| {
                            language::generate_semantic_meaning_until(&grammar, deadline)
                        });
                    let mut generation = match generation {
                        Ok(value) => value,
                        Err(error) => {
                            return self.stop(
                                &mut state,
                                Lifecycle::Suspend,
                                "CONTENT_PLAN_UNAVAILABLE",
                                &error.to_string(),
                            );
                        }
                    };
                    if let Err(error) =
                        validate_comparison_generation(&meaning_plan, &grammar, &generation.report)
                    {
                        return self.stop(
                            &mut state,
                            Lifecycle::Suspend,
                            "CONTENT_PLAN_INVALID",
                            &error.to_string(),
                        );
                    }
                    let report = generation.report.text.clone();
                    let comparison_units = generation
                        .report
                        .units
                        .iter()
                        .map(|unit| unit.text.clone())
                        .collect::<Vec<_>>();
                    let model_ref = digest(&canonical(&generation.model)?);
                    generation.report.model_artifact_ref = Some(model_ref);
                    let artifact_bytes = canonical(&RelationComparisonGeneratedArtifact {
                        kind: relation_comparison_generated_kind(*format).into(),
                        result_id: result.id.clone(),
                        report_sha256: digest(report.as_bytes()),
                        subject_a: subject_a.clone(),
                        subject_b: subject_b.clone(),
                        scope: scope.clone(),
                        inventory_a: inventory_a.clone(),
                        inventory_b: inventory_b.clone(),
                        meaning_plan: meaning_plan.clone(),
                        content_generation: generation.report.clone(),
                    })?;
                    generation_proof_ids = Some(meaning_plan.proof_ids);
                    content_generation = Some(generation.report);
                    generation_model = Some(generation.model);
                    generation_emission = Some(generation.emission);
                    (report, comparison_units, artifact_bytes)
                } else {
                    let mut report = relation_comparison_report(
                        subject_a,
                        subject_b,
                        scope,
                        &inventory_a,
                        &inventory_b,
                    );
                    let mut comparison_units = vec![report.clone()];
                    let artifact_bytes;
                    let (content_request, content_kind) = relation_comparison_content_request(
                        &inventories.comparison_rules,
                        subject_a,
                        subject_b,
                        scope,
                        &inventory_a,
                        &inventory_b,
                        request.budget.max_evidence_bytes,
                    )?;
                    if let Some(content_request) = content_request {
                        let composition = match language::compose_typed_meaning(&content_request) {
                            Ok(value) => value,
                            Err(error) => {
                                return self.stop(
                                    &mut state,
                                    Lifecycle::Suspend,
                                    "CONTENT_PLAN_UNAVAILABLE",
                                    &error.to_string(),
                                );
                            }
                        };
                        let expected_proofs =
                            relation_comparison_proof_ids(&inventory_a, &inventory_b);
                        let composed_proofs = composition
                            .evidence_ids
                            .iter()
                            .cloned()
                            .collect::<BTreeSet<_>>();
                        if composed_proofs != expected_proofs {
                            return self.stop(
                                &mut state,
                                Lifecycle::Suspend,
                                "CONTENT_PROOF_MAPPING",
                                "比較の新内容計画が左右の関係proof IDを完全には保持しません。",
                            );
                        }
                        comparison_units =
                            match composed_content_units(&content_request, &composition) {
                                Ok(value) => value,
                                Err(error) => {
                                    return self.stop(
                                        &mut state,
                                        Lifecycle::Suspend,
                                        "CONTENT_PLAN_INVALID",
                                        &error.to_string(),
                                    );
                                }
                            };
                        report = composition.text.clone();
                        approved_surfaces = Some(vec![report.clone()]);
                        artifact_bytes = canonical(&RelationComparisonContentArtifact {
                            kind: content_kind.into(),
                            result_id: result.id.clone(),
                            report_sha256: digest(report.as_bytes()),
                            subject_a: subject_a.clone(),
                            subject_b: subject_b.clone(),
                            scope: scope.clone(),
                            inventory_a: inventory_a.clone(),
                            inventory_b: inventory_b.clone(),
                            content_request,
                            content_composition: composition,
                        })?;
                    } else {
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
                        artifact_bytes = canonical(&artifact)?;
                    }
                    (report, comparison_units, artifact_bytes)
                };
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
                if let Some(model) = &generation_model {
                    self.store.put_artifact(actor, &request.case_id, model)?;
                }
                let report_ref = self
                    .store
                    .put_bytes(actor, &request.case_id, &artifact_bytes)?;
                units.extend(comparison_units);
                result.dependencies.push(report_ref);
                if let Some(proof_ids) = &generation_proof_ids {
                    result.dependencies.extend(proof_ids.iter().cloned());
                } else {
                    result.dependencies.extend(
                        inventory_a
                            .dependencies
                            .iter()
                            .chain(&inventory_b.dependencies)
                            .cloned()
                            .collect::<BTreeSet<_>>(),
                    );
                }
                // The report contrasts only the saved record set; omitted facts
                // and outside-world truth have not been checked.
                result.guarantee = Guarantee::Hypothesis;
                if generated {
                    report_goal_content = Some(ReportGoalContent {
                        predicate: RELATION_COMPARISON_PREDICATE,
                        guarantee: Guarantee::Hypothesis,
                        dependencies: result.dependencies.clone(),
                        units: units.clone(),
                    });
                }
            }
            Intent::AskCommonRelations {
                subject_a,
                subject_b,
                scope,
            } => {
                let replay = CommonRelationsReplay {
                    subject_a,
                    subject_b,
                    scope,
                    max_actions: request.budget.max_actions,
                    max_evidence_bytes: request.budget.max_evidence_bytes,
                    result_id: &result.id,
                };
                let (memory_revision, evaluation) =
                    self.active_common_relations_answer(actor, replay)?;
                result.actor_memory_revision = Some(memory_revision);
                state.result = Some(result.clone());
                let answer = match evaluation {
                    Ok(CommonRelationsEvaluation::Complete(answer)) => answer,
                    Ok(CommonRelationsEvaluation::Blocked { code, message }) => {
                        return self.stop(&mut state, Lifecycle::Suspend, code, &message);
                    }
                    Err(Error::Contract { code, message }) if code == "RELATION_BUDGET" => {
                        return self.stop(
                            &mut state,
                            Lifecycle::Suspend,
                            "RESOURCE_LIMIT",
                            &message,
                        );
                    }
                    Err(error) => return Err(error),
                };
                let report_ref =
                    self.store
                        .put_bytes(actor, &request.case_id, &answer.artifact_bytes)?;
                units.push(answer.report);
                result.dependencies.push(report_ref);
                result.dependencies.extend(answer.proof_dependencies);
                result.guarantee = answer.guarantee;
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

                let query =
                    if input::is_registered_previous_relation_query(&bundle.meaning.parser_version)
                    {
                        let memory = self.store.load_memory(actor)?;
                        let mut remaining_actions = request.budget.max_actions;
                        let resolution = match resolve_previous_relation_for_bundle(
                            &self.store,
                            &bundle,
                            &state.prior_results,
                            prior,
                            &memory,
                            &mut remaining_actions,
                        ) {
                            Ok(resolution) => resolution,
                            Err(Error::Contract { code, message }) if code == "RELATION_BUDGET" => {
                                return self.stop(
                                    &mut state,
                                    Lifecycle::Suspend,
                                    "RESOURCE_LIMIT",
                                    &message,
                                );
                            }
                            Err(Error::Contract { code, message })
                                if matches!(
                                    code.as_str(),
                                    "CONTEXT_PROOF_CHANGED" | "PRIOR_INPUT_RESULT_MISMATCH"
                                ) =>
                            {
                                return self.stop(&mut state, Lifecycle::Suspend, &code, &message);
                            }
                            Err(error) => return Err(error),
                        };
                        PreviousRelationQuery::from_snapshot(memory, resolution, remaining_actions)
                    } else {
                        let query = self.active_previous_relation_query(
                            actor,
                            prior,
                            proposition,
                            explicit_scope.as_deref(),
                            request.budget.max_actions,
                        )?;
                        if contextual_query_memory
                            .as_ref()
                            .is_some_and(|memory| memory.revision != query.memory.revision)
                        {
                            return self.stop(
                            &mut state, Lifecycle::Suspend, "CONTEXT_PROOF_CHANGED",
                            "文脈の照合中に記憶が更新されました。新しい問いで確認してください。",
                        );
                        }
                        query
                    };
                result.actor_memory_revision = Some(query.memory.revision);
                state.result = Some(result.clone());
                let answer = match query.answer {
                    Ok(answer) => answer,
                    Err(Error::Contract { code, message }) if code == "RELATION_BUDGET" => {
                        return self.stop(
                            &mut state,
                            Lifecycle::Suspend,
                            "RESOURCE_LIMIT",
                            &message,
                        );
                    }
                    Err(error) => return Err(error),
                };
                let resolution = query.resolution.ok_or_else(|| {
                    failure(
                        "PREVIOUS_RELATION_RESOLUTION_MISSING",
                        "a prior relation query stopped without recording its resolution",
                    )
                })?;
                let resolved = match resolution {
                    PreviousRelationResolution::Resolved(proposition) => proposition,
                    PreviousRelationResolution::Missing => {
                        return self.stop(
                            &mut state,
                            Lifecycle::Suspend,
                            "MISSING_RELATION_CONTEXT",
                            "直前の回答が参照する有効な直接前提がありません。明示したscopeを含め、代名詞の対象を推測しません。",
                        );
                    }
                    PreviousRelationResolution::Ambiguous => {
                        return self.stop(
                            &mut state,
                            Lifecycle::Suspend,
                            "AMBIGUOUS_RELATION_CONTEXT",
                            "直前の回答は複数の直接前提を参照しています。代名詞の対象を一意に決められません。",
                        );
                    }
                };
                let answer = answer.ok_or_else(|| {
                    failure(
                        "PREVIOUS_RELATION_QUERY_MISSING",
                        "a resolved previous relation did not produce a proof query",
                    )
                })?;
                retain_relation_answer_dependencies(&mut result, &answer);
                state.result = Some(result.clone());
                if let Some((lifecycle, code, message)) = condition_block(&answer) {
                    return self.stop(&mut state, lifecycle, code, &message);
                }
                let answer_dependencies = relations::answer_dependencies(&answer);
                let answer_guarantee = relations::answer_guarantee(&answer);
                if let Some(answer_units) =
                    previous_relation_success_units(&query.memory, &resolved, &answer, *explain)
                {
                    units.extend(answer_units);
                } else if answer.status == "CONFLICT" {
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
                } else {
                    let message = if *explain {
                        format!(
                            "範囲「{}」の「{}」を説明する有効な前提または導出経路がありません。",
                            resolved.scope,
                            describe(&resolved)
                        )
                    } else {
                        format!(
                            "範囲「{}」の「{}」を判定する有効な前提または導出経路がありません。",
                            resolved.scope,
                            describe(&resolved)
                        )
                    };
                    return self.stop(&mut state, Lifecycle::Suspend, "MISSING_PREMISES", &message);
                }
                result.dependencies = answer_dependencies;
                result.guarantee = answer_guarantee;
            }
            Intent::Ask { proposition } => {
                let (memory, answer) = match self.active_relation_query(
                    actor,
                    proposition,
                    request.budget.max_actions,
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
                    Err(error) => return Err(error),
                };
                result.actor_memory_revision = Some(memory.revision);
                retain_relation_answer_dependencies(&mut result, &answer);
                state.result = Some(result.clone());
                if let Some((lifecycle, code, message)) = condition_block(&answer) {
                    return self.stop(&mut state, lifecycle, code, &message);
                }
                let answer_dependencies = relations::answer_dependencies(&answer);
                let answer_guarantee = relations::answer_guarantee(&answer);
                match answer.status.as_str() {
                    "SUPPORTED"=>{
                        units.push(relation_answer_surface(proposition, false));
                        result.dependencies = answer_dependencies.clone();
                        result.guarantee=answer_guarantee;
                    },
                    "CONTRADICTED"=>{
                        units.push(relation_answer_surface(proposition, true));
                        result.dependencies = answer_dependencies;
                        result.guarantee=answer_guarantee;
                    },
                    "CONFLICT"=>return self.stop(&mut state,Lifecycle::Suspend,"CONFLICTING_PREMISES",
                        &format!("範囲「{}」の「{}」に肯定と否定の根拠があります。一方を恣意的に採用しません。",proposition.scope,describe(proposition))),
                    _=>return self.stop_unresolved_relation(
                        &mut state,
                        &mut result,
                        &memory,
                        proposition,
                        request.budget.max_actions,
                        &format!("範囲「{}」の「{}」を判定する有効な前提または導出経路がありません。",proposition.scope,describe(proposition)),
                    ),
                }
            }
            Intent::AskRelationObjects {
                subject,
                predicate,
                negative,
                scope,
            } => {
                let (memory, answer) = match self.active_relation_objects_answer(
                    actor,
                    RelationObjectsReplay {
                        subject,
                        predicate,
                        negative: *negative,
                        scope,
                        max_actions: request.budget.max_actions,
                        max_evidence_bytes: request.budget.max_evidence_bytes,
                    },
                ) {
                    Ok(value) => value,
                    Err(error) => {
                        let code = match &error {
                            Error::Contract { code, .. } => code.as_str(),
                            _ => "RELATION_QUERY_UNAVAILABLE",
                        };
                        return self.stop(&mut state, Lifecycle::Suspend, code, &error.to_string());
                    }
                };
                result.actor_memory_revision = Some(memory.revision);
                state.result = Some(result.clone());
                let answer = match answer {
                    Ok(answer) => answer,
                    Err(Error::Contract { code, message }) if code == "RELATION_BUDGET" => {
                        return self.stop(
                            &mut state,
                            Lifecycle::Suspend,
                            "RESOURCE_LIMIT",
                            &message,
                        );
                    }
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
                let direct_input = direct_atomic_meaning_input(&bundle, proposition)?;
                let (memory, answer) = match self.active_relation_query(
                    actor,
                    proposition,
                    request.budget.max_actions,
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
                    Err(error) => return Err(error),
                };
                result.actor_memory_revision = Some(memory.revision);
                retain_relation_answer_dependencies(&mut result, &answer);
                state.result = Some(result.clone());
                if let Some((lifecycle, code, message)) = condition_block(&answer) {
                    return self.stop(&mut state, lifecycle, code, &message);
                }
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
                        return self.stop_unresolved_relation(
                            &mut state,
                            &mut result,
                            &memory,
                            proposition,
                            request.budget.max_actions,
                            &format!(
                                "範囲「{}」の「{}」を説明する有効な前提または導出経路がありません。",
                                proposition.scope,
                                describe(proposition)
                            ),
                        );
                    }
                }
                if let Some(direct_input) = &direct_input {
                    let mut actual_source_ids = answer_dependencies.clone();
                    actual_source_ids.sort();
                    actual_source_ids.dedup();
                    if direct_input.source_ids != actual_source_ids {
                        return self.stop(
                            &mut state,
                            Lifecycle::Suspend,
                            "ATOMIC_MEANING_EVIDENCE_CHANGED",
                            "内容生成前に根拠集合が変化しました。元の証拠参照を差し替えず保留します。",
                        );
                    }
                }
                result.dependencies = answer_dependencies.clone();
                result.guarantee = answer_guarantee.clone();
                state.result = Some(result.clone());
                let requires_generation = goal
                    .required_predicates
                    .iter()
                    .any(|predicate| predicate == GENERATED_RELATION_EXPLANATION_PREDICATE);
                // Actor-composed rules can additionally adapt the generator.
                let uses_composed_rule = bundle.meaning.source_kind == "USER_REQUEST"
                    && memory.rules.iter().any(|rule| {
                        rule.source_input == "derived:proof-composition-v1"
                            && answer.used_rules.contains(&rule.id)
                    });
                if requires_generation || direct_input.is_some() || uses_composed_rule {
                    let Some(explained) = relation_explanation_target(proposition, &answer) else {
                        return self.stop(
                            &mut state,
                            Lifecycle::Suspend,
                            "MISSING_PREMISES",
                            "根拠付き説明を型付き意味として確定できません。",
                        );
                    };
                    let meaning_plan = match relation_atomic_meaning_plan(
                        &memory,
                        proposition,
                        &explained,
                        &answer,
                        &answer_dependencies,
                    ) {
                        Ok(value) => value,
                        Err(error) => {
                            return self.stop(
                                &mut state,
                                Lifecycle::Suspend,
                                "CONTENT_PLAN_UNAVAILABLE",
                                &error.to_string(),
                            );
                        }
                    };
                    let generation_request = match relation_meaning_plan_to_generation_request(
                        &meaning_plan,
                        relation_explanation_output_budget(
                            &bundle.meaning,
                            request.budget.max_evidence_bytes,
                        ),
                    ) {
                        Ok(value) => value,
                        Err(error) => {
                            return self.stop(
                                &mut state,
                                Lifecycle::Suspend,
                                "CONTENT_PLAN_UNAVAILABLE",
                                &error.to_string(),
                            );
                        }
                    };
                    let generation_deadline = started
                        .checked_add(Duration::from_millis(request.budget.deadline_ms))
                        .ok_or_else(|| {
                            failure(
                                "INVALID_DEADLINE",
                                "execution deadline overflows this clock",
                            )
                        })?;
                    let selected_adaptation = if uses_composed_rule {
                        let selection = select_rule_grammar_adaptation(
                            &self.store,
                            &bundle,
                            previous,
                            &meaning_plan,
                        )
                        .and_then(|selection| match selection {
                            Some(selection) => Ok(Some(selection)),
                            None => Ok(rule_grammar_documents(&meaning_plan, None)?.map(
                                |(rule_ids, documents)| {
                                    (RuleGrammarAdaptation::ActorRules { rule_ids }, documents)
                                },
                            )),
                        });
                        match selection {
                            Ok(selection) => selection,
                            Err(error) => {
                                return self.stop(
                                    &mut state,
                                    Lifecycle::Suspend,
                                    "MODEL_ADAPTATION_UNAVAILABLE",
                                    &error.to_string(),
                                );
                            }
                        }
                    } else {
                        None
                    };
                    let rule_documents = selected_adaptation
                        .as_ref()
                        .map_or(&[][..], |(_, documents)| documents.as_slice());
                    let generated = match language::generate_semantic_meaning_with_rules_until(
                        &generation_request,
                        rule_documents,
                        generation_deadline,
                    ) {
                        Ok(value) => value,
                        Err(error) => {
                            return self.stop(
                                &mut state,
                                Lifecycle::Suspend,
                                "CONTENT_PLAN_UNAVAILABLE",
                                &error.to_string(),
                            );
                        }
                    };
                    if let Err(error) = validate_relation_meaning_generation(
                        &meaning_plan,
                        &generation_request,
                        &generated.report,
                    ) {
                        return self.stop(
                            &mut state,
                            Lifecycle::Suspend,
                            "MEANING_REVERSE_CHECK",
                            &error.to_string(),
                        );
                    }
                    let represented = generated
                        .report
                        .evidence_ids
                        .iter()
                        .cloned()
                        .collect::<BTreeSet<_>>();
                    let expected = answer_dependencies.iter().cloned().collect::<BTreeSet<_>>();
                    if represented != expected {
                        return self.stop(
                            &mut state,
                            Lifecycle::Suspend,
                            "CONTENT_PROOF_MAPPING",
                            "新しい内容計画の根拠参照が関係proofと一致しません。",
                        );
                    }
                    if let Err(error) =
                        validate_relation_meaning_adoption(RelationMeaningAdoptionInput {
                            memory: &memory,
                            asked: proposition,
                            conclusion: &explained,
                            answer: &answer,
                            result: &result,
                            plan: &meaning_plan,
                            request: &generation_request,
                            report: &generated.report,
                            emission: &generated.emission,
                        })
                    {
                        return self.stop(
                            &mut state,
                            Lifecycle::Suspend,
                            "CONTENT_ADOPTION_MISMATCH",
                            &error.to_string(),
                        );
                    }
                    let model_artifact_ref =
                        self.store
                            .put_artifact(actor, &request.case_id, &generated.model)?;
                    let mut generation_report = generated.report;
                    generation_report.model_artifact_ref = Some(model_artifact_ref);
                    units = generation_report
                        .units
                        .iter()
                        .map(|unit| unit.text.clone())
                        .collect();
                    adopted_relation_meaning = Some(explained);
                    relation_proof_dependencies = Some(answer_dependencies.clone());
                    approved_surfaces = Some(vec![generated.emission.text.clone()]);
                    relation_meaning_plan = Some(meaning_plan);
                    content_generation = Some(generation_report);
                    applied_rule_adaptation = selected_adaptation.map(|(source, _)| source);
                    generation_model = Some(generated.model);
                    generation_emission = Some(generated.emission);
                } else {
                    let explanation_units =
                        relation_why_explanation_units(&memory, proposition, &answer)?;
                    approved_surfaces =
                        Some(language::relation_explanation_surfaces(&explanation_units));
                    units.extend(explanation_units);
                }
                report_goal_content = Some(ReportGoalContent {
                    predicate: if requires_generation {
                        GENERATED_RELATION_EXPLANATION_PREDICATE
                    } else {
                        RELATION_EXPLANATION_PREDICATE
                    },
                    guarantee: answer_guarantee,
                    dependencies: answer_dependencies,
                    units: units.clone(),
                });
            }
            Intent::InspectPremises { subject } => {
                let (memory_revision, inventory) =
                    self.active_premise_inventory(actor, subject, request.budget.max_actions)?;
                result.actor_memory_revision = Some(memory_revision);
                state.result = Some(result.clone());
                let inventory = match inventory {
                    Ok(inventory) => inventory,
                    Err(Error::Contract { code, .. }) if code == "RELATION_BUDGET" => {
                        return self.stop(
                            &mut state,
                            Lifecycle::Suspend,
                            "RESOURCE_LIMIT",
                            "保存済み関係を列挙・導出する作業記憶予算が不足しています。",
                        );
                    }
                    Err(Error::Contract { code, .. }) if code == "MISSING_PREMISES" => {
                        return self.stop(
                            &mut state,
                            Lifecycle::Suspend,
                            "MISSING_PREMISES",
                            "この対象について有効な提示前提または規則導出はありません。",
                        );
                    }
                    Err(Error::Contract { code, .. }) if code == "UNSUPPORTED_RELATION_RESULT" => {
                        return self.stop(
                            &mut state,
                            Lifecycle::Suspend,
                            "UNSUPPORTED_RELATION_RESULT",
                            "関係の列挙が既知の終端状態を返しませんでした。",
                        );
                    }
                    Err(error) => return Err(error),
                };

                let mut evidence_bytes = 0usize;
                for (index, line) in premise_inventory_units(subject, &inventory).enumerate() {
                    evidence_bytes = evidence_bytes
                        .checked_add(line.len())
                        .and_then(|total| total.checked_add(if index == 0 { 0 } else { 1 }))
                        .unwrap_or(usize::MAX);
                    if evidence_bytes > request.budget.max_evidence_bytes {
                        return self.stop(
                            &mut state,
                            Lifecycle::Suspend,
                            "RESOURCE_LIMIT",
                            "保存済み関係の一覧が要求の証拠予算を超えました。",
                        );
                    }
                    units.push(line);
                }
                result.dependencies = inventory.dependencies;
                result.guarantee = inventory.guarantee;
            }
            Intent::DraftRelationProfile { subject } => {
                let (memory_revision, answer) = self.active_relation_profile_answer(
                    actor,
                    subject,
                    request.budget.max_actions,
                    request.budget.max_evidence_bytes,
                )?;
                result.actor_memory_revision = Some(memory_revision);
                state.result = Some(result.clone());
                let answer = match answer {
                    Ok(answer) => answer,
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
                    Err(Error::Contract { code, message }) if code == "RESOURCE_LIMIT" => {
                        return self.stop(&mut state, Lifecycle::Suspend, &code, &message);
                    }
                    Err(Error::Contract { code, message }) => {
                        return self.stop(&mut state, Lifecycle::Suspend, &code, &message);
                    }
                    Err(error) => return Err(error),
                };
                approved_surfaces = Some(language::relation_profile_surfaces(&answer.units));
                units.extend(answer.units);
                result.dependencies = answer.dependencies;
                // The proof lineage remains attached, while completeness and
                // outside-world truth of a requested profile are unverified.
                result.guarantee = answer.guarantee;
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
            Intent::Retrieve { .. } | Intent::FetchNextUnobservedSource => {
                let urls = direct_urls.expect("direct retrieval URLs were bound to the Goal");
                let mut retrieval = match retrieval_budget(urls.len(), &state, &self.store) {
                    Ok(budget) => budget,
                    Err(error) => {
                        return self.stop(
                            &mut state,
                            Lifecycle::Suspend,
                            "EVIDENCE_UNAVAILABLE",
                            &error.to_string(),
                        );
                    }
                };
                let mut source_requires = BTreeMap::new();
                if let Some(continuation) = direct_continuation.as_ref() {
                    let reference =
                        self.store
                            .put_artifact(actor, &request.case_id, continuation.source)?;
                    graph.observe(PREVIOUS_DIRECT_RETRIEVAL_NODE, &reference, &BTreeMap::new())?;
                    source_requires.insert(PREVIOUS_DIRECT_RETRIEVAL_NODE.into(), reference);
                    result.dependencies.push(continuation.source.id.clone());
                    retrieval.exclude_observed_routes(&continuation.observed_routes);
                }
                for (source_index, uri) in urls.iter().enumerate() {
                    let read = match ports::fetch_with_budget(
                        uri,
                        &goal.permissions,
                        &mut retrieval,
                        &read_admission(&state)?,
                    ) {
                        Ok(read) => read,
                        Err(error) if read_case_changed(&error) => return Err(error),
                        Err(error) => {
                            state.result = Some(result);
                            return self.stop(
                                &mut state,
                                Lifecycle::Suspend,
                                "EVIDENCE_UNAVAILABLE",
                                &error.to_string(),
                            );
                        }
                    };
                    let packet = read.observation;
                    let observed = self.store.put_artifact(actor, &request.case_id, &packet)?;
                    if packet.failure.is_none() && packet.extracted_text.is_some() {
                        graph.observe(
                            &format!("source-{source_index}"),
                            &observed,
                            &source_requires,
                        )?;
                    }
                    let refused_dns = packet.failure.as_deref() == Some("PRIVATE_READ_TARGET");
                    let deferred_redirect = read.redirect_wait.is_some()
                        || packet.failure.as_deref() == Some(ports::REDIRECT_RETRY_DEFERRED);
                    result.dependencies.push(packet.id.clone());
                    result.evidence.push(packet);
                    if deferred_redirect {
                        return self.suspend_redirect_retry(
                            &mut state,
                            result,
                            read.redirect_wait.as_deref(),
                        );
                    }
                    // Keep the refusal terminal even when the port could
                    // retain evidence of an earlier redirect response.
                    if refused_dns {
                        state.result = Some(result);
                        return self.stop(
                            &mut state,
                            Lifecycle::Suspend,
                            "EVIDENCE_UNAVAILABLE",
                            &ports::private_dns_refusal().to_string(),
                        );
                    }
                    // Preserve the received prefix even if a later read is
                    // interrupted or the original deadline has now elapsed.
                    state.result = Some(result.clone());
                    self.advance(
                        &mut state,
                        Lifecycle::Running,
                        Some("RETRIEVAL_SOURCE_OBSERVED".into()),
                    )?;
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
                units = result.evidence.iter().map(direct_source_line).collect();
                if !language::model_source_bytes_fit(&units) {
                    let display = direct_source_display(&result.evidence);
                    units = display.units();
                    let display_bytes = canonical(&display)?;
                    if !language::model_source_bytes_fit(&units)
                        || display_bytes.len().saturating_add(units.join("\n").len())
                            > request.budget.max_evidence_bytes
                    {
                        state.result = Some(result);
                        return self.stop(
                            &mut state,
                            Lifecycle::Suspend,
                            "SOURCE_DISPLAY_RESOURCE_LIMIT",
                            "取得した本文は保存しました。出典と引用を表示する予算が不足しています。",
                        );
                    }
                    let reference =
                        self.store
                            .put_bytes(actor, &request.case_id, &display_bytes)?;
                    result.dependencies.push(reference);
                }
                result.guarantee = Guarantee::SourceReport;
                state.result = Some(result.clone());
            }
            Intent::GenerateCode { .. } | Intent::GenerateReturnFunction { .. } => {
                let generated = match &goal.intent {
                    Intent::GenerateCode { document } => crate::code_generation::generate(document),
                    _ => crate::code_generation::generate_return_function(&bundle),
                };
                let report = match generated {
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
                if matches!(goal.intent, Intent::GenerateReturnFunction { .. }) {
                    report_goal_content = Some(ReportGoalContent {
                        predicate: RETURN_FUNCTION_PREDICATE,
                        guarantee: result.guarantee.clone(),
                        dependencies: result.dependencies.clone(),
                        units: units.clone(),
                    });
                }
            }
            Intent::VerifyPreviousCode { .. }
                if bundle.meaning.parser_version
                    == input::EXPLAINED_CODE_SINGLE_CASE_PARSER_VERSION =>
            {
                let Some(prior) = previous.filter(|prior| prior.lifecycle == Lifecycle::Commit)
                else {
                    return self.stop(
                        &mut state,
                        Lifecycle::Suspend,
                        "MISSING_EXPLAINED_CODE_CONTEXT",
                        "同じ会話で確定した貼り付けコードの説明を選んでください。",
                    );
                };
                let deadline =
                    started.checked_add(Duration::from_millis(request.budget.deadline_ms));
                let verified = match crate::code_generation::verify_explained_code(
                    &self.store,
                    &state,
                    &bundle,
                    prior,
                    deadline,
                ) {
                    Ok(verified) => verified,
                    Err(Error::Contract { code, message }) => {
                        return self.stop(&mut state, Lifecycle::Suspend, &code, &message);
                    }
                    Err(error) => return Err(error),
                };
                let reference =
                    self.store
                        .put_artifact(actor, &request.case_id, &verified.report)?;
                result.dependencies = verified.dependencies(reference);
                result.guarantee = Guarantee::Computed;
                units = verified.units;
                report_goal_content = Some(ReportGoalContent {
                    predicate: crate::code_generation::EXPLAINED_CODE_VERIFICATION_PREDICATE,
                    guarantee: Guarantee::Computed,
                    dependencies: result.dependencies.clone(),
                    units: units.clone(),
                });
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
                let deadline =
                    started.checked_add(Duration::from_millis(request.budget.deadline_ms));
                let source = match crate::code_generation::previous_code_verification(
                    &self.store,
                    &state,
                    prior,
                    deadline,
                ) {
                    Ok(source) => source,
                    Err(Error::Contract { code, message }) => {
                        return self.stop(&mut state, Lifecycle::Suspend, &code, &message);
                    }
                    Err(error) => return Err(error),
                };
                let Some(source) = source else {
                    return self.stop(
                        &mut state,
                        Lifecycle::Suspend,
                        "MISSING_CODE_GENERATION_CONTEXT",
                        "直前の採用結果に、同一ケースで再検証できる構造化コード生成レポートがありません。",
                    );
                };
                if source.report.source_sha256 != digest(source.report.source.as_bytes()) {
                    return self.stop(
                        &mut state,
                        Lifecycle::Suspend,
                        "CODE_GENERATION_CONTEXT_MISMATCH",
                        "直前の生成レポートのソースハッシュが保存済みコードと一致しません。",
                    );
                }
                let report = match crate::code_capability::verify_previous_source(
                    &source.report.source,
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
                    .extend(source.dependencies(verification_ref));
                units.extend(crate::code_capability::previous_code_verification_lines(
                    &report,
                )?);
                result.guarantee = Guarantee::Computed;
            }
            Intent::ExplainCode { .. } => {
                let source = input::verified_code_explanation_source(&bundle)?;
                let (report, explanation) = match crate::code_capability::explain_source(source) {
                    Ok(explanation) => explanation,
                    Err(Error::Contract { code, message }) => {
                        return self.stop(
                            &mut state,
                            Lifecycle::Suspend,
                            &code,
                            &format!("このPythonコードの説明を保留しました。{message}"),
                        );
                    }
                    Err(error) => return Err(error),
                };
                let report_bytes = canonical(&report)?;
                if report_bytes
                    .len()
                    .saturating_add(explanation.join("\n").len())
                    > goal.budget.max_evidence_bytes
                {
                    return self.stop(
                        &mut state,
                        Lifecycle::Suspend,
                        "RESOURCE_LIMIT",
                        "コードの解析結果と説明が証拠予算を超えました。",
                    );
                }
                let report_ref = self
                    .store
                    .put_bytes(actor, &request.case_id, &report_bytes)?;
                result.dependencies.push(report_ref);
                result.guarantee = Guarantee::Computed;
                units = explanation;
                report_goal_content = Some(ReportGoalContent {
                    predicate: CODE_EXPLANATION_PREDICATE,
                    guarantee: result.guarantee.clone(),
                    dependencies: result.dependencies.clone(),
                    units: units.clone(),
                });
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
                let object_keys_only =
                    bundle.meaning.parser_version == input::JSON_OBJECT_KEY_PATH_PARSER_VERSION;
                match ports::json_pointer_with_mode(document, pointer, object_keys_only) {
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
            intent @ (Intent::InspectFactSource { .. }
            | Intent::InspectConditionSource { .. }
            | Intent::InspectConditionSourceByDefinition { .. }
            | Intent::InspectRuleSource { .. }
            | Intent::InspectDeclaredRuleSource { .. }
            | Intent::InspectPreviousRuleSource
            | Intent::InspectPreviousComposedRuleSource
            | Intent::InspectPreviousRuleSourceById { .. }
            | Intent::ListPreviousRules
            | Intent::AskListedRuleSource { .. }
            | Intent::InspectComposedRuleSource { .. }) => {
                let report = check_record_source_actions(&goal).and_then(|()| {
                    if matches!(intent, Intent::ListPreviousRules) {
                        return previous_rule_list(
                            &self.store,
                            &bundle,
                            &state.prior_results,
                            &goal,
                        )
                        .map(|readout| (None, readout));
                    }
                    let target = record_source_target(&self.store, &bundle, &state.prior_results)?;
                    let memory = record_source_memory_at_head(&self.store, actor)?;
                    let record = lookup_source_record(&memory, &target.lookup)?;
                    let readout =
                        target.readout(record, &self.store, actor, memory.revision, &goal)?;
                    Ok((Some(memory.revision), readout))
                });
                let (revision, readout) = match report {
                    Ok(report) => report,
                    Err(Error::Contract { code, message }) => {
                        return self.stop(&mut state, Lifecycle::Suspend, &code, &message);
                    }
                    Err(error) => return Err(error),
                };
                let proof_ref = self
                    .store
                    .put_bytes(actor, &request.case_id, &readout.proof)?;
                result.actor_memory_revision = revision;
                result.dependencies = readout.dependencies;
                result.dependencies.push(proof_ref);
                result.guarantee = Guarantee::Computed;
                units = readout.units;
                report_goal_content = Some(ReportGoalContent {
                    predicate: record_source_contract(intent)
                        .expect("record source intent")
                        .1,
                    guarantee: Guarantee::Computed,
                    dependencies: result.dependencies.clone(),
                    units: units.clone(),
                });
            }
            intent @ (Intent::EvaluateCondition { .. }
            | Intent::EvaluateConditionDetailed { .. }) => {
                let report =
                    condition_report_at_head(&self.store, actor, &goal).and_then(|report| {
                        report.verify_evidence_budget(goal.budget.max_evidence_bytes)?;
                        Ok(report)
                    });
                let report = match report {
                    Ok(report) => report,
                    Err(Error::Contract { code, message }) => {
                        return self.stop(&mut state, Lifecycle::Suspend, &code, &message);
                    }
                    Err(error) => return Err(error),
                };
                let proof =
                    self.store
                        .put_bytes(actor, &request.case_id, &report.proof_bytes()?)?;
                result.actor_memory_revision = Some(report.revision);
                result.dependencies = report.evaluation.dependencies;
                result.dependencies.push(proof);
                result.guarantee = Guarantee::Computed;
                units = report.units;
                report_goal_content = Some(ReportGoalContent {
                    predicate: condition_evaluation_contract(intent)
                        .expect("condition evaluation intent")
                        .1,
                    guarantee: Guarantee::Computed,
                    dependencies: result.dependencies.clone(),
                    units: units.clone(),
                });
            }
            Intent::DefineCondition { definition } => {
                let _ = direct_condition_input(&bundle, definition)?;
                let mut memory = self.store.load_memory(actor)?;
                let condition =
                    relations::define_condition(&mut memory, definition.clone(), &fingerprint)?;
                self.store.save_memory(actor, &memory)?;
                units.push(format!(
                    "条件定義をCore記憶へ保存しました（条件ID: {}; 意味版: {}）。",
                    condition.definition.condition_id, condition.semantics_version
                ));
                units.push(format!(
                    "適用主体: {}; scope: {}; 依存条件数: {}。",
                    condition.definition.subject,
                    condition.definition.scope,
                    condition.definition.dependencies.len()
                ));
                result.dependencies.push(condition.id);
                result.guarantee = Guarantee::UserPremise;
            }
            Intent::Greet { salutation } => {
                units.push(format!("{salutation}。ミニドラです。"));
                units.push(
                    if previous.is_some_and(|result| result.lifecycle == Lifecycle::Commit) {
                        "会話の続きを聞かせてください。新しい質問もどうぞ。".into()
                    } else {
                        "質問したいことや、一緒に考えたいことを教えてください。".into()
                    },
                );
                result.guarantee = Guarantee::Computed;
            }
            Intent::AskLocalIdentity => {
                let engine = Self::capabilities();
                units.extend([
                    "ミニドラは、この端末で日本語の会話や推論、計算を行うRustエンジンです。".into(),
                    format!(
                        "この実行版は {}、標準APIは {} です。",
                        engine.version, engine.api_version
                    ),
                    "教えてもらった事実や規則を、同じ利用者との後続の会話で使えます。".into(),
                ]);
                result.guarantee = Guarantee::Computed;
            }
            Intent::AskLocalCapabilities => {
                units.extend([
                    "計算や、教えてもらった事実と規則を使った推論ができます。".into(),
                    "例えば『17×23はいくつ？』や『ルリは鳥です』と入力してください。".into(),
                    "答えが出たあとに『詳しく説明して』と聞くと、計算手順や根拠をたどれます。"
                        .into(),
                ]);
                result.guarantee = Guarantee::Computed;
            }
            Intent::Unknown { reason } => {
                return self.stop(
                    &mut state,
                    Lifecycle::Suspend,
                    "CAPABILITY_UNAVAILABLE",
                    &format!("入力を確定できないため保留しました。{reason}"),
                );
            }
            Intent::Cancel => unreachable!("handled before computation"),
        }
        if started.elapsed().as_millis() >= u128::from(request.budget.deadline_ms) {
            return self.stop(
                &mut state,
                Lifecycle::Suspend,
                "DEADLINE",
                "採用前に締切へ達しました。",
            );
        }
        state.result = Some(result.clone());
        let compute_artifact = if let (Some(meaning_plan), Some(content_generation)) =
            (&calculation_meaning_plan, &content_generation)
        {
            self.store.put_artifact(
                actor,
                &request.case_id,
                &GeneratedCalculationArtifact {
                    kind: GENERATED_CALCULATION_KIND,
                    meaning_plan,
                    content_generation,
                },
            )?
        } else if let (Some(relation_meaning), Some(relation_proof_dependencies)) =
            (adopted_relation_meaning, relation_proof_dependencies)
        {
            if let Some(content_generation) = content_generation.clone() {
                let Some(relation_meaning_plan) = relation_meaning_plan else {
                    return self.stop(
                        &mut state,
                        Lifecycle::Suspend,
                        "CONTENT_PLAN_ARTIFACT_MISSING",
                        "意味生成traceに結び付くCore意味計画がありません。",
                    );
                };
                self.store.put_artifact(
                    actor,
                    &request.case_id,
                    &CoreComputationArtifactGeneratedContent {
                        kind: generated_artifact_kind(applied_rule_adaptation.as_ref()).into(),
                        calculation: result.calculation.clone(),
                        units: units.clone(),
                        evidence: result.evidence.clone(),
                        dependencies: result.dependencies.clone(),
                        relation_meaning,
                        relation_proof_dependencies,
                        relation_meaning_plan,
                        content_generation,
                        adaptation: applied_rule_adaptation,
                    },
                )?
            } else {
                let (Some(content_request), Some(content_composition)) =
                    (content_request, content_composition)
                else {
                    return self.stop(
                        &mut state,
                        Lifecycle::Suspend,
                        "CONTENT_PLAN_ARTIFACT_MISSING",
                        "関係説明の内容計画traceが保存できません。",
                    );
                };
                let artifact_kind = if relation_meaning_plan.is_some() {
                    CORE_COMPUTATION_ARTIFACT_KIND
                } else {
                    CORE_COMPUTATION_ARTIFACT_KIND_V3
                };
                self.store.put_artifact(
                    actor,
                    &request.case_id,
                    &CoreComputationArtifactContent {
                        kind: artifact_kind.into(),
                        calculation: result.calculation.clone(),
                        units: units.clone(),
                        evidence: result.evidence.clone(),
                        dependencies: result.dependencies.clone(),
                        relation_meaning,
                        relation_proof_dependencies,
                        relation_meaning_plan,
                        content_request,
                        content_composition,
                    },
                )?
            }
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
            .checked_add(Duration::from_millis(goal.budget.deadline_ms))
            .ok_or_else(|| {
                failure(
                    "INVALID_DEADLINE",
                    "execution deadline overflows this clock",
                )
            })?;
        let model = if let Some(model) = generation_model.take() {
            model
        } else {
            match language::SequenceModel::train_until(&units, 2, 1, deadline) {
                Ok(model) => model,
                Err(error) => {
                    return self.stop(
                        &mut state,
                        Lifecycle::Suspend,
                        "LANGUAGE_RESOURCE_OR_COVERAGE",
                        &error.to_string(),
                    );
                }
            }
        };
        let emission = if let Some(emission) = generation_emission.take() {
            if content_generation.as_ref().is_none_or(|report| {
                report.text != emission.text
                    || report.model_revision != emission.model_revision
                    || report.lossless_projection != emission.lossless_projection
            }) {
                return self.stop(
                    &mut state,
                    Lifecycle::Fail,
                    "MEANING_GENERATION_TRACE_MISMATCH",
                    "生成traceと同じCoreが出力するbyte列が一致しません。",
                );
            }
            emission
        } else {
            let surfaces = approved_surfaces.unwrap_or_else(|| vec![units.join("\n")]);
            match model.realize_supported_until(
                &surfaces,
                request.budget.max_evidence_bytes,
                deadline,
            ) {
                Ok(value) => value,
                Err(error) => {
                    return self.stop(
                        &mut state,
                        Lifecycle::Suspend,
                        "LANGUAGE_RESOURCE_OR_COVERAGE",
                        &error.to_string(),
                    );
                }
            }
        };
        let model_artifact_ref = if let Some(report) = content_generation.as_ref() {
            let Some(model_artifact_ref) = report.model_artifact_ref.as_deref() else {
                return self.stop(
                    &mut state,
                    Lifecycle::Fail,
                    "MEANING_GENERATION_MODEL_MISSING",
                    "生成traceに永続モデルartifactへの参照がありません。",
                );
            };
            let Some(stored_model) =
                self.store
                    .get_bytes_if_present(actor, &request.case_id, model_artifact_ref)?
            else {
                return self.stop(
                    &mut state,
                    Lifecycle::Suspend,
                    "MEANING_GENERATION_MODEL_UNAVAILABLE",
                    "生成traceが参照するCoreモデルartifactを再取得できません。",
                );
            };
            let expected_model = canonical(&model)?;
            if digest(&stored_model) != model_artifact_ref || stored_model != expected_model {
                return self.stop(
                    &mut state,
                    Lifecycle::Fail,
                    "MEANING_GENERATION_MODEL_MISMATCH",
                    "保存済みCoreモデルartifactが生成traceのモデルと一致しません。",
                );
            }
            model_artifact_ref.to_owned()
        } else {
            self.store.put_artifact(actor, &request.case_id, &model)?
        };
        let model_observation_ref = self
            .store
            .put_artifact(actor, &request.case_id, &emission)?;
        let mut plan = ContentPlan {
            id: String::new(),
            text: emission.text.clone(),
            locale: request.locale.clone(),
            guarantee: result.guarantee.clone(),
            required_units: units,
            dependencies: result.dependencies.clone(),
            model_revision: emission.model_revision.clone(),
            model_artifact_ref,
            model_observation_ref,
        };
        plan.dependencies.push(compute_artifact.clone());
        if let Some(generation_report) = content_generation.as_ref()
            && let Err(error) = validate_generated_content_plan_adoption(
                &result,
                generation_report,
                &emission,
                generation_proof_ids
                    .as_deref()
                    .unwrap_or(&result.dependencies),
                &compute_artifact,
                &request.locale,
                &plan,
            )
        {
            return self.stop(
                &mut state,
                Lifecycle::Suspend,
                "CONTENT_ADOPTION_MISMATCH",
                &error.to_string(),
            );
        }
        if let Err(error) = verify_goal_content(&goal, &result, &plan) {
            return self.stop(
                &mut state,
                Lifecycle::Suspend,
                "GOAL_CALCULATION_MISMATCH",
                &error.to_string(),
            );
        }
        if let Err(Error::Contract { code, message }) = verify_report_goal_content(
            &goal,
            &result,
            &plan,
            &compute_artifact,
            report_goal_content.as_ref(),
        ) {
            return self.stop(&mut state, Lifecycle::Suspend, &code, &message);
        }
        if let Some(observed) = sequence_result
            && let Err(error) = observed.verify_content(&result, &plan)
        {
            return self.stop(
                &mut state,
                Lifecycle::Suspend,
                "GOAL_CALCULATION_SEQUENCE_MISMATCH",
                &error.to_string(),
            );
        }
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
        if Instant::now() >= deadline {
            return self.stop(
                &mut state,
                Lifecycle::Suspend,
                "DEADLINE",
                "描画前に元Goalの締切へ達しました。",
            );
        }
        let target = match &goal.output_path {
            Some(path) => RenderTarget::LocalFile { path: path.clone() },
            None => RenderTarget::Display,
        };
        let render = output::render_to(&plan, target)?;
        if !validate_goal_render(&goal, &plan, &render) {
            return self.stop(
                &mut state,
                Lifecycle::Fail,
                "RENDER_SEMANTIC_MISMATCH",
                "描画の版、対象、実バイトが元GoalとCoreの採用内容に一致しません。",
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
        #[cfg(test)]
        let deadline = self.output_deadline_at(OutputCheckpoint::Rendered, deadline);
        if Instant::now() >= deadline {
            return self.stop(
                &mut state,
                Lifecycle::Suspend,
                "DEADLINE",
                "描画の照合後に元Goalの締切へ達しました。",
            );
        }
        // Required file effect is performed once under a durable, fully bound
        // identity. Physical output never independently closes the case.
        if let Some(ref path) = goal.output_path {
            if !graph.ready().iter().any(|id| id == "file") {
                return self.stop(
                    &mut state,
                    Lifecycle::Suspend,
                    "ACTION_BUDGET_EXHAUSTED",
                    "ファイル作用を実行する予算がありません。",
                );
            }
            if let Some((code, message)) = required_file_permission_stop(&goal) {
                return self.stop(&mut state, Lifecycle::Suspend, code, message);
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
                .put_artifact(actor, &request.case_id, &goal.permissions)?;
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
            for phase in [
                EffectPhase::Proposed,
                EffectPhase::Authorized,
                EffectPhase::Prepared,
                EffectPhase::Attempted,
            ] {
                #[cfg(test)]
                let deadline =
                    self.output_deadline_at(OutputCheckpoint::FilePhase(phase), deadline);
                action.phase = phase;
                match self
                    .store
                    .save_live_file_phase(actor, &action, state.revision, deadline)
                {
                    Ok(()) => {}
                    Err(Error::Contract { code, .. }) if code == "DEADLINE" => {
                        // The timed-out phase was not committed. Close any
                        // preceding, unattempted phases of this same action.
                        if phase != EffectPhase::Proposed {
                            action.phase = EffectPhase::Cancelled;
                            self.store.save_action(actor, &action)?;
                        }
                        return self.stop(
                            &mut state,
                            Lifecycle::Suspend,
                            "DEADLINE",
                            "ファイル作用の発行前に元Goalの締切へ達しました。",
                        );
                    }
                    Err(error) => return Err(error),
                }
            }
            #[cfg(test)]
            let deadline = self.output_deadline_at(OutputCheckpoint::FileDelivery, deadline);
            let receipt = match output::write_file_until(&action, &render.bytes, deadline) {
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
                if Instant::now() >= deadline {
                    return self.stop(
                        &mut state,
                        Lifecycle::Suspend,
                        "DEADLINE",
                        "締切時点のファイル作用の記録を保持しました。",
                    );
                }
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
                || !output::matches_file_bytes(path, &render.bytes)
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
        result.execution_graph_ref = Some(store_execution_graph(
            &self.store,
            actor,
            &request.case_id,
            &graph,
        )?);
        #[cfg(test)]
        let deadline = self.output_deadline_at(OutputCheckpoint::Commit, deadline);
        if Instant::now() >= deadline {
            state.result = Some(result);
            return self.stop(
                &mut state,
                Lifecycle::Suspend,
                "DEADLINE",
                "元Goalの締切に達したため、得られた成果と作用記録を保持しました。",
            );
        }
        result.lifecycle = Lifecycle::Commit;
        state.result = Some(result.clone());
        #[cfg(test)]
        let deadline = self.output_deadline_at(OutputCheckpoint::CommitPersistence, deadline);
        match self.advance_before(&mut state, Lifecycle::Commit, None, Some(deadline)) {
            Ok(()) => {}
            Err(Error::Contract { code, .. }) if code == "DEADLINE" => {
                return self.stop(
                    &mut state,
                    Lifecycle::Suspend,
                    "DEADLINE",
                    "終端を保存する前に元Goalの締切に達しました。得られた成果と作用記録を保持しました。",
                );
            }
            Err(error) => return Err(error),
        }
        Ok(result)
    }
    fn cancel_prior_input(&self, state: &mut CaseRevision, journal_reason: String) -> Result<()> {
        let fingerprint = state.input_fingerprint.as_deref().ok_or_else(|| {
            failure(
                "INPUT_NOT_REGISTERED",
                "the cancelled revision has no original input",
            )
        })?;
        let registered;
        let original = match &state.bundle {
            Some(bundle) => &bundle.request,
            None => {
                registered = self.store.registered_request_by_fingerprint(
                    &state.actor_id,
                    &state.case_id,
                    fingerprint,
                )?;
                &registered
            }
        };
        if original.actor_id != state.actor_id
            || original.case_id != state.case_id
            || identity(original)? != fingerprint
        {
            return Err(failure(
                "INTEGRITY_FAILURE",
                "the cancelled input differs from the case revision identity",
            ));
        }
        let result = state.result.get_or_insert_with(|| base_result(original));
        if result.id != fingerprint
            || result.input_id != original.input_id
            || result.input_version != original.input_version
        {
            return Err(failure(
                "INTEGRITY_FAILURE",
                "the cancelled result differs from the original input identity",
            ));
        }
        let message =
            "この依頼の取消を記録しました。既発生・未確定の作用は消去せず保持しています。";
        if result.reason.is_none() {
            result.reason = Some(format!("AUTHORIZED_CANCEL: {message}"));
        }
        if result.text.is_empty() {
            result.text = message.into();
        }
        self.advance(state, Lifecycle::Cancel, Some(journal_reason))
    }

    fn advance(
        &self,
        state: &mut CaseRevision,
        next: Lifecycle,
        reason: Option<String>,
    ) -> Result<()> {
        self.advance_before(state, next, reason, None)
    }

    fn advance_before(
        &self,
        state: &mut CaseRevision,
        next: Lifecycle,
        reason: Option<String>,
        deadline: Option<Instant>,
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
        self.store
            .append_case_before(&state.actor_id, &updated, deadline)?;
        *state = updated;
        Ok(())
    }
    fn stop_unresolved_relation(
        &self,
        state: &mut CaseRevision,
        result: &mut ResultRecord,
        memory: &MemoryState,
        proposition: &Proposition,
        max_actions: u32,
        missing_message: &str,
    ) -> Result<ResultRecord> {
        let out_of_scope =
            match relations::direct_out_of_scope_premises(memory, proposition, max_actions) {
                Ok(facts) => facts,
                Err(Error::Contract { code, message }) if code == "RELATION_BUDGET" => {
                    return self.stop(state, Lifecycle::Suspend, "RESOURCE_LIMIT", &message);
                }
                Err(error) => return Err(error),
            };
        self.stop_unresolved_relation_with_facts(
            state,
            result,
            proposition,
            &out_of_scope,
            missing_message,
        )
    }

    fn stop_unresolved_relation_with_facts(
        &self,
        state: &mut CaseRevision,
        result: &mut ResultRecord,
        proposition: &Proposition,
        out_of_scope: &[Fact],
        missing_message: &str,
    ) -> Result<ResultRecord> {
        if out_of_scope.is_empty() {
            return self.stop(
                state,
                Lifecycle::Suspend,
                "MISSING_PREMISES",
                missing_message,
            );
        }

        result
            .dependencies
            .extend(out_of_scope.iter().map(|fact| fact.id.clone()));
        result.dependencies.sort();
        result.dependencies.dedup();
        state.result = Some(result.clone());
        self.stop(
            state,
            Lifecycle::Suspend,
            "OUT_OF_SCOPE",
            &format!(
                "範囲「{}」には適用できる直接前提がなく、同一命題の直接記録は別scopeにあります。根拠IDを依存記録へ保持しました。",
                proposition.scope
            ),
        )
    }
    fn suspend_redirect_retry(
        &self,
        state: &mut CaseRevision,
        result: ResultRecord,
        redirect_wait: Option<&str>,
    ) -> Result<ResultRecord> {
        state.result = Some(result);
        let (code, message) = match redirect_wait {
            Some(reason) => (ports::RETRY_DEFERRED, reason),
            None => (
                ports::REDIRECT_RETRY_DEFERRED,
                "Retry-After の待機条件を満たしたと確認できないため、転送先への接続を停止しました。",
            ),
        };
        self.stop(state, Lifecycle::Suspend, code, message)
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

pub(crate) fn supports_japanese_output_locale(locale: &str) -> bool {
    matches!(locale, "ja" | "ja-JP")
}

fn validate_direct_relation_request(actor: &str, request: &RequestEnvelope) -> Result<()> {
    if actor != request.actor_id {
        return Err(failure(
            "ACTOR_MISMATCH",
            "authenticated actor and request actor differ",
        ));
    }
    if request.context_ref.is_some() {
        return Err(failure(
            "DIRECT_CONTEXT_UNSUPPORTED",
            "typed relation construction requires an explicit proposition and proof context",
        ));
    }
    if !request.text.is_empty() {
        return Err(failure(
            "DIRECT_TEXT_FORBIDDEN",
            "typed relation construction accepts typed fields, not caller-supplied prose",
        ));
    }
    if !supports_japanese_output_locale(&request.locale) {
        return Err(failure(
            "DIRECT_LOCALE_UNSUPPORTED",
            "typed relation construction currently supports Japanese output",
        ));
    }
    Ok(())
}

fn direct_typed_relation_bundle(
    request: &RequestEnvelope,
    source_kind: &str,
    parser_version: &str,
    role: &str,
    intent: Intent,
) -> Result<InputBundle> {
    let context_sha256 = identity(&None::<&ResultRecord>)?;
    let raw_sha256 = digest(request.text.as_bytes());
    let source_span = SourceSpan {
        start: 0,
        end: request.text.len(),
        text: request.text.clone(),
    };
    let meaning = MeaningIR {
        parser_version: parser_version.into(),
        intent,
        spans: vec![source_span.clone()],
        source_roles: BTreeMap::from([(role.into(), vec![source_span])]),
        unresolved: Vec::new(),
        source_kind: source_kind.into(),
        context_result_ref: None,
    };
    let fingerprint = identity(&(request, &raw_sha256, &context_sha256, parser_version))?;
    let semantic_signature = identity(&meaning)?;
    Ok(InputBundle {
        request: request.clone(),
        raw_sha256,
        context_sha256,
        fingerprint,
        meaning,
        semantic_signature,
    })
}

fn direct_typed_relation_assertion_bundle(
    request: &RequestEnvelope,
    input: &DirectTypedRelationAssertionInput,
) -> Result<InputBundle> {
    direct_typed_relation_bundle(
        request,
        DIRECT_TYPED_RELATION_ASSERTION_KIND,
        DIRECT_TYPED_RELATION_ASSERTION_PARSER,
        "typed-relation-assertion",
        Intent::Assert {
            proposition: input.proposition.clone(),
        },
    )
}

fn direct_typed_relation_correction_bundle(
    request: &RequestEnvelope,
    input: &DirectTypedRelationCorrectionInput,
) -> Result<InputBundle> {
    direct_typed_relation_bundle(
        request,
        DIRECT_TYPED_RELATION_CORRECTION_KIND,
        DIRECT_TYPED_RELATION_CORRECTION_PARSER,
        "typed-relation-correction",
        Intent::CorrectPremise {
            superseded: input.superseded.clone(),
            replacement: input.replacement.clone(),
        },
    )
}

fn direct_typed_relation_rule_bundle(
    request: &RequestEnvelope,
    input: &DirectTypedRelationRuleInput,
) -> Result<InputBundle> {
    let intent = match input.premises.as_slice() {
        [premise] => Intent::AddRule {
            premise: premise.clone(),
            conclusion: input.conclusion.clone(),
        },
        [_, _] => Intent::AddConjunctiveRule {
            premises: input.premises.clone(),
            conclusion: input.conclusion.clone(),
        },
        _ => {
            return Err(failure(
                "UNSUPPORTED_RULE_PREMISE_COUNT",
                "a typed relation rule requires one or two explicit premises",
            ));
        }
    };
    direct_typed_relation_bundle(
        request,
        DIRECT_TYPED_RELATION_RULE_KIND,
        DIRECT_TYPED_RELATION_RULE_PARSER,
        "typed-relation-rule",
        intent,
    )
}

fn direct_condition_bundle(
    request: &RequestEnvelope,
    input: &DirectTypedConditionInput,
) -> Result<InputBundle> {
    direct_typed_relation_bundle(
        request,
        DIRECT_TYPED_CONDITION_KIND,
        DIRECT_TYPED_CONDITION_PARSER,
        "typed-condition-definition",
        Intent::DefineCondition {
            definition: input.definition.clone(),
        },
    )
}

fn direct_condition_input(
    bundle: &InputBundle,
    definition: &ConditionDefinition,
) -> Result<Option<DirectTypedConditionInput>> {
    if bundle.meaning.source_kind != DIRECT_TYPED_CONDITION_KIND {
        return Ok(None);
    }
    let input: DirectTypedConditionInput = serde_json::from_str(&bundle.request.text)?;
    if input.schema != DIRECT_TYPED_CONDITION_KIND
        || input.definition != *definition
        || serde_json::to_string(&input)? != bundle.request.text
        || direct_condition_bundle(&bundle.request, &input)? != *bundle
    {
        return Err(failure(
            "TYPED_CONDITION_SEAL_INVALID",
            "typed condition definition or sealed Core input differs",
        ));
    }
    Ok(Some(input))
}

fn direct_typed_relation_assertion_input(
    bundle: &InputBundle,
    proposition: &Proposition,
) -> Result<Option<DirectTypedRelationAssertionInput>> {
    if bundle.meaning.source_kind != DIRECT_TYPED_RELATION_ASSERTION_KIND {
        return Ok(None);
    }
    let input: DirectTypedRelationAssertionInput = serde_json::from_str(&bundle.request.text)?;
    if input.schema != DIRECT_TYPED_RELATION_ASSERTION_KIND
        || input.proposition != *proposition
        || serde_json::to_string(&input)? != bundle.request.text
        || direct_typed_relation_assertion_bundle(&bundle.request, &input)? != *bundle
    {
        return Err(failure(
            "TYPED_RELATION_SEAL_INVALID",
            "typed relation assertion or sealed bundle does not match",
        ));
    }
    Ok(Some(input))
}

fn direct_typed_relation_correction_input(
    bundle: &InputBundle,
    superseded: &Proposition,
    replacement: &Proposition,
) -> Result<Option<DirectTypedRelationCorrectionInput>> {
    if bundle.meaning.source_kind != DIRECT_TYPED_RELATION_CORRECTION_KIND {
        return Ok(None);
    }
    let input: DirectTypedRelationCorrectionInput = serde_json::from_str(&bundle.request.text)?;
    if input.schema != DIRECT_TYPED_RELATION_CORRECTION_KIND
        || input.superseded != *superseded
        || input.replacement != *replacement
        || serde_json::to_string(&input)? != bundle.request.text
        || direct_typed_relation_correction_bundle(&bundle.request, &input)? != *bundle
    {
        return Err(failure(
            "TYPED_RELATION_CORRECTION_SEAL_INVALID",
            "typed relation correction or sealed bundle does not match",
        ));
    }
    Ok(Some(input))
}

fn direct_typed_relation_rule_input(
    bundle: &InputBundle,
    premises: &[Proposition],
    conclusion: &Proposition,
) -> Result<Option<DirectTypedRelationRuleInput>> {
    if bundle.meaning.source_kind != DIRECT_TYPED_RELATION_RULE_KIND {
        return Ok(None);
    }
    let input: DirectTypedRelationRuleInput = serde_json::from_str(&bundle.request.text)?;
    if input.schema != DIRECT_TYPED_RELATION_RULE_KIND
        || input.premises.as_slice() != premises
        || input.conclusion != *conclusion
        || serde_json::to_string(&input)? != bundle.request.text
        || direct_typed_relation_rule_bundle(&bundle.request, &input)? != *bundle
    {
        return Err(failure(
            "TYPED_RELATION_SEAL_INVALID",
            "typed relation rule or sealed bundle does not match",
        ));
    }
    Ok(Some(input))
}

fn direct_atomic_meaning_bundle(
    request: &RequestEnvelope,
    input: &DirectAtomicMeaningInput,
) -> Result<InputBundle> {
    let parser_version = DIRECT_ATOMIC_MEANING_PARSER;
    let context_sha256 = identity(&None::<&ResultRecord>)?;
    let raw_sha256 = digest(request.text.as_bytes());
    let source_span = SourceSpan {
        start: 0,
        end: request.text.len(),
        text: request.text.clone(),
    };
    let meaning = MeaningIR {
        parser_version: parser_version.into(),
        intent: Intent::AskWhy {
            proposition: input.proposition.clone(),
        },
        spans: vec![source_span.clone()],
        source_roles: BTreeMap::from([("atomic-meaning".into(), vec![source_span])]),
        unresolved: Vec::new(),
        source_kind: DIRECT_ATOMIC_MEANING_KIND.into(),
        context_result_ref: None,
    };
    let fingerprint = identity(&(request, &raw_sha256, &context_sha256, parser_version))?;
    let semantic_signature = identity(&meaning)?;
    Ok(InputBundle {
        request: request.clone(),
        raw_sha256,
        context_sha256,
        fingerprint,
        meaning,
        semantic_signature,
    })
}

fn direct_relation_comparison_bundle(
    request: &RequestEnvelope,
    input: &DirectRelationComparisonInput,
) -> Result<InputBundle> {
    let parser_version = DIRECT_RELATION_COMPARISON_PARSER;
    let context_sha256 = identity(&None::<&ResultRecord>)?;
    let raw_sha256 = digest(request.text.as_bytes());
    let source_span = SourceSpan {
        start: 0,
        end: request.text.len(),
        text: request.text.clone(),
    };
    let meaning = MeaningIR {
        parser_version: parser_version.into(),
        intent: Intent::CompareRelationInventories {
            subject_a: input.subject_a.clone(),
            subject_b: input.subject_b.clone(),
            scope: input.scope.clone(),
            format: None,
        },
        spans: vec![source_span.clone()],
        source_roles: BTreeMap::from([("relation-comparison".into(), vec![source_span])]),
        unresolved: Vec::new(),
        source_kind: DIRECT_RELATION_COMPARISON_KIND.into(),
        context_result_ref: None,
    };
    let fingerprint = identity(&(request, &raw_sha256, &context_sha256, parser_version))?;
    let semantic_signature = identity(&meaning)?;
    Ok(InputBundle {
        request: request.clone(),
        raw_sha256,
        context_sha256,
        fingerprint,
        meaning,
        semantic_signature,
    })
}

fn direct_relation_comparison_input(
    bundle: &InputBundle,
    subject_a: &str,
    subject_b: &str,
    scope: &str,
) -> Result<Option<DirectRelationComparisonInput>> {
    if bundle.meaning.source_kind != DIRECT_RELATION_COMPARISON_KIND {
        return Ok(None);
    }
    let input: DirectRelationComparisonInput = serde_json::from_str(&bundle.request.text)?;
    if input.schema != DIRECT_RELATION_COMPARISON_KIND
        || input.purpose != "compare-relations"
        || input.subject_a != subject_a
        || input.subject_b != subject_b
        || input.scope != scope
        || input.subject_a == input.subject_b
        || input.source_ids_a.windows(2).any(|pair| pair[0] >= pair[1])
        || input.source_ids_b.windows(2).any(|pair| pair[0] >= pair[1])
        || serde_json::to_string(&input)? != bundle.request.text
        || direct_relation_comparison_bundle(&bundle.request, &input)? != *bundle
    {
        return Err(failure(
            "RELATION_COMPARISON_SEAL_INVALID",
            "typed relation comparison, source references, or sealed bundle do not match",
        ));
    }
    Ok(Some(input))
}

fn direct_atomic_meaning_input(
    bundle: &InputBundle,
    proposition: &Proposition,
) -> Result<Option<DirectAtomicMeaningInput>> {
    if bundle.meaning.source_kind != DIRECT_ATOMIC_MEANING_KIND {
        return Ok(None);
    }
    let input: DirectAtomicMeaningInput = serde_json::from_str(&bundle.request.text)?;
    if input.schema != DIRECT_ATOMIC_MEANING_KIND
        || input.purpose != "explain"
        || input.proposition != *proposition
        || input.source_ids.windows(2).any(|pair| pair[0] >= pair[1])
        || serde_json::to_string(&input)? != bundle.request.text
        || direct_atomic_meaning_bundle(&bundle.request, &input)? != *bundle
    {
        return Err(failure(
            "ATOMIC_MEANING_SEAL_INVALID",
            "typed atomic meaning, source references, or sealed bundle do not match",
        ));
    }
    Ok(Some(input))
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
    propositions: (&Proposition, &Proposition),
    fingerprint: &str,
    source_evidence: Option<SealedSourceEvidence>,
) -> Result<(Vec<String>, usize)> {
    let (superseded, replacement) = propositions;
    let original_memory_revision = memory.revision;
    let mut invalidated =
        relations::invalidate_in_order(memory, targets.iter().map(String::as_str))?;
    let negative_superseded = Proposition {
        negative: true,
        ..superseded.clone()
    };
    let negative_fact = relations::assert_fact_with_source_evidence(
        memory,
        negative_superseded,
        fingerprint,
        source_evidence.clone(),
    )?;
    let replacement_fact = relations::assert_fact_with_source_evidence(
        memory,
        replacement.clone(),
        fingerprint,
        source_evidence,
    )?;
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
const READ_CASE_CHANGED: &str = "HTTP_CASE_REVISION_CHANGED";

fn read_case_changed(error: &Error) -> bool {
    matches!(error, Error::Contract { code, .. } if code == READ_CASE_CHANGED)
}

pub(crate) fn verify_retained_retry_source(
    store: &Store,
    state: &CaseRevision,
    result: &ResultRecord,
    packet: &EvidencePacket,
    history: Option<&VerifiedCaseHistory<'_>>,
) -> Result<()> {
    let invalid = || {
        failure(
            "RETRY_AFTER_SOURCE_INVALID",
            "Saved Retry-After does not match the case's observed failure artifact",
        )
    };
    let read_input = || match history {
        Some(history) => history.input_bundle(&result.input_id, result.input_version),
        None => store.get_input_bundle(
            &state.actor_id,
            &state.case_id,
            &result.input_id,
            result.input_version,
        ),
    };
    let requested = url::Url::parse(&packet.request_uri).map_err(|_| invalid())?;
    if packet.raw_sha256 != digest(&packet.raw_body)
        || packet.redirect_chain.first().map(String::as_str) != Some(requested.as_str())
        || packet.redirect_chain.last() != Some(&packet.final_uri)
    {
        return Err(invalid());
    }
    if packet.packet_kind == "SEARCH_LIST" {
        let index = result
            .evidence
            .iter()
            .position(|evidence| evidence == packet)
            .ok_or_else(invalid)?;
        let reference = result.dependencies.get(index).ok_or_else(invalid)?;
        let bytes = store.get_bytes(&state.actor_id, &state.case_id, reference)?;
        let observation: search::SearchObservation = serde_json::from_slice(&bytes)?;
        if observation.listing != *packet || canonical(&observation)? != bytes {
            return Err(invalid());
        }
        search::validate_failed_observation(&observation, None)?;
        if result.lifecycle == Lifecycle::Cancel {
            let (_, bundle) = read_input()?.ok_or_else(invalid)?;
            let plan = search::search_plan(&bundle.meaning.intent)?.ok_or_else(invalid)?;
            if identity(&bundle.request)? != result.id
                || plan.fallback.is_none()
                || plan.primary != observation.provider
                || plan.query != observation.query
                || !search::allows_fallback(&observation)
                || result.evidence.len() != 1
                || result.dependencies.len() != 1
            {
                return Err(invalid());
            }
            search::validate_failed_observation(&observation, Some(&bundle.request.permissions))?;
        }
        if result.lifecycle == Lifecycle::Commit
            && search::previous_observation_with_history(
                store,
                &state.actor_id,
                &state.case_id,
                result,
                history,
            )?
            .is_none()
        {
            return Err(invalid());
        }
    } else {
        let bytes = canonical(packet)?;
        let reference = digest(&bytes);
        if packet.provider_id != "https-direct-url-v1"
            || packet.id != ports::fetch_packet_identity(packet)?
            || !result
                .dependencies
                .iter()
                .any(|id| *id == packet.id || *id == reference)
            || store.get_bytes(&state.actor_id, &state.case_id, &reference)? != bytes
        {
            return Err(invalid());
        }
        if result.lifecycle == Lifecycle::Cancel {
            let (_, bundle) = read_input()?.ok_or_else(invalid)?;
            let continuation;
            let (urls, dependencies) = match &bundle.meaning.intent {
                Intent::Retrieve { urls, .. } => (urls.as_slice(), result.dependencies.as_slice()),
                Intent::FetchNextUnobservedSource => {
                    let previous = bundle
                        .meaning
                        .context_result_ref
                        .as_ref()
                        .and_then(|id| state.prior_results.get(id));
                    let pending = crate::recovery::next_direct_retrieval(
                        store,
                        &state.actor_id,
                        &bundle,
                        previous,
                        &state.prior_results,
                        history,
                    )?
                    .ok_or_else(invalid)?;
                    let Some((reference, dependencies)) = result.dependencies.split_first() else {
                        return Err(invalid());
                    };
                    if reference != &pending.source.id {
                        return Err(invalid());
                    }
                    continuation = pending.uri;
                    (std::slice::from_ref(&continuation), dependencies)
                }
                _ => return Err(invalid()),
            };
            if identity(&bundle.request)? != result.id
                || result.evidence.len() != dependencies.len()
                || result.evidence.len() > urls.len()
                || result.evidence.iter().zip(dependencies).zip(urls).any(
                    |((observation, reference), uri)| {
                        observation.request_uri != *uri || observation.id != *reference
                    },
                )
            {
                return Err(invalid());
            }
            ports::validate_observed_route(packet, &bundle.request.permissions)?;
        }
    }
    Ok(())
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
        actor_memory_revision: None,
        execution_graph_ref: None,
    }
}

pub(crate) fn direct_source_line(packet: &EvidencePacket) -> String {
    format!(
        "出典: {}\n本文（出典報告）:\n{}",
        packet.final_uri,
        packet.extracted_text.as_deref().unwrap_or_default()
    )
}

#[derive(serde::Serialize)]
pub(crate) struct DirectSourceDisplay<'a> {
    operation: &'static str,
    sources: Vec<DirectSourceExcerpt<'a>>,
}

#[derive(serde::Serialize)]
struct DirectSourceExcerpt<'a> {
    packet_id: &'a str,
    final_uri: &'a str,
    observed_at_unix_ms: u64,
    raw_body_bytes: usize,
    raw_body_sha256: &'a str,
    source_start: usize,
    source_end: usize,
    source_span_sha256: String,
    text: &'a str,
}

pub(crate) fn direct_source_display(packets: &[EvidencePacket]) -> DirectSourceDisplay<'_> {
    const PER_SOURCE_QUOTE_BYTES: usize = 512;
    const TOTAL_QUOTE_BYTES: usize = 4_096;
    let each = PER_SOURCE_QUOTE_BYTES.min(TOTAL_QUOTE_BYTES / packets.len().max(1));
    let sources = packets
        .iter()
        .map(|packet| {
            let text = packet.extracted_text.as_deref().unwrap_or_default();
            let source_end = text.floor_char_boundary(text.len().min(each));
            let excerpt = &text[..source_end];
            DirectSourceExcerpt {
                packet_id: &packet.id,
                final_uri: &packet.final_uri,
                observed_at_unix_ms: packet.observed_at_unix_ms,
                raw_body_bytes: packet.raw_body.len(),
                raw_body_sha256: &packet.raw_sha256,
                source_start: 0,
                source_end,
                source_span_sha256: digest(excerpt.as_bytes()),
                text: excerpt,
            }
        })
        .collect();
    DirectSourceDisplay {
        operation: "minidora.direct-source-display.v1",
        sources,
    }
}

impl DirectSourceDisplay<'_> {
    pub(crate) fn source_ranges(&self) -> impl Iterator<Item = (usize, usize, &str)> {
        self.sources.iter().map(|source| {
            (
                source.source_start,
                source.source_end,
                source.source_span_sha256.as_str(),
            )
        })
    }

    pub(crate) fn units(&self) -> Vec<String> {
        let mut units =
            vec!["取得した全文を保存しました。以下の表示は各本文の冒頭からの引用です。".into()];
        units.extend(self.sources.iter().map(|source| format!(
            "出典: {}\n取得時刻（Unix ms）: {}\n本文packet ID: {}\n原本文 {} bytes / SHA-256: {}\n引用範囲 byte[{}..{}) / SHA-256: {}\n{}",
            source.final_uri, source.observed_at_unix_ms, source.packet_id, source.raw_body_bytes, source.raw_body_sha256,
            source.source_start, source.source_end, source.source_span_sha256, source.text,
        )));
        units
    }
}

fn rejected(request: &RequestEnvelope, code: &str, message: &str) -> ResultRecord {
    let mut r = base_result(request);
    r.lifecycle = Lifecycle::Reject;
    r.reason = Some(format!("{code}: {message}"));
    r.text = message.into();
    r
}
pub(crate) fn registered_math_report_explanation(
    store: &Store,
    actor: &str,
    case: &str,
    previous: &ResultRecord,
) -> Result<Option<Vec<String>>> {
    if previous.lifecycle != Lifecycle::Commit || previous.guarantee != Guarantee::Computed {
        return Ok(None);
    }
    let Some((_, bundle)) =
        store.get_input_bundle(actor, case, &previous.input_id, previous.input_version)?
    else {
        return Ok(None);
    };
    if !matches!(
        bundle.meaning.intent,
        Intent::Statistics { .. }
            | Intent::NormalizeProbabilityWeights { .. }
            | Intent::FiniteProbabilityStatistics { .. }
            | Intent::FiniteEventProbability { .. }
            | Intent::EvaluatePolynomial { .. }
            | Intent::LinearSystem { .. }
            | Intent::QuadraticEquation { .. }
            | Intent::LinearEquation { .. }
            | Intent::Symbolic { .. }
    ) {
        return Ok(None);
    }
    if identity(&bundle.request)? != previous.id {
        return Err(failure(
            "MATH_CONTEXT_INVALID",
            "the previous mathematical result does not match its registered input identity",
        ));
    }
    crate::math::previous_math_report_explanation(previous)
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

pub(crate) struct PreviousRelationQuery {
    pub(crate) resolution: Option<PreviousRelationResolution>,
    pub(crate) memory: MemoryState,
    pub(crate) answer: Result<Option<relations::QueryOutcome>>,
}

impl PreviousRelationQuery {
    fn from_snapshot(
        memory: MemoryState,
        resolution: PreviousRelationResolution,
        max_actions: u32,
    ) -> Self {
        let answer = match &resolution {
            PreviousRelationResolution::Resolved(proposition) => {
                relations::query(&memory, proposition, max_actions).map(Some)
            }
            PreviousRelationResolution::Missing | PreviousRelationResolution::Ambiguous => Ok(None),
        };
        Self {
            resolution: Some(resolution),
            memory,
            answer,
        }
    }
}

pub(crate) enum PreviousRelationResolution {
    Resolved(Proposition),
    Missing,
    Ambiguous,
}

/// Re-evaluate a prior-result relation query from the same revision-matched
/// index transaction used by ordinary Core. `None` means the index is cold or
/// stale and the caller should use its authenticated snapshot path.
pub(crate) fn indexed_previous_relation_query_from_store(
    store: &Store,
    actor: &str,
    previous: &ResultRecord,
    proposition: &Proposition,
    explicit_scope: Option<&str>,
    max_actions: u32,
) -> Result<Option<PreviousRelationQuery>> {
    store.with_relation_index(actor, |index| {
        let attempt = (|| -> Result<PreviousRelationQuery> {
            let mut actions = 0_u32;
            let mut anchor_facts = Vec::new();
            let mut seen_dependencies = BTreeSet::new();
            for dependency in &previous.dependencies {
                if !seen_dependencies.insert(dependency.as_str()) {
                    continue;
                }
                actions = actions.checked_add(1).ok_or_else(|| {
                    failure(
                        "RELATION_BUDGET",
                        "prior relation lookup action count overflowed",
                    )
                })?;
                if actions > max_actions {
                    return Err(failure(
                        "RELATION_BUDGET",
                        "prior relation lookup exceeds the sealed action budget",
                    ));
                }
                if let Some(record) = index.record_by_id(dependency)?
                    && let crate::store::IndexedMemoryValue::Fact(fact) = record.value
                {
                    anchor_facts.push((record.ordinal, fact));
                }
            }
            anchor_facts.sort_by_key(|(ordinal, _)| *ordinal);
            let resolution_memory = MemoryState {
                revision: index.revision(),
                facts: anchor_facts.into_iter().map(|(_, fact)| fact).collect(),
                ..MemoryState::default()
            };
            let resolution = resolve_previous_relation_proposition(
                &resolution_memory,
                previous,
                proposition,
                explicit_scope,
            );
            let PreviousRelationResolution::Resolved(resolved) = &resolution else {
                return Ok(PreviousRelationQuery {
                    resolution: Some(resolution),
                    memory: resolution_memory,
                    answer: Ok(None),
                });
            };

            let projection_budget = max_actions.checked_sub(actions).ok_or_else(|| {
                failure(
                    "RELATION_BUDGET",
                    "prior relation lookup exhausted the sealed action budget",
                )
            })?;
            let (memory, projection_actions) =
                relations::active_relation_projection(index, resolved, projection_budget)?;
            actions = actions
                .checked_add(projection_actions)
                .ok_or_else(|| failure("RELATION_BUDGET", "relation action count overflowed"))?;
            let query_budget = max_actions.checked_sub(actions).ok_or_else(|| {
                failure(
                    "RELATION_BUDGET",
                    "active memory projection exhausted the relation action budget",
                )
            })?;
            let mut answer = relations::query(&memory, resolved, query_budget)?;
            actions = actions
                .checked_add(answer.steps)
                .ok_or_else(|| failure("RELATION_BUDGET", "relation action count overflowed"))?;
            answer.steps = actions;
            Ok(PreviousRelationQuery {
                resolution: Some(resolution),
                memory,
                answer: Ok(Some(answer)),
            })
        })();
        match attempt {
            Ok(result) => Ok(result),
            Err(error)
                if matches!(
                    &error,
                    Error::Contract { code, .. } if code == "RELATION_BUDGET"
                ) =>
            {
                Ok(PreviousRelationQuery {
                    resolution: None,
                    memory: MemoryState {
                        revision: index.revision(),
                        ..MemoryState::default()
                    },
                    answer: Err(error),
                })
            }
            Err(error) => Err(error),
        }
    })
}

pub(crate) fn resolve_previous_relation_proposition(
    memory: &MemoryState,
    previous: &ResultRecord,
    proposition: &Proposition,
    explicit_scope: Option<&str>,
) -> PreviousRelationResolution {
    let prior_dependencies: BTreeSet<_> =
        previous.dependencies.iter().map(String::as_str).collect();
    let mut candidates = memory.facts.iter().filter(|fact| {
        fact.valid
            && fact.guarantee == Guarantee::UserPremise
            && fact.dependencies.is_empty()
            && !fact.source_input.is_empty()
            && prior_dependencies.contains(fact.id.as_str())
            && explicit_scope.is_none_or(|scope| fact.proposition.scope == scope)
    });
    let Some(anchor) = candidates.next() else {
        return PreviousRelationResolution::Missing;
    };
    if candidates.next().is_some() {
        return PreviousRelationResolution::Ambiguous;
    }
    PreviousRelationResolution::Resolved(Proposition {
        subject: anchor.proposition.subject.clone(),
        scope: explicit_scope
            .map(str::to_owned)
            .unwrap_or_else(|| anchor.proposition.scope.clone()),
        ..proposition.clone()
    })
}

pub(crate) fn resolve_previous_relation_for_bundle(
    store: &Store,
    bundle: &InputBundle,
    prior_results: &BTreeMap<String, ResultRecord>,
    previous: &ResultRecord,
    memory: &MemoryState,
    remaining_actions: &mut u32,
) -> Result<PreviousRelationResolution> {
    let invalid = || {
        failure(
            "PRIOR_INPUT_RESULT_MISMATCH",
            "代名詞が参照する確定済みの前提と、封印された入力を照合できません。",
        )
    };
    let Intent::AskPreviousRelation {
        proposition,
        explicit_scope,
        ..
    } = &bundle.meaning.intent
    else {
        return Err(invalid());
    };
    let parser_version = &bundle.meaning.parser_version;
    if !input::is_registered_previous_relation_query(parser_version) {
        return Ok(resolve_previous_relation_proposition(
            memory,
            previous,
            proposition,
            explicit_scope.as_deref(),
        ));
    }
    let request = &bundle.request;
    if !registered_relation_context_matches(bundle, prior_results, previous)? {
        return Err(invalid());
    }
    let actor = &request.actor_id;
    let case = &request.case_id;
    if matches!(
        parser_version.as_str(),
        input::NAMED_CORRECTION_RELATION_QUERY_PARSER_VERSION
            | input::CONTEXTUAL_CORRECTION_RELATION_QUERY_PARSER_VERSION
    ) {
        let Some(correction) = active_previous_correction(
            store,
            bundle,
            prior_results,
            previous,
            memory,
            remaining_actions,
        )?
        else {
            return Ok(PreviousRelationResolution::Missing);
        };
        return Ok(PreviousRelationResolution::Resolved(Proposition {
            subject: correction.replacement.subject,
            scope: correction.replacement.scope,
            ..proposition.clone()
        }));
    }
    let derived_anchor = match parser_version.as_str() {
        input::NAMED_DERIVED_RELATION_QUERY_PARSER_VERSION => Some((
            previous_named_derived_relation_source(store, actor, case, previous)?
                .ok_or_else(invalid)?,
            false,
        )),
        input::DERIVED_RELATION_FOLLOWUP_PARSER_VERSION => {
            let followup = previous_derived_followup_question(store, actor, case, previous)?
                .ok_or_else(invalid)?;
            verify_named_origin_for_derived_followup(
                store,
                prior_results,
                &followup,
                memory,
                remaining_actions,
            )?;
            Some((followup.proposition, followup.explain))
        }
        _ => None,
    };
    if let Some((anchor, explain)) = derived_anchor {
        if explicit_scope
            .as_ref()
            .is_some_and(|scope| scope != &anchor.scope)
        {
            return Ok(PreviousRelationResolution::Missing);
        }
        return resolve_derived_relation_context(
            store,
            request,
            previous,
            memory,
            &anchor,
            explain,
            proposition,
            remaining_actions,
        );
    }
    let assertion = previous_assertion_context(
        store,
        &request.actor_id,
        &request.case_id,
        prior_results,
        previous,
    )?
    .filter(|assertion| assertion.context.is_some())
    .ok_or_else(invalid)?;
    let anchor = &assertion.active_fact(memory)?.proposition;
    if explicit_scope
        .as_ref()
        .is_some_and(|scope| scope != &anchor.scope)
    {
        return Ok(PreviousRelationResolution::Missing);
    }
    Ok(PreviousRelationResolution::Resolved(Proposition {
        subject: anchor.subject.clone(),
        scope: anchor.scope.clone(),
        ..proposition.clone()
    }))
}

fn registered_relation_context_matches(
    bundle: &InputBundle,
    prior_results: &BTreeMap<String, ResultRecord>,
    previous: &ResultRecord,
) -> Result<bool> {
    Ok(
        bundle.meaning.context_result_ref.as_ref() == Some(&previous.id)
            && bundle
                .request
                .context_ref
                .as_ref()
                .is_none_or(|id| id == &previous.id)
            && prior_results.get(&previous.id) == Some(previous)
            && bundle.context_sha256 == identity(&Some(previous))?,
    )
}

struct PreviousCorrectionContext {
    negative: Proposition,
    negative_id: String,
    replacement: Proposition,
    replacement_id: String,
    source_evidence: SealedSourceEvidence,
}

fn previous_natural_correction_input(
    store: &Store,
    actor: &str,
    case: &str,
    prior: &ResultRecord,
) -> Result<Option<InputBundle>> {
    Ok(committed_user_premise_input(store, actor, case, prior)?.filter(is_natural_correction))
}

fn committed_user_premise_input(
    store: &Store,
    actor: &str,
    case: &str,
    prior: &ResultRecord,
) -> Result<Option<InputBundle>> {
    if prior.lifecycle != Lifecycle::Commit || prior.guarantee != Guarantee::UserPremise {
        return Ok(None);
    }
    Ok(store
        .get_input_bundle(actor, case, &prior.input_id, prior.input_version)?
        .map(|(_, bundle)| bundle))
}

fn is_natural_correction(bundle: &InputBundle) -> bool {
    // The typed API may explicitly move a premise to a different scope.
    bundle.meaning.parser_version == input::PARSER_VERSION
        && matches!(
            bundle.meaning.intent,
            Intent::CorrectPremise { .. } | Intent::CorrectPreviousPremise { .. }
        )
}

fn correction_assertion_source(
    store: &Store,
    actor: &str,
    case: &str,
    prior: &ResultRecord,
) -> Result<Option<input::SourceContext>> {
    let Some(bundle) = committed_user_premise_input(store, actor, case, prior)? else {
        return Ok(None);
    };
    if is_natural_correction(&bundle) {
        return Ok(Some(input::SourceContext::CorrectionRelationAssertion));
    }
    // A second assertion can inherit the first one, but cannot itself serve
    // as the first step of another implicit continuation.
    Ok(
        (bundle.meaning.parser_version == input::CORRECTION_RELATION_ASSERTION_PARSER_VERSION
            && matches!(bundle.meaning.intent, Intent::AssertPreviousRelation { .. }))
        .then_some(input::SourceContext::CorrectionAssertionFollowup),
    )
}

fn correction_context_mismatch() -> Error {
    failure(
        "PRIOR_INPUT_RESULT_MISMATCH",
        "参照した訂正と封印された入力・記憶の事実IDを照合できません。",
    )
}

fn previous_correction_context(
    bundle: &InputBundle,
    prior: &ResultRecord,
    superseded: Proposition,
    replacement: Proposition,
) -> Result<PreviousCorrectionContext> {
    let source_evidence =
        input::assertion_source_evidence(bundle)?.ok_or_else(correction_context_mismatch)?;
    let negative = Proposition {
        negative: true,
        ..superseded
    };
    let negative_id = relations::user_premise_id(&negative, &prior.id)?;
    let replacement_id = relations::user_premise_id(&replacement, &prior.id)?;
    // apply_premise_correction appends exactly these two fact IDs, after the
    // invalidated premises, with the replacement last.
    if source_evidence.input_fingerprint != prior.id
        || !matches!(
            prior.dependencies.as_slice(),
            [_, .., neg, pos] if neg == &negative_id && pos == &replacement_id
        )
    {
        return Err(correction_context_mismatch());
    }
    Ok(PreviousCorrectionContext {
        negative,
        negative_id,
        replacement,
        replacement_id,
        source_evidence,
    })
}

pub(crate) const PREVIOUS_CORRECTION_EXPLANATION_PREDICATE: &str =
    "PREVIOUS_CORRECTION_EXPLANATION_V1";

/// Explain a registered correction from one observed memory version. Inactive
/// facts remain evidence for this historical explanation, never for inference.
pub(crate) fn previous_correction_explanation(
    store: &Store,
    state: &CaseRevision,
    followup: &InputBundle,
    prior: &ResultRecord,
    memory: &MemoryState,
    started: Instant,
) -> Result<PreviousAssertionExplanation> {
    let invalid = correction_context_mismatch;
    let request = &followup.request;
    let actor = &request.actor_id;
    let case = &request.case_id;
    let check_time = || {
        if started.elapsed() >= Duration::from_millis(request.budget.deadline_ms) {
            return Err(failure(
                "DEADLINE",
                "訂正の根拠の照合中に締切へ達しました。",
            ));
        }
        Ok(())
    };
    if actor != &state.actor_id
        || case != &state.case_id
        || !matches!(followup.meaning.intent, Intent::ExplainPrevious)
        || followup.meaning.parser_version != input::PREVIOUS_CORRECTION_EXPLANATION_PARSER_VERSION
        || !registered_relation_context_matches(followup, &state.prior_results, prior)?
        || prior.lifecycle != Lifecycle::Commit
        || prior.guarantee != Guarantee::UserPremise
        || prior.calculation.is_some()
        || !prior.evidence.is_empty()
    {
        return Err(invalid());
    }
    check_time()?;
    let [affected_ids @ .., _, _] = prior.dependencies.as_slice() else {
        return Err(invalid());
    };
    let budget_error = || {
        failure(
            "RELATION_BUDGET",
            "訂正の記憶記録を照合する手数が不足しています。",
        )
    };
    let proof_actions = u32::try_from(prior.dependencies.len()).map_err(|_| budget_error())?;
    let mut remaining_actions = request
        .budget
        .max_actions
        .checked_sub(3 + u32::from(request.output_path.is_some()))
        .and_then(|remaining| remaining.checked_sub(proof_actions))
        .ok_or_else(budget_error)?;
    let (bundle_ref, source) = store.with_verified_case_history(actor, case, |history| {
        if history
            .input_result(&prior.input_id, prior.input_version)?
            .as_ref()
            != Some(prior)
        {
            return Err(invalid());
        }
        history
            .input_bundle(&prior.input_id, prior.input_version)?
            .ok_or_else(invalid)
    })?;
    if !is_natural_correction(&source) || identity(&source.request)? != prior.id {
        return Err(invalid());
    }
    let mut anchor = None;
    let (superseded, replacement) = match &source.meaning.intent {
        Intent::CorrectPremise {
            superseded,
            replacement,
        } => (superseded.clone(), replacement.clone()),
        Intent::CorrectPreviousPremise { .. } => {
            anchor = source.meaning.context_result_ref.as_ref();
            previous_contextual_correction_target(
                store,
                &source,
                &state.prior_results,
                prior,
                memory,
                &mut remaining_actions,
            )?
        }
        _ => return Err(invalid()),
    };
    let correction = previous_correction_context(&source, prior, superseded.clone(), replacement)?;
    let premise = |id: &str, proposition: &Proposition| {
        memory
            .facts
            .iter()
            .find(|fact| {
                fact.id == id
                    && &fact.proposition == proposition
                    && fact.source_input == prior.id
                    && fact.guarantee == Guarantee::UserPremise
                    && fact.dependencies.is_empty()
                    && fact.source_evidence.as_ref() == Some(&correction.source_evidence)
            })
            .ok_or_else(invalid)
    };
    let negative = premise(&correction.negative_id, &correction.negative)?;
    let replacement = premise(&correction.replacement_id, &correction.replacement)?;
    let affected: BTreeSet<_> = affected_ids.iter().map(String::as_str).collect();
    let mut projection = MemoryState::default();
    projection.facts.extend(
        memory
            .facts
            .iter()
            .filter(|record| !record.valid && affected.contains(record.id.as_str()))
            .cloned(),
    );
    projection.rules.extend(
        memory
            .rules
            .iter()
            .filter(|record| !record.valid && affected.contains(record.id.as_str()))
            .cloned(),
    );
    projection.conditions.extend(
        memory
            .conditions
            .iter()
            .filter(|record| !record.valid && affected.contains(record.id.as_str()))
            .cloned(),
    );
    // Restrict replay to the saved affected set. A later memory version is
    // not evidence that additional records were affected by this correction.
    let roots = projection
        .facts
        .iter()
        .filter(|fact| {
            fact.proposition == superseded
                && fact.guarantee == Guarantee::UserPremise
                && fact.dependencies.is_empty()
                && !fact.source_input.is_empty()
        })
        .map(|fact| {
            if relations::user_premise_id(&fact.proposition, &fact.source_input)? != fact.id {
                return Err(invalid());
            }
            Ok(fact.id.clone())
        })
        .collect::<Result<Vec<_>>>()?;
    if roots.is_empty()
        || relations::invalidate_in_order(&mut projection, roots.iter().map(String::as_str))?
            != affected_ids
    {
        return Err(invalid());
    }
    let status = |fact: &Fact| if fact.valid { "有効" } else { "失効" };
    let mut units = vec![
        "前回の訂正を、封印された原文と保存済みの記憶から照合しました。".into(),
        format!("訂正入力: {:?}", source.request.text),
        format!(
            "訂正原文 SHA-256: {}",
            correction.source_evidence.raw_sha256
        ),
        format!("記憶版 {} で照合した状態です。", memory.revision),
        format!(
            "以前の前提: {}（範囲: {}、失効）。",
            describe(&superseded),
            superseded.scope
        ),
        format!("以前の前提ID: {}", roots.join("、")),
        format!(
            "訂正で記録した否定: {}（範囲: {}、{}）。",
            describe(&negative.proposition),
            negative.proposition.scope,
            status(negative)
        ),
        format!("否定前提ID: {}", negative.id),
        format!(
            "置き換え後の提示前提: {}（範囲: {}、{}）。",
            describe(&replacement.proposition),
            replacement.proposition.scope,
            status(replacement)
        ),
        format!("置き換え前提ID: {}", replacement.id),
        format!(
            "訂正の失効対象として記録されたID: {}",
            affected_ids.join("、")
        ),
    ];
    let mut dependencies = vec![prior.id.clone(), bundle_ref];
    dependencies.extend(prior.dependencies.iter().cloned());
    if let Some(anchor) = anchor {
        units.push(format!("主語を参照した結果ID: {anchor}"));
        dependencies.push(anchor.clone());
    }
    units.push(
        "これは提示前提と保存状態の説明で、現実世界の真偽を検証したものではありません。".into(),
    );
    if units.join("\n").len() > request.budget.max_evidence_bytes {
        return Err(failure(
            "RESOURCE_LIMIT",
            "訂正の根拠と説明が要求の証拠予算を超えています。",
        ));
    }
    check_time()?;
    Ok(PreviousAssertionExplanation {
        dependencies,
        units,
    })
}

fn previous_contextual_correction_target(
    store: &Store,
    correction: &InputBundle,
    prior_results: &BTreeMap<String, ResultRecord>,
    prior: &ResultRecord,
    memory: &MemoryState,
    remaining_actions: &mut u32,
) -> Result<(Proposition, Proposition)> {
    let Intent::CorrectPreviousPremise {
        predicate,
        superseded_object,
        replacement_object,
        scope,
    } = &correction.meaning.intent
    else {
        return Err(correction_context_mismatch());
    };
    let anchor = correction
        .meaning
        .context_result_ref
        .as_ref()
        .and_then(|id| prior_results.get(id))
        .filter(|anchor| anchor.lifecycle == Lifecycle::Commit && anchor.id != prior.id)
        .ok_or_else(correction_context_mismatch)?;
    if !registered_relation_context_matches(correction, prior_results, anchor)?
        || store
            .get_input_result(
                &correction.request.actor_id,
                &correction.request.case_id,
                &anchor.input_id,
                anchor.input_version,
            )?
            .as_ref()
            != Some(anchor)
    {
        return Err(correction_context_mismatch());
    }
    let [invalidated @ .., _, _] = prior.dependencies.as_slice() else {
        return Err(correction_context_mismatch());
    };
    let invalidated: BTreeSet<_> = invalidated.iter().map(String::as_str).collect();
    let anchor_dependencies: BTreeSet<_> = anchor.dependencies.iter().map(String::as_str).collect();
    let mut superseded = None;
    // The original subject is not written in the correction. Use only direct
    // premises in both its authenticated anchor and its recorded invalidations.
    for fact in &memory.facts {
        if fact.valid
            || fact.guarantee != Guarantee::UserPremise
            || !fact.dependencies.is_empty()
            || fact.source_input.is_empty()
            || fact.proposition.negative
            || fact.proposition.predicate != *predicate
            || fact.proposition.object != *superseded_object
            || scope
                .as_ref()
                .is_some_and(|scope| fact.proposition.scope != *scope)
            || !invalidated.contains(fact.id.as_str())
            || !anchor_dependencies.contains(fact.id.as_str())
        {
            continue;
        }
        *remaining_actions = remaining_actions.checked_sub(1).ok_or_else(|| {
            failure(
                "RELATION_BUDGET",
                "contextual correction anchor exceeds the sealed action budget",
            )
        })?;
        if relations::user_premise_id(&fact.proposition, &fact.source_input)? != fact.id
            || superseded.is_some_and(|old| old != &fact.proposition)
        {
            return Err(correction_context_mismatch());
        }
        superseded = Some(&fact.proposition);
    }
    let superseded = superseded.ok_or_else(correction_context_mismatch)?;
    let replacement = Proposition {
        object: replacement_object.clone(),
        ..superseded.clone()
    };
    Ok((superseded.clone(), replacement))
}

fn active_previous_correction(
    store: &Store,
    bundle: &InputBundle,
    prior_results: &BTreeMap<String, ResultRecord>,
    previous: &ResultRecord,
    memory: &MemoryState,
    remaining_actions: &mut u32,
) -> Result<Option<PreviousCorrectionContext>> {
    let invalid = correction_context_mismatch;
    let actor = &bundle.request.actor_id;
    let case = &bundle.request.case_id;
    let parser_version = &bundle.meaning.parser_version;
    let explicit_scope = match &bundle.meaning.intent {
        Intent::AskPreviousRelation { explicit_scope, .. }
        | Intent::AssertPreviousRelation { explicit_scope, .. } => explicit_scope,
        _ => return Err(invalid()),
    };
    let source =
        previous_natural_correction_input(store, actor, case, previous)?.ok_or_else(invalid)?;
    let (superseded, replacement) = match (parser_version.as_str(), &source.meaning.intent) {
        (
            input::NAMED_CORRECTION_RELATION_QUERY_PARSER_VERSION
            | input::CORRECTION_RELATION_ASSERTION_PARSER_VERSION,
            Intent::CorrectPremise {
                superseded,
                replacement,
            },
        ) => (superseded.clone(), replacement.clone()),
        (
            input::CONTEXTUAL_CORRECTION_RELATION_QUERY_PARSER_VERSION
            | input::CORRECTION_RELATION_ASSERTION_PARSER_VERSION,
            Intent::CorrectPreviousPremise { .. },
        ) => previous_contextual_correction_target(
            store,
            &source,
            prior_results,
            previous,
            memory,
            remaining_actions,
        )?,
        _ => return Err(invalid()),
    };
    let correction = previous_correction_context(&source, previous, superseded, replacement)?;
    if explicit_scope
        .as_ref()
        .is_some_and(|scope| scope != &correction.replacement.scope)
    {
        return Ok(None);
    }
    *remaining_actions = remaining_actions.checked_sub(2).ok_or_else(|| {
        failure(
            "RELATION_BUDGET",
            "correction context verification exceeds the sealed action budget",
        )
    })?;
    let (mut negative_matches, mut replacement_matches) = (0, 0);
    for fact in &memory.facts {
        if fact.valid
            && fact.source_input == previous.id
            && fact.guarantee == Guarantee::UserPremise
            && fact.dependencies.is_empty()
            && fact.source_evidence.as_ref() == Some(&correction.source_evidence)
        {
            if fact.id == correction.negative_id && fact.proposition == correction.negative {
                negative_matches += 1;
            } else if fact.id == correction.replacement_id
                && fact.proposition == correction.replacement
            {
                replacement_matches += 1;
            }
        }
    }
    if (negative_matches, replacement_matches) != (1, 1) {
        return Err(failure(
            "CONTEXT_PROOF_CHANGED",
            "参照した訂正の否定と置き換えを、現在有効な提示前提として照合できません。",
        ));
    }
    Ok(Some(correction))
}

fn correction_assertion_anchor(
    store: &Store,
    bundle: &InputBundle,
    prior_results: &BTreeMap<String, ResultRecord>,
    previous: &ResultRecord,
    memory: &MemoryState,
    remaining: &mut u32,
) -> Result<Proposition> {
    let starts_with_correction =
        bundle.meaning.parser_version == input::CORRECTION_RELATION_ASSERTION_PARSER_VERSION;
    let missing_scope = || {
        failure(
            "MISSING_RELATION_CONTEXT",
            if starts_with_correction {
                "指定した範囲は参照した訂正の範囲と一致しません。"
            } else {
                "指定した範囲は参照した前提の範囲と一致しません。"
            },
        )
    };
    if starts_with_correction {
        return active_previous_correction(
            store,
            bundle,
            prior_results,
            previous,
            memory,
            remaining,
        )?
        .map(|correction| correction.replacement)
        .ok_or_else(missing_scope);
    }
    let invalid = correction_context_mismatch;
    let Intent::AssertPreviousRelation { explicit_scope, .. } = &bundle.meaning.intent else {
        return Err(invalid());
    };
    if bundle.meaning.parser_version != input::CORRECTION_ASSERTION_FOLLOWUP_PARSER_VERSION {
        return Err(invalid());
    }
    *remaining = remaining.checked_sub(1).ok_or_else(|| {
        failure(
            "RELATION_BUDGET",
            "the first assertion exceeds the sealed action budget",
        )
    })?;
    let actor = &bundle.request.actor_id;
    let case = &bundle.request.case_id;
    let first = committed_user_premise_input(store, actor, case, previous)?.ok_or_else(invalid)?;
    let [correction_id, _] = previous.dependencies.as_slice() else {
        return Err(invalid());
    };
    let correction = prior_results.get(correction_id).ok_or_else(invalid)?;
    if first.meaning.parser_version != input::CORRECTION_RELATION_ASSERTION_PARSER_VERSION
        || !matches!(first.meaning.intent, Intent::AssertPreviousRelation { .. })
        || !registered_relation_context_matches(&first, prior_results, correction)?
        || previous_natural_correction_input(store, actor, case, correction)?.is_none()
    {
        return Err(invalid());
    }
    let assertion = previous_assertion_context(store, actor, case, prior_results, previous)?
        .filter(|assertion| assertion.context.is_some())
        .ok_or_else(invalid)?;
    let fact = assertion.active_fact(memory)?;
    let revision = previous.actor_memory_revision.ok_or_else(invalid)?;
    // The accepted first assertion is now its own user premise. The historical
    // correction proves the chain's origin, but is not re-applied to today's memory.
    if fact.source_evidence.is_none()
        || !store
            .load_memory_revision(actor, revision)?
            .is_some_and(|saved| saved.facts.contains(fact))
    {
        return Err(invalid());
    }
    if explicit_scope
        .as_ref()
        .is_some_and(|scope| scope != &fact.proposition.scope)
    {
        return Err(missing_scope());
    }
    Ok(fact.proposition.clone())
}

pub(crate) fn verified_correction_assertion_proposition(
    store: &Store,
    bundle: &InputBundle,
    prior_results: &BTreeMap<String, ResultRecord>,
    previous: &ResultRecord,
    memory: &MemoryState,
) -> Result<Proposition> {
    let Intent::AssertPreviousRelation { proposition, .. } = &bundle.meaning.intent else {
        return Err(correction_context_mismatch());
    };
    let request = &bundle.request;
    if !input::is_correction_context_assertion(&bundle.meaning.parser_version)
        || !registered_relation_context_matches(bundle, prior_results, previous)?
        || store
            .get_input_result(
                &request.actor_id,
                &request.case_id,
                &previous.input_id,
                previous.input_version,
            )?
            .as_ref()
            != Some(previous)
    {
        return Err(correction_context_mismatch());
    }
    let output_actions = 3 + u32::from(request.output_path.is_some());
    let mut remaining = request
        .budget
        .max_actions
        .checked_sub(output_actions)
        .ok_or_else(|| {
            failure(
                "RELATION_BUDGET",
                "the assertion has no action budget for correction context",
            )
        })?;
    let anchor = correction_assertion_anchor(
        store,
        bundle,
        prior_results,
        previous,
        memory,
        &mut remaining,
    )?;
    let answer = relations::query(memory, &anchor, remaining)?;
    if let Some((_, code, message)) = condition_block(&answer) {
        return Err(failure(code, message));
    }
    if answer.status != "SUPPORTED"
        || relations::answer_guarantee(&answer) != Guarantee::UserPremise
    {
        return Err(failure(
            "CONTEXT_PROOF_CHANGED",
            if bundle.meaning.parser_version == input::CORRECTION_RELATION_ASSERTION_PARSER_VERSION
            {
                "参照した訂正の置き換えを、現在有効な提示前提として確認できません。"
            } else {
                "先に追加した前提を、現在有効な提示前提として確認できません。"
            },
        ));
    }
    Ok(Proposition {
        subject: anchor.subject,
        scope: anchor.scope,
        ..proposition.clone()
    })
}

pub(crate) fn verify_correction_assertion_context_active(
    store: &Store,
    bundle: &InputBundle,
    prior_results: &BTreeMap<String, ResultRecord>,
    previous: &ResultRecord,
    memory: &MemoryState,
) -> Result<()> {
    let mut remaining = bundle.request.budget.max_actions;
    correction_assertion_anchor(
        store,
        bundle,
        prior_results,
        previous,
        memory,
        &mut remaining,
    )?;
    Ok(())
}

fn previous_named_derived_relation_source(
    store: &Store,
    actor: &str,
    case: &str,
    prior: &ResultRecord,
) -> Result<Option<Proposition>> {
    if prior.lifecycle != Lifecycle::Commit
        || prior.guarantee != Guarantee::Derived
        || prior.actor_memory_revision.is_none()
    {
        return Ok(None);
    }
    let Some((_, registered)) =
        store.get_input_bundle(actor, case, &prior.input_id, prior.input_version)?
    else {
        return Ok(None);
    };
    let Intent::Ask { proposition } = registered.meaning.intent else {
        return Ok(None);
    };
    if identity(&registered.request)? != prior.id {
        return Err(failure(
            "PRIOR_INPUT_RESULT_MISMATCH",
            "代名詞が参照する関係質問の確定済み入力と結果IDが一致しません。",
        ));
    }
    Ok(Some(proposition))
}

fn previous_named_derived_relation_has_single_subject_and_multiple_premises(
    store: &Store,
    actor: &str,
    case: &str,
    prior: &ResultRecord,
) -> Result<bool> {
    let Some(revision) = prior.actor_memory_revision else {
        return Ok(false);
    };
    // A derived answer with at least two direct premises also contains a derived fact.
    if prior
        .dependencies
        .iter()
        .filter(|id| id.starts_with("fact:"))
        .take(3)
        .count()
        < 3
    {
        return Ok(false);
    }
    let Some(anchor) = previous_named_derived_relation_source(store, actor, case, prior)? else {
        return Ok(false);
    };
    let memory = store
        .load_memory_revision(actor, revision)?
        .ok_or_else(|| {
            failure(
                "PRIOR_INPUT_RESULT_MISMATCH",
                "代名詞が参照する関係質問の記憶版を照合できません。",
            )
        })?;
    let mut premises = previous_named_derived_direct_premises(&memory, prior);
    let (Some(first), Some(second)) = (premises.next(), premises.next()) else {
        return Ok(false);
    };
    Ok([first, second].into_iter().chain(premises).all(|fact| {
        fact.proposition.subject == anchor.subject && fact.proposition.scope == anchor.scope
    }))
}

fn previous_named_derived_direct_premises<'a>(
    memory: &'a MemoryState,
    previous: &'a ResultRecord,
) -> impl Iterator<Item = &'a Fact> {
    memory.facts.iter().filter(|fact| {
        fact.valid
            && fact.guarantee == Guarantee::UserPremise
            && fact.dependencies.is_empty()
            && !fact.source_input.is_empty()
            && previous.dependencies.binary_search(&fact.id).is_ok()
    })
}

struct PreviousDerivedFollowup {
    proposition: Proposition,
    explain: bool,
    bundle: InputBundle,
}

fn previous_derived_followup_question(
    store: &Store,
    actor: &str,
    case: &str,
    previous: &ResultRecord,
) -> Result<Option<PreviousDerivedFollowup>> {
    let Some(revision) = previous.actor_memory_revision else {
        return Ok(None);
    };
    if previous.lifecycle != Lifecycle::Commit
        || previous.guarantee != Guarantee::Derived
        || previous
            .dependencies
            .iter()
            .filter(|id| id.starts_with("fact:"))
            .take(3)
            .count()
            < 3
    {
        return Ok(None);
    }
    let Some((_, registered)) =
        store.get_input_bundle(actor, case, &previous.input_id, previous.input_version)?
    else {
        return Ok(None);
    };
    if !matches!(
        registered.meaning.parser_version.as_str(),
        input::NAMED_DERIVED_RELATION_QUERY_PARSER_VERSION
            | input::DERIVED_RELATION_FOLLOWUP_PARSER_VERSION
    ) {
        return Ok(None);
    }
    let invalid = || {
        failure(
            "PRIOR_INPUT_RESULT_MISMATCH",
            "代名詞が参照する関係回答と確定時の入力・記憶版を照合できません。",
        )
    };
    let Intent::AskPreviousRelation {
        proposition,
        explain,
        explicit_scope,
    } = &registered.meaning.intent
    else {
        return Err(invalid());
    };
    if identity(&registered.request)? != previous.id {
        return Err(invalid());
    }
    // Recover the subject and scope from this answer's own proof, keeping the
    // remaining proposition fields from its registered question.
    let original_memory = store
        .load_memory_revision(actor, revision)?
        .ok_or_else(invalid)?;
    let mut premises = previous_named_derived_direct_premises(&original_memory, previous);
    let (Some(first), Some(second)) = (premises.next(), premises.next()) else {
        return Ok(None);
    };
    let anchor = &first.proposition;
    if explicit_scope
        .as_ref()
        .is_some_and(|scope| scope != &anchor.scope)
        || std::iter::once(second).chain(premises).any(|fact| {
            fact.proposition.subject != anchor.subject || fact.proposition.scope != anchor.scope
        })
    {
        return Ok(None);
    }
    Ok(Some(PreviousDerivedFollowup {
        proposition: Proposition {
            subject: anchor.subject.clone(),
            scope: anchor.scope.clone(),
            ..proposition.clone()
        },
        explain: *explain,
        bundle: registered,
    }))
}

fn verify_named_origin_for_derived_followup(
    store: &Store,
    prior_results: &BTreeMap<String, ResultRecord>,
    followup: &PreviousDerivedFollowup,
    memory: &MemoryState,
    remaining_actions: &mut u32,
) -> Result<()> {
    let bundle = &followup.bundle;
    if bundle.meaning.parser_version != input::NAMED_DERIVED_RELATION_QUERY_PARSER_VERSION {
        return Ok(());
    }
    // v187 still binds to its named question. A v191 result already carries
    // its own verified proof, so longer chains stop at the immediately prior result.
    let changed = || {
        failure(
            "CONTEXT_PROOF_CHANGED",
            "前回の関係回答で使った主語と根拠を、現在の記憶で再確認できません。",
        )
    };
    let original = bundle
        .meaning
        .context_result_ref
        .as_ref()
        .and_then(|id| prior_results.get(id))
        .ok_or_else(changed)?;
    if !registered_relation_context_matches(bundle, prior_results, original)? {
        return Err(changed());
    }
    let request = &bundle.request;
    let anchor = previous_named_derived_relation_source(
        store,
        &request.actor_id,
        &request.case_id,
        original,
    )?
    .ok_or_else(changed)?;
    let resolution = resolve_derived_relation_context(
        store,
        request,
        original,
        memory,
        &anchor,
        false,
        &followup.proposition,
        remaining_actions,
    )?;
    match resolution {
        PreviousRelationResolution::Resolved(resolved) if resolved == followup.proposition => {
            Ok(())
        }
        _ => Err(changed()),
    }
}

fn resolve_derived_relation_context(
    store: &Store,
    request: &RequestEnvelope,
    previous: &ResultRecord,
    memory: &MemoryState,
    anchor: &Proposition,
    explain: bool,
    question: &Proposition,
    remaining_actions: &mut u32,
) -> Result<PreviousRelationResolution> {
    const CODE: &str = "CONTEXT_PROOF_CHANGED";
    let proof_changed = || {
        failure(
            CODE,
            "参照した関係質問の命題と確定時の根拠を現在の記憶で照合できません。",
        )
    };
    let answer = relations::query(memory, anchor, *remaining_actions)?;
    *remaining_actions = remaining_actions
        .checked_sub(answer.steps)
        .ok_or_else(|| failure("RELATION_BUDGET", "relation action count overflowed"))?;
    let expected_units = previous_relation_success_units(memory, anchor, &answer, explain)
        .ok_or_else(proof_changed)?;
    if relations::answer_dependencies(&answer) != previous.dependencies
        || relations::answer_guarantee(&answer) != previous.guarantee
        || !previous.content_plan.as_ref().is_some_and(|plan| {
            plan.required_units == expected_units
                && plan.dependencies.starts_with(&previous.dependencies)
        })
    {
        return Err(proof_changed());
    }
    crate::document::revalidate_previous_document_render(
        store,
        &request.actor_id,
        &request.case_id,
        previous,
        None,
        CODE,
        "named derived relation answer",
    )?;

    let mut direct_premises = previous_named_derived_direct_premises(memory, previous);
    let Some(first) = direct_premises.next() else {
        return Ok(PreviousRelationResolution::Missing);
    };
    if std::iter::once(first).chain(direct_premises).any(|fact| {
        fact.proposition.subject != anchor.subject || fact.proposition.scope != anchor.scope
    }) {
        return Ok(PreviousRelationResolution::Ambiguous);
    }
    Ok(PreviousRelationResolution::Resolved(Proposition {
        subject: anchor.subject.clone(),
        scope: anchor.scope.clone(),
        ..question.clone()
    }))
}

pub(crate) fn previous_relation_success_units(
    memory: &MemoryState,
    proposition: &Proposition,
    answer: &relations::QueryOutcome,
    explain: bool,
) -> Option<Vec<String>> {
    if explain {
        let mut units = Vec::new();
        let explained = match answer.status.as_str() {
            "SUPPORTED" => proposition.clone(),
            "CONTRADICTED" => {
                let mut opposite = proposition.clone();
                opposite.negative = !opposite.negative;
                units.push(relation_answer_surface(proposition, true));
                opposite
            }
            _ => return None,
        };
        units.extend(relation_explanation_lines(
            memory,
            &explained,
            answer,
            "提示された証拠が支える結論",
        ));
        Some(units)
    } else {
        match answer.status.as_str() {
            "SUPPORTED" => Some(vec![relation_answer_surface(proposition, false)]),
            "CONTRADICTED" => Some(vec![relation_answer_surface(proposition, true)]),
            _ => None,
        }
    }
}

pub(crate) const PREVIOUS_ASSERTION_EXPLANATION_PREDICATE: &str =
    "PREVIOUS_ASSERTION_EXPLANATION_V1";

pub(crate) struct PreviousAssertionContext {
    result_id: String,
    bundle_ref: String,
    fact_id: String,
    proposition: Proposition,
    context: Option<AssertionSubjectContext>,
    source_evidence: Option<SealedSourceEvidence>,
}

struct AssertionSubjectContext {
    result_id: String,
    explicit_scope: Option<String>,
}

pub(crate) struct PreviousAssertionExplanation {
    pub(crate) dependencies: Vec<String>,
    pub(crate) units: Vec<String>,
}

impl PreviousAssertionContext {
    fn active_fact<'a>(&self, memory: &'a MemoryState) -> Result<&'a Fact> {
        let changed = || {
            failure(
                "CONTEXT_PROOF_CHANGED",
                "前回の提示前提は現在有効な記憶として確認できません。元の回答は履歴で確認できます。",
            )
        };
        let fact = memory
            .facts
            .iter()
            .find(|fact| {
                fact.valid
                    && fact.id == self.fact_id
                    && fact.source_input == self.result_id
                    && fact.guarantee == Guarantee::UserPremise
                    && fact.dependencies.is_empty()
                    && fact
                        .source_evidence
                        .as_ref()
                        .is_none_or(|evidence| self.source_evidence.as_ref() == Some(evidence))
            })
            .ok_or_else(changed)?;
        let proposition = &fact.proposition;
        let expected = if let Some(context) = &self.context {
            if input::singular_previous_subject(&proposition.subject) {
                return Err(changed());
            }
            Proposition {
                subject: proposition.subject.clone(),
                scope: context
                    .explicit_scope
                    .clone()
                    .unwrap_or_else(|| proposition.scope.clone()),
                ..self.proposition.clone()
            }
        } else {
            self.proposition.clone()
        };
        if *proposition != expected
            || relations::user_premise_id(proposition, &self.result_id)? != self.fact_id
        {
            return Err(changed());
        }
        Ok(fact)
    }

    pub(crate) fn explain(&self, memory: &MemoryState) -> Result<PreviousAssertionExplanation> {
        let proposition = &self.active_fact(memory)?.proposition;
        let mut dependencies = vec![
            self.result_id.clone(),
            self.bundle_ref.clone(),
            self.fact_id.clone(),
        ];
        let mut units = vec![
            "前回の回答は、あなたが入力した前提を記録したものです。".into(),
            format!(
                "提示前提: {}（範囲: {}）。",
                describe(proposition),
                proposition.scope
            ),
            format!("前提ID: {}", self.fact_id),
            "記憶状態: 有効。".into(),
        ];
        if let Some(context) = &self.context {
            dependencies.push(context.result_id.clone());
            units.push(format!("主語を参照した結果ID: {}", context.result_id));
        }
        if let Some(time) = &proposition.temporal_scope {
            units.push(format!("適用時点: {time}"));
        }
        if !proposition.condition_refs.is_empty() {
            units.push(format!(
                "適用条件ID: {}",
                proposition
                    .condition_refs
                    .iter()
                    .cloned()
                    .collect::<Vec<_>>()
                    .join("、")
            ));
        }
        units.push("この説明では、適用条件や現実世界の真偽を評価していません。".into());
        Ok(PreviousAssertionExplanation {
            dependencies,
            units,
        })
    }
}

pub(crate) fn previous_assertion_context(
    store: &Store,
    actor: &str,
    case: &str,
    prior_results: &BTreeMap<String, ResultRecord>,
    prior: &ResultRecord,
) -> Result<Option<PreviousAssertionContext>> {
    if prior.lifecycle != Lifecycle::Commit || prior.guarantee != Guarantee::UserPremise {
        return Ok(None);
    }
    let Some((bundle_ref, bundle)) =
        store.get_input_bundle(actor, case, &prior.input_id, prior.input_version)?
    else {
        return Ok(None);
    };
    let invalid = || {
        failure(
            "PRIOR_INPUT_RESULT_MISMATCH",
            "説明対象の前提と確定済み入力・記録IDが一致しません。",
        )
    };
    let (proposition, fact_id, context) = match &bundle.meaning.intent {
        Intent::Assert { proposition } => {
            let fact_id = relations::user_premise_id(proposition, &prior.id)?;
            if prior.dependencies.as_slice() != std::slice::from_ref(&fact_id) {
                return Err(invalid());
            }
            (proposition, fact_id, None)
        }
        Intent::AssertPreviousRelation {
            proposition,
            explicit_scope,
        } => {
            let [.., reference, fact] = prior.dependencies.as_slice() else {
                return Err(invalid());
            };
            if bundle.meaning.context_result_ref.as_ref() != Some(reference)
                || !prior_results.get(reference).is_some_and(|result| {
                    result.id == *reference && result.lifecycle == Lifecycle::Commit
                })
                || reference == &prior.id
            {
                return Err(invalid());
            }
            (
                proposition,
                fact.clone(),
                Some(AssertionSubjectContext {
                    result_id: reference.clone(),
                    explicit_scope: explicit_scope.clone(),
                }),
            )
        }
        _ => return Ok(None),
    };
    if identity(&bundle.request)? != prior.id {
        return Err(invalid());
    }
    Ok(Some(PreviousAssertionContext {
        result_id: prior.id.clone(),
        bundle_ref,
        fact_id,
        proposition: proposition.clone(),
        context,
        source_evidence: input::assertion_source_evidence(&bundle)?,
    }))
}

struct PreviousContextualAssertion {
    fact: Fact,
    has_contextual_assertion_parent: bool,
}

fn previous_contextual_assertion_fact(
    store: &Store,
    actor: &str,
    case: &str,
    prior_results: &BTreeMap<String, ResultRecord>,
    previous: &ResultRecord,
    memory: &MemoryState,
) -> Result<Option<PreviousContextualAssertion>> {
    let Some(assertion) = previous_assertion_context(store, actor, case, prior_results, previous)?
    else {
        return Ok(None);
    };
    let Some(context) = &assertion.context else {
        return Ok(None);
    };
    let fact = assertion.active_fact(memory)?;
    let has_contextual_assertion_parent = memory.facts.iter().any(|fact| {
        fact.source_input == context.result_id
            && fact.guarantee == Guarantee::UserPremise
            && fact.dependencies.is_empty()
    });
    Ok(Some(PreviousContextualAssertion {
        fact: fact.clone(),
        has_contextual_assertion_parent,
    }))
}

#[derive(serde::Serialize)]
struct RetainedGroupMeanHypothesis<'a> {
    kind: &'static str,
    adoption_state: &'static str,
    source_result_id: &'a str,
    statistics_sha256: &'a str,
    scope: &'static str,
    sample: &'a crate::document::GroupMeanAnswer,
    candidate_group: &'a str,
    counterexample_criterion: &'static str,
    counterexample_refs: &'a [String],
    population: Option<&'a str>,
    observed_at: Option<&'a str>,
    empirical_contract_ref: Option<&'a str>,
}

pub(crate) struct GroupMeanHypothesisPlan {
    pub(crate) artifacts: Vec<Vec<u8>>,
    pub(crate) dependencies: Vec<String>,
    pub(crate) units: Vec<String>,
}

impl GroupMeanHypothesisPlan {
    pub(crate) fn verify_artifacts(&self, store: &Store, actor: &str, case: &str) -> Result<()> {
        for bytes in &self.artifacts {
            if store.get_bytes(actor, case, &digest(bytes))? != *bytes {
                return Err(failure(
                    "GROUP_MEAN_HYPOTHESIS_SOURCE_MISMATCH",
                    "保存済みの仮説成果物が再計算した内容と一致しません。",
                ));
            }
        }
        Ok(())
    }
}

/// Resolve the sealed prior observation, or verify a prior retention against
/// that same observation. Hypotheses stay in case artifacts, outside Fact/Rule.
pub(crate) fn previous_group_mean_hypothesis(
    store: &Store,
    state: &CaseRevision,
) -> Result<Option<GroupMeanHypothesisPlan>> {
    let Some(bundle) = &state.bundle else {
        return Ok(None);
    };
    let new_document = match &bundle.meaning.intent {
        Intent::RetainPreviousGroupMeanHypothesis | Intent::ExplainPreviousGroupMeanHypothesis => {
            None
        }
        Intent::AssessPreviousGroupMeanHypothesis { document } => Some(document.as_str()),
        _ => return Ok(None),
    };
    let forming = matches!(
        bundle.meaning.intent,
        Intent::RetainPreviousGroupMeanHypothesis
    );
    let actor = &state.actor_id;
    let case = &state.case_id;
    let context_error = || {
        failure(
            "GROUP_MEAN_HYPOTHESIS_CONTEXT_REQUIRED",
            "同じケースで確定したグループ別統計、またはその統計から保持した仮説が必要です。",
        )
    };
    let prior = bundle
        .meaning
        .context_result_ref
        .as_ref()
        .and_then(|id| state.prior_results.get(id))
        .filter(|result| result.lifecycle == Lifecycle::Commit)
        .ok_or_else(context_error)?;
    if identity(&Some(prior))? != bundle.context_sha256 {
        return Err(context_error());
    }
    let exact_input = |result: &ResultRecord| -> Result<Intent> {
        let (_, input) = store
            .get_input_bundle(actor, case, &result.input_id, result.input_version)?
            .ok_or_else(context_error)?;
        if identity(&input.request)? != result.id {
            return Err(context_error());
        }
        Ok(input.meaning.intent)
    };
    let previous_retention = || {
        prior
            .dependencies
            .first()
            .and_then(|id| state.prior_results.get(id))
            .ok_or_else(context_error)
    };
    let mut earlier_assessment_document = None;
    let retained = if forming {
        None
    } else {
        if prior.guarantee != Guarantee::Hypothesis {
            return Err(context_error());
        }
        let retention = match exact_input(prior)? {
            Intent::RetainPreviousGroupMeanHypothesis => prior,
            Intent::AssessPreviousGroupMeanHypothesis { document } => {
                earlier_assessment_document = Some(document);
                previous_retention()?
            }
            Intent::ExplainPreviousGroupMeanHypothesis if new_document.is_some() => {
                previous_retention()?
            }
            _ => return Err(context_error()),
        };
        if retention.lifecycle != Lifecycle::Commit
            || retention.guarantee != Guarantee::Hypothesis
            || !matches!(
                exact_input(retention)?,
                Intent::RetainPreviousGroupMeanHypothesis
            )
        {
            return Err(context_error());
        }
        Some(retention)
    };
    let observation = if let Some(retention) = retained {
        retention
            .dependencies
            .first()
            .and_then(|id| state.prior_results.get(id))
            .ok_or_else(context_error)?
    } else {
        prior
    };
    let Intent::JsonTableGroupStatistics {
        document,
        group_column,
        value_column,
    } = exact_input(observation)?
    else {
        return Err(context_error());
    };
    let (source_ref, statistics_ref, sample) =
        crate::document::previous_json_table_highest_group_mean(store, actor, case, observation)?
            .ok_or_else(context_error)?;
    if source_ref != digest(document.as_bytes())
        || group_column != sample.group_by
        || value_column != sample.value_column
    {
        return Err(failure(
            "GROUP_MEAN_HYPOTHESIS_SOURCE_MISMATCH",
            "グループ別統計と封印済みの観測表が一致しません。",
        ));
    }
    let [leader] = sample.winners.as_slice() else {
        return Err(failure(
            "GROUP_MEAN_HYPOTHESIS_NO_CONTRAST",
            "標本内に平均が単独で最大のグループがないため、候補を保持しません。",
        ));
    };
    if sample.comparisons.len() < 2 {
        return Err(failure(
            "GROUP_MEAN_HYPOTHESIS_NO_CONTRAST",
            "グループ間の平均を比べるには二つ以上のグループが必要です。",
        ));
    }
    let bytes = canonical(&RetainedGroupMeanHypothesis {
        kind: "minidora.learning.group-mean-hypothesis.v1",
        adoption_state: "HYPOTHESIS_RETAINED",
        source_result_id: &observation.id,
        statistics_sha256: &statistics_ref,
        scope: "REPEAT_SAMPLE_SAME_GROUPS_MEASURE_AND_UNIT",
        sample: &sample,
        candidate_group: &leader.group,
        counterexample_criterion: "ON_COMPARABLE_SAMPLE_LEADER_NOT_STRICTLY_HIGHEST",
        counterexample_refs: &[],
        population: None,
        observed_at: None,
        empirical_contract_ref: None,
    })?;
    let reference = digest(&bytes);
    let units = vec![
        "仮説を保持しています（HYPOTHESIS_RETAINED）。".into(),
        format!(
            "元の標本で、グループ {:?} の {:?} の平均 {} が単独で最大でした。",
            leader.group, sample.value_column, leader.mean,
        ),
        format!(
            "仮説は、同じグループ・測定項目・単位で再度採った標本でも {:?} の平均が単独で最大になる、というものです。",
            leader.group,
        ),
        "同条件の再観測で他のグループが同率以上なら、この仮説への反例です。".into(),
        "母集団・観測時点・事前の採用契約が確認できないため、採用は保留です。".into(),
        format!("元の標本 SHA-256 {source_ref}"),
    ];
    let mut plan = GroupMeanHypothesisPlan {
        artifacts: vec![bytes],
        dependencies: vec![
            observation.id.clone(),
            source_ref,
            statistics_ref,
            reference.clone(),
        ],
        units,
    };
    if let Some(retention) = retained {
        if retention.dependencies != plan.dependencies {
            return Err(failure(
                "GROUP_MEAN_HYPOTHESIS_SOURCE_MISMATCH",
                "保持済み仮説が元の標本と一致しません。",
            ));
        }
        plan.verify_artifacts(store, actor, case)?;
        plan.dependencies.insert(0, retention.id.clone());
        if let Some(document) = new_document.or(earlier_assessment_document.as_deref()) {
            let original = serde_json::from_str(&observation.text)?;
            plan = assess_group_mean_hypothesis(
                document,
                &original,
                &leader.group,
                &retention.id,
                &reference,
                plan.dependencies,
            )?;
            if new_document.is_none() {
                if prior.dependencies != plan.dependencies {
                    return Err(failure(
                        "GROUP_MEAN_HYPOTHESIS_SOURCE_MISMATCH",
                        "保存済みの照合結果が新標本と一致しません。",
                    ));
                }
                plan.verify_artifacts(store, actor, case)?;
                plan.dependencies.insert(1, prior.id.clone());
            }
        }
    }
    let bytes_used = plan
        .artifacts
        .iter()
        .map(Vec::len)
        .chain(plan.units.iter().map(String::len))
        .sum::<usize>();
    if bytes_used > bundle.request.budget.max_evidence_bytes {
        return Err(failure(
            "RESOURCE_LIMIT",
            "仮説と出所が要求の証拠予算を超えました。",
        ));
    }
    Ok(Some(plan))
}

#[derive(serde::Serialize)]
struct GroupMeanHypothesisAssessment<'a> {
    kind: &'static str,
    adoption_state: &'static str,
    outcome: &'static str,
    hypothesis_result_id: &'a str,
    hypothesis_sha256: &'a str,
    candidate_group: &'a str,
    sample: &'a crate::document::GroupMeanAnswer,
    statistics_sha256: &'a str,
    counterexample_refs: &'a [String],
}

fn assess_group_mean_hypothesis(
    document: &str,
    original: &ports::JsonTableGroupStatisticsReport,
    candidate: &str,
    hypothesis_result_id: &str,
    hypothesis_ref: &str,
    mut dependencies: Vec<String>,
) -> Result<GroupMeanHypothesisPlan> {
    fn group_values(report: &ports::JsonTableGroupStatisticsReport) -> BTreeMap<&str, &[String]> {
        report
            .groups
            .iter()
            .map(|group| {
                (
                    group.value.as_str(),
                    group.statistics.sorted_values.as_slice(),
                )
            })
            .collect()
    }

    let statistics =
        ports::json_table_group_statistics(document, &original.group_by, &original.value_column)?;
    let sample = crate::document::json_table_highest_group_mean(&statistics)?;
    let original_values = group_values(original);
    let repeat_values = group_values(&statistics);
    if !original_values.keys().eq(repeat_values.keys())
        || sample.comparison_unit != original.groups[0].statistics.unit
    {
        return Err(failure(
            "GROUP_MEAN_HYPOTHESIS_SCOPE_MISMATCH",
            "同じ群の集合・測定項目・単位で得た標本が必要です。",
        ));
    }
    if original_values == repeat_values {
        return Err(failure(
            "GROUP_MEAN_HYPOTHESIS_SAMPLE_REUSED",
            "各群の厳密値とその件数が元の標本と同じです。",
        ));
    }
    let matches_prediction =
        matches!(sample.winners.as_slice(), [winner] if winner.group == candidate);
    let statistics_bytes = canonical(&statistics)?;
    let statistics_ref = digest(&statistics_bytes);
    let assessment = canonical(&GroupMeanHypothesisAssessment {
        kind: "minidora.learning.group-mean-hypothesis-assessment.v1",
        adoption_state: "HYPOTHESIS_RETAINED",
        outcome: if matches_prediction {
            "MATCHED_ON_SAMPLE"
        } else {
            "COUNTEREXAMPLE_ON_SAMPLE"
        },
        hypothesis_result_id,
        hypothesis_sha256: hypothesis_ref,
        candidate_group: candidate,
        sample: &sample,
        statistics_sha256: &statistics_ref,
        counterexample_refs: if matches_prediction {
            &[]
        } else {
            std::slice::from_ref(&statistics_ref)
        },
    })?;
    let candidate_mean = &sample
        .comparisons
        .iter()
        .find(|entry| entry.group == candidate)
        .expect("the candidate is in the verified group set")
        .mean;
    let winners = sample
        .winners
        .iter()
        .map(|winner| format!("{:?}", winner.group))
        .collect::<Vec<_>>()
        .join("、");
    let units = vec![
        if matches_prediction {
            "今回の標本では仮説と一致し、反例は観測されませんでした（MATCHED_ON_SAMPLE）。".into()
        } else {
            "今回の標本では候補群が単独首位を外れ、反例が観測されました（COUNTEREXAMPLE_ON_SAMPLE）。".into()
        },
        format!(
            "候補群 {candidate:?} の平均は {candidate_mean}、最大平均は {}（{winners}）です。",
            sample.winners[0].mean
        ),
        "この判定の範囲は提示された標本です。独立採取は未確認で、仮説の採用は保留です。".into(),
        format!("照合した標本 SHA-256 {}", sample.source_sha256),
    ];
    dependencies.extend([sample.source_sha256, statistics_ref, digest(&assessment)]);
    Ok(GroupMeanHypothesisPlan {
        artifacts: vec![document.as_bytes().to_vec(), statistics_bytes, assessment],
        dependencies,
        units,
    })
}

fn select_rule_grammar_adaptation(
    store: &Store,
    bundle: &InputBundle,
    previous: Option<&ResultRecord>,
    current: &CoreRelationMeaningPlan,
) -> Result<Option<(RuleGrammarAdaptation, Vec<String>)>> {
    let Some(source) = previous.filter(|result| result.lifecycle == Lifecycle::Commit) else {
        return Ok(None);
    };
    let Some(plan) = source
        .content_plan
        .as_ref()
        .filter(|plan| plan.locale == bundle.request.locale)
    else {
        return Ok(None);
    };
    let Some(source_compute_ref) = plan.dependencies.last() else {
        return Ok(None);
    };
    let actor = &bundle.request.actor_id;
    let case = &bundle.request.case_id;
    let Some((_, source_bundle)) =
        store.get_input_bundle(actor, case, &source.input_id, source.input_version)?
    else {
        return Ok(None);
    };
    let Intent::AskWhy { proposition } = &source_bundle.meaning.intent else {
        return Ok(None);
    };
    if source_bundle.meaning.source_kind != "USER_REQUEST"
        || proposition.scope != current.asked.scope
        || proposition.temporal_scope != current.asked.temporal_scope
        || proposition.condition_refs != current.asked.condition_refs
    {
        return Ok(None);
    }
    if identity(&source_bundle.request)? != source.id
        || bundle.meaning.context_result_ref.as_ref() != Some(&source.id)
        || bundle.context_sha256 != identity(&Some(source))?
    {
        return Err(failure(
            "MODEL_ADAPTATION_SOURCE_MISMATCH",
            "the rule grammar source differs from the sealed prior result",
        ));
    }
    let bytes = store.get_bytes(actor, case, source_compute_ref)?;
    let value: serde_json::Value = serde_json::from_slice(&bytes)?;
    if !matches!(
        value.get("kind").and_then(serde_json::Value::as_str),
        Some(
            CORE_COMPUTATION_ARTIFACT_KIND_V5
                | CORE_COMPUTATION_ARTIFACT_KIND_V6
                | CORE_COMPUTATION_ARTIFACT_KIND_V7
        )
    ) {
        return Ok(None);
    }
    let artifact: CoreComputationArtifactGeneratedContent = serde_json::from_value(value)?;
    let source_meaning = &artifact.relation_meaning_plan;
    let mut unsigned_plan = plan.clone();
    unsigned_plan.id.clear();
    if plan.id != identity(&unsigned_plan)?
        || artifact.kind != generated_artifact_kind(artifact.adaptation.as_ref())
        || artifact.units != plan.required_units
        || artifact.dependencies != source.dependencies
        || source_meaning.dependencies != source.dependencies
        || source_meaning.asked != *proposition
        || source_meaning.id != relation_meaning_plan_id(source_meaning)?
    {
        return Err(failure(
            "MODEL_ADAPTATION_SOURCE_MISMATCH",
            "the retained rule grammar source failed its meaning or plan binding",
        ));
    }

    Ok(
        rule_grammar_documents(current, Some(source_meaning))?.map(|(rule_ids, documents)| {
            (
                RuleGrammarAdaptation::PriorResult {
                    source_result_id: source.id.clone(),
                    source_compute_ref: source_compute_ref.clone(),
                    rule_ids,
                },
                documents,
            )
        }),
    )
}

fn rule_grammar_documents(
    current: &CoreRelationMeaningPlan,
    shared_with: Option<&CoreRelationMeaningPlan>,
) -> Result<Option<(Vec<String>, Vec<String>)>> {
    // Use only the composed rule and its exact parents from the verified
    // proof. Claim, fact, condition-evidence and response text are excluded.
    for unit in &current.units {
        let CoreSemanticMeaning::Rule {
            record_id,
            record_dependencies,
            composed: true,
            ..
        } = &unit.meaning
        else {
            continue;
        };
        let mut rule_ids = vec![record_id.clone()];
        rule_ids.extend(record_dependencies.iter().cloned());
        let mut retained_rules = Vec::new();
        for id in &rule_ids {
            let Some(current_rule) = current.units.iter().find(|candidate| {
                matches!(&candidate.meaning, CoreSemanticMeaning::Rule { record_id, .. } if record_id == id)
            }) else {
                break;
            };
            if shared_with.is_some_and(|source| {
                !source
                    .units
                    .iter()
                    .any(|candidate| candidate.meaning == current_rule.meaning)
            }) {
                break;
            }
            retained_rules.push(&current_rule.meaning);
        }
        if retained_rules.len() != rule_ids.len() {
            continue;
        }
        let mut documents = Vec::new();
        for meaning in retained_rules {
            for fragment in core_semantic_meaning_fragments(meaning)? {
                match fragment {
                    language::MeaningGrammarFragment::Literal { value } => documents.push(value),
                    language::MeaningGrammarFragment::ExactField { .. } => {}
                    language::MeaningGrammarFragment::Choice { alternatives, .. } => {
                        documents.extend(alternatives);
                    }
                }
            }
        }
        return Ok(Some((rule_ids, documents)));
    }
    Ok(None)
}

fn verify_generated_relation_replay(
    store: &Store,
    (actor, case): (&str, &str),
    previous: &ResultRecord,
    plan: &ContentPlan,
    compute_ref: &str,
    artifact: &CoreComputationArtifactGeneratedContent,
    rule_documents: &[String],
) -> Result<()> {
    let generation_report = &artifact.content_generation;
    let request = relation_meaning_plan_to_generation_request(
        &artifact.relation_meaning_plan,
        generation_report.max_output_bytes,
    )?;
    if request.max_tokens != generation_report.max_tokens {
        return Err(failure(
            "PRIOR_COMPUTATION_ARTIFACT_MISMATCH",
            "the retained grammar budget differs from its generation report",
        ));
    }
    let mut replay = language::replay_semantic_meaning_with_rules(&request, rule_documents)
        .map_err(|_| {
            failure(
                "PRIOR_COMPUTATION_ARTIFACT_MISMATCH",
                "the retained Core meaning grammar cannot be replayed",
            )
        })?;
    replay.report.model_artifact_ref = generation_report.model_artifact_ref.clone();
    validate_relation_meaning_generation(
        &artifact.relation_meaning_plan,
        &request,
        &replay.report,
    )?;
    let Some(model_artifact_ref) = generation_report.model_artifact_ref.as_deref() else {
        return Err(failure(
            "PRIOR_COMPUTATION_ARTIFACT_MISMATCH",
            "the retained semantic generation has no persisted model artifact reference",
        ));
    };
    let Some(stored_model) = store.get_bytes_if_present(actor, case, model_artifact_ref)? else {
        return Err(failure(
            "PRIOR_COMPUTATION_ARTIFACT_UNAVAILABLE",
            "the persisted semantic generation model is unavailable",
        ));
    };
    let replayed_model = canonical(&replay.model)?;
    let Some(stored_emission) =
        store.get_bytes_if_present(actor, case, &plan.model_observation_ref)?
    else {
        return Err(failure(
            "PRIOR_COMPUTATION_ARTIFACT_UNAVAILABLE",
            "the persisted semantic generation observation is unavailable",
        ));
    };
    let replayed_emission = canonical(&replay.emission)?;
    let replayed_units = replay
        .report
        .units
        .iter()
        .map(|unit| unit.text.clone())
        .collect::<Vec<_>>();
    let content_plan_matches = validate_generated_content_plan_adoption(
        previous,
        generation_report,
        &replay.emission,
        &previous.dependencies,
        compute_ref,
        &plan.locale,
        plan,
    )
    .is_ok();
    let meaning_plan_matches = artifact.relation_meaning_plan.conclusion
        == artifact.relation_meaning
        && relation_meaning_status_matches(
            &artifact.relation_meaning_plan.asked,
            &artifact.relation_meaning_plan.conclusion,
            &artifact.relation_meaning_plan.answer_status,
        )
        && artifact.relation_meaning_plan.guarantee == previous.guarantee
        && artifact.relation_meaning_plan.dependencies == artifact.relation_proof_dependencies
        && artifact.relation_meaning_plan.dependencies == previous.dependencies
        && artifact
            .relation_meaning_plan
            .required_units
            .iter()
            .cloned()
            .collect::<BTreeSet<_>>()
            == replay
                .report
                .required_unit_ids
                .iter()
                .cloned()
                .collect::<BTreeSet<_>>()
        && relation_meaning_plan_to_generation_request(
            &artifact.relation_meaning_plan,
            generation_report.max_output_bytes,
        )
        .is_ok_and(|expected| expected == request);
    let has_adaptation = !rule_documents.is_empty();
    if artifact.kind != generated_artifact_kind(artifact.adaptation.as_ref())
        || artifact.adaptation.is_some() != has_adaptation
        || artifact.calculation != previous.calculation
        || artifact.units != plan.required_units
        || artifact.units != replayed_units
        || artifact.evidence != previous.evidence
        || artifact.dependencies != previous.dependencies
        || artifact.relation_proof_dependencies != previous.dependencies
        || !meaning_plan_matches
        || !content_plan_matches
        || replay.report != *generation_report
        || replay.report.text != previous.text
        || plan.text != replay.report.text
        || plan.required_units != artifact.units
        || plan.guarantee != previous.guarantee
        || plan.model_revision != replay.report.model_revision
        || plan.model_artifact_ref != model_artifact_ref
        || digest(&stored_model) != model_artifact_ref
        || stored_model != replayed_model
        || digest(&stored_emission) != plan.model_observation_ref
        || stored_emission != replayed_emission
    {
        return Err(failure(
            "PRIOR_COMPUTATION_ARTIFACT_MISMATCH",
            "the retained generated meaning, model, proof IDs and rendered sequence do not match their committed artifacts",
        ));
    }
    Ok(())
}

pub(crate) struct RelationRecoveryProofs<'a> {
    pub(crate) current: (&'a MemoryState, &'a relations::QueryOutcome),
    pub(crate) original: Option<(&'a MemoryState, &'a relations::QueryOutcome)>,
}

pub(crate) fn verify_generated_why_computation(
    store: &Store,
    state: &CaseRevision,
    result: &ResultRecord,
    plan: &ContentPlan,
    proposition: &Proposition,
    proofs: RelationRecoveryProofs<'_>,
    budget: &ResourceBudget,
) -> Result<Option<String>> {
    let actor = &state.actor_id;
    let case = &state.case_id;
    let Some(compute_ref) = plan.dependencies.last() else {
        return Ok(None);
    };
    let bytes = store.get_bytes(actor, case, compute_ref)?;
    let value: serde_json::Value = serde_json::from_slice(&bytes)?;
    if !matches!(
        value.get("kind").and_then(serde_json::Value::as_str),
        Some(
            CORE_COMPUTATION_ARTIFACT_KIND_V5
                | CORE_COMPUTATION_ARTIFACT_KIND_V6
                | CORE_COMPUTATION_ARTIFACT_KIND_V7
        )
    ) {
        return Ok(None);
    }
    let artifact: CoreComputationArtifactGeneratedContent = serde_json::from_value(value)?;
    let revision = result.actor_memory_revision.ok_or_else(|| {
        failure(
            "RECOVERY_MEMORY_REVISION_MISSING",
            "the generated explanation does not bind its original actor-memory revision",
        )
    })?;
    let matches_proof = |memory: &MemoryState, answer: &relations::QueryOutcome| -> Result<bool> {
        let Some(conclusion) = relation_explanation_target(proposition, answer) else {
            return Ok(false);
        };
        let dependencies = relations::answer_dependencies(answer);
        if dependencies != result.dependencies
            || relations::answer_guarantee(answer) != result.guarantee
        {
            return Ok(false);
        }
        Ok(
            relation_atomic_meaning_plan(memory, proposition, &conclusion, answer, &dependencies)?
                == artifact.relation_meaning_plan,
        )
    };
    let (current_memory, current_answer) = proofs.current;
    if !matches_proof(current_memory, current_answer)? {
        return Err(failure(
            "RECOVERY_PROOF_INVALIDATED",
            "current relation evidence no longer reproduces the adopted meaning plan",
        ));
    }
    if current_memory.revision != revision {
        let (original_memory, original_answer) = proofs
            .original
            .filter(|(memory, _)| memory.revision == revision)
            .ok_or_else(|| {
                failure(
                    "RECOVERY_MEMORY_REVISION_MISSING",
                    "the original actor-memory snapshot for the generated explanation is missing",
                )
            })?;
        if !matches_proof(original_memory, original_answer)? {
            return Err(failure(
                "RECOVERY_MEMORY_PROOF_MISMATCH",
                "the original snapshot does not reproduce the adopted meaning plan",
            ));
        }
    }
    let output_budget = state.bundle.as_ref().map_or(
        budget
            .max_evidence_bytes
            .min(LEGACY_RELATION_GENERATION_BYTES),
        |bundle| relation_explanation_output_budget(&bundle.meaning, budget.max_evidence_bytes),
    );
    let expected_request = relation_meaning_plan_to_generation_request(
        &artifact.relation_meaning_plan,
        output_budget,
    )?;
    if expected_request.max_output_bytes != artifact.content_generation.max_output_bytes
        || expected_request.max_tokens != artifact.content_generation.max_tokens
    {
        return Err(failure(
            "RECOVERY_GOAL_MISMATCH",
            "the generated meaning does not use the original goal's output budget",
        ));
    }
    let rule_documents = match &artifact.adaptation {
        Some(
            adaptation @ RuleGrammarAdaptation::PriorResult {
                source_result_id, ..
            },
        ) => {
            let bundle = state.bundle.as_ref().ok_or_else(|| {
                failure(
                    "MODEL_ADAPTATION_SOURCE_MISMATCH",
                    "sealed adaptation input is missing",
                )
            })?;
            let previous = state.prior_results.get(source_result_id);
            let Some((source, documents)) = select_rule_grammar_adaptation(
                store,
                bundle,
                previous,
                &artifact.relation_meaning_plan,
            )?
            else {
                return Err(failure(
                    "MODEL_ADAPTATION_SOURCE_MISMATCH",
                    "retained shared rule source is unavailable",
                ));
            };
            if source != *adaptation {
                return Err(failure(
                    "MODEL_ADAPTATION_SOURCE_MISMATCH",
                    "retained shared rules no longer reproduce the exact adaptation source",
                ));
            }
            documents
        }
        Some(RuleGrammarAdaptation::ActorRules { rule_ids }) => {
            let source = rule_grammar_documents(&artifact.relation_meaning_plan, None)?;
            let ordinary = state.bundle.as_ref().is_some_and(|bundle| {
                bundle.meaning.source_kind == "USER_REQUEST"
                    && matches!(bundle.meaning.intent, Intent::AskWhy { .. })
            });
            match source {
                Some((ids, documents)) if ordinary && ids == *rule_ids => documents,
                _ => {
                    return Err(failure(
                        "MODEL_ADAPTATION_SOURCE_MISMATCH",
                        "retained actor-memory rules no longer reproduce the exact adaptation source",
                    ));
                }
            }
        }
        None => Vec::new(),
    };
    verify_generated_relation_replay(
        store,
        (actor, case),
        result,
        plan,
        compute_ref,
        &artifact,
        &rule_documents,
    )?;
    Ok(Some(compute_ref.clone()))
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
    let prior = store.get_input_bundle(actor, case, &previous.input_id, previous.input_version)?;
    previous_relation_meaning_from_input(
        store,
        actor,
        case,
        previous,
        prior.as_ref().map(|(_, b)| b),
    )
}

fn previous_relation_meaning_from_input(
    store: &Store,
    actor: &str,
    case: &str,
    previous: &ResultRecord,
    prior_bundle: Option<&InputBundle>,
) -> Result<Option<PriorRelationMeaning>> {
    if let Some(prior_bundle) = prior_bundle {
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
        let artifact_kind = value
            .get("kind")
            .and_then(serde_json::Value::as_str)
            .map(str::to_owned);
        if artifact_kind.as_deref().is_some_and(|kind| {
            kind.starts_with("minidora.core-computation.relation-meaning.")
                && kind != CORE_COMPUTATION_ARTIFACT_KIND_V1
                && kind != CORE_COMPUTATION_ARTIFACT_KIND_V2
                && kind != CORE_COMPUTATION_ARTIFACT_KIND_V3
                && kind != CORE_COMPUTATION_ARTIFACT_KIND_V4
                && kind != CORE_COMPUTATION_ARTIFACT_KIND_V5
        }) {
            return Err(failure(
                "PRIOR_COMPUTATION_ARTIFACT_VERSION_UNSUPPORTED",
                "the prior relation meaning uses an unsupported computation artifact version",
            ));
        }
        if artifact_kind.as_deref() == Some(CORE_COMPUTATION_ARTIFACT_KIND_V5) {
            let artifact: CoreComputationArtifactGeneratedContent = serde_json::from_value(value)?;
            verify_generated_relation_replay(
                store,
                (actor, case),
                previous,
                plan,
                compute_ref,
                &artifact,
                &[],
            )?;
            return Ok(Some(PriorRelationMeaning::AdoptedConclusion {
                proposition: artifact.relation_meaning,
                proof_dependencies: artifact.relation_proof_dependencies,
            }));
        }
        if artifact_kind.as_deref() == Some(CORE_COMPUTATION_ARTIFACT_KIND_V1) {
            let artifact: CoreComputationArtifact = serde_json::from_value(value)?;
            if artifact.kind != CORE_COMPUTATION_ARTIFACT_KIND_V1
                || artifact.calculation != previous.calculation
                || artifact.units != plan.required_units
                || artifact.evidence != previous.evidence
                || artifact.dependencies != previous.dependencies
                || artifact.relation_proof_dependencies != previous.dependencies
            {
                return Err(failure(
                    "PRIOR_COMPUTATION_ARTIFACT_MISMATCH",
                    "the retained legacy relation meaning does not match its committed computation",
                ));
            }
            return Ok(Some(PriorRelationMeaning::AdoptedConclusion {
                proposition: artifact.relation_meaning,
                proof_dependencies: artifact.relation_proof_dependencies,
            }));
        }
        if artifact_kind.as_deref() == Some(CORE_COMPUTATION_ARTIFACT_KIND_V2)
            || artifact_kind.as_deref() == Some(CORE_COMPUTATION_ARTIFACT_KIND_V3)
            || artifact_kind.as_deref() == Some(CORE_COMPUTATION_ARTIFACT_KIND)
        {
            let artifact: CoreComputationArtifactContent = serde_json::from_value(value)?;
            let replay = language::compose_typed_meaning(&artifact.content_request).ok();
            let by_id = artifact
                .content_request
                .units
                .iter()
                .map(|unit| (unit.id.as_str(), unit.text.as_str()))
                .collect::<BTreeMap<_, _>>();
            let replayed_units = artifact
                .content_composition
                .required_unit_ids
                .iter()
                .map(|id| by_id.get(id.as_str()).map(|text| (*text).to_owned()))
                .collect::<Option<Vec<_>>>();
            let proof_ids = artifact
                .relation_proof_dependencies
                .iter()
                .cloned()
                .collect::<BTreeSet<_>>();
            let composed_evidence = artifact
                .content_composition
                .evidence_ids
                .iter()
                .cloned()
                .collect::<BTreeSet<_>>();
            let meaning_plan_matches = if artifact.kind == CORE_COMPUTATION_ARTIFACT_KIND {
                artifact.relation_meaning_plan.as_ref().is_some_and(|plan| {
                    plan.conclusion == artifact.relation_meaning
                        && plan.dependencies == artifact.relation_proof_dependencies
                        && plan.required_units
                            == artifact
                                .content_request
                                .units
                                .iter()
                                .map(|unit| unit.id.clone())
                                .collect::<Vec<_>>()
                        && relation_meaning_plan_to_composition_request(
                            plan,
                            artifact.content_request.max_output_bytes,
                        )
                        .is_ok_and(|request| request == artifact.content_request)
                })
            } else {
                artifact.relation_meaning_plan.is_none()
            };
            if artifact_kind.as_deref() != Some(artifact.kind.as_str())
                || artifact.calculation != previous.calculation
                || artifact.units != plan.required_units
                || artifact.evidence != previous.evidence
                || artifact.dependencies != previous.dependencies
                || artifact.relation_proof_dependencies != previous.dependencies
                || replay.as_ref() != Some(&artifact.content_composition)
                || replayed_units.as_ref() != Some(&artifact.units)
                || artifact.content_composition.text != previous.text
                || composed_evidence != proof_ids
                || !meaning_plan_matches
            {
                return Err(failure(
                    "PRIOR_COMPUTATION_ARTIFACT_MISMATCH",
                    "the retained relation meaning and content plan do not match their committed computation",
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

fn relation_meaning_status_matches(
    asked: &Proposition,
    conclusion: &Proposition,
    answer_status: &str,
) -> bool {
    match answer_status {
        "SUPPORTED" => asked == conclusion,
        "CONTRADICTED" => {
            let mut opposite = asked.clone();
            opposite.negative = !opposite.negative;
            opposite == *conclusion
        }
        _ => false,
    }
}

pub(crate) fn relation_answer_surface(proposition: &Proposition, contradicted: bool) -> String {
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

fn retain_relation_answer_dependencies(
    result: &mut ResultRecord,
    answer: &relations::QueryOutcome,
) {
    let mut dependencies: BTreeSet<_> = result.dependencies.drain(..).collect();
    dependencies.extend(relations::answer_dependencies(answer));
    result.dependencies = dependencies.into_iter().collect();
}

fn condition_block(answer: &relations::QueryOutcome) -> Option<(Lifecycle, &'static str, String)> {
    let status = answer.condition_status?;
    if status == ConditionStatus::True {
        return None;
    }
    let code = match status {
        ConditionStatus::True => return None,
        ConditionStatus::False => "CONDITION_FALSE",
        ConditionStatus::Unknown => "CONDITION_UNKNOWN",
        ConditionStatus::Conflict => "CONDITION_CONFLICT",
    };
    let lifecycle = if status == ConditionStatus::False {
        Lifecycle::Reject
    } else {
        Lifecycle::Suspend
    };
    let detail = answer
        .condition_evaluations
        .iter()
        .map(|evaluation| {
            format!(
                "{}={:?}{}",
                evaluation.condition_id,
                evaluation.status,
                evaluation
                    .reason
                    .as_deref()
                    .map(|reason| format!("({reason})"))
                    .unwrap_or_default()
            )
        })
        .collect::<Vec<_>>()
        .join("、");
    Some((
        lifecycle,
        code,
        format!(
            "条件評価が{:?}のため、推論と採用を停止しました。{}",
            status, detail
        ),
    ))
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

/// Build a typed, sentence-free meaning plan from one verified relation proof.
fn relation_atomic_meaning_plan(
    memory: &MemoryState,
    asked: &Proposition,
    explained: &Proposition,
    answer: &relations::QueryOutcome,
    proof_dependencies: &[String],
) -> Result<CoreRelationMeaningPlan> {
    use language::{
        MeaningCompositionFormat, MeaningCompositionPurpose, MeaningCompositionStyle, MeaningRole,
    };

    if !matches!(answer.status.as_str(), "SUPPORTED" | "CONTRADICTED") {
        return Err(failure(
            "RELATION_EXPLANATION_UNSUPPORTED",
            "only a supported or refuted relation has a content plan",
        ));
    }

    let expected = proof_dependencies.iter().cloned().collect::<BTreeSet<_>>();
    let mut proof_facts_by_id = memory
        .facts
        .iter()
        .filter(|fact| expected.contains(&fact.id) && fact.valid)
        .map(|fact| (fact.id.clone(), fact.clone()))
        .collect::<BTreeMap<_, _>>();
    for fact in &answer.facts {
        if expected.contains(&fact.id) && fact.valid {
            proof_facts_by_id.insert(fact.id.clone(), fact.clone());
        }
    }
    let proof_facts = proof_facts_by_id.into_values().collect::<Vec<_>>();
    let proof_rules = memory
        .rules
        .iter()
        .filter(|rule| expected.contains(&rule.id) && rule.valid)
        .cloned()
        .collect::<Vec<_>>();
    let target_facts = proof_facts
        .iter()
        .filter(|fact| fact.proposition == *explained)
        .cloned()
        .collect::<Vec<_>>();
    if target_facts.is_empty() {
        return Err(failure(
            "CONTENT_PROOF_MAPPING",
            "the selected explanation target is absent from the verified relation proof",
        ));
    }

    let unit_id = |kind: &str, source: &str| -> Result<String> {
        let fingerprint = identity(&("core-relation-content-unit-v2", kind, source))?;
        Ok(format!("{kind}_{}", &fingerprint[..40]))
    };
    let mut units = Vec::<CoreSemanticMeaningUnit>::new();
    let mut rule_units = BTreeMap::<String, String>::new();
    let mut derived_fact_unit_ids = Vec::new();

    let claim_id = unit_id(
        "claim",
        &identity(&(asked, explained, answer.status.as_str()))?,
    )?;
    units.push(CoreSemanticMeaningUnit {
        id: claim_id.clone(),
        role: MeaningRole::Claim,
        meaning: CoreSemanticMeaning::Claim {
            asked: asked.clone(),
            conclusion: explained.clone(),
            answer_status: answer.status.clone(),
            guarantee: relations::answer_guarantee(answer),
        },
        evidence_ids: target_facts.iter().map(|fact| fact.id.clone()).collect(),
        dependencies: Vec::new(),
    });

    for condition in &answer.condition_evaluations {
        if condition.status != ConditionStatus::True {
            return Err(failure(
                "CONDITION_NOT_SATISFIED",
                "content planning requires every applicability condition to evaluate TRUE",
            ));
        }
        let unit_id = unit_id(
            "condition",
            &identity(&(
                &condition.condition_id,
                condition.status,
                &condition.dependencies,
            ))?,
        )?;
        units.push(CoreSemanticMeaningUnit {
            id: unit_id,
            role: MeaningRole::Evidence,
            meaning: CoreSemanticMeaning::Condition {
                evaluation: condition.clone(),
            },
            evidence_ids: condition.dependencies.clone(),
            dependencies: vec![claim_id.clone()],
        });
    }

    let mut direct_fact_unit_ids = Vec::new();
    let mut ordered_facts = proof_facts.clone();
    ordered_facts.sort_by(|left, right| left.id.cmp(&right.id));
    for fact in &ordered_facts {
        if fact.proposition == *explained && fact.guarantee != Guarantee::UserPremise {
            continue;
        }
        let (kind, role) = match fact.guarantee {
            Guarantee::UserPremise if fact.dependencies.is_empty() => {
                ("premise", MeaningRole::Evidence)
            }
            Guarantee::Derived => ("derived", MeaningRole::Detail),
            Guarantee::UserPremise => {
                return Err(failure(
                    "CONTENT_PROOF_MAPPING",
                    "a proof premise has an unexpected dependency chain",
                ));
            }
            _ => {
                return Err(failure(
                    "CONTENT_PROOF_MAPPING",
                    "the relation proof contains a guarantee without a supported content role",
                ));
            }
        };
        let id = unit_id(kind, &fact.id)?;
        if role == MeaningRole::Evidence {
            direct_fact_unit_ids.push(id.clone());
        } else if role == MeaningRole::Detail {
            derived_fact_unit_ids.push(id.clone());
        }
        units.push(CoreSemanticMeaningUnit {
            id,
            role,
            meaning: CoreSemanticMeaning::Fact {
                proposition: fact.proposition.clone(),
                guarantee: fact.guarantee.clone(),
                record_id: fact.id.clone(),
                record_dependencies: fact.dependencies.clone(),
            },
            evidence_ids: vec![fact.id.clone()],
            dependencies: Vec::new(),
        });
    }

    let mut ordered_rules = proof_rules.clone();
    ordered_rules.sort_by(|left, right| left.id.cmp(&right.id));
    for rule in &ordered_rules {
        let id = unit_id("rule", &rule.id)?;
        rule_units.insert(rule.id.clone(), id.clone());
    }
    for rule in &ordered_rules {
        let id = rule_units.get(&rule.id).cloned().ok_or_else(|| {
            failure(
                "CONTENT_PROOF_MAPPING",
                "a verified proof rule is missing its content unit ID",
            )
        })?;
        let mut evidence_ids = vec![rule.id.clone()];
        evidence_ids.extend(rule.dependencies.iter().cloned());
        evidence_ids.sort();
        evidence_ids.dedup();
        let composed = rule.source_input == "derived:proof-composition-v1";
        let (role, dependencies) = if composed {
            if rule.dependencies.len() != 2 {
                return Err(failure(
                    "CONTENT_PROOF_MAPPING",
                    "a proof-composition rule must retain exactly two parent rule IDs",
                ));
            }
            let parent_units = rule
                .dependencies
                .iter()
                .map(|dependency| {
                    rule_units.get(dependency).cloned().ok_or_else(|| {
                        failure(
                            "CONTENT_PROOF_MAPPING",
                            "a proof-composition parent rule is absent from the verified proof",
                        )
                    })
                })
                .collect::<Result<Vec<_>>>()?;
            (MeaningRole::Detail, parent_units)
        } else {
            (MeaningRole::Rule, direct_fact_unit_ids.clone())
        };
        units.push(CoreSemanticMeaningUnit {
            id,
            role,
            meaning: CoreSemanticMeaning::Rule {
                premises: std::iter::once(&rule.premise)
                    .chain(rule.additional_premises.iter())
                    .cloned()
                    .collect(),
                conclusion: rule.conclusion.clone(),
                record_id: rule.id.clone(),
                record_dependencies: rule.dependencies.clone(),
                composed,
            },
            evidence_ids,
            dependencies,
        });
    }

    let rule_unit_ids = rule_units.values().cloned().collect::<Vec<_>>();
    for unit in &mut units {
        if derived_fact_unit_ids.contains(&unit.id) {
            let mut dependencies = direct_fact_unit_ids.clone();
            dependencies.extend(rule_unit_ids.iter().cloned());
            dependencies.sort();
            dependencies.dedup();
            unit.dependencies = dependencies;
        }
    }

    let represented_record_ids = ordered_facts
        .iter()
        .map(|fact| fact.id.clone())
        .chain(rule_units.keys().cloned())
        .collect::<BTreeSet<_>>();
    for reference in expected.difference(&represented_record_ids) {
        let mut dependencies = direct_fact_unit_ids.clone();
        dependencies.extend(rule_unit_ids.iter().cloned());
        dependencies.sort();
        dependencies.dedup();
        units.push(CoreSemanticMeaningUnit {
            id: unit_id("proofref", reference)?,
            role: MeaningRole::Detail,
            meaning: CoreSemanticMeaning::ProofReference {
                record_id: reference.clone(),
            },
            evidence_ids: vec![reference.clone()],
            dependencies,
        });
    }

    let mut limitation_dependencies = units.iter().map(|unit| unit.id.clone()).collect::<Vec<_>>();
    limitation_dependencies.sort();
    limitation_dependencies.dedup();
    units.push(CoreSemanticMeaningUnit {
        id: unit_id(
            "limitation",
            &identity(&(explained.scope.as_str(), &expected))?,
        )?,
        role: MeaningRole::Limitation,
        meaning: CoreSemanticMeaning::Limitation {
            scope: explained.scope.clone(),
        },
        evidence_ids: Vec::new(),
        dependencies: limitation_dependencies,
    });

    let represented = units
        .iter()
        .flat_map(|unit| unit.evidence_ids.iter().cloned())
        .collect::<BTreeSet<_>>();
    if represented != expected {
        return Err(failure(
            "CONTENT_PROOF_MAPPING",
            "Core content units do not represent the complete relation proof dependency set",
        ));
    }
    let required_units = units.iter().map(|unit| unit.id.clone()).collect();
    let mut plan = CoreRelationMeaningPlan {
        schema: CORE_RELATION_MEANING_PLAN_SCHEMA.into(),
        id: String::new(),
        purpose: MeaningCompositionPurpose::Explain,
        format: MeaningCompositionFormat::Paragraph,
        style: MeaningCompositionStyle::Neutral,
        asked: asked.clone(),
        conclusion: explained.clone(),
        answer_status: answer.status.clone(),
        guarantee: relations::answer_guarantee(answer),
        condition_evaluations: answer.condition_evaluations.clone(),
        units,
        required_units,
        dependencies: expected.into_iter().collect(),
    };
    plan.id = relation_meaning_plan_id(&plan)?;
    Ok(plan)
}

fn relation_meaning_plan_id(plan: &CoreRelationMeaningPlan) -> Result<String> {
    identity(&(
        plan.schema.as_str(),
        plan.purpose,
        plan.format,
        plan.style,
        &plan.asked,
        &plan.conclusion,
        plan.answer_status.as_str(),
        plan.guarantee.clone(),
        &plan.condition_evaluations,
        &plan.units,
        &plan.required_units,
        &plan.dependencies,
    ))
}

fn relation_meaning_plan_to_composition_request(
    plan: &CoreRelationMeaningPlan,
    max_output_bytes: usize,
) -> Result<language::MeaningCompositionRequest> {
    use language::TypedMeaningUnit;

    if plan.schema != CORE_RELATION_MEANING_PLAN_SCHEMA
        || plan.id != relation_meaning_plan_id(plan)?
        || plan
            .units
            .iter()
            .map(|unit| unit.id.clone())
            .collect::<Vec<_>>()
            != plan.required_units
        || plan.units.iter().any(|unit| unit.id.trim().is_empty())
        || plan
            .condition_evaluations
            .iter()
            .any(|evaluation| evaluation.status != ConditionStatus::True)
    {
        return Err(failure(
            "CONTENT_PLAN_INVALID",
            "the Core semantic meaning plan failed its identity or applicability checks",
        ));
    }
    let expected = plan.dependencies.iter().cloned().collect::<BTreeSet<_>>();
    let represented = plan
        .units
        .iter()
        .flat_map(|unit| unit.evidence_ids.iter().cloned())
        .collect::<BTreeSet<_>>();
    if represented != expected {
        return Err(failure(
            "CONTENT_PROOF_MAPPING",
            "the Core semantic meaning plan does not retain the complete proof dependency set",
        ));
    }
    let mut unit_ids = BTreeSet::new();
    let units = plan
        .units
        .iter()
        .map(|unit| {
            if !unit_ids.insert(unit.id.as_str()) {
                return Err(failure(
                    "CONTENT_PLAN_INVALID",
                    "the Core semantic meaning plan repeats a unit identity",
                ));
            }
            Ok(TypedMeaningUnit {
                id: unit.id.clone(),
                role: unit.role,
                text: realize_core_semantic_meaning(&unit.meaning)?,
                evidence_ids: unit.evidence_ids.clone(),
                dependencies: unit.dependencies.clone(),
            })
        })
        .collect::<Result<Vec<_>>>()?;
    let max_output_bytes = max_output_bytes.min(8192);
    if max_output_bytes == 0 {
        return Err(failure(
            "CONTENT_PLAN_UNAVAILABLE",
            "the request has no output byte budget for a relation explanation",
        ));
    }
    Ok(language::MeaningCompositionRequest {
        purpose: plan.purpose,
        format: plan.format,
        style: plan.style,
        units,
        prior_sequence: Vec::new(),
        max_output_bytes,
        max_tokens: max_output_bytes,
    })
}

fn relation_explanation_output_budget(meaning: &MeaningIR, budget: usize) -> usize {
    if input::uses_generated_relation_explanation(meaning) {
        budget
    } else {
        budget.min(LEGACY_RELATION_GENERATION_BYTES)
    }
}

fn relation_meaning_plan_to_generation_request(
    plan: &CoreRelationMeaningPlan,
    max_output_bytes: usize,
) -> Result<language::MeaningGenerationRequest> {
    if plan.schema != CORE_RELATION_MEANING_PLAN_SCHEMA
        || plan.id != relation_meaning_plan_id(plan)?
        || plan
            .units
            .iter()
            .map(|unit| unit.id.clone())
            .collect::<Vec<_>>()
            != plan.required_units
        || plan.units.iter().any(|unit| unit.id.trim().is_empty())
        || plan
            .condition_evaluations
            .iter()
            .any(|evaluation| evaluation.status != ConditionStatus::True)
    {
        return Err(failure(
            "CONTENT_PLAN_INVALID",
            "the Core semantic meaning plan failed its identity or applicability checks",
        ));
    }
    let expected = plan.dependencies.iter().cloned().collect::<BTreeSet<_>>();
    let represented = plan
        .units
        .iter()
        .flat_map(|unit| unit.evidence_ids.iter().cloned())
        .collect::<BTreeSet<_>>();
    if represented != expected {
        return Err(failure(
            "CONTENT_PROOF_MAPPING",
            "the Core semantic meaning plan does not retain the complete proof dependency set",
        ));
    }
    let mut unit_ids = BTreeSet::new();
    let units = plan
        .units
        .iter()
        .map(|unit| {
            if !unit_ids.insert(unit.id.as_str()) {
                return Err(failure(
                    "CONTENT_PLAN_INVALID",
                    "the Core semantic meaning plan repeats a unit identity",
                ));
            }
            Ok(language::MeaningGrammarUnit {
                id: unit.id.clone(),
                role: unit.role,
                evidence_ids: unit.evidence_ids.clone(),
                dependencies: unit.dependencies.clone(),
                semantic_sha256: identity(&(
                    "core-semantic-meaning-unit-v1",
                    &unit.id,
                    unit.role,
                    &unit.meaning,
                    &unit.evidence_ids,
                    &unit.dependencies,
                ))?,
                fragments: core_semantic_meaning_fragments(&unit.meaning)?,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    let max_output_bytes = max_output_bytes.min(language::MAX_RELATION_GENERATION_BYTES);
    if max_output_bytes == 0 {
        return Err(failure(
            "CONTENT_PLAN_UNAVAILABLE",
            "the request has no output byte budget for a relation explanation",
        ));
    }
    Ok(language::MeaningGenerationRequest {
        purpose: plan.purpose,
        format: plan.format,
        style: plan.style,
        units,
        max_output_bytes,
        max_tokens: max_output_bytes,
    })
}

fn grammar_choice(slot: &str, alternatives: &[&str]) -> language::MeaningGrammarFragment {
    language::MeaningGrammarFragment::Choice {
        slot: slot.into(),
        alternatives: alternatives.iter().map(|value| (*value).into()).collect(),
    }
}

fn append_proposition_surface(
    fragments: &mut Vec<language::MeaningGrammarFragment>,
    proposition: &Proposition,
    slot_prefix: &str,
) {
    use language::MeaningGrammarFragment::Literal;

    let godan_negative = if proposition.negative {
        japanese_godan_negative_form(&proposition.predicate)
    } else {
        Some(proposition.predicate.clone())
    };
    let supported_verb = matches!(proposition.predicate.as_str(), "is" | "eat" | "use")
        || (["う", "く", "ぐ", "す", "つ", "ぬ", "ぶ", "む", "る"]
            .iter()
            .any(|ending| proposition.predicate.ends_with(ending))
            && godan_negative.is_some());
    if !supported_verb {
        append_unclassified_proposition_surface(fragments, proposition, slot_prefix);
    } else {
        fragments.push(Literal {
            value: "「".into()
        });
        fragments.push(Literal {
            value: proposition.subject.clone(),
        });
        fragments.push(grammar_choice(
            &format!("{slot_prefix}_subject_particle"),
            &["」は", "」については"],
        ));
    }
    if supported_verb {
        match proposition.predicate.as_str() {
            "is" => {
                fragments.push(Literal {
                    value: "「".into()
                });
                fragments.push(Literal {
                    value: proposition.object.clone(),
                });
                fragments.push(grammar_choice(
                    &format!("{slot_prefix}_predicate"),
                    if proposition.negative {
                        &["」ではない", "」ではありません"]
                    } else {
                        &["」である", "」です"]
                    },
                ));
            }
            "eat" | "use" => {
                fragments.push(Literal {
                    value: "「".into()
                });
                fragments.push(Literal {
                    value: proposition.object.clone(),
                });
                fragments.push(Literal {
                    value: "」を".into(),
                });
                fragments.push(grammar_choice(
                    &format!("{slot_prefix}_predicate"),
                    match (proposition.predicate.as_str(), proposition.negative) {
                        ("eat", false) => &["食べる", "食べます"],
                        ("eat", true) => &["食べない", "食べません"],
                        ("use", false) => &["使う", "使います"],
                        ("use", true) => &["使わない", "使いません"],
                        _ => unreachable!(),
                    },
                ));
            }
            predicate
                if ["う", "く", "ぐ", "す", "つ", "ぬ", "ぶ", "む", "る"]
                    .iter()
                    .any(|ending| predicate.ends_with(ending)) =>
            {
                fragments.push(Literal {
                    value: "「".into()
                });
                fragments.push(Literal {
                    value: proposition.object.clone(),
                });
                fragments.push(Literal {
                    value: "」を".into(),
                });
                fragments.push(Literal {
                    value: godan_negative
                        .clone()
                        .unwrap_or_else(|| predicate.to_owned()),
                });
            }
            _ => unreachable!("only supported verbs reach the natural relation grammar"),
        }
    }
    if let Some(time) = &proposition.temporal_scope {
        fragments.push(Literal {
            value: format!("（時刻範囲: {time}）"),
        });
    }
}

fn append_unclassified_proposition_surface(
    fragments: &mut Vec<language::MeaningGrammarFragment>,
    proposition: &Proposition,
    slot_prefix: &str,
) {
    use language::MeaningGrammarFragment::Literal;

    fragments.push(Literal {
        value: format!(
            "「{}」について述語「{}」・対象「",
            proposition.subject, proposition.predicate
        ),
    });
    fragments.push(Literal {
        value: proposition.object.clone(),
    });
    fragments.push(Literal {
        value: "」の関係は".into(),
    });
    fragments.push(grammar_choice(
        &format!("{slot_prefix}_polarity"),
        if proposition.negative {
            &["否定されています", "否定関係です"]
        } else {
            &["肯定されています", "肯定関係です"]
        },
    ));
}

fn core_semantic_meaning_fragments(
    meaning: &CoreSemanticMeaning,
) -> Result<Vec<language::MeaningGrammarFragment>> {
    use language::MeaningGrammarFragment::Literal;
    let mut fragments = Vec::new();
    match meaning {
        CoreSemanticMeaning::Claim {
            asked,
            conclusion,
            answer_status,
            guarantee,
        } if answer_status == "CONTRADICTED" => {
            fragments.push(grammar_choice(
                "claim_opening",
                &["質問された関係「", "確認対象の関係「"],
            ));
            append_proposition_surface(&mut fragments, asked, "asked");
            fragments.push(Literal {
                value: "」は支持されず、保存記録では反対の関係「".into(),
            });
            append_proposition_surface(&mut fragments, conclusion, "conclusion");
            fragments.push(Literal {
                value: format!("」が支えられています（範囲: {}）。", conclusion.scope),
            });
        }
        CoreSemanticMeaning::Claim {
            conclusion,
            guarantee: Guarantee::UserPremise,
            ..
        } => {
            fragments.push(grammar_choice(
                "claim_opening",
                &["有効な保存前提では「", "保存された入力前提では「"],
            ));
            append_proposition_surface(&mut fragments, conclusion, "conclusion");
            fragments.push(Literal {
                value: format!("」が記録されています（範囲: {}）。", conclusion.scope),
            });
        }
        CoreSemanticMeaning::Claim {
            conclusion,
            guarantee: Guarantee::Derived,
            ..
        } => {
            fragments.push(grammar_choice(
                "claim_opening",
                &["保存前提と規則から「", "適用した保存規則により「"],
            ));
            append_proposition_surface(&mut fragments, conclusion, "conclusion");
            fragments.push(Literal {
                value: format!("」が導かれています（範囲: {}）。", conclusion.scope),
            });
        }
        CoreSemanticMeaning::Claim {
            conclusion,
            guarantee,
            ..
        } => {
            fragments.push(grammar_choice(
                "claim_opening",
                &["保存された根拠は「", "有効な記録は「"],
            ));
            append_proposition_surface(&mut fragments, conclusion, "conclusion");
            fragments.push(Literal {
                value: format!(
                    "」を{}として支えています（範囲: {}）。",
                    guarantee_label(guarantee),
                    conclusion.scope
                ),
            });
        }
        CoreSemanticMeaning::Condition { evaluation } => {
            if evaluation.status != ConditionStatus::True {
                return Err(failure(
                    "CONDITION_NOT_SATISFIED",
                    "semantic generation requires a true applicability condition",
                ));
            }
            fragments.push(grammar_choice(
                "condition_subject",
                &["適用条件", "評価した条件"],
            ));
            fragments.push(Literal {
                value: format!("「{}」は", evaluation.condition_id),
            });
            fragments.push(grammar_choice(
                "condition_value",
                &["TRUEです。", "TRUEと評価しました。"],
            ));
        }
        CoreSemanticMeaning::Fact {
            proposition,
            guarantee: Guarantee::UserPremise,
            record_id,
            record_dependencies,
        } if record_dependencies.is_empty() => {
            fragments.push(grammar_choice(
                "fact_opening",
                &["直接提示された前提として「", "入力時の前提として「"],
            ));
            append_proposition_surface(&mut fragments, proposition, "fact");
            fragments.push(Literal {
                value: format!(
                    "」が記録されています（範囲: {}; 前提ID: {}）。",
                    proposition.scope, record_id
                ),
            });
        }
        CoreSemanticMeaning::Fact {
            proposition,
            guarantee: Guarantee::Derived,
            record_id,
            ..
        } => {
            fragments.push(grammar_choice(
                "fact_opening",
                &["証明に含まれる導出記録は「", "導出された関係は「"],
            ));
            append_proposition_surface(&mut fragments, proposition, "fact");
            fragments.push(Literal {
                value: format!(
                    "」です（範囲: {}; 導出ID: {}）。",
                    proposition.scope, record_id
                ),
            });
        }
        CoreSemanticMeaning::Fact { .. } => {
            return Err(failure(
                "CONTENT_PROOF_MAPPING",
                "a semantic fact unit has no supported provenance role",
            ));
        }
        CoreSemanticMeaning::Rule {
            record_dependencies,
            record_id,
            composed: true,
            ..
        } => {
            if record_dependencies.len() != 2 {
                return Err(failure(
                    "CONTENT_PROOF_MAPPING",
                    "a composed semantic rule must retain exactly two parent rule IDs",
                ));
            }
            fragments.push(grammar_choice(
                "rule_opening",
                &[
                    "合成規則をこの証明で使用しました",
                    "この証明では短縮規則を適用しました",
                ],
            ));
            fragments.push(Literal {
                value: format!(
                    "（規則ID: {}; 親規則ID: {}）。",
                    record_id,
                    record_dependencies.join("、")
                ),
            });
        }
        CoreSemanticMeaning::Rule {
            premises,
            conclusion,
            record_id,
            composed: false,
            ..
        } => {
            if premises.is_empty() {
                return Err(failure(
                    "CONTENT_PROOF_MAPPING",
                    "a semantic rule must have at least one typed premise",
                ));
            }
            fragments.push(grammar_choice(
                "rule_opening",
                &["保存規則では", "記録された規則を適用すると"],
            ));
            for (index, premise) in premises.iter().enumerate() {
                if index > 0 {
                    fragments.push(grammar_choice("rule_premise_join", &["かつ", "同時に"]));
                }
                append_proposition_surface(&mut fragments, premise, &format!("premise_{index}"));
            }
            fragments.push(Literal {
                value: "なら".into(),
            });
            append_proposition_surface(&mut fragments, conclusion, "rule_conclusion");
            fragments.push(Literal {
                value: format!(
                    "です（結論範囲: {}; 規則ID: {}）。",
                    conclusion.scope, record_id
                ),
            });
        }
        CoreSemanticMeaning::ProofReference { record_id } => {
            fragments.push(grammar_choice(
                "proof_reference_opening",
                &["証明に使われた参照記録は", "証明で参照した記録IDは"],
            ));
            fragments.push(Literal {
                value: format!("「{}」です。", record_id),
            });
        }
        CoreSemanticMeaning::Limitation { scope } => {
            fragments.push(grammar_choice(
                "limitation_opening",
                &["この説明の対象範囲は", "説明が適用される範囲は"],
            ));
            fragments.push(Literal {
                value: format!(
                    "「{}」です。有効な保存記録に限った説明で、現実世界の真偽を確認したものではありません。",
                    scope
                ),
            });
        }
    }
    Ok(fragments)
}

fn validate_relation_meaning_generation(
    plan: &CoreRelationMeaningPlan,
    request: &language::MeaningGenerationRequest,
    report: &language::MeaningGenerationReport,
) -> Result<()> {
    let expected_request =
        relation_meaning_plan_to_generation_request(plan, request.max_output_bytes)?;
    if expected_request != *request {
        return Err(failure(
            "MEANING_REVERSE_CHECK",
            "the generation grammar differs from the Core relation meaning plan",
        ));
    }
    validate_meaning_generation(request, &plan.dependencies, report)
}

fn validate_meaning_generation(
    request: &language::MeaningGenerationRequest,
    proof_ids: &[String],
    report: &language::MeaningGenerationReport,
) -> Result<()> {
    if report.schema != "core-meaning-generation-v1"
        || report.grammar_version != "core-typed-meaning-fragment-grammar-v1"
        || report.stop_reason != "EOS"
        || report.max_output_bytes != request.max_output_bytes
        || report.max_tokens != request.max_tokens
        || report.units.len() != request.units.len()
        || report.required_unit_ids.len() != request.units.len()
        || report.text.len() > report.max_output_bytes
        || report.decoder_token_count != report.token_steps.len()
        || report
            .token_steps
            .last()
            .is_none_or(|step| step.token != "<EOS>")
        || report.lossless_projection.recover_bytes()? != report.text.as_bytes()
    {
        return Err(failure(
            "MEANING_REVERSE_CHECK",
            "generated report failed its schema, dependency, EOS, budget or byte round trip",
        ));
    }
    let expected_unit_ids = request
        .units
        .iter()
        .map(|unit| unit.id.clone())
        .collect::<BTreeSet<_>>();
    let traced_unit_ids = report
        .units
        .iter()
        .map(|unit| unit.unit_id.clone())
        .collect::<BTreeSet<_>>();
    let reported_unit_ids = report
        .required_unit_ids
        .iter()
        .cloned()
        .collect::<BTreeSet<_>>();
    let traced_unit_order = report
        .units
        .iter()
        .map(|unit| unit.unit_id.clone())
        .collect::<Vec<_>>();
    if expected_unit_ids != traced_unit_ids
        || expected_unit_ids != reported_unit_ids
        || report.required_unit_ids != traced_unit_order
    {
        return Err(failure(
            "MEANING_REVERSE_CHECK",
            "the generated sequence does not contain each planned semantic unit exactly once",
        ));
    }
    let expected_dependencies = proof_ids.iter().cloned().collect::<BTreeSet<_>>();
    let actual_dependencies = report.evidence_ids.iter().cloned().collect::<BTreeSet<_>>();
    if expected_dependencies != actual_dependencies {
        return Err(failure(
            "CONTENT_PROOF_MAPPING",
            "generated semantic units do not retain every relation proof dependency",
        ));
    }
    let mut previous_end = 0usize;
    let mut emitted_ids = BTreeSet::new();
    for trace in &report.units {
        let grammar_unit = request
            .units
            .iter()
            .find(|unit| unit.id == trace.unit_id)
            .ok_or_else(|| {
                failure(
                    "MEANING_REVERSE_CHECK",
                    "the generated sequence references an unknown semantic grammar unit",
                )
            })?;
        let expected_surface_sha256 = identity(&(
            grammar_unit.id.as_str(),
            grammar_unit.role,
            &grammar_unit.evidence_ids,
            &grammar_unit.dependencies,
            grammar_unit.semantic_sha256.as_str(),
            trace.text.as_str(),
        ))?;
        let Some(surface) = report.text.get(trace.byte_start..trace.byte_end) else {
            return Err(failure(
                "MEANING_REVERSE_CHECK",
                "a generated unit span is outside the final output bytes",
            ));
        };
        if trace.unit_id != grammar_unit.id
            || trace.role != grammar_unit.role
            || trace.evidence_ids != grammar_unit.evidence_ids
            || trace.dependencies != grammar_unit.dependencies
            || trace.semantic_sha256 != grammar_unit.semantic_sha256
            || trace.surface_sha256 != expected_surface_sha256
            || trace.byte_start != previous_end
            || surface != trace.text
            || trace.text.is_empty()
            || trace
                .dependencies
                .iter()
                .any(|dependency| !emitted_ids.contains(dependency))
            || !emitted_ids.insert(trace.unit_id.clone())
        {
            return Err(failure(
                "MEANING_REVERSE_CHECK",
                "generated output cannot be reversed to its exact Core meaning unit and proof IDs",
            ));
        }
        previous_end = trace.byte_end;
    }
    if previous_end != report.text.len() {
        return Err(failure(
            "MEANING_REVERSE_CHECK",
            "the generated unit spans do not cover the complete output byte sequence",
        ));
    }
    let text_token_count = report
        .decoder_token_count
        .checked_sub(1)
        .ok_or_else(|| failure("MEANING_REVERSE_CHECK", "the explicit EOS step is missing"))?;
    let expected_round_trip = identity(&(
        "core-meaning-generation-v1",
        &request.units,
        &report.units,
        report.text.as_str(),
        &report.lossless_projection.raw_sha256,
        report.sequence_token_count,
        text_token_count,
    ))?;
    if expected_round_trip != report.semantic_round_trip_sha256
        || report.lossless_projection.raw_sha256 != digest(report.text.as_bytes())
    {
        return Err(failure(
            "MEANING_REVERSE_CHECK",
            "generated semantic projection checksum does not round trip",
        ));
    }
    Ok(())
}

/// K7 adoption boundary for generated AskWhy content. Rebuild the
/// plan from the same live proof state, then bind its exact conclusion,
/// guarantee, conditions and dependencies to the generated trace and K3 result.
struct RelationMeaningAdoptionInput<'a> {
    memory: &'a MemoryState,
    asked: &'a Proposition,
    conclusion: &'a Proposition,
    answer: &'a relations::QueryOutcome,
    result: &'a ResultRecord,
    plan: &'a CoreRelationMeaningPlan,
    request: &'a language::MeaningGenerationRequest,
    report: &'a language::MeaningGenerationReport,
    emission: &'a language::ModelEmission,
}

fn validate_relation_meaning_adoption(input: RelationMeaningAdoptionInput<'_>) -> Result<()> {
    let RelationMeaningAdoptionInput {
        memory,
        asked,
        conclusion,
        answer,
        result,
        plan,
        request,
        report,
        emission,
    } = input;
    let dependencies = relations::answer_dependencies(answer);
    let guarantee = relations::answer_guarantee(answer);
    let expected_plan =
        relation_atomic_meaning_plan(memory, asked, conclusion, answer, &dependencies)?;
    let expected_request =
        relation_meaning_plan_to_generation_request(plan, request.max_output_bytes)?;
    let report_dependencies = report.evidence_ids.iter().cloned().collect::<BTreeSet<_>>();
    let expected_dependencies = dependencies.iter().cloned().collect::<BTreeSet<_>>();
    if relation_explanation_target(asked, answer).as_ref() != Some(conclusion)
        || !relation_meaning_status_matches(asked, conclusion, &answer.status)
        || result.dependencies != dependencies
        || result.guarantee != guarantee
        || plan != &expected_plan
        || plan.asked != *asked
        || plan.conclusion != *conclusion
        || plan.answer_status != answer.status
        || plan.guarantee != guarantee
        || plan.condition_evaluations != answer.condition_evaluations
        || plan.dependencies != dependencies
        || expected_request != *request
        || request.max_tokens != request.max_output_bytes
        || report.model_artifact_ref.is_some()
        || report_dependencies != expected_dependencies
        || report.text != emission.text
        || report.model_revision != emission.model_revision
        || report.lossless_projection != emission.lossless_projection
    {
        return Err(failure(
            "CONTENT_ADOPTION_MISMATCH",
            "the generated meaning, live K3 proof, applicability state, guarantee or output observation disagree",
        ));
    }
    validate_relation_meaning_generation(plan, request, report)
}

/// Verify the exact fields that K7 is about to seal in ContentPlan. A
/// mismatched generated unit, proof dependency, model artifact or observed
/// emission remains unadopted.
fn validate_generated_content_plan_adoption(
    result: &ResultRecord,
    report: &language::MeaningGenerationReport,
    emission: &language::ModelEmission,
    proof_ids: &[String],
    compute_artifact_ref: &str,
    locale: &str,
    plan: &ContentPlan,
) -> Result<()> {
    let expected_units = report
        .units
        .iter()
        .map(|unit| unit.text.clone())
        .collect::<Vec<_>>();
    let mut expected_dependencies = result.dependencies.clone();
    expected_dependencies.push(compute_artifact_ref.to_owned());
    let observation_ref = digest(&canonical(emission)?);
    let proof_dependencies = report.evidence_ids.iter().cloned().collect::<BTreeSet<_>>();
    let expected_proof_dependencies = proof_ids.iter().cloned().collect::<BTreeSet<_>>();
    let id_matches = if plan.id.is_empty() {
        true
    } else {
        let mut unsigned_plan = plan.clone();
        let stored_id = std::mem::take(&mut unsigned_plan.id);
        identity(&unsigned_plan)? == stored_id
    };
    if !id_matches
        || plan.text != report.text
        || plan.text != emission.text
        || plan.locale != locale
        || plan.guarantee != result.guarantee
        || plan.required_units != expected_units
        || plan.dependencies != expected_dependencies
        || plan.model_revision != report.model_revision
        || plan.model_revision != emission.model_revision
        || report.model_artifact_ref.as_deref() != Some(plan.model_artifact_ref.as_str())
        || report.lossless_projection != emission.lossless_projection
        || plan.model_observation_ref != observation_ref
        || proof_dependencies != expected_proof_dependencies
    {
        return Err(failure(
            "CONTENT_ADOPTION_MISMATCH",
            "the proposed ContentPlan does not exactly seal the verified generated units, proof dependencies, model and emission",
        ));
    }
    Ok(())
}

fn realize_core_semantic_meaning(meaning: &CoreSemanticMeaning) -> Result<String> {
    match meaning {
        CoreSemanticMeaning::Claim {
            asked,
            conclusion,
            answer_status,
            guarantee,
        } if answer_status == "CONTRADICTED" => Ok(format!(
            "質問された関係「{}」は支持されず、反対の「{}」が保存記録で支えられています（範囲: {}）。",
            describe_without_condition_refs(asked),
            describe_without_condition_refs(conclusion),
            conclusion.scope
        )),
        CoreSemanticMeaning::Claim {
            conclusion,
            guarantee: Guarantee::UserPremise,
            ..
        } => Ok(format!(
            "有効な保存前提には「{}」が記録されています（範囲: {}）。",
            describe_without_condition_refs(conclusion),
            conclusion.scope
        )),
        CoreSemanticMeaning::Claim {
            conclusion,
            guarantee: Guarantee::Derived,
            ..
        } => Ok(format!(
            "保存された前提と規則から「{}」が導かれています（範囲: {}）。",
            describe_without_condition_refs(conclusion),
            conclusion.scope
        )),
        CoreSemanticMeaning::Claim { conclusion, .. } => Ok(format!(
            "保存された根拠は「{}」を支えています（範囲: {}）。",
            describe_without_condition_refs(conclusion),
            conclusion.scope
        )),
        CoreSemanticMeaning::Condition { evaluation } => Ok(format!(
            "適用条件{}をTRUEと評価しました。",
            condition_reference_label(&evaluation.condition_id)
        )),
        CoreSemanticMeaning::Fact {
            proposition,
            guarantee: Guarantee::UserPremise,
            record_id,
            record_dependencies,
        } if record_dependencies.is_empty() => Ok(format!(
            "保存前提「{}」は範囲「{}」の入力記録です（前提ID: {}）。",
            describe_without_condition_refs(proposition),
            proposition.scope,
            record_id
        )),
        CoreSemanticMeaning::Fact {
            proposition,
            guarantee: Guarantee::Derived,
            record_id,
            ..
        } => Ok(format!(
            "証明に含まれる導出記録は「{}」です（範囲: {}; 導出ID: {}）。",
            describe_without_condition_refs(proposition),
            proposition.scope,
            record_id
        )),
        CoreSemanticMeaning::Fact { .. } => Err(failure(
            "CONTENT_PROOF_MAPPING",
            "a semantic fact unit has no supported provenance role",
        )),
        CoreSemanticMeaning::Rule {
            record_dependencies,
            record_id,
            composed: true,
            ..
        } => {
            if record_dependencies.len() != 2 {
                return Err(failure(
                    "CONTENT_PROOF_MAPPING",
                    "a composed semantic rule must retain exactly two parent rule IDs",
                ));
            }
            Ok(format!(
                "Coreが親規則から合成した短縮規則をこの証明で使用しています（短縮規則ID: {}; 親規則ID: {}）。",
                record_id,
                record_dependencies.join("、")
            ))
        }
        CoreSemanticMeaning::Rule {
            premises,
            conclusion,
            record_id,
            composed: false,
            ..
        } => Ok(format!(
            "保存規則「{}」ならば「{}」です（範囲: {}; 規則ID: {}）。",
            premises
                .iter()
                .map(describe_without_condition_refs)
                .collect::<Vec<_>>()
                .join(" かつ "),
            describe_without_condition_refs(conclusion),
            conclusion.scope,
            record_id
        )),
        CoreSemanticMeaning::ProofReference { record_id } => {
            Ok(format!("証明に使われた参照記録IDは「{}」です。", record_id))
        }
        CoreSemanticMeaning::Limitation { scope } => Ok(format!(
            "この説明は範囲「{}」の有効な保存記録に限られ、現実世界の真偽を確認したものではありません。",
            scope
        )),
    }
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
        .and_then(|count| count.checked_add(memory.conditions.len()))
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
        .and_then(|count| count.checked_add(memory.conditions.len()))
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
            conditions: memory.conditions.clone(),
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

fn relation_comparison_inventory_from_index(
    index: &MemoryIndexSnapshot<'_, '_>,
    subject: &str,
    scope: &str,
    max_actions: u32,
) -> Result<(relations::RelationInventory, Vec<RelationRule>)> {
    match relations::active_scoped_relation_inventory_projection(index, subject, scope, max_actions)
    {
        Err(Error::Contract { code, .. }) if code == "MISSING_PREMISES" => Ok((
            relations::RelationInventory {
                facts: Vec::new(),
                contested_fact_ids: BTreeSet::new(),
                has_conflict: false,
                dependencies: Vec::new(),
                guarantee: Guarantee::Unknown,
            },
            Vec::new(),
        )),
        other => other.map(|(inventory, _actions, rules)| (inventory, rules)),
    }
}

type RelationComparisonKey = (
    String,
    String,
    bool,
    String,
    Option<String>,
    BTreeSet<String>,
);

fn relation_comparison_key_identity(key: &RelationComparisonKey) -> Result<String> {
    let (predicate, object, negative, scope, temporal_scope, condition_refs) = key;
    match (temporal_scope, condition_refs.is_empty()) {
        // Preserve pre-alpha-9 comparison unit identities for the legacy
        // untimed shape while making explicit time part of new identities.
        (None, true) => identity(&(predicate, object, negative, scope)),
        (Some(temporal_scope), true) => identity(&(
            "core-relation-comparison-temporal-key-v1",
            predicate,
            object,
            negative,
            scope,
            temporal_scope,
        )),
        (temporal_scope, false) => identity(&(
            "core-relation-comparison-condition-key-v1",
            predicate,
            object,
            negative,
            scope,
            temporal_scope,
            condition_refs,
        )),
    }
}

fn relation_comparison_proof_ids(
    inventory_a: &relations::RelationInventory,
    inventory_b: &relations::RelationInventory,
) -> BTreeSet<String> {
    inventory_a
        .dependencies
        .iter()
        .chain(&inventory_b.dependencies)
        .cloned()
        .chain(
            inventory_a
                .facts
                .iter()
                .chain(&inventory_b.facts)
                .flat_map(|fact| {
                    std::iter::once(fact.id.clone()).chain(fact.dependencies.iter().cloned())
                }),
        )
        .collect()
}

fn relation_comparison_supporting_rules(
    rules: &[RelationRule],
    inventory_a: &relations::RelationInventory,
    inventory_b: &relations::RelationInventory,
) -> Vec<RelationRule> {
    let proof_ids = relation_comparison_proof_ids(inventory_a, inventory_b);
    rules
        .iter()
        .filter(|rule| proof_ids.contains(&rule.id))
        .cloned()
        .collect()
}

fn relation_comparison_content_request(
    rules: &[RelationRule],
    subject_a: &str,
    subject_b: &str,
    scope: &str,
    inventory_a: &relations::RelationInventory,
    inventory_b: &relations::RelationInventory,
    max_output_bytes: usize,
) -> Result<(Option<language::MeaningCompositionRequest>, &'static str)> {
    let proof_ids = relation_comparison_proof_ids(inventory_a, inventory_b);
    let comparison_rules = rules
        .iter()
        .filter(|rule| proof_ids.contains(&rule.id))
        .cloned()
        .collect::<Vec<_>>();
    if comparison_rules.is_empty() {
        return Ok((
            relation_comparison_content_request_v2(
                subject_a,
                subject_b,
                scope,
                inventory_a,
                inventory_b,
                max_output_bytes,
            )?,
            RELATION_COMPARISON_CONTENT_KIND,
        ));
    }
    if let Some(request) = relation_comparison_content_request_v3(
        subject_a,
        subject_b,
        scope,
        inventory_a,
        inventory_b,
        &comparison_rules,
        max_output_bytes,
    )? {
        return Ok((Some(request), RELATION_COMPARISON_CONTENT_KIND_V3));
    }
    if let Some(request) = relation_comparison_content_request_v4(
        subject_a,
        subject_b,
        scope,
        inventory_a,
        inventory_b,
        &comparison_rules,
        max_output_bytes,
    )? {
        return Ok((Some(request), RELATION_COMPARISON_CONTENT_KIND_V4));
    }
    Ok((
        relation_comparison_content_request_v2(
            subject_a,
            subject_b,
            scope,
            inventory_a,
            inventory_b,
            max_output_bytes,
        )?,
        RELATION_COMPARISON_CONTENT_KIND,
    ))
}

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
                proposition.temporal_scope.clone(),
                proposition.condition_refs.clone(),
            ))
            .or_insert_with(Vec::new)
            .push(fact);
    }
    grouped
}

fn relation_comparison_meaning_plan(
    subject_a: &str,
    subject_b: &str,
    scope: &str,
    format: Option<MeaningCompositionFormat>,
    inventory_a: &relations::RelationInventory,
    inventory_b: &relations::RelationInventory,
) -> RelationComparisonMeaningPlan {
    let left = relation_comparison_map(inventory_a);
    let right = relation_comparison_map(inventory_b);
    let keys = left.keys().chain(right.keys()).collect::<BTreeSet<_>>();
    let rows = keys
        .into_iter()
        .map(|key| {
            let left = left.get(key).map(Vec::as_slice).unwrap_or_default();
            let right = right.get(key).map(Vec::as_slice).unwrap_or_default();
            let contested = left
                .iter()
                .any(|fact| inventory_a.contested_fact_ids.contains(&fact.id))
                || right
                    .iter()
                    .any(|fact| inventory_b.contested_fact_ids.contains(&fact.id));
            let class = match (contested, left.is_empty(), right.is_empty()) {
                (true, _, _) => RelationComparisonClass::Contested,
                (false, false, false) => RelationComparisonClass::Common,
                (false, false, true) => RelationComparisonClass::LeftOnly,
                (false, true, false) => RelationComparisonClass::RightOnly,
                _ => unreachable!("comparison keys originate in either inventory"),
            };
            RelationComparisonMeaningRow {
                class,
                left_fact_ids: left.iter().map(|fact| fact.id.clone()).collect(),
                right_fact_ids: right.iter().map(|fact| fact.id.clone()).collect(),
            }
        })
        .collect();
    RelationComparisonMeaningPlan {
        subject_a: subject_a.into(),
        subject_b: subject_b.into(),
        scope: scope.into(),
        format,
        rows,
        proof_ids: relation_comparison_proof_ids(inventory_a, inventory_b)
            .into_iter()
            .collect(),
    }
}

fn comparison_generation_request(
    plan: &RelationComparisonMeaningPlan,
    inventory_a: &relations::RelationInventory,
    inventory_b: &relations::RelationInventory,
    max_output_bytes: usize,
) -> Result<language::MeaningGenerationRequest> {
    use language::{
        MeaningCompositionFormat, MeaningCompositionPurpose, MeaningCompositionStyle,
        MeaningGrammarFragment::Literal, MeaningGrammarUnit, MeaningRole,
    };

    let literal = |value: &str| Literal {
        value: value.into(),
    };
    let mut units = vec![MeaningGrammarUnit {
        id: "opening".into(),
        role: MeaningRole::Opening,
        semantic_sha256: identity(&("relation-comparison-v5", plan))?,
        evidence_ids: Vec::new(),
        dependencies: Vec::new(),
        fragments: vec![literal(RELATION_COMPARISON_GENERATED_HEADING)],
    }];
    for (id, role, subject, inventory) in [
        ("left", MeaningRole::SubjectA, &plan.subject_a, inventory_a),
        ("right", MeaningRole::SubjectB, &plan.subject_b, inventory_b),
    ] {
        let mut fragments = vec![
            grammar_choice(
                "operand",
                if id == "left" {
                    &["左の対象は", "左側の対象は"]
                } else {
                    &["右の対象は", "右側の対象は"]
                },
            ),
            literal("「"),
            literal(subject),
            literal("」です。範囲「"),
            literal(&plan.scope),
            literal("」"),
        ];
        fragments.push(literal(if inventory.facts.is_empty() {
            "に比較できる保存根拠はありません。"
        } else {
            "の有効な保存根拠を参照します。"
        }));
        units.push(MeaningGrammarUnit {
            id: id.into(),
            role,
            semantic_sha256: identity(&(
                "relation-comparison-operand-v1",
                subject,
                &plan.scope,
                inventory,
            ))?,
            evidence_ids: Vec::new(),
            dependencies: vec!["opening".into()],
            fragments,
        });
    }
    let left = inventory_a
        .facts
        .iter()
        .map(|fact| (fact.id.as_str(), fact))
        .collect::<BTreeMap<_, _>>();
    let right = inventory_b
        .facts
        .iter()
        .map(|fact| (fact.id.as_str(), fact))
        .collect::<BTreeMap<_, _>>();
    let mut represented = BTreeSet::new();
    for (index, row) in plan.rows.iter().enumerate() {
        let (role, labels) = match row.class {
            RelationComparisonClass::Common => (
                MeaningRole::Commonality,
                &["両側の支持：", "共通する支持："][..],
            ),
            RelationComparisonClass::LeftOnly => (
                MeaningRole::Difference,
                &["左側のみで確認した支持：", "左の記録でのみ確認した支持："][..],
            ),
            RelationComparisonClass::RightOnly => (
                MeaningRole::Difference,
                &["右側のみで確認した支持：", "右の記録でのみ確認した支持："][..],
            ),
            RelationComparisonClass::Contested => (
                MeaningRole::Difference,
                &["競合根拠を含む関係：", "対立根拠がある関係："][..],
            ),
        };
        let mut fragments = vec![grammar_choice("comparison_class", labels)];
        let mut facts = Vec::new();
        for (ids, inventory, slot) in [
            (&row.left_fact_ids, &left, "left"),
            (&row.right_fact_ids, &right, "right"),
        ] {
            let side = ids
                .iter()
                .map(|id| {
                    inventory.get(id.as_str()).copied().ok_or_else(|| {
                        failure(
                            "CONTENT_PROOF_MAPPING",
                            "the comparison row references an absent operand fact",
                        )
                    })
                })
                .collect::<Result<Vec<_>>>()?;
            if let Some(fact) = side.first() {
                if !facts.is_empty() {
                    fragments.push(literal("／"));
                }
                append_proposition_surface(&mut fragments, &fact.proposition, slot);
                fragments.push(literal("（"));
                let origins = side
                    .iter()
                    .map(|fact| guarantee_label(&fact.guarantee))
                    .collect::<BTreeSet<_>>();
                fragments.push(literal(&origins.into_iter().collect::<Vec<_>>().join("・")));
                fragments.push(literal("）"));
            }
            facts.extend(side);
        }
        if let Some(fact) = facts.first()
            && !fact.proposition.condition_refs.is_empty()
        {
            fragments.push(literal("（適用条件"));
            fragments.push(literal(
                &fact
                    .proposition
                    .condition_refs
                    .iter()
                    .map(|id| condition_reference_label(id))
                    .collect::<Vec<_>>()
                    .join("、"),
            ));
            fragments.push(literal("）"));
        }
        fragments.push(literal(
            if row.class == RelationComparisonClass::Contested {
                "。競合は解消していません。"
            } else {
                "。"
            },
        ));
        let evidence_ids = facts
            .iter()
            .flat_map(|fact| std::iter::once(&fact.id).chain(&fact.dependencies))
            .cloned()
            .collect::<BTreeSet<_>>()
            .into_iter()
            .take(16)
            .collect::<Vec<_>>();
        represented.extend(evidence_ids.iter().cloned());
        units.push(MeaningGrammarUnit {
            id: format!("relation_{index:02}"),
            role,
            semantic_sha256: identity(&("relation-comparison-row-v1", &plan.scope, row, &facts))?,
            evidence_ids,
            dependencies: vec!["left".into(), "right".into()],
            fragments,
        });
    }
    let other_proofs = plan
        .proof_ids
        .iter()
        .filter(|id| !represented.contains(*id))
        .cloned()
        .collect::<Vec<_>>();
    for (index, proof_ids) in other_proofs.chunks(16).enumerate() {
        units.push(MeaningGrammarUnit {
            id: format!("proof_{index:02}"),
            role: MeaningRole::Evidence,
            semantic_sha256: identity(&("relation-comparison-additional-proof-v1", proof_ids))?,
            evidence_ids: proof_ids.to_vec(),
            dependencies: vec!["left".into(), "right".into()],
            fragments: vec![
                grammar_choice(
                    "proof",
                    &["照合に用いた追加の根拠参照は", "比較を支える別の根拠参照は"],
                ),
                literal(&proof_ids.len().to_string()),
                literal("件です。"),
            ],
        });
    }
    units.push(MeaningGrammarUnit {
        id: "limitation".into(),
        role: MeaningRole::Limitation,
        semantic_sha256: identity(&("relation-comparison-coverage-limit-v1", &plan.scope))?,
        evidence_ids: Vec::new(),
        dependencies: units.iter().map(|unit| unit.id.clone()).collect(),
        fragments: vec![
            grammar_choice(
                "missing_support",
                &[
                    "片側で支持が見つからなくても",
                    "一方で支持を確認できなくても",
                ],
            ),
            literal("関係が存在しないとは言えません。"),
            literal("保存記録の網羅性と現実世界の真偽は確認していません。"),
        ],
    });
    let max_output_bytes = max_output_bytes.min(language::MAX_RELATION_GENERATION_BYTES);
    Ok(language::MeaningGenerationRequest {
        purpose: MeaningCompositionPurpose::Compare,
        format: plan.format.unwrap_or(MeaningCompositionFormat::Bullets),
        style: MeaningCompositionStyle::Neutral,
        units,
        max_output_bytes,
        max_tokens: max_output_bytes,
    })
}

fn relation_comparison_generated_kind(format: Option<MeaningCompositionFormat>) -> &'static str {
    if format.is_some() {
        RELATION_COMPARISON_CONTENT_KIND_V6
    } else {
        RELATION_COMPARISON_CONTENT_KIND_V5
    }
}

fn validate_comparison_generation(
    plan: &RelationComparisonMeaningPlan,
    request: &language::MeaningGenerationRequest,
    report: &language::MeaningGenerationReport,
) -> Result<()> {
    validate_meaning_generation(request, &plan.proof_ids, report)?;
    let Some(format) = plan.format else {
        return Ok(());
    };
    let line_breaks = ['\r', '\n', '\u{0085}', '\u{2028}', '\u{2029}'];
    let layout_matches = match format {
        MeaningCompositionFormat::Paragraph => {
            !report.text.contains(line_breaks)
                && report
                    .units
                    .first()
                    .is_some_and(|unit| unit.text == RELATION_COMPARISON_GENERATED_HEADING)
        }
        MeaningCompositionFormat::Bullets => {
            report.units.iter().enumerate().all(|(index, unit)| {
                let prefix = if index == 0 { "・" } else { "\n・" };
                unit.text.strip_prefix(prefix).is_some_and(|text| {
                    if index == 0 {
                        text == RELATION_COMPARISON_GENERATED_HEADING
                    } else {
                        !text.contains(line_breaks)
                    }
                })
            })
        }
    };
    if request.format != format || !layout_matches {
        return Err(failure(
            "COMPARISON_FORMAT_MISMATCH",
            "the generated comparison does not have the sealed layout",
        ));
    }
    Ok(())
}

fn relation_comparison_content_request_v2(
    subject_a: &str,
    subject_b: &str,
    scope: &str,
    inventory_a: &relations::RelationInventory,
    inventory_b: &relations::RelationInventory,
    max_output_bytes: usize,
) -> Result<Option<language::MeaningCompositionRequest>> {
    relation_comparison_content_request_base(
        subject_a,
        subject_b,
        scope,
        inventory_a,
        inventory_b,
        max_output_bytes,
        false,
    )
}

fn relation_comparison_content_request_base(
    subject_a: &str,
    subject_b: &str,
    scope: &str,
    inventory_a: &relations::RelationInventory,
    inventory_b: &relations::RelationInventory,
    max_output_bytes: usize,
    compact_dependency_summary: bool,
) -> Result<Option<language::MeaningCompositionRequest>> {
    use language::{
        MeaningCompositionFormat, MeaningCompositionPurpose, MeaningCompositionStyle, MeaningRole,
        TypedMeaningUnit,
    };

    let max_output_bytes = max_output_bytes.min(8192);
    if max_output_bytes == 0 {
        return Ok(None);
    }
    let unit_id = |kind: &str, source: &str| -> Result<String> {
        let fingerprint = identity(&("core-relation-comparison-content-unit-v1", kind, source))?;
        Ok(format!("{kind}_{}", &fingerprint[..40]))
    };
    let mut units = vec![TypedMeaningUnit {
        id: unit_id("opening", "relation-comparison")?,
        role: MeaningRole::Opening,
        text: RELATION_COMPARISON_CONTENT_HEADER.into(),
        evidence_ids: Vec::new(),
        dependencies: Vec::new(),
    }];
    units.extend([
        TypedMeaningUnit {
            id: unit_id("subject_a", subject_a)?,
            role: MeaningRole::SubjectA,
            text: format!("比較対象Aは「{subject_a}」です（範囲: {scope}）。"),
            evidence_ids: Vec::new(),
            dependencies: Vec::new(),
        },
        TypedMeaningUnit {
            id: unit_id("subject_b", subject_b)?,
            role: MeaningRole::SubjectB,
            text: format!("比較対象Bは「{subject_b}」です（範囲: {scope}）。"),
            evidence_ids: Vec::new(),
            dependencies: Vec::new(),
        },
    ]);

    let left = relation_comparison_map(inventory_a);
    let right = relation_comparison_map(inventory_b);
    let mut keys: BTreeSet<_> = left.keys().cloned().collect();
    keys.extend(right.keys().cloned());
    for key in keys {
        let key_identity = relation_comparison_key_identity(&key)?;
        let facts_a = left.get(&key).map(Vec::as_slice).unwrap_or_default();
        let facts_b = right.get(&key).map(Vec::as_slice).unwrap_or_default();
        let contested_a = facts_a
            .iter()
            .any(|fact| inventory_a.contested_fact_ids.contains(&fact.id));
        let contested_b = facts_b
            .iter()
            .any(|fact| inventory_b.contested_fact_ids.contains(&fact.id));
        let (role, text, source) = if contested_a || contested_b {
            let example = facts_a.first().or_else(|| facts_b.first()).unwrap();
            (
                MeaningRole::Difference,
                format!(
                    "競合根拠を含む関係（共通・片側のみと確定しません）: {}。",
                    describe(&example.proposition)
                ),
                format!("contested:{key_identity}"),
            )
        } else if !facts_a.is_empty() && !facts_b.is_empty() {
            (
                MeaningRole::Commonality,
                format!(
                    "両側で確認した同じ関係: {} ／ {}。",
                    describe(&facts_a[0].proposition),
                    describe(&facts_b[0].proposition)
                ),
                format!("common:{key_identity}"),
            )
        } else if !facts_a.is_empty() {
            (
                MeaningRole::Difference,
                format!(
                    "{subject_a}側で確認した関係（{subject_b}側では同じ関係の支持を確認できず）: {}。",
                    describe(&facts_a[0].proposition)
                ),
                format!("side-a:{key_identity}"),
            )
        } else {
            (
                MeaningRole::Difference,
                format!(
                    "{subject_b}側で確認した関係（{subject_a}側では同じ関係の支持を確認できず）: {}。",
                    describe(&facts_b[0].proposition)
                ),
                format!("side-b:{key_identity}"),
            )
        };
        let mut evidence_ids = facts_a
            .first()
            .into_iter()
            .chain(facts_b.first())
            .map(|fact| fact.id.clone())
            .collect::<Vec<_>>();
        evidence_ids.sort();
        evidence_ids.dedup();
        units.push(TypedMeaningUnit {
            id: unit_id("relation", &source)?,
            role,
            text,
            evidence_ids,
            dependencies: Vec::new(),
        });
    }

    let mut facts = inventory_a
        .facts
        .iter()
        .map(|fact| (subject_a, inventory_a, fact))
        .chain(
            inventory_b
                .facts
                .iter()
                .map(|fact| (subject_b, inventory_b, fact)),
        )
        .collect::<Vec<_>>();
    facts.sort_by(|left, right| (left.0, &left.2.id).cmp(&(right.0, &right.2.id)));
    let mut fact_proof_ids = BTreeSet::new();
    for (side, inventory, fact) in facts {
        fact_proof_ids.insert(fact.id.clone());
        fact_proof_ids.extend(fact.dependencies.iter().cloned());
        let mut evidence_ids = std::iter::once(fact.id.clone())
            .chain(fact.dependencies.iter().cloned())
            .collect::<Vec<_>>();
        evidence_ids.sort();
        evidence_ids.dedup();
        let mut dependencies = fact.dependencies.clone();
        dependencies.sort();
        dependencies.dedup();
        let dependency_text = if dependencies.is_empty() {
            "なし".to_owned()
        } else {
            dependencies.join(", ")
        };
        let contested = if inventory.contested_fact_ids.contains(&fact.id) {
            "; 対立根拠あり"
        } else {
            ""
        };
        let evidence_text = if compact_dependency_summary {
            format!(
                "根拠({side}): {}（{}; 記録ID: {}; 依存根拠: {}件{contested}）。",
                describe(&fact.proposition),
                guarantee_label(&fact.guarantee),
                fact.id,
                dependencies.len()
            )
        } else {
            format!(
                "根拠({side}): {}（{}; 記録ID: {}; 依存ID: {dependency_text}{contested}）。",
                describe(&fact.proposition),
                guarantee_label(&fact.guarantee),
                fact.id
            )
        };
        units.push(TypedMeaningUnit {
            id: unit_id("evidence", &format!("{side}:{}", fact.id))?,
            role: MeaningRole::Evidence,
            text: evidence_text,
            evidence_ids,
            dependencies: Vec::new(),
        });
    }

    let proof_ids = relation_comparison_proof_ids(inventory_a, inventory_b);
    for reference in proof_ids.difference(&fact_proof_ids) {
        units.push(TypedMeaningUnit {
            id: unit_id("proofref", reference)?,
            role: MeaningRole::Evidence,
            text: format!("再照合に用いた依存根拠参照IDは「{reference}」です。"),
            evidence_ids: vec![reference.clone()],
            dependencies: Vec::new(),
        });
    }

    let mut limitation_dependencies = units.iter().map(|unit| unit.id.clone()).collect::<Vec<_>>();
    limitation_dependencies.sort();
    units.push(TypedMeaningUnit {
        id: unit_id("limitation", &identity(&(subject_a, subject_b, scope, &proof_ids))?)?,
        role: MeaningRole::Limitation,
        text: format!(
            "この比較は範囲「{scope}」の保存記録内の支持を分類し、片側の無支持は関係の不存在を証明せず、保存記録の網羅性と現実世界の真偽も確認していません。"
        ),
        evidence_ids: Vec::new(),
        dependencies: limitation_dependencies,
    });

    let represented = units
        .iter()
        .flat_map(|unit| unit.evidence_ids.iter().cloned())
        .collect::<BTreeSet<_>>();
    if represented != proof_ids
        || units.len() > 16
        || units
            .iter()
            .any(|unit| !valid_comparison_unit_text(&unit.text) || unit.evidence_ids.len() > 16)
        || units.iter().map(|unit| unit.text.len()).sum::<usize>() > 8192
    {
        return Ok(None);
    }
    Ok(Some(language::MeaningCompositionRequest {
        purpose: MeaningCompositionPurpose::Compare,
        format: MeaningCompositionFormat::Paragraph,
        style: MeaningCompositionStyle::Neutral,
        units,
        prior_sequence: Vec::new(),
        max_output_bytes,
        max_tokens: max_output_bytes,
    }))
}

struct RelationComparisonContentOptions {
    max_output_bytes: usize,
    compact_dependency_summary: bool,
    rule_unit_namespace: &'static str,
}

fn relation_comparison_content_request_v3(
    subject_a: &str,
    subject_b: &str,
    scope: &str,
    inventory_a: &relations::RelationInventory,
    inventory_b: &relations::RelationInventory,
    rules: &[RelationRule],
    max_output_bytes: usize,
) -> Result<Option<language::MeaningCompositionRequest>> {
    relation_comparison_content_request_with_rules(
        subject_a,
        subject_b,
        scope,
        inventory_a,
        inventory_b,
        rules,
        RelationComparisonContentOptions {
            max_output_bytes,
            compact_dependency_summary: false,
            rule_unit_namespace: "core-relation-comparison-content-unit-v3",
        },
    )
}

fn relation_comparison_content_request_v4(
    subject_a: &str,
    subject_b: &str,
    scope: &str,
    inventory_a: &relations::RelationInventory,
    inventory_b: &relations::RelationInventory,
    rules: &[RelationRule],
    max_output_bytes: usize,
) -> Result<Option<language::MeaningCompositionRequest>> {
    relation_comparison_content_request_with_rules(
        subject_a,
        subject_b,
        scope,
        inventory_a,
        inventory_b,
        rules,
        RelationComparisonContentOptions {
            max_output_bytes,
            compact_dependency_summary: true,
            rule_unit_namespace: "core-relation-comparison-content-unit-v4",
        },
    )
}

fn relation_comparison_content_request_with_rules(
    subject_a: &str,
    subject_b: &str,
    scope: &str,
    inventory_a: &relations::RelationInventory,
    inventory_b: &relations::RelationInventory,
    rules: &[RelationRule],
    options: RelationComparisonContentOptions,
) -> Result<Option<language::MeaningCompositionRequest>> {
    use language::{MeaningRole, TypedMeaningUnit};

    let Some(mut request) = relation_comparison_content_request_base(
        subject_a,
        subject_b,
        scope,
        inventory_a,
        inventory_b,
        options.max_output_bytes,
        options.compact_dependency_summary,
    )?
    else {
        return Ok(None);
    };
    if rules.is_empty() {
        return Ok(None);
    }

    let proof_ids = relation_comparison_proof_ids(inventory_a, inventory_b);
    let mut used_rules = rules
        .iter()
        .filter(|rule| rule.valid && proof_ids.contains(&rule.id))
        .collect::<Vec<_>>();
    used_rules.sort_by(|left, right| left.id.cmp(&right.id));
    used_rules.dedup_by(|left, right| left.id == right.id);
    if used_rules.is_empty() {
        return Ok(None);
    }
    let mut rule_unit_ids = BTreeMap::new();
    for rule in &used_rules {
        let fingerprint = identity(&(options.rule_unit_namespace, "rule", &rule.id))?;
        rule_unit_ids.insert(rule.id.clone(), format!("rule_{}", &fingerprint[..40]));
    }

    let limitation_index = request
        .units
        .iter()
        .position(|unit| unit.role == MeaningRole::Limitation)
        .ok_or_else(|| {
            failure(
                "CONTENT_PLAN_INVALID",
                "comparison v2 request has no limitation unit",
            )
        })?;
    let mut limitation = request.units.remove(limitation_index);
    for rule in used_rules {
        let premises = std::iter::once(&rule.premise)
            .chain(rule.additional_premises.iter())
            .map(describe)
            .collect::<Vec<_>>()
            .join(" かつ ");
        let mut evidence_ids = vec![rule.id.clone()];
        evidence_ids.extend(rule.dependencies.iter().cloned());
        evidence_ids.sort();
        evidence_ids.dedup();
        let mut dependencies = rule
            .dependencies
            .iter()
            .filter_map(|id| rule_unit_ids.get(id).cloned())
            .collect::<Vec<_>>();
        dependencies.sort();
        dependencies.dedup();
        request.units.push(TypedMeaningUnit {
            id: rule_unit_ids[&rule.id].clone(),
            role: MeaningRole::Evidence,
            text: format!(
                "比較に用いた保存規則「{}」ならば「{}」です（範囲: {}; 規則ID: {}）。",
                premises,
                describe(&rule.conclusion),
                rule.conclusion.scope,
                rule.id
            ),
            evidence_ids,
            dependencies,
        });
    }

    if request.units.len() >= 16 {
        return Ok(None);
    }
    let mut limitation_dependencies = request
        .units
        .iter()
        .map(|unit| unit.id.clone())
        .collect::<Vec<_>>();
    limitation_dependencies.sort();
    limitation_dependencies.dedup();
    limitation.dependencies = limitation_dependencies;
    request.units.push(limitation);

    let represented = request
        .units
        .iter()
        .flat_map(|unit| unit.evidence_ids.iter().cloned())
        .collect::<BTreeSet<_>>();
    if represented != proof_ids
        || request.units.len() > 16
        || request
            .units
            .iter()
            .any(|unit| !valid_comparison_unit_text(&unit.text) || unit.evidence_ids.len() > 16)
        || request
            .units
            .iter()
            .map(|unit| unit.text.len())
            .sum::<usize>()
            > 8192
    {
        return Ok(None);
    }
    Ok(Some(request))
}

fn valid_comparison_unit_text(text: &str) -> bool {
    text.len() <= 384
        && !text.is_empty()
        && text.trim() == text
        && !text.chars().any(char::is_control)
        && text
            .chars()
            .filter(|ch| matches!(ch, '。' | '！' | '？' | '.' | '!' | '?'))
            .count()
            == 1
        && text
            .chars()
            .last()
            .is_some_and(|ch| matches!(ch, '。' | '！' | '？' | '.' | '!' | '?'))
}

fn composed_content_units(
    request: &language::MeaningCompositionRequest,
    composition: &language::MeaningCompositionReport,
) -> Result<Vec<String>> {
    composition
        .required_unit_ids
        .iter()
        .map(|id| {
            request
                .units
                .iter()
                .find(|unit| &unit.id == id)
                .map(|unit| unit.text.clone())
                .ok_or_else(|| {
                    failure(
                        "CONTENT_PLAN_INVALID",
                        "meaning composition references an unknown Core unit",
                    )
                })
        })
        .collect()
}

fn decode_relation_comparison_artifact(bytes: &[u8]) -> Result<RelationComparisonArtifactView> {
    let value: serde_json::Value = serde_json::from_slice(bytes)?;
    let kind = value
        .get("kind")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| {
            failure(
                "RELATION_COMPARISON_ARTIFACT_INVALID",
                "missing artifact kind",
            )
        })?;
    if kind == RELATION_COMPARISON_REPORT_KIND {
        let artifact: RelationComparisonReportArtifact = serde_json::from_value(value)?;
        return Ok(RelationComparisonArtifactView {
            canonical_bytes: canonical(&artifact)?,
            kind: artifact.kind,
            result_id: artifact.result_id,
            report_sha256: artifact.report_sha256,
            subject_a: artifact.subject_a,
            subject_b: artifact.subject_b,
            scope: artifact.scope,
            inventory_a: artifact.inventory_a,
            inventory_b: artifact.inventory_b,
            content: None,
            generated: None,
        });
    }
    if matches!(
        kind,
        RELATION_COMPARISON_CONTENT_KIND
            | RELATION_COMPARISON_CONTENT_KIND_V3
            | RELATION_COMPARISON_CONTENT_KIND_V4
    ) {
        let artifact: RelationComparisonContentArtifact = serde_json::from_value(value)?;
        return Ok(RelationComparisonArtifactView {
            canonical_bytes: canonical(&artifact)?,
            kind: artifact.kind,
            result_id: artifact.result_id,
            report_sha256: artifact.report_sha256,
            subject_a: artifact.subject_a,
            subject_b: artifact.subject_b,
            scope: artifact.scope,
            inventory_a: artifact.inventory_a,
            inventory_b: artifact.inventory_b,
            content: Some((artifact.content_request, artifact.content_composition)),
            generated: None,
        });
    }
    if matches!(
        kind,
        RELATION_COMPARISON_CONTENT_KIND_V5 | RELATION_COMPARISON_CONTENT_KIND_V6
    ) {
        let artifact: RelationComparisonGeneratedArtifact = serde_json::from_value(value)?;
        return Ok(RelationComparisonArtifactView {
            canonical_bytes: canonical(&artifact)?,
            kind: artifact.kind,
            result_id: artifact.result_id,
            report_sha256: artifact.report_sha256,
            subject_a: artifact.subject_a,
            subject_b: artifact.subject_b,
            scope: artifact.scope,
            inventory_a: artifact.inventory_a,
            inventory_b: artifact.inventory_b,
            content: None,
            generated: Some((artifact.meaning_plan, artifact.content_generation)),
        });
    }
    Err(failure(
        "RELATION_COMPARISON_ARTIFACT_INVALID",
        "unsupported comparison artifact kind",
    ))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct RelationComparisonReplay<'a> {
    pub subject_a: &'a str,
    pub subject_b: &'a str,
    pub scope: &'a str,
    pub locale: &'a str,
    pub format: Option<MeaningCompositionFormat>,
    pub max_actions: u32,
    pub max_evidence_bytes: usize,
    pub generated: bool,
}

pub(crate) fn verify_relation_comparison_memory_result(
    memory: &MemoryState,
    artifact_bytes: &[u8],
    result: &ResultRecord,
    plan: &ContentPlan,
    replay: RelationComparisonReplay<'_>,
) -> Result<()> {
    let per_subject_actions = replay.max_actions / 2;
    if per_subject_actions == 0 {
        return Err(failure(
            "RECOVERY_MEMORY_PROOF_MISMATCH",
            "the retained comparison has no bounded replay budget",
        ));
    }
    let inventory_a =
        relation_comparison_inventory(memory, replay.subject_a, replay.scope, per_subject_actions)?;
    let inventory_b =
        relation_comparison_inventory(memory, replay.subject_b, replay.scope, per_subject_actions)?;
    verify_relation_comparison_inventories(
        &inventory_a,
        &inventory_b,
        &memory.rules,
        artifact_bytes,
        result,
        plan,
        replay,
    )
}

/// Revalidates pending comparison bytes against the current, revision-matched
/// scope index without materializing the actor's complete retained history.
/// `None` means the derived index is cold or stale and the caller must use the
/// authenticated snapshot fallback.
pub(crate) fn verify_relation_comparison_memory_result_with_index(
    store: &Store,
    actor: &str,
    artifact_bytes: &[u8],
    result: &ResultRecord,
    plan: &ContentPlan,
    replay: RelationComparisonReplay<'_>,
) -> Result<Option<()>> {
    let Some(verified) = store.with_relation_index(actor, |index| {
        let per_subject_actions = replay.max_actions / 2;
        if per_subject_actions == 0 {
            return Err(failure(
                "RECOVERY_MEMORY_PROOF_MISMATCH",
                "the retained comparison has no bounded replay budget",
            ));
        }
        let (inventory_a, rules_a) = relation_comparison_inventory_from_index(
            index,
            replay.subject_a,
            replay.scope,
            per_subject_actions,
        )?;
        let (inventory_b, rules_b) = relation_comparison_inventory_from_index(
            index,
            replay.subject_b,
            replay.scope,
            per_subject_actions,
        )?;
        let mut rules = BTreeMap::new();
        for rule in rules_a.into_iter().chain(rules_b) {
            rules.insert(rule.id.clone(), rule);
        }
        verify_relation_comparison_inventories(
            &inventory_a,
            &inventory_b,
            &rules.into_values().collect::<Vec<_>>(),
            artifact_bytes,
            result,
            plan,
            replay,
        )?;
        Ok(())
    })?
    else {
        return Ok(None);
    };
    Ok(Some(verified))
}

fn verify_relation_comparison_inventories(
    inventory_a: &relations::RelationInventory,
    inventory_b: &relations::RelationInventory,
    rules: &[RelationRule],
    artifact_bytes: &[u8],
    result: &ResultRecord,
    plan: &ContentPlan,
    replay: RelationComparisonReplay<'_>,
) -> Result<()> {
    let report_ref = result.dependencies.first().ok_or_else(|| {
        failure(
            "RECOVERY_MEMORY_PROOF_MISMATCH",
            "the retained comparison has no report artifact reference",
        )
    })?;
    let artifact = decode_relation_comparison_artifact(artifact_bytes).map_err(|error| {
        failure(
            "RECOVERY_MEMORY_PROOF_MISMATCH",
            format!("the retained comparison artifact cannot be decoded: {error}"),
        )
    })?;
    if artifact.canonical_bytes != artifact_bytes
        || digest(artifact_bytes) != *report_ref
        || artifact.result_id != result.id
        || artifact.subject_a != replay.subject_a
        || artifact.subject_b != replay.subject_b
        || artifact.scope != replay.scope
    {
        return Err(failure(
            "RECOVERY_MEMORY_PROOF_MISMATCH",
            "the retained comparison artifact identity differs from the adopted request",
        ));
    }
    if inventory_a.facts.is_empty() && inventory_b.facts.is_empty() {
        return Err(failure(
            "RECOVERY_MEMORY_PROOF_MISMATCH",
            "the retained comparison no longer has a supporting saved relation",
        ));
    }
    if &artifact.inventory_a != inventory_a || &artifact.inventory_b != inventory_b {
        return Err(failure(
            "RECOVERY_MEMORY_PROOF_MISMATCH",
            "the retained comparison inventories differ from the actor-memory revision",
        ));
    }
    if replay.generated != artifact.generated.is_some() {
        return Err(failure(
            "RECOVERY_MEMORY_PROOF_MISMATCH",
            "the comparison artifact does not satisfy the sealed generation contract",
        ));
    }
    let (report, expected_units) = if replay.generated {
        verify_generated_comparison(&artifact, inventory_a, inventory_b, result, plan, replay)?
    } else {
        let (content_request, content_kind) = relation_comparison_content_request(
            rules,
            replay.subject_a,
            replay.subject_b,
            replay.scope,
            inventory_a,
            inventory_b,
            replay.max_evidence_bytes,
        )?;
        if let Some(content_request) = content_request {
            if artifact.kind != content_kind {
                return Err(failure(
                    "RECOVERY_MEMORY_PROOF_MISMATCH",
                    "the retained comparison content version differs from Core's selected version",
                ));
            }
            let composition = language::compose_typed_meaning(&content_request)?;
            let expected_proofs = relation_comparison_proof_ids(inventory_a, inventory_b);
            let composed_proofs = composition
                .evidence_ids
                .iter()
                .cloned()
                .collect::<BTreeSet<_>>();
            if composed_proofs != expected_proofs
                || artifact.content.as_ref()
                    != Some(&(content_request.clone(), composition.clone()))
            {
                return Err(failure(
                    "RECOVERY_MEMORY_PROOF_MISMATCH",
                    "the retained comparison composition differs from its exact Core proof request",
                ));
            }
            let units = composed_content_units(&content_request, &composition)?;
            (composition.text, units)
        } else {
            if artifact.kind != RELATION_COMPARISON_REPORT_KIND || artifact.content.is_some() {
                return Err(failure(
                    "RECOVERY_MEMORY_PROOF_MISMATCH",
                    "the retained comparison report artifact has an unexpected content variant",
                ));
            }
            let report = relation_comparison_report(
                replay.subject_a,
                replay.subject_b,
                replay.scope,
                inventory_a,
                inventory_b,
            );
            (report.clone(), vec![report])
        }
    };
    let mut expected_dependencies = vec![report_ref.clone()];
    if let Some((plan, _)) = &artifact.generated {
        expected_dependencies.extend(plan.proof_ids.iter().cloned());
    } else {
        expected_dependencies.extend(
            inventory_a
                .dependencies
                .iter()
                .chain(&inventory_b.dependencies)
                .cloned()
                .collect::<BTreeSet<_>>(),
        );
    }
    if artifact.report_sha256 != digest(report.as_bytes())
        || report.len().saturating_add(artifact_bytes.len()) > replay.max_evidence_bytes
        || result.text != report
        || plan.text != report
        || result.dependencies != expected_dependencies
        || result.guarantee != Guarantee::Hypothesis
        || plan.guarantee != Guarantee::Hypothesis
        || plan.required_units != expected_units
        || plan.dependencies.get(..result.dependencies.len())
            != Some(result.dependencies.as_slice())
    {
        return Err(failure(
            "RECOVERY_MEMORY_PROOF_MISMATCH",
            "the retained comparison output, plan or proof lineage differs from its actor-memory revision",
        ));
    }
    Ok(())
}

fn verify_generated_comparison(
    artifact: &RelationComparisonArtifactView,
    inventory_a: &relations::RelationInventory,
    inventory_b: &relations::RelationInventory,
    result: &ResultRecord,
    plan: &ContentPlan,
    replay: RelationComparisonReplay<'_>,
) -> Result<(String, Vec<String>)> {
    let expected_plan = relation_comparison_meaning_plan(
        replay.subject_a,
        replay.subject_b,
        replay.scope,
        replay.format,
        inventory_a,
        inventory_b,
    );
    let Some((meaning_plan, report)) = &artifact.generated else {
        return Err(failure(
            "CONTENT_PLAN_INVALID",
            "the generated comparison artifact is missing",
        ));
    };
    if result.actor_memory_revision.is_none()
        || meaning_plan != &expected_plan
        || artifact.kind != relation_comparison_generated_kind(replay.format)
        || !supports_japanese_output_locale(replay.locale)
    {
        return Err(failure(
            "CONTENT_PLAN_INVALID",
            "the retained comparison requires its pinned memory, verified operands and sealed format",
        ));
    }
    let grammar = comparison_generation_request(
        &expected_plan,
        inventory_a,
        inventory_b,
        replay.max_evidence_bytes,
    )?;
    let mut generation = language::generate_semantic_meaning(&grammar)?;
    generation.report.model_artifact_ref = Some(digest(&canonical(&generation.model)?));
    validate_comparison_generation(&expected_plan, &grammar, &generation.report)?;
    if generation.report != *report {
        return Err(failure(
            "CONTENT_PLAN_INVALID",
            "the retained comparison generation does not replay exactly",
        ));
    }
    let compute_ref = plan.dependencies.last().ok_or_else(|| {
        failure(
            "CONTENT_ADOPTION_MISMATCH",
            "the comparison content has no compute artifact",
        )
    })?;
    validate_generated_content_plan_adoption(
        result,
        report,
        &generation.emission,
        &meaning_plan.proof_ids,
        compute_ref,
        replay.locale,
        plan,
    )?;
    Ok((
        report.text.clone(),
        report.units.iter().map(|unit| unit.text.clone()).collect(),
    ))
}

pub(crate) fn verify_generated_model_artifacts(
    store: &Store,
    actor: &str,
    case: &str,
    plan: &ContentPlan,
) -> Result<()> {
    // The proof replay checks the expected canonical digests; require both
    // actual artifacts to be intact and bound to this case.
    store.get_bytes(actor, case, &plan.model_artifact_ref)?;
    store.get_bytes(actor, case, &plan.model_observation_ref)?;
    Ok(())
}

pub(crate) fn relation_comparison_explanation_units(
    previous: &ResultRecord,
    prior_plan: &ContentPlan,
) -> Vec<String> {
    let mut explanation = vec![format!(
        "前回の保存関係比較を詳しく説明します（比較結果ID: {}）。同じactorの現在有効な記憶から左右のscope別inventoryを再計算し、前回と一致することを確認しました。",
        previous.id
    )];
    if is_generated_comparison_text(&previous.text) {
        explanation.extend(
            prior_plan
                .required_units
                .iter()
                .skip(1)
                .map(|unit| unit.strip_prefix('\n').unwrap_or(unit).to_owned()),
        );
        explanation.extend(
            previous
                .dependencies
                .iter()
                .skip(1)
                .map(|proof_id| format!("照合した根拠ID: {proof_id}")),
        );
    } else {
        explanation.extend(previous.text.lines().skip(1).map(str::to_string));
    }
    explanation.push(
        "比較分類は保存済みの前提・規則の範囲に限られます。片側で支持を確認できないことは、関係が偽または存在しない証明ではありません。記録の網羅性と現実世界の真偽は確認していません。".into(),
    );
    explanation
}

fn is_generated_comparison_text(text: &str) -> bool {
    text.strip_prefix('・')
        .unwrap_or(text)
        .starts_with(RELATION_COMPARISON_GENERATED_HEADING)
}

pub(crate) fn is_saved_relation_comparison_result(result: &ResultRecord) -> bool {
    result.text.starts_with(RELATION_COMPARISON_REPORT_HEADER)
        || result.text.starts_with(RELATION_COMPARISON_CONTENT_HEADER)
        || is_generated_comparison_text(&result.text)
}

pub(crate) fn is_common_relations_result(result: &ResultRecord) -> bool {
    result.text.starts_with(COMMON_RELATIONS_REPORT_HEADER)
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

#[cfg(test)]
mod relation_comparison_temporal_identity_tests {
    use super::*;

    #[test]
    fn untimed_comparison_key_identity_matches_the_legacy_shape() {
        let key = (
            "is".to_owned(),
            "ore".to_owned(),
            false,
            "lab".to_owned(),
            None,
            BTreeSet::new(),
        );
        let legacy_key = ("is".to_owned(), "ore".to_owned(), false, "lab".to_owned());

        assert_eq!(
            relation_comparison_key_identity(&key).unwrap(),
            identity(&legacy_key).unwrap()
        );
    }
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
        describe_without_condition_refs(proposition),
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
                describe_without_condition_refs(&fact.proposition),
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
            describe_without_condition_refs(&fact.proposition),
            fact.proposition.scope,
        ));
    }
    for id in &answer.used_rules {
        if let Some(rule) = memory.rules.iter().find(|rule| &rule.id == id) {
            let premises = std::iter::once(&rule.premise)
                .chain(rule.additional_premises.iter())
                .map(describe_without_condition_refs)
                .collect::<Vec<_>>()
                .join(" かつ ");
            lines.push(format!(
                "適用規則（範囲: {}）: {} ならば {}（規則ID: {}）",
                rule.conclusion.scope,
                premises,
                describe_without_condition_refs(&rule.conclusion),
                rule.id
            ));
        }
    }
    for condition in &answer.condition_evaluations {
        lines.push(format!(
            "適用条件{}を{}と評価しました。",
            condition_reference_label(&condition.condition_id),
            condition_status_name(condition.status)
        ));
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
    let surface = if p.predicate == "is" {
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
            japanese_godan_negative_form(&p.predicate)
                .unwrap_or_else(|| format!("{} / {} / {} / 否定", p.subject, p.predicate, p.object))
        } else {
            p.predicate.clone()
        };
        if p.negative && predicate.contains(" / ") {
            predicate
        } else {
            format!("{}は{}を{}", p.subject, p.object, predicate)
        }
    } else {
        format!(
            "{} / {} / {} / {}",
            p.subject,
            p.predicate,
            p.object,
            if p.negative { "否定" } else { "肯定" }
        )
    };
    let surface = match p.temporal_scope.as_deref() {
        Some(time) => format!("{surface}（時刻範囲: {time}）"),
        None => surface,
    };
    if p.condition_refs.is_empty() {
        surface
    } else {
        format!(
            "{surface}（条件参照: {}）",
            p.condition_refs
                .iter()
                .cloned()
                .collect::<Vec<_>>()
                .join(", ")
        )
    }
}

fn describe_without_condition_refs(proposition: &Proposition) -> String {
    let mut without_condition_refs = proposition.clone();
    without_condition_refs.condition_refs.clear();
    describe(&without_condition_refs)
}

fn required_file_permission_stop(goal: &GoalContract) -> Option<(&'static str, &'static str)> {
    goal.output_path
        .as_ref()
        .is_some_and(|path| !goal.permissions.write_paths.contains(path))
        .then_some((
            "WRITE_PERMISSION_MISSING",
            "指定ファイルへの書込権限がありません。",
        ))
}

fn relation_registration_stop(
    goal: &GoalContract,
    budget_message: &'static str,
) -> Option<(&'static str, &'static str)> {
    required_file_permission_stop(goal).or_else(|| {
        (goal.budget.max_actions < 3 + u32::from(goal.output_path.is_some()))
            .then_some(("ACTION_BUDGET_EXHAUSTED", budget_message))
    })
}

pub(crate) fn premise_registration_units(fact: &Fact) -> Vec<String> {
    vec![
        format!("前提を保存しました: {}。", describe(&fact.proposition)),
        format!("前提ID: {}", fact.id),
        "保証範囲: 使用者が提示した前提。現実世界の真偽を検証したものではありません。".into(),
    ]
}

pub(crate) const CORRECTION_ASSERTION_PREDICATE: &str = "CORRECTION_CONTEXT_PREMISE_V1";

pub(crate) fn contextual_assertion_units(prior_id: &str, fact: &Fact) -> Vec<String> {
    let proposition = &fact.proposition;
    let mut units = vec![format!(
        "直前結果 {prior_id} の関係根拠を再確認し、主語「{}」とscope「{}」を復元しました。",
        proposition.subject, proposition.scope
    )];
    units.extend(premise_registration_units(fact));
    units
}

pub(crate) fn rule_registration_units(rule: &RelationRule) -> Vec<String> {
    let premise = describe(&rule.premise);
    let conclusion = describe(&rule.conclusion);
    let (statement, guarantee) = if let [second] = rule.additional_premises.as_slice() {
        (
            format!(
                "二つの条件付き前提を保存しました: {premise} かつ {} ならば {conclusion}。",
                describe(second)
            ),
            "保証範囲: 提示された二条件の組み合わせの中でのみ利用します。普遍的事実として学習しません。",
        )
    } else {
        (
            format!("条件付き前提を保存しました: {premise} ならば {conclusion}。"),
            "保証範囲: 提示された条件付き前提の中でのみ利用します。普遍的事実として学習しません。",
        )
    };
    vec![statement, format!("規則ID: {}", rule.id), guarantee.into()]
}

fn condition_reference_label(condition_id: &str) -> String {
    if condition_id.len() <= 64 {
        format!("「{condition_id}」")
    } else {
        String::new()
    }
}

fn condition_status_name(status: ConditionStatus) -> &'static str {
    match status {
        ConditionStatus::True => "TRUE",
        ConditionStatus::False => "FALSE",
        ConditionStatus::Unknown => "UNKNOWN",
        ConditionStatus::Conflict => "CONFLICT",
    }
}

pub(crate) struct ConditionReadout {
    pub(crate) revision: u64,
    pub(crate) evaluation: ConditionEvaluation,
    pub(crate) units: Vec<String>,
    pub(crate) detail: Option<Vec<relations::ConditionTraceStep>>,
}

impl ConditionReadout {
    pub(crate) fn proof_bytes(&self) -> Result<Vec<u8>> {
        if let Some(detail) = &self.detail {
            canonical(&(
                "minidora-condition-evaluation-detail-v1",
                self.revision,
                &self.evaluation,
                detail,
            ))
        } else {
            canonical(&(
                "minidora-condition-evaluation-v1",
                self.revision,
                &self.evaluation,
            ))
        }
    }

    pub(crate) fn verify_evidence_budget(&self, max_bytes: usize) -> Result<()> {
        if self
            .proof_bytes()?
            .len()
            .saturating_add(self.units.join("\n").len())
            > max_bytes
        {
            return Err(failure(
                "RESOURCE_LIMIT",
                "条件評価の証拠と表示が要求の予算を超えています。",
            ));
        }
        Ok(())
    }
}

pub(crate) fn evaluate_condition_report(
    index: &MemoryIndexSnapshot<'_, '_>,
    goal: &GoalContract,
) -> Result<ConditionReadout> {
    let (condition_id, subject, scope, temporal_scope, detailed) = match &goal.intent {
        Intent::EvaluateCondition {
            condition_id,
            subject,
            scope,
            temporal_scope,
        } => (condition_id, subject, scope, temporal_scope, false),
        Intent::EvaluateConditionDetailed {
            condition_id,
            subject,
            scope,
            temporal_scope,
        } => (condition_id, subject, scope, temporal_scope, true),
        _ => {
            return Err(failure(
                "CONDITION_EVALUATION_GOAL_INVALID",
                "条件評価の要求が一致しません。",
            ));
        }
    };
    let completion_actions = 3 + u32::from(goal.output_path.is_some());
    let (evaluation, trace) = relations::evaluate_active_condition(
        index,
        condition_id,
        subject,
        scope,
        temporal_scope.as_deref(),
        goal.budget.max_actions.saturating_sub(completion_actions),
        detailed,
    )?;
    let mut units = vec![
        format!(
            "条件 {condition_id}: {}",
            condition_status_name(evaluation.status)
        ),
        format!(
            "評価主体: {subject}、範囲: {scope}、時点: {}",
            temporal_scope.as_deref().unwrap_or("未指定")
        ),
    ];
    if let Some(reason) = &evaluation.reason {
        units.push(format!("評価理由: {reason}"));
    }
    if !evaluation.definition_record_ids.is_empty() {
        units.push(format!(
            "条件定義の記録ID: {}",
            evaluation.definition_record_ids.join("、")
        ));
    }
    if !evaluation.evidence_fact_ids.is_empty() {
        units.push(format!(
            "証拠Fact ID: {}",
            evaluation.evidence_fact_ids.join("、")
        ));
    }
    let detail = detailed.then_some(trace);
    if let Some(trace) = &detail {
        for (index, step) in trace.iter().enumerate() {
            append_condition_trace_units(&mut units, index + 1, step);
        }
    }
    Ok(ConditionReadout {
        revision: index.revision(),
        evaluation,
        units,
        detail,
    })
}

fn append_condition_trace_units(
    units: &mut Vec<String>,
    number: usize,
    step: &relations::ConditionTraceStep,
) {
    use relations::ConditionTraceTarget;
    let target = match &step.target {
        ConditionTraceTarget::Condition { condition_id } => format!("条件 {condition_id}"),
        ConditionTraceTarget::Fact {
            subject,
            predicate,
            object,
        } => {
            format!("Fact「{subject} / {predicate} / {object}」")
        }
        ConditionTraceTarget::And => "AND".into(),
        ConditionTraceTarget::Or => "OR".into(),
        ConditionTraceTarget::Not => "NOT".into(),
    };
    let status = condition_status_name(step.status);
    if let Some(parent) = step.parent {
        units.push(format!(
            "内訳 {number}（親 {}） {target}: {status}",
            parent + 1
        ));
    } else {
        units.push(format!("内訳 {number} {target}: {status}"));
    }
    if let Some(reason) = &step.reason {
        units.push(format!("内訳 {number} の診断: {reason}"));
    }
    for (label, ids) in [
        ("肯定", &step.positive_fact_ids),
        ("否定", &step.negative_fact_ids),
    ] {
        if !ids.is_empty() {
            units.push(format!(
                "内訳 {number} の{label}Fact ID: {}",
                ids.join("、")
            ));
        }
    }
}

pub(crate) fn condition_report_at_head(
    store: &Store,
    actor: &str,
    goal: &GoalContract,
) -> Result<ConditionReadout> {
    if let Some(report) =
        store.with_relation_index(actor, |index| evaluate_condition_report(index, goal))?
    {
        return Ok(report);
    }
    let snapshot = store.load_memory(actor)?;
    Store::with_snapshot_relation_index(actor, &snapshot, |index| {
        evaluate_condition_report(index, goal)
    })
}

pub(crate) struct RecordSourceReadout {
    pub(crate) proof: Vec<u8>,
    pub(crate) units: Vec<String>,
    pub(crate) dependencies: Vec<String>,
}

impl RecordSourceReadout {
    fn within_evidence_budget(self, max_bytes: usize, limit_message: &str) -> Result<Self> {
        if self.proof.len().saturating_add(self.units.join("\n").len()) > max_bytes {
            return Err(failure("RESOURCE_LIMIT", limit_message));
        }
        Ok(self)
    }

    pub(crate) fn matches_surface(&self, result: &ResultRecord, plan: &ContentPlan) -> bool {
        result
            .dependencies
            .split_last()
            .is_some_and(|(proof_ref, dependencies)| {
                proof_ref == &digest(&self.proof) && dependencies == self.dependencies
            })
            && result.guarantee == Guarantee::Computed
            && plan.guarantee == Guarantee::Computed
            && result.calculation.is_none()
            && result.evidence.is_empty()
            && plan.required_units == self.units
            && result.text == self.units.join("\n")
            && plan.text == result.text
    }
}

pub(crate) struct RecordSourceTarget {
    pub(crate) lookup: Intent,
    previous: Option<PreviousRuleSource>,
}

struct PreviousRuleSource {
    reference: PreviousRuleReference,
    composed_parents: Option<[String; 2]>,
    list: Option<RuleListReference>,
}

struct RuleListReference {
    result_id: String,
    result_sha256: String,
    input_ref: String,
    proof_ref: String,
}

struct PreviousRuleReference {
    result_id: String,
    result_sha256: String,
    input_ref: String,
    computation_ref: String,
}

struct PreviousTypedRule {
    id: String,
    parent_ids: Vec<String>,
    composed: bool,
}

impl RecordSourceTarget {
    pub(crate) fn readout(
        &self,
        record: SourceMemoryRecord<'_>,
        store: &Store,
        actor: &str,
        revision: u64,
        goal: &GoalContract,
    ) -> Result<RecordSourceReadout> {
        if matches!(self.lookup, Intent::InspectComposedRuleSource { .. }) {
            match goal.intent {
                Intent::InspectPreviousRuleSourceById { .. } => {
                    require_record_source_actions(goal, 4)?
                }
                Intent::AskListedRuleSource { .. } => require_record_source_actions(goal, 5)?,
                _ => {}
            }
        }
        if let Some(parents) = self
            .previous
            .as_ref()
            .and_then(|previous| previous.composed_parents.as_ref())
            && !matches!(record, SourceMemoryRecord::ComposedRule { rule, .. } if rule.dependencies == *parents)
        {
            return Err(previous_composed_rule_source_unavailable());
        }
        let mut readout = record.readout(store, actor, revision, goal)?;
        let Some(previous) = &self.previous else {
            return Ok(readout);
        };
        let rule_proof: serde_json::Value = serde_json::from_slice(&readout.proof)?;
        let proof_kind = if matches!(goal.intent, Intent::InspectPreviousRuleSourceById { .. }) {
            "minidora-selected-previous-rule-source-v1"
        } else if previous.composed_parents.is_some() {
            "minidora-previous-composed-rule-source-v1"
        } else {
            "minidora-previous-rule-source-v1"
        };
        let reference = &previous.reference;
        let why_identity = (
            &reference.result_id,
            &reference.result_sha256,
            &reference.input_ref,
            &reference.computation_ref,
        );
        readout.proof = if let Some(list) = &previous.list {
            canonical(&(
                "minidora-listed-rule-source-v1",
                (
                    &list.result_id,
                    &list.result_sha256,
                    &list.input_ref,
                    &list.proof_ref,
                ),
                why_identity,
                rule_proof,
            ))?
        } else {
            let (result_id, result_sha256, input_ref, computation_ref) = why_identity;
            canonical(&(
                proof_kind,
                result_id,
                result_sha256,
                input_ref,
                computation_ref,
                rule_proof,
            ))?
        };
        readout.dependencies.splice(
            ..0,
            [
                reference.result_id.clone(),
                reference.computation_ref.clone(),
            ],
        );
        readout
            .units
            .insert(0, format!("参照した理由説明ID: {}", reference.result_id));
        if let Some(list) = &previous.list {
            readout
                .dependencies
                .splice(..0, [list.result_id.clone(), list.proof_ref.clone()]);
            readout
                .units
                .insert(0, format!("参照した規則一覧ID: {}", list.result_id));
        }
        readout.within_evidence_budget(
            goal.budget.max_evidence_bytes,
            "前の理由説明と規則の出典が要求の証拠予算を超えています。",
        )
    }
}

fn previous_composed_rule_source_unavailable() -> Error {
    failure(
        "PREVIOUS_COMPOSED_RULE_SOURCE_UNAVAILABLE",
        "参照した確定済み理由説明から、使用した合成規則を確認できません。",
    )
}

pub(crate) fn record_source_target(
    store: &Store,
    bundle: &InputBundle,
    prior_results: &BTreeMap<String, ResultRecord>,
) -> Result<RecordSourceTarget> {
    let (composed_only, selected_id) = match &bundle.meaning.intent {
        Intent::InspectPreviousRuleSource => (false, None),
        Intent::InspectPreviousComposedRuleSource => (true, None),
        Intent::InspectPreviousRuleSourceById { rule_id } => (false, Some(rule_id.as_str())),
        Intent::AskListedRuleSource { rule_id } => (false, Some(rule_id.as_str())),
        _ => {
            return Ok(RecordSourceTarget {
                lookup: bundle.meaning.intent.clone(),
                previous: None,
            });
        }
    };
    let from_list = matches!(bundle.meaning.intent, Intent::AskListedRuleSource { .. });
    let unavailable = || {
        if from_list {
            failure(
                "LISTED_RULE_SOURCE_UNAVAILABLE",
                "参照した確定済み規則一覧と元の理由説明を照合できません。",
            )
        } else if composed_only {
            previous_composed_rule_source_unavailable()
        } else {
            failure(
                "PREVIOUS_RULE_SOURCE_UNAVAILABLE",
                "参照した回答から、使用した規則の確定済み理由説明を確認できません。",
            )
        }
    };
    let (reference, rules, list) = if from_list {
        let (reference, rules, list) =
            verified_listed_rules(store, bundle, prior_results, &unavailable)?;
        (reference, rules, Some(list))
    } else {
        let (reference, rules) =
            verified_previous_rules(store, bundle, prior_results, false, &unavailable)?;
        (reference, rules, None)
    };
    let mut candidates = rules.iter().filter(|rule| {
        (!composed_only || rule.composed) && selected_id.is_none_or(|selected| selected == rule.id)
    });
    let rule = match (candidates.next(), candidates.next()) {
        (Some(rule), None) => rule,
        (None, _) if selected_id.is_some() => {
            return Err(if from_list {
                failure(
                    "LISTED_RULE_SOURCE_ID_NOT_LISTED",
                    "指定した規則IDは参照した規則一覧に含まれていません。",
                )
            } else {
                failure(
                    "PREVIOUS_RULE_SOURCE_ID_NOT_USED",
                    "指定した規則IDは参照した理由説明の証拠に含まれていません。",
                )
            });
        }
        (None, _) => return Err(unavailable()),
        _ => {
            return Err(if composed_only {
                failure(
                    "PREVIOUS_COMPOSED_RULE_SOURCE_AMBIGUOUS",
                    "参照した理由説明には複数の合成規則があります。合成規則IDを指定してください。",
                )
            } else {
                failure(
                    "PREVIOUS_RULE_SOURCE_AMBIGUOUS",
                    "参照した理由説明には複数の規則があります。規則IDを指定してください。",
                )
            });
        }
    };
    let composed_parents = if composed_only || (selected_id.is_some() && rule.composed) {
        let ids = rules.iter().map(|rule| rule.id.as_str()).collect();
        Some(verified_previous_parent_ids(rule, &ids).ok_or_else(unavailable)?)
    } else {
        None
    };
    let rule_id = rule.id.clone();
    Ok(RecordSourceTarget {
        lookup: if composed_parents.is_some() {
            Intent::InspectComposedRuleSource { rule_id }
        } else {
            Intent::InspectDeclaredRuleSource { rule_id }
        },
        previous: Some(PreviousRuleSource {
            reference,
            composed_parents,
            list,
        }),
    })
}

fn verified_previous_result<'a>(
    store: &Store,
    bundle: &InputBundle,
    prior_results: &'a BTreeMap<String, ResultRecord>,
    unavailable: &impl Fn() -> Error,
) -> Result<(&'a ResultRecord, String, InputBundle)> {
    let previous = bundle
        .meaning
        .context_result_ref
        .as_ref()
        .and_then(|id| prior_results.get(id).filter(|result| &result.id == id))
        .filter(|result| result.lifecycle == Lifecycle::Commit)
        .ok_or_else(unavailable)?;
    if bundle.context_sha256 != identity(&Some(previous))?
        || bundle
            .request
            .context_ref
            .as_ref()
            .is_some_and(|id| id != &previous.id)
    {
        return Err(unavailable());
    }
    let actor = &bundle.request.actor_id;
    let case = &bundle.request.case_id;
    let (input_ref, origin) = store.with_verified_case_history(actor, case, |history| {
        let (input_ref, origin) = history
            .input_bundle(&previous.input_id, previous.input_version)?
            .ok_or_else(unavailable)?;
        if identity(&origin.request)? != previous.id
            || history
                .input_result(&previous.input_id, previous.input_version)?
                .as_ref()
                != Some(previous)
            || !history.contains_committed_result(previous)?
        {
            return Err(unavailable());
        }
        Ok((input_ref, origin))
    })?;
    Ok((previous, input_ref, origin))
}

fn verified_previous_rules(
    store: &Store,
    bundle: &InputBundle,
    prior_results: &BTreeMap<String, ResultRecord>,
    include_direct: bool,
    unavailable: &impl Fn() -> Error,
) -> Result<(PreviousRuleReference, Vec<PreviousTypedRule>)> {
    let (previous, input_ref, origin) =
        verified_previous_result(store, bundle, prior_results, unavailable)?;
    if previous.guarantee != Guarantee::Derived
        && !(include_direct && previous.guarantee == Guarantee::UserPremise)
    {
        return Err(unavailable());
    }
    let Intent::AskWhy { proposition } = &origin.meaning.intent else {
        return Err(unavailable());
    };
    if origin.meaning.source_kind != "USER_REQUEST"
        || !input::uses_generated_relation_explanation(&origin.meaning)
    {
        return Err(unavailable());
    }
    let plan = previous.content_plan.as_ref().ok_or_else(unavailable)?;
    let render = previous.render.as_ref().ok_or_else(unavailable)?;
    let (computation_ref, dependencies) = plan.dependencies.split_last().ok_or_else(unavailable)?;
    if dependencies != previous.dependencies
        || plan.guarantee != previous.guarantee
        || plan.text != previous.text
        || !validate_render(plan, render)
    {
        return Err(unavailable());
    }
    let bytes = store
        .get_bytes_if_present(
            &bundle.request.actor_id,
            &bundle.request.case_id,
            computation_ref,
        )?
        .ok_or_else(unavailable)?;
    let artifact: CoreComputationArtifactGeneratedContent =
        serde_json::from_slice(&bytes).map_err(|_| unavailable())?;
    let meaning = &artifact.relation_meaning_plan;
    if artifact.kind != generated_artifact_kind(artifact.adaptation.as_ref())
        || &meaning.asked != proposition
        || meaning.conclusion != artifact.relation_meaning
        || meaning.guarantee != previous.guarantee
        || meaning.dependencies != dependencies
        || artifact.relation_proof_dependencies != dependencies
        || artifact.dependencies != dependencies
        || artifact.units != plan.required_units
        || artifact.content_generation.text != previous.text
    {
        return Err(unavailable());
    }
    let rules: Vec<_> = meaning
        .units
        .iter()
        .filter_map(|unit| match &unit.meaning {
            CoreSemanticMeaning::Rule {
                record_id,
                record_dependencies,
                composed,
                ..
            } => Some(PreviousTypedRule {
                id: record_id.clone(),
                parent_ids: record_dependencies.clone(),
                composed: *composed,
            }),
            _ => None,
        })
        .collect();
    let proof_rule_ids: BTreeSet<_> = dependencies
        .iter()
        .map(String::as_str)
        .filter(|id| id.starts_with("rule:"))
        .collect();
    if rules
        .iter()
        .map(|rule| rule.id.as_str())
        .collect::<BTreeSet<_>>()
        != proof_rule_ids
    {
        return Err(unavailable());
    }

    Ok((
        PreviousRuleReference {
            result_id: previous.id.clone(),
            result_sha256: identity(previous)?,
            input_ref,
            computation_ref: computation_ref.clone(),
        },
        rules,
    ))
}

fn verified_previous_parent_ids(
    rule: &PreviousTypedRule,
    ids: &BTreeSet<&str>,
) -> Option<[String; 2]> {
    let [left, right] = rule.parent_ids.as_slice() else {
        return None;
    };
    if left == right
        || [left.as_str(), right.as_str()]
            .into_iter()
            .any(|id| id == rule.id || !ids.contains(id))
    {
        return None;
    }
    Some([left.clone(), right.clone()])
}

pub(crate) fn previous_rule_list(
    store: &Store,
    bundle: &InputBundle,
    prior_results: &BTreeMap<String, ResultRecord>,
    goal: &GoalContract,
) -> Result<RecordSourceReadout> {
    let unavailable = || {
        failure(
            "PREVIOUS_RULE_LIST_UNAVAILABLE",
            "参照した確定済み理由説明から使用した規則の一覧を確認できません。",
        )
    };
    let (reference, rules) =
        verified_previous_rules(store, bundle, prior_results, true, &unavailable)?;
    previous_rule_inventory(&reference, &rules, &unavailable)?.within_evidence_budget(
        goal.budget.max_evidence_bytes,
        "規則の一覧と照合証拠が要求の証拠予算を超えています。",
    )
}

fn previous_rule_inventory(
    reference: &PreviousRuleReference,
    rules: &[PreviousTypedRule],
    unavailable: &impl Fn() -> Error,
) -> Result<RecordSourceReadout> {
    let ids: BTreeSet<_> = rules.iter().map(|rule| rule.id.as_str()).collect();
    if ids.len() != rules.len() {
        return Err(unavailable());
    }
    let mut units = vec![
        format!("参照した理由説明ID: {}", reference.result_id),
        format!("使用規則: {}件", rules.len()),
    ];
    for rule in rules {
        if rule.composed {
            let [left, right] = verified_previous_parent_ids(rule, &ids).ok_or_else(unavailable)?;
            units.push(format!("合成規則: {}（親規則: {left}、{right}）", rule.id));
        } else {
            if !rule.parent_ids.is_empty() {
                return Err(unavailable());
            }
            units.push(format!("宣言規則: {}", rule.id));
        }
    }
    let rows: Vec<_> = rules
        .iter()
        .map(|rule| (&rule.id, &rule.parent_ids, rule.composed))
        .collect();
    let proof = canonical(&(
        "minidora-previous-rule-list-v1",
        &reference.result_id,
        &reference.result_sha256,
        &reference.input_ref,
        &reference.computation_ref,
        &rows,
    ))?;
    Ok(RecordSourceReadout {
        proof,
        units,
        dependencies: vec![
            reference.result_id.clone(),
            reference.computation_ref.clone(),
        ],
    })
}

fn verified_listed_rules(
    store: &Store,
    bundle: &InputBundle,
    prior_results: &BTreeMap<String, ResultRecord>,
    unavailable: &impl Fn() -> Error,
) -> Result<(
    PreviousRuleReference,
    Vec<PreviousTypedRule>,
    RuleListReference,
)> {
    let (list, input_ref, origin) =
        verified_previous_result(store, bundle, prior_results, unavailable)?;
    if origin.meaning.intent != Intent::ListPreviousRules
        || origin.meaning.parser_version != input::PREVIOUS_RULE_LIST_PARSER_VERSION
        || origin.meaning.source_kind != "USER_REQUEST"
        || list.actor_memory_revision.is_some()
    {
        return Err(unavailable());
    }
    let plan = list.content_plan.as_ref().ok_or_else(unavailable)?;
    let render = list.render.as_ref().ok_or_else(unavailable)?;
    let proof_ref = list.dependencies.last().ok_or_else(unavailable)?;
    let (compute_ref, plan_dependencies) =
        plan.dependencies.split_last().ok_or_else(unavailable)?;
    let (reference, rules) =
        verified_previous_rules(store, &origin, prior_results, true, unavailable)?;
    let readout = previous_rule_inventory(&reference, &rules, unavailable)?;
    let actor = &bundle.request.actor_id;
    let case = &bundle.request.case_id;
    let computation = canonical(&(
        &list.calculation,
        &readout.units,
        &list.evidence,
        &list.dependencies,
    ))?;
    if !readout.matches_surface(list, plan)
        || plan_dependencies != list.dependencies
        || !validate_render(plan, render)
        || store
            .get_bytes_if_present(actor, case, proof_ref)?
            .as_deref()
            != Some(&readout.proof)
        || store
            .get_bytes_if_present(actor, case, compute_ref)?
            .as_deref()
            != Some(&computation)
    {
        return Err(unavailable());
    }
    let list_ref = RuleListReference {
        result_id: list.id.clone(),
        result_sha256: identity(list)?,
        input_ref,
        proof_ref: proof_ref.clone(),
    };
    Ok((reference, rules, list_ref))
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum SourceMemoryRecord<'a> {
    Fact(&'a Fact),
    Condition(&'a ConditionRecord),
    Rule(&'a RelationRule),
    ComposedRule {
        rule: &'a RelationRule,
        parents: [&'a RelationRule; 2],
    },
}

struct CommittedRecordSource {
    case_id: String,
    input_id: String,
    version: u64,
    bundle_ref: String,
    bundle: InputBundle,
}

fn verified_committed_record_source(
    store: &Store,
    actor: &str,
    id: &str,
    origin: &str,
    unavailable: impl Fn() -> Error,
) -> Result<CommittedRecordSource> {
    let (case_id, input_id, version) = store
        .owned_source_input_key(actor, origin)?
        .ok_or_else(&unavailable)?;
    let (bundle_ref, bundle) = store.with_verified_case_history(actor, &case_id, |history| {
        let (bundle_ref, bundle) = history
            .input_bundle(&input_id, version)?
            .ok_or_else(&unavailable)?;
        let source = history
            .input_result(&input_id, version)?
            .ok_or_else(&unavailable)?;
        if identity(&bundle.request)? != origin
            || source.id != origin
            || source.input_id != input_id
            || source.input_version != version
            || source.lifecycle != Lifecycle::Commit
            || source.guarantee != Guarantee::UserPremise
            || !source
                .dependencies
                .iter()
                .any(|dependency| dependency == id)
            || !history.contains_committed_result(&source)?
        {
            return Err(unavailable());
        }
        Ok((bundle_ref, bundle))
    })?;
    Ok(CommittedRecordSource {
        case_id,
        input_id,
        version,
        bundle_ref,
        bundle,
    })
}

fn verified_declared_rule(rule: &RelationRule, bundle: &InputBundle) -> Result<bool> {
    let Some((premises @ [premise, additional @ ..], conclusion)) =
        rule_declaration(&bundle.meaning.intent)
    else {
        return Ok(false);
    };
    Ok(premise == &rule.premise
        && additional == rule.additional_premises
        && conclusion == &rule.conclusion
        && rule.dependencies.is_empty()
        && rule.id == relations::declared_rule_id(premises, conclusion, &rule.source_input)?)
}

fn composed_rule_source_unavailable() -> Error {
    failure(
        "COMPOSED_RULE_SOURCE_UNAVAILABLE",
        "指定した合成規則と二つの親規則の確定済み入力を照合できません。",
    )
}

impl SourceMemoryRecord<'_> {
    pub(crate) fn readout(
        self,
        store: &Store,
        actor: &str,
        revision: u64,
        goal: &GoalContract,
    ) -> Result<RecordSourceReadout> {
        let (id, origin, code, message) = match self {
            Self::Fact(fact) => (
                &fact.id,
                &fact.source_input,
                "FACT_SOURCE_UNAVAILABLE",
                "指定したFactの確定済み入力と原文の対応を確認できません。",
            ),
            Self::Condition(record) => (
                &record.id,
                &record.source_input,
                "CONDITION_SOURCE_UNAVAILABLE",
                "指定した条件定義の確定済み入力と原文の対応を確認できません。",
            ),
            Self::Rule(rule) => (
                &rule.id,
                &rule.source_input,
                "RULE_SOURCE_UNAVAILABLE",
                "指定した規則の確定済み入力と原文の対応を確認できません。",
            ),
            Self::ComposedRule { rule, parents } => {
                return read_composed_rule_source(store, actor, revision, goal, rule, parents);
            }
        };
        let unavailable = || failure(code, message);
        let CommittedRecordSource {
            case_id: source_case,
            input_id,
            version,
            bundle_ref,
            bundle,
        } = verified_committed_record_source(store, actor, id, origin, unavailable)?;
        let (proof, mut units, source_roles, limit_message) = match self {
            Self::Fact(fact) => {
                let evidence = fact.source_evidence.as_ref().ok_or_else(unavailable)?;
                if fact.guarantee != Guarantee::UserPremise
                    || fact.id != relations::user_premise_id(&fact.proposition, &fact.source_input)?
                    || input::assertion_source_evidence(&bundle)?.as_ref() != Some(evidence)
                {
                    return Err(unavailable());
                }
                (
                    canonical(&(
                        "minidora-fact-source-v1",
                        revision,
                        fact,
                        &source_case,
                        &input_id,
                        version,
                        &bundle_ref,
                    ))?,
                    vec![
                        format!("保存前提 {}", fact.id),
                        format!("記憶状態: {}", if fact.valid { "有効" } else { "失効" }),
                        format!("命題: {}", describe(&fact.proposition)),
                    ],
                    Some(&evidence.source_roles),
                    "Factの出典と表示が要求の証拠予算を超えています。",
                )
            }
            Self::Condition(record) => {
                let definition = &record.definition;
                if record.semantics_version != CONDITION_SEMANTICS_VERSION
                    || record.id
                        != format!(
                            "condition-record:{}",
                            identity(&(CONDITION_SEMANTICS_VERSION, definition))?
                        )
                    || !input::verified_condition_definition_source(&bundle, definition)?
                {
                    return Err(unavailable());
                }
                let mut units = vec![
                    format!("条件定義 {}", record.id),
                    format!("記憶状態: {}", if record.valid { "有効" } else { "失効" }),
                    format!("条件ID: {}", definition.condition_id),
                    format!(
                        "適用主体: {}、範囲: {}",
                        definition.subject, definition.scope
                    ),
                    format!(
                        "期間の下限（含む）: {}、上限（含まない）: {}",
                        definition.valid_from.as_deref().unwrap_or("なし"),
                        definition.valid_until.as_deref().unwrap_or("なし"),
                    ),
                    format!("意味版: {}", record.semantics_version),
                    format!("条件式: {}", serde_json::to_string(&definition.expression)?),
                ];
                if !record.dependencies.is_empty() {
                    units.push(format!(
                        "依存する定義記録ID: {}",
                        record.dependencies.join("、")
                    ));
                }
                (
                    canonical(&(
                        "minidora-condition-source-v1",
                        revision,
                        record,
                        &source_case,
                        &input_id,
                        version,
                        &bundle_ref,
                        &bundle.raw_sha256,
                        &bundle.meaning.source_roles,
                    ))?,
                    units,
                    Some(&bundle.meaning.source_roles),
                    "条件定義の出典と表示が要求の証拠予算を超えています。",
                )
            }
            Self::Rule(rule) => {
                const LIMIT_MESSAGE: &str = "規則の出典と表示が要求の証拠予算を超えています。";
                let roles = &bundle.meaning.source_roles;
                if !verified_declared_rule(rule, &bundle)? {
                    return Err(unavailable());
                }
                let (proof, roles) = if !roles.is_empty()
                    && input::verified_guarded_relation_rule_source(&bundle)?
                {
                    let proof = canonical(&(
                        "minidora-rule-source-v1",
                        revision,
                        rule,
                        &source_case,
                        &input_id,
                        version,
                        &bundle_ref,
                        &bundle.raw_sha256,
                        roles,
                    ))?;
                    (proof, Some(roles))
                } else if matches!(
                    goal.intent,
                    Intent::InspectDeclaredRuleSource { .. }
                        | Intent::InspectPreviousRuleSource
                        | Intent::InspectPreviousRuleSourceById { .. }
                        | Intent::AskListedRuleSource { .. }
                ) && input::verified_plain_rule_source(&bundle)?
                {
                    let proof = canonical(&(
                        "minidora-plain-rule-source-v1",
                        revision,
                        rule,
                        &source_case,
                        &input_id,
                        version,
                        &bundle_ref,
                        &bundle.raw_sha256,
                        &bundle.meaning.parser_version,
                    ))?;
                    (proof, None)
                } else {
                    return Err(unavailable());
                };
                (proof, rule_source_units("規則", rule), roles, LIMIT_MESSAGE)
            }
            Self::ComposedRule { .. } => unreachable!("composed rule source handled above"),
        };
        units.extend([
            format!("出典ケース: {source_case:?}、入力ID: {input_id:?}、版: {version}"),
            format!("元の結果ID: {origin}"),
            format!("原文SHA-256: {}", bundle.raw_sha256),
        ]);
        if let Some(roles) = source_roles {
            for (role, spans) in roles {
                for span in spans {
                    units.push(format!(
                        "原文 {role} [{}, {}): {:?}",
                        span.start, span.end, span.text
                    ));
                }
            }
        } else {
            units.push(format!("原文: {:?}", bundle.request.text));
        }
        RecordSourceReadout {
            proof,
            units,
            dependencies: vec![id.clone(), origin.clone()],
        }
        .within_evidence_budget(goal.budget.max_evidence_bytes, limit_message)
    }
}

fn rule_source_units(label: &str, rule: &RelationRule) -> Vec<String> {
    let mut units = vec![
        format!("{label} {}", rule.id),
        format!("記憶状態: {}", if rule.valid { "有効" } else { "失効" }),
        format!("適用範囲: {}", rule.premise.scope),
    ];
    units.extend(
        std::iter::once(&rule.premise)
            .chain(&rule.additional_premises)
            .enumerate()
            .map(|(index, premise)| format!("前提 {}: {}", index + 1, describe(premise))),
    );
    units.push(format!("結論: {}", describe(&rule.conclusion)));
    units
}

fn read_composed_rule_source(
    store: &Store,
    actor: &str,
    revision: u64,
    goal: &GoalContract,
    rule: &RelationRule,
    parents: [&RelationRule; 2],
) -> Result<RecordSourceReadout> {
    if (rule.valid && parents.iter().any(|parent| !parent.valid))
        || !relations::verified_composed_rule(rule, parents[0], parents[1])?
    {
        return Err(composed_rule_source_unavailable());
    }
    let mut units = rule_source_units("合成規則", rule);
    let mut origins = Vec::with_capacity(parents.len());
    for (index, parent) in parents.into_iter().enumerate() {
        let origin = verified_committed_record_source(
            store,
            actor,
            &parent.id,
            &parent.source_input,
            composed_rule_source_unavailable,
        )?;
        if !verified_declared_rule(parent, &origin.bundle)?
            || !(input::verified_guarded_relation_rule_source(&origin.bundle)?
                || input::verified_plain_rule_source(&origin.bundle)?)
        {
            return Err(composed_rule_source_unavailable());
        }
        units.extend([
            format!("親規則 {}: {}", index + 1, parent.id),
            format!(
                "親規則の記憶状態: {}",
                if parent.valid { "有効" } else { "失効" }
            ),
            format!(
                "出典ケース: {:?}、入力ID: {:?}、版: {}",
                origin.case_id, origin.input_id, origin.version
            ),
            format!("元の結果ID: {}", parent.source_input),
            format!("原文SHA-256: {}", origin.bundle.raw_sha256),
            format!("原文: {:?}", origin.bundle.request.text),
        ]);
        origins.push(origin);
    }
    let parent_sources: Vec<_> = parents
        .iter()
        .zip(&origins)
        .map(|(parent, origin)| {
            (
                parent,
                &origin.case_id,
                &origin.input_id,
                origin.version,
                &origin.bundle_ref,
                &origin.bundle.raw_sha256,
                &origin.bundle.meaning.parser_version,
            )
        })
        .collect();
    let proof = canonical(&(
        "minidora-composed-rule-source-v1",
        revision,
        rule,
        parent_sources,
    ))?;
    RecordSourceReadout {
        proof,
        units,
        dependencies: vec![
            rule.id.clone(),
            parents[0].id.clone(),
            parents[1].id.clone(),
            parents[0].source_input.clone(),
            parents[1].source_input.clone(),
        ],
    }
    .within_evidence_budget(
        goal.budget.max_evidence_bytes,
        "合成規則と親規則の出典が要求の証拠予算を超えています。",
    )
}

pub(crate) fn check_record_source_actions(goal: &GoalContract) -> Result<()> {
    // Reserve the record and verified source-history reads in addition to
    // compute, content, render and optional file confirmation.
    let reads = match goal.intent {
        Intent::ListPreviousRules => 1,
        Intent::InspectPreviousComposedRuleSource | Intent::AskListedRuleSource { .. } => 4,
        Intent::InspectComposedRuleSource { .. }
        | Intent::InspectPreviousRuleSource
        | Intent::InspectPreviousRuleSourceById { .. } => 3,
        _ => 2,
    };
    require_record_source_actions(goal, reads)
}

fn require_record_source_actions(goal: &GoalContract, reads: u32) -> Result<()> {
    if goal.budget.max_actions < reads + 3 + u32::from(goal.output_path.is_some()) {
        let message = match &goal.intent {
            Intent::InspectFactSource { .. } => "Factと元入力を照合する行動予算が不足しています。",
            Intent::InspectRuleSource { .. } | Intent::InspectDeclaredRuleSource { .. } => {
                "規則と元入力を照合する行動予算が不足しています。"
            }
            Intent::InspectComposedRuleSource { .. } => {
                "合成規則と親規則の元入力を照合する行動予算が不足しています。"
            }
            Intent::InspectPreviousRuleSource | Intent::InspectPreviousRuleSourceById { .. } => {
                "前の理由説明と規則の元入力を照合する行動予算が不足しています。"
            }
            Intent::InspectPreviousComposedRuleSource => {
                "前の理由説明と合成規則の親規則を照合する行動予算が不足しています。"
            }
            Intent::ListPreviousRules => "前の理由説明を照合する行動予算が不足しています。",
            Intent::AskListedRuleSource { .. } => {
                "規則一覧と規則の元入力を照合する行動予算が不足しています。"
            }
            _ => "条件定義と元入力を照合する行動予算が不足しています。",
        };
        return Err(failure("ACTION_BUDGET_EXHAUSTED", message));
    }
    Ok(())
}

pub(crate) fn record_source_memory_at_head(store: &Store, actor: &str) -> Result<MemoryState> {
    // Read validity from the authoritative snapshot, including for retained
    // inactive records that are omitted by normal evidence selection.
    let (snapshot, head) = store.load_memory_with_reference(actor)?;
    if head.is_some_and(|(revision, _)| u64::try_from(revision).ok() != Some(snapshot.revision)) {
        return Err(failure(
            "INTEGRITY_FAILURE",
            "the current memory row differs from its immutable snapshot version",
        ));
    }
    Ok(snapshot)
}

pub(crate) fn lookup_source_record<'a>(
    memory: &'a MemoryState,
    intent: &Intent,
) -> Result<SourceMemoryRecord<'a>> {
    let condition_not_found = || {
        failure(
            "CONDITION_SOURCE_NOT_FOUND",
            "指定した条件定義を利用者の保存記録から確認できません。",
        )
    };
    match intent {
        Intent::InspectFactSource { fact_id } => memory
            .facts
            .iter()
            .find(|fact| &fact.id == fact_id)
            .map(SourceMemoryRecord::Fact)
            .ok_or_else(|| {
                failure(
                    "FACT_SOURCE_NOT_FOUND",
                    "指定したFactを利用者の保存記録から確認できません。",
                )
            }),
        Intent::InspectConditionSource { record_id } => memory
            .conditions
            .iter()
            .find(|record| &record.id == record_id)
            .map(SourceMemoryRecord::Condition)
            .ok_or_else(condition_not_found),
        Intent::InspectRuleSource { rule_id } | Intent::InspectDeclaredRuleSource { rule_id } => {
            memory
                .rules
                .iter()
                .find(|rule| &rule.id == rule_id)
                .map(SourceMemoryRecord::Rule)
                .ok_or_else(|| {
                    failure(
                        "RULE_SOURCE_NOT_FOUND",
                        "指定した規則を利用者の保存記録から確認できません。",
                    )
                })
        }
        Intent::InspectComposedRuleSource { rule_id } => {
            let find_rule = |id: &str| memory.rules.iter().find(|rule| rule.id == id);
            let rule = find_rule(rule_id).ok_or_else(|| {
                failure(
                    "COMPOSED_RULE_SOURCE_NOT_FOUND",
                    "指定した合成規則を利用者の保存記録から確認できません。",
                )
            })?;
            let [left, right] = rule.dependencies.as_slice() else {
                return Err(composed_rule_source_unavailable());
            };
            let left = find_rule(left).ok_or_else(composed_rule_source_unavailable)?;
            let right = find_rule(right).ok_or_else(composed_rule_source_unavailable)?;
            Ok(SourceMemoryRecord::ComposedRule {
                rule,
                parents: [left, right],
            })
        }
        Intent::InspectConditionSourceByDefinition {
            condition_id,
            subject,
            scope,
        } => memory
            .conditions
            .iter()
            .find(|record| {
                // The authenticated snapshot keeps original definitions after invalidation.
                let definition = &record.definition;
                &definition.condition_id == condition_id
                    && &definition.subject == subject
                    && &definition.scope == scope
            })
            .map(SourceMemoryRecord::Condition)
            .ok_or_else(condition_not_found),
        _ => Err(failure(
            "RECORD_SOURCE_GOAL_INVALID",
            "出典照会の要求が一致しません。",
        )),
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
        && matches!(
            (render.renderer_version.as_str(), &render.target),
            (output::LEGACY_RENDERER_VERSION, None) | (output::RENDERER_VERSION, Some(_))
        )
}

pub(crate) fn validate_goal_render(
    goal: &GoalContract,
    plan: &ContentPlan,
    render: &RenderObservation,
) -> bool {
    validate_render(plan, render)
        && match goal.renderer_version.as_deref() {
            None => render.renderer_version == output::LEGACY_RENDERER_VERSION,
            Some(output::RENDERER_VERSION) => {
                render.renderer_version == output::RENDERER_VERSION
                    && match (&goal.output_path, &render.target) {
                        (None, Some(RenderTarget::Display)) => true,
                        (Some(expected), Some(RenderTarget::LocalFile { path })) => {
                            expected.as_os_str() == path.as_os_str()
                        }
                        _ => false,
                    }
            }
            Some(_) => false,
        }
}

#[cfg(test)]
mod active_relation_projection_tests {
    use super::*;
    use std::fs;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT_TEMP_DIRECTORY: AtomicU64 = AtomicU64::new(0);

    fn proposition(subject: &str, predicate: &str, object: &str) -> Proposition {
        Proposition {
            subject: subject.into(),
            predicate: predicate.into(),
            object: object.into(),
            negative: false,
            scope: "lab".into(),
            condition_refs: BTreeSet::new(),
            temporal_scope: None,
        }
    }

    #[test]
    fn indexed_ask_projection_keeps_only_the_goal_and_condition_closure() {
        let serial = NEXT_TEMP_DIRECTORY.fetch_add(1, Ordering::Relaxed);
        let directory = std::env::temp_dir().join(format!(
            "minidora-active-relation-test-{}-{serial}",
            std::process::id()
        ));
        fs::create_dir(&directory).expect("create isolated store directory");
        let mut engine = Engine::open(&directory).expect("open isolated engine");
        let request = RequestEnvelope::new("active-relation-case", "input", "alice", "保存前提");
        let source_input = identity(&request).expect("source input fingerprint");
        engine
            .store
            .register_input("alice", &request, &source_input)
            .expect("register source input");

        let mut full_memory = MemoryState::default();
        let evidence = relations::assert_fact(
            &mut full_memory,
            proposition("Nera", "is", "ready"),
            &source_input,
        )
        .expect("condition evidence fact");
        let condition_id = "condition:ready".to_owned();
        relations::define_condition(
            &mut full_memory,
            ConditionDefinition {
                condition_id: condition_id.clone(),
                subject: "$subject".into(),
                scope: "lab".into(),
                expression: ConditionExpression::Fact {
                    predicate: "is".into(),
                    object: "ready".into(),
                },
                valid_from: None,
                valid_until: None,
                dependencies: BTreeSet::new(),
            },
            &source_input,
        )
        .expect("condition definition");

        let target = Proposition {
            condition_refs: BTreeSet::from([condition_id]),
            ..proposition("Nera", "is", "mineral")
        };
        let premise = Proposition {
            condition_refs: target.condition_refs.clone(),
            ..proposition("Nera", "is", "ore")
        };
        let premise_fact = relations::assert_fact(&mut full_memory, premise.clone(), &source_input)
            .expect("relation premise");
        relations::add_rule(
            &mut full_memory,
            Proposition {
                subject: "$subject".into(),
                ..premise
            },
            Proposition {
                subject: "$subject".into(),
                ..target.clone()
            },
            &source_input,
        )
        .expect("relation rule");

        let ask_fact = relations::assert_fact(
            &mut full_memory,
            proposition("Nera", "is", "鉱石"),
            &source_input,
        )
        .expect("ordinary Ask premise");
        let ask_rule = relations::add_rule(
            &mut full_memory,
            proposition("$subject", "is", "鉱石"),
            proposition("$subject", "is", "鉱物"),
            &source_input,
        )
        .expect("ordinary Ask rule");
        relations::assert_fact(
            &mut full_memory,
            proposition("Nera", "is", "copper"),
            &source_input,
        )
        .expect("same-scope direct control");
        for ordinal in 0..24 {
            let mut out_of_scope = proposition("Nera", "is", "copper");
            out_of_scope.scope = format!("archive-{ordinal}");
            relations::assert_fact(&mut full_memory, out_of_scope, &source_input)
                .expect("out-of-scope direct control");
        }

        for ordinal in 0..40 {
            relations::assert_fact(
                &mut full_memory,
                proposition(&format!("unrelated-{ordinal}"), "is", "unrelated"),
                &source_input,
            )
            .expect("unrelated fact");
        }
        full_memory.revision = 1;
        engine
            .store
            .save_memory("alice", &full_memory)
            .expect("persist actor memory and its derived index");
        let connection = rusqlite::Connection::open(directory.join("state.sqlite3"))
            .expect("open derived index for cold-start setup");
        connection
            .execute("DELETE FROM memory_index_state WHERE actor_id='alice'", [])
            .expect("mark actor index cold");
        drop(connection);

        let (active, answer) = engine
            .active_relation_query("alice", &target, 128)
            .expect("run indexed typed relation query");
        assert_eq!(active.revision, full_memory.revision);
        assert_eq!(
            active.facts.len(),
            2,
            "only premise and condition evidence remain active"
        );
        assert_eq!(active.rules.len(), 1);
        assert_eq!(active.conditions.len(), 1);
        assert_eq!(answer.status, "SUPPORTED");
        assert_eq!(answer.condition_status, Some(ConditionStatus::True));
        assert_eq!(
            answer.condition_evaluations[0].evidence_fact_ids,
            vec![evidence.id.clone()]
        );
        assert!(answer.steps <= 128);
        assert!(relations::answer_dependencies(&answer).contains(&premise_fact.id));

        let full_answer = relations::query(&full_memory, &target, 128)
            .expect("run full-snapshot reference query");
        assert_eq!(answer.status, full_answer.status);
        assert_eq!(
            relations::answer_dependencies(&answer),
            relations::answer_dependencies(&full_answer)
        );

        let ordinary_ask = engine
            .execute(
                "alice",
                RequestEnvelope::new(
                    "active-relation-ask-case",
                    "active-relation-ask-input",
                    "alice",
                    "[scope=lab]Neraは鉱物ですか",
                ),
            )
            .expect("run the ordinary Ask intent through Core");
        assert_eq!(
            ordinary_ask.lifecycle,
            Lifecycle::Commit,
            "{ordinary_ask:?}"
        );
        assert!(ordinary_ask.dependencies.contains(&ask_fact.id));
        assert!(ordinary_ask.dependencies.contains(&ask_rule.id));

        let (scoped_memory, scoped_answer) = engine
            .active_relation_query("alice", &proposition("Nera", "is", "copper"), 4)
            .expect("supported queries do not spend budget on scope diagnostics");
        assert_eq!(scoped_memory.facts.len(), 1);
        assert_eq!(scoped_answer.status, "SUPPORTED");
        assert!(scoped_answer.steps <= 4);

        let mut next = full_memory.clone();
        next.revision = 2;
        next.facts
            .iter_mut()
            .find(|fact| fact.id == premise_fact.id)
            .expect("premise remains in immutable history")
            .valid = false;
        engine
            .store
            .save_memory("alice", &next)
            .expect("incrementally apply premise tombstone");
        let (_, after_tombstone) = engine
            .active_relation_query("alice", &target, 128)
            .expect("query updated index revision");
        assert_eq!(after_tombstone.status, "UNKNOWN");

        drop(engine);
        fs::remove_dir_all(directory).expect("remove isolated test store");
    }

    #[test]
    fn indexed_common_relations_use_bounded_subject_candidates_and_preserve_artifacts() {
        let serial = NEXT_TEMP_DIRECTORY.fetch_add(1, Ordering::Relaxed);
        let directory = std::env::temp_dir().join(format!(
            "minidora-active-common-relations-test-{}-{serial}",
            std::process::id()
        ));
        fs::create_dir(&directory).expect("create isolated store directory");
        let mut engine = Engine::open(&directory).expect("open isolated engine");
        let source_request = RequestEnvelope::new(
            "active-common-seed-case",
            "active-common-seed-input",
            "alice",
            "保存共通関係",
        );
        let source_input = identity(&source_request).expect("source input fingerprint");
        engine
            .store
            .register_input("alice", &source_request, &source_input)
            .expect("register relation source");

        let mut full_memory = MemoryState::default();
        let nera_rock = relations::assert_fact(
            &mut full_memory,
            proposition("Nera", "is", "rock"),
            &source_input,
        )
        .expect("Nera relation");
        let miko_rock = relations::assert_fact(
            &mut full_memory,
            proposition("Miko", "is", "rock"),
            &source_input,
        )
        .expect("Miko relation");
        let mineral_rule = relations::add_rule(
            &mut full_memory,
            proposition("$subject", "is", "rock"),
            proposition("$subject", "is", "mineral"),
            &source_input,
        )
        .expect("common variable-subject rule");
        for ordinal in 0..80 {
            relations::assert_fact(
                &mut full_memory,
                proposition(
                    &format!("unrelated-{ordinal}"),
                    "is",
                    &format!("unrelated-{ordinal}"),
                ),
                &source_input,
            )
            .expect("unrelated retained history");
        }
        full_memory.revision = 1;
        engine
            .store
            .save_memory("alice", &full_memory)
            .expect("persist actor memory and its derived index");

        let selected_ids = engine
            .store
            .with_relation_index("alice", |index| {
                let candidates = index.common_relation_candidates("Nera", "Miko", "lab", 8)?;
                Ok((
                    index.revision(),
                    candidates
                        .into_iter()
                        .map(|record| record.value.id().to_owned())
                        .collect::<BTreeSet<_>>(),
                ))
            })
            .expect("read subject index")
            .expect("save_memory creates the current relation index");
        assert_eq!(selected_ids.0, full_memory.revision);
        assert_eq!(
            selected_ids.1,
            BTreeSet::from([
                nera_rock.id.clone(),
                miko_rock.id.clone(),
                mineral_rule.id.clone(),
            ])
        );

        // Force the ordinary Core route through its cold-index rebuild path.
        let connection = rusqlite::Connection::open(directory.join("state.sqlite3"))
            .expect("open derived index for cold-start setup");
        connection
            .execute("DELETE FROM memory_index_state WHERE actor_id='alice'", [])
            .expect("mark actor index cold");
        drop(connection);

        let mut request = RequestEnvelope::new(
            "active-common-query-case",
            "active-common-query-input",
            "alice",
            "[scope=lab]NeraとMikoの共通点は何ですか",
        );
        request.budget.max_actions = 64;
        let result = engine
            .execute("alice", request.clone())
            .expect("run the normal Japanese AskCommonRelations route");
        assert_eq!(result.lifecycle, Lifecycle::Commit, "{result:?}");
        assert_eq!(result.actor_memory_revision, Some(full_memory.revision));
        assert_eq!(result.guarantee, Guarantee::Derived);
        assert!(result.dependencies.contains(&nera_rock.id));
        assert!(result.dependencies.contains(&miko_rock.id));
        assert!(result.dependencies.contains(&mineral_rule.id));
        assert!(
            !result
                .dependencies
                .iter()
                .any(|dependency| dependency.contains("unrelated"))
        );

        let result_id = base_result(&request).id;
        let indexed = engine
            .store
            .with_relation_index("alice", |index| {
                Ok((
                    index.revision(),
                    indexed_common_relations_answer(
                        index,
                        "Nera",
                        "Miko",
                        "lab",
                        10_000,
                        request.budget.max_evidence_bytes,
                        &result_id,
                    )?,
                ))
            })
            .expect("evaluate indexed common relations")
            .expect("current index is available");
        assert_eq!(indexed.0, full_memory.revision);
        let indexed_answer = match indexed.1 {
            CommonRelationsEvaluation::Complete(answer) => answer,
            CommonRelationsEvaluation::Blocked { code, message } => {
                panic!("indexed query unexpectedly blocked: {code}: {message}")
            }
        };
        let full_answer = match common_relations_answer(
            &full_memory,
            "Nera",
            "Miko",
            "lab",
            10_000,
            request.budget.max_evidence_bytes,
            &result_id,
        )
        .expect("full-snapshot reference evaluation")
        {
            CommonRelationsEvaluation::Complete(answer) => answer,
            CommonRelationsEvaluation::Blocked { code, message } => {
                panic!("full-snapshot reference unexpectedly blocked: {code}: {message}")
            }
        };
        assert_eq!(indexed_answer, full_answer);

        let artifact_ref = result
            .dependencies
            .iter()
            .find(|dependency| {
                dependency.len() == 64 && dependency.bytes().all(|byte| byte.is_ascii_hexdigit())
            })
            .expect("common-relations report artifact reference");
        assert_eq!(
            engine
                .store
                .get_bytes("alice", &request.case_id, artifact_ref)
                .expect("read adopted common-relations artifact"),
            indexed_answer.artifact_bytes
        );

        let mut budget_request = RequestEnvelope::new(
            "active-common-budget-case",
            "active-common-budget-input",
            "alice",
            "[scope=lab]NeraとMikoの共通点は何ですか",
        );
        budget_request.budget.max_actions = 3;
        let budget_stop = engine
            .execute("alice", budget_request)
            .expect("suspend when candidate selection exceeds the sealed budget");
        assert_eq!(budget_stop.lifecycle, Lifecycle::Suspend, "{budget_stop:?}");
        assert_eq!(
            budget_stop.actor_memory_revision,
            Some(full_memory.revision)
        );
        assert!(
            budget_stop
                .reason
                .as_deref()
                .is_some_and(|reason| reason.starts_with("RESOURCE_LIMIT:"))
        );

        // The former full-table route cannot fit the same request budget;
        // this records the bounded selection as the difference in behavior.
        let former_full_scan = common_relations_answer(
            &full_memory,
            "Nera",
            "Miko",
            "lab",
            64,
            request.budget.max_evidence_bytes,
            &result_id,
        )
        .expect("run old full-snapshot path at the normal route budget");
        assert!(matches!(
            former_full_scan,
            CommonRelationsEvaluation::Blocked {
                code: "RESOURCE_LIMIT",
                ..
            }
        ));

        drop(engine);
        fs::remove_dir_all(directory).expect("remove isolated test store");
    }

    #[test]
    fn indexed_conjunction_projects_each_clause_within_the_shared_budget() {
        let serial = NEXT_TEMP_DIRECTORY.fetch_add(1, Ordering::Relaxed);
        let directory = std::env::temp_dir().join(format!(
            "minidora-active-conjunction-test-{}-{serial}",
            std::process::id()
        ));
        fs::create_dir(&directory).expect("create isolated store directory");
        let mut engine = Engine::open(&directory).expect("open isolated engine");
        let source_request = RequestEnvelope::new(
            "active-conjunction-seed-case",
            "active-conjunction-seed-input",
            "alice",
            "保存関係",
        );
        let source_input = identity(&source_request).expect("source input fingerprint");
        engine
            .store
            .register_input("alice", &source_request, &source_input)
            .expect("register relation source");

        let mut full_memory = MemoryState::default();
        let direct_ore = relations::assert_fact(
            &mut full_memory,
            proposition("Nera", "is", "鉱石"),
            &source_input,
        )
        .expect("direct conjunction premise");
        let direct_life = relations::assert_fact(
            &mut full_memory,
            proposition("Nera", "is", "生物"),
            &source_input,
        )
        .expect("third direct conjunction premise");
        let rock = relations::assert_fact(
            &mut full_memory,
            proposition("Nera", "is", "岩石"),
            &source_input,
        )
        .expect("premise for the derived conjunction clause");
        let mineral_rule = relations::add_rule(
            &mut full_memory,
            proposition("$subject", "is", "岩石"),
            proposition("$subject", "is", "鉱物"),
            &source_input,
        )
        .expect("rule for the derived conjunction clause");
        let mut out_of_scope_rare = proposition("Nera", "is", "希少");
        out_of_scope_rare.scope = "archive".into();
        let rare = relations::assert_fact(&mut full_memory, out_of_scope_rare, &source_input)
            .expect("out-of-scope diagnostic premise");
        for ordinal in 0..72 {
            relations::assert_fact(
                &mut full_memory,
                proposition(
                    &format!("unrelated-{ordinal}"),
                    "is",
                    &format!("unrelated-{ordinal}"),
                ),
                &source_input,
            )
            .expect("unrelated retained history");
        }
        full_memory.revision = 1;
        engine
            .store
            .save_memory("alice", &full_memory)
            .expect("persist actor memory and its derived index");

        let mineral = proposition("Nera", "is", "鉱物");
        let ore = proposition("Nera", "is", "鉱石");
        let life = proposition("Nera", "is", "生物");
        let per_relation_budget = 16;
        let (
            mineral_projection,
            mineral_projection_actions,
            ore_projection,
            ore_projection_actions,
            life_projection,
            life_projection_actions,
        ) = engine
            .store
            .with_relation_index("alice", |index| {
                let (mineral_memory, mineral_actions) =
                    relations::active_relation_projection(index, &mineral, per_relation_budget)?;
                let (ore_memory, ore_actions) =
                    relations::active_relation_projection(index, &ore, per_relation_budget)?;
                let (life_memory, life_actions) =
                    relations::active_relation_projection(index, &life, per_relation_budget)?;
                Ok((
                    mineral_memory,
                    mineral_actions,
                    ore_memory,
                    ore_actions,
                    life_memory,
                    life_actions,
                ))
            })
            .expect("read one revision-matched relation index")
            .expect("save_memory creates the current relation index");
        assert_eq!(mineral_projection.revision, full_memory.revision);
        assert_eq!(ore_projection.revision, full_memory.revision);
        assert_eq!(life_projection.revision, full_memory.revision);
        assert_eq!(mineral_projection.facts.len(), 1);
        assert_eq!(mineral_projection.rules.len(), 1);
        assert_eq!(ore_projection.facts.len(), 1);
        assert!(ore_projection.rules.is_empty());
        assert_eq!(life_projection.facts.len(), 1);
        assert!(life_projection.rules.is_empty());
        assert!(mineral_projection_actions < per_relation_budget);
        assert!(ore_projection_actions < per_relation_budget);
        assert!(life_projection_actions < per_relation_budget);
        assert!(
            mineral_projection
                .facts
                .iter()
                .all(|fact| fact.id == rock.id)
        );
        assert!(
            mineral_projection
                .rules
                .iter()
                .all(|rule| rule.id == mineral_rule.id)
        );
        assert!(
            ore_projection
                .facts
                .iter()
                .all(|fact| fact.id == direct_ore.id)
        );
        assert!(
            life_projection
                .facts
                .iter()
                .all(|fact| fact.id == direct_life.id)
        );

        let full_mineral = relations::query(&full_memory, &mineral, per_relation_budget)
            .expect("full-snapshot mineral reference");
        let full_ore = relations::query(&full_memory, &ore, per_relation_budget)
            .expect("full-snapshot ore reference");
        let full_life = relations::query(&full_memory, &life, per_relation_budget)
            .expect("full-snapshot life reference");
        assert_eq!(full_mineral.status, "SUPPORTED");
        assert_eq!(full_ore.status, "SUPPORTED");
        assert_eq!(full_life.status, "SUPPORTED");

        let mut request = RequestEnvelope::new(
            "active-conjunction-query-case",
            "active-conjunction-query-input",
            "alice",
            "[scope=lab]Neraは鉱物かつ鉱石かつ生物ですか",
        );
        request.budget.max_actions = per_relation_budget * 3;
        let result = engine
            .execute("alice", request)
            .expect("run a normal Japanese AskConjunction through Core");
        assert_eq!(result.lifecycle, Lifecycle::Commit, "{result:?}");
        assert_eq!(result.actor_memory_revision, Some(full_memory.revision));
        assert_eq!(result.guarantee, Guarantee::Derived);
        let mut full_dependencies = relations::answer_dependencies(&full_mineral);
        full_dependencies.extend(relations::answer_dependencies(&full_ore));
        full_dependencies.extend(relations::answer_dependencies(&full_life));
        full_dependencies.sort();
        full_dependencies.dedup();
        assert_eq!(result.dependencies, full_dependencies);
        assert!(result.dependencies.contains(&rock.id));
        assert!(result.dependencies.contains(&mineral_rule.id));
        assert!(result.dependencies.contains(&direct_ore.id));
        assert!(result.dependencies.contains(&direct_life.id));
        assert!(
            !result
                .dependencies
                .iter()
                .any(|id| id.contains("unrelated"))
        );

        let mut out_of_scope_request = RequestEnvelope::new(
            "active-conjunction-out-of-scope-case",
            "active-conjunction-out-of-scope-input",
            "alice",
            "[scope=lab]Neraは鉱物かつ希少ですか",
        );
        out_of_scope_request.budget.max_actions = per_relation_budget * 2;
        let out_of_scope = engine
            .execute("alice", out_of_scope_request)
            .expect("classify one unresolved clause from the same indexed revision");
        assert_eq!(
            out_of_scope.lifecycle,
            Lifecycle::Suspend,
            "{out_of_scope:?}"
        );
        assert_eq!(
            out_of_scope.actor_memory_revision,
            Some(full_memory.revision)
        );
        assert!(
            out_of_scope
                .reason
                .as_deref()
                .is_some_and(|reason| reason.starts_with("OUT_OF_SCOPE:"))
        );
        assert!(out_of_scope.dependencies.contains(&rare.id));
        assert!(out_of_scope.dependencies.contains(&rock.id));
        assert!(out_of_scope.dependencies.contains(&mineral_rule.id));

        let mut budget_request = RequestEnvelope::new(
            "active-conjunction-budget-case",
            "active-conjunction-budget-input",
            "alice",
            "[scope=lab]Neraは鉱物かつ鉱石ですか",
        );
        budget_request.budget.max_actions = 4;
        let budget_stop = engine
            .execute("alice", budget_request)
            .expect("suspend when each clause cannot fit the shared budget");
        assert_eq!(budget_stop.lifecycle, Lifecycle::Suspend, "{budget_stop:?}");
        assert_eq!(
            budget_stop.actor_memory_revision,
            Some(full_memory.revision),
            "{budget_stop:?}"
        );
        assert!(
            budget_stop
                .reason
                .as_deref()
                .is_some_and(|reason| reason.starts_with("RESOURCE_LIMIT:"))
        );

        drop(engine);
        fs::remove_dir_all(directory).expect("remove isolated conjunction store");
    }

    #[test]
    fn indexed_premise_inventory_uses_subject_candidates_and_preserves_the_full_inventory() {
        let serial = NEXT_TEMP_DIRECTORY.fetch_add(1, Ordering::Relaxed);
        let directory = std::env::temp_dir().join(format!(
            "minidora-active-premise-inventory-test-{}-{serial}",
            std::process::id()
        ));
        fs::create_dir(&directory).expect("create isolated store directory");
        let mut engine = Engine::open(&directory).expect("open isolated engine");
        let source_request = RequestEnvelope::new(
            "active-premise-seed-case",
            "active-premise-seed-input",
            "alice",
            "保存関係",
        );
        let source_input = identity(&source_request).expect("source input fingerprint");
        engine
            .store
            .register_input("alice", &source_request, &source_input)
            .expect("register relation source");

        let mut full_memory = MemoryState::default();
        let rock = relations::assert_fact(
            &mut full_memory,
            proposition("Nera", "has", "rock"),
            &source_input,
        )
        .expect("Nera premise");
        let mineral_rule = relations::add_rule(
            &mut full_memory,
            proposition("$subject", "has", "rock"),
            proposition("$subject", "is", "mineral"),
            &source_input,
        )
        .expect("variable-subject inventory rule");
        let positive = relations::assert_fact(
            &mut full_memory,
            proposition("Nera", "is", "tall"),
            &source_input,
        )
        .expect("positive conflicting premise");
        let negative = relations::assert_fact(
            &mut full_memory,
            Proposition {
                negative: true,
                ..proposition("Nera", "is", "tall")
            },
            &source_input,
        )
        .expect("negative conflicting premise");
        let archived = relations::assert_fact(
            &mut full_memory,
            Proposition {
                scope: "archive".into(),
                ..proposition("Nera", "likes", "blue")
            },
            &source_input,
        )
        .expect("relation in a second scope");
        for ordinal in 0..80 {
            relations::assert_fact(
                &mut full_memory,
                proposition(
                    &format!("unrelated-{ordinal}"),
                    "is",
                    &format!("unrelated-{ordinal}"),
                ),
                &source_input,
            )
            .expect("unrelated retained history");
        }
        full_memory.revision = 1;
        engine
            .store
            .save_memory("alice", &full_memory)
            .expect("persist actor memory and its derived index");

        let indexed = engine
            .store
            .with_relation_index("alice", |index| {
                let candidates = index.relation_profile_candidates("Nera", 8)?;
                let candidate_ids = candidates
                    .into_iter()
                    .map(|record| record.value.id().to_owned())
                    .collect::<BTreeSet<_>>();
                let (inventory, actions) =
                    relations::active_relation_inventory_projection(index, "Nera", 64)?;
                Ok((index.revision(), candidate_ids, inventory, actions))
            })
            .expect("read premise inventory index")
            .expect("save_memory creates the current relation index");
        assert_eq!(indexed.0, full_memory.revision);
        assert_eq!(
            indexed.1,
            BTreeSet::from([
                rock.id.clone(),
                mineral_rule.id.clone(),
                positive.id.clone(),
                negative.id.clone(),
                archived.id.clone(),
            ])
        );
        assert!(indexed.3 <= 64);
        let full_inventory = relations::inventory_about(&full_memory, "Nera", 10_000)
            .expect("full-snapshot premise inventory reference");
        assert_eq!(indexed.2, full_inventory);
        assert!(
            matches!(
                relations::inventory_about(&full_memory, "Nera", 64),
                Err(Error::Contract { code, .. }) if code == "RELATION_BUDGET"
            ),
            "the full-snapshot baseline must exceed the same action budget"
        );

        // Force the normal Japanese Core route through cold-index reconstruction.
        let connection = rusqlite::Connection::open(directory.join("state.sqlite3"))
            .expect("open derived index for cold-start setup");
        connection
            .execute("DELETE FROM memory_index_state WHERE actor_id='alice'", [])
            .expect("mark actor index cold");
        drop(connection);

        let mut request = RequestEnvelope::new(
            "active-premise-query-case",
            "active-premise-query-input",
            "alice",
            "Neraについて、何が分かっていますか",
        );
        request.budget.max_actions = 64;
        let result = engine
            .execute("alice", request)
            .expect("run ordinary Japanese premise inspection through Core");
        assert_eq!(result.lifecycle, Lifecycle::Commit, "{result:?}");
        assert_eq!(result.actor_memory_revision, Some(full_memory.revision));
        assert_eq!(result.dependencies, full_inventory.dependencies);
        assert_eq!(result.guarantee, full_inventory.guarantee);
        let content_units = result
            .content_plan
            .as_ref()
            .expect("adopted premise inventory")
            .required_units
            .join("\n");
        assert!(content_units.contains("mineral"), "{content_units}");
        assert!(content_units.contains("archive"), "{content_units}");
        assert!(content_units.contains("対立根拠として"), "{content_units}");
        assert!(
            !result
                .dependencies
                .iter()
                .any(|dependency| dependency.contains("unrelated"))
        );

        let mut small_request = RequestEnvelope::new(
            "active-premise-budget-case",
            "active-premise-budget-input",
            "alice",
            "Neraについて、何が分かっていますか",
        );
        small_request.budget.max_actions = 8;
        let budget_stop = engine
            .execute("alice", small_request)
            .expect("run a bounded premise inspection");
        assert_eq!(budget_stop.lifecycle, Lifecycle::Suspend, "{budget_stop:?}");
        assert_eq!(
            budget_stop.actor_memory_revision,
            Some(full_memory.revision)
        );
        assert!(
            budget_stop
                .reason
                .as_deref()
                .is_some_and(|reason| reason.starts_with("RESOURCE_LIMIT:"))
        );

        drop(engine);
        fs::remove_dir_all(directory).expect("remove isolated store directory");
    }

    #[test]
    fn indexed_relation_profile_uses_bounded_subject_candidates_and_preserves_inventory() {
        let serial = NEXT_TEMP_DIRECTORY.fetch_add(1, Ordering::Relaxed);
        let directory = std::env::temp_dir().join(format!(
            "minidora-active-relation-profile-test-{}-{serial}",
            std::process::id()
        ));
        fs::create_dir(&directory).expect("create isolated store directory");
        let mut engine = Engine::open(&directory).expect("open isolated engine");
        let source_request = RequestEnvelope::new(
            "active-profile-seed-case",
            "active-profile-seed-input",
            "alice",
            "保存関係",
        );
        let source_input = identity(&source_request).expect("source input fingerprint");
        engine
            .store
            .register_input("alice", &source_request, &source_input)
            .expect("register relation source");

        let mut full_memory = MemoryState::default();
        let rock = relations::assert_fact(
            &mut full_memory,
            proposition("Nera", "has", "rock"),
            &source_input,
        )
        .expect("Nera premise");
        let mineral_rule = relations::add_rule(
            &mut full_memory,
            proposition("$subject", "has", "rock"),
            proposition("$subject", "is", "mineral"),
            &source_input,
        )
        .expect("variable-subject profile rule");
        let positive = relations::assert_fact(
            &mut full_memory,
            proposition("Nera", "is", "tall"),
            &source_input,
        )
        .expect("positive conflicting premise");
        let negative = relations::assert_fact(
            &mut full_memory,
            Proposition {
                negative: true,
                ..proposition("Nera", "is", "tall")
            },
            &source_input,
        )
        .expect("negative conflicting premise");
        let archived = relations::assert_fact(
            &mut full_memory,
            Proposition {
                scope: "archive".into(),
                ..proposition("Nera", "likes", "blue")
            },
            &source_input,
        )
        .expect("relation in a second scope");
        for ordinal in 0..80 {
            relations::assert_fact(
                &mut full_memory,
                proposition(
                    &format!("unrelated-{ordinal}"),
                    "is",
                    &format!("unrelated-{ordinal}"),
                ),
                &source_input,
            )
            .expect("unrelated retained history");
        }
        full_memory.revision = 1;
        engine
            .store
            .save_memory("alice", &full_memory)
            .expect("persist actor memory and its derived index");

        let indexed = engine
            .store
            .with_relation_index("alice", |index| {
                let candidates = index.relation_profile_candidates("Nera", 8)?;
                let candidate_ids = candidates
                    .into_iter()
                    .map(|record| record.value.id().to_owned())
                    .collect::<BTreeSet<_>>();
                Ok((
                    index.revision(),
                    candidate_ids,
                    relation_profile_answer_from_index(index, "Nera", 64, 16_384),
                ))
            })
            .expect("read relation-profile index")
            .expect("save_memory creates the current relation index");
        assert_eq!(indexed.0, full_memory.revision);
        assert_eq!(
            indexed.1,
            BTreeSet::from([
                rock.id.clone(),
                mineral_rule.id.clone(),
                positive.id.clone(),
                negative.id.clone(),
                archived.id.clone(),
            ])
        );
        let full_answer = relation_profile_answer(&full_memory, "Nera", 10_000, 16_384)
            .expect("full-snapshot profile reference");
        assert_eq!(indexed.2.expect("bounded indexed profile"), full_answer);
        assert!(
            matches!(
                relation_profile_answer(&full_memory, "Nera", 64, 16_384),
                Err(Error::Contract { code, .. }) if code == "RELATION_BUDGET"
            ),
            "the full-snapshot baseline must exceed the same action budget"
        );

        // Force the normal Core route through cold-index reconstruction.
        let connection = rusqlite::Connection::open(directory.join("state.sqlite3"))
            .expect("open derived index for cold-start setup");
        connection
            .execute("DELETE FROM memory_index_state WHERE actor_id='alice'", [])
            .expect("mark actor index cold");
        drop(connection);

        let mut request = RequestEnvelope::new(
            "active-profile-query-case",
            "active-profile-query-input",
            "alice",
            "Neraについて保存されている情報をもとに短い紹介文を書いて",
        );
        request.budget.max_actions = 64;
        let result = engine
            .execute("alice", request)
            .expect("run the ordinary Japanese DraftRelationProfile route");
        assert_eq!(result.lifecycle, Lifecycle::Commit, "{result:?}");
        assert_eq!(result.actor_memory_revision, Some(full_memory.revision));
        assert_eq!(result.guarantee, Guarantee::Hypothesis);
        assert_eq!(result.dependencies, full_answer.dependencies);
        assert_eq!(
            result
                .content_plan
                .as_ref()
                .expect("profile content plan")
                .required_units,
            full_answer.units
        );
        assert!(result.text.contains("紹介文案"));
        assert!(result.text.contains("archive"));
        assert!(result.text.contains("対立"));
        assert!(
            !result
                .dependencies
                .iter()
                .any(|dependency| dependency.contains("unrelated"))
        );

        let mut budget_request = RequestEnvelope::new(
            "active-profile-budget-case",
            "active-profile-budget-input",
            "alice",
            "Neraについて保存されている情報をもとに短い紹介文を書いて",
        );
        budget_request.budget.max_actions = 8;
        let budget_stop = engine
            .execute("alice", budget_request)
            .expect("suspend when subject candidates exceed the sealed budget");
        assert_eq!(budget_stop.lifecycle, Lifecycle::Suspend, "{budget_stop:?}");
        assert_eq!(
            budget_stop.actor_memory_revision,
            Some(full_memory.revision),
            "{budget_stop:?}"
        );
        assert!(
            budget_stop
                .reason
                .as_deref()
                .is_some_and(|reason| reason.starts_with("RESOURCE_LIMIT:"))
        );

        drop(engine);
        fs::remove_dir_all(directory).expect("remove isolated relation-profile store");
    }

    #[test]
    fn indexed_previous_relation_resolves_only_prior_dependencies_before_projection() {
        let serial = NEXT_TEMP_DIRECTORY.fetch_add(1, Ordering::Relaxed);
        let directory = std::env::temp_dir().join(format!(
            "minidora-active-previous-relation-test-{}-{serial}",
            std::process::id()
        ));
        fs::create_dir(&directory).expect("create isolated store directory");
        let engine = Engine::open(&directory).expect("open isolated engine");
        let source_request = RequestEnvelope::new(
            "active-previous-case",
            "active-previous-source",
            "alice",
            "保存関係",
        );
        let source_input = identity(&source_request).expect("source input fingerprint");
        engine
            .store
            .register_input("alice", &source_request, &source_input)
            .expect("register relation source");

        let mut full_memory = MemoryState::default();
        let anchor = relations::assert_fact(
            &mut full_memory,
            proposition("Nera", "is", "bird"),
            &source_input,
        )
        .expect("direct premise referenced by the prior result");
        let rule = relations::add_rule(
            &mut full_memory,
            proposition("$subject", "is", "bird"),
            proposition("$subject", "is", "mammal"),
            &source_input,
        )
        .expect("goal-relevant inference rule");
        for ordinal in 0..72 {
            relations::assert_fact(
                &mut full_memory,
                proposition(
                    &format!("unrelated-{ordinal}"),
                    &format!("unrelated-predicate-{ordinal}"),
                    &format!("unrelated-value-{ordinal}"),
                ),
                &source_input,
            )
            .expect("unrelated retained history");
        }
        full_memory.revision = 1;
        engine
            .store
            .save_memory("alice", &full_memory)
            .expect("persist actor memory and its derived index");

        let mut previous = base_result(&source_request);
        previous.lifecycle = Lifecycle::Commit;
        previous.dependencies = vec![anchor.id.clone()];
        let query = proposition("それ", "is", "mammal");
        let query_result = engine
            .active_previous_relation_query("alice", &previous, &query, None, 40)
            .expect("resolve the prior premise and run the bounded indexed query");
        let resolved = match query_result
            .resolution
            .expect("indexed query records its prior-premise resolution")
        {
            PreviousRelationResolution::Resolved(proposition) => proposition,
            PreviousRelationResolution::Missing => panic!("prior premise should resolve"),
            PreviousRelationResolution::Ambiguous => panic!("prior premise is unique"),
        };
        assert_eq!(resolved.subject, "Nera");
        assert_eq!(query_result.memory.revision, full_memory.revision);
        assert_eq!(query_result.memory.facts.len(), 1);
        assert!(
            query_result
                .memory
                .facts
                .iter()
                .any(|fact| fact.id == anchor.id)
        );
        assert_eq!(query_result.memory.rules.len(), 1);
        assert_eq!(query_result.memory.rules[0].id, rule.id);

        let answer = query_result
            .answer
            .expect("bounded relation query succeeds")
            .expect("resolved prior relation must run a proof query");
        assert_eq!(answer.status, "SUPPORTED");
        assert!(answer.steps <= 40);
        let full_answer = relations::query(&full_memory, &resolved, 40)
            .expect("run the full-snapshot reference query");
        assert_eq!(answer.status, full_answer.status);
        assert_eq!(
            relations::answer_dependencies(&answer),
            relations::answer_dependencies(&full_answer)
        );
        assert!(relations::answer_dependencies(&answer).contains(&anchor.id));
        assert!(relations::answer_dependencies(&answer).contains(&rule.id));

        let budget_stop = engine
            .active_previous_relation_query("alice", &previous, &query, None, 3)
            .expect("bounded stop retains the selected actor-memory revision");
        assert_eq!(budget_stop.memory.revision, full_memory.revision);
        assert!(matches!(
            budget_stop.answer,
            Err(Error::Contract { code, .. }) if code == "RELATION_BUDGET"
        ));

        drop(engine);
        fs::remove_dir_all(directory).expect("remove isolated test store");
    }

    #[test]
    fn indexed_relation_object_query_uses_only_the_goal_and_rule_closure() {
        let serial = NEXT_TEMP_DIRECTORY.fetch_add(1, Ordering::Relaxed);
        let directory = std::env::temp_dir().join(format!(
            "minidora-active-object-test-{}-{serial}",
            std::process::id()
        ));
        fs::create_dir(&directory).expect("create isolated store directory");
        let mut engine = Engine::open(&directory).expect("open isolated engine");
        let source_request = RequestEnvelope::new(
            "active-object-seed-case",
            "active-object-seed-input",
            "alice",
            "保存関係値",
        );
        let source_input = identity(&source_request).expect("source input fingerprint");
        engine
            .store
            .register_input("alice", &source_request, &source_input)
            .expect("register source input");

        let mut full_memory = MemoryState::default();
        let direct = relations::assert_fact(
            &mut full_memory,
            proposition("ミナ", "use", "はさみ"),
            &source_input,
        )
        .expect("direct relation object");
        let premise = relations::assert_fact(
            &mut full_memory,
            proposition("ミナ", "own", "端末"),
            &source_input,
        )
        .expect("rule premise for a second relation object");
        let rule = relations::add_rule(
            &mut full_memory,
            proposition("$subject", "own", "$object"),
            proposition("$subject", "use", "$object"),
            &source_input,
        )
        .expect("object-preserving relation rule");
        relations::assert_fact(
            &mut full_memory,
            proposition("サキ", "use", "バス"),
            &source_input,
        )
        .expect("same-scope other-subject control");
        let mut other_scope = proposition("ミナ", "use", "布");
        other_scope.scope = "archive".into();
        relations::assert_fact(&mut full_memory, other_scope, &source_input)
            .expect("out-of-scope control");
        for ordinal in 0..60 {
            relations::assert_fact(
                &mut full_memory,
                proposition(
                    &format!("unrelated-{ordinal}"),
                    &format!("unrelated-predicate-{ordinal}"),
                    &format!("unrelated-value-{ordinal}"),
                ),
                &source_input,
            )
            .expect("unrelated retained fact");
        }
        full_memory.revision = 1;
        engine
            .store
            .save_memory("alice", &full_memory)
            .expect("persist actor memory and its derived index");

        let full_answer =
            relation_objects_answer(&full_memory, "ミナ", "use", false, "lab", 256, 16 * 1024)
                .expect("full-snapshot reference answer");
        assert!(matches!(
            relation_objects_answer(
                &full_memory,
                "ミナ",
                "use",
                false,
                "lab",
                20,
                16 * 1024,
            ),
            Err(Error::Contract { code, .. }) if code == "RESOURCE_LIMIT"
        ));

        let connection = rusqlite::Connection::open(directory.join("state.sqlite3"))
            .expect("open derived index for cold-start setup");
        connection
            .execute("DELETE FROM memory_index_state WHERE actor_id='alice'", [])
            .expect("mark actor index cold");
        drop(connection);

        let mut request = RequestEnvelope::new(
            "active-object-query-case",
            "active-object-query-input",
            "alice",
            "[scope=lab]ミナは何を使いますか",
        );
        request.budget.max_actions = 20;
        let result = engine
            .execute("alice", request)
            .expect("run ordinary Japanese AskRelationObjects through Core");
        assert_eq!(result.lifecycle, Lifecycle::Commit, "{result:?}");
        assert_eq!(result.actor_memory_revision, Some(full_memory.revision));
        assert_eq!(result.text, full_answer.text);
        assert_eq!(result.guarantee, full_answer.guarantee);
        assert!(result.dependencies.contains(&direct.id));
        assert!(result.dependencies.contains(&premise.id));
        assert!(result.dependencies.contains(&rule.id));
        assert!(
            !result
                .dependencies
                .iter()
                .any(|id| id.contains("unrelated"))
        );

        let mut budget_limited_request = RequestEnvelope::new(
            "active-object-budget-case",
            "active-object-budget-input",
            "alice",
            "[scope=lab]ミナは何を使いますか",
        );
        budget_limited_request.budget.max_actions = 4;
        let budget_stopped = engine
            .execute("alice", budget_limited_request)
            .expect("suspend the relation-object query when projection exceeds its budget");
        assert_eq!(
            budget_stopped.lifecycle,
            Lifecycle::Suspend,
            "{budget_stopped:?}"
        );
        assert_eq!(
            budget_stopped.actor_memory_revision,
            Some(full_memory.revision),
            "{budget_stopped:?}"
        );
        assert!(
            budget_stopped
                .reason
                .as_deref()
                .is_some_and(|reason| reason.starts_with("RESOURCE_LIMIT:"))
        );

        let (active, indexed_answer) = engine
            .active_relation_objects_answer(
                "alice",
                RelationObjectsReplay {
                    subject: "ミナ",
                    predicate: "use",
                    negative: false,
                    scope: "lab",
                    max_actions: 20,
                    max_evidence_bytes: 16 * 1024,
                },
            )
            .expect("run warm indexed object query");
        let indexed_answer = indexed_answer.expect("finish indexed object query");
        assert_eq!(active.revision, full_memory.revision);
        assert_eq!(active.facts.len(), 2);
        assert_eq!(active.rules.len(), 1);
        assert_eq!(indexed_answer.text, full_answer.text);
        assert_eq!(indexed_answer.dependencies, full_answer.dependencies);
        assert_eq!(indexed_answer.guarantee, full_answer.guarantee);

        let mut next = full_memory.clone();
        next.revision = 2;
        for fact_id in [&direct.id, &premise.id] {
            next.facts
                .iter_mut()
                .find(|fact| fact.id == *fact_id)
                .expect("fact remains in immutable history")
                .valid = false;
        }
        next.rules
            .iter_mut()
            .find(|current| current.id == rule.id)
            .expect("rule remains in immutable history")
            .valid = false;
        engine
            .store
            .save_memory("alice", &next)
            .expect("incrementally tombstone the relation proof closure");
        let mut after_invalidation = RequestEnvelope::new(
            "active-object-after-invalidation-case",
            "active-object-after-invalidation-input",
            "alice",
            "[scope=lab]ミナは何を使いますか",
        );
        after_invalidation.budget.max_actions = 20;
        let stopped = engine
            .execute("alice", after_invalidation)
            .expect("requery the invalidated object relations");
        assert_eq!(stopped.lifecycle, Lifecycle::Suspend, "{stopped:?}");
        assert_eq!(stopped.actor_memory_revision, Some(next.revision));
        assert!(!stopped.dependencies.contains(&direct.id));
        assert!(!stopped.dependencies.contains(&premise.id));
        assert!(!stopped.dependencies.contains(&rule.id));
        assert!(!stopped.text.contains("はさみ"));
        assert!(!stopped.text.contains("端末"));

        drop(engine);
        fs::remove_dir_all(directory).expect("remove isolated test store");
    }

    #[test]
    fn indexed_relation_comparison_selects_only_the_requested_scope_on_the_normal_route() {
        let serial = NEXT_TEMP_DIRECTORY.fetch_add(1, Ordering::Relaxed);
        let directory = std::env::temp_dir().join(format!(
            "minidora-active-relation-comparison-test-{}-{serial}",
            std::process::id()
        ));
        fs::create_dir(&directory).expect("create isolated store directory");
        let mut engine = Engine::open(&directory).expect("open isolated engine");
        let source_request =
            RequestEnvelope::new("active-comparison-seed-case", "seed", "alice", "保存関係");
        let source_input = identity(&source_request).expect("source input fingerprint");
        engine
            .store
            .register_input("alice", &source_request, &source_input)
            .expect("register comparison source");

        let mut full_memory = MemoryState::default();
        let nera_shared = relations::assert_fact(
            &mut full_memory,
            proposition("Nera", "likes", "tea"),
            &source_input,
        )
        .expect("Nera shared relation");
        let nera_unique = relations::assert_fact(
            &mut full_memory,
            proposition("Nera", "owns", "boat"),
            &source_input,
        )
        .expect("Nera one-sided relation");
        let miko_shared = relations::assert_fact(
            &mut full_memory,
            proposition("Miko", "likes", "tea"),
            &source_input,
        )
        .expect("Miko shared relation");
        let miko_unique = relations::assert_fact(
            &mut full_memory,
            proposition("Miko", "owns", "cup"),
            &source_input,
        )
        .expect("Miko one-sided relation");
        for ordinal in 0..80 {
            relations::assert_fact(
                &mut full_memory,
                Proposition {
                    scope: format!("archive-{ordinal}"),
                    ..proposition(
                        &format!("unrelated-{ordinal}"),
                        "is",
                        &format!("unrelated-{ordinal}"),
                    )
                },
                &source_input,
            )
            .expect("off-scope retained history");
        }
        full_memory.revision = 1;
        engine
            .store
            .save_memory("alice", &full_memory)
            .expect("persist actor memory and its derived index");

        let full_inventory_a = relation_comparison_inventory(&full_memory, "Nera", "lab", 10_000)
            .expect("full-snapshot Nera comparison reference");
        let full_inventory_b = relation_comparison_inventory(&full_memory, "Miko", "lab", 10_000)
            .expect("full-snapshot Miko comparison reference");
        assert!(matches!(
            relation_comparison_inventory(&full_memory, "Nera", "lab", 64),
            Err(Error::Contract { code, .. }) if code == "RELATION_BUDGET"
        ));

        let indexed = engine
            .store
            .with_relation_index("alice", |index| {
                let candidates = index.relation_scope_candidates("lab", 8)?;
                let candidate_ids = candidates
                    .into_iter()
                    .map(|record| record.value.id().to_owned())
                    .collect::<BTreeSet<_>>();
                let inventory_a =
                    relation_comparison_inventory_from_index(index, "Nera", "lab", 64)?;
                let inventory_b =
                    relation_comparison_inventory_from_index(index, "Miko", "lab", 64)?;
                Ok((index.revision(), candidate_ids, inventory_a, inventory_b))
            })
            .expect("read scoped relation index")
            .expect("save_memory creates the current relation index");
        assert_eq!(indexed.0, full_memory.revision);
        assert_eq!(
            indexed.1,
            BTreeSet::from([
                nera_shared.id.clone(),
                nera_unique.id.clone(),
                miko_shared.id.clone(),
                miko_unique.id.clone(),
            ])
        );
        assert_eq!(indexed.2.0, full_inventory_a);
        assert_eq!(indexed.3.0, full_inventory_b);

        // Force the typed Core comparison through its cold-index rebuild path.
        let connection = rusqlite::Connection::open(directory.join("state.sqlite3"))
            .expect("open derived index for cold-start setup");
        connection
            .execute("DELETE FROM memory_index_state WHERE actor_id='alice'", [])
            .expect("mark actor index cold");
        drop(connection);

        let mut request =
            RequestEnvelope::new("active-comparison-case", "compare-input", "alice", "");
        request.budget.max_actions = 128;
        let result = engine
            .compose_atomic_relation_comparison("alice", request, "Nera", "Miko", "lab")
            .expect("run ordinary typed comparison through Core");
        assert_eq!(result.lifecycle, Lifecycle::Commit, "{result:?}");
        assert_eq!(result.actor_memory_revision, Some(full_memory.revision));
        let plan = result
            .content_plan
            .as_ref()
            .expect("Core adopted the relation comparison");
        assert!(plan.text.contains("likes"), "{}", plan.text);
        assert!(plan.text.contains("owns"), "{}", plan.text);
        assert!(result.dependencies.contains(&nera_shared.id));
        assert!(result.dependencies.contains(&nera_unique.id));
        assert!(result.dependencies.contains(&miko_shared.id));
        assert!(result.dependencies.contains(&miko_unique.id));

        drop(engine);
        fs::remove_dir_all(directory).expect("remove isolated comparison store");
    }
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
            actor_memory_revision: None,
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
                    temporal_scope: None,
                    condition_refs: Default::default(),
                },
                proof_dependencies: vec!["legacy-direct-premise".into()],
            }
        );

        drop(store);
        fs::remove_dir_all(directory).expect("remove isolated legacy relation store");
    }
}

#[cfg(test)]
mod core_construction_observation_tests {
    use super::*;
    use std::collections::BTreeSet;
    use std::fs;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT_TEMP_DIRECTORY: AtomicU64 = AtomicU64::new(0);

    fn execute_observation(
        engine: &mut Engine,
        actor: &str,
        case: &str,
        input: &str,
        text: &str,
    ) -> ResultRecord {
        engine
            .execute(actor, RequestEnvelope::new(case, input, actor, text))
            .unwrap_or_else(|error| panic!("Engine::execute failed for {input}: {error}"))
    }

    #[test]
    fn direct_english_assertion_source_roles_follow_the_fact_into_reopened_proofs() {
        let serial = NEXT_TEMP_DIRECTORY.fetch_add(1, Ordering::Relaxed);
        let directory = std::env::temp_dir().join(format!(
            "minidora-english-assertion-source-proof-{}-{serial}",
            std::process::id()
        ));
        fs::create_dir(&directory).expect("create isolated English assertion provenance state");
        let actor = "english-assertion-source-proof-actor";
        let assertion_request = RequestEnvelope::new(
            "english-assertion-source-proof-case",
            "english-assertion-source-proof-input",
            actor,
            "[scope=biology]Coco is a bird",
        );
        let mut engine = Engine::open(&directory).expect("open Core");
        let assertion = engine
            .execute(actor, assertion_request.clone())
            .expect("execute sealed English assertion");
        assert_eq!(assertion.lifecycle, Lifecycle::Commit, "{assertion:?}");
        assert_eq!(assertion.guarantee, Guarantee::UserPremise);
        let assertion_case = engine
            .latest(actor, &assertion_request.case_id)
            .expect("load English assertion case")
            .expect("English assertion case exists");
        let assertion_bundle = assertion_case
            .bundle
            .as_ref()
            .expect("English assertion bundle is sealed");
        let expected_evidence = input::assertion_source_evidence(assertion_bundle)
            .expect("build sealed English source evidence")
            .expect("English copular roles are complete and unambiguous");
        assert_eq!(expected_evidence.parser_version, input::PARSER_VERSION);
        for (role, expected) in [
            ("/Assert/proposition/scope", "biology"),
            ("/Assert/proposition/subject", "Coco"),
            ("/Assert/proposition/predicate", "is"),
            ("/Assert/proposition/article", "a"),
            ("/Assert/proposition/object", "bird"),
        ] {
            let spans = expected_evidence
                .source_roles
                .get(role)
                .unwrap_or_else(|| panic!("missing exact English role {role}"));
            assert_eq!(spans.len(), 1, "ambiguous source role {role}");
            let span = &spans[0];
            assert_eq!(span.text, expected, "wrong source value for {role}");
            assert_eq!(
                assertion_request.text.get(span.start..span.end),
                Some(expected),
                "role {role} does not point into the original UTF-8 request"
            );
        }

        let mut incomplete_bundle = assertion_bundle.clone();
        incomplete_bundle
            .meaning
            .source_roles
            .remove("/Assert/proposition/article");
        assert!(
            input::assertion_source_evidence(&incomplete_bundle)
                .expect("validate missing article role")
                .is_none(),
            "incomplete English source roles are not promoted into Fact evidence"
        );

        let fact_id = assertion.dependencies[0].clone();
        let memory = engine
            .store
            .load_memory(actor)
            .expect("load Core memory after English assertion");
        let premise = memory
            .facts
            .iter()
            .find(|fact| fact.id == fact_id)
            .expect("English assertion Fact is in Core memory");
        assert_eq!(premise.source_evidence, Some(expected_evidence.clone()));
        let proposition = premise.proposition.clone();

        let negative_request = RequestEnvelope::new(
            "english-assertion-source-proof-negative-case",
            "english-assertion-source-proof-negative-input",
            actor,
            "Mice are not mammals",
        );
        let negative_result = engine
            .execute(actor, negative_request.clone())
            .expect("execute English negative assertion with are");
        assert_eq!(negative_result.lifecycle, Lifecycle::Commit);
        let negative_case = engine
            .latest(actor, &negative_request.case_id)
            .expect("load negative English case")
            .expect("negative English case exists");
        let negative_bundle = negative_case
            .bundle
            .as_ref()
            .expect("sealed negative input");
        let negative_evidence = input::assertion_source_evidence(negative_bundle)
            .expect("build negative English source evidence")
            .expect("are/not roles are exact");
        assert_eq!(
            negative_evidence.source_roles["/Assert/proposition/predicate"][0].text,
            "are"
        );
        assert_eq!(
            negative_evidence.source_roles["/Assert/proposition/negation"][0].text,
            "not"
        );
        let negative_fact = engine
            .store
            .load_memory(actor)
            .expect("load memory with negative English assertion")
            .facts
            .into_iter()
            .find(|fact| negative_result.dependencies.contains(&fact.id))
            .expect("negative assertion Fact is present");
        assert_eq!(
            negative_fact.source_evidence,
            Some(negative_evidence.clone())
        );

        drop(engine);
        let mut reopened = Engine::open(&directory).expect("reopen Core state");
        let restored_memory = reopened
            .store
            .load_memory(actor)
            .expect("restore actor memory");
        let restored_premise = restored_memory
            .facts
            .iter()
            .find(|fact| fact.id == fact_id)
            .expect("restore English source-backed premise");
        assert_eq!(
            restored_premise.source_evidence,
            Some(expected_evidence.clone()),
            "the reopened memory retains exact English source spans"
        );
        let restored_negative = restored_memory
            .facts
            .iter()
            .find(|fact| negative_result.dependencies.contains(&fact.id))
            .expect("restore negative English source-backed premise");
        assert_eq!(
            restored_negative.source_evidence,
            Some(negative_evidence.clone()),
            "the reopened memory retains are/not source spans"
        );
        let proof = relations::query(&restored_memory, &proposition, 32)
            .expect("query English source-backed premise");
        assert_eq!(proof.status, "SUPPORTED");
        assert_eq!(proof.facts.len(), 1);
        assert_eq!(
            proof.facts[0].source_evidence,
            Some(expected_evidence.clone())
        );
        assert!(relations::answer_dependencies(&proof).contains(&fact_id));

        let ask = RequestEnvelope::new(
            "english-assertion-source-proof-case",
            "english-assertion-source-proof-followup",
            actor,
            "[scope=biology]Coco is a bird?",
        );
        let answer = reopened
            .execute(actor, ask)
            .expect("Core consumes the restored premise through its normal Ask route");
        assert_eq!(answer.lifecycle, Lifecycle::Commit);
        assert_eq!(answer.guarantee, Guarantee::UserPremise);
        assert!(answer.dependencies.contains(&fact_id));

        println!(
            "B30_ENGLISH_ASSERTION_SOURCE_PROOF={}",
            serde_json::to_string(&serde_json::json!({
                "source": assertion_request.text,
                "parser_version": expected_evidence.parser_version,
                "raw_sha256": expected_evidence.raw_sha256,
                "semantic_signature": expected_evidence.semantic_signature,
                "source_roles": expected_evidence.source_roles,
                "fact_id": fact_id,
                "assertion_lifecycle": assertion.lifecycle,
                "article_role_required": true,
                "incomplete_role_map_not_projected": true,
                "negative_are_source_roles": negative_evidence.source_roles,
                "memory_reopened_with_same_source_evidence": true,
                "query_proof_returns_source_evidence": true,
                "normal_core_followup_consumes_fact": answer.dependencies.contains(&fact_id),
                "outside_world_truth_verified": false,
                "external_network_or_effects": false
            }))
            .expect("serialize B30 English assertion source proof observation")
        );
        drop(reopened);
        fs::remove_dir_all(directory).expect("remove isolated English assertion provenance state");
    }

    #[test]
    fn direct_assertion_source_roles_follow_the_fact_into_reopened_proofs() {
        let serial = NEXT_TEMP_DIRECTORY.fetch_add(1, Ordering::Relaxed);
        let directory = std::env::temp_dir().join(format!(
            "minidora-assertion-source-proof-{}-{serial}",
            std::process::id()
        ));
        fs::create_dir(&directory).expect("create isolated assertion provenance state");
        let actor = "assertion-source-proof-actor";
        let assertion_request = RequestEnvelope::new(
            "assertion-source-proof-case",
            "assertion-source-proof-input",
            actor,
            "[scope=ｶﾞ]ｶﾞはｶﾞではない",
        );
        let mut engine = Engine::open(&directory).expect("open Core");
        let assertion = engine
            .execute(actor, assertion_request.clone())
            .expect("execute sealed assertion");
        assert_eq!(assertion.lifecycle, Lifecycle::Commit, "{assertion:?}");
        assert_eq!(assertion.guarantee, Guarantee::UserPremise);
        let assertion_case = engine
            .latest(actor, &assertion_request.case_id)
            .expect("load assertion case")
            .expect("assertion case exists");
        let assertion_bundle = assertion_case
            .bundle
            .as_ref()
            .expect("assertion bundle is sealed");
        let expected_evidence = input::assertion_source_evidence(assertion_bundle)
            .expect("build sealed source evidence")
            .expect("copular role spans are complete and unambiguous");
        assert_eq!(expected_evidence.raw_sha256, assertion_bundle.raw_sha256);
        assert_eq!(
            expected_evidence.bundle_fingerprint,
            assertion_bundle.fingerprint
        );
        assert_eq!(
            expected_evidence.input_fingerprint,
            identity(&assertion_bundle.request).expect("registered request fingerprint")
        );
        let fact_id = assertion.dependencies[0].clone();
        let memory = engine
            .store
            .load_memory(actor)
            .expect("load Core memory after assertion");
        let premise = memory
            .facts
            .iter()
            .find(|fact| fact.id == fact_id)
            .expect("assertion fact is in Core memory");
        assert_eq!(premise.source_evidence, Some(expected_evidence.clone()));
        let mut tampered_memory = memory.clone();
        tampered_memory
            .facts
            .iter_mut()
            .find(|fact| fact.id == fact_id)
            .and_then(|fact| fact.source_evidence.as_mut())
            .expect("direct fact source evidence")
            .raw_sha256 = "0".repeat(64);
        assert!(
            engine.store.save_memory(actor, &tampered_memory).is_err(),
            "store must reject source evidence that differs from its sealed input"
        );
        assert_eq!(
            engine.store.load_memory(actor).unwrap(),
            memory,
            "rejected source tampering leaves the retained memory unchanged"
        );

        let mut ambiguous_bundle = assertion_bundle.clone();
        let subject_spans = ambiguous_bundle
            .meaning
            .source_roles
            .get_mut("/Assert/proposition/subject")
            .expect("subject role exists");
        subject_spans.push(subject_spans[0].clone());
        assert!(
            input::assertion_source_evidence(&ambiguous_bundle)
                .expect("validate ambiguous evidence")
                .is_none(),
            "ambiguous role attribution is not promoted into fact evidence"
        );
        let mut swapped_bundle = assertion_bundle.clone();
        let subject_span = swapped_bundle
            .meaning
            .source_roles
            .get("/Assert/proposition/subject")
            .expect("subject role exists")[0]
            .clone();
        let object_span = swapped_bundle
            .meaning
            .source_roles
            .get("/Assert/proposition/object")
            .expect("object role exists")[0]
            .clone();
        swapped_bundle
            .meaning
            .source_roles
            .insert("/Assert/proposition/subject".into(), vec![object_span]);
        swapped_bundle
            .meaning
            .source_roles
            .insert("/Assert/proposition/object".into(), vec![subject_span]);
        assert!(
            input::assertion_source_evidence(&swapped_bundle)
                .expect("validate swapped evidence")
                .is_none(),
            "equal normalized values do not allow role positions to be swapped"
        );
        let mut missing_scope_bundle = assertion_bundle.clone();
        missing_scope_bundle
            .meaning
            .source_roles
            .remove("/Assert/proposition/scope");
        assert!(
            input::assertion_source_evidence(&missing_scope_bundle)
                .expect("validate missing explicit-scope evidence")
                .is_none(),
            "an explicit scope marker without its source span is not promoted"
        );

        let implicit_request = RequestEnvelope::new(
            "assertion-source-proof-implicit-scope",
            "assertion-source-proof-implicit-input",
            actor,
            "defaultはdefaultではない",
        );
        let implicit_result = engine
            .execute(actor, implicit_request.clone())
            .expect("execute implicit-default direct assertion");
        assert_eq!(implicit_result.lifecycle, Lifecycle::Commit);
        let implicit_case = engine
            .latest(actor, &implicit_request.case_id)
            .expect("load implicit-default case")
            .expect("implicit-default case exists");
        let implicit_bundle = implicit_case.bundle.as_ref().expect("sealed input");
        let implicit_evidence = input::assertion_source_evidence(implicit_bundle)
            .expect("build implicit-default source evidence")
            .expect("implicit default assertion still has attributable lexical roles");
        assert!(
            !implicit_evidence
                .source_roles
                .contains_key("/Assert/proposition/scope"),
            "a generated default scope has no source location"
        );
        let implicit_fact = engine
            .store
            .load_memory(actor)
            .expect("load memory with implicit-default assertion")
            .facts
            .into_iter()
            .find(|fact| implicit_result.dependencies.contains(&fact.id))
            .expect("implicit assertion fact is present");
        assert_eq!(
            implicit_fact.source_evidence,
            Some(implicit_evidence.clone())
        );

        drop(engine);
        let mut reopened = Engine::open(&directory).expect("reopen Core state");
        let restored_memory = reopened
            .store
            .load_memory(actor)
            .expect("restore actor memory");
        let restored_premise = restored_memory
            .facts
            .iter()
            .find(|fact| fact.id == fact_id)
            .expect("restore source-backed premise");
        assert_eq!(
            restored_premise.source_evidence,
            Some(expected_evidence.clone()),
            "the memory revision retains sealed role spans after reopen"
        );
        let proposition = restored_premise.proposition.clone();
        let proof = relations::query(&restored_memory, &proposition, 32)
            .expect("query the source-backed premise");
        assert_eq!(proof.status, "SUPPORTED");
        assert_eq!(proof.facts.len(), 1);
        assert_eq!(
            proof.facts[0].source_evidence,
            Some(expected_evidence.clone())
        );
        assert!(relations::answer_dependencies(&proof).contains(&fact_id));

        let ask = RequestEnvelope::new(
            "assertion-source-proof-case",
            "assertion-source-proof-followup",
            actor,
            "[scope=ｶﾞ]ｶﾞはｶﾞではない?",
        );
        let answer = reopened
            .execute(actor, ask)
            .expect("Core consumes the restored premise through the normal query route");
        assert_eq!(answer.lifecycle, Lifecycle::Commit);
        assert_eq!(answer.guarantee, Guarantee::UserPremise);
        assert!(answer.dependencies.contains(&fact_id));
        assert_eq!(
            reopened.execute(actor, implicit_request).unwrap(),
            implicit_result,
            "implicit-default result replays without reinterpreting its proof fact"
        );

        println!(
            "B23_ASSERTION_SOURCE_PROOF={}",
            serde_json::to_string(&serde_json::json!({
                "source": assertion_request.text,
                "parser_version": expected_evidence.parser_version,
                "bundle_fingerprint": expected_evidence.input_fingerprint,
                "raw_sha256": expected_evidence.raw_sha256,
                "semantic_signature": expected_evidence.semantic_signature,
                "source_roles": expected_evidence.source_roles,
                "implicit_default_source_roles": implicit_evidence.source_roles,
                "implicit_default_has_no_scope_span": !implicit_evidence.source_roles.contains_key("/Assert/proposition/scope"),
                "request_fingerprint": expected_evidence.input_fingerprint,
                "assertion_result_text": assertion.text,
                "follow_up_question": "[scope=ｶﾞ]ｶﾞはｶﾞではない?",
                "follow_up_result_text": answer.text,
                "fact_id": fact_id,
                "assertion_lifecycle": assertion.lifecycle,
                "tampered_raw_digest_rejected_without_memory_change": true,
                "duplicated_role_map_not_projected": true,
                "swapped_equal_value_roles_not_projected": true,
                "explicit_scope_without_source_span_not_projected": true,
                "legacy_fact_json_compatibility_pass": true,
                "memory_reopened_with_same_source_evidence": true,
                "query_proof_returns_source_evidence": true,
                "normal_core_followup_consumes_fact": answer.dependencies.contains(&fact_id),
                "source_roles_are_themselves_world_truth": false,
                "external_network_or_effects": false
            }))
            .expect("serialize B23 assertion source proof observation")
        );
        drop(reopened);
        fs::remove_dir_all(directory).expect("remove isolated assertion provenance state");
    }

    #[test]
    fn direct_transitive_assertion_source_roles_follow_the_fact_into_reopened_proofs() {
        let serial = NEXT_TEMP_DIRECTORY.fetch_add(1, Ordering::Relaxed);
        let directory = std::env::temp_dir().join(format!(
            "minidora-core-transitive-source-proof-{}-{serial}",
            std::process::id()
        ));
        fs::create_dir(&directory).expect("unique transitive-source state directory");
        let actor = "transitive-source-proof-actor";
        let assertion_request = RequestEnvelope::new(
            "transitive-source-proof-case",
            "transitive-source-proof-input",
            actor,
            "[scope=biology]ミナは毒を食べません。",
        );
        let mut engine = Engine::open(&directory).expect("open Core");
        let assertion = engine
            .execute(actor, assertion_request.clone())
            .expect("execute sealed Japanese transitive assertion");
        assert_eq!(assertion.lifecycle, Lifecycle::Commit, "{assertion:?}");
        assert_eq!(assertion.guarantee, Guarantee::UserPremise);
        let assertion_case = engine
            .latest(actor, &assertion_request.case_id)
            .expect("load assertion case")
            .expect("assertion case exists");
        let assertion_bundle = assertion_case
            .bundle
            .as_ref()
            .expect("assertion bundle is sealed");
        let expected_evidence = input::assertion_source_evidence(assertion_bundle)
            .expect("build sealed transitive source evidence")
            .expect("transitive source roles are complete and unambiguous");
        assert_eq!(
            expected_evidence.parser_version,
            input::PARSER_VERSION,
            "the new role projection has an explicit parser revision"
        );
        for (role, expected) in [
            ("/Assert/proposition/scope", "biology"),
            ("/Assert/proposition/subject", "ミナ"),
            ("/Assert/proposition/subject_particle", "は"),
            ("/Assert/proposition/object", "毒"),
            ("/Assert/proposition/object_particle", "を"),
            ("/Assert/proposition/predicate", "食べません"),
        ] {
            let spans = expected_evidence
                .source_roles
                .get(role)
                .unwrap_or_else(|| panic!("missing source role {role}"));
            assert_eq!(spans.len(), 1, "ambiguous source role {role}");
            assert_eq!(spans[0].text, expected, "wrong source role {role}");
            assert_eq!(
                assertion_request.text.get(spans[0].start..spans[0].end),
                Some(expected),
                "source role {role} does not point into original UTF-8 bytes"
            );
        }

        let fact_id = assertion.dependencies[0].clone();
        let memory = engine
            .store
            .load_memory(actor)
            .expect("load Core memory after assertion");
        let premise = memory
            .facts
            .iter()
            .find(|fact| fact.id == fact_id)
            .expect("transitive assertion fact is in Core memory");
        assert_eq!(premise.source_evidence, Some(expected_evidence.clone()));
        assert!(premise.proposition.negative);

        let mut ambiguous_bundle = assertion_bundle.clone();
        let duplicate_predicate_span =
            ambiguous_bundle.meaning.source_roles["/Assert/proposition/predicate"][0].clone();
        ambiguous_bundle
            .meaning
            .source_roles
            .get_mut("/Assert/proposition/predicate")
            .expect("predicate source role exists")
            .push(duplicate_predicate_span);
        assert!(
            input::assertion_source_evidence(&ambiguous_bundle)
                .expect("validate ambiguous transitive source evidence")
                .is_none(),
            "duplicated inflected-predicate spans are not projected"
        );

        drop(engine);
        let mut reopened = Engine::open(&directory).expect("reopen Core state");
        let restored_memory = reopened
            .store
            .load_memory(actor)
            .expect("restore actor memory");
        let restored_premise = restored_memory
            .facts
            .iter()
            .find(|fact| fact.id == fact_id)
            .expect("restore source-backed transitive premise");
        assert_eq!(
            restored_premise.source_evidence,
            Some(expected_evidence.clone()),
            "the memory revision retains transitive source roles after reopen"
        );
        let proof = relations::query(&restored_memory, &restored_premise.proposition, 32)
            .expect("query the source-backed transitive premise");
        assert_eq!(proof.status, "SUPPORTED");
        assert_eq!(proof.facts.len(), 1);
        assert_eq!(
            proof.facts[0].source_evidence,
            Some(expected_evidence.clone())
        );
        assert!(relations::answer_dependencies(&proof).contains(&fact_id));

        let ask = RequestEnvelope::new(
            "transitive-source-proof-case",
            "transitive-source-proof-followup",
            actor,
            "[scope=biology]ミナは毒を食べませんか",
        );
        let answer = reopened
            .execute(actor, ask)
            .expect("Core consumes the restored premise through the normal relation query");
        assert_eq!(answer.lifecycle, Lifecycle::Commit);
        assert_eq!(answer.guarantee, Guarantee::UserPremise);
        assert!(answer.dependencies.contains(&fact_id));

        println!(
            "B24_TRANSITIVE_ASSERTION_SOURCE_PROOF={}",
            serde_json::to_string(&serde_json::json!({
                "source": assertion_request.text,
                "parser_version": expected_evidence.parser_version,
                "bundle_fingerprint": expected_evidence.bundle_fingerprint,
                "raw_sha256": expected_evidence.raw_sha256,
                "semantic_signature": expected_evidence.semantic_signature,
                "source_roles": expected_evidence.source_roles,
                "assertion_result_text": assertion.text,
                "fact_id": &fact_id,
                "reopened_proof_status": proof.status,
                "follow_up_question": "[scope=biology]ミナは毒を食べませんか",
                "follow_up_result_text": answer.text,
                "follow_up_dependencies": &answer.dependencies,
                "negative_verb_polarity_preserved": restored_premise.proposition.negative,
                "duplicated_predicate_role_not_projected": true,
                "memory_reopened_with_same_source_evidence": true,
                "query_proof_returns_source_evidence": true,
                "normal_core_followup_consumes_fact": answer.dependencies.contains(&fact_id),
                "source_roles_are_themselves_world_truth": false,
                "external_network_or_effects": false
            }))
            .expect("serialize B24 transitive assertion source proof observation")
        );
        drop(reopened);
        fs::remove_dir_all(directory).expect("remove isolated transitive-source state");
    }

    #[test]
    fn contextual_japanese_assertion_source_roles_follow_the_fact_into_reopened_proofs() {
        let serial = NEXT_TEMP_DIRECTORY.fetch_add(1, Ordering::Relaxed);
        let directory = std::env::temp_dir().join(format!(
            "minidora-core-contextual-assertion-source-proof-{}-{serial}",
            std::process::id()
        ));
        fs::create_dir(&directory).expect("unique contextual-source state directory");
        let actor = "contextual-source-proof-actor";
        let case = "contextual-source-proof-case";
        let mut engine = Engine::open(&directory).expect("open Core");

        let assertion_request = RequestEnvelope::new(
            case,
            "context-anchor",
            actor,
            "[scope=biology]ミナは毒を食べません。",
        );
        let assertion = engine
            .execute(actor, assertion_request)
            .expect("execute direct Japanese relation assertion");
        assert_eq!(assertion.lifecycle, Lifecycle::Commit, "{assertion:?}");

        let anchor_request = RequestEnvelope::new(
            case,
            "context-anchor-query",
            actor,
            "[scope=biology]ミナは毒を食べませんか",
        );
        let anchor = engine
            .execute(actor, anchor_request)
            .expect("execute actual relation query used as the contextual anchor");
        assert_eq!(anchor.lifecycle, Lifecycle::Commit, "{anchor:?}");
        assert!(
            assertion
                .dependencies
                .iter()
                .any(|dependency| anchor.dependencies.contains(dependency))
        );

        let contextual_request = RequestEnvelope::new(
            case,
            "contextual-assertion",
            actor,
            "[scope=biology]それは水を使う。",
        );
        let contextual = engine
            .execute(actor, contextual_request.clone())
            .expect("execute context-restored Japanese assertion");
        assert_eq!(contextual.lifecycle, Lifecycle::Commit, "{contextual:?}");
        assert_eq!(contextual.guarantee, Guarantee::UserPremise);
        assert!(contextual.dependencies.contains(&anchor.id));

        let contextual_state = engine
            .latest(actor, case)
            .expect("read contextual assertion case")
            .expect("contextual assertion state exists");
        let contextual_bundle = contextual_state
            .bundle
            .as_ref()
            .expect("contextual assertion bundle is sealed");
        assert_eq!(
            contextual_bundle.meaning.context_result_ref.as_deref(),
            Some(anchor.id.as_str()),
            "the source bundle binds the pronoun to the actual preceding result"
        );
        assert!(matches!(
            contextual_bundle.meaning.intent,
            Intent::AssertPreviousRelation { .. }
        ));
        let expected_evidence = input::assertion_source_evidence(contextual_bundle)
            .expect("validate contextual source evidence")
            .expect("supported Japanese contextual assertion has exact source roles");
        assert_eq!(expected_evidence.raw_sha256, contextual_bundle.raw_sha256);
        assert_eq!(
            expected_evidence.bundle_fingerprint,
            contextual_bundle.fingerprint
        );
        assert_eq!(
            expected_evidence.input_fingerprint,
            identity(&contextual_bundle.request).expect("context request fingerprint")
        );

        let memory = engine
            .store
            .load_memory(actor)
            .expect("load Core memory after contextual assertion");
        let premise = memory
            .facts
            .iter()
            .find(|fact| fact.source_evidence.as_ref() == Some(&expected_evidence))
            .expect("contextual USER_PREMISE fact is in Core memory");
        let contextual_fact_id = premise.id.clone();
        assert!(contextual.dependencies.contains(&contextual_fact_id));
        assert_eq!(premise.proposition.subject, "ミナ");
        assert_eq!(premise.proposition.object, "水");
        assert_eq!(premise.proposition.scope, "biology");
        assert_eq!(premise.source_evidence, Some(expected_evidence.clone()));
        for (role, expected) in [
            ("/AssertPreviousRelation/proposition/subject", "それ"),
            ("/AssertPreviousRelation/proposition/subject_particle", "は"),
            ("/AssertPreviousRelation/proposition/object", "水"),
            ("/AssertPreviousRelation/proposition/object_particle", "を"),
            ("/AssertPreviousRelation/proposition/predicate", "使う"),
            ("/AssertPreviousRelation/proposition/scope", "biology"),
        ] {
            let spans = expected_evidence
                .source_roles
                .get(role)
                .unwrap_or_else(|| panic!("missing contextual source role {role}"));
            assert_eq!(spans.len(), 1, "ambiguous contextual source role {role}");
            assert_eq!(spans[0].text, expected, "wrong source role {role}");
            assert_eq!(
                contextual_request.text.get(spans[0].start..spans[0].end),
                Some(expected),
                "contextual role {role} does not point into original UTF-8 bytes"
            );
        }

        let mut ambiguous_bundle = contextual_bundle.clone();
        let duplicate_predicate_span =
            ambiguous_bundle.meaning.source_roles["/AssertPreviousRelation/proposition/predicate"]
                [0]
            .clone();
        let predicate_spans = ambiguous_bundle
            .meaning
            .source_roles
            .get_mut("/AssertPreviousRelation/proposition/predicate")
            .expect("contextual predicate role exists");
        predicate_spans.push(duplicate_predicate_span);
        assert!(
            input::assertion_source_evidence(&ambiguous_bundle)
                .expect("validate duplicated contextual role")
                .is_none(),
            "a duplicated contextual role is not promoted into fact evidence"
        );

        drop(engine);
        let mut reopened = Engine::open(&directory).expect("reopen Core state");
        let restored_memory = reopened
            .store
            .load_memory(actor)
            .expect("restore actor memory and revalidate sealed provenance");
        let restored_premise = restored_memory
            .facts
            .iter()
            .find(|fact| fact.id == contextual_fact_id)
            .expect("restore contextual source-backed premise");
        assert_eq!(
            restored_premise.source_evidence,
            Some(expected_evidence.clone()),
            "the source bundle and context result link remain after reopen"
        );
        let proof = relations::query(&restored_memory, &restored_premise.proposition, 32)
            .expect("query the contextual source-backed premise");
        assert_eq!(proof.status, "SUPPORTED");
        assert_eq!(proof.facts.len(), 1);
        assert_eq!(
            proof.facts[0].source_evidence,
            Some(expected_evidence.clone())
        );
        assert!(relations::answer_dependencies(&proof).contains(&contextual_fact_id));

        let followup = RequestEnvelope::new(
            case,
            "contextual-followup",
            actor,
            "[scope=biology]ミナは水を使う?",
        );
        let answer = reopened
            .execute(actor, followup)
            .expect("normal Core query consumes the reopened contextual premise");
        assert_eq!(answer.lifecycle, Lifecycle::Commit);
        assert_eq!(answer.guarantee, Guarantee::UserPremise);
        assert!(answer.dependencies.contains(&contextual_fact_id));

        let copular_case = "contextual-copular-source-proof-case";
        let copular_seed = reopened
            .execute(
                actor,
                RequestEnvelope::new(
                    copular_case,
                    "copular-context-seed",
                    actor,
                    "[scope=biology]ミナは鳥です。",
                ),
            )
            .expect("execute separate copular context seed");
        assert_eq!(copular_seed.lifecycle, Lifecycle::Commit);
        let copular_anchor = reopened
            .execute(
                actor,
                RequestEnvelope::new(
                    copular_case,
                    "copular-context-anchor",
                    actor,
                    "[scope=biology]ミナは鳥ですか",
                ),
            )
            .expect("execute copular relation query");
        assert_eq!(copular_anchor.lifecycle, Lifecycle::Commit);
        let copular_request = RequestEnvelope::new(
            copular_case,
            "copular-context-assertion",
            actor,
            "[scope=biology]それは動物です。",
        );
        let copular_assertion = reopened
            .execute(actor, copular_request.clone())
            .expect("execute contextual Japanese copular assertion");
        assert_eq!(copular_assertion.lifecycle, Lifecycle::Commit);
        assert!(copular_assertion.dependencies.contains(&copular_anchor.id));
        let copular_state = reopened
            .latest(actor, copular_case)
            .expect("read contextual copular state")
            .expect("contextual copular state exists");
        let copular_bundle = copular_state
            .bundle
            .as_ref()
            .expect("contextual copular input is sealed");
        let copular_evidence = input::assertion_source_evidence(copular_bundle)
            .expect("validate contextual copular source evidence")
            .expect("contextual copular source roles are complete");
        let copular_memory = reopened
            .store
            .load_memory(actor)
            .expect("load memory after contextual copular assertion");
        let copular_fact = copular_memory
            .facts
            .iter()
            .find(|fact| fact.source_evidence.as_ref() == Some(&copular_evidence))
            .expect("contextual copular fact is persisted");
        let copular_fact_id = &copular_fact.id;
        assert!(copular_assertion.dependencies.contains(copular_fact_id));
        assert_eq!(copular_fact.proposition.subject, "ミナ");
        assert_eq!(copular_fact.source_evidence, Some(copular_evidence.clone()));
        assert_eq!(
            copular_evidence.source_roles["/AssertPreviousRelation/proposition/subject"][0].text,
            "それ"
        );
        assert_eq!(
            copular_evidence.source_roles["/AssertPreviousRelation/proposition/predicate"][0].text,
            "は"
        );
        let copular_followup = reopened
            .execute(
                actor,
                RequestEnvelope::new(
                    copular_case,
                    "copular-context-followup",
                    actor,
                    "[scope=biology]ミナは動物ですか",
                ),
            )
            .expect("normal Core query consumes the contextual copular fact");
        assert_eq!(copular_followup.lifecycle, Lifecycle::Commit);
        assert!(copular_followup.dependencies.contains(copular_fact_id));

        let missing_context = reopened
            .execute(
                actor,
                RequestEnvelope::new(
                    "contextual-assertion-without-anchor",
                    "missing-context-assertion",
                    actor,
                    "[scope=biology]それは水を使う。",
                ),
            )
            .expect("missing-context assertion returns a typed result");
        assert_eq!(missing_context.lifecycle, Lifecycle::Suspend);
        assert!(missing_context.dependencies.is_empty());
        let missing_context_state = reopened
            .latest(actor, "contextual-assertion-without-anchor")
            .expect("read missing-context state")
            .expect("missing-context request is retained");
        let missing_context_bundle = missing_context_state
            .bundle
            .as_ref()
            .expect("missing-context input is sealed with unresolved context");
        assert!(missing_context_bundle.meaning.context_result_ref.is_none());
        assert!(
            input::assertion_source_evidence(missing_context_bundle)
                .expect("validate absent contextual source authority")
                .is_none()
        );

        println!(
            "B25_CONTEXTUAL_ASSERTION_SOURCE_PROOF={}",
            serde_json::to_string(&serde_json::json!({
                "anchor_result_id": anchor.id,
                "contextual_source": contextual_request.text,
                "context_result_ref": contextual_bundle.meaning.context_result_ref,
                "resolved_fact_subject": restored_premise.proposition.subject,
                "source_pronoun": expected_evidence.source_roles["/AssertPreviousRelation/proposition/subject"][0].text,
                "parser_version": expected_evidence.parser_version,
                "bundle_fingerprint": expected_evidence.bundle_fingerprint,
                "raw_sha256": expected_evidence.raw_sha256,
                "semantic_signature": expected_evidence.semantic_signature,
                "source_roles": expected_evidence.source_roles,
                "fact_id": contextual_fact_id,
                "reopened_proof_status": proof.status,
                "follow_up_result": answer.text,
                "follow_up_dependencies": answer.dependencies,
                "duplicated_contextual_role_not_projected": true,
                "memory_reopened_with_source_and_context_link": true,
                "query_proof_returns_source_evidence": true,
                "normal_core_followup_consumes_fact": answer.dependencies.contains(&contextual_fact_id),
                "copular_context_source": copular_request.text,
                "copular_context_source_roles": copular_evidence.source_roles,
                "copular_fact_id": copular_fact_id,
                "copular_followup_dependencies": copular_followup.dependencies,
                "missing_context_lifecycle": missing_context.lifecycle,
                "missing_context_has_no_proof_fact": missing_context.dependencies.is_empty(),
                "source_roles_are_themselves_world_truth": false,
                "external_network_or_effects": false
            }))
            .expect("serialize B25 contextual source proof observation")
        );
        drop(reopened);
        fs::remove_dir_all(directory).expect("remove isolated contextual source state");
    }

    #[test]
    fn contextual_negative_japanese_transitive_assertion_preserves_halfwidth_surface_roles_after_reopen()
     {
        let serial = NEXT_TEMP_DIRECTORY.fetch_add(1, Ordering::Relaxed);
        let directory = std::env::temp_dir().join(format!(
            "minidora-core-contextual-negative-transitive-source-{}-{serial}",
            std::process::id()
        ));
        fs::create_dir(&directory).expect("unique contextual negative-source state directory");
        let actor = "contextual-negative-source-actor";
        let case = "contextual-negative-source-case";
        let mut engine = Engine::open(&directory).expect("open Core");

        let seed = engine
            .execute(
                actor,
                RequestEnvelope::new(
                    case,
                    "negative-source-seed",
                    actor,
                    "[scope=biology]ミナは鳥です。",
                ),
            )
            .expect("commit the same-case context premise");
        assert_eq!(seed.lifecycle, Lifecycle::Commit, "{seed:?}");
        let anchor = engine
            .execute(
                actor,
                RequestEnvelope::new(
                    case,
                    "negative-source-anchor",
                    actor,
                    "[scope=biology]ミナは鳥ですか",
                ),
            )
            .expect("commit the actual relation result used as the context anchor");
        assert_eq!(anchor.lifecycle, Lifecycle::Commit, "{anchor:?}");
        assert!(
            seed.dependencies
                .iter()
                .any(|dependency| { anchor.dependencies.contains(dependency) })
        );

        let request = RequestEnvelope::new(
            case,
            "negative-contextual-assertion",
            actor,
            "[scope=biology]それがﾐｽﾞを使いません。",
        );
        let result = engine
            .execute(actor, request.clone())
            .expect("execute a negative contextual Japanese transitive assertion");
        assert_eq!(result.lifecycle, Lifecycle::Commit, "{result:?}");
        assert_eq!(result.guarantee, Guarantee::UserPremise);
        assert!(result.dependencies.contains(&anchor.id));

        let state = engine
            .latest(actor, case)
            .expect("read contextual negative assertion state")
            .expect("contextual negative assertion exists");
        let bundle = state.bundle.as_ref().expect("input is sealed");
        assert_eq!(
            bundle.meaning.context_result_ref.as_deref(),
            Some(anchor.id.as_str())
        );
        assert_eq!(bundle.meaning.parser_version, input::PARSER_VERSION);
        assert!(matches!(
            &bundle.meaning.intent,
            Intent::AssertPreviousRelation { proposition, .. }
                if proposition.subject == "それ"
                    && proposition.predicate == "use"
                    && proposition.object == "ﾐｽﾞ"
                    && proposition.negative
                    && proposition.scope == "biology"
        ));

        let evidence = input::assertion_source_evidence(bundle)
            .expect("validate the contextual source role map")
            .expect("supported contextual negative assertion retains source evidence");
        assert_eq!(evidence.raw_sha256, bundle.raw_sha256);
        assert_eq!(evidence.bundle_fingerprint, bundle.fingerprint);
        assert_eq!(
            evidence.input_fingerprint,
            identity(&bundle.request).expect("context request identity")
        );
        assert_eq!(evidence.source_roles.len(), 6);
        for (role, expected) in [
            ("/AssertPreviousRelation/proposition/subject", "それ"),
            ("/AssertPreviousRelation/proposition/subject_particle", "が"),
            ("/AssertPreviousRelation/proposition/object", "ﾐｽﾞ"),
            ("/AssertPreviousRelation/proposition/object_particle", "を"),
            (
                "/AssertPreviousRelation/proposition/predicate",
                "使いません",
            ),
            ("/AssertPreviousRelation/proposition/scope", "biology"),
        ] {
            let spans = evidence
                .source_roles
                .get(role)
                .unwrap_or_else(|| panic!("missing source role {role}"));
            assert_eq!(spans.len(), 1, "ambiguous source role {role}");
            let span = &spans[0];
            assert_eq!(span.text, expected, "wrong surface value for {role}");
            assert_eq!(
                request.text.get(span.start..span.end),
                Some(expected),
                "role {role} is not mapped to the original UTF-8 bytes"
            );
        }
        let object_span = &evidence.source_roles["/AssertPreviousRelation/proposition/object"][0];
        assert_eq!(
            object_span.end - object_span.start,
            "ﾐｽﾞ".len(),
            "the half-width source spelling keeps its original byte width"
        );

        let memory = engine
            .store
            .load_memory(actor)
            .expect("load memory with the contextual negative Fact");
        let fact = memory
            .facts
            .iter()
            .find(|fact| fact.source_evidence.as_ref() == Some(&evidence))
            .expect("contextual Fact exists in Core memory");
        let fact_id = fact.id.clone();
        assert!(result.dependencies.contains(&fact_id));
        assert_eq!(fact.proposition.subject, "ミナ");
        assert_eq!(fact.proposition.object, "ﾐｽﾞ");
        assert!(fact.proposition.negative);
        assert_eq!(fact.source_evidence, Some(evidence.clone()));

        drop(engine);
        let mut reopened = Engine::open(&directory).expect("reopen Core state");
        let restored_memory = reopened
            .store
            .load_memory(actor)
            .expect("revalidate sealed input evidence after reopen");
        let restored_fact = restored_memory
            .facts
            .iter()
            .find(|fact| fact.id == fact_id)
            .expect("restore the contextual negative Fact");
        assert_eq!(restored_fact.source_evidence, Some(evidence.clone()));
        assert!(restored_fact.proposition.negative);
        let proof = relations::query(&restored_memory, &restored_fact.proposition, 32)
            .expect("query the reopened contextual negative Fact");
        assert_eq!(proof.status, "SUPPORTED");
        assert_eq!(proof.facts.len(), 1);
        assert_eq!(proof.facts[0].source_evidence, Some(evidence.clone()));
        assert!(relations::answer_dependencies(&proof).contains(&fact_id));

        let followup = reopened
            .execute(
                actor,
                RequestEnvelope::new(
                    case,
                    "negative-contextual-followup",
                    actor,
                    "[scope=biology]ミナはﾐｽﾞを使いませんか",
                ),
            )
            .expect("normal Core question consumes the reopened negative premise");
        assert_eq!(followup.lifecycle, Lifecycle::Commit, "{followup:?}");
        assert_eq!(followup.guarantee, Guarantee::UserPremise);
        assert!(followup.dependencies.contains(&fact_id));

        println!(
            "B26_CONTEXTUAL_HALF_WIDTH_NEGATIVE_TRANSITIVE_SOURCE_PROOF={}",
            serde_json::to_string(&serde_json::json!({
                "anchor_result_id": anchor.id,
                "context_result_ref": bundle.meaning.context_result_ref,
                "source": request.text,
                "parser_version": evidence.parser_version,
                "source_roles": evidence.source_roles,
                "half_width_object_original_bytes_match": true,
                "resolved_fact_subject": restored_fact.proposition.subject,
                "fact_object_surface": restored_fact.proposition.object,
                "negative_polarity_preserved": restored_fact.proposition.negative,
                "raw_sha256": evidence.raw_sha256,
                "bundle_fingerprint": evidence.bundle_fingerprint,
                "semantic_signature": evidence.semantic_signature,
                "fact_id": fact_id,
                "reopened_proof_status": proof.status,
                "followup_result": followup.text,
                "followup_dependencies": followup.dependencies,
                "same_fact_evidence_survives_reopen": true,
                "normal_core_followup_consumes_fact": followup.dependencies.contains(&fact_id),
                "source_roles_are_themselves_world_truth": false,
                "external_network_or_effects": false
            }))
            .expect("serialize B26 contextual negative-source observation")
        );
        drop(reopened);
        fs::remove_dir_all(directory).expect("remove isolated contextual negative-source state");
    }

    #[test]
    fn japanese_premise_correction_source_roles_follow_both_facts_into_reopened_proofs() {
        let serial = NEXT_TEMP_DIRECTORY.fetch_add(1, Ordering::Relaxed);
        let directory = std::env::temp_dir().join(format!(
            "minidora-core-premise-correction-source-{}-{serial}",
            std::process::id()
        ));
        fs::create_dir(&directory).expect("unique premise-correction source state directory");
        let actor = "premise-correction-source-actor";
        let case = "premise-correction-source-case";
        let mut engine = Engine::open(&directory).expect("open Core");

        let seed = engine
            .execute(
                actor,
                RequestEnvelope::new(
                    case,
                    "correction-seed",
                    actor,
                    "[scope=biology]ココは哺乳類です。",
                ),
            )
            .expect("commit the premise that will be corrected");
        assert_eq!(seed.lifecycle, Lifecycle::Commit, "{seed:?}");

        let request = RequestEnvelope::new(
            case,
            "correction-input",
            actor,
            "[scope=biology]訂正: ココは哺乳類ではなく鳥です。",
        );
        let correction = engine
            .execute(actor, request.clone())
            .expect("execute a scoped Japanese premise correction");
        assert_eq!(correction.lifecycle, Lifecycle::Commit, "{correction:?}");
        assert_eq!(correction.guarantee, Guarantee::UserPremise);

        let state = engine
            .latest(actor, case)
            .expect("read correction state")
            .expect("correction result exists");
        let bundle = state.bundle.as_ref().expect("correction input is sealed");
        assert_eq!(bundle.meaning.parser_version, input::PARSER_VERSION);
        assert!(matches!(
            &bundle.meaning.intent,
            Intent::CorrectPremise { superseded, replacement }
                if superseded.subject == "ココ"
                    && superseded.object == "哺乳類"
                    && replacement.subject == "ココ"
                    && replacement.object == "鳥"
                    && superseded.scope == "biology"
                    && replacement.scope == "biology"
        ));
        let evidence = input::assertion_source_evidence(bundle)
            .expect("validate the sealed correction source roles")
            .expect("supported Japanese correction retains source evidence");
        assert_eq!(evidence.raw_sha256, bundle.raw_sha256);
        assert_eq!(evidence.bundle_fingerprint, bundle.fingerprint);
        assert_eq!(
            evidence.input_fingerprint,
            identity(&bundle.request).unwrap()
        );
        for (role, expected) in [
            ("/CorrectPremise/subject", "ココ"),
            ("/CorrectPremise/subject_particle", "は"),
            ("/CorrectPremise/superseded_object", "哺乳類"),
            ("/CorrectPremise/correction_marker", "ではなく"),
            ("/CorrectPremise/replacement_object", "鳥"),
            ("/CorrectPremise/scope", "biology"),
        ] {
            let spans = evidence
                .source_roles
                .get(role)
                .unwrap_or_else(|| panic!("missing correction source role {role}"));
            assert_eq!(spans.len(), 1, "ambiguous correction source role {role}");
            assert_eq!(spans[0].text, expected, "incorrect correction role {role}");
            assert_eq!(
                request.text.get(spans[0].start..spans[0].end),
                Some(expected)
            );
        }

        let corrected_old = Proposition {
            subject: "ココ".into(),
            predicate: "is".into(),
            object: "哺乳類".into(),
            scope: "biology".into(),
            negative: true,
            temporal_scope: None,
            condition_refs: Default::default(),
        };
        let replacement = Proposition {
            subject: "ココ".into(),
            predicate: "is".into(),
            object: "鳥".into(),
            scope: "biology".into(),
            negative: false,
            temporal_scope: None,
            condition_refs: Default::default(),
        };
        let memory = engine
            .store
            .load_memory(actor)
            .expect("load corrected premises with source provenance");
        let corrected_old_fact = memory
            .facts
            .iter()
            .find(|fact| fact.valid && fact.proposition == corrected_old)
            .expect("negative superseded premise is recorded");
        let replacement_fact = memory
            .facts
            .iter()
            .find(|fact| fact.valid && fact.proposition == replacement)
            .expect("replacement premise is recorded");
        assert_eq!(corrected_old_fact.source_evidence, Some(evidence.clone()));
        assert_eq!(replacement_fact.source_evidence, Some(evidence.clone()));
        assert!(correction.dependencies.contains(&corrected_old_fact.id));
        assert!(correction.dependencies.contains(&replacement_fact.id));
        let corrected_old_id = corrected_old_fact.id.clone();
        let replacement_id = replacement_fact.id.clone();

        drop(engine);
        let mut reopened = Engine::open(&directory).expect("reopen Core state");
        let restored_memory = reopened
            .store
            .load_memory(actor)
            .expect("revalidate correction source evidence after reopen");
        for (proposition, fact_id) in [
            (&corrected_old, &corrected_old_id),
            (&replacement, &replacement_id),
        ] {
            let proof = relations::query(&restored_memory, proposition, 32)
                .expect("query a reopened correction premise");
            assert_eq!(proof.status, "SUPPORTED");
            assert!(relations::answer_dependencies(&proof).contains(fact_id));
            assert_eq!(proof.facts[0].source_evidence, Some(evidence.clone()));
        }

        let followup = reopened
            .execute(
                actor,
                RequestEnvelope::new(
                    case,
                    "correction-followup",
                    actor,
                    "[scope=biology]ココは鳥ですか",
                ),
            )
            .expect("ordinary Core query consumes the replacement fact");
        assert_eq!(followup.lifecycle, Lifecycle::Commit, "{followup:?}");
        assert!(followup.dependencies.contains(&replacement_id));
        println!(
            "B27_JAPANESE_CORRECTION_SOURCE_ROLES_REOPENED_PROOF={}",
            serde_json::to_string(&serde_json::json!({
                "input": request.text,
                "parser_version": evidence.parser_version,
                "input_fingerprint": evidence.input_fingerprint,
                "raw_sha256": evidence.raw_sha256,
                "bundle_fingerprint": evidence.bundle_fingerprint,
                "semantic_signature": evidence.semantic_signature,
                "source_roles": evidence.source_roles,
                "corrected_old_fact": corrected_old_id,
                "replacement_fact": replacement_id,
                "both_facts_link_same_source_evidence": true,
                "reopened_negative_proof": "SUPPORTED",
                "reopened_replacement_proof": "SUPPORTED",
                "followup_dependencies": followup.dependencies,
                "followup_consumes_replacement_fact": followup.dependencies.contains(&replacement_id),
                "source_roles_are_themselves_world_truth": false,
                "external_network_or_effects": false
            }))
            .expect("serialize B27 correction source observation")
        );
        drop(reopened);
        fs::remove_dir_all(directory).expect("remove isolated premise-correction source state");
    }

    #[test]
    fn english_premise_correction_source_roles_follow_both_facts_into_reopened_proofs() {
        let serial = NEXT_TEMP_DIRECTORY.fetch_add(1, Ordering::Relaxed);
        let directory = std::env::temp_dir().join(format!(
            "minidora-core-english-correction-source-{}-{serial}",
            std::process::id()
        ));
        fs::create_dir(&directory).expect("unique English correction state directory");
        let actor = "english-correction-source-actor";
        let case = "english-correction-source-case";
        let mut engine = Engine::open(&directory).expect("open Core");

        let seed = engine
            .execute(
                actor,
                RequestEnvelope::new(
                    case,
                    "english-correction-seed",
                    actor,
                    "[scope=biology]Coco is a mammal.",
                ),
            )
            .expect("commit the English premise that will be corrected");
        assert_eq!(seed.lifecycle, Lifecycle::Commit, "{seed:?}");

        let request = RequestEnvelope::new(
            case,
            "english-correction-input",
            actor,
            "[scope=biology]Correction: Coco is a bird, not a mammal.",
        );
        let correction = engine
            .execute(actor, request.clone())
            .expect("execute a scoped English premise correction");
        assert_eq!(correction.lifecycle, Lifecycle::Commit, "{correction:?}");
        assert_eq!(correction.guarantee, Guarantee::UserPremise);

        let state = engine
            .latest(actor, case)
            .expect("read English correction state")
            .expect("English correction result exists");
        let bundle = state.bundle.as_ref().expect("correction input is sealed");
        assert_eq!(bundle.meaning.parser_version, input::PARSER_VERSION);
        assert!(matches!(
            &bundle.meaning.intent,
            Intent::CorrectPremise { superseded, replacement }
                if superseded.subject == "Coco"
                    && superseded.object == "mammal"
                    && replacement.subject == "Coco"
                    && replacement.object == "bird"
                    && superseded.scope == "biology"
                    && replacement.scope == "biology"
        ));
        let evidence = input::assertion_source_evidence(bundle)
            .expect("validate the English correction source roles")
            .expect("supported English correction retains source evidence");
        assert_eq!(evidence.raw_sha256, bundle.raw_sha256);
        assert_eq!(evidence.bundle_fingerprint, bundle.fingerprint);
        assert_eq!(
            evidence.input_fingerprint,
            identity(&bundle.request).unwrap()
        );
        for (role, expected) in [
            ("/CorrectPremise/subject", "Coco"),
            ("/CorrectPremise/copula", "is"),
            ("/CorrectPremise/superseded_article", "a"),
            ("/CorrectPremise/superseded_object", "mammal"),
            ("/CorrectPremise/correction_marker", "not"),
            ("/CorrectPremise/replacement_article", "a"),
            ("/CorrectPremise/replacement_object", "bird"),
            ("/CorrectPremise/scope", "biology"),
        ] {
            let spans = evidence
                .source_roles
                .get(role)
                .unwrap_or_else(|| panic!("missing English correction role {role}"));
            assert_eq!(spans.len(), 1, "ambiguous English correction role {role}");
            assert_eq!(
                spans[0].text, expected,
                "incorrect English correction role {role}"
            );
            assert_eq!(
                request.text.get(spans[0].start..spans[0].end),
                Some(expected)
            );
        }

        let corrected_old = Proposition {
            subject: "Coco".into(),
            predicate: "is".into(),
            object: "mammal".into(),
            scope: "biology".into(),
            negative: true,
            temporal_scope: None,
            condition_refs: Default::default(),
        };
        let replacement = Proposition {
            subject: "Coco".into(),
            predicate: "is".into(),
            object: "bird".into(),
            scope: "biology".into(),
            negative: false,
            temporal_scope: None,
            condition_refs: Default::default(),
        };
        let memory = engine
            .store
            .load_memory(actor)
            .expect("load corrected English premises with provenance");
        let corrected_old_fact = memory
            .facts
            .iter()
            .find(|fact| fact.valid && fact.proposition == corrected_old)
            .expect("negative superseded English premise is recorded");
        let replacement_fact = memory
            .facts
            .iter()
            .find(|fact| fact.valid && fact.proposition == replacement)
            .expect("English replacement premise is recorded");
        assert_eq!(corrected_old_fact.source_evidence, Some(evidence.clone()));
        assert_eq!(replacement_fact.source_evidence, Some(evidence.clone()));
        let corrected_old_id = corrected_old_fact.id.clone();
        let replacement_id = replacement_fact.id.clone();

        drop(engine);
        let mut reopened = Engine::open(&directory).expect("reopen English Core state");
        let restored_memory = reopened
            .store
            .load_memory(actor)
            .expect("revalidate English correction evidence after reopen");
        for (proposition, fact_id) in [
            (&corrected_old, &corrected_old_id),
            (&replacement, &replacement_id),
        ] {
            let proof = relations::query(&restored_memory, proposition, 32)
                .expect("query a reopened English correction premise");
            assert_eq!(proof.status, "SUPPORTED");
            assert!(relations::answer_dependencies(&proof).contains(fact_id));
            assert_eq!(proof.facts[0].source_evidence, Some(evidence.clone()));
        }

        let followup = reopened
            .execute(
                actor,
                RequestEnvelope::new(
                    case,
                    "english-correction-followup",
                    actor,
                    "[scope=biology]Coco is a bird?",
                ),
            )
            .expect("ordinary Core query consumes the English replacement fact");
        assert_eq!(followup.lifecycle, Lifecycle::Commit, "{followup:?}");
        assert!(followup.dependencies.contains(&replacement_id));
        println!(
            "B28_ENGLISH_CORRECTION_SOURCE_ROLES_REOPENED_PROOF={}",
            serde_json::to_string(&serde_json::json!({
                "input": request.text,
                "parser_version": evidence.parser_version,
                "input_fingerprint": evidence.input_fingerprint,
                "raw_sha256": evidence.raw_sha256,
                "bundle_fingerprint": evidence.bundle_fingerprint,
                "semantic_signature": evidence.semantic_signature,
                "source_roles": evidence.source_roles,
                "corrected_old_fact": corrected_old_id,
                "replacement_fact": replacement_id,
                "both_facts_link_same_source_evidence": true,
                "reopened_negative_proof": "SUPPORTED",
                "reopened_replacement_proof": "SUPPORTED",
                "followup_dependencies": followup.dependencies,
                "followup_consumes_replacement_fact": followup.dependencies.contains(&replacement_id),
                "source_roles_are_themselves_world_truth": false,
                "external_network_or_effects": false
            }))
            .expect("serialize B28 English correction source observation")
        );
        drop(reopened);
        fs::remove_dir_all(directory).expect("remove isolated English correction state");
    }

    #[test]
    fn contextual_premise_correction_source_roles_follow_both_facts_into_reopened_proofs() {
        struct Scenario<'a> {
            actor: &'a str,
            case: &'a str,
            seed_input: &'a str,
            anchor_input: &'a str,
            correction_input: &'a str,
            followup_input: &'a str,
            subject: &'a str,
            superseded_object: &'a str,
            replacement_object: &'a str,
            roles: &'a [(&'a str, &'a str)],
        }
        fn observe_case(parent: &std::path::Path, scenario: Scenario<'_>) {
            let Scenario {
                actor,
                case,
                seed_input,
                anchor_input,
                correction_input,
                followup_input,
                subject,
                superseded_object,
                replacement_object,
                roles,
            } = scenario;
            let directory = parent.join(case);
            fs::create_dir(&directory).expect("create isolated contextual-correction state");
            let mut engine = Engine::open(&directory).expect("open contextual Core");
            let seed = engine
                .execute(actor, RequestEnvelope::new(case, "seed", actor, seed_input))
                .expect("commit the premise later corrected");
            assert_eq!(seed.lifecycle, Lifecycle::Commit, "{seed:?}");
            let anchor = engine
                .execute(
                    actor,
                    RequestEnvelope::new(case, "anchor", actor, anchor_input),
                )
                .expect("commit the actual contextual correction anchor");
            assert_eq!(anchor.lifecycle, Lifecycle::Commit, "{anchor:?}");
            assert!(
                seed.dependencies
                    .iter()
                    .any(|dependency| anchor.dependencies.contains(dependency))
            );

            let request = RequestEnvelope::new(case, "correction", actor, correction_input);
            let correction = engine
                .execute(actor, request.clone())
                .expect("execute contextual premise correction");
            assert_eq!(correction.lifecycle, Lifecycle::Commit, "{correction:?}");
            assert_eq!(correction.guarantee, Guarantee::UserPremise);
            let state = engine
                .latest(actor, case)
                .expect("read contextual correction result")
                .expect("contextual correction result exists");
            let bundle = state.bundle.as_ref().expect("sealed correction input");
            assert_eq!(
                bundle.meaning.context_result_ref.as_deref(),
                Some(anchor.id.as_str())
            );
            match &bundle.meaning.intent {
                Intent::CorrectPreviousPremise {
                    predicate,
                    superseded_object: parsed_old,
                    replacement_object: parsed_new,
                    scope,
                } => {
                    assert_eq!(predicate, "is");
                    assert_eq!(parsed_old, superseded_object);
                    assert_eq!(parsed_new, replacement_object);
                    assert_eq!(scope.as_deref(), Some("biology"));
                }
                other => panic!("expected contextual correction, got {other:?}"),
            }
            assert_eq!(bundle.meaning.parser_version, input::PARSER_VERSION);
            let evidence = input::assertion_source_evidence(bundle)
                .expect("validate contextual correction source roles")
                .expect("the exact source roles bind to the actual prior result");
            assert_eq!(evidence.bundle_fingerprint, bundle.fingerprint);
            assert_eq!(evidence.raw_sha256, bundle.raw_sha256);
            assert_eq!(
                evidence.input_fingerprint,
                identity(&bundle.request).unwrap()
            );
            let role_root = "/CorrectPreviousPremise";
            for (role, expected) in roles {
                let role = format!("{role_root}/{role}");
                let spans = evidence
                    .source_roles
                    .get(&role)
                    .unwrap_or_else(|| panic!("missing contextual correction role {role}"));
                assert_eq!(
                    spans.len(),
                    1,
                    "ambiguous contextual correction role {role}"
                );
                assert_eq!(spans[0].text, *expected, "incorrect contextual role {role}");
                assert_eq!(
                    request.text.get(spans[0].start..spans[0].end),
                    Some(*expected)
                );
            }

            let corrected_old = Proposition {
                subject: subject.into(),
                predicate: "is".into(),
                object: superseded_object.into(),
                scope: "biology".into(),
                negative: true,
                temporal_scope: None,
                condition_refs: Default::default(),
            };
            let replacement = Proposition {
                object: replacement_object.into(),
                negative: false,
                ..corrected_old.clone()
            };
            let memory = engine
                .store
                .load_memory(actor)
                .expect("load contextual correction facts");
            let corrected_old_fact = memory
                .facts
                .iter()
                .find(|fact| fact.valid && fact.proposition == corrected_old)
                .expect("negative superseded fact is persisted");
            let replacement_fact = memory
                .facts
                .iter()
                .find(|fact| fact.valid && fact.proposition == replacement)
                .expect("replacement fact is persisted");
            assert_eq!(corrected_old_fact.source_evidence, Some(evidence.clone()));
            assert_eq!(replacement_fact.source_evidence, Some(evidence.clone()));
            assert!(correction.dependencies.contains(&corrected_old_fact.id));
            assert!(correction.dependencies.contains(&replacement_fact.id));
            let corrected_old_id = corrected_old_fact.id.clone();
            let replacement_id = replacement_fact.id.clone();

            drop(engine);
            let mut reopened = Engine::open(&directory).expect("reopen contextual Core");
            let restored_memory = reopened
                .store
                .load_memory(actor)
                .expect("Store revalidates sealed contextual evidence");
            for (proposition, fact_id) in [
                (&corrected_old, &corrected_old_id),
                (&replacement, &replacement_id),
            ] {
                let proof = relations::query(&restored_memory, proposition, 32)
                    .expect("query reopened contextual correction fact");
                assert_eq!(proof.status, "SUPPORTED");
                assert!(relations::answer_dependencies(&proof).contains(fact_id));
                assert_eq!(proof.facts[0].source_evidence, Some(evidence.clone()));
            }
            let followup = reopened
                .execute(
                    actor,
                    RequestEnvelope::new(case, "followup", actor, followup_input),
                )
                .expect("ordinary Core query consumes the replacement fact");
            assert_eq!(followup.lifecycle, Lifecycle::Commit, "{followup:?}");
            assert!(followup.dependencies.contains(&replacement_id));
            println!(
                "B29_CONTEXTUAL_CORRECTION_SOURCE_PROOF={}",
                serde_json::to_string(&serde_json::json!({
                    "input": request.text,
                    "anchor_result_id": anchor.id,
                    "context_result_ref": bundle.meaning.context_result_ref,
                    "parser_version": evidence.parser_version,
                    "source_roles": evidence.source_roles,
                    "corrected_old_fact_id": corrected_old_id,
                    "replacement_fact_id": replacement_id,
                    "both_facts_link_same_source_evidence": true,
                    "reopened_negative_proof": "SUPPORTED",
                    "reopened_replacement_proof": "SUPPORTED",
                    "followup_consumes_replacement_fact": followup.dependencies.contains(&replacement_id),
                    "source_roles_are_themselves_world_truth": false,
                    "external_network_or_effects": false
                }))
                .expect("serialize B29 contextual correction observation")
            );
            drop(reopened);
            fs::remove_dir_all(&directory).expect("remove isolated contextual correction state");
        }

        let serial = NEXT_TEMP_DIRECTORY.fetch_add(1, Ordering::Relaxed);
        let parent = std::env::temp_dir().join(format!(
            "minidora-core-contextual-correction-source-{}-{serial}",
            std::process::id()
        ));
        fs::create_dir(&parent).expect("unique contextual-correction source state directory");
        observe_case(
            &parent,
            Scenario {
                actor: "context-ja-correction-actor",
                case: "context-ja-correction-case",
                seed_input: "[scope=biology]ココは哺乳類です。",
                anchor_input: "[scope=biology]ココは哺乳類ですか",
                correction_input: "[scope=biology]それは哺乳類ではなく鳥です。",
                followup_input: "[scope=biology]ココは鳥ですか",
                subject: "ココ",
                superseded_object: "哺乳類",
                replacement_object: "鳥",
                roles: &[
                    ("subject", "それ"),
                    ("subject_particle", "は"),
                    ("superseded_object", "哺乳類"),
                    ("correction_marker", "ではなく"),
                    ("replacement_object", "鳥"),
                    ("scope", "biology"),
                ],
            },
        );
        observe_case(
            &parent,
            Scenario {
                actor: "context-en-correction-actor",
                case: "context-en-correction-case",
                seed_input: "[scope=biology]Coco is a mammal.",
                anchor_input: "[scope=biology]Coco is a mammal?",
                correction_input: "[scope=biology]It is a bird, not a mammal.",
                followup_input: "[scope=biology]Coco is a bird?",
                subject: "Coco",
                superseded_object: "mammal",
                replacement_object: "bird",
                roles: &[
                    ("subject", "It"),
                    ("copula", "is"),
                    ("superseded_article", "a"),
                    ("superseded_object", "mammal"),
                    ("correction_marker", "not"),
                    ("replacement_article", "a"),
                    ("replacement_object", "bird"),
                    ("scope", "biology"),
                ],
            },
        );

        let mut engine = Engine::open(&parent).expect("open Core for missing-context boundary");
        let missing = engine
            .execute(
                "context-correction-missing-actor",
                RequestEnvelope::new(
                    "context-correction-missing-case",
                    "missing-context",
                    "context-correction-missing-actor",
                    "[scope=biology]それは哺乳類ではなく鳥です。",
                ),
            )
            .expect("missing contextual anchor returns a typed stop");
        assert_eq!(missing.lifecycle, Lifecycle::Suspend, "{missing:?}");
        assert!(missing.dependencies.is_empty());
        let missing_state = engine
            .latest(
                "context-correction-missing-actor",
                "context-correction-missing-case",
            )
            .expect("read missing-context result")
            .expect("retain missing-context request");
        let missing_bundle = missing_state
            .bundle
            .as_ref()
            .expect("sealed missing-context input");
        assert!(missing_bundle.meaning.context_result_ref.is_none());
        assert!(
            input::assertion_source_evidence(missing_bundle)
                .expect("validate missing-context source roles")
                .is_none(),
            "a correction without its actual previous result has no sealed evidence"
        );
        drop(engine);
        fs::remove_dir_all(parent).expect("remove contextual correction state directory");
    }

    #[test]
    fn direct_core_observation_preserves_relation_conditions_subject_scope_and_negation() {
        let serial = NEXT_TEMP_DIRECTORY.fetch_add(1, Ordering::Relaxed);
        let directory = std::env::temp_dir().join(format!(
            "minidora-core-construction-observation-{}-{serial}",
            std::process::id()
        ));
        fs::create_dir(&directory).expect("unique core observation state directory");
        let mut engine = Engine::open(&directory).expect("open isolated Core state");
        let actor = "core-observation-actor";
        let case = "core-observation-case";
        let mut observations = Vec::new();

        let rule = execute_observation(
            &mut engine,
            actor,
            case,
            "rule",
            "If [scope=virtual-lab]X is a ceramic and heated then [scope=virtual-lab]X is conductive",
        );
        assert_eq!(rule.lifecycle, Lifecycle::Commit);
        assert_eq!(rule.guarantee, Guarantee::UserPremise);
        let rule_id = rule.dependencies[0].clone();
        observations.push(serde_json::json!({
            "input_id": "rule",
            "input": "If [scope=virtual-lab]X is a ceramic and heated then [scope=virtual-lab]X is conductive",
            "result": rule,
        }));

        let ceramic = execute_observation(
            &mut engine,
            actor,
            case,
            "pico-ceramic",
            "[scope=virtual-lab]Pico is a ceramic",
        );
        assert_eq!(ceramic.lifecycle, Lifecycle::Commit);
        let ceramic_id = ceramic.dependencies[0].clone();
        observations.push(serde_json::json!({
            "input_id": "pico-ceramic",
            "input": "[scope=virtual-lab]Pico is a ceramic",
            "result": ceramic,
        }));

        let missing_condition = execute_observation(
            &mut engine,
            actor,
            case,
            "pico-conductive-before-heating",
            "[scope=virtual-lab]Pico is conductive?",
        );
        assert_eq!(missing_condition.lifecycle, Lifecycle::Suspend);
        assert!(
            missing_condition
                .reason
                .as_deref()
                .unwrap_or_default()
                .contains("MISSING_PREMISES")
        );
        observations.push(serde_json::json!({
            "input_id": "pico-conductive-before-heating",
            "input": "[scope=virtual-lab]Pico is conductive?",
            "result": missing_condition,
        }));

        let heated = execute_observation(
            &mut engine,
            actor,
            case,
            "pico-heated",
            "[scope=virtual-lab]Pico is heated",
        );
        assert_eq!(heated.lifecycle, Lifecycle::Commit);
        let heated_id = heated.dependencies[0].clone();
        observations.push(serde_json::json!({
            "input_id": "pico-heated",
            "input": "[scope=virtual-lab]Pico is heated",
            "result": heated,
        }));

        let supported = execute_observation(
            &mut engine,
            actor,
            case,
            "pico-conductive-after-heating",
            "[scope=virtual-lab]Pico is conductive?",
        );
        assert_eq!(supported.lifecycle, Lifecycle::Commit);
        assert_eq!(supported.guarantee, Guarantee::Derived);
        let memory_after_derivation = engine
            .store
            .load_memory(actor)
            .expect("read Core memory after relation derivation");
        assert_eq!(memory_after_derivation.rules.len(), 1);
        assert_eq!(memory_after_derivation.facts.len(), 2);
        assert!(memory_after_derivation.facts.iter().all(|fact| {
            fact.guarantee == Guarantee::UserPremise && fact.dependencies.is_empty()
        }));
        let derived_ids: Vec<_> = supported
            .dependencies
            .iter()
            .filter(|id| id.starts_with("fact:") && *id != &ceramic_id && *id != &heated_id)
            .cloned()
            .collect();
        assert_eq!(
            derived_ids.len(),
            1,
            "one derived proof identity is reported"
        );
        let derived_id = derived_ids[0].clone();
        assert_eq!(
            supported
                .dependencies
                .iter()
                .cloned()
                .collect::<BTreeSet<_>>(),
            [
                rule_id.clone(),
                ceramic_id.clone(),
                heated_id.clone(),
                derived_id,
            ]
            .into_iter()
            .collect()
        );
        observations.push(serde_json::json!({
            "input_id": "pico-conductive-after-heating",
            "input": "[scope=virtual-lab]Pico is conductive?",
            "result": supported,
        }));

        for (input_id, input) in [
            ("luma-conductive", "[scope=virtual-lab]Luma is conductive?"),
            ("pico-default-scope", "[scope=default]Pico is conductive?"),
        ] {
            let result = execute_observation(&mut engine, actor, case, input_id, input);
            assert_eq!(result.lifecycle, Lifecycle::Suspend, "{input_id}");
            assert!(
                result
                    .reason
                    .as_deref()
                    .unwrap_or_default()
                    .contains("MISSING_PREMISES"),
                "{input_id}: {:?}",
                result.reason
            );
            observations.push(serde_json::json!({
                "input_id": input_id,
                "input": input,
                "result": result,
            }));
        }

        let fenn_ceramic = execute_observation(
            &mut engine,
            actor,
            case,
            "fenn-ceramic",
            "[scope=virtual-lab]Fenn is a ceramic",
        );
        assert_eq!(fenn_ceramic.lifecycle, Lifecycle::Commit);
        assert_eq!(fenn_ceramic.guarantee, Guarantee::UserPremise);
        observations.push(serde_json::json!({
            "input_id": "fenn-ceramic",
            "input": "[scope=virtual-lab]Fenn is a ceramic",
            "result": fenn_ceramic,
        }));
        let fenn_not_heated = execute_observation(
            &mut engine,
            actor,
            case,
            "fenn-not-heated",
            "[scope=virtual-lab]Fenn is not heated",
        );
        assert_eq!(fenn_not_heated.lifecycle, Lifecycle::Commit);
        let negative_id = fenn_not_heated.dependencies[0].clone();
        observations.push(serde_json::json!({
            "input_id": "fenn-not-heated",
            "input": "[scope=virtual-lab]Fenn is not heated",
            "result": fenn_not_heated,
        }));

        let heated_query = execute_observation(
            &mut engine,
            actor,
            case,
            "fenn-heated-query",
            "[scope=virtual-lab]Fenn is heated?",
        );
        assert_eq!(heated_query.lifecycle, Lifecycle::Commit);
        assert_eq!(heated_query.guarantee, Guarantee::UserPremise);
        assert!(heated_query.text.contains("Fennはheatedではない"));
        assert!(heated_query.dependencies.contains(&negative_id));
        observations.push(serde_json::json!({
            "input_id": "fenn-heated-query",
            "input": "[scope=virtual-lab]Fenn is heated?",
            "result": heated_query,
        }));

        let negative_condition = execute_observation(
            &mut engine,
            actor,
            case,
            "fenn-conductive",
            "[scope=virtual-lab]Fenn is conductive?",
        );
        assert_eq!(negative_condition.lifecycle, Lifecycle::Suspend);
        assert!(
            negative_condition
                .reason
                .as_deref()
                .unwrap_or_default()
                .contains("MISSING_PREMISES")
        );
        observations.push(serde_json::json!({
            "input_id": "fenn-conductive",
            "input": "[scope=virtual-lab]Fenn is conductive?",
            "result": negative_condition,
        }));

        let memory = engine
            .store
            .load_memory(actor)
            .expect("read Core actor memory");
        assert_eq!(memory.facts.len(), 4);
        assert_eq!(memory.rules.len(), 1);
        assert!(memory.facts.iter().all(|fact| {
            fact.guarantee == Guarantee::UserPremise && fact.dependencies.is_empty()
        }));
        let evidence = serde_json::json!({
            "entry": "in-process minidora_engine::Engine::execute",
            "actor_id": actor,
            "case_id": case,
            "observations": observations,
            "memory": memory,
        });
        println!(
            "B10_CORE_OBSERVATION_01={}",
            serde_json::to_string(&evidence).expect("serialize Core observation evidence")
        );

        drop(engine);
        fs::remove_dir_all(directory).expect("remove isolated Core observation state");
    }
}
