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

const COMPARE_LEFT_JSON: &str = r#"{"columns":["id","名前","値"],"rows":[["2","猫","001"],["gone","犬\r\n南",""],["1","同じ","1"]]}"#;
const COMPARE_RIGHT_JSON: &str = r#"{"columns":["\u0069d","名前","値"],"rows":[["1","\u540cじ","1"],["\u0032","狐","1"],["new","鳥","9"]]}"#;
const COMPARE_EQUAL_JSON: &str = r#"{"columns":["id","名前","値"],"rows":[["1","\u540cじ","1"],["2","\u732b","001"],["gone","犬\r\n南",""]]}"#;
const COMPARE_EMPTY_JSON: &str = r#"{"columns":["id","名前","値"],"rows":[]}"#;
const COMPARE_LEFT_CSV: &str = "id,名前,値\r\n2,\"猫\",001\r\ngone,\"犬\r\n南\",\r\n1,同じ,1";
const COMPARE_RIGHT_CSV: &str = "\"id\",名前,値\r\n1,同じ,1\r\n\"2\",狐,1\r\nnew,鳥,9";
const COMPARE_EQUAL_CSV: &str =
    "\"id\",名前,値\r\n1,同じ,1\r\n\"2\",猫,001\r\ngone,\"犬\r\n南\",\"\"";

fn comparison_wrapper(left: &str, right: &str) -> String {
    format!(r#"{{"left":{left},"right":{right}}}"#)
}

fn comparison_csv(left: &str, right: &str) -> String {
    format!("--- MINIDORA LEFT CSV ---\n{left}\r\n--- MINIDORA RIGHT CSV ---\n{right}")
}

// Each side/fragment/token below is fixed independently of the returned maps.
fn comparison_fixture_span(source: &str, side: &str, fragment: &str, token: &str) -> Value {
    assert_eq!(source.matches(side).count(), 1);
    assert_eq!(side.matches(fragment).count(), 1);
    assert_eq!(fragment.matches(token).count(), 1);
    let start =
        source.find(side).unwrap() + side.find(fragment).unwrap() + fragment.find(token).unwrap();
    serde_json::json!({
        "source_start":start, "source_end":start+token.len(),
        "source_sha256":minidora_domain::digest(token.as_bytes())
    })
}

fn assert_comparison_side_hashes(report: &Value, left: &str, right: &str) {
    assert_eq!(
        report["left_sha256"],
        minidora_domain::digest(left.as_bytes())
    );
    assert_eq!(
        report["right_sha256"],
        minidora_domain::digest(right.as_bytes())
    );
}

#[test]
fn structured_comparison_json_exact_values_and_pointer_differences() {
    let equal_left = r#"{"a/b~c":[1,-0,9007199254740993,1.2300e+40,"猫"],"empty":[]}"#;
    let equal_right = r#"{"empty":[],"\u0061/b~c":[1.0,0,9007199254740993.0,123e38,"\u732b"]}"#;
    let left =
        r#"{"":null,"a/b~c":[1,2,3],"gone":null,"huge":9007199254740993,"kind":"1","text":"é"}"#;
    let right =
        r#"{"":false,"a/b~c":[2,1],"huge":9007199254740992,"kind":1,"new":null,"text":"e\u0301"}"#;
    for (case, (a, b)) in [
        (equal_left, equal_right),
        (left, right),
        ("1", "[]"),
        ("{}", "{}"),
    ]
    .into_iter()
    .enumerate()
    {
        for (actor, header) in [
            ("japanese", "JSONを比較して:"),
            ("english", "json-compare:"),
            ("mixed-case", "JsOn-CoMpArE:"),
        ] {
            let scratch = Scratch::new();
            let source = comparison_wrapper(a, b);
            let request = format!("{header}\n{source}");
            let result = record(&execute(&scratch.0, actor, &[&request]));
            let report = normal_row_report(&scratch.0, actor, &result, &request, &source);
            assert_comparison_side_hashes(&report, a, b);
            assert_eq!(report["comparison"], "JSON_STRUCTURAL_EXACT_DECIMAL");
            let expected = if case == 1 {
                let differences = [
                    (
                        "/",
                        "CHANGED",
                        Some((r#""":null"#, "null")),
                        Some((r#""":false"#, "false")),
                    ),
                    (
                        "/a~1b~0c/0",
                        "CHANGED",
                        Some(("[1,2,3]", "1")),
                        Some(("[2,1]", "2")),
                    ),
                    (
                        "/a~1b~0c/1",
                        "CHANGED",
                        Some(("[1,2,3]", "2")),
                        Some(("[2,1]", "1")),
                    ),
                    ("/a~1b~0c/2", "REMOVED", Some(("[1,2,3]", "3")), None),
                    ("/gone", "REMOVED", Some((r#""gone":null"#, "null")), None),
                    (
                        "/huge",
                        "CHANGED",
                        Some((r#""huge":9007199254740993"#, "9007199254740993")),
                        Some((r#""huge":9007199254740992"#, "9007199254740992")),
                    ),
                    (
                        "/kind",
                        "CHANGED",
                        Some((r#""kind":"1""#, r#""1""#)),
                        Some((r#""kind":1"#, "1")),
                    ),
                    ("/new", "ADDED", None, Some((r#""new":null"#, "null"))),
                    (
                        "/text",
                        "CHANGED",
                        Some((r#""text":"é""#, r#""é""#)),
                        Some((r#""text":"e\u0301""#, r#""e\u0301""#)),
                    ),
                ];
                serde_json::json!(
                    differences
                        .into_iter()
                        .map(|(pointer, change, l, r)| serde_json::json!({
                            "pointer":pointer,"change":change,
                            "left":l.map(|(f,t)|comparison_fixture_span(&source,a,f,t)),
                            "right":r.map(|(f,t)|comparison_fixture_span(&source,b,f,t))
                        }))
                        .collect::<Vec<_>>()
                )
            } else if case == 2 {
                serde_json::json!([{"pointer":"","change":"CHANGED",
                    "left":comparison_fixture_span(&source,a,a,a),
                    "right":comparison_fixture_span(&source,b,b,b)}])
            } else {
                serde_json::json!([])
            };
            assert_eq!(report["equal"], case == 0 || case == 3);
            assert_eq!(report["differences"], expected);
        }
    }
}

fn assert_json_table_comparison(
    report: &Value,
    source: &str,
    left: &str,
    right: &str,
    changed: bool,
) {
    assert_comparison_side_hashes(report, left, right);
    assert_eq!(report["key_column"], "id");
    assert_eq!(report["equal"], !changed);
    let expected = if changed {
        serde_json::json!([
            {"change":"CELL_CHANGED","key":"2","column":"名前","left_value":"猫","right_value":"狐",
             "left_pointer":"/left/rows/0/1","right_pointer":"/right/rows/1/1",
             "left":comparison_fixture_span(source,left,r#"["2","猫","001"]"#,r#""猫""#),
             "right":comparison_fixture_span(source,right,r#"["\u0032","狐","1"]"#,r#""狐""#)},
            {"change":"CELL_CHANGED","key":"2","column":"値","left_value":"001","right_value":"1",
             "left_pointer":"/left/rows/0/2","right_pointer":"/right/rows/1/2",
             "left":comparison_fixture_span(source,left,r#"["2","猫","001"]"#,r#""001""#),
             "right":comparison_fixture_span(source,right,r#"["\u0032","狐","1"]"#,r#""1""#)},
            {"change":"ROW_REMOVED","key":"gone","column":null,"left_value":null,"right_value":null,
             "left_pointer":"/left/rows/1","right_pointer":null,
             "left":comparison_fixture_span(source,left,r#"["gone","犬\r\n南",""]"#,r#"["gone","犬\r\n南",""]"#),"right":null},
            {"change":"ROW_ADDED","key":"new","column":null,"left_value":null,"right_value":null,
             "left_pointer":null,"right_pointer":"/right/rows/2","left":null,
             "right":comparison_fixture_span(source,right,r#"["new","鳥","9"]"#,r#"["new","鳥","9"]"#)}
        ])
    } else {
        serde_json::json!([])
    };
    assert_eq!(report["changes"], expected);
}

#[test]
fn structured_comparison_json_tables_preserve_key_matching_and_two_sided_locations() {
    for (left, right, changed) in [
        (COMPARE_LEFT_JSON, COMPARE_RIGHT_JSON, true),
        (COMPARE_LEFT_JSON, COMPARE_EQUAL_JSON, false),
        (COMPARE_EMPTY_JSON, COMPARE_EMPTY_JSON, false),
    ] {
        for (actor, header) in [
            ("japanese", "JSON表を列「id」で比較して:"),
            ("english", "json-table-compare \"id\":"),
            ("mixed-case", "JsOn-TaBlE-CoMpArE \"id\":"),
        ] {
            let scratch = Scratch::new();
            let source = comparison_wrapper(left, right);
            let request = format!("{header}\n{source}");
            let result = record(&execute(&scratch.0, actor, &[&request]));
            let report = normal_row_report(&scratch.0, actor, &result, &request, &source);
            assert_json_table_comparison(&report, &source, left, right, changed);
        }
    }
}

fn expected_csv_comparison_span(
    artifact: &Value,
    source: &str,
    kind: &str,
    location: (bool, usize, usize),
    occurrence: Option<usize>,
) -> Value {
    let (left, row, column) = location;
    let (side, raw, decoded) = if left {
        (
            COMPARE_LEFT_CSV,
            [
                ["id", "名前", "値"],
                ["2", "\"猫\"", "001"],
                ["gone", "\"犬\r\n南\"", ""],
                ["1", "同じ", "1"],
            ],
            [
                ["id", "名前", "値"],
                ["2", "猫", "001"],
                ["gone", "犬\r\n南", ""],
                ["1", "同じ", "1"],
            ],
        )
    } else {
        (
            COMPARE_RIGHT_CSV,
            [
                ["\"id\"", "名前", "値"],
                ["1", "同じ", "1"],
                ["\"2\"", "狐", "1"],
                ["new", "鳥", "9"],
            ],
            [
                ["id", "名前", "値"],
                ["1", "同じ", "1"],
                ["2", "狐", "1"],
                ["new", "鳥", "9"],
            ],
        )
    };
    let fragment = raw[row].join(",");
    let start = source.find(side).unwrap()
        + side.find(&fragment).unwrap()
        + raw[row][..column]
            .iter()
            .map(|s| s.len() + 1)
            .sum::<usize>();
    let token = serde_json::to_string(decoded[row][column]).unwrap();
    let output = artifact["text"].as_str().unwrap();
    let output_start = occurrence.map(|n| output.match_indices(&token).nth(n).unwrap().0);
    serde_json::json!({"kind":kind,"row":row,"column":column,"source_start":start,
        "source_end":start+raw[row][column].len(),"source_text":raw[row][column],
        "decoded_value":decoded[row][column],"output_start":output_start,
        "output_end":output_start.map(|p|p+token.len())})
}

fn assert_csv_comparison(artifact: &Value, source: &str, changed: bool, empty: bool) {
    let report: Value = serde_json::from_str(artifact["text"].as_str().unwrap()).unwrap();
    assert_eq!(report["key_column"], "id");
    let changes = if changed {
        serde_json::json!([
            {"kind":"CELL_CHANGED","key":"2","column":"名前","left":"猫","right":"狐"},
            {"kind":"CELL_CHANGED","key":"2","column":"値","left":"001","right":"1"},
            {"kind":"ROW_REMOVED","key":"gone","row":["gone","犬\r\n南",""]},
            {"kind":"ROW_ADDED","key":"new","row":["new","鳥","9"]}
        ])
    } else {
        serde_json::json!([])
    };
    assert_eq!(
        report,
        serde_json::json!({"key_column":"id","changes":changes})
    );
    for (name, value) in [
        ("key_column", "id"),
        ("key_index", "0"),
        (
            "matched_key_count",
            if empty {
                "0"
            } else if changed {
                "2"
            } else {
                "3"
            },
        ),
        ("changed_cell_count", if changed { "2" } else { "0" }),
        ("removed_row_count", if changed { "1" } else { "0" }),
        ("added_row_count", if changed { "1" } else { "0" }),
        ("ordering", "left_row_then_column_then_right_row"),
        ("comparison", "exact_decoded_strings"),
    ] {
        assert_eq!(artifact["parameters"][name], value);
    }
    if changed {
        let mut spans = vec![expected_csv_comparison_span(
            artifact,
            source,
            "CSV_COMPARE_KEY_HEADER",
            (true, 0, 0),
            Some(0),
        )];
        for column in 1..=2 {
            for (kind, location, occurrence) in [
                ("CSV_COMPARE_CHANGED_KEY", (true, 1, 0), Some(column - 1)),
                ("CSV_COMPARE_MATCHED_KEY", (false, 2, 0), None),
                ("CSV_COMPARE_COLUMN", (true, 0, column), Some(0)),
                ("CSV_COMPARE_LEFT_CELL", (true, 1, column), Some(0)),
                ("CSV_COMPARE_RIGHT_CELL", (false, 2, column), Some(0)),
            ] {
                spans.push(expected_csv_comparison_span(
                    artifact, source, kind, location, occurrence,
                ));
            }
        }
        for (left, row, key_kind, cell_kind) in [
            (
                true,
                2,
                "CSV_COMPARE_REMOVED_KEY",
                "CSV_COMPARE_REMOVED_CELL",
            ),
            (false, 3, "CSV_COMPARE_ADDED_KEY", "CSV_COMPARE_ADDED_CELL"),
        ] {
            spans.push(expected_csv_comparison_span(
                artifact,
                source,
                key_kind,
                (left, row, 0),
                Some(0),
            ));
            for column in 0..3 {
                spans.push(expected_csv_comparison_span(
                    artifact,
                    source,
                    cell_kind,
                    (left, row, column),
                    Some(usize::from(column == 0)),
                ));
            }
        }
        assert_eq!(artifact["spans"], serde_json::json!(spans));
    } else {
        assert_eq!(artifact["spans"].as_array().unwrap().len(), 1);
        let span = &artifact["spans"][0];
        assert_eq!(span["kind"], "CSV_COMPARE_KEY_HEADER");
        assert_eq!(span["row"], 0);
        assert_eq!(span["column"], 0);
        assert_eq!(span["source_text"], "id");
        assert_eq!(span["decoded_value"], "id");
        let start = source.find("id,名前,値").unwrap();
        assert_eq!(span["source_start"], start);
        assert_eq!(span["source_end"], start + 2);
        let output_start = artifact["text"].as_str().unwrap().find(r#""id""#).unwrap();
        assert_eq!(span["output_start"], output_start);
        assert_eq!(span["output_end"], output_start + 4);
    }
}

#[test]
fn structured_comparison_csv_tables_preserve_change_order_and_complete_emitted_maps() {
    for (left, right, changed, empty) in [
        (COMPARE_LEFT_CSV, COMPARE_RIGHT_CSV, true, false),
        (COMPARE_LEFT_CSV, COMPARE_EQUAL_CSV, false, false),
        ("id,名前,値", "id,名前,値", false, true),
    ] {
        for (actor, header) in [
            ("japanese", "CSVを列「id」で2つの表を比較して:"),
            ("english", "csv-compare-by \"id\":"),
            ("mixed-case", "CsV-CoMpArE-By \"id\":"),
        ] {
            let scratch = Scratch::new();
            let source = comparison_csv(left, right);
            let request = format!("{header}\n{source}");
            let result = record(&execute(&scratch.0, actor, &[&request]));
            let artifact = normal_document_artifact(
                &scratch.0,
                actor,
                &result,
                &request,
                &source,
                "CSV_TABLE_COMPARE",
            );
            assert_csv_comparison(&artifact, &source, changed, empty);
        }
    }
}

#[test]
fn structured_comparison_actual_reports_feed_explicit_difference_projection() {
    for (header, source, field, csv) in [
        (
            "json-compare:",
            r#"{"left":{"x":null},"right":{}}"#.to_string(),
            "differences",
            false,
        ),
        (
            "json-table-compare \"id\":",
            comparison_wrapper(COMPARE_LEFT_JSON, COMPARE_RIGHT_JSON),
            "changes",
            false,
        ),
        (
            "csv-compare-by \"id\":",
            comparison_csv(COMPARE_LEFT_CSV, COMPARE_RIGHT_CSV),
            "changes",
            true,
        ),
    ] {
        let scratch = Scratch::new();
        let request = format!("{header}\n{source}");
        let result = record(&execute(&scratch.0, "owner", &[&request]));
        let report = if csv {
            let artifact = normal_document_artifact(
                &scratch.0,
                "owner",
                &result,
                &request,
                &source,
                "CSV_TABLE_COMPARE",
            );
            assert_csv_comparison(&artifact, &source, true, false);
            serde_json::from_str::<Value>(result["text"].as_str().unwrap()).unwrap()
        } else {
            normal_row_report(&scratch.0, "owner", &result, &request, &source)
        };
        let actual = result["text"].as_str().unwrap();
        let projected_request = format!("json-object-project [\"{field}\"]:\n{actual}");
        let projected = record(&execute(&scratch.0, "owner", &[&projected_request]));
        let artifact =
            normal_object_projection(&scratch.0, "owner", &projected, &projected_request, actual);
        let raw_key = serde_json::to_string(field).unwrap();
        let pointer = format!("/{field}");
        assert_object_projection_maps(&artifact, actual, &[(field, &raw_key, &pointer)]);
        let mut expected = serde_json::Map::new();
        expected.insert(field.into(), report[field].clone());
        assert_eq!(
            serde_json::from_str::<Value>(projected["text"].as_str().unwrap()).unwrap(),
            Value::Object(expected)
        );
    }
}

fn assert_comparison_stop(header: &str, source: &str, code: &str) {
    let scratch = Scratch::new();
    let request = format!("{header}\n{source}");
    let result = stopped(&execute(&scratch.0, "owner", &[&request]), 2, "SUSPEND");
    assert!(result["reason"].as_str().unwrap().contains(code));
    assert_eq!(result["guarantee"], "UNKNOWN");
    assert_eq!(result["dependencies"], serde_json::json!([]));
    let saved = inspect_normal_case(&scratch.0, "owner");
    assert_eq!(saved["bundle"]["request"]["text"], request);
    assert_eq!(saved["result"], result);
    assert_eq!(saved["lifecycle"], "SUSPEND");
}

#[test]
fn structured_comparison_invalid_inputs_and_all_reserved_markers_stop_durably() {
    for (header, source, code) in [
        (
            "json-compare:",
            r#"{"left":1,"right":1,"extra":0}"#,
            "JSON_COMPARE_SHAPE",
        ),
        (
            "json-compare:",
            r#"{"left":{"a":1,"\u0061":2},"right":{}}"#,
            "DUPLICATE_JSON_KEY",
        ),
        (
            "json-compare:",
            r#"{"left":1,"right":1}x"#,
            "INVALID_JSON_DOCUMENT",
        ),
        (
            "json-compare:",
            r#"{"left":1e1025,"right":1e1025}"#,
            "EXPONENT_LIMIT",
        ),
        (
            "json-compare and send:",
            r#"{"left":1,"right":1}"#,
            "CAPABILITY_UNAVAILABLE",
        ),
    ] {
        assert_comparison_stop(header, source, code);
    }
    let one = r#"{"columns":["id","v"],"rows":[["k","x"]]}"#;
    let duplicate = r#"{"columns":["id","v"],"rows":[["k","x"],["\u006b","y"]]}"#;
    for (header, left, right, code) in [
        (
            "json-table-compare \"id\":",
            duplicate,
            one,
            "JSON_TABLE_COMPARE_DUPLICATE_KEY",
        ),
        (
            "json-table-compare \"id\":",
            one,
            duplicate,
            "JSON_TABLE_COMPARE_DUPLICATE_KEY",
        ),
        (
            "json-table-compare \"id\":",
            one,
            r#"{"columns":["v","id"],"rows":[["x","k"]]}"#,
            "JSON_TABLE_COMPARE_SCHEMA",
        ),
        (
            "json-table-compare \"missing\":",
            one,
            one,
            "JSON_TABLE_COMPARE_KEY",
        ),
        (
            "json-table-compare \"id\":",
            one,
            r#"{"columns":["id","v"],"rows":[["k",1]]}"#,
            "JSON_TABLE_COMPARE_CELL",
        ),
        (
            "json-table-compare \"id\" and send:",
            one,
            one,
            "CAPABILITY_UNAVAILABLE",
        ),
    ] {
        assert_comparison_stop(header, &comparison_wrapper(left, right), code);
    }
    let one = "id,v\r\nk,x";
    let duplicate = "id,v\r\nk,x\r\n\"k\",y";
    for (header, left, right, code) in [
        (
            "csv-compare-by \"id\":",
            duplicate,
            one,
            "CSV_COMPARE_DUPLICATE_KEY",
        ),
        (
            "csv-compare-by \"id\":",
            one,
            duplicate,
            "CSV_COMPARE_DUPLICATE_KEY",
        ),
        (
            "csv-compare-by \"id\":",
            one,
            "v,id\r\nx,k",
            "CSV_COMPARE_SCHEMA",
        ),
        ("csv-compare-by \"missing\":", one, one, "CSV_COLUMN"),
        (
            "csv-compare-by \"id\":",
            "id,v\nk,x",
            one,
            "CSV_LINE_ENDING",
        ),
        (
            "csv-compare-by \"id\" and send:",
            one,
            one,
            "CAPABILITY_UNAVAILABLE",
        ),
    ] {
        assert_comparison_stop(header, &comparison_csv(left, right), code);
    }
    assert_comparison_stop("csv-compare-by \"id\":", one, "CSV_COMPARE_DELIMITER");
    for left_side in [true, false] {
        for marker in ["LEFT", "RIGHT"] {
            let embedded = format!("id,v\r\nk,--- MINIDORA {marker} CSV ---");
            let (left, right) = if left_side {
                (embedded.as_str(), one)
            } else {
                (one, embedded.as_str())
            };
            assert_comparison_stop(
                "csv-compare-by \"id\":",
                &comparison_csv(left, right),
                "CSV_COMPARE_DELIMITER",
            );
        }
    }
}

const COUNT_CSV: &str = "id,分類\r\na,猫\r\nb,open\r\nc,\"猫\"\r\nd,Open\r\ne,open \r\nf, open\r\ng,001\r\nh,1\r\ni,\"\"\r\nj,猫\r\nk,\"open\"\r\nl,\r\nm,é\r\nn,e\u{301}\r\n";
const COUNT_JSON: &str = r#"{"columns":["id","分類"],"rows":[["a","猫"],["b","open"],["c","\u732b"],["d","Open"],["e","open "],["f"," open"],["g","001"],["h","1"],["i",""],["j","猫"],["k","\u006fpen"],["l",""],["m","é"],["n","e\u0301"]]}"#;
const COUNT_EMPTY_JSON: &str = r#"{"columns":["id","分類"],"rows":[]}"#;
const COUNT_KEYS: [&str; 14] = [
    "猫", "open", "猫", "Open", "open ", " open", "001", "1", "", "猫", "open", "", "é", "e\u{301}",
];
const COUNT_CSV_KEYS: [&str; 14] = [
    "猫", "open", "\"猫\"", "Open", "open ", " open", "001", "1", "\"\"", "猫", "\"open\"", "",
    "é", "e\u{301}",
];
const COUNT_GROUPS: [(&str, &[usize]); 10] = [
    ("猫", &[0, 2, 9]),
    ("open", &[1, 10]),
    ("Open", &[3]),
    ("open ", &[4]),
    (" open", &[5]),
    ("001", &[6]),
    ("1", &[7]),
    ("", &[8, 11]),
    ("é", &[12]),
    ("e\u{301}", &[13]),
];

fn expected_count_groups(empty: bool) -> Value {
    let groups: Vec<_> = if empty {
        Vec::new()
    } else {
        COUNT_GROUPS
            .iter()
            .map(|(value, rows)| serde_json::json!({"value": value, "count": rows.len()}))
            .collect()
    };
    serde_json::json!(groups)
}

fn count_csv_key_range(source: &str, row: usize) -> (usize, usize) {
    if row == 0 {
        let start = source.find("id,分類").unwrap() + 3;
        (start, start + "分類".len())
    } else {
        let id = char::from(b'a' + (row - 1) as u8);
        let raw = COUNT_CSV_KEYS[row - 1];
        let fragment = format!("{id},{raw}");
        let start = source.find(&fragment).unwrap() + 2;
        (start, start + raw.len())
    }
}

fn assert_csv_count_maps(artifact: &Value, source: &str, empty: bool) {
    let text = artifact["text"].as_str().unwrap();
    let report: Value = serde_json::from_str(text).unwrap();
    assert_eq!(report["group_by"], "分類");
    assert_eq!(report["groups"], expected_count_groups(empty));
    assert_eq!(artifact["parameters"]["column_index"], "1");
    assert_eq!(artifact["parameters"]["column_name"], "分類");
    assert_eq!(artifact["parameters"]["group_order"], "first_seen");
    assert_eq!(
        artifact["parameters"]["row_count"],
        if empty { "0" } else { "14" }
    );
    assert_eq!(
        artifact["parameters"]["group_count"],
        if empty { "0" } else { "10" }
    );
    let mut expected = std::collections::BTreeSet::from([("CSV_GROUP_COLUMN", 0)]);
    if !empty {
        for row in 1..=14 {
            expected.insert(("CSV_GROUP_MEMBER", row));
        }
        for (_, rows) in COUNT_GROUPS {
            expected.insert(("CSV_GROUP_VALUE", rows[0] + 1));
        }
    }
    let spans = artifact["spans"].as_array().unwrap();
    assert_eq!(spans.len(), expected.len());
    let mut seen = std::collections::BTreeSet::new();
    for span in spans {
        let kind = span["kind"].as_str().unwrap();
        let row = span["row"].as_u64().unwrap() as usize;
        assert_eq!(span["column"], 1);
        assert!(expected.contains(&(kind, row)));
        assert!(seen.insert((kind, row)));
        let (start, end) = count_csv_key_range(source, row);
        assert_eq!(span["source_start"], start);
        assert_eq!(span["source_end"], end);
        assert_eq!(span["source_text"], &source[start..end]);
        let decoded = if row == 0 {
            "分類"
        } else {
            COUNT_KEYS[row - 1]
        };
        assert_eq!(span["decoded_value"], decoded);
        if kind == "CSV_GROUP_MEMBER" {
            assert_eq!(span["output_start"], Value::Null);
            assert_eq!(span["output_end"], Value::Null);
        } else {
            let token = serde_json::to_string(decoded).unwrap();
            let output_start = text.find(&token).unwrap();
            assert_eq!(span["output_start"], output_start);
            assert_eq!(span["output_end"], output_start + token.len());
        }
    }
    assert_eq!(seen, expected);
    assert_eq!(
        report["groups"]
            .as_array()
            .unwrap()
            .iter()
            .map(|group| group["count"].as_u64().unwrap())
            .sum::<u64>(),
        if empty { 0 } else { 14 }
    );
}

fn check_count_json_key_span(span: &Value, source: &str, row: Option<usize>) {
    let (fragment, raw, offset) = if let Some(index) = row {
        let raw = match index {
            2 => r#""\u732b""#.to_string(),
            10 => r#""\u006fpen""#.to_string(),
            13 => r#""e\u0301""#.to_string(),
            _ => serde_json::to_string(COUNT_KEYS[index]).unwrap(),
        };
        let id = serde_json::to_string(&char::from(b'a' + index as u8).to_string()).unwrap();
        (format!("[{id},{raw}]"), raw, id.len() + 2)
    } else {
        (r#"["id","分類"]"#.to_string(), r#""分類""#.to_string(), 6)
    };
    let start = source.find(&fragment).unwrap() + offset;
    let end = start + raw.len();
    assert_eq!(span["source_start"], start);
    assert_eq!(span["source_end"], end);
    assert_eq!(
        span["source_sha256"],
        minidora_domain::digest(&source.as_bytes()[start..end])
    );
    assert_eq!(
        serde_json::from_str::<String>(&source[start..end]).unwrap(),
        row.map_or("分類", |index| COUNT_KEYS[index])
    );
}

fn assert_json_count_maps(report: &Value, source: &str, empty: bool) {
    assert_eq!(report["group_by"], "分類");
    assert_eq!(report["group_by_index"], 1);
    assert_eq!(report["row_count"], if empty { 0 } else { 14 });
    check_count_json_key_span(&report["group_by_source"], source, None);
    let groups = report["groups"].as_array().unwrap();
    assert_eq!(groups.len(), if empty { 0 } else { 10 });
    let mut seen = std::collections::BTreeSet::new();
    for (group, (value, rows)) in groups.iter().zip(COUNT_GROUPS) {
        assert_eq!(group["value"], value);
        assert_eq!(group["count"], rows.len());
        assert_eq!(group["source_row_indices"], serde_json::json!(rows));
        let spans = group["member_source_spans"].as_array().unwrap();
        assert_eq!(spans.len(), rows.len());
        for (span, row) in spans.iter().zip(rows) {
            assert!(seen.insert(*row));
            check_count_json_key_span(span, source, Some(*row));
        }
    }
    assert_eq!(
        seen,
        if empty {
            std::collections::BTreeSet::new()
        } else {
            (0..14).collect()
        }
    );
    assert_eq!(
        groups
            .iter()
            .map(|group| group["count"].as_u64().unwrap())
            .sum::<u64>(),
        if empty { 0 } else { 14 }
    );
}

#[test]
fn table_count_csv_groups_preserve_first_seen_keys_and_every_original_member() {
    for empty in [false, true] {
        let source = if empty { "id,分類\r\n" } else { COUNT_CSV };
        for (actor, header) in [
            ("japanese", "CSVを列「分類」で件数集計して:"),
            ("english", "csv-count-by \"分類\":"),
            ("mixed-case", "CsV-CoUnT-By \"分類\":"),
        ] {
            let scratch = Scratch::new();
            let request = format!("{header}\n{source}");
            let result = record(&execute(&scratch.0, actor, &[&request]));
            let artifact = normal_document_artifact(
                &scratch.0,
                actor,
                &result,
                &request,
                source,
                "CSV_GROUP_COUNT",
            );
            assert_csv_count_maps(&artifact, source, empty);
        }
    }
}

#[test]
fn table_count_json_groups_preserve_decoded_equality_and_original_member_ranges() {
    for empty in [false, true] {
        let source = if empty { COUNT_EMPTY_JSON } else { COUNT_JSON };
        for (actor, header) in [
            ("japanese", "JSON表を列「分類」で件数集計して:"),
            ("english", "json-table-count-by \"分類\":"),
            ("mixed-case", "JsOn-TaBlE-CoUnT-By \"分類\":"),
        ] {
            let scratch = Scratch::new();
            let request = format!("{header}\n{source}");
            let result = record(&execute(&scratch.0, actor, &[&request]));
            let report = normal_row_report(&scratch.0, actor, &result, &request, source);
            assert_json_count_maps(&report, source, empty);
        }
    }
}

#[test]
fn table_count_actual_reports_feed_explicit_groups_projection() {
    for (header, source, csv) in [
        ("csv-count-by \"分類\":", COUNT_CSV, true),
        ("json-table-count-by \"分類\":", COUNT_JSON, false),
    ] {
        let scratch = Scratch::new();
        let request = format!("{header}\n{source}");
        let counted = record(&execute(&scratch.0, "owner", &[&request]));
        let report = if csv {
            let artifact = normal_document_artifact(
                &scratch.0,
                "owner",
                &counted,
                &request,
                source,
                "CSV_GROUP_COUNT",
            );
            assert_csv_count_maps(&artifact, source, false);
            serde_json::from_str::<Value>(counted["text"].as_str().unwrap()).unwrap()
        } else {
            let report = normal_row_report(&scratch.0, "owner", &counted, &request, source);
            assert_json_count_maps(&report, source, false);
            report
        };
        let actual = counted["text"].as_str().unwrap();
        let projection_request = format!("json-object-project [\"groups\"]:\n{actual}");
        let projected = record(&execute(&scratch.0, "owner", &[&projection_request]));
        let artifact =
            normal_object_projection(&scratch.0, "owner", &projected, &projection_request, actual);
        assert_object_projection_maps(&artifact, actual, &[("groups", r#""groups""#, "/groups")]);
        assert_eq!(
            serde_json::from_str::<Value>(projected["text"].as_str().unwrap()).unwrap(),
            serde_json::json!({"groups": report["groups"]})
        );
    }
}

#[test]
fn table_count_missing_columns_and_invalid_shapes_stop_durably() {
    for (header, source, code) in [
        ("csv-count-by \"missing\":", COUNT_CSV, "CSV_COLUMN"),
        (
            "json-table-count-by \"missing\":",
            COUNT_JSON,
            "JSON_TABLE_COUNT_COLUMN",
        ),
        (
            "csv-count-by \"分類\":",
            "id,分類\na,x\n",
            "CSV_LINE_ENDING",
        ),
        (
            "csv-count-by \"分類\":",
            "分類,分類\r\nx,y\r\n",
            "DUPLICATE_COLUMNS",
        ),
        (
            "json-table-count-by \"分類\":",
            r#"{"columns":["id","分類"],"rows":[["a",1]]}"#,
            "JSON_TABLE_COMPARE_CELL",
        ),
        (
            "json-table-count-by \"分類\":",
            r#"{"columns":["分類","\u5206\u985e"],"rows":[["x","y"]]}"#,
            "DUPLICATE_COLUMNS",
        ),
        (
            "json-table-count-by \"分類\":",
            r#"{"columns":["id","分類"],"rows":[["a","x"]],"extra":0}"#,
            "JSON_TABLE_COMPARE_SHAPE",
        ),
        (
            "csv-count-by \"分類\" and send:",
            COUNT_CSV,
            "CAPABILITY_UNAVAILABLE",
        ),
        (
            "json-table-count-by \"分類\" and send:",
            COUNT_JSON,
            "CAPABILITY_UNAVAILABLE",
        ),
    ] {
        let scratch = Scratch::new();
        let request = format!("{header}\n{source}");
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

const EMPTY_PROJECTION_JSON: &str = r#"{"columns":["名前","値","補足"],"rows":[]}"#;
const OBJECT_PROJECTION_JSON: &str = r#"{"drop":"未選択","a/b~c": { "n":1.2300e+40,"s":"\u732b" },"\u006et":9007199254740993,"tail":[true,null]}"#;

fn check_projection_source_span(span: &Value, source: &str, physical_row: usize, column: usize) {
    let (start, end) = conversion_fixture_token(
        source,
        physical_row,
        column,
        true,
        source == CONVERSION_ESCAPED_JSON,
    );
    assert_eq!(span["source_start"], start);
    assert_eq!(span["source_end"], end);
    assert_eq!(
        span["source_sha256"],
        minidora_domain::digest(&source.as_bytes()[start..end])
    );
    assert_eq!(
        serde_json::from_str::<Value>(&source[start..end]).unwrap(),
        CONVERSION_VALUES[physical_row][column]
    );
}

fn normal_object_projection(
    directory: &Path,
    actor: &str,
    result: &Value,
    request: &str,
    source: &str,
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
    assert_eq!(
        artifact["source_sha256"],
        minidora_domain::digest(source.as_bytes())
    );
    assert_eq!(
        artifact["output_sha256"],
        minidora_domain::digest(text.as_bytes())
    );
    assert_eq!(artifact["json"], text);
    let revision = inspect_normal_case(directory, actor);
    assert_eq!(revision["bundle"]["request"]["text"], request);
    assert_eq!(revision["bundle"]["meaning"]["intent"]["document"], source);
    assert_eq!(revision["result"], *result);
    artifact
}

fn assert_object_projection_maps(artifact: &Value, source: &str, expected: &[(&str, &str, &str)]) {
    let output = artifact["json"].as_str().unwrap();
    let fields = artifact["fields"].as_array().unwrap();
    assert_eq!(fields.len(), expected.len());
    let mut cursor = 1;
    for (index, (field, (name, raw_key, pointer))) in fields.iter().zip(expected).enumerate() {
        assert_eq!(field["name"], *name);
        assert_eq!(field["source_pointer"], *pointer);
        let key_start = source.find(raw_key).unwrap();
        let key_end = key_start + raw_key.len();
        assert_eq!(field["key_source"]["source_start"], key_start);
        assert_eq!(field["key_source"]["source_end"], key_end);
        assert_eq!(
            field["key_source"]["source_sha256"],
            minidora_domain::digest(raw_key.as_bytes())
        );
        let colon = key_end + source[key_end..].find(':').unwrap();
        let mut value_start = colon + 1;
        while source.as_bytes()[value_start].is_ascii_whitespace() {
            value_start += 1;
        }
        let mut stream =
            serde_json::Deserializer::from_str(&source[value_start..]).into_iter::<Value>();
        stream.next().unwrap().unwrap();
        let value_end = value_start + stream.byte_offset();
        let raw_value = &source[value_start..value_end];
        assert_eq!(field["value_source"]["source_start"], value_start);
        assert_eq!(field["value_source"]["source_end"], value_end);
        assert_eq!(
            field["value_source"]["source_sha256"],
            minidora_domain::digest(raw_value.as_bytes())
        );
        let encoded_key = serde_json::to_string(name).unwrap();
        assert_eq!(field["output_key_start"], cursor);
        assert_eq!(field["output_key_end"], cursor + encoded_key.len());
        assert_eq!(&output[cursor..cursor + encoded_key.len()], encoded_key);
        cursor += encoded_key.len() + 1;
        assert_eq!(field["output_value_start"], cursor);
        assert_eq!(field["output_value_end"], cursor + raw_value.len());
        assert_eq!(&output[cursor..cursor + raw_value.len()], raw_value);
        cursor += raw_value.len();
        assert_eq!(
            output.as_bytes()[cursor],
            if index + 1 == expected.len() {
                b'}'
            } else {
                b','
            }
        );
        cursor += 1;
    }
    assert_eq!(cursor, output.len());
    serde_json::from_str::<Value>(output).unwrap();
}

#[test]
fn table_projection_column_selection_preserves_strings_and_original_positions() {
    for empty in [false, true] {
        let source = if empty {
            EMPTY_PROJECTION_JSON
        } else {
            CONVERSION_ESCAPED_JSON
        };
        for (actor, header) in [
            ("japanese", "JSON表の列「値」を選択して:"),
            ("english", "json-table-select \"値\":"),
            ("mixed-case", "JsOn-TaBlE-SeLeCt \"値\":"),
        ] {
            let scratch = Scratch::new();
            let request = format!("{header}\n{source}");
            let result = record(&execute(&scratch.0, actor, &[&request]));
            let report = normal_row_report(&scratch.0, actor, &result, &request, source);
            assert_eq!(report["column_name"], "値");
            assert_eq!(report["column_index"], 1);
            check_projection_source_span(&report["column_source"], source, 0, 1);
            assert_eq!(
                report["values"],
                if empty {
                    serde_json::json!([])
                } else {
                    serde_json::json!(["001", "9007199254740993"])
                }
            );
            assert_eq!(
                report["source_row_indices"],
                if empty {
                    serde_json::json!([])
                } else {
                    serde_json::json!([0, 1])
                }
            );
            let spans = report["source_spans"].as_array().unwrap();
            assert_eq!(spans.len(), if empty { 0 } else { 2 });
            for (row, span) in spans.iter().enumerate() {
                check_projection_source_span(span, source, row + 1, 1);
            }
        }
    }
}

#[test]
fn table_projection_reorders_named_columns_without_reordering_rows() {
    for empty in [false, true] {
        let source = if empty {
            EMPTY_PROJECTION_JSON
        } else {
            CONVERSION_ESCAPED_JSON
        };
        for (actor, header) in [
            ("japanese", "JSON表から列[\"補足\",\"名前\"]を抽出して:"),
            ("english", "json-table-project [\"補足\",\"名前\"]:"),
            ("mixed-case", "JsOn-TaBlE-PrOjEcT [\"補足\",\"名前\"]:"),
        ] {
            let scratch = Scratch::new();
            let request = format!("{header}\n{source}");
            let result = record(&execute(&scratch.0, actor, &[&request]));
            let report = normal_row_report(&scratch.0, actor, &result, &request, source);
            assert_eq!(report["columns"], serde_json::json!(["補足", "名前"]));
            assert_eq!(report["selected_column_indices"], serde_json::json!([2, 0]));
            assert_eq!(
                report["rows"],
                if empty {
                    serde_json::json!([])
                } else {
                    serde_json::json!([["1e3", "猫,\"北\""], ["", "犬\r\n南"]])
                }
            );
            assert_eq!(
                report["source_row_indices"],
                if empty {
                    serde_json::json!([])
                } else {
                    serde_json::json!([0, 1])
                }
            );
            let columns = report["column_source_spans"].as_array().unwrap();
            assert_eq!(columns.len(), 2);
            for (span, column) in columns.iter().zip([2, 0]) {
                check_projection_source_span(span, source, 0, column);
            }
            let cells = report["cell_source_spans"].as_array().unwrap();
            assert_eq!(cells.len(), if empty { 0 } else { 2 });
            for (row, spans) in cells.iter().enumerate() {
                let spans = spans.as_array().unwrap();
                assert_eq!(spans.len(), 2);
                for (span, column) in spans.iter().zip([2, 0]) {
                    check_projection_source_span(span, source, row + 1, column);
                }
            }
        }
    }
}

#[test]
fn table_projection_object_fields_preserve_raw_values_and_json_pointer_identity() {
    for (actor, header) in [
        (
            "japanese",
            "JSONオブジェクトからフィールド[\"nt\",\"a/b~c\"]を抽出して:",
        ),
        ("english", "json-object-project [\"nt\",\"a/b~c\"]:"),
        ("mixed-case", "JsOn-ObJeCt-PrOjEcT [\"nt\",\"a/b~c\"]:"),
    ] {
        let scratch = Scratch::new();
        let request = format!("{header}\n{OBJECT_PROJECTION_JSON}");
        let result = record(&execute(&scratch.0, actor, &[&request]));
        assert_eq!(
            result["text"],
            r#"{"nt":9007199254740993,"a/b~c":{ "n":1.2300e+40,"s":"\u732b" }}"#
        );
        let artifact =
            normal_object_projection(&scratch.0, actor, &result, &request, OBJECT_PROJECTION_JSON);
        assert_object_projection_maps(
            &artifact,
            OBJECT_PROJECTION_JSON,
            &[
                ("nt", r#""\u006et""#, "/nt"),
                ("a/b~c", r#""a/b~c""#, "/a~1b~0c"),
            ],
        );
    }
}

#[test]
fn table_projection_actual_sort_report_becomes_csv_only_after_explicit_projection() {
    for (actor, sort_header, project_header) in [
        (
            "japanese",
            "JSON表の列「順位」を昇順に並べ替えて:",
            "JSONオブジェクトからフィールド[\"columns\",\"rows\"]を抽出して:",
        ),
        (
            "english",
            "json-table-sort \"順位\" ascending:",
            "json-object-project [\"columns\",\"rows\"]:",
        ),
        (
            "mixed-case",
            "JsOn-TaBlE-SoRt \"順位\" AsCeNdInG:",
            "JsOn-ObJeCt-PrOjEcT [\"columns\",\"rows\"]:",
        ),
    ] {
        let scratch = Scratch::new();
        let sort_request = format!("{sort_header}\n{ROW_JSON}");
        let sorted = record(&execute(&scratch.0, actor, &[&sort_request]));
        let report = normal_row_report(&scratch.0, actor, &sorted, &sort_request, ROW_JSON);
        let order = [4, 1, 3, 0, 2, 5];
        assert_json_row_maps(&report, ROW_JSON, &order, None);
        let actual_report = sorted["text"].as_str().unwrap();
        let raw_export_request = format!("json-to-csv:\n{actual_report}");
        let raw_stop = stopped(
            &execute(&scratch.0, actor, &[&raw_export_request]),
            2,
            "SUSPEND",
        );
        assert!(
            raw_stop["reason"]
                .as_str()
                .unwrap()
                .contains("JSON_TABLE_SHAPE")
        );
        assert_eq!(raw_stop["dependencies"], serde_json::json!([]));
        let failed_revision = inspect_normal_case(&scratch.0, actor);
        assert_eq!(
            failed_revision["bundle"]["request"]["text"],
            raw_export_request
        );
        assert_eq!(failed_revision["result"], raw_stop);
        let project_request = format!("{project_header}\n{actual_report}");
        let projected = record(&execute(&scratch.0, actor, &[&project_request]));
        let artifact = normal_object_projection(
            &scratch.0,
            actor,
            &projected,
            &project_request,
            actual_report,
        );
        assert_object_projection_maps(
            &artifact,
            actual_report,
            &[
                ("columns", r#""columns""#, "/columns"),
                ("rows", r#""rows""#, "/rows"),
            ],
        );
        let actual_table = projected["text"].as_str().unwrap();
        let table: Value = serde_json::from_str(actual_table).unwrap();
        assert_eq!(
            table,
            serde_json::json!({"columns": report["columns"], "rows": report["rows"]})
        );
        let export_request = format!("json-to-csv:\n{actual_table}");
        let exported = record(&execute(&scratch.0, actor, &[&export_request]));
        let expected = format!(
            "{}\r\n{}\r\n",
            ROW_CSV_TOKENS[0].join(","),
            order
                .iter()
                .map(|index| ROW_CSV_TOKENS[index + 1].join(","))
                .collect::<Vec<_>>()
                .join("\r\n")
        );
        assert_eq!(exported["text"], expected);
        normal_document_artifact(
            &scratch.0,
            actor,
            &exported,
            &export_request,
            actual_table,
            "JSON_TO_CSV",
        );
        let revision = inspect_normal_case(&scratch.0, actor);
        assert!(
            revision["prior_results"]
                .as_object()
                .unwrap()
                .values()
                .any(|prior| prior == &raw_stop)
        );
    }
}

#[test]
fn table_projection_invalid_column_and_field_requests_stop_durably() {
    for (header, source, code) in [
        (
            "json-table-select \"missing\":",
            CONVERSION_ESCAPED_JSON,
            "JSON_TABLE_SELECT_COLUMN",
        ),
        (
            "json-table-project []:",
            CONVERSION_ESCAPED_JSON,
            "JSON_TABLE_PROJECT_COLUMNS",
        ),
        (
            "json-table-project [\"名前\",\"名前\"]:",
            CONVERSION_ESCAPED_JSON,
            "JSON_TABLE_PROJECT_DUPLICATE_COLUMN",
        ),
        (
            "json-table-project [\"missing\"]:",
            CONVERSION_ESCAPED_JSON,
            "JSON_TABLE_PROJECT_COLUMN",
        ),
        (
            "json-object-project []:",
            OBJECT_PROJECTION_JSON,
            "JSON_OBJECT_PROJECT_FIELDS",
        ),
        (
            "json-object-project [\"nt\",\"nt\"]:",
            OBJECT_PROJECTION_JSON,
            "JSON_OBJECT_PROJECT_DUPLICATE_FIELD",
        ),
        (
            "json-object-project [\"missing\"]:",
            OBJECT_PROJECTION_JSON,
            "JSON_OBJECT_PROJECT_FIELD",
        ),
        (
            "json-object-project [\"a\"]:",
            r#"[{"a":1}]"#,
            "JSON_OBJECT_PROJECT_SHAPE",
        ),
        (
            "json-object-project [\"a\"]:",
            r#"{"a":1,"\u0061":2}"#,
            "DUPLICATE_JSON_KEY",
        ),
        (
            "json-table-select \"a\":",
            r#"{"columns":["a"],"rows":[["x"]],"extra":0}"#,
            "JSON_TABLE_COMPARE_SHAPE",
        ),
    ] {
        let scratch = Scratch::new();
        let request = format!("{header}\n{source}");
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

const ROW_CSV: &str = "id,状態,順位\r\n\"猫,甲\",open,2\r\n犬,Open,10\r\n鳥,open ,2\r\n魚,open,10\r\n狐, open,01\r\n馬,open,2\r\n";
const ROW_JSON: &str = r#"{"columns":["id","状態","順位"],"rows":[["\u732b,甲","open","2"],["犬","Open","10"],["鳥","open ","2"],["魚","\u006fpen","10"],["狐"," open","01"],["馬","open","2"]]}"#;
const ROW_VALUES: [[&str; 3]; 7] = [
    ["id", "状態", "順位"],
    ["猫,甲", "open", "2"],
    ["犬", "Open", "10"],
    ["鳥", "open ", "2"],
    ["魚", "open", "10"],
    ["狐", " open", "01"],
    ["馬", "open", "2"],
];
const ROW_CSV_TOKENS: [[&str; 3]; 7] = [
    ["id", "状態", "順位"],
    ["\"猫,甲\"", "open", "2"],
    ["犬", "Open", "10"],
    ["鳥", "open ", "2"],
    ["魚", "open", "10"],
    ["狐", " open", "01"],
    ["馬", "open", "2"],
];

fn assert_csv_row_maps(artifact: &Value, selected: &[usize], filter: Option<&str>) {
    let source = artifact["source_document"].as_str().unwrap();
    let text = artifact["text"].as_str().unwrap();
    let table: Value = serde_json::from_str(text).unwrap();
    let rows: Vec<_> = selected.iter().map(|index| ROW_VALUES[index + 1]).collect();
    assert_eq!(
        table,
        serde_json::json!({"columns": ROW_VALUES[0], "rows": rows})
    );
    let operation = if filter.is_some() {
        "CSV_FILTER"
    } else {
        "CSV_SORT"
    };
    let mut expected = std::collections::BTreeSet::new();
    for column in 0..3 {
        expected.insert((format!("{operation}_HEADER"), 0, column));
        for index in selected {
            expected.insert((format!("{operation}_CELL"), index + 1, column));
        }
    }
    if let Some(query) = filter {
        assert_eq!(artifact["parameters"]["equals"], query);
        assert_eq!(
            artifact["parameters"]["matched_rows"],
            selected.len().to_string()
        );
        for row in 1..7 {
            expected.insert(("CSV_FILTER_DECISION".to_string(), row, 1));
        }
    }
    let spans = artifact["spans"].as_array().unwrap();
    assert_eq!(spans.len(), expected.len());
    let mut seen = std::collections::BTreeSet::new();
    for span in spans {
        let kind = span["kind"].as_str().unwrap();
        let row = span["row"].as_u64().unwrap() as usize;
        let column = span["column"].as_u64().unwrap() as usize;
        let key = (kind.to_string(), row, column);
        assert!(
            expected.contains(&key),
            "unexpected source-span kind/row/column: {key:?}"
        );
        assert!(seen.insert(key));
        let raw = ROW_CSV_TOKENS[row][column];
        let record = ROW_CSV_TOKENS[row].join(",");
        let start = source.find(&record).unwrap()
            + ROW_CSV_TOKENS[row]
                .iter()
                .take(column)
                .map(|cell| cell.len() + 1)
                .sum::<usize>();
        assert_eq!(span["source_start"], start);
        assert_eq!(span["source_end"], start + raw.len());
        assert_eq!(span["source_text"], &source[start..start + raw.len()]);
        assert_eq!(span["decoded_value"], ROW_VALUES[row][column]);
        if kind == "CSV_FILTER_DECISION" {
            assert_eq!(span["output_start"], Value::Null);
            assert_eq!(span["output_end"], Value::Null);
        } else {
            let array = if row == 0 {
                &table["columns"]
            } else {
                let output_row = selected.iter().position(|index| index + 1 == row).unwrap();
                &table["rows"][output_row]
            };
            let fragment = serde_json::to_string(array).unwrap();
            let output_start = text.find(&fragment).unwrap()
                + 1
                + array
                    .as_array()
                    .unwrap()
                    .iter()
                    .take(column)
                    .map(|cell| serde_json::to_string(cell).unwrap().len() + 1)
                    .sum::<usize>();
            let token = serde_json::to_string(&array[column]).unwrap();
            assert_eq!(span["output_start"], output_start);
            assert_eq!(span["output_end"], output_start + token.len());
            assert_eq!(&text[output_start..output_start + token.len()], token);
        }
    }
    assert_eq!(seen, expected);
}

fn normal_row_report(
    directory: &Path,
    actor: &str,
    result: &Value,
    request: &str,
    source: &str,
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
    let report: Value = serde_json::from_str(text).unwrap();
    assert_eq!(
        report["source_sha256"],
        minidora_domain::digest(source.as_bytes())
    );
    let dependencies = result["dependencies"].as_array().unwrap();
    assert_eq!(dependencies.len(), 1);
    let store = minidora_engine::store::Store::open(directory).unwrap();
    let bytes = store
        .get_bytes(actor, "normal-route", dependencies[0].as_str().unwrap())
        .unwrap();
    assert_eq!(minidora_domain::digest(&bytes), dependencies[0]);
    assert_eq!(serde_json::from_slice::<Value>(&bytes).unwrap(), report);
    let revision = inspect_normal_case(directory, actor);
    assert_eq!(revision["bundle"]["request"]["text"], request);
    assert_eq!(revision["bundle"]["meaning"]["intent"]["document"], source);
    assert_eq!(revision["result"], *result);
    report
}

fn check_row_json_span(span: &Value, source: &str, row: Option<usize>, column: usize) {
    let table: Value = serde_json::from_str(source).unwrap();
    let array = row.map_or(&table["columns"], |index| &table["rows"][index]);
    let mut tokens: Vec<_> = array
        .as_array()
        .unwrap()
        .iter()
        .map(|cell| serde_json::to_string(cell).unwrap())
        .collect();
    if source == ROW_JSON {
        if row == Some(0) {
            tokens[0] = r#""\u732b,甲""#.to_string();
        }
        if row == Some(3) {
            tokens[1] = r#""\u006fpen""#.to_string();
        }
    }
    let fragment = format!("[{}]", tokens.join(","));
    let start = source.find(&fragment).unwrap()
        + 1
        + tokens
            .iter()
            .take(column)
            .map(|token| token.len() + 1)
            .sum::<usize>();
    let end = start + tokens[column].len();
    assert_eq!(span["source_start"], start);
    assert_eq!(span["source_end"], end);
    assert_eq!(
        serde_json::from_str::<Value>(&source[start..end]).unwrap(),
        array[column]
    );
    assert_eq!(
        span["source_sha256"],
        minidora_domain::digest(&source.as_bytes()[start..end])
    );
}

fn assert_json_row_maps(report: &Value, source: &str, selected: &[usize], filter: Option<&str>) {
    let table: Value = serde_json::from_str(source).unwrap();
    let expected_rows: Vec<_> = selected
        .iter()
        .map(|index| table["rows"][*index].clone())
        .collect();
    assert_eq!(report["columns"], table["columns"]);
    assert_eq!(report["rows"], serde_json::json!(expected_rows));
    assert_eq!(report["source_row_indices"], serde_json::json!(selected));
    let columns = report["column_source_spans"].as_array().unwrap();
    assert_eq!(columns.len(), 3);
    for (column, span) in columns.iter().enumerate() {
        check_row_json_span(span, source, None, column);
    }
    let cells = report["cell_source_spans"].as_array().unwrap();
    assert_eq!(cells.len(), selected.len());
    for (spans, index) in cells.iter().zip(selected) {
        let spans = spans.as_array().unwrap();
        assert_eq!(spans.len(), 3);
        for (column, span) in spans.iter().enumerate() {
            check_row_json_span(span, source, Some(*index), column);
        }
    }
    if let Some(query) = filter {
        assert_eq!(report["filter_column"], "状態");
        assert_eq!(report["filter_column_index"], 1);
        assert_eq!(report["equals"], query);
        let decisions = report["decisions"].as_array().unwrap();
        assert_eq!(decisions.len(), table["rows"].as_array().unwrap().len());
        for (index, decision) in decisions.iter().enumerate() {
            assert_eq!(decision["source_row_index"], index);
            assert_eq!(decision["value"], table["rows"][index][1]);
            assert_eq!(
                decision["matched"],
                table["rows"][index][1].as_str().unwrap() == query
            );
            check_row_json_span(&decision["source"], source, Some(index), 1);
        }
    } else {
        assert_eq!(report["sort_column"], "順位");
        assert_eq!(report["sort_column_index"], 2);
    }
}

#[test]
fn table_row_csv_filter_retains_predicates_and_feeds_actual_output_to_json_sort() {
    for (actor, format) in [
        ("japanese", "ja"),
        ("english", "en"),
        ("mixed-case", "mixed"),
    ] {
        for query in ["open", "missing"] {
            let header = match format {
                "ja" => format!("CSVの列「状態」が値「{query}」の行を選択して:"),
                "en" => format!("csv-filter \"状態\" equals \"{query}\":"),
                _ => format!("CsV-FiLtEr \"状態\" EqUaLs \"{query}\":"),
            };
            let scratch = Scratch::new();
            let request = format!("{header}\n{ROW_CSV}");
            let filtered = record(&execute(&scratch.0, actor, &[&request]));
            let artifact = normal_document_artifact(
                &scratch.0,
                actor,
                &filtered,
                &request,
                ROW_CSV,
                "CSV_FILTER",
            );
            let selected: &[usize] = if query == "open" { &[0, 3, 5] } else { &[] };
            assert_csv_row_maps(&artifact, selected, Some(query));
            let actual = filtered["text"].as_str().unwrap();
            let sort_request = format!("json-table-sort \"順位\" ascending:\n{actual}");
            let sorted = record(&execute(&scratch.0, actor, &[&sort_request]));
            let report = normal_row_report(&scratch.0, actor, &sorted, &sort_request, actual);
            let order: &[usize] = if query == "open" { &[1, 0, 2] } else { &[] };
            assert_json_row_maps(&report, actual, order, None);
            assert_eq!(report["sort_order"], "ascending");
        }
    }
}

#[test]
fn table_row_csv_sort_is_stable_in_both_string_directions() {
    for descending in [false, true] {
        for (actor, format) in [
            ("japanese", "ja"),
            ("english", "en"),
            ("mixed-case", "mixed"),
        ] {
            let direction = if descending {
                "descending"
            } else {
                "ascending"
            };
            let header = match format {
                "ja" => format!(
                    "CSVの列「順位」を{}に並べ替えて:",
                    if descending { "降順" } else { "昇順" }
                ),
                "en" => format!("csv-sort \"順位\" {direction}:"),
                _ => format!("CsV-SoRt \"順位\" {direction}:"),
            };
            let scratch = Scratch::new();
            let request = format!("{header}\n{ROW_CSV}");
            let result = record(&execute(&scratch.0, actor, &[&request]));
            let artifact =
                normal_document_artifact(&scratch.0, actor, &result, &request, ROW_CSV, "CSV_SORT");
            let order: &[usize] = if descending {
                &[0, 2, 5, 1, 3, 4]
            } else {
                &[4, 1, 3, 0, 2, 5]
            };
            assert_csv_row_maps(&artifact, order, None);
            assert_eq!(artifact["parameters"]["sort_order"], direction);
        }
    }
}

#[test]
fn table_row_json_filter_and_sort_preserve_exact_values_and_source_ranges() {
    for (actor, format) in [
        ("japanese", "ja"),
        ("english", "en"),
        ("mixed-case", "mixed"),
    ] {
        for query in ["open", "missing"] {
            let header = match format {
                "ja" => format!("JSON表の列「状態」が値「{query}」の行を選択して:"),
                "en" => format!("json-table-filter \"状態\" equals \"{query}\":"),
                _ => format!("JsOn-TaBlE-FiLtEr \"状態\" EqUaLs \"{query}\":"),
            };
            let scratch = Scratch::new();
            let request = format!("{header}\n{ROW_JSON}");
            let result = record(&execute(&scratch.0, actor, &[&request]));
            let report = normal_row_report(&scratch.0, actor, &result, &request, ROW_JSON);
            let selected: &[usize] = if query == "open" { &[0, 3, 5] } else { &[] };
            assert_json_row_maps(&report, ROW_JSON, selected, Some(query));
        }
        for descending in [false, true] {
            let direction = if descending {
                "descending"
            } else {
                "ascending"
            };
            let header = match format {
                "ja" => format!(
                    "JSON表の列「順位」を{}に並べ替えて:",
                    if descending { "降順" } else { "昇順" }
                ),
                "en" => format!("json-table-sort \"順位\" {direction}:"),
                _ => format!("JsOn-TaBlE-SoRt \"順位\" {direction}:"),
            };
            let scratch = Scratch::new();
            let request = format!("{header}\n{ROW_JSON}");
            let result = record(&execute(&scratch.0, actor, &[&request]));
            let report = normal_row_report(&scratch.0, actor, &result, &request, ROW_JSON);
            let order: &[usize] = if descending {
                &[0, 2, 5, 1, 3, 4]
            } else {
                &[4, 1, 3, 0, 2, 5]
            };
            assert_json_row_maps(&report, ROW_JSON, order, None);
            assert_eq!(report["sort_order"], direction);
        }
    }
}

#[test]
fn table_row_missing_columns_and_unsupported_operators_stop_durably() {
    for (header, document, code) in [
        (
            "csv-filter \"missing\" equals \"open\":",
            ROW_CSV,
            "CSV_COLUMN",
        ),
        ("csv-sort \"missing\" ascending:", ROW_CSV, "CSV_COLUMN"),
        (
            "json-table-filter \"missing\" equals \"open\":",
            ROW_JSON,
            "JSON_TABLE_FILTER_COLUMN",
        ),
        (
            "json-table-sort \"missing\" ascending:",
            ROW_JSON,
            "JSON_TABLE_SORT_COLUMN",
        ),
        (
            "csv-sort \"順位\" natural:",
            ROW_CSV,
            "CAPABILITY_UNAVAILABLE",
        ),
        (
            "json-table-sort \"順位\" natural:",
            ROW_JSON,
            "CAPABILITY_UNAVAILABLE",
        ),
        (
            "csv-filter \"状態\" contains \"open\":",
            ROW_CSV,
            "CAPABILITY_UNAVAILABLE",
        ),
        (
            "json-table-filter \"状態\" contains \"open\":",
            ROW_JSON,
            "CAPABILITY_UNAVAILABLE",
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
