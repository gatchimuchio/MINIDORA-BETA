use minidora_domain::{Intent, Lifecycle, RequestEnvelope};
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
            "minidora-core-atomic-relation-comparison-36-{}-{serial}",
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

fn seed(engine: &mut Engine, actor: &str, case: &str, text: &str) -> String {
    let result = engine
        .execute(actor, RequestEnvelope::new(case, case, actor, text))
        .expect("seed one actor-memory relation");
    assert_eq!(result.lifecycle, Lifecycle::Commit, "{result:?}");
    result
        .dependencies
        .first()
        .expect("seed result exposes its adopted fact ID")
        .clone()
}

fn request(case: &str, actor: &str) -> RequestEnvelope {
    RequestEnvelope::new(case, "typed-relation-comparison", actor, "")
}

#[test]
fn typed_atomic_relation_comparison_reaches_k7_and_preserves_stops() {
    let directory = TestDirectory::new();
    let actor = "b36-atomic-relation-comparison";
    let scope = "atomic-relation-comparison-lab";
    let mut engine = Engine::open(directory.path()).expect("open isolated Core state");

    let aster_pilot = seed(
        &mut engine,
        actor,
        "comparison-aster-pilot",
        "[scope=atomic-relation-comparison-lab]Aster is a pilot",
    );
    let aster_musician = seed(
        &mut engine,
        actor,
        "comparison-aster-musician",
        "[scope=atomic-relation-comparison-lab]Aster is a musician",
    );
    let beryl_pilot = seed(
        &mut engine,
        actor,
        "comparison-beryl-pilot",
        "[scope=atomic-relation-comparison-lab]Beryl is a pilot",
    );
    let beryl_geologist = seed(
        &mut engine,
        actor,
        "comparison-beryl-geologist",
        "[scope=atomic-relation-comparison-lab]Beryl is a geologist",
    );

    let compared = engine
        .compose_atomic_relation_comparison(
            actor,
            request("comparison-supported", actor),
            "Aster",
            "Beryl",
            scope,
        )
        .expect("execute typed atomic relation comparison");
    assert_eq!(compared.lifecycle, Lifecycle::Commit, "{compared:?}");
    assert_eq!(compared.guarantee, minidora_domain::Guarantee::Hypothesis);
    assert!(compared.text.contains("両側で確認した同じ関係"));
    assert!(compared.text.contains("Aster"));
    assert!(compared.text.contains("Beryl"));
    assert!(compared.text.contains("pilot"));
    assert!(compared.text.contains("musician"));
    assert!(compared.text.contains("geologist"));

    let plan = compared
        .content_plan
        .as_ref()
        .expect("Core adopted a generated comparison content plan");
    assert_eq!(plan.guarantee, minidora_domain::Guarantee::Hypothesis);
    assert!(!plan.required_units.is_empty());
    let render = compared
        .render
        .as_ref()
        .expect("K7 produced an actual render observation");
    assert_eq!(render.status, "RENDERED");
    assert_eq!(render.plan_id, plan.id);
    assert_eq!(render.bytes, plan.text.as_bytes());
    assert!(!render.actual_sha256.is_empty());

    let compared_case = engine
        .latest(actor, "comparison-supported")
        .expect("read supported comparison case")
        .expect("supported comparison case exists");
    let bundle = compared_case
        .bundle
        .as_ref()
        .expect("typed comparison request was sealed in Core");
    assert_eq!(
        bundle.meaning.source_kind,
        "CORE_TYPED_RELATION_COMPARISON_V1"
    );
    assert_eq!(
        bundle.meaning.parser_version,
        "core-typed-relation-comparison-v1"
    );
    assert_eq!(
        bundle.meaning.intent,
        Intent::CompareRelationInventories {
            subject_a: "Aster".into(),
            subject_b: "Beryl".into(),
            scope: scope.into(),
        }
    );
    let direct_input: Value =
        serde_json::from_str(&bundle.request.text).expect("sealed typed comparison JSON");
    assert_eq!(direct_input["purpose"], "compare-relations");
    let mut expected_a = vec![aster_pilot.clone(), aster_musician.clone()];
    expected_a.sort();
    let mut expected_b = vec![beryl_pilot.clone(), beryl_geologist.clone()];
    expected_b.sort();
    assert_eq!(direct_input["source_ids_a"], json!(expected_a));
    assert_eq!(direct_input["source_ids_b"], json!(expected_b));

    let report_ref = compared
        .dependencies
        .first()
        .expect("comparison result retains its report artifact reference");
    let report_bytes = Store::open(directory.path())
        .expect("reopen isolated artifact store")
        .get_bytes(actor, "comparison-supported", report_ref)
        .expect("read retained comparison artifact");
    let report_artifact: Value =
        serde_json::from_slice(&report_bytes).expect("parse comparison report artifact");
    assert_eq!(
        report_artifact["kind"],
        "minidora.relation-comparison-report.v1"
    );
    assert_eq!(report_artifact["subject_a"], "Aster");
    assert_eq!(report_artifact["subject_b"], "Beryl");
    assert_eq!(report_artifact["scope"], scope);
    assert_eq!(
        report_artifact["inventory_a"]["facts"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    assert_eq!(
        report_artifact["inventory_b"]["facts"]
            .as_array()
            .unwrap()
            .len(),
        2
    );

    let missing = engine
        .compose_atomic_relation_comparison(
            actor,
            request("comparison-missing", actor),
            "Cato",
            "Dara",
            scope,
        )
        .expect("missing comparison returns a terminal Core result");
    assert_eq!(missing.lifecycle, Lifecycle::Suspend, "{missing:?}");
    assert!(
        missing
            .reason
            .as_deref()
            .unwrap_or_default()
            .contains("MISSING_PREMISES")
    );
    assert!(missing.content_plan.is_none());
    assert!(missing.render.is_none());

    let forbidden_path = directory.path().join("unauthorized-comparison.txt");
    let mut denied_request = request("comparison-denied-write", actor);
    denied_request.output_path = Some(forbidden_path.clone());
    let denied = engine
        .compose_atomic_relation_comparison(actor, denied_request, "Aster", "Beryl", scope)
        .expect("unauthorized comparison write returns a terminal Core result");
    assert_eq!(denied.lifecycle, Lifecycle::Suspend, "{denied:?}");
    assert!(
        denied
            .reason
            .as_deref()
            .unwrap_or_default()
            .contains("WRITE_PERMISSION_MISSING")
    );
    assert!(denied.receipts.is_empty());
    assert!(!forbidden_path.exists());
    let actions = Store::open(directory.path())
        .expect("open isolated action ledger")
        .actions(actor, "comparison-denied-write")
        .expect("read denied comparison action ledger");
    assert!(actions.is_empty());

    println!(
        "MINIDORA_B36_TRACE={}",
        json!({
            "schema": "MINIDORA_CORE_ATOMIC_RELATION_COMPARISON_RUNTIME_TRACE_V1",
            "os": std::env::consts::OS,
            "producer": "Engine::compose_atomic_relation_comparison",
            "seed_proofs": {
                "aster_pilot": aster_pilot,
                "aster_musician": aster_musician,
                "beryl_pilot": beryl_pilot,
                "beryl_geologist": beryl_geologist,
            },
            "supported": {
                "case_id": &bundle.request.case_id,
                "sealed_request_text": &bundle.request.text,
                "typed_input": &direct_input,
                "source_kind": &bundle.meaning.source_kind,
                "parser_version": &bundle.meaning.parser_version,
                "semantic_signature": &bundle.semantic_signature,
                "result": {
                    "id": &compared.id,
                    "lifecycle": &compared.lifecycle,
                    "guarantee": &compared.guarantee,
                    "text": &compared.text,
                    "dependencies": &compared.dependencies,
                },
                "artifact": report_artifact,
                "content_plan": {
                    "id": &plan.id,
                    "text": &plan.text,
                    "required_units": &plan.required_units,
                    "dependencies": &plan.dependencies,
                    "model_revision": &plan.model_revision,
                },
                "render": {
                    "plan_id": &render.plan_id,
                    "status": &render.status,
                    "actual_sha256": &render.actual_sha256,
                    "byte_length": render.bytes.len(),
                    "utf8_text": String::from_utf8(render.bytes.clone()).expect("render is UTF-8"),
                },
            },
            "missing_evidence": {
                "case_id": "comparison-missing",
                "result_id": missing.id,
                "lifecycle": missing.lifecycle,
                "reason": missing.reason,
                "content_plan_adopted": missing.content_plan.is_some(),
                "render_created": missing.render.is_some(),
            },
            "unauthorized_write": {
                "case_id": "comparison-denied-write",
                "result_id": denied.id,
                "lifecycle": denied.lifecycle,
                "reason": denied.reason,
                "receipts": denied.receipts,
                "action_count": actions.len(),
                "target_created": forbidden_path.exists(),
            },
        })
    );
}
