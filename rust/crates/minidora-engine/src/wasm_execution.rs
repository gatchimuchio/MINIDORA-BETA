//! Core-owned authorization and observation of the local WebAssembly port.
use crate::csv_wasm::CsvAggregationProof;
use crate::durability::{
    is_link_or_reparse_point, keeps_local_target_name, local_target_and_ancestors,
};
use crate::store::Store;
use crate::wasm_process::{self, ExecutionObservation};
use minidora_domain::{
    ActionRecord, CaseRevision, DeliveryReceipt, EffectPhase, GoalContract, Guarantee, InputBundle,
    Intent, Lifecycle, Result, canonical, digest, failure, identity,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

pub(crate) const EFFECT: &str = "SANDBOX_WASM_EXECUTE";
const PORT: &str = "local-wasm-v8-wasi-closed-v1";
const MAX_PROGRAM_BYTES: usize = 65536;
const PROCESS_LIMIT: Duration = Duration::from_secs(4);
const REPORT_VERSION: &str = "wasm-execution-observation-v1";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ExecutionProgram {
    pub module_hex: String,
    #[serde(default)]
    pub stdin_hex: String,
    #[serde(default)]
    pub files: Vec<String>,
}

fn invalid_program() -> minidora_domain::Error {
    failure(
        "WASM_EXECUTION_INPUT",
        "WebAssemblyの小文字16進データをmodule_hexに、任意の標準入力をstdin_hexに、安全な生成物名をfilesに指定してください。",
    )
}

fn hex_bytes(value: &str, limit: usize) -> Result<Vec<u8>> {
    if value.len() % 2 != 0 || value.len() / 2 > limit {
        return Err(invalid_program());
    }
    let nibble = |c: u8| match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'f' => Some(c - b'a' + 10),
        _ => None,
    };
    value
        .as_bytes()
        .chunks_exact(2)
        .map(|pair| {
            nibble(pair[0])
                .zip(nibble(pair[1]))
                .map(|(a, b)| a * 16 + b)
                .ok_or_else(invalid_program)
        })
        .collect()
}

pub(crate) fn artifact_name(name: &str) -> bool {
    let first = name.split('.').next().unwrap_or("").to_ascii_uppercase();
    let reserved = matches!(first.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || first
            .strip_prefix("COM")
            .or_else(|| first.strip_prefix("LPT"))
            .is_some_and(|n| matches!(n.as_bytes(), [b'1'..=b'9']));
    !reserved
        && !name.eq_ignore_ascii_case("receipt.json")
        && !name.ends_with('.')
        && name.len() <= 64
        && name
            .as_bytes()
            .first()
            .is_some_and(u8::is_ascii_alphanumeric)
        && name
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'.' | b'-' | b'_'))
}

pub(crate) struct ExecutionPlan {
    pub action: ActionRecord,
    pub program: ExecutionProgram,
    compiled: Option<CsvAggregationProof>,
    authorization: Vec<u8>,
    submission: Vec<u8>,
}

pub(crate) fn handles(intent: &Intent) -> bool {
    matches!(
        intent,
        Intent::ExecuteWasm { .. } | Intent::ExecuteAttachedCsvAggregation { .. }
    )
}

pub(crate) fn plan(bundle: &InputBundle) -> Result<ExecutionPlan> {
    let request = &bundle.request;
    let document = match bundle.meaning.intent {
        Intent::ExecuteWasm { .. } => Some(crate::input::verified_wasm_document(bundle)?),
        Intent::ExecuteAttachedCsvAggregation { .. } => None,
        _ => return Err(invalid_program()),
    };
    if !request.permissions.wasm_execution {
        return Err(failure(
            "WASM_EXECUTION_PERMISSION_REQUIRED",
            "この発話には隔離実行の許可が必要です。チャットでは直前に /allow-exec を入力してください。",
        ));
    }
    if request.output_path.is_some() {
        return Err(failure(
            "WASM_EXTERNAL_OUTPUT_UNSUPPORTED",
            "隔離実行の生成物は実行領域に保存されます。先に /output off で別の保存先指定を解除してください。",
        ));
    }
    let (program, compiled) = if let Some(document) = document {
        crate::ports::strict_json_document(document)?;
        let program = serde_json::from_str(document).map_err(|_| invalid_program())?;
        (program, None)
    } else {
        let (program, proof) = crate::csv_wasm::compile(bundle)?;
        (program, Some(proof))
    };
    let code = hex_bytes(&program.module_hex, MAX_PROGRAM_BYTES)?;
    if !code.starts_with(b"\0asm\x01\0\0\0")
        || program.files.len() > 4
        || program.files.iter().any(|name| !artifact_name(name))
        || program
            .files
            .iter()
            .map(|name| name.to_ascii_lowercase())
            .collect::<BTreeSet<_>>()
            .len()
            != program.files.len()
    {
        return Err(invalid_program());
    }
    hex_bytes(&program.stdin_hex, MAX_PROGRAM_BYTES)?;
    let target = format!("wasm:sha256:{}", digest(&code));
    let authorization = canonical(&(
        PORT,
        "LOCAL_EXPLICIT_EXECUTION_GRANT",
        &bundle.fingerprint,
        &request.actor_id,
        &request.case_id,
        &request.input_id,
        request.input_version,
        &request.permissions,
        &request.budget,
        &target,
    ))?;
    let submission = canonical(&program)?;
    let authorization_sha256 = digest(&authorization);
    let exact_request_sha256 = digest(&submission);
    let idempotency_key = identity(&(PORT, EFFECT, &bundle.fingerprint, &exact_request_sha256))?;
    let action_id = identity(&(EFFECT, &idempotency_key, &authorization_sha256, &target))?;
    Ok(ExecutionPlan {
        action: ActionRecord {
            case_id: request.case_id.clone(),
            actor_id: request.actor_id.clone(),
            action_id,
            input_fingerprint: identity(request)?,
            idempotency_key,
            effect_class: EFFECT.into(),
            target,
            authorization_sha256,
            exact_request_sha256,
            phase: EffectPhase::Proposed,
            receipt: None,
        },
        program,
        compiled,
        authorization,
        submission,
    })
}

pub(crate) struct NodeRuntime {
    path: PathBuf,
    sha256: String,
}

impl NodeRuntime {
    pub(crate) fn configure(path: PathBuf) -> Result<Self> {
        let expected_name = if cfg!(windows) { "node.exe" } else { "node" };
        if path.file_name().and_then(|name| name.to_str()) != Some(expected_name)
            || crate::durability::validate_local_target_path(&path).is_err()
        {
            return Err(failure(
                "WASM_RUNTIME_PATH",
                "信頼するNode実行ファイルを絶対パスで指定してください。",
            ));
        }
        let sha256 = runtime_hash(&path)?;
        Ok(Self { path, sha256 })
    }
}

fn runtime_hash(path: &Path) -> Result<String> {
    for part in local_target_and_ancestors(path) {
        if is_link_or_reparse_point(&fs::symlink_metadata(part)?) {
            return Err(failure(
                "WASM_RUNTIME_PATH",
                "Nodeのパスにリンクまたは再解析点があります。",
            ));
        }
    }
    let file = File::open(path)?;
    let metadata = file.metadata()?;
    if !metadata.is_file() || metadata.len() > 256 * 1024 * 1024 || !keeps_local_target_name(path)?
    {
        return Err(failure(
            "WASM_RUNTIME_PATH",
            "Node実行ファイルの実体を確認できません。",
        ));
    }
    let mut bytes = [0u8; 16384];
    let mut limited = file.take(metadata.len() + 1);
    let mut hasher = Sha256::new();
    let mut observed = 0u64;
    loop {
        let n = limited.read(&mut bytes)?;
        if n == 0 {
            break;
        }
        hasher.update(&bytes[..n]);
        observed += n as u64;
    }
    if observed != metadata.len() {
        return Err(failure(
            "WASM_RUNTIME_CHANGED",
            "Node実行ファイルの読み取り中に長さが変わりました。",
        ));
    }
    Ok(format!("{:x}", hasher.finalize()))
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ExecutionReport {
    version: String,
    action_id: String,
    module_identity: String,
    runtime_sha256: String,
    worker_sha256: String,
    request_sha256: String,
    scratch: PathBuf,
    observation: Option<ExecutionObservation>,
    launch_failure: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    compiled: Option<CsvAggregationProof>,
}

pub(crate) struct Outcome {
    pub artifact: String,
    pub receipt: DeliveryReceipt,
    pub summary: String,
}

pub(crate) fn execute(
    store: &Store,
    state: &CaseRevision,
    bundle: &InputBundle,
    runtime: Option<&NodeRuntime>,
    deadline: Instant,
) -> Result<Outcome> {
    let mut planned = plan(bundle)?;
    let runtime = runtime.ok_or_else(|| {
        failure(
            "WASM_RUNTIME_REQUIRED",
            "起動時に --wasm-node で、信頼するローカルNodeを指定してください。",
        )
    })?;
    if runtime_hash(&runtime.path)? != runtime.sha256 {
        return Err(failure(
            "WASM_RUNTIME_CHANGED",
            "起動時に照合したNode実行ファイルから変更されています。",
        ));
    }
    let actor = &state.actor_id;
    let case = &state.case_id;
    if actor != &planned.action.actor_id
        || case != &planned.action.case_id
        || state.input_fingerprint.as_ref() != Some(&planned.action.input_fingerprint)
        || Instant::now() >= deadline
    {
        return Err(failure(
            "WASM_GOAL_NOT_ACTIVE",
            "隔離実行の依頼または期限を照合できません。",
        ));
    }
    // Two streams can each appear as hex in both the report and its rendering.
    // Reserve space for all declared file bytes, the request and path metadata.
    let reserve = MAX_PROGRAM_BYTES * (8 + 2 * planned.program.files.len())
        + planned.submission.len()
        + 131_072;
    if bundle.request.budget.max_evidence_bytes < reserve {
        return Err(failure(
            "WASM_EVIDENCE_BUDGET",
            "隔離実行の出力と受領を保存する証拠予算が不足しています。",
        ));
    }
    let scratch = store.create_wasm_scratch(&planned.action.action_id)?;
    let auth_ref = store.put_bytes(actor, case, &planned.authorization)?;
    let submission_ref = store.put_bytes(actor, case, &planned.submission)?;
    if auth_ref != planned.action.authorization_sha256
        || submission_ref != planned.action.exact_request_sha256
    {
        return Err(failure(
            "WASM_ACTION_MISMATCH",
            "保存した実行権限と入力を照合できません。",
        ));
    }
    for phase in [
        EffectPhase::Proposed,
        EffectPhase::Authorized,
        EffectPhase::Prepared,
        EffectPhase::Attempted,
    ] {
        planned.action.phase = phase;
        store.save_live_wasm_phase(actor, &planned.action, state.revision, deadline)?;
    }
    let process_deadline = (Instant::now() + PROCESS_LIMIT).min(deadline);
    let (observation, launch_failure) =
        match wasm_process::run(&runtime.path, &scratch, &planned.program, process_deadline) {
            Ok(value) => (Some(value), None),
            Err(error) => (None, Some(error.to_string())),
        };
    let report = ExecutionReport {
        version: REPORT_VERSION.into(),
        action_id: planned.action.action_id.clone(),
        module_identity: planned.action.target.clone(),
        runtime_sha256: runtime.sha256.clone(),
        worker_sha256: digest(include_bytes!("wasm_guest.js")),
        request_sha256: submission_ref,
        scratch,
        observation,
        launch_failure,
        compiled: planned.compiled.clone(),
    };
    let bytes = canonical(&report)?;
    // Preserve a bounded raw observation before applying the display budget.
    let artifact = store.put_bytes(actor, case, &bytes)?;
    let phase = match report
        .observation
        .as_ref()
        .map(|value| value.status.as_str())
    {
        Some("UNCONFIRMED") => EffectPhase::Unknown,
        None => EffectPhase::Failed,
        Some(_) => EffectPhase::Confirmed,
    };
    let receipt = DeliveryReceipt {
        action_id: report.action_id.clone(),
        target: report.module_identity.clone(),
        content_sha256: artifact.clone(),
        phase,
        observed_bytes: Some(bytes.len() as u64),
        reason: Some(
            report
                .observation
                .as_ref()
                .map_or("NOT_LAUNCHED", |x| x.status.as_str())
                .into(),
        ),
    };
    planned.action.phase = phase;
    planned.action.receipt = Some(receipt.clone());
    store.save_action(actor, &planned.action)?;
    let summary = observed_summary(&report, &receipt, bundle.request.budget.max_evidence_bytes)?;
    Ok(Outcome {
        artifact,
        receipt,
        summary,
    })
}

fn observed_summary(
    report: &ExecutionReport,
    receipt: &DeliveryReceipt,
    evidence_budget: usize,
) -> Result<String> {
    let summary = summarize(report)?;
    let report_bytes = receipt
        .observed_bytes
        .and_then(|size| usize::try_from(size).ok())
        .unwrap_or(usize::MAX);
    if summary.len().saturating_add(report_bytes) > evidence_budget {
        Ok(format!(
            "実行の観測を保存しました。証拠予算に達したため、出力の表示を保留します。記録 SHA-256 {}",
            receipt.content_sha256
        ))
    } else {
        Ok(summary)
    }
}

fn stream(value: &str) -> Result<String> {
    let bytes = hex_bytes(value, MAX_PROGRAM_BYTES)?;
    match String::from_utf8(bytes) {
        Ok(text) => Ok(text
            .chars()
            .map(|ch| {
                if (ch.is_control() && !matches!(ch, '\n' | '\t'))
                    || matches!(ch, '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}')
                {
                    '�'
                } else {
                    ch
                }
            })
            .collect()),
        Err(_) => Ok(format!("16進 {value}")),
    }
}

fn summarize(report: &ExecutionReport) -> Result<String> {
    let Some(observation) = &report.observation else {
        return Ok(format!(
            "隔離プロセスを起動できませんでした。\n{}",
            report.launch_failure.as_deref().unwrap_or("起動失敗")
        ));
    };
    let disposition = match observation.status.as_str() {
        "EXITED" if observation.guest_exit_code == Some(0) => "正常終了",
        "EXITED" => "終了コードが非ゼロ",
        "REFUSED" => "実行前にモジュールを拒否",
        "TRAP" => "実行中のトラップ",
        "TIMEOUT" => "時間上限で停止",
        "OUTPUT_LIMIT" => "出力上限で停止",
        _ => "終了を確認できませんでした",
    };
    let exit = |code: Option<i32>| code.map_or("なし".into(), |n| n.to_string());
    let mut lines = vec![
        format!(
            "WebAssembly隔離実行は{disposition}。ゲスト終了コード {}、プロセス終了コード {}。",
            exit(observation.guest_exit_code),
            exit(observation.physical_exit_code)
        ),
        format!("標準出力\n{}", stream(&observation.stdout_hex)?),
        format!("標準エラー\n{}", stream(&observation.stderr_hex)?),
    ];
    if report.compiled.is_some() {
        lines.insert(
            0,
            format!(
                "添付CSVの依頼から生成したWebAssembly {}。実行入力 SHA-256 {}。",
                report.module_identity, report.request_sha256
            ),
        );
    }
    if let Some(detail) = &observation.detail {
        lines.push(detail.clone());
    }
    for file in &observation.files {
        lines.push(format!(
            "生成物 {}（{}バイト、SHA-256 {}）\n{}",
            file.name,
            file.bytes_hex.len() / 2,
            file.sha256,
            report.scratch.join(&file.name).display()
        ));
    }
    Ok(lines.join("\n"))
}

fn action_matches_plan(action: &ActionRecord, planned: &ExecutionPlan) -> bool {
    let mut identity_only = action.clone();
    identity_only.phase = EffectPhase::Proposed;
    identity_only.receipt = None;
    identity_only == planned.action
}

fn read_bound_report(
    store: &Store,
    action: &ActionRecord,
    receipt: &DeliveryReceipt,
    planned: &ExecutionPlan,
) -> Result<ExecutionReport> {
    let bytes = store.get_bytes(&action.actor_id, &action.case_id, &receipt.content_sha256)?;
    let report: ExecutionReport = serde_json::from_slice(&bytes)?;
    if report.version != REPORT_VERSION
        || report.action_id != action.action_id
        || report.module_identity != action.target
        || report.request_sha256 != action.exact_request_sha256
        || report.compiled != planned.compiled
        || digest(&bytes) != receipt.content_sha256
        || receipt.observed_bytes != Some(bytes.len() as u64)
    {
        return Err(failure(
            "WASM_RECOVERY_REPORT",
            "保存した実行観測を照合できません。",
        ));
    }
    if report.compiled.is_some()
        && (store.get_bytes(
            &action.actor_id,
            &action.case_id,
            &action.authorization_sha256,
        )? != planned.authorization
            || store.get_bytes(
                &action.actor_id,
                &action.case_id,
                &action.exact_request_sha256,
            )? != planned.submission)
    {
        return Err(failure(
            "WASM_RECOVERY_COMPILATION",
            "元のCSVから再現した実行入力と保存済みの許可を照合できません。",
        ));
    }
    if report.compiled.is_some()
        && let Some(observation) = &report.observation
    {
        let mut names = BTreeSet::new();
        for file in &observation.files {
            if !planned.program.files.contains(&file.name)
                || !names.insert(file.name.as_str())
                || digest(&hex_bytes(&file.bytes_hex, MAX_PROGRAM_BYTES)?) != file.sha256
            {
                return Err(failure(
                    "WASM_RECOVERY_COMPILATION",
                    "保存したCSV生成物の宣言とハッシュを照合できません。",
                ));
            }
        }
        if receipt.target != action.target
            || receipt.reason.as_deref() != Some(observation.status.as_str())
        {
            return Err(failure(
                "WASM_RECOVERY_COMPILATION",
                "保存したCSV実行結果の状態と受領を照合できません。",
            ));
        }
    }
    Ok(report)
}

pub(crate) struct RetainedCsvArtifact {
    pub text: String,
    pub execution_id: String,
    pub report_ref: String,
    pub source_sha256: String,
}

/// Select the authenticated bytes from a committed execution report. The
/// original scratch directory is only the delivery destination.
pub(crate) fn read_csv_artifact(
    store: &Store,
    state: &CaseRevision,
    consumer: &InputBundle,
    name: &str,
) -> Result<RetainedCsvArtifact> {
    let invalid_context = || {
        failure(
            "WASM_CSV_CONTEXT_INVALID",
            "同じ会話で正常終了を確認した隔離実行を指定してください。",
        )
    };
    let unavailable = || {
        failure(
            "WASM_CSV_ARTIFACT_UNAVAILABLE",
            "指定されたCSVが、実行時に宣言して保存した生成物に見つかりません。",
        )
    };
    if !artifact_name(name)
        || !name
            .rsplit_once('.')
            .is_some_and(|(_, extension)| extension.eq_ignore_ascii_case("csv"))
    {
        return Err(failure(
            "WASM_CSV_ARTIFACT_NAME",
            "保存済みのCSV生成物の名前を指定してください。",
        ));
    }
    let req = &consumer.request;
    if req.actor_id != state.actor_id || req.case_id != state.case_id {
        return Err(invalid_context());
    }
    let prior = consumer
        .meaning
        .context_result_ref
        .as_ref()
        .and_then(|id| state.prior_results.get(id).filter(|r| &r.id == id))
        .ok_or_else(invalid_context)?;
    if prior.lifecycle != Lifecycle::Commit
        || prior.guarantee != Guarantee::Computed
        || identity(&Some(prior))? != consumer.context_sha256
        || store
            .get_input_result(
                &req.actor_id,
                &req.case_id,
                &prior.input_id,
                prior.input_version,
            )?
            .as_ref()
            != Some(prior)
    {
        return Err(invalid_context());
    }
    let (_, source) = store
        .get_input_bundle(
            &req.actor_id,
            &req.case_id,
            &prior.input_id,
            prior.input_version,
        )?
        .ok_or_else(invalid_context)?;
    if source.request.actor_id != req.actor_id
        || source.request.case_id != req.case_id
        || identity(&source.request)? != prior.id
        || !handles(&source.meaning.intent)
    {
        return Err(invalid_context());
    }
    let planned = plan(&source)?;
    let [receipt] = prior.receipts.as_slice() else {
        return Err(invalid_context());
    };
    if receipt.phase != EffectPhase::Confirmed
        || receipt.reason.as_deref() != Some("EXITED")
        || prior.dependencies.as_slice() != [receipt.content_sha256.as_str()]
    {
        return Err(invalid_context());
    }
    let action = store
        .actions(&req.actor_id, &req.case_id)?
        .into_iter()
        .find(|candidate| candidate.action_id == receipt.action_id)
        .filter(|candidate| {
            candidate.phase == EffectPhase::Confirmed
                && candidate.receipt.as_ref() == Some(receipt)
                && action_matches_plan(candidate, &planned)
        })
        .ok_or_else(invalid_context)?;
    if receipt
        .observed_bytes
        .is_none_or(|size| size > req.budget.max_evidence_bytes as u64)
    {
        return Err(failure(
            "WASM_CSV_EVIDENCE_LIMIT",
            "元の実行記録を確認するための証拠予算が不足しています。",
        ));
    }
    crate::document::revalidate_previous_document_render(
        store,
        &req.actor_id,
        &req.case_id,
        prior,
        Some(&receipt.content_sha256),
        "WASM_CSV_CONTEXT_INVALID",
        "Wasm execution",
    )?;
    let report = read_bound_report(store, &action, receipt, &planned)?;
    let observation = report.observation.ok_or_else(invalid_context)?;
    if report.launch_failure.is_some()
        || observation.status != "EXITED"
        || observation.guest_exit_code != Some(0)
        || observation.physical_exit_code != Some(0)
    {
        return Err(invalid_context());
    }
    if !planned
        .program
        .files
        .iter()
        .any(|declared| declared == name)
    {
        return Err(unavailable());
    }
    let mut files = observation
        .files
        .into_iter()
        .filter(|file| file.name == name);
    let file = files.next().ok_or_else(unavailable)?;
    let raw = hex_bytes(&file.bytes_hex, MAX_PROGRAM_BYTES).map_err(|_| unavailable())?;
    if files.next().is_some() || digest(&raw) != file.sha256 {
        return Err(unavailable());
    }
    let text = String::from_utf8(raw).map_err(|_| {
        failure(
            "WASM_CSV_UTF8",
            "保存したCSVの文字コードをUTF-8として確認できません。",
        )
    })?;
    Ok(RetainedCsvArtifact {
        text,
        execution_id: prior.id.clone(),
        report_ref: receipt.content_sha256.clone(),
        source_sha256: file.sha256,
    })
}

pub(crate) fn reconciliation_required(state: &CaseRevision) -> bool {
    state
        .bundle
        .as_ref()
        .is_some_and(|bundle| handles(&bundle.meaning.intent))
}

pub(crate) fn verify_committed_csv_aggregation(
    store: &Store,
    state: &CaseRevision,
    result: &minidora_domain::ResultRecord,
) -> Result<()> {
    let Some(bundle) = state.bundle.as_ref().filter(|bundle| {
        matches!(
            bundle.meaning.intent,
            Intent::ExecuteAttachedCsvAggregation { .. }
        )
    }) else {
        return Ok(());
    };
    const CODE: &str = "WASM_RECOVERY_COMPILATION";
    let mismatch = || failure(CODE, "保存したCSV隔離実行と元の依頼を照合できません。");
    let request = &bundle.request;
    let [receipt] = result.receipts.as_slice() else {
        return Err(mismatch());
    };
    let goal_ref = state.goal_sha256.as_deref().ok_or_else(mismatch)?;
    let goal: GoalContract =
        serde_json::from_slice(&store.get_bytes(&state.actor_id, &state.case_id, goal_ref)?)?;
    if request.actor_id != state.actor_id
        || request.case_id != state.case_id
        || result.id != identity(request)?
        || state.input_fingerprint.as_deref() != Some(result.id.as_str())
        || result.lifecycle != Lifecycle::Commit
        || result.guarantee != Guarantee::Computed
        || receipt.phase != EffectPhase::Confirmed
        || result.dependencies.as_slice() != [receipt.content_sha256.as_str()]
        || goal.input_fingerprint != result.id
        || goal.intent != bundle.meaning.intent
        || goal.output_path.is_some()
        || goal.permissions != request.permissions
        || goal.budget != request.budget
        || goal.required_predicates != crate::core::goal_predicates(&bundle.meaning, false, None)
        || store
            .get_input_bundle(
                &state.actor_id,
                &state.case_id,
                &result.input_id,
                result.input_version,
            )?
            .as_ref()
            .is_none_or(|(_, registered)| registered != bundle)
    {
        return Err(mismatch());
    }
    let planned = plan(bundle)?;
    let action = store
        .actions(&state.actor_id, &state.case_id)?
        .into_iter()
        .find(|candidate| candidate.action_id == receipt.action_id)
        .filter(|candidate| {
            candidate.phase == EffectPhase::Confirmed
                && candidate.receipt.as_ref() == Some(receipt)
                && action_matches_plan(candidate, &planned)
        })
        .ok_or_else(mismatch)?;
    let report = read_bound_report(store, &action, receipt, &planned)?;
    if result.text != observed_summary(&report, receipt, request.budget.max_evidence_bytes)? {
        return Err(mismatch());
    }
    crate::document::revalidate_previous_document_render(
        store,
        &state.actor_id,
        &state.case_id,
        result,
        Some(&receipt.content_sha256),
        CODE,
        "compiled CSV execution",
    )?;
    Ok(())
}

pub(crate) fn reconcile_observation(
    store: &Store,
    state: &CaseRevision,
) -> Result<minidora_domain::ResultRecord> {
    let bundle = state
        .bundle
        .as_ref()
        .ok_or_else(|| failure("WASM_RECOVERY_INPUT", "元の実行依頼がありません。"))?;
    let planned = match plan(bundle) {
        Ok(planned) => planned,
        Err(error) if state.lifecycle == minidora_domain::Lifecycle::Suspend => {
            return state.result.clone().map_or(Err(error), Ok);
        }
        Err(error) => return Err(error),
    };
    let actor = &state.actor_id;
    let case = &state.case_id;
    let Some(action) = store
        .actions(actor, case)?
        .into_iter()
        .find(|action| action.action_id == planned.action.action_id)
    else {
        return state.result.clone().ok_or_else(|| {
            failure(
                "WASM_RECOVERY_NO_ATTEMPT",
                "この入力には保存された実行試行がありません。",
            )
        });
    };
    if !action_matches_plan(&action, &planned) {
        return Err(failure(
            "WASM_ACTION_MISMATCH",
            "保存した実行作用と元の入力を照合できません。",
        ));
    }
    let mut result = state
        .result
        .clone()
        .unwrap_or_else(|| crate::core::base_result(&bundle.request));
    if let Some(receipt) = &action.receipt {
        let report = read_bound_report(store, &action, receipt, &planned)?;
        result.text = summarize(&report)?;
        result.dependencies = vec![receipt.content_sha256.clone()];
        result.receipts = vec![receipt.clone()];
    } else {
        result.text = match action.phase {
            EffectPhase::Proposed
            | EffectPhase::Authorized
            | EffectPhase::Prepared
            | EffectPhase::Cancelled => "隔離実行は試行に進んでいません。",
            _ => "隔離実行の確認記録がありません。同じ実行IDの結果は未確定です。",
        }
        .into();
    }
    result.lifecycle = minidora_domain::Lifecycle::Suspend;
    result.reason = Some("WASM_RECOVERY_OBSERVATION: 保存済みの記録を確認しました。".into());
    // This is an inspection only. A live attempt can still finish and commit.
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use minidora_domain::{GoalContract, Lifecycle, RequestEnvelope, SCHEMA_VERSION};

    #[test]
    fn pending_wasm_attempt_cannot_be_reissued_or_reconciled_as_success() {
        let temp = std::env::temp_dir().join(format!(
            "minidora-pending-wasm-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
        ));
        let store = Store::open(&temp).unwrap();
        let actor = "test";
        let case = "orphan";
        let mut request = RequestEnvelope::new(
            case,
            "first",
            actor,
            r#"WebAssemblyを隔離実行して: {"module_hex":"0061736d01000000"}"#,
        );
        request.permissions.wasm_execution = true;
        let bundle = crate::input::seal(&request, None).unwrap();
        let fingerprint = identity(&request).unwrap();
        store.register_input(actor, &request, &fingerprint).unwrap();
        let mut state = CaseRevision {
            schema_version: SCHEMA_VERSION.into(),
            case_id: case.into(),
            actor_id: actor.into(),
            revision: 0,
            parent_revision: None,
            lifecycle: Lifecycle::Received,
            goal_sha256: None,
            input_fingerprint: Some(fingerprint.clone()),
            bundle: None,
            result: None,
            reason: None,
            prior_results: Default::default(),
        };
        store.append_case(actor, &state).unwrap();
        state.revision = 1;
        state.parent_revision = Some(0);
        state.lifecycle = Lifecycle::Sealed;
        state.bundle = Some(bundle.clone());
        store.append_case(actor, &state).unwrap();
        let goal = GoalContract {
            input_fingerprint: fingerprint,
            intent: bundle.meaning.intent.clone(),
            permissions: request.permissions.clone(),
            budget: request.budget.clone(),
            output_path: None,
            required_predicates: crate::core::goal_predicates(&bundle.meaning, false, None),
            required_graph_ref: store
                .put_artifact(actor, case, &Vec::<String>::new())
                .unwrap(),
            renderer_version: None,
        };
        state.revision = 2;
        state.parent_revision = Some(1);
        state.lifecycle = Lifecycle::Running;
        state.goal_sha256 = Some(store.put_artifact(actor, case, &goal).unwrap());
        store.append_case(actor, &state).unwrap();

        // Construct the exact on-disk interruption boundary without a child.
        let mut plan = plan(&bundle).unwrap();
        store.put_bytes(actor, case, &plan.authorization).unwrap();
        store.put_bytes(actor, case, &plan.submission).unwrap();
        let deadline = Instant::now() + Duration::from_secs(2);
        for phase in [
            EffectPhase::Proposed,
            EffectPhase::Authorized,
            EffectPhase::Prepared,
            EffectPhase::Attempted,
        ] {
            plan.action.phase = phase;
            store
                .save_live_wasm_phase(actor, &plan.action, state.revision, deadline)
                .unwrap();
        }
        let duplicate = store.save_live_wasm_phase(actor, &plan.action, state.revision, deadline);
        assert!(matches!(
            duplicate,
            Err(minidora_domain::Error::Contract { code, .. }) if code == "EFFECT_ALREADY_ATTEMPTED"
        ));
        let observed = crate::recovery::reconcile(&store, actor, case).unwrap();
        assert_eq!(observed.lifecycle, Lifecycle::Suspend);
        assert!(observed.text.contains("未確定"));
        assert!(observed.receipts.is_empty());
        let actions = store.actions(actor, case).unwrap();
        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0].phase, EffectPhase::Unknown);
        assert_eq!(actions[0].action_id, plan.action.action_id);
        assert!(!temp.join("wasm-runs").exists());
        assert_eq!(
            store.load_case(actor, case).unwrap().unwrap().lifecycle,
            Lifecycle::Running
        );
    }
}
