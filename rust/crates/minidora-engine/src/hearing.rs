use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::time::{Duration, Instant};

use minidora_domain::{
    Calculation, EffectPhase, GoalContract, Guarantee, InputBundle, Intent, Lifecycle,
    RenderTarget, RequestEnvelope, ResourceBudget, Result, ResultRecord, SourceSpan,
    WorkPlanWithoutPersonnel, canonical, digest, failure, identity,
};
use serde::{Deserialize, Serialize};

use crate::{
    core, document, input, language, output, search, store::Store, work_plan, work_plan_input,
};

pub(crate) const REQUIRED: &str = "HEARING_REQUIRED";
pub(crate) const PREDICATE: &str = "RESOLVED_HEARING_V1";
const PROMPT_KIND: &str = "minidora.core.hearing.v1";

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Prompt {
    kind: String,
    source_ref: String,
    dependencies: Vec<String>,
    task: Task,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
enum Task {
    Length {
        value: Calculation,
        provenance: Vec<String>,
        evidence_bytes: usize,
    },
    FilteredCsv {
        row_count: usize,
        columns: Vec<String>,
    },
    WorkPlanPersonnel {
        units: Vec<String>,
    },
}

pub(crate) struct Outcome {
    pub(crate) units: Vec<String>,
    pub(crate) calculation: Option<Calculation>,
    pub(crate) dependencies: Vec<String>,
    pub(crate) report_bytes: Option<Vec<u8>>,
}

impl Outcome {
    pub(crate) fn matches_result(&self, result: &ResultRecord) -> bool {
        result.guarantee == Guarantee::Computed
            && result.calculation == self.calculation
            && result.actor_memory_revision.is_none()
            && result.evidence.is_empty()
            && result.dependencies == self.dependencies
            && result.content_plan.as_ref().is_some_and(|plan| {
                plan.guarantee == result.guarantee
                    && plan.required_units == self.units
                    && plan.text == self.units.join("\n")
                    && result.text == plan.text
                    && plan
                        .dependencies
                        .strip_prefix(self.dependencies.as_slice())
                        .is_some_and(|refs| refs.len() == 1)
            })
    }
}

pub(crate) enum Preparation {
    Question(Prompt),
    Complete(Outcome),
}

pub(crate) struct AnswerContext {
    question: ResultRecord,
    original: InputBundle,
    prompt: Prompt,
    prompt_ref: String,
}

fn invalid() -> minidora_domain::Error {
    failure(
        "HEARING_CONTEXT_INVALID",
        "補足の対象となる元の依頼と確定済みの根拠を照合できません。",
    )
}

impl Prompt {
    pub(crate) fn preview(&self, reference: &str) -> Outcome {
        let (units, calculation) = match &self.task {
            Task::Length {
                value, provenance, ..
            } => {
                let mut units = vec![format!("確定した計算値は {} です。", value.exact_value)];
                units.extend(provenance.iter().take(2).cloned());
                units.push("どの単位に換算しますか？".into());
                (units, Some(value.clone()))
            }
            Task::FilteredCsv { row_count, columns } => (
                vec![
                    format!("前回の絞り込みで確定した行は {row_count} 件です。"),
                    format!(
                        "どの列を集計しますか？ 候補は {} です。",
                        columns
                            .iter()
                            .enumerate()
                            .map(|(index, column)| format!("候補{}番 {column:?}", index + 1))
                            .collect::<Vec<_>>()
                            .join("、")
                    ),
                ],
                None,
            ),
            Task::WorkPlanPersonnel { units } => (units.clone(), None),
        };
        let mut dependencies = self.dependencies.clone();
        dependencies.push(reference.into());
        Outcome {
            units,
            calculation,
            dependencies,
            report_bytes: None,
        }
    }

    fn answer_span(&self, text: &str) -> Option<SourceSpan> {
        match &self.task {
            Task::Length { .. } => input::hearing_unit_span(text),
            Task::WorkPlanPersonnel { .. } => {
                work_plan_input::personnel_answer(text).map(|(span, _)| span)
            }
            Task::FilteredCsv { columns, .. } => {
                let (field, _) = csv_answer(columns, text)?;
                let start = usize::checked_sub(field.as_ptr() as usize, text.as_ptr() as usize)?;
                Some(SourceSpan {
                    start,
                    end: start + field.len(),
                    text: field.into(),
                })
            }
        }
    }
}

/// Exact or quoted names remain available. An ordinal also gives short,
/// typeable answers for headers containing newlines or long names.
fn csv_answer<'text, 'columns>(
    columns: &'columns [String],
    text: &'text str,
) -> Option<(&'text str, &'columns str)> {
    let trimmed = text.trim();
    let find = |name: &str| {
        columns
            .iter()
            .find(|column| column.as_str() == name)
            .map(String::as_str)
    };
    let term = trimmed
        .trim_end_matches(['。', '.'])
        .trim_end_matches("でお願いします")
        .trim_end_matches("です")
        .trim_end_matches('列')
        .trim();
    if let Some(name) = term
        .strip_prefix('「')
        .and_then(|s| s.strip_suffix('」'))
        .or_else(|| term.strip_prefix('"').and_then(|s| s.strip_suffix('"')))
    {
        return Some((name, find(name)?));
    }
    if let Some(index) = term
        .strip_prefix("候補")
        .and_then(|s| s.strip_suffix('番'))
        .and_then(|s| s.parse::<usize>().ok())
        .and_then(|n| n.checked_sub(1))
    {
        let selected = columns.get(index)?.as_str();
        if find(trimmed).is_some_and(|exact| exact != selected) {
            return None;
        }
        return Some((term, selected));
    }
    match (find(trimmed), find(term)) {
        (Some(exact), Some(suffix)) if exact != suffix => None,
        (Some(exact), _) => Some((trimmed, exact)),
        (_, Some(name)) => Some((term, name)),
        _ => None,
    }
}

fn csv_outcome(value: document::FilteredCsvStatistics) -> Outcome {
    Outcome {
        units: value.units,
        calculation: None,
        dependencies: value.dependencies,
        report_bytes: Some(value.report_bytes),
    }
}

fn work_plan_known_units(
    draft: &WorkPlanWithoutPersonnel,
    preview: &work_plan::PersonnelPreview,
) -> Vec<String> {
    let title = draft
        .purpose
        .as_ref()
        .map_or("作業", |purpose| &purpose.text);
    let tasks = draft
        .tasks
        .iter()
        .map(|task| format!("{}は{}分", task.name.text, task.duration_minutes))
        .collect::<Vec<_>>()
        .join("、");
    let mut units = vec![format!(
        "「{title}」の作業は{tasks}。期限は{}分です。",
        draft.deadline_minutes
    )];
    if !draft.dependencies.is_empty() {
        let dependencies = draft
            .dependencies
            .iter()
            .map(|edge| format!("{}の後に{}", edge.before.text, edge.after.text))
            .collect::<Vec<_>>()
            .join("、");
        units.push(format!("作業の前後関係は{dependencies}です。"));
    }
    units.push(format!(
        "作業順と所要時間から、完了まで最低{}分かかります。",
        preview.lower_bound_minutes
    ));
    if preview.minimum_crew > 1 {
        units.push(format!(
            "一つの工程に同時に{}人が必要です。",
            preview.minimum_crew
        ));
    }
    units
}

fn needs_work_plan_personnel(
    draft: &WorkPlanWithoutPersonnel,
    preview: &work_plan::PersonnelPreview,
) -> bool {
    preview.max_personnel_bound_minutes <= u32::from(draft.deadline_minutes)
        && !(preview.no_overlap && preview.minimum_crew == 1)
}

fn work_plan_question_units(
    draft: &WorkPlanWithoutPersonnel,
    preview: &work_plan::PersonnelPreview,
) -> Vec<String> {
    let mut units = work_plan_known_units(draft, preview);
    units.push("同時に作業できる人数は何人ですか？".into());
    units
}

#[derive(Serialize)]
struct WorkPlanHearingReport<'a> {
    kind: &'static str,
    original_bundle: &'a str,
    answer: Option<(&'a str, &'a SourceSpan)>,
    plan: &'a work_plan::WorkPlanMeaningPlan,
}

fn plan_outcome(
    original: &InputBundle,
    answer: Option<(&str, &SourceSpan)>,
    plan: &work_plan::WorkPlanMeaningPlan,
    budget: &ResourceBudget,
) -> Result<Outcome> {
    let mut units = plan.units();
    if answer.is_none() {
        units.insert(
            0,
            "同時に1人作業できれば、この日程を組めます。実際の人数は未指定です。".into(),
        );
    }
    let report_bytes = canonical(&WorkPlanHearingReport {
        kind: "minidora.core.hearing-work-plan.v1",
        original_bundle: &original.fingerprint,
        answer,
        plan,
    })?;
    if report_bytes.len().saturating_add(units.join("\n").len()) > budget.max_evidence_bytes {
        return Err(failure(
            "RESOURCE_LIMIT",
            "作業計画と根拠が証拠予算を超えています。",
        ));
    }
    Ok(Outcome {
        units,
        calculation: None,
        dependencies: vec![digest(&report_bytes)],
        report_bytes: Some(report_bytes),
    })
}

fn prepare_work_plan(
    bundle: &InputBundle,
    draft: &WorkPlanWithoutPersonnel,
    started: Instant,
) -> Result<Preparation> {
    if !input::verified_work_plan_input(bundle)? {
        return Err(invalid());
    }
    let preview = work_plan::personnel_preview(draft)?;
    let budget = &bundle.request.budget;
    let due = u32::from(draft.deadline_minutes);
    if preview.max_personnel_bound_minutes > due {
        let mut units = work_plan_known_units(draft, &preview);
        units.push(if preview.lower_bound_minutes > due {
            "作業の所要時間と順番を保つ限り、人数を増やしても期限に間に合いません。".into()
        } else {
            format!(
                "この計画で扱える上限の{}人でも、作業量から最低{}分かかるため、期限に間に合いません。",
                work_plan::MAX_PERSONNEL, preview.max_personnel_bound_minutes
            )
        });
        return Ok(Preparation::Complete(Outcome {
            units,
            calculation: None,
            dependencies: Vec::new(),
            report_bytes: None,
        }));
    }
    if !needs_work_plan_personnel(draft, &preview) {
        let deadline = started
            .checked_add(Duration::from_millis(budget.deadline_ms))
            .ok_or_else(invalid)?;
        let plan = work_plan::WorkPlanMeaningPlan::new(
            draft.clone().with_personnel(1),
            bundle.fingerprint.clone(),
            deadline,
        )?;
        return plan_outcome(bundle, None, &plan, budget).map(Preparation::Complete);
    }
    let units = work_plan_question_units(draft, &preview);
    let display_bytes = units.join("\n").len();
    let prompt = Prompt {
        kind: PROMPT_KIND.into(),
        source_ref: bundle.fingerprint.clone(),
        dependencies: Vec::new(),
        task: Task::WorkPlanPersonnel { units },
    };
    if canonical(&prompt)?.len().saturating_add(display_bytes) > budget.max_evidence_bytes {
        return Err(failure(
            "RESOURCE_LIMIT",
            "補足に必要な作業情報が証拠予算を超えています。",
        ));
    }
    Ok(Preparation::Question(prompt))
}

pub(crate) fn prepare(
    store: &Store,
    bundle: &InputBundle,
    previous: Option<&ResultRecord>,
    prior_results: &BTreeMap<String, ResultRecord>,
    started: Instant,
) -> Result<Preparation> {
    if let Intent::ClarifyWorkPlanPersonnel { draft } = &bundle.meaning.intent {
        return prepare_work_plan(bundle, draft, started);
    }
    let previous = previous
        .filter(|prior| prior.lifecycle == Lifecycle::Commit)
        .ok_or_else(|| {
            failure(
                "MISSING_ACTUAL_RESULT",
                "同じ会話に対象となる確定済みの結果がありません。",
            )
        })?;
    let (task, dependencies) = match &bundle.meaning.intent {
        Intent::ClarifyPreviousLength => {
            let (value, provenance, dependencies, evidence_bytes) = if previous.guarantee
                == Guarantee::SourceReport
            {
                let origin = search::verified_web_length_origin(store, bundle, previous, started)?;
                (
                    origin.calculation,
                    origin.units,
                    origin.dependencies,
                    origin.source_bytes,
                )
            } else {
                let value = core::verified_calculation_in_context(
                    store,
                    bundle,
                    previous,
                    prior_results,
                    started,
                )?;
                (value, Vec::new(), vec![previous.id.clone()], 0)
            };
            let source_unit = value
                .exact_value
                .rsplit_once(' ')
                .map(|(_, unit)| unit)
                .filter(|unit| crate::math::exact_length_scale_to_meters(unit).is_some())
                .ok_or_else(|| {
                    failure(
                        "LENGTH_UNIT_REQUIRED",
                        "確定した値の単位を長さとして扱えません。",
                    )
                })?;
            // Validate the exact rational without guessing a requested destination.
            language::convert_exact_length(&value, source_unit)?;
            (
                Task::Length {
                    value,
                    provenance,
                    evidence_bytes,
                },
                dependencies,
            )
        }
        Intent::ClarifyFilteredCsvStatistics => {
            let choices =
                document::filtered_csv_statistics_choices(store, bundle, previous, started)?;
            if choices.row_count == 0 {
                return Err(failure(
                    "CSV_FILTERED_STATISTICS_EMPTY",
                    "絞り込み結果に集計できる行がありません。",
                ));
            }
            if let [column] = &choices.columns[..] {
                return document::filtered_csv_statistics_with_column(
                    store, bundle, previous, column, started,
                )
                .map(csv_outcome)
                .map(Preparation::Complete);
            }
            if choices.columns.is_empty() {
                return Err(failure(
                    "CSV_NO_NUMERIC_COLUMNS",
                    "絞り込んだ表に集計できる数値列がありません。",
                ));
            }
            (
                Task::FilteredCsv {
                    row_count: choices.row_count,
                    columns: choices.columns,
                },
                choices.dependencies,
            )
        }
        _ => return Err(invalid()),
    };
    let prompt = Prompt {
        kind: PROMPT_KIND.into(),
        source_ref: previous.id.clone(),
        dependencies,
        task,
    };
    if let Task::Length { evidence_bytes, .. } = &prompt.task
        && evidence_bytes.saturating_add(prompt.preview("").units.join("\n").len())
            > bundle.request.budget.max_evidence_bytes
    {
        return Err(failure(
            "RESOURCE_LIMIT",
            "保存済みの出典と補足の表示が証拠予算を超えています。",
        ));
    }
    Ok(Preparation::Question(prompt))
}

fn read_question(
    store: &Store,
    actor: &str,
    case: &str,
    question: &ResultRecord,
) -> Result<AnswerContext> {
    let revision = store
        .load_suspended_result_revision(actor, case, question)?
        .ok_or_else(invalid)?;
    let (_, original) = store
        .get_input_bundle(actor, case, &question.input_id, question.input_version)?
        .ok_or_else(invalid)?;
    let goal_ref = revision.goal_sha256.as_deref().ok_or_else(invalid)?;
    let goal: GoalContract = serde_json::from_slice(&store.get_bytes(actor, case, goal_ref)?)?;
    let work_plan = matches!(
        original.meaning.intent,
        Intent::ClarifyWorkPlanPersonnel { .. }
    );
    let valid_source = if work_plan {
        input::verified_work_plan_input(&original)?
    } else {
        original.meaning.parser_version == input::HEARING_PARSER_VERSION
            && matches!(
                original.meaning.intent,
                Intent::ClarifyPreviousLength | Intent::ClarifyFilteredCsvStatistics
            )
    };
    if !valid_source
        || identity(&original.request)? != question.id
        || original.request.actor_id != actor
        || original.request.case_id != case
        || revision.bundle.as_ref() != Some(&original)
        || revision.reason != question.reason
        || goal.intent != original.meaning.intent
        || goal.input_fingerprint != question.id
        || goal.output_path != original.request.output_path
        || goal.permissions != original.request.permissions
        || goal.budget != original.request.budget
        || goal
            .output_path
            .as_ref()
            .is_some_and(|path| !goal.permissions.write_paths.contains(path))
        || !is_question(question)
        || !question.receipts.is_empty()
        || !question.evidence.is_empty()
    {
        return Err(invalid());
    }
    let prompt_ref = question.dependencies.last().ok_or_else(invalid)?.clone();
    let prompt_bytes = store.get_bytes(actor, case, &prompt_ref)?;
    let prompt: Prompt = serde_json::from_slice(&prompt_bytes)?;
    let expected = prompt.preview(&prompt_ref);
    let plan = question.content_plan.as_ref().ok_or_else(invalid)?;
    let source_matches = match (&original.meaning.intent, &prompt.task) {
        (Intent::ClarifyWorkPlanPersonnel { draft }, Task::WorkPlanPersonnel { units }) => {
            let preview = work_plan::personnel_preview(draft)?;
            prompt.source_ref == original.fingerprint
                && prompt.dependencies.is_empty()
                && needs_work_plan_personnel(draft, &preview)
                && *units == work_plan_question_units(draft, &preview)
                && prompt_bytes.len().saturating_add(units.join("\n").len())
                    <= original.request.budget.max_evidence_bytes
        }
        (Intent::ClarifyPreviousLength, Task::Length { .. })
        | (Intent::ClarifyFilteredCsvStatistics, Task::FilteredCsv { .. }) => {
            original.meaning.context_result_ref.as_deref() == Some(prompt.source_ref.as_str())
        }
        _ => false,
    };
    if prompt.kind != PROMPT_KIND
        || !source_matches
        || expected.dependencies != question.dependencies
        || expected.calculation != question.calculation
        || expected.units != plan.required_units
        || plan
            .dependencies
            .strip_prefix(question.dependencies.as_slice())
            .is_none_or(|rest| rest.len() != 1)
        || plan.guarantee != Guarantee::Computed
        || question.guarantee != plan.guarantee
        || question.text != plan.text
        || question.render.as_ref() != Some(&output::render_to(plan, RenderTarget::Display)?)
    {
        return Err(invalid());
    }
    Ok(AnswerContext {
        question: question.clone(),
        original,
        prompt,
        prompt_ref,
    })
}

pub(crate) fn is_question(result: &ResultRecord) -> bool {
    result.lifecycle == Lifecycle::Suspend
        && result.reason.as_deref().is_some_and(|reason| {
            reason
                .split_once(':')
                .is_some_and(|(code, _)| code == REQUIRED)
        })
}

const MAX_ANSWER_BYTES: usize = 256;

/// A historical context ID alone cannot reactivate a consumed or
/// cancelled question. The case journal is already authenticated by Store.
fn active_questions(
    store: &Store,
    request: &RequestEnvelope,
    started: Instant,
    mut visit: impl FnMut(&ResultRecord) -> Result<bool>,
) -> Result<()> {
    let Some((head, order)) =
        store.load_case_with_history_order(&request.actor_id, &request.case_id)?
    else {
        return Ok(());
    };
    let mut answered = BTreeSet::new();
    let mut issued_effects = None;
    for result in head
        .result
        .iter()
        .chain(order.iter().filter_map(|id| head.prior_results.get(id)))
    {
        if result.lifecycle == Lifecycle::Cancel
            || started.elapsed() >= Duration::from_millis(request.budget.deadline_ms)
        {
            break;
        }
        if let [.., question_ref, _] = result.dependencies.as_slice()
            && head
                .prior_results
                .get(question_ref)
                .is_some_and(is_question)
        {
            let consumed = if result.lifecycle == Lifecycle::Commit {
                true
            } else if result.lifecycle == Lifecycle::Suspend {
                if issued_effects.is_none() {
                    issued_effects = Some(store.actions(&request.actor_id, &request.case_id)?);
                }
                issued_effects.as_ref().is_some_and(|actions| {
                    actions.iter().any(|action| {
                        action.input_fingerprint == result.id
                            && matches!(
                                action.phase,
                                EffectPhase::Attempted
                                    | EffectPhase::Confirmed
                                    | EffectPhase::Unknown
                            )
                    })
                })
            } else {
                false
            };
            if consumed
                && let Some((_, bundle)) = store.get_input_bundle(
                    &request.actor_id,
                    &request.case_id,
                    &result.input_id,
                    result.input_version,
                )?
                && let Intent::SupplyHearingAnswer { question_ref, .. } = bundle.meaning.intent
            {
                answered.insert(question_ref);
            }
        }
        if is_question(result) && !answered.contains(&result.id) && !visit(result)? {
            break;
        }
    }
    Ok(())
}

pub(crate) fn find_answer(
    store: &Store,
    request: &RequestEnvelope,
    started: Instant,
    personnel_only: bool,
) -> Result<Option<(AnswerContext, SourceSpan)>> {
    if request.text.len() > MAX_ANSWER_BYTES
        || request.text.chars().any(|ch| ch.is_control() && ch != '\t')
    {
        return Ok(None);
    }
    let mut matched = None;
    active_questions(store, request, started, |result| {
        if request
            .context_ref
            .as_ref()
            .is_some_and(|id| id != &result.id)
        {
            return Ok(true);
        }
        let context = read_question(store, &request.actor_id, &request.case_id, result)?;
        if !context.admits_answer(request) || (personnel_only && !context.is_work_plan_personnel())
        {
            return Ok(true);
        }
        let Some(span) = context.prompt.answer_span(&request.text) else {
            return Ok(true);
        };
        if matched.is_some() {
            // An implicit short answer must identify one pending question.
            matched = None;
            return Ok(false);
        }
        matched = Some((context, span));
        // An explicit result identity has no competing candidates.
        Ok(request.context_ref.is_none())
    })?;
    if started.elapsed() >= Duration::from_millis(request.budget.deadline_ms) {
        return Ok(None);
    }
    Ok(matched)
}

pub(crate) fn equivalent_question(
    store: &Store,
    bundle: &InputBundle,
    previous: Option<&ResultRecord>,
    started: Instant,
) -> Result<Option<String>> {
    let independent = matches!(
        bundle.meaning.intent,
        Intent::ClarifyWorkPlanPersonnel { .. }
    );
    if previous.is_none() && !independent {
        return Ok(None);
    }
    let request = &bundle.request;
    let mut existing = None;
    active_questions(store, request, started, |result| {
        let context = read_question(store, &request.actor_id, &request.case_id, result)?;
        if context.original.meaning.intent == bundle.meaning.intent
            && context.original.request.output_path == request.output_path
            && (independent
                || previous.is_some_and(|prior| {
                    context.prompt.source_ref == prior.id || result.id == prior.id
                }))
        {
            existing = Some(result.id.clone());
            return Ok(false);
        }
        Ok(true)
    })?;
    Ok(existing)
}

pub(crate) fn restore_answer(
    store: &Store,
    bundle: &InputBundle,
    prior_results: &BTreeMap<String, ResultRecord>,
) -> Result<AnswerContext> {
    let Intent::SupplyHearingAnswer {
        question_ref,
        answer,
    } = &bundle.meaning.intent
    else {
        return Err(invalid());
    };
    let question = prior_results.get(question_ref).ok_or_else(invalid)?;
    let context = read_question(
        store,
        &bundle.request.actor_id,
        &bundle.request.case_id,
        question,
    )?;
    if !context.admits_answer(&bundle.request)
        || context.prompt.answer_span(&bundle.request.text).as_ref() != Some(answer)
        || input::seal_hearing_answer(
            &bundle.request,
            question,
            answer.clone(),
            context.is_work_plan_personnel(),
        )? != *bundle
    {
        return Err(invalid());
    }
    Ok(context)
}

impl AnswerContext {
    pub(crate) fn is_work_plan_personnel(&self) -> bool {
        matches!(self.prompt.task, Task::WorkPlanPersonnel { .. })
    }

    fn admits_answer(&self, request: &RequestEnvelope) -> bool {
        !self.is_work_plan_personnel()
            || (request.attachments.is_empty()
                && request.speech.is_none()
                && request.purpose.is_none())
    }

    pub(crate) fn effective_budget(&self, request: &RequestEnvelope) -> ResourceBudget {
        let mut budget = request.budget.clone();
        if self.is_work_plan_personnel() {
            let original = &self.original.request.budget;
            budget.max_actions = budget.max_actions.min(original.max_actions);
            budget.max_evidence_bytes = budget.max_evidence_bytes.min(original.max_evidence_bytes);
            budget.deadline_ms = budget.deadline_ms.min(original.deadline_ms);
        }
        budget
    }

    pub(crate) fn question(&self) -> &ResultRecord {
        &self.question
    }

    pub(crate) fn output_path(&self, request: &RequestEnvelope) -> Result<Option<PathBuf>> {
        let saved = &self.original.request.output_path;
        if request
            .output_path
            .as_ref()
            .is_some_and(|path| saved.as_ref() != Some(path))
        {
            return Err(failure(
                "HEARING_OUTPUT_TARGET_CHANGED",
                "補足では元の出力先を変更できません。",
            ));
        }
        if saved
            .as_ref()
            .is_some_and(|path| !request.permissions.write_paths.contains(path))
        {
            return Err(failure(
                "WRITE_PERMISSION_MISSING",
                "元の出力先には、この発話でも書き込み許可が必要です。",
            ));
        }
        Ok(saved.clone())
    }

    pub(crate) fn complete(
        &self,
        store: &Store,
        bundle: &InputBundle,
        prior_results: &BTreeMap<String, ResultRecord>,
        started: Instant,
    ) -> Result<Outcome> {
        let Intent::SupplyHearingAnswer {
            question_ref,
            answer,
        } = &bundle.meaning.intent
        else {
            return Err(invalid());
        };
        if question_ref != &self.question.id {
            return Err(invalid());
        }
        if let (Task::WorkPlanPersonnel { .. }, Intent::ClarifyWorkPlanPersonnel { draft }) =
            (&self.prompt.task, &self.original.meaning.intent)
        {
            let (_, personnel) = work_plan_input::personnel_answer(&bundle.request.text)
                .filter(|(span, _)| span == answer)
                .ok_or_else(invalid)?;
            let budget = self.effective_budget(&bundle.request);
            let deadline = started
                .checked_add(Duration::from_millis(budget.deadline_ms))
                .ok_or_else(invalid)?;
            let plan = work_plan::WorkPlanMeaningPlan::new(
                draft.clone().with_personnel(personnel),
                self.original.fingerprint.clone(),
                deadline,
            )?;
            let mut outcome = plan_outcome(
                &self.original,
                Some((&bundle.fingerprint, answer)),
                &plan,
                &budget,
            )?;
            outcome
                .dependencies
                .extend([self.question.id.clone(), self.prompt_ref.clone()]);
            return Ok(outcome);
        }
        let previous = prior_results
            .get(&self.prompt.source_ref)
            .ok_or_else(invalid)?;
        if previous.lifecycle != Lifecycle::Commit
            || self.original.context_sha256 != identity(&Some(previous))?
        {
            return Err(invalid());
        }
        let request = &bundle.request;
        let original_budget = &self.original.request.budget;
        let deadline_ms = original_budget.deadline_ms.min(request.budget.deadline_ms);
        let evidence_bytes = original_budget
            .max_evidence_bytes
            .min(request.budget.max_evidence_bytes);
        let mut outcome = match &self.prompt.task {
            Task::Length {
                value,
                provenance,
                evidence_bytes: source_bytes,
            } => {
                let target = input::hearing_length_target(&answer.text).ok_or_else(invalid)?;
                let calculation = language::convert_exact_length(value, target)?;
                let mut units = vec![core::exact_calculation_unit(&calculation)];
                units.extend(provenance.iter().cloned());
                if source_bytes.saturating_add(units.join("\n").len()) > evidence_bytes {
                    return Err(failure(
                        "RESOURCE_LIMIT",
                        "元の根拠と追加の換算結果が証拠予算を超えています。",
                    ));
                }
                Outcome {
                    units,
                    calculation: Some(calculation),
                    dependencies: self.prompt.dependencies.clone(),
                    report_bytes: None,
                }
            }
            Task::FilteredCsv { columns, .. } => {
                let (_, column) = csv_answer(columns, &bundle.request.text).ok_or_else(invalid)?;
                // This local copy supplies stricter runtime limits to the existing
                // evaluator. The registered original request stays intact.
                let mut source = self.original.clone();
                let budget = &mut source.request.budget;
                budget.deadline_ms = deadline_ms;
                budget.max_evidence_bytes = evidence_bytes;
                budget.max_actions = original_budget.max_actions.min(request.budget.max_actions);
                csv_outcome(document::filtered_csv_statistics_with_column(
                    store, &source, previous, column, started,
                )?)
            }
            Task::WorkPlanPersonnel { .. } => return Err(invalid()),
        };
        if started.elapsed() >= Duration::from_millis(deadline_ms) {
            return Err(failure(
                "DEADLINE",
                "補足を適用する途中で締切へ達しました。",
            ));
        }
        outcome
            .dependencies
            .extend([self.question.id.clone(), self.prompt_ref.clone()]);
        Ok(outcome)
    }
}
