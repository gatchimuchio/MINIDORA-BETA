use minidora_domain::{Guarantee, InputBundle, Intent, Lifecycle, RequestEnvelope, SourceSpan};
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
            "minidora-input-machine-observation-five-{}-{serial}",
            std::process::id()
        ));
        fs::create_dir(&directory).expect("create isolated exact-role state");
        Self(directory)
    }
}

impl Drop for TempState {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn exact_role(
    bundle: &InputBundle,
    role: &str,
    expected: &str,
    expected_start: usize,
) -> SourceSpan {
    let spans = bundle
        .meaning
        .source_roles
        .get(role)
        .unwrap_or_else(|| panic!("missing source role {role}"));
    let matching = spans
        .iter()
        .filter(|span| span.text == expected)
        .collect::<Vec<_>>();
    assert_eq!(matching.len(), 1, "ambiguous {role} span for {expected:?}");
    let span = matching[0];
    assert_eq!(span.start, expected_start, "wrong {role} source occurrence");
    assert_eq!(
        bundle.request.text.get(span.start..span.end),
        Some(expected)
    );
    span.clone()
}

#[test]
fn repeated_nfkc_equivalent_assertion_values_keep_their_structural_source_roles() {
    let state = TempState::create();
    let actor = "b22-exact-source-role-observation";
    let request = RequestEnvelope::new(
        "repeated-normalized-relation",
        "repeated-normalized-input",
        actor,
        "[scope=ｶﾞ]ｶﾞはｶﾞではない",
    );
    let mut engine = Engine::open(&state.0).expect("open ordinary Core route");
    let result = engine
        .execute(actor, request.clone())
        .expect("execute negative relation through Engine");
    assert_eq!(result.lifecycle, Lifecycle::Commit);
    assert_eq!(result.guarantee, Guarantee::UserPremise);

    let first_case = engine
        .latest(actor, &request.case_id)
        .expect("read sealed case")
        .expect("relation case exists");
    let bundle = first_case.bundle.as_ref().expect("sealed input bundle");
    assert!(matches!(
        &bundle.meaning.intent,
        Intent::Assert { proposition }
            if proposition.subject == "ｶﾞ"
                && proposition.object == "ｶﾞ"
                && proposition.negative
                && proposition.scope == "ｶﾞ"
    ));

    let scope_start = "[scope=".len();
    let scope = exact_role(bundle, "/Assert/proposition/scope", "ｶﾞ", scope_start);
    let subject_start = bundle.request.text.find(']').unwrap() + 1;
    let subject = exact_role(bundle, "/Assert/proposition/subject", "ｶﾞ", subject_start);
    let predicate_start = subject.end;
    let predicate = exact_role(
        bundle,
        "/Assert/proposition/predicate",
        "は",
        predicate_start,
    );
    let object_start = predicate.end;
    let object = exact_role(bundle, "/Assert/proposition/object", "ｶﾞ", object_start);
    let negation_start = object.end;
    let negation = exact_role(
        bundle,
        "/Assert/proposition/negation",
        "ではない",
        negation_start,
    );

    let implicit_scope_request = RequestEnvelope::new(
        "implicit-default-scope",
        "implicit-default-scope-input",
        actor,
        "defaultはdefaultではない",
    );
    let implicit_scope_result = engine
        .execute(actor, implicit_scope_request.clone())
        .expect("execute assertion with an implicit default scope");
    assert_eq!(implicit_scope_result.lifecycle, Lifecycle::Commit);
    let implicit_scope_case = engine
        .latest(actor, &implicit_scope_request.case_id)
        .expect("read implicit-scope case")
        .expect("implicit-scope case exists");
    let implicit_scope_bundle = implicit_scope_case
        .bundle
        .as_ref()
        .expect("implicit-scope input is sealed");
    assert!(matches!(
        &implicit_scope_bundle.meaning.intent,
        Intent::Assert { proposition }
            if proposition.subject == "default"
                && proposition.object == "default"
                && proposition.scope == "default"
    ));
    assert!(
        !implicit_scope_bundle
            .meaning
            .source_roles
            .contains_key("/Assert/proposition/scope"),
        "the parser-generated default must not be assigned a source location"
    );

    drop(engine);
    let mut reopened = Engine::open(&state.0).expect("reopen persisted source-role case");
    assert_eq!(
        reopened.execute(actor, request.clone()).unwrap(),
        result,
        "same input version must replay the exact stored result"
    );
    assert_eq!(
        reopened.latest(actor, &request.case_id).unwrap(),
        Some(first_case.clone()),
        "the exact role spans must remain in the saved CaseRevision"
    );
    assert_eq!(
        reopened
            .execute(actor, implicit_scope_request.clone())
            .unwrap(),
        implicit_scope_result
    );
    assert_eq!(
        reopened
            .latest(actor, &implicit_scope_request.case_id)
            .unwrap(),
        Some(implicit_scope_case.clone())
    );

    println!(
        "B22_INPUT_MACHINE_OBSERVATION_05={}",
        serde_json::to_string(&json!({
            "entry": "in-process minidora_engine::Engine::execute",
            "source": bundle.request.text,
            "parser_version": bundle.meaning.parser_version,
            "lifecycle": result.lifecycle,
            "guarantee": result.guarantee,
            "roles": {
                "scope": scope,
                "subject": subject,
                "predicate": predicate,
                "object": object,
                "negation": negation
            },
            "same_normalized_value_has_distinct_original_byte_spans": true,
            "saved_result_and_case_revision_replay_after_reopen": true,
            "implicit_default_scope_has_no_fabricated_source_span": !implicit_scope_bundle.meaning.source_roles.contains_key("/Assert/proposition/scope"),
            "implicit_default_case_replayed_after_reopen": true,
            "source_roles_are_proof_inputs": false,
            "external_network_or_side_effects": false
        }))
        .expect("serialize exact source-role observation")
    );
}
