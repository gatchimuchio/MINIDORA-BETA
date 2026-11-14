use minidora_domain::{InputBundle, Intent, Lifecycle, RequestEnvelope};
use minidora_engine::Engine;
use serde_json::json;
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_TEMP_DIRECTORY: AtomicU64 = AtomicU64::new(0);

struct TempState(PathBuf);

impl TempState {
    fn create() -> Self {
        let serial = NEXT_TEMP_DIRECTORY.fetch_add(1, Ordering::Relaxed);
        let directory = std::env::temp_dir().join(format!(
            "minidora-input-machine-observation-three-{}-{serial}",
            std::process::id()
        ));
        fs::create_dir(&directory).expect("create isolated input provenance state");
        Self(directory)
    }
}

impl Drop for TempState {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn exact_role(bundle: &InputBundle, role: &str, expected: &str) {
    let spans = bundle
        .meaning
        .source_roles
        .get(role)
        .unwrap_or_else(|| panic!("missing source role {role}"));
    let exact = spans
        .iter()
        .filter(|span| span.text == expected)
        .collect::<Vec<_>>();
    assert_eq!(exact.len(), 1, "expected one {role} span for {expected}");
    let span = exact[0];
    assert_eq!(
        bundle.request.text.get(span.start..span.end),
        Some(expected)
    );
}

#[test]
fn all_intent_string_leaves_keep_exact_source_ranges_and_replay() {
    let state = TempState::create();
    let actor = "b19-source-span-observation";
    let mut engine = Engine::open(&state.0).expect("open ordinary Core route");

    let statistics_request = RequestEnvelope::new(
        "statistics-source-map",
        "statistics-input",
        actor,
        "1,10,3 の平均と分散を計算して",
    );
    let statistics_result = engine
        .execute(actor, statistics_request.clone())
        .expect("execute statistics through Engine");
    assert_eq!(statistics_result.lifecycle, Lifecycle::Commit);
    let statistics_case = engine
        .latest(actor, &statistics_request.case_id)
        .expect("read sealed statistics case")
        .expect("statistics case exists");
    let statistics_bundle = statistics_case
        .bundle
        .as_ref()
        .expect("sealed statistics input");
    assert!(matches!(
        &statistics_bundle.meaning.intent,
        Intent::Statistics { values, units: None }
            if values == &["1", "10", "3"]
    ));
    assert_eq!(
        statistics_bundle.meaning.source_roles["/Statistics/values/0"]
            .iter()
            .map(|span| span.text.as_str())
            .collect::<Vec<_>>(),
        vec!["1"],
        "the value 1 must not capture the prefix of 10"
    );
    exact_role(statistics_bundle, "/Statistics/values/1", "10");
    exact_role(statistics_bundle, "/Statistics/values/2", "3");

    let document = r#"{"numbers":[1,2]}"#;
    let json_request = RequestEnvelope::new(
        "json-document-source-map",
        "json-input",
        actor,
        &format!("json-pointer /numbers\n{document}"),
    );
    let json_result = engine
        .execute(actor, json_request.clone())
        .expect("execute JSON pointer through Engine");
    assert_eq!(json_result.lifecycle, Lifecycle::Commit);
    let json_case = engine
        .latest(actor, &json_request.case_id)
        .expect("read sealed JSON case")
        .expect("JSON case exists");
    let json_bundle = json_case.bundle.as_ref().expect("sealed JSON input");
    assert!(matches!(
        &json_bundle.meaning.intent,
        Intent::JsonPointer { document: parsed, pointer }
            if parsed == document && pointer == "/numbers"
    ));
    exact_role(json_bundle, "/JsonPointer/document", document);
    exact_role(json_bundle, "/JsonPointer/pointer", "/numbers");

    let unsupported_locale_reason =
        "semantic verification supports only the declared Japanese/English grammar";
    let mut unknown_request = RequestEnvelope::new(
        "unknown-source-map",
        "unknown-input",
        actor,
        unsupported_locale_reason,
    );
    unknown_request.locale = "fr".into();
    let unknown_result = engine
        .execute(actor, unknown_request.clone())
        .expect("execute unsupported locale through Engine");
    assert_eq!(unknown_result.lifecycle, Lifecycle::Suspend);
    let unknown_case = engine
        .latest(actor, &unknown_request.case_id)
        .expect("read sealed unknown case")
        .expect("unknown case exists");
    let unknown_bundle = unknown_case.bundle.as_ref().expect("sealed unknown input");
    assert!(matches!(
        &unknown_bundle.meaning.intent,
        Intent::Unknown { reason } if reason == unsupported_locale_reason
    ));
    assert!(
        unknown_bundle.meaning.source_roles.is_empty(),
        "generated Unknown reason text must not become a source claim"
    );

    let repeated_numeric_values = (1..=350)
        .map(|length| "1".repeat(length))
        .collect::<Vec<_>>()
        .join(",");
    let overlap_request = RequestEnvelope::new(
        "overlap-source-map-limit",
        "overlap-input",
        actor,
        &format!("統計 [{repeated_numeric_values}]"),
    );
    let overlap_result = engine
        .execute(actor, overlap_request.clone())
        .expect("overlap limit returns a typed result");
    assert_eq!(overlap_result.lifecycle, Lifecycle::Suspend);
    assert!(
        overlap_result
            .reason
            .as_deref()
            .is_some_and(|reason| reason.contains("SOURCE_SPAN_INDEX_LIMIT"))
    );
    let overlap_case = engine
        .latest(actor, &overlap_request.case_id)
        .expect("read resource-limited sealed case")
        .expect("resource-limited case exists");
    let overlap_bundle = overlap_case
        .bundle
        .as_ref()
        .expect("resource-limited input remains sealed");
    assert!(overlap_bundle.meaning.source_roles.is_empty());
    assert!(
        overlap_bundle
            .meaning
            .unresolved
            .iter()
            .any(|reason| reason.starts_with("source role indexing stopped:"))
    );

    drop(engine);
    let mut reopened = Engine::open(&state.0).expect("reopen Core state");
    assert_eq!(
        reopened.execute(actor, statistics_request.clone()).unwrap(),
        statistics_result
    );
    assert_eq!(
        reopened.latest(actor, &statistics_request.case_id).unwrap(),
        Some(statistics_case.clone())
    );
    assert_eq!(
        reopened.execute(actor, json_request.clone()).unwrap(),
        json_result
    );
    assert_eq!(
        reopened.latest(actor, &json_request.case_id).unwrap(),
        Some(json_case.clone())
    );
    assert_eq!(
        reopened.execute(actor, unknown_request.clone()).unwrap(),
        unknown_result
    );
    assert_eq!(
        reopened.latest(actor, &unknown_request.case_id).unwrap(),
        Some(unknown_case.clone())
    );
    assert_eq!(
        reopened.execute(actor, overlap_request.clone()).unwrap(),
        overlap_result
    );
    assert_eq!(
        reopened.latest(actor, &overlap_request.case_id).unwrap(),
        Some(overlap_case.clone())
    );

    println!(
        "B20_INPUT_MACHINE_OBSERVATION_03={}",
        serde_json::to_string(&json!({
            "entry": "in-process minidora_engine::Engine::execute",
            "parser_version": statistics_bundle.meaning.parser_version,
            "statistics_source_roles": statistics_bundle.meaning.source_roles,
            "json_source_roles": json_bundle.meaning.source_roles,
            "unknown_source_roles_empty": unknown_bundle.meaning.source_roles.is_empty(),
            "unknown_lifecycle": unknown_result.lifecycle,
            "overlap_limit_lifecycle": overlap_result.lifecycle,
            "overlap_limit_reason": overlap_result.reason,
            "all_four_saved_cases_replayed_exactly": true,
            "external_network_or_side_effects": false
        }))
        .expect("serialize source-span observation")
    );
}
