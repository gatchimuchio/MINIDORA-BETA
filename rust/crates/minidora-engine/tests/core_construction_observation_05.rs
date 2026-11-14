use minidora_domain::{Guarantee, Intent, Lifecycle, Proposition, RequestEnvelope, ResultRecord};
use minidora_engine::{Engine, store::Store};
use serde_json::{Value, json};
use std::fs;
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_TEMP_DIRECTORY: AtomicU64 = AtomicU64::new(0);

fn run(engine: &mut Engine, actor: &str, input_id: &str, text: &str) -> ResultRecord {
    engine
        .execute(
            actor,
            RequestEnvelope::new("core-observation-five", input_id, actor, text),
        )
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
            "dependencies": plan.dependencies,
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
    })
}

fn sealed_trace(engine: &Engine, actor: &str) -> Value {
    let state = engine
        .latest(actor, "core-observation-five")
        .expect("read latest sealed Core observation")
        .expect("Core observation case revision exists");
    let bundle = state
        .bundle
        .as_ref()
        .expect("Core observation input is sealed");
    json!({
        "case_revision": state.revision,
        "parent_revision": state.parent_revision,
        "lifecycle": state.lifecycle,
        "input_id": bundle.request.input_id,
        "input_version": bundle.request.input_version,
        "original_text": bundle.request.text,
        "raw_sha256": bundle.raw_sha256,
        "parser_version": bundle.meaning.parser_version,
        "semantic_signature": bundle.semantic_signature,
        "context_result_ref": bundle.meaning.context_result_ref,
        "intent": bundle.meaning.intent,
    })
}

fn proposition_trace(proposition: &Proposition) -> Value {
    json!({
        "subject": proposition.subject,
        "predicate": proposition.predicate,
        "object": proposition.object,
        "negative": proposition.negative,
        "scope": proposition.scope,
    })
}

#[test]
fn direct_core_observation_five_composes_observed_rules_adapts_and_invalidates_counterexample_lineage()
 {
    let serial = NEXT_TEMP_DIRECTORY.fetch_add(1, Ordering::Relaxed);
    let directory = std::env::temp_dir().join(format!(
        "minidora-core-construction-observation-five-{}-{serial}",
        std::process::id()
    ));
    fs::create_dir(&directory).expect("unique isolated observation state");
    let mut engine = Engine::open(&directory).expect("open isolated Core state");
    let actor = "b14-adaptive-relation-observation";
    let case = "core-observation-five";
    let mut observations = Vec::new();

    let rule_one_text =
        "If [scope=materials-lab]X is a ceramic then [scope=materials-lab]X is fragile";
    let rule_one = run(
        &mut engine,
        actor,
        "root-rule-ceramic-fragile",
        rule_one_text,
    );
    assert_eq!(rule_one.lifecycle, Lifecycle::Commit);
    let rule_one_id = rule_one.dependencies[0].clone();
    observations.push(json!({
        "input": rule_one_text,
        "sealed_request": sealed_trace(&engine, actor),
        "result": result_trace(&rule_one),
    }));

    let rule_two_text =
        "If [scope=materials-lab]X is a fragile and struck then [scope=materials-lab]X is cracked";
    let rule_two = run(
        &mut engine,
        actor,
        "root-rule-fragile-struck-cracked",
        rule_two_text,
    );
    assert_eq!(rule_two.lifecycle, Lifecycle::Commit);
    assert!(matches!(
        engine
            .latest(actor, case)
            .expect("read sealed rule-two revision")
            .expect("rule-two revision")
            .bundle
            .expect("rule-two sealed input")
            .meaning
            .intent,
        Intent::AddConjunctiveRule { .. }
    ));
    let rule_two_id = rule_two.dependencies[0].clone();
    observations.push(json!({
        "input": rule_two_text,
        "sealed_request": sealed_trace(&engine, actor),
        "result": result_trace(&rule_two),
    }));

    let store = Store::open(&directory).expect("open isolated store for rule snapshot");
    let memory_after_rule_composition = store
        .load_memory(actor)
        .expect("load rules composed from supplied root rules");
    let learned_rule = memory_after_rule_composition
        .rules
        .iter()
        .find(|rule| {
            rule.valid
                && rule.dependencies.contains(&rule_one_id)
                && rule.dependencies.contains(&rule_two_id)
                && rule.conclusion.subject == "$v0"
                && rule.conclusion.predicate == "is"
                && rule.conclusion.object == "cracked"
                && rule.conclusion.scope == "materials-lab"
        })
        .expect("Core composed an adapted shortcut from both declared root rules");
    assert_eq!(learned_rule.premise.subject, "$v0");
    assert_eq!(learned_rule.premise.object, "ceramic");
    assert_eq!(learned_rule.additional_premises.len(), 1);
    assert_eq!(learned_rule.additional_premises[0].object, "struck");
    let learned_rule_id = learned_rule.id.clone();
    observations.push(json!({
        "state": "after root rule composition",
        "memory_revision": memory_after_rule_composition.revision,
        "derived_shortcut": {
            "id": learned_rule.id,
            "premise": proposition_trace(&learned_rule.premise),
            "additional_premises": learned_rule.additional_premises.iter().map(proposition_trace).collect::<Vec<_>>(),
            "conclusion": proposition_trace(&learned_rule.conclusion),
            "parent_rule_ids": learned_rule.dependencies,
            "valid": learned_rule.valid,
        }
    }));

    let ceramic_text = "[scope=materials-lab]Riva is a ceramic";
    let ceramic = run(&mut engine, actor, "new-subject-ceramic", ceramic_text);
    assert_eq!(ceramic.lifecycle, Lifecycle::Commit);
    let ceramic_id = ceramic.dependencies[0].clone();
    let ceramic_seal = sealed_trace(&engine, actor);
    let struck_text = "[scope=materials-lab]Riva is struck";
    let struck = run(&mut engine, actor, "new-subject-struck", struck_text);
    assert_eq!(struck.lifecycle, Lifecycle::Commit);
    let struck_id = struck.dependencies[0].clone();
    let struck_seal = sealed_trace(&engine, actor);
    observations.extend([
        json!({"input": ceramic_text, "sealed_request": ceramic_seal, "result": result_trace(&ceramic)}),
        json!({"input": struck_text, "sealed_request": struck_seal, "result": result_trace(&struck)}),
    ]);

    let novel_query_text = "[scope=materials-lab]Riva is cracked?";
    let novel_query = run(&mut engine, actor, "unseen-subject-query", novel_query_text);
    assert_eq!(novel_query.lifecycle, Lifecycle::Commit);
    let novel_query_seal = sealed_trace(&engine, actor);
    assert_eq!(novel_query.guarantee, Guarantee::Derived);
    for dependency in [
        &ceramic_id,
        &struck_id,
        &rule_one_id,
        &rule_two_id,
        &learned_rule_id,
    ] {
        assert!(
            novel_query.dependencies.contains(dependency),
            "unseen-subject proof omitted dependency {dependency}"
        );
    }
    let memory_before_counterexample = store
        .load_memory(actor)
        .expect("read memory before counterexample");
    assert!(
        !memory_before_counterexample.facts.iter().any(|fact| {
            fact.valid
                && fact.proposition.subject == "Riva"
                && fact.proposition.object == "cracked"
                && fact.proposition.scope == "materials-lab"
                && fact.guarantee == Guarantee::Derived
        }),
        "the query result is stored in case history; transient closure facts are not silently promoted to actor memory"
    );
    observations.extend([
        json!({"input": novel_query_text, "sealed_request": novel_query_seal, "result": result_trace(&novel_query)}),
        json!({
            "state": "after novel-subject derivation",
            "memory_revision": memory_before_counterexample.revision,
            "adopted_result": result_trace(&novel_query),
            "persistence_boundary": "the derived relation and exact proof roots live in the committed ResultRecord; transient query closure is not stored as a memory Fact"
        }),
    ]);

    let correction_text = "[scope=materials-lab]Correction: Riva is a polymer, not a ceramic.";
    let correction = run(
        &mut engine,
        actor,
        "counterexample-correction",
        correction_text,
    );
    assert_eq!(
        correction.lifecycle,
        Lifecycle::Commit,
        "counterexample correction: {:?}",
        correction.reason
    );
    let memory_after_counterexample = store
        .load_memory(actor)
        .expect("read memory after counterexample correction");
    let original_ceramic = memory_after_counterexample
        .facts
        .iter()
        .find(|fact| fact.id == ceramic_id)
        .expect("original observation retained");
    assert!(
        !original_ceramic.valid,
        "the contradicted original stays retained but invalid"
    );
    assert!(
        correction.dependencies.contains(&ceramic_id),
        "the correction records the original proof root it invalidates"
    );
    assert!(
        novel_query.dependencies.contains(&ceramic_id),
        "the historical derived result records the now-invalidated source"
    );
    let saved_counterexample = memory_after_counterexample
        .facts
        .iter()
        .find(|fact| {
            fact.valid
                && fact.proposition.subject == "Riva"
                && fact.proposition.object == "polymer"
                && fact.proposition.scope == "materials-lab"
                && fact.dependencies.is_empty()
        })
        .expect("replacement user premise is persisted");
    let saved_counterexample_id = saved_counterexample.id.clone();
    let updated_rule = memory_after_counterexample
        .rules
        .iter()
        .find(|rule| rule.id == learned_rule_id)
        .expect("composed rule remains retained");
    assert!(
        updated_rule.valid,
        "unrelated rule structure remains active"
    );
    observations.extend([
        json!({"input": correction_text, "sealed_request": sealed_trace(&engine, actor), "result": result_trace(&correction)}),
        json!({
            "state": "after counterexample correction",
            "memory_revision": memory_after_counterexample.revision,
            "original_observation": {
                "id": original_ceramic.id,
                "proposition": proposition_trace(&original_ceramic.proposition),
                "valid": original_ceramic.valid,
            },
            "invalidated_result_dependency": ceramic_id,
            "historical_result_referencing_invalidated_dependency": result_trace(&novel_query),
            "correction_dependency_ids": correction.dependencies,
            "retained_composed_rule": {
                "id": updated_rule.id,
                "valid": updated_rule.valid,
            },
            "new_counterexample_fact": {
                "id": saved_counterexample.id,
                "proposition": proposition_trace(&saved_counterexample.proposition),
                "valid": saved_counterexample.valid,
                "guarantee": saved_counterexample.guarantee,
            },
        }),
    ]);

    let after_counterexample_text = "[scope=materials-lab]Riva is cracked?";
    let after_counterexample = run(
        &mut engine,
        actor,
        "query-after-counterexample",
        after_counterexample_text,
    );
    assert_eq!(after_counterexample.lifecycle, Lifecycle::Suspend);
    assert!(
        after_counterexample
            .reason
            .as_deref()
            .unwrap_or_default()
            .contains("MISSING_PREMISES"),
        "the prior COMMIT must not be replayed after its source was invalidated: {:?}",
        after_counterexample.reason
    );
    assert!(after_counterexample.content_plan.is_none());
    observations.push(json!({
        "input": after_counterexample_text,
        "sealed_request": sealed_trace(&engine, actor),
        "expected_terminal": "SUSPEND/MISSING_PREMISES",
        "result": result_trace(&after_counterexample),
    }));

    let final_state = engine
        .latest(actor, case)
        .expect("read final case state")
        .expect("case remains recorded");
    assert!(final_state.prior_results.contains_key(&novel_query.id));
    assert_eq!(
        final_state.prior_results.get(&novel_query.id),
        Some(&novel_query),
        "historical adopted result is retained while current proof is invalidated"
    );
    let evidence = json!({
        "observation": "MB-BUILD-20261114 core construction observation five",
        "entry": "in-process minidora_engine::Engine::execute",
        "actor": actor,
        "case_id": case,
        "adaptation_mechanism": "Core relation engine composes two explicitly supplied root rules into a bounded proof-preserving shortcut; it does not induce a rule from unstructured examples",
        "novel_subject": "Riva was not used in the root rules; its new USER_PREMISE facts cause the first committed cracked result",
        "counterexample_and_feedback": "a scope-specific user correction replaces the active Riva-is-ceramic premise with Riva-is-polymer; the original fact remains retained with valid=false; the prior COMMIT retains that fact ID as a proof dependency, the correction result records the invalidated dependency, and the next query suspends rather than replaying the stale conclusion",
        "root_rule_ids": [rule_one_id, rule_two_id],
        "learned_shortcut_id": learned_rule_id,
        "novel_adopted_result_id": novel_query.id,
        "invalidated_result_dependency_id": ceramic_id,
        "counterexample_fact_id": saved_counterexample_id,
        "final_case_revision": final_state.revision,
        "retained_novel_result": final_state.prior_results.get(&novel_query.id).map(result_trace),
        "observations": observations,
        "external_semantic_generation": false,
        "external_network_or_side_effects": false,
        "limitations": [
            "bounded to composition of declared one/two-premise rules and one corrected source fact",
            "USER_PREMISE and DERIVED do not verify claims about the world",
            "not general rule induction from raw examples or full learning acceptance",
            "full workspace runtime suite and all requirement acceptance are not established"
        ]
    });
    println!(
        "B14_CORE_OBSERVATION_05={}",
        serde_json::to_string(&evidence).expect("serialize direct Core observation")
    );

    drop(store);
    drop(engine);
    fs::remove_dir_all(directory).expect("remove isolated Core observation state");
}
