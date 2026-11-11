//! These tests enter through the installed binary; no action/result is injected.
use serde_json::Value;
use std::ffi::OsStr;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

static NEXT_DIRECTORY: AtomicU64 = AtomicU64::new(0);

struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Self {
        let epoch = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let serial = NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "minidora-cli-{}-{epoch}-{serial}",
            std::process::id()
        ));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        // Explicit diagnostic runs can retain real CLI state for later inspection.
        if std::env::var_os("MINIDORA_KEEP_TEST_STATE").is_none() {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
}

fn execute(directory: &Path, actor: &str, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_minidora-rust"))
        .args([
            "--db",
            directory.to_str().unwrap(),
            "--case",
            "normal-route",
            "--actor",
            actor,
            "--json",
        ])
        .args(args)
        .output()
        .unwrap()
}

fn record(output: &Output) -> Value {
    assert!(
        output.status.success(),
        "status={:?} stderr={} stdout={}",
        output.status.code(),
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

#[test]
fn calculation_followup_and_explicit_retry_survive_process_restarts() {
    let scratch = Scratch::new();
    let first = record(&execute(
        &scratch.0,
        "owner",
        &["--input", "calculation", "2+3"],
    ));
    assert_eq!(first["lifecycle"], "COMMIT");
    assert_eq!(first["calculation"]["exact_value"], "5");

    let retry = record(&execute(
        &scratch.0,
        "owner",
        &["--input", "calculation", "2+3"],
    ));
    assert_eq!(
        retry, first,
        "explicit retry must return the durable original result"
    );

    let explanation = record(&execute(&scratch.0, "owner", &["詳しく説明して"]));
    assert_eq!(explanation["lifecycle"], "COMMIT");
    assert!(explanation["text"].as_str().unwrap().contains('5'));
    assert!(
        explanation["dependencies"]
            .as_array()
            .unwrap()
            .contains(&first["id"])
    );
}

#[test]
fn explicit_store_migration_cli_is_idempotent_for_alpha8() {
    let scratch = Scratch::new();
    let implicit_path = Command::new(env!("CARGO_BIN_EXE_minidora-rust"))
        .current_dir(&scratch.0)
        .args(["migrate"])
        .output()
        .unwrap();
    assert!(!implicit_path.status.success());
    assert!(!scratch.0.join(".minidora-rust-state").exists());

    let before = record(&execute(
        &scratch.0,
        "owner",
        &["--input", "migration-noop", "2+3"],
    ));
    let migration = Command::new(env!("CARGO_BIN_EXE_minidora-rust"))
        .args(["migrate", "--db"])
        .arg(&scratch.0)
        .arg("--json")
        .output()
        .unwrap();
    assert!(
        migration.status.success(),
        "status={:?} stderr={}",
        migration.status.code(),
        String::from_utf8_lossy(&migration.stderr)
    );
    let report: Value = serde_json::from_slice(&migration.stdout).unwrap();
    assert_eq!(report["from_schema"], "minidora-rust/0.7.0-alpha.7");
    assert_eq!(report["to_schema"], "minidora-rust/0.7.0-alpha.8");
    assert_eq!(report["migrated"], false);

    let after = record(&execute(
        &scratch.0,
        "owner",
        &["--input", "migration-noop", "2+3"],
    ));
    assert_eq!(after, before);
}

#[test]
fn mixed_unit_arithmetic_is_saved_and_explained_through_the_normal_route() {
    let scratch = Scratch::new();
    let calculation = record(&execute(
        &scratch.0,
        "owner",
        &["--input", "mixed-unit-addition", "1 km + 500 m"],
    ));
    assert_eq!(calculation["lifecycle"], "COMMIT");
    assert_eq!(calculation["calculation"]["expression"], "1 km + 500 m");
    assert_eq!(calculation["calculation"]["exact_value"], "1500 m");
    assert_eq!(calculation["calculation"]["steps"][0], "1 km = 1000 m");
    assert_eq!(
        calculation["calculation"]["steps"][2],
        "1000 m + 500 m = 1500 m"
    );

    let explanation = record(&execute(&scratch.0, "owner", &["詳しく説明して"]));
    assert_eq!(explanation["lifecycle"], "COMMIT");
    assert_eq!(explanation["calculation"]["exact_value"], "1500 m");
    assert!(
        explanation["text"]
            .as_str()
            .unwrap()
            .contains("1000 m + 500 m = 1500 m")
    );
    assert!(
        explanation["dependencies"]
            .as_array()
            .unwrap()
            .contains(&calculation["id"])
    );

    let incompatible = stopped(
        &execute(
            &scratch.0,
            "owner",
            &["--input", "incompatible-units", "1 km + 2 s"],
        ),
        2,
        "SUSPEND",
    );
    assert!(
        incompatible["reason"]
            .as_str()
            .unwrap()
            .starts_with("UNIT_MISMATCH:")
    );
    assert!(incompatible["calculation"].is_null());
    assert!(incompatible["receipts"].as_array().unwrap().is_empty());
}

#[test]
fn explicit_operand_correction_creates_a_new_version_linked_to_the_actual_result() {
    let scratch = Scratch::new();
    let original = record(&execute(
        &scratch.0,
        "owner",
        &["--input", "measurement", "--version", "1", "1 km + 500 m"],
    ));
    assert_eq!(original["input_version"], 1);
    assert_eq!(original["calculation"]["exact_value"], "1500 m");

    let corrected = record(&execute(
        &scratch.0,
        "owner",
        &[
            "--input",
            "measurement",
            "--version",
            "2",
            "第二項を600 mに変更して",
        ],
    ));
    assert_eq!(corrected["lifecycle"], "COMMIT");
    assert_eq!(corrected["calculation"]["expression"], "1 km + 600 m");
    assert_eq!(corrected["calculation"]["exact_value"], "1600 m");
    assert_eq!(corrected["input_version"], 2);
    assert_ne!(corrected["id"], original["id"]);
    assert!(
        corrected["dependencies"]
            .as_array()
            .unwrap()
            .contains(&original["id"])
    );

    let original_retry = record(&execute(
        &scratch.0,
        "owner",
        &["--input", "measurement", "--version", "1", "1 km + 500 m"],
    ));
    assert_eq!(original_retry, original);

    let explanation = record(&execute(&scratch.0, "owner", &["詳しく説明して"]));
    assert_eq!(explanation["lifecycle"], "COMMIT");
    assert_eq!(explanation["calculation"]["exact_value"], "1600 m");
    assert!(
        explanation["dependencies"]
            .as_array()
            .unwrap()
            .contains(&corrected["id"])
    );

    let missing = stopped(
        &execute(&Scratch::new().0, "owner", &["第二項を600 mに変更して"]),
        2,
        "SUSPEND",
    );
    assert!(
        missing["reason"]
            .as_str()
            .unwrap()
            .starts_with("MISSING_ACTUAL_RESULT:")
    );
    assert!(missing["calculation"].is_null());
}

#[test]
fn explicit_operand_correction_recomputes_other_bounded_binary_operators() {
    for (expression, replacement, expected_expression, expected_initial, expected_corrected) in [
        ("2 * 3", "第二項を4に変更して", "2 * 4", "6", "8"),
        ("2 * 3", "第一項を4に変更して", "4 * 3", "6", "12"),
        ("12 / 3", "第二項を4に変更して", "12 / 4", "4", "3"),
        ("10 % 3", "第二項を4に変更して", "10 % 4", "1", "2"),
        ("2 ** 3", "第二項を4に変更して", "2 ** 4", "8", "16"),
        ("2 ^ 3", "第二項を4に変更して", "2 ^ 4", "8", "16"),
    ] {
        let scratch = Scratch::new();
        let original = record(&execute(
            &scratch.0,
            "owner",
            &[
                "--input",
                "operator-correction",
                "--version",
                "1",
                expression,
            ],
        ));
        assert_eq!(original["lifecycle"], "COMMIT", "{expression}");
        assert_eq!(original["calculation"]["exact_value"], expected_initial);

        let corrected = record(&execute(
            &scratch.0,
            "owner",
            &[
                "--input",
                "operator-correction",
                "--version",
                "2",
                replacement,
            ],
        ));
        assert_eq!(corrected["lifecycle"], "COMMIT", "{expression}");
        assert_eq!(corrected["input_version"], 2);
        assert_eq!(corrected["calculation"]["expression"], expected_expression);
        assert_eq!(corrected["calculation"]["exact_value"], expected_corrected);
        assert!(
            corrected["dependencies"]
                .as_array()
                .unwrap()
                .contains(&original["id"])
        );
        if expression == "12 / 3" {
            let zero_divisor = stopped(
                &execute(
                    &scratch.0,
                    "owner",
                    &[
                        "--input",
                        "operator-correction",
                        "--version",
                        "3",
                        "第二項を0に変更して",
                    ],
                ),
                2,
                "SUSPEND",
            );
            assert!(
                zero_divisor["reason"]
                    .as_str()
                    .unwrap()
                    .starts_with("DIVISION_BY_ZERO:")
            );
            assert_eq!(
                record(&execute(
                    &scratch.0,
                    "owner",
                    &[
                        "--input",
                        "operator-correction",
                        "--version",
                        "2",
                        replacement,
                    ],
                )),
                corrected
            );
        }
        assert_eq!(
            record(&execute(
                &scratch.0,
                "owner",
                &[
                    "--input",
                    "operator-correction",
                    "--version",
                    "1",
                    expression
                ],
            )),
            original
        );
    }

    let scratch = Scratch::new();
    let compound = record(&execute(
        &scratch.0,
        "owner",
        &["--input", "compound-correction", "--version", "1", "2+3*4"],
    ));
    assert_eq!(compound["calculation"]["exact_value"], "14");
    let refused = stopped(
        &execute(
            &scratch.0,
            "owner",
            &[
                "--input",
                "compound-correction",
                "--version",
                "2",
                "第二項を5に変更して",
            ],
        ),
        2,
        "SUSPEND",
    );
    assert!(
        refused["reason"]
            .as_str()
            .unwrap()
            .starts_with("CORRECTION_UNSUPPORTED:")
    );
    assert_eq!(
        record(&execute(
            &scratch.0,
            "owner",
            &["--input", "compound-correction", "--version", "1", "2+3*4"],
        )),
        compound
    );
}

#[test]
fn meaning_comparison_variants_reach_cli_and_compound_effects_suspend() {
    let scratch = Scratch::new();
    let comparison = record(&execute(
        &scratch.0,
        "owner",
        &[
            "--input",
            "comparison",
            "次の2文を比較して、意味の差を説明してください。\nA: 太郎は書類を提出した。\nB: 太郎は書類を提出しなかった。",
        ],
    ));
    assert_eq!(comparison["lifecycle"], "COMMIT");
    assert!(comparison["text"].as_str().unwrap().contains("肯否"));
    assert!(comparison["text"].as_str().unwrap().contains("書類"));

    let numeric_comparison = record(&execute(
        &scratch.0,
        "owner",
        &[
            "--input",
            "numeric-comparison",
            "次の2文を比較してください。\nA: 参加者は三人以上だった。\nB: 参加者は４人以上だった。",
        ],
    ));
    assert_eq!(numeric_comparison["lifecycle"], "COMMIT");
    assert!(
        numeric_comparison["text"]
            .as_str()
            .unwrap()
            .contains("3人以上")
    );
    assert!(
        numeric_comparison["text"]
            .as_str()
            .unwrap()
            .contains("4人以上")
    );

    let scaled_numeric_comparison = record(&execute(
        &scratch.0,
        "owner",
        &[
            "--input",
            "scaled-numeric-comparison",
            "次の2文を比較してください。\nA: 参加者は1.5万人以上だった。\nB: 参加者は二万人以上だった。",
        ],
    ));
    assert_eq!(scaled_numeric_comparison["lifecycle"], "COMMIT");
    assert!(
        scaled_numeric_comparison["text"]
            .as_str()
            .unwrap()
            .contains("15000人以上")
    );
    assert!(
        scaled_numeric_comparison["text"]
            .as_str()
            .unwrap()
            .contains("20000人以上")
    );

    let compound = execute(
        &scratch.0,
        "owner",
        &[
            "--input",
            "compound-comparison",
            "AとBの違いを説明して送信して\nA: 太郎は鳥です。\nB: 花子は魚です。",
        ],
    );
    assert_eq!(compound.status.code(), Some(2));
    let suspended: Value = serde_json::from_slice(&compound.stdout).unwrap();
    assert_eq!(suspended["lifecycle"], "SUSPEND");
}

#[test]
fn identity_collision_does_not_overwrite_original_result() {
    let scratch = Scratch::new();
    let first = record(&execute(&scratch.0, "owner", &["--input", "stable", "7*8"]));
    let collision = execute(&scratch.0, "owner", &["--input", "stable", "7*9"]);
    assert!(!collision.status.success());
    let recovered = record(&execute(&scratch.0, "owner", &["--input", "stable", "7*8"]));
    assert_eq!(recovered, first);
}

#[test]
fn another_actor_cannot_inspect_known_case_identity() {
    let scratch = Scratch::new();
    record(&execute(
        &scratch.0,
        "owner",
        &["--input", "private", "18*3"],
    ));
    let inspection = Command::new(env!("CARGO_BIN_EXE_minidora-rust"))
        .args([
            "inspect",
            "--db",
            scratch.0.to_str().unwrap(),
            "--case",
            "normal-route",
            "--actor",
            "outsider",
        ])
        .output()
        .unwrap();
    assert!(!inspection.status.success());
    assert!(
        inspection.stdout.is_empty(),
        "denied inspection must not export a revision"
    );
}

#[test]
fn requested_file_needs_separate_exact_permission_and_real_receipt() {
    let scratch = Scratch::new();
    let output_path = scratch.0.join("answer.txt");
    let denied = execute(
        &scratch.0,
        "owner",
        &[
            "--input",
            "denied",
            "--output",
            output_path.to_str().unwrap(),
            "9*7",
        ],
    );
    assert!(!denied.status.success());
    assert!(!output_path.exists());

    let permitted = record(&execute(
        &scratch.0,
        "owner",
        &[
            "--input",
            "permitted",
            "--output",
            output_path.to_str().unwrap(),
            "--allow-write",
            output_path.to_str().unwrap(),
            "9*7",
        ],
    ));
    assert_eq!(permitted["lifecycle"], "COMMIT");
    let actual = std::fs::read_to_string(&output_path).unwrap();
    assert_eq!(actual, permitted["text"].as_str().unwrap());
    assert!(actual.contains("63"));
    assert!(
        permitted["receipts"]
            .as_array()
            .unwrap()
            .iter()
            .any(|receipt| receipt["phase"] == "CONFIRMED")
    );
}

#[test]
fn unsupported_input_is_suspension_and_not_an_empty_success() {
    let scratch = Scratch::new();
    let output = execute(&scratch.0, "owner", &["未定義の機能を実行してください"]);
    assert_eq!(output.status.code(), Some(2));
    let record: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(record["lifecycle"], "SUSPEND");
    assert!(!record["reason"].as_str().unwrap().is_empty());
}

#[test]
fn public_bind_is_rejected_before_service_start() {
    let scratch = Scratch::new();
    let output = Command::new(env!("CARGO_BIN_EXE_minidora-rust"))
        .args([
            OsStr::new("serve"),
            OsStr::new("--db"),
            scratch.0.as_os_str(),
            OsStr::new("--bind"),
            OsStr::new("0.0.0.0:8080"),
        ])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&output.stderr).contains("loopback"));
}

struct Server(Child);

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn http(address: SocketAddr, token: &str, body: &[u8]) -> (u16, Value) {
    let mut stream = TcpStream::connect_timeout(&address, Duration::from_secs(3)).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    write!(stream, "POST /v1/execute HTTP/1.1\r\nHost: {address}\r\nAuthorization: Bearer {token}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len()).unwrap();
    stream.write_all(body).unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).unwrap();
    let (headers, body) = response.split_once("\r\n\r\n").unwrap();
    let status = headers.split(' ').nth(1).unwrap().parse().unwrap();
    (status, serde_json::from_str(body).unwrap())
}

#[test]
fn authenticated_http_and_cli_share_actual_case_result() {
    let scratch = Scratch::new();
    let token = "test-only-constant-not-a-secret-0123456789";
    let child = Command::new(env!("CARGO_BIN_EXE_minidora-rust"))
        .args([
            "serve",
            "--db",
            scratch.0.to_str().unwrap(),
            "--actor",
            "owner",
            "--bind",
            "127.0.0.1:0",
        ])
        .env("MINIDORA_API_TOKEN", token)
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut server = Server(child);
    let stderr = server.0.stderr.take().unwrap();
    let (sender, receiver) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut line = String::new();
        let outcome = BufReader::new(stderr).read_line(&mut line).map(|_| line);
        let _ = sender.send(outcome);
    });
    let line = receiver
        .recv_timeout(Duration::from_secs(10))
        .expect("server startup timeout")
        .unwrap();
    let address: SocketAddr = line
        .split("listening on ")
        .nth(1)
        .expect("server readiness line")
        .split(';')
        .next()
        .unwrap()
        .parse()
        .unwrap();
    let request =
        minidora_domain::RequestEnvelope::new("normal-route", "http-input", "owner", "16/4");
    let body = serde_json::to_vec(&request).unwrap();
    assert_eq!(http(address, "incorrect", &body).0, 401);
    let (status, api_record) = http(address, token, &body);
    assert_eq!(status, 200);
    assert_eq!(api_record["lifecycle"], "COMMIT");
    assert_eq!(api_record["calculation"]["exact_value"], "4");
    let explanation = record(&execute(&scratch.0, "owner", &["詳しく説明して"]));
    assert_eq!(explanation["lifecycle"], "COMMIT");
    assert!(
        explanation["dependencies"]
            .as_array()
            .unwrap()
            .contains(&api_record["id"])
    );
}

fn stopped(output: &Output, exit_code: i32, state: &str) -> Value {
    assert_eq!(
        output.status.code(),
        Some(exit_code),
        "stderr={} stdout={}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    let result: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(result["lifecycle"], state);
    assert!(!result["reason"].as_str().unwrap().is_empty());
    result
}

#[test]
fn decimal_statistics_are_exact_and_retain_actual_observations() {
    let scratch = Scratch::new();
    let result = record(&execute(&scratch.0, "owner", &["統計 [0.1,0.2,0.3]"]));
    assert_eq!(result["guarantee"], "COMPUTED");
    let statistics: Value = serde_json::from_str(result["text"].as_str().unwrap()).unwrap();
    assert_eq!(statistics["mean"], "1/5");
    assert_eq!(statistics["median"], "1/5");
    assert_eq!(statistics["min"], "1/10");
    assert_eq!(statistics["max"], "3/10");
    assert_eq!(statistics["sum"], "3/5");
    assert_eq!(statistics["population_variance"], "1/150");
    assert_eq!(statistics["sample_variance"], "1/100");
    assert_eq!(statistics["count"], 3);
    assert_eq!(
        statistics["sorted_values"],
        serde_json::json!(["1/10", "1/5", "3/10"])
    );
    assert_eq!(
        statistics["input_values"],
        serde_json::json!(["0.1", "0.2", "0.3"])
    );
    assert!(
        statistics["scope"]
            .as_str()
            .unwrap()
            .contains("supplied values")
    );
}

#[test]
fn probability_normalization_reports_exact_intermediate_values_on_the_normal_route() {
    let scratch = Scratch::new();
    let japanese = record(&execute(
        &scratch.0,
        "owner",
        &["--input", "normalize-ja", "確率分布 [1,2,3] を正規化して"],
    ));
    assert_eq!(japanese["lifecycle"], "COMMIT");
    assert_eq!(japanese["guarantee"], "COMPUTED");
    let japanese_report: Value = serde_json::from_str(japanese["text"].as_str().unwrap()).unwrap();
    assert_eq!(
        japanese_report["input_weights"],
        serde_json::json!(["1", "2", "3"])
    );
    assert_eq!(japanese_report["total_weight"], "6");
    assert_eq!(
        japanese_report["normalized_probabilities"],
        serde_json::json!(["1/6", "1/3", "1/2"])
    );
    assert_eq!(japanese_report["probability_sum"], "1");

    let english = record(&execute(
        &scratch.0,
        "owner",
        &["--input", "normalize-en", "normalize-probabilities [2,2]"],
    ));
    let english_report: Value = serde_json::from_str(english["text"].as_str().unwrap()).unwrap();
    assert_eq!(english_report["total_weight"], "4");
    assert_eq!(
        english_report["normalized_probabilities"],
        serde_json::json!(["1/2", "1/2"])
    );
    assert_eq!(english_report["probability_sum"], "1");

    let zero_total = stopped(
        &execute(
            &scratch.0,
            "owner",
            &["--input", "normalize-zero", "normalize-probabilities [0,0]"],
        ),
        2,
        "SUSPEND",
    );
    assert!(
        zero_total["reason"]
            .as_str()
            .unwrap()
            .contains("ZERO_PROBABILITY_WEIGHT_TOTAL")
    );
}

#[test]
fn finite_expectation_and_variance_report_exact_intermediate_values() {
    let scratch = Scratch::new();
    let japanese = record(&execute(
        &scratch.0,
        "owner",
        &[
            "--input",
            "expected-value-ja",
            "期待値 [-1,3] 重み [1,3] を計算して",
        ],
    ));
    let japanese_report: Value = serde_json::from_str(japanese["text"].as_str().unwrap()).unwrap();
    assert_eq!(japanese["lifecycle"], "COMMIT");
    assert_eq!(japanese["guarantee"], "COMPUTED");
    assert_eq!(japanese_report["total_weight"], "4");
    assert_eq!(
        japanese_report["normalized_probabilities"],
        serde_json::json!(["1/4", "3/4"])
    );
    assert_eq!(japanese_report["probability_sum"], "1");
    assert_eq!(
        japanese_report["expected_value_contributions"],
        serde_json::json!(["-1/4", "9/4"])
    );
    assert_eq!(japanese_report["expected_value"], "2");
    assert_eq!(
        japanese_report["variance_contributions"],
        serde_json::json!(["9/4", "3/4"])
    );
    assert_eq!(japanese_report["variance"], "3");

    let english = record(&execute(
        &scratch.0,
        "owner",
        &[
            "--input",
            "expected-value-en",
            "expected-value [0,4] weights [3,1]",
        ],
    ));
    let english_report: Value = serde_json::from_str(english["text"].as_str().unwrap()).unwrap();
    assert_eq!(
        english_report["normalized_probabilities"],
        serde_json::json!(["3/4", "1/4"])
    );
    assert_eq!(
        english_report["expected_value_contributions"],
        serde_json::json!(["0", "1"])
    );
    assert_eq!(english_report["expected_value"], "1");
    assert_eq!(
        english_report["variance_contributions"],
        serde_json::json!(["3/4", "9/4"])
    );
    assert_eq!(english_report["variance"], "3");
}

#[test]
fn finite_event_probability_reports_membership_and_exact_mass_on_the_normal_route() {
    let scratch = Scratch::new();
    let japanese = record(&execute(
        &scratch.0,
        "owner",
        &[
            "--input",
            "event-probability-ja",
            "事象確率 [0,1,1,2] 重み [1,2,3,4] 事象値 [1,2,2] を計算して",
        ],
    ));
    let japanese_report: Value = serde_json::from_str(japanese["text"].as_str().unwrap()).unwrap();
    assert_eq!(japanese["lifecycle"], "COMMIT");
    assert_eq!(japanese["guarantee"], "COMPUTED");
    assert_eq!(japanese_report["total_weight"], "10");
    assert_eq!(
        japanese_report["normalized_probabilities"],
        serde_json::json!(["1/10", "1/5", "3/10", "2/5"])
    );
    assert_eq!(japanese_report["probability_sum"], "1");
    assert_eq!(
        japanese_report["event_membership"],
        serde_json::json!([false, true, true, true])
    );
    assert_eq!(
        japanese_report["event_probability_contributions"],
        serde_json::json!(["0", "1/5", "3/10", "2/5"])
    );
    assert_eq!(japanese_report["event_probability"], "9/10");

    let english = record(&execute(
        &scratch.0,
        "owner",
        &[
            "--input",
            "event-probability-en",
            "event-probability [1,2,3] weights [1,2,1] values [1]",
        ],
    ));
    let english_report: Value = serde_json::from_str(english["text"].as_str().unwrap()).unwrap();
    assert_eq!(
        english_report["normalized_probabilities"],
        serde_json::json!(["1/4", "1/2", "1/4"])
    );
    assert_eq!(
        english_report["event_membership"],
        serde_json::json!([true, false, false])
    );
    assert_eq!(
        english_report["event_probability_contributions"],
        serde_json::json!(["1/4", "0", "0"])
    );
    assert_eq!(english_report["event_probability"], "1/4");
}

#[test]
fn polynomial_evaluation_reports_each_exact_contribution_on_the_normal_route() {
    let scratch = Scratch::new();
    let japanese = record(&execute(
        &scratch.0,
        "owner",
        &["--input", "polynomial-ja", "評価 2*x^2-3*x+1 代入 x=2"],
    ));
    let japanese_report: Value = serde_json::from_str(japanese["text"].as_str().unwrap()).unwrap();
    assert_eq!(japanese["lifecycle"], "COMMIT");
    assert_eq!(japanese["guarantee"], "COMPUTED");
    assert_eq!(japanese_report["bindings"], serde_json::json!({"x":"2"}));
    assert_eq!(japanese_report["value"], "3");
    assert_eq!(
        japanese_report["terms"],
        serde_json::json!([
            {"powers":{"x":2},"coefficient":"2","monomial_value":"4","contribution":"8"},
            {"powers":{"x":1},"coefficient":"-3","monomial_value":"2","contribution":"-6"},
            {"powers":{},"coefficient":"1","monomial_value":"1","contribution":"1"}
        ])
    );

    let english = record(&execute(
        &scratch.0,
        "owner",
        &["--input", "polynomial-en", "evaluate x^2+2*x+y at x=3,y=2"],
    ));
    let english_report: Value = serde_json::from_str(english["text"].as_str().unwrap()).unwrap();
    assert_eq!(
        english_report["bindings"],
        serde_json::json!({"x":"3", "y":"2"})
    );
    assert_eq!(english_report["value"], "17");
    assert_eq!(
        english_report["terms"],
        serde_json::json!([
            {"powers":{"x":2},"coefficient":"1","monomial_value":"9","contribution":"9"},
            {"powers":{"x":1},"coefficient":"2","monomial_value":"3","contribution":"6"},
            {"powers":{"y":1},"coefficient":"1","monomial_value":"2","contribution":"2"}
        ])
    );
}

#[test]
fn mixed_length_units_are_converted_exactly_on_the_normal_route() {
    let scratch = Scratch::new();
    let result = record(&execute(
        &scratch.0,
        "owner",
        &["--input", "mixed-length-statistics", "統計 [1 km, 500 m]"],
    ));
    assert_eq!(result["lifecycle"], "COMMIT");
    let statistics: Value = serde_json::from_str(result["text"].as_str().unwrap()).unwrap();
    assert_eq!(statistics["unit"], "m");
    assert_eq!(statistics["input_values"], serde_json::json!(["1", "500"]));
    assert_eq!(statistics["input_units"], serde_json::json!(["km", "m"]));
    assert_eq!(
        statistics["normalized_values"],
        serde_json::json!(["1000", "500"])
    );
    assert_eq!(statistics["sum"], "1500");
    assert_eq!(statistics["mean"], "750");
    assert_eq!(statistics["variance_unit"], "(m)^2");
}

#[test]
fn incompatible_mixed_units_suspend_without_adopting_a_numeric_result() {
    let scratch = Scratch::new();
    let result = stopped(
        &execute(
            &scratch.0,
            "owner",
            &["--input", "incompatible-statistics", "統計 [1 km, 5 s]"],
        ),
        2,
        "SUSPEND",
    );
    assert!(result["reason"].as_str().unwrap().contains("UNIT_MISMATCH"));
    assert!(result["content_plan"].is_null());
    assert!(result["receipts"].as_array().unwrap().is_empty());
}

#[test]
fn linear_equation_result_preserves_matrix_and_exact_solution() {
    let scratch = Scratch::new();
    let result = record(&execute(
        &scratch.0,
        "owner",
        &["連立方程式 [[2,1],[1,-1]] = [5,1]"],
    ));
    let solution: Value = serde_json::from_str(result["text"].as_str().unwrap()).unwrap();
    assert_eq!(result["guarantee"], "COMPUTED");
    assert_eq!(solution["status"], "UNIQUE");
    assert_eq!(solution["solution"], serde_json::json!(["2", "1"]));
    assert_eq!(
        solution["original_coefficients"],
        serde_json::json!([["2", "1"], ["1", "-1"]])
    );
    assert_eq!(solution["original_rhs"], serde_json::json!(["5", "1"]));
    assert!(!solution["operations"].as_array().unwrap().is_empty());
}

#[test]
fn explicit_linear_and_quadratic_equations_reach_the_normal_cli_route() {
    let scratch = Scratch::new();
    let linear = record(&execute(
        &scratch.0,
        "owner",
        &["--input", "linear-rational", "一次方程式 2*x+1=0 を解いて"],
    ));
    assert_eq!(linear["lifecycle"], "COMMIT");
    assert_eq!(linear["guarantee"], "COMPUTED");
    let report: Value = serde_json::from_str(linear["text"].as_str().unwrap()).unwrap();
    assert_eq!(report["kind"], "UNIQUE_SOLUTION");
    assert_eq!(report["exact_solution"], "-1/2");
    assert_eq!(report["coefficients"], serde_json::json!(["2", "1"]));

    let identity = record(&execute(
        &scratch.0,
        "owner",
        &["--input", "linear-identity", "solve-linear x=x"],
    ));
    let identity_report: Value = serde_json::from_str(identity["text"].as_str().unwrap()).unwrap();
    assert_eq!(identity_report["kind"], "ALL_REAL_VALUES");
    assert!(identity_report["exact_solution"].is_null());

    let contradiction = record(&execute(
        &scratch.0,
        "owner",
        &["--input", "linear-contradiction", "solve-linear x+1=x+2"],
    ));
    let contradiction_report: Value =
        serde_json::from_str(contradiction["text"].as_str().unwrap()).unwrap();
    assert_eq!(contradiction_report["kind"], "NO_SOLUTION");

    let unsupported = stopped(
        &execute(
            &scratch.0,
            "owner",
            &["--input", "linear-degree-rejection", "solve-linear x^2=1"],
        ),
        2,
        "SUSPEND",
    );
    assert!(
        unsupported["reason"]
            .as_str()
            .unwrap()
            .starts_with("LINEAR_EQUATION_DEGREE:")
    );

    let quadratic = record(&execute(
        &scratch.0,
        "owner",
        &[
            "--input",
            "quadratic-rational",
            "二次方程式 2*x^2-3*x+1=0 を解いて",
        ],
    ));
    let quadratic_report: Value =
        serde_json::from_str(quadratic["text"].as_str().unwrap()).unwrap();
    assert_eq!(quadratic_report["root_kind"], "TWO_RATIONAL_REAL_ROOTS");
    assert_eq!(quadratic_report["real_roots"][0]["exact_value"], "1/2");
    assert_eq!(quadratic_report["real_roots"][1]["exact_value"], "1");
}

#[test]
fn conditional_premises_form_reusable_scoped_proof_across_restarts_and_retract() {
    let scratch = Scratch::new();
    stopped(
        &execute(&scratch.0, "owner", &["隼は生物ですか"]),
        2,
        "SUSPEND",
    );
    let first_rule = record(&execute(
        &scratch.0,
        "owner",
        &["--input", "first-rule", "もしXは鳥ならXは動物"],
    ));
    let first_rule_id = first_rule["dependencies"][0].as_str().unwrap();
    assert_eq!(first_rule["guarantee"], "USER_PREMISE");
    record(&execute(&scratch.0, "owner", &["もしXは動物ならXは生物"]));
    let source = record(&execute(&scratch.0, "owner", &["隼は鳥"]));
    assert_eq!(source["guarantee"], "USER_PREMISE");
    let derived = record(&execute(&scratch.0, "owner", &["隼は生物ですか"]));
    assert_eq!(derived["guarantee"], "DERIVED");
    assert!(
        derived["dependencies"]
            .as_array()
            .unwrap()
            .contains(&Value::String(first_rule_id.into()))
    );

    // A different subject arrives through a fresh process and ordinary input;
    // neither its answer nor an action/result is supplied to the engine.
    record(&execute(&scratch.0, "owner", &["梟は鳥"]));
    let unseen = record(&execute(&scratch.0, "owner", &["梟は生物ですか"]));
    assert_eq!(unseen["guarantee"], "DERIVED");
    let counterexample = record(&execute(&scratch.0, "owner", &["梟は鳥ではない"]));
    let counterexample_id = counterexample["dependencies"][0].as_str().unwrap();
    stopped(
        &execute(&scratch.0, "owner", &["梟は生物ですか"]),
        2,
        "SUSPEND",
    );
    record(&execute(
        &scratch.0,
        "owner",
        &[&format!("{counterexample_id}を無効化して")],
    ));
    assert_eq!(
        record(&execute(&scratch.0, "owner", &["梟は生物ですか"]))["guarantee"],
        "DERIVED"
    );
    record(&execute(
        &scratch.0,
        "owner",
        &[&format!("{first_rule_id}を無効化して")],
    ));
    stopped(
        &execute(&scratch.0, "owner", &["梟は生物ですか"]),
        2,
        "SUSPEND",
    );
    let historical = record(&execute(
        &scratch.0,
        "owner",
        &["--input", "first-rule", "もしXは鳥ならXは動物"],
    ));
    assert_eq!(
        historical, first_rule,
        "invalidation must retain the original rule result"
    );
}

#[test]
fn agreeing_examples_do_not_promote_a_universal_empirical_rule() {
    let scratch = Scratch::new();
    for premise in ["甲は鳥", "甲は青色", "乙は鳥", "乙は青色", "丙は鳥"] {
        let source = record(&execute(&scratch.0, "owner", &[premise]));
        assert_eq!(source["guarantee"], "USER_PREMISE");
    }
    let absent = stopped(
        &execute(&scratch.0, "owner", &["丙は青色ですか"]),
        2,
        "SUSPEND",
    );
    assert!(
        absent["reason"]
            .as_str()
            .unwrap()
            .contains("MISSING_PREMISES")
    );
}

#[test]
fn compound_conditions_and_extra_deliveries_are_not_partially_committed() {
    let scratch = Scratch::new();
    for text in [
        "もしXは鳥かつ青ならXは動物",
        "2+3を計算して送信して",
        "統計 [1,2] を計算しないで",
        "連立方程式 [[1]] = [2] を解いて送信して",
    ] {
        let result = stopped(&execute(&scratch.0, "owner", &[text]), 2, "SUSPEND");
        assert!(
            result["content_plan"].is_null(),
            "unsupported request must not adopt a partial content plan: {text}"
        );
        assert!(result["receipts"].as_array().unwrap().is_empty());
    }
}

#[test]
fn input_limit_failure_retains_original_identity_and_durable_terminal_result() {
    let scratch = Scratch::new();
    let args = [
        "--input",
        "oversized-original",
        "--version",
        "7",
        "--max-input-bytes",
        "2",
        "2+3",
    ];
    let failed = stopped(&execute(&scratch.0, "owner", &args), 4, "FAIL");
    assert_eq!(failed["input_id"], "oversized-original");
    assert_eq!(failed["input_version"], 7);
    assert!(failed["reason"].as_str().unwrap().contains("INPUT_LIMIT"));
    let retry = stopped(&execute(&scratch.0, "owner", &args), 4, "FAIL");
    assert_eq!(failed, retry);
}

#[test]
fn existing_file_conflict_preserves_bytes_and_does_not_close_adopted_goal() {
    let scratch = Scratch::new();
    let target = scratch.0.join("existing.txt");
    let original = b"pre-existing user content\n";
    std::fs::write(&target, original).unwrap();
    let result = stopped(
        &execute(
            &scratch.0,
            "owner",
            &[
                "--output",
                target.to_str().unwrap(),
                "--allow-write",
                target.to_str().unwrap(),
                "12*11",
            ],
        ),
        2,
        "SUSPEND",
    );
    assert_eq!(std::fs::read(&target).unwrap(), original);
    assert!(
        !result["content_plan"].is_null(),
        "computed content remains adopted despite required effect failure"
    );
    assert!(result["text"].as_str().unwrap().contains("132"));
    assert!(
        result["receipts"]
            .as_array()
            .unwrap()
            .iter()
            .all(|receipt| receipt["phase"] != "CONFIRMED")
    );
}

#[test]
fn insufficient_action_budget_cannot_reach_a_requested_file_effect() {
    let scratch = Scratch::new();
    let target = scratch.0.join("unreached.txt");
    let result = stopped(
        &execute(
            &scratch.0,
            "owner",
            &[
                "--max-actions",
                "1",
                "--output",
                target.to_str().unwrap(),
                "--allow-write",
                target.to_str().unwrap(),
                "31*7",
            ],
        ),
        2,
        "SUSPEND",
    );
    assert!(!target.exists());
    assert!(result["receipts"].as_array().unwrap().is_empty());
    let reason = result["reason"].as_str().unwrap();
    assert!(reason.contains("RESOURCE") || reason.contains("BUDGET"));
}

#[test]
fn invalid_relation_stops_durably_without_wedging_later_inputs() {
    let scratch = Scratch::new();
    let invalid = execute(
        &scratch.0,
        "owner",
        &["--input", "unbound-rule", "もしXは鳥ならYは動物"],
    );
    assert!(matches!(invalid.status.code(), Some(2..=4)));
    let result: Value = serde_json::from_slice(&invalid.stdout)
        .expect("domain failure must have a typed result record");
    assert!(matches!(
        result["lifecycle"].as_str(),
        Some("SUSPEND" | "REJECT" | "FAIL")
    ));
    assert_eq!(result["input_id"], "unbound-rule");
    let after = record(&execute(&scratch.0, "owner", &["3+4"]));
    assert_eq!(after["calculation"]["exact_value"], "7");
}

#[test]
fn reconcile_of_committed_file_reuses_original_result_without_writing_again() {
    let scratch = Scratch::new();
    let target = scratch.0.join("once-only.txt");
    let original = record(&execute(
        &scratch.0,
        "owner",
        &[
            "--input",
            "original-operation",
            "--output",
            target.to_str().unwrap(),
            "--allow-write",
            target.to_str().unwrap(),
            "7*11",
        ],
    ));
    let bytes = std::fs::read(&target).unwrap();
    let modified = std::fs::metadata(&target).unwrap().modified().unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_minidora-rust"))
        .args([
            "reconcile",
            "--db",
            scratch.0.to_str().unwrap(),
            "--case",
            "normal-route",
            "--actor",
            "owner",
            "--json",
        ])
        .output()
        .unwrap();
    let reconciled = record(&output);
    assert_eq!(
        reconciled, original,
        "reconciliation must preserve the original durable result and operation identity"
    );
    assert_eq!(std::fs::read(&target).unwrap(), bytes);
    assert_eq!(
        std::fs::metadata(&target).unwrap().modified().unwrap(),
        modified
    );
}

#[test]
fn csv_selection_retains_exact_source_newlines_and_decoded_cell_content() {
    let scratch = Scratch::new();
    let csv = "name,note\r\nalpha,\"line one\nline two\"\r\nbeta,\"say \"\"hi\"\"\"\r\n";
    let request = format!("CSVの列「note」を選択して:\r\n{csv}");
    let result = record(&execute(&scratch.0, "owner", &[&request]));
    let selected: Value = serde_json::from_str(result["text"].as_str().unwrap()).unwrap();
    assert_eq!(
        selected,
        serde_json::json!(["line one\nline two", "say \"hi\""])
    );
    assert_eq!(result["guarantee"], "COMPUTED");
    assert!(!result["dependencies"].as_array().unwrap().is_empty());
    let inspected = Command::new(env!("CARGO_BIN_EXE_minidora-rust"))
        .args([
            "inspect",
            "--db",
            scratch.0.to_str().unwrap(),
            "--case",
            "normal-route",
            "--actor",
            "owner",
            "--json",
        ])
        .output()
        .unwrap();
    let revision = record(&inspected);
    assert_eq!(revision["bundle"]["request"]["text"], request);
    assert_eq!(revision["bundle"]["meaning"]["intent"]["document"], csv);
}

#[test]
fn grouped_csv_statistics_report_exact_results_and_source_spans_on_the_normal_route() {
    let csv = "地域,売上\r\n北,0.1\r\n北,0.3\r\n南,1.0\r\n南,2.0\r\n";
    for (actor, header) in [
        ("japanese", "CSVの「地域」ごとに「売上」の統計量を計算して:"),
        ("english", "csv-group-statistics \"地域\" \"売上\":"),
        ("uppercase", "CSV-GROUP-STATISTICS \"地域\" \"売上\":"),
    ] {
        let scratch = Scratch::new();
        let request = format!("{header}\n{csv}");
        let result = record(&execute(&scratch.0, actor, &[&request]));
        assert_eq!(result["lifecycle"], "COMMIT");
        assert_eq!(result["guarantee"], "COMPUTED");
        assert_eq!(result["dependencies"].as_array().unwrap().len(), 1);
        assert_eq!(result["content_plan"]["text"], result["text"]);
        assert_eq!(
            result["content_plan"]["required_units"],
            serde_json::json!([result["text"]])
        );
        assert_eq!(result["render"]["status"], "RENDERED");
        assert_eq!(
            result["render"]["actual_sha256"].as_str().unwrap().len(),
            64
        );

        let report: Value = serde_json::from_str(result["text"].as_str().unwrap()).unwrap();
        assert_eq!(report["group_column"], "地域");
        assert_eq!(report["value_column"], "売上");
        assert_eq!(report["source_sha256"].as_str().unwrap().len(), 64);
        let groups = report["groups"].as_array().unwrap();
        assert_eq!(groups.len(), 2);
        assert_eq!(groups[0]["value"], "北");
        assert_eq!(
            groups[0]["source_record_numbers"],
            serde_json::json!([1, 2])
        );
        assert_eq!(groups[1]["value"], "南");
        assert_eq!(
            groups[1]["source_record_numbers"],
            serde_json::json!([3, 4])
        );

        let north = &groups[0]["statistics"];
        assert_eq!(north["count"], 2);
        assert_eq!(north["sum"], "2/5");
        assert_eq!(north["mean"], "1/5");
        assert_eq!(north["median"], "1/5");
        assert_eq!(north["min"], "1/10");
        assert_eq!(north["max"], "3/10");
        assert_eq!(north["population_variance"], "1/100");
        assert_eq!(north["sample_variance"], "1/50");

        let south = &groups[1]["statistics"];
        assert_eq!(south["count"], 2);
        assert_eq!(south["sum"], "3");
        assert_eq!(south["mean"], "3/2");
        assert_eq!(south["median"], "3/2");
        assert_eq!(south["min"], "1");
        assert_eq!(south["max"], "2");
        assert_eq!(south["population_variance"], "1/4");
        assert_eq!(south["sample_variance"], "1/2");

        let spans = report["source_spans"].as_array().unwrap();
        assert_eq!(spans.len(), 10);
        for span in spans {
            let start = span["source_start"].as_u64().unwrap() as usize;
            let end = span["source_end"].as_u64().unwrap() as usize;
            let source_text = span["source_text"].as_str().unwrap();
            assert!(start < end && end <= csv.len());
            assert_eq!(&csv.as_bytes()[start..end], source_text.as_bytes());
        }
        let northern_key_spans = spans
            .iter()
            .filter(|span| span["kind"] == "CSV_GROUP_MEMBER" && span["decoded_value"] == "北")
            .count();
        assert_eq!(northern_key_spans, 2);

        let inspected = Command::new(env!("CARGO_BIN_EXE_minidora-rust"))
            .args([
                "inspect",
                "--db",
                scratch.0.to_str().unwrap(),
                "--case",
                "normal-route",
                "--actor",
                actor,
                "--json",
            ])
            .output()
            .unwrap();
        let revision = record(&inspected);
        assert_eq!(revision["bundle"]["request"]["text"], request);
        assert_eq!(revision["bundle"]["meaning"]["intent"]["document"], csv);
        assert_eq!(revision["result"]["dependencies"], result["dependencies"]);
    }
}

#[test]
fn json_table_group_statistics_preserve_exact_values_and_original_lexemes() {
    let document = r#"{"columns":["注記","売上","地域"],"rows":[["非数値","0.1","北"],["未選択","1.0","南"],["文字列","0.3","\u5317"],["別の注記","2.0","南"]]}"#;
    for (actor, header) in [
        (
            "japanese",
            "JSON表の「地域」ごとに「売上」の統計量を計算して:",
        ),
        ("english", "json-table-group-statistics \"地域\" \"売上\":"),
        (
            "mixed-case",
            "JsOn-TaBlE-GrOuP-StAtIsTiCs \"地域\" \"売上\":",
        ),
    ] {
        let scratch = Scratch::new();
        let request = format!("{header}\n{document}");
        let result = record(&execute(&scratch.0, actor, &[&request]));
        assert_eq!(result["lifecycle"], "COMMIT");
        assert_eq!(result["guarantee"], "COMPUTED");
        assert_eq!(result["dependencies"].as_array().unwrap().len(), 1);
        assert_eq!(result["content_plan"]["text"], result["text"]);
        assert_eq!(result["render"]["status"], "RENDERED");
        assert_eq!(
            result["render"]["actual_sha256"],
            minidora_domain::digest(result["text"].as_str().unwrap().as_bytes())
        );
        let report: Value = serde_json::from_str(result["text"].as_str().unwrap()).unwrap();
        assert_eq!(
            report["source_sha256"],
            minidora_domain::digest(document.as_bytes())
        );
        assert_eq!(report["group_by"], "地域");
        assert_eq!(report["group_by_index"], 2);
        assert_eq!(report["value_column"], "売上");
        assert_eq!(report["value_column_index"], 1);
        assert_eq!(report["row_count"], 4);
        let check_span = |span: &Value, expected: &str| {
            let start = span["source_start"].as_u64().unwrap() as usize;
            let end = span["source_end"].as_u64().unwrap() as usize;
            assert!(start < end && end <= document.len());
            let raw = &document.as_bytes()[start..end];
            assert_eq!(serde_json::from_slice::<String>(raw).unwrap(), expected);
            assert_eq!(span["source_sha256"], minidora_domain::digest(raw));
        };
        check_span(&report["group_by_source"], "地域");
        check_span(&report["value_column_source"], "売上");
        let groups = report["groups"].as_array().unwrap();
        assert_eq!(groups.len(), 2);
        for (index, key, rows, values, expected) in [
            (
                0,
                "北",
                [0, 2],
                ["0.1", "0.3"],
                ["2/5", "1/5", "1/5", "1/10", "3/10", "1/100", "1/50"],
            ),
            (
                1,
                "南",
                [1, 3],
                ["1.0", "2.0"],
                ["3", "3/2", "3/2", "1", "2", "1/4", "1/2"],
            ),
        ] {
            let group = &groups[index];
            assert_eq!(group["value"], key);
            assert_eq!(group["source_row_indices"], serde_json::json!(rows));
            assert_eq!(group["statistics"]["count"], 2);
            for (field, value) in [
                "sum",
                "mean",
                "median",
                "min",
                "max",
                "population_variance",
                "sample_variance",
            ]
            .into_iter()
            .zip(expected)
            {
                assert_eq!(group["statistics"][field], value);
            }
            let key_spans = group["group_source_spans"].as_array().unwrap();
            let value_spans = group["value_source_spans"].as_array().unwrap();
            assert_eq!(key_spans.len(), 2);
            assert_eq!(value_spans.len(), 2);
            for spans in [key_spans, value_spans] {
                assert!(spans.windows(2).all(|pair| {
                    pair[0]["source_end"].as_u64().unwrap()
                        <= pair[1]["source_start"].as_u64().unwrap()
                }));
            }
            for (span, value) in value_spans.iter().zip(values) {
                check_span(span, value);
            }
            for span in key_spans {
                check_span(span, key);
            }
        }
        let escaped = &groups[0]["group_source_spans"][1];
        let start = escaped["source_start"].as_u64().unwrap() as usize;
        let end = escaped["source_end"].as_u64().unwrap() as usize;
        assert_eq!(&document[start..end], r#""\u5317""#);

        let inspected = Command::new(env!("CARGO_BIN_EXE_minidora-rust"))
            .args([
                "inspect",
                "--db",
                scratch.0.to_str().unwrap(),
                "--case",
                "normal-route",
                "--actor",
                actor,
                "--json",
            ])
            .output()
            .unwrap();
        let revision = record(&inspected);
        assert_eq!(revision["bundle"]["request"]["text"], request);
        assert_eq!(
            revision["bundle"]["meaning"]["intent"]["document"],
            document
        );
        assert_eq!(revision["result"], result);
    }
}

#[test]
fn json_table_group_statistics_mixed_values_suspend_with_a_durable_typed_error() {
    let scratch = Scratch::new();
    let document = r#"{"columns":["region","sales"],"rows":[["north","1 m"],["north","2"]]}"#;
    let request = format!("JSON表の「region」ごとに「sales」の統計量を計算して:\n{document}");
    let result = stopped(&execute(&scratch.0, "owner", &[&request]), 2, "SUSPEND");
    assert!(
        result["reason"]
            .as_str()
            .unwrap()
            .contains("JSON_TABLE_GROUP_STATISTICS_MIXED_VALUES")
    );
    assert_eq!(result["guarantee"], "UNKNOWN");
    assert_eq!(result["dependencies"], serde_json::json!([]));
    let inspected = Command::new(env!("CARGO_BIN_EXE_minidora-rust"))
        .args([
            "inspect",
            "--db",
            scratch.0.to_str().unwrap(),
            "--case",
            "normal-route",
            "--actor",
            "owner",
            "--json",
        ])
        .output()
        .unwrap();
    let revision = record(&inspected);
    assert_eq!(revision["lifecycle"], "SUSPEND");
    assert_eq!(revision["result"], result);
}

#[test]
fn mixed_csv_group_values_suspend_with_a_durable_typed_error_and_no_artifact() {
    let scratch = Scratch::new();
    let csv = "region,sales\r\nnorth,1 m\r\nnorth,2\r\n";
    let request = format!("CSVの「region」ごとに「sales」の統計量を計算して:\n{csv}");
    let result = stopped(&execute(&scratch.0, "owner", &[&request]), 2, "SUSPEND");
    assert!(
        result["reason"]
            .as_str()
            .unwrap()
            .contains("CSV_GROUP_STATISTICS_MIXED_VALUES")
    );
    assert!(result["dependencies"].as_array().unwrap().is_empty());
    assert_eq!(result["guarantee"], "UNKNOWN");

    let inspected = Command::new(env!("CARGO_BIN_EXE_minidora-rust"))
        .args([
            "inspect",
            "--db",
            scratch.0.to_str().unwrap(),
            "--case",
            "normal-route",
            "--actor",
            "owner",
            "--json",
        ])
        .output()
        .unwrap();
    let revision = record(&inspected);
    assert_eq!(revision["lifecycle"], "SUSPEND");
    assert_eq!(revision["result"]["reason"], result["reason"]);
    assert_eq!(revision["result"]["dependencies"], serde_json::json!([]));
}

fn inspect_normal_case(directory: &Path, actor: &str) -> Value {
    record(
        &Command::new(env!("CARGO_BIN_EXE_minidora-rust"))
            .args([
                "inspect",
                "--db",
                directory.to_str().unwrap(),
                "--case",
                "normal-route",
                "--actor",
                actor,
                "--json",
            ])
            .output()
            .unwrap(),
    )
}

const CONVERSION_CSV: &str =
    "名前,値,補足\r\n\"猫,\"\"北\"\"\",001,1e3\r\n\"犬\r\n南\",9007199254740993,\"\"\r\n";
const CONVERSION_VALUES: [[&str; 3]; 3] = [
    ["名前", "値", "補足"],
    ["猫,\"北\"", "001", "1e3"],
    ["犬\r\n南", "9007199254740993", ""],
];
const CONVERSION_CSV_TOKENS: [[&str; 3]; 3] = [
    ["名前", "値", "補足"],
    ["\"猫,\"\"北\"\"\"", "001", "1e3"],
    ["\"犬\r\n南\"", "9007199254740993", "\"\""],
];
const CONVERSION_ESCAPED_JSON: &str = r#"{"rows":[["\u732b,\"北\"","001","1e3"],["犬\r\n南","9007199254740993",""]],"columns":["名前","値","補足"]}"#;

fn normal_document_artifact(
    directory: &Path,
    actor: &str,
    result: &Value,
    request: &str,
    source: &str,
    operation: &str,
) -> Value {
    assert_eq!(result["lifecycle"], "COMMIT");
    assert_eq!(result["guarantee"], "COMPUTED");
    assert_eq!(result["content_plan"]["text"], result["text"]);
    assert_eq!(result["render"]["status"], "RENDERED");
    let text = result["text"].as_str().unwrap();
    assert_eq!(
        result["render"]["actual_sha256"],
        minidora_domain::digest(text.as_bytes())
    );
    let dependencies = result["dependencies"].as_array().unwrap();
    assert_eq!(dependencies.len(), 1);
    let store = minidora_engine::store::Store::open(directory).unwrap();
    let bytes = store
        .get_bytes(actor, "normal-route", dependencies[0].as_str().unwrap())
        .unwrap();
    assert_eq!(minidora_domain::digest(&bytes), dependencies[0]);
    let artifact: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(artifact["operation"], operation);
    assert_eq!(artifact["source_document"], source);
    assert_eq!(
        artifact["source_sha256"],
        minidora_domain::digest(source.as_bytes())
    );
    assert_eq!(artifact["text"], text);
    assert_eq!(
        artifact["output_sha256"],
        minidora_domain::digest(text.as_bytes())
    );
    let revision = inspect_normal_case(directory, actor);
    assert_eq!(revision["bundle"]["request"]["text"], request);
    assert_eq!(revision["bundle"]["meaning"]["intent"]["document"], source);
    assert_eq!(revision["result"], *result);
    artifact
}

// Expected positions come from the known fixture row/cell, including quoted
// empty strings that also occur inside other escaped tokens.
fn conversion_fixture_token(
    text: &str,
    physical_row: usize,
    column: usize,
    json: bool,
    escaped_cat: bool,
) -> (usize, usize) {
    let mut tokens: Vec<String> = if json {
        CONVERSION_VALUES[physical_row]
            .iter()
            .map(|value| serde_json::to_string(value).unwrap())
            .collect()
    } else {
        CONVERSION_CSV_TOKENS[physical_row]
            .iter()
            .map(|value| value.to_string())
            .collect()
    };
    if json && escaped_cat && physical_row == 1 {
        tokens[0] = r#""\u732b,\"北\"""#.into();
    }
    let row_text = if json {
        format!("[{}]", tokens.join(","))
    } else {
        tokens.join(",")
    };
    let start = text.find(&row_text).unwrap()
        + usize::from(json)
        + tokens
            .iter()
            .take(column)
            .map(|value| value.len() + 1)
            .sum::<usize>();
    (start, start + tokens[column].len())
}

fn assert_conversion_fixture_spans(
    artifact: &Value,
    has_header: bool,
    to_json: bool,
    escaped_cat: bool,
) {
    let source = artifact["source_document"].as_str().unwrap();
    let output = artifact["text"].as_str().unwrap();
    let spans = artifact["spans"].as_array().unwrap();
    assert_eq!(spans.len(), 9);
    assert_eq!(artifact["parameters"]["has_header"], has_header.to_string());
    let mut expected = std::collections::BTreeSet::new();
    for physical_row in 0..3 {
        let header = has_header && physical_row == 0;
        let kind = match (to_json, header) {
            (true, true) => "CSV_HEADER",
            (true, false) => "CSV_CELL",
            (false, true) => "JSON_HEADER",
            (false, false) => "JSON_CELL",
        };
        let row = if to_json {
            Some(physical_row)
        } else if header {
            None
        } else {
            Some(physical_row - usize::from(has_header))
        };
        for column in 0..3 {
            expected.insert((kind, row, column));
        }
    }
    let mut seen = std::collections::BTreeSet::new();
    for span in spans {
        let kind = span["kind"].as_str().unwrap();
        let row = span["row"].as_u64().map(|value| value as usize);
        let column = span["column"].as_u64().unwrap() as usize;
        assert!(expected.contains(&(kind, row, column)));
        assert!(seen.insert((kind, row, column)));
        let physical_row = if to_json {
            row.unwrap()
        } else if kind == "JSON_HEADER" {
            0
        } else {
            row.unwrap() + usize::from(has_header)
        };
        let (start, end) =
            conversion_fixture_token(source, physical_row, column, !to_json, escaped_cat);
        assert_eq!(span["source_start"], start);
        assert_eq!(span["source_end"], end);
        assert_eq!(span["source_text"], &source[start..end]);
        assert_eq!(
            span["decoded_value"],
            CONVERSION_VALUES[physical_row][column]
        );
        let (output_start, output_end) =
            conversion_fixture_token(output, physical_row, column, to_json, false);
        assert_eq!(span["output_start"], output_start);
        assert_eq!(span["output_end"], output_end);
    }
    assert_eq!(seen, expected);
}

#[test]
fn table_conversion_csv_roundtrips_use_actual_outputs_in_both_header_modes() {
    for (has_header, headers) in [
        (
            true,
            [
                ("japanese", "CSVをJSONに変換して（先頭行をヘッダーとして）:"),
                ("english", "csv-to-json header:"),
                ("mixed-case", "CsV-To-JsOn HeAdEr:"),
            ],
        ),
        (
            false,
            [
                ("japanese", "CSVをJSONに変換して（ヘッダーなし）:"),
                ("english", "csv-to-json no-header:"),
                ("mixed-case", "CsV-To-JsOn No-HeAdEr:"),
            ],
        ),
    ] {
        for (actor, header) in headers {
            let scratch = Scratch::new();
            let request = format!("{header}\n{CONVERSION_CSV}");
            let converted = record(&execute(&scratch.0, actor, &[&request]));
            let artifact = normal_document_artifact(
                &scratch.0,
                actor,
                &converted,
                &request,
                CONVERSION_CSV,
                "CSV_TO_JSON",
            );
            assert_conversion_fixture_spans(&artifact, has_header, true, false);
            let table: Value = serde_json::from_str(converted["text"].as_str().unwrap()).unwrap();
            assert_eq!(
                table,
                serde_json::json!({
                    "columns": if has_header { serde_json::json!(CONVERSION_VALUES[0]) } else { Value::Null },
                    "rows": if has_header { serde_json::json!(&CONVERSION_VALUES[1..]) } else { serde_json::json!(CONVERSION_VALUES) }
                })
            );
            let actual_json = converted["text"].as_str().unwrap();
            let back_request = format!("json-to-csv:\n{actual_json}");
            let back = record(&execute(&scratch.0, actor, &[&back_request]));
            assert_eq!(back["text"], CONVERSION_CSV);
            let artifact = normal_document_artifact(
                &scratch.0,
                actor,
                &back,
                &back_request,
                actual_json,
                "JSON_TO_CSV",
            );
            assert_conversion_fixture_spans(&artifact, has_header, false, false);
        }
    }
}

#[test]
fn table_conversion_json_escapes_roundtrip_through_actual_csv_output() {
    for (actor, json_header, csv_header) in [
        (
            "japanese",
            "JSONをCSVに変換して:",
            "CSVをJSONに変換して（先頭行をヘッダーとして）:",
        ),
        ("english", "json-to-csv:", "csv-to-json header:"),
        ("mixed-case", "JsOn-To-CsV:", "CsV-To-JsOn HeAdEr:"),
    ] {
        let scratch = Scratch::new();
        let request = format!("{json_header}\n{CONVERSION_ESCAPED_JSON}");
        let converted = record(&execute(&scratch.0, actor, &[&request]));
        assert_eq!(converted["text"], CONVERSION_CSV);
        let artifact = normal_document_artifact(
            &scratch.0,
            actor,
            &converted,
            &request,
            CONVERSION_ESCAPED_JSON,
            "JSON_TO_CSV",
        );
        assert_conversion_fixture_spans(&artifact, true, false, true);
        let actual_csv = converted["text"].as_str().unwrap();
        let back_request = format!("{csv_header}\n{actual_csv}");
        let back = record(&execute(&scratch.0, actor, &[&back_request]));
        let artifact = normal_document_artifact(
            &scratch.0,
            actor,
            &back,
            &back_request,
            actual_csv,
            "CSV_TO_JSON",
        );
        assert_conversion_fixture_spans(&artifact, true, true, false);
        let restored: Value = serde_json::from_str(back["text"].as_str().unwrap()).unwrap();
        assert_eq!(
            restored,
            serde_json::from_str::<Value>(CONVERSION_ESCAPED_JSON).unwrap()
        );
    }
}

#[test]
fn table_conversion_formula_strings_are_inert_json_and_withheld_on_csv_export() {
    for (header, value) in [
        ("値", "=1+1"),
        ("値", "  +cmd"),
        ("値", "\u{feff}-2"),
        ("値", "@command"),
        ("=header", "safe"),
    ] {
        let scratch = Scratch::new();
        let csv = format!("{header}\r\n{value}\r\n");
        let request = format!("CSVをJSONに変換して（先頭行をヘッダーとして）:\n{csv}");
        let converted = record(&execute(&scratch.0, "owner", &[&request]));
        normal_document_artifact(
            &scratch.0,
            "owner",
            &converted,
            &request,
            &csv,
            "CSV_TO_JSON",
        );
        let table: Value = serde_json::from_str(converted["text"].as_str().unwrap()).unwrap();
        assert_eq!(table["columns"], serde_json::json!([header]));
        assert_eq!(table["rows"], serde_json::json!([[value]]));
        let actual_json = converted["text"].as_str().unwrap();
        let export_request = format!("JSONをCSVに変換して:\n{actual_json}");
        let stopped = stopped(
            &execute(&scratch.0, "owner", &[&export_request]),
            2,
            "SUSPEND",
        );
        assert!(
            stopped["reason"]
                .as_str()
                .unwrap()
                .contains("CSV_FORMULA_RISK")
        );
        assert_eq!(stopped["guarantee"], "UNKNOWN");
        assert_eq!(stopped["dependencies"], serde_json::json!([]));
        let revision = inspect_normal_case(&scratch.0, "owner");
        assert_eq!(revision["bundle"]["request"]["text"], export_request);
        assert_eq!(
            revision["bundle"]["meaning"]["intent"]["document"],
            actual_json
        );
        assert_eq!(revision["result"], stopped);
        assert!(
            revision["prior_results"]
                .as_object()
                .unwrap()
                .values()
                .any(|prior| prior == &converted)
        );
    }
}

#[test]
fn table_conversion_schema_errors_and_missing_header_mode_stop_durably() {
    for (header, document, code) in [
        ("csv-to-json header:", "a,b\nx,y\n", "CSV_LINE_ENDING"),
        ("csv-to-json header:", "a,b\r\nx", "CSV_SHAPE"),
        ("csv-to-json:", "a,b\r\nx,y\r\n", "CAPABILITY_UNAVAILABLE"),
        (
            "json-to-csv:",
            r#"{"columns":["a"],"rows":[[1]]}"#,
            "JSON_TABLE_SYNTAX",
        ),
        (
            "json-to-csv:",
            r#"{"columns":["a","\u0061"],"rows":[["x","y"]]}"#,
            "DUPLICATE_COLUMNS",
        ),
        (
            "json-to-csv:",
            r#"{"columns":["a"],"rows":[],"rows":[]}"#,
            "DUPLICATE_JSON_KEY",
        ),
        (
            "json-to-csv:",
            r#"{"columns":["a"],"rows":[["x"]],"extra":0}"#,
            "JSON_TABLE_SHAPE",
        ),
    ] {
        let scratch = Scratch::new();
        let request = format!("{header}\n{document}");
        let result = stopped(&execute(&scratch.0, "owner", &[&request]), 2, "SUSPEND");
        assert!(result["reason"].as_str().unwrap().contains(code));
        assert_eq!(result["guarantee"], "UNKNOWN");
        assert_eq!(result["dependencies"], serde_json::json!([]));
        let revision = inspect_normal_case(&scratch.0, "owner");
        assert_eq!(revision["bundle"]["request"]["text"], request);
        assert_eq!(revision["lifecycle"], "SUSPEND");
        assert_eq!(revision["result"], result);
    }
}

#[test]
fn csv_inner_join_preserves_left_order_and_two_sided_source_locations() {
    let left = "名前,id\r\n\"犬,大\",2\r\n猫,1\r\n狐,4";
    let right = "record_id,分類\r\n1,猫科\r\n3,鳥類\r\n2,イヌ科\r\n";
    let document =
        format!("--- MINIDORA LEFT CSV ---\n{left}\r\n--- MINIDORA RIGHT CSV ---\n{right}");
    for (actor, header) in [
        ("japanese", "CSVを「id」と「record_id」で内部結合して:"),
        ("english", "csv-inner-join \"id\" \"record_id\":"),
        ("uppercase", "CSV-INNER-JOIN \"id\" \"record_id\":"),
    ] {
        let scratch = Scratch::new();
        let request = format!("{header}\n{document}");
        let result = record(&execute(&scratch.0, actor, &[&request]));
        assert_eq!(result["lifecycle"], "COMMIT");
        assert_eq!(result["guarantee"], "COMPUTED");
        assert_eq!(result["render"]["status"], "RENDERED");
        assert_eq!(result["content_plan"]["text"], result["text"]);
        let text = result["text"].as_str().unwrap();
        assert_eq!(
            result["render"]["actual_sha256"],
            minidora_domain::digest(text.as_bytes())
        );
        let joined: Value = serde_json::from_str(text)
            .unwrap_or_else(|error| panic!("invalid joined JSON: {error}; text={text}"));
        assert_eq!(
            joined["columns"],
            serde_json::json!(["left.名前", "left.id", "right.record_id", "right.分類"])
        );
        assert_eq!(
            joined["rows"],
            serde_json::json!([["犬,大", "2", "2", "イヌ科"], ["猫", "1", "1", "猫科"]])
        );
        assert_eq!(
            joined["left_source_record_numbers"],
            serde_json::json!([1, 2])
        );
        assert_eq!(
            joined["right_source_record_numbers"],
            serde_json::json!([3, 1])
        );
        assert_eq!(
            joined["unmatched_left"],
            serde_json::json!([{"key":"4","source_record_number":3}])
        );
        assert_eq!(
            joined["unmatched_right"],
            serde_json::json!([{"key":"3","source_record_number":2}])
        );
        let dependencies = result["dependencies"].as_array().unwrap();
        assert_eq!(dependencies.len(), 1);
        let store = minidora_engine::store::Store::open(&scratch.0).unwrap();
        let bytes = store
            .get_bytes(actor, "normal-route", dependencies[0].as_str().unwrap())
            .unwrap();
        assert_eq!(minidora_domain::digest(&bytes), dependencies[0]);
        let artifact: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(artifact["source_document"], document);
        assert_eq!(
            artifact["source_sha256"],
            minidora_domain::digest(document.as_bytes())
        );
        assert_eq!(artifact["text"], text);
        assert_eq!(
            artifact["output_sha256"],
            minidora_domain::digest(text.as_bytes())
        );
        let spans = artifact["spans"].as_array().unwrap();
        assert_eq!(spans.len(), 14);
        let mut seen = std::collections::BTreeSet::new();
        for span in spans {
            let kind = span["kind"].as_str().unwrap();
            let is_left = kind.contains("_LEFT_");
            assert!(is_left || kind.contains("_RIGHT_"));
            let source = if is_left { left } else { right };
            let base = document.find(source).unwrap();
            let rows = if is_left {
                [["名前", "id"], ["\"犬,大\"", "2"], ["猫", "1"], ["狐", "4"]]
            } else {
                [
                    ["record_id", "分類"],
                    ["1", "猫科"],
                    ["3", "鳥類"],
                    ["2", "イヌ科"],
                ]
            };
            let row = span["row"].as_u64().unwrap() as usize;
            let column = span["column"].as_u64().unwrap() as usize;
            assert!(seen.insert((kind, row, column)));
            let record_text = rows[row].join(",");
            let start = base
                + source.find(&record_text).unwrap()
                + rows[row]
                    .iter()
                    .take(column)
                    .map(|cell| cell.len() + 1)
                    .sum::<usize>();
            let raw = rows[row][column];
            let end = start + raw.len();
            assert_eq!(span["source_start"], start);
            assert_eq!(span["source_end"], end);
            assert_eq!(&document[start..end], raw);
            assert_eq!(span["source_text"], raw);
            let decoded = raw.trim_matches('"');
            assert_eq!(span["decoded_value"], decoded);
            let output_start = span["output_start"].as_u64().unwrap() as usize;
            let output_end = span["output_end"].as_u64().unwrap() as usize;
            let expected_output_start = if kind.contains("_UNMATCHED_") {
                let field = if is_left {
                    "unmatched_left"
                } else {
                    "unmatched_right"
                };
                let fragment = serde_json::to_string(&joined[field][0]).unwrap();
                text.find(&fragment).unwrap()
                    + fragment
                        .find(&serde_json::to_string(decoded).unwrap())
                        .unwrap()
            } else {
                let (array, index) = if kind.ends_with("_HEADER") {
                    (
                        &joined["columns"],
                        if is_left { column } else { 2 + column },
                    )
                } else if is_left {
                    (&joined["rows"][row - 1], column)
                } else {
                    let output_row = match row {
                        3 => 0,
                        1 => 1,
                        _ => panic!("unexpected matched right row"),
                    };
                    (&joined["rows"][output_row], 2 + column)
                };
                let fragment = serde_json::to_string(array).unwrap();
                text.find(&fragment).unwrap()
                    + 1
                    + array
                        .as_array()
                        .unwrap()
                        .iter()
                        .take(index)
                        .map(|value| serde_json::to_string(value).unwrap().len() + 1)
                        .sum::<usize>()
            };
            assert_eq!(output_start, expected_output_start);
            let output_value: String =
                serde_json::from_str(&text[output_start..output_end]).unwrap();
            let expected = if kind.ends_with("_HEADER") {
                format!("{}{decoded}", if is_left { "left." } else { "right." })
            } else {
                decoded.to_string()
            };
            assert_eq!(output_value, expected);
            assert_eq!(
                output_end,
                output_start + serde_json::to_string(&expected).unwrap().len()
            );
        }
        assert_eq!(
            seen,
            std::collections::BTreeSet::from([
                ("CSV_INNER_JOIN_LEFT_HEADER", 0, 0),
                ("CSV_INNER_JOIN_LEFT_HEADER", 0, 1),
                ("CSV_INNER_JOIN_RIGHT_HEADER", 0, 0),
                ("CSV_INNER_JOIN_RIGHT_HEADER", 0, 1),
                ("CSV_INNER_JOIN_LEFT_CELL", 1, 0),
                ("CSV_INNER_JOIN_LEFT_CELL", 1, 1),
                ("CSV_INNER_JOIN_LEFT_CELL", 2, 0),
                ("CSV_INNER_JOIN_LEFT_CELL", 2, 1),
                ("CSV_INNER_JOIN_RIGHT_CELL", 3, 0),
                ("CSV_INNER_JOIN_RIGHT_CELL", 3, 1),
                ("CSV_INNER_JOIN_RIGHT_CELL", 1, 0),
                ("CSV_INNER_JOIN_RIGHT_CELL", 1, 1),
                ("CSV_INNER_JOIN_UNMATCHED_LEFT_KEY", 3, 1),
                ("CSV_INNER_JOIN_UNMATCHED_RIGHT_KEY", 2, 0),
            ])
        );
        let revision = inspect_normal_case(&scratch.0, actor);
        assert_eq!(revision["bundle"]["request"]["text"], request);
        assert_eq!(
            revision["bundle"]["meaning"]["intent"]["document"],
            document
        );
        assert_eq!(revision["result"], result);
    }
}

#[test]
fn json_table_inner_join_preserves_decoded_keys_and_original_source_ranges() {
    let left = r#"{"columns":["名前","id"],"rows":[["犬","北"],["猫","南"],["狐","西"]]}"#;
    let right = r#"{"columns":["record_id","分類"],"rows":[["南","猫科"],["東","鳥類"],["\u5317","イヌ科"]]}"#;
    let document = format!(r#"{{"left":{left},"right":{right}}}"#);
    for (actor, header) in [
        ("japanese", "JSON表を「id」と「record_id」で内部結合して:"),
        ("english", "json-table-inner-join \"id\" \"record_id\":"),
        ("mixed-case", "JsOn-TaBlE-InNeR-JoIn \"id\" \"record_id\":"),
    ] {
        let scratch = Scratch::new();
        let request = format!("{header}\n{document}");
        let result = record(&execute(&scratch.0, actor, &[&request]));
        assert_eq!(result["lifecycle"], "COMMIT");
        assert_eq!(result["guarantee"], "SOURCE_REPORT");
        assert_eq!(result["render"]["status"], "RENDERED");
        assert_eq!(result["content_plan"]["text"], result["text"]);
        assert_eq!(
            result["render"]["actual_sha256"],
            minidora_domain::digest(result["text"].as_str().unwrap().as_bytes())
        );
        assert_eq!(result["dependencies"].as_array().unwrap().len(), 1);
        let joined: Value = serde_json::from_str(result["text"].as_str().unwrap()).unwrap();
        assert_eq!(
            joined["columns"],
            serde_json::json!(["left.名前", "left.id", "right.record_id", "right.分類"])
        );
        assert_eq!(
            joined["rows"],
            serde_json::json!([["犬", "北", "北", "イヌ科"], ["猫", "南", "南", "猫科"]])
        );
        assert_eq!(joined["left_source_row_indices"], serde_json::json!([0, 1]));
        assert_eq!(
            joined["right_source_row_indices"],
            serde_json::json!([2, 0])
        );
        assert_eq!(joined["left_key_column"], "id");
        assert_eq!(joined["left_key_column_index"], 1);
        assert_eq!(joined["right_key_column"], "record_id");
        assert_eq!(joined["right_key_column_index"], 0);
        assert_eq!(
            joined["source_sha256"],
            minidora_domain::digest(document.as_bytes())
        );
        assert_eq!(
            joined["left_sha256"],
            minidora_domain::digest(left.as_bytes())
        );
        assert_eq!(
            joined["right_sha256"],
            minidora_domain::digest(right.as_bytes())
        );
        let check_span = |span: &Value, source: &str, raw: &str, expected: &str| {
            let start = document.find(source).unwrap() + source.find(raw).unwrap();
            let end = start + raw.len();
            assert_eq!(span["source_start"], start);
            assert_eq!(span["source_end"], end);
            let bytes = &document.as_bytes()[start..end];
            assert_eq!(serde_json::from_slice::<String>(bytes).unwrap(), expected);
            assert_eq!(span["source_sha256"], minidora_domain::digest(bytes));
        };
        let column_spans = joined["column_source_spans"].as_array().unwrap();
        assert_eq!(column_spans.len(), 4);
        for (index, source, name) in [
            (0, left, "名前"),
            (1, left, "id"),
            (2, right, "record_id"),
            (3, right, "分類"),
        ] {
            check_span(
                &column_spans[index],
                source,
                &serde_json::to_string(name).unwrap(),
                name,
            );
        }
        assert_eq!(joined["left_key_column_source"], column_spans[1]);
        assert_eq!(joined["right_key_column_source"], column_spans[2]);
        for (side, source, expected_rows) in [
            ("left_cell_source_spans", left, [["犬", "北"], ["猫", "南"]]),
            (
                "right_cell_source_spans",
                right,
                [["北", "イヌ科"], ["南", "猫科"]],
            ),
        ] {
            let rows = joined[side].as_array().unwrap();
            assert_eq!(rows.len(), 2);
            for (spans, expected) in rows.iter().zip(expected_rows) {
                let spans = spans.as_array().unwrap();
                assert_eq!(spans.len(), 2);
                for (span, value) in spans.iter().zip(expected) {
                    let raw = if side == "right_cell_source_spans" && value == "北" {
                        r#""\u5317""#.to_string()
                    } else {
                        serde_json::to_string(value).unwrap()
                    };
                    check_span(span, source, &raw, value);
                }
            }
        }
        for (field, source, key, row) in [
            ("unmatched_left", left, "西", 2),
            ("unmatched_right", right, "東", 1),
        ] {
            let unmatched = joined[field].as_array().unwrap();
            assert_eq!(unmatched.len(), 1);
            assert_eq!(unmatched[0]["key"], key);
            assert_eq!(unmatched[0]["source_row_index"], row);
            check_span(
                &unmatched[0]["key_source"],
                source,
                &serde_json::to_string(key).unwrap(),
                key,
            );
        }
        let revision = inspect_normal_case(&scratch.0, actor);
        assert_eq!(revision["bundle"]["request"]["text"], request);
        assert_eq!(
            revision["bundle"]["meaning"]["intent"]["document"],
            document
        );
        assert_eq!(revision["result"], result);
    }
}

#[test]
fn inner_joins_duplicate_keys_suspend_without_result_dependencies() {
    for duplicate_side in ["left", "right"] {
        let left_rows = if duplicate_side == "left" {
            "1\r\n1"
        } else {
            "1"
        };
        let right_rows = if duplicate_side == "right" {
            "1\r\n1"
        } else {
            "1"
        };
        let csv = format!(
            "--- MINIDORA LEFT CSV ---\nid\r\n{left_rows}\r\n--- MINIDORA RIGHT CSV ---\nrecord_id\r\n{right_rows}\r\n"
        );
        let duplicate = serde_json::json!([["1"], ["1"]]);
        let single = serde_json::json!([["1"]]);
        let json = serde_json::json!({
            "left": {"columns": ["id"], "rows": if duplicate_side == "left" { &duplicate } else { &single }},
            "right": {"columns": ["record_id"], "rows": if duplicate_side == "right" { &duplicate } else { &single }}
        }).to_string();
        for (header, document, code) in [
            (
                "csv-inner-join \"id\" \"record_id\":",
                csv,
                "CSV_INNER_JOIN_DUPLICATE_KEY",
            ),
            (
                "json-table-inner-join \"id\" \"record_id\":",
                json,
                "JSON_TABLE_INNER_JOIN_DUPLICATE_KEY",
            ),
        ] {
            let scratch = Scratch::new();
            let request = format!("{header}\n{document}");
            let result = stopped(&execute(&scratch.0, "owner", &[&request]), 2, "SUSPEND");
            assert!(result["reason"].as_str().unwrap().contains(code));
            assert_eq!(result["guarantee"], "UNKNOWN");
            assert_eq!(result["dependencies"], serde_json::json!([]));
            let revision = inspect_normal_case(&scratch.0, "owner");
            assert_eq!(revision["bundle"]["request"]["text"], request);
            assert_eq!(revision["lifecycle"], "SUSPEND");
            assert_eq!(revision["result"], result);
        }
    }
}

#[test]
fn exact_replace_count_failure_does_not_create_a_partial_file() {
    let scratch = Scratch::new();
    let target = scratch.0.join("must-not-exist.txt");
    let request = "replace \"old\" -> \"new\" count 1:\nold\r\nold\n";
    let result = stopped(
        &execute(
            &scratch.0,
            "owner",
            &[
                "--output",
                target.to_str().unwrap(),
                "--allow-write",
                target.to_str().unwrap(),
                request,
            ],
        ),
        2,
        "SUSPEND",
    );
    assert!(!target.exists());
    assert!(result["content_plan"].is_null());
    assert!(result["receipts"].as_array().unwrap().is_empty());
    assert!(result["reason"].as_str().unwrap().contains("COUNT"));
}

#[test]
fn metadata_search_without_read_grant_is_a_typed_stop_without_an_answer() {
    let scratch = Scratch::new();
    for request in [
        "検索 wikipedia: 重力波",
        "search crossref: \"stellar spectra\"",
        "search europepmc: \"cell signalling\"",
    ] {
        let result = stopped(&execute(&scratch.0, "owner", &[request]), 2, "SUSPEND");
        assert!(
            result["reason"]
                .as_str()
                .unwrap()
                .contains("SEARCH_UNAVAILABLE")
        );
        assert!(result["content_plan"].is_null());
        assert!(result["evidence"].as_array().unwrap().is_empty());
        assert!(result["receipts"].as_array().unwrap().is_empty());
    }
}

#[test]
fn cancelled_effect_reconciliation_preserves_case_cancel_with_no_new_goal() {
    let scratch = Scratch::new();
    let cancelled = stopped(&execute(&scratch.0, "owner", &["キャンセル"]), 5, "CANCEL");
    let ledger = Command::new(env!("CARGO_BIN_EXE_minidora-rust"))
        .args([
            "reconcile-effects",
            "--db",
            scratch.0.to_str().unwrap(),
            "--case",
            "normal-route",
            "--actor",
            "owner",
            "--json",
        ])
        .output()
        .unwrap();
    assert_eq!(record(&ledger), serde_json::json!([]));
    let inspected = Command::new(env!("CARGO_BIN_EXE_minidora-rust"))
        .args([
            "inspect",
            "--db",
            scratch.0.to_str().unwrap(),
            "--case",
            "normal-route",
            "--actor",
            "owner",
            "--json",
        ])
        .output()
        .unwrap();
    let revision = record(&inspected);
    assert_eq!(revision["lifecycle"], "CANCEL");
    assert_eq!(revision["result"], cancelled);
}
