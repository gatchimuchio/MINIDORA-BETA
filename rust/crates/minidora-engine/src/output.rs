//! Physical UTF-8 rendering and exact-target local file delivery.
//!
//! The Core owns authorization and persists ATTEMPTED before invoking this
//! module. Parent directories must be controlled by a trusted local owner;
//! portable `std` path checks do not isolate hostile ancestor replacement.

use minidora_domain::{
    ActionRecord, ContentPlan, DeliveryReceipt, EffectPhase, RenderObservation, Result, digest,
    failure,
};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};

pub fn render(plan: &ContentPlan) -> Result<RenderObservation> {
    if plan.id.is_empty() {
        return Err(failure(
            "INVALID_CONTENT_PLAN",
            "A plan identity is required",
        ));
    }
    let bytes = plan.text.as_bytes().to_vec();
    Ok(RenderObservation {
        plan_id: plan.id.clone(),
        renderer_version: "utf8-bytes-v1".into(),
        actual_sha256: digest(&bytes),
        bytes,
        status: "RENDERED".into(),
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
    if !path.is_absolute()
        || path
            .components()
            .any(|c| matches!(c, Component::ParentDir | Component::CurDir))
        || path.file_name().is_none()
    {
        return Err(failure(
            "INVALID_WRITE_TARGET",
            "An absolute file path without traversal is required",
        ));
    }
    // Reject lexical dot segments too: Path::components normalizes interior '.'
    // on some hosts. Backslash is conservatively rejected across projections.
    if target.contains('\\') || target.split('/').any(|s| s == "." || s == "..") {
        return Err(failure(
            "INVALID_WRITE_TARGET",
            "Dot segments and ambiguous separators are unsupported",
        ));
    }
    for ancestor in path.ancestors() {
        match fs::symlink_metadata(ancestor) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err(failure(
                    "SYMLINK_WRITE_TARGET",
                    "File targets and their ancestors cannot be symlinks",
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
            Err(error) if ancestor == path && error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
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
    let mut file = File::open(path)?;
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
    if file.sync_all().is_err()
        || File::open(path.parent().expect("absolute file parent"))
            .and_then(|dir| dir.sync_all())
            .is_err()
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
    match fs::symlink_metadata(&path) {
        Ok(_) => return confirm_existing(action, &path, bytes),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
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
        File::open(parent)?.sync_all()?;
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

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
