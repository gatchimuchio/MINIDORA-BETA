use minidora_domain::{Lifecycle, RequestEnvelope};
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
            "minidora-core-atomic-relation-comparison-38-{}-{serial}",
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
        .execute(
            actor,
            RequestEnvelope::new(case, "b38-comparison", actor, text),
        )
        .expect("seed one actor-memory relation");
    assert_eq!(result.lifecycle, Lifecycle::Commit, "{result:?}");
    result
        .dependencies
        .first()
        .expect("seed result exposes its adopted fact ID")
        .clone()
}

fn request(case: &str, actor: &str) -> RequestEnvelope {
    RequestEnvelope::new(case, "b38-comparison", actor, "")
}

#[test]
fn proof_derived_comparison_content_is_adopted_replayed_and_recovered() {
    let directory = TestDirectory::new();
    let actor = "b38-atomic-relation-comparison";
    let scope = "b38-comparison-lab";
    let mut engine = Engine::open(directory.path()).expect("open isolated Core state");

    let aster_pilot = seed(
        &mut engine,
        actor,
        "b38-comparison-aster-pilot",
        "[scope=b38-comparison-lab]Aster is a pilot",
    );
    let aster_musician = seed(
        &mut engine,
        actor,
        "b38-comparison-aster-musician",
        "[scope=b38-comparison-lab]Aster is a musician",
    );
    let beryl_pilot = seed(
        &mut engine,
        actor,
        "b38-comparison-beryl-pilot",
        "[scope=b38-comparison-lab]Beryl is a pilot",
    );
    let beryl_geologist = seed(
        &mut engine,
        actor,
        "b38-comparison-beryl-geologist",
        "[scope=b38-comparison-lab]Beryl is a geologist",
    );

    let compared = engine
        .compose_atomic_relation_comparison(
            actor,
            request("b38-comparison-supported", actor),
            "Aster",
            "Beryl",
            scope,
        )
        .expect("execute typed atomic relation comparison");
    assert_eq!(compared.lifecycle, Lifecycle::Commit, "{compared:?}");
    assert!(
        compared
            .text
            .starts_with("左右の保存関係根拠を照合した比較です。")
    );
    assert!(compared.text.contains("両側で確認した同じ関係"));
    assert!(compared.text.contains("Aster"));
    assert!(compared.text.contains("Beryl"));
    assert!(compared.text.contains("pilot"));
    assert!(compared.text.contains("musician"));
    assert!(compared.text.contains("geologist"));
    let plan = compared
        .content_plan
        .as_ref()
        .expect("Core adopted the comparison content plan");
    assert!(plan.required_units.len() >= 8);
    assert_eq!(plan.text, compared.text);
    let render = compared
        .render
        .as_ref()
        .expect("K7 returned the actual render observation");
    assert_eq!(render.status, "RENDERED");
    assert_eq!(render.plan_id, plan.id);
    assert_eq!(render.bytes, plan.text.as_bytes());

    let bundle = engine
        .latest(actor, "b38-comparison-supported")
        .expect("read supported comparison case")
        .expect("supported comparison case exists")
        .bundle
        .expect("typed comparison request was sealed in Core");
    let report_ref = compared
        .dependencies
        .first()
        .expect("result retains the comparison content artifact");
    let artifact_bytes = Store::open(directory.path())
        .expect("reopen isolated artifact store")
        .get_bytes(actor, &bundle.request.case_id, report_ref)
        .expect("read retained comparison content artifact");
    let artifact: Value =
        serde_json::from_slice(&artifact_bytes).expect("parse comparison content artifact");
    assert_eq!(artifact["kind"], "minidora.relation-comparison-content.v2");
    assert_eq!(artifact["content_request"]["purpose"], "COMPARE");
    assert_eq!(artifact["content_composition"]["text"], compared.text);
    let roles = artifact["content_request"]["units"]
        .as_array()
        .expect("Core content units")
        .iter()
        .filter_map(|unit| unit["role"].as_str())
        .collect::<Vec<_>>();
    for required_role in [
        "OPENING",
        "SUBJECT_A",
        "SUBJECT_B",
        "COMMONALITY",
        "DIFFERENCE",
        "EVIDENCE",
        "LIMITATION",
    ] {
        assert!(
            roles.contains(&required_role),
            "missing role {required_role}"
        );
    }
    let proof_ids = artifact["content_composition"]["evidence_ids"]
        .as_array()
        .expect("composition retains proof IDs")
        .iter()
        .filter_map(Value::as_str)
        .map(str::to_owned)
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(
        proof_ids,
        [
            &aster_pilot,
            &aster_musician,
            &beryl_pilot,
            &beryl_geologist
        ]
        .into_iter()
        .cloned()
        .collect()
    );
    assert_eq!(
        artifact["report_sha256"],
        minidora_domain::digest(compared.text.as_bytes())
    );

    let coda_pilot = seed(
        &mut engine,
        actor,
        "b38-conflict-coda-pilot",
        "[scope=b38-conflict-lab]Coda is a pilot",
    );
    let coda_not_pilot = seed(
        &mut engine,
        actor,
        "b38-conflict-coda-not-pilot",
        "[scope=b38-conflict-lab]Coda is not a pilot",
    );
    let dara_pilot = seed(
        &mut engine,
        actor,
        "b38-conflict-dara-pilot",
        "[scope=b38-conflict-lab]Dara is a pilot",
    );
    let conflict = engine
        .compose_atomic_relation_comparison(
            actor,
            request("b38-comparison-conflict", actor),
            "Coda",
            "Dara",
            "b38-conflict-lab",
        )
        .expect("execute comparison with competing saved premises");
    assert_eq!(conflict.lifecycle, Lifecycle::Commit, "{conflict:?}");
    assert!(conflict.text.contains("競合根拠を含む関係"));
    assert!(conflict.text.contains("共通・片側のみと確定しません"));
    assert!(!conflict.text.contains("両側で確認した同じ関係"));
    let conflict_plan = conflict
        .content_plan
        .as_ref()
        .expect("Core adopted conflict-aware comparison content");
    assert_eq!(
        conflict_plan.text.as_bytes(),
        conflict.render.as_ref().unwrap().bytes
    );
    let conflict_case = engine
        .latest(actor, "b38-comparison-conflict")
        .expect("read competing comparison case")
        .expect("competing comparison case exists");
    let conflict_bundle = conflict_case.bundle.expect("sealed comparison meaning");
    let conflict_ref = conflict
        .dependencies
        .first()
        .expect("comparison artifact ref");
    let conflict_bytes = Store::open(directory.path())
        .expect("open conflict artifact store")
        .get_bytes(actor, &conflict_bundle.request.case_id, conflict_ref)
        .expect("read conflict-aware comparison artifact");
    let conflict_artifact: Value =
        serde_json::from_slice(&conflict_bytes).expect("parse conflict-aware artifact");
    assert_eq!(conflict_artifact["inventory_a"]["has_conflict"], true);
    let conflict_proofs = conflict_artifact["content_composition"]["evidence_ids"]
        .as_array()
        .expect("conflicting proof IDs retained")
        .iter()
        .filter_map(Value::as_str)
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(
        conflict_proofs,
        [&coda_pilot, &coda_not_pilot, &dara_pilot]
            .into_iter()
            .map(String::as_str)
            .collect()
    );

    drop(engine);
    let mut reopened = Engine::open(directory.path()).expect("reopen persisted Core state");
    let mut explain_request = RequestEnvelope::new(
        "b38-comparison-supported",
        "b38-comparison-explain",
        actor,
        "なぜ",
    );
    explain_request.context_ref = Some(compared.id.clone());
    let explained = reopened
        .execute(actor, explain_request)
        .expect("revalidate prior typed comparison through normal Core execution");
    assert_eq!(explained.lifecycle, Lifecycle::Commit, "{explained:?}");
    assert!(explained.dependencies.contains(&compared.id));
    assert!(explained.text.contains("比較結果ID"));
    assert!(explained.render.is_some());

    let legacy = reopened
        .compose_atomic_relation_comparison(
            actor,
            request("b38-comparison-legacy", actor),
            "Aster.",
            "Beryl",
            scope,
        )
        .expect("execute bounded legacy fallback comparison");
    assert_eq!(legacy.lifecycle, Lifecycle::Commit, "{legacy:?}");
    assert!(legacy.text.starts_with(
        "保存済みの根拠を比較した記録です。片側のみの表示はその側の保存記録で支持され、他方では同じ関係の支持を確認できなかった意味です。保存記録の網羅性と現実世界の真偽は確認していません。"
    ));
    let legacy_report_ref = legacy
        .dependencies
        .first()
        .expect("legacy result retains its report artifact");
    let legacy_report_bytes = Store::open(directory.path())
        .expect("reopen isolated artifact store")
        .get_bytes(actor, "b38-comparison-legacy", legacy_report_ref)
        .expect("read retained version 1 report artifact");
    let legacy_report: Value =
        serde_json::from_slice(&legacy_report_bytes).expect("parse legacy report artifact");
    assert_eq!(
        legacy_report["kind"],
        "minidora.relation-comparison-report.v1"
    );

    drop(reopened);
    let mut legacy_reopened = Engine::open(directory.path()).expect("reopen version 1 comparison");
    let mut legacy_explain_request = RequestEnvelope::new(
        "b38-comparison-legacy",
        "b38-comparison-legacy-explain",
        actor,
        "なぜ",
    );
    legacy_explain_request.context_ref = Some(legacy.id.clone());
    let legacy_explained = legacy_reopened
        .execute(actor, legacy_explain_request)
        .expect("read and revalidate the prior version 1 artifact");
    assert_eq!(
        legacy_explained.lifecycle,
        Lifecycle::Commit,
        "{legacy_explained:?}"
    );
    assert!(legacy_explained.dependencies.contains(&legacy.id));

    println!(
        "MINIDORA_B38_TRACE={}",
        json!({
            "schema": "MINIDORA_CORE_ATOMIC_RELATION_COMPARISON_RUNTIME_TRACE_V2",
            "os": std::env::consts::OS,
            "producer": "relation_comparison_inventory -> Core typed Compare units -> MeaningCompositionReport",
            "subjects": ["Aster", "Beryl"],
            "scope": scope,
            "seed_proofs": [aster_pilot, aster_musician, beryl_pilot, beryl_geologist],
            "comparison": {
                "result_id": &compared.id,
                "lifecycle": &compared.lifecycle,
                "text": &compared.text,
                "artifact_sha256": minidora_domain::digest(&artifact_bytes),
                "content_units": plan.required_units,
                "proof_ids": proof_ids,
                "render_sha256": &render.actual_sha256,
                "render_bytes": render.bytes.len(),
            },
            "conflicting_comparison": {
                "result_id": &conflict.id,
                "lifecycle": &conflict.lifecycle,
                "text": &conflict.text,
                "proof_ids": conflict_proofs,
                "has_conflict": conflict_artifact["inventory_a"]["has_conflict"],
            },
            "reopened_context": {
                "result_id": &explained.id,
                "lifecycle": &explained.lifecycle,
                "prior_comparison_consumed": explained.dependencies.contains(&compared.id),
                "rendered": explained.render.is_some(),
            },
            "legacy_artifact_reader": {
                "kind": legacy_report["kind"].as_str(),
                "reopened_followup_lifecycle": legacy_explained.lifecycle,
                "prior_comparison_consumed": legacy_explained.dependencies.contains(&legacy.id),
            },
            "outside_world_truth": "not checked",
        })
    );
}
