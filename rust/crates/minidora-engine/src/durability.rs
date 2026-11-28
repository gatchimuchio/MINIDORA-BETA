//! Platform paths and access modes for durable local effects.

use std::{
    fs::{File, Metadata},
    io,
    path::{Component, Path},
};

#[cfg(windows)]
use std::{
    fs::{self, OpenOptions},
    os::windows::fs::{MetadataExt, OpenOptionsExt},
    path::Prefix,
};

pub(crate) enum LocalTargetPathError {
    NotAbsoluteFile,
    AmbiguousSpelling,
}

pub(crate) fn validate_local_target_path(path: &Path) -> Result<(), LocalTargetPathError> {
    if !path.is_absolute()
        || path.file_name().is_none()
        || path
            .components()
            .any(|part| matches!(part, Component::CurDir | Component::ParentDir))
    {
        return Err(LocalTargetPathError::NotAbsoluteFile);
    }
    #[cfg(windows)]
    let supported = path.to_str().is_some_and(is_windows_drive_file_spelling);
    #[cfg(not(windows))]
    let supported = {
        // Path::components can normalize interior dot segments.
        let spelling = path.to_string_lossy();
        !spelling.contains('\\') && !spelling.split('/').any(|part| matches!(part, "." | ".."))
    };
    if !supported {
        return Err(LocalTargetPathError::AmbiguousSpelling);
    }
    Ok(())
}

#[cfg(any(windows, test))]
fn is_windows_drive_file_spelling(spelling: &str) -> bool {
    let [drive, b':', b'\\' | b'/', ..] = spelling.as_bytes() else {
        return false;
    };
    drive.is_ascii_alphabetic()
        && spelling[3..]
            .split(['\\', '/'])
            .all(is_windows_file_component)
}

#[cfg(any(windows, test))]
fn is_windows_file_component(name: &str) -> bool {
    if name.is_empty()
        || name.starts_with(' ')
        || name.ends_with([' ', '.'])
        || name.contains(['<', '>', ':', '"', '|', '?', '*'])
        || name.bytes().any(|byte| byte < 32)
    {
        return false;
    }
    let (stem, extension) = name.split_once('.').unwrap_or((name, ""));
    let device_stem = stem.trim_end_matches(' ');
    if ["CON", "PRN", "AUX", "NUL", "CONIN$", "CONOUT$", "CLOCK$"]
        .iter()
        .any(|device| device_stem.eq_ignore_ascii_case(device))
    {
        return false;
    }
    if let Some(prefix) = device_stem.get(..3)
        && (prefix.eq_ignore_ascii_case("COM") || prefix.eq_ignore_ascii_case("LPT"))
        && matches!(
            &device_stem[3..],
            "0" | "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" | "¹" | "²" | "³"
        )
    {
        return false;
    }
    // A generated DOS alias is not an exact spelling of a long file name.
    !(stem.is_ascii()
        && stem.len() <= 8
        && extension.is_ascii()
        && extension.len() <= 3
        && stem.rsplit_once('~').is_some_and(|(name, sequence)| {
            !name.is_empty()
                && !sequence.is_empty()
                && sequence.bytes().all(|byte| byte.is_ascii_digit())
        }))
}

pub(crate) fn is_link_or_reparse_point(metadata: &Metadata) -> bool {
    #[cfg(windows)]
    {
        const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x0400;
        metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
    }
    #[cfg(not(windows))]
    {
        metadata.file_type().is_symlink()
    }
}

pub(crate) fn local_target_and_ancestors(path: &Path) -> impl Iterator<Item = &Path> {
    #[cfg(windows)]
    {
        // Reject a reparse point before performing path-based lookups below it.
        path.ancestors().collect::<Vec<_>>().into_iter().rev()
    }
    #[cfg(not(windows))]
    {
        path.ancestors()
    }
}

/// Reject aliases of existing paths without changing the requested spelling.
/// Call this after checking each ancestor's own metadata for reparse points.
pub(crate) fn keeps_local_target_name(path: &Path) -> io::Result<bool> {
    #[cfg(windows)]
    {
        let resolved = fs::canonicalize(path)?;
        let mut requested = path.components();
        let mut observed = resolved.components();
        let (Some(Component::Prefix(requested_drive)), Some(Component::Prefix(observed_drive))) =
            (requested.next(), observed.next())
        else {
            return Ok(false);
        };
        if !matches!(
            (requested_drive.kind(), observed_drive.kind()),
            (Prefix::Disk(wanted), Prefix::VerbatimDisk(actual) | Prefix::Disk(actual))
                if wanted.eq_ignore_ascii_case(&actual)
        ) {
            return Ok(false);
        }
        loop {
            match (requested.next(), observed.next()) {
                (None, None) => return Ok(true),
                (Some(Component::RootDir), Some(Component::RootDir)) => {}
                (Some(Component::Normal(wanted)), Some(Component::Normal(actual)))
                    if wanted == actual
                        || wanted.to_str().zip(actual.to_str()).is_some_and(
                            |(wanted, actual)| wanted.eq_ignore_ascii_case(actual),
                        ) => {}
                _ => return Ok(false),
            }
        }
    }
    #[cfg(not(windows))]
    {
        let _ = path;
        Ok(true)
    }
}

pub(crate) fn sync_directory(path: &Path) -> io::Result<()> {
    #[cfg(windows)]
    let directory = {
        // CreateFileW requires this flag for directories. FlushFileBuffers,
        // which backs File::sync_all on Windows, requires write access.
        const FILE_FLAG_BACKUP_SEMANTICS: u32 = 0x0200_0000;
        let directory = OpenOptions::new()
            .read(true)
            .write(true)
            .custom_flags(FILE_FLAG_BACKUP_SEMANTICS)
            .open(path)?;
        if !directory.metadata()?.is_dir() {
            return Err(io::Error::new(
                io::ErrorKind::NotADirectory,
                "directory sync target is not a directory",
            ));
        }
        directory
    };
    #[cfg(not(windows))]
    let directory = File::open(path)?;

    // Propagate unsupported or failed flushes rather than certifying them.
    directory.sync_all()
}

/// Open one existing handle for read-back and a subsequent durability check.
pub(crate) fn open_file_for_sync(path: &Path) -> io::Result<File> {
    #[cfg(windows)]
    {
        OpenOptions::new().read(true).write(true).open(path)
    }
    #[cfg(not(windows))]
    {
        File::open(path)
    }
}

/// Capture one explicitly selected local CSV before submitting the request.
/// The snapshot carries the actual bytes as UTF-8 and no host path or read grant.
pub fn snapshot_local_csv_attachment(
    path: &Path,
    max_bytes: usize,
) -> minidora_domain::Result<minidora_domain::RequestAttachment> {
    snapshot_attachment(path, max_bytes, true)
}

/// Capture one explicitly selected local PDF, XLSX, PNG, JPEG or UTF-8 text file.
pub fn snapshot_local_attachment(
    path: &Path,
    max_bytes: usize,
) -> minidora_domain::Result<minidora_domain::RequestAttachment> {
    snapshot_attachment(path, max_bytes, false)
}

fn snapshot_attachment(
    path: &Path,
    max_bytes: usize,
    csv_only: bool,
) -> minidora_domain::Result<minidora_domain::RequestAttachment> {
    use minidora_domain::{RequestAttachment, ResourceBudget, failure};
    use std::io::Read;

    validate_local_target_path(path).map_err(|_| {
        failure(
            "INVALID_ATTACHMENT_PATH",
            "添付には通常ファイルの絶対パスを指定してください。",
        )
    })?;
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| {
            if csv_only {
                RequestAttachment::is_csv_name(name)
            } else {
                RequestAttachment::is_supported_name(name)
            }
        })
        .ok_or_else(|| {
            failure(
                "INVALID_ATTACHMENT_NAME",
                if csv_only {
                    "拡張子が .csv のファイルを指定してください。"
                } else {
                    "拡張子が .csv、.txt、.md、.pdf、.xlsx、.png、.jpg、.jpeg のファイルを指定してください。"
                },
            )
        })?;
    let is_pdf = RequestAttachment::is_pdf_name(name);
    let is_xlsx = RequestAttachment::is_xlsx_name(name);
    let is_image = RequestAttachment::is_image_name(name);
    let content_budget = max_bytes
        .min(ResourceBudget::development().max_input_bytes)
        .checked_sub(name.len())
        .filter(|limit| *limit > 0)
        .ok_or_else(attachment_size_error)?;
    // Four encoded bytes represent at most three bytes of binary input.
    let file_budget = if is_pdf || is_xlsx || is_image {
        (content_budget / 4) * 3
    } else {
        content_budget
    };

    #[cfg(not(windows))]
    if csv_only {
        return Err(failure(
            "LOCAL_ATTACHMENT_UNAVAILABLE",
            "パスを指定するCSV添付は現在Windowsで利用できます。",
        ));
    }

    let (file, _parents) = open_local_attachment(path)?;
    let file_budget = u64::try_from(file_budget).unwrap_or(u64::MAX);
    if file.metadata()?.len() > file_budget {
        return Err(attachment_size_error());
    }
    let mut bytes = Vec::new();
    file.take(file_budget.saturating_add(1))
        .read_to_end(&mut bytes)?;
    if u64::try_from(bytes.len()).unwrap_or(u64::MAX) > file_budget {
        return Err(attachment_size_error());
    }
    if is_pdf {
        let signature = b"%PDF-";
        if !bytes[..bytes.len().min(1024)]
            .windows(signature.len())
            .any(|header| header == signature)
        {
            return Err(failure(
                "PDF_INVALID",
                "添付ファイルにPDFのヘッダーがありません。",
            ));
        }
        return Ok(RequestAttachment::pdf_snapshot(name, &bytes));
    }
    if is_xlsx {
        if !bytes.starts_with(b"PK\x03\x04") {
            return Err(failure(
                "XLSX_INVALID",
                "添付ファイルにXLSXのパッケージがありません。",
            ));
        }
        return Ok(RequestAttachment::binary_snapshot(name, &bytes));
    }
    if is_image {
        if !RequestAttachment::image_header_matches(name, &bytes) {
            return Err(failure(
                "IMAGE_INVALID",
                "添付の内容がファイル名のPNGまたはJPEG形式と一致しません。",
            ));
        }
        return Ok(RequestAttachment::binary_snapshot(name, &bytes));
    }
    let file_kind = if csv_only { "CSV" } else { "ファイル" };
    let text = String::from_utf8(bytes).map_err(|_| {
        failure(
            "INVALID_ATTACHMENT_UTF8",
            format!("添付した{file_kind}をUTF-8として読み取れませんでした。"),
        )
    })?;
    if text.is_empty() {
        return Err(failure(
            "ATTACHMENT_EMPTY",
            format!("添付した{file_kind}が空です。"),
        ));
    }
    Ok(RequestAttachment {
        name: name.into(),
        text,
        binary_base64: None,
    })
}

pub(crate) fn snapshot_local_wave(path: &Path, limit: usize) -> minidora_domain::Result<Vec<u8>> {
    use minidora_domain::failure;
    use std::io::Read;

    if !crate::output::valid_wave_path(path) {
        return Err(failure(
            "SPEECH_INPUT_PATH",
            "読み取るローカルWAVの絶対パスを指定してください。",
        ));
    }
    let (file, _parents) = open_local_attachment(path)?;
    let over_limit = || {
        failure(
            "SPEECH_INPUT_LIMIT",
            "音声ファイルが入力上限を超えています。",
        )
    };
    let limit = u64::try_from(limit).unwrap_or(u64::MAX);
    if file.metadata()?.len() > limit {
        return Err(over_limit());
    }
    let mut bytes = Vec::new();
    file.take(limit.saturating_add(1)).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > limit {
        return Err(over_limit());
    }
    Ok(bytes)
}

fn attachment_size_error() -> minidora_domain::Error {
    minidora_domain::failure(
        "ATTACHMENT_SIZE_LIMIT",
        "添付のファイル名と内容が入力上限を超えています。",
    )
}

#[cfg(windows)]
fn open_local_attachment(path: &Path) -> minidora_domain::Result<(File, Vec<File>)> {
    use minidora_domain::failure;

    const FILE_FLAG_BACKUP_SEMANTICS: u32 = 0x0200_0000;
    const FILE_FLAG_OPEN_REPARSE_POINT: u32 = 0x0020_0000;
    const FILE_READ_ATTRIBUTES: u32 = 0x0080;
    const FILE_SHARE_READ: u32 = 0x0001;

    // Keep every parent open without sharing writes or deletes. Relative to
    // those held directory names, a leaf cannot be redirected while we read.
    let mut parents = Vec::new();
    for parent in local_target_and_ancestors(path).filter(|entry| *entry != path) {
        let handle = OpenOptions::new()
            .access_mode(FILE_READ_ATTRIBUTES)
            .share_mode(FILE_SHARE_READ)
            .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
            .open(parent)?;
        let metadata = handle.metadata()?;
        if !metadata.is_dir() || is_link_or_reparse_point(&metadata) {
            return Err(failure(
                "INVALID_ATTACHMENT_PATH",
                "添付の親フォルダーにリンクまたは再解析ポイントがあります。",
            ));
        }
        parents.push(handle);
    }
    let handle = OpenOptions::new()
        .read(true)
        .share_mode(FILE_SHARE_READ)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
        .open(path)?;
    let metadata = handle.metadata()?;
    if !metadata.is_file() || is_link_or_reparse_point(&metadata) || !keeps_local_target_name(path)?
    {
        return Err(failure(
            "INVALID_ATTACHMENT_PATH",
            "ローカルの通常ファイルを元のドライブ名とファイル名で指定してください。",
        ));
    }
    Ok((handle, parents))
}

#[cfg(not(windows))]
fn open_local_attachment(_path: &Path) -> minidora_domain::Result<(File, Vec<File>)> {
    Err(minidora_domain::failure(
        "LOCAL_ATTACHMENT_UNAVAILABLE",
        "パスを指定するファイル添付は現在Windowsで利用できます。",
    ))
}

#[cfg(test)]
mod tests {
    use super::is_windows_drive_file_spelling;

    #[test]
    fn windows_targets_require_unambiguous_drive_and_file_names() {
        for allowed in [
            r"C:\Users\利用者\報告.txt",
            "c:/Users/利用者/.report",
            r"D:\2026 reports/確定.txt",
        ] {
            assert!(is_windows_drive_file_spelling(allowed), "{allowed}");
        }
        for blocked in [
            r"C:relative.txt",
            r"C:\",
            r"C:\a\\b.txt",
            r"C:\a\",
            r"C:\a\.\b.txt",
            r"C:/a\../b.txt",
            r"\\server\share\report.txt",
            r"\\?\C:\report.txt",
            r"\\.\C:\report.txt",
            r"C:\report.txt:extra",
            r"C:\report.txt::$DATA",
            r"C:\report?.txt",
            r"C:\ report.txt",
            r"C:\report.txt ",
            r"C:\report.txt.",
            r"C:\AUX .txt",
            r"C:\nul.tar.gz",
            r"C:\COM¹.txt",
            r"C:\LPT9.txt",
            r"C:\CONIN$.txt",
            r"C:\CONOUT$",
            r"C:\PROGRA~1\report.txt",
        ] {
            assert!(!is_windows_drive_file_spelling(blocked), "{blocked}");
        }
    }
}
