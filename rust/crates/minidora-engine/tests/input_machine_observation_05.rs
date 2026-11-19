use minidora_domain::{
    ConditionDefinition, ConditionExpression, Guarantee, InputBundle, Intent, Lifecycle,
    RequestEnvelope, SourceSpan,
};
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

#[test]
fn qualified_japanese_explanation_consumes_exact_time_condition_and_source_roles() {
    let state = TempState::create();
    let actor = "input-source";
    let mut engine = Engine::open(&state.0).unwrap();
    let condition_id = "calibrated";
    let start = "2026-11-19T08:00:00Z";
    let end = "2026-11-19T09:00:00Z";
    let definition = engine
        .define_condition(
            actor,
            RequestEnvelope::new("definition", "condition", actor, ""),
            ConditionDefinition {
                condition_id: condition_id.into(),
                subject: "$subject".into(),
                scope: "鳥".into(),
                expression: ConditionExpression::Fact {
                    predicate: "is".into(),
                    object: "校正済み".into(),
                },
                valid_from: Some(start.into()),
                valid_until: Some(end.into()),
                dependencies: Default::default(),
            },
        )
        .unwrap();
    assert_eq!(definition.lifecycle, Lifecycle::Commit);
    let evidence = engine
        .execute(
            actor,
            RequestEnvelope::new(
                "source-evidence",
                "available",
                actor,
                &format!("[time={start}][scope=鳥]ネラは校正済み"),
            ),
        )
        .unwrap();
    let premise = engine
        .execute(
            actor,
            RequestEnvelope::new(
                "premise",
                "negative",
                actor,
                &format!("[scope=鳥][condition={condition_id}][time={start}]ネラは鳥ではない"),
            ),
        )
        .unwrap();
    assert_eq!(evidence.lifecycle, Lifecycle::Commit);
    assert_eq!(premise.lifecycle, Lifecycle::Commit);
    let store = minidora_engine::store::Store::open(&state.0).unwrap();
    let memory = store.load_memory(actor).unwrap();
    for result in [&evidence, &premise] {
        let fact = memory
            .facts
            .iter()
            .find(|f| f.id == result.dependencies[0])
            .unwrap();
        let source = fact.source_evidence.as_ref().unwrap();
        assert_eq!(
            source.source_roles["/Assert/proposition/temporal_scope"][0].text,
            start
        );
    }

    let text =
        format!("　なぜ [scope=鳥][condition={condition_id}][time={start}]ネラは鳥ですか？　");
    let request = RequestEnvelope::new("explanation", "qualified", actor, &text);
    let result = engine.execute(actor, request.clone()).unwrap();
    assert_eq!(result.lifecycle, Lifecycle::Commit, "{result:?}");
    assert_eq!(result.guarantee, Guarantee::UserPremise);
    for id in [
        &definition.dependencies[0],
        &evidence.dependencies[0],
        &premise.dependencies[0],
    ] {
        assert!(
            result.dependencies.contains(id),
            "missing proof dependency {id}"
        );
    }
    let case = engine.latest(actor, "explanation").unwrap().unwrap();
    let bundle = case.bundle.as_ref().unwrap();
    assert_eq!(bundle.request.text, text);
    assert_eq!(bundle.raw_sha256, minidora_domain::digest(text.as_bytes()));
    assert!(matches!(
        &bundle.meaning.intent,
        Intent::AskWhy { proposition } if proposition.subject == "ネラ"
            && proposition.scope == "鳥" && proposition.object == "鳥"
            && proposition.temporal_scope.as_deref() == Some(start)
            && proposition.condition_refs.contains(condition_id)
    ));
    exact_role(
        bundle,
        "/AskWhy/proposition/scope",
        "鳥",
        text.find('鳥').unwrap(),
    );
    exact_role(
        bundle,
        "/AskWhy/proposition/object",
        "鳥",
        text.rfind('鳥').unwrap(),
    );
    exact_role(
        bundle,
        "/AskWhy/proposition/condition_refs/0",
        condition_id,
        text.find(condition_id).unwrap(),
    );
    exact_role(
        bundle,
        "/AskWhy/proposition/temporal_scope",
        start,
        text.find(start).unwrap(),
    );
    let plan = result.content_plan.as_ref().unwrap();
    let compute_ref = plan.dependencies.last().unwrap();
    let artifact: serde_json::Value =
        serde_json::from_slice(&store.get_bytes(actor, "explanation", compute_ref).unwrap())
            .unwrap();
    let meaning = &artifact["relation_meaning_plan"];
    assert_eq!(meaning["answer_status"], "CONTRADICTED");
    assert_eq!(meaning["conclusion"]["negative"], true);
    assert_eq!(meaning["conclusion"]["object"], "鳥");
    assert_eq!(meaning["conclusion"]["temporal_scope"], start);
    assert_eq!(artifact["content_generation"]["text"], result.text);
    assert_eq!(
        result.render.as_ref().unwrap().bytes,
        result.text.as_bytes()
    );

    for (id, time, condition, lifecycle, code) in [
        (
            "expired",
            end,
            condition_id,
            Lifecycle::Reject,
            "CONDITION_FALSE",
        ),
        (
            "missing",
            start,
            "undefined",
            Lifecycle::Suspend,
            "CONDITION_UNKNOWN",
        ),
    ] {
        let text = format!("なぜ[scope=鳥][time={time}][condition={condition}]ネラは鳥ですか");
        let blocked = engine
            .execute(actor, RequestEnvelope::new(id, id, actor, &text))
            .unwrap();
        assert_eq!(blocked.lifecycle, lifecycle, "{blocked:?}");
        assert!(blocked.reason.unwrap().starts_with(code));
        assert!(blocked.content_plan.is_none());
        assert!(blocked.render.is_none());
    }
    assert_eq!(store.load_memory(actor).unwrap(), memory);
    drop(engine);
    let mut reopened = Engine::open(&state.0).unwrap();
    assert_eq!(reopened.execute(actor, request).unwrap(), result);
    assert_eq!(reopened.latest(actor, "explanation").unwrap(), Some(case));
}
