use minidora_domain::{Lifecycle, MemoryState, RequestEnvelope, ResultRecord, digest};
use minidora_engine::{Engine, store::Store};
use serde_json::{Value, json};
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
            "minidora-core-atomic-relation-comparison-47-{}-{serial}",
            std::process::id()
        ));
        fs::create_dir(&path).expect("create isolated actor-isolation state");
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

fn commit_seed(engine: &mut Engine, actor: &str, case: &str, input: &str, text: &str) -> String {
    let result = engine
        .execute(actor, RequestEnvelope::new(case, input, actor, text))
        .unwrap_or_else(|error| panic!("Engine::execute failed for {actor}/{input}: {error}"));
    assert_eq!(result.lifecycle, Lifecycle::Commit, "{actor}: {result:?}");
    result
        .dependencies
        .first()
        .expect("seed result exposes its adopted relation or rule ID")
        .clone()
}

fn seed_rule_chain(
    engine: &mut Engine,
    actor: &str,
    namespace: &str,
    scope: &str,
) -> (String, String) {
    let root_rule_id = commit_seed(
        engine,
        actor,
        &format!("{namespace}-rule-one"),
        "sensor_to_inspected",
        &format!("If [scope={scope}]X is a sensor then [scope={scope}]X is inspected"),
    );
    let second_rule_id = commit_seed(
        engine,
        actor,
        &format!("{namespace}-rule-two"),
        "inspected_to_reliable",
        &format!("If [scope={scope}]X is inspected then [scope={scope}]X is reliable"),
    );
    for (subject, input) in [("Aster", "aster_sensor"), ("Beryl", "beryl_sensor")] {
        commit_seed(
            engine,
            actor,
            &format!("{namespace}-{input}"),
            input,
            &format!("[scope={scope}]{subject} is a sensor"),
        );
    }
    (root_rule_id, second_rule_id)
}

fn inventory_proof_ids(artifact: &Value) -> BTreeSet<String> {
    [&artifact["inventory_a"], &artifact["inventory_b"]]
        .into_iter()
        .fold(BTreeSet::new(), |mut all, inventory| {
            all.extend(
                inventory["dependencies"]
                    .as_array()
                    .expect("inventory dependencies are an array")
                    .iter()
                    .map(|id| id.as_str().expect("dependency ID is a string").to_owned()),
            );
            for fact in inventory["facts"]
                .as_array()
                .expect("inventory facts are an array")
            {
                all.insert(fact["id"].as_str().expect("fact ID is a string").to_owned());
                all.extend(
                    fact["dependencies"]
                        .as_array()
                        .expect("fact dependencies are an array")
                        .iter()
                        .map(|id| id.as_str().expect("dependency ID is a string").to_owned()),
                );
            }
            all
        })
}

struct SavedComparison {
    result_id: String,
    artifact_ref: String,
    artifact_sha256: String,
    proof_ids: BTreeSet<String>,
}

fn save_comparison(
    engine: &mut Engine,
    store: &Store,
    actor: &str,
    case: &str,
    input_id: &str,
    scope: &str,
    expected_rule_ids: &[&str],
) -> SavedComparison {
    let mut request = RequestEnvelope::new(case, input_id, actor, "");
    request.budget.max_actions = 256;
    let compared = engine
        .compose_atomic_relation_comparison(actor, request, "Aster", "Beryl", scope)
        .unwrap_or_else(|error| panic!("{actor} comparison failed: {error}"));
    assert_eq!(
        compared.lifecycle,
        Lifecycle::Commit,
        "{actor}: {compared:?}"
    );
    let plan = compared
        .content_plan
        .as_ref()
        .expect("comparison has an adopted ContentPlan");
    assert_eq!(plan.text, compared.text);
    let render = compared
        .render
        .as_ref()
        .expect("comparison has a render receipt");
    assert_eq!(render.status, "RENDERED");
    assert_eq!(render.bytes, compared.text.as_bytes());

    let artifact_ref = compared
        .dependencies
        .first()
        .expect("comparison result has its persisted artifact reference");
    let artifact_bytes = store
        .get_bytes(actor, case, artifact_ref)
        .unwrap_or_else(|error| panic!("{actor} artifact read failed: {error}"));
    let artifact: Value = serde_json::from_slice(&artifact_bytes).expect("parse V4 artifact");
    assert_eq!(artifact["kind"], "minidora.relation-comparison-content.v4");
    assert_eq!(artifact["result_id"], compared.id);
    assert_eq!(artifact["scope"], scope);
    let proof_ids = inventory_proof_ids(&artifact);
    for rule_id in expected_rule_ids {
        assert!(
            proof_ids.contains(*rule_id),
            "{actor} proof omitted {rule_id}"
        );
    }

    SavedComparison {
        result_id: compared.id,
        artifact_ref: artifact_ref.clone(),
        artifact_sha256: digest(&artifact_bytes),
        proof_ids,
    }
}

fn dependent_rules(memory: &MemoryState, root_rule_id: &str) -> Vec<String> {
    memory
        .rules
        .iter()
        .filter(|rule| rule.dependencies.iter().any(|id| id == root_rule_id))
        .map(|rule| rule.id.clone())
        .collect()
}

fn invalidate_rule(engine: &mut Engine, actor: &str, rule_id: &str) -> ResultRecord {
    let result = engine
        .execute(
            actor,
            RequestEnvelope::new(
                "b47-actor-a-invalidation",
                "invalidate_actor_a_root_rule",
                actor,
                &format!("invalidate {rule_id}"),
            ),
        )
        .unwrap_or_else(|error| panic!("{actor} rule invalidation failed: {error}"));
    assert_eq!(result.lifecycle, Lifecycle::Commit, "{result:?}");
    assert!(result.dependencies.iter().any(|id| id == rule_id));
    result
}

fn follow_up(
    engine: &mut Engine,
    actor: &str,
    case: &str,
    input_id: &str,
    comparison: &SavedComparison,
) -> ResultRecord {
    let mut request = RequestEnvelope::new(case, input_id, actor, "なぜ");
    request.budget.max_actions = 256;
    request.context_ref = Some(comparison.result_id.clone());
    engine
        .execute(actor, request)
        .unwrap_or_else(|error| panic!("{actor} selected-artifact follow-up failed: {error}"))
}

fn assert_proof_changed_stop(result: &ResultRecord) {
    assert_eq!(result.lifecycle, Lifecycle::Suspend, "{result:?}");
    assert!(
        result
            .reason
            .as_deref()
            .is_some_and(|reason| reason.starts_with("CONTEXT_PROOF_CHANGED:"))
    );
    assert!(
        result.content_plan.is_none(),
        "stale V4 content was adopted"
    );
    assert!(result.render.is_none(), "stale V4 content was rendered");
}

fn assert_v4_replayed(result: &ResultRecord, comparison: &SavedComparison) {
    assert_eq!(result.lifecycle, Lifecycle::Commit, "{result:?}");
    assert!(result.dependencies.contains(&comparison.result_id));
    assert!(
        result.content_plan.is_some(),
        "selected V4 result has no ContentPlan"
    );
    let render = result
        .render
        .as_ref()
        .expect("selected V4 result has a render receipt");
    assert_eq!(render.status, "RENDERED");
    assert_eq!(render.bytes, result.text.as_bytes());
}

#[test]
fn invalidating_one_actor_rule_does_not_change_another_actors_saved_v4_proof() {
    let directory = TestDirectory::new();
    let actor_a = "b47-rule-owner-a";
    let actor_b = "b47-independent-owner-b";
    let scope = "b47-shared-semantic-scope";
    let case_a = "b47-actor-a-comparison";
    let case_b = "b47-actor-b-comparison";
    let mut engine = Engine::open(directory.path()).expect("open isolated Core state");

    let rules_a = seed_rule_chain(&mut engine, actor_a, "b47-actor-a", scope);
    let rules_b = seed_rule_chain(&mut engine, actor_b, "b47-actor-b", scope);
    assert_ne!(rules_a.0, rules_b.0, "root rule IDs are actor-specific");
    assert_ne!(rules_a.1, rules_b.1, "second rule IDs are actor-specific");

    let store = Store::open(directory.path()).expect("open isolated actor memories and artifacts");
    let comparison_a = save_comparison(
        &mut engine,
        &store,
        actor_a,
        case_a,
        "b47-actor-a-save-v4",
        scope,
        &[&rules_a.0, &rules_a.1],
    );
    let comparison_b = save_comparison(
        &mut engine,
        &store,
        actor_b,
        case_b,
        "b47-actor-b-save-v4",
        scope,
        &[&rules_b.0, &rules_b.1],
    );
    assert_ne!(comparison_a.result_id, comparison_b.result_id);
    assert!(comparison_a.proof_ids.is_disjoint(&comparison_b.proof_ids));

    let memory_a_before = store
        .load_memory(actor_a)
        .expect("read actor A memory before invalidation");
    let memory_b_before = store
        .load_memory(actor_b)
        .expect("read actor B memory before invalidation");
    let dependent_a = dependent_rules(&memory_a_before, &rules_a.0);
    let dependent_b = dependent_rules(&memory_b_before, &rules_b.0);
    assert!(
        !dependent_a.is_empty(),
        "actor A has no composed rule dependent on its root"
    );
    assert!(
        !dependent_b.is_empty(),
        "actor B has no composed rule dependent on its root"
    );
    assert!(
        dependent_a
            .iter()
            .all(|id| comparison_a.proof_ids.contains(id))
    );
    assert!(
        dependent_b
            .iter()
            .all(|id| comparison_b.proof_ids.contains(id))
    );

    for (actor, case, comparison) in [
        (actor_a, case_a, &comparison_a),
        (actor_b, case_b, &comparison_b),
    ] {
        let bytes = store
            .get_bytes(actor, case, &comparison.artifact_ref)
            .unwrap_or_else(|error| panic!("{actor} saved artifact read failed: {error}"));
        assert_eq!(digest(&bytes), comparison.artifact_sha256);
    }

    let invalidation = invalidate_rule(&mut engine, actor_a, &rules_a.0);
    assert!(
        dependent_a
            .iter()
            .all(|id| invalidation.dependencies.contains(id))
    );
    let revision_a_before = memory_a_before.revision;
    let revision_b_before = memory_b_before.revision;
    drop(store);
    drop(engine);

    let mut reopened =
        Engine::open(directory.path()).expect("reopen Core after actor A invalidation");
    let reopened_store =
        Store::open(directory.path()).expect("reopen actor memories and artifacts");
    let memory_a_after = reopened_store
        .load_memory(actor_a)
        .expect("read invalidated actor A memory");
    let memory_b_after = reopened_store
        .load_memory(actor_b)
        .expect("read actor B memory after actor A invalidation");
    assert!(memory_a_after.revision > revision_a_before);
    assert_eq!(
        memory_b_after, memory_b_before,
        "actor A invalidation changed actor B memory"
    );

    let root_a = memory_a_after
        .rules
        .iter()
        .find(|rule| rule.id == rules_a.0)
        .expect("actor A root retained");
    assert!(
        !root_a.valid,
        "actor A root rule remains active after invalidation"
    );
    for id in &dependent_a {
        let rule = memory_a_after
            .rules
            .iter()
            .find(|rule| &rule.id == id)
            .expect("actor A dependent retained");
        assert!(!rule.valid, "actor A dependent rule remains active: {id}");
    }
    let root_b = memory_b_after
        .rules
        .iter()
        .find(|rule| rule.id == rules_b.0)
        .expect("actor B root retained");
    let second_b = memory_b_after
        .rules
        .iter()
        .find(|rule| rule.id == rules_b.1)
        .expect("actor B second rule retained");
    assert!(
        root_b.valid && second_b.valid,
        "actor B's independent rule chain changed"
    );
    assert!(dependent_b.iter().all(|id| {
        memory_b_after
            .rules
            .iter()
            .any(|rule| rule.id == *id && rule.valid)
    }));
    assert_eq!(memory_b_after.revision, revision_b_before);

    for (actor, case, comparison) in [
        (actor_a, case_a, &comparison_a),
        (actor_b, case_b, &comparison_b),
    ] {
        let bytes = reopened_store
            .get_bytes(actor, case, &comparison.artifact_ref)
            .unwrap_or_else(|error| panic!("{actor} artifact was not retained: {error}"));
        assert_eq!(digest(&bytes), comparison.artifact_sha256);
    }

    let follow_up_a = follow_up(
        &mut reopened,
        actor_a,
        case_a,
        "b47-actor-a-follow-up-after-own-invalidation",
        &comparison_a,
    );
    assert_proof_changed_stop(&follow_up_a);
    let follow_up_b = follow_up(
        &mut reopened,
        actor_b,
        case_b,
        "b47-actor-b-follow-up-after-actor-a-invalidation",
        &comparison_b,
    );
    assert_v4_replayed(&follow_up_b, &comparison_b);
    let memory_b_after_follow_up = reopened_store
        .load_memory(actor_b)
        .expect("read actor B memory after replay");
    assert_eq!(memory_b_after_follow_up, memory_b_before);

    println!(
        "MINIDORA_B47_TRACE={}",
        json!({
            "schema": "MINIDORA_CORE_ACTOR_ISOLATED_V4_PROOF_INVALIDATION_TRACE_V1",
            "scope": scope,
            "actor_a": {
                "actor_id": actor_a,
                "case_id": case_a,
                "root_rule_id": rules_a.0,
                "dependent_rule_ids_retained_invalid": dependent_a,
                "memory_revision_before": revision_a_before,
                "memory_revision_after": memory_a_after.revision,
                "artifact_ref": comparison_a.artifact_ref,
                "artifact_sha256": comparison_a.artifact_sha256,
                "follow_up_result_id": follow_up_a.id,
                "lifecycle": follow_up_a.lifecycle,
                "reason": follow_up_a.reason,
                "content_plan_created": follow_up_a.content_plan.is_some(),
                "render_created": follow_up_a.render.is_some()
            },
            "actor_b": {
                "actor_id": actor_b,
                "case_id": case_b,
                "root_rule_id": rules_b.0,
                "second_rule_id": rules_b.1,
                "dependent_rule_ids_still_valid": dependent_b,
                "memory_revision_before": revision_b_before,
                "memory_revision_after_actor_a_invalidation": memory_b_after.revision,
                "memory_unchanged_after_replay": memory_b_after_follow_up == memory_b_before,
                "artifact_ref": comparison_b.artifact_ref,
                "artifact_sha256": comparison_b.artifact_sha256,
                "selected_result_id": comparison_b.result_id,
                "follow_up_result_id": follow_up_b.id,
                "lifecycle": follow_up_b.lifecycle,
                "render_status": follow_up_b.render.as_ref().map(|render| render.status.as_str()),
                "render_sha256": follow_up_b.render.as_ref().map(|render| render.actual_sha256.as_str())
            },
            "rule_ids_disjoint_by_actor": rules_a.0 != rules_b.0 && rules_a.1 != rules_b.1,
            "proof_ids_disjoint_by_actor": comparison_a.proof_ids.is_disjoint(&comparison_b.proof_ids),
            "engine_reopened_after_invalidation": true,
            "outside_world_truth": "not checked",
            "migration_or_concurrency": "not checked"
        })
    );
}
