//! Immutable branch anchors and authenticated views of their source history.
use super::{Store, integer, sql, valid_digest, valid_id};
use minidora_domain::{
    BranchHistoryEntry, CaseBranch, CaseRevision, Lifecycle, RequestEnvelope, Result, ResultRecord,
    failure, identity,
};
use rusqlite::{Connection, OptionalExtension, Transaction, TransactionBehavior, params};
use std::collections::BTreeSet;

const ANCHOR_VERSION: &str = "minidora.store.case-branch.v1";
const MAX_HISTORY_PAGE: usize = 20;

pub(super) const SCHEMA: &str = r#"
BEGIN;
CREATE TABLE IF NOT EXISTS case_branch(
 case_id TEXT PRIMARY KEY NOT NULL REFERENCES case_registry(case_id),
 actor_id TEXT NOT NULL,
 source_case_id TEXT NOT NULL,
 source_result_id TEXT NOT NULL CHECK(length(source_result_id)=64),
 source_revision INTEGER NOT NULL CHECK(source_revision>=0),
 source_state_sha256 TEXT NOT NULL REFERENCES artifact_ref(sha256),
 anchor_sha256 TEXT NOT NULL CHECK(length(anchor_sha256)=64),
 CHECK(case_id<>source_case_id),
 FOREIGN KEY(source_case_id,source_revision) REFERENCES case_revision(case_id,revision));
CREATE TRIGGER IF NOT EXISTS case_branch_validate_insert BEFORE INSERT ON case_branch
 WHEN EXISTS(SELECT 1 FROM case_branch WHERE case_id=NEW.case_id)
 OR (SELECT actor_id FROM case_registry WHERE case_id=NEW.case_id) IS NOT NEW.actor_id
 OR (SELECT actor_id FROM case_registry WHERE case_id=NEW.source_case_id) IS NOT NEW.actor_id
 OR NOT EXISTS(SELECT 1 FROM case_revision
   WHERE case_id=NEW.source_case_id AND revision=NEW.source_revision
     AND state_sha256=NEW.source_state_sha256 AND lifecycle='COMMIT')
 BEGIN SELECT RAISE(ABORT,'invalid or immutable case branch'); END;
CREATE TRIGGER IF NOT EXISTS case_branch_no_update
 BEFORE UPDATE ON case_branch BEGIN SELECT RAISE(ABORT,'immutable case branch'); END;
CREATE TRIGGER IF NOT EXISTS case_branch_no_delete
 BEFORE DELETE ON case_branch BEGIN SELECT RAISE(ABORT,'immutable case branch'); END;
COMMIT;
"#;

fn integrity_error() -> minidora_domain::Error {
    failure(
        "CASE_BRANCH_INTEGRITY",
        "分岐の利用者、元の会話、または分岐時の確定結果を照合できません。",
    )
}

fn unavailable_source() -> minidora_domain::Error {
    failure(
        "CASE_BRANCH_SOURCE_UNAVAILABLE",
        "元の会話で分岐できる確定結果を確認できません。確定済みの結果IDを指定してください。",
    )
}

fn committed_content(state: &CaseRevision) -> Result<(&RequestEnvelope, &ResultRecord)> {
    let request = &state.bundle.as_ref().ok_or_else(integrity_error)?.request;
    let result = state.result.as_ref().ok_or_else(integrity_error)?;
    if state.lifecycle != Lifecycle::Commit
        || result.lifecycle != Lifecycle::Commit
        || request.actor_id != state.actor_id
        || request.case_id != state.case_id
        || request.input_id != result.input_id
        || request.input_version != result.input_version
        || state.input_fingerprint.as_deref() != Some(result.id.as_str())
        || identity(request)? != result.id
    {
        return Err(integrity_error());
    }
    Ok((request, result))
}

fn load_branch(connection: &Connection, actor: &str, case: &str) -> Result<Option<CaseBranch>> {
    if !Store::actor_for_case(connection, actor, case)? {
        return Ok(None);
    }
    let raw = sql(connection
        .query_row(
            "SELECT actor_id,source_case_id,source_result_id,source_revision,source_state_sha256,anchor_sha256
             FROM case_branch WHERE case_id=?1",
            [case],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, i64>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, String>(5)?,
                ))
            },
        )
        .optional())?;
    let Some((owner, source_case_id, source_result_id, revision, source_state_sha256, hash)) = raw
    else {
        return Ok(None);
    };
    let branch = CaseBranch {
        case_id: case.into(),
        source_case_id,
        source_result_id,
        source_revision: revision.try_into().map_err(|_| integrity_error())?,
        source_state_sha256,
    };
    if owner != actor
        || branch.source_case_id == case
        || !valid_digest(&branch.source_state_sha256)
        || !valid_digest(&branch.source_result_id)
        || identity(&(ANCHOR_VERSION, actor, &branch))? != hash
    {
        return Err(integrity_error());
    }
    Store::require_case(connection, actor, &branch.source_case_id)?;
    Ok(Some(branch))
}

impl Store {
    pub(crate) fn case_registered(&self, actor: &str, case: &str) -> Result<bool> {
        Self::actor_for_case(&self.connection, actor, case)
    }

    pub(crate) fn create_case_branch(
        &self,
        actor: &str,
        source_case: &str,
        case: &str,
        result_id: Option<&str>,
    ) -> Result<CaseBranch> {
        valid_id(case)?;
        if source_case == case {
            return Err(failure(
                "CASE_BRANCH_DESTINATION_INVALID",
                "元の会話と異なる会話IDを指定してください。",
            ));
        }
        let transaction = self.transaction()?;
        Self::require_case(&transaction, actor, source_case)?;
        if Self::actor_for_case(&transaction, actor, case)? {
            return Err(failure(
                "CASE_BRANCH_DESTINATION_USED",
                "分岐先の会話IDはすでに使用されています。",
            ));
        }
        let mut selected = None;
        let mut commit_mismatch = false;
        let head = self
            .read_verified_case(source_case, |revision, sha, committed| {
                commit_mismatch |= committed != (revision.lifecycle == Lifecycle::Commit);
                if committed
                    && revision
                        .result
                        .as_ref()
                        .is_some_and(|result| result_id.is_none_or(|id| id == result.id))
                {
                    selected = Some((
                        committed_content(revision).map(|(_, result)| result.id.clone()),
                        revision.revision,
                        sha.to_owned(),
                    ));
                }
            })?
            .ok_or_else(unavailable_source)?;
        if head.lifecycle == Lifecycle::Cancel {
            return Err(failure(
                "CASE_BRANCH_SOURCE_CANCELLED",
                "元の会話は取り消しで終了しています。",
            ));
        }
        if commit_mismatch {
            return Err(integrity_error());
        }
        let (result, source_revision, source_state_sha256) =
            selected.ok_or_else(unavailable_source)?;
        if result_id.is_none() && head.revision != source_revision {
            return Err(unavailable_source());
        }
        let branch = CaseBranch {
            case_id: case.into(),
            source_case_id: source_case.into(),
            source_result_id: result?,
            source_revision,
            source_state_sha256,
        };
        let hash = identity(&(ANCHOR_VERSION, actor, &branch))?;
        sql(transaction.execute(
            "INSERT INTO case_registry(case_id,actor_id) VALUES(?1,?2)",
            params![case, actor],
        ))?;
        sql(transaction.execute(
            "INSERT INTO case_branch(case_id,actor_id,source_case_id,source_result_id,source_revision,source_state_sha256,anchor_sha256)
             VALUES(?1,?2,?3,?4,?5,?6,?7)",
            params![
                case,
                actor,
                branch.source_case_id,
                branch.source_result_id,
                integer(branch.source_revision)?,
                branch.source_state_sha256,
                hash,
            ],
        ))?;
        sql(transaction.commit())?;
        Ok(branch)
    }

    fn verify_branch_source(
        &self,
        branch: &CaseBranch,
        mut visit_committed: impl FnMut(&CaseRevision),
    ) -> Result<CaseRevision> {
        let mut anchor_matches = false;
        let mut commit_mismatch = false;
        let state = self
            .read_verified_case_through(
                &branch.source_case_id,
                Some(branch.source_revision),
                |revision, sha, committed| {
                    commit_mismatch |= committed != (revision.lifecycle == Lifecycle::Commit);
                    if revision.revision == branch.source_revision {
                        anchor_matches = committed && sha == branch.source_state_sha256;
                    }
                    if committed {
                        visit_committed(revision);
                    }
                },
            )?
            .ok_or_else(integrity_error)?;
        if commit_mismatch
            || !anchor_matches
            || committed_content(&state)?.1.id != branch.source_result_id
        {
            return Err(integrity_error());
        }
        Ok(state)
    }

    pub(crate) fn branch_source(
        &self,
        actor: &str,
        case: &str,
    ) -> Result<Option<(CaseBranch, CaseRevision)>> {
        let transaction = sql(Transaction::new_unchecked(
            &self.connection,
            TransactionBehavior::Deferred,
        ))?;
        let source = load_branch(&transaction, actor, case)?
            .map(|branch| {
                self.verify_branch_source(&branch, |_| {})
                    .map(|state| (branch, state))
            })
            .transpose()?;
        sql(transaction.commit())?;
        Ok(source)
    }

    pub(crate) fn branch_history(
        &self,
        actor: &str,
        case: &str,
        offset: usize,
        limit: usize,
    ) -> Result<Vec<BranchHistoryEntry>> {
        if limit > MAX_HISTORY_PAGE {
            return Err(failure(
                "CASE_BRANCH_HISTORY_LIMIT",
                "引き継いだ履歴は一度に20件まで表示できます。",
            ));
        }
        let transaction = sql(Transaction::new_unchecked(
            &self.connection,
            TransactionBehavior::Deferred,
        ))?;
        let Some(branch) = load_branch(&transaction, actor, case)? else {
            sql(transaction.commit())?;
            return Ok(Vec::new());
        };
        let requested = {
            let mut statement = sql(transaction.prepare(
                "SELECT revision FROM case_revision
                 WHERE case_id=?1 AND revision<=?2 AND lifecycle='COMMIT'
                 ORDER BY revision DESC LIMIT ?3 OFFSET ?4",
            ))?;
            let rows = sql(statement.query_map(
                params![
                    branch.source_case_id,
                    integer(branch.source_revision)?,
                    limit as i64,
                    i64::try_from(offset).unwrap_or(i64::MAX),
                ],
                |row| row.get::<_, i64>(0),
            ))?;
            sql(rows.collect::<rusqlite::Result<Vec<_>>>())?
                .into_iter()
                .map(|revision| u64::try_from(revision).map_err(|_| integrity_error()))
                .collect::<Result<BTreeSet<_>>>()?
        };
        let mut entries = Vec::with_capacity(requested.len());
        self.verify_branch_source(&branch, |revision| {
            if requested.contains(&revision.revision) {
                entries.push(committed_content(revision).map(|(request, result)| {
                    BranchHistoryEntry {
                        source_case_id: branch.source_case_id.clone(),
                        result_id: result.id.clone(),
                        input_text: request.text.clone(),
                        text: result.text.clone(),
                    }
                }));
            }
        })?;
        if entries.len() != requested.len() {
            return Err(integrity_error());
        }
        let history = entries.into_iter().rev().collect::<Result<_>>()?;
        sql(transaction.commit())?;
        Ok(history)
    }
}
