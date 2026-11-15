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
            "minidora-core-scope-polarity-54-{}-{serial}",
            std::process::id()
        ));
        fs::create_dir(&path).expect("create isolated scope-polarity state");
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

fn assert_missing_before_content(result: &minidora_domain::ResultRecord) {
    assert_eq!(result.lifecycle, Lifecycle::Suspend, "{result:?}");
    assert!(
        result
            .reason
            .as_deref()
            .is_some_and(|reason| { reason.starts_with("MISSING_PREMISES:") })
    );
    assert!(result.content_plan.is_none());
    assert!(result.render.is_none());
}

#[test]
fn world_scope_and_negation_remain_separate_through_proof_and_correction() {
    let directory = TestDirectory::new();
    let actor = "b54-scope-polarity";
    let case = "b54-scope-polarity-case";
    let lab_scope = "b54-lab-world";
    let archive_scope = "b54-archive-world";
    let mut engine = Engine::open(directory.path()).expect("open isolated Core state");

    let cross_scope_rule = engine
        .add_atomic_relation_rule(
            actor,
            request(case, "cross-scope-rule", actor),
            vec![relation("$x", "ore", lab_scope, false)],
            relation("$x", "refined ore", archive_scope, false),
        )
        .expect("cross-scope rule gets a typed Core stop");
    assert_eq!(
        cross_scope_rule.lifecycle,
        Lifecycle::Suspend,
        "{cross_scope_rule:?}"
    );
    assert!(cross_scope_rule.text.contains("SCOPE_MISMATCH"));
    assert!(cross_scope_rule.content_plan.is_none());
    assert!(cross_scope_rule.render.is_none());

    let rule = engine
        .add_atomic_relation_rule(
            actor,
            request(case, "lab-refinement-rule", actor),
            vec![relation("$x", "ore", lab_scope, false)],
            relation("$x", "refined ore", lab_scope, false),
        )
        .expect("persist same-scope positive rule");
    assert_eq!(rule.lifecycle, Lifecycle::Commit, "{rule:?}");
    assert_eq!(rule.guarantee, Guarantee::UserPremise);
    let rule_id = rule.dependencies[0].clone();

    let original_premise = relation("Nera", "ore", lab_scope, false);
    let premise = engine
        .assert_atomic_relation(
            actor,
            request(case, "nera-positive-ore", actor),
            original_premise.clone(),
        )
        .expect("persist positive premise in the lab scope");
    assert_eq!(premise.lifecycle, Lifecycle::Commit, "{premise:?}");
    let premise_id = premise.dependencies[0].clone();

    let negative_premise = relation("Mira", "ore", lab_scope, true);
    let negative_fact = engine
        .assert_atomic_relation(
            actor,
            request(case, "mira-negative-ore", actor),
            negative_premise.clone(),
        )
        .expect("persist a distinct negative premise");
    assert_eq!(
        negative_fact.lifecycle,
        Lifecycle::Commit,
        "{negative_fact:?}"
    );
    let negative_fact_id = negative_fact.dependencies[0].clone();

    let same_scope = engine
        .compose_atomic_explanation(
            actor,
            request(case, "nera-refined-in-lab", actor),
            relation("Nera", "refined ore", lab_scope, false),
        )
        .expect("derive the exact same-scope positive conclusion");
    assert_eq!(same_scope.lifecycle, Lifecycle::Commit, "{same_scope:?}");
    assert_eq!(same_scope.guarantee, Guarantee::Derived);
    assert!(same_scope.dependencies.contains(&rule_id));
    assert!(same_scope.dependencies.contains(&premise_id));
    assert!(same_scope.text.contains(lab_scope));
    let same_scope_plan = same_scope
        .content_plan
        .as_ref()
        .expect("Core adopted the same-scope derived content");
    assert_eq!(same_scope_plan.guarantee, Guarantee::Derived);
    assert!(same_scope_plan.dependencies.contains(&rule_id));
    assert!(same_scope_plan.dependencies.contains(&premise_id));
    let same_scope_render = same_scope
        .render
        .as_ref()
        .expect("K7 rendered the same-scope derived content");
    assert_eq!(same_scope_render.status, "RENDERED");
    assert_eq!(same_scope_render.bytes, same_scope_plan.text.as_bytes());

    let other_scope = engine
        .compose_atomic_explanation(
            actor,
            request(case, "nera-refined-in-archive", actor),
            relation("Nera", "refined ore", archive_scope, false),
        )
        .expect("scope mismatch follows the bounded stop path");
    assert_missing_before_content(&other_scope);

    let negative_does_not_fire_positive_rule = engine
        .compose_atomic_explanation(
            actor,
            request(case, "mira-refined-from-negative-ore", actor),
            relation("Mira", "refined ore", lab_scope, false),
        )
        .expect("negative premise cannot satisfy the positive rule premise");
    assert_missing_before_content(&negative_does_not_fire_positive_rule);

    let positive_question = engine
        .compose_atomic_explanation(
            actor,
            request(case, "mira-positive-ore-question", actor),
            relation("Mira", "ore", lab_scope, false),
        )
        .expect("positive question preserves the contrary negative proof");
    assert_eq!(
        positive_question.lifecycle,
        Lifecycle::Commit,
        "{positive_question:?}"
    );
    assert!(positive_question.dependencies.contains(&negative_fact_id));
    assert!(positive_question.text.contains("ではない"));
    let negative_plan = positive_question
        .content_plan
        .as_ref()
        .expect("Core adopted the contradiction explanation");
    assert!(negative_plan.dependencies.contains(&negative_fact_id));
    assert!(negative_plan.text.contains("ではない"));
    let negative_render = positive_question
        .render
        .as_ref()
        .expect("K7 rendered the contradiction explanation");
    assert_eq!(negative_render.status, "RENDERED");
    assert_eq!(negative_render.bytes, negative_plan.text.as_bytes());

    let corrected = engine
        .correct_atomic_relation(
            actor,
            request(case, "move-nera-ore-to-archive", actor),
            original_premise,
            relation("Nera", "ore", archive_scope, false),
        )
        .expect("move only Nera's premise to the new world scope");
    assert_eq!(corrected.lifecycle, Lifecycle::Commit, "{corrected:?}");
    assert_eq!(corrected.guarantee, Guarantee::UserPremise);

    let old_proof_after_correction = engine
        .compose_atomic_explanation(
            actor,
            request(case, "old-lab-proof-after-correction", actor),
            relation("Nera", "refined ore", lab_scope, false),
        )
        .expect("recheck the invalidated lab conclusion");
    assert_missing_before_content(&old_proof_after_correction);

    let replacement_scope_has_no_rule = engine
        .compose_atomic_explanation(
            actor,
            request(case, "archive-proof-without-rule", actor),
            relation("Nera", "refined ore", archive_scope, false),
        )
        .expect("replacement scope cannot inherit the lab rule");
    assert_missing_before_content(&replacement_scope_has_no_rule);

    let replacement_fact = engine
        .compose_atomic_explanation(
            actor,
            request(case, "archive-ore-premise", actor),
            relation("Nera", "ore", archive_scope, false),
        )
        .expect("replacement premise is available only in its new scope");
    assert_eq!(
        replacement_fact.lifecycle,
        Lifecycle::Commit,
        "{replacement_fact:?}"
    );
    assert_eq!(replacement_fact.guarantee, Guarantee::UserPremise);

    let store = Store::open(directory.path()).expect("inspect persisted scope and polarity state");
    let memory = store.load_memory(actor).expect("load actor memory");
    assert!(memory.facts.iter().any(|fact| {
        fact.id == premise_id && !fact.valid && fact.proposition.scope == lab_scope
    }));
    assert!(memory.facts.iter().any(|fact| {
        fact.valid
            && fact.proposition.subject == "Nera"
            && fact.proposition.object == "ore"
            && fact.proposition.scope == archive_scope
            && !fact.proposition.negative
    }));
    assert!(memory.facts.iter().any(|fact| {
        fact.id == negative_fact_id
            && fact.valid
            && fact.proposition.subject == "Mira"
            && fact.proposition.scope == lab_scope
            && fact.proposition.negative
    }));
    assert!(memory.rules.iter().any(|stored_rule| {
        stored_rule.id == rule_id
            && stored_rule.valid
            && stored_rule.premise.scope == lab_scope
            && stored_rule.conclusion.scope == lab_scope
    }));

    println!(
        "B54_SCOPE_POLARITY_OBSERVATION={}",
        serde_json::to_string(&json!({
            "work_item": "B54 exact world scope and negation through Core relation proof and correction",
            "actor": actor,
            "scopes": [lab_scope, archive_scope],
            "rule_id": rule_id,
            "premise_id": premise_id,
            "negative_fact_id": negative_fact_id,
            "same_scope_positive_derivation": {
                "lifecycle": same_scope.lifecycle,
                "guarantee": same_scope.guarantee,
                "dependencies": same_scope.dependencies,
                "text": same_scope.text,
                "content_plan_id": same_scope_plan.id,
                "render_status": same_scope_render.status,
                "rendered_bytes_match_plan": same_scope_render.bytes == same_scope_plan.text.as_bytes(),
            },
            "negative_polarity": {
                "positive_rule_from_negative_premise": negative_does_not_fire_positive_rule.lifecycle,
                "no_content_plan": negative_does_not_fire_positive_rule.content_plan.is_none(),
                "no_render": negative_does_not_fire_positive_rule.render.is_none(),
                "positive_question_against_negative_fact": positive_question.lifecycle,
                "negative_fact_dependency_retained": positive_question.dependencies.contains(&negative_fact_id),
                "contrary_wording_present": positive_question.text.contains("ではない"),
                "contrary_render_status": negative_render.status,
                "contrary_rendered_bytes_match_plan": negative_render.bytes == negative_plan.text.as_bytes(),
            },
            "scope_boundary": {
                "cross_scope_rule": cross_scope_rule.lifecycle,
                "cross_scope_rule_error": cross_scope_rule.text,
                "archive_query_without_rule": other_scope.lifecycle,
                "archive_rule_not_inherited_after_correction": replacement_scope_has_no_rule.lifecycle,
            },
            "correction": {
                "lifecycle": corrected.lifecycle,
                "old_lab_conclusion_after_correction": old_proof_after_correction.lifecycle,
                "old_conclusion_reason": old_proof_after_correction.reason,
                "replacement_scope_fact": replacement_fact.lifecycle,
                "unrelated_negative_fact_remains_valid": memory.facts.iter().any(|fact| fact.id == negative_fact_id && fact.valid),
            },
            "gate_assessment": {
                "CORE-G01": "PARTIAL; typed world scope and polarity remain distinct through inference and correction; condition truth, natural-language conditions, wider scope families, time and joint novel-subject acceptance are not closed by this fixture",
                "CORE-G03": "PARTIAL; this typed relation fixture does not establish the general new-content acceptance predicate",
            },
            "limits": ["direct typed Core API", "one rule, two scopes and two subjects", "no natural-language parser", "no general condition evaluator"]
        }))
        .expect("serialize B54 runtime observation")
    );
}
