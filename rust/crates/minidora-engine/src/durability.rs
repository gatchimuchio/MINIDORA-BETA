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
