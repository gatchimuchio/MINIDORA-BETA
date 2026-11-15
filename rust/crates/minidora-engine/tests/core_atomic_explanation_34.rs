use minidora_domain::{Guarantee, Intent, Lifecycle, Proposition, RequestEnvelope};
use minidora_engine::{
    Engine,
    language::{ModelEmission, SequenceModel, rational_text},
    store::Store,
};
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_TEMP_DIRECTORY: AtomicU64 = AtomicU64::new(0);

struct TestDirectory(PathBuf);

impl TestDirectory {
    fn new() -> Self {
        let serial = NEXT_TEMP_DIRECTORY.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "minidora-core-atomic-explanation-34-{}-{serial}",
            std::process::id()
        ));
        fs::create_dir(&path).expect("create isolated derived explanation state");
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

fn proposition(subject: &str, object: &str) -> Proposition {
    Proposition {
        subject: subject.into(),
        predicate: "is".into(),
        object: object.into(),
        negative: false,
        scope: "derived-atomic-explanation-lab".into(),
    }
}

fn request(case: &str, input: &str, actor: &str) -> RequestEnvelope {
    RequestEnvelope::new(case, input, actor, "")
}

fn execute(
    engine: &mut Engine,
    actor: &str,
    case: &str,
    input: &str,
    text: &str,
) -> minidora_domain::ResultRecord {
    engine
        .execute(actor, RequestEnvelope::new(case, input, actor, text))
        .unwrap_or_else(|error| panic!("Engine::execute failed for {input}: {error}"))
}

#[test]
fn typed_derived_explanation_carries_premise_and_rule_lineage_through_k7() {
    let directory = TestDirectory::new();
    let actor = "b34-derived-atomic-explanation";
    let case = "atomic-derived-seed";
    let mut engine = Engine::open(directory.path()).expect("open isolated Core state");

    let rule_text = "If [scope=derived-atomic-explanation-lab]X is a sensor and calibrated then [scope=derived-atomic-explanation-lab]X is reliable";
    let rule = execute(&mut engine, actor, case, "reliable-rule", rule_text);
    assert_eq!(rule.lifecycle, Lifecycle::Commit, "{rule:?}");
    assert_eq!(rule.guarantee, Guarantee::UserPremise);
    let rule_id = rule
        .dependencies
        .first()
        .expect("rule result exposes the adopted rule ID")
        .clone();

    let sensor_text = "[scope=derived-atomic-explanation-lab]Vela is a sensor";
    let sensor = execute(&mut engine, actor, case, "vela-sensor", sensor_text);
    assert_eq!(sensor.lifecycle, Lifecycle::Commit, "{sensor:?}");
    let sensor_id = sensor
        .dependencies
        .first()
        .expect("sensor result exposes its premise ID")
        .clone();

    let calibrated_text = "[scope=derived-atomic-explanation-lab]Vela is calibrated";
    let calibrated = execute(&mut engine, actor, case, "vela-calibrated", calibrated_text);
    assert_eq!(calibrated.lifecycle, Lifecycle::Commit, "{calibrated:?}");
    let calibrated_id = calibrated
        .dependencies
        .first()
        .expect("calibrated result exposes its premise ID")
        .clone();

    let target = proposition("Vela", "reliable");
    let supported = engine
        .compose_atomic_explanation(
            actor,
            request("atomic-derived-supported", "explain-vela-reliable", actor),
            target.clone(),
        )
        .expect("execute typed explanation for an unasserted derived relation");
    assert_eq!(supported.lifecycle, Lifecycle::Commit, "{supported:?}");
    assert_eq!(supported.guarantee, Guarantee::Derived);
    for expected in [&rule_id, &sensor_id, &calibrated_id] {
        assert!(
            supported.dependencies.contains(expected),
            "missing proof ID {expected}"
        );
    }
    assert!(supported.text.contains("Velaはreliableである"));
    assert!(supported.text.contains("sensor"));
    assert!(supported.text.contains("calibrated"));
    assert!(supported.text.contains(&rule_id));

    let plan = supported
        .content_plan
        .as_ref()
        .expect("Core adopted a derived explanation plan");
    assert_eq!(plan.guarantee, Guarantee::Derived);
    assert!(!plan.required_units.is_empty());
    for expected in [&rule_id, &sensor_id, &calibrated_id] {
        assert!(
            plan.dependencies.contains(expected),
            "plan lost proof ID {expected}"
        );
    }
    assert!(
        plan.required_units
            .iter()
            .any(|unit| unit.contains("sensor"))
    );
    assert!(
        plan.required_units
            .iter()
            .any(|unit| unit.contains("calibrated"))
    );
    assert!(
        plan.required_units
            .iter()
            .any(|unit| unit.contains(&rule_id))
    );

    let render = supported
        .render
        .as_ref()
        .expect("K7 returned the actual render observation");
    assert_eq!(render.status, "RENDERED");
    assert_eq!(render.plan_id, plan.id);
    assert_eq!(render.bytes, plan.text.as_bytes());
    assert!(supported.execution_graph_ref.is_some());

    let supported_case = engine
        .latest(actor, "atomic-derived-supported")
        .expect("read derived explanation case")
        .expect("derived explanation case exists");
    let bundle = supported_case
        .bundle
        .as_ref()
        .expect("typed input is sealed");
    assert_eq!(bundle.meaning.source_kind, "CORE_TYPED_ATOMIC_MEANING_V1");
    assert_eq!(
        bundle.meaning.parser_version,
        "core-typed-atomic-meaning-v1"
    );
    assert_eq!(
        bundle.meaning.intent,
        Intent::AskWhy {
            proposition: target.clone()
        }
    );

    let artifact_store = Store::open(directory.path()).expect("open scoped artifact store");
    let model_bytes = artifact_store
        .get_bytes(actor, &bundle.request.case_id, &plan.model_artifact_ref)
        .expect("read the exact Core-trained model artifact");
    assert_eq!(
        minidora_domain::digest(&model_bytes),
        plan.model_artifact_ref,
        "model artifact reference must identify its exact stored bytes"
    );
    let model_value: Value =
        serde_json::from_slice(&model_bytes).expect("stored model artifact JSON");
    let model: SequenceModel =
        serde_json::from_slice(&model_bytes).expect("deserialize stored model artifact");
    model.validate().expect("stored model validates");

    let emission_bytes = artifact_store
        .get_bytes(actor, &bundle.request.case_id, &plan.model_observation_ref)
        .expect("read the exact Core model-emission observation");
    assert_eq!(
        minidora_domain::digest(&emission_bytes),
        plan.model_observation_ref,
        "emission reference must identify its exact stored bytes"
    );
    let emission_value: Value =
        serde_json::from_slice(&emission_bytes).expect("stored emission observation JSON");
    let emission: ModelEmission =
        serde_json::from_slice(&emission_bytes).expect("deserialize stored emission observation");
    assert_eq!(emission.model_revision, plan.model_revision);
    assert_eq!(emission.text, plan.text);
    let authorized_surfaces = emission
        .authorized_surfaces
        .as_ref()
        .expect("Core supplied a finite set of equivalent surfaces");
    assert_eq!(authorized_surfaces.len(), 2);
    let connective_a = "根拠のつながりは次のとおりです。";
    let connective_b = "結論を支える記録を順に示します。";
    assert_eq!(authorized_surfaces[0].matches(connective_a).count(), 1);
    assert_eq!(authorized_surfaces[1].matches(connective_b).count(), 1);
    assert_eq!(
        authorized_surfaces[0].replace(connective_a, "{model-selected-transition}"),
        authorized_surfaces[1].replace(connective_b, "{model-selected-transition}"),
        "authorized alternatives must preserve the same factual and proof text"
    );
    for surface in authorized_surfaces {
        for required_unit in &plan.required_units {
            assert!(
                surface.contains(required_unit),
                "authorized alternative dropped Core-required content: {required_unit}"
            );
        }
    }
    let candidate_scores = authorized_surfaces
        .iter()
        .map(|surface| {
            (
                surface.clone(),
                model
                    .lossless_sequence_probability(surface.as_bytes())
                    .expect("score a Core-authorized candidate with the stored model"),
            )
        })
        .collect::<Vec<_>>();
    let expected_surface = candidate_scores
        .iter()
        .max_by(|(left_text, left_score), (right_text, right_score)| {
            left_score
                .cmp(right_score)
                .then_with(|| right_text.cmp(left_text))
        })
        .map(|(text, _)| text)
        .expect("at least one authorized surface");
    assert_eq!(
        &emission.text, expected_surface,
        "stored emission must select the maximum-scoring authorized surface"
    );
    let selected_score = candidate_scores
        .iter()
        .find(|(surface, _)| surface == &emission.text)
        .expect("selected surface has a computed exact score");
    assert_eq!(
        emission.sequence_probability,
        Some(rational_text(&selected_score.1))
    );
    let replay = model
        .realize_supported(authorized_surfaces, 2_000_000)
        .expect("replay the same authorized set from the stored model");
    assert_eq!(
        serde_json::to_value(replay).expect("serialize deterministic model replay"),
        serde_json::to_value(&emission).expect("serialize retained model emission"),
        "same model and candidate set must reproduce the retained emission"
    );
    let candidate_observations = candidate_scores
        .iter()
        .enumerate()
        .map(|(index, (surface, score))| {
            serde_json::json!({
                "candidate_index": index,
                "text": surface,
                "sequence_probability": rational_text(score),
                "selected": surface == &emission.text,
            })
        })
        .collect::<Vec<_>>();

    let direct_input: Value =
        serde_json::from_str(&bundle.request.text).expect("sealed typed request JSON");
    for expected in [&rule_id, &sensor_id, &calibrated_id] {
        assert!(
            direct_input["source_ids"]
                .as_array()
                .is_some_and(|ids| { ids.iter().any(|id| id.as_str() == Some(expected.as_str())) })
        );
    }
    let memory = Store::open(directory.path())
        .expect("open isolated relation memory")
        .load_memory(actor)
        .expect("read actor memory");
    assert!(!memory.facts.iter().any(|fact| {
        fact.valid && fact.guarantee == Guarantee::UserPremise && fact.proposition == target
    }));

    let missing_target = proposition("Vela", "conductive");
    let missing = engine
        .compose_atomic_explanation(
            actor,
            request("atomic-derived-missing", "explain-unsupported", actor),
            missing_target.clone(),
        )
        .expect("unsupported typed relation returns a terminal result");
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

    let forbidden_path = directory.path().join("derived-unauthorized-output.txt");
    let mut denied_request = request("atomic-derived-denied-write", "explain-write", actor);
    denied_request.output_path = Some(forbidden_path.clone());
    let write_grant_count = denied_request.permissions.write_paths.len();
    assert_eq!(write_grant_count, 0);
    let denied = engine
        .compose_atomic_explanation(actor, denied_request, target.clone())
        .expect("unauthorized file output returns a terminal result");
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
    let denied_actions = Store::open(directory.path())
        .expect("open isolated action ledger")
        .actions(actor, "atomic-derived-denied-write")
        .expect("read denied-write action ledger");
    assert!(denied_actions.is_empty());

    let rendered_text = String::from_utf8(render.bytes.clone()).expect("rendered output is UTF-8");
    let target_filename = forbidden_path
        .file_name()
        .expect("test output path has a filename")
        .to_string_lossy()
        .into_owned();
    println!(
        "MINIDORA_B35_TRACE={}",
        serde_json::json!({
            "schema": "MINIDORA_CORE_DERIVED_ATOMIC_MODEL_SELECTION_OBSERVATION_V1",
            "os": std::env::consts::OS,
            "producer": "Engine::compose_atomic_explanation",
            "seed": {
                "actor_id": actor,
                "case_id": case,
                "rule": {"input_id": "reliable-rule", "original_text": rule_text, "result_id": &rule.id, "rule_id": &rule_id},
                "sensor": {"input_id": "vela-sensor", "original_text": sensor_text, "result_id": &sensor.id, "proof_id": &sensor_id},
                "calibrated": {"input_id": "vela-calibrated", "original_text": calibrated_text, "result_id": &calibrated.id, "proof_id": &calibrated_id},
                "conclusion_user_premise_present": memory.facts.iter().any(|fact| {
                    fact.valid && fact.guarantee == Guarantee::UserPremise && fact.proposition == proposition("Vela", "reliable")
                }),
            },
            "supported": {
                "case_id": &bundle.request.case_id,
                "input_id": &bundle.request.input_id,
                "input_version": bundle.request.input_version,
                "sealed_request_text": &bundle.request.text,
                "typed_input": &direct_input,
                "source_kind": &bundle.meaning.source_kind,
                "parser_version": &bundle.meaning.parser_version,
                "raw_sha256": &bundle.raw_sha256,
                "fingerprint": &bundle.fingerprint,
                "semantic_signature": &bundle.semantic_signature,
                "result": {
                    "id": &supported.id,
                    "lifecycle": &supported.lifecycle,
                    "guarantee": &supported.guarantee,
                    "text": &supported.text,
                    "dependencies": &supported.dependencies,
                    "execution_graph_ref": &supported.execution_graph_ref,
                },
                "content_plan": {
                    "id": &plan.id,
                    "text": &plan.text,
                    "required_units": &plan.required_units,
                    "dependencies": &plan.dependencies,
                    "model_revision": &plan.model_revision,
                    "model_artifact_ref": &plan.model_artifact_ref,
                    "model_observation_ref": &plan.model_observation_ref,
                },
                "model": {
                    "artifact_ref": &plan.model_artifact_ref,
                    "artifact_sha256": minidora_domain::digest(&model_bytes),
                    "artifact_bytes": model_bytes.len(),
                    "serialized_artifact": model_value,
                    "revision": &plan.model_revision,
                    "validated": true,
                    "replayed_emission_matches": true,
                    "authorized_candidates": candidate_observations,
                    "emission": {
                        "artifact_ref": &plan.model_observation_ref,
                        "artifact_sha256": minidora_domain::digest(&emission_bytes),
                        "artifact_bytes": emission_bytes.len(),
                        "serialized_artifact": emission_value,
                    },
                },
                "render": {
                    "plan_id": &render.plan_id,
                    "renderer_version": &render.renderer_version,
                    "status": &render.status,
                    "actual_sha256": &render.actual_sha256,
                    "byte_length": render.bytes.len(),
                    "utf8_text": rendered_text,
                },
            },
            "missing_evidence": {
                "typed_proposition": missing_target,
                "result_id": &missing.id,
                "lifecycle": &missing.lifecycle,
                "reason": &missing.reason,
                "content_plan_adopted": missing.content_plan.is_some(),
                "render_created": missing.render.is_some(),
            },
            "denied_write": {
                "typed_proposition": target,
                "requested_target_filename": target_filename,
                "write_grant_count": write_grant_count,
                "result_id": &denied.id,
                "lifecycle": &denied.lifecycle,
                "reason": &denied.reason,
                "content_plan_id": denied.content_plan.as_ref().map(|plan| &plan.id),
                "render_status": denied.render.as_ref().map(|render| &render.status),
                "receipt_count": denied.receipts.len(),
                "target_exists": forbidden_path.exists(),
                "action_count": denied_actions.len(),
            },
        })
    );
}
