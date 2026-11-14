use minidora_domain::{ActionRecord, EffectPhase, Lifecycle, RequestEnvelope, ResultRecord};
use minidora_engine::{Engine, store::Store};
use serde_json::{Value, json};
use std::fs;
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_TEMP_DIRECTORY: AtomicU64 = AtomicU64::new(0);

fn run(engine: &mut Engine, actor: &str, request: RequestEnvelope) -> ResultRecord {
    let input_id = request.input_id.clone();
    engine
        .execute(actor, request)
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
        "receipts": result.receipts,
    })
}

fn case_trace(engine: &Engine, actor: &str, case: &str) -> Value {
    let state = engine
        .latest(actor, case)
        .expect("read construction case")
        .expect("construction case exists");
    json!({
        "revision": state.revision,
        "parent_revision": state.parent_revision,
        "lifecycle": state.lifecycle,
        "goal_sha256": state.goal_sha256,
        "input_id": state.bundle.as_ref().map(|bundle| bundle.request.input_id.clone()),
        "input_version": state.bundle.as_ref().map(|bundle| bundle.request.input_version),
        "original_text": state.bundle.as_ref().map(|bundle| bundle.request.text.clone()),
        "raw_sha256": state.bundle.as_ref().map(|bundle| bundle.raw_sha256.clone()),
        "parser_version": state.bundle.as_ref().map(|bundle| bundle.meaning.parser_version.clone()),
        "intent": state.bundle.as_ref().map(|bundle| bundle.meaning.intent.clone()),
        "result": state.result.as_ref().map(result_trace),
        "retained_result_ids": state.prior_results.keys().collect::<Vec<_>>(),
    })
}

#[test]
fn direct_core_observation_six_never_completes_permission_budget_cancel_or_unknown_effect_stops() {
    let serial = NEXT_TEMP_DIRECTORY.fetch_add(1, Ordering::Relaxed);
    let directory = std::env::temp_dir().join(format!(
        "minidora-core-construction-observation-six-{}-{serial}",
        std::process::id()
    ));
    fs::create_dir(&directory).expect("unique isolated observation state");
    let mut engine = Engine::open(&directory).expect("open isolated Core state");
    let actor = "b15-safe-stop-observation";
    let mut observations = Vec::new();

    let deadline_case = "deadline-case";
    let mut deadline_request =
        RequestEnvelope::new(deadline_case, "deadline-zero", actor, "4+6を計算して");
    deadline_request.budget.deadline_ms = 0;
    let deadline_stop = run(&mut engine, actor, deadline_request);
    assert_eq!(deadline_stop.lifecycle, Lifecycle::Suspend);
    assert!(
        deadline_stop
            .reason
            .as_deref()
            .unwrap_or_default()
            .contains("RESOURCE_LIMIT")
    );
    assert!(deadline_stop.content_plan.is_none());
    assert!(deadline_stop.render.is_none());
    let deadline_state = case_trace(&engine, actor, deadline_case);
    observations.push(json!({
        "request": {"text": "4+6を計算して", "deadline_ms": 0},
        "case_after_stop": deadline_state,
        "result": result_trace(&deadline_stop),
    }));

    let action_case = "action-budget-case";
    let mut action_request =
        RequestEnvelope::new(action_case, "too-little-actions", actor, "4+6を計算して");
    action_request.budget.max_actions = 2;
    let action_stop = run(&mut engine, actor, action_request);
    assert_eq!(action_stop.lifecycle, Lifecycle::Suspend);
    assert!(
        action_stop
            .reason
            .as_deref()
            .unwrap_or_default()
            .contains("ACTION_BUDGET_EXHAUSTED")
    );
    assert!(action_stop.content_plan.is_none());
    assert!(action_stop.render.is_none());
    observations.push(json!({
        "request": {"text": "4+6を計算して", "max_actions": 2},
        "case_after_stop": case_trace(&engine, actor, action_case),
        "result": result_trace(&action_stop),
    }));

    let permission_case = "permission-case";
    let forbidden_path = directory.join("not-authorized.txt");
    let mut permission_request = RequestEnvelope::new(
        permission_case,
        "missing-write-grant",
        actor,
        "4+6を計算して",
    );
    permission_request.output_path = Some(forbidden_path.clone());
    let permission_stop = run(&mut engine, actor, permission_request);
    assert_eq!(permission_stop.lifecycle, Lifecycle::Suspend);
    assert!(
        permission_stop
            .reason
            .as_deref()
            .unwrap_or_default()
            .contains("WRITE_PERMISSION_MISSING")
    );
    assert_eq!(
        permission_stop
            .calculation
            .as_ref()
            .map(|value| value.exact_value.as_str()),
        Some("10")
    );
    assert!(permission_stop.content_plan.is_some());
    assert_eq!(
        permission_stop
            .render
            .as_ref()
            .map(|render| render.status.as_str()),
        Some("RENDERED")
    );
    assert!(permission_stop.receipts.is_empty());
    assert!(permission_stop.execution_graph_ref.is_none());
    assert!(!forbidden_path.exists());
    let store = Store::open(&directory).expect("open isolated Core action ledger");
    assert!(
        store
            .actions(actor, permission_case)
            .expect("read permission-case actions")
            .is_empty()
    );
    observations.push(json!({
        "request": {
            "text": "4+6を計算して",
            "output_path": forbidden_path,
            "write_grant": false,
        },
        "file_exists_after_stop": forbidden_path.exists(),
        "action_ledger": store.actions(actor, permission_case).expect("read empty action ledger"),
        "case_after_stop": case_trace(&engine, actor, permission_case),
        "result": result_trace(&permission_stop),
        "goal_unmet": "CONFIRMED_FILE required predicate has no receipt; case remains SUSPEND",
    }));

    let effect_case = "unknown-effect-case";
    let effect_seed = run(
        &mut engine,
        actor,
        RequestEnvelope::new(effect_case, "effect-origin", actor, "2+2を計算して"),
    );
    assert_eq!(effect_seed.lifecycle, Lifecycle::Commit);
    let effect_seed_state = engine
        .latest(actor, effect_case)
        .expect("read effect seed state")
        .expect("effect seed case exists");
    let fingerprint = effect_seed_state
        .input_fingerprint
        .clone()
        .expect("seed input fingerprint");
    let auth_ref = store
        .put_bytes(
            actor,
            effect_case,
            b"construction fixture authorization record",
        )
        .expect("store fixture authorization reference");
    let request_ref = store
        .put_bytes(
            actor,
            effect_case,
            b"construction fixture exact request bytes",
        )
        .expect("store fixture request bytes");
    let uncertain_target = directory.join("unknown-effect-never-written.txt");
    let mut uncertain_action = ActionRecord {
        case_id: effect_case.into(),
        actor_id: actor.into(),
        action_id: "fixture-unknown-local-write".into(),
        input_fingerprint: fingerprint,
        idempotency_key: "fixture-unknown-local-write-key".into(),
        effect_class: "LOCAL_WRITE".into(),
        target: uncertain_target.to_string_lossy().into_owned(),
        authorization_sha256: auth_ref,
        exact_request_sha256: request_ref,
        phase: EffectPhase::Proposed,
        receipt: None,
    };
    for phase in [
        EffectPhase::Proposed,
        EffectPhase::Authorized,
        EffectPhase::Prepared,
        EffectPhase::Attempted,
    ] {
        uncertain_action.phase = phase;
        store
            .save_action(actor, &uncertain_action)
            .unwrap_or_else(|error| panic!("seed phase {phase:?}: {error}"));
    }
    let observed_action = store
        .actions(actor, effect_case)
        .expect("read uncertain effect action")
        .into_iter()
        .next()
        .expect("effect fixture action exists");
    assert_eq!(observed_action.phase, EffectPhase::Unknown);
    assert!(observed_action.receipt.is_none());
    assert!(!uncertain_target.exists());

    let blocked_work = run(
        &mut engine,
        actor,
        RequestEnvelope::new(effect_case, "work-while-unknown", actor, "3+4を計算して"),
    );
    assert_eq!(blocked_work.lifecycle, Lifecycle::Suspend);
    assert!(
        blocked_work
            .reason
            .as_deref()
            .unwrap_or_default()
            .contains("EXTERNAL_EFFECT_UNKNOWN")
    );
    assert!(blocked_work.content_plan.is_none());
    let state_after_block = engine
        .latest(actor, effect_case)
        .expect("latest after unknown-effect block")
        .expect("effect seed case retained");
    assert_eq!(
        state_after_block.result.as_ref().map(|result| &result.id),
        Some(&effect_seed.id)
    );
    observations.extend([
        json!({
            "state": "attempted action without confirmed receipt",
            "fixture_source": "Store action journal transition PROPOSED -> AUTHORIZED -> PREPARED -> ATTEMPTED; Store::actions exposes ATTEMPTED as UNKNOWN",
            "external_effect_performed": false,
            "target_exists": uncertain_target.exists(),
            "action": observed_action,
            "case_after_seed": case_trace(&engine, actor, effect_case),
        }),
        json!({
            "request": "3+4を計算して",
            "prior_unknown_action_id": observed_action.action_id,
            "case_after_stop": case_trace(&engine, actor, effect_case),
            "result": result_trace(&blocked_work),
            "previous_committed_case_result_unchanged": state_after_block.result.as_ref().is_some_and(|result| result.id == effect_seed.id),
        }),
    ]);

    let cancellation = run(
        &mut engine,
        actor,
        RequestEnvelope::new(
            effect_case,
            "cancel-with-unknown-effect",
            actor,
            "キャンセル",
        ),
    );
    assert_eq!(cancellation.lifecycle, Lifecycle::Cancel);
    assert!(
        cancellation
            .reason
            .as_deref()
            .unwrap_or_default()
            .contains("AUTHORIZED_CANCEL")
    );
    assert!(cancellation.content_plan.is_none());
    assert_eq!(cancellation.receipts.len(), 1);
    assert_eq!(cancellation.receipts[0].phase, EffectPhase::Unknown);
    let final_actions = store
        .actions(actor, effect_case)
        .expect("unknown external effect remains recorded after cancellation");
    assert_eq!(final_actions.len(), 1);
    assert_eq!(final_actions[0].phase, EffectPhase::Unknown);
    assert!(!uncertain_target.exists());
    observations.push(json!({
        "request": "キャンセル",
        "case_after_cancel": case_trace(&engine, actor, effect_case),
        "action_ledger_after_cancel": final_actions,
        "target_exists_after_cancel": uncertain_target.exists(),
        "result": result_trace(&cancellation),
    }));

    let empty_cancel_case = "empty-cancel-case";
    let empty_cancel = run(
        &mut engine,
        actor,
        RequestEnvelope::new(empty_cancel_case, "cancel-before-work", actor, "キャンセル"),
    );
    assert_eq!(empty_cancel.lifecycle, Lifecycle::Cancel);
    assert!(empty_cancel.content_plan.is_none());
    observations.push(json!({
        "request": "キャンセル",
        "case_after_cancel": case_trace(&engine, actor, empty_cancel_case),
        "result": result_trace(&empty_cancel),
    }));

    let evidence = json!({
        "observation": "MB-BUILD-20261114 core construction observation six",
        "entry": "in-process minidora_engine::Engine::execute",
        "actor": actor,
        "cases": [deadline_case, action_case, permission_case, effect_case, empty_cancel_case],
        "external_semantic_generation": false,
        "external_network": false,
        "external_file_write_attempts": false,
        "unknown_effect_fixture": "The action ledger was advanced through ATTEMPTED without invoking the writer; Store recovery exposed UNKNOWN with no receipt, modeling an ambiguous effect record without claiming that a physical write occurred.",
        "observations": observations,
        "limitations": [
            "isolated in-process Core and temporary SQLite only",
            "no physical power-loss/crash test, external provider or browser acceptance",
            "no full workspace runtime or requirement acceptance claim"
        ],
    });
    println!(
        "B15_CORE_OBSERVATION_06={}",
        serde_json::to_string(&evidence).expect("serialize direct Core stop observation")
    );

    drop(store);
    drop(engine);
    fs::remove_dir_all(directory).expect("remove isolated Core state");
}
