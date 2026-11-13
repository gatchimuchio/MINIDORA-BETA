//! Core-owned reconciliation of a previously attempted local file effect.
//!
//! This module never writes target content, invokes a provider, creates an
//! action/token or reinterprets an input. It verifies the stored original goal
//! and artifacts, observes the existing exact target, then submits the resulting
//! receipt/revisions through the same Core-owned persistence boundary. Host
//! authentication and trusted ownership of ancestor directories remain required.
use crate::{
    core::validate_render,
    language,
    scheduler::{GoalNode, GoalNodeKind, Scheduler},
    store::Store,
};
use minidora_domain::*;
use std::{
    collections::BTreeMap,
    fs::{self, File},
    io::Read,
    path::{Component, Path},
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
    let (mut action, receipt, mut graph, render_ref) = match inspected {
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
    result.execution_graph_ref = Some(store.put_artifact(
        actor,
        case,
        &(
            graph.nodes(),
            graph.observed(),
            graph.consumed_by("content"),
            graph.consumed_by("render"),
            graph.consumed_by("file"),
        ),
    )?);
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

/// Reconcile already-issued local file effects after the goal was cancelled.
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

type Inspected = (ActionRecord, DeliveryReceipt, Scheduler, String);

struct SourceSummaryExpectation {
    source_packet_refs: Vec<(usize, String)>,
}

struct PreviousResultSummaryExpectation {
    content_ref: String,
    render_ref: String,
    summary_ref: String,
}

struct SourceQuestionExpectation {
    source_packet_refs: Vec<(usize, String)>,
    answer_ref: String,
}

struct PreviousTextEditExpectation {
    source_content_ref: String,
    source_render_ref: String,
    edit_ref: String,
}

type JsonGroupMeanRecovery = (String, String, String, String, Vec<u8>, Vec<String>);

fn recompute_source_question_artifact(
    operation: &str,
    source_result_id: &str,
    query: &str,
    packets: &[(usize, &EvidencePacket, String)],
) -> Result<(Vec<u8>, Vec<String>)> {
    match operation {
        "SOURCE_QUESTION_EXTRACTIVE_CANDIDATES_V1" => {
            let answer = crate::text_summary::answer_fetched_source_question_v1(
                source_result_id,
                query,
                packets,
            )?;
            Ok((
                canonical(&answer)?,
                crate::text_summary::source_question_answer_lines_v1(&answer),
            ))
        }
        "SOURCE_QUESTION_VERSIONED_EXTRACTIVE_CANDIDATES_V2" => {
            let answer = crate::text_summary::answer_fetched_source_question_v2(
                source_result_id,
                query,
                packets,
            )?;
            Ok((
                canonical(&answer)?,
                crate::text_summary::source_question_answer_lines_v2(&answer),
            ))
        }
        _ => Err(failure(
            "RECOVERY_SOURCE_QUESTION_VERSION",
            "the retained source-question artifact uses an unsupported derivation version",
        )),
    }
}

fn verify_search_candidate_context(
    store: &Store,
    actor: &str,
    case: &str,
    state: &CaseRevision,
    result: &ResultRecord,
    plan: &ContentPlan,
    goal: &GoalContract,
) -> Result<Option<u32>> {
    let Intent::FetchPreviousSearchCandidate { rank } = &goal.intent else {
        return Ok(None);
    };
    let rank = *rank;
    let bundle = state
        .bundle
        .as_ref()
        .ok_or_else(|| failure("RECOVERY_DATA_UNAVAILABLE", "sealed original unavailable"))?;
    let previous_id = bundle.meaning.context_result_ref.as_ref().ok_or_else(|| {
        failure(
            "RECOVERY_SEARCH_CONTEXT_MISMATCH",
            "candidate-body result has no sealed prior-search reference",
        )
    })?;
    let previous = state.prior_results.get(previous_id).ok_or_else(|| {
        failure(
            "RECOVERY_SEARCH_CONTEXT_MISMATCH",
            "the original search result is no longer retained",
        )
    })?;
    if previous.id != *previous_id || previous.lifecycle != Lifecycle::Commit {
        return Err(failure(
            "RECOVERY_SEARCH_CONTEXT_MISMATCH",
            "the referenced search result is not the same committed result",
        ));
    }
    let (search_ref, observation) =
        crate::search::previous_observation(store, actor, case, previous)?.ok_or_else(|| {
            failure(
                "RECOVERY_SEARCH_CONTEXT_MISMATCH",
                "the original live search report is no longer available unchanged",
            )
        })?;
    let candidate = observation
        .candidates
        .iter()
        .find(|candidate| candidate.rank == rank)
        .ok_or_else(|| {
            failure(
                "RECOVERY_SEARCH_CONTEXT_MISMATCH",
                "the selected candidate no longer appears at its original rank",
            )
        })?;
    if result.evidence.len() != 1 {
        return Err(failure(
            "RECOVERY_EVIDENCE_MISMATCH",
            "candidate-body result must retain exactly one fetched packet",
        ));
    }
    let packet = &result.evidence[0];
    let expected_uri = crate::search::candidate_body_uri(candidate)?;
    crate::ports::validate_successful_packet(packet, &goal.permissions)?;
    if packet.request_uri != expected_uri || packet.raw_body.len() > goal.budget.max_evidence_bytes
    {
        return Err(failure(
            "RECOVERY_EVIDENCE_MISMATCH",
            "the fetched packet differs from the selected candidate or original budget",
        ));
    }
    let packet_bytes = canonical(packet)?;
    let body_ref = digest(&packet_bytes);
    if store.get_bytes(actor, case, &body_ref)? != packet_bytes {
        return Err(failure(
            "RECOVERY_EVIDENCE_MISMATCH",
            "the original candidate body artifact is no longer present unchanged",
        ));
    }
    let expected_dependencies = vec![previous.id.clone(), search_ref, body_ref];
    if result.dependencies != expected_dependencies
        || plan.required_units != crate::search::candidate_body_lines(candidate, packet)
        || result.guarantee != Guarantee::SourceReport
    {
        return Err(failure(
            "RECOVERY_SEARCH_CONTEXT_MISMATCH",
            "the candidate, retrieved body or displayed source report differs from the adopted result",
        ));
    }
    Ok(Some(rank))
}

fn verify_source_summary_context(
    store: &Store,
    actor: &str,
    case: &str,
    state: &CaseRevision,
    result: &ResultRecord,
    plan: &ContentPlan,
    goal: &GoalContract,
) -> Result<Option<SourceSummaryExpectation>> {
    if !matches!(
        goal.intent,
        Intent::SummarizePreviousSource | Intent::SummarizePreviousSourceConstrained { .. }
    ) {
        return Ok(None);
    }
    let summary_limits = crate::text_summary::constrained_source_summary_limits(&goal.intent);
    let bundle = state
        .bundle
        .as_ref()
        .ok_or_else(|| failure("RECOVERY_DATA_UNAVAILABLE", "sealed original unavailable"))?;
    let prior_id = bundle.meaning.context_result_ref.as_ref().ok_or_else(|| {
        failure(
            "RECOVERY_SOURCE_CONTEXT_MISMATCH",
            "source summary has no sealed prior-result reference",
        )
    })?;
    let prior = state.prior_results.get(prior_id).ok_or_else(|| {
        failure(
            "RECOVERY_SOURCE_CONTEXT_MISMATCH",
            "the exact prior source result is not retained",
        )
    })?;
    if prior.id != *prior_id || prior.lifecycle != Lifecycle::Commit {
        return Err(failure(
            "RECOVERY_SOURCE_CONTEXT_MISMATCH",
            "the referenced prior result is not the same committed result",
        ));
    }
    let source_packets: Vec<_> = prior
        .evidence
        .iter()
        .enumerate()
        .filter(|(_, packet)| {
            packet.packet_kind == "FETCHED_BODY"
                && packet.failure.is_none()
                && packet.extracted_text.is_some()
        })
        .collect();
    if source_packets.is_empty() || !result.evidence.is_empty() {
        return Err(failure(
            "RECOVERY_SOURCE_CONTEXT_MISMATCH",
            "the prior result lacks fetched bodies or the derived result added undeclared evidence",
        ));
    }
    if summary_limits.is_some() && source_packets.len() != 1 {
        return Err(failure(
            "RECOVERY_SOURCE_CONTEXT_AMBIGUOUS",
            "the constrained source summary requires exactly one fetched body",
        ));
    }
    let mut dependencies = vec![prior.id.clone()];
    let mut packet_refs = Vec::new();
    let mut summary_refs = Vec::new();
    let mut source_packet_refs = Vec::new();
    let mut units = Vec::new();
    let mut report_bytes = 0usize;
    for (index, packet) in source_packets {
        let packet_bytes = canonical(packet)?;
        let packet_ref = digest(&packet_bytes);
        if store.get_bytes(actor, case, &packet_ref)? != packet_bytes {
            return Err(failure(
                "RECOVERY_SOURCE_PACKET_MISMATCH",
                "the original source packet blob differs from its retained record",
            ));
        }
        let report = match summary_limits {
            Some((max_sentences, max_chars)) => {
                crate::text_summary::summarize_fetched_source_with_limits(
                    &prior.id,
                    packet,
                    &packet_ref,
                    max_sentences,
                    max_chars,
                )?
            }
            None => crate::text_summary::summarize_fetched_source(&prior.id, packet, &packet_ref)?,
        };
        let encoded = canonical(&report)?;
        report_bytes = report_bytes.saturating_add(encoded.len());
        if report_bytes > goal.budget.max_evidence_bytes {
            return Err(failure(
                "RECOVERY_SOURCE_SUMMARY_LIMIT",
                "the retained source summary exceeds the original evidence budget",
            ));
        }
        let report_ref = digest(&encoded);
        if store.get_bytes(actor, case, &report_ref)? != encoded {
            return Err(failure(
                "RECOVERY_SOURCE_SUMMARY_MISMATCH",
                "the retained source summary artifact differs from the exact source packet",
            ));
        }
        packet_refs.push(packet_ref.clone());
        summary_refs.push(report_ref);
        source_packet_refs.push((index, packet_ref));
        units.push(format!(
            "出典: {}\n出典packet: {}\n取得本文SHA-256: {}\nライセンス情報: 未記録。\n{}",
            report.source_final_uri,
            report.source_packet_id,
            report.raw_body_sha256,
            report.summary.text
        ));
    }
    dependencies.extend(packet_refs);
    dependencies.extend(summary_refs);
    if dependencies != result.dependencies
        || units != plan.required_units
        || result.guarantee != Guarantee::Hypothesis
    {
        return Err(failure(
            "RECOVERY_SOURCE_SUMMARY_MISMATCH",
            "source dependencies or displayed candidate units differ from Core's retained derivation",
        ));
    }
    Ok(Some(SourceSummaryExpectation { source_packet_refs }))
}

fn verify_previous_result_summary_context(
    store: &Store,
    actor: &str,
    case: &str,
    state: &CaseRevision,
    result: &ResultRecord,
    plan: &ContentPlan,
    goal: &GoalContract,
) -> Result<Option<PreviousResultSummaryExpectation>> {
    let Intent::SummarizePreviousResult {
        max_sentences,
        max_chars,
    } = &goal.intent
    else {
        return Ok(None);
    };
    let bundle = state
        .bundle
        .as_ref()
        .ok_or_else(|| failure("RECOVERY_DATA_UNAVAILABLE", "sealed original unavailable"))?;
    let source_result_id = bundle
        .meaning
        .context_result_ref
        .as_deref()
        .ok_or_else(|| {
            failure(
                "RECOVERY_RESULT_SUMMARY_CONTEXT_MISMATCH",
                "summary has no sealed prior-result reference",
            )
        })?;
    let source = state.prior_results.get(source_result_id).ok_or_else(|| {
        failure(
            "RECOVERY_RESULT_SUMMARY_CONTEXT_MISMATCH",
            "exact source result is not retained",
        )
    })?;
    if source.id != source_result_id || source.lifecycle != Lifecycle::Commit {
        return Err(failure(
            "RECOVERY_RESULT_SUMMARY_CONTEXT_MISMATCH",
            "summary source is not the same committed prior result",
        ));
    }
    let (Some(source_plan), Some(source_render)) = (&source.content_plan, &source.render) else {
        return Err(failure(
            "RECOVERY_RESULT_SUMMARY_SOURCE_UNAVAILABLE",
            "source result has no retained content plan or render",
        ));
    };
    let mut unsigned_plan = source_plan.clone();
    unsigned_plan.id.clear();
    if source_plan.id != identity(&unsigned_plan)?
        || source.text != source_plan.text
        || !crate::core::validate_render(source_plan, source_render)
    {
        return Err(failure(
            "RECOVERY_RESULT_SUMMARY_SOURCE_INTEGRITY",
            "the retained source result does not match its content plan and render",
        ));
    }
    let content_bytes = canonical(source_plan)?;
    let content_ref = digest(&content_bytes);
    let render_bytes = canonical(source_render)?;
    let render_ref = digest(&render_bytes);
    if store.get_bytes(actor, case, &content_ref)? != content_bytes
        || store.get_bytes(actor, case, &render_ref)? != render_bytes
    {
        return Err(failure(
            "RECOVERY_RESULT_SUMMARY_SOURCE_INTEGRITY",
            "the source content or render artifact differs from the retained result",
        ));
    }
    let artifact = crate::text_summary::summarize_previous_result_with_limits(
        &result.id,
        &source.id,
        &source_plan.id,
        &source_render.actual_sha256,
        &source.text,
        max_sentences.unwrap_or(3),
        *max_chars,
    )?;
    if artifact.summary.sentence_count > goal.budget.max_actions as usize {
        return Err(failure(
            "RECOVERY_RESULT_SUMMARY_LIMIT",
            "the source sentence scan exceeds the original action budget",
        ));
    }
    let output_unit =
        crate::text_summary::previous_result_summary_unit(&source.id, &artifact.summary);
    let artifact_bytes = canonical(&artifact)?;
    if artifact_bytes.len().saturating_add(output_unit.len()) > goal.budget.max_evidence_bytes {
        return Err(failure(
            "RECOVERY_RESULT_SUMMARY_LIMIT",
            "the retained summary exceeds the original evidence budget",
        ));
    }
    let summary_ref = digest(&artifact_bytes);
    if store.get_bytes(actor, case, &summary_ref)? != artifact_bytes {
        return Err(failure(
            "RECOVERY_RESULT_SUMMARY_MISMATCH",
            "the retained summary artifact differs from the original result text",
        ));
    }
    let mut expected_dependencies = vec![
        summary_ref.clone(),
        source.id.clone(),
        content_ref.clone(),
        render_ref.clone(),
    ];
    expected_dependencies.extend(source.dependencies.iter().cloned());
    if result.id != artifact.summary_result_id
        || result.guarantee != Guarantee::Hypothesis
        || !result.evidence.is_empty()
        || result.dependencies != expected_dependencies
        || plan.required_units != [output_unit]
    {
        return Err(failure(
            "RECOVERY_RESULT_SUMMARY_MISMATCH",
            "the summary result, source dependencies, or displayed unit differs from the recomputed report",
        ));
    }
    Ok(Some(PreviousResultSummaryExpectation {
        content_ref,
        render_ref,
        summary_ref,
    }))
}

fn verify_source_question_context(
    store: &Store,
    actor: &str,
    case: &str,
    state: &CaseRevision,
    result: &ResultRecord,
    plan: &ContentPlan,
    goal: &GoalContract,
) -> Result<Option<SourceQuestionExpectation>> {
    let Intent::AskPreviousSourceQuestion { query } = &goal.intent else {
        return Ok(None);
    };
    let bundle = state
        .bundle
        .as_ref()
        .ok_or_else(|| failure("RECOVERY_DATA_UNAVAILABLE", "sealed original unavailable"))?;
    let prior_id = bundle.meaning.context_result_ref.as_ref().ok_or_else(|| {
        failure(
            "RECOVERY_SOURCE_QUESTION_MISMATCH",
            "source question has no sealed prior-result reference",
        )
    })?;
    let prior = state.prior_results.get(prior_id).ok_or_else(|| {
        failure(
            "RECOVERY_SOURCE_QUESTION_MISMATCH",
            "the exact prior source result is not retained",
        )
    })?;
    if prior.id != *prior_id || prior.lifecycle != Lifecycle::Commit {
        return Err(failure(
            "RECOVERY_SOURCE_QUESTION_MISMATCH",
            "the referenced prior source result is not the same committed result",
        ));
    }
    let mut packets = Vec::new();
    let mut source_packet_refs = Vec::new();
    let mut dependencies = vec![prior.id.clone()];
    for (index, packet) in prior.evidence.iter().enumerate().filter(|(_, packet)| {
        packet.packet_kind == "FETCHED_BODY"
            && packet.failure.is_none()
            && packet.extracted_text.is_some()
    }) {
        let encoded = canonical(packet)?;
        let packet_ref = digest(&encoded);
        if store.get_bytes(actor, case, &packet_ref)? != encoded {
            return Err(failure(
                "RECOVERY_SOURCE_QUESTION_PACKET_MISMATCH",
                "the original fetched-body packet differs from its retained artifact",
            ));
        }
        packets.push((index, packet, packet_ref.clone()));
        source_packet_refs.push((index, packet_ref.clone()));
        dependencies.push(packet_ref);
    }
    let answer_ref = result.dependencies.last().ok_or_else(|| {
        failure(
            "RECOVERY_SOURCE_QUESTION_MISMATCH",
            "the committed source question has no answer artifact reference",
        )
    })?;
    let stored_answer = store.get_bytes(actor, case, answer_ref)?;
    if digest(&stored_answer) != *answer_ref {
        return Err(failure(
            "RECOVERY_SOURCE_QUESTION_MISMATCH",
            "the retained source-question answer artifact digest is invalid",
        ));
    }
    let answer_record: serde_json::Value = serde_json::from_slice(&stored_answer)?;
    let operation = answer_record
        .get("operation")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| {
            failure(
                "RECOVERY_SOURCE_QUESTION_VERSION",
                "the retained source-question artifact has no derivation version",
            )
        })?;
    let (encoded, expected_units) =
        recompute_source_question_artifact(operation, &prior.id, query, &packets)?;
    if digest(&encoded) != *answer_ref || stored_answer != encoded {
        return Err(failure(
            "RECOVERY_SOURCE_QUESTION_MISMATCH",
            "the retained answer artifact differs from its versioned source-derived reconstruction",
        ));
    }
    let output_bytes = expected_units.join("\n").len();
    if encoded.len() > goal.budget.max_evidence_bytes
        || output_bytes > goal.budget.max_evidence_bytes
    {
        return Err(failure(
            "RECOVERY_SOURCE_QUESTION_MISMATCH",
            "the retained answer artifact or output exceeds its exact source-derived identity",
        ));
    }
    dependencies.push(answer_ref.clone());
    if !result.evidence.is_empty()
        || dependencies != result.dependencies
        || expected_units != plan.required_units
        || result.guarantee != Guarantee::Hypothesis
    {
        return Err(failure(
            "RECOVERY_SOURCE_QUESTION_MISMATCH",
            "source packet dependencies, answer candidates or guarantee differ from the adopted result",
        ));
    }
    Ok(Some(SourceQuestionExpectation {
        source_packet_refs,
        answer_ref: answer_ref.clone(),
    }))
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
    let expected_predicates = vec![
        "SUPPORTED_CONTENT".to_string(),
        "EXACT_RENDER".into(),
        "CONFIRMED_FILE".into(),
    ];
    if goal.required_predicates != expected_predicates {
        return Err(failure(
            "UNSUPPORTED_RECOVERY_GOAL",
            "unrecognized completion predicates remain unresolved",
        ));
    }
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
                    | Intent::SummarizePreviousResult { .. }
                    | Intent::AskPreviousSourceQuestion { .. }
                    | Intent::DraftRelationProfile { .. }
            ))
        || !validate_render(plan, render)
    {
        return Err(failure(
            "RECOVERY_CONTENT_MISMATCH",
            "retained content identity/meaning/render is unverified",
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
    let source_summary =
        verify_source_summary_context(store, actor, case, state, result, plan, &goal)?;
    let previous_result_summary =
        verify_previous_result_summary_context(store, actor, case, state, result, plan, &goal)?;
    let source_question =
        verify_source_question_context(store, actor, case, state, result, plan, &goal)?;
    let previous_text_edit =
        verify_previous_text_edit_context(store, actor, case, state, result, plan, &goal)?;
    let search_candidate_rank =
        verify_search_candidate_context(store, actor, case, state, result, plan, &goal)?;
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
    let previous_json_table_transform_explanation =
        if matches!(goal.intent, Intent::ExplainPrevious) {
            bundle
                .meaning
                .context_result_ref
                .as_ref()
                .and_then(|id| state.prior_results.get(id))
                .map(|previous| {
                    crate::document::previous_json_table_transform_explanation(
                        store,
                        actor,
                        case,
                        previous,
                        goal.budget.max_evidence_bytes,
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
    if let Intent::Ask { proposition } | Intent::AskWhy { proposition } = &goal.intent {
        // A case in another client may have invalidated a premise while this
        // output was pending. Revalidate the same proof lineage; never adopt a
        // newly different answer during mere effect reconciliation.
        let memory = store.load_memory(actor)?;
        let answer = crate::relations::query(&memory, proposition, goal.budget.max_actions)?;
        let current_proof_contract = matches!(
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
            crate::relations::answer_dependencies(&answer)
        } else {
            // Preserve recovery for records produced before the v46 lineage
            // and guarantee contract.
            let mut legacy: Vec<String> = answer.facts.iter().map(|fact| fact.id.clone()).collect();
            legacy.extend(answer.used_rules.iter().cloned());
            legacy
        };
        let guarantee_matches = !current_proof_contract
            || result.guarantee == crate::relations::answer_guarantee(&answer);
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
        // old bytes cannot be confirmed as the current goal's result.
        let memory = store.load_memory(actor)?;
        let answer = crate::core::relation_objects_answer(
            &memory,
            subject,
            predicate,
            *negative,
            scope,
            goal.budget.max_actions,
            goal.budget.max_evidence_bytes,
        )
        .map_err(|error| {
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
    }
    if let Intent::InspectPremises { subject } = &goal.intent {
        let memory = store.load_memory(actor)?;
        let current_inventory = if matches!(
            bundle.meaning.parser_version.as_str(),
            "declarative-ja-en-v68-previous-result-summary"
                | "declarative-ja-en-v69-relation-profile-draft"
                | "declarative-ja-en-v70-generic-transitive-verbs"
        ) {
            crate::relations::inventory_dependencies_and_guarantee(
                &memory,
                subject,
                goal.budget.max_actions,
            )
            .ok()
        } else {
            let dependencies = crate::relations::explicit_premises_about(&memory, subject)
                .into_iter()
                .map(|fact| fact.id)
                .collect();
            Some((dependencies, Guarantee::UserPremise))
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
    if matches!(
        bundle.meaning.parser_version.as_str(),
        "declarative-ja-en-v48-related-context-explanation"
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
            | "declarative-ja-en-v70-generic-transitive-verbs"
    ) && matches!(goal.intent, Intent::ExplainPrevious)
        && let Some(previous_id) = bundle.meaning.context_result_ref.as_ref()
        && let Some(previous) = state.prior_results.get(previous_id)
        && let Some(proposition) = crate::core::previous_relation_answer(&previous.text)?
    {
        let memory = store.load_memory(actor)?;
        let answer = crate::relations::query(&memory, &proposition, goal.budget.max_actions)?;
        let mut dependencies = vec![previous.id.clone()];
        dependencies.extend(crate::relations::answer_dependencies(&answer));
        if previous.lifecycle != Lifecycle::Commit
            || answer.status != "SUPPORTED"
            || dependencies != result.dependencies
            || previous.guarantee != crate::relations::answer_guarantee(&answer)
            || result.guarantee != crate::relations::answer_guarantee(&answer)
        {
            return Err(failure(
                "RECOVERY_CONTEXT_PROOF_CHANGED",
                "the relation proof behind the previous-answer explanation changed",
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
        &previous_json_table_transform_explanation
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
    let model_bytes = store.get_bytes(actor, case, &plan.model_artifact_ref)?;
    let model: language::SequenceModel = serde_json::from_slice(&model_bytes)?;
    model.validate()?;
    let emission_bytes = store.get_bytes(actor, case, &plan.model_observation_ref)?;
    let emission: language::ModelEmission = serde_json::from_slice(&emission_bytes)?;
    let authorized_surfaces = match &emission.authorized_surfaces {
        Some(surfaces) => {
            let expected_surfaces = match (&goal.intent, result.calculation.as_ref()) {
                (Intent::ExplainPrevious, Some(calculation)) => {
                    Some(language::calculation_explanation_surfaces(calculation))
                }
                (Intent::AskWhy { proposition }, None) => {
                    let memory = store.load_memory(actor)?;
                    let answer =
                        crate::relations::query(&memory, proposition, goal.budget.max_actions)?;
                    let explanation_units = crate::core::relation_why_explanation_units(
                        &memory,
                        proposition,
                        &answer,
                    )
                    .map_err(|_| {
                        failure(
                            "RECOVERY_MODEL_CANDIDATE_MISMATCH",
                            "the current relation proof has no matching explanation surfaces",
                        )
                    })?;
                    if crate::relations::answer_dependencies(&answer) != result.dependencies
                        || crate::relations::answer_guarantee(&answer) != result.guarantee
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
                    let memory = store.load_memory(actor)?;
                    let inventory = crate::relations::inventory_about(
                        &memory,
                        subject,
                        goal.budget.max_actions,
                    )
                    .map_err(|_| {
                        failure(
                            "RECOVERY_MODEL_CANDIDATE_MISMATCH",
                            "the current actor-scoped relation inventory is unavailable",
                        )
                    })?;
                    let profile_units = crate::core::relation_profile_units(subject, &inventory);
                    if inventory.dependencies != result.dependencies
                        || result.guarantee != Guarantee::Hypothesis
                        || profile_units != plan.required_units
                    {
                        return Err(failure(
                            "RECOVERY_MODEL_CANDIDATE_MISMATCH",
                            "the current relation inventory differs from the adopted profile draft",
                        ));
                    }
                    Some(language::relation_profile_surfaces(&profile_units))
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
    let nodes: Vec<GoalNode> =
        serde_json::from_slice(&store.get_bytes(actor, case, &goal.required_graph_ref)?)?;
    let mut expected_nodes = Vec::new();
    let mut sources = Vec::new();
    if let Intent::Retrieve { urls, .. } = &goal.intent {
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
                requires: Vec::new(),
            });
        }
    } else if let Some(rank) = search_candidate_rank {
        let id = format!("search-candidate-body-{rank}");
        sources.push(id.clone());
        expected_nodes.push(GoalNode {
            id,
            kind: GoalNodeKind::Evidence,
            requires: Vec::new(),
        });
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
    if previous_json_table_transform_explanation.is_some() {
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
    let mut graph = Scheduler::new(nodes, goal.budget.max_actions)?;
    let mut source_refs = BTreeMap::new();
    for (index, packet) in result.evidence.iter().enumerate() {
        let reference = verified_artifact(store, actor, case, packet)?;
        let id = search_candidate_rank.map_or_else(
            || format!("source-{index}"),
            |rank| format!("search-candidate-body-{rank}"),
        );
        graph.observe(&id, &reference, &BTreeMap::new())?;
        source_refs.insert(id, reference);
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
    if let Some((bundle_ref, report_ref, _)) = &previous_json_table_transform_explanation {
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
    let compute = (
        &result.calculation,
        &plan.required_units,
        &result.evidence,
        &result.dependencies,
    );
    let compute_ref = verified_artifact(store, actor, case, &compute)?;
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
    Ok((action, receipt, graph, render_ref))
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
    if !path.is_absolute()
        || path
            .components()
            .any(|p| matches!(p, Component::ParentDir | Component::CurDir))
        || path.file_name().is_none()
        || path.to_string_lossy().contains('\\')
        || path
            .to_string_lossy()
            .split('/')
            .any(|segment| segment == "." || segment == "..")
    {
        return Err(failure(
            "UNSAFE_RECONCILIATION_TARGET",
            "original target is not an unambiguous absolute path",
        ));
    }
    for ancestor in path.ancestors() {
        let metadata = fs::symlink_metadata(ancestor)?;
        if metadata.file_type().is_symlink()
            || (ancestor != path && !metadata.is_dir())
            || (ancestor == path && !metadata.is_file())
        {
            return Err(failure(
                "UNSAFE_RECONCILIATION_TARGET",
                "symlink or non-regular original target",
            ));
        }
    }
    let parent = path.parent().ok_or_else(|| {
        failure(
            "UNSAFE_RECONCILIATION_TARGET",
            "original parent unavailable",
        )
    })?;
    let parent_identity = parent.canonicalize()?;
    let mut file = File::open(path)?;
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
    File::open(parent)?.sync_all()?;
    Ok(())
}

fn advance(
    store: &Store,
    state: &mut CaseRevision,
    result: &ResultRecord,
    next: Lifecycle,
) -> Result<()> {
    let mut revision = state.clone();
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
        };
        let packet_ref = digest(&canonical(&packet).unwrap());
        let packets = [(0, &packet, packet_ref)];
        for operation in [
            "SOURCE_QUESTION_EXTRACTIVE_CANDIDATES_V1",
            "SOURCE_QUESTION_VERSIONED_EXTRACTIVE_CANDIDATES_V2",
        ] {
            let (bytes, lines) = recompute_source_question_artifact(
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
        let (replayed_bytes, replayed_lines) = recompute_source_question_artifact(
            "SOURCE_QUESTION_EXTRACTIVE_CANDIDATES_V1",
            &"a".repeat(64),
            "photosynthesis",
            &packets,
        )
        .unwrap();
        assert_eq!(replayed_bytes, expected_bytes);
        assert_eq!(replayed_lines, expected_lines);
        assert!(
            recompute_source_question_artifact(
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
            assert_eq!(
                engine.execute("alice", request).unwrap().lifecycle,
                Lifecycle::Commit
            );
        }
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
        let result_sha = digest(&canonical(revision.result.as_ref().unwrap()).unwrap());
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
                "UPDATE input_identity SET result_sha256=?1",
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

    fn attempted_statistics_explanation_prefix() -> Fixture {
        attempted_math_report_explanation_prefix(
            "statistics",
            "統計 [0.1,0.2,0.3]",
            "statistics-explanation.txt",
        )
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

    fn attempted_json_table_context_explanation_prefix(operation: &str) -> Fixture {
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
    fn json_table_operation_explanation_reconciles_without_resending() {
        for (operation, expected_marker) in [
            ("filter", "前回のJSON表フィルター"),
            ("inner_join", "前回のJSON表内部結合"),
        ] {
            let fixture = attempted_json_table_context_explanation_prefix(operation);
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
        request.output_path = Some(fixture.target.clone());
        request
            .permissions
            .write_paths
            .insert(fixture.target.clone());
        let result = engine.execute("alice", request).unwrap();
        assert_eq!(result.lifecycle, Lifecycle::Commit);
        assert_eq!(result.guarantee, Guarantee::UserPremise);
        assert_eq!(result.dependencies, vec![counterexample_id]);
        assert!(result.text.contains("ココは動物ではない"));
        let revision = engine.latest("alice", "case").unwrap().unwrap();
        assert!(inspect(&engine.store, "alice", &revision, &result).is_ok());
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
        request.output_path = Some(fixture.target.clone());
        request
            .permissions
            .write_paths
            .insert(fixture.target.clone());
        let answer = engine.execute("alice", request).unwrap();
        assert_eq!(answer.lifecycle, Lifecycle::Commit);
        assert!(answer.dependencies.contains(&premise_id));
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
