use minidora_domain::{Guarantee, Intent, Lifecycle, RequestEnvelope};
use minidora_engine::Engine;
use serde_json::json;
use std::fs;
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_TEMP_DIRECTORY: AtomicU64 = AtomicU64::new(0);

#[test]
fn concise_arithmetic_question_reaches_core_once_and_replays_its_versioned_result() {
    let serial = NEXT_TEMP_DIRECTORY.fetch_add(1, Ordering::Relaxed);
    let directory = std::env::temp_dir().join(format!(
        "minidora-input-machine-observation-one-{}-{serial}",
        std::process::id()
    ));
    fs::create_dir(&directory).expect("create isolated input-machine state");

    let actor = "b17-input-machine-observation";
    let case = "input-machine-observation-one";
    let request = RequestEnvelope::new(case, "arithmetic-question", actor, "６＋７は？");
    let mut engine = Engine::open(&directory).expect("open Core for natural question");
    let result = engine
        .execute(actor, request.clone())
        .expect("execute exact arithmetic question");
    assert_eq!(result.lifecycle, Lifecycle::Commit);
    assert_eq!(result.input_version, 1);
    assert_eq!(result.guarantee, Guarantee::Computed);
    assert_eq!(
        result
            .calculation
            .as_ref()
            .map(|calculation| calculation.exact_value.as_str()),
        Some("13")
    );
    assert!(result.render.is_some());

    let first_case = engine
        .latest(actor, case)
        .expect("read first input case")
        .expect("first input was adopted");
    let bundle = first_case.bundle.as_ref().expect("one sealed input bundle");
    assert!(matches!(
        &bundle.meaning.intent,
        Intent::Calculate { expression } if expression == "6+7"
    ));
    assert_eq!(
        bundle.meaning.parser_version,
        "declarative-ja-en-v73-arithmetic-topic-question"
    );
    assert_eq!(bundle.request.input_version, 1);
    assert_eq!(bundle.request.text, "６＋７は？");
    assert_eq!(bundle.meaning.spans.len(), 1);
    assert_eq!(bundle.meaning.spans[0].start, 0);
    assert_eq!(bundle.meaning.spans[0].end, request.text.len());
    assert_eq!(bundle.meaning.spans[0].text, request.text);

    drop(engine);
    let mut reopened = Engine::open(&directory).expect("reopen persisted input state");
    let replay = reopened
        .execute(actor, request)
        .expect("replay same immutable input version");
    let final_case = reopened
        .latest(actor, case)
        .expect("read case after replay")
        .expect("case remains present");
    assert_eq!(replay, result);
    assert_eq!(final_case, first_case);

    let evidence = json!({
        "observation": "MB-BUILD-20261114 sixth instruction; input-machine observation 01",
        "entry": "in-process minidora_engine::Engine::execute",
        "actor": actor,
        "case_id": case,
        "source_text": bundle.request.text,
        "input_version": bundle.request.input_version,
        "parser_version": bundle.meaning.parser_version,
        "semantic_intent": bundle.meaning.intent,
        "source_hash": bundle.raw_sha256,
        "whole_source_utf8_byte_span": bundle.meaning.spans[0],
        "result_id": result.id,
        "lifecycle": result.lifecycle,
        "guarantee": result.guarantee,
        "calculation": result.calculation,
        "model_revision": result.content_plan.as_ref().map(|plan| plan.model_revision.clone()),
        "render_sha256": result.render.as_ref().map(|render| render.actual_sha256.clone()),
        "same_input_version_replayed_exact_saved_result": replay == result,
        "case_revision_unchanged_on_replay": final_case == first_case,
        "external_semantic_generation": false,
        "external_network_or_side_effects": false,
        "interpretation_scope": "exact arithmetic expression followed by Japanese topic particle は and a terminal question mark",
        "limitations": [
            "one bounded Japanese input form and one exact Core arithmetic route",
            "not broad Japanese language or open-domain question acceptance",
            "not full workspace runtime acceptance"
        ]
    });
    println!(
        "B17_INPUT_MACHINE_OBSERVATION_01={}",
        serde_json::to_string(&evidence).expect("serialize input-machine evidence")
    );

    drop(reopened);
    fs::remove_dir_all(directory).expect("remove isolated input-machine state");
}
