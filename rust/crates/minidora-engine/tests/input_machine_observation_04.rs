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
            "minidora-input-machine-observation-four-{}-{serial}",
            std::process::id()
        ));
        fs::create_dir(&directory).expect("create isolated Unicode provenance state");
        Self(directory)
    }
}

impl Drop for TempState {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn exact_role<'a>(
    bundle: &'a InputBundle,
    role: &str,
    expected: &str,
) -> &'a minidora_domain::SourceSpan {
    let spans = bundle
        .meaning
        .source_roles
        .get(role)
        .unwrap_or_else(|| panic!("missing source role {role}"));
    let matching = spans
        .iter()
        .filter(|span| span.text == expected)
        .collect::<Vec<_>>();
    assert_eq!(matching.len(), 1, "expected one {role} span for {expected}");
    let span = matching[0];
    assert_eq!(
        bundle.request.text.get(span.start..span.end),
        Some(expected)
    );
    span
}

#[test]
fn nfkc_composes_japanese_marks_but_maps_the_original_source_bytes() {
    let state = TempState::create();
    let actor = "b21-source-span-unicode-observation";
    let mut engine = Engine::open(&state.0).expect("open ordinary Core route");
    let request = RequestEnvelope::new("unicode-source-map", "unicode-input", actor, "ｶﾞはsensor");
    let result = engine
        .execute(actor, request.clone())
        .expect("execute decomposed Japanese relation through Engine");
    assert_eq!(result.lifecycle, Lifecycle::Commit);

    let first_case = engine
        .latest(actor, &request.case_id)
        .expect("read sealed Unicode case")
        .expect("Unicode case exists");
    let bundle = first_case.bundle.as_ref().expect("sealed Unicode input");
    assert!(matches!(
        &bundle.meaning.intent,
        Intent::Assert { proposition } if proposition.subject == "ｶﾞ"
    ));
    let subject = exact_role(bundle, "/Assert/proposition/subject", "ｶﾞ");
    assert_eq!(subject.end - subject.start, "ｶﾞ".len());

    let hangul_request =
        RequestEnvelope::new("hangul-source-map", "hangul-input", actor, "가はsensor");
    let hangul_result = engine
        .execute(actor, hangul_request.clone())
        .expect("execute Hangul Jamo relation through Engine");
    assert_eq!(hangul_result.lifecycle, Lifecycle::Commit);
    let hangul_case = engine
        .latest(actor, &hangul_request.case_id)
        .expect("read sealed Hangul case")
        .expect("Hangul case exists");
    let hangul_bundle = hangul_case.bundle.as_ref().expect("sealed Hangul input");
    assert!(matches!(
        &hangul_bundle.meaning.intent,
        Intent::Assert { proposition } if proposition.subject == "가"
    ));
    let hangul_subject = exact_role(hangul_bundle, "/Assert/proposition/subject", "가");
    assert_eq!(hangul_subject.end - hangul_subject.start, "가".len());

    drop(engine);
    let mut reopened = Engine::open(&state.0).expect("reopen persisted Unicode source span");
    assert_eq!(
        reopened.execute(actor, request.clone()).unwrap(),
        result,
        "same input version must replay the exact saved result"
    );
    assert_eq!(
        reopened.latest(actor, &request.case_id).unwrap(),
        Some(first_case.clone()),
        "the sealed original byte span must survive store reopen"
    );
    assert_eq!(
        reopened.execute(actor, hangul_request.clone()).unwrap(),
        hangul_result
    );
    assert_eq!(
        reopened.latest(actor, &hangul_request.case_id).unwrap(),
        Some(hangul_case.clone())
    );

    println!(
        "B21_INPUT_MACHINE_OBSERVATION_04={}",
        serde_json::to_string(&json!({
            "entry": "in-process minidora_engine::Engine::execute",
            "parser_version": bundle.meaning.parser_version,
            "original_subject": "ｶﾞ",
            "normalized_subject": "ガ",
            "source_span": subject,
            "halfwidth_katakana_source_bytes_match": true,
            "hangul_jamo_subject": "가",
            "hangul_normalized_subject": "가",
            "hangul_source_span": hangul_subject,
            "both_results_commit": result.lifecycle == Lifecycle::Commit && hangul_result.lifecycle == Lifecycle::Commit,
            "both_saved_cases_replayed_after_reopen": true,
            "external_network_or_side_effects": false
        }))
        .expect("serialize Unicode source-span observation")
    );
}
