//! Core-owned reconciliation of a previously attempted local file effect.
//!
//! This module never writes target content, invokes a provider, creates an
//! action/token or reinterprets an input. It verifies the stored original goal
//! and artifacts, observes the existing exact target, then submits the resulting
//! receipt/revisions through the same Core-owned persistence boundary. Host
//! authentication and trusted ownership of ancestor directories remain required.
use crate::{
    core::validate_render,
    language,
    scheduler::{GoalNode, GoalNodeKind, Scheduler},
    store::Store,
};
use minidora_domain::*;
use std::{
    collections::BTreeMap,
    fs::{self, File},
    io::Read,
    path::{Component, Path},
};

pub fn reconcile(store: &Store, actor: &str, case: &str) -> Result<ResultRecord> {
    let mut state = store
        .load_case(actor, case)?
        .ok_or_else(|| failure("CASE_NOT_FOUND", "no saved case to reconcile"))?;
    if state.lifecycle == Lifecycle::Commit {
        return state
            .result
            .ok_or_else(|| failure("INTEGRITY_FAILURE", "committed case has no result"));
    }
    if !matches!(state.lifecycle, Lifecycle::Adopted | Lifecycle::Suspend) {
        return Err(failure(
            "INVALID_RECOVERY_STATE",
            "only an adopted/suspended case can reconcile a local file",
        ));
    }
    let mut result = state
        .result
        .clone()
        .ok_or_else(|| failure("RECOVERY_DATA_UNAVAILABLE", "no adopted result is retained"))?;
    let inspected = inspect(store, actor, &state, &result);
    let (mut action, receipt, mut graph, render_ref) = match inspected {
        Ok(value) => value,
        Err(error) => return suspend(store, &mut state, result, &error.to_string()),
    };
    // Persist a recovered UNKNOWN observation before its later confirmation.
    // The exact token, target, authority and request bytes are unchanged.
    if action.phase == EffectPhase::Unknown {
        store.save_action(actor, &action)?;
        action.phase = EffectPhase::Confirmed;
        action.receipt = Some(receipt.clone());
        store.save_action(actor, &action)?;
    }
    result
        .receipts
        .retain(|old| old.action_id != receipt.action_id);
    result.receipts.push(receipt.clone());
    let receipt_ref = store.put_artifact(actor, case, &receipt)?;
    graph.observe(
        "file",
        &receipt_ref,
        &BTreeMap::from([("render".into(), render_ref)]),
    )?;
    if !graph.is_complete() {
        return suspend(
            store,
            &mut state,
            result,
            "UNMET_GOAL_NODES: reconciliation retained unresolved required artifacts",
        );
    }
    result.execution_graph_ref = Some(store.put_artifact(
        actor,
        case,
        &(
            graph.nodes(),
            graph.observed(),
            graph.consumed_by("content"),
            graph.consumed_by("render"),
            graph.consumed_by("file"),
        ),
    )?);
    result.reason = Some(
        "LOCAL_FILE_RECONCILED: exact original artifact observed; no effect resubmitted".into(),
    );
    if state.lifecycle == Lifecycle::Suspend {
        advance(store, &mut state, &result, Lifecycle::Running)?;
        advance(store, &mut state, &result, Lifecycle::Adopted)?;
    }
    advance(store, &mut state, &result, Lifecycle::Commit)?;
    state
        .result
        .ok_or_else(|| failure("INTEGRITY_FAILURE", "reconciliation lost result"))
}

/// Reconcile already-issued local file effects after the goal was cancelled.
/// Only the immutable original action's bytes, token, target and authorization
/// are eligible. This records effect observations, never restarts/completes the
/// cancelled goal or sends anything. Unobservable/unsupported effects remain
/// UNKNOWN; they are never guessed to have failed.
pub fn reconcile_cancelled_effects(
    store: &Store,
    actor: &str,
    case: &str,
) -> Result<Vec<ActionRecord>> {
    let state = store
        .load_case(actor, case)?
        .ok_or_else(|| failure("CASE_NOT_FOUND", "no cancelled case to inspect"))?;
    if state.lifecycle != Lifecycle::Cancel {
        return Err(failure(
            "INVALID_RECOVERY_STATE",
            "cancelled-effect observation requires a CANCEL case",
        ));
    }
    for mut action in store.actions(actor, case)? {
        if action.phase != EffectPhase::Unknown {
            continue;
        }
        // Persist the recovery uncertainty if the final saved phase was
        // ATTEMPTED. This is not permission to repeat that physical operation.
        store.save_action(actor, &action)?;
        if action.effect_class != "LOCAL_WRITE"
            || action.actor_id != actor
            || action.case_id != case
        {
            continue;
        }
        let observation = (|| -> Result<DeliveryReceipt> {
            let exact = store.get_bytes(actor, case, &action.exact_request_sha256)?;
            if action.exact_request_sha256 != digest(&exact) {
                return Err(failure(
                    "RECOVERY_ARTIFACT_MISMATCH",
                    "original effect bytes differ",
                ));
            }
            let path = Path::new(&action.target);
            let authorization: Permissions = serde_json::from_slice(&store.get_bytes(
                actor,
                case,
                &action.authorization_sha256,
            )?)?;
            if !authorization.write_paths.contains(path)
                || action.idempotency_key != action.action_id
                || action.action_id
                    != identity(&(
                        case,
                        &action.input_fingerprint,
                        "file",
                        path,
                        &action.exact_request_sha256,
                    ))?
            {
                return Err(failure(
                    "RECOVERY_AUTHORITY_MISMATCH",
                    "original scope or action identity differs",
                ));
            }
            observe_file(path, &exact)?;
            Ok(DeliveryReceipt {
                action_id: action.action_id.clone(),
                target: action.target.clone(),
                content_sha256: action.exact_request_sha256.clone(),
                phase: EffectPhase::Confirmed,
                observed_bytes: Some(exact.len() as u64),
                reason: Some("CANCELLED_GOAL_LOCAL_EFFECT_OBSERVED_NO_RESEND".into()),
            })
        })();
        if let Ok(receipt) = observation {
            action.phase = EffectPhase::Confirmed;
            action.receipt = Some(receipt);
            store.save_action(actor, &action)?;
        }
    }
    // Case lifecycle/revision is deliberately unchanged. The canceled input's
    // historical result and original goal can never become COMMIT here.
    store.actions(actor, case)
}

type Inspected = (ActionRecord, DeliveryReceipt, Scheduler, String);
fn inspect(
    store: &Store,
    actor: &str,
    state: &CaseRevision,
    result: &ResultRecord,
) -> Result<Inspected> {
    let case = &state.case_id;
    let bundle = state
        .bundle
        .as_ref()
        .ok_or_else(|| failure("RECOVERY_DATA_UNAVAILABLE", "sealed original unavailable"))?;
    let request = &bundle.request;
    let fingerprint = identity(request)?;
    if request.actor_id != actor
        || request.case_id != *case
        || state.input_fingerprint.as_deref() != Some(fingerprint.as_str())
    {
        return Err(failure(
            "RECOVERY_IDENTITY_MISMATCH",
            "sealed actor/case/input differs",
        ));
    }
    let goal_ref = state.goal_sha256.as_ref().ok_or_else(|| {
        failure(
            "RECOVERY_DATA_UNAVAILABLE",
            "bound original goal unavailable",
        )
    })?;
    let goal: GoalContract = serde_json::from_slice(&store.get_bytes(actor, case, goal_ref)?)?;
    if goal.input_fingerprint != fingerprint
        || goal.intent != bundle.meaning.intent
        || goal.permissions != request.permissions
        || goal.output_path != request.output_path
        || goal.budget != request.budget
    {
        return Err(failure(
            "RECOVERY_GOAL_MISMATCH",
            "saved goal differs from sealed original request",
        ));
    }
    let path = goal.output_path.as_ref().ok_or_else(|| {
        failure(
            "NO_LOCAL_FILE_GOAL",
            "original goal does not require a local file",
        )
    })?;
    if !goal.permissions.write_paths.contains(path) {
        return Err(failure(
            "WRITE_PERMISSION_MISSING",
            "original target lacks original matching authority",
        ));
    }
    let expected_predicates = vec![
        "SUPPORTED_CONTENT".to_string(),
        "EXACT_RENDER".into(),
        "CONFIRMED_FILE".into(),
    ];
    if goal.required_predicates != expected_predicates {
        return Err(failure(
            "UNSUPPORTED_RECOVERY_GOAL",
            "unrecognized completion predicates remain unresolved",
        ));
    }
    let plan = result.content_plan.as_ref().ok_or_else(|| {
        failure(
            "RECOVERY_DATA_UNAVAILABLE",
            "adopted content plan unavailable",
        )
    })?;
    let render = result.render.as_ref().ok_or_else(|| {
        failure(
            "RECOVERY_DATA_UNAVAILABLE",
            "actual retained rendering unavailable",
        )
    })?;
    let mut unsigned_plan = plan.clone();
    unsigned_plan.id.clear();
    if plan.id != identity(&unsigned_plan)?
        || plan.text != result.text
        || plan.guarantee != result.guarantee
        || plan.locale != request.locale
        || plan.locale != "ja"
        || plan.required_units.is_empty()
        || matches!(plan.guarantee, Guarantee::Unknown | Guarantee::Hypothesis)
        || !validate_render(plan, render)
    {
        return Err(failure(
            "RECOVERY_CONTENT_MISMATCH",
            "retained content identity/meaning/render is unverified",
        ));
    }
    if result.input_id != request.input_id
        || result.input_version != request.input_version
        || result.id != fingerprint
    {
        return Err(failure(
            "RECOVERY_IDENTITY_MISMATCH",
            "adopted result differs from original input",
        ));
    }
    if let Intent::Ask { proposition } = &goal.intent {
        // A case in another client may have invalidated a premise while this
        // output was pending. Revalidate the same proof lineage; never adopt a
        // newly different answer during mere effect reconciliation.
        let memory = store.load_memory(actor)?;
        let answer = crate::relations::query(&memory, proposition, goal.budget.max_actions)?;
        let mut dependencies: Vec<String> =
            answer.facts.iter().map(|fact| fact.id.clone()).collect();
        dependencies.extend(answer.used_rules);
        if !matches!(answer.status.as_str(), "SUPPORTED" | "CONTRADICTED")
            || dependencies != result.dependencies
        {
            return Err(failure(
                "RECOVERY_PROOF_INVALIDATED",
                "original adopted relation proof is no longer valid unchanged",
            ));
        }
    }
    let model_bytes = store.get_bytes(actor, case, &plan.model_artifact_ref)?;
    let model: language::SequenceModel = serde_json::from_slice(&model_bytes)?;
    model.validate()?;
    let emission_bytes = store.get_bytes(actor, case, &plan.model_observation_ref)?;
    let emission: language::ModelEmission = serde_json::from_slice(&emission_bytes)?;
    let authorized_surfaces = match &emission.authorized_surfaces {
        Some(surfaces) => {
            let calculation = result.calculation.as_ref().ok_or_else(|| {
                failure(
                    "RECOVERY_MODEL_CANDIDATE_MISMATCH",
                    "multi-surface output has no retained calculation",
                )
            })?;
            if !matches!(goal.intent, Intent::ExplainPrevious)
                || surfaces.as_slice()
                    != language::calculation_explanation_surfaces(calculation).as_slice()
                || !surfaces.contains(&plan.text)
            {
                return Err(failure(
                    "RECOVERY_MODEL_CANDIDATE_MISMATCH",
                    "retained multi-surface output differs from the Core-approved calculation surfaces",
                ));
            }
            surfaces.clone()
        }
        // Legacy alpha 8 records contain no candidate list and can only prove
        // the exact singleton surface they already retained.
        None => vec![plan.text.clone()],
    };
    let verified_emission =
        model.realize_supported(&authorized_surfaces, request.budget.max_evidence_bytes)?;
    if canonical(&verified_emission)? != emission_bytes
        || emission.text != plan.text
        || emission.model_revision != plan.model_revision
    {
        return Err(failure(
            "RECOVERY_MODEL_MISMATCH",
            "retained native model observation does not reproduce its adopted surface",
        ));
    }
    let nodes: Vec<GoalNode> =
        serde_json::from_slice(&store.get_bytes(actor, case, &goal.required_graph_ref)?)?;
    let mut expected_nodes = Vec::new();
    let mut sources = Vec::new();
    if let Intent::Retrieve { urls, .. } = &goal.intent {
        if urls.len() != result.evidence.len() {
            return Err(failure(
                "RECOVERY_EVIDENCE_MISMATCH",
                "required original evidence is incomplete",
            ));
        }
        for (index, uri) in urls.iter().enumerate() {
            let packet = &result.evidence[index];
            if packet.request_uri != *uri
                || packet.failure.is_some()
                || packet.extracted_text.is_none()
                || packet.raw_sha256 != digest(&packet.raw_body)
            {
                return Err(failure(
                    "RECOVERY_EVIDENCE_MISMATCH",
                    "captured body identity or original evidence failed",
                ));
            }
            let id = format!("source-{index}");
            sources.push(id.clone());
            expected_nodes.push(GoalNode {
                id,
                kind: GoalNodeKind::Evidence,
                requires: Vec::new(),
            });
        }
    } else if !result.evidence.is_empty() {
        return Err(failure(
            "RECOVERY_EVIDENCE_MISMATCH",
            "undeclared evidence cannot satisfy this original graph",
        ));
    }
    expected_nodes.extend([
        GoalNode {
            id: "compute".into(),
            kind: GoalNodeKind::CoreResult,
            requires: sources.clone(),
        },
        GoalNode {
            id: "content".into(),
            kind: GoalNodeKind::ContentPlan,
            requires: vec!["compute".into()],
        },
        GoalNode {
            id: "render".into(),
            kind: GoalNodeKind::RenderedArtifact,
            requires: vec!["content".into()],
        },
        GoalNode {
            id: "file".into(),
            kind: GoalNodeKind::RequiredFile,
            requires: vec!["render".into()],
        },
    ]);
    if nodes != expected_nodes {
        return Err(failure(
            "UNSUPPORTED_RECOVERY_GRAPH",
            "graph differs from this verifier's supported artifact topology",
        ));
    }
    let mut graph = Scheduler::new(nodes, goal.budget.max_actions)?;
    let mut source_refs = BTreeMap::new();
    for (index, packet) in result.evidence.iter().enumerate() {
        let reference = verified_artifact(store, actor, case, packet)?;
        let id = format!("source-{index}");
        graph.observe(&id, &reference, &BTreeMap::new())?;
        source_refs.insert(id, reference);
    }
    let compute = (
        &result.calculation,
        &plan.required_units,
        &result.evidence,
        &result.dependencies,
    );
    let compute_ref = verified_artifact(store, actor, case, &compute)?;
    let mut dependencies = result.dependencies.clone();
    dependencies.push(compute_ref.clone());
    if plan.dependencies != dependencies {
        return Err(failure(
            "RECOVERY_DEPENDENCY_MISMATCH",
            "content did not retain the actual compute artifact",
        ));
    }
    graph.observe("compute", &compute_ref, &source_refs)?;
    let content_ref = verified_artifact(store, actor, case, plan)?;
    graph.observe(
        "content",
        &content_ref,
        &BTreeMap::from([("compute".into(), compute_ref)]),
    )?;
    let render_ref = verified_artifact(store, actor, case, render)?;
    graph.observe(
        "render",
        &render_ref,
        &BTreeMap::from([("content".into(), content_ref)]),
    )?;
    if !graph.ready().iter().any(|id| id == "file") {
        return Err(failure(
            "ACTION_BUDGET_EXHAUSTED",
            "original graph leaves no confirmed-file completion allowance",
        ));
    }
    let expected_action = identity(&(case, &fingerprint, "file", path, &render.actual_sha256))?;
    let actions: Vec<_> = store
        .actions(actor, case)?
        .into_iter()
        .filter(|action| action.input_fingerprint == fingerprint)
        .collect();
    if actions.len() != 1 {
        return Err(failure(
            "RECOVERY_ACTION_UNAVAILABLE",
            "exactly one original local effect identity is required",
        ));
    }
    let action = actions.into_iter().next().expect("one action was checked");
    if action.actor_id != actor
        || action.case_id != *case
        || action.effect_class != "LOCAL_WRITE"
        || action.action_id != expected_action
        || action.idempotency_key != expected_action
        || Some(action.target.as_str()) != path.to_str()
        || action.exact_request_sha256 != render.actual_sha256
        || !matches!(action.phase, EffectPhase::Unknown | EffectPhase::Confirmed)
    {
        return Err(failure(
            "RECOVERY_ACTION_MISMATCH",
            "original attempted identity/target/phase is not reconcilable",
        ));
    }
    let authorization: Permissions =
        serde_json::from_slice(&store.get_bytes(actor, case, &action.authorization_sha256)?)?;
    let exact = store.get_bytes(actor, case, &action.exact_request_sha256)?;
    if authorization != goal.permissions || exact != render.bytes {
        return Err(failure(
            "RECOVERY_AUTHORITY_MISMATCH",
            "original authorization or exact effect bytes differ",
        ));
    }
    observe_file(path, &exact)?;
    let receipt = if action.phase == EffectPhase::Confirmed {
        let receipt = action.receipt.clone().ok_or_else(|| {
            failure(
                "RECOVERY_RECEIPT_MISSING",
                "confirmed action has no original receipt",
            )
        })?;
        if receipt.action_id != action.action_id
            || receipt.target != action.target
            || receipt.phase != EffectPhase::Confirmed
            || receipt.content_sha256 != digest(&exact)
            || receipt.observed_bytes != Some(exact.len() as u64)
        {
            return Err(failure(
                "RECOVERY_RECEIPT_MISMATCH",
                "stored receipt does not match observed exact artifact",
            ));
        }
        receipt
    } else {
        DeliveryReceipt {
            action_id: action.action_id.clone(),
            target: action.target.clone(),
            content_sha256: digest(&exact),
            phase: EffectPhase::Confirmed,
            observed_bytes: Some(exact.len() as u64),
            reason: Some("LOCAL_FILE_RECONCILED_NO_RESEND".into()),
        }
    };
    Ok((action, receipt, graph, render_ref))
}

fn verified_artifact<T: serde::Serialize>(
    store: &Store,
    actor: &str,
    case: &str,
    value: &T,
) -> Result<String> {
    let expected = canonical(value)?;
    let reference = digest(&expected);
    if store.get_bytes(actor, case, &reference)? != expected {
        return Err(failure(
            "RECOVERY_ARTIFACT_MISMATCH",
            "retained actual artifact differs",
        ));
    }
    Ok(reference)
}

fn observe_file(path: &Path, expected: &[u8]) -> Result<()> {
    if !path.is_absolute()
        || path
            .components()
            .any(|p| matches!(p, Component::ParentDir | Component::CurDir))
        || path.file_name().is_none()
        || path.to_string_lossy().contains('\\')
        || path
            .to_string_lossy()
            .split('/')
            .any(|segment| segment == "." || segment == "..")
    {
        return Err(failure(
            "UNSAFE_RECONCILIATION_TARGET",
            "original target is not an unambiguous absolute path",
        ));
    }
    for ancestor in path.ancestors() {
        let metadata = fs::symlink_metadata(ancestor)?;
        if metadata.file_type().is_symlink()
            || (ancestor != path && !metadata.is_dir())
            || (ancestor == path && !metadata.is_file())
        {
            return Err(failure(
                "UNSAFE_RECONCILIATION_TARGET",
                "symlink or non-regular original target",
            ));
        }
    }
    let parent = path.parent().ok_or_else(|| {
        failure(
            "UNSAFE_RECONCILIATION_TARGET",
            "original parent unavailable",
        )
    })?;
    let parent_identity = parent.canonicalize()?;
    let mut file = File::open(path)?;
    if file.metadata()?.len() != expected.len() as u64 {
        return Err(failure(
            "LOCAL_EFFECT_UNCONFIRMED",
            "existing file has different bytes",
        ));
    }
    let mut actual = Vec::new();
    Read::by_ref(&mut file)
        .take(expected.len() as u64 + 1)
        .read_to_end(&mut actual)?;
    if actual != expected || parent.canonicalize()? != parent_identity {
        return Err(failure(
            "LOCAL_EFFECT_UNCONFIRMED",
            "existing file or parent identity changed",
        ));
    }
    // Confirm durability of the existing object; never create or rewrite it.
    file.sync_all()?;
    File::open(parent)?.sync_all()?;
    Ok(())
}

fn advance(
    store: &Store,
    state: &mut CaseRevision,
    result: &ResultRecord,
    next: Lifecycle,
) -> Result<()> {
    let mut revision = state.clone();
    revision.parent_revision = Some(state.revision);
    revision.revision += 1;
    revision.lifecycle = next;
    let mut observed = result.clone();
    observed.lifecycle = next;
    revision.reason = observed.reason.clone();
    revision.result = Some(observed);
    store.append_case(&state.actor_id, &revision)?;
    *state = revision;
    Ok(())
}
fn suspend(
    store: &Store,
    state: &mut CaseRevision,
    mut result: ResultRecord,
    reason: &str,
) -> Result<ResultRecord> {
    result.reason = Some(format!("LOCAL_RECONCILIATION_UNCONFIRMED: {reason}"));
    if state.lifecycle == Lifecycle::Suspend {
        if state.goal_sha256.is_none() || state.bundle.is_none() {
            result.lifecycle = Lifecycle::Suspend;
            return Ok(result);
        }
        advance(store, state, &result, Lifecycle::Running)?;
    }
    advance(store, state, &result, Lifecycle::Suspend)?;
    state
        .result
        .clone()
        .ok_or_else(|| failure("INTEGRITY_FAILURE", "suspension lost result"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::Engine;
    use rusqlite::{Connection, params};
    use std::{
        path::PathBuf,
        time::{SystemTime, UNIX_EPOCH},
    };

    struct Fixture {
        directory: PathBuf,
        state: PathBuf,
        target: PathBuf,
        token: String,
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.directory);
        }
    }

    fn empty_fixture() -> Fixture {
        let directory = std::env::temp_dir().join(format!(
            "minidora-recovery-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir(&directory).unwrap();
        Fixture {
            state: directory.join("state"),
            target: directory.join("actual-report.txt"),
            directory,
            token: String::new(),
        }
    }

    fn attempted_prefix() -> Fixture {
        let directory = std::env::temp_dir().join(format!(
            "minidora-recovery-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir(&directory).unwrap();
        let state = directory.join("state");
        let target = directory.join("actual-report.txt");
        let mut request = RequestEnvelope::new("case", "input", "alice", "2+3");
        request.output_path = Some(target.clone());
        request.permissions.write_paths.insert(target.clone());
        {
            let mut engine = Engine::open(&state).unwrap();
            assert_eq!(
                engine.execute("alice", request).unwrap().lifecycle,
                Lifecycle::Commit
            );
        }
        // Test-only crash injection: retain an actual production journal prefix
        // immediately before effect confirmation, while keeping the physical
        // output that already occurred. This models rollback of the later
        // transactions rather than fabricating a successful action result.
        let database = Connection::open(state.join("state.sqlite3")).unwrap();
        let snapshots: Vec<(i64, String)> = {
            let mut statement = database
                .prepare("SELECT revision,state_sha256 FROM case_revision ORDER BY revision")
                .unwrap();
            statement
                .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
                .unwrap()
                .collect::<std::result::Result<_, _>>()
                .unwrap()
        };
        let mut retained = None;
        for (number, sha) in snapshots {
            let revision: CaseRevision =
                serde_json::from_slice(&fs::read(state.join("blobs").join(sha)).unwrap()).unwrap();
            if revision.lifecycle == Lifecycle::Adopted
                && revision.result.as_ref().is_some_and(|r| r.render.is_some())
            {
                retained = Some((number, revision));
            }
        }
        let (number, revision) =
            retained.expect("normal Core persisted actual rendering before file effect");
        let result_sha = digest(&canonical(revision.result.as_ref().unwrap()).unwrap());
        let token: String = database
            .query_row("SELECT idempotency_key FROM action_identity", [], |r| {
                r.get(0)
            })
            .unwrap();
        database.execute_batch("PRAGMA foreign_keys=ON; DROP TRIGGER case_revision_no_delete; DROP TRIGGER action_history_no_delete;").unwrap();
        database
            .execute("DELETE FROM case_revision WHERE revision>?1", [number])
            .unwrap();
        database
            .execute("DELETE FROM action_history WHERE sequence>3", [])
            .unwrap();
        database
            .execute(
                "UPDATE input_identity SET result_sha256=?1",
                params![result_sha],
            )
            .unwrap();
        drop(database);
        Fixture {
            directory,
            state,
            target,
            token,
        }
    }

    #[test]
    fn calculation_explanation_candidates_replay_for_recovery() {
        let fixture = empty_fixture();
        let mut engine = Engine::open(&fixture.state).unwrap();
        let mut calculation = RequestEnvelope::new("case", "calculation", "alice", "2+3");
        calculation.output_path = Some(fixture.target.clone());
        calculation
            .permissions
            .write_paths
            .insert(fixture.target.clone());
        assert_eq!(
            engine.execute("alice", calculation).unwrap().lifecycle,
            Lifecycle::Commit
        );

        let explanation_target = fixture.target.with_file_name("explanation.txt");
        let mut request = RequestEnvelope::new("case", "explanation", "alice", "詳しく説明して");
        request.output_path = Some(explanation_target.clone());
        request.permissions.write_paths.insert(explanation_target);
        assert_eq!(
            engine.execute("alice", request).unwrap().lifecycle,
            Lifecycle::Commit
        );

        let revision = engine.latest("alice", "case").unwrap().unwrap();
        let result = revision.result.as_ref().unwrap();
        let plan = result.content_plan.as_ref().unwrap();
        let emission_bytes = engine
            .store
            .get_bytes("alice", "case", &plan.model_observation_ref)
            .unwrap();
        let emission: language::ModelEmission = serde_json::from_slice(&emission_bytes).unwrap();
        assert_eq!(emission.authorized_surfaces.as_ref().map(Vec::len), Some(2));
        assert!(inspect(&engine.store, "alice", &revision, result).is_ok());
    }

    #[test]
    fn interrupted_actual_write_reconciles_same_identity_without_resending() {
        let fixture = attempted_prefix();
        let original = fs::read(&fixture.target).unwrap();
        let modified = fs::metadata(&fixture.target).unwrap().modified().unwrap();
        let store = Store::open(&fixture.state).unwrap();
        assert_eq!(
            store.actions("alice", "case").unwrap()[0].phase,
            EffectPhase::Unknown
        );
        let result = reconcile(&store, "alice", "case").unwrap();
        assert_eq!(result.lifecycle, Lifecycle::Commit);
        assert!(result.execution_graph_ref.is_some());
        let actions = store.actions("alice", "case").unwrap();
        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0].idempotency_key, fixture.token);
        assert_eq!(actions[0].phase, EffectPhase::Confirmed);
        assert_eq!(fs::read(&fixture.target).unwrap(), original);
        assert_eq!(
            fs::metadata(&fixture.target).unwrap().modified().unwrap(),
            modified
        );
        drop(store);
        let restarted = Store::open(&fixture.state).unwrap();
        assert_eq!(
            reconcile(&restarted, "alice", "case").unwrap().lifecycle,
            Lifecycle::Commit
        );
        assert_eq!(restarted.actions("alice", "case").unwrap().len(), 1);
    }
    #[test]
    fn missing_and_different_files_stay_unknown_and_are_never_rewritten() {
        for missing in [false, true] {
            let fixture = attempted_prefix();
            if missing {
                fs::remove_file(&fixture.target).unwrap();
            } else {
                fs::write(&fixture.target, b"different actual file").unwrap();
            }
            let store = Store::open(&fixture.state).unwrap();
            let result = reconcile(&store, "alice", "case").unwrap();
            assert_eq!(result.lifecycle, Lifecycle::Suspend);
            assert_eq!(
                store.actions("alice", "case").unwrap()[0].phase,
                EffectPhase::Unknown
            );
            if missing {
                assert!(!fixture.target.exists());
            } else {
                assert_eq!(fs::read(&fixture.target).unwrap(), b"different actual file");
            }
            assert!(reconcile(&store, "bob", "case").is_err());
        }
    }
    #[test]
    fn originally_unauthorized_file_cannot_be_confirmed_by_recovery() {
        let directory = std::env::temp_dir().join(format!(
            "minidora-recovery-noauth-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir(&directory).unwrap();
        let state = directory.join("state");
        let target = directory.join("report.txt");
        let mut request = RequestEnvelope::new("case", "input", "alice", "2+3");
        request.output_path = Some(target.clone());
        let mut engine = Engine::open(&state).unwrap();
        let result = engine.execute("alice", request).unwrap();
        assert_eq!(result.lifecycle, Lifecycle::Suspend);
        drop(engine);
        let store = Store::open(&state).unwrap();
        assert_eq!(
            reconcile(&store, "alice", "case").unwrap().lifecycle,
            Lifecycle::Suspend
        );
        assert!(store.actions("alice", "case").unwrap().is_empty());
        assert!(!target.exists());
        drop(store);
        fs::remove_dir_all(directory).unwrap();
    }
    #[cfg(unix)]
    #[test]
    fn a_replaced_symlink_is_not_an_actual_original_file_receipt() {
        use std::os::unix::fs::symlink;
        let fixture = attempted_prefix();
        let moved = fixture.directory.join("different-target.txt");
        fs::rename(&fixture.target, &moved).unwrap();
        symlink(&moved, &fixture.target).unwrap();
        let store = Store::open(&fixture.state).unwrap();
        assert_eq!(
            reconcile(&store, "alice", "case").unwrap().lifecycle,
            Lifecycle::Suspend
        );
        assert_eq!(
            store.actions("alice", "case").unwrap()[0].phase,
            EffectPhase::Unknown
        );
    }
    #[test]
    fn authorized_cancel_stops_pending_goal_and_retains_unknown_effect_identity() {
        let fixture = attempted_prefix();
        let original = fs::read(&fixture.target).unwrap();
        let modified = fs::metadata(&fixture.target).unwrap().modified().unwrap();
        let mut engine = Engine::open(&fixture.state).unwrap();
        let cancel = RequestEnvelope::new("case", "cancel-input", "alice", "キャンセル");
        let result = engine.execute("alice", cancel).unwrap();
        assert_eq!(result.lifecycle, Lifecycle::Cancel);
        assert_eq!(result.input_id, "cancel-input");
        let latest = engine.latest("alice", "case").unwrap().unwrap();
        assert_eq!(latest.lifecycle, Lifecycle::Cancel);
        assert_eq!(
            latest.bundle.as_ref().unwrap().request.input_id,
            "cancel-input"
        );
        let original_cancel = latest
            .prior_results
            .values()
            .find(|prior| prior.input_id == "input")
            .expect("old result retained");
        assert_eq!(original_cancel.lifecycle, Lifecycle::Cancel);
        let store = Store::open(&fixture.state).unwrap();
        let actions = store.actions("alice", "case").unwrap();
        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0].idempotency_key, fixture.token);
        assert_eq!(actions[0].phase, EffectPhase::Unknown);
        assert_eq!(
            store
                .get_input_result("alice", "case", "input", 1)
                .unwrap()
                .unwrap()
                .lifecycle,
            Lifecycle::Cancel
        );
        assert_eq!(fs::read(&fixture.target).unwrap(), original);
        assert_eq!(
            fs::metadata(&fixture.target).unwrap().modified().unwrap(),
            modified
        );
        assert!(engine.reconcile("alice", "case").is_err());
    }

    #[test]
    fn unknown_original_blocks_new_effect_until_same_identity_reconciliation() {
        let fixture = attempted_prefix();
        let next_target = fixture.directory.join("new-effect.txt");
        let original = fs::read(&fixture.target).unwrap();
        let mut engine = Engine::open(&fixture.state).unwrap();
        let mut request = RequestEnvelope::new("case", "new-effect-input", "alice", "7+8");
        request.output_path = Some(next_target.clone());
        request.permissions.write_paths.insert(next_target.clone());
        let blocked = engine.execute("alice", request.clone()).unwrap();
        assert_eq!(blocked.lifecycle, Lifecycle::Suspend);
        assert!(!next_target.exists());
        assert_eq!(
            engine
                .latest("alice", "case")
                .unwrap()
                .unwrap()
                .bundle
                .unwrap()
                .request
                .input_id,
            "input"
        );
        {
            let store = Store::open(&fixture.state).unwrap();
            let actions = store.actions("alice", "case").unwrap();
            assert_eq!(actions.len(), 1);
            assert_eq!(actions[0].idempotency_key, fixture.token);
            assert_eq!(actions[0].phase, EffectPhase::Unknown);
        }
        let recovered = engine.reconcile("alice", "case").unwrap();
        assert_eq!(recovered.lifecycle, Lifecycle::Commit);
        assert_eq!(recovered.input_id, "input");
        assert!(!next_target.exists());
        assert_eq!(fs::read(&fixture.target).unwrap(), original);
        {
            let store = Store::open(&fixture.state).unwrap();
            let actions = store.actions("alice", "case").unwrap();
            assert_eq!(actions.len(), 1);
            assert_eq!(actions[0].idempotency_key, fixture.token);
            assert_eq!(actions[0].phase, EffectPhase::Confirmed);
        }
        // A previously rejected/interrupted identity is a tombstone. Explicit
        // new input version is necessary for a new later authorized effect.
        assert_eq!(
            engine.execute("alice", request.clone()).unwrap().lifecycle,
            Lifecycle::Suspend
        );
        assert!(!next_target.exists());
        request.input_version = 2;
        assert_eq!(
            engine.execute("alice", request).unwrap().lifecycle,
            Lifecycle::Commit
        );
        assert!(next_target.is_file());
        assert_eq!(fs::read(&fixture.target).unwrap(), original);
    }

    #[test]
    fn cancellation_from_suspended_unknown_case_preserves_observed_effects() {
        let fixture = attempted_prefix();
        // A mismatching local artifact makes explicit recovery suspend. A
        // later cancel must still stop future work without altering that file.
        fs::write(&fixture.target, b"actual changed artifact").unwrap();
        let mut engine = Engine::open(&fixture.state).unwrap();
        assert_eq!(
            engine.reconcile("alice", "case").unwrap().lifecycle,
            Lifecycle::Suspend
        );
        let cancel = RequestEnvelope::new("case", "cancel-suspended", "alice", "キャンセル");
        assert_eq!(
            engine.execute("alice", cancel).unwrap().lifecycle,
            Lifecycle::Cancel
        );
        let store = Store::open(&fixture.state).unwrap();
        let actions = store.actions("alice", "case").unwrap();
        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0].idempotency_key, fixture.token);
        assert_eq!(actions[0].phase, EffectPhase::Unknown);
        assert_eq!(
            fs::read(&fixture.target).unwrap(),
            b"actual changed artifact"
        );
    }
    #[test]
    fn cancelled_goal_stays_cancelled_when_original_file_effect_is_observed() {
        let fixture = attempted_prefix();
        let original = fs::read(&fixture.target).unwrap();
        let modified = fs::metadata(&fixture.target).unwrap().modified().unwrap();
        let mut engine = Engine::open(&fixture.state).unwrap();
        assert!(engine.reconcile_cancelled_effects("alice", "case").is_err());
        let cancel = RequestEnvelope::new("case", "cancel-bookkeeping", "alice", "キャンセル");
        assert_eq!(
            engine.execute("alice", cancel).unwrap().lifecycle,
            Lifecycle::Cancel
        );
        let before = engine.latest("alice", "case").unwrap().unwrap();
        let actions = engine.reconcile_cancelled_effects("alice", "case").unwrap();
        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0].phase, EffectPhase::Confirmed);
        assert_eq!(actions[0].idempotency_key, fixture.token);
        assert!(
            actions[0]
                .receipt
                .as_ref()
                .unwrap()
                .reason
                .as_deref()
                .unwrap()
                .contains("NO_RESEND")
        );
        assert_eq!(engine.latest("alice", "case").unwrap().unwrap(), before);
        assert_eq!(fs::read(&fixture.target).unwrap(), original);
        assert_eq!(
            fs::metadata(&fixture.target).unwrap().modified().unwrap(),
            modified
        );
        // Once the old physical outcome is known, fresh authorized work can
        // proceed; the canceled input remains a canceled historical result.
        let next = RequestEnvelope::new("case", "after-cancel-bookkeeping", "alice", "11+2");
        assert_eq!(
            engine.execute("alice", next).unwrap().lifecycle,
            Lifecycle::Commit
        );
        let store = Store::open(&fixture.state).unwrap();
        assert_eq!(
            store
                .get_input_result("alice", "case", "cancel-bookkeeping", 1)
                .unwrap()
                .unwrap()
                .lifecycle,
            Lifecycle::Cancel
        );
        assert_eq!(store.actions("alice", "case").unwrap().len(), 1);
    }

    #[test]
    fn cancelled_missing_effect_stays_unknown_and_new_cancel_permissions_do_not_apply() {
        let fixture = attempted_prefix();
        fs::remove_file(&fixture.target).unwrap();
        let other = fixture.directory.join("unrelated-permission.txt");
        let original_auth = Store::open(&fixture.state)
            .unwrap()
            .actions("alice", "case")
            .unwrap()[0]
            .authorization_sha256
            .clone();
        let mut engine = Engine::open(&fixture.state).unwrap();
        let mut cancel =
            RequestEnvelope::new("case", "cancel-with-other-scope", "alice", "キャンセル");
        cancel.output_path = Some(other.clone());
        cancel.permissions.write_paths.insert(other.clone());
        assert_eq!(
            engine.execute("alice", cancel).unwrap().lifecycle,
            Lifecycle::Cancel
        );
        let before = engine.latest("alice", "case").unwrap().unwrap();
        assert!(engine.reconcile_cancelled_effects("bob", "case").is_err());
        let actions = engine.reconcile_cancelled_effects("alice", "case").unwrap();
        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0].phase, EffectPhase::Unknown);
        assert_eq!(actions[0].idempotency_key, fixture.token);
        assert_eq!(actions[0].authorization_sha256, original_auth);
        assert!(!fixture.target.exists());
        assert!(!other.exists());
        assert_eq!(engine.latest("alice", "case").unwrap().unwrap(), before);
        let mut next = RequestEnvelope::new("case", "still-blocked-after-cancel", "alice", "7+8");
        next.output_path = Some(other.clone());
        next.permissions.write_paths.insert(other.clone());
        assert_eq!(
            engine.execute("alice", next).unwrap().lifecycle,
            Lifecycle::Suspend
        );
        assert!(!other.exists());
        assert_eq!(
            engine.latest("alice", "case").unwrap().unwrap().lifecycle,
            Lifecycle::Cancel
        );
    }
}
