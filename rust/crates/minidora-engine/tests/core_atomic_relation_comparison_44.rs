use minidora_domain::{Lifecycle, RequestEnvelope, digest};
use minidora_engine::{Engine, store::Store};
use serde_json::{Value, json};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_TEMP_DIRECTORY: AtomicU64 = AtomicU64::new(0);

struct TestDirectory(PathBuf);

impl TestDirectory {
    fn new() -> Self {
        let serial = NEXT_TEMP_DIRECTORY.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "minidora-core-atomic-relation-comparison-44-{}-{serial}",
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

struct SavedComparison {
    version: &'static str,
    kind: &'static str,
    scope: &'static str,
    result_id: String,
    artifact_ref: String,
    artifact_sha256: String,
    proof_count: usize,
}

fn inventory_proof_ids(artifact: &Value) -> Vec<String> {
    let mut ids = Vec::new();
    for inventory in [&artifact["inventory_a"], &artifact["inventory_b"]] {
        for dependency in inventory["dependencies"]
            .as_array()
            .expect("inventory dependencies are an array")
        {
            ids.push(
                dependency
                    .as_str()
                    .expect("inventory dependency is an ID")
                    .to_owned(),
            );
        }
        for fact in inventory["facts"]
            .as_array()
            .expect("inventory facts are an array")
        {
            ids.push(
                fact["id"]
                    .as_str()
                    .expect("inventory fact ID is a string")
                    .to_owned(),
            );
            for dependency in fact["dependencies"]
                .as_array()
                .expect("fact dependencies are an array")
            {
                ids.push(
                    dependency
                        .as_str()
                        .expect("fact dependency is an ID")
                        .to_owned(),
                );
            }
        }
    }
    ids.sort();
    ids.dedup();
    ids
}

#[test]
fn changed_scope_stops_selected_comparison_without_affecting_other_version() {
    let directory = TestDirectory::new();
    let actor = "b44-scoped-comparison-invalidation";
    let comparison_case = "b44-coexisting-comparison-artifacts";
    let subjects = ("Aster", "Beryl");
    let target_scope = "b44-rule-proof-target";
    let control_scope = "b44-direct-proof-control";
    let mut engine = Engine::open(directory.path()).expect("open isolated Core state");

    commit_seed(
        &mut engine,
        actor,
        "b44-rule",
        "sensor-to-reliable",
        "If [scope=b44-rule-proof-target]X is a sensor then [scope=b44-rule-proof-target]X is reliable",
    );
    let changed_fact_id = commit_seed(
        &mut engine,
        actor,
        "b44-aster-target-fact",
        "aster-sensor",
        "[scope=b44-rule-proof-target]Aster is a sensor",
    );
    commit_seed(
        &mut engine,
        actor,
        "b44-beryl-target-fact",
        "beryl-sensor",
        "[scope=b44-rule-proof-target]Beryl is a sensor",
    );
    for (case, input, statement) in [
        (
            "b44-aster-control-fact",
            "aster-pilot",
            "[scope=b44-direct-proof-control]Aster is a pilot",
        ),
        (
            "b44-beryl-control-fact",
            "beryl-pilot",
            "[scope=b44-direct-proof-control]Beryl is a pilot",
        ),
    ] {
        commit_seed(&mut engine, actor, case, input, statement);
    }

    let store = Store::open(directory.path()).expect("open isolated artifact store");
    let comparison_specs = [
        (
            "v3-target",
            "b44-comparison-v3-target",
            target_scope,
            "minidora.relation-comparison-content.v3",
        ),
        (
            "v2-control",
            "b44-comparison-v2-control",
            control_scope,
            "minidora.relation-comparison-content.v2",
        ),
    ];
    let mut saved = Vec::new();
    for (version, input_id, scope, expected_kind) in comparison_specs {
        let mut request = RequestEnvelope::new(comparison_case, input_id, actor, "");
        request.budget.max_actions = 256;
        let compared = engine
            .compose_atomic_relation_comparison(actor, request, subjects.0, subjects.1, scope)
            .unwrap_or_else(|error| panic!("{version} comparison failed: {error}"));
        assert_eq!(
            compared.lifecycle,
            Lifecycle::Commit,
            "{version}: {compared:?}"
        );
        let plan = compared
            .content_plan
            .as_ref()
            .unwrap_or_else(|| panic!("{version} comparison has no adopted ContentPlan"));
        assert_eq!(plan.text, compared.text, "{version} plan text");
        let render = compared
            .render
            .as_ref()
            .unwrap_or_else(|| panic!("{version} comparison has no render observation"));
        assert_eq!(render.status, "RENDERED", "{version} render");
        assert_eq!(render.plan_id, plan.id, "{version} render-plan binding");
        assert_eq!(
            render.bytes,
            compared.text.as_bytes(),
            "{version} render bytes"
        );

        let artifact_ref = compared
            .dependencies
            .first()
            .unwrap_or_else(|| panic!("{version} result has no comparison artifact"));
        let artifact_bytes = store
            .get_bytes(actor, comparison_case, artifact_ref)
            .unwrap_or_else(|error| panic!("{version} artifact read failed: {error}"));
        let artifact: Value =
            serde_json::from_slice(&artifact_bytes).expect("parse persisted comparison artifact");
        assert_eq!(artifact["kind"], expected_kind, "{version} artifact kind");
        assert_eq!(
            artifact["result_id"], compared.id,
            "{version} result identity"
        );
        assert_eq!(artifact["scope"], scope, "{version} scope");
        let proofs = inventory_proof_ids(&artifact);
        assert!(!proofs.is_empty(), "{version} inventory proof IDs");
        if version == "v3-target" {
            assert!(proofs.contains(&changed_fact_id));
            assert!(proofs.iter().any(|id| id != &changed_fact_id));
        }
        saved.push(SavedComparison {
            version,
            kind: expected_kind,
            scope,
            result_id: compared.id,
            artifact_ref: artifact_ref.clone(),
            artifact_sha256: digest(&artifact_bytes),
            proof_count: proofs.len(),
        });
    }
    assert_ne!(saved[0].artifact_ref, saved[1].artifact_ref);
    for comparison in &saved {
        let bytes = store
            .get_bytes(actor, comparison_case, &comparison.artifact_ref)
            .unwrap_or_else(|error| panic!("{} was not co-readable: {error}", comparison.version));
        assert_eq!(
            digest(&bytes),
            comparison.artifact_sha256,
            "initial artifact hash"
        );
    }

    let invalidation = engine
        .execute(
            actor,
            RequestEnvelope::new(
                "b44-invalidation-case",
                "invalidate-target-premise",
                actor,
                &format!("invalidate {changed_fact_id}"),
            ),
        )
        .expect("invalidate one direct target-scope premise");
    assert_eq!(
        invalidation.lifecycle,
        Lifecycle::Commit,
        "{invalidation:?}"
    );
    assert!(
        invalidation.dependencies.contains(&changed_fact_id),
        "invalidation result omitted the selected proof ID: {invalidation:?}"
    );

    drop(store);
    drop(engine);
    let mut reopened = Engine::open(directory.path()).expect("reopen persisted Core state");

    let target = &saved[0];
    let mut target_request = RequestEnvelope::new(
        comparison_case,
        "b44-followup-v3-after-proof-change",
        actor,
        "なぜ",
    );
    target_request.budget.max_actions = 256;
    target_request.context_ref = Some(target.result_id.clone());
    let stopped = reopened
        .execute(actor, target_request)
        .expect("replay selected comparison after proof invalidation");
    assert_eq!(stopped.lifecycle, Lifecycle::Suspend, "{stopped:?}");
    assert!(
        stopped
            .reason
            .as_deref()
            .is_some_and(|reason| reason.starts_with("CONTEXT_PROOF_CHANGED:")),
        "target follow-up did not report CONTEXT_PROOF_CHANGED: {stopped:?}"
    );
    assert!(
        stopped.content_plan.is_none(),
        "stale proof was adopted: {stopped:?}"
    );
    assert!(
        stopped.render.is_none(),
        "stale proof was rendered: {stopped:?}"
    );

    let control = &saved[1];
    let mut control_request = RequestEnvelope::new(
        comparison_case,
        "b44-followup-v2-unaffected-control",
        actor,
        "なぜ",
    );
    control_request.budget.max_actions = 256;
    control_request.context_ref = Some(control.result_id.clone());
    let followed = reopened
        .execute(actor, control_request)
        .expect("follow up unaffected comparison from another saved version");
    assert_eq!(followed.lifecycle, Lifecycle::Commit, "{followed:?}");
    assert!(
        followed.dependencies.contains(&control.result_id),
        "control follow-up did not consume the selected V2 result: {followed:?}"
    );
    assert!(
        followed
            .text
            .contains(&format!("比較結果ID: {}", control.result_id)),
        "control follow-up omitted its selected result ID: {followed:?}"
    );
    assert!(followed.content_plan.is_some());
    let render = followed
        .render
        .as_ref()
        .expect("unaffected follow-up has a render observation");
    assert_eq!(render.status, "RENDERED");

    let store = Store::open(directory.path()).expect("reopen comparison artifact store");
    let retained_artifacts = saved
        .iter()
        .map(|comparison| {
            let bytes = store
                .get_bytes(actor, comparison_case, &comparison.artifact_ref)
                .unwrap_or_else(|error| {
                    panic!("{} artifact was not retained: {error}", comparison.version)
                });
            assert_eq!(digest(&bytes), comparison.artifact_sha256);
            json!({
                "version": comparison.version,
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
    assert_eq!(retained_artifacts.len(), 2);

    println!(
        "MINIDORA_B44_TRACE={}",
        json!({
            "schema": "MINIDORA_CORE_RELATION_COMPARISON_SCOPED_INVALIDATION_TRACE_V1",
            "os": std::env::consts::OS,
            "actor": actor,
            "comparison_case": comparison_case,
            "target_scope": target_scope,
            "control_scope": control_scope,
            "engine_reopened_after_invalidation": true,
            "invalidated_fact_id": changed_fact_id,
            "invalidation_result_id": invalidation.id,
            "target_followup": {
                "selected_result_id": target.result_id,
                "result_id": stopped.id,
                "lifecycle": stopped.lifecycle,
                "reason": stopped.reason,
                "content_plan_created": stopped.content_plan.is_some(),
                "render_created": stopped.render.is_some(),
            },
            "unaffected_control_followup": {
                "selected_result_id": control.result_id,
                "result_id": followed.id,
                "lifecycle": followed.lifecycle,
                "render_status": render.status,
                "render_sha256": render.actual_sha256,
            },
            "retained_artifacts": retained_artifacts,
            "outside_world_truth": "not checked",
            "migration_or_concurrency": "not checked",
        })
    );
}
