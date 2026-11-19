use minidora_domain::{Guarantee, Lifecycle, Proposition, RequestEnvelope};
use minidora_engine::{Engine, store::Store};
use serde_json::json;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_TEMP_DIRECTORY: AtomicU64 = AtomicU64::new(0);

struct TestDirectory(PathBuf);

impl TestDirectory {
    fn new() -> Self {
        let serial = NEXT_TEMP_DIRECTORY.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "minidora-core-conjunctive-proof-55-{}-{serial}",
            std::process::id()
        ));
        fs::create_dir(&path).expect("create isolated conjunctive-proof state");
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

fn relation(subject: &str, object: &str, scope: &str, negative: bool) -> Proposition {
    Proposition {
        subject: subject.into(),
        predicate: "is".into(),
        object: object.into(),
        negative,
        scope: scope.into(),
        temporal_scope: None,
        condition_refs: Default::default(),
    }
}

fn request(case: &str, input: &str, actor: &str) -> RequestEnvelope {
    RequestEnvelope::new(case, input, actor, "")
}

fn assert_stop_before_content(result: &minidora_domain::ResultRecord, code: &str) {
    assert_eq!(result.lifecycle, Lifecycle::Suspend, "{result:?}");
    assert!(
        result
            .reason
            .as_deref()
            .is_some_and(|reason| reason.starts_with(&format!("{code}:"))),
        "expected {code} stop, got {:?}",
        result.reason
    );
    assert!(result.content_plan.is_none());
    assert!(result.render.is_none());
}

#[test]
fn two_premises_derive_only_while_both_are_supported_and_uncontested() {
    let directory = TestDirectory::new();
    let actor = "b55-conjunctive-proof";
    let case = "b55-conjunctive-proof-case";
    let scope = "b55-calibration-world";
    let subject = "Vela";
    let first = relation("$x", "calibrated", scope, false);
    let second = relation("$x", "power stable", scope, false);
    let conclusion = relation("$x", "safe", scope, false);
    let mut engine = Engine::open(directory.path()).expect("open isolated Core state");

    let rule = engine
        .add_atomic_relation_rule(
            actor,
            request(case, "two-premise-safe-rule", actor),
            vec![first.clone(), second.clone()],
            conclusion.clone(),
        )
        .expect("save the two-premise rule through Core");
    assert_eq!(rule.lifecycle, Lifecycle::Commit, "{rule:?}");
    assert_eq!(rule.guarantee, Guarantee::UserPremise);
    let rule_id = rule.dependencies[0].clone();

    let first_fact = engine
        .assert_atomic_relation(
            actor,
            request(case, "vela-calibrated", actor),
            relation(subject, "calibrated", scope, false),
        )
        .expect("save the first new premise through Core");
    assert_eq!(first_fact.lifecycle, Lifecycle::Commit, "{first_fact:?}");
    let first_fact_id = first_fact.dependencies[0].clone();

    let missing_second_premise = engine
        .compose_atomic_explanation(
            actor,
            request(case, "safe-before-power-stability", actor),
            relation(subject, "safe", scope, false),
        )
        .expect("missing conjunctive premise takes the bounded Core stop path");
    assert_stop_before_content(&missing_second_premise, "MISSING_PREMISES");

    let second_fact = engine
        .assert_atomic_relation(
            actor,
            request(case, "vela-power-stable", actor),
            relation(subject, "power stable", scope, false),
        )
        .expect("save the second new premise through Core");
    assert_eq!(second_fact.lifecycle, Lifecycle::Commit, "{second_fact:?}");
    let second_fact_id = second_fact.dependencies[0].clone();

    let derived = engine
        .compose_atomic_explanation(
            actor,
            request(case, "safe-with-both-premises", actor),
            relation(subject, "safe", scope, false),
        )
        .expect("combine both exact premises through the Core proof consumer");
    assert_eq!(derived.lifecycle, Lifecycle::Commit, "{derived:?}");
    assert_eq!(derived.guarantee, Guarantee::Derived);
    for dependency in [&first_fact_id, &second_fact_id, &rule_id] {
        assert!(
            derived.dependencies.contains(dependency),
            "derived result omitted proof dependency {dependency}"
        );
    }
    let plan = derived
        .content_plan
        .as_ref()
        .expect("Core adopted the conjunctive proof content");
    for dependency in [&first_fact_id, &second_fact_id, &rule_id] {
        assert!(plan.dependencies.contains(dependency));
    }
    let render = derived
        .render
        .as_ref()
        .expect("K7 rendered the adopted conjunctive proof");
    assert_eq!(render.status, "RENDERED");
    assert_eq!(render.bytes, plan.text.as_bytes());

    let unrelated = engine
        .assert_atomic_relation(
            actor,
            request(case, "other-subject-calibrated", actor),
            relation("Rook", "calibrated", scope, false),
        )
        .unwrap();
    assert_eq!(unrelated.lifecycle, Lifecycle::Commit);
    let same_proof = engine
        .compose_atomic_explanation(
            actor,
            request(case, "safe-after-unrelated-fact", actor),
            relation(subject, "safe", scope, false),
        )
        .unwrap();
    assert_eq!(same_proof.lifecycle, derived.lifecycle);
    assert_eq!(same_proof.guarantee, derived.guarantee);
    assert_eq!(same_proof.dependencies, derived.dependencies);
    assert_eq!(same_proof.content_plan, derived.content_plan);
    assert_eq!(same_proof.render, derived.render);
    assert!(!same_proof.dependencies.contains(&unrelated.dependencies[0]));

    let contrary_second_fact = engine
        .assert_atomic_relation(
            actor,
            request(case, "vela-not-power-stable", actor),
            relation(subject, "power stable", scope, true),
        )
        .expect("retain the direct counterevidence as a separate premise");
    assert_eq!(
        contrary_second_fact.lifecycle,
        Lifecycle::Commit,
        "{contrary_second_fact:?}"
    );
    let contrary_fact_id = contrary_second_fact.dependencies[0].clone();

    let derived_after_contradiction = engine
        .compose_atomic_explanation(
            actor,
            request(case, "safe-after-premise-contradiction", actor),
            relation(subject, "safe", scope, false),
        )
        .expect("the contested premise no longer supports the conclusion");
    assert_stop_before_content(&derived_after_contradiction, "MISSING_PREMISES");

    let contested_question = engine
        .compose_atomic_explanation(
            actor,
            request(case, "ask-contested-power-stability", actor),
            relation(subject, "power stable", scope, false),
        )
        .expect("the direct conflicting premise pair takes the Core conflict stop");
    assert_stop_before_content(&contested_question, "CONFLICTING_PREMISES");

    let store = Store::open(directory.path()).expect("inspect persisted Core relation state");
    let memory = store
        .load_memory(actor)
        .expect("load actor relation memory");
    assert!(memory.rules.iter().any(|stored| {
        stored.id == rule_id
            && stored.valid
            && stored.additional_premises == vec![second.clone()]
            && stored.conclusion == conclusion
    }));
    for (id, negative, expected_valid) in [
        (&first_fact_id, false, true),
        (&second_fact_id, false, true),
        (&contrary_fact_id, true, true),
    ] {
        assert!(memory.facts.iter().any(|fact| {
            fact.id == *id
                && fact.valid == expected_valid
                && fact.proposition.negative == negative
                && fact.proposition.subject == subject
        }));
    }

    println!(
        "B55_CONJUNCTIVE_PROOF_OBSERVATION={}",
        serde_json::to_string(&json!({
            "work_item": "B55 two-premise Core derivation and contradiction boundary",
            "actor": actor,
            "case": case,
            "scope": scope,
            "rule_id": rule_id,
            "first_fact_id": first_fact_id,
            "second_fact_id": second_fact_id,
            "contrary_fact_id": contrary_fact_id,
            "before_second_premise": {
                "lifecycle": missing_second_premise.lifecycle,
                "reason": missing_second_premise.reason,
                "content_plan": missing_second_premise.content_plan.is_some(),
                "render": missing_second_premise.render.is_some(),
            },
            "with_both_premises": {
                "lifecycle": derived.lifecycle,
                "guarantee": derived.guarantee,
                "dependencies": derived.dependencies,
                "plan_dependencies": plan.dependencies,
                "render_status": render.status,
                "rendered_bytes_match_plan": render.bytes == plan.text.as_bytes(),
            },
            "after_contradiction": {
                "derived_query_lifecycle": derived_after_contradiction.lifecycle,
                "derived_query_reason": derived_after_contradiction.reason,
                "derived_query_has_plan_or_render": derived_after_contradiction.content_plan.is_some() || derived_after_contradiction.render.is_some(),
                "contested_premise_lifecycle": contested_question.lifecycle,
                "contested_premise_reason": contested_question.reason,
                "contested_query_has_plan_or_render": contested_question.content_plan.is_some() || contested_question.render.is_some(),
            },
            "memory": {
                "rule_remains_valid": memory.rules.iter().any(|stored| stored.id == rule_id && stored.valid),
                "both_direct_roots_remain_valid": memory.facts.iter().any(|fact| fact.id == first_fact_id && fact.valid)
                    && memory.facts.iter().any(|fact| fact.id == second_fact_id && fact.valid),
                "contrary_direct_root_remains_valid": memory.facts.iter().any(|fact| fact.id == contrary_fact_id && fact.valid),
            },
            "gate_assessment": {
                "CORE-G02": "PARTIAL: a missing conjunctive premise suspends before content; two positive premises produce a dependency-complete derived proof and matching K7 bytes; a direct contradiction blocks reuse and is reported as CONFLICTING_PREMISES; counterfactual routes and broader premise graphs remain unverified",
                "CORE-G01": "PARTIAL; not reassessed in B55",
                "CORE-G03": "PARTIAL; not reassessed in B55",
                "CORE-G04": "NOT_REASSESSED",
                "CORE-G05": "NOT_REASSESSED",
                "CORE-G06": "NOT_REASSESSED",
                "CORE-G07": "NOT_REASSESSED"
            },
            "limits": ["direct typed Engine API", "one two-premise rule", "one subject and scope", "no counterfactual route in this fixture", "does not establish full CORE-G02 predicate"]
        }))
        .expect("serialize B55 runtime observation")
    );
}
