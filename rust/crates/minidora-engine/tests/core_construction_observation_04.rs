use minidora_domain::{Guarantee, Intent, Lifecycle, RequestEnvelope, ResultRecord};
use minidora_engine::{Engine, store::Store};
use serde_json::json;
use std::fs;
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_TEMP_DIRECTORY: AtomicU64 = AtomicU64::new(0);

fn run(engine: &mut Engine, actor: &str, request: RequestEnvelope) -> ResultRecord {
    let input_id = request.input_id.clone();
    let version = request.input_version;
    engine
        .execute(actor, request)
        .unwrap_or_else(|error| panic!("Engine::execute failed for {input_id} v{version}: {error}"))
}

fn normal_request(case: &str, input_id: &str, actor: &str, text: &str) -> RequestEnvelope {
    RequestEnvelope::new(case, input_id, actor, text)
}

#[test]
fn direct_core_observation_four_consumes_actual_math_and_relation_results_across_versions() {
    let serial = NEXT_TEMP_DIRECTORY.fetch_add(1, Ordering::Relaxed);
    let directory = std::env::temp_dir().join(format!(
        "minidora-core-construction-observation-four-{}-{serial}",
        std::process::id()
    ));
    fs::create_dir(&directory).expect("unique isolated observation state");
    let mut engine = Engine::open(&directory).expect("open isolated Core state");
    let actor = "b13-versioned-core-observation";
    let case = "core-observation-four";
    let mut observations = Vec::new();

    let calculation_v1 = run(
        &mut engine,
        actor,
        normal_request(case, "calculation", actor, "8+5を計算して"),
    );
    assert_eq!(calculation_v1.lifecycle, Lifecycle::Commit);
    assert_eq!(calculation_v1.input_version, 1);
    assert_eq!(
        calculation_v1
            .calculation
            .as_ref()
            .expect("actual first calculation")
            .exact_value,
        "13"
    );
    observations.push(json!({"request": "8+5を計算して", "result": calculation_v1}));

    let mut calculation_v2_request =
        normal_request(case, "calculation", actor, "第二項を3に変更してください");
    calculation_v2_request.input_version = 2;
    let calculation_v2 = run(&mut engine, actor, calculation_v2_request);
    assert_eq!(calculation_v2.lifecycle, Lifecycle::Commit);
    assert_eq!(calculation_v2.input_version, 2);
    assert!(calculation_v2.dependencies.contains(&calculation_v1.id));
    assert_eq!(
        calculation_v2
            .calculation
            .as_ref()
            .expect("corrected actual calculation")
            .exact_value,
        "11"
    );
    let calculation_v2_state = engine
        .latest(actor, case)
        .expect("read calculation version two")
        .expect("calculation version two case revision");
    let calculation_v2_bundle = calculation_v2_state
        .bundle
        .as_ref()
        .expect("sealed calculation version two input");
    assert_eq!(calculation_v2_bundle.request.input_version, 2);
    assert_eq!(
        calculation_v2_bundle.meaning.context_result_ref.as_deref(),
        Some(calculation_v1.id.as_str())
    );
    assert!(matches!(
        calculation_v2_bundle.meaning.intent,
        Intent::CorrectCalculation {
            operand_index: 1,
            ..
        }
    ));
    assert!(calculation_v2_state.revision > 0);
    assert_eq!(
        calculation_v2_state
            .result
            .as_ref()
            .map(|result| result.id.as_str()),
        Some(calculation_v2.id.as_str())
    );
    assert_eq!(
        calculation_v2_state.prior_results.get(&calculation_v1.id),
        Some(&calculation_v1),
        "version one remains retained when version two is adopted"
    );
    observations.push(json!({
        "request": "第二項を3に変更してください",
        "same_input_id_new_version": true,
        "sealed_context_result_ref": calculation_v2_bundle.meaning.context_result_ref,
        "case_revision": calculation_v2_state.revision,
        "retained_v1": calculation_v2_state.prior_results.get(&calculation_v1.id),
        "result": calculation_v2,
    }));

    let explanation = run(
        &mut engine,
        actor,
        normal_request(case, "calculation-explanation", actor, "Why?"),
    );
    assert_eq!(explanation.lifecycle, Lifecycle::Commit);
    assert!(explanation.dependencies.contains(&calculation_v2.id));
    assert_eq!(
        explanation
            .calculation
            .as_ref()
            .expect("explanation carries consumed calculation")
            .exact_value,
        "11"
    );
    observations.push(json!({"request": "Why?", "result": explanation}));

    let rule_text =
        "If [scope=virtual-lab]X is a ceramic and heated then [scope=virtual-lab]X is conductive";
    let rule = run(
        &mut engine,
        actor,
        normal_request(case, "relation-rule", actor, rule_text),
    );
    assert_eq!(rule.lifecycle, Lifecycle::Commit);
    let ceramic = run(
        &mut engine,
        actor,
        normal_request(
            case,
            "relation-ceramic",
            actor,
            "[scope=virtual-lab]Pico is a ceramic",
        ),
    );
    assert_eq!(ceramic.lifecycle, Lifecycle::Commit);
    let heated = run(
        &mut engine,
        actor,
        normal_request(
            case,
            "relation-heated",
            actor,
            "[scope=virtual-lab]Pico is heated",
        ),
    );
    assert_eq!(heated.lifecycle, Lifecycle::Commit);
    let relation_v1 = run(
        &mut engine,
        actor,
        normal_request(
            case,
            "relation-question",
            actor,
            "[scope=virtual-lab]Pico is conductive?",
        ),
    );
    assert_eq!(relation_v1.lifecycle, Lifecycle::Commit);
    assert_eq!(relation_v1.guarantee, Guarantee::Derived);
    for proof_root in [
        &rule.dependencies[0],
        &ceramic.dependencies[0],
        &heated.dependencies[0],
    ] {
        assert!(
            relation_v1.dependencies.contains(proof_root),
            "relation proof lacks root {proof_root}"
        );
    }
    observations.extend([
        json!({"request": rule_text, "result": rule}),
        json!({"request": "[scope=virtual-lab]Pico is a ceramic", "result": ceramic}),
        json!({"request": "[scope=virtual-lab]Pico is heated", "result": heated}),
        json!({"request": "[scope=virtual-lab]Pico is conductive?", "result": relation_v1}),
    ]);

    let mut relation_v2_request = normal_request(
        case,
        "relation-question",
        actor,
        "[scope=virtual-lab]it is monitored",
    );
    relation_v2_request.input_version = 2;
    let relation_v2 = run(&mut engine, actor, relation_v2_request);
    assert_eq!(relation_v2.lifecycle, Lifecycle::Commit);
    assert_eq!(relation_v2.input_version, 2);
    assert!(relation_v2.dependencies.contains(&relation_v1.id));
    let relation_v2_state = engine
        .latest(actor, case)
        .expect("read contextual relation version two")
        .expect("contextual relation version two revision");
    let relation_v2_bundle = relation_v2_state
        .bundle
        .as_ref()
        .expect("sealed relation version two input");
    assert!(matches!(
        relation_v2_bundle.meaning.intent,
        Intent::AssertPreviousRelation { .. }
    ));
    assert_eq!(
        relation_v2_bundle.meaning.context_result_ref.as_deref(),
        Some(relation_v1.id.as_str())
    );
    let relation_v2_fact_id = relation_v2
        .dependencies
        .iter()
        .find(|dependency| *dependency != &relation_v1.id)
        .expect("new relation premise reference");
    let store = Store::open(&directory).expect("open isolated Core store for memory snapshot");
    let relation_memory = store
        .load_memory(actor)
        .expect("read persisted relation memory");
    let saved_relation = relation_memory
        .facts
        .iter()
        .find(|fact| fact.id == *relation_v2_fact_id)
        .expect("persisted version-two relation premise");
    assert_eq!(saved_relation.proposition.subject, "Pico");
    assert_eq!(saved_relation.proposition.predicate, "is");
    assert_eq!(saved_relation.proposition.object, "monitored");
    assert_eq!(saved_relation.proposition.scope, "virtual-lab");
    assert_eq!(saved_relation.guarantee, Guarantee::UserPremise);
    assert_eq!(
        relation_v2_state.prior_results.get(&relation_v1.id),
        Some(&relation_v1),
        "relation version one remains retained after contextual version two"
    );
    observations.push(json!({
        "request": "[scope=virtual-lab]it is monitored",
        "same_input_id_new_version": true,
        "sealed_context_result_ref": relation_v2_bundle.meaning.context_result_ref,
        "case_revision": relation_v2_state.revision,
        "retained_v1": relation_v2_state.prior_results.get(&relation_v1.id),
        "persisted_fact": saved_relation,
        "result": relation_v2,
    }));

    let relation_v2_verified = run(
        &mut engine,
        actor,
        normal_request(
            case,
            "verify-relation-v2",
            actor,
            "[scope=virtual-lab]Pico is monitored?",
        ),
    );
    assert_eq!(relation_v2_verified.lifecycle, Lifecycle::Commit);
    assert!(
        relation_v2_verified
            .dependencies
            .contains(relation_v2_fact_id)
    );
    observations.push(json!({
        "request": "[scope=virtual-lab]Pico is monitored?",
        "verifies_version_two_premise": true,
        "result": relation_v2_verified,
    }));

    let final_state = engine
        .latest(actor, case)
        .expect("read final case revision")
        .expect("final case revision exists");
    assert!(final_state.prior_results.contains_key(&calculation_v1.id));
    assert!(final_state.prior_results.contains_key(&calculation_v2.id));
    assert!(final_state.prior_results.contains_key(&relation_v1.id));
    assert!(final_state.prior_results.contains_key(&relation_v2.id));
    let evidence = json!({
        "observation": "MB-BUILD-20261114 core construction observation four",
        "entry": "in-process minidora_engine::Engine::execute",
        "actor": actor,
        "case_id": case,
        "version_contract": "input_id + input_version; accepted later versions are new immutable sealed inputs and case revisions; prior committed ResultRecords remain in prior_results",
        "calculation_v1_result_id": calculation_v1.id,
        "calculation_v2_result_id": calculation_v2.id,
        "relation_v1_result_id": relation_v1.id,
        "relation_v2_result_id": relation_v2.id,
        "observations": observations,
        "final_case_revision": final_state.revision,
        "retained_result_ids": final_state.prior_results.keys().collect::<Vec<_>>(),
        "external_semantic_generation": false,
        "natural_language_input_parser_used": true,
        "limitations": [
            "isolated in-process Engine observation only",
            "no claim of full workspace runtime, external quality review, or A01/A03/R2-17 acceptance"
        ],
    });
    println!(
        "B13_CORE_OBSERVATION_04={}",
        serde_json::to_string(&evidence).expect("serialize direct Core observation")
    );

    drop(store);
    drop(engine);
    fs::remove_dir_all(directory).expect("remove isolated Core observation state");
}
