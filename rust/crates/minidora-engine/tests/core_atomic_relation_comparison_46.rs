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
            "minidora-core-atomic-relation-comparison-46-{}-{serial}",
            std::process::id()
        ));
        fs::create_dir(&path).expect("create isolated relation-comparison state");
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
        .unwrap_or_else(|error| panic!("Engine::execute failed for {input}: {error}"));
    assert_eq!(result.lifecycle, Lifecycle::Commit, "{result:?}");
    result
        .dependencies
        .first()
        .expect("seed result exposes its adopted relation or rule ID")
        .clone()
}

fn seed_v1_control(engine: &mut Engine, actor: &str, scope: &str) -> String {
    let prefix = "b46-v1-control";
    commit_seed(
        engine,
        actor,
        "b46-v1-control-rule",
        "sensor-to-reliable",
        &format!("If [scope={scope}]X is a sensor then [scope={scope}]X is reliable"),
    );
    let mut aster_sensor_id = None;
    for (name, subject, object) in [
        ("aster-sensor", "Aster", "sensor"),
        ("aster-musician", "Aster", "musician"),
        ("aster-pilot", "Aster", "pilot"),
        ("beryl-sensor", "Beryl", "sensor"),
        ("beryl-pilot", "Beryl", "pilot"),
        ("beryl-geologist", "Beryl", "geologist"),
    ] {
        let id = commit_seed(
            engine,
            actor,
            &format!("{prefix}-{name}"),
            &name.replace('-', "_"),
            &format!("[scope={scope}]{subject} is a {object}"),
        );
        if name == "aster-sensor" {
            aster_sensor_id = Some(id);
        }
    }
    aster_sensor_id.expect("V1 control scope includes Aster's direct sensor premise")
}

fn seed_v4_rule_scope(engine: &mut Engine, actor: &str, scope: &str) -> (String, String) {
    let first_rule = commit_seed(
        engine,
        actor,
        "b46-v4-target-rule-one",
        "sensor-to-inspected",
        &format!("If [scope={scope}]X is a sensor then [scope={scope}]X is inspected"),
    );
    let second_rule = commit_seed(
        engine,
        actor,
        "b46-v4-target-rule-two",
        "inspected-to-reliable",
        &format!("If [scope={scope}]X is inspected then [scope={scope}]X is reliable"),
    );
    commit_seed(
        engine,
        actor,
        "b46-v4-target-aster-premise",
        "aster-sensor",
        &format!("[scope={scope}]Aster is a sensor"),
    );
    commit_seed(
        engine,
        actor,
        "b46-v4-target-beryl-premise",
        "beryl-sensor",
        &format!("[scope={scope}]Beryl is a sensor"),
    );
    (first_rule, second_rule)
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
    label: &'static str,
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
    label: &'static str,
    input_id: &str,
    scope: &str,
    expected_kind: &str,
    expected_proof_id: &str,
) -> SavedComparison {
    let mut request = RequestEnvelope::new(case, input_id, actor, "");
    request.budget.max_actions = 256;
    let compared = engine
        .compose_atomic_relation_comparison(actor, request, "Aster", "Beryl", scope)
        .unwrap_or_else(|error| panic!("{label} comparison failed: {error}"));
    assert_eq!(
        compared.lifecycle,
        Lifecycle::Commit,
        "{label}: {compared:?}"
    );
    let plan = compared
        .content_plan
        .as_ref()
        .unwrap_or_else(|| panic!("{label} comparison has no adopted ContentPlan"));
    assert_eq!(plan.text, compared.text, "{label} plan text");
    let render = compared
        .render
        .as_ref()
        .unwrap_or_else(|| panic!("{label} comparison has no render observation"));
    assert_eq!(render.status, "RENDERED", "{label} render");
    assert_eq!(
        render.bytes,
        compared.text.as_bytes(),
        "{label} render bytes"
    );

    let artifact_ref = compared
        .dependencies
        .first()
        .unwrap_or_else(|| panic!("{label} result has no comparison artifact"));
    let artifact_bytes = store
        .get_bytes(actor, case, artifact_ref)
        .unwrap_or_else(|error| panic!("{label} artifact read failed: {error}"));
    let artifact: Value =
        serde_json::from_slice(&artifact_bytes).expect("parse persisted comparison artifact");
    assert_eq!(artifact["kind"], expected_kind, "{label} artifact kind");
    assert_eq!(
        artifact["result_id"], compared.id,
        "{label} result identity"
    );
    assert_eq!(artifact["scope"], scope, "{label} artifact scope");
    let proof_ids = inventory_proof_ids(&artifact);
    assert!(!proof_ids.is_empty(), "{label} has no inventory proof IDs");
    assert!(
        proof_ids.contains(expected_proof_id),
        "{label} omitted the selected proof ID {expected_proof_id}"
    );

    SavedComparison {
        label,
        result_id: compared.id,
        artifact_ref: artifact_ref.clone(),
        artifact_sha256: digest(&artifact_bytes),
        proof_ids,
    }
}

fn invalidate_rule(engine: &mut Engine, actor: &str, case: &str, rule_id: &str) -> ResultRecord {
    let result = engine
        .execute(
            actor,
            RequestEnvelope::new(
                case,
                "b46-invalidate-v4-target-rule",
                actor,
                &format!("invalidate {rule_id}"),
            ),
        )
        .unwrap_or_else(|error| panic!("invalidate target rule failed: {error}"));
    assert_eq!(result.lifecycle, Lifecycle::Commit, "{result:?}");
    assert!(
        result
            .dependencies
            .iter()
            .any(|dependency| dependency == rule_id),
        "rule invalidation result omitted its selected ID: {result:?}"
    );
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
        .unwrap_or_else(|error| panic!("{} follow-up failed: {error}", comparison.label))
}

fn assert_proof_changed_stop(result: &ResultRecord) {
    assert_eq!(result.lifecycle, Lifecycle::Suspend, "{result:?}");
    assert!(
        result
            .reason
            .as_deref()
            .is_some_and(|reason| reason.starts_with("CONTEXT_PROOF_CHANGED:")),
        "V4 target did not report CONTEXT_PROOF_CHANGED: {result:?}"
    );
    assert!(
        result.content_plan.is_none(),
        "stale V4 content was adopted"
    );
    assert!(result.render.is_none(), "stale V4 content was rendered");
}

fn assert_v1_control_replayed(result: &ResultRecord, comparison: &SavedComparison) {
    assert_eq!(result.lifecycle, Lifecycle::Commit, "{result:?}");
    assert!(
        result.dependencies.contains(&comparison.result_id),
        "V1 control did not consume its explicitly selected result: {result:?}"
    );
    assert!(
        result.content_plan.is_some(),
        "V1 control has no ContentPlan"
    );
    let render = result
        .render
        .as_ref()
        .expect("V1 control has a render receipt");
    assert_eq!(render.status, "RENDERED");
    assert_eq!(render.bytes, result.text.as_bytes());
}

fn assert_invalidated_rule_and_descendants_retained(
    memory: &MemoryState,
    rule_id: &str,
    dependent_rule_ids: &[String],
) {
    let rule = memory
        .rules
        .iter()
        .find(|rule| rule.id == rule_id)
        .expect("invalidated root rule remains stored");
    assert!(!rule.valid, "invalidated root rule remains active");
    assert!(
        !dependent_rule_ids.is_empty(),
        "no composed dependent rule was observed"
    );
    for dependent_id in dependent_rule_ids {
        let dependent = memory
            .rules
            .iter()
            .find(|rule| &rule.id == dependent_id)
            .expect("dependent composed rule remains stored");
        assert!(
            !dependent.valid,
            "dependent rule remains active: {dependent_id}"
        );
        assert!(
            dependent.dependencies.contains(&rule_id.to_owned()),
            "dependent rule lost its root-rule lineage: {dependent:?}"
        );
    }
}

#[test]
fn v4_saved_comparison_stops_after_rule_invalidation_while_v1_control_replays() {
    let directory = TestDirectory::new();
    let actor = "b46-v4-rule-v1-control";
    let comparison_case = "b46-coexisting-v4-rule-v1-comparison";
    let invalidation_case = "b46-invalidate-target-rule";
    let target_scope = "b46-v4-target-scope";
    let control_scope = "b46-v1-control-scope";
    let mut engine = Engine::open(directory.path()).expect("open isolated Core state");

    let (target_rule_id, _second_rule_id) = seed_v4_rule_scope(&mut engine, actor, target_scope);
    let control_fact_id = seed_v1_control(&mut engine, actor, control_scope);
    let store = Store::open(directory.path()).expect("open isolated artifact store");
    let v4_target = save_comparison(
        &mut engine,
        &store,
        actor,
        comparison_case,
        "v4-target",
        "b46-save-v4-target",
        target_scope,
        "minidora.relation-comparison-content.v4",
        &target_rule_id,
    );
    let v1_control = save_comparison(
        &mut engine,
        &store,
        actor,
        comparison_case,
        "v1-control",
        "b46-save-v1-control",
        control_scope,
        "minidora.relation-comparison-report.v1",
        &control_fact_id,
    );
    assert_ne!(v4_target.artifact_ref, v1_control.artifact_ref);
    let initial_memory = store
        .load_memory(actor)
        .expect("read active target and control memory");
    let dependent_rule_ids = initial_memory
        .rules
        .iter()
        .filter(|rule| rule.dependencies.contains(&target_rule_id))
        .map(|rule| rule.id.clone())
        .collect::<Vec<_>>();
    assert!(
        !dependent_rule_ids.is_empty(),
        "Core did not retain a composed rule depending on the target rule"
    );
    assert!(
        dependent_rule_ids
            .iter()
            .all(|id| v4_target.proof_ids.contains(id))
    );

    let original_target_bytes = store
        .get_bytes(actor, comparison_case, &v4_target.artifact_ref)
        .expect("read original V4 target artifact");
    let original_control_bytes = store
        .get_bytes(actor, comparison_case, &v1_control.artifact_ref)
        .expect("read original V1 control artifact");
    assert_eq!(digest(&original_target_bytes), v4_target.artifact_sha256);
    assert_eq!(digest(&original_control_bytes), v1_control.artifact_sha256);

    let invalidation = invalidate_rule(&mut engine, actor, invalidation_case, &target_rule_id);
    assert!(
        dependent_rule_ids
            .iter()
            .all(|id| invalidation.dependencies.contains(id))
    );
    let memory_before_invalidation = initial_memory.revision;
    drop(store);
    drop(engine);

    let mut reopened = Engine::open(directory.path()).expect("reopen Core after rule invalidation");
    let reopened_store = Store::open(directory.path()).expect("reopen artifact and memory store");
    let invalidated_memory = reopened_store
        .load_memory(actor)
        .expect("read rule invalidation after restart");
    assert!(invalidated_memory.revision > memory_before_invalidation);
    assert_invalidated_rule_and_descendants_retained(
        &invalidated_memory,
        &target_rule_id,
        &dependent_rule_ids,
    );
    assert_eq!(
        digest(
            &reopened_store
                .get_bytes(actor, comparison_case, &v4_target.artifact_ref)
                .expect("V4 artifact remains stored after rule invalidation")
        ),
        v4_target.artifact_sha256
    );
    assert_eq!(
        digest(
            &reopened_store
                .get_bytes(actor, comparison_case, &v1_control.artifact_ref)
                .expect("V1 control artifact remains stored after rule invalidation")
        ),
        v1_control.artifact_sha256
    );

    let v4_follow_up = follow_up(
        &mut reopened,
        actor,
        comparison_case,
        "b46-follow-up-v4-target-after-rule-invalidation",
        &v4_target,
    );
    assert_proof_changed_stop(&v4_follow_up);
    let v1_follow_up = follow_up(
        &mut reopened,
        actor,
        comparison_case,
        "b46-follow-up-v1-control-after-v4-target-stop",
        &v1_control,
    );
    assert_v1_control_replayed(&v1_follow_up, &v1_control);

    println!(
        "MINIDORA_B46_TRACE={}",
        json!({
            "schema": "MINIDORA_CORE_RELATION_COMPARISON_V4_RULE_INVALIDATION_TRACE_V1",
            "actor": actor,
            "comparison_case": comparison_case,
            "invalidation_case": invalidation_case,
            "scopes": { "v4_target": target_scope, "v1_control": control_scope },
            "invalidated_rule_id": target_rule_id,
            "invalidated_record_ids": invalidation.dependencies,
            "dependent_rule_ids_retained_invalid": dependent_rule_ids,
            "memory_revision_before": memory_before_invalidation,
            "memory_revision_after": invalidated_memory.revision,
            "engine_reopened_after_invalidation": true,
            "target": {
                "artifact_kind": "minidora.relation-comparison-content.v4",
                "artifact_ref": v4_target.artifact_ref,
                "artifact_sha256": v4_target.artifact_sha256,
                "proof_ids": v4_target.proof_ids,
                "retained_after_invalidation": true,
                "follow_up_result_id": v4_follow_up.id,
                "lifecycle": v4_follow_up.lifecycle,
                "reason": v4_follow_up.reason,
                "content_plan_created": v4_follow_up.content_plan.is_some(),
                "render_created": v4_follow_up.render.is_some()
            },
            "control": {
                "artifact_kind": "minidora.relation-comparison-report.v1",
                "artifact_ref": v1_control.artifact_ref,
                "artifact_sha256": v1_control.artifact_sha256,
                "selected_result_id": v1_control.result_id,
                "retained_after_invalidation": true,
                "follow_up_result_id": v1_follow_up.id,
                "selected_comparison_dependency": v1_follow_up.dependencies.contains(&v1_control.result_id),
                "lifecycle": v1_follow_up.lifecycle,
                "render_status": v1_follow_up.render.as_ref().map(|render| render.status.as_str()),
                "render_sha256": v1_follow_up.render.as_ref().map(|render| render.actual_sha256.as_str())
            },
            "outside_world_truth": "not checked",
            "migration_or_concurrency": "not checked"
        })
    );
}
