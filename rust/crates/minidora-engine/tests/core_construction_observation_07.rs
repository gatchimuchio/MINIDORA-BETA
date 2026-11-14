use minidora_domain::{
    CaseRevision, Guarantee, InputBundle, Lifecycle, MemoryState, RequestEnvelope, ResultRecord,
};
use minidora_engine::{Engine, store::Store};
use serde_json::{Value, json};
use std::fs;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_TEMP_DIRECTORY: AtomicU64 = AtomicU64::new(0);

const ACTOR: &str = "b16-deterministic-core-observation";
const CASE: &str = "core-observation-seven";

fn run(engine: &mut Engine, request: RequestEnvelope) -> ResultRecord {
    let input_id = request.input_id.clone();
    engine
        .execute(ACTOR, request)
        .unwrap_or_else(|error| panic!("Engine::execute failed for {input_id}: {error}"))
}

fn result_trace(result: &ResultRecord) -> Value {
    json!({
        "id": result.id,
        "input_id": result.input_id,
        "input_version": result.input_version,
        "lifecycle": result.lifecycle,
        "text": result.text,
        "reason": result.reason,
        "guarantee": result.guarantee,
        "calculation": result.calculation,
        "dependencies": result.dependencies,
        "execution_graph_ref": result.execution_graph_ref,
        "content_plan": result.content_plan.as_ref().map(|plan| json!({
            "id": plan.id,
            "text": plan.text,
            "required_units": plan.required_units,
            "dependencies": plan.dependencies,
            "guarantee": plan.guarantee,
            "model_revision": plan.model_revision,
            "model_artifact_ref": plan.model_artifact_ref,
            "model_observation_ref": plan.model_observation_ref,
        })),
        "render": result.render.as_ref().map(|render| json!({
            "plan_id": render.plan_id,
            "actual_sha256": render.actual_sha256,
            "byte_length": render.bytes.len(),
            "renderer_version": render.renderer_version,
            "status": render.status,
        })),
    })
}

fn bundle_trace(bundle: &InputBundle) -> Value {
    json!({
        "input_id": bundle.request.input_id,
        "input_version": bundle.request.input_version,
        "original_text": bundle.request.text,
        "raw_sha256": bundle.raw_sha256,
        "parser_version": bundle.meaning.parser_version,
        "intent": bundle.meaning.intent,
        "semantic_signature": bundle.semantic_signature,
        "context_result_ref": bundle.meaning.context_result_ref,
    })
}

struct Snapshot {
    initial_results: Vec<ResultRecord>,
    initial_bundle: InputBundle,
    initial_case: CaseRevision,
    initial_memory: MemoryState,
    initial_sensor_fact_id: String,
    correction: ResultRecord,
    correction_bundle: InputBundle,
    corrected_case: CaseRevision,
    corrected_memory: MemoryState,
    repeated_query: ResultRecord,
    final_case: CaseRevision,
    final_memory: MemoryState,
}

fn execute_snapshot(path: &Path) -> Snapshot {
    let mut engine = Engine::open(path).expect("open deterministic snapshot Core");
    let rule_text = "If [scope=repeatability-lab]X is a sensor and calibrated then [scope=repeatability-lab]X is reliable";
    let rule = run(
        &mut engine,
        RequestEnvelope::new(CASE, "rule", ACTOR, rule_text),
    );
    assert_eq!(rule.lifecycle, Lifecycle::Commit);

    let sensor = run(
        &mut engine,
        RequestEnvelope::new(
            CASE,
            "sensor-observation",
            ACTOR,
            "[scope=repeatability-lab]Nera is a sensor",
        ),
    );
    assert_eq!(sensor.lifecycle, Lifecycle::Commit);
    let sensor_fact_id = sensor.dependencies[0].clone();
    let calibrated = run(
        &mut engine,
        RequestEnvelope::new(
            CASE,
            "calibrated-observation",
            ACTOR,
            "[scope=repeatability-lab]Nera is calibrated",
        ),
    );
    assert_eq!(calibrated.lifecycle, Lifecycle::Commit);
    let query_text = "[scope=repeatability-lab]Nera is reliable?";
    let supported = run(
        &mut engine,
        RequestEnvelope::new(CASE, "reliability-query", ACTOR, query_text),
    );
    assert_eq!(supported.lifecycle, Lifecycle::Commit);
    assert_eq!(supported.guarantee, Guarantee::Derived);
    assert!(supported.dependencies.contains(&sensor_fact_id));
    let initial_case = engine
        .latest(ACTOR, CASE)
        .expect("read identical initial case snapshot")
        .expect("initial case state exists");
    let initial_bundle = initial_case
        .bundle
        .clone()
        .expect("sealed input of supported result");
    let store = Store::open(path).expect("open deterministic state for memory snapshot");
    let initial_memory = store.load_memory(ACTOR).expect("read initial actor memory");
    assert!(
        initial_memory
            .facts
            .iter()
            .find(|fact| fact.id == sensor_fact_id)
            .is_some_and(|fact| fact.valid)
    );

    drop(store);
    drop(engine);
    let mut engine = Engine::open(path).expect("reopen same state before correction");
    let mut correction_request = RequestEnvelope::new(
        CASE,
        "sensor-observation",
        ACTOR,
        "[scope=repeatability-lab]Correction: Nera is a polymer, not a sensor.",
    );
    correction_request.input_version = 2;
    let correction = run(&mut engine, correction_request);
    assert_eq!(correction.lifecycle, Lifecycle::Commit);
    assert_eq!(correction.input_version, 2);
    let corrected_case = engine
        .latest(ACTOR, CASE)
        .expect("read corrected input version state")
        .expect("corrected case state exists");
    let correction_bundle = corrected_case
        .bundle
        .clone()
        .expect("sealed second version of the observation input");
    assert_eq!(correction_bundle.request.input_version, 2);
    let store = Store::open(path).expect("open deterministic state for corrected memory");
    let corrected_memory = store
        .load_memory(ACTOR)
        .expect("read corrected actor memory");
    assert_eq!(corrected_memory.revision, initial_memory.revision + 1);
    assert!(
        corrected_memory
            .facts
            .iter()
            .find(|fact| fact.id == sensor_fact_id)
            .is_some_and(|fact| !fact.valid)
    );

    let repeated_query = run(
        &mut engine,
        RequestEnvelope::new(CASE, "reliability-query-after-v2", ACTOR, query_text),
    );
    assert_eq!(repeated_query.lifecycle, Lifecycle::Suspend);
    assert!(
        repeated_query
            .reason
            .as_deref()
            .unwrap_or_default()
            .contains("MISSING_PREMISES")
    );
    assert!(repeated_query.content_plan.is_none());
    let final_case = engine
        .latest(ACTOR, CASE)
        .expect("read final deterministic case state")
        .expect("final case state exists");
    assert!(final_case.prior_results.contains_key(&supported.id));
    assert_eq!(
        final_case.prior_results.get(&supported.id),
        Some(&supported)
    );
    let final_memory = store.load_memory(ACTOR).expect("read final actor memory");

    drop(store);
    drop(engine);
    Snapshot {
        initial_results: vec![rule, sensor, calibrated, supported],
        initial_bundle,
        initial_case,
        initial_memory,
        initial_sensor_fact_id: sensor_fact_id,
        correction,
        correction_bundle,
        corrected_case,
        corrected_memory,
        repeated_query,
        final_case,
        final_memory,
    }
}

#[test]
fn direct_core_observation_seven_reproduces_identical_state_and_versions_a_caused_change() {
    let serial = NEXT_TEMP_DIRECTORY.fetch_add(1, Ordering::Relaxed);
    let first_directory = std::env::temp_dir().join(format!(
        "minidora-core-construction-observation-seven-a-{}-{serial}",
        std::process::id()
    ));
    let second_directory = std::env::temp_dir().join(format!(
        "minidora-core-construction-observation-seven-b-{}-{serial}",
        std::process::id()
    ));
    fs::create_dir(&first_directory).expect("unique first deterministic snapshot");
    fs::create_dir(&second_directory).expect("unique second deterministic snapshot");

    let first = execute_snapshot(&first_directory);
    let repeated = execute_snapshot(&second_directory);

    assert_eq!(first.initial_results, repeated.initial_results);
    assert_eq!(first.initial_bundle, repeated.initial_bundle);
    assert_eq!(first.initial_case, repeated.initial_case);
    assert_eq!(first.initial_memory, repeated.initial_memory);
    assert_eq!(
        first.initial_sensor_fact_id,
        repeated.initial_sensor_fact_id
    );
    assert_eq!(first.correction, repeated.correction);
    assert_eq!(first.correction_bundle, repeated.correction_bundle);
    assert_eq!(first.corrected_case, repeated.corrected_case);
    assert_eq!(first.corrected_memory, repeated.corrected_memory);
    assert_eq!(first.repeated_query, repeated.repeated_query);
    assert_eq!(first.final_case, repeated.final_case);
    assert_eq!(first.final_memory, repeated.final_memory);

    let initial_query = first.initial_results.last().expect("initial query result");
    assert_eq!(initial_query.lifecycle, Lifecycle::Commit);
    assert_eq!(initial_query.guarantee, Guarantee::Derived);
    assert!(
        first
            .correction
            .dependencies
            .contains(&first.initial_sensor_fact_id)
    );
    assert!(
        first
            .correction_bundle
            .meaning
            .context_result_ref
            .as_deref()
            .is_some()
    );
    assert_eq!(
        first.initial_memory.revision + 1,
        first.corrected_memory.revision
    );
    assert_eq!(first.repeated_query.lifecycle, Lifecycle::Suspend);
    assert!(
        first
            .final_case
            .prior_results
            .contains_key(&initial_query.id)
    );

    let evidence = json!({
        "observation": "MB-BUILD-20261114 core construction observation seven",
        "entry": "in-process minidora_engine::Engine::execute; two independently initialized identical local SQLite snapshots; Engine reopened before changed input version",
        "actor": ACTOR,
        "case_id": CASE,
        "same_input_and_data_reproduction": true,
        "exact_initial_records_equal": first.initial_results == repeated.initial_results,
        "initial_case_revision_equal": first.initial_case == repeated.initial_case,
        "initial_memory_equal": first.initial_memory == repeated.initial_memory,
        "exact_corrected_input_equal": first.correction_bundle == repeated.correction_bundle,
        "exact_correction_result_equal": first.correction == repeated.correction,
        "exact_post_change_query_equal": first.repeated_query == repeated.repeated_query,
        "exact_final_case_equal": first.final_case == repeated.final_case,
        "exact_final_memory_equal": first.final_memory == repeated.final_memory,
        "initial_model_revisions": first.initial_results.iter().filter_map(|result| result.content_plan.as_ref().map(|plan| plan.model_revision.clone())).collect::<Vec<_>>(),
        "corrected_input_model_revision": first.correction.content_plan.as_ref().map(|plan| plan.model_revision.clone()),
        "initial_result_traces": first.initial_results.iter().map(result_trace).collect::<Vec<_>>(),
        "initial_query_input": bundle_trace(&first.initial_bundle),
        "changed_source_v2_input": bundle_trace(&first.correction_bundle),
        "changed_source_v2_result": result_trace(&first.correction),
        "memory_revision_before_change": first.initial_memory.revision,
        "memory_revision_after_change": first.corrected_memory.revision,
        "invalidated_source_fact_id": first.initial_sensor_fact_id,
        "same_question_after_changed_memory": result_trace(&first.repeated_query),
        "final_case_revision": first.final_case.revision,
        "historical_supported_result_retained": first.final_case.prior_results.get(&initial_query.id).map(result_trace),
        "changed_outcome_cause": "The same exact relation question changes from DERIVED/COMMIT to SUSPEND/MISSING_PREMISES after input_id sensor-observation advances from version 1 (sensor) to version 2 (polymer); the source fact is invalidated and the actor memory revision increments by one.",
        "external_semantic_generation": false,
        "external_network_or_side_effects": false,
        "document_version": "NOT_APPLICABLE_NO_DOCUMENT_INPUT",
        "execution_environment": {
            "os": std::env::consts::OS,
            "architecture": std::env::consts::ARCH,
        },
        "limitations": [
            "two deterministic isolated local snapshots on one host and toolchain",
            "not cross-platform, cross-process timing, full workspace runtime, or broad requirement acceptance"
        ],
    });
    println!(
        "B16_CORE_OBSERVATION_07={}",
        serde_json::to_string(&evidence).expect("serialize deterministic Core observation")
    );

    drop(first);
    drop(repeated);
    fs::remove_dir_all(first_directory).expect("remove first deterministic state");
    fs::remove_dir_all(second_directory).expect("remove second deterministic state");
}
