use minidora_domain::{Guarantee, Intent, Lifecycle, RequestEnvelope, SourceSpan};
use minidora_engine::Engine;
use minidora_engine::input::PARSER_VERSION;
use serde_json::json;
use std::fs;
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_TEMP_DIRECTORY: AtomicU64 = AtomicU64::new(0);

fn exact_role_span<'a>(
    source: &str,
    roles: &'a std::collections::BTreeMap<String, Vec<SourceSpan>>,
    role: &str,
    expected: &str,
) -> &'a SourceSpan {
    let span = roles[role]
        .iter()
        .find(|span| span.text == expected)
        .unwrap_or_else(|| panic!("missing {role} source span for {expected}"));
    assert_eq!(source.get(span.start..span.end), Some(expected));
    span
}

#[test]
fn parsed_relation_roles_keep_exact_source_ranges_across_persisted_input_replay() {
    let serial = NEXT_TEMP_DIRECTORY.fetch_add(1, Ordering::Relaxed);
    let directory = std::env::temp_dir().join(format!(
        "minidora-input-machine-observation-two-{}-{serial}",
        std::process::id()
    ));
    fs::create_dir(&directory).expect("create isolated input role state");

    let actor = "b18-input-role-source-observation";
    let case = "input-machine-observation-two";
    let request = RequestEnvelope::new(
        case,
        "negative-relation",
        actor,
        "[scope=materials-lab]Neraはsensorではない",
    );
    let mut engine = Engine::open(&directory).expect("open Core for relation input");
    let result = engine
        .execute(actor, request.clone())
        .expect("execute versioned relation input");
    assert_eq!(result.lifecycle, Lifecycle::Commit);
    assert_eq!(result.guarantee, Guarantee::UserPremise);
    assert_eq!(result.input_version, 1);

    let first_case = engine
        .latest(actor, case)
        .expect("read sealed relation case")
        .expect("relation case exists");
    let bundle = first_case
        .bundle
        .as_ref()
        .expect("relation request has an immutable bundle");
    assert!(matches!(
        &bundle.meaning.intent,
        Intent::Assert { proposition }
            if proposition.subject == "Nera"
                && proposition.object == "sensor"
                && proposition.negative
                && proposition.scope == "materials-lab"
    ));
    assert_eq!(bundle.meaning.parser_version, PARSER_VERSION);
    let source = &bundle.request.text;
    let roles = &bundle.meaning.source_roles;
    let subject = exact_role_span(source, roles, "/Assert/proposition/subject", "Nera");
    let predicate = exact_role_span(source, roles, "/Assert/proposition/predicate", "は");
    assert_eq!(roles["/Assert/proposition/predicate"].len(), 1);
    let object = exact_role_span(source, roles, "/Assert/proposition/object", "sensor");
    let scope = exact_role_span(source, roles, "/Assert/proposition/scope", "materials-lab");
    let negation = exact_role_span(source, roles, "/Assert/proposition/negation", "ではない");

    drop(engine);
    let mut reopened = Engine::open(&directory).expect("reopen persisted semantic spans");
    let replay = reopened
        .execute(actor, request)
        .expect("load the same sealed input version");
    let final_case = reopened
        .latest(actor, case)
        .expect("read persisted case after replay")
        .expect("persisted case remains present");
    assert_eq!(replay, result);
    assert_eq!(final_case, first_case);

    let evidence = json!({
        "observation": "MB-BUILD-20261114 sixth instruction; input-machine observation 02",
        "entry": "in-process minidora_engine::Engine::execute",
        "actor": actor,
        "case_id": case,
        "source_text": source,
        "input_version": bundle.request.input_version,
        "parser_version": bundle.meaning.parser_version,
        "semantic_intent": bundle.meaning.intent,
        "source_sha256": bundle.raw_sha256,
        "source_roles": roles,
        "result_id": result.id,
        "lifecycle": result.lifecycle,
        "guarantee": result.guarantee,
        "model_revision": result.content_plan.as_ref().map(|plan| plan.model_revision.clone()),
        "render_sha256": result.render.as_ref().map(|render| render.actual_sha256.clone()),
        "verified_role_ranges": {
            "subject": subject,
            "predicate": predicate,
            "object": object,
            "scope": scope,
            "negation": negation,
        },
        "exact_saved_bundle_replayed_after_engine_reopen": final_case == first_case,
        "exact_saved_result_replayed": replay == result,
        "external_semantic_generation": false,
        "external_network_or_side_effects": false,
        "limitations": [
            "string-valued Intent leaves are indexed when an exact lexical match is present",
            "each role retains at most 32 source matches and lexical matching does not prove semantic correctness",
            "unknown-intent reason text is excluded from source evidence"
        ]
    });
    println!(
        "B18_INPUT_MACHINE_OBSERVATION_02={}",
        serde_json::to_string(&evidence).expect("serialize input source map evidence")
    );

    drop(reopened);
    fs::remove_dir_all(directory).expect("remove isolated input role state");
}
