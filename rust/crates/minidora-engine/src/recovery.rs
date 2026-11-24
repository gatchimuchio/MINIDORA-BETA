//! Core-owned reconciliation of a previously attempted local file effect.
//!
//! This module never writes target content, invokes a provider, creates an
//! action/token or reinterprets an input. It verifies the stored original goal
//! and artifacts, observes the existing exact target, then submits the resulting
//! receipt/revisions through the same Core-owned persistence boundary. Host
//! authentication and trusted ownership of ancestor directories remain required.
use crate::{
    core::{
        APPENDED_SEQUENCE_EXPLANATION_PREDICATE, CHAINED_SEQUENCE_EXPLANATION_PREDICATE,
        GENERATED_RELATION_EXPLANATION_PREDICATE, INSERTED_SEQUENCE_EXPLANATION_PREDICATE,
        MOVED_SEQUENCE_EXPLANATION_PREDICATE, RELATION_COMPARISON_PREDICATE,
        RELATION_EXPLANATION_PREDICATE, REMOVED_SEQUENCE_EXPLANATION_PREDICATE,
        REPEATED_SEQUENCE_CORRECTION_PREDICATE, REVISED_SEQUENCE_EXPLANATION_PREDICATE,
        REWIRED_SEQUENCE_CORRECTION_PREDICATE, REWIRED_SEQUENCE_EXPLANATION_PREDICATE,
        SEQUENCE_APPEND_PREDICATE, SEQUENCE_CORRECTION_PREDICATE, SEQUENCE_EXPLANATION_PREDICATE,
        SEQUENCE_HISTORY_PREDICATE, SEQUENCE_INSERT_PREDICATE, SEQUENCE_MOVE_PREDICATE,
        SEQUENCE_REMOVE_PREDICATE, SEQUENCE_STEP_EXPLANATION_PREDICATE, calculation_sequence_nodes,
        calculation_sequence_report, condition_evaluation_contract,
        corrected_calculation_from_context, exact_calculation_unit, goal_predicates,
        prepare_sequence_edit, previous_calculation_sequence, run_calculation_sequence,
        run_sequence_edit, store_execution_graph, validate_goal_render, validate_render,
        verify_goal_content,
    },
    language,
    scheduler::{GoalNode, GoalNodeKind, Scheduler},
    search,
    store::{Store, VerifiedCaseHistory},
};
use minidora_domain::*;
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::Read,
    path::Path,
    time::{Duration, Instant},
};

pub fn reconcile(store: &Store, actor: &str, case: &str) -> Result<ResultRecord> {
    let mut state = store
        .load_case(actor, case)?
        .ok_or_else(|| failure("CASE_NOT_FOUND", "no saved case to reconcile"))?;
    if state.lifecycle == Lifecycle::Commit {
        return state
            .result
            .ok_or_else(|| failure("INTEGRITY_FAILURE", "committed case has no result"));
    }
    if !matches!(state.lifecycle, Lifecycle::Adopted | Lifecycle::Suspend) {
        return Err(failure(
            "INVALID_RECOVERY_STATE",
            "only an adopted/suspended case can reconcile a local file",
        ));
    }
    let mut result = state
        .result
        .clone()
        .ok_or_else(|| failure("RECOVERY_DATA_UNAVAILABLE", "no adopted result is retained"))?;
    let inspected = inspect(store, actor, &state, &result);
    let (mut action, receipt, mut graph, render_ref, explains_sequence) = match inspected {
        Ok(value) => value,
        Err(error) => return suspend(store, &mut state, result, &error.to_string()),
    };
    // Persist a recovered UNKNOWN observation before its later confirmation.
    // The exact token, target, authority and request bytes are unchanged.
    if action.phase == EffectPhase::Unknown {
        store.save_action(actor, &action)?;
        action.phase = EffectPhase::Confirmed;
        action.receipt = Some(receipt.clone());
        store.save_action(actor, &action)?;
    }
    result
        .receipts
        .retain(|old| old.action_id != receipt.action_id);
    result.receipts.push(receipt.clone());
    let receipt_ref = store.put_artifact(actor, case, &receipt)?;
    graph.observe(
        "file",
        &receipt_ref,
        &BTreeMap::from([("render".into(), render_ref)]),
    )?;
    if !graph.is_complete() {
        return suspend(
            store,
            &mut state,
            result,
            "UNMET_GOAL_NODES: reconciliation retained unresolved required artifacts",
        );
    }
    result.execution_graph_ref = Some(
        if explains_sequence
            || state.bundle.as_ref().is_some_and(|bundle| {
                matches!(
                    bundle.meaning.intent,
                    Intent::CalculateSequence { .. }
                        | Intent::CorrectCalculationSequenceStep { .. }
                        | Intent::AppendCalculationSequenceStep { .. }
                        | Intent::RemoveCalculationSequenceStep { .. }
                        | Intent::MoveCalculationSequenceStep { .. }
                        | Intent::InsertCalculationSequenceStep { .. }
                        | Intent::ShowCalculationSequenceHistory
                        | Intent::ExplainCalculationSequenceStep { .. }
                        | Intent::EvaluateCondition { .. }
                        | Intent::EvaluateConditionDetailed { .. }
                        | Intent::InspectFactSource { .. }
                        | Intent::InspectConditionSource { .. }
                        | Intent::InspectConditionSourceByDefinition { .. }
                        | Intent::InspectRuleSource { .. }
                        | Intent::InspectDeclaredRuleSource { .. }
                        | Intent::InspectPreviousRuleSource
                        | Intent::InspectPreviousComposedRuleSource
                        | Intent::InspectPreviousRuleSourceById { .. }
                        | Intent::ListPreviousRules
                        | Intent::AskListedRuleSource { .. }
                        | Intent::InspectComposedRuleSource { .. }
                        | Intent::FetchPreviousSearchCandidate { .. }
                        | Intent::FetchNextUnobservedSource
                        | Intent::Search { .. }
                        | Intent::SearchWithFallback { .. }
                        | Intent::AskWeb { .. }
                ) || matches!(
                    bundle.meaning.parser_version.as_str(),
                    crate::input::SOURCE_SUMMARY_PARSER_VERSION
                        | crate::input::EXPLAINED_SEARCH_BODY_FOLLOWUP_PARSER_VERSION
                        | crate::input::GUARDED_RELATION_RULE_PARSER_VERSION
                        | crate::input::GUARDED_CONJUNCTIVE_RULE_PARSER_VERSION
                        | crate::input::MULTI_CONDITION_RULE_PARSER_VERSION
                        | crate::input::MULTI_CONDITION_RELATION_PARSER_VERSION
                ) || crate::input::uses_derived_source_context(&bundle.meaning.parser_version)
            })
        {
            store_execution_graph(store, actor, case, &graph)?
        } else {
            store.put_artifact(
                actor,
                case,
                &(
                    graph.nodes(),
                    graph.observed(),
                    graph.consumed_by("content"),
                    graph.consumed_by("render"),
                    graph.consumed_by("file"),
                ),
            )?
        },
    );
    result.reason = Some(
        "LOCAL_FILE_RECONCILED: exact original artifact observed; no effect resubmitted".into(),
    );
    if state.lifecycle == Lifecycle::Suspend {
        advance(store, &mut state, &result, Lifecycle::Running)?;
        advance(store, &mut state, &result, Lifecycle::Adopted)?;
    }
    advance(store, &mut state, &result, Lifecycle::Commit)?;
    state
        .result
        .ok_or_else(|| failure("INTEGRITY_FAILURE", "reconciliation lost result"))
}

/// Reconcile local file actions after the goal was cancelled.
/// Preparations belonging to cancelled inputs are closed without a write.
/// Only the immutable original action's bytes, token, target and authorization
/// are eligible. This records effect observations, never restarts/completes the
/// cancelled goal or sends anything. Unobservable/unsupported effects remain
/// UNKNOWN; they are never guessed to have failed.
pub fn reconcile_cancelled_effects(
    store: &Store,
    actor: &str,
    case: &str,
) -> Result<Vec<ActionRecord>> {
    let state = store
        .load_case(actor, case)?
        .ok_or_else(|| failure("CASE_NOT_FOUND", "no cancelled case to inspect"))?;
    if state.lifecycle != Lifecycle::Cancel {
        return Err(failure(
            "INVALID_RECOVERY_STATE",
            "cancelled-effect observation requires a CANCEL case",
        ));
    }
    close_cancelled_file_preparations(store, &state)?;
    for mut action in store.actions(actor, case)? {
        if action.phase != EffectPhase::Unknown {
            continue;
        }
        // Persist the recovery uncertainty if the final saved phase was
        // ATTEMPTED. This is not permission to repeat that physical operation.
        store.save_action(actor, &action)?;
        if action.effect_class != "LOCAL_WRITE"
            || action.actor_id != actor
            || action.case_id != case
        {
            continue;
        }
        let observation = (|| -> Result<DeliveryReceipt> {
            let exact = store.get_bytes(actor, case, &action.exact_request_sha256)?;
            if action.exact_request_sha256 != digest(&exact) {
                return Err(failure(
                    "RECOVERY_ARTIFACT_MISMATCH",
                    "original effect bytes differ",
                ));
            }
            let path = Path::new(&action.target);
            let authorization: Permissions = serde_json::from_slice(&store.get_bytes(
                actor,
                case,
                &action.authorization_sha256,
            )?)?;
            if !authorization.write_paths.contains(path)
                || action.idempotency_key != action.action_id
                || action.action_id
                    != identity(&(
                        case,
                        &action.input_fingerprint,
                        "file",
                        path,
                        &action.exact_request_sha256,
                    ))?
            {
                return Err(failure(
                    "RECOVERY_AUTHORITY_MISMATCH",
                    "original scope or action identity differs",
                ));
            }
            observe_file(path, &exact)?;
            Ok(DeliveryReceipt {
                action_id: action.action_id.clone(),
                target: action.target.clone(),
                content_sha256: action.exact_request_sha256.clone(),
                phase: EffectPhase::Confirmed,
                observed_bytes: Some(exact.len() as u64),
                reason: Some("CANCELLED_GOAL_LOCAL_EFFECT_OBSERVED_NO_RESEND".into()),
            })
        })();
        if let Ok(receipt) = observation {
            action.phase = EffectPhase::Confirmed;
            action.receipt = Some(receipt);
            store.save_action(actor, &action)?;
        }
    }
    // Case lifecycle/revision is deliberately unchanged. The canceled input's
    // historical result and original goal can never become COMMIT here.
    store.actions(actor, case)
}

pub(crate) fn is_unattempted_local_write(action: &ActionRecord) -> bool {
    action.effect_class == "LOCAL_WRITE"
        && matches!(
            action.phase,
            EffectPhase::Proposed | EffectPhase::Authorized | EffectPhase::Prepared
        )
}

/// Skip a physical write and close only actions whose owning input was durably
/// cancelled. An issued attempt keeps its existing observation path.
pub(crate) fn close_cancelled_file_preparations(store: &Store, state: &CaseRevision) -> Result<()> {
    if state.lifecycle != Lifecycle::Cancel {
        return Err(failure(
            "INVALID_RECOVERY_STATE",
            "file preparation closure requires a cancelled goal",
        ));
    }
    for mut action in store.actions(&state.actor_id, &state.case_id)? {
        let owner_cancelled = state.input_fingerprint.as_ref() == Some(&action.input_fingerprint)
            || state
                .prior_results
                .get(&action.input_fingerprint)
                .is_some_and(|result| result.lifecycle == Lifecycle::Cancel);
        if is_unattempted_local_write(&action) && owner_cancelled {
            action.phase = EffectPhase::Cancelled;
            action.receipt = None;
            store.save_action(&state.actor_id, &action)?;
        }
    }
    Ok(())
}

type Inspected = (ActionRecord, DeliveryReceipt, Scheduler, String, bool);

struct PreviousTextEditExpectation {
    source_content_ref: String,
    source_render_ref: String,
    edit_ref: String,
}

type JsonGroupMeanRecovery = (String, String, String, String, Vec<u8>, Vec<String>);

fn verify_saved_previous_relation_query_context(
    store: &Store,
    actor: &str,
    case: &str,
    state: &CaseRevision,
    previous: &ResultRecord,
    explanation: (&ResultRecord, &ContentPlan),
    goal: &GoalContract,
) -> Result<()> {
    let (result, plan) = explanation;
    let Some((_, prior_bundle)) =
        store.get_input_bundle(actor, case, &previous.input_id, previous.input_version)?
    else {
        return Ok(());
    };
    let Intent::AskPreviousRelation {
        proposition,
        explain,
        explicit_scope,
    } = &prior_bundle.meaning.intent
    else {
        return Ok(());
    };
    if previous.lifecycle != Lifecycle::Commit
        || previous.calculation.is_some()
        || prior_bundle.request.actor_id != actor
        || prior_bundle.request.case_id != case
        || identity(&prior_bundle.request)? != previous.id
    {
        return Err(failure(
            "RECOVERY_CONTEXT_PROOF_CHANGED",
            "the saved AskPreviousRelation input and committed result identities differ",
        ));
    }

    let anchor_id = prior_bundle
        .meaning
        .context_result_ref
        .as_deref()
        .ok_or_else(|| {
            failure(
                "RECOVERY_CONTEXT_PROOF_CHANGED",
                "the saved AskPreviousRelation has no retained result anchor",
            )
        })?;
    let anchor = state.prior_results.get(anchor_id).ok_or_else(|| {
        failure(
            "RECOVERY_CONTEXT_PROOF_CHANGED",
            "the saved AskPreviousRelation anchor is no longer retained",
        )
    })?;
    if anchor.id != anchor_id || anchor.lifecycle != Lifecycle::Commit {
        return Err(failure(
            "RECOVERY_CONTEXT_PROOF_CHANGED",
            "the saved AskPreviousRelation anchor is not the exact committed result",
        ));
    }
    let Some((_, anchor_bundle)) =
        store.get_input_bundle(actor, case, &anchor.input_id, anchor.input_version)?
    else {
        return Err(failure(
            "RECOVERY_CONTEXT_PROOF_CHANGED",
            "the saved AskPreviousRelation anchor is not bound to its sealed input identity",
        ));
    };
    if anchor_bundle.request.actor_id != actor
        || anchor_bundle.request.case_id != case
        || identity(&anchor_bundle.request)? != anchor.id
    {
        return Err(failure(
            "RECOVERY_CONTEXT_PROOF_CHANGED",
            "the saved AskPreviousRelation anchor is not bound to its sealed input identity",
        ));
    }

    let prior_plan = previous.content_plan.as_ref().ok_or_else(|| {
        failure(
            "RECOVERY_CONTEXT_PROOF_CHANGED",
            "the saved AskPreviousRelation has no retained ContentPlan",
        )
    })?;
    let prior_render = previous.render.as_ref().ok_or_else(|| {
        failure(
            "RECOVERY_CONTEXT_PROOF_CHANGED",
            "the saved AskPreviousRelation has no retained render record",
        )
    })?;
    let mut unsigned_plan = prior_plan.clone();
    unsigned_plan.id.clear();
    if prior_plan.id != identity(&unsigned_plan)?
        || prior_plan.text != previous.text
        || prior_plan.guarantee != previous.guarantee
        || !crate::core::validate_render(prior_plan, prior_render)
        || prior_plan.dependencies.get(..previous.dependencies.len())
            != Some(previous.dependencies.as_slice())
    {
        return Err(failure(
            "RECOVERY_CONTEXT_PROOF_CHANGED",
            "the saved AskPreviousRelation output or render identity is inconsistent",
        ));
    }

    let original_memory = result_memory_snapshot(store, actor, previous)?.ok_or_else(|| {
        failure(
            "RECOVERY_MEMORY_REVISION_MISSING",
            "the saved AskPreviousRelation has no retained actor-memory revision",
        )
    })?;
    let original_resolved = match crate::core::resolve_previous_relation_proposition(
        &original_memory,
        anchor,
        proposition,
        explicit_scope.as_deref(),
    ) {
        crate::core::PreviousRelationResolution::Resolved(value) => value,
        crate::core::PreviousRelationResolution::Missing
        | crate::core::PreviousRelationResolution::Ambiguous => {
            return Err(failure(
                "RECOVERY_CONTEXT_PROOF_CHANGED",
                "the saved AskPreviousRelation no longer resolves to one direct premise in its original revision",
            ));
        }
    };
    let original_answer = crate::relations::query(
        &original_memory,
        &original_resolved,
        prior_bundle.request.budget.max_actions,
    )?;
    let original_units = crate::core::previous_relation_success_units(
        &original_memory,
        &original_resolved,
        &original_answer,
        *explain,
    )
    .ok_or_else(|| {
        failure(
            "RECOVERY_CONTEXT_PROOF_CHANGED",
            "the saved AskPreviousRelation no longer has its adopted answer in the original revision",
        )
    })?;
    if crate::relations::answer_dependencies(&original_answer) != previous.dependencies
        || crate::relations::answer_guarantee(&original_answer) != previous.guarantee
        || original_units != prior_plan.required_units
    {
        return Err(failure(
            "RECOVERY_CONTEXT_PROOF_CHANGED",
            "the saved AskPreviousRelation differs from its original actor-memory proof revision",
        ));
    }

    let current = crate::core::indexed_previous_relation_query_from_store(
        store,
        actor,
        anchor,
        proposition,
        explicit_scope.as_deref(),
        goal.budget.max_actions,
    )?;
    let (current_memory, current_resolved, current_answer) = if let Some(query) = current {
        let resolved = match query.resolution {
            Some(crate::core::PreviousRelationResolution::Resolved(value)) => value,
            Some(
                crate::core::PreviousRelationResolution::Missing
                | crate::core::PreviousRelationResolution::Ambiguous,
            )
            | None => {
                return Err(failure(
                    "RECOVERY_CONTEXT_PROOF_CHANGED",
                    "the saved AskPreviousRelation anchor no longer resolves uniquely at the current head",
                ));
            }
        };
        let answer = query.answer?.ok_or_else(|| {
            failure(
                "RECOVERY_CONTEXT_PROOF_CHANGED",
                "the saved AskPreviousRelation has no supported or contradicted result at the current head",
            )
        })?;
        (query.memory, resolved, answer)
    } else {
        let memory = store.load_memory(actor)?;
        let resolved = match crate::core::resolve_previous_relation_proposition(
            &memory,
            anchor,
            proposition,
            explicit_scope.as_deref(),
        ) {
            crate::core::PreviousRelationResolution::Resolved(value) => value,
            crate::core::PreviousRelationResolution::Missing
            | crate::core::PreviousRelationResolution::Ambiguous => {
                return Err(failure(
                    "RECOVERY_CONTEXT_PROOF_CHANGED",
                    "the saved AskPreviousRelation anchor no longer resolves uniquely at the current head",
                ));
            }
        };
        let answer = crate::relations::query(&memory, &resolved, goal.budget.max_actions)?;
        (memory, resolved, answer)
    };
    let current_units = crate::core::previous_relation_success_units(
        &current_memory,
        &current_resolved,
        &current_answer,
        *explain,
    )
    .ok_or_else(|| {
        failure(
            "RECOVERY_CONTEXT_PROOF_CHANGED",
            "the saved AskPreviousRelation no longer has its adopted answer at the current head",
        )
    })?;
    if current_resolved != original_resolved
        || crate::relations::answer_dependencies(&current_answer) != previous.dependencies
        || crate::relations::answer_guarantee(&current_answer) != previous.guarantee
        || current_units != prior_plan.required_units
    {
        return Err(failure(
            "RECOVERY_CONTEXT_PROOF_CHANGED",
            "the saved AskPreviousRelation anchor or proof changed at the current head",
        ));
    }

    if crate::core::previous_relation_meaning(store, actor, case, previous)?.is_none()
        && (result.dependencies != [previous.id.clone()]
            || result.guarantee != previous.guarantee
            || result.calculation.is_some()
            || plan.required_units != [previous.text.clone()]
            || plan.text != result.text
            || result.text != previous.text
            || plan.guarantee != result.guarantee
            || plan.dependencies.get(..result.dependencies.len())
                != Some(result.dependencies.as_slice()))
    {
        return Err(failure(
            "RECOVERY_CONTEXT_PROOF_CHANGED",
            "the explanation does not preserve the exact saved AskPreviousRelation result",
        ));
    }
    Ok(())
}

fn verify_previous_relation_context(
    store: &Store,
    actor: &str,
    case: &str,
    state: &CaseRevision,
    result: &ResultRecord,
    plan: &ContentPlan,
    goal: &GoalContract,
) -> Result<()> {
    let Some(previous_id) = state
        .bundle
        .as_ref()
        .and_then(|bundle| bundle.meaning.context_result_ref.as_ref())
    else {
        return Ok(());
    };
    let Some(previous) = state.prior_results.get(previous_id) else {
        return Ok(());
    };
    if previous.lifecycle != Lifecycle::Commit {
        return Err(failure(
            "RECOVERY_CONTEXT_PROOF_CHANGED",
            "the prior result is not committed",
        ));
    }
    match store.get_input_bundle(actor, case, &previous.input_id, previous.input_version)? {
        Some((_, prior_bundle)) if identity(&prior_bundle.request)? != previous.id => {
            return Err(failure(
                "RECOVERY_CONTEXT_PROOF_CHANGED",
                "the saved prior result is not bound to its sealed input identity",
            ));
        }
        Some(_) => {}
        None if previous.actor_memory_revision.is_some() => {
            return Err(failure(
                "RECOVERY_CONTEXT_PROOF_CHANGED",
                "the saved prior result is not bound to its sealed input identity",
            ));
        }
        None => {}
    }
    if previous.calculation.is_some() {
        return Ok(());
    }
    if let Some((subject, _, _)) =
        crate::core::previous_relation_profile_request(store, actor, case, previous)?
    {
        if let Some((_, current_answer)) = crate::core::indexed_relation_profile_answer_from_store(
            store,
            actor,
            &subject,
            goal.budget.max_actions,
            goal.budget.max_evidence_bytes,
        )
        .map_err(|error| {
            failure(
                "RECOVERY_CONTEXT_PROOF_CHANGED",
                format!("the saved relation profile behind the explanation changed: {error}"),
            )
        })? {
            let current_answer = current_answer.map_err(|error| {
                failure(
                    "RECOVERY_CONTEXT_PROOF_CHANGED",
                    format!("the saved relation profile behind the explanation changed: {error}"),
                )
            })?;
            return verify_explain_previous_relation_profile_answer_result(
                store,
                state,
                previous,
                result,
                plan,
                goal,
                (&subject, current_answer),
            )
            .map_err(|error| {
                failure(
                    "RECOVERY_CONTEXT_PROOF_CHANGED",
                    format!("the saved relation profile behind the explanation changed: {error}"),
                )
            });
        }
        let memory = store.load_memory(actor)?;
        return verify_explain_previous_relation_profile_memory_result(
            store, state, &memory, previous, result, plan, goal,
        )
        .map_err(|error| {
            failure(
                "RECOVERY_CONTEXT_PROOF_CHANGED",
                format!("the saved relation profile behind the explanation changed: {error}"),
            )
        });
    }
    if crate::core::is_saved_relation_comparison_result(previous) {
        return verify_explain_previous_relation_comparison_memory_result(
            store, state, None, previous, result, plan, goal,
        )
        .map_err(|error| {
            failure(
                "RECOVERY_CONTEXT_PROOF_CHANGED",
                format!("the saved comparison behind the explanation changed: {error}"),
            )
        });
    }
    if crate::core::is_common_relations_result(previous) {
        return verify_explain_previous_common_relations_memory_result(
            store, state, None, previous, result, plan, goal,
        )
        .map_err(|error| {
            failure(
                "RECOVERY_CONTEXT_PROOF_CHANGED",
                format!("the common-relations report behind the explanation changed: {error}"),
            )
        });
    }
    if let Some((_, prior_bundle)) =
        store.get_input_bundle(actor, case, &previous.input_id, previous.input_version)?
        && matches!(prior_bundle.meaning.intent, Intent::InspectPremises { .. })
    {
        return verify_explain_previous_premise_inventory_memory_result(
            store, state, previous, result, plan, goal,
        )
        .map_err(|error| {
            failure(
                "RECOVERY_CONTEXT_PROOF_CHANGED",
                format!("the saved premise inventory behind the explanation changed: {error}"),
            )
        });
    }
    if let Some((_, prior_bundle)) =
        store.get_input_bundle(actor, case, &previous.input_id, previous.input_version)?
        && matches!(
            prior_bundle.meaning.intent,
            Intent::AskRelationObjects { .. }
        )
    {
        return verify_explain_previous_relation_objects_memory_result(
            store, state, None, previous, result, plan, goal,
        )
        .map_err(|error| {
            failure(
                "RECOVERY_CONTEXT_PROOF_CHANGED",
                format!("the relation-object list behind the explanation changed: {error}"),
            )
        });
    }
    if let Some((_, prior_bundle)) =
        store.get_input_bundle(actor, case, &previous.input_id, previous.input_version)?
        && matches!(&prior_bundle.meaning.intent, Intent::AskConjunction { .. })
    {
        return verify_explain_previous_conjunction_memory_result(
            store, state, None, previous, result, plan, goal,
        )
        .map_err(|error| {
            failure(
                "RECOVERY_CONTEXT_PROOF_CHANGED",
                format!("the saved conjunction behind the explanation changed: {error}"),
            )
        });
    }
    verify_saved_previous_relation_query_context(
        store,
        actor,
        case,
        state,
        previous,
        (result, plan),
        goal,
    )
    .map_err(|error| {
        failure(
            "RECOVERY_CONTEXT_PROOF_CHANGED",
            format!("the saved prior-relation answer changed: {error}"),
        )
    })?;
    let Some(prior_meaning) = crate::core::previous_relation_meaning(store, actor, case, previous)?
    else {
        return Ok(());
    };
    let (proposition, adopted_proof_dependencies) = match prior_meaning {
        crate::core::PriorRelationMeaning::OriginalQuestion(proposition) => (proposition, None),
        crate::core::PriorRelationMeaning::AdoptedConclusion {
            proposition,
            proof_dependencies,
        } => (proposition, Some(proof_dependencies)),
    };
    if let Some((memory, answer)) = crate::core::indexed_relation_query_from_store(
        store,
        actor,
        &proposition,
        goal.budget.max_actions,
    )
    .map_err(|error| {
        failure(
            "RECOVERY_CONTEXT_PROOF_CHANGED",
            format!("the relation proof behind the previous-answer explanation changed: {error}"),
        )
    })? {
        let answer = answer.map_err(|error| {
            failure(
                "RECOVERY_CONTEXT_PROOF_CHANGED",
                format!(
                    "the relation proof behind the previous-answer explanation changed: {error}"
                ),
            )
        })?;
        return verify_explain_previous_relation_answer(
            &memory,
            previous,
            &proposition,
            adopted_proof_dependencies,
            &answer,
            result,
            plan,
        )
        .map_err(|error| {
            failure(
                "RECOVERY_CONTEXT_PROOF_CHANGED",
                format!(
                    "the relation proof behind the previous-answer explanation changed: {error}"
                ),
            )
        });
    }
    let memory = store.load_memory(actor)?;
    verify_explain_previous_relation_memory_result(store, state, &memory, result, plan, goal)
        .map_err(|error| {
            failure(
                "RECOVERY_CONTEXT_PROOF_CHANGED",
                format!(
                    "the relation proof behind the previous-answer explanation changed: {error}"
                ),
            )
        })
}

fn verify_explain_previous_relation_objects_memory_result(
    store: &Store,
    state: &CaseRevision,
    current_memory: Option<&MemoryState>,
    previous: &ResultRecord,
    result: &ResultRecord,
    plan: &ContentPlan,
    goal: &GoalContract,
) -> Result<()> {
    let bundle = state
        .bundle
        .as_ref()
        .ok_or_else(|| failure("RECOVERY_MEMORY_PROOF_MISMATCH", "sealed input is missing"))?;
    let previous_id = bundle
        .meaning
        .context_result_ref
        .as_deref()
        .ok_or_else(|| {
            failure(
                "RECOVERY_MEMORY_PROOF_MISMATCH",
                "the prior relation-object result reference is missing",
            )
        })?;
    if previous.id != previous_id || previous.lifecycle != Lifecycle::Commit {
        return Err(failure(
            "RECOVERY_MEMORY_PROOF_MISMATCH",
            "the exact prior relation-object result is not committed",
        ));
    }
    let Some((_, prior_bundle)) = store.get_input_bundle(
        &state.actor_id,
        &state.case_id,
        &previous.input_id,
        previous.input_version,
    )?
    else {
        return Err(failure(
            "RECOVERY_MEMORY_PROOF_MISMATCH",
            "the exact relation-object input bundle is not retained",
        ));
    };
    if prior_bundle.request.actor_id != state.actor_id
        || prior_bundle.request.case_id != state.case_id
        || identity(&prior_bundle.request)? != previous.id
    {
        return Err(failure(
            "RECOVERY_MEMORY_PROOF_MISMATCH",
            "the relation-object input and committed result identities differ",
        ));
    }
    let Intent::AskRelationObjects {
        subject,
        predicate,
        negative,
        scope,
    } = &prior_bundle.meaning.intent
    else {
        return Err(failure(
            "RECOVERY_MEMORY_PROOF_MISMATCH",
            "the prior input does not retain a typed relation-object request",
        ));
    };
    let prior_plan = previous.content_plan.as_ref().ok_or_else(|| {
        failure(
            "RECOVERY_MEMORY_PROOF_MISMATCH",
            "the relation-object result has no retained content plan",
        )
    })?;
    let prior_render = previous.render.as_ref().ok_or_else(|| {
        failure(
            "RECOVERY_MEMORY_PROOF_MISMATCH",
            "the relation-object result has no retained render record",
        )
    })?;
    let mut unsigned_prior_plan = prior_plan.clone();
    unsigned_prior_plan.id.clear();
    if prior_plan.id != identity(&unsigned_prior_plan)?
        || prior_plan.text != previous.text
        || prior_plan.guarantee != previous.guarantee
        || prior_plan.required_units != vec![previous.text.clone()]
        || !crate::core::validate_render(prior_plan, prior_render)
    {
        return Err(failure(
            "RECOVERY_MEMORY_PROOF_MISMATCH",
            "the retained relation-object result, content plan or render is invalid",
        ));
    }
    verified_artifact(store, &state.actor_id, &state.case_id, prior_plan)?;
    verified_artifact(store, &state.actor_id, &state.case_id, prior_render)?;

    if let Some(original_memory) = result_memory_snapshot(store, &state.actor_id, previous)? {
        let original_answer = crate::core::relation_objects_answer(
            &original_memory,
            subject,
            predicate,
            *negative,
            scope,
            prior_bundle.request.budget.max_actions,
            prior_bundle.request.budget.max_evidence_bytes,
        )?;
        if previous.text != original_answer.text
            || previous.dependencies != original_answer.dependencies
            || previous.guarantee != original_answer.guarantee
            || prior_plan.required_units != vec![original_answer.text]
        {
            return Err(failure(
                "RECOVERY_MEMORY_PROOF_MISMATCH",
                "the retained relation-object list differs from its actor-memory revision",
            ));
        }
    }

    let replay = crate::core::RelationObjectsReplay {
        subject,
        predicate,
        negative: *negative,
        scope,
        max_actions: goal.budget.max_actions,
        max_evidence_bytes: goal.budget.max_evidence_bytes,
    };
    let evaluation = if let Some(memory) = current_memory {
        crate::core::relation_objects_explanation(memory, previous, replay)?
    } else if let Some((_, evaluation)) =
        crate::core::indexed_relation_objects_explanation_from_store(
            store,
            &state.actor_id,
            previous,
            replay,
        )?
    {
        evaluation?
    } else {
        let memory = store.load_memory(&state.actor_id)?;
        crate::core::relation_objects_explanation(&memory, previous, replay)?
    };
    let explanation = match evaluation {
        crate::core::RelationObjectsExplanationEvaluation::Complete(answer) => answer,
        crate::core::RelationObjectsExplanationEvaluation::Blocked { code, message } => {
            return Err(failure(
                "RECOVERY_MEMORY_PROOF_MISMATCH",
                format!("the relation-object explanation cannot be replayed ({code}: {message})"),
            ));
        }
    };
    if explanation.dependencies != result.dependencies
        || explanation.guarantee != result.guarantee
        || explanation.guarantee != plan.guarantee
        || explanation.units != plan.required_units
        || plan.text != result.text
        || result.calculation.is_some()
        || plan.dependencies.get(..result.dependencies.len())
            != Some(result.dependencies.as_slice())
    {
        return Err(failure(
            "RECOVERY_MEMORY_PROOF_MISMATCH",
            "the relation-object explanation differs from its exact proof, dependencies or units",
        ));
    }
    Ok(())
}

fn verify_explain_previous_premise_inventory_memory_result(
    store: &Store,
    state: &CaseRevision,
    previous: &ResultRecord,
    result: &ResultRecord,
    plan: &ContentPlan,
    goal: &GoalContract,
) -> Result<()> {
    let bundle = state
        .bundle
        .as_ref()
        .ok_or_else(|| failure("RECOVERY_MEMORY_PROOF_MISMATCH", "sealed input is missing"))?;
    let previous_id = bundle
        .meaning
        .context_result_ref
        .as_deref()
        .ok_or_else(|| {
            failure(
                "RECOVERY_MEMORY_PROOF_MISMATCH",
                "the prior premise-inventory result reference is missing",
            )
        })?;
    if previous.id != previous_id
        || previous.lifecycle != Lifecycle::Commit
        || previous.calculation.is_some()
    {
        return Err(failure(
            "RECOVERY_MEMORY_PROOF_MISMATCH",
            "the exact prior premise-inventory result is not committed",
        ));
    }
    let Some((_, prior_bundle)) = store.get_input_bundle(
        &state.actor_id,
        &state.case_id,
        &previous.input_id,
        previous.input_version,
    )?
    else {
        return Err(failure(
            "RECOVERY_MEMORY_PROOF_MISMATCH",
            "the exact premise-inventory input bundle is not retained",
        ));
    };
    if prior_bundle.request.actor_id != state.actor_id
        || prior_bundle.request.case_id != state.case_id
        || identity(&prior_bundle.request)? != previous.id
    {
        return Err(failure(
            "RECOVERY_MEMORY_PROOF_MISMATCH",
            "the premise-inventory input and committed result identities differ",
        ));
    }
    let Intent::InspectPremises { subject } = &prior_bundle.meaning.intent else {
        return Err(failure(
            "RECOVERY_MEMORY_PROOF_MISMATCH",
            "the prior input does not retain a typed premise-inventory request",
        ));
    };
    let prior_plan = previous.content_plan.as_ref().ok_or_else(|| {
        failure(
            "RECOVERY_MEMORY_PROOF_MISMATCH",
            "the premise-inventory result has no retained content plan",
        )
    })?;
    let prior_render = previous.render.as_ref().ok_or_else(|| {
        failure(
            "RECOVERY_MEMORY_PROOF_MISMATCH",
            "the premise-inventory result has no retained render record",
        )
    })?;
    let mut unsigned_prior_plan = prior_plan.clone();
    unsigned_prior_plan.id.clear();
    if prior_plan.id != identity(&unsigned_prior_plan)?
        || prior_plan.text != previous.text
        || prior_plan.guarantee != previous.guarantee
        || prior_plan.dependencies.get(..previous.dependencies.len())
            != Some(previous.dependencies.as_slice())
        || !crate::core::validate_render(prior_plan, prior_render)
    {
        return Err(failure(
            "RECOVERY_MEMORY_PROOF_MISMATCH",
            "the retained premise-inventory result, content plan or render is invalid",
        ));
    }
    verified_artifact(store, &state.actor_id, &state.case_id, prior_plan)?;
    verified_artifact(store, &state.actor_id, &state.case_id, prior_render)?;

    let original_memory =
        result_memory_snapshot(store, &state.actor_id, previous)?.ok_or_else(|| {
            failure(
                "RECOVERY_MEMORY_REVISION_MISSING",
                "the original premise-inventory actor-memory revision is missing",
            )
        })?;
    let original_inventory = crate::relations::inventory_about(
        &original_memory,
        subject,
        prior_bundle.request.budget.max_actions,
    )?;
    let original_units =
        crate::core::premise_inventory_units(subject, &original_inventory).collect::<Vec<_>>();
    if previous.dependencies != original_inventory.dependencies
        || previous.guarantee != original_inventory.guarantee
        || prior_plan.required_units != original_units
    {
        return Err(failure(
            "RECOVERY_MEMORY_PROOF_MISMATCH",
            "the retained premise inventory does not match its actor-memory revision",
        ));
    }

    let current_inventory = match crate::core::indexed_premise_inventory_from_store(
        store,
        &state.actor_id,
        subject,
        goal.budget.max_actions,
    )? {
        Some((_revision, inventory)) => inventory?,
        None => {
            let memory = store.load_memory(&state.actor_id)?;
            crate::relations::inventory_about(&memory, subject, goal.budget.max_actions)?
        }
    };
    if current_inventory != original_inventory {
        return Err(failure(
            "RECOVERY_MEMORY_PROOF_MISMATCH",
            "the current premise inventory differs from the prior committed inventory",
        ));
    }

    if result.dependencies != [previous.id.clone()]
        || result.guarantee != previous.guarantee
        || plan.required_units != [previous.text.clone()]
        || plan.text != result.text
        || result.text != previous.text
        || plan.guarantee != result.guarantee
        || result.calculation.is_some()
        || plan.dependencies.get(..result.dependencies.len())
            != Some(result.dependencies.as_slice())
    {
        return Err(failure(
            "RECOVERY_MEMORY_PROOF_MISMATCH",
            "the premise-inventory explanation differs from its exact prior result",
        ));
    }
    Ok(())
}

fn verify_explain_previous_conjunction_memory_result(
    store: &Store,
    state: &CaseRevision,
    current_memory: Option<&MemoryState>,
    previous: &ResultRecord,
    result: &ResultRecord,
    plan: &ContentPlan,
    goal: &GoalContract,
) -> Result<()> {
    let bundle = state
        .bundle
        .as_ref()
        .ok_or_else(|| failure("RECOVERY_MEMORY_PROOF_MISMATCH", "sealed input is missing"))?;
    let previous_id = bundle
        .meaning
        .context_result_ref
        .as_deref()
        .ok_or_else(|| {
            failure(
                "RECOVERY_MEMORY_PROOF_MISMATCH",
                "the prior conjunction result reference is missing",
            )
        })?;
    if previous.id != previous_id || previous.lifecycle != Lifecycle::Commit {
        return Err(failure(
            "RECOVERY_MEMORY_PROOF_MISMATCH",
            "the exact prior conjunction result is not committed",
        ));
    }
    let Some((_, prior_bundle)) = store.get_input_bundle(
        &state.actor_id,
        &state.case_id,
        &previous.input_id,
        previous.input_version,
    )?
    else {
        return Err(failure(
            "RECOVERY_MEMORY_PROOF_MISMATCH",
            "the exact conjunction input bundle is not retained",
        ));
    };
    if prior_bundle.request.actor_id != state.actor_id
        || prior_bundle.request.case_id != state.case_id
        || identity(&prior_bundle.request)? != previous.id
    {
        return Err(failure(
            "RECOVERY_MEMORY_PROOF_MISMATCH",
            "the conjunction input and committed result identities differ",
        ));
    }
    let Intent::AskConjunction { propositions } = &prior_bundle.meaning.intent else {
        return Err(failure(
            "RECOVERY_MEMORY_PROOF_MISMATCH",
            "the prior input does not retain a typed conjunction request",
        ));
    };
    let prior_plan = previous.content_plan.as_ref().ok_or_else(|| {
        failure(
            "RECOVERY_MEMORY_PROOF_MISMATCH",
            "the conjunction result has no retained content plan",
        )
    })?;
    let prior_render = previous.render.as_ref().ok_or_else(|| {
        failure(
            "RECOVERY_MEMORY_PROOF_MISMATCH",
            "the conjunction result has no retained render record",
        )
    })?;
    let mut unsigned_prior_plan = prior_plan.clone();
    unsigned_prior_plan.id.clear();
    if prior_plan.id != identity(&unsigned_prior_plan)?
        || prior_plan.text != previous.text
        || prior_plan.guarantee != previous.guarantee
        || !crate::core::validate_render(prior_plan, prior_render)
    {
        return Err(failure(
            "RECOVERY_MEMORY_PROOF_MISMATCH",
            "the retained conjunction result, content plan or render is invalid",
        ));
    }
    verified_artifact(store, &state.actor_id, &state.case_id, prior_plan)?;
    verified_artifact(store, &state.actor_id, &state.case_id, prior_render)?;

    if let Some(original_memory) = result_memory_snapshot(store, &state.actor_id, previous)? {
        let original_outcomes = conjunction_memory_outcomes(
            &original_memory,
            propositions,
            prior_bundle.request.budget.max_actions,
        )?;
        verify_conjunction_outcomes(propositions, original_outcomes, previous, prior_plan)?;
    }

    let outcomes = if let Some(memory) = current_memory {
        conjunction_memory_outcomes(memory, propositions, goal.budget.max_actions)?
    } else if let Some((_, outcomes)) =
        crate::core::indexed_conjunction_relation_answers_from_store(
            store,
            &state.actor_id,
            propositions,
            conjunction_per_relation_budget(propositions, goal.budget.max_actions)?,
        )?
    {
        outcomes?
    } else {
        let memory = store.load_memory(&state.actor_id)?;
        conjunction_memory_outcomes(&memory, propositions, goal.budget.max_actions)?
    };
    verify_conjunction_outcomes(propositions, outcomes, previous, prior_plan)?;

    if result.dependencies != vec![previous.id.clone()]
        || result.guarantee != previous.guarantee
        || plan.required_units != vec![previous.text.clone()]
        || plan.text != result.text
        || result.text != previous.text
        || result.calculation.is_some()
        || plan.dependencies.get(..result.dependencies.len())
            != Some(result.dependencies.as_slice())
    {
        return Err(failure(
            "RECOVERY_MEMORY_PROOF_MISMATCH",
            "the conjunction explanation differs from its exact prior result",
        ));
    }
    Ok(())
}

fn verify_explain_previous_common_relations_memory_result(
    store: &Store,
    state: &CaseRevision,
    current_memory: Option<&MemoryState>,
    previous: &ResultRecord,
    result: &ResultRecord,
    plan: &ContentPlan,
    goal: &GoalContract,
) -> Result<()> {
    let bundle = state
        .bundle
        .as_ref()
        .ok_or_else(|| failure("RECOVERY_MEMORY_PROOF_MISMATCH", "sealed input is missing"))?;
    let previous_id = bundle
        .meaning
        .context_result_ref
        .as_deref()
        .ok_or_else(|| {
            failure(
                "RECOVERY_MEMORY_PROOF_MISMATCH",
                "the prior common-relations result reference is missing",
            )
        })?;
    if previous.id != previous_id || previous.lifecycle != Lifecycle::Commit {
        return Err(failure(
            "RECOVERY_MEMORY_PROOF_MISMATCH",
            "the exact prior common-relations result is not committed",
        ));
    }
    let Some((_, prior_bundle)) = store.get_input_bundle(
        &state.actor_id,
        &state.case_id,
        &previous.input_id,
        previous.input_version,
    )?
    else {
        return Err(failure(
            "RECOVERY_MEMORY_PROOF_MISMATCH",
            "the exact common-relations input bundle is not retained",
        ));
    };
    if prior_bundle.request.actor_id != state.actor_id
        || prior_bundle.request.case_id != state.case_id
        || identity(&prior_bundle.request)? != previous.id
    {
        return Err(failure(
            "RECOVERY_MEMORY_PROOF_MISMATCH",
            "the common-relations input and committed result identities differ",
        ));
    }
    let Intent::AskCommonRelations {
        subject_a,
        subject_b,
        scope,
    } = &prior_bundle.meaning.intent
    else {
        return Err(failure(
            "RECOVERY_MEMORY_PROOF_MISMATCH",
            "the prior input does not retain a typed common-relations request",
        ));
    };
    let prior_plan = previous.content_plan.as_ref().ok_or_else(|| {
        failure(
            "RECOVERY_MEMORY_PROOF_MISMATCH",
            "the common-relations result has no retained content plan",
        )
    })?;
    let prior_render = previous.render.as_ref().ok_or_else(|| {
        failure(
            "RECOVERY_MEMORY_PROOF_MISMATCH",
            "the common-relations result has no retained render record",
        )
    })?;
    let mut unsigned_prior_plan = prior_plan.clone();
    unsigned_prior_plan.id.clear();
    if prior_plan.id != identity(&unsigned_prior_plan)?
        || prior_plan.text != previous.text
        || prior_plan.guarantee != previous.guarantee
        || prior_plan.required_units != vec![previous.text.clone()]
        || !crate::core::validate_render(prior_plan, prior_render)
    {
        return Err(failure(
            "RECOVERY_MEMORY_PROOF_MISMATCH",
            "the retained common-relations result, content plan or render is invalid",
        ));
    }
    verified_artifact(store, &state.actor_id, &state.case_id, prior_plan)?;
    verified_artifact(store, &state.actor_id, &state.case_id, prior_render)?;
    let report_ref = previous.dependencies.first().ok_or_else(|| {
        failure(
            "RECOVERY_MEMORY_PROOF_MISMATCH",
            "the common-relations result has no report artifact reference",
        )
    })?;
    let report_bytes = store.get_bytes(&state.actor_id, &state.case_id, report_ref)?;

    if let Some(original_memory) = result_memory_snapshot(store, &state.actor_id, previous)? {
        let original_answer = match crate::core::common_relations_answer(
            &original_memory,
            subject_a,
            subject_b,
            scope,
            prior_bundle.request.budget.max_actions,
            prior_bundle.request.budget.max_evidence_bytes,
            &previous.id,
        )? {
            crate::core::CommonRelationsEvaluation::Complete(answer) => answer,
            crate::core::CommonRelationsEvaluation::Blocked { code, message } => {
                return Err(failure(
                    "RECOVERY_MEMORY_PROOF_MISMATCH",
                    format!(
                        "the original common-relations report cannot be replayed ({code}: {message})"
                    ),
                ));
            }
        };
        let mut expected_previous_dependencies = vec![report_ref.clone()];
        expected_previous_dependencies.extend(original_answer.proof_dependencies.iter().cloned());
        if report_bytes != original_answer.artifact_bytes
            || previous.dependencies != expected_previous_dependencies
            || previous.guarantee != original_answer.guarantee
            || previous.text != original_answer.report
            || prior_plan.required_units != vec![original_answer.report]
        {
            return Err(failure(
                "RECOVERY_MEMORY_PROOF_MISMATCH",
                "the retained common-relations report differs from its actor-memory revision",
            ));
        }
    }

    let replay = crate::core::CommonRelationsExplanationReplay {
        subject_a,
        subject_b,
        scope,
        max_actions: goal.budget.max_actions,
        max_evidence_bytes: goal.budget.max_evidence_bytes,
    };
    let evaluation = if let Some(memory) = current_memory {
        crate::core::common_relations_explanation(
            memory,
            &report_bytes,
            report_ref,
            previous,
            replay,
        )?
    } else if let Some((_, evaluation)) =
        crate::core::indexed_common_relations_explanation_from_store(
            store,
            &state.actor_id,
            &report_bytes,
            report_ref,
            previous,
            replay,
        )?
    {
        evaluation?
    } else {
        let memory = store.load_memory(&state.actor_id)?;
        crate::core::common_relations_explanation(
            &memory,
            &report_bytes,
            report_ref,
            previous,
            replay,
        )?
    };
    let explanation = match evaluation {
        crate::core::CommonRelationsExplanationEvaluation::Complete(answer) => answer,
        crate::core::CommonRelationsExplanationEvaluation::Blocked { code, message } => {
            return Err(failure(
                "RECOVERY_MEMORY_PROOF_MISMATCH",
                format!("the common-relations explanation cannot be replayed ({code}: {message})"),
            ));
        }
    };
    if explanation.dependencies != result.dependencies
        || explanation.guarantee != result.guarantee
        || explanation.guarantee != plan.guarantee
        || explanation.units != plan.required_units
        || plan.text != result.text
        || result.calculation.is_some()
        || plan.dependencies.get(..result.dependencies.len())
            != Some(result.dependencies.as_slice())
    {
        return Err(failure(
            "RECOVERY_MEMORY_PROOF_MISMATCH",
            "the common-relations explanation differs from its exact proof, dependencies or units",
        ));
    }
    Ok(())
}

fn result_memory_snapshot(
    store: &Store,
    actor: &str,
    result: &ResultRecord,
) -> Result<Option<MemoryState>> {
    Ok(result_memory_snapshot_with_digest(store, actor, result)?.map(|(memory, _)| memory))
}

fn result_memory_snapshot_with_digest(
    store: &Store,
    actor: &str,
    result: &ResultRecord,
) -> Result<Option<(MemoryState, String)>> {
    let Some(revision) = result.actor_memory_revision else {
        return Ok(None);
    };
    store
        .load_memory_revision_with_digest(actor, revision)?
        .map(Some)
        .ok_or_else(|| {
            failure(
                "RECOVERY_MEMORY_REVISION_MISSING",
                "the actor-memory revision used by Core is no longer retained",
            )
        })
}

fn premise_memory_inventory(
    memory: &MemoryState,
    subject: &str,
    max_actions: u32,
) -> Result<(Vec<String>, Guarantee)> {
    premise_relation_inventory_summary(crate::relations::inventory_about(
        memory,
        subject,
        max_actions,
    ))
}

fn premise_relation_inventory_summary(
    inventory: Result<crate::relations::RelationInventory>,
) -> Result<(Vec<String>, Guarantee)> {
    let inventory = inventory.map_err(|error| {
        failure(
            "RECOVERY_MEMORY_PROOF_MISMATCH",
            format!("the retained premise inventory cannot be replayed: {error}"),
        )
    })?;
    if inventory.dependencies.is_empty() {
        return Err(failure(
            "RECOVERY_MEMORY_PROOF_MISMATCH",
            "the retained premise inventory has no proof dependencies",
        ));
    }
    Ok((inventory.dependencies, inventory.guarantee))
}

fn conjunction_per_relation_budget(propositions: &[Proposition], max_actions: u32) -> Result<u32> {
    if !(2..=3).contains(&propositions.len()) {
        return Err(failure(
            "RECOVERY_MEMORY_PROOF_MISMATCH",
            "the retained conjunction does not contain two or three propositions",
        ));
    }
    let clause_count = u32::try_from(propositions.len()).map_err(|_| {
        failure(
            "RECOVERY_MEMORY_PROOF_MISMATCH",
            "the retained conjunction clause count is out of range",
        )
    })?;
    let per_relation_budget = max_actions / clause_count;
    if per_relation_budget == 0 {
        return Err(failure(
            "RECOVERY_MEMORY_PROOF_MISMATCH",
            "the retained conjunction has no per-relation proof budget",
        ));
    }
    Ok(per_relation_budget)
}

fn conjunction_memory_outcomes(
    memory: &MemoryState,
    propositions: &[Proposition],
    max_actions: u32,
) -> Result<Vec<crate::relations::QueryOutcome>> {
    let per_relation_budget = conjunction_per_relation_budget(propositions, max_actions)?;
    propositions
        .iter()
        .map(|proposition| crate::relations::query(memory, proposition, per_relation_budget))
        .collect()
}

fn verify_conjunction_outcomes(
    propositions: &[Proposition],
    outcomes: Vec<crate::relations::QueryOutcome>,
    result: &ResultRecord,
    plan: &ContentPlan,
) -> Result<()> {
    if outcomes.len() != propositions.len() {
        return Err(failure(
            "RECOVERY_MEMORY_PROOF_MISMATCH",
            "the retained conjunction has an incomplete clause evaluation",
        ));
    }
    let mut dependencies = std::collections::BTreeSet::new();
    let mut all_supported = true;
    let mut any_derived = false;
    let mut conclusions = Vec::with_capacity(propositions.len());
    for (proposition, answer) in propositions.iter().zip(outcomes) {
        match answer.status.as_str() {
            "SUPPORTED" => conclusions.push(format!(
                "- {}",
                crate::core::relation_answer_surface(proposition, false)
            )),
            "CONTRADICTED" => {
                all_supported = false;
                conclusions.push(format!(
                    "- {}",
                    crate::core::relation_answer_surface(proposition, true)
                ));
            }
            _ => {
                return Err(failure(
                    "RECOVERY_MEMORY_PROOF_MISMATCH",
                    "a retained conjunction clause is no longer supported or contradicted",
                ));
            }
        }
        dependencies.extend(crate::relations::answer_dependencies(&answer));
        any_derived |= crate::relations::answer_guarantee(&answer) == Guarantee::Derived;
    }

    let relation_count = if propositions.len() == 2 {
        "二つ"
    } else {
        "三つ"
    };
    let mut required_units = vec![if all_supported {
        format!("提示された前提の範囲では、尋ねた{relation_count}の関係がともに支持されます。")
    } else {
        format!(
            "提示された根拠は、尋ねた{relation_count}の関係がともに成立するという組み合わせを支持しません。"
        )
    }];
    required_units.extend(conclusions);
    let guarantee = if any_derived {
        Guarantee::Derived
    } else {
        Guarantee::UserPremise
    };
    if dependencies.into_iter().collect::<Vec<_>>() != result.dependencies
        || guarantee != result.guarantee
        || required_units != plan.required_units
    {
        return Err(failure(
            "RECOVERY_MEMORY_PROOF_MISMATCH",
            "the retained conjunction does not match its actor-memory proof revision",
        ));
    }
    Ok(())
}

fn verify_conjunction_memory_result(
    memory: &MemoryState,
    result: &ResultRecord,
    plan: &ContentPlan,
    goal: &GoalContract,
) -> Result<()> {
    let Intent::AskConjunction { propositions } = &goal.intent else {
        return Ok(());
    };
    let outcomes = conjunction_memory_outcomes(memory, propositions, goal.budget.max_actions)?;
    verify_conjunction_outcomes(propositions, outcomes, result, plan)
}

fn verify_conjunction_memory_result_with_index(
    store: &Store,
    actor: &str,
    result: &ResultRecord,
    plan: &ContentPlan,
    goal: &GoalContract,
) -> Result<Option<()>> {
    let Intent::AskConjunction { propositions } = &goal.intent else {
        return Ok(Some(()));
    };
    let per_relation_budget =
        conjunction_per_relation_budget(propositions, goal.budget.max_actions)?;
    let Some((_, outcomes)) = crate::core::indexed_conjunction_relation_answers_from_store(
        store,
        actor,
        propositions,
        per_relation_budget,
    )?
    else {
        return Ok(None);
    };
    verify_conjunction_outcomes(propositions, outcomes?, result, plan)?;
    Ok(Some(()))
}

fn verify_common_relations_memory_result(
    store: &Store,
    actor: &str,
    case: &str,
    memory: &MemoryState,
    result: &ResultRecord,
    plan: &ContentPlan,
    goal: &GoalContract,
) -> Result<()> {
    let Intent::AskCommonRelations {
        subject_a,
        subject_b,
        scope,
    } = &goal.intent
    else {
        return Ok(());
    };
    let evaluation = crate::core::common_relations_answer(
        memory,
        subject_a,
        subject_b,
        scope,
        goal.budget.max_actions,
        goal.budget.max_evidence_bytes,
        &result.id,
    )?;
    verify_common_relations_evaluation(store, actor, case, evaluation, result, plan)
}

fn verify_relation_objects_current_answer(
    answer: Result<crate::core::RelationObjectsAnswer>,
    result: &ResultRecord,
    plan: &ContentPlan,
) -> Result<()> {
    let answer = answer.map_err(|error| {
        failure(
            "RECOVERY_PROOF_INVALIDATED",
            format!("the original object-answer proof is no longer valid: {error}"),
        )
    })?;
    if result.dependencies != answer.dependencies
        || result.guarantee != answer.guarantee
        || plan.required_units != vec![answer.text]
    {
        return Err(failure(
            "RECOVERY_PROOF_INVALIDATED",
            "the object-answer content or proof lineage differs from the original adoption",
        ));
    }
    Ok(())
}

fn verify_common_relations_memory_result_with_index(
    store: &Store,
    actor: &str,
    case: &str,
    result: &ResultRecord,
    plan: &ContentPlan,
    goal: &GoalContract,
) -> Result<Option<()>> {
    let Intent::AskCommonRelations {
        subject_a,
        subject_b,
        scope,
    } = &goal.intent
    else {
        return Ok(Some(()));
    };
    let Some((_, evaluation)) = crate::core::indexed_common_relations_answer_from_store(
        store,
        actor,
        crate::core::CommonRelationsReplay {
            subject_a,
            subject_b,
            scope,
            max_actions: goal.budget.max_actions,
            max_evidence_bytes: goal.budget.max_evidence_bytes,
            result_id: &result.id,
        },
    )?
    else {
        return Ok(None);
    };
    verify_common_relations_evaluation(store, actor, case, evaluation?, result, plan)?;
    Ok(Some(()))
}

fn verify_common_relations_evaluation(
    store: &Store,
    actor: &str,
    case: &str,
    evaluation: crate::core::CommonRelationsEvaluation,
    result: &ResultRecord,
    plan: &ContentPlan,
) -> Result<()> {
    let answer = match evaluation {
        crate::core::CommonRelationsEvaluation::Complete(answer) => answer,
        crate::core::CommonRelationsEvaluation::Blocked { code, message } => {
            return Err(failure(
                "RECOVERY_MEMORY_PROOF_MISMATCH",
                format!(
                    "the retained common-relation result cannot be replayed ({code}: {message})"
                ),
            ));
        }
    };
    let Some(report_ref) = result.dependencies.first() else {
        return Err(failure(
            "RECOVERY_MEMORY_PROOF_MISMATCH",
            "the retained common-relation result has no report artifact reference",
        ));
    };
    let retained_artifact = store.get_bytes(actor, case, report_ref)?;
    let mut expected_dependencies = vec![report_ref.clone()];
    expected_dependencies.extend(answer.proof_dependencies.iter().cloned());
    if retained_artifact != answer.artifact_bytes
        || result.dependencies != expected_dependencies
        || result.guarantee != answer.guarantee
        || plan.required_units != vec![answer.report]
    {
        return Err(failure(
            "RECOVERY_MEMORY_PROOF_MISMATCH",
            "the retained common-relation report, artifact or proof lineage differs from its actor-memory revision",
        ));
    }
    Ok(())
}

fn verify_relation_comparison_memory_result(
    store: &Store,
    actor: &str,
    case: &str,
    memory: &MemoryState,
    result: &ResultRecord,
    plan: &ContentPlan,
    goal: &GoalContract,
) -> Result<()> {
    let Intent::CompareRelationInventories {
        subject_a,
        subject_b,
        scope,
        format,
    } = &goal.intent
    else {
        return Ok(());
    };
    let report_ref = result.dependencies.first().ok_or_else(|| {
        failure(
            "RECOVERY_MEMORY_PROOF_MISMATCH",
            "the retained comparison has no report artifact reference",
        )
    })?;
    let artifact = store.get_bytes(actor, case, report_ref)?;
    crate::core::verify_relation_comparison_memory_result(
        memory,
        &artifact,
        result,
        plan,
        crate::core::RelationComparisonReplay {
            subject_a,
            subject_b,
            scope,
            format: *format,
            max_actions: goal.budget.max_actions,
            max_evidence_bytes: goal.budget.max_evidence_bytes,
            generated: goal
                .required_predicates
                .iter()
                .any(|predicate| predicate == RELATION_COMPARISON_PREDICATE),
        },
    )
}

fn verify_relation_comparison_memory_result_with_index(
    store: &Store,
    actor: &str,
    case: &str,
    result: &ResultRecord,
    plan: &ContentPlan,
    goal: &GoalContract,
) -> Result<Option<()>> {
    let Intent::CompareRelationInventories {
        subject_a,
        subject_b,
        scope,
        format,
    } = &goal.intent
    else {
        return Ok(Some(()));
    };
    let report_ref = result.dependencies.first().ok_or_else(|| {
        failure(
            "RECOVERY_MEMORY_PROOF_MISMATCH",
            "the retained comparison has no report artifact reference",
        )
    })?;
    let artifact = store.get_bytes(actor, case, report_ref)?;
    crate::core::verify_relation_comparison_memory_result_with_index(
        store,
        actor,
        &artifact,
        result,
        plan,
        crate::core::RelationComparisonReplay {
            subject_a,
            subject_b,
            scope,
            format: *format,
            max_actions: goal.budget.max_actions,
            max_evidence_bytes: goal.budget.max_evidence_bytes,
            generated: goal
                .required_predicates
                .iter()
                .any(|predicate| predicate == RELATION_COMPARISON_PREDICATE),
        },
    )
}

fn verify_relation_profile_memory_result(
    memory: &MemoryState,
    result: &ResultRecord,
    plan: &ContentPlan,
    goal: &GoalContract,
) -> Result<()> {
    let Intent::DraftRelationProfile { subject } = &goal.intent else {
        return Ok(());
    };
    let answer = crate::core::relation_profile_answer(
        memory,
        subject,
        goal.budget.max_actions,
        goal.budget.max_evidence_bytes,
    );
    verify_relation_profile_answer(answer, result, plan)
}

fn verify_relation_profile_memory_result_with_index(
    store: &Store,
    actor: &str,
    result: &ResultRecord,
    plan: &ContentPlan,
    goal: &GoalContract,
) -> Result<Option<()>> {
    let Intent::DraftRelationProfile { subject } = &goal.intent else {
        return Ok(Some(()));
    };
    let Some((_, answer)) = crate::core::indexed_relation_profile_answer_from_store(
        store,
        actor,
        subject,
        goal.budget.max_actions,
        goal.budget.max_evidence_bytes,
    )?
    else {
        return Ok(None);
    };
    verify_relation_profile_answer(answer, result, plan)?;
    Ok(Some(()))
}

fn verify_relation_profile_answer(
    answer: Result<crate::core::RelationProfileAnswer>,
    result: &ResultRecord,
    plan: &ContentPlan,
) -> Result<()> {
    let answer = answer.map_err(|error| {
        failure(
            "RECOVERY_MEMORY_PROOF_MISMATCH",
            format!("the retained relation profile cannot be replayed: {error}"),
        )
    })?;
    if answer.dependencies != result.dependencies
        || answer.guarantee != result.guarantee
        || answer.units != plan.required_units
    {
        return Err(failure(
            "RECOVERY_MEMORY_PROOF_MISMATCH",
            "the retained relation profile does not match its actor-memory revision",
        ));
    }
    Ok(())
}

fn verify_previous_relation_memory_result(
    state: &CaseRevision,
    memory: &MemoryState,
    result: &ResultRecord,
    plan: &ContentPlan,
    goal: &GoalContract,
) -> Result<()> {
    let Intent::AskPreviousRelation {
        proposition,
        explain,
        explicit_scope,
    } = &goal.intent
    else {
        return Ok(());
    };
    let bundle = state
        .bundle
        .as_ref()
        .ok_or_else(|| failure("RECOVERY_MEMORY_PROOF_MISMATCH", "sealed input is missing"))?;
    let previous_id = bundle
        .meaning
        .context_result_ref
        .as_deref()
        .ok_or_else(|| {
            failure(
                "RECOVERY_MEMORY_PROOF_MISMATCH",
                "the prior relation result reference is missing",
            )
        })?;
    let previous = state.prior_results.get(previous_id).ok_or_else(|| {
        failure(
            "RECOVERY_MEMORY_PROOF_MISMATCH",
            "the exact prior relation result is not retained",
        )
    })?;
    if previous.id != previous_id || previous.lifecycle != Lifecycle::Commit {
        return Err(failure(
            "RECOVERY_MEMORY_PROOF_MISMATCH",
            "the exact prior relation result is not committed",
        ));
    }
    let resolved = match crate::core::resolve_previous_relation_proposition(
        memory,
        previous,
        proposition,
        explicit_scope.as_deref(),
    ) {
        crate::core::PreviousRelationResolution::Resolved(proposition) => proposition,
        crate::core::PreviousRelationResolution::Missing
        | crate::core::PreviousRelationResolution::Ambiguous => {
            return Err(failure(
                "RECOVERY_MEMORY_PROOF_MISMATCH",
                "the exact prior result no longer resolves to one active direct premise",
            ));
        }
    };
    let answer = crate::relations::query(memory, &resolved, goal.budget.max_actions)?;
    let expected_units =
        crate::core::previous_relation_success_units(memory, &resolved, &answer, *explain)
            .ok_or_else(|| {
                failure(
                    "RECOVERY_MEMORY_PROOF_MISMATCH",
                    "the prior relation no longer has the adopted supported or contradicted result",
                )
            })?;
    if crate::relations::answer_dependencies(&answer) != result.dependencies
        || crate::relations::answer_guarantee(&answer) != result.guarantee
        || expected_units != plan.required_units
    {
        return Err(failure(
            "RECOVERY_MEMORY_PROOF_MISMATCH",
            "the prior relation answer differs from its actor-memory proof revision",
        ));
    }
    Ok(())
}

fn verify_previous_relation_memory_result_with_index(
    store: &Store,
    actor: &str,
    state: &CaseRevision,
    result: &ResultRecord,
    plan: &ContentPlan,
    goal: &GoalContract,
) -> Result<Option<()>> {
    let Intent::AskPreviousRelation {
        proposition,
        explain,
        explicit_scope,
    } = &goal.intent
    else {
        return Ok(Some(()));
    };
    let bundle = state
        .bundle
        .as_ref()
        .ok_or_else(|| failure("RECOVERY_MEMORY_PROOF_MISMATCH", "sealed input is missing"))?;
    let previous_id = bundle
        .meaning
        .context_result_ref
        .as_deref()
        .ok_or_else(|| {
            failure(
                "RECOVERY_MEMORY_PROOF_MISMATCH",
                "the prior relation result reference is missing",
            )
        })?;
    let previous = state.prior_results.get(previous_id).ok_or_else(|| {
        failure(
            "RECOVERY_MEMORY_PROOF_MISMATCH",
            "the exact prior relation result is not retained",
        )
    })?;
    if previous.id != previous_id || previous.lifecycle != Lifecycle::Commit {
        return Err(failure(
            "RECOVERY_MEMORY_PROOF_MISMATCH",
            "the exact prior relation result is not committed",
        ));
    }
    let Some(query) = crate::core::indexed_previous_relation_query_from_store(
        store,
        actor,
        previous,
        proposition,
        explicit_scope.as_deref(),
        goal.budget.max_actions,
    )?
    else {
        return Ok(None);
    };
    let answer = query.answer?.ok_or_else(|| {
        failure(
            "RECOVERY_MEMORY_PROOF_MISMATCH",
            "the prior relation no longer has the adopted supported or contradicted result",
        )
    })?;
    let resolved = match query.resolution {
        Some(crate::core::PreviousRelationResolution::Resolved(resolved)) => resolved,
        Some(
            crate::core::PreviousRelationResolution::Missing
            | crate::core::PreviousRelationResolution::Ambiguous,
        )
        | None => {
            return Err(failure(
                "RECOVERY_MEMORY_PROOF_MISMATCH",
                "the exact prior result no longer resolves to one active direct premise",
            ));
        }
    };
    let expected_units =
        crate::core::previous_relation_success_units(&query.memory, &resolved, &answer, *explain)
            .ok_or_else(|| {
            failure(
                "RECOVERY_MEMORY_PROOF_MISMATCH",
                "the prior relation no longer has the adopted supported or contradicted result",
            )
        })?;
    if crate::relations::answer_dependencies(&answer) != result.dependencies
        || crate::relations::answer_guarantee(&answer) != result.guarantee
        || expected_units != plan.required_units
    {
        return Err(failure(
            "RECOVERY_MEMORY_PROOF_MISMATCH",
            "the prior relation answer differs from its actor-memory proof revision",
        ));
    }
    Ok(Some(()))
}

fn verify_explain_previous_relation_comparison_memory_result(
    store: &Store,
    state: &CaseRevision,
    current_memory: Option<&MemoryState>,
    previous: &ResultRecord,
    result: &ResultRecord,
    plan: &ContentPlan,
    goal: &GoalContract,
) -> Result<()> {
    if !matches!(goal.intent, Intent::ExplainPrevious) {
        return Ok(());
    }
    let Some(bundle) = state.bundle.as_ref() else {
        return Err(failure(
            "RECOVERY_MEMORY_PROOF_MISMATCH",
            "sealed explanation input is missing",
        ));
    };
    if bundle.meaning.context_result_ref.as_deref() != Some(previous.id.as_str()) {
        return Err(failure(
            "RECOVERY_MEMORY_PROOF_MISMATCH",
            "the exact saved comparison is not the sealed explanation target",
        ));
    }
    let (Some(prior_plan), Some(prior_render)) = (&previous.content_plan, &previous.render) else {
        return Err(failure(
            "RECOVERY_MEMORY_PROOF_MISMATCH",
            "the saved comparison has no retained ContentPlan or render record",
        ));
    };
    let Some((_, prior_bundle)) = store.get_input_bundle(
        &state.actor_id,
        &state.case_id,
        &previous.input_id,
        previous.input_version,
    )?
    else {
        return Err(failure(
            "RECOVERY_MEMORY_PROOF_MISMATCH",
            "the exact saved comparison input bundle is not retained",
        ));
    };
    if prior_bundle.request.actor_id != state.actor_id
        || prior_bundle.request.case_id != state.case_id
        || identity(&prior_bundle.request)? != previous.id
    {
        return Err(failure(
            "RECOVERY_MEMORY_PROOF_MISMATCH",
            "the saved comparison input and committed result identities differ",
        ));
    }
    let Intent::CompareRelationInventories {
        subject_a,
        subject_b,
        scope,
        format,
    } = &prior_bundle.meaning.intent
    else {
        return Err(failure(
            "RECOVERY_MEMORY_PROOF_MISMATCH",
            "the saved comparison input does not retain a typed comparison request",
        ));
    };
    let report_ref = previous.dependencies.first().ok_or_else(|| {
        failure(
            "RECOVERY_MEMORY_PROOF_MISMATCH",
            "the saved comparison has no report artifact reference",
        )
    })?;
    let report_bytes = store.get_bytes(&state.actor_id, &state.case_id, report_ref)?;
    let mut unsigned_plan = prior_plan.clone();
    unsigned_plan.id.clear();
    if previous.id.is_empty()
        || previous.lifecycle != Lifecycle::Commit
        || previous.calculation.is_some()
        || previous.guarantee != Guarantee::Hypothesis
        || prior_plan.guarantee != Guarantee::Hypothesis
        || prior_plan.id != identity(&unsigned_plan)?
        || prior_plan.text != previous.text
        || !validate_render(prior_plan, prior_render)
        || previous.dependencies.first() != Some(report_ref)
    {
        return Err(failure(
            "RECOVERY_MEMORY_PROOF_MISMATCH",
            "the saved comparison result, plan or render identity is invalid",
        ));
    }
    let prior_content_bytes = canonical(prior_plan)?;
    let prior_render_bytes = canonical(prior_render)?;
    for (reference, expected) in [
        (digest(&prior_content_bytes), prior_content_bytes),
        (digest(&prior_render_bytes), prior_render_bytes),
    ] {
        match store.get_bytes_if_present(&state.actor_id, &state.case_id, &reference)? {
            Some(stored) if stored == expected => {}
            Some(_) => {
                return Err(failure(
                    "RECOVERY_MEMORY_PROOF_MISMATCH",
                    "the saved comparison plan or render differs from its stored artifact",
                ));
            }
            None => {
                return Err(failure(
                    "RECOVERY_MEMORY_PROOF_MISMATCH",
                    "the saved comparison plan or render artifact is missing",
                ));
            }
        }
    }
    let original_replay = crate::core::RelationComparisonReplay {
        subject_a,
        subject_b,
        scope,
        format: *format,
        max_actions: prior_bundle.request.budget.max_actions,
        max_evidence_bytes: prior_bundle.request.budget.max_evidence_bytes,
        generated: crate::input::uses_generated_comparison(&prior_bundle.meaning),
    };
    if previous.actor_memory_revision.is_some() {
        let original_memory = result_memory_snapshot(store, &state.actor_id, previous)?
            .ok_or_else(|| {
                failure(
                    "RECOVERY_MEMORY_REVISION_MISSING",
                    "the saved comparison's actor-memory revision is not retained",
                )
            })?;
        crate::core::verify_relation_comparison_memory_result(
            &original_memory,
            &report_bytes,
            previous,
            prior_plan,
            original_replay,
        )?;
    }
    let current_replay = crate::core::RelationComparisonReplay {
        max_actions: goal.budget.max_actions,
        ..original_replay
    };
    if let Some(memory) = current_memory {
        crate::core::verify_relation_comparison_memory_result(
            memory,
            &report_bytes,
            previous,
            prior_plan,
            current_replay,
        )?;
    } else if crate::core::verify_relation_comparison_memory_result_with_index(
        store,
        &state.actor_id,
        &report_bytes,
        previous,
        prior_plan,
        current_replay,
    )?
    .is_none()
    {
        let memory = store.load_memory(&state.actor_id)?;
        crate::core::verify_relation_comparison_memory_result(
            &memory,
            &report_bytes,
            previous,
            prior_plan,
            current_replay,
        )?;
    }

    if original_replay.generated {
        crate::core::verify_generated_model_artifacts(
            store,
            &state.actor_id,
            &state.case_id,
            prior_plan,
        )?;
    }
    let expected_dependencies = std::iter::once(previous.id.clone())
        .chain(previous.dependencies.iter().cloned())
        .collect::<Vec<_>>();
    let expected_units = crate::core::relation_comparison_explanation_units(previous, prior_plan);
    if expected_dependencies != result.dependencies
        || expected_units != plan.required_units
        || report_bytes.len() > goal.budget.max_evidence_bytes
        || expected_units.join("\n").len() > goal.budget.max_evidence_bytes
        || result.guarantee != Guarantee::Hypothesis
        || plan.guarantee != Guarantee::Hypothesis
        || result.calculation.is_some()
        || plan.text != result.text
        || plan.dependencies.get(..result.dependencies.len())
            != Some(result.dependencies.as_slice())
    {
        return Err(failure(
            "RECOVERY_MEMORY_PROOF_MISMATCH",
            "the saved comparison explanation differs from its exact proof, dependencies or units",
        ));
    }
    Ok(())
}

fn verify_explain_previous_relation_memory_result(
    store: &Store,
    state: &CaseRevision,
    memory: &MemoryState,
    result: &ResultRecord,
    plan: &ContentPlan,
    goal: &GoalContract,
) -> Result<()> {
    if !matches!(goal.intent, Intent::ExplainPrevious) {
        return Ok(());
    }
    let bundle = state
        .bundle
        .as_ref()
        .ok_or_else(|| failure("RECOVERY_MEMORY_PROOF_MISMATCH", "sealed input is missing"))?;
    let previous_id = bundle
        .meaning
        .context_result_ref
        .as_deref()
        .ok_or_else(|| {
            failure(
                "RECOVERY_MEMORY_PROOF_MISMATCH",
                "the prior relation result reference is missing",
            )
        })?;
    let previous = state.prior_results.get(previous_id).ok_or_else(|| {
        failure(
            "RECOVERY_MEMORY_PROOF_MISMATCH",
            "the exact prior relation result is not retained",
        )
    })?;
    if previous.id != previous_id
        || previous.lifecycle != Lifecycle::Commit
        || previous.calculation.is_some()
    {
        return Err(failure(
            "RECOVERY_MEMORY_PROOF_MISMATCH",
            "the exact prior relation result is not a committed relation answer",
        ));
    }
    if crate::core::previous_relation_profile_request(
        store,
        &state.actor_id,
        &state.case_id,
        previous,
    )?
    .is_some()
    {
        return verify_explain_previous_relation_profile_memory_result(
            store, state, memory, previous, result, plan, goal,
        );
    }
    if crate::core::is_saved_relation_comparison_result(previous) {
        return verify_explain_previous_relation_comparison_memory_result(
            store,
            state,
            Some(memory),
            previous,
            result,
            plan,
            goal,
        );
    }
    if crate::core::is_common_relations_result(previous) {
        return verify_explain_previous_common_relations_memory_result(
            store,
            state,
            Some(memory),
            previous,
            result,
            plan,
            goal,
        );
    }
    if let Some((_, prior_bundle)) = store.get_input_bundle(
        &state.actor_id,
        &state.case_id,
        &previous.input_id,
        previous.input_version,
    )? && matches!(
        prior_bundle.meaning.intent,
        Intent::AskRelationObjects { .. }
    ) {
        return verify_explain_previous_relation_objects_memory_result(
            store,
            state,
            Some(memory),
            previous,
            result,
            plan,
            goal,
        );
    }
    let prior_meaning =
        crate::core::previous_relation_meaning(store, &state.actor_id, &state.case_id, previous)?
            .ok_or_else(|| {
            failure(
                "RECOVERY_MEMORY_PROOF_MISMATCH",
                "the exact prior result does not retain a relation meaning",
            )
        })?;
    let (proposition, adopted_proof_dependencies) = match prior_meaning {
        crate::core::PriorRelationMeaning::OriginalQuestion(proposition) => (proposition, None),
        crate::core::PriorRelationMeaning::AdoptedConclusion {
            proposition,
            proof_dependencies,
        } => (proposition, Some(proof_dependencies)),
    };
    let answer = crate::relations::query(memory, &proposition, goal.budget.max_actions)?;
    verify_explain_previous_relation_answer(
        memory,
        previous,
        &proposition,
        adopted_proof_dependencies,
        &answer,
        result,
        plan,
    )
}

fn verify_explain_previous_relation_answer(
    memory: &MemoryState,
    previous: &ResultRecord,
    proposition: &Proposition,
    adopted_proof_dependencies: Option<Vec<String>>,
    answer: &crate::relations::QueryOutcome,
    result: &ResultRecord,
    plan: &ContentPlan,
) -> Result<()> {
    let explained = match adopted_proof_dependencies {
        None => crate::core::relation_explanation_target(proposition, answer).ok_or_else(|| {
            failure(
                "RECOVERY_MEMORY_PROOF_MISMATCH",
                "the prior relation no longer has a supported or refuted result",
            )
        })?,
        Some(expected_proof_dependencies) => {
            if answer.status != "SUPPORTED"
                || crate::relations::answer_dependencies(answer) != expected_proof_dependencies
            {
                return Err(failure(
                    "RECOVERY_MEMORY_PROOF_MISMATCH",
                    "the adopted prior relation is no longer supported by the same proof",
                ));
            }
            proposition.clone()
        }
    };
    let proof_dependencies = crate::relations::answer_dependencies(answer);
    let guarantee = crate::relations::answer_guarantee(answer);
    let mut expected_dependencies = vec![previous.id.clone()];
    expected_dependencies.extend(proof_dependencies.iter().cloned());
    let expected_units =
        crate::core::relation_explanation_lines(memory, &explained, answer, "前回の結論");
    if proof_dependencies != previous.dependencies
        || expected_dependencies != result.dependencies
        || guarantee != previous.guarantee
        || guarantee != result.guarantee
        || expected_units != plan.required_units
    {
        return Err(failure(
            "RECOVERY_MEMORY_PROOF_MISMATCH",
            "the prior-answer explanation does not match its actor-memory proof revision",
        ));
    }
    Ok(())
}

fn verify_explain_previous_relation_profile_memory_result(
    store: &Store,
    state: &CaseRevision,
    memory: &MemoryState,
    previous: &ResultRecord,
    result: &ResultRecord,
    plan: &ContentPlan,
    goal: &GoalContract,
) -> Result<()> {
    let Some((subject, _, _)) = crate::core::previous_relation_profile_request(
        store,
        &state.actor_id,
        &state.case_id,
        previous,
    )?
    else {
        return Err(failure(
            "RECOVERY_MEMORY_PROOF_MISMATCH",
            "the exact prior result is not a typed relation profile",
        ));
    };
    let current_answer = crate::core::relation_profile_answer(
        memory,
        &subject,
        goal.budget.max_actions,
        goal.budget.max_evidence_bytes,
    )
    .map_err(|error| {
        failure(
            "RECOVERY_MEMORY_PROOF_MISMATCH",
            format!("the saved relation profile cannot be replayed in this memory: {error}"),
        )
    })?;
    verify_explain_previous_relation_profile_answer_result(
        store,
        state,
        previous,
        result,
        plan,
        goal,
        (&subject, current_answer),
    )
}

fn verify_explain_previous_relation_profile_answer_result(
    store: &Store,
    state: &CaseRevision,
    previous: &ResultRecord,
    result: &ResultRecord,
    plan: &ContentPlan,
    goal: &GoalContract,
    current_profile: (&str, crate::core::RelationProfileAnswer),
) -> Result<()> {
    let (subject, current_answer) = current_profile;
    let bundle = state
        .bundle
        .as_ref()
        .ok_or_else(|| failure("RECOVERY_MEMORY_PROOF_MISMATCH", "sealed input is missing"))?;
    let previous_id = bundle
        .meaning
        .context_result_ref
        .as_deref()
        .ok_or_else(|| {
            failure(
                "RECOVERY_MEMORY_PROOF_MISMATCH",
                "the prior relation-profile result reference is missing",
            )
        })?;
    if previous.id != previous_id
        || previous.lifecycle != Lifecycle::Commit
        || previous.calculation.is_some()
    {
        return Err(failure(
            "RECOVERY_MEMORY_PROOF_MISMATCH",
            "the exact prior relation-profile result is not committed",
        ));
    }
    let previous_memory =
        result_memory_snapshot(store, &state.actor_id, previous)?.ok_or_else(|| {
            failure(
                "RECOVERY_MEMORY_REVISION_MISSING",
                "the original profile actor-memory revision is not retained",
            )
        })?;
    let (previous_subject, original_answer) = crate::core::relation_profile_result_from_memory(
        store,
        &state.actor_id,
        &state.case_id,
        previous,
        &previous_memory,
    )?;
    if previous_subject != subject {
        return Err(failure(
            "RECOVERY_MEMORY_PROOF_MISMATCH",
            "the saved relation profile subject changed behind the explanation",
        ));
    }
    let previous_memory_revision = previous.actor_memory_revision.ok_or_else(|| {
        failure(
            "RECOVERY_MEMORY_REVISION_MISSING",
            "the original profile actor-memory revision is missing",
        )
    })?;
    let expected_units = crate::core::relation_profile_explanation_units(
        &previous.id,
        previous_memory_revision,
        &current_answer,
    );
    let expected_dependencies = std::iter::once(previous.id.clone())
        .chain(previous.dependencies.iter().cloned())
        .collect::<Vec<_>>();
    if current_answer != original_answer
        || result.actor_memory_revision.is_none()
        || result.dependencies != expected_dependencies
        || result.guarantee != previous.guarantee
        || result.guarantee != current_answer.guarantee
        || result.calculation.is_some()
        || plan.text != result.text
        || plan.guarantee != result.guarantee
        || plan.required_units != expected_units
        || plan.dependencies.get(..result.dependencies.len())
            != Some(result.dependencies.as_slice())
        || expected_units.join("\n").len() > goal.budget.max_evidence_bytes
    {
        return Err(failure(
            "RECOVERY_MEMORY_PROOF_MISMATCH",
            "the prior relation-profile explanation differs from its original or current proof",
        ));
    }
    Ok(())
}

fn uses_snapshot_relation_index(state: &CaseRevision, proposition: &Proposition) -> bool {
    !proposition.condition_refs.is_empty()
        && state
            .bundle
            .as_ref()
            .is_some_and(|bundle| bundle.meaning.source_kind == "USER_REQUEST")
}

#[cfg(test)]
thread_local! {
    static RECOVERY_SNAPSHOT_QUERIES: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
}

fn query_recovery_snapshot(
    state: &CaseRevision,
    memory: MemoryState,
    proposition: &Proposition,
    max_actions: u32,
) -> Result<(MemoryState, crate::relations::QueryOutcome)> {
    if uses_snapshot_relation_index(state, proposition) {
        return Store::with_snapshot_relation_index(&state.actor_id, &memory, |index| {
            #[cfg(test)]
            RECOVERY_SNAPSHOT_QUERIES.with(|calls| calls.set(calls.get() + 1));
            crate::relations::query_active_relation(index, proposition, max_actions)
        });
    }
    let answer = crate::relations::query(&memory, proposition, max_actions)?;
    Ok((memory, answer))
}

struct RetainedRelationProof {
    snapshot_sha256: String,
    projection: (MemoryState, crate::relations::QueryOutcome),
}

fn verify_multi_condition_premise(
    store: &Store,
    bundle: &InputBundle,
    result: &ResultRecord,
    plan: &ContentPlan,
    proposition: &Proposition,
) -> Result<()> {
    let invalid = || {
        failure(
            "RECOVERY_MULTI_CONDITION_PREMISE_MISMATCH",
            "the retained premise differs from its original conditional registration",
        )
    };
    let source = identity(&bundle.request)?;
    let id = crate::relations::user_premise_id(proposition, &source)?;
    let actor = &bundle.request.actor_id;
    let revision = result.actor_memory_revision.ok_or_else(invalid)?;
    let memory = store
        .load_memory_revision(actor, revision)?
        .ok_or_else(invalid)?;
    let fact = memory
        .facts
        .iter()
        .find(|fact| fact.id == id)
        .ok_or_else(invalid)?;
    let evidence = crate::input::assertion_source_evidence(bundle)?.ok_or_else(invalid)?;
    let units = crate::core::premise_registration_units(fact);
    if &fact.proposition != proposition
        || fact.source_input != source
        || fact.source_evidence.as_ref() != Some(&evidence)
        || fact.guarantee != Guarantee::UserPremise
        || !fact.valid
        || !fact.dependencies.is_empty()
        || result.dependencies != [id]
        || result.guarantee != Guarantee::UserPremise
        || plan.guarantee != Guarantee::UserPremise
        || result.calculation.is_some()
        || !result.evidence.is_empty()
        || plan.required_units != units
        || result.text != units.join("\n")
        || plan.text != result.text
        || result.text.len() > bundle.request.budget.max_evidence_bytes
        || bundle.request.budget.max_actions < 4
    {
        return Err(invalid());
    }
    if !crate::core::record_source_memory_at_head(store, actor)?
        .facts
        .contains(fact)
    {
        return Err(failure(
            "RECOVERY_MULTI_CONDITION_PREMISE_CHANGED",
            "the current premise differs from the adopted registration",
        ));
    }
    Ok(())
}

fn verify_guarded_rule_registration(
    store: &Store,
    bundle: &InputBundle,
    result: &ResultRecord,
    plan: &ContentPlan,
    goal: &GoalContract,
) -> Result<()> {
    const MISMATCH: &str = "RECOVERY_GUARDED_RULE_MISMATCH";
    let invalid = || {
        failure(
            MISMATCH,
            "the retained guarded rule differs from its sealed registration",
        )
    };
    verify_registered_input(store, bundle, MISMATCH)?;
    if !crate::input::verified_guarded_relation_rule_source(bundle)? {
        return Err(invalid());
    }
    let Some((premises @ [premise, additional @ ..], conclusion)) =
        crate::core::rule_declaration(&goal.intent)
    else {
        return Err(invalid());
    };
    let source_input = identity(&bundle.request)?;
    let id = crate::relations::declared_rule_id(premises, conclusion, &source_input)?;
    let actor = &bundle.request.actor_id;
    let revision = result.actor_memory_revision.ok_or_else(invalid)?;
    let memory = store
        .load_memory_revision(actor, revision)?
        .ok_or_else(invalid)?;
    let rule = memory
        .rules
        .iter()
        .find(|rule| rule.id == id)
        .ok_or_else(invalid)?;
    let units = crate::core::rule_registration_units(rule);
    if &rule.premise != premise
        || &rule.conclusion != conclusion
        || rule.source_input != source_input
        || rule.additional_premises != additional
        || !rule.dependencies.is_empty()
        || !rule.valid
        || result.dependencies != [id]
        || result.guarantee != Guarantee::UserPremise
        || plan.guarantee != Guarantee::UserPremise
        || result.calculation.is_some()
        || !result.evidence.is_empty()
        || plan.required_units != units
        || result.text != units.join("\n")
        || plan.text != result.text
        || result.text.len() > goal.budget.max_evidence_bytes
        || goal.budget.max_actions < 3 + u32::from(goal.output_path.is_some())
    {
        return Err(invalid());
    }
    let current = crate::core::record_source_memory_at_head(store, actor)?;
    if !current.rules.contains(rule) {
        return Err(failure(
            "RECOVERY_GUARDED_RULE_CHANGED",
            "the current guarded rule differs from the adopted registration",
        ));
    }
    Ok(())
}

fn verify_record_source_result(
    store: &Store,
    state: &CaseRevision,
    bundle: &InputBundle,
    result: &ResultRecord,
    plan: &ContentPlan,
    goal: &GoalContract,
) -> Result<()> {
    if matches!(goal.intent, Intent::ListPreviousRules) {
        const MISMATCH: &str = "RECOVERY_PREVIOUS_RULE_LIST_MISMATCH";
        verify_registered_input(store, bundle, MISMATCH)?;
        crate::core::check_record_source_actions(goal)?;
        if result.actor_memory_revision.is_some() {
            return Err(failure(
                MISMATCH,
                "the historical rule list must not depend on current memory",
            ));
        }
        let readout = crate::core::previous_rule_list(store, bundle, &state.prior_results, goal)
            .map_err(|error| match error {
                Error::Contract { code, message } if code == "PREVIOUS_RULE_LIST_UNAVAILABLE" => {
                    failure(MISMATCH, message)
                }
                error => error,
            })?;
        return verify_record_source_readout(store, bundle, result, plan, readout, MISMATCH);
    }
    let (mismatch, changed) = match &goal.intent {
        Intent::InspectFactSource { .. } => (
            "RECOVERY_FACT_SOURCE_MISMATCH",
            "RECOVERY_FACT_SOURCE_CHANGED",
        ),
        Intent::InspectRuleSource { .. } | Intent::InspectDeclaredRuleSource { .. } => (
            "RECOVERY_RULE_SOURCE_MISMATCH",
            "RECOVERY_RULE_SOURCE_CHANGED",
        ),
        Intent::InspectPreviousRuleSource => (
            "RECOVERY_PREVIOUS_RULE_SOURCE_MISMATCH",
            "RECOVERY_RULE_SOURCE_CHANGED",
        ),
        Intent::InspectPreviousComposedRuleSource => (
            "RECOVERY_PREVIOUS_COMPOSED_RULE_SOURCE_MISMATCH",
            "RECOVERY_COMPOSED_RULE_SOURCE_CHANGED",
        ),
        Intent::InspectPreviousRuleSourceById { .. } => (
            "RECOVERY_PREVIOUS_RULE_SOURCE_BY_ID_MISMATCH",
            "RECOVERY_PREVIOUS_RULE_SOURCE_BY_ID_CHANGED",
        ),
        Intent::AskListedRuleSource { .. } => (
            "RECOVERY_LISTED_RULE_SOURCE_MISMATCH",
            "RECOVERY_LISTED_RULE_SOURCE_CHANGED",
        ),
        Intent::InspectComposedRuleSource { .. } => (
            "RECOVERY_COMPOSED_RULE_SOURCE_MISMATCH",
            "RECOVERY_COMPOSED_RULE_SOURCE_CHANGED",
        ),
        _ => (
            "RECOVERY_CONDITION_SOURCE_MISMATCH",
            "RECOVERY_CONDITION_SOURCE_CHANGED",
        ),
    };
    verify_registered_input(store, bundle, mismatch)?;
    crate::core::check_record_source_actions(goal)?;
    let context_mismatch = |error| match error {
        Error::Contract { code, message }
            if matches!(
                code.as_str(),
                "PREVIOUS_RULE_SOURCE_UNAVAILABLE"
                    | "PREVIOUS_RULE_SOURCE_AMBIGUOUS"
                    | "PREVIOUS_RULE_SOURCE_ID_NOT_USED"
                    | "PREVIOUS_COMPOSED_RULE_SOURCE_UNAVAILABLE"
                    | "PREVIOUS_COMPOSED_RULE_SOURCE_AMBIGUOUS"
                    | "LISTED_RULE_SOURCE_UNAVAILABLE"
                    | "LISTED_RULE_SOURCE_ID_NOT_LISTED"
            ) =>
        {
            failure(mismatch, &message)
        }
        error => error,
    };
    let target = crate::core::record_source_target(store, bundle, &state.prior_results)
        .map_err(context_mismatch)?;
    let actor = &bundle.request.actor_id;
    let revision = result.actor_memory_revision.ok_or_else(|| {
        failure(
            mismatch,
            "the source result has no retained actor-memory revision",
        )
    })?;
    let snapshot = store
        .load_memory_revision(actor, revision)?
        .ok_or_else(|| {
            failure(
                "RECOVERY_MEMORY_REVISION_MISSING",
                "the original source memory is unavailable",
            )
        })?;
    let record = crate::core::lookup_source_record(&snapshot, &target.lookup).map_err(|_| {
        failure(
            mismatch,
            "the sealed record is absent from its original actor-memory revision",
        )
    })?;
    let readout = target
        .readout(record, store, actor, revision, goal)
        .map_err(context_mismatch)?;
    verify_record_source_readout(store, bundle, result, plan, readout, mismatch)?;
    let head = crate::core::record_source_memory_at_head(store, actor)?;
    let changed = || {
        failure(
            changed,
            "the current record or its validity differs from the adopted source result",
        )
    };
    let current = match crate::core::lookup_source_record(&head, &target.lookup) {
        Err(Error::Contract { code, .. })
            if matches!(
                code.as_str(),
                "RULE_SOURCE_NOT_FOUND"
                    | "COMPOSED_RULE_SOURCE_NOT_FOUND"
                    | "COMPOSED_RULE_SOURCE_UNAVAILABLE"
            ) =>
        {
            return Err(changed());
        }
        record => record?,
    };
    if current != record {
        return Err(changed());
    }
    Ok(())
}

fn verify_record_source_readout(
    store: &Store,
    bundle: &InputBundle,
    result: &ResultRecord,
    plan: &ContentPlan,
    readout: crate::core::RecordSourceReadout,
    mismatch: &str,
) -> Result<()> {
    let request = &bundle.request;
    let proof_ref = digest(&readout.proof);
    if !readout.matches_surface(result, plan)
        || store.get_bytes(&request.actor_id, &request.case_id, &proof_ref)? != readout.proof
    {
        return Err(failure(
            mismatch,
            "the retained record source differs from its sealed proof and surface",
        ));
    }
    Ok(())
}

fn verify_condition_evaluation_result(
    store: &Store,
    bundle: &InputBundle,
    result: &ResultRecord,
    plan: &ContentPlan,
    goal: &GoalContract,
) -> Result<()> {
    const CODE: &str = "RECOVERY_CONDITION_EVALUATION_MISMATCH";
    verify_registered_input(store, bundle, CODE)?;
    let actor = &bundle.request.actor_id;
    let case = &bundle.request.case_id;
    let revision = result.actor_memory_revision.ok_or_else(|| {
        failure(
            CODE,
            "the condition evaluation has no retained actor-memory revision",
        )
    })?;
    let memory = match store.load_memory_revision(actor, revision)? {
        Some(memory) => memory,
        None if revision == 0 => MemoryState::default(),
        None => {
            return Err(failure(
                "RECOVERY_MEMORY_REVISION_MISSING",
                "the condition evaluation's actor-memory revision is no longer retained",
            ));
        }
    };
    let report = Store::with_snapshot_relation_index(actor, &memory, |index| {
        crate::core::evaluate_condition_report(index, goal)
    })?;
    // The admitted bytes belong to the retained revision, while the current
    // revision supplies only the evaluation and proof-dependency comparison.
    report.verify_evidence_budget(goal.budget.max_evidence_bytes)?;
    let current = crate::core::condition_report_at_head(store, actor, goal)?;
    if report.evaluation != current.evaluation || report.detail != current.detail {
        return Err(failure(
            "RECOVERY_CONDITION_EVALUATION_CHANGED",
            "the current condition definition or relevant fact proof differs from the adopted evaluation",
        ));
    }
    let proof = report.proof_bytes()?;
    let proof_ref = digest(&proof);
    let mut dependencies = report.evaluation.dependencies.clone();
    dependencies.push(proof_ref.clone());
    if report.revision != revision
        || result.dependencies != dependencies
        || result.guarantee != Guarantee::Computed
        || plan.guarantee != Guarantee::Computed
        || result.calculation.is_some()
        || !result.evidence.is_empty()
        || plan.required_units != report.units
        || result.text != report.units.join("\n")
        || plan.text != result.text
        || store.get_bytes(actor, case, &proof_ref)? != proof
    {
        return Err(failure(
            CODE,
            "the retained condition result differs from its sealed evaluation proof and exact surface",
        ));
    }
    Ok(())
}

fn verify_result_memory_provenance(
    store: &Store,
    actor: &str,
    case: &str,
    state: &CaseRevision,
    result: &ResultRecord,
    plan: &ContentPlan,
    goal: &GoalContract,
) -> Result<Option<RetainedRelationProof>> {
    let Some((memory, snapshot_sha256)) = result_memory_snapshot_with_digest(store, actor, result)?
    else {
        return Ok(None);
    };
    match &goal.intent {
        Intent::Ask { proposition } | Intent::AskWhy { proposition } => {
            let (memory, answer) =
                query_recovery_snapshot(state, memory, proposition, goal.budget.max_actions)?;
            if !matches!(answer.status.as_str(), "SUPPORTED" | "CONTRADICTED")
                || crate::relations::answer_dependencies(&answer) != result.dependencies
                || crate::relations::answer_guarantee(&answer) != result.guarantee
            {
                return Err(failure(
                    "RECOVERY_MEMORY_PROOF_MISMATCH",
                    "the retained result does not match the actor-memory revision it records",
                ));
            }
            if matches!(goal.intent, Intent::AskWhy { .. }) {
                return Ok(Some(RetainedRelationProof {
                    snapshot_sha256,
                    projection: (memory, answer),
                }));
            }
        }
        Intent::AskRelationObjects {
            subject,
            predicate,
            negative,
            scope,
        } => {
            let answer = crate::core::relation_objects_answer(
                &memory,
                subject,
                predicate,
                *negative,
                scope,
                goal.budget.max_actions,
                goal.budget.max_evidence_bytes,
            )?;
            if answer.dependencies != result.dependencies
                || answer.guarantee != result.guarantee
                || plan.required_units != vec![answer.text]
            {
                return Err(failure(
                    "RECOVERY_MEMORY_PROOF_MISMATCH",
                    "the retained object answer does not match its actor-memory revision",
                ));
            }
        }
        Intent::AskConjunction { .. } => {
            verify_conjunction_memory_result(&memory, result, plan, goal)?;
        }
        Intent::AskCommonRelations { .. } => {
            verify_common_relations_memory_result(store, actor, case, &memory, result, plan, goal)?;
        }
        Intent::CompareRelationInventories { .. } => {
            verify_relation_comparison_memory_result(
                store, actor, case, &memory, result, plan, goal,
            )?;
        }
        Intent::DraftRelationProfile { .. } => {
            verify_relation_profile_memory_result(&memory, result, plan, goal)?;
        }
        Intent::AskPreviousRelation { .. } => {
            verify_previous_relation_memory_result(state, &memory, result, plan, goal)?;
        }
        Intent::ExplainPrevious => {
            verify_explain_previous_relation_memory_result(
                store, state, &memory, result, plan, goal,
            )?;
        }
        Intent::InspectPremises { subject } => {
            let (dependencies, guarantee) =
                premise_memory_inventory(&memory, subject, goal.budget.max_actions)?;
            if dependencies != result.dependencies || guarantee != result.guarantee {
                return Err(failure(
                    "RECOVERY_MEMORY_PROOF_MISMATCH",
                    "the retained premise inventory does not match its actor-memory revision",
                ));
            }
        }
        _ => {}
    }
    Ok(None)
}

/// Confirm that a retained result can supply bodies to a source question or summary.
pub(crate) fn source_context_available(
    store: &Store,
    actor: &str,
    case: &str,
    source: &ResultRecord,
    history: Option<&VerifiedCaseHistory<'_>>,
) -> Result<bool> {
    if source.lifecycle == Lifecycle::Commit {
        return Ok(true);
    }
    let Some(stop) = crate::text_summary::partial_retrieval_stop(source) else {
        return Ok(false);
    };
    let verify = |history: &VerifiedCaseHistory<'_>| {
        Ok(verified_partial_retrieval(store, actor, case, source, &stop, Some(history))?.is_some())
    };
    match history {
        Some(history) => verify(history),
        None => store.with_verified_case_history(actor, case, verify),
    }
}

fn verified_partial_retrieval(
    store: &Store,
    actor: &str,
    case: &str,
    source: &ResultRecord,
    stop: &crate::text_summary::PartialRetrievalStop<'_>,
    history: Option<&VerifiedCaseHistory<'_>>,
) -> Result<Option<InputBundle>> {
    use crate::text_summary::{FirstHopStop, PartialRetrievalStop};

    let invalid = || {
        failure(
            "SOURCE_CONTEXT_INVALID",
            "The partial retrieval differs from its original input or retained packets",
        )
    };
    let (_, bundle) = match history {
        Some(history) => history.input_bundle(&source.input_id, source.input_version),
        None => store.get_input_bundle(actor, case, &source.input_id, source.input_version),
    }?
    .ok_or_else(invalid)?;
    let Intent::Retrieve { urls, .. } = &bundle.meaning.intent else {
        return Ok(None);
    };
    let observed = source.evidence.len();
    let url_count_matches = match stop {
        PartialRetrievalStop::AfterAllUrls => observed == urls.len(),
        PartialRetrievalStop::AtRedirect => observed <= urls.len(),
        PartialRetrievalStop::BeforeFirstHop(_) => observed < urls.len(),
    };
    if identity(&bundle.request)? != source.id
        || source.guarantee != Guarantee::Unknown
        || source.content_plan.is_some()
        || source.render.is_some()
        || !source.receipts.is_empty()
        || source.execution_graph_ref.is_some()
        || !url_count_matches
        || observed != source.dependencies.len()
    {
        return Err(invalid());
    }

    if let PartialRetrievalStop::BeforeFirstHop(first_hop) = stop {
        let original = match history {
            Some(history) => history.suspended_result_revision(source),
            None => store.load_suspended_result_revision(actor, case, source),
        }?
        .ok_or_else(invalid)?;
        let goal_ref = original.goal_sha256.as_deref().ok_or_else(invalid)?;
        if original.bundle.as_ref() != Some(&bundle)
            || original.input_fingerprint.as_ref() != Some(&source.id)
            || original.reason != source.reason
        {
            return Err(invalid());
        }
        let permissions = &bundle.request.permissions;
        let next_uri = &urls[observed];
        match first_hop {
            FirstHopStop::Wait(detail) => {
                if !first_hop_wait_matches(
                    store,
                    &original,
                    permissions,
                    next_uri,
                    detail,
                    history,
                )? {
                    return Err(invalid());
                }
            }
            FirstHopStop::Deadline | FirstHopStop::HttpStarts | FirstHopStop::EvidenceBytes => {
                // The port admits the first URL before checking the read limits.
                crate::ports::approved_url(next_uri, permissions)?;
            }
        }
        if matches!(first_hop, FirstHopStop::HttpStarts) {
            let goal: GoalContract =
                serde_json::from_slice(&store.get_bytes(actor, case, goal_ref)?)?;
            if goal.input_fingerprint != source.id
                || goal.intent != bundle.meaning.intent
                || goal.permissions != bundle.request.permissions
                || goal.output_path != bundle.request.output_path
                || goal.budget != bundle.request.budget
            {
                return Err(invalid());
            }
            let nodes: Vec<GoalNode> =
                serde_json::from_slice(&store.get_bytes(actor, case, &goal.required_graph_ref)?)?;
            let reserved = nodes.len().checked_sub(urls.len()).ok_or_else(invalid)?;
            let max_actions = goal.budget.max_actions as usize;
            // Each recorded hop used one start, including hops retained as failures.
            let used: usize = source
                .evidence
                .iter()
                .map(|packet| packet.redirect_chain.len())
                .sum();
            if max_actions < nodes.len() || used != max_actions - reserved {
                return Err(invalid());
            }
        }
    } else if match history {
        Some(history) => history.input_result(&source.input_id, source.input_version),
        None => store.get_input_result(actor, case, &source.input_id, source.input_version),
    }?
    .as_ref()
        != Some(source)
    {
        return Err(invalid());
    }

    let mut bytes_left = bundle.request.budget.max_evidence_bytes;
    for ((packet, dependency), uri) in source.evidence.iter().zip(&source.dependencies).zip(urls) {
        let encoded = canonical(packet)?;
        bytes_left = bytes_left
            .checked_sub(packet.raw_body.len())
            .ok_or_else(invalid)?;
        if packet.request_uri != *uri
            || &packet.id != dependency
            || store.get_bytes(actor, case, &digest(&encoded))? != encoded
        {
            return Err(invalid());
        }
        if packet.packet_kind == "FETCHED_BODY" {
            crate::ports::validate_successful_packet(packet, &bundle.request.permissions)?;
        } else {
            crate::ports::validate_observed_route(packet, &bundle.request.permissions)?;
            if packet.mode != "LIVE"
                || packet.provider_id != "https-direct-url-v1"
                || packet.parser_version != "strict-utf8-identity-v1"
                || packet.failure.is_none()
                || packet.extracted_text.is_some()
                || packet.independence_group != packet.final_uri
                || packet.raw_sha256 != digest(&packet.raw_body)
                || packet.id != crate::ports::fetch_packet_identity(packet)?
            {
                return Err(invalid());
            }
        }
    }
    match stop {
        PartialRetrievalStop::BeforeFirstHop(FirstHopStop::EvidenceBytes) if bytes_left != 0 => {
            Err(invalid())
        }
        PartialRetrievalStop::BeforeFirstHop(FirstHopStop::HttpStarts) if bytes_left == 0 => {
            Err(invalid())
        }
        _ => Ok(Some(bundle)),
    }
}

pub(crate) const PREVIOUS_DIRECT_RETRIEVAL_NODE: &str = "previous-direct-retrieval";

pub(crate) struct DirectContinuation<'a> {
    pub(crate) source: &'a ResultRecord,
    pub(crate) uri: String,
    pub(crate) observed_routes: BTreeSet<String>,
}

pub(crate) fn next_direct_retrieval<'a>(
    store: &Store,
    actor: &str,
    bundle: &InputBundle,
    previous: Option<&'a ResultRecord>,
    prior_results: &BTreeMap<String, ResultRecord>,
    history: Option<&VerifiedCaseHistory<'_>>,
) -> Result<Option<DirectContinuation<'a>>> {
    let Some(source) = previous else {
        return Ok(None);
    };
    let resolve = |history: &VerifiedCaseHistory<'_>| {
        next_direct_retrieval_in_history(store, actor, bundle, source, prior_results, history)
    };
    match history {
        Some(history) => resolve(history),
        None => store.with_verified_case_history(actor, &bundle.request.case_id, resolve),
    }
}

fn next_direct_retrieval_in_history<'a>(
    store: &Store,
    actor: &str,
    bundle: &InputBundle,
    source: &'a ResultRecord,
    prior_results: &BTreeMap<String, ResultRecord>,
    history: &VerifiedCaseHistory<'_>,
) -> Result<Option<DirectContinuation<'a>>> {
    let invalid = || {
        failure(
            "SOURCE_CONTEXT_INVALID",
            "The direct continuation differs from its sealed acquisition chain",
        )
    };
    if !binds_direct_predecessor(bundle, source)? {
        return Err(invalid());
    }
    let case = &bundle.request.case_id;
    let mut root = source;
    let mut visited = BTreeSet::new();
    let mut steps = Vec::new();
    let mut only_legacy_stops = true;
    loop {
        let stop = direct_preconnection_stop(root);
        if root.lifecycle != Lifecycle::Commit && stop.is_none() {
            break;
        }
        if !visited.insert(&root.id) {
            return Err(invalid());
        }
        let (_, sealed) = history
            .input_bundle(&root.input_id, root.input_version)?
            .ok_or_else(invalid)?;
        if !matches!(sealed.meaning.intent, Intent::FetchNextUnobservedSource) {
            return if steps.is_empty() {
                Ok(None)
            } else {
                Err(invalid())
            };
        }
        let without_legacy_lineage = || {
            if only_legacy_stops {
                Ok(None)
            } else {
                Err(invalid())
            }
        };
        let legacy_version = if matches!(stop, Some(DirectPreconnectionStop::LegacyNoPending)) {
            let Some(version) = LegacyNoPendingVersion::from_bundle(&sealed)
                .filter(|_| sealed.meaning.context_result_ref.is_some())
            else {
                return without_legacy_lineage();
            };
            Some(version)
        } else {
            None
        };
        let parent = sealed
            .meaning
            .context_result_ref
            .as_ref()
            .and_then(|id| prior_results.get(id))
            .ok_or_else(invalid)?;
        let legacy_versions = if let Some(version) = legacy_version {
            let Some(parent_version) = version.saved_parent_version(history, parent)? else {
                // These producers also wrote this reason for ordinary terminal histories.
                return without_legacy_lineage();
            };
            Some((version, parent_version))
        } else {
            None
        };
        if identity(&sealed.request)? != root.id || !binds_direct_predecessor(&sealed, parent)? {
            return Err(invalid());
        }
        if root.lifecycle == Lifecycle::Commit
            && history
                .input_result(&root.input_id, root.input_version)?
                .as_ref()
                != Some(root)
        {
            return Err(invalid());
        }
        only_legacy_stops &= legacy_versions.is_some();
        steps.push((parent, root, sealed, stop, legacy_versions));
        root = parent;
    }
    let Some(stop @ crate::text_summary::PartialRetrievalStop::BeforeFirstHop(_)) =
        crate::text_summary::partial_retrieval_stop(root)
    else {
        return if steps.is_empty() {
            Ok(None)
        } else {
            Err(invalid())
        };
    };
    let Some(original) =
        verified_partial_retrieval(store, actor, case, root, &stop, Some(history))?
    else {
        return if steps.is_empty() {
            Ok(None)
        } else {
            Err(invalid())
        };
    };
    let Intent::Retrieve { urls, .. } = original.meaning.intent else {
        return Err(invalid());
    };
    let mut urls = urls.into_iter().skip(root.evidence.len()).peekable();
    let mut observed_routes: BTreeSet<_> = root
        .evidence
        .iter()
        .flat_map(|packet| &packet.redirect_chain)
        .cloned()
        .collect();
    let mut legacy_prefix: Option<LegacyNoPendingPrefix> = None;
    for (parent, result, sealed, stop, legacy_versions) in steps.into_iter().rev() {
        let request = &sealed.request;
        if let Some(stop) = stop {
            let uri = urls.peek().map(String::as_str);
            if legacy_versions.is_none() {
                legacy_prefix = None;
                if uri.is_none_or(|uri| {
                    url::Url::parse(uri).is_ok_and(|url| observed_routes.contains(url.as_str()))
                }) {
                    return Err(invalid());
                }
            }
            if let Some((version, parent_version)) = legacy_versions {
                let prefix = legacy_prefix.ok_or_else(invalid)?;
                legacy_prefix = prefix.extended_by(version, parent_version);
                if legacy_prefix.is_none() {
                    return Ok(None);
                }
            }
            verify_direct_preconnection_stop(store, &sealed, result, parent, uri, stop, history)?;
            // This request observed no URL, so the pending slot has not moved.
            continue;
        }
        let uri = urls.next().ok_or_else(invalid)?;
        let packet =
            verify_direct_continuation_body(parent, result, request, &uri, &observed_routes)?;
        let parent_ref = verified_artifact(store, actor, case, parent)?;
        let packet_ref = verified_artifact(store, actor, case, packet)?;
        let graph_ref = result.execution_graph_ref.as_ref().ok_or_else(invalid)?;
        let graph: serde_json::Value =
            serde_json::from_slice(&store.get_bytes(actor, case, graph_ref)?)?;
        let nodes = graph[0].as_array().ok_or_else(invalid)?;
        let starts = (request.budget.max_actions as usize)
            .checked_sub(nodes.len().saturating_sub(1))
            .ok_or_else(invalid)?;
        if packet.redirect_chain.len() > starts
            || graph[1][PREVIOUS_DIRECT_RETRIEVAL_NODE].as_str() != Some(&parent_ref)
            || graph[1]["source-0"].as_str() != Some(&packet_ref)
            || graph[5]["source-0"]
                != serde_json::json!({ (PREVIOUS_DIRECT_RETRIEVAL_NODE): parent_ref })
            || graph[5]["compute"] != serde_json::json!({ "source-0": packet_ref })
        {
            return Err(invalid());
        }
        observed_routes.extend(packet.redirect_chain.iter().cloned());
        // B138 could commit one continuation directly from the original retrieval.
        legacy_prefix = (parent.id == root.id
            && sealed.meaning.parser_version
                == crate::input::FIRST_DIRECT_CONTINUATION_PARSER_VERSION)
            .then_some(LegacyNoPendingPrefix::FirstCommit);
    }
    let Some(uri) = urls.next() else {
        return Ok(None);
    };
    // Match saved normalized routes without requiring the original grant on
    // URLs the original acquisition never reached. The new port checks its grant.
    if url::Url::parse(&uri).is_ok_and(|uri| observed_routes.contains(uri.as_str())) {
        return Ok(None);
    }
    Ok(Some(DirectContinuation {
        source,
        uri,
        observed_routes,
    }))
}

fn binds_direct_predecessor(bundle: &InputBundle, previous: &ResultRecord) -> Result<bool> {
    Ok(
        bundle.meaning.context_result_ref.as_ref() == Some(&previous.id)
            && bundle.context_sha256 == identity(&Some(previous))?,
    )
}

#[derive(Clone, Copy)]
enum LegacyNoPendingVersion {
    V1,
    V2ToV4,
    V5,
    V6,
    V7,
    V8,
    V9,
}

impl LegacyNoPendingVersion {
    fn from_bundle(bundle: &InputBundle) -> Option<Self> {
        if bundle.meaning.intent != Intent::FetchNextUnobservedSource {
            return None;
        }
        match bundle.meaning.parser_version.as_str() {
            crate::input::FIRST_DIRECT_CONTINUATION_PARSER_VERSION => Some(Self::V1),
            "direct-source-continuation-v2"
            | "direct-source-continuation-v3"
            | "direct-source-continuation-v4" => Some(Self::V2ToV4),
            "direct-source-continuation-v5" => Some(Self::V5),
            "direct-source-continuation-v6" => Some(Self::V6),
            "direct-source-continuation-v7" => Some(Self::V7),
            "direct-source-continuation-v8" => Some(Self::V8),
            "direct-source-continuation-v9" => Some(Self::V9),
            _ => None,
        }
    }

    fn saved_parent_version(
        self,
        history: &VerifiedCaseHistory<'_>,
        parent: &ResultRecord,
    ) -> Result<Option<Self>> {
        let parent_is_stop = matches!(
            direct_preconnection_stop(parent),
            Some(DirectPreconnectionStop::LegacyNoPending)
        );
        if !(parent_is_stop || (matches!(self, Self::V1) && parent.lifecycle == Lifecycle::Commit))
        {
            return Ok(None);
        }
        Ok(history
            .input_bundle(&parent.input_id, parent.input_version)?
            .and_then(|(_, bundle)| Self::from_bundle(&bundle))
            .filter(|parent_version| {
                !matches!(self, Self::V1) || matches!(parent_version, Self::V1)
            }))
    }
}

#[derive(Clone, Copy)]
enum LegacyNoPendingPrefix {
    FirstCommit,
    OneV1Stop,
    MultipleV1Stops,
    WithV5Stop,
    WithV2ToV4Stop,
}

impl LegacyNoPendingPrefix {
    fn extended_by(
        self,
        version: LegacyNoPendingVersion,
        parent_version: LegacyNoPendingVersion,
    ) -> Option<Self> {
        use LegacyNoPendingVersion::{V1, V2ToV4, V5, V6, V7, V8};

        // Once a V2–V4 stop is verified, none of the later saved producers
        // could resolve the chain. Repeated stops could come from older builds.
        if matches!(self, Self::WithV2ToV4Stop) {
            return (!matches!(version, V1)).then_some(self);
        }
        if matches!(version, V2ToV4) && !matches!(self, Self::FirstCommit) {
            return Some(Self::WithV2ToV4Stop);
        }

        // Preserve the producer boundaries for chains without a V2–V4 stop.
        match (self, version, parent_version) {
            (Self::FirstCommit, V1, V1) => Some(Self::OneV1Stop),
            (Self::OneV1Stop | Self::MultipleV1Stops, V1, V1) => Some(Self::MultipleV1Stops),
            (Self::OneV1Stop | Self::MultipleV1Stops | Self::WithV5Stop, V5, V1 | V5) => {
                Some(Self::WithV5Stop)
            }
            (Self::MultipleV1Stops, V6, V1 | V6)
            | (Self::MultipleV1Stops, V7, V6 | V7)
            | (Self::WithV5Stop, V6, V5 | V6)
            | (Self::WithV5Stop, V7, V5 | V6 | V7)
            | (Self::WithV5Stop, V8, V5 | V6 | V7 | V8) => Some(self),
            _ => None,
        }
    }
}

enum DirectPreconnectionStop<'a> {
    Permission,
    Wait(&'a str),
    NoComputeBudget,
    InsufficientActions,
    NoEvidenceBytes,
    LegacyNoPending,
}

impl DirectPreconnectionStop<'_> {
    fn has_predecessor_artifact(&self) -> bool {
        !matches!(
            self,
            Self::NoComputeBudget | Self::InsufficientActions | Self::LegacyNoPending
        )
    }
}

fn direct_preconnection_stop(result: &ResultRecord) -> Option<DirectPreconnectionStop<'_>> {
    if result.lifecycle != Lifecycle::Suspend || !result.evidence.is_empty() {
        return None;
    }
    let (code, detail) = result.reason.as_deref()?.split_once(": ")?;
    match (code, detail) {
        (crate::core::NO_PENDING_DIRECT_URL, crate::core::NO_PENDING_DIRECT_URL_MESSAGE) => {
            return Some(DirectPreconnectionStop::LegacyNoPending);
        }
        ("RESOURCE_LIMIT", crate::core::NO_COMPUTE_BUDGET) => {
            return Some(DirectPreconnectionStop::NoComputeBudget);
        }
        ("ACTION_BUDGET_EXHAUSTED", detail)
            if detail.starts_with(crate::core::ACTION_BUDGET_RESIDUALS) =>
        {
            return Some(DirectPreconnectionStop::InsufficientActions);
        }
        ("EVIDENCE_UNAVAILABLE", _) => {}
        _ => return None,
    }
    match detail.split_once(": ")? {
        (
            crate::ports::READ_PERMISSION_REQUIRED,
            crate::ports::READ_PERMISSION_REQUIRED_MESSAGE,
        ) => Some(DirectPreconnectionStop::Permission),
        (crate::ports::RETRY_DEFERRED, detail) => Some(DirectPreconnectionStop::Wait(detail)),
        ("RESOURCE_LIMIT", crate::ports::NO_REMAINING_EVIDENCE_BYTES) => {
            Some(DirectPreconnectionStop::NoEvidenceBytes)
        }
        _ => None,
    }
}

fn verify_direct_preconnection_stop(
    store: &Store,
    bundle: &InputBundle,
    result: &ResultRecord,
    parent: &ResultRecord,
    uri: Option<&str>,
    stop: DirectPreconnectionStop<'_>,
    history: &VerifiedCaseHistory<'_>,
) -> Result<()> {
    let invalid = || {
        failure(
            "SOURCE_CONTEXT_INVALID",
            "The direct continuation was not a verified pre-connection stop",
        )
    };
    let anchored = stop.has_predecessor_artifact();
    let dependencies_match = if anchored {
        result.dependencies.as_slice() == [parent.id.as_str()]
    } else {
        result.dependencies.is_empty()
    };
    if !dependencies_match
        || result.guarantee != Guarantee::Unknown
        || result.content_plan.is_some()
        || result.calculation.is_some()
        || result.render.is_some()
        || !result.receipts.is_empty()
        || result.execution_graph_ref.is_some()
    {
        return Err(invalid());
    }
    let request = &bundle.request;
    let actor = &request.actor_id;
    let case = &request.case_id;
    let stopped = history
        .suspended_result_revision(result)?
        .ok_or_else(invalid)?;
    if stopped.bundle.as_ref() != Some(bundle)
        || stopped.input_fingerprint.as_ref() != Some(&result.id)
        || stopped.reason != result.reason
    {
        return Err(invalid());
    }
    if matches!(stop, DirectPreconnectionStop::LegacyNoPending) {
        // The old resolver suspended after sealing, before constructing a Goal.
        if stopped.goal_sha256.is_none()
            && result.text == crate::core::NO_PENDING_DIRECT_URL_MESSAGE
            && request.budget.max_actions > 0
            && request.budget.deadline_ms > 0
            && store.input_start_predecessor(&stopped)?.as_ref() == Some(parent)
        {
            return Ok(());
        }
        return Err(invalid());
    }
    let uri = uri.ok_or_else(invalid)?;
    let goal_ref = stopped.goal_sha256.as_deref().ok_or_else(invalid)?;
    let goal: GoalContract = serde_json::from_slice(&store.get_bytes(actor, case, goal_ref)?)?;
    if goal.input_fingerprint != result.id
        || goal.intent != Intent::FetchNextUnobservedSource
        || goal.permissions != request.permissions
        || goal.output_path != request.output_path
        || goal.budget != request.budget
    {
        return Err(invalid());
    }
    let nodes: Vec<GoalNode> =
        serde_json::from_slice(&store.get_bytes(actor, case, &goal.required_graph_ref)?)?;
    let includes_retrieval = !matches!(stop, DirectPreconnectionStop::NoComputeBudget);
    if nodes != direct_continuation_stop_nodes(includes_retrieval, goal.output_path.is_some()) {
        return Err(invalid());
    }
    let has_compute = goal.budget.max_actions > 0 && goal.budget.deadline_ms > 0;
    let sufficient_actions = (goal.budget.max_actions as usize) >= nodes.len();
    let matches_original_stop = match stop {
        DirectPreconnectionStop::NoComputeBudget => {
            // This guard leaves before the live latest-predecessor check.
            !has_compute && store.input_start_predecessor(&stopped)?.as_ref() == Some(parent)
        }
        DirectPreconnectionStop::InsufficientActions => {
            let residuals: Vec<_> = nodes.iter().map(|node| node.id.as_str()).collect();
            let reason = format!(
                "ACTION_BUDGET_EXHAUSTED: {} {residuals:?}",
                crate::core::ACTION_BUDGET_RESIDUALS
            );
            has_compute && !sufficient_actions && result.reason.as_deref() == Some(&reason)
        }
        DirectPreconnectionStop::NoEvidenceBytes => {
            has_compute
                && sufficient_actions
                && goal.budget.max_evidence_bytes == 0
                && crate::ports::approved_url(uri, &request.permissions).is_ok()
        }
        DirectPreconnectionStop::Permission => {
            has_compute
                && sufficient_actions
                && matches!(
                    crate::ports::approved_url(uri, &request.permissions),
                    Err(Error::Contract { code, message })
                        if code == crate::ports::READ_PERMISSION_REQUIRED
                            && message == crate::ports::READ_PERMISSION_REQUIRED_MESSAGE
                )
        }
        DirectPreconnectionStop::Wait(detail) => {
            // The packet proves the historical stop. The fresh budget
            // independently checks the current host wait.
            has_compute
                && sufficient_actions
                && first_hop_wait_matches(
                    store,
                    &stopped,
                    &request.permissions,
                    uri,
                    detail,
                    Some(history),
                )?
        }
        DirectPreconnectionStop::LegacyNoPending => unreachable!("checked above"),
    };
    if !matches_original_stop {
        return Err(invalid());
    }
    if anchored {
        verified_artifact(store, actor, case, parent)?;
    }
    Ok(())
}

fn direct_continuation_stop_nodes(includes_retrieval: bool, includes_file: bool) -> Vec<GoalNode> {
    let mut nodes = Vec::new();
    if includes_retrieval {
        nodes.extend([
            GoalNode {
                id: PREVIOUS_DIRECT_RETRIEVAL_NODE.into(),
                kind: GoalNodeKind::Evidence,
                requires: Vec::new(),
            },
            GoalNode {
                id: "source-0".into(),
                kind: GoalNodeKind::Evidence,
                requires: vec![PREVIOUS_DIRECT_RETRIEVAL_NODE.into()],
            },
        ]);
    }
    nodes.extend([
        GoalNode {
            id: "compute".into(),
            kind: GoalNodeKind::CoreResult,
            requires: if includes_retrieval {
                vec!["source-0".into()]
            } else {
                Vec::new()
            },
        },
        GoalNode {
            id: "content".into(),
            kind: GoalNodeKind::ContentPlan,
            requires: vec!["compute".into()],
        },
        GoalNode {
            id: "render".into(),
            kind: GoalNodeKind::RenderedArtifact,
            requires: vec!["content".into()],
        },
    ]);
    if includes_file {
        nodes.push(GoalNode {
            id: "file".into(),
            kind: GoalNodeKind::RequiredFile,
            requires: vec!["render".into()],
        });
    }
    nodes
}

fn verify_direct_continuation_body<'a>(
    source: &ResultRecord,
    result: &'a ResultRecord,
    request: &RequestEnvelope,
    uri: &str,
    observed_routes: &BTreeSet<String>,
) -> Result<&'a EvidencePacket> {
    let invalid = || {
        failure(
            "SOURCE_CONTEXT_INVALID",
            "The direct continuation packet or report differs from the pending URL",
        )
    };
    let [packet] = result.evidence.as_slice() else {
        return Err(invalid());
    };
    crate::ports::validate_successful_packet(packet, &request.permissions)?;
    if packet.request_uri != uri
        || packet.raw_body.len() > request.budget.max_evidence_bytes
        || packet
            .redirect_chain
            .iter()
            .any(|route| observed_routes.contains(route))
        || result.dependencies != [source.id.clone(), packet.id.clone()]
        || result.guarantee != Guarantee::SourceReport
        || result
            .content_plan
            .as_ref()
            .is_none_or(|plan| plan.required_units != [crate::core::direct_source_line(packet)])
    {
        return Err(invalid());
    }
    Ok(packet)
}

fn first_hop_wait_matches(
    store: &Store,
    original: &CaseRevision,
    permissions: &Permissions,
    next_uri: &str,
    detail: &str,
    history: Option<&VerifiedCaseHistory<'_>>,
) -> Result<bool> {
    let url = crate::ports::approved_url(next_uri, permissions)?;
    let host = url.host_str().expect("approved domain");
    let message = crate::ports::retry_deferred_message(host, None);
    let wait_on_host = |packet: &EvidencePacket| {
        packet.mode == "LIVE"
            && matches!(packet.packet_kind.as_str(), "FETCH_FAILURE" | "SEARCH_LIST")
            && matches!(packet.http_status, 429 | 503)
            && packet.failure.is_some()
            && packet.extracted_text.is_none()
            && packet.retry_after.is_some()
            && crate::ports::approved_url(&packet.final_uri, permissions)
                .is_ok_and(|uri| uri.host_str() == Some(host))
    };

    let Some(suffix) = detail.strip_prefix(&message) else {
        return Ok(false);
    };
    if suffix.is_empty() {
        // A wait observed in this input is recorded in its own packet prefix.
        // An indefinite wait may have been bounded by the original deadline.
        return Ok(original.result.as_ref().is_some_and(|result| {
            result
                .evidence
                .iter()
                .rev()
                .find(|packet| wait_on_host(packet))
                .is_some_and(|packet| {
                    packet.retry_after.as_ref().is_some_and(|retry| {
                        retry
                            .not_before_unix_ms
                            .is_none_or(|due| due > retry.received_at_unix_ms)
                    })
                })
        }));
    }
    let Some(packet_id) = suffix
        .strip_prefix(" (saved packet ")
        .and_then(|value| value.strip_suffix(')'))
    else {
        return Ok(false);
    };
    // Use the original stop's history. A later observation cannot supply its wait.
    for result in original.prior_results.values().filter(|result| {
        matches!(
            result.lifecycle,
            Lifecycle::Suspend | Lifecycle::Commit | Lifecycle::Cancel
        )
    }) {
        if let Some(packet) = result.evidence.iter().find(|packet| {
            packet.id == packet_id
                && wait_on_host(packet)
                && packet.retry_after.as_ref().is_some_and(|retry| {
                    retry
                        .not_before_unix_ms
                        .is_some_and(|due| due > retry.received_at_unix_ms)
                })
        }) {
            crate::core::verify_retained_retry_source(store, original, result, packet, history)?;
            return Ok(true);
        }
    }
    Ok(false)
}

/// Authenticate the original source of a committed follow-up or a no-match question.
pub(crate) fn source_followup_anchor<'a>(
    store: &Store,
    state: &'a CaseRevision,
    allow_explanation: bool,
) -> Result<Option<&'a ResultRecord>> {
    let (Some(bundle), Some(result)) = (&state.bundle, &state.result) else {
        return Ok(None);
    };
    let committed = state.lifecycle == Lifecycle::Commit && result.lifecycle == Lifecycle::Commit;
    let source_followup = committed
        && matches!(
            bundle.meaning.intent,
            Intent::AskPreviousSourceQuestion { .. }
                | Intent::SummarizePreviousSource
                | Intent::SummarizePreviousSourceConstrained { .. }
                | Intent::SummarizePreviousSourceLines { .. }
                | Intent::SummarizePreviousSourcePacketLines { .. }
        );
    let explanation = allow_explanation
        && committed
        && result.guarantee == Guarantee::Hypothesis
        && matches!(bundle.meaning.intent, Intent::ExplainPrevious);
    let no_match_query = match &bundle.meaning.intent {
        Intent::AskPreviousSourceQuestion { query }
            if state.lifecycle == Lifecycle::Suspend
                && result.lifecycle == Lifecycle::Suspend
                && bundle.request.budget.max_actions > 0
                && bundle.request.budget.deadline_ms > 0
                && result
                    .reason
                    .as_deref()
                    .is_some_and(|reason| reason.starts_with("SOURCE_QUESTION_NO_MATCH: ")) =>
        {
            Some(query.as_str())
        }
        _ => None,
    };
    if !source_followup && !explanation && no_match_query.is_none() {
        return Ok(None);
    }

    let actor = &state.actor_id;
    let case = &state.case_id;
    let request = &bundle.request;
    let mismatch = || {
        failure(
            "SOURCE_CONTEXT_INVALID",
            "The retained source follow-up does not match its sealed result",
        )
    };
    if identity(request)? != result.id
        || state.input_fingerprint.as_ref() != Some(&result.id)
        || !store
            .get_input_bundle(actor, case, &result.input_id, result.input_version)?
            .is_some_and(|(_, registered)| &registered == bundle)
    {
        return Err(mismatch());
    }
    let source_id = bundle
        .meaning
        .context_result_ref
        .as_ref()
        .ok_or_else(mismatch)?;
    let source = state.prior_results.get(source_id).ok_or_else(mismatch)?;
    if let Some(query) = no_match_query {
        if state.goal_sha256.is_some()
            || state.reason != result.reason
            || result.guarantee != Guarantee::Unknown
            || result.content_plan.is_some()
            || result.render.is_some()
            || !result.receipts.is_empty()
            || !result.evidence.is_empty()
            || !result.dependencies.is_empty()
            || result.execution_graph_ref.is_some()
        {
            return Err(mismatch());
        }
        let context = crate::text_summary::source_followup_context(
            store,
            bundle,
            &state.prior_results,
            source,
        )?
        .ok_or_else(mismatch)?;
        let packets =
            crate::text_summary::verified_source_question_packets(store, actor, case, context)?;
        let same_stop = |error: Option<Error>| {
            let Some(error) = error else { return false };
            matches!(&error, Error::Contract { code, .. } if code == "SOURCE_QUESTION_NO_MATCH")
                && result.reason.as_deref()
                    == Some(format!("SOURCE_QUESTION_NO_MATCH: {error}").as_str())
        };
        // Stopped questions predate operation-tagged answer artifacts. Require
        // the full recorded diagnostic from the matching lexical version.
        if !same_stop(
            crate::text_summary::answer_fetched_source_question_v2(
                &context.source.id,
                query,
                &packets,
            )
            .err(),
        ) && !same_stop(
            crate::text_summary::answer_fetched_source_question_v1(
                &context.source.id,
                query,
                &packets,
            )
            .err(),
        ) {
            return Err(mismatch());
        }
        return Ok(Some(if allow_explanation {
            context.base_context
        } else {
            source
        }));
    }
    let goal_ref = state.goal_sha256.as_ref().ok_or_else(mismatch)?;
    let goal: GoalContract = serde_json::from_slice(&store.get_bytes(actor, case, goal_ref)?)?;
    if goal.input_fingerprint != result.id
        || goal.intent != bundle.meaning.intent
        || goal.permissions != request.permissions
        || goal.output_path != request.output_path
        || goal.budget != request.budget
    {
        return Err(mismatch());
    }
    let plan = result.content_plan.as_ref().ok_or_else(mismatch)?;
    let mut unsigned_plan = plan.clone();
    unsigned_plan.id.clear();
    if plan.id != identity(&unsigned_plan)?
        || plan.text != result.text
        || plan.locale != request.locale
        || plan.guarantee != result.guarantee
    {
        return Err(mismatch());
    }

    if explanation {
        let Some(proof) = crate::text_summary::previous_summary_explanation(
            store,
            bundle,
            &state.prior_results,
            source,
        )?
        else {
            return Ok(None);
        };
        let Some(anchor_id) = proof.source_context_ref.as_ref() else {
            return Ok(None);
        };
        crate::text_summary::verify_source_explanation_result(store, bundle, result, &proof, None)?;
        // Keep the already verified source scope as the next implicit context.
        let anchor = state.prior_results.get(anchor_id).ok_or_else(mismatch)?;
        return Ok(Some(anchor));
    }

    let anchor_id = if matches!(goal.intent, Intent::AskPreviousSourceQuestion { .. }) {
        verify_source_question_context(store, actor, case, state, result, plan, &goal)?
            .map(|proof| proof.source_context_ref)
            .ok_or_else(mismatch)?
    } else {
        verify_source_summary_context(store, actor, case, state, result, plan, &goal)?
            .map(|proof| proof.source_context_ref)
            .ok_or_else(mismatch)?
    };
    let anchor = if allow_explanation {
        state.prior_results.get(&anchor_id).ok_or_else(mismatch)?
    } else {
        source
    };
    Ok(Some(anchor))
}

fn verify_source_summary_context(
    store: &Store,
    actor: &str,
    case: &str,
    state: &CaseRevision,
    result: &ResultRecord,
    plan: &ContentPlan,
    goal: &GoalContract,
) -> Result<Option<crate::text_summary::RetainedSourceSummary>> {
    let sealed_line = state.bundle.as_ref().is_some_and(|bundle| {
        matches!(
            bundle.meaning.parser_version.as_str(),
            crate::input::SOURCE_LINE_SUMMARY_PARSER_VERSION
                | crate::input::SOURCE_PACKET_LINE_SUMMARY_PARSER_VERSION
                | crate::input::SCOPED_SOURCE_PACKET_LINE_SUMMARY_PARSER_VERSION
                | crate::input::EXPLICIT_EXPLANATION_PACKET_LINE_SUMMARY_PARSER_VERSION
                | crate::input::EXPLICIT_EXPLANATION_LINE_SUMMARY_PARSER_VERSION
                | crate::input::IMPLICIT_EXPLANATION_LINE_SUMMARY_PARSER_VERSION
                | crate::input::IMPLICIT_SOURCE_SUMMARY_LINE_PARSER_VERSION
                | crate::input::EXPLICIT_SOURCE_SUMMARY_LINE_PARSER_VERSION
                | crate::input::EXPLICIT_SOURCE_SUMMARY_PACKET_LINE_PARSER_VERSION
                | crate::input::IMPLICIT_SOURCE_SUMMARY_PACKET_LINE_PARSER_VERSION
        ) || crate::input::source_line_summary_parser(&bundle.meaning.intent).is_some()
    });
    let goal_line = crate::input::source_line_summary_parser(&goal.intent).is_some();
    if (sealed_line || goal_line)
        && state
            .bundle
            .as_ref()
            .is_none_or(|bundle| !goal_line || bundle.meaning.intent != goal.intent)
    {
        return Err(failure(
            "RECOVERY_SOURCE_SUMMARY_MISMATCH",
            "the retained Goal differs from the sealed source line summary requirement",
        ));
    }
    if !matches!(
        goal.intent,
        Intent::SummarizePreviousSource
            | Intent::SummarizePreviousSourceConstrained { .. }
            | Intent::SummarizePreviousSourceLines { .. }
            | Intent::SummarizePreviousSourcePacketLines { .. }
    ) {
        return Ok(None);
    }
    let bundle = state
        .bundle
        .as_ref()
        .filter(|bundle| bundle.request.actor_id == actor && bundle.request.case_id == case)
        .ok_or_else(|| failure("RECOVERY_DATA_UNAVAILABLE", "sealed original unavailable"))?;
    if matches!(
        goal.intent,
        Intent::SummarizePreviousSourcePacketLines { .. }
    ) || crate::input::is_explanation_line_summary_version(&bundle.meaning.parser_version)
        || crate::input::uses_summary_source_context(&bundle.meaning.parser_version)
    {
        verify_registered_input(store, bundle, "RECOVERY_SOURCE_SUMMARY_MISMATCH")?;
    }
    if crate::input::uses_implicit_source_context(&bundle.meaning.parser_version) {
        let context = bundle
            .meaning
            .context_result_ref
            .as_ref()
            .and_then(|id| state.prior_results.get(id));
        if context.is_none() || store.input_start_predecessor(state)?.as_ref() != context {
            return Err(failure(
                "RECOVERY_SOURCE_CONTEXT_MISMATCH",
                "the sealed source context was not the preceding result when the input was received",
            ));
        }
    }
    crate::text_summary::verify_retained_source_summary(
        store,
        bundle,
        &state.prior_results,
        result,
        plan,
        None,
    )
}

const TEXT_SUMMARY_CANDIDATE_MISMATCH: &str = "RECOVERY_TEXT_SUMMARY_CANDIDATE_MISMATCH";

fn verify_registered_input(store: &Store, bundle: &InputBundle, code: &str) -> Result<()> {
    let request = &bundle.request;
    let registered = store.get_input_bundle(
        &request.actor_id,
        &request.case_id,
        &request.input_id,
        request.input_version,
    )?;
    if registered.as_ref().map(|(_, retained)| retained) != Some(bundle) {
        return Err(failure(
            code,
            "the interrupted request does not match its registered sealed input",
        ));
    }
    Ok(())
}

fn verify_text_summary_candidate(
    store: &Store,
    bundle: &InputBundle,
    result: &ResultRecord,
    plan: &ContentPlan,
) -> Result<String> {
    let request = &bundle.request;
    let meaning = &bundle.meaning;
    if meaning.source_kind != "USER_REQUEST"
        || !meaning.unresolved.is_empty()
        || request.text.len() > request.budget.max_input_bytes
        || plan.locale != request.locale
        || !matches!(meaning.spans.as_slice(), [span] if span.start == 0
            && span.end == request.text.len() && span.text == request.text)
        || result.id != identity(request)?
        || result.input_id != request.input_id
        || result.input_version != request.input_version
    {
        return Err(failure(
            TEXT_SUMMARY_CANDIDATE_MISMATCH,
            "the retained summary input version, raw document or result identity changed",
        ));
    }
    let report = crate::text_summary::direct_summary_candidates(bundle)?;
    let bytes = canonical(&report)?;
    let reference = digest(&bytes);
    if bytes.len() > request.budget.max_evidence_bytes
        || result.guarantee != Guarantee::Hypothesis
        || result.calculation.is_some()
        || !result.evidence.is_empty()
        || result.dependencies.as_slice() != std::slice::from_ref(&reference)
        || result.text != report.text
        || plan.required_units.as_slice() != std::slice::from_ref(&report.text)
        || store
            .get_bytes_if_present(&request.actor_id, &request.case_id, &reference)?
            .as_deref()
            != Some(bytes.as_slice())
    {
        return Err(failure(
            TEXT_SUMMARY_CANDIDATE_MISMATCH,
            "the canonical summary report or adopted candidate does not match the sealed input",
        ));
    }
    Ok(reference)
}

fn verify_legacy_text_summary_explanation(
    store: &Store,
    state: &CaseRevision,
    bundle: &InputBundle,
    result: &ResultRecord,
    plan: &ContentPlan,
) -> Result<bool> {
    if !matches!(bundle.meaning.intent, Intent::ExplainPrevious)
        || !crate::input::is_legacy_text_summary_explanation_version(&bundle.meaning.parser_version)
        || result.guarantee != Guarantee::Hypothesis
    {
        return Ok(false);
    }
    let Some(previous) = bundle
        .meaning
        .context_result_ref
        .as_ref()
        .and_then(|id| state.prior_results.get(id))
    else {
        return Ok(false);
    };
    let request = &bundle.request;
    let actor = &request.actor_id;
    let case = &request.case_id;
    let Some((_, prior_bundle)) =
        store.get_input_bundle(actor, case, &previous.input_id, previous.input_version)?
    else {
        return Ok(false);
    };
    if !matches!(
        prior_bundle.meaning.intent,
        Intent::TextSummaryCandidates { .. }
    ) {
        return Ok(false);
    }
    verify_registered_input(store, bundle, TEXT_SUMMARY_CANDIDATE_MISMATCH)?;
    let Some(prior_plan) = &previous.content_plan else {
        return Err(failure(
            TEXT_SUMMARY_CANDIDATE_MISMATCH,
            "no retained candidate plan",
        ));
    };
    if previous.lifecycle != Lifecycle::Commit
        || bundle.meaning.context_result_ref.as_ref() != Some(&previous.id)
        || bundle.context_sha256 != identity(&Some(previous))?
        || result.dependencies.as_slice() != std::slice::from_ref(&previous.id)
        || result.calculation.is_some()
        || !result.evidence.is_empty()
        || result.text != previous.text
        || plan.required_units.as_slice() != std::slice::from_ref(&previous.text)
    {
        return Err(failure(
            TEXT_SUMMARY_CANDIDATE_MISMATCH,
            "the legacy explanation does not match the sealed prior candidate",
        ));
    }
    let report_ref = verify_text_summary_candidate(store, &prior_bundle, previous, prior_plan)?;
    crate::document::revalidate_previous_document_render(
        store,
        actor,
        case,
        previous,
        Some(&report_ref),
        TEXT_SUMMARY_CANDIDATE_MISMATCH,
        "extractive summary candidates",
    )?;
    let compute_ref = verified_artifact(
        store,
        actor,
        case,
        &(
            &previous.calculation,
            &prior_plan.required_units,
            &previous.evidence,
            &previous.dependencies,
        ),
    )?;
    if prior_plan.dependencies != [report_ref, compute_ref] {
        return Err(failure(
            TEXT_SUMMARY_CANDIDATE_MISMATCH,
            "the legacy source plan is missing its exact summary report or computation",
        ));
    }
    Ok(true)
}

fn verify_previous_result_summary_context(
    store: &Store,
    actor: &str,
    case: &str,
    state: &CaseRevision,
    result: &ResultRecord,
    plan: &ContentPlan,
    goal: &GoalContract,
) -> Result<Option<crate::text_summary::RetainedPreviousResultSummary>> {
    let sealed_line = state.bundle.as_ref().is_some_and(|bundle| {
        bundle.meaning.parser_version == crate::input::PREVIOUS_RESULT_LINE_SUMMARY_PARSER_VERSION
            || matches!(
                bundle.meaning.intent,
                Intent::SummarizePreviousResultLines { .. }
            )
    });
    let goal_line = matches!(goal.intent, Intent::SummarizePreviousResultLines { .. });
    if (sealed_line || goal_line)
        && state
            .bundle
            .as_ref()
            .is_none_or(|bundle| !goal_line || bundle.meaning.intent != goal.intent)
    {
        return Err(failure(
            "RECOVERY_RESULT_SUMMARY_MISMATCH",
            "the retained Goal differs from the sealed line summary requirement",
        ));
    }
    if crate::text_summary::PreviousResultSummaryLimits::from_intent(&goal.intent).is_none() {
        return Ok(None);
    }
    let bundle = state
        .bundle
        .as_ref()
        .filter(|bundle| bundle.request.actor_id == actor && bundle.request.case_id == case)
        .ok_or_else(|| failure("RECOVERY_DATA_UNAVAILABLE", "sealed original unavailable"))?;
    crate::text_summary::verify_retained_previous_result_summary(
        store,
        bundle,
        &state.prior_results,
        result,
        plan,
    )
}

fn verify_source_question_context(
    store: &Store,
    actor: &str,
    case: &str,
    state: &CaseRevision,
    result: &ResultRecord,
    plan: &ContentPlan,
    goal: &GoalContract,
) -> Result<Option<crate::text_summary::RetainedSourceQuestion>> {
    if !matches!(goal.intent, Intent::AskPreviousSourceQuestion { .. }) {
        return Ok(None);
    }
    let bundle = state
        .bundle
        .as_ref()
        .filter(|bundle| bundle.request.actor_id == actor && bundle.request.case_id == case)
        .ok_or_else(|| failure("RECOVERY_DATA_UNAVAILABLE", "sealed original unavailable"))?;
    crate::text_summary::verify_retained_source_question(
        store,
        bundle,
        &state.prior_results,
        result,
        plan,
        None,
    )
}

fn verify_previous_text_edit_context(
    store: &Store,
    actor: &str,
    case: &str,
    state: &CaseRevision,
    result: &ResultRecord,
    plan: &ContentPlan,
    goal: &GoalContract,
) -> Result<Option<PreviousTextEditExpectation>> {
    let Intent::ReplacePreviousText { old, new } = &goal.intent else {
        return Ok(None);
    };
    let bundle = state
        .bundle
        .as_ref()
        .ok_or_else(|| failure("RECOVERY_DATA_UNAVAILABLE", "sealed original unavailable"))?;
    let prior_id = bundle.meaning.context_result_ref.as_ref().ok_or_else(|| {
        failure(
            "RECOVERY_PREVIOUS_TEXT_MISMATCH",
            "the previous-output edit has no sealed prior-result reference",
        )
    })?;
    let prior = state.prior_results.get(prior_id).ok_or_else(|| {
        failure(
            "RECOVERY_PREVIOUS_TEXT_MISMATCH",
            "the exact prior output is not retained in the case history",
        )
    })?;
    if prior.id != *prior_id || prior.lifecycle != Lifecycle::Commit {
        return Err(failure(
            "RECOVERY_PREVIOUS_TEXT_MISMATCH",
            "the referenced previous output is not the same committed result",
        ));
    }
    let (source_content_ref, source_render_ref, artifact) =
        crate::document::previous_text_replacement(prior, old, new)?;
    let source_content = prior.content_plan.as_ref().ok_or_else(|| {
        failure(
            "RECOVERY_PREVIOUS_TEXT_MISMATCH",
            "the prior content plan is not retained",
        )
    })?;
    let source_render = prior.render.as_ref().ok_or_else(|| {
        failure(
            "RECOVERY_PREVIOUS_TEXT_MISMATCH",
            "the prior actual rendering is not retained",
        )
    })?;
    let verified_content_ref = verified_artifact(store, actor, case, source_content)?;
    let verified_render_ref = verified_artifact(store, actor, case, source_render)?;
    let edit_bytes = canonical(&artifact)?;
    let edit_ref = digest(&edit_bytes);
    let stored_edit = store.get_bytes(actor, case, &edit_ref)?;
    let expected_dependencies = vec![
        prior.id.clone(),
        source_content_ref.clone(),
        source_render_ref.clone(),
        edit_ref.clone(),
    ];
    let output_units = crate::document::previous_text_replacement_lines(&artifact);
    if verified_content_ref != source_content_ref
        || verified_render_ref != source_render_ref
        || stored_edit != edit_bytes
        || result.dependencies != expected_dependencies
        || !result.evidence.is_empty()
        || result.calculation.is_some()
        || result.guarantee != Guarantee::UserPremise
        || plan.required_units != output_units
        || edit_bytes.len() > goal.budget.max_evidence_bytes
        || output_units.join("\n").len() > goal.budget.max_evidence_bytes
    {
        return Err(failure(
            "RECOVERY_PREVIOUS_TEXT_MISMATCH",
            "the previous result, rendered bytes, edit report, dependencies or adopted output differ from the exact request",
        ));
    }
    Ok(Some(PreviousTextEditExpectation {
        source_content_ref,
        source_render_ref,
        edit_ref,
    }))
}

fn inspect(
    store: &Store,
    actor: &str,
    state: &CaseRevision,
    result: &ResultRecord,
) -> Result<Inspected> {
    let case = &state.case_id;
    let bundle = state
        .bundle
        .as_ref()
        .ok_or_else(|| failure("RECOVERY_DATA_UNAVAILABLE", "sealed original unavailable"))?;
    let request = &bundle.request;
    let fingerprint = identity(request)?;
    if request.actor_id != actor
        || request.case_id != *case
        || state.input_fingerprint.as_deref() != Some(fingerprint.as_str())
    {
        return Err(failure(
            "RECOVERY_IDENTITY_MISMATCH",
            "sealed actor/case/input differs",
        ));
    }
    if matches!(
        bundle.meaning.parser_version.as_str(),
        crate::input::EXPLAINED_SEARCH_BODY_FOLLOWUP_PARSER_VERSION
            | crate::input::EXPLICIT_SOURCE_EXPLANATION_FOLLOWUP_PARSER_VERSION
    ) {
        verify_registered_input(store, bundle, "RECOVERY_SOURCE_CONTEXT_MISMATCH")?;
    }
    let goal_ref = state.goal_sha256.as_ref().ok_or_else(|| {
        failure(
            "RECOVERY_DATA_UNAVAILABLE",
            "bound original goal unavailable",
        )
    })?;
    let goal: GoalContract = serde_json::from_slice(&store.get_bytes(actor, case, goal_ref)?)?;
    if goal.input_fingerprint != fingerprint
        || goal.intent != bundle.meaning.intent
        || goal.permissions != request.permissions
        || goal.output_path != request.output_path
        || goal.budget != request.budget
    {
        return Err(failure(
            "RECOVERY_GOAL_MISMATCH",
            "saved goal differs from sealed original request",
        ));
    }
    let path = goal.output_path.as_ref().ok_or_else(|| {
        failure(
            "NO_LOCAL_FILE_GOAL",
            "original goal does not require a local file",
        )
    })?;
    if !goal.permissions.write_paths.contains(path) {
        return Err(failure(
            "WRITE_PERMISSION_MISSING",
            "original target lacks original matching authority",
        ));
    }
    let legacy_predicates = ["SUPPORTED_CONTENT", "EXACT_RENDER", "CONFIRMED_FILE"];
    let requires_generated_comparison = crate::input::uses_generated_comparison(&bundle.meaning);
    let requires_generated_calculation =
        bundle.meaning.parser_version == crate::input::CALCULATION_PARSER_VERSION;
    let requires_verified_correction =
        bundle.meaning.parser_version == crate::input::CALCULATION_CORRECTION_PARSER_VERSION;
    let condition_contract = condition_evaluation_contract(&goal.intent);
    let sealed_condition_parser = matches!(
        bundle.meaning.parser_version.as_str(),
        crate::input::CONDITION_EVALUATION_PARSER_VERSION
            | crate::input::CONDITION_EVALUATION_DETAIL_PARSER_VERSION
    )
    .then_some(bundle.meaning.parser_version.as_str());
    if condition_contract.map(|(parser, _)| parser) != sealed_condition_parser {
        return Err(failure(
            "RECOVERY_CONDITION_EVALUATION_MISMATCH",
            "the condition evaluation intent differs from the sealed parser contract",
        ));
    }
    let requires_condition_evaluation = condition_contract.is_some();
    let source_contract = crate::core::record_source_contract(&goal.intent);
    let sealed_source_parser = matches!(
        bundle.meaning.parser_version.as_str(),
        crate::input::FACT_SOURCE_PARSER_VERSION
            | crate::input::CONDITION_SOURCE_PARSER_VERSION
            | crate::input::CONDITION_SOURCE_BY_DEFINITION_PARSER_VERSION
            | crate::input::RULE_SOURCE_PARSER_VERSION
            | crate::input::DECLARED_RULE_SOURCE_PARSER_VERSION
            | crate::input::PREVIOUS_RULE_SOURCE_PARSER_VERSION
            | crate::input::PREVIOUS_COMPOSED_RULE_SOURCE_PARSER_VERSION
            | crate::input::PREVIOUS_RULE_SOURCE_BY_ID_PARSER_VERSION
            | crate::input::PREVIOUS_RULE_LIST_PARSER_VERSION
            | crate::input::LISTED_RULE_SOURCE_PARSER_VERSION
            | crate::input::COMPOSED_RULE_SOURCE_PARSER_VERSION
    )
    .then_some(bundle.meaning.parser_version.as_str());
    if source_contract.map(|(parser, _)| parser) != sealed_source_parser {
        let code = if sealed_source_parser == Some(crate::input::LISTED_RULE_SOURCE_PARSER_VERSION)
            || matches!(goal.intent, Intent::AskListedRuleSource { .. })
        {
            "RECOVERY_LISTED_RULE_SOURCE_MISMATCH"
        } else if sealed_source_parser == Some(crate::input::PREVIOUS_RULE_LIST_PARSER_VERSION)
            || matches!(goal.intent, Intent::ListPreviousRules)
        {
            "RECOVERY_PREVIOUS_RULE_LIST_MISMATCH"
        } else if sealed_source_parser
            == Some(crate::input::PREVIOUS_RULE_SOURCE_BY_ID_PARSER_VERSION)
            || matches!(goal.intent, Intent::InspectPreviousRuleSourceById { .. })
        {
            "RECOVERY_PREVIOUS_RULE_SOURCE_BY_ID_MISMATCH"
        } else if sealed_source_parser
            == Some(crate::input::PREVIOUS_COMPOSED_RULE_SOURCE_PARSER_VERSION)
            || matches!(goal.intent, Intent::InspectPreviousComposedRuleSource)
        {
            "RECOVERY_PREVIOUS_COMPOSED_RULE_SOURCE_MISMATCH"
        } else if sealed_source_parser == Some(crate::input::PREVIOUS_RULE_SOURCE_PARSER_VERSION)
            || matches!(goal.intent, Intent::InspectPreviousRuleSource)
        {
            "RECOVERY_PREVIOUS_RULE_SOURCE_MISMATCH"
        } else if sealed_source_parser == Some(crate::input::COMPOSED_RULE_SOURCE_PARSER_VERSION)
            || matches!(goal.intent, Intent::InspectComposedRuleSource { .. })
        {
            "RECOVERY_COMPOSED_RULE_SOURCE_MISMATCH"
        } else if matches!(
            sealed_source_parser,
            Some(
                crate::input::RULE_SOURCE_PARSER_VERSION
                    | crate::input::DECLARED_RULE_SOURCE_PARSER_VERSION
            )
        ) || matches!(
            goal.intent,
            Intent::InspectRuleSource { .. } | Intent::InspectDeclaredRuleSource { .. }
        ) {
            "RECOVERY_RULE_SOURCE_MISMATCH"
        } else if sealed_source_parser == Some(crate::input::FACT_SOURCE_PARSER_VERSION)
            || matches!(goal.intent, Intent::InspectFactSource { .. })
        {
            "RECOVERY_FACT_SOURCE_MISMATCH"
        } else {
            "RECOVERY_CONDITION_SOURCE_MISMATCH"
        };
        return Err(failure(
            code,
            "the record-source intent differs from its sealed parser contract",
        ));
    }
    let requires_record_source = source_contract.is_some();
    let requires_multi_condition_relation =
        bundle.meaning.parser_version == crate::input::MULTI_CONDITION_RELATION_PARSER_VERSION;
    if requires_multi_condition_relation {
        const CODE: &str = "RECOVERY_MULTI_CONDITION_RELATION_MISMATCH";
        verify_registered_input(store, bundle, CODE)?;
        if !crate::input::verified_multi_condition_relation_source(bundle)?
            || result.actor_memory_revision.is_none()
        {
            return Err(failure(
                CODE,
                "the original multiple-condition input or actor-memory revision is unavailable",
            ));
        }
    }
    let requires_guarded_rule = matches!(
        bundle.meaning.parser_version.as_str(),
        crate::input::GUARDED_RELATION_RULE_PARSER_VERSION
            | crate::input::GUARDED_CONJUNCTIVE_RULE_PARSER_VERSION
            | crate::input::MULTI_CONDITION_RULE_PARSER_VERSION
    );
    let requires_sequence_edit = matches!(
        bundle.meaning.parser_version.as_str(),
        crate::input::SEQUENCE_CORRECTION_PARSER_VERSION
            | crate::input::SEQUENCE_REWIRE_PARSER_VERSION
            | crate::input::SEQUENCE_APPEND_PARSER_VERSION
            | crate::input::SEQUENCE_REMOVE_PARSER_VERSION
            | crate::input::SEQUENCE_MOVE_PARSER_VERSION
            | crate::input::SEQUENCE_INSERT_PARSER_VERSION
    );
    if crate::input::sequence_edit_parser(&goal.intent)
        != requires_sequence_edit.then_some(bundle.meaning.parser_version.as_str())
    {
        return Err(failure(
            "RECOVERY_SEQUENCE_CORRECTION_MISMATCH",
            "the sequence edit intent differs from the sealed parser contract",
        ));
    }
    let sequence_report_parser = match &goal.intent {
        Intent::ShowCalculationSequenceHistory => {
            Some(crate::input::SEQUENCE_HISTORY_PARSER_VERSION)
        }
        Intent::ExplainCalculationSequenceStep { .. } => {
            Some(crate::input::SEQUENCE_STEP_EXPLANATION_PARSER_VERSION)
        }
        _ => None,
    };
    let requires_typed_sequence_report = matches!(
        bundle.meaning.parser_version.as_str(),
        crate::input::SEQUENCE_HISTORY_PARSER_VERSION
            | crate::input::SEQUENCE_STEP_EXPLANATION_PARSER_VERSION
    );
    if sequence_report_parser
        != requires_typed_sequence_report.then_some(bundle.meaning.parser_version.as_str())
    {
        return Err(failure(
            "RECOVERY_CALCULATION_SEQUENCE_CONTEXT_INVALID",
            "the sequence report intent differs from the sealed parser contract",
        ));
    }
    let requires_generated_why = crate::input::uses_generated_relation_explanation(&bundle.meaning);
    let sequence_goal_predicate =
        goal.required_predicates
            .iter()
            .map(String::as_str)
            .find(|predicate| match goal.intent {
                Intent::ExplainPrevious => matches!(
                    *predicate,
                    SEQUENCE_EXPLANATION_PREDICATE
                        | CHAINED_SEQUENCE_EXPLANATION_PREDICATE
                        | REVISED_SEQUENCE_EXPLANATION_PREDICATE
                        | REWIRED_SEQUENCE_EXPLANATION_PREDICATE
                        | APPENDED_SEQUENCE_EXPLANATION_PREDICATE
                        | REMOVED_SEQUENCE_EXPLANATION_PREDICATE
                        | MOVED_SEQUENCE_EXPLANATION_PREDICATE
                        | INSERTED_SEQUENCE_EXPLANATION_PREDICATE
                ),
                Intent::CorrectCalculationSequenceStep { .. } => matches!(
                    *predicate,
                    SEQUENCE_CORRECTION_PREDICATE
                        | REPEATED_SEQUENCE_CORRECTION_PREDICATE
                        | REWIRED_SEQUENCE_CORRECTION_PREDICATE
                ),
                Intent::AppendCalculationSequenceStep { .. } => {
                    *predicate == SEQUENCE_APPEND_PREDICATE
                }
                Intent::RemoveCalculationSequenceStep { .. } => {
                    *predicate == SEQUENCE_REMOVE_PREDICATE
                }
                Intent::MoveCalculationSequenceStep { .. } => *predicate == SEQUENCE_MOVE_PREDICATE,
                Intent::InsertCalculationSequenceStep { .. } => {
                    *predicate == SEQUENCE_INSERT_PREDICATE
                }
                Intent::ShowCalculationSequenceHistory => *predicate == SEQUENCE_HISTORY_PREDICATE,
                Intent::ExplainCalculationSequenceStep { .. } => {
                    *predicate == SEQUENCE_STEP_EXPLANATION_PREDICATE
                }
                _ => false,
            });
    let sequence_predicate = sequence_goal_predicate.filter(|_| !requires_sequence_edit);
    let explains_sequence = sequence_predicate.is_some();
    let assertion_predicate = (matches!(goal.intent, Intent::ExplainPrevious)
        && goal
            .required_predicates
            .iter()
            .any(|predicate| predicate == crate::core::PREVIOUS_ASSERTION_EXPLANATION_PREDICATE))
    .then_some(crate::core::PREVIOUS_ASSERTION_EXPLANATION_PREDICATE);
    if goal.required_predicates
        != goal_predicates(
            &bundle.meaning,
            true,
            sequence_goal_predicate.or(assertion_predicate),
        )
        && (requires_generated_comparison
            || requires_generated_calculation
            || requires_verified_correction
            || requires_condition_evaluation
            || requires_record_source
            || requires_guarded_rule
            || requires_multi_condition_relation
            || requires_sequence_edit
            || requires_typed_sequence_report
            || requires_generated_why
            || matches!(
                goal.intent,
                Intent::AskWeb { .. }
                    | Intent::ExplainCode { .. }
                    | Intent::GenerateReturnFunction { .. }
            )
            || explains_sequence
            || assertion_predicate.is_some()
            || goal.required_predicates != legacy_predicates)
    {
        return Err(failure(
            "UNSUPPORTED_RECOVERY_GOAL",
            "unrecognized completion predicates remain unresolved",
        ));
    }
    // V1 measured the original replay from the first calculation step.
    let sequence_started = matches!(
        sequence_predicate,
        Some(
            CHAINED_SEQUENCE_EXPLANATION_PREDICATE
                | REVISED_SEQUENCE_EXPLANATION_PREDICATE
                | REWIRED_SEQUENCE_EXPLANATION_PREDICATE
                | APPENDED_SEQUENCE_EXPLANATION_PREDICATE
                | REMOVED_SEQUENCE_EXPLANATION_PREDICATE
                | MOVED_SEQUENCE_EXPLANATION_PREDICATE
                | INSERTED_SEQUENCE_EXPLANATION_PREDICATE
                | SEQUENCE_HISTORY_PREDICATE
                | SEQUENCE_STEP_EXPLANATION_PREDICATE
        )
    )
    .then(Instant::now);
    let previous_sequence = if let Some(predicate) = sequence_predicate {
        let invalid = || {
            failure(
                "RECOVERY_CALCULATION_SEQUENCE_CONTEXT_INVALID",
                "the sealed explanation no longer names its committed calculation sequence",
            )
        };
        verify_registered_input(
            store,
            bundle,
            "RECOVERY_CALCULATION_SEQUENCE_CONTEXT_INVALID",
        )?;
        let previous = bundle
            .meaning
            .context_result_ref
            .as_ref()
            .and_then(|id| state.prior_results.get(id))
            .ok_or_else(invalid)?;
        let context = previous_calculation_sequence(
            store,
            bundle,
            previous,
            &state.prior_results,
            sequence_started,
        )?
        .filter(|context| requires_typed_sequence_report || context.predicate() == predicate)
        .ok_or_else(invalid)?;
        Some(context)
    } else {
        None
    };
    let edit_started = Instant::now();
    let sequence_edit = if requires_sequence_edit {
        verify_registered_input(store, bundle, "RECOVERY_SEQUENCE_CORRECTION_MISMATCH")?;
        let previous = bundle
            .meaning
            .context_result_ref
            .as_ref()
            .and_then(|id| state.prior_results.get(id))
            .ok_or_else(|| {
                failure(
                    "RECOVERY_SEQUENCE_CORRECTION_MISMATCH",
                    "the sealed sequence edit has no retained context",
                )
            })?;
        let context =
            prepare_sequence_edit(store, bundle, previous, &state.prior_results, edit_started)?;
        if Some(context.predicate()) != sequence_goal_predicate {
            return Err(failure(
                "RECOVERY_SEQUENCE_CORRECTION_MISMATCH",
                "the authenticated sequence edit lineage differs from the exact goal predicate",
            ));
        }
        Some(context)
    } else {
        None
    };
    let requires_proof_explanation = goal.required_predicates.iter().any(|predicate| {
        matches!(
            predicate.as_str(),
            RELATION_EXPLANATION_PREDICATE | GENERATED_RELATION_EXPLANATION_PREDICATE
        )
    });
    let plan = result.content_plan.as_ref().ok_or_else(|| {
        failure(
            "RECOVERY_DATA_UNAVAILABLE",
            "adopted content plan unavailable",
        )
    })?;
    let render = result.render.as_ref().ok_or_else(|| {
        failure(
            "RECOVERY_DATA_UNAVAILABLE",
            "actual retained rendering unavailable",
        )
    })?;
    if requires_verified_correction {
        let started = Instant::now();
        let invalid = || {
            failure(
                "RECOVERY_CALCULATION_CORRECTION_MISMATCH",
                "the adopted correction differs from the exact sealed operand and its prior result",
            )
        };
        verify_registered_input(store, bundle, "RECOVERY_CALCULATION_CORRECTION_MISMATCH")?;
        let previous = bundle
            .meaning
            .context_result_ref
            .as_ref()
            .and_then(|id| state.prior_results.get(id))
            .ok_or_else(invalid)?;
        let calculation = corrected_calculation_from_context(
            store,
            bundle,
            previous,
            &state.prior_results,
            started,
        )?;
        let unit = exact_calculation_unit(&calculation);
        if result.calculation.as_ref() != Some(&calculation)
            || result.guarantee != Guarantee::Computed
            || plan.guarantee != Guarantee::Computed
            || result.dependencies.as_slice() != std::slice::from_ref(&previous.id)
            || !result.evidence.is_empty()
            || plan.required_units.as_slice() != std::slice::from_ref(&unit)
            || plan.text != unit
            || result.text != unit
        {
            return Err(invalid());
        }
    }
    let saved_comparison_explanation = matches!(&goal.intent, Intent::ExplainPrevious)
        && bundle
            .meaning
            .context_result_ref
            .as_deref()
            .and_then(|id| state.prior_results.get(id))
            .is_some_and(crate::core::is_saved_relation_comparison_result);
    let saved_relation_profile_explanation = if matches!(&goal.intent, Intent::ExplainPrevious) {
        match bundle
            .meaning
            .context_result_ref
            .as_deref()
            .and_then(|id| state.prior_results.get(id))
        {
            Some(previous) => {
                crate::core::previous_relation_profile_request(store, actor, case, previous)?
                    .is_some()
            }
            None => false,
        }
    } else {
        false
    };
    let previous_document_explanation = if matches!(goal.intent, Intent::ExplainPrevious) {
        bundle
            .meaning
            .context_result_ref
            .as_ref()
            .and_then(|id| state.prior_results.get(id))
            .map(|previous| {
                crate::document::previous_document_explanation(
                    store,
                    actor,
                    case,
                    previous,
                    &goal.budget,
                    &bundle.meaning.parser_version,
                    Instant::now(),
                )
            })
            .transpose()?
            .flatten()
    } else {
        None
    };
    let saved_summary_explanation =
        previous_document_explanation
            .as_ref()
            .is_some_and(|explanation| {
                explanation.report_node == crate::document::TEXT_SUMMARY_CANDIDATE_REPORT_NODE
            });
    let direct_summary_candidate = matches!(
        goal.intent,
        Intent::TextSummaryCandidates { .. } | Intent::SummarizePastedText { .. }
    );
    if direct_summary_candidate {
        verify_registered_input(store, bundle, TEXT_SUMMARY_CANDIDATE_MISMATCH)?;
        verify_text_summary_candidate(store, bundle, result, plan)?;
    }
    let legacy_summary_explanation =
        verify_legacy_text_summary_explanation(store, state, bundle, result, plan)?;
    let retained_summary_explanation = if matches!(goal.intent, Intent::ExplainPrevious)
        && result.guarantee == Guarantee::Hypothesis
    {
        bundle
            .meaning
            .context_result_ref
            .as_ref()
            .and_then(|id| state.prior_results.get(id))
            .map(|prior| {
                crate::text_summary::previous_summary_explanation(
                    store,
                    bundle,
                    &state.prior_results,
                    prior,
                )
            })
            .transpose()?
            .flatten()
    } else {
        None
    };
    if let Some(explanation) = &retained_summary_explanation {
        const CODE: &str = "RECOVERY_RESULT_SUMMARY_EXPLANATION_MISMATCH";
        verify_registered_input(store, bundle, CODE)?;
        if result.dependencies != explanation.dependencies
            || plan.required_units != explanation.units
            || result.text != explanation.units.join("\n")
            || result.calculation.is_some()
            || !result.evidence.is_empty()
        {
            return Err(failure(
                CODE,
                "the adopted explanation differs from the sealed result-summary context",
            ));
        }
    }
    if matches!(goal.intent, Intent::ExplainPrevious) && result.guarantee == Guarantee::SourceReport
    {
        const CODE: &str = "RECOVERY_SEARCH_EXPLANATION_MISMATCH";
        let previous = bundle
            .meaning
            .context_result_ref
            .as_ref()
            .and_then(|id| state.prior_results.get(id))
            .ok_or_else(|| failure(CODE, "the sealed prior source result is not retained"))?;
        if let Some(explanation) =
            search::previous_search_explanation(store, bundle, &state.prior_results, previous)?
        {
            verify_registered_input(store, bundle, CODE)?;
            if result.dependencies != explanation.dependencies
                || plan.required_units != explanation.units
                || result.text != explanation.units.join("\n")
                || result.calculation.is_some()
                || !result.evidence.is_empty()
            {
                return Err(failure(
                    CODE,
                    "the adopted explanation differs from the original search context",
                ));
            }
        }
    }
    let mut unsigned_plan = plan.clone();
    unsigned_plan.id.clear();
    if plan.id != identity(&unsigned_plan)?
        || plan.text != result.text
        || plan.guarantee != result.guarantee
        || plan.locale != request.locale
        || plan.locale != "ja"
        || plan.required_units.is_empty()
        || plan.guarantee == Guarantee::Unknown
        || (plan.guarantee == Guarantee::Hypothesis
            && !matches!(
                goal.intent,
                Intent::SummarizePreviousSource
                    | Intent::SummarizePreviousSourceConstrained { .. }
                    | Intent::SummarizePreviousSourceLines { .. }
                    | Intent::SummarizePreviousSourcePacketLines { .. }
                    | Intent::SummarizePreviousResult { .. }
                    | Intent::SummarizePreviousResultLines { .. }
                    | Intent::AskPreviousSourceQuestion { .. }
                    | Intent::CompareRelationInventories { .. }
                    | Intent::DraftRelationProfile { .. }
                    | Intent::RetainPreviousGroupMeanHypothesis
                    | Intent::ExplainPreviousGroupMeanHypothesis
                    | Intent::AssessPreviousGroupMeanHypothesis { .. }
            )
            && !saved_comparison_explanation
            && !saved_relation_profile_explanation
            && !saved_summary_explanation
            && !direct_summary_candidate
            && !legacy_summary_explanation
            && retained_summary_explanation.is_none())
    {
        return Err(failure(
            "RECOVERY_CONTENT_MISMATCH",
            "retained content identity or meaning is unverified",
        ));
    }
    if !validate_goal_render(&goal, plan, render) {
        return Err(failure(
            "RECOVERY_RENDER_MISMATCH",
            "retained renderer version, destination, status or bytes do not match the original goal and adopted plan",
        ));
    }
    if result.input_id != request.input_id
        || result.input_version != request.input_version
        || result.id != fingerprint
    {
        return Err(failure(
            "RECOVERY_IDENTITY_MISMATCH",
            "adopted result differs from original input",
        ));
    }
    verify_goal_content(&goal, result, plan)?;
    if let Some((_, predicate)) = condition_contract {
        if goal.required_predicates.first().map(String::as_str) != Some(predicate) {
            return Err(failure(
                "RECOVERY_CONDITION_EVALUATION_MISMATCH",
                "the condition evaluation has no exact typed completion predicate",
            ));
        }
        verify_condition_evaluation_result(store, bundle, result, plan, &goal)?;
    }
    if requires_record_source {
        verify_record_source_result(store, state, bundle, result, plan, &goal)?;
    }
    if requires_guarded_rule {
        verify_guarded_rule_registration(store, bundle, result, plan, &goal)?;
    }
    if requires_multi_condition_relation && let Intent::Assert { proposition } = &goal.intent {
        verify_multi_condition_premise(store, bundle, result, plan, proposition)?;
    }
    if let Some(hypothesis) = crate::core::previous_group_mean_hypothesis(store, state)? {
        if result.guarantee != Guarantee::Hypothesis
            || result.dependencies != hypothesis.dependencies
            || plan.required_units != hypothesis.units
        {
            return Err(failure(
                "RECOVERY_GROUP_MEAN_HYPOTHESIS_CHANGED",
                "the retained hypothesis and its observation no longer match the adopted result",
            ));
        }
        hypothesis.verify_artifacts(store, actor, case)?;
    }
    let original_relation = if requires_condition_evaluation || requires_record_source {
        None
    } else {
        verify_result_memory_provenance(store, actor, case, state, result, plan, &goal)?
    };
    let source_summary =
        verify_source_summary_context(store, actor, case, state, result, plan, &goal)?;
    let previous_result_summary =
        verify_previous_result_summary_context(store, actor, case, state, result, plan, &goal)?;
    let source_question =
        verify_source_question_context(store, actor, case, state, result, plan, &goal)?;
    let previous_text_edit =
        verify_previous_text_edit_context(store, actor, case, state, result, plan, &goal)?;
    let previous_csv_statistics_explanation = if matches!(goal.intent, Intent::ExplainPrevious) {
        match bundle
            .meaning
            .context_result_ref
            .as_ref()
            .and_then(|id| state.prior_results.get(id))
        {
            Some(previous) => {
                crate::document::previous_csv_statistics_explanation(store, actor, case, previous)?
            }
            None => None,
        }
    } else {
        None
    };
    let previous_json_table_group_statistics_explanation =
        if matches!(goal.intent, Intent::ExplainPrevious) {
            match bundle
                .meaning
                .context_result_ref
                .as_ref()
                .and_then(|id| state.prior_results.get(id))
            {
                Some(previous) => {
                    crate::document::previous_json_table_group_statistics_explanation(
                        store, actor, case, previous,
                    )?
                }
                None => None,
            }
        } else {
            None
        };
    let previous_json_table_operation_explanation =
        if matches!(goal.intent, Intent::ExplainPrevious) {
            bundle
                .meaning
                .context_result_ref
                .as_ref()
                .and_then(|id| state.prior_results.get(id))
                .map(|previous| {
                    crate::document::previous_json_table_operation_explanation(
                        store,
                        actor,
                        case,
                        previous,
                        goal.budget.max_evidence_bytes,
                        &bundle.meaning.parser_version,
                    )
                })
                .transpose()?
                .flatten()
        } else {
            None
        };
    let previous_json_group_mean_answer = if matches!(goal.intent, Intent::AskPreviousGroupMean) {
        bundle
            .meaning
            .context_result_ref
            .as_ref()
            .and_then(|id| state.prior_results.get(id))
            .filter(|previous| previous.lifecycle == Lifecycle::Commit)
            .map(|previous| -> Result<Option<JsonGroupMeanRecovery>> {
                let Some((source_ref, report_ref, answer)) =
                    crate::document::previous_json_table_highest_group_mean(
                        store, actor, case, previous,
                    )?
                else {
                    return Ok(None);
                };
                let answer_bytes = canonical(&answer)?;
                let answer_ref = digest(&answer_bytes);
                let expected_units = crate::document::json_group_mean_answer_lines(&answer);
                Ok(Some((
                    previous.id.clone(),
                    source_ref,
                    report_ref,
                    answer_ref,
                    answer_bytes,
                    expected_units,
                )))
            })
            .transpose()?
            .flatten()
    } else {
        None
    };
    let previous_math_report_explanation = if matches!(goal.intent, Intent::ExplainPrevious) {
        bundle
            .meaning
            .context_result_ref
            .as_ref()
            .and_then(|id| state.prior_results.get(id))
            .map(crate::math::previous_math_report_explanation)
            .transpose()?
            .flatten()
    } else {
        None
    };
    let mut generated_compute_ref = if requires_generated_calculation {
        Some(crate::core::verify_generated_calculation_computation(
            store, actor, case, &goal, result, plan,
        )?)
    } else {
        None
    };
    if let Intent::Ask { proposition } | Intent::AskWhy { proposition } = &goal.intent {
        // A case in another client may have invalidated a premise while this
        // output was pending. Revalidate the same proof lineage; never adopt a
        // newly different answer during mere effect reconciliation. Use a
        // current revision-matched Goal projection when available; cold/stale
        // indexes retain the authenticated full-snapshot path.
        let evaluated;
        let (memory, answer) = match crate::core::indexed_relation_query_from_store(
            store,
            actor,
            proposition,
            goal.budget.max_actions,
        )? {
            Some((projected_memory, answer)) => {
                evaluated = (projected_memory, answer?);
                &evaluated
            }
            None => {
                // Validate the current memory and its input evidence first.
                // Only the projection and proof of the exact same artifact
                // can be reused, for this same actor and sealed query.
                let (memory, reference) = store.load_memory_with_reference(actor)?;
                let same_snapshot = original_relation.as_ref().filter(|proof| {
                    uses_snapshot_relation_index(state, proposition)
                        && reference.as_ref().is_some_and(|(revision, sha256)| {
                            u64::try_from(*revision).ok() == Some(proof.projection.0.revision)
                                && *sha256 == proof.snapshot_sha256
                        })
                });
                if let Some(proof) = same_snapshot {
                    &proof.projection
                } else {
                    evaluated = query_recovery_snapshot(
                        state,
                        memory,
                        proposition,
                        goal.budget.max_actions,
                    )?;
                    &evaluated
                }
            }
        };
        if matches!(goal.intent, Intent::AskWhy { .. }) {
            generated_compute_ref = crate::core::verify_generated_why_computation(
                store,
                state,
                result,
                plan,
                proposition,
                crate::core::RelationRecoveryProofs {
                    current: (memory, answer),
                    original: original_relation
                        .as_ref()
                        .map(|proof| (&proof.projection.0, &proof.projection.1)),
                },
                &goal.budget,
            )?;
            if requires_generated_why && generated_compute_ref.is_none() {
                return Err(failure(
                    "RECOVERY_GENERATED_PROOF_REQUIRED",
                    "the sealed explanation goal requires its typed generation artifact",
                ));
            }
        }
        // Revision-pinned results use the complete lineage regardless of the
        // parser release that sealed their input.
        let current_proof_contract = result.actor_memory_revision.is_some()
            || requires_proof_explanation
            || generated_compute_ref.is_some()
            || matches!(
                bundle.meaning.parser_version.as_str(),
                "declarative-ja-en-v46-proof-lineage-guarantee"
                    | "declarative-ja-en-v47-premise-inspection"
                    | "declarative-ja-en-v48-related-context-explanation"
                    | "declarative-ja-en-v49-why-proof-query"
                    | "declarative-ja-en-v50-code-context-explanation"
                    | "declarative-ja-en-v51-generated-code-verification"
                    | "declarative-ja-en-v52-search-candidate-body"
                    | "declarative-ja-en-v53-group-mean-query"
                    | "declarative-ja-en-v54-source-question-followup"
                    | "declarative-ja-en-v55-natural-premise-question"
                    | "declarative-ja-en-v56-natural-premise-correction"
                    | "declarative-ja-en-v57-contextual-premise-correction"
                    | "declarative-ja-en-v58-prior-result-pronoun-query"
                    | "declarative-ja-en-v59-conjunctive-relation-question"
                    | "declarative-ja-en-v60-conjunctive-relation-rule"
                    | "declarative-ja-en-v61-three-clause-relation-question"
                    | "declarative-ja-en-v62-constrained-source-summary"
                    | "declarative-ja-en-v63-previous-output-edit"
                    | "declarative-ja-en-v68-previous-result-summary"
                    | "declarative-ja-en-v70-generic-transitive-verbs"
            );
        let dependencies = if current_proof_contract {
            crate::relations::answer_dependencies(answer)
        } else {
            // Preserve recovery for records produced before the v46 lineage
            // and guarantee contract.
            let mut legacy: Vec<String> = answer.facts.iter().map(|fact| fact.id.clone()).collect();
            legacy.extend(answer.used_rules.iter().cloned());
            legacy
        };
        let guarantee_matches = !current_proof_contract
            || result.guarantee == crate::relations::answer_guarantee(answer);
        if !matches!(answer.status.as_str(), "SUPPORTED" | "CONTRADICTED")
            || dependencies != result.dependencies
            || !guarantee_matches
        {
            return Err(failure(
                "RECOVERY_PROOF_INVALIDATED",
                "original adopted relation proof is no longer valid unchanged",
            ));
        }
    }
    if let Intent::AskRelationObjects {
        subject,
        predicate,
        negative,
        scope,
    } = &goal.intent
    {
        // Rebuild the exact supported object list from current Core memory.
        // If a premise or rule changed after the original write attempt, the
        // old bytes cannot be confirmed as the current goal's result. Use the
        // revision-matched scoped index when available; cold/stale stores keep
        // the authenticated full-snapshot compatibility path.
        let replay = crate::core::RelationObjectsReplay {
            subject,
            predicate,
            negative: *negative,
            scope,
            max_actions: goal.budget.max_actions,
            max_evidence_bytes: goal.budget.max_evidence_bytes,
        };
        match crate::core::indexed_relation_objects_answer_from_store(store, actor, replay)? {
            Some((_indexed_memory, answer)) => {
                verify_relation_objects_current_answer(answer, result, plan)?;
            }
            None => {
                let memory = store.load_memory(actor)?;
                let answer = crate::core::relation_objects_answer(
                    &memory,
                    subject,
                    predicate,
                    *negative,
                    scope,
                    goal.budget.max_actions,
                    goal.budget.max_evidence_bytes,
                );
                verify_relation_objects_current_answer(answer, result, plan)?;
            }
        }
    }
    if matches!(goal.intent, Intent::AskCommonRelations { .. })
        && result.actor_memory_revision.is_some()
    {
        // The exact immutable snapshot was replayed above. Before confirming
        // pending bytes, ensure the common relations still have identical
        // current support and artifact contents. Use the current revision's
        // bounded index when available; a cold or stale index keeps the
        // authenticated full-snapshot compatibility path.
        let current_verification = (|| match verify_common_relations_memory_result_with_index(
            store, actor, case, result, plan, &goal,
        )? {
            Some(()) => Ok(()),
            None => {
                let memory = store.load_memory(actor)?;
                verify_common_relations_memory_result(
                    store, actor, case, &memory, result, plan, &goal,
                )
            }
        })();
        current_verification.map_err(|error| {
            failure(
                "RECOVERY_PROOF_INVALIDATED",
                format!("the original common-relation proof is no longer valid: {error}"),
            )
        })?;
    }
    if matches!(goal.intent, Intent::CompareRelationInventories { .. }) {
        // The exact immutable snapshot was replayed above when this result has
        // a revision. Older results without one must still match current
        // memory before their retained report can be confirmed.
        let current_verification = (|| {
            match verify_relation_comparison_memory_result_with_index(
                store, actor, case, result, plan, &goal,
            )? {
                Some(()) => Ok(()),
                None => {
                    // Preserve old-store compatibility when the derived index
                    // has not yet been built for the current memory head.
                    let memory = store.load_memory(actor)?;
                    verify_relation_comparison_memory_result(
                        store, actor, case, &memory, result, plan, &goal,
                    )
                }
            }
        })();
        current_verification.map_err(|error| {
            failure(
                "RECOVERY_PROOF_INVALIDATED",
                format!("the original relation-comparison proof is no longer valid: {error}"),
            )
        })?;
    }
    if matches!(goal.intent, Intent::DraftRelationProfile { .. })
        && result.actor_memory_revision.is_some()
    {
        // Rebuild from the current actor memory separately from the exact
        // snapshot replay above. Use the revision-matched subject index when
        // available; cold/stale indexes retain the authenticated full-snapshot
        // path. Changed or invalidated profile evidence must stop recovery.
        let current_verification = (|| match verify_relation_profile_memory_result_with_index(
            store, actor, result, plan, &goal,
        )? {
            Some(()) => Ok(()),
            None => {
                let memory = store.load_memory(actor)?;
                verify_relation_profile_memory_result(&memory, result, plan, &goal)
            }
        })();
        current_verification.map_err(|error| {
            failure(
                "RECOVERY_PROOF_INVALIDATED",
                format!("the original relation-profile proof is no longer valid: {error}"),
            )
        })?;
    }
    if matches!(goal.intent, Intent::AskConjunction { .. }) {
        // The original immutable snapshot was checked above. Before confirming
        // pending bytes, ensure the conjunction is still true of the current
        // actor memory. Use the revision-matched clause index when available;
        // cold/stale indexes retain the authenticated full-snapshot path.
        let current_verification = (|| match verify_conjunction_memory_result_with_index(
            store, actor, result, plan, &goal,
        )? {
            Some(()) => Ok(()),
            None => {
                let memory = store.load_memory(actor)?;
                verify_conjunction_memory_result(&memory, result, plan, &goal)
            }
        })();
        current_verification.map_err(|error| {
            failure(
                "RECOVERY_PROOF_INVALIDATED",
                format!("the original conjunction proof is no longer valid: {error}"),
            )
        })?;
    }
    if matches!(goal.intent, Intent::AskPreviousRelation { .. })
        && result.actor_memory_revision.is_some()
    {
        // Replay used the retained snapshot above; separately ensure the
        // prior-result anchor still resolves to the same proof in live memory.
        // Use the shared revision-matched prior-dependency index when present;
        // cold/stale indexes retain the authenticated full-snapshot path.
        let current_verification = (|| match verify_previous_relation_memory_result_with_index(
            store, actor, state, result, plan, &goal,
        )? {
            Some(()) => Ok(()),
            None => {
                let memory = store.load_memory(actor)?;
                verify_previous_relation_memory_result(state, &memory, result, plan, &goal)
            }
        })();
        current_verification.map_err(|error| {
            failure(
                "RECOVERY_PROOF_INVALIDATED",
                format!("the original prior-relation proof is no longer valid: {error}"),
            )
        })?;
    }
    if let Intent::InspectPremises { subject } = &goal.intent {
        let current_inventory = if result.actor_memory_revision.is_some() {
            match crate::core::indexed_premise_inventory_from_store(
                store,
                actor,
                subject,
                goal.budget.max_actions,
            )? {
                Some((_revision, inventory)) => premise_relation_inventory_summary(inventory).ok(),
                None => {
                    let memory = store.load_memory(actor)?;
                    premise_memory_inventory(&memory, subject, goal.budget.max_actions).ok()
                }
            }
        } else {
            let memory = store.load_memory(actor)?;
            if matches!(
                bundle.meaning.parser_version.as_str(),
                "declarative-ja-en-v68-previous-result-summary"
                    | "declarative-ja-en-v69-relation-profile-draft"
                    | "declarative-ja-en-v70-generic-transitive-verbs"
            ) {
                premise_memory_inventory(&memory, subject, goal.budget.max_actions).ok()
            } else {
                let dependencies = crate::relations::explicit_premises_about(&memory, subject)
                    .into_iter()
                    .map(|fact| fact.id)
                    .collect();
                Some((dependencies, Guarantee::UserPremise))
            }
        };
        let Some((dependencies, guarantee)) = current_inventory else {
            return Err(failure(
                "RECOVERY_PREMISES_CHANGED",
                "the original active premise listing is no longer valid unchanged",
            ));
        };
        if dependencies.is_empty()
            || dependencies != result.dependencies
            || result.guarantee != guarantee
        {
            return Err(failure(
                "RECOVERY_PREMISES_CHANGED",
                "the original active premise listing is no longer valid unchanged",
            ));
        }
    }
    if matches!(goal.intent, Intent::ExplainPrevious) && !explains_sequence {
        verify_previous_relation_context(store, actor, case, state, result, plan, &goal)?;
    }
    if matches!(goal.intent, Intent::GenerateReturnFunction { .. }) {
        const CODE: &str = "RECOVERY_RETURN_FUNCTION_MISMATCH";
        verify_registered_input(store, bundle, CODE)?;
        let report = crate::code_generation::generate_return_function(bundle)?;
        let reference = verified_artifact(store, actor, case, &report)?;
        if result.guarantee != Guarantee::Computed
            || result.dependencies != [reference]
            || plan.guarantee != result.guarantee
            || plan.required_units != [report.source.as_str()]
            || plan.text != report.source
            || result.text != report.source
            || serde_json::to_vec(&report)?.len() > goal.budget.max_evidence_bytes
        {
            return Err(failure(
                CODE,
                "the retained code differs from the original function request",
            ));
        }
    }
    if matches!(goal.intent, Intent::ExplainCode { .. }) {
        const CODE: &str = "RECOVERY_SOURCE_CODE_EXPLANATION_MISMATCH";
        verify_registered_input(store, bundle, CODE)?;
        let source = crate::input::verified_code_explanation_source(bundle)?;
        let (report, units) = crate::code_capability::explain_source(source)?;
        let expected_text = units.join("\n");
        let report_size = canonical(&report)?.len();
        let report_ref = verified_artifact(store, actor, case, &report)?;
        if result.guarantee != Guarantee::Computed
            || result.dependencies != [report_ref]
            || plan.guarantee != result.guarantee
            || plan.required_units != units
            || plan.text != expected_text
            || result.text != expected_text
            || report_size.saturating_add(expected_text.len()) > goal.budget.max_evidence_bytes
        {
            return Err(failure(
                CODE,
                "The retained source report or explanation differs from the original code.",
            ));
        }
    }
    if matches!(
        bundle.meaning.parser_version.as_str(),
        "declarative-ja-en-v50-code-context-explanation"
            | "declarative-ja-en-v70-generic-transitive-verbs"
    ) && matches!(goal.intent, Intent::ExplainPrevious)
        && let Some(previous_id) = bundle.meaning.context_result_ref.as_ref()
        && let Some(previous) = state.prior_results.get(previous_id)
        && let Some((report_ref, expected_units)) =
            crate::code_generation::previous_code_explanation(store, actor, case, previous)?
    {
        let expected_dependencies = vec![previous.id.clone(), report_ref];
        if previous.lifecycle != Lifecycle::Commit
            || result.dependencies != expected_dependencies
            || plan.required_units != expected_units
            || result.guarantee != previous.guarantee
        {
            return Err(failure(
                "RECOVERY_CODE_CONTEXT_CHANGED",
                "the code-generation source or structured explanation no longer matches the prior result",
            ));
        }
    }
    if assertion_predicate.is_some() {
        verify_previous_assertion_explanation(store, state, bundle, result, plan)?;
    }
    if let Some((report_ref, expected_units)) = &previous_csv_statistics_explanation {
        let previous_id = bundle.meaning.context_result_ref.as_ref().ok_or_else(|| {
            failure(
                "RECOVERY_CSV_CONTEXT_CHANGED",
                "the prior CSV-statistics result reference is missing",
            )
        })?;
        let previous = state.prior_results.get(previous_id).ok_or_else(|| {
            failure(
                "RECOVERY_CSV_CONTEXT_CHANGED",
                "the prior CSV-statistics result is no longer retained",
            )
        })?;
        if previous.lifecycle != Lifecycle::Commit
            || result.dependencies != vec![previous.id.clone(), report_ref.clone()]
            || plan.required_units != *expected_units
            || result.guarantee != previous.guarantee
        {
            return Err(failure(
                "RECOVERY_CSV_CONTEXT_CHANGED",
                "the exact statistics report or source-cell explanation changed",
            ));
        }
    }
    if let Some((source_ref, report_ref, expected_units)) =
        &previous_json_table_group_statistics_explanation
    {
        let previous_id = bundle.meaning.context_result_ref.as_ref().ok_or_else(|| {
            failure(
                "RECOVERY_JSON_TABLE_CONTEXT_CHANGED",
                "the prior JSON grouped-statistics result reference is missing",
            )
        })?;
        let previous = state.prior_results.get(previous_id).ok_or_else(|| {
            failure(
                "RECOVERY_JSON_TABLE_CONTEXT_CHANGED",
                "the prior JSON grouped-statistics result is no longer retained",
            )
        })?;
        let expected_dependencies =
            vec![previous.id.clone(), source_ref.clone(), report_ref.clone()];
        if previous.lifecycle != Lifecycle::Commit
            || result.dependencies != expected_dependencies
            || plan.required_units != *expected_units
            || result.guarantee != previous.guarantee
        {
            return Err(failure(
                "RECOVERY_JSON_TABLE_CONTEXT_CHANGED",
                "the exact JSON source, grouped-statistics report or explanation changed",
            ));
        }
    }
    if let Some((bundle_ref, report_ref, expected_units)) =
        &previous_json_table_operation_explanation
    {
        let previous_id = bundle.meaning.context_result_ref.as_ref().ok_or_else(|| {
            failure(
                "RECOVERY_JSON_TABLE_CONTEXT_CHANGED",
                "the prior JSON table transformation reference is missing",
            )
        })?;
        let previous = state.prior_results.get(previous_id).ok_or_else(|| {
            failure(
                "RECOVERY_JSON_TABLE_CONTEXT_CHANGED",
                "the prior JSON table transformation is no longer retained",
            )
        })?;
        let expected_dependencies =
            vec![previous.id.clone(), bundle_ref.clone(), report_ref.clone()];
        if previous.lifecycle != Lifecycle::Commit
            || result.dependencies != expected_dependencies
            || plan.required_units != *expected_units
            || result.guarantee != previous.guarantee
        {
            return Err(failure(
                "RECOVERY_JSON_TABLE_CONTEXT_CHANGED",
                "the sealed JSON source, exact report artifact or source-span explanation changed",
            ));
        }
    }
    if let Some(explanation) = &previous_document_explanation {
        let code = if saved_summary_explanation {
            "RECOVERY_TEXT_SUMMARY_CONTEXT_CHANGED"
        } else {
            "RECOVERY_CSV_TABLE_CONTEXT_CHANGED"
        };
        let previous_id = bundle
            .meaning
            .context_result_ref
            .as_ref()
            .ok_or_else(|| failure(code, "the prior document result reference is missing"))?;
        let previous = state
            .prior_results
            .get(previous_id)
            .ok_or_else(|| failure(code, "the prior document result is no longer retained"))?;
        let expected_dependencies = vec![
            previous.id.clone(),
            explanation.bundle_ref.clone(),
            explanation.report_ref.clone(),
        ];
        if previous.lifecycle != Lifecycle::Commit
            || result.dependencies != expected_dependencies
            || plan.required_units != explanation.lines
            || result.guarantee != previous.guarantee
        {
            return Err(failure(
                code,
                "the sealed input, exact report or source-span explanation changed",
            ));
        }
    }
    if let Some((previous_id, source_ref, report_ref, answer_ref, answer_bytes, expected_units)) =
        &previous_json_group_mean_answer
    {
        let previous = state.prior_results.get(previous_id).ok_or_else(|| {
            failure(
                "RECOVERY_JSON_GROUP_MEAN_CHANGED",
                "the queried grouped-statistics result is no longer retained",
            )
        })?;
        let expected_dependencies = vec![
            previous.id.clone(),
            source_ref.clone(),
            report_ref.clone(),
            answer_ref.clone(),
        ];
        if previous.lifecycle != Lifecycle::Commit
            || result.dependencies != expected_dependencies
            || plan.required_units != *expected_units
            || result.guarantee != Guarantee::Computed
            || store.get_bytes(actor, case, answer_ref)? != *answer_bytes
        {
            return Err(failure(
                "RECOVERY_JSON_GROUP_MEAN_CHANGED",
                "the revalidated source, grouped means or highest-group answer changed",
            ));
        }
    }
    if let Some(expected_units) = &previous_math_report_explanation {
        let previous_id = bundle.meaning.context_result_ref.as_ref().ok_or_else(|| {
            failure(
                "RECOVERY_MATH_CONTEXT_CHANGED",
                "the prior mathematical result reference is missing",
            )
        })?;
        let previous = state.prior_results.get(previous_id).ok_or_else(|| {
            failure(
                "RECOVERY_MATH_CONTEXT_CHANGED",
                "the prior mathematical result is no longer retained",
            )
        })?;
        if previous.lifecycle != Lifecycle::Commit
            || result.dependencies != vec![previous.id.clone()]
            || plan.required_units != *expected_units
            || result.guarantee != previous.guarantee
        {
            return Err(failure(
                "RECOVERY_MATH_CONTEXT_CHANGED",
                "the exact mathematical report or explanation no longer matches the prior result",
            ));
        }
    }
    if matches!(
        bundle.meaning.parser_version.as_str(),
        "declarative-ja-en-v51-generated-code-verification"
            | "declarative-ja-en-v52-search-candidate-body"
            | "declarative-ja-en-v53-group-mean-query"
            | "declarative-ja-en-v54-source-question-followup"
            | "declarative-ja-en-v55-natural-premise-question"
            | "declarative-ja-en-v56-natural-premise-correction"
            | "declarative-ja-en-v57-contextual-premise-correction"
            | "declarative-ja-en-v58-prior-result-pronoun-query"
            | "declarative-ja-en-v59-conjunctive-relation-question"
            | "declarative-ja-en-v60-conjunctive-relation-rule"
            | "declarative-ja-en-v61-three-clause-relation-question"
            | "declarative-ja-en-v62-constrained-source-summary"
            | "declarative-ja-en-v63-previous-output-edit"
            | "declarative-ja-en-v70-generic-transitive-verbs"
    ) && let Intent::VerifyPreviousCode { document } = &goal.intent
    {
        let previous_id = bundle.meaning.context_result_ref.as_ref().ok_or_else(|| {
            failure(
                "RECOVERY_CODE_CONTEXT_CHANGED",
                "the adopted verification has no retained prior-result context",
            )
        })?;
        let previous = state.prior_results.get(previous_id).ok_or_else(|| {
            failure(
                "RECOVERY_CODE_CONTEXT_CHANGED",
                "the adopted verification's prior result is no longer available",
            )
        })?;
        let (source_ref, source_report) =
            crate::code_generation::previous_code_report(store, actor, case, previous)?
                .ok_or_else(|| {
                    failure(
                        "RECOVERY_CODE_CONTEXT_CHANGED",
                        "the original generated-code report is no longer available unchanged",
                    )
                })?;
        let deadline = Instant::now().checked_add(Duration::from_millis(goal.budget.deadline_ms));
        let verification = crate::code_capability::verify_previous_source(
            &source_report.source,
            document,
            deadline,
        )?;
        let verification_bytes = canonical(&verification)?;
        let verification_ref = digest(&verification_bytes);
        let verification_dependencies =
            vec![previous.id.clone(), source_ref, verification_ref.clone()];
        let stored_verification = store.get_bytes(actor, case, &verification_ref)?;
        let expected_units =
            crate::code_capability::previous_code_verification_lines(&verification)?;
        if previous.lifecycle != Lifecycle::Commit
            || result.dependencies != verification_dependencies
            || result.guarantee != Guarantee::Computed
            || plan.required_units != expected_units
            || stored_verification != verification_bytes
        {
            return Err(failure(
                "RECOVERY_CODE_VERIFICATION_CHANGED",
                "the prior generated source or finite-case verification differs from the adopted result",
            ));
        }
    }
    if requires_generated_comparison {
        crate::core::verify_generated_model_artifacts(store, actor, case, plan)?;
    } else if generated_compute_ref.is_none() {
        let model_bytes = store.get_bytes(actor, case, &plan.model_artifact_ref)?;
        let model: language::SequenceModel = serde_json::from_slice(&model_bytes)?;
        model.validate()?;
        let emission_bytes = store.get_bytes(actor, case, &plan.model_observation_ref)?;
        let emission: language::ModelEmission = serde_json::from_slice(&emission_bytes)?;
        if requires_proof_explanation && emission.authorized_surfaces.is_none() {
            return Err(failure(
                "RECOVERY_MODEL_CANDIDATE_MISMATCH",
                "the sealed explanation Goal requires the original proof's approved surfaces",
            ));
        }
        let authorized_surfaces = match &emission.authorized_surfaces {
            Some(surfaces) => {
                let expected_surfaces = match (&goal.intent, result.calculation.as_ref()) {
                    (
                        Intent::ExplainPrevious
                        | Intent::ShowCalculationSequenceHistory
                        | Intent::ExplainCalculationSequenceStep { .. },
                        Some(_),
                    ) if explains_sequence => Some(vec![plan.required_units.join("\n")]),
                    (Intent::ExplainPrevious, Some(calculation)) => {
                        Some(language::calculation_explanation_surfaces(calculation))
                    }
                    (Intent::AskWhy { proposition }, None) => {
                        let fallback;
                        let (memory, answer) = if let Some(proof) = original_relation.as_ref() {
                            &proof.projection
                        } else {
                            fallback = query_recovery_snapshot(
                                state,
                                store.load_memory(actor)?,
                                proposition,
                                goal.budget.max_actions,
                            )?;
                            &fallback
                        };
                        let explanation_units = crate::core::relation_why_explanation_units(
                            memory,
                            proposition,
                            answer,
                        )
                        .map_err(|_| {
                            failure(
                                "RECOVERY_MODEL_CANDIDATE_MISMATCH",
                                "the current relation proof has no matching explanation surfaces",
                            )
                        })?;
                        if crate::relations::answer_dependencies(answer) != result.dependencies
                            || crate::relations::answer_guarantee(answer) != result.guarantee
                            || explanation_units != plan.required_units
                        {
                            return Err(failure(
                                "RECOVERY_MODEL_CANDIDATE_MISMATCH",
                                "the current proof lineage differs from the adopted explanation",
                            ));
                        }
                        Some(language::relation_explanation_surfaces(&explanation_units))
                    }
                    (Intent::DraftRelationProfile { subject }, None) => {
                        let memory = match result_memory_snapshot(store, actor, result)? {
                            Some(memory) => memory,
                            None => store.load_memory(actor)?,
                        };
                        let answer = crate::core::relation_profile_answer(
                            &memory,
                            subject,
                            goal.budget.max_actions,
                            goal.budget.max_evidence_bytes,
                        )
                        .map_err(|_| {
                            failure(
                                "RECOVERY_MODEL_CANDIDATE_MISMATCH",
                                "the retained actor-scoped relation inventory is unavailable",
                            )
                        })?;
                        if answer.dependencies != result.dependencies
                            || answer.guarantee != result.guarantee
                            || answer.units != plan.required_units
                        {
                            return Err(failure(
                                "RECOVERY_MODEL_CANDIDATE_MISMATCH",
                                "the retained relation inventory differs from the adopted profile draft",
                            ));
                        }
                        Some(language::relation_profile_surfaces(&answer.units))
                    }
                    _ => None,
                };
                if expected_surfaces.as_ref() != Some(surfaces) || !surfaces.contains(&plan.text) {
                    return Err(failure(
                        "RECOVERY_MODEL_CANDIDATE_MISMATCH",
                        "retained multi-surface output differs from the Core-approved current explanation",
                    ));
                }
                surfaces.clone()
            }
            // Legacy alpha 8 records contain no candidate list and can only prove
            // the exact singleton surface they already retained.
            None => vec![plan.text.clone()],
        };
        let verified_emission =
            model.realize_supported(&authorized_surfaces, request.budget.max_evidence_bytes)?;
        if canonical(&verified_emission)? != emission_bytes
            || emission.text != plan.text
            || emission.model_revision != plan.model_revision
        {
            return Err(failure(
                "RECOVERY_MODEL_MISMATCH",
                "retained native model observation does not reproduce its adopted surface",
            ));
        }
    }
    let nodes: Vec<GoalNode> =
        serde_json::from_slice(&store.get_bytes(actor, case, &goal.required_graph_ref)?)?;
    let web_answer = match &goal.intent {
        Intent::AskWeb { .. } => {
            verify_registered_input(store, bundle, "RECOVERY_WEB_ANSWER_MISMATCH")?;
            Some(verify_web_answer(store, bundle, result, plan, nodes.len())?.refs)
        }
        _ => None,
    };
    let search_listing = search::verify_retained_search_listing(store, bundle, result, plan)?
        .map(|proof| proof.proof_ref);
    // Older sealed search goals used a compute-only graph. They still
    // require the original listing to reproduce the adopted content.
    let search_requires_listing = matches!(goal.intent, Intent::SearchWithFallback { .. })
        || nodes
            .iter()
            .any(|node| node.id == search::SEARCH_LISTING_NODE);
    let search_candidate = if matches!(goal.intent, Intent::FetchPreviousSearchCandidate { .. }) {
        let previous = bundle
            .meaning
            .context_result_ref
            .as_ref()
            .and_then(|id| state.prior_results.get(id))
            .ok_or_else(|| {
                failure(
                    "RECOVERY_SEARCH_CONTEXT_MISMATCH",
                    "the original search predecessor is no longer retained",
                )
            })?;
        let retains_listing = nodes
            .iter()
            .any(|node| node.id == search::SEARCH_CANDIDATE_LISTING_NODE);
        Some(search::verify_retained_candidate_body(
            store,
            bundle,
            previous,
            result,
            plan,
            retains_listing,
            None,
        )?)
    } else {
        None
    };
    let mut expected_nodes = Vec::new();
    let mut sources = Vec::new();
    let direct_continuation = if matches!(goal.intent, Intent::FetchNextUnobservedSource) {
        let invalid = || {
            failure(
                "RECOVERY_SOURCE_CONTEXT_MISMATCH",
                "The continued read differs from its sealed direct retrieval",
            )
        };
        let previous = bundle
            .meaning
            .context_result_ref
            .as_ref()
            .and_then(|id| state.prior_results.get(id));
        let continuation =
            next_direct_retrieval(store, actor, bundle, previous, &state.prior_results, None)?
                .ok_or_else(invalid)?;
        let packet = verify_direct_continuation_body(
            continuation.source,
            result,
            request,
            &continuation.uri,
            &continuation.observed_routes,
        )?;
        let allowed_starts = (goal.budget.max_actions as usize)
            .checked_sub(nodes.len().saturating_sub(1))
            .ok_or_else(invalid)?;
        if packet.redirect_chain.len() > allowed_starts {
            return Err(invalid());
        }
        let reference = verified_artifact(store, actor, case, continuation.source)?;
        expected_nodes.push(GoalNode {
            id: PREVIOUS_DIRECT_RETRIEVAL_NODE.into(),
            kind: GoalNodeKind::Evidence,
            requires: Vec::new(),
        });
        Some((continuation.uri, reference))
    } else {
        None
    };
    let direct_urls = match &goal.intent {
        Intent::Retrieve { urls, .. } => Some(urls.as_slice()),
        Intent::FetchNextUnobservedSource => direct_continuation
            .as_ref()
            .map(|(uri, _)| std::slice::from_ref(uri)),
        _ => None,
    };
    if let Some(urls) = direct_urls {
        if urls.len() != result.evidence.len() {
            return Err(failure(
                "RECOVERY_EVIDENCE_MISMATCH",
                "required original evidence is incomplete",
            ));
        }
        for (index, uri) in urls.iter().enumerate() {
            let packet = &result.evidence[index];
            if packet.request_uri != *uri
                || packet.failure.is_some()
                || packet.retry_after.is_some()
                || packet.extracted_text.is_none()
                || packet.raw_sha256 != digest(&packet.raw_body)
            {
                return Err(failure(
                    "RECOVERY_EVIDENCE_MISMATCH",
                    "captured body identity or original evidence failed",
                ));
            }
            let id = format!("source-{index}");
            sources.push(id.clone());
            expected_nodes.push(GoalNode {
                id,
                kind: GoalNodeKind::Evidence,
                requires: direct_continuation
                    .is_some()
                    .then(|| PREVIOUS_DIRECT_RETRIEVAL_NODE.into())
                    .into_iter()
                    .collect(),
            });
        }
    } else if search_listing.is_some() {
        if search_requires_listing {
            let id = search::SEARCH_LISTING_NODE.to_string();
            sources.push(id.clone());
            expected_nodes.push(GoalNode {
                id,
                kind: GoalNodeKind::Evidence,
                requires: Vec::new(),
            });
        }
    } else if let Some(candidate) = &search_candidate {
        let mut requires = Vec::new();
        if candidate.listing_ref().is_some() {
            let id = search::SEARCH_CANDIDATE_LISTING_NODE.to_string();
            expected_nodes.push(GoalNode {
                id: id.clone(),
                kind: GoalNodeKind::Evidence,
                requires: Vec::new(),
            });
            requires.push(id);
        }
        sources.push(candidate.body_id.clone());
        expected_nodes.push(GoalNode {
            id: candidate.body_id.clone(),
            kind: GoalNodeKind::Evidence,
            requires,
        });
    } else if web_answer.is_some() {
        expected_nodes.extend(crate::web_answer::nodes());
        sources.push(crate::web_answer::ANSWER_NODE.into());
    } else if !result.evidence.is_empty() {
        return Err(failure(
            "RECOVERY_EVIDENCE_MISMATCH",
            "undeclared evidence cannot satisfy this original graph",
        ));
    }
    if let Some(summary) = &source_summary {
        for (index, _) in &summary.source_packet_refs {
            let id = format!("followup-source-{index}");
            sources.push(id.clone());
            expected_nodes.push(GoalNode {
                id,
                kind: GoalNodeKind::Evidence,
                requires: Vec::new(),
            });
        }
    }
    if previous_csv_statistics_explanation.is_some() {
        let id = "previous-csv-statistics".to_string();
        sources.push(id.clone());
        expected_nodes.push(GoalNode {
            id,
            kind: GoalNodeKind::Evidence,
            requires: Vec::new(),
        });
    }
    if previous_json_table_group_statistics_explanation.is_some() {
        let source_id = "previous-json-group-statistics-source".to_string();
        let report_id = "previous-json-group-statistics-report".to_string();
        expected_nodes.push(GoalNode {
            id: source_id.clone(),
            kind: GoalNodeKind::Evidence,
            requires: Vec::new(),
        });
        expected_nodes.push(GoalNode {
            id: report_id.clone(),
            kind: GoalNodeKind::ComputedValue,
            requires: vec![source_id],
        });
        sources.push(report_id);
    }
    if previous_json_table_operation_explanation.is_some() {
        let input_id = "previous-json-table-transform-input".to_string();
        let report_id = "previous-json-table-transform-report".to_string();
        expected_nodes.push(GoalNode {
            id: input_id.clone(),
            kind: GoalNodeKind::Evidence,
            requires: Vec::new(),
        });
        expected_nodes.push(GoalNode {
            id: report_id.clone(),
            kind: GoalNodeKind::ComputedValue,
            requires: vec![input_id],
        });
        sources.push(report_id);
    }
    if let Some(explanation) = &previous_document_explanation {
        let input_id = explanation.input_node.to_string();
        let report_id = explanation.report_node.to_string();
        expected_nodes.push(GoalNode {
            id: input_id.clone(),
            kind: GoalNodeKind::Evidence,
            requires: Vec::new(),
        });
        expected_nodes.push(GoalNode {
            id: report_id.clone(),
            kind: GoalNodeKind::ComputedValue,
            requires: vec![input_id],
        });
        sources.push(report_id);
    }
    if previous_json_group_mean_answer.is_some() {
        let source_id = "previous-json-group-statistics-source".to_string();
        let report_id = "previous-json-group-statistics-report".to_string();
        let answer_id = "previous-json-group-mean-answer".to_string();
        expected_nodes.push(GoalNode {
            id: source_id.clone(),
            kind: GoalNodeKind::Evidence,
            requires: Vec::new(),
        });
        expected_nodes.push(GoalNode {
            id: report_id.clone(),
            kind: GoalNodeKind::ComputedValue,
            requires: vec![source_id],
        });
        expected_nodes.push(GoalNode {
            id: answer_id.clone(),
            kind: GoalNodeKind::ComputedValue,
            requires: vec![report_id],
        });
        sources.push(answer_id);
    }
    if let Some(question) = &source_question {
        let mut source_ids = Vec::new();
        for (index, _) in &question.source_packet_refs {
            let source_id = format!("previous-question-source-{index}");
            expected_nodes.push(GoalNode {
                id: source_id.clone(),
                kind: GoalNodeKind::Evidence,
                requires: Vec::new(),
            });
            source_ids.push(source_id);
        }
        let answer_id = "previous-source-question-answer".to_string();
        expected_nodes.push(GoalNode {
            id: answer_id.clone(),
            kind: GoalNodeKind::ComputedValue,
            requires: source_ids,
        });
        sources.push(answer_id);
    }
    if previous_text_edit.is_some() {
        let content_id = "previous-text-content".to_string();
        let render_id = "previous-text-render".to_string();
        let edit_id = "previous-text-replacement".to_string();
        expected_nodes.push(GoalNode {
            id: content_id.clone(),
            kind: GoalNodeKind::Evidence,
            requires: Vec::new(),
        });
        expected_nodes.push(GoalNode {
            id: render_id.clone(),
            kind: GoalNodeKind::RenderedArtifact,
            requires: vec![content_id.clone()],
        });
        expected_nodes.push(GoalNode {
            id: edit_id.clone(),
            kind: GoalNodeKind::ComputedValue,
            requires: vec![content_id, render_id],
        });
        sources.push(edit_id);
    }
    if previous_result_summary.is_some() {
        let content_id = "previous-result-content".to_string();
        let render_id = "previous-result-render".to_string();
        let summary_id = "previous-result-summary".to_string();
        expected_nodes.push(GoalNode {
            id: content_id.clone(),
            kind: GoalNodeKind::Evidence,
            requires: Vec::new(),
        });
        expected_nodes.push(GoalNode {
            id: render_id.clone(),
            kind: GoalNodeKind::RenderedArtifact,
            requires: vec![content_id.clone()],
        });
        expected_nodes.push(GoalNode {
            id: summary_id.clone(),
            kind: GoalNodeKind::ComputedValue,
            requires: vec![content_id, render_id],
        });
        sources.push(summary_id);
    }
    let sequence_steps = sequence_edit
        .as_ref()
        .map(|context| context.steps.as_slice())
        .or_else(|| {
            previous_sequence
                .as_ref()
                .map(|context| context.effective_steps())
        })
        .or(match &goal.intent {
            Intent::CalculateSequence { steps } => Some(steps.as_slice()),
            _ => None,
        });
    if let Some(steps) = sequence_steps {
        let sequence_nodes = calculation_sequence_nodes(steps);
        sources.extend(sequence_nodes.iter().map(|node| node.id.clone()));
        expected_nodes.extend(sequence_nodes);
    }
    expected_nodes.extend([
        GoalNode {
            id: "compute".into(),
            kind: GoalNodeKind::CoreResult,
            requires: sources.clone(),
        },
        GoalNode {
            id: "content".into(),
            kind: GoalNodeKind::ContentPlan,
            requires: vec!["compute".into()],
        },
        GoalNode {
            id: "render".into(),
            kind: GoalNodeKind::RenderedArtifact,
            requires: vec!["content".into()],
        },
        GoalNode {
            id: "file".into(),
            kind: GoalNodeKind::RequiredFile,
            requires: vec!["render".into()],
        },
    ]);
    if nodes != expected_nodes {
        return Err(failure(
            "UNSUPPORTED_RECOVERY_GRAPH",
            "graph differs from this verifier's supported artifact topology",
        ));
    }
    if previous_sequence.as_ref().is_some_and(|context| {
        nodes.len() + context.context_actions() > goal.budget.max_actions as usize
    }) {
        return Err(failure(
            "ACTION_BUDGET_EXHAUSTED",
            "the original allowance does not cover the explanation history and required nodes",
        ));
    }
    if sequence_edit.as_ref().is_some_and(|context| {
        nodes.len() + context.verification_actions() > goal.budget.max_actions as usize
    }) {
        return Err(failure(
            "ACTION_BUDGET_EXHAUSTED",
            "the original allowance does not cover the sequence edit proofs and required nodes",
        ));
    }
    let mut graph = Scheduler::new(nodes, goal.budget.max_actions)?;
    let mut source_refs = BTreeMap::new();
    let verify_sequence_artifact = |expected: &[u8]| {
        let reference = digest(expected);
        if store.get_bytes(actor, case, &reference)? != expected {
            return Err(failure(
                "RECOVERY_ARTIFACT_MISMATCH",
                "retained calculation step, correction or report differs",
            ));
        }
        Ok(reference)
    };
    if let Some(context) = &sequence_edit {
        let mut dependencies = Vec::new();
        let observed = run_sequence_edit(
            store,
            bundle,
            context,
            &mut graph,
            edit_started,
            &mut dependencies,
            verify_sequence_artifact,
        )?;
        observed.verify_content(result, plan)?;
        if dependencies != result.dependencies
            || !result.evidence.is_empty()
            || result.text != plan.text
        {
            return Err(failure(
                "RECOVERY_DEPENDENCY_MISMATCH",
                "the result differs from the sealed sequence edit and its ordered proofs",
            ));
        }
        source_refs.clone_from(graph.observed());
    } else if let Some(context) = &previous_sequence {
        let explanation = calculation_sequence_report(
            store,
            context,
            &goal.intent,
            &mut graph,
            &goal.budget,
            sequence_started.unwrap_or_else(Instant::now),
        )?;
        if result.calculation.as_ref() != Some(&explanation.calculation)
            || result.guarantee != Guarantee::Computed
            || plan.guarantee != Guarantee::Computed
            || result.dependencies != explanation.dependencies
            || !result.evidence.is_empty()
            || plan.required_units != explanation.units
            || plan.text != explanation.units.join("\n")
            || result.text != plan.text
        {
            return Err(failure(
                "RECOVERY_CALCULATION_SEQUENCE_CONTEXT_INVALID",
                "the adopted explanation differs from the verified calculation sequence",
            ));
        }
        source_refs.clone_from(graph.observed());
    } else if let Intent::CalculateSequence { steps } = &goal.intent {
        let mut dependencies = Vec::new();
        let observed = run_calculation_sequence(
            steps,
            |_| &request.text,
            &mut graph,
            &goal.budget,
            Instant::now(),
            &mut dependencies,
            verify_sequence_artifact,
        )?;
        observed.verify_content(result, plan)?;
        if dependencies != result.dependencies {
            return Err(failure(
                "RECOVERY_DEPENDENCY_MISMATCH",
                "the result did not retain every required sequence step and exact report in order",
            ));
        }
        source_refs.clone_from(graph.observed());
    }
    if let Some(reference) = search_listing {
        if search_requires_listing {
            graph.observe(search::SEARCH_LISTING_NODE, &reference, &BTreeMap::new())?;
            source_refs.insert(search::SEARCH_LISTING_NODE.into(), reference);
        }
    } else if let Some(candidate) = &search_candidate {
        let mut requires = BTreeMap::new();
        if let Some(reference) = candidate.listing_ref() {
            let id = search::SEARCH_CANDIDATE_LISTING_NODE.to_string();
            graph.observe(&id, reference, &BTreeMap::new())?;
            requires.insert(id, reference.clone());
        }
        graph.observe(&candidate.body_id, &candidate.body_ref, &requires)?;
        source_refs.insert(candidate.body_id.clone(), candidate.body_ref.clone());
    } else if let Some([listing_ref, body_ref, answer_ref]) = web_answer {
        use crate::web_answer::{ANSWER_NODE, BODY_NODE, LISTING_NODE};

        graph.observe(LISTING_NODE, listing_ref, &BTreeMap::new())?;
        graph.observe(
            BODY_NODE,
            body_ref,
            &BTreeMap::from([(LISTING_NODE.into(), listing_ref.to_owned())]),
        )?;
        graph.observe(
            ANSWER_NODE,
            answer_ref,
            &BTreeMap::from([(BODY_NODE.into(), body_ref.to_owned())]),
        )?;
        source_refs.insert(ANSWER_NODE.into(), answer_ref.to_owned());
    } else {
        let mut source_requires = BTreeMap::new();
        if let Some((_, reference)) = direct_continuation {
            graph.observe(PREVIOUS_DIRECT_RETRIEVAL_NODE, &reference, &BTreeMap::new())?;
            source_requires.insert(PREVIOUS_DIRECT_RETRIEVAL_NODE.into(), reference);
        }
        for (index, packet) in result.evidence.iter().enumerate() {
            let reference = verified_artifact(store, actor, case, packet)?;
            let id = format!("source-{index}");
            graph.observe(&id, &reference, &source_requires)?;
            source_refs.insert(id, reference);
        }
    }
    if let Some(summary) = &source_summary {
        for (index, reference) in &summary.source_packet_refs {
            let id = format!("followup-source-{index}");
            graph.observe(&id, reference, &BTreeMap::new())?;
            source_refs.insert(id, reference.clone());
        }
    }
    if let Some(summary) = &previous_result_summary {
        graph.observe(
            "previous-result-content",
            &summary.content_ref,
            &BTreeMap::new(),
        )?;
        graph.observe(
            "previous-result-render",
            &summary.render_ref,
            &BTreeMap::from([(
                "previous-result-content".into(),
                summary.content_ref.clone(),
            )]),
        )?;
        graph.observe(
            "previous-result-summary",
            &summary.summary_ref,
            &BTreeMap::from([
                (
                    "previous-result-content".into(),
                    summary.content_ref.clone(),
                ),
                ("previous-result-render".into(), summary.render_ref.clone()),
            ]),
        )?;
        source_refs.insert(
            "previous-result-summary".into(),
            summary.summary_ref.clone(),
        );
    }
    if let Some((report_ref, _)) = &previous_csv_statistics_explanation {
        let id = "previous-csv-statistics".to_string();
        graph.observe(&id, report_ref, &BTreeMap::new())?;
        source_refs.insert(id, report_ref.clone());
    }
    if let Some((source_ref, report_ref, _)) = &previous_json_table_group_statistics_explanation {
        let source_id = "previous-json-group-statistics-source".to_string();
        let report_id = "previous-json-group-statistics-report".to_string();
        graph.observe(&source_id, source_ref, &BTreeMap::new())?;
        graph.observe(
            &report_id,
            report_ref,
            &BTreeMap::from([(source_id, source_ref.clone())]),
        )?;
        source_refs.insert(report_id, report_ref.clone());
    }
    if let Some((bundle_ref, report_ref, _)) = &previous_json_table_operation_explanation {
        let input_id = "previous-json-table-transform-input".to_string();
        let report_id = "previous-json-table-transform-report".to_string();
        graph.observe(&input_id, bundle_ref, &BTreeMap::new())?;
        graph.observe(
            &report_id,
            report_ref,
            &BTreeMap::from([(input_id, bundle_ref.clone())]),
        )?;
        source_refs.insert(report_id, report_ref.clone());
    }
    if let Some(explanation) = &previous_document_explanation {
        graph.observe(
            explanation.input_node,
            &explanation.bundle_ref,
            &BTreeMap::new(),
        )?;
        graph.observe(
            explanation.report_node,
            &explanation.report_ref,
            &BTreeMap::from([(
                explanation.input_node.into(),
                explanation.bundle_ref.clone(),
            )]),
        )?;
        source_refs.insert(
            explanation.report_node.into(),
            explanation.report_ref.clone(),
        );
    }
    if let Some((_, source_ref, report_ref, answer_ref, _, _)) = &previous_json_group_mean_answer {
        let source_id = "previous-json-group-statistics-source".to_string();
        let report_id = "previous-json-group-statistics-report".to_string();
        let answer_id = "previous-json-group-mean-answer".to_string();
        graph.observe(&source_id, source_ref, &BTreeMap::new())?;
        graph.observe(
            &report_id,
            report_ref,
            &BTreeMap::from([(source_id.clone(), source_ref.clone())]),
        )?;
        graph.observe(
            &answer_id,
            answer_ref,
            &BTreeMap::from([(report_id.clone(), report_ref.clone())]),
        )?;
        source_refs.insert(answer_id, answer_ref.clone());
    }
    if let Some(question) = &source_question {
        let answer_id = "previous-source-question-answer".to_string();
        let mut answer_inputs = BTreeMap::new();
        for (index, reference) in &question.source_packet_refs {
            let source_id = format!("previous-question-source-{index}");
            graph.observe(&source_id, reference, &BTreeMap::new())?;
            answer_inputs.insert(source_id, reference.clone());
        }
        graph.observe(&answer_id, &question.answer_ref, &answer_inputs)?;
        source_refs.insert(answer_id, question.answer_ref.clone());
    }
    if let Some(edit) = &previous_text_edit {
        let content_id = "previous-text-content".to_string();
        let render_id = "previous-text-render".to_string();
        let edit_id = "previous-text-replacement".to_string();
        graph.observe(&content_id, &edit.source_content_ref, &BTreeMap::new())?;
        graph.observe(
            &render_id,
            &edit.source_render_ref,
            &BTreeMap::from([(content_id.clone(), edit.source_content_ref.clone())]),
        )?;
        graph.observe(
            &edit_id,
            &edit.edit_ref,
            &BTreeMap::from([
                (content_id, edit.source_content_ref.clone()),
                (render_id, edit.source_render_ref.clone()),
            ]),
        )?;
        source_refs.insert(edit_id, edit.edit_ref.clone());
    }
    let compute_ref = match generated_compute_ref {
        Some(reference) => reference,
        None => verified_artifact(
            store,
            actor,
            case,
            &(
                &result.calculation,
                &plan.required_units,
                &result.evidence,
                &result.dependencies,
            ),
        )?,
    };
    let mut dependencies = result.dependencies.clone();
    dependencies.push(compute_ref.clone());
    if plan.dependencies != dependencies {
        return Err(failure(
            "RECOVERY_DEPENDENCY_MISMATCH",
            "content did not retain the actual compute artifact",
        ));
    }
    graph.observe("compute", &compute_ref, &source_refs)?;
    let content_ref = verified_artifact(store, actor, case, plan)?;
    graph.observe(
        "content",
        &content_ref,
        &BTreeMap::from([("compute".into(), compute_ref)]),
    )?;
    let render_ref = verified_artifact(store, actor, case, render)?;
    graph.observe(
        "render",
        &render_ref,
        &BTreeMap::from([("content".into(), content_ref)]),
    )?;
    if !graph.ready().iter().any(|id| id == "file") {
        return Err(failure(
            "ACTION_BUDGET_EXHAUSTED",
            "original graph leaves no confirmed-file completion allowance",
        ));
    }
    let expected_action = identity(&(case, &fingerprint, "file", path, &render.actual_sha256))?;
    let actions: Vec<_> = store
        .actions(actor, case)?
        .into_iter()
        .filter(|action| action.input_fingerprint == fingerprint)
        .collect();
    if actions.len() != 1 {
        return Err(failure(
            "RECOVERY_ACTION_UNAVAILABLE",
            "exactly one original local effect identity is required",
        ));
    }
    let action = actions.into_iter().next().expect("one action was checked");
    if action.actor_id != actor
        || action.case_id != *case
        || action.effect_class != "LOCAL_WRITE"
        || action.action_id != expected_action
        || action.idempotency_key != expected_action
        || Some(action.target.as_str()) != path.to_str()
        || action.exact_request_sha256 != render.actual_sha256
        || !matches!(action.phase, EffectPhase::Unknown | EffectPhase::Confirmed)
    {
        return Err(failure(
            "RECOVERY_ACTION_MISMATCH",
            "original attempted identity/target/phase is not reconcilable",
        ));
    }
    let authorization: Permissions =
        serde_json::from_slice(&store.get_bytes(actor, case, &action.authorization_sha256)?)?;
    let exact = store.get_bytes(actor, case, &action.exact_request_sha256)?;
    if authorization != goal.permissions || exact != render.bytes {
        return Err(failure(
            "RECOVERY_AUTHORITY_MISMATCH",
            "original authorization or exact effect bytes differ",
        ));
    }
    observe_file(path, &exact)?;
    let receipt = if action.phase == EffectPhase::Confirmed {
        let receipt = action.receipt.clone().ok_or_else(|| {
            failure(
                "RECOVERY_RECEIPT_MISSING",
                "confirmed action has no original receipt",
            )
        })?;
        if receipt.action_id != action.action_id
            || receipt.target != action.target
            || receipt.phase != EffectPhase::Confirmed
            || receipt.content_sha256 != digest(&exact)
            || receipt.observed_bytes != Some(exact.len() as u64)
        {
            return Err(failure(
                "RECOVERY_RECEIPT_MISMATCH",
                "stored receipt does not match observed exact artifact",
            ));
        }
        receipt
    } else {
        DeliveryReceipt {
            action_id: action.action_id.clone(),
            target: action.target.clone(),
            content_sha256: digest(&exact),
            phase: EffectPhase::Confirmed,
            observed_bytes: Some(exact.len() as u64),
            reason: Some("LOCAL_FILE_RECONCILED_NO_RESEND".into()),
        }
    };
    Ok((action, receipt, graph, render_ref, explains_sequence))
}

pub(crate) struct RetainedWebAnswer<'a> {
    pub(crate) refs: [&'a str; 3],
    pub(crate) candidate: search::SearchCandidate,
    pub(crate) answer: crate::web_answer::WebAnswer,
}

pub(crate) fn verify_web_answer<'a>(
    store: &Store,
    bundle: &InputBundle,
    result: &'a ResultRecord,
    plan: &ContentPlan,
    graph_nodes: usize,
) -> Result<RetainedWebAnswer<'a>> {
    const CODE: &str = "RECOVERY_WEB_ANSWER_MISMATCH";
    let invalid = || {
        failure(
            CODE,
            "the retained search, body or answer differs from the original web goal",
        )
    };
    let request = &bundle.request;
    let actor = &request.actor_id;
    let case = &request.case_id;
    let Intent::AskWeb {
        topic,
        elevation_in_kilometres,
    } = &bundle.meaning.intent
    else {
        return Err(invalid());
    };
    let [listing_ref, body_ref, answer_ref] = result.dependencies.as_slice() else {
        return Err(invalid());
    };
    let [listing, body] = result.evidence.as_slice() else {
        return Err(invalid());
    };
    if result.id != identity(request)?
        || result.input_id != request.input_id
        || result.input_version != request.input_version
        || result.calculation.is_some()
        || result.guarantee != Guarantee::SourceReport
        || plan.guarantee != result.guarantee
        || plan.locale != request.locale
        || result.text != plan.text
    {
        return Err(invalid());
    }
    let listing_bytes = store.get_bytes(actor, case, listing_ref)?;
    let observation: search::SearchObservation = serde_json::from_slice(&listing_bytes)?;
    if observation.query != *topic
        || observation.listing != *listing
        || canonical(&observation)? != listing_bytes
        || verified_artifact(store, actor, case, body)? != *body_ref
    {
        return Err(invalid());
    }
    // The original scheduler reserves non-network nodes and shares the
    // remaining start and byte allowance across the two live reads.
    let non_read_nodes = graph_nodes.checked_sub(2).ok_or_else(invalid)?;
    let allowed_starts = (request.budget.max_actions as usize)
        .checked_sub(non_read_nodes)
        .ok_or_else(invalid)?;
    if listing.redirect_chain.len() + body.redirect_chain.len() > allowed_starts
        || listing.raw_body.len().saturating_add(body.raw_body.len())
            > request.budget.max_evidence_bytes
    {
        return Err(invalid());
    }
    let candidate = crate::web_answer::select_candidate(&observation, &request.permissions)?;
    let answer = crate::web_answer::answer(
        topic,
        *elevation_in_kilometres,
        candidate,
        body,
        &request.permissions,
    )?;
    let answer_bytes = canonical(&answer)?;
    let units = answer.units();
    let text = units.join("\n");
    if answer_bytes.len().saturating_add(text.len()) > request.budget.max_evidence_bytes
        || store.get_bytes(actor, case, answer_ref)? != answer_bytes
        || plan.required_units != units
        || plan.text != text
    {
        return Err(invalid());
    }
    Ok(RetainedWebAnswer {
        refs: [listing_ref, body_ref, answer_ref],
        candidate: candidate.clone(),
        answer,
    })
}

fn verify_previous_assertion_explanation(
    store: &Store,
    state: &CaseRevision,
    bundle: &InputBundle,
    result: &ResultRecord,
    plan: &ContentPlan,
) -> Result<()> {
    const CODE: &str = "RECOVERY_ASSERTION_EXPLANATION_MISMATCH";
    let invalid = || {
        failure(
            CODE,
            "The original assertion, memory revision or explanation changed.",
        )
    };
    let request = &bundle.request;
    let prior = bundle
        .meaning
        .context_result_ref
        .as_ref()
        .and_then(|id| state.prior_results.get(id))
        .ok_or_else(invalid)?;
    verify_registered_input(store, bundle, CODE)?;
    let assertion =
        crate::core::previous_assertion_context(store, &request.actor_id, &request.case_id, prior)?
            .ok_or_else(invalid)?;
    // Recheck the state at adoption, without changing an attempted effect's
    // original input or treating the current memory revision as its source.
    let revision = result.actor_memory_revision.ok_or_else(invalid)?;
    let memory = store
        .load_memory_revision(&request.actor_id, revision)?
        .ok_or_else(invalid)?;
    let expected = assertion.explain(&memory)?;
    if result.id != identity(request)?
        || result.input_id != request.input_id
        || result.input_version != request.input_version
        || result.guarantee != Guarantee::UserPremise
        || result.calculation.is_some()
        || !result.evidence.is_empty()
        || result.dependencies != expected.dependencies
        || plan.guarantee != result.guarantee
        || plan.required_units != expected.units
        || plan.text != expected.units.join("\n")
        || result.text != plan.text
    {
        return Err(invalid());
    }
    Ok(())
}

fn verified_artifact<T: serde::Serialize>(
    store: &Store,
    actor: &str,
    case: &str,
    value: &T,
) -> Result<String> {
    let expected = canonical(value)?;
    let reference = digest(&expected);
    if store.get_bytes(actor, case, &reference)? != expected {
        return Err(failure(
            "RECOVERY_ARTIFACT_MISMATCH",
            "retained actual artifact differs",
        ));
    }
    Ok(reference)
}

fn observe_file(path: &Path, expected: &[u8]) -> Result<()> {
    crate::durability::validate_local_target_path(path).map_err(|_| {
        failure(
            "UNSAFE_RECONCILIATION_TARGET",
            "original target is not an unambiguous absolute path",
        )
    })?;
    for ancestor in crate::durability::local_target_and_ancestors(path) {
        let metadata = fs::symlink_metadata(ancestor)?;
        if crate::durability::is_link_or_reparse_point(&metadata)
            || (ancestor != path && !metadata.is_dir())
            || (ancestor == path && !metadata.is_file())
        {
            return Err(failure(
                "UNSAFE_RECONCILIATION_TARGET",
                if cfg!(windows) {
                    "reparse point or non-regular original target"
                } else {
                    "symlink or non-regular original target"
                },
            ));
        }
    }
    if !crate::durability::keeps_local_target_name(path)? {
        return Err(failure(
            "UNSAFE_RECONCILIATION_TARGET",
            "Windows paths must identify the original local drive and file names",
        ));
    }
    let parent = path.parent().ok_or_else(|| {
        failure(
            "UNSAFE_RECONCILIATION_TARGET",
            "original parent unavailable",
        )
    })?;
    let parent_identity = parent.canonicalize()?;
    let mut file = crate::durability::open_file_for_sync(path)?;
    if file.metadata()?.len() != expected.len() as u64 {
        return Err(failure(
            "LOCAL_EFFECT_UNCONFIRMED",
            "existing file has different bytes",
        ));
    }
    let mut actual = Vec::new();
    Read::by_ref(&mut file)
        .take(expected.len() as u64 + 1)
        .read_to_end(&mut actual)?;
    if actual != expected || parent.canonicalize()? != parent_identity {
        return Err(failure(
            "LOCAL_EFFECT_UNCONFIRMED",
            "existing file or parent identity changed",
        ));
    }
    // Confirm durability of the existing object; never create or rewrite it.
    file.sync_all()?;
    crate::durability::sync_directory(parent)?;
    Ok(())
}

fn advance(
    store: &Store,
    state: &mut CaseRevision,
    result: &ResultRecord,
    next: Lifecycle,
) -> Result<()> {
    let mut revision = state.clone();
    // A migrated journal keeps its older revisions. Only the new recovery
    // revision uses the current store schema.
    revision.schema_version = SCHEMA_VERSION.into();
    revision.parent_revision = Some(state.revision);
    revision.revision += 1;
    revision.lifecycle = next;
    let mut observed = result.clone();
    observed.lifecycle = next;
    revision.reason = observed.reason.clone();
    revision.result = Some(observed);
    store.append_case(&state.actor_id, &revision)?;
    *state = revision;
    Ok(())
}
fn suspend(
    store: &Store,
    state: &mut CaseRevision,
    mut result: ResultRecord,
    reason: &str,
) -> Result<ResultRecord> {
    result.reason = Some(format!("LOCAL_RECONCILIATION_UNCONFIRMED: {reason}"));
    if state.lifecycle == Lifecycle::Suspend {
        if state.goal_sha256.is_none() || state.bundle.is_none() {
            result.lifecycle = Lifecycle::Suspend;
            return Ok(result);
        }
        advance(store, state, &result, Lifecycle::Running)?;
    }
    advance(store, state, &result, Lifecycle::Suspend)?;
    state
        .result
        .clone()
        .ok_or_else(|| failure("INTEGRITY_FAILURE", "suspension lost result"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::Engine;
    use rusqlite::{Connection, params};
    use std::{
        collections::BTreeSet,
        path::PathBuf,
        time::{SystemTime, UNIX_EPOCH},
    };

    struct Fixture {
        directory: PathBuf,
        state: PathBuf,
        target: PathBuf,
        token: String,
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.directory);
        }
    }

    fn empty_fixture() -> Fixture {
        let directory = std::env::temp_dir().join(format!(
            "minidora-recovery-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir(&directory).unwrap();
        Fixture {
            state: directory.join("state"),
            target: directory.join("actual-report.txt"),
            directory,
            token: String::new(),
        }
    }

    #[test]
    fn scoped_quotes_cannot_supply_relation_premises_or_rules() {
        let fixture = empty_fixture();
        let mut engine = Engine::open(&fixture.state).unwrap();
        let execute = |engine: &mut Engine, id: &str, text: &str| {
            engine
                .execute("alice", RequestEnvelope::new("case", id, "alice", text))
                .unwrap()
        };
        for (id, text) in [
            ("scope-quoted-premise", "[scope=lab]「Nera is a sensor」"),
            ("scope-quoted-japanese", "[scope=lab]『ネラは測定器です』"),
            (
                "quoted-rule-premise",
                "If [scope=lab]「Nera is a sensor」 then [scope=lab]Nera is reliable",
            ),
            (
                "quoted-rule-conclusion",
                "If [scope=lab]Nera is a sensor then [scope=lab]「Nera is reliable」",
            ),
            ("quoted-query", "なぜ[scope=lab]「ネラは測定器」ですか"),
        ] {
            let mut request = RequestEnvelope::new("case", id, "alice", text);
            request.output_path = Some(fixture.target.clone());
            request
                .permissions
                .write_paths
                .insert(fixture.target.clone());
            let result = engine.execute("alice", request).unwrap();
            assert_eq!(result.lifecycle, Lifecycle::Suspend, "{text}: {result:?}");
            assert!(result.content_plan.is_none() && result.receipts.is_empty());
            assert!(result.dependencies.is_empty());
            let sealed = engine
                .latest("alice", "case")
                .unwrap()
                .unwrap()
                .bundle
                .unwrap();
            assert_eq!(sealed.request.text, text);
            assert!(matches!(sealed.meaning.intent, Intent::Unknown { .. }));
        }
        assert_eq!(
            engine.store.load_memory("alice").unwrap(),
            MemoryState::default()
        );
        let rule = execute(
            &mut engine,
            "rule",
            "If [scope=lab]X is a sensor then [scope=lab]X is reliable",
        );
        assert_eq!(rule.lifecycle, Lifecycle::Commit);
        let question = "Why [scope=lab]is Nera reliable?";
        let before = execute(&mut engine, "before", question);
        assert_eq!(before.lifecycle, Lifecycle::Suspend, "{before:?}");
        assert!(before.content_plan.is_none());
        let premise = execute(&mut engine, "premise", "[scope=lab]Nera is a sensor");
        assert_eq!(premise.guarantee, Guarantee::UserPremise);
        let after = execute(&mut engine, "after", question);
        assert_eq!(after.lifecycle, Lifecycle::Commit, "{after:?}");
        assert_eq!(after.guarantee, Guarantee::Derived);
        assert!(after.dependencies.contains(&premise.dependencies[0]));
        assert!(after.dependencies.contains(&rule.dependencies[0]));
        assert!(after.render.is_some());
        assert!(!fixture.target.exists());
        assert!(engine.store.actions("alice", "case").unwrap().is_empty());
    }

    #[test]
    fn live_relation_query_reconciles_its_pinned_proof_lineage() {
        let fixture = empty_fixture();
        let mut engine = Engine::open(&fixture.state).unwrap();
        let mut roots = Vec::new();
        for (id, text) in [
            ("premise-a", "[scope=lab]ネラは測定器です"),
            ("premise-b", "[scope=lab]ネラは校正済みです"),
            (
                "rule",
                "もし[scope=lab]Xは測定器かつ校正済みなら[scope=lab]Xは使用可能",
            ),
        ] {
            let result = engine
                .execute("alice", RequestEnvelope::new("case", id, "alice", text))
                .unwrap();
            assert_eq!(result.lifecycle, Lifecycle::Commit, "{result:?}");
            roots.push(result.dependencies[0].clone());
        }
        let mut request = RequestEnvelope::new(
            "case",
            "relation-query",
            "alice",
            "[scope=lab]ネラは使用可能ですか",
        );
        request.output_path = Some(fixture.target.clone());
        request
            .permissions
            .write_paths
            .insert(fixture.target.clone());
        let result = engine.execute("alice", request).unwrap();
        assert_eq!(result.lifecycle, Lifecycle::Commit, "{result:?}");
        assert_eq!(result.guarantee, Guarantee::Derived);
        assert!(result.actor_memory_revision.is_some());
        assert!(roots.iter().all(|root| result.dependencies.contains(root)));
        let bytes = fs::read(&fixture.target).unwrap();
        let modified = fs::metadata(&fixture.target).unwrap().modified().unwrap();
        drop(engine);

        let token = retain_attempted_prefix(&fixture.state);
        let mut restored = Engine::open(&fixture.state).unwrap();
        let replayed = restored.reconcile("alice", "case").unwrap();
        assert_eq!(replayed.lifecycle, Lifecycle::Commit, "{replayed:?}");
        assert_eq!(replayed.dependencies, result.dependencies);
        assert_eq!(replayed.receipts[0].action_id, token);
        assert_eq!(fs::read(&fixture.target).unwrap(), bytes);
        assert_eq!(
            fs::metadata(&fixture.target).unwrap().modified().unwrap(),
            modified
        );
    }

    #[test]
    fn group_mean_hypothesis_survives_restart_without_becoming_a_premise() {
        let fixture = empty_fixture();
        let mut engine = Engine::open(&fixture.state).unwrap();
        let retain = "前回のグループ別統計から仮説を保持して";
        let explain = "前回の仮説を説明して";
        let statistics =
            |table: &str| format!("JSON表の「班」ごとに「値」の統計量を計算して:\n{table}",);
        let execute = |engine: &mut Engine, id: &str, text: &str| {
            engine
                .execute("alice", RequestEnvelope::new("case", id, "alice", text))
                .unwrap()
        };
        let missing = execute(&mut engine, "missing", retain);
        assert_eq!(missing.lifecycle, Lifecycle::Suspend);
        assert!(
            missing
                .reason
                .unwrap()
                .starts_with("GROUP_MEAN_HYPOTHESIS_CONTEXT_REQUIRED:")
        );
        let sample = statistics(
            r#"{"columns":["班","値"],"rows":[["甲","1"],["甲","5"],["乙","9"],["乙","7"]]}"#,
        );
        let observed = execute(&mut engine, "observed", &sample);
        assert_eq!(observed.lifecycle, Lifecycle::Commit, "{observed:?}");
        assert_eq!(observed.guarantee, Guarantee::Computed);
        let retention = execute(&mut engine, "retain", retain);
        assert_eq!(retention.lifecycle, Lifecycle::Commit, "{retention:?}");
        assert_eq!(retention.guarantee, Guarantee::Hypothesis);
        assert_eq!(retention.dependencies[0], observed.id);
        assert_eq!(&retention.dependencies[1..3], observed.dependencies);
        let reference = retention.dependencies.last().unwrap();
        let saved = engine.store.get_bytes("alice", "case", reference).unwrap();
        let candidate: serde_json::Value = serde_json::from_slice(&saved).unwrap();
        assert_eq!(candidate["adoption_state"], "HYPOTHESIS_RETAINED");
        assert_eq!(candidate["candidate_group"], "乙");
        assert_eq!(candidate["sample"]["winners"][0]["mean"], "8");
        assert_eq!(candidate["sample"]["winners"][0]["source_row_count"], 2);
        assert_eq!(
            candidate["sample"]["source_sha256"],
            observed.dependencies[0]
        );
        assert_eq!(candidate["statistics_sha256"], observed.dependencies[1]);
        for unknown in ["population", "observed_at", "empirical_contract_ref"] {
            assert!(candidate[unknown].is_null());
        }
        assert!(
            candidate["counterexample_refs"]
                .as_array()
                .unwrap()
                .is_empty()
        );
        assert_eq!(
            engine.store.load_memory("alice").unwrap(),
            MemoryState::default()
        );
        drop(engine);

        let mut engine = Engine::open(&fixture.state).unwrap();
        let mut request = RequestEnvelope::new("case", "explain", "alice", explain);
        request.output_path = Some(fixture.target.clone());
        request
            .permissions
            .write_paths
            .insert(fixture.target.clone());
        let explanation = engine.execute("alice", request).unwrap();
        assert_eq!(explanation.lifecycle, Lifecycle::Commit, "{explanation:?}");
        assert_eq!(explanation.guarantee, Guarantee::Hypothesis);
        assert_eq!(explanation.dependencies[0], retention.id);
        assert_eq!(&explanation.dependencies[1..], retention.dependencies);
        assert!(explanation.text.contains("HYPOTHESIS_RETAINED"));
        assert!(explanation.text.contains("反例"));
        let bytes = fs::read(&fixture.target).unwrap();
        assert_eq!(bytes, explanation.text.as_bytes());
        let modified = fs::metadata(&fixture.target).unwrap().modified().unwrap();
        drop(engine);

        let token = retain_attempted_prefix(&fixture.state);
        let mut engine = Engine::open(&fixture.state).unwrap();
        let recovered = engine.reconcile("alice", "case").unwrap();
        assert_eq!(recovered.lifecycle, Lifecycle::Commit, "{recovered:?}");
        assert_eq!(recovered.guarantee, Guarantee::Hypothesis);
        assert_eq!(recovered.receipts[0].action_id, token);
        assert_eq!(recovered.dependencies, explanation.dependencies);
        assert_eq!(fs::read(&fixture.target).unwrap(), bytes);
        assert_eq!(
            fs::metadata(&fixture.target).unwrap().modified().unwrap(),
            modified
        );
        assert_eq!(
            engine.store.get_bytes("alice", "case", reference).unwrap(),
            saved
        );

        let ordinary = execute(&mut engine, "fact", "乙は最高ですか");
        assert_eq!(ordinary.lifecycle, Lifecycle::Suspend);
        assert!(ordinary.content_plan.is_none());
        let other = engine
            .execute(
                "bob",
                RequestEnvelope::new("other", "explain", "bob", explain),
            )
            .unwrap();
        assert_eq!(other.lifecycle, Lifecycle::Suspend);
        assert!(
            other
                .reason
                .unwrap()
                .starts_with("GROUP_MEAN_HYPOTHESIS_CONTEXT_REQUIRED:")
        );
        assert_eq!(
            engine.store.load_memory("alice").unwrap(),
            MemoryState::default()
        );
    }

    #[test]
    fn group_mean_hypothesis_assessment_records_a_tie_and_reconciles_its_report() {
        let fixture = empty_fixture();
        let mut engine = Engine::open(&fixture.state).unwrap();
        let execute = |engine: &mut Engine, id: &str, text: &str| {
            engine
                .execute("alice", RequestEnvelope::new("case", id, "alice", text))
                .unwrap()
        };
        let observed = execute(
            &mut engine,
            "sample",
            "JSON表の「班」ごとに「値」の統計量を計算して:\n{\"columns\":[\"班\",\"値\"],\"rows\":[[\"甲\",\"1\"],[\"甲\",\"5\"],[\"乙\",\"9\"],[\"乙\",\"7\"]]}",
        );
        assert_eq!(observed.lifecycle, Lifecycle::Commit);
        let retained = execute(
            &mut engine,
            "retain",
            "前回のグループ別統計から仮説を保持して",
        );
        assert_eq!(retained.lifecycle, Lifecycle::Commit);
        let hypothesis_ref = retained.dependencies.last().unwrap();
        let hypothesis_bytes = engine
            .store
            .get_bytes("alice", "case", hypothesis_ref)
            .unwrap();
        let explained = execute(&mut engine, "explain-before", "前回の仮説を説明して");
        assert_eq!(explained.lifecycle, Lifecycle::Commit);
        drop(engine);

        let mut engine = Engine::open(&fixture.state).unwrap();
        let matched = execute(
            &mut engine,
            "matched",
            "前回の仮説を次の標本で照合して:\n{\"columns\":[\"値\",\"班\"],\"rows\":[[\"10\",\"乙\"],[\"6\",\"甲\"]]}",
        );
        assert_eq!(matched.lifecycle, Lifecycle::Commit, "{matched:?}");
        assert_eq!(matched.guarantee, Guarantee::Hypothesis);
        assert!(matched.text.contains("MATCHED_ON_SAMPLE"));
        let matching_record: serde_json::Value = serde_json::from_slice(
            &engine
                .store
                .get_bytes("alice", "case", matched.dependencies.last().unwrap())
                .unwrap(),
        )
        .unwrap();
        assert!(
            matching_record["counterexample_refs"]
                .as_array()
                .unwrap()
                .is_empty()
        );

        let second_sample = r#"{"columns":["班","値"],"rows":[["甲","17/2"],["乙","8.5"]]}"#;
        let mut request = RequestEnvelope::new(
            "case",
            "counterexample",
            "alice",
            &format!("前回の仮説を次の標本で照合して:\n{second_sample}"),
        );
        request.output_path = Some(fixture.target.clone());
        request
            .permissions
            .write_paths
            .insert(fixture.target.clone());
        let assessed = engine.execute("alice", request).unwrap();
        assert_eq!(assessed.lifecycle, Lifecycle::Commit, "{assessed:?}");
        assert_eq!(assessed.guarantee, Guarantee::Hypothesis);
        assert!(assessed.text.contains("COUNTEREXAMPLE_ON_SAMPLE"));
        assert_eq!(assessed.dependencies[0], retained.id);
        assert_eq!(&assessed.dependencies[1..5], retained.dependencies);
        assert_eq!(assessed.dependencies[5], digest(second_sample.as_bytes()));
        let assessment_ref = assessed.dependencies.last().unwrap();
        let record: serde_json::Value = serde_json::from_slice(
            &engine
                .store
                .get_bytes("alice", "case", assessment_ref)
                .unwrap(),
        )
        .unwrap();
        assert_eq!(record["hypothesis_sha256"], *hypothesis_ref);
        assert_eq!(record["adoption_state"], "HYPOTHESIS_RETAINED");
        assert_eq!(record["outcome"], "COUNTEREXAMPLE_ON_SAMPLE");
        assert_eq!(record["sample"]["winners"].as_array().unwrap().len(), 2);
        assert_eq!(record["sample"]["winners"][0]["mean"], "17/2");
        assert_eq!(
            record["counterexample_refs"],
            serde_json::json!([assessed.dependencies[6]])
        );

        let actual_bytes = fs::read(&fixture.target).unwrap();
        assert_eq!(actual_bytes, assessed.text.as_bytes());
        let modified = fs::metadata(&fixture.target).unwrap().modified().unwrap();
        let state = engine.latest("alice", "case").unwrap().unwrap();
        let mut goal: GoalContract = serde_json::from_slice(
            &engine
                .store
                .get_bytes("alice", "case", state.goal_sha256.as_ref().unwrap())
                .unwrap(),
        )
        .unwrap();
        assert_eq!(
            goal.required_predicates,
            [
                "GROUP_MEAN_HYPOTHESIS_RESULT_V1",
                "SUPPORTED_CONTENT",
                "EXACT_RENDER",
                "CONFIRMED_FILE"
            ]
        );
        let mut legacy = state.clone();
        goal.required_predicates.remove(0);
        legacy.goal_sha256 = Some(engine.store.put_artifact("alice", "case", &goal).unwrap());
        inspect(&engine.store, "alice", &legacy, &assessed).unwrap();
        let mut altered = assessed.clone();
        *altered.dependencies.last_mut().unwrap() = matched.dependencies.last().unwrap().clone();
        let error = inspect(&engine.store, "alice", &state, &altered).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("RECOVERY_GROUP_MEAN_HYPOTHESIS_CHANGED"),
            "{error:?}"
        );
        drop(engine);

        let token = retain_attempted_prefix(&fixture.state);
        let mut engine = Engine::open(&fixture.state).unwrap();
        let recovered = engine.reconcile("alice", "case").unwrap();
        assert_eq!(recovered.lifecycle, Lifecycle::Commit, "{recovered:?}");
        assert_eq!(recovered.receipts[0].action_id, token);
        assert_eq!(recovered.dependencies, assessed.dependencies);
        assert_eq!(fs::read(&fixture.target).unwrap(), actual_bytes);
        assert_eq!(
            fs::metadata(&fixture.target).unwrap().modified().unwrap(),
            modified
        );
        let explanation = execute(&mut engine, "explain-after", "前回の仮説を説明して");
        assert_eq!(explanation.lifecycle, Lifecycle::Commit, "{explanation:?}");
        assert_eq!(explanation.text, recovered.text);
        assert_eq!(explanation.dependencies[1], assessed.id);
        assert_eq!(
            engine
                .store
                .get_bytes("alice", "case", hypothesis_ref)
                .unwrap(),
            hypothesis_bytes
        );
        assert_eq!(
            engine.store.load_memory("alice").unwrap(),
            MemoryState::default()
        );
        assert_eq!(engine.store.actions("alice", "case").unwrap().len(), 1);
    }

    #[test]
    fn group_mean_hypothesis_assessment_requires_distinct_in_scope_observations_and_authority() {
        let fixture = empty_fixture();
        let mut engine = Engine::open(&fixture.state).unwrap();
        let observed = engine.execute(
            "alice",
            RequestEnvelope::new("case", "sample", "alice",
                "JSON表の「班」ごとに「値」の統計量を計算して:\n{\"columns\":[\"班\",\"値\"],\"rows\":[[\"甲\",\"3m\"],[\"乙\",\"8m\"]]}"),
        ).unwrap();
        assert_eq!(observed.lifecycle, Lifecycle::Commit);
        let retained = engine
            .execute(
                "alice",
                RequestEnvelope::new(
                    "case",
                    "retain",
                    "alice",
                    "前回のグループ別統計から仮説を保持して",
                ),
            )
            .unwrap();
        assert_eq!(retained.lifecycle, Lifecycle::Commit);
        for (id, table, code) in [
            (
                "alias",
                r#"{ "columns": ["値","班"], "rows": [["8.0m","乙"],["3.00m","甲"]] }"#,
                "GROUP_MEAN_HYPOTHESIS_SAMPLE_REUSED",
            ),
            (
                "groups",
                r#"{"columns":["班","値"],"rows":[["甲","4m"],["丙","9m"]]}"#,
                "GROUP_MEAN_HYPOTHESIS_SCOPE_MISMATCH",
            ),
            (
                "units",
                r#"{"columns":["班","値"],"rows":[["甲","4cm"],["乙","9cm"]]}"#,
                "GROUP_MEAN_HYPOTHESIS_SCOPE_MISMATCH",
            ),
            (
                "measure",
                r#"{"columns":["班","別の値"],"rows":[["甲","4m"],["乙","9m"]]}"#,
                "JSON_TABLE_GROUP_STATISTICS_COLUMN",
            ),
            (
                "unauthorized",
                r#"{"columns":["班","値"],"rows":[["甲","9m"],["乙","2m"]]}"#,
                "WRITE_PERMISSION_MISSING",
            ),
        ] {
            let mut request = RequestEnvelope::new(
                "case",
                id,
                "alice",
                &format!("前回の仮説を次の標本で照合して:\n{table}"),
            );
            request.context_ref = Some(retained.id.clone());
            request.output_path = Some(fixture.target.clone());
            if id != "unauthorized" {
                request
                    .permissions
                    .write_paths
                    .insert(fixture.target.clone());
            }
            let result = engine.execute("alice", request).unwrap();
            assert_eq!(result.lifecycle, Lifecycle::Suspend, "{result:?}");
            assert!(
                result.reason.as_deref().unwrap().starts_with(code),
                "{result:?}"
            );
            assert!(!fixture.target.exists());
            assert!(engine.store.actions("alice", "case").unwrap().is_empty());
        }
    }

    #[test]
    fn group_mean_hypothesis_requires_a_comparable_unique_sample_contrast() {
        for (table, code) in [
            (
                r#"{"columns":["班","値"],"rows":[["甲","3"],["乙","3"]]}"#,
                "GROUP_MEAN_HYPOTHESIS_NO_CONTRAST",
            ),
            (
                r#"{"columns":["班","値"],"rows":[["甲","3"]]}"#,
                "GROUP_MEAN_HYPOTHESIS_NO_CONTRAST",
            ),
            (
                r#"{"columns":["班","値"],"rows":[["甲","1m"],["乙","30cm"]]}"#,
                "JSON_GROUP_MEAN_UNIT_MISMATCH",
            ),
        ] {
            let fixture = empty_fixture();
            let mut engine = Engine::open(&fixture.state).unwrap();
            let text = format!("JSON表の「班」ごとに「値」の統計量を計算して:\n{table}");
            let observation = engine
                .execute(
                    "alice",
                    RequestEnvelope::new("case", "sample", "alice", &text),
                )
                .unwrap();
            assert_eq!(observation.lifecycle, Lifecycle::Commit, "{observation:?}");
            let result = engine
                .execute(
                    "alice",
                    RequestEnvelope::new(
                        "case",
                        "retain",
                        "alice",
                        "前回のグループ別統計から仮説を保持して",
                    ),
                )
                .unwrap();
            assert_eq!(result.lifecycle, Lifecycle::Suspend, "{result:?}");
            assert!(
                result.reason.as_deref().unwrap().starts_with(code),
                "{result:?}"
            );
            assert!(result.content_plan.is_none());
            assert!(result.dependencies.is_empty());
            assert!(engine.store.actions("alice", "case").unwrap().is_empty());
        }
    }

    #[test]
    fn source_question_recovery_revalidates_the_exact_source_answer_artifact() {
        use minidora_domain::{
            EvidencePacket, Guarantee, ResultRecord, canonical, digest, identity,
        };

        let fixture = empty_fixture();
        let store = Store::open(&fixture.state).unwrap();
        let source_text =
            "Photosynthesis stores light energy. Plants use photosynthesis in their leaves.";
        let raw_body = source_text.as_bytes().to_vec();
        let packet = EvidencePacket {
            id: "b".repeat(64),
            mode: "FIXTURE".into(),
            packet_kind: "FETCHED_BODY".into(),
            request_uri: "https://example.test/article".into(),
            final_uri: "https://example.test/article".into(),
            redirect_chain: Vec::new(),
            observed_at_unix_ms: 1,
            provider_id: "recovery-test-fixture".into(),
            http_status: 200,
            raw_sha256: digest(&raw_body),
            raw_body,
            extracted_text: Some(source_text.into()),
            parser_version: "fixture-exact-text-v1".into(),
            independence_group: "https://example.test/article".into(),
            failure: None,
            retry_after: None,
        };
        let prior = ResultRecord {
            id: "a".repeat(64),
            input_id: "source-result".into(),
            input_version: 1,
            lifecycle: Lifecycle::Commit,
            text: "Fetched source retained.".into(),
            reason: None,
            guarantee: Guarantee::SourceReport,
            calculation: None,
            content_plan: None,
            render: None,
            receipts: Vec::new(),
            evidence: vec![packet.clone()],
            dependencies: Vec::new(),
            actor_memory_revision: None,
            execution_graph_ref: None,
        };
        let mut request = RequestEnvelope::new(
            "case",
            "source-question",
            "alice",
            "What does the source say about photosynthesis?",
        );
        request.context_ref = Some(prior.id.clone());
        let bundle = crate::input::seal(&request, Some(&prior)).unwrap();
        let fingerprint = identity(&request).unwrap();
        store
            .register_input("alice", &request, &fingerprint)
            .unwrap();
        let packet_ref = store.put_artifact("alice", "case", &packet).unwrap();
        let answer = crate::text_summary::answer_fetched_source_question_v2(
            &prior.id,
            "photosynthesis",
            &[(0, &packet, packet_ref.clone())],
        )
        .unwrap();
        let answer_bytes = canonical(&answer).unwrap();
        let answer_ref = store.put_bytes("alice", "case", &answer_bytes).unwrap();
        let expected_units = crate::text_summary::source_question_answer_lines_v2(&answer);
        let dependencies = vec![prior.id.clone(), packet_ref.clone(), answer_ref.clone()];
        let plan = ContentPlan {
            id: "plan-fixture".into(),
            text: expected_units.join("\n"),
            locale: "ja".into(),
            guarantee: Guarantee::Hypothesis,
            required_units: expected_units.clone(),
            dependencies: dependencies.clone(),
            model_revision: "fixture".into(),
            model_artifact_ref: "fixture".into(),
            model_observation_ref: "fixture".into(),
        };
        let result = ResultRecord {
            id: fingerprint.clone(),
            input_id: request.input_id.clone(),
            input_version: request.input_version,
            lifecycle: Lifecycle::Adopted,
            text: plan.text.clone(),
            reason: None,
            guarantee: Guarantee::Hypothesis,
            calculation: None,
            content_plan: Some(plan.clone()),
            render: None,
            receipts: Vec::new(),
            evidence: Vec::new(),
            dependencies: dependencies.clone(),
            actor_memory_revision: None,
            execution_graph_ref: None,
        };
        let goal = GoalContract {
            input_fingerprint: fingerprint.clone(),
            intent: bundle.meaning.intent.clone(),
            output_path: None,
            permissions: request.permissions.clone(),
            budget: request.budget.clone(),
            required_predicates: Vec::new(),
            required_graph_ref: "fixture".into(),
            renderer_version: None,
        };
        let state = CaseRevision {
            schema_version: minidora_domain::SCHEMA_VERSION.into(),
            case_id: "case".into(),
            actor_id: "alice".into(),
            revision: 0,
            parent_revision: None,
            lifecycle: Lifecycle::Adopted,
            goal_sha256: None,
            input_fingerprint: Some(fingerprint),
            bundle: Some(bundle),
            result: Some(result.clone()),
            reason: None,
            prior_results: BTreeMap::from([(prior.id.clone(), prior)]),
        };
        let verified =
            verify_source_question_context(&store, "alice", "case", &state, &result, &plan, &goal)
                .unwrap()
                .unwrap();
        assert_eq!(verified.source_packet_refs, vec![(0, packet_ref)]);
        assert_eq!(verified.answer_ref, answer_ref);

        let mut changed_context = state.clone();
        changed_context
            .prior_results
            .values_mut()
            .next()
            .unwrap()
            .text
            .push_str(" altered after sealing");
        let error = verify_source_question_context(
            &store,
            "alice",
            "case",
            &changed_context,
            &result,
            &plan,
            &goal,
        )
        .err()
        .unwrap();
        assert!(
            error
                .to_string()
                .contains("RECOVERY_SOURCE_QUESTION_MISMATCH")
        );

        let mut changed_plan = plan;
        changed_plan.required_units.push("extra sentence".into());
        assert!(
            verify_source_question_context(
                &store,
                "alice",
                "case",
                &state,
                &result,
                &changed_plan,
                &goal,
            )
            .is_err()
        );
    }

    #[test]
    fn source_question_recovery_dispatches_versioned_and_legacy_artifacts() {
        use minidora_domain::{EvidencePacket, canonical, digest};

        let source_text = "Photosynthesis stores light energy.";
        let raw_body = source_text.as_bytes().to_vec();
        let packet = EvidencePacket {
            id: "b".repeat(64),
            mode: "FIXTURE".into(),
            packet_kind: "FETCHED_BODY".into(),
            request_uri: "https://example.test/article".into(),
            final_uri: "https://example.test/article".into(),
            redirect_chain: Vec::new(),
            observed_at_unix_ms: 1,
            provider_id: "recovery-test-fixture".into(),
            http_status: 200,
            raw_sha256: digest(&raw_body),
            raw_body,
            extracted_text: Some(source_text.into()),
            parser_version: "fixture-exact-text-v1".into(),
            independence_group: "https://example.test/article".into(),
            failure: None,
            retry_after: None,
        };
        let packet_ref = digest(&canonical(&packet).unwrap());
        let packets = [(0, &packet, packet_ref)];
        for operation in [
            "SOURCE_QUESTION_EXTRACTIVE_CANDIDATES_V1",
            "SOURCE_QUESTION_VERSIONED_EXTRACTIVE_CANDIDATES_V2",
        ] {
            let (bytes, lines) = crate::text_summary::recompute_source_question_artifact(
                operation,
                &"a".repeat(64),
                "photosynthesis",
                &packets,
            )
            .unwrap();
            let artifact: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
            assert_eq!(artifact["operation"], operation);
            assert!(
                lines
                    .join("\n")
                    .contains("Photosynthesis stores light energy.")
            );
        }
    }

    #[test]
    fn direct_text_summary_candidate_recovery_revalidates_report_without_resending_file() {
        for missing_report in [false, true] {
            let fixture = empty_fixture();
            let body = r#"{"text":"\u732b is quiet. The moon is bright.","max_sentences":1,"max_chars":64,"required_sentence_indices":[0]}"#;
            let input = format!("text-summary:\n{body}");
            let mut request = RequestEnvelope::new("case", "summary", "alice", &input);
            request.output_path = Some(fixture.target.clone());
            request
                .permissions
                .write_paths
                .insert(fixture.target.clone());
            let mut engine = Engine::open(&fixture.state).unwrap();
            let original = engine.execute("alice", request.clone()).unwrap();
            assert_eq!(original.lifecycle, Lifecycle::Commit, "{original:?}");
            assert_eq!(original.guarantee, Guarantee::Hypothesis);
            assert!(original.text.contains("猫 is quiet."));
            assert!(!original.text.contains("The moon is bright."));
            assert_eq!(original.dependencies.len(), 1);
            let report_ref = &original.dependencies[0];
            let report = engine.store.get_bytes("alice", "case", report_ref).unwrap();
            let artifact: serde_json::Value = serde_json::from_slice(&report).unwrap();
            assert_eq!(
                artifact["operation"],
                "TEXT_EXTRACTIVE_SUMMARY_CANDIDATES_V4"
            );
            assert_eq!(artifact["coverage_verified"], false);
            assert_eq!(digest(&report), *report_ref);
            let bytes = fs::read(&fixture.target).unwrap();
            assert_eq!(bytes, original.text.as_bytes());
            let modified = fs::metadata(&fixture.target).unwrap().modified().unwrap();
            let original_actions = engine.store.actions("alice", "case").unwrap();
            assert_eq!(original_actions.len(), 1);
            assert_eq!(original_actions[0].phase, EffectPhase::Confirmed);
            drop(engine);

            let token = retain_attempted_prefix(&fixture.state);
            if missing_report {
                // Remove only this report's case binding; its exact blob and
                // the genuine adopted state/action journal remain intact.
                let database = Connection::open(fixture.state.join("state.sqlite3")).unwrap();
                assert_eq!(
                    database
                        .execute(
                            "DELETE FROM case_artifact WHERE case_id=?1 AND sha256=?2",
                            params!["case", report_ref],
                        )
                        .unwrap(),
                    1
                );
                assert_eq!(
                    fs::read(fixture.state.join("blobs").join(report_ref)).unwrap(),
                    report
                );
            }
            let mut restored = Engine::open(&fixture.state).unwrap();
            let retained = restored.latest("alice", "case").unwrap().unwrap();
            assert_eq!(retained.lifecycle, Lifecycle::Adopted);
            assert_eq!(retained.bundle.as_ref().unwrap().request, request);
            assert!(matches!(
                retained.bundle.as_ref().unwrap().meaning.intent,
                Intent::TextSummaryCandidates { .. }
            ));
            assert_eq!(
                retained.result.as_ref().unwrap().guarantee,
                Guarantee::Hypothesis
            );
            assert!(retained.result.as_ref().unwrap().receipts.is_empty());
            let attempted = restored.store.actions("alice", "case").unwrap();
            assert_eq!(attempted.len(), 1);
            assert_eq!(attempted[0].phase, EffectPhase::Unknown);
            assert!(attempted[0].receipt.is_none());
            assert_eq!(attempted[0].action_id, original_actions[0].action_id);
            assert_eq!(attempted[0].idempotency_key, token);

            let recovered = restored.reconcile("alice", "case").unwrap();
            assert_eq!(recovered.id, original.id);
            assert_eq!(recovered.guarantee, Guarantee::Hypothesis);
            assert_eq!(recovered.text, original.text);
            assert_eq!(recovered.dependencies, original.dependencies);
            assert_eq!(recovered.content_plan, original.content_plan);
            assert_eq!(recovered.render, original.render);
            let actions = restored.store.actions("alice", "case").unwrap();
            if missing_report {
                assert_eq!(recovered.lifecycle, Lifecycle::Suspend, "{recovered:?}");
                assert!(
                    recovered
                        .reason
                        .as_deref()
                        .unwrap()
                        .contains("RECOVERY_TEXT_SUMMARY_CANDIDATE_MISMATCH")
                );
                assert!(recovered.receipts.is_empty());
                assert_eq!(actions, attempted);
            } else {
                assert_eq!(recovered.lifecycle, Lifecycle::Commit, "{recovered:?}");
                assert_eq!(recovered.receipts.len(), 1);
                let receipt = &recovered.receipts[0];
                assert_eq!(receipt.action_id, attempted[0].action_id);
                assert_eq!(receipt.phase, EffectPhase::Confirmed);
                assert_eq!(receipt.content_sha256, digest(&bytes));
                assert_eq!(receipt.observed_bytes, Some(bytes.len() as u64));
                assert_eq!(
                    receipt.reason.as_deref(),
                    Some("LOCAL_FILE_RECONCILED_NO_RESEND")
                );
                let mut expected = attempted[0].clone();
                expected.phase = EffectPhase::Confirmed;
                expected.receipt = Some(receipt.clone());
                assert_eq!(actions, vec![expected]);
                assert_eq!(restored.reconcile("alice", "case").unwrap(), recovered);
            }
            assert_eq!(restored.execute("alice", request).unwrap(), recovered);
            assert_eq!(restored.store.actions("alice", "case").unwrap(), actions);
            assert_eq!(fs::read(&fixture.target).unwrap(), bytes);
            assert_eq!(
                fs::metadata(&fixture.target).unwrap().modified().unwrap(),
                modified
            );
        }
    }

    #[test]
    fn previous_result_summary_recovery_revalidates_source_and_candidate_artifact() {
        use minidora_domain::{GoalContract, Guarantee};

        let fixture = empty_fixture();
        let mut engine = Engine::open(&fixture.state).unwrap();
        for (input_id, input) in [
            ("coco", "ココは鳥"),
            ("mike", "ミケは鳥"),
            ("common", "ココとミケの共通点は何ですか"),
        ] {
            assert_eq!(
                engine
                    .execute(
                        "alice",
                        RequestEnvelope::new("case", input_id, "alice", input)
                    )
                    .unwrap()
                    .lifecycle,
                Lifecycle::Commit
            );
        }
        let summary = engine
            .execute(
                "alice",
                RequestEnvelope::new("case", "summary", "alice", "前回の比較を要約して"),
            )
            .unwrap();
        assert_eq!(summary.lifecycle, Lifecycle::Commit);
        assert_eq!(summary.guarantee, Guarantee::Hypothesis);

        let state = engine.latest("alice", "case").unwrap().unwrap();
        let goal_ref = state.goal_sha256.as_deref().unwrap();
        let goal: GoalContract =
            serde_json::from_slice(&engine.store.get_bytes("alice", "case", goal_ref).unwrap())
                .unwrap();
        let plan = summary.content_plan.as_ref().unwrap();
        let expected = verify_previous_result_summary_context(
            &engine.store,
            "alice",
            "case",
            &state,
            &summary,
            plan,
            &goal,
        )
        .unwrap()
        .expect("the exact summary context should be replayable");
        assert_eq!(summary.dependencies[0], expected.summary_ref);
        assert!(summary.dependencies.contains(&expected.content_ref));
        assert!(summary.dependencies.contains(&expected.render_ref));

        let mut altered = state.clone();
        let source_result_id = altered
            .bundle
            .as_ref()
            .unwrap()
            .meaning
            .context_result_ref
            .as_deref()
            .unwrap();
        altered
            .prior_results
            .get_mut(source_result_id)
            .unwrap()
            .text
            .push(' ');
        let changed = verify_previous_result_summary_context(
            &engine.store,
            "alice",
            "case",
            &altered,
            &summary,
            plan,
            &goal,
        )
        .err()
        .unwrap();
        assert!(
            changed
                .to_string()
                .contains("RECOVERY_RESULT_SUMMARY_SOURCE_INTEGRITY")
        );
    }

    #[test]
    fn source_question_recovery_keeps_the_legacy_v1_derivation_readable() {
        use minidora_domain::{EvidencePacket, canonical, digest};

        let source_text = "Photosynthesis stores light energy.";
        let raw_body = source_text.as_bytes().to_vec();
        let packet = EvidencePacket {
            id: "b".repeat(64),
            mode: "FIXTURE".into(),
            packet_kind: "FETCHED_BODY".into(),
            request_uri: "https://example.test/article".into(),
            final_uri: "https://example.test/article".into(),
            redirect_chain: Vec::new(),
            observed_at_unix_ms: 1,
            provider_id: "recovery-test-fixture".into(),
            http_status: 200,
            raw_sha256: digest(&raw_body),
            raw_body,
            extracted_text: Some(source_text.into()),
            parser_version: "fixture-exact-text-v1".into(),
            independence_group: "https://example.test/article".into(),
            failure: None,
            retry_after: None,
        };
        let packet_ref = digest(&canonical(&packet).unwrap());
        let packets = [(0, &packet, packet_ref)];
        let answer = crate::text_summary::answer_fetched_source_question_v1(
            &"a".repeat(64),
            "photosynthesis",
            &packets,
        )
        .unwrap();
        let expected_bytes = canonical(&answer).unwrap();
        let expected_lines = crate::text_summary::source_question_answer_lines_v1(&answer);
        let (replayed_bytes, replayed_lines) =
            crate::text_summary::recompute_source_question_artifact(
                "SOURCE_QUESTION_EXTRACTIVE_CANDIDATES_V1",
                &"a".repeat(64),
                "photosynthesis",
                &packets,
            )
            .unwrap();
        assert_eq!(replayed_bytes, expected_bytes);
        assert_eq!(replayed_lines, expected_lines);
        assert!(
            crate::text_summary::recompute_source_question_artifact(
                "SOURCE_QUESTION_UNKNOWN_V9",
                &"a".repeat(64),
                "photosynthesis",
                &packets,
            )
            .unwrap_err()
            .to_string()
            .contains("RECOVERY_SOURCE_QUESTION_VERSION")
        );
    }

    fn attempted_prefix() -> Fixture {
        attempted_calculation_prefix(false)
    }

    fn attempted_calculation_prefix(legacy: bool) -> Fixture {
        let directory = std::env::temp_dir().join(format!(
            "minidora-recovery-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir(&directory).unwrap();
        let state = directory.join("state");
        let target = directory.join("actual-report.txt");
        let mut request = RequestEnvelope::new("case", "input", "alice", "2+3");
        request.output_path = Some(target.clone());
        request.permissions.write_paths.insert(target.clone());
        {
            let mut engine = Engine::open(&state).unwrap();
            let adopted = if legacy {
                engine
                    .execute_legacy_calculation_fixture("alice", request)
                    .unwrap()
            } else {
                engine.execute("alice", request).unwrap()
            };
            assert_eq!(adopted.lifecycle, Lifecycle::Commit, "{adopted:?}");
        }
        let token = retain_attempted_prefix(&state);
        Fixture {
            directory,
            state,
            target,
            token,
        }
    }

    fn retain_attempted_prefix(state: &Path) -> String {
        // Test-only crash injection: retain an actual production journal prefix
        // immediately before effect confirmation, while keeping the physical
        // output that already occurred. This models rollback of the later
        // transactions rather than fabricating a successful action result.
        let database = Connection::open(state.join("state.sqlite3")).unwrap();
        let snapshots: Vec<(i64, String)> = {
            let mut statement = database
                .prepare("SELECT revision,state_sha256 FROM case_revision ORDER BY revision")
                .unwrap();
            statement
                .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
                .unwrap()
                .collect::<std::result::Result<_, _>>()
                .unwrap()
        };
        let mut retained = None;
        for (number, sha) in snapshots {
            let revision: CaseRevision =
                serde_json::from_slice(&fs::read(state.join("blobs").join(sha)).unwrap()).unwrap();
            if revision.lifecycle == Lifecycle::Adopted
                && revision.result.as_ref().is_some_and(|r| r.render.is_some())
            {
                retained = Some((number, revision));
            }
        }
        let (number, revision) =
            retained.expect("normal Core persisted actual rendering before file effect");
        let result = revision.result.as_ref().unwrap();
        let result_sha = digest(&canonical(result).unwrap());
        let token: String = database
            .query_row("SELECT idempotency_key FROM action_identity", [], |r| {
                r.get(0)
            })
            .unwrap();
        database.execute_batch("PRAGMA foreign_keys=ON; DROP TRIGGER case_revision_no_delete; DROP TRIGGER action_history_no_delete;").unwrap();
        database
            .execute("DELETE FROM case_revision WHERE revision>?1", [number])
            .unwrap();
        database
            .execute("DELETE FROM action_history WHERE sequence>3", [])
            .unwrap();
        database
            .execute(
                "UPDATE input_identity SET result_sha256=?1
                 WHERE case_id=?2 AND input_id=?3 AND input_version=?4",
                params![
                    result_sha,
                    revision.case_id,
                    result.input_id,
                    i64::try_from(result.input_version).unwrap()
                ],
            )
            .unwrap();
        token
    }

    fn attempted_statistics_explanation_prefix() -> Fixture {
        attempted_math_report_explanation_prefix(
            "statistics",
            "統計 [0.1,0.2,0.3]",
            "statistics-explanation.txt",
        )
    }

    #[test]
    fn operand_correction_recovery_verifies_sealed_change_and_context() {
        let fixture = empty_fixture();
        let mut engine = Engine::open(&fixture.state).unwrap();
        let original = engine
            .execute(
                "alice",
                RequestEnvelope::new("case", "original", "alice", "1 km + 500 m"),
            )
            .unwrap();
        assert_eq!(original.lifecycle, Lifecycle::Commit);
        let detail = engine
            .execute(
                "alice",
                RequestEnvelope::new("case", "detail", "alice", "詳しく説明して"),
            )
            .unwrap();
        assert_eq!(detail.lifecycle, Lifecycle::Commit);
        let mut request =
            RequestEnvelope::new("case", "correction", "alice", "第二項を600 mに変更して");
        request.budget.max_actions = 5;
        request.output_path = Some(fixture.target.clone());
        request
            .permissions
            .write_paths
            .insert(fixture.target.clone());
        let corrected = engine.execute("alice", request).unwrap();
        assert_eq!(corrected.lifecycle, Lifecycle::Commit, "{corrected:?}");
        assert_eq!(corrected.text, "1 km + 600 m = 1600 m");
        assert_eq!(corrected.dependencies, [detail.id]);
        let bytes = fs::read(&fixture.target).unwrap();
        let modified = fs::metadata(&fixture.target).unwrap().modified().unwrap();
        drop(engine);

        let token = retain_attempted_prefix(&fixture.state);
        let store = Store::open(&fixture.state).unwrap();
        let state = store.load_case("alice", "case").unwrap().unwrap();
        let mut changed = state.result.as_ref().unwrap().clone();
        changed.calculation = Some(language::calculate("1 km + 700 m").unwrap());
        let error = inspect(&store, "alice", &state, &changed).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("RECOVERY_CALCULATION_CORRECTION_MISMATCH")
        );
        let mut without_original = state.clone();
        assert!(
            without_original
                .prior_results
                .remove(&original.id)
                .is_some()
        );
        let error = inspect(
            &store,
            "alice",
            &without_original,
            state.result.as_ref().unwrap(),
        )
        .unwrap_err();
        assert!(error.to_string().contains("CORRECTION_SOURCE_MISMATCH"));
        assert_eq!(
            store.actions("alice", "case").unwrap()[0].phase,
            EffectPhase::Unknown
        );

        let recovered = reconcile(&store, "alice", "case").unwrap();
        assert_eq!(recovered.lifecycle, Lifecycle::Commit, "{recovered:?}");
        assert_eq!(recovered.dependencies, corrected.dependencies);
        assert_eq!(recovered.receipts[0].action_id, token);
        assert_eq!(store.actions("alice", "case").unwrap().len(), 1);
        assert_eq!(fs::read(&fixture.target).unwrap(), bytes);
        assert_eq!(
            fs::metadata(&fixture.target).unwrap().modified().unwrap(),
            modified
        );
    }

    #[test]
    fn sequence_correction_recovery_authenticates_original_and_revision() {
        let fixture = empty_fixture();
        let mut engine = Engine::open(&fixture.state).unwrap();
        let original = engine
            .execute(
                "alice",
                RequestEnvelope::new(
                    "case",
                    "sequence",
                    "alice",
                    "計算手順を実行して:\nbase: 1 km + 500 m\nfinal: 100 m + @{base}",
                ),
            )
            .unwrap();
        assert_eq!(original.lifecycle, Lifecycle::Commit);
        let mut request = RequestEnvelope::new(
            "case",
            "sequence",
            "alice",
            "計算手順の base を訂正: 500 m + 100 m",
        );
        request.input_version = 2;
        request.budget.max_actions = 8;
        request.output_path = Some(fixture.target.clone());
        request
            .permissions
            .write_paths
            .insert(fixture.target.clone());
        let corrected = engine.execute("alice", request).unwrap();
        assert_eq!(corrected.lifecycle, Lifecycle::Commit, "{corrected:?}");
        assert_eq!(corrected.calculation.as_ref().unwrap().exact_value, "700 m");
        let bytes = fs::read(&fixture.target).unwrap();
        let modified = fs::metadata(&fixture.target).unwrap().modified().unwrap();
        drop(engine);

        let token = retain_attempted_prefix(&fixture.state);
        let store = Store::open(&fixture.state).unwrap();
        let state = store.load_case("alice", "case").unwrap().unwrap();
        let mut altered = state.result.as_ref().unwrap().clone();
        altered.dependencies.remove(1);
        let error = inspect(&store, "alice", &state, &altered).unwrap_err();
        assert!(error.to_string().contains("RECOVERY_DEPENDENCY_MISMATCH"));
        let mut without_original = state.clone();
        without_original.prior_results.remove(&original.id).unwrap();
        let error = inspect(
            &store,
            "alice",
            &without_original,
            state.result.as_ref().unwrap(),
        )
        .unwrap_err();
        assert!(
            error
                .to_string()
                .contains("RECOVERY_SEQUENCE_CORRECTION_MISMATCH")
        );
        assert_eq!(
            store.actions("alice", "case").unwrap()[0].phase,
            EffectPhase::Unknown
        );

        let recovered = reconcile(&store, "alice", "case").unwrap();
        assert_eq!(recovered.lifecycle, Lifecycle::Commit, "{recovered:?}");
        assert_eq!(recovered.calculation, corrected.calculation);
        assert_eq!(recovered.dependencies, corrected.dependencies);
        assert_eq!(recovered.content_plan, corrected.content_plan);
        assert_eq!(recovered.render, corrected.render);
        assert_eq!(recovered.text, corrected.text);
        assert_eq!(
            recovered.receipts[0].reason.as_deref(),
            Some("LOCAL_FILE_RECONCILED_NO_RESEND")
        );
        assert_eq!(recovered.receipts[0].action_id, token);
        assert_eq!(store.actions("alice", "case").unwrap().len(), 1);
        assert_eq!(fs::read(&fixture.target).unwrap(), bytes);
        assert_eq!(
            fs::metadata(&fixture.target).unwrap().modified().unwrap(),
            modified
        );
    }

    #[test]
    fn repeated_sequence_correction_reconciles_mixed_history_without_resend() {
        assert_revised_sequence_file_recovery(false);
    }

    #[test]
    fn revised_sequence_explanation_reconciles_mixed_history_without_resend() {
        assert_revised_sequence_file_recovery(true);
    }

    #[test]
    fn rewired_sequence_correction_reconciles_changed_graph_without_resend() {
        fn graph(store: &Store, result: &ResultRecord) -> serde_json::Value {
            serde_json::from_slice(
                &store
                    .get_bytes(
                        "alice",
                        "case",
                        result.execution_graph_ref.as_ref().unwrap(),
                    )
                    .unwrap(),
            )
            .unwrap()
        }

        let fixture = empty_fixture();
        let mut engine = Engine::open(&fixture.state).unwrap();
        let original = engine
            .execute(
                "alice",
                RequestEnvelope::new(
                    "case",
                    "sequence",
                    "alice",
                    "計算手順を実行して:\nleft: 2+3\nright: 7+4\nfinal: @{left}*2",
                ),
            )
            .unwrap();
        assert_eq!(original.lifecycle, Lifecycle::Commit, "{original:?}");
        assert_eq!(original.calculation.as_ref().unwrap().exact_value, "10");
        let original_graph = graph(&engine.store, &original);
        assert_eq!(
            original_graph[5]["sequence-final"]["sequence-left"],
            original_graph[1]["sequence-left"]
        );
        assert!(
            original_graph[5]["sequence-final"]
                .get("sequence-right")
                .is_none()
        );

        let mut request = RequestEnvelope::new(
            "case",
            "rewire",
            "alice",
            "計算手順の final を参照先も含めて訂正: @{right}*2",
        );
        request.budget.max_actions = 10;
        request.output_path = Some(fixture.target.clone());
        request
            .permissions
            .write_paths
            .insert(fixture.target.clone());
        let result = engine.execute("alice", request.clone()).unwrap();
        assert_eq!(result.lifecycle, Lifecycle::Commit, "{result:?}");
        assert_eq!(result.calculation.as_ref().unwrap().exact_value, "22");
        let changed_graph = graph(&engine.store, &result);
        assert_eq!(changed_graph.as_array().unwrap().len(), 6);
        assert_eq!(
            changed_graph[5]["sequence-final"]["sequence-right"],
            changed_graph[1]["sequence-right"]
        );
        assert!(
            changed_graph[5]["sequence-final"]
                .get("sequence-left")
                .is_none()
        );
        let bytes = fs::read(&fixture.target).unwrap();
        let modified = fs::metadata(&fixture.target).unwrap().modified().unwrap();
        drop(engine);

        let token = retain_attempted_prefix(&fixture.state);
        let store = Store::open(&fixture.state).unwrap();
        let state = store.load_case("alice", "case").unwrap().unwrap();
        let bundle = state.bundle.as_ref().unwrap();
        assert_eq!(
            bundle.meaning.parser_version,
            crate::input::SEQUENCE_REWIRE_PARSER_VERSION
        );
        assert!(matches!(
            bundle.meaning.intent,
            Intent::CorrectCalculationSequenceStep {
                change_dependencies: true,
                ..
            }
        ));
        let goal: GoalContract = serde_json::from_slice(
            &store
                .get_bytes("alice", "case", state.goal_sha256.as_ref().unwrap())
                .unwrap(),
        )
        .unwrap();
        assert_eq!(
            goal.required_predicates[0],
            REWIRED_SEQUENCE_CORRECTION_PREDICATE
        );
        let mut nodes: Vec<GoalNode> = serde_json::from_slice(
            &store
                .get_bytes("alice", "case", &goal.required_graph_ref)
                .unwrap(),
        )
        .unwrap();
        let final_node = nodes
            .iter_mut()
            .find(|node| node.id == "sequence-final")
            .unwrap();
        assert_eq!(final_node.requires, ["sequence-right"]);
        final_node.requires = vec!["sequence-left".into()];
        let actions = store.actions("alice", "case").unwrap();
        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0].phase, EffectPhase::Unknown);
        let retained = state.result.as_ref().unwrap();
        let mut wrong_graph = state.clone();
        let mut wrong_goal = goal.clone();
        wrong_goal.required_graph_ref = store.put_artifact("alice", "case", &nodes).unwrap();
        wrong_graph.goal_sha256 = Some(store.put_artifact("alice", "case", &wrong_goal).unwrap());
        let error = inspect(&store, "alice", &wrong_graph, retained).unwrap_err();
        assert!(
            error.to_string().contains("UNSUPPORTED_RECOVERY_GRAPH"),
            "{error:?}"
        );
        let mut wrong_marker = state.clone();
        let mut wrong_goal = goal;
        wrong_goal.required_predicates[0] = REPEATED_SEQUENCE_CORRECTION_PREDICATE.into();
        wrong_marker.goal_sha256 = Some(store.put_artifact("alice", "case", &wrong_goal).unwrap());
        let error = inspect(&store, "alice", &wrong_marker, retained).unwrap_err();
        assert!(
            error.to_string().contains("UNSUPPORTED_RECOVERY_GOAL"),
            "{error:?}"
        );
        assert_eq!(store.actions("alice", "case").unwrap(), actions);
        assert_eq!(fs::read(&fixture.target).unwrap(), bytes);
        assert_eq!(
            fs::metadata(&fixture.target).unwrap().modified().unwrap(),
            modified
        );

        let recovered = reconcile(&store, "alice", "case").unwrap();
        assert_eq!(recovered.lifecycle, Lifecycle::Commit, "{recovered:?}");
        assert_eq!(recovered.calculation, result.calculation);
        assert_eq!(recovered.dependencies, result.dependencies);
        assert_eq!(recovered.content_plan, result.content_plan);
        assert_eq!(recovered.render, result.render);
        assert_eq!(recovered.text, result.text);
        assert_eq!(recovered.receipts[0].action_id, token);
        assert_eq!(
            recovered.receipts[0].reason.as_deref(),
            Some("LOCAL_FILE_RECONCILED_NO_RESEND")
        );
        let mut expected_graph = changed_graph;
        expected_graph[1]["file"] = digest(&canonical(&recovered.receipts[0]).unwrap()).into();
        assert_eq!(graph(&store, &recovered), expected_graph);
        let confirmed = store.actions("alice", "case").unwrap();
        assert_eq!(confirmed.len(), 1);
        drop(store);
        let mut engine = Engine::open(&fixture.state).unwrap();
        assert_eq!(engine.execute("alice", request).unwrap(), recovered);
        assert_eq!(engine.reconcile("alice", "case").unwrap(), recovered);
        assert_eq!(engine.store.actions("alice", "case").unwrap(), confirmed);
        assert_eq!(fs::read(&fixture.target).unwrap(), bytes);
        assert_eq!(
            fs::metadata(&fixture.target).unwrap().modified().unwrap(),
            modified
        );
    }

    #[test]
    fn appended_sequence_step_reconciles_new_graph_without_resend() {
        let read_graph = |store: &Store, result: &ResultRecord| -> serde_json::Value {
            serde_json::from_slice(
                &store
                    .get_bytes(
                        "alice",
                        "case",
                        result.execution_graph_ref.as_ref().unwrap(),
                    )
                    .unwrap(),
            )
            .unwrap()
        };
        let fixture = empty_fixture();
        let mut engine = Engine::open(&fixture.state).unwrap();
        let original_request = RequestEnvelope::new(
            "case",
            "sequence",
            "alice",
            "計算手順を実行して:\nbase: 2+3\nfinal: @{base}*2",
        );
        let original = engine.execute("alice", original_request.clone()).unwrap();
        assert_eq!(original.lifecycle, Lifecycle::Commit, "{original:?}");
        assert_eq!(original.calculation.as_ref().unwrap().exact_value, "10");
        let original_graph = read_graph(&engine.store, &original);
        assert!(original_graph[1].get("sequence-total").is_none());

        let mut request = RequestEnvelope::new(
            "case",
            "append",
            "alice",
            "計算手順の末尾に total を追加: @{final}+2",
        );
        request.budget.max_actions = 9;
        request.output_path = Some(fixture.target.clone());
        request
            .permissions
            .write_paths
            .insert(fixture.target.clone());
        let result = engine.execute("alice", request.clone()).unwrap();
        assert_eq!(result.lifecycle, Lifecycle::Commit, "{result:?}");
        assert_eq!(result.calculation.as_ref().unwrap().exact_value, "12");
        let appended_graph = read_graph(&engine.store, &result);
        assert_eq!(appended_graph.as_array().unwrap().len(), 6);
        for id in ["sequence-base", "sequence-final"] {
            assert_eq!(appended_graph[1][id], original_graph[1][id]);
        }
        assert_eq!(
            appended_graph[5]["sequence-total"]["sequence-final"],
            appended_graph[1]["sequence-final"]
        );
        assert_eq!(
            appended_graph[5]["sequence-total"]
                .as_object()
                .unwrap()
                .len(),
            1
        );
        let bytes = fs::read(&fixture.target).unwrap();
        let modified = fs::metadata(&fixture.target).unwrap().modified().unwrap();
        drop(engine);

        let token = retain_attempted_prefix(&fixture.state);
        let store = Store::open(&fixture.state).unwrap();
        let state = store.load_case("alice", "case").unwrap().unwrap();
        let bundle = state.bundle.as_ref().unwrap();
        assert_eq!(
            bundle.meaning.parser_version,
            crate::input::SEQUENCE_APPEND_PARSER_VERSION
        );
        assert!(matches!(
            bundle.meaning.intent,
            Intent::AppendCalculationSequenceStep { .. }
        ));
        let goal: GoalContract = serde_json::from_slice(
            &store
                .get_bytes("alice", "case", state.goal_sha256.as_ref().unwrap())
                .unwrap(),
        )
        .unwrap();
        assert_eq!(goal.required_predicates[0], SEQUENCE_APPEND_PREDICATE);
        let mut nodes: Vec<GoalNode> = serde_json::from_slice(
            &store
                .get_bytes("alice", "case", &goal.required_graph_ref)
                .unwrap(),
        )
        .unwrap();
        let appended_node = nodes
            .iter_mut()
            .find(|node| node.id == "sequence-total")
            .unwrap();
        assert_eq!(appended_node.requires, ["sequence-final"]);
        appended_node.requires = vec!["sequence-base".into()];
        let actions = store.actions("alice", "case").unwrap();
        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0].phase, EffectPhase::Unknown);
        let retained = state.result.as_ref().unwrap();
        let mut wrong_graph = state.clone();
        let mut wrong_goal = goal.clone();
        wrong_goal.required_graph_ref = store.put_artifact("alice", "case", &nodes).unwrap();
        wrong_graph.goal_sha256 = Some(store.put_artifact("alice", "case", &wrong_goal).unwrap());
        let error = inspect(&store, "alice", &wrong_graph, retained).unwrap_err();
        assert!(
            error.to_string().contains("UNSUPPORTED_RECOVERY_GRAPH"),
            "{error:?}"
        );
        let mut wrong_marker = state.clone();
        let mut wrong_goal = goal;
        wrong_goal.required_predicates[0] = REWIRED_SEQUENCE_CORRECTION_PREDICATE.into();
        wrong_marker.goal_sha256 = Some(store.put_artifact("alice", "case", &wrong_goal).unwrap());
        let error = inspect(&store, "alice", &wrong_marker, retained).unwrap_err();
        assert!(
            error.to_string().contains("UNSUPPORTED_RECOVERY_GOAL"),
            "{error:?}"
        );
        assert_eq!(store.actions("alice", "case").unwrap(), actions);
        assert_eq!(fs::read(&fixture.target).unwrap(), bytes);
        assert_eq!(
            fs::metadata(&fixture.target).unwrap().modified().unwrap(),
            modified
        );

        let recovered = reconcile(&store, "alice", "case").unwrap();
        assert_eq!(recovered.lifecycle, Lifecycle::Commit, "{recovered:?}");
        assert_eq!(recovered.calculation, result.calculation);
        assert_eq!(recovered.dependencies, result.dependencies);
        assert_eq!(recovered.content_plan, result.content_plan);
        assert_eq!(recovered.render, result.render);
        assert_eq!(recovered.text, result.text);
        assert_eq!(recovered.receipts[0].action_id, token);
        assert_eq!(
            recovered.receipts[0].reason.as_deref(),
            Some("LOCAL_FILE_RECONCILED_NO_RESEND")
        );
        let mut expected_graph = appended_graph;
        expected_graph[1]["file"] = digest(&canonical(&recovered.receipts[0]).unwrap()).into();
        assert_eq!(read_graph(&store, &recovered), expected_graph);
        let confirmed = store.actions("alice", "case").unwrap();
        assert_eq!(confirmed.len(), 1);
        drop(store);
        let mut engine = Engine::open(&fixture.state).unwrap();
        assert_eq!(engine.execute("alice", request).unwrap(), recovered);
        assert_eq!(engine.reconcile("alice", "case").unwrap(), recovered);
        assert_eq!(engine.execute("alice", original_request).unwrap(), original);
        assert_eq!(engine.store.actions("alice", "case").unwrap(), confirmed);
        assert_eq!(fs::read(&fixture.target).unwrap(), bytes);
        assert_eq!(
            fs::metadata(&fixture.target).unwrap().modified().unwrap(),
            modified
        );
    }

    #[test]
    fn removed_sequence_step_reconciles_reduced_graph_without_resend() {
        let read_graph = |store: &Store, result: &ResultRecord| -> serde_json::Value {
            serde_json::from_slice(
                &store
                    .get_bytes(
                        "alice",
                        "case",
                        result.execution_graph_ref.as_ref().unwrap(),
                    )
                    .unwrap(),
            )
            .unwrap()
        };
        let fixture = empty_fixture();
        let mut engine = Engine::open(&fixture.state).unwrap();
        let original_request = RequestEnvelope::new(
            "case",
            "sequence",
            "alice",
            "計算手順を実行して:\nbase: 2+3\nspare: 8/4\nfinal: @{base}*2",
        );
        let original = engine.execute("alice", original_request.clone()).unwrap();
        assert_eq!(original.lifecycle, Lifecycle::Commit, "{original:?}");
        let original_graph = read_graph(&engine.store, &original);
        assert!(original_graph[1].get("sequence-spare").is_some());
        let mut request =
            RequestEnvelope::new("case", "remove", "alice", "計算手順の spare を削除");
        request.budget.max_actions = 9;
        request.output_path = Some(fixture.target.clone());
        request
            .permissions
            .write_paths
            .insert(fixture.target.clone());
        let result = engine.execute("alice", request.clone()).unwrap();
        assert_eq!(result.lifecycle, Lifecycle::Commit, "{result:?}");
        assert_eq!(result.calculation.as_ref().unwrap().exact_value, "10");
        let reduced_graph = read_graph(&engine.store, &result);
        assert_eq!(reduced_graph.as_array().unwrap().len(), 6);
        assert!(reduced_graph[1].get("sequence-spare").is_none());
        for id in ["sequence-base", "sequence-final"] {
            assert_eq!(reduced_graph[1][id], original_graph[1][id]);
        }
        assert_eq!(
            reduced_graph[5]["sequence-final"]["sequence-base"],
            reduced_graph[1]["sequence-base"]
        );
        let bytes = fs::read(&fixture.target).unwrap();
        let modified = fs::metadata(&fixture.target).unwrap().modified().unwrap();
        drop(engine);

        let token = retain_attempted_prefix(&fixture.state);
        let store = Store::open(&fixture.state).unwrap();
        let state = store.load_case("alice", "case").unwrap().unwrap();
        let bundle = state.bundle.as_ref().unwrap();
        assert_eq!(
            bundle.meaning.parser_version,
            crate::input::SEQUENCE_REMOVE_PARSER_VERSION
        );
        assert!(matches!(
            bundle.meaning.intent,
            Intent::RemoveCalculationSequenceStep { .. }
        ));
        let goal: GoalContract = serde_json::from_slice(
            &store
                .get_bytes("alice", "case", state.goal_sha256.as_ref().unwrap())
                .unwrap(),
        )
        .unwrap();
        assert_eq!(goal.required_predicates[0], SEQUENCE_REMOVE_PREDICATE);
        let mut nodes: Vec<GoalNode> = serde_json::from_slice(
            &store
                .get_bytes("alice", "case", &goal.required_graph_ref)
                .unwrap(),
        )
        .unwrap();
        assert!(!nodes.iter().any(|node| node.id == "sequence-spare"));
        nodes.push(GoalNode {
            id: "sequence-spare".into(),
            kind: GoalNodeKind::ComputedValue,
            requires: Vec::new(),
        });
        let actions = store.actions("alice", "case").unwrap();
        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0].phase, EffectPhase::Unknown);
        let retained = state.result.as_ref().unwrap();
        let mut wrong_graph = state.clone();
        let mut wrong_goal = goal.clone();
        wrong_goal.required_graph_ref = store.put_artifact("alice", "case", &nodes).unwrap();
        wrong_graph.goal_sha256 = Some(store.put_artifact("alice", "case", &wrong_goal).unwrap());
        let error = inspect(&store, "alice", &wrong_graph, retained).unwrap_err();
        assert!(
            error.to_string().contains("UNSUPPORTED_RECOVERY_GRAPH"),
            "{error:?}"
        );
        let mut wrong_marker = state.clone();
        let mut wrong_goal = goal;
        wrong_goal.required_predicates[0] = SEQUENCE_APPEND_PREDICATE.into();
        wrong_marker.goal_sha256 = Some(store.put_artifact("alice", "case", &wrong_goal).unwrap());
        let error = inspect(&store, "alice", &wrong_marker, retained).unwrap_err();
        assert!(
            error.to_string().contains("UNSUPPORTED_RECOVERY_GOAL"),
            "{error:?}"
        );
        let mut malformed_history = state.clone();
        malformed_history
            .prior_results
            .get_mut(&original.id)
            .unwrap()
            .dependencies
            .swap(0, 1);
        let error = inspect(&store, "alice", &malformed_history, retained).unwrap_err();
        assert!(
            error.to_string().contains("CALCULATION_CONTEXT_"),
            "{error:?}"
        );
        assert_eq!(store.actions("alice", "case").unwrap(), actions);
        assert_eq!(fs::read(&fixture.target).unwrap(), bytes);
        assert_eq!(
            fs::metadata(&fixture.target).unwrap().modified().unwrap(),
            modified
        );

        let recovered = reconcile(&store, "alice", "case").unwrap();
        assert_eq!(recovered.lifecycle, Lifecycle::Commit, "{recovered:?}");
        assert_eq!(recovered.calculation, result.calculation);
        assert_eq!(recovered.dependencies, result.dependencies);
        assert_eq!(recovered.content_plan, result.content_plan);
        assert_eq!(recovered.render, result.render);
        assert_eq!(recovered.receipts[0].action_id, token);
        assert_eq!(
            recovered.receipts[0].reason.as_deref(),
            Some("LOCAL_FILE_RECONCILED_NO_RESEND")
        );
        let mut expected_graph = reduced_graph;
        expected_graph[1]["file"] = digest(&canonical(&recovered.receipts[0]).unwrap()).into();
        assert_eq!(read_graph(&store, &recovered), expected_graph);
        let confirmed = store.actions("alice", "case").unwrap();
        assert_eq!(confirmed.len(), 1);
        assert_eq!(confirmed[0].phase, EffectPhase::Confirmed);
        drop(store);
        let mut engine = Engine::open(&fixture.state).unwrap();
        assert_eq!(engine.execute("alice", request).unwrap(), recovered);
        assert_eq!(engine.reconcile("alice", "case").unwrap(), recovered);
        assert_eq!(engine.execute("alice", original_request).unwrap(), original);
        assert_eq!(engine.store.actions("alice", "case").unwrap(), confirmed);
        assert_eq!(fs::read(&fixture.target).unwrap(), bytes);
        assert_eq!(
            fs::metadata(&fixture.target).unwrap().modified().unwrap(),
            modified
        );
    }

    #[test]
    fn moved_sequence_step_reconciles_reordered_graph_without_resend() {
        let read_graph = |store: &Store, result: &ResultRecord| -> serde_json::Value {
            serde_json::from_slice(
                &store
                    .get_bytes(
                        "alice",
                        "case",
                        result.execution_graph_ref.as_ref().unwrap(),
                    )
                    .unwrap(),
            )
            .unwrap()
        };
        let sequence_order = |graph: &serde_json::Value| -> Vec<String> {
            graph[0]
                .as_array()
                .unwrap()
                .iter()
                .filter_map(|node| node["id"].as_str())
                .filter(|id| id.starts_with("sequence-"))
                .map(str::to_owned)
                .collect()
        };
        let fixture = empty_fixture();
        let mut engine = Engine::open(&fixture.state).unwrap();
        let original_request = RequestEnvelope::new(
            "case",
            "sequence",
            "alice",
            "計算手順を実行して:\nbase: 2+3\nspare: 8/4\nfinal: @{base}*2",
        );
        let original = engine.execute("alice", original_request.clone()).unwrap();
        assert_eq!(original.lifecycle, Lifecycle::Commit, "{original:?}");
        assert_eq!(original.calculation.as_ref().unwrap().exact_value, "10");
        let original_graph = read_graph(&engine.store, &original);
        assert_eq!(
            sequence_order(&original_graph),
            ["sequence-base", "sequence-spare", "sequence-final"]
        );
        let mut request = RequestEnvelope::new(
            "case",
            "move",
            "alice",
            "計算手順の spare を final の後に移動",
        );
        request.budget.max_actions = 10;
        request.output_path = Some(fixture.target.clone());
        request
            .permissions
            .write_paths
            .insert(fixture.target.clone());
        let result = engine.execute("alice", request.clone()).unwrap();
        assert_eq!(result.lifecycle, Lifecycle::Commit, "{result:?}");
        assert_eq!(result.calculation.as_ref().unwrap().exact_value, "2");
        let moved_graph = read_graph(&engine.store, &result);
        assert_eq!(moved_graph.as_array().unwrap().len(), 6);
        assert_eq!(
            sequence_order(&moved_graph),
            ["sequence-base", "sequence-final", "sequence-spare"]
        );
        for id in ["sequence-base", "sequence-spare", "sequence-final"] {
            assert_eq!(moved_graph[1][id], original_graph[1][id]);
        }
        let bytes = fs::read(&fixture.target).unwrap();
        let modified = fs::metadata(&fixture.target).unwrap().modified().unwrap();
        drop(engine);

        let token = retain_attempted_prefix(&fixture.state);
        let store = Store::open(&fixture.state).unwrap();
        let state = store.load_case("alice", "case").unwrap().unwrap();
        let bundle = state.bundle.as_ref().unwrap();
        assert_eq!(
            bundle.meaning.parser_version,
            crate::input::SEQUENCE_MOVE_PARSER_VERSION
        );
        assert!(matches!(
            bundle.meaning.intent,
            Intent::MoveCalculationSequenceStep { before: false, .. }
        ));
        let goal: GoalContract = serde_json::from_slice(
            &store
                .get_bytes("alice", "case", state.goal_sha256.as_ref().unwrap())
                .unwrap(),
        )
        .unwrap();
        assert_eq!(goal.required_predicates[0], SEQUENCE_MOVE_PREDICATE);
        let mut nodes: Vec<GoalNode> = serde_json::from_slice(
            &store
                .get_bytes("alice", "case", &goal.required_graph_ref)
                .unwrap(),
        )
        .unwrap();
        nodes.swap(1, 2);
        nodes
            .iter_mut()
            .find(|node| node.id == "compute")
            .unwrap()
            .requires
            .swap(1, 2);
        let actions = store.actions("alice", "case").unwrap();
        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0].phase, EffectPhase::Unknown);
        let retained = state.result.as_ref().unwrap();
        let mut old_graph = state.clone();
        let mut wrong_goal = goal.clone();
        wrong_goal.required_graph_ref = store.put_artifact("alice", "case", &nodes).unwrap();
        old_graph.goal_sha256 = Some(store.put_artifact("alice", "case", &wrong_goal).unwrap());
        let error = inspect(&store, "alice", &old_graph, retained).unwrap_err();
        assert!(
            error.to_string().contains("UNSUPPORTED_RECOVERY_GRAPH"),
            "{error:?}"
        );
        let mut wrong_marker = state.clone();
        let mut wrong_goal = goal;
        wrong_goal.required_predicates[0] = SEQUENCE_REMOVE_PREDICATE.into();
        wrong_marker.goal_sha256 = Some(store.put_artifact("alice", "case", &wrong_goal).unwrap());
        let error = inspect(&store, "alice", &wrong_marker, retained).unwrap_err();
        assert!(
            error.to_string().contains("UNSUPPORTED_RECOVERY_GOAL"),
            "{error:?}"
        );
        assert_eq!(store.actions("alice", "case").unwrap(), actions);
        assert_eq!(fs::read(&fixture.target).unwrap(), bytes);
        assert_eq!(
            fs::metadata(&fixture.target).unwrap().modified().unwrap(),
            modified
        );

        let recovered = reconcile(&store, "alice", "case").unwrap();
        assert_eq!(recovered.lifecycle, Lifecycle::Commit, "{recovered:?}");
        assert_eq!(recovered.calculation, result.calculation);
        assert_eq!(recovered.dependencies, result.dependencies);
        assert_eq!(recovered.content_plan, result.content_plan);
        assert_eq!(recovered.render, result.render);
        assert_eq!(recovered.receipts[0].action_id, token);
        assert_eq!(
            recovered.receipts[0].reason.as_deref(),
            Some("LOCAL_FILE_RECONCILED_NO_RESEND")
        );
        let mut expected_graph = moved_graph;
        expected_graph[1]["file"] = digest(&canonical(&recovered.receipts[0]).unwrap()).into();
        assert_eq!(read_graph(&store, &recovered), expected_graph);
        let confirmed = store.actions("alice", "case").unwrap();
        assert_eq!(confirmed.len(), 1);
        assert_eq!(confirmed[0].phase, EffectPhase::Confirmed);
        drop(store);
        let mut engine = Engine::open(&fixture.state).unwrap();
        assert_eq!(engine.execute("alice", request).unwrap(), recovered);
        assert_eq!(engine.reconcile("alice", "case").unwrap(), recovered);
        assert_eq!(engine.execute("alice", original_request).unwrap(), original);
        assert_eq!(read_graph(&engine.store, &original), original_graph);
        assert_eq!(engine.store.actions("alice", "case").unwrap(), confirmed);
        assert_eq!(fs::read(&fixture.target).unwrap(), bytes);
        assert_eq!(
            fs::metadata(&fixture.target).unwrap().modified().unwrap(),
            modified
        );
    }

    #[test]
    fn inserted_sequence_step_reconciles_expanded_graph_without_resend() {
        let read_graph = |store: &Store, result: &ResultRecord| -> serde_json::Value {
            serde_json::from_slice(
                &store
                    .get_bytes(
                        "alice",
                        "case",
                        result.execution_graph_ref.as_ref().unwrap(),
                    )
                    .unwrap(),
            )
            .unwrap()
        };
        let fixture = empty_fixture();
        let mut engine = Engine::open(&fixture.state).unwrap();
        let original_request = RequestEnvelope::new(
            "case",
            "sequence",
            "alice",
            "計算手順を実行して:\nbase: 2+3\nfinal: @{base}*2",
        );
        let original = engine.execute("alice", original_request.clone()).unwrap();
        assert_eq!(original.lifecycle, Lifecycle::Commit, "{original:?}");
        let original_graph = read_graph(&engine.store, &original);
        let mut request = RequestEnvelope::new(
            "case",
            "insert",
            "alice",
            "計算手順の fee を final の前に挿入: @{base}+2",
        );
        request.budget.max_actions = 9;
        request.output_path = Some(fixture.target.clone());
        request
            .permissions
            .write_paths
            .insert(fixture.target.clone());
        let result = engine.execute("alice", request.clone()).unwrap();
        assert_eq!(result.lifecycle, Lifecycle::Commit, "{result:?}");
        assert_eq!(result.calculation.as_ref().unwrap().exact_value, "10");
        let inserted_graph = read_graph(&engine.store, &result);
        assert_eq!(inserted_graph.as_array().unwrap().len(), 6);
        let order: Vec<_> = inserted_graph[0]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|node| node["id"].as_str())
            .filter(|id| id.starts_with("sequence-"))
            .collect();
        assert_eq!(order, ["sequence-base", "sequence-fee", "sequence-final"]);
        for id in ["sequence-base", "sequence-final"] {
            assert_eq!(inserted_graph[1][id], original_graph[1][id]);
        }
        assert_eq!(
            inserted_graph[5]["sequence-fee"]["sequence-base"],
            inserted_graph[1]["sequence-base"]
        );
        let fee: serde_json::Value = serde_json::from_slice(
            &engine
                .store
                .get_bytes(
                    "alice",
                    "case",
                    inserted_graph[1]["sequence-fee"].as_str().unwrap(),
                )
                .unwrap(),
        )
        .unwrap();
        assert_eq!(fee["calculation"]["exact_value"], "7");
        let bytes = fs::read(&fixture.target).unwrap();
        let modified = fs::metadata(&fixture.target).unwrap().modified().unwrap();
        drop(engine);

        let token = retain_attempted_prefix(&fixture.state);
        let store = Store::open(&fixture.state).unwrap();
        let state = store.load_case("alice", "case").unwrap().unwrap();
        let bundle = state.bundle.as_ref().unwrap();
        assert_eq!(
            bundle.meaning.parser_version,
            crate::input::SEQUENCE_INSERT_PARSER_VERSION
        );
        assert!(matches!(
            bundle.meaning.intent,
            Intent::InsertCalculationSequenceStep { before: true, .. }
        ));
        let goal: GoalContract = serde_json::from_slice(
            &store
                .get_bytes("alice", "case", state.goal_sha256.as_ref().unwrap())
                .unwrap(),
        )
        .unwrap();
        assert_eq!(goal.required_predicates[0], SEQUENCE_INSERT_PREDICATE);
        let mut nodes: Vec<GoalNode> = serde_json::from_slice(
            &store
                .get_bytes("alice", "case", &goal.required_graph_ref)
                .unwrap(),
        )
        .unwrap();
        nodes.retain(|node| node.id != "sequence-fee");
        nodes
            .iter_mut()
            .find(|node| node.id == "compute")
            .unwrap()
            .requires
            .retain(|id| id != "sequence-fee");
        let actions = store.actions("alice", "case").unwrap();
        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0].phase, EffectPhase::Unknown);
        let retained = state.result.as_ref().unwrap();
        let mut old_graph = state.clone();
        let mut wrong_goal = goal.clone();
        wrong_goal.required_graph_ref = store.put_artifact("alice", "case", &nodes).unwrap();
        old_graph.goal_sha256 = Some(store.put_artifact("alice", "case", &wrong_goal).unwrap());
        let error = inspect(&store, "alice", &old_graph, retained).unwrap_err();
        assert!(
            error.to_string().contains("UNSUPPORTED_RECOVERY_GRAPH"),
            "{error:?}"
        );
        let mut wrong_marker = state.clone();
        let mut wrong_goal = goal;
        wrong_goal.required_predicates[0] = SEQUENCE_APPEND_PREDICATE.into();
        wrong_marker.goal_sha256 = Some(store.put_artifact("alice", "case", &wrong_goal).unwrap());
        let error = inspect(&store, "alice", &wrong_marker, retained).unwrap_err();
        assert!(
            error.to_string().contains("UNSUPPORTED_RECOVERY_GOAL"),
            "{error:?}"
        );
        assert_eq!(store.actions("alice", "case").unwrap(), actions);
        assert_eq!(fs::read(&fixture.target).unwrap(), bytes);
        assert_eq!(
            fs::metadata(&fixture.target).unwrap().modified().unwrap(),
            modified
        );

        let recovered = reconcile(&store, "alice", "case").unwrap();
        assert_eq!(recovered.lifecycle, Lifecycle::Commit, "{recovered:?}");
        assert_eq!(recovered.calculation, result.calculation);
        assert_eq!(recovered.dependencies, result.dependencies);
        assert_eq!(recovered.content_plan, result.content_plan);
        assert_eq!(recovered.render, result.render);
        assert_eq!(recovered.receipts[0].action_id, token);
        assert_eq!(
            recovered.receipts[0].reason.as_deref(),
            Some("LOCAL_FILE_RECONCILED_NO_RESEND")
        );
        let mut expected_graph = inserted_graph;
        expected_graph[1]["file"] = digest(&canonical(&recovered.receipts[0]).unwrap()).into();
        assert_eq!(read_graph(&store, &recovered), expected_graph);
        let confirmed = store.actions("alice", "case").unwrap();
        assert_eq!(confirmed.len(), 1);
        assert_eq!(confirmed[0].phase, EffectPhase::Confirmed);
        drop(store);
        let mut engine = Engine::open(&fixture.state).unwrap();
        assert_eq!(engine.execute("alice", request).unwrap(), recovered);
        assert_eq!(engine.reconcile("alice", "case").unwrap(), recovered);
        assert_eq!(engine.execute("alice", original_request).unwrap(), original);
        assert_eq!(read_graph(&engine.store, &original), original_graph);
        assert_eq!(engine.store.actions("alice", "case").unwrap(), confirmed);
        assert_eq!(fs::read(&fixture.target).unwrap(), bytes);
        assert_eq!(
            fs::metadata(&fixture.target).unwrap().modified().unwrap(),
            modified
        );
    }

    #[test]
    fn calculation_sequence_history_reconciles_ordered_units_without_resend() {
        let read_graph = |store: &Store, result: &ResultRecord| -> serde_json::Value {
            serde_json::from_slice(
                &store
                    .get_bytes(
                        "alice",
                        "case",
                        result.execution_graph_ref.as_ref().unwrap(),
                    )
                    .unwrap(),
            )
            .unwrap()
        };
        let fixture = empty_fixture();
        let mut engine = Engine::open(&fixture.state).unwrap();
        let original_request = RequestEnvelope::new(
            "case",
            "sequence",
            "alice",
            "計算手順を実行して:\nbase: 2+3\nfinal: @{base}*2",
        );
        let original = engine.execute("alice", original_request.clone()).unwrap();
        assert_eq!(original.lifecycle, Lifecycle::Commit, "{original:?}");
        let inserted = engine
            .execute(
                "alice",
                RequestEnvelope::new(
                    "case",
                    "insert",
                    "alice",
                    "計算手順の fee を final の前に挿入: @{base}+2",
                ),
            )
            .unwrap();
        assert_eq!(inserted.lifecycle, Lifecycle::Commit, "{inserted:?}");
        let mut request =
            RequestEnvelope::new("case", "history", "alice", "計算手順の変更履歴を表示");
        request.budget.max_actions = 9;
        request.output_path = Some(fixture.target.clone());
        request
            .permissions
            .write_paths
            .insert(fixture.target.clone());
        let result = engine.execute("alice", request.clone()).unwrap();
        assert_eq!(result.lifecycle, Lifecycle::Commit, "{result:?}");
        assert_eq!(result.calculation, inserted.calculation);
        assert!(result.dependencies.contains(&original.id));
        assert!(result.dependencies.contains(&inserted.id));
        let units = &result.content_plan.as_ref().unwrap().required_units;
        assert!(units.iter().any(|unit| unit.contains("fee")));
        assert_eq!(units.join("\n"), result.text);
        let history_graph = read_graph(&engine.store, &result);
        assert_eq!(history_graph.as_array().unwrap().len(), 6);
        let order: Vec<_> = history_graph[0]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|node| node["id"].as_str())
            .filter(|id| id.starts_with("sequence-"))
            .collect();
        assert_eq!(order, ["sequence-base", "sequence-fee", "sequence-final"]);
        let bytes = fs::read(&fixture.target).unwrap();
        let modified = fs::metadata(&fixture.target).unwrap().modified().unwrap();
        drop(engine);

        let token = retain_attempted_prefix(&fixture.state);
        let store = Store::open(&fixture.state).unwrap();
        let state = store.load_case("alice", "case").unwrap().unwrap();
        let bundle = state.bundle.as_ref().unwrap();
        assert_eq!(
            bundle.meaning.parser_version,
            crate::input::SEQUENCE_HISTORY_PARSER_VERSION
        );
        assert!(matches!(
            bundle.meaning.intent,
            Intent::ShowCalculationSequenceHistory
        ));
        let goal: GoalContract = serde_json::from_slice(
            &store
                .get_bytes("alice", "case", state.goal_sha256.as_ref().unwrap())
                .unwrap(),
        )
        .unwrap();
        assert_eq!(goal.required_predicates[0], SEQUENCE_HISTORY_PREDICATE);
        let actions = store.actions("alice", "case").unwrap();
        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0].phase, EffectPhase::Unknown);
        let retained = state.result.as_ref().unwrap();
        let mut wrong_units = retained.clone();
        let units = &mut wrong_units.content_plan.as_mut().unwrap().required_units;
        assert_ne!(units[0], units[1]);
        units.swap(0, 1);
        assert!(inspect(&store, "alice", &state, &wrong_units).is_err());
        let mut wrong_dependencies = retained.clone();
        wrong_dependencies.dependencies.swap(0, 1);
        let error = inspect(&store, "alice", &state, &wrong_dependencies).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("RECOVERY_CALCULATION_SEQUENCE_CONTEXT_INVALID"),
            "{error:?}"
        );
        for marker in [Some(INSERTED_SEQUENCE_EXPLANATION_PREDICATE), None] {
            let mut wrong_marker = state.clone();
            let mut wrong_goal = goal.clone();
            if let Some(marker) = marker {
                wrong_goal.required_predicates[0] = marker.into();
            } else {
                wrong_goal.required_predicates.remove(0);
            }
            wrong_marker.goal_sha256 =
                Some(store.put_artifact("alice", "case", &wrong_goal).unwrap());
            let error = inspect(&store, "alice", &wrong_marker, retained).unwrap_err();
            assert!(
                error.to_string().contains("UNSUPPORTED_RECOVERY_GOAL"),
                "{error:?}"
            );
        }
        assert_eq!(store.actions("alice", "case").unwrap(), actions);
        assert_eq!(fs::read(&fixture.target).unwrap(), bytes);
        assert_eq!(
            fs::metadata(&fixture.target).unwrap().modified().unwrap(),
            modified
        );

        let recovered = reconcile(&store, "alice", "case").unwrap();
        assert_eq!(recovered.lifecycle, Lifecycle::Commit, "{recovered:?}");
        assert_eq!(recovered.calculation, result.calculation);
        assert_eq!(recovered.dependencies, result.dependencies);
        assert_eq!(recovered.content_plan, result.content_plan);
        assert_eq!(recovered.render, result.render);
        assert_eq!(recovered.receipts[0].action_id, token);
        assert_eq!(
            recovered.receipts[0].reason.as_deref(),
            Some("LOCAL_FILE_RECONCILED_NO_RESEND")
        );
        let mut expected_graph = history_graph;
        expected_graph[1]["file"] = digest(&canonical(&recovered.receipts[0]).unwrap()).into();
        assert_eq!(read_graph(&store, &recovered), expected_graph);
        let confirmed = store.actions("alice", "case").unwrap();
        assert_eq!(confirmed.len(), 1);
        assert_eq!(confirmed[0].phase, EffectPhase::Confirmed);
        drop(store);
        let mut engine = Engine::open(&fixture.state).unwrap();
        assert_eq!(engine.execute("alice", request).unwrap(), recovered);
        assert_eq!(engine.reconcile("alice", "case").unwrap(), recovered);
        assert_eq!(engine.execute("alice", original_request).unwrap(), original);
        assert_eq!(engine.store.actions("alice", "case").unwrap(), confirmed);
        assert_eq!(fs::read(&fixture.target).unwrap(), bytes);
        assert_eq!(
            fs::metadata(&fixture.target).unwrap().modified().unwrap(),
            modified
        );
    }

    #[test]
    fn condition_evaluation_reconciles_pinned_proof_and_rejects_relevant_changes() {
        let read_graph = |store: &Store, result: &ResultRecord| -> serde_json::Value {
            serde_json::from_slice(
                &store
                    .get_bytes(
                        "alice",
                        "case",
                        result.execution_graph_ref.as_ref().unwrap(),
                    )
                    .unwrap(),
            )
            .unwrap()
        };
        for invalidate in [false, true] {
            let fixture = empty_fixture();
            let mut engine = Engine::open(&fixture.state).unwrap();
            for (id, text) in [
                (
                    "condition",
                    "条件定義: [scope=実験室][condition=校正] ユリは校正済み",
                ),
                ("fact", "[scope=実験室]ユリは校正済み"),
            ] {
                let result = engine
                    .execute("alice", RequestEnvelope::new("case", id, "alice", text))
                    .unwrap();
                assert_eq!(result.lifecycle, Lifecycle::Commit, "{result:?}");
            }
            let mut request = RequestEnvelope::new(
                "case",
                "evaluation",
                "alice",
                "条件評価: [condition=校正][subject=ユリ][scope=実験室]",
            );
            let preview = engine
                .execute(
                    "alice",
                    RequestEnvelope::new("case", "preview", "alice", &request.text),
                )
                .unwrap();
            assert_eq!(preview.lifecycle, Lifecycle::Commit, "{preview:?}");
            let proof_bytes = engine
                .store
                .get_bytes("alice", "case", preview.dependencies.last().unwrap())
                .unwrap();
            request.budget.max_evidence_bytes = proof_bytes.len() + preview.text.len();
            request.output_path = Some(fixture.target.clone());
            request
                .permissions
                .write_paths
                .insert(fixture.target.clone());
            let result = engine.execute("alice", request.clone()).unwrap();
            assert_eq!(result.lifecycle, Lifecycle::Commit, "{result:?}");
            assert_eq!(result.text.lines().next(), Some("条件 校正: TRUE"));
            assert_eq!(result.guarantee, Guarantee::Computed);
            assert!(result.calculation.is_none());
            assert!(result.evidence.is_empty());
            let revision = result.actor_memory_revision.unwrap();
            let proof: (String, u64, ConditionEvaluation) = serde_json::from_slice(
                &engine
                    .store
                    .get_bytes("alice", "case", result.dependencies.last().unwrap())
                    .unwrap(),
            )
            .unwrap();
            assert_eq!(proof.0, "minidora-condition-evaluation-v1");
            assert_eq!(proof.1, revision);
            assert_eq!(proof.2.status, ConditionStatus::True);
            assert_eq!(proof.2.definition_record_ids.len(), 1);
            assert_eq!(proof.2.evidence_fact_ids.len(), 1);
            assert_eq!(
                &result.dependencies[..result.dependencies.len() - 1],
                proof.2.dependencies
            );
            let graph = read_graph(&engine.store, &result);
            assert_eq!(graph.as_array().unwrap().len(), 6);
            let node_ids: Vec<_> = graph[0]
                .as_array()
                .unwrap()
                .iter()
                .map(|node| node["id"].as_str().unwrap())
                .collect();
            assert_eq!(node_ids, ["compute", "content", "render", "file"]);
            let bytes = fs::read(&fixture.target).unwrap();
            let modified = fs::metadata(&fixture.target).unwrap().modified().unwrap();
            drop(engine);

            let token = retain_attempted_prefix(&fixture.state);
            let mut engine = Engine::open(&fixture.state).unwrap();
            let state = engine.store.load_case("alice", "case").unwrap().unwrap();
            assert_eq!(
                state.bundle.as_ref().unwrap().meaning.parser_version,
                crate::input::CONDITION_EVALUATION_PARSER_VERSION
            );
            let retained = state.result.as_ref().unwrap();
            let goal: GoalContract = serde_json::from_slice(
                &engine
                    .store
                    .get_bytes("alice", "case", state.goal_sha256.as_ref().unwrap())
                    .unwrap(),
            )
            .unwrap();
            assert_eq!(
                goal.required_predicates[0],
                crate::core::CONDITION_EVALUATION_PREDICATE
            );
            for marker in [Some("EXACT_CALCULATION_V1"), None] {
                let mut wrong_state = state.clone();
                let mut wrong_goal = goal.clone();
                if let Some(marker) = marker {
                    wrong_goal.required_predicates[0] = marker.into();
                } else {
                    wrong_goal.required_predicates.remove(0);
                }
                wrong_state.goal_sha256 = Some(
                    engine
                        .store
                        .put_artifact("alice", "case", &wrong_goal)
                        .unwrap(),
                );
                let error = inspect(&engine.store, "alice", &wrong_state, retained).unwrap_err();
                assert!(
                    error.to_string().contains("UNSUPPORTED_RECOVERY_GOAL"),
                    "{error:?}"
                );
            }
            let mut no_proof = retained.clone();
            no_proof.dependencies.pop();
            let error = inspect(&engine.store, "alice", &state, &no_proof).unwrap_err();
            assert!(
                error
                    .to_string()
                    .contains("RECOVERY_CONDITION_EVALUATION_MISMATCH"),
                "{error:?}"
            );
            let mut no_revision = retained.clone();
            no_revision.actor_memory_revision = None;
            let error = inspect(&engine.store, "alice", &state, &no_revision).unwrap_err();
            assert!(
                error
                    .to_string()
                    .contains("RECOVERY_CONDITION_EVALUATION_MISMATCH"),
                "{error:?}"
            );
            let actions = engine.store.actions("alice", "case").unwrap();
            assert_eq!(actions.len(), 1);
            assert_eq!(actions[0].phase, EffectPhase::Unknown);

            // The head revision needs another digit. Reconciliation verifies
            // the original report bytes under the original budget.
            assert!(revision < 10);
            for index in revision..10 {
                let unrelated = engine
                    .execute(
                        "alice",
                        RequestEnvelope::new(
                            "other-case",
                            &format!("unrelated-{index}"),
                            "alice",
                            &format!("[scope=archive]ナナは項目{index}です"),
                        ),
                    )
                    .unwrap();
                assert_eq!(unrelated.lifecycle, Lifecycle::Commit, "{unrelated:?}");
            }
            assert_eq!(engine.store.load_memory("alice").unwrap().revision, 10);
            inspect(&engine.store, "alice", &state, retained).unwrap();
            if invalidate {
                let opposite = engine
                    .execute(
                        "alice",
                        RequestEnvelope::new(
                            "other-case",
                            "opposite",
                            "alice",
                            "[scope=実験室]ユリは校正済みではない",
                        ),
                    )
                    .unwrap();
                assert_eq!(opposite.lifecycle, Lifecycle::Commit, "{opposite:?}");
                assert_eq!(
                    crate::core::condition_report_at_head(&engine.store, "alice", &goal)
                        .unwrap()
                        .evaluation
                        .status,
                    ConditionStatus::Conflict
                );
                let error = inspect(&engine.store, "alice", &state, retained).unwrap_err();
                assert!(
                    error
                        .to_string()
                        .contains("RECOVERY_CONDITION_EVALUATION_CHANGED"),
                    "{error:?}"
                );
            }
            let recovered = engine.reconcile("alice", "case").unwrap();
            if invalidate {
                assert_eq!(recovered.lifecycle, Lifecycle::Suspend, "{recovered:?}");
                assert!(
                    recovered
                        .reason
                        .as_deref()
                        .unwrap()
                        .contains("RECOVERY_CONDITION_EVALUATION_CHANGED")
                );
                assert_eq!(engine.store.actions("alice", "case").unwrap(), actions);
            } else {
                assert_eq!(recovered.lifecycle, Lifecycle::Commit, "{recovered:?}");
                assert_eq!(recovered.actor_memory_revision, Some(revision));
                assert_eq!(recovered.dependencies, result.dependencies);
                assert_eq!(recovered.content_plan, result.content_plan);
                assert_eq!(recovered.render, result.render);
                assert_eq!(recovered.receipts[0].action_id, token);
                assert_eq!(
                    recovered.receipts[0].reason.as_deref(),
                    Some("LOCAL_FILE_RECONCILED_NO_RESEND")
                );
                let mut expected_graph = graph;
                expected_graph[1]["file"] =
                    digest(&canonical(&recovered.receipts[0]).unwrap()).into();
                assert_eq!(read_graph(&engine.store, &recovered), expected_graph);
                let confirmed = engine.store.actions("alice", "case").unwrap();
                assert_eq!(confirmed.len(), 1);
                assert_eq!(confirmed[0].phase, EffectPhase::Confirmed);
                assert_eq!(confirmed[0].idempotency_key, token);
            }
            let final_actions = engine.store.actions("alice", "case").unwrap();
            assert_eq!(engine.execute("alice", request).unwrap(), recovered);
            assert_eq!(
                engine.store.actions("alice", "case").unwrap(),
                final_actions
            );
            assert_eq!(fs::read(&fixture.target).unwrap(), bytes);
            assert_eq!(
                fs::metadata(&fixture.target).unwrap().modified().unwrap(),
                modified
            );
        }
    }

    #[test]
    fn fact_source_reconciles_original_input_without_resend() {
        let fixture = empty_fixture();
        let mut engine = Engine::open(&fixture.state).unwrap();
        let asserted = engine
            .execute(
                "alice",
                RequestEnvelope::new("case", "assertion", "alice", "[scope=実験室]ユリは校正済み"),
            )
            .unwrap();
        assert_eq!(asserted.lifecycle, Lifecycle::Commit, "{asserted:?}");
        let command = format!("Factの出典: {}", asserted.dependencies[0]);
        let mut request = RequestEnvelope::new("case", "provenance", "alice", &command);
        request.budget.max_actions = 6;
        request.output_path = Some(fixture.target.clone());
        request
            .permissions
            .write_paths
            .insert(fixture.target.clone());
        let result = engine.execute("alice", request.clone()).unwrap();
        assert_eq!(result.lifecycle, Lifecycle::Commit, "{result:?}");
        assert_eq!(result.dependencies[1], asserted.id);
        let bytes = fs::read(&fixture.target).unwrap();
        let modified = fs::metadata(&fixture.target).unwrap().modified().unwrap();
        drop(engine);

        let token = retain_attempted_prefix(&fixture.state);
        let mut engine = Engine::open(&fixture.state).unwrap();
        let state = engine.store.load_case("alice", "case").unwrap().unwrap();
        let retained = state.result.as_ref().unwrap();
        let mut changed = retained.clone();
        changed.dependencies[1] = changed.dependencies[0].clone();
        let error = inspect(&engine.store, "alice", &state, &changed).unwrap_err();
        assert!(
            error.to_string().contains("RECOVERY_FACT_SOURCE_MISMATCH"),
            "{error:?}"
        );
        let unrelated = engine
            .execute(
                "alice",
                RequestEnvelope::new(
                    "other",
                    "unrelated",
                    "alice",
                    "[scope=archive]ナナは記録済み",
                ),
            )
            .unwrap();
        assert_eq!(unrelated.lifecycle, Lifecycle::Commit, "{unrelated:?}");
        let recovered = engine.reconcile("alice", "case").unwrap();
        assert_eq!(recovered.lifecycle, Lifecycle::Commit, "{recovered:?}");
        assert_eq!(
            recovered.actor_memory_revision,
            result.actor_memory_revision
        );
        assert_eq!(recovered.receipts[0].action_id, token);
        assert_eq!(
            recovered.receipts[0].reason.as_deref(),
            Some("LOCAL_FILE_RECONCILED_NO_RESEND")
        );
        assert_eq!(fs::read(&fixture.target).unwrap(), bytes);
        assert_eq!(
            fs::metadata(&fixture.target).unwrap().modified().unwrap(),
            modified
        );
        assert_eq!(engine.execute("alice", request).unwrap(), recovered);
        assert_eq!(engine.store.actions("alice", "case").unwrap().len(), 1);
    }

    #[test]
    fn multi_condition_premise_reconciles_exact_registered_evidence_without_resend() {
        let fixture = empty_fixture();
        let mut request = RequestEnvelope::new(
            "case",
            "multiple-conditions",
            "alice",
            "[scope=実験室][condition=点検][condition=校正]ユリは入室可",
        );
        request.budget.max_actions = 4;
        request.output_path = Some(fixture.target.clone());
        request
            .permissions
            .write_paths
            .insert(fixture.target.clone());
        let mut engine = Engine::open(&fixture.state).unwrap();
        let result = engine.execute("alice", request.clone()).unwrap();
        assert_eq!(result.lifecycle, Lifecycle::Commit, "{result:?}");
        let bytes = fs::read(&fixture.target).unwrap();
        let modified = fs::metadata(&fixture.target).unwrap().modified().unwrap();
        drop(engine);

        let token = retain_attempted_prefix(&fixture.state);
        let mut engine = Engine::open(&fixture.state).unwrap();
        let state = engine.store.load_case("alice", "case").unwrap().unwrap();
        let mut altered = state.result.as_ref().unwrap().clone();
        altered.dependencies.clear();
        let error = inspect(&engine.store, "alice", &state, &altered).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("RECOVERY_MULTI_CONDITION_PREMISE_MISMATCH"),
            "{error:?}"
        );
        let unrelated = engine
            .execute(
                "alice",
                RequestEnvelope::new("other", "unrelated", "alice", "[scope=別室]ユリは待機中"),
            )
            .unwrap();
        assert_eq!(unrelated.lifecycle, Lifecycle::Commit, "{unrelated:?}");
        let recovered = engine.reconcile("alice", "case").unwrap();
        assert_eq!(recovered.lifecycle, Lifecycle::Commit, "{recovered:?}");
        assert_eq!(recovered.dependencies, result.dependencies);
        assert_eq!(
            recovered.actor_memory_revision,
            result.actor_memory_revision
        );
        assert_eq!(recovered.receipts[0].action_id, token);
        assert_eq!(
            recovered.receipts[0].reason.as_deref(),
            Some("LOCAL_FILE_RECONCILED_NO_RESEND")
        );
        assert_eq!(fs::read(&fixture.target).unwrap(), bytes);
        assert_eq!(
            fs::metadata(&fixture.target).unwrap().modified().unwrap(),
            modified
        );
        assert_eq!(engine.execute("alice", request).unwrap(), recovered);
        assert_eq!(engine.store.actions("alice", "case").unwrap().len(), 1);
    }

    #[test]
    fn guarded_rule_registration_reconciles_the_exact_persisted_rule_without_resend() {
        assert_guarded_rule_registration_reconciles(
            "もし[scope=実験室][condition=点検]Xは研究員なら[scope=実験室][condition=点検]Xは入室可",
            crate::core::GUARDED_RELATION_RULE_PREDICATE,
        );
    }

    #[test]
    fn guarded_conjunctive_registration_reconciles_the_ordered_rule_without_resend() {
        assert_guarded_rule_registration_reconciles(
            concat!(
                "もし[scope=実験室][condition=点検]Xは研究員 かつ ",
                "[scope=実験室][condition=点検]Xは当番なら[scope=実験室][condition=点検]Xは入室可"
            ),
            crate::core::GUARDED_CONJUNCTIVE_RULE_PREDICATE,
        );
    }

    #[test]
    fn multi_condition_rule_registration_reconciles_order_and_proof_without_resend() {
        assert_guarded_rule_registration_reconciles(
            concat!(
                "もし[scope=実験室][condition=点検][condition=校正]Xは研究員 かつ ",
                "[scope=実験室][condition=校正][condition=点検]Xは当番なら",
                "[scope=実験室][condition=点検][condition=校正]Xは入室可"
            ),
            crate::core::MULTI_CONDITION_RULE_PREDICATE,
        );
    }

    fn assert_guarded_rule_registration_reconciles(declaration: &str, predicate: &str) {
        let fixture = empty_fixture();
        let mut engine = Engine::open(&fixture.state).unwrap();
        let mut request = RequestEnvelope::new("case", "guarded-rule", "alice", declaration);
        request.budget.max_actions = 4;
        request.output_path = Some(fixture.target.clone());
        request
            .permissions
            .write_paths
            .insert(fixture.target.clone());
        let result = engine.execute("alice", request.clone()).unwrap();
        assert_eq!(result.lifecycle, Lifecycle::Commit, "{result:?}");
        let bytes = fs::read(&fixture.target).unwrap();
        let modified = fs::metadata(&fixture.target).unwrap().modified().unwrap();
        drop(engine);

        let token = retain_attempted_prefix(&fixture.state);
        let mut engine = Engine::open(&fixture.state).unwrap();
        let state = engine.store.load_case("alice", "case").unwrap().unwrap();
        let goal_bytes = engine
            .store
            .get_bytes("alice", "case", state.goal_sha256.as_deref().unwrap())
            .unwrap();
        let mut goal: GoalContract = serde_json::from_slice(&goal_bytes).unwrap();
        assert_eq!(goal.required_predicates[0], predicate);
        if predicate == crate::core::MULTI_CONDITION_RULE_PREDICATE {
            goal.required_predicates[0] = crate::core::GUARDED_CONJUNCTIVE_RULE_PREDICATE.into();
            let mut replaced = state.clone();
            replaced.goal_sha256 = Some(engine.store.put_artifact("alice", "case", &goal).unwrap());
            let error = inspect(
                &engine.store,
                "alice",
                &replaced,
                state.result.as_ref().unwrap(),
            )
            .unwrap_err();
            assert!(
                error.to_string().contains("UNSUPPORTED_RECOVERY_GOAL"),
                "{error:?}"
            );
        }
        let mut altered = state.result.as_ref().unwrap().clone();
        let bundle = state.bundle.as_ref().unwrap();
        if let Intent::AddConjunctiveRule {
            premises,
            conclusion,
        } = &bundle.meaning.intent
        {
            let source_input = identity(&request).unwrap();
            let reversed = [premises[1].clone(), premises[0].clone()];
            altered.dependencies[0] =
                crate::relations::declared_rule_id(&reversed, conclusion, &source_input).unwrap();
        } else {
            altered.actor_memory_revision = None;
        }
        let error = inspect(&engine.store, "alice", &state, &altered).unwrap_err();
        assert!(
            error.to_string().contains("RECOVERY_GUARDED_RULE_MISMATCH"),
            "{error:?}"
        );
        let unrelated = engine
            .execute(
                "alice",
                RequestEnvelope::new("other", "unrelated", "alice", "[scope=別室]ユリは待機中"),
            )
            .unwrap();
        assert_eq!(unrelated.lifecycle, Lifecycle::Commit, "{unrelated:?}");
        let confirmed = engine.reconcile("alice", "case").unwrap();
        assert_eq!(confirmed.lifecycle, Lifecycle::Commit, "{confirmed:?}");
        assert_eq!(confirmed.dependencies, result.dependencies);
        assert_eq!(
            confirmed.actor_memory_revision,
            result.actor_memory_revision
        );
        assert_eq!(confirmed.receipts[0].action_id, token);
        assert_eq!(
            confirmed.receipts[0].reason.as_deref(),
            Some("LOCAL_FILE_RECONCILED_NO_RESEND")
        );
        assert_eq!(fs::read(&fixture.target).unwrap(), bytes);
        assert_eq!(
            fs::metadata(&fixture.target).unwrap().modified().unwrap(),
            modified
        );
        assert_eq!(engine.execute("alice", request).unwrap(), confirmed);
        assert_eq!(engine.store.actions("alice", "case").unwrap().len(), 1);
        assert_eq!(engine.store.load_memory("alice").unwrap().rules.len(), 1);
    }

    #[test]
    fn condition_source_reconciles_original_input_without_resend() {
        assert_condition_source_reconciles_without_resend(false);
    }

    #[test]
    fn condition_source_by_definition_reconciles_original_input_without_resend() {
        assert_condition_source_reconciles_without_resend(true);
    }

    fn assert_condition_source_reconciles_without_resend(by_definition: bool) {
        let fixture = empty_fixture();
        let mut engine = Engine::open(&fixture.state).unwrap();
        let defined = engine
            .execute(
                "alice",
                RequestEnvelope::new(
                    "case",
                    "definition",
                    "alice",
                    "条件定義: [scope=実験室][condition=点検] ユリは点検済み",
                ),
            )
            .unwrap();
        assert_eq!(defined.lifecycle, Lifecycle::Commit, "{defined:?}");
        let command = if by_definition {
            "条件定義の出典: [condition=点検][subject=ユリ][scope=実験室]".into()
        } else {
            format!("条件定義の出典: {}", defined.dependencies[0])
        };
        let mut request = RequestEnvelope::new("case", "provenance", "alice", &command);
        request.budget.max_actions = 6;
        request.output_path = Some(fixture.target.clone());
        request
            .permissions
            .write_paths
            .insert(fixture.target.clone());
        let result = engine.execute("alice", request.clone()).unwrap();
        assert_eq!(result.lifecycle, Lifecycle::Commit, "{result:?}");
        assert_eq!(result.dependencies[1], defined.id);
        let bytes = fs::read(&fixture.target).unwrap();
        let modified = fs::metadata(&fixture.target).unwrap().modified().unwrap();
        drop(engine);

        let token = retain_attempted_prefix(&fixture.state);
        let mut engine = Engine::open(&fixture.state).unwrap();
        let state = engine.store.load_case("alice", "case").unwrap().unwrap();
        let mut altered = state.result.as_ref().unwrap().clone();
        altered.dependencies[1] = altered.dependencies[0].clone();
        let error = inspect(&engine.store, "alice", &state, &altered).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("RECOVERY_CONDITION_SOURCE_MISMATCH"),
            "{error:?}"
        );
        if by_definition {
            let original_goal_ref = state.goal_sha256.as_deref().unwrap();
            let mut goal: GoalContract = serde_json::from_slice(
                &engine
                    .store
                    .get_bytes("alice", "case", original_goal_ref)
                    .unwrap(),
            )
            .unwrap();
            assert_eq!(
                goal.required_predicates[0],
                crate::core::CONDITION_SOURCE_BY_DEFINITION_PREDICATE
            );
            goal.required_predicates[0] = crate::core::CONDITION_SOURCE_PREDICATE.into();
            let mut substituted = state.clone();
            substituted.goal_sha256 =
                Some(engine.store.put_artifact("alice", "case", &goal).unwrap());
            let error = inspect(
                &engine.store,
                "alice",
                &substituted,
                state.result.as_ref().unwrap(),
            )
            .unwrap_err();
            assert!(
                error.to_string().contains("UNSUPPORTED_RECOVERY_GOAL"),
                "{error:?}"
            );
        }
        let unrelated = engine
            .execute(
                "alice",
                RequestEnvelope::new(
                    "other",
                    "unrelated",
                    "alice",
                    "[scope=archive]ナナは記録済み",
                ),
            )
            .unwrap();
        assert_eq!(unrelated.lifecycle, Lifecycle::Commit, "{unrelated:?}");
        let recovered = engine.reconcile("alice", "case").unwrap();
        assert_eq!(recovered.lifecycle, Lifecycle::Commit, "{recovered:?}");
        assert_eq!(
            recovered.actor_memory_revision,
            result.actor_memory_revision
        );
        assert_eq!(recovered.dependencies, result.dependencies);
        assert_eq!(recovered.receipts[0].action_id, token);
        assert_eq!(
            recovered.receipts[0].reason.as_deref(),
            Some("LOCAL_FILE_RECONCILED_NO_RESEND")
        );
        assert_eq!(fs::read(&fixture.target).unwrap(), bytes);
        assert_eq!(
            fs::metadata(&fixture.target).unwrap().modified().unwrap(),
            modified
        );
        assert_eq!(engine.execute("alice", request).unwrap(), recovered);
        assert_eq!(engine.store.actions("alice", "case").unwrap().len(), 1);
    }

    #[test]
    fn rule_source_reconciles_the_original_declaration_without_resend() {
        assert_rule_source_reconciliation(
            "もし[scope=実験室][condition=点検]Xは研究員なら[scope=実験室][condition=点検]Xは入室可",
            "minidora-rule-source-v1",
        );
    }

    #[test]
    fn plain_rule_source_reconciles_the_original_declaration_without_resend() {
        assert_rule_source_reconciliation(
            "もし[scope=実験室]Xは研究員なら[scope=実験室]Xは入室可",
            "minidora-plain-rule-source-v1",
        );
    }

    fn assert_rule_source_reconciliation(declaration: &str, proof_version: &str) {
        let fixture = empty_fixture();
        let mut engine = Engine::open(&fixture.state).unwrap();
        let declared = engine
            .execute(
                "alice",
                RequestEnvelope::new("case", "declaration", "alice", declaration),
            )
            .unwrap();
        assert_eq!(declared.lifecycle, Lifecycle::Commit, "{declared:?}");
        let command = format!("規則の出典: {}", declared.dependencies[0]);
        let mut request = RequestEnvelope::new("case", "provenance", "alice", &command);
        request.budget.max_actions = 6;
        request.output_path = Some(fixture.target.clone());
        request
            .permissions
            .write_paths
            .insert(fixture.target.clone());
        let report = engine.execute("alice", request.clone()).unwrap();
        assert_eq!(report.lifecycle, Lifecycle::Commit, "{report:?}");
        assert_eq!(report.dependencies[1], declared.id);
        let proof: serde_json::Value = serde_json::from_slice(
            &engine
                .store
                .get_bytes("alice", "case", &report.dependencies[2])
                .unwrap(),
        )
        .unwrap();
        assert_eq!(proof[0], proof_version);
        let bytes = fs::read(&fixture.target).unwrap();
        let modified = fs::metadata(&fixture.target).unwrap().modified().unwrap();
        drop(engine);

        let token = retain_attempted_prefix(&fixture.state);
        let mut engine = Engine::open(&fixture.state).unwrap();
        let state = engine.store.load_case("alice", "case").unwrap().unwrap();
        let retained = state.result.as_ref().unwrap();
        let mut goal: GoalContract = serde_json::from_slice(
            &engine
                .store
                .get_bytes("alice", "case", state.goal_sha256.as_deref().unwrap())
                .unwrap(),
        )
        .unwrap();
        assert_eq!(
            goal.required_predicates[0],
            crate::core::DECLARED_RULE_SOURCE_PREDICATE
        );
        goal.required_predicates[0] = crate::core::RULE_SOURCE_PREDICATE.into();
        let mut downgraded = state.clone();
        downgraded.goal_sha256 = Some(engine.store.put_artifact("alice", "case", &goal).unwrap());
        let error = inspect(&engine.store, "alice", &downgraded, retained).unwrap_err();
        assert!(
            error.to_string().contains("UNSUPPORTED_RECOVERY_GOAL"),
            "{error:?}"
        );
        let mut altered = retained.clone();
        altered.dependencies[1] = altered.dependencies[0].clone();
        let error = inspect(&engine.store, "alice", &state, &altered).unwrap_err();
        assert!(
            error.to_string().contains("RECOVERY_RULE_SOURCE_MISMATCH"),
            "{error:?}"
        );
        let unrelated = engine
            .execute(
                "alice",
                RequestEnvelope::new("other", "unrelated", "alice", "[scope=別室]ユリは待機中"),
            )
            .unwrap();
        assert_eq!(unrelated.lifecycle, Lifecycle::Commit, "{unrelated:?}");

        let recovered = engine.reconcile("alice", "case").unwrap();
        assert_eq!(recovered.lifecycle, Lifecycle::Commit, "{recovered:?}");
        assert_eq!(recovered.dependencies, report.dependencies);
        assert_eq!(
            recovered.actor_memory_revision,
            report.actor_memory_revision
        );
        assert_eq!(recovered.receipts[0].action_id, token);
        assert_eq!(
            recovered.receipts[0].reason.as_deref(),
            Some("LOCAL_FILE_RECONCILED_NO_RESEND")
        );
        assert_eq!(fs::read(&fixture.target).unwrap(), bytes);
        assert_eq!(
            fs::metadata(&fixture.target).unwrap().modified().unwrap(),
            modified
        );
        assert_eq!(engine.execute("alice", request).unwrap(), recovered);
        assert_eq!(engine.store.actions("alice", "case").unwrap().len(), 1);
    }

    #[test]
    fn previous_rule_source_recovery_rechecks_the_historical_why_without_resend() {
        let fixture = empty_fixture();
        let mut engine = Engine::open(&fixture.state).unwrap();
        let mut answers = Vec::new();
        for (id, text) in [
            ("rule", "もし[scope=lab]Xは鳥なら[scope=lab]Xは動物"),
            ("fact", "[scope=lab]シロは鳥"),
            ("why", "なぜ[scope=lab]シロは動物ですか"),
        ] {
            let result = engine
                .execute("alice", RequestEnvelope::new("case", id, "alice", text))
                .unwrap();
            assert_eq!(result.lifecycle, Lifecycle::Commit, "{result:?}");
            answers.push(result);
        }
        let previous = answers.last().unwrap();
        let mut request = RequestEnvelope::new(
            "case",
            "previous-rule",
            "alice",
            "前の回答で使った規則の出典",
        );
        request.budget.max_actions = 7;
        request.output_path = Some(fixture.target.clone());
        request
            .permissions
            .write_paths
            .insert(fixture.target.clone());
        let report = engine.execute("alice", request.clone()).unwrap();
        assert_eq!(report.lifecycle, Lifecycle::Commit, "{report:?}");
        assert_eq!(report.dependencies[0], previous.id);
        assert_eq!(report.dependencies[2], answers[0].dependencies[0]);
        let bytes = fs::read(&fixture.target).unwrap();
        let modified = fs::metadata(&fixture.target).unwrap().modified().unwrap();
        drop(engine);

        let token = retain_attempted_prefix(&fixture.state);
        let mut engine = Engine::open(&fixture.state).unwrap();
        let state = engine.store.load_case("alice", "case").unwrap().unwrap();
        let mut altered = state.clone();
        altered
            .prior_results
            .get_mut(&previous.id)
            .unwrap()
            .text
            .push('。');
        let error = inspect(
            &engine.store,
            "alice",
            &altered,
            state.result.as_ref().unwrap(),
        )
        .unwrap_err();
        assert!(
            error
                .to_string()
                .contains("RECOVERY_PREVIOUS_RULE_SOURCE_MISMATCH"),
            "{error:?}"
        );
        let unrelated = engine
            .execute(
                "alice",
                RequestEnvelope::new("other", "unrelated", "alice", "[scope=別室]ユリは待機中"),
            )
            .unwrap();
        assert_eq!(unrelated.lifecycle, Lifecycle::Commit, "{unrelated:?}");

        let recovered = engine.reconcile("alice", "case").unwrap();
        assert_eq!(recovered.lifecycle, Lifecycle::Commit, "{recovered:?}");
        assert_eq!(recovered.dependencies, report.dependencies);
        assert_eq!(recovered.receipts[0].action_id, token);
        assert_eq!(
            recovered.receipts[0].reason.as_deref(),
            Some("LOCAL_FILE_RECONCILED_NO_RESEND")
        );
        assert_eq!(fs::read(&fixture.target).unwrap(), bytes);
        assert_eq!(
            fs::metadata(&fixture.target).unwrap().modified().unwrap(),
            modified
        );
        assert_eq!(engine.execute("alice", request).unwrap(), recovered);
        assert_eq!(engine.store.actions("alice", "case").unwrap().len(), 1);
    }

    #[test]
    fn composed_rule_source_reconciles_the_ordered_parent_lineage_without_resend() {
        let fixture = empty_fixture();
        let mut engine = Engine::open(&fixture.state).unwrap();
        let mut parent_ids = Vec::new();
        for (id, text) in [
            ("left", "もし[scope=lab]Xは鳥なら[scope=lab]Xは動物"),
            ("right", "もし[scope=lab]Xは動物なら[scope=lab]Xは温血動物"),
        ] {
            let result = engine
                .execute("alice", RequestEnvelope::new("case", id, "alice", text))
                .unwrap();
            assert_eq!(result.lifecycle, Lifecycle::Commit, "{result:?}");
            parent_ids.push(result.dependencies[0].clone());
        }
        let memory = engine.store.load_memory("alice").unwrap();
        let shortcut = memory
            .rules
            .iter()
            .find(|rule| rule.dependencies == parent_ids)
            .unwrap();
        let command = format!("合成規則の出典: {}", shortcut.id);
        let mut request = RequestEnvelope::new("case", "composed-provenance", "alice", &command);
        request.budget.max_actions = 7;
        request.output_path = Some(fixture.target.clone());
        request
            .permissions
            .write_paths
            .insert(fixture.target.clone());
        let report = engine.execute("alice", request.clone()).unwrap();
        assert_eq!(report.lifecycle, Lifecycle::Commit, "{report:?}");
        assert_eq!(report.dependencies[1..3], parent_ids);
        let bytes = fs::read(&fixture.target).unwrap();
        let modified = fs::metadata(&fixture.target).unwrap().modified().unwrap();
        drop(engine);

        let token = retain_attempted_prefix(&fixture.state);
        let mut engine = Engine::open(&fixture.state).unwrap();
        let state = engine.store.load_case("alice", "case").unwrap().unwrap();
        let mut swapped = state.result.as_ref().unwrap().clone();
        swapped.dependencies.swap(1, 2);
        let error = inspect(&engine.store, "alice", &state, &swapped).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("RECOVERY_COMPOSED_RULE_SOURCE_MISMATCH"),
            "{error:?}"
        );
        let unrelated = engine
            .execute(
                "alice",
                RequestEnvelope::new(
                    "other",
                    "unrelated",
                    "alice",
                    "[scope=archive]ユリは記録済み",
                ),
            )
            .unwrap();
        assert_eq!(unrelated.lifecycle, Lifecycle::Commit, "{unrelated:?}");

        let recovered = engine.reconcile("alice", "case").unwrap();
        assert_eq!(recovered.lifecycle, Lifecycle::Commit, "{recovered:?}");
        assert_eq!(recovered.dependencies, report.dependencies);
        assert_eq!(
            recovered.actor_memory_revision,
            report.actor_memory_revision
        );
        assert_eq!(recovered.receipts[0].action_id, token);
        assert_eq!(
            recovered.receipts[0].reason.as_deref(),
            Some("LOCAL_FILE_RECONCILED_NO_RESEND")
        );
        assert_eq!(fs::read(&fixture.target).unwrap(), bytes);
        assert_eq!(
            fs::metadata(&fixture.target).unwrap().modified().unwrap(),
            modified
        );
        assert_eq!(engine.execute("alice", request).unwrap(), recovered);
        assert_eq!(engine.store.actions("alice", "case").unwrap().len(), 1);
    }

    #[test]
    fn detailed_condition_evaluation_reconciles_the_exact_trace_without_resend() {
        let fixture = empty_fixture();
        let mut engine = Engine::open(&fixture.state).unwrap();
        for (id, text) in [
            (
                "condition",
                "条件定義: [scope=実験室][condition=停止] ユリは停止中",
            ),
            ("positive", "[scope=実験室]ユリは停止中"),
            ("negative", "[scope=実験室]ユリは停止中ではない"),
        ] {
            let result = engine
                .execute("alice", RequestEnvelope::new("case", id, "alice", text))
                .unwrap();
            assert_eq!(result.lifecycle, Lifecycle::Commit, "{result:?}");
        }
        let mut request = RequestEnvelope::new(
            "case",
            "detail",
            "alice",
            "条件評価の内訳: [condition=停止][subject=ユリ][scope=実験室]",
        );
        request.output_path = Some(fixture.target.clone());
        request
            .permissions
            .write_paths
            .insert(fixture.target.clone());
        let result = engine.execute("alice", request.clone()).unwrap();
        assert_eq!(result.lifecycle, Lifecycle::Commit, "{result:?}");
        assert!(result.text.contains("内訳 1 条件 停止: CONFLICT"));
        let bytes = fs::read(&fixture.target).unwrap();
        let modified = fs::metadata(&fixture.target).unwrap().modified().unwrap();
        drop(engine);

        let token = retain_attempted_prefix(&fixture.state);
        let mut engine = Engine::open(&fixture.state).unwrap();
        let mut state = engine.store.load_case("alice", "case").unwrap().unwrap();
        let retained = state.result.as_ref().unwrap();
        let mut shortened = retained.clone();
        shortened.dependencies.pop();
        let error = inspect(&engine.store, "alice", &state, &shortened).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("RECOVERY_CONDITION_EVALUATION_MISMATCH"),
            "{error:?}"
        );

        let mut goal: GoalContract = serde_json::from_slice(
            &engine
                .store
                .get_bytes("alice", "case", state.goal_sha256.as_ref().unwrap())
                .unwrap(),
        )
        .unwrap();
        assert_eq!(
            goal.required_predicates[0],
            crate::core::CONDITION_EVALUATION_DETAIL_PREDICATE
        );
        goal.required_predicates[0] = crate::core::CONDITION_EVALUATION_PREDICATE.into();
        state.goal_sha256 = Some(engine.store.put_artifact("alice", "case", &goal).unwrap());
        let error = inspect(&engine.store, "alice", &state, retained).unwrap_err();
        assert!(
            error.to_string().contains("UNSUPPORTED_RECOVERY_GOAL"),
            "{error:?}"
        );
        let recovered = engine.reconcile("alice", "case").unwrap();
        assert_eq!(recovered.lifecycle, Lifecycle::Commit, "{recovered:?}");
        assert_eq!(recovered.dependencies, result.dependencies);
        assert_eq!(recovered.receipts[0].action_id, token);
        assert_eq!(
            recovered.receipts[0].reason.as_deref(),
            Some("LOCAL_FILE_RECONCILED_NO_RESEND")
        );
        assert_eq!(fs::read(&fixture.target).unwrap(), bytes);
        assert_eq!(
            fs::metadata(&fixture.target).unwrap().modified().unwrap(),
            modified
        );
        assert_eq!(engine.execute("alice", request).unwrap(), recovered);
        assert_eq!(engine.store.actions("alice", "case").unwrap().len(), 1);
    }

    #[test]
    fn selected_sequence_step_reconciles_source_after_history_without_resend() {
        let read_graph = |store: &Store, result: &ResultRecord| -> serde_json::Value {
            serde_json::from_slice(
                &store
                    .get_bytes(
                        "alice",
                        "case",
                        result.execution_graph_ref.as_ref().unwrap(),
                    )
                    .unwrap(),
            )
            .unwrap()
        };
        let fixture = empty_fixture();
        let mut engine = Engine::open(&fixture.state).unwrap();
        let original_request = RequestEnvelope::new(
            "case",
            "sequence",
            "alice",
            "計算手順を実行して:\nbase: 2+3\nfinal: @{base}*2",
        );
        let original = engine.execute("alice", original_request.clone()).unwrap();
        assert_eq!(original.lifecycle, Lifecycle::Commit, "{original:?}");
        let insert_request = RequestEnvelope::new(
            "case",
            "insert",
            "alice",
            "計算手順の fee を final の前に挿入: @{base}+2",
        );
        let inserted = engine.execute("alice", insert_request.clone()).unwrap();
        assert_eq!(inserted.lifecycle, Lifecycle::Commit, "{inserted:?}");
        let history_request =
            RequestEnvelope::new("case", "history", "alice", "計算手順の変更履歴を表示");
        let history = engine.execute("alice", history_request.clone()).unwrap();
        assert_eq!(history.lifecycle, Lifecycle::Commit, "{history:?}");
        assert_eq!(history.calculation.as_ref().unwrap().exact_value, "10");
        let mut request = RequestEnvelope::new(
            "case",
            "fee-detail",
            "alice",
            "計算手順の fee を詳しく説明して",
        );
        request.budget.max_actions = 10;
        request.output_path = Some(fixture.target.clone());
        request
            .permissions
            .write_paths
            .insert(fixture.target.clone());
        let result = engine.execute("alice", request.clone()).unwrap();
        assert_eq!(result.lifecycle, Lifecycle::Commit, "{result:?}");
        assert_eq!(result.calculation.as_ref().unwrap().exact_value, "7");
        assert!(result.dependencies.contains(&history.id));
        assert!(result.dependencies.contains(&inserted.id));
        let expression = "@{base}+2";
        let start = insert_request.text.find(expression).unwrap();
        let expected_units = vec![
            "手順 fee の元の式: @{base}+2".to_owned(),
            "代入した先行手順の値: base = 5".to_owned(),
            "手順 fee の結果: (5)+2 = 7".to_owned(),
            format!(
                "式の出典: 結果ID {}、原文SHA-256 {}、UTF-8バイト [{}, {})",
                inserted.id,
                digest(insert_request.text.as_bytes()),
                start,
                start + expression.len()
            ),
        ];
        assert_eq!(
            result.content_plan.as_ref().unwrap().required_units,
            expected_units
        );
        assert_eq!(result.text, expected_units.join("\n"));
        let selected_graph = read_graph(&engine.store, &result);
        assert_eq!(selected_graph.as_array().unwrap().len(), 6);
        let order: Vec<_> = selected_graph[0]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|node| node["id"].as_str())
            .filter(|id| id.starts_with("sequence-"))
            .collect();
        assert_eq!(order, ["sequence-base", "sequence-fee", "sequence-final"]);
        assert_eq!(
            selected_graph[5]["sequence-fee"]["sequence-base"],
            selected_graph[1]["sequence-base"]
        );
        let bytes = fs::read(&fixture.target).unwrap();
        let modified = fs::metadata(&fixture.target).unwrap().modified().unwrap();
        drop(engine);

        let token = retain_attempted_prefix(&fixture.state);
        let store = Store::open(&fixture.state).unwrap();
        let state = store.load_case("alice", "case").unwrap().unwrap();
        let bundle = state.bundle.as_ref().unwrap();
        assert_eq!(
            bundle.meaning.parser_version,
            crate::input::SEQUENCE_STEP_EXPLANATION_PARSER_VERSION
        );
        assert!(
            matches!(&bundle.meaning.intent, Intent::ExplainCalculationSequenceStep { step_id } if step_id == "fee")
        );
        let goal: GoalContract = serde_json::from_slice(
            &store
                .get_bytes("alice", "case", state.goal_sha256.as_ref().unwrap())
                .unwrap(),
        )
        .unwrap();
        assert_eq!(
            goal.required_predicates[0],
            SEQUENCE_STEP_EXPLANATION_PREDICATE
        );
        let actions = store.actions("alice", "case").unwrap();
        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0].phase, EffectPhase::Unknown);
        let retained = state.result.as_ref().unwrap();
        let mut wrong_step_value = retained.clone();
        wrong_step_value.calculation = history.calculation.clone();
        let error = inspect(&store, "alice", &state, &wrong_step_value).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("RECOVERY_CALCULATION_SEQUENCE_CONTEXT_INVALID"),
            "{error:?}"
        );
        let mut wrong_source = retained.clone();
        wrong_source.content_plan.as_mut().unwrap().required_units[3].push('0');
        assert!(inspect(&store, "alice", &state, &wrong_source).is_err());
        for marker in [Some(SEQUENCE_HISTORY_PREDICATE), None] {
            let mut wrong_marker = state.clone();
            let mut wrong_goal = goal.clone();
            if let Some(marker) = marker {
                wrong_goal.required_predicates[0] = marker.into();
            } else {
                wrong_goal.required_predicates.remove(0);
            }
            wrong_marker.goal_sha256 =
                Some(store.put_artifact("alice", "case", &wrong_goal).unwrap());
            let error = inspect(&store, "alice", &wrong_marker, retained).unwrap_err();
            assert!(
                error.to_string().contains("UNSUPPORTED_RECOVERY_GOAL"),
                "{error:?}"
            );
        }
        assert_eq!(store.actions("alice", "case").unwrap(), actions);
        assert_eq!(fs::read(&fixture.target).unwrap(), bytes);
        assert_eq!(
            fs::metadata(&fixture.target).unwrap().modified().unwrap(),
            modified
        );

        let recovered = reconcile(&store, "alice", "case").unwrap();
        assert_eq!(recovered.lifecycle, Lifecycle::Commit, "{recovered:?}");
        assert_eq!(recovered.calculation, result.calculation);
        assert_eq!(recovered.dependencies, result.dependencies);
        assert_eq!(recovered.content_plan, result.content_plan);
        assert_eq!(recovered.render, result.render);
        assert_eq!(recovered.receipts[0].action_id, token);
        assert_eq!(
            recovered.receipts[0].reason.as_deref(),
            Some("LOCAL_FILE_RECONCILED_NO_RESEND")
        );
        let mut expected_graph = selected_graph;
        expected_graph[1]["file"] = digest(&canonical(&recovered.receipts[0]).unwrap()).into();
        assert_eq!(read_graph(&store, &recovered), expected_graph);
        let confirmed = store.actions("alice", "case").unwrap();
        assert_eq!(confirmed.len(), 1);
        drop(store);
        let mut engine = Engine::open(&fixture.state).unwrap();
        assert_eq!(engine.execute("alice", request).unwrap(), recovered);
        assert_eq!(engine.reconcile("alice", "case").unwrap(), recovered);
        assert_eq!(engine.execute("alice", history_request).unwrap(), history);
        assert_eq!(engine.execute("alice", original_request).unwrap(), original);
        assert_eq!(engine.store.actions("alice", "case").unwrap(), confirmed);
        assert_eq!(fs::read(&fixture.target).unwrap(), bytes);
        assert_eq!(
            fs::metadata(&fixture.target).unwrap().modified().unwrap(),
            modified
        );
    }

    fn assert_revised_sequence_file_recovery(explanation_file: bool) {
        let fixture = empty_fixture();
        let mut engine = Engine::open(&fixture.state).unwrap();
        let original = engine
            .execute(
                "alice",
                RequestEnvelope::new(
                    "case",
                    "sequence",
                    "alice",
                    "計算手順を実行して:\nbase: 1 km + 500 m\nfinal: 100 m + @{base}",
                ),
            )
            .unwrap();
        assert_eq!(original.lifecycle, Lifecycle::Commit);
        let first = engine
            .execute(
                "alice",
                RequestEnvelope::new(
                    "case",
                    "first-correction",
                    "alice",
                    "計算手順の base を訂正: 500 m + 100 m",
                ),
            )
            .unwrap();
        assert_eq!(first.lifecycle, Lifecycle::Commit, "{first:?}");
        assert_eq!(first.calculation.as_ref().unwrap().exact_value, "700 m");
        let detail = engine
            .execute(
                "alice",
                RequestEnvelope::new("case", "detail", "alice", "詳しく説明して"),
            )
            .unwrap();
        assert_eq!(detail.lifecycle, Lifecycle::Commit, "{detail:?}");
        assert_eq!(detail.calculation, first.calculation);
        let mut request = RequestEnvelope::new(
            "case",
            "second-correction",
            "alice",
            "計算手順の final を訂正: 200 m + @{base}",
        );
        request.budget.max_actions = if explanation_file { 10 } else { 11 };
        if !explanation_file {
            request.output_path = Some(fixture.target.clone());
            request
                .permissions
                .write_paths
                .insert(fixture.target.clone());
        }
        let corrected = engine.execute("alice", request.clone()).unwrap();
        assert_eq!(corrected.lifecycle, Lifecycle::Commit, "{corrected:?}");
        assert_eq!(corrected.calculation.as_ref().unwrap().exact_value, "800 m");
        assert_eq!(corrected.dependencies[0], detail.id);
        let result = if explanation_file {
            request = RequestEnvelope::new("case", "revised-detail", "alice", "詳しく説明して");
            request.budget.max_actions = 11;
            request.output_path = Some(fixture.target.clone());
            request
                .permissions
                .write_paths
                .insert(fixture.target.clone());
            let explained = engine.execute("alice", request.clone()).unwrap();
            assert_eq!(explained.lifecycle, Lifecycle::Commit, "{explained:?}");
            assert_eq!(explained.calculation, corrected.calculation);
            assert_eq!(explained.dependencies[0], corrected.id);
            assert!(explained.text.contains("200 m + 600 m = 800 m"));
            explained
        } else {
            corrected
        };
        let latest = engine.latest("alice", "case").unwrap().unwrap();
        let goal: GoalContract = serde_json::from_slice(
            &engine
                .store
                .get_bytes("alice", "case", latest.goal_sha256.as_ref().unwrap())
                .unwrap(),
        )
        .unwrap();
        assert_eq!(
            goal.required_predicates[0],
            if explanation_file {
                REVISED_SEQUENCE_EXPLANATION_PREDICATE
            } else {
                REPEATED_SEQUENCE_CORRECTION_PREDICATE
            }
        );
        let bytes = fs::read(&fixture.target).unwrap();
        let modified = fs::metadata(&fixture.target).unwrap().modified().unwrap();
        drop(engine);

        let token = retain_attempted_prefix(&fixture.state);
        let store = Store::open(&fixture.state).unwrap();
        let state = store.load_case("alice", "case").unwrap().unwrap();
        let actions = store.actions("alice", "case").unwrap();
        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0].phase, EffectPhase::Unknown);
        let retained = state.result.as_ref().unwrap();
        let mut changed = retained.clone();
        changed.dependencies.swap(0, 1);
        let error = inspect(&store, "alice", &state, &changed).unwrap_err();
        assert!(
            error.to_string().contains(if explanation_file {
                "RECOVERY_CALCULATION_SEQUENCE_CONTEXT_INVALID"
            } else {
                "RECOVERY_DEPENDENCY_MISMATCH"
            }),
            "{error:?}"
        );
        let mut malformed_history = state.clone();
        malformed_history
            .prior_results
            .get_mut(&first.id)
            .unwrap()
            .dependencies
            .swap(0, 1);
        let error = inspect(&store, "alice", &malformed_history, retained).unwrap_err();
        assert!(
            error.to_string().contains("CALCULATION_CONTEXT_"),
            "{error:?}"
        );
        let mut missing_original = state.clone();
        missing_original.prior_results.remove(&original.id).unwrap();
        assert!(inspect(&store, "alice", &missing_original, retained).is_err());
        let mut downgraded = state.clone();
        let mut downgraded_goal = goal;
        downgraded_goal.required_predicates[0] = if explanation_file {
            CHAINED_SEQUENCE_EXPLANATION_PREDICATE
        } else {
            SEQUENCE_CORRECTION_PREDICATE
        }
        .into();
        downgraded.goal_sha256 = Some(
            store
                .put_artifact("alice", "case", &downgraded_goal)
                .unwrap(),
        );
        let error = inspect(&store, "alice", &downgraded, retained).unwrap_err();
        assert!(
            error.to_string().contains(if explanation_file {
                "RECOVERY_CALCULATION_SEQUENCE_CONTEXT_INVALID"
            } else {
                "RECOVERY_SEQUENCE_CORRECTION_MISMATCH"
            }),
            "{error:?}"
        );
        downgraded_goal.required_predicates[0] = if explanation_file {
            REPEATED_SEQUENCE_CORRECTION_PREDICATE
        } else {
            REVISED_SEQUENCE_EXPLANATION_PREDICATE
        }
        .into();
        downgraded.goal_sha256 = Some(
            store
                .put_artifact("alice", "case", &downgraded_goal)
                .unwrap(),
        );
        let error = inspect(&store, "alice", &downgraded, retained).unwrap_err();
        assert!(
            error.to_string().contains("UNSUPPORTED_RECOVERY_GOAL"),
            "{error:?}"
        );
        assert_eq!(store.actions("alice", "case").unwrap(), actions);
        assert_eq!(fs::read(&fixture.target).unwrap(), bytes);
        assert_eq!(
            fs::metadata(&fixture.target).unwrap().modified().unwrap(),
            modified
        );

        let recovered = reconcile(&store, "alice", "case").unwrap();
        assert_eq!(recovered.lifecycle, Lifecycle::Commit, "{recovered:?}");
        assert_eq!(recovered.calculation, result.calculation);
        assert_eq!(recovered.dependencies, result.dependencies);
        assert_eq!(recovered.content_plan, result.content_plan);
        assert_eq!(recovered.render, result.render);
        assert_eq!(recovered.text, result.text);
        assert_eq!(recovered.receipts[0].action_id, token);
        assert_eq!(
            recovered.receipts[0].reason.as_deref(),
            Some("LOCAL_FILE_RECONCILED_NO_RESEND")
        );
        let read_graph = |result: &ResultRecord| -> serde_json::Value {
            serde_json::from_slice(
                &store
                    .get_bytes(
                        "alice",
                        "case",
                        result.execution_graph_ref.as_ref().unwrap(),
                    )
                    .unwrap(),
            )
            .unwrap()
        };
        let mut expected_graph = read_graph(&result);
        assert_eq!(expected_graph.as_array().unwrap().len(), 6);
        assert_eq!(
            expected_graph[1]["file"],
            digest(&canonical(&result.receipts[0]).unwrap())
        );
        expected_graph[1]["file"] = digest(&canonical(&recovered.receipts[0]).unwrap()).into();
        assert_eq!(read_graph(&recovered), expected_graph);
        let confirmed = store.actions("alice", "case").unwrap();
        assert_eq!(confirmed.len(), 1);
        drop(store);
        let mut engine = Engine::open(&fixture.state).unwrap();
        assert_eq!(engine.execute("alice", request).unwrap(), recovered);
        assert_eq!(engine.reconcile("alice", "case").unwrap(), recovered);
        assert_eq!(engine.store.actions("alice", "case").unwrap(), confirmed);
        assert_eq!(fs::read(&fixture.target).unwrap(), bytes);
        assert_eq!(
            fs::metadata(&fixture.target).unwrap().modified().unwrap(),
            modified
        );
    }

    #[test]
    fn calculation_sequence_reconciles_observed_steps_without_rewriting_file() {
        let fixture = empty_fixture();
        let mut engine = Engine::open(&fixture.state).unwrap();
        let mut request = RequestEnvelope::new(
            "case",
            "sequence",
            "alice",
            "calculation-sequence:\nbase: 2+3\nlarger: @{base}*2\nfinal: @{larger}+@{base}+@{base}",
        );
        request.budget.max_actions = 7;
        request.output_path = Some(fixture.target.clone());
        request
            .permissions
            .write_paths
            .insert(fixture.target.clone());
        let result = engine.execute("alice", request).unwrap();
        assert_eq!(result.lifecycle, Lifecycle::Commit, "{result:?}");
        assert_eq!(result.calculation.as_ref().unwrap().exact_value, "20");
        let original = fs::read(&fixture.target).unwrap();
        let modified = fs::metadata(&fixture.target).unwrap().modified().unwrap();
        drop(engine);

        let token = retain_attempted_prefix(&fixture.state);
        let store = Store::open(&fixture.state).unwrap();
        let state = store.load_case("alice", "case").unwrap().unwrap();
        let actions = store.actions("alice", "case").unwrap();
        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0].phase, EffectPhase::Unknown);
        let mut mismatched = state.result.as_ref().unwrap().clone();
        mismatched.dependencies.swap(0, 1);
        let error = inspect(&store, "alice", &state, &mismatched).unwrap_err();
        assert!(
            error.to_string().contains("RECOVERY_DEPENDENCY_MISMATCH"),
            "{error:?}"
        );
        assert_eq!(store.actions("alice", "case").unwrap(), actions);

        let mut legacy = state.clone();
        let mut original_goal: GoalContract = serde_json::from_slice(
            &store
                .get_bytes("alice", "case", state.goal_sha256.as_ref().unwrap())
                .unwrap(),
        )
        .unwrap();
        assert_eq!(
            original_goal.required_predicates[0],
            "EXACT_CALCULATION_SEQUENCE_V1"
        );
        original_goal.required_predicates.remove(0);
        legacy.goal_sha256 = Some(store.put_artifact("alice", "case", &original_goal).unwrap());
        inspect(&store, "alice", &legacy, state.result.as_ref().unwrap()).unwrap();

        let recovered = reconcile(&store, "alice", "case").unwrap();
        assert_eq!(recovered.lifecycle, Lifecycle::Commit, "{recovered:?}");
        assert_eq!(recovered.calculation, result.calculation);
        assert_eq!(recovered.dependencies, result.dependencies);
        assert_eq!(recovered.receipts.len(), 1);
        assert_eq!(recovered.receipts[0].action_id, token);
        assert_eq!(store.actions("alice", "case").unwrap().len(), 1);
        assert_eq!(fs::read(&fixture.target).unwrap(), original);
        assert_eq!(
            fs::metadata(&fixture.target).unwrap().modified().unwrap(),
            modified
        );
        let graph: serde_json::Value = serde_json::from_slice(
            &store
                .get_bytes(
                    "alice",
                    "case",
                    recovered.execution_graph_ref.as_ref().unwrap(),
                )
                .unwrap(),
        )
        .unwrap();
        assert_eq!(
            graph[5]["sequence-final"]["sequence-base"],
            result.dependencies[0]
        );
        assert_eq!(
            graph[5]["sequence-final"]["sequence-larger"],
            result.dependencies[1]
        );
    }

    #[test]
    fn previous_calculation_sequence_explanation_reconciles_exact_steps_and_file() {
        assert_sequence_explanation_file_recovery(false);
    }

    #[test]
    fn chained_calculation_sequence_explanation_reconciles_exact_steps_and_file() {
        assert_sequence_explanation_file_recovery(true);
    }

    fn assert_sequence_explanation_file_recovery(chained: bool) {
        let fixture = empty_fixture();
        let mut engine = Engine::open(&fixture.state).unwrap();
        let original = engine
            .execute(
                "alice",
                RequestEnvelope::new(
                    "case",
                    "sequence",
                    "alice",
                    "calculation-sequence:\nbase: 2+3\nunused: 8/4\nfinal: @{base}+@{base}",
                ),
            )
            .unwrap();
        assert_eq!(original.lifecycle, Lifecycle::Commit);
        let mut expected = Vec::new();
        if chained {
            let first = engine
                .execute(
                    "alice",
                    RequestEnvelope::new("case", "first-detail", "alice", "詳しく説明して"),
                )
                .unwrap();
            assert_eq!(first.lifecycle, Lifecycle::Commit);
            expected.push(first.id);
        }
        let mut request = RequestEnvelope::new("case", "detail", "alice", "詳しく説明して");
        request.budget.max_actions = if chained { 8 } else { 7 };
        request.output_path = Some(fixture.target.clone());
        request
            .permissions
            .write_paths
            .insert(fixture.target.clone());
        let detail = engine.execute("alice", request).unwrap();
        assert_eq!(detail.lifecycle, Lifecycle::Commit, "{detail:?}");
        assert!(detail.text.contains("手順 unused の結果: 8/4 = 2"));
        assert!(
            detail
                .text
                .contains("代入した先行手順の値: base = 5、base = 5")
        );
        assert_eq!(detail.calculation, original.calculation);
        expected.push(original.id.clone());
        expected.extend(original.dependencies);
        assert_eq!(detail.dependencies, expected);
        let bytes = fs::read(&fixture.target).unwrap();
        let modified = fs::metadata(&fixture.target).unwrap().modified().unwrap();
        drop(engine);

        let token = retain_attempted_prefix(&fixture.state);
        let store = Store::open(&fixture.state).unwrap();
        let state = store.load_case("alice", "case").unwrap().unwrap();
        if chained {
            let mut missing_original = state.clone();
            assert!(
                missing_original
                    .prior_results
                    .remove(&original.id)
                    .is_some()
            );
            let retained = state.result.as_ref().unwrap();
            let error = inspect(&store, "alice", &missing_original, retained).unwrap_err();
            assert!(error.to_string().contains("CALCULATION_CONTEXT_INVALID"));
            assert_eq!(
                store.actions("alice", "case").unwrap()[0].phase,
                EffectPhase::Unknown
            );
        }
        let mut changed = state.result.as_ref().unwrap().clone();
        changed.dependencies.swap(1, 2);
        let error = inspect(&store, "alice", &state, &changed).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("RECOVERY_CALCULATION_SEQUENCE_CONTEXT_INVALID")
        );
        assert_eq!(
            store.actions("alice", "case").unwrap()[0].phase,
            EffectPhase::Unknown
        );

        let recovered = reconcile(&store, "alice", "case").unwrap();
        assert_eq!(recovered.lifecycle, Lifecycle::Commit, "{recovered:?}");
        assert_eq!(recovered.dependencies, detail.dependencies);
        assert_eq!(recovered.receipts[0].action_id, token);
        let read_graph = |result: &ResultRecord| -> serde_json::Value {
            let reference = result.execution_graph_ref.as_ref().unwrap();
            serde_json::from_slice(&store.get_bytes("alice", "case", reference).unwrap()).unwrap()
        };
        let mut expected_graph = read_graph(&detail);
        let original_receipt = digest(&canonical(&detail.receipts[0]).unwrap());
        assert_eq!(expected_graph[1]["file"], original_receipt);
        let recovered_receipt = &recovered.receipts[0];
        assert_eq!(
            recovered_receipt.reason.as_deref(),
            Some("LOCAL_FILE_RECONCILED_NO_RESEND")
        );
        expected_graph[1]["file"] = digest(&canonical(recovered_receipt).unwrap()).into();
        assert_eq!(read_graph(&recovered), expected_graph);
        assert_eq!(store.actions("alice", "case").unwrap().len(), 1);
        assert_eq!(fs::read(&fixture.target).unwrap(), bytes);
        assert_eq!(
            fs::metadata(&fixture.target).unwrap().modified().unwrap(),
            modified
        );
    }

    #[test]
    fn exact_arithmetic_goal_requires_the_sealed_value() {
        let fixture = empty_fixture();
        let mut engine = Engine::open(&fixture.state).unwrap();
        let mut request =
            RequestEnvelope::new("display", "arithmetic", "alice", "７／２を計算して");
        request.budget.max_actions = 3;
        let result = engine.execute("alice", request).unwrap();
        assert_eq!(result.lifecycle, Lifecycle::Commit, "{result:?}");
        assert_eq!(result.calculation.as_ref().unwrap().exact_value, "7/2");
        assert!(result.text.starts_with("7/2 = 7/2。"));
        assert_eq!(
            result.render.as_ref().unwrap().bytes,
            result.text.as_bytes()
        );
        let display = engine.latest("alice", "display").unwrap().unwrap();
        let goal: GoalContract = serde_json::from_slice(
            &engine
                .store
                .get_bytes("alice", "display", display.goal_sha256.as_ref().unwrap())
                .unwrap(),
        )
        .unwrap();
        assert_eq!(
            goal.required_predicates,
            ["EXACT_CALCULATION_V2", "SUPPORTED_CONTENT", "EXACT_RENDER"]
        );
        assert!(matches!(&goal.intent, Intent::Calculate { expression } if expression == "7/2"));

        let mut compact = RequestEnvelope::new("compact", "arithmetic", "alice", "7/2");
        compact.budget.max_evidence_bytes = "7/2 = 7/2".len();
        let compact_result = engine.execute("alice", compact.clone()).unwrap();
        assert_eq!(
            compact_result.lifecycle,
            Lifecycle::Commit,
            "{compact_result:?}"
        );
        assert_eq!(compact_result.render.as_ref().unwrap().bytes, b"7/2 = 7/2");
        let plan = compact_result.content_plan.as_ref().unwrap();
        let compact_compute: serde_json::Value = serde_json::from_slice(
            &engine
                .store
                .get_bytes("alice", "compact", plan.dependencies.last().unwrap())
                .unwrap(),
        )
        .unwrap();
        assert_eq!(compact_compute["content_generation"]["stop_reason"], "EOS");
        compact.input_id = "shorter".into();
        compact.budget.max_evidence_bytes -= 1;
        let stopped = engine.execute("alice", compact).unwrap();
        assert_eq!(stopped.lifecycle, Lifecycle::Suspend);
        assert!(stopped.content_plan.is_none() && stopped.render.is_none());

        let fixture = attempted_prefix();
        let store = Store::open(&fixture.state).unwrap();
        let case = store.load_case("alice", "case").unwrap().unwrap();
        let result = case.result.as_ref().unwrap();
        let mut goal: GoalContract = serde_json::from_slice(
            &store
                .get_bytes("alice", "case", case.goal_sha256.as_ref().unwrap())
                .unwrap(),
        )
        .unwrap();
        assert_eq!(
            goal.required_predicates,
            [
                "EXACT_CALCULATION_V2",
                "SUPPORTED_CONTENT",
                "EXACT_RENDER",
                "CONFIRMED_FILE"
            ]
        );
        let mut wrong_result = result.clone();
        wrong_result.calculation.as_mut().unwrap().exact_value = "6".into();
        let error = inspect(&store, "alice", &case, &wrong_result).unwrap_err();
        assert!(
            matches!(error, Error::Contract { ref code, .. } if code == "GOAL_CALCULATION_MISMATCH"),
            "{error:?}"
        );
        assert_eq!(
            store.load_case("alice", "case").unwrap(),
            Some(case.clone())
        );
        assert_eq!(store.actions("alice", "case").unwrap().len(), 1);
        assert_eq!(fs::read(&fixture.target).unwrap(), result.text.as_bytes());
        let modified = fs::metadata(&fixture.target).unwrap().modified().unwrap();

        let mut mismatched = result.clone();
        let mismatched_plan = mismatched.content_plan.as_mut().unwrap();
        let unrelated_model =
            language::SequenceModel::train(&["another model".into()], 2, 1).unwrap();
        mismatched_plan.model_artifact_ref = store
            .put_artifact("alice", "case", &unrelated_model)
            .unwrap();
        mismatched_plan.id.clear();
        mismatched_plan.id = identity(&*mismatched_plan).unwrap();
        mismatched.render.as_mut().unwrap().plan_id = mismatched_plan.id.clone();
        let error = inspect(&store, "alice", &case, &mismatched).unwrap_err();
        assert!(
            matches!(error, Error::Contract { ref code, .. } if code == "CONTENT_ADOPTION_MISMATCH"),
            "{error:?}"
        );

        let mut downgraded_case = case.clone();
        goal.required_predicates.remove(0);
        downgraded_case.goal_sha256 = Some(store.put_artifact("alice", "case", &goal).unwrap());
        let error = inspect(&store, "alice", &downgraded_case, result).unwrap_err();
        assert!(
            matches!(error, Error::Contract { ref code, .. } if code == "UNSUPPORTED_RECOVERY_GOAL"),
            "{error:?}"
        );

        let recovered = reconcile(&store, "alice", "case").unwrap();
        assert_eq!(recovered.lifecycle, Lifecycle::Commit);
        assert_eq!(recovered.calculation, result.calculation);
        assert_eq!(store.actions("alice", "case").unwrap().len(), 1);
        assert_eq!(recovered.receipts[0].action_id, fixture.token);
        assert_eq!(fs::read(&fixture.target).unwrap(), result.text.as_bytes());
        assert_eq!(
            fs::metadata(&fixture.target).unwrap().modified().unwrap(),
            modified
        );
    }

    #[test]
    fn legacy_arithmetic_goal_retains_its_original_adoption_and_file() {
        let fixture = attempted_calculation_prefix(true);
        let store = Store::open(&fixture.state).unwrap();
        let mut case = store.load_case("alice", "case").unwrap().unwrap();
        let saved = case.result.as_ref().unwrap().clone();
        let mut goal: GoalContract = serde_json::from_slice(
            &store
                .get_bytes("alice", "case", case.goal_sha256.as_ref().unwrap())
                .unwrap(),
        )
        .unwrap();
        assert_eq!(goal.required_predicates[0], "EXACT_CALCULATION_V1");
        inspect(&store, "alice", &case, &saved).unwrap();

        goal.required_predicates.remove(0);
        case.goal_sha256 = Some(store.put_artifact("alice", "case", &goal).unwrap());
        inspect(&store, "alice", &case, &saved).unwrap();

        let recovered = reconcile(&store, "alice", "case").unwrap();
        assert_eq!(recovered.lifecycle, Lifecycle::Commit);
        assert_eq!(recovered.content_plan, saved.content_plan);
        assert_eq!(recovered.receipts[0].action_id, fixture.token);
        assert_eq!(fs::read(&fixture.target).unwrap(), b"2+3 = 5");
    }

    fn attempted_probability_explanation_prefix() -> Fixture {
        attempted_math_report_explanation_prefix(
            "weights",
            "確率分布 [1,2,3] を正規化して",
            "weights-explanation.txt",
        )
    }

    fn attempted_quadratic_explanation_prefix() -> Fixture {
        attempted_math_report_explanation_prefix(
            "quadratic",
            "二次方程式 2*x^2-3*x+1=0 を解いて",
            "quadratic-explanation.txt",
        )
    }

    fn attempted_symbolic_explanation_prefix() -> Fixture {
        attempted_math_report_explanation_prefix(
            "symbolic",
            "微分 x^3+2*x 変数 x",
            "symbolic-explanation.txt",
        )
    }

    fn attempted_math_report_explanation_prefix(
        input_id: &str,
        input_text: &str,
        target_name: &str,
    ) -> Fixture {
        let directory = std::env::temp_dir().join(format!(
            "minidora-recovery-math-context-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir(&directory).unwrap();
        let state = directory.join("state");
        let target = directory.join(target_name);
        let mut engine = Engine::open(&state).unwrap();
        let statistics = engine
            .execute(
                "alice",
                RequestEnvelope::new("case", input_id, "alice", input_text),
            )
            .unwrap();
        assert_eq!(statistics.lifecycle, Lifecycle::Commit);
        let mut explanation =
            RequestEnvelope::new("case", "explanation", "alice", "詳しく説明して");
        explanation.output_path = Some(target.clone());
        explanation.permissions.write_paths.insert(target.clone());
        assert_eq!(
            engine.execute("alice", explanation).unwrap().lifecycle,
            Lifecycle::Commit
        );
        drop(engine);

        // Keep the actual Core journal at the post-render/pre-receipt edge;
        // retain the physical write so reconciliation must observe it.
        let database = Connection::open(state.join("state.sqlite3")).unwrap();
        let snapshots: Vec<(i64, String)> = {
            let mut statement = database
                .prepare("SELECT revision,state_sha256 FROM case_revision ORDER BY revision")
                .unwrap();
            statement
                .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
                .unwrap()
                .collect::<std::result::Result<_, _>>()
                .unwrap()
        };
        let mut retained = None;
        for (number, sha) in snapshots {
            let revision: CaseRevision =
                serde_json::from_slice(&fs::read(state.join("blobs").join(sha)).unwrap()).unwrap();
            if revision.lifecycle == Lifecycle::Adopted
                && revision
                    .bundle
                    .as_ref()
                    .is_some_and(|bundle| matches!(bundle.meaning.intent, Intent::ExplainPrevious))
                && revision.result.as_ref().is_some_and(|result| {
                    result.dependencies.len() == 1
                        && result.content_plan.is_some()
                        && result.render.is_some()
                })
            {
                retained = Some((number, revision));
            }
        }
        let (number, revision) =
            retained.expect("Core retained the math explanation before file confirmation");
        let result = revision.result.as_ref().unwrap();
        let result_sha = digest(&canonical(result).unwrap());
        let token: String = database
            .query_row("SELECT idempotency_key FROM action_identity", [], |row| {
                row.get(0)
            })
            .unwrap();
        database
            .execute_batch(
                "PRAGMA foreign_keys=ON; DROP TRIGGER case_revision_no_delete; DROP TRIGGER action_history_no_delete;",
            )
            .unwrap();
        database
            .execute("DELETE FROM case_revision WHERE revision>?1", [number])
            .unwrap();
        database
            .execute("DELETE FROM action_history WHERE sequence>3", [])
            .unwrap();
        database
            .execute(
                "UPDATE input_identity SET result_sha256=?1 WHERE case_id='case' AND input_id='explanation' AND input_version=1",
                params![result_sha],
            )
            .unwrap();
        drop(database);
        Fixture {
            directory,
            state,
            target,
            token,
        }
    }

    fn attempted_table_context_explanation_prefix(operation: &str) -> Fixture {
        let directory = std::env::temp_dir().join(format!(
            "minidora-recovery-json-table-context-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir(&directory).unwrap();
        let state = directory.join("state");
        let target = directory.join("json-table-explanation.txt");
        let mut engine = Engine::open(&state).unwrap();
        let request = match operation {
            "comparison" => {
                let left =
                    r#"{"columns":["id","name"],"rows":[["north","cat"],["removed","dog"]]}"#;
                let right =
                    r#"{"columns":["id","name"],"rows":[["north","fox"],["added","bird"]]}"#;
                let source = format!(r#"{{"left":{left},"right":{right}}}"#);
                format!("JSON表を列「id」で比較して:\n{source}")
            }
            "filter" => {
                let source = r#"{"columns":["state","name"],"rows":[["open","Mina"],["closed","Nia"],["open","Kai"]]}"#;
                format!("JSON表の列「state」が値「open」の行を選択して:\n{source}")
            }
            "inner_join" => {
                let left = r#"{"columns":["name","id"],"rows":[["Mina","north"],["Nia","west"]]}"#;
                let right =
                    r#"{"columns":["record_id","group"],"rows":[["north","A"],["south","B"]]}"#;
                let source = format!(r#"{{"left":{left},"right":{right}}}"#);
                format!("JSON表を「id」と「record_id」で内部結合して:\n{source}")
            }
            "csv_comparison" => {
                let left = "id,name\r\nnorth,猫\r\nremoved,犬";
                let right = "id,name\r\nnorth,狐\r\nadded,鳥";
                format!(
                    "CSVを列「id」で2つの表を比較して:\n--- MINIDORA LEFT CSV ---\n{left}\r\n--- MINIDORA RIGHT CSV ---\n{right}"
                )
            }
            _ => panic!("unsupported JSON table context fixture: {operation}"),
        };
        let first = engine
            .execute(
                "alice",
                RequestEnvelope::new("case", "table-operation", "alice", &request),
            )
            .unwrap();
        assert_eq!(first.lifecycle, Lifecycle::Commit);
        let mut explanation =
            RequestEnvelope::new("case", "explanation", "alice", "詳しく説明して");
        explanation.output_path = Some(target.clone());
        explanation.permissions.write_paths.insert(target.clone());
        assert_eq!(
            engine.execute("alice", explanation).unwrap().lifecycle,
            Lifecycle::Commit
        );
        drop(engine);

        // Preserve the actual Core journal at the post-render/pre-receipt
        // edge while leaving the physical local write in place.
        let database = Connection::open(state.join("state.sqlite3")).unwrap();
        let snapshots: Vec<(i64, String)> = {
            let mut statement = database
                .prepare("SELECT revision,state_sha256 FROM case_revision ORDER BY revision")
                .unwrap();
            statement
                .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
                .unwrap()
                .collect::<std::result::Result<_, _>>()
                .unwrap()
        };
        let mut retained = None;
        for (number, sha) in snapshots {
            let revision: CaseRevision =
                serde_json::from_slice(&fs::read(state.join("blobs").join(sha)).unwrap()).unwrap();
            if revision.lifecycle == Lifecycle::Adopted
                && revision
                    .bundle
                    .as_ref()
                    .is_some_and(|bundle| matches!(bundle.meaning.intent, Intent::ExplainPrevious))
                && revision.result.as_ref().is_some_and(|result| {
                    result.dependencies.len() == 3
                        && result.content_plan.is_some()
                        && result.render.is_some()
                })
            {
                retained = Some((number, revision));
            }
        }
        let (number, revision) =
            retained.expect("Core retained the table explanation before local-file confirmation");
        let result = revision.result.as_ref().unwrap();
        let result_sha = digest(&canonical(result).unwrap());
        let token: String = database
            .query_row("SELECT idempotency_key FROM action_identity", [], |row| {
                row.get(0)
            })
            .unwrap();
        database
            .execute_batch(
                "PRAGMA foreign_keys=ON; DROP TRIGGER case_revision_no_delete; DROP TRIGGER action_history_no_delete;",
            )
            .unwrap();
        database
            .execute("DELETE FROM case_revision WHERE revision>?1", [number])
            .unwrap();
        database
            .execute("DELETE FROM action_history WHERE sequence>3", [])
            .unwrap();
        database
            .execute(
                "UPDATE input_identity SET result_sha256=?1 WHERE case_id='case' AND input_id='explanation' AND input_version=1",
                params![result_sha],
            )
            .unwrap();
        drop(database);
        Fixture {
            directory,
            state,
            target,
            token,
        }
    }

    #[test]
    fn calculation_explanation_candidates_replay_for_recovery() {
        let fixture = empty_fixture();
        let mut engine = Engine::open(&fixture.state).unwrap();
        let mut calculation = RequestEnvelope::new("case", "calculation", "alice", "2+3");
        calculation.output_path = Some(fixture.target.clone());
        calculation
            .permissions
            .write_paths
            .insert(fixture.target.clone());
        assert_eq!(
            engine.execute("alice", calculation).unwrap().lifecycle,
            Lifecycle::Commit
        );

        let explanation_target = fixture.target.with_file_name("explanation.txt");
        let mut request = RequestEnvelope::new("case", "explanation", "alice", "詳しく説明して");
        request.output_path = Some(explanation_target.clone());
        request.permissions.write_paths.insert(explanation_target);
        assert_eq!(
            engine.execute("alice", request).unwrap().lifecycle,
            Lifecycle::Commit
        );

        let revision = engine.latest("alice", "case").unwrap().unwrap();
        let result = revision.result.as_ref().unwrap();
        let plan = result.content_plan.as_ref().unwrap();
        let emission_bytes = engine
            .store
            .get_bytes("alice", "case", &plan.model_observation_ref)
            .unwrap();
        let emission: language::ModelEmission = serde_json::from_slice(&emission_bytes).unwrap();
        assert_eq!(emission.authorized_surfaces.as_ref().map(Vec::len), Some(2));
        assert!(inspect(&engine.store, "alice", &revision, result).is_ok());
    }

    #[test]
    fn statistics_explanation_reconciles_the_original_file_without_resending() {
        let fixture = attempted_statistics_explanation_prefix();
        let original = fs::read(&fixture.target).unwrap();
        let modified = fs::metadata(&fixture.target).unwrap().modified().unwrap();
        let store = Store::open(&fixture.state).unwrap();
        let actions = store.actions("alice", "case").unwrap();
        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0].idempotency_key, fixture.token);
        assert_eq!(actions[0].phase, EffectPhase::Unknown);

        let result = reconcile(&store, "alice", "case").unwrap();
        assert_eq!(result.lifecycle, Lifecycle::Commit);
        assert!(result.text.contains("前回の記述統計を元の値から再計算"));
        assert_eq!(result.dependencies.len(), 1);
        assert_eq!(fs::read(&fixture.target).unwrap(), original);
        assert_eq!(
            fs::metadata(&fixture.target).unwrap().modified().unwrap(),
            modified
        );
        let actions = store.actions("alice", "case").unwrap();
        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0].phase, EffectPhase::Confirmed);
        assert_eq!(actions[0].idempotency_key, fixture.token);
    }

    #[test]
    fn structured_table_operation_explanation_reconciles_without_resending() {
        for (operation, expected_marker) in [
            ("filter", "前回のJSON表フィルター"),
            ("inner_join", "前回のJSON表内部結合"),
            ("comparison", "前回のJSON表比較"),
            ("csv_comparison", "前回のCSV表比較"),
        ] {
            let fixture = attempted_table_context_explanation_prefix(operation);
            let original = fs::read(&fixture.target).unwrap();
            let modified = fs::metadata(&fixture.target).unwrap().modified().unwrap();
            let store = Store::open(&fixture.state).unwrap();
            let actions = store.actions("alice", "case").unwrap();
            assert_eq!(actions.len(), 1);
            assert_eq!(actions[0].idempotency_key, fixture.token);
            assert_eq!(actions[0].phase, EffectPhase::Unknown);

            let result = reconcile(&store, "alice", "case").unwrap();
            assert_eq!(result.lifecycle, Lifecycle::Commit);
            assert!(result.text.contains(expected_marker));
            assert!(result.text.contains("封印済みInputBundle SHA-256"));
            assert!(result.text.contains("report artifact SHA-256"));
            assert!(result.text.contains("byte["));
            assert_eq!(result.dependencies.len(), 3);
            assert_eq!(fs::read(&fixture.target).unwrap(), original);
            assert_eq!(
                fs::metadata(&fixture.target).unwrap().modified().unwrap(),
                modified
            );
            let actions = store.actions("alice", "case").unwrap();
            assert_eq!(actions.len(), 1);
            assert_eq!(actions[0].phase, EffectPhase::Confirmed);
            assert_eq!(actions[0].idempotency_key, fixture.token);
        }
    }

    #[test]
    fn probability_explanation_reconciles_the_original_file_without_resending() {
        let fixture = attempted_probability_explanation_prefix();
        let original = fs::read(&fixture.target).unwrap();
        let modified = fs::metadata(&fixture.target).unwrap().modified().unwrap();
        let store = Store::open(&fixture.state).unwrap();
        let actions = store.actions("alice", "case").unwrap();
        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0].idempotency_key, fixture.token);
        assert_eq!(actions[0].phase, EffectPhase::Unknown);

        let result = reconcile(&store, "alice", "case").unwrap();
        assert_eq!(result.lifecycle, Lifecycle::Commit);
        assert!(result.text.contains("前回の重み分布を元の入力から再計算"));
        assert!(result.text.contains("正規化後の確率: [1/6, 1/3, 1/2]"));
        assert_eq!(result.dependencies.len(), 1);
        assert_eq!(fs::read(&fixture.target).unwrap(), original);
        assert_eq!(
            fs::metadata(&fixture.target).unwrap().modified().unwrap(),
            modified
        );
        let actions = store.actions("alice", "case").unwrap();
        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0].phase, EffectPhase::Confirmed);
        assert_eq!(actions[0].idempotency_key, fixture.token);
    }

    #[test]
    fn quadratic_explanation_reconciles_the_original_file_without_resending() {
        let fixture = attempted_quadratic_explanation_prefix();
        let original = fs::read(&fixture.target).unwrap();
        let modified = fs::metadata(&fixture.target).unwrap().modified().unwrap();
        let store = Store::open(&fixture.state).unwrap();
        let actions = store.actions("alice", "case").unwrap();
        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0].idempotency_key, fixture.token);
        assert_eq!(actions[0].phase, EffectPhase::Unknown);

        let result = reconcile(&store, "alice", "case").unwrap();
        assert_eq!(result.lifecycle, Lifecycle::Commit);
        assert!(result.text.contains("前回の二次方程式を元の両辺から再計算"));
        assert!(result.text.contains("判別式: 1"));
        assert_eq!(result.dependencies.len(), 1);
        assert_eq!(fs::read(&fixture.target).unwrap(), original);
        assert_eq!(
            fs::metadata(&fixture.target).unwrap().modified().unwrap(),
            modified
        );
        let actions = store.actions("alice", "case").unwrap();
        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0].phase, EffectPhase::Confirmed);
        assert_eq!(actions[0].idempotency_key, fixture.token);
    }

    #[test]
    fn symbolic_explanation_reconciles_the_original_file_without_resending() {
        let fixture = attempted_symbolic_explanation_prefix();
        let original = fs::read(&fixture.target).unwrap();
        let modified = fs::metadata(&fixture.target).unwrap().modified().unwrap();
        let store = Store::open(&fixture.state).unwrap();
        let actions = store.actions("alice", "case").unwrap();
        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0].idempotency_key, fixture.token);
        assert_eq!(actions[0].phase, EffectPhase::Unknown);

        let result = reconcile(&store, "alice", "case").unwrap();
        assert_eq!(result.lifecycle, Lifecycle::Commit);
        assert!(result.text.contains("前回の記号式操作 (derive)"));
        assert!(result.text.contains("変換後の標準式:"));
        assert_eq!(result.dependencies.len(), 1);
        assert_eq!(fs::read(&fixture.target).unwrap(), original);
        assert_eq!(
            fs::metadata(&fixture.target).unwrap().modified().unwrap(),
            modified
        );
        let actions = store.actions("alice", "case").unwrap();
        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0].phase, EffectPhase::Confirmed);
        assert_eq!(actions[0].idempotency_key, fixture.token);
    }

    #[test]
    fn ordinary_why_adapts_from_shared_retained_rules_after_restart() {
        let fixture = empty_fixture();
        let mut engine = Engine::open(&fixture.state).unwrap();
        let execute = |engine: &mut Engine, case: &str, id: &str, text: &str| {
            let mut request = RequestEnvelope::new(case, id, "alice", text);
            request.budget.max_actions = 128;
            engine.execute("alice", request).unwrap()
        };
        let rule_a = "もし[scope=lab]Xは鳥なら[scope=lab]Xは動物";
        let rule_b = "もし[scope=lab]Xは動物なら[scope=lab]Xは温血動物";
        let mut parent_a = String::new();
        for (id, text) in [
            ("rule-a", rule_a),
            ("premise-a", "[scope=lab]甲は鳥です"),
            ("premise-b", "[scope=lab]乙は鳥です"),
        ] {
            let result = execute(&mut engine, "case", id, text);
            assert_eq!(result.lifecycle, Lifecycle::Commit, "{result:?}");
            if id == "rule-a" {
                parent_a.clone_from(&result.dependencies[0]);
            }
        }
        let read_compute = |engine: &Engine, case: &str, result: &ResultRecord| {
            let plan = result.content_plan.as_ref().unwrap();
            serde_json::from_slice::<serde_json::Value>(
                &engine
                    .store
                    .get_bytes("alice", case, plan.dependencies.last().unwrap())
                    .unwrap(),
            )
            .unwrap()
        };
        let question = "なぜ[scope=lab]乙は温血動物ですか";
        let cross_target = fixture.directory.join("other-case-report.txt");
        let mut cross_request = RequestEnvelope::new("cross-case", "why-b", "alice", question);
        cross_request.budget.max_actions = 128;
        cross_request.output_path = Some(cross_target.clone());
        cross_request
            .permissions
            .write_paths
            .insert(cross_target.clone());
        let mut before_request = cross_request.clone();
        before_request.case_id = "before-case".into();
        let before = engine.execute("alice", before_request).unwrap();
        assert_eq!(before.lifecycle, Lifecycle::Suspend);
        assert!(before.content_plan.is_none());
        assert!(
            engine
                .store
                .actions("alice", "before-case")
                .unwrap()
                .is_empty()
        );
        assert!(!cross_target.exists());
        let formed = execute(&mut engine, "case", "rule-b", rule_b);
        assert_eq!(formed.lifecycle, Lifecycle::Commit, "{formed:?}");

        let question_input = RequestEnvelope::new("control-case", "why-b", "alice", question);
        let Intent::AskWhy { proposition } = crate::input::seal(&question_input, None)
            .unwrap()
            .meaning
            .intent
        else {
            panic!("ordinary question did not seal an AskWhy");
        };
        let mut control_request = RequestEnvelope::new("control-case", "why-b", "alice", "");
        control_request.budget.max_actions = 128;
        let baseline = engine
            .compose_atomic_explanation("alice", control_request, proposition)
            .unwrap();
        assert_eq!(baseline.lifecycle, Lifecycle::Commit, "{baseline:?}");
        let control = read_compute(&engine, "control-case", &baseline);
        assert_eq!(
            control["kind"],
            "minidora.core-computation.relation-meaning.v5"
        );
        let source = execute(
            &mut engine,
            "case",
            "why-a",
            "なぜ[scope=lab]甲は温血動物ですか",
        );
        assert_eq!(source.lifecycle, Lifecycle::Commit, "{source:?}");
        let source_compute = read_compute(&engine, "case", &source);
        assert_eq!(
            source_compute["kind"],
            "minidora.core-computation.relation-meaning.v7"
        );
        drop(engine);

        let mut engine = Engine::open(&fixture.state).unwrap();
        let mut request = RequestEnvelope::new("case", "why-b", "alice", question);
        request.budget.max_actions = 128;
        request.output_path = Some(fixture.target.clone());
        request
            .permissions
            .write_paths
            .insert(fixture.target.clone());
        let adapted = engine.execute("alice", request).unwrap();
        assert_eq!(adapted.lifecycle, Lifecycle::Commit, "{adapted:?}");
        let actual = read_compute(&engine, "case", &adapted);
        assert_eq!(
            actual["kind"],
            "minidora.core-computation.relation-meaning.v6"
        );
        assert_eq!(actual["adaptation"]["source_result_id"], source.id);
        assert_eq!(
            actual["adaptation"]["rule_ids"].as_array().unwrap().len(),
            3
        );
        assert_eq!(
            actual["relation_meaning_plan"],
            control["relation_meaning_plan"]
        );
        assert_eq!(adapted.dependencies, baseline.dependencies);
        assert_eq!(adapted.guarantee, baseline.guarantee);
        let base_steps = control["content_generation"]["token_steps"]
            .as_array()
            .unwrap();
        let actual_steps = actual["content_generation"]["token_steps"]
            .as_array()
            .unwrap();
        let changed_distributions = base_steps
            .iter()
            .zip(actual_steps)
            .filter(|(base, actual)| {
                base["unit_id"] == actual["unit_id"]
                    && base["fragment_index"] == actual["fragment_index"]
                    && base["admissible_tokens"] == actual["admissible_tokens"]
                    && base["admissible_tokens"]
                        .as_array()
                        .is_some_and(|support| support.len() > 1)
                    && base["distribution"] != actual["distribution"]
            })
            .count();
        assert!(
            changed_distributions > 0,
            "real admissible-token probabilities must change"
        );
        eprintln!(
            "B99 comparable distributions changed={changed_distributions}, rendered text changed={}",
            baseline.text != adapted.text,
        );
        let model_ref = &adapted.content_plan.as_ref().unwrap().model_artifact_ref;
        let model: language::SequenceModel =
            serde_json::from_slice(&engine.store.get_bytes("alice", "case", model_ref).unwrap())
                .unwrap();
        assert!(!model.vocabulary.contains("甲"));
        let retained = engine.latest("alice", "case").unwrap().unwrap();
        let original_bytes = fs::read(&fixture.target).unwrap();
        let cross_result = engine.execute("alice", cross_request).unwrap();
        assert_eq!(
            cross_result.lifecycle,
            Lifecycle::Commit,
            "{cross_result:?}"
        );
        assert_eq!(cross_result.guarantee, baseline.guarantee);
        assert_eq!(cross_result.dependencies, baseline.dependencies);
        let cross = read_compute(&engine, "cross-case", &cross_result);
        assert_eq!(
            cross["kind"],
            "minidora.core-computation.relation-meaning.v7"
        );
        assert_eq!(
            cross["adaptation"]["rule_ids"],
            actual["adaptation"]["rule_ids"]
        );
        assert!(cross["adaptation"].get("source_result_id").is_none());
        assert_eq!(
            cross["relation_meaning_plan"],
            control["relation_meaning_plan"]
        );
        assert_eq!(
            cross["content_generation"]["token_steps"],
            actual["content_generation"]["token_steps"]
        );
        let cross_plan = cross_result.content_plan.as_ref().unwrap();
        assert_eq!(cross_plan.model_artifact_ref, *model_ref);
        let cross_render = cross_result.render.as_ref().unwrap();
        let cross_actions = engine.store.actions("alice", "cross-case").unwrap();
        assert_eq!(cross_actions.len(), 1);
        assert_eq!(cross_actions[0].phase, EffectPhase::Confirmed);
        assert_eq!(
            cross_actions[0].exact_request_sha256,
            cross_render.actual_sha256
        );
        assert_eq!(
            cross_actions[0].receipt.as_ref(),
            cross_result.receipts.first()
        );
        assert_eq!(fs::read(&cross_target).unwrap(), cross_render.bytes);
        let graph: serde_json::Value = serde_json::from_slice(
            &engine
                .store
                .get_bytes(
                    "alice",
                    "cross-case",
                    cross_result.execution_graph_ref.as_ref().unwrap(),
                )
                .unwrap(),
        )
        .unwrap();
        assert_eq!(
            graph[2]["compute"],
            *cross_plan.dependencies.last().unwrap()
        );
        assert_eq!(graph[3]["content"], graph[1]["content"]);
        assert_eq!(graph[4]["render"], graph[1]["render"]);
        let cross_retained = engine.latest("alice", "cross-case").unwrap().unwrap();
        assert!(cross_retained.prior_results.is_empty());
        assert_eq!(
            cross_retained
                .bundle
                .as_ref()
                .unwrap()
                .meaning
                .context_result_ref,
            None
        );
        drop(engine);

        let mut engine = Engine::open(&fixture.state).unwrap();
        inspect(&engine.store, "alice", &retained, &adapted).unwrap();
        inspect(&engine.store, "alice", &cross_retained, &cross_result).unwrap();
        let control_model = &baseline.content_plan.as_ref().unwrap().model_artifact_ref;
        let control_model_bytes = engine
            .store
            .get_bytes("alice", "control-case", control_model)
            .unwrap();
        let mut altered = adapted.clone();
        let altered_plan = altered.content_plan.as_mut().unwrap();
        altered_plan.model_artifact_ref = engine
            .store
            .put_bytes("alice", "case", &control_model_bytes)
            .unwrap();
        assert_ne!(&altered_plan.model_artifact_ref, model_ref);
        altered_plan.id.clear();
        altered_plan.id = identity(&*altered_plan).unwrap();
        altered.render.as_mut().unwrap().plan_id = altered_plan.id.clone();
        let error = inspect(&engine.store, "alice", &retained, &altered).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("PRIOR_COMPUTATION_ARTIFACT_MISMATCH"),
            "{error:?}"
        );
        let foreign = engine
            .execute(
                "bob",
                RequestEnvelope::new("bob-case", "why-b", "bob", question),
            )
            .unwrap();
        assert_eq!(foreign.lifecycle, Lifecycle::Suspend);
        assert!(foreign.content_plan.is_none());
        for (id, text) in [
            (
                "other-rule-a",
                "もし[scope=other]Xは鳥なら[scope=other]Xは動物",
            ),
            (
                "other-rule-b",
                "もし[scope=other]Xは動物なら[scope=other]Xは温血動物",
            ),
            ("other-premise", "[scope=other]乙は鳥です"),
        ] {
            let result = execute(&mut engine, "other-case", id, text);
            assert_eq!(result.lifecycle, Lifecycle::Commit, "{result:?}");
        }
        let mut outside = RequestEnvelope::new(
            "case",
            "why-other",
            "alice",
            "なぜ[scope=other]乙は温血動物ですか",
        );
        outside.context_ref = Some(source.id.clone());
        let outside = engine.execute("alice", outside).unwrap();
        assert_eq!(outside.lifecycle, Lifecycle::Commit, "{outside:?}");
        let outside_compute = read_compute(&engine, "case", &outside);
        assert_eq!(
            outside_compute["kind"],
            "minidora.core-computation.relation-meaning.v7"
        );
        let outside_rules = outside_compute["adaptation"]["rule_ids"]
            .as_array()
            .unwrap();
        let applied_rules = cross["adaptation"]["rule_ids"].as_array().unwrap();
        assert_eq!(outside_rules.len(), 3);
        assert!(outside_rules.iter().all(|id| !applied_rules.contains(id)));
        assert!(
            outside_compute["adaptation"]
                .get("source_result_id")
                .is_none()
        );
        inspect(&engine.store, "alice", &retained, &adapted).unwrap();
        inspect(&engine.store, "alice", &cross_retained, &cross_result).unwrap();

        let invalidation = execute(
            &mut engine,
            "case",
            "invalidate",
            &format!("{parent_a}を無効化して"),
        );
        assert_eq!(
            invalidation.lifecycle,
            Lifecycle::Commit,
            "{invalidation:?}"
        );
        let error = inspect(&engine.store, "alice", &retained, &adapted).unwrap_err();
        assert!(
            error.to_string().contains("RECOVERY_PROOF_INVALIDATED"),
            "{error:?}"
        );
        let error = inspect(&engine.store, "alice", &cross_retained, &cross_result).unwrap_err();
        assert!(
            error.to_string().contains("RECOVERY_PROOF_INVALIDATED"),
            "{error:?}"
        );
        let no_longer_supported = execute(
            &mut engine,
            "cross-case",
            "why-after-invalidation",
            question,
        );
        assert_eq!(no_longer_supported.lifecycle, Lifecycle::Suspend);
        assert!(no_longer_supported.content_plan.is_none());
        assert_eq!(
            engine.store.actions("alice", "cross-case").unwrap(),
            cross_actions
        );
        assert_eq!(fs::read(&fixture.target).unwrap(), original_bytes);
        assert_eq!(engine.store.actions("alice", "case").unwrap().len(), 1);
    }

    #[test]
    fn ordinary_why_generates_from_a_saved_composed_proof_and_replays_it() {
        let fixture = empty_fixture();
        let mut engine = Engine::open(&fixture.state).unwrap();
        let question = "なぜ[scope=lab]シロは温血動物ですか";
        let execute = |engine: &mut Engine, id: &str, text: &str| {
            engine
                .execute("alice", RequestEnvelope::new("case", id, "alice", text))
                .unwrap()
        };
        let fact = execute(&mut engine, "premise", "[scope=lab]シロは鳥です");
        assert_eq!(fact.lifecycle, Lifecycle::Commit, "{fact:?}");
        let before = execute(&mut engine, "before-learning", question);
        assert_eq!(before.lifecycle, Lifecycle::Suspend, "{before:?}");
        assert!(before.content_plan.is_none());

        let parent_a = execute(
            &mut engine,
            "first-rule",
            "もし[scope=lab]Xは鳥なら[scope=lab]Xは動物",
        );
        let parent_b = execute(
            &mut engine,
            "second-rule",
            "もし[scope=lab]Xは動物なら[scope=lab]Xは温血動物",
        );
        assert_eq!(parent_a.lifecycle, Lifecycle::Commit, "{parent_a:?}");
        assert_eq!(parent_b.lifecycle, Lifecycle::Commit, "{parent_b:?}");
        let memory = engine.store.load_memory("alice").unwrap();
        let shortcut = memory
            .rules
            .iter()
            .find(|rule| rule.source_input == "derived:proof-composition-v1")
            .expect("Core formed a persisted composed rule");
        let mut request = RequestEnvelope::new("case", "generated-why", "alice", question);
        request.budget.max_actions = 128;
        request.output_path = Some(fixture.target.clone());
        request
            .permissions
            .write_paths
            .insert(fixture.target.clone());
        let result = engine.execute("alice", request).unwrap();
        assert_eq!(result.lifecycle, Lifecycle::Commit, "{result:?}");
        assert_eq!(result.guarantee, Guarantee::Derived);
        for id in [
            &fact.dependencies[0],
            &parent_a.dependencies[0],
            &parent_b.dependencies[0],
            &shortcut.id,
        ] {
            assert!(result.dependencies.contains(id), "{id}");
        }
        let plan = result.content_plan.as_ref().unwrap();
        let compute: serde_json::Value = serde_json::from_slice(
            &engine
                .store
                .get_bytes("alice", "case", plan.dependencies.last().unwrap())
                .unwrap(),
        )
        .unwrap();
        assert_eq!(
            compute["kind"],
            "minidora.core-computation.relation-meaning.v7"
        );
        assert_eq!(compute["relation_meaning_plan"]["asked"]["subject"], "シロ");
        assert_eq!(compute["relation_meaning_plan"]["asked"]["scope"], "lab");
        let emission: language::ModelEmission = serde_json::from_slice(
            &engine
                .store
                .get_bytes("alice", "case", &plan.model_observation_ref)
                .unwrap(),
        )
        .unwrap();
        assert!(emission.authorized_surfaces.is_none());
        assert_eq!(emission.text, result.text);
        let original_bytes = fs::read(&fixture.target).unwrap();
        assert_eq!(original_bytes, result.text.as_bytes());
        let modified = fs::metadata(&fixture.target).unwrap().modified().unwrap();
        drop(engine);

        let token = retain_attempted_prefix(&fixture.state);
        let mut engine = Engine::open(&fixture.state).unwrap();
        let recovered = engine.reconcile("alice", "case").unwrap();
        assert_eq!(recovered.lifecycle, Lifecycle::Commit, "{recovered:?}");
        assert_eq!(recovered.text, result.text);
        assert_eq!(recovered.receipts[0].action_id, token);
        assert_eq!(
            fs::metadata(&fixture.target).unwrap().modified().unwrap(),
            modified
        );
        let retained = engine.latest("alice", "case").unwrap().unwrap();
        let followup = execute(&mut engine, "followup", "詳しく説明して");
        assert_eq!(followup.lifecycle, Lifecycle::Commit, "{followup:?}");
        assert_eq!(followup.dependencies.first(), Some(&result.id));
        let unrelated = execute(&mut engine, "new-other-scope", "[scope=other]ハナは鳥です");
        assert_eq!(unrelated.lifecycle, Lifecycle::Commit, "{unrelated:?}");
        assert_ne!(
            result.actor_memory_revision,
            Some(engine.store.load_memory("alice").unwrap().revision)
        );
        assert!(inspect(&engine.store, "alice", &retained, &result).is_ok());
        let out_of_scope = execute(
            &mut engine,
            "out-of-scope",
            "なぜ[scope=other]ハナは温血動物ですか",
        );
        assert_eq!(out_of_scope.lifecycle, Lifecycle::Suspend);
        assert!(out_of_scope.content_plan.is_none());

        let invalidation = execute(
            &mut engine,
            "invalidate-parent",
            &format!("{}を無効化して", parent_a.dependencies[0]),
        );
        assert_eq!(
            invalidation.lifecycle,
            Lifecycle::Commit,
            "{invalidation:?}"
        );
        let error = inspect(&engine.store, "alice", &retained, &result).unwrap_err();
        assert!(
            error.to_string().contains("RECOVERY_PROOF_INVALIDATED"),
            "{error:?}"
        );
        assert_eq!(fs::read(&fixture.target).unwrap(), original_bytes);
        assert_eq!(engine.store.actions("alice", "case").unwrap().len(), 1);
    }

    #[test]
    fn current_relation_answer_lineage_and_guarantee_replay_for_recovery() {
        let fixture = empty_fixture();
        let mut engine = Engine::open(&fixture.state).unwrap();

        let rule = engine
            .execute(
                "alice",
                RequestEnvelope::new("case", "rule", "alice", "もしXは鳥ならXは動物"),
            )
            .unwrap();
        let rule_id = rule.dependencies[0].clone();
        let observation = engine
            .execute(
                "alice",
                RequestEnvelope::new("case", "observation", "alice", "ココは鳥です"),
            )
            .unwrap();
        let observation_id = observation.dependencies[0].clone();
        let derived = engine
            .execute(
                "alice",
                RequestEnvelope::new("case", "derived", "alice", "ココは動物ですか"),
            )
            .unwrap();
        assert_eq!(derived.guarantee, Guarantee::Derived);
        assert!(derived.dependencies.contains(&rule_id));
        assert!(derived.dependencies.contains(&observation_id));

        let counterexample = engine
            .execute(
                "alice",
                RequestEnvelope::new("case", "counterexample", "alice", "ココは動物ではない"),
            )
            .unwrap();
        let counterexample_id = counterexample.dependencies[0].clone();
        assert_eq!(
            engine
                .execute(
                    "alice",
                    RequestEnvelope::new("case", "conflict", "alice", "ココは動物ですか"),
                )
                .unwrap()
                .lifecycle,
            Lifecycle::Suspend
        );
        engine
            .execute(
                "alice",
                RequestEnvelope::new(
                    "case",
                    "invalidate",
                    "alice",
                    &format!("{rule_id}を無効化して"),
                ),
            )
            .unwrap();

        let mut request =
            RequestEnvelope::new("case", "after-invalidation", "alice", "ココは動物ですか");
        request.budget.max_actions = 32;
        request.output_path = Some(fixture.target.clone());
        request
            .permissions
            .write_paths
            .insert(fixture.target.clone());
        let result = engine.execute("alice", request).unwrap();
        assert_eq!(result.lifecycle, Lifecycle::Commit);
        assert_eq!(result.guarantee, Guarantee::UserPremise);
        assert_eq!(result.dependencies, vec![counterexample_id.clone()]);
        assert!(result.text.contains("ココは動物ではない"));
        let revision = engine.latest("alice", "case").unwrap().unwrap();
        let memory_revision = result
            .actor_memory_revision
            .expect("Core records the actor-memory revision for the relation answer");
        let original_memory = engine
            .store
            .load_memory_revision("alice", memory_revision)
            .unwrap()
            .expect("the relation answer's immutable actor-memory revision is retained");
        assert!(inspect(&engine.store, "alice", &revision, &result).is_ok());
        let original_bytes = fs::read(&fixture.target).unwrap();

        drop(engine);
        let mut engine = Engine::open(&fixture.state).unwrap();
        assert_eq!(
            engine
                .store
                .load_memory_revision("alice", memory_revision)
                .unwrap(),
            Some(original_memory)
        );
        let database = Connection::open(fixture.state.join("state.sqlite3")).unwrap();
        database
            .execute("DELETE FROM memory_index_state WHERE actor_id='alice'", [])
            .unwrap();
        drop(database);
        assert!(inspect(&engine.store, "alice", &revision, &result).is_ok());

        let history_request = RequestEnvelope::new(
            "history-case",
            "relation-query-off-scope-history",
            "alice",
            "関係照会Recoveryの範囲外履歴",
        );
        let history_source = identity(&history_request).unwrap();
        engine
            .store
            .register_input("alice", &history_request, &history_source)
            .unwrap();
        let mut current_memory = engine.store.load_memory("alice").unwrap();
        let next_revision = current_memory.revision + 1;
        for ordinal in 0..80 {
            crate::relations::assert_fact(
                &mut current_memory,
                Proposition {
                    subject: format!("archive-subject-{ordinal}"),
                    predicate: "is".into(),
                    object: format!("archive-object-{ordinal}"),
                    negative: false,
                    scope: format!("archive-{ordinal}"),
                    condition_refs: BTreeSet::new(),
                    temporal_scope: None,
                },
                &history_source,
            )
            .unwrap();
        }
        current_memory.revision = next_revision;
        engine.store.save_memory("alice", &current_memory).unwrap();
        let goal_ref = revision.goal_sha256.as_ref().unwrap();
        let goal: GoalContract =
            serde_json::from_slice(&engine.store.get_bytes("alice", "case", goal_ref).unwrap())
                .unwrap();
        let (Intent::Ask { proposition } | Intent::AskWhy { proposition }) = &goal.intent else {
            panic!("retained relation answer lost its typed query");
        };
        let full_answer =
            crate::relations::query(&current_memory, proposition, goal.budget.max_actions).unwrap();
        let (projected_memory, indexed_answer) = crate::core::indexed_relation_query_from_store(
            &engine.store,
            "alice",
            proposition,
            goal.budget.max_actions,
        )
        .unwrap()
        .expect("current relation index is available");
        let indexed_answer = indexed_answer.unwrap();
        assert_eq!(current_memory.facts.len(), 82);
        assert_eq!(indexed_answer.status, full_answer.status);
        assert_eq!(indexed_answer.facts, full_answer.facts);
        assert_eq!(indexed_answer.used_rules, full_answer.used_rules);
        assert_eq!(
            crate::relations::answer_dependencies(&indexed_answer),
            crate::relations::answer_dependencies(&full_answer)
        );
        assert_eq!(
            crate::relations::answer_guarantee(&indexed_answer),
            crate::relations::answer_guarantee(&full_answer)
        );
        assert_eq!(projected_memory.revision, current_memory.revision);
        assert_eq!(projected_memory.facts.len(), 1);
        assert_eq!(projected_memory.rules.len(), 0);
        assert!(inspect(&engine.store, "alice", &revision, &result).is_ok());

        let invalidation = engine
            .execute(
                "alice",
                RequestEnvelope::new(
                    "case",
                    "invalidate-current-counterexample",
                    "alice",
                    &format!("{counterexample_id}を無効化して"),
                ),
            )
            .unwrap();
        assert_eq!(invalidation.lifecycle, Lifecycle::Commit);
        let error = inspect(&engine.store, "alice", &revision, &result).unwrap_err();
        assert!(error.to_string().contains("RECOVERY_PROOF_INVALIDATED"));
        assert_eq!(fs::read(&fixture.target).unwrap(), original_bytes);
    }

    #[test]
    fn premise_inventory_recovery_pins_and_rechecks_its_memory_revision() {
        let fixture = empty_fixture();
        let mut engine = Engine::open(&fixture.state).unwrap();
        let rule = engine
            .execute(
                "alice",
                RequestEnvelope::new("case", "rule", "alice", "もしXは鳥ならXは動物"),
            )
            .unwrap();
        let rule_id = rule.dependencies[0].clone();
        let premise = engine
            .execute(
                "alice",
                RequestEnvelope::new("case", "premise", "alice", "ココは鳥です"),
            )
            .unwrap();
        let premise_id = premise.dependencies[0].clone();

        let mut request = RequestEnvelope::new(
            "case",
            "premise-inventory",
            "alice",
            "ココについて、何が分かっていますか",
        );
        request.budget.max_actions = 64;
        request.output_path = Some(fixture.target.clone());
        request
            .permissions
            .write_paths
            .insert(fixture.target.clone());
        let answer = engine.execute("alice", request).unwrap();
        assert_eq!(answer.lifecycle, Lifecycle::Commit);
        assert_eq!(answer.guarantee, Guarantee::Derived);
        assert!(answer.dependencies.contains(&rule_id));
        assert!(answer.dependencies.contains(&premise_id));
        let memory_revision = answer
            .actor_memory_revision
            .expect("Core records the actor-memory snapshot used by the inventory");
        let original_memory = engine
            .store
            .load_memory_revision("alice", memory_revision)
            .unwrap()
            .expect("the inventory's actor-memory snapshot is retained");
        assert!(
            original_memory
                .rules
                .iter()
                .any(|stored| stored.id == rule_id)
        );
        assert!(
            original_memory
                .facts
                .iter()
                .any(|stored| stored.id == premise_id)
        );

        let retained = engine.latest("alice", "case").unwrap().unwrap();
        let retained_result = retained.result.as_ref().unwrap();
        assert_eq!(retained_result.actor_memory_revision, Some(memory_revision));
        assert!(inspect(&engine.store, "alice", &retained, retained_result).is_ok());
        let original_bytes = fs::read(&fixture.target).unwrap();

        drop(engine);
        let mut engine = Engine::open(&fixture.state).unwrap();
        assert_eq!(
            engine
                .store
                .load_memory_revision("alice", memory_revision)
                .unwrap(),
            Some(original_memory)
        );
        assert!(inspect(&engine.store, "alice", &retained, retained_result).is_ok());

        let database = Connection::open(fixture.state.join("state.sqlite3")).unwrap();
        database
            .execute("DELETE FROM memory_index_state WHERE actor_id='alice'", [])
            .unwrap();
        drop(database);
        assert!(inspect(&engine.store, "alice", &retained, retained_result).is_ok());

        let history_request = RequestEnvelope::new(
            "history-case",
            "premise-inventory-recovery-off-scope-history",
            "alice",
            "前提inventoryRecoveryの範囲外履歴",
        );
        let history_source = identity(&history_request).unwrap();
        engine
            .store
            .register_input("alice", &history_request, &history_source)
            .unwrap();
        let mut current_memory = engine.store.load_memory("alice").unwrap();
        let next_revision = current_memory.revision + 1;
        for ordinal in 0..80 {
            crate::relations::assert_fact(
                &mut current_memory,
                Proposition {
                    subject: format!("archive-subject-{ordinal}"),
                    predicate: "is".into(),
                    object: format!("archive-object-{ordinal}"),
                    negative: false,
                    scope: format!("archive-{ordinal}"),
                    condition_refs: BTreeSet::new(),
                    temporal_scope: None,
                },
                &history_source,
            )
            .unwrap();
        }
        current_memory.revision = next_revision;
        engine.store.save_memory("alice", &current_memory).unwrap();

        let goal_ref = retained.goal_sha256.as_ref().unwrap();
        let goal: GoalContract =
            serde_json::from_slice(&engine.store.get_bytes("alice", "case", goal_ref).unwrap())
                .unwrap();
        let Intent::InspectPremises { subject } = &goal.intent else {
            panic!("retained premise inventory lost its typed subject");
        };
        let full_inventory =
            crate::relations::inventory_about(&current_memory, subject, 10_000).unwrap();
        let (indexed_revision, indexed_inventory) =
            crate::core::indexed_premise_inventory_from_store(
                &engine.store,
                "alice",
                subject,
                goal.budget.max_actions,
            )
            .unwrap()
            .expect("current premise subject index is available");
        let indexed_inventory = indexed_inventory.unwrap();
        assert_eq!(current_memory.facts.len(), 81);
        assert_eq!(indexed_revision, current_memory.revision);
        assert_eq!(indexed_inventory, full_inventory);
        assert!(matches!(
            crate::relations::inventory_about(
                &current_memory,
                subject,
                goal.budget.max_actions,
            ),
            Err(Error::Contract { code, .. }) if code == "RELATION_BUDGET"
        ));
        assert!(inspect(&engine.store, "alice", &retained, retained_result).is_ok());

        let invalidation = engine
            .execute(
                "alice",
                RequestEnvelope::new(
                    "case",
                    "invalidate-inventory-rule",
                    "alice",
                    &format!("{rule_id}を無効化して"),
                ),
            )
            .unwrap();
        assert_eq!(invalidation.lifecycle, Lifecycle::Commit);
        let error = inspect(&engine.store, "alice", &retained, retained_result).unwrap_err();
        assert!(error.to_string().contains("RECOVERY_PREMISES_CHANGED"));
        assert_eq!(fs::read(&fixture.target).unwrap(), original_bytes);
    }

    #[test]
    fn conjunction_answer_recovery_pins_and_rechecks_its_memory_revision() {
        let fixture = empty_fixture();
        let mut engine = Engine::open(&fixture.state).unwrap();
        let bird = engine
            .execute(
                "alice",
                RequestEnvelope::new("case", "bird", "alice", "ココは鳥です"),
            )
            .unwrap();
        let bird_id = bird.dependencies[0].clone();
        let animal = engine
            .execute(
                "alice",
                RequestEnvelope::new("case", "animal", "alice", "ココは動物です"),
            )
            .unwrap();
        let animal_id = animal.dependencies[0].clone();

        let mut request =
            RequestEnvelope::new("case", "conjunction", "alice", "ココは鳥かつ動物ですか");
        request.budget.max_actions = 32;
        request.output_path = Some(fixture.target.clone());
        request
            .permissions
            .write_paths
            .insert(fixture.target.clone());
        let answer = engine.execute("alice", request).unwrap();
        assert_eq!(answer.lifecycle, Lifecycle::Commit);
        assert!(answer.dependencies.contains(&bird_id));
        assert!(answer.dependencies.contains(&animal_id));
        let memory_revision = answer
            .actor_memory_revision
            .expect("Core records the actor-memory snapshot used by the conjunction");
        let original_memory = engine
            .store
            .load_memory_revision("alice", memory_revision)
            .unwrap()
            .expect("the conjunction's actor-memory snapshot is retained");
        assert!(original_memory.facts.iter().any(|fact| fact.id == bird_id));
        assert!(
            original_memory
                .facts
                .iter()
                .any(|fact| fact.id == animal_id)
        );

        let retained = engine.latest("alice", "case").unwrap().unwrap();
        let retained_result = retained.result.as_ref().unwrap();
        assert_eq!(retained_result.actor_memory_revision, Some(memory_revision));
        assert!(inspect(&engine.store, "alice", &retained, retained_result).is_ok());

        drop(engine);
        let mut engine = Engine::open(&fixture.state).unwrap();
        assert_eq!(
            engine
                .store
                .load_memory_revision("alice", memory_revision)
                .unwrap(),
            Some(original_memory)
        );
        assert!(inspect(&engine.store, "alice", &retained, retained_result).is_ok());

        let database = Connection::open(fixture.state.join("state.sqlite3")).unwrap();
        database
            .execute("DELETE FROM memory_index_state WHERE actor_id='alice'", [])
            .unwrap();
        drop(database);
        assert!(inspect(&engine.store, "alice", &retained, retained_result).is_ok());

        let history_request = RequestEnvelope::new(
            "history-case",
            "conjunction-recovery-off-scope-history",
            "alice",
            "連言照会Recoveryの範囲外履歴",
        );
        let history_source = identity(&history_request).unwrap();
        engine
            .store
            .register_input("alice", &history_request, &history_source)
            .unwrap();
        let mut current_memory = engine.store.load_memory("alice").unwrap();
        let next_revision = current_memory.revision + 1;
        for ordinal in 0..80 {
            crate::relations::assert_fact(
                &mut current_memory,
                Proposition {
                    subject: format!("archive-subject-{ordinal}"),
                    predicate: "is".into(),
                    object: format!("archive-object-{ordinal}"),
                    negative: false,
                    scope: format!("archive-{ordinal}"),
                    condition_refs: BTreeSet::new(),
                    temporal_scope: None,
                },
                &history_source,
            )
            .unwrap();
        }
        current_memory.revision = next_revision;
        engine.store.save_memory("alice", &current_memory).unwrap();

        let goal_ref = retained.goal_sha256.as_ref().unwrap();
        let goal: GoalContract =
            serde_json::from_slice(&engine.store.get_bytes("alice", "case", goal_ref).unwrap())
                .unwrap();
        let Intent::AskConjunction { propositions } = &goal.intent else {
            panic!("retained conjunction lost its typed query");
        };
        let clause_count = u32::try_from(propositions.len()).unwrap();
        let per_relation_budget = goal.budget.max_actions / clause_count;
        let full_answers = propositions
            .iter()
            .map(|proposition| {
                crate::relations::query(&current_memory, proposition, per_relation_budget).unwrap()
            })
            .collect::<Vec<_>>();
        let (indexed_revision, indexed_answers) =
            crate::core::indexed_conjunction_relation_answers_from_store(
                &engine.store,
                "alice",
                propositions,
                per_relation_budget,
            )
            .unwrap()
            .expect("current conjunction relation index is available");
        let indexed_answers = indexed_answers.unwrap();
        assert_eq!(current_memory.facts.len(), 82);
        assert_eq!(indexed_revision, current_memory.revision);
        assert_eq!(indexed_answers.len(), full_answers.len());
        for (indexed, full) in indexed_answers.iter().zip(&full_answers) {
            assert_eq!(indexed.status, full.status);
            assert_eq!(indexed.facts, full.facts);
            assert_eq!(indexed.used_rules, full.used_rules);
            assert_eq!(
                crate::relations::answer_dependencies(indexed),
                crate::relations::answer_dependencies(full)
            );
            assert_eq!(
                crate::relations::answer_guarantee(indexed),
                crate::relations::answer_guarantee(full)
            );
        }
        let projected_fact_counts = engine
            .store
            .with_relation_index("alice", |index| {
                propositions
                    .iter()
                    .map(|proposition| {
                        crate::relations::active_relation_projection(
                            index,
                            proposition,
                            per_relation_budget,
                        )
                        .map(|(memory, _)| memory.facts.len())
                    })
                    .collect::<Result<Vec<_>>>()
            })
            .unwrap()
            .expect("current conjunction relation index remains revision-matched");
        assert_eq!(projected_fact_counts, vec![1, 1]);
        assert!(inspect(&engine.store, "alice", &retained, retained_result).is_ok());
        let original_bytes = fs::read(&fixture.target).unwrap();

        let invalidation = engine
            .execute(
                "alice",
                RequestEnvelope::new(
                    "case",
                    "invalidate-bird-premise",
                    "alice",
                    &format!("{bird_id}を無効化して"),
                ),
            )
            .unwrap();
        assert_eq!(invalidation.lifecycle, Lifecycle::Commit);
        let error = inspect(&engine.store, "alice", &retained, retained_result).unwrap_err();
        assert!(error.to_string().contains("RECOVERY_PROOF_INVALIDATED"));
        assert_eq!(fs::read(&fixture.target).unwrap(), original_bytes);
    }

    #[test]
    fn object_answer_recovery_rechecks_proof_before_confirming_saved_bytes() {
        let fixture = empty_fixture();
        let mut engine = Engine::open(&fixture.state).unwrap();
        let premise = engine
            .execute(
                "alice",
                RequestEnvelope::new("case", "premise", "alice", "ミナは水を飲む"),
            )
            .unwrap();
        let premise_id = premise.dependencies[0].clone();
        let mut request = RequestEnvelope::new("case", "object-answer", "alice", "ミナは何を飲む?");
        request.budget.max_actions = 32;
        request.output_path = Some(fixture.target.clone());
        request
            .permissions
            .write_paths
            .insert(fixture.target.clone());
        let answer = engine.execute("alice", request).unwrap();
        assert_eq!(answer.lifecycle, Lifecycle::Commit);
        assert!(answer.dependencies.contains(&premise_id));
        let memory_revision = answer
            .actor_memory_revision
            .expect("Core records the actor-memory snapshot used by the object answer");
        let original_memory = engine
            .store
            .load_memory_revision("alice", memory_revision)
            .unwrap()
            .expect("the referenced actor-memory revision is retained");
        assert!(
            original_memory
                .facts
                .iter()
                .any(|fact| fact.id == premise_id)
        );
        let original_bytes = fs::read(&fixture.target).unwrap();

        let database = Connection::open(fixture.state.join("state.sqlite3")).unwrap();
        let snapshots: Vec<(i64, String)> = {
            let mut statement = database
                .prepare("SELECT revision,state_sha256 FROM case_revision ORDER BY revision")
                .unwrap();
            statement
                .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
                .unwrap()
                .collect::<std::result::Result<_, _>>()
                .unwrap()
        };
        let mut retained = None;
        for (number, sha) in snapshots {
            let revision: CaseRevision =
                serde_json::from_slice(&fs::read(fixture.state.join("blobs").join(sha)).unwrap())
                    .unwrap();
            if revision.lifecycle == Lifecycle::Adopted
                && revision.bundle.as_ref().is_some_and(|bundle| {
                    matches!(bundle.meaning.intent, Intent::AskRelationObjects { .. })
                })
            {
                retained = Some((number, revision));
            }
        }
        let (_, adopted) =
            retained.expect("Core persisted the object answer before its file write");
        let adopted_result = adopted.result.as_ref().unwrap();
        assert_eq!(adopted_result.actor_memory_revision, Some(memory_revision));
        assert!(inspect(&engine.store, "alice", &adopted, adopted_result).is_ok());

        drop(database);
        drop(engine);
        let mut engine = Engine::open(&fixture.state).unwrap();
        assert_eq!(
            engine
                .store
                .load_memory_revision("alice", memory_revision)
                .unwrap(),
            Some(original_memory)
        );
        let database = Connection::open(fixture.state.join("state.sqlite3")).unwrap();
        database
            .execute("DELETE FROM memory_index_state WHERE actor_id='alice'", [])
            .unwrap();
        drop(database);
        assert!(inspect(&engine.store, "alice", &adopted, adopted_result).is_ok());

        let history_request = RequestEnvelope::new(
            "history-case",
            "relation-objects-off-scope-history",
            "alice",
            "関係値Recoveryの範囲外履歴",
        );
        let history_source = identity(&history_request).unwrap();
        engine
            .store
            .register_input("alice", &history_request, &history_source)
            .unwrap();
        let mut current_memory = engine.store.load_memory("alice").unwrap();
        let next_revision = current_memory.revision + 1;
        for ordinal in 0..80 {
            crate::relations::assert_fact(
                &mut current_memory,
                Proposition {
                    subject: format!("archive-subject-{ordinal}"),
                    predicate: "is".into(),
                    object: format!("archive-object-{ordinal}"),
                    negative: false,
                    scope: format!("archive-{ordinal}"),
                    condition_refs: BTreeSet::new(),
                    temporal_scope: None,
                },
                &history_source,
            )
            .unwrap();
        }
        current_memory.revision = next_revision;
        engine.store.save_memory("alice", &current_memory).unwrap();
        let goal_ref = adopted.goal_sha256.as_ref().unwrap();
        let goal: GoalContract =
            serde_json::from_slice(&engine.store.get_bytes("alice", "case", goal_ref).unwrap())
                .unwrap();
        let Intent::AskRelationObjects {
            subject,
            predicate,
            negative,
            scope,
        } = &goal.intent
        else {
            panic!("retained object answer lost its typed goal");
        };
        let full_snapshot_error = crate::core::relation_objects_answer(
            &current_memory,
            subject,
            predicate,
            *negative,
            scope,
            goal.budget.max_actions,
            goal.budget.max_evidence_bytes,
        )
        .unwrap_err();
        assert!(
            full_snapshot_error.to_string().contains("RESOURCE_LIMIT"),
            "{full_snapshot_error:?}"
        );
        assert!(inspect(&engine.store, "alice", &adopted, adopted_result).is_ok());

        let invalidation = engine
            .execute(
                "alice",
                RequestEnvelope::new(
                    "case",
                    "invalidate-drink-premise",
                    "alice",
                    &format!("{premise_id}を無効化して"),
                ),
            )
            .unwrap();
        assert_eq!(invalidation.lifecycle, Lifecycle::Commit);
        let error = inspect(&engine.store, "alice", &adopted, adopted_result).unwrap_err();
        assert!(error.to_string().contains("RECOVERY_PROOF_INVALIDATED"));
        assert_eq!(fs::read(&fixture.target).unwrap(), original_bytes);
    }

    #[test]
    fn previous_relation_recovery_pins_and_rechecks_its_memory_revision() {
        let fixture = empty_fixture();
        let mut engine = Engine::open(&fixture.state).unwrap();
        let premise = engine
            .execute(
                "alice",
                RequestEnvelope::new("case", "premise", "alice", "ココは鳥です"),
            )
            .unwrap();
        let premise_id = premise.dependencies[0].clone();
        let prior = engine
            .execute(
                "alice",
                RequestEnvelope::new("case", "explicit-query", "alice", "ココは鳥ですか"),
            )
            .unwrap();
        assert_eq!(prior.lifecycle, Lifecycle::Commit);
        assert!(prior.dependencies.contains(&premise_id));

        // Add retained history after the prior result without changing the
        // same-case context that the pronoun query must resolve.
        let noise_request = RequestEnvelope::new(
            "noise-case",
            "noise-source",
            "alice",
            "長い履歴のテストデータ",
        );
        let noise_source = identity(&noise_request).unwrap();
        engine
            .store
            .register_input("alice", &noise_request, &noise_source)
            .unwrap();
        let mut memory = engine.store.load_memory("alice").unwrap();
        let next_revision = memory.revision + 1;
        for ordinal in 0..72 {
            crate::relations::assert_fact(
                &mut memory,
                Proposition {
                    subject: format!("unrelated-{ordinal}"),
                    predicate: format!("unrelated-predicate-{ordinal}"),
                    object: format!("unrelated-value-{ordinal}"),
                    negative: false,
                    scope: "default".into(),
                    condition_refs: BTreeSet::new(),
                    temporal_scope: None,
                },
                &noise_source,
            )
            .unwrap();
        }
        for (premise_object, conclusion_object) in
            [("鳥", "脊椎動物"), ("脊椎動物", "生物"), ("生物", "動物")]
        {
            crate::relations::add_rule(
                &mut memory,
                Proposition {
                    subject: "$subject".into(),
                    predicate: "is".into(),
                    object: premise_object.into(),
                    negative: false,
                    scope: "default".into(),
                    condition_refs: BTreeSet::new(),
                    temporal_scope: None,
                },
                Proposition {
                    subject: "$subject".into(),
                    predicate: "is".into(),
                    object: conclusion_object.into(),
                    negative: false,
                    scope: "default".into(),
                    condition_refs: BTreeSet::new(),
                    temporal_scope: None,
                },
                &noise_source,
            )
            .unwrap();
        }
        memory.revision = next_revision;
        engine.store.save_memory("alice", &memory).unwrap();

        let mut request =
            RequestEnvelope::new("case", "prior-pronoun-query", "alice", "それは鳥ですか");
        request.output_path = Some(fixture.target.clone());
        request
            .permissions
            .write_paths
            .insert(fixture.target.clone());
        let answer = engine.execute("alice", request).unwrap();
        assert_eq!(answer.lifecycle, Lifecycle::Commit, "{answer:?}");
        assert!(answer.text.contains("ココは鳥"));
        assert!(answer.dependencies.contains(&premise_id));
        let memory_revision = answer
            .actor_memory_revision
            .expect("Core records the actor-memory snapshot used to resolve the prior result");
        assert_eq!(memory_revision, 2);
        let original_memory = engine
            .store
            .load_memory_revision("alice", memory_revision)
            .unwrap()
            .expect("the prior-relation actor-memory snapshot is retained");
        assert_eq!(original_memory.facts.len(), 73);
        assert!(
            original_memory
                .facts
                .iter()
                .any(|fact| fact.id == premise_id)
        );

        let mut limited_request = RequestEnvelope::new(
            "case",
            "prior-pronoun-budget-stop",
            "alice",
            "それは動物ですか",
        );
        limited_request.budget.max_actions = 11;
        let budget_stop = engine.execute("alice", limited_request).unwrap();
        assert_eq!(budget_stop.lifecycle, Lifecycle::Suspend, "{budget_stop:?}");
        assert!(
            budget_stop
                .reason
                .as_deref()
                .is_some_and(|reason| reason.contains("RESOURCE_LIMIT"))
        );
        assert_eq!(budget_stop.actor_memory_revision, Some(memory_revision));

        let database = Connection::open(fixture.state.join("state.sqlite3")).unwrap();
        let snapshots: Vec<(i64, String)> = {
            let mut statement = database
                .prepare("SELECT revision,state_sha256 FROM case_revision ORDER BY revision")
                .unwrap();
            statement
                .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
                .unwrap()
                .collect::<std::result::Result<_, _>>()
                .unwrap()
        };
        let mut adopted = None;
        for (number, sha) in snapshots {
            let revision: CaseRevision =
                serde_json::from_slice(&fs::read(fixture.state.join("blobs").join(sha)).unwrap())
                    .unwrap();
            if revision.lifecycle == Lifecycle::Adopted
                && revision
                    .bundle
                    .as_ref()
                    .is_some_and(|bundle| bundle.request.input_id == "prior-pronoun-query")
            {
                adopted = Some((number, revision));
            }
        }
        let (_, adopted) =
            adopted.expect("Core persisted the pronoun answer before its file write");
        let adopted_result = adopted.result.as_ref().unwrap().clone();
        assert_eq!(adopted_result.actor_memory_revision, Some(memory_revision));
        assert!(inspect(&engine.store, "alice", &adopted, &adopted_result).is_ok());
        let original_bytes = fs::read(&fixture.target).unwrap();

        drop(database);
        drop(engine);
        let engine = Engine::open(&fixture.state).unwrap();
        assert_eq!(
            engine
                .store
                .load_memory_revision("alice", memory_revision)
                .unwrap(),
            Some(original_memory)
        );
        assert!(inspect(&engine.store, "alice", &adopted, &adopted_result).is_ok());

        let database = Connection::open(fixture.state.join("state.sqlite3")).unwrap();
        database
            .execute("DELETE FROM memory_index_state WHERE actor_id='alice'", [])
            .unwrap();
        drop(database);
        assert!(inspect(&engine.store, "alice", &adopted, &adopted_result).is_ok());

        let history_request = RequestEnvelope::new(
            "extended-history-case",
            "previous-relation-recovery-off-scope-history",
            "alice",
            "直前関係Recoveryの範囲外履歴",
        );
        let history_source = identity(&history_request).unwrap();
        engine
            .store
            .register_input("alice", &history_request, &history_source)
            .unwrap();
        let mut current_memory = engine.store.load_memory("alice").unwrap();
        let next_revision = current_memory.revision + 1;
        for ordinal in 0..80 {
            crate::relations::assert_fact(
                &mut current_memory,
                Proposition {
                    subject: format!("archive-subject-{ordinal}"),
                    predicate: "is".into(),
                    object: format!("archive-object-{ordinal}"),
                    negative: false,
                    scope: format!("archive-{ordinal}"),
                    condition_refs: BTreeSet::new(),
                    temporal_scope: None,
                },
                &history_source,
            )
            .unwrap();
        }
        current_memory.revision = next_revision;
        engine.store.save_memory("alice", &current_memory).unwrap();

        let goal_ref = adopted.goal_sha256.as_ref().unwrap();
        let goal: GoalContract =
            serde_json::from_slice(&engine.store.get_bytes("alice", "case", goal_ref).unwrap())
                .unwrap();
        let Intent::AskPreviousRelation {
            proposition,
            explain,
            explicit_scope,
        } = &goal.intent
        else {
            panic!("retained prior-relation answer lost its typed query");
        };
        let previous_id = adopted
            .bundle
            .as_ref()
            .and_then(|bundle| bundle.meaning.context_result_ref.as_deref())
            .expect("the sealed query retains its prior-result reference");
        let previous = adopted
            .prior_results
            .get(previous_id)
            .expect("the exact prior result remains retained");
        let resolved = match crate::core::resolve_previous_relation_proposition(
            &current_memory,
            previous,
            proposition,
            explicit_scope.as_deref(),
        ) {
            crate::core::PreviousRelationResolution::Resolved(proposition) => proposition,
            crate::core::PreviousRelationResolution::Missing
            | crate::core::PreviousRelationResolution::Ambiguous => {
                panic!("full current memory no longer resolves the prior relation")
            }
        };
        let full_answer =
            crate::relations::query(&current_memory, &resolved, goal.budget.max_actions).unwrap();
        let full_units = crate::core::previous_relation_success_units(
            &current_memory,
            &resolved,
            &full_answer,
            *explain,
        )
        .expect("full current memory reproduces the adopted clause");
        let indexed = crate::core::indexed_previous_relation_query_from_store(
            &engine.store,
            "alice",
            previous,
            proposition,
            explicit_scope.as_deref(),
            goal.budget.max_actions,
        )
        .unwrap()
        .expect("current prior-relation index is available");
        assert_eq!(indexed.memory.revision, current_memory.revision);
        let indexed_resolved = match indexed.resolution.as_ref() {
            Some(crate::core::PreviousRelationResolution::Resolved(proposition)) => {
                proposition.clone()
            }
            _ => panic!("indexed prior dependencies no longer resolve uniquely"),
        };
        assert_eq!(indexed_resolved, resolved);
        let indexed_answer = indexed.answer.unwrap().unwrap();
        assert_eq!(indexed_answer.status, full_answer.status);
        assert_eq!(indexed_answer.facts, full_answer.facts);
        assert_eq!(indexed_answer.used_rules, full_answer.used_rules);
        assert_eq!(
            crate::relations::answer_dependencies(&indexed_answer),
            crate::relations::answer_dependencies(&full_answer)
        );
        assert_eq!(
            crate::relations::answer_guarantee(&indexed_answer),
            crate::relations::answer_guarantee(&full_answer)
        );
        let indexed_units = crate::core::previous_relation_success_units(
            &indexed.memory,
            &indexed_resolved,
            &indexed_answer,
            *explain,
        )
        .expect("indexed current memory reproduces the adopted clause");
        assert_eq!(indexed_units, full_units);
        assert_eq!(current_memory.facts.len(), 153);
        assert!(indexed.memory.facts.len() < current_memory.facts.len());
        assert!(inspect(&engine.store, "alice", &adopted, &adopted_result).is_ok());

        let mut engine = engine;
        let explanation_prior = engine
            .execute(
                "alice",
                RequestEnvelope::new(
                    "explanation-case",
                    "explicit-explanation-query",
                    "alice",
                    "ココは鳥ですか",
                ),
            )
            .unwrap();
        assert_eq!(explanation_prior.lifecycle, Lifecycle::Commit);
        let explanation = engine
            .execute(
                "alice",
                RequestEnvelope::new(
                    "explanation-case",
                    "prior-pronoun-explanation",
                    "alice",
                    "なぜそれは鳥ですか",
                ),
            )
            .unwrap();
        assert_eq!(explanation.lifecycle, Lifecycle::Commit, "{explanation:?}");
        assert!(explanation.text.contains("提示された証拠が支える結論"));
        assert!(explanation.actor_memory_revision.is_some());

        let invalidation = engine
            .execute(
                "alice",
                RequestEnvelope::new(
                    "invalidation-case",
                    "invalidate-pronoun-premise",
                    "alice",
                    &format!("{premise_id}を無効化して"),
                ),
            )
            .unwrap();
        assert_eq!(invalidation.lifecycle, Lifecycle::Commit);
        let error = inspect(&engine.store, "alice", &adopted, &adopted_result).unwrap_err();
        assert!(error.to_string().contains("RECOVERY_PROOF_INVALIDATED"));
        assert_eq!(fs::read(&fixture.target).unwrap(), original_bytes);
    }

    #[test]
    fn previous_answer_explanation_pins_and_rechecks_its_memory_revision() {
        let fixture = empty_fixture();
        let mut engine = Engine::open(&fixture.state).unwrap();
        let premise = engine
            .execute(
                "alice",
                RequestEnvelope::new("case", "premise", "alice", "ココは鳥です"),
            )
            .unwrap();
        let premise_id = premise.dependencies[0].clone();
        let prior = engine
            .execute(
                "alice",
                RequestEnvelope::new("case", "explicit-query", "alice", "ココは鳥ですか"),
            )
            .unwrap();
        assert_eq!(prior.lifecycle, Lifecycle::Commit);
        assert!(prior.dependencies.contains(&premise_id));

        let mut request = RequestEnvelope::new("case", "explanation", "alice", "詳しく説明して");
        request.budget.max_actions = 32;
        request.output_path = Some(fixture.target.clone());
        request
            .permissions
            .write_paths
            .insert(fixture.target.clone());
        let explanation = engine.execute("alice", request).unwrap();
        assert_eq!(explanation.lifecycle, Lifecycle::Commit, "{explanation:?}");
        assert!(explanation.text.contains("前回の結論"));
        assert!(explanation.dependencies.contains(&prior.id));
        assert!(explanation.dependencies.contains(&premise_id));
        let memory_revision = explanation
            .actor_memory_revision
            .expect("Core records the actor-memory snapshot used by the explanation");
        let original_memory = engine
            .store
            .load_memory_revision("alice", memory_revision)
            .unwrap()
            .expect("the explanation's actor-memory revision is retained");
        assert!(
            original_memory
                .facts
                .iter()
                .any(|fact| fact.id == premise_id)
        );

        let retained = engine.latest("alice", "case").unwrap().unwrap();
        let retained_result = retained.result.as_ref().unwrap().clone();
        assert_eq!(retained_result.actor_memory_revision, Some(memory_revision));
        assert!(matches!(
            retained.bundle.as_ref().unwrap().meaning.intent,
            Intent::ExplainPrevious
        ));
        inspect(&engine.store, "alice", &retained, &retained_result)
            .unwrap_or_else(|error| panic!("{error:?}"));
        let original_bytes = fs::read(&fixture.target).unwrap();

        drop(engine);
        let mut engine = Engine::open(&fixture.state).unwrap();
        assert_eq!(
            engine
                .store
                .load_memory_revision("alice", memory_revision)
                .unwrap(),
            Some(original_memory)
        );
        assert!(inspect(&engine.store, "alice", &retained, &retained_result).is_ok());

        let database = Connection::open(fixture.state.join("state.sqlite3")).unwrap();
        database
            .execute("DELETE FROM memory_index_state WHERE actor_id='alice'", [])
            .unwrap();
        drop(database);
        assert!(inspect(&engine.store, "alice", &retained, &retained_result).is_ok());

        let history_request = RequestEnvelope::new(
            "extended-history-case",
            "previous-answer-explanation-off-scope-history",
            "alice",
            "前回回答説明Recoveryの範囲外履歴",
        );
        let history_source = identity(&history_request).unwrap();
        engine
            .store
            .register_input("alice", &history_request, &history_source)
            .unwrap();
        let mut current_memory = engine.store.load_memory("alice").unwrap();
        let next_revision = current_memory.revision + 1;
        for ordinal in 0..80 {
            crate::relations::assert_fact(
                &mut current_memory,
                Proposition {
                    subject: format!("archive-subject-{ordinal}"),
                    predicate: "is".into(),
                    object: format!("archive-object-{ordinal}"),
                    negative: false,
                    scope: format!("archive-{ordinal}"),
                    condition_refs: BTreeSet::new(),
                    temporal_scope: None,
                },
                &history_source,
            )
            .unwrap();
        }
        current_memory.revision = next_revision;
        engine.store.save_memory("alice", &current_memory).unwrap();

        let goal_ref = retained.goal_sha256.as_ref().unwrap();
        let goal: GoalContract =
            serde_json::from_slice(&engine.store.get_bytes("alice", "case", goal_ref).unwrap())
                .unwrap();
        assert!(matches!(goal.intent, Intent::ExplainPrevious));
        let previous_id = retained
            .bundle
            .as_ref()
            .and_then(|bundle| bundle.meaning.context_result_ref.as_deref())
            .expect("the sealed explanation retains its prior-result reference");
        let previous = retained
            .prior_results
            .get(previous_id)
            .expect("the exact prior relation answer remains retained");
        let prior_meaning =
            crate::core::previous_relation_meaning(&engine.store, "alice", "case", previous)
                .unwrap()
                .expect("the prior result retains its typed relation meaning");
        let (proposition, _) = match prior_meaning {
            crate::core::PriorRelationMeaning::OriginalQuestion(proposition) => (proposition, None),
            crate::core::PriorRelationMeaning::AdoptedConclusion {
                proposition,
                proof_dependencies,
            } => (proposition, Some(proof_dependencies)),
        };
        let full_answer = crate::relations::query(&current_memory, &proposition, 10_000).unwrap();
        let (indexed_memory, indexed_answer) = crate::core::indexed_relation_query_from_store(
            &engine.store,
            "alice",
            &proposition,
            goal.budget.max_actions,
        )
        .unwrap()
        .expect("the current relation index is available");
        let indexed_answer = indexed_answer.unwrap();
        assert_eq!(current_memory.facts.len(), 81);
        assert_eq!(indexed_memory.revision, current_memory.revision);
        assert_eq!(indexed_memory.facts.len(), 1);
        assert!(indexed_memory.facts.len() < current_memory.facts.len());
        assert!(indexed_answer.steps <= goal.budget.max_actions);
        assert_eq!(indexed_answer.status, full_answer.status);
        assert_eq!(indexed_answer.facts, full_answer.facts);
        assert_eq!(indexed_answer.used_rules, full_answer.used_rules);
        assert_eq!(
            indexed_answer.condition_evaluations,
            full_answer.condition_evaluations
        );
        assert_eq!(
            indexed_answer.condition_status,
            full_answer.condition_status
        );
        assert_eq!(
            crate::relations::answer_dependencies(&indexed_answer),
            crate::relations::answer_dependencies(&full_answer)
        );
        assert_eq!(
            crate::relations::answer_guarantee(&indexed_answer),
            crate::relations::answer_guarantee(&full_answer)
        );
        assert_eq!(
            crate::core::relation_explanation_lines(
                &indexed_memory,
                &proposition,
                &indexed_answer,
                "前回の結論",
            ),
            crate::core::relation_explanation_lines(
                &current_memory,
                &proposition,
                &full_answer,
                "前回の結論",
            )
        );
        assert!(inspect(&engine.store, "alice", &retained, &retained_result).is_ok());

        let invalidation = engine
            .execute(
                "alice",
                RequestEnvelope::new(
                    "invalidation-case",
                    "invalidate-explanation-premise",
                    "alice",
                    &format!("{premise_id}を無効化して"),
                ),
            )
            .unwrap();
        assert_eq!(invalidation.lifecycle, Lifecycle::Commit);
        let error = inspect(&engine.store, "alice", &retained, &retained_result).unwrap_err();
        assert!(error.to_string().contains("RECOVERY_CONTEXT_PROOF_CHANGED"));
        assert_eq!(fs::read(&fixture.target).unwrap(), original_bytes);
    }

    #[test]
    fn saved_previous_relation_explanation_rechecks_anchor_and_current_head() {
        let fixture = empty_fixture();
        let mut engine = Engine::open(&fixture.state).unwrap();
        let premise = engine
            .execute(
                "alice",
                RequestEnvelope::new("case", "premise", "alice", "ココは鳥です"),
            )
            .unwrap();
        assert_eq!(premise.lifecycle, Lifecycle::Commit, "{premise:?}");
        let premise_id = premise.dependencies[0].clone();

        let explicit_answer = engine
            .execute(
                "alice",
                RequestEnvelope::new("case", "explicit-query", "alice", "ココは鳥ですか"),
            )
            .unwrap();
        assert_eq!(
            explicit_answer.lifecycle,
            Lifecycle::Commit,
            "{explicit_answer:?}"
        );
        assert!(explicit_answer.dependencies.contains(&premise_id));

        let mut prior_relation_request =
            RequestEnvelope::new("case", "prior-relation-why", "alice", "なぜそれは鳥ですか");
        prior_relation_request.budget.max_actions = 64;
        let prior_relation = engine.execute("alice", prior_relation_request).unwrap();
        assert_eq!(
            prior_relation.lifecycle,
            Lifecycle::Commit,
            "{prior_relation:?}"
        );
        assert!(prior_relation.dependencies.contains(&premise_id));
        assert!(prior_relation.actor_memory_revision.is_some());
        let (_, prior_relation_bundle) = engine
            .store
            .get_input_bundle(
                "alice",
                "case",
                &prior_relation.input_id,
                prior_relation.input_version,
            )
            .unwrap()
            .expect("the typed AskPreviousRelation input is retained");
        assert!(matches!(
            &prior_relation_bundle.meaning.intent,
            Intent::AskPreviousRelation { explain: true, .. }
        ));
        let Intent::AskPreviousRelation {
            proposition,
            explain,
            explicit_scope,
        } = &prior_relation_bundle.meaning.intent
        else {
            unreachable!("the prior request was checked as AskPreviousRelation");
        };
        assert!(crate::core::previous_relation_meaning(
            &engine.store,
            "alice",
            "case",
            &prior_relation,
        )
        .unwrap()
        .is_none());

        let mut explanation_request =
            RequestEnvelope::new("case", "explanation", "alice", "詳しく説明して");
        explanation_request.budget.max_actions = 64;
        explanation_request.output_path = Some(fixture.target.clone());
        explanation_request
            .permissions
            .write_paths
            .insert(fixture.target.clone());
        let explanation = engine.execute("alice", explanation_request).unwrap();
        assert_eq!(explanation.lifecycle, Lifecycle::Commit, "{explanation:?}");
        assert_eq!(explanation.text, prior_relation.text);
        assert_eq!(explanation.dependencies, [prior_relation.id.clone()]);
        assert_eq!(explanation.guarantee, prior_relation.guarantee);

        let retained = engine.latest("alice", "case").unwrap().unwrap();
        let retained_result = retained.result.as_ref().unwrap().clone();
        let original_bytes = fs::read(&fixture.target).unwrap();
        inspect(&engine.store, "alice", &retained, &retained_result)
            .unwrap_or_else(|error| panic!("{error:?}"));

        drop(engine);
        let mut engine = Engine::open(&fixture.state).unwrap();
        let database = Connection::open(fixture.state.join("state.sqlite3")).unwrap();
        database
            .execute("DELETE FROM memory_index_state WHERE actor_id='alice'", [])
            .unwrap();
        drop(database);
        assert!(inspect(&engine.store, "alice", &retained, &retained_result).is_ok());

        let history_request = RequestEnvelope::new(
            "history-case",
            "previous-relation-explanation-off-scope-history",
            "alice",
            "前回関係回答説明Recoveryの範囲外履歴",
        );
        let history_source = identity(&history_request).unwrap();
        engine
            .store
            .register_input("alice", &history_request, &history_source)
            .unwrap();
        let mut current_memory = engine.store.load_memory("alice").unwrap();
        let next_revision = current_memory.revision + 1;
        for ordinal in 0..80 {
            crate::relations::assert_fact(
                &mut current_memory,
                Proposition {
                    subject: format!("archive-subject-{ordinal}"),
                    predicate: "is".into(),
                    object: format!("archive-object-{ordinal}"),
                    negative: false,
                    scope: format!("archive-{ordinal}"),
                    condition_refs: BTreeSet::new(),
                    temporal_scope: None,
                },
                &history_source,
            )
            .unwrap();
        }
        current_memory.revision = next_revision;
        engine.store.save_memory("alice", &current_memory).unwrap();

        let anchor_id = prior_relation_bundle
            .meaning
            .context_result_ref
            .as_deref()
            .expect("the saved AskPreviousRelation retains its direct result anchor");
        let anchor = retained
            .prior_results
            .get(anchor_id)
            .expect("the exact direct result anchor remains retained");
        let indexed_query = crate::core::indexed_previous_relation_query_from_store(
            &engine.store,
            "alice",
            anchor,
            proposition,
            explicit_scope.as_deref(),
            64,
        )
        .unwrap()
        .expect("the current-head relation index is available");
        let indexed_resolution = match indexed_query.resolution {
            Some(crate::core::PreviousRelationResolution::Resolved(resolved)) => resolved,
            _ => panic!("the indexed query must retain its direct-prior resolution"),
        };
        let full_resolution = match crate::core::resolve_previous_relation_proposition(
            &current_memory,
            anchor,
            proposition,
            explicit_scope.as_deref(),
        ) {
            crate::core::PreviousRelationResolution::Resolved(resolved) => resolved,
            _ => panic!("the full-snapshot query must retain its direct-prior resolution"),
        };
        assert_eq!(indexed_resolution, full_resolution);
        let indexed_memory = indexed_query.memory;
        assert_eq!(indexed_memory.revision, current_memory.revision);
        assert!(indexed_memory.facts.len() < current_memory.facts.len());
        let indexed_answer = indexed_query
            .answer
            .unwrap()
            .expect("the indexed prior relation remains supported or contradicted");
        let full_answer =
            crate::relations::query(&current_memory, &full_resolution, 10_000).unwrap();
        assert_eq!(indexed_answer.status, full_answer.status);
        assert_eq!(
            crate::relations::answer_dependencies(&indexed_answer),
            crate::relations::answer_dependencies(&full_answer)
        );
        assert_eq!(
            crate::relations::answer_guarantee(&indexed_answer),
            crate::relations::answer_guarantee(&full_answer)
        );
        assert_eq!(
            crate::core::previous_relation_success_units(
                &indexed_memory,
                &indexed_resolution,
                &indexed_answer,
                *explain,
            ),
            crate::core::previous_relation_success_units(
                &current_memory,
                &full_resolution,
                &full_answer,
                *explain,
            )
        );
        assert!(inspect(&engine.store, "alice", &retained, &retained_result).is_ok());
        assert_eq!(fs::read(&fixture.target).unwrap(), original_bytes);

        let invalidation = engine
            .execute(
                "alice",
                RequestEnvelope::new(
                    "invalidation-case",
                    "invalidate-prior-relation-premise",
                    "alice",
                    &format!("{premise_id}を無効化して"),
                ),
            )
            .unwrap();
        assert_eq!(
            invalidation.lifecycle,
            Lifecycle::Commit,
            "{invalidation:?}"
        );
        let error = inspect(&engine.store, "alice", &retained, &retained_result).unwrap_err();
        assert!(error.to_string().contains("RECOVERY_CONTEXT_PROOF_CHANGED"));
        assert_eq!(fs::read(&fixture.target).unwrap(), original_bytes);
    }

    #[test]
    fn saved_previous_relation_explanation_rejects_missing_or_misbound_anchor() {
        let fixture = empty_fixture();
        let mut engine = Engine::open(&fixture.state).unwrap();
        let premise = engine
            .execute(
                "alice",
                RequestEnvelope::new("case", "premise", "alice", "ココは鳥です"),
            )
            .unwrap();
        assert_eq!(premise.lifecycle, Lifecycle::Commit, "{premise:?}");

        let explicit_answer = engine
            .execute(
                "alice",
                RequestEnvelope::new("case", "explicit-query", "alice", "ココは鳥ですか"),
            )
            .unwrap();
        assert_eq!(
            explicit_answer.lifecycle,
            Lifecycle::Commit,
            "{explicit_answer:?}"
        );

        let prior_relation = engine
            .execute(
                "alice",
                RequestEnvelope::new("case", "prior-relation-why", "alice", "なぜそれは鳥ですか"),
            )
            .unwrap();
        assert_eq!(
            prior_relation.lifecycle,
            Lifecycle::Commit,
            "{prior_relation:?}"
        );
        let (_, prior_bundle) = engine
            .store
            .get_input_bundle(
                "alice",
                "case",
                &prior_relation.input_id,
                prior_relation.input_version,
            )
            .unwrap()
            .expect("the typed AskPreviousRelation input is retained");
        let anchor_id = prior_bundle
            .meaning
            .context_result_ref
            .clone()
            .expect("the saved AskPreviousRelation retains its direct-result anchor");
        assert_eq!(anchor_id, explicit_answer.id);

        let mut explanation_request =
            RequestEnvelope::new("case", "explanation", "alice", "詳しく説明して");
        explanation_request.output_path = Some(fixture.target.clone());
        explanation_request
            .permissions
            .write_paths
            .insert(fixture.target.clone());
        let explanation = engine.execute("alice", explanation_request).unwrap();
        assert_eq!(explanation.lifecycle, Lifecycle::Commit, "{explanation:?}");
        assert_eq!(explanation.text, prior_relation.text);

        drop(engine);
        let engine = Engine::open(&fixture.state).unwrap();
        let retained = engine.latest("alice", "case").unwrap().unwrap();
        let retained_result = retained.result.as_ref().unwrap().clone();
        let original_bytes = fs::read(&fixture.target).unwrap();
        assert!(inspect(&engine.store, "alice", &retained, &retained_result).is_ok());

        let mut missing_anchor = retained.clone();
        assert!(missing_anchor.prior_results.remove(&anchor_id).is_some());
        let error = inspect(&engine.store, "alice", &missing_anchor, &retained_result).unwrap_err();
        assert!(error.to_string().contains("RECOVERY_CONTEXT_PROOF_CHANGED"));
        assert!(error.to_string().contains("no longer retained"));
        assert_eq!(fs::read(&fixture.target).unwrap(), original_bytes);

        let mut misbound_anchor = retained.clone();
        let anchor = misbound_anchor
            .prior_results
            .get_mut(&anchor_id)
            .expect("the persisted case retains the original anchor");
        let original_anchor_id = anchor.id.clone();
        anchor.id = format!("{original_anchor_id}-mismatched");
        let error =
            inspect(&engine.store, "alice", &misbound_anchor, &retained_result).unwrap_err();
        assert!(error.to_string().contains("RECOVERY_CONTEXT_PROOF_CHANGED"));
        assert!(error.to_string().contains("not the exact committed result"));
        assert_eq!(fs::read(&fixture.target).unwrap(), original_bytes);
    }

    #[test]
    fn saved_previous_relation_explanation_rejects_uncommitted_anchor() {
        let fixture = empty_fixture();
        let mut engine = Engine::open(&fixture.state).unwrap();
        let premise = engine
            .execute(
                "alice",
                RequestEnvelope::new("case", "premise", "alice", "ココは鳥です"),
            )
            .unwrap();
        assert_eq!(premise.lifecycle, Lifecycle::Commit, "{premise:?}");

        let explicit_answer = engine
            .execute(
                "alice",
                RequestEnvelope::new("case", "explicit-query", "alice", "ココは鳥ですか"),
            )
            .unwrap();
        assert_eq!(
            explicit_answer.lifecycle,
            Lifecycle::Commit,
            "{explicit_answer:?}"
        );

        let prior_relation = engine
            .execute(
                "alice",
                RequestEnvelope::new("case", "prior-relation-why", "alice", "なぜそれは鳥ですか"),
            )
            .unwrap();
        assert_eq!(
            prior_relation.lifecycle,
            Lifecycle::Commit,
            "{prior_relation:?}"
        );
        let (_, prior_bundle) = engine
            .store
            .get_input_bundle(
                "alice",
                "case",
                &prior_relation.input_id,
                prior_relation.input_version,
            )
            .unwrap()
            .expect("the typed AskPreviousRelation input is retained");
        let anchor_id = prior_bundle
            .meaning
            .context_result_ref
            .clone()
            .expect("the saved AskPreviousRelation retains its direct-result anchor");
        assert_eq!(anchor_id, explicit_answer.id);

        let mut explanation_request =
            RequestEnvelope::new("case", "explanation", "alice", "詳しく説明して");
        explanation_request.output_path = Some(fixture.target.clone());
        explanation_request
            .permissions
            .write_paths
            .insert(fixture.target.clone());
        let explanation = engine.execute("alice", explanation_request).unwrap();
        assert_eq!(explanation.lifecycle, Lifecycle::Commit, "{explanation:?}");
        assert_eq!(explanation.text, prior_relation.text);

        drop(engine);
        let engine = Engine::open(&fixture.state).unwrap();
        let retained = engine.latest("alice", "case").unwrap().unwrap();
        let retained_result = retained.result.as_ref().unwrap().clone();
        let original_bytes = fs::read(&fixture.target).unwrap();
        assert!(inspect(&engine.store, "alice", &retained, &retained_result).is_ok());

        let mut uncommitted_anchor = retained.clone();
        let anchor = uncommitted_anchor
            .prior_results
            .get_mut(&anchor_id)
            .expect("the persisted case retains the original anchor");
        assert_eq!(anchor.id, anchor_id);
        anchor.lifecycle = Lifecycle::Received;
        let error = inspect(
            &engine.store,
            "alice",
            &uncommitted_anchor,
            &retained_result,
        )
        .unwrap_err();
        assert!(error.to_string().contains("RECOVERY_CONTEXT_PROOF_CHANGED"));
        assert!(error.to_string().contains("not the exact committed result"));
        assert_eq!(fs::read(&fixture.target).unwrap(), original_bytes);
    }

    #[test]
    fn saved_previous_relation_explanation_rejects_unbound_anchor_input() {
        let fixture = empty_fixture();
        let mut engine = Engine::open(&fixture.state).unwrap();
        let premise = engine
            .execute(
                "alice",
                RequestEnvelope::new("case", "premise", "alice", "ココは鳥です"),
            )
            .unwrap();
        assert_eq!(premise.lifecycle, Lifecycle::Commit, "{premise:?}");

        let explicit_answer = engine
            .execute(
                "alice",
                RequestEnvelope::new("case", "explicit-query", "alice", "ココは鳥ですか"),
            )
            .unwrap();
        assert_eq!(
            explicit_answer.lifecycle,
            Lifecycle::Commit,
            "{explicit_answer:?}"
        );

        let prior_relation = engine
            .execute(
                "alice",
                RequestEnvelope::new("case", "prior-relation-why", "alice", "なぜそれは鳥ですか"),
            )
            .unwrap();
        assert_eq!(
            prior_relation.lifecycle,
            Lifecycle::Commit,
            "{prior_relation:?}"
        );
        let (_, prior_bundle) = engine
            .store
            .get_input_bundle(
                "alice",
                "case",
                &prior_relation.input_id,
                prior_relation.input_version,
            )
            .unwrap()
            .expect("the typed AskPreviousRelation input is retained");
        let anchor_id = prior_bundle
            .meaning
            .context_result_ref
            .clone()
            .expect("the saved AskPreviousRelation retains its direct-result anchor");
        assert_eq!(anchor_id, explicit_answer.id);

        let mut explanation_request =
            RequestEnvelope::new("case", "explanation", "alice", "詳しく説明して");
        explanation_request.output_path = Some(fixture.target.clone());
        explanation_request
            .permissions
            .write_paths
            .insert(fixture.target.clone());
        let explanation = engine.execute("alice", explanation_request).unwrap();
        assert_eq!(explanation.lifecycle, Lifecycle::Commit, "{explanation:?}");
        assert_eq!(explanation.text, prior_relation.text);

        drop(engine);
        let engine = Engine::open(&fixture.state).unwrap();
        let retained = engine.latest("alice", "case").unwrap().unwrap();
        let retained_result = retained.result.as_ref().unwrap().clone();
        let original_bytes = fs::read(&fixture.target).unwrap();
        assert!(inspect(&engine.store, "alice", &retained, &retained_result).is_ok());

        let mut unbound_anchor = retained.clone();
        let anchor = unbound_anchor
            .prior_results
            .get_mut(&anchor_id)
            .expect("the persisted case retains the original anchor");
        assert_eq!(anchor.id, anchor_id);
        assert_eq!(anchor.lifecycle, Lifecycle::Commit);
        anchor.input_id = format!("{}-unbound", anchor.input_id);
        let error = inspect(&engine.store, "alice", &unbound_anchor, &retained_result).unwrap_err();
        assert!(error.to_string().contains("RECOVERY_CONTEXT_PROOF_CHANGED"));
        assert!(
            error
                .to_string()
                .contains("not bound to its sealed input identity")
        );
        assert_eq!(engine.latest("alice", "case").unwrap().unwrap(), retained);
        assert_eq!(fs::read(&fixture.target).unwrap(), original_bytes);
    }

    #[test]
    fn saved_previous_relation_explanation_rejects_wrong_anchor_input_version() {
        let fixture = empty_fixture();
        let mut engine = Engine::open(&fixture.state).unwrap();
        let premise = engine
            .execute(
                "alice",
                RequestEnvelope::new("case", "premise", "alice", "ココは鳥です"),
            )
            .unwrap();
        assert_eq!(premise.lifecycle, Lifecycle::Commit, "{premise:?}");

        let explicit_answer_v1 = engine
            .execute(
                "alice",
                RequestEnvelope::new("case", "explicit-query", "alice", "ココは鳥ですか"),
            )
            .unwrap();
        assert_eq!(explicit_answer_v1.lifecycle, Lifecycle::Commit);
        assert_eq!(explicit_answer_v1.input_version, 1);

        let mut explicit_query_v2 =
            RequestEnvelope::new("case", "explicit-query", "alice", "ココは鳥ですか");
        explicit_query_v2.input_version = 2;
        let explicit_answer_v2 = engine.execute("alice", explicit_query_v2).unwrap();
        assert_eq!(
            explicit_answer_v2.lifecycle,
            Lifecycle::Commit,
            "{explicit_answer_v2:?}"
        );
        assert_eq!(explicit_answer_v2.input_id, explicit_answer_v1.input_id);
        assert_eq!(explicit_answer_v2.input_version, 2);
        assert_ne!(explicit_answer_v2.id, explicit_answer_v1.id);

        let prior_relation = engine
            .execute(
                "alice",
                RequestEnvelope::new("case", "prior-relation-why", "alice", "なぜそれは鳥ですか"),
            )
            .unwrap();
        assert_eq!(
            prior_relation.lifecycle,
            Lifecycle::Commit,
            "{prior_relation:?}"
        );
        let (_, prior_bundle) = engine
            .store
            .get_input_bundle(
                "alice",
                "case",
                &prior_relation.input_id,
                prior_relation.input_version,
            )
            .unwrap()
            .expect("the typed AskPreviousRelation input is retained");
        let anchor_id = prior_bundle
            .meaning
            .context_result_ref
            .clone()
            .expect("the saved AskPreviousRelation retains its direct-result anchor");
        assert_eq!(anchor_id, explicit_answer_v2.id);

        let mut explanation_request =
            RequestEnvelope::new("case", "explanation", "alice", "詳しく説明して");
        explanation_request.output_path = Some(fixture.target.clone());
        explanation_request
            .permissions
            .write_paths
            .insert(fixture.target.clone());
        let explanation = engine.execute("alice", explanation_request).unwrap();
        assert_eq!(explanation.lifecycle, Lifecycle::Commit, "{explanation:?}");
        assert_eq!(explanation.text, prior_relation.text);

        drop(engine);
        let engine = Engine::open(&fixture.state).unwrap();
        let retained = engine.latest("alice", "case").unwrap().unwrap();
        let retained_result = retained.result.as_ref().unwrap().clone();
        let original_bytes = fs::read(&fixture.target).unwrap();
        assert!(inspect(&engine.store, "alice", &retained, &retained_result).is_ok());

        let mut wrong_version_anchor = retained.clone();
        let anchor = wrong_version_anchor
            .prior_results
            .get_mut(&anchor_id)
            .expect("the persisted case retains the direct result anchor");
        assert_eq!(anchor.id, anchor_id);
        assert_eq!(anchor.lifecycle, Lifecycle::Commit);
        assert_eq!(anchor.input_id, explicit_answer_v2.input_id);
        assert_eq!(anchor.input_version, explicit_answer_v2.input_version);
        anchor.input_version = explicit_answer_v1.input_version;
        assert_ne!(anchor.input_version, explicit_answer_v2.input_version);

        let error = inspect(
            &engine.store,
            "alice",
            &wrong_version_anchor,
            &retained_result,
        )
        .unwrap_err();
        assert!(error.to_string().contains("RECOVERY_CONTEXT_PROOF_CHANGED"));
        assert!(
            error
                .to_string()
                .contains("not bound to its sealed input identity")
        );
        assert_eq!(engine.latest("alice", "case").unwrap().unwrap(), retained);
        assert_eq!(fs::read(&fixture.target).unwrap(), original_bytes);
    }

    #[test]
    fn saved_previous_relation_explanation_rejects_anchor_bound_to_other_input_id() {
        let fixture = empty_fixture();
        let mut engine = Engine::open(&fixture.state).unwrap();
        let premise = engine
            .execute(
                "alice",
                RequestEnvelope::new("case", "premise", "alice", "ココは鳥です"),
            )
            .unwrap();
        assert_eq!(premise.lifecycle, Lifecycle::Commit, "{premise:?}");

        let mut alternate_request =
            RequestEnvelope::new("case", "alternate-query", "alice", "ココは鳥ですか");
        alternate_request.input_version = 2;
        let alternate_answer = engine.execute("alice", alternate_request).unwrap();
        assert_eq!(
            alternate_answer.lifecycle,
            Lifecycle::Commit,
            "{alternate_answer:?}"
        );
        assert_eq!(alternate_answer.input_id, "alternate-query");
        assert_eq!(alternate_answer.input_version, 2);

        let explicit_answer_v1 = engine
            .execute(
                "alice",
                RequestEnvelope::new("case", "explicit-query", "alice", "ココは鳥ですか"),
            )
            .unwrap();
        assert_eq!(explicit_answer_v1.lifecycle, Lifecycle::Commit);
        assert_eq!(explicit_answer_v1.input_version, 1);

        let mut explicit_query_v2 =
            RequestEnvelope::new("case", "explicit-query", "alice", "ココは鳥ですか");
        explicit_query_v2.input_version = 2;
        let explicit_answer_v2 = engine.execute("alice", explicit_query_v2).unwrap();
        assert_eq!(explicit_answer_v2.lifecycle, Lifecycle::Commit);
        assert_eq!(explicit_answer_v2.input_version, 2);
        assert_ne!(alternate_answer.id, explicit_answer_v2.id);

        let prior_relation = engine
            .execute(
                "alice",
                RequestEnvelope::new("case", "prior-relation-why", "alice", "なぜそれは鳥ですか"),
            )
            .unwrap();
        assert_eq!(
            prior_relation.lifecycle,
            Lifecycle::Commit,
            "{prior_relation:?}"
        );
        let (_, prior_bundle) = engine
            .store
            .get_input_bundle(
                "alice",
                "case",
                &prior_relation.input_id,
                prior_relation.input_version,
            )
            .unwrap()
            .expect("the typed AskPreviousRelation input is retained");
        let anchor_id = prior_bundle
            .meaning
            .context_result_ref
            .clone()
            .expect("the saved AskPreviousRelation retains its direct-result anchor");
        assert_eq!(anchor_id, explicit_answer_v2.id);

        let mut explanation_request =
            RequestEnvelope::new("case", "explanation", "alice", "詳しく説明して");
        explanation_request.output_path = Some(fixture.target.clone());
        explanation_request
            .permissions
            .write_paths
            .insert(fixture.target.clone());
        let explanation = engine.execute("alice", explanation_request).unwrap();
        assert_eq!(explanation.lifecycle, Lifecycle::Commit, "{explanation:?}");
        assert_eq!(explanation.text, prior_relation.text);

        drop(engine);
        let engine = Engine::open(&fixture.state).unwrap();
        let retained = engine.latest("alice", "case").unwrap().unwrap();
        let retained_result = retained.result.as_ref().unwrap().clone();
        let original_bytes = fs::read(&fixture.target).unwrap();
        assert!(inspect(&engine.store, "alice", &retained, &retained_result).is_ok());

        let mut wrong_input_id_anchor = retained.clone();
        let anchor = wrong_input_id_anchor
            .prior_results
            .get_mut(&anchor_id)
            .expect("the persisted case retains the direct result anchor");
        assert_eq!(anchor.id, anchor_id);
        assert_eq!(anchor.lifecycle, Lifecycle::Commit);
        assert_eq!(anchor.input_id, explicit_answer_v2.input_id);
        assert_eq!(anchor.input_version, explicit_answer_v2.input_version);
        anchor.input_id = alternate_answer.input_id.clone();
        assert_eq!(anchor.input_version, alternate_answer.input_version);

        let (_, alternate_bundle) = engine
            .store
            .get_input_bundle("alice", "case", &anchor.input_id, anchor.input_version)
            .unwrap()
            .expect("the alternate sealed input exists at the same version");
        assert_ne!(identity(&alternate_bundle.request).unwrap(), anchor.id);

        let error = inspect(
            &engine.store,
            "alice",
            &wrong_input_id_anchor,
            &retained_result,
        )
        .unwrap_err();
        assert!(error.to_string().contains("RECOVERY_CONTEXT_PROOF_CHANGED"));
        assert!(
            error
                .to_string()
                .contains("not bound to its sealed input identity")
        );
        assert_eq!(engine.latest("alice", "case").unwrap().unwrap(), retained);
        assert_eq!(fs::read(&fixture.target).unwrap(), original_bytes);
    }

    #[test]
    fn saved_previous_relation_explanation_rejects_missing_previous_result_input() {
        let fixture = empty_fixture();
        let mut engine = Engine::open(&fixture.state).unwrap();
        let premise = engine
            .execute(
                "alice",
                RequestEnvelope::new("case", "premise", "alice", "ココは鳥です"),
            )
            .unwrap();
        assert_eq!(premise.lifecycle, Lifecycle::Commit, "{premise:?}");

        let explicit_answer = engine
            .execute(
                "alice",
                RequestEnvelope::new("case", "explicit-query", "alice", "ココは鳥ですか"),
            )
            .unwrap();
        assert_eq!(
            explicit_answer.lifecycle,
            Lifecycle::Commit,
            "{explicit_answer:?}"
        );

        let prior_relation = engine
            .execute(
                "alice",
                RequestEnvelope::new("case", "prior-relation-why", "alice", "なぜそれは鳥ですか"),
            )
            .unwrap();
        assert_eq!(
            prior_relation.lifecycle,
            Lifecycle::Commit,
            "{prior_relation:?}"
        );
        assert!(prior_relation.actor_memory_revision.is_some());

        let mut explanation_request =
            RequestEnvelope::new("case", "explanation", "alice", "詳しく説明して");
        explanation_request.output_path = Some(fixture.target.clone());
        explanation_request
            .permissions
            .write_paths
            .insert(fixture.target.clone());
        let explanation = engine.execute("alice", explanation_request).unwrap();
        assert_eq!(explanation.lifecycle, Lifecycle::Commit, "{explanation:?}");
        assert_eq!(explanation.text, prior_relation.text);

        drop(engine);
        let engine = Engine::open(&fixture.state).unwrap();
        let retained = engine.latest("alice", "case").unwrap().unwrap();
        let retained_result = retained.result.as_ref().unwrap().clone();
        let original_bytes = fs::read(&fixture.target).unwrap();
        assert!(inspect(&engine.store, "alice", &retained, &retained_result).is_ok());

        let mut unbound_previous_result = retained.clone();
        let previous = unbound_previous_result
            .prior_results
            .get_mut(&prior_relation.id)
            .expect("the persisted case retains the AskPreviousRelation result");
        assert_eq!(previous.id, prior_relation.id);
        assert_eq!(previous.lifecycle, Lifecycle::Commit);
        assert_eq!(previous.input_id, prior_relation.input_id);
        assert_eq!(previous.input_version, prior_relation.input_version);
        assert!(previous.actor_memory_revision.is_some());
        previous.input_id = "missing-ask-previous-relation-input".into();

        let error = inspect(
            &engine.store,
            "alice",
            &unbound_previous_result,
            &retained_result,
        )
        .unwrap_err();
        assert!(error.to_string().contains("RECOVERY_CONTEXT_PROOF_CHANGED"));
        assert!(
            error
                .to_string()
                .contains("not bound to its sealed input identity")
        );
        assert_eq!(engine.latest("alice", "case").unwrap().unwrap(), retained);
        assert_eq!(fs::read(&fixture.target).unwrap(), original_bytes);
    }

    #[test]
    fn saved_previous_relation_explanation_preserves_revisionless_missing_input_fallback() {
        let fixture = empty_fixture();
        let mut engine = Engine::open(&fixture.state).unwrap();
        let premise = engine
            .execute(
                "alice",
                RequestEnvelope::new("case", "premise", "alice", "ココは鳥です"),
            )
            .unwrap();
        assert_eq!(premise.lifecycle, Lifecycle::Commit, "{premise:?}");

        let explicit_answer = engine
            .execute(
                "alice",
                RequestEnvelope::new("case", "explicit-query", "alice", "ココは鳥ですか"),
            )
            .unwrap();
        assert_eq!(
            explicit_answer.lifecycle,
            Lifecycle::Commit,
            "{explicit_answer:?}"
        );

        let prior_relation = engine
            .execute(
                "alice",
                RequestEnvelope::new("case", "prior-relation-why", "alice", "なぜそれは鳥ですか"),
            )
            .unwrap();
        assert_eq!(
            prior_relation.lifecycle,
            Lifecycle::Commit,
            "{prior_relation:?}"
        );
        assert!(prior_relation.actor_memory_revision.is_some());

        let mut explanation_request =
            RequestEnvelope::new("case", "explanation", "alice", "詳しく説明して");
        explanation_request.output_path = Some(fixture.target.clone());
        explanation_request
            .permissions
            .write_paths
            .insert(fixture.target.clone());
        let explanation = engine.execute("alice", explanation_request).unwrap();
        assert_eq!(explanation.lifecycle, Lifecycle::Commit, "{explanation:?}");
        assert_eq!(explanation.text, prior_relation.text);

        drop(engine);
        let engine = Engine::open(&fixture.state).unwrap();
        let retained = engine.latest("alice", "case").unwrap().unwrap();
        let retained_result = retained.result.as_ref().unwrap().clone();
        let original_bytes = fs::read(&fixture.target).unwrap();
        assert!(inspect(&engine.store, "alice", &retained, &retained_result).is_ok());

        let mut revisionless_previous_result = retained.clone();
        let previous = revisionless_previous_result
            .prior_results
            .get_mut(&prior_relation.id)
            .expect("the persisted case retains the AskPreviousRelation result");
        assert_eq!(previous.id, prior_relation.id);
        assert_eq!(previous.lifecycle, Lifecycle::Commit);
        assert_eq!(previous.input_version, prior_relation.input_version);
        assert!(previous.actor_memory_revision.is_some());
        previous.actor_memory_revision = None;
        previous.input_id = "missing-legacy-ask-previous-relation-input".into();

        assert!(
            inspect(
                &engine.store,
                "alice",
                &revisionless_previous_result,
                &retained_result,
            )
            .is_ok()
        );
        assert_eq!(engine.latest("alice", "case").unwrap().unwrap(), retained);
        assert_eq!(fs::read(&fixture.target).unwrap(), original_bytes);
    }

    #[test]
    fn saved_previous_relation_explanation_rejects_revisionless_result_bound_to_other_input() {
        let fixture = empty_fixture();
        let mut engine = Engine::open(&fixture.state).unwrap();
        let premise = engine
            .execute(
                "alice",
                RequestEnvelope::new("case", "premise", "alice", "ココは鳥です"),
            )
            .unwrap();
        assert_eq!(premise.lifecycle, Lifecycle::Commit, "{premise:?}");

        let explicit_answer = engine
            .execute(
                "alice",
                RequestEnvelope::new("case", "explicit-query", "alice", "ココは鳥ですか"),
            )
            .unwrap();
        assert_eq!(
            explicit_answer.lifecycle,
            Lifecycle::Commit,
            "{explicit_answer:?}"
        );

        let prior_relation = engine
            .execute(
                "alice",
                RequestEnvelope::new("case", "prior-relation-why", "alice", "なぜそれは鳥ですか"),
            )
            .unwrap();
        assert_eq!(
            prior_relation.lifecycle,
            Lifecycle::Commit,
            "{prior_relation:?}"
        );
        assert!(prior_relation.actor_memory_revision.is_some());
        assert_eq!(explicit_answer.input_version, prior_relation.input_version);

        let mut explanation_request =
            RequestEnvelope::new("case", "explanation", "alice", "詳しく説明して");
        explanation_request.output_path = Some(fixture.target.clone());
        explanation_request
            .permissions
            .write_paths
            .insert(fixture.target.clone());
        let explanation = engine.execute("alice", explanation_request).unwrap();
        assert_eq!(explanation.lifecycle, Lifecycle::Commit, "{explanation:?}");
        assert_eq!(explanation.text, prior_relation.text);

        drop(engine);
        let engine = Engine::open(&fixture.state).unwrap();
        let retained = engine.latest("alice", "case").unwrap().unwrap();
        let retained_result = retained.result.as_ref().unwrap().clone();
        let original_bytes = fs::read(&fixture.target).unwrap();
        assert!(inspect(&engine.store, "alice", &retained, &retained_result).is_ok());

        let mut misbound_revisionless_result = retained.clone();
        let previous = misbound_revisionless_result
            .prior_results
            .get_mut(&prior_relation.id)
            .expect("the persisted case retains the AskPreviousRelation result");
        assert_eq!(previous.id, prior_relation.id);
        assert_eq!(previous.lifecycle, Lifecycle::Commit);
        assert_eq!(previous.input_version, prior_relation.input_version);
        assert_eq!(previous.input_id, prior_relation.input_id);
        assert!(previous.actor_memory_revision.is_some());
        previous.actor_memory_revision = None;
        previous.input_id = explicit_answer.input_id.clone();

        let (_, misbound_bundle) = engine
            .store
            .get_input_bundle("alice", "case", &previous.input_id, previous.input_version)
            .unwrap()
            .expect("the other sealed input exists at the same version");
        assert_ne!(identity(&misbound_bundle.request).unwrap(), previous.id);

        let error = inspect(
            &engine.store,
            "alice",
            &misbound_revisionless_result,
            &retained_result,
        )
        .unwrap_err();
        assert!(error.to_string().contains("RECOVERY_CONTEXT_PROOF_CHANGED"));
        assert!(
            error
                .to_string()
                .contains("not bound to its sealed input identity")
        );
        assert_eq!(engine.latest("alice", "case").unwrap().unwrap(), retained);
        assert_eq!(fs::read(&fixture.target).unwrap(), original_bytes);
    }

    #[test]
    fn saved_previous_relation_explanation_rejects_revisionless_result_bound_to_other_input_version()
     {
        let fixture = empty_fixture();
        let mut engine = Engine::open(&fixture.state).unwrap();
        let premise = engine
            .execute(
                "alice",
                RequestEnvelope::new("case", "premise", "alice", "ココは鳥です"),
            )
            .unwrap();
        assert_eq!(premise.lifecycle, Lifecycle::Commit, "{premise:?}");

        let prior_relation_v1 = engine
            .execute(
                "alice",
                RequestEnvelope::new("case", "prior-relation-why", "alice", "ココは鳥ですか"),
            )
            .unwrap();
        assert_eq!(prior_relation_v1.lifecycle, Lifecycle::Commit);
        assert_eq!(prior_relation_v1.input_version, 1);

        let mut prior_relation_request_v2 =
            RequestEnvelope::new("case", "prior-relation-why", "alice", "なぜそれは鳥ですか");
        prior_relation_request_v2.input_version = 2;
        let prior_relation_v2 = engine.execute("alice", prior_relation_request_v2).unwrap();
        assert_eq!(
            prior_relation_v2.lifecycle,
            Lifecycle::Commit,
            "{prior_relation_v2:?}"
        );
        assert_eq!(prior_relation_v2.input_id, prior_relation_v1.input_id);
        assert_eq!(prior_relation_v2.input_version, 2);
        assert_ne!(prior_relation_v2.id, prior_relation_v1.id);
        assert!(prior_relation_v2.actor_memory_revision.is_some());

        let mut explanation_request =
            RequestEnvelope::new("case", "explanation", "alice", "詳しく説明して");
        explanation_request.output_path = Some(fixture.target.clone());
        explanation_request
            .permissions
            .write_paths
            .insert(fixture.target.clone());
        let explanation = engine.execute("alice", explanation_request).unwrap();
        assert_eq!(explanation.lifecycle, Lifecycle::Commit, "{explanation:?}");
        assert_eq!(explanation.text, prior_relation_v2.text);

        drop(engine);
        let engine = Engine::open(&fixture.state).unwrap();
        let retained = engine.latest("alice", "case").unwrap().unwrap();
        let retained_result = retained.result.as_ref().unwrap().clone();
        let original_bytes = fs::read(&fixture.target).unwrap();
        assert!(inspect(&engine.store, "alice", &retained, &retained_result).is_ok());

        let mut revisionless_wrong_version = retained.clone();
        let previous = revisionless_wrong_version
            .prior_results
            .get_mut(&prior_relation_v2.id)
            .expect("the persisted case retains the version 2 AskPreviousRelation result");
        assert_eq!(previous.id, prior_relation_v2.id);
        assert_eq!(previous.lifecycle, Lifecycle::Commit);
        assert_eq!(previous.input_id, prior_relation_v2.input_id);
        assert_eq!(previous.input_version, prior_relation_v2.input_version);
        assert!(previous.actor_memory_revision.is_some());
        previous.actor_memory_revision = None;
        previous.input_version = prior_relation_v1.input_version;

        let (_, wrong_version_bundle) = engine
            .store
            .get_input_bundle("alice", "case", &previous.input_id, previous.input_version)
            .unwrap()
            .expect("the same input ID has an existing sealed version 1 bundle");
        assert_ne!(
            identity(&wrong_version_bundle.request).unwrap(),
            previous.id
        );

        let error = inspect(
            &engine.store,
            "alice",
            &revisionless_wrong_version,
            &retained_result,
        )
        .unwrap_err();
        assert!(error.to_string().contains("RECOVERY_CONTEXT_PROOF_CHANGED"));
        assert!(
            error
                .to_string()
                .contains("not bound to its sealed input identity")
        );
        assert_eq!(engine.latest("alice", "case").unwrap().unwrap(), retained);
        assert_eq!(fs::read(&fixture.target).unwrap(), original_bytes);
    }

    #[test]
    fn saved_previous_relation_explanation_rejects_revisionless_exact_input_without_memory_revision()
     {
        let fixture = empty_fixture();
        let mut engine = Engine::open(&fixture.state).unwrap();
        let premise = engine
            .execute(
                "alice",
                RequestEnvelope::new("case", "premise", "alice", "ココは鳥です"),
            )
            .unwrap();
        assert_eq!(premise.lifecycle, Lifecycle::Commit, "{premise:?}");

        let explicit_answer = engine
            .execute(
                "alice",
                RequestEnvelope::new("case", "explicit-query", "alice", "ココは鳥ですか"),
            )
            .unwrap();
        assert_eq!(
            explicit_answer.lifecycle,
            Lifecycle::Commit,
            "{explicit_answer:?}"
        );

        let prior_relation = engine
            .execute(
                "alice",
                RequestEnvelope::new("case", "prior-relation-why", "alice", "なぜそれは鳥ですか"),
            )
            .unwrap();
        assert_eq!(
            prior_relation.lifecycle,
            Lifecycle::Commit,
            "{prior_relation:?}"
        );
        assert!(prior_relation.actor_memory_revision.is_some());

        let mut explanation_request =
            RequestEnvelope::new("case", "explanation", "alice", "詳しく説明して");
        explanation_request.output_path = Some(fixture.target.clone());
        explanation_request
            .permissions
            .write_paths
            .insert(fixture.target.clone());
        let explanation = engine.execute("alice", explanation_request).unwrap();
        assert_eq!(explanation.lifecycle, Lifecycle::Commit, "{explanation:?}");
        assert_eq!(explanation.text, prior_relation.text);

        drop(engine);
        let engine = Engine::open(&fixture.state).unwrap();
        let retained = engine.latest("alice", "case").unwrap().unwrap();
        let retained_result = retained.result.as_ref().unwrap().clone();
        let original_bytes = fs::read(&fixture.target).unwrap();
        assert!(inspect(&engine.store, "alice", &retained, &retained_result).is_ok());

        let mut revisionless_exact_input = retained.clone();
        let previous = revisionless_exact_input
            .prior_results
            .get_mut(&prior_relation.id)
            .expect("the persisted case retains the AskPreviousRelation result");
        assert_eq!(previous.id, prior_relation.id);
        assert_eq!(previous.lifecycle, Lifecycle::Commit);
        assert_eq!(previous.input_id, prior_relation.input_id);
        assert_eq!(previous.input_version, prior_relation.input_version);
        assert!(previous.actor_memory_revision.is_some());
        previous.actor_memory_revision = None;

        let (_, exact_bundle) = engine
            .store
            .get_input_bundle("alice", "case", &previous.input_id, previous.input_version)
            .unwrap()
            .expect("the exact sealed prior input remains available");
        assert_eq!(identity(&exact_bundle.request).unwrap(), previous.id);

        let error = inspect(
            &engine.store,
            "alice",
            &revisionless_exact_input,
            &retained_result,
        )
        .unwrap_err();
        assert!(error.to_string().contains("RECOVERY_CONTEXT_PROOF_CHANGED"));
        assert!(
            error
                .to_string()
                .contains("RECOVERY_MEMORY_REVISION_MISSING")
        );
        assert!(
            error
                .to_string()
                .contains("no retained actor-memory revision")
        );
        assert_eq!(engine.latest("alice", "case").unwrap().unwrap(), retained);
        assert_eq!(fs::read(&fixture.target).unwrap(), original_bytes);
    }

    #[test]
    fn saved_previous_relation_explanation_rejects_revision_pointer_to_invalidated_snapshot() {
        let fixture = empty_fixture();
        let mut engine = Engine::open(&fixture.state).unwrap();
        let premise = engine
            .execute(
                "alice",
                RequestEnvelope::new("case", "premise", "alice", "ココは鳥です"),
            )
            .unwrap();
        assert_eq!(premise.lifecycle, Lifecycle::Commit, "{premise:?}");
        let premise_id = premise.dependencies[0].clone();

        let explicit_answer = engine
            .execute(
                "alice",
                RequestEnvelope::new("case", "explicit-query", "alice", "ココは鳥ですか"),
            )
            .unwrap();
        assert_eq!(
            explicit_answer.lifecycle,
            Lifecycle::Commit,
            "{explicit_answer:?}"
        );

        let mut prior_relation_request =
            RequestEnvelope::new("case", "prior-relation-why", "alice", "なぜそれは鳥ですか");
        prior_relation_request.budget.max_actions = 64;
        let prior_relation = engine.execute("alice", prior_relation_request).unwrap();
        assert_eq!(
            prior_relation.lifecycle,
            Lifecycle::Commit,
            "{prior_relation:?}"
        );
        assert!(prior_relation.dependencies.contains(&premise_id));
        let pinned_revision = prior_relation
            .actor_memory_revision
            .expect("the prior relation result records its actor-memory revision");
        let pinned_memory = engine
            .store
            .load_memory_revision("alice", pinned_revision)
            .unwrap()
            .expect("the saved actor-memory snapshot is retained");
        assert!(
            pinned_memory
                .facts
                .iter()
                .any(|fact| fact.id == premise_id && fact.valid)
        );

        let mut explanation_request =
            RequestEnvelope::new("case", "explanation", "alice", "詳しく説明して");
        explanation_request.budget.max_actions = 64;
        explanation_request.output_path = Some(fixture.target.clone());
        explanation_request
            .permissions
            .write_paths
            .insert(fixture.target.clone());
        let explanation = engine.execute("alice", explanation_request).unwrap();
        assert_eq!(explanation.lifecycle, Lifecycle::Commit, "{explanation:?}");
        assert_eq!(explanation.text, prior_relation.text);

        drop(engine);
        let mut engine = Engine::open(&fixture.state).unwrap();
        let retained = engine.latest("alice", "case").unwrap().unwrap();
        let retained_result = retained.result.as_ref().unwrap().clone();
        let original_bytes = fs::read(&fixture.target).unwrap();
        assert!(inspect(&engine.store, "alice", &retained, &retained_result).is_ok());

        let invalidation = engine
            .execute(
                "alice",
                RequestEnvelope::new(
                    "invalidation-case",
                    "invalidate-pinned-prior-premise",
                    "alice",
                    &format!("{premise_id}を無効化して"),
                ),
            )
            .unwrap();
        assert_eq!(
            invalidation.lifecycle,
            Lifecycle::Commit,
            "{invalidation:?}"
        );
        let changed_memory = engine.store.load_memory("alice").unwrap();
        assert_ne!(changed_memory.revision, pinned_revision);
        let changed_snapshot = engine
            .store
            .load_memory_revision("alice", changed_memory.revision)
            .unwrap()
            .expect("the invalidation creates another retained snapshot");
        assert!(
            changed_snapshot
                .facts
                .iter()
                .any(|fact| fact.id == premise_id && !fact.valid)
        );

        let mut mispinned_revision = retained.clone();
        let previous = mispinned_revision
            .prior_results
            .get_mut(&prior_relation.id)
            .expect("the retained case keeps its AskPreviousRelation result");
        assert_eq!(previous.id, prior_relation.id);
        assert_eq!(previous.input_id, prior_relation.input_id);
        assert_eq!(previous.input_version, prior_relation.input_version);
        assert_eq!(previous.actor_memory_revision, Some(pinned_revision));
        previous.actor_memory_revision = Some(changed_memory.revision);

        let (_, prior_bundle) = engine
            .store
            .get_input_bundle("alice", "case", &previous.input_id, previous.input_version)
            .unwrap()
            .expect("the exact sealed input remains available");
        assert_eq!(identity(&prior_bundle.request).unwrap(), previous.id);

        let error = inspect(
            &engine.store,
            "alice",
            &mispinned_revision,
            &retained_result,
        )
        .unwrap_err();
        assert!(
            error.to_string().contains("RECOVERY_CONTEXT_PROOF_CHANGED"),
            "{error:?}"
        );
        assert!(
            error
                .to_string()
                .contains("no longer resolves to one direct premise in its original revision"),
            "{error:?}"
        );
        assert_eq!(engine.latest("alice", "case").unwrap().unwrap(), retained);
        assert_eq!(fs::read(&fixture.target).unwrap(), original_bytes);
    }

    #[test]
    fn saved_previous_relation_explanation_rejects_missing_actor_memory_snapshot_revision() {
        let fixture = empty_fixture();
        let mut engine = Engine::open(&fixture.state).unwrap();
        let premise = engine
            .execute(
                "alice",
                RequestEnvelope::new("case", "premise", "alice", "ココは鳥です"),
            )
            .unwrap();
        assert_eq!(premise.lifecycle, Lifecycle::Commit, "{premise:?}");
        let premise_id = premise.dependencies[0].clone();

        let explicit_answer = engine
            .execute(
                "alice",
                RequestEnvelope::new("case", "explicit-query", "alice", "ココは鳥ですか"),
            )
            .unwrap();
        assert_eq!(
            explicit_answer.lifecycle,
            Lifecycle::Commit,
            "{explicit_answer:?}"
        );
        assert!(explicit_answer.dependencies.contains(&premise_id));

        let mut prior_relation_request =
            RequestEnvelope::new("case", "prior-relation-why", "alice", "なぜそれは鳥ですか");
        prior_relation_request.budget.max_actions = 64;
        let prior_relation = engine.execute("alice", prior_relation_request).unwrap();
        assert_eq!(
            prior_relation.lifecycle,
            Lifecycle::Commit,
            "{prior_relation:?}"
        );
        assert!(prior_relation.dependencies.contains(&premise_id));
        let pinned_revision = prior_relation
            .actor_memory_revision
            .expect("the prior relation result records its actor-memory revision");
        assert!(
            engine
                .store
                .load_memory_revision("alice", pinned_revision)
                .unwrap()
                .is_some()
        );

        let mut explanation_request =
            RequestEnvelope::new("case", "explanation", "alice", "詳しく説明して");
        explanation_request.budget.max_actions = 64;
        explanation_request.output_path = Some(fixture.target.clone());
        explanation_request
            .permissions
            .write_paths
            .insert(fixture.target.clone());
        let explanation = engine.execute("alice", explanation_request).unwrap();
        assert_eq!(explanation.lifecycle, Lifecycle::Commit, "{explanation:?}");
        assert_eq!(explanation.text, prior_relation.text);

        drop(engine);
        let engine = Engine::open(&fixture.state).unwrap();
        let retained = engine.latest("alice", "case").unwrap().unwrap();
        let retained_result = retained.result.as_ref().unwrap().clone();
        let original_bytes = fs::read(&fixture.target).unwrap();
        assert!(inspect(&engine.store, "alice", &retained, &retained_result).is_ok());

        let current_memory = engine.store.load_memory("alice").unwrap();
        let missing_revision = current_memory
            .revision
            .checked_add(1)
            .expect("the temporary fixture revision is in range");
        assert!(
            engine
                .store
                .load_memory_revision("alice", missing_revision)
                .unwrap()
                .is_none()
        );

        let mut missing_revision_pointer = retained.clone();
        let previous = missing_revision_pointer
            .prior_results
            .get_mut(&prior_relation.id)
            .expect("the retained case keeps its AskPreviousRelation result");
        assert_eq!(previous.id, prior_relation.id);
        assert_eq!(previous.input_id, prior_relation.input_id);
        assert_eq!(previous.input_version, prior_relation.input_version);
        assert_eq!(previous.actor_memory_revision, Some(pinned_revision));
        previous.actor_memory_revision = Some(missing_revision);

        let (_, prior_bundle) = engine
            .store
            .get_input_bundle("alice", "case", &previous.input_id, previous.input_version)
            .unwrap()
            .expect("the exact sealed prior input remains available");
        assert_eq!(identity(&prior_bundle.request).unwrap(), previous.id);

        let error = inspect(
            &engine.store,
            "alice",
            &missing_revision_pointer,
            &retained_result,
        )
        .unwrap_err();
        assert!(
            error.to_string().contains("RECOVERY_CONTEXT_PROOF_CHANGED"),
            "{error:?}"
        );
        assert!(
            error
                .to_string()
                .contains("RECOVERY_MEMORY_REVISION_MISSING"),
            "{error:?}"
        );
        assert_eq!(engine.latest("alice", "case").unwrap().unwrap(), retained);
        assert_eq!(engine.store.load_memory("alice").unwrap(), current_memory);
        assert_eq!(fs::read(&fixture.target).unwrap(), original_bytes);
    }

    #[test]
    fn conjunction_explanation_rejects_missing_actor_memory_snapshot_revision() {
        let fixture = empty_fixture();
        let mut engine = Engine::open(&fixture.state).unwrap();
        for (input_id, text) in [("bird", "ココは鳥です"), ("animal", "ココは動物です")]
        {
            let premise = engine
                .execute(
                    "alice",
                    RequestEnvelope::new("case", input_id, "alice", text),
                )
                .unwrap();
            assert_eq!(premise.lifecycle, Lifecycle::Commit, "{premise:?}");
        }

        let mut conjunction_request =
            RequestEnvelope::new("case", "conjunction", "alice", "ココは鳥かつ動物ですか");
        conjunction_request.budget.max_actions = 32;
        let prior = engine.execute("alice", conjunction_request).unwrap();
        assert_eq!(prior.lifecycle, Lifecycle::Commit, "{prior:?}");
        let pinned_revision = prior
            .actor_memory_revision
            .expect("the conjunction records the actor-memory revision it used");
        assert!(
            engine
                .store
                .load_memory_revision("alice", pinned_revision)
                .unwrap()
                .is_some()
        );

        let mut explanation_request =
            RequestEnvelope::new("case", "conjunction-explanation", "alice", "詳しく説明して");
        explanation_request.budget.max_actions = 32;
        explanation_request.output_path = Some(fixture.target.clone());
        explanation_request
            .permissions
            .write_paths
            .insert(fixture.target.clone());
        let explanation = engine.execute("alice", explanation_request).unwrap();
        assert_eq!(explanation.lifecycle, Lifecycle::Commit, "{explanation:?}");
        assert_eq!(explanation.text, prior.text);

        drop(engine);
        let engine = Engine::open(&fixture.state).unwrap();
        let retained = engine.latest("alice", "case").unwrap().unwrap();
        let retained_result = retained.result.as_ref().unwrap().clone();
        let original_bytes = fs::read(&fixture.target).unwrap();
        assert!(inspect(&engine.store, "alice", &retained, &retained_result).is_ok());

        let current_memory = engine.store.load_memory("alice").unwrap();
        let missing_revision = current_memory
            .revision
            .checked_add(1)
            .expect("the temporary fixture revision is in range");
        assert!(
            engine
                .store
                .load_memory_revision("alice", missing_revision)
                .unwrap()
                .is_none()
        );

        let mut missing_revision_pointer = retained.clone();
        let previous = missing_revision_pointer
            .prior_results
            .get_mut(&prior.id)
            .expect("the retained case keeps the exact conjunction result");
        assert_eq!(previous.actor_memory_revision, Some(pinned_revision));
        previous.actor_memory_revision = Some(missing_revision);
        let (_, prior_bundle) = engine
            .store
            .get_input_bundle("alice", "case", &previous.input_id, previous.input_version)
            .unwrap()
            .expect("the exact sealed conjunction input remains available");
        assert_eq!(identity(&prior_bundle.request).unwrap(), previous.id);
        assert!(matches!(
            prior_bundle.meaning.intent,
            Intent::AskConjunction { .. }
        ));

        let error = inspect(
            &engine.store,
            "alice",
            &missing_revision_pointer,
            &retained_result,
        )
        .unwrap_err();
        assert!(
            error.to_string().contains("RECOVERY_CONTEXT_PROOF_CHANGED"),
            "{error:?}"
        );
        assert!(
            error
                .to_string()
                .contains("RECOVERY_MEMORY_REVISION_MISSING"),
            "{error:?}"
        );
        assert_eq!(engine.latest("alice", "case").unwrap().unwrap(), retained);
        assert_eq!(engine.store.load_memory("alice").unwrap(), current_memory);
        assert_eq!(fs::read(&fixture.target).unwrap(), original_bytes);
    }

    #[test]
    fn conjunction_explanation_rechecks_the_current_memory_revision() {
        let fixture = empty_fixture();
        let mut engine = Engine::open(&fixture.state).unwrap();
        let bird = engine
            .execute(
                "alice",
                RequestEnvelope::new("case", "bird", "alice", "ココは鳥です"),
            )
            .unwrap();
        let bird_id = bird.dependencies[0].clone();
        let animal = engine
            .execute(
                "alice",
                RequestEnvelope::new("case", "animal", "alice", "ココは動物です"),
            )
            .unwrap();
        let animal_id = animal.dependencies[0].clone();

        let mut conjunction_request =
            RequestEnvelope::new("case", "conjunction", "alice", "ココは鳥かつ動物ですか");
        conjunction_request.budget.max_actions = 32;
        let prior = engine.execute("alice", conjunction_request).unwrap();
        assert_eq!(prior.lifecycle, Lifecycle::Commit, "{prior:?}");
        assert!(prior.dependencies.contains(&bird_id));
        assert!(prior.dependencies.contains(&animal_id));
        let memory_revision = prior
            .actor_memory_revision
            .expect("the conjunction records the actor-memory revision it used");

        let mut explanation_request =
            RequestEnvelope::new("case", "conjunction-explanation", "alice", "詳しく説明して");
        explanation_request.budget.max_actions = 32;
        explanation_request.output_path = Some(fixture.target.clone());
        explanation_request
            .permissions
            .write_paths
            .insert(fixture.target.clone());
        let explanation = engine.execute("alice", explanation_request).unwrap();
        assert_eq!(explanation.lifecycle, Lifecycle::Commit, "{explanation:?}");
        assert_eq!(explanation.text, prior.text);
        assert_eq!(explanation.dependencies, vec![prior.id.clone()]);
        let retained = engine.latest("alice", "case").unwrap().unwrap();
        let retained_result = retained.result.as_ref().unwrap().clone();
        let original_bytes = fs::read(&fixture.target).unwrap();
        assert!(inspect(&engine.store, "alice", &retained, &retained_result).is_ok());

        drop(engine);
        let mut engine = Engine::open(&fixture.state).unwrap();
        assert_eq!(
            engine
                .store
                .load_memory_revision("alice", memory_revision)
                .unwrap()
                .unwrap()
                .revision,
            memory_revision
        );
        assert!(inspect(&engine.store, "alice", &retained, &retained_result).is_ok());

        let database = Connection::open(fixture.state.join("state.sqlite3")).unwrap();
        database
            .execute("DELETE FROM memory_index_state WHERE actor_id='alice'", [])
            .unwrap();
        drop(database);
        assert!(inspect(&engine.store, "alice", &retained, &retained_result).is_ok());

        let history_request = RequestEnvelope::new(
            "history-case",
            "conjunction-explanation-off-scope-history",
            "alice",
            "連言説明Recoveryの範囲外履歴",
        );
        let history_source = identity(&history_request).unwrap();
        engine
            .store
            .register_input("alice", &history_request, &history_source)
            .unwrap();
        let mut current_memory = engine.store.load_memory("alice").unwrap();
        let next_revision = current_memory.revision + 1;
        for ordinal in 0..80 {
            crate::relations::assert_fact(
                &mut current_memory,
                Proposition {
                    subject: format!("archive-subject-{ordinal}"),
                    predicate: "is".into(),
                    object: format!("archive-object-{ordinal}"),
                    negative: false,
                    scope: format!("archive-{ordinal}"),
                    condition_refs: BTreeSet::new(),
                    temporal_scope: None,
                },
                &history_source,
            )
            .unwrap();
        }
        current_memory.revision = next_revision;
        engine.store.save_memory("alice", &current_memory).unwrap();

        let goal_ref = retained.goal_sha256.as_ref().unwrap();
        let goal: GoalContract =
            serde_json::from_slice(&engine.store.get_bytes("alice", "case", goal_ref).unwrap())
                .unwrap();
        let previous_id = retained
            .bundle
            .as_ref()
            .and_then(|bundle| bundle.meaning.context_result_ref.as_deref())
            .expect("the sealed explanation retains its prior-result reference");
        let previous = retained
            .prior_results
            .get(previous_id)
            .expect("the exact conjunction result remains retained");
        verify_explain_previous_conjunction_memory_result(
            &engine.store,
            &retained,
            Some(&current_memory),
            previous,
            &retained_result,
            retained_result.content_plan.as_ref().unwrap(),
            &goal,
        )
        .unwrap();
        assert_eq!(current_memory.facts.len(), 82);

        let (_, prior_bundle) = engine
            .store
            .get_input_bundle("alice", "case", &previous.input_id, previous.input_version)
            .unwrap()
            .expect("the conjunction input remains retained");
        let Intent::AskConjunction { propositions } = &prior_bundle.meaning.intent else {
            panic!("the prior result lost its typed conjunction");
        };
        let per_relation_budget =
            conjunction_per_relation_budget(propositions, goal.budget.max_actions).unwrap();
        let (indexed_revision, indexed_outcomes) =
            crate::core::indexed_conjunction_relation_answers_from_store(
                &engine.store,
                "alice",
                propositions,
                per_relation_budget,
            )
            .unwrap()
            .expect("the current conjunction relation index is available");
        assert_eq!(indexed_revision, current_memory.revision);
        let indexed_outcomes = indexed_outcomes.unwrap();
        let full_snapshot_outcomes =
            conjunction_memory_outcomes(&current_memory, propositions, goal.budget.max_actions)
                .unwrap();
        assert_eq!(indexed_outcomes.len(), full_snapshot_outcomes.len());
        for (indexed, full) in indexed_outcomes.iter().zip(&full_snapshot_outcomes) {
            assert_eq!(indexed.status, full.status);
            assert_eq!(indexed.facts, full.facts);
            assert_eq!(indexed.used_rules, full.used_rules);
            assert_eq!(
                crate::relations::answer_dependencies(indexed),
                crate::relations::answer_dependencies(full)
            );
            assert_eq!(
                crate::relations::answer_guarantee(indexed),
                crate::relations::answer_guarantee(full)
            );
        }

        verify_explain_previous_conjunction_memory_result(
            &engine.store,
            &retained,
            None,
            previous,
            &retained_result,
            retained_result.content_plan.as_ref().unwrap(),
            &goal,
        )
        .unwrap();
        assert!(inspect(&engine.store, "alice", &retained, &retained_result).is_ok());

        let invalidation = engine
            .execute(
                "alice",
                RequestEnvelope::new(
                    "invalidation-case",
                    "invalidate-conjunction-bird",
                    "alice",
                    &format!("{bird_id}を無効化して"),
                ),
            )
            .unwrap();
        assert_eq!(invalidation.lifecycle, Lifecycle::Commit);
        let error = inspect(&engine.store, "alice", &retained, &retained_result).unwrap_err();
        assert!(
            error.to_string().contains("RECOVERY_CONTEXT_PROOF_CHANGED"),
            "{error:?}"
        );
        assert_eq!(fs::read(&fixture.target).unwrap(), original_bytes);
    }

    #[test]
    fn relation_comparison_recovery_pins_and_rechecks_its_memory_revision() {
        let fixture = empty_fixture();
        let mut engine = Engine::open(&fixture.state).unwrap();
        let coco = engine
            .execute(
                "alice",
                RequestEnvelope::new("case", "coco", "alice", "ココは鳥です"),
            )
            .unwrap();
        let coco_id = coco.dependencies[0].clone();
        let mike = engine
            .execute(
                "alice",
                RequestEnvelope::new("case", "mike", "alice", "ミケは鳥です"),
            )
            .unwrap();
        let mike_id = mike.dependencies[0].clone();

        let mut request = RequestEnvelope::new(
            "case",
            "relation-comparison",
            "alice",
            "ココとミケの保存済みの関係を段落で比較して",
        );
        request.budget.max_actions = 32;
        request.output_path = Some(fixture.target.clone());
        request
            .permissions
            .write_paths
            .insert(fixture.target.clone());
        let comparison = engine.execute("alice", request).unwrap();
        assert_eq!(comparison.lifecycle, Lifecycle::Commit, "{comparison:?}");
        assert_eq!(comparison.guarantee, Guarantee::Hypothesis);
        assert!(
            comparison
                .text
                .starts_with("保存済みの関係を左右で比較します。")
        );
        assert_eq!(comparison.text.lines().count(), 1);
        assert!(comparison.dependencies.contains(&coco_id));
        assert!(comparison.dependencies.contains(&mike_id));
        let memory_revision = comparison
            .actor_memory_revision
            .expect("Core records the actor-memory snapshot used by the comparison");
        let original_memory = engine
            .store
            .load_memory_revision("alice", memory_revision)
            .unwrap()
            .expect("the comparison's actor-memory revision is retained");
        assert!(original_memory.facts.iter().any(|fact| fact.id == coco_id));
        assert!(original_memory.facts.iter().any(|fact| fact.id == mike_id));

        let retained = engine.latest("alice", "case").unwrap().unwrap();
        let retained_result = retained.result.as_ref().unwrap().clone();
        assert_eq!(retained_result.actor_memory_revision, Some(memory_revision));
        inspect(&engine.store, "alice", &retained, &retained_result)
            .unwrap_or_else(|error| panic!("{error:?}"));
        let goal_ref = retained.goal_sha256.as_ref().unwrap();
        let goal: GoalContract =
            serde_json::from_slice(&engine.store.get_bytes("alice", "case", goal_ref).unwrap())
                .unwrap();
        let mut wrong_goal = goal.clone();
        let Intent::CompareRelationInventories { format, .. } = &mut wrong_goal.intent else {
            panic!("comparison must seal its actual goal");
        };
        assert_eq!(*format, Some(MeaningCompositionFormat::Paragraph));
        *format = Some(MeaningCompositionFormat::Bullets);
        let mut wrong_case = retained.clone();
        wrong_case.goal_sha256 = Some(
            engine
                .store
                .put_artifact("alice", "case", &wrong_goal)
                .unwrap(),
        );
        let mismatch = inspect(&engine.store, "alice", &wrong_case, &retained_result).unwrap_err();
        assert!(
            mismatch.to_string().contains("RECOVERY_GOAL_MISMATCH"),
            "{mismatch:?}"
        );

        let saved_report = engine
            .store
            .get_bytes("alice", "case", &retained_result.dependencies[0])
            .unwrap();
        let mut wrong_report: serde_json::Value = serde_json::from_slice(&saved_report).unwrap();
        wrong_report["meaning_plan"]["format"] = "BULLETS".into();
        let wrong_ref = engine
            .store
            .put_artifact("alice", "case", &wrong_report)
            .unwrap();
        let mut wrong_result = retained_result.clone();
        wrong_result.dependencies[0] = wrong_ref.clone();
        let wrong_plan = wrong_result.content_plan.as_mut().unwrap();
        wrong_plan.dependencies[0] = wrong_ref;
        wrong_plan.id.clear();
        wrong_plan.id = identity(&*wrong_plan).unwrap();
        wrong_result.render.as_mut().unwrap().plan_id = wrong_plan.id.clone();
        let mismatch = inspect(&engine.store, "alice", &retained, &wrong_result).unwrap_err();
        assert!(
            mismatch.to_string().contains("CONTENT_PLAN_INVALID"),
            "{mismatch:?}"
        );

        let original_bytes = fs::read(&fixture.target).unwrap();
        let original_modified = fs::metadata(&fixture.target).unwrap().modified().unwrap();

        // Change only the model reference and re-sign the local plan envelope.
        // The alternative is an actual model artifact from a prior operation.
        let mut swapped = retained_result.clone();
        let swapped_plan = swapped.content_plan.as_mut().unwrap();
        swapped_plan.model_artifact_ref = coco
            .content_plan
            .as_ref()
            .unwrap()
            .model_artifact_ref
            .clone();
        assert_ne!(
            swapped_plan.model_artifact_ref,
            retained_result
                .content_plan
                .as_ref()
                .unwrap()
                .model_artifact_ref,
        );
        swapped_plan.id.clear();
        swapped_plan.id = identity(&*swapped_plan).unwrap();
        swapped.render.as_mut().unwrap().plan_id = swapped_plan.id.clone();
        let mismatch = inspect(&engine.store, "alice", &retained, &swapped).unwrap_err();
        assert!(
            mismatch.to_string().contains("CONTENT_ADOPTION_MISMATCH"),
            "{mismatch:?}"
        );

        drop(engine);
        let action_id = retain_attempted_prefix(&fixture.state);
        let mut engine = Engine::open(&fixture.state).unwrap();
        let replayed = engine.reconcile("alice", "case").unwrap();
        assert_eq!(replayed.lifecycle, Lifecycle::Commit, "{replayed:?}");
        assert_eq!(replayed.receipts[0].action_id, action_id);
        assert_eq!(replayed.content_plan, retained_result.content_plan);
        assert_eq!(fs::read(&fixture.target).unwrap(), original_bytes);
        assert_eq!(
            fs::metadata(&fixture.target).unwrap().modified().unwrap(),
            original_modified
        );
        assert_eq!(
            engine
                .store
                .load_memory_revision("alice", memory_revision)
                .unwrap(),
            Some(original_memory)
        );
        let database = Connection::open(fixture.state.join("state.sqlite3")).unwrap();
        database
            .execute("DELETE FROM memory_index_state WHERE actor_id='alice'", [])
            .unwrap();
        drop(database);
        assert!(inspect(&engine.store, "alice", &retained, &retained_result).is_ok());

        let history_request = RequestEnvelope::new(
            "history-case",
            "comparison-off-scope-history",
            "alice",
            "比較Recoveryの範囲外履歴",
        );
        let history_source = identity(&history_request).unwrap();
        engine
            .store
            .register_input("alice", &history_request, &history_source)
            .unwrap();
        let mut current_memory = engine.store.load_memory("alice").unwrap();
        let next_revision = current_memory.revision + 1;
        for ordinal in 0..80 {
            crate::relations::assert_fact(
                &mut current_memory,
                Proposition {
                    subject: format!("archive-subject-{ordinal}"),
                    predicate: "is".into(),
                    object: format!("archive-object-{ordinal}"),
                    negative: false,
                    scope: format!("archive-{ordinal}"),
                    condition_refs: BTreeSet::new(),
                    temporal_scope: None,
                },
                &history_source,
            )
            .unwrap();
        }
        current_memory.revision = next_revision;
        engine.store.save_memory("alice", &current_memory).unwrap();
        let full_snapshot_error = verify_relation_comparison_memory_result(
            &engine.store,
            "alice",
            "case",
            &current_memory,
            &retained_result,
            retained_result.content_plan.as_ref().unwrap(),
            &goal,
        )
        .unwrap_err();
        assert!(
            full_snapshot_error.to_string().contains("RELATION_BUDGET"),
            "{full_snapshot_error:?}"
        );
        assert!(inspect(&engine.store, "alice", &retained, &retained_result).is_ok());

        let invalidation = engine
            .execute(
                "alice",
                RequestEnvelope::new(
                    "invalidation-case",
                    "invalidate-comparison-premise",
                    "alice",
                    &format!("{mike_id}を無効化して"),
                ),
            )
            .unwrap();
        assert_eq!(invalidation.lifecycle, Lifecycle::Commit);
        let error = inspect(&engine.store, "alice", &retained, &retained_result).unwrap_err();
        assert!(error.to_string().contains("RECOVERY_PROOF_INVALIDATED"));
        assert_eq!(fs::read(&fixture.target).unwrap(), original_bytes);
    }

    #[test]
    fn saved_relation_comparison_explanation_pins_and_rechecks_its_memory_revision() {
        let fixture = empty_fixture();
        let mut engine = Engine::open(&fixture.state).unwrap();
        let coco = engine
            .execute(
                "alice",
                RequestEnvelope::new("case", "coco", "alice", "ココは鳥です"),
            )
            .unwrap();
        let coco_id = coco.dependencies[0].clone();
        let mike = engine
            .execute(
                "alice",
                RequestEnvelope::new("case", "mike", "alice", "ミケは鳥です"),
            )
            .unwrap();
        let mike_id = mike.dependencies[0].clone();
        let comparison = engine
            .execute(
                "alice",
                RequestEnvelope::new(
                    "case",
                    "relation-comparison",
                    "alice",
                    "ココとミケの保存済みの関係を比較して",
                ),
            )
            .unwrap();
        assert_eq!(comparison.lifecycle, Lifecycle::Commit, "{comparison:?}");
        let comparison_revision = comparison
            .actor_memory_revision
            .expect("Core records the saved comparison's actor-memory revision");

        let mut request =
            RequestEnvelope::new("case", "comparison-explanation", "alice", "詳しく説明して");
        request.budget.max_actions = 32;
        request.output_path = Some(fixture.target.clone());
        request
            .permissions
            .write_paths
            .insert(fixture.target.clone());
        let explanation = engine.execute("alice", request).unwrap();
        assert_eq!(explanation.lifecycle, Lifecycle::Commit, "{explanation:?}");
        assert!(explanation.text.contains(&comparison.id));
        assert_eq!(
            explanation.actor_memory_revision,
            Some(comparison_revision),
            "the explanation records the exact memory used to revalidate the saved comparison"
        );
        let mut expected_dependencies = vec![comparison.id.clone()];
        expected_dependencies.extend(comparison.dependencies.iter().cloned());
        assert_eq!(explanation.dependencies, expected_dependencies);
        let original_memory = engine
            .store
            .load_memory_revision("alice", comparison_revision)
            .unwrap()
            .expect("the comparison explanation's actor-memory revision is retained");
        assert!(original_memory.facts.iter().any(|fact| fact.id == coco_id));
        assert!(original_memory.facts.iter().any(|fact| fact.id == mike_id));

        let retained = engine.latest("alice", "case").unwrap().unwrap();
        let retained_result = retained.result.as_ref().unwrap().clone();
        assert!(matches!(
            retained.bundle.as_ref().unwrap().meaning.intent,
            Intent::ExplainPrevious
        ));
        inspect(&engine.store, "alice", &retained, &retained_result)
            .unwrap_or_else(|error| panic!("{error:?}"));
        let original_bytes = fs::read(&fixture.target).unwrap();

        drop(engine);
        let mut engine = Engine::open(&fixture.state).unwrap();
        assert_eq!(
            engine
                .store
                .load_memory_revision("alice", comparison_revision)
                .unwrap(),
            Some(original_memory)
        );
        assert!(inspect(&engine.store, "alice", &retained, &retained_result).is_ok());

        let database = Connection::open(fixture.state.join("state.sqlite3")).unwrap();
        database
            .execute("DELETE FROM memory_index_state WHERE actor_id='alice'", [])
            .unwrap();
        drop(database);
        assert!(inspect(&engine.store, "alice", &retained, &retained_result).is_ok());

        let history_request = RequestEnvelope::new(
            "extended-history-case",
            "comparison-explanation-off-scope-history",
            "alice",
            "関係比較説明Recoveryの範囲外履歴",
        );
        let history_source = identity(&history_request).unwrap();
        engine
            .store
            .register_input("alice", &history_request, &history_source)
            .unwrap();
        let mut current_memory = engine.store.load_memory("alice").unwrap();
        let next_revision = current_memory.revision + 1;
        for ordinal in 0..80 {
            crate::relations::assert_fact(
                &mut current_memory,
                Proposition {
                    subject: format!("archive-subject-{ordinal}"),
                    predicate: "is".into(),
                    object: format!("archive-object-{ordinal}"),
                    negative: false,
                    scope: format!("archive-{ordinal}"),
                    condition_refs: BTreeSet::new(),
                    temporal_scope: None,
                },
                &history_source,
            )
            .unwrap();
        }
        current_memory.revision = next_revision;
        engine.store.save_memory("alice", &current_memory).unwrap();

        let goal_ref = retained.goal_sha256.as_ref().unwrap();
        let goal: GoalContract =
            serde_json::from_slice(&engine.store.get_bytes("alice", "case", goal_ref).unwrap())
                .unwrap();
        assert!(matches!(goal.intent, Intent::ExplainPrevious));
        let previous_id = retained
            .bundle
            .as_ref()
            .and_then(|bundle| bundle.meaning.context_result_ref.as_deref())
            .expect("the sealed comparison explanation retains its prior-result reference");
        let previous = retained
            .prior_results
            .get(previous_id)
            .expect("the exact saved comparison remains retained");
        let full_snapshot_error = verify_explain_previous_relation_comparison_memory_result(
            &engine.store,
            &retained,
            Some(&current_memory),
            previous,
            &retained_result,
            retained_result.content_plan.as_ref().unwrap(),
            &goal,
        )
        .unwrap_err();
        assert!(
            full_snapshot_error.to_string().contains("RELATION_BUDGET"),
            "{full_snapshot_error:?}"
        );
        assert_eq!(current_memory.facts.len(), 82);
        assert!(current_memory.rules.is_empty());
        let mut full_reference_goal = goal.clone();
        full_reference_goal.budget.max_actions = 10_000;
        verify_explain_previous_relation_comparison_memory_result(
            &engine.store,
            &retained,
            Some(&current_memory),
            previous,
            &retained_result,
            retained_result.content_plan.as_ref().unwrap(),
            &full_reference_goal,
        )
        .unwrap();
        verify_explain_previous_relation_comparison_memory_result(
            &engine.store,
            &retained,
            None,
            previous,
            &retained_result,
            retained_result.content_plan.as_ref().unwrap(),
            &goal,
        )
        .unwrap();
        assert!(inspect(&engine.store, "alice", &retained, &retained_result).is_ok());

        let invalidation = engine
            .execute(
                "alice",
                RequestEnvelope::new(
                    "invalidation-case",
                    "invalidate-comparison-explanation-premise",
                    "alice",
                    &format!("{mike_id}を無効化して"),
                ),
            )
            .unwrap();
        assert_eq!(invalidation.lifecycle, Lifecycle::Commit);
        let error = inspect(&engine.store, "alice", &retained, &retained_result).unwrap_err();
        assert!(error.to_string().contains("RECOVERY_CONTEXT_PROOF_CHANGED"));
        assert_eq!(fs::read(&fixture.target).unwrap(), original_bytes);
    }

    #[test]
    fn common_relations_explanation_rejects_missing_actor_memory_snapshot_revision() {
        let fixture = empty_fixture();
        let mut engine = Engine::open(&fixture.state).unwrap();
        let rule = engine
            .execute(
                "alice",
                RequestEnvelope::new("case", "rule", "alice", "もしXは鳥ならXは動物"),
            )
            .unwrap();
        let rule_id = rule.dependencies[0].clone();
        let coco = engine
            .execute(
                "alice",
                RequestEnvelope::new("case", "coco", "alice", "ココは鳥です"),
            )
            .unwrap();
        let coco_id = coco.dependencies[0].clone();
        let mike = engine
            .execute(
                "alice",
                RequestEnvelope::new("case", "mike", "alice", "ミケは鳥です"),
            )
            .unwrap();
        let mike_id = mike.dependencies[0].clone();
        let common = engine
            .execute(
                "alice",
                RequestEnvelope::new(
                    "case",
                    "common-relations",
                    "alice",
                    "ココとミケの共通点は何ですか",
                ),
            )
            .unwrap();
        assert_eq!(common.lifecycle, Lifecycle::Commit, "{common:?}");
        assert_eq!(common.guarantee, Guarantee::Derived);
        assert!(common.dependencies.contains(&rule_id));
        assert!(common.dependencies.contains(&coco_id));
        assert!(common.dependencies.contains(&mike_id));
        let pinned_revision = common
            .actor_memory_revision
            .expect("the common-relations result records its actor-memory revision");
        assert!(
            engine
                .store
                .load_memory_revision("alice", pinned_revision)
                .unwrap()
                .is_some()
        );

        let mut explanation_request =
            RequestEnvelope::new("case", "common-explanation", "alice", "詳しく説明して");
        explanation_request.budget.max_actions = 32;
        explanation_request.output_path = Some(fixture.target.clone());
        explanation_request
            .permissions
            .write_paths
            .insert(fixture.target.clone());
        let explanation = engine.execute("alice", explanation_request).unwrap();
        assert_eq!(explanation.lifecycle, Lifecycle::Commit, "{explanation:?}");
        assert!(explanation.text.contains(&common.id));

        drop(engine);
        let engine = Engine::open(&fixture.state).unwrap();
        let retained = engine.latest("alice", "case").unwrap().unwrap();
        let retained_result = retained.result.as_ref().unwrap().clone();
        let original_bytes = fs::read(&fixture.target).unwrap();
        assert!(inspect(&engine.store, "alice", &retained, &retained_result).is_ok());

        let current_memory = engine.store.load_memory("alice").unwrap();
        let missing_revision = current_memory
            .revision
            .checked_add(1)
            .expect("the temporary fixture revision is in range");
        assert!(
            engine
                .store
                .load_memory_revision("alice", missing_revision)
                .unwrap()
                .is_none()
        );

        let mut missing_revision_pointer = retained.clone();
        let previous = missing_revision_pointer
            .prior_results
            .get_mut(&common.id)
            .expect("the retained case keeps the exact common-relations result");
        assert_eq!(previous.id, common.id);
        assert_eq!(previous.input_id, common.input_id);
        assert_eq!(previous.input_version, common.input_version);
        assert_eq!(previous.actor_memory_revision, Some(pinned_revision));
        previous.actor_memory_revision = Some(missing_revision);
        let (_, prior_bundle) = engine
            .store
            .get_input_bundle("alice", "case", &previous.input_id, previous.input_version)
            .unwrap()
            .expect("the exact sealed common-relations input remains available");
        assert_eq!(identity(&prior_bundle.request).unwrap(), previous.id);
        assert!(matches!(
            prior_bundle.meaning.intent,
            Intent::AskCommonRelations { .. }
        ));

        let error = inspect(
            &engine.store,
            "alice",
            &missing_revision_pointer,
            &retained_result,
        )
        .unwrap_err();
        assert!(
            error
                .to_string()
                .contains("RECOVERY_MEMORY_REVISION_MISSING"),
            "{error:?}"
        );
        assert_eq!(engine.latest("alice", "case").unwrap().unwrap(), retained);
        assert_eq!(engine.store.load_memory("alice").unwrap(), current_memory);
        assert_eq!(fs::read(&fixture.target).unwrap(), original_bytes);
    }

    #[test]
    fn common_relations_explanation_pins_and_rechecks_its_memory_revision() {
        let fixture = empty_fixture();
        let mut engine = Engine::open(&fixture.state).unwrap();
        let rule = engine
            .execute(
                "alice",
                RequestEnvelope::new("case", "rule", "alice", "もしXは鳥ならXは動物"),
            )
            .unwrap();
        let rule_id = rule.dependencies[0].clone();
        let coco = engine
            .execute(
                "alice",
                RequestEnvelope::new("case", "coco", "alice", "ココは鳥です"),
            )
            .unwrap();
        let coco_id = coco.dependencies[0].clone();
        let mike = engine
            .execute(
                "alice",
                RequestEnvelope::new("case", "mike", "alice", "ミケは鳥です"),
            )
            .unwrap();
        let mike_id = mike.dependencies[0].clone();
        let common = engine
            .execute(
                "alice",
                RequestEnvelope::new(
                    "case",
                    "common-relations",
                    "alice",
                    "ココとミケの共通点は何ですか",
                ),
            )
            .unwrap();
        assert_eq!(common.lifecycle, Lifecycle::Commit, "{common:?}");
        assert_eq!(common.guarantee, Guarantee::Derived);
        assert!(common.dependencies.contains(&rule_id));
        assert!(common.dependencies.contains(&coco_id));
        assert!(common.dependencies.contains(&mike_id));
        let memory_revision = common
            .actor_memory_revision
            .expect("the common-relations result records its actor-memory revision");

        let mut request =
            RequestEnvelope::new("case", "common-explanation", "alice", "詳しく説明して");
        request.budget.max_actions = 32;
        request.output_path = Some(fixture.target.clone());
        request
            .permissions
            .write_paths
            .insert(fixture.target.clone());
        let explanation = engine.execute("alice", request).unwrap();
        assert_eq!(explanation.lifecycle, Lifecycle::Commit, "{explanation:?}");
        assert!(explanation.text.contains(&common.id));
        assert_eq!(
            explanation.actor_memory_revision,
            Some(memory_revision),
            "the explanation records the exact memory used to recheck the common report"
        );
        let mut expected_dependencies = vec![common.id.clone()];
        expected_dependencies.extend(common.dependencies.iter().cloned());
        assert_eq!(explanation.dependencies, expected_dependencies);
        let original_memory = engine
            .store
            .load_memory_revision("alice", memory_revision)
            .unwrap()
            .expect("the explanation's actor-memory revision is retained");
        assert!(original_memory.rules.iter().any(|rule| rule.id == rule_id));
        assert!(original_memory.facts.iter().any(|fact| fact.id == coco_id));
        assert!(original_memory.facts.iter().any(|fact| fact.id == mike_id));
        assert_eq!(original_memory.facts.len(), 2);
        assert_eq!(original_memory.rules.len(), 1);
        let common_scope = original_memory
            .facts
            .iter()
            .find(|fact| fact.id == coco_id)
            .unwrap()
            .proposition
            .scope
            .clone();

        let retained = engine.latest("alice", "case").unwrap().unwrap();
        let retained_result = retained.result.as_ref().unwrap().clone();
        assert!(inspect(&engine.store, "alice", &retained, &retained_result).is_ok());
        let original_bytes = fs::read(&fixture.target).unwrap();
        drop(engine);

        let mut engine = Engine::open(&fixture.state).unwrap();
        assert_eq!(
            engine
                .store
                .load_memory_revision("alice", memory_revision)
                .unwrap(),
            Some(original_memory.clone())
        );
        assert!(inspect(&engine.store, "alice", &retained, &retained_result).is_ok());
        let database = Connection::open(fixture.state.join("state.sqlite3")).unwrap();
        database
            .execute("DELETE FROM memory_index_state WHERE actor_id='alice'", [])
            .unwrap();
        drop(database);
        assert!(inspect(&engine.store, "alice", &retained, &retained_result).is_ok());

        let history_request = RequestEnvelope::new(
            "independent-proof-history-case",
            "common-explanation-independent-proof-history",
            "alice",
            "共通関係説明Recoveryの無関係な規則履歴",
        );
        let history_source = identity(&history_request).unwrap();
        engine
            .store
            .register_input("alice", &history_request, &history_source)
            .unwrap();
        let mut current_memory = engine.store.load_memory("alice").unwrap();
        let next_revision = current_memory.revision + 1;
        for ordinal in 0..80 {
            let subject = format!("archive-subject-{ordinal}");
            let premise = Proposition {
                subject: subject.clone(),
                predicate: format!("archive-input-{ordinal}"),
                object: "present".into(),
                negative: false,
                scope: common_scope.clone(),
                condition_refs: BTreeSet::new(),
                temporal_scope: None,
            };
            crate::relations::add_rule(
                &mut current_memory,
                premise.clone(),
                Proposition {
                    subject,
                    predicate: format!("archive-output-{ordinal}"),
                    object: "derived".into(),
                    negative: false,
                    scope: common_scope.clone(),
                    condition_refs: BTreeSet::new(),
                    temporal_scope: None,
                },
                &history_source,
            )
            .unwrap();
            crate::relations::assert_fact(&mut current_memory, premise, &history_source).unwrap();
        }
        current_memory.revision = next_revision;
        engine.store.save_memory("alice", &current_memory).unwrap();

        let goal_ref = retained.goal_sha256.as_ref().unwrap();
        let goal: GoalContract =
            serde_json::from_slice(&engine.store.get_bytes("alice", "case", goal_ref).unwrap())
                .unwrap();
        let previous_id = retained
            .bundle
            .as_ref()
            .and_then(|bundle| bundle.meaning.context_result_ref.as_deref())
            .expect("the sealed explanation retains its prior-result reference");
        let previous = retained
            .prior_results
            .get(previous_id)
            .expect("the exact common-relations result remains retained");
        let full_snapshot_error = verify_explain_previous_common_relations_memory_result(
            &engine.store,
            &retained,
            Some(&current_memory),
            previous,
            &retained_result,
            retained_result.content_plan.as_ref().unwrap(),
            &goal,
        )
        .unwrap_err();
        assert!(
            full_snapshot_error.to_string().contains("RELATION_BUDGET"),
            "{full_snapshot_error:?}"
        );
        assert_eq!(current_memory.facts.len(), 82);
        assert_eq!(current_memory.rules.len(), 81);
        assert!(current_memory.rules.iter().any(|rule| rule.id == rule_id));
        let mut full_reference_goal = goal.clone();
        full_reference_goal.budget.max_actions = 10_000;
        verify_explain_previous_common_relations_memory_result(
            &engine.store,
            &retained,
            Some(&current_memory),
            previous,
            &retained_result,
            retained_result.content_plan.as_ref().unwrap(),
            &full_reference_goal,
        )
        .unwrap();
        verify_explain_previous_common_relations_memory_result(
            &engine.store,
            &retained,
            None,
            previous,
            &retained_result,
            retained_result.content_plan.as_ref().unwrap(),
            &goal,
        )
        .unwrap();
        assert!(inspect(&engine.store, "alice", &retained, &retained_result).is_ok());

        let invalidation = engine
            .execute(
                "alice",
                RequestEnvelope::new(
                    "invalidation-case",
                    "invalidate-common-explanation-premise",
                    "alice",
                    &format!("{mike_id}を無効化して"),
                ),
            )
            .unwrap();
        assert_eq!(invalidation.lifecycle, Lifecycle::Commit);
        let error = inspect(&engine.store, "alice", &retained, &retained_result).unwrap_err();
        assert!(error.to_string().contains("RECOVERY_CONTEXT_PROOF_CHANGED"));
        assert_eq!(fs::read(&fixture.target).unwrap(), original_bytes);
    }

    #[test]
    fn relation_objects_explanation_rejects_missing_actor_memory_snapshot_revision() {
        let fixture = empty_fixture();
        let mut engine = Engine::open(&fixture.state).unwrap();
        let direct = engine
            .execute(
                "alice",
                RequestEnvelope::new("case", "direct", "alice", "ミナはカッターを使う"),
            )
            .unwrap();
        let direct_id = direct.dependencies[0].clone();
        let rule = engine
            .execute(
                "alice",
                RequestEnvelope::new(
                    "case",
                    "rule",
                    "alice",
                    "もしXはカッターを使うならXは道具を使う",
                ),
            )
            .unwrap();
        let rule_id = rule.dependencies[0].clone();
        let objects = engine
            .execute(
                "alice",
                RequestEnvelope::new("case", "objects", "alice", "ミナは何を使いますか"),
            )
            .unwrap();
        assert_eq!(objects.lifecycle, Lifecycle::Commit, "{objects:?}");
        assert_eq!(objects.guarantee, Guarantee::Derived);
        assert!(objects.dependencies.contains(&direct_id));
        assert!(objects.dependencies.contains(&rule_id));
        let pinned_revision = objects
            .actor_memory_revision
            .expect("the object list records its actor-memory revision");
        assert!(
            engine
                .store
                .load_memory_revision("alice", pinned_revision)
                .unwrap()
                .is_some()
        );

        let mut explanation_request =
            RequestEnvelope::new("case", "object-explanation", "alice", "詳しく説明して");
        explanation_request.budget.max_actions = 32;
        explanation_request.output_path = Some(fixture.target.clone());
        explanation_request
            .permissions
            .write_paths
            .insert(fixture.target.clone());
        let explanation = engine.execute("alice", explanation_request).unwrap();
        assert_eq!(explanation.lifecycle, Lifecycle::Commit, "{explanation:?}");

        drop(engine);
        let engine = Engine::open(&fixture.state).unwrap();
        let retained = engine.latest("alice", "case").unwrap().unwrap();
        let retained_result = retained.result.as_ref().unwrap().clone();
        let original_bytes = fs::read(&fixture.target).unwrap();
        assert!(inspect(&engine.store, "alice", &retained, &retained_result).is_ok());

        let current_memory = engine.store.load_memory("alice").unwrap();
        let missing_revision = current_memory
            .revision
            .checked_add(1)
            .expect("the temporary fixture revision is in range");
        assert!(
            engine
                .store
                .load_memory_revision("alice", missing_revision)
                .unwrap()
                .is_none()
        );

        let mut missing_revision_pointer = retained.clone();
        let previous = missing_revision_pointer
            .prior_results
            .get_mut(&objects.id)
            .expect("the retained case keeps the exact relation-object result");
        assert_eq!(previous.id, objects.id);
        assert_eq!(previous.input_id, objects.input_id);
        assert_eq!(previous.input_version, objects.input_version);
        assert_eq!(previous.actor_memory_revision, Some(pinned_revision));
        previous.actor_memory_revision = Some(missing_revision);
        let (_, prior_bundle) = engine
            .store
            .get_input_bundle("alice", "case", &previous.input_id, previous.input_version)
            .unwrap()
            .expect("the exact sealed relation-object input remains available");
        assert_eq!(identity(&prior_bundle.request).unwrap(), previous.id);
        assert!(matches!(
            prior_bundle.meaning.intent,
            Intent::AskRelationObjects { .. }
        ));

        let error = inspect(
            &engine.store,
            "alice",
            &missing_revision_pointer,
            &retained_result,
        )
        .unwrap_err();
        assert!(
            error
                .to_string()
                .contains("RECOVERY_MEMORY_REVISION_MISSING"),
            "{error:?}"
        );
        assert_eq!(engine.latest("alice", "case").unwrap().unwrap(), retained);
        assert_eq!(engine.store.load_memory("alice").unwrap(), current_memory);
        assert_eq!(fs::read(&fixture.target).unwrap(), original_bytes);
    }

    #[test]
    fn relation_objects_explanation_pins_and_rechecks_its_memory_revision() {
        let fixture = empty_fixture();
        let mut engine = Engine::open(&fixture.state).unwrap();
        let direct = engine
            .execute(
                "alice",
                RequestEnvelope::new("case", "direct", "alice", "ミナはカッターを使う"),
            )
            .unwrap();
        let direct_id = direct.dependencies[0].clone();
        let rule = engine
            .execute(
                "alice",
                RequestEnvelope::new(
                    "case",
                    "rule",
                    "alice",
                    "もしXはカッターを使うならXは道具を使う",
                ),
            )
            .unwrap();
        let rule_id = rule.dependencies[0].clone();
        let objects = engine
            .execute(
                "alice",
                RequestEnvelope::new("case", "objects", "alice", "ミナは何を使いますか"),
            )
            .unwrap();
        assert_eq!(objects.lifecycle, Lifecycle::Commit, "{objects:?}");
        assert_eq!(objects.guarantee, Guarantee::Derived);
        assert!(objects.dependencies.contains(&direct_id));
        assert!(objects.dependencies.contains(&rule_id));
        let memory_revision = objects
            .actor_memory_revision
            .expect("the object inventory records its actor-memory revision");

        let mut request =
            RequestEnvelope::new("case", "object-explanation", "alice", "詳しく説明して");
        request.budget.max_actions = 32;
        request.output_path = Some(fixture.target.clone());
        request
            .permissions
            .write_paths
            .insert(fixture.target.clone());
        let explanation = engine.execute("alice", request).unwrap();
        assert_eq!(explanation.lifecycle, Lifecycle::Commit, "{explanation:?}");
        assert_eq!(
            explanation.actor_memory_revision,
            Some(memory_revision),
            "the explanation records the exact memory used to rebuild the object list"
        );
        assert!(explanation.text.contains("前回回答に含まれる関係の根拠"));
        assert!(explanation.text.contains(&direct_id));
        assert!(explanation.text.contains(&rule_id));
        let mut expected_dependencies = vec![objects.id.clone()];
        expected_dependencies.extend(objects.dependencies.iter().cloned());
        assert_eq!(explanation.dependencies, expected_dependencies);
        let original_memory = engine
            .store
            .load_memory_revision("alice", memory_revision)
            .unwrap()
            .expect("the object explanation's actor-memory revision is retained");
        assert!(
            original_memory
                .facts
                .iter()
                .any(|fact| fact.id == direct_id)
        );
        assert!(original_memory.rules.iter().any(|rule| rule.id == rule_id));
        assert_eq!(original_memory.facts.len(), 1);
        assert_eq!(original_memory.rules.len(), 1);

        let retained = engine.latest("alice", "case").unwrap().unwrap();
        let retained_result = retained.result.as_ref().unwrap().clone();
        assert!(inspect(&engine.store, "alice", &retained, &retained_result).is_ok());
        let original_bytes = fs::read(&fixture.target).unwrap();
        drop(engine);

        let mut engine = Engine::open(&fixture.state).unwrap();
        assert_eq!(
            engine
                .store
                .load_memory_revision("alice", memory_revision)
                .unwrap(),
            Some(original_memory.clone())
        );
        assert!(inspect(&engine.store, "alice", &retained, &retained_result).is_ok());

        let database = Connection::open(fixture.state.join("state.sqlite3")).unwrap();
        database
            .execute("DELETE FROM memory_index_state WHERE actor_id='alice'", [])
            .unwrap();
        drop(database);
        assert!(inspect(&engine.store, "alice", &retained, &retained_result).is_ok());

        let history_request = RequestEnvelope::new(
            "extended-history-case",
            "objects-explanation-history",
            "alice",
            "関係一覧説明Recoveryの範囲外履歴",
        );
        let history_source = identity(&history_request).unwrap();
        engine
            .store
            .register_input("alice", &history_request, &history_source)
            .unwrap();
        let mut current_memory = engine.store.load_memory("alice").unwrap();
        let next_revision = current_memory.revision + 1;
        for ordinal in 0..80 {
            crate::relations::assert_fact(
                &mut current_memory,
                Proposition {
                    subject: format!("archive-subject-{ordinal}"),
                    predicate: "unrelated".into(),
                    object: format!("archive-object-{ordinal}"),
                    negative: false,
                    scope: format!("archive-{ordinal}"),
                    condition_refs: BTreeSet::new(),
                    temporal_scope: None,
                },
                &history_source,
            )
            .unwrap();
        }
        current_memory.revision = next_revision;
        engine.store.save_memory("alice", &current_memory).unwrap();

        let goal_ref = retained.goal_sha256.as_ref().unwrap();
        let goal: GoalContract =
            serde_json::from_slice(&engine.store.get_bytes("alice", "case", goal_ref).unwrap())
                .unwrap();
        let previous_id = retained
            .bundle
            .as_ref()
            .and_then(|bundle| bundle.meaning.context_result_ref.as_deref())
            .expect("the sealed explanation retains its prior-result reference");
        let previous = retained
            .prior_results
            .get(previous_id)
            .expect("the exact relation-object result remains retained");
        let full_snapshot_error = verify_explain_previous_relation_objects_memory_result(
            &engine.store,
            &retained,
            Some(&current_memory),
            previous,
            &retained_result,
            retained_result.content_plan.as_ref().unwrap(),
            &goal,
        )
        .unwrap_err();
        assert!(
            full_snapshot_error.to_string().contains("RESOURCE_LIMIT"),
            "{full_snapshot_error:?}"
        );
        assert_eq!(current_memory.facts.len(), 81);
        assert_eq!(current_memory.rules.len(), 1);
        let mut full_reference_goal = goal.clone();
        full_reference_goal.budget.max_actions = 10_000;
        verify_explain_previous_relation_objects_memory_result(
            &engine.store,
            &retained,
            Some(&current_memory),
            previous,
            &retained_result,
            retained_result.content_plan.as_ref().unwrap(),
            &full_reference_goal,
        )
        .unwrap();
        verify_explain_previous_relation_objects_memory_result(
            &engine.store,
            &retained,
            None,
            previous,
            &retained_result,
            retained_result.content_plan.as_ref().unwrap(),
            &goal,
        )
        .unwrap();
        assert!(inspect(&engine.store, "alice", &retained, &retained_result).is_ok());

        let invalidation = engine
            .execute(
                "alice",
                RequestEnvelope::new(
                    "invalidation-case",
                    "invalidate-object-explanation-premise",
                    "alice",
                    &format!("{direct_id}を無効化して"),
                ),
            )
            .unwrap();
        assert_eq!(invalidation.lifecycle, Lifecycle::Commit);
        let error = inspect(&engine.store, "alice", &retained, &retained_result).unwrap_err();
        assert!(error.to_string().contains("RECOVERY_CONTEXT_PROOF_CHANGED"));
        assert_eq!(fs::read(&fixture.target).unwrap(), original_bytes);
    }

    #[test]
    fn common_relations_recovery_pins_and_rechecks_its_memory_revision() {
        let fixture = empty_fixture();
        let mut engine = Engine::open(&fixture.state).unwrap();
        let rule = engine
            .execute(
                "alice",
                RequestEnvelope::new("case", "rule", "alice", "もしXは鳥ならXは動物"),
            )
            .unwrap();
        let rule_id = rule.dependencies[0].clone();
        let coco = engine
            .execute(
                "alice",
                RequestEnvelope::new("case", "coco", "alice", "ココは鳥です"),
            )
            .unwrap();
        let coco_id = coco.dependencies[0].clone();
        let mike = engine
            .execute(
                "alice",
                RequestEnvelope::new("case", "mike", "alice", "ミケは鳥です"),
            )
            .unwrap();
        let mike_id = mike.dependencies[0].clone();

        let mut request = RequestEnvelope::new(
            "case",
            "common-relations",
            "alice",
            "ココとミケの共通点は何ですか",
        );
        request.budget.max_actions = 32;
        request.output_path = Some(fixture.target.clone());
        request
            .permissions
            .write_paths
            .insert(fixture.target.clone());
        let answer = engine.execute("alice", request).unwrap();
        assert_eq!(answer.lifecycle, Lifecycle::Commit);
        assert_eq!(answer.guarantee, Guarantee::Derived);
        assert!(answer.text.contains("共通関係"));
        assert!(answer.dependencies.contains(&rule_id));
        assert!(answer.dependencies.contains(&coco_id));
        assert!(answer.dependencies.contains(&mike_id));
        let memory_revision = answer
            .actor_memory_revision
            .expect("Core records the actor-memory snapshot used by the common-relations result");
        let original_memory = engine
            .store
            .load_memory_revision("alice", memory_revision)
            .unwrap()
            .expect("the comparison's actor-memory revision is retained");
        assert!(original_memory.rules.iter().any(|rule| rule.id == rule_id));
        assert!(original_memory.facts.iter().any(|fact| fact.id == coco_id));
        assert!(original_memory.facts.iter().any(|fact| fact.id == mike_id));

        let retained = engine.latest("alice", "case").unwrap().unwrap();
        let retained_result = retained.result.as_ref().unwrap().clone();
        assert_eq!(retained_result.actor_memory_revision, Some(memory_revision));
        assert!(inspect(&engine.store, "alice", &retained, &retained_result).is_ok());
        let original_bytes = fs::read(&fixture.target).unwrap();

        drop(engine);
        let mut engine = Engine::open(&fixture.state).unwrap();
        assert_eq!(
            engine
                .store
                .load_memory_revision("alice", memory_revision)
                .unwrap(),
            Some(original_memory)
        );
        let database = Connection::open(fixture.state.join("state.sqlite3")).unwrap();
        database
            .execute("DELETE FROM memory_index_state WHERE actor_id='alice'", [])
            .unwrap();
        drop(database);
        assert!(inspect(&engine.store, "alice", &retained, &retained_result).is_ok());

        let history_request = RequestEnvelope::new(
            "history-case",
            "common-relation-off-scope-history",
            "alice",
            "共通関係Recoveryの範囲外履歴",
        );
        let history_source = identity(&history_request).unwrap();
        engine
            .store
            .register_input("alice", &history_request, &history_source)
            .unwrap();
        let mut current_memory = engine.store.load_memory("alice").unwrap();
        let next_revision = current_memory.revision + 1;
        for ordinal in 0..80 {
            crate::relations::assert_fact(
                &mut current_memory,
                Proposition {
                    subject: format!("archive-subject-{ordinal}"),
                    predicate: "is".into(),
                    object: format!("archive-object-{ordinal}"),
                    negative: false,
                    scope: format!("archive-{ordinal}"),
                    condition_refs: BTreeSet::new(),
                    temporal_scope: None,
                },
                &history_source,
            )
            .unwrap();
        }
        current_memory.revision = next_revision;
        engine.store.save_memory("alice", &current_memory).unwrap();
        let goal_ref = retained.goal_sha256.as_ref().unwrap();
        let goal: GoalContract =
            serde_json::from_slice(&engine.store.get_bytes("alice", "case", goal_ref).unwrap())
                .unwrap();
        let full_snapshot_error = verify_common_relations_memory_result(
            &engine.store,
            "alice",
            "case",
            &current_memory,
            &retained_result,
            retained_result.content_plan.as_ref().unwrap(),
            &goal,
        )
        .unwrap_err();
        assert!(
            full_snapshot_error.to_string().contains("RESOURCE_LIMIT"),
            "{full_snapshot_error:?}"
        );
        assert!(inspect(&engine.store, "alice", &retained, &retained_result).is_ok());

        let invalidation = engine
            .execute(
                "alice",
                RequestEnvelope::new(
                    "case",
                    "invalidate-common-relation",
                    "alice",
                    &format!("{mike_id}を無効化して"),
                ),
            )
            .unwrap();
        assert_eq!(invalidation.lifecycle, Lifecycle::Commit);
        let error = inspect(&engine.store, "alice", &retained, &retained_result).unwrap_err();
        assert!(error.to_string().contains("RECOVERY_PROOF_INVALIDATED"));
        assert_eq!(fs::read(&fixture.target).unwrap(), original_bytes);
    }

    #[test]
    fn relation_profile_recovery_pins_and_rechecks_its_memory_revision() {
        let fixture = empty_fixture();
        let mut engine = Engine::open(&fixture.state).unwrap();
        let rule = engine
            .execute(
                "alice",
                RequestEnvelope::new("case", "rule", "alice", "もしXは鳥ならXは動物"),
            )
            .unwrap();
        let rule_id = rule.dependencies[0].clone();
        let premise = engine
            .execute(
                "alice",
                RequestEnvelope::new("case", "premise", "alice", "ココは鳥です"),
            )
            .unwrap();
        let premise_id = premise.dependencies[0].clone();

        let mut request = RequestEnvelope::new(
            "case",
            "relation-profile",
            "alice",
            "ココについて保存されている情報をもとに短い紹介文を書いて",
        );
        request.budget.max_actions = 64;
        request.output_path = Some(fixture.target.clone());
        request
            .permissions
            .write_paths
            .insert(fixture.target.clone());
        let profile = engine.execute("alice", request).unwrap();
        assert_eq!(profile.lifecycle, Lifecycle::Commit);
        assert_eq!(profile.guarantee, Guarantee::Hypothesis);
        assert!(profile.text.contains("紹介文案"));
        assert!(profile.dependencies.contains(&rule_id));
        assert!(profile.dependencies.contains(&premise_id));
        let memory_revision = profile
            .actor_memory_revision
            .expect("Core records the actor-memory snapshot used by the profile");
        let original_memory = engine
            .store
            .load_memory_revision("alice", memory_revision)
            .unwrap()
            .expect("the profile's actor-memory revision is retained");
        assert!(original_memory.rules.iter().any(|rule| rule.id == rule_id));
        assert!(
            original_memory
                .facts
                .iter()
                .any(|fact| fact.id == premise_id)
        );

        let retained = engine.latest("alice", "case").unwrap().unwrap();
        let retained_result = retained.result.as_ref().unwrap().clone();
        assert_eq!(retained_result.actor_memory_revision, Some(memory_revision));
        assert!(inspect(&engine.store, "alice", &retained, &retained_result).is_ok());
        let original_bytes = fs::read(&fixture.target).unwrap();

        drop(engine);
        let mut engine = Engine::open(&fixture.state).unwrap();
        assert_eq!(
            engine
                .store
                .load_memory_revision("alice", memory_revision)
                .unwrap(),
            Some(original_memory)
        );
        assert!(inspect(&engine.store, "alice", &retained, &retained_result).is_ok());

        let database = Connection::open(fixture.state.join("state.sqlite3")).unwrap();
        database
            .execute("DELETE FROM memory_index_state WHERE actor_id='alice'", [])
            .unwrap();
        drop(database);
        assert!(inspect(&engine.store, "alice", &retained, &retained_result).is_ok());

        let history_request = RequestEnvelope::new(
            "history-case",
            "profile-recovery-off-scope-history",
            "alice",
            "紹介文Recoveryの範囲外履歴",
        );
        let history_source = identity(&history_request).unwrap();
        engine
            .store
            .register_input("alice", &history_request, &history_source)
            .unwrap();
        let mut current_memory = engine.store.load_memory("alice").unwrap();
        let next_revision = current_memory.revision + 1;
        for ordinal in 0..80 {
            crate::relations::assert_fact(
                &mut current_memory,
                Proposition {
                    subject: format!("archive-subject-{ordinal}"),
                    predicate: "is".into(),
                    object: format!("archive-object-{ordinal}"),
                    negative: false,
                    scope: format!("archive-{ordinal}"),
                    condition_refs: BTreeSet::new(),
                    temporal_scope: None,
                },
                &history_source,
            )
            .unwrap();
        }
        current_memory.revision = next_revision;
        engine.store.save_memory("alice", &current_memory).unwrap();

        let goal_ref = retained.goal_sha256.as_ref().unwrap();
        let goal: GoalContract =
            serde_json::from_slice(&engine.store.get_bytes("alice", "case", goal_ref).unwrap())
                .unwrap();
        let Intent::DraftRelationProfile { subject } = &goal.intent else {
            panic!("retained profile lost its typed subject");
        };
        let full_answer = crate::core::relation_profile_answer(
            &current_memory,
            subject,
            10_000,
            goal.budget.max_evidence_bytes,
        )
        .unwrap();
        let (indexed_revision, indexed_answer) =
            crate::core::indexed_relation_profile_answer_from_store(
                &engine.store,
                "alice",
                subject,
                goal.budget.max_actions,
                goal.budget.max_evidence_bytes,
            )
            .unwrap()
            .expect("current profile subject index is available");
        let indexed_answer = indexed_answer.unwrap();
        assert_eq!(current_memory.facts.len(), 81);
        assert_eq!(indexed_revision, current_memory.revision);
        assert_eq!(indexed_answer, full_answer);
        assert!(matches!(
            crate::core::relation_profile_answer(
                &current_memory,
                subject,
                goal.budget.max_actions,
                goal.budget.max_evidence_bytes,
            ),
            Err(Error::Contract { code, .. }) if code == "RELATION_BUDGET"
        ));
        assert!(inspect(&engine.store, "alice", &retained, &retained_result).is_ok());

        let invalidation = engine
            .execute(
                "alice",
                RequestEnvelope::new(
                    "case",
                    "invalidate-profile-rule",
                    "alice",
                    &format!("{rule_id}を無効化して"),
                ),
            )
            .unwrap();
        assert_eq!(invalidation.lifecycle, Lifecycle::Commit);
        let error = inspect(&engine.store, "alice", &retained, &retained_result).unwrap_err();
        assert!(error.to_string().contains("RECOVERY_PROOF_INVALIDATED"));
        assert_eq!(fs::read(&fixture.target).unwrap(), original_bytes);
    }

    #[test]
    fn relation_profile_explanation_pins_and_rechecks_its_memory_revision() {
        let fixture = empty_fixture();
        let mut engine = Engine::open(&fixture.state).unwrap();
        let rule = engine
            .execute(
                "alice",
                RequestEnvelope::new("case", "rule", "alice", "もしXは鳥ならXは動物"),
            )
            .unwrap();
        let rule_id = rule.dependencies[0].clone();
        let premise = engine
            .execute(
                "alice",
                RequestEnvelope::new("case", "premise", "alice", "ココは鳥です"),
            )
            .unwrap();
        let premise_id = premise.dependencies[0].clone();

        let mut profile_request = RequestEnvelope::new(
            "case",
            "relation-profile",
            "alice",
            "ココについて保存されている情報をもとに短い紹介文を書いて",
        );
        profile_request.output_path = Some(fixture.target.clone());
        profile_request
            .permissions
            .write_paths
            .insert(fixture.target.clone());
        let profile = engine.execute("alice", profile_request).unwrap();
        assert_eq!(profile.lifecycle, Lifecycle::Commit);
        assert_eq!(profile.guarantee, Guarantee::Hypothesis);
        let profile_memory_revision = profile
            .actor_memory_revision
            .expect("Core records the actor-memory snapshot used by the profile");

        let explanation_target = fixture.target.with_file_name("profile-explanation.txt");
        let mut explanation_request =
            RequestEnvelope::new("case", "profile-explanation", "alice", "詳しく説明して");
        explanation_request.budget.max_actions = 32;
        explanation_request.output_path = Some(explanation_target.clone());
        explanation_request
            .permissions
            .write_paths
            .insert(explanation_target.clone());
        let explanation = engine.execute("alice", explanation_request).unwrap();
        assert_eq!(explanation.lifecycle, Lifecycle::Commit, "{explanation:?}");
        assert_eq!(explanation.guarantee, Guarantee::Hypothesis);
        assert_eq!(
            explanation.actor_memory_revision,
            Some(profile_memory_revision)
        );
        for marker in [
            "前回の保存済み紹介文案を説明します",
            profile.id.as_str(),
            "会話で提示された前提は「ココは鳥である」",
            "保存された規則による導出は「ココは動物である」",
            "現実世界の真偽や記録の網羅性は確認していません",
        ] {
            assert!(explanation.text.contains(marker), "missing {marker:?}");
        }
        let mut expected_dependencies = vec![profile.id.clone()];
        expected_dependencies.extend(profile.dependencies.iter().cloned());
        assert_eq!(explanation.dependencies, expected_dependencies);
        assert!(explanation.dependencies.contains(&rule_id));
        assert!(explanation.dependencies.contains(&premise_id));

        let retained = engine.latest("alice", "case").unwrap().unwrap();
        let retained_result = retained.result.as_ref().unwrap().clone();
        assert!(matches!(
            retained.bundle.as_ref().unwrap().meaning.intent,
            Intent::ExplainPrevious
        ));
        inspect(&engine.store, "alice", &retained, &retained_result)
            .unwrap_or_else(|error| panic!("{error:?}"));
        let explanation_bytes = fs::read(&explanation_target).unwrap();

        drop(engine);
        let mut engine = Engine::open(&fixture.state).unwrap();
        assert!(
            engine
                .store
                .load_memory_revision("alice", profile_memory_revision)
                .unwrap()
                .is_some()
        );
        assert!(inspect(&engine.store, "alice", &retained, &retained_result).is_ok());

        let database = Connection::open(fixture.state.join("state.sqlite3")).unwrap();
        database
            .execute("DELETE FROM memory_index_state WHERE actor_id='alice'", [])
            .unwrap();
        drop(database);
        assert!(inspect(&engine.store, "alice", &retained, &retained_result).is_ok());

        let history_request = RequestEnvelope::new(
            "extended-history-case",
            "profile-explanation-off-scope-history",
            "alice",
            "紹介文説明Recoveryの範囲外履歴",
        );
        let history_source = identity(&history_request).unwrap();
        engine
            .store
            .register_input("alice", &history_request, &history_source)
            .unwrap();
        let mut current_memory = engine.store.load_memory("alice").unwrap();
        let next_revision = current_memory.revision + 1;
        for ordinal in 0..80 {
            crate::relations::assert_fact(
                &mut current_memory,
                Proposition {
                    subject: format!("archive-subject-{ordinal}"),
                    predicate: "is".into(),
                    object: format!("archive-object-{ordinal}"),
                    negative: false,
                    scope: format!("archive-{ordinal}"),
                    condition_refs: BTreeSet::new(),
                    temporal_scope: None,
                },
                &history_source,
            )
            .unwrap();
        }
        current_memory.revision = next_revision;
        engine.store.save_memory("alice", &current_memory).unwrap();

        let goal_ref = retained.goal_sha256.as_ref().unwrap();
        let goal: GoalContract =
            serde_json::from_slice(&engine.store.get_bytes("alice", "case", goal_ref).unwrap())
                .unwrap();
        assert!(matches!(goal.intent, Intent::ExplainPrevious));
        let previous_id = retained
            .bundle
            .as_ref()
            .and_then(|bundle| bundle.meaning.context_result_ref.as_deref())
            .expect("the sealed profile explanation retains its prior-result reference");
        let previous = retained
            .prior_results
            .get(previous_id)
            .expect("the exact saved relation profile remains retained");
        let (subject, _, _) = crate::core::previous_relation_profile_request(
            &engine.store,
            "alice",
            "case",
            previous,
        )
        .unwrap()
        .expect("the previous result retains its typed profile request");
        let full_answer = crate::core::relation_profile_answer(
            &current_memory,
            &subject,
            10_000,
            goal.budget.max_evidence_bytes,
        )
        .unwrap();
        assert!(matches!(
            crate::core::relation_profile_answer(
                &current_memory,
                &subject,
                goal.budget.max_actions,
                goal.budget.max_evidence_bytes,
            ),
            Err(Error::Contract { code, .. }) if code == "RELATION_BUDGET"
        ));
        let (indexed_revision, indexed_answer) =
            crate::core::indexed_relation_profile_answer_from_store(
                &engine.store,
                "alice",
                &subject,
                goal.budget.max_actions,
                goal.budget.max_evidence_bytes,
            )
            .unwrap()
            .expect("current profile subject index is available");
        let indexed_answer = indexed_answer.unwrap();
        assert_eq!(current_memory.facts.len(), 81);
        assert_eq!(indexed_revision, current_memory.revision);
        assert_eq!(indexed_answer, full_answer);
        assert!(inspect(&engine.store, "alice", &retained, &retained_result).is_ok());

        let invalidation = engine
            .execute(
                "alice",
                RequestEnvelope::new(
                    "invalidation-case",
                    "invalidate-profile-explanation-rule",
                    "alice",
                    &format!("{rule_id}を無効化して"),
                ),
            )
            .unwrap();
        assert_eq!(invalidation.lifecycle, Lifecycle::Commit);
        let error = inspect(&engine.store, "alice", &retained, &retained_result).unwrap_err();
        assert!(error.to_string().contains("RECOVERY_CONTEXT_PROOF_CHANGED"));
        assert_eq!(fs::read(&explanation_target).unwrap(), explanation_bytes);
    }

    #[test]
    fn tight_composite_explanation_reconciles_with_unrelated_history_and_cold_index() {
        let fixture = empty_fixture();
        let mut engine = Engine::open(&fixture.state).unwrap();
        let mut source_input = String::new();
        for (id, text) in [
            (
                "calibration-condition",
                "条件定義: [scope=実験室][condition=校正] ユリは校正済み",
            ),
            (
                "inspection-condition",
                "条件定義: [scope=実験室][condition=点検] ユリは点検済み",
            ),
            (
                "stop-condition",
                "条件定義: [scope=実験室][condition=停止] ユリは停止中",
            ),
            (
                "combined-condition",
                "条件定義: [scope=実験室][condition=監視][subject=ユリ] AND([ref=校正], OR([ref=点検], NOT([ref=停止])))",
            ),
            ("calibration", "[scope=実験室]ユリは校正済み"),
            ("not-stopped", "[scope=実験室]ユリは停止中ではない"),
            (
                "guarded-claim",
                "[scope=実験室][condition=監視]ユリは観測可能",
            ),
        ] {
            let result = engine
                .execute("alice", RequestEnvelope::new("case", id, "alice", text))
                .unwrap();
            assert_eq!(result.lifecycle, Lifecycle::Commit, "{result:?}");
            source_input = result.id;
        }

        let mut original = engine.store.load_memory("alice").unwrap();
        let next_revision = original.revision + 1;
        for ordinal in 0..24 {
            crate::relations::assert_fact(
                &mut original,
                Proposition {
                    subject: format!("archive-subject-{ordinal}"),
                    predicate: "is".into(),
                    object: "archived".into(),
                    negative: false,
                    scope: "archive".into(),
                    condition_refs: BTreeSet::new(),
                    temporal_scope: None,
                },
                &source_input,
            )
            .unwrap();
        }
        original.revision = next_revision;
        engine.store.save_memory("alice", &original).unwrap();

        let mut request = RequestEnvelope::new(
            "case",
            "tight-why",
            "alice",
            "なぜ[scope=実験室][condition=監視]ユリは観測可能ですか",
        );
        request.budget.max_actions = 30;
        request.output_path = Some(fixture.target.clone());
        request
            .permissions
            .write_paths
            .insert(fixture.target.clone());
        let Intent::AskWhy { proposition } =
            crate::input::seal(&request, None).unwrap().meaning.intent
        else {
            panic!("expected an ordinary relation explanation");
        };
        assert!(matches!(
            crate::relations::query(&original, &proposition, request.budget.max_actions),
            Err(Error::Contract { code, .. }) if code == "RELATION_BUDGET"
        ));
        let saved = engine.execute("alice", request.clone()).unwrap();
        assert_eq!(saved.lifecycle, Lifecycle::Commit, "{saved:?}");
        assert_eq!(saved.actor_memory_revision, Some(original.revision));
        assert_eq!(saved.dependencies.len(), 7);
        let file_bytes = fs::read(&fixture.target).unwrap();
        let modified = fs::metadata(&fixture.target).unwrap().modified().unwrap();
        drop(engine);
        let token = retain_attempted_prefix(&fixture.state);

        let mut engine = Engine::open(&fixture.state).unwrap();
        let database = Connection::open(fixture.state.join("state.sqlite3")).unwrap();
        database
            .execute("DELETE FROM memory_index_state WHERE actor_id='alice'", [])
            .unwrap();
        let pending = engine.store.load_case("alice", "case").unwrap().unwrap();
        RECOVERY_SNAPSHOT_QUERIES.with(|calls| calls.set(0));
        inspect(
            &engine.store,
            "alice",
            &pending,
            pending.result.as_ref().unwrap(),
        )
        .unwrap();
        RECOVERY_SNAPSHOT_QUERIES.with(|calls| assert_eq!(calls.get(), 1));

        let later = engine
            .execute(
                "alice",
                RequestEnvelope::new(
                    "other-case",
                    "unrelated",
                    "alice",
                    "[scope=archive]ナナは記録済み",
                ),
            )
            .unwrap();
        assert_eq!(later.lifecycle, Lifecycle::Commit);
        let current = engine.store.load_memory("alice").unwrap();
        assert!(current.revision > original.revision);
        database
            .execute("DELETE FROM memory_index_state WHERE actor_id='alice'", [])
            .unwrap();

        RECOVERY_SNAPSHOT_QUERIES.with(|calls| calls.set(0));
        let recovered = engine.reconcile("alice", "case").unwrap();
        RECOVERY_SNAPSHOT_QUERIES.with(|calls| assert_eq!(calls.get(), 2));
        assert_eq!(recovered.lifecycle, Lifecycle::Commit, "{recovered:?}");
        assert_eq!(recovered.id, saved.id);
        assert_eq!(recovered.dependencies, saved.dependencies);
        assert_eq!(recovered.actor_memory_revision, saved.actor_memory_revision);
        assert_eq!(recovered.render, saved.render);
        assert_eq!(
            recovered.receipts[0].reason.as_deref(),
            Some("LOCAL_FILE_RECONCILED_NO_RESEND")
        );
        let actions = engine.store.actions("alice", "case").unwrap();
        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0].action_id, recovered.receipts[0].action_id);
        assert_eq!(actions[0].idempotency_key, token);
        assert_eq!(actions[0].phase, EffectPhase::Confirmed);
        assert_eq!(fs::read(&fixture.target).unwrap(), file_bytes);
        assert_eq!(
            fs::metadata(&fixture.target).unwrap().modified().unwrap(),
            modified
        );
        assert_eq!(engine.store.load_memory("alice").unwrap(), current);
        assert!(
            engine
                .store
                .with_relation_index("alice", |_| Ok(()))
                .unwrap()
                .is_none()
        );
        assert_eq!(engine.execute("alice", request).unwrap(), recovered);
    }

    #[test]
    fn interrupted_actual_write_reconciles_same_identity_without_resending() {
        let fixture = attempted_prefix();
        let original = fs::read(&fixture.target).unwrap();
        let modified = fs::metadata(&fixture.target).unwrap().modified().unwrap();
        let store = Store::open(&fixture.state).unwrap();
        assert_eq!(
            store.actions("alice", "case").unwrap()[0].phase,
            EffectPhase::Unknown
        );
        let result = reconcile(&store, "alice", "case").unwrap();
        assert_eq!(result.lifecycle, Lifecycle::Commit);
        assert!(result.execution_graph_ref.is_some());
        let actions = store.actions("alice", "case").unwrap();
        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0].idempotency_key, fixture.token);
        assert_eq!(actions[0].phase, EffectPhase::Confirmed);
        assert_eq!(fs::read(&fixture.target).unwrap(), original);
        assert_eq!(
            fs::metadata(&fixture.target).unwrap().modified().unwrap(),
            modified
        );
        drop(store);
        let restarted = Store::open(&fixture.state).unwrap();
        assert_eq!(
            reconcile(&restarted, "alice", "case").unwrap().lifecycle,
            Lifecycle::Commit
        );
        assert_eq!(restarted.actions("alice", "case").unwrap().len(), 1);
    }

    #[test]
    fn substituted_render_contract_cannot_confirm_an_actual_file() {
        for change in [
            "version",
            "target",
            "alias-target",
            "missing-target",
            "legacy",
            "status",
        ] {
            let fixture = attempted_prefix();
            let store = Store::open(&fixture.state).unwrap();
            let mut state = store.load_case("alice", "case").unwrap().unwrap();
            let mut result = state.result.clone().unwrap();
            let render = result.render.as_mut().unwrap();
            let bytes = fs::read(&fixture.target).unwrap();
            let modified = fs::metadata(&fixture.target).unwrap().modified().unwrap();
            assert_eq!(render.bytes, bytes);
            let original_goal = state.goal_sha256.clone();
            match change {
                "version" => render.renderer_version = "unknown-renderer".into(),
                "target" => {
                    render.target = Some(RenderTarget::LocalFile {
                        path: fixture.directory.join("different-report.txt"),
                    })
                }
                "alias-target" => {
                    let alias = fixture.directory.join(".").join("actual-report.txt");
                    assert_eq!(alias, fixture.target);
                    assert_ne!(alias.as_os_str(), fixture.target.as_os_str());
                    render.target = Some(RenderTarget::LocalFile { path: alias });
                }
                "missing-target" => render.target = None,
                "legacy" => {
                    *render = crate::output::render(result.content_plan.as_ref().unwrap()).unwrap();
                }
                "status" => render.status = "FAILED".into(),
                _ => unreachable!(),
            }
            // Publish the substituted observation with its own valid artifact
            // identity so the original Goal must reject it on its merits.
            store.put_artifact("alice", "case", render).unwrap();
            advance(&store, &mut state, &result, Lifecycle::Suspend).unwrap();

            let stopped = reconcile(&store, "alice", "case").unwrap();
            assert_eq!(
                stopped.lifecycle,
                Lifecycle::Suspend,
                "{change}: {stopped:?}"
            );
            assert!(
                stopped.reason.unwrap().contains("RECOVERY_RENDER_MISMATCH"),
                "{change}"
            );
            assert!(stopped.receipts.is_empty());
            assert_eq!(
                store
                    .load_case("alice", "case")
                    .unwrap()
                    .unwrap()
                    .goal_sha256,
                original_goal
            );
            let actions = store.actions("alice", "case").unwrap();
            assert_eq!(actions.len(), 1);
            assert_eq!(actions[0].action_id, fixture.token);
            assert_eq!(actions[0].phase, EffectPhase::Unknown);
            assert_eq!(fs::read(&fixture.target).unwrap(), bytes);
            assert_eq!(
                fs::metadata(&fixture.target).unwrap().modified().unwrap(),
                modified
            );
        }
    }
    #[test]
    fn missing_and_different_files_stay_unknown_and_are_never_rewritten() {
        for missing in [false, true] {
            let fixture = attempted_prefix();
            if missing {
                fs::remove_file(&fixture.target).unwrap();
            } else {
                fs::write(&fixture.target, b"different actual file").unwrap();
            }
            let store = Store::open(&fixture.state).unwrap();
            let result = reconcile(&store, "alice", "case").unwrap();
            assert_eq!(result.lifecycle, Lifecycle::Suspend);
            assert_eq!(
                store.actions("alice", "case").unwrap()[0].phase,
                EffectPhase::Unknown
            );
            if missing {
                assert!(!fixture.target.exists());
            } else {
                assert_eq!(fs::read(&fixture.target).unwrap(), b"different actual file");
            }
            assert!(reconcile(&store, "bob", "case").is_err());
        }
    }
    #[test]
    fn originally_unauthorized_file_cannot_be_confirmed_by_recovery() {
        let directory = std::env::temp_dir().join(format!(
            "minidora-recovery-noauth-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir(&directory).unwrap();
        let state = directory.join("state");
        let target = directory.join("report.txt");
        let mut request = RequestEnvelope::new("case", "input", "alice", "2+3");
        request.output_path = Some(target.clone());
        let mut engine = Engine::open(&state).unwrap();
        let result = engine.execute("alice", request).unwrap();
        assert_eq!(result.lifecycle, Lifecycle::Suspend);
        drop(engine);
        let store = Store::open(&state).unwrap();
        assert_eq!(
            reconcile(&store, "alice", "case").unwrap().lifecycle,
            Lifecycle::Suspend
        );
        assert!(store.actions("alice", "case").unwrap().is_empty());
        assert!(!target.exists());
        drop(store);
        fs::remove_dir_all(directory).unwrap();
    }
    #[cfg(unix)]
    #[test]
    fn a_replaced_symlink_is_not_an_actual_original_file_receipt() {
        use std::os::unix::fs::symlink;
        let fixture = attempted_prefix();
        let moved = fixture.directory.join("different-target.txt");
        fs::rename(&fixture.target, &moved).unwrap();
        symlink(&moved, &fixture.target).unwrap();
        let store = Store::open(&fixture.state).unwrap();
        assert_eq!(
            reconcile(&store, "alice", "case").unwrap().lifecycle,
            Lifecycle::Suspend
        );
        assert_eq!(
            store.actions("alice", "case").unwrap()[0].phase,
            EffectPhase::Unknown
        );
    }
    #[test]
    fn sequence_cancel_closes_only_unattempted_local_write() {
        for (cut, phase) in [
            (0, EffectPhase::Proposed),
            (1, EffectPhase::Authorized),
            (2, EffectPhase::Prepared),
        ] {
            let fixture = sequence_file_preparation_prefix(cut);
            let bytes = fs::read(&fixture.target).unwrap();
            let modified = fs::metadata(&fixture.target).unwrap().modified().unwrap();
            let stale_writer = Store::open(&fixture.state).unwrap();
            let old = stale_writer.load_case("alice", "case").unwrap().unwrap();
            let mut action = stale_writer.actions("alice", "case").unwrap().remove(0);
            assert_eq!(action.phase, phase);
            assert_eq!(action.action_id, fixture.token);
            let mut engine = Engine::open(&fixture.state).unwrap();
            if phase == EffectPhase::Prepared {
                // Matching bytes alone cannot prove a write that was never issued.
                let pending = engine.reconcile("alice", "case").unwrap();
                assert_eq!(pending.lifecycle, Lifecycle::Suspend);
                assert!(pending.receipts.is_empty());
                let blocked = engine
                    .execute(
                        "alice",
                        RequestEnvelope::new("case", "blocked-before-cancel", "alice", "1+1"),
                    )
                    .unwrap();
                assert_eq!(blocked.lifecycle, Lifecycle::Suspend);
                assert!(
                    blocked
                        .reason
                        .unwrap()
                        .starts_with("EFFECT_PREPARATION_UNRESOLVED")
                );
            }
            let cancellation = engine
                .execute(
                    "alice",
                    RequestEnvelope::new("case", "cancel-sequence", "alice", "キャンセル"),
                )
                .unwrap();
            assert_eq!(cancellation.lifecycle, Lifecycle::Cancel);
            assert_eq!(cancellation.receipts[0].phase, EffectPhase::Cancelled);
            assert!(cancellation.receipts[0].observed_bytes.is_none());
            let closed = stale_writer.actions("alice", "case").unwrap();
            assert_eq!(closed.len(), 1);
            assert_eq!(closed[0].action_id, fixture.token);
            assert_eq!(closed[0].phase, EffectPhase::Cancelled);
            assert!(closed[0].receipt.is_none());
            let state = engine.latest("alice", "case").unwrap().unwrap();
            assert_eq!(
                state.prior_results[old.input_fingerprint.as_ref().unwrap()].lifecycle,
                Lifecycle::Cancel
            );
            assert_eq!(
                engine.reconcile_cancelled_effects("alice", "case").unwrap(),
                closed
            );
            assert_eq!(engine.latest("alice", "case").unwrap(), Some(state));
            action.phase = EffectPhase::Attempted;
            let error = stale_writer
                .save_live_file_phase(
                    "alice",
                    &action,
                    old.revision,
                    Instant::now() + Duration::from_secs(30),
                )
                .unwrap_err();
            assert!(
                error.to_string().contains("EFFECT_GOAL_NOT_ACTIVE"),
                "{error:?}"
            );
            assert_eq!(fs::read(&fixture.target).unwrap(), bytes);
            assert_eq!(
                fs::metadata(&fixture.target).unwrap().modified().unwrap(),
                modified
            );

            let new_target = fixture.directory.join("fresh-file.txt");
            let mut fresh = RequestEnvelope::new("case", "fresh-write", "alice", "7+8");
            fresh.output_path = Some(new_target.clone());
            fresh.permissions.write_paths.insert(new_target.clone());
            let completed = engine.execute("alice", fresh).unwrap();
            assert_eq!(completed.lifecycle, Lifecycle::Commit);
            assert!(completed.text.starts_with("7+8 = 15。"));
            assert_eq!(fs::read(&new_target).unwrap(), completed.text.as_bytes());
        }
    }

    #[test]
    fn local_file_attempt_cannot_be_acquired_twice() {
        let fixture = sequence_file_preparation_prefix(2);
        let store = Store::open(&fixture.state).unwrap();
        let revision = store.load_case("alice", "case").unwrap().unwrap().revision;
        let mut action = store.actions("alice", "case").unwrap().remove(0);
        action.phase = EffectPhase::Attempted;
        let deadline = Instant::now() + Duration::from_secs(30);
        let mut misdirected = action.clone();
        misdirected.target = fixture
            .directory
            .join("wrong-target.txt")
            .to_str()
            .unwrap()
            .into();
        let error = store
            .save_live_file_phase("alice", &misdirected, revision, deadline)
            .unwrap_err();
        assert!(
            error.to_string().contains("EFFECT_GOAL_NOT_ACTIVE"),
            "{error:?}"
        );
        assert_eq!(
            store.actions("alice", "case").unwrap()[0].phase,
            EffectPhase::Prepared
        );
        store
            .save_live_file_phase("alice", &action, revision, deadline)
            .unwrap();
        let other_writer = Store::open(&fixture.state).unwrap();
        let error = other_writer
            .save_live_file_phase("alice", &action, revision, deadline)
            .unwrap_err();
        assert!(
            error.to_string().contains("EFFECT_ALREADY_ATTEMPTED"),
            "{error:?}"
        );
        let actions = other_writer.actions("alice", "case").unwrap();
        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0].phase, EffectPhase::Unknown);
        assert_eq!(actions[0].action_id, fixture.token);
    }

    #[test]
    fn local_file_deadline_stops_at_the_observed_effect_boundary() {
        use crate::core::OutputCheckpoint;

        for (checkpoint, phase, history_len) in [
            (OutputCheckpoint::Rendered, None, 0),
            (OutputCheckpoint::FilePhase(EffectPhase::Proposed), None, 0),
            (
                OutputCheckpoint::FilePhase(EffectPhase::Attempted),
                Some(EffectPhase::Cancelled),
                4,
            ),
            (OutputCheckpoint::FileDelivery, Some(EffectPhase::Failed), 5),
            (OutputCheckpoint::Commit, Some(EffectPhase::Confirmed), 5),
            (
                OutputCheckpoint::CommitPersistence,
                Some(EffectPhase::Confirmed),
                5,
            ),
        ] {
            let fixture = empty_fixture();
            let mut engine = Engine::open(&fixture.state).unwrap();
            engine.expire_output_at = Some(checkpoint);
            let mut request = RequestEnvelope::new("case", "deadline", "alice", "7+8");
            request.output_path = Some(fixture.target.clone());
            request
                .permissions
                .write_paths
                .insert(fixture.target.clone());
            let result = engine.execute("alice", request.clone()).unwrap();
            assert_eq!(result.lifecycle, Lifecycle::Suspend, "{checkpoint:?}");
            assert!(result.reason.as_deref().unwrap().starts_with("DEADLINE:"));
            assert!(result.text.starts_with("7+8 = 15。"));
            assert_eq!(
                result.render.as_ref().unwrap().bytes,
                result.text.as_bytes()
            );

            drop(engine);
            let mut restored = Engine::open(&fixture.state).unwrap();
            assert_eq!(
                restored.latest("alice", "case").unwrap().unwrap().result,
                Some(result.clone())
            );
            let actions = restored.store.actions("alice", "case").unwrap();
            assert_eq!(actions.first().map(|action| action.phase), phase);
            assert_eq!(actions.len(), usize::from(phase.is_some()));
            let journal = Connection::open(fixture.state.join("state.sqlite3")).unwrap();
            let events: i64 = journal
                .query_row("SELECT COUNT(*) FROM action_history", [], |row| row.get(0))
                .unwrap();
            assert_eq!(events, history_len, "{checkpoint:?}");
            let commits: i64 = journal
                .query_row(
                    "SELECT COUNT(*) FROM case_revision WHERE lifecycle='COMMIT'",
                    [],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(commits, 0, "{checkpoint:?}");
            match phase {
                Some(EffectPhase::Confirmed) => {
                    assert_eq!(fs::read(&fixture.target).unwrap(), result.text.as_bytes());
                    assert_eq!(result.receipts[0].phase, EffectPhase::Confirmed);
                    assert_eq!(result.receipts[0].action_id, actions[0].action_id);
                    assert!(result.execution_graph_ref.is_some());
                }
                Some(EffectPhase::Failed) => {
                    assert!(!fixture.target.exists());
                    assert_eq!(result.receipts[0].phase, EffectPhase::Failed);
                    assert_eq!(result.receipts[0].action_id, actions[0].action_id);
                    assert!(result.receipts[0].observed_bytes.is_none());
                    assert!(
                        result.receipts[0]
                            .reason
                            .as_deref()
                            .unwrap()
                            .starts_with("DEADLINE:")
                    );
                    assert!(result.execution_graph_ref.is_none());
                }
                _ => {
                    assert!(!fixture.target.exists());
                    assert!(result.receipts.is_empty());
                    assert!(actions.iter().all(|action| action.receipt.is_none()));
                    assert!(result.execution_graph_ref.is_none());
                }
            }
            assert_eq!(restored.execute("alice", request).unwrap(), result);
            assert_eq!(restored.store.actions("alice", "case").unwrap(), actions);
        }
    }

    #[test]
    fn cancelled_goal_restart_closes_preparation_left_before_cleanup() {
        let fixture = sequence_file_preparation_prefix(2);
        let store = Store::open(&fixture.state).unwrap();
        let mut state = store.load_case("alice", "case").unwrap().unwrap();
        state.parent_revision = Some(state.revision);
        state.revision += 1;
        state.lifecycle = Lifecycle::Cancel;
        state.result.as_mut().unwrap().lifecycle = Lifecycle::Cancel;
        store.append_case("alice", &state).unwrap();
        drop(store);
        let mut engine = Engine::open(&fixture.state).unwrap();
        let observed = engine.reconcile_cancelled_effects("alice", "case").unwrap();
        assert_eq!(observed.len(), 1);
        assert_eq!(observed[0].phase, EffectPhase::Cancelled);
        assert!(observed[0].receipt.is_none());
        assert_eq!(engine.latest("alice", "case").unwrap(), Some(state));
    }

    fn sequence_file_preparation_prefix(cut: i64) -> Fixture {
        let mut fixture = empty_fixture();
        let mut request = RequestEnvelope::new(
            "case",
            "sequence",
            "alice",
            "calculation-sequence:\nbase: 2+3\nfinal: @{base}*2",
        );
        request.output_path = Some(fixture.target.clone());
        request
            .permissions
            .write_paths
            .insert(fixture.target.clone());
        {
            let mut engine = Engine::open(&fixture.state).unwrap();
            let result = engine.execute("alice", request).unwrap();
            assert_eq!(result.lifecycle, Lifecycle::Commit, "{result:?}");
        }
        fixture.token = retain_attempted_prefix(&fixture.state);
        let database = Connection::open(fixture.state.join("state.sqlite3")).unwrap();
        database
            .execute("DELETE FROM action_history WHERE sequence>?1", [cut])
            .unwrap();
        fixture
    }

    #[test]
    fn authorized_cancel_stops_pending_goal_and_retains_unknown_effect_identity() {
        let fixture = attempted_prefix();
        let original = fs::read(&fixture.target).unwrap();
        let modified = fs::metadata(&fixture.target).unwrap().modified().unwrap();
        let mut engine = Engine::open(&fixture.state).unwrap();
        let cancel = RequestEnvelope::new("case", "cancel-input", "alice", "キャンセル");
        let result = engine.execute("alice", cancel).unwrap();
        assert_eq!(result.lifecycle, Lifecycle::Cancel);
        assert_eq!(result.input_id, "cancel-input");
        let latest = engine.latest("alice", "case").unwrap().unwrap();
        assert_eq!(latest.lifecycle, Lifecycle::Cancel);
        assert_eq!(
            latest.bundle.as_ref().unwrap().request.input_id,
            "cancel-input"
        );
        let original_cancel = latest
            .prior_results
            .values()
            .find(|prior| prior.input_id == "input")
            .expect("old result retained");
        assert_eq!(original_cancel.lifecycle, Lifecycle::Cancel);
        let store = Store::open(&fixture.state).unwrap();
        let actions = store.actions("alice", "case").unwrap();
        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0].idempotency_key, fixture.token);
        assert_eq!(actions[0].phase, EffectPhase::Unknown);
        assert_eq!(
            store
                .get_input_result("alice", "case", "input", 1)
                .unwrap()
                .unwrap()
                .lifecycle,
            Lifecycle::Cancel
        );
        assert_eq!(fs::read(&fixture.target).unwrap(), original);
        assert_eq!(
            fs::metadata(&fixture.target).unwrap().modified().unwrap(),
            modified
        );
        assert!(engine.reconcile("alice", "case").is_err());
    }

    #[test]
    fn unknown_original_blocks_new_effect_until_same_identity_reconciliation() {
        let fixture = attempted_prefix();
        let next_target = fixture.directory.join("new-effect.txt");
        let original = fs::read(&fixture.target).unwrap();
        let mut engine = Engine::open(&fixture.state).unwrap();
        let mut request = RequestEnvelope::new("case", "new-effect-input", "alice", "7+8");
        request.output_path = Some(next_target.clone());
        request.permissions.write_paths.insert(next_target.clone());
        let blocked = engine.execute("alice", request.clone()).unwrap();
        assert_eq!(blocked.lifecycle, Lifecycle::Suspend);
        assert!(!next_target.exists());
        assert_eq!(
            engine
                .latest("alice", "case")
                .unwrap()
                .unwrap()
                .bundle
                .unwrap()
                .request
                .input_id,
            "input"
        );
        {
            let store = Store::open(&fixture.state).unwrap();
            let actions = store.actions("alice", "case").unwrap();
            assert_eq!(actions.len(), 1);
            assert_eq!(actions[0].idempotency_key, fixture.token);
            assert_eq!(actions[0].phase, EffectPhase::Unknown);
        }
        let recovered = engine.reconcile("alice", "case").unwrap();
        assert_eq!(recovered.lifecycle, Lifecycle::Commit);
        assert_eq!(recovered.input_id, "input");
        assert!(!next_target.exists());
        assert_eq!(fs::read(&fixture.target).unwrap(), original);
        {
            let store = Store::open(&fixture.state).unwrap();
            let actions = store.actions("alice", "case").unwrap();
            assert_eq!(actions.len(), 1);
            assert_eq!(actions[0].idempotency_key, fixture.token);
            assert_eq!(actions[0].phase, EffectPhase::Confirmed);
        }
        // A previously rejected/interrupted identity is a tombstone. Explicit
        // new input version is necessary for a new later authorized effect.
        assert_eq!(
            engine.execute("alice", request.clone()).unwrap().lifecycle,
            Lifecycle::Suspend
        );
        assert!(!next_target.exists());
        request.input_version = 2;
        assert_eq!(
            engine.execute("alice", request).unwrap().lifecycle,
            Lifecycle::Commit
        );
        assert!(next_target.is_file());
        assert_eq!(fs::read(&fixture.target).unwrap(), original);
    }

    #[test]
    fn cancellation_from_suspended_unknown_case_preserves_observed_effects() {
        let fixture = attempted_prefix();
        // A mismatching local artifact makes explicit recovery suspend. A
        // later cancel must still stop future work without altering that file.
        fs::write(&fixture.target, b"actual changed artifact").unwrap();
        let mut engine = Engine::open(&fixture.state).unwrap();
        assert_eq!(
            engine.reconcile("alice", "case").unwrap().lifecycle,
            Lifecycle::Suspend
        );
        let cancel = RequestEnvelope::new("case", "cancel-suspended", "alice", "キャンセル");
        assert_eq!(
            engine.execute("alice", cancel).unwrap().lifecycle,
            Lifecycle::Cancel
        );
        let store = Store::open(&fixture.state).unwrap();
        let actions = store.actions("alice", "case").unwrap();
        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0].idempotency_key, fixture.token);
        assert_eq!(actions[0].phase, EffectPhase::Unknown);
        assert_eq!(
            fs::read(&fixture.target).unwrap(),
            b"actual changed artifact"
        );
    }
    #[test]
    fn cancelled_goal_stays_cancelled_when_original_file_effect_is_observed() {
        let fixture = attempted_prefix();
        let original = fs::read(&fixture.target).unwrap();
        let modified = fs::metadata(&fixture.target).unwrap().modified().unwrap();
        let mut engine = Engine::open(&fixture.state).unwrap();
        assert!(engine.reconcile_cancelled_effects("alice", "case").is_err());
        let cancel = RequestEnvelope::new("case", "cancel-bookkeeping", "alice", "キャンセル");
        assert_eq!(
            engine.execute("alice", cancel).unwrap().lifecycle,
            Lifecycle::Cancel
        );
        let before = engine.latest("alice", "case").unwrap().unwrap();
        let actions = engine.reconcile_cancelled_effects("alice", "case").unwrap();
        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0].phase, EffectPhase::Confirmed);
        assert_eq!(actions[0].idempotency_key, fixture.token);
        assert!(
            actions[0]
                .receipt
                .as_ref()
                .unwrap()
                .reason
                .as_deref()
                .unwrap()
                .contains("NO_RESEND")
        );
        assert_eq!(engine.latest("alice", "case").unwrap().unwrap(), before);
        assert_eq!(fs::read(&fixture.target).unwrap(), original);
        assert_eq!(
            fs::metadata(&fixture.target).unwrap().modified().unwrap(),
            modified
        );
        // Once the old physical outcome is known, fresh authorized work can
        // proceed; the canceled input remains a canceled historical result.
        let next = RequestEnvelope::new("case", "after-cancel-bookkeeping", "alice", "11+2");
        assert_eq!(
            engine.execute("alice", next).unwrap().lifecycle,
            Lifecycle::Commit
        );
        let store = Store::open(&fixture.state).unwrap();
        assert_eq!(
            store
                .get_input_result("alice", "case", "cancel-bookkeeping", 1)
                .unwrap()
                .unwrap()
                .lifecycle,
            Lifecycle::Cancel
        );
        assert_eq!(store.actions("alice", "case").unwrap().len(), 1);
    }

    #[test]
    fn cancelled_missing_effect_stays_unknown_and_new_cancel_permissions_do_not_apply() {
        let fixture = attempted_prefix();
        fs::remove_file(&fixture.target).unwrap();
        let other = fixture.directory.join("unrelated-permission.txt");
        let original_auth = Store::open(&fixture.state)
            .unwrap()
            .actions("alice", "case")
            .unwrap()[0]
            .authorization_sha256
            .clone();
        let mut engine = Engine::open(&fixture.state).unwrap();
        let mut cancel =
            RequestEnvelope::new("case", "cancel-with-other-scope", "alice", "キャンセル");
        cancel.output_path = Some(other.clone());
        cancel.permissions.write_paths.insert(other.clone());
        assert_eq!(
            engine.execute("alice", cancel).unwrap().lifecycle,
            Lifecycle::Cancel
        );
        let before = engine.latest("alice", "case").unwrap().unwrap();
        assert!(engine.reconcile_cancelled_effects("bob", "case").is_err());
        let actions = engine.reconcile_cancelled_effects("alice", "case").unwrap();
        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0].phase, EffectPhase::Unknown);
        assert_eq!(actions[0].idempotency_key, fixture.token);
        assert_eq!(actions[0].authorization_sha256, original_auth);
        assert!(!fixture.target.exists());
        assert!(!other.exists());
        assert_eq!(engine.latest("alice", "case").unwrap().unwrap(), before);
        let mut next = RequestEnvelope::new("case", "still-blocked-after-cancel", "alice", "7+8");
        next.output_path = Some(other.clone());
        next.permissions.write_paths.insert(other.clone());
        assert_eq!(
            engine.execute("alice", next).unwrap().lifecycle,
            Lifecycle::Suspend
        );
        assert!(!other.exists());
        assert_eq!(
            engine.latest("alice", "case").unwrap().unwrap().lifecycle,
            Lifecycle::Cancel
        );
    }
}
