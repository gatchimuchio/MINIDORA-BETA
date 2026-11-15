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
            "minidora-core-atomic-relation-comparison-41-{}-{serial}",
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
    RequestEnvelope::new(case, "b41-comparison", actor, "")
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
fn derived_comparison_v4_replays_rules_renders_and_survives_reopen() {
    let directory = TestDirectory::new();
    let actor = "b41-derived-atomic-relation-comparison";
    let scope = "b41-derived-comparison-lab";
    let comparison_case = "b41-comparison-derived";
    let mut engine = Engine::open(directory.path()).expect("open isolated Core state");

    let first_rule = execute(
        &mut engine,
        actor,
        "b41-rule-one",
        "sensor-to-inspected",
        "If [scope=b41-derived-comparison-lab]X is a sensor then [scope=b41-derived-comparison-lab]X is inspected",
    );
    let second_rule = execute(
        &mut engine,
        actor,
        "b41-rule-two",
        "inspected-to-reliable",
        "If [scope=b41-derived-comparison-lab]X is inspected then [scope=b41-derived-comparison-lab]X is reliable",
    );
    let aster_premise = execute(
        &mut engine,
        actor,
        "b41-aster-premise",
        "aster-sensor",
        "[scope=b41-derived-comparison-lab]Aster is a sensor",
    );
    let beryl_premise = execute(
        &mut engine,
        actor,
        "b41-beryl-premise",
        "beryl-sensor",
        "[scope=b41-derived-comparison-lab]Beryl is a sensor",
    );

    let mut request = comparison_request(comparison_case, actor);
    request.budget.max_actions = 256;
    let compared = engine
        .compose_atomic_relation_comparison(actor, request, "Aster", "Beryl", scope)
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
    assert_eq!(artifact["kind"], "minidora.relation-comparison-content.v4");
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
    assert!(
        rule_units.len() >= 2,
        "both active source rules are in the proof"
    );
    for (rule_id, premise, conclusion) in [
        (&first_rule, "sensor", "inspected"),
        (&second_rule, "inspected", "reliable"),
    ] {
        let rule_text = rule_units
            .iter()
            .filter_map(|unit| unit["text"].as_str())
            .find(|text| text.contains(rule_id))
            .expect("rule evidence retains each used rule ID");
        assert!(rule_text.contains("規則ID:"));
        assert!(rule_text.contains(&format!("範囲: {scope}")));
        assert!(rule_text.contains(premise) && rule_text.contains(conclusion));
    }

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
    let mut v4_max_fact_evidence_bytes = 0;
    let mut reconstructed_v3_max_fact_evidence_bytes = 0;
    for unit in units.iter().filter(|unit| {
        unit["role"] == "EVIDENCE"
            && unit["text"]
                .as_str()
                .is_some_and(|text| text.starts_with("根拠("))
    }) {
        let text = unit["text"].as_str().expect("fact evidence text");
        v4_max_fact_evidence_bytes = v4_max_fact_evidence_bytes.max(text.len());
        let record_id = text
            .split_once("記録ID: ")
            .and_then(|(_, suffix)| suffix.split_once(';'))
            .map(|(id, _)| id)
            .expect("fact evidence retains its record ID");
        let dependencies = ids(&unit["evidence_ids"])
            .into_iter()
            .filter(|id| id != record_id)
            .collect::<Vec<_>>();
        let dependency_text = if dependencies.is_empty() {
            "なし".to_owned()
        } else {
            dependencies.join(", ")
        };
        let compact_marker = format!("依存根拠: {}件", dependencies.len());
        assert!(text.contains(&compact_marker));
        let reconstructed_v3_text =
            text.replace(&compact_marker, &format!("依存ID: {dependency_text}"));
        reconstructed_v3_max_fact_evidence_bytes =
            reconstructed_v3_max_fact_evidence_bytes.max(reconstructed_v3_text.len());
    }
    assert!(v4_max_fact_evidence_bytes <= 384);
    assert!(
        reconstructed_v3_max_fact_evidence_bytes > 384,
        "the same fact evidence with dependency IDs restored must exceed the inherited unit limit"
    );
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
    let expected_rule_ids = expected_proofs
        .iter()
        .filter(|id| id.starts_with("rule:"))
        .cloned()
        .collect::<BTreeSet<_>>();
    let represented_rule_ids = rule_units
        .iter()
        .flat_map(|unit| ids(&unit["evidence_ids"]))
        .filter(|id| id.starts_with("rule:"))
        .collect::<BTreeSet<_>>();
    assert_eq!(represented_rule_ids, expected_rule_ids);
    for inventory in [&artifact["inventory_a"], &artifact["inventory_b"]] {
        let reliable = inventory["facts"]
            .as_array()
            .expect("inventory facts")
            .iter()
            .find(|fact| {
                fact["proposition"]["object"] == "reliable" && fact["guarantee"] == "DERIVED"
            })
            .expect("two-rule chain derives reliable");
        let reliable_id = reliable["id"].as_str().expect("derived fact ID");
        let dependency_ids = ids(&reliable["dependencies"]);
        assert!(dependency_ids.contains(&first_rule));
        assert!(dependency_ids.contains(&second_rule));
        let evidence_unit = units
            .iter()
            .find(|unit| {
                unit["role"] == "EVIDENCE"
                    && unit["text"]
                        .as_str()
                        .is_some_and(|text| text.starts_with("根拠("))
                    && ids(&unit["evidence_ids"]).contains(reliable_id)
            })
            .expect("derived fact has a typed evidence unit");
        let text = evidence_unit["text"].as_str().expect("fact evidence text");
        assert!(text.contains("依存根拠:"));
        assert!(!text.contains("依存ID:"));
        let unit_evidence_ids = ids(&evidence_unit["evidence_ids"]);
        assert!(unit_evidence_ids.contains(&first_rule));
        assert!(unit_evidence_ids.contains(&second_rule));
    }
    for expected in [&aster_premise, &beryl_premise, &first_rule, &second_rule] {
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
        RequestEnvelope::new(comparison_case, "b41-comparison-followup", actor, "なぜ");
    followup_request.budget.max_actions = 256;
    followup_request.context_ref = Some(compared.id.clone());
    let followup = reopened
        .execute(actor, followup_request)
        .expect("regenerate and replay the V4 comparison artifact");
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
        "MINIDORA_B41_TRACE={}",
        json!({
            "schema": "MINIDORA_CORE_ATOMIC_RELATION_COMPARISON_RUNTIME_TRACE_V1",
            "os": std::env::consts::OS,
            "producer": "relation inventory -> V4 rule-bearing typed comparison -> ContentPlan -> K7 render",
            "subjects": ["Aster", "Beryl"],
            "scope": scope,
            "seed_rule_ids": [first_rule, second_rule],
            "seed_premise_ids": [aster_premise, beryl_premise],
            "comparison": {
                "result_id": compared.id,
                "artifact_sha256": minidora_domain::digest(&artifact_bytes),
                "artifact_kind": artifact["kind"],
                "rule_unit_count": rule_units.len(),
                "unit_count": units.len(),
                "v4_max_fact_evidence_bytes": v4_max_fact_evidence_bytes,
                "reconstructed_v3_max_fact_evidence_bytes": reconstructed_v3_max_fact_evidence_bytes,
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
