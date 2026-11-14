use minidora_domain::{
    Guarantee, Lifecycle, MemoryState, Proposition, RequestEnvelope, ResultRecord,
};
use minidora_engine::{Engine, store::Store};
use serde_json::json;
use std::fs;
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_TEMP_DIRECTORY: AtomicU64 = AtomicU64::new(0);

fn execute(
    engine: &mut Engine,
    actor: &str,
    case: &str,
    input_id: &str,
    text: &str,
) -> ResultRecord {
    engine
        .execute(actor, RequestEnvelope::new(case, input_id, actor, text))
        .unwrap_or_else(|error| panic!("Engine::execute failed for {input_id}: {error}"))
}

fn retain(
    observations: &mut Vec<serde_json::Value>,
    input_id: &str,
    text: &str,
    result: ResultRecord,
) {
    observations.push(json!({
        "input_id": input_id,
        "input": text,
        "result": result,
    }));
}

fn assert_missing(result: &ResultRecord, input_id: &str) {
    assert_eq!(result.lifecycle, Lifecycle::Suspend, "{input_id}");
    assert!(
        result
            .reason
            .as_deref()
            .unwrap_or_default()
            .contains("MISSING_PREMISES"),
        "{input_id}: {:?}",
        result.reason
    );
    assert!(
        result.content_plan.is_none(),
        "{input_id} adopted unsupported content"
    );
}

fn proposition(subject: &str, object: &str, negative: bool) -> Proposition {
    Proposition {
        subject: subject.into(),
        predicate: "is".into(),
        object: object.into(),
        negative,
        scope: "virtual-lab".into(),
    }
}

#[test]
fn direct_core_observation_two_separates_derived_conflicting_and_unmet_relations() {
    let serial = NEXT_TEMP_DIRECTORY.fetch_add(1, Ordering::Relaxed);
    let directory = std::env::temp_dir().join(format!(
        "minidora-core-construction-observation-two-{}-{serial}",
        std::process::id()
    ));
    fs::create_dir(&directory).expect("unique isolated observation state");
    let mut engine = Engine::open(&directory).expect("open isolated Core state");
    let primary_actor = "b11-primary-world";
    let scenario_actor = "b11-counterfactual-fixture";
    let case = "core-observation-two";
    let scenario_case = "core-observation-two-scenario";
    let rule_text =
        "If [scope=virtual-lab]X is a sensor and calibrated then [scope=virtual-lab]X is reliable";
    let mut observations = Vec::new();

    let rule = execute(&mut engine, primary_actor, case, "primary-rule", rule_text);
    assert_eq!(rule.lifecycle, Lifecycle::Commit);
    assert_eq!(rule.guarantee, Guarantee::UserPremise);
    let rule_id = rule.dependencies[0].clone();
    retain(&mut observations, "primary-rule", rule_text, rule);

    let sensor = execute(
        &mut engine,
        primary_actor,
        case,
        "nera-sensor",
        "[scope=virtual-lab]Nera is a sensor",
    );
    assert_eq!(sensor.lifecycle, Lifecycle::Commit);
    let sensor_id = sensor.dependencies[0].clone();
    retain(
        &mut observations,
        "nera-sensor",
        "[scope=virtual-lab]Nera is a sensor",
        sensor,
    );

    let calibrated = execute(
        &mut engine,
        primary_actor,
        case,
        "nera-calibrated",
        "[scope=virtual-lab]Nera is calibrated",
    );
    assert_eq!(calibrated.lifecycle, Lifecycle::Commit);
    let calibrated_id = calibrated.dependencies[0].clone();
    retain(
        &mut observations,
        "nera-calibrated",
        "[scope=virtual-lab]Nera is calibrated",
        calibrated,
    );

    let unrelated = execute(
        &mut engine,
        primary_actor,
        case,
        "nera-blue",
        "[scope=virtual-lab]Nera is blue",
    );
    assert_eq!(unrelated.lifecycle, Lifecycle::Commit);
    let unrelated_id = unrelated.dependencies[0].clone();
    retain(
        &mut observations,
        "nera-blue",
        "[scope=virtual-lab]Nera is blue",
        unrelated,
    );

    let supported = execute(
        &mut engine,
        primary_actor,
        case,
        "nera-reliable-supported",
        "[scope=virtual-lab]Nera is reliable?",
    );
    assert_eq!(supported.lifecycle, Lifecycle::Commit);
    assert_eq!(supported.guarantee, Guarantee::Derived);
    for expected in [&rule_id, &sensor_id, &calibrated_id] {
        assert!(
            supported.dependencies.contains(expected),
            "missing proof root {expected}"
        );
    }
    retain(
        &mut observations,
        "nera-reliable-supported",
        "[scope=virtual-lab]Nera is reliable?",
        supported,
    );

    let scenario_rule = execute(
        &mut engine,
        scenario_actor,
        scenario_case,
        "scenario-rule",
        rule_text,
    );
    assert_eq!(scenario_rule.lifecycle, Lifecycle::Commit);
    retain(&mut observations, "scenario-rule", rule_text, scenario_rule);
    let scenario_sensor = execute(
        &mut engine,
        scenario_actor,
        scenario_case,
        "scenario-nera-sensor",
        "[scope=virtual-lab]Nera is a sensor",
    );
    assert_eq!(scenario_sensor.lifecycle, Lifecycle::Commit);
    retain(
        &mut observations,
        "scenario-nera-sensor",
        "[scope=virtual-lab]Nera is a sensor",
        scenario_sensor,
    );
    let not_calibrated = execute(
        &mut engine,
        scenario_actor,
        scenario_case,
        "scenario-nera-not-calibrated",
        "[scope=virtual-lab]Nera is not calibrated",
    );
    assert_eq!(not_calibrated.lifecycle, Lifecycle::Commit);
    assert_eq!(not_calibrated.guarantee, Guarantee::UserPremise);
    retain(
        &mut observations,
        "scenario-nera-not-calibrated",
        "[scope=virtual-lab]Nera is not calibrated",
        not_calibrated,
    );
    let scenario_unmet = execute(
        &mut engine,
        scenario_actor,
        scenario_case,
        "scenario-nera-reliable",
        "[scope=virtual-lab]Nera is reliable?",
    );
    assert_missing(&scenario_unmet, "scenario-nera-reliable");
    retain(
        &mut observations,
        "scenario-nera-reliable",
        "[scope=virtual-lab]Nera is reliable?",
        scenario_unmet,
    );

    let primary_after_scenario = execute(
        &mut engine,
        primary_actor,
        case,
        "nera-reliable-after-scenario",
        "[scope=virtual-lab]Nera is reliable?",
    );
    assert_eq!(primary_after_scenario.lifecycle, Lifecycle::Commit);
    assert_eq!(primary_after_scenario.guarantee, Guarantee::Derived);
    retain(
        &mut observations,
        "nera-reliable-after-scenario",
        "[scope=virtual-lab]Nera is reliable?",
        primary_after_scenario,
    );

    let not_reliable = execute(
        &mut engine,
        primary_actor,
        case,
        "nera-counterexample",
        "[scope=virtual-lab]Nera is not reliable",
    );
    assert_eq!(not_reliable.lifecycle, Lifecycle::Commit);
    let counterexample_id = not_reliable.dependencies[0].clone();
    retain(
        &mut observations,
        "nera-counterexample",
        "[scope=virtual-lab]Nera is not reliable",
        not_reliable,
    );
    let conflict = execute(
        &mut engine,
        primary_actor,
        case,
        "nera-reliable-conflict",
        "[scope=virtual-lab]Nera is reliable?",
    );
    assert_eq!(conflict.lifecycle, Lifecycle::Suspend);
    assert!(
        conflict
            .reason
            .as_deref()
            .unwrap_or_default()
            .contains("CONFLICTING_PREMISES")
    );
    assert!(conflict.content_plan.is_none());
    assert!(conflict.dependencies.is_empty());
    retain(
        &mut observations,
        "nera-reliable-conflict",
        "[scope=virtual-lab]Nera is reliable?",
        conflict,
    );

    let unrelated_after_conflict = execute(
        &mut engine,
        primary_actor,
        case,
        "nera-blue-after-conflict",
        "[scope=virtual-lab]Nera is blue?",
    );
    assert_eq!(unrelated_after_conflict.lifecycle, Lifecycle::Commit);
    assert_eq!(unrelated_after_conflict.guarantee, Guarantee::UserPremise);
    assert_eq!(unrelated_after_conflict.dependencies.len(), 1);
    assert_eq!(unrelated_after_conflict.dependencies[0], unrelated_id);
    retain(
        &mut observations,
        "nera-blue-after-conflict",
        "[scope=virtual-lab]Nera is blue?",
        unrelated_after_conflict,
    );

    let incomplete_sensor = execute(
        &mut engine,
        primary_actor,
        case,
        "tovi-sensor",
        "[scope=virtual-lab]Tovi is a sensor",
    );
    assert_eq!(incomplete_sensor.lifecycle, Lifecycle::Commit);
    retain(
        &mut observations,
        "tovi-sensor",
        "[scope=virtual-lab]Tovi is a sensor",
        incomplete_sensor,
    );
    let incomplete = execute(
        &mut engine,
        primary_actor,
        case,
        "tovi-reliable",
        "[scope=virtual-lab]Tovi is reliable?",
    );
    assert_missing(&incomplete, "tovi-reliable");
    retain(
        &mut observations,
        "tovi-reliable",
        "[scope=virtual-lab]Tovi is reliable?",
        incomplete,
    );

    let hypothetical_sensor = execute(
        &mut engine,
        scenario_actor,
        scenario_case,
        "scenario-tovi-sensor",
        "[scope=virtual-lab]Tovi is a sensor",
    );
    assert_eq!(hypothetical_sensor.lifecycle, Lifecycle::Commit);
    retain(
        &mut observations,
        "scenario-tovi-sensor",
        "[scope=virtual-lab]Tovi is a sensor",
        hypothetical_sensor,
    );
    let hypothetical_negative_condition = execute(
        &mut engine,
        scenario_actor,
        scenario_case,
        "scenario-tovi-not-calibrated",
        "[scope=virtual-lab]Tovi is not calibrated",
    );
    assert_eq!(hypothetical_negative_condition.lifecycle, Lifecycle::Commit);
    assert_eq!(
        hypothetical_negative_condition.guarantee,
        Guarantee::UserPremise
    );
    retain(
        &mut observations,
        "scenario-tovi-not-calibrated",
        "[scope=virtual-lab]Tovi is not calibrated",
        hypothetical_negative_condition,
    );
    let hypothetical_unmet = execute(
        &mut engine,
        scenario_actor,
        scenario_case,
        "scenario-tovi-reliable",
        "[scope=virtual-lab]Tovi is reliable?",
    );
    assert_missing(&hypothetical_unmet, "scenario-tovi-reliable");
    retain(
        &mut observations,
        "scenario-tovi-reliable",
        "[scope=virtual-lab]Tovi is reliable?",
        hypothetical_unmet,
    );

    let unsupported_counterfactual_text =
        "If [scope=virtual-lab]Nera is not calibrated then [scope=virtual-lab]Nera is reliable?";
    let unsupported_counterfactual = execute(
        &mut engine,
        primary_actor,
        case,
        "counterfactual-question",
        unsupported_counterfactual_text,
    );
    assert_eq!(unsupported_counterfactual.lifecycle, Lifecycle::Suspend);
    assert!(
        unsupported_counterfactual
            .reason
            .as_deref()
            .unwrap_or_default()
            .contains("CAPABILITY_UNAVAILABLE")
    );
    assert!(unsupported_counterfactual.content_plan.is_none());
    retain(
        &mut observations,
        "counterfactual-question",
        unsupported_counterfactual_text,
        unsupported_counterfactual,
    );

    let counterfactual_actor = "b11-typed-counterfactual";
    let counterfactual_case = "core-observation-two-counterfactual";
    let counterfactual_rule_text = "If [scope=virtual-lab]X is a sensor and not calibrated then [scope=virtual-lab]X is not reliable";
    let counterfactual_rule = execute(
        &mut engine,
        counterfactual_actor,
        counterfactual_case,
        "counterfactual-rule",
        counterfactual_rule_text,
    );
    assert_eq!(counterfactual_rule.lifecycle, Lifecycle::Commit);
    let counterfactual_rule_id = counterfactual_rule.dependencies[0].clone();
    let counterfactual_sensor = execute(
        &mut engine,
        counterfactual_actor,
        counterfactual_case,
        "counterfactual-nera-sensor",
        "[scope=virtual-lab]Nera is a sensor",
    );
    assert_eq!(counterfactual_sensor.lifecycle, Lifecycle::Commit);
    let counterfactual_sensor_id = counterfactual_sensor.dependencies[0].clone();
    let counterfactual_calibrated = execute(
        &mut engine,
        counterfactual_actor,
        counterfactual_case,
        "counterfactual-nera-calibrated",
        "[scope=virtual-lab]Nera is calibrated",
    );
    assert_eq!(counterfactual_calibrated.lifecycle, Lifecycle::Commit);
    let counterfactual_calibrated_id = counterfactual_calibrated.dependencies[0].clone();
    let counterfactual_blue = execute(
        &mut engine,
        counterfactual_actor,
        counterfactual_case,
        "counterfactual-nera-blue",
        "[scope=virtual-lab]Nera is blue",
    );
    assert_eq!(counterfactual_blue.lifecycle, Lifecycle::Commit);
    let counterfactual_blue_id = counterfactual_blue.dependencies[0].clone();
    let baseline_not_reliable = execute(
        &mut engine,
        counterfactual_actor,
        counterfactual_case,
        "counterfactual-baseline-not-reliable",
        "[scope=virtual-lab]Nera is not reliable?",
    );
    assert_missing(
        &baseline_not_reliable,
        "counterfactual-baseline-not-reliable",
    );

    let counterfactual_store = Store::open(&directory).expect("read cloned scenario state");
    let counterfactual_memory_before = counterfactual_store
        .load_memory(counterfactual_actor)
        .expect("counterfactual actor memory before scenario");
    let assumption = proposition("Nera", "calibrated", true);
    let target = proposition("Nera", "reliable", true);
    let counterfactual_report = engine
        .evaluate_counterfactual_relation(
            counterfactual_actor,
            counterfactual_case,
            std::slice::from_ref(&assumption),
            std::slice::from_ref(&counterfactual_calibrated_id),
            &target,
            128,
        )
        .expect("evaluate explicit typed counterfactual");
    assert_eq!(
        counterfactual_report.status,
        minidora_engine::core::CounterfactualRelationStatus::Supported
    );
    assert_eq!(counterfactual_report.guarantee, Guarantee::Hypothesis);
    assert_eq!(counterfactual_report.hypothesis_ids.len(), 1);
    assert_eq!(counterfactual_report.hypothesis_ids_used.len(), 1);
    assert!(
        counterfactual_report
            .dependencies
            .contains(&counterfactual_rule_id)
    );
    assert!(
        counterfactual_report
            .dependencies
            .contains(&counterfactual_sensor_id)
    );
    assert!(
        !counterfactual_report
            .dependencies
            .contains(&counterfactual_calibrated_id)
    );
    assert!(
        counterfactual_report
            .dependencies
            .contains(&counterfactual_report.hypothesis_ids[0])
    );
    assert_eq!(counterfactual_report.invalidated_record_ids.len(), 1);
    assert_eq!(
        counterfactual_report.invalidated_record_ids[0],
        counterfactual_calibrated_id
    );
    assert!(
        !counterfactual_report
            .invalidated_record_ids
            .contains(&counterfactual_rule_id)
    );
    assert!(
        !counterfactual_report
            .invalidated_record_ids
            .contains(&counterfactual_sensor_id)
    );
    assert!(
        !counterfactual_report
            .invalidated_record_ids
            .contains(&counterfactual_blue_id)
    );
    let counterfactual_memory_after = counterfactual_store
        .load_memory(counterfactual_actor)
        .expect("counterfactual actor memory after scenario");
    assert_eq!(counterfactual_memory_before, counterfactual_memory_after);
    assert!(
        counterfactual_memory_after
            .facts
            .iter()
            .all(|fact| fact.guarantee != Guarantee::Hypothesis)
    );
    let blue_after_counterfactual = execute(
        &mut engine,
        counterfactual_actor,
        counterfactual_case,
        "counterfactual-nera-blue-after",
        "[scope=virtual-lab]Nera is blue?",
    );
    assert_eq!(blue_after_counterfactual.lifecycle, Lifecycle::Commit);
    assert_eq!(blue_after_counterfactual.guarantee, Guarantee::UserPremise);
    assert_eq!(blue_after_counterfactual.dependencies.len(), 1);
    assert_eq!(
        blue_after_counterfactual.dependencies[0],
        counterfactual_blue_id
    );

    let conflicting_counterfactual = engine
        .evaluate_counterfactual_relation(
            counterfactual_actor,
            counterfactual_case,
            std::slice::from_ref(&assumption),
            &[],
            &target,
            128,
        )
        .expect("return a typed stop for an unsuperseded opposing premise");
    assert_eq!(
        conflicting_counterfactual.status,
        minidora_engine::core::CounterfactualRelationStatus::Conflict
    );
    assert_eq!(conflicting_counterfactual.guarantee, Guarantee::Unknown);
    let invalid_override = engine.evaluate_counterfactual_relation(
        counterfactual_actor,
        counterfactual_case,
        &[proposition("Nera", "blue", true)],
        std::slice::from_ref(&counterfactual_calibrated_id),
        &target,
        128,
    );
    assert!(
        invalid_override
            .expect_err("an unrelated fact cannot be superseded")
            .to_string()
            .contains("COUNTERFACTUAL_OVERRIDE_INVALID")
    );
    let normal_after_counterfactual = execute(
        &mut engine,
        counterfactual_actor,
        counterfactual_case,
        "counterfactual-normal-after",
        "[scope=virtual-lab]Nera is not reliable?",
    );
    assert_missing(&normal_after_counterfactual, "counterfactual-normal-after");
    let final_counterfactual_memory = counterfactual_store
        .load_memory(counterfactual_actor)
        .expect("counterfactual actor memory after all isolated scenarios");
    assert_eq!(counterfactual_memory_before, final_counterfactual_memory);

    let store = Store::open(&directory).expect("open Core store for final memory snapshot");
    let primary_memory: MemoryState = store
        .load_memory(primary_actor)
        .expect("primary actor memory");
    let scenario_memory: MemoryState = store
        .load_memory(scenario_actor)
        .expect("scenario actor memory");
    assert_eq!(primary_memory.rules.len(), 1);
    assert_eq!(scenario_memory.rules.len(), 1);
    assert!(
        primary_memory
            .facts
            .iter()
            .all(|fact| fact.guarantee == Guarantee::UserPremise)
    );
    assert!(
        scenario_memory
            .facts
            .iter()
            .all(|fact| fact.guarantee == Guarantee::UserPremise)
    );
    assert!(
        primary_memory
            .facts
            .iter()
            .any(|fact| fact.id == counterexample_id)
    );
    assert!(
        !primary_memory
            .facts
            .iter()
            .any(|fact| fact.id == calibrated_id && !fact.valid)
    );
    let evidence = json!({
        "entry": "in-process minidora_engine::Engine::execute",
        "primary_actor": primary_actor,
        "scenario_actor": scenario_actor,
        "case_id": case,
        "rule_id": rule_id,
        "observations": observations,
        "final_memory": {
            "primary": primary_memory,
            "separate_scenario": scenario_memory,
        },
        "unrelated_fact_id": unrelated_id,
        "scenario_isolation": "The second actor is a test fixture and remains USER_PREMISE in Core memory; it is not a first-class HYPOTHESIS or counterfactual intent.",
        "counterfactual_language_support": "CAPABILITY_UNAVAILABLE; no counterfactual result was adopted",
        "typed_counterfactual": {
            "rule_input": counterfactual_rule_text,
            "assumption": assumption,
            "superseded_fact_id": counterfactual_calibrated_id,
            "target": target,
            "baseline_not_reliable": baseline_not_reliable,
            "report": counterfactual_report,
            "conflicting_assumption_report": conflicting_counterfactual,
            "unrelated_fact_id": counterfactual_blue_id,
            "memory_before": counterfactual_memory_before,
            "memory_after": final_counterfactual_memory,
            "normal_query_after": normal_after_counterfactual,
            "invalid_override": "COUNTERFACTUAL_OVERRIDE_INVALID",
        },
    });
    println!(
        "B11_CORE_OBSERVATION_02={}",
        serde_json::to_string(&evidence).expect("serialize direct Core observation")
    );

    drop(store);
    drop(engine);
    fs::remove_dir_all(directory).expect("remove isolated Core observation state");
}
