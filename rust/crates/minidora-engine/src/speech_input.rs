//! Admit original audio and bind the recognized utterance before semantic sealing.
use crate::{asr_julius, durability::snapshot_local_wave, output::inspect_wave_pcm, store::Store};
use minidora_domain::{RequestEnvelope, Result, SpeechInput, digest, failure};
use serde::Serialize;
use std::{
    borrow::Cow,
    path::Path,
    time::{Duration, Instant},
};

const SAMPLE_RATE: u32 = 16_000;
const MAX_WAVE_BYTES: usize = 2 * SAMPLE_RATE as usize * 10 + 4_096;
const MAX_TEXT_BYTES: usize = 2_048;

pub(crate) enum WaveSource<'a> {
    File(&'a Path),
    Bytes(&'a [u8]),
}

/// The utterance and provenance returned by Core, without the source WAV bytes.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct RecognizedWave {
    pub text: String,
    pub recognizer: String,
    pub wave_sha256: String,
    pub sample_rate: u32,
    pub start_frame: usize,
    pub end_frame: usize,
}

impl RecognizedWave {
    pub(crate) fn from_bound_request(request: &RequestEnvelope) -> Result<Self> {
        let source = request.speech.as_ref().ok_or_else(invalid_source)?;
        Ok(Self {
            text: request.text.clone(),
            recognizer: source.recognizer.clone(),
            wave_sha256: source.wave_sha256.clone(),
            sample_rate: source.sample_rate,
            start_frame: source.start_frame,
            end_frame: source.end_frame,
        })
    }
}

fn invalid_source() -> minidora_domain::Error {
    failure(
        "SPEECH_INPUT_INVALID",
        "元の音声・区間・認識した発話を照合できません。",
    )
}

fn verified_pcm(wave: &[u8]) -> Result<&[u8]> {
    let (facts, pcm) = inspect_wave_pcm(wave)?;
    if facts.sample_rate != SAMPLE_RATE || facts.frame_count > SAMPLE_RATE as usize * 10 {
        return Err(failure(
            "SPEECH_INPUT_FORMAT",
            "16 kHz・16 bit・モノラルで10秒以内のPCM WAVを指定してください。",
        ));
    }
    Ok(pcm)
}

fn normalize_wave(pcm: &[u8]) -> Vec<u8> {
    let data_len = pcm.len() as u32;
    let mut wave = Vec::with_capacity(44 + pcm.len());
    wave.extend_from_slice(b"RIFF");
    wave.extend_from_slice(&(36 + data_len).to_le_bytes());
    wave.extend_from_slice(b"WAVEfmt ");
    wave.extend_from_slice(&16u32.to_le_bytes());
    wave.extend_from_slice(&1u16.to_le_bytes()); // PCM
    wave.extend_from_slice(&1u16.to_le_bytes()); // Mono
    wave.extend_from_slice(&SAMPLE_RATE.to_le_bytes());
    wave.extend_from_slice(&(2 * SAMPLE_RATE).to_le_bytes());
    wave.extend_from_slice(&2u16.to_le_bytes()); // Frame alignment
    wave.extend_from_slice(&16u16.to_le_bytes());
    wave.extend_from_slice(b"data");
    wave.extend_from_slice(&data_len.to_le_bytes());
    wave.extend_from_slice(pcm);
    wave
}

fn permitted(request: &RequestEnvelope) -> bool {
    request.locale == "ja"
        && request.attachments.is_empty()
        && request.permissions.read_hosts.is_empty()
        && !request.permissions.wasm_execution
}

pub(crate) fn verify(request: &RequestEnvelope) -> Result<()> {
    let Some(source) = &request.speech else {
        return Ok(());
    };
    if !permitted(request)
        || source.recognizer != asr_julius::MODEL_ID
        || source.sample_rate != SAMPLE_RATE
        || source.start_frame != 0
        || request.text.is_empty()
        || request.text.trim() != request.text
        || request.text.len() > MAX_TEXT_BYTES
        || request.text.chars().any(char::is_control)
        || request.input_bytes() > request.budget.max_input_bytes
    {
        return Err(invalid_source());
    }
    let wave = source.wave_bytes(MAX_WAVE_BYTES)?;
    if verified_pcm(&wave)?.len() / 2 != source.end_frame {
        return Err(invalid_source());
    }
    Ok(())
}

pub(crate) fn bind(
    store: &Store,
    mut request: RequestEnvelope,
    source: WaveSource<'_>,
    started: Instant,
) -> Result<RequestEnvelope> {
    if !permitted(&request) || !request.text.is_empty() || request.speech.is_some() {
        return Err(failure(
            "SPEECH_INPUT_SCOPE",
            "音声入力は音声ファイル1件から発話を受け付けます。",
        ));
    }
    store.load_case(&request.actor_id, &request.case_id)?;
    let limit = request.budget.max_input_bytes.min(MAX_WAVE_BYTES);
    let wave = match source {
        WaveSource::File(path) => Cow::Owned(snapshot_local_wave(path, limit)?),
        WaveSource::Bytes(bytes) if bytes.len() <= limit => Cow::Borrowed(bytes),
        WaveSource::Bytes(_) => {
            return Err(failure(
                "SPEECH_INPUT_LIMIT",
                "音声ファイルが入力上限を超えています。",
            ));
        }
    };
    let pcm = verified_pcm(&wave)?;
    let existing = store.get_input_bundle(
        &request.actor_id,
        &request.case_id,
        &request.input_id,
        request.input_version,
    )?;
    if let Some((_, bundle)) = existing {
        let retained = bundle.request;
        verify(&retained)?;
        if !retained
            .speech
            .as_ref()
            .is_some_and(|source| source.wave_sha256 == digest(&wave))
        {
            return Err(failure(
                "INPUT_IDENTITY_CONFLICT",
                "同じ入力IDと版には、元の音声と条件を指定してください。",
            ));
        }
        request.text.clone_from(&retained.text);
        request.speech.clone_from(&retained.speech);
        if retained != request {
            return Err(failure(
                "INPUT_IDENTITY_CONFLICT",
                "同じ入力IDと版には、元の音声と条件を指定してください。",
            ));
        }
        return Ok(request);
    }
    let deadline = started
        .checked_add(Duration::from_millis(request.budget.deadline_ms))
        .ok_or_else(|| failure("SPEECH_INPUT_DEADLINE", "音声入力の期限を確定できません。"))?;
    let recognition_wave = normalize_wave(pcm);
    request.text = asr_julius::recognize(&recognition_wave, deadline)?.text;
    request.speech = Some(SpeechInput::from_wave(
        asr_julius::MODEL_ID,
        &wave,
        SAMPLE_RATE,
        pcm.len() / 2,
    ));
    verify(&request)?;
    Ok(request)
}
