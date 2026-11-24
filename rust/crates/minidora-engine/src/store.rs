//! Core-owned persistence adapter. SQLite owns logical transactions; immutable
//! blobs are durably published first. Orphan blobs are harmless, while a
//! missing or corrupt referenced blob is an integrity failure.
use crate::durability::sync_directory;
use minidora_domain::{
    ALPHA8_SCHEMA_VERSION, ALPHA10_SCHEMA_VERSION, ActionRecord, CaseRevision, ConditionRecord,
    EffectPhase, Fact, InputBundle, LEGACY_SCHEMA_VERSION, Lifecycle, MemoryState,
    PREVIOUS_SCHEMA_VERSION, Proposition, RelationRule, RenderTarget, RequestEnvelope, Result,
    ResultRecord, SCHEMA_VERSION, canonical, digest, failure, identity,
};
use rusqlite::{
    Connection, OpenFlags, OptionalExtension, Transaction, TransactionBehavior, params,
};
use serde::{Serialize, de::DeserializeOwned};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, File, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, Instant},
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
fn canonical_string<T: Serialize>(value: &T) -> Result<String> {
    String::from_utf8(canonical(value)?)
        .map_err(|error| failure("INTEGRITY_FAILURE", error.to_string()))
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

/// Exact input and result reads in one authenticated, verified SQLite snapshot.
pub(crate) struct VerifiedCaseHistory<'a> {
    store: &'a Store,
    connection: &'a Connection,
    actor: &'a str,
    case: &'a str,
}

impl VerifiedCaseHistory<'_> {
    pub(crate) fn input_bundle(
        &self,
        input: &str,
        version: u64,
    ) -> Result<Option<(String, InputBundle)>> {
        self.store
            .read_input_bundle(self.connection, self.actor, self.case, input, version)
    }

    pub(crate) fn input_result(&self, input: &str, version: u64) -> Result<Option<ResultRecord>> {
        self.store
            .read_input_result(self.connection, self.case, input, version)
    }

    pub(crate) fn contains_committed_result(&self, result: &ResultRecord) -> Result<bool> {
        let mut statement = sql(self.connection.prepare(
            "SELECT state_sha256 FROM case_revision WHERE case_id=?1 AND lifecycle='COMMIT' ORDER BY revision DESC",
        ))?;
        let rows = sql(statement.query_map([self.case], |row| row.get::<_, String>(0)))?;
        for row in rows {
            let revision: CaseRevision = self.store.read_blob(&sql(row)?)?;
            if revision.lifecycle == Lifecycle::Commit
                && revision.input_fingerprint.as_deref() == Some(result.id.as_str())
                && revision.result.as_ref() == Some(result)
            {
                return Ok(true);
            }
        }
        Ok(false)
    }

    pub(crate) fn suspended_result_revision(
        &self,
        result: &ResultRecord,
    ) -> Result<Option<CaseRevision>> {
        self.store
            .read_suspended_result_revision(self.connection, self.case, result, || {
                self.input_result(&result.input_id, result.input_version)
            })
    }
}

#[derive(Clone, Debug)]
pub(crate) enum IndexedMemoryValue {
    Fact(Fact),
    Rule(RelationRule),
    Condition(ConditionRecord),
}

#[derive(Clone, Debug)]
pub(crate) struct IndexedMemoryRecord {
    pub(crate) ordinal: usize,
    pub(crate) value: IndexedMemoryValue,
}

impl IndexedMemoryValue {
    pub(crate) fn id(&self) -> &str {
        self.as_ref().id()
    }

    fn as_ref(&self) -> IndexedMemoryValueRef<'_> {
        match self {
            Self::Fact(record) => IndexedMemoryValueRef::Fact(record),
            Self::Rule(record) => IndexedMemoryValueRef::Rule(record),
            Self::Condition(record) => IndexedMemoryValueRef::Condition(record),
        }
    }
}

#[derive(Clone, Copy)]
enum IndexedMemoryValueRef<'a> {
    Fact(&'a Fact),
    Rule(&'a RelationRule),
    Condition(&'a ConditionRecord),
}

impl<'a> IndexedMemoryValueRef<'a> {
    fn id(self) -> &'a str {
        match self {
            Self::Fact(record) => &record.id,
            Self::Rule(record) => &record.id,
            Self::Condition(record) => &record.id,
        }
    }

    fn payload_json(self) -> Result<String> {
        match self {
            Self::Fact(record) => canonical_string(record),
            Self::Rule(record) => canonical_string(record),
            Self::Condition(record) => canonical_string(record),
        }
    }
}

#[derive(Clone, Copy)]
enum MemoryIndexSource<'a> {
    Durable,
    Snapshot(&'a MemoryState),
}

pub(crate) struct MemoryIndexSnapshot<'tx, 'conn> {
    transaction: &'tx Transaction<'conn>,
    actor: String,
    revision: u64,
    source: MemoryIndexSource<'tx>,
}

fn memory_index_limit(max_records: usize) -> Result<i64> {
    let limit = max_records
        .checked_add(1)
        .ok_or_else(|| failure("RELATION_BUDGET", "active memory record limit overflowed"))?;
    i64::try_from(limit).map_err(|_| {
        failure(
            "RELATION_BUDGET",
            "active memory record limit exceeds storage range",
        )
    })
}

impl MemoryIndexSnapshot<'_, '_> {
    pub(crate) fn revision(&self) -> u64 {
        self.revision
    }

    fn select_records<P: rusqlite::Params>(
        &self,
        statement: &str,
        params: P,
        max_records: usize,
    ) -> Result<Vec<IndexedMemoryRecord>> {
        let mut statement = sql(self.transaction.prepare(statement))?;
        let rows = sql(statement.query_map(params, StoredMemoryIndexRow::read))?;
        let mut records = Vec::new();
        for row in rows {
            records.push(sql(row)?.decode(self.source)?);
            if records.len() > max_records {
                return Err(failure(
                    "RELATION_BUDGET",
                    "goal-relevant memory projection exceeds the sealed action budget",
                ));
            }
        }
        Ok(records)
    }

    pub(crate) fn relation_candidates(
        &self,
        proposition: &Proposition,
        max_records: usize,
    ) -> Result<Vec<IndexedMemoryRecord>> {
        let condition_refs = canonical_string(&proposition.condition_refs)?;
        let subject_is_variable =
            proposition.subject.starts_with('$') || proposition.subject.starts_with('?');
        let object_is_variable =
            proposition.object.starts_with('$') || proposition.object.starts_with('?');
        self.select_records(
            "SELECT record_kind,record_id,ordinal,payload_json,payload_sha256,valid,subject,predicate,object,negative,scope,temporal_scope,condition_refs,condition_id FROM memory_index_record WHERE actor_id=?1 AND valid=1 AND record_kind IN ('FACT','RULE') AND predicate=?2 AND scope=?3 AND temporal_scope IS ?4 AND condition_refs=?5 AND (?6=1 OR subject=?7 OR (record_kind='RULE' AND (substr(subject,1,1)='$' OR substr(subject,1,1)='?'))) AND (?8=1 OR object=?9 OR (record_kind='RULE' AND (substr(object,1,1)='$' OR substr(object,1,1)='?'))) ORDER BY record_kind,ordinal LIMIT ?10",
            params![
                self.actor,
                proposition.predicate,
                proposition.scope,
                proposition.temporal_scope,
                condition_refs,
                if subject_is_variable { 1_i64 } else { 0_i64 },
                proposition.subject,
                if object_is_variable { 1_i64 } else { 0_i64 },
                proposition.object,
                memory_index_limit(max_records)?,
            ],
            max_records,
        )
    }

    /// Select only indexed relations that can seed a common-relation query
    /// for either named subject. Variable-subject Rules remain eligible so
    /// Core can bind their conclusions to the two requested subjects.
    pub(crate) fn common_relation_candidates(
        &self,
        subject_a: &str,
        subject_b: &str,
        scope: &str,
        max_records: usize,
    ) -> Result<Vec<IndexedMemoryRecord>> {
        let mut statement = sql(self.transaction.prepare(
            "SELECT record_kind,record_id,ordinal,payload_json,payload_sha256,valid,subject,predicate,object,negative,scope,temporal_scope,condition_refs,condition_id FROM memory_index_record INDEXED BY memory_index_common_subject_scope WHERE actor_id=?1 AND valid=1 AND scope=?2 AND record_kind IN ('FACT','RULE') AND subject IN (?3,?4) UNION ALL SELECT record_kind,record_id,ordinal,payload_json,payload_sha256,valid,subject,predicate,object,negative,scope,temporal_scope,condition_refs,condition_id FROM memory_index_record INDEXED BY memory_index_common_subject_scope WHERE actor_id=?1 AND valid=1 AND scope=?2 AND record_kind IN ('FACT','RULE') AND subject >= '$' AND subject < '%' UNION ALL SELECT record_kind,record_id,ordinal,payload_json,payload_sha256,valid,subject,predicate,object,negative,scope,temporal_scope,condition_refs,condition_id FROM memory_index_record INDEXED BY memory_index_common_subject_scope WHERE actor_id=?1 AND valid=1 AND scope=?2 AND record_kind IN ('FACT','RULE') AND subject >= '?' AND subject < '@'",
        ))?;
        let rows = sql(statement.query_map(
            params![self.actor, scope, subject_a, subject_b],
            StoredMemoryIndexRow::read,
        ))?;
        let mut records = std::collections::BTreeMap::new();
        for row in rows {
            let record = sql(row)?.decode(self.source)?;
            let id = record.value.id().to_owned();
            records.entry(id).or_insert(record);
            if records.len() > max_records {
                return Err(failure(
                    "RELATION_BUDGET",
                    "common-relation candidate projection exceeds the sealed action budget",
                ));
            }
        }
        Ok(records.into_values().collect())
    }

    /// Select active relation records whose indexed conclusion belongs to one
    /// exact scope. Core filters mixed-scope Rules before using them as
    /// comparison inventory seeds.
    pub(crate) fn relation_scope_candidates(
        &self,
        scope: &str,
        max_records: usize,
    ) -> Result<Vec<IndexedMemoryRecord>> {
        self.select_records(
            "SELECT record_kind,record_id,ordinal,payload_json,payload_sha256,valid,subject,predicate,object,negative,scope,temporal_scope,condition_refs,condition_id FROM memory_index_record INDEXED BY memory_index_scope WHERE actor_id=?1 AND valid=1 AND scope=?2 AND record_kind IN ('FACT','RULE') ORDER BY record_kind,ordinal LIMIT ?3",
            params![self.actor, scope, memory_index_limit(max_records)?],
            max_records,
        )
    }

    /// Select the subject-local records that can seed a saved relation profile.
    /// Variable-subject Rules remain eligible because Core binds them to the
    /// requested subject before evaluating their proof closure.
    pub(crate) fn relation_profile_candidates(
        &self,
        subject: &str,
        max_records: usize,
    ) -> Result<Vec<IndexedMemoryRecord>> {
        let mut statement = sql(self.transaction.prepare(
            "SELECT record_kind,record_id,ordinal,payload_json,payload_sha256,valid,subject,predicate,object,negative,scope,temporal_scope,condition_refs,condition_id FROM memory_index_record INDEXED BY memory_index_profile_subject WHERE actor_id=?1 AND valid=1 AND record_kind='FACT' AND subject=?2 UNION ALL SELECT record_kind,record_id,ordinal,payload_json,payload_sha256,valid,subject,predicate,object,negative,scope,temporal_scope,condition_refs,condition_id FROM memory_index_record INDEXED BY memory_index_profile_subject WHERE actor_id=?1 AND valid=1 AND record_kind='RULE' AND subject=?2 UNION ALL SELECT record_kind,record_id,ordinal,payload_json,payload_sha256,valid,subject,predicate,object,negative,scope,temporal_scope,condition_refs,condition_id FROM memory_index_record INDEXED BY memory_index_profile_subject WHERE actor_id=?1 AND valid=1 AND record_kind='RULE' AND subject >= '$' AND subject < '%' UNION ALL SELECT record_kind,record_id,ordinal,payload_json,payload_sha256,valid,subject,predicate,object,negative,scope,temporal_scope,condition_refs,condition_id FROM memory_index_record INDEXED BY memory_index_profile_subject WHERE actor_id=?1 AND valid=1 AND record_kind='RULE' AND subject >= '?' AND subject < '@' LIMIT ?3",
        ))?;
        let rows = sql(statement.query_map(
            params![self.actor, subject, memory_index_limit(max_records)?],
            StoredMemoryIndexRow::read,
        ))?;
        let mut records = BTreeMap::new();
        for row in rows {
            let record = sql(row)?.decode(self.source)?;
            records
                .entry(record.value.id().to_owned())
                .or_insert(record);
            if records.len() > max_records {
                return Err(failure(
                    "RELATION_BUDGET",
                    "relation-profile candidate projection exceeds the sealed action budget",
                ));
            }
        }
        Ok(records.into_values().collect())
    }

    pub(crate) fn out_of_scope_direct_facts(
        &self,
        proposition: &Proposition,
        max_records: usize,
    ) -> Result<Vec<IndexedMemoryRecord>> {
        let condition_refs = canonical_string(&proposition.condition_refs)?;
        self.select_records(
            "SELECT record_kind,record_id,ordinal,payload_json,payload_sha256,valid,subject,predicate,object,negative,scope,temporal_scope,condition_refs,condition_id FROM memory_index_record WHERE actor_id=?1 AND valid=1 AND record_kind='FACT' AND subject=?2 AND predicate=?3 AND object=?4 AND negative=?5 AND scope<>?6 AND temporal_scope IS ?7 AND condition_refs=?8 ORDER BY ordinal LIMIT ?9",
            params![
                self.actor,
                proposition.subject,
                proposition.predicate,
                proposition.object,
                if proposition.negative { 1_i64 } else { 0_i64 },
                proposition.scope,
                proposition.temporal_scope,
                condition_refs,
                memory_index_limit(max_records)?,
            ],
            max_records,
        )
    }

    pub(crate) fn condition_evidence_facts(
        &self,
        subject: &str,
        predicate: &str,
        object: &str,
        scope: &str,
        temporal_scope: Option<&str>,
        max_records: usize,
    ) -> Result<Vec<IndexedMemoryRecord>> {
        self.select_records(
            "SELECT record_kind,record_id,ordinal,payload_json,payload_sha256,valid,subject,predicate,object,negative,scope,temporal_scope,condition_refs,condition_id FROM memory_index_record WHERE actor_id=?1 AND valid=1 AND record_kind='FACT' AND subject=?2 AND predicate=?3 AND object=?4 AND scope=?5 AND temporal_scope IS ?6 ORDER BY ordinal LIMIT ?7",
            params![
                self.actor,
                subject,
                predicate,
                object,
                scope,
                temporal_scope,
                memory_index_limit(max_records)?,
            ],
            max_records,
        )
    }

    pub(crate) fn condition_definitions(
        &self,
        condition_id: &str,
        max_records: usize,
    ) -> Result<Vec<IndexedMemoryRecord>> {
        self.select_records(
            "SELECT record_kind,record_id,ordinal,payload_json,payload_sha256,valid,subject,predicate,object,negative,scope,temporal_scope,condition_refs,condition_id FROM memory_index_record WHERE actor_id=?1 AND valid=1 AND record_kind='CONDITION' AND condition_id=?2 ORDER BY ordinal LIMIT ?3",
            params![
                self.actor,
                condition_id,
                memory_index_limit(max_records)?,
            ],
            max_records,
        )
    }

    pub(crate) fn record_by_id(&self, id: &str) -> Result<Option<IndexedMemoryRecord>> {
        let mut statement = sql(self.transaction.prepare(
            "SELECT record_kind,record_id,ordinal,payload_json,payload_sha256,valid,subject,predicate,object,negative,scope,temporal_scope,condition_refs,condition_id FROM memory_index_record WHERE actor_id=?1 AND record_id=?2",
        ))?;
        let record = sql(statement
            .query_row(params![self.actor, id], StoredMemoryIndexRow::read)
            .optional())?
        .map(|record| record.decode(self.source))
        .transpose()?;
        Ok(record)
    }
}

struct MemoryIndexFields<'a> {
    kind: &'static str,
    valid: bool,
    subject: Option<&'a str>,
    predicate: Option<&'a str>,
    object: Option<&'a str>,
    negative: Option<i64>,
    scope: Option<&'a str>,
    temporal_scope: Option<&'a str>,
    condition_refs: Option<String>,
    condition_id: Option<&'a str>,
}

struct StoredMemoryIndexRow {
    kind: String,
    record_id: String,
    ordinal: i64,
    payload_json: Option<String>,
    payload_sha256: Option<String>,
    valid: i64,
    subject: Option<String>,
    predicate: Option<String>,
    object: Option<String>,
    negative: Option<i64>,
    scope: Option<String>,
    temporal_scope: Option<String>,
    condition_refs: Option<String>,
    condition_id: Option<String>,
}

impl StoredMemoryIndexRow {
    fn read(row: &rusqlite::Row<'_>) -> SqlResult<Self> {
        Ok(Self {
            kind: row.get(0)?,
            record_id: row.get(1)?,
            ordinal: row.get(2)?,
            payload_json: row.get(3)?,
            payload_sha256: row.get(4)?,
            valid: row.get(5)?,
            subject: row.get(6)?,
            predicate: row.get(7)?,
            object: row.get(8)?,
            negative: row.get(9)?,
            scope: row.get(10)?,
            temporal_scope: row.get(11)?,
            condition_refs: row.get(12)?,
            condition_id: row.get(13)?,
        })
    }

    fn stored_value(&self) -> Result<IndexedMemoryValue> {
        let (Some(payload), Some(sha256)) =
            (self.payload_json.as_deref(), self.payload_sha256.as_deref())
        else {
            return Err(failure(
                "MEMORY_INDEX_CORRUPT",
                "durable memory index payload is missing",
            ));
        };
        if digest(payload.as_bytes()) != sha256 {
            return Err(failure(
                "MEMORY_INDEX_CORRUPT",
                "indexed memory record checksum differs",
            ));
        }
        match self.kind.as_str() {
            "FACT" => Ok(IndexedMemoryValue::Fact(serde_json::from_str(payload)?)),
            "RULE" => Ok(IndexedMemoryValue::Rule(serde_json::from_str(payload)?)),
            "CONDITION" => Ok(IndexedMemoryValue::Condition(serde_json::from_str(
                payload,
            )?)),
            _ => Err(failure(
                "MEMORY_INDEX_CORRUPT",
                "indexed memory record kind is unknown",
            )),
        }
    }

    fn snapshot_value(&self, memory: &MemoryState, ordinal: usize) -> Result<IndexedMemoryValue> {
        let value = match self.kind.as_str() {
            "FACT" => memory
                .facts
                .get(ordinal)
                .cloned()
                .map(IndexedMemoryValue::Fact),
            "RULE" => memory
                .rules
                .get(ordinal)
                .cloned()
                .map(IndexedMemoryValue::Rule),
            "CONDITION" => memory
                .conditions
                .get(ordinal)
                .cloned()
                .map(IndexedMemoryValue::Condition),
            _ => None,
        };
        value.ok_or_else(|| {
            failure(
                "MEMORY_INDEX_CORRUPT",
                "indexed memory ordinal has no record of this kind in the authenticated snapshot",
            )
        })
    }

    fn ordinal(&self) -> Result<usize> {
        usize::try_from(self.ordinal)
            .map_err(|_| failure("MEMORY_INDEX_CORRUPT", "indexed memory ordinal is negative"))
    }

    fn validity(&self) -> Result<bool> {
        match self.valid {
            0 => Ok(false),
            1 => Ok(true),
            _ => Err(failure(
                "MEMORY_INDEX_CORRUPT",
                "indexed memory validity flag is invalid",
            )),
        }
    }

    fn decode(self, source: MemoryIndexSource<'_>) -> Result<IndexedMemoryRecord> {
        let (value, valid, ordinal) = match source {
            MemoryIndexSource::Durable => (self.stored_value()?, self.validity()?, self.ordinal()?),
            MemoryIndexSource::Snapshot(memory) => {
                let ordinal = self.ordinal()?;
                (
                    self.snapshot_value(memory, ordinal)?,
                    self.validity()?,
                    ordinal,
                )
            }
        };
        let fields = memory_index_fields(value.as_ref())?;
        if value.id() != self.record_id || fields.valid != valid {
            return Err(failure(
                "MEMORY_INDEX_CORRUPT",
                "indexed memory identity or validity differs from its payload",
            ));
        }
        if fields.kind != self.kind
            || fields.subject != self.subject.as_deref()
            || fields.predicate != self.predicate.as_deref()
            || fields.object != self.object.as_deref()
            || fields.negative != self.negative
            || fields.scope != self.scope.as_deref()
            || fields.temporal_scope != self.temporal_scope.as_deref()
            || fields.condition_refs != self.condition_refs
            || fields.condition_id != self.condition_id.as_deref()
        {
            return Err(failure(
                "MEMORY_INDEX_CORRUPT",
                "indexed relation keys differ from their typed memory payload",
            ));
        }
        Ok(IndexedMemoryRecord { ordinal, value })
    }
}

fn memory_index_fields(value: IndexedMemoryValueRef<'_>) -> Result<MemoryIndexFields<'_>> {
    let (kind, valid, proposition) = match value {
        IndexedMemoryValueRef::Fact(record) => ("FACT", record.valid, &record.proposition),
        IndexedMemoryValueRef::Rule(record) => ("RULE", record.valid, &record.conclusion),
        IndexedMemoryValueRef::Condition(record) => {
            return Ok(MemoryIndexFields {
                kind: "CONDITION",
                valid: record.valid,
                subject: None,
                predicate: None,
                object: None,
                negative: None,
                scope: None,
                temporal_scope: None,
                condition_refs: None,
                condition_id: Some(&record.definition.condition_id),
            });
        }
    };
    Ok(MemoryIndexFields {
        kind,
        valid,
        subject: Some(&proposition.subject),
        predicate: Some(&proposition.predicate),
        object: Some(&proposition.object),
        negative: Some(i64::from(proposition.negative)),
        scope: Some(&proposition.scope),
        temporal_scope: proposition.temporal_scope.as_deref(),
        condition_refs: Some(canonical_string(&proposition.condition_refs)?),
        condition_id: None,
    })
}

const MEMORY_INDEX_UPSERT: &str = "INSERT INTO memory_index_record(actor_id,record_id,record_kind,ordinal,payload_json,payload_sha256,valid,subject,predicate,object,negative,scope,temporal_scope,condition_refs,condition_id) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15) ON CONFLICT(actor_id,record_id) DO UPDATE SET record_kind=excluded.record_kind,ordinal=excluded.ordinal,payload_json=excluded.payload_json,payload_sha256=excluded.payload_sha256,valid=excluded.valid,subject=excluded.subject,predicate=excluded.predicate,object=excluded.object,negative=excluded.negative,scope=excluded.scope,temporal_scope=excluded.temporal_scope,condition_refs=excluded.condition_refs,condition_id=excluded.condition_id";

fn memory_index_record(
    statement: &mut rusqlite::Statement<'_>,
    actor: &str,
    ordinal: usize,
    value: IndexedMemoryValueRef<'_>,
    source: MemoryIndexSource<'_>,
) -> Result<()> {
    let payload_json = match source {
        MemoryIndexSource::Durable => Some(value.payload_json()?),
        MemoryIndexSource::Snapshot(_) => None,
    };
    let payload_sha256 = payload_json
        .as_ref()
        .map(|payload| digest(payload.as_bytes()));
    let fields = memory_index_fields(value)?;
    sql(statement.execute(params![
        actor,
        value.id(),
        fields.kind,
        integer(u64::try_from(ordinal).map_err(|_| failure(
            "MEMORY_INDEX_CORRUPT",
            "memory index ordinal exceeds its representation"
        ))?)?,
        payload_json,
        payload_sha256,
        fields.valid,
        fields.subject,
        fields.predicate,
        fields.object,
        fields.negative,
        fields.scope,
        fields.temporal_scope,
        fields.condition_refs,
        fields.condition_id,
    ]))?;
    Ok(())
}

fn index_memory_state_records(
    transaction: &Transaction<'_>,
    actor: &str,
    memory: &MemoryState,
    source: MemoryIndexSource<'_>,
) -> Result<()> {
    let mut statement = sql(transaction.prepare(MEMORY_INDEX_UPSERT))?;
    for (ordinal, record) in memory.facts.iter().enumerate() {
        memory_index_record(
            &mut statement,
            actor,
            ordinal,
            IndexedMemoryValueRef::Fact(record),
            source,
        )?;
    }
    for (ordinal, record) in memory.rules.iter().enumerate() {
        memory_index_record(
            &mut statement,
            actor,
            ordinal,
            IndexedMemoryValueRef::Rule(record),
            source,
        )?;
    }
    for (ordinal, record) in memory.conditions.iter().enumerate() {
        memory_index_record(
            &mut statement,
            actor,
            ordinal,
            IndexedMemoryValueRef::Condition(record),
            source,
        )?;
    }
    Ok(())
}

fn sync_memory_index(
    transaction: &Transaction<'_>,
    actor: &str,
    previous: Option<&MemoryState>,
    memory: &MemoryState,
) -> Result<()> {
    let indexed_revision: Option<i64> = sql(transaction
        .query_row(
            "SELECT revision FROM memory_index_state WHERE actor_id=?1",
            [actor],
            |row| row.get(0),
        )
        .optional())?;
    let can_update_incrementally =
        previous.is_some_and(|previous| indexed_revision == integer(previous.revision).ok());
    if can_update_incrementally {
        let previous = previous.expect("incremental index update requires a previous snapshot");
        let mut statement = sql(transaction.prepare(MEMORY_INDEX_UPSERT))?;
        macro_rules! update_records {
            ($field:ident, $variant:ident) => {{
                for (ordinal, (old, new)) in previous.$field.iter().zip(&memory.$field).enumerate()
                {
                    if old.valid != new.valid {
                        memory_index_record(
                            &mut statement,
                            actor,
                            ordinal,
                            IndexedMemoryValueRef::$variant(new),
                            MemoryIndexSource::Durable,
                        )?;
                    }
                }
                for (ordinal, new) in memory.$field.iter().enumerate().skip(previous.$field.len()) {
                    memory_index_record(
                        &mut statement,
                        actor,
                        ordinal,
                        IndexedMemoryValueRef::$variant(new),
                        MemoryIndexSource::Durable,
                    )?;
                }
            }};
        }
        update_records!(facts, Fact);
        update_records!(rules, Rule);
        update_records!(conditions, Condition);
    } else {
        sql(transaction.execute("DELETE FROM memory_index_record WHERE actor_id=?1", [actor]))?;
        index_memory_state_records(transaction, actor, memory, MemoryIndexSource::Durable)?;
    }
    sql(transaction.execute(
        "INSERT INTO memory_index_state(actor_id,revision) VALUES(?1,?2) ON CONFLICT(actor_id) DO UPDATE SET revision=excluded.revision",
        params![actor, integer(memory.revision)?],
    ))?;
    Ok(())
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
            create_relation_index_schema(&connection, MemoryIndexSource::Durable)?;
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
        if schema != SCHEMA_VERSION
            && !(allow_legacy
                && matches!(
                    schema.as_str(),
                    LEGACY_SCHEMA_VERSION
                        | ALPHA8_SCHEMA_VERSION
                        | PREVIOUS_SCHEMA_VERSION
                        | ALPHA10_SCHEMA_VERSION
                ))
        {
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
        if initialize {
            sql(store.connection.execute(
                "CREATE INDEX IF NOT EXISTS input_identity_fingerprint ON input_identity(fingerprint)",
                [],
            ))?;
        }
        Ok(store)
    }

    pub(crate) fn with_relation_index<T>(
        &self,
        actor: &str,
        read: impl FnOnce(&MemoryIndexSnapshot<'_, '_>) -> Result<T>,
    ) -> Result<Option<T>> {
        valid_id(actor)?;
        let transaction =
            Transaction::new_unchecked(&self.connection, TransactionBehavior::Deferred)
                .map_err(|error| failure("STORE_SQLITE", error.to_string()))?;
        let head: Option<i64> = sql(transaction.query_row(
            "SELECT revision FROM memory_revision WHERE actor_id=?1 ORDER BY revision DESC LIMIT 1",
            [actor],
            |row| row.get(0),
        ).optional())?;
        let head = head.unwrap_or(0);
        let indexed: Option<i64> = sql(transaction
            .query_row(
                "SELECT revision FROM memory_index_state WHERE actor_id=?1",
                [actor],
                |row| row.get(0),
            )
            .optional())?;
        if indexed != Some(head) {
            sql(transaction.commit())?;
            return Ok(None);
        }
        let revision = u64::try_from(head)
            .map_err(|_| failure("INTEGRITY_FAILURE", "memory index revision is negative"))?;
        let snapshot = MemoryIndexSnapshot {
            transaction: &transaction,
            actor: actor.into(),
            revision,
            source: MemoryIndexSource::Durable,
        };
        let value = read(&snapshot)?;
        sql(transaction.commit())?;
        Ok(Some(value))
    }

    /// Use the same selectors as the durable index, keeping only keys and
    /// ordinals in SQLite. Selected values come from the authenticated snapshot.
    pub(crate) fn with_snapshot_relation_index<T>(
        actor: &str,
        memory: &MemoryState,
        read: impl FnOnce(&MemoryIndexSnapshot<'_, '_>) -> Result<T>,
    ) -> Result<T> {
        valid_id(actor)?;
        let mut connection = sql(Connection::open_in_memory())?;
        sql(connection.pragma_update(None, "temp_store", "MEMORY"))?;
        let source = MemoryIndexSource::Snapshot(memory);
        create_relation_index_schema(&connection, source)?;
        let transaction = sql(connection.transaction())?;
        index_memory_state_records(&transaction, actor, memory, source)?;
        let value = read(&MemoryIndexSnapshot {
            transaction: &transaction,
            actor: actor.into(),
            revision: memory.revision,
            source,
        })?;
        sql(transaction.commit())?;
        Ok(value)
    }

    /// Rebuilds a cold or stale derived index from one already loaded snapshot.
    /// The source revision and digest are rechecked under the write lock so a
    /// concurrent memory update cannot publish an index for the wrong head.
    pub(crate) fn rebuild_relation_index_if_current(
        &self,
        actor: &str,
        memory: &MemoryState,
    ) -> Result<bool> {
        valid_id(actor)?;
        let bytes = canonical(memory)?;
        let sha = digest(&bytes);
        let transaction = self.transaction()?;
        let head: Option<(i64, String)> = sql(transaction.query_row(
            "SELECT revision,record_sha256 FROM memory_revision WHERE actor_id=?1 ORDER BY revision DESC LIMIT 1",
            [actor],
            |row| Ok((row.get(0)?, row.get(1)?)),
        ).optional())?;
        let is_current = match head {
            Some((revision, stored_sha)) => {
                u64::try_from(revision).ok() == Some(memory.revision) && stored_sha == sha
            }
            None => memory == &MemoryState::default(),
        };
        if !is_current {
            sql(transaction.commit())?;
            return Ok(false);
        }
        let revision = integer(memory.revision)?;
        let indexed: Option<i64> = sql(transaction
            .query_row(
                "SELECT revision FROM memory_index_state WHERE actor_id=?1",
                [actor],
                |row| row.get(0),
            )
            .optional())?;
        if indexed == Some(revision) {
            sql(transaction.commit())?;
            return Ok(false);
        }
        sql(transaction.execute("DELETE FROM memory_index_record WHERE actor_id=?1", [actor]))?;
        index_memory_state_records(&transaction, actor, memory, MemoryIndexSource::Durable)?;
        sql(transaction.execute(
            "INSERT INTO memory_index_state(actor_id,revision) VALUES(?1,?2) ON CONFLICT(actor_id) DO UPDATE SET revision=excluded.revision",
            params![actor, revision],
        ))?;
        sql(transaction.commit())?;
        Ok(true)
    }

    /// Explicitly upgrades alpha 7, alpha 8, alpha 9 or alpha 10 store metadata after
    /// verifying every referenced blob, case journal, action journal and
    /// conflict chain.
    /// Immutable case revision blobs keep their original schema identity.
    /// Returns the observed source and current schema versions.
    pub fn migrate_to_current(path: impl AsRef<Path>) -> Result<(String, String)> {
        let path = path.as_ref();
        let store = Self::open_internal(path, true, false)?;
        let transaction = store.transaction()?;
        // Recheck under the SQLite write lock so no legacy writer can append
        // state between verification and the metadata transition.
        store.verify_integrity()?;
        let schema: String = sql(transaction.query_row(
            "SELECT value FROM store_metadata WHERE key='schema'",
            [],
            |row| row.get(0),
        ))?;
        if schema == SCHEMA_VERSION {
            sql(transaction.commit())?;
            return Ok((schema.clone(), schema));
        }
        if !matches!(
            schema.as_str(),
            LEGACY_SCHEMA_VERSION
                | ALPHA8_SCHEMA_VERSION
                | PREVIOUS_SCHEMA_VERSION
                | ALPHA10_SCHEMA_VERSION
        ) {
            return Err(failure(
                "SCHEMA_MISMATCH",
                format!(
                    "explicit migration supports only {LEGACY_SCHEMA_VERSION}, {ALPHA8_SCHEMA_VERSION}, {PREVIOUS_SCHEMA_VERSION} or {ALPHA10_SCHEMA_VERSION}"
                ),
            ));
        }
        let changed = sql(transaction.execute(
            "UPDATE store_metadata SET value=?1 WHERE key='schema' AND value=?2",
            params![SCHEMA_VERSION, schema],
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
        Ok((schema, SCHEMA_VERSION.into()))
    }

    /// Historical entry point retained for source compatibility. New callers
    /// should use `migrate_to_current`, which also accepts alpha 8 and alpha 9 stores.
    #[deprecated(note = "use migrate_to_current")]
    pub fn migrate_alpha7_to_alpha8(path: impl AsRef<Path>) -> Result<bool> {
        let (from, to) = Self::migrate_to_current(path)?;
        Ok(from != to)
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
        self.read_case_artifact(&self.connection, case, sha)
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

    fn read_case_artifact(
        &self,
        connection: &Connection,
        case: &str,
        sha: &str,
    ) -> Result<Vec<u8>> {
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
        self.read_blob_bytes(sha)
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

    /// An already registered request was stopped before sealing or entering the
    /// case. Preserve its response without changing the predecessor's journal.
    pub(crate) fn record_admission_stop(
        &self,
        actor: &str,
        request: &RequestEnvelope,
        result: &ResultRecord,
    ) -> Result<()> {
        if actor != request.actor_id
            || result.id != identity(request)?
            || result.input_id != request.input_id
            || result.input_version != request.input_version
            || result.lifecycle != Lifecycle::Suspend
            || result.reason.is_none()
            || result.calculation.is_some()
            || result.content_plan.is_some()
            || result.render.is_some()
            || !result.receipts.is_empty()
            || !result.evidence.is_empty()
            || !result.dependencies.is_empty()
            || result.actor_memory_revision.is_some()
            || result.execution_graph_ref.is_some()
        {
            return Err(failure(
                "INVALID_ADMISSION_RESULT",
                "only a stop without execution or effects can be recorded before admission",
            ));
        }
        let case = &request.case_id;
        let request_sha = digest(&canonical(request)?);
        Self::require_case(&self.connection, actor, case)?;
        let (sha, length) = self.publish_blob(result)?;
        let transaction = self.transaction()?;
        Self::require_case(&transaction, actor, case)?;
        Self::reference(&transaction, &sha, length, "result")?;
        let changed = sql(transaction.execute(
            "UPDATE input_identity SET result_sha256=?4
             WHERE case_id=?1 AND input_id=?2 AND input_version=?3
               AND fingerprint=?5 AND request_sha256=?6
               AND bundle_sha256 IS NULL AND result_sha256 IS NULL",
            params![
                case,
                result.input_id,
                integer(result.input_version)?,
                sha,
                result.id,
                request_sha
            ],
        ))?;
        if changed != 1 {
            return Err(failure(
                "ADMISSION_RESULT_NOT_RECORDED",
                "the original registered request is absent or already has a seal or result",
            ));
        }
        sql(transaction.commit())?;
        Ok(())
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

    /// Recover the registered original of a case that was cancelled before sealing.
    pub(crate) fn registered_request_by_fingerprint(
        &self,
        actor: &str,
        case: &str,
        fingerprint: &str,
    ) -> Result<RequestEnvelope> {
        Self::require_case(&self.connection, actor, case)?;
        let registered: Option<(String, i64, String)> = sql(self.connection.query_row(
            "SELECT input_id,input_version,request_sha256 FROM input_identity WHERE case_id=?1 AND fingerprint=?2",
            params![case, fingerprint],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        ).optional())?;
        let Some((input, version, sha)) = registered else {
            return Err(failure(
                "INPUT_NOT_REGISTERED",
                "the case revision has no registered original request",
            ));
        };
        let request: RequestEnvelope = self.read_blob(&sha)?;
        if request.actor_id != actor
            || request.case_id != case
            || request.input_id != input
            || integer(request.input_version)? != version
            || identity(&request)? != fingerprint
        {
            return Err(failure(
                "INTEGRITY_FAILURE",
                "the registered original request differs from its case identity",
            ));
        }
        Ok(request)
    }

    fn latest_case(&self, case: &str) -> Result<Option<CaseRevision>> {
        self.latest_case_ref(case)?
            .map(|sha| self.read_blob(&sha))
            .transpose()
    }

    fn latest_case_ref(&self, case: &str) -> Result<Option<String>> {
        sql(self.connection.query_row(
            "SELECT state_sha256 FROM case_revision WHERE case_id=?1 ORDER BY revision DESC LIMIT 1", [case], |r| r.get(0),
        ).optional())
    }

    /// Compare the head with an exact revision already verified and held by Core.
    pub(crate) fn is_case_head(&self, actor: &str, case: &str, reference: &str) -> Result<bool> {
        Self::require_case(&self.connection, actor, case)?;
        Ok(self.latest_case_ref(case)?.as_deref() == Some(reference))
    }

    pub fn append_case(&self, actor: &str, revision: &CaseRevision) -> Result<()> {
        self.append_case_before(actor, revision, None)
    }

    pub(crate) fn append_case_before(
        &self,
        actor: &str,
        revision: &CaseRevision,
        deadline: Option<Instant>,
    ) -> Result<()> {
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
        if deadline.is_some_and(|limit| Instant::now() >= limit) {
            return Err(failure(
                "DEADLINE",
                "the Goal deadline elapsed before case persistence",
            ));
        }
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
            self.read_case_artifact(&transaction, &revision.case_id, goal)?;
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
        if deadline.is_some_and(|limit| Instant::now() >= limit) {
            return Err(failure(
                "DEADLINE",
                "the Goal deadline elapsed before case persistence",
            ));
        }
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
        self.read_input_result(&self.connection, case, input, version)
    }

    fn read_input_result(
        &self,
        connection: &Connection,
        case: &str,
        input: &str,
        version: u64,
    ) -> Result<Option<ResultRecord>> {
        let sha: Option<String> = sql(connection.query_row(
            "SELECT result_sha256 FROM input_identity WHERE case_id=?1 AND input_id=?2 AND input_version=?3",
            params![case, input, integer(version)?], |r| r.get(0),
        ).optional())?.flatten();
        sha.map(|sha| self.read_blob(&sha)).transpose()
    }

    /// Recover the context at the original stop, before any later inputs.
    pub(crate) fn load_suspended_result_revision(
        &self,
        actor: &str,
        case: &str,
        result: &ResultRecord,
    ) -> Result<Option<CaseRevision>> {
        self.read_suspended_result_revision(&self.connection, case, result, || {
            self.get_input_result(actor, case, &result.input_id, result.input_version)
        })
    }

    fn read_suspended_result_revision(
        &self,
        connection: &Connection,
        case: &str,
        result: &ResultRecord,
        read_result: impl FnOnce() -> Result<Option<ResultRecord>>,
    ) -> Result<Option<CaseRevision>> {
        if result.lifecycle != Lifecycle::Suspend || read_result()?.as_ref() != Some(result) {
            return Ok(None);
        }
        let mut statement = sql(connection.prepare(
            "SELECT state_sha256 FROM case_revision WHERE case_id=?1 AND lifecycle='SUSPEND' ORDER BY revision",
        ))?;
        let rows = sql(statement.query_map([case], |row| row.get::<_, String>(0)))?;
        for row in rows {
            let revision: CaseRevision = self.read_blob(&sql(row)?)?;
            if revision.lifecycle == Lifecycle::Suspend && revision.result.as_ref() == Some(result)
            {
                return Ok(Some(revision));
            }
        }
        Ok(None)
    }

    /// Find the preceding result at the start of the input in a verified revision.
    pub(crate) fn input_start_predecessor(
        &self,
        state: &CaseRevision,
    ) -> Result<Option<ResultRecord>> {
        let refs: Option<(String, Option<String>)> = sql(self
            .connection
            .query_row(
                "SELECT received.state_sha256, prior.state_sha256
             FROM case_revision AS received
             LEFT JOIN case_revision AS prior
               ON prior.case_id=received.case_id AND prior.revision=received.parent_revision
             WHERE received.case_id=?1 AND received.revision<?2 AND received.lifecycle='RECEIVED'
             ORDER BY received.revision DESC LIMIT 1",
                params![state.case_id, integer(state.revision)?],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional())?;
        let Some((received_ref, Some(prior_ref))) = refs else {
            return Ok(None);
        };
        let received: CaseRevision = self.read_blob(&received_ref)?;
        if received.lifecycle != Lifecycle::Received
            || received.input_fingerprint != state.input_fingerprint
        {
            return Ok(None);
        }
        let previous: CaseRevision = self.read_blob(&prior_ref)?;
        Ok(previous.result)
    }

    /// Locate the original input key for a fingerprint owned by this actor.
    /// Consumers still verify the sealed input and result in its case history.
    pub(crate) fn owned_source_input_key(
        &self,
        actor: &str,
        fingerprint: &str,
    ) -> Result<Option<(String, String, u64)>> {
        let source_key: Option<(String, String, i64)> = sql(self.connection.query_row(
            "SELECT i.case_id,i.input_id,i.input_version FROM input_identity i JOIN case_registry c ON c.case_id=i.case_id WHERE c.actor_id=?1 AND i.fingerprint=?2 ORDER BY i.rowid LIMIT 1",
            params![actor, fingerprint],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        ).optional())?;
        source_key
            .map(|(case, input, version)| {
                let version = u64::try_from(version).map_err(|_| {
                    failure(
                        "SOURCE_EVIDENCE_IDENTITY",
                        "fact source evidence has a negative input version",
                    )
                })?;
                Ok((case, input, version))
            })
            .transpose()
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
        self.read_input_bundle(&self.connection, actor, case, input, version)
    }

    /// Keep all history lookups for one proof on the same read snapshot.
    /// The caller must finish reading before advancing the case or issuing an effect.
    pub(crate) fn with_verified_case_history<T>(
        &self,
        actor: &str,
        case: &str,
        read: impl FnOnce(&VerifiedCaseHistory<'_>) -> Result<T>,
    ) -> Result<T> {
        let transaction = sql(Transaction::new_unchecked(
            &self.connection,
            TransactionBehavior::Deferred,
        ))?;
        Self::require_case(&transaction, actor, case)?;
        self.verify_case(case)?;
        let value = read(&VerifiedCaseHistory {
            store: self,
            connection: &transaction,
            actor,
            case,
        })?;
        sql(transaction.commit())?;
        Ok(value)
    }

    fn read_input_bundle(
        &self,
        connection: &Connection,
        actor: &str,
        case: &str,
        input: &str,
        version: u64,
    ) -> Result<Option<(String, InputBundle)>> {
        let registered: Option<(String, String, Option<String>)> = sql(connection.query_row(
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
        self.save_action_inner(actor, action, None)
    }

    /// Prepare and issue a local write only while its adopted case revision is
    /// active and its deadline remains open. Each phase and the head check
    /// share one write transaction.
    pub(crate) fn save_live_file_phase(
        &self,
        actor: &str,
        action: &ActionRecord,
        adopted_revision: u64,
        deadline: Instant,
    ) -> Result<()> {
        if action.effect_class != "LOCAL_WRITE"
            || !matches!(
                action.phase,
                EffectPhase::Proposed
                    | EffectPhase::Authorized
                    | EffectPhase::Prepared
                    | EffectPhase::Attempted
            )
        {
            return Err(failure(
                "INVALID_EFFECT_TRANSITION",
                "live file preparation requires a local write at or before ATTEMPTED",
            ));
        }
        self.save_action_inner(actor, action, Some((adopted_revision, deadline)))
    }

    fn save_action_inner(
        &self,
        actor: &str,
        action: &ActionRecord,
        live: Option<(u64, Instant)>,
    ) -> Result<()> {
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
        let published = live
            .is_none()
            .then(|| self.publish_blob(action))
            .transpose()?;
        let transaction = self.transaction()?;
        Self::require_case(&transaction, actor, &action.case_id)?;
        if let Some((revision, deadline)) = live {
            let head = self.latest_case(&action.case_id)?;
            if !head.is_some_and(|head| {
                head.revision == revision
                    && head.lifecycle == Lifecycle::Adopted
                    && head.input_fingerprint.as_ref() == Some(&action.input_fingerprint)
                    && head
                        .result
                        .as_ref()
                        .and_then(|record| record.render.as_ref())
                        .is_some_and(|render| {
                            render.actual_sha256 == action.exact_request_sha256
                                && matches!(
                                    &render.target,
                                    Some(RenderTarget::LocalFile { path })
                                        if path.to_str() == Some(action.target.as_str())
                                )
                        })
            }) {
                return Err(failure(
                    "EFFECT_GOAL_NOT_ACTIVE",
                    "the adopted input revision for this local effect is no longer active",
                ));
            }
            if Instant::now() >= deadline {
                return Err(failure(
                    "DEADLINE",
                    "file action phase is past the Goal deadline",
                ));
            }
        }
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
                if live.is_some() && action.phase == EffectPhase::Attempted {
                    return Err(failure(
                        "EFFECT_ALREADY_ATTEMPTED",
                        "an existing attempt does not authorize another local write",
                    ));
                }
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
        self.read_case_artifact(&transaction, &action.case_id, &action.authorization_sha256)?;
        self.read_case_artifact(&transaction, &action.case_id, &action.exact_request_sha256)?;
        let (sha, length) = match published {
            Some(blob) => blob,
            None => self.publish_blob(action)?,
        };
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
        if live.is_some_and(|(_, deadline)| Instant::now() >= deadline) {
            return Err(failure(
                "DEADLINE",
                "file action phase is past the Goal deadline",
            ));
        }
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
            .chain(
                memory
                    .conditions
                    .iter()
                    .map(|condition| condition.id.as_str()),
            )
            .collect();
        if ids.len() != memory.facts.len() + memory.rules.len() + memory.conditions.len() {
            return Err(failure(
                "INTEGRITY_FAILURE",
                "duplicate actor memory identity",
            ));
        }
        let condition_records = memory
            .conditions
            .iter()
            .map(|condition| (condition.definition.condition_id.as_str(), condition))
            .collect::<BTreeMap<_, _>>();
        if condition_records.len() != memory.conditions.len()
            || condition_records.keys().any(|id| ids.contains(id))
        {
            return Err(failure(
                "INTEGRITY_FAILURE",
                "condition IDs must be unique and distinct from memory record IDs",
            ));
        }
        let mut verified_sources = BTreeSet::new();
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
            .chain(
                memory
                    .conditions
                    .iter()
                    .map(|condition| (&condition.source_input, &condition.dependencies)),
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
            if verified_sources.contains(source.as_str()) {
                continue;
            }
            let mut ownership = sql(self.connection.prepare_cached(
                "SELECT EXISTS(SELECT 1 FROM input_identity i JOIN case_registry c ON c.case_id=i.case_id WHERE c.actor_id=?1 AND i.fingerprint=?2)",
            ))?;
            let owned: bool = sql(ownership.query_row(params![actor, source], |row| row.get(0)))?;
            if !owned {
                return Err(failure(
                    "ACCESS_DENIED",
                    "memory provenance is not an original input owned by this actor",
                ));
            }
            // Only reuse successful ownership checks within this actor snapshot.
            verified_sources.insert(source.as_str());
        }
        for condition in &memory.conditions {
            crate::relations::validate_condition_definition(&condition.definition)?;
            if condition.semantics_version != minidora_domain::CONDITION_SEMANTICS_VERSION
                || condition.source_input.is_empty()
                || condition
                    .dependencies
                    .windows(2)
                    .any(|pair| pair[0] >= pair[1])
            {
                return Err(failure(
                    "INTEGRITY_FAILURE",
                    "condition record version, provenance, or dependencies are invalid",
                ));
            }
            if !condition.valid {
                continue;
            }
            let expected_dependencies = condition
                .definition
                .dependencies
                .iter()
                .filter_map(|condition_id| {
                    condition_records
                        .get(condition_id.as_str())
                        .filter(|record| record.valid)
                        .map(|record| record.id.as_str())
                })
                .collect::<BTreeSet<_>>();
            // The recorded IDs are already checked to be strictly sorted.
            // Every expected ID comes from an active condition record.
            if !condition
                .dependencies
                .iter()
                .map(String::as_str)
                .eq(expected_dependencies)
            {
                return Err(failure(
                    "INTEGRITY_FAILURE",
                    "active condition dependency records differ from the definition",
                ));
            }
        }
        let mut evidence_facts = memory
            .facts
            .iter()
            .filter_map(|fact| Some((fact, fact.source_evidence.as_ref()?)))
            .peekable();
        if evidence_facts.peek().is_none() {
            return Ok(());
        }
        // All source lookups and case proofs use this connection's read
        // snapshot. A caller inside a transaction already owns that snapshot.
        let transaction = if self.connection.is_autocommit() {
            Some(sql(Transaction::new_unchecked(
                &self.connection,
                TransactionBehavior::Deferred,
            ))?)
        } else {
            None
        };
        let mut verified_cases = BTreeSet::new();
        let mut verified_evidence = BTreeMap::new();
        for (fact, evidence) in evidence_facts {
            if let Some(verified) = verified_evidence.get(fact.source_input.as_str()) {
                if *verified != evidence {
                    return Err(failure(
                        "SOURCE_EVIDENCE_MISMATCH",
                        "fact source evidence differs from its immutable sealed input",
                    ));
                }
                continue;
            }
            let Some((case, input, version)) =
                self.owned_source_input_key(actor, &fact.source_input)?
            else {
                return Err(failure(
                    "SOURCE_EVIDENCE_UNOWNED",
                    "fact source evidence does not resolve to an actor-owned sealed input",
                ));
            };
            let case_verified = verified_cases.contains(&case);
            let original = if case_verified {
                self.read_input_bundle(&self.connection, actor, &case, &input, version)?
            } else {
                self.get_input_bundle(actor, &case, &input, version)?
            };
            let Some((_, bundle)) = original else {
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
            if !case_verified {
                verified_cases.insert(case);
            }
            // Borrow the Fact's exact evidence only after comparing it to the
            // sealed input in this actor's current read snapshot.
            verified_evidence.insert(fact.source_input.as_str(), evidence);
        }
        if let Some(transaction) = transaction {
            sql(transaction.commit())?;
        }
        Ok(())
    }

    pub fn load_memory(&self, actor: &str) -> Result<MemoryState> {
        Ok(self.load_memory_with_reference(actor)?.0)
    }

    pub(crate) fn load_memory_with_reference(
        &self,
        actor: &str,
    ) -> Result<(MemoryState, Option<(i64, String)>)> {
        valid_id(actor)?;
        let reference: Option<(i64, String)> = sql(self.connection.query_row(
            "SELECT revision,record_sha256 FROM memory_revision WHERE actor_id=?1 ORDER BY revision DESC LIMIT 1",
            [actor], |row| Ok((row.get(0)?,row.get(1)?)),
        ).optional())?;
        let memory = reference
            .as_ref()
            .map(|(_, sha)| self.read_blob(sha))
            .transpose()?
            .unwrap_or_default();
        self.validate_memory_scope(actor, &memory)?;
        Ok((memory, reference))
    }

    /// Loads one retained actor-memory snapshot without substituting the
    /// current head. Result recovery uses this to reproduce the exact memory
    /// version consumed by Core before checking that version against today's
    /// still-valid proof.
    pub fn load_memory_revision(&self, actor: &str, revision: u64) -> Result<Option<MemoryState>> {
        Ok(self
            .load_memory_revision_with_digest(actor, revision)?
            .map(|(memory, _)| memory))
    }

    pub(crate) fn load_memory_revision_with_digest(
        &self,
        actor: &str,
        revision: u64,
    ) -> Result<Option<(MemoryState, String)>> {
        valid_id(actor)?;
        let revision = integer(revision)?;
        let sha: Option<String> = sql(self
            .connection
            .query_row(
                "SELECT record_sha256 FROM memory_revision WHERE actor_id=?1 AND revision=?2",
                params![actor, revision],
                |row| row.get(0),
            )
            .optional())?;
        let Some(sha) = sha else {
            return Ok(None);
        };
        let memory: MemoryState = self.read_blob(&sha)?;
        let stored_revision = u64::try_from(revision)
            .map_err(|_| failure("INTEGRITY_FAILURE", "actor memory revision is negative"))?;
        if memory.revision != stored_revision {
            return Err(failure(
                "INTEGRITY_FAILURE",
                "actor memory revision row differs from its immutable snapshot",
            ));
        }
        self.validate_memory_scope(actor, &memory)?;
        Ok(Some((memory, sha)))
    }

    fn validate_memory_evolution(previous: &MemoryState, next: &MemoryState) -> Result<()> {
        if previous.revision.checked_add(1) != Some(next.revision) {
            return Err(failure(
                "REVISION_CONFLICT",
                "actor memory revision compare-and-swap failed",
            ));
        }

        macro_rules! preserve_append_only_records {
            ($field:ident, $kind:literal) => {{
                let old_records = &previous.$field;
                let new_records = &next.$field;
                if new_records.len() < old_records.len() {
                    return Err(failure(
                        "MEMORY_HISTORY_IMMUTABLE",
                        concat!("actor memory ", $kind, " history cannot be truncated"),
                    ));
                }
                for (old, new) in old_records.iter().zip(new_records) {
                    if old.id != new.id {
                        return Err(failure(
                            "MEMORY_HISTORY_IMMUTABLE",
                            concat!(
                                "actor memory ",
                                $kind,
                                " records cannot be reordered or replaced"
                            ),
                        ));
                    }
                    if !old.valid && new.valid {
                        return Err(failure(
                            "MEMORY_HISTORY_IMMUTABLE",
                            concat!(
                                "invalidated actor memory ",
                                $kind,
                                " records cannot be reactivated"
                            ),
                        ));
                    }
                    let mut expected = old.clone();
                    expected.valid = new.valid;
                    if expected != *new {
                        return Err(failure(
                            "MEMORY_HISTORY_IMMUTABLE",
                            concat!("actor memory ", $kind, " record content is immutable"),
                        ));
                    }
                }
            }};
        }

        preserve_append_only_records!(facts, "Fact");
        preserve_append_only_records!(rules, "Rule");
        preserve_append_only_records!(conditions, "Condition");
        Ok(())
    }

    pub fn save_memory(&self, actor: &str, memory: &MemoryState) -> Result<()> {
        valid_id(actor)?;
        self.validate_memory_scope(actor, memory)?;
        let bytes = canonical(memory)?;
        let sha = digest(&bytes);
        let old: Option<(i64, String)> = sql(self.connection.query_row(
            "SELECT revision,record_sha256 FROM memory_revision WHERE actor_id=?1 ORDER BY revision DESC LIMIT 1",
            [actor],
            |row| Ok((row.get(0)?, row.get(1)?)),
        ).optional())?;
        let previous_memory = if let Some((revision, old_sha)) = &old {
            let previous: MemoryState = self.read_blob(old_sha)?;
            let stored_revision = u64::try_from(*revision)
                .map_err(|_| failure("INTEGRITY_FAILURE", "actor memory revision is negative"))?;
            if previous.revision != stored_revision {
                return Err(failure(
                    "INTEGRITY_FAILURE",
                    "actor memory revision row differs from its immutable snapshot",
                ));
            }
            if *old_sha == sha {
                return Ok(());
            }
            Self::validate_memory_evolution(&previous, memory)?;
            Some(previous)
        } else if memory.revision > 1 {
            return Err(failure(
                "REVISION_CONFLICT",
                "first memory revision must be zero or one",
            ));
        } else {
            None
        };

        let (published_sha, length) = self.publish_bytes(&bytes)?;
        if published_sha != sha {
            return Err(failure(
                "INTEGRITY_FAILURE",
                "published actor memory digest differs from its canonical snapshot",
            ));
        }
        let transaction = self.transaction()?;
        let current: Option<(i64, String)> = sql(transaction.query_row(
            "SELECT revision,record_sha256 FROM memory_revision WHERE actor_id=?1 ORDER BY revision DESC LIMIT 1", [actor], |r| Ok((r.get(0)?,r.get(1)?)),
        ).optional())?;
        if current != old {
            return Err(failure(
                "REVISION_CONFLICT",
                "actor memory changed while a new immutable revision was prepared",
            ));
        }
        Self::reference(&transaction, &sha, length, "memory")?;
        sql(transaction.execute(
            "INSERT INTO memory_revision(actor_id,revision,record_sha256) VALUES(?1,?2,?3)",
            params![actor, integer(memory.revision)?, sha],
        ))?;
        sync_memory_index(&transaction, actor, previous_memory.as_ref(), memory)?;
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
            if !matches!(
                revision.schema_version.as_str(),
                SCHEMA_VERSION
                    | LEGACY_SCHEMA_VERSION
                    | ALPHA8_SCHEMA_VERSION
                    | PREVIOUS_SCHEMA_VERSION
                    | ALPHA10_SCHEMA_VERSION
            ) || revision.actor_id != owner
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

fn create_relation_index_schema(
    connection: &Connection,
    source: MemoryIndexSource<'_>,
) -> Result<()> {
    let payload_columns = match source {
        MemoryIndexSource::Durable => {
            "payload_json TEXT NOT NULL,payload_sha256 TEXT NOT NULL CHECK(length(payload_sha256)=64)"
        }
        MemoryIndexSource::Snapshot(_) => {
            "payload_json TEXT CHECK(payload_json IS NULL),payload_sha256 TEXT CHECK(payload_sha256 IS NULL)"
        }
    };
    sql(connection.execute_batch(&format!(r#"
CREATE TABLE IF NOT EXISTS memory_index_record(
 actor_id TEXT NOT NULL,record_id TEXT NOT NULL,record_kind TEXT NOT NULL CHECK(record_kind IN ('FACT','RULE','CONDITION')),
 ordinal INTEGER NOT NULL CHECK(ordinal>=0),{payload_columns},
 valid INTEGER NOT NULL CHECK(valid IN (0,1)),subject TEXT,predicate TEXT,object TEXT,negative INTEGER,scope TEXT,
 temporal_scope TEXT,condition_refs TEXT,condition_id TEXT,
 PRIMARY KEY(actor_id,record_id));
CREATE INDEX IF NOT EXISTS memory_index_relation_lookup
 ON memory_index_record(actor_id,valid,record_kind,predicate,scope,temporal_scope,condition_refs,subject,object);
CREATE INDEX IF NOT EXISTS memory_index_common_subject_scope
 ON memory_index_record(actor_id,valid,scope,subject,record_kind);
CREATE INDEX IF NOT EXISTS memory_index_scope
 ON memory_index_record(actor_id,valid,scope,record_kind,ordinal);
CREATE INDEX IF NOT EXISTS memory_index_profile_subject
 ON memory_index_record(actor_id,valid,subject,record_kind);
CREATE INDEX IF NOT EXISTS memory_index_condition_evidence
 ON memory_index_record(actor_id,valid,record_kind,subject,predicate,object,scope,temporal_scope);
CREATE INDEX IF NOT EXISTS memory_index_condition_id
 ON memory_index_record(actor_id,valid,record_kind,condition_id);
"#)))
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
CREATE TABLE IF NOT EXISTS memory_index_state(actor_id TEXT PRIMARY KEY,revision INTEGER NOT NULL CHECK(revision>=0));
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
    use minidora_domain::{
        ConditionDefinition, ConditionExpression, ConditionRecord, Fact, Guarantee, InputBundle,
        Intent, MeaningIR, Permissions, Proposition, RelationRule,
    };
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
    fn transient_relation_index_reads_exact_snapshot_records_without_storing_payloads() {
        let proposition = Proposition {
            subject: "ユリ".into(),
            predicate: "状態".into(),
            object: "校正済み".into(),
            negative: false,
            scope: "実験室".into(),
            condition_refs: BTreeSet::new(),
            temporal_scope: None,
        };
        let fact = Fact {
            id: "fact-live".into(),
            proposition: proposition.clone(),
            guarantee: Guarantee::UserPremise,
            source_input: "test-input".into(),
            source_evidence: None,
            dependencies: vec!["fact-retained".into()],
            valid: true,
        };
        let retained = Fact {
            id: "fact-retained".into(),
            valid: false,
            dependencies: Vec::new(),
            ..fact.clone()
        };
        let memory = MemoryState {
            revision: 7,
            facts: vec![fact.clone(), retained.clone()],
            ..MemoryState::default()
        };

        Store::with_snapshot_relation_index("alice", &memory, |index| {
            let rows: (i64, i64, i64) = sql(index.transaction.query_row(
                "SELECT count(*),count(payload_json),count(payload_sha256) FROM memory_index_record",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            ))?;
            assert_eq!(rows, (2, 0, 0));
            assert_eq!(index.revision(), 7);
            let selected = index.relation_candidates(&proposition, 1)?;
            assert!(matches!(
                selected.as_slice(),
                [IndexedMemoryRecord { ordinal: 0, value: IndexedMemoryValue::Fact(value) }] if value == &fact
            ));
            assert!(matches!(
                index.record_by_id(&retained.id)?,
                Some(IndexedMemoryRecord { ordinal: 1, value: IndexedMemoryValue::Fact(value) }) if value == retained
            ));

            sql(index.transaction.execute(
                "UPDATE memory_index_record SET ordinal=1 WHERE record_id='fact-live'",
                [],
            ))?;
            assert!(matches!(
                index.record_by_id(&fact.id),
                Err(minidora_domain::Error::Contract { code, .. }) if code == "MEMORY_INDEX_CORRUPT"
            ));
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn reopening_an_existing_store_indexes_source_fingerprints_in_original_row_order() {
        let directory = Directory::new();
        let request = RequestEnvelope::new("case-a", "input-a", "alice", "隼は鳥");
        let fingerprint = identity(&request).unwrap();
        let mut engine = crate::core::Engine::open(&directory.0).unwrap();
        let result = engine.execute("alice", request).unwrap();
        assert_eq!(result.lifecycle, Lifecycle::Commit);
        drop(engine);

        let store = Store::open(&directory.0).unwrap();
        let original = store.load_memory("alice").unwrap();
        assert_eq!(original.facts[0].source_input, fingerprint);
        assert!(original.facts[0].source_evidence.is_some());

        // The Store API accepts nonunique fingerprints. A later alias in an
        // earlier-sorting case must not displace the original sealed evidence.
        let alias = RequestEnvelope::new("case-0", "input-a", "alice", "another request");
        assert!(store.register_input("alice", &alias, &fingerprint).unwrap());
        store
            .connection
            .execute("DROP INDEX input_identity_fingerprint", [])
            .unwrap();
        assert_eq!(store.load_memory("alice").unwrap(), original);
        drop(store);

        let reopened = Store::open(&directory.0).unwrap();
        let indexed_column: String = reopened
            .connection
            .query_row("PRAGMA index_info(input_identity_fingerprint)", [], |row| {
                row.get(2)
            })
            .unwrap();
        assert_eq!(indexed_column, "fingerprint");
        assert_eq!(reopened.load_memory("alice").unwrap(), original);
        assert_eq!(
            reopened
                .load_memory_revision("alice", original.revision)
                .unwrap(),
            Some(original)
        );
    }

    #[test]
    fn memory_source_validation_shares_case_history_only_within_one_read() {
        let directory = Directory::new();
        let mut engine = crate::core::Engine::open(&directory.0).unwrap();
        for (input, text) in [("input-a", "隼は鳥"), ("input-b", "梟は鳥")] {
            let request = RequestEnvelope::new("case-a", input, "alice", text);
            let result = engine.execute("alice", request).unwrap();
            assert_eq!(result.lifecycle, Lifecycle::Commit);
        }
        drop(engine);

        let store = Store::open(&directory.0).unwrap();
        let memory = store.load_memory("alice").unwrap();
        assert_eq!(memory.facts.len(), 2);
        assert!(
            memory
                .facts
                .iter()
                .all(|fact| fact.source_evidence.is_some())
        );
        assert_ne!(memory.facts[0].source_input, memory.facts[1].source_input);
        store
            .with_verified_case_history("alice", "case-a", |_| {
                assert_eq!(store.load_memory("alice")?, memory);
                assert_eq!(
                    store.load_memory_revision("alice", memory.revision)?,
                    Some(memory.clone())
                );
                assert!(!store.connection.is_autocommit());
                Ok(())
            })
            .unwrap();

        let mut forged = memory.clone();
        forged.facts[1]
            .source_evidence
            .as_mut()
            .unwrap()
            .semantic_signature
            .clear();
        assert!(matches!(
            store.save_memory("alice", &forged),
            Err(minidora_domain::Error::Contract { code, .. }) if code == "SOURCE_EVIDENCE_MISMATCH"
        ));
        assert!(store.connection.is_autocommit());
        assert_eq!(store.load_memory("alice").unwrap(), memory);

        let head = store.latest_case_ref("case-a").unwrap().unwrap();
        fs::write(store.blobs.join(head), b"corrupt case revision").unwrap();
        assert!(matches!(
            store.load_memory("alice"),
            Err(minidora_domain::Error::Contract { code, .. }) if code == "INTEGRITY_FAILURE"
        ));
        assert!(store.connection.is_autocommit());
    }

    #[test]
    fn correction_facts_share_verified_input_but_each_must_match_its_evidence() {
        let directory = Directory::new();
        let premise =
            RequestEnvelope::new("case-a", "premise", "alice", "[scope=別室]補助計器は携帯型");
        let correction = RequestEnvelope::new(
            "case-a",
            "correction",
            "alice",
            "[scope=別室]訂正: 補助計器は携帯型ではなく据置型です",
        );
        let fingerprint = identity(&correction).unwrap();
        let mut engine = crate::core::Engine::open(&directory.0).unwrap();
        for request in [premise, correction] {
            let result = engine.execute("alice", request).unwrap();
            assert_eq!(result.lifecycle, Lifecycle::Commit, "{result:?}");
        }
        drop(engine);

        let store = Store::open(&directory.0).unwrap();
        let memory = store.load_memory("alice").unwrap();
        let [first, second] = memory
            .facts
            .iter()
            .enumerate()
            .filter_map(|(ordinal, fact)| (fact.source_input == fingerprint).then_some(ordinal))
            .collect::<Vec<_>>()
            .try_into()
            .expect("the correction records both negation and replacement");
        assert!(memory.facts[first].source_evidence.is_some());
        assert_eq!(
            memory.facts[first].source_evidence,
            memory.facts[second].source_evidence
        );

        let mut forged = memory.clone();
        forged.facts[second]
            .source_evidence
            .as_mut()
            .unwrap()
            .raw_sha256 = digest(b"not the correction");
        assert!(matches!(
            store.save_memory("alice", &forged),
            Err(minidora_domain::Error::Contract { code, .. }) if code == "SOURCE_EVIDENCE_MISMATCH"
        ));
        assert_eq!(store.load_memory("alice").unwrap(), memory);

        let request_sha: String = store
            .connection
            .query_row(
                "SELECT request_sha256 FROM input_identity WHERE case_id='case-a' AND input_id='correction'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        fs::write(store.blobs.join(request_sha), b"corrupt correction input").unwrap();
        assert!(matches!(
            store.load_memory("alice"),
            Err(minidora_domain::Error::Contract { code, .. }) if code == "INTEGRITY_FAILURE"
        ));
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

    #[test]
    fn verified_case_history_keeps_snapshot_and_rechecks_new_operations() {
        fn append_stopped_input(
            store: &Store,
            request: &RequestEnvelope,
            prior: Option<&CaseRevision>,
        ) -> (CaseRevision, InputBundle, ResultRecord) {
            assert!(
                store
                    .register_input("alice", request, &identity(request).unwrap())
                    .unwrap()
            );
            let mut initial = received(request);
            if let Some(prior) = prior {
                initial.revision = prior.revision + 1;
                initial.parent_revision = Some(prior.revision);
                initial.prior_results = prior.prior_results.clone();
                if let Some(result) = &prior.result {
                    initial
                        .prior_results
                        .insert(result.id.clone(), result.clone());
                }
            }
            store.append_case("alice", &initial).unwrap();
            let bundle = crate::input::seal(request, None).unwrap();
            let mut next = sealed(store, request, &initial);
            next.bundle = Some(bundle.clone());
            store.append_case("alice", &next).unwrap();
            let result = ResultRecord {
                id: identity(request).unwrap(),
                input_id: request.input_id.clone(),
                input_version: request.input_version,
                lifecycle: Lifecycle::Suspend,
                text: format!("retained test stop for {}", request.text),
                reason: Some("TEST_BOUNDARY".into()),
                guarantee: Guarantee::Unknown,
                calculation: None,
                content_plan: None,
                render: None,
                receipts: vec![],
                evidence: vec![],
                dependencies: vec![],
                actor_memory_revision: None,
                execution_graph_ref: None,
            };
            next.parent_revision = Some(next.revision);
            next.revision += 1;
            next.lifecycle = Lifecycle::Suspend;
            next.reason = result.reason.clone();
            next.result = Some(result.clone());
            store.append_case("alice", &next).unwrap();
            (next, bundle, result)
        }

        let directory = Directory::new();
        let store = Store::open(&directory.0).unwrap();
        let request = request();
        let (original_revision, original_bundle, original_result) =
            append_stopped_input(&store, &request, None);
        let expected_original = Some((
            digest(&canonical(&original_bundle).unwrap()),
            original_bundle,
        ));
        assert_eq!(
            store
                .get_input_bundle("alice", "case-a", "input-a", 1)
                .unwrap(),
            expected_original
        );
        assert_eq!(
            store
                .get_input_result("alice", "case-a", "input-a", 1)
                .unwrap(),
            Some(original_result.clone())
        );
        assert_eq!(
            store
                .load_suspended_result_revision("alice", "case-a", &original_result)
                .unwrap(),
            Some(original_revision.clone())
        );
        let mut wrong_result = original_result.clone();
        wrong_result
            .text
            .push_str(" changed without a new identity");
        let writer = Store::open(&directory.0).unwrap();
        let mut new_request = request.clone();
        new_request.input_version = 2;
        new_request.text = "3+4".into();

        let (new_revision, new_bundle, new_result) = store
            .with_verified_case_history("alice", "case-a", |history| {
                assert_eq!(history.input_bundle("input-a", 1)?, expected_original);
                assert_eq!(
                    history.input_result("input-a", 1)?,
                    Some(original_result.clone())
                );
                assert_eq!(
                    history.suspended_result_revision(&original_result)?,
                    Some(original_revision.clone())
                );
                assert!(history.suspended_result_revision(&wrong_result)?.is_none());
                assert!(history.input_bundle("input-a", 2)?.is_none());
                assert!(history.input_result("input-a", 2)?.is_none());
                let (revision, bundle, result) =
                    append_stopped_input(&writer, &new_request, Some(&original_revision));
                assert_eq!(
                    writer.get_input_bundle("alice", "case-a", "input-a", 2)?,
                    Some((digest(&canonical(&bundle)?), bundle.clone()))
                );
                assert_eq!(
                    writer.get_input_result("alice", "case-a", "input-a", 2)?,
                    Some(result.clone())
                );
                assert_eq!(
                    writer.load_suspended_result_revision("alice", "case-a", &result)?,
                    Some(revision.clone())
                );
                assert!(history.input_bundle("input-a", 2)?.is_none());
                assert!(history.input_result("input-a", 2)?.is_none());
                assert!(history.suspended_result_revision(&result)?.is_none());
                assert_eq!(history.input_bundle("input-a", 1)?, expected_original);
                assert_eq!(
                    history.input_result("input-a", 1)?,
                    Some(original_result.clone())
                );
                assert_eq!(
                    history.suspended_result_revision(&original_result)?,
                    Some(original_revision.clone())
                );
                Ok((revision, bundle, result))
            })
            .unwrap();

        store
            .with_verified_case_history("alice", "case-a", |history| {
                assert_eq!(history.input_bundle("input-a", 1)?, expected_original);
                assert_eq!(
                    history.suspended_result_revision(&original_result)?,
                    Some(original_revision)
                );
                assert_eq!(
                    history.suspended_result_revision(&new_result)?,
                    Some(new_revision)
                );
                assert!(history.suspended_result_revision(&wrong_result)?.is_none());
                assert_eq!(history.input_result("input-a", 1)?, Some(original_result));
                assert_eq!(
                    history.input_bundle("input-a", 2)?,
                    Some((digest(&canonical(&new_bundle)?), new_bundle))
                );
                assert_eq!(
                    history.input_result("input-a", 2)?,
                    Some(new_result.clone())
                );
                assert!(history.input_bundle("missing", 1)?.is_none());
                assert!(history.input_result("missing", 1)?.is_none());
                Ok(())
            })
            .unwrap();

        let called = std::cell::Cell::new(false);
        let error = store
            .with_verified_case_history("bob", "case-a", |_| {
                called.set(true);
                Ok(())
            })
            .unwrap_err();
        assert!(!called.get());
        assert!(
            matches!(error, minidora_domain::Error::Contract { code, .. } if code == "ACCESS_DENIED")
        );

        let result_ref = digest(&canonical(&new_result).unwrap());
        fs::write(
            directory.0.join("blobs").join(result_ref),
            b"corrupt retained result",
        )
        .unwrap();
        let error = store
            .with_verified_case_history("alice", "case-a", |history| {
                history.input_result("input-a", 2)
            })
            .unwrap_err();
        assert!(
            matches!(error, minidora_domain::Error::Contract { code, .. } if code == "INTEGRITY_FAILURE")
        );
        assert!(store.connection.is_autocommit());
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
        let bytes = b"\x00\xffexact evidence\n";
        let sha = store.put_bytes("alice", "case-a", bytes).unwrap();
        assert_eq!(store.get_bytes("alice", "case-a", &sha).unwrap(), bytes);
        assert!(matches!(
            store.get_bytes("bob", "case-a", &sha),
            Err(minidora_domain::Error::Contract { code, .. }) if code == "ACCESS_DENIED"
        ));
        let unrelated = RequestEnvelope::new("case-b", "input-a", "alice", "2+3");
        register(&store, &unrelated);
        assert!(matches!(
            store.get_bytes("alice", "case-b", &sha),
            Err(minidora_domain::Error::Contract { code, .. }) if code == "INTEGRITY_FAILURE"
        ));
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
        let error = Store::migrate_to_current(&directory.0).unwrap_err();
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

        assert_eq!(
            Store::migrate_to_current(&directory.0).unwrap(),
            (LEGACY_SCHEMA_VERSION.into(), SCHEMA_VERSION.into())
        );
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
        assert_eq!(
            Store::migrate_to_current(&directory.0).unwrap(),
            (SCHEMA_VERSION.into(), SCHEMA_VERSION.into())
        );
    }

    #[test]
    fn explicit_alpha8_migration_preserves_legacy_case_blob_and_is_idempotent() {
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
        legacy_revision.schema_version = ALPHA8_SCHEMA_VERSION.into();
        let (state_sha, state_length) = store.publish_blob(&legacy_revision).unwrap();
        let transaction = store.transaction().unwrap();
        Store::reference(&transaction, &state_sha, state_length, "case_revision").unwrap();
        let event_hash = identity(&(
            "case-revision-event-v1",
            &request.case_id,
            legacy_revision.revision,
            &Option::<String>::None,
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
                    Option::<String>::None,
                    event_hash,
                    "RECEIVED",
                    Option::<String>::None,
                ],
            )
            .unwrap();
        transaction
            .execute(
                "UPDATE store_metadata SET value=?1 WHERE key='schema'",
                [ALPHA8_SCHEMA_VERSION],
            )
            .unwrap();
        transaction.commit().unwrap();
        drop(store);

        assert!(matches!(
            Store::open(&directory.0),
            Err(minidora_domain::Error::Contract { code, .. }) if code == "SCHEMA_MISMATCH"
        ));
        assert_eq!(
            Store::migrate_to_current(&directory.0).unwrap(),
            (ALPHA8_SCHEMA_VERSION.into(), SCHEMA_VERSION.into())
        );
        let migrated = Store::open(&directory.0).unwrap();
        let latest = migrated.load_case("alice", "case-a").unwrap().unwrap();
        assert_eq!(latest.schema_version, ALPHA8_SCHEMA_VERSION);
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
        assert_eq!(
            Store::migrate_to_current(&directory.0).unwrap(),
            (SCHEMA_VERSION.into(), SCHEMA_VERSION.into())
        );
    }

    #[test]
    fn explicit_alpha9_migration_preserves_legacy_case_blob_and_is_idempotent() {
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
        legacy_revision.schema_version = PREVIOUS_SCHEMA_VERSION.into();
        let (state_sha, state_length) = store.publish_blob(&legacy_revision).unwrap();
        let transaction = store.transaction().unwrap();
        Store::reference(&transaction, &state_sha, state_length, "case_revision").unwrap();
        let event_hash = identity(&(
            "case-revision-event-v1",
            &request.case_id,
            legacy_revision.revision,
            &Option::<String>::None,
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
                    Option::<String>::None,
                    event_hash,
                    "RECEIVED",
                    Option::<String>::None,
                ],
            )
            .unwrap();
        transaction
            .execute(
                "UPDATE store_metadata SET value=?1 WHERE key='schema'",
                [PREVIOUS_SCHEMA_VERSION],
            )
            .unwrap();
        transaction.commit().unwrap();
        drop(store);

        assert!(matches!(
            Store::open(&directory.0),
            Err(minidora_domain::Error::Contract { code, .. }) if code == "SCHEMA_MISMATCH"
        ));
        assert_eq!(
            Store::migrate_to_current(&directory.0).unwrap(),
            (PREVIOUS_SCHEMA_VERSION.into(), SCHEMA_VERSION.into())
        );
        let migrated = Store::open(&directory.0).unwrap();
        let latest = migrated.load_case("alice", "case-a").unwrap().unwrap();
        assert_eq!(latest.schema_version, PREVIOUS_SCHEMA_VERSION);
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
        assert_eq!(
            Store::migrate_to_current(&directory.0).unwrap(),
            (SCHEMA_VERSION.into(), SCHEMA_VERSION.into())
        );
    }

    #[test]
    fn explicit_alpha10_migration_preserves_legacy_condition_refs_and_memory_blob() {
        let directory = Directory::new();
        let store = Store::open(&directory.0).unwrap();
        let request = request();
        let fingerprint = identity(&request).unwrap();
        assert!(
            store
                .register_input("alice", &request, &fingerprint)
                .unwrap()
        );
        let memory = MemoryState {
            revision: 1,
            facts: vec![Fact {
                id: "fact:legacy-condition-ref".into(),
                proposition: Proposition {
                    subject: "Nera".into(),
                    predicate: "is".into(),
                    object: "ore".into(),
                    negative: false,
                    scope: "lab".into(),
                    temporal_scope: None,
                    condition_refs: BTreeSet::from(["condition:legacy".into()]),
                },
                guarantee: Guarantee::UserPremise,
                source_input: fingerprint,
                source_evidence: None,
                dependencies: Vec::new(),
                valid: true,
            }],
            ..MemoryState::default()
        };
        store.save_memory("alice", &memory).unwrap();
        let memory_sha: String = store
            .connection
            .query_row(
                "SELECT record_sha256 FROM memory_revision WHERE actor_id='alice' AND revision=1",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let blob_path = directory.0.join("blobs").join(&memory_sha);
        let original_blob = fs::read(&blob_path).unwrap();
        drop(store);

        let connection = Connection::open(directory.0.join("state.sqlite3")).unwrap();
        connection
            .execute(
                "UPDATE store_metadata SET value=?1 WHERE key='schema'",
                [ALPHA10_SCHEMA_VERSION],
            )
            .unwrap();
        drop(connection);

        assert!(matches!(
            Store::open(&directory.0),
            Err(minidora_domain::Error::Contract { code, .. }) if code == "SCHEMA_MISMATCH"
        ));
        assert_eq!(
            Store::migrate_to_current(&directory.0).unwrap(),
            (ALPHA10_SCHEMA_VERSION.into(), SCHEMA_VERSION.into())
        );
        let migrated = Store::open(&directory.0).unwrap();
        let restored = migrated.load_memory("alice").unwrap();
        assert_eq!(restored, memory);
        assert!(restored.conditions.is_empty());
        assert_eq!(
            restored.facts[0].proposition.condition_refs,
            BTreeSet::from(["condition:legacy".into()])
        );
        assert_eq!(fs::read(&blob_path).unwrap(), original_blob);
        assert_eq!(
            Store::migrate_to_current(&directory.0).unwrap(),
            (SCHEMA_VERSION.into(), SCHEMA_VERSION.into())
        );
    }

    #[test]
    fn corrupt_or_missing_referenced_blob_blocks_restart() {
        for missing in [false, true] {
            let directory = Directory::new();
            let store = Store::open(&directory.0).unwrap();
            register(&store, &request());
            let sha = store
                .put_bytes("alice", "case-a", b"original evidence")
                .unwrap();
            let path = directory.0.join("blobs").join(&sha);
            if missing {
                fs::remove_file(path).unwrap();
            } else {
                fs::write(path, b"altered evidence").unwrap();
            }
            assert!(matches!(
                store.get_bytes("alice", "case-a", &sha),
                Err(minidora_domain::Error::Contract { code, .. }) if code == "INTEGRITY_FAILURE"
            ));
            drop(store);
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
    fn condition_dependencies_bind_to_active_records_in_the_whole_snapshot() {
        let directory = Directory::new();
        let store = Store::open(&directory.0).unwrap();
        let request = request();
        register(&store, &request);
        let source = identity(&request).unwrap();
        let mut memory = MemoryState::default();
        let fact = crate::relations::assert_fact(
            &mut memory,
            Proposition {
                subject: "Nera".into(),
                predicate: "is".into(),
                object: "ready".into(),
                negative: false,
                scope: "lab".into(),
                condition_refs: BTreeSet::new(),
                temporal_scope: None,
            },
            &source,
        )
        .unwrap();
        let atom = |id: &str| ConditionDefinition {
            condition_id: id.into(),
            subject: "Nera".into(),
            scope: "lab".into(),
            expression: ConditionExpression::Fact {
                predicate: "is".into(),
                object: id.into(),
            },
            valid_from: None,
            valid_until: None,
            dependencies: BTreeSet::new(),
        };
        let ready =
            crate::relations::define_condition(&mut memory, atom("ready"), &source).unwrap();
        crate::relations::define_condition(&mut memory, atom("checked"), &source).unwrap();
        let definition = ConditionDefinition {
            expression: ConditionExpression::And {
                operands: ["checked", "ready"]
                    .into_iter()
                    .map(|id| ConditionExpression::ConditionRef {
                        condition_id: id.into(),
                    })
                    .collect(),
            },
            dependencies: BTreeSet::from(["checked".into(), "ready".into()]),
            ..atom("approved")
        };
        let parent = crate::relations::define_condition(&mut memory, definition, &source).unwrap();
        memory.conditions.rotate_right(1);
        memory.revision = 1;
        store.save_memory("alice", &memory).unwrap();
        assert_eq!(store.load_memory("alice").unwrap(), memory);

        let mut wrong_kind = memory.clone();
        wrong_kind.conditions[0].dependencies[0] = fact.id;
        wrong_kind.conditions[0].dependencies.sort();
        let mut inactive_dependency = memory.clone();
        inactive_dependency.conditions[1].valid = false;
        for invalid in [&wrong_kind, &inactive_dependency] {
            assert!(matches!(
                store.validate_memory_scope("alice", invalid),
                Err(minidora_domain::Error::Contract { code, message })
                    if code == "INTEGRITY_FAILURE"
                        && message == "active condition dependency records differ from the definition"
            ));
        }

        let mut duplicate_definition = memory.clone();
        duplicate_definition.conditions[2].definition.condition_id = "ready".into();
        duplicate_definition.conditions[2].valid = false;
        assert!(matches!(
            store.validate_memory_scope("alice", &duplicate_definition),
            Err(minidora_domain::Error::Contract { code, message })
                if code == "INTEGRITY_FAILURE"
                    && message == "condition IDs must be unique and distinct from memory record IDs"
        ));

        let old = memory.clone();
        let invalidated = crate::relations::invalidate(&mut memory, &ready.id).unwrap();
        assert!(invalidated.contains(&parent.id));
        assert!(!memory.conditions[0].valid);
        assert_eq!(memory.conditions[0].dependencies, parent.dependencies);
        store.save_memory("alice", &memory).unwrap();
        assert_eq!(store.load_memory("alice").unwrap(), memory);
        assert_eq!(store.load_memory_revision("alice", 1).unwrap(), Some(old));
    }

    #[test]
    fn memory_revision_preserves_immutable_records_and_allows_tombstones() {
        let directory = Directory::new();
        let request = request();
        let fingerprint = identity(&request).unwrap();
        let store = Store::open(&directory.0).unwrap();
        register(&store, &request);

        let proposition = |object: &str| Proposition {
            subject: "Nera".into(),
            predicate: "is".into(),
            object: object.into(),
            negative: false,
            scope: "lab".into(),
            temporal_scope: None,
            condition_refs: BTreeSet::new(),
        };
        let fact = |id: &str, object: &str| Fact {
            id: id.into(),
            proposition: proposition(object),
            guarantee: Guarantee::UserPremise,
            source_input: fingerprint.clone(),
            source_evidence: None,
            dependencies: Vec::new(),
            valid: true,
        };
        let condition_definition = ConditionDefinition {
            condition_id: "condition:lab-check".into(),
            subject: "Nera".into(),
            scope: "lab".into(),
            expression: ConditionExpression::Fact {
                predicate: "is".into(),
                object: "ore".into(),
            },
            valid_from: None,
            valid_until: None,
            dependencies: BTreeSet::new(),
        };
        let condition = ConditionRecord {
            id: format!(
                "condition-record:{}",
                identity(&(
                    minidora_domain::CONDITION_SEMANTICS_VERSION,
                    &condition_definition
                ))
                .unwrap()
            ),
            semantics_version: minidora_domain::CONDITION_SEMANTICS_VERSION.into(),
            definition: condition_definition,
            source_input: fingerprint.clone(),
            dependencies: Vec::new(),
            valid: true,
        };
        let rule = RelationRule {
            id: "rule:root-one".into(),
            premise: Proposition {
                subject: "$subject".into(),
                ..proposition("ore")
            },
            additional_premises: Vec::new(),
            conclusion: Proposition {
                subject: "$subject".into(),
                ..proposition("mineral")
            },
            source_input: fingerprint.clone(),
            dependencies: Vec::new(),
            valid: true,
        };
        let original = MemoryState {
            revision: 1,
            facts: vec![fact("fact:original", "ore")],
            rules: vec![rule],
            conditions: vec![condition],
        };
        let mut bad_dependency = original.clone();
        bad_dependency.rules[0].dependencies = vec!["fact:missing".into()];
        assert!(matches!(
            store.save_memory("alice", &bad_dependency),
            Err(minidora_domain::Error::Contract { code, message })
                if code == "ACCESS_DENIED" && message.contains("dependency outside")
        ));

        let foreign = RequestEnvelope::new("case-b", "input-a", "bob", "2+3");
        let foreign_source = identity(&foreign).unwrap();
        assert!(
            store
                .register_input("bob", &foreign, &foreign_source)
                .unwrap()
        );
        for source in [foreign_source, digest(b"unregistered source")] {
            let mut bad_source = original.clone();
            bad_source.conditions[0].source_input = source;
            assert!(matches!(
                store.save_memory("alice", &bad_source),
                Err(minidora_domain::Error::Contract { code, message })
                    if code == "ACCESS_DENIED" && message.contains("provenance is not")
            ));
        }

        let mut derived = original.clone();
        let derived_source = "derived:proof-composition-v1";
        derived.rules[0].source_input = derived_source.into();
        derived.rules[0].dependencies = vec![original.facts[0].id.clone()];
        derived.conditions[0].source_input = derived_source.into();
        assert!(matches!(
            store.save_memory("alice", &derived),
            Err(minidora_domain::Error::Contract { code, message })
                if code == "ACCESS_DENIED" && message.contains("provenance is not")
        ));

        let original_bytes = canonical(&original).unwrap();
        store.save_memory("alice", &original).unwrap();
        assert_eq!(
            store.load_memory_revision("alice", 1).unwrap(),
            Some(original.clone())
        );
        assert_eq!(store.load_memory_revision("alice", 2).unwrap(), None);
        let first_sha: String = store
            .connection
            .query_row(
                "SELECT record_sha256 FROM memory_revision WHERE actor_id='alice' AND revision=1",
                [],
                |row| row.get(0),
            )
            .unwrap();

        let mut next = original.clone();
        next.revision = 2;
        next.facts[0].valid = false;
        next.rules[0].valid = false;
        next.conditions[0].valid = false;
        next.facts.push(fact("fact:replacement", "mineral"));
        store.save_memory("alice", &next).unwrap();
        assert_eq!(store.load_memory("alice").unwrap(), next);

        let path = directory.0.clone();
        drop(store);
        let reopened = Store::open(&path).unwrap();
        assert_eq!(reopened.load_memory("alice").unwrap(), next);
        assert_eq!(
            reopened.load_memory_revision("alice", 1).unwrap(),
            Some(original.clone())
        );
        assert_eq!(
            reopened.load_memory_revision("alice", 2).unwrap(),
            Some(next.clone())
        );
        assert_eq!(
            reopened.read_blob_bytes(&first_sha).unwrap(),
            original_bytes
        );

        let mut removed = next.clone();
        removed.revision = 3;
        removed.facts.remove(0);
        assert!(matches!(
            reopened.save_memory("alice", &removed),
            Err(minidora_domain::Error::Contract { code, .. })
                if code == "MEMORY_HISTORY_IMMUTABLE"
        ));

        let mut rewritten = next.clone();
        rewritten.revision = 3;
        rewritten.rules[0].conclusion.object = "changed".into();
        assert!(matches!(
            reopened.save_memory("alice", &rewritten),
            Err(minidora_domain::Error::Contract { code, .. })
                if code == "MEMORY_HISTORY_IMMUTABLE"
        ));

        let mut reactivated = next.clone();
        reactivated.revision = 3;
        reactivated.conditions[0].valid = true;
        assert!(matches!(
            reopened.save_memory("alice", &reactivated),
            Err(minidora_domain::Error::Contract { code, .. })
                if code == "MEMORY_HISTORY_IMMUTABLE"
        ));

        assert_eq!(reopened.load_memory("alice").unwrap(), next);
        assert!(reopened.orphan_blobs().unwrap().is_empty());
        let revisions: i64 = reopened
            .connection
            .query_row(
                "SELECT COUNT(*) FROM memory_revision WHERE actor_id='alice'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(revisions, 2);
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
