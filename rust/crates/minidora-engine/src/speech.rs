//! Bind WAV delivery to the exact text of an adopted answer in the same case.
use crate::document::revalidate_previous_document_render;
use crate::input::verify_speech_input;
use crate::output::{
    WAVE_PCM_DISPLAY_RENDERER_VERSION, WAVE_PCM_RENDERER_VERSION, WaveFacts, inspect_wave,
    matches_render_target, valid_wave_path, validate_wave_render,
};
use crate::store::Store;
use minidora_domain::{
    CaseRevision, ContentPlan, Guarantee, InputBundle, Lifecycle, RenderObservation, Result,
    ResultRecord, canonical, digest, failure, identity,
};
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::time::{Duration, Instant};

pub(crate) const PREDICATE: &str = "SPEECH_OF_ADOPTED_ANSWER_V1";
pub(crate) const SOURCE_NODE: &str = "speech-source-answer";
const REPORT_KIND: &str = "minidora.speech-delivery.v1";
const CONTEXT_ERROR: &str = "SPEECH_SOURCE_INVALID";

pub(crate) struct SpeechOutcome {
    pub text: String,
    pub guarantee: Guarantee,
    pub source_render_ref: String,
    pub wave: Vec<u8>,
    pub dependencies: Vec<String>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SpeechReport {
    kind: String,
    source_result_id: String,
    source_render_ref: String,
    transcript_sha256: String,
    voice_id: String,
    wave_sha256: String,
    wave_bytes: usize,
    facts: WaveFacts,
}

pub(crate) fn check_target(path: Option<&Path>) -> Result<()> {
    if path.is_none_or(valid_wave_path) {
        return Ok(());
    }
    Err(failure(
        "SPEECH_WAVE_TARGET_REQUIRED",
        "保存を許可したWAVの絶対パスを /output で選んでください。",
    ))
}

pub(crate) fn renderer_version(path: Option<&Path>) -> &'static str {
    if path.is_some() {
        WAVE_PCM_RENDERER_VERSION
    } else {
        WAVE_PCM_DISPLAY_RENDERER_VERSION
    }
}

fn invalid_source() -> minidora_domain::Error {
    failure(
        CONTEXT_ERROR,
        "同じ会話に保存された元回答の日本語本文を照合できません。",
    )
}

fn source<'a>(
    store: &Store,
    state: &'a CaseRevision,
    bundle: &InputBundle,
) -> Result<(&'a ResultRecord, String)> {
    verify_speech_input(bundle)?;
    if bundle.request.actor_id != state.actor_id || bundle.request.case_id != state.case_id {
        return Err(invalid_source());
    }
    let previous = bundle
        .meaning
        .context_result_ref
        .as_ref()
        .and_then(|id| {
            state
                .prior_results
                .get(id)
                .filter(|result| &result.id == id)
        })
        .ok_or_else(invalid_source)?;
    if previous.lifecycle != Lifecycle::Commit
        || previous.text.trim().is_empty()
        || identity(&Some(previous))? != bundle.context_sha256
        || previous
            .content_plan
            .as_ref()
            .is_none_or(|plan| !matches!(plan.locale.as_str(), "ja" | "ja-JP"))
    {
        return Err(invalid_source());
    }
    let actor = &state.actor_id;
    let case = &state.case_id;
    let (_, prior_input) = store
        .get_input_bundle(actor, case, &previous.input_id, previous.input_version)?
        .ok_or_else(invalid_source)?;
    if identity(&prior_input.request)? != previous.id
        || store
            .get_input_result(actor, case, &previous.input_id, previous.input_version)?
            .as_ref()
            != Some(previous)
    {
        return Err(invalid_source());
    }
    let render_ref = revalidate_previous_document_render(
        store,
        actor,
        case,
        previous,
        None,
        CONTEXT_ERROR,
        "speech source",
    )?;
    Ok((previous, render_ref))
}

pub(crate) fn produce(
    store: &Store,
    state: &CaseRevision,
    bundle: &InputBundle,
    started: Instant,
) -> Result<SpeechOutcome> {
    check_target(bundle.request.output_path.as_deref())?;
    let (previous, source_render_ref) = source(store, state, bundle)?;
    let budget = &bundle.request.budget;
    let deadline = started
        .checked_add(Duration::from_millis(budget.deadline_ms))
        .ok_or_else(|| failure("RESOURCE_LIMIT", "音声処理の期限を確定できません。"))?;
    let synthesis = crate::tts_japanese::synthesize(&previous.text, deadline)?;
    let report = SpeechReport {
        kind: REPORT_KIND.into(),
        source_result_id: previous.id.clone(),
        source_render_ref: source_render_ref.clone(),
        transcript_sha256: digest(previous.text.as_bytes()),
        voice_id: synthesis.voice_id,
        wave_sha256: digest(&synthesis.bytes),
        wave_bytes: synthesis.bytes.len(),
        facts: inspect_wave(&synthesis.bytes)?,
    };
    let report_bytes = canonical(&report)?;
    if synthesis
        .bytes
        .len()
        .saturating_add(report_bytes.len())
        .saturating_add(previous.text.len())
        > budget.max_evidence_bytes
        || Instant::now() >= deadline
    {
        return Err(failure(
            "RESOURCE_LIMIT",
            "音声が依頼の時間または根拠の上限を超えました。",
        ));
    }
    let actor = &state.actor_id;
    let case = &state.case_id;
    let wave_ref = store.put_bytes(actor, case, &synthesis.bytes)?;
    let report_ref = store.put_bytes(actor, case, &report_bytes)?;
    if wave_ref != report.wave_sha256 {
        return Err(failure(
            "INTEGRITY_FAILURE",
            "音声の計算値と保存ハッシュが一致しません。",
        ));
    }
    Ok(SpeechOutcome {
        text: previous.text.clone(),
        guarantee: previous.guarantee.clone(),
        source_render_ref: source_render_ref.clone(),
        wave: synthesis.bytes,
        dependencies: vec![previous.id.clone(), source_render_ref, wave_ref, report_ref],
    })
}

pub(crate) fn verify_delivery(
    store: &Store,
    state: &CaseRevision,
    bundle: &InputBundle,
    result: &ResultRecord,
    plan: &ContentPlan,
    render: &RenderObservation,
) -> Result<String> {
    let invalid = || {
        failure(
            "RECOVERY_SPEECH_MISMATCH",
            "保存した音声と元回答を照合できません。",
        )
    };
    let output_path = bundle.request.output_path.as_deref();
    check_target(output_path)?;
    let (previous, source_render_ref) = source(store, state, bundle)?;
    let [prior_id, prior_render, wave_ref, report_ref] = result.dependencies.as_slice() else {
        return Err(invalid());
    };
    if prior_id != &previous.id
        || prior_render != &source_render_ref
        || result.text != previous.text
        || plan.text != previous.text
        || plan.required_units != [previous.text.as_str()]
        || result.guarantee != previous.guarantee
        || plan.guarantee != result.guarantee
        || !matches_render_target(output_path, render.target.as_ref())
        || render.renderer_version != renderer_version(output_path)
        || !validate_wave_render(plan, render)
        || wave_ref != &render.actual_sha256
    {
        return Err(invalid());
    }
    let actor = &state.actor_id;
    let case = &state.case_id;
    let report_bytes = store.get_bytes(actor, case, report_ref)?;
    let report: SpeechReport = serde_json::from_slice(&report_bytes).map_err(|_| invalid())?;
    if report.kind != REPORT_KIND
        || report.source_result_id != *prior_id
        || report.source_render_ref != *prior_render
        || report.transcript_sha256 != digest(previous.text.as_bytes())
        || report.voice_id != crate::tts_japanese::VOICE_ID
        || report.wave_sha256 != *wave_ref
        || report.wave_bytes != render.bytes.len()
        || report.facts != inspect_wave(&render.bytes)?
        || render
            .bytes
            .len()
            .saturating_add(report_bytes.len())
            .saturating_add(previous.text.len())
            > bundle.request.budget.max_evidence_bytes
        || canonical(&report)? != report_bytes
        || store.get_bytes(actor, case, wave_ref)? != render.bytes
    {
        return Err(invalid());
    }
    Ok(source_render_ref)
}
