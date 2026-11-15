use minidora_domain::{Lifecycle, RequestEnvelope};
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
            "minidora-core-atomic-relation-comparison-43-{}-{serial}",
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

fn execute(engine: &mut Engine, actor: &str, case: &str, input: &str, text: &str) -> String {
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

fn ids(values: &Value) -> BTreeSet<String> {
    values
        .as_array()
        .expect("artifact field is an array")
        .iter()
        .map(|value| value.as_str().expect("artifact ID is a string").to_owned())
        .collect()
}

fn inventory_proofs(artifact: &Value) -> BTreeSet<String> {
    [&artifact["inventory_a"], &artifact["inventory_b"]]
        .into_iter()
        .fold(BTreeSet::new(), |mut all, inventory| {
            all.extend(ids(&inventory["dependencies"]));
            for fact in inventory["facts"].as_array().expect("inventory facts") {
                all.insert(fact["id"].as_str().expect("fact ID").to_owned());
                all.extend(ids(&fact["dependencies"]));
            }
            all
        })
}

struct SavedComparison {
    label: &'static str,
    expected_kind: &'static str,
    result_id: String,
    artifact_ref: String,
    artifact_sha256: String,
    proof_count: usize,
    content_plan_id: String,
    render_sha256: String,
    render_bytes: usize,
}

#[test]
fn all_comparison_artifact_versions_coexist_and_replay_after_reopen() {
    let directory = TestDirectory::new();
    let actor = "b43-comparison-version-coexistence";
    let comparison_case = "b43-comparison-version-coexistence";
    let subjects = ("Aster", "Beryl");
    let mut engine = Engine::open(directory.path()).expect("open isolated Core state");

    let v2_scope = "b43-v2-direct-lab";
    for (case, text) in [
        (
            "b43-v2-aster-pilot",
            "[scope=b43-v2-direct-lab]Aster is a pilot",
        ),
        (
            "b43-v2-aster-musician",
            "[scope=b43-v2-direct-lab]Aster is a musician",
        ),
        (
            "b43-v2-beryl-pilot",
            "[scope=b43-v2-direct-lab]Beryl is a pilot",
        ),
        (
            "b43-v2-beryl-geologist",
            "[scope=b43-v2-direct-lab]Beryl is a geologist",
        ),
    ] {
        execute(&mut engine, actor, case, "seed", text);
    }

    let v3_scope = "b43-v3-rule-lab";
    execute(
        &mut engine,
        actor,
        "b43-v3-rule",
        "sensor-to-reliable",
        "If [scope=b43-v3-rule-lab]X is a sensor then [scope=b43-v3-rule-lab]X is reliable",
    );
    for (case, subject) in [("b43-v3-aster", "Aster"), ("b43-v3-beryl", "Beryl")] {
        execute(
            &mut engine,
            actor,
            case,
            "seed-sensor",
            &format!("[scope={v3_scope}]{subject} is a sensor"),
        );
    }

    let v4_scope = "b43-v4-chain-lab";
    execute(
        &mut engine,
        actor,
        "b43-v4-rule-one",
        "sensor-to-inspected",
        "If [scope=b43-v4-chain-lab]X is a sensor then [scope=b43-v4-chain-lab]X is inspected",
    );
    execute(
        &mut engine,
        actor,
        "b43-v4-rule-two",
        "inspected-to-reliable",
        "If [scope=b43-v4-chain-lab]X is inspected then [scope=b43-v4-chain-lab]X is reliable",
    );
    for (case, subject) in [("b43-v4-aster", "Aster"), ("b43-v4-beryl", "Beryl")] {
        execute(
            &mut engine,
            actor,
            case,
            "seed-sensor",
            &format!("[scope={v4_scope}]{subject} is a sensor"),
        );
    }

    let v1_scope = "b43-v1-overflow-lab";
    execute(
        &mut engine,
        actor,
        "b43-v1-rule",
        "sensor-to-reliable",
        "If [scope=b43-v1-overflow-lab]X is a sensor then [scope=b43-v1-overflow-lab]X is reliable",
    );
    for (case, text) in [
        (
            "b43-v1-aster-sensor",
            "[scope=b43-v1-overflow-lab]Aster is a sensor",
        ),
        (
            "b43-v1-aster-musician",
            "[scope=b43-v1-overflow-lab]Aster is a musician",
        ),
        (
            "b43-v1-aster-pilot",
            "[scope=b43-v1-overflow-lab]Aster is a pilot",
        ),
        (
            "b43-v1-beryl-sensor",
            "[scope=b43-v1-overflow-lab]Beryl is a sensor",
        ),
        (
            "b43-v1-beryl-pilot",
            "[scope=b43-v1-overflow-lab]Beryl is a pilot",
        ),
        (
            "b43-v1-beryl-geologist",
            "[scope=b43-v1-overflow-lab]Beryl is a geologist",
        ),
    ] {
        execute(&mut engine, actor, case, "seed-relation", text);
    }

    let version_specs = [
        (
            "v2",
            "b43-comparison-v2",
            v2_scope,
            "minidora.relation-comparison-content.v2",
        ),
        (
            "v3",
            "b43-comparison-v3",
            v3_scope,
            "minidora.relation-comparison-content.v3",
        ),
        (
            "v4",
            "b43-comparison-v4",
            v4_scope,
            "minidora.relation-comparison-content.v4",
        ),
        (
            "v1",
            "b43-comparison-v1",
            v1_scope,
            "minidora.relation-comparison-report.v1",
        ),
    ];
    let store = Store::open(directory.path()).expect("open isolated artifact store");
    let mut saved = Vec::new();
    for (label, input_id, scope, expected_kind) in version_specs {
        let mut request = RequestEnvelope::new(comparison_case, input_id, actor, "");
        request.budget.max_actions = 256;
        let compared = engine
            .compose_atomic_relation_comparison(actor, request, subjects.0, subjects.1, scope)
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
        assert_eq!(render.plan_id, plan.id, "{label} render-plan binding");
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
            .get_bytes(actor, comparison_case, artifact_ref)
            .unwrap_or_else(|error| panic!("{label} artifact read failed: {error}"));
        let artifact: Value =
            serde_json::from_slice(&artifact_bytes).expect("parse persisted comparison artifact");
        assert_eq!(artifact["kind"], expected_kind, "{label} artifact kind");
        assert_eq!(
            artifact["result_id"], compared.id,
            "{label} result identity"
        );
        assert_eq!(
            artifact["report_sha256"],
            minidora_domain::digest(compared.text.as_bytes()),
            "{label} report digest"
        );
        if label == "v1" {
            assert!(artifact.get("content_request").is_none());
            assert!(artifact.get("content_composition").is_none());
        } else {
            assert!(artifact.get("content_request").is_some());
            assert!(artifact.get("content_composition").is_some());
        }
        let proofs = inventory_proofs(&artifact);
        assert!(!proofs.is_empty(), "{label} inventory proofs");
        if label != "v1" {
            assert_eq!(
                ids(&artifact["content_composition"]["evidence_ids"]),
                proofs,
                "{label} content retains every inventory proof"
            );
        }
        assert_eq!(artifact["subject_a"], subjects.0, "{label} left subject");
        assert_eq!(artifact["subject_b"], subjects.1, "{label} right subject");
        saved.push(SavedComparison {
            label,
            expected_kind,
            result_id: compared.id,
            artifact_ref: artifact_ref.clone(),
            artifact_sha256: minidora_domain::digest(&artifact_bytes),
            proof_count: proofs.len(),
            content_plan_id: plan.id.clone(),
            render_sha256: render.actual_sha256.clone(),
            render_bytes: render.bytes.len(),
        });
    }
    let artifact_refs = saved
        .iter()
        .map(|item| item.artifact_ref.as_str())
        .collect::<BTreeSet<_>>();
    assert_eq!(
        artifact_refs.len(),
        4,
        "all versions coexist as separate blobs"
    );
    let mut simultaneously_available_refs = BTreeSet::new();
    for comparison in &saved {
        let bytes = store
            .get_bytes(actor, comparison_case, &comparison.artifact_ref)
            .unwrap_or_else(|error| {
                panic!(
                    "{} artifact was not available alongside the other versions: {error}",
                    comparison.label
                )
            });
        assert_eq!(
            minidora_domain::digest(&bytes),
            comparison.artifact_sha256,
            "{} coexisting artifact hash",
            comparison.label
        );
        simultaneously_available_refs.insert(comparison.artifact_ref.as_str());
    }
    assert_eq!(simultaneously_available_refs, artifact_refs);

    drop(store);
    drop(engine);
    let mut reopened = Engine::open(directory.path()).expect("reopen persisted Core state");
    let mut followup_results = Vec::new();
    for comparison in &saved {
        let input_id = format!("b43-followup-{}", comparison.label);
        let mut request = RequestEnvelope::new(comparison_case, &input_id, actor, "なぜ");
        request.budget.max_actions = 256;
        request.context_ref = Some(comparison.result_id.clone());
        let followup = reopened
            .execute(actor, request)
            .unwrap_or_else(|error| panic!("{} replay failed: {error}", comparison.label));
        assert_eq!(
            followup.lifecycle,
            Lifecycle::Commit,
            "{} follow-up: {followup:?}",
            comparison.label
        );
        assert!(
            followup.dependencies.contains(&comparison.result_id),
            "{} follow-up did not consume its selected result",
            comparison.label
        );
        assert!(
            followup
                .text
                .contains(&format!("比較結果ID: {}", comparison.result_id)),
            "{} follow-up omitted its selected comparison ID",
            comparison.label
        );
        assert!(followup.content_plan.is_some());
        let followup_render = followup
            .render
            .as_ref()
            .expect("replayed follow-up has a render observation");
        assert_eq!(followup_render.status, "RENDERED");
        followup_results.push(json!({
            "version": comparison.label,
            "selected_result_id": comparison.result_id,
            "followup_result_id": followup.id,
            "selected_prior_consumed": followup.dependencies.contains(&comparison.result_id),
            "lifecycle": followup.lifecycle,
            "render_status": followup_render.status,
            "render_sha256": followup_render.actual_sha256,
        }));
    }

    println!(
        "MINIDORA_B43_TRACE={}",
        json!({
            "schema": "MINIDORA_CORE_RELATION_COMPARISON_VERSION_COEXISTENCE_TRACE_V1",
            "os": std::env::consts::OS,
            "actor": actor,
            "case": comparison_case,
            "coexisting_artifact_count": saved.len(),
            "distinct_artifact_refs": artifact_refs.len(),
            "engine_reopened_before_replay": true,
            "comparisons": saved.iter().map(|item| json!({
                "version": item.label,
                "kind": item.expected_kind,
                "result_id": item.result_id,
                "artifact_ref": item.artifact_ref,
                "artifact_sha256": item.artifact_sha256,
                "proof_count": item.proof_count,
                "content_plan_id": item.content_plan_id,
                "render_sha256": item.render_sha256,
                "render_bytes": item.render_bytes,
            })).collect::<Vec<_>>(),
            "reopened_followups": followup_results,
            "outside_world_truth": "not checked",
        })
    );
}
