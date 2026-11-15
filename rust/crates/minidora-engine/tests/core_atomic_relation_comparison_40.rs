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
            "minidora-core-atomic-relation-comparison-40-{}-{serial}",
            std::process::id()
        ));
        fs::create_dir(&path).expect("create isolated derived relation-comparison state");
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

fn comparison_request(case: &str, actor: &str) -> RequestEnvelope {
    RequestEnvelope::new(case, "b40-comparison", actor, "")
}

fn ids(values: &Value) -> BTreeSet<String> {
    values
        .as_array()
        .expect("artifact field is an array")
        .iter()
        .filter_map(Value::as_str)
        .map(str::to_owned)
        .collect()
}

#[test]
fn derived_comparison_v3_replays_rules_renders_and_survives_reopen() {
    let directory = TestDirectory::new();
    let actor = "b40-derived-atomic-relation-comparison";
    let scope = "b40-derived-comparison-lab";
    let comparison_case = "b40-comparison-derived";
    let mut engine = Engine::open(directory.path()).expect("open isolated Core state");

    let first_rule = execute(
        &mut engine,
        actor,
        "b40-rule-one",
        "sensor-to-reliable",
        "If [scope=b40-derived-comparison-lab]X is a sensor then [scope=b40-derived-comparison-lab]X is reliable",
    );
    let aster_premise = execute(
        &mut engine,
        actor,
        "b40-aster-premise",
        "aster-sensor",
        "[scope=b40-derived-comparison-lab]Aster is a sensor",
    );
    let beryl_premise = execute(
        &mut engine,
        actor,
        "b40-beryl-premise",
        "beryl-sensor",
        "[scope=b40-derived-comparison-lab]Beryl is a sensor",
    );

    let compared = engine
        .compose_atomic_relation_comparison(
            actor,
            comparison_request(comparison_case, actor),
            "Aster",
            "Beryl",
            scope,
        )
        .expect("execute typed comparison over rule-derived inventories");
    assert_eq!(compared.lifecycle, Lifecycle::Commit, "{compared:?}");
    assert!(
        compared
            .text
            .contains("左右の保存関係根拠を照合した比較です。"),
        "missing the opening unit from composed comparison text: {}",
        compared.text
    );
    assert!(compared.text.contains("Aster"));
    assert!(compared.text.contains("Beryl"));
    assert!(compared.text.contains("reliable"));
    assert!(compared.text.contains("現実世界の真偽も確認していません"));

    let plan = compared
        .content_plan
        .as_ref()
        .expect("Core adopted the rule-bearing comparison ContentPlan");
    assert_eq!(plan.text, compared.text);
    let render = compared
        .render
        .as_ref()
        .expect("K7 returned the actual render observation");
    assert_eq!(render.status, "RENDERED");
    assert_eq!(render.plan_id, plan.id);
    assert_eq!(render.bytes, plan.text.as_bytes());
    assert_eq!(render.actual_sha256, minidora_domain::digest(&render.bytes));

    let bundle = engine
        .latest(actor, comparison_case)
        .expect("read derived comparison case")
        .expect("derived comparison case exists")
        .bundle
        .expect("typed comparison request was sealed in Core");
    let artifact_ref = compared
        .dependencies
        .first()
        .expect("comparison result retains its artifact reference");
    let artifact_bytes = Store::open(directory.path())
        .expect("open isolated artifact store")
        .get_bytes(actor, &bundle.request.case_id, artifact_ref)
        .expect("read retained comparison content artifact");
    let artifact: Value =
        serde_json::from_slice(&artifact_bytes).expect("parse comparison content artifact");
    assert_eq!(artifact["kind"], "minidora.relation-comparison-content.v3");
    assert_eq!(artifact["result_id"], compared.id);
    assert_eq!(artifact["content_request"]["purpose"], "COMPARE");
    assert_eq!(artifact["content_composition"]["text"], compared.text);
    assert_eq!(
        artifact["report_sha256"],
        minidora_domain::digest(compared.text.as_bytes())
    );
    for inventory in [&artifact["inventory_a"], &artifact["inventory_b"]] {
        assert!(
            inventory["facts"]
                .as_array()
                .expect("comparison inventory facts")
                .iter()
                .any(|fact| {
                    fact["proposition"]["object"] == "reliable" && fact["guarantee"] == "DERIVED"
                })
        );
    }

    let units = artifact["content_request"]["units"]
        .as_array()
        .expect("Core-authored typed comparison units");
    let rule_units = units
        .iter()
        .filter(|unit| {
            unit["role"] == "EVIDENCE"
                && unit["text"]
                    .as_str()
                    .is_some_and(|text| text.starts_with("比較に用いた保存規則"))
        })
        .collect::<Vec<_>>();
    assert_eq!(
        rule_units.len(),
        1,
        "one active source rule is in the proof"
    );
    assert!(
        rule_units[0]["text"]
            .as_str()
            .is_some_and(|text| text.contains(&first_rule)),
        "rule evidence must retain the used rule ID"
    );
    let rule_text = rule_units[0]["text"].as_str().expect("rule evidence text");
    assert!(rule_text.contains("規則ID:"));
    assert!(rule_text.contains(&format!("範囲: {scope}")));
    assert!(rule_text.contains("sensor") && rule_text.contains("reliable"));

    let expected_proofs = [&artifact["inventory_a"], &artifact["inventory_b"]]
        .into_iter()
        .fold(BTreeSet::new(), |mut all, inventory| {
            all.extend(ids(&inventory["dependencies"]));
            for fact in inventory["facts"].as_array().expect("inventory facts") {
                all.insert(fact["id"].as_str().expect("fact ID").to_owned());
                all.extend(ids(&fact["dependencies"]));
            }
            all
        });
    assert_eq!(
        ids(&artifact["content_composition"]["evidence_ids"]),
        expected_proofs
    );
    assert_eq!(
        units
            .iter()
            .flat_map(|unit| ids(&unit["evidence_ids"]))
            .collect::<BTreeSet<_>>(),
        expected_proofs,
        "typed units must represent every comparison proof ID"
    );
    for expected in [&aster_premise, &beryl_premise, &first_rule] {
        assert!(
            expected_proofs.contains(expected),
            "missing proof {expected}"
        );
    }
    for rule_unit in &rule_units {
        let text = rule_unit["text"].as_str().expect("rule evidence text");
        assert!(
            plan.required_units.contains(&text.to_owned()),
            "adopted ContentPlan omits rule evidence unit"
        );
    }

    drop(engine);
    let mut reopened = Engine::open(directory.path()).expect("reopen persisted Core state");
    let mut followup_request =
        RequestEnvelope::new(comparison_case, "b40-comparison-followup", actor, "なぜ");
    followup_request.context_ref = Some(compared.id.clone());
    let followup = reopened
        .execute(actor, followup_request)
        .expect("regenerate and replay the V3 comparison artifact");
    assert_eq!(followup.lifecycle, Lifecycle::Commit, "{followup:?}");
    assert!(followup.dependencies.contains(&compared.id));
    assert!(followup.text.contains("比較結果ID"));
    assert!(followup.content_plan.is_some());
    assert_eq!(
        followup
            .render
            .as_ref()
            .map(|observation| observation.status.as_str()),
        Some("RENDERED")
    );

    println!(
        "MINIDORA_B40_TRACE={}",
        json!({
            "schema": "MINIDORA_CORE_ATOMIC_RELATION_COMPARISON_RUNTIME_TRACE_V1",
            "os": std::env::consts::OS,
            "producer": "relation inventory -> V3 rule-bearing typed comparison -> ContentPlan -> K7 render",
            "subjects": ["Aster", "Beryl"],
            "scope": scope,
            "seed_rule_ids": [first_rule],
            "seed_premise_ids": [aster_premise, beryl_premise],
            "comparison": {
                "result_id": compared.id,
                "artifact_sha256": minidora_domain::digest(&artifact_bytes),
                "artifact_kind": artifact["kind"],
                "rule_unit_count": rule_units.len(),
                "proof_ids": expected_proofs,
                "plan_id": plan.id,
                "render_sha256": render.actual_sha256,
                "render_bytes": render.bytes.len(),
            },
            "reopened_followup": {
                "result_id": followup.id,
                "lifecycle": followup.lifecycle,
                "prior_comparison_consumed": followup.dependencies.contains(&compared.id),
                "rendered": followup.render.is_some(),
            },
            "outside_world_truth": "not checked",
        })
    );
}
