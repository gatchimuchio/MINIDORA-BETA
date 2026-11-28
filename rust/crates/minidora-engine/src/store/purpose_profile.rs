//! Durable, actor-scoped snapshots of reusable purpose profiles.
use super::{Store, sql, valid_id};
use minidora_domain::{PurposeProfile, Result, canonical, digest, failure};
use rusqlite::{Connection, OptionalExtension, params};

const RECORD_VERSION: &str = "minidora.store.purpose-profile.v1";
const MAX_PROFILES_PER_ACTOR: u32 = 32;
const MAX_RECORD_BYTES: usize = 512 * 1024;

pub(super) const SCHEMA: &str = r#"
BEGIN;
CREATE TABLE IF NOT EXISTS purpose_profile(
 actor_id TEXT NOT NULL CHECK(length(actor_id)>0),
 name TEXT NOT NULL CHECK(length(name)>0),
 sha256 TEXT NOT NULL CHECK(length(sha256)=64 AND sha256 NOT GLOB '*[^0-9a-f]*'),
 canonical BLOB NOT NULL CHECK(typeof(canonical)='blob' AND length(canonical) BETWEEN 1 AND 524288),
 PRIMARY KEY(actor_id,name));
CREATE TRIGGER IF NOT EXISTS purpose_profile_no_replace
 BEFORE INSERT ON purpose_profile
 WHEN EXISTS(SELECT 1 FROM purpose_profile WHERE actor_id=NEW.actor_id AND name=NEW.name)
 BEGIN SELECT RAISE(ABORT,'immutable purpose profile'); END;
CREATE TRIGGER IF NOT EXISTS purpose_profile_no_update
 BEFORE UPDATE ON purpose_profile BEGIN SELECT RAISE(ABORT,'immutable purpose profile'); END;
CREATE TRIGGER IF NOT EXISTS purpose_profile_no_delete
 BEFORE DELETE ON purpose_profile BEGIN SELECT RAISE(ABORT,'immutable purpose profile'); END;
COMMIT;
"#;

fn invariant_error() -> minidora_domain::Error {
    failure(
        "PURPOSE_PROFILE_INTEGRITY",
        "用途設定の保存内容、利用者、名前、またはSHA-256が一致しません。",
    )
}

fn load(
    connection: &Connection,
    actor: &str,
    name: &str,
) -> Result<Option<(String, PurposeProfile)>> {
    let stored: Option<(String, Vec<u8>)> = sql(connection
        .query_row(
            "SELECT sha256,canonical FROM purpose_profile WHERE actor_id=?1 AND name=?2",
            params![actor, name],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional())?;
    let Some((sha256, bytes)) = stored else {
        return Ok(None);
    };
    if bytes.len() > MAX_RECORD_BYTES || digest(&bytes) != sha256 {
        return Err(invariant_error());
    }
    let (version, owner, profile): (String, String, PurposeProfile) =
        serde_json::from_slice(&bytes).map_err(|_| invariant_error())?;
    if version != RECORD_VERSION
        || owner != actor
        || profile.name != name
        || canonical(&(RECORD_VERSION, actor, &profile))? != bytes
    {
        return Err(invariant_error());
    }
    crate::purpose_profile::validate_definition(&profile).map_err(|_| invariant_error())?;
    Ok(Some((sha256, profile)))
}

impl Store {
    pub(crate) fn save_purpose_profile(
        &self,
        actor: &str,
        profile: &PurposeProfile,
    ) -> Result<String> {
        valid_id(actor)?;
        crate::purpose_profile::validate_definition(profile)?;
        let bytes = canonical(&(RECORD_VERSION, actor, profile))?;
        if bytes.len() > MAX_RECORD_BYTES {
            return Err(failure(
                "PURPOSE_PROFILE_LIMIT",
                "用途設定と参照資料の保存サイズが上限を超えました。",
            ));
        }
        let transaction = self.transaction()?;
        if let Some((sha256, existing)) = load(&transaction, actor, &profile.name)? {
            if existing != *profile {
                return Err(failure(
                    "PURPOSE_PROFILE_CONFLICT",
                    "同じ名前の用途設定がすでにあります。異なる内容で保存するには別の名前を指定してください。",
                ));
            }
            sql(transaction.commit())?;
            return Ok(sha256);
        }
        let count: u32 = sql(transaction.query_row(
            "SELECT COUNT(*) FROM purpose_profile WHERE actor_id=?1",
            [actor],
            |row| row.get(0),
        ))?;
        if count >= MAX_PROFILES_PER_ACTOR {
            return Err(failure(
                "PURPOSE_PROFILE_LIMIT",
                "この利用者が保存できる用途設定の件数が上限に達しました。",
            ));
        }
        let sha256 = digest(&bytes);
        sql(transaction.execute(
            "INSERT INTO purpose_profile(actor_id,name,sha256,canonical) VALUES(?1,?2,?3,?4)",
            params![actor, profile.name, sha256, bytes],
        ))?;
        sql(transaction.commit())?;
        Ok(sha256)
    }

    pub(crate) fn load_purpose_profile(
        &self,
        actor: &str,
        name: &str,
    ) -> Result<Option<(String, PurposeProfile)>> {
        valid_id(actor)?;
        valid_id(name)?;
        load(&self.connection, actor, name)
    }
}
