use minidora_domain::{Lifecycle, RequestEnvelope, ResultRecord, digest};
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
            "minidora-core-atomic-relation-comparison-45-{}-{serial}",
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

fn seed_legacy_scope(engine: &mut Engine, actor: &str, prefix: &str, scope: &str) -> String {
    commit_seed(
        engine,
        actor,
        &format!("{prefix}-rule"),
        "sensor-to-reliable",
        &format!("If [scope={scope}]X is a sensor then [scope={scope}]X is reliable"),
    );
    let mut target_fact_id = None;
    for (name, subject, object) in [
        ("aster-sensor", "Aster", "sensor"),
        ("aster-musician", "Aster", "musician"),
        ("aster-pilot", "Aster", "pilot"),
        ("beryl-sensor", "Beryl", "sensor"),
        ("beryl-pilot", "Beryl", "pilot"),
        ("beryl-geologist", "Beryl", "geologist"),
    ] {
        let input = name.replace('-', "_");
        let case = format!("{prefix}-{name}");
        let id = commit_seed(
            engine,
            actor,
            &case,
            &input,
            &format!("[scope={scope}]{subject} is a {object}"),
        );
        if name == "aster-sensor" {
            target_fact_id = Some(id);
        }
    }
    target_fact_id.expect("legacy target scope includes Aster's direct sensor premise")
}

fn seed_v4_scope(engine: &mut Engine, actor: &str, prefix: &str, scope: &str) -> String {
    commit_seed(
        engine,
        actor,
        &format!("{prefix}-rule-one"),
        "sensor-to-inspected",
        &format!("If [scope={scope}]X is a sensor then [scope={scope}]X is inspected"),
    );
    commit_seed(
        engine,
        actor,
        &format!("{prefix}-rule-two"),
        "inspected-to-reliable",
        &format!("If [scope={scope}]X is inspected then [scope={scope}]X is reliable"),
    );
    let aster_fact_id = commit_seed(
        engine,
        actor,
        &format!("{prefix}-aster-premise"),
        "aster-sensor",
        &format!("[scope={scope}]Aster is a sensor"),
    );
    commit_seed(
        engine,
        actor,
        &format!("{prefix}-beryl-premise"),
        "beryl-sensor",
        &format!("[scope={scope}]Beryl is a sensor"),
    );
    aster_fact_id
}

struct SavedComparison {
    label: &'static str,
    kind: &'static str,
    scope: &'static str,
    result_id: String,
    artifact_ref: String,
    artifact_sha256: String,
    proof_count: usize,
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

fn save_comparison(
    engine: &mut Engine,
    store: &Store,
    actor: &str,
    case: &str,
    label: &'static str,
    input_id: &str,
    scope: &'static str,
    expected_kind: &'static str,
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
    assert_eq!(render.plan_id, plan.id, "{label} plan/render binding");
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
    if expected_kind == "minidora.relation-comparison-report.v1" {
        assert!(artifact.get("content_request").is_none());
        assert!(artifact.get("content_composition").is_none());
    } else {
        assert!(artifact.get("content_request").is_some());
        assert!(artifact.get("content_composition").is_some());
    }
    let proofs = inventory_proof_ids(&artifact);
    assert!(!proofs.is_empty(), "{label} has no inventory proof IDs");
    assert!(
        proofs.contains(expected_proof_id),
        "{label} omitted the fact selected for invalidation"
    );

    SavedComparison {
        label,
        kind: expected_kind,
        scope,
        result_id: compared.id,
        artifact_ref: artifact_ref.clone(),
        artifact_sha256: digest(&artifact_bytes),
        proof_count: proofs.len(),
    }
}

fn invalidate(engine: &mut Engine, actor: &str, suffix: &str, fact_id: &str) -> ResultRecord {
    let result = engine
        .execute(
            actor,
            RequestEnvelope::new(
                &format!("b45-invalidation-{suffix}"),
                "invalidate-target-premise",
                actor,
                &format!("invalidate {fact_id}"),
            ),
        )
        .unwrap_or_else(|error| panic!("invalidate {suffix} premise failed: {error}"));
    assert_eq!(result.lifecycle, Lifecycle::Commit, "{result:?}");
    assert!(
        result
            .dependencies
            .iter()
            .any(|dependency| dependency == fact_id),
        "{suffix} invalidation result omitted its selected premise: {result:?}"
    );
    result
}

fn follow_up(
    engine: &mut Engine,
    actor: &str,
    case: &str,
    input: &str,
    comparison: &SavedComparison,
) -> ResultRecord {
    let mut request = RequestEnvelope::new(case, input, actor, "なぜ");
    request.budget.max_actions = 256;
    request.context_ref = Some(comparison.result_id.clone());
    engine
        .execute(actor, request)
        .unwrap_or_else(|error| panic!("{} follow-up failed: {error}", comparison.label))
}

fn assert_proof_changed_stop(result: &ResultRecord, label: &str) {
    assert_eq!(result.lifecycle, Lifecycle::Suspend, "{label}: {result:?}");
    assert!(
        result
            .reason
            .as_deref()
            .is_some_and(|reason| reason.starts_with("CONTEXT_PROOF_CHANGED:")),
        "{label} follow-up did not report CONTEXT_PROOF_CHANGED: {result:?}"
    );
    assert!(
        result.content_plan.is_none(),
        "{label} adopted stale content"
    );
    assert!(result.render.is_none(), "{label} rendered stale content");
}

fn assert_replayed_control(result: &ResultRecord, comparison: &SavedComparison) {
    assert_eq!(result.lifecycle, Lifecycle::Commit, "{result:?}");
    assert!(
        result.dependencies.contains(&comparison.result_id),
        "{} control did not consume its selected result: {result:?}",
        comparison.label
    );
    assert!(
        result
            .text
            .contains(&format!("比較結果ID: {}", comparison.result_id)),
        "{} control omitted its selected result ID: {result:?}",
        comparison.label
    );
    assert!(result.content_plan.is_some());
    assert_eq!(
        result.render.as_ref().map(|render| render.status.as_str()),
        Some("RENDERED")
    );
}

#[test]
fn v1_and_v4_comparisons_invalidate_only_their_selected_scope() {
    let directory = TestDirectory::new();
    let actor = "b45-v1-v4-scoped-invalidation";
    let comparison_case = "b45-coexisting-v1-v4-comparisons";
    let v1_target_scope = "b45-v1-target-scope";
    let v1_control_scope = "b45-v1-control-scope";
    let v4_target_scope = "b45-v4-target-scope";
    let v4_control_scope = "b45-v4-control-scope";
    let mut engine = Engine::open(directory.path()).expect("open isolated Core state");

    let v1_target_fact = seed_legacy_scope(&mut engine, actor, "b45-v1-target", v1_target_scope);
    let v1_control_fact = seed_legacy_scope(&mut engine, actor, "b45-v1-control", v1_control_scope);
    let v4_target_fact = seed_v4_scope(&mut engine, actor, "b45-v4-target", v4_target_scope);
    let v4_control_fact = seed_v4_scope(&mut engine, actor, "b45-v4-control", v4_control_scope);

    let store = Store::open(directory.path()).expect("open isolated artifact store");
    let saved = vec![
        save_comparison(
            &mut engine,
            &store,
            actor,
            comparison_case,
            "v1-target",
            "b45-comparison-v1-target",
            v1_target_scope,
            "minidora.relation-comparison-report.v1",
            &v1_target_fact,
        ),
        save_comparison(
            &mut engine,
            &store,
            actor,
            comparison_case,
            "v4-control",
            "b45-comparison-v4-control",
            v4_control_scope,
            "minidora.relation-comparison-content.v4",
            &v4_control_fact,
        ),
        save_comparison(
            &mut engine,
            &store,
            actor,
            comparison_case,
            "v4-target",
            "b45-comparison-v4-target",
            v4_target_scope,
            "minidora.relation-comparison-content.v4",
            &v4_target_fact,
        ),
        save_comparison(
            &mut engine,
            &store,
            actor,
            comparison_case,
            "v1-control",
            "b45-comparison-v1-control",
            v1_control_scope,
            "minidora.relation-comparison-report.v1",
            &v1_control_fact,
        ),
    ];
    let unique_refs = saved
        .iter()
        .map(|comparison| comparison.artifact_ref.as_str())
        .collect::<BTreeSet<_>>();
    assert_eq!(
        unique_refs.len(),
        4,
        "all four artifacts have distinct refs"
    );
    for comparison in &saved {
        let bytes = store
            .get_bytes(actor, comparison_case, &comparison.artifact_ref)
            .unwrap_or_else(|error| panic!("{} was not co-readable: {error}", comparison.label));
        assert_eq!(digest(&bytes), comparison.artifact_sha256);
    }

    let v1_invalidation = invalidate(&mut engine, actor, "v1-target", &v1_target_fact);
    let v4_invalidation = invalidate(&mut engine, actor, "v4-target", &v4_target_fact);
    drop(store);
    drop(engine);

    let mut reopened = Engine::open(directory.path()).expect("reopen persisted Core state");
    let v1_target = follow_up(
        &mut reopened,
        actor,
        comparison_case,
        "b45-followup-v1-target-after-invalidation",
        &saved[0],
    );
    assert_proof_changed_stop(&v1_target, "V1 target");
    let v4_target = follow_up(
        &mut reopened,
        actor,
        comparison_case,
        "b45-followup-v4-target-after-invalidation",
        &saved[2],
    );
    assert_proof_changed_stop(&v4_target, "V4 target");

    let v4_control = follow_up(
        &mut reopened,
        actor,
        comparison_case,
        "b45-followup-v4-control-after-target-stops",
        &saved[1],
    );
    assert_replayed_control(&v4_control, &saved[1]);
    let v1_control = follow_up(
        &mut reopened,
        actor,
        comparison_case,
        "b45-followup-v1-control-after-target-stops",
        &saved[3],
    );
    assert_replayed_control(&v1_control, &saved[3]);

    let store = Store::open(directory.path()).expect("reopen comparison artifact store");
    let retained_artifacts = saved
        .iter()
        .map(|comparison| {
            let bytes = store
                .get_bytes(actor, comparison_case, &comparison.artifact_ref)
                .unwrap_or_else(|error| {
                    panic!("{} artifact was not retained: {error}", comparison.label)
                });
            assert_eq!(digest(&bytes), comparison.artifact_sha256);
            json!({
                "label": comparison.label,
                "kind": comparison.kind,
                "scope": comparison.scope,
                "result_id": comparison.result_id,
                "artifact_ref": comparison.artifact_ref,
                "artifact_sha256": comparison.artifact_sha256,
                "proof_count": comparison.proof_count,
                "retained_after_invalidation": true,
            })
        })
        .collect::<Vec<_>>();

    println!(
        "MINIDORA_B45_TRACE={}",
        json!({
            "schema": "MINIDORA_CORE_RELATION_COMPARISON_V1_V4_INVALIDATION_TRACE_V1",
            "os": std::env::consts::OS,
            "actor": actor,
            "comparison_case": comparison_case,
            "engine_reopened_after_invalidation": true,
            "invalidated_fact_ids": [v1_target_fact, v4_target_fact],
            "invalidation_result_ids": [v1_invalidation.id, v4_invalidation.id],
            "targets": [
                {
                    "label": saved[0].label,
                    "result_id": v1_target.id,
                    "selected_comparison_id": saved[0].result_id,
                    "lifecycle": v1_target.lifecycle,
                    "reason": v1_target.reason,
                    "content_plan_created": v1_target.content_plan.is_some(),
                    "render_created": v1_target.render.is_some(),
                },
                {
                    "label": saved[2].label,
                    "result_id": v4_target.id,
                    "selected_comparison_id": saved[2].result_id,
                    "lifecycle": v4_target.lifecycle,
                    "reason": v4_target.reason,
                    "content_plan_created": v4_target.content_plan.is_some(),
                    "render_created": v4_target.render.is_some(),
                },
            ],
            "controls": [
                {
                    "label": saved[1].label,
                    "result_id": v4_control.id,
                    "selected_comparison_id": saved[1].result_id,
                    "lifecycle": v4_control.lifecycle,
                    "render_status": v4_control.render.as_ref().map(|r| r.status.as_str()),
                    "render_sha256": v4_control.render.as_ref().map(|r| r.actual_sha256.as_str()),
                },
                {
                    "label": saved[3].label,
                    "result_id": v1_control.id,
                    "selected_comparison_id": saved[3].result_id,
                    "lifecycle": v1_control.lifecycle,
                    "render_status": v1_control.render.as_ref().map(|r| r.status.as_str()),
                    "render_sha256": v1_control.render.as_ref().map(|r| r.actual_sha256.as_str()),
                },
            ],
            "retained_artifacts": retained_artifacts,
            "outside_world_truth": "not checked",
            "migration_or_concurrency": "not checked",
        })
    );
}
