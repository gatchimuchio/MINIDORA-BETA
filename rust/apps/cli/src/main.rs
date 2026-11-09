//! Thin local frontends: interpretation, execution and judgment remain in Engine.
#![forbid(unsafe_code)]

use minidora_domain::{
    ActionRecord, EffectPhase, Lifecycle, RequestEnvelope, ResourceBudget, ResultRecord,
    SCHEMA_VERSION,
};
use minidora_engine::Engine;
use std::collections::{BTreeMap, BTreeSet};
use std::io::{self, BufRead, BufReader, IsTerminal, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::{Duration, Instant};

const HELP: &str = r#"MINIDORA Rust 0.7.0-alpha.6 — local execution frontend

Usage:
  minidora-rust [run] [OPTIONS] [--] REQUEST...
  minidora-rust inspect [OPTIONS]
  minidora-rust reconcile [--db DIRECTORY] [--case ID] [--actor ID] [--json]
  minidora-rust reconcile-effects [--db DIRECTORY] [--case ID] [--actor ID] [--json]
  minidora-rust serve [OPTIONS] [--bind 127.0.0.1:8080]

Options:
  --db DIRECTORY             Durable state directory (default .minidora-rust-state)
  --case ID                  Case identity (default default)
  --input ID                 Explicit durable input identity; omit for next revision
  --version N                Input version (default 1)
  --actor ID                 Trusted local frontend identity
  --json                     Print the full result or case revision as JSON
  --allow-read-host HOST      Permit one exact read destination; repeatable
  --allow-write EXACT_PATH    Permit one exact output path; repeatable
  --output EXACT_PATH        Request an output file; does not grant write permission
  --max-actions N            Finite action admission budget
  --deadline-ms N            Finite execution deadline
  --max-input-bytes N        Input admission limit
  --max-evidence-bytes N     New evidence admission limit
  --help                     Show this help

Without text arguments, run reads bounded UTF-8 text from redirected stdin.
Use -- before request text beginning with a dash. Retries with an explicit
(case,input,version) reuse the durable result or report an identity conflict.

Actor identity comes from --actor, MINIDORA_ACTOR, or the local USER/USERNAME.
These are trusted local configuration, not remote authentication. State directory
access must be protected by the host. Development budgets are finite admission
defaults, not measured performance or Windows/8GB guarantees.

Reconcile asks Core to inspect the original saved operation and actual file.
It accepts no new goal, permissions or target, and performs no write or resend.
Reconcile-effects inspects pending operations only for a cancelled case. It
updates the saved action ledger from the original local file and preserves
case CANCEL. Exit 2 means an effect still requires reconciliation; exit 0 means
all recorded effects are confirmed, failed or cancelled.

Serve binds only a loopback IP and requires MINIDORA_API_TOKEN (at least 32 bytes).
GET /health reports the schema. POST /v1/execute requires Authorization: Bearer
TOKEN and a full RequestEnvelope JSON object. The configured server actor is the
only authenticated identity. Request permissions must be subsets of --allow-*
grants; budgets are capped to server limits. HTTP is local plaintext only.

Exit: COMMIT 0; SUSPEND 2; REJECT 3; FAIL 4; CANCEL 5; frontend error 1.
"#;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Mode {
    Run,
    Inspect,
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
    input_version: u64,
    actor: String,
    json: bool,
    read_hosts: BTreeSet<String>,
    write_paths: BTreeSet<PathBuf>,
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
        input_version: 1,
        actor: default_actor(),
        json: false,
        read_hosts: BTreeSet::new(),
        write_paths: BTreeSet::new(),
        output: None,
        budget: ResourceBudget::development(),
        bind: "127.0.0.1:8080".parse().expect("literal socket address"),
        text: Vec::new(),
    };
    let mut args = args.into_iter().peekable();
    match args.peek().map(String::as_str) {
        Some("run") => {
            args.next();
        }
        Some("inspect") => {
            options.mode = Mode::Inspect;
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
        if matches!(options.mode, Mode::Reconcile | Mode::ReconcileEffects)
            && !matches!(
                arg.as_str(),
                "--db" | "--case" | "--actor" | "--json" | "--help" | "-h"
            )
        {
            return Err("reconciliation accepts only --db, --case, --actor and --json; saved operations cannot gain a new goal or permission".into());
        }
        match arg.as_str() {
            "--" => {
                options.text.extend(args);
                break;
            }
            "--help" | "-h" => options.mode = Mode::Help,
            "--db" => options.db = next_value(&mut args, &arg)?.into(),
            "--case" => options.case_id = next_value(&mut args, &arg)?,
            "--input" => options.input_id = Some(next_value(&mut args, &arg)?),
            "--version" => options.input_version = number(next_value(&mut args, &arg)?, &arg)?,
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
    if options.mode != Mode::Run && !options.text.is_empty() {
        return Err("only run accepts request text".into());
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
    }
    Ok(())
}

fn run(options: Options) -> Result<u8, String> {
    if options.mode == Mode::Help {
        print!("{HELP}");
        return Ok(0);
    }
    let mut engine = Engine::open(&options.db).map_err(|error| error.to_string())?;
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
        let revision = engine
            .latest(&options.actor, &options.case_id)
            .map_err(|error| error.to_string())?;
        let stdout = io::stdout();
        let mut output = stdout.lock();
        serde_json::to_writer(&mut output, &revision).map_err(|error| error.to_string())?;
        writeln!(output).map_err(|error| error.to_string())?;
        return Ok(0);
    }
    if options.mode == Mode::Serve {
        return serve(&mut engine, &options);
    }
    let text = if options.text.is_empty() {
        if io::stdin().is_terminal() {
            return Err("supply request text or redirect UTF-8 stdin; see --help".into());
        }
        let mut text = String::new();
        let limit = u64::try_from(options.budget.max_input_bytes)
            .unwrap_or(u64::MAX)
            .saturating_add(1);
        io::stdin()
            .take(limit)
            .read_to_string(&mut text)
            .map_err(|error| error.to_string())?;
        if text.len() > options.budget.max_input_bytes {
            return Err("stdin exceeds --max-input-bytes; no input was executed".into());
        }
        text
    } else {
        options.text.join(" ")
    };
    if text.trim().is_empty() {
        return Err("request text is empty".into());
    }
    let input_id = match options.input_id {
        Some(id) => id,
        None => {
            let revision = engine
                .latest(&options.actor, &options.case_id)
                .map_err(|error| error.to_string())?;
            let next = revision.map_or(Ok(1), |revision| {
                revision
                    .revision
                    .checked_add(1)
                    .ok_or_else(|| "case revision overflow".to_owned())
            })?;
            format!("input-{next}")
        }
    };
    let mut request = RequestEnvelope::new(&options.case_id, &input_id, &options.actor, &text);
    request.input_version = options.input_version;
    request.permissions.read_hosts = options.read_hosts;
    request.permissions.write_paths = options.write_paths;
    request.output_path = options.output;
    request.budget = options.budget;
    let result = engine
        .execute(&options.actor, request)
        .map_err(|error| error.to_string())?;
    print_result(&result, options.json)?;
    Ok(terminal_exit(result.lifecycle))
}

fn print_actions(actions: &[ActionRecord], json: bool) -> Result<(), String> {
    let stdout = io::stdout();
    let mut output = stdout.lock();
    if json {
        serde_json::to_writer(&mut output, actions).map_err(|error| error.to_string())?;
        writeln!(output).map_err(|error| error.to_string())?;
    } else {
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
    if request.method == "GET" && request.target == "/health" && request.body.is_empty() {
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
    if request.method != "POST" || request.target != "/v1/execute" {
        return Err(HttpError(404, "route not found"));
    }
    if request.headers.get("content-type").map(String::as_str) != Some("application/json") {
        return Err(HttpError(415, "Content-Type must be application/json"));
    }
    if !request.headers.contains_key("content-length") {
        return Err(HttpError(411, "Content-Length is required"));
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
    let body = if request.method == "GET" {
        serde_json::to_vec(&serde_json::json!({ "status": "ok", "schema_version": SCHEMA_VERSION }))
            .map_err(|_| HttpError(500, "cannot encode response"))?
    } else {
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
