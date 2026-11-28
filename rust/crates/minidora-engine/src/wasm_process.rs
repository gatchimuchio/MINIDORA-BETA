//! Run the fixed WebAssembly worker in a distinct, bounded OS process.
use crate::durability::is_link_or_reparse_point;
use crate::wasm_execution::ExecutionProgram;
use minidora_domain::{Result, digest, failure};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::ffi::OsString;
use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant};

const MAX_BYTES: usize = 65_536;
const MAX_FILES: usize = 4;
const RECEIPT: &str = "receipt.json";
const WORKER: &str = include_str!("wasm_guest.js");
const WATCHDOG_EXIT_CODE: i32 = 124;
// The launcher remains responsive while the trusted worker runs synchronous
// guest code. The process deadline survives termination of the Rust parent.
const WATCHDOG: &str = r#"
const { Worker } = require("node:worker_threads");
const scratch = process.argv[1];
const watchdog = setTimeout(() => process.exit(timeoutExitCode), 4000);
const worker = new Worker(guestSource, {
    eval: true,
    workerData: scratch,
    execArgv: ["--permission", "--allow-fs-write=" + scratch],
});
let failed = false;
worker.once("error", () => { failed = true; });
worker.once("exit", (code) => {
    clearTimeout(watchdog);
    process.exitCode = failed ? 3 : code;
});
"#;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ExecutionObservation {
    pub status: String,
    pub physical_exit_code: Option<i32>,
    pub guest_exit_code: Option<i32>,
    pub stdout_hex: String,
    pub stderr_hex: String,
    pub files: Vec<ExecutionFile>,
    pub detail: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ExecutionFile {
    pub name: String,
    pub bytes_hex: String,
    pub sha256: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WorkerReceipt {
    status: String,
    guest_exit_code: Option<i32>,
    files: Vec<String>,
    detail: Option<String>,
}

fn io_detail(message: &str, error: &io::Error) -> String {
    format!("{message} (OS {:?})", error.raw_os_error())
}

fn checked_scratch(path: &Path) -> Result<PathBuf> {
    let error = || {
        failure(
            "WASM_EXECUTION_SCRATCH_INVALID",
            "実行専用の空の作業フォルダーを照合できません。",
        )
    };
    let metadata = fs::symlink_metadata(path).map_err(|_| error())?;
    if !path.is_absolute() || !metadata.is_dir() || is_link_or_reparse_point(&metadata) {
        return Err(error());
    }
    let actual = fs::canonicalize(path).map_err(|_| error())?;
    if fs::read_dir(&actual).map_err(|_| error())?.next().is_some() {
        return Err(error());
    }
    let worker = worker_path(&actual).ok_or_else(error)?;
    if worker
        .to_str()
        .is_none_or(|path| path.contains(['*', '?', ',']))
        || fs::canonicalize(&worker).map_err(|_| error())? != actual
    {
        return Err(error());
    }
    Ok(worker)
}

#[cfg(windows)]
fn worker_path(path: &Path) -> Option<PathBuf> {
    use std::path::{Component, Prefix};
    let mut parts = path.components();
    let Component::Prefix(prefix) = parts.next()? else {
        return None;
    };
    match prefix.kind() {
        Prefix::Disk(_) => Some(path.into()),
        Prefix::VerbatimDisk(drive) => {
            if parts.next()? != Component::RootDir {
                return None;
            }
            // Node's permission resolver accepts a normal drive path here.
            let mut result = PathBuf::from(format!("{}:\\", char::from(drive)));
            result.extend(parts);
            Some(result)
        }
        _ => None,
    }
}

#[cfg(not(windows))]
fn worker_path(path: &Path) -> Option<PathBuf> {
    Some(path.into())
}

fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut result = String::with_capacity(bytes.len() * 2);
    for &byte in bytes {
        result.push(DIGITS[(byte >> 4) as usize] as char);
        result.push(DIGITS[(byte & 15) as usize] as char);
    }
    result
}

fn capture(mut pipe: impl Read, over_limit: &AtomicBool) -> (Vec<u8>, bool) {
    let mut buffer = [0; 4096];
    let mut captured = Vec::new();
    loop {
        match pipe.read(&mut buffer) {
            Ok(0) => return (captured, true),
            Ok(read) => {
                let keep = read.min(MAX_BYTES - captured.len());
                captured.extend_from_slice(&buffer[..keep]);
                if keep != read {
                    over_limit.store(true, Ordering::Relaxed);
                    return (captured, false);
                }
            }
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(_) => return (captured, false),
        }
    }
}

fn read_bounded_file(path: &Path, max: usize) -> io::Result<Vec<u8>> {
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.is_file() || is_link_or_reparse_point(&metadata) || metadata.len() > max as u64 {
        return Err(io::Error::other("unexpected file type or size"));
    }
    let mut bytes = Vec::new();
    File::open(path)?
        .take((max + 1) as u64)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 != metadata.len() || bytes.len() > max {
        return Err(io::Error::other("file length changed"));
    }
    Ok(bytes)
}

fn checked_receipt(
    scratch: &Path,
    allowed: &[String],
    actual: ExitStatus,
) -> io::Result<(WorkerReceipt, Vec<ExecutionFile>)> {
    let invalid = || io::Error::other("invalid worker receipt");
    let raw = read_bounded_file(&scratch.join(RECEIPT), 4096)?;
    let receipt: WorkerReceipt = serde_json::from_slice(&raw).map_err(|_| invalid())?;
    let valid_exit = match (&*receipt.status, receipt.guest_exit_code) {
        ("EXITED", Some(code @ 0..=255)) => actual.code() == Some(code),
        ("TRAP", None) => actual.code() == Some(1),
        ("REFUSED", None) => actual.code() == Some(2) && receipt.files.is_empty(),
        _ => false,
    };
    let names: BTreeSet<_> = receipt.files.iter().map(String::as_str).collect();
    if !valid_exit
        || names.len() != receipt.files.len()
        || receipt.files.len() > MAX_FILES
        || receipt.files.iter().any(|name| !allowed.contains(name))
    {
        return Err(invalid());
    }
    let mut entries = BTreeSet::new();
    for entry in fs::read_dir(scratch)? {
        if entries.len() > MAX_FILES {
            return Err(invalid());
        }
        entries.insert(entry?.file_name().into_string().map_err(|_| invalid())?);
    }
    if !entries.remove(RECEIPT)
        || entries.iter().map(String::as_str).collect::<BTreeSet<_>>() != names
    {
        return Err(invalid());
    }
    let files = receipt
        .files
        .iter()
        .map(|name| {
            let bytes = read_bounded_file(&scratch.join(name), MAX_BYTES)?;
            Ok(ExecutionFile {
                name: name.clone(),
                bytes_hex: hex(&bytes),
                sha256: digest(&bytes),
            })
        })
        .collect::<io::Result<_>>()?;
    Ok((receipt, files))
}

pub(crate) fn run(
    runtime: &Path,
    scratch: &Path,
    program: &ExecutionProgram,
    deadline: Instant,
) -> Result<ExecutionObservation> {
    let scratch = checked_scratch(scratch)?;
    let runtime_ok = runtime.is_absolute()
        && fs::symlink_metadata(runtime)
            .is_ok_and(|m| m.is_file() && !is_link_or_reparse_point(&m));
    if !runtime_ok {
        return Err(failure(
            "WASM_RUNTIME_INVALID",
            "指定された実行環境の実ファイルを照合できません。",
        ));
    }
    if program.module_hex.len() > MAX_BYTES * 2
        || program.stdin_hex.len() > MAX_BYTES * 2
        || program.files.len() > MAX_FILES
        || program
            .files
            .iter()
            .any(|name| !crate::wasm_execution::artifact_name(name))
        || program
            .files
            .iter()
            .map(|name| name.to_ascii_lowercase())
            .collect::<BTreeSet<_>>()
            .len()
            != program.files.len()
    {
        return Err(failure(
            "WASM_EXECUTION_LIMIT",
            "実行データまたは成果物の指定が上限を超えています。",
        ));
    }
    let input = serde_json::to_vec(program)?;
    let launcher = format!(
        "const guestSource = {};\nconst timeoutExitCode = {WATCHDOG_EXIT_CODE};\n{WATCHDOG}",
        serde_json::to_string(WORKER)?,
    );
    if Instant::now() >= deadline {
        return Err(failure(
            "WASM_EXECUTION_DEADLINE",
            "実行を始める前に期限に達しました。",
        ));
    }
    let mut permit = OsString::from("--allow-fs-write=");
    permit.push(&scratch);
    let mut command = Command::new(runtime);
    command
        .current_dir(&scratch)
        .env_clear()
        .args([
            "--permission",
            "--allow-worker",
            "--disable-warning=SecurityWarning",
        ])
        .arg(permit)
        .args([
            "--max-old-space-size=48",
            "--max-semi-space-size=4",
            "--wasm-max-mem-pages=128",
            "--wasm-max-table-size=256",
            "--wasm-max-module-size=65536",
            "--wasm-num-compilation-tasks=1",
            "--no-experimental-wasm-multi-memory",
            "--no-experimental-wasm-shared",
            "-e",
        ])
        .arg(launcher)
        .arg("--")
        .arg(&scratch)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let system_root = std::env::var_os("SystemRoot")
            .filter(|root| {
                let root = Path::new(root);
                root.is_absolute() && root.join("System32").is_dir()
            })
            .ok_or_else(|| {
                failure(
                    "WASM_RUNTIME_START_FAILED",
                    "Windows実行環境のSystemRootを確認できません。",
                )
            })?;
        // Windows crypto initialization needs this OS locator. Other
        // environment variables remain absent, and the guest has no env API.
        command
            .env("SystemRoot", system_root)
            .creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    let mut child = command.spawn().map_err(|error| {
        failure(
            "WASM_RUNTIME_START_FAILED",
            io_detail("分離実行プロセスを開始できません。", &error),
        )
    })?;
    let mut stdin = child.stdin.take().expect("configured stdin pipe");
    let stdout = child.stdout.take().expect("configured stdout pipe");
    let stderr = child.stderr.take().expect("configured stderr pipe");
    let over_limit = Arc::new(AtomicBool::new(false));
    let out_limit = Arc::clone(&over_limit);
    let err_limit = Arc::clone(&over_limit);
    let reader_out = thread::spawn(move || capture(stdout, &out_limit));
    let reader_err = thread::spawn(move || capture(stderr, &err_limit));
    let writer = thread::spawn(move || stdin.write_all(&input));

    let (forced, actual) = loop {
        match child.try_wait() {
            Ok(Some(exit)) => break (None, Ok(exit)),
            Ok(None) => {}
            Err(error) => break (Some("UNCONFIRMED"), Err(error)),
        }
        let reason = if over_limit.load(Ordering::Relaxed) {
            "OUTPUT_LIMIT"
        } else if Instant::now() >= deadline {
            "TIMEOUT"
        } else {
            thread::sleep(Duration::from_millis(5));
            continue;
        };
        break (Some(reason), Err(io::Error::other(reason)));
    };
    let actual = if actual.is_err() {
        let kill_failure = child.kill().err();
        let reap_deadline = Instant::now() + Duration::from_millis(750);
        loop {
            match child.try_wait() {
                Ok(Some(status)) => break Ok(status),
                Ok(None) if Instant::now() < reap_deadline => {
                    thread::sleep(Duration::from_millis(5));
                }
                Ok(None) => {
                    break Err(kill_failure.unwrap_or_else(|| io::Error::other("reap timeout")));
                }
                Err(error) => break Err(error),
            }
        }
    } else {
        actual
    };
    let actual = match actual {
        Ok(status) => status,
        Err(error) => {
            return Ok(ExecutionObservation {
                status: "UNCONFIRMED".into(),
                physical_exit_code: None,
                guest_exit_code: None,
                stdout_hex: String::new(),
                stderr_hex: String::new(),
                files: Vec::new(),
                detail: Some(io_detail(
                    "分離実行プロセスの終了を確認できません。",
                    &error,
                )),
            });
        }
    };
    let input_complete = writer.join().is_ok_and(|result| result.is_ok());
    let (out, out_complete) = reader_out.join().unwrap_or_default();
    let (err, err_complete) = reader_err.join().unwrap_or_default();
    let mut observation = ExecutionObservation {
        status: "UNCONFIRMED".into(),
        physical_exit_code: actual.code(),
        guest_exit_code: None,
        stdout_hex: hex(&out),
        stderr_hex: hex(&err),
        files: Vec::new(),
        detail: None,
    };
    if let Some(reason) = forced {
        observation.status = reason.into();
    } else if over_limit.load(Ordering::Relaxed) {
        observation.status = "OUTPUT_LIMIT".into();
    } else {
        let exchanged = input_complete && out_complete && err_complete;
        match checked_receipt(&scratch, &program.files, actual) {
            Ok((receipt, files)) if exchanged => {
                observation.status = receipt.status;
                observation.guest_exit_code = receipt.guest_exit_code;
                observation.files = files;
                observation.detail = receipt.detail;
            }
            Err(_) if actual.code() == Some(WATCHDOG_EXIT_CODE) => {
                // A guest may return this same number. Without the receipt,
                // keep the physical exit code but do not attribute its cause.
                observation.detail =
                    Some("プロセスの終了コードは124です。終了理由の受領を照合できません。".into());
            }
            Err(_) if exchanged => {
                observation.detail =
                    Some("実行結果と作業フォルダーの成果物を照合できません。".into());
            }
            _ => {
                observation.detail = Some("実行プロセスとのデータ交換を確認できません。".into());
            }
        }
    }
    Ok(observation)
}
