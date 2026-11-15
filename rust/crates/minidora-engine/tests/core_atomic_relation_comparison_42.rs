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
            "minidora-core-atomic-relation-comparison-42-{}-{serial}",
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
        .filter_map(Value::as_str)
        .map(str::to_owned)
        .collect()
}

#[test]
fn overfull_rule_comparison_uses_legacy_report_and_replays_all_proofs() {
    let directory = TestDirectory::new();
    let actor = "b42-comparison-unit-boundary";
    let scope = "b42-comparison-unit-boundary-lab";
    let comparison_case = "b42-comparison-overfull";
    let mut engine = Engine::open(directory.path()).expect("open isolated Core state");

    let rule_id = execute(
        &mut engine,
        actor,
        "b42-rule",
        "sensor-to-reliable",
        "If [scope=b42-comparison-unit-boundary-lab]X is a sensor then [scope=b42-comparison-unit-boundary-lab]X is reliable",
    );
    let mut seed_ids = Vec::new();
    for (case, input, text) in [
        (
            "b42-aster-sensor",
            "aster-sensor",
            "[scope=b42-comparison-unit-boundary-lab]Aster is a sensor",
        ),
        (
            "b42-aster-musician",
            "aster-musician",
            "[scope=b42-comparison-unit-boundary-lab]Aster is a musician",
        ),
        (
            "b42-aster-pilot",
            "aster-pilot",
            "[scope=b42-comparison-unit-boundary-lab]Aster is a pilot",
        ),
        (
            "b42-beryl-sensor",
            "beryl-sensor",
            "[scope=b42-comparison-unit-boundary-lab]Beryl is a sensor",
        ),
        (
            "b42-beryl-pilot",
            "beryl-pilot",
            "[scope=b42-comparison-unit-boundary-lab]Beryl is a pilot",
        ),
        (
            "b42-beryl-geologist",
            "beryl-geologist",
            "[scope=b42-comparison-unit-boundary-lab]Beryl is a geologist",
        ),
    ] {
        seed_ids.push(execute(&mut engine, actor, case, input, text));
    }

    let mut request = RequestEnvelope::new(comparison_case, "b42-comparison", actor, "");
    request.budget.max_actions = 256;
    let compared = engine
        .compose_atomic_relation_comparison(actor, request, "Aster", "Beryl", scope)
        .expect("execute relation comparison beyond the typed-unit envelope");
    assert_eq!(compared.lifecycle, Lifecycle::Commit, "{compared:?}");
    let plan = compared
        .content_plan
        .as_ref()
        .expect("Core adopted a ContentPlan for the legacy report");
    assert_eq!(plan.text, compared.text);
    assert!(
        compared
            .text
            .starts_with("保存済みの根拠を比較した記録です。")
    );
    for relation in ["sensor", "reliable", "musician", "pilot", "geologist"] {
        assert!(
            compared.text.contains(relation),
            "missing relation {relation}"
        );
    }
    assert!(compared.text.contains("現実世界の真偽は確認していません"));
    let render = compared
        .render
        .as_ref()
        .expect("legacy report also returns the actual render observation");
    assert_eq!(render.status, "RENDERED");
    assert_eq!(render.plan_id, plan.id);
    assert_eq!(render.bytes, compared.text.as_bytes());
    assert_eq!(render.actual_sha256, minidora_domain::digest(&render.bytes));

    let bundle = engine
        .latest(actor, comparison_case)
        .expect("read comparison case")
        .expect("comparison case exists")
        .bundle
        .expect("typed comparison request was sealed in Core");
    let artifact_ref = compared
        .dependencies
        .first()
        .expect("comparison result retains its report artifact reference");
    let artifact_bytes = Store::open(directory.path())
        .expect("open isolated artifact store")
        .get_bytes(actor, &bundle.request.case_id, artifact_ref)
        .expect("read persisted legacy comparison artifact");
    let artifact: Value = serde_json::from_slice(&artifact_bytes).expect("parse artifact");
    assert_eq!(artifact["kind"], "minidora.relation-comparison-report.v1");
    assert_eq!(artifact["result_id"], compared.id);
    assert!(artifact.get("content_request").is_none());
    assert_eq!(
        artifact["report_sha256"],
        minidora_domain::digest(compared.text.as_bytes())
    );

    let inventories = [&artifact["inventory_a"], &artifact["inventory_b"]];
    let mut proof_ids = BTreeSet::new();
    let mut fact_proof_ids = BTreeSet::new();
    let mut facts = Vec::new();
    let mut relation_keys = BTreeSet::new();
    let mut max_v2_fact_unit_bytes = 0;
    for inventory in inventories {
        proof_ids.extend(ids(&inventory["dependencies"]));
        let side_facts = inventory["facts"].as_array().expect("inventory facts");
        facts.extend(side_facts.iter());
        let contested_ids = ids(&inventory["contested_fact_ids"]);
        for fact in side_facts {
            let fact_id = fact["id"].as_str().expect("fact ID");
            let proposition = &fact["proposition"];
            let subject = proposition["subject"].as_str().expect("subject");
            let predicate = proposition["predicate"].as_str().expect("predicate");
            let object = proposition["object"].as_str().expect("object");
            let negative = proposition["negative"].as_bool().expect("polarity");
            let fact_scope = proposition["scope"].as_str().expect("scope");
            relation_keys.insert((
                predicate.to_owned(),
                object.to_owned(),
                negative,
                fact_scope.to_owned(),
            ));

            fact_proof_ids.insert(fact_id.to_owned());
            let dependencies = ids(&fact["dependencies"]);
            proof_ids.extend(dependencies.iter().cloned());
            fact_proof_ids.extend(dependencies.iter().cloned());
            let dependency_text = if dependencies.is_empty() {
                "なし".to_owned()
            } else {
                dependencies.iter().cloned().collect::<Vec<_>>().join(", ")
            };
            let guarantee = match fact["guarantee"].as_str().expect("guarantee") {
                "USER_PREMISE" => "直接提示された前提",
                "DERIVED" => "保存規則からの導出",
                other => panic!("unexpected fixture guarantee {other}"),
            };
            assert_eq!(predicate, "is");
            assert!(!negative);
            let description = format!("{subject}は{object}である");
            let contested = if contested_ids.contains(fact_id) {
                "; 対立根拠あり"
            } else {
                ""
            };
            let v2_fact_text = format!(
                "根拠({subject}): {description}（{guarantee}; 記録ID: {fact_id}; 依存ID: {dependency_text}{contested}）。"
            );
            max_v2_fact_unit_bytes = max_v2_fact_unit_bytes.max(v2_fact_text.len());
        }
    }
    assert_eq!(facts.len(), 8, "two derived and six direct facts");
    assert_eq!(relation_keys.len(), 5);
    assert_eq!(proof_ids.len(), 9, "eight fact IDs and one rule ID");
    assert!(proof_ids.contains(&rule_id));
    for seed_id in &seed_ids {
        assert!(proof_ids.contains(seed_id), "missing proof {seed_id}");
    }
    assert!(proof_ids.is_superset(&fact_proof_ids));
    let proof_reference_units = proof_ids.difference(&fact_proof_ids).count();
    assert_eq!(proof_reference_units, 0);
    assert!(max_v2_fact_unit_bytes <= 384);

    // V2's structure is 3 opening/subject units, one per relation key, one
    // per fact, any proof-reference units, and one limitation unit. V3/V4
    // add one rule unit. Both candidates cross the composer's 16-unit bound.
    let v2_candidate_units = 3 + relation_keys.len() + facts.len() + proof_reference_units + 1;
    let rule_unit_count = proof_ids
        .iter()
        .filter(|id| id.starts_with("rule:"))
        .count();
    let v3_v4_candidate_units = v2_candidate_units + rule_unit_count;
    assert_eq!(v2_candidate_units, 17);
    assert_eq!(rule_unit_count, 1);
    assert_eq!(v3_v4_candidate_units, 18);
    assert!(v2_candidate_units > 16 && v3_v4_candidate_units > 16);

    let artifact_proofs = [&artifact["inventory_a"], &artifact["inventory_b"]]
        .into_iter()
        .fold(BTreeSet::new(), |mut all, inventory| {
            all.extend(ids(&inventory["dependencies"]));
            for fact in inventory["facts"].as_array().expect("inventory facts") {
                all.insert(fact["id"].as_str().expect("fact ID").to_owned());
                all.extend(ids(&fact["dependencies"]));
            }
            all
        });
    assert_eq!(artifact_proofs, proof_ids);

    drop(engine);
    let mut reopened = Engine::open(directory.path()).expect("reopen persisted Core state");
    let mut followup_request =
        RequestEnvelope::new(comparison_case, "b42-comparison-followup", actor, "なぜ");
    followup_request.budget.max_actions = 256;
    followup_request.context_ref = Some(compared.id.clone());
    let followup = reopened
        .execute(actor, followup_request)
        .expect("replay the bounded legacy report artifact");
    assert_eq!(followup.lifecycle, Lifecycle::Commit, "{followup:?}");
    assert!(followup.dependencies.contains(&compared.id));
    assert!(followup.content_plan.is_some());
    assert!(followup.text.contains("比較結果ID"));
    assert_eq!(
        followup
            .render
            .as_ref()
            .map(|observation| observation.status.as_str()),
        Some("RENDERED")
    );

    println!(
        "MINIDORA_B42_TRACE={}",
        json!({
            "schema": "MINIDORA_CORE_ATOMIC_RELATION_COMPARISON_RUNTIME_TRACE_V1",
            "os": std::env::consts::OS,
            "producer": "actor relation inventories -> V3/V4/V2 unit-bound checks -> V1 report -> ContentPlan/K7 render",
            "subjects": ["Aster", "Beryl"],
            "scope": scope,
            "max_actions": 256,
            "proof_count": proof_ids.len(),
            "fact_count": facts.len(),
            "relation_key_count": relation_keys.len(),
            "v2_candidate_units": v2_candidate_units,
            "v3_v4_candidate_units": v3_v4_candidate_units,
            "max_v2_fact_unit_bytes": max_v2_fact_unit_bytes,
            "comparison": {
                "result_id": compared.id,
                "content_plan_id": plan.id,
                "artifact_sha256": minidora_domain::digest(&artifact_bytes),
                "artifact_kind": artifact["kind"],
                "proof_ids": proof_ids,
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
