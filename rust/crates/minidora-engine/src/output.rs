//! Physical rendering and exact-target local file delivery.
//!
//! The Core owns authorization and persists ATTEMPTED before invoking this
//! module. Parent directories must be controlled by a trusted local owner;
//! portable `std` path checks do not isolate hostile ancestor replacement.

use crate::durability::{
    LocalTargetPathError, is_link_or_reparse_point, keeps_local_target_name,
    local_target_and_ancestors, open_file_for_sync, sync_directory, validate_local_target_path,
};
use minidora_domain::{
    ActionRecord, ContentPlan, DeliveryReceipt, EffectPhase, RenderObservation, RenderTarget,
    Result, digest, failure,
};
use serde::{Deserialize, Serialize};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::Instant;

pub(crate) const LEGACY_RENDERER_VERSION: &str = "utf8-bytes-v1";
pub(crate) const RENDERER_VERSION: &str = "utf8-bytes-v2";
pub(crate) const PNG_RENDERER_VERSION: &str = "png-image-artifact-v1";
pub(crate) const PNG_DISPLAY_RENDERER_VERSION: &str = "png-image-display-v1";
pub(crate) const WAVE_PCM_RENDERER_VERSION: &str = "wav-pcm16-mono-v1";
pub(crate) const WAVE_PCM_DISPLAY_RENDERER_VERSION: &str = "wav-pcm16-mono-display-v1";
const WAVE_MAX_SECONDS: usize = 60;
const WAVE_MAX_CHUNKS: usize = 32;
const WAVE_MAX_EXTRA_BYTES: usize = 65_536;
pub(crate) const WAVE_PCM_MAX_BYTES: usize = 48_000 * 2 * WAVE_MAX_SECONDS + WAVE_MAX_EXTRA_BYTES;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct WaveFacts {
    pub channels: u16,
    pub sample_rate: u32,
    pub bits_per_sample: u16,
    pub frame_count: usize,
    pub data_bytes: usize,
    pub nonzero_frames: usize,
    pub peak_amplitude: u16,
    pub pcm_sha256: String,
}

fn invalid_wave() -> minidora_domain::Error {
    failure(
        "INVALID_WAVE_PCM",
        "音声を有効なモノラル16ビットPCMのWAVとして確認できません。",
    )
}

pub(crate) fn inspect_wave(bytes: &[u8]) -> Result<WaveFacts> {
    inspect_wave_pcm(bytes).map(|(facts, _)| facts)
}

pub(crate) fn inspect_wave_pcm(bytes: &[u8]) -> Result<(WaveFacts, &[u8])> {
    if bytes.len() > WAVE_PCM_MAX_BYTES {
        return Err(failure(
            "WAVE_PCM_LIMIT",
            "WAVのバイト数が上限を超えました。",
        ));
    }
    let [
        b'R',
        b'I',
        b'F',
        b'F',
        l0,
        l1,
        l2,
        l3,
        b'W',
        b'A',
        b'V',
        b'E',
        rest @ ..,
    ] = bytes
    else {
        return Err(invalid_wave());
    };
    if u64::from(u32::from_le_bytes([*l0, *l1, *l2, *l3])) + 8 != bytes.len() as u64 {
        return Err(invalid_wave());
    }

    let mut chunks = rest;
    let mut sample_rate = None;
    let mut pcm = None;
    for _ in 0..WAVE_MAX_CHUNKS {
        if chunks.is_empty() {
            break;
        }
        let [a, b, c, d, l0, l1, l2, l3, payload @ ..] = chunks else {
            return Err(invalid_wave());
        };
        let size = u32::from_le_bytes([*l0, *l1, *l2, *l3]) as usize;
        let padded_size = size.checked_add(size % 2).ok_or_else(invalid_wave)?;
        let (chunk, remaining) = payload
            .split_at_checked(padded_size)
            .ok_or_else(invalid_wave)?;
        let data = &chunk[..size];
        match &[*a, *b, *c, *d] {
            b"fmt " if sample_rate.is_none() => {
                sample_rate = Some(parse_pcm_format(data)?);
            }
            b"data" if pcm.is_none() && sample_rate.is_some() => {
                pcm = Some(data);
            }
            b"fmt " | b"data" => return Err(invalid_wave()),
            _ => {}
        }
        chunks = remaining;
    }
    if !chunks.is_empty() {
        return Err(invalid_wave());
    }
    let sample_rate = sample_rate.ok_or_else(invalid_wave)?;
    let pcm = pcm.ok_or_else(invalid_wave)?;
    let frame_count = pcm.len() / 2;
    if pcm.len() % 2 != 0
        || frame_count as u64 * 1_000 < u64::from(sample_rate) * 20
        || frame_count > sample_rate as usize * WAVE_MAX_SECONDS
        || bytes.len() - pcm.len() > WAVE_MAX_EXTRA_BYTES
    {
        return Err(invalid_wave());
    }
    let mut nonzero_frames = 0;
    let mut peak_amplitude = 0;
    for frame in pcm.chunks_exact(2) {
        let amplitude = i16::from_le_bytes([frame[0], frame[1]]).unsigned_abs();
        nonzero_frames += usize::from(amplitude != 0);
        peak_amplitude = peak_amplitude.max(amplitude);
    }
    if nonzero_frames == 0 {
        return Err(invalid_wave());
    }
    Ok((
        WaveFacts {
            channels: 1,
            sample_rate,
            bits_per_sample: 16,
            frame_count,
            data_bytes: pcm.len(),
            nonzero_frames,
            peak_amplitude,
            pcm_sha256: digest(pcm),
        },
        pcm,
    ))
}

fn parse_pcm_format(data: &[u8]) -> Result<u32> {
    let [
        tag0,
        tag1,
        ch0,
        ch1,
        r0,
        r1,
        r2,
        r3,
        b0,
        b1,
        b2,
        b3,
        align0,
        align1,
        bits0,
        bits1,
        extra @ ..,
    ] = data
    else {
        return Err(invalid_wave());
    };
    let rate = u32::from_le_bytes([*r0, *r1, *r2, *r3]);
    if u16::from_le_bytes([*tag0, *tag1]) != 1
        || u16::from_le_bytes([*ch0, *ch1]) != 1
        || !(16_000..=48_000).contains(&rate)
        || u32::from_le_bytes([*b0, *b1, *b2, *b3]) != rate * 2
        || u16::from_le_bytes([*align0, *align1]) != 2
        || u16::from_le_bytes([*bits0, *bits1]) != 16
        || !matches!(extra, [] | [0, 0])
    {
        return Err(invalid_wave());
    }
    Ok(rate)
}

pub(crate) fn matches_render_target(path: Option<&Path>, target: Option<&RenderTarget>) -> bool {
    match (path, target) {
        (None, Some(RenderTarget::Display)) => true,
        (Some(expected), Some(RenderTarget::LocalFile { path })) => {
            expected.as_os_str() == path.as_os_str()
        }
        _ => false,
    }
}

fn wave_target_version(target: &RenderTarget) -> Option<&'static str> {
    match target {
        RenderTarget::Display => Some(WAVE_PCM_DISPLAY_RENDERER_VERSION),
        RenderTarget::LocalFile { path } if valid_wave_path(path) => {
            Some(WAVE_PCM_RENDERER_VERSION)
        }
        _ => None,
    }
}

pub(crate) fn valid_wave_path(path: &Path) -> bool {
    validate_local_target_path(path).is_ok()
        && path
            .extension()
            .and_then(|ext| ext.to_str())
            .is_some_and(|ext| ext.eq_ignore_ascii_case("wav"))
}

pub(crate) fn render_wave_to(
    plan: &ContentPlan,
    target: RenderTarget,
    bytes: Vec<u8>,
) -> Result<RenderObservation> {
    if plan.id.is_empty() {
        return Err(failure(
            "INVALID_CONTENT_PLAN",
            "A plan identity is required",
        ));
    }
    let version = wave_target_version(&target).ok_or_else(|| {
        failure(
            "INVALID_WAVE_TARGET",
            "音声の保存先には拡張子.wavのローカル絶対パスを指定してください。",
        )
    })?;
    inspect_wave(&bytes)?;
    Ok(RenderObservation {
        plan_id: plan.id.clone(),
        renderer_version: version.into(),
        actual_sha256: digest(&bytes),
        bytes,
        status: "RENDERED".into(),
        target: Some(target),
    })
}

pub(crate) fn validate_wave_render(plan: &ContentPlan, render: &RenderObservation) -> bool {
    !plan.id.is_empty()
        && render.status == "RENDERED"
        && render.plan_id == plan.id
        && render.target.as_ref().and_then(wave_target_version)
            == Some(render.renderer_version.as_str())
        && inspect_wave(&render.bytes).is_ok()
        && render.actual_sha256 == digest(&render.bytes)
}

pub(crate) fn valid_png_path(path: &Path) -> bool {
    validate_local_target_path(path).is_ok()
        && path
            .extension()
            .and_then(|ext| ext.to_str())
            .is_some_and(|ext| ext.eq_ignore_ascii_case("png"))
}

fn png_target_version(target: &RenderTarget) -> Option<&'static str> {
    match target {
        RenderTarget::Display => Some(PNG_DISPLAY_RENDERER_VERSION),
        RenderTarget::LocalFile { path } if valid_png_path(path) => Some(PNG_RENDERER_VERSION),
        _ => None,
    }
}

pub(crate) fn render_png_to(
    plan: &ContentPlan,
    target: RenderTarget,
    bytes: Vec<u8>,
) -> Result<RenderObservation> {
    if plan.id.is_empty() {
        return Err(failure(
            "INVALID_CONTENT_PLAN",
            "採用した内容の識別子がありません。",
        ));
    }
    let version = png_target_version(&target).ok_or_else(|| {
        failure(
            "INVALID_PNG_TARGET",
            "画像の保存先には拡張子.pngのローカル絶対パスを指定してください。",
        )
    })?;
    crate::image_artifact::inspect_png(&bytes)?;
    Ok(RenderObservation {
        plan_id: plan.id.clone(),
        renderer_version: version.into(),
        actual_sha256: digest(&bytes),
        bytes,
        status: "RENDERED".into(),
        target: Some(target),
    })
}

pub(crate) fn validate_png_render(plan: &ContentPlan, render: &RenderObservation) -> bool {
    !plan.id.is_empty()
        && render.status == "RENDERED"
        && render.plan_id == plan.id
        && render.target.as_ref().and_then(png_target_version)
            == Some(render.renderer_version.as_str())
        && render.actual_sha256 == digest(&render.bytes)
        && crate::image_artifact::inspect_png(&render.bytes).is_ok()
}

/// Direct rendering retains the original observation format for callers
/// without a sealed Goal.
pub fn render(plan: &ContentPlan) -> Result<RenderObservation> {
    render_inner(plan, None)
}

pub(crate) fn render_to(plan: &ContentPlan, target: RenderTarget) -> Result<RenderObservation> {
    render_inner(plan, Some(target))
}

fn render_inner(plan: &ContentPlan, target: Option<RenderTarget>) -> Result<RenderObservation> {
    if plan.id.is_empty() {
        return Err(failure(
            "INVALID_CONTENT_PLAN",
            "A plan identity is required",
        ));
    }
    let bytes = plan.text.as_bytes().to_vec();
    let renderer_version = if target.is_some() {
        RENDERER_VERSION
    } else {
        LEGACY_RENDERER_VERSION
    };
    Ok(RenderObservation {
        plan_id: plan.id.clone(),
        renderer_version: renderer_version.into(),
        actual_sha256: digest(&bytes),
        bytes,
        status: "RENDERED".into(),
        target,
    })
}

fn hash_identity(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
}

fn checked_target(target: &str) -> Result<PathBuf> {
    let path = Path::new(target);
    validate_local_target_path(path).map_err(|error| {
        failure(
            "INVALID_WRITE_TARGET",
            match error {
                LocalTargetPathError::NotAbsoluteFile => {
                    "An absolute file path without traversal is required"
                }
                LocalTargetPathError::AmbiguousSpelling if cfg!(windows) => {
                    "An unambiguous absolute Windows drive path is required"
                }
                LocalTargetPathError::AmbiguousSpelling => {
                    "Dot segments and ambiguous separators are unsupported"
                }
            },
        )
    })?;
    let mut existing_path = path;
    for ancestor in local_target_and_ancestors(path) {
        match fs::symlink_metadata(ancestor) {
            Ok(metadata) if is_link_or_reparse_point(&metadata) => {
                return Err(failure(
                    "SYMLINK_WRITE_TARGET",
                    if cfg!(windows) {
                        "File targets and their ancestors cannot be Windows reparse points"
                    } else {
                        "File targets and their ancestors cannot be symlinks"
                    },
                ));
            }
            Ok(metadata) if ancestor != path && !metadata.is_dir() => {
                return Err(failure(
                    "INVALID_WRITE_TARGET",
                    "Every parent must be an existing directory",
                ));
            }
            Ok(metadata) if ancestor == path && !metadata.is_file() => {
                return Err(failure(
                    "INVALID_WRITE_TARGET",
                    "The target cannot be a directory or special file",
                ));
            }
            Ok(_) => {}
            Err(error) if ancestor == path && error.kind() == std::io::ErrorKind::NotFound => {
                existing_path = path.parent().expect("absolute file parent");
            }
            Err(error) => return Err(error.into()),
        }
    }
    if !keeps_local_target_name(existing_path)? {
        return Err(failure(
            "INVALID_WRITE_TARGET",
            "Windows paths must identify the original local drive and file names",
        ));
    }
    Ok(path.to_owned())
}

fn receipt(
    action: &ActionRecord,
    phase: EffectPhase,
    observed_bytes: Option<u64>,
    reason: Option<&str>,
) -> DeliveryReceipt {
    DeliveryReceipt {
        action_id: action.action_id.clone(),
        target: action.target.clone(),
        content_sha256: action.exact_request_sha256.clone(),
        phase,
        observed_bytes,
        reason: reason.map(str::to_owned),
    }
}

fn confirm_existing(action: &ActionRecord, path: &Path, bytes: &[u8]) -> Result<DeliveryReceipt> {
    checked_target(&action.target)?;
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.is_file() || metadata.len() != bytes.len() as u64 {
        return Err(failure(
            "EXISTING_FILE_CONFLICT",
            "An existing different artifact will not be overwritten",
        ));
    }
    let (mut file, can_sync) = match open_file_for_sync(path) {
        Ok(file) => (file, true),
        // A read-only Windows handle can still distinguish matching bytes
        // from a conflicting artifact, but cannot confirm durability.
        Err(_) if cfg!(windows) => (File::open(path)?, false),
        Err(error) => return Err(error.into()),
    };
    let mut actual = Vec::new();
    Read::by_ref(&mut file)
        .take(bytes.len() as u64 + 1)
        .read_to_end(&mut actual)?;
    if actual != bytes {
        return Err(failure(
            "EXISTING_FILE_CONFLICT",
            "An existing different artifact will not be overwritten",
        ));
    }
    checked_target(&action.target)?;
    if !can_sync
        || file.sync_all().is_err()
        || sync_directory(path.parent().expect("absolute file parent")).is_err()
    {
        return Ok(receipt(
            action,
            EffectPhase::Unknown,
            Some(actual.len() as u64),
            Some("DURABILITY_UNCONFIRMED"),
        ));
    }
    Ok(receipt(
        action,
        EffectPhase::Confirmed,
        Some(actual.len() as u64),
        None,
    ))
}

pub fn write_file(action: &ActionRecord, bytes: &[u8]) -> Result<DeliveryReceipt> {
    write_file_inner(action, bytes, None)
}

pub(crate) fn write_file_until(
    action: &ActionRecord,
    bytes: &[u8],
    deadline: Instant,
) -> Result<DeliveryReceipt> {
    write_file_inner(action, bytes, Some(deadline))
}

fn write_file_inner(
    action: &ActionRecord,
    bytes: &[u8],
    deadline: Option<Instant>,
) -> Result<DeliveryReceipt> {
    if action.phase != EffectPhase::Attempted
        || action.effect_class != "LOCAL_WRITE"
        || action.action_id.is_empty()
        || action.case_id.is_empty()
        || action.actor_id.is_empty()
        || action.idempotency_key.is_empty()
        || !hash_identity(&action.authorization_sha256)
        || action.exact_request_sha256 != digest(bytes)
    {
        return Err(failure(
            "INVALID_WRITE_ACTION",
            "A persisted, authorized ATTEMPTED action must bind the exact bytes",
        ));
    }
    let path = checked_target(&action.target)?;
    let parent = path.parent().expect("absolute file parent");
    let original_parent = parent.canonicalize()?;
    let exists = match fs::symlink_metadata(&path) {
        Ok(_) => true,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => false,
        Err(error) => return Err(error.into()),
    };
    if deadline.is_some_and(|limit| Instant::now() >= limit) {
        return Err(failure(
            "DEADLINE",
            "file delivery did not start before the Goal deadline",
        ));
    }
    if exists {
        return confirm_existing(action, &path, bytes);
    }
    let mut file = match OpenOptions::new().write(true).create_new(true).open(&path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            return confirm_existing(action, &path, bytes);
        }
        Err(error) => return Err(error.into()),
    };
    // From successful exclusive creation onward, a failure may leave an actual
    // partial/complete file. Retain it and the same action identity for Core
    // reconciliation; never delete it or report a fictional clean failure.
    let physical = (|| -> Result<()> {
        file.write_all(bytes)?;
        file.flush()?;
        file.sync_all()?;
        sync_directory(parent)?;
        checked_target(&action.target)?;
        if parent.canonicalize()? != original_parent {
            return Err(failure(
                "WRITE_PARENT_CHANGED",
                "The target directory identity changed",
            ));
        }
        let mut actual = Vec::new();
        File::open(&path)?
            .take(bytes.len() as u64 + 1)
            .read_to_end(&mut actual)?;
        if actual != bytes {
            return Err(failure(
                "WRITE_VERIFICATION_FAILED",
                "The observed artifact differs from the requested bytes",
            ));
        }
        Ok(())
    })();
    match physical {
        Ok(()) => Ok(receipt(
            action,
            EffectPhase::Confirmed,
            Some(bytes.len() as u64),
            None,
        )),
        Err(_) => Ok(receipt(
            action,
            EffectPhase::Unknown,
            file.metadata().ok().map(|m| m.len()),
            Some("WRITE_OR_DURABILITY_UNCONFIRMED"),
        )),
    }
}

pub(crate) fn matches_file_bytes(path: &Path, expected: &[u8]) -> bool {
    let observation = (|| -> std::io::Result<bool> {
        let metadata = fs::symlink_metadata(path)?;
        if !metadata.is_file() || metadata.len() != expected.len() as u64 {
            return Ok(false);
        }
        let mut bytes = Vec::with_capacity(expected.len());
        File::open(path)?
            .take(expected.len() as u64 + 1)
            .read_to_end(&mut bytes)?;
        Ok(bytes == expected)
    })();
    observation.unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;
    use std::time::Instant;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn wave() -> Vec<u8> {
        let resolution = [1, 0, 1, 0, 128, 62, 0, 0, 0, 125, 0, 0, 2, 0, 16, 0, 0, 0];
        let mut pcm = [0; 640];
        pcm[100..102].copy_from_slice(&i16::MIN.to_le_bytes());
        let mut bytes = b"RIFF".to_vec();
        bytes.extend_from_slice(&(4 + 8 + 18 + 8 + pcm.len() as u32).to_le_bytes());
        bytes.extend_from_slice(b"WAVEfmt ");
        bytes.extend_from_slice(&18u32.to_le_bytes());
        bytes.extend_from_slice(&resolution);
        bytes.extend_from_slice(b"data");
        bytes.extend_from_slice(&(pcm.len() as u32).to_le_bytes());
        bytes.extend_from_slice(&pcm);
        bytes
    }

    #[test]
    fn wave_render_checks_pcm_shape_and_retained_identity() {
        let plan = ContentPlan {
            id: "plan".into(),
            text: "読み上げの原文。".into(),
            locale: "ja".into(),
            guarantee: minidora_domain::Guarantee::UserPremise,
            required_units: Vec::new(),
            dependencies: Vec::new(),
            model_revision: String::new(),
            model_artifact_ref: String::new(),
            model_observation_ref: String::new(),
        };
        let wave = wave();
        let facts = inspect_wave(&wave).unwrap();
        assert_eq!(facts.sample_rate, 16_000);
        assert_eq!(facts.frame_count, 320);
        assert_eq!(facts.nonzero_frames, 1);
        assert_eq!(facts.peak_amplitude, 32_768);
        assert_eq!(facts.pcm_sha256, digest(&wave[46..]));
        let path = PathBuf::from(if cfg!(windows) {
            r"C:\output.wav"
        } else {
            "/output.wav"
        });
        let target = RenderTarget::LocalFile { path };
        let render = render_wave_to(&plan, target, wave.clone()).unwrap();
        assert!(validate_wave_render(&plan, &render));
        assert_eq!(render.renderer_version, WAVE_PCM_RENDERER_VERSION);

        let display = render_wave_to(&plan, RenderTarget::Display, wave.clone()).unwrap();
        assert!(validate_wave_render(&plan, &display));
        assert_eq!(display.renderer_version, WAVE_PCM_DISPLAY_RENDERER_VERSION);
        assert_eq!(display.target, Some(RenderTarget::Display));
        assert_eq!(display.bytes, wave);
        assert_eq!(display.actual_sha256, render.actual_sha256);
        let mut crossed_target = display;
        crossed_target.target = render.target.clone();
        assert!(!validate_wave_render(&plan, &crossed_target));

        let mut stale = render.clone();
        *stale.bytes.last_mut().unwrap() = 1;
        assert!(!validate_wave_render(&plan, &stale));
        let mut wrong_plan = plan;
        wrong_plan.id = "other".into();
        assert!(!validate_wave_render(&wrong_plan, &render));

        let mut malformed = wave.clone();
        malformed.push(0);
        assert!(inspect_wave(&malformed).is_err());
        malformed = wave;
        malformed[46..].fill(0);
        assert!(inspect_wave(&malformed).is_err());
    }

    fn directory() -> PathBuf {
        let p = std::env::temp_dir().join(format!(
            "minidora-output-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir(&p).unwrap();
        p
    }
    fn action(path: &Path, bytes: &[u8]) -> ActionRecord {
        ActionRecord {
            case_id: "case".into(),
            actor_id: "actor".into(),
            action_id: "action".into(),
            input_fingerprint: digest(b"input"),
            idempotency_key: "stable".into(),
            effect_class: "LOCAL_WRITE".into(),
            target: path.to_str().unwrap().into(),
            authorization_sha256: digest(b"explicit scope"),
            exact_request_sha256: digest(bytes),
            phase: EffectPhase::Attempted,
            receipt: None,
        }
    }
    #[test]
    fn actual_bytes_are_exclusive_and_idempotent() {
        let dir = directory();
        let path = dir.join("result.txt");
        let bytes = "演算結果: 5\n".as_bytes();
        let a = action(&path, bytes);
        assert_eq!(write_file(&a, bytes).unwrap().phase, EffectPhase::Confirmed);
        assert_eq!(fs::read(&path).unwrap(), bytes);
        assert_eq!(write_file(&a, bytes).unwrap().phase, EffectPhase::Confirmed);
        assert!(write_file(&action(&path, b"changed"), b"changed").is_err());
        assert_eq!(fs::read(&path).unwrap(), bytes);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn separate_processes_cannot_overwrite_a_shared_target_with_different_bytes() {
        let dir = directory();
        let target = dir.join("shared.txt");
        let first_bytes = b"first complete writer".to_vec();
        let second_bytes = b"second complete writer".to_vec();
        let mut first = action(&target, &first_bytes);
        first.action_id = "writer-first".into();
        first.idempotency_key = "writer-first-key".into();
        let mut second = action(&target, &second_bytes);
        second.action_id = "writer-second".into();
        second.idempotency_key = "writer-second-key".into();

        let gate = dir.join("release");
        let mut children = Vec::new();
        let mut ready_paths = Vec::new();
        let mut result_paths = Vec::new();
        for (worker, (action, bytes)) in
            [(first, first_bytes.clone()), (second, second_bytes.clone())]
                .into_iter()
                .enumerate()
        {
            let action_path = dir.join(format!("action-{worker}.json"));
            let bytes_path = dir.join(format!("bytes-{worker}.bin"));
            let ready = dir.join(format!("ready-{worker}"));
            let result = dir.join(format!("result-{worker}"));
            fs::write(&action_path, serde_json::to_vec(&action).unwrap()).unwrap();
            fs::write(&bytes_path, bytes).unwrap();
            let child = Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "output::tests::multi_process_write_worker",
                    "--nocapture",
                ])
                .env("MINIDORA_OUTPUT_RACE_ACTION", &action_path)
                .env("MINIDORA_OUTPUT_RACE_BYTES", &bytes_path)
                .env("MINIDORA_OUTPUT_RACE_GATE", &gate)
                .env("MINIDORA_OUTPUT_RACE_READY", &ready)
                .env("MINIDORA_OUTPUT_RACE_RESULT", &result)
                .spawn()
                .unwrap();
            children.push(child);
            ready_paths.push(ready);
            result_paths.push(result);
        }

        let deadline = Instant::now() + std::time::Duration::from_secs(10);
        loop {
            if ready_paths.iter().all(|path| path.exists()) {
                break;
            }
            for child in &mut children {
                if let Some(status) = child.try_wait().unwrap() {
                    panic!("local writer exited before the barrier: {status}");
                }
            }
            assert!(
                Instant::now() < deadline,
                "local writers did not reach the barrier"
            );
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        fs::write(&gate, b"go").unwrap();
        for child in children {
            let output = child.wait_with_output().unwrap();
            assert!(
                output.status.success(),
                "local writer process failed: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }

        let mut outcomes: Vec<_> = result_paths
            .iter()
            .map(|path| fs::read_to_string(path).unwrap())
            .collect();
        outcomes.sort();
        assert_eq!(outcomes, ["CONFIRMED", "ERR:EXISTING_FILE_CONFLICT"]);
        let actual = fs::read(&target).unwrap();
        assert!(actual == first_bytes || actual == second_bytes);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn multi_process_write_worker() {
        let Ok(action_path) = std::env::var("MINIDORA_OUTPUT_RACE_ACTION") else {
            return;
        };
        let bytes_path = std::env::var("MINIDORA_OUTPUT_RACE_BYTES").unwrap();
        let gate = std::env::var("MINIDORA_OUTPUT_RACE_GATE").unwrap();
        let ready = std::env::var("MINIDORA_OUTPUT_RACE_READY").unwrap();
        let result = std::env::var("MINIDORA_OUTPUT_RACE_RESULT").unwrap();
        let action: ActionRecord = serde_json::from_slice(&fs::read(action_path).unwrap()).unwrap();
        let bytes = fs::read(bytes_path).unwrap();
        fs::write(ready, b"ready").unwrap();
        let deadline = Instant::now() + std::time::Duration::from_secs(15);
        while !Path::new(&gate).exists() {
            assert!(
                Instant::now() < deadline,
                "parent did not release local writer barrier"
            );
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        let outcome = match write_file(&action, &bytes) {
            Ok(receipt) if receipt.phase == EffectPhase::Confirmed => "CONFIRMED".to_owned(),
            Ok(receipt) => format!("PHASE:{:?}", receipt.phase),
            Err(minidora_domain::Error::Contract { code, .. }) => format!("ERR:{code}"),
            Err(error) => format!("ERR:{error}"),
        };
        fs::write(result, outcome).unwrap();
    }

    #[test]
    fn authority_and_traversal_are_checked_before_creation() {
        let dir = directory();
        let path = dir.join("result.txt");
        let mut a = action(&path, b"x");
        a.phase = EffectPhase::Prepared;
        assert!(write_file(&a, b"x").is_err());
        assert!(!path.exists());
        assert!(write_file(&action(&dir.join("../escape"), b"x"), b"x").is_err());
        assert!(write_file(&action(&dir.join("./escape"), b"x"), b"x").is_err());
        fs::remove_dir_all(dir).unwrap();
    }
    #[cfg(unix)]
    #[test]
    fn symlink_ancestors_and_leaf_are_rejected() {
        use std::os::unix::fs::symlink;
        let dir = directory();
        let real = dir.join("real");
        fs::create_dir(&real).unwrap();
        symlink(&real, dir.join("linked")).unwrap();
        assert!(write_file(&action(&dir.join("linked/file"), b"x"), b"x").is_err());
        symlink(real.join("file"), dir.join("leaf")).unwrap();
        assert!(write_file(&action(&dir.join("leaf"), b"x"), b"x").is_err());
        fs::remove_dir_all(dir).unwrap();
    }
}
