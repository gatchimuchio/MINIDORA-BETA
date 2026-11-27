//! Scoped proof-preserving relations. Source premises are never world facts.
//! Declared rules support one or two premises; internal shortcuts are bounded
//! to three antecedents and retain their parent proof DAG.
use crate::store::{IndexedMemoryRecord, IndexedMemoryValue, MemoryIndexSnapshot};
use minidora_domain::*;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, VecDeque};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct QueryOutcome {
    pub status: String,
    pub facts: Vec<Fact>,
    pub used_rules: Vec<String>,
    pub steps: u32,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub condition_evaluations: Vec<ConditionEvaluation>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub condition_status: Option<ConditionStatus>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RelationInventory {
    pub facts: Vec<Fact>,
    pub contested_fact_ids: BTreeSet<String>,
    pub has_conflict: bool,
    pub dependencies: Vec<String>,
    pub guarantee: Guarantee,
}

/// Return active, directly supplied premises for one subject. Derived facts
/// are deliberately excluded: callers can present exactly what entered memory
/// without describing inference as an observation.
pub fn explicit_premises_about(memory: &MemoryState, subject: &str) -> Vec<Fact> {
    let mut facts: Vec<_> = memory
        .facts
        .iter()
        .filter(|fact| {
            fact.valid
                && fact.guarantee == Guarantee::UserPremise
                && fact.dependencies.is_empty()
                && !fact.source_input.is_empty()
                && fact.proposition.subject == subject
        })
        .cloned()
        .collect();
    facts.sort_by(|left, right| {
        left.proposition
            .cmp(&right.proposition)
            .then_with(|| left.id.cmp(&right.id))
    });
    facts
}

/// Build the bounded set of exact relation queries that can return direct or
/// rule-derived facts for one subject. Predicate and scope come only from
/// stored facts or rule conclusions. A variable conclusion object is expanded
/// only over terms already present in the same actor's memory.
pub fn query_shapes_about(
    memory: &MemoryState,
    subject: &str,
    max_queries: usize,
) -> Result<BTreeSet<Proposition>> {
    let mut terms = BTreeSet::from([subject.to_string()]);
    for fact in &memory.facts {
        if !fact.valid || matches!(fact.guarantee, Guarantee::Hypothesis | Guarantee::Unknown) {
            continue;
        }
        for term in [&fact.proposition.subject, &fact.proposition.object] {
            if !variable(term) {
                terms.insert(term.clone());
            }
        }
    }
    for rule in memory.rules.iter().filter(|rule| rule.valid) {
        for proposition in std::iter::once(&rule.premise)
            .chain(rule.additional_premises.iter())
            .chain(std::iter::once(&rule.conclusion))
        {
            for term in [&proposition.subject, &proposition.object] {
                if !variable(term) {
                    terms.insert(term.clone());
                }
            }
        }
    }

    let mut shapes = BTreeSet::new();
    for fact in &memory.facts {
        if fact.valid
            && !matches!(fact.guarantee, Guarantee::Hypothesis | Guarantee::Unknown)
            && fact.proposition.subject == subject
        {
            insert_query_shape(&mut shapes, &fact.proposition, max_queries)?;
        }
    }
    for rule in memory.rules.iter().filter(|rule| rule.valid) {
        let conclusion = &rule.conclusion;
        if conclusion.subject != subject && !variable(&conclusion.subject) {
            continue;
        }
        if variable(&conclusion.object) {
            for object in &terms {
                insert_query_shape(
                    &mut shapes,
                    &Proposition {
                        subject: subject.to_string(),
                        object: object.clone(),
                        negative: false,
                        ..conclusion.clone()
                    },
                    max_queries,
                )?;
            }
        } else {
            insert_query_shape(
                &mut shapes,
                &Proposition {
                    subject: subject.to_string(),
                    negative: false,
                    ..conclusion.clone()
                },
                max_queries,
            )?;
        }
    }
    Ok(shapes)
}

/// Build the current actor-scoped, bounded supported relation inventory for a
/// subject. Consumers retain exact facts, conflicts, dependencies and the
/// strongest guarantee found; no relation is promoted to outside-world truth.
pub fn inventory_about(
    memory: &MemoryState,
    subject: &str,
    max_actions: u32,
) -> Result<RelationInventory> {
    let scanned_records = memory
        .facts
        .len()
        .checked_add(memory.rules.len())
        .ok_or_else(|| {
            failure(
                "RELATION_BUDGET",
                "relation inventory scan count overflowed",
            )
        })?;
    let action_budget = usize::try_from(max_actions).unwrap_or(usize::MAX);
    if scanned_records > action_budget {
        return Err(failure(
            "RELATION_BUDGET",
            "relation inventory scan exceeds the original action budget",
        ));
    }
    let remaining_actions =
        max_actions.saturating_sub(u32::try_from(scanned_records).unwrap_or(u32::MAX));
    let query_shapes = query_shapes_about(
        memory,
        subject,
        usize::try_from(remaining_actions).unwrap_or(usize::MAX),
    )?;
    if query_shapes.is_empty() {
        return Err(failure(
            "MISSING_PREMISES",
            "no active supported relation query applies to this subject",
        ));
    }
    let query_count = u32::try_from(query_shapes.len())
        .map_err(|_| failure("RELATION_BUDGET", "relation query count overflowed"))?;
    let per_query_budget = remaining_actions / query_count;
    if per_query_budget == 0 {
        return Err(failure(
            "RELATION_BUDGET",
            "no proof budget remains for every relation query",
        ));
    }

    let mut dependencies = BTreeSet::new();
    let mut reported_facts = BTreeMap::new();
    let mut contested_fact_ids = BTreeSet::new();
    let mut has_conflict = false;
    for shape in query_shapes {
        let answer = query(memory, &shape, per_query_budget)?;
        match answer.status.as_str() {
            "SUPPORTED" | "CONTRADICTED" => {}
            "CONFLICT" => {
                has_conflict = true;
                contested_fact_ids.extend(answer.facts.iter().map(|fact| fact.id.clone()));
            }
            "UNKNOWN" | "MISSING_PREMISES" => continue,
            _ => {
                return Err(failure(
                    "UNSUPPORTED_RELATION_RESULT",
                    "relation inventory proof returned an unknown terminal status",
                ));
            }
        }
        dependencies.extend(answer_dependencies(&answer));
        for fact in answer.facts {
            reported_facts.insert(fact.id.clone(), fact);
        }
    }
    if reported_facts.is_empty() {
        return Err(failure(
            "MISSING_PREMISES",
            "no supported relation result remains for this subject",
        ));
    }
    let guarantee = if reported_facts
        .values()
        .any(|fact| matches!(fact.guarantee, Guarantee::Derived))
    {
        Guarantee::Derived
    } else if reported_facts
        .values()
        .any(|fact| matches!(fact.guarantee, Guarantee::UserPremise))
    {
        Guarantee::UserPremise
    } else if reported_facts
        .values()
        .any(|fact| matches!(fact.guarantee, Guarantee::Computed))
    {
        Guarantee::Computed
    } else if reported_facts
        .values()
        .any(|fact| matches!(fact.guarantee, Guarantee::SourceReport))
    {
        Guarantee::SourceReport
    } else {
        Guarantee::Unknown
    };
    let mut facts: Vec<_> = reported_facts.into_values().collect();
    facts.sort_by(|left, right| {
        left.proposition
            .cmp(&right.proposition)
            .then_with(|| left.id.cmp(&right.id))
    });
    Ok(RelationInventory {
        facts,
        contested_fact_ids,
        has_conflict,
        dependencies: dependencies.into_iter().collect(),
        guarantee,
    })
}

/// Recovery uses the same inventory computation to ensure an adopted listing
/// still describes the actor memory that originally supported it.
pub fn inventory_dependencies_and_guarantee(
    memory: &MemoryState,
    subject: &str,
    max_actions: u32,
) -> Result<(Vec<String>, Guarantee)> {
    let inventory = inventory_about(memory, subject, max_actions)?;
    Ok((inventory.dependencies, inventory.guarantee))
}

fn insert_query_shape(
    shapes: &mut BTreeSet<Proposition>,
    proposition: &Proposition,
    max_queries: usize,
) -> Result<()> {
    let shape = Proposition {
        negative: false,
        ..proposition.clone()
    };
    if shapes.insert(shape) && shapes.len() > max_queries {
        return Err(failure(
            "RELATION_BUDGET",
            "relation inventory exceeds the bounded query budget",
        ));
    }
    Ok(())
}

fn variable(term: &str) -> bool {
    term.starts_with('$') || term.starts_with('?')
}
fn variables(proposition: &Proposition) -> BTreeSet<String> {
    [&proposition.subject, &proposition.object]
        .into_iter()
        .filter(|term| variable(term))
        .cloned()
        .collect()
}
pub(crate) fn validate(proposition: &Proposition) -> Result<()> {
    if [
        &proposition.subject,
        &proposition.predicate,
        &proposition.object,
        &proposition.scope,
    ]
    .iter()
    .any(|value| value.trim().is_empty() || value.len() > 1024)
    {
        return Err(failure(
            "INVALID_RELATION",
            "relation fields must be nonempty and bounded",
        ));
    }
    if proposition.temporal_scope.as_deref().is_some_and(|value| {
        value.trim().is_empty()
            || value.len() > 1024
            || value.chars().any(char::is_control)
            || variable(value)
    }) {
        return Err(failure(
            "INVALID_TIME_SCOPE",
            "an explicit temporal scope must be nonempty, bounded, non-variable, and contain no control characters",
        ));
    }
    if proposition.condition_refs.len() > 32
        || proposition.condition_refs.iter().any(|value| {
            value.trim().is_empty()
                || value.len() > 1024
                || value.chars().any(char::is_control)
                || variable(value)
        })
    {
        return Err(failure(
            "INVALID_CONDITION_REFS",
            "condition references must be a bounded set of nonempty, non-variable values without control characters",
        ));
    }
    if variable(&proposition.predicate) || variable(&proposition.scope) {
        return Err(failure(
            "UNSUPPORTED_RELATION",
            "predicate and scope variables are not supported",
        ));
    }
    Ok(())
}

#[derive(Default)]
struct ConditionEvaluationState {
    status: Option<ConditionStatus>,
    definition_record_ids: BTreeSet<String>,
    evidence_fact_ids: BTreeSet<String>,
    dependencies: BTreeSet<String>,
    reason: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub(crate) struct ConditionTraceStep {
    pub(crate) parent: Option<usize>,
    pub(crate) target: ConditionTraceTarget,
    pub(crate) status: ConditionStatus,
    pub(crate) reason: Option<String>,
    pub(crate) positive_fact_ids: Vec<String>,
    pub(crate) negative_fact_ids: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "SCREAMING_SNAKE_CASE")]
pub(crate) enum ConditionTraceTarget {
    Condition {
        condition_id: String,
    },
    Fact {
        subject: String,
        predicate: String,
        object: String,
    },
    And,
    Or,
    Not,
}

#[derive(Clone, Copy)]
struct ConditionTarget<'a> {
    subject: &'a str,
    scope: &'a str,
    temporal_scope: Option<&'a str>,
}

impl<'a> From<&'a Proposition> for ConditionTarget<'a> {
    fn from(proposition: &'a Proposition) -> Self {
        Self {
            subject: &proposition.subject,
            scope: &proposition.scope,
            temporal_scope: proposition.temporal_scope.as_deref(),
        }
    }
}

type ConditionEvidenceKey<'a> = (&'a str, &'a str, &'a str, &'a str);

struct ConditionTrace<'a> {
    steps: &'a mut Vec<ConditionTraceStep>,
    parent: Option<usize>,
}

struct ConditionEvaluationBudget<'a, 'memory> {
    max_steps: u32,
    steps: &'a mut u32,
    projected_evidence: Option<&'a [usize]>,
    evidence: Option<BTreeMap<ConditionEvidenceKey<'memory>, Vec<&'memory Fact>>>,
    trace: Option<ConditionTrace<'a>>,
}

impl<'memory> ConditionEvaluationBudget<'_, 'memory> {
    fn trace_evaluation(
        &mut self,
        target: impl FnOnce() -> Option<ConditionTraceTarget>,
        evaluate: impl FnOnce(&mut Self) -> Result<ConditionEvaluationState>,
    ) -> Result<ConditionEvaluationState> {
        let Some(trace) = self.trace.as_mut() else {
            return evaluate(self);
        };
        let Some(target) = target() else {
            return evaluate(self);
        };

        // A caller charges this evaluation before opening its preorder row.
        let position = trace.steps.len();
        let parent = trace.parent.replace(position);
        trace.steps.push(ConditionTraceStep {
            parent,
            target,
            status: ConditionStatus::Unknown,
            reason: None,
            positive_fact_ids: Vec::new(),
            negative_fact_ids: Vec::new(),
        });
        let result = evaluate(self);
        if let Some(trace) = self.trace.as_mut() {
            trace.parent = parent;
            if let Ok(state) = &result {
                let step = &mut trace.steps[position];
                step.status = state.status.unwrap_or(ConditionStatus::Unknown);
                step.reason.clone_from(&state.reason);
                step.positive_fact_ids.sort_unstable();
                step.negative_fact_ids.sort_unstable();
            }
        }
        result
    }

    fn trace_admitted_fact(&mut self, fact: &Fact) {
        if let Some(trace) = self.trace.as_mut()
            && let Some(position) = trace.parent
        {
            let step = &mut trace.steps[position];
            let evidence = if fact.proposition.negative {
                &mut step.negative_fact_ids
            } else {
                &mut step.positive_fact_ids
            };
            evidence.push(fact.id.clone());
        }
    }

    fn matching_evidence(
        &mut self,
        memory: &'memory MemoryState,
        temporal_scope: Option<&str>,
        allowed_hypotheses: Option<&BTreeSet<String>>,
        key: ConditionEvidenceKey<'memory>,
    ) -> Result<Vec<&'memory Fact>> {
        if self.evidence.is_none() {
            // One charged pass for this query's immutable facts. A fact's
            // own conditions are checked later on the active recursion path.
            let mut index = BTreeMap::<_, Vec<_>>::new();
            let count = self
                .projected_evidence
                .map_or(memory.facts.len(), <[usize]>::len);
            for position in 0..count {
                let fact_index = self
                    .projected_evidence
                    .map_or(position, |indices| indices[position]);
                let fact = &memory.facts[fact_index];
                charge_step(self.steps, self.max_steps)?;
                let proposition = &fact.proposition;
                if !fact.valid
                    || fact.guarantee == Guarantee::Unknown
                    || (fact.guarantee == Guarantee::Hypothesis
                        && !allowed_hypotheses.is_some_and(|ids| ids.contains(&fact.id)))
                    || proposition.temporal_scope.as_deref() != temporal_scope
                    || fact
                        .dependencies
                        .iter()
                        .any(|id| !active_memory_dependency(memory, id))
                {
                    continue;
                }
                index
                    .entry((
                        proposition.subject.as_str(),
                        proposition.predicate.as_str(),
                        proposition.object.as_str(),
                        proposition.scope.as_str(),
                    ))
                    .or_default()
                    .push(fact);
            }
            self.evidence = Some(index);
        }
        Ok(self
            .evidence
            .as_ref()
            .and_then(|index| index.get(&key))
            .cloned()
            .unwrap_or_default())
    }
}

pub(crate) fn valid_utc_instant(value: &str) -> bool {
    let bytes = value.as_bytes();
    if bytes.len() != 20
        || bytes[4] != b'-'
        || bytes[7] != b'-'
        || bytes[10] != b'T'
        || bytes[13] != b':'
        || bytes[16] != b':'
        || bytes[19] != b'Z'
        || [0..4, 5..7, 8..10, 11..13, 14..16, 17..19]
            .into_iter()
            .any(|range| !bytes[range].iter().all(u8::is_ascii_digit))
    {
        return false;
    }
    let number = |range: std::ops::Range<usize>| -> Option<u32> { value.get(range)?.parse().ok() };
    let (Some(year), Some(month), Some(day), Some(hour), Some(minute), Some(second)) = (
        number(0..4),
        number(5..7),
        number(8..10),
        number(11..13),
        number(14..16),
        number(17..19),
    ) else {
        return false;
    };
    if year == 0 || !(1..=12).contains(&month) || hour > 23 || minute > 59 || second > 59 {
        return false;
    }
    let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
    let days = match month {
        2 if leap => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    };
    (1..=days).contains(&day)
}

pub(crate) const MAX_CONDITION_EXPRESSION_NODES: usize = 64;

fn expression_condition_refs(
    expression: &ConditionExpression,
    node_count: &mut usize,
    references: &mut BTreeSet<String>,
) -> Result<()> {
    *node_count = node_count.checked_add(1).ok_or_else(|| {
        failure(
            "CONDITION_COMPLEXITY_LIMIT",
            "condition node count overflow",
        )
    })?;
    if *node_count > MAX_CONDITION_EXPRESSION_NODES {
        return Err(failure(
            "CONDITION_COMPLEXITY_LIMIT",
            "condition expressions are limited to 64 nodes",
        ));
    }
    match expression {
        ConditionExpression::Fact { predicate, object } => {
            if [predicate, object].iter().any(|value| {
                value.trim().is_empty()
                    || value.len() > 1024
                    || value.chars().any(char::is_control)
                    || variable(value)
            }) {
                return Err(failure(
                    "INVALID_CONDITION_PREDICATE",
                    "condition fact predicates require bounded, non-variable predicate and object values",
                ));
            }
        }
        ConditionExpression::ConditionRef { condition_id } => {
            validate_condition_id(condition_id)?;
            references.insert(condition_id.clone());
        }
        ConditionExpression::And { operands } | ConditionExpression::Or { operands } => {
            if !(2..=16).contains(&operands.len()) {
                return Err(failure(
                    "INVALID_CONDITION_ARITY",
                    "AND and OR conditions require two to sixteen operands",
                ));
            }
            for operand in operands {
                expression_condition_refs(operand, node_count, references)?;
            }
        }
        ConditionExpression::Not { operand } => {
            expression_condition_refs(operand, node_count, references)?;
        }
    }
    Ok(())
}

pub(crate) fn validate_condition_id(value: &str) -> Result<()> {
    if value.trim().is_empty()
        || value.len() > 512
        || value.chars().any(char::is_control)
        || variable(value)
    {
        return Err(failure(
            "INVALID_CONDITION_ID",
            "condition IDs must be nonempty, bounded, and non-variable",
        ));
    }
    Ok(())
}

pub fn validate_condition_definition(definition: &ConditionDefinition) -> Result<()> {
    validate_condition_id(&definition.condition_id)?;
    if definition.subject != "$subject"
        && (definition.subject.trim().is_empty()
            || definition.subject.len() > 1024
            || definition.subject.chars().any(char::is_control)
            || variable(&definition.subject))
    {
        return Err(failure(
            "INVALID_CONDITION_SUBJECT",
            "condition subject must be a bounded exact subject or $subject",
        ));
    }
    if definition.scope.trim().is_empty()
        || definition.scope.len() > 1024
        || definition.scope.chars().any(char::is_control)
        || variable(&definition.scope)
    {
        return Err(failure(
            "INVALID_CONDITION_SCOPE",
            "condition scope must be a bounded exact scope",
        ));
    }
    for bound in [
        definition.valid_from.as_deref(),
        definition.valid_until.as_deref(),
    ]
    .into_iter()
    .flatten()
    {
        if !valid_utc_instant(bound) {
            return Err(failure(
                "INVALID_CONDITION_PERIOD",
                "condition validity bounds require canonical YYYY-MM-DDTHH:MM:SSZ UTC instants",
            ));
        }
    }
    if definition
        .valid_from
        .as_ref()
        .zip(definition.valid_until.as_ref())
        .is_some_and(|(start, end)| start >= end)
    {
        return Err(failure(
            "INVALID_CONDITION_PERIOD",
            "condition validity is a nonempty half-open interval [valid_from, valid_until)",
        ));
    }
    let mut references = BTreeSet::new();
    expression_condition_refs(&definition.expression, &mut 0, &mut references)?;
    if references != definition.dependencies {
        return Err(failure(
            "CONDITION_DEPENDENCY_MISMATCH",
            "declared condition dependencies must equal the expression's referenced condition IDs",
        ));
    }
    if definition.dependencies.len() > 32 {
        return Err(failure(
            "CONDITION_COMPLEXITY_LIMIT",
            "a condition may depend on at most 32 other conditions",
        ));
    }
    Ok(())
}

pub fn define_condition(
    memory: &mut MemoryState,
    definition: ConditionDefinition,
    source_input: &str,
) -> Result<ConditionRecord> {
    validate_condition_definition(&definition)?;
    if source_input.is_empty() {
        return Err(failure(
            "MISSING_PROVENANCE",
            "a condition definition needs its original input identity",
        ));
    }
    if memory
        .facts
        .iter()
        .any(|fact| fact.id == definition.condition_id)
        || memory
            .rules
            .iter()
            .any(|rule| rule.id == definition.condition_id)
        || memory
            .conditions
            .iter()
            .any(|condition| condition.id == definition.condition_id)
    {
        return Err(failure(
            "CONDITION_ID_COLLISION",
            "condition IDs occupy a separate identity space from memory record IDs",
        ));
    }
    if let Some(existing) = memory
        .conditions
        .iter()
        .find(|record| record.definition.condition_id == definition.condition_id)
    {
        if existing.definition == definition && existing.valid {
            return Ok(existing.clone());
        }
        return Err(failure(
            "CONDITION_ID_COLLISION",
            "condition IDs are immutable; define a revised condition under a new ID",
        ));
    }
    let mut dependency_records = Vec::with_capacity(definition.dependencies.len());
    for dependency in &definition.dependencies {
        let mut matches = memory
            .conditions
            .iter()
            .filter(|record| record.valid && record.definition.condition_id == *dependency);
        let Some(record) = matches.next() else {
            return Err(failure(
                "UNKNOWN_CONDITION_DEPENDENCY",
                format!("condition dependency {dependency:?} is not defined and active"),
            ));
        };
        if matches.next().is_some() {
            return Err(failure(
                "AMBIGUOUS_CONDITION_DEPENDENCY",
                format!("condition dependency {dependency:?} has multiple active definitions"),
            ));
        }
        dependency_records.push(record.id.clone());
    }
    dependency_records.sort();
    let id = format!(
        "condition-record:{}",
        identity(&(CONDITION_SEMANTICS_VERSION, &definition))?
    );
    let record = ConditionRecord {
        id,
        semantics_version: CONDITION_SEMANTICS_VERSION.into(),
        definition,
        source_input: source_input.into(),
        dependencies: dependency_records,
        valid: true,
    };
    memory.conditions.push(record.clone());
    bump(memory)?;
    Ok(record)
}

fn status_support(status: ConditionStatus) -> (bool, bool) {
    match status {
        ConditionStatus::True => (true, false),
        ConditionStatus::False => (false, true),
        ConditionStatus::Unknown => (false, false),
        ConditionStatus::Conflict => (true, true),
    }
}

fn support_status(true_support: bool, false_support: bool) -> ConditionStatus {
    match (true_support, false_support) {
        (true, false) => ConditionStatus::True,
        (false, true) => ConditionStatus::False,
        (false, false) => ConditionStatus::Unknown,
        (true, true) => ConditionStatus::Conflict,
    }
}

fn combine_and(left: ConditionStatus, right: ConditionStatus) -> ConditionStatus {
    let (left_true, left_false) = status_support(left);
    let (right_true, right_false) = status_support(right);
    support_status(left_true && right_true, left_false || right_false)
}

fn combine_or(left: ConditionStatus, right: ConditionStatus) -> ConditionStatus {
    let (left_true, left_false) = status_support(left);
    let (right_true, right_false) = status_support(right);
    support_status(left_true || right_true, left_false && right_false)
}

fn negate_status(status: ConditionStatus) -> ConditionStatus {
    let (true_support, false_support) = status_support(status);
    support_status(false_support, true_support)
}

fn active_memory_dependency(memory: &MemoryState, id: &str) -> bool {
    memory.facts.iter().any(|fact| fact.id == id && fact.valid)
        || memory.rules.iter().any(|rule| rule.id == id && rule.valid)
        || memory
            .conditions
            .iter()
            .any(|record| record.id == id && record.valid)
}

fn evaluate_condition_expression<'memory>(
    memory: &'memory MemoryState,
    expression: &'memory ConditionExpression,
    definition: &'memory ConditionDefinition,
    target: ConditionTarget<'memory>,
    allowed_hypotheses: Option<&BTreeSet<String>>,
    budget: &mut ConditionEvaluationBudget<'_, 'memory>,
    visiting: &mut BTreeSet<String>,
) -> Result<ConditionEvaluationState> {
    charge_step(budget.steps, budget.max_steps)?;
    budget.trace_evaluation(
        || match expression {
            ConditionExpression::Fact { predicate, object } => {
                let subject = if definition.subject == "$subject" {
                    target.subject
                } else {
                    definition.subject.as_str()
                };
                Some(ConditionTraceTarget::Fact {
                    subject: subject.to_owned(),
                    predicate: predicate.clone(),
                    object: object.clone(),
                })
            }
            ConditionExpression::ConditionRef { .. } => None,
            ConditionExpression::And { .. } => Some(ConditionTraceTarget::And),
            ConditionExpression::Or { .. } => Some(ConditionTraceTarget::Or),
            ConditionExpression::Not { .. } => Some(ConditionTraceTarget::Not),
        },
        |budget| match expression {
            ConditionExpression::Fact { predicate, object } => {
                let evidence_subject = if definition.subject == "$subject" {
                    target.subject
                } else {
                    definition.subject.as_str()
                };
                let mut true_support = false;
                let mut false_support = false;
                let mut uncertain_evidence = false;
                let mut result = ConditionEvaluationState::default();
                let key = (
                    evidence_subject,
                    predicate.as_str(),
                    object.as_str(),
                    definition.scope.as_str(),
                );
                for fact in budget.matching_evidence(
                    memory,
                    target.temporal_scope,
                    allowed_hypotheses,
                    key,
                )? {
                    let mut fact_conditions = ConditionStatus::True;
                    for condition_id in &fact.proposition.condition_refs {
                        let evaluated = evaluate_condition(
                            memory,
                            condition_id,
                            target,
                            allowed_hypotheses,
                            budget,
                            visiting,
                        )?;
                        fact_conditions = combine_and(
                            fact_conditions,
                            evaluated.status.unwrap_or(ConditionStatus::Unknown),
                        );
                        result
                            .definition_record_ids
                            .extend(evaluated.definition_record_ids);
                        result.evidence_fact_ids.extend(evaluated.evidence_fact_ids);
                        result.dependencies.extend(evaluated.dependencies);
                        result.reason = result.reason.or(evaluated.reason);
                    }
                    if fact_conditions != ConditionStatus::True {
                        uncertain_evidence = true;
                        continue;
                    }
                    budget.trace_admitted_fact(fact);
                    result.evidence_fact_ids.insert(fact.id.clone());
                    result.dependencies.insert(fact.id.clone());
                    result
                        .dependencies
                        .extend(fact.dependencies.iter().cloned());
                    if fact.proposition.negative {
                        false_support = true;
                    } else {
                        true_support = true;
                    }
                }
                let status = support_status(true_support, false_support);
                Ok(ConditionEvaluationState {
                    status: Some(
                        if status == ConditionStatus::Unknown && uncertain_evidence {
                            ConditionStatus::Unknown
                        } else {
                            status
                        },
                    ),
                    reason: if status == ConditionStatus::Unknown {
                        Some(if uncertain_evidence {
                            "CONDITION_EVIDENCE_CONDITION_UNRESOLVED".into()
                        } else {
                            "CONDITION_EVIDENCE_NOT_FOUND".into()
                        })
                    } else {
                        None
                    },
                    ..result
                })
            }
            ConditionExpression::ConditionRef { condition_id } => evaluate_condition(
                memory,
                condition_id,
                target,
                allowed_hypotheses,
                budget,
                visiting,
            ),
            ConditionExpression::And { operands } | ConditionExpression::Or { operands } => {
                let is_and = matches!(expression, ConditionExpression::And { .. });
                let mut result = ConditionEvaluationState {
                    status: Some(if is_and {
                        ConditionStatus::True
                    } else {
                        ConditionStatus::False
                    }),
                    ..ConditionEvaluationState::default()
                };
                for operand in operands {
                    let evaluated = evaluate_condition_expression(
                        memory,
                        operand,
                        definition,
                        target,
                        allowed_hypotheses,
                        budget,
                        visiting,
                    )?;
                    let current = result.status.unwrap_or(ConditionStatus::Unknown);
                    let next = evaluated.status.unwrap_or(ConditionStatus::Unknown);
                    result.status = Some(if is_and {
                        combine_and(current, next)
                    } else {
                        combine_or(current, next)
                    });
                    result
                        .definition_record_ids
                        .extend(evaluated.definition_record_ids);
                    result.evidence_fact_ids.extend(evaluated.evidence_fact_ids);
                    result.dependencies.extend(evaluated.dependencies);
                    result.reason = result.reason.or(evaluated.reason);
                }
                Ok(result)
            }
            ConditionExpression::Not { operand } => {
                let mut result = evaluate_condition_expression(
                    memory,
                    operand,
                    definition,
                    target,
                    allowed_hypotheses,
                    budget,
                    visiting,
                )?;
                result.status = result.status.map(negate_status);
                Ok(result)
            }
        },
    )
}

fn evaluate_condition<'memory>(
    memory: &'memory MemoryState,
    condition_id: &str,
    target: ConditionTarget<'memory>,
    allowed_hypotheses: Option<&BTreeSet<String>>,
    budget: &mut ConditionEvaluationBudget<'_, 'memory>,
    visiting: &mut BTreeSet<String>,
) -> Result<ConditionEvaluationState> {
    charge_step(budget.steps, budget.max_steps)?;
    budget.trace_evaluation(
        || {
            Some(ConditionTraceTarget::Condition {
                condition_id: condition_id.to_owned(),
            })
        },
        |budget| {
            if !visiting.insert(condition_id.to_owned()) {
                return Ok(ConditionEvaluationState {
                    status: Some(ConditionStatus::Unknown),
                    reason: Some("CONDITION_DEPENDENCY_CYCLE".into()),
                    ..ConditionEvaluationState::default()
                });
            }
            let matches = memory
                .conditions
                .iter()
                .filter(|record| {
                    record.valid
                        && record.semantics_version == CONDITION_SEMANTICS_VERSION
                        && record.definition.condition_id == condition_id
                })
                .collect::<Vec<_>>();
            let Some(record) = matches.first().copied() else {
                visiting.remove(condition_id);
                return Ok(ConditionEvaluationState {
                    status: Some(ConditionStatus::Unknown),
                    reason: Some("CONDITION_DEFINITION_MISSING".into()),
                    ..ConditionEvaluationState::default()
                });
            };
            if matches.len() != 1 {
                visiting.remove(condition_id);
                return Ok(ConditionEvaluationState {
                    status: Some(ConditionStatus::Conflict),
                    definition_record_ids: matches.iter().map(|record| record.id.clone()).collect(),
                    dependencies: matches.iter().map(|record| record.id.clone()).collect(),
                    reason: Some("CONDITION_DEFINITION_CONFLICT".into()),
                    ..ConditionEvaluationState::default()
                });
            }
            if record
                .dependencies
                .iter()
                .any(|id| !active_memory_dependency(memory, id))
            {
                visiting.remove(condition_id);
                return Ok(ConditionEvaluationState {
                    status: Some(ConditionStatus::Unknown),
                    definition_record_ids: BTreeSet::from([record.id.clone()]),
                    dependencies: BTreeSet::from([record.id.clone()]),
                    reason: Some("CONDITION_DEPENDENCY_INACTIVE".into()),
                    ..ConditionEvaluationState::default()
                });
            }
            if record.definition.scope != target.scope
                || (record.definition.subject != "$subject"
                    && record.definition.subject != target.subject)
            {
                visiting.remove(condition_id);
                return Ok(ConditionEvaluationState {
                    status: Some(ConditionStatus::False),
                    definition_record_ids: BTreeSet::from([record.id.clone()]),
                    dependencies: BTreeSet::from([record.id.clone()]),
                    reason: Some("CONDITION_APPLICABILITY_MISMATCH".into()),
                    ..ConditionEvaluationState::default()
                });
            }
            if (record.definition.valid_from.is_some() || record.definition.valid_until.is_some())
                && target
                    .temporal_scope
                    .is_none_or(|time| !valid_utc_instant(time))
            {
                visiting.remove(condition_id);
                return Ok(ConditionEvaluationState {
                    status: Some(ConditionStatus::Unknown),
                    definition_record_ids: BTreeSet::from([record.id.clone()]),
                    dependencies: BTreeSet::from([record.id.clone()]),
                    reason: Some("CONDITION_EVALUATION_TIME_UNKNOWN".into()),
                    ..ConditionEvaluationState::default()
                });
            }
            if target.temporal_scope.is_some_and(|time| {
                record
                    .definition
                    .valid_from
                    .as_deref()
                    .is_some_and(|start| time < start)
                    || record
                        .definition
                        .valid_until
                        .as_deref()
                        .is_some_and(|end| time >= end)
            }) {
                visiting.remove(condition_id);
                return Ok(ConditionEvaluationState {
                    status: Some(ConditionStatus::False),
                    definition_record_ids: BTreeSet::from([record.id.clone()]),
                    dependencies: BTreeSet::from([record.id.clone()]),
                    reason: Some("CONDITION_OUTSIDE_VALID_PERIOD".into()),
                    ..ConditionEvaluationState::default()
                });
            }
            let mut result = evaluate_condition_expression(
                memory,
                &record.definition.expression,
                &record.definition,
                target,
                allowed_hypotheses,
                budget,
                visiting,
            )?;
            visiting.remove(condition_id);
            result.definition_record_ids.insert(record.id.clone());
            result.dependencies.insert(record.id.clone());
            result
                .dependencies
                .extend(record.dependencies.iter().cloned());
            Ok(result)
        },
    )
}

fn evaluate_condition_refs<'a, 'memory>(
    memory: &'memory MemoryState,
    target: ConditionTarget<'memory>,
    condition_ids: impl ExactSizeIterator<Item = &'a str>,
    allowed_hypotheses: Option<&BTreeSet<String>>,
    budget: &mut ConditionEvaluationBudget<'_, 'memory>,
) -> Result<(
    Option<ConditionStatus>,
    Vec<ConditionEvaluation>,
    BTreeSet<String>,
)> {
    if condition_ids.len() == 0 {
        return Ok((None, Vec::new(), BTreeSet::new()));
    }
    let mut overall = ConditionStatus::True;
    let mut evaluations = Vec::with_capacity(condition_ids.len());
    let mut dependencies = BTreeSet::new();
    for condition_id in condition_ids {
        let state = evaluate_condition(
            memory,
            condition_id,
            target,
            allowed_hypotheses,
            budget,
            &mut BTreeSet::new(),
        )?;
        let status = state.status.unwrap_or(ConditionStatus::Unknown);
        overall = combine_and(overall, status);
        let definition_record_ids = state.definition_record_ids.into_iter().collect::<Vec<_>>();
        let evidence_fact_ids = state.evidence_fact_ids.into_iter().collect::<Vec<_>>();
        dependencies.extend(state.dependencies.iter().cloned());
        evaluations.push(ConditionEvaluation {
            condition_id: condition_id.to_owned(),
            status,
            definition_record_ids,
            evidence_fact_ids,
            dependencies: state.dependencies.into_iter().collect(),
            reason: state.reason,
        });
    }
    Ok((Some(overall), evaluations, dependencies))
}

pub(crate) fn is_ground(proposition: &Proposition) -> bool {
    variables(proposition).is_empty()
}
fn next_memory_revision(revision: u64) -> Result<u64> {
    revision
        .checked_add(1)
        .ok_or_else(|| failure("MEMORY_REVISION_LIMIT", "memory revision overflow"))
}
fn bump(memory: &mut MemoryState) -> Result<()> {
    memory.revision = next_memory_revision(memory.revision)?;
    Ok(())
}

/// Add a scenario-only assumption to a cloned memory projection. The caller
/// must never persist this projection; the stable ID binds the actor/case,
/// source revision, ordinal and proposition.
pub(crate) fn add_hypothesis(
    memory: &mut MemoryState,
    proposition: Proposition,
    scenario_identity: &str,
    ordinal: usize,
) -> Result<Fact> {
    validate(&proposition)?;
    if !is_ground(&proposition) {
        return Err(failure(
            "UNBOUND_VARIABLE",
            "a counterfactual assumption requires bound subject and object",
        ));
    }
    let id = format!(
        "hypothesis:{}",
        identity(&(
            "counterfactual-assumption-v1",
            scenario_identity,
            memory.revision,
            ordinal,
            &proposition,
        ))?
    );
    let source_input = format!("counterfactual:{scenario_identity}");
    if let Some(old) = memory.facts.iter().find(|fact| fact.id == id) {
        return Ok(old.clone());
    }
    let fact = Fact {
        id,
        proposition,
        guarantee: Guarantee::Hypothesis,
        source_input,
        source_evidence: None,
        dependencies: Vec::new(),
        valid: true,
    };
    memory.facts.push(fact.clone());
    bump(memory)?;
    Ok(fact)
}

pub fn assert_fact(
    memory: &mut MemoryState,
    proposition: Proposition,
    source_input: &str,
) -> Result<Fact> {
    assert_fact_with_source_evidence(memory, proposition, source_input, None)
}

pub(crate) fn user_premise_id(proposition: &Proposition, source_input: &str) -> Result<String> {
    Ok(format!(
        "fact:{}",
        identity(&("user-premise-v1", proposition, source_input))?
    ))
}

pub(crate) fn assert_fact_with_source_evidence(
    memory: &mut MemoryState,
    proposition: Proposition,
    source_input: &str,
    source_evidence: Option<SealedSourceEvidence>,
) -> Result<Fact> {
    validate(&proposition)?;
    if !variables(&proposition).is_empty() {
        return Err(failure(
            "UNBOUND_VARIABLE",
            "a fact requires bound subject and object",
        ));
    }
    if source_input.is_empty() {
        return Err(failure(
            "MISSING_PROVENANCE",
            "a premise needs its original input identity",
        ));
    }
    if let Some(evidence) = &source_evidence {
        let valid_digest =
            |value: &str| value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit());
        let one_assertion_family = evidence
            .source_roles
            .keys()
            .all(|role| role.starts_with("/Assert/proposition/"))
            || evidence
                .source_roles
                .keys()
                .all(|role| role.starts_with("/AssertPreviousRelation/proposition/"));
        let one_correction_family = evidence
            .source_roles
            .keys()
            .all(|role| role.starts_with("/CorrectPremise/"))
            || evidence
                .source_roles
                .keys()
                .all(|role| role.starts_with("/CorrectPreviousPremise/"));
        let spans_are_well_formed = (one_assertion_family || one_correction_family)
            && evidence.source_roles.iter().all(|(_, spans)| {
                !spans.is_empty()
                    && spans.iter().all(|span| {
                        span.start < span.end && span.end - span.start == span.text.len()
                    })
            });
        if evidence.evidence_version != "minidora.sealed-source-evidence.v1"
            || evidence.input_fingerprint != source_input
            || !valid_digest(&evidence.bundle_fingerprint)
            || !valid_digest(&evidence.raw_sha256)
            || !valid_digest(&evidence.semantic_signature)
            || evidence.parser_version.is_empty()
            || evidence.semantic_signature.is_empty()
            || evidence.source_roles.is_empty()
            || !spans_are_well_formed
        {
            return Err(failure(
                "INVALID_SOURCE_EVIDENCE",
                "premise source evidence is not bound to one valid sealed assertion input",
            ));
        }
    }
    let id = user_premise_id(&proposition, source_input)?;
    if let Some(old) = memory.facts.iter().find(|fact| fact.id == id) {
        if source_evidence
            .as_ref()
            .is_some_and(|evidence| old.source_evidence.as_ref() != Some(evidence))
        {
            return Err(failure(
                "SOURCE_EVIDENCE_CONFLICT",
                "the same direct premise identity has different sealed source evidence",
            ));
        }
        return Ok(old.clone());
    }
    let fact = Fact {
        id,
        proposition,
        guarantee: Guarantee::UserPremise,
        source_input: source_input.into(),
        source_evidence,
        dependencies: Vec::new(),
        valid: true,
    };
    memory.facts.push(fact.clone());
    bump(memory)?;
    Ok(fact)
}

pub fn add_rule(
    memory: &mut MemoryState,
    premise: Proposition,
    conclusion: Proposition,
    source_input: &str,
) -> Result<RelationRule> {
    add_rule_with_premises(memory, vec![premise], conclusion, source_input)
}

pub(crate) fn declared_rule_id(
    premises: &[Proposition],
    conclusion: &Proposition,
    source_input: &str,
) -> Result<String> {
    let digest = match premises {
        [premise] => identity(&("declared-rule-v1", premise, conclusion, source_input))?,
        [_, _] => identity(&(
            "declared-conjunctive-rule-v1",
            premises,
            conclusion,
            source_input,
        ))?,
        _ => {
            return Err(failure(
                "UNSUPPORTED_RULE_PREMISE_COUNT",
                "a relation rule requires one or two explicit premises",
            ));
        }
    };
    Ok(format!("rule:{digest}"))
}

pub fn add_rule_with_premises(
    memory: &mut MemoryState,
    premises: Vec<Proposition>,
    conclusion: Proposition,
    source_input: &str,
) -> Result<RelationRule> {
    if !(1..=2).contains(&premises.len()) {
        return Err(failure(
            "UNSUPPORTED_RULE_PREMISE_COUNT",
            "a relation rule requires one or two explicit premises",
        ));
    }
    for premise in &premises {
        validate(premise)?;
    }
    validate(&conclusion)?;
    let premise = &premises[0];
    let additional_premises = premises[1..].to_vec();
    if premises
        .iter()
        .any(|antecedent| antecedent.scope != premise.scope)
        || premise.scope != conclusion.scope
    {
        return Err(failure(
            "SCOPE_MISMATCH",
            "a rule may not silently change scope",
        ));
    }
    if premises
        .iter()
        .any(|antecedent| antecedent.temporal_scope != premise.temporal_scope)
        || premise.temporal_scope != conclusion.temporal_scope
    {
        return Err(failure(
            "TIME_SCOPE_MISMATCH",
            "a rule may not silently change temporal scope",
        ));
    }
    if premises
        .iter()
        .any(|antecedent| antecedent.condition_refs != premise.condition_refs)
        || premise.condition_refs != conclusion.condition_refs
    {
        return Err(failure(
            "CONDITION_SCOPE_MISMATCH",
            "a rule may not silently change applicability-condition references",
        ));
    }
    if premises
        .iter()
        .enumerate()
        .any(|(index, antecedent)| premises[index + 1..].contains(antecedent))
    {
        return Err(failure(
            "DUPLICATE_RULE_PREMISE",
            "a conjunctive rule may not repeat the same premise",
        ));
    }
    if additional_premises.len() == 1
        && variables(premise).is_disjoint(&variables(&additional_premises[0]))
    {
        return Err(failure(
            "UNBOUND_VARIABLE",
            "two-premise relation rules require a shared variable to join their evidence",
        ));
    }
    let bound_variables: BTreeSet<_> = premises.iter().flat_map(variables).collect();
    if !variables(&conclusion).is_subset(&bound_variables) {
        return Err(failure(
            "UNBOUND_VARIABLE",
            "conclusion contains a variable not bound by its premises",
        ));
    }
    if additional_premises.is_empty() && premise == &conclusion {
        return Err(failure("CYCLIC_RULE", "a self-rule adds no consequence"));
    }
    if source_input.is_empty() {
        return Err(failure(
            "MISSING_PROVENANCE",
            "a rule needs original input provenance",
        ));
    }
    let id = declared_rule_id(&premises, &conclusion, source_input)?;
    if let Some(old) = memory.rules.iter().find(|rule| rule.id == id) {
        return Ok(old.clone());
    }
    let rule = RelationRule {
        id,
        premise: premise.clone(),
        additional_premises,
        conclusion,
        source_input: source_input.into(),
        dependencies: Vec::new(),
        valid: true,
    };
    memory.rules.push(rule.clone());
    // Learn a proof-preserving shortcut from each newly declared rule and each
    // existing valid *root* rule. This does not invent a rule from examples.
    let roots: Vec<_> = memory
        .rules
        .iter()
        .filter(|candidate| candidate.valid && candidate.dependencies.is_empty())
        .cloned()
        .collect();
    let mut additions = BTreeMap::new();
    for other in roots {
        if other.id == rule.id {
            continue;
        }
        for (left, right) in [(&other, &rule), (&rule, &other)] {
            for composed in compose(left, right)? {
                if !memory.rules.iter().any(|old| {
                    old.premise == composed.premise
                        && old.additional_premises == composed.additional_premises
                        && old.conclusion == composed.conclusion
                        && old.valid
                }) {
                    additions.insert(composed.id.clone(), composed);
                }
            }
        }
    }
    memory.rules.extend(additions.into_values());
    bump(memory)?;
    Ok(rule)
}

fn renamed(proposition: &Proposition, prefix: &str) -> Proposition {
    let term = |value: &str| {
        if variable(value) {
            format!("${prefix}_{}", &value[1..])
        } else {
            value.to_string()
        }
    };
    Proposition {
        subject: term(&proposition.subject),
        object: term(&proposition.object),
        ..proposition.clone()
    }
}
fn resolve(term: &str, bindings: &BTreeMap<String, String>) -> String {
    let mut value = term.to_string();
    let mut seen = BTreeSet::new();
    while let Some(next) = bindings.get(&value) {
        if !seen.insert(value.clone()) {
            break;
        }
        value = next.clone();
    }
    value
}
fn unify_term(left: &str, right: &str, bindings: &mut BTreeMap<String, String>) -> bool {
    let left = resolve(left, bindings);
    let right = resolve(right, bindings);
    if left == right {
        true
    } else if variable(&left) {
        bindings.insert(left, right);
        true
    } else if variable(&right) {
        bindings.insert(right, left);
        true
    } else {
        false
    }
}
fn same_shape(left: &Proposition, right: &Proposition) -> bool {
    left.predicate == right.predicate
        && left.negative == right.negative
        && left.scope == right.scope
        && left.temporal_scope == right.temporal_scope
        && left.condition_refs == right.condition_refs
}
fn substitute(proposition: &Proposition, bindings: &BTreeMap<String, String>) -> Proposition {
    Proposition {
        subject: resolve(&proposition.subject, bindings),
        object: resolve(&proposition.object, bindings),
        ..proposition.clone()
    }
}
fn canonical_rule_variables(premises: &mut [Proposition], conclusion: &mut Proposition) {
    let mut names = BTreeMap::new();
    let terms = premises
        .iter_mut()
        .flat_map(|premise| [&mut premise.subject, &mut premise.object])
        .chain([&mut conclusion.subject, &mut conclusion.object]);
    for term in terms {
        if variable(term) {
            let next = format!("$v{}", names.len());
            *term = names.entry(term.clone()).or_insert(next).clone();
        }
    }
}

fn compose(left: &RelationRule, right: &RelationRule) -> Result<Vec<RelationRule>> {
    // Declared parent rules have one or two premises. Never recursively
    // flatten a previously composed shortcut into another shortcut.
    if left.additional_premises.len() > 1 || right.additional_premises.len() > 1 {
        return Ok(Vec::new());
    }
    let left_conclusion = renamed(&left.conclusion, "left");
    let right_conclusion = renamed(&right.conclusion, "right");
    let left_premises: Vec<_> = std::iter::once(&left.premise)
        .chain(left.additional_premises.iter())
        .map(|premise| renamed(premise, "left"))
        .collect();
    let right_premises: Vec<_> = std::iter::once(&right.premise)
        .chain(right.additional_premises.iter())
        .map(|premise| renamed(premise, "right"))
        .collect();
    let dependencies = vec![left.id.clone(), right.id.clone()];
    let mut composed_rules = BTreeMap::new();
    for (matched_index, right_premise) in right_premises.iter().enumerate() {
        if !same_shape(&left_conclusion, right_premise) {
            continue;
        }
        let mut bindings = BTreeMap::new();
        if !unify_term(
            &left_conclusion.subject,
            &right_premise.subject,
            &mut bindings,
        ) || !unify_term(
            &left_conclusion.object,
            &right_premise.object,
            &mut bindings,
        ) {
            continue;
        }
        let mut premises: Vec<_> = left_premises
            .iter()
            .map(|premise| substitute(premise, &bindings))
            .collect();
        premises.extend(
            right_premises
                .iter()
                .enumerate()
                .filter(|(index, _)| *index != matched_index)
                .map(|(_, premise)| substitute(premise, &bindings)),
        );
        if !(1..=3).contains(&premises.len())
            || premises
                .iter()
                .enumerate()
                .any(|(index, premise)| premises[index + 1..].contains(premise))
        {
            continue;
        }
        let mut conclusion = substitute(&right_conclusion, &bindings);
        canonical_rule_variables(&mut premises, &mut conclusion);
        let bound_variables: BTreeSet<_> = premises.iter().flat_map(variables).collect();
        if (premises.len() == 1 && premises[0] == conclusion)
            || !variables(&conclusion).is_subset(&bound_variables)
            || (premises.len() == 2
                && variables(&premises[0]).is_disjoint(&variables(&premises[1])))
        {
            continue;
        }
        let id = if premises.len() == 1 {
            format!(
                "rule:{}",
                identity(&(
                    "proof-composition-v1",
                    &premises[0],
                    &conclusion,
                    &dependencies
                ))?
            )
        } else if premises.len() == 2 {
            format!(
                "rule:{}",
                identity(&(
                    "proof-composition-conjunctive-v1",
                    &premises,
                    &conclusion,
                    &dependencies
                ))?
            )
        } else {
            format!(
                "rule:{}",
                identity(&(
                    "proof-composition-ternary-v1",
                    &premises,
                    &conclusion,
                    &dependencies
                ))?
            )
        };
        composed_rules.insert(
            id.clone(),
            RelationRule {
                id,
                premise: premises[0].clone(),
                additional_premises: premises[1..].to_vec(),
                conclusion,
                source_input: "derived:proof-composition-v1".into(),
                dependencies: dependencies.clone(),
                valid: true,
            },
        );
    }
    Ok(composed_rules.into_values().collect())
}

pub(crate) fn verified_composed_rule(
    rule: &RelationRule,
    left: &RelationRule,
    right: &RelationRule,
) -> Result<bool> {
    if left.id == right.id
        || [left, right].into_iter().any(|parent| {
            !parent.dependencies.is_empty()
                || parent.source_input.is_empty()
                || parent.source_input.starts_with("derived:")
        })
    {
        return Ok(false);
    }
    Ok(compose(left, right)?.into_iter().any(|mut candidate| {
        // The immutable proof can be inspected after a withdrawal.
        candidate.valid = rule.valid;
        candidate == *rule
    }))
}

fn permitted_rule<'a>(
    rule: &'a RelationRule,
    rules: &BTreeMap<&str, &'a RelationRule>,
    seen: &mut BTreeSet<String>,
) -> bool {
    // The guard evaluation belongs to the requested subject. It cannot
    // discharge a premise or a shortcut parent with a different subject.
    let guard_keeps_subject = rule.conclusion.condition_refs.is_empty()
        || std::iter::once(&rule.premise)
            .chain(&rule.additional_premises)
            .all(|premise| premise.subject == rule.conclusion.subject);
    if !rule.valid || !guard_keeps_subject || !seen.insert(rule.id.clone()) {
        return false;
    }
    let valid = rule.dependencies.iter().all(|id| {
        rules
            .get(id.as_str())
            .is_some_and(|parent| permitted_rule(parent, rules, seen))
    });
    seen.remove(&rule.id);
    valid
}
fn matches(pattern: &Proposition, actual: &Proposition) -> Option<BTreeMap<String, String>> {
    matches_with_bindings(pattern, actual, &BTreeMap::new())
}
fn matches_with_bindings(
    pattern: &Proposition,
    actual: &Proposition,
    existing: &BTreeMap<String, String>,
) -> Option<BTreeMap<String, String>> {
    if !same_shape(pattern, actual) {
        return None;
    }
    let mut bindings = existing.clone();
    if !unify_term(&pattern.subject, &actual.subject, &mut bindings)
        || !unify_term(&pattern.object, &actual.object, &mut bindings)
    {
        return None;
    }
    Some(bindings)
}
pub(crate) fn opposition(proposition: &Proposition) -> Proposition {
    Proposition {
        negative: !proposition.negative,
        ..proposition.clone()
    }
}

pub fn query(
    memory: &MemoryState,
    proposition: &Proposition,
    max_steps: u32,
) -> Result<QueryOutcome> {
    query_internal(memory, proposition, max_steps, None, None)
}

/// Form and consume the exact condition-evidence selection in the same
/// projection. All projected Facts remain available to the relation proof.
pub(crate) fn query_active_relation(
    index: &MemoryIndexSnapshot<'_, '_>,
    proposition: &Proposition,
    max_actions: u32,
) -> Result<(MemoryState, QueryOutcome)> {
    let mut evidence_indices = Vec::new();
    let collect_evidence =
        (!proposition.condition_refs.is_empty()).then_some(&mut evidence_indices);
    let (memory, projection_actions) =
        project_relation(index, proposition, max_actions, collect_evidence)?;
    let query_budget = max_actions.checked_sub(projection_actions).ok_or_else(|| {
        failure(
            "RELATION_BUDGET",
            "active memory projection exhausted the relation action budget",
        )
    })?;
    let mut answer = query_internal(
        &memory,
        proposition,
        query_budget,
        None,
        Some(&evidence_indices),
    )?;
    answer.steps = projection_actions
        .checked_add(answer.steps)
        .ok_or_else(|| failure("RELATION_BUDGET", "relation action count overflowed"))?;
    Ok((memory, answer))
}

/// Evaluate an explicit condition against the current indexed evidence.
pub(crate) fn evaluate_active_condition(
    index: &MemoryIndexSnapshot<'_, '_>,
    condition_id: &str,
    subject: &str,
    scope: &str,
    temporal_scope: Option<&str>,
    max_actions: u32,
    detailed: bool,
) -> Result<(ConditionEvaluation, Vec<ConditionTraceStep>)> {
    validate_condition_id(condition_id)?;
    let invalid_term = |value: &str| {
        value.trim().is_empty()
            || value.len() > 1024
            || value.chars().any(char::is_control)
            || variable(value)
    };
    if invalid_term(subject) || invalid_term(scope) || temporal_scope.is_some_and(invalid_term) {
        return Err(failure(
            "INVALID_CONDITION_TARGET",
            "condition evaluation requires bounded, exact subject, scope and optional time",
        ));
    }
    let target = ConditionTarget {
        subject,
        scope,
        temporal_scope,
    };
    let mut evidence_indices = Vec::new();
    let (memory, mut actions) = project_memory(
        index,
        None,
        target,
        std::iter::once(condition_id),
        max_actions,
        Some(&mut evidence_indices),
    )?;
    let mut trace = Vec::new();
    let (_, mut evaluations, _) = evaluate_condition_refs(
        &memory,
        target,
        std::iter::once(condition_id),
        None,
        &mut ConditionEvaluationBudget {
            max_steps: max_actions,
            steps: &mut actions,
            projected_evidence: Some(&evidence_indices),
            evidence: None,
            trace: detailed.then_some(ConditionTrace {
                steps: &mut trace,
                parent: None,
            }),
        },
    )?;
    let evaluation = evaluations.pop().ok_or_else(|| {
        failure(
            "CONDITION_EVALUATION_UNAVAILABLE",
            "the explicit condition did not produce an evaluation",
        )
    })?;
    Ok((evaluation, trace))
}

/// Build a bounded, rebuildable current-memory projection for a typed Ask.
/// The index only supplies stored records; Core remains the sole caller that
/// evaluates conditions and adopts the resulting proof.
pub(crate) fn active_relation_projection(
    index: &MemoryIndexSnapshot<'_, '_>,
    proposition: &Proposition,
    max_actions: u32,
) -> Result<(MemoryState, u32)> {
    project_relation(index, proposition, max_actions, None)
}

fn project_relation(
    index: &MemoryIndexSnapshot<'_, '_>,
    proposition: &Proposition,
    max_actions: u32,
    evidence_indices: Option<&mut Vec<usize>>,
) -> Result<(MemoryState, u32)> {
    validate(proposition)?;
    project_memory(
        index,
        Some(proposition),
        proposition.into(),
        proposition.condition_refs.iter().map(String::as_str),
        max_actions,
        evidence_indices,
    )
}

fn project_memory<'a>(
    index: &MemoryIndexSnapshot<'_, '_>,
    proposition: Option<&Proposition>,
    target: ConditionTarget<'_>,
    condition_ids: impl IntoIterator<Item = &'a str>,
    max_actions: u32,
    mut evidence_indices: Option<&mut Vec<usize>>,
) -> Result<(MemoryState, u32)> {
    if max_actions == 0 {
        return Err(failure(
            "RELATION_BUDGET",
            "active relation projection budget must be positive",
        ));
    }

    fn charge(actions: &mut u32, max_actions: u32) -> Result<()> {
        *actions = actions.checked_add(1).ok_or_else(|| {
            failure(
                "RELATION_BUDGET",
                "active memory projection action overflow",
            )
        })?;
        if *actions > max_actions {
            return Err(failure(
                "RELATION_BUDGET",
                "goal-relevant memory projection exceeds the sealed action budget",
            ));
        }
        Ok(())
    }

    fn expression_parts(
        expression: &ConditionExpression,
        references: &mut BTreeSet<String>,
        fact_atoms: &mut BTreeSet<(String, String)>,
    ) {
        match expression {
            ConditionExpression::Fact { predicate, object } => {
                fact_atoms.insert((predicate.clone(), object.clone()));
            }
            ConditionExpression::ConditionRef { condition_id } => {
                references.insert(condition_id.clone());
            }
            ConditionExpression::And { operands } | ConditionExpression::Or { operands } => {
                for operand in operands {
                    expression_parts(operand, references, fact_atoms);
                }
            }
            ConditionExpression::Not { operand } => {
                expression_parts(operand, references, fact_atoms);
            }
        }
    }

    let mut actions = 0;
    let mut records = BTreeMap::<String, IndexedMemoryRecord>::new();
    let mut relation_shapes = VecDeque::from_iter(proposition.cloned());
    let mut seen_relation_shapes = BTreeSet::new();
    let mut condition_ids = VecDeque::from_iter(condition_ids.into_iter().map(str::to_owned));
    let mut seen_condition_ids = BTreeSet::new();
    let mut record_ids = VecDeque::new();
    let mut requested_record_ids = BTreeSet::new();
    let mut evidence_queries = BTreeSet::new();
    let mut condition_evidence_ids = BTreeSet::new();

    macro_rules! include_record {
        ($record:expr) => {{
            let record = $record;
            let id = record.value.id().to_owned();
            if !records.contains_key(&id) {
                charge(&mut actions, max_actions)?;
                match &record.value {
                    IndexedMemoryValue::Fact(fact) => {
                        condition_ids.extend(fact.proposition.condition_refs.iter().cloned());
                        record_ids.extend(fact.dependencies.iter().cloned());
                    }
                    IndexedMemoryValue::Rule(rule) => {
                        condition_ids.extend(rule.conclusion.condition_refs.iter().cloned());
                        for premise in
                            std::iter::once(&rule.premise).chain(rule.additional_premises.iter())
                        {
                            relation_shapes.push_back(premise.clone());
                            condition_ids.extend(premise.condition_refs.iter().cloned());
                        }
                        record_ids.extend(rule.dependencies.iter().cloned());
                    }
                    IndexedMemoryValue::Condition(condition) => {
                        record_ids.extend(condition.dependencies.iter().cloned());
                        condition_ids.extend(condition.definition.dependencies.iter().cloned());
                    }
                }
                records.insert(id, record);
            }
        }};
    }

    while !relation_shapes.is_empty() || !condition_ids.is_empty() || !record_ids.is_empty() {
        while let Some(shape) = relation_shapes.pop_front() {
            if !seen_relation_shapes.insert(shape.clone()) {
                continue;
            }
            charge(&mut actions, max_actions)?;
            let remaining = usize::try_from(max_actions - actions).unwrap_or(usize::MAX);
            for record in index.relation_candidates(&shape, remaining)? {
                include_record!(record);
            }
        }

        while let Some(condition_id) = condition_ids.pop_front() {
            if !seen_condition_ids.insert(condition_id.clone()) {
                continue;
            }
            charge(&mut actions, max_actions)?;
            let remaining = usize::try_from(max_actions - actions).unwrap_or(usize::MAX);
            let definitions = index.condition_definitions(&condition_id, remaining)?;
            for record in definitions {
                let definition = match &record.value {
                    IndexedMemoryValue::Condition(condition) => condition.definition.clone(),
                    _ => {
                        return Err(failure(
                            "MEMORY_INDEX_CORRUPT",
                            "condition lookup returned a non-condition record",
                        ));
                    }
                };
                let mut referenced_conditions = BTreeSet::new();
                let mut fact_atoms = BTreeSet::new();
                expression_parts(
                    &definition.expression,
                    &mut referenced_conditions,
                    &mut fact_atoms,
                );
                condition_ids.extend(referenced_conditions);
                include_record!(record);

                let subject = if definition.subject == "$subject" {
                    target.subject
                } else {
                    definition.subject.as_str()
                };
                for (predicate, object) in fact_atoms {
                    let evidence_key = (
                        subject.to_owned(),
                        predicate.clone(),
                        object.clone(),
                        definition.scope.clone(),
                        target.temporal_scope.map(str::to_owned),
                    );
                    if !evidence_queries.insert(evidence_key) {
                        continue;
                    }
                    charge(&mut actions, max_actions)?;
                    let remaining = usize::try_from(max_actions - actions).unwrap_or(usize::MAX);
                    let evidence = index.condition_evidence_facts(
                        subject,
                        &predicate,
                        &object,
                        &definition.scope,
                        target.temporal_scope,
                        remaining,
                    )?;
                    for fact in evidence {
                        if evidence_indices.is_some() {
                            // Record the evidence role even when another
                            // lookup already loaded this same Fact.
                            condition_evidence_ids.insert(fact.value.id().to_owned());
                        }
                        include_record!(fact);
                    }
                }
            }
        }

        while let Some(record_id) = record_ids.pop_front() {
            if records.contains_key(&record_id) || !requested_record_ids.insert(record_id.clone()) {
                continue;
            }
            charge(&mut actions, max_actions)?;
            if let Some(record) = index.record_by_id(&record_id)? {
                include_record!(record);
            }
        }
    }

    let mut memory = MemoryState {
        revision: index.revision(),
        ..MemoryState::default()
    };
    let mut facts = Vec::new();
    let mut rules = Vec::new();
    let mut conditions = Vec::new();
    for record in records.into_values() {
        match record.value {
            IndexedMemoryValue::Fact(fact) => facts.push((record.ordinal, fact)),
            IndexedMemoryValue::Rule(rule) => rules.push((record.ordinal, rule)),
            IndexedMemoryValue::Condition(condition) => {
                conditions.push((record.ordinal, condition));
            }
        }
    }
    facts.sort_by_key(|(ordinal, _)| *ordinal);
    rules.sort_by_key(|(ordinal, _)| *ordinal);
    conditions.sort_by_key(|(ordinal, _)| *ordinal);
    memory.facts = facts
        .into_iter()
        .enumerate()
        .map(|(position, (_, fact))| {
            if let Some(indices) = evidence_indices.as_deref_mut()
                && condition_evidence_ids.contains(&fact.id)
            {
                indices.push(position);
            }
            fact
        })
        .collect();
    if evidence_indices.is_some_and(|indices| indices.len() != condition_evidence_ids.len()) {
        return Err(failure(
            "MEMORY_INDEX_CORRUPT",
            "condition evidence does not match the projected Fact records",
        ));
    }
    memory.rules = rules.into_iter().map(|(_, record)| record).collect();
    memory.conditions = conditions.into_iter().map(|(_, record)| record).collect();
    Ok((memory, actions))
}

/// Build a bounded relation inventory for one subject from the revision-matched
/// Store index. Candidate discovery starts with that subject's active Facts and
/// applicable Rules, then reuses the ordinary proof projection and evaluator
/// for each supported relation shape.
pub(crate) fn active_relation_inventory_projection(
    index: &MemoryIndexSnapshot<'_, '_>,
    subject: &str,
    max_actions: u32,
) -> Result<(RelationInventory, u32)> {
    let (projection, actions) = active_relation_inventory(index, subject, max_actions)?;
    let (inventory, _) = projection.require_inventory()?;
    Ok((inventory, actions))
}

/// The same indexed inventory, with the exact work used even when no active
/// premises support the subject. An unavailable proof is still an error.
pub(crate) fn active_relation_inventory_with_cost(
    index: &MemoryIndexSnapshot<'_, '_>,
    subject: &str,
    max_actions: u32,
) -> Result<(Option<RelationInventory>, u32)> {
    let (projection, actions) = active_relation_inventory(index, subject, max_actions)?;
    let inventory = match projection {
        RelationInventoryProjection::Supported(inventory, _) => Some(inventory),
        RelationInventoryProjection::Missing(_) => None,
    };
    Ok((inventory, actions))
}

enum RelationInventoryProjection {
    Supported(RelationInventory, Vec<RelationRule>),
    Missing(&'static str),
}

impl RelationInventoryProjection {
    fn require_inventory(self) -> Result<(RelationInventory, Vec<RelationRule>)> {
        match self {
            Self::Supported(inventory, rules) => Ok((inventory, rules)),
            Self::Missing(reason) => Err(failure("MISSING_PREMISES", reason)),
        }
    }
}

fn active_relation_inventory(
    index: &MemoryIndexSnapshot<'_, '_>,
    subject: &str,
    max_actions: u32,
) -> Result<(RelationInventoryProjection, u32)> {
    if max_actions == 0 {
        return Err(failure(
            "RELATION_BUDGET",
            "relation-profile projection budget must be positive",
        ));
    }

    let mut actions = 1_u32;
    let remaining = max_actions - actions;
    let candidates = index
        .relation_profile_candidates(subject, usize::try_from(remaining).unwrap_or(usize::MAX))?;
    actions = actions
        .checked_add(u32::try_from(candidates.len()).map_err(|_| {
            failure(
                "RELATION_BUDGET",
                "relation-profile candidate count overflowed",
            )
        })?)
        .ok_or_else(|| {
            failure(
                "RELATION_BUDGET",
                "relation-profile candidate action count overflowed",
            )
        })?;

    let mut seeds = MemoryState {
        revision: index.revision(),
        ..MemoryState::default()
    };
    for candidate in candidates {
        match candidate.value {
            IndexedMemoryValue::Fact(fact) => seeds.facts.push(fact),
            IndexedMemoryValue::Rule(rule) => seeds.rules.push(rule),
            IndexedMemoryValue::Condition(_) => {
                return Err(failure(
                    "MEMORY_INDEX_CORRUPT",
                    "relation-profile candidate lookup returned a condition record",
                ));
            }
        }
    }

    active_relation_inventory_from_seeds(index, subject, max_actions, actions, seeds)
}

/// Build the comparison inventory from active relation records in the exact
/// requested scope. Rules with any premise outside that scope are discarded,
/// matching the snapshot comparison projection.
pub(crate) fn active_scoped_relation_inventory_projection(
    index: &MemoryIndexSnapshot<'_, '_>,
    subject: &str,
    scope: &str,
    max_actions: u32,
) -> Result<(RelationInventory, u32, Vec<RelationRule>)> {
    if max_actions == 0 {
        return Err(failure(
            "RELATION_BUDGET",
            "relation-comparison projection budget must be positive",
        ));
    }

    let mut actions = 1_u32;
    let remaining = max_actions - actions;
    let candidates =
        index.relation_scope_candidates(scope, usize::try_from(remaining).unwrap_or(usize::MAX))?;
    actions = actions
        .checked_add(u32::try_from(candidates.len()).map_err(|_| {
            failure(
                "RELATION_BUDGET",
                "relation-comparison candidate count overflowed",
            )
        })?)
        .ok_or_else(|| {
            failure(
                "RELATION_BUDGET",
                "relation-comparison candidate action count overflowed",
            )
        })?;

    let mut facts = Vec::new();
    let mut rules = Vec::new();
    for candidate in candidates {
        match candidate.value {
            IndexedMemoryValue::Fact(fact) if fact.proposition.scope == scope => {
                facts.push((candidate.ordinal, fact));
            }
            IndexedMemoryValue::Fact(_) => {}
            IndexedMemoryValue::Rule(rule)
                if rule.conclusion.scope == scope
                    && rule.premise.scope == scope
                    && rule
                        .additional_premises
                        .iter()
                        .all(|premise| premise.scope == scope) =>
            {
                rules.push((candidate.ordinal, rule));
            }
            IndexedMemoryValue::Rule(_) => {}
            IndexedMemoryValue::Condition(_) => {
                return Err(failure(
                    "MEMORY_INDEX_CORRUPT",
                    "relation-scope candidate lookup returned a condition record",
                ));
            }
        }
    }
    facts.sort_by_key(|(ordinal, _)| *ordinal);
    rules.sort_by_key(|(ordinal, _)| *ordinal);
    let seeds = MemoryState {
        revision: index.revision(),
        facts: facts.into_iter().map(|(_, fact)| fact).collect(),
        rules: rules.into_iter().map(|(_, rule)| rule).collect(),
        ..MemoryState::default()
    };

    let (projection, actions) =
        active_relation_inventory_from_seeds(index, subject, max_actions, actions, seeds)?;
    let (inventory, rules) = projection.require_inventory()?;
    Ok((inventory, actions, rules))
}

fn active_relation_inventory_from_seeds(
    index: &MemoryIndexSnapshot<'_, '_>,
    subject: &str,
    max_actions: u32,
    mut actions: u32,
    seeds: MemoryState,
) -> Result<(RelationInventoryProjection, u32)> {
    if actions > max_actions {
        return Err(failure(
            "RELATION_BUDGET",
            "relation candidate scan exceeds the sealed action budget",
        ));
    }

    let remaining = max_actions.checked_sub(actions).ok_or_else(|| {
        failure(
            "RELATION_BUDGET",
            "relation-profile candidate scan exceeds the sealed action budget",
        )
    })?;
    let query_shapes = query_shapes_about(
        &seeds,
        subject,
        usize::try_from(remaining).unwrap_or(usize::MAX),
    )?;
    if query_shapes.is_empty() {
        return Ok((
            RelationInventoryProjection::Missing(
                "no active supported relation query applies to this subject",
            ),
            actions,
        ));
    }
    let query_count = u32::try_from(query_shapes.len())
        .map_err(|_| failure("RELATION_BUDGET", "relation query count overflowed"))?;
    actions = actions.checked_add(query_count).ok_or_else(|| {
        failure(
            "RELATION_BUDGET",
            "relation-profile query planning action count overflowed",
        )
    })?;

    let mut dependencies = BTreeSet::new();
    let mut reported_facts = BTreeMap::new();
    let mut supporting_rules = BTreeMap::new();
    let mut contested_fact_ids = BTreeSet::new();
    let mut has_conflict = false;
    for shape in query_shapes {
        let remaining_actions = max_actions.checked_sub(actions).ok_or_else(|| {
            failure(
                "RELATION_BUDGET",
                "relation-profile query planning exceeds the sealed action budget",
            )
        })?;
        // A longer proof can use the remaining inventory budget. Each shape
        // is charged its actual work, and no inventory is returned until all
        // shapes are evaluated. Reserve one action for the proof evaluator.
        let projection_budget = remaining_actions.checked_sub(1).ok_or_else(|| {
            failure(
                "RELATION_BUDGET",
                "no action remains for relation proof evaluation",
            )
        })?;

        let (projected, projection_actions) =
            active_relation_projection(index, &shape, projection_budget)?;
        for rule in &projected.rules {
            supporting_rules.insert(rule.id.clone(), rule.clone());
        }
        actions = actions.checked_add(projection_actions).ok_or_else(|| {
            failure(
                "RELATION_BUDGET",
                "relation-profile proof projection action count overflowed",
            )
        })?;
        let proof_budget = remaining_actions
            .checked_sub(projection_actions)
            .ok_or_else(|| {
                failure(
                    "RELATION_BUDGET",
                    "relation-profile proof projection exhausted the query allowance",
                )
            })?;
        let answer = query(&projected, &shape, proof_budget)?;
        actions = actions.checked_add(answer.steps).ok_or_else(|| {
            failure(
                "RELATION_BUDGET",
                "relation-profile proof action count overflowed",
            )
        })?;

        match answer.status.as_str() {
            "SUPPORTED" | "CONTRADICTED" => {}
            "CONFLICT" => {
                has_conflict = true;
                contested_fact_ids.extend(answer.facts.iter().map(|fact| fact.id.clone()));
            }
            "UNKNOWN" | "MISSING_PREMISES" => continue,
            _ => {
                return Err(failure(
                    "UNSUPPORTED_RELATION_RESULT",
                    "relation inventory proof returned an unknown terminal status",
                ));
            }
        }
        dependencies.extend(answer_dependencies(&answer));
        for fact in answer.facts {
            reported_facts.insert(fact.id.clone(), fact);
        }
    }

    if reported_facts.is_empty() {
        return Ok((
            RelationInventoryProjection::Missing(
                "no supported relation result remains for this subject",
            ),
            actions,
        ));
    }
    let guarantee = if reported_facts
        .values()
        .any(|fact| matches!(fact.guarantee, Guarantee::Derived))
    {
        Guarantee::Derived
    } else if reported_facts
        .values()
        .any(|fact| matches!(fact.guarantee, Guarantee::UserPremise))
    {
        Guarantee::UserPremise
    } else if reported_facts
        .values()
        .any(|fact| matches!(fact.guarantee, Guarantee::Computed))
    {
        Guarantee::Computed
    } else if reported_facts
        .values()
        .any(|fact| matches!(fact.guarantee, Guarantee::SourceReport))
    {
        Guarantee::SourceReport
    } else {
        Guarantee::Unknown
    };
    let mut facts: Vec<_> = reported_facts.into_values().collect();
    facts.sort_by(|left, right| {
        left.proposition
            .cmp(&right.proposition)
            .then_with(|| left.id.cmp(&right.id))
    });
    Ok((
        RelationInventoryProjection::Supported(
            RelationInventory {
                facts,
                contested_fact_ids,
                has_conflict,
                dependencies: dependencies.into_iter().collect(),
                guarantee,
            },
            supporting_rules.into_values().collect(),
        ),
        actions,
    ))
}

/// Preserve OUT_OF_SCOPE classification only after the proof query is
/// unresolved. Supported queries therefore do not spend budget on diagnostic
/// facts from other scopes.
pub(crate) fn active_out_of_scope_facts(
    index: &MemoryIndexSnapshot<'_, '_>,
    proposition: &Proposition,
    max_actions: u32,
) -> Result<(Vec<Fact>, u32)> {
    if max_actions == 0 {
        return Err(failure(
            "RELATION_BUDGET",
            "out-of-scope classification budget must be positive",
        ));
    }
    let max_records = usize::try_from(max_actions - 1).unwrap_or(usize::MAX);
    let records = index.out_of_scope_direct_facts(proposition, max_records)?;
    let actions = 1_u32
        .checked_add(
            u32::try_from(records.len())
                .map_err(|_| failure("RELATION_BUDGET", "out-of-scope record count overflowed"))?,
        )
        .ok_or_else(|| failure("RELATION_BUDGET", "out-of-scope action count overflowed"))?;
    let mut facts = records
        .into_iter()
        .filter_map(|record| match record.value {
            IndexedMemoryValue::Fact(fact)
                if fact.valid
                    && fact.guarantee == Guarantee::UserPremise
                    && fact.dependencies.is_empty()
                    && !fact.source_input.is_empty() =>
            {
                Some(fact)
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    facts.sort_by(|left, right| left.id.cmp(&right.id));
    Ok((facts, actions))
}

/// Evaluate only the explicitly supplied scenario assumptions as hypotheses.
/// Persistent hypothesis records remain excluded from this bounded projection.
pub(crate) fn query_hypothetical(
    memory: &MemoryState,
    proposition: &Proposition,
    max_steps: u32,
    allowed_hypotheses: &BTreeSet<String>,
) -> Result<QueryOutcome> {
    query_internal(
        memory,
        proposition,
        max_steps,
        Some(allowed_hypotheses),
        None,
    )
}

/// Return direct, active premises that match a query in every typed field
/// except exact world scope. Core uses these only to explain an `OUT_OF_SCOPE`
/// stop; they are never admitted to the query's proof closure.
pub(crate) fn direct_out_of_scope_premises(
    memory: &MemoryState,
    proposition: &Proposition,
    max_steps: u32,
) -> Result<Vec<Fact>> {
    let scanned_facts = u32::try_from(memory.facts.len()).unwrap_or(u32::MAX);
    if max_steps == 0 || scanned_facts > max_steps {
        return Err(failure(
            "RELATION_BUDGET",
            "checking for an exact proposition in another scope exceeds the relation budget",
        ));
    }
    let mut facts = memory
        .facts
        .iter()
        .filter(|fact| {
            fact.valid
                && fact.guarantee == Guarantee::UserPremise
                && fact.dependencies.is_empty()
                && !fact.source_input.is_empty()
                && fact.proposition.scope != proposition.scope
                && fact.proposition.subject == proposition.subject
                && fact.proposition.predicate == proposition.predicate
                && fact.proposition.object == proposition.object
                && fact.proposition.negative == proposition.negative
                && fact.proposition.temporal_scope == proposition.temporal_scope
                && fact.proposition.condition_refs == proposition.condition_refs
        })
        .cloned()
        .collect::<Vec<_>>();
    facts.sort_by(|left, right| left.id.cmp(&right.id));
    Ok(facts)
}

fn direct_fact_matches(
    patterns: &[Proposition],
    known: &BTreeMap<Proposition, Fact>,
) -> Option<Vec<Fact>> {
    fn match_from(
        index: usize,
        patterns: &[Proposition],
        candidates: &[&Fact],
        known: &BTreeMap<Proposition, Fact>,
        bindings: &BTreeMap<String, String>,
        selected: &mut Vec<Fact>,
    ) -> Option<Vec<Fact>> {
        if index == patterns.len() {
            return Some(selected.clone());
        }
        for candidate in candidates {
            if known.contains_key(&opposition(&candidate.proposition)) {
                continue;
            }
            let Some(next_bindings) =
                matches_with_bindings(&patterns[index], &candidate.proposition, bindings)
            else {
                continue;
            };
            selected.push((*candidate).clone());
            if let Some(result) = match_from(
                index + 1,
                patterns,
                candidates,
                known,
                &next_bindings,
                selected,
            ) {
                return Some(result);
            }
            selected.pop();
        }
        None
    }

    let candidates: Vec<_> = known
        .values()
        .filter(|fact| {
            fact.dependencies.is_empty()
                && !matches!(fact.guarantee, Guarantee::Hypothesis | Guarantee::Unknown)
        })
        .collect();
    match_from(
        0,
        patterns,
        &candidates,
        known,
        &BTreeMap::new(),
        &mut Vec::new(),
    )
}

/// Select a composed proof without also expanding its two parent rules when
/// the actor's active rules in that scope are exactly the isolated proof pair
/// plus its shortcut. When the intermediate conclusion is explicitly
/// contradicted, discard that shortcut and let the original path stop at the
/// conflict. More complex rule graphs retain the existing closure behavior.
fn isolated_composition_plan(
    proposition: &Proposition,
    known: &BTreeMap<Proposition, Fact>,
    rules: &[&RelationRule],
    rule_map: &BTreeMap<&str, &RelationRule>,
    allowed_hypotheses: Option<&BTreeSet<String>>,
) -> (BTreeSet<String>, BTreeSet<String>) {
    let mut disabled_shortcuts = BTreeSet::new();
    let mut elided_parent_rules = BTreeSet::new();
    if allowed_hypotheses.is_some()
        || known.values().any(|fact| {
            matches(proposition, &fact.proposition).is_some()
                || matches(&opposition(proposition), &fact.proposition).is_some()
        })
    {
        return (disabled_shortcuts, elided_parent_rules);
    }

    for shortcut in rules.iter().copied().filter(|rule| {
        rule.dependencies.len() == 2
            && (matches(&rule.conclusion, proposition).is_some()
                || matches(&rule.conclusion, &opposition(proposition)).is_some())
    }) {
        let Some(left) = rule_map.get(shortcut.dependencies[0].as_str()).copied() else {
            continue;
        };
        let Some(right) = rule_map.get(shortcut.dependencies[1].as_str()).copied() else {
            continue;
        };
        if !left.dependencies.is_empty() || !right.dependencies.is_empty() {
            continue;
        }
        let same_scope_rules: Vec<_> = rules
            .iter()
            .copied()
            .filter(|rule| {
                rule.premise.scope == proposition.scope
                    && rule.premise.temporal_scope == proposition.temporal_scope
                    && rule.premise.condition_refs == proposition.condition_refs
                    && rule.conclusion.scope == proposition.scope
                    && rule.conclusion.temporal_scope == proposition.temporal_scope
                    && rule.conclusion.condition_refs == proposition.condition_refs
                    && rule.additional_premises.iter().all(|premise| {
                        premise.scope == proposition.scope
                            && premise.temporal_scope == proposition.temporal_scope
                            && premise.condition_refs == proposition.condition_refs
                    })
            })
            .collect();
        let expected = BTreeSet::from([shortcut.id.as_str(), left.id.as_str(), right.id.as_str()]);
        let actual: BTreeSet<_> = same_scope_rules
            .iter()
            .map(|rule| rule.id.as_str())
            .collect();
        if same_scope_rules.len() != 3 || actual != expected {
            continue;
        }

        let shortcut_premises: Vec<_> = std::iter::once(&shortcut.premise)
            .chain(shortcut.additional_premises.iter())
            .cloned()
            .collect();
        let Some(matched_facts) = direct_fact_matches(&shortcut_premises, known) else {
            continue;
        };
        let left_premises: Vec<_> = std::iter::once(&left.premise)
            .chain(left.additional_premises.iter())
            .collect();
        let left_bindings = left_premises.iter().zip(&matched_facts).try_fold(
            BTreeMap::new(),
            |bindings, (pattern, fact)| {
                matches_with_bindings(pattern, &fact.proposition, &bindings)
            },
        );
        let Some(left_bindings) = left_bindings else {
            continue;
        };
        let intermediate = substitute(&left.conclusion, &left_bindings);
        if known.contains_key(&opposition(&intermediate)) {
            disabled_shortcuts.insert(shortcut.id.clone());
        } else {
            elided_parent_rules.insert(left.id.clone());
            elided_parent_rules.insert(right.id.clone());
        }
    }
    (disabled_shortcuts, elided_parent_rules)
}

fn query_internal(
    memory: &MemoryState,
    proposition: &Proposition,
    max_steps: u32,
    allowed_hypotheses: Option<&BTreeSet<String>>,
    projected_evidence: Option<&[usize]>,
) -> Result<QueryOutcome> {
    validate(proposition)?;
    if max_steps == 0 {
        return Err(failure(
            "RELATION_BUDGET",
            "relation budget must be positive",
        ));
    }
    let mut steps = 0u32;
    let (condition_status, condition_evaluations, condition_dependencies) =
        evaluate_condition_refs(
            memory,
            proposition.into(),
            proposition.condition_refs.iter().map(String::as_str),
            allowed_hypotheses,
            &mut ConditionEvaluationBudget {
                max_steps,
                steps: &mut steps,
                projected_evidence,
                evidence: None,
                trace: None,
            },
        )?;
    if condition_status.is_some_and(|status| status != ConditionStatus::True) {
        return Ok(QueryOutcome {
            status: "UNKNOWN".into(),
            facts: Vec::new(),
            used_rules: Vec::new(),
            steps,
            condition_evaluations,
            condition_status,
        });
    }
    let rule_map: BTreeMap<_, _> = memory
        .rules
        .iter()
        .map(|rule| (rule.id.as_str(), rule))
        .collect();
    let fact_map: BTreeMap<_, _> = memory
        .facts
        .iter()
        .map(|fact| (fact.id.as_str(), fact))
        .collect();
    let condition_map: BTreeMap<_, _> = memory
        .conditions
        .iter()
        .map(|condition| (condition.id.as_str(), condition))
        .collect();
    let rules: Vec<_> = memory
        .rules
        .iter()
        .filter(|rule| {
            rule.additional_premises.len() <= 2
                && permitted_rule(rule, &rule_map, &mut BTreeSet::new())
        })
        .collect();
    let mut known = BTreeMap::<Proposition, Fact>::new();
    for fact in &memory.facts {
        if fact.valid
            && fact.dependencies.iter().all(|id| {
                fact_map.get(id.as_str()).is_some_and(|parent| parent.valid)
                    || rule_map
                        .get(id.as_str())
                        .is_some_and(|rule| permitted_rule(rule, &rule_map, &mut BTreeSet::new()))
                    || condition_map
                        .get(id.as_str())
                        .is_some_and(|condition| condition.valid)
            })
            && fact.guarantee != Guarantee::Unknown
            && (fact.guarantee != Guarantee::Hypothesis
                || allowed_hypotheses.is_some_and(|ids| ids.contains(&fact.id)))
        {
            known
                .entry(fact.proposition.clone())
                .or_insert_with(|| fact.clone());
        }
    }
    let (disabled_shortcuts, elided_parent_rules) =
        isolated_composition_plan(proposition, &known, &rules, &rule_map, allowed_hypotheses);
    let rules: Vec<_> = rules
        .into_iter()
        .filter(|rule| {
            !disabled_shortcuts.contains(&rule.id) && !elided_parent_rules.contains(&rule.id)
        })
        .collect();
    let mut applied_single = BTreeSet::new();
    let mut joined_conjunctive = BTreeSet::<(String, Vec<String>)>::new();
    loop {
        let before = known.len();
        let current: Vec<_> = known.values().cloned().collect();
        for fact in current {
            // A contradicted premise cannot support an unqualified consequence.
            if known.contains_key(&opposition(&fact.proposition)) {
                continue;
            }
            for rule in &rules {
                if rule.additional_premises.is_empty() {
                    let Some(bindings) = matches(&rule.premise, &fact.proposition) else {
                        continue;
                    };
                    if !applied_single.insert((rule.id.clone(), fact.id.clone())) {
                        continue;
                    }
                    charge_step(&mut steps, max_steps)?;
                    derive(
                        &mut known,
                        rule,
                        std::slice::from_ref(&fact),
                        &bindings,
                        &condition_dependencies,
                    )?;
                    continue;
                }

                let Some(first_bindings) = matches(&rule.premise, &fact.proposition) else {
                    continue;
                };
                let mut partials = vec![(first_bindings, vec![fact.clone()])];
                for joined_premise in &rule.additional_premises {
                    let candidates: Vec<_> = known.values().cloned().collect();
                    let mut next_partials = Vec::new();
                    for (bindings, matched_facts) in partials {
                        for candidate in &candidates {
                            if known.contains_key(&opposition(&candidate.proposition)) {
                                continue;
                            }
                            let Some(joined_bindings) = matches_with_bindings(
                                joined_premise,
                                &candidate.proposition,
                                &bindings,
                            ) else {
                                continue;
                            };
                            let mut joined_facts = matched_facts.clone();
                            joined_facts.push(candidate.clone());
                            let application = (
                                rule.id.clone(),
                                joined_facts
                                    .iter()
                                    .map(|premise| premise.id.clone())
                                    .collect(),
                            );
                            if joined_conjunctive.insert(application) {
                                charge_step(&mut steps, max_steps)?;
                            }
                            next_partials.push((joined_bindings, joined_facts));
                        }
                    }
                    partials = next_partials;
                    if partials.is_empty() {
                        break;
                    }
                }
                for (bindings, matched_facts) in partials {
                    derive(
                        &mut known,
                        rule,
                        &matched_facts,
                        &bindings,
                        &condition_dependencies,
                    )?;
                }
            }
        }
        if known.len() == before {
            break;
        }
    }
    // A later contradiction invalidates every dependent proof. Retain the
    // contradictory records for diagnostics, but never answer a descendant
    // using a premise that the closure itself has contradicted.
    let contested: BTreeSet<_> = known
        .values()
        .filter(|fact| known.contains_key(&opposition(&fact.proposition)))
        .map(|fact| fact.id.clone())
        .collect();
    let proof_clear = |fact: &&Fact| !fact.dependencies.iter().any(|id| contested.contains(id));
    let mut positive: Vec<_> = known
        .values()
        .filter(proof_clear)
        .filter(|fact| matches(proposition, &fact.proposition).is_some())
        .cloned()
        .collect();
    let negative: Vec<_> = known
        .values()
        .filter(proof_clear)
        .filter(|fact| matches(&opposition(proposition), &fact.proposition).is_some())
        .cloned()
        .collect();
    let status = if !positive.is_empty() && !negative.is_empty() {
        "CONFLICT"
    } else if !positive.is_empty() {
        "SUPPORTED"
    } else if !negative.is_empty() {
        "CONTRADICTED"
    } else {
        "UNKNOWN"
    };
    positive.extend(negative);
    let mut used_rules = BTreeSet::new();
    for fact in &positive {
        for id in &fact.dependencies {
            if rule_map.contains_key(id.as_str()) {
                used_rules.insert(id.clone());
            }
        }
    }
    Ok(QueryOutcome {
        status: status.into(),
        facts: positive,
        used_rules: used_rules.into_iter().collect(),
        steps,
        condition_evaluations,
        condition_status,
    })
}

fn charge_step(steps: &mut u32, max_steps: u32) -> Result<()> {
    *steps = steps
        .checked_add(1)
        .ok_or_else(|| failure("RELATION_BUDGET", "relation step overflow"))?;
    if *steps > max_steps {
        return Err(failure(
            "RELATION_BUDGET",
            "proof expansion exceeded the task relation budget",
        ));
    }
    Ok(())
}

fn derive(
    known: &mut BTreeMap<Proposition, Fact>,
    rule: &RelationRule,
    premises: &[Fact],
    bindings: &BTreeMap<String, String>,
    condition_dependencies: &BTreeSet<String>,
) -> Result<()> {
    let conclusion = substitute(&rule.conclusion, bindings);
    if !variables(&conclusion).is_empty() {
        return Err(failure(
            "UNBOUND_VARIABLE",
            "derived conclusion remains unbound",
        ));
    }
    if known.contains_key(&conclusion) {
        return Ok(());
    }
    let mut dependencies: BTreeSet<_> = premises
        .iter()
        .flat_map(|fact| {
            fact.dependencies
                .iter()
                .cloned()
                .chain(std::iter::once(fact.id.clone()))
        })
        .collect();
    dependencies.insert(rule.id.clone());
    dependencies.extend(rule.dependencies.iter().cloned());
    dependencies.extend(condition_dependencies.iter().cloned());
    let dependencies: Vec<_> = dependencies.into_iter().collect();
    let id = format!(
        "fact:{}",
        identity(&("derived-v1", &conclusion, &dependencies))?
    );
    known.insert(
        conclusion.clone(),
        Fact {
            id,
            proposition: conclusion,
            guarantee: if premises
                .iter()
                .any(|premise| premise.guarantee == Guarantee::Hypothesis)
            {
                Guarantee::Hypothesis
            } else {
                Guarantee::Derived
            },
            source_input: premises
                .first()
                .map_or_else(String::new, |fact| fact.source_input.clone()),
            source_evidence: None,
            dependencies,
            valid: true,
        },
    );
    Ok(())
}

/// Retain the queried fact and every transitive premise/rule identity used by
/// its proof. This makes a Core answer's dependency ledger independently
/// traceable to the user-provided roots.
pub fn answer_dependencies(answer: &QueryOutcome) -> Vec<String> {
    let mut dependencies = BTreeSet::new();
    for fact in &answer.facts {
        dependencies.insert(fact.id.clone());
        dependencies.extend(fact.dependencies.iter().cloned());
    }
    dependencies.extend(answer.used_rules.iter().cloned());
    for evaluation in &answer.condition_evaluations {
        dependencies.extend(evaluation.dependencies.iter().cloned());
    }
    dependencies.into_iter().collect()
}

/// A direct user premise stays a premise even when it refutes the asked
/// proposition; rule-backed conclusions remain derived.
pub fn answer_guarantee(answer: &QueryOutcome) -> Guarantee {
    if answer
        .facts
        .iter()
        .any(|fact| fact.guarantee == Guarantee::Hypothesis)
    {
        Guarantee::Hypothesis
    } else if answer.used_rules.is_empty() {
        Guarantee::UserPremise
    } else {
        Guarantee::Derived
    }
}

/// Preserve originals and append validity changes in the Core's next durable
/// memory revision. No counterexample or rule record is physically deleted.
pub fn invalidate(memory: &mut MemoryState, id: &str) -> Result<Vec<String>> {
    invalidate_in_order(memory, std::iter::once(id))
}

/// Return each root's new affected IDs in lexical order, processing roots in
/// caller order. Each root still advances the candidate memory revision.
pub(crate) fn invalidate_in_order<'a>(
    memory: &mut MemoryState,
    roots: impl Iterator<Item = &'a str> + Clone,
) -> Result<Vec<String>> {
    let Some(first) = roots.clone().next() else {
        return Ok(Vec::new());
    };
    let records = memory
        .facts
        .iter()
        .map(|fact| (&fact.id, &fact.dependencies))
        .chain(
            memory
                .rules
                .iter()
                .map(|rule| (&rule.id, &rule.dependencies)),
        )
        .chain(
            memory
                .conditions
                .iter()
                .map(|condition| (&condition.id, &condition.dependencies)),
        );
    let mut missing: BTreeSet<_> = roots.clone().collect();
    for (record_id, _) in records.clone() {
        missing.remove(record_id.as_str());
        if missing.is_empty() {
            break;
        }
    }
    let unknown_id = || {
        failure(
            "UNKNOWN_MEMORY_ID",
            "no fact or rule has the requested identity",
        )
    };
    if missing.contains(first) {
        return Err(unknown_id());
    }

    let mut dependents = BTreeMap::<&str, Vec<&str>>::new();
    for (record_id, dependencies) in records {
        for dependency in dependencies {
            dependents.entry(dependency).or_default().push(record_id);
        }
    }
    let mut first_root = BTreeMap::new();
    let mut pending = VecDeque::new();
    let mut revision = memory.revision;
    let status: Result<()> = (|| {
        for (ordinal, id) in roots.enumerate() {
            if missing.contains(id) {
                return Err(unknown_id());
            }
            first_root.entry(id).or_insert_with(|| {
                pending.push_back(id);
                ordinal
            });
            // Inactive records still carry edges. Each reached ID belongs to
            // its first root, so later roots can reuse the traversed paths.
            while let Some(dependency) = pending.pop_front() {
                for &record_id in dependents.get(dependency).into_iter().flatten() {
                    first_root.entry(record_id).or_insert_with(|| {
                        pending.push_back(record_id);
                        ordinal
                    });
                }
            }
            revision = next_memory_revision(revision)?;
        }
        Ok(())
    })();
    let mut invalid: Vec<_> = first_root
        .into_iter()
        .map(|(id, ordinal)| (id.to_owned(), ordinal))
        .collect();
    let is_invalid = |id: &str| {
        invalid
            .binary_search_by(|(affected, _)| affected.as_str().cmp(id))
            .is_ok()
    };
    for fact in &mut memory.facts {
        if is_invalid(&fact.id) {
            fact.valid = false;
        }
    }
    for rule in &mut memory.rules {
        if is_invalid(&rule.id) {
            rule.valid = false;
        }
    }
    for condition in &mut memory.conditions {
        if is_invalid(&condition.id) {
            condition.valid = false;
        }
    }
    // Keep the mutations up to the same stopping point as sequential calls.
    memory.revision = revision;
    status?;
    invalid.sort_by_key(|(_, ordinal)| *ordinal);
    Ok(invalid.into_iter().map(|(id, _)| id).collect())
}

pub fn format_proposition(proposition: &Proposition) -> String {
    let surface = if proposition.predicate == "is" {
        format!(
            "{}は{}{}",
            proposition.subject,
            proposition.object,
            if proposition.negative {
                "ではない"
            } else {
                "である"
            }
        )
    } else {
        format!(
            "{} {} {}{}",
            proposition.subject,
            proposition.predicate,
            proposition.object,
            if proposition.negative {
                " (否定)"
            } else {
                ""
            }
        )
    };
    let surface = proposition
        .temporal_scope
        .as_ref()
        .map_or(surface.clone(), |time| {
            format!("{surface}（時刻範囲: {time}）")
        });
    if proposition.condition_refs.is_empty() {
        surface
    } else {
        format!(
            "{surface}（条件参照: {}）",
            proposition
                .condition_refs
                .iter()
                .cloned()
                .collect::<Vec<_>>()
                .join(", ")
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn p(subject: &str, object: &str) -> Proposition {
        Proposition {
            subject: subject.into(),
            predicate: "is".into(),
            object: object.into(),
            negative: false,
            scope: "premise-world".into(),
            temporal_scope: None,
            condition_refs: Default::default(),
        }
    }

    #[test]
    fn a_guarded_rule_cannot_reuse_the_conclusion_subjects_condition_for_a_different_premise() {
        let mut memory = MemoryState::default();
        let guard = |subject, object| Proposition {
            condition_refs: BTreeSet::from(["ready".into()]),
            ..p(subject, object)
        };
        define_condition(
            &mut memory,
            ConditionDefinition {
                condition_id: "ready".into(),
                subject: "sensor".into(),
                scope: "premise-world".into(),
                expression: ConditionExpression::Fact {
                    predicate: "is".into(),
                    object: "ready".into(),
                },
                valid_from: None,
                valid_until: None,
                dependencies: BTreeSet::new(),
            },
            "condition",
        )
        .unwrap();
        assert_fact(&mut memory, p("sensor", "ready"), "evidence").unwrap();
        assert_fact(&mut memory, guard("other", "checked"), "other").unwrap();
        add_rule(
            &mut memory,
            guard("other", "checked"),
            guard("sensor", "allowed"),
            "mixed-subject-rule",
        )
        .unwrap();
        let query_target = guard("sensor", "reported");
        add_rule(
            &mut memory,
            guard("$x", "allowed"),
            guard("$x", "reported"),
            "same-subject-rule",
        )
        .unwrap();
        assert!(memory.rules.iter().any(|rule| rule.dependencies.len() == 2));
        let blocked = query(&memory, &query_target, 80).unwrap();
        assert_eq!(blocked.condition_status, Some(ConditionStatus::True));
        assert_eq!(blocked.status, "UNKNOWN");
        assert!(blocked.facts.is_empty());
        assert_fact(&mut memory, guard("sensor", "allowed"), "own").unwrap();
        assert_eq!(
            query(&memory, &query_target, 80).unwrap().status,
            "SUPPORTED"
        );
    }

    #[test]
    fn invalidation_follows_record_dependencies_through_inactive_and_cyclic_paths() {
        let mut memory = MemoryState::default();
        let ready = ConditionDefinition {
            condition_id: "ready".into(),
            subject: "sensor".into(),
            scope: "premise-world".into(),
            expression: ConditionExpression::Fact {
                predicate: "is".into(),
                object: "ready".into(),
            },
            valid_from: None,
            valid_until: None,
            dependencies: BTreeSet::new(),
        };
        let root = define_condition(&mut memory, ready.clone(), "root-input").unwrap();
        let dependent = define_condition(
            &mut memory,
            ConditionDefinition {
                condition_id: "monitored".into(),
                expression: ConditionExpression::ConditionRef {
                    condition_id: "ready".into(),
                },
                dependencies: BTreeSet::from(["ready".into()]),
                ..ready
            },
            "dependent-input",
        )
        .unwrap();
        let rule = add_rule(&mut memory, p("$x", "ready"), p("$x", "seen"), "rule-input").unwrap();
        memory.rules[0].dependencies.push(dependent.id.clone());
        memory.rules[0].valid = false;

        let mut guarded = p("sensor", "recorded");
        guarded.condition_refs.insert("ready".into());
        assert_fact(&mut memory, guarded, "guarded-input").unwrap();
        let fact_id = |index| format!("chain:{index:03}");
        for index in (0..128).rev() {
            let dependencies = if index == 0 {
                // The bridge reaches a cycle between the first and last Facts.
                vec![rule.id.clone(), fact_id(127), rule.id.clone()]
            } else {
                vec![fact_id(index - 1)]
            };
            memory.facts.push(Fact {
                id: fact_id(index),
                proposition: p("sensor", "seen"),
                guarantee: Guarantee::Derived,
                source_input: "chain-input".into(),
                source_evidence: None,
                dependencies,
                valid: true,
            });
        }

        let before = memory.clone();
        assert!(matches!(
            invalidate(&mut memory, "unknown"),
            Err(Error::Contract { code, .. }) if code == "UNKNOWN_MEMORY_ID"
        ));
        assert_eq!(memory, before);

        let mut affected = vec![root.id.clone(), dependent.id.clone(), rule.id];
        affected.extend((0..128).map(fact_id));
        affected.sort();
        let mut expected = before;
        expected
            .conditions
            .iter_mut()
            .for_each(|record| record.valid = false);
        expected.facts[1..]
            .iter_mut()
            .for_each(|record| record.valid = false);
        for _ in 0..2 {
            expected.revision += 1;
            assert_eq!(invalidate(&mut memory, &root.id).unwrap(), affected);
            assert_eq!(memory, expected);
        }
    }

    #[test]
    fn ordered_invalidation_shares_dependents_and_preserves_the_stop_point() {
        let fact = |id: &str, dependencies: &[&str]| Fact {
            id: id.into(),
            proposition: p("sensor", "ready"),
            guarantee: Guarantee::UserPremise,
            source_input: id.into(),
            source_evidence: None,
            dependencies: dependencies.iter().map(|id| (*id).into()).collect(),
            valid: true,
        };
        let original = MemoryState {
            revision: 10,
            facts: vec![
                fact("z", &[]),
                fact("a", &[]),
                fact("m", &["z", "a"]),
                fact("b", &["a"]),
                fact("unrelated", &[]),
            ],
            ..MemoryState::default()
        };
        let mut memory = original.clone();
        assert_eq!(
            invalidate_in_order(&mut memory, ["z", "a", "z"].into_iter()).unwrap(),
            ["m", "z", "a", "b"]
        );
        let mut expected = original.clone();
        expected.facts[..4]
            .iter_mut()
            .for_each(|fact| fact.valid = false);
        expected.revision = 13;
        assert_eq!(memory, expected);
        assert!(
            invalidate_in_order(&mut memory, std::iter::empty())
                .unwrap()
                .is_empty()
        );
        assert_eq!(memory, expected);

        let mut memory = original.clone();
        assert!(matches!(
            invalidate_in_order(&mut memory, ["z", "missing", "a"].into_iter()),
            Err(Error::Contract { code, .. }) if code == "UNKNOWN_MEMORY_ID"
        ));
        let mut expected_prefix = original.clone();
        expected_prefix.facts[0].valid = false;
        expected_prefix.facts[2].valid = false;
        expected_prefix.revision = 11;
        assert_eq!(memory, expected_prefix);

        let mut memory = original;
        memory.revision = u64::MAX - 1;
        assert!(matches!(
            invalidate_in_order(&mut memory, ["z", "a", "unrelated"].into_iter()),
            Err(Error::Contract { code, .. }) if code == "MEMORY_REVISION_LIMIT"
        ));
        expected.revision = u64::MAX;
        assert_eq!(memory, expected);
    }

    #[test]
    fn condition_evidence_reuse_keeps_guard_evaluation_on_the_active_path() {
        let mut memory = MemoryState::default();
        for (condition_id, object) in [("a", "calibrated"), ("b", "powered")] {
            define_condition(
                &mut memory,
                ConditionDefinition {
                    condition_id: condition_id.into(),
                    subject: "sensor".into(),
                    scope: "premise-world".into(),
                    expression: ConditionExpression::Fact {
                        predicate: "is".into(),
                        object: object.into(),
                    },
                    valid_from: None,
                    valid_until: None,
                    dependencies: BTreeSet::new(),
                },
                condition_id,
            )
            .unwrap();
        }
        let mut guarded_calibration = p("sensor", "calibrated");
        guarded_calibration.condition_refs.insert("b".into());
        assert_fact(&mut memory, guarded_calibration, "guarded-calibration").unwrap();
        let calibration =
            assert_fact(&mut memory, p("sensor", "calibrated"), "calibration").unwrap();
        let mut guarded_power = p("sensor", "powered");
        guarded_power.condition_refs.insert("a".into());
        let power = assert_fact(&mut memory, guarded_power, "guarded-power").unwrap();
        let mut target = p("sensor", "ready");
        target.condition_refs.extend(["a".into(), "b".into()]);
        let fact = assert_fact(&mut memory, target.clone(), "ready").unwrap();

        let answer = query(&memory, &target, 128).unwrap();
        assert_eq!(answer.status, "SUPPORTED");
        assert_eq!(answer.facts, vec![fact]);
        assert_eq!(answer.condition_status, Some(ConditionStatus::True));
        let [a, b] = answer.condition_evaluations.as_slice() else {
            panic!("expected both condition evaluations");
        };
        assert_eq!(a.status, ConditionStatus::True);
        assert_eq!(a.evidence_fact_ids, vec![calibration.id.clone()]);
        // The nested evaluation of b under a encounters a cycle. The
        // top-level b must evaluate again with an empty visiting set.
        assert_eq!(b.status, ConditionStatus::True);
        assert!(b.evidence_fact_ids.contains(&power.id));
        assert!(b.evidence_fact_ids.contains(&calibration.id));
    }

    #[test]
    fn older_direct_fact_without_optional_source_evidence_remains_wire_compatible() {
        let old_json = serde_json::json!({
            "id": "fact:legacy",
            "proposition": {
                "subject": "legacy subject",
                "predicate": "is",
                "object": "legacy object",
                "negative": false,
                "scope": "premise-world"
            },
            "guarantee": "USER_PREMISE",
            "source_input": "legacy-input-fingerprint",
            "dependencies": [],
            "valid": true
        });
        let old_fact: Fact = serde_json::from_value(old_json)
            .expect("older memory records omit the additive source evidence field");
        assert!(old_fact.source_evidence.is_none());
        assert!(
            serde_json::to_value(old_fact)
                .unwrap()
                .get("source_evidence")
                .is_none()
        );
    }

    #[test]
    fn unseen_entity_uses_proof_composition_and_retains_dependency() {
        let mut memory = MemoryState::default();
        let first = add_rule(&mut memory, p("$x", "鳥"), p("$x", "動物"), "rule-input-1").unwrap();
        add_rule(
            &mut memory,
            p("$y", "動物"),
            p("$y", "生物"),
            "rule-input-2",
        )
        .unwrap();
        assert!(
            memory
                .rules
                .iter()
                .any(|rule| !rule.dependencies.is_empty())
        );
        assert_fact(&mut memory, p("初見の個体", "鳥"), "new-input").unwrap();
        let result = query(&memory, &p("初見の個体", "生物"), 100).unwrap();
        assert_eq!(result.status, "SUPPORTED");
        assert_eq!(result.facts[0].guarantee, Guarantee::Derived);
        assert!(!result.used_rules.is_empty());
        invalidate(&mut memory, &first.id).unwrap();
        assert_eq!(
            query(&memory, &p("初見の個体", "生物"), 100)
                .unwrap()
                .status,
            "UNKNOWN"
        );
        assert_eq!(memory.rules.len(), 3);
    }
    #[test]
    fn query_answer_dependencies_retain_root_premise_and_rule() {
        let mut memory = MemoryState::default();
        let rule = add_rule(&mut memory, p("$x", "鳥"), p("$x", "動物"), "rule-input").unwrap();
        let fact = assert_fact(&mut memory, p("初見の個体", "鳥"), "new-input").unwrap();
        let answer = query(&memory, &p("初見の個体", "動物"), 100).unwrap();
        assert_eq!(answer.status, "SUPPORTED");
        assert_eq!(answer_guarantee(&answer), Guarantee::Derived);
        let dependencies = answer_dependencies(&answer);
        assert!(dependencies.contains(&fact.id));
        assert!(dependencies.contains(&rule.id));
        assert!(dependencies.iter().any(|id| id.starts_with("fact:")));
    }
    #[test]
    fn explicit_premise_listing_excludes_invalid_derived_and_other_subject_facts() {
        let mut memory = MemoryState::default();
        let rule = add_rule(&mut memory, p("$x", "鳥"), p("$x", "動物"), "rule-input").unwrap();
        let direct = assert_fact(&mut memory, p("ココ", "鳥"), "coco-input").unwrap();
        assert_fact(&mut memory, p("ミケ", "猫"), "mike-input").unwrap();
        let mut negative = p("ココ", "鳥");
        negative.negative = true;
        let direct_negative = assert_fact(&mut memory, negative, "counterexample-input").unwrap();
        invalidate(&mut memory, &direct_negative.id).unwrap();

        let listed = explicit_premises_about(&memory, "ココ");
        assert_eq!(listed, vec![direct]);
        assert!(!listed.iter().any(|fact| fact.id == rule.id));
    }
    #[test]
    fn contradicted_direct_premise_keeps_user_premise_guarantee() {
        let mut memory = MemoryState::default();
        let mut negative = p("個体", "動物");
        negative.negative = true;
        let fact = assert_fact(&mut memory, negative, "counterexample").unwrap();
        let answer = query(&memory, &p("個体", "動物"), 100).unwrap();
        assert_eq!(answer.status, "CONTRADICTED");
        assert_eq!(answer_guarantee(&answer), Guarantee::UserPremise);
        assert_eq!(answer_dependencies(&answer), vec![fact.id]);
    }
    #[test]
    fn contradictory_premises_cannot_drive_descendants() {
        let mut memory = MemoryState::default();
        assert_fact(&mut memory, p("個体", "鳥"), "positive").unwrap();
        let mut negative = p("個体", "鳥");
        negative.negative = true;
        assert_fact(&mut memory, negative, "negative").unwrap();
        add_rule(&mut memory, p("$x", "鳥"), p("$x", "動物"), "rule").unwrap();
        assert_eq!(
            query(&memory, &p("個体", "鳥"), 100).unwrap().status,
            "CONFLICT"
        );
        assert_eq!(
            query(&memory, &p("個体", "動物"), 100).unwrap().status,
            "UNKNOWN"
        );
    }
    #[test]
    fn wrong_scope_and_unbound_conclusions_are_rejected() {
        let mut memory = MemoryState::default();
        assert!(add_rule(&mut memory, p("$x", "鳥"), p("$z", "動物"), "source").is_err());
        assert_fact(&mut memory, p("個体", "鳥"), "source").unwrap();
        let mut other = p("個体", "鳥");
        other.scope = "outside-world".into();
        assert_eq!(query(&memory, &other, 100).unwrap().status, "UNKNOWN");
        assert!(query(&memory, &other, 0).is_err());
    }
}

#[cfg(test)]
mod contrary_evidence_tests {
    use super::*;
    #[test]
    fn contradicted_answer_retains_actual_negative_fact_and_proof() {
        let mut memory = MemoryState::default();
        let base = Proposition {
            subject: "対象".into(),
            predicate: "is".into(),
            object: "鳥".into(),
            negative: false,
            scope: "story".into(),
            temporal_scope: None,
            condition_refs: Default::default(),
        };
        let mut conclusion = base.clone();
        conclusion.object = "機械".into();
        conclusion.negative = true;
        let source = assert_fact(&mut memory, base.clone(), "actual-premise").unwrap();
        let rule = add_rule(
            &mut memory,
            base.clone(),
            conclusion.clone(),
            "conditional-premise",
        )
        .unwrap();
        conclusion.negative = false;
        let result = query(&memory, &conclusion, 20).unwrap();
        assert_eq!(result.status, "CONTRADICTED");
        assert_eq!(result.facts.len(), 1);
        assert!(result.facts[0].proposition.negative);
        assert!(result.facts[0].dependencies.contains(&source.id));
        assert!(result.used_rules.contains(&rule.id));
    }
}

#[cfg(test)]
mod conjunctive_rule_tests {
    use super::*;

    fn p(subject: &str, object: &str) -> Proposition {
        Proposition {
            subject: subject.into(),
            predicate: "is".into(),
            object: object.into(),
            negative: false,
            scope: "biology".into(),
            temporal_scope: None,
            condition_refs: Default::default(),
        }
    }

    #[test]
    fn single_premise_rule_identity_stays_compatible() {
        let premise = p("$x", "bird");
        let conclusion = p("$x", "animal");
        let expected = format!(
            "rule:{}",
            identity(&("declared-rule-v1", &premise, &conclusion, "original-input")).unwrap()
        );
        let rule = add_rule(
            &mut MemoryState::default(),
            premise,
            conclusion,
            "original-input",
        )
        .unwrap();
        assert_eq!(rule.id, expected);
    }

    #[test]
    fn single_premise_composition_identity_stays_compatible() {
        let mut memory = MemoryState::default();
        let first_premise = p("$x", "bird");
        let first_conclusion = p("$x", "animal");
        let first = add_rule(
            &mut memory,
            first_premise,
            first_conclusion,
            "first-rule-input",
        )
        .unwrap();
        let second = add_rule(
            &mut memory,
            p("$y", "animal"),
            p("$y", "organism"),
            "second-rule-input",
        )
        .unwrap();
        let composed_premise = p("$v0", "bird");
        let composed_conclusion = p("$v0", "organism");
        let dependencies = vec![first.id.clone(), second.id.clone()];
        let expected = format!(
            "rule:{}",
            identity(&(
                "proof-composition-v1",
                &composed_premise,
                &composed_conclusion,
                &dependencies
            ))
            .unwrap()
        );
        assert!(memory.rules.iter().any(|rule| rule.id == expected));
    }

    #[test]
    fn shared_variable_join_binds_a_new_conclusion_subject() {
        let mut memory = MemoryState::default();
        let rule = add_rule_with_premises(
            &mut memory,
            vec![p("$x", "bird"), p("$x", "$y")],
            p("$y", "animal"),
            "shared-variable-rule-input",
        )
        .unwrap();
        let bird = assert_fact(&mut memory, p("osprey", "bird"), "bird-input").unwrap();
        let relation = assert_fact(&mut memory, p("osprey", "Robin"), "relation-input").unwrap();
        let answer = query(&memory, &p("Robin", "animal"), 32).unwrap();
        assert_eq!(answer.status, "SUPPORTED");
        let dependencies = answer_dependencies(&answer);
        assert!(dependencies.contains(&bird.id));
        assert!(dependencies.contains(&relation.id));
        assert!(dependencies.contains(&rule.id));
    }

    #[test]
    fn two_premise_join_transfers_to_a_new_subject_and_keeps_every_dependency() {
        let mut memory = MemoryState::default();
        let conjunction = add_rule_with_premises(
            &mut memory,
            vec![p("$x", "bird"), p("$x", "feathered")],
            p("$x", "warm-blooded"),
            "conjunctive-rule-input",
        )
        .unwrap();
        let follow_up = add_rule(
            &mut memory,
            p("$x", "warm-blooded"),
            p("$x", "animal"),
            "single-rule-input",
        )
        .unwrap();
        assert_eq!(memory.rules.len(), 3);
        let composed = memory
            .rules
            .iter()
            .find(|candidate| {
                candidate.dependencies.len() == 2
                    && candidate.additional_premises.len() == 1
                    && candidate.conclusion == p("$v0", "animal")
            })
            .cloned()
            .expect("the composed shortcut must retain both antecedents");
        assert_eq!(composed.premise, p("$v0", "bird"));
        assert_eq!(composed.additional_premises[0], p("$v0", "feathered"));
        assert!(composed.dependencies.contains(&conjunction.id));
        assert!(composed.dependencies.contains(&follow_up.id));

        let bird = assert_fact(&mut memory, p("osprey", "bird"), "new-bird-input").unwrap();
        assert_eq!(
            query(&memory, &p("osprey", "animal"), 32).unwrap().status,
            "UNKNOWN",
            "one antecedent must not satisfy the join"
        );
        let feathers =
            assert_fact(&mut memory, p("osprey", "feathered"), "new-feather-input").unwrap();
        let mut roots_only = memory.clone();
        roots_only.rules.retain(|rule| rule.dependencies.is_empty());
        let uncomposed = query(&roots_only, &p("osprey", "animal"), 32).unwrap();
        assert_eq!(uncomposed.status, "SUPPORTED");
        assert_eq!(uncomposed.steps, 2);

        let answer = query(&memory, &p("osprey", "animal"), 32).unwrap();
        assert_eq!(answer.status, "SUPPORTED");
        assert_eq!(
            answer.steps, 1,
            "the isolated shortcut is used without expanding its parent rules"
        );
        assert_eq!(query(&memory, &p("osprey", "animal"), 1).unwrap().steps, 1);
        assert_eq!(answer_guarantee(&answer), Guarantee::Derived);
        let dependencies = answer_dependencies(&answer);
        assert!(dependencies.contains(&bird.id));
        assert!(dependencies.contains(&feathers.id));
        assert!(dependencies.contains(&conjunction.id));
        assert!(dependencies.contains(&follow_up.id));
        assert!(dependencies.contains(&composed.id));

        invalidate(&mut memory, &feathers.id).unwrap();
        assert_eq!(
            query(&memory, &p("osprey", "animal"), 32).unwrap().status,
            "UNKNOWN",
            "invalidating either antecedent must remove the joined proof"
        );
    }

    #[test]
    fn a_composed_shortcut_does_not_bypass_a_contradicted_intermediate() {
        let mut memory = MemoryState::default();
        add_rule(
            &mut memory,
            p("$x", "bird"),
            p("$x", "warm-blooded"),
            "root-bird-rule",
        )
        .unwrap();
        add_rule_with_premises(
            &mut memory,
            vec![p("$x", "warm-blooded"), p("$x", "mammal")],
            p("$x", "animal"),
            "root-animal-rule",
        )
        .unwrap();
        assert_fact(&mut memory, p("osprey", "bird"), "osprey-bird").unwrap();
        assert_fact(&mut memory, p("osprey", "mammal"), "osprey-mammal").unwrap();
        let mut contradicted = p("osprey", "warm-blooded");
        contradicted.negative = true;
        assert_fact(&mut memory, contradicted, "osprey-not-warm-blooded").unwrap();

        let answer = query(&memory, &p("osprey", "animal"), 32).unwrap();
        assert_eq!(answer.status, "UNKNOWN");
        assert!(answer.facts.is_empty());
    }

    #[test]
    fn a_single_rule_before_a_two_premise_rule_keeps_the_other_antecedent() {
        let mut memory = MemoryState::default();
        let first = add_rule(
            &mut memory,
            p("$x", "bird"),
            p("$x", "feathered"),
            "single-rule-input",
        )
        .unwrap();
        let second = add_rule_with_premises(
            &mut memory,
            vec![p("$x", "feathered"), p("$x", "mammal")],
            p("$x", "animal"),
            "conjunctive-rule-input",
        )
        .unwrap();
        let composed = memory
            .rules
            .iter()
            .find(|candidate| {
                candidate.dependencies.len() == 2 && candidate.conclusion == p("$v0", "animal")
            })
            .cloned()
            .expect("single-to-conjunctive composition should retain its unmatched premise");
        assert_eq!(composed.premise, p("$v0", "bird"));
        assert_eq!(composed.additional_premises, vec![p("$v0", "mammal")]);
        assert!(composed.dependencies.contains(&first.id));
        assert!(composed.dependencies.contains(&second.id));

        let bird = assert_fact(&mut memory, p("osprey", "bird"), "bird-input").unwrap();
        let mammal = assert_fact(&mut memory, p("osprey", "mammal"), "mammal-input").unwrap();
        let answer = query(&memory, &p("osprey", "animal"), 32).unwrap();
        assert_eq!(answer.status, "SUPPORTED");
        let dependencies = answer_dependencies(&answer);
        assert!(dependencies.contains(&bird.id));
        assert!(dependencies.contains(&mammal.id));
        assert!(dependencies.contains(&first.id));
        assert!(dependencies.contains(&second.id));
        assert!(dependencies.contains(&composed.id));
    }

    #[test]
    fn two_two_premise_rules_compose_to_a_bounded_three_antecedent_shortcut() {
        let mut memory = MemoryState::default();
        let first = add_rule_with_premises(
            &mut memory,
            vec![p("$x", "bird"), p("$x", "feathered")],
            p("$x", "warm-blooded"),
            "bird-and-feathered-rule",
        )
        .unwrap();
        let second = add_rule_with_premises(
            &mut memory,
            vec![p("$x", "warm-blooded"), p("$x", "mammal")],
            p("$x", "animal"),
            "warm-blooded-and-mammal-rule",
        )
        .unwrap();
        let shortcut = memory
            .rules
            .iter()
            .find(|candidate| {
                candidate.dependencies.len() == 2
                    && candidate.additional_premises.len() == 2
                    && candidate.conclusion == p("$v0", "animal")
            })
            .cloned()
            .expect("two conjunctive parent rules should create a three-premise shortcut");
        assert_eq!(shortcut.premise, p("$v0", "bird"));
        assert_eq!(
            shortcut.additional_premises,
            vec![p("$v0", "feathered"), p("$v0", "mammal")]
        );
        assert!(shortcut.dependencies.contains(&first.id));
        assert!(shortcut.dependencies.contains(&second.id));
        assert!(shortcut.id.starts_with("rule:"));

        let bird = assert_fact(&mut memory, p("osprey", "bird"), "osprey-bird").unwrap();
        let feathered =
            assert_fact(&mut memory, p("osprey", "feathered"), "osprey-feathered").unwrap();
        let mammal = assert_fact(&mut memory, p("osprey", "mammal"), "osprey-mammal").unwrap();
        let answer = query(&memory, &p("osprey", "animal"), 64).unwrap();
        assert_eq!(answer.status, "SUPPORTED");
        assert_eq!(answer_guarantee(&answer), Guarantee::Derived);
        let dependencies = answer_dependencies(&answer);
        for identity in [
            &bird.id,
            &feathered.id,
            &mammal.id,
            &first.id,
            &second.id,
            &shortcut.id,
        ] {
            assert!(dependencies.contains(identity), "missing {identity}");
        }
        assert!(
            answer.steps <= 64,
            "multi-stage joins must remain within the query budget"
        );
        assert!(matches!(
            query(&memory, &p("osprey", "animal"), 1),
            Err(Error::Contract { code, .. }) if code == "RELATION_BUDGET"
        ));

        let mut invalidated_rule_memory = memory.clone();
        invalidate(&mut invalidated_rule_memory, &first.id).unwrap();
        assert_eq!(
            query(&invalidated_rule_memory, &p("osprey", "animal"), 64)
                .unwrap()
                .status,
            "UNKNOWN",
            "invalidating a parent rule must invalidate the composed shortcut"
        );

        invalidate(&mut memory, &mammal.id).unwrap();
        assert_eq!(
            query(&memory, &p("osprey", "animal"), 64).unwrap().status,
            "UNKNOWN",
            "invalidating one of the three direct antecedents removes the shortcut proof"
        );
    }

    #[test]
    fn declared_relation_rules_remain_limited_to_two_explicit_premises() {
        let mut memory = MemoryState::default();
        let result = add_rule_with_premises(
            &mut memory,
            vec![p("$x", "bird"), p("$x", "feathered"), p("$x", "mammal")],
            p("$x", "animal"),
            "three-premise-user-rule",
        );
        assert!(matches!(
            result,
            Err(Error::Contract { code, .. }) if code == "UNSUPPORTED_RULE_PREMISE_COUNT"
        ));
        assert!(memory.rules.is_empty());
    }

    #[test]
    fn older_serialized_relation_rules_default_to_single_premise() {
        let old_record = serde_json::json!({
            "id": "rule:legacy",
            "premise": p("$x", "bird"),
            "conclusion": p("$x", "animal"),
            "source_input": "legacy-input",
            "dependencies": [],
            "valid": true
        });
        let rule: RelationRule = serde_json::from_value(old_record).unwrap();
        assert!(rule.additional_premises.is_empty());
    }
}
