//! Scoped proof-preserving relations. Source premises are never world facts.
//! Declared rules support one or two premises; internal shortcuts are bounded
//! to three antecedents and retain their parent proof DAG.
use minidora_domain::*;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct QueryOutcome {
    pub status: String,
    pub facts: Vec<Fact>,
    pub used_rules: Vec<String>,
    pub steps: u32,
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
fn validate(proposition: &Proposition) -> Result<()> {
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
    if variable(&proposition.predicate) || variable(&proposition.scope) {
        return Err(failure(
            "UNSUPPORTED_RELATION",
            "predicate and scope variables are not supported",
        ));
    }
    Ok(())
}
fn bump(memory: &mut MemoryState) -> Result<()> {
    memory.revision = memory
        .revision
        .checked_add(1)
        .ok_or_else(|| failure("MEMORY_REVISION_LIMIT", "memory revision overflow"))?;
    Ok(())
}

pub fn assert_fact(
    memory: &mut MemoryState,
    proposition: Proposition,
    source_input: &str,
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
    let id = format!(
        "fact:{}",
        identity(&("user-premise-v1", &proposition, source_input))?
    );
    if let Some(old) = memory.facts.iter().find(|fact| fact.id == id) {
        return Ok(old.clone());
    }
    let fact = Fact {
        id,
        proposition,
        guarantee: Guarantee::UserPremise,
        source_input: source_input.into(),
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
    let id = if additional_premises.is_empty() {
        format!(
            "rule:{}",
            identity(&("declared-rule-v1", premise, &conclusion, source_input))?
        )
    } else {
        format!(
            "rule:{}",
            identity(&(
                "declared-conjunctive-rule-v1",
                &premises,
                &conclusion,
                source_input
            ))?
        )
    };
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

fn permitted_rule<'a>(
    rule: &'a RelationRule,
    rules: &BTreeMap<&str, &'a RelationRule>,
    seen: &mut BTreeSet<String>,
) -> bool {
    if !rule.valid || !seen.insert(rule.id.clone()) {
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
fn opposition(proposition: &Proposition) -> Proposition {
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
    validate(proposition)?;
    if max_steps == 0 {
        return Err(failure(
            "RELATION_BUDGET",
            "relation budget must be positive",
        ));
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
            })
            && !matches!(fact.guarantee, Guarantee::Hypothesis | Guarantee::Unknown)
        {
            known
                .entry(fact.proposition.clone())
                .or_insert_with(|| fact.clone());
        }
    }
    let mut steps = 0u32;
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
                    derive(&mut known, rule, std::slice::from_ref(&fact), &bindings)?;
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
                    derive(&mut known, rule, &matched_facts, &bindings)?;
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
            guarantee: Guarantee::Derived,
            source_input: premises
                .first()
                .map_or_else(String::new, |fact| fact.source_input.clone()),
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
    dependencies.into_iter().collect()
}

/// A direct user premise stays a premise even when it refutes the asked
/// proposition; rule-backed conclusions remain derived.
pub fn answer_guarantee(answer: &QueryOutcome) -> Guarantee {
    if answer.used_rules.is_empty() {
        Guarantee::UserPremise
    } else {
        Guarantee::Derived
    }
}

/// Preserve originals and append validity changes in the Core's next durable
/// memory revision. No counterexample or rule record is physically deleted.
pub fn invalidate(memory: &mut MemoryState, id: &str) -> Result<Vec<String>> {
    if !memory.facts.iter().any(|fact| fact.id == id)
        && !memory.rules.iter().any(|rule| rule.id == id)
    {
        return Err(failure(
            "UNKNOWN_MEMORY_ID",
            "no fact or rule has the requested identity",
        ));
    }
    let mut invalid = BTreeSet::from([id.to_string()]);
    loop {
        let before = invalid.len();
        for (record_id, dependencies) in memory
            .facts
            .iter()
            .map(|fact| (&fact.id, &fact.dependencies))
            .chain(
                memory
                    .rules
                    .iter()
                    .map(|rule| (&rule.id, &rule.dependencies)),
            )
        {
            if dependencies
                .iter()
                .any(|dependency| invalid.contains(dependency))
            {
                invalid.insert(record_id.clone());
            }
        }
        if invalid.len() == before {
            break;
        }
    }
    for fact in &mut memory.facts {
        if invalid.contains(&fact.id) {
            fact.valid = false;
        }
    }
    for rule in &mut memory.rules {
        if invalid.contains(&rule.id) {
            rule.valid = false;
        }
    }
    bump(memory)?;
    Ok(invalid.into_iter().collect())
}

pub fn format_proposition(proposition: &Proposition) -> String {
    if proposition.predicate == "is" {
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
        }
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
        let answer = query(&memory, &p("osprey", "animal"), 32).unwrap();
        assert_eq!(answer.status, "SUPPORTED");
        assert_eq!(
            answer.steps, 3,
            "each matching relation rule path is charged once"
        );
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
