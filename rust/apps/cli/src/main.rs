//! Thin local frontends: interpretation, execution and judgment remain in Engine.
#![forbid(unsafe_code)]

mod attachment;
mod case_http;
mod chat;
mod microphone;

use minidora_domain::{
    ActionRecord, EffectPhase, Lifecycle, MAX_REQUEST_ATTACHMENTS, PurposeUse, RequestAttachment,
    RequestEnvelope, ResourceBudget, ResultRecord, SCHEMA_VERSION,
};
use minidora_engine::{Engine, store::Store};
use std::collections::{BTreeMap, BTreeSet};
use std::io::{self, BufRead, BufReader, IsTerminal, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const HELP: &str = concat!(
    "MINIDORA Rust ",
    env!("CARGO_PKG_VERSION"),
    r#" — local execution frontend

Usage:
  minidora-rust [run] [OPTIONS] [--] REQUEST...
  minidora-rust chat [--db DIRECTORY] [--actor ID] [--case ID] [--allow-read-host HOST]...
    [--allow-write EXACT_PATH]... [--wasm-node ABSOLUTE_PATH] [--speech-assets ABSOLUTE_DIR]
    [--asr-assets ABSOLUTE_DIR] [--ocr-assets ABSOLUTE_DIR]
  minidora-rust capabilities [--json]
  minidora-rust inspect [--db DIRECTORY] [--case ID] [--actor ID] [--input ID [--version N]] [--json]
  minidora-rust migrate --db DIRECTORY [--json]
  minidora-rust reconcile [--db DIRECTORY] [--case ID] [--actor ID] [--json]
  minidora-rust reconcile-effects [--db DIRECTORY] [--case ID] [--actor ID] [--json]
  minidora-rust serve [OPTIONS] [--bind 127.0.0.1:8080]

Options:
  --db DIRECTORY             Durable state directory (default .minidora-rust-state)
  --case ID                  Case identity (default default)
  --input ID                 Exact input identity; run creates one when omitted
  --context RESULT_ID        Reference an exact prior result in the same case (run only)
  --version N                Input version (default 1)
  --actor ID                 Trusted local frontend identity
  --json                     Print the full result or case revision as JSON
  --allow-read-host HOST      Permit one exact read destination; repeatable
  --attach EXACT_PATH        Read one local PDF, XLSX, PNG, JPEG, or UTF-8 .csv, .txt, or .md (Windows run; repeatable)
  --allow-write EXACT_PATH    Permit one exact output path; repeatable
  --wasm-node ABSOLUTE_PATH   Configure the trusted Node executable for local WebAssembly
  --speech-assets ABSOLUTE_DIR  Configure local Japanese WAV generation (run, chat, serve)
                              Include naist-jdic/ and nitech_jp_atr503_m001.htsvoice
  --asr-assets ABSOLUTE_DIR     Configure local Japanese recognition (chat, serve)
  --ocr-assets ABSOLUTE_DIR     Configure local image text recognition (run, chat, serve)
  --allow-wasm-execution      Permit WebAssembly execution for this request (run only)
  --output EXACT_PATH        Request an output file; does not grant write permission
  --max-actions N            Finite action admission budget
  --deadline-ms N            Finite execution deadline
  --max-input-bytes N        Input admission limit
  --max-evidence-bytes N     New evidence admission limit
  --help                     Show this help

Without text arguments, run reads bounded UTF-8 text from redirected stdin.
Use -- before request text beginning with a dash. Retries with an explicit
(case,input,version) reuse the durable result or report an identity conflict.
Chat reads one turn at a time from stdin. Type /help inside chat for session,
multiline input and network controls. With --wasm-node configured, enter
/allow-exec directly before the WebAssembly request to grant that single turn.
Use /attach EXACT_PATH to select a local file for the next chat request.
Use /listen EXACT_PATH to submit one spoken turn from a local WAV file.
Capabilities prints the engine's version and implemented capabilities as JSON.

Inspect emits the latest case revision as JSON. With --input ID it reads the
saved result for that exact input and version. A successful lookup exits 0,
including null when no record is saved. An invalid or denied lookup exits 1.

Actor identity comes from --actor, MINIDORA_ACTOR, or the local USER/USERNAME.
These are trusted local configuration, not remote authentication. State directory
access must be protected by the host. Development budgets are finite admission
defaults, not measured performance or Windows/8GB guarantees.

Migrate explicitly accepts an existing alpha 7, alpha 8, alpha 9 or alpha 10 state directory,
verifies it, then updates only its schema metadata to the current schema. It
requires --db and also accepts an already-migrated current directory as a
verified no-op.

Reconcile asks Core to inspect the original saved operation and actual file.
It accepts no new goal, permissions or target, and performs no write or resend.
Reconcile-effects inspects pending operations only for a cancelled case. It
updates the saved action ledger from the original local file and preserves
case CANCEL. Exit 2 means an effect still requires reconciliation; exit 0 means
all recorded effects are confirmed, failed or cancelled. Plain output reports
saved receipt reasons on stderr. With --json the reasons remain in the records.

Serve binds only a loopback IP and requires MINIDORA_API_TOKEN (at least 32 bytes).
GET / serves a small same-origin browser workspace; GET /health reports the
schema. POST /v1/execute and POST /v1/execute/stream require Authorization:
Bearer TOKEN and a full RequestEnvelope JSON object. The stream route requires
Accept: text/event-stream and emits versioned provisional/invalidation/final
events; only the final Core result carries answer content. The configured server
actor is the only authenticated identity. Request permissions must be subsets of
--allow-* grants; budgets are capped to server limits. WebAssembly execution is
available through local run and chat. HTTP is local plaintext only.

Exit: COMMIT 0; SUSPEND 2; REJECT 3; FAIL 4; CANCEL 5; frontend error 1.
"#,
);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Mode {
    Run,
    Chat,
    Capabilities,
    Inspect,
    Migrate,
    Reconcile,
    ReconcileEffects,
    Serve,
    Help,
}

#[derive(Debug)]
struct Options {
    mode: Mode,
    db: PathBuf,
    case_id: String,
    input_id: Option<String>,
    context_ref: Option<String>,
    input_version: u64,
    actor: String,
    json: bool,
    read_hosts: BTreeSet<String>,
    write_paths: BTreeSet<PathBuf>,
    wasm_node: Option<PathBuf>,
    speech_assets: Option<PathBuf>,
    asr_assets: Option<PathBuf>,
    ocr_assets: Option<PathBuf>,
    allow_wasm_execution: bool,
    attachment_paths: Vec<PathBuf>,
    output: Option<PathBuf>,
    budget: ResourceBudget,
    bind: SocketAddr,
    text: Vec<String>,
}

fn default_actor() -> String {
    std::env::var("MINIDORA_ACTOR")
        .ok()
        .filter(|name| !name.trim().is_empty())
        .unwrap_or_else(|| {
            let name = std::env::var("USER")
                .or_else(|_| std::env::var("USERNAME"))
                .unwrap_or_else(|_| "local".into());
            format!("local:{name}")
        })
}

fn next_value(args: &mut impl Iterator<Item = String>, flag: &str) -> Result<String, String> {
    args.next()
        .ok_or_else(|| format!("{flag} requires a value"))
}

fn number<T: std::str::FromStr>(text: String, flag: &str) -> Result<T, String> {
    if text.is_empty() || !text.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(format!("{flag} requires an unsigned integer"));
    }
    text.parse().map_err(|_| format!("{flag} is out of range"))
}

fn parse(args: impl IntoIterator<Item = String>) -> Result<Options, String> {
    let mut options = Options {
        mode: Mode::Run,
        db: ".minidora-rust-state".into(),
        case_id: "default".into(),
        input_id: None,
        context_ref: None,
        input_version: 1,
        actor: default_actor(),
        json: false,
        read_hosts: BTreeSet::new(),
        write_paths: BTreeSet::new(),
        wasm_node: None,
        speech_assets: None,
        asr_assets: None,
        ocr_assets: None,
        allow_wasm_execution: false,
        attachment_paths: Vec::new(),
        output: None,
        budget: ResourceBudget::development(),
        bind: "127.0.0.1:8080".parse().expect("literal socket address"),
        text: Vec::new(),
    };
    let mut explicit_db = false;
    let mut explicit_input_version = false;
    let mut args = args.into_iter().peekable();
    match args.peek().map(String::as_str) {
        Some("run") => {
            args.next();
        }
        Some("chat") => {
            options.mode = Mode::Chat;
            args.next();
        }
        Some("capabilities") => {
            options.mode = Mode::Capabilities;
            args.next();
        }
        Some("inspect") => {
            options.mode = Mode::Inspect;
            args.next();
        }
        Some("migrate") => {
            options.mode = Mode::Migrate;
            args.next();
        }
        Some("reconcile") => {
            options.mode = Mode::Reconcile;
            args.next();
        }
        Some("reconcile-effects") => {
            options.mode = Mode::ReconcileEffects;
            args.next();
        }
        Some("serve") => {
            options.mode = Mode::Serve;
            args.next();
        }
        _ => {}
    }
    while let Some(arg) = args.next() {
        if options.mode == Mode::Capabilities && !matches!(arg.as_str(), "--json" | "--help" | "-h")
        {
            return Err("capabilities accepts only --json and --help".into());
        }
        if options.mode == Mode::Chat
            && !matches!(
                arg.as_str(),
                "--db"
                    | "--case"
                    | "--actor"
                    | "--allow-read-host"
                    | "--allow-write"
                    | "--wasm-node"
                    | "--speech-assets"
                    | "--asr-assets"
                    | "--ocr-assets"
                    | "--max-actions"
                    | "--deadline-ms"
                    | "--max-input-bytes"
                    | "--max-evidence-bytes"
                    | "--help"
                    | "-h"
            )
        {
            return Err(format!("chat does not accept {arg}; use chat --help"));
        }
        if matches!(options.mode, Mode::Reconcile | Mode::ReconcileEffects)
            && !matches!(
                arg.as_str(),
                "--db" | "--case" | "--actor" | "--json" | "--help" | "-h"
            )
        {
            return Err("reconciliation accepts only --db, --case, --actor and --json; saved operations cannot gain a new goal or permission".into());
        }
        if options.mode == Mode::Migrate
            && !matches!(arg.as_str(), "--db" | "--json" | "--help" | "-h")
        {
            return Err("migration accepts only --db and --json; it does not accept a request, actor or effect permission".into());
        }
        match arg.as_str() {
            "--" => {
                options.text.extend(args);
                break;
            }
            "--help" | "-h" => options.mode = Mode::Help,
            "--db" => {
                explicit_db = true;
                options.db = next_value(&mut args, &arg)?.into();
            }
            "--case" => options.case_id = next_value(&mut args, &arg)?,
            "--input" => options.input_id = Some(next_value(&mut args, &arg)?),
            "--context" => options.context_ref = Some(next_value(&mut args, &arg)?),
            "--version" => {
                options.input_version = number(next_value(&mut args, &arg)?, &arg)?;
                explicit_input_version = true;
            }
            "--actor" => options.actor = next_value(&mut args, &arg)?,
            "--json" => options.json = true,
            "--allow-read-host" => {
                options.read_hosts.insert(next_value(&mut args, &arg)?);
            }
            "--allow-write" => {
                options
                    .write_paths
                    .insert(next_value(&mut args, &arg)?.into());
            }
            "--wasm-node" => {
                if !matches!(options.mode, Mode::Run | Mode::Chat) {
                    return Err("--wasm-node is only accepted by local run and chat".into());
                }
                let path = PathBuf::from(next_value(&mut args, &arg)?);
                if !path.is_absolute() || options.wasm_node.is_some() {
                    return Err("--wasm-node requires one absolute executable path".into());
                }
                options.wasm_node = Some(path);
            }
            "--speech-assets" => {
                if !matches!(options.mode, Mode::Run | Mode::Chat | Mode::Serve) {
                    return Err("--speech-assets is only accepted by run, chat, and serve".into());
                }
                let path = PathBuf::from(next_value(&mut args, &arg)?);
                if !path.is_absolute() || options.speech_assets.is_some() {
                    return Err("--speech-assets requires one absolute directory path".into());
                }
                options.speech_assets = Some(path);
            }
            "--asr-assets" => {
                if !matches!(options.mode, Mode::Chat | Mode::Serve) {
                    return Err("--asr-assets is only accepted by chat and serve".into());
                }
                let path = PathBuf::from(next_value(&mut args, &arg)?);
                if !path.is_absolute() || options.asr_assets.is_some() {
                    return Err("--asr-assets requires one absolute directory path".into());
                }
                options.asr_assets = Some(path);
            }
            "--ocr-assets" => {
                if !matches!(options.mode, Mode::Run | Mode::Chat | Mode::Serve) {
                    return Err("--ocr-assets is only accepted by run, chat, and serve".into());
                }
                let path = PathBuf::from(next_value(&mut args, &arg)?);
                if !path.is_absolute() || options.ocr_assets.is_some() {
                    return Err("--ocr-assets requires one absolute directory path".into());
                }
                options.ocr_assets = Some(path);
            }
            "--allow-wasm-execution" => {
                if options.mode != Mode::Run {
                    return Err("--allow-wasm-execution is only accepted by local run".into());
                }
                options.allow_wasm_execution = true;
            }
            "--attach" => {
                if options.mode != Mode::Run {
                    return Err(
                        "--attach is only accepted by local run; use /attach in chat".into(),
                    );
                }
                if options.attachment_paths.len() == MAX_REQUEST_ATTACHMENTS {
                    return Err(format!(
                        "--attach accepts at most {MAX_REQUEST_ATTACHMENTS} files"
                    ));
                }
                options
                    .attachment_paths
                    .push(next_value(&mut args, &arg)?.into());
            }
            "--output" => options.output = Some(next_value(&mut args, &arg)?.into()),
            "--max-actions" => {
                options.budget.max_actions = number(next_value(&mut args, &arg)?, &arg)?
            }
            "--deadline-ms" => {
                options.budget.deadline_ms = number(next_value(&mut args, &arg)?, &arg)?
            }
            "--max-input-bytes" => {
                options.budget.max_input_bytes = number(next_value(&mut args, &arg)?, &arg)?
            }
            "--max-evidence-bytes" => {
                options.budget.max_evidence_bytes = number(next_value(&mut args, &arg)?, &arg)?
            }
            "--bind" => {
                options.bind = next_value(&mut args, &arg)?
                    .parse()
                    .map_err(|_| "--bind requires a numeric loopback IP:port".to_owned())?
            }
            value if value.starts_with('-') => {
                return Err(format!(
                    "unknown option {value}; use -- before request text"
                ));
            }
            _ => options.text.push(arg),
        }
    }
    if options.actor.trim().is_empty()
        || options.case_id.trim().is_empty()
        || options
            .input_id
            .as_ref()
            .is_some_and(|value| value.trim().is_empty())
    {
        return Err("actor, case and input identities must be nonempty".into());
    }
    if options.mode == Mode::Migrate && !explicit_db {
        return Err("migration requires an explicit --db DIRECTORY".into());
    }
    if options.mode == Mode::Inspect && explicit_input_version && options.input_id.is_none() {
        return Err("inspect --version requires --input ID".into());
    }
    if options.mode != Mode::Run && !options.text.is_empty() {
        return Err("only run accepts request text".into());
    }
    if let Some(context) = &options.context_ref
        && (options.mode != Mode::Run || context.trim().is_empty())
    {
        return Err(
            "--context requires a nonempty result identity and is only accepted by run".into(),
        );
    }
    if !options.bind.ip().is_loopback() {
        return Err("serve requires a loopback bind address".into());
    }
    Ok(options)
}

fn terminal_exit(lifecycle: Lifecycle) -> u8 {
    match lifecycle {
        Lifecycle::Commit => 0,
        Lifecycle::Suspend => 2,
        Lifecycle::Reject => 3,
        Lifecycle::Fail => 4,
        Lifecycle::Cancel => 5,
        // A frontend must not treat a nonterminal response as goal completion.
        _ => 4,
    }
}

fn print_result(result: &ResultRecord, json: bool) -> Result<(), String> {
    let stdout = io::stdout();
    let mut output = stdout.lock();
    if json {
        serde_json::to_writer(&mut output, result).map_err(|error| error.to_string())?;
        writeln!(output).map_err(|error| error.to_string())?;
    } else {
        writeln!(output, "{}", result.text).map_err(|error| error.to_string())?;
        let state = serde_json::to_string(&result.lifecycle).map_err(|error| error.to_string())?;
        writeln!(
            output,
            "terminal={} result={}",
            state.trim_matches('"'),
            result.id
        )
        .map_err(|error| error.to_string())?;
        if let Some(reason) = &result.reason {
            writeln!(output, "reason={reason}").map_err(|error| error.to_string())?;
        }
        for receipt in &result.receipts {
            chat::show_receipt(receipt);
        }
    }
    Ok(())
}

fn run(options: Options) -> Result<u8, String> {
    if options.mode == Mode::Help {
        print!("{HELP}");
        return Ok(0);
    }
    if options.mode == Mode::Capabilities {
        let mut output = io::stdout().lock();
        serde_json::to_writer(&mut output, &Engine::capabilities())
            .map_err(|error| error.to_string())?;
        writeln!(output).map_err(|error| error.to_string())?;
        return Ok(0);
    }
    if options.mode == Mode::Migrate {
        let (from_schema, to_schema) =
            Store::migrate_to_current(&options.db).map_err(|error| error.to_string())?;
        let migrated = from_schema != to_schema;
        let report = serde_json::json!({
            "from_schema": from_schema,
            "to_schema": to_schema,
            "migrated": migrated,
        });
        if options.json {
            println!(
                "{}",
                serde_json::to_string(&report).map_err(|error| error.to_string())?
            );
        } else if migrated {
            println!(
                "store metadata migrated from {from_schema} to {to_schema}; immutable case and effect records were retained"
            );
        } else {
            println!("store already uses {SCHEMA_VERSION}; no migration was needed");
        }
        return Ok(0);
    }
    if options.mode == Mode::Run {
        return run_request(options);
    }
    let mut engine = open_engine(&options)?;
    if options.mode == Mode::Chat {
        return chat::run(&mut engine, options);
    }
    if options.mode == Mode::ReconcileEffects {
        let actions = engine
            .reconcile_cancelled_effects(&options.actor, &options.case_id)
            .map_err(|error| error.to_string())?;
        print_actions(&actions, options.json)?;
        return Ok(
            if actions.iter().all(|action| {
                matches!(
                    action.phase,
                    EffectPhase::Confirmed | EffectPhase::Failed | EffectPhase::Cancelled
                )
            }) {
                0
            } else {
                2
            },
        );
    }
    if options.mode == Mode::Reconcile {
        let result = engine
            .reconcile(&options.actor, &options.case_id)
            .map_err(|error| error.to_string())?;
        print_result(&result, options.json)?;
        return Ok(terminal_exit(result.lifecycle));
    }
    if options.mode == Mode::Inspect {
        let stdout = io::stdout();
        let mut output = stdout.lock();
        if let Some(input) = &options.input_id {
            let result = engine
                .input_result(
                    &options.actor,
                    &options.case_id,
                    input,
                    options.input_version,
                )
                .map_err(|error| error.to_string())?;
            serde_json::to_writer(&mut output, &result)
        } else {
            let revision = engine
                .latest(&options.actor, &options.case_id)
                .map_err(|error| error.to_string())?;
            serde_json::to_writer(&mut output, &revision)
        }
        .map_err(|error| error.to_string())?;
        writeln!(output).map_err(|error| error.to_string())?;
        return Ok(0);
    }
    serve(&mut engine, &options)
}

fn run_request(options: Options) -> Result<u8, String> {
    let text = if options.text.is_empty() {
        if io::stdin().is_terminal() {
            return Err("supply request text or redirect UTF-8 stdin; see --help".into());
        }
        let mut bytes = Vec::new();
        let limit = u64::try_from(options.budget.max_input_bytes)
            .unwrap_or(u64::MAX)
            .saturating_add(1);
        io::stdin()
            .take(limit)
            .read_to_end(&mut bytes)
            .map_err(|error| error.to_string())?;
        if bytes.len() > options.budget.max_input_bytes {
            return Err("stdin exceeds --max-input-bytes; no input was executed".into());
        }
        String::from_utf8(bytes)
            .map_err(|error| format!("stdin is not valid UTF-8: {}", error.utf8_error()))?
    } else {
        options.text.join(" ")
    };
    if text.trim().is_empty() {
        return Err("request text is empty".into());
    }
    let mut attachments = Vec::new();
    let attachment_budget = options.budget.max_input_bytes.saturating_sub(text.len());
    for path in &options.attachment_paths {
        attachment::stage(&mut attachments, path, attachment_budget)?;
    }
    let mut engine = open_engine(&options)?;
    let result = execute_request(
        &mut engine,
        &options,
        RequestInput::Text(&text),
        &options.read_hosts,
        options.allow_wasm_execution,
        attachments,
        None,
    )?;
    print_result(&result, options.json)?;
    Ok(terminal_exit(result.lifecycle))
}

fn open_engine(options: &Options) -> Result<Engine, String> {
    let mut engine = Engine::open(&options.db).map_err(|error| error.to_string())?;
    if let Some(path) = &options.wasm_node {
        engine
            .configure_wasm_node(path.clone())
            .map_err(|error| error.to_string())?;
    }
    if let Some(path) = &options.speech_assets {
        let dictionary = path.join("naist-jdic");
        let voice = path.join("nitech_jp_atr503_m001.htsvoice");
        minidora_engine::configure_japanese_speech(&dictionary, &voice)
            .map_err(|error| error.to_string())?;
    }
    if let Some(path) = &options.asr_assets {
        minidora_engine::configure_japanese_recognition(path).map_err(|error| error.to_string())?;
    }
    if let Some(path) = &options.ocr_assets {
        minidora_engine::configure_image_ocr(path).map_err(|error| error.to_string())?;
    }
    Ok(engine)
}

#[derive(Clone, Copy)]
enum RequestInput<'a> {
    Text(&'a str),
    Speech(&'a Path),
    SpeechBytes(&'a [u8]),
}

fn execute_request(
    engine: &mut Engine,
    options: &Options,
    input: RequestInput<'_>,
    read_hosts: &BTreeSet<String>,
    allow_wasm_execution: bool,
    attachments: Vec<RequestAttachment>,
    purpose: Option<PurposeUse>,
) -> Result<ResultRecord, String> {
    let input_id = match &options.input_id {
        Some(id) => id.clone(),
        None => {
            let started = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_err(|error| error.to_string())?;
            format!("run-{:x}-{:x}", started.as_nanos(), std::process::id())
        }
    };
    let mut request = RequestEnvelope::new(
        &options.case_id,
        &input_id,
        &options.actor,
        match input {
            RequestInput::Text(text) => text,
            RequestInput::Speech(_) | RequestInput::SpeechBytes(_) => "",
        },
    );
    request.input_version = options.input_version;
    request.context_ref = options.context_ref.clone();
    request.permissions.write_paths = options.write_paths.clone();
    request.output_path = options.output.clone();
    request.budget = options.budget.clone();
    request.purpose = purpose.map(Box::new);
    let speech = match input {
        RequestInput::Text(_) => {
            request.attachments = attachments;
            request.permissions.read_hosts = read_hosts.clone();
            request.permissions.wasm_execution = allow_wasm_execution;
            return engine
                .execute(&options.actor, request)
                .map_err(|error| error.to_string());
        }
        RequestInput::Speech(path) => engine.execute_wave_file(&options.actor, request, path),
        RequestInput::SpeechBytes(wave) => engine
            .execute_wave_bytes(&options.actor, request, wave)
            .map(|(recognition, result)| (recognition.text, result)),
    };
    speech
        .map(|(transcript, result)| {
            eprintln!("認識した発話: {transcript}");
            result
        })
        .map_err(|error| error.to_string())
}

fn print_actions(actions: &[ActionRecord], json: bool) -> Result<(), String> {
    let stdout = io::stdout();
    let mut output = stdout.lock();
    if json {
        serde_json::to_writer(&mut output, actions).map_err(|error| error.to_string())?;
        writeln!(output).map_err(|error| error.to_string())?;
    } else {
        let stderr = io::stderr();
        let mut diagnostics = stderr.lock();
        for action in actions {
            let phase = serde_json::to_string(&action.phase).map_err(|error| error.to_string())?;
            writeln!(
                output,
                "action={} phase={} target={}",
                action.action_id,
                phase.trim_matches('"'),
                action.target
            )
            .map_err(|error| error.to_string())?;
            if let Some(reason) = action
                .receipt
                .as_ref()
                .and_then(|receipt| receipt.reason.as_deref())
            {
                // A diagnostic does not change the reconciliation status.
                let _ = writeln!(diagnostics, "action={} reason={reason:?}", action.action_id);
            }
        }
        writeln!(
            output,
            "case remains CANCEL; no write or resend was performed"
        )
        .map_err(|error| error.to_string())?;
    }
    Ok(())
}

fn main() -> ExitCode {
    #[cfg(windows)]
    {
        let mut args = std::env::args_os().skip(1);
        if args.next().as_deref() == Some(std::ffi::OsStr::new("--minidora-internal-pdf-reader-v1"))
            && args.next().is_none()
        {
            if minidora_engine::run_pdf_worker_stdio().is_ok() {
                return ExitCode::SUCCESS;
            }
            eprintln!("minidora-rust: PDF文書の処理を完了できませんでした。");
            return ExitCode::FAILURE;
        }
        if let Ok(executable) = std::env::current_exe() {
            let _ = minidora_engine::configure_pdf_worker(executable);
        }
    }
    match parse(std::env::args().skip(1)).and_then(run) {
        Ok(code) => ExitCode::from(code),
        Err(message) => {
            eprintln!("minidora-rust: {message}");
            ExitCode::from(1)
        }
    }
}

struct HttpRequest {
    method: String,
    target: String,
    headers: BTreeMap<String, String>,
    body: Vec<u8>,
}

struct AdmissionReader<'a> {
    stream: &'a mut TcpStream,
    deadline: Instant,
}

impl Read for AdmissionReader<'_> {
    fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
        let remaining = self.deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "request admission deadline",
            ));
        }
        self.stream.set_read_timeout(Some(remaining))?;
        self.stream.read(bytes)
    }
}

#[derive(Debug)]
struct HttpError(u16, &'static str);

const STREAM_API_VERSION: &str = "minidora-stream/v1";
const WAVE_STREAM_ROUTE: &str = "/v1/execute/wave/stream";
const READ_HOSTS_ROUTE: &str = "/v1/read-hosts";

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct ReadHostsRequest {
    actor_id: String,
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct WaveStreamRequest {
    actor_id: String,
    case_id: String,
    input_id: String,
    input_version: u64,
    wave: Vec<u8>,
    budget: ResourceBudget,
}

fn static_asset(target: &str) -> Option<(&'static str, &'static [u8])> {
    match target {
        "/" | "/index.html" => Some((
            "text/html; charset=utf-8",
            include_bytes!("../web/index.html"),
        )),
        "/favicon.svg" => Some(("image/svg+xml", include_bytes!("../web/favicon.svg"))),
        "/app.js" => Some((
            "application/javascript; charset=utf-8",
            include_bytes!("../web/app.js"),
        )),
        "/microphone-worklet.js" => Some((
            "application/javascript; charset=utf-8",
            include_bytes!("../web/microphone-worklet.js"),
        )),
        "/styles.css" => Some((
            "text/css; charset=utf-8",
            include_bytes!("../web/styles.css"),
        )),
        _ => None,
    }
}

fn read_http(reader: &mut impl BufRead, max_body: usize) -> Result<HttpRequest, HttpError> {
    let mut header = Vec::new();
    loop {
        let remaining = 16_384usize.saturating_sub(header.len());
        if remaining == 0 {
            return Err(HttpError(431, "headers exceed admission limit"));
        }
        let mut line = Vec::new();
        (&mut *reader)
            .take(remaining as u64)
            .read_until(b'\n', &mut line)
            .map_err(|_| HttpError(400, "cannot read request headers"))?;
        if line.is_empty() || !line.ends_with(b"\r\n") {
            return Err(HttpError(400, "HTTP headers require CRLF"));
        }
        header.extend_from_slice(&line);
        if line == b"\r\n" {
            break;
        }
    }
    let header =
        std::str::from_utf8(&header).map_err(|_| HttpError(400, "invalid header encoding"))?;
    let mut lines = header.split("\r\n");
    let request_line = lines.next().ok_or(HttpError(400, "missing request line"))?;
    let fields: Vec<&str> = request_line.split(' ').collect();
    if fields.len() != 3 || fields[2] != "HTTP/1.1" || fields[0].is_empty() || fields[1].is_empty()
    {
        return Err(HttpError(400, "expected HTTP/1.1 request line"));
    }
    let mut headers: BTreeMap<String, String> = BTreeMap::new();
    for line in lines.take_while(|line| !line.is_empty()) {
        let (key, value) = line
            .split_once(':')
            .ok_or(HttpError(400, "invalid header"))?;
        if key.is_empty()
            || !key
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
            || value
                .bytes()
                .any(|byte| (byte < 32 && byte != b'\t') || byte == 127)
        {
            return Err(HttpError(400, "invalid header syntax"));
        }
        if headers
            .insert(key.to_ascii_lowercase(), value.trim().into())
            .is_some()
        {
            return Err(HttpError(400, "duplicate headers are not supported"));
        }
    }
    if headers.contains_key("transfer-encoding") || headers.contains_key("expect") {
        return Err(HttpError(
            400,
            "transfer encoding and expect are not supported",
        ));
    }
    let length = match headers.get("content-length") {
        None => 0,
        Some(value) if !value.is_empty() && value.bytes().all(|byte| byte.is_ascii_digit()) => {
            value
                .parse::<usize>()
                .map_err(|_| HttpError(413, "body too large"))?
        }
        _ => return Err(HttpError(400, "invalid content length")),
    };
    if length > max_body {
        return Err(HttpError(413, "body exceeds admission limit"));
    }
    let mut body = vec![0; length];
    reader
        .read_exact(&mut body)
        .map_err(|_| HttpError(400, "incomplete request body"))?;
    Ok(HttpRequest {
        method: fields[0].into(),
        target: fields[1].into(),
        headers,
        body,
    })
}

fn constant_time_equal(left: &[u8], right: &[u8]) -> bool {
    let mut difference = left.len() ^ right.len();
    for index in 0..left.len().max(right.len()) {
        difference |= usize::from(
            left.get(index).copied().unwrap_or(0) ^ right.get(index).copied().unwrap_or(0),
        );
    }
    difference == 0
}

fn validate_http(request: &HttpRequest, address: SocketAddr, token: &str) -> Result<(), HttpError> {
    let port = address.port();
    let allowed_hosts = [
        format!("localhost:{port}"),
        format!("127.0.0.1:{port}"),
        format!("[::1]:{port}"),
    ];
    let host = request
        .headers
        .get("host")
        .ok_or(HttpError(400, "Host is required"))?;
    if !allowed_hosts.contains(host) {
        return Err(HttpError(403, "Host is not a loopback server identity"));
    }
    if let Some(origin) = request.headers.get("origin")
        && !allowed_hosts
            .iter()
            .any(|host| origin == &format!("http://{host}"))
    {
        return Err(HttpError(403, "Origin is not permitted"));
    }
    if request.method == "GET"
        && request.body.is_empty()
        && (request.target == "/health" || static_asset(&request.target).is_some())
    {
        return Ok(());
    }
    let authorization = request
        .headers
        .get("authorization")
        .map(String::as_str)
        .unwrap_or("");
    let supplied = authorization.strip_prefix("Bearer ").unwrap_or("");
    if !constant_time_equal(supplied.as_bytes(), token.as_bytes()) {
        return Err(HttpError(401, "authentication required"));
    }
    if request.method != "POST"
        || !matches!(
            request.target.as_str(),
            "/v1/execute"
                | "/v1/execute/stream"
                | WAVE_STREAM_ROUTE
                | READ_HOSTS_ROUTE
                | case_http::FORK_ROUTE
                | case_http::HISTORY_ROUTE
        )
    {
        return Err(HttpError(404, "route not found"));
    }
    if request.headers.get("content-type").map(String::as_str) != Some("application/json") {
        return Err(HttpError(415, "Content-Type must be application/json"));
    }
    if !request.headers.contains_key("content-length") {
        return Err(HttpError(411, "Content-Length is required"));
    }
    if matches!(
        request.target.as_str(),
        "/v1/execute/stream" | WAVE_STREAM_ROUTE
    ) && request.headers.get("accept").map(String::as_str) != Some("text/event-stream")
    {
        return Err(HttpError(406, "Accept must be text/event-stream"));
    }
    Ok(())
}

fn constrain_request(
    mut request: RequestEnvelope,
    options: &Options,
) -> Result<RequestEnvelope, HttpError> {
    if request.actor_id != options.actor {
        return Err(HttpError(
            403,
            "request actor differs from authenticated server actor",
        ));
    }
    if request.permissions.wasm_execution {
        return Err(HttpError(
            403,
            "WebAssembly execution requires local operator authorization",
        ));
    }
    if !request
        .permissions
        .read_hosts
        .is_subset(&options.read_hosts)
        || !request
            .permissions
            .write_paths
            .is_subset(&options.write_paths)
    {
        return Err(HttpError(
            403,
            "request exceeds configured server permission scope",
        ));
    }
    if request
        .output_path
        .as_ref()
        .is_some_and(|path| !options.write_paths.contains(path))
    {
        return Err(HttpError(
            403,
            "output path is not a configured exact write target",
        ));
    }
    request.budget.max_input_bytes = request
        .budget
        .max_input_bytes
        .min(options.budget.max_input_bytes);
    request.budget.max_evidence_bytes = request
        .budget
        .max_evidence_bytes
        .min(options.budget.max_evidence_bytes);
    request.budget.max_actions = request.budget.max_actions.min(options.budget.max_actions);
    request.budget.deadline_ms = request.budget.deadline_ms.min(options.budget.deadline_ms);
    Ok(request)
}

fn send_http(stream: &mut TcpStream, status: u16, body: &[u8]) -> io::Result<()> {
    let reason = match status {
        200 => "OK",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        409 => "Conflict",
        411 => "Length Required",
        413 => "Content Too Large",
        415 => "Unsupported Media Type",
        406 => "Not Acceptable",
        431 => "Request Header Fields Too Large",
        _ => "Internal Server Error",
    };
    write!(
        stream,
        "HTTP/1.1 {status} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\nCache-Control: no-store\r\nX-Content-Type-Options: nosniff\r\n\r\n",
        body.len()
    )?;
    stream.write_all(body)?;
    stream.flush()
}

fn send_static_http(
    stream: &mut TcpStream,
    content_type: &str,
    body: &[u8],
    allows_microphone: bool,
) -> io::Result<()> {
    let microphone = if allows_microphone { "(self)" } else { "()" };
    write!(
        stream,
        "HTTP/1.1 200 OK\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\nCache-Control: no-store, no-transform\r\nX-Content-Type-Options: nosniff\r\nX-Frame-Options: DENY\r\nReferrer-Policy: no-referrer\r\nPermissions-Policy: camera=(), microphone={microphone}, geolocation=()\r\nContent-Security-Policy: default-src 'self'; base-uri 'none'; frame-ancestors 'none'; form-action 'self'; object-src 'none'; script-src 'self'; style-src 'self'; connect-src 'self'; img-src 'self' blob:; media-src 'self' blob:\r\n\r\n",
        body.len()
    )?;
    stream.write_all(body)?;
    stream.flush()
}

fn send_sse_headers(stream: &mut TcpStream) -> io::Result<()> {
    write!(
        stream,
        "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream; charset=utf-8\r\nConnection: close\r\nCache-Control: no-store\r\nX-Content-Type-Options: nosniff\r\nReferrer-Policy: no-referrer\r\n\r\n"
    )?;
    stream.flush()
}

fn send_sse_event(
    stream: &mut TcpStream,
    event: &'static str,
    payload: &serde_json::Value,
) -> io::Result<()> {
    let data = serde_json::to_string(payload)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    write!(stream, "event: {event}\ndata: {data}\n\n")?;
    stream.flush()
}

fn handle_sse(
    engine: &mut Engine,
    actor: &str,
    request: RequestEnvelope,
    stream: &mut TcpStream,
    wave: Option<&[u8]>,
) -> Result<(), HttpError> {
    let input_id = request.input_id.clone();
    let stream_id = format!("{}:{input_id}", request.case_id);
    send_sse_headers(stream).map_err(|_| HttpError(500, "cannot write stream headers"))?;
    send_sse_event(
        stream,
        "provisional",
        &serde_json::json!({
            "api_version": STREAM_API_VERSION,
            "stream_id": stream_id,
            "input_id": input_id,
            "phase": "PROVISIONAL",
            "authority": "NONE",
            "content": null
        }),
    )
    .map_err(|_| HttpError(500, "cannot write provisional stream event"))?;

    let execution = match wave {
        Some(wave) => engine
            .execute_wave_bytes(actor, request, wave)
            .map(|(recognition, result)| (result, Some(recognition))),
        None => engine.execute(actor, request).map(|result| (result, None)),
    };
    match execution {
        Ok((result, recognition)) => {
            if result.lifecycle != Lifecycle::Commit {
                send_sse_event(
                    stream,
                    "invalidated",
                    &serde_json::json!({
                        "api_version": STREAM_API_VERSION,
                        "stream_id": stream_id,
                        "phase": "INVALIDATED",
                        "authority": "NONE",
                        "reason": "CORE_TERMINAL_RESULT_NOT_COMMIT"
                    }),
                )
                .map_err(|_| HttpError(500, "cannot write invalidation stream event"))?;
            }
            let result = serde_json::to_value(result)
                .map_err(|_| HttpError(500, "cannot encode stream result"))?;
            let mut frame = serde_json::json!({
                "api_version": STREAM_API_VERSION,
                "stream_id": stream_id,
                "phase": "FINAL",
                "result": result
            });
            if let Some(recognition) = recognition {
                frame["recognition"] = serde_json::to_value(recognition)
                    .map_err(|_| HttpError(500, "cannot encode speech recognition"))?;
            }
            send_sse_event(stream, "final", &frame)
                .map_err(|_| HttpError(500, "cannot write final stream event"))?;
        }
        Err(error) => {
            send_sse_event(
                stream,
                "invalidated",
                &serde_json::json!({
                    "api_version": STREAM_API_VERSION,
                    "stream_id": stream_id,
                    "phase": "INVALIDATED",
                    "authority": "NONE",
                    "reason": "CORE_EXECUTION_ERROR"
                }),
            )
            .map_err(|_| HttpError(500, "cannot write invalidation stream event"))?;
            let mut frame = serde_json::json!({
                    "api_version": STREAM_API_VERSION,
                    "stream_id": stream_id,
                    "phase": "ERROR",
                    "status": 409,
                    "error": "engine rejected request; inspect durable state with the trusted local CLI"
            });
            if wave.is_some() {
                let (code, message) = wave_stream_error(&error);
                frame["code"] = serde_json::json!(code);
                frame["error"] = serde_json::json!(message);
            }
            send_sse_event(stream, "error", &frame)
                .map_err(|_| HttpError(500, "cannot write stream error event"))?;
        }
    }
    Ok(())
}

fn wave_stream_error(error: &minidora_domain::Error) -> (&'static str, &'static str) {
    let code = match error {
        minidora_domain::Error::Contract { code, .. } => code.as_str(),
        _ => "",
    };
    match code {
        "INVALID_WAVE_PCM" => (
            "INVALID_WAVE_PCM",
            "選んだWAVをPCM音声として読み取れません。",
        ),
        "SPEECH_INPUT_FORMAT" => (
            "SPEECH_INPUT_FORMAT",
            "16 kHz・16 bit・モノラルPCMのWAVを指定してください。",
        ),
        "SPEECH_INPUT_LIMIT" => (
            "SPEECH_INPUT_LIMIT",
            "WAVまたは認識文が依頼の入力上限を超えました。",
        ),
        "SPEECH_INPUT_UNCONFIGURED" => (
            "SPEECH_INPUT_UNCONFIGURED",
            "サーバー起動時に --asr-assets で音声認識資産を指定してください。",
        ),
        "SPEECH_INPUT_UNAVAILABLE" => (
            "SPEECH_INPUT_UNAVAILABLE",
            "この実行環境ではローカル日本語音声認識を利用できません。",
        ),
        "SPEECH_INPUT_DEADLINE" => (
            "SPEECH_INPUT_DEADLINE",
            "期限内に音声を認識できませんでした。",
        ),
        "SPEECH_INPUT_NO_RESULT" => (
            "SPEECH_INPUT_NO_RESULT",
            "WAVの発話を認識できませんでした。",
        ),
        "SPEECH_INPUT_MULTIPLE_UTTERANCES" => (
            "SPEECH_INPUT_MULTIPLE_UTTERANCES",
            "一つの発話に分けたWAVを選んでください。",
        ),
        "ACCESS_DENIED" => (
            "ACCESS_DENIED",
            "現在の利用者から指定の会話にはアクセスできません。",
        ),
        "INPUT_IDENTITY_CONFLICT" => (
            "INPUT_IDENTITY_CONFLICT",
            "この入力IDには異なる依頼が記録されています。",
        ),
        "INVALID_IDENTITY" => ("INVALID_IDENTITY", "会話IDまたは入力IDが無効です。"),
        _ => (
            "SPEECH_INPUT_FAILED",
            "音声を処理できませんでした。ファイルと音声認識の設定を確認してください。",
        ),
    }
}

fn handle_http(
    engine: &mut Engine,
    options: &Options,
    token: &str,
    address: SocketAddr,
    stream: &mut TcpStream,
) -> Result<(), HttpError> {
    let max_body = options
        .budget
        .max_input_bytes
        .saturating_mul(6)
        .saturating_add(16_384)
        .min(8_000_000);
    let input = AdmissionReader {
        stream: &mut *stream,
        deadline: Instant::now() + Duration::from_secs(5),
    };
    let request = read_http(&mut BufReader::new(input), max_body)?;
    validate_http(&request, address, token)?;
    if request.method == "GET"
        && let Some((content_type, body)) = static_asset(&request.target)
    {
        let allows_microphone = matches!(request.target.as_str(), "/" | "/index.html");
        send_static_http(stream, content_type, body, allows_microphone)
            .map_err(|_| HttpError(500, "cannot write response"))?;
        return Ok(());
    }
    if request.method == "POST" && request.target == "/v1/execute/stream" {
        let envelope = serde_json::from_slice::<RequestEnvelope>(&request.body)
            .map_err(|_| HttpError(400, "invalid RequestEnvelope JSON"))?;
        let envelope = constrain_request(envelope, options)?;
        return handle_sse(engine, &options.actor, envelope, stream, None);
    }
    if request.method == "POST" && request.target == WAVE_STREAM_ROUTE {
        let input = serde_json::from_slice::<WaveStreamRequest>(&request.body)
            .map_err(|_| HttpError(400, "WAV入力のJSONが無効です。"))?;
        if input.input_version == 0 {
            return Err(HttpError(400, "入力版には1以上を指定してください。"));
        }
        let mut envelope =
            RequestEnvelope::new(&input.case_id, &input.input_id, &input.actor_id, "");
        envelope.input_version = input.input_version;
        envelope.budget = input.budget;
        let envelope = constrain_request(envelope, options)?;
        return handle_sse(engine, &options.actor, envelope, stream, Some(&input.wave));
    }
    let body = match request.target.as_str() {
        "/health" => serde_json::to_vec(
            &serde_json::json!({ "status": "ok", "schema_version": SCHEMA_VERSION }),
        )
        .map_err(|_| HttpError(500, "cannot encode response"))?,
        READ_HOSTS_ROUTE => {
            let input = serde_json::from_slice::<ReadHostsRequest>(&request.body)
                .map_err(|_| HttpError(400, "通信先の確認に必要な入力が無効です。"))?;
            if input.actor_id != options.actor {
                return Err(HttpError(403, "指定した利用者が認証先と一致しません。"));
            }
            serde_json::to_vec(&serde_json::json!({ "read_hosts": &options.read_hosts }))
                .map_err(|_| HttpError(500, "cannot encode response"))?
        }
        case_http::FORK_ROUTE => case_http::fork(engine, &options.actor, &request.body)?,
        case_http::HISTORY_ROUTE => case_http::history(engine, &options.actor, &request.body)?,
        _ => {
            let envelope = serde_json::from_slice::<RequestEnvelope>(&request.body)
                .map_err(|_| HttpError(400, "invalid RequestEnvelope JSON"))?;
            let envelope = constrain_request(envelope, options)?;
            let result = engine.execute(&options.actor, envelope).map_err(|_| {
                HttpError(
                    409,
                    "engine rejected request; inspect durable state with the trusted local CLI",
                )
            })?;
            serde_json::to_vec(&result).map_err(|_| HttpError(500, "cannot encode response"))?
        }
    };
    send_http(stream, 200, &body).map_err(|_| HttpError(500, "cannot write response"))
}

fn serve(engine: &mut Engine, options: &Options) -> Result<u8, String> {
    let token = std::env::var("MINIDORA_API_TOKEN").map_err(|_| {
        "serve requires MINIDORA_API_TOKEN; configure it securely in the local process".to_owned()
    })?;
    if token.len() < 32 || token.bytes().any(|byte| !(33..=126).contains(&byte)) {
        return Err(
            "MINIDORA_API_TOKEN must have at least 32 printable ASCII bytes without spaces".into(),
        );
    }
    let listener = TcpListener::bind(options.bind).map_err(|error| error.to_string())?;
    let address = listener.local_addr().map_err(|error| error.to_string())?;
    eprintln!(
        "MINIDORA local API listening on {address}; actor={} schema={SCHEMA_VERSION}",
        options.actor
    );
    for incoming in listener.incoming() {
        let mut stream = incoming.map_err(|error| error.to_string())?;
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .map_err(|error| error.to_string())?;
        stream
            .set_write_timeout(Some(Duration::from_secs(5)))
            .map_err(|error| error.to_string())?;
        if let Err(HttpError(status, message)) =
            handle_http(engine, options, &token, address, &mut stream)
        {
            let body = serde_json::to_vec(&serde_json::json!({ "error": message }))
                .map_err(|error| error.to_string())?;
            // A disconnected client cannot alter execution already recorded by Core.
            let _ = send_http(&mut stream, status, &body);
        }
    }
    Ok(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(values: &[&str]) -> Result<Options, String> {
        parse(values.iter().map(|value| (*value).into()))
    }

    #[test]
    fn output_path_does_not_grant_write_authority() {
        let options = args(&["--output", "/tmp/result", "2+3"]).unwrap();
        assert!(options.write_paths.is_empty());
        assert_eq!(options.output, Some(PathBuf::from("/tmp/result")));
    }

    #[test]
    fn wasm_configuration_and_grants_have_separate_local_scope() {
        let absolute = std::env::current_dir().unwrap().join("node");
        let path = absolute.to_str().unwrap();
        let configured = args(&["chat", "--wasm-node", path]).unwrap();
        assert_eq!(configured.wasm_node, Some(absolute.clone()));
        assert!(!configured.allow_wasm_execution);

        let requested = args(&["run", "--allow-wasm-execution", "2+3"]).unwrap();
        assert!(requested.allow_wasm_execution);
        assert!(requested.wasm_node.is_none());
        assert!(args(&["chat", "--allow-wasm-execution"]).is_err());
        assert!(args(&["serve", "--allow-wasm-execution"]).is_err());
        assert!(args(&["serve", "--wasm-node", path]).is_err());
        assert!(args(&["reconcile", "--wasm-node", path]).is_err());
        assert!(args(&["run", "--wasm-node", "relative/path"]).is_err());
        assert!(args(&["run", "--wasm-node", path, "--wasm-node", path]).is_err());
    }

    #[test]
    fn reconcile_cannot_introduce_new_request_or_effect_permissions() {
        assert_eq!(
            args(&["reconcile", "--case", "saved-case"]).unwrap().mode,
            Mode::Reconcile
        );
        assert_eq!(
            args(&["reconcile-effects", "--case", "saved-case"])
                .unwrap()
                .mode,
            Mode::ReconcileEffects
        );
        for command in ["reconcile", "reconcile-effects"] {
            for forbidden in [
                "--input",
                "--version",
                "--allow-write",
                "--allow-read-host",
                "--output",
                "new request",
            ] {
                assert!(args(&[command, forbidden, "new-value"]).is_err());
            }
        }
    }

    #[test]
    fn schema_migration_accepts_only_a_database_path_and_json_output() {
        assert!(args(&["migrate"]).is_err());
        assert!(args(&["migrate", "--help"]).is_ok());
        assert_eq!(
            args(&["migrate", "--db", "/tmp/state"]).unwrap().mode,
            Mode::Migrate
        );
        assert_eq!(
            args(&["migrate", "--db", "/tmp/state", "--json"])
                .unwrap()
                .mode,
            Mode::Migrate
        );
        for forbidden in [
            "--case",
            "--input",
            "--version",
            "--output",
            "--allow-write",
            "request",
        ] {
            assert!(args(&["migrate", "--db", "/tmp/state", forbidden, "value"]).is_err());
        }
    }

    #[test]
    fn reject_public_server_and_bad_unsigned_budgets() {
        assert!(args(&["serve", "--bind", "0.0.0.0:8080"]).is_err());
        assert!(args(&["--max-actions", "-1", "2+3"]).is_err());
        assert!(args(&["--version", "+1", "2+3"]).is_err());
    }

    #[test]
    fn request_flags_can_be_escaped() {
        assert_eq!(args(&["--", "--cancel"]).unwrap().text, ["--cancel"]);
    }

    #[test]
    fn transport_rejects_duplicate_length_and_transfer_encoding() {
        for header in [
            "POST /v1/execute HTTP/1.1\r\nContent-Length: 0\r\nContent-Length: 0\r\n\r\n",
            "POST /v1/execute HTTP/1.1\r\nTransfer-Encoding: chunked\r\n\r\n",
        ] {
            assert!(read_http(&mut io::Cursor::new(header), 100).is_err());
        }
    }

    #[test]
    fn transport_enforces_bounded_headers_and_body() {
        let header = format!(
            "POST /v1/execute HTTP/1.1\r\nX-Test: {}\r\n\r\n",
            "x".repeat(16_384)
        );
        assert!(read_http(&mut io::Cursor::new(header), 100).is_err());
        let request = "POST /v1/execute HTTP/1.1\r\nContent-Length: 101\r\n\r\n";
        assert!(matches!(
            read_http(&mut io::Cursor::new(request), 100),
            Err(HttpError(413, _))
        ));
    }

    #[test]
    fn transport_rejects_wrong_host_origin_and_token() {
        let token = "a".repeat(32);
        let body = "{}";
        let text = format!(
            "POST /v1/execute HTTP/1.1\r\nHost: localhost:8080\r\nContent-Type: application/json\r\nContent-Length: {}\r\nAuthorization: Bearer {token}\r\n\r\n{body}",
            body.len()
        );
        let mut request = read_http(&mut io::Cursor::new(text), 100).unwrap();
        let address = "127.0.0.1:8080".parse().unwrap();
        assert!(validate_http(&request, address, &token).is_ok());
        assert!(matches!(
            validate_http(&request, address, "wrong"),
            Err(HttpError(401, _))
        ));
        request
            .headers
            .insert("origin".into(), "https://attacker.invalid".into());
        assert!(matches!(
            validate_http(&request, address, &token),
            Err(HttpError(403, _))
        ));
        request.headers.remove("origin");
        request
            .headers
            .insert("host".into(), "attacker.invalid:8080".into());
        assert!(matches!(
            validate_http(&request, address, &token),
            Err(HttpError(403, _))
        ));
    }

    #[test]
    fn server_actor_and_grants_cannot_be_claimed_by_request() {
        let options = args(&["serve", "--actor", "trusted"]).unwrap();
        let forged = RequestEnvelope::new("case", "input", "untrusted", "2+3");
        assert!(matches!(
            constrain_request(forged, &options),
            Err(HttpError(403, _))
        ));
        let mut remote_execution = RequestEnvelope::new("case", "input", "trusted", "2+3");
        remote_execution.permissions.wasm_execution = true;
        assert!(matches!(
            constrain_request(remote_execution, &options),
            Err(HttpError(403, _))
        ));
        let mut request = RequestEnvelope::new("case", "input", "trusted", "2+3");
        request.permissions.read_hosts.insert("example.com".into());
        assert!(matches!(
            constrain_request(request, &options),
            Err(HttpError(403, _))
        ));
    }

    #[test]
    fn terminal_codes_are_distinct_and_nonterminal_is_failure() {
        let codes: BTreeSet<u8> = [
            Lifecycle::Commit,
            Lifecycle::Suspend,
            Lifecycle::Reject,
            Lifecycle::Fail,
            Lifecycle::Cancel,
        ]
        .into_iter()
        .map(terminal_exit)
        .collect();
        assert_eq!(codes.len(), 5);
        assert_eq!(terminal_exit(Lifecycle::Adopted), 4);
    }
}
