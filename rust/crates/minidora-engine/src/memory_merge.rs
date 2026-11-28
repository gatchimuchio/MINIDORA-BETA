use crate::relations;
use minidora_domain::{Error, Guarantee, MemoryState, Result, failure};
use std::collections::{BTreeMap, BTreeSet};

const COMPOSED_RULE_SOURCE: &str = "derived:proof-composition-v1";

fn invalid_edit() -> Error {
    failure(
        "MEMORY_CASE_EDIT_INVALID",
        "この会話の記憶と今回の変更を照合できません。",
    )
}

fn scope_conflict() -> Error {
    failure(
        "MEMORY_SCOPE_CONFLICT",
        "この会話の範囲内で記憶の変更を適用できません。",
    )
}

/// Merge one edit prepared from the Store's authenticated case projection.
/// Existing records keep their order in the actor's full, append-only head.
pub(crate) fn merge_case_edit(
    actor: &str,
    raw: &MemoryState,
    baseline: &MemoryState,
    proposed: &MemoryState,
    local_input: &str,
) -> Result<MemoryState> {
    if actor.trim().is_empty() || local_input.is_empty() || local_input == COMPOSED_RULE_SOURCE {
        return Err(invalid_edit());
    }
    if baseline.revision != raw.revision {
        return Err(failure(
            "REVISION_CONFLICT",
            "変更前に記憶が更新されました。",
        ));
    }

    let mut invalidations = BTreeSet::new();
    macro_rules! appended_records {
        ($field:ident) => {{
            let count = baseline.$field.len();
            let retained = proposed.$field.get(..count).ok_or_else(invalid_edit)?;
            let mut original = raw.$field.iter();
            for (before, after) in baseline.$field.iter().zip(retained) {
                if original.find(|record| record.id == before.id) != Some(before)
                    || (!before.valid && after.valid)
                {
                    return Err(invalid_edit());
                }
                let mut expected = before.clone();
                expected.valid = after.valid;
                if &expected != after {
                    return Err(invalid_edit());
                }
                if before.valid && !after.valid {
                    invalidations.insert(before.id.as_str());
                }
            }
            &proposed.$field[count..]
        }};
    }

    let facts = appended_records!(facts);
    let rules = appended_records!(rules);
    let conditions = appended_records!(conditions);
    let changed_records = !invalidations.is_empty()
        || !facts.is_empty()
        || !rules.is_empty()
        || !conditions.is_empty();
    let revision = if changed_records || proposed.revision != raw.revision {
        raw.revision
            .checked_add(1)
            .ok_or_else(|| failure("MEMORY_REVISION_LIMIT", "記憶の版番号が上限に達しました。"))?
    } else {
        raw.revision
    };
    if proposed.revision != revision {
        return Err(invalid_edit());
    }
    if !changed_records {
        let mut merged = raw.clone();
        merged.revision = revision;
        return Ok(merged);
    }
    let raw_activity = raw
        .facts
        .iter()
        .map(|record| (record.id.as_str(), record.valid))
        .chain(
            raw.rules
                .iter()
                .map(|record| (record.id.as_str(), record.valid)),
        )
        .chain(
            raw.conditions
                .iter()
                .map(|record| (record.id.as_str(), record.valid)),
        )
        .collect::<BTreeMap<_, _>>();
    let local_active = proposed
        .facts
        .iter()
        .filter(|record| record.valid)
        .map(|record| record.id.as_str())
        .chain(
            proposed
                .rules
                .iter()
                .filter(|record| record.valid)
                .map(|record| record.id.as_str()),
        )
        .chain(
            proposed
                .conditions
                .iter()
                .filter(|record| record.valid)
                .map(|record| record.id.as_str()),
        )
        .collect::<BTreeSet<_>>();
    let mut record_ids = raw_activity.keys().copied().collect::<BTreeSet<_>>();
    let mut condition_ids = raw
        .conditions
        .iter()
        .map(|record| record.definition.condition_id.as_str())
        .collect::<BTreeSet<_>>();
    let additions = facts
        .iter()
        .map(|record| (record.id.as_str(), record.valid, &record.dependencies))
        .chain(
            rules
                .iter()
                .map(|record| (record.id.as_str(), record.valid, &record.dependencies)),
        )
        .chain(
            conditions
                .iter()
                .map(|record| (record.id.as_str(), record.valid, &record.dependencies)),
        );
    for (id, valid, dependencies) in additions {
        if !valid
            || dependencies
                .iter()
                .any(|dependency| !local_active.contains(dependency.as_str()))
        {
            return Err(invalid_edit());
        }
        if condition_ids.contains(id) || !record_ids.insert(id) {
            return Err(scope_conflict());
        }
    }
    if !conditions.is_empty() {
        let mut local_conditions = MemoryState {
            conditions: proposed.conditions[..baseline.conditions.len()].to_vec(),
            ..MemoryState::default()
        };
        for record in conditions {
            let condition_id = record.definition.condition_id.as_str();
            if record.source_input != local_input {
                return Err(invalid_edit());
            }
            if record_ids.contains(condition_id) || !condition_ids.insert(condition_id) {
                return Err(scope_conflict());
            }
            if relations::define_condition(
                &mut local_conditions,
                record.definition.clone(),
                local_input,
            )? != *record
            {
                return Err(invalid_edit());
            }
        }
    }
    for fact in facts {
        if fact.source_input != local_input
            || fact.guarantee != Guarantee::UserPremise
            || !fact.dependencies.is_empty()
            || fact.id != relations::user_premise_id(&fact.proposition, local_input)?
        {
            return Err(invalid_edit());
        }
    }
    for rule in rules {
        if rule.source_input == COMPOSED_RULE_SOURCE {
            let [left, right] = rule.dependencies.as_slice() else {
                return Err(invalid_edit());
            };
            let local_parent = |id: &str| {
                proposed
                    .rules
                    .iter()
                    .find(|parent| parent.valid && parent.id == id)
            };
            let left = local_parent(left).ok_or_else(invalid_edit)?;
            let right = local_parent(right).ok_or_else(invalid_edit)?;
            if !relations::verified_composed_rule(rule, left, right)? {
                return Err(invalid_edit());
            }
        } else {
            let mut premises = vec![rule.premise.clone()];
            premises.extend_from_slice(&rule.additional_premises);
            if rule.source_input != local_input
                || !rule.dependencies.is_empty()
                || rule.id != relations::declared_rule_id(&premises, &rule.conclusion, local_input)?
            {
                return Err(invalid_edit());
            }
        }
    }

    let mut merged = raw.clone();
    if !invalidations.is_empty() {
        // Probe the actual cascade on the complete dependency graph. The
        // accepted edit publishes one revision regardless of root count.
        merged.revision = 0;
        let affected = relations::invalidate_in_order(&mut merged, invalidations.iter().copied())?;
        if affected.iter().any(|id| {
            raw_activity.get(id.as_str()) == Some(&true) && !invalidations.contains(id.as_str())
        }) {
            return Err(scope_conflict());
        }
    }
    merged.facts.extend_from_slice(facts);
    merged.rules.extend_from_slice(rules);
    merged.conditions.extend_from_slice(conditions);
    merged.revision = revision;
    Ok(merged)
}

#[cfg(test)]
mod tests {
    use super::*;
    use minidora_domain::Proposition;

    fn pattern(object: &str) -> Proposition {
        Proposition {
            subject: "$x".into(),
            predicate: "is".into(),
            object: object.into(),
            negative: false,
            scope: "実験室".into(),
            condition_refs: BTreeSet::new(),
            temporal_scope: None,
        }
    }

    #[test]
    fn local_invalidation_cannot_change_a_foreign_rule_proof() {
        let mut raw = MemoryState::default();
        let local = relations::add_rule(&mut raw, pattern("甲"), pattern("乙"), "case-a").unwrap();
        let foreign =
            relations::add_rule(&mut raw, pattern("乙"), pattern("丙"), "case-b").unwrap();
        assert_eq!(raw.rules.len(), 3);
        assert!(raw.rules[2].dependencies.contains(&local.id));
        assert!(raw.rules[2].dependencies.contains(&foreign.id));

        let baseline = MemoryState {
            revision: raw.revision,
            rules: vec![local.clone()],
            ..MemoryState::default()
        };
        let mut proposed = baseline.clone();
        relations::invalidate(&mut proposed, &local.id).unwrap();
        assert!(matches!(
            merge_case_edit("owner", &raw, &baseline, &proposed, "case-a-edit"),
            Err(Error::Contract { code, .. }) if code == "MEMORY_SCOPE_CONFLICT"
        ));

        // The foreign proof may remain in storage once it is already invalid.
        raw.rules[2].valid = false;
        let merged = merge_case_edit("owner", &raw, &baseline, &proposed, "case-a-edit").unwrap();
        assert_eq!(merged.revision, raw.revision + 1);
        assert!(!merged.rules[0].valid);
        assert_eq!(&merged.rules[1..], &raw.rules[1..]);
    }
}
