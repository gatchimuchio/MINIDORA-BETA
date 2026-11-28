//! Versioned cross-case memory settings and case-local memory projections.
use super::{Store, integer, sql, valid_id};
use minidora_domain::{Intent, MemoryState, RequestEnvelope, Result, failure, identity};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, VecDeque};

pub(super) const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS actor_memory_policy_event(
 actor_id TEXT NOT NULL,
 revision INTEGER NOT NULL CHECK(revision>0),
 enabled INTEGER NOT NULL CHECK(enabled IN (0,1)),
 case_id TEXT NOT NULL,
 input_id TEXT NOT NULL,
 input_version INTEGER NOT NULL CHECK(input_version>=0),
 request_fingerprint TEXT NOT NULL CHECK(length(request_fingerprint)=64),
 PRIMARY KEY(actor_id,revision),
 UNIQUE(actor_id,request_fingerprint),
 UNIQUE(case_id,input_id,input_version),
 FOREIGN KEY(case_id,input_id,input_version) REFERENCES input_identity(case_id,input_id,input_version));
CREATE TABLE IF NOT EXISTS input_memory_policy(
 actor_id TEXT NOT NULL,
 case_id TEXT NOT NULL,
 input_id TEXT NOT NULL,
 input_version INTEGER NOT NULL CHECK(input_version>=0),
 request_fingerprint TEXT NOT NULL CHECK(length(request_fingerprint)=64),
 revision INTEGER NOT NULL CHECK(revision>=0),
 enabled INTEGER NOT NULL CHECK(enabled IN (0,1)),
 CHECK(revision>0 OR enabled=1),
 PRIMARY KEY(case_id,input_id,input_version),
 UNIQUE(actor_id,case_id,input_id,input_version),
 FOREIGN KEY(case_id,input_id,input_version) REFERENCES input_identity(case_id,input_id,input_version));
CREATE TABLE IF NOT EXISTS memory_consumed(
 actor_id TEXT NOT NULL,
 case_id TEXT NOT NULL,
 input_id TEXT NOT NULL,
 input_version INTEGER NOT NULL CHECK(input_version>=0),
 request_fingerprint TEXT NOT NULL CHECK(length(request_fingerprint)=64),
 PRIMARY KEY(actor_id,case_id,input_id,input_version),
 UNIQUE(case_id,input_id,input_version),
 FOREIGN KEY(actor_id,case_id,input_id,input_version)
  REFERENCES input_memory_policy(actor_id,case_id,input_id,input_version));
CREATE TRIGGER IF NOT EXISTS actor_memory_policy_event_no_update
 BEFORE UPDATE ON actor_memory_policy_event BEGIN SELECT RAISE(ABORT,'immutable actor memory policy event'); END;
CREATE TRIGGER IF NOT EXISTS actor_memory_policy_event_no_delete
 BEFORE DELETE ON actor_memory_policy_event BEGIN SELECT RAISE(ABORT,'immutable actor memory policy event'); END;
CREATE TRIGGER IF NOT EXISTS input_memory_policy_no_update
 BEFORE UPDATE ON input_memory_policy BEGIN SELECT RAISE(ABORT,'immutable input memory policy'); END;
CREATE TRIGGER IF NOT EXISTS input_memory_policy_no_delete
 BEFORE DELETE ON input_memory_policy BEGIN SELECT RAISE(ABORT,'immutable input memory policy'); END;
CREATE TRIGGER IF NOT EXISTS memory_consumed_no_update
 BEFORE UPDATE ON memory_consumed BEGIN SELECT RAISE(ABORT,'immutable memory consumption'); END;
CREATE TRIGGER IF NOT EXISTS memory_consumed_no_delete
 BEFORE DELETE ON memory_consumed BEGIN SELECT RAISE(ABORT,'immutable memory consumption'); END;
"#;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct MemoryPolicy {
    pub(crate) revision: u64,
    pub(crate) enabled: bool,
}

impl Default for MemoryPolicy {
    fn default() -> Self {
        Self {
            revision: 0,
            enabled: true,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct MemoryUse {
    pub(crate) actor: String,
    pub(crate) case: String,
    pub(crate) policy: MemoryPolicy,
    pub(crate) accessed: bool,
    pub(crate) input_fingerprint: Option<String>,
}

fn invalid_evidence() -> minidora_domain::Error {
    failure(
        "MEMORY_POLICY_INTEGRITY",
        "記憶設定とその入力・利用記録を照合できません。",
    )
}

fn decode_policy(revision: i64, enabled: i64) -> Result<MemoryPolicy> {
    let revision = u64::try_from(revision).map_err(|_| invalid_evidence())?;
    if !matches!(enabled, 0 | 1) || (revision == 0 && enabled != 1) {
        return Err(invalid_evidence());
    }
    Ok(MemoryPolicy {
        revision,
        enabled: enabled == 1,
    })
}

fn verify_policy(connection: &Connection, actor: &str, policy: MemoryPolicy) -> Result<()> {
    if policy.revision == 0 {
        return if policy == MemoryPolicy::default() {
            Ok(())
        } else {
            Err(invalid_evidence())
        };
    }
    let recorded: Option<i64> = sql(connection
        .query_row(
            "SELECT e.enabled FROM actor_memory_policy_event e
             JOIN case_registry c ON c.case_id=e.case_id AND c.actor_id=e.actor_id
             JOIN input_identity i ON i.case_id=e.case_id AND i.input_id=e.input_id
                 AND i.input_version=e.input_version AND i.fingerprint=e.request_fingerprint
                 AND i.bundle_sha256 IS NOT NULL
             WHERE e.actor_id=?1 AND e.revision=?2",
            params![actor, integer(policy.revision)?],
            |row| row.get(0),
        )
        .optional())?;
    if recorded != Some(i64::from(policy.enabled)) {
        return Err(invalid_evidence());
    }
    Ok(())
}

pub(super) fn memory_policy_on(connection: &Connection, actor: &str) -> Result<MemoryPolicy> {
    valid_id(actor)?;
    let head: Option<(i64, i64)> = sql(connection
        .query_row(
            "SELECT revision,enabled FROM actor_memory_policy_event
             WHERE actor_id=?1 ORDER BY revision DESC LIMIT 1",
            [actor],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional())?;
    let Some((revision, enabled)) = head else {
        return Ok(MemoryPolicy::default());
    };
    let policy = decode_policy(revision, enabled)?;
    verify_policy(connection, actor, policy)?;
    Ok(policy)
}

fn bound_policy_on(
    connection: &Connection,
    actor: &str,
    case: &str,
    input: &str,
    version: u64,
) -> Result<Option<MemoryPolicy>> {
    let binding: Option<(String, String, i64, i64, String)> = sql(connection
        .query_row(
            "SELECT p.actor_id,p.request_fingerprint,p.revision,p.enabled,i.fingerprint
             FROM input_memory_policy p
             JOIN input_identity i
              ON i.case_id=p.case_id AND i.input_id=p.input_id AND i.input_version=p.input_version
             WHERE p.case_id=?1 AND p.input_id=?2 AND p.input_version=?3",
            params![case, input, integer(version)?],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                ))
            },
        )
        .optional())?;
    let Some((owner, proof, revision, enabled, registered)) = binding else {
        return Ok(None);
    };
    if owner != actor || proof != registered {
        return Err(invalid_evidence());
    }
    let policy = decode_policy(revision, enabled)?;
    verify_policy(connection, actor, policy)?;
    Ok(Some(policy))
}

impl Store {
    pub(crate) fn memory_policy(&self, actor: &str) -> Result<MemoryPolicy> {
        memory_policy_on(&self.connection, actor)
    }

    fn policy_request_bundle(
        &self,
        connection: &Connection,
        request: &RequestEnvelope,
    ) -> Result<minidora_domain::InputBundle> {
        Self::require_case(connection, &request.actor_id, &request.case_id)?;
        let (_, bundle) = self
            .read_input_bundle(
                connection,
                &request.actor_id,
                &request.case_id,
                &request.input_id,
                request.input_version,
            )?
            .ok_or_else(invalid_evidence)?;
        if &bundle.request != request {
            return Err(invalid_evidence());
        }
        Ok(bundle)
    }

    /// Must run inside the case's final COMMIT transaction.
    pub(super) fn commit_memory_policy(
        &self,
        connection: &Connection,
        request: &RequestEnvelope,
        expected: MemoryPolicy,
    ) -> Result<()> {
        let bundle = self.policy_request_bundle(connection, request)?;
        if !matches!(bundle.meaning.intent, Intent::SetMemorySharing { enabled: requested } if requested == expected.enabled)
        {
            return Err(failure(
                "MEMORY_POLICY_UNAUTHORIZED",
                "封印された依頼からこの記憶設定の変更を確認できません。",
            ));
        }
        let changed = || {
            failure(
                "MEMORY_POLICY_CHANGED",
                "記憶設定の確定前に別の依頼が設定を変更しました。",
            )
        };
        let fingerprint = identity(request)?;
        let saved: Option<(String, String, i64, i64, i64)> = sql(connection
            .query_row(
                "SELECT case_id,input_id,input_version,revision,enabled
                 FROM actor_memory_policy_event WHERE actor_id=?1 AND request_fingerprint=?2",
                params![request.actor_id, fingerprint],
                |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                    ))
                },
            )
            .optional())?;
        if let Some((case, input, version, revision, value)) = saved {
            if case != request.case_id
                || input != request.input_id
                || version != integer(request.input_version)?
            {
                return Err(invalid_evidence());
            }
            if decode_policy(revision, value)? != expected {
                return Err(changed());
            }
            return Ok(());
        }
        let bound = bound_policy_on(
            connection,
            &request.actor_id,
            &request.case_id,
            &request.input_id,
            request.input_version,
        )?
        .ok_or_else(invalid_evidence)?;
        let head = memory_policy_on(connection, &request.actor_id)?;
        if bound != head || head.revision.checked_add(1) != Some(expected.revision) {
            return Err(changed());
        }
        sql(connection.execute(
            "INSERT INTO actor_memory_policy_event
             (actor_id,revision,enabled,case_id,input_id,input_version,request_fingerprint)
             VALUES (?1,?2,?3,?4,?5,?6,?7)",
            params![
                request.actor_id,
                integer(expected.revision)?,
                expected.enabled,
                request.case_id,
                request.input_id,
                integer(request.input_version)?,
                fingerprint,
            ],
        ))?;
        Ok(())
    }

    pub(crate) fn bind_memory_policy(
        &self,
        request: &RequestEnvelope,
        policy: MemoryPolicy,
    ) -> Result<()> {
        let transaction = self.transaction()?;
        self.policy_request_bundle(&transaction, request)?;
        let saved = bound_policy_on(
            &transaction,
            &request.actor_id,
            &request.case_id,
            &request.input_id,
            request.input_version,
        )?;
        if let Some(saved) = saved {
            if saved != policy {
                return Err(invalid_evidence());
            }
        } else {
            if memory_policy_on(&transaction, &request.actor_id)? != policy {
                return Err(failure(
                    "MEMORY_POLICY_CHANGED",
                    "入力の受付中に記憶設定が変更されました。",
                ));
            }
            sql(transaction.execute(
                "INSERT INTO input_memory_policy
                 (actor_id,case_id,input_id,input_version,request_fingerprint,revision,enabled)
                 VALUES (?1,?2,?3,?4,?5,?6,?7)",
                params![
                    request.actor_id,
                    request.case_id,
                    request.input_id,
                    integer(request.input_version)?,
                    identity(request)?,
                    integer(policy.revision)?,
                    policy.enabled,
                ],
            ))?;
        }
        sql(transaction.commit())
    }

    pub(crate) fn policy_for_input(
        &self,
        actor: &str,
        case: &str,
        input: &str,
        version: u64,
    ) -> Result<MemoryPolicy> {
        Self::require_case(&self.connection, actor, case)?;
        Ok(bound_policy_on(&self.connection, actor, case, input, version)?.unwrap_or_default())
    }

    pub(crate) fn record_memory_consumption(&self, request: &RequestEnvelope) -> Result<()> {
        let transaction = self.transaction()?;
        self.policy_request_bundle(&transaction, request)?;
        if bound_policy_on(
            &transaction,
            &request.actor_id,
            &request.case_id,
            &request.input_id,
            request.input_version,
        )?
        .is_none()
        {
            return Err(invalid_evidence());
        }
        sql(transaction.execute(
            "INSERT INTO memory_consumed
             (actor_id,case_id,input_id,input_version,request_fingerprint)
             VALUES (?1,?2,?3,?4,?5)
             ON CONFLICT(case_id,input_id,input_version) DO NOTHING",
            params![
                request.actor_id,
                request.case_id,
                request.input_id,
                integer(request.input_version)?,
                identity(request)?,
            ],
        ))?;
        if !consumed_on(
            &transaction,
            &request.actor_id,
            &request.case_id,
            &request.input_id,
            request.input_version,
        )? {
            return Err(invalid_evidence());
        }
        sql(transaction.commit())
    }

    pub(crate) fn input_consumed_memory(
        &self,
        actor: &str,
        case: &str,
        input: &str,
        version: u64,
    ) -> Result<bool> {
        Self::require_case(&self.connection, actor, case)?;
        consumed_on(&self.connection, actor, case, input, version)
    }

    /// Returns a transient subset of an already authenticated memory snapshot.
    /// The resulting projection must never replace the actor's stored memory.
    pub(crate) fn project_memory(
        &self,
        actor: &str,
        case: &str,
        memory: &MemoryState,
    ) -> Result<MemoryState> {
        self.project_memory_filtered(actor, case, memory, false)
    }

    /// Historical source inspection keeps invalidated records unchanged.
    pub(crate) fn project_memory_with_inactive(
        &self,
        actor: &str,
        case: &str,
        memory: &MemoryState,
    ) -> Result<MemoryState> {
        self.project_memory_filtered(actor, case, memory, true)
    }

    fn project_memory_filtered(
        &self,
        actor: &str,
        case: &str,
        memory: &MemoryState,
        include_inactive: bool,
    ) -> Result<MemoryState> {
        if !Self::actor_for_case(&self.connection, actor, case)? {
            return Ok(MemoryState {
                revision: memory.revision,
                ..MemoryState::default()
            });
        }
        let relevant_sources: BTreeSet<_> = memory
            .facts
            .iter()
            .filter(|fact| include_inactive || fact.valid)
            .map(|fact| fact.source_input.as_str())
            .chain(
                memory
                    .rules
                    .iter()
                    .filter(|rule| include_inactive || rule.valid)
                    .map(|rule| rule.source_input.as_str()),
            )
            .chain(
                memory
                    .conditions
                    .iter()
                    .filter(|condition| include_inactive || condition.valid)
                    .map(|condition| condition.source_input.as_str()),
            )
            .collect();
        let mut statement = sql(self.connection.prepare(
            "SELECT i.fingerprint,i.input_id,i.input_version
             FROM input_identity i JOIN case_registry c ON c.case_id=i.case_id
             WHERE c.actor_id=?1 AND i.case_id=?2 AND i.bundle_sha256 IS NOT NULL",
        ))?;
        let rows = sql(statement.query_map(params![actor, case], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)?,
            ))
        }))?;
        let mut inputs = BTreeSet::new();
        for row in rows {
            let (fingerprint, input, version) = sql(row)?;
            if !relevant_sources.contains(fingerprint.as_str()) {
                continue;
            }
            let version = u64::try_from(version).map_err(|_| invalid_evidence())?;
            let (_, bundle) = self
                .read_input_bundle(&self.connection, actor, case, &input, version)?
                .ok_or_else(invalid_evidence)?;
            let contextual = matches!(
                bundle.meaning.intent,
                Intent::AssertPreviousRelation { .. } | Intent::CorrectPreviousPremise { .. }
            );
            // Older contextual writes can have empty dependency lists even
            // when their antecedents came from another case. Only an original
            // OFF binding proves that Core already applied this same boundary.
            if !contextual
                || bound_policy_on(&self.connection, actor, case, &input, version)?
                    .is_some_and(|policy| !policy.enabled)
            {
                inputs.insert(fingerprint);
            }
        }
        let candidates = memory
            .facts
            .iter()
            .filter(|fact| (include_inactive || fact.valid) && inputs.contains(&fact.source_input))
            .map(|fact| (fact.id.as_str(), fact.dependencies.as_slice()))
            .chain(
                memory
                    .rules
                    .iter()
                    .filter(|rule| {
                        (include_inactive || rule.valid)
                            && (inputs.contains(&rule.source_input)
                                || (rule.source_input == "derived:proof-composition-v1"
                                    && !rule.dependencies.is_empty()))
                    })
                    .map(|rule| (rule.id.as_str(), rule.dependencies.as_slice())),
            )
            .chain(
                memory
                    .conditions
                    .iter()
                    .filter(|condition| {
                        (include_inactive || condition.valid)
                            && inputs.contains(&condition.source_input)
                    })
                    .map(|condition| (condition.id.as_str(), condition.dependencies.as_slice())),
            );
        let retained = local_dependency_closure(candidates)?;
        Ok(MemoryState {
            revision: memory.revision,
            facts: memory
                .facts
                .iter()
                .filter(|fact| retained.contains(fact.id.as_str()))
                .cloned()
                .collect(),
            rules: memory
                .rules
                .iter()
                .filter(|rule| retained.contains(rule.id.as_str()))
                .cloned()
                .collect(),
            conditions: memory
                .conditions
                .iter()
                .filter(|condition| retained.contains(condition.id.as_str()))
                .cloned()
                .collect(),
        })
    }
}

fn consumed_on(
    connection: &Connection,
    actor: &str,
    case: &str,
    input: &str,
    version: u64,
) -> Result<bool> {
    let record: Option<(String, String, String, String)> = sql(connection
        .query_row(
            "SELECT m.actor_id,m.request_fingerprint,p.actor_id,p.request_fingerprint
             FROM memory_consumed m JOIN input_memory_policy p ON p.case_id=m.case_id
              AND p.input_id=m.input_id AND p.input_version=m.input_version
             WHERE m.case_id=?1 AND m.input_id=?2 AND m.input_version=?3",
            params![case, input, integer(version)?],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .optional())?;
    let Some((owner, fingerprint, binding_owner, bound_fingerprint)) = record else {
        return Ok(false);
    };
    if owner != actor || binding_owner != actor || fingerprint != bound_fingerprint {
        return Err(invalid_evidence());
    }
    Ok(true)
}

/// Missing and cyclic prerequisites never become ready.
fn local_dependency_closure<'a>(
    candidates: impl Iterator<Item = (&'a str, &'a [String])>,
) -> Result<BTreeSet<&'a str>> {
    let mut remaining = BTreeMap::<&str, usize>::new();
    let mut downstream = BTreeMap::<&str, Vec<&str>>::new();
    let mut ready = VecDeque::new();
    for (id, dependencies) in candidates {
        if remaining.insert(id, dependencies.len()).is_some() {
            return Err(invalid_evidence());
        }
        if dependencies.is_empty() {
            ready.push_back(id);
        }
        for dependency in dependencies {
            downstream.entry(dependency).or_default().push(id);
        }
    }
    let mut retained = BTreeSet::new();
    while let Some(id) = ready.pop_front() {
        retained.insert(id);
        for other in downstream.get(id).into_iter().flatten() {
            let available = remaining
                .get_mut(other)
                .expect("recorded dependent has a readiness count");
            *available -= 1;
            if *available == 0 {
                ready.push_back(other);
            }
        }
    }
    Ok(retained)
}
