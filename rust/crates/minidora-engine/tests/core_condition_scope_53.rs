use minidora_domain::{
    ConditionDefinition, ConditionExpression, Guarantee, Lifecycle, Proposition, RequestEnvelope,
    SCHEMA_VERSION,
};
use minidora_engine::{Engine, store::Store};
use serde_json::json;
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_TEMP_DIRECTORY: AtomicU64 = AtomicU64::new(0);

struct TestDirectory(PathBuf);

impl TestDirectory {
    fn new() -> Self {
        let serial = NEXT_TEMP_DIRECTORY.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "minidora-core-condition-scope-53-{}-{serial}",
            std::process::id()
        ));
        fs::create_dir(&path).expect("create isolated condition-scope state");
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TestDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn relation(subject: &str, object: &str, scope: &str, conditions: &[&str]) -> Proposition {
    Proposition {
        subject: subject.into(),
        predicate: "is".into(),
        object: object.into(),
        negative: false,
        scope: scope.into(),
        temporal_scope: None,
        condition_refs: conditions.iter().map(|value| (*value).to_owned()).collect(),
    }
}

fn request(case: &str, input: &str, actor: &str) -> RequestEnvelope {
    RequestEnvelope::new(case, input, actor, "")
}

fn condition(condition_id: &str, object: &str, scope: &str) -> ConditionDefinition {
    ConditionDefinition {
        condition_id: condition_id.into(),
        subject: "$subject".into(),
        scope: scope.into(),
        expression: ConditionExpression::Fact {
            predicate: "is".into(),
            object: object.into(),
        },
        valid_from: None,
        valid_until: None,
        dependencies: BTreeSet::new(),
    }
}

#[test]
fn typed_condition_refs_bind_rules_queries_correction_and_comparison() {
    let directory = TestDirectory::new();
    let actor = "b53-condition-scope";
    let case = "b53-condition-case";
    let scope = "b53-materials-lab";
    let dry = "condition:dry";
    let wet = "condition:wet";
    let mut engine = Engine::open(directory.path()).expect("open isolated Core state");

    for (input, id, object) in [
        ("define-dry-condition", dry, "dry"),
        ("define-wet-condition", wet, "wet"),
    ] {
        let defined = engine
            .define_condition(
                actor,
                request(case, input, actor),
                condition(id, object, scope),
            )
            .expect("define condition through the same Core writer");
        assert_eq!(defined.lifecycle, Lifecycle::Commit, "{defined:?}");
    }

    for (input, subject, object) in [
        ("nera-dry-condition-evidence", "Nera", "dry"),
        ("nera-wet-condition-evidence", "Nera", "wet"),
        ("aster-dry-condition-evidence", "Aster", "dry"),
        ("beryl-wet-condition-evidence", "Beryl", "wet"),
    ] {
        let evidence = engine
            .assert_atomic_relation(
                actor,
                request(case, input, actor),
                relation(subject, object, scope, &[]),
            )
            .expect("save unconditioned Fact evidence through Core");
        assert_eq!(evidence.lifecycle, Lifecycle::Commit, "{evidence:?}");
    }

    let mismatch = engine.add_atomic_relation_rule(
        actor,
        request(case, "mismatched-condition-rule", actor),
        vec![relation("$x", "ore", scope, &[dry])],
        relation("$x", "refined ore", scope, &[wet]),
    );
    let mismatch = mismatch.expect("mismatched rule returns a typed Core stop");
    assert_eq!(mismatch.lifecycle, Lifecycle::Suspend, "{mismatch:?}");
    assert!(mismatch.text.contains("CONDITION_SCOPE_MISMATCH"));
    assert!(mismatch.content_plan.is_none());
    assert!(mismatch.render.is_none());

    let rule = engine
        .add_atomic_relation_rule(
            actor,
            request(case, "dry-ore-rule", actor),
            vec![relation("$x", "ore", scope, &[dry])],
            relation("$x", "refined ore", scope, &[dry]),
        )
        .expect("persist exact-condition rule through Core");
    assert_eq!(rule.lifecycle, Lifecycle::Commit, "{rule:?}");
    assert_eq!(rule.guarantee, Guarantee::UserPremise);
    let rule_id = rule.dependencies[0].clone();

    let original_premise = relation("Nera", "ore", scope, &[dry]);
    let premise = engine
        .assert_atomic_relation(
            actor,
            request(case, "dry-ore-premise", actor),
            original_premise.clone(),
        )
        .expect("persist exact-condition premise through Core");
    assert_eq!(premise.lifecycle, Lifecycle::Commit, "{premise:?}");
    let premise_id = premise.dependencies[0].clone();

    let target = relation("Nera", "refined ore", scope, &[dry]);
    let derived = engine
        .compose_atomic_explanation(actor, request(case, "dry-proof", actor), target.clone())
        .expect("resolve exact-condition proof through Core");
    assert_eq!(derived.lifecycle, Lifecycle::Commit, "{derived:?}");
    assert_eq!(derived.guarantee, Guarantee::Derived);
    assert!(derived.dependencies.contains(&rule_id));
    assert!(derived.dependencies.contains(&premise_id));
    assert!(derived.text.contains(dry));

    for (input, conditions) in [
        ("missing-condition", &[][..]),
        ("different-condition", &[wet][..]),
    ] {
        let stopped = engine
            .compose_atomic_explanation(
                actor,
                request(case, input, actor),
                relation("Nera", "refined ore", scope, conditions),
            )
            .expect("condition mismatch takes the normal bounded stop path");
        assert_eq!(stopped.lifecycle, Lifecycle::Suspend, "{stopped:?}");
        assert!(
            stopped
                .reason
                .as_deref()
                .is_some_and(|reason| { reason.starts_with("MISSING_PREMISES:") })
        );
        assert!(stopped.content_plan.is_none());
        assert!(stopped.render.is_none());
    }

    let corrected = engine
        .correct_atomic_relation(
            actor,
            request(case, "correct-condition-reference", actor),
            original_premise,
            relation("Nera", "ore", scope, &[wet]),
        )
        .expect("correct only the exact conditioned premise");
    assert_eq!(corrected.lifecycle, Lifecycle::Commit, "{corrected:?}");
    assert_eq!(corrected.guarantee, Guarantee::UserPremise);

    let after_correction = engine
        .compose_atomic_explanation(
            actor,
            request(case, "dry-proof-after-correction", actor),
            target,
        )
        .expect("recheck the old exact-condition conclusion");
    assert_eq!(
        after_correction.lifecycle,
        Lifecycle::Suspend,
        "{after_correction:?}"
    );
    assert!(
        after_correction
            .reason
            .as_deref()
            .is_some_and(|reason| { reason.starts_with("MISSING_PREMISES:") })
    );
    assert!(after_correction.content_plan.is_none());
    assert!(after_correction.render.is_none());

    let aster = engine
        .assert_atomic_relation(
            actor,
            request("b53-comparison", "aster-dry", actor),
            relation("Aster", "rare ore", scope, &[dry]),
        )
        .expect("save Aster relation with dry reference");
    let beryl = engine
        .assert_atomic_relation(
            actor,
            request("b53-comparison", "beryl-wet", actor),
            relation("Beryl", "rare ore", scope, &[wet]),
        )
        .expect("save Beryl relation with wet reference");
    assert_eq!(aster.lifecycle, Lifecycle::Commit, "{aster:?}");
    assert_eq!(beryl.lifecycle, Lifecycle::Commit, "{beryl:?}");

    let mut comparison_request = request("b53-condition-comparison", "compare", actor);
    comparison_request.budget.max_actions = 128;
    let comparison = engine
        .compose_atomic_relation_comparison(actor, comparison_request, "Aster", "Beryl", scope)
        .expect("compare facts with distinct exact-condition identities");
    assert_eq!(comparison.lifecycle, Lifecycle::Commit, "{comparison:?}");
    assert!(comparison.text.contains(dry));
    assert!(comparison.text.contains(wet));
    assert!(comparison.text.contains("では同じ関係の支持を確認できず"));
    assert!(!comparison.text.contains("両側で確認した同じ関係"));

    let store = Store::open(directory.path()).expect("inspect persisted conditioned relations");
    let memory = store.load_memory(actor).expect("load actor memory");
    assert!(memory.facts.iter().any(|fact| {
        fact.id == premise_id && !fact.valid && fact.proposition.condition_refs.contains(dry)
    }));
    assert!(memory.facts.iter().any(|fact| {
        fact.valid
            && fact.proposition.subject == "Nera"
            && fact.proposition.condition_refs == BTreeSet::from([wet.to_owned()])
    }));
    assert!(memory.rules.iter().any(|stored_rule| {
        stored_rule.id == rule_id
            && stored_rule.premise.condition_refs == BTreeSet::from([dry.to_owned()])
            && stored_rule.conclusion.condition_refs == BTreeSet::from([dry.to_owned()])
    }));
    assert_eq!(SCHEMA_VERSION, "minidora-rust/0.7.0-alpha.11");

    println!(
        "W04_CONDITION_SCOPE_OBSERVATION={}",
        serde_json::to_string(&json!({
            "work_item": "E1-W04 exact condition references with alpha11 Core evaluation",
            "actor": actor,
            "scope": scope,
            "condition_refs": [dry, wet],
            "rule_id": rule_id,
            "premise_id": premise_id,
            "same_condition": {
                "lifecycle": derived.lifecycle,
                "guarantee": derived.guarantee,
                "dependencies": derived.dependencies,
                "text": derived.text,
            },
            "mismatches": ["missing condition reference", "different condition reference"],
            "correction": {
                "lifecycle": corrected.lifecycle,
                "old_proof_after_correction": after_correction.lifecycle,
                "reason": after_correction.reason,
                "content_plan": after_correction.content_plan.is_some(),
                "render": after_correction.render.is_some(),
            },
            "comparison": {
                "lifecycle": comparison.lifecycle,
                "common_label_present": comparison.text.contains("両側で確認した同じ関係"),
                "condition_labels_present": comparison.text.contains(dry) && comparison.text.contains(wet),
            },
            "gate_assessment": {
                "CORE-G01": "PARTIAL; this bounded typed condition fixture does not cover the gate's complete success predicates",
                "CORE-G03": "PARTIAL; this typed route does not establish the general new-content acceptance path",
            },
            "limits": ["direct typed Engine API", "exact subject/scope/time matching", "four-valued condition expression subset", "no natural-language condition parser"]
        }))
        .expect("serialize W04 runtime observation")
    );
}
