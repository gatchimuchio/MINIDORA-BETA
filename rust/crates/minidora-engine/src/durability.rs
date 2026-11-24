//! Platform access modes for flushing application state and local effects.

use std::{fs::File, io, path::Path};

#[cfg(windows)]
use std::{fs::OpenOptions, os::windows::fs::OpenOptionsExt};

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
