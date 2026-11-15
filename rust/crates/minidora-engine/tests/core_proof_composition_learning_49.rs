use minidora_domain::{Guarantee, Lifecycle, Proposition, RequestEnvelope, ResultRecord};
use minidora_engine::{Engine, relations, store::Store};
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
            "minidora-proof-composition-learning-49-{}-{serial}",
            std::process::id()
        ));
        fs::create_dir(&path).expect("create isolated proof-composition state");
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

fn execute(
    engine: &mut Engine,
    actor: &str,
    case: &str,
    input_id: &str,
    text: &str,
) -> ResultRecord {
    engine
        .execute(actor, RequestEnvelope::new(case, input_id, actor, text))
        .unwrap_or_else(|error| panic!("{actor}/{input_id} failed: {error}"))
}

fn commit(result: &ResultRecord) {
    assert_eq!(result.lifecycle, Lifecycle::Commit, "{result:?}");
}

fn relation(subject: &str, object: &str, scope: &str) -> Proposition {
    Proposition {
        subject: subject.into(),
        predicate: "is".into(),
        object: object.into(),
        negative: false,
        scope: scope.into(),
        temporal_scope: None,
    }
}

#[test]
fn proof_composition_changes_a_direct_core_result_and_uses_the_shorter_path() {
    let directory = TestDirectory::new();
    let actor = "b49-proof-composition-actor";
    let case = "b49-proof-composition-case";
    let scope = "materials-lab";
    let mut engine = Engine::open(directory.path()).expect("open isolated Core state");

    let aster_ceramic_text = format!("[scope={scope}]Aster is a ceramic");
    let aster_struck_text = format!("[scope={scope}]Aster is struck");
    let aster_query_text = format!("[scope={scope}]Aster is cracked?");
    let aster_ceramic = execute(
        &mut engine,
        actor,
        case,
        "aster-ceramic-before-learning",
        &aster_ceramic_text,
    );
    commit(&aster_ceramic);
    let ceramic_fact_id = aster_ceramic.dependencies[0].clone();
    let aster_struck = execute(
        &mut engine,
        actor,
        case,
        "aster-struck-before-learning",
        &aster_struck_text,
    );
    commit(&aster_struck);
    let struck_fact_id = aster_struck.dependencies[0].clone();

    let before_learning = execute(
        &mut engine,
        actor,
        case,
        "aster-cracked-before-learning",
        &aster_query_text,
    );
    assert_eq!(before_learning.lifecycle, Lifecycle::Suspend);
    assert!(
        before_learning
            .reason
            .as_deref()
            .unwrap_or_default()
            .contains("MISSING_PREMISES")
    );
    assert!(before_learning.content_plan.is_none());

    let first_rule_text =
        format!("If [scope={scope}]X is a ceramic then [scope={scope}]X is fragile");
    let first_rule = execute(
        &mut engine,
        actor,
        case,
        "ceramic-implies-fragile",
        &first_rule_text,
    );
    commit(&first_rule);
    let first_rule_id = first_rule.dependencies[0].clone();
    let second_rule_text =
        format!("If [scope={scope}]X is fragile and struck then [scope={scope}]X is cracked");
    let second_rule = execute(
        &mut engine,
        actor,
        case,
        "fragile-and-struck-implies-cracked",
        &second_rule_text,
    );
    commit(&second_rule);
    let second_rule_id = second_rule.dependencies[0].clone();

    let store = Store::open(directory.path()).expect("open Core memory snapshot");
    let memory_after_learning = store
        .load_memory(actor)
        .expect("load actor memory after proof composition");
    let shortcut = memory_after_learning
        .rules
        .iter()
        .find(|rule| {
            rule.valid
                && rule.dependencies.contains(&first_rule_id)
                && rule.dependencies.contains(&second_rule_id)
                && rule.conclusion == relation("$v0", "cracked", scope)
        })
        .expect("Core retained the composed proof with both declared parents");
    let shortcut_id = shortcut.id.clone();

    let target = relation("Aster", "cracked", scope);
    let composed_steps = relations::query(&memory_after_learning, &target, 1)
        .expect("the composed proof fits a one-step relation budget")
        .steps;
    let mut uncomposed_memory = memory_after_learning.clone();
    uncomposed_memory
        .rules
        .retain(|rule| rule.dependencies.is_empty());
    let root_steps = relations::query(&uncomposed_memory, &target, 2)
        .expect("the original two-rule proof fits a two-step budget")
        .steps;
    assert_eq!(root_steps, 2);
    assert_eq!(composed_steps, 1);

    drop(store);
    drop(engine);
    let mut reopened = Engine::open(directory.path()).expect("reopen Core after learning");
    let after_learning = execute(
        &mut reopened,
        actor,
        case,
        "aster-cracked-after-learning",
        &aster_query_text,
    );
    commit(&after_learning);
    assert_eq!(after_learning.guarantee, Guarantee::Derived);
    assert!(after_learning.dependencies.contains(&ceramic_fact_id));
    assert!(after_learning.dependencies.contains(&struck_fact_id));
    assert!(after_learning.dependencies.contains(&first_rule_id));
    assert!(after_learning.dependencies.contains(&second_rule_id));
    assert!(after_learning.dependencies.contains(&shortcut_id));
    assert!(after_learning.content_plan.is_some());
    assert_eq!(
        after_learning
            .render
            .as_ref()
            .map(|render| render.status.as_str()),
        Some("RENDERED")
    );

    let cyra_ceramic = execute(
        &mut reopened,
        actor,
        case,
        "unseen-cyra-ceramic",
        &format!("[scope={scope}]Cyra is a ceramic"),
    );
    commit(&cyra_ceramic);
    let cyra_struck = execute(
        &mut reopened,
        actor,
        case,
        "unseen-cyra-struck",
        &format!("[scope={scope}]Cyra is struck"),
    );
    commit(&cyra_struck);
    let unseen_subject = execute(
        &mut reopened,
        actor,
        case,
        "unseen-cyra-cracked",
        &format!("[scope={scope}]Cyra is cracked?"),
    );
    commit(&unseen_subject);
    assert_eq!(unseen_subject.guarantee, Guarantee::Derived);
    assert!(unseen_subject.dependencies.contains(&shortcut_id));
    assert!(!first_rule_text.contains("Cyra") && !second_rule_text.contains("Cyra"));

    let outside_ceramic = execute(
        &mut reopened,
        actor,
        case,
        "out-of-domain-rook-ceramic",
        "[scope=other-lab]Rook is a ceramic",
    );
    commit(&outside_ceramic);
    let outside_struck = execute(
        &mut reopened,
        actor,
        case,
        "out-of-domain-rook-struck",
        "[scope=other-lab]Rook is struck",
    );
    commit(&outside_struck);
    let outside_domain = execute(
        &mut reopened,
        actor,
        case,
        "out-of-domain-rook-cracked",
        "[scope=other-lab]Rook is cracked?",
    );
    assert_eq!(outside_domain.lifecycle, Lifecycle::Suspend);
    assert!(
        outside_domain
            .reason
            .as_deref()
            .unwrap_or_default()
            .contains("MISSING_PREMISES")
    );
    assert!(outside_domain.content_plan.is_none());

    let correction_text = format!("[scope={scope}]Correction: Aster is a polymer, not a ceramic.");
    let correction = execute(
        &mut reopened,
        actor,
        case,
        "later-aster-counterexample",
        &correction_text,
    );
    commit(&correction);
    assert!(correction.dependencies.contains(&ceramic_fact_id));
    let after_counterexample = execute(
        &mut reopened,
        actor,
        case,
        "aster-cracked-after-counterexample",
        &aster_query_text,
    );
    assert_eq!(after_counterexample.lifecycle, Lifecycle::Suspend);
    assert!(
        after_counterexample
            .reason
            .as_deref()
            .unwrap_or_default()
            .contains("MISSING_PREMISES")
    );
    assert!(after_counterexample.content_plan.is_none());
    let after_counterexample_cyra = execute(
        &mut reopened,
        actor,
        case,
        "cyra-remains-supported-after-aster-correction",
        &format!("[scope={scope}]Cyra is cracked?"),
    );
    commit(&after_counterexample_cyra);
    assert_eq!(after_counterexample_cyra.guarantee, Guarantee::Derived);
    assert!(
        after_counterexample_cyra
            .dependencies
            .contains(&shortcut_id)
    );

    let final_case = reopened
        .latest(actor, case)
        .expect("read the final Core case")
        .expect("Core case remains recorded");
    assert!(final_case.prior_results.contains_key(&after_learning.id));
    assert_eq!(
        final_case.prior_results.get(&after_learning.id),
        Some(&after_learning),
        "the original committed derived result remains immutable after the counterexample"
    );

    println!(
        "MINIDORA_B49_TRACE={}",
        json!({
            "schema": "MINIDORA_CORE_PROOF_COMPOSITION_LEARNING_TRACE_V1",
            "actor_id": actor,
            "case_id": case,
            "scope": scope,
            "core_reopened_after_composition": true,
            "before_learning": {
                "lifecycle": before_learning.lifecycle,
                "reason": before_learning.reason,
                "content_plan_created": before_learning.content_plan.is_some()
            },
            "after_learning": {
                "lifecycle": after_learning.lifecycle,
                "guarantee": after_learning.guarantee,
                "dependencies": after_learning.dependencies,
                "render_status": after_learning.render.as_ref().map(|render| render.status.as_str()),
                "result_id": after_learning.id
            },
            "shortcut_id": shortcut_id,
            "root_rule_ids": [first_rule_id, second_rule_id],
            "measured_query_steps": {
                "root_rules_only": root_steps,
                "composed_shortcut": composed_steps,
                "core_commit_with_default_request_budget": true
            },
            "unseen_subject_after_learning": {
                "subject": "Cyra",
                "lifecycle": unseen_subject.lifecycle,
                "guarantee": unseen_subject.guarantee,
                "shortcut_dependency_retained": unseen_subject.dependencies.contains(&shortcut_id)
            },
            "out_of_domain_scope": {
                "scope": "other-lab",
                "lifecycle": outside_domain.lifecycle,
                "reason": outside_domain.reason,
                "content_plan_created": outside_domain.content_plan.is_some()
            },
            "later_counterexample": {
                "lifecycle": correction.lifecycle,
                "invalidated_fact_id": ceramic_fact_id,
                "same_query_after_correction": after_counterexample.lifecycle,
                "same_query_reason": after_counterexample.reason,
                "prior_derived_result_retained": final_case.prior_results.contains_key(&after_learning.id),
                "unaffected_unseen_subject_still_derived": after_counterexample_cyra.lifecycle == Lifecycle::Commit
            },
            "outside_world_truth": "not checked",
            "empirical_hypothesis_learning": "not exercised",
            "normal_cli_http_and_full_cap_d05_acceptance": "not checked"
        })
    );
}
