//! Scoped proof-preserving relations. Source premises are never world facts.
//! This version supports one-premise rules; unrepresented conjunctions and
//! quantifiers must be refused by the input grammar rather than dropped.
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
    validate(&premise)?;
    validate(&conclusion)?;
    if premise.scope != conclusion.scope {
        return Err(failure(
            "SCOPE_MISMATCH",
            "a rule may not silently change scope",
        ));
    }
    if !variables(&conclusion).is_subset(&variables(&premise)) {
        return Err(failure(
            "UNBOUND_VARIABLE",
            "conclusion contains a variable not bound by its premise",
        ));
    }
    if premise == conclusion {
        return Err(failure("CYCLIC_RULE", "a self-rule adds no consequence"));
    }
    if source_input.is_empty() {
        return Err(failure(
            "MISSING_PROVENANCE",
            "a rule needs original input provenance",
        ));
    }
    let id = format!(
        "rule:{}",
        identity(&("declared-rule-v1", &premise, &conclusion, source_input))?
    );
    if let Some(old) = memory.rules.iter().find(|rule| rule.id == id) {
        return Ok(old.clone());
    }
    let rule = RelationRule {
        id,
        premise,
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
            if let Some(composed) = compose(left, right)?
                && !memory.rules.iter().any(|old| {
                    old.premise == composed.premise
                        && old.conclusion == composed.conclusion
                        && old.valid
                })
            {
                additions.insert(composed.id.clone(), composed);
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
fn canonical_variables(premise: &mut Proposition, conclusion: &mut Proposition) {
    let mut names = BTreeMap::new();
    for term in [
        &mut premise.subject,
        &mut premise.object,
        &mut conclusion.subject,
        &mut conclusion.object,
    ] {
        if variable(term) {
            let next = format!("$v{}", names.len());
            *term = names.entry(term.clone()).or_insert(next).clone();
        }
    }
}
fn compose(left: &RelationRule, right: &RelationRule) -> Result<Option<RelationRule>> {
    let left_premise = renamed(&left.premise, "left");
    let left_conclusion = renamed(&left.conclusion, "left");
    let right_premise = renamed(&right.premise, "right");
    let right_conclusion = renamed(&right.conclusion, "right");
    if !same_shape(&left_conclusion, &right_premise) {
        return Ok(None);
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
        return Ok(None);
    }
    let mut premise = substitute(&left_premise, &bindings);
    let mut conclusion = substitute(&right_conclusion, &bindings);
    canonical_variables(&mut premise, &mut conclusion);
    if premise == conclusion || !variables(&conclusion).is_subset(&variables(&premise)) {
        return Ok(None);
    }
    let dependencies = vec![left.id.clone(), right.id.clone()];
    let id = format!(
        "rule:{}",
        identity(&("proof-composition-v1", &premise, &conclusion, &dependencies))?
    );
    Ok(Some(RelationRule {
        id,
        premise,
        conclusion,
        source_input: "derived:proof-composition-v1".into(),
        dependencies,
        valid: true,
    }))
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
    if !same_shape(pattern, actual) {
        return None;
    }
    let mut bindings = BTreeMap::new();
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
        .filter(|rule| permitted_rule(rule, &rule_map, &mut BTreeSet::new()))
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
    let mut applied = BTreeSet::new();
    loop {
        let before = known.len();
        let current: Vec<_> = known.values().cloned().collect();
        for fact in current {
            // A contradicted premise cannot support an unqualified consequence.
            if known.contains_key(&opposition(&fact.proposition)) {
                continue;
            }
            for rule in &rules {
                let Some(bindings) = matches(&rule.premise, &fact.proposition) else {
                    continue;
                };
                if !applied.insert((rule.id.clone(), fact.id.clone())) {
                    continue;
                }
                steps = steps
                    .checked_add(1)
                    .ok_or_else(|| failure("RELATION_BUDGET", "relation step overflow"))?;
                if steps > max_steps {
                    return Err(failure(
                        "RELATION_BUDGET",
                        "proof expansion exceeded the task relation budget",
                    ));
                }
                let conclusion = substitute(&rule.conclusion, &bindings);
                if !variables(&conclusion).is_empty() {
                    return Err(failure(
                        "UNBOUND_VARIABLE",
                        "derived conclusion remains unbound",
                    ));
                }
                if known.contains_key(&conclusion) {
                    continue;
                }
                let mut dependencies: BTreeSet<_> = fact.dependencies.iter().cloned().collect();
                dependencies.insert(fact.id.clone());
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
                        source_input: fact.source_input.clone(),
                        dependencies,
                        valid: true,
                    },
                );
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
