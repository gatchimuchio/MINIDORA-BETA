//! Core-owned persistence adapter. SQLite owns logical transactions; immutable
//! blobs are durably published first. Orphan blobs are harmless, while a
//! missing or corrupt referenced blob is an integrity failure.
use minidora_domain::{
    ActionRecord, CaseRevision, EffectPhase, InputBundle, LEGACY_SCHEMA_VERSION, Lifecycle,
    MemoryState, RequestEnvelope, Result, ResultRecord, SCHEMA_VERSION, canonical, digest, failure,
    identity,
};
use rusqlite::{
    Connection, OpenFlags, OptionalExtension, Transaction, TransactionBehavior, params,
};
use serde::{Serialize, de::DeserializeOwned};
use std::{
    collections::BTreeSet,
    fs::{self, File, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
    time::Duration,
};

static TEMP_SERIAL: AtomicU64 = AtomicU64::new(0);

type SqlResult<T> = std::result::Result<T, rusqlite::Error>;
fn sql<T>(result: SqlResult<T>) -> Result<T> {
    result.map_err(|error| failure("STORE_SQLITE", error.to_string()))
}
fn integer(value: u64) -> Result<i64> {
    i64::try_from(value)
        .map_err(|_| failure("INVALID_REVISION", "revision exceeds SQLite integer range"))
}
fn valid_id(value: &str) -> Result<()> {
    if value.trim().is_empty() || value.len() > 4096 {
        return Err(failure(
            "INVALID_IDENTITY",
            "identity is empty or oversized",
        ));
    }
    Ok(())
}
fn valid_digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}
fn sync_directory(path: &Path) -> Result<()> {
    // Unsupported directory fsync is a typed I/O failure, never a silent claim
    // of the durable profile on that filesystem/target.
    File::open(path)?.sync_all()?;
    Ok(())
}

fn install_action_identity_guards(connection: &Connection) -> Result<()> {
    sql(connection.execute_batch(
        "CREATE TRIGGER IF NOT EXISTS action_identity_no_update BEFORE UPDATE ON action_identity BEGIN SELECT RAISE(ABORT,'immutable action identity'); END;
         CREATE TRIGGER IF NOT EXISTS action_identity_no_delete BEFORE DELETE ON action_identity BEGIN SELECT RAISE(ABORT,'immutable action identity'); END;",
    ))?;
    Ok(())
}

fn next_temp_serial() -> Result<u64> {
    let mut current = TEMP_SERIAL.load(Ordering::Relaxed);
    loop {
        let next = current.checked_add(1).ok_or_else(|| {
            failure(
                "STORE_TEMP_SERIAL_EXHAUSTED",
                "blob stage identity exhausted",
            )
        })?;
        match TEMP_SERIAL.compare_exchange_weak(current, next, Ordering::Relaxed, Ordering::Relaxed)
        {
            Ok(_) => return Ok(current),
            Err(observed) => current = observed,
        }
    }
}

fn create_stage_file_with(
    blobs: &Path,
    process_id: u32,
    mut next_serial: impl FnMut() -> Result<u64>,
) -> Result<(PathBuf, File)> {
    loop {
        let serial = next_serial()?;
        let temporary = blobs.join(format!(".stage-{process_id}-{serial}"));
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
        {
            Ok(file) => return Ok((temporary, file)),
            // A previous process may have crashed after creating its stage
            // file, then the OS may later reuse its PID. Keep the orphan and
            // advance to another exclusive name instead of failing the write.
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error.into()),
        }
    }
}

pub struct Store {
    connection: Connection,
    blobs: PathBuf,
}

impl Store {
    /// Opens a single-owner state directory, not an arbitrary SQLite filename.
    /// Authentication belongs to the host; every operation below checks the
    /// authenticated actor argument against persisted ownership.
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        Self::open_internal(path.as_ref(), false, true)
    }

    fn open_internal(path: &Path, allow_legacy: bool, initialize: bool) -> Result<Self> {
        if path.exists() && fs::symlink_metadata(path)?.file_type().is_symlink() {
            return Err(failure(
                "UNSAFE_STORAGE_PATH",
                "state directory is a symlink",
            ));
        }
        if initialize {
            fs::create_dir_all(path)?;
            let parent = path
                .parent()
                .filter(|p| !p.as_os_str().is_empty())
                .unwrap_or_else(|| Path::new("."));
            sync_directory(parent)?;
        } else if !path.is_dir() {
            return Err(failure(
                "MIGRATION_SOURCE_MISSING",
                "migration requires an existing state directory",
            ));
        }
        let blobs = path.join("blobs");
        if blobs.exists() && fs::symlink_metadata(&blobs)?.file_type().is_symlink() {
            return Err(failure(
                "UNSAFE_STORAGE_PATH",
                "blob directory is a symlink",
            ));
        }
        if initialize {
            fs::create_dir_all(&blobs)?;
            sync_directory(path)?;
        } else if !blobs.is_dir() {
            return Err(failure(
                "MIGRATION_SOURCE_MISSING",
                "migration requires the existing immutable blob directory",
            ));
        }
        let database = path.join("state.sqlite3");
        if !initialize && !database.exists() {
            return Err(failure(
                "MIGRATION_SOURCE_MISSING",
                "migration requires an existing state database",
            ));
        }
        if database.exists() && !fs::symlink_metadata(&database)?.is_file() {
            return Err(failure(
                "UNSAFE_STORAGE_PATH",
                "database is not a regular file",
            ));
        }
        let connection = if initialize {
            sql(Connection::open(&database))?
        } else {
            sql(Connection::open_with_flags(
                &database,
                OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NOFOLLOW,
            ))?
        };
        sql(connection.busy_timeout(Duration::from_secs(5)))?;
        if initialize {
            sql(connection.execute_batch(
                "PRAGMA foreign_keys=ON;
                 PRAGMA journal_mode=WAL;
                 PRAGMA synchronous=FULL;
                 PRAGMA wal_autocheckpoint=1000;",
            ))?;
        } else {
            // Configure only connection-local settings. Migration must not
            // change an unexpected on-disk journal mode before verification.
            sql(connection.execute_batch(
                "PRAGMA foreign_keys=ON;
                 PRAGMA synchronous=FULL;
                 PRAGMA wal_autocheckpoint=1000;",
            ))?;
        }
        let foreign_keys: i64 = sql(connection.query_row("PRAGMA foreign_keys", [], |r| r.get(0)))?;
        let synchronous: i64 = sql(connection.query_row("PRAGMA synchronous", [], |r| r.get(0)))?;
        let journal_mode: String =
            sql(connection.query_row("PRAGMA journal_mode", [], |r| r.get(0)))?;
        if foreign_keys != 1 || synchronous != 2 || journal_mode != "wal" {
            return Err(failure(
                "DURABILITY_UNAVAILABLE",
                "SQLite cannot enforce FK/WAL/FULL profile",
            ));
        }
        if initialize {
            sql(connection.execute_batch(SCHEMA))?;
            sql(connection.execute(
                "INSERT OR IGNORE INTO store_metadata(key,value) VALUES('schema',?1)",
                [SCHEMA_VERSION],
            ))?;
        }
        let schema: String = sql(connection.query_row(
            "SELECT value FROM store_metadata WHERE key='schema'",
            [],
            |r| r.get(0),
        ))?;
        if schema != SCHEMA_VERSION && !(allow_legacy && schema == LEGACY_SCHEMA_VERSION) {
            return Err(failure(
                "SCHEMA_MISMATCH",
                "state requires an explicit migration",
            ));
        }
        if initialize {
            install_action_identity_guards(&connection)?;
            sync_directory(path)?;
        }
        let store = Self { connection, blobs };
        store.verify_integrity()?;
        Ok(store)
    }

    /// Explicitly upgrades alpha 7 store metadata after verifying every
    /// referenced blob, case journal, action journal and conflict chain. Case
    /// revision blobs remain immutable and keep their original schema version.
    /// Returns `false` for a valid alpha 8 store without changing it.
    pub fn migrate_alpha7_to_alpha8(path: impl AsRef<Path>) -> Result<bool> {
        let path = path.as_ref();
        let store = Self::open_internal(path, true, false)?;
        let transaction = store.transaction()?;
        // Recheck under the SQLite write lock so no concurrent alpha 7 writer
        // can append state between verification and the metadata transition.
        store.verify_integrity()?;
        let schema: String = sql(transaction.query_row(
            "SELECT value FROM store_metadata WHERE key='schema'",
            [],
            |row| row.get(0),
        ))?;
        if schema == SCHEMA_VERSION {
            sql(transaction.commit())?;
            return Ok(false);
        }
        if schema != LEGACY_SCHEMA_VERSION {
            return Err(failure(
                "SCHEMA_MISMATCH",
                format!("explicit migration supports only {LEGACY_SCHEMA_VERSION}"),
            ));
        }
        let changed = sql(transaction.execute(
            "UPDATE store_metadata SET value=?1 WHERE key='schema' AND value=?2",
            params![SCHEMA_VERSION, LEGACY_SCHEMA_VERSION],
        ))?;
        if changed != 1 {
            return Err(failure(
                "SCHEMA_MIGRATION_CONFLICT",
                "schema metadata changed during the migration",
            ));
        }
        if let Err(error) = transaction.commit() {
            return Err(failure(
                "MIGRATION_DURABILITY_UNKNOWN",
                format!("SQLite commit outcome could not be confirmed: {error}"),
            ));
        }
        if let Err(error) = install_action_identity_guards(&store.connection) {
            return Err(failure(
                "MIGRATION_DURABILITY_UNKNOWN",
                format!("schema migration committed, but action identity guards failed: {error}"),
            ));
        }
        if let Err(error) = sync_directory(path) {
            return Err(failure(
                "MIGRATION_DURABILITY_UNKNOWN",
                format!("schema commit completed, but directory sync failed: {error}"),
            ));
        }
        Ok(true)
    }

    fn transaction(&self) -> Result<Transaction<'_>> {
        sql(Transaction::new_unchecked(
            &self.connection,
            TransactionBehavior::Immediate,
        ))
    }

    fn actor_for_case(connection: &Connection, actor: &str, case: &str) -> Result<bool> {
        valid_id(actor)?;
        valid_id(case)?;
        let owner: Option<String> = sql(connection
            .query_row(
                "SELECT actor_id FROM case_registry WHERE case_id=?1",
                [case],
                |r| r.get(0),
            )
            .optional())?;
        match owner {
            Some(owner) if owner != actor => {
                Err(failure("ACCESS_DENIED", "actor does not own this case"))
            }
            Some(_) => Ok(true),
            None => Ok(false),
        }
    }

    fn require_case(connection: &Connection, actor: &str, case: &str) -> Result<()> {
        if !Self::actor_for_case(connection, actor, case)? {
            return Err(failure("CASE_NOT_FOUND", "case has not been registered"));
        }
        Ok(())
    }

    fn publish_blob<T: Serialize>(&self, value: &T) -> Result<(String, usize)> {
        self.publish_bytes(&canonical(value)?)
    }

    fn publish_bytes(&self, bytes: &[u8]) -> Result<(String, usize)> {
        let sha = digest(bytes);
        let destination = self.blobs.join(&sha);
        if destination.exists() {
            self.read_blob_bytes(&sha)?;
            return Ok((sha, bytes.len()));
        }
        let (temporary, mut file) =
            create_stage_file_with(&self.blobs, std::process::id(), next_temp_serial)?;
        let result = (|| -> Result<()> {
            file.write_all(bytes)?;
            file.sync_all()?;
            match fs::hard_link(&temporary, &destination) {
                Ok(()) => (),
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                    self.read_blob_bytes(&sha)?;
                }
                Err(error) => return Err(error.into()),
            }
            sync_directory(&self.blobs)?;
            Ok(())
        })();
        let cleanup = fs::remove_file(&temporary);
        result?;
        cleanup?;
        sync_directory(&self.blobs)?;
        Ok((sha, bytes.len()))
    }

    fn reference(connection: &Connection, sha: &str, length: usize, kind: &str) -> Result<()> {
        sql(connection.execute(
            "INSERT OR IGNORE INTO artifact_ref(sha256,byte_length,kind) VALUES(?1,?2,?3)",
            params![
                sha,
                i64::try_from(length)
                    .map_err(|_| failure("ARTIFACT_TOO_LARGE", "blob size overflow"))?,
                kind
            ],
        ))?;
        let stored: i64 = sql(connection.query_row(
            "SELECT byte_length FROM artifact_ref WHERE sha256=?1",
            [sha],
            |r| r.get(0),
        ))?;
        if stored != length as i64 {
            return Err(failure(
                "INTEGRITY_FAILURE",
                "artifact length differs for the same digest",
            ));
        }
        Ok(())
    }

    fn read_blob_bytes(&self, sha: &str) -> Result<Vec<u8>> {
        if !valid_digest(sha) {
            return Err(failure("INTEGRITY_FAILURE", "invalid blob identity"));
        }
        let path = self.blobs.join(sha);
        let metadata = fs::symlink_metadata(&path).map_err(|_| {
            failure(
                "INTEGRITY_FAILURE",
                format!("missing referenced blob {sha}"),
            )
        })?;
        if !metadata.is_file() || metadata.file_type().is_symlink() {
            return Err(failure("INTEGRITY_FAILURE", "blob is not a regular file"));
        }
        let bytes = fs::read(path)?;
        if digest(&bytes) != sha {
            return Err(failure(
                "INTEGRITY_FAILURE",
                format!("corrupt referenced blob {sha}"),
            ));
        }
        Ok(bytes)
    }

    fn read_blob<T: DeserializeOwned>(&self, sha: &str) -> Result<T> {
        Ok(serde_json::from_slice(&self.read_blob_bytes(sha)?)?)
    }

    /// Store an original typed artifact after checking its actor/case scope.
    pub fn put_artifact<T: Serialize>(&self, actor: &str, case: &str, value: &T) -> Result<String> {
        self.put_bytes(actor, case, &canonical(value)?)
    }

    /// Exact unmodified bytes; the returned digest can identify an effect
    /// request or the actual rendered artifact without format conversion.
    pub fn put_bytes(&self, actor: &str, case: &str, bytes: &[u8]) -> Result<String> {
        Self::require_case(&self.connection, actor, case)?;
        let (sha, length) = self.publish_bytes(bytes)?;
        let transaction = self.transaction()?;
        Self::require_case(&transaction, actor, case)?;
        Self::reference(&transaction, &sha, length, "case_artifact")?;
        sql(transaction.execute(
            "INSERT OR IGNORE INTO case_artifact(case_id,sha256) VALUES(?1,?2)",
            params![case, sha],
        ))?;
        sql(transaction.commit())?;
        Ok(sha)
    }

    /// Read an exact verified artifact through its authenticated case scope.
    pub fn get_bytes(&self, actor: &str, case: &str, sha: &str) -> Result<Vec<u8>> {
        Self::require_case(&self.connection, actor, case)?;
        self.require_artifact(&self.connection, case, sha)?;
        self.read_blob_bytes(sha)
    }

    /// Reads a case-scoped artifact only when the supplied identity is bound
    /// to this case. A missing binding is an ordinary non-match; a bound but
    /// missing or corrupt blob remains an integrity failure.
    pub fn get_bytes_if_present(
        &self,
        actor: &str,
        case: &str,
        sha: &str,
    ) -> Result<Option<Vec<u8>>> {
        Self::require_case(&self.connection, actor, case)?;
        let available: bool = sql(self.connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM case_artifact WHERE case_id=?1 AND sha256=?2)",
            params![case, sha],
            |row| row.get(0),
        ))?;
        if !available {
            return Ok(None);
        }
        Ok(Some(self.read_blob_bytes(sha)?))
    }

    fn require_artifact(&self, connection: &Connection, case: &str, sha: &str) -> Result<()> {
        let available: bool = sql(connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM case_artifact WHERE case_id=?1 AND sha256=?2)",
            params![case, sha],
            |r| r.get(0),
        ))?;
        if !available {
            return Err(failure(
                "INTEGRITY_FAILURE",
                "artifact is not durably bound to this case",
            ));
        }
        self.read_blob_bytes(sha)?;
        Ok(())
    }

    pub fn register_input(
        &self,
        actor: &str,
        request: &RequestEnvelope,
        fingerprint: &str,
    ) -> Result<bool> {
        valid_id(&request.input_id)?;
        valid_id(fingerprint)?;
        if actor != request.actor_id {
            return Err(failure(
                "ACCESS_DENIED",
                "request actor differs from authenticated actor",
            ));
        }
        Self::actor_for_case(&self.connection, actor, &request.case_id)?;
        let (sha, length) = self.publish_blob(request)?;
        let transaction = self.transaction()?;
        if !Self::actor_for_case(&transaction, actor, &request.case_id)? {
            sql(transaction.execute(
                "INSERT INTO case_registry(case_id,actor_id) VALUES(?1,?2)",
                params![request.case_id, actor],
            ))?;
        }
        let existing: Option<(String, String)> = sql(transaction.query_row(
            "SELECT fingerprint,request_sha256 FROM input_identity WHERE case_id=?1 AND input_id=?2 AND input_version=?3",
            params![request.case_id, request.input_id, integer(request.input_version)?],
            |row| Ok((row.get(0)?, row.get(1)?)),
        ).optional())?;
        if let Some((original_fingerprint, original_request)) = existing {
            if original_fingerprint != fingerprint || original_request != sha {
                // Immutable observation, not a replacement input or case judgment.
                Self::reference(&transaction, &sha, length, "conflicting_request")?;
                let previous: Option<(i64,String)> = sql(transaction.query_row(
                    "SELECT sequence,event_hash FROM input_conflict WHERE case_id=?1 ORDER BY sequence DESC LIMIT 1",
                    [&request.case_id],|r|Ok((r.get(0)?,r.get(1)?)),
                ).optional())?;
                let sequence = previous.as_ref().map_or(0, |(number, _)| number + 1);
                let parent_hash = previous.map(|(_, hash)| hash);
                let event_hash = identity(&(
                    "input-conflict-observation-v1",
                    &request.case_id,
                    sequence,
                    &parent_hash,
                    &request.input_id,
                    request.input_version,
                    &original_fingerprint,
                    fingerprint,
                    &original_request,
                    &sha,
                ))?;
                sql(transaction.execute(
                    "INSERT INTO input_conflict(case_id,sequence,input_id,input_version,accepted_fingerprint,attempt_fingerprint,accepted_request_sha256,attempt_request_sha256,parent_hash,event_hash) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",
                    params![request.case_id,sequence,request.input_id,integer(request.input_version)?,original_fingerprint,fingerprint,original_request,sha,parent_hash,event_hash],
                ))?;
                sql(transaction.commit())?;
                return Err(failure(
                    "INPUT_IDENTITY_COLLISION",
                    "same input identity has different body, context, actor or authority; conflict evidence retained",
                ));
            }
            self.read_blob_bytes(&original_request)?;
            return Ok(false);
        }
        Self::reference(&transaction, &sha, length, "request")?;
        sql(transaction.execute(
            "INSERT INTO input_identity(case_id,input_id,input_version,fingerprint,request_sha256) VALUES(?1,?2,?3,?4,?5)",
            params![request.case_id, request.input_id, integer(request.input_version)?, fingerprint, sha],
        ))?;
        sql(transaction.commit())?;
        Ok(true)
    }

    /// Original conflicting envelopes are visible only to the case owner.
    pub fn conflict_attempts(&self, actor: &str, case: &str) -> Result<Vec<RequestEnvelope>> {
        if !Self::actor_for_case(&self.connection, actor, case)? {
            return Ok(Vec::new());
        }
        self.verify_conflicts(case)?;
        let mut statement = sql(self.connection.prepare(
            "SELECT attempt_request_sha256 FROM input_conflict WHERE case_id=?1 ORDER BY sequence",
        ))?;
        let rows = sql(statement.query_map([case], |r| r.get::<_, String>(0)))?;
        rows.map(|row| self.read_blob(&sql(row)?)).collect()
    }

    pub fn load_case(&self, actor: &str, case: &str) -> Result<Option<CaseRevision>> {
        if !Self::actor_for_case(&self.connection, actor, case)? {
            return Ok(None);
        }
        self.verify_case(case)?;
        self.latest_case(case)
    }

    fn latest_case(&self, case: &str) -> Result<Option<CaseRevision>> {
        let sha: Option<String> = sql(self.connection.query_row(
            "SELECT state_sha256 FROM case_revision WHERE case_id=?1 ORDER BY revision DESC LIMIT 1", [case], |r| r.get(0),
        ).optional())?;
        sha.map(|sha| self.read_blob(&sha)).transpose()
    }

    pub fn append_case(&self, actor: &str, revision: &CaseRevision) -> Result<()> {
        if actor != revision.actor_id || revision.schema_version != SCHEMA_VERSION {
            return Err(failure(
                "ACCESS_DENIED",
                "revision actor or schema does not match",
            ));
        }
        Self::require_case(&self.connection, actor, &revision.case_id)?;
        self.verify_case(&revision.case_id)?;
        let (sha, length) = self.publish_blob(revision)?;
        let bundle = revision
            .bundle
            .as_ref()
            .map(|bundle| self.publish_blob(bundle))
            .transpose()?;
        let result = revision
            .result
            .as_ref()
            .map(|result| self.publish_blob(result))
            .transpose()?;
        let transaction = self.transaction()?;
        Self::require_case(&transaction, actor, &revision.case_id)?;
        let registered = if let Some(fingerprint) = &revision.input_fingerprint {
            sql(transaction.query_row(
                "SELECT EXISTS(SELECT 1 FROM input_identity WHERE case_id=?1 AND fingerprint=?2)",
                params![revision.case_id, fingerprint],
                |r| r.get::<_, bool>(0),
            ))?
        } else {
            false
        };
        if !registered {
            return Err(failure(
                "INPUT_NOT_REGISTERED",
                "revision has no registered original input",
            ));
        }
        let prior: Option<(i64, String, String)> = sql(transaction.query_row(
            "SELECT revision,state_sha256,event_hash FROM case_revision WHERE case_id=?1 ORDER BY revision DESC LIMIT 1",
            [&revision.case_id], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        ).optional())?;
        match &prior {
            None => {
                if revision.revision != 0
                    || revision.parent_revision.is_some()
                    || revision.lifecycle != Lifecycle::Received
                {
                    return Err(failure(
                        "REVISION_CONFLICT",
                        "first revision must be pre-goal RECEIVED revision zero",
                    ));
                }
            }
            Some((number, prior_sha, _)) => {
                if revision.revision != *number as u64 + 1
                    || revision.parent_revision != Some(*number as u64)
                {
                    return Err(failure(
                        "REVISION_CONFLICT",
                        "case revision compare-and-swap failed",
                    ));
                }
                let old: CaseRevision = self.read_blob(prior_sha)?;
                Self::validate_transition(&old, revision)?;
            }
        }
        if matches!(
            revision.lifecycle,
            Lifecycle::Running | Lifecycle::Adopted | Lifecycle::Commit
        ) && revision.goal_sha256.is_none()
        {
            return Err(failure(
                "GOAL_NOT_BOUND",
                "running/adopted/committed revision needs a bound goal",
            ));
        }
        if matches!(
            revision.lifecycle,
            Lifecycle::Sealed | Lifecycle::Running | Lifecycle::Adopted | Lifecycle::Commit
        ) && revision.bundle.is_none()
        {
            return Err(failure(
                "INVALID_SEAL",
                "active revision requires a sealed bundle",
            ));
        }
        if revision.lifecycle == Lifecycle::Received
            && (revision.bundle.is_some() || revision.goal_sha256.is_some())
        {
            return Err(failure(
                "INVALID_SEAL",
                "RECEIVED cannot carry an invented sealed bundle",
            ));
        }
        if let Some(bundle_value) = &revision.bundle {
            if bundle_value.request.case_id != revision.case_id
                || bundle_value.request.actor_id != actor
            {
                return Err(failure(
                    "INVALID_SEAL",
                    "bundle identity differs from case revision",
                ));
            }
            let original: Option<(String,String)> = sql(transaction.query_row(
                "SELECT fingerprint,request_sha256 FROM input_identity WHERE case_id=?1 AND input_id=?2 AND input_version=?3",
                params![revision.case_id, bundle_value.request.input_id, integer(bundle_value.request.input_version)?],
                |r| Ok((r.get(0)?,r.get(1)?)),
            ).optional())?;
            if !original.is_some_and(|(fingerprint, request_sha)| {
                revision.input_fingerprint.as_deref() == Some(fingerprint.as_str())
                    && canonical(&bundle_value.request)
                        .is_ok_and(|bytes| digest(&bytes) == request_sha)
            }) {
                return Err(failure(
                    "INVALID_SEAL",
                    "bundle has no matching registered original input",
                ));
            }
        }
        if let Some(goal) = &revision.goal_sha256 {
            self.require_artifact(&transaction, &revision.case_id, goal)?;
        }
        Self::reference(&transaction, &sha, length, "case_revision")?;
        if let Some((bundle_sha, bundle_length)) = &bundle {
            Self::reference(&transaction, bundle_sha, *bundle_length, "input_bundle")?;
            let request = &revision
                .bundle
                .as_ref()
                .expect("bundle tuple comes from value")
                .request;
            sql(transaction.execute(
                "UPDATE input_identity SET bundle_sha256=?4 WHERE case_id=?1 AND input_id=?2 AND input_version=?3",
                params![revision.case_id, request.input_id, integer(request.input_version)?, bundle_sha],
            ))?;
        }
        if let Some((result_sha, result_length)) = &result {
            let value = revision
                .result
                .as_ref()
                .expect("result tuple comes from value");
            Self::reference(&transaction, result_sha, *result_length, "result")?;
            let changed = sql(transaction.execute(
                "UPDATE input_identity SET result_sha256=?4 WHERE case_id=?1 AND input_id=?2 AND input_version=?3",
                params![revision.case_id, value.input_id, integer(value.input_version)?, result_sha],
            ))?;
            if changed != 1 {
                return Err(failure(
                    "INPUT_NOT_REGISTERED",
                    "result has no registered input identity",
                ));
            }
        }
        let parent_hash = prior.as_ref().map(|(_, _, hash)| hash.clone());
        let event_hash = identity(&(
            "case-revision-event-v1",
            &revision.case_id,
            revision.revision,
            &parent_hash,
            &sha,
        ))?;
        sql(transaction.execute(
            "INSERT INTO case_revision(case_id,revision,parent_revision,state_sha256,parent_hash,event_hash,lifecycle,goal_sha256) VALUES(?1,?2,?3,?4,?5,?6,?7,?8)",
            params![revision.case_id, integer(revision.revision)?, revision.parent_revision.map(integer).transpose()?, sha, parent_hash, event_hash,
                serde_json::to_value(revision.lifecycle)?.as_str().unwrap_or("INVALID"), revision.goal_sha256],
        ))?;
        sql(transaction.commit())?;
        Ok(())
    }

    fn validate_transition(old: &CaseRevision, new: &CaseRevision) -> Result<()> {
        if !old.lifecycle.permits(new.lifecycle) {
            return Err(failure(
                "INVALID_TRANSITION",
                format!(
                    "{:?} cannot transition to {:?}",
                    old.lifecycle, new.lifecycle
                ),
            ));
        }
        let new_input = old.input_fingerprint != new.input_fingerprint;
        if new.lifecycle == Lifecycle::Received && !new_input {
            return Err(failure(
                "INVALID_INPUT_REVISION",
                "same accepted input cannot restart reception",
            ));
        }
        for (id, result) in &old.prior_results {
            if new.prior_results.get(id) != Some(result) {
                return Err(failure(
                    "IMMUTABLE_HISTORY",
                    "prior result history cannot be erased or changed",
                ));
            }
        }
        if new_input {
            if new.lifecycle != Lifecycle::Received
                || new.bundle.is_some()
                || new.goal_sha256.is_some()
            {
                return Err(failure(
                    "INVALID_INPUT_REVISION",
                    "new input must return to RECEIVED and re-seal",
                ));
            }
        } else {
            if old.goal_sha256.is_some() && old.goal_sha256 != new.goal_sha256 {
                return Err(failure(
                    "IMMUTABLE_GOAL",
                    "same accepted input cannot change its goal",
                ));
            }
            if let Some(old_bundle) = &old.bundle
                && new.bundle.as_ref() != Some(old_bundle)
            {
                return Err(failure(
                    "IMMUTABLE_SEAL",
                    "sealed input cannot change within its input version",
                ));
            }
        }
        Ok(())
    }

    pub fn get_input_result(
        &self,
        actor: &str,
        case: &str,
        input: &str,
        version: u64,
    ) -> Result<Option<ResultRecord>> {
        if !Self::actor_for_case(&self.connection, actor, case)? {
            return Ok(None);
        }
        self.verify_case(case)?;
        let sha: Option<String> = sql(self.connection.query_row(
            "SELECT result_sha256 FROM input_identity WHERE case_id=?1 AND input_id=?2 AND input_version=?3",
            params![case, input, integer(version)?], |r| r.get(0),
        ).optional())?.flatten();
        sha.map(|sha| self.read_blob(&sha)).transpose()
    }

    /// Load the exact immutable sealed input for one retained case input.
    /// Actor/case/input identity and content-derived bundle fields are
    /// rechecked before a consumer can use the original interpretation.
    pub(crate) fn get_input_bundle(
        &self,
        actor: &str,
        case: &str,
        input: &str,
        version: u64,
    ) -> Result<Option<(String, InputBundle)>> {
        if !Self::actor_for_case(&self.connection, actor, case)? {
            return Ok(None);
        }
        self.verify_case(case)?;
        let registered: Option<(String, String, Option<String>)> = sql(self.connection.query_row(
            "SELECT fingerprint,request_sha256,bundle_sha256 FROM input_identity WHERE case_id=?1 AND input_id=?2 AND input_version=?3",
            params![case, input, integer(version)?],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        ).optional())?;
        let Some((request_fingerprint, request_sha, Some(bundle_sha))) = registered else {
            return Ok(None);
        };
        let bundle: InputBundle = self.read_blob(&bundle_sha)?;
        let request_bytes = self.read_blob_bytes(&request_sha)?;
        let expected_bundle_fingerprint = identity(&(
            &bundle.request,
            &bundle.raw_sha256,
            &bundle.context_sha256,
            bundle.meaning.parser_version.as_str(),
        ))?;
        if bundle.request.actor_id != actor
            || bundle.request.case_id != case
            || bundle.request.input_id != input
            || bundle.request.input_version != version
            || canonical(&bundle.request)? != request_bytes
            || identity(&bundle.request)? != request_fingerprint
            || bundle.raw_sha256 != digest(bundle.request.text.as_bytes())
            || bundle.fingerprint != expected_bundle_fingerprint
            || bundle.semantic_signature != identity(&bundle.meaning)?
        {
            return Err(failure(
                "INPUT_BUNDLE_INTEGRITY",
                "retained sealed input does not match its immutable case identity",
            ));
        }
        Ok(Some((bundle_sha, bundle)))
    }

    pub fn save_action(&self, actor: &str, action: &ActionRecord) -> Result<()> {
        if actor != action.actor_id {
            return Err(failure(
                "ACCESS_DENIED",
                "action actor differs from authenticated actor",
            ));
        }
        Self::require_case(&self.connection, actor, &action.case_id)?;
        for value in [
            &action.action_id,
            &action.idempotency_key,
            &action.input_fingerprint,
            &action.target,
        ] {
            valid_id(value)?;
        }
        if !valid_digest(&action.authorization_sha256)
            || !valid_digest(&action.exact_request_sha256)
        {
            return Err(failure(
                "INVALID_ACTION_IDENTITY",
                "request and authorization need exact digests",
            ));
        }
        if let Some(receipt) = &action.receipt
            && (receipt.action_id != action.action_id
                || receipt.target != action.target
                || receipt.phase != action.phase
                || (action.effect_class == "LOCAL_WRITE"
                    && receipt.phase == EffectPhase::Confirmed
                    && receipt.content_sha256 != action.exact_request_sha256))
        {
            return Err(failure(
                "INVALID_RECEIPT",
                "receipt does not match action identity and phase",
            ));
        }
        if action.phase == EffectPhase::Confirmed && action.receipt.is_none() {
            return Err(failure(
                "INVALID_RECEIPT",
                "CONFIRMED requires an actual effect receipt",
            ));
        }
        let (sha, length) = self.publish_blob(action)?;
        let transaction = self.transaction()?;
        Self::require_case(&transaction, actor, &action.case_id)?;
        let old: Option<(i64, String, String)> = sql(transaction.query_row(
            "SELECT sequence,record_sha256,event_hash FROM action_history WHERE case_id=?1 AND action_id=?2 ORDER BY sequence DESC LIMIT 1",
            params![action.case_id, action.action_id], |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?)),
        ).optional())?;
        let idempotency_owner: Option<String> = sql(transaction
            .query_row(
                "SELECT action_id FROM action_identity WHERE case_id=?1 AND idempotency_key=?2",
                params![action.case_id, action.idempotency_key],
                |row| row.get(0),
            )
            .optional())?;
        if idempotency_owner
            .as_deref()
            .is_some_and(|owner| owner != action.action_id)
        {
            return Err(failure(
                "ACTION_IDENTITY_COLLISION",
                "idempotency key already belongs to a different action",
            ));
        }
        let mut sequence = 0;
        if let Some((number, prior_sha, _)) = &old {
            let prior: ActionRecord = self.read_blob(prior_sha)?;
            if prior == *action {
                return Ok(());
            }
            let mut unchanged = action.clone();
            unchanged.phase = prior.phase;
            unchanged.receipt.clone_from(&prior.receipt);
            if unchanged != prior {
                return Err(failure(
                    "ACTION_IDENTITY_COLLISION",
                    "an action cannot change exact request, authority, target or idempotency identity",
                ));
            }
            if !prior.phase.permits(action.phase) {
                return Err(failure(
                    "INVALID_EFFECT_TRANSITION",
                    "effect phase cannot be retried or downgraded",
                ));
            }
            sequence = number + 1;
        } else if action.phase != EffectPhase::Proposed {
            return Err(failure(
                "INVALID_EFFECT_TRANSITION",
                "new action must begin PROPOSED",
            ));
        }
        let registered: bool = sql(transaction.query_row(
            "SELECT EXISTS(SELECT 1 FROM input_identity WHERE case_id=?1 AND fingerprint=?2)",
            params![action.case_id, action.input_fingerprint],
            |r| r.get(0),
        ))?;
        if !registered {
            return Err(failure(
                "INPUT_NOT_REGISTERED",
                "action has no accepted input fingerprint",
            ));
        }
        self.require_artifact(&transaction, &action.case_id, &action.authorization_sha256)?;
        self.require_artifact(&transaction, &action.case_id, &action.exact_request_sha256)?;
        Self::reference(&transaction, &sha, length, "action")?;
        let parent_hash = old.as_ref().map(|(_, _, hash)| hash.clone());
        let event_hash = identity(&(
            "action-event-v1",
            &action.case_id,
            &action.action_id,
            sequence,
            &parent_hash,
            &sha,
        ))?;
        if old.is_none() {
            sql(transaction.execute(
                "INSERT INTO action_identity(case_id,action_id,idempotency_key) VALUES(?1,?2,?3)",
                params![action.case_id, action.action_id, action.idempotency_key],
            ))?;
        }
        sql(transaction.execute(
            "INSERT INTO action_history(case_id,action_id,sequence,record_sha256,parent_hash,event_hash) VALUES(?1,?2,?3,?4,?5,?6)",
            params![action.case_id,action.action_id,sequence,sha,parent_hash,event_hash],
        ))?;
        sql(transaction.commit())?;
        Ok(())
    }

    pub fn actions(&self, actor: &str, case: &str) -> Result<Vec<ActionRecord>> {
        if !Self::actor_for_case(&self.connection, actor, case)? {
            return Ok(Vec::new());
        }
        self.verify_actions(case)?;
        let mut statement = sql(self.connection.prepare(
            "SELECT h.record_sha256 FROM action_history h WHERE h.case_id=?1 AND h.sequence=(SELECT MAX(i.sequence) FROM action_history i WHERE i.case_id=h.case_id AND i.action_id=h.action_id) ORDER BY h.action_id",
        ))?;
        let rows = sql(statement.query_map([case], |r| r.get::<_, String>(0)))?;
        rows.map(|row| {
            let mut action: ActionRecord = self.read_blob(&sql(row)?)?;
            // A saved ATTEMPTED without a durable confirmation is ambiguous.
            // Return a recovery observation; Core must reconcile the same action
            // identity. Storage never invokes a port or manufactures a token.
            if action.phase == EffectPhase::Attempted {
                action.phase = EffectPhase::Unknown;
                action.receipt = None;
            }
            Ok(action)
        })
        .collect()
    }

    fn validate_memory_scope(&self, actor: &str, memory: &MemoryState) -> Result<()> {
        let ids: BTreeSet<&str> = memory
            .facts
            .iter()
            .map(|f| f.id.as_str())
            .chain(memory.rules.iter().map(|r| r.id.as_str()))
            .collect();
        if ids.len() != memory.facts.len() + memory.rules.len() {
            return Err(failure(
                "INTEGRITY_FAILURE",
                "duplicate actor memory identity",
            ));
        }
        for (source, dependencies) in memory
            .facts
            .iter()
            .map(|f| (&f.source_input, &f.dependencies))
            .chain(
                memory
                    .rules
                    .iter()
                    .map(|r| (&r.source_input, &r.dependencies)),
            )
        {
            if dependencies.iter().any(|id| !ids.contains(id.as_str())) {
                return Err(failure(
                    "ACCESS_DENIED",
                    "memory references a dependency outside this actor projection",
                ));
            }
            if source == "derived:proof-composition-v1" && !dependencies.is_empty() {
                continue;
            }
            let owned:bool = sql(self.connection.query_row(
                "SELECT EXISTS(SELECT 1 FROM input_identity i JOIN case_registry c ON c.case_id=i.case_id WHERE c.actor_id=?1 AND i.fingerprint=?2)",
                params![actor,source],|r|r.get(0),
            ))?;
            if !owned {
                return Err(failure(
                    "ACCESS_DENIED",
                    "memory provenance is not an original input owned by this actor",
                ));
            }
        }
        for fact in &memory.facts {
            let Some(evidence) = &fact.source_evidence else {
                continue;
            };
            let source_key: Option<(String, String, i64)> = sql(self.connection.query_row(
                "SELECT i.case_id,i.input_id,i.input_version FROM input_identity i JOIN case_registry c ON c.case_id=i.case_id WHERE c.actor_id=?1 AND i.fingerprint=?2",
                params![actor, fact.source_input],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            ).optional())?;
            let Some((case, input, version)) = source_key else {
                return Err(failure(
                    "SOURCE_EVIDENCE_UNOWNED",
                    "fact source evidence does not resolve to an actor-owned sealed input",
                ));
            };
            let version = u64::try_from(version).map_err(|_| {
                failure(
                    "SOURCE_EVIDENCE_IDENTITY",
                    "fact source evidence has a negative input version",
                )
            })?;
            let Some((_, bundle)) = self.get_input_bundle(actor, &case, &input, version)? else {
                return Err(failure(
                    "SOURCE_EVIDENCE_UNSEALED",
                    "fact source evidence does not resolve to a retained sealed input",
                ));
            };
            if crate::input::assertion_source_evidence(&bundle)?.as_ref() != Some(evidence) {
                return Err(failure(
                    "SOURCE_EVIDENCE_MISMATCH",
                    "fact source evidence differs from its immutable sealed input",
                ));
            }
        }
        Ok(())
    }

    pub fn load_memory(&self, actor: &str) -> Result<MemoryState> {
        valid_id(actor)?;
        let sha: Option<String> = sql(self.connection.query_row(
            "SELECT record_sha256 FROM memory_revision WHERE actor_id=?1 ORDER BY revision DESC LIMIT 1", [actor], |r| r.get(0),
        ).optional())?;
        let memory = sha
            .map(|sha| self.read_blob(&sha))
            .transpose()?
            .unwrap_or_default();
        self.validate_memory_scope(actor, &memory)?;
        Ok(memory)
    }

    pub fn save_memory(&self, actor: &str, memory: &MemoryState) -> Result<()> {
        valid_id(actor)?;
        self.validate_memory_scope(actor, memory)?;
        let (sha, length) = self.publish_blob(memory)?;
        let transaction = self.transaction()?;
        let old: Option<(i64,String)> = sql(transaction.query_row(
            "SELECT revision,record_sha256 FROM memory_revision WHERE actor_id=?1 ORDER BY revision DESC LIMIT 1", [actor], |r| Ok((r.get(0)?,r.get(1)?)),
        ).optional())?;
        if let Some((revision, old_sha)) = &old {
            if *old_sha == sha {
                return Ok(());
            }
            if memory.revision != *revision as u64 + 1 {
                return Err(failure(
                    "REVISION_CONFLICT",
                    "actor memory revision compare-and-swap failed",
                ));
            }
        } else if memory.revision > 1 {
            return Err(failure(
                "REVISION_CONFLICT",
                "first memory revision must be zero or one",
            ));
        }
        Self::reference(&transaction, &sha, length, "memory")?;
        sql(transaction.execute(
            "INSERT INTO memory_revision(actor_id,revision,record_sha256) VALUES(?1,?2,?3)",
            params![actor, integer(memory.revision)?, sha],
        ))?;
        sql(transaction.commit())?;
        Ok(())
    }

    /// Reports orphan published content for diagnostics. It does not delete
    /// evidence, rewrite history, or adopt uncommitted content.
    pub fn orphan_blobs(&self) -> Result<Vec<String>> {
        let mut statement = sql(self.connection.prepare("SELECT sha256 FROM artifact_ref"))?;
        let referenced: BTreeSet<String> = sql(statement.query_map([], |r| r.get(0)))?
            .map(sql)
            .collect::<Result<_>>()?;
        let mut orphans = Vec::new();
        for entry in fs::read_dir(&self.blobs)? {
            let name = entry?.file_name().to_string_lossy().into_owned();
            if valid_digest(&name) && !referenced.contains(&name) {
                self.read_blob_bytes(&name)?;
                orphans.push(name);
            }
        }
        orphans.sort();
        Ok(orphans)
    }

    fn verify_integrity(&self) -> Result<()> {
        let check: String = sql(self
            .connection
            .query_row("PRAGMA quick_check", [], |r| r.get(0)))?;
        if check != "ok" {
            return Err(failure("INTEGRITY_FAILURE", check));
        }
        let mut foreign = sql(self.connection.prepare("PRAGMA foreign_key_check"))?;
        if sql(foreign.query([]))?
            .next()
            .map_err(|e| failure("STORE_SQLITE", e.to_string()))?
            .is_some()
        {
            return Err(failure(
                "INTEGRITY_FAILURE",
                "foreign key reference is invalid",
            ));
        }
        let mut statement = sql(self
            .connection
            .prepare("SELECT sha256,byte_length FROM artifact_ref"))?;
        for row in
            sql(statement.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?))))?
        {
            let (sha, length) = sql(row)?;
            if self.read_blob_bytes(&sha)?.len() as i64 != length {
                return Err(failure(
                    "INTEGRITY_FAILURE",
                    "referenced byte length differs",
                ));
            }
        }
        let mut cases = sql(self.connection.prepare("SELECT case_id FROM case_registry"))?;
        for row in sql(cases.query_map([], |r| r.get::<_, String>(0)))? {
            let case = sql(row)?;
            self.verify_case(&case)?;
            self.verify_actions(&case)?;
            self.verify_conflicts(&case)?;
        }
        Ok(())
    }

    fn verify_conflicts(&self, case: &str) -> Result<()> {
        let mut statement=sql(self.connection.prepare(
            "SELECT sequence,input_id,input_version,accepted_fingerprint,attempt_fingerprint,accepted_request_sha256,attempt_request_sha256,parent_hash,event_hash FROM input_conflict WHERE case_id=?1 ORDER BY sequence",
        ))?;
        let rows = sql(statement.query_map([case], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, i64>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, String>(4)?,
                r.get::<_, String>(5)?,
                r.get::<_, String>(6)?,
                r.get::<_, Option<String>>(7)?,
                r.get::<_, String>(8)?,
            ))
        }))?;
        let mut previous_hash = None;
        for (expected, row) in rows.enumerate() {
            let (
                sequence,
                input,
                version,
                accepted_fingerprint,
                attempt_fingerprint,
                accepted_request,
                attempt_request,
                parent_hash,
                event_hash,
            ) = sql(row)?;
            if sequence != expected as i64
                || parent_hash != previous_hash
                || event_hash
                    != identity(&(
                        "input-conflict-observation-v1",
                        case,
                        sequence,
                        &parent_hash,
                        &input,
                        version as u64,
                        &accepted_fingerprint,
                        &attempt_fingerprint,
                        &accepted_request,
                        &attempt_request,
                    ))?
            {
                return Err(failure(
                    "INTEGRITY_FAILURE",
                    "input-conflict observation chain differs",
                ));
            }
            self.read_blob_bytes(&accepted_request)?;
            self.read_blob_bytes(&attempt_request)?;
            previous_hash = Some(event_hash);
        }
        Ok(())
    }

    fn verify_case(&self, case: &str) -> Result<()> {
        let mut statement = sql(self.connection.prepare(
            "SELECT revision,state_sha256,parent_hash,event_hash FROM case_revision WHERE case_id=?1 ORDER BY revision",
        ))?;
        let rows = sql(statement.query_map([case], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, Option<String>>(2)?,
                r.get::<_, String>(3)?,
            ))
        }))?;
        let mut previous_hash = None;
        let mut previous: Option<CaseRevision> = None;
        for (expected, row) in rows.enumerate() {
            let (number, sha, parent_hash, event_hash) = sql(row)?;
            if number != expected as i64 || parent_hash != previous_hash {
                return Err(failure(
                    "INTEGRITY_FAILURE",
                    "case journal sequence or parent differs",
                ));
            }
            let calculated = identity(&(
                "case-revision-event-v1",
                case,
                number as u64,
                &parent_hash,
                &sha,
            ))?;
            if event_hash != calculated {
                return Err(failure(
                    "INTEGRITY_FAILURE",
                    "case event hash chain differs",
                ));
            }
            let revision: CaseRevision = self.read_blob(&sha)?;
            let owner: String = sql(self.connection.query_row(
                "SELECT actor_id FROM case_registry WHERE case_id=?1",
                [case],
                |r| r.get(0),
            ))?;
            if (revision.schema_version != SCHEMA_VERSION
                && revision.schema_version != LEGACY_SCHEMA_VERSION)
                || revision.actor_id != owner
                || revision.case_id != case
                || revision.revision != number as u64
                || revision.parent_revision != previous.as_ref().map(|p| p.revision)
            {
                return Err(failure(
                    "INTEGRITY_FAILURE",
                    "case body differs from journal identity",
                ));
            }
            if let Some(prior) = &previous {
                Self::validate_transition(prior, &revision)?;
            } else if revision.lifecycle != Lifecycle::Received {
                return Err(failure("INTEGRITY_FAILURE", "case did not start RECEIVED"));
            }
            previous_hash = Some(event_hash);
            previous = Some(revision);
        }
        Ok(())
    }

    fn verify_actions(&self, case: &str) -> Result<()> {
        let mut statement = sql(self.connection.prepare(
            "SELECT action_id,sequence,record_sha256,parent_hash,event_hash FROM action_history WHERE case_id=?1 ORDER BY action_id,sequence",
        ))?;
        let rows = sql(statement.query_map([case], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, i64>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, Option<String>>(3)?,
                r.get::<_, String>(4)?,
            ))
        }))?;
        let mut previous: Option<(String, i64, String)> = None;
        for row in rows {
            let (action_id, number, sha, parent_hash, event_hash) = sql(row)?;
            let prior = previous.as_ref().filter(|(id, _, _)| id == &action_id);
            if number != prior.map_or(0, |(_, number, _)| number + 1)
                || parent_hash != prior.map(|(_, _, hash)| hash.clone())
                || event_hash
                    != identity(&(
                        "action-event-v1",
                        case,
                        &action_id,
                        number,
                        &parent_hash,
                        &sha,
                    ))?
            {
                return Err(failure(
                    "INTEGRITY_FAILURE",
                    "action event sequence or hash chain differs",
                ));
            }
            let action: ActionRecord = self.read_blob(&sha)?;
            if action.case_id != case || action.action_id != action_id {
                return Err(failure(
                    "INTEGRITY_FAILURE",
                    "action body differs from journal identity",
                ));
            }
            let identity_key: Option<String> = sql(self
                .connection
                .query_row(
                    "SELECT idempotency_key FROM action_identity WHERE case_id=?1 AND action_id=?2",
                    params![case, action_id],
                    |row| row.get(0),
                )
                .optional())?;
            if identity_key.as_deref() != Some(action.idempotency_key.as_str()) {
                return Err(failure(
                    "INTEGRITY_FAILURE",
                    "action idempotency key differs from its immutable identity",
                ));
            }
            previous = Some((action_id, number, event_hash));
        }
        Ok(())
    }
}

const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS store_metadata(key TEXT PRIMARY KEY,value TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS artifact_ref(sha256 TEXT PRIMARY KEY CHECK(length(sha256)=64),byte_length INTEGER NOT NULL CHECK(byte_length>=0),kind TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS case_registry(case_id TEXT PRIMARY KEY,actor_id TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS case_artifact(case_id TEXT NOT NULL REFERENCES case_registry(case_id),sha256 TEXT NOT NULL REFERENCES artifact_ref(sha256),PRIMARY KEY(case_id,sha256));
CREATE TABLE IF NOT EXISTS input_identity(
 case_id TEXT NOT NULL REFERENCES case_registry(case_id),input_id TEXT NOT NULL,input_version INTEGER NOT NULL CHECK(input_version>=0),
 fingerprint TEXT NOT NULL,request_sha256 TEXT NOT NULL REFERENCES artifact_ref(sha256),
 bundle_sha256 TEXT REFERENCES artifact_ref(sha256),result_sha256 TEXT REFERENCES artifact_ref(sha256),
 PRIMARY KEY(case_id,input_id,input_version));
CREATE TABLE IF NOT EXISTS input_conflict(
 case_id TEXT NOT NULL REFERENCES case_registry(case_id),sequence INTEGER NOT NULL CHECK(sequence>=0),input_id TEXT NOT NULL,input_version INTEGER NOT NULL CHECK(input_version>=0),
 accepted_fingerprint TEXT NOT NULL,attempt_fingerprint TEXT NOT NULL,accepted_request_sha256 TEXT NOT NULL REFERENCES artifact_ref(sha256),attempt_request_sha256 TEXT NOT NULL REFERENCES artifact_ref(sha256),
 parent_hash TEXT,event_hash TEXT NOT NULL UNIQUE,PRIMARY KEY(case_id,sequence));
CREATE TABLE IF NOT EXISTS case_revision(
 case_id TEXT NOT NULL REFERENCES case_registry(case_id),revision INTEGER NOT NULL CHECK(revision>=0),parent_revision INTEGER,
 state_sha256 TEXT NOT NULL REFERENCES artifact_ref(sha256),parent_hash TEXT,event_hash TEXT NOT NULL UNIQUE,
 lifecycle TEXT NOT NULL CHECK(lifecycle IN ('RECEIVED','SEALED','RUNNING','ADOPTED','COMMIT','SUSPEND','REJECT','FAIL','CANCEL')),
 goal_sha256 TEXT REFERENCES artifact_ref(sha256),
 CHECK(lifecycle NOT IN ('RUNNING','ADOPTED','COMMIT') OR goal_sha256 IS NOT NULL),
 PRIMARY KEY(case_id,revision),FOREIGN KEY(case_id,parent_revision) REFERENCES case_revision(case_id,revision));
CREATE TABLE IF NOT EXISTS action_identity(
 case_id TEXT NOT NULL REFERENCES case_registry(case_id),action_id TEXT NOT NULL,idempotency_key TEXT NOT NULL,
 PRIMARY KEY(case_id,action_id),UNIQUE(case_id,idempotency_key));
CREATE TABLE IF NOT EXISTS action_history(
 case_id TEXT NOT NULL,action_id TEXT NOT NULL,sequence INTEGER NOT NULL CHECK(sequence>=0),
 record_sha256 TEXT NOT NULL REFERENCES artifact_ref(sha256),parent_hash TEXT,event_hash TEXT NOT NULL UNIQUE,
 PRIMARY KEY(case_id,action_id,sequence),FOREIGN KEY(case_id,action_id) REFERENCES action_identity(case_id,action_id));
CREATE TABLE IF NOT EXISTS memory_revision(
 actor_id TEXT NOT NULL,revision INTEGER NOT NULL CHECK(revision>=0),record_sha256 TEXT NOT NULL REFERENCES artifact_ref(sha256),PRIMARY KEY(actor_id,revision));
CREATE TRIGGER IF NOT EXISTS input_conflict_no_update BEFORE UPDATE ON input_conflict BEGIN SELECT RAISE(ABORT,'immutable conflict observation'); END;
CREATE TRIGGER IF NOT EXISTS input_conflict_no_delete BEFORE DELETE ON input_conflict BEGIN SELECT RAISE(ABORT,'immutable conflict observation'); END;
CREATE TRIGGER IF NOT EXISTS case_revision_no_update BEFORE UPDATE ON case_revision BEGIN SELECT RAISE(ABORT,'immutable case revision'); END;
CREATE TRIGGER IF NOT EXISTS case_revision_no_delete BEFORE DELETE ON case_revision BEGIN SELECT RAISE(ABORT,'immutable case revision'); END;
CREATE TRIGGER IF NOT EXISTS action_history_no_update BEFORE UPDATE ON action_history BEGIN SELECT RAISE(ABORT,'immutable action history'); END;
CREATE TRIGGER IF NOT EXISTS action_history_no_delete BEFORE DELETE ON action_history BEGIN SELECT RAISE(ABORT,'immutable action history'); END;
CREATE TRIGGER IF NOT EXISTS memory_revision_no_update BEFORE UPDATE ON memory_revision BEGIN SELECT RAISE(ABORT,'immutable memory revision'); END;
CREATE TRIGGER IF NOT EXISTS memory_revision_no_delete BEFORE DELETE ON memory_revision BEGIN SELECT RAISE(ABORT,'immutable memory revision'); END;
CREATE TRIGGER IF NOT EXISTS case_owner_no_update BEFORE UPDATE ON case_registry BEGIN SELECT RAISE(ABORT,'immutable case owner'); END;
CREATE TRIGGER IF NOT EXISTS input_identity_no_rebind BEFORE UPDATE OF fingerprint,request_sha256 ON input_identity BEGIN SELECT RAISE(ABORT,'immutable input identity'); END;
CREATE TRIGGER IF NOT EXISTS input_identity_no_delete BEFORE DELETE ON input_identity BEGIN SELECT RAISE(ABORT,'durable input tombstone'); END;
CREATE TRIGGER IF NOT EXISTS artifact_no_update BEFORE UPDATE ON artifact_ref BEGIN SELECT RAISE(ABORT,'immutable artifact'); END;
"#;

#[cfg(test)]
mod tests {
    use super::*;
    use minidora_domain::{InputBundle, Intent, MeaningIR, Permissions};
    use std::collections::BTreeMap;
    use std::process::Command;
    use std::time::Instant;

    struct Directory(PathBuf);
    impl Directory {
        fn new() -> Self {
            let serial = TEMP_SERIAL.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!(
                "minidora-store-test-{}-{serial}",
                std::process::id()
            ));
            fs::create_dir(&path).expect("unique test directory");
            Self(path)
        }
    }
    impl Drop for Directory {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    fn request() -> RequestEnvelope {
        RequestEnvelope::new("case-a", "input-a", "alice", "2+3")
    }
    fn received(request: &RequestEnvelope) -> CaseRevision {
        CaseRevision {
            schema_version: SCHEMA_VERSION.into(),
            case_id: request.case_id.clone(),
            actor_id: request.actor_id.clone(),
            revision: 0,
            parent_revision: None,
            lifecycle: Lifecycle::Received,
            goal_sha256: None,
            input_fingerprint: Some(identity(request).unwrap()),
            bundle: None,
            result: None,
            reason: None,
            prior_results: BTreeMap::new(),
        }
    }
    fn register(store: &Store, request: &RequestEnvelope) -> CaseRevision {
        assert!(
            store
                .register_input("alice", request, &identity(request).unwrap())
                .unwrap()
        );
        let revision = received(request);
        store.append_case("alice", &revision).unwrap();
        revision
    }
    fn sealed(store: &Store, request: &RequestEnvelope, prior: &CaseRevision) -> CaseRevision {
        let mut next = prior.clone();
        next.revision += 1;
        next.parent_revision = Some(prior.revision);
        next.lifecycle = Lifecycle::Sealed;
        next.bundle = Some(InputBundle {
            request: request.clone(),
            raw_sha256: digest(request.text.as_bytes()),
            context_sha256: digest(b"context"),
            fingerprint: identity(&(request, "actual-resolved-context")).unwrap(),
            semantic_signature: identity(&request.text).unwrap(),
            meaning: MeaningIR {
                parser_version: "test".into(),
                intent: Intent::Calculate {
                    expression: "2+3".into(),
                },
                spans: vec![],
                source_roles: std::collections::BTreeMap::new(),
                unresolved: vec![],
                source_kind: "USER".into(),
                context_result_ref: None,
            },
        });
        next.goal_sha256 = Some(
            store
                .put_artifact("alice", "case-a", &"exact-test-goal")
                .unwrap(),
        );
        next
    }

    #[test]
    fn retained_input_bundle_is_loaded_by_exact_actor_case_input_and_version() {
        let directory = Directory::new();
        let store = Store::open(&directory.0).unwrap();
        let request = RequestEnvelope::new(
            "case-a",
            "input-a",
            "alice",
            "json-table-select \"name\":\n{\"columns\":[\"name\"],\"rows\":[[\"Mina\"]]}",
        );
        let fingerprint = identity(&request).unwrap();
        assert!(
            store
                .register_input("alice", &request, &fingerprint)
                .unwrap()
        );
        let received = received(&request);
        store.append_case("alice", &received).unwrap();
        let bundle = crate::input::seal(&request, None).unwrap();
        let bundle_ref = digest(&canonical(&bundle).unwrap());
        let mut sealed = received;
        sealed.revision = 1;
        sealed.parent_revision = Some(0);
        sealed.lifecycle = Lifecycle::Sealed;
        sealed.bundle = Some(bundle.clone());
        sealed.goal_sha256 = Some(
            store
                .put_artifact("alice", "case-a", &"exact-retained-input-test-goal")
                .unwrap(),
        );
        store.append_case("alice", &sealed).unwrap();

        let (loaded_ref, loaded) = store
            .get_input_bundle("alice", "case-a", "input-a", 1)
            .unwrap()
            .expect("the exact sealed input is retained");
        assert_eq!(loaded_ref, bundle_ref);
        assert_eq!(loaded, bundle);
        assert!(
            store
                .get_input_bundle("alice", "case-a", "missing", 1)
                .unwrap()
                .is_none()
        );
        let error = store
            .get_input_bundle("bob", "case-a", "input-a", 1)
            .unwrap_err();
        assert!(matches!(
            error,
            minidora_domain::Error::Contract { code, .. } if code == "ACCESS_DENIED"
        ));
    }

    fn run_store_workers(store_directory: &Path, mode: &str, records: [Vec<u8>; 2]) -> Vec<String> {
        let scenario = store_directory.join(format!("race-{mode}"));
        fs::create_dir(&scenario).unwrap();
        let gate = scenario.join("start");
        let mut workers = Vec::new();
        let mut ready_paths = Vec::new();
        let mut result_paths = Vec::new();
        for (worker, record) in records.iter().enumerate() {
            let record_path = scenario.join(format!("record-{worker}.json"));
            fs::write(&record_path, record).unwrap();
            let ready = scenario.join(format!("ready-{worker}"));
            let result = scenario.join(format!("result-{worker}"));
            let child = Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "store::tests::multi_process_writer_worker",
                    "--nocapture",
                ])
                .env("MINIDORA_STORE_MP_DIR", store_directory)
                .env("MINIDORA_STORE_MP_MODE", mode)
                .env("MINIDORA_STORE_MP_RECORD", &record_path)
                .env("MINIDORA_STORE_MP_GATE", &gate)
                .env("MINIDORA_STORE_MP_READY", &ready)
                .env("MINIDORA_STORE_MP_RESULT", &result)
                .spawn()
                .unwrap();
            workers.push(child);
            ready_paths.push(ready);
            result_paths.push(result);
        }

        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            if ready_paths.iter().all(|path| path.exists()) {
                break;
            }
            for child in &mut workers {
                if let Some(status) = child.try_wait().unwrap() {
                    panic!("writer worker exited before the barrier: {status}");
                }
            }
            assert!(
                Instant::now() < deadline,
                "writer workers did not reach the barrier"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
        fs::write(&gate, b"go").unwrap();

        for child in workers {
            let output = child.wait_with_output().unwrap();
            assert!(
                output.status.success(),
                "writer process failed: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
        let mut outcomes: Vec<_> = result_paths
            .iter()
            .map(|path| fs::read_to_string(path).unwrap())
            .collect();
        outcomes.sort();
        outcomes
    }

    fn run_abrupt_store_worker(
        store_directory: &Path,
        worker_test: &str,
        record: Option<&[u8]>,
        expected_exit: i32,
    ) -> PathBuf {
        let scenario = store_directory.join(format!("crash-{worker_test}"));
        fs::create_dir(&scenario).unwrap();
        let gate = scenario.join("release");
        let ready = scenario.join("ready");
        let result = scenario.join("result");
        let record_path = scenario.join("record.json");
        if let Some(record) = record {
            fs::write(&record_path, record).unwrap();
        }
        let mut command = Command::new(std::env::current_exe().unwrap());
        command
            .args(["--exact", worker_test, "--nocapture"])
            .env("MINIDORA_STORE_CRASH_DIR", store_directory)
            .env("MINIDORA_STORE_CRASH_GATE", &gate)
            .env("MINIDORA_STORE_CRASH_READY", &ready)
            .env("MINIDORA_STORE_CRASH_RESULT", &result);
        if record.is_some() {
            command.env("MINIDORA_STORE_CRASH_RECORD", &record_path);
        }
        let mut child = command.spawn().unwrap();
        let deadline = Instant::now() + Duration::from_secs(10);
        while !ready.exists() {
            if let Some(status) = child.try_wait().unwrap() {
                panic!("abrupt worker exited before the barrier: {status}");
            }
            assert!(
                Instant::now() < deadline,
                "abrupt worker did not reach the barrier"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
        fs::write(&gate, b"exit now").unwrap();
        let output = child.wait_with_output().unwrap();
        assert_eq!(
            output.status.code(),
            Some(expected_exit),
            "unexpected abrupt-worker status: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        result
    }

    fn action(store: &Store, request: &RequestEnvelope) -> ActionRecord {
        ActionRecord {
            case_id: request.case_id.clone(),
            actor_id: request.actor_id.clone(),
            action_id: "write-a".into(),
            input_fingerprint: identity(request).unwrap(),
            idempotency_key: "stable-token".into(),
            effect_class: "LOCAL_WRITE".into(),
            target: "report.txt".into(),
            authorization_sha256: store
                .put_artifact("alice", "case-a", &Permissions::default())
                .unwrap(),
            exact_request_sha256: store
                .put_bytes("alice", "case-a", b"actual report")
                .unwrap(),
            phase: EffectPhase::Proposed,
            receipt: None,
        }
    }
    #[test]
    fn exact_input_tombstone_survives_restart_and_rejects_collision() {
        let directory = Directory::new();
        let original = request();
        {
            let store = Store::open(&directory.0).unwrap();
            register(&store, &original);
        }
        let store = Store::open(&directory.0).unwrap();
        assert!(
            !store
                .register_input("alice", &original, &identity(&original).unwrap())
                .unwrap()
        );
        let mut changed = original.clone();
        changed.text = "2+4".into();
        assert!(
            store
                .register_input("alice", &changed, &identity(&changed).unwrap())
                .is_err()
        );
        assert_eq!(
            store.conflict_attempts("alice", "case-a").unwrap(),
            vec![changed]
        );
        drop(store);
        let store = Store::open(&directory.0).unwrap();
        assert_eq!(store.conflict_attempts("alice", "case-a").unwrap().len(), 1);
        assert!(store.conflict_attempts("bob", "case-a").is_err());
        assert_eq!(
            store
                .load_case("alice", "case-a")
                .unwrap()
                .unwrap()
                .revision,
            0
        );
    }
    #[test]
    fn compare_and_swap_prevents_stale_writer() {
        let directory = Directory::new();
        let store = Store::open(&directory.0).unwrap();
        let req = request();
        let initial = register(&store, &req);
        let next = sealed(&store, &req, &initial);
        store.append_case("alice", &next).unwrap();
        assert!(store.append_case("alice", &next).is_err());
        assert_eq!(store.load_case("alice", "case-a").unwrap().unwrap(), next);
    }

    #[test]
    fn blob_staging_skips_a_stale_name_after_process_id_reuse() {
        let directory = Directory::new();
        let stale = directory.0.join(".stage-4242-41");
        fs::write(&stale, b"orphan from a crashed process").unwrap();
        let mut serials = [41, 42].into_iter();

        let (created, file) = create_stage_file_with(&directory.0, 4242, || {
            Ok(serials.next().expect("test provides a fallback serial"))
        })
        .unwrap();

        assert_eq!(created, directory.0.join(".stage-4242-42"));
        assert_eq!(fs::read(&stale).unwrap(), b"orphan from a crashed process");
        assert!(created.exists());
        drop(file);
    }

    #[test]
    fn separate_processes_serialize_case_revision_compare_and_swap() {
        let directory = Directory::new();
        let store = Store::open(&directory.0).unwrap();
        let request = request();
        let initial = register(&store, &request);
        let candidate = sealed(&store, &request, &initial);
        drop(store);
        let outcomes = run_store_workers(
            &directory.0,
            "append",
            [
                serde_json::to_vec(&candidate).unwrap(),
                serde_json::to_vec(&candidate).unwrap(),
            ],
        );
        assert_eq!(outcomes, ["ERR:REVISION_CONFLICT", "OK"]);
        assert_eq!(
            Store::open(&directory.0)
                .unwrap()
                .load_case("alice", "case-a")
                .unwrap()
                .unwrap(),
            candidate
        );
    }

    #[test]
    fn separate_processes_register_exact_input_once() {
        let directory = Directory::new();
        Store::open(&directory.0).unwrap();
        let request = request();
        let request_bytes = serde_json::to_vec(&request).unwrap();
        let outcomes = run_store_workers(
            &directory.0,
            "register",
            [request_bytes.clone(), request_bytes],
        );
        assert_eq!(outcomes, ["NEW", "REPLAY"]);

        let store = Store::open(&directory.0).unwrap();
        assert!(
            !store
                .register_input("alice", &request, &identity(&request).unwrap())
                .unwrap()
        );
    }

    #[test]
    fn separate_processes_retain_conflicting_input_attempt() {
        let directory = Directory::new();
        Store::open(&directory.0).unwrap();
        let accepted = request();
        let mut conflict = accepted.clone();
        conflict.text = "2+4".into();
        let outcomes = run_store_workers(
            &directory.0,
            "register",
            [
                serde_json::to_vec(&accepted).unwrap(),
                serde_json::to_vec(&conflict).unwrap(),
            ],
        );
        assert_eq!(outcomes, ["ERR:INPUT_IDENTITY_COLLISION", "NEW"]);
        let attempts = Store::open(&directory.0)
            .unwrap()
            .conflict_attempts("alice", "case-a")
            .unwrap();
        assert_eq!(attempts.len(), 1);
        assert!(attempts == vec![accepted] || attempts == vec![conflict]);
    }

    #[test]
    fn separate_processes_cannot_claim_one_case_for_different_actors() {
        let directory = Directory::new();
        Store::open(&directory.0).unwrap();
        let alice = request();
        let mut bob = alice.clone();
        bob.actor_id = "bob".into();
        let outcomes = run_store_workers(
            &directory.0,
            "register",
            [
                serde_json::to_vec(&alice).unwrap(),
                serde_json::to_vec(&bob).unwrap(),
            ],
        );
        assert_eq!(outcomes, ["ERR:ACCESS_DENIED", "NEW"]);

        let store = Store::open(&directory.0).unwrap();
        match store.register_input("alice", &alice, &identity(&alice).unwrap()) {
            Ok(false) => assert!(
                store
                    .register_input("bob", &bob, &identity(&bob).unwrap())
                    .is_err()
            ),
            Err(minidora_domain::Error::Contract { code, .. }) if code == "ACCESS_DENIED" => {
                assert!(
                    !store
                        .register_input("bob", &bob, &identity(&bob).unwrap())
                        .unwrap()
                );
            }
            other => panic!("unexpected owner observation: {other:?}"),
        }
    }

    #[test]
    fn separate_processes_save_identical_action_once() {
        let directory = Directory::new();
        let store = Store::open(&directory.0).unwrap();
        let request = request();
        register(&store, &request);
        let action = action(&store, &request);
        drop(store);

        let action_bytes = serde_json::to_vec(&action).unwrap();
        let outcomes = run_store_workers(
            &directory.0,
            "save-action",
            [action_bytes.clone(), action_bytes],
        );
        assert_eq!(outcomes, ["OK", "OK"]);
        assert_eq!(
            Store::open(&directory.0)
                .unwrap()
                .actions("alice", "case-a")
                .unwrap(),
            vec![action]
        );
    }

    #[test]
    fn separate_processes_reject_action_idempotency_collision() {
        let directory = Directory::new();
        let store = Store::open(&directory.0).unwrap();
        let request = request();
        register(&store, &request);
        let first = action(&store, &request);
        let mut second = first.clone();
        second.action_id = "write-b".into();
        drop(store);

        let outcomes = run_store_workers(
            &directory.0,
            "save-action-collision",
            [
                serde_json::to_vec(&first).unwrap(),
                serde_json::to_vec(&second).unwrap(),
            ],
        );
        assert_eq!(outcomes, ["ERR:ACTION_IDENTITY_COLLISION", "OK"]);
        let stored = Store::open(&directory.0)
            .unwrap()
            .actions("alice", "case-a")
            .unwrap();
        assert_eq!(stored.len(), 1);
        assert!(stored[0] == first || stored[0] == second);
    }

    #[test]
    fn multi_process_writer_worker() {
        let Ok(directory) = std::env::var("MINIDORA_STORE_MP_DIR") else {
            return;
        };
        let mode = std::env::var("MINIDORA_STORE_MP_MODE").unwrap();
        let record_path = std::env::var("MINIDORA_STORE_MP_RECORD").unwrap();
        let gate = std::env::var("MINIDORA_STORE_MP_GATE").unwrap();
        let ready = std::env::var("MINIDORA_STORE_MP_READY").unwrap();
        let result = std::env::var("MINIDORA_STORE_MP_RESULT").unwrap();
        let record = fs::read(record_path).unwrap();
        let store = Store::open(directory).unwrap();
        fs::write(ready, b"ready").unwrap();
        let deadline = Instant::now() + Duration::from_secs(15);
        while !Path::new(&gate).exists() {
            assert!(
                Instant::now() < deadline,
                "parent did not release writer barrier"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
        let outcome = match mode.as_str() {
            "append" => {
                let candidate: CaseRevision = serde_json::from_slice(&record).unwrap();
                match store.append_case("alice", &candidate) {
                    Ok(()) => "OK".to_owned(),
                    Err(minidora_domain::Error::Contract { code, .. }) => format!("ERR:{code}"),
                    Err(error) => format!("ERR:{error}"),
                }
            }
            "register" => {
                let request: RequestEnvelope = serde_json::from_slice(&record).unwrap();
                match store.register_input(
                    &request.actor_id,
                    &request,
                    &identity(&request).unwrap(),
                ) {
                    Ok(true) => "NEW".to_owned(),
                    Ok(false) => "REPLAY".to_owned(),
                    Err(minidora_domain::Error::Contract { code, .. }) => format!("ERR:{code}"),
                    Err(error) => format!("ERR:{error}"),
                }
            }
            "save-action" | "save-action-collision" => {
                let action: ActionRecord = serde_json::from_slice(&record).unwrap();
                match store.save_action(&action.actor_id, &action) {
                    Ok(()) => "OK".to_owned(),
                    Err(minidora_domain::Error::Contract { code, .. }) => format!("ERR:{code}"),
                    Err(error) => format!("ERR:{error}"),
                }
            }
            _ => panic!("unsupported multi-process test mode"),
        };
        fs::write(result, outcome).unwrap();
    }

    #[test]
    fn case_and_action_access_are_scoped_to_persisted_actor() {
        let directory = Directory::new();
        let store = Store::open(&directory.0).unwrap();
        let req = request();
        register(&store, &req);
        assert!(store.load_case("bob", "case-a").is_err());
        assert!(store.actions("bob", "case-a").is_err());
        assert!(store.put_bytes("bob", "case-a", b"private").is_err());
        assert!(
            store
                .get_input_result("bob", "case-a", "input-a", 1)
                .is_err()
        );
        let mut impostor = req.clone();
        impostor.actor_id = "bob".into();
        assert!(
            store
                .register_input("bob", &impostor, &identity(&impostor).unwrap())
                .is_err()
        );
    }
    #[test]
    fn bound_goal_is_immutable_but_pre_goal_received_is_restorable() {
        let directory = Directory::new();
        let store = Store::open(&directory.0).unwrap();
        let req = request();
        let initial = register(&store, &req);
        assert!(
            store
                .load_case("alice", "case-a")
                .unwrap()
                .unwrap()
                .goal_sha256
                .is_none()
        );
        let next = sealed(&store, &req, &initial);
        store.append_case("alice", &next).unwrap();
        let mut invalid = next.clone();
        invalid.revision += 1;
        invalid.parent_revision = Some(next.revision);
        invalid.lifecycle = Lifecycle::Running;
        invalid.goal_sha256 = Some(
            store
                .put_artifact("alice", "case-a", &"different goal")
                .unwrap(),
        );
        assert!(store.append_case("alice", &invalid).is_err());
    }
    #[test]
    fn new_input_version_must_reenter_received_before_new_seal() {
        let directory = Directory::new();
        let store = Store::open(&directory.0).unwrap();
        let req = request();
        let initial = register(&store, &req);
        let mut current = sealed(&store, &req, &initial);
        store.append_case("alice", &current).unwrap();
        current.parent_revision = Some(current.revision);
        current.revision += 1;
        current.lifecycle = Lifecycle::Suspend;
        store.append_case("alice", &current).unwrap();
        let mut new_request = req.clone();
        new_request.input_version = 2;
        new_request.text = "3+4".into();
        assert!(
            store
                .register_input("alice", &new_request, &identity(&new_request).unwrap())
                .unwrap()
        );
        let mut invalid = current.clone();
        invalid.parent_revision = Some(current.revision);
        invalid.revision += 1;
        invalid.lifecycle = Lifecycle::Running;
        invalid.input_fingerprint = Some(identity(&new_request).unwrap());
        assert!(store.append_case("alice", &invalid).is_err());
        invalid.lifecycle = Lifecycle::Received;
        invalid.bundle = None;
        invalid.goal_sha256 = None;
        store.append_case("alice", &invalid).unwrap();
        let resealed = sealed(&store, &new_request, &invalid);
        store.append_case("alice", &resealed).unwrap();
        assert_eq!(
            store
                .load_case("alice", "case-a")
                .unwrap()
                .unwrap()
                .bundle
                .unwrap()
                .request
                .input_version,
            2
        );
    }

    #[test]
    fn explicit_alpha7_migration_preserves_legacy_case_blobs_and_is_idempotent() {
        let directory = Directory::new();
        let store = Store::open(&directory.0).unwrap();
        let request = request();
        let fingerprint = identity(&request).unwrap();
        assert!(
            store
                .register_input("alice", &request, &fingerprint)
                .unwrap()
        );

        let mut legacy_revision = received(&request);
        legacy_revision.schema_version = LEGACY_SCHEMA_VERSION.into();
        let (state_sha, state_length) = store.publish_blob(&legacy_revision).unwrap();
        let transaction = store.transaction().unwrap();
        Store::reference(&transaction, &state_sha, state_length, "case_revision").unwrap();
        let parent_hash: Option<String> = None;
        let event_hash = identity(&(
            "case-revision-event-v1",
            &request.case_id,
            legacy_revision.revision,
            &parent_hash,
            &state_sha,
        ))
        .unwrap();
        transaction
            .execute(
                "INSERT INTO case_revision(case_id,revision,parent_revision,state_sha256,parent_hash,event_hash,lifecycle,goal_sha256) VALUES(?1,?2,?3,?4,?5,?6,?7,?8)",
                params![
                    request.case_id,
                    0_i64,
                    Option::<i64>::None,
                    state_sha,
                    parent_hash,
                    event_hash,
                    "RECEIVED",
                    Option::<String>::None,
                ],
            )
            .unwrap();
        transaction
            .execute(
                "UPDATE store_metadata SET value=?1 WHERE key='schema'",
                [LEGACY_SCHEMA_VERSION],
            )
            .unwrap();
        transaction.commit().unwrap();
        drop(store);

        let database_path = directory.0.join("state.sqlite3");
        let legacy = Connection::open(&database_path).unwrap();
        legacy
            .execute_batch(
                "DROP TRIGGER action_identity_no_update;
                 DROP TRIGGER action_identity_no_delete;",
            )
            .unwrap();
        drop(legacy);
        let error = match Store::open(&directory.0) {
            Ok(_) => panic!("ordinary open must reject alpha 7 metadata"),
            Err(error) => error,
        };
        assert!(matches!(
            error,
            minidora_domain::Error::Contract { code, .. } if code == "SCHEMA_MISMATCH"
        ));
        let legacy = Connection::open(&database_path).unwrap();
        let guard_count: i64 = legacy
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='trigger' AND name IN ('action_identity_no_update','action_identity_no_delete')",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(
            guard_count, 0,
            "rejected open must not mutate alpha 7 guards"
        );
        drop(legacy);

        let state_blob = directory.0.join("blobs").join(&state_sha);
        let original_blob = fs::read(&state_blob).unwrap();
        fs::write(&state_blob, b"corrupt legacy blob").unwrap();
        let error = Store::migrate_alpha7_to_alpha8(&directory.0).unwrap_err();
        assert!(matches!(
            error,
            minidora_domain::Error::Contract { code, .. } if code == "INTEGRITY_FAILURE"
        ));
        let connection = Connection::open(directory.0.join("state.sqlite3")).unwrap();
        let schema: String = connection
            .query_row(
                "SELECT value FROM store_metadata WHERE key='schema'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(schema, LEGACY_SCHEMA_VERSION);
        drop(connection);
        fs::write(&state_blob, original_blob).unwrap();

        assert!(Store::migrate_alpha7_to_alpha8(&directory.0).unwrap());
        let migrated_db = Connection::open(&database_path).unwrap();
        let guard_count: i64 = migrated_db
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='trigger' AND name IN ('action_identity_no_update','action_identity_no_delete')",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(guard_count, 2, "explicit migration installs both guards");
        drop(migrated_db);
        let migrated = Store::open(&directory.0).unwrap();
        let latest = migrated.load_case("alice", "case-a").unwrap().unwrap();
        assert_eq!(latest.schema_version, LEGACY_SCHEMA_VERSION);
        assert_eq!(latest.revision, 0);
        let stored_sha: String = migrated
            .connection
            .query_row(
                "SELECT state_sha256 FROM case_revision WHERE case_id='case-a' AND revision=0",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(stored_sha, state_sha);
        assert!(!Store::migrate_alpha7_to_alpha8(&directory.0).unwrap());
    }

    #[test]
    fn corrupt_or_missing_referenced_blob_blocks_restart() {
        for missing in [false, true] {
            let directory = Directory::new();
            let sha;
            {
                let store = Store::open(&directory.0).unwrap();
                register(&store, &request());
                sha = store
                    .put_bytes("alice", "case-a", b"original evidence")
                    .unwrap();
            }
            let path = directory.0.join("blobs").join(sha);
            if missing {
                fs::remove_file(path).unwrap();
            } else {
                fs::write(path, b"altered evidence").unwrap();
            }
            assert!(Store::open(&directory.0).is_err());
        }
    }
    #[test]
    fn crash_before_database_commit_leaves_orphan_and_no_case() {
        let directory = Directory::new();
        let sha;
        {
            let store = Store::open(&directory.0).unwrap();
            sha = store.publish_blob(&request()).unwrap().0;
            // Fault boundary: blob was synced/published; no logical transaction committed.
        }
        let store = Store::open(&directory.0).unwrap();
        assert_eq!(store.orphan_blobs().unwrap(), vec![sha]);
        assert!(store.load_case("alice", "case-a").unwrap().is_none());
    }

    #[test]
    fn abrupt_process_exit_after_blob_publication_leaves_only_an_orphan() {
        let directory = Directory::new();
        Store::open(&directory.0).unwrap();
        let result = run_abrupt_store_worker(
            &directory.0,
            "store::tests::abrupt_blob_publisher_worker",
            None,
            86,
        );
        let sha = fs::read_to_string(result).unwrap();

        let store = Store::open(&directory.0).unwrap();
        assert_eq!(store.orphan_blobs().unwrap(), vec![sha]);
        assert!(store.load_case("alice", "case-a").unwrap().is_none());
    }

    #[test]
    fn abrupt_process_exit_after_attempted_action_recovers_unknown() {
        let directory = Directory::new();
        let request = request();
        let action_bytes = {
            let store = Store::open(&directory.0).unwrap();
            register(&store, &request);
            serde_json::to_vec(&action(&store, &request)).unwrap()
        };
        run_abrupt_store_worker(
            &directory.0,
            "store::tests::abrupt_attempt_worker",
            Some(&action_bytes),
            87,
        );

        let store = Store::open(&directory.0).unwrap();
        let recovered = store.actions("alice", "case-a").unwrap().remove(0);
        assert_eq!(recovered.phase, EffectPhase::Unknown);
        assert_eq!(recovered.action_id, "write-a");
        assert_eq!(recovered.idempotency_key, "stable-token");
        assert!(recovered.receipt.is_none());
    }

    #[test]
    fn abrupt_blob_publisher_worker() {
        let Ok(directory) = std::env::var("MINIDORA_STORE_CRASH_DIR") else {
            return;
        };
        let store = Store::open(directory).unwrap();
        fs::write(
            std::env::var("MINIDORA_STORE_CRASH_READY").unwrap(),
            b"ready",
        )
        .unwrap();
        let gate = std::env::var("MINIDORA_STORE_CRASH_GATE").unwrap();
        let deadline = Instant::now() + Duration::from_secs(15);
        while !Path::new(&gate).exists() {
            assert!(
                Instant::now() < deadline,
                "parent did not release abrupt-blob barrier"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
        let sha = store.publish_blob(&request()).unwrap().0;
        fs::write(std::env::var("MINIDORA_STORE_CRASH_RESULT").unwrap(), sha).unwrap();
        std::process::exit(86);
    }

    #[test]
    fn abrupt_attempt_worker() {
        let Ok(directory) = std::env::var("MINIDORA_STORE_CRASH_DIR") else {
            return;
        };
        let record = fs::read(std::env::var("MINIDORA_STORE_CRASH_RECORD").unwrap()).unwrap();
        let mut action: ActionRecord = serde_json::from_slice(&record).unwrap();
        let store = Store::open(directory).unwrap();
        fs::write(
            std::env::var("MINIDORA_STORE_CRASH_READY").unwrap(),
            b"ready",
        )
        .unwrap();
        let gate = std::env::var("MINIDORA_STORE_CRASH_GATE").unwrap();
        let deadline = Instant::now() + Duration::from_secs(15);
        while !Path::new(&gate).exists() {
            assert!(
                Instant::now() < deadline,
                "parent did not release abrupt-attempt barrier"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
        for phase in [
            EffectPhase::Proposed,
            EffectPhase::Authorized,
            EffectPhase::Prepared,
            EffectPhase::Attempted,
        ] {
            action.phase = phase;
            store.save_action("alice", &action).unwrap();
        }
        std::process::exit(87);
    }

    #[test]
    fn interrupted_attempt_is_unknown_and_cannot_be_reissued() {
        let directory = Directory::new();
        let req = request();
        {
            let store = Store::open(&directory.0).unwrap();
            register(&store, &req);
            let mut value = action(&store, &req);
            for phase in [
                EffectPhase::Proposed,
                EffectPhase::Authorized,
                EffectPhase::Prepared,
                EffectPhase::Attempted,
            ] {
                value.phase = phase;
                store.save_action("alice", &value).unwrap();
            }
        }
        let store = Store::open(&directory.0).unwrap();
        let mut recovered = store.actions("alice", "case-a").unwrap().remove(0);
        assert_eq!(recovered.phase, EffectPhase::Unknown);
        store.save_action("alice", &recovered).unwrap();
        recovered.phase = EffectPhase::Attempted;
        assert!(store.save_action("alice", &recovered).is_err());
        recovered.phase = EffectPhase::Unknown;
        recovered.idempotency_key = "fresh-unsafe-token".into();
        assert!(store.save_action("alice", &recovered).is_err());
    }

    #[test]
    fn idempotency_key_cannot_be_reassigned_to_a_different_action() {
        let directory = Directory::new();
        let store = Store::open(&directory.0).unwrap();
        let request = request();
        register(&store, &request);
        let original = action(&store, &request);
        store.save_action("alice", &original).unwrap();

        let mut duplicate_key = original.clone();
        duplicate_key.action_id = "write-b".into();
        let error = store.save_action("alice", &duplicate_key).unwrap_err();
        assert!(matches!(
            error,
            minidora_domain::Error::Contract { code, .. }
                if code == "ACTION_IDENTITY_COLLISION"
        ));
        assert_eq!(
            store.actions("alice", "case-a").unwrap(),
            vec![original.clone()]
        );
        drop(store);
        assert_eq!(
            Store::open(&directory.0)
                .unwrap()
                .actions("alice", "case-a")
                .unwrap(),
            vec![original]
        );
    }

    #[test]
    fn action_idempotency_mapping_is_immutable_and_verified_on_open() {
        let directory = Directory::new();
        let store = Store::open(&directory.0).unwrap();
        let request = request();
        register(&store, &request);
        store
            .save_action("alice", &action(&store, &request))
            .unwrap();
        drop(store);

        let connection = Connection::open(directory.0.join("state.sqlite3")).unwrap();
        assert!(connection
            .execute(
                "UPDATE action_identity SET idempotency_key='tampered' WHERE case_id='case-a' AND action_id='write-a'",
                [],
            )
            .is_err());
        connection
            .execute_batch(
                "DROP TRIGGER action_identity_no_update;
                 UPDATE action_identity SET idempotency_key='tampered' WHERE case_id='case-a' AND action_id='write-a';",
            )
            .unwrap();
        drop(connection);

        let error = match Store::open(&directory.0) {
            Ok(_) => panic!("tampered action identity must fail closed"),
            Err(error) => error,
        };
        assert!(matches!(
            error,
            minidora_domain::Error::Contract { code, .. } if code == "INTEGRITY_FAILURE"
        ));
    }

    #[test]
    fn confirmation_requires_matching_receipt() {
        let directory = Directory::new();
        let store = Store::open(&directory.0).unwrap();
        let req = request();
        register(&store, &req);
        let mut value = action(&store, &req);
        for phase in [
            EffectPhase::Proposed,
            EffectPhase::Authorized,
            EffectPhase::Prepared,
            EffectPhase::Attempted,
        ] {
            value.phase = phase;
            store.save_action("alice", &value).unwrap();
        }
        value.phase = EffectPhase::Confirmed;
        assert!(store.save_action("alice", &value).is_err());
        value.receipt = Some(minidora_domain::DeliveryReceipt {
            action_id: "different-action".into(),
            target: value.target.clone(),
            content_sha256: value.exact_request_sha256.clone(),
            phase: EffectPhase::Confirmed,
            observed_bytes: Some(13),
            reason: None,
        });
        assert!(store.save_action("alice", &value).is_err());
        value.receipt.as_mut().unwrap().action_id = value.action_id.clone();
        store.save_action("alice", &value).unwrap();
        assert_eq!(
            store.actions("alice", "case-a").unwrap()[0].phase,
            EffectPhase::Confirmed
        );
    }
    #[test]
    fn memory_is_actor_scoped_and_retains_revision_history() {
        let directory = Directory::new();
        let store = Store::open(&directory.0).unwrap();
        let memory = MemoryState {
            revision: 1,
            ..MemoryState::default()
        };
        store.save_memory("alice", &memory).unwrap();
        assert_eq!(store.load_memory("alice").unwrap(), memory);
        assert_eq!(store.load_memory("bob").unwrap(), MemoryState::default());
        let next = MemoryState {
            revision: 2,
            ..memory
        };
        store.save_memory("alice", &next).unwrap();
        assert!(store.save_memory("alice", &MemoryState::default()).is_err());
        let count: i64 = store
            .connection
            .query_row(
                "SELECT COUNT(*) FROM memory_revision WHERE actor_id='alice'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(count, 2);
    }
    #[test]
    fn sqlite_foreign_keys_and_immutable_revision_triggers_are_effective() {
        let directory = Directory::new();
        let store = Store::open(&directory.0).unwrap();
        register(&store, &request());
        assert!(
            store
                .connection
                .execute("UPDATE case_revision SET event_hash='tampered'", [])
                .is_err()
        );
        assert!(
            store
                .connection
                .execute(
                    "INSERT INTO case_artifact(case_id,sha256) VALUES('missing','missing')",
                    []
                )
                .is_err()
        );
    }
}
