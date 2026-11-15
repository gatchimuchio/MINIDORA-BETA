use minidora_domain::{Lifecycle, RequestEnvelope, ResultRecord};
use minidora_engine::{Engine, store::Store};
use serde_json::json;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_TEMP_DIRECTORY: AtomicU64 = AtomicU64::new(0);

struct TestDirectory(PathBuf);

impl TestDirectory {
    fn new() -> Self {
        let serial = NEXT_TEMP_DIRECTORY.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "minidora-core-actor-context-boundary-48-{}-{serial}",
            std::process::id()
        ));
        fs::create_dir(&path).expect("create isolated actor-context state");
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

fn commit(
    engine: &mut Engine,
    actor: &str,
    case: &str,
    input_id: &str,
    text: &str,
) -> ResultRecord {
    let result = engine
        .execute(actor, RequestEnvelope::new(case, input_id, actor, text))
        .unwrap_or_else(|error| panic!("{actor}/{input_id} failed: {error}"));
    assert_eq!(result.lifecycle, Lifecycle::Commit, "{actor}: {result:?}");
    result
}

fn assert_rendered_follow_up(result: &ResultRecord, selected_result_id: &str) {
    assert_eq!(result.lifecycle, Lifecycle::Commit, "{result:?}");
    assert!(
        result
            .dependencies
            .iter()
            .any(|id| id == selected_result_id),
        "the follow-up did not consume its explicitly selected result: {result:?}"
    );
    assert!(
        result.content_plan.is_some(),
        "no adopted ContentPlan: {result:?}"
    );
    let render = result
        .render
        .as_ref()
        .expect("follow-up has a render observation");
    assert_eq!(render.status, "RENDERED");
    assert_eq!(render.bytes, result.text.as_bytes());
}

fn explain_selected_result(
    engine: &mut Engine,
    actor: &str,
    case: &str,
    input_id: &str,
    selected_result_id: &str,
) -> ResultRecord {
    let mut request = RequestEnvelope::new(case, input_id, actor, "なぜですか");
    request.context_ref = Some(selected_result_id.to_owned());
    engine
        .execute(actor, request)
        .unwrap_or_else(|error| panic!("{actor}/{input_id} explanation failed: {error}"))
}

#[test]
fn actor_cannot_use_another_actors_result_as_explicit_context() {
    let directory = TestDirectory::new();
    let actor_a = "b48-context-owner-a";
    let actor_b = "b48-context-owner-b";
    let case_a = "b48-context-case-a";
    let case_b = "b48-context-case-b";
    let scope = "b48-context-scope";
    let mut engine = Engine::open(directory.path()).expect("open isolated Core state");

    let fact_a = commit(
        &mut engine,
        actor_a,
        case_a,
        "actor_a_assertion",
        &format!("[scope={scope}]ココは鳥です"),
    );
    let fact_b = commit(
        &mut engine,
        actor_b,
        case_b,
        "actor_b_assertion",
        &format!("[scope={scope}]ミサゴは鳥です"),
    );
    let answer_a = commit(
        &mut engine,
        actor_a,
        case_a,
        "actor_a_relation_question",
        &format!("[scope={scope}]ココは鳥ですか"),
    );
    let answer_b = commit(
        &mut engine,
        actor_b,
        case_b,
        "actor_b_relation_question",
        &format!("[scope={scope}]ミサゴは鳥ですか"),
    );
    assert_ne!(
        answer_a.id, answer_b.id,
        "actor-owned result identities differ"
    );
    assert_ne!(fact_a.dependencies[0], fact_b.dependencies[0]);
    assert!(answer_a.dependencies.contains(&fact_a.dependencies[0]));
    assert!(answer_b.dependencies.contains(&fact_b.dependencies[0]));

    let store = Store::open(directory.path()).expect("open actor-scoped memory store");
    let memory_a_before = store.load_memory(actor_a).expect("read actor A memory");
    let memory_b_before = store.load_memory(actor_b).expect("read actor B memory");
    drop(store);
    drop(engine);

    let mut reopened =
        Engine::open(directory.path()).expect("reopen Core before context selection");
    let mut foreign_request = RequestEnvelope::new(
        case_a,
        "actor_a_supplies_actor_b_result_id",
        actor_a,
        "なぜですか",
    );
    foreign_request.context_ref = Some(answer_b.id.clone());
    let foreign_context = reopened
        .execute(actor_a, foreign_request)
        .expect("foreign context is recorded as a typed terminal");
    assert_eq!(
        foreign_context.lifecycle,
        Lifecycle::Fail,
        "{foreign_context:?}"
    );
    assert!(foreign_context.reason.as_deref().is_some_and(|reason| {
        reason.starts_with("INPUT_FAILURE:") && reason.contains("CONTEXT_MISMATCH")
    }));
    assert!(foreign_context.content_plan.is_none());
    assert!(foreign_context.render.is_none());
    let serialized_evidence = serde_json::to_string(&foreign_context.evidence)
        .expect("serialize rejected result evidence for the no-leak assertion");
    assert!(
        !foreign_context.text.contains("ミサゴ") && !serialized_evidence.contains("ミサゴ"),
        "actor A received actor B's result content: {foreign_context:?}"
    );

    let store = Store::open(directory.path()).expect("read actor memory after rejected context");
    let memory_a_after_rejection = store
        .load_memory(actor_a)
        .expect("read actor A memory after rejection");
    let memory_b_after_rejection = store
        .load_memory(actor_b)
        .expect("read actor B memory after rejection");
    assert_eq!(memory_a_after_rejection, memory_a_before);
    assert_eq!(memory_b_after_rejection, memory_b_before);
    let case_a_after_rejection = store
        .load_case(actor_a, case_a)
        .expect("load actor A case")
        .expect("actor A case remains stored");
    assert_eq!(case_a_after_rejection.lifecycle, Lifecycle::Fail);
    assert!(
        case_a_after_rejection
            .prior_results
            .contains_key(&answer_a.id)
    );

    let own_context_a = explain_selected_result(
        &mut reopened,
        actor_a,
        case_a,
        "actor_a_selects_own_prior_result",
        &answer_a.id,
    );
    assert_rendered_follow_up(&own_context_a, &answer_a.id);
    let own_context_b = explain_selected_result(
        &mut reopened,
        actor_b,
        case_b,
        "actor_b_selects_own_prior_result",
        &answer_b.id,
    );
    assert_rendered_follow_up(&own_context_b, &answer_b.id);

    let memory_a_after_follow_up = store
        .load_memory(actor_a)
        .expect("read actor A memory after own replay");
    let memory_b_after_follow_up = store
        .load_memory(actor_b)
        .expect("read actor B memory after own replay");
    assert_eq!(memory_a_after_follow_up, memory_a_before);
    assert_eq!(memory_b_after_follow_up, memory_b_before);

    println!(
        "MINIDORA_B48_TRACE={}",
        json!({
            "schema": "MINIDORA_CORE_CROSS_ACTOR_CONTEXT_REJECTION_TRACE_V1",
            "scope": scope,
            "engine_reopened_before_foreign_context_attempt": true,
            "actor_a": {
                "actor_id": actor_a,
                "case_id": case_a,
                "owned_result_id": answer_a.id,
                "foreign_result_id_attempted": answer_b.id,
                "foreign_context_result_id": foreign_context.id,
                "foreign_context_lifecycle": foreign_context.lifecycle,
                "foreign_context_reason": foreign_context.reason,
                "content_plan_created": foreign_context.content_plan.is_some(),
                "render_created": foreign_context.render.is_some(),
                "foreign_result_text_leaked": foreign_context.text.contains("ミサゴ"),
                "memory_unchanged_after_rejection": memory_a_after_rejection == memory_a_before,
                "own_context_lifecycle": own_context_a.lifecycle,
                "own_context_render_status": own_context_a.render.as_ref().map(|render| render.status.as_str())
            },
            "actor_b": {
                "actor_id": actor_b,
                "case_id": case_b,
                "owned_result_id": answer_b.id,
                "memory_unchanged_after_actor_a_attempt_and_own_replay": memory_b_after_follow_up == memory_b_before,
                "own_context_lifecycle": own_context_b.lifecycle,
                "own_context_render_status": own_context_b.render.as_ref().map(|render| render.status.as_str())
            },
            "cross_actor_access_rejected": true,
            "outside_world_truth": "not checked",
            "normal_cli_http_and_full_cap_d11_acceptance": "not checked"
        })
    );
}
