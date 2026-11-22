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

struct SequenceExpectation {
    id: String,
    source: String,
    evaluated: String,
    value: String,
    requires: Vec<String>,
    occurrences: Vec<String>,
}

fn sequence_step(
    id: &str,
    source: &str,
    evaluated: &str,
    value: &str,
    requires: &[&str],
    occurrences: &[&str],
) -> SequenceExpectation {
    SequenceExpectation {
        id: id.into(),
        source: source.into(),
        evaluated: evaluated.into(),
        value: value.into(),
        requires: requires.iter().map(|s| (*s).into()).collect(),
        occurrences: occurrences.iter().map(|s| (*s).into()).collect(),
    }
}

fn seed_fetched_source_case(directory: &Path, actor: &str) -> (String, String, String) {
    let sources = [(
        "https://example.test/article",
        "Energy enters the ecosystem. Photosynthesis stores light energy. Plants use photosynthesis in their leaves.",
    )];
    let (result_id, packet_refs, texts) =
        seed_fetched_source_case_with_sources(directory, actor, &sources);
    (result_id, packet_refs[0].clone(), texts[0].clone())
}

fn seed_fetched_source_case_with_sources(
    directory: &Path,
    actor: &str,
    sources: &[(&str, &str)],
) -> (String, Vec<String>, Vec<String>) {
    use minidora_domain::{
        CaseRevision, EvidencePacket, GoalContract, Guarantee, Lifecycle, RequestEnvelope,
        ResultRecord, SCHEMA_VERSION, digest, identity,
    };

    let case = "normal-route";
    let request = RequestEnvelope::new(case, "source-fixture", actor, "source fixture");
    let bundle = minidora_engine::input::seal(&request, None).unwrap();
    let fingerprint = identity(&request).unwrap();
    let store = minidora_engine::store::Store::open(directory).unwrap();
    store.register_input(actor, &request, &fingerprint).unwrap();
    let mut packets = Vec::new();
    let mut packet_refs = Vec::new();
    let mut texts = Vec::new();
    for (index, (uri, extracted_text)) in sources.iter().enumerate() {
        let raw_body = extracted_text.as_bytes().to_vec();
        let mut packet = EvidencePacket {
            id: String::new(),
            mode: "FIXTURE".into(),
            packet_kind: "FETCHED_BODY".into(),
            request_uri: (*uri).into(),
            final_uri: (*uri).into(),
            redirect_chain: vec![(*uri).into()],
            observed_at_unix_ms: index as u64 + 1,
            provider_id: "normal-route-test-fixture".into(),
            http_status: 200,
            raw_sha256: digest(&raw_body),
            raw_body,
            extracted_text: Some((*extracted_text).into()),
            parser_version: "fixture-exact-text-v1".into(),
            independence_group: (*uri).into(),
            failure: None,
            retry_after: None,
        };
        packet.id = identity(&(
            &packet.mode,
            &packet.packet_kind,
            &packet.request_uri,
            &packet.final_uri,
            packet.http_status,
            &packet.raw_sha256,
            &packet.parser_version,
            &packet.independence_group,
            &packet.failure,
            &packet.redirect_chain,
            &packet.provider_id,
        ))
        .unwrap();
        packet_refs.push(store.put_artifact(actor, case, &packet).unwrap());
        packets.push(packet);
        texts.push((*extracted_text).into());
    }
    let graph_ref = store
        .put_artifact(
            actor,
            case,
            &serde_json::json!({"fixture": "source-producer"}),
        )
        .unwrap();
    let goal = GoalContract {
        input_fingerprint: fingerprint.clone(),
        intent: bundle.meaning.intent.clone(),
        output_path: None,
        permissions: request.permissions.clone(),
        budget: request.budget.clone(),
        required_predicates: vec!["SUPPORTED_CONTENT".into()],
        required_graph_ref: graph_ref.clone(),
        renderer_version: None,
    };
    let goal_ref = store.put_artifact(actor, case, &goal).unwrap();
    let mut result = ResultRecord {
        id: fingerprint.clone(),
        input_id: request.input_id.clone(),
        input_version: request.input_version,
        lifecycle: Lifecycle::Adopted,
        text: "Fixture source body retained.".into(),
        reason: None,
        guarantee: Guarantee::SourceReport,
        calculation: None,
        content_plan: None,
        render: None,
        receipts: Vec::new(),
        evidence: packets,
        dependencies: packet_refs.clone(),
        actor_memory_revision: None,
        execution_graph_ref: Some(graph_ref),
    };
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
    state.bundle = Some(bundle);
    store.append_case(actor, &state).unwrap();
    state.revision = 2;
    state.parent_revision = Some(1);
    state.lifecycle = Lifecycle::Running;
    state.goal_sha256 = Some(goal_ref);
    store.append_case(actor, &state).unwrap();
    result.lifecycle = Lifecycle::Adopted;
    state.revision = 3;
    state.parent_revision = Some(2);
    state.lifecycle = Lifecycle::Adopted;
    state.result = Some(result.clone());
    store.append_case(actor, &state).unwrap();
    result.lifecycle = Lifecycle::Commit;
    state.revision = 4;
    state.parent_revision = Some(3);
    state.lifecycle = Lifecycle::Commit;
    state.result = Some(result);
    store.append_case(actor, &state).unwrap();
    (fingerprint, packet_refs, texts)
}

fn sequence_artifact(store: &minidora_engine::store::Store, sha: &Value) -> Value {
    let sha = sha.as_str().unwrap();
    let bytes = store.get_bytes("owner", "normal-route", sha).unwrap();
    assert_eq!(minidora_domain::digest(&bytes), sha);
    serde_json::from_slice(&bytes).unwrap()
}

fn assert_sequence_artifacts(
    directory: &Path,
    result: &Value,
    request: &str,
    expected: &[SequenceExpectation],
) -> Value {
    basic_raw_result(directory, result, request, None);
    let saved = inspect_normal_case(directory, "owner");
    let store = minidora_engine::store::Store::open(directory).unwrap();
    let dependencies = result["dependencies"].as_array().unwrap();
    assert_eq!(dependencies.len(), expected.len() + 1);
    let intent = &saved["bundle"]["meaning"]["intent"];
    assert_eq!(intent["kind"], "CALCULATE_SEQUENCE");
    assert_eq!(intent["steps"].as_array().unwrap().len(), expected.len());
    let mut parents = serde_json::Map::new();
    let mut values = std::collections::BTreeMap::new();
    let mut expected_nodes = Vec::new();
    let mut expected_consumption = serde_json::Map::new();
    let mut report_steps = Vec::new();
    let mut display = Vec::new();
    let mut source_cursor = 0;
    for (i, step) in expected.iter().enumerate() {
        let artifact = sequence_artifact(&store, &dependencies[i]);
        let marker = format!("{}:", step.id);
        let line_start = source_cursor + request[source_cursor..].find(&marker).unwrap();
        let expression_tail = &request[line_start + marker.len()..];
        let start =
            line_start + marker.len() + expression_tail.len() - expression_tail.trim_start().len();
        let end = start + step.source.len();
        assert_eq!(&request[start..end], step.source);
        source_cursor = end;
        let span = serde_json::json!({"start":start,"end":end,"text":step.source});
        assert_eq!(artifact["id"], step.id);
        assert_eq!(artifact["source_expression"], span);
        assert_eq!(artifact["evaluated_expression"], step.evaluated);
        assert_eq!(artifact["calculation"]["expression"], step.evaluated);
        assert_eq!(artifact["calculation"]["exact_value"], step.value);
        if i + 1 == expected.len() {
            assert_eq!(result["calculation"], artifact["calculation"]);
        }
        assert_eq!(
            intent["steps"][i],
            serde_json::json!({
                "id":step.id,"expression":span,"requires":step.requires
            })
        );
        let consumed = artifact["consumed"].as_array().unwrap();
        assert_eq!(consumed.len(), step.occurrences.len());
        let mut cursor = 0;
        for (occurrence, id) in step.occurrences.iter().enumerate() {
            let token = format!("@{{{id}}}");
            let at = cursor + step.source[cursor..].find(&token).unwrap();
            cursor = at + token.len();
            assert_eq!(&request[start + at..start + cursor], token);
            assert_eq!(
                consumed[occurrence],
                serde_json::json!({
                    "step_id":id,"expression_start":at,"expression_end":cursor,
                    "occurrence":occurrence,"exact_value":values[id],
                    "artifact_sha256":parents[&format!("sequence-{id}")]
                })
            );
        }
        assert!(!step.source[cursor..].contains("@{"));
        let graph_id = format!("sequence-{}", step.id);
        let requires: Vec<_> = step
            .requires
            .iter()
            .map(|id| format!("sequence-{id}"))
            .collect();
        let consumed_map: serde_json::Map<_, _> = requires
            .iter()
            .map(|id| (id.clone(), parents[id].clone()))
            .collect();
        expected_consumption.insert(graph_id.clone(), Value::Object(consumed_map));
        expected_nodes
            .push(serde_json::json!({"id":graph_id,"kind":"COMPUTED_VALUE","requires":requires}));
        parents.insert(graph_id, dependencies[i].clone());
        values.insert(step.id.clone(), step.value.clone());
        report_steps.push(serde_json::json!([step.id, dependencies[i]]));
        display.push(format!("{}: {} = {}", step.id, step.evaluated, step.value));
    }
    let last = expected.last().unwrap();
    assert_eq!(result["calculation"]["exact_value"], last.value);
    assert_eq!(result["calculation"]["expression"], last.evaluated);
    assert_eq!(result["text"], display.join("\n"));
    assert_eq!(
        sequence_artifact(&store, dependencies.last().unwrap()),
        serde_json::json!([
            "minidora-calculation-sequence-report-v1",
            report_steps,
            last.id,
            last.value,
            "each declared downstream reference was substituted with the exact value of its observed upstream calculation artifact"
        ])
    );
    let graph = sequence_artifact(&store, &result["execution_graph_ref"]);
    assert_eq!(graph.as_array().unwrap().len(), 6);
    let observed = graph[1].as_object().unwrap();
    let sequence_ids: Vec<_> = expected
        .iter()
        .map(|s| format!("sequence-{}", s.id))
        .collect();
    expected_nodes.extend([
        serde_json::json!({"id":"compute","kind":"CORE_RESULT","requires":sequence_ids}),
        serde_json::json!({"id":"content","kind":"CONTENT_PLAN","requires":["compute"]}),
        serde_json::json!({"id":"render","kind":"RENDERED_ARTIFACT","requires":["content"]}),
    ]);
    let has_file = !saved["bundle"]["request"]["output_path"].is_null();
    if has_file {
        expected_nodes
            .push(serde_json::json!({"id":"file","kind":"REQUIRED_FILE","requires":["render"]}));
    }
    assert_eq!(graph[0], Value::Array(expected_nodes));
    assert_eq!(observed.len(), expected.len() + 3 + usize::from(has_file));
    for (id, sha) in &parents {
        assert_eq!(observed[id], *sha);
    }
    expected_consumption.insert("compute".into(), Value::Object(parents));
    expected_consumption.insert(
        "content".into(),
        serde_json::json!({"compute":observed["compute"]}),
    );
    expected_consumption.insert(
        "render".into(),
        serde_json::json!({"content":observed["content"]}),
    );
    if has_file {
        expected_consumption.insert(
            "file".into(),
            serde_json::json!({"render":observed["render"]}),
        );
        assert_eq!(
            sequence_artifact(&store, &observed["file"]),
            result["receipts"][0]
        );
    }
    assert_eq!(graph[5], Value::Object(expected_consumption));
    assert_eq!(graph[2], graph[5]["content"]);
    assert_eq!(graph[3], graph[5]["render"]);
    assert_eq!(
        graph[4],
        if has_file {
            graph[5]["file"].clone()
        } else {
            Value::Null
        }
    );
    assert_eq!(
        sequence_artifact(&store, &observed["compute"]),
        serde_json::json!([result["calculation"], display, [], dependencies])
    );
    assert_eq!(
        sequence_artifact(&store, &observed["content"]),
        result["content_plan"]
    );
    assert_eq!(
        sequence_artifact(&store, &observed["render"]),
        result["render"]
    );
    let mut plan_dependencies = dependencies.clone();
    plan_dependencies.push(observed["compute"].clone());
    assert_eq!(
        result["content_plan"]["dependencies"],
        Value::Array(plan_dependencies)
    );
    let goal = sequence_artifact(&store, &saved["goal_sha256"]);
    assert_eq!(goal["intent"], *intent);
    for field in ["budget", "output_path", "permissions"] {
        assert_eq!(goal[field], saved["bundle"]["request"][field]);
    }
    assert_eq!(
        sequence_artifact(&store, &goal["required_graph_ref"]),
        graph[0]
    );
    assert!(
        store
            .get_bytes(
                "stranger",
                "normal-route",
                dependencies[0].as_str().unwrap()
            )
            .is_err()
    );
    saved
}

#[test]
fn calculation_sequence_route_scalar_maps_order_and_capacity() {
    for header in [
        "計算手順を実行して:",
        "calculation-sequence:",
        "CaLcUlAtIoN-SeQuEnCe:",
    ] {
        for newline in ["\n", "\r\n"] {
            let scratch = Scratch::new();
            let request = format!(
                "{header}{newline}{newline}  z: ０.１-０.６  {newline}unused: 7/2{newline} a: ０+@{{z}}*2 + @{{z}} {newline}final: @{{z}} - @{{a}} + @{{z}}{newline}"
            );
            let expected = [
                sequence_step("z", "０.１-０.６", "０.１-０.６", "-1/2", &[], &[]),
                sequence_step("unused", "7/2", "7/2", "7/2", &[], &[]),
                sequence_step(
                    "a",
                    "０+@{z}*2 + @{z}",
                    "０+(-1/2)*2 + (-1/2)",
                    "-3/2",
                    &["z"],
                    &["z", "z"],
                ),
                sequence_step(
                    "final",
                    "@{z} - @{a} + @{z}",
                    "(-1/2) - (-3/2) + (-1/2)",
                    "1/2",
                    &["z", "a"],
                    &["z", "a", "z"],
                ),
            ];
            let result = record(&execute(
                &scratch.0,
                "owner",
                &["--max-actions", "7", &request],
            ));
            assert_sequence_artifacts(&scratch.0, &result, &request, &expected);
        }
    }
    for count in [1, 16] {
        let scratch = Scratch::new();
        let mut request = "calculation-sequence:\ns0: 1".to_string();
        let mut expected = vec![sequence_step("s0", "1", "1", "1", &[], &[])];
        for i in 1..count {
            let id = format!("s{i}");
            let prior = format!("s{}", i - 1);
            let source = format!("@{{{prior}}}+1");
            request.push_str(&format!("\n{id}: {source}"));
            expected.push(sequence_step(
                &id,
                &source,
                &format!("({i})+1"),
                &(i + 1).to_string(),
                &[&prior],
                &[&prior],
            ));
        }
        let budget = (count + 3).to_string();
        let result = record(&execute(
            &scratch.0,
            "owner",
            &["--max-actions", &budget, &request],
        ));
        assert_sequence_artifacts(&scratch.0, &result, &request, &expected);
    }
}

#[test]
fn calculation_sequence_route_units_consume_whole_exact_operands() {
    let fixtures = vec![
        vec![
            sequence_step("u", "1 km+500 m", "1 km+500 m", "1500 m", &[], &[]),
            sequence_step("v", "@{u}+500 m", "1500 m+500 m", "2000 m", &["u"], &["u"]),
            sequence_step(
                "final",
                "@{v}-1 km",
                "2000 m-1 km",
                "1000 m",
                &["v"],
                &["v"],
            ),
        ],
        vec![
            sequence_step("u", "0.25 m+0.5 m", "0.25 m+0.5 m", "3/4 m", &[], &[]),
            sequence_step("final", "@{u}+25 cm", "3/4 m+25 cm", "1 m", &["u"], &["u"]),
        ],
        vec![
            sequence_step("u", "1e3 m+2e3 m", "1e3 m+2e3 m", "3000 m", &[], &[]),
            sequence_step(
                "final",
                "@{u}+1e3 m",
                "3000 m+1e3 m",
                "4000 m",
                &["u"],
                &["u"],
            ),
        ],
        vec![
            sequence_step("u", "1 m+2 m", "1 m+2 m", "3 m", &[], &[]),
            sequence_step("final", "-@{u}+1 m", "-3 m+1 m", "-2 m", &["u"], &["u"]),
        ],
        vec![
            sequence_step("u", "1 m+2 m", "1 m+2 m", "3 m", &[], &[]),
            sequence_step(
                "final",
                "@{u}＋@{u}",
                "3 m＋3 m",
                "6 m",
                &["u"],
                &["u", "u"],
            ),
        ],
        vec![
            sequence_step("u", "1 e+2 e", "1 e+2 e", "3 e", &[], &[]),
            sequence_step("final", "1 e+@{u}", "1 e+3 e", "4 e", &["u"], &["u"]),
        ],
        vec![
            sequence_step("u", "1 m/1e+2 m/1e", "1 m/1e+2 m/1e", "3 m/1e", &[], &[]),
            sequence_step(
                "final",
                "1 m/1e+@{u}",
                "1 m/1e+3 m/1e",
                "4 m/1e",
                &["u"],
                &["u"],
            ),
        ],
    ];
    for header in [
        "計算手順を実行して:",
        "calculation-sequence:",
        "CaLcUlAtIoN-SeQuEnCe:",
    ] {
        for expected in &fixtures {
            let scratch = Scratch::new();
            let request = format!(
                "{header}\n{}",
                expected
                    .iter()
                    .map(|s| format!("{}: {}", s.id, s.source))
                    .collect::<Vec<_>>()
                    .join("\n")
            );
            let budget = (expected.len() + 3).to_string();
            let result = record(&execute(
                &scratch.0,
                "owner",
                &["--max-actions", &budget, &request],
            ));
            assert_sequence_artifacts(&scratch.0, &result, &request, expected);
        }
    }
}

fn sequence_stored_step_ids(directory: &Path) -> Vec<String> {
    let store = minidora_engine::store::Store::open(directory).unwrap();
    let mut ids = Vec::new();
    for entry in std::fs::read_dir(directory.join("blobs")).unwrap() {
        let name = entry.unwrap().file_name().into_string().unwrap();
        if let Ok(bytes) = store.get_bytes("owner", "normal-route", &name)
            && let Ok(value) = serde_json::from_slice::<Value>(&bytes)
            && value.get("evaluated_expression").is_some()
            && value.get("source_expression").is_some()
            && value.get("consumed").is_some()
        {
            ids.push(value["id"].as_str().unwrap().to_string());
        }
    }
    ids.sort();
    ids
}

#[test]
fn calculation_sequence_route_typed_stops_preserve_only_actual_progress() {
    let too_many = (0..17)
        .map(|i| format!("s{i}: 1"))
        .collect::<Vec<_>>()
        .join("\n");
    let malformed = [
        "",
        "u: 1\nu: 2",
        "1u: 1",
        "u: ",
        "u 1",
        "u: @{missing}+1",
        "u: @{u}+1",
        "u: @{v}+1\nv: 2",
        "u: @{v}+1\nv: @{u}+1",
        "u: 1\nv: @{u",
        "u: 1\nv: @{}+1",
        &too_many,
    ];
    for body in malformed {
        let scratch = Scratch::new();
        let request = format!("calculation-sequence:\n{body}");
        let result = stopped(&execute(&scratch.0, "owner", &[&request]), 2, "SUSPEND");
        assert!(
            result["reason"]
                .as_str()
                .unwrap()
                .starts_with("CAPABILITY_UNAVAILABLE:")
        );
        assert_eq!(result["guarantee"], "UNKNOWN");
        assert_eq!(result["dependencies"], serde_json::json!([]));
        assert_eq!(sequence_stored_step_ids(&scratch.0), Vec::<String>::new());
        let saved = inspect_normal_case(&scratch.0, "owner");
        assert_eq!(saved["bundle"]["request"]["text"], request);
        assert_eq!(saved["result"], result);
    }
    let mut failures = Vec::new();
    for expression in [
        "2@{u}+1 m",
        "@{u}m+1 mm",
        "@{u}/s+1 m/s",
        "1/@{u}+1/3 m",
        "@{u}*2",
        "(@{u})+1 m",
        "@{u}^2+1 m",
        "1e+@{u}+1 m",
        "1E-@{u}+1 m",
        "１ｅ＋@{u}＋１ m",
        "1e−@{u}+1 m",
        "1 e-@{u}+1 e-3m",
        "@{u}",
    ] {
        failures.push((
            format!("calculation-sequence:\nu: 1 m+2 m\nbad: {expression}\nlater: 9"),
            "CALCULATION_REFERENCE_CONTEXT",
            1,
        ));
    }
    failures.extend([
        (
            "calculation-sequence:\nu: 1+2\nbad: 2@{u}+1\nlater: 9".into(),
            "INVALID_EXPRESSION",
            1,
        ),
        (
            "calculation-sequence:\nu: 1+2\nbad: @{u}/0\nlater: 9".into(),
            "DIVISION_BY_ZERO",
            1,
        ),
        (
            "calculation-sequence:\nu: 1 m+2 m\nbad: @{u}+1 s\nlater: 9".into(),
            "UNIT_MISMATCH",
            1,
        ),
    ]);
    for (request, code, completed) in failures {
        let scratch = Scratch::new();
        let result = stopped(&execute(&scratch.0, "owner", &[&request]), 2, "SUSPEND");
        assert!(
            result["reason"]
                .as_str()
                .unwrap()
                .starts_with(&format!("{code}:")),
            "{request}: {}",
            result["reason"]
        );
        assert_eq!(result["guarantee"], "UNKNOWN");
        assert_eq!(result["dependencies"].as_array().unwrap().len(), completed);
        for field in [
            "calculation",
            "content_plan",
            "render",
            "execution_graph_ref",
        ] {
            assert!(result[field].is_null());
        }
        assert_eq!(result["receipts"], serde_json::json!([]));
        assert_eq!(sequence_stored_step_ids(&scratch.0), vec!["u"]);
        let store = minidora_engine::store::Store::open(&scratch.0).unwrap();
        assert_eq!(
            sequence_artifact(&store, &result["dependencies"][0])["id"],
            "u"
        );
        let saved = inspect_normal_case(&scratch.0, "owner");
        assert_eq!(saved["bundle"]["request"]["text"], request);
        assert_eq!(saved["result"], result);
    }
    let request = "calculation-sequence:\nu: 2+3\nfinal: @{u}*2";
    for (args, code, count) in [
        (vec!["--max-actions", "4"], "ACTION_BUDGET_EXHAUSTED", 0),
        (vec!["--deadline-ms", "0"], "RESOURCE_LIMIT", 0),
        (vec!["--max-evidence-bytes", "1"], "RESOURCE_LIMIT", 2),
    ] {
        let scratch = Scratch::new();
        let mut args = args;
        args.push(request);
        let result = stopped(&execute(&scratch.0, "owner", &args), 2, "SUSPEND");
        assert!(
            result["reason"]
                .as_str()
                .unwrap()
                .starts_with(&format!("{code}:"))
        );
        assert_eq!(result["guarantee"], "UNKNOWN");
        assert_eq!(result["dependencies"].as_array().unwrap().len(), count);
        assert_eq!(sequence_stored_step_ids(&scratch.0).len(), count);
        for field in [
            "calculation",
            "content_plan",
            "render",
            "execution_graph_ref",
        ] {
            assert!(result[field].is_null());
        }
        let saved = inspect_normal_case(&scratch.0, "owner");
        assert_eq!(saved["bundle"]["request"]["text"], request);
        assert_eq!(saved["result"], result);
    }
    let scratch = Scratch::new();
    let path = scratch.0.join("not-authorized.txt");
    let result = stopped(
        &execute(
            &scratch.0,
            "owner",
            &["--output", path.to_str().unwrap(), request],
        ),
        2,
        "SUSPEND",
    );
    assert!(
        result["reason"]
            .as_str()
            .unwrap()
            .starts_with("WRITE_PERMISSION_MISSING:")
    );
    assert_eq!(result["guarantee"], "COMPUTED");
    assert_eq!(result["calculation"]["exact_value"], "10");
    assert_eq!(result["dependencies"].as_array().unwrap().len(), 3);
    assert_eq!(result["content_plan"]["text"], result["text"]);
    assert_eq!(result["render"]["status"], "RENDERED");
    assert!(result["execution_graph_ref"].is_null());
    assert_eq!(result["receipts"], serde_json::json!([]));
    assert!(!path.exists());
    assert!(
        minidora_engine::store::Store::open(&scratch.0)
            .unwrap()
            .actions("owner", "normal-route")
            .unwrap()
            .is_empty()
    );
    let saved = inspect_normal_case(&scratch.0, "owner");
    assert_eq!(saved["bundle"]["request"]["text"], request);
    assert_eq!(saved["result"], result);
}

#[test]
fn calculation_sequence_route_retry_explanation_and_version_history() {
    let scratch = Scratch::new();
    let request = "calculation-sequence:\nu: 2+3\nfinal: @{u}*2";
    let expected = [
        sequence_step("u", "2+3", "2+3", "5", &[], &[]),
        sequence_step("final", "@{u}*2", "(5)*2", "10", &["u"], &["u"]),
    ];
    let args = ["--input", "sequence-history", request];
    let first = record(&execute(&scratch.0, "owner", &args));
    assert_sequence_artifacts(&scratch.0, &first, request, &expected);
    assert_eq!(record(&execute(&scratch.0, "owner", &args)), first);
    let explanation = record(&execute(&scratch.0, "owner", &["詳しく説明して"]));
    assert_eq!(explanation["lifecycle"], "COMMIT");
    assert_eq!(explanation["calculation"], first["calculation"]);
    let units = [
        "手順 u の元の式: 2+3",
        "手順 u の結果: 2+3 = 5",
        "手順 final の元の式: @{u}*2",
        "代入した先行手順の値: u = 5",
        "手順 final の結果: (5)*2 = 10",
        "最終結果: 10",
    ];
    assert_eq!(explanation["text"], units.join("\n"));
    let mut dependencies = vec![first["id"].clone()];
    dependencies.extend(first["dependencies"].as_array().unwrap().iter().cloned());
    assert_eq!(
        explanation["dependencies"].as_array().unwrap(),
        &dependencies
    );
    assert_eq!(
        inspect_normal_case(&scratch.0, "owner")["bundle"]["meaning"]["context_result_ref"],
        first["id"]
    );
    let mut preceding = explanation;
    for input in ["sequence-detail-again", "sequence-detail-third"] {
        let repeated = record(&execute(
            &scratch.0,
            "owner",
            &["--input", input, "詳しく説明して"],
        ));
        assert_eq!(repeated["lifecycle"], "COMMIT");
        assert_eq!(repeated["calculation"], first["calculation"]);
        assert_eq!(repeated["text"], units.join("\n"));
        let mut expected = vec![preceding["id"].clone()];
        expected.extend(dependencies.iter().cloned());
        assert_eq!(repeated["dependencies"].as_array().unwrap(), &expected);
        preceding = repeated;
    }
    let changed = request.replace("2+3", "4+3");
    let expected = [
        sequence_step("u", "4+3", "4+3", "7", &[], &[]),
        sequence_step("final", "@{u}*2", "(7)*2", "14", &["u"], &["u"]),
    ];
    let second = record(&execute(
        &scratch.0,
        "owner",
        &["--input", "sequence-history", "--version", "2", &changed],
    ));
    let saved = assert_sequence_artifacts(&scratch.0, &second, &changed, &expected);
    assert_eq!(saved["prior_results"][first["id"].as_str().unwrap()], first);
    assert_eq!(record(&execute(&scratch.0, "owner", &args)), first);
    assert_eq!(inspect_normal_case(&scratch.0, "owner")["result"], second);
}

#[test]
fn calculation_sequence_route_named_correction_preserves_dependency_graph() {
    let scratch = Scratch::new();
    let original_args = [
        "--input",
        "sequence-source",
        "計算手順を実行して:\nbase: 1 km + 500 m\nindependent: 2+3\nfinal: 100 m + @{base}",
    ];
    let original = record(&execute(&scratch.0, "owner", &original_args));
    let id = original["id"].as_str().unwrap();
    let revision_args = [
        "--input",
        "sequence-step-correction",
        "--context",
        id,
        "--max-actions",
        "9",
        "計算手順の base を訂正: 500 m + 100 m",
    ];
    let corrected = record(&execute(&scratch.0, "owner", &revision_args));
    assert_eq!(corrected["calculation"]["exact_value"], "700 m");
    assert!(
        corrected["text"]
            .as_str()
            .unwrap()
            .starts_with("訂正した手順: base\n")
    );
    assert_eq!(corrected["dependencies"][0], original["id"]);
    assert_eq!(corrected["dependencies"][3], original["dependencies"][1]);
    assert_ne!(corrected["dependencies"][2], original["dependencies"][0]);
    assert_ne!(corrected["dependencies"][4], original["dependencies"][2]);
    assert_eq!(
        record(&execute(&scratch.0, "owner", &revision_args)),
        corrected
    );
    assert_eq!(
        record(&execute(&scratch.0, "owner", &original_args)),
        original
    );

    let removed_dependency = stopped(
        &execute(
            &scratch.0,
            "owner",
            &["--context", id, "計算手順の final を訂正: 300 m"],
        ),
        2,
        "SUSPEND",
    );
    assert!(
        removed_dependency["reason"]
            .as_str()
            .unwrap()
            .contains("SEQUENCE_CORRECTION_DEPENDENCIES_CHANGED")
    );
    let path = scratch.0.join("over-budget.txt");
    let path_text = path.to_str().unwrap();
    let long_change = format!("計算手順の base を訂正: 500 m{}+ 100 m", " ".repeat(4000));
    let over_budget = stopped(
        &execute(
            &scratch.0,
            "owner",
            &[
                "--context",
                id,
                "--max-evidence-bytes",
                "8000",
                "--output",
                path_text,
                "--allow-write",
                path_text,
                &long_change,
            ],
        ),
        2,
        "SUSPEND",
    );
    assert!(
        over_budget["reason"]
            .as_str()
            .unwrap()
            .contains("RESOURCE_LIMIT")
    );
    assert!(!path.exists());
    let premise = record(&execute(
        &scratch.0,
        "owner",
        &["計算手順の説明は技術文書です"],
    ));
    assert_eq!(premise["guarantee"], "USER_PREMISE");
}

#[test]
fn calculation_sequence_route_repeated_corrections_keep_active_formulas_and_lineage() {
    let scratch = Scratch::new();
    let original_request = "calculation-sequence:\nbase: 2+3\nfinal: @{base}*2";
    let original_args = ["--input", "original-sequence", original_request];
    let original = record(&execute(&scratch.0, "owner", &original_args));
    assert_sequence_artifacts(
        &scratch.0,
        &original,
        original_request,
        &[
            sequence_step("base", "2+3", "2+3", "5", &[], &[]),
            sequence_step("final", "@{base}*2", "(5)*2", "10", &["base"], &["base"]),
        ],
    );
    let store = minidora_engine::store::Store::open(&scratch.0).unwrap();
    let assert_goal = |result: &Value, request: &str, context: &Value, predicate: &str| {
        basic_raw_result(&scratch.0, result, request, None);
        assert_eq!(result["content_plan"]["guarantee"], "COMPUTED");
        assert_eq!(result["receipts"], serde_json::json!([]));
        let saved = inspect_normal_case(&scratch.0, "owner");
        assert_eq!(
            saved["bundle"]["meaning"]["context_result_ref"],
            context["id"]
        );
        let goal = sequence_artifact(&store, &saved["goal_sha256"]);
        assert_eq!(
            goal["required_predicates"],
            serde_json::json!([predicate, "SUPPORTED_CONTENT", "EXACT_RENDER"])
        );
    };
    let expected_detail = |expression: &str, value: &str, final_value: &str| {
        [
            format!("手順 base の元の式: {expression}"),
            format!("手順 base の結果: {expression} = {value}"),
            "手順 final の元の式: @{base}*2".into(),
            format!("代入した先行手順の値: base = {value}"),
            format!("手順 final の結果: ({value})*2 = {final_value}"),
            format!("最終結果: {final_value}"),
        ]
        .join("\n")
    };
    let detail = |input: &str, selected: &Value, active: &Value, budget: &str, text: &str| {
        let args = [
            "--input",
            input,
            "--context",
            selected["id"].as_str().unwrap(),
            "--max-actions",
            budget,
            "詳しく説明して",
        ];
        let result = record(&execute(&scratch.0, "owner", &args));
        let predicate = if active["id"] == original["id"] {
            "EXACT_PREVIOUS_CALCULATION_SEQUENCE_V1"
        } else {
            "EXACT_PREVIOUS_CALCULATION_SEQUENCE_V3"
        };
        assert_goal(&result, "詳しく説明して", selected, predicate);
        assert_eq!(result["text"], text);
        assert_eq!(result["calculation"], active["calculation"]);
        let mut dependencies = vec![selected["id"].clone()];
        if selected["id"] != active["id"] {
            dependencies.push(active["id"].clone());
        }
        dependencies.extend_from_slice(active["dependencies"].as_array().unwrap());
        assert_eq!(result["dependencies"], Value::Array(dependencies));
        assert_eq!(record(&execute(&scratch.0, "owner", &args)), result);
        result
    };

    let first_request = "計算手順の base を訂正: 4+3";
    let first_args = [
        "--input",
        "first-correction",
        "--context",
        original["id"].as_str().unwrap(),
        "--max-actions",
        "7",
        first_request,
    ];
    let first = record(&execute(&scratch.0, "owner", &first_args));
    assert_goal(
        &first,
        first_request,
        &original,
        "EXACT_CALCULATION_SEQUENCE_CORRECTION_V1",
    );
    assert_eq!(
        first["text"],
        "訂正した手順: base\nbase: 4+3 = 7\nfinal: (7)*2 = 14"
    );
    assert_eq!(first["dependencies"].as_array().unwrap().len(), 5);
    assert_eq!(first["dependencies"][0], original["id"]);
    let first_span = serde_json::json!({"start": first_request.find("4+3").unwrap(),
        "end": first_request.len(), "text": "4+3"});
    let first_base = sequence_artifact(&store, &first["dependencies"][2]);
    assert_eq!(first_base["source_expression"], first_span);
    let first_detail = detail(
        "first-detail",
        &first,
        &first,
        "7",
        &expected_detail("4+3", "7", "14"),
    );

    let second_request = "計算手順の base を訂正: 8+1";
    let second_args = [
        "--input",
        "second-correction",
        "--context",
        first_detail["id"].as_str().unwrap(),
        "--max-actions",
        "10",
        second_request,
    ];
    let second = record(&execute(&scratch.0, "owner", &second_args));
    assert_goal(
        &second,
        second_request,
        &first_detail,
        "EXACT_CALCULATION_SEQUENCE_CORRECTION_V2",
    );
    assert_eq!(
        second["text"],
        "訂正した手順: base\nbase: 8+1 = 9\nfinal: (9)*2 = 18"
    );
    let second_dependencies = second["dependencies"].as_array().unwrap();
    assert_eq!(second_dependencies.len(), 6);
    assert_eq!(
        &second_dependencies[..2],
        &[first_detail["id"].clone(), first["id"].clone()]
    );
    let second_span = serde_json::json!({"start": second_request.find("8+1").unwrap(),
        "end": second_request.len(), "text": "8+1"});
    let mut proof_dependencies = vec![first_detail["id"].clone(), first["id"].clone()];
    proof_dependencies.extend_from_slice(first["dependencies"].as_array().unwrap());
    assert_eq!(
        sequence_artifact(&store, &second_dependencies[2]),
        serde_json::json!([
            "minidora-calculation-sequence-correction-v2",
            first_detail["id"],
            original["id"],
            minidora_domain::digest(first_request.as_bytes()),
            second["id"],
            minidora_domain::digest(second_request.as_bytes()),
            "base",
            first_span,
            second_span,
            proof_dependencies
        ])
    );
    let second_base = sequence_artifact(&store, &second_dependencies[3]);
    let second_final = sequence_artifact(&store, &second_dependencies[4]);
    assert_eq!(second_base["source_expression"], second_span);
    assert_eq!(second_base["calculation"]["exact_value"], "9");
    assert_eq!(second_final["evaluated_expression"], "(9)*2");
    assert_eq!(
        second_final["consumed"][0]["artifact_sha256"],
        second_dependencies[3]
    );
    assert_eq!(
        second_final["source_expression"],
        sequence_artifact(&store, &original["dependencies"][1])["source_expression"]
    );
    let second_detail = detail(
        "second-detail",
        &second,
        &second,
        "10",
        &expected_detail("8+1", "9", "18"),
    );
    let repeated_detail = detail(
        "repeated-detail",
        &second_detail,
        &second,
        "11",
        &expected_detail("8+1", "9", "18"),
    );
    let old_detail = detail(
        "original-history-detail",
        &original,
        &original,
        "5",
        &expected_detail("2+3", "5", "10"),
    );
    assert_eq!(record(&execute(&scratch.0, "owner", &first_args)), first);
    assert_eq!(record(&execute(&scratch.0, "owner", &second_args)), second);
    assert_eq!(
        record(&execute(&scratch.0, "owner", &original_args)),
        original
    );
    assert_eq!(
        inspect_normal_case(&scratch.0, "owner")["result"],
        old_detail
    );

    let completed = sequence_stored_step_ids(&scratch.0);
    for (input, selected, budget, request) in [
        ("first-correction-budget", &original, "6", first_request),
        ("corrected-detail-budget", &first, "6", "詳しく説明して"),
        (
            "repeat-correction-budget",
            &first_detail,
            "9",
            second_request,
        ),
    ] {
        let args = [
            "--input",
            input,
            "--context",
            selected["id"].as_str().unwrap(),
            "--max-actions",
            budget,
            request,
        ];
        let result = stopped(&execute(&scratch.0, "owner", &args), 2, "SUSPEND");
        assert!(
            result["reason"]
                .as_str()
                .unwrap()
                .starts_with("ACTION_BUDGET_EXHAUSTED:")
        );
        assert_eq!(result["guarantee"], "UNKNOWN");
        assert_eq!(result["dependencies"], serde_json::json!([]));
        assert_eq!(result["receipts"], serde_json::json!([]));
        for field in [
            "calculation",
            "content_plan",
            "render",
            "execution_graph_ref",
        ] {
            assert!(result[field].is_null());
        }
        let saved = inspect_normal_case(&scratch.0, "owner");
        assert_eq!(saved["result"], result);
        assert_eq!(
            saved["bundle"]["meaning"]["context_result_ref"],
            selected["id"]
        );
    }
    assert_eq!(sequence_stored_step_ids(&scratch.0), completed);
    assert!(store.actions("owner", "normal-route").unwrap().is_empty());
    let saved = inspect_normal_case(&scratch.0, "owner");
    for prior in [
        &original,
        &first,
        &first_detail,
        &second,
        &second_detail,
        &repeated_detail,
        &old_detail,
    ] {
        assert_eq!(
            saved["prior_results"][prior["id"].as_str().unwrap()],
            *prior
        );
    }
}

#[test]
fn calculation_sequence_route_explicit_rewire_changes_consumed_edge_and_preserves_old_syntax() {
    let scratch = Scratch::new();
    let source = "calculation-sequence:\nleft: 2+3\nright: 8+1\nfinal: @{left}*2";
    let original = record(&execute(
        &scratch.0,
        "owner",
        &["--input", "edge-source", source],
    ));
    assert_sequence_artifacts(
        &scratch.0,
        &original,
        source,
        &[
            sequence_step("left", "2+3", "2+3", "5", &[], &[]),
            sequence_step("right", "8+1", "8+1", "9", &[], &[]),
            sequence_step("final", "@{left}*2", "(5)*2", "10", &["left"], &["left"]),
        ],
    );
    let store = minidora_engine::store::Store::open(&scratch.0).unwrap();
    let span = |request: &str, expression: &str| {
        let start = request.find(expression).unwrap();
        serde_json::json!({"start": start, "end": start + expression.len(), "text": expression})
    };
    let verify = |result: &Value,
                  request: &str,
                  context: &Value,
                  predicate: &str,
                  expression: &Value| {
        basic_raw_result(&scratch.0, result, request, None);
        assert_eq!(result["content_plan"]["guarantee"], "COMPUTED");
        assert_eq!(result["receipts"], serde_json::json!([]));
        let saved = inspect_normal_case(&scratch.0, "owner");
        assert_eq!(
            saved["bundle"]["meaning"]["context_result_ref"],
            context["id"]
        );
        let goal = sequence_artifact(&store, &saved["goal_sha256"]);
        assert_eq!(
            goal["required_predicates"],
            serde_json::json!([predicate, "SUPPORTED_CONTENT", "EXACT_RENDER"])
        );
        let graph = sequence_artifact(&store, &result["execution_graph_ref"]);
        let final_node = graph[0]
            .as_array()
            .unwrap()
            .iter()
            .find(|node| node["id"] == "sequence-final")
            .unwrap();
        assert_eq!(
            final_node["requires"],
            serde_json::json!(["sequence-right"])
        );
        assert_eq!(graph[1]["sequence-left"], original["dependencies"][0]);
        assert_eq!(graph[1]["sequence-right"], original["dependencies"][1]);
        assert_eq!(
            graph[5]["sequence-final"],
            serde_json::json!({
                "sequence-right": original["dependencies"][1]
            })
        );
        let final_step = sequence_artifact(&store, &graph[1]["sequence-final"]);
        assert_eq!(final_step["source_expression"], *expression);
        assert_eq!(final_step["calculation"], result["calculation"]);
        assert_eq!(
            final_step["consumed"],
            serde_json::json!([{
                "step_id": "right", "expression_start": 0, "expression_end": 8,
                "occurrence": 0, "exact_value": "9", "artifact_sha256": original["dependencies"][1]
            }])
        );
    };
    let request = "計算手順の final を参照先も含めて訂正: @{right}*2";
    let args = [
        "--input",
        "rewire-final",
        "--context",
        original["id"].as_str().unwrap(),
        "--max-actions",
        "9",
        request,
    ];
    let rewired = record(&execute(&scratch.0, "owner", &args));
    let revised_span = span(request, "@{right}*2");
    verify(
        &rewired,
        request,
        &original,
        "EXACT_CALCULATION_SEQUENCE_CORRECTION_V3",
        &revised_span,
    );
    assert_eq!(
        rewired["text"],
        "訂正した手順: final\nleft: 2+3 = 5\nright: 8+1 = 9\nfinal: (9)*2 = 18"
    );
    assert_eq!(rewired["calculation"]["exact_value"], "18");
    let saved = inspect_normal_case(&scratch.0, "owner");
    assert_eq!(
        saved["bundle"]["meaning"]["intent"]["change_dependencies"],
        true
    );
    let dependencies = rewired["dependencies"].as_array().unwrap();
    assert_eq!(dependencies.len(), 6);
    assert_eq!(dependencies[0], original["id"]);
    assert_eq!(
        &dependencies[2..4],
        &original["dependencies"].as_array().unwrap()[..2]
    );
    assert_ne!(dependencies[4], original["dependencies"][2]);
    let mut prior_dependencies = vec![original["id"].clone()];
    prior_dependencies.extend_from_slice(original["dependencies"].as_array().unwrap());
    assert_eq!(
        sequence_artifact(&store, &dependencies[1]),
        serde_json::json!({
            "kind": "minidora-calculation-sequence-correction-v3",
            "selected_result_id": original["id"], "original_result_id": original["id"],
            "previous_raw_sha256": minidora_domain::digest(source.as_bytes()),
            "correction_input_id": rewired["id"],
            "correction_raw_sha256": minidora_domain::digest(request.as_bytes()),
            "step_id": "final", "previous_expression": span(source, "@{left}*2"),
            "revised_expression": revised_span, "previous_requires": ["left"],
            "revised_requires": ["right"], "dependencies": prior_dependencies
        })
    );
    let expected_detail = |factor: &str, value: &str| {
        [
            "手順 left の元の式: 2+3".to_string(),
            "手順 left の結果: 2+3 = 5".to_string(),
            "手順 right の元の式: 8+1".to_string(),
            "手順 right の結果: 8+1 = 9".to_string(),
            format!("手順 final の元の式: @{{right}}*{factor}"),
            "代入した先行手順の値: right = 9".to_string(),
            format!("手順 final の結果: (9)*{factor} = {value}"),
            format!("最終結果: {value}"),
        ]
        .join("\n")
    };
    let detail_args = [
        "--input",
        "rewire-detail",
        "--context",
        rewired["id"].as_str().unwrap(),
        "--max-actions",
        "9",
        "詳しく説明して",
    ];
    let detail = record(&execute(&scratch.0, "owner", &detail_args));
    verify(
        &detail,
        "詳しく説明して",
        &rewired,
        "EXACT_PREVIOUS_CALCULATION_SEQUENCE_V4",
        &revised_span,
    );
    assert_eq!(detail["text"], expected_detail("2", "18"));
    let mut detail_dependencies = vec![rewired["id"].clone()];
    detail_dependencies.extend_from_slice(dependencies);
    assert_eq!(detail["dependencies"], Value::Array(detail_dependencies));

    let unchanged_request = "計算手順の final を訂正: @{right}*3";
    let unchanged_args = [
        "--input",
        "keep-rewired-edge",
        "--context",
        rewired["id"].as_str().unwrap(),
        "--max-actions",
        "12",
        unchanged_request,
    ];
    let unchanged = record(&execute(&scratch.0, "owner", &unchanged_args));
    let unchanged_span = span(unchanged_request, "@{right}*3");
    verify(
        &unchanged,
        unchanged_request,
        &rewired,
        "EXACT_CALCULATION_SEQUENCE_CORRECTION_V2",
        &unchanged_span,
    );
    assert_eq!(
        unchanged["text"],
        "訂正した手順: final\nleft: 2+3 = 5\nright: 8+1 = 9\nfinal: (9)*3 = 27"
    );
    let old_proof = sequence_artifact(&store, &unchanged["dependencies"][1]);
    assert_eq!(old_proof[0], "minidora-calculation-sequence-correction-v2");
    assert_eq!(old_proof[3], minidora_domain::digest(request.as_bytes()));
    assert_eq!(old_proof[7], revised_span);
    assert_eq!(old_proof[8], unchanged_span);
    let old_detail_args = [
        "--input",
        "old-syntax-detail",
        "--context",
        unchanged["id"].as_str().unwrap(),
        "--max-actions",
        "12",
        "詳しく説明して",
    ];
    let old_detail = record(&execute(&scratch.0, "owner", &old_detail_args));
    verify(
        &old_detail,
        "詳しく説明して",
        &unchanged,
        "EXACT_PREVIOUS_CALCULATION_SEQUENCE_V4",
        &unchanged_span,
    );
    assert_eq!(old_detail["text"], expected_detail("3", "27"));
    for (replay_args, result) in [
        (&args[..], &rewired),
        (&detail_args[..], &detail),
        (&unchanged_args[..], &unchanged),
        (&old_detail_args[..], &old_detail),
    ] {
        assert_eq!(record(&execute(&scratch.0, "owner", replay_args)), *result);
    }
    let completed = sequence_stored_step_ids(&scratch.0);
    for (input, context, budget, text, code) in [
        (
            "rewire-budget",
            &original,
            "8",
            request,
            "ACTION_BUDGET_EXHAUSTED",
        ),
        (
            "rewire-detail-budget",
            &rewired,
            "8",
            "詳しく説明して",
            "ACTION_BUDGET_EXHAUSTED",
        ),
        (
            "old-syntax-wrong-edge",
            &rewired,
            "128",
            "計算手順の final を訂正: @{left}*3",
            "SEQUENCE_CORRECTION_DEPENDENCIES_CHANGED",
        ),
        (
            "forward-edge",
            &original,
            "128",
            "計算手順の left を参照先も含めて訂正: @{right}*2",
            "SEQUENCE_CORRECTION_FORWARD_REFERENCE",
        ),
    ] {
        let result = stopped(
            &execute(
                &scratch.0,
                "owner",
                &[
                    "--input",
                    input,
                    "--context",
                    context["id"].as_str().unwrap(),
                    "--max-actions",
                    budget,
                    text,
                ],
            ),
            2,
            "SUSPEND",
        );
        assert!(
            result["reason"]
                .as_str()
                .unwrap()
                .starts_with(&format!("{code}:"))
        );
        assert_eq!(result["dependencies"], serde_json::json!([]));
        assert_eq!(result["receipts"], serde_json::json!([]));
        assert_eq!(result["guarantee"], "UNKNOWN");
        for field in [
            "calculation",
            "content_plan",
            "render",
            "execution_graph_ref",
        ] {
            assert!(result[field].is_null());
        }
    }
    assert_eq!(sequence_stored_step_ids(&scratch.0), completed);
    assert!(store.actions("owner", "normal-route").unwrap().is_empty());
    let saved = inspect_normal_case(&scratch.0, "owner");
    for prior in [&original, &rewired, &detail, &unchanged, &old_detail] {
        assert_eq!(
            saved["prior_results"][prior["id"].as_str().unwrap()],
            *prior
        );
    }
}

#[test]
fn calculation_sequence_route_appends_a_stage_using_the_actual_previous_result() {
    let scratch = Scratch::new();
    let source = "calculation-sequence:\nbase: 2+3\nfinal: @{base}*2";
    let original_args = ["--input", "append-source", source];
    let original = record(&execute(&scratch.0, "owner", &original_args));
    assert_sequence_artifacts(
        &scratch.0,
        &original,
        source,
        &[
            sequence_step("base", "2+3", "2+3", "5", &[], &[]),
            sequence_step("final", "@{base}*2", "(5)*2", "10", &["base"], &["base"]),
        ],
    );
    let store = minidora_engine::store::Store::open(&scratch.0).unwrap();
    let prior_artifacts: Vec<_> = original["dependencies"]
        .as_array()
        .unwrap()
        .iter()
        .map(|sha| {
            store
                .get_bytes("owner", "normal-route", sha.as_str().unwrap())
                .unwrap()
        })
        .collect();
    let run = |input: &str, context: &Value, budget: &str, text: &str| {
        execute(
            &scratch.0,
            "owner",
            &[
                "--input",
                input,
                "--context",
                context["id"].as_str().unwrap(),
                "--max-actions",
                budget,
                text,
            ],
        )
    };
    let check = |result: &Value, text: &str, predicate: &str| {
        basic_raw_result(&scratch.0, result, text, None);
        assert_eq!(result["receipts"], serde_json::json!([]));
        let saved = inspect_normal_case(&scratch.0, "owner");
        let goal = sequence_artifact(&store, &saved["goal_sha256"]);
        assert_eq!(
            goal["required_predicates"],
            serde_json::json!([predicate, "SUPPORTED_CONTENT", "EXACT_RENDER"])
        );
        saved
    };
    let request = "計算手順の末尾に total を追加: @{final}+2";
    let appended = record(&run("append-total", &original, "8", request));
    let saved = check(&appended, request, "EXACT_CALCULATION_SEQUENCE_APPEND_V1");
    assert_eq!(
        saved["bundle"]["meaning"]["parser_version"],
        "declarative-ja-en-v121-calculation-sequence-append"
    );
    assert_eq!(
        saved["bundle"]["meaning"]["context_result_ref"],
        original["id"]
    );
    assert_eq!(
        appended["text"],
        "追加した手順: total\nbase: 2+3 = 5\nfinal: (5)*2 = 10\ntotal: (10)+2 = 12"
    );
    assert_eq!(appended["calculation"]["exact_value"], "12");
    let dependencies = appended["dependencies"].as_array().unwrap();
    assert_eq!(dependencies.len(), 6);
    assert_eq!(dependencies[0], original["id"]);
    assert_eq!(
        &dependencies[2..4],
        &original["dependencies"].as_array().unwrap()[..2]
    );
    let start = request.find("@{final}+2").unwrap();
    let expression =
        serde_json::json!({"start": start, "end": request.len(), "text": "@{final}+2"});
    let mut prior_dependencies = vec![original["id"].clone()];
    prior_dependencies.extend_from_slice(original["dependencies"].as_array().unwrap());
    assert_eq!(
        sequence_artifact(&store, &dependencies[1]),
        serde_json::json!({
            "kind": "minidora-calculation-sequence-append-v1",
            "selected_result_id": original["id"], "original_result_id": original["id"],
            "append_input_id": appended["id"], "append_raw_sha256": minidora_domain::digest(request.as_bytes()),
            "previous_final_step_id": "final", "appended_step": {"id": "total", "expression": expression, "requires": ["final"]},
            "dependencies": prior_dependencies
        })
    );
    let total = sequence_artifact(&store, &dependencies[4]);
    assert_eq!(total["source_expression"], expression);
    assert_eq!(total["evaluated_expression"], "(10)+2");
    assert_eq!(total["calculation"], appended["calculation"]);
    assert_eq!(
        total["consumed"],
        serde_json::json!([{
            "step_id": "final", "expression_start": 0, "expression_end": 8,
            "occurrence": 0, "exact_value": "10", "artifact_sha256": original["dependencies"][1]
        }])
    );
    let graph = sequence_artifact(&store, &appended["execution_graph_ref"]);
    assert_eq!(
        graph[0]
            .as_array()
            .unwrap()
            .iter()
            .find(|n| n["id"] == "sequence-total")
            .unwrap()["requires"],
        serde_json::json!(["sequence-final"])
    );
    assert_eq!(
        graph[5]["sequence-total"],
        serde_json::json!({"sequence-final": original["dependencies"][1]})
    );
    let detail = record(&run("append-detail", &appended, "8", "詳しく説明して"));
    check(
        &detail,
        "詳しく説明して",
        "EXACT_PREVIOUS_CALCULATION_SEQUENCE_V5",
    );
    assert_eq!(
        detail["text"],
        "手順 base の元の式: 2+3\n手順 base の結果: 2+3 = 5\n手順 final の元の式: @{base}*2\n代入した先行手順の値: base = 5\n手順 final の結果: (5)*2 = 10\n手順 total の元の式: @{final}+2\n代入した先行手順の値: final = 10\n手順 total の結果: (10)+2 = 12\n最終結果: 12"
    );
    let mut expected = vec![appended["id"].clone()];
    expected.extend_from_slice(dependencies);
    assert_eq!(detail["dependencies"], Value::Array(expected));
    assert_eq!(
        record(&run("append-total", &original, "8", request)),
        appended
    );
    assert_eq!(
        record(&run("append-detail", &appended, "8", "詳しく説明して")),
        detail
    );
    assert_eq!(
        record(&execute(&scratch.0, "owner", &original_args)),
        original
    );
    assert_eq!(inspect_normal_case(&scratch.0, "owner")["result"], detail);
    let completed = sequence_stored_step_ids(&scratch.0);
    for (input, context, budget, text, code) in [
        (
            "append-short",
            &original,
            "7",
            request,
            "ACTION_BUDGET_EXHAUSTED",
        ),
        (
            "append-detail-short",
            &appended,
            "7",
            "詳しく説明して",
            "ACTION_BUDGET_EXHAUSTED",
        ),
        (
            "append-duplicate",
            &original,
            "8",
            "計算手順の末尾に final を追加: 1",
            "SEQUENCE_APPEND_DUPLICATE_STEP",
        ),
    ] {
        let result = stopped(&run(input, context, budget, text), 2, "SUSPEND");
        assert!(
            result["reason"]
                .as_str()
                .unwrap()
                .starts_with(&format!("{code}:"))
        );
        assert_eq!(result["dependencies"], serde_json::json!([]));
        for field in [
            "calculation",
            "content_plan",
            "render",
            "execution_graph_ref",
        ] {
            assert!(result[field].is_null());
        }
    }
    assert_eq!(sequence_stored_step_ids(&scratch.0), completed);
    assert!(store.actions("owner", "normal-route").unwrap().is_empty());
    let saved = inspect_normal_case(&scratch.0, "owner");
    for prior in [&original, &appended, &detail] {
        assert_eq!(
            saved["prior_results"][prior["id"].as_str().unwrap()],
            *prior
        );
    }
    for (sha, bytes) in original["dependencies"]
        .as_array()
        .unwrap()
        .iter()
        .zip(prior_artifacts)
    {
        assert_eq!(
            store
                .get_bytes("owner", "normal-route", sha.as_str().unwrap())
                .unwrap(),
            bytes
        );
    }
}

#[test]
fn calculation_sequence_route_removes_only_an_unreferenced_stage() {
    let scratch = Scratch::new();
    let source = "calculation-sequence:\nbase: 2+3\nspare: 40+2\nfinal: @{base}*2";
    let original_args = ["--input", "remove-source", source];
    let original = record(&execute(&scratch.0, "owner", &original_args));
    let selected = original["id"].as_str().unwrap();
    let remove = |input, text| {
        execute(
            &scratch.0,
            "owner",
            &[
                "--input",
                input,
                "--context",
                selected,
                "--max-actions",
                "8",
                text,
            ],
        )
    };

    let request = "計算手順の spare を削除";
    let removed = record(&remove("remove-spare", request));
    assert_eq!(removed["calculation"]["exact_value"], "10");
    assert_eq!(
        removed["text"],
        "削除した手順: spare\nbase: 2+3 = 5\nfinal: (5)*2 = 10"
    );
    assert_eq!(removed["dependencies"][0], original["id"]);
    assert_eq!(removed["dependencies"][2], original["dependencies"][0]);
    assert_eq!(removed["dependencies"][3], original["dependencies"][2]);
    assert_eq!(removed["dependencies"].as_array().unwrap().len(), 5);
    let store = minidora_engine::store::Store::open(&scratch.0).unwrap();
    let proof = sequence_artifact(&store, &removed["dependencies"][1]);
    assert_eq!(proof["kind"], "minidora-calculation-sequence-remove-v1");
    assert_eq!(proof["removed_step_source_input_id"], original["id"]);
    assert_eq!(
        proof["removed_step_source_sha256"],
        minidora_domain::digest(source.as_bytes())
    );
    assert_eq!(proof["removal_input_id"], removed["id"]);
    assert_eq!(
        proof["removal_raw_sha256"],
        minidora_domain::digest(request.as_bytes())
    );
    assert_eq!(proof["removed_step"]["id"], "spare");
    assert_eq!(proof["removed_step"]["expression"]["text"], "40+2");
    assert_eq!(proof["previous_final_step_id"], "final");
    assert_eq!(proof["revised_final_step_id"], "final");
    let graph = sequence_artifact(&store, &removed["execution_graph_ref"]);
    assert!(
        !graph[0]
            .as_array()
            .unwrap()
            .iter()
            .any(|node| node["id"] == "sequence-spare")
    );

    let terminal = record(&remove("remove-terminal", "計算手順の final を削除"));
    assert_eq!(terminal["calculation"]["exact_value"], "42");
    let terminal_proof = sequence_artifact(&store, &terminal["dependencies"][1]);
    assert_eq!(terminal_proof["previous_final_step_id"], "final");
    assert_eq!(terminal_proof["revised_final_step_id"], "spare");
    assert_eq!(record(&remove("remove-spare", request)), removed);
    assert_eq!(
        record(&execute(&scratch.0, "owner", &original_args)),
        original
    );

    let blocked = stopped(
        &remove("remove-consumed", "計算手順の base を削除"),
        2,
        "SUSPEND",
    );
    let reason = blocked["reason"].as_str().unwrap();
    assert!(reason.starts_with("SEQUENCE_REMOVE_STEP_IN_USE:") && reason.contains("final"));
    let target = scratch.0.join("question-output.txt");
    let path = target.to_str().unwrap();
    let question = execute(
        &scratch.0,
        "owner",
        &[
            "--input",
            "remove-question",
            "--context",
            selected,
            "--output",
            path,
            "--allow-write",
            path,
            "計算手順の spare を削除？。",
        ],
    );
    let unresolved = stopped(&question, 2, "SUSPEND");
    let reason = unresolved["reason"].as_str().unwrap();
    assert!(reason.contains("CAPABILITY_UNAVAILABLE") && reason.contains("疑問形"));
    assert!(!target.exists());
    assert!(store.actions("owner", "normal-route").unwrap().is_empty());
}

#[test]
fn calculation_sequence_route_moves_stage_only_when_dependencies_stay_earlier() {
    let scratch = Scratch::new();
    let source = "calculation-sequence:\nbase: 2+3\nspare: 40+2\nfinal: @{base}*2";
    let original_args = ["--input", "move-source", source];
    let original = record(&execute(&scratch.0, "owner", &original_args));
    let selected = original["id"].as_str().unwrap();
    let request = "計算手順の spare を final の後に移動";
    let args = [
        "--input",
        "move-spare",
        "--context",
        selected,
        "--max-actions",
        "9",
        request,
    ];
    let moved = record(&execute(&scratch.0, "owner", &args));
    assert_eq!(moved["calculation"]["exact_value"], "42");
    assert_eq!(
        moved["text"],
        "移動した手順: spare\nbase: 2+3 = 5\nfinal: (5)*2 = 10\nspare: 40+2 = 42"
    );
    let dependencies = moved["dependencies"].as_array().unwrap();
    assert_eq!(dependencies.len(), 6);
    assert_eq!(dependencies[0], original["id"]);
    assert_eq!(
        &dependencies[2..5],
        &[
            original["dependencies"][0].clone(),
            original["dependencies"][2].clone(),
            original["dependencies"][1].clone()
        ]
    );
    let store = minidora_engine::store::Store::open(&scratch.0).unwrap();
    let proof = sequence_artifact(&store, &dependencies[1]);
    assert_eq!(proof["kind"], "minidora-calculation-sequence-move-v1");
    assert_eq!(proof["move_input_id"], moved["id"]);
    assert_eq!(
        proof["move_raw_sha256"],
        minidora_domain::digest(request.as_bytes())
    );
    assert_eq!(proof["step_id"], "spare");
    assert_eq!(proof["anchor_step_id"], "final");
    assert_eq!(proof["before"], false);
    assert_eq!(
        proof["previous_step_order"],
        serde_json::json!(["base", "spare", "final"])
    );
    assert_eq!(
        proof["revised_step_order"],
        serde_json::json!(["base", "final", "spare"])
    );
    let graph = sequence_artifact(&store, &moved["execution_graph_ref"]);
    assert_eq!(graph[0][1]["id"], "sequence-final");
    assert_eq!(
        graph[0][1]["requires"],
        serde_json::json!(["sequence-base"])
    );
    assert_eq!(graph[0][2]["id"], "sequence-spare");
    assert_eq!(record(&execute(&scratch.0, "owner", &args)), moved);
    assert_eq!(
        record(&execute(&scratch.0, "owner", &original_args)),
        original
    );

    let target = scratch.0.join("disallowed-move.txt");
    let path = target.to_str().unwrap();
    for (input, text, code) in [
        (
            "move-prerequisite",
            "計算手順の base を final の後に移動",
            "SEQUENCE_MOVE_DEPENDENCY_ORDER",
        ),
        (
            "move-question",
            "計算手順の spare を final の後に移動？。",
            "CAPABILITY_UNAVAILABLE",
        ),
    ] {
        let status = stopped(
            &execute(
                &scratch.0,
                "owner",
                &[
                    "--input",
                    input,
                    "--context",
                    selected,
                    "--output",
                    path,
                    "--allow-write",
                    path,
                    text,
                ],
            ),
            2,
            "SUSPEND",
        );
        assert!(status["reason"].as_str().unwrap().contains(code));
        assert!(!target.exists());
    }
    assert!(store.actions("owner", "normal-route").unwrap().is_empty());
}

#[test]
fn calculation_sequence_route_inserts_stage_with_only_earlier_references() {
    let scratch = Scratch::new();
    let source = "calculation-sequence:\nbase: 2+3\nfinal: @{base}*2";
    let original_args = ["--input", "insert-source", source];
    let original = record(&execute(&scratch.0, "owner", &original_args));
    let selected = original["id"].as_str().unwrap();
    let request = "計算手順の fee を final の前に挿入: @{base}+2";
    let args = [
        "--input",
        "insert-fee",
        "--context",
        selected,
        "--max-actions",
        "8",
        request,
    ];
    let inserted = record(&execute(&scratch.0, "owner", &args));
    assert_eq!(inserted["calculation"]["exact_value"], "10");
    assert_eq!(
        inserted["text"],
        "挿入した手順: fee\nbase: 2+3 = 5\nfee: (5)+2 = 7\nfinal: (5)*2 = 10"
    );
    let dependencies = inserted["dependencies"].as_array().unwrap();
    assert_eq!(dependencies.len(), 6);
    assert_eq!(dependencies[0], original["id"]);
    assert_eq!(dependencies[2], original["dependencies"][0]);
    assert_eq!(dependencies[4], original["dependencies"][1]);
    let store = minidora_engine::store::Store::open(&scratch.0).unwrap();
    let proof = sequence_artifact(&store, &dependencies[1]);
    assert_eq!(proof["kind"], "minidora-calculation-sequence-insert-v1");
    assert_eq!(proof["insert_input_id"], inserted["id"]);
    assert_eq!(
        proof["insert_raw_sha256"],
        minidora_domain::digest(request.as_bytes())
    );
    assert_eq!(proof["anchor_step_id"], "final");
    assert_eq!(proof["before"], true);
    assert_eq!(proof["inserted_step"]["expression"]["text"], "@{base}+2");
    assert_eq!(
        proof["inserted_step"]["requires"],
        serde_json::json!(["base"])
    );
    assert_eq!(
        proof["previous_step_order"],
        serde_json::json!(["base", "final"])
    );
    assert_eq!(
        proof["revised_step_order"],
        serde_json::json!(["base", "fee", "final"])
    );
    let step = sequence_artifact(&store, &dependencies[3]);
    assert_eq!(
        step["consumed"][0]["artifact_sha256"],
        original["dependencies"][0]
    );

    let target = scratch.0.join("forward-insertion.txt");
    let path = target.to_str().unwrap();
    let blocked = stopped(
        &execute(
            &scratch.0,
            "owner",
            &[
                "--input",
                "insert-forward",
                "--context",
                selected,
                "--output",
                path,
                "--allow-write",
                path,
                "計算手順の total を final の前に挿入: @{final}+2",
            ],
        ),
        2,
        "SUSPEND",
    );
    assert!(
        blocked["reason"]
            .as_str()
            .unwrap()
            .contains("SEQUENCE_INSERT_FORWARD_REFERENCE")
    );
    assert!(!target.exists());

    let after = record(&execute(
        &scratch.0,
        "owner",
        &[
            "--input",
            "insert-after",
            "--context",
            selected,
            "--max-actions",
            "8",
            "計算手順の total を final の後に挿入: @{final}+2",
        ],
    ));
    assert_eq!(after["calculation"]["exact_value"], "12");
    let after_proof = sequence_artifact(&store, &after["dependencies"][1]);
    assert_eq!(
        after_proof["kind"],
        "minidora-calculation-sequence-insert-v1"
    );
    assert_eq!(after_proof["before"], false);
    assert_eq!(record(&execute(&scratch.0, "owner", &args)), inserted);
    assert_eq!(
        record(&execute(&scratch.0, "owner", &original_args)),
        original
    );
    assert!(store.actions("owner", "normal-route").unwrap().is_empty());
}

#[test]
fn calculation_sequence_route_history_shows_only_edits_of_selected_edition() {
    let scratch = Scratch::new();
    let source = "calculation-sequence:\nbase: 2+3\nfinal: @{base}*2";
    let original = record(&execute(
        &scratch.0,
        "owner",
        &["--input", "history-root", source],
    ));
    let root_id = original["id"].as_str().unwrap();
    let inserted = record(&execute(
        &scratch.0,
        "owner",
        &[
            "--input",
            "history-insert",
            "--context",
            root_id,
            "--max-actions",
            "8",
            "計算手順の fee を final の前に挿入: @{base}+2",
        ],
    ));
    let inserted_id = inserted["id"].as_str().unwrap();
    let command = "計算手順の変更履歴を表示";
    let root_row = format!("元の計算: 最終段 final = 10 [結果ID: {root_id}]");
    let expected =
        format!("{root_row}\n1. 挿入 fee (final の前): 最終段 final = 10 [結果ID: {inserted_id}]",);
    let history = record(&execute(
        &scratch.0,
        "owner",
        &[
            "--input",
            "history-show",
            "--context",
            inserted_id,
            "--max-actions",
            "8",
            command,
        ],
    ));
    assert_eq!(history["calculation"]["exact_value"], "10");
    assert_eq!(history["text"], expected);
    assert_eq!(
        history["content_plan"]["required_units"],
        serde_json::json!(expected.lines().collect::<Vec<_>>())
    );
    assert_eq!(history["dependencies"][0], inserted_id);

    let repeated = record(&execute(
        &scratch.0,
        "owner",
        &[
            "--input",
            "history-repeat",
            "--context",
            history["id"].as_str().unwrap(),
            "--max-actions",
            "9",
            "計算手順の変更履歴を表示して。",
        ],
    ));
    assert_eq!(repeated["text"], expected);
    assert_eq!(repeated["dependencies"][0], history["id"]);
    assert_eq!(repeated["dependencies"][1], inserted_id);
    let older = record(&execute(
        &scratch.0,
        "owner",
        &[
            "--input",
            "history-older",
            "--context",
            root_id,
            "--max-actions",
            "5",
            command,
        ],
    ));
    assert_eq!(older["text"], root_row);
    assert_eq!(older["dependencies"][0], root_id);

    let low = stopped(
        &execute(
            &scratch.0,
            "owner",
            &[
                "--input",
                "history-low",
                "--context",
                inserted_id,
                "--max-actions",
                "7",
                command,
            ],
        ),
        2,
        "SUSPEND",
    );
    assert!(
        low["reason"]
            .as_str()
            .unwrap()
            .contains("ACTION_BUDGET_EXHAUSTED")
    );
    let store = minidora_engine::store::Store::open(&scratch.0).unwrap();
    assert!(store.actions("owner", "normal-route").unwrap().is_empty());
}

#[test]
fn calculation_sequence_route_explains_one_step_and_preserves_edition_context() {
    let scratch = Scratch::new();
    let source = "calculation-sequence:\nbase: 2+3\nfinal: @{base}*2";
    let original = record(&execute(
        &scratch.0,
        "owner",
        &["--input", "step-root", source],
    ));
    let original_id = original["id"].as_str().unwrap();
    let edit = "計算手順の fee を final の前に挿入: @{base}+2";
    let inserted = record(&execute(
        &scratch.0,
        "owner",
        &["--input", "step-insert", "--context", original_id, edit],
    ));
    let inserted_id = inserted["id"].as_str().unwrap();
    let expression_start = edit.find("@{base}+2").unwrap();
    let source_sha = minidora_domain::digest(edit.as_bytes());
    let source_end = edit.len();
    let expected = format!(
        "手順 fee の元の式: @{{base}}+2\n代入した先行手順の値: base = 5\n手順 fee の結果: (5)+2 = 7\n式の出典: 結果ID {inserted_id}、原文SHA-256 {source_sha}、UTF-8バイト [{expression_start}, {source_end})"
    );
    let command = "計算手順の fee を詳しく説明して。";
    let details = record(&execute(
        &scratch.0,
        "owner",
        &[
            "--input",
            "step-detail",
            "--context",
            inserted_id,
            "--max-actions",
            "8",
            command,
        ],
    ));
    assert_eq!(details["calculation"]["exact_value"], "7");
    assert_eq!(details["text"], expected);
    assert_eq!(details["dependencies"][0], inserted_id);
    let history = record(&execute(
        &scratch.0,
        "owner",
        &[
            "--input",
            "step-history",
            "--context",
            details["id"].as_str().unwrap(),
            "--max-actions",
            "9",
            "計算手順の変更履歴を表示",
        ],
    ));
    assert_eq!(history["calculation"]["exact_value"], "10");
    assert_eq!(history["text"].as_str().unwrap().lines().count(), 2);
    assert_eq!(history["dependencies"][0], details["id"]);
    assert_eq!(history["dependencies"][1], inserted_id);
    let skipped = stopped(
        &execute(
            &scratch.0,
            "owner",
            &["--input", "step-unknown", "--context", original_id, command],
        ),
        2,
        "SUSPEND",
    );
    assert!(
        skipped["reason"]
            .as_str()
            .unwrap()
            .contains("SEQUENCE_EXPLANATION_STEP_UNKNOWN")
    );
    let store = minidora_engine::store::Store::open(&scratch.0).unwrap();
    assert!(store.actions("owner", "normal-route").unwrap().is_empty());
}

#[test]
fn calculation_sequence_route_file_effect_is_confirmed_once() {
    let scratch = Scratch::new();
    let path = scratch.0.join("sequence-result.txt");
    let path_text = path.to_str().unwrap();
    let request = "calculation-sequence:\nu: 2+3\nfinal: @{u}*2";
    let expected = [
        sequence_step("u", "2+3", "2+3", "5", &[], &[]),
        sequence_step("final", "@{u}*2", "(5)*2", "10", &["u"], &["u"]),
    ];
    let args = [
        "--input",
        "sequence-file",
        "--max-actions",
        "6",
        "--output",
        path_text,
        "--allow-write",
        path_text,
        request,
    ];
    let result = record(&execute(&scratch.0, "owner", &args));
    assert_sequence_artifacts(&scratch.0, &result, request, &expected);
    let bytes = std::fs::read(&path).unwrap();
    assert_eq!(bytes, result["text"].as_str().unwrap().as_bytes());
    assert_eq!(result["receipts"].as_array().unwrap().len(), 1);
    assert_eq!(result["receipts"][0]["phase"], "CONFIRMED");
    assert_eq!(result["receipts"][0]["target"], path_text);
    assert_eq!(result["receipts"][0]["observed_bytes"], bytes.len());
    assert_eq!(
        result["receipts"][0]["content_sha256"],
        minidora_domain::digest(&bytes)
    );
    let modified = std::fs::metadata(&path).unwrap().modified().unwrap();
    assert_eq!(record(&execute(&scratch.0, "owner", &args)), result);
    assert_eq!(std::fs::read(&path).unwrap(), bytes);
    assert_eq!(
        std::fs::metadata(&path).unwrap().modified().unwrap(),
        modified
    );
    let store = minidora_engine::store::Store::open(&scratch.0).unwrap();
    let actions = store.actions("owner", "normal-route").unwrap();
    assert_eq!(actions.len(), 1);
    assert_eq!(
        serde_json::to_value(actions[0].receipt.as_ref().unwrap()).unwrap(),
        result["receipts"][0]
    );
}

struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Self {
        let path = if let Some(path) = std::env::var_os("MINIDORA_PREVIOUS_TEXT_EDIT_STATE_DIR") {
            PathBuf::from(path)
        } else if let Some(path) = std::env::var_os("MINIDORA_SOURCE_QUESTION_STATE_DIR") {
            PathBuf::from(path)
        } else {
            let epoch = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let serial = NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed);
            std::env::temp_dir().join(format!(
                "minidora-cli-{}-{epoch}-{serial}",
                std::process::id()
            ))
        };
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
    execute_in_case(directory, actor, "normal-route", args)
}

fn execute_in_case(directory: &Path, actor: &str, case: &str, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_minidora-rust"))
        .args([
            "--db",
            directory.to_str().unwrap(),
            "--case",
            case,
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

fn assert_why_generation(directory: &Path, actor: &str, case: &str, result: &Value) -> Value {
    assert_eq!(result["lifecycle"], "COMMIT");
    let store = minidora_engine::store::Store::open(directory).unwrap();
    let plan = &result["content_plan"];
    let compute_ref = plan["dependencies"].as_array().unwrap().last().unwrap();
    let artifact: Value = serde_json::from_slice(
        &store
            .get_bytes(actor, case, compute_ref.as_str().unwrap())
            .unwrap(),
    )
    .unwrap();
    let generation = &artifact["content_generation"];
    assert_eq!(generation["schema"], "core-meaning-generation-v1");
    assert_eq!(generation["stop_reason"], "EOS");
    assert_eq!(generation["text"], result["text"]);
    assert_eq!(generation["text"], plan["text"]);
    assert_eq!(generation["model_revision"], plan["model_revision"]);
    assert_eq!(generation["model_artifact_ref"], plan["model_artifact_ref"]);
    let steps = generation["token_steps"].as_array().unwrap();
    assert_eq!(steps.last().unwrap()["token"], "<EOS>");
    assert!(steps.iter().any(|step| {
        step["distribution"]
            .as_array()
            .is_some_and(|distribution| distribution.len() > 1)
    }));
    let proof_set = |values: &Value| {
        values
            .as_array()
            .unwrap()
            .iter()
            .map(|value| value.as_str().unwrap().to_owned())
            .collect::<std::collections::BTreeSet<_>>()
    };
    assert_eq!(
        proof_set(&generation["evidence_ids"]),
        proof_set(&result["dependencies"])
    );
    assert_eq!(
        artifact["relation_proof_dependencies"],
        result["dependencies"]
    );
    let required_units = generation["units"]
        .as_array()
        .unwrap()
        .iter()
        .map(|unit| unit["text"].clone())
        .collect();
    assert_eq!(plan["required_units"], Value::Array(required_units));
    let observation: Value = serde_json::from_slice(
        &store
            .get_bytes(actor, case, plan["model_observation_ref"].as_str().unwrap())
            .unwrap(),
    )
    .unwrap();
    assert_eq!(observation["text"], result["text"]);
    assert_eq!(
        observation["decoder_version"],
        generation["decoder_version"]
    );
    assert!(observation["authorized_surfaces"].is_null());
    assert_eq!(result["render"]["status"], "RENDERED");
    assert_eq!(
        result["render"]["bytes"],
        serde_json::json!(result["text"].as_str().unwrap().as_bytes())
    );
    artifact
}

fn assert_why_record<'a>(artifact: &'a Value, kind: &str, record_id: &Value) -> &'a Value {
    let unit = artifact["relation_meaning_plan"]["units"]
        .as_array()
        .unwrap()
        .iter()
        .find(|unit| unit["meaning"]["kind"] == kind && &unit["meaning"]["record_id"] == record_id)
        .unwrap();
    let trace = artifact["content_generation"]["units"]
        .as_array()
        .unwrap()
        .iter()
        .find(|trace| trace["unit_id"] == unit["id"])
        .unwrap();
    assert!(
        trace["evidence_ids"]
            .as_array()
            .unwrap()
            .contains(record_id)
    );
    assert!(
        trace["text"]
            .as_str()
            .unwrap()
            .contains(record_id.as_str().unwrap())
    );
    &unit["meaning"]
}

fn assert_why_proposition(
    proposition: &Value,
    subject: &str,
    object: &str,
    scope: &str,
    negative: bool,
) {
    assert_eq!(proposition["subject"], subject);
    assert_eq!(proposition["predicate"], "is");
    assert_eq!(proposition["object"], object);
    assert_eq!(proposition["scope"], scope);
    assert_eq!(proposition["negative"], negative);
}

#[test]
fn scalar_arithmetic_generation_preserves_exact_fields_and_real_render() {
    let scratch = Scratch::new();
    let wide_rational = format!("1{}", "0".repeat(8000));
    for (input_id, expression, rational, unit, classification) in [
        ("scalar", "2/3-3/2", "-5/6", None, "負の分数です。"),
        (
            "quantity",
            "0.5 m - 2 m",
            "-3/2",
            Some("m"),
            "負の分数で表される数量です。",
        ),
        (
            "unicode-minus-quantity",
            "0.5 m − 2 m",
            "-3/2",
            Some("m"),
            "負の分数で表される数量です。",
        ),
        (
            "expression-whitespace",
            "2\u{000c}+3",
            "5",
            None,
            "正の整数です。",
        ),
        (
            "wide-integer",
            "100000000^1000",
            wide_rational.as_str(),
            None,
            "正の整数です。",
        ),
    ] {
        let output_path = scratch.0.join(format!("{input_id}.txt"));
        let path = output_path.to_str().unwrap();
        let result = record(&execute(
            &scratch.0,
            "owner",
            &[
                "--input",
                input_id,
                "--output",
                path,
                "--allow-write",
                path,
                expression,
            ],
        ));
        assert_eq!(result["lifecycle"], "COMMIT");
        assert_eq!(result["guarantee"], "COMPUTED");
        let exact_value =
            unit.map_or_else(|| rational.to_owned(), |unit| format!("{rational} {unit}"));
        let calculation = &result["calculation"];
        assert_eq!(calculation["expression"], expression);
        assert_eq!(calculation["exact_value"], exact_value);
        let rendered = result["text"].as_str().unwrap();
        assert!(rendered.starts_with(&format!("{expression} = {exact_value}。")));
        assert!(rendered.ends_with(classification));
        assert_eq!(result["render"]["status"], "RENDERED");
        assert_eq!(
            result["render"]["bytes"],
            serde_json::json!(rendered.as_bytes())
        );
        assert_eq!(std::fs::read(&output_path).unwrap(), rendered.as_bytes());
        assert_eq!(result["receipts"][0]["phase"], "CONFIRMED");

        let store = minidora_engine::store::Store::open(&scratch.0).unwrap();
        let saved = inspect_normal_case(&scratch.0, "owner");
        assert_eq!(saved["bundle"]["request"]["text"], expression);
        assert_eq!(
            saved["bundle"]["meaning"]["parser_version"],
            minidora_engine::input::CALCULATION_PARSER_VERSION
        );
        let goal = sequence_artifact(&store, &saved["goal_sha256"]);
        assert_eq!(
            goal["required_predicates"],
            serde_json::json!([
                "EXACT_CALCULATION_V2",
                "SUPPORTED_CONTENT",
                "EXACT_RENDER",
                "CONFIRMED_FILE"
            ])
        );
        let plan = &result["content_plan"];
        let compute_ref = plan["dependencies"].as_array().unwrap().last().unwrap();
        let artifact = sequence_artifact(&store, compute_ref);
        assert_eq!(artifact["kind"], "minidora.exact-calculation-content.v1");
        let meaning_plan = &artifact["meaning_plan"];
        assert_eq!(meaning_plan["expression"], expression);
        assert_eq!(meaning_plan["rational"], rational);
        assert_eq!(meaning_plan["unit"], serde_json::json!(unit));
        assert_eq!(
            meaning_plan["calculation_sha256"],
            minidora_domain::identity(calculation).unwrap()
        );

        let generation = &artifact["content_generation"];
        assert_eq!(generation["schema"], "core-meaning-generation-v1");
        assert_eq!(generation["text"], rendered);
        assert_eq!(generation["stop_reason"], "EOS");
        assert_eq!(generation["model_revision"], plan["model_revision"]);
        assert_eq!(generation["model_artifact_ref"], plan["model_artifact_ref"]);
        assert_eq!(plan["required_units"], serde_json::json!([rendered]));
        let steps = generation["token_steps"].as_array().unwrap();
        assert_eq!(steps.last().unwrap()["token"], "<EOS>");
        assert!(
            steps
                .iter()
                .any(|step| step["distribution"].as_array().unwrap().len() > 1)
        );
        for exact_field in [expression, rational].into_iter().chain(unit) {
            let step = steps
                .iter()
                .find(|step| step["token"] == exact_field)
                .unwrap();
            assert_eq!(step["admissible_tokens"], serde_json::json!([exact_field]));
            assert_eq!(
                step["distribution"],
                serde_json::json!([{"token": exact_field, "probability": "1"}])
            );
        }
    }
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
    let explanation_text = explanation["text"].as_str().unwrap();
    assert!(explanation_text.contains("2+3"));
    assert!(explanation_text.contains("2 + 3 = 5"));
    assert!(explanation_text.contains('5'));
    assert!(
        explanation_text.contains("答えは") || explanation_text.contains("計算結果は"),
        "explanation must connect the retained calculation to its result"
    );
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
fn operand_correction_requires_a_step_when_the_context_originated_in_a_sequence() {
    let scratch = Scratch::new();
    let sequence = record(&execute(
        &scratch.0,
        "owner",
        &[
            "--input",
            "sequence",
            "計算手順を実行して:\nbase: 1 km + 500 m\nfinal: 100 m + @{base}",
        ],
    ));
    assert_eq!(sequence["lifecycle"], "COMMIT");
    assert_eq!(sequence["calculation"]["exact_value"], "1600 m");
    let detail = record(&execute(
        &scratch.0,
        "owner",
        &["--input", "detail", "詳しく説明して"],
    ));
    let repeated = record(&execute(
        &scratch.0,
        "owner",
        &["--input", "detail-again", "詳しく説明して"],
    ));
    for (input, context) in [
        ("correct-original", sequence),
        ("correct-detail", detail),
        ("correct-detail-again", repeated),
    ] {
        assert_eq!(context["lifecycle"], "COMMIT");
        let result = stopped(
            &execute(
                &scratch.0,
                "owner",
                &[
                    "--input",
                    input,
                    "--context",
                    context["id"].as_str().unwrap(),
                    "第二項を600 mに変更して",
                ],
            ),
            2,
            "SUSPEND",
        );
        let reason = result["reason"].as_str().unwrap();
        assert!(reason.starts_with("SEQUENCE_CORRECTION_REQUIRES_STEP:"));
        assert_eq!(
            reason.matches("SEQUENCE_CORRECTION_REQUIRES_STEP").count(),
            1
        );
        assert!(result["calculation"].is_null());
        assert!(result["receipts"].as_array().unwrap().is_empty());
    }
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
fn input_quote_and_unresolved_subject_boundaries_prevent_premises_and_effects() {
    let scratch = Scratch::new();
    let actor = "owner";
    let output = scratch.0.join("quoted-source.txt");
    let target = output.to_str().unwrap();
    let store = minidora_engine::store::Store::open(&scratch.0).unwrap();
    let before = store.load_memory(actor).unwrap();
    for (id, text) in [
        ("angle", "〈ネラは測定器〉"),
        ("scoped", "[scope=lab]《ネラは測定器》"),
        (
            "timed",
            "[scope=lab][time=2026-11-19T08:00:00Z]「ネラは測定器」",
        ),
        ("fullwidth", "＂Nera is a sensor＂"),
        ("label", "引用: 「ネラは測定器」"),
        ("markdown", "> ネラは測定器"),
        ("embedded-label", "[scope=lab]引用：中止"),
        ("plural", "これらは測定器"),
        ("they", "They are sensors"),
        ("common-pronoun", "[scope=lab]私と乙の共通点は何ですか"),
        ("object-quote", "[scope=lab]〈ネラ〉は何を使いますか"),
        ("unresolved-time", "今朝ネラは測定器"),
        ("invalid-time", "[time=2026-02-30T08:00:00Z]ネラは測定器"),
        ("unknown-tag", "[scope=lab][unknown=x]ネラは測定器"),
        (
            "qualified-correction",
            "訂正: [time=2026-11-19T08:00:00Z]それは測定器ではなく装置です",
        ),
    ] {
        let result = stopped(
            &execute(
                &scratch.0,
                actor,
                &[
                    "--input",
                    id,
                    "--output",
                    target,
                    "--allow-write",
                    target,
                    text,
                ],
            ),
            2,
            "SUSPEND",
        );
        let state = inspect_normal_case(&scratch.0, actor);
        let bundle = &state["bundle"];
        assert_eq!(bundle["request"]["text"], text);
        assert_eq!(
            bundle["raw_sha256"],
            minidora_domain::digest(text.as_bytes())
        );
        assert_eq!(bundle["meaning"]["intent"]["kind"], "UNKNOWN");
        let diagnostic = bundle["meaning"]["unresolved"][0].as_str().unwrap();
        assert!(result["reason"].as_str().unwrap().contains(diagnostic));
        assert_eq!(result["guarantee"], "UNKNOWN");
        assert!(result["content_plan"].is_null());
        assert_eq!(result["receipts"], serde_json::json!([]));
        assert!(!output.exists());
    }
    assert_eq!(store.load_memory(actor).unwrap(), before);
    assert!(store.actions(actor, "normal-route").unwrap().is_empty());
}

#[test]
fn requested_file_needs_separate_exact_permission_and_real_receipt() {
    let scratch = Scratch::new();
    let output_path = scratch.0.join("answer.txt");
    let target = serde_json::json!({"kind": "LOCAL_FILE", "path": output_path});
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
    let denied = stopped(&denied, 2, "SUSPEND");
    assert_eq!(denied["render"]["status"], "RENDERED");
    assert_eq!(denied["render"]["target"], target);
    assert_eq!(denied["receipts"], serde_json::json!([]));
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
    let render = &permitted["render"];
    assert_eq!(render["renderer_version"], "utf8-bytes-v2");
    assert_eq!(render["target"], target);
    assert_eq!(render["bytes"], serde_json::json!(actual.as_bytes()));
    assert_eq!(
        render["actual_sha256"],
        minidora_domain::digest(actual.as_bytes())
    );
    assert_eq!(render["plan_id"], permitted["content_plan"]["id"]);
    let receipt = &permitted["receipts"][0];
    assert_eq!(receipt["phase"], "CONFIRMED");
    assert_eq!(receipt["target"], target["path"]);
    assert_eq!(receipt["content_sha256"], render["actual_sha256"]);
    assert_eq!(receipt["observed_bytes"], actual.len());
    let store = minidora_engine::store::Store::open(&scratch.0).unwrap();
    let actions = store.actions("owner", "normal-route").unwrap();
    assert_eq!(actions.len(), 1);
    assert_eq!(
        serde_json::to_value(actions[0].receipt.as_ref().unwrap()).unwrap(),
        *receipt
    );
    let case = inspect_normal_case(&scratch.0, "owner");
    let goal = sequence_artifact(&store, &case["goal_sha256"]);
    assert_eq!(goal["renderer_version"], render["renderer_version"]);
    assert_eq!(goal["output_path"], target["path"]);
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

fn http_get(address: SocketAddr, target: &str) -> (u16, String, String) {
    let mut stream = TcpStream::connect_timeout(&address, Duration::from_secs(3)).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    write!(
        stream,
        "GET {target} HTTP/1.1\r\nHost: {address}\r\nConnection: close\r\n\r\n"
    )
    .unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).unwrap();
    let (headers, body) = response.split_once("\r\n\r\n").unwrap();
    let status = headers.split(' ').nth(1).unwrap().parse().unwrap();
    (status, headers.to_ascii_lowercase(), body.to_owned())
}

fn http_stream_with_accept(
    address: SocketAddr,
    token: &str,
    body: &[u8],
    accept: &str,
) -> (u16, String, String) {
    let mut stream = TcpStream::connect_timeout(&address, Duration::from_secs(3)).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    write!(stream, "POST /v1/execute/stream HTTP/1.1\r\nHost: {address}\r\nAuthorization: Bearer {token}\r\nContent-Type: application/json\r\nAccept: {accept}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len()).unwrap();
    stream.write_all(body).unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).unwrap();
    let (headers, body) = response.split_once("\r\n\r\n").unwrap();
    let status = headers.split(' ').nth(1).unwrap().parse().unwrap();
    (status, headers.to_ascii_lowercase(), body.to_owned())
}

fn http_stream(address: SocketAddr, token: &str, body: &[u8]) -> (u16, String, String) {
    http_stream_with_accept(address, token, body, "text/event-stream")
}

fn sse_events(body: &str) -> Vec<(String, Value)> {
    body.split("\n\n")
        .filter(|frame| !frame.trim().is_empty())
        .map(|frame| {
            let mut event = None;
            let mut data = Vec::new();
            for line in frame.lines() {
                if let Some(value) = line.strip_prefix("event:") {
                    event = Some(value.trim().to_owned());
                } else if let Some(value) = line.strip_prefix("data:") {
                    data.push(value.trim_start());
                }
            }
            (
                event.expect("every server event has a name"),
                serde_json::from_str(&data.join("\n")).expect("event data is JSON"),
            )
        })
        .collect()
}

#[test]
fn loopback_browser_assets_are_fixed_and_security_headered() {
    let scratch = Scratch::new();
    let token = "test-only-constant-not-a-secret-0123456789";
    let child = Command::new(env!("CARGO_BIN_EXE_minidora-rust"))
        .args([
            "serve",
            "--db",
            scratch.0.to_str().unwrap(),
            "--actor",
            "browser-owner",
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

    let (status, headers, page) = http_get(address, "/");
    assert_eq!(status, 200);
    assert!(headers.contains("content-type: text/html; charset=utf-8"));
    assert!(headers.contains("content-security-policy: default-src 'self'"));
    assert!(headers.contains("script-src 'self'; style-src 'self'; connect-src 'self'"));
    assert!(headers.contains("x-content-type-options: nosniff"));
    assert!(headers.contains("referrer-policy: no-referrer"));
    assert!(page.contains("/app.js") && page.contains("/styles.css"));
    assert!(page.contains("/favicon.svg"));
    assert!(page.contains("method=\"post\" action=\"/v1/execute\""));
    assert!(!page.contains("name=\"token\""));
    assert!(page.contains("id=\"cancel-case-button\""));
    assert!(page.contains("記録済みの結果や外部作用は元に戻しません"));

    let (status, headers, script) = http_get(address, "/app.js");
    assert_eq!(status, 200);
    assert!(headers.contains("content-type: application/javascript; charset=utf-8"));
    assert!(script.contains("fetch(\"/v1/execute/stream\""));
    assert!(!script.contains("localStorage"));
    assert!(!script.contains("innerHTML"));

    let (status, headers, stylesheet) = http_get(address, "/styles.css");
    assert_eq!(status, 200);
    assert!(headers.contains("content-type: text/css; charset=utf-8"));
    assert!(stylesheet.contains("@media (max-width: 540px)"));

    let (status, headers, favicon) = http_get(address, "/favicon.svg");
    assert_eq!(status, 200);
    assert!(headers.contains("content-type: image/svg+xml"));
    assert!(favicon.contains("<svg"));

    assert_eq!(http_get(address, "/../Cargo.toml").0, 401);
}

#[test]
fn http_streaming_separates_provisional_invalidation_and_final_core_results() {
    let scratch = Scratch::new();
    let actor = "stream-profile-owner";
    let case = "stream-profile-case";
    let rule = record(&execute_in_case(
        &scratch.0,
        actor,
        case,
        &["--input", "stream-rule", "もしXは鳥ならXは動物"],
    ));
    let premise = record(&execute_in_case(
        &scratch.0,
        actor,
        case,
        &["--input", "stream-premise", "ココは鳥"],
    ));
    let token = "test-only-constant-not-a-secret-0123456789";
    let child = Command::new(env!("CARGO_BIN_EXE_minidora-rust"))
        .args([
            "serve",
            "--db",
            scratch.0.to_str().unwrap(),
            "--actor",
            actor,
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

    let profile = minidora_domain::RequestEnvelope::new(
        case,
        "stream-profile-result",
        actor,
        "ココについて保存されている情報をもとに短い紹介文を書いて",
    );
    let profile_body = serde_json::to_vec(&profile).unwrap();
    assert_eq!(
        http_stream_with_accept(address, token, &profile_body, "*/*").0,
        406
    );
    let (status, headers, body) = http_stream(address, token, &profile_body);
    assert_eq!(status, 200);
    assert!(headers.contains("content-type: text/event-stream; charset=utf-8"));
    assert!(headers.contains("cache-control: no-store"));
    let events = sse_events(&body);
    assert_eq!(
        events
            .iter()
            .map(|(name, _)| name.as_str())
            .collect::<Vec<_>>(),
        ["provisional", "final"]
    );
    assert_eq!(events[0].1["api_version"], "minidora-stream/v1");
    assert_eq!(events[0].1["phase"], "PROVISIONAL");
    assert_eq!(events[0].1["authority"], "NONE");
    assert!(events[0].1["content"].is_null());
    let profile_result = &events[1].1["result"];
    assert_eq!(events[1].1["phase"], "FINAL");
    assert_eq!(profile_result["lifecycle"], "COMMIT");
    assert_eq!(profile_result["guarantee"], "HYPOTHESIS");
    for dependency in [
        rule["dependencies"][0].as_str().unwrap(),
        premise["dependencies"][0].as_str().unwrap(),
    ] {
        assert!(
            profile_result["dependencies"]
                .as_array()
                .unwrap()
                .iter()
                .any(|value| value.as_str() == Some(dependency))
        );
    }

    let missing = minidora_domain::RequestEnvelope::new(
        case,
        "stream-missing-profile",
        actor,
        "タクについて保存されている情報をもとに短い紹介文を書いて",
    );
    let (status, _, body) = http_stream(address, token, &serde_json::to_vec(&missing).unwrap());
    assert_eq!(status, 200);
    let events = sse_events(&body);
    assert_eq!(
        events
            .iter()
            .map(|(name, _)| name.as_str())
            .collect::<Vec<_>>(),
        ["provisional", "invalidated", "final"]
    );
    assert_eq!(events[1].1["phase"], "INVALIDATED");
    assert_eq!(events[1].1["authority"], "NONE");
    assert_eq!(events[1].1["reason"], "CORE_TERMINAL_RESULT_NOT_COMMIT");
    assert_eq!(events[2].1["result"]["lifecycle"], "SUSPEND");
    assert!(events[2].1["result"]["content_plan"].is_null());

    let cancel = minidora_domain::RequestEnvelope::new(case, "stream-cancel", actor, "キャンセル");
    let (status, _, body) = http_stream(address, token, &serde_json::to_vec(&cancel).unwrap());
    assert_eq!(status, 200);
    let events = sse_events(&body);
    assert_eq!(
        events
            .iter()
            .map(|(name, _)| name.as_str())
            .collect::<Vec<_>>(),
        ["provisional", "invalidated", "final"]
    );
    assert_eq!(events[2].1["result"]["lifecycle"], "CANCEL");
}

#[test]
fn authenticated_http_and_cli_share_actual_case_result() {
    let scratch = Scratch::new();
    let output_path = scratch.0.join("http-answer.txt");
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
            "--allow-write",
            output_path.to_str().unwrap(),
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
    let mut request =
        minidora_domain::RequestEnvelope::new("normal-route", "http-input", "owner", "16/4");
    request.output_path = Some(output_path.clone());
    request.permissions.write_paths.insert(output_path.clone());
    let body = serde_json::to_vec(&request).unwrap();
    assert_eq!(http(address, "incorrect", &body).0, 401);
    assert!(!output_path.exists());
    let (status, api_record) = http(address, token, &body);
    assert_eq!(status, 200);
    assert_eq!(api_record["lifecycle"], "COMMIT");
    assert_eq!(api_record["calculation"]["exact_value"], "4");
    let actual = std::fs::read(&output_path).unwrap();
    assert_eq!(
        api_record["render"]["target"],
        serde_json::json!({"kind": "LOCAL_FILE", "path": output_path})
    );
    assert_eq!(api_record["render"]["bytes"], serde_json::json!(actual));
    assert_eq!(
        api_record["receipts"][0]["content_sha256"],
        minidora_domain::digest(&actual)
    );
    let explanation = record(&execute(&scratch.0, "owner", &["詳しく説明して"]));
    assert_eq!(explanation["lifecycle"], "COMMIT");
    assert_eq!(
        explanation["render"]["target"],
        serde_json::json!({"kind": "DISPLAY"})
    );
    assert_eq!(explanation["receipts"], serde_json::json!([]));
    assert!(
        explanation["dependencies"]
            .as_array()
            .unwrap()
            .contains(&api_record["id"])
    );
}

#[test]
fn http_profile_request_flows_through_core_and_cli_summary() {
    let scratch = Scratch::new();
    let actor = "http-profile-owner";
    let case = "http-profile-case";
    let rule = record(&execute_in_case(
        &scratch.0,
        actor,
        case,
        &["--input", "http-profile-rule", "もしXは鳥ならXは動物"],
    ));
    let bird = record(&execute_in_case(
        &scratch.0,
        actor,
        case,
        &["--input", "http-profile-premise", "ココは鳥"],
    ));
    let token = "test-only-constant-not-a-secret-0123456789";
    let child = Command::new(env!("CARGO_BIN_EXE_minidora-rust"))
        .args([
            "serve",
            "--db",
            scratch.0.to_str().unwrap(),
            "--actor",
            actor,
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
    let request = minidora_domain::RequestEnvelope::new(
        case,
        "http-profile-request",
        actor,
        "ココについて保存されている情報をもとに短い紹介文を書いて",
    );
    let body = serde_json::to_vec(&request).unwrap();
    assert_eq!(http(address, "incorrect", &body).0, 401);
    let (status, profile) = http(address, token, &body);
    assert_eq!(status, 200);
    assert_eq!(profile["lifecycle"], "COMMIT");
    assert_eq!(profile["guarantee"], "HYPOTHESIS");
    let text = profile["text"].as_str().unwrap();
    assert!(text.contains("会話で提示された前提は「ココは鳥である」"));
    assert!(text.contains("保存された規則による導出は「ココは動物である」"));
    assert!(text.contains("現実世界の真偽や記録の網羅性は確認していません"));
    for dependency in [
        bird["dependencies"][0].as_str().unwrap(),
        rule["dependencies"][0].as_str().unwrap(),
    ] {
        assert!(
            profile["dependencies"]
                .as_array()
                .unwrap()
                .iter()
                .any(|value| { value.as_str().is_some_and(|text| text == dependency) })
        );
    }

    let model_observation_ref = profile["content_plan"]["model_observation_ref"]
        .as_str()
        .unwrap();
    let store = minidora_engine::store::Store::open(&scratch.0).unwrap();
    let emission: Value =
        serde_json::from_slice(&store.get_bytes(actor, case, model_observation_ref).unwrap())
            .unwrap();
    assert_eq!(emission["text"], profile["text"]);
    let surfaces = emission["authorized_surfaces"].as_array().unwrap();
    assert_eq!(surfaces.len(), 2);
    assert!(surfaces.iter().all(|surface| {
        surface
            .as_str()
            .unwrap()
            .contains("現実世界の真偽や記録の網羅性は確認していません")
    }));
    drop(store);

    let summary = record(&execute_in_case(
        &scratch.0,
        actor,
        case,
        &[
            "--input",
            "http-profile-follow-up-summary",
            "前回の説明を2文以内で要約して",
        ],
    ));
    assert_eq!(summary["lifecycle"], "COMMIT");
    assert_eq!(summary["guarantee"], "HYPOTHESIS");
    assert!(
        summary["dependencies"]
            .as_array()
            .unwrap()
            .contains(&profile["id"])
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
fn condition_definition_from_ordinary_input_changes_the_same_query() {
    let scratch = Scratch::new();
    let actor = "owner";
    let evidence = record(&execute(
        &scratch.0,
        actor,
        &["--input", "calibration", "[scope=ユリ]ユリは校正済み"],
    ));
    let premise = record(&execute(
        &scratch.0,
        actor,
        &[
            "--input",
            "qualified-premise",
            "[scope=ユリ][condition=ユリ]ユリは観測可能",
        ],
    ));
    let question = "なぜ[scope=ユリ][condition=ユリ]ユリは観測可能ですか";
    let before = stopped(
        &execute(
            &scratch.0,
            actor,
            &["--input", "before-definition", question],
        ),
        2,
        "SUSPEND",
    );
    assert!(
        before["reason"]
            .as_str()
            .unwrap()
            .contains("CONDITION_DEFINITION_MISSING")
    );
    assert!(before["render"].is_null());

    let definition = "　条件定義： [scope=ユリ][condition=ユリ] ユリは校正済み。　";
    for invalid in [
        "条件定義: [scope=ユリ][condition=ユリ] ユリは校正済みですか",
        "条件定義: [scope=ユリ][condition=ユリ]「ユリは校正済み」",
        "条件定義: [condition=ユリ][condition=重複] ユリは校正済み",
        "条件定義: [condition=ユリ][time=2026-11-19T08:00:00Z] ユリは校正済み",
    ] {
        stopped(&execute(&scratch.0, actor, &[invalid]), 2, "SUSPEND");
    }
    let store = minidora_engine::store::Store::open(&scratch.0).unwrap();
    assert!(store.load_memory(actor).unwrap().conditions.is_empty());

    let registered = record(&execute(
        &scratch.0,
        actor,
        &["--input", "condition", definition],
    ));
    let id = registered["dependencies"][0].as_str().unwrap();
    assert!(id.starts_with("condition-record:"));
    let saved = inspect_normal_case(&scratch.0, actor);
    let bundle = &saved["bundle"];
    assert_eq!(bundle["request"]["text"], definition);
    assert_eq!(
        bundle["raw_sha256"],
        minidora_domain::digest(definition.as_bytes())
    );
    let meaning = &bundle["meaning"];
    assert_eq!(
        meaning["parser_version"],
        "declarative-ja-en-v92-condition-definition"
    );
    assert_eq!(meaning["intent"]["kind"], "DEFINE_CONDITION");
    assert_eq!(meaning["intent"]["definition"]["condition_id"], "ユリ");
    for (role, prefix) in [
        ("scope", "scope="),
        ("condition_id", "condition="),
        ("subject", "] "),
    ] {
        let spans = meaning["source_roles"][format!("/DefineCondition/definition/{role}")]
            .as_array()
            .unwrap();
        assert_eq!(spans.len(), 1);
        let start = definition.find(prefix).unwrap() + prefix.len();
        assert_eq!(spans[0]["start"], start);
        assert_eq!(spans[0]["end"], start + "ユリ".len());
        assert_eq!(spans[0]["text"], "ユリ");
    }
    let memory = store.load_memory(actor).unwrap();
    assert_eq!(memory.conditions.len(), 1);
    assert_eq!(memory.conditions[0].id, id);
    assert_eq!(memory.conditions[0].source_input, registered["id"]);

    let actual = record(&execute(
        &scratch.0,
        actor,
        &["--input", "with-definition", question],
    ));
    let artifact = assert_why_generation(&scratch.0, actor, "normal-route", &actual);
    for dependency in [
        &evidence["dependencies"][0],
        &premise["dependencies"][0],
        &registered["dependencies"][0],
    ] {
        assert!(
            actual["dependencies"]
                .as_array()
                .unwrap()
                .contains(dependency)
        );
    }
    let proof = &artifact["relation_meaning_plan"];
    assert!(proof["units"].as_array().unwrap().iter().any(|unit| {
        let meaning = &unit["meaning"];
        meaning["kind"] == "CONDITION" && meaning["evaluation"]["status"] == "TRUE"
    }));
    assert_eq!(
        actual["render"]["target"],
        serde_json::json!({"kind": "DISPLAY"})
    );
    assert_eq!(
        actual["render"]["bytes"],
        serde_json::json!(actual["text"].as_str().unwrap().as_bytes())
    );

    record(&execute(
        &scratch.0,
        actor,
        &["[scope=ユリ]訂正: ユリは校正済みではなく未校正です"],
    ));
    let refuted = stopped(
        &execute(
            &scratch.0,
            actor,
            &["--input", "after-correction", question],
        ),
        3,
        "REJECT",
    );
    assert!(
        refuted["reason"]
            .as_str()
            .unwrap()
            .starts_with("CONDITION_FALSE")
    );
    assert!(refuted["content_plan"].is_null());
    assert!(refuted["render"].is_null());
}

#[test]
fn timed_condition_definition_from_ordinary_input_respects_half_open_interval() {
    let scratch = Scratch::new();
    let actor = "owner";
    let start = "2026-11-20T00:00:00Z";
    let end = "2026-11-21T00:00:00Z";
    let before_start = "2026-11-19T23:59:59Z";
    let inside_without_evidence = "2026-11-20T12:00:00Z";
    for time in [before_start, start, end] {
        let evidence = format!("[scope=実験室][time={time}]ユリは校正済み");
        record(&execute(&scratch.0, actor, &[&evidence]));
    }
    for time in [before_start, start, inside_without_evidence, end] {
        let premise = format!("[scope=実験室][time={time}][condition=校正有効]ユリは観測可能");
        record(&execute(&scratch.0, actor, &[&premise]));
    }
    let definition = format!(
        "　条件定義： [valid_until= {end} ][scope=実験室][condition=校正有効][valid_from={start}] ユリは校正済み。　"
    );
    let invalid_definitions = [
        format!("条件定義: [condition=逆転][valid_from={end}][valid_until={start}] ユリは校正済み"),
        format!(
            "条件定義: [condition=重複][valid_from={start}][valid_from={start}] ユリは校正済み"
        ),
        "条件定義: [condition=日付][valid_from=2026-02-29T00:00:00Z] ユリは校正済み".into(),
        format!("条件定義: [condition=時点][time={start}][valid_until={end}] ユリは校正済み"),
        format!("[valid_until={end}] ユリは校正済み"),
    ];
    let store = minidora_engine::store::Store::open(&scratch.0).unwrap();
    let before = store.load_memory(actor).unwrap();
    for invalid in &invalid_definitions {
        let result = stopped(&execute(&scratch.0, actor, &[invalid]), 2, "SUSPEND");
        assert!(result["content_plan"].is_null(), "{result}");
        assert!(result["receipts"].as_array().unwrap().is_empty());
    }
    assert_eq!(store.load_memory(actor).unwrap(), before);

    let registration = ["--input", "timed-condition", &definition];
    let registered = record(&execute(&scratch.0, actor, &registration));
    let bundle = &inspect_normal_case(&scratch.0, actor)["bundle"];
    assert_eq!(bundle["request"]["text"], definition);
    let meaning = &bundle["meaning"];
    assert_eq!(
        meaning["parser_version"],
        "declarative-ja-en-v115-timed-condition-definition"
    );
    for (field, value) in [("valid_from", start), ("valid_until", end)] {
        assert_eq!(meaning["intent"]["definition"][field], value);
        let offset = definition.find(value).unwrap();
        assert_eq!(
            meaning["source_roles"][format!("/DefineCondition/definition/{field}")],
            serde_json::json!([{ "start": offset, "end": offset + value.len(), "text": value }])
        );
    }
    let memory = store.load_memory(actor).unwrap();
    assert_eq!(memory.conditions.len(), 1);
    assert_eq!(registered["dependencies"][0], memory.conditions[0].id);
    assert_eq!(registered["id"], memory.conditions[0].source_input);

    let question = |time: &str| {
        format!("なぜ[scope=実験室][time={time}][condition=校正有効]ユリは観測可能ですか")
    };
    let actual = record(&execute(&scratch.0, actor, &[&question(start)]));
    let artifact = assert_why_generation(&scratch.0, actor, "normal-route", &actual);
    assert!(
        artifact["relation_meaning_plan"]["units"]
            .as_array()
            .unwrap()
            .iter()
            .any(|unit| unit["meaning"]["evaluation"]["status"] == "TRUE")
    );
    assert!(
        actual["dependencies"]
            .as_array()
            .unwrap()
            .contains(&registered["dependencies"][0])
    );
    for (question, code, exit, lifecycle) in [
        (question(before_start), "CONDITION_FALSE", 3, "REJECT"),
        (question(end), "CONDITION_FALSE", 3, "REJECT"),
        (
            question(inside_without_evidence),
            "CONDITION_UNKNOWN",
            2,
            "SUSPEND",
        ),
        (
            "なぜ[scope=実験室][condition=校正有効]ユリは観測可能ですか".into(),
            "CONDITION_UNKNOWN",
            2,
            "SUSPEND",
        ),
    ] {
        let result = stopped(&execute(&scratch.0, actor, &[&question]), exit, lifecycle);
        assert!(
            result["reason"].as_str().unwrap().starts_with(code),
            "{result}"
        );
        assert!(result["render"].is_null());
        assert!(result["receipts"].as_array().unwrap().is_empty());
    }
    assert_eq!(
        record(&execute(&scratch.0, actor, &registration)),
        registered
    );
    assert_eq!(store.load_memory(actor).unwrap(), memory);
}

#[test]
fn condition_evaluation_reports_four_states_without_writing_memory() {
    let scratch = Scratch::new();
    let actor = "owner";
    let command = "　条件評価： [condition=校正][subject=ユリ][scope=実験室]。　";
    let missing = record(&execute(
        &scratch.0,
        actor,
        &["--input", "eval-missing", command],
    ));
    assert_eq!(missing["actor_memory_revision"], 0);
    assert!(
        missing["text"]
            .as_str()
            .unwrap()
            .starts_with("条件 校正: UNKNOWN\n")
    );
    assert!(
        missing["text"]
            .as_str()
            .unwrap()
            .contains("CONDITION_DEFINITION_MISSING")
    );
    let definition = record(&execute(
        &scratch.0,
        actor,
        &["条件定義: [scope=実験室][condition=校正] ユリは校正済み"],
    ));
    let negative = record(&execute(
        &scratch.0,
        actor,
        &["[scope=実験室]ユリは校正済みではない"],
    ));
    let false_result = record(&execute(
        &scratch.0,
        actor,
        &["--input", "eval-false", command],
    ));
    assert!(
        false_result["text"]
            .as_str()
            .unwrap()
            .starts_with("条件 校正: FALSE\n")
    );
    assert!(
        false_result["dependencies"]
            .as_array()
            .unwrap()
            .contains(&negative["dependencies"][0])
    );
    record(&execute(
        &scratch.0,
        actor,
        &["条件定義: [scope=実験室][condition=未校正][subject=ユリ] NOT([ref=校正])"],
    ));
    let true_result = record(&execute(
        &scratch.0,
        actor,
        &["条件評価: [condition=未校正][subject=ユリ][scope=実験室]"],
    ));
    assert!(
        true_result["text"]
            .as_str()
            .unwrap()
            .starts_with("条件 未校正: TRUE\n")
    );
    let positive = record(&execute(
        &scratch.0,
        actor,
        &["[scope=実験室]ユリは校正済み"],
    ));
    let store = minidora_engine::store::Store::open(&scratch.0).unwrap();
    let before = store.load_memory(actor).unwrap();
    let conflict = record(&execute(
        &scratch.0,
        actor,
        &["--input", "eval-conflict", command],
    ));
    assert!(
        conflict["text"]
            .as_str()
            .unwrap()
            .starts_with("条件 校正: CONFLICT\n")
    );
    let proof_ref = conflict["dependencies"].as_array().unwrap().last().unwrap();
    let proof = sequence_artifact(&store, proof_ref);
    assert_eq!(proof[0], "minidora-condition-evaluation-v1");
    assert_eq!(proof[1], before.revision);
    assert_eq!(
        proof[2]["definition_record_ids"],
        serde_json::json!([definition["dependencies"][0]])
    );
    let fact_ids = proof[2]["evidence_fact_ids"].as_array().unwrap();
    assert_eq!(fact_ids.len(), 2);
    assert!(fact_ids.contains(&negative["dependencies"][0]));
    assert!(fact_ids.contains(&positive["dependencies"][0]));
    assert_eq!(
        record(&execute(
            &scratch.0,
            actor,
            &["--input", "eval-false", command]
        )),
        false_result
    );
    assert_eq!(store.load_memory(actor).unwrap(), before);

    let target = scratch.0.join("rejected-evaluation.txt");
    let path = target.to_str().unwrap();
    let denied = stopped(
        &execute(
            &scratch.0,
            actor,
            &[
                "--output",
                path,
                "--allow-write",
                path,
                "--max-actions",
                "4",
                command,
            ],
        ),
        2,
        "SUSPEND",
    );
    assert!(
        denied["reason"]
            .as_str()
            .unwrap()
            .contains("RELATION_BUDGET")
    );
    assert!(!target.exists());
    assert!(store.actions(actor, "normal-route").unwrap().is_empty());
}

#[test]
fn fact_source_resolves_exact_saved_input_across_owned_cases() {
    let scratch = Scratch::new();
    let source = "[scope=実験室]ユリは停止中ではない";
    let asserted = record(&execute(
        &scratch.0,
        "owner",
        &["--case", "observations", "--input", "inspection", source],
    ));
    let id = asserted["dependencies"][0].as_str().unwrap();
    let command = format!("　Factの出典： {id}。　");
    let report = record(&execute(
        &scratch.0,
        "owner",
        &["--input", "provenance", "--max-actions", "5", &command],
    ));
    assert_eq!(report["dependencies"][0], id);
    assert_eq!(report["dependencies"][1], asserted["id"]);
    let text = report["text"].as_str().unwrap();
    assert!(text.contains("記憶状態: 有効"));
    assert!(text.contains("出典ケース: \"observations\"、入力ID: \"inspection\"、版: 1"));
    let store = minidora_engine::store::Store::open(&scratch.0).unwrap();
    let proof = sequence_artifact(&store, &report["dependencies"][2]);
    assert_eq!(proof[0], "minidora-fact-source-v1");
    assert_eq!(proof[1], report["actor_memory_revision"]);
    assert_eq!(proof[2]["proposition"]["negative"], true);
    assert_eq!(
        proof[2]["source_evidence"]["raw_sha256"],
        minidora_domain::digest(source.as_bytes())
    );
    let roles = proof[2]["source_evidence"]["source_roles"]
        .as_object()
        .unwrap();
    assert!(roles.contains_key("/Assert/proposition/negation"));
    for spans in roles.values() {
        for span in spans.as_array().unwrap() {
            let start = span["start"].as_u64().unwrap() as usize;
            let end = span["end"].as_u64().unwrap() as usize;
            assert_eq!(source.get(start..end), span["text"].as_str());
        }
    }
    let foreign = stopped(
        &execute(&scratch.0, "other", &["--case", "foreign", &command]),
        2,
        "SUSPEND",
    );
    assert!(
        foreign["reason"]
            .as_str()
            .unwrap()
            .contains("FACT_SOURCE_NOT_FOUND")
    );
    let missing_command = format!("Factの出典: fact:{}", "0".repeat(64));
    let missing = stopped(
        &execute(
            &scratch.0,
            "other",
            &["--case", "foreign", &missing_command],
        ),
        2,
        "SUSPEND",
    );
    assert_eq!(foreign["reason"], missing["reason"]);
    assert_eq!(foreign["text"], missing["text"]);
    assert!(foreign["content_plan"].is_null());
    assert!(foreign["render"].is_null());
    assert!(store.actions("owner", "normal-route").unwrap().is_empty());
}

#[test]
fn condition_source_resolves_exact_timed_definition_across_owned_cases() {
    let scratch = Scratch::new();
    let source = concat!(
        "条件定義: [scope=実験室][condition=点検]",
        "[valid_from=2026-11-20T00:00:00Z][valid_until=2026-11-21T00:00:00Z] ユリは点検済み"
    );
    let defined = record(&execute(
        &scratch.0,
        "owner",
        &["--case", "definitions", "--input", "inspection", source],
    ));
    let id = defined["dependencies"][0].as_str().unwrap();
    assert!(id.starts_with("condition-record:"));
    let command = format!("　条件定義の出典： {id}。　");
    let report = record(&execute(
        &scratch.0,
        "owner",
        &[
            "--input",
            "condition-provenance",
            "--max-actions",
            "5",
            &command,
        ],
    ));
    assert_eq!(report["dependencies"][0], id);
    assert_eq!(report["dependencies"][1], defined["id"]);
    let text = report["text"].as_str().unwrap();
    assert!(text.contains("記憶状態: 有効"));
    assert!(text.contains("出典ケース: \"definitions\"、入力ID: \"inspection\"、版: 1"));
    assert!(text.contains(
        "期間の下限（含む）: 2026-11-20T00:00:00Z、上限（含まない）: 2026-11-21T00:00:00Z"
    ));
    let store = minidora_engine::store::Store::open(&scratch.0).unwrap();
    let proof = sequence_artifact(&store, &report["dependencies"][2]);
    assert_eq!(proof[0], "minidora-condition-source-v1");
    assert_eq!(proof[1], report["actor_memory_revision"]);
    assert_eq!(proof[2]["id"], id);
    assert_eq!(proof[2]["source_input"], defined["id"]);
    assert_eq!(proof[7], minidora_domain::digest(source.as_bytes()));
    let roles = proof[8].as_object().unwrap();
    assert!(!roles.is_empty());
    for spans in roles.values() {
        for span in spans.as_array().unwrap() {
            let start = span["start"].as_u64().unwrap() as usize;
            let end = span["end"].as_u64().unwrap() as usize;
            assert_eq!(source.get(start..end), span["text"].as_str());
        }
    }
    let foreign = stopped(
        &execute(&scratch.0, "other", &["--case", "foreign", &command]),
        2,
        "SUSPEND",
    );
    let missing_command = format!("条件定義の出典: condition-record:{}", "0".repeat(64));
    let missing = stopped(
        &execute(
            &scratch.0,
            "other",
            &["--case", "foreign", &missing_command],
        ),
        2,
        "SUSPEND",
    );
    assert!(
        foreign["reason"]
            .as_str()
            .unwrap()
            .contains("CONDITION_SOURCE_NOT_FOUND")
    );
    assert_eq!(foreign["reason"], missing["reason"]);
    assert_eq!(foreign["text"], missing["text"]);
    assert!(foreign["content_plan"].is_null());
    assert!(foreign["render"].is_null());
    assert!(store.actions("owner", "normal-route").unwrap().is_empty());
}

#[test]
fn condition_source_by_definition_resolves_withdrawn_record_from_exact_qualifiers() {
    let scratch = Scratch::new();
    let source = "条件定義: [scope=実験室][condition=点検] ユリは点検済み";
    let defined = record(&execute(
        &scratch.0,
        "owner",
        &["--case", "archive", source],
    ));
    let id = defined["dependencies"][0].as_str().unwrap();
    record(&execute(
        &scratch.0,
        "owner",
        &["--case", "withdraw", &format!("{id}を無効化して")],
    ));
    let evaluation = record(&execute(
        &scratch.0,
        "owner",
        &["条件評価: [condition=点検][subject=ユリ][scope=実験室]"],
    ));
    assert!(
        evaluation["text"]
            .as_str()
            .unwrap()
            .starts_with("条件 点検: UNKNOWN")
    );
    assert!(!evaluation["text"].as_str().unwrap().contains(id));

    let store = minidora_engine::store::Store::open(&scratch.0).unwrap();
    let revision = store.load_memory("owner").unwrap().revision;
    let command = "　条件定義の出典： [scope= 実験室 ][condition=点検] [subject=ユリ]。　";
    let named = record(&execute(
        &scratch.0,
        "owner",
        &["--case", "named-source", "--max-actions", "5", command],
    ));
    let by_record = record(&execute(
        &scratch.0,
        "owner",
        &[&format!("条件定義の出典: {id}")],
    ));
    assert_eq!(named["text"], by_record["text"]);
    assert!(named["text"].as_str().unwrap().contains("記憶状態: 失効"));
    assert_eq!(named["dependencies"], by_record["dependencies"]);
    assert_eq!(named["dependencies"][0], id);
    assert_eq!(named["dependencies"][1], defined["id"]);
    assert_eq!(named["actor_memory_revision"], revision);
    let sealed = store
        .load_case("owner", "named-source")
        .unwrap()
        .unwrap()
        .bundle
        .unwrap();
    assert_eq!(
        sealed.meaning.parser_version,
        "declarative-ja-en-v131-condition-source-by-definition"
    );
    assert_eq!(sealed.meaning.source_roles.len(), 3);
    for (role, expected) in [
        ("condition_id", "点検"),
        ("subject", "ユリ"),
        ("scope", "実験室"),
    ] {
        let spans =
            &sealed.meaning.source_roles[&format!("/InspectConditionSourceByDefinition/{role}")];
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].text, expected);
        assert_eq!(command.get(spans[0].start..spans[0].end), Some(expected));
    }

    let forbidden = scratch.0.join("denied-provenance.txt");
    let path = forbidden.to_str().unwrap();
    let same_public_stop = |actor, case, raw| {
        stopped(
            &execute(
                &scratch.0,
                actor,
                &["--case", case, "--output", path, "--allow-write", path, raw],
            ),
            2,
            "SUSPEND",
        )
    };
    let missing = same_public_stop(
        "owner",
        "denied",
        "条件定義の出典: [condition=未登録][subject=ユリ][scope=実験室]",
    );
    assert!(
        missing["reason"]
            .as_str()
            .unwrap()
            .contains("CONDITION_SOURCE_NOT_FOUND")
    );
    for denied in [
        same_public_stop("visitor", "visitor-denied", command),
        same_public_stop(
            "owner",
            "denied",
            "条件定義の出典: [condition=点検][subject=葵][scope=実験室]",
        ),
        same_public_stop(
            "owner",
            "denied",
            "条件定義の出典: [condition=点検][subject=ユリ][scope=別室]",
        ),
    ] {
        assert_eq!(denied["reason"], missing["reason"]);
        assert_eq!(denied["text"], missing["text"]);
        assert!(denied["render"].is_null());
    }
    assert!(!forbidden.exists());
    assert!(store.actions("owner", "denied").unwrap().is_empty());
    assert!(
        store
            .actions("visitor", "visitor-denied")
            .unwrap()
            .is_empty()
    );
    assert_eq!(store.load_memory("owner").unwrap().revision, revision);
}

#[test]
fn rule_source_resolves_the_exact_guarded_declaration_across_owned_cases() {
    let scratch = Scratch::new();
    let source = concat!(
        "　もし[scope=実験室][condition=点検][condition=校正]Xは研究員 かつ ",
        "[scope=実験室][condition=校正][condition=点検]Xは当番なら",
        "[scope=実験室][condition=点検][condition=校正]Xは入室可。　"
    );
    let declared = record(&execute(
        &scratch.0,
        "owner",
        &["--case", "declarations", "--input", "access-rule", source],
    ));
    let rule_id = declared["dependencies"][0].as_str().unwrap();
    let command = format!("　規則の出典： {rule_id}。　");
    let report = record(&execute(
        &scratch.0,
        "owner",
        &["--input", "rule-provenance", "--max-actions", "5", &command],
    ));
    assert_eq!(report["guarantee"], "COMPUTED");
    assert_eq!(report["dependencies"][0], rule_id);
    assert_eq!(report["dependencies"][1], declared["id"]);
    let report_text = report["text"].as_str().unwrap();
    assert!(report_text.contains("前提 1: $xは研究員である"));
    assert!(report_text.contains("前提 2: $xは当番である"));
    assert!(report_text.contains("出典ケース: \"declarations\"、入力ID: \"access-rule\"、版: 1"));

    let store = minidora_engine::store::Store::open(&scratch.0).unwrap();
    let proof = sequence_artifact(&store, &report["dependencies"][2]);
    assert_eq!(proof[0], "minidora-rule-source-v1");
    assert_eq!(proof[1], report["actor_memory_revision"]);
    assert_eq!(proof[2]["id"], rule_id);
    assert_eq!(proof[2]["source_input"], declared["id"]);
    assert_eq!(proof[7], minidora_domain::digest(source.as_bytes()));
    let roles = proof[8].as_object().unwrap();
    assert!(roles.contains_key("/AddConjunctiveRule/premises/1/condition_refs/1"));
    assert_eq!(
        roles["/AddConjunctiveRule/premises/0/subject"][0]["text"],
        "X"
    );
    for spans in roles.values() {
        for span in spans.as_array().unwrap() {
            let start = span["start"].as_u64().unwrap() as usize;
            let end = span["end"].as_u64().unwrap() as usize;
            assert_eq!(source.get(start..end), span["text"].as_str());
        }
    }
    let saved = store.load_case("owner", "normal-route").unwrap().unwrap();
    let bundle = saved.bundle.unwrap();
    assert_eq!(
        bundle.meaning.parser_version,
        "declarative-ja-en-v138-declared-rule-source"
    );
    assert_eq!(bundle.meaning.source_roles.len(), 1);
    let request_span = &bundle.meaning.source_roles["/InspectDeclaredRuleSource/rule_id"][0];
    assert_eq!(
        command.get(request_span.start..request_span.end),
        Some(rule_id)
    );
    assert!(store.actions("owner", "normal-route").unwrap().is_empty());
}

#[test]
fn plain_rule_source_follows_a_derived_answer_to_the_exact_prior_input_version() {
    let scratch = Scratch::new();
    let run = |args: &[&str]| record(&execute(&scratch.0, "owner", args));
    let declaration = concat!(
        "　もし[scope=実験室]Xは研究員 かつ ",
        "[scope=実験室]Xは当番なら、[scope=実験室]Xは入室可。　"
    );
    let registered = run(&[
        "--case",
        "declarations",
        "--input",
        "access",
        "--version",
        "2",
        declaration,
    ]);
    run(&[
        "--case",
        "declarations",
        "--input",
        "access",
        "--version",
        "3",
        "もし[scope=別室]Xは研究員なら[scope=別室]Xは記録済み",
    ]);
    run(&["[scope=実験室]ユリは研究員"]);
    run(&["[scope=実験室]ユリは当番"]);
    let why = run(&["なぜ[scope=実験室]ユリは入室可ですか"]);
    assert_eq!(why["guarantee"], "DERIVED");
    let rule_id = registered["dependencies"][0].as_str().unwrap();
    assert!(
        why["dependencies"]
            .as_array()
            .unwrap()
            .contains(&registered["dependencies"][0])
    );

    let command = format!("規則の出典: {rule_id}");
    let report = run(&["--max-actions", "5", &command]);
    assert_eq!(report["guarantee"], "COMPUTED");
    assert_eq!(report["dependencies"][0], rule_id);
    assert_eq!(report["dependencies"][1], registered["id"]);
    let text = report["text"].as_str().unwrap();
    assert!(text.contains("出典ケース: \"declarations\"、入力ID: \"access\"、版: 2"));
    assert!(text.contains(&format!("原文: {declaration:?}")));
    let store = minidora_engine::store::Store::open(&scratch.0).unwrap();
    let proof = sequence_artifact(&store, &report["dependencies"][2]);
    assert_eq!(proof[0], "minidora-plain-rule-source-v1");
    assert_eq!(proof[2]["additional_premises"][0]["object"], "当番");
    assert_eq!(proof[6].as_str().unwrap().len(), 64);
    assert_eq!(proof[7], minidora_domain::digest(declaration.as_bytes()));
    assert_eq!(proof[8], "declarative-ja-en-v89-sealed-relation-boundary");
    assert!(store.actions("owner", "normal-route").unwrap().is_empty());
}

#[test]
fn previous_rule_source_uses_the_bound_why_instead_of_searching_turns() {
    let scratch = Scratch::new();
    let run = |args: &[&str]| record(&execute(&scratch.0, "owner", args));
    let declaration = "　もし[scope=実験室]Xは研究員なら[scope=実験室]Xは入室可。　";
    let registered = run(&["--case", "declarations", declaration]);
    run(&["--case", "evidence", "[scope=実験室]ユリは研究員"]);
    let why = run(&["--input", "why", "なぜ[scope=実験室]ユリは入室可ですか"]);
    assert_eq!(why["guarantee"], "DERIVED", "{why:?}");
    let why_id = why["id"].as_str().unwrap();
    let phrase = "　前の回答で使った規則の出典を教えて。　";
    let report = run(&["--input", "implicit", "--max-actions", "6", phrase]);
    assert_eq!(report["dependencies"][0], why_id);
    assert_eq!(
        report["dependencies"][1],
        why["content_plan"]["dependencies"]
            .as_array()
            .unwrap()
            .last()
            .unwrap()
            .clone()
    );
    assert_eq!(report["dependencies"][2], registered["dependencies"][0]);
    assert_eq!(report["dependencies"][3], registered["id"]);
    let text = report["text"].as_str().unwrap();
    assert!(text.starts_with(&format!("参照した理由説明ID: {why_id}\n")));
    assert!(text.contains(&format!("原文: {declaration:?}")));
    let store = minidora_engine::store::Store::open(&scratch.0).unwrap();
    let proof = sequence_artifact(&store, &report["dependencies"][4]);
    assert_eq!(proof[0], "minidora-previous-rule-source-v1");
    assert_eq!(proof[1], why_id);
    assert_eq!(proof[4], report["dependencies"][1]);
    assert_eq!(proof[5][0], "minidora-plain-rule-source-v1");

    run(&["--input", "other-answer", "2+3"]);
    let stopped = stopped(
        &execute(&scratch.0, "owner", &["--input", "still-implicit", phrase]),
        2,
        "SUSPEND",
    );
    assert!(
        stopped["reason"]
            .as_str()
            .unwrap()
            .contains("PREVIOUS_RULE_SOURCE_UNAVAILABLE")
    );
    assert!(stopped["render"].is_null());
    let explicit = run(&["--input", "explicit", "--context", why_id, phrase]);
    assert_eq!(explicit["text"], report["text"]);
    assert_eq!(explicit["dependencies"], report["dependencies"]);
    assert!(store.actions("owner", "normal-route").unwrap().is_empty());
}

#[test]
fn composed_rule_source_follows_the_actual_explanation_to_both_guarded_declarations() {
    let scratch = Scratch::new();
    let run = |args: &[&str]| record(&execute(&scratch.0, "owner", args));
    let left = "　もし[scope=実験室][condition=点検]Xは研究員なら[scope=実験室][condition=点検]Xは当番。　";
    let right =
        "もし[scope=実験室][condition=点検]Yは当番なら[scope=実験室][condition=点検]Yは入室可";
    let declared_left = run(&["--case", "left", "--input", "rule-left", left]);
    let declared_right = run(&["--case", "right", "--input", "rule-right", right]);
    run(&["条件定義: [scope=実験室][condition=点検] ユリは点検済み"]);
    run(&["[scope=実験室]ユリは点検済み"]);
    run(&["[scope=実験室][condition=点検]ユリは研究員"]);
    let why = run(&["なぜ[scope=実験室][condition=点検]ユリは入室可ですか"]);
    assert_eq!(why["guarantee"], "DERIVED", "{why:?}");

    let store = minidora_engine::store::Store::open(&scratch.0).unwrap();
    let memory = store.load_memory("owner").unwrap();
    let parents = [
        declared_left["dependencies"][0].as_str().unwrap(),
        declared_right["dependencies"][0].as_str().unwrap(),
    ];
    let shortcut = memory
        .rules
        .iter()
        .find(|rule| rule.dependencies == parents)
        .expect("saved composed rule with the exact ordered parents");
    assert!(
        why["dependencies"]
            .as_array()
            .unwrap()
            .contains(&serde_json::json!(shortcut.id))
    );
    let command = format!("　合成規則の出典： {}。　", shortcut.id);
    let report = run(&["--case", "source", "--max-actions", "6", &command]);
    assert_eq!(report["guarantee"], "COMPUTED");
    let deps = report["dependencies"].as_array().unwrap();
    assert_eq!(
        deps[..5],
        [
            serde_json::json!(shortcut.id),
            serde_json::json!(parents[0]),
            serde_json::json!(parents[1]),
            declared_left["id"].clone(),
            declared_right["id"].clone(),
        ]
    );
    let proof: Value = serde_json::from_slice(
        &store
            .get_bytes("owner", "source", deps[5].as_str().unwrap())
            .unwrap(),
    )
    .unwrap();
    assert_eq!(proof[0], "minidora-composed-rule-source-v1");
    assert_eq!(proof[1], report["actor_memory_revision"]);
    assert_eq!(proof[2]["id"], shortcut.id);
    for (index, original, case) in [(0, left, "left"), (1, right, "right")] {
        assert_eq!(proof[3][index][0]["id"], parents[index]);
        assert_eq!(proof[3][index][1], case);
        assert_eq!(
            proof[3][index][5],
            minidora_domain::digest(original.as_bytes())
        );
        assert!(
            report["text"]
                .as_str()
                .unwrap()
                .contains(&format!("原文: {original:?}"))
        );
    }
    let saved = store.load_case("owner", "source").unwrap().unwrap();
    let bundle = saved.bundle.unwrap();
    assert_eq!(
        bundle.meaning.parser_version,
        "declarative-ja-en-v137-composed-rule-source"
    );
    assert_eq!(bundle.meaning.source_roles.len(), 1);
    let span = &bundle.meaning.source_roles["/InspectComposedRuleSource/rule_id"][0];
    assert_eq!(
        command.get(span.start..span.end),
        Some(shortcut.id.as_str())
    );
    let why_id = why["id"].as_str().unwrap();
    let contextual = run(&[
        "--input",
        "previous-composed-source",
        "--max-actions",
        "7",
        "　前の回答で使った合成規則の出典を教えて。　",
    ]);
    assert_eq!(
        contextual["text"].as_str().unwrap(),
        format!(
            "参照した理由説明ID: {why_id}\n{}",
            report["text"].as_str().unwrap()
        )
    );
    let contextual_deps = contextual["dependencies"].as_array().unwrap();
    assert_eq!(contextual_deps[0], why_id);
    assert_eq!(contextual_deps[2..7], deps[..5]);
    let contextual_proof = sequence_artifact(&store, &contextual_deps[7]);
    assert_eq!(
        contextual_proof[0],
        "minidora-previous-composed-rule-source-v1"
    );
    assert_eq!(contextual_proof[1], why_id);
    assert_eq!(contextual_proof[4], contextual_deps[1]);
    assert_eq!(contextual_proof[5], proof);
    let ambiguous = stopped(
        &execute(
            &scratch.0,
            "owner",
            &["--context", why_id, "前の回答で使った規則の出典"],
        ),
        2,
        "SUSPEND",
    );
    assert!(
        ambiguous["reason"]
            .as_str()
            .unwrap()
            .contains("PREVIOUS_RULE_SOURCE_AMBIGUOUS")
    );
    let listed = run(&[
        "--input",
        "previous-rule-list",
        "--context",
        why_id,
        "--max-actions",
        "4",
        "前の回答で使った規則の一覧",
    ]);
    assert!(listed["actor_memory_revision"].is_null());
    let listing = listed["text"].as_str().unwrap();
    let reference = listing
        .lines()
        .next()
        .unwrap()
        .strip_prefix("参照した理由説明ID: ")
        .unwrap();
    assert_eq!(reference, why_id);
    assert!(listing.contains("使用規則: 3件"));
    assert!(listing.contains(&format!(
        "合成規則: {}（親規則: {}、{}）",
        shortcut.id, parents[0], parents[1]
    )));
    let list_proof = sequence_artifact(&store, &listed["dependencies"][2]);
    assert_eq!(list_proof[0], "minidora-previous-rule-list-v1");
    assert_eq!(list_proof[1], why_id);
    let rows = list_proof[5].as_array().unwrap();
    assert_eq!(rows.len(), 3);
    let composed_row = rows.iter().find(|row| row[2] == true).unwrap();
    let listed_id = composed_row[0].as_str().unwrap();
    assert_eq!(listed_id, shortcut.id);
    assert_eq!(composed_row[1], serde_json::json!(parents));

    let list_id = listed["id"].as_str().unwrap();
    let from_list_command = format!("　一覧の規則の出典： {listed_id}。　");
    let from_list = run(&[
        "--input",
        "source-from-list",
        "--max-actions",
        "8",
        &from_list_command,
    ]);
    assert_eq!(
        from_list["text"].as_str().unwrap(),
        format!(
            "参照した規則一覧ID: {list_id}\n{}",
            contextual["text"].as_str().unwrap()
        )
    );
    let list_source_dependencies = from_list["dependencies"].as_array().unwrap();
    assert_eq!(list_source_dependencies[0], list_id);
    assert_eq!(list_source_dependencies[1], listed["dependencies"][2]);
    assert_eq!(list_source_dependencies[2..9], contextual_deps[..7]);
    let from_list_proof = sequence_artifact(&store, &list_source_dependencies[9]);
    assert_eq!(from_list_proof[0], "minidora-listed-rule-source-v1");
    assert_eq!(from_list_proof[1][0], list_id);
    assert_eq!(from_list_proof[1][3], listed["dependencies"][2]);
    assert_eq!(from_list_proof[2][0], why_id);
    assert_eq!(from_list_proof[3], proof);
    let from_list_bundle = store
        .load_case("owner", "normal-route")
        .unwrap()
        .unwrap()
        .bundle
        .unwrap();
    assert_eq!(
        from_list_bundle.meaning.parser_version,
        "declarative-ja-en-v143-listed-rule-source"
    );
    let spans = &from_list_bundle.meaning.source_roles["/AskListedRuleSource/rule_id"];
    assert_eq!(spans.len(), 1);
    assert_eq!(
        from_list_command.get(spans[0].start..spans[0].end),
        Some(listed_id)
    );

    let selected_command = format!("　前の回答で使った規則の出典： {listed_id}。　");
    let selected = run(&[
        "--input",
        "select-shortcut",
        "--context",
        reference,
        "--max-actions",
        "7",
        &selected_command,
    ]);
    assert_eq!(selected["text"], contextual["text"]);
    let selected_deps = selected["dependencies"].as_array().unwrap();
    assert_eq!(selected_deps[..7], contextual_deps[..7]);
    let selected_proof = sequence_artifact(&store, &selected_deps[7]);
    assert_eq!(
        selected_proof[0],
        "minidora-selected-previous-rule-source-v1"
    );
    assert_eq!(selected_proof[1], why_id);
    assert_eq!(selected_proof[5], proof);
    let selected_bundle = store
        .load_case("owner", "normal-route")
        .unwrap()
        .unwrap()
        .bundle
        .unwrap();
    assert_eq!(
        selected_bundle.meaning.parser_version,
        "declarative-ja-en-v141-previous-rule-source-by-id"
    );
    let spans = &selected_bundle.meaning.source_roles["/InspectPreviousRuleSourceById/rule_id"];
    assert_eq!(spans.len(), 1);
    assert_eq!(
        selected_command.get(spans[0].start..spans[0].end),
        Some(shortcut.id.as_str())
    );
    let selected_parent = run(&[
        "--input",
        "select-parent",
        "--context",
        why_id,
        "--max-actions",
        "6",
        &format!("前の回答で使った規則の出典: {}", parents[0]),
    ]);
    let parent_deps = selected_parent["dependencies"].as_array().unwrap();
    assert_eq!(parent_deps[0], why_id);
    assert_eq!(parent_deps[2], parents[0]);
    assert_eq!(parent_deps[3], declared_left["id"]);
    let parent_proof = sequence_artifact(&store, &parent_deps[4]);
    assert_eq!(parent_proof[0], "minidora-selected-previous-rule-source-v1");
    assert_eq!(parent_proof[5][0], "minidora-rule-source-v1");
    let from_older_list = run(&[
        "--input",
        "source-from-older-list",
        "--context",
        list_id,
        "--max-actions",
        "7",
        &format!("一覧の規則の出典: {}", parents[0]),
    ]);
    assert_eq!(
        from_older_list["text"].as_str().unwrap(),
        format!(
            "参照した規則一覧ID: {list_id}\n{}",
            selected_parent["text"].as_str().unwrap()
        )
    );
    assert_eq!(store.load_memory("owner").unwrap(), memory);
    assert!(store.actions("owner", "source").unwrap().is_empty());
}

#[test]
fn previous_rule_source_list_preserves_zero_rules_for_a_direct_why() {
    let scratch = Scratch::new();
    let run = |args: &[&str]| record(&execute(&scratch.0, "owner", args));
    run(&["[scope=実験室]ユリは当番"]);
    let why = run(&["なぜ[scope=実験室]ユリは当番ですか"]);
    assert_eq!(why["guarantee"], "USER_PREMISE");
    let listed = run(&[
        "--max-actions",
        "4",
        "　前の回答で使った規則の一覧を教えて。　",
    ]);
    assert_eq!(
        listed["text"],
        format!(
            "参照した理由説明ID: {}\n使用規則: 0件",
            why["id"].as_str().unwrap()
        )
    );
    assert!(listed["actor_memory_revision"].is_null());
    let store = minidora_engine::store::Store::open(&scratch.0).unwrap();
    let proof = sequence_artifact(&store, &listed["dependencies"][2]);
    assert_eq!(proof[1], why["id"]);
    assert!(proof[5].as_array().unwrap().is_empty());
    assert!(store.actions("owner", "normal-route").unwrap().is_empty());
}

#[test]
fn condition_rule_registration_derives_only_while_its_guard_is_supported() {
    let scratch = Scratch::new();
    let run = |args: &[&str]| record(&execute(&scratch.0, "owner", args));
    let declaration = "　もし [scope=実験室][condition=点検]Xは研究員ならば、[condition=点検][scope=実験室]Xは入室可。　";
    let file = scratch.0.join("ungranted.txt");
    let denied = stopped(
        &execute(
            &scratch.0,
            "owner",
            &[
                "--case",
                "unauthorized-rule",
                "--output",
                file.to_str().unwrap(),
                declaration,
            ],
        ),
        2,
        "SUSPEND",
    );
    assert!(
        denied["reason"]
            .as_str()
            .unwrap()
            .contains("WRITE_PERMISSION_MISSING")
    );
    assert!(!file.exists());
    let store = minidora_engine::store::Store::open(&scratch.0).unwrap();
    assert!(store.load_memory("owner").unwrap().rules.is_empty());

    let definition = run(&["条件定義: [scope=実験室][condition=点検] ユリは点検済み"]);
    let premise = run(&["[scope=実験室][condition=点検]ユリは研究員"]);
    let rule = run(&["--case", "guard-rule", "--max-actions", "3", declaration]);
    assert_eq!(rule["guarantee"], "USER_PREMISE");
    let saved = store.load_case("owner", "guard-rule").unwrap().unwrap();
    let bundle = saved.bundle.unwrap();
    assert_eq!(
        bundle.meaning.parser_version,
        "declarative-ja-en-v132-guarded-relation-rule"
    );
    let spans = &bundle.meaning.source_roles;
    for role in ["premise", "conclusion"] {
        for (field, value) in [
            ("subject", "X"),
            ("scope", "実験室"),
            ("condition_refs/0", "点検"),
        ] {
            let source = &spans[&format!("/AddRule/{role}/{field}")][0];
            assert_eq!(declaration.get(source.start..source.end), Some(value));
        }
    }
    let query = "[scope=実験室][condition=点検]ユリは入室可ですか";
    let suspended =
        |command: &str| stopped(&execute(&scratch.0, "owner", &[command]), 2, "SUSPEND");
    assert!(
        suspended(query)["reason"]
            .as_str()
            .unwrap()
            .contains("CONDITION_UNKNOWN")
    );
    let evidence = run(&["[scope=実験室]ユリは点検済み"]);
    let derived = run(&[query]);
    assert_eq!(derived["guarantee"], "DERIVED");
    for source in [definition, premise, rule, evidence] {
        assert!(
            derived["dependencies"]
                .as_array()
                .unwrap()
                .contains(&source["dependencies"][0])
        );
    }
    assert!(
        suspended("[scope=実験室]ユリは入室可ですか")["reason"]
            .as_str()
            .unwrap()
            .contains("MISSING_PREMISES")
    );
    let opposite = run(&["[scope=実験室]ユリは点検済みではない"]);
    assert!(
        suspended(query)["reason"]
            .as_str()
            .unwrap()
            .contains("CONDITION_CONFLICT")
    );
    run(&[&format!(
        "{}を無効化して",
        opposite["dependencies"][0].as_str().unwrap()
    )]);
    assert_eq!(run(&[query])["dependencies"], derived["dependencies"]);
    let condition_record = store.load_memory("owner").unwrap().conditions[0].id.clone();
    run(&[&format!("{condition_record}を無効化して")]);
    let stopped_after = suspended(query);
    assert!(
        stopped_after["reason"]
            .as_str()
            .unwrap()
            .contains("CONDITION_UNKNOWN")
    );
    assert!(stopped_after["render"].is_null());
}

#[test]
fn condition_rule_requires_both_guarded_premises_for_an_ordinary_derivation() {
    let scratch = Scratch::new();
    let run = |args: &[&str]| record(&execute(&scratch.0, "owner", args));
    let declaration = concat!(
        "　もし [scope=実験室][condition=点検]Xは研究員 且つ ",
        "[condition=点検][scope=実験室]Xは当番ならば、",
        "[scope=実験室][condition=点検]Xは入室可。　"
    );
    let rule = run(&[
        "--case",
        "two-premise-rule",
        "--max-actions",
        "3",
        declaration,
    ]);
    let store = minidora_engine::store::Store::open(&scratch.0).unwrap();
    let case = store
        .load_case("owner", "two-premise-rule")
        .unwrap()
        .unwrap();
    let bundle = case.bundle.unwrap();
    assert_eq!(
        bundle.meaning.parser_version,
        "declarative-ja-en-v133-guarded-conjunctive-rule"
    );
    for (role, value) in [
        ("premises/0/object", "研究員"),
        ("premises/1/object", "当番"),
        ("conclusion/object", "入室可"),
        ("premises/0/condition_refs/0", "点検"),
        ("premises/1/condition_refs/0", "点検"),
        ("conclusion/condition_refs/0", "点検"),
    ] {
        let source = &bundle.meaning.source_roles[&format!("/AddConjunctiveRule/{role}")][0];
        assert_eq!(declaration.get(source.start..source.end), Some(value));
    }
    let memory = store.load_memory("owner").unwrap();
    assert_eq!(memory.rules.len(), 1);
    assert_eq!(memory.rules[0].premise.object, "研究員");
    assert_eq!(memory.rules[0].additional_premises[0].object, "当番");

    let definition = run(&["条件定義: [scope=実験室][condition=点検] ユリは点検済み"]);
    let evidence = run(&["[scope=実験室]ユリは点検済み"]);
    let first = run(&["[scope=実験室][condition=点検]ユリは研究員"]);
    let query = "[scope=実験室][condition=点検]ユリは入室可ですか";
    let blocked = || stopped(&execute(&scratch.0, "owner", &[query]), 2, "SUSPEND");
    assert!(
        blocked()["reason"]
            .as_str()
            .unwrap()
            .contains("MISSING_PREMISES")
    );
    let second = run(&["[scope=実験室][condition=点検]ユリは当番"]);
    let accepted = run(&[query]);
    assert_eq!(accepted["guarantee"], "DERIVED");
    for source in [rule, definition, evidence, first, second] {
        assert!(
            accepted["dependencies"]
                .as_array()
                .unwrap()
                .contains(&source["dependencies"][0])
        );
    }
    let opposing = run(&["[scope=実験室][condition=点検]ユリは当番ではない"]);
    assert!(blocked()["render"].is_null());
    run(&[&format!(
        "{}を無効化して",
        opposing["dependencies"][0].as_str().unwrap()
    )]);
    assert_eq!(run(&[query])["dependencies"], accepted["dependencies"]);
}

#[test]
fn condition_rules_use_the_same_multiple_conditions_in_every_clause() {
    let scratch = Scratch::new();
    let run = |args: &[&str]| record(&execute(&scratch.0, "owner", args));
    let guard = "[scope=実験室][condition=校正][condition=点検]";
    let reverse = "[condition=点検][scope=実験室][condition=校正]";
    let one = format!("もし{guard}Xは研究員なら{reverse}Xは案内係");
    let two = format!("もし {reverse}Xは研究員 かつ {guard}Xは当番ならば、{reverse}Xは入室可。");
    let rule_one = run(&["--case", "single-multi", "--max-actions", "3", &one]);
    let rule_two = run(&["--case", "pair-multi", "--max-actions", "3", &two]);
    let store = minidora_engine::store::Store::open(&scratch.0).unwrap();
    for (case, raw, roles) in [
        (
            "single-multi",
            &one,
            &["/AddRule/premise", "/AddRule/conclusion"][..],
        ),
        (
            "pair-multi",
            &two,
            &[
                "/AddConjunctiveRule/premises/0",
                "/AddConjunctiveRule/premises/1",
                "/AddConjunctiveRule/conclusion",
            ][..],
        ),
    ] {
        let bundle = store
            .load_case("owner", case)
            .unwrap()
            .unwrap()
            .bundle
            .unwrap();
        assert_eq!(
            bundle.meaning.parser_version,
            "declarative-ja-en-v135-multi-condition-rule"
        );
        for root in roles {
            for (index, value) in ["校正", "点検"].into_iter().enumerate() {
                let role = format!("{root}/condition_refs/{index}");
                let spans = &bundle.meaning.source_roles[&role];
                assert_eq!(spans.len(), 1);
                assert_eq!(raw.get(spans[0].start..spans[0].end), Some(value));
            }
        }
    }
    let revision = store.load_memory("owner").unwrap().revision;
    let different = two.replace(
        &format!("{guard}Xは当番"),
        "[scope=実験室][condition=点検][condition=別条件]Xは当番",
    );
    let denied = stopped(&execute(&scratch.0, "owner", &[&different]), 2, "SUSPEND");
    assert!(
        denied["reason"]
            .as_str()
            .unwrap()
            .contains("CAPABILITY_UNAVAILABLE")
    );
    assert_eq!(store.load_memory("owner").unwrap().revision, revision);

    let calibration = run(&["条件定義: [scope=実験室][condition=校正] ユリは校正済み"]);
    let inspection = run(&["条件定義: [scope=実験室][condition=点検] ユリは点検済み"]);
    let witness_inspection = run(&["[scope=実験室]ユリは点検済み"]);
    let first = run(&[&format!("{guard}ユリは研究員")]);
    let query_one = format!("{guard}ユリは案内係ですか");
    let query_two = format!("{guard}ユリは入室可ですか");
    let blocked = |command: &str| stopped(&execute(&scratch.0, "owner", &[command]), 2, "SUSPEND");
    assert!(
        blocked(&query_one)["reason"]
            .as_str()
            .unwrap()
            .contains("CONDITION_UNKNOWN")
    );
    let witness_calibration = run(&["[scope=実験室]ユリは校正済み"]);
    let derived_one = run(&[&query_one]);
    assert_eq!(derived_one["guarantee"], "DERIVED");
    assert!(
        derived_one["dependencies"]
            .as_array()
            .unwrap()
            .contains(&rule_one["dependencies"][0])
    );
    assert!(
        blocked(&query_two)["reason"]
            .as_str()
            .unwrap()
            .contains("MISSING_PREMISES")
    );
    let second = run(&[&format!("{guard}ユリは当番")]);
    let derived_two = run(&[&query_two]);
    assert_eq!(derived_two["guarantee"], "DERIVED");
    for source in [
        rule_two,
        calibration,
        inspection,
        witness_inspection,
        witness_calibration,
        first,
        second,
    ] {
        assert!(
            derived_two["dependencies"]
                .as_array()
                .unwrap()
                .contains(&source["dependencies"][0])
        );
    }
}

#[test]
fn condition_references_on_one_premise_require_all_named_conditions() {
    let scratch = Scratch::new();
    let run = |args: &[&str]| record(&execute(&scratch.0, "owner", args));
    let reg = "　[scope=実験室][condition=点検][condition=校正]ユリは入室可。　";
    let first = run(&["条件定義: [scope=実験室][condition=点検] ユリは点検済み"]);
    let second = run(&["条件定義: [scope=実験室][condition=校正] ユリは校正済み"]);
    let evidence = run(&["[scope=実験室]ユリは点検済み"]);
    let premise = run(&["--case", "multiple-conditions", "--max-actions", "3", reg]);
    assert_eq!(premise["guarantee"], "USER_PREMISE");
    let store = minidora_engine::store::Store::open(&scratch.0).unwrap();
    let bundle = store
        .load_case("owner", "multiple-conditions")
        .unwrap()
        .unwrap()
        .bundle
        .unwrap();
    assert_eq!(
        bundle.meaning.parser_version,
        "declarative-ja-en-v134-multi-condition-relation"
    );
    for (index, value) in ["校正", "点検"].into_iter().enumerate() {
        let role = format!("/Assert/proposition/condition_refs/{index}");
        let spans = &bundle.meaning.source_roles[&role];
        assert_eq!(spans.len(), 1);
        assert_eq!(reg.get(spans[0].start..spans[0].end), Some(value));
    }
    let query = "[condition=校正][condition=点検][scope=実験室]ユリは入室可ですか";
    let stopped_result = || stopped(&execute(&scratch.0, "owner", &[query]), 2, "SUSPEND");
    let pending = stopped_result();
    assert!(
        pending["reason"]
            .as_str()
            .unwrap()
            .contains("CONDITION_UNKNOWN")
    );
    assert!(pending["reason"].as_str().unwrap().contains("点検=True"));
    let second_evidence = run(&["[scope=実験室]ユリは校正済み"]);
    let supported = run(&[query]);
    assert_eq!(supported["guarantee"], "USER_PREMISE");
    for item in [&first, &second, &premise, &evidence, &second_evidence] {
        assert!(
            supported["dependencies"]
                .as_array()
                .unwrap()
                .contains(&item["dependencies"][0])
        );
    }
    let without_second = "[scope=実験室][condition=点検]ユリは入室可ですか";
    assert!(
        stopped(
            &execute(&scratch.0, "owner", &[without_second]),
            2,
            "SUSPEND"
        )["reason"]
            .as_str()
            .unwrap()
            .contains("MISSING_PREMISES")
    );
    let fact_id = premise["dependencies"][0].as_str().unwrap();
    let source = run(&[&format!("Factの出典: {fact_id}")]);
    assert!(
        source["text"]
            .as_str()
            .unwrap()
            .contains("condition_refs/1")
    );
    run(&[&format!(
        "{}を無効化して",
        second_evidence["dependencies"][0].as_str().unwrap()
    )]);
    assert!(
        stopped_result()["reason"]
            .as_str()
            .unwrap()
            .contains("CONDITION_UNKNOWN")
    );
}

#[test]
fn condition_evaluation_detail_exposes_conflict_and_missing_operands() {
    let scratch = Scratch::new();
    let run = |args: &[&str]| record(&execute(&scratch.0, "owner", args));
    run(&["条件定義: [scope=実験室][condition=停止] ユリは停止中"]);
    let positive = run(&["[scope=実験室]ユリは停止中"]);
    let negative = run(&["[scope=実験室]ユリは停止中ではない"]);
    run(&[
        "条件定義: [scope=実験室][condition=監視][subject=ユリ] OR(AND([fact=ユリは点検済み], [ref=停止]), [fact=ユリは通信可能])",
    ]);
    let summary = run(&["条件評価: [condition=監視][subject=ユリ][scope=実験室]"]);
    let detail = run(&["　条件評価の内訳： [condition=監視][subject=ユリ][scope=実験室]。"]);
    let text = detail["text"].as_str().unwrap();
    assert!(text.starts_with("条件 監視: UNKNOWN\n"));
    assert!(text.starts_with(summary["text"].as_str().unwrap()));
    assert!(text.contains("内訳 3（親 2） AND: FALSE"));
    assert!(text.contains("内訳 5（親 3） 条件 停止: CONFLICT"));
    assert!(text.contains("内訳 4 の診断: CONDITION_EVIDENCE_NOT_FOUND"));
    assert!(!summary["text"].as_str().unwrap().contains("内訳 "));

    let store = minidora_engine::store::Store::open(&scratch.0).unwrap();
    let proof_of = |report: &Value| {
        sequence_artifact(
            &store,
            report["dependencies"].as_array().unwrap().last().unwrap(),
        )
    };
    let proof = proof_of(&detail);
    assert_eq!(proof[0], "minidora-condition-evaluation-detail-v1");
    assert_eq!(proof[1], detail["actor_memory_revision"]);
    assert_eq!(proof[2], proof_of(&summary)[2]);
    let trace = proof[3].as_array().unwrap();
    assert_eq!(trace.len(), 7);
    assert_eq!(trace[3]["target"]["object"], "点検済み");
    assert_eq!(trace[3]["reason"], "CONDITION_EVIDENCE_NOT_FOUND");
    assert_eq!(trace[5]["parent"], 4);
    assert_eq!(trace[5]["status"], "CONFLICT");
    assert_eq!(
        trace[5]["positive_fact_ids"],
        serde_json::json!([positive["dependencies"][0]])
    );
    assert_eq!(
        trace[5]["negative_fact_ids"],
        serde_json::json!([negative["dependencies"][0]])
    );
    assert_eq!(trace[6]["target"]["object"], "通信可能");
    assert_eq!(trace[6]["status"], "UNKNOWN");
    assert_eq!(
        detail["actor_memory_revision"],
        summary["actor_memory_revision"]
    );
    assert_eq!(
        store.load_memory("owner").unwrap().revision,
        detail["actor_memory_revision"].as_u64().unwrap()
    );
    assert!(store.actions("owner", "normal-route").unwrap().is_empty());
}

#[test]
fn composite_condition_from_ordinary_input_binds_and_invalidates_dependencies() {
    let scratch = Scratch::new();
    let actor = "owner";
    let condition_ids: Vec<_> = [
        "条件定義: [scope=実験室][condition=校正] ユリは校正済み",
        "条件定義: [scope=実験室][condition=点検] ユリは点検済み",
        "条件定義: [scope=実験室][condition=停止] ユリは停止中",
    ]
    .into_iter()
    .map(|text| {
        let result = record(&execute(&scratch.0, actor, &[text]));
        result["dependencies"][0].as_str().unwrap().to_owned()
    })
    .collect();
    let facts: Vec<_> = [
        "[scope=実験室]ユリは校正済み",
        "[scope=実験室]ユリは停止中ではない",
        "[scope=実験室][condition=監視]ユリは観測可能",
    ]
    .into_iter()
    .map(|text| record(&execute(&scratch.0, actor, &[text])))
    .collect();
    let store = minidora_engine::store::Store::open(&scratch.0).unwrap();
    let before = store.load_memory(actor).unwrap();
    let missing = [
        "--input",
        "missing-child",
        "条件定義: [scope=実験室][condition=監視][subject=ユリ] AND([ref=校正], [ref=未登録])",
    ];
    let stop = stopped(&execute(&scratch.0, actor, &missing), 2, "SUSPEND");
    assert!(
        stop["reason"]
            .as_str()
            .unwrap()
            .contains("UNKNOWN_CONDITION_DEPENDENCY")
    );
    assert!(stop["render"].is_null());
    assert!(stop["receipts"].as_array().unwrap().is_empty());
    assert_eq!(store.load_memory(actor).unwrap(), before);
    assert!(store.actions(actor, "normal-route").unwrap().is_empty());

    let definition = "条件定義: [scope=実験室][condition=監視][subject=ユリ] AND([ref=校正], OR([ref=点検], NOT([ref=停止])))";
    let registered = record(&execute(
        &scratch.0,
        actor,
        &["--input", "compound", definition],
    ));
    let condition_record = registered["dependencies"][0].as_str().unwrap();
    let memory = store.load_memory(actor).unwrap();
    let parent = memory
        .conditions
        .iter()
        .find(|condition| condition.id == condition_record)
        .unwrap();
    let mut expected = condition_ids.clone();
    expected.sort();
    assert_eq!(parent.dependencies, expected);
    assert_eq!(
        parent.definition.dependencies,
        ["停止".into(), "校正".into(), "点検".into()].into()
    );
    assert_eq!(parent.source_input, registered["id"]);

    let question = "なぜ[scope=実験室][condition=監視]ユリは観測可能ですか";
    let limited = stopped(
        &execute(
            &scratch.0,
            actor,
            &["--input", "budget-stop", "--max-actions", "29", question],
        ),
        2,
        "SUSPEND",
    );
    assert!(
        limited["reason"]
            .as_str()
            .unwrap()
            .starts_with("RESOURCE_LIMIT")
    );
    assert!(limited["render"].is_null());
    let actual = record(&execute(
        &scratch.0,
        actor,
        &["--input", "with-compound", question],
    ));
    let proof = assert_why_generation(&scratch.0, actor, "normal-route", &actual);
    assert!(
        proof["relation_meaning_plan"]["units"]
            .as_array()
            .unwrap()
            .iter()
            .any(|unit| unit["meaning"]["evaluation"]["status"] == "TRUE")
    );
    for id in condition_ids
        .iter()
        .map(String::as_str)
        .chain([condition_record])
    {
        assert!(
            actual["dependencies"]
                .as_array()
                .unwrap()
                .iter()
                .any(|value| value == id)
        );
    }

    let opposed = record(&execute(
        &scratch.0,
        actor,
        &["[scope=実験室]ユリは校正済みではない"],
    ));
    let conflict_limited = stopped(
        &execute(&scratch.0, actor, &["--max-actions", "31", question]),
        2,
        "SUSPEND",
    );
    assert!(
        conflict_limited["reason"]
            .as_str()
            .unwrap()
            .starts_with("RESOURCE_LIMIT")
    );
    let conflict = stopped(&execute(&scratch.0, actor, &[question]), 2, "SUSPEND");
    assert!(
        conflict["reason"]
            .as_str()
            .unwrap()
            .starts_with("CONDITION_CONFLICT")
    );
    assert!(
        conflict["dependencies"]
            .as_array()
            .unwrap()
            .contains(&opposed["dependencies"][0])
    );
    assert!(
        conflict["dependencies"]
            .as_array()
            .unwrap()
            .contains(&facts[0]["dependencies"][0])
    );
    assert!(conflict["render"].is_null());

    for fact_id in [&opposed["dependencies"][0], &facts[1]["dependencies"][0]] {
        record(&execute(
            &scratch.0,
            actor,
            &[&format!("invalidate {}", fact_id.as_str().unwrap())],
        ));
    }
    for text in [
        "[scope=実験室]ユリは停止中",
        "[scope=実験室]ユリは点検済みではない",
    ] {
        record(&execute(&scratch.0, actor, &[text]));
    }
    let false_limited = stopped(
        &execute(&scratch.0, actor, &["--max-actions", "31", question]),
        2,
        "SUSPEND",
    );
    assert!(
        false_limited["reason"]
            .as_str()
            .unwrap()
            .starts_with("RESOURCE_LIMIT")
    );
    let refuted = stopped(&execute(&scratch.0, actor, &[question]), 3, "REJECT");
    assert!(
        refuted["reason"]
            .as_str()
            .unwrap()
            .starts_with("CONDITION_FALSE")
    );
    assert_eq!(refuted["dependencies"].as_array().unwrap().len(), 7);
    assert!(refuted["render"].is_null());
    assert!(store.actions(actor, "normal-route").unwrap().is_empty());

    record(&execute(
        &scratch.0,
        actor,
        &[&format!("invalidate {}", condition_ids[2])],
    ));
    let stop_after = stopped(
        &execute(
            &scratch.0,
            actor,
            &["--input", "after-invalidation", question],
        ),
        2,
        "SUSPEND",
    );
    assert!(
        stop_after["reason"]
            .as_str()
            .unwrap()
            .starts_with("CONDITION_UNKNOWN")
    );
    assert!(stop_after["render"].is_null());
    let updated = store.load_memory(actor).unwrap();
    assert_eq!(updated.conditions.len(), memory.conditions.len());
    for condition in &updated.conditions {
        assert_eq!(
            condition.valid,
            condition.id != condition_record && condition.id != condition_ids[2]
        );
    }
    assert_eq!(
        stopped(&execute(&scratch.0, actor, &missing), 2, "SUSPEND"),
        stop
    );
}

#[test]
fn a_guarded_claim_remains_condition_evidence_when_both_index_lookups_select_it() {
    let scratch = Scratch::new();
    let actor = "owner";
    for text in [
        "条件定義: [scope=実験室][condition=校正] ユリは校正済み",
        "[scope=実験室][condition=校正]ユリは校正済み",
    ] {
        record(&execute(&scratch.0, actor, &[text]));
    }
    let question = "なぜ[scope=実験室][condition=校正]ユリは校正済みですか";
    let stopped_on_cycle = stopped(&execute(&scratch.0, actor, &[question]), 2, "SUSPEND");
    assert!(
        stopped_on_cycle["reason"]
            .as_str()
            .unwrap()
            .contains("CONDITION_EVIDENCE_CONDITION_UNRESOLVED")
    );
    assert!(stopped_on_cycle["render"].is_null());

    let evidence = record(&execute(
        &scratch.0,
        actor,
        &["[scope=実験室]ユリは校正済み"],
    ));
    let actual = record(&execute(&scratch.0, actor, &[question]));
    assert!(
        actual["dependencies"]
            .as_array()
            .unwrap()
            .contains(&evidence["dependencies"][0])
    );
}

#[test]
fn declared_rule_transfers_to_new_subject_in_new_case_with_bounded_lineage() {
    let scratch = Scratch::new();
    let rule = record(&execute_in_case(
        &scratch.0,
        "owner",
        "learning-origin",
        &[
            "--input",
            "cross-case-rule",
            "[scope=biology]Xは鳥なら[scope=biology]Xは動物",
        ],
    ));
    let rule_id = rule["dependencies"][0].as_str().unwrap();
    assert_eq!(rule["guarantee"], "USER_PREMISE");

    let original_bird = record(&execute_in_case(
        &scratch.0,
        "owner",
        "learning-origin",
        &[
            "--input",
            "original-subject-bird",
            "[scope=biology]ココは鳥",
        ],
    ));
    let original_answer = record(&execute_in_case(
        &scratch.0,
        "owner",
        "learning-origin",
        &[
            "--input",
            "original-subject-answer",
            "[scope=biology]ココは動物ですか",
        ],
    ));
    assert_eq!(original_answer["guarantee"], "DERIVED");

    // A genuinely new case and subject consume the actor's retained rule.
    let new_bird = record(&execute_in_case(
        &scratch.0,
        "owner",
        "learning-transfer",
        &["--input", "new-subject-bird", "[scope=biology]ミサゴは鳥"],
    ));
    let new_bird_id = new_bird["dependencies"][0].as_str().unwrap();
    let new_answer = record(&execute_in_case(
        &scratch.0,
        "owner",
        "learning-transfer",
        &[
            "--input",
            "new-subject-answer",
            "[scope=biology]ミサゴは動物ですか",
        ],
    ));
    assert_eq!(new_answer["guarantee"], "DERIVED");
    let new_dependencies = new_answer["dependencies"].as_array().unwrap();
    assert!(new_dependencies.contains(&Value::String(rule_id.into())));
    assert!(new_dependencies.contains(&Value::String(new_bird_id.into())));
    assert!(
        !new_dependencies.contains(&original_bird["dependencies"][0]),
        "the new-subject proof must not depend on the original subject"
    );

    let why = record(&execute_in_case(
        &scratch.0,
        "owner",
        "learning-transfer",
        &[
            "--input",
            "new-subject-why",
            "どうして[scope=biology]ミサゴは動物なのですか",
        ],
    ));
    assert_eq!(why["guarantee"], "DERIVED");
    let why_text = why["text"].as_str().unwrap();
    let artifact = assert_why_generation(&scratch.0, "owner", "learning-transfer", &why);
    assert_why_proposition(
        &artifact["relation_meaning_plan"]["conclusion"],
        "ミサゴ",
        "動物",
        "biology",
        false,
    );
    let premise = assert_why_record(&artifact, "FACT", &Value::from(new_bird_id));
    assert_why_proposition(&premise["proposition"], "ミサゴ", "鳥", "biology", false);
    let rule = assert_why_record(&artifact, "RULE", &Value::from(rule_id));
    assert_why_proposition(&rule["premises"][0], "$x", "鳥", "biology", false);
    assert_why_proposition(&rule["conclusion"], "$x", "動物", "biology", false);
    assert!(why_text.contains("「ミサゴ」") && why_text.contains("「鳥」"));
    assert_eq!(why["dependencies"], new_answer["dependencies"]);

    record(&execute_in_case(
        &scratch.0,
        "owner",
        "learning-transfer",
        &[
            "--input",
            "new-subject-counterexample",
            "[scope=biology]ミサゴは動物ではない",
        ],
    ));
    stopped(
        &execute_in_case(
            &scratch.0,
            "owner",
            "learning-transfer",
            &[
                "--input",
                "contested-new-subject-question",
                "[scope=biology]ミサゴは動物ですか",
            ],
        ),
        2,
        "SUSPEND",
    );

    record(&execute_in_case(
        &scratch.0,
        "owner",
        "learning-transfer",
        &[
            "--input",
            "invalidate-transferred-rule",
            &format!("{rule_id}を無効化して"),
        ],
    ));
    let answer_after_invalidation = record(&execute_in_case(
        &scratch.0,
        "owner",
        "learning-transfer",
        &[
            "--input",
            "answer-after-rule-invalidation",
            "[scope=biology]ミサゴは動物ですか",
        ],
    ));
    assert_eq!(answer_after_invalidation["guarantee"], "USER_PREMISE");
    assert_eq!(
        answer_after_invalidation["dependencies"],
        record(&execute_in_case(
            &scratch.0,
            "owner",
            "learning-transfer",
            &[
                "--input",
                "new-subject-counterexample",
                "[scope=biology]ミサゴは動物ではない"
            ],
        ))["dependencies"]
    );

    // A later case with another subject cannot use the invalidated rule.
    record(&execute_in_case(
        &scratch.0,
        "owner",
        "after-invalidation-new-task",
        &["--input", "later-subject-bird", "[scope=biology]ノスリは鳥"],
    ));
    stopped(
        &execute_in_case(
            &scratch.0,
            "owner",
            "after-invalidation-new-task",
            &[
                "--input",
                "later-subject-question",
                "[scope=biology]ノスリは動物ですか",
            ],
        ),
        2,
        "SUSPEND",
    );

    // The rule is scoped; a same-actor premise in a different scope is not enough.
    record(&execute_in_case(
        &scratch.0,
        "owner",
        "scope-isolation",
        &["--input", "other-scope-bird", "[scope=ecology]ペンギンは鳥"],
    ));
    stopped(
        &execute_in_case(
            &scratch.0,
            "owner",
            "scope-isolation",
            &[
                "--input",
                "other-scope-question",
                "[scope=ecology]ペンギンは動物ですか",
            ],
        ),
        2,
        "SUSPEND",
    );

    // Actor memories remain isolated even when the case and scope are explicit.
    record(&execute_in_case(
        &scratch.0,
        "separate-actor",
        "actor-isolation",
        &[
            "--input",
            "isolated-actor-bird",
            "[scope=biology]カモメは鳥",
        ],
    ));
    stopped(
        &execute_in_case(
            &scratch.0,
            "separate-actor",
            "actor-isolation",
            &[
                "--input",
                "isolated-actor-question",
                "[scope=biology]カモメは動物ですか",
            ],
        ),
        2,
        "SUSPEND",
    );

    // The original case result is still the committed result for its input.
    assert_eq!(
        record(&execute_in_case(
            &scratch.0,
            "owner",
            "learning-origin",
            &[
                "--input",
                "original-subject-answer",
                "[scope=biology]ココは動物ですか"
            ],
        )),
        original_answer
    );
}

#[test]
fn conjunctive_rule_joins_two_new_subject_premises_and_explains_both() {
    let scratch = Scratch::new();
    let rule = record(&execute_in_case(
        &scratch.0,
        "owner",
        "rule-origin",
        &[
            "--input",
            "conjunctive-rule",
            "If [scope=biology]X is a bird and feathered then [scope=biology]X is warm-blooded",
        ],
    ));
    let rule_id = rule["dependencies"][0].as_str().unwrap();
    assert_eq!(rule["guarantee"], "USER_PREMISE");
    assert!(
        rule["text"]
            .as_str()
            .unwrap()
            .contains("二つの条件付き前提")
    );

    let bird = record(&execute_in_case(
        &scratch.0,
        "owner",
        "new-subject",
        &["--input", "mira-bird", "[scope=biology]Mira is a bird"],
    ));
    let missing = stopped(
        &execute_in_case(
            &scratch.0,
            "owner",
            "new-subject",
            &[
                "--input",
                "mira-incomplete-question",
                "[scope=biology]Mira is warm-blooded?",
            ],
        ),
        2,
        "SUSPEND",
    );
    assert!(
        missing["reason"]
            .as_str()
            .unwrap()
            .contains("MISSING_PREMISES")
    );

    let feathered = record(&execute_in_case(
        &scratch.0,
        "owner",
        "new-subject",
        &[
            "--input",
            "mira-feathered",
            "[scope=biology]Mira is feathered",
        ],
    ));
    let answer = record(&execute_in_case(
        &scratch.0,
        "owner",
        "new-subject",
        &[
            "--input",
            "mira-warm-blooded",
            "[scope=biology]Mira is warm-blooded?",
        ],
    ));
    assert_eq!(answer["guarantee"], "DERIVED");
    for id in [
        rule_id,
        bird["dependencies"][0].as_str().unwrap(),
        feathered["dependencies"][0].as_str().unwrap(),
    ] {
        assert!(
            answer["dependencies"]
                .as_array()
                .unwrap()
                .contains(&Value::String(id.into())),
            "proof lineage is missing {id}"
        );
    }

    let why = record(&execute_in_case(
        &scratch.0,
        "owner",
        "new-subject",
        &[
            "--input",
            "mira-warm-blooded-why",
            "なぜ[scope=biology]Miraはwarm-bloodedですか",
        ],
    ));
    let explanation = why["text"].as_str().unwrap();
    let artifact = assert_why_generation(&scratch.0, "owner", "new-subject", &why);
    assert_why_proposition(
        &artifact["relation_meaning_plan"]["conclusion"],
        "Mira",
        "warm-blooded",
        "biology",
        false,
    );
    let rule = assert_why_record(&artifact, "RULE", &Value::from(rule_id));
    assert_eq!(rule["premises"].as_array().unwrap().len(), 2);
    for (object, fact) in [("bird", &bird), ("feathered", &feathered)] {
        let premise = assert_why_record(&artifact, "FACT", &fact["dependencies"][0]);
        assert_why_proposition(&premise["proposition"], "Mira", object, "biology", false);
        assert!(rule["premises"].as_array().unwrap().iter().any(|premise| {
            premise["subject"] == "$x"
                && premise["object"] == object
                && premise["scope"] == "biology"
        }));
        assert!(explanation.contains(&format!("「{object}」")));
    }
    assert_eq!(why["dependencies"], answer["dependencies"]);

    let followup_rule = record(&execute_in_case(
        &scratch.0,
        "owner",
        "rule-origin",
        &[
            "--input",
            "warm-blooded-to-animal",
            "[scope=biology]Xはwarm-bloodedなら[scope=biology]Xはanimal",
        ],
    ));
    let animal_answer = record(&execute_in_case(
        &scratch.0,
        "owner",
        "new-subject",
        &[
            "--input",
            "mira-animal",
            "[scope=biology]Miraはanimalですか",
        ],
    ));
    assert_eq!(animal_answer["guarantee"], "DERIVED");
    for id in [
        rule_id,
        followup_rule["dependencies"][0].as_str().unwrap(),
        bird["dependencies"][0].as_str().unwrap(),
        feathered["dependencies"][0].as_str().unwrap(),
    ] {
        assert!(
            animal_answer["dependencies"]
                .as_array()
                .unwrap()
                .contains(&Value::String(id.into())),
            "composed proof lineage is missing {id}"
        );
    }
    let animal_why = record(&execute_in_case(
        &scratch.0,
        "owner",
        "new-subject",
        &[
            "--input",
            "mira-animal-why",
            "なぜ[scope=biology]Miraはanimalですか",
        ],
    ));
    let animal_explanation = animal_why["text"].as_str().unwrap();
    assert_eq!(animal_why["dependencies"], animal_answer["dependencies"]);
    assert_eq!(animal_why["guarantee"], "DERIVED");
    for (object, fact) in [("bird", &bird), ("feathered", &feathered)] {
        let fact_id = fact["dependencies"][0].as_str().unwrap();
        assert!(animal_explanation.lines().any(|line| {
            line.contains("「Mira」")
                && line.contains(&format!("「{object}」"))
                && line.contains(fact_id)
        }));
    }
    let store = minidora_engine::store::Store::open(&scratch.0).unwrap();
    let compute_ref = animal_why["content_plan"]["dependencies"]
        .as_array()
        .unwrap()
        .last()
        .unwrap()
        .as_str()
        .unwrap();
    let compute: Value = serde_json::from_slice(
        &store
            .get_bytes("owner", "new-subject", compute_ref)
            .unwrap(),
    )
    .unwrap();
    assert_eq!(
        compute["kind"],
        "minidora.core-computation.relation-meaning.v7"
    );
    let meaning_plan = &compute["relation_meaning_plan"];
    assert_eq!(meaning_plan["conclusion"]["subject"], "Mira");
    assert_eq!(meaning_plan["conclusion"]["object"], "animal");
    assert_eq!(meaning_plan["conclusion"]["scope"], "biology");
    assert!(animal_explanation.contains("「animal」"));
    let composed_rule = meaning_plan["units"]
        .as_array()
        .unwrap()
        .iter()
        .map(|unit| &unit["meaning"])
        .find(|meaning| meaning["kind"] == "RULE" && meaning["composed"] == true)
        .unwrap();
    assert_eq!(composed_rule["conclusion"]["object"], "animal");
    let premise_objects: std::collections::BTreeSet<_> = composed_rule["premises"]
        .as_array()
        .unwrap()
        .iter()
        .map(|premise| premise["object"].as_str().unwrap())
        .collect();
    assert_eq!(
        premise_objects,
        std::collections::BTreeSet::from(["bird", "feathered"])
    );
    let rule_line = animal_explanation
        .lines()
        .find(|line| line.contains(composed_rule["record_id"].as_str().unwrap()))
        .unwrap();
    for id in [rule_id, followup_rule["dependencies"][0].as_str().unwrap()] {
        assert!(
            composed_rule["record_dependencies"]
                .as_array()
                .unwrap()
                .contains(&Value::from(id))
        );
        assert!(rule_line.contains(id));
    }

    record(&execute_in_case(
        &scratch.0,
        "owner",
        "rule-origin",
        &[
            "--input",
            "invalidate-followup-rule",
            &format!(
                "{}を無効化して",
                followup_rule["dependencies"][0].as_str().unwrap()
            ),
        ],
    ));
    stopped(
        &execute_in_case(
            &scratch.0,
            "owner",
            "new-subject",
            &[
                "--input",
                "mira-animal-after-rule-invalidation",
                "[scope=biology]Miraはanimalですか",
            ],
        ),
        2,
        "SUSPEND",
    );
    assert_eq!(
        record(&execute_in_case(
            &scratch.0,
            "owner",
            "new-subject",
            &[
                "--input",
                "mira-warm-blooded",
                "[scope=biology]Mira is warm-blooded?"
            ],
        )),
        answer,
        "composed-rule invalidation must retain earlier case results"
    );

    let feather_id = feathered["dependencies"][0].as_str().unwrap();
    record(&execute_in_case(
        &scratch.0,
        "owner",
        "new-subject",
        &[
            "--input",
            "invalidate-feathered-premise",
            &format!("{feather_id}を無効化して"),
        ],
    ));
    stopped(
        &execute_in_case(
            &scratch.0,
            "owner",
            "new-subject",
            &[
                "--input",
                "mira-after-invalidation",
                "[scope=biology]Mira is warm-blooded?",
            ],
        ),
        2,
        "SUSPEND",
    );
    assert_eq!(
        record(&execute_in_case(
            &scratch.0,
            "owner",
            "new-subject",
            &[
                "--input",
                "mira-warm-blooded",
                "[scope=biology]Mira is warm-blooded?",
            ],
        )),
        answer,
        "the earlier committed proof record must remain immutable"
    );

    let english_rule = record(&execute_in_case(
        &scratch.0,
        "english-owner",
        "english-rule-origin",
        &[
            "--input",
            "english-conjunctive-rule",
            "If X is a bird and X is feathered then X is warm-blooded",
        ],
    ));
    let english_bird = record(&execute_in_case(
        &scratch.0,
        "english-owner",
        "english-new-subject",
        &["--input", "robin-bird", "Robin is a bird"],
    ));
    let english_feathered = record(&execute_in_case(
        &scratch.0,
        "english-owner",
        "english-new-subject",
        &["--input", "robin-feathered", "Robin is feathered"],
    ));
    let english_answer = record(&execute_in_case(
        &scratch.0,
        "english-owner",
        "english-new-subject",
        &["--input", "robin-question", "Is Robin warm-blooded?"],
    ));
    assert_eq!(english_answer["guarantee"], "DERIVED");
    for id in [
        english_rule["dependencies"][0].as_str().unwrap(),
        english_bird["dependencies"][0].as_str().unwrap(),
        english_feathered["dependencies"][0].as_str().unwrap(),
    ] {
        assert!(
            english_answer["dependencies"]
                .as_array()
                .unwrap()
                .contains(&Value::String(id.into())),
            "English proof lineage is missing {id}"
        );
    }
    let english_why = record(&execute_in_case(
        &scratch.0,
        "english-owner",
        "english-new-subject",
        &["--input", "robin-why", "Why is Robin warm-blooded?"],
    ));
    let artifact = assert_why_generation(
        &scratch.0,
        "english-owner",
        "english-new-subject",
        &english_why,
    );
    assert_why_proposition(
        &artifact["relation_meaning_plan"]["conclusion"],
        "Robin",
        "warm-blooded",
        "default",
        false,
    );
    for (object, source) in [("bird", &english_bird), ("feathered", &english_feathered)] {
        let premise = assert_why_record(&artifact, "FACT", &source["dependencies"][0]);
        assert_why_proposition(&premise["proposition"], "Robin", object, "default", false);
    }
    let rule = assert_why_record(&artifact, "RULE", &english_rule["dependencies"][0]);
    assert_eq!(rule["premises"].as_array().unwrap().len(), 2);
    assert_eq!(english_why["dependencies"], english_answer["dependencies"]);

    let shared_variable_rule = record(&execute_in_case(
        &scratch.0,
        "shared-variable-owner",
        "relation-rule-origin",
        &[
            "--input",
            "shared-variable-rule",
            "If X is a bird and X is Y then Y is an animal",
        ],
    ));
    let joined_subject_bird = record(&execute_in_case(
        &scratch.0,
        "shared-variable-owner",
        "relation-rule-consumer",
        &["--input", "osprey-bird", "Osprey is a bird"],
    ));
    let bound_relation = record(&execute_in_case(
        &scratch.0,
        "shared-variable-owner",
        "relation-rule-consumer",
        &["--input", "osprey-robin", "Osprey is Robin"],
    ));
    let bound_subject_answer = record(&execute_in_case(
        &scratch.0,
        "shared-variable-owner",
        "relation-rule-consumer",
        &["--input", "robin-animal", "Is Robin an animal?"],
    ));
    assert_eq!(bound_subject_answer["guarantee"], "DERIVED");
    for id in [
        shared_variable_rule["dependencies"][0].as_str().unwrap(),
        joined_subject_bird["dependencies"][0].as_str().unwrap(),
        bound_relation["dependencies"][0].as_str().unwrap(),
    ] {
        assert!(
            bound_subject_answer["dependencies"]
                .as_array()
                .unwrap()
                .contains(&Value::String(id.into())),
            "shared-variable proof lineage is missing {id}"
        );
    }
    let bound_subject_why = record(&execute_in_case(
        &scratch.0,
        "shared-variable-owner",
        "relation-rule-consumer",
        &["--input", "robin-why-animal", "Why is Robin an animal?"],
    ));
    let artifact = assert_why_generation(
        &scratch.0,
        "shared-variable-owner",
        "relation-rule-consumer",
        &bound_subject_why,
    );
    assert_why_proposition(
        &artifact["relation_meaning_plan"]["conclusion"],
        "Robin",
        "animal",
        "default",
        false,
    );
    for (object, source) in [("bird", &joined_subject_bird), ("Robin", &bound_relation)] {
        let premise = assert_why_record(&artifact, "FACT", &source["dependencies"][0]);
        assert_why_proposition(&premise["proposition"], "Osprey", object, "default", false);
    }
    let rule = assert_why_record(&artifact, "RULE", &shared_variable_rule["dependencies"][0]);
    assert_eq!(rule["premises"].as_array().unwrap().len(), 2);
    assert_eq!(
        bound_subject_why["dependencies"],
        bound_subject_answer["dependencies"]
    );
}

#[test]
fn ordinary_question_lists_direct_and_derived_relations_with_provenance_after_restart() {
    let scratch = Scratch::new();
    let bird = record(&execute(
        &scratch.0,
        "owner",
        &["--input", "coco-bird", "ココは鳥"],
    ));
    let gray = record(&execute(
        &scratch.0,
        "owner",
        &["--input", "coco-gray", "[scope=story]ココは灰色"],
    ));
    let negative = record(&execute(
        &scratch.0,
        "owner",
        &["--input", "coco-negative", "ココは哺乳類ではない"],
    ));
    let negative_animal = record(&execute(
        &scratch.0,
        "owner",
        &["--input", "coco-negative-animal", "ココは動物ではない"],
    ));
    let rule = record(&execute(
        &scratch.0,
        "owner",
        &["--input", "bird-rule", "もしXは鳥ならXは動物"],
    ));
    let listed = record(&execute(
        &scratch.0,
        "owner",
        &[
            "--input",
            "coco-listing",
            "ココについて、何が分かっていますか",
        ],
    ));

    assert_eq!(listed["lifecycle"], "COMMIT");
    assert_eq!(listed["guarantee"], "DERIVED");
    assert!(
        listed["text"]
            .as_str()
            .unwrap()
            .contains("現実世界の真偽や全件網羅性を確認したものではありません")
    );
    assert!(listed["text"].as_str().unwrap().contains("規則導出"));
    assert!(listed["text"].as_str().unwrap().contains("導出ID:"));
    assert!(listed["text"].as_str().unwrap().contains("対立根拠として"));
    assert!(listed["text"].as_str().unwrap().contains("ココは鳥である"));
    assert!(
        listed["text"]
            .as_str()
            .unwrap()
            .contains("ココは哺乳類ではない")
    );
    assert!(listed["text"].as_str().unwrap().contains("範囲: story"));
    assert!(
        listed["text"]
            .as_str()
            .unwrap()
            .contains("ココは動物である")
    );
    assert!(
        listed["text"]
            .as_str()
            .unwrap()
            .contains("ココは動物ではない")
    );
    assert_eq!(listed["dependencies"].as_array().unwrap().len(), 6);
    for source in [&bird, &gray, &negative] {
        assert!(
            listed["dependencies"]
                .as_array()
                .unwrap()
                .contains(&source["dependencies"][0])
        );
    }
    assert!(
        listed["dependencies"]
            .as_array()
            .unwrap()
            .contains(&negative_animal["dependencies"][0])
    );
    assert!(
        listed["dependencies"]
            .as_array()
            .unwrap()
            .contains(&rule["dependencies"][0])
    );
    let derived_line = listed["text"]
        .as_str()
        .unwrap()
        .lines()
        .find(|line| line.contains("導出ID: "))
        .unwrap();
    let derived_id = derived_line
        .split("導出ID: ")
        .nth(1)
        .unwrap()
        .split(';')
        .next()
        .unwrap();
    assert!(
        listed["dependencies"]
            .as_array()
            .unwrap()
            .contains(&Value::String(derived_id.to_string()))
    );
    let saved = inspect_normal_case(&scratch.0, "owner");
    assert_eq!(
        saved["bundle"]["meaning"]["parser_version"],
        minidora_engine::input::PARSER_VERSION
    );
    assert_eq!(
        saved["bundle"]["meaning"]["intent"]["kind"],
        "INSPECT_PREMISES"
    );
    assert_eq!(saved["bundle"]["meaning"]["intent"]["subject"], "ココ");

    record(&execute(
        &scratch.0,
        "owner",
        &[
            "--input",
            "invalidate-coco-bird",
            &format!("{}を無効化して", bird["dependencies"][0].as_str().unwrap()),
        ],
    ));
    let after_invalidation = record(&execute(
        &scratch.0,
        "owner",
        &[
            "--input",
            "coco-listing-after",
            "ココについて、何が分かっていますか",
        ],
    ));
    assert_eq!(
        after_invalidation["dependencies"].as_array().unwrap().len(),
        3
    );
    assert_eq!(after_invalidation["guarantee"], "USER_PREMISE");
    assert!(
        !after_invalidation["dependencies"]
            .as_array()
            .unwrap()
            .contains(&bird["dependencies"][0])
    );
    assert!(
        !after_invalidation["text"]
            .as_str()
            .unwrap()
            .contains("ココは鳥である")
    );
    assert!(
        after_invalidation["text"]
            .as_str()
            .unwrap()
            .contains("ココは動物ではない")
    );
    assert!(
        !after_invalidation["text"]
            .as_str()
            .unwrap()
            .contains("規則導出")
    );
    let missing = stopped(
        &execute(
            &scratch.0,
            "owner",
            &[
                "--input",
                "unknown-subject-listing",
                "ミケについて何が分かっていますか",
            ],
        ),
        2,
        "SUSPEND",
    );
    assert!(
        missing["reason"]
            .as_str()
            .unwrap()
            .contains("MISSING_PREMISES")
    );
    let other_actor = stopped(
        &execute_in_case(
            &scratch.0,
            "separate-actor",
            "other-actor-inventory",
            &[
                "--input",
                "other-actor-listing",
                "ココについて、何が分かっていますか",
            ],
        ),
        2,
        "SUSPEND",
    );
    assert!(
        other_actor["reason"]
            .as_str()
            .unwrap()
            .contains("MISSING_PREMISES")
    );

    let resource_limited = stopped(
        &execute_in_case(
            &scratch.0,
            "owner",
            "inventory-budget-limit",
            &[
                "--input",
                "coco-listing-low-budget",
                "--max-actions",
                "6",
                "ココについて、何が分かっていますか",
            ],
        ),
        2,
        "SUSPEND",
    );
    assert!(
        resource_limited["reason"]
            .as_str()
            .unwrap()
            .contains("RESOURCE_LIMIT")
    );
}

#[test]
fn natural_transitive_object_questions_use_same_actor_scope_and_relation_proofs() {
    let scratch = Scratch::new();
    let direct = record(&execute(
        &scratch.0,
        "owner",
        &["--input", "mina-cutter", "ミナはカッターを使う"],
    ));
    let rule = record(&execute(
        &scratch.0,
        "owner",
        &[
            "--input",
            "tool-rule",
            "もしXはカッターを使うならXは道具を使う",
        ],
    ));
    let supported = record(&execute(
        &scratch.0,
        "owner",
        &["--input", "mina-uses-question", "ミナは何を使いますか"],
    ));
    assert_eq!(supported["lifecycle"], "COMMIT");
    assert_eq!(supported["guarantee"], "DERIVED");
    assert!(
        supported["text"]
            .as_str()
            .unwrap()
            .contains("ミナはカッターを使う")
    );
    assert!(
        supported["text"]
            .as_str()
            .unwrap()
            .contains("ミナは道具を使う")
    );
    assert!(supported["text"].as_str().unwrap().contains("直接提示:"));
    assert!(supported["text"].as_str().unwrap().contains("規則導出:"));
    assert!(
        supported["text"]
            .as_str()
            .unwrap()
            .contains("網羅性や現実世界の真偽は確認していません")
    );
    for identity in [&direct["dependencies"][0], &rule["dependencies"][0]] {
        assert!(
            supported["dependencies"]
                .as_array()
                .unwrap()
                .contains(identity)
        );
    }
    let saved = inspect_normal_case(&scratch.0, "owner");
    assert_eq!(
        saved["bundle"]["meaning"]["intent"]["kind"],
        "ASK_RELATION_OBJECTS"
    );
    assert_eq!(saved["bundle"]["meaning"]["intent"]["subject"], "ミナ");
    assert_eq!(saved["bundle"]["meaning"]["intent"]["predicate"], "use");

    let negative = record(&execute(
        &scratch.0,
        "owner",
        &["--input", "mina-not-cloud", "ミナは毒を使わない"],
    ));
    let negative_answer = record(&execute(
        &scratch.0,
        "owner",
        &[
            "--input",
            "mina-not-uses-question",
            "ミナは何を使いませんか",
        ],
    ));
    assert_eq!(negative_answer["lifecycle"], "COMMIT");
    assert_eq!(negative_answer["guarantee"], "USER_PREMISE");
    assert!(
        negative_answer["text"]
            .as_str()
            .unwrap()
            .contains("ミナは毒を使わない")
    );
    assert!(
        negative_answer["dependencies"]
            .as_array()
            .unwrap()
            .contains(&negative["dependencies"][0])
    );

    let drink = record(&execute(
        &scratch.0,
        "owner",
        &["--input", "mina-drinks-water", "ミナは水を飲む"],
    ));
    assert_eq!(drink["lifecycle"], "COMMIT");
    assert!(drink["text"].as_str().unwrap().contains("ミナは水を飲む"));
    let drink_rule = record(&execute(
        &scratch.0,
        "owner",
        &["--input", "drink-rule", "もしXは水を飲むならXは液体を飲む"],
    ));
    assert_eq!(drink_rule["lifecycle"], "COMMIT");
    let drink_question = record(&execute(
        &scratch.0,
        "owner",
        &["--input", "mina-what-drinks", "ミナは何を飲む?"],
    ));
    assert_eq!(drink_question["lifecycle"], "COMMIT");
    assert_eq!(drink_question["guarantee"], "DERIVED");
    let drink_text = drink_question["text"].as_str().unwrap();
    assert!(drink_text.contains("直接提示: ミナは水を飲む"));
    assert!(drink_text.contains("規則導出: ミナは液体を飲む"));
    for identity in [&drink["dependencies"][0], &drink_rule["dependencies"][0]] {
        assert!(
            drink_question["dependencies"]
                .as_array()
                .unwrap()
                .contains(identity)
        );
    }
    let drink_no_poison = record(&execute(
        &scratch.0,
        "owner",
        &["--input", "mina-no-poison-drink", "ミナは毒を飲まない"],
    ));
    assert_eq!(drink_no_poison["lifecycle"], "COMMIT");
    assert!(
        drink_no_poison["text"]
            .as_str()
            .unwrap()
            .contains("ミナは毒を飲まない")
    );
    let no_drink_question = record(&execute(
        &scratch.0,
        "owner",
        &["--input", "mina-what-not-drinks", "ミナは何を飲まない?"],
    ));
    assert_eq!(no_drink_question["lifecycle"], "COMMIT");
    assert_eq!(no_drink_question["guarantee"], "USER_PREMISE");
    assert!(
        no_drink_question["text"]
            .as_str()
            .unwrap()
            .contains("ミナは毒を飲まない")
    );
    assert!(
        no_drink_question["dependencies"]
            .as_array()
            .unwrap()
            .contains(&drink_no_poison["dependencies"][0])
    );
    let yes_no_drink = record(&execute(
        &scratch.0,
        "owner",
        &["--input", "mina-drinks-water-question", "ミナは水を飲むか"],
    ));
    assert_eq!(yes_no_drink["lifecycle"], "COMMIT");
    assert!(
        yes_no_drink["text"]
            .as_str()
            .unwrap()
            .contains("ミナは水を飲む")
    );
    let polite_drink_question = stopped(
        &execute(
            &scratch.0,
            "owner",
            &[
                "--input",
                "unsupported-polite-drink",
                "ミナは何を飲みますか",
            ],
        ),
        2,
        "SUSPEND",
    );
    assert!(
        polite_drink_question["reason"]
            .as_str()
            .unwrap()
            .contains("CAPABILITY_UNAVAILABLE")
    );

    record(&execute(
        &scratch.0,
        "owner",
        &[
            "--input",
            "mina-story-notebook",
            "[scope=story]ミナはノートを使う",
        ],
    ));
    let wrong_scope = stopped(
        &execute(
            &scratch.0,
            "owner",
            &[
                "--input",
                "mina-ecology-question",
                "[scope=ecology]ミナは何を使う?",
            ],
        ),
        2,
        "SUSPEND",
    );
    assert!(
        wrong_scope["reason"]
            .as_str()
            .unwrap()
            .contains("MISSING_PREMISES")
    );
    let missing_subject = stopped(
        &execute(
            &scratch.0,
            "owner",
            &["--input", "saki-uses-question", "サキは何を使う?"],
        ),
        2,
        "SUSPEND",
    );
    assert!(
        missing_subject["reason"]
            .as_str()
            .unwrap()
            .contains("MISSING_PREMISES")
    );
    let other_actor = stopped(
        &execute_in_case(
            &scratch.0,
            "another-actor",
            "separate-actor-case",
            &["--input", "mina-other-actor-question", "ミナは何を使う?"],
        ),
        2,
        "SUSPEND",
    );
    assert!(
        other_actor["reason"]
            .as_str()
            .unwrap()
            .contains("MISSING_PREMISES")
    );

    let contradictory = record(&execute(
        &scratch.0,
        "owner",
        &["--input", "mina-uses-poison", "ミナは毒を使う"],
    ));
    let conflict = stopped(
        &execute(
            &scratch.0,
            "owner",
            &["--input", "mina-uses-conflict-question", "ミナは何を使う?"],
        ),
        2,
        "SUSPEND",
    );
    assert!(
        conflict["reason"]
            .as_str()
            .unwrap()
            .contains("CONFLICTING_PREMISES")
    );

    record(&execute(
        &scratch.0,
        "owner",
        &[
            "--input",
            "invalidate-poison-positive",
            &format!(
                "{}を無効化して",
                contradictory["dependencies"][0].as_str().unwrap()
            ),
        ],
    ));
    let after_invalidation = record(&execute(
        &scratch.0,
        "owner",
        &["--input", "mina-uses-question", "ミナは何を使いますか"],
    ));
    assert_eq!(after_invalidation["text"], supported["text"]);
    assert_eq!(
        after_invalidation["dependencies"],
        supported["dependencies"]
    );
    assert!(
        !after_invalidation["text"]
            .as_str()
            .unwrap()
            .contains("ミナは毒を使う")
    );
}

#[test]
fn japanese_transitive_yes_no_questions_use_relation_support_negation_and_scope() {
    let scratch = Scratch::new();
    let direct = record(&execute(
        &scratch.0,
        "owner",
        &["--input", "yu-notebook", "ユウはノートを使う"],
    ));
    let rule = record(&execute(
        &scratch.0,
        "owner",
        &[
            "--input",
            "yu-tool-rule",
            "もしXはノートを使うならXは道具を使う",
        ],
    ));

    let direct_answer = record(&execute(
        &scratch.0,
        "owner",
        &[
            "--input",
            "yu-notebook-question",
            "ユウはノートを使いますか",
        ],
    ));
    assert_eq!(direct_answer["lifecycle"], "COMMIT");
    assert_eq!(direct_answer["guarantee"], "USER_PREMISE");
    assert!(
        direct_answer["text"]
            .as_str()
            .unwrap()
            .contains("ユウはノートを使う")
    );
    assert!(
        direct_answer["dependencies"]
            .as_array()
            .unwrap()
            .contains(&direct["dependencies"][0])
    );

    let derived_answer = record(&execute(
        &scratch.0,
        "owner",
        &["--input", "yu-tool-question", "ユウは道具を使いますか"],
    ));
    assert_eq!(derived_answer["lifecycle"], "COMMIT");
    assert_eq!(derived_answer["guarantee"], "DERIVED");
    assert!(
        derived_answer["text"]
            .as_str()
            .unwrap()
            .contains("ユウは道具を使う")
    );
    for identity in [&direct["dependencies"][0], &rule["dependencies"][0]] {
        assert!(
            derived_answer["dependencies"]
                .as_array()
                .unwrap()
                .contains(identity)
        );
    }

    let negative = record(&execute(
        &scratch.0,
        "owner",
        &["--input", "yu-not-poison", "ユウは毒を使わない"],
    ));
    let negative_answer = record(&execute(
        &scratch.0,
        "owner",
        &[
            "--input",
            "yu-not-poison-question",
            "ユウは毒を使いませんか",
        ],
    ));
    assert_eq!(negative_answer["lifecycle"], "COMMIT");
    assert_eq!(negative_answer["guarantee"], "USER_PREMISE");
    assert!(
        negative_answer["text"]
            .as_str()
            .unwrap()
            .contains("ユウは毒を使わない")
    );
    let refuted = record(&execute(
        &scratch.0,
        "owner",
        &[
            "--input",
            "yu-positive-poison-question",
            "ユウは毒を使いますか",
        ],
    ));
    assert_eq!(refuted["lifecycle"], "COMMIT");
    assert_eq!(refuted["guarantee"], "USER_PREMISE");
    assert!(
        refuted["text"]
            .as_str()
            .unwrap()
            .contains("ユウは毒を使わない")
    );
    assert!(
        refuted["dependencies"]
            .as_array()
            .unwrap()
            .contains(&negative["dependencies"][0])
    );

    record(&execute(
        &scratch.0,
        "owner",
        &[
            "--input",
            "yu-story-notebook",
            "[scope=story]ユウはノートを使う",
        ],
    ));
    let wrong_scope = stopped(
        &execute(
            &scratch.0,
            "owner",
            &[
                "--input",
                "yu-ecology-question",
                "[scope=ecology]ユウは道具を使いますか",
            ],
        ),
        2,
        "SUSPEND",
    );
    assert!(
        wrong_scope["reason"]
            .as_str()
            .unwrap()
            .contains("MISSING_PREMISES")
    );
    let missing = stopped(
        &execute(
            &scratch.0,
            "owner",
            &["--input", "yu-pen-question", "ユウはペンを使いますか"],
        ),
        2,
        "SUSPEND",
    );
    assert!(
        missing["reason"]
            .as_str()
            .unwrap()
            .contains("MISSING_PREMISES")
    );
    let other_actor = stopped(
        &execute_in_case(
            &scratch.0,
            "another-actor",
            "separate-actor-case",
            &[
                "--input",
                "yu-other-actor-question",
                "ユウはノートを使いますか",
            ],
        ),
        2,
        "SUSPEND",
    );
    assert!(
        other_actor["reason"]
            .as_str()
            .unwrap()
            .contains("MISSING_PREMISES")
    );

    let contradictory = record(&execute(
        &scratch.0,
        "owner",
        &["--input", "yu-poison-positive", "ユウは毒を使う"],
    ));
    let conflict = stopped(
        &execute(
            &scratch.0,
            "owner",
            &["--input", "yu-conflicting-question", "ユウは毒を使いますか"],
        ),
        2,
        "SUSPEND",
    );
    assert!(
        conflict["reason"]
            .as_str()
            .unwrap()
            .contains("CONFLICTING_PREMISES")
    );
    record(&execute(
        &scratch.0,
        "owner",
        &[
            "--input",
            "invalidate-yu-poison-positive",
            &format!(
                "{}を無効化して",
                contradictory["dependencies"][0].as_str().unwrap()
            ),
        ],
    ));
    let replay = record(&execute(
        &scratch.0,
        "owner",
        &["--input", "yu-tool-question", "ユウは道具を使いますか"],
    ));
    assert_eq!(replay["text"], derived_answer["text"]);
    assert_eq!(replay["dependencies"], derived_answer["dependencies"]);
}

#[test]
fn natural_definition_questions_retrieve_only_the_exact_stored_premise() {
    let scratch = Scratch::new();
    let japanese_premise = record(&execute(
        &scratch.0,
        "owner",
        &["--input", "coco-definition-premise", "ココは鳥"],
    ));
    let english_premise = record(&execute(
        &scratch.0,
        "owner",
        &["--input", "coco-definition-premise-en", "Coco is a mammal"],
    ));

    let japanese = record(&execute(
        &scratch.0,
        "owner",
        &["--input", "coco-definition-query", "ココとは何ですか"],
    ));
    assert_eq!(japanese["lifecycle"], "COMMIT");
    assert_eq!(japanese["guarantee"], "USER_PREMISE");
    assert!(
        japanese["text"]
            .as_str()
            .unwrap()
            .contains("ココは鳥である")
    );
    assert!(
        japanese["text"]
            .as_str()
            .unwrap()
            .contains("現実世界の真偽や全件網羅性を確認したものではありません")
    );
    assert!(
        japanese["dependencies"]
            .as_array()
            .unwrap()
            .contains(&japanese_premise["dependencies"][0])
    );
    assert!(
        !japanese["dependencies"]
            .as_array()
            .unwrap()
            .contains(&english_premise["dependencies"][0])
    );

    let english = record(&execute(
        &scratch.0,
        "owner",
        &["--input", "coco-definition-query-en", "What is Coco?"],
    ));
    assert_eq!(english["lifecycle"], "COMMIT");
    assert_eq!(english["guarantee"], "USER_PREMISE");
    assert!(
        english["text"]
            .as_str()
            .unwrap()
            .contains("Cocoはmammalである")
    );
    assert!(
        english["dependencies"]
            .as_array()
            .unwrap()
            .contains(&english_premise["dependencies"][0])
    );
    assert!(
        !english["dependencies"]
            .as_array()
            .unwrap()
            .contains(&japanese_premise["dependencies"][0])
    );

    let missing = stopped(
        &execute(
            &scratch.0,
            "owner",
            &[
                "--input",
                "coco-definition-missing",
                "What is an unknown animal?",
            ],
        ),
        2,
        "SUSPEND",
    );
    assert!(
        missing["reason"]
            .as_str()
            .unwrap()
            .contains("MISSING_PREMISES")
    );
}

#[test]
fn natural_relation_profile_uses_current_proofs_and_feeds_a_follow_up_consumer() {
    let scratch = Scratch::new();
    let rule = record(&execute(
        &scratch.0,
        "owner",
        &["--input", "profile-rule", "もしXは鳥ならXは動物"],
    ));
    let bird = record(&execute(
        &scratch.0,
        "owner",
        &["--input", "profile-premise", "ココは鳥"],
    ));
    let profile = record(&execute(
        &scratch.0,
        "owner",
        &[
            "--input",
            "profile-from-records",
            "ココについて保存されている情報をもとに短い紹介文を書いて",
        ],
    ));
    assert_eq!(profile["lifecycle"], "COMMIT");
    assert_eq!(profile["guarantee"], "HYPOTHESIS");
    let text = profile["text"].as_str().unwrap();
    assert!(text.contains("会話で提示された前提は「ココは鳥である」"));
    assert!(text.contains("保存された規則による導出は「ココは動物である」"));
    assert!(text.contains("現実世界の真偽や記録の網羅性は確認していません"));
    for dependency in [
        bird["dependencies"][0].as_str().unwrap(),
        rule["dependencies"][0].as_str().unwrap(),
    ] {
        assert!(
            profile["dependencies"]
                .as_array()
                .unwrap()
                .iter()
                .any(|value| { value.as_str().is_some_and(|text| text == dependency) })
        );
    }

    let model_observation_ref = profile["content_plan"]["model_observation_ref"]
        .as_str()
        .unwrap();
    let store = minidora_engine::store::Store::open(&scratch.0).unwrap();
    let emission: Value = serde_json::from_slice(
        &store
            .get_bytes("owner", "normal-route", model_observation_ref)
            .unwrap(),
    )
    .unwrap();
    assert_eq!(emission["text"], profile["text"]);
    assert_eq!(
        emission["decoder_version"],
        "bounded-finite-surface-literal-exact-v1"
    );
    let surfaces = emission["authorized_surfaces"].as_array().unwrap();
    assert_eq!(surfaces.len(), 2);
    for surface in surfaces {
        let surface = surface.as_str().unwrap();
        assert!(surface.contains("ココは鳥である"));
        assert!(surface.contains("ココは動物である"));
        assert!(surface.contains("現実世界の真偽や記録の網羅性は確認していません"));
    }
    drop(store);

    let recovered = minidora_engine::Engine::open(&scratch.0)
        .unwrap()
        .reconcile("owner", "normal-route")
        .unwrap();
    assert_eq!(recovered.id, profile["id"]);
    assert_eq!(recovered.text, text);

    let summary = record(&execute(
        &scratch.0,
        "owner",
        &[
            "--input",
            "profile-follow-up-summary",
            "前回の説明を2文以内で要約して",
        ],
    ));
    assert_eq!(summary["lifecycle"], "COMMIT");
    assert_eq!(summary["guarantee"], "HYPOTHESIS");
    assert!(
        summary["dependencies"]
            .as_array()
            .unwrap()
            .contains(&profile["id"])
    );

    let missing = stopped(
        &execute_in_case(
            &scratch.0,
            "other-actor",
            "profile-other-actor",
            &[
                "--input",
                "profile-no-cross-actor-fallback",
                "ココについて保存されている情報をもとに短い紹介文を書いて",
            ],
        ),
        2,
        "SUSPEND",
    );
    assert!(
        missing["reason"]
            .as_str()
            .unwrap()
            .contains("MISSING_PREMISES")
    );

    record(&execute_in_case(
        &scratch.0,
        "conflict-owner",
        "profile-conflict-case",
        &["--input", "profile-conflict-positive", "ミケは鳥"],
    ));
    record(&execute_in_case(
        &scratch.0,
        "conflict-owner",
        "profile-conflict-case",
        &["--input", "profile-conflict-negative", "ミケは鳥ではない"],
    ));
    let conflicted = record(&execute_in_case(
        &scratch.0,
        "conflict-owner",
        "profile-conflict-case",
        &[
            "--input",
            "profile-conflict-draft",
            "ミケについて保存されている情報をもとに短い紹介文を書いて",
        ],
    ));
    assert_eq!(conflicted["lifecycle"], "COMMIT");
    assert!(
        conflicted["text"]
            .as_str()
            .unwrap()
            .contains("肯定と否定の根拠")
    );
    assert!(
        conflicted["text"]
            .as_str()
            .unwrap()
            .contains("対立根拠の一方")
    );
    assert!(
        conflicted["text"]
            .as_str()
            .unwrap()
            .contains("ミケは鳥である")
    );
    assert!(
        conflicted["text"]
            .as_str()
            .unwrap()
            .contains("ミケは鳥ではない")
    );
}

#[test]
fn natural_relation_profile_explanation_replays_the_prior_result_in_the_cli() {
    let scratch = Scratch::new();
    let rule = record(&execute(
        &scratch.0,
        "owner",
        &[
            "--input",
            "profile-explanation-rule",
            "もしXは鳥ならXは動物",
        ],
    ));
    let premise = record(&execute(
        &scratch.0,
        "owner",
        &["--input", "profile-explanation-premise", "ココは鳥"],
    ));
    let profile = record(&execute(
        &scratch.0,
        "owner",
        &[
            "--input",
            "profile-explanation-source",
            "ココについて保存されている情報をもとに短い紹介文を書いて",
        ],
    ));
    assert_eq!(profile["lifecycle"], "COMMIT");
    assert_eq!(profile["guarantee"], "HYPOTHESIS");

    let explanation = record(&execute(
        &scratch.0,
        "owner",
        &["--input", "profile-explanation-follow-up", "詳しく説明して"],
    ));
    assert_eq!(explanation["lifecycle"], "COMMIT");
    assert_eq!(explanation["guarantee"], "HYPOTHESIS");
    let explanation_text = explanation["text"].as_str().unwrap();
    for marker in [
        "前回の保存済み紹介文案を説明します",
        profile["id"].as_str().unwrap(),
        "会話で提示された前提は「ココは鳥である」",
        "保存された規則による導出は「ココは動物である」",
        "現実世界の真偽や記録の網羅性は確認していません",
    ] {
        assert!(explanation_text.contains(marker), "missing {marker:?}");
    }
    for dependency in [
        profile["id"].as_str().unwrap(),
        rule["dependencies"][0].as_str().unwrap(),
        premise["dependencies"][0].as_str().unwrap(),
    ] {
        assert!(
            explanation["dependencies"]
                .as_array()
                .unwrap()
                .iter()
                .any(|value| value.as_str() == Some(dependency)),
            "missing dependency {dependency}"
        );
    }
}

#[test]
fn input_correction_binds_mixed_article_values_to_the_original_bytes() {
    let scratch = Scratch::new();
    let actor = "owner";
    let original = record(&execute(
        &scratch.0,
        actor,
        &[
            "--input",
            "before-correction",
            "[scope=lab]Nera is a sensor",
        ],
    ));
    let text = "[scope=lab]Neraはa sensorではなくan operatorです";
    let corrected = record(&execute(
        &scratch.0,
        actor,
        &["--input", "correction", text],
    ));
    assert_eq!(corrected["lifecycle"], "COMMIT");
    let case = inspect_normal_case(&scratch.0, actor);
    assert_eq!(
        case["prior_results"][original["id"].as_str().unwrap()],
        original
    );
    let bundle = &case["bundle"];
    let store = minidora_engine::store::Store::open(&scratch.0).unwrap();
    let memory = store.load_memory(actor).unwrap();
    let previous = memory
        .facts
        .iter()
        .find(|fact| fact.id == original["dependencies"][0])
        .unwrap();
    assert!(!previous.valid);
    let replacement = memory
        .facts
        .iter()
        .find(|fact| fact.valid && fact.proposition.object == "operator")
        .unwrap();
    let evidence = replacement.source_evidence.as_ref().unwrap();
    assert_eq!(evidence.semantic_signature, bundle["semantic_signature"]);
    assert_eq!(
        evidence.raw_sha256,
        minidora_domain::digest(text.as_bytes())
    );
    for (role, value) in [
        ("superseded_article", "a"),
        ("superseded_object", "sensor"),
        ("replacement_article", "an"),
        ("replacement_object", "operator"),
    ] {
        let spans = &evidence.source_roles[&format!("/CorrectPremise/{role}")];
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].text, value);
        assert_eq!(text.get(spans[0].start..spans[0].end), Some(value));
    }
}

#[test]
fn natural_premise_correction_invalidates_only_exact_scope_and_dependents() {
    let scratch = Scratch::new();
    let mammal = record(&execute(
        &scratch.0,
        "owner",
        &["--input", "coco-mammal", "ココは哺乳類"],
    ));
    let story_mammal = record(&execute(
        &scratch.0,
        "owner",
        &["--input", "coco-story-mammal", "[scope=story]ココは哺乳類"],
    ));
    let gray = record(&execute(
        &scratch.0,
        "owner",
        &["--input", "coco-gray-for-correction", "ココは灰色"],
    ));
    let rule = record(&execute(
        &scratch.0,
        "owner",
        &["--input", "mammal-rule", "もしXは哺乳類ならXは動物"],
    ));
    let derived = record(&execute(
        &scratch.0,
        "owner",
        &[
            "--input",
            "coco-animal-before-correction",
            "ココは動物ですか",
        ],
    ));
    assert_eq!(derived["guarantee"], "DERIVED");

    let old_id = mammal["dependencies"][0].as_str().unwrap();
    let rule_id = rule["dependencies"][0].as_str().unwrap();
    assert!(
        derived["dependencies"]
            .as_array()
            .unwrap()
            .contains(&Value::String(old_id.into()))
    );
    assert!(
        derived["dependencies"]
            .as_array()
            .unwrap()
            .contains(&Value::String(rule_id.into()))
    );
    let correction = record(&execute(
        &scratch.0,
        "owner",
        &[
            "--input",
            "coco-natural-correction",
            "訂正: ココは哺乳類ではなく鳥です",
        ],
    ));
    assert_eq!(correction["lifecycle"], "COMMIT");
    assert_eq!(correction["guarantee"], "USER_PREMISE");
    assert!(
        correction["text"]
            .as_str()
            .unwrap()
            .contains("現実世界の真偽を検証したものではありません")
    );
    assert!(
        correction["dependencies"]
            .as_array()
            .unwrap()
            .contains(&Value::String(old_id.into()))
    );
    assert!(
        !correction["dependencies"]
            .as_array()
            .unwrap()
            .contains(&Value::String(rule_id.into()))
    );
    assert!(
        !correction["dependencies"]
            .as_array()
            .unwrap()
            .contains(&story_mammal["dependencies"][0])
    );
    assert!(
        !correction["dependencies"]
            .as_array()
            .unwrap()
            .contains(&gray["dependencies"][0])
    );

    let contradicted = record(&execute(
        &scratch.0,
        "owner",
        &[
            "--input",
            "coco-mammal-after-correction",
            "ココは哺乳類ですか",
        ],
    ));
    assert_eq!(contradicted["lifecycle"], "COMMIT");
    assert_eq!(contradicted["guarantee"], "USER_PREMISE");
    assert!(
        contradicted["text"]
            .as_str()
            .unwrap()
            .contains("ココは哺乳類ではない")
    );

    stopped(
        &execute(
            &scratch.0,
            "owner",
            &[
                "--input",
                "coco-animal-after-correction",
                "ココは動物ですか",
            ],
        ),
        2,
        "SUSPEND",
    );

    // A smaller sealed allowance must stop the entire inventory, even if
    // some of its shapes have already been evaluated.
    let budget_limited = stopped(
        &execute(
            &scratch.0,
            "owner",
            &[
                "--input",
                "coco-after-correction-list-low-budget",
                "--max-actions",
                "20",
                "ココとは何ですか",
            ],
        ),
        2,
        "SUSPEND",
    );
    assert!(
        budget_limited["reason"]
            .as_str()
            .unwrap()
            .starts_with("RESOURCE_LIMIT:")
    );
    assert!(budget_limited["content_plan"].is_null());
    assert_eq!(budget_limited["receipts"], serde_json::json!([]));
    let listing = record(&execute(
        &scratch.0,
        "owner",
        &["--input", "coco-after-correction-list", "ココとは何ですか"],
    ));
    assert_eq!(listing["lifecycle"], "COMMIT");
    assert_eq!(listing["guarantee"], "USER_PREMISE");
    let listing_text = listing["text"].as_str().unwrap();
    assert!(listing_text.contains("ココは鳥である"));
    assert!(listing_text.contains("ココは哺乳類ではない"));
    assert!(listing_text.contains("ココは哺乳類である（範囲: story"));
    assert!(listing_text.contains("ココは灰色である"));
    assert!(
        !listing["dependencies"]
            .as_array()
            .unwrap()
            .contains(&Value::String(old_id.into()))
    );
    assert!(
        listing["dependencies"]
            .as_array()
            .unwrap()
            .contains(&story_mammal["dependencies"][0])
    );
    assert!(
        listing["dependencies"]
            .as_array()
            .unwrap()
            .contains(&gray["dependencies"][0])
    );

    let scope_mismatch = stopped(
        &execute(
            &scratch.0,
            "owner",
            &[
                "--input",
                "coco-default-scope-mismatch",
                "ココは哺乳類ではなく魚です",
            ],
        ),
        2,
        "SUSPEND",
    );
    assert!(
        scope_mismatch["reason"]
            .as_str()
            .unwrap()
            .contains("MISSING_CORRECTION_TARGET")
    );
    let after_scope_mismatch = record(&execute(
        &scratch.0,
        "owner",
        &["--input", "coco-after-scope-mismatch", "ココとは何ですか"],
    ));
    assert_eq!(
        after_scope_mismatch["dependencies"],
        listing["dependencies"]
    );

    let unmatched = stopped(
        &execute(
            &scratch.0,
            "owner",
            &[
                "--input",
                "unknown-correction-target",
                "ミケは哺乳類ではなく猫です",
            ],
        ),
        2,
        "SUSPEND",
    );
    assert!(
        unmatched["reason"]
            .as_str()
            .unwrap()
            .contains("MISSING_CORRECTION_TARGET")
    );
    let other_actor = stopped(
        &execute_in_case(
            &scratch.0,
            "stranger",
            "other-actor-correction-case",
            &[
                "--input",
                "other-actor-correction-target",
                "Coco is a bird, not a mammal",
            ],
        ),
        2,
        "SUSPEND",
    );
    assert!(
        other_actor["reason"]
            .as_str()
            .unwrap()
            .contains("MISSING_CORRECTION_TARGET")
    );

    let english_initial = record(&execute_in_case(
        &scratch.0,
        "owner",
        "english-correction",
        &["--input", "coco-mammal-en", "Coco is a mammal"],
    ));
    let english_correction = record(&execute_in_case(
        &scratch.0,
        "owner",
        "english-correction",
        &[
            "--input",
            "coco-correction-en",
            "Correction: Coco is a bird, not a mammal.",
        ],
    ));
    let english_listing = record(&execute_in_case(
        &scratch.0,
        "owner",
        "english-correction",
        &["--input", "coco-list-after-correction-en", "What is Coco?"],
    ));
    assert_eq!(english_initial["guarantee"], "USER_PREMISE");
    assert_eq!(english_correction["lifecycle"], "COMMIT");
    assert_eq!(english_correction["guarantee"], "USER_PREMISE");
    assert_eq!(english_listing["lifecycle"], "COMMIT");
    assert!(
        english_listing["text"]
            .as_str()
            .unwrap()
            .contains("Cocoはbirdである")
    );
    assert!(
        english_listing["text"]
            .as_str()
            .unwrap()
            .contains("Cocoはmammalではない")
    );
    assert!(
        !english_listing["dependencies"]
            .as_array()
            .unwrap()
            .contains(&english_initial["dependencies"][0])
    );
}

#[test]
fn contextual_natural_correction_uses_only_unique_prior_direct_premise() {
    let scratch = Scratch::new();
    let premise = record(&execute_in_case(
        &scratch.0,
        "context-owner",
        "context-correction",
        &["--input", "coco-mammal", "Coco is a mammal"],
    ));
    let prior_answer = record(&execute_in_case(
        &scratch.0,
        "context-owner",
        "context-correction",
        &["--input", "ask-coco-mammal", "Is Coco a mammal?"],
    ));
    let old_id = premise["dependencies"][0].as_str().unwrap();
    assert!(
        prior_answer["dependencies"]
            .as_array()
            .unwrap()
            .contains(&Value::String(old_id.into()))
    );
    let correction = record(&execute_in_case(
        &scratch.0,
        "context-owner",
        "context-correction",
        &["--input", "it-is-a-bird", "It is a bird, not a mammal."],
    ));
    assert_eq!(correction["lifecycle"], "COMMIT");
    assert_eq!(correction["guarantee"], "USER_PREMISE");
    assert!(
        correction["text"]
            .as_str()
            .unwrap()
            .contains("直前の回答が参照した提示前提ID")
    );
    assert!(
        correction["dependencies"]
            .as_array()
            .unwrap()
            .contains(&Value::String(old_id.into()))
    );
    let listing = record(&execute_in_case(
        &scratch.0,
        "context-owner",
        "context-correction",
        &["--input", "list-coco", "What is Coco?"],
    ));
    assert!(
        listing["text"]
            .as_str()
            .unwrap()
            .contains("Cocoはbirdである")
    );
    assert!(
        listing["text"]
            .as_str()
            .unwrap()
            .contains("Cocoはmammalではない")
    );
    assert!(
        !listing["dependencies"]
            .as_array()
            .unwrap()
            .contains(&Value::String(old_id.into()))
    );

    let no_case_context = stopped(
        &execute_in_case(
            &scratch.0,
            "context-owner",
            "no-context-correction",
            &[
                "--input",
                "pronoun-without-prior",
                "It is a fish, not a mammal.",
            ],
        ),
        2,
        "SUSPEND",
    );
    assert!(
        no_case_context["reason"]
            .as_str()
            .unwrap()
            .contains("MISSING_CORRECTION_CONTEXT")
    );
    let still_listed = record(&execute_in_case(
        &scratch.0,
        "context-owner",
        "context-correction",
        &["--input", "list-after-cross-case-stop", "What is Coco?"],
    ));
    assert_eq!(still_listed["dependencies"], listing["dependencies"]);

    let derived_premise = record(&execute_in_case(
        &scratch.0,
        "derived-owner",
        "derived-correction",
        &["--input", "coco-mammal-derived", "Coco is a mammal"],
    ));
    let rule = record(&execute_in_case(
        &scratch.0,
        "derived-owner",
        "derived-correction",
        &[
            "--input",
            "mammal-implies-animal",
            "If X is a mammal then X is an animal",
        ],
    ));
    let derived_answer = record(&execute_in_case(
        &scratch.0,
        "derived-owner",
        "derived-correction",
        &["--input", "ask-coco-animal", "Is Coco an animal?"],
    ));
    assert_eq!(derived_answer["guarantee"], "DERIVED");
    assert!(
        derived_answer["dependencies"]
            .as_array()
            .unwrap()
            .contains(&derived_premise["dependencies"][0])
    );
    assert!(
        derived_answer["dependencies"]
            .as_array()
            .unwrap()
            .contains(&rule["dependencies"][0])
    );
    let derived_correction = stopped(
        &execute_in_case(
            &scratch.0,
            "derived-owner",
            "derived-correction",
            &[
                "--input",
                "do-not-correct-derived-only",
                "It is a fish, not an animal.",
            ],
        ),
        2,
        "SUSPEND",
    );
    assert!(
        derived_correction["reason"]
            .as_str()
            .unwrap()
            .contains("MISSING_CORRECTION_TARGET")
    );
    let still_mammal = record(&execute_in_case(
        &scratch.0,
        "derived-owner",
        "derived-correction",
        &["--input", "verify-original-premise", "Is Coco a mammal?"],
    ));
    assert_eq!(still_mammal["guarantee"], "USER_PREMISE");
    assert!(
        still_mammal["dependencies"]
            .as_array()
            .unwrap()
            .contains(&derived_premise["dependencies"][0])
    );

    record(&execute_in_case(
        &scratch.0,
        "scope-owner",
        "ambiguous-context-correction",
        &["--input", "default-mammal", "ココは哺乳類"],
    ));
    record(&execute_in_case(
        &scratch.0,
        "scope-owner",
        "ambiguous-context-correction",
        &["--input", "story-mammal", "[scope=story]ココは哺乳類"],
    ));
    let two_scopes = record(&execute_in_case(
        &scratch.0,
        "scope-owner",
        "ambiguous-context-correction",
        &["--input", "list-two-scopes", "ココとは何ですか"],
    ));
    let ambiguous = stopped(
        &execute_in_case(
            &scratch.0,
            "scope-owner",
            "ambiguous-context-correction",
            &["--input", "ambiguous-pronoun", "それは哺乳類ではなく鳥です"],
        ),
        2,
        "SUSPEND",
    );
    assert!(
        ambiguous["reason"]
            .as_str()
            .unwrap()
            .contains("AMBIGUOUS_CORRECTION_CONTEXT")
    );
    let after_ambiguous = record(&execute_in_case(
        &scratch.0,
        "scope-owner",
        "ambiguous-context-correction",
        &["--input", "relist-two-scopes", "ココとは何ですか"],
    ));
    assert_eq!(after_ambiguous["dependencies"], two_scopes["dependencies"]);
    let scoped_correction = record(&execute_in_case(
        &scratch.0,
        "scope-owner",
        "ambiguous-context-correction",
        &[
            "--input",
            "scope-anchored-pronoun",
            "[scope=story]それは哺乳類ではなく鳥です",
        ],
    ));
    assert_eq!(scoped_correction["lifecycle"], "COMMIT");
    let scoped_listing = record(&execute_in_case(
        &scratch.0,
        "scope-owner",
        "ambiguous-context-correction",
        &["--input", "list-after-scoped-pronoun", "ココとは何ですか"],
    ));
    let scoped_text = scoped_listing["text"].as_str().unwrap();
    assert!(scoped_text.contains("ココは哺乳類である"));
    assert!(scoped_text.contains("ココは哺乳類ではない（範囲: story"));
    assert!(scoped_text.contains("ココは鳥である（範囲: story"));
}

#[test]
fn relation_answer_follow_up_explains_the_same_current_proof_after_restart() {
    let scratch = Scratch::new();
    let rule = record(&execute(
        &scratch.0,
        "owner",
        &[
            "--input",
            "bird-rule-for-explanation",
            "もしXは鳥ならXは動物",
        ],
    ));
    let bird = record(&execute(
        &scratch.0,
        "owner",
        &["--input", "coco-bird-for-explanation", "ココは鳥"],
    ));
    let answer = record(&execute(
        &scratch.0,
        "owner",
        &["--input", "coco-animal-question", "ココは動物ですか"],
    ));
    assert_eq!(answer["guarantee"], "DERIVED");
    let explanation = record(&execute(&scratch.0, "owner", &["詳しく説明して"]));
    assert_eq!(explanation["lifecycle"], "COMMIT");
    assert_eq!(explanation["guarantee"], "DERIVED");
    let explanation_text = explanation["text"].as_str().unwrap();
    assert!(explanation_text.contains("前回の結論: ココは動物である"));
    assert!(explanation_text.contains("直接提示された前提: ココは鳥である"));
    assert!(
        explanation_text.contains("適用規則（範囲: default）: $xは鳥である ならば $xは動物である")
    );
    assert!(explanation_text.contains("現実世界での真偽確認ではありません"));
    assert!(
        explanation["dependencies"]
            .as_array()
            .unwrap()
            .contains(&answer["id"])
    );
    for dependency in answer["dependencies"].as_array().unwrap() {
        assert!(
            explanation["dependencies"]
                .as_array()
                .unwrap()
                .contains(dependency)
        );
    }
    assert!(
        explanation["dependencies"]
            .as_array()
            .unwrap()
            .contains(&rule["dependencies"][0])
    );
    assert!(
        explanation["dependencies"]
            .as_array()
            .unwrap()
            .contains(&bird["dependencies"][0])
    );
    let saved = inspect_normal_case(&scratch.0, "owner");
    assert_eq!(
        saved["bundle"]["meaning"]["parser_version"],
        minidora_engine::input::PARSER_VERSION
    );
    assert_eq!(
        saved["bundle"]["meaning"]["context_result_ref"],
        answer["id"]
    );

    let stale_answer = record(&execute_in_case(
        &scratch.0,
        "owner",
        "stale-explanation",
        &["--input", "stale-coco-question", "ココは動物ですか"],
    ));
    assert_eq!(stale_answer["guarantee"], "DERIVED");
    record(&execute_in_case(
        &scratch.0,
        "owner",
        "rule-invalidation",
        &[
            "--input",
            "invalidate-bird-rule",
            &format!("{}を無効化して", rule["dependencies"][0].as_str().unwrap()),
        ],
    ));
    let stale_explanation = stopped(
        &execute_in_case(
            &scratch.0,
            "owner",
            "stale-explanation",
            &["--input", "stale-coco-explanation", "詳しく説明して"],
        ),
        2,
        "SUSPEND",
    );
    assert!(
        stale_explanation["reason"]
            .as_str()
            .unwrap()
            .contains("CONTEXT_PROOF_CHANGED")
    );

    record(&execute(
        &scratch.0,
        "owner",
        &["--input", "mike-negative-premise", "ミケは鳥ではない"],
    ));
    let contradicted = record(&execute(
        &scratch.0,
        "owner",
        &["--input", "mike-positive-question", "ミケは鳥ですか"],
    ));
    assert_eq!(contradicted["guarantee"], "USER_PREMISE");
    assert!(
        contradicted["text"]
            .as_str()
            .unwrap()
            .contains("ミケは鳥ではない")
    );
    let negative_explanation = record(&execute(&scratch.0, "owner", &["詳しく説明して"]));
    assert_eq!(negative_explanation["lifecycle"], "COMMIT");
    assert_eq!(negative_explanation["guarantee"], "USER_PREMISE");
    assert!(
        negative_explanation["text"]
            .as_str()
            .unwrap()
            .contains("直接提示された前提: ミケは鳥ではない")
    );
    assert!(
        negative_explanation["dependencies"]
            .as_array()
            .unwrap()
            .contains(&contradicted["id"])
    );
}

#[test]
fn natural_why_question_explains_only_current_relation_proofs() {
    let scratch = Scratch::new();
    let rule = record(&execute(
        &scratch.0,
        "owner",
        &["--input", "why-bird-rule", "もしXは鳥ならXは動物"],
    ));
    let bird = record(&execute(
        &scratch.0,
        "owner",
        &["--input", "why-coco-bird", "ココは鳥"],
    ));
    let why = record(&execute(
        &scratch.0,
        "owner",
        &["--input", "why-coco-animal", "どうしてココは動物なのですか"],
    ));
    assert_eq!(why["lifecycle"], "COMMIT");
    assert_eq!(why["guarantee"], "DERIVED");
    let why_text = why["text"].as_str().unwrap();
    assert!(why_text.contains("「ココ」") && why_text.contains("「動物」"));
    assert!(why_text.contains("現実世界の真偽を確認したものではありません"));
    let artifact = assert_why_generation(&scratch.0, "owner", "normal-route", &why);
    assert_eq!(
        artifact["kind"],
        "minidora.core-computation.relation-meaning.v5"
    );
    let meaning = &artifact["relation_meaning_plan"];
    assert_why_proposition(&meaning["conclusion"], "ココ", "動物", "default", false);
    assert_eq!(meaning["answer_status"], "SUPPORTED");
    let proof_fact = assert_why_record(&artifact, "FACT", &bird["dependencies"][0]);
    assert_why_proposition(&proof_fact["proposition"], "ココ", "鳥", "default", false);
    let proof_rule = assert_why_record(&artifact, "RULE", &rule["dependencies"][0]);
    assert_why_proposition(&proof_rule["premises"][0], "$x", "鳥", "default", false);
    assert_why_proposition(&proof_rule["conclusion"], "$x", "動物", "default", false);

    let saved = inspect_normal_case(&scratch.0, "owner");
    assert_eq!(
        saved["bundle"]["meaning"]["parser_version"],
        "declarative-ja-en-v90-explanation-source-roles"
    );
    let store = minidora_engine::store::Store::open(&scratch.0).unwrap();
    let goal = sequence_artifact(&store, &saved["goal_sha256"]);
    assert_eq!(
        goal["required_predicates"],
        serde_json::json!([
            "RELATION_PROOF_EXPLANATION_V2",
            "SUPPORTED_CONTENT",
            "EXACT_RENDER"
        ])
    );
    assert_eq!(goal["intent"]["proposition"], meaning["asked"]);

    let english_rule = record(&execute(
        &scratch.0,
        "owner",
        &[
            "--input",
            "why-english-rule",
            "If X is a bird then X is an animal",
        ],
    ));
    let english_premise = record(&execute(
        &scratch.0,
        "owner",
        &["--input", "why-english-premise", "Coco is a bird"],
    ));
    let english = record(&execute(
        &scratch.0,
        "owner",
        &["--input", "why-coco-animal-en", "Why is Coco an animal?"],
    ));
    assert_eq!(english["guarantee"], "DERIVED");
    let artifact = assert_why_generation(&scratch.0, "owner", "normal-route", &english);
    assert_why_proposition(
        &artifact["relation_meaning_plan"]["conclusion"],
        "Coco",
        "animal",
        "default",
        false,
    );
    let fact = assert_why_record(&artifact, "FACT", &english_premise["dependencies"][0]);
    assert_why_proposition(&fact["proposition"], "Coco", "bird", "default", false);
    assert_why_record(&artifact, "RULE", &english_rule["dependencies"][0]);

    let missing = stopped(
        &execute(
            &scratch.0,
            "owner",
            &["--input", "why-missing", "どうしてミケは鳥ですか"],
        ),
        2,
        "SUSPEND",
    );
    assert!(
        missing["reason"]
            .as_str()
            .unwrap()
            .contains("MISSING_PREMISES")
    );
    assert!(missing["content_plan"].is_null());
    assert!(missing["render"].is_null());

    let negative = record(&execute(
        &scratch.0,
        "owner",
        &["--input", "why-mike-negative", "ミケは鳥ではない"],
    ));
    let refuted_why = record(&execute(
        &scratch.0,
        "owner",
        &["--input", "why-mike-bird", "なぜミケは鳥ですか"],
    ));
    assert_eq!(refuted_why["lifecycle"], "COMMIT");
    assert_eq!(refuted_why["guarantee"], "USER_PREMISE");
    let artifact = assert_why_generation(&scratch.0, "owner", "normal-route", &refuted_why);
    let meaning = &artifact["relation_meaning_plan"];
    assert_eq!(meaning["answer_status"], "CONTRADICTED");
    assert_why_proposition(&meaning["asked"], "ミケ", "鳥", "default", false);
    assert_why_proposition(&meaning["conclusion"], "ミケ", "鳥", "default", true);
    let fact = assert_why_record(&artifact, "FACT", &negative["dependencies"][0]);
    assert_why_proposition(&fact["proposition"], "ミケ", "鳥", "default", true);
    assert!(
        refuted_why["text"]
            .as_str()
            .unwrap()
            .contains("は支持されず")
    );
    record(&execute(
        &scratch.0,
        "owner",
        &["--input", "why-mike-positive", "ミケは鳥"],
    ));
    let conflict = stopped(
        &execute(
            &scratch.0,
            "owner",
            &["--input", "why-mike-conflict", "なぜミケは鳥ですか"],
        ),
        2,
        "SUSPEND",
    );
    assert!(
        conflict["reason"]
            .as_str()
            .unwrap()
            .contains("CONFLICTING_PREMISES")
    );
    assert!(conflict["content_plan"].is_null());
    assert!(conflict["render"].is_null());
}

#[test]
fn ordinary_why_generates_a_long_premise_without_losing_its_value() {
    let scratch = Scratch::new();
    let object = "灰".repeat(150);
    let premise = record(&execute(
        &scratch.0,
        "owner",
        &["--input", "long-premise", &format!("ミケは{object}")],
    ));
    let why = record(&execute(
        &scratch.0,
        "owner",
        &[
            "--input",
            "long-premise-why",
            &format!("なぜミケは{object}ですか"),
        ],
    ));
    assert_eq!(why["guarantee"], "USER_PREMISE");
    let artifact = assert_why_generation(&scratch.0, "owner", "normal-route", &why);
    let fact = assert_why_record(&artifact, "FACT", &premise["dependencies"][0]);
    assert_why_proposition(&fact["proposition"], "ミケ", &object, "default", false);
    assert!(
        why["text"]
            .as_str()
            .unwrap()
            .contains(&format!("「{object}」"))
    );
    assert!(
        artifact["content_generation"]["units"]
            .as_array()
            .unwrap()
            .iter()
            .any(|unit| {
                unit["text"].as_str().unwrap().len() > 384
                    && unit["text"].as_str().unwrap().contains(&object)
            })
    );
}

#[test]
fn elliptical_why_questions_explain_the_previous_committed_result() {
    let scratch = Scratch::new();

    let direct_actor = "elliptical-why-direct";
    let direct_case = "elliptical-why-direct-case";
    let direct = record(&execute_in_case(
        &scratch.0,
        direct_actor,
        direct_case,
        &["--input", "robot-notebook", "ロボットはノートを使う"],
    ));
    let direct_answer = record(&execute_in_case(
        &scratch.0,
        direct_actor,
        direct_case,
        &[
            "--input",
            "robot-notebook-question",
            "ロボットはノートを使いますか",
        ],
    ));
    assert_eq!(direct_answer["lifecycle"], "COMMIT");
    assert!(
        direct_answer["dependencies"]
            .as_array()
            .unwrap()
            .contains(&direct["dependencies"][0])
    );
    let direct_why = record(&execute_in_case(
        &scratch.0,
        direct_actor,
        direct_case,
        &["--input", "why-direct-answer", "なぜですか"],
    ));
    assert_eq!(direct_why["lifecycle"], "COMMIT");
    assert_eq!(direct_why["guarantee"], "USER_PREMISE");
    assert!(
        direct_why["text"]
            .as_str()
            .unwrap()
            .contains("前回の結論: ロボットはノートを使う")
    );
    assert!(
        direct_why["text"]
            .as_str()
            .unwrap()
            .contains("直接提示された前提: ロボットはノートを使う")
    );
    assert!(
        direct_why["dependencies"]
            .as_array()
            .unwrap()
            .contains(&direct_answer["id"])
    );

    let derived_actor = "elliptical-why-derived";
    let derived_case = "elliptical-why-derived-case";
    let rule = record(&execute_in_case(
        &scratch.0,
        derived_actor,
        derived_case,
        &[
            "--input",
            "robot-tool-rule",
            "もしXはノートを使うならXは道具を使う",
        ],
    ));
    let notebook = record(&execute_in_case(
        &scratch.0,
        derived_actor,
        derived_case,
        &[
            "--input",
            "robot-notebook-premise",
            "ロボットはノートを使う",
        ],
    ));
    let derived_answer = record(&execute_in_case(
        &scratch.0,
        derived_actor,
        derived_case,
        &[
            "--input",
            "robot-tool-question",
            "ロボットは道具を使いますか",
        ],
    ));
    assert_eq!(derived_answer["guarantee"], "DERIVED");
    let derived_why = record(&execute_in_case(
        &scratch.0,
        derived_actor,
        derived_case,
        &["--input", "why-derived-answer", "どうして？"],
    ));
    assert_eq!(derived_why["lifecycle"], "COMMIT");
    assert_eq!(derived_why["guarantee"], "DERIVED");
    assert!(
        derived_why["text"]
            .as_str()
            .unwrap()
            .contains("前回の結論: ロボットは道具を使う")
    );
    assert!(
        derived_why["text"]
            .as_str()
            .unwrap()
            .contains("直接提示された前提: ロボットはノートを使う")
    );
    assert!(
        derived_why["text"]
            .as_str()
            .unwrap()
            .contains("適用規則（範囲: default）:")
    );
    assert!(
        derived_why["dependencies"]
            .as_array()
            .unwrap()
            .contains(&derived_answer["id"])
    );
    assert!(
        derived_why["text"]
            .as_str()
            .unwrap()
            .contains(rule["dependencies"][0].as_str().unwrap())
    );
    assert!(
        derived_why["text"]
            .as_str()
            .unwrap()
            .contains(notebook["dependencies"][0].as_str().unwrap())
    );

    let refuted_actor = "elliptical-why-refuted";
    let refuted_case = "elliptical-why-refuted-case";
    let negative = record(&execute_in_case(
        &scratch.0,
        refuted_actor,
        refuted_case,
        &["--input", "robot-not-poison", "ロボットは毒を使わない"],
    ));
    let refuted_answer = record(&execute_in_case(
        &scratch.0,
        refuted_actor,
        refuted_case,
        &[
            "--input",
            "robot-poison-question",
            "ロボットは毒を使いますか",
        ],
    ));
    assert!(
        refuted_answer["text"]
            .as_str()
            .unwrap()
            .contains("ロボットは毒を使わない")
    );
    let refuted_why = record(&execute_in_case(
        &scratch.0,
        refuted_actor,
        refuted_case,
        &["--input", "why-refuted-answer", "Why?"],
    ));
    assert_eq!(refuted_why["lifecycle"], "COMMIT");
    assert!(
        refuted_why["text"]
            .as_str()
            .unwrap()
            .contains("直接提示された前提: ロボットは毒を使わない")
    );
    assert!(
        refuted_why["dependencies"]
            .as_array()
            .unwrap()
            .contains(&refuted_answer["id"])
    );
    assert!(
        refuted_why["text"]
            .as_str()
            .unwrap()
            .contains(negative["dependencies"][0].as_str().unwrap())
    );

    let calculation_actor = "elliptical-why-calculation";
    let calculation_case = "elliptical-why-calculation-case";
    let calculation = record(&execute_in_case(
        &scratch.0,
        calculation_actor,
        calculation_case,
        &["--input", "sum", "2+3"],
    ));
    let calculation_why = record(&execute_in_case(
        &scratch.0,
        calculation_actor,
        calculation_case,
        &["--input", "why-sum", "Why?"],
    ));
    assert_eq!(calculation_why["lifecycle"], "COMMIT");
    assert!(
        calculation_why["text"]
            .as_str()
            .unwrap()
            .contains("2 + 3 = 5")
    );
    assert!(
        calculation_why["dependencies"]
            .as_array()
            .unwrap()
            .contains(&calculation["id"])
    );

    let missing = stopped(
        &execute_in_case(
            &scratch.0,
            direct_actor,
            "elliptical-why-no-prior-case",
            &["--input", "why-without-prior", "なぜですか"],
        ),
        2,
        "SUSPEND",
    );
    assert!(
        missing["reason"]
            .as_str()
            .unwrap()
            .contains("MISSING_ACTUAL_RESULT")
    );
    let other_actor = stopped(
        &execute_in_case(
            &scratch.0,
            "elliptical-why-other-actor",
            direct_case,
            &["--input", "why-other-actor", "なぜですか"],
        ),
        3,
        "REJECT",
    );
    assert!(
        other_actor["reason"]
            .as_str()
            .unwrap()
            .contains("CASE_ACCESS_DENIED")
    );
}

#[test]
fn natural_common_relation_questions_compare_two_subjects_using_shared_proofs() {
    let scratch = Scratch::new();

    let direct_actor = "common-relation-direct";
    let direct_case = "common-relation-direct-case";
    let coco_bird = record(&execute_in_case(
        &scratch.0,
        direct_actor,
        direct_case,
        &["--input", "coco-bird", "ココは鳥"],
    ));
    let mike_bird = record(&execute_in_case(
        &scratch.0,
        direct_actor,
        direct_case,
        &["--input", "mike-bird", "ミケは鳥"],
    ));
    let coco_feathers = record(&execute_in_case(
        &scratch.0,
        direct_actor,
        direct_case,
        &["--input", "coco-feathers", "ココは羽毛持ち"],
    ));
    let direct_common = record(&execute_in_case(
        &scratch.0,
        direct_actor,
        direct_case,
        &["--input", "common-ja", "ココとミケの共通点は何ですか"],
    ));
    assert_eq!(direct_common["lifecycle"], "COMMIT");
    assert_eq!(direct_common["guarantee"], "USER_PREMISE");
    let direct_text = direct_common["text"].as_str().unwrap();
    assert!(direct_text.contains("ココは鳥である"));
    assert!(direct_text.contains("ミケは鳥である"));
    assert!(!direct_text.contains("羽毛持ち"));
    assert!(direct_text.contains("網羅的な比較でも"));
    for premise in [&coco_bird["dependencies"][0], &mike_bird["dependencies"][0]] {
        assert!(
            direct_common["dependencies"]
                .as_array()
                .unwrap()
                .contains(premise)
        );
    }
    assert!(
        !direct_common["dependencies"]
            .as_array()
            .unwrap()
            .contains(&coco_feathers["dependencies"][0])
    );
    let direct_explanation = record(&execute_in_case(
        &scratch.0,
        direct_actor,
        direct_case,
        &["--input", "common-ja-explanation", "詳しく説明して"],
    ));
    assert_eq!(direct_explanation["lifecycle"], "COMMIT");
    assert_eq!(direct_explanation["guarantee"], "USER_PREMISE");
    let direct_explanation_text = direct_explanation["text"].as_str().unwrap();
    assert_ne!(direct_explanation_text, direct_text);
    for marker in [
        "前回比較の詳細説明",
        direct_common["id"].as_str().unwrap(),
        "ココ側の証明: ココは鳥である",
        "ミケ側の証明: ミケは鳥である",
        coco_bird["dependencies"][0].as_str().unwrap(),
        mike_bird["dependencies"][0].as_str().unwrap(),
    ] {
        assert!(direct_explanation_text.contains(marker), "missing {marker}");
    }
    for dependency in direct_common["dependencies"].as_array().unwrap() {
        assert!(
            direct_explanation["dependencies"]
                .as_array()
                .unwrap()
                .contains(dependency),
            "detailed comparison explanation lost {dependency}"
        );
    }

    let derived_actor = "common-relation-derived";
    let derived_case = "common-relation-derived-case";
    let rule = record(&execute_in_case(
        &scratch.0,
        derived_actor,
        derived_case,
        &["--input", "bird-animal-rule", "もしXは鳥ならXは動物"],
    ));
    let derived_coco = record(&execute_in_case(
        &scratch.0,
        derived_actor,
        derived_case,
        &["--input", "derived-coco-bird", "ココは鳥"],
    ));
    let derived_mike = record(&execute_in_case(
        &scratch.0,
        derived_actor,
        derived_case,
        &["--input", "derived-mike-bird", "ミケは鳥"],
    ));
    let derived_common = record(&execute_in_case(
        &scratch.0,
        derived_actor,
        derived_case,
        &[
            "--input",
            "derived-common-ja",
            "ココとミケの共通点は何ですか",
        ],
    ));
    assert_eq!(derived_common["lifecycle"], "COMMIT");
    assert_eq!(derived_common["guarantee"], "DERIVED");
    let derived_text = derived_common["text"].as_str().unwrap();
    assert!(derived_text.contains("ココは動物である"));
    assert!(derived_text.contains("ミケは動物である"));
    assert!(derived_text.contains("適用規則（範囲: default）:"));
    for dependency in [
        &rule["dependencies"][0],
        &derived_coco["dependencies"][0],
        &derived_mike["dependencies"][0],
    ] {
        assert!(
            derived_common["dependencies"]
                .as_array()
                .unwrap()
                .contains(dependency)
        );
    }
    let derived_explanation = record(&execute_in_case(
        &scratch.0,
        derived_actor,
        derived_case,
        &["--input", "derived-common-explanation", "詳しく説明して"],
    ));
    assert_eq!(derived_explanation["lifecycle"], "COMMIT");
    assert_eq!(derived_explanation["guarantee"], "DERIVED");
    let derived_explanation_text = derived_explanation["text"].as_str().unwrap();
    assert_ne!(derived_explanation_text, derived_text);
    for marker in [
        "ココ側の証明: ココは動物である",
        "ミケ側の証明: ミケは動物である",
        "適用規則（範囲: default）:",
        rule["dependencies"][0].as_str().unwrap(),
        derived_coco["dependencies"][0].as_str().unwrap(),
        derived_mike["dependencies"][0].as_str().unwrap(),
    ] {
        assert!(
            derived_explanation_text.contains(marker),
            "missing {marker}"
        );
    }

    let stale_actor = "common-relation-stale";
    let stale_case = "common-relation-stale-case";
    let stale_fact = record(&execute_in_case(
        &scratch.0,
        stale_actor,
        stale_case,
        &["--input", "stale-coco-bird", "ココは鳥"],
    ));
    record(&execute_in_case(
        &scratch.0,
        stale_actor,
        stale_case,
        &["--input", "stale-mike-bird", "ミケは鳥"],
    ));
    record(&execute_in_case(
        &scratch.0,
        stale_actor,
        stale_case,
        &["--input", "stale-common", "ココとミケの共通点は何ですか"],
    ));
    record(&execute_in_case(
        &scratch.0,
        stale_actor,
        "common-relation-stale-invalidation",
        &[
            "--input",
            "stale-invalidate-coco",
            &format!(
                "{}を無効化して",
                stale_fact["dependencies"][0].as_str().unwrap()
            ),
        ],
    ));
    let stale_explanation = stopped(
        &execute_in_case(
            &scratch.0,
            stale_actor,
            stale_case,
            &["--input", "stale-common-explanation", "詳しく説明して"],
        ),
        2,
        "SUSPEND",
    );
    assert!(
        stale_explanation["reason"]
            .as_str()
            .unwrap()
            .contains("CONTEXT_PROOF_CHANGED")
    );

    let english_actor = "common-relation-english";
    let english_case = "common-relation-english-case";
    record(&execute_in_case(
        &scratch.0,
        english_actor,
        english_case,
        &["--input", "coco-bird-en", "Coco is a bird"],
    ));
    record(&execute_in_case(
        &scratch.0,
        english_actor,
        english_case,
        &["--input", "mike-bird-en", "Mike is a bird"],
    ));
    let english_common = record(&execute_in_case(
        &scratch.0,
        english_actor,
        english_case,
        &[
            "--input",
            "common-en",
            "What do Coco and Mike have in common?",
        ],
    ));
    assert_eq!(english_common["lifecycle"], "COMMIT");
    assert!(
        english_common["text"]
            .as_str()
            .unwrap()
            .contains("Cocoはbirdである")
    );
    assert!(
        english_common["text"]
            .as_str()
            .unwrap()
            .contains("Mikeはbirdである")
    );

    let negative_actor = "common-relation-negative";
    let negative_case = "common-relation-negative-case";
    let coco_negative = record(&execute_in_case(
        &scratch.0,
        negative_actor,
        negative_case,
        &["--input", "coco-not-mammal", "ココは哺乳類ではない"],
    ));
    let mike_negative = record(&execute_in_case(
        &scratch.0,
        negative_actor,
        negative_case,
        &["--input", "mike-not-mammal", "ミケは哺乳類ではない"],
    ));
    let negative_common = record(&execute_in_case(
        &scratch.0,
        negative_actor,
        negative_case,
        &["--input", "negative-common", "ココとミケの共通点は何ですか"],
    ));
    assert_eq!(negative_common["lifecycle"], "COMMIT");
    assert!(
        negative_common["text"]
            .as_str()
            .unwrap()
            .contains("ココは哺乳類ではない")
    );
    assert!(
        negative_common["text"]
            .as_str()
            .unwrap()
            .contains("ミケは哺乳類ではない")
    );
    for premise in [
        &coco_negative["dependencies"][0],
        &mike_negative["dependencies"][0],
    ] {
        assert!(
            negative_common["dependencies"]
                .as_array()
                .unwrap()
                .contains(premise)
        );
    }

    let missing_actor = "common-relation-missing";
    let missing_case = "common-relation-missing-case";
    record(&execute_in_case(
        &scratch.0,
        missing_actor,
        missing_case,
        &["--input", "coco-only-bird", "ココは鳥"],
    ));
    record(&execute_in_case(
        &scratch.0,
        missing_actor,
        missing_case,
        &["--input", "mike-not-bird", "ミケは鳥ではない"],
    ));
    let missing = stopped(
        &execute_in_case(
            &scratch.0,
            missing_actor,
            missing_case,
            &["--input", "no-common-proof", "ココとミケの共通点は何ですか"],
        ),
        2,
        "SUSPEND",
    );
    assert!(
        missing["reason"]
            .as_str()
            .unwrap()
            .contains("MISSING_PREMISES")
    );
    assert!(
        !missing["text"]
            .as_str()
            .unwrap()
            .contains("共通点はありません")
    );

    let scope_actor = "common-relation-scope";
    let scope_case = "common-relation-scope-case";
    record(&execute_in_case(
        &scratch.0,
        scope_actor,
        scope_case,
        &["--input", "coco-biology", "[scope=biology]ココは鳥"],
    ));
    record(&execute_in_case(
        &scratch.0,
        scope_actor,
        scope_case,
        &["--input", "mike-default", "ミケは鳥"],
    ));
    let wrong_scope = stopped(
        &execute_in_case(
            &scratch.0,
            scope_actor,
            scope_case,
            &[
                "--input",
                "biology-common",
                "[scope=biology]ココとミケの共通点は何ですか",
            ],
        ),
        2,
        "SUSPEND",
    );
    assert!(
        wrong_scope["reason"]
            .as_str()
            .unwrap()
            .contains("MISSING_PREMISES")
    );

    let conflict_actor = "common-relation-conflict";
    let conflict_case = "common-relation-conflict-case";
    record(&execute_in_case(
        &scratch.0,
        conflict_actor,
        conflict_case,
        &["--input", "coco-bird", "ココは鳥"],
    ));
    record(&execute_in_case(
        &scratch.0,
        conflict_actor,
        conflict_case,
        &["--input", "coco-not-bird", "ココは鳥ではない"],
    ));
    record(&execute_in_case(
        &scratch.0,
        conflict_actor,
        conflict_case,
        &["--input", "mike-bird", "ミケは鳥"],
    ));
    let conflict = stopped(
        &execute_in_case(
            &scratch.0,
            conflict_actor,
            conflict_case,
            &[
                "--input",
                "conflicting-common",
                "ココとミケの共通点は何ですか",
            ],
        ),
        2,
        "SUSPEND",
    );
    assert!(
        conflict["reason"]
            .as_str()
            .unwrap()
            .contains("CONFLICTING_PREMISES")
    );

    let actor_isolated = stopped(
        &execute_in_case(
            &scratch.0,
            "common-relation-other-actor",
            "common-relation-other-actor-case",
            &["--input", "no-actor-memory", "ココとミケの共通点は何ですか"],
        ),
        2,
        "SUSPEND",
    );
    assert!(
        actor_isolated["reason"]
            .as_str()
            .unwrap()
            .contains("MISSING_PREMISES")
    );
}

#[test]
fn natural_relation_comparison_consumes_saved_proofs_and_flows_into_previous_summary() {
    use std::collections::BTreeSet;

    let scratch = Scratch::new();
    let actor = "relation-comparison-owner";
    let case = "relation-comparison-case";
    let rule = record(&execute_in_case(
        &scratch.0,
        actor,
        case,
        &[
            "--input",
            "biology-rule",
            "もし[scope=biology]Xは鳥なら[scope=biology]Xは動物",
        ],
    ));
    let coco_bird = record(&execute_in_case(
        &scratch.0,
        actor,
        case,
        &["--input", "coco-bird", "[scope=biology]ココは鳥"],
    ));
    let mike_bird = record(&execute_in_case(
        &scratch.0,
        actor,
        case,
        &["--input", "mike-bird", "[scope=biology]ミケは鳥"],
    ));
    let coco_feathers = record(&execute_in_case(
        &scratch.0,
        actor,
        case,
        &["--input", "coco-feathers", "[scope=biology]ココは羽毛持ち"],
    ));
    let mike_cat = record(&execute_in_case(
        &scratch.0,
        actor,
        case,
        &["--input", "mike-cat", "[scope=biology]ミケは猫"],
    ));
    record(&execute_in_case(
        &scratch.0,
        actor,
        case,
        &["--input", "other-scope", "[scope=fiction]ココは王族"],
    ));

    let comparison = record(&execute_in_case(
        &scratch.0,
        actor,
        case,
        &[
            "--input",
            "compare-relation-inventories",
            "--max-actions",
            "128",
            "[scope=biology]ココとミケの保存済みの関係を比較して",
        ],
    ));
    assert_eq!(comparison["lifecycle"], "COMMIT");
    assert_eq!(comparison["guarantee"], "HYPOTHESIS");
    let comparison_text = comparison["text"].as_str().unwrap();
    assert!(comparison_text.starts_with("・保存済みの関係を左右で比較します。"));
    for marker in [
        "保存記録の網羅性",
        "「ココ」",
        "「ミケ」",
        "「鳥」",
        "「動物」",
        "「羽毛持ち」",
        "「猫」",
        "保存規則からの導出",
    ] {
        assert!(comparison_text.contains(marker), "missing {marker}");
    }
    assert!(!comparison_text.contains("王族"));
    for dependency in [
        &rule["dependencies"][0],
        &coco_bird["dependencies"][0],
        &mike_bird["dependencies"][0],
        &coco_feathers["dependencies"][0],
        &mike_cat["dependencies"][0],
    ] {
        assert!(
            comparison["dependencies"]
                .as_array()
                .unwrap()
                .contains(dependency),
            "comparison lost source dependency {dependency}"
        );
    }

    let comparison_report_ref = comparison["dependencies"][0].as_str().unwrap();
    let store = minidora_engine::store::Store::open(&scratch.0).unwrap();
    let artifact_bytes = store.get_bytes(actor, case, comparison_report_ref).unwrap();
    assert_eq!(
        minidora_domain::digest(&artifact_bytes),
        comparison_report_ref
    );
    let artifact: Value = serde_json::from_slice(&artifact_bytes).unwrap();
    assert_eq!(artifact["kind"], "minidora.relation-comparison-content.v5");
    assert_eq!(artifact["result_id"], comparison["id"]);
    assert_eq!(artifact["scope"], "biology");
    assert_eq!(
        artifact["inventory_a"]["facts"].as_array().unwrap().len(),
        3
    );
    assert_eq!(
        artifact["inventory_b"]["facts"].as_array().unwrap().len(),
        3
    );
    for side in ["inventory_a", "inventory_b"] {
        assert!(
            artifact[side]["facts"]
                .as_array()
                .unwrap()
                .iter()
                .all(|fact| { fact["proposition"]["scope"] == "biology" })
        );
    }
    let meaning_plan = &artifact["meaning_plan"];
    assert!(meaning_plan.get("format").is_none());
    assert_eq!(meaning_plan["subject_a"], "ココ");
    assert_eq!(meaning_plan["subject_b"], "ミケ");
    assert_eq!(meaning_plan["scope"], "biology");
    let rows = meaning_plan["rows"].as_array().unwrap();
    assert_eq!(rows.len(), 4);
    for (class, left, right) in [
        ("COMMON", Some(&coco_bird), Some(&mike_bird)),
        ("LEFT_ONLY", Some(&coco_feathers), None),
        ("RIGHT_ONLY", None, Some(&mike_cat)),
    ] {
        let refs = |record: Option<&Value>| {
            record.map_or_else(Vec::new, |record| vec![record["dependencies"][0].clone()])
        };
        assert!(rows.iter().any(|row| {
            row["class"] == class
                && row["left_fact_ids"] == Value::Array(refs(left))
                && row["right_fact_ids"] == Value::Array(refs(right))
        }));
    }
    assert_eq!(
        rows.iter().filter(|row| row["class"] == "COMMON").count(),
        2
    );

    let generated = &artifact["content_generation"];
    assert_eq!(generated["schema"], "core-meaning-generation-v1");
    assert_eq!(generated["stop_reason"], "EOS");
    assert_eq!(generated["text"], comparison["text"]);
    let generated_proof_ids = generated["evidence_ids"]
        .as_array()
        .unwrap()
        .iter()
        .map(|value| value.as_str().unwrap())
        .collect::<BTreeSet<_>>();
    for expected in [
        meaning_plan["proof_ids"].as_array().unwrap().as_slice(),
        &comparison["dependencies"].as_array().unwrap()[1..],
    ] {
        assert_eq!(
            generated_proof_ids,
            expected
                .iter()
                .map(|value| value.as_str().unwrap())
                .collect::<BTreeSet<_>>()
        );
    }
    let token_steps = generated["token_steps"].as_array().unwrap();
    assert!(token_steps.iter().any(|step| {
        step["distribution"]
            .as_array()
            .is_some_and(|support| support.len() > 1)
    }));
    let plan = &comparison["content_plan"];
    assert_eq!(generated["model_revision"], plan["model_revision"]);
    assert_eq!(generated["model_artifact_ref"], plan["model_artifact_ref"]);
    assert_eq!(plan["text"], comparison["text"]);
    let required_units = generated["units"]
        .as_array()
        .unwrap()
        .iter()
        .map(|unit| unit["text"].clone())
        .collect();
    assert_eq!(plan["required_units"], Value::Array(required_units));
    assert_eq!(comparison["render"]["status"], "RENDERED");
    assert_eq!(
        comparison["render"]["bytes"],
        serde_json::json!(comparison_text.as_bytes())
    );

    let summary = record(&execute_in_case(
        &scratch.0,
        actor,
        case,
        &[
            "--input",
            "summarize-relation-comparison",
            "前回の比較を2文以内で要約して",
        ],
    ));
    assert_eq!(summary["lifecycle"], "COMMIT");
    assert_eq!(summary["guarantee"], "HYPOTHESIS");
    for dependency in
        std::iter::once(&comparison["id"]).chain(comparison["dependencies"].as_array().unwrap())
    {
        assert!(
            summary["dependencies"]
                .as_array()
                .unwrap()
                .contains(dependency),
            "previous-result summary lost relation comparison dependency {dependency}"
        );
    }

    let conflict_case = "relation-comparison-conflict-case";
    let conflict_actor = "relation-comparison-conflict-owner";
    let coco_positive = record(&execute_in_case(
        &scratch.0,
        conflict_actor,
        conflict_case,
        &["--input", "coco-bird-positive", "[scope=biology]ココは鳥"],
    ));
    let coco_negative = record(&execute_in_case(
        &scratch.0,
        conflict_actor,
        conflict_case,
        &[
            "--input",
            "coco-bird-negative",
            "[scope=biology]ココは鳥ではない",
        ],
    ));
    let mike_positive = record(&execute_in_case(
        &scratch.0,
        conflict_actor,
        conflict_case,
        &["--input", "mike-bird-positive", "[scope=biology]ミケは鳥"],
    ));
    let conflicting_comparison = record(&execute_in_case(
        &scratch.0,
        conflict_actor,
        conflict_case,
        &[
            "--input",
            "compare-conflicting-relations",
            "--max-actions",
            "128",
            "[scope=biology]ココとミケの関係を比較して",
        ],
    ));
    assert_eq!(conflicting_comparison["lifecycle"], "COMMIT");
    assert_eq!(conflicting_comparison["guarantee"], "HYPOTHESIS");
    let conflict_text = conflicting_comparison["text"].as_str().unwrap();
    assert!(conflict_text.contains("競合"), "{conflict_text}");
    assert!(
        conflict_text.contains("「ココ」") && conflict_text.contains("「鳥」"),
        "{conflict_text}"
    );
    assert!(
        conflict_text.contains("ではない") || conflict_text.contains("ではありません"),
        "{conflict_text}"
    );
    let conflict_ref = conflicting_comparison["dependencies"][0].as_str().unwrap();
    let conflict_artifact: Value = serde_json::from_slice(
        &store
            .get_bytes(conflict_actor, conflict_case, conflict_ref)
            .unwrap(),
    )
    .unwrap();
    let contested_rows = conflict_artifact["meaning_plan"]["rows"]
        .as_array()
        .unwrap();
    assert_eq!(contested_rows.len(), 2);
    assert!(contested_rows.iter().all(|row| row["class"] == "CONTESTED"));
    assert!(contested_rows.iter().any(|row| {
        row["left_fact_ids"] == serde_json::json!([coco_positive["dependencies"][0]])
            && row["right_fact_ids"] == serde_json::json!([mike_positive["dependencies"][0]])
    }));
    assert!(contested_rows.iter().any(|row| {
        row["left_fact_ids"] == serde_json::json!([coco_negative["dependencies"][0]])
            && row["right_fact_ids"] == serde_json::json!([])
    }));
    for source in [&coco_positive, &coco_negative, &mike_positive] {
        for dependency in source["dependencies"].as_array().unwrap() {
            assert!(
                conflicting_comparison["dependencies"]
                    .as_array()
                    .unwrap()
                    .contains(dependency),
                "comparison lost conflicting premise dependency {dependency}; got {}",
                conflicting_comparison["dependencies"]
            );
        }
    }

    let exhausted = stopped(
        &execute_in_case(
            &scratch.0,
            actor,
            case,
            &[
                "--input",
                "relation-comparison-low-budget",
                "--max-actions",
                "32",
                "[scope=biology]ココとミケの関係を比較して",
            ],
        ),
        2,
        "SUSPEND",
    );
    assert!(
        exhausted["reason"]
            .as_str()
            .unwrap()
            .starts_with("RESOURCE_LIMIT:")
    );
    assert!(exhausted["content_plan"].is_null());
    assert!(exhausted["render"].is_null());

    let no_memory = stopped(
        &execute_in_case(
            &scratch.0,
            "relation-comparison-other-actor",
            "relation-comparison-empty-case",
            &[
                "--input",
                "no-saved-relations",
                "ココとミケの関係を比較して",
            ],
        ),
        2,
        "SUSPEND",
    );
    assert!(
        no_memory["reason"]
            .as_str()
            .unwrap()
            .contains("MISSING_PREMISES")
    );
    assert!(no_memory["dependencies"].as_array().unwrap().is_empty());
}

#[test]
fn relation_comparison_generates_every_axis_beyond_short_explanation_size() {
    let scratch = Scratch::new();
    let actor = "multi-axis-comparison-owner";
    let case = "multi-axis-comparison-case";
    let long_object = "記録".repeat(40);
    for (input, premise) in (0..13)
        .map(|index| {
            (
                format!("left-{index}"),
                format!("[scope=lab]甲は資料{index}"),
            )
        })
        .chain([
            ("left-long".into(), format!("[scope=lab]甲は{long_object}")),
            ("right-long".into(), format!("[scope=lab]乙は{long_object}")),
        ])
    {
        record(&execute_in_case(
            &scratch.0,
            actor,
            case,
            &["--input", &input, &premise],
        ));
    }

    let comparison = record(&execute_in_case(
        &scratch.0,
        actor,
        case,
        &[
            "--input",
            "compare-all",
            "--max-actions",
            "256",
            "[scope=lab]甲と乙の関係を比較して",
        ],
    ));
    let store = minidora_engine::store::Store::open(&scratch.0).unwrap();
    let source_ref = comparison["dependencies"][0].as_str().unwrap();
    let artifact: Value =
        serde_json::from_slice(&store.get_bytes(actor, case, source_ref).unwrap()).unwrap();
    let rows = artifact["meaning_plan"]["rows"].as_array().unwrap();
    assert_eq!(rows.len(), 14);
    assert_eq!(
        rows.iter()
            .filter(|row| row["class"] == "LEFT_ONLY")
            .count(),
        13
    );
    assert_eq!(
        rows.iter().filter(|row| row["class"] == "COMMON").count(),
        1
    );
    let generation = &artifact["content_generation"];
    assert!(generation["units"].as_array().unwrap().len() > 16);
    assert!(generation["units"].as_array().unwrap().iter().any(|unit| {
        let text = unit["text"].as_str().unwrap();
        text.len() > 384 && text.matches(&long_object).count() == 2
    }));
    assert_eq!(generation["text"], comparison["text"]);
}

#[test]
fn requested_comparison_layout_changes_generation_and_keeps_the_same_proofs() {
    let scratch = Scratch::new();
    for (input_id, request) in [
        ("left-common", "[scope=甲]甲は観測器"),
        ("right-common", "[scope=甲]乙は観測器"),
        ("left-only", "[scope=甲]甲は無線式"),
        ("right-only", "[scope=甲]乙は有線式"),
    ] {
        let premise = record(&execute(
            &scratch.0,
            "owner",
            &["--input", input_id, request],
        ));
        assert_eq!(premise["lifecycle"], "COMMIT");
    }
    let store = minidora_engine::store::Store::open(&scratch.0).unwrap();
    let mut observations = Vec::new();
    for (input_id, phrase, format) in [
        ("list", "箇条書き", "BULLETS"),
        ("paragraph", "段落", "PARAGRAPH"),
    ] {
        let request = format!("　[scope=甲]甲と乙の保存済みの関係を{phrase}で比較して。　");
        let path = scratch.0.join(format!("{input_id}.txt"));
        let target = path.to_str().unwrap();
        let result = record(&execute(
            &scratch.0,
            "owner",
            &[
                "--input",
                input_id,
                "--max-actions",
                "128",
                "--output",
                target,
                "--allow-write",
                target,
                &request,
            ],
        ));
        assert_eq!(result["lifecycle"], "COMMIT");
        assert_eq!(result["guarantee"], "HYPOTHESIS");

        let saved = inspect_normal_case(&scratch.0, "owner");
        let meaning = &saved["bundle"]["meaning"];
        assert_eq!(saved["bundle"]["request"]["text"], request);
        assert_eq!(
            meaning["parser_version"],
            "declarative-ja-en-v91-comparison-source-roles"
        );
        assert_eq!(meaning["intent"]["format"], format);
        let source = &meaning["source_roles"]["/CompareRelationInventories/format"][0];
        let span_start = source["start"].as_u64().unwrap() as usize;
        let span_end = source["end"].as_u64().unwrap() as usize;
        let source_text = request.get(span_start..span_end).unwrap();
        assert_eq!(source["text"], source_text);
        assert_eq!(source_text, phrase);
        for (role, start) in [
            ("scope", request.find('甲').unwrap()),
            ("subject_a", request.rfind('甲').unwrap()),
        ] {
            let occurrences = meaning["source_roles"]
                [format!("/CompareRelationInventories/{role}")]
            .as_array()
            .unwrap();
            assert_eq!(occurrences.len(), 1);
            assert_eq!(occurrences[0]["start"], start);
            assert_eq!(occurrences[0]["text"], "甲");
        }
        let goal = sequence_artifact(&store, &saved["goal_sha256"]);
        assert_eq!(goal["intent"], meaning["intent"]);

        let artifact = sequence_artifact(&store, &result["dependencies"][0]);
        assert_eq!(artifact["kind"], "minidora.relation-comparison-content.v6");
        assert_eq!(artifact["meaning_plan"]["format"], format);
        let generation = &artifact["content_generation"];
        let text = result["text"].as_str().unwrap();
        assert_eq!(generation["schema"], "core-meaning-generation-v1");
        assert_eq!(generation["stop_reason"], "EOS");
        assert_eq!(generation["text"], text);
        assert_eq!(
            generation["model_revision"],
            result["content_plan"]["model_revision"]
        );
        assert_eq!(
            generation["model_artifact_ref"],
            result["content_plan"]["model_artifact_ref"]
        );
        let required_units = generation["units"]
            .as_array()
            .unwrap()
            .iter()
            .map(|unit| unit["text"].clone())
            .collect();
        assert_eq!(
            result["content_plan"]["required_units"],
            Value::Array(required_units)
        );
        assert_eq!(result["render"]["status"], "RENDERED");
        assert_eq!(
            result["render"]["bytes"],
            serde_json::json!(text.as_bytes())
        );
        assert_eq!(std::fs::read(&path).unwrap(), text.as_bytes());
        assert_eq!(result["receipts"][0]["phase"], "CONFIRMED");
        if format == "BULLETS" {
            assert!(text.lines().count() > 1);
            assert!(text.lines().all(|line| line.starts_with('・')));
        } else {
            assert!(!text.starts_with('・'));
            assert_eq!(text.lines().count(), 1);
        }
        observations.push((result, artifact));
    }
    let [(bullet_result, bullets), (paragraph_result, paragraph)] = observations.as_slice() else {
        unreachable!("both requested comparison formats were exercised");
    };
    assert_eq!(
        bullet_result["actor_memory_revision"],
        paragraph_result["actor_memory_revision"]
    );
    for side in ["inventory_a", "inventory_b"] {
        assert_eq!(bullets[side], paragraph[side]);
    }
    for field in ["subject_a", "subject_b", "scope", "rows", "proof_ids"] {
        assert_eq!(
            bullets["meaning_plan"][field],
            paragraph["meaning_plan"][field]
        );
    }
    assert_eq!(
        paragraph["meaning_plan"]["rows"].as_array().unwrap().len(),
        3
    );
    assert_eq!(
        bullets["content_generation"]["evidence_ids"],
        paragraph["content_generation"]["evidence_ids"]
    );
    for field in ["context_state_sha256", "model_revision", "text"] {
        assert_ne!(
            bullets["content_generation"][field],
            paragraph["content_generation"][field]
        );
    }

    let explanation = record(&execute(
        &scratch.0,
        "owner",
        &[
            "--input",
            "explain-paragraph",
            "--max-actions",
            "128",
            "詳しく説明して",
        ],
    ));
    assert_eq!(explanation["lifecycle"], "COMMIT");
    assert_eq!(explanation["guarantee"], "HYPOTHESIS");
    assert_eq!(explanation["dependencies"][0], paragraph_result["id"]);
    let details = explanation["text"].as_str().unwrap();
    for content in ["「甲」", "「乙」", "「無線式」", "「有線式」", "「観測器」"]
    {
        assert!(details.contains(content), "lost {content}: {details}");
    }

    let refused = scratch.0.join("refused.txt");
    let target = refused.to_str().unwrap();
    for (input_id, text) in [
        (
            "table",
            "[scope=bench]甲と乙の保存済みの関係を表形式で比較して",
        ),
        (
            "compound",
            "[scope=bench]甲と乙の保存済みの関係を箇条書きで比較して送信して",
        ),
    ] {
        let result = stopped(
            &execute(
                &scratch.0,
                "owner",
                &[
                    "--input",
                    input_id,
                    "--output",
                    target,
                    "--allow-write",
                    target,
                    text,
                ],
            ),
            2,
            "SUSPEND",
        );
        assert!(result["content_plan"].is_null());
        assert!(result["receipts"].as_array().unwrap().is_empty());
        assert!(!refused.exists());
    }

    // A saved operand can contain a Unicode line separator. Keep its
    // exact bytes and refuse adoption if those bytes break the layout.
    let premise = record(&execute(
        &scratch.0,
        "owner",
        &["--input", "layout-break", "[scope=layout]丙は前\u{2028}後"],
    ));
    assert_eq!(premise["lifecycle"], "COMMIT");
    for (input_id, layout) in [("broken-paragraph", "段落"), ("broken-bullets", "箇条書き")] {
        let text = format!("[scope=layout]丙と丁の関係を{layout}で比較して");
        let result = stopped(
            &execute(
                &scratch.0,
                "owner",
                &[
                    "--input",
                    input_id,
                    "--output",
                    target,
                    "--allow-write",
                    target,
                    &text,
                ],
            ),
            2,
            "SUSPEND",
        );
        assert!(
            result["reason"]
                .as_str()
                .unwrap()
                .contains("COMPARISON_FORMAT_MISMATCH")
        );
        assert!(result["content_plan"].is_null());
        assert!(result["receipts"].as_array().unwrap().is_empty());
        assert!(!refused.exists());
    }
}

#[test]
fn saved_relation_comparison_explanation_revalidates_both_scoped_inventories() {
    let scratch = Scratch::new();
    let actor = "relation-comparison-explanation-owner";
    let comparison_case = "relation-comparison-explanation-case";
    let rule = record(&execute_in_case(
        &scratch.0,
        actor,
        comparison_case,
        &[
            "--input",
            "biology-rule",
            "もし[scope=biology]Xは鳥なら[scope=biology]Xは動物",
        ],
    ));
    let coco = record(&execute_in_case(
        &scratch.0,
        actor,
        comparison_case,
        &["--input", "coco-bird", "[scope=biology]ココは鳥"],
    ));
    let mike = record(&execute_in_case(
        &scratch.0,
        actor,
        comparison_case,
        &["--input", "mike-bird", "[scope=biology]ミケは鳥"],
    ));
    record(&execute_in_case(
        &scratch.0,
        actor,
        comparison_case,
        &["--input", "coco-feathers", "[scope=biology]ココは羽毛持ち"],
    ));
    record(&execute_in_case(
        &scratch.0,
        actor,
        comparison_case,
        &["--input", "mike-cat", "[scope=biology]ミケは猫"],
    ));

    let comparison = record(&execute_in_case(
        &scratch.0,
        actor,
        comparison_case,
        &[
            "--input",
            "compare-relations-for-explanation",
            "--max-actions",
            "128",
            "[scope=biology]ココとミケの保存済みの関係を比較して",
        ],
    ));
    assert_eq!(comparison["lifecycle"], "COMMIT");
    assert_eq!(comparison["guarantee"], "HYPOTHESIS");

    let explanation = record(&execute_in_case(
        &scratch.0,
        actor,
        comparison_case,
        &[
            "--input",
            "explain-relation-comparison",
            "--max-actions",
            "128",
            "詳しく説明して",
        ],
    ));
    assert_eq!(explanation["lifecycle"], "COMMIT");
    assert_eq!(explanation["guarantee"], "HYPOTHESIS");
    let explanation_text = explanation["text"].as_str().unwrap();
    assert_ne!(explanation_text, comparison["text"]);
    for marker in [
        "前回の保存関係比較を詳しく説明します",
        "左右のscope別inventoryを再計算",
        "「ココ」",
        "「ミケ」",
        "「羽毛持ち」",
        "「猫」",
        "「動物」",
        "保存規則からの導出",
        "関係が偽または存在しない証明ではありません",
        "現実世界の真偽は確認していません",
    ] {
        assert!(explanation_text.contains(marker), "missing {marker}");
    }
    for source in [&rule, &coco, &mike] {
        for dependency in source["dependencies"].as_array().unwrap() {
            assert!(explanation_text.contains(dependency.as_str().unwrap()));
        }
    }
    let explanation_dependencies = explanation["dependencies"].as_array().unwrap();
    for dependency in
        std::iter::once(&comparison["id"]).chain(comparison["dependencies"].as_array().unwrap())
    {
        assert!(
            explanation_dependencies.contains(dependency),
            "comparison explanation lost dependency {dependency}"
        );
    }

    let stale_comparison = record(&execute_in_case(
        &scratch.0,
        actor,
        comparison_case,
        &[
            "--input",
            "compare-relations-before-invalidation",
            "--max-actions",
            "128",
            "[scope=biology]ココとミケの保存済みの関係を比較して",
        ],
    ));
    record(&execute_in_case(
        &scratch.0,
        actor,
        "relation-comparison-explanation-maintenance-case",
        &[
            "--input",
            "invalidate-coco-bird",
            &format!("{}を無効化して", coco["dependencies"][0].as_str().unwrap()),
        ],
    ));
    let stale_explanation = stopped(
        &execute_in_case(
            &scratch.0,
            actor,
            comparison_case,
            &[
                "--input",
                "explain-stale-relation-comparison",
                "--max-actions",
                "128",
                "詳しく説明して",
            ],
        ),
        2,
        "SUSPEND",
    );
    assert_eq!(
        stale_explanation["reason"]
            .as_str()
            .unwrap()
            .split(':')
            .next(),
        Some("CONTEXT_PROOF_CHANGED")
    );
    assert_eq!(stale_comparison["lifecycle"], "COMMIT");
}

#[test]
fn previous_result_summary_reuses_the_exact_output_and_explains_selected_spans() {
    let scratch = Scratch::new();
    let actor = "previous-result-summary-owner";
    let coco = record(&execute(
        &scratch.0,
        actor,
        &["--input", "summary-coco-bird", "ココは鳥"],
    ));
    let mike = record(&execute(
        &scratch.0,
        actor,
        &["--input", "summary-mike-bird", "ミケは鳥"],
    ));
    let comparison = record(&execute(
        &scratch.0,
        actor,
        &["--input", "summary-common", "ココとミケの共通点は何ですか"],
    ));
    assert_eq!(comparison["lifecycle"], "COMMIT");
    assert_eq!(comparison["guarantee"], "USER_PREMISE");
    let output_path = scratch.0.join("previous-result-summary.txt");
    let summary = record(&execute(
        &scratch.0,
        actor,
        &[
            "--input",
            "summary-previous-common",
            "--output",
            output_path.to_str().unwrap(),
            "--allow-write",
            output_path.to_str().unwrap(),
            "前回の比較を2文以内で要約して",
        ],
    ));
    assert_eq!(summary["lifecycle"], "COMMIT");
    assert_eq!(summary["guarantee"], "HYPOTHESIS");
    assert_ne!(summary["text"], comparison["text"]);
    let summary_text = summary["text"].as_str().unwrap();
    assert!(summary_text.contains("前回結果からの抽出候補"));
    assert!(summary_text.contains("目的・意味・条件・否定の網羅は検証していません"));
    assert!(
        summary_text
            .lines()
            .filter(|line| line.starts_with("- "))
            .count()
            <= 2
    );
    assert_eq!(std::fs::read_to_string(&output_path).unwrap(), summary_text);
    assert_eq!(summary["receipts"][0]["phase"], "CONFIRMED");
    let summary_modified = std::fs::metadata(&output_path).unwrap().modified().unwrap();
    let summary_dependencies = summary["dependencies"].as_array().unwrap();
    assert_eq!(summary_dependencies[0].as_str().unwrap().len(), 64);
    for dependency in
        std::iter::once(&comparison["id"]).chain(comparison["dependencies"].as_array().unwrap())
    {
        assert!(summary_dependencies.contains(dependency));
    }
    for premise in [&coco["dependencies"][0], &mike["dependencies"][0]] {
        assert!(summary_dependencies.contains(premise));
    }

    let explanation_path = scratch.0.join("previous-result-summary-explanation.txt");
    let explanation_args = [
        "--input",
        "summary-explain",
        "--output",
        explanation_path.to_str().unwrap(),
        "--allow-write",
        explanation_path.to_str().unwrap(),
        "詳しく説明して",
    ];
    let explanation = record(&execute(&scratch.0, actor, &explanation_args));
    assert_eq!(explanation["lifecycle"], "COMMIT");
    assert_eq!(explanation["guarantee"], "HYPOTHESIS");
    let explanation_text = explanation["text"].as_str().unwrap();
    assert!(explanation_text.contains("前回結果の要約説明"));
    assert!(explanation_text.contains(comparison["id"].as_str().unwrap()));
    assert!(explanation_text.contains("選択文"));
    assert!(explanation_text.contains("元結果byte"));
    assert!(explanation_text.contains("要約の網羅性・意味・条件保持は未検証"));
    let mut expected_dependencies = vec![summary["id"].clone()];
    expected_dependencies.extend_from_slice(summary_dependencies);
    assert_eq!(
        explanation["dependencies"],
        Value::Array(expected_dependencies)
    );
    for (result, path) in [(&summary, &output_path), (&explanation, &explanation_path)] {
        let bytes = std::fs::read(path).unwrap();
        assert_eq!(bytes, result["text"].as_str().unwrap().as_bytes());
        assert_eq!(result["content_plan"]["text"], result["text"]);
        assert_eq!(result["content_plan"]["guarantee"], "HYPOTHESIS");
        let hash = minidora_domain::digest(&bytes);
        assert_eq!(result["render"]["actual_sha256"], hash);
        assert_eq!(result["receipts"].as_array().unwrap().len(), 1);
        let receipt = &result["receipts"][0];
        assert_eq!(receipt["phase"], "CONFIRMED");
        assert_eq!(receipt["target"], path.to_str().unwrap());
        assert_eq!(receipt["content_sha256"], hash);
        assert_eq!(receipt["observed_bytes"], bytes.len());
    }
    let store = minidora_engine::store::Store::open(&scratch.0).unwrap();
    let actions = store.actions(actor, "normal-route").unwrap();
    assert_eq!(actions.len(), 2);
    assert_ne!(actions[0].action_id, actions[1].action_id);
    assert!(actions.iter().all(|action| {
        action.phase == minidora_domain::EffectPhase::Confirmed
            && [&summary, &explanation].iter().any(|result| {
                result["receipts"][0]["action_id"] == action.action_id
                    && result["receipts"][0]["target"] == action.target
            })
    }));
    let explanation_modified = std::fs::metadata(&explanation_path)
        .unwrap()
        .modified()
        .unwrap();
    assert_eq!(
        record(&execute(&scratch.0, actor, &explanation_args)),
        explanation
    );
    assert_eq!(store.actions(actor, "normal-route").unwrap(), actions);
    for (path, result, modified) in [
        (&output_path, &summary, summary_modified),
        (&explanation_path, &explanation, explanation_modified),
    ] {
        assert_eq!(
            std::fs::read(path).unwrap(),
            result["text"].as_str().unwrap().as_bytes()
        );
        assert_eq!(
            std::fs::metadata(path).unwrap().modified().unwrap(),
            modified
        );
    }
    let saved = inspect_normal_case(&scratch.0, actor);
    assert_eq!(saved["result"], explanation);
    assert_eq!(
        saved["bundle"]["meaning"]["context_result_ref"],
        summary["id"]
    );
    assert_eq!(
        saved["prior_results"][comparison["id"].as_str().unwrap()],
        comparison
    );
    assert_eq!(
        saved["prior_results"][summary["id"].as_str().unwrap()],
        summary
    );

    let missing = stopped(
        &execute(
            &Scratch::new().0,
            "empty-summary-owner",
            &["前回の結果を要約して"],
        ),
        2,
        "SUSPEND",
    );
    assert!(
        missing["reason"]
            .as_str()
            .unwrap()
            .starts_with("MISSING_ACTUAL_RESULT:")
    );
}

#[test]
fn imported_text_with_a_result_summary_heading_keeps_its_user_premise_explanation() {
    let scratch = Scratch::new();
    let actor = "imported-summary-heading-owner";
    let text = "前回結果からの抽出候補（一般の文章）。\nこれは取り込んだ本文です。";
    let request = format!(
        "text-import:\n{}",
        serde_json::json!({"text": text, "protected": [], "max_chars": 128})
    );
    let imported = record(&execute(
        &scratch.0,
        actor,
        &["--input", "import-heading", &request],
    ));
    assert_eq!(imported["lifecycle"], "COMMIT");
    assert_eq!(imported["guarantee"], "USER_PREMISE");
    assert_eq!(imported["text"], text);
    let explanation_args = ["--input", "explain-import-heading", "詳しく説明して"];
    let explanation = record(&execute(&scratch.0, actor, &explanation_args));
    assert_eq!(explanation["lifecycle"], "COMMIT");
    assert_eq!(explanation["guarantee"], "USER_PREMISE");
    assert_eq!(explanation["text"], text);
    assert_eq!(explanation["content_plan"]["guarantee"], "USER_PREMISE");
    assert_eq!(
        explanation["dependencies"],
        serde_json::json!([imported["id"]])
    );
    assert_eq!(explanation["receipts"], serde_json::json!([]));
    assert_eq!(
        record(&execute(&scratch.0, actor, &explanation_args)),
        explanation
    );
    let saved = inspect_normal_case(&scratch.0, actor);
    assert_eq!(saved["result"], explanation);
    assert_eq!(
        saved["bundle"]["meaning"]["context_result_ref"],
        imported["id"]
    );
    assert_eq!(
        saved["prior_results"][imported["id"].as_str().unwrap()],
        imported
    );
    assert!(
        minidora_engine::store::Store::open(&scratch.0)
            .unwrap()
            .actions(actor, "normal-route")
            .unwrap()
            .is_empty()
    );
}

#[test]
fn previous_result_summary_distinguishes_exact_lines_from_a_line_limit() {
    let scratch = Scratch::new();
    let actor = "line-summary-owner";
    record(&execute(&scratch.0, actor, &["ココは鳥"]));
    record(&execute(&scratch.0, actor, &["ミケは鳥"]));
    let prior = record(&execute(
        &scratch.0,
        actor,
        &["ココとミケの共通点は何ですか"],
    ));
    let prior_id = prior["id"].as_str().unwrap();
    let prior_text = prior["text"].as_str().unwrap();

    let exact = record(&execute(&scratch.0, actor, &["前の結果を2行で要約して"]));
    let exact_text = exact["text"].as_str().unwrap();
    assert_eq!(exact_text.lines().count(), 2);
    assert_eq!(exact["guarantee"], "HYPOTHESIS");
    assert!(exact_text.contains("抽出候補・網羅性未確認"));
    let store = minidora_engine::store::Store::open(&scratch.0).unwrap();
    let proof: Value = serde_json::from_slice(
        &store
            .get_bytes(
                actor,
                "normal-route",
                exact["dependencies"][0].as_str().unwrap(),
            )
            .unwrap(),
    )
    .unwrap();
    assert_eq!(proof["line_limit"], serde_json::json!({ "EXACT": 2 }));
    assert_eq!(proof["source_result_id"], prior_id);
    assert_eq!(
        proof["summary_unit_sha256"],
        minidora_domain::digest(exact_text.as_bytes())
    );
    for selected in proof["summary"]["sentences"].as_array().unwrap() {
        for (text, start, end) in [
            (prior_text, "source_start", "source_end"),
            (exact_text, "output_start", "output_end"),
        ] {
            let span = selected[start].as_u64().unwrap() as usize
                ..selected[end].as_u64().unwrap() as usize;
            assert_eq!(text.get(span), selected["text"].as_str());
        }
    }
    let explanation = record(&execute(&scratch.0, actor, &["詳しく説明して"]));
    assert!(explanation["text"].as_str().unwrap().contains("元結果byte"));

    let one_sentence = record(&execute(
        &scratch.0,
        actor,
        &["text-import:\n{\"text\":\"水温は12.5度でした。\",\"protected\":[],\"max_chars\":128}"],
    ));
    let one_sentence_id = one_sentence["id"].as_str().unwrap();
    let capped = record(&execute(
        &scratch.0,
        actor,
        &[
            "--input",
            "capped",
            "--context",
            one_sentence_id,
            "summarize the previous result in up to 2 lines",
        ],
    ));
    let capped_text = capped["text"].as_str().unwrap();
    assert_eq!(capped_text.lines().count(), 1);
    assert!(capped_text.contains("水温は12.5度でした。"));
    let target = scratch.0.join("unmet-lines.txt");
    let target_arg = target.to_str().unwrap();
    let unmet = stopped(
        &execute(
            &scratch.0,
            actor,
            &[
                "--input",
                "unmet",
                "--context",
                one_sentence_id,
                "--output",
                target_arg,
                "--allow-write",
                target_arg,
                "前の結果を2行で要約して",
            ],
        ),
        2,
        "SUSPEND",
    );
    let reason = unmet["reason"].as_str().unwrap();
    assert_eq!(reason.matches("RESULT_SUMMARY_LINE_COUNT_UNMET").count(), 1);
    assert!(unmet["content_plan"].is_null());
    assert!(unmet["receipts"].as_array().unwrap().is_empty());
    assert!(!target.exists());
}

#[test]
fn previous_result_summary_stops_when_source_scan_exceeds_action_budget() {
    let scratch = Scratch::new();
    let source = serde_json::json!({
        "text": "First source sentence. Second source sentence.",
        "max_sentences": 2
    })
    .to_string();
    let previous = record(&execute(
        &scratch.0,
        "summary-budget-owner",
        &[&format!("text-summary:\n{source}")],
    ));
    assert_eq!(previous["lifecycle"], "COMMIT");

    let limited = stopped(
        &execute(
            &scratch.0,
            "summary-budget-owner",
            &[
                "--input",
                "summary-over-budget",
                "--max-actions",
                "1",
                "前回の結果を要約して",
            ],
        ),
        2,
        "SUSPEND",
    );
    assert!(
        limited["reason"]
            .as_str()
            .unwrap()
            .starts_with("RESOURCE_LIMIT:")
    );
    assert!(limited["dependencies"].as_array().unwrap().is_empty());
}

#[test]
fn conjunctive_relation_questions_query_every_clause_and_keep_their_proofs() {
    let scratch = Scratch::new();
    let rule = record(&execute(
        &scratch.0,
        "owner",
        &[
            "--input",
            "conjunction-biology-rule",
            "[scope=biology]Xは鳥なら[scope=biology]Xは動物",
        ],
    ));
    let bird = record(&execute(
        &scratch.0,
        "owner",
        &[
            "--input",
            "conjunction-coco-bird",
            "[scope=biology]ココは鳥",
        ],
    ));
    let supported = record(&execute(
        &scratch.0,
        "owner",
        &[
            "--input",
            "conjunction-both-supported",
            "[scope=biology]ココは鳥かつ動物ですか",
        ],
    ));
    assert_eq!(supported["lifecycle"], "COMMIT");
    assert_eq!(supported["guarantee"], "DERIVED");
    let supported_text = supported["text"].as_str().unwrap();
    assert!(supported_text.contains("二つの関係"));
    assert!(supported_text.contains("鳥である"));
    assert!(supported_text.contains("動物である"));
    assert!(
        supported["dependencies"]
            .as_array()
            .unwrap()
            .contains(&rule["dependencies"][0])
    );
    assert!(
        supported["dependencies"]
            .as_array()
            .unwrap()
            .contains(&bird["dependencies"][0])
    );
    let negative_clause = record(&execute(
        &scratch.0,
        "owner",
        &[
            "--input",
            "conjunction-negative-clause",
            "[scope=biology]ココは鳥ではないかつ動物ですか",
        ],
    ));
    assert_eq!(negative_clause["lifecycle"], "COMMIT");
    assert!(
        negative_clause["text"]
            .as_str()
            .unwrap()
            .contains("組み合わせを支持しません")
    );
    assert!(
        negative_clause["text"]
            .as_str()
            .unwrap()
            .contains("ココは鳥である")
    );
    assert!(
        negative_clause["dependencies"]
            .as_array()
            .unwrap()
            .contains(&bird["dependencies"][0])
    );

    let not_mammal = record(&execute(
        &scratch.0,
        "owner",
        &[
            "--input",
            "conjunction-coco-not-mammal",
            "[scope=biology]ココは哺乳類ではない",
        ],
    ));
    let refuted = record(&execute(
        &scratch.0,
        "owner",
        &[
            "--input",
            "conjunction-one-refuted",
            "[scope=biology]ココは鳥かつ哺乳類ですか",
        ],
    ));
    assert_eq!(refuted["lifecycle"], "COMMIT");
    assert!(
        refuted["text"]
            .as_str()
            .unwrap()
            .contains("組み合わせを支持しません")
    );
    assert!(
        refuted["text"]
            .as_str()
            .unwrap()
            .contains("ココは哺乳類ではない")
    );
    assert!(
        refuted["dependencies"]
            .as_array()
            .unwrap()
            .contains(&not_mammal["dependencies"][0])
    );

    let missing = stopped(
        &execute(
            &scratch.0,
            "owner",
            &[
                "--input",
                "conjunction-missing-clause",
                "[scope=biology]ココは鳥かつふわふわですか",
            ],
        ),
        2,
        "SUSPEND",
    );
    assert!(
        missing["reason"]
            .as_str()
            .unwrap()
            .contains("MISSING_PREMISES")
    );

    record(&execute(
        &scratch.0,
        "owner",
        &[
            "--input",
            "conjunction-coco-mammal",
            "[scope=biology]ココは哺乳類",
        ],
    ));
    let conflict = stopped(
        &execute(
            &scratch.0,
            "owner",
            &[
                "--input",
                "conjunction-conflicting-clause",
                "[scope=biology]ココは鳥かつ哺乳類ですか",
            ],
        ),
        2,
        "SUSPEND",
    );
    assert!(
        conflict["reason"]
            .as_str()
            .unwrap()
            .contains("CONFLICTING_PREMISES")
    );

    let three_clause_actor = "conjunction-three-clause-owner";
    let three_clause_case = "conjunction-three-clause-case";
    let three_clause_rule = record(&execute_in_case(
        &scratch.0,
        three_clause_actor,
        three_clause_case,
        &[
            "--input",
            "conjunction-three-clause-rule",
            "[scope=biology]Xは鳥なら[scope=biology]Xは動物",
        ],
    ));
    let three_clause_bird = record(&execute_in_case(
        &scratch.0,
        three_clause_actor,
        three_clause_case,
        &[
            "--input",
            "conjunction-three-clause-bird",
            "[scope=biology]ココは鳥",
        ],
    ));
    let three_clause_mammal = record(&execute_in_case(
        &scratch.0,
        three_clause_actor,
        three_clause_case,
        &[
            "--input",
            "conjunction-three-clause-mammal",
            "[scope=biology]ココは哺乳類",
        ],
    ));
    let three_clause = record(&execute_in_case(
        &scratch.0,
        three_clause_actor,
        three_clause_case,
        &[
            "--input",
            "conjunction-three-clause-question",
            "[scope=biology]ココは鳥かつ動物かつ哺乳類ですか",
        ],
    ));
    assert_eq!(three_clause["lifecycle"], "COMMIT");
    assert_eq!(three_clause["guarantee"], "DERIVED");
    let three_clause_text = three_clause["text"].as_str().unwrap();
    assert!(three_clause_text.contains("三つの関係"));
    assert!(three_clause_text.contains("鳥である"));
    assert!(three_clause_text.contains("動物である"));
    assert!(three_clause_text.contains("哺乳類である"));
    for dependency in [
        &three_clause_rule["dependencies"][0],
        &three_clause_bird["dependencies"][0],
        &three_clause_mammal["dependencies"][0],
    ] {
        assert!(
            three_clause["dependencies"]
                .as_array()
                .unwrap()
                .contains(dependency)
        );
    }
    let three_clause_negative = record(&execute_in_case(
        &scratch.0,
        three_clause_actor,
        three_clause_case,
        &[
            "--input",
            "conjunction-three-clause-negative-question",
            "[scope=biology]ココは鳥ではないかつ動物かつ哺乳類ですか",
        ],
    ));
    assert_eq!(three_clause_negative["lifecycle"], "COMMIT");
    assert!(
        three_clause_negative["text"]
            .as_str()
            .unwrap()
            .contains("三つの関係")
    );
    assert!(
        three_clause_negative["text"]
            .as_str()
            .unwrap()
            .contains("組み合わせを支持しません")
    );
    assert!(
        three_clause_negative["dependencies"]
            .as_array()
            .unwrap()
            .contains(&three_clause_bird["dependencies"][0])
    );
    let conflict_actor = "conjunction-three-clause-conflict-owner";
    let conflict_case = "conjunction-three-clause-conflict-case";
    for (input_id, fact) in [
        ("three-clause-conflict-bird", "[scope=biology]ココは鳥"),
        ("three-clause-conflict-animal", "[scope=biology]ココは動物"),
        (
            "three-clause-conflict-mammal",
            "[scope=biology]ココは哺乳類",
        ),
        (
            "three-clause-conflict-not-mammal",
            "[scope=biology]ココは哺乳類ではない",
        ),
    ] {
        record(&execute_in_case(
            &scratch.0,
            conflict_actor,
            conflict_case,
            &["--input", input_id, fact],
        ));
    }
    let three_clause_conflict = stopped(
        &execute_in_case(
            &scratch.0,
            conflict_actor,
            conflict_case,
            &[
                "--input",
                "conjunction-three-clause-conflict-question",
                "[scope=biology]ココは鳥かつ動物かつ哺乳類ですか",
            ],
        ),
        2,
        "SUSPEND",
    );
    assert!(
        three_clause_conflict["reason"]
            .as_str()
            .unwrap()
            .contains("CONFLICTING_PREMISES")
    );

    record(&execute_in_case(
        &scratch.0,
        "owner",
        "conjunction-english",
        &[
            "--input",
            "conjunction-english-rule",
            "If X is a bird then X is an animal",
        ],
    ));
    let english_bird = record(&execute_in_case(
        &scratch.0,
        "owner",
        "conjunction-english",
        &["--input", "conjunction-english-bird", "Coco is a bird"],
    ));
    let english = record(&execute_in_case(
        &scratch.0,
        "owner",
        "conjunction-english",
        &[
            "--input",
            "conjunction-english-question",
            "Is Coco a bird and an animal?",
        ],
    ));
    assert_eq!(english["lifecycle"], "COMMIT");
    assert_eq!(english["guarantee"], "DERIVED");
    assert!(
        english["text"]
            .as_str()
            .unwrap()
            .contains("Cocoはanimalである")
    );
    assert!(
        english["dependencies"]
            .as_array()
            .unwrap()
            .contains(&english_bird["dependencies"][0])
    );

    record(&execute_in_case(
        &scratch.0,
        "owner",
        "conjunction-english-three-clause",
        &[
            "--input",
            "conjunction-english-three-clause-mammal",
            "Coco is a mammal",
        ],
    ));
    let english_three_clause = record(&execute_in_case(
        &scratch.0,
        "owner",
        "conjunction-english-three-clause",
        &[
            "--input",
            "conjunction-english-three-clause-question",
            "Is Coco a bird and an animal and a mammal?",
        ],
    ));
    assert_eq!(english_three_clause["lifecycle"], "COMMIT");
    assert!(
        english_three_clause["text"]
            .as_str()
            .unwrap()
            .contains("三つの関係")
    );
    assert!(
        english_three_clause["text"]
            .as_str()
            .unwrap()
            .contains("Cocoはmammalである")
    );
}

#[test]
fn singular_pronoun_relation_questions_use_only_unique_same_case_proof_context() {
    let scratch = Scratch::new();
    let rule = record(&execute(
        &scratch.0,
        "owner",
        &[
            "--input",
            "context-biology-rule",
            "[scope=biology]Xは鳥なら[scope=biology]Xは動物",
        ],
    ));
    let bird = record(&execute(
        &scratch.0,
        "owner",
        &["--input", "context-coco-bird", "[scope=biology]ココは鳥"],
    ));
    let answer = record(&execute(
        &scratch.0,
        "owner",
        &["--input", "context-pronoun-question", "それは動物ですか"],
    ));
    assert_eq!(answer["guarantee"], "DERIVED");
    assert!(
        answer["text"]
            .as_str()
            .unwrap()
            .contains("ココは動物である")
    );
    assert!(
        answer["dependencies"]
            .as_array()
            .unwrap()
            .contains(&rule["dependencies"][0])
    );
    assert!(
        answer["dependencies"]
            .as_array()
            .unwrap()
            .contains(&bird["dependencies"][0])
    );

    let why = record(&execute(
        &scratch.0,
        "owner",
        &["--input", "context-pronoun-why", "なぜそれは動物ですか"],
    ));
    assert_eq!(why["guarantee"], "DERIVED");
    assert!(
        why["text"]
            .as_str()
            .unwrap()
            .contains("直接提示された前提: ココは鳥である")
    );
    assert_eq!(why["dependencies"], answer["dependencies"]);

    let wrong_scope = stopped(
        &execute(
            &scratch.0,
            "owner",
            &[
                "--input",
                "context-wrong-scope",
                "[scope=ecology]それは動物ですか",
            ],
        ),
        2,
        "SUSPEND",
    );
    assert!(
        wrong_scope["reason"]
            .as_str()
            .unwrap()
            .contains("MISSING_RELATION_CONTEXT")
    );

    record(&execute(&scratch.0, "owner", &["2+3"]));
    let after_topic_change = stopped(
        &execute(&scratch.0, "owner", &["それは動物ですか"]),
        2,
        "SUSPEND",
    );
    assert!(
        after_topic_change["reason"]
            .as_str()
            .unwrap()
            .contains("MISSING_RELATION_CONTEXT")
    );

    let no_case_context = stopped(
        &execute_in_case(
            &scratch.0,
            "owner",
            "pronoun-without-case-context",
            &["--input", "pronoun-question", "それは動物ですか"],
        ),
        2,
        "SUSPEND",
    );
    assert!(
        no_case_context["reason"]
            .as_str()
            .unwrap()
            .contains("MISSING_RELATION_CONTEXT")
    );

    let other_actor = stopped(
        &execute_in_case(
            &scratch.0,
            "other-actor",
            "pronoun-without-actor-context",
            &["--input", "pronoun-question", "それは動物ですか"],
        ),
        2,
        "SUSPEND",
    );
    assert!(
        other_actor["reason"]
            .as_str()
            .unwrap()
            .contains("MISSING_RELATION_CONTEXT")
    );

    let biology_rule = record(&execute_in_case(
        &scratch.0,
        "owner",
        "pronoun-ambiguous-context",
        &[
            "--input",
            "ambiguous-context-rule",
            "[scope=biology]Xは鳥なら[scope=biology]Xは動物",
        ],
    ));
    let biology_bird = record(&execute_in_case(
        &scratch.0,
        "owner",
        "pronoun-ambiguous-context",
        &[
            "--input",
            "ambiguous-biology-premise",
            "[scope=biology]ツバメは鳥",
        ],
    ));
    let ecology_bird = record(&execute_in_case(
        &scratch.0,
        "owner",
        "pronoun-ambiguous-context",
        &[
            "--input",
            "ambiguous-ecology-premise",
            "[scope=ecology]ツバメは鳥",
        ],
    ));
    let listed = record(&execute_in_case(
        &scratch.0,
        "owner",
        "pronoun-ambiguous-context",
        &["--input", "list-pronoun-context", "ツバメとは何ですか"],
    ));
    assert_eq!(listed["guarantee"], "DERIVED");
    assert_eq!(listed["dependencies"].as_array().unwrap().len(), 4);
    assert!(listed["text"].as_str().unwrap().contains("範囲: biology"));
    assert!(listed["text"].as_str().unwrap().contains("範囲: ecology"));
    assert!(listed["text"].as_str().unwrap().contains("規則導出"));
    for source in [&biology_bird, &ecology_bird] {
        assert!(
            listed["dependencies"]
                .as_array()
                .unwrap()
                .contains(&source["dependencies"][0]),
            "missing prior dependency {:?} from {:?}; listed text: {}; rule result: {:?}",
            source["dependencies"][0],
            listed["dependencies"],
            listed["text"],
            biology_rule
        );
    }
    assert!(
        listed["dependencies"]
            .as_array()
            .unwrap()
            .contains(&rule["dependencies"][0])
            || listed["dependencies"]
                .as_array()
                .unwrap()
                .contains(&biology_rule["dependencies"][0]),
        "the derived relation must retain one applicable rule proof"
    );
    let derived_line = listed["text"]
        .as_str()
        .unwrap()
        .lines()
        .find(|line| line.contains("導出ID: "))
        .unwrap();
    let derived_id = derived_line
        .split("導出ID: ")
        .nth(1)
        .unwrap()
        .split(';')
        .next()
        .unwrap();
    assert!(
        listed["dependencies"]
            .as_array()
            .unwrap()
            .contains(&Value::String(derived_id.to_string()))
    );
    let ambiguous = stopped(
        &execute_in_case(
            &scratch.0,
            "owner",
            "pronoun-ambiguous-context",
            &["--input", "ambiguous-pronoun-question", "それは動物ですか"],
        ),
        2,
        "SUSPEND",
    );
    assert!(
        ambiguous["reason"]
            .as_str()
            .unwrap()
            .contains("AMBIGUOUS_RELATION_CONTEXT")
    );
    let relisted = record(&execute_in_case(
        &scratch.0,
        "owner",
        "pronoun-ambiguous-context",
        &["--input", "relist-pronoun-context", "ツバメとは何ですか"],
    ));
    assert_eq!(relisted["dependencies"], listed["dependencies"]);
    let disambiguated = record(&execute_in_case(
        &scratch.0,
        "owner",
        "pronoun-ambiguous-context",
        &[
            "--input",
            "scoped-pronoun-question",
            "[scope=biology]それは動物ですか",
        ],
    ));
    assert_eq!(disambiguated["guarantee"], "DERIVED");
    assert!(
        disambiguated["text"]
            .as_str()
            .unwrap()
            .contains("ツバメは動物である")
    );
    assert!(
        disambiguated["dependencies"]
            .as_array()
            .unwrap()
            .contains(&biology_bird["dependencies"][0])
    );

    record(&execute_in_case(
        &scratch.0,
        "owner",
        "english-pronoun-context",
        &[
            "--input",
            "english-context-rule",
            "If X is a bird then X is an animal",
        ],
    ));
    let english_bird = record(&execute_in_case(
        &scratch.0,
        "owner",
        "english-pronoun-context",
        &["--input", "english-context-bird", "Coco is a bird"],
    ));
    let english_answer = record(&execute_in_case(
        &scratch.0,
        "owner",
        "english-pronoun-context",
        &["--input", "english-pronoun-question", "Is it an animal?"],
    ));
    assert_eq!(english_answer["guarantee"], "DERIVED");
    assert!(
        english_answer["dependencies"]
            .as_array()
            .unwrap()
            .contains(&english_bird["dependencies"][0])
    );
    let english_why = record(&execute_in_case(
        &scratch.0,
        "owner",
        "english-pronoun-context",
        &["--input", "english-pronoun-why", "Why is it an animal?"],
    ));
    assert_eq!(english_why["guarantee"], "DERIVED");
    assert_eq!(english_why["dependencies"], english_answer["dependencies"]);
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
        "もしXは鳥または魚ならXは動物",
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
fn stdin_checks_byte_budget_before_decoding_and_before_opening_state() {
    let scratch = Scratch::new();
    let db = scratch.0.join("state");
    let invoke = |input: &[u8], limit: usize| {
        let mut child = Command::new(env!("CARGO_BIN_EXE_minidora-rust"))
            .arg("--db")
            .arg(&db)
            .args([
                "--case",
                "normal-route",
                "--actor",
                "owner",
                "--input",
                "stdin-input",
                "--json",
                "--max-input-bytes",
                &limit.to_string(),
            ])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child.stdin.take().unwrap().write_all(input).unwrap();
        child.wait_with_output().unwrap()
    };

    let original = "　２＋３を計算して。　\n";
    for (bytes, limit, reason) in [
        (original.as_bytes(), 3, "stdin exceeds --max-input-bytes"),
        (b"2+3".as_slice(), 2, "stdin exceeds --max-input-bytes"),
        (b"\xff".as_slice(), 1, "stdin is not valid UTF-8"),
        (b"2".as_slice(), 0, "stdin exceeds --max-input-bytes"),
        (b"".as_slice(), 0, "request text is empty"),
        ("　\n".as_bytes(), 4, "request text is empty"),
    ] {
        let output = invoke(bytes, limit);
        assert_eq!(output.status.code(), Some(1), "{output:?}");
        let stderr = std::str::from_utf8(&output.stderr).unwrap();
        assert!(stderr.contains(reason), "{stderr}");
        assert!(output.stdout.is_empty());
        assert!(!db.exists(), "inadmissible stdin must not open a Store");
    }

    let result = record(&invoke(original.as_bytes(), original.len()));
    assert_eq!(result["input_id"], "stdin-input");
    let store = minidora_engine::store::Store::open(&db).unwrap();
    let case = store.load_case("owner", "normal-route").unwrap().unwrap();
    let bundle = case.bundle.as_ref().unwrap();
    assert_eq!(bundle.request.text, original);
    assert_eq!(
        bundle.raw_sha256,
        minidora_domain::digest(original.as_bytes())
    );
    assert_eq!(bundle.meaning.spans[0].end, original.len());

    let refused = invoke(b"\xff", 1);
    assert_eq!(refused.status.code(), Some(1));
    let latest = store.load_case("owner", "normal-route").unwrap().unwrap();
    assert_eq!(latest.revision, case.revision);
    assert_eq!(latest.input_fingerprint, case.input_fingerprint);
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
        assert_eq!(result["dependencies"].as_array().unwrap().len(), 2);
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
        let dependencies = result["dependencies"].as_array().unwrap();
        assert_eq!(
            dependencies[0],
            minidora_domain::digest(document.as_bytes())
        );
        let store = minidora_engine::store::Store::open(&scratch.0).unwrap();
        assert_eq!(
            store
                .get_bytes(actor, "normal-route", dependencies[0].as_str().unwrap())
                .unwrap(),
            document.as_bytes()
        );
        assert_eq!(
            store
                .get_bytes(actor, "normal-route", dependencies[1].as_str().unwrap())
                .unwrap(),
            minidora_domain::canonical(&report).unwrap()
        );
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
fn json_table_group_statistics_feed_a_recomputed_same_case_explanation() {
    let scratch = Scratch::new();
    let document = r#"{"columns":["売上","地域"],"rows":[["0.1","北"],["1.0","南"],["0.3","北"],["2.0","南"]]}"#;
    let original = record(&execute(
        &scratch.0,
        "owner",
        &[
            "--input",
            "json-group-statistics",
            &format!("JSON表の「地域」ごとに「売上」の統計量を計算して:\n{document}"),
        ],
    ));
    assert_eq!(original["lifecycle"], "COMMIT");
    let source_ref = minidora_domain::digest(document.as_bytes());
    let report_ref = original["dependencies"][1].as_str().unwrap();
    assert_eq!(
        original["dependencies"],
        serde_json::json!([source_ref, report_ref])
    );

    let explained = record(&execute(
        &scratch.0,
        "owner",
        &[
            "--input",
            "json-group-statistics-explanation",
            "詳しく説明して",
        ],
    ));
    assert_eq!(explained["lifecycle"], "COMMIT");
    assert_eq!(explained["guarantee"], "COMPUTED");
    assert_eq!(
        explained["dependencies"],
        serde_json::json!([original["id"], source_ref, report_ref])
    );
    let text = explained["text"].as_str().unwrap();
    for expected in [
        "再計算し、保存レポートと照合しました",
        "元資料 SHA-256:",
        "グループ列: 地域; 集計列: 売上",
        "グループ \"北\": n=2; 合計=2/5; 平均=1/5",
        "元値 行1・列1",
        "元値 行3・列1",
    ] {
        assert!(text.contains(expected), "missing {expected:?}: {text}");
    }

    let store = minidora_engine::store::Store::open(&scratch.0).unwrap();
    let graph: Value = serde_json::from_slice(
        &store
            .get_bytes(
                "owner",
                "normal-route",
                explained["execution_graph_ref"].as_str().unwrap(),
            )
            .unwrap(),
    )
    .unwrap();
    assert_eq!(
        graph[5]["previous-json-group-statistics-report"]["previous-json-group-statistics-source"],
        source_ref
    );
    assert_ne!(explained["text"], original["text"]);
}

#[test]
fn natural_group_mean_question_consumes_the_actual_prior_json_statistics() {
    let scratch = Scratch::new();
    let document = r#"{"columns":["score","team"],"rows":[["0.1","north"],["1.0","south"],["0.3","north"],["2.0","south"],["0.4","east"]]}"#;
    let original = record(&execute(
        &scratch.0,
        "owner",
        &[
            "--input",
            "group-mean-source",
            &format!("JSON表の「team」ごとに「score」の統計量を計算して:\n{document}"),
        ],
    ));
    assert_eq!(original["lifecycle"], "COMMIT");

    let answer = record(&execute(
        &scratch.0,
        "owner",
        &[
            "--input",
            "group-mean-question",
            "どのグループの平均値が一番高いですか",
        ],
    ));
    assert_eq!(answer["lifecycle"], "COMMIT");
    assert_eq!(answer["guarantee"], "COMPUTED");
    assert_eq!(answer["dependencies"][0], original["id"]);
    assert!(
        answer["text"]
            .as_str()
            .unwrap()
            .contains("平均が最も高いグループ: \"south\"（平均=3/2）")
    );
    assert!(
        answer["text"]
            .as_str()
            .unwrap()
            .contains("\"north\": 平均=1/5")
    );
    assert!(
        answer["text"]
            .as_str()
            .unwrap()
            .contains("\"east\": 平均=2/5")
    );
    assert!(answer["text"].as_str().unwrap().contains("データ行2:"));

    let store = minidora_engine::store::Store::open(&scratch.0).unwrap();
    let answer_ref = answer["dependencies"][3].as_str().unwrap();
    let answer_bytes = store
        .get_bytes("owner", "normal-route", answer_ref)
        .unwrap();
    assert_eq!(minidora_domain::digest(&answer_bytes), answer_ref);
    let typed: Value = serde_json::from_slice(&answer_bytes).unwrap();
    assert_eq!(typed["comparisons"].as_array().unwrap().len(), 3);
    assert_eq!(typed["winners"][0]["group"], "south");
    assert_eq!(typed["winners"][0]["mean"], "3/2");
    assert_eq!(typed["winners"][0]["source_row_count"], 2);

    let graph: Value = serde_json::from_slice(
        &store
            .get_bytes(
                "owner",
                "normal-route",
                answer["execution_graph_ref"].as_str().unwrap(),
            )
            .unwrap(),
    )
    .unwrap();
    assert!(graph.to_string().contains(answer_ref));
    assert!(
        graph
            .to_string()
            .contains("previous-json-group-mean-answer")
    );
}

#[test]
fn natural_group_mean_question_preserves_ties_and_refuses_mixed_units() {
    let tied = Scratch::new();
    let tied_document = r#"{"columns":["score","team"],"rows":[["0.2","north"],["0.8","north"],["0.5","south"],["0.1","east"]]}"#;
    let _ = record(&execute(
        &tied.0,
        "owner",
        &[
            "--input",
            "group-mean-source",
            &format!("JSON表の「team」ごとに「score」の統計量を計算して:\n{tied_document}"),
        ],
    ));
    let tied_answer = record(&execute(
        &tied.0,
        "owner",
        &[
            "--input",
            "group-mean-question",
            "Which group has the highest mean?",
        ],
    ));
    assert_eq!(tied_answer["lifecycle"], "COMMIT");
    assert!(tied_answer["text"].as_str().unwrap().contains("同率で2件"));
    assert!(
        tied_answer["text"]
            .as_str()
            .unwrap()
            .contains("\"north\"（平均=1/2）")
    );
    assert!(
        tied_answer["text"]
            .as_str()
            .unwrap()
            .contains("\"south\"（平均=1/2）")
    );

    let mixed = Scratch::new();
    let mixed_document =
        r#"{"columns":["score","team"],"rows":[["1 cm","north"],["1 m","south"]]}"#;
    let _ = record(&execute(
        &mixed.0,
        "owner",
        &[
            "--input",
            "group-mean-source",
            &format!("JSON表の「team」ごとに「score」の統計量を計算して:\n{mixed_document}"),
        ],
    ));
    let mixed_answer = stopped(
        &execute(
            &mixed.0,
            "owner",
            &[
                "--input",
                "group-mean-question",
                "平均が最も高いグループはどれですか",
            ],
        ),
        2,
        "SUSPEND",
    );
    assert!(
        mixed_answer["reason"]
            .as_str()
            .unwrap()
            .contains("JSON_GROUP_MEAN_UNIT_MISMATCH")
    );
    assert!(mixed_answer["dependencies"].as_array().unwrap().is_empty());
}

#[test]
fn natural_group_mean_question_requires_an_actual_prior_statistics_result() {
    let empty = Scratch::new();
    let missing = stopped(
        &execute(
            &empty.0,
            "owner",
            &[
                "--input",
                "group-mean-question",
                "Which group has the highest mean?",
            ],
        ),
        2,
        "SUSPEND",
    );
    assert!(
        missing["reason"]
            .as_str()
            .unwrap()
            .contains("MISSING_ACTUAL_RESULT")
    );

    let wrong = Scratch::new();
    let calculation = record(&execute(
        &wrong.0,
        "owner",
        &["--input", "prior-calc", "2+3"],
    ));
    assert_eq!(calculation["lifecycle"], "COMMIT");
    let unsupported = stopped(
        &execute(
            &wrong.0,
            "owner",
            &[
                "--input",
                "group-mean-question",
                "平均が最も高いグループはどれですか",
            ],
        ),
        2,
        "SUSPEND",
    );
    assert!(
        unsupported["reason"]
            .as_str()
            .unwrap()
            .contains("MISSING_GROUP_STATISTICS_CONTEXT")
    );
    assert!(unsupported["dependencies"].as_array().unwrap().is_empty());
}

#[test]
fn natural_source_question_consumes_the_actual_prior_fetched_body() {
    let scratch = Scratch::new();
    let (source_result_id, packet_ref, source_text) = seed_fetched_source_case(&scratch.0, "owner");
    let output_path = scratch.0.join("source-answer.txt");
    let answer = record(&execute(
        &scratch.0,
        "owner",
        &[
            "--input",
            "source-question",
            "--output",
            output_path.to_str().unwrap(),
            "--allow-write",
            output_path.to_str().unwrap(),
            "資料では「photosynthesis」について何と述べていますか",
        ],
    ));
    assert_eq!(answer["lifecycle"], "COMMIT");
    assert_eq!(answer["guarantee"], "HYPOTHESIS");
    let dependencies = answer["dependencies"].as_array().unwrap();
    assert_eq!(dependencies.len(), 3);
    assert_eq!(dependencies[0], source_result_id);
    assert_eq!(dependencies[1], packet_ref);
    let answer_ref = answer["dependencies"][2].as_str().unwrap();
    let store = minidora_engine::store::Store::open(&scratch.0).unwrap();
    let answer_bytes = store
        .get_bytes("owner", "normal-route", answer_ref)
        .unwrap();
    assert_eq!(minidora_domain::digest(&answer_bytes), answer_ref);
    let typed: Value = serde_json::from_slice(&answer_bytes).unwrap();
    assert_eq!(
        typed["operation"],
        "SOURCE_QUESTION_VERSIONED_EXTRACTIVE_CANDIDATES_V2"
    );
    assert_eq!(typed["source_result_id"], source_result_id);
    assert_eq!(typed["query_terms"], serde_json::json!(["photosynthesis"]));
    assert_eq!(typed["active_source_count"], 1);
    assert_eq!(typed["matched_sentence_count"], 2);
    assert_eq!(typed["source_versions"][0]["active_packet_ref"], packet_ref);
    assert_eq!(typed["candidates"][0]["source_packet_ref"], packet_ref);
    assert_eq!(typed["candidates"][0]["source_mode"], "FIXTURE");
    let expected_candidates = [
        (29, 64, "Photosynthesis stores light energy."),
        (65, 107, "Plants use photosynthesis in their leaves."),
    ];
    let candidates = typed["candidates"].as_array().unwrap();
    assert_eq!(candidates.len(), expected_candidates.len());
    for (index, (candidate, &(start, end, raw))) in
        candidates.iter().zip(&expected_candidates).enumerate()
    {
        assert_eq!(&source_text[start..end], raw);
        assert_eq!(candidate["rank"], index + 1);
        assert_eq!(candidate["source_packet_ref"], packet_ref);
        assert_eq!(candidate["source_start"], start);
        assert_eq!(candidate["source_end"], end);
        assert_eq!(candidate["text"], raw);
        assert_eq!(
            candidate["source_span_sha256"],
            minidora_domain::digest(raw.as_bytes())
        );
        assert_eq!(
            candidate["matched_query_terms"],
            serde_json::json!(["photosynthesis"])
        );
    }
    assert!(
        answer["text"]
            .as_str()
            .unwrap()
            .contains("Photosynthesis stores light energy.")
    );
    assert_eq!(
        std::fs::read(&output_path).unwrap(),
        answer["text"].as_str().unwrap().as_bytes()
    );
    assert_eq!(answer["receipts"][0]["phase"], "CONFIRMED");
    let graph: Value = serde_json::from_slice(
        &store
            .get_bytes(
                "owner",
                "normal-route",
                answer["execution_graph_ref"].as_str().unwrap(),
            )
            .unwrap(),
    )
    .unwrap();
    assert!(
        graph
            .to_string()
            .contains("previous-source-question-answer")
    );
    assert!(graph.to_string().contains(&packet_ref));

    let saved = inspect_normal_case(&scratch.0, "owner");
    let original_actions = store.actions("owner", "normal-route").unwrap();
    assert_eq!(original_actions.len(), 1);
    assert_eq!(
        original_actions[0].phase,
        minidora_domain::EffectPhase::Confirmed
    );
    let modified = std::fs::metadata(&output_path).unwrap().modified().unwrap();
    let explanation_args = ["--input", "source-question-explain", "詳しく説明して"];
    let explanation = record(&execute(&scratch.0, "owner", &explanation_args));
    assert_eq!(explanation["lifecycle"], "COMMIT");
    assert_eq!(explanation["guarantee"], "HYPOTHESIS");
    assert_eq!(explanation["content_plan"]["guarantee"], "HYPOTHESIS");
    assert_eq!(explanation["content_plan"]["text"], explanation["text"]);
    assert_eq!(explanation["receipts"], serde_json::json!([]));
    let mut expected_dependencies = vec![answer["id"].clone()];
    expected_dependencies.extend_from_slice(dependencies);
    assert_eq!(
        explanation["dependencies"],
        Value::Array(expected_dependencies)
    );
    let explanation_text = explanation["text"].as_str().unwrap();
    assert!(
        explanation_text.starts_with("前回の資料質問の根拠"),
        "{explanation_text}"
    );
    assert_eq!(
        explanation["render"]["actual_sha256"],
        minidora_domain::digest(explanation_text.as_bytes())
    );
    let lines: Vec<_> = explanation_text.lines().collect();
    assert!(
        lines.contains(&"照合語: photosynthesis"),
        "{explanation_text}"
    );
    assert_eq!(
        lines.iter().filter(|line| line.starts_with("候補")).count(),
        2
    );
    let mut previous_heading = None;
    for (index, &(start, end, raw)) in expected_candidates.iter().enumerate() {
        let heading = format!(
            "候補{} / 一致語: photosynthesis / 取得モード: FIXTURE / 出典: https://example.test/article",
            index + 1
        );
        let position = lines
            .iter()
            .position(|line| *line == heading)
            .unwrap_or_else(|| panic!("missing {heading:?} in {explanation_text}"));
        if let Some(previous) = previous_heading {
            assert!(previous < position);
        }
        previous_heading = Some(position);
        assert_eq!(
            lines[position + 3],
            format!(
                "  抽出本文byte[{start}..{end}) / 範囲SHA-256: {} / 原文候補: {raw:?}",
                minidora_domain::digest(raw.as_bytes())
            )
        );
    }
    assert_eq!(
        record(&execute(&scratch.0, "owner", &explanation_args)),
        explanation
    );
    let explained_saved = inspect_normal_case(&scratch.0, "owner");
    assert_eq!(explained_saved["result"], explanation);
    assert_eq!(
        explained_saved["bundle"]["meaning"]["parser_version"],
        "declarative-ja-en-v112-search-candidate-body-explanation"
    );
    assert_eq!(
        explained_saved["bundle"]["meaning"]["context_result_ref"],
        answer["id"]
    );
    assert_eq!(
        explained_saved["prior_results"][answer["id"].as_str().unwrap()],
        answer
    );
    assert_eq!(
        explained_saved["prior_results"][&source_result_id],
        saved["prior_results"][&source_result_id]
    );
    assert_eq!(
        store.actions("owner", "normal-route").unwrap(),
        original_actions
    );
    assert_eq!(
        std::fs::read(&output_path).unwrap(),
        answer["text"].as_str().unwrap().as_bytes()
    );
    assert_eq!(
        std::fs::metadata(&output_path).unwrap().modified().unwrap(),
        modified
    );
}

#[test]
fn natural_previous_answer_edit_consumes_and_chains_actual_rendered_output() {
    let scratch = Scratch::new();
    let original = record(&execute(
        &scratch.0,
        "owner",
        &["--input", "prior-calc", "2+3"],
    ));
    assert_eq!(original["lifecycle"], "COMMIT");
    let output_path = scratch.0.join("edited-answer.txt");
    let edited = record(&execute(
        &scratch.0,
        "owner",
        &[
            "--input",
            "replace-ja",
            "--output",
            output_path.to_str().unwrap(),
            "--allow-write",
            output_path.to_str().unwrap(),
            "前回の回答の「2+3」を「4+1」に置き換えてください",
        ],
    ));
    assert_eq!(edited["lifecycle"], "COMMIT");
    assert_eq!(edited["guarantee"], "USER_PREMISE");
    let original_text = original["text"].as_str().unwrap();
    assert_eq!(original_text.matches("2+3").count(), 1);
    let expected_edit = original_text.replacen("2+3", "4+1", 1);
    assert!(edited["text"].as_str().unwrap().contains(&expected_edit));
    assert!(
        edited["text"]
            .as_str()
            .unwrap()
            .contains("正確さは検証していません")
    );
    assert_eq!(
        std::fs::read(&output_path).unwrap(),
        edited["text"].as_str().unwrap().as_bytes()
    );
    assert_eq!(edited["receipts"][0]["phase"], "CONFIRMED");

    let store = minidora_engine::store::Store::open(&scratch.0).unwrap();
    let dependencies = edited["dependencies"].as_array().unwrap();
    assert_eq!(dependencies.len(), 4);
    assert_eq!(dependencies[0], original["id"]);
    let content_ref =
        minidora_domain::digest(&minidora_domain::canonical(&original["content_plan"]).unwrap());
    let render_ref =
        minidora_domain::digest(&minidora_domain::canonical(&original["render"]).unwrap());
    assert_eq!(dependencies[1], content_ref);
    assert_eq!(dependencies[2], render_ref);
    let edit_ref = dependencies[3].as_str().unwrap();
    let edit_bytes = store.get_bytes("owner", "normal-route", edit_ref).unwrap();
    assert_eq!(minidora_domain::digest(&edit_bytes), edit_ref);
    let artifact: Value = serde_json::from_slice(&edit_bytes).unwrap();
    assert_eq!(artifact["operation"], "PREVIOUS_OUTPUT_REPLACE_EXACT_V1");
    assert_eq!(artifact["source_result_id"], original["id"]);
    assert_eq!(artifact["source_content_plan_sha256"], content_ref);
    assert_eq!(artifact["source_render_sha256"], render_ref);
    assert_eq!(artifact["edit"]["source_document"], original["text"]);
    assert_eq!(artifact["edit"]["text"], expected_edit);
    assert_eq!(artifact["edit"]["spans"][0]["source_text"], "2+3");

    let graph: Value = serde_json::from_slice(
        &store
            .get_bytes(
                "owner",
                "normal-route",
                edited["execution_graph_ref"].as_str().unwrap(),
            )
            .unwrap(),
    )
    .unwrap();
    let graph_text = graph.to_string();
    for id in [
        "previous-text-content",
        "previous-text-render",
        "previous-text-replacement",
    ] {
        assert!(graph_text.contains(id));
    }
    assert!(graph_text.contains(&content_ref));
    assert!(graph_text.contains(&render_ref));
    assert!(graph_text.contains(edit_ref));

    let english = record(&execute(
        &scratch.0,
        "owner",
        &[
            "--input",
            "replace-en",
            "Replace \"4+1\" with \"2+3\" in the previous answer",
        ],
    ));
    assert_eq!(english["lifecycle"], "COMMIT");
    assert_eq!(english["guarantee"], "USER_PREMISE");
    assert!(english["text"].as_str().unwrap().contains("2+3 = 5"));
    assert_eq!(english["dependencies"][0], edited["id"]);
}

#[test]
fn natural_previous_answer_edit_suspends_for_missing_or_nonunique_literals() {
    let missing = Scratch::new();
    let no_prior = stopped(
        &execute(
            &missing.0,
            "owner",
            &[
                "--input",
                "replace-missing-context",
                "前回の回答の「古い」を「新しい」に置き換えて",
            ],
        ),
        2,
        "SUSPEND",
    );
    assert!(
        no_prior["reason"]
            .as_str()
            .unwrap()
            .contains("MISSING_ACTUAL_RESULT")
    );
    assert!(no_prior["dependencies"].as_array().unwrap().is_empty());

    let absent = Scratch::new();
    let _ = record(&execute(
        &absent.0,
        "owner",
        &["--input", "prior-calc", "2+3"],
    ));
    let not_found = stopped(
        &execute(
            &absent.0,
            "owner",
            &[
                "--input",
                "replace-absent",
                "前回の回答の「存在しない」を「追加」に置き換えて",
            ],
        ),
        2,
        "SUSPEND",
    );
    assert!(
        not_found["reason"]
            .as_str()
            .unwrap()
            .contains("PREVIOUS_TEXT_NOT_FOUND")
    );
    assert!(not_found["dependencies"].as_array().unwrap().is_empty());

    let ambiguous = Scratch::new();
    let _ = record(&execute(
        &ambiguous.0,
        "owner",
        &["--input", "prior-calc", "2+2"],
    ));
    let multiple = stopped(
        &execute(
            &ambiguous.0,
            "owner",
            &[
                "--input",
                "replace-ambiguous",
                "前回の回答の「2」を「3」に置き換えて",
            ],
        ),
        2,
        "SUSPEND",
    );
    assert!(
        multiple["reason"]
            .as_str()
            .unwrap()
            .contains("PREVIOUS_TEXT_AMBIGUOUS")
    );
    assert!(multiple["dependencies"].as_array().unwrap().is_empty());
}

#[test]
fn natural_source_question_consumes_every_retained_candidate_body_in_order() {
    let scratch = Scratch::new();
    let sources = [
        (
            "https://ja.wikipedia.org/wiki/First?action=raw",
            "Photosynthesis stores light energy.",
        ),
        (
            "https://ja.wikipedia.org/wiki/Second?action=raw",
            "Photosynthesis converts sunlight into chemical energy.",
        ),
    ];
    let (source_result_id, packet_refs, texts) =
        seed_fetched_source_case_with_sources(&scratch.0, "owner", &sources);
    let answer = record(&execute(
        &scratch.0,
        "owner",
        &[
            "--input",
            "source-question-multiple",
            "What does the source say about photosynthesis?",
        ],
    ));
    assert_eq!(answer["lifecycle"], "COMMIT");
    assert_eq!(answer["guarantee"], "HYPOTHESIS");

    let dependencies = answer["dependencies"].as_array().unwrap();
    assert_eq!(dependencies[0], source_result_id);
    assert_eq!(dependencies[1].as_str(), Some(packet_refs[0].as_str()));
    assert_eq!(dependencies[2].as_str(), Some(packet_refs[1].as_str()));
    let answer_ref = dependencies[3].as_str().unwrap();
    let store = minidora_engine::store::Store::open(&scratch.0).unwrap();
    let answer_bytes = store
        .get_bytes("owner", "normal-route", answer_ref)
        .unwrap();
    assert_eq!(minidora_domain::digest(&answer_bytes), answer_ref);
    let typed: Value = serde_json::from_slice(&answer_bytes).unwrap();
    assert_eq!(
        typed["operation"],
        "SOURCE_QUESTION_VERSIONED_EXTRACTIVE_CANDIDATES_V2"
    );
    assert_eq!(typed["source_packet_count"], 2);
    assert_eq!(typed["active_source_count"], 2);
    assert_eq!(typed["superseded_version_count"], 0);
    assert_eq!(typed["matched_sentence_count"], 2);
    assert_eq!(typed["candidates"].as_array().unwrap().len(), 2);
    let expected_sentences = [
        "Photosynthesis stores light energy.",
        "Photosynthesis converts sunlight into chemical energy.",
    ];
    for (index, candidate) in typed["candidates"].as_array().unwrap().iter().enumerate() {
        let packet_ref = &packet_refs[index];
        let packet_bytes = store
            .get_bytes("owner", "normal-route", packet_ref)
            .unwrap();
        let packet: Value = serde_json::from_slice(&packet_bytes).unwrap();
        assert_eq!(
            candidate["source_packet_ref"].as_str(),
            Some(packet_ref.as_str())
        );
        assert_eq!(
            candidate["source_packet_id"].as_str(),
            packet["id"].as_str()
        );
        assert_eq!(candidate["source_mode"], "FIXTURE");
        assert_eq!(candidate["source_final_uri"], sources[index].0);
        let body_sha = minidora_domain::digest(texts[index].as_bytes());
        assert_eq!(candidate["raw_body_sha256"], body_sha);
        assert_eq!(candidate["extracted_text_sha256"], body_sha);
        assert_eq!(candidate["text"], expected_sentences[index]);
        let start = candidate["source_start"].as_u64().unwrap() as usize;
        let end = candidate["source_end"].as_u64().unwrap() as usize;
        assert_eq!(&texts[index][start..end], expected_sentences[index]);
    }
    assert!(
        answer["text"]
            .as_str()
            .unwrap()
            .contains("Photosynthesis stores light energy.")
    );
    assert!(
        answer["text"]
            .as_str()
            .unwrap()
            .contains("Photosynthesis converts sunlight into chemical energy.")
    );
    let graph: Value = serde_json::from_slice(
        &store
            .get_bytes(
                "owner",
                "normal-route",
                answer["execution_graph_ref"].as_str().unwrap(),
            )
            .unwrap(),
    )
    .unwrap();
    for packet_ref in &packet_refs {
        assert!(graph.to_string().contains(packet_ref));
    }
}

#[test]
fn natural_source_question_uses_latest_body_version_for_the_same_final_uri() {
    let scratch = Scratch::new();
    let sources = [
        (
            "https://example.test/article",
            "Photosynthesis was thought to store light energy.",
        ),
        (
            "https://example.test/article",
            "Photosynthesis stores chemical energy from light.",
        ),
    ];
    let (source_result_id, packet_refs, texts) =
        seed_fetched_source_case_with_sources(&scratch.0, "owner", &sources);
    let output_path = scratch.0.join("versioned-source-answer.txt");
    let answer = record(&execute(
        &scratch.0,
        "owner",
        &[
            "--input",
            "source-question-versioned",
            "--output",
            output_path.to_str().unwrap(),
            "--allow-write",
            output_path.to_str().unwrap(),
            "What does the source say about photosynthesis?",
        ],
    ));
    assert_eq!(answer["lifecycle"], "COMMIT");
    assert_eq!(answer["guarantee"], "HYPOTHESIS");
    let dependencies = answer["dependencies"].as_array().unwrap();
    assert_eq!(dependencies[0], source_result_id);
    assert_eq!(dependencies[1], packet_refs[0]);
    assert_eq!(dependencies[2], packet_refs[1]);

    let answer_ref = dependencies[3].as_str().unwrap();
    let store = minidora_engine::store::Store::open(&scratch.0).unwrap();
    let answer_bytes = store
        .get_bytes("owner", "normal-route", answer_ref)
        .unwrap();
    assert_eq!(minidora_domain::digest(&answer_bytes), answer_ref);
    let typed: Value = serde_json::from_slice(&answer_bytes).unwrap();
    assert_eq!(
        typed["operation"],
        "SOURCE_QUESTION_VERSIONED_EXTRACTIVE_CANDIDATES_V2"
    );
    assert_eq!(typed["source_packet_count"], 2);
    assert_eq!(typed["active_source_count"], 1);
    assert_eq!(typed["superseded_version_count"], 1);
    assert_eq!(typed["matched_sentence_count"], 1);
    assert_eq!(typed["candidates"].as_array().unwrap().len(), 1);

    let source_version = &typed["source_versions"][0];
    assert_eq!(source_version["source_final_uri"], sources[0].0);
    assert_eq!(source_version["active_packet_ref"], packet_refs[1]);
    assert_eq!(source_version["active_observed_at_unix_ms"], 2);
    assert_eq!(
        source_version["active_raw_body_sha256"],
        minidora_domain::digest(texts[1].as_bytes())
    );
    assert_eq!(
        source_version["active_version_packet_refs"],
        serde_json::json!([packet_refs[1]])
    );
    assert_eq!(
        source_version["superseded_versions"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        source_version["superseded_versions"][0]["raw_body_sha256"],
        minidora_domain::digest(texts[0].as_bytes())
    );
    assert_eq!(
        source_version["superseded_versions"][0]["packet_refs"],
        serde_json::json!([packet_refs[0]])
    );
    assert_eq!(typed["candidates"][0]["source_packet_ref"], packet_refs[1]);
    assert_eq!(typed["candidates"][0]["text"], texts[1]);
    let output_text = answer["text"].as_str().unwrap();
    assert!(output_text.contains(texts[1].as_str()));
    assert!(!output_text.contains(texts[0].as_str()));
    assert!(output_text.contains("旧本文版は履歴に保持"));
    assert_eq!(std::fs::read(&output_path).unwrap(), output_text.as_bytes());
    assert_eq!(answer["receipts"][0]["phase"], "CONFIRMED");

    let graph: Value = serde_json::from_slice(
        &store
            .get_bytes(
                "owner",
                "normal-route",
                answer["execution_graph_ref"].as_str().unwrap(),
            )
            .unwrap(),
    )
    .unwrap();
    assert!(graph.to_string().contains(&packet_refs[0]));
    assert!(graph.to_string().contains(&packet_refs[1]));
}

#[test]
fn natural_source_question_suspends_without_prior_body_or_topic_match() {
    let missing = Scratch::new();
    let no_prior = stopped(
        &execute(
            &missing.0,
            "owner",
            &[
                "--input",
                "source-question",
                "What does the source say about photosynthesis?",
            ],
        ),
        2,
        "SUSPEND",
    );
    assert!(
        no_prior["reason"]
            .as_str()
            .unwrap()
            .contains("MISSING_ACTUAL_RESULT")
    );

    let unrelated = Scratch::new();
    let _ = record(&execute(
        &unrelated.0,
        "owner",
        &["--input", "prior-calculation", "2+3"],
    ));
    let no_body = stopped(
        &execute(
            &unrelated.0,
            "owner",
            &[
                "--input",
                "source-question",
                "What does the source say about photosynthesis?",
            ],
        ),
        2,
        "SUSPEND",
    );
    assert!(
        no_body["reason"]
            .as_str()
            .unwrap()
            .contains("SOURCE_BODY_UNAVAILABLE")
    );

    let unmatched = Scratch::new();
    let _ = seed_fetched_source_case(&unmatched.0, "owner");
    let no_topic_match = stopped(
        &execute(
            &unmatched.0,
            "owner",
            &[
                "--input",
                "source-question",
                "What does the source say about volcanos?",
            ],
        ),
        2,
        "SUSPEND",
    );
    assert!(
        no_topic_match["reason"]
            .as_str()
            .unwrap()
            .contains("SOURCE_QUESTION_NO_MATCH")
    );
    assert!(
        no_topic_match["dependencies"]
            .as_array()
            .unwrap()
            .is_empty()
    );
}

#[test]
fn constrained_previous_source_summary_binds_one_actual_packet_and_stops_on_ambiguity() {
    let no_prior = Scratch::new();
    let result = stopped(
        &execute(
            &no_prior.0,
            "owner",
            &[
                "--input",
                "source-summary",
                "前回の資料を2点以内、10字以内で要約して",
            ],
        ),
        2,
        "SUSPEND",
    );
    assert!(
        result["reason"]
            .as_str()
            .unwrap()
            .contains("MISSING_ACTUAL_RESULT")
    );
    assert_eq!(result["dependencies"], serde_json::json!([]));

    let no_body = Scratch::new();
    record(&execute(&no_body.0, "owner", &["--input", "prior", "2+3"]));
    let result = stopped(
        &execute(
            &no_body.0,
            "owner",
            &[
                "--input",
                "source-summary",
                "前回の資料を2点以内、10字以内で要約して",
            ],
        ),
        2,
        "SUSPEND",
    );
    assert!(
        result["reason"]
            .as_str()
            .unwrap()
            .contains("SOURCE_BODY_UNAVAILABLE")
    );
    assert_eq!(result["dependencies"], serde_json::json!([]));

    let scratch = Scratch::new();
    let source_text = "First source sentence. Short.";
    let (prior_id, packet_refs, texts) = seed_fetched_source_case_with_sources(
        &scratch.0,
        "owner",
        &[("https://example.test/one", source_text)],
    );
    let result = record(&execute(
        &scratch.0,
        "owner",
        &[
            "--input",
            "source-summary",
            "前回の資料を2点以内、10字以内で要約して",
        ],
    ));
    assert_eq!(result["lifecycle"], "COMMIT");
    assert_eq!(result["guarantee"], "HYPOTHESIS");
    assert!(result["text"].as_str().unwrap().contains("Short."));
    assert!(
        !result["text"]
            .as_str()
            .unwrap()
            .contains("First source sentence.")
    );
    assert_eq!(result["dependencies"].as_array().unwrap().len(), 3);
    assert_eq!(result["dependencies"][0], prior_id);
    assert_eq!(result["dependencies"][1], packet_refs[0]);

    let saved = inspect_normal_case(&scratch.0, "owner");
    assert_eq!(
        saved["bundle"]["meaning"]["intent"]["kind"],
        "SUMMARIZE_PREVIOUS_SOURCE_CONSTRAINED"
    );
    assert_eq!(saved["bundle"]["meaning"]["intent"]["max_sentences"], 2);
    assert_eq!(saved["bundle"]["meaning"]["intent"]["max_chars"], 10);
    let store = minidora_engine::store::Store::open(&scratch.0).unwrap();
    let report = sequence_artifact(&store, &result["dependencies"][2]);
    assert_eq!(report["source_result_id"], prior_id);
    assert_eq!(report["source_packet_ref"], packet_refs[0]);
    assert_eq!(report["summary"]["source_text"], texts[0]);
    assert_eq!(report["summary"]["max_sentences"], 2);
    assert_eq!(report["summary"]["max_chars"], 10);
    assert_eq!(
        report["summary"]["selected_sentence_indices"],
        serde_json::json!([1])
    );
    assert_eq!(report["summary"]["selected_character_count"], 6);
    assert_eq!(report["summary"]["coverage_verified"], false);

    const LABEL: &str = "抽出候補です。文の頻度と位置で選択しました。目的・意味・条件・否定の網羅は検証していません。";
    let summary_body = format!("{LABEL}\n- Short.");
    let output_start = LABEL.len() + "\n- ".len();
    let output_end = summary_body.len();
    assert_eq!(&source_text[23..29], "Short.");
    assert_eq!(&summary_body[output_start..output_end], "Short.");
    assert_eq!(report["summary"]["text"], summary_body);
    let sentence = &report["summary"]["sentences"][0];
    assert_eq!(report["summary"]["sentences"].as_array().unwrap().len(), 1);
    assert_eq!(sentence["sentence_index"], 1);
    assert_eq!(sentence["selection_rank"], 1);
    assert_eq!(sentence["source_start"], 23);
    assert_eq!(sentence["source_end"], 29);
    assert_eq!(sentence["output_start"], output_start);
    assert_eq!(sentence["output_end"], output_end);
    assert_eq!(sentence["text"], "Short.");
    let explanation_args = ["--input", "source-summary-explain", "詳しく説明して"];
    let explanation = record(&execute(&scratch.0, "owner", &explanation_args));
    assert_eq!(explanation["lifecycle"], "COMMIT");
    assert_eq!(explanation["guarantee"], "HYPOTHESIS");
    assert_eq!(explanation["content_plan"]["guarantee"], "HYPOTHESIS");
    assert_eq!(explanation["content_plan"]["text"], explanation["text"]);
    assert_eq!(explanation["receipts"], serde_json::json!([]));
    let mut dependencies = vec![result["id"].clone()];
    dependencies.extend_from_slice(result["dependencies"].as_array().unwrap());
    assert_eq!(explanation["dependencies"], Value::Array(dependencies));
    let text = explanation["text"].as_str().unwrap();
    assert!(text.starts_with("前回の取得資料要約の根拠"), "{text}");
    for expected in [
        "取得モード: FIXTURE / 取得時刻（Unix ms）: 1".to_string(),
        "候補文2件、選択1件、未選択1件。文数上限2、文字数上限10。".to_string(),
        format!(
            "原本文SHA-256: {}",
            minidora_domain::digest(source_text.as_bytes())
        ),
        format!(
            "抽出本文SHA-256: {}",
            minidora_domain::digest(source_text.as_bytes())
        ),
        format!(
            "選択文2（順位1、抽出本文byte 23..29、要約本文byte {output_start}..{output_end}）: {:?}",
            "Short."
        ),
    ] {
        assert!(
            text.lines().any(|line| line == expected),
            "missing {expected:?} in {text}"
        );
    }
    assert_eq!(
        record(&execute(&scratch.0, "owner", &explanation_args)),
        explanation
    );
    let explained_saved = inspect_normal_case(&scratch.0, "owner");
    assert_eq!(explained_saved["result"], explanation);
    assert_eq!(
        explained_saved["bundle"]["meaning"]["parser_version"],
        "declarative-ja-en-v112-search-candidate-body-explanation"
    );
    assert_eq!(
        explained_saved["bundle"]["meaning"]["context_result_ref"],
        result["id"]
    );
    assert_eq!(
        explained_saved["prior_results"][result["id"].as_str().unwrap()],
        result
    );
    assert_eq!(
        explained_saved["prior_results"][&prior_id],
        saved["prior_results"][&prior_id]
    );
    assert!(store.actions("owner", "normal-route").unwrap().is_empty());

    let ambiguous = Scratch::new();
    seed_fetched_source_case_with_sources(
        &ambiguous.0,
        "owner",
        &[
            ("https://example.test/one", "First source sentence."),
            ("https://example.test/two", "Second source sentence."),
        ],
    );
    let result = stopped(
        &execute(
            &ambiguous.0,
            "owner",
            &[
                "--input",
                "source-summary",
                "前回の資料を2点以内、10字以内で要約して",
            ],
        ),
        2,
        "SUSPEND",
    );
    assert!(
        result["reason"]
            .as_str()
            .unwrap()
            .contains("AMBIGUOUS_SOURCE_CONTEXT")
    );
    assert_eq!(result["dependencies"], serde_json::json!([]));

    let legacy = Scratch::new();
    seed_fetched_source_case_with_sources(
        &legacy.0,
        "owner",
        &[(
            "https://example.test/legacy",
            "First sentence. Second sentence.",
        )],
    );
    let result = record(&execute(
        &legacy.0,
        "owner",
        &["--input", "source-summary", "前回の資料を要約して"],
    ));
    assert_eq!(result["lifecycle"], "COMMIT");
    let saved = inspect_normal_case(&legacy.0, "owner");
    assert_eq!(
        saved["bundle"]["meaning"]["intent"]["kind"],
        "SUMMARIZE_PREVIOUS_SOURCE"
    );
    let store = minidora_engine::store::Store::open(&legacy.0).unwrap();
    let report = sequence_artifact(&store, &result["dependencies"][2]);
    assert_eq!(report["summary"]["max_sentences"], 3);
    assert_eq!(report["summary"]["max_chars"], 100_000);
    assert_eq!(report["summary"]["sentence_count"], 2);
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

struct LedgerSourceFixture {
    raw_id: &'static str,
    raw_group: &'static str,
    claims: Vec<(&'static str, &'static str, &'static str)>,
}

fn ledger_source_fixtures() -> Vec<LedgerSourceFixture> {
    vec![
        LedgerSourceFixture {
            raw_id: r#""資料/甲~0""#,
            raw_group: r#""出版社 A""#,
            claims: vec![
                (r#""z-single""#, r#""雨\n仮説""#, "REFUTES"),
                (r#""a-agree""#, r#""橋は開いた。""#, "SUPPORTS"),
                (r#""b-repeat""#, r#""同じ観測。""#, "SUPPORTS"),
                (r#""c-conflict""#, r#""日時は未確定。""#, "SUPPORTS"),
                (r#""d-refute""#, r#""水温は10度。""#, "REFUTES"),
            ],
        },
        LedgerSourceFixture {
            raw_id: r#""資料乙""#,
            raw_group: r#""\u51fa版社 A""#,
            claims: vec![
                (r#""b-repeat""#, r#""\u540cじ観測。""#, "SUPPORTS"),
                (r#""\u0061-agree""#, r#""\u6a4bは開いた。""#, "SUPPORTS"),
                (r#""c-conflict""#, r#""日時は未確定。""#, "REFUTES"),
                (r#""d-refute""#, r#""水温は10度。""#, "REFUTES"),
            ],
        },
        LedgerSourceFixture {
            raw_id: r#""資料丙""#,
            raw_group: r#""独立B""#,
            claims: vec![
                (r#""a-agree""#, r#""橋は開いた。""#, "SUPPORTS"),
                (r#""c-conflict""#, r#""日時は未確定。""#, "SUPPORTS"),
                (r#""d-refute""#, r#""水温は10度。""#, "REFUTES"),
            ],
        },
        LedgerSourceFixture {
            raw_id: r#""資料丁""#,
            raw_group: r#""独立C""#,
            claims: vec![(r#""c-conflict""#, r#""日時は未確定。""#, "SUPPORTS")],
        },
    ]
}

fn ledger_claim_json(raw_id: &str, raw_text: &str, stance: &str) -> String {
    format!(r#"{{"id":{raw_id},"text":{raw_text},"stance":"{stance}"}}"#)
}

fn ledger_source_json(source: &LedgerSourceFixture) -> String {
    let claims = source
        .claims
        .iter()
        .map(|(id, text, stance)| ledger_claim_json(id, text, stance))
        .collect::<Vec<_>>()
        .join(",");
    format!(
        r#"{{"source_id":{},"independence_group":{},"claims":[{claims}]}}"#,
        source.raw_id, source.raw_group
    )
}

const LEDGER_INFERENCE_1: &str =
    r#"{"id":"i-contradiction","text":"争点があると申告。","premises":["c-conflict"]}"#;
const LEDGER_INFERENCE_2: &str = r#"{"id":"i-chain","text":"前提は全て真だと申告。","premises":["\u0069-contradiction","d-refute"]}"#;

fn ledger_document() -> String {
    let sources = ledger_source_fixtures()
        .iter()
        .map(ledger_source_json)
        .collect::<Vec<_>>()
        .join(",");
    format!(
        " \r\n{{\"sources\":[{sources}],\"inferences\":[{LEDGER_INFERENCE_1},{LEDGER_INFERENCE_2}]}} \t\n"
    )
}

fn ledger_decode(raw: &str) -> String {
    serde_json::from_str(raw).unwrap()
}

fn assert_ledger_report(report: &Value, document: &str) {
    assert_eq!(report["source_count"], 4);
    assert_eq!(report["evidence_record_count"], 13);
    assert_eq!(
        report["independence_basis"],
        "independence_group labels are caller-supplied and not independently verified"
    );
    assert_eq!(
        report["guarantee"],
        "explicit evidence ledger classification only; source identities, claim text, stances and inference links are caller-supplied; no source fact extraction, claim truth or inference validity is asserted"
    );
    let fixtures = ledger_source_fixtures();
    let containers = fixtures.iter().map(ledger_source_json).collect::<Vec<_>>();
    let expected_sources=fixtures.iter().zip(&containers).map(|(source,container)| {
        serde_json::json!({
            "source_id":ledger_decode(source.raw_id),"independence_group":ledger_decode(source.raw_group),
            "source_id_span":comparison_fixture_span(document,container,&format!(r#""source_id":{}"#,source.raw_id),source.raw_id),
            "independence_group_span":comparison_fixture_span(document,container,&format!(r#""independence_group":{}"#,source.raw_group),source.raw_group)
        })
    }).collect::<Vec<_>>();
    assert_eq!(report["sources"], serde_json::json!(expected_sources));
    let citation = |index: usize, id: &str| {
        let (raw_id, raw_text, stance) = fixtures[index]
            .claims
            .iter()
            .find(|(raw_id, _, _)| ledger_decode(raw_id) == id)
            .unwrap();
        let fragment = ledger_claim_json(raw_id, raw_text, stance);
        serde_json::json!({
            "source_index":index,
            "claim_text_span":comparison_fixture_span(document,&containers[index],&fragment,raw_text),
            "stance_span":comparison_fixture_span(document,&containers[index],&fragment,&serde_json::to_string(stance).unwrap())
        })
    };
    let expected_claims=[
        ("a-agree","橋は開いた。","AGREEMENT",Some("SUPPORTS"),2,0,vec![0,1,2],vec![]),
        ("b-repeat","同じ観測。","SAME_GROUP_REPETITION",Some("SUPPORTS"),1,0,vec![0,1],vec![]),
        ("c-conflict","日時は未確定。","CONTRADICTION",None,3,1,vec![0,2,3],vec![1]),
        ("d-refute","水温は10度。","AGREEMENT",Some("REFUTES"),0,2,vec![],vec![0,1,2]),
        ("z-single","雨\n仮説","SINGLE_SOURCE",Some("REFUTES"),0,1,vec![],vec![0]),
    ].into_iter().map(|(id,text,status,consensus,support_groups,refute_groups,support,refute)|serde_json::json!({
        "claim_id":id,"text":text,"status":status,"consensus_stance":consensus,
        "supporting_independence_group_count":support_groups,"refuting_independence_group_count":refute_groups,
        "supporting":support.into_iter().map(|index|citation(index,id)).collect::<Vec<_>>(),
        "refuting":refute.into_iter().map(|index|citation(index,id)).collect::<Vec<_>>()
    })).collect::<Vec<_>>();
    assert_eq!(report["claims"], serde_json::json!(expected_claims));
    let expected_inferences=[
        (LEDGER_INFERENCE_1,r#""i-contradiction""#,r#""争点があると申告。""#,vec![r#""c-conflict""#]),
        (LEDGER_INFERENCE_2,r#""i-chain""#,r#""前提は全て真だと申告。""#,vec![r#""\u0069-contradiction""#,r#""d-refute""#]),
    ].into_iter().map(|(container,id,text,premises)|{
        let premise_fragment=format!(r#""premises":[{}]"#,premises.join(","));
        serde_json::json!({
            "inference_id":ledger_decode(id),"text":ledger_decode(text),
            "premise_ids":premises.iter().map(|raw|ledger_decode(raw)).collect::<Vec<_>>(),
            "classification":"USER_DECLARED_INFERENCE_NOT_VALIDATED",
            "inference_id_span":comparison_fixture_span(document,container,&format!(r#""id":{id}"#),id),
            "text_span":comparison_fixture_span(document,container,&format!(r#""text":{text}"#),text),
            "premise_spans":premises.iter().map(|raw|comparison_fixture_span(document,container,&premise_fragment,raw)).collect::<Vec<_>>()
        })
    }).collect::<Vec<_>>();
    assert_eq!(
        report["declared_inferences"],
        serde_json::json!(expected_inferences)
    );
}

#[test]
fn evidence_ledger_normal_routes_preserve_supplied_classifications_and_all_reported_source_maps() {
    let document = ledger_document();
    for header in [
        "証拠を整理して:",
        "evidence-synthesize:",
        "EvIdEnCe-SyNtHeSiZe:",
    ] {
        let scratch = Scratch::new();
        let request = format!("{header}\n{document}");
        let result = record(&execute(&scratch.0, "owner", &[&request]));
        let report = normal_row_report(&scratch.0, "owner", &result, &request, &document);
        assert_ledger_report(&report, &document);
    }
}

fn ledger_pair(groups: [&str; 2], stances: [&str; 2], different_ids: bool) -> Value {
    serde_json::json!({"sources":[
        {"source_id":"s0","independence_group":groups[0],"claims":[{"id":if different_ids {"a"} else {"c"},"text":"猫の記録。","stance":stances[0]}]},
        {"source_id":"s1","independence_group":groups[1],"claims":[{"id":if different_ids {"b"} else {"c"},"text":"猫の記録。","stance":stances[1]}]}
    ],"inferences":[]})
}

#[test]
fn evidence_ledger_independence_labels_and_claim_identity_are_exact_unverified_metadata() {
    for (groups, stances, different_ids, status, consensus, support_count, refute_count) in [
        (
            ["同群", "同群"],
            ["SUPPORTS", "SUPPORTS"],
            false,
            "SAME_GROUP_REPETITION",
            Some("SUPPORTS"),
            1,
            0,
        ),
        (
            ["同群", "同群 "],
            ["SUPPORTS", "SUPPORTS"],
            false,
            "AGREEMENT",
            Some("SUPPORTS"),
            2,
            0,
        ),
        (
            ["Group", "group"],
            ["SUPPORTS", "SUPPORTS"],
            false,
            "AGREEMENT",
            Some("SUPPORTS"),
            2,
            0,
        ),
        (
            ["é", "e\u{301}"],
            ["SUPPORTS", "SUPPORTS"],
            false,
            "AGREEMENT",
            Some("SUPPORTS"),
            2,
            0,
        ),
        (
            ["同群", "同群"],
            ["SUPPORTS", "REFUTES"],
            false,
            "CONTRADICTION",
            None,
            1,
            1,
        ),
        (
            ["同群", "同群"],
            ["REFUTES", "REFUTES"],
            false,
            "SAME_GROUP_REPETITION",
            Some("REFUTES"),
            0,
            1,
        ),
        (
            ["A", "B"],
            ["SUPPORTS", "SUPPORTS"],
            true,
            "SINGLE_SOURCE",
            Some("SUPPORTS"),
            1,
            0,
        ),
    ] {
        let scratch = Scratch::new();
        let document = ledger_pair(groups, stances, different_ids).to_string();
        let request = format!("evidence-synthesize:\n{document}");
        let result = record(&execute(&scratch.0, "owner", &[&request]));
        let report = normal_row_report(&scratch.0, "owner", &result, &request, &document);
        assert_eq!(report["source_count"], 2);
        assert_eq!(report["evidence_record_count"], 2);
        assert_eq!(report["sources"][0]["independence_group"], groups[0]);
        assert_eq!(report["sources"][1]["independence_group"], groups[1]);
        assert_eq!(report["declared_inferences"], serde_json::json!([]));
        let claims = report["claims"].as_array().unwrap();
        assert_eq!(claims.len(), if different_ids { 2 } else { 1 });
        for (index, claim) in claims.iter().enumerate() {
            assert_eq!(
                claim["claim_id"],
                if different_ids {
                    ["a", "b"][index]
                } else {
                    "c"
                }
            );
            assert_eq!(claim["text"], "猫の記録。");
            assert_eq!(claim["status"], status);
            assert_eq!(claim["consensus_stance"], serde_json::json!(consensus));
            assert_eq!(claim["supporting_independence_group_count"], support_count);
            assert_eq!(claim["refuting_independence_group_count"], refute_count);
            let supports = claim["supporting"]
                .as_array()
                .unwrap()
                .iter()
                .map(|c| c["source_index"].as_u64().unwrap())
                .collect::<Vec<_>>();
            let refutes = claim["refuting"]
                .as_array()
                .unwrap()
                .iter()
                .map(|c| c["source_index"].as_u64().unwrap())
                .collect::<Vec<_>>();
            assert_eq!(
                supports,
                if different_ids {
                    vec![index as u64]
                } else {
                    (0..2)
                        .filter(|i| stances[*i] == "SUPPORTS")
                        .map(|i| i as u64)
                        .collect()
                }
            );
            assert_eq!(
                refutes,
                (0..2)
                    .filter(|i| stances[*i] == "REFUTES")
                    .map(|i| i as u64)
                    .collect::<Vec<_>>()
            );
        }
        assert!(
            report["independence_basis"]
                .as_str()
                .unwrap()
                .contains("not independently verified")
        );
    }
}

#[test]
fn evidence_ledger_actual_report_projection_retains_conflicts_and_unvalidated_inference_labels() {
    let document = ledger_document();
    for header in ["証拠を整理して:", "evidence-synthesize:"] {
        let scratch = Scratch::new();
        let request = format!("{header}\n{document}");
        let result = record(&execute(&scratch.0, "owner", &[&request]));
        let report = normal_row_report(&scratch.0, "owner", &result, &request, &document);
        assert_ledger_report(&report, &document);
        let actual = result["text"].as_str().unwrap();
        let next = format!("json-object-project [\"declared_inferences\",\"claims\"]:\n{actual}");
        let projected = record(&execute(&scratch.0, "owner", &[&next]));
        let artifact = normal_object_projection(&scratch.0, "owner", &projected, &next, actual);
        assert_object_projection_maps(
            &artifact,
            actual,
            &[
                (
                    "declared_inferences",
                    r#""declared_inferences""#,
                    "/declared_inferences",
                ),
                ("claims", r#""claims""#, "/claims"),
            ],
        );
        assert_eq!(
            serde_json::from_str::<Value>(projected["text"].as_str().unwrap()).unwrap(),
            serde_json::json!({"declared_inferences":report["declared_inferences"],"claims":report["claims"]})
        );
        let selected = projected["text"].as_str().unwrap();
        for (pointer, expected) in [
            ("/claims/2/status", r#""CONTRADICTION""#),
            (
                "/declared_inferences/1/classification",
                r#""USER_DECLARED_INFERENCE_NOT_VALIDATED""#,
            ),
        ] {
            let pointer_request = format!("json-pointer {pointer}\n{selected}");
            let value = record(&execute(&scratch.0, "owner", &[&pointer_request]));
            basic_raw_result(&scratch.0, &value, &pointer_request, Some(selected));
            assert_eq!(value["text"], expected);
            assert_eq!(value["dependencies"], serde_json::json!([]));
        }
    }
}

#[test]
fn evidence_ledger_invalid_schemas_identities_and_inference_links_stop_durably() {
    let mut base = ledger_pair(["A", "B"], ["SUPPORTS", "SUPPORTS"], false);
    base["inferences"] = serde_json::json!([
        {"id":"i1","text":"申告した推論。","premises":["c"]},
        {"id":"i2","text":"後続の申告。","premises":["i1"]}
    ]);
    let mut cases = Vec::new();
    let mut bad = base.clone();
    bad["extra"] = serde_json::json!(0);
    cases.push((bad, "EVIDENCE_LEDGER_SHAPE"));
    let mut bad = base.clone();
    bad["sources"].as_array_mut().unwrap().truncate(1);
    cases.push((bad, "EVIDENCE_LEDGER_SOURCE_LIMIT"));
    let mut bad = base.clone();
    bad["sources"][0]["extra"] = serde_json::json!(0);
    cases.push((bad, "EVIDENCE_SOURCE_SHAPE"));
    let mut bad = base.clone();
    bad["sources"][0]["source_id"] = serde_json::json!(" ");
    cases.push((bad, "EVIDENCE_SOURCE_ID"));
    let mut bad = base.clone();
    bad["sources"][0]["independence_group"] = serde_json::json!(1);
    cases.push((bad, "EVIDENCE_INDEPENDENCE_GROUP"));
    let mut bad = base.clone();
    bad["sources"][1]["source_id"] = serde_json::json!("s0");
    cases.push((bad, "EVIDENCE_DUPLICATE_SOURCE_ID"));
    let mut bad = base.clone();
    bad["sources"][0]["claims"] = serde_json::json!([]);
    cases.push((bad, "EVIDENCE_SOURCE_NO_CLAIMS"));
    let mut bad = base.clone();
    let duplicate = bad["sources"][0]["claims"][0].clone();
    bad["sources"][0]["claims"]
        .as_array_mut()
        .unwrap()
        .push(duplicate);
    cases.push((bad, "EVIDENCE_DUPLICATE_SOURCE_CLAIM"));
    let mut bad = base.clone();
    bad["sources"][1]["claims"][0]["text"] = serde_json::json!("異なる主張。");
    cases.push((bad, "EVIDENCE_CLAIM_LABEL_CONFLICT"));
    let mut bad = base.clone();
    bad["sources"][0]["claims"][0]["stance"] = serde_json::json!("UNKNOWN");
    cases.push((bad, "EVIDENCE_STANCE"));
    let mut bad = base.clone();
    bad["sources"][0]["claims"][0]["stance"] = serde_json::json!("supports");
    cases.push((bad, "EVIDENCE_STANCE"));
    let mut bad = base.clone();
    bad["sources"][0]["claims"][0]["text"] = serde_json::json!(" ");
    cases.push((bad, "EVIDENCE_CLAIM_TEXT"));
    let mut bad = base.clone();
    bad["sources"][0]["claims"][0]["extra"] = serde_json::json!(0);
    cases.push((bad, "EVIDENCE_CLAIM_SHAPE"));
    let mut bad = base.clone();
    bad["inferences"][0]["id"] = serde_json::json!("c");
    cases.push((bad, "EVIDENCE_DUPLICATE_INFERENCE_ID"));
    let mut bad = base.clone();
    bad["inferences"][0]["id"] = serde_json::json!("s0");
    cases.push((bad, "EVIDENCE_DUPLICATE_INFERENCE_ID"));
    let mut bad = base.clone();
    bad["inferences"][1]["id"] = serde_json::json!("i1");
    cases.push((bad, "EVIDENCE_DUPLICATE_INFERENCE_ID"));
    let mut bad = base.clone();
    bad["inferences"][0]["premises"] = serde_json::json!([]);
    cases.push((bad, "EVIDENCE_INFERENCE_PREMISES"));
    let mut bad = base.clone();
    bad["inferences"][0]["premises"] = serde_json::json!(["absent"]);
    cases.push((bad, "EVIDENCE_INFERENCE_PREMISE"));
    let mut bad = base.clone();
    bad["inferences"][0]["premises"] = serde_json::json!(["s0"]);
    cases.push((bad, "EVIDENCE_INFERENCE_PREMISE"));
    let mut bad = base.clone();
    bad["inferences"][0]["premises"] = serde_json::json!(["i1"]);
    cases.push((bad, "EVIDENCE_INFERENCE_PREMISE"));
    let mut bad = base.clone();
    bad["inferences"][0]["premises"] = serde_json::json!(["i2"]);
    cases.push((bad, "EVIDENCE_INFERENCE_PREMISE"));
    let mut bad = base.clone();
    bad["inferences"][0]["premises"] = serde_json::json!(["i2"]);
    bad["inferences"][1]["premises"] = serde_json::json!(["i1"]);
    cases.push((bad, "EVIDENCE_INFERENCE_PREMISE"));
    let mut bad = base.clone();
    bad["inferences"][0]["premises"] = serde_json::json!(["c", "c"]);
    cases.push((bad, "EVIDENCE_INFERENCE_PREMISE"));
    let mut bad = base.clone();
    bad["inferences"][0]["text"] = serde_json::json!(" ");
    cases.push((bad, "EVIDENCE_INFERENCE_TEXT"));
    let mut bad = base.clone();
    bad["inferences"][0]["extra"] = serde_json::json!(0);
    cases.push((bad, "EVIDENCE_INFERENCE_SHAPE"));
    assert_eq!(cases.len(), 25);
    for (document, code) in cases {
        assert_comparison_stop("evidence-synthesize:", &document.to_string(), code);
    }
    assert_comparison_stop(
        "evidence-synthesize:",
        r#"{"sources":[],"sources":[],"inferences":[]}"#,
        "DUPLICATE_JSON_KEY",
    );
    assert_comparison_stop(
        "evidence-synthesize:",
        r#"{"sources":"#,
        "INVALID_JSON_DOCUMENT",
    );
    assert_comparison_stop(
        "evidence-synthesize and send:",
        &base.to_string(),
        "CAPABILITY_UNAVAILABLE",
    );
    let scratch = Scratch::new();
    let request = format!("evidence-synthesize:\n{base}");
    let result = stopped(
        &execute(
            &scratch.0,
            "owner",
            &["--max-evidence-bytes", "32", &request],
        ),
        2,
        "SUSPEND",
    );
    assert!(
        result["reason"]
            .as_str()
            .unwrap()
            .contains("RESOURCE_LIMIT")
    );
    assert_eq!(result["guarantee"], "UNKNOWN");
    assert_eq!(result["dependencies"], serde_json::json!([]));
    let saved = inspect_normal_case(&scratch.0, "owner");
    assert_eq!(saved["bundle"]["request"]["text"], request);
    assert_eq!(saved["result"], result);
}

const BASIC_SELECTION_CSV: &str = "id,\"値 名前\"\r\na,\"猫,北\"\r\nb,\"犬\r\n南\"\r\nc,\"\"\r\nd,\r\ne,9007199254740993\r\nf,\"say \"\"hi\"\"\"\r\n";
const BASIC_NUMERIC_ARRAY: &str = "[1.20,-0,3e-1,2]";
const BASIC_POINTER_JSON: &str = " \r\n {\"前\":\"猫\",\"a/b\":{\"~key\":[1.2300e+40,1e400,\"\\u732b\"]},\"samples\":[1.20,-0,3e-1,2]} \t\n";

fn basic_raw_result(directory: &Path, result: &Value, request: &str, document: Option<&str>) {
    assert_eq!(result["lifecycle"], "COMMIT");
    assert_eq!(result["guarantee"], "COMPUTED");
    assert_eq!(result["render"]["status"], "RENDERED");
    assert_eq!(result["content_plan"]["text"], result["text"]);
    assert_eq!(
        result["render"]["actual_sha256"],
        minidora_domain::digest(result["text"].as_str().unwrap().as_bytes())
    );
    let saved = inspect_normal_case(directory, "owner");
    assert_eq!(saved["bundle"]["request"]["text"], request);
    assert_eq!(saved["result"], *result);
    if let Some(source) = document {
        assert_eq!(saved["bundle"]["meaning"]["intent"]["document"], source);
    }
}

fn assert_basic_csv_maps(
    artifact: &Value,
    source: &str,
    raw: &[&str],
    decoded: &[&str],
    selection: bool,
) {
    assert_eq!(raw.len(), decoded.len());
    let spans = artifact["spans"].as_array().unwrap();
    assert_eq!(spans.len(), raw.len());
    let mut cursor = 1;
    for (row, span) in spans.iter().enumerate() {
        let prefix = if row == 0 {
            "id".to_string()
        } else {
            char::from(b'a' + (row - 1) as u8).to_string()
        };
        let fragment = if row == 0 {
            format!("{prefix},{}", raw[row])
        } else {
            format!("\r\n{prefix},{}", raw[row])
        };
        let start = source.find(&fragment).unwrap() + usize::from(row > 0) * 2 + prefix.len() + 1;
        let output_start = if selection && row > 0 {
            Some(cursor)
        } else {
            None
        };
        let token = serde_json::to_string(decoded[row]).unwrap();
        assert_eq!(
            *span,
            serde_json::json!({
                "kind":if row==0 {"CSV_HEADER"} else if selection {"CSV_SELECTED_CELL"} else {"CSV_NUMERIC_OBSERVATION"},
                "row":row,"column":1,"source_start":start,"source_end":start+raw[row].len(),
                "source_text":raw[row],"decoded_value":decoded[row],"output_start":output_start,
                "output_end":output_start.map(|p|p+token.len())
            })
        );
        if output_start.is_some() {
            let text = artifact["text"].as_str().unwrap();
            assert_eq!(&text[cursor..cursor + token.len()], token);
            cursor += token.len() + 1;
        }
    }
    if selection && raw.len() > 1 {
        assert_eq!(cursor, artifact["text"].as_str().unwrap().len());
    }
}

#[test]
fn basic_document_csv_selection_preserves_all_original_and_output_maps() {
    for empty in [false, true] {
        let source = if empty {
            "id,\"値 名前\"\r\n"
        } else {
            BASIC_SELECTION_CSV
        };
        for header in ["CSVの列「値 名前」を選択して:", "csv-select \"値 名前\""] {
            let scratch = Scratch::new();
            let request = format!("{header}\n{source}");
            let result = record(&execute(&scratch.0, "owner", &[&request]));
            let artifact = normal_document_artifact(
                &scratch.0,
                "owner",
                &result,
                &request,
                source,
                "CSV_SELECT",
            );
            let raw = if empty {
                vec!["\"値 名前\""]
            } else {
                vec![
                    "\"値 名前\"",
                    "\"猫,北\"",
                    "\"犬\r\n南\"",
                    "\"\"",
                    "",
                    "9007199254740993",
                    "\"say \"\"hi\"\"\"",
                ]
            };
            let decoded = if empty {
                vec!["値 名前"]
            } else {
                vec![
                    "値 名前",
                    "猫,北",
                    "犬\r\n南",
                    "",
                    "",
                    "9007199254740993",
                    "say \"hi\"",
                ]
            };
            assert_eq!(
                serde_json::from_str::<Value>(result["text"].as_str().unwrap()).unwrap(),
                serde_json::json!(&decoded[1..])
            );
            assert_eq!(
                artifact["parameters"],
                serde_json::json!({"column_index":"1","has_header":"true","value_type":"STRING","column_name":"値 名前"})
            );
            assert_basic_csv_maps(&artifact, source, &raw, &decoded, true);
        }
    }
}

fn assert_basic_statistics(
    stats: &Value,
    inputs: &[&str],
    normalized: &[&str],
    sorted: &[&str],
    expected: [&str; 7],
    units: Option<(&str, &[&str])>,
) {
    assert_eq!(stats["count"], inputs.len());
    assert_eq!(stats["input_values"], serde_json::json!(inputs));
    assert_eq!(stats["normalized_values"], serde_json::json!(normalized));
    assert_eq!(stats["sorted_values"], serde_json::json!(sorted));
    assert_eq!(
        stats["unit"],
        serde_json::json!(units.map(|(unit, _)| unit))
    );
    assert_eq!(
        stats["input_units"],
        serde_json::json!(units.map(|(_, items)| items))
    );
    assert_eq!(
        stats["variance_unit"],
        serde_json::json!(units.map(|(unit, _)| format!("({unit})^2")))
    );
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
        assert_eq!(
            stats[field],
            if value.is_empty() {
                Value::Null
            } else {
                serde_json::json!(value)
            },
            "{field}"
        );
    }
    assert!(stats["scope"].as_str().unwrap().contains("no inference"));
}

#[test]
fn basic_document_csv_statistics_keep_exact_expressions_units_and_source_cells() {
    for (inputs, normalized, sorted, expected, units) in [
        (
            ["0.1", "0.2"],
            ["1/10", "1/5"],
            ["1/10", "1/5"],
            ["3/10", "3/20", "3/20", "1/10", "1/5", "1/400", "1/200"],
            None,
        ),
        (
            ["1e3", "2e3"],
            ["1000", "2000"],
            ["1000", "2000"],
            ["3000", "1500", "1500", "1000", "2000", "250000", "500000"],
            None,
        ),
        (
            ["1 km", "500 m"],
            ["1000", "500"],
            ["500", "1000"],
            ["1500", "750", "750", "500", "1000", "62500", "125000"],
            Some(("m", ["km", "m"])),
        ),
        (
            ["1e3/2", "1e3-2e3"],
            ["500", "-1000"],
            ["-1000", "500"],
            ["-500", "-250", "-250", "-1000", "500", "562500", "1125000"],
            None,
        ),
    ] {
        let source = format!("id,v\r\na,{}\r\nb,{}\r\n", inputs[0], inputs[1]);
        for header in ["CSVの列「v」の統計量を計算して:", "csv-statistics v"] {
            let scratch = Scratch::new();
            let request = format!("{header}\n{source}");
            let result = record(&execute(&scratch.0, "owner", &[&request]));
            let artifact = normal_document_artifact(
                &scratch.0,
                "owner",
                &result,
                &request,
                &source,
                "CSV_STATISTICS",
            );
            let report: Value = serde_json::from_str(result["text"].as_str().unwrap()).unwrap();
            assert_eq!(
                report["source_sha256"],
                minidora_domain::digest(source.as_bytes())
            );
            assert_eq!(report["column_name"], "v");
            assert_eq!(report["column_index"], 1);
            assert_eq!(report["source_spans"], artifact["spans"]);
            assert_basic_csv_maps(
                &artifact,
                &source,
                &["v", inputs[0], inputs[1]],
                &["v", inputs[0], inputs[1]],
                false,
            );
            let input_values = if units.is_some() {
                ["1", "500"]
            } else {
                inputs
            };
            assert_basic_statistics(
                &report["statistics"],
                &input_values,
                &normalized,
                &sorted,
                expected,
                units.as_ref().map(|(u, items)| (*u, items.as_slice())),
            );
            assert_eq!(artifact["parameters"]["observation_count"], "2");
            assert_eq!(
                artifact["parameters"]["interpretation"],
                if units.is_some() {
                    "exact_numeric_literal_with_unit"
                } else {
                    "exact_unitless_expression"
                }
            );
        }
    }
}

#[test]
fn basic_document_json_pointer_preserves_body_and_large_raw_lexemes() {
    let root = BASIC_POINTER_JSON.trim();
    for (pointer, expected) in [
        ("/a~1b/~0key/0", "1.2300e+40"),
        ("/a~1b/~0key/1", "1e400"),
        ("/a~1b/~0key/2", r#""\u732b""#),
        ("/samples", BASIC_NUMERIC_ARRAY),
        ("", root),
    ] {
        for header in [
            format!("json-pointer {pointer}\n"),
            format!("JsOn-PoInTeR {pointer}\r\n"),
        ] {
            let scratch = Scratch::new();
            let request = format!("{header}{BASIC_POINTER_JSON}");
            let result = record(&execute(&scratch.0, "owner", &[&request]));
            assert_eq!(result["text"], expected);
            assert_eq!(result["dependencies"], serde_json::json!([]));
            basic_raw_result(&scratch.0, &result, &request, Some(BASIC_POINTER_JSON));
        }
    }
}

#[test]
fn json_pointer_explanation_binds_original_raw_value_to_sealed_input_and_render() {
    for (header, pointer, raw, kind) in [
        ("json-pointer", "/a~1b/~0key/1", "1e400", "number"),
        ("JsOn-PoInTeR", "/a~1b/~0key/2", r#""\u732b""#, "string"),
        ("json-pointer", "", BASIC_POINTER_JSON.trim(), "object"),
    ] {
        let scratch = Scratch::new();
        let request = format!("{header} {pointer}\r\n{BASIC_POINTER_JSON}");
        let first = record(&execute(
            &scratch.0,
            "owner",
            &["--input", "json-pointer", &request],
        ));
        basic_raw_result(&scratch.0, &first, &request, Some(BASIC_POINTER_JSON));
        assert_eq!(first["text"], raw);
        assert_eq!(first["dependencies"], serde_json::json!([]));
        let original = inspect_normal_case(&scratch.0, "owner");
        let input_ref =
            minidora_domain::digest(&minidora_domain::canonical(&original["bundle"]).unwrap());
        let render_bytes = minidora_domain::canonical(&first["render"]).unwrap();
        let render_ref = minidora_domain::digest(&render_bytes);
        let explained = record(&execute(
            &scratch.0,
            "owner",
            &["--input", "explain", "詳しく説明して"],
        ));
        basic_raw_result(&scratch.0, &explained, "詳しく説明して", None);
        assert_ne!(explained["text"], first["text"]);
        assert_eq!(explained["receipts"], serde_json::json!([]));
        assert_eq!(
            explained["dependencies"],
            serde_json::json!([first["id"], input_ref, render_ref])
        );
        let text = explained["text"].as_str().unwrap();
        let start = BASIC_POINTER_JSON.find(raw).unwrap();
        let raw_ref = minidora_domain::digest(raw.as_bytes());
        for expected in [
            "前回のJSON Pointer抽出".to_string(),
            format!("JSON Pointer: {pointer:?}"),
            format!("型: {kind}"),
            format!("抽出値（原文）: {raw:?}"),
            format!("元 byte[{start}..{}) sha256={raw_ref}", start + raw.len()),
            format!(
                "元資料 SHA-256: {}",
                minidora_domain::digest(BASIC_POINTER_JSON.as_bytes())
            ),
            format!("封印済みInputBundle SHA-256: {input_ref}"),
            format!("抽出結果 SHA-256: {raw_ref}"),
            format!("描画記録 SHA-256: {render_ref}"),
        ] {
            assert!(text.contains(&expected), "missing {expected:?} in {text}");
        }
        let store = minidora_engine::store::Store::open(&scratch.0).unwrap();
        assert_eq!(
            store
                .get_bytes("owner", "normal-route", &render_ref)
                .unwrap(),
            render_bytes
        );
        assert!(store.actions("owner", "normal-route").unwrap().is_empty());
        let graph = sequence_artifact(&store, &explained["execution_graph_ref"]);
        let nodes = graph[0].as_array().unwrap();
        assert!(
            nodes
                .iter()
                .any(|node| node["id"] == "previous-json-pointer-input"
                    && node["kind"] == "EVIDENCE"
                    && node["requires"] == serde_json::json!([]))
        );
        assert!(
            nodes
                .iter()
                .any(|node| node["id"] == "previous-json-pointer-render"
                    && node["kind"] == "COMPUTED_VALUE"
                    && node["requires"] == serde_json::json!(["previous-json-pointer-input"]))
        );
        assert!(nodes.iter().any(|node| {
            node["id"] == "compute"
                && node["requires"]
                    .as_array()
                    .unwrap()
                    .contains(&serde_json::json!("previous-json-pointer-render"))
        }));
        assert_eq!(graph[1]["previous-json-pointer-input"], input_ref);
        assert_eq!(graph[1]["previous-json-pointer-render"], render_ref);
        assert_eq!(
            graph[5]["previous-json-pointer-render"],
            serde_json::json!({"previous-json-pointer-input":input_ref})
        );
        assert_eq!(
            graph[5]["compute"]["previous-json-pointer-render"],
            render_ref
        );
        let replayed = record(&execute(
            &scratch.0,
            "owner",
            &["--input", "json-pointer", &request],
        ));
        assert_eq!(replayed, first);
        let saved = inspect_normal_case(&scratch.0, "owner");
        assert_eq!(
            saved["bundle"]["meaning"]["context_result_ref"],
            first["id"]
        );
        assert_eq!(saved["prior_results"][first["id"].as_str().unwrap()], first);
        assert_eq!(saved["result"], explained);
    }
}

#[test]
fn csv_statistics_followup_explains_revalidated_source_cells() {
    let scratch = Scratch::new();
    let source = "label,value\r\na,0.1\r\nb,0.2\r\nc,0.3\r\n";
    let original = record(&execute(
        &scratch.0,
        "owner",
        &[
            "--input",
            "csv-statistics",
            &format!("CSVの列「value」の統計量を計算して:\n{source}"),
        ],
    ));
    assert_eq!(original["lifecycle"], "COMMIT");
    assert_eq!(original["guarantee"], "COMPUTED");
    assert_eq!(original["dependencies"].as_array().unwrap().len(), 1);
    let statistics: Value = serde_json::from_str(original["text"].as_str().unwrap()).unwrap();
    assert_eq!(statistics["statistics"]["mean"], "1/5");

    let explained = record(&execute(
        &scratch.0,
        "owner",
        &["--input", "csv-statistics-explanation", "詳しく説明して"],
    ));
    assert_eq!(explained["lifecycle"], "COMMIT");
    assert_eq!(explained["guarantee"], "COMPUTED");
    assert_eq!(
        explained["dependencies"],
        serde_json::json!([original["id"], original["dependencies"][0]])
    );
    let text = explained["text"].as_str().unwrap();
    assert!(text.contains("前回のCSV統計: value列"));
    assert!(text.contains("正確な集計値:"));
    assert!(text.contains("mean\":\"1/5\""));
    assert!(text.contains("計算に使った元セル: 3件"));
    assert!(text.contains("元セル 行2・列2"));
}

#[test]
fn statistics_followup_recomputes_the_previous_exact_b03_report() {
    let scratch = Scratch::new();
    let original = record(&execute(
        &scratch.0,
        "owner",
        &["--input", "b03-statistics", "統計 [0.1,0.2,0.3]"],
    ));
    assert_eq!(original["lifecycle"], "COMMIT");
    assert_eq!(original["guarantee"], "COMPUTED");
    assert_eq!(original["dependencies"], serde_json::json!([]));

    let explained = record(&execute(
        &scratch.0,
        "owner",
        &["--input", "b03-statistics-explanation", "詳しく説明して"],
    ));
    assert_eq!(explained["lifecycle"], "COMMIT");
    assert_eq!(explained["guarantee"], "COMPUTED");
    assert_eq!(
        explained["dependencies"],
        serde_json::json!([original["id"]])
    );
    let text = explained["text"].as_str().unwrap();
    assert!(text.contains("前回の記述統計を元の値から再計算"));
    assert!(text.contains("入力値 (n=3): [0.1, 0.2, 0.3]"));
    assert!(text.contains("合計: 3/5; 平均: 1/5; 中央値: 1/5"));
    assert!(text.contains("母分散: 1/150"));
    assert!(text.contains("no inference about population representativeness"));
    assert_ne!(text, original["text"]);
    assert_eq!(
        inspect_normal_case(&scratch.0, "owner")["bundle"]["meaning"]["context_result_ref"],
        original["id"]
    );
}

#[test]
fn probability_and_polynomial_followups_recompute_the_previous_b03_report() {
    let scratch = Scratch::new();
    let cases: [(&str, &str, &str, &[&str]); 4] = [
        (
            "weights",
            "確率分布 [1,2,3] を正規化して",
            "weights-explain",
            &[
                "重み合計: 6",
                "正規化後の確率: [1/6, 1/3, 1/2]",
                "確率合計: 1",
            ],
        ),
        (
            "expectation",
            "期待値 [-1,3] 重み [1,3] を計算して",
            "expectation-explain",
            &[
                "期待値の各寄与: [-1/4, 9/4]",
                "期待値: 2",
                "分散の各寄与: [9/4, 3/4]",
                "分散: 3",
            ],
        ),
        (
            "event",
            "事象確率 [0,1,1,2] 重み [1,2,3,4] 事象値 [1,2,2] を計算して",
            "event-explain",
            &[
                "対象事象の値: [1, 2, 2]",
                "結果ごとの事象所属: [false, true, true, true]",
                "事象の確率: 9/10",
            ],
        ),
        (
            "polynomial",
            "評価 2*x^2-3*x+1 代入 x=2",
            "polynomial-explain",
            &[
                "式: 2*x^2-3*x+1",
                "項1: powers={\"x\": 2}; 係数 2 × 単項式値 4 = 寄与 8",
                "合計値: 3",
            ],
        ),
    ];
    for (case, request, followup, expected_lines) in cases {
        let original = record(&execute_in_case(
            &scratch.0,
            "owner",
            case,
            &["--input", "b03-original", request],
        ));
        assert_eq!(original["lifecycle"], "COMMIT", "{case}");
        assert_eq!(original["guarantee"], "COMPUTED", "{case}");
        assert_eq!(original["dependencies"], serde_json::json!([]), "{case}");

        let explained = record(&execute_in_case(
            &scratch.0,
            "owner",
            case,
            &["--input", followup, "詳しく説明して"],
        ));
        assert_eq!(explained["lifecycle"], "COMMIT", "{case}");
        assert_eq!(explained["guarantee"], "COMPUTED", "{case}");
        assert_eq!(
            explained["dependencies"],
            serde_json::json!([original["id"]]),
            "{case}"
        );
        let text = explained["text"].as_str().unwrap();
        assert!(
            text.contains("再計算し、保存レポートと照合しました"),
            "{case}: {text}"
        );
        for line in expected_lines {
            assert!(text.contains(line), "{case}: missing {line:?} in {text}");
        }
        assert_ne!(text, original["text"].as_str().unwrap(), "{case}");
    }
}

#[test]
fn linear_and_quadratic_followups_recompute_the_previous_b03_report() {
    let scratch = Scratch::new();
    let cases: [(&str, &str, &[&str]); 3] = [
        (
            "linear-system",
            "連立方程式 [[2,1],[1,-1]] = [5,1]",
            &["状態: Unique; rank: 2", "一意解: [2, 1]"],
        ),
        (
            "linear-equation",
            "一次方程式 2*x+1=0 を解いて",
            &["分類: UniqueSolution; 解: -1/2"],
        ),
        (
            "quadratic-equation",
            "二次方程式 x^2-3*x+2=0 を解いて",
            &[
                "判別式: 1",
                "TwoRationalRealRoots",
                "1 (RATIONAL",
                "2 (RATIONAL",
            ],
        ),
    ];
    for (case, request, expected_lines) in cases {
        let original = record(&execute_in_case(
            &scratch.0,
            "owner",
            case,
            &["--input", "b03-equation", request],
        ));
        assert_eq!(original["lifecycle"], "COMMIT", "{case}");
        assert_eq!(original["guarantee"], "COMPUTED", "{case}");
        assert_eq!(original["dependencies"], serde_json::json!([]), "{case}");

        let explained = record(&execute_in_case(
            &scratch.0,
            "owner",
            case,
            &["--input", "equation-explanation", "詳しく説明して"],
        ));
        assert_eq!(explained["lifecycle"], "COMMIT", "{case}");
        assert_eq!(explained["guarantee"], "COMPUTED", "{case}");
        assert_eq!(
            explained["dependencies"],
            serde_json::json!([original["id"]]),
            "{case}"
        );
        let text = explained["text"].as_str().unwrap();
        assert!(
            text.contains("再計算し、保存レポートと照合しました"),
            "{case}: {text}"
        );
        for line in expected_lines {
            assert!(text.contains(line), "{case}: missing {line:?} in {text}");
        }
        assert_ne!(text, original["text"].as_str().unwrap(), "{case}");
    }
}

#[test]
fn symbolic_followups_recompute_the_previous_b03_report() {
    let scratch = Scratch::new();
    let cases = [
        ("simplify", "簡約 (x+1)*(x-1)"),
        ("derive", "微分 x^3+2*x 変数 x"),
        ("integrate", "積分 x^2 変数 x"),
    ];
    for (case, request) in cases {
        let original = record(&execute_in_case(
            &scratch.0,
            "owner",
            case,
            &["--input", "b03-symbolic", request],
        ));
        assert_eq!(original["lifecycle"], "COMMIT", "{case}");
        assert_eq!(original["guarantee"], "COMPUTED", "{case}");
        assert_eq!(original["dependencies"], serde_json::json!([]), "{case}");
        let report: Value = serde_json::from_str(original["text"].as_str().unwrap()).unwrap();
        assert_eq!(report["operation"], case);

        let explained = record(&execute_in_case(
            &scratch.0,
            "owner",
            case,
            &["--input", "symbolic-explanation", "詳しく説明して"],
        ));
        assert_eq!(explained["lifecycle"], "COMMIT", "{case}");
        assert_eq!(explained["guarantee"], "COMPUTED", "{case}");
        assert_eq!(
            explained["dependencies"],
            serde_json::json!([original["id"]]),
            "{case}"
        );
        let text = explained["text"].as_str().unwrap();
        assert!(text.contains("前回の記号式操作"), "{case}: {text}");
        assert!(
            text.contains(&format!(
                "変換後の標準式: {}",
                report["canonical_expression"].as_str().unwrap()
            )),
            "{case}: {text}"
        );
        if case == "integrate" {
            assert!(text.contains("積分の任意項"), "{text}");
        }
        assert_ne!(text, original["text"].as_str().unwrap(), "{case}");
    }
}

fn assert_basic_json_statistics(report: &Value, source: &str, pointer: &str, singleton: bool) {
    assert_eq!(report["pointer"], pointer);
    let array = if singleton {
        "[5e-1]"
    } else {
        BASIC_NUMERIC_ARRAY
    };
    let tokens = if singleton {
        vec!["5e-1"]
    } else {
        vec!["1.20", "-0", "3e-1", "2"]
    };
    let start = source.find(array).unwrap();
    let mut expected = vec![
        serde_json::json!({"kind":"JSON_ARRAY","source_start":start,"source_end":start+array.len(),"source_text":array,"array_index":null}),
    ];
    let mut cursor = start + 1;
    for (index, token) in tokens.iter().enumerate() {
        let at = cursor + source[cursor..].find(token).unwrap();
        expected.push(serde_json::json!({"kind":"JSON_NUMBER","source_start":at,"source_end":at+token.len(),"source_text":token,"array_index":index}));
        cursor = at + token.len();
    }
    assert_eq!(report["spans"], serde_json::json!(expected));
    if singleton {
        assert_basic_statistics(
            &report["statistics"],
            &["5e-1"],
            &["1/2"],
            &["1/2"],
            ["1/2", "1/2", "1/2", "1/2", "1/2", "0", ""],
            None,
        );
    } else {
        assert_basic_statistics(
            &report["statistics"],
            &tokens,
            &["6/5", "0", "3/10", "2"],
            &["0", "3/10", "6/5", "2"],
            ["7/2", "7/8", "3/4", "0", "2", "987/1600", "329/400"],
            None,
        );
    }
}

#[test]
fn basic_document_json_statistics_preserve_selected_array_and_number_lexemes() {
    let nested = format!(" \r\n{{\"前\":\"猫\",\"a/b\":{{\"~key\":{BASIC_NUMERIC_ARRAY}}}}} \t");
    for (source, pointer) in [
        (nested.as_str(), "/a~1b/~0key"),
        (BASIC_NUMERIC_ARRAY, ""),
        ("[5e-1]", ""),
    ] {
        let singleton = source == "[5e-1]";
        for header in [
            format!("JSONの配列「{pointer}」の統計量を計算して:"),
            format!(
                "json-statistics {}",
                serde_json::to_string(pointer).unwrap()
            ),
        ] {
            let scratch = Scratch::new();
            let request = format!("{header}\n{source}");
            let result = record(&execute(&scratch.0, "owner", &[&request]));
            let report = normal_row_report(&scratch.0, "owner", &result, &request, source);
            assert_basic_json_statistics(&report, source, pointer, singleton);
        }
    }
}

#[test]
fn json_statistics_explanation_preserves_exact_math_and_selected_number_sources() {
    let nested = format!(" \r\n{{\"前\":\"猫\",\"a/b\":{{\"~key\":{BASIC_NUMERIC_ARRAY}}}}} \t");
    for (source, pointer, header) in [
        (
            nested.as_str(),
            "/a~1b/~0key",
            "JSONの配列「/a~1b/~0key」の統計量を計算して:",
        ),
        (BASIC_NUMERIC_ARRAY, "", "json-statistics \"\""),
    ] {
        let scratch = Scratch::new();
        let request = format!("{header}\n{source}");
        let first = record(&execute(
            &scratch.0,
            "owner",
            &["--input", "json-statistics", &request],
        ));
        let report = normal_row_report(&scratch.0, "owner", &first, &request, source);
        assert_basic_json_statistics(&report, source, pointer, false);
        let explained =
            assert_table_explanation(&scratch.0, &first, &report, "previous-json-statistics");
        let text = explained["text"].as_str().unwrap();
        for expected in [
            "前回のJSON統計".to_string(),
            format!("JSON Pointer: {pointer:?}"),
            "件数: 4、合計: 7/2、平均: 7/8".to_string(),
            "母分散: 987/1600、標本分散: 329/400".to_string(),
        ] {
            assert!(text.contains(&expected), "missing {expected:?} in {text}");
        }
        let array_start = source.find(BASIC_NUMERIC_ARRAY).unwrap();
        let array_line = text.lines().find(|line| line.contains("元の配列")).unwrap();
        assert!(
            array_line.contains(&format!(
                "元 byte[{array_start}..{}) sha256={}",
                array_start + BASIC_NUMERIC_ARRAY.len(),
                minidora_domain::digest(BASIC_NUMERIC_ARRAY.as_bytes())
            )),
            "{array_line}"
        );
        let mut cursor = array_start + 1;
        for (index, (raw, normalized)) in
            [("1.20", "6/5"), ("-0", "0"), ("3e-1", "3/10"), ("2", "2")]
                .into_iter()
                .enumerate()
        {
            let start = cursor + source[cursor..].find(raw).unwrap();
            let end = start + raw.len();
            let expected = format!(
                "要素{index} JSON Pointer {pointer}/{index}。数値表記: {raw:?}、厳密値: {normalized}。元 byte[{start}..{end}) sha256={}",
                minidora_domain::digest(raw.as_bytes())
            );
            assert!(
                text.lines().any(|line| line.contains(&expected)),
                "missing {expected:?} in {text}"
            );
            cursor = end;
        }
    }
}

#[test]
fn basic_document_actual_extractions_feed_statistics_and_explicit_report_projection() {
    for japanese in [false, true] {
        let scratch = Scratch::new();
        let request = format!("json-pointer /samples\n{BASIC_POINTER_JSON}");
        let extracted = record(&execute(&scratch.0, "owner", &[&request]));
        assert_eq!(extracted["text"], BASIC_NUMERIC_ARRAY);
        basic_raw_result(&scratch.0, &extracted, &request, Some(BASIC_POINTER_JSON));
        let actual = extracted["text"].as_str().unwrap();
        let statistics_request = format!(
            "{}\n{actual}",
            if japanese {
                "JSONの配列「」の統計量を計算して:"
            } else {
                "json-statistics"
            }
        );
        let result = record(&execute(&scratch.0, "owner", &[&statistics_request]));
        let report = normal_row_report(&scratch.0, "owner", &result, &statistics_request, actual);
        assert_basic_json_statistics(&report, actual, "", false);
        let report_text = result["text"].as_str().unwrap();
        let projection_request = format!("json-object-project [\"statistics\"]:\n{report_text}");
        let projected = record(&execute(&scratch.0, "owner", &[&projection_request]));
        let artifact = normal_object_projection(
            &scratch.0,
            "owner",
            &projected,
            &projection_request,
            report_text,
        );
        assert_object_projection_maps(
            &artifact,
            report_text,
            &[("statistics", r#""statistics""#, "/statistics")],
        );
        assert_eq!(
            serde_json::from_str::<Value>(projected["text"].as_str().unwrap()).unwrap(),
            serde_json::json!({"statistics":report["statistics"]})
        );
    }
    let scratch = Scratch::new();
    let request = format!("csv-select \"値 名前\"\n{BASIC_SELECTION_CSV}");
    let selected = record(&execute(&scratch.0, "owner", &[&request]));
    normal_document_artifact(
        &scratch.0,
        "owner",
        &selected,
        &request,
        BASIC_SELECTION_CSV,
        "CSV_SELECT",
    );
    let actual = selected["text"].as_str().unwrap();
    let next = format!("json-pointer /0\n{actual}");
    let result = record(&execute(&scratch.0, "owner", &[&next]));
    basic_raw_result(&scratch.0, &result, &next, Some(actual));
    assert_eq!(result["text"], serde_json::to_string("猫,北").unwrap());
}

#[test]
fn basic_document_grouped_and_direct_statistics_use_the_same_numeric_classification() {
    for (inputs, normalized, sorted, expected) in [
        (
            ["1e3", "2e3"],
            ["1000", "2000"],
            ["1000", "2000"],
            ["3000", "1500", "1500", "1000", "2000", "250000", "500000"],
        ),
        (
            ["1e-3", "2e-3"],
            ["1/1000", "1/500"],
            ["1/1000", "1/500"],
            [
                "3/1000",
                "3/2000",
                "3/2000",
                "1/1000",
                "1/500",
                "1/4000000",
                "1/2000000",
            ],
        ),
        (
            ["1e3/2", "1e3-2e3"],
            ["500", "-1000"],
            ["-1000", "500"],
            ["-500", "-250", "-250", "-1000", "500", "562500", "1125000"],
        ),
    ] {
        let csv = format!("g,v\r\na,{}\r\na,{}", inputs[0], inputs[1]);
        let json =
            serde_json::json!({"columns":["g","v"],"rows":[["a",inputs[0]],["a",inputs[1]]]})
                .to_string();
        for (kind, request, source) in [
            (
                0,
                if inputs[0].contains('/') {
                    format!("statistics {}", serde_json::to_string(&inputs).unwrap())
                } else {
                    format!("statistics [{},{}]", inputs[0], inputs[1])
                },
                String::new(),
            ),
            (1, format!("csv-group-statistics \"g\" \"v\":\n{csv}"), csv),
            (
                2,
                format!("json-table-group-statistics \"g\" \"v\":\n{json}"),
                json,
            ),
        ] {
            let scratch = Scratch::new();
            let result = record(&execute(&scratch.0, "owner", &[&request]));
            let report = if kind == 0 {
                basic_raw_result(&scratch.0, &result, &request, None);
                serde_json::from_str::<Value>(result["text"].as_str().unwrap()).unwrap()
            } else if kind == 1 {
                normal_document_artifact(
                    &scratch.0,
                    "owner",
                    &result,
                    &request,
                    &source,
                    "CSV_GROUP_STATISTICS",
                );
                serde_json::from_str::<Value>(result["text"].as_str().unwrap()).unwrap()
            } else {
                normal_row_report(&scratch.0, "owner", &result, &request, &source)
            };
            let stats = if kind == 0 {
                &report
            } else {
                assert_eq!(report["groups"].as_array().unwrap().len(), 1);
                &report["groups"][0]["statistics"]
            };
            assert_basic_statistics(stats, &inputs, &normalized, &sorted, expected, None);
        }
    }
    for (request, expected) in [("1e3+2e3", "3000"), ("1/2e3+1/4e3", "3/4000")] {
        let scratch = Scratch::new();
        let result = record(&execute(&scratch.0, "owner", &[request]));
        basic_raw_result(&scratch.0, &result, request, None);
        assert_eq!(result["calculation"]["exact_value"], expected);
    }
}

#[test]
fn basic_document_invalid_inputs_and_exponent_limit_bypasses_stop_durably() {
    for (header, source, code) in [
        ("csv-select missing", "id,v\r\na,1", "CSV_COLUMN"),
        ("csv-select v", "v,v\r\n1,2", "DUPLICATE_COLUMNS"),
        ("csv-select v", "id,v\na,1", "CSV_LINE_ENDING"),
        ("csv-statistics missing", "id,v\r\na,1", "CSV_COLUMN"),
        ("csv-statistics v", "id,v", "STATISTICS_INPUT_LIMIT"),
        ("csv-statistics v", "id,v\r\na,", "MATH_INPUT_LIMIT"),
        (
            "csv-statistics v",
            "id,v\r\na,1 m\r\nb,2",
            "CSV_STATISTICS_MIXED_VALUES",
        ),
        (
            "csv-statistics v",
            "id,v\r\na,1 m\r\nb,2 s",
            "UNIT_MISMATCH",
        ),
        ("json-pointer /x~2", "{\"x\":1}", "INVALID_JSON_POINTER"),
        ("json-pointer /a/01", "{\"a\":[1]}", "INVALID_JSON_POINTER"),
        (
            "json-pointer /missing",
            "{\"a\":1}",
            "JSON_POINTER_NOT_FOUND",
        ),
        (
            "json-pointer /a",
            "{\"a\":1,\"x\":0,\"\\u0078\":2}",
            "DUPLICATE_JSON_KEY",
        ),
        ("json-pointer /a", "{\"a\":1}x", "INVALID_JSON_DOCUMENT"),
        (
            "json-pointer /a",
            "\u{a0}{\"a\":1}",
            "INVALID_JSON_DOCUMENT",
        ),
        (
            "json-statistics /a",
            "{\"a\":1}",
            "JSON_STATISTICS_NOT_ARRAY",
        ),
        (
            "json-statistics /a",
            "{\"a\":[]}",
            "JSON_STATISTICS_INPUT_LIMIT",
        ),
        (
            "json-statistics /a",
            "{\"a\":[1,\"2\"]}",
            "JSON_STATISTICS_NON_NUMERIC",
        ),
        (
            "json-statistics /a",
            "{\"a\":[[1]]}",
            "JSON_STATISTICS_NON_NUMERIC",
        ),
        (
            "json-statistics /missing",
            "{\"a\":[1]}",
            "JSON_POINTER_NOT_FOUND",
        ),
        (
            "csv-select v and send",
            "id,v\r\na,1",
            "CAPABILITY_UNAVAILABLE",
        ),
        (
            "csv-statistics v and send",
            "id,v\r\na,1",
            "CAPABILITY_UNAVAILABLE",
        ),
        (
            "json-statistics /a and send",
            "{\"a\":[1]}",
            "CAPABILITY_UNAVAILABLE",
        ),
        (
            "json-pointer /a and send",
            "{\"a\":1}",
            "CAPABILITY_UNAVAILABLE",
        ),
    ] {
        assert_comparison_stop(header, source, code);
    }
    for value in ["1e1025", "1e1025/m"] {
        let csv = format!("g,v\r\na,{value}\r\na,{value}");
        let json =
            serde_json::json!({"columns":["g","v"],"rows":[["a",value],["a",value]]}).to_string();
        for (header, source) in [
            ("csv-statistics v".to_string(), csv.clone()),
            ("csv-group-statistics \"g\" \"v\":".to_string(), csv),
            ("json-table-group-statistics \"g\" \"v\":".to_string(), json),
            (
                if value.contains('/') {
                    format!(
                        "statistics {}",
                        serde_json::to_string(&[value, value]).unwrap()
                    )
                } else {
                    format!("statistics [{value},{value}]")
                },
                String::new(),
            ),
            (format!("{value}+{value}"), String::new()),
        ] {
            assert_comparison_stop(&header, &source, "EXPONENT_LIMIT");
        }
    }
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

#[test]
fn json_comparison_explanation_preserves_nested_side_evidence_and_semantic_equality() {
    let changed_left = r#"{"a/b~c":{"change":["\u732b"],"gone":null},"same":1.00}"#;
    let changed_right = r#"{"a/b~c":{"change":["犬"],"new":null},"same":1}"#;
    let equal_left = r#"{"a/b~c":[1,-0,9007199254740993,1.2300e+40,"猫"],"empty":[]}"#;
    let equal_right = r#"{"empty":[],"\u0061/b~c":[1.0,0,9007199254740993.0,123e38,"\u732b"]}"#;
    let differences = [
        (
            "/a~1b~0c/change/0",
            "CHANGED",
            Some((r#"["\u732b"]"#, r#""\u732b""#)),
            Some((r#"["犬"]"#, r#""犬""#)),
        ),
        (
            "/a~1b~0c/gone",
            "REMOVED",
            Some((r#""gone":null"#, "null")),
            None,
        ),
        (
            "/a~1b~0c/new",
            "ADDED",
            None,
            Some((r#""new":null"#, "null")),
        ),
    ];
    for (left, right, header, equal) in [
        (changed_left, changed_right, "JSONを比較して:", false),
        (equal_left, equal_right, "json-compare:", true),
    ] {
        let scratch = Scratch::new();
        let source = comparison_wrapper(left, right);
        let request = format!("{header}\r\n{source}");
        let first = record(&execute(
            &scratch.0,
            "owner",
            &["--input", "json-compare", &request],
        ));
        let report = normal_row_report(&scratch.0, "owner", &first, &request, &source);
        assert_comparison_side_hashes(&report, left, right);
        assert_ne!(report["left_sha256"], report["right_sha256"]);
        assert_eq!(report["comparison"], "JSON_STRUCTURAL_EXACT_DECIMAL");
        assert_eq!(report["equal"], equal);
        let expected_differences: Vec<_> = differences
            .iter()
            .take(if equal { 0 } else { differences.len() })
            .map(|&(pointer, change, l, r)| {
                serde_json::json!({
                    "pointer":pointer,"change":change,
                    "left":l.map(|(fragment,raw)|comparison_fixture_span(&source,left,fragment,raw)),
                    "right":r.map(|(fragment,raw)|comparison_fixture_span(&source,right,fragment,raw))
                })
            })
            .collect();
        assert_eq!(
            report["differences"],
            serde_json::json!(expected_differences)
        );
        let explained =
            assert_table_explanation(&scratch.0, &first, &report, "previous-json-comparison");
        let text = explained["text"].as_str().unwrap();
        for expected in [
            "前回のJSON構造比較".to_string(),
            format!(
                "比較結果: {}。差分数: {}。",
                if equal { "一致" } else { "不一致" },
                expected_differences.len()
            ),
            format!(
                "左の値 SHA-256: {}",
                minidora_domain::digest(left.as_bytes())
            ),
            format!(
                "右の値 SHA-256: {}",
                minidora_domain::digest(right.as_bytes())
            ),
        ] {
            assert!(text.contains(&expected), "missing {expected:?} in {text}");
        }
        let lines: Vec<_> = text.lines().collect();
        assert_eq!(
            lines
                .iter()
                .filter(|line| line.contains(" JSON Pointer "))
                .count(),
            expected_differences.len()
        );
        let mut previous_heading = None;
        for (index, &(pointer, change, l, r)) in differences
            .iter()
            .take(expected_differences.len())
            .enumerate()
        {
            let heading = format!("差分{} JSON Pointer {pointer:?} {change}", index + 1);
            let heading_index = lines
                .iter()
                .position(|line| line.contains(&heading))
                .unwrap();
            if let Some(previous) = previous_heading {
                assert!(previous < heading_index);
            }
            previous_heading = Some(heading_index);
            for (offset, label, side, selection) in [(1, "左", left, l), (2, "右", right, r)] {
                let expected = if let Some((fragment, raw)) = selection {
                    let span = comparison_fixture_span(&source, side, fragment, raw);
                    format!(
                        "{label} 元 byte[{}..{}) sha256={} 原文={raw:?}",
                        span["source_start"].as_u64().unwrap(),
                        span["source_end"].as_u64().unwrap(),
                        minidora_domain::digest(raw.as_bytes())
                    )
                } else {
                    format!("{label} 対応する値なし")
                };
                assert!(
                    lines[heading_index + offset].contains(&expected),
                    "missing {expected:?} in {}",
                    lines[heading_index + offset]
                );
            }
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
fn json_object_projection_explanation_preserves_field_order_and_raw_source_output_pairs() {
    let selected = [
        ("nt", r#""\u006et""#, "/nt", "9007199254740993"),
        (
            "a/b~c",
            r#""a/b~c""#,
            "/a~1b~0c",
            r#"{ "n":1.2300e+40,"s":"\u732b" }"#,
        ),
    ];
    for (header, order, expected_output) in [
        (
            "JSONオブジェクトからフィールド[\"nt\",\"a/b~c\"]を抽出して:",
            [0, 1],
            r#"{"nt":9007199254740993,"a/b~c":{ "n":1.2300e+40,"s":"\u732b" }}"#,
        ),
        (
            "json-object-project [\"a/b~c\",\"nt\"]:",
            [1, 0],
            r#"{"a/b~c":{ "n":1.2300e+40,"s":"\u732b" },"nt":9007199254740993}"#,
        ),
    ] {
        let scratch = Scratch::new();
        let request = format!("{header}\r\n{OBJECT_PROJECTION_JSON}");
        let first = record(&execute(
            &scratch.0,
            "owner",
            &["--input", "object-project", &request],
        ));
        assert_eq!(first["text"], expected_output);
        let report = normal_object_projection(
            &scratch.0,
            "owner",
            &first,
            &request,
            OBJECT_PROJECTION_JSON,
        );
        let expected_fields: Vec<_> = order
            .iter()
            .map(|&index| {
                let (name, raw_key, pointer, _) = selected[index];
                (name, raw_key, pointer)
            })
            .collect();
        assert_object_projection_maps(&report, OBJECT_PROJECTION_JSON, &expected_fields);
        let explained =
            assert_table_explanation(&scratch.0, &first, &report, "previous-json-object-project");
        let text = explained["text"].as_str().unwrap();
        for expected in [
            "前回のJSONオブジェクト項目選択".to_string(),
            "選択項目数: 2".to_string(),
            format!(
                "選択結果 SHA-256: {}",
                minidora_domain::digest(expected_output.as_bytes())
            ),
        ] {
            assert!(text.contains(&expected), "missing {expected:?} in {text}");
        }
        let lines: Vec<_> = text.lines().collect();
        let mut previous_heading = None;
        for (index, field_index) in order.into_iter().enumerate() {
            let (name, raw_key, pointer, raw_value) = selected[field_index];
            let heading = format!("項目{} {name:?} JSON Pointer {pointer:?}", index + 1);
            let heading_index = lines
                .iter()
                .position(|line| line.contains(&heading))
                .unwrap();
            if let Some(previous) = previous_heading {
                assert!(previous < heading_index);
            }
            previous_heading = Some(heading_index);
            let output_key = serde_json::to_string(name).unwrap();
            let output_field = format!("{output_key}:{raw_value}");
            let output_key_start = expected_output.find(&output_field).unwrap();
            for (offset, label, source_raw, output_raw, output_start) in [
                (1, "キー", raw_key, output_key.as_str(), output_key_start),
                (
                    2,
                    "値",
                    raw_value,
                    raw_value,
                    output_key_start + output_key.len() + 1,
                ),
            ] {
                let source_start = OBJECT_PROJECTION_JSON.find(source_raw).unwrap();
                let line = lines[heading_index + offset];
                for expected in [
                    format!(
                        "{label} 元 byte[{source_start}..{}) sha256={} 原文={source_raw:?}",
                        source_start + source_raw.len(),
                        minidora_domain::digest(source_raw.as_bytes())
                    ),
                    format!(
                        "→ 出力 byte[{output_start}..{}) sha256={} 表記={output_raw:?}",
                        output_start + output_raw.len(),
                        minidora_domain::digest(output_raw.as_bytes())
                    ),
                ] {
                    assert!(line.contains(&expected), "missing {expected:?} in {line}");
                }
            }
        }
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
    let store = minidora_engine::store::Store::open(directory).unwrap();
    let report_bytes = minidora_domain::canonical(&report).unwrap();
    let report_id = minidora_domain::digest(&report_bytes);
    assert!(dependencies.contains(&Value::String(report_id.clone())));
    let stored_report = store.get_bytes(actor, "normal-route", &report_id).unwrap();
    assert_eq!(stored_report, report_bytes);
    assert_eq!(
        serde_json::from_slice::<Value>(&stored_report).unwrap(),
        report
    );
    let source_id = minidora_domain::digest(source.as_bytes());
    if dependencies.contains(&Value::String(source_id.clone())) {
        assert_eq!(
            store.get_bytes(actor, "normal-route", &source_id).unwrap(),
            source.as_bytes()
        );
    }
    assert!(dependencies.iter().all(|dependency| {
        dependency == &Value::String(report_id.clone())
            || dependency == &Value::String(source_id.clone())
    }));
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
fn csv_row_explanation_filter_replays_exact_predicate_and_empty_selection() {
    let source = "id,状態,順位\r\n\"猫,甲\",\"open\"\"x\",2\r\n犬,\"Open\"\"x\",10\r\n鳥,\"open\"\"x \",2\r\n魚,\"open\"\"x\",10\r\n";
    for query in ["open\"x", "missing"] {
        let scratch = Scratch::new();
        let request = format!(
            "csv-filter \"状態\" equals {}:\n{source}",
            serde_json::to_string(query).unwrap()
        );
        let first = record(&execute(
            &scratch.0,
            "owner",
            &["--input", "filter", &request],
        ));
        let artifact =
            normal_document_artifact(&scratch.0, "owner", &first, &request, source, "CSV_FILTER");
        let matched = query == "open\"x";
        let rows = if matched {
            serde_json::json!([["猫,甲", "open\"x", "2"], ["魚", "open\"x", "10"]])
        } else {
            serde_json::json!([])
        };
        let table: Value = serde_json::from_str(first["text"].as_str().unwrap()).unwrap();
        assert_eq!(
            table,
            serde_json::json!({"columns":["id","状態","順位"],"rows":rows})
        );
        assert_eq!(artifact["parameters"]["equals"], query);
        let explanation = assert_csv_row_explanation(&scratch.0, &first, &artifact, "filter");
        let text = explanation["text"].as_str().unwrap();
        assert!(text.contains("前回のCSV表絞り込み"), "{text}");
        assert!(text.contains(&format!("{query:?}")), "{text}");
        assert!(
            text.contains(if matched {
                "一致行: 2、不一致行: 2"
            } else {
                "一致行: 0、不一致行: 4"
            }),
            "{text}"
        );
        let predicate_spans: Vec<_> = artifact["spans"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|span| span["kind"] == "CSV_FILTER_DECISION")
            .collect();
        assert_eq!(predicate_spans.len(), 4);
        for (row, value) in [
            (1, "open\"x"),
            (2, "Open\"x"),
            (3, "open\"x "),
            (4, "open\"x"),
        ] {
            let span = predicate_spans
                .iter()
                .find(|span| span["row"] == row)
                .unwrap();
            assert_eq!(span["column"], 1);
            assert_eq!(span["decoded_value"], value);
            let start = span["source_start"].as_u64().unwrap() as usize;
            let end = span["source_end"].as_u64().unwrap() as usize;
            let raw = &source[start..end];
            assert_eq!(span["source_text"], raw);
            let marker = format!(
                "元行{row}: {}",
                if value == query {
                    "一致"
                } else {
                    "不一致"
                }
            );
            let line = text
                .lines()
                .find(|line| line.starts_with(&marker))
                .unwrap_or_else(|| panic!("missing {marker:?} in {text}"));
            assert!(line.contains(&format!("byte[{start}..{end})")), "{line}");
            assert!(
                line.contains(&format!(
                    "sha256={}",
                    minidora_domain::digest(raw.as_bytes())
                )),
                "{line}"
            );
        }
        if matched {
            assert!(text.contains("出力行1 ← 元行1"), "{text}");
            assert!(text.contains("出力行2 ← 元行4"), "{text}");
        } else {
            assert!(!text.contains("出力行1 ←"), "{text}");
        }
    }
}

#[test]
fn csv_row_explanation_sort_replays_string_order_and_stable_ties() {
    for (direction, label, order) in [
        ("ascending", "昇順", &[4usize, 1, 3, 0, 2, 5][..]),
        ("descending", "降順", &[0usize, 2, 5, 1, 3, 4][..]),
    ] {
        let scratch = Scratch::new();
        let request = format!("csv-sort \"順位\" {direction}:\n{ROW_CSV}");
        let first = record(&execute(
            &scratch.0,
            "owner",
            &["--input", "sort", &request],
        ));
        let artifact =
            normal_document_artifact(&scratch.0, "owner", &first, &request, ROW_CSV, "CSV_SORT");
        assert_csv_row_maps(&artifact, order, None);
        let explanation = assert_csv_row_explanation(&scratch.0, &first, &artifact, "sort");
        let text = explanation["text"].as_str().unwrap();
        for expected in ["前回のCSV表並べ替え", label, "同じ値の行は元の順序"] {
            assert!(text.contains(expected), "missing {expected:?} in {text}");
        }
        for (output_row, source_row) in order.iter().enumerate() {
            let marker = format!("出力行{} ← 元行{}", output_row + 1, source_row + 1);
            let line = text
                .lines()
                .find(|line| line.starts_with(&marker))
                .unwrap_or_else(|| panic!("missing {marker:?} in {text}"));
            let span = artifact["spans"]
                .as_array()
                .unwrap()
                .iter()
                .find(|span| {
                    span["kind"] == "CSV_SORT_CELL"
                        && span["row"] == source_row + 1
                        && span["column"] == 2
                })
                .unwrap();
            let start = span["source_start"].as_u64().unwrap() as usize;
            let end = span["source_end"].as_u64().unwrap() as usize;
            assert!(
                line.contains(&format!("{:?}", ROW_VALUES[source_row + 1][2])),
                "{line}"
            );
            assert!(line.contains(&format!("byte[{start}..{end})")), "{line}");
            assert!(
                line.contains(&format!(
                    "sha256={}",
                    minidora_domain::digest(&ROW_CSV.as_bytes()[start..end])
                )),
                "{line}"
            );
        }
    }
}

#[test]
fn csv_group_explanation_count_preserves_first_seen_members_and_empty_table() {
    for (source, groups, rows) in [
        (
            "id,分類\r\n1,\"猫,北\"\r\n2,犬\r\n3,\"猫,北\"\r\n",
            serde_json::json!([{"value":"猫,北","count":2},{"value":"犬","count":1}]),
            3,
        ),
        (
            "id,分類\r\n2,犬\r\n1,\"猫,北\"\r\n3,\"猫,北\"\r\n",
            serde_json::json!([{"value":"犬","count":1},{"value":"猫,北","count":2}]),
            3,
        ),
        ("id,分類\r\n", serde_json::json!([]), 0),
    ] {
        let scratch = Scratch::new();
        let request = format!("CSVを列「分類」で件数集計して:\n{source}");
        let first = record(&execute(
            &scratch.0,
            "owner",
            &["--input", "count", &request],
        ));
        let artifact = normal_document_artifact(
            &scratch.0,
            "owner",
            &first,
            &request,
            source,
            "CSV_GROUP_COUNT",
        );
        let report: Value = serde_json::from_str(first["text"].as_str().unwrap()).unwrap();
        assert_eq!(report["group_by"], "分類");
        assert_eq!(report["groups"], groups);
        assert_eq!(artifact["parameters"]["row_count"], rows.to_string());
        let explanation = assert_csv_row_explanation(&scratch.0, &first, &artifact, "group-count");
        let text = explanation["text"].as_str().unwrap();
        for expected in [
            "前回のCSVグループ件数".to_string(),
            "初出順".to_string(),
            format!("行数: {rows}"),
            format!("グループ数: {}", groups.as_array().unwrap().len()),
        ] {
            assert!(text.contains(&expected), "missing {expected:?} in {text}");
        }
        for (index, group) in groups.as_array().unwrap().iter().enumerate() {
            let expected = format!(
                "グループ{} {:?}。件数: {}",
                index + 1,
                group["value"].as_str().unwrap(),
                group["count"]
            );
            assert!(text.contains(&expected), "missing {expected:?} in {text}");
        }
        assert_csv_group_explanation_sources(text, &artifact, &["CSV_GROUP_MEMBER"]);
    }
}

#[test]
fn csv_group_explanation_statistics_preserves_exact_units_and_source_cells() {
    let source = "地域,測定\r\n\"北,陸\",1 km\r\n南,1 s\r\n\"北,陸\",500 m\r\n南,2 s\r\n";
    let request = format!("CSVの「地域」ごとに「測定」の統計量を計算して:\n{source}");
    let scratch = Scratch::new();
    let first = record(&execute(
        &scratch.0,
        "owner",
        &["--input", "group-statistics", &request],
    ));
    let artifact = normal_document_artifact(
        &scratch.0,
        "owner",
        &first,
        &request,
        source,
        "CSV_GROUP_STATISTICS",
    );
    let report: Value = serde_json::from_str(first["text"].as_str().unwrap()).unwrap();
    assert_eq!(report["group_column"], "地域");
    assert_eq!(report["value_column"], "測定");
    let groups = report["groups"].as_array().unwrap();
    assert_eq!(groups.len(), 2);
    for (index, key, rows, normalized, sum, mean, unit) in [
        (0, "北,陸", [1, 3], ["1000", "500"], "1500", "750", "m"),
        (1, "南", [2, 4], ["1", "2"], "3", "3/2", "s"),
    ] {
        assert_eq!(groups[index]["value"], key);
        assert_eq!(
            groups[index]["source_record_numbers"],
            serde_json::json!(rows)
        );
        let stats = &groups[index]["statistics"];
        assert_eq!(stats["count"], 2);
        assert_eq!(stats["normalized_values"], serde_json::json!(normalized));
        assert_eq!(stats["sum"], sum);
        assert_eq!(stats["mean"], mean);
        assert_eq!(stats["unit"], unit);
    }
    let explanation = assert_csv_row_explanation(&scratch.0, &first, &artifact, "group-statistics");
    let text = explanation["text"].as_str().unwrap();
    for expected in [
        "前回のCSVグループ統計",
        "初出順",
        "行数: 4",
        "グループ数: 2",
        "グループ1 \"北,陸\"。件数: 2、合計: 1500、平均: 750、単位: m",
        "グループ2 \"南\"。件数: 2、合計: 3、平均: 3/2、単位: s",
        "計測セル",
    ] {
        assert!(text.contains(expected), "missing {expected:?} in {text}");
    }
    assert_csv_group_explanation_sources(
        text,
        &artifact,
        &["CSV_GROUP_MEMBER", "CSV_GROUP_STATISTIC_VALUE"],
    );
}

fn assert_csv_group_explanation_sources(text: &str, artifact: &Value, kinds: &[&str]) {
    let source = artifact["source_document"].as_str().unwrap();
    let row_count = artifact["parameters"]["row_count"]
        .as_str()
        .unwrap()
        .parse::<usize>()
        .unwrap();
    let mut seen = std::collections::BTreeSet::new();
    for span in artifact["spans"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|span| kinds.contains(&span["kind"].as_str().unwrap()))
    {
        let row = span["row"].as_u64().unwrap() as usize;
        assert!((1..=row_count).contains(&row));
        assert!(seen.insert((row, span["kind"].as_str().unwrap())));
        let start = span["source_start"].as_u64().unwrap() as usize;
        let end = span["source_end"].as_u64().unwrap() as usize;
        let raw = &source[start..end];
        assert_eq!(span["source_text"], raw);
        let marker = format!("元行{row} キー");
        let line = text
            .lines()
            .find(|line| line.contains(&marker))
            .unwrap_or_else(|| panic!("missing {marker:?} in {text}"));
        assert!(line.contains(&format!("byte[{start}..{end})")), "{line}");
        assert!(
            line.contains(&format!(
                "sha256={}",
                minidora_domain::digest(raw.as_bytes())
            )),
            "{line}"
        );
    }
    assert_eq!(seen.len(), row_count * kinds.len());
}

#[test]
fn csv_selection_explanation_preserves_named_columns_and_exact_source_cells() {
    let numeric_name = BASIC_SELECTION_CSV.replacen("\"値 名前\"", "0", 1);
    for (header, source, name, raw_header, empty) in [
        (
            "CSVの列「値 名前」を選択して:",
            BASIC_SELECTION_CSV,
            "値 名前",
            "\"値 名前\"",
            false,
        ),
        (
            "csv-select \"値 名前\"",
            BASIC_SELECTION_CSV,
            "値 名前",
            "\"値 名前\"",
            false,
        ),
        ("csv-select \"0\"", numeric_name.as_str(), "0", "0", false),
        (
            "CSVの列「値 名前」を選択して:",
            "id,\"値 名前\"\r\n",
            "値 名前",
            "\"値 名前\"",
            true,
        ),
    ] {
        let mut raw = vec![raw_header];
        let mut decoded = vec![name];
        if !empty {
            raw.extend([
                "\"猫,北\"",
                "\"犬\r\n南\"",
                "\"\"",
                "",
                "9007199254740993",
                "\"say \"\"hi\"\"\"",
            ]);
            decoded.extend([
                "猫,北",
                "犬\r\n南",
                "",
                "",
                "9007199254740993",
                "say \"hi\"",
            ]);
        }
        let scratch = Scratch::new();
        let request = format!("{header}\n{source}");
        let first = record(&execute(
            &scratch.0,
            "owner",
            &["--input", "select", &request],
        ));
        let artifact =
            normal_document_artifact(&scratch.0, "owner", &first, &request, source, "CSV_SELECT");
        assert_basic_csv_maps(&artifact, source, &raw, &decoded, true);
        assert_eq!(artifact["parameters"]["column_name"], name);
        assert_eq!(artifact["parameters"]["column_index"], "1");
        assert_eq!(artifact["parameters"]["has_header"], "true");
        let selected: Value = serde_json::from_str(first["text"].as_str().unwrap()).unwrap();
        assert_eq!(selected, serde_json::json!(&decoded[1..]));
        let explanation = assert_csv_row_explanation(&scratch.0, &first, &artifact, "select");
        let text = explanation["text"].as_str().unwrap();
        for expected in [
            "前回のCSV列選択".to_string(),
            format!("選択列: {name:?} (元列番号 2)"),
            format!("選択値数: {}", decoded.len() - 1),
        ] {
            assert!(text.contains(&expected), "missing {expected:?} in {text}");
        }
        // Fixed raw tokens plus unique record IDs bind each source row and
        // zero-width empty independently of the saved artifact's span metadata.
        for row in 0..raw.len() {
            let id = if row == 0 {
                "id".to_string()
            } else {
                char::from(b'a' + (row - 1) as u8).to_string()
            };
            let fragment = if row == 0 {
                format!("{id},{}", raw[row])
            } else {
                format!("\r\n{id},{}", raw[row])
            };
            let start = source.find(&fragment).unwrap() + usize::from(row > 0) * 2 + id.len() + 1;
            let end = start + raw[row].len();
            let marker = if row == 0 {
                "列見出し: ".to_string()
            } else {
                format!("元行{row}: 値 {:?}。", decoded[row])
            };
            let line = text
                .lines()
                .find(|line| line.starts_with(&marker))
                .unwrap_or_else(|| panic!("missing {marker:?} in {text}"));
            assert!(line.contains(&format!("byte[{start}..{end})")), "{line}");
            assert!(
                line.contains(&format!(
                    "sha256={}",
                    minidora_domain::digest(raw[row].as_bytes())
                )),
                "{line}"
            );
        }
        if empty {
            assert!(!text.contains("元行1:"), "{text}");
        }
    }
}

const INDEXED_SELECTION_CSV: &str = "r0,\"001\"\r\nr1,001\r\nr2,\"猫\r\n北\"\r\nr3,\"\"\r\nr4,\r\n";

#[test]
fn csv_indexed_selection_explanation_preserves_header_mode_and_exact_cell_maps() {
    let cells = [
        ("r0", "\"001\"", "001"),
        ("r1", "001", "001"),
        ("r2", "\"猫\r\n北\"", "猫\r\n北"),
        ("r3", "\"\"", ""),
        ("r4", "", ""),
    ];
    for (header, has_header) in [
        ("見出しなしCSVの第2列を選択して:", false),
        ("csv-select-index header 2:", true),
    ] {
        let scratch = Scratch::new();
        let request = format!("{header}\r\n{INDEXED_SELECTION_CSV}");
        let first = record(&execute(
            &scratch.0,
            "owner",
            &["--input", "select-index", &request],
        ));
        let artifact = normal_document_artifact(
            &scratch.0,
            "owner",
            &first,
            &request,
            INDEXED_SELECTION_CSV,
            "CSV_SELECT",
        );
        let skip = usize::from(has_header);
        let values: Vec<_> = cells[skip..].iter().map(|cell| cell.2).collect();
        let expected_output = serde_json::to_string(&values).unwrap();
        assert_eq!(first["text"], expected_output);
        assert_eq!(
            artifact["parameters"],
            serde_json::json!({"column_index":"1","has_header":has_header.to_string(),"value_type":"STRING"})
        );
        let original = inspect_normal_case(&scratch.0, "owner");
        assert_eq!(original["bundle"]["meaning"]["intent"]["column_index"], 1);
        assert_eq!(
            original["bundle"]["meaning"]["intent"]["has_header"],
            has_header
        );
        let mut expected_spans = Vec::new();
        let mut source_ranges = Vec::new();
        let mut cursor = 1;
        for (row, &(id, raw, decoded)) in cells.iter().enumerate() {
            let prefix = if row == 0 {
                format!("{id},")
            } else {
                format!("\r\n{id},")
            };
            let start = INDEXED_SELECTION_CSV.find(&prefix).unwrap() + prefix.len();
            let end = start + raw.len();
            assert_eq!(&INDEXED_SELECTION_CSV[start..end], raw);
            source_ranges.push((start, end));
            let is_header = has_header && row == 0;
            let token = serde_json::to_string(decoded).unwrap();
            let output_start = (!is_header).then_some(cursor);
            expected_spans.push(serde_json::json!({
                "kind":if is_header {"CSV_HEADER"} else {"CSV_SELECTED_CELL"},
                "row":row,"column":1,"source_start":start,"source_end":end,
                "source_text":raw,"decoded_value":decoded,"output_start":output_start,
                "output_end":output_start.map(|at|at+token.len())
            }));
            if !is_header {
                assert_eq!(&expected_output[cursor..cursor + token.len()], token);
                cursor += token.len() + 1;
            }
        }
        assert_eq!(cursor, expected_output.len());
        assert_eq!(artifact["spans"], serde_json::json!(expected_spans));
        let explained = assert_table_explanation(
            &scratch.0,
            &first,
            &artifact,
            "previous-csv-table-select-index",
        );
        let text = explained["text"].as_str().unwrap();
        for expected in [
            "選択列: 元列番号 2".to_string(),
            if has_header {
                "先頭行を見出しとして扱い"
            } else {
                "先頭行もデータとして扱い"
            }
            .to_string(),
            format!("選択値数: {}", values.len()),
        ] {
            assert!(text.contains(&expected), "missing {expected:?} in {text}");
        }
        let lines: Vec<_> = text.lines().collect();
        assert_eq!(
            lines
                .iter()
                .filter(|line| line.starts_with("列見出し:"))
                .count(),
            skip
        );
        assert_eq!(
            lines.iter().filter(|line| line.starts_with("元行")).count(),
            values.len()
        );
        let mut previous_line = None;
        for (row, &(_, raw, decoded)) in cells.iter().enumerate() {
            let marker = if has_header && row == 0 {
                "列見出し: ".to_string()
            } else {
                format!("元行{}: 値 {decoded:?}。", row - skip + 1)
            };
            let (start, end) = source_ranges[row];
            let expected = format!(
                "{marker}byte[{start}..{end}) sha256={} 原文={raw:?}",
                minidora_domain::digest(raw.as_bytes())
            );
            let position = lines
                .iter()
                .position(|line| *line == expected)
                .unwrap_or_else(|| panic!("missing {expected:?} in {text}"));
            if let Some(previous) = previous_line {
                assert!(previous < position);
            }
            previous_line = Some(position);
        }
    }
}

#[test]
fn csv_indexed_selection_rejects_invalid_position_mode_width_and_suffix() {
    for (header, reason) in [
        ("見出しなしCSVの第0列を選択して:", "positive"),
        ("csv-select-index 2:", "explicit header mode"),
        ("csv-select-index header 3:", "CSV_COLUMN"),
        (
            "csv-select-index no-header 2 and send:",
            "explicit header mode",
        ),
    ] {
        assert_comparison_stop(header, INDEXED_SELECTION_CSV, reason);
    }
}

fn assert_csv_row_explanation(
    directory: &Path,
    first: &Value,
    artifact: &Value,
    operation: &str,
) -> Value {
    assert_table_explanation(
        directory,
        first,
        artifact,
        &format!("previous-csv-table-{operation}"),
    )
}

fn assert_table_explanation(
    directory: &Path,
    first: &Value,
    artifact: &Value,
    node_prefix: &str,
) -> Value {
    let report_ref = minidora_domain::digest(&minidora_domain::canonical(artifact).unwrap());
    assert_eq!(first["dependencies"], serde_json::json!([report_ref]));
    assert_document_explanation(
        directory,
        first,
        artifact,
        node_prefix,
        "COMPUTED",
        "元資料",
    )
}

fn assert_document_explanation(
    directory: &Path,
    first: &Value,
    artifact: &Value,
    node_prefix: &str,
    guarantee: &str,
    source_label: &str,
) -> Value {
    let original = inspect_normal_case(directory, "owner");
    assert_eq!(original["result"], *first);
    let input_bytes = minidora_domain::canonical(&original["bundle"]).unwrap();
    let input_ref = minidora_domain::digest(&input_bytes);
    let report_bytes = minidora_domain::canonical(artifact).unwrap();
    let report_ref = minidora_domain::digest(&report_bytes);
    let explanation = record(&execute(
        directory,
        "owner",
        &["--input", "explain", "詳しく説明して"],
    ));
    assert_eq!(explanation["lifecycle"], "COMMIT");
    assert_eq!(explanation["guarantee"], guarantee);
    assert_ne!(explanation["text"], first["text"]);
    assert_eq!(explanation["content_plan"]["text"], explanation["text"]);
    assert_eq!(explanation["render"]["status"], "RENDERED");
    assert_eq!(explanation["receipts"], serde_json::json!([]));
    let text = explanation["text"].as_str().unwrap();
    for expected in [
        format!(
            "{source_label} SHA-256: {}",
            artifact["source_sha256"].as_str().unwrap()
        ),
        format!("封印済みInputBundle SHA-256: {input_ref}"),
        format!("report artifact SHA-256: {report_ref}"),
    ] {
        assert!(text.contains(&expected), "missing {expected:?} in {text}");
    }
    assert_eq!(
        explanation["dependencies"],
        serde_json::json!([first["id"], input_ref, report_ref])
    );
    assert_eq!(
        explanation["render"]["actual_sha256"],
        minidora_domain::digest(text.as_bytes())
    );
    let store = minidora_engine::store::Store::open(directory).unwrap();
    assert_eq!(
        store
            .get_bytes("owner", "normal-route", &report_ref)
            .unwrap(),
        report_bytes
    );
    assert!(store.actions("owner", "normal-route").unwrap().is_empty());
    let graph = sequence_artifact(&store, &explanation["execution_graph_ref"]);
    let input_id = format!("{node_prefix}-input");
    let report_id = format!("{node_prefix}-report");
    let nodes = graph[0].as_array().unwrap();
    assert!(nodes.iter().any(|node| node["id"] == input_id
        && node["kind"] == "EVIDENCE"
        && node["requires"] == serde_json::json!([])));
    assert!(nodes.iter().any(|node| node["id"] == report_id
        && node["kind"] == "COMPUTED_VALUE"
        && node["requires"] == serde_json::json!([input_id])));
    assert!(nodes.iter().any(|node| {
        node["id"] == "compute"
            && node["requires"]
                .as_array()
                .unwrap()
                .contains(&Value::String(report_id.clone()))
    }));
    assert_eq!(graph[1][&input_id], input_ref);
    assert_eq!(graph[1][&report_id], report_ref);
    assert_eq!(
        graph[5][&report_id],
        serde_json::json!({input_id:input_ref})
    );
    assert_eq!(graph[5]["compute"][&report_id], report_ref);
    let replayed = record(&execute(
        directory,
        "owner",
        &[
            "--input",
            first["input_id"].as_str().unwrap(),
            original["bundle"]["request"]["text"].as_str().unwrap(),
        ],
    ));
    assert_eq!(replayed, *first);
    let revision = inspect_normal_case(directory, "owner");
    assert_eq!(
        revision["bundle"]["meaning"]["context_result_ref"],
        first["id"]
    );
    assert_eq!(
        revision["prior_results"][first["id"].as_str().unwrap()],
        *first
    );
    assert_eq!(revision["result"], explanation);
    explanation
}

#[test]
fn json_table_group_count_explanation_preserves_decoded_keys_and_original_tokens() {
    const SOURCE: &str = r#"{"columns":["id","\u5206\u985e"],"rows":[["a","\u732b"],["b",""],["c","猫"],["d","open"],["e","\u006fpen"],["f","Open"],["g",""]]}"#;
    const EMPTY: &str = r#"{"columns":["id","\u5206\u985e"],"rows":[]}"#;
    const MEMBERS: [(&str, &[usize]); 4] = [
        ("猫", &[0, 2]),
        ("", &[1, 6]),
        ("open", &[3, 4]),
        ("Open", &[5]),
    ];
    const TOKENS: [&str; 7] = [
        r#""\u732b""#,
        "\"\"",
        "\"猫\"",
        "\"open\"",
        r#""\u006fpen""#,
        "\"Open\"",
        "\"\"",
    ];
    for empty in [false, true] {
        let source = if empty { EMPTY } else { SOURCE };
        for header in [
            "JSON表を列「分類」で件数集計して:",
            "json-table-count-by \"分類\":",
        ] {
            let scratch = Scratch::new();
            let request = format!("{header}\n{source}");
            let first = record(&execute(
                &scratch.0,
                "owner",
                &["--input", "group-count", &request],
            ));
            let report = normal_row_report(&scratch.0, "owner", &first, &request, source);
            assert_eq!(report["group_by"], "分類");
            assert_eq!(report["group_by_index"], 1);
            assert_eq!(report["row_count"], if empty { 0 } else { 7 });
            let groups = report["groups"].as_array().unwrap();
            assert_eq!(groups.len(), if empty { 0 } else { MEMBERS.len() });
            let explanation = assert_table_explanation(
                &scratch.0,
                &first,
                &report,
                "previous-json-table-transform",
            );
            let text = explanation["text"].as_str().unwrap();
            for expected in [
                "前回のJSON表グループ件数".to_string(),
                "初出順".to_string(),
                format!(
                    "行数: {}、グループ数: {}",
                    if empty { 0 } else { 7 },
                    groups.len()
                ),
            ] {
                assert!(text.contains(&expected), "missing {expected:?} in {text}");
            }
            // Literal fixture tokens and row IDs are the oracle, independently
            // of report spans; repeated empty/decoded keys have distinct ranges.
            let check_span =
                |span: &Value, pointer: &str, fragment: &str, offset: usize, token: &str| {
                    let start = source.find(fragment).unwrap() + offset;
                    let end = start + token.len();
                    assert_eq!(&source[start..end], token);
                    assert_eq!(span["source_start"], start);
                    assert_eq!(span["source_end"], end);
                    let hash = minidora_domain::digest(token.as_bytes());
                    assert_eq!(span["source_sha256"], hash);
                    let marker = format!("JSON Pointer {pointer}");
                    let line = text
                        .lines()
                        .find(|line| line.contains(&marker))
                        .unwrap_or_else(|| panic!("missing {marker:?} in {text}"));
                    assert!(line.contains(&format!("byte[{start}..{end})=")), "{line}");
                    assert!(line.contains(&format!("sha256={hash}")), "{line}");
                };
            check_span(
                &report["group_by_source"],
                "/columns/1",
                r#"["id","\u5206\u985e"]"#,
                6,
                r#""\u5206\u985e""#,
            );
            for (index, (group, (value, rows))) in groups.iter().zip(MEMBERS).enumerate() {
                assert_eq!(group["value"], value);
                assert_eq!(group["count"], rows.len());
                assert_eq!(group["source_row_indices"], serde_json::json!(rows));
                let spans = group["member_source_spans"].as_array().unwrap();
                assert_eq!(spans.len(), rows.len());
                let summary = format!("グループ{} {value:?}。件数: {}", index + 1, rows.len());
                assert!(text.contains(&summary), "missing {summary:?} in {text}");
                for (span, row) in spans.iter().zip(rows) {
                    let id = char::from(b'a' + *row as u8);
                    let fragment = format!("[\"{id}\",{}]", TOKENS[*row]);
                    let pointer = format!("/rows/{row}/1");
                    check_span(span, &pointer, &fragment, 5, TOKENS[*row]);
                    assert!(
                        text.contains(&format!("元行{} JSON Pointer {pointer}", row + 1)),
                        "{text}"
                    );
                }
            }
        }
    }
}

#[test]
fn json_table_transform_explanations_replay_the_sealed_input_and_report() {
    for (input_id, request, source, marker) in [
        (
            "filter-explain",
            format!("JSON表の列「状態」が値「open」の行を選択して:\n{ROW_JSON}"),
            ROW_JSON,
            "前回のJSON表フィルター",
        ),
        (
            "sort-explain",
            format!("JSON表の列「順位」を昇順に並べ替えて:\n{ROW_JSON}"),
            ROW_JSON,
            "前回のJSON表整列",
        ),
        (
            "select-explain",
            format!("JSON表の列「値」を選択して:\n{CONVERSION_ESCAPED_JSON}"),
            CONVERSION_ESCAPED_JSON,
            "前回のJSON表列選択",
        ),
        (
            "project-explain",
            format!("JSON表から列[\"補足\",\"名前\"]を抽出して:\n{CONVERSION_ESCAPED_JSON}"),
            CONVERSION_ESCAPED_JSON,
            "前回のJSON表投影",
        ),
    ] {
        let scratch = Scratch::new();
        let first = record(&execute(
            &scratch.0,
            "owner",
            &["--input", input_id, &request],
        ));
        assert_eq!(first["lifecycle"], "COMMIT");
        let report = normal_row_report(&scratch.0, "owner", &first, &request, source);
        let original_revision = inspect_normal_case(&scratch.0, "owner");
        let input_bundle_ref = minidora_domain::digest(
            &minidora_domain::canonical(&original_revision["bundle"]).unwrap(),
        );
        let report_ref = minidora_domain::digest(&minidora_domain::canonical(&report).unwrap());

        let explanation = record(&execute(&scratch.0, "owner", &["詳しく説明して"]));
        assert_eq!(explanation["lifecycle"], "COMMIT");
        let text = explanation["text"].as_str().unwrap();
        assert!(text.contains(marker), "{text}");
        assert!(text.contains("封印済みInputBundle SHA-256"), "{text}");
        assert!(text.contains("report artifact SHA-256"), "{text}");
        assert!(text.contains("byte["), "{text}");
        assert!(text.contains("sha256="), "{text}");
        let dependencies = explanation["dependencies"].as_array().unwrap();
        assert_eq!(
            dependencies,
            &vec![
                first["id"].clone(),
                Value::String(input_bundle_ref),
                Value::String(report_ref),
            ]
        );
        let revision = inspect_normal_case(&scratch.0, "owner");
        assert_eq!(
            revision["bundle"]["meaning"]["context_result_ref"],
            first["id"]
        );
        assert_eq!(revision["result"], explanation);
    }
}

#[test]
fn csv_inner_join_explanation_replays_sealed_rows_and_unmatched_keys() {
    let left = "名前,id\r\n\"犬,\"\"北\"\"\",2\r\n猫,1\r\n狐,4";
    let right = "record_id,分類\r\n1,猫科\r\n3,鳥類\r\n2,イヌ科\r\n";
    let source =
        format!("--- MINIDORA LEFT CSV ---\n{left}\r\n--- MINIDORA RIGHT CSV ---\n{right}");
    let request = format!("CSVを「id」と「record_id」で内部結合して:\n{source}");
    let scratch = Scratch::new();
    let first = record(&execute(
        &scratch.0,
        "owner",
        &["--input", "csv-join-explain", &request],
    ));
    let artifact = normal_document_artifact(
        &scratch.0,
        "owner",
        &first,
        &request,
        &source,
        "CSV_INNER_JOIN",
    );
    let joined: Value = serde_json::from_str(first["text"].as_str().unwrap()).unwrap();
    assert_eq!(
        joined["rows"],
        serde_json::json!([["犬,\"北\"", "2", "2", "イヌ科"], ["猫", "1", "1", "猫科"]])
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
    let original_revision = inspect_normal_case(&scratch.0, "owner");
    let input_bundle_ref =
        minidora_domain::digest(&minidora_domain::canonical(&original_revision["bundle"]).unwrap());
    let report_ref = minidora_domain::digest(&minidora_domain::canonical(&artifact).unwrap());
    assert_eq!(first["dependencies"], serde_json::json!([report_ref]));

    let explanation = record(&execute(&scratch.0, "owner", &["詳しく説明して"]));
    assert_eq!(explanation["lifecycle"], "COMMIT");
    assert_eq!(explanation["guarantee"], "COMPUTED");
    assert_eq!(explanation["content_plan"]["text"], explanation["text"]);
    assert_eq!(explanation["render"]["status"], "RENDERED");
    assert_ne!(explanation["text"], first["text"]);
    let text = explanation["text"].as_str().unwrap();
    for expected in [
        "前回のCSV表内部結合",
        "一致行: 2、左の未一致行: 1、右の未一致行: 1",
        "出力行1 ← 左元行1 + 右元行3",
        "出力行2 ← 左元行2 + 右元行1",
        "左の未一致: 元行3 キー",
        "右の未一致: 元行2 キー",
        "\"id\"",
        "\"record_id\"",
        "封印済みInputBundle SHA-256",
        "report artifact SHA-256",
    ] {
        assert!(text.contains(expected), "missing {expected:?} in {text}");
    }
    for reference in [
        minidora_domain::digest(source.as_bytes()),
        input_bundle_ref.clone(),
        report_ref.clone(),
    ] {
        assert!(text.contains(&reference), "missing {reference} in {text}");
    }
    let spans = artifact["spans"].as_array().unwrap();
    let disclosed: Vec<_> = spans
        .iter()
        .filter(|span| {
            matches!(
                span["kind"].as_str(),
                Some(
                    "CSV_INNER_JOIN_LEFT_CELL"
                        | "CSV_INNER_JOIN_RIGHT_CELL"
                        | "CSV_INNER_JOIN_UNMATCHED_LEFT_KEY"
                        | "CSV_INNER_JOIN_UNMATCHED_RIGHT_KEY"
                )
            )
        })
        .collect();
    assert_eq!(disclosed.len(), 10);
    for span in disclosed {
        let start = span["source_start"].as_u64().unwrap() as usize;
        let end = span["source_end"].as_u64().unwrap() as usize;
        let raw = &source[start..end];
        assert_eq!(span["source_text"], raw);
        assert!(text.contains(&format!("byte[{start}..{end})")));
        assert!(text.contains(&format!(
            "sha256={}",
            minidora_domain::digest(raw.as_bytes())
        )));
    }
    assert_eq!(
        explanation["dependencies"].as_array().unwrap(),
        &vec![
            first["id"].clone(),
            Value::String(input_bundle_ref.clone()),
            Value::String(report_ref.clone()),
        ]
    );
    let store = minidora_engine::store::Store::open(&scratch.0).unwrap();
    let graph = sequence_artifact(&store, &explanation["execution_graph_ref"]);
    let input_id = "previous-csv-table-inner-join-input";
    let report_id = "previous-csv-table-inner-join-report";
    let nodes = graph[0].as_array().unwrap();
    assert!(nodes.iter().any(|node| {
        node["id"] == input_id
            && node["kind"] == "EVIDENCE"
            && node["requires"] == serde_json::json!([])
    }));
    assert!(nodes.iter().any(|node| {
        node["id"] == report_id
            && node["kind"] == "COMPUTED_VALUE"
            && node["requires"] == serde_json::json!([input_id])
    }));
    assert!(nodes.iter().any(|node| {
        node["id"] == "compute"
            && node["requires"]
                .as_array()
                .unwrap()
                .contains(&Value::String(report_id.into()))
    }));
    assert_eq!(graph[1][input_id], input_bundle_ref);
    assert_eq!(graph[1][report_id], report_ref);
    assert_eq!(
        graph[5][report_id],
        serde_json::json!({input_id:input_bundle_ref})
    );
    assert_eq!(graph[5]["compute"][report_id], report_ref);
    assert_eq!(
        explanation["render"]["actual_sha256"],
        minidora_domain::digest(text.as_bytes())
    );
    let revision = inspect_normal_case(&scratch.0, "owner");
    assert_eq!(
        revision["bundle"]["meaning"]["context_result_ref"],
        first["id"]
    );
    assert_eq!(revision["result"], explanation);
}

#[test]
fn csv_inner_join_explanation_stops_on_budget_and_missing_file_permission() {
    let source = "--- MINIDORA LEFT CSV ---\nid,name\r\n1,猫\r\n--- MINIDORA RIGHT CSV ---\nrecord_id,kind\r\n1,猫科\r\n";
    let request = format!("csv-inner-join \"id\" \"record_id\":\n{source}");
    for budget_limited in [true, false] {
        let scratch = Scratch::new();
        let first = record(&execute(
            &scratch.0,
            "owner",
            &["--input", "csv-join-before-stop", &request],
        ));
        assert_eq!(first["lifecycle"], "COMMIT");
        let target = scratch.0.join("unauthorized-explanation.txt");
        let args = if budget_limited {
            vec!["--max-evidence-bytes", "1", "詳しく説明して"]
        } else {
            vec!["--output", target.to_str().unwrap(), "詳しく説明して"]
        };
        let result = stopped(&execute(&scratch.0, "owner", &args), 2, "SUSPEND");
        let code = if budget_limited {
            "RESOURCE_LIMIT:"
        } else {
            "WRITE_PERMISSION_MISSING:"
        };
        assert!(result["reason"].as_str().unwrap().starts_with(code));
        assert_eq!(result["receipts"], serde_json::json!([]));
        assert!(!target.exists());
        let store = minidora_engine::store::Store::open(&scratch.0).unwrap();
        assert!(store.actions("owner", "normal-route").unwrap().is_empty());
        if budget_limited {
            assert_eq!(result["dependencies"], serde_json::json!([]));
            assert!(result["content_plan"].is_null());
        } else {
            assert_eq!(result["dependencies"].as_array().unwrap().len(), 3);
            assert_eq!(result["dependencies"][0], first["id"]);
            assert_eq!(result["render"]["status"], "RENDERED");
        }
        let revision = inspect_normal_case(&scratch.0, "owner");
        assert_eq!(revision["result"], result);
        assert_eq!(
            revision["bundle"]["meaning"]["context_result_ref"],
            first["id"]
        );
    }
}

#[test]
fn json_table_inner_join_explanation_replays_sealed_rows_and_unmatched_keys() {
    let left = r#"{"columns":["名前","id"],"rows":[["犬","北"],["猫","南"],["狐","西"]]}"#;
    let right = r#"{"columns":["record_id","分類"],"rows":[["南","猫科"],["東","鳥類"],["\u5317","イヌ科"]]}"#;
    let source = format!(r#"{{"left":{left},"right":{right}}}"#);
    let request = format!("JSON表を「id」と「record_id」で内部結合して:\n{source}");
    let scratch = Scratch::new();
    let first = record(&execute(
        &scratch.0,
        "owner",
        &["--input", "join-explain", &request],
    ));
    assert_eq!(first["lifecycle"], "COMMIT");
    assert_eq!(first["guarantee"], "SOURCE_REPORT");
    assert_eq!(first["content_plan"]["text"], first["text"]);
    assert_eq!(first["render"]["status"], "RENDERED");
    assert_eq!(
        first["render"]["actual_sha256"],
        minidora_domain::digest(first["text"].as_str().unwrap().as_bytes())
    );
    let report: Value = serde_json::from_str(first["text"].as_str().unwrap()).unwrap();
    assert_eq!(
        report["source_sha256"],
        minidora_domain::digest(source.as_bytes())
    );
    assert_eq!(report["rows"].as_array().unwrap().len(), 2);
    assert_eq!(report["unmatched_left"].as_array().unwrap().len(), 1);
    assert_eq!(report["unmatched_right"].as_array().unwrap().len(), 1);

    let original_revision = inspect_normal_case(&scratch.0, "owner");
    let input_bundle_ref =
        minidora_domain::digest(&minidora_domain::canonical(&original_revision["bundle"]).unwrap());
    let report_ref = minidora_domain::digest(&minidora_domain::canonical(&report).unwrap());
    let explanation = record(&execute(&scratch.0, "owner", &["詳しく説明して"]));
    assert_eq!(explanation["lifecycle"], "COMMIT");
    let text = explanation["text"].as_str().unwrap();
    for expected in [
        "前回のJSON表内部結合",
        "一致行: 2; 左の未一致行: 1; 右の未一致行: 1",
        "出力行1 ← 左元行1 + 右元行3",
        "左の未一致: 元行3 キー",
        "右の未一致: 元行2 キー",
        "封印済みInputBundle SHA-256",
        "report artifact SHA-256",
        "byte[",
        "sha256=",
    ] {
        assert!(text.contains(expected), "missing {expected:?} in {text}");
    }
    assert_eq!(
        explanation["dependencies"].as_array().unwrap(),
        &vec![
            first["id"].clone(),
            Value::String(input_bundle_ref.clone()),
            Value::String(report_ref.clone()),
        ]
    );
    let store = minidora_engine::store::Store::open(&scratch.0).unwrap();
    let graph = sequence_artifact(&store, &explanation["execution_graph_ref"]);
    let input_id = "previous-json-table-transform-input";
    let report_id = "previous-json-table-transform-report";
    let nodes = graph[0].as_array().unwrap();
    assert!(nodes.iter().any(|node| node["id"] == input_id));
    assert!(nodes.iter().any(|node| {
        node["id"] == report_id
            && node["kind"] == "COMPUTED_VALUE"
            && node["requires"] == serde_json::json!([input_id])
    }));
    assert_eq!(graph[1][input_id], input_bundle_ref);
    assert_eq!(graph[1][report_id], report_ref);
    assert_eq!(
        graph[5][report_id],
        serde_json::json!({input_id:input_bundle_ref})
    );
    assert_eq!(
        explanation["render"]["actual_sha256"],
        minidora_domain::digest(text.as_bytes())
    );
    let revision = inspect_normal_case(&scratch.0, "owner");
    assert_eq!(
        revision["bundle"]["meaning"]["context_result_ref"],
        first["id"]
    );
    assert_eq!(revision["result"], explanation);
}

#[test]
fn json_table_comparison_explanation_replays_two_sided_cells_and_changed_rows() {
    let left = r#"{"columns":["id","name"],"rows":[["north","\u732b"],["removed","犬"]]}"#;
    let right = r#"{"columns":["id","name"],"rows":[["north","狐"],["added","鳥"]]}"#;
    let source = format!(r#"{{"left":{left},"right":{right}}}"#);
    let request = format!("JSON表を列「id」で比較して:\n{source}");
    let scratch = Scratch::new();
    let first = record(&execute(
        &scratch.0,
        "owner",
        &["--input", "compare-explain", &request],
    ));
    let report = normal_row_report(&scratch.0, "owner", &first, &request, &source);
    assert_eq!(report["equal"], false);
    assert_eq!(report["changes"].as_array().unwrap().len(), 3);
    let cell_change = &report["changes"][0];
    assert_eq!(cell_change["change"], "CELL_CHANGED");
    assert_eq!(cell_change["key"], "north");
    assert_eq!(cell_change["column"], "name");
    assert_eq!(cell_change["left_pointer"], "/left/rows/0/1");
    assert_eq!(cell_change["right_pointer"], "/right/rows/0/1");
    for (side, raw) in [("left", r#""\u732b""#), ("right", r#""狐""#)] {
        let span = &cell_change[side];
        let start = span["source_start"].as_u64().unwrap() as usize;
        let end = span["source_end"].as_u64().unwrap() as usize;
        assert_eq!(&source[start..end], raw);
        assert_eq!(
            span["source_sha256"],
            minidora_domain::digest(raw.as_bytes())
        );
    }
    assert_eq!(report["changes"][1]["change"], "ROW_REMOVED");
    assert_eq!(report["changes"][1]["left_pointer"], "/left/rows/1");
    assert_eq!(report["changes"][2]["change"], "ROW_ADDED");
    assert_eq!(report["changes"][2]["right_pointer"], "/right/rows/1");
    for change in report["changes"].as_array().unwrap() {
        for side in ["left", "right"] {
            let Some(span) = change[side].as_object() else {
                continue;
            };
            let start = span["source_start"].as_u64().unwrap() as usize;
            let end = span["source_end"].as_u64().unwrap() as usize;
            let raw = &source[start..end];
            assert_eq!(
                span["source_sha256"],
                minidora_domain::digest(raw.as_bytes())
            );
        }
    }

    let original_revision = inspect_normal_case(&scratch.0, "owner");
    let input_bundle_ref =
        minidora_domain::digest(&minidora_domain::canonical(&original_revision["bundle"]).unwrap());
    let report_ref = minidora_domain::digest(&minidora_domain::canonical(&report).unwrap());
    let explanation = record(&execute(&scratch.0, "owner", &["詳しく説明して"]));
    assert_eq!(explanation["lifecycle"], "COMMIT");
    let text = explanation["text"].as_str().unwrap();
    for expected in [
        "前回のJSON表比較",
        "比較キー: \"id\"; 結果: 差分あり",
        "差分: 3件 (セル変更: 1; 左側のみの行: 1; 右側のみの行: 1)",
        "セル変更 キー \"north\" / 列 \"name\"",
        "左 \"猫\" /left/rows/0/1",
        "右 \"狐\" /right/rows/0/1",
        "/left/rows/0/1",
        "/right/rows/0/1",
        "左側のみの行 キー \"removed\"",
        "右側のみの行 キー \"added\"",
        "値の意味や現実の真偽は判定していません。",
        "封印済みInputBundle SHA-256",
        "report artifact SHA-256",
        "byte[",
        "sha256=",
    ] {
        assert!(text.contains(expected), "missing {expected:?} in {text}");
    }
    for change in report["changes"].as_array().unwrap() {
        for side in ["left", "right"] {
            let Some(span) = change[side].as_object() else {
                continue;
            };
            let start = span["source_start"].as_u64().unwrap() as usize;
            let end = span["source_end"].as_u64().unwrap() as usize;
            assert!(text.contains(&format!("byte[{start}..{end})")));
            assert!(text.contains(span["source_sha256"].as_str().unwrap()));
        }
    }
    assert_eq!(
        explanation["dependencies"].as_array().unwrap(),
        &vec![
            first["id"].clone(),
            Value::String(input_bundle_ref.clone()),
            Value::String(report_ref.clone()),
        ]
    );
    let store = minidora_engine::store::Store::open(&scratch.0).unwrap();
    let graph = sequence_artifact(&store, &explanation["execution_graph_ref"]);
    let input_id = "previous-json-table-transform-input";
    let report_id = "previous-json-table-transform-report";
    let nodes = graph[0].as_array().unwrap();
    assert!(nodes.iter().any(|node| node["id"] == input_id));
    assert!(nodes.iter().any(|node| {
        node["id"] == report_id
            && node["kind"] == "COMPUTED_VALUE"
            && node["requires"] == serde_json::json!([input_id])
    }));
    assert_eq!(graph[1][input_id], input_bundle_ref);
    assert_eq!(graph[1][report_id], report_ref);
    assert_eq!(
        graph[5][report_id],
        serde_json::json!({input_id:input_bundle_ref})
    );
    assert_eq!(
        explanation["render"]["actual_sha256"],
        minidora_domain::digest(text.as_bytes())
    );
    let revision = inspect_normal_case(&scratch.0, "owner");
    assert_eq!(
        revision["bundle"]["meaning"]["context_result_ref"],
        first["id"]
    );
    assert_eq!(revision["result"], explanation);

    let equal_scratch = Scratch::new();
    let equal_source = format!(r#"{{"left":{left},"right":{left}}}"#);
    let equal_request = format!("json-table-compare \"id\":\n{equal_source}");
    let equal_first = record(&execute(
        &equal_scratch.0,
        "owner",
        &["--input", "equal-compare-explain", &equal_request],
    ));
    let equal_result = record(&execute(&equal_scratch.0, "owner", &["詳しく説明して"]));
    assert_eq!(equal_result["lifecycle"], "COMMIT");
    assert!(
        equal_result["text"]
            .as_str()
            .unwrap()
            .contains("結果: 一致")
    );
    assert!(
        equal_result["text"]
            .as_str()
            .unwrap()
            .contains("差分: 0件 (セル変更: 0; 左側のみの行: 0; 右側のみの行: 0)")
    );
    assert_eq!(equal_result["dependencies"][0], equal_first["id"]);
}

#[test]
fn csv_table_comparison_explanation_replays_two_sided_cells_and_changed_rows() {
    let source = comparison_csv(COMPARE_LEFT_CSV, COMPARE_RIGHT_CSV);
    let request = format!("CSVを列「id」で2つの表を比較して:\n{source}");
    let scratch = Scratch::new();
    let first = record(&execute(
        &scratch.0,
        "owner",
        &["--input", "csv-compare-explain", &request],
    ));
    let artifact = normal_document_artifact(
        &scratch.0,
        "owner",
        &first,
        &request,
        &source,
        "CSV_TABLE_COMPARE",
    );
    assert_csv_comparison(&artifact, &source, true, false);
    let report_ref = minidora_domain::digest(&minidora_domain::canonical(&artifact).unwrap());
    let original_revision = inspect_normal_case(&scratch.0, "owner");
    let input_bundle_ref =
        minidora_domain::digest(&minidora_domain::canonical(&original_revision["bundle"]).unwrap());

    let explanation = record(&execute(&scratch.0, "owner", &["詳しく説明して"]));
    assert_eq!(explanation["lifecycle"], "COMMIT");
    let text = explanation["text"].as_str().unwrap();
    for expected in [
        "前回のCSV表比較",
        "比較キー: \"id\"; 結果: 差分あり (デコード後文字列の完全一致)",
        "差分: 4件 (セル変更: 2; 左側のみの行: 1; 右側のみの行: 1)",
        "セル変更 キー \"2\" / 列 \"名前\"",
        "セル変更 キー \"2\" / 列 \"値\"",
        "左側のみの行 元行2 キー \"gone\"",
        "右側のみの行 元行3 キー \"new\"",
        "値の意味、数値としての同一性、現実の真偽は判定していません。",
        "キー見出し:",
        "封印済みInputBundle SHA-256",
        "report artifact SHA-256",
        "byte[",
        "sha256=",
    ] {
        assert!(text.contains(expected), "missing {expected:?} in {text}");
    }
    for span in artifact["spans"].as_array().unwrap().iter().filter(|span| {
        matches!(
            span["kind"].as_str(),
            Some(
                "CSV_COMPARE_LEFT_CELL"
                    | "CSV_COMPARE_RIGHT_CELL"
                    | "CSV_COMPARE_REMOVED_CELL"
                    | "CSV_COMPARE_ADDED_CELL"
            )
        )
    }) {
        let start = span["source_start"].as_u64().unwrap() as usize;
        let end = span["source_end"].as_u64().unwrap() as usize;
        let raw = &source[start..end];
        assert!(text.contains(&format!("byte[{start}..{end})")));
        assert!(text.contains(&format!(
            "sha256={}",
            minidora_domain::digest(raw.as_bytes())
        )));
    }
    assert_eq!(
        explanation["dependencies"].as_array().unwrap(),
        &vec![
            first["id"].clone(),
            Value::String(input_bundle_ref.clone()),
            Value::String(report_ref.clone()),
        ]
    );
    let store = minidora_engine::store::Store::open(&scratch.0).unwrap();
    let graph = sequence_artifact(&store, &explanation["execution_graph_ref"]);
    let input_id = "previous-csv-table-comparison-input";
    let report_id = "previous-csv-table-comparison-report";
    let nodes = graph[0].as_array().unwrap();
    assert!(nodes.iter().any(|node| node["id"] == input_id));
    assert!(nodes.iter().any(|node| {
        node["id"] == report_id
            && node["kind"] == "COMPUTED_VALUE"
            && node["requires"] == serde_json::json!([input_id])
    }));
    assert_eq!(graph[1][input_id], input_bundle_ref);
    assert_eq!(graph[1][report_id], report_ref);
    assert_eq!(
        graph[5][report_id],
        serde_json::json!({input_id:input_bundle_ref})
    );
    assert_eq!(
        explanation["render"]["actual_sha256"],
        minidora_domain::digest(text.as_bytes())
    );
    let revision = inspect_normal_case(&scratch.0, "owner");
    assert_eq!(
        revision["bundle"]["meaning"]["context_result_ref"],
        first["id"]
    );
    assert_eq!(revision["result"], explanation);

    let equal_source = comparison_csv(COMPARE_LEFT_CSV, COMPARE_EQUAL_CSV);
    let equal_request = format!("csv-compare-by \"id\":\n{equal_source}");
    let equal_scratch = Scratch::new();
    let equal_first = record(&execute(
        &equal_scratch.0,
        "owner",
        &["--input", "csv-equal-compare-explain", &equal_request],
    ));
    assert_eq!(equal_first["lifecycle"], "COMMIT");
    let equal_result = record(&execute(&equal_scratch.0, "owner", &["詳しく説明して"]));
    assert_eq!(equal_result["lifecycle"], "COMMIT");
    let equal_text = equal_result["text"].as_str().unwrap();
    assert!(equal_text.contains("比較キー: \"id\"; 結果: 一致"));
    assert!(equal_text.contains("差分: 0件 (セル変更: 0; 左側のみの行: 0; 右側のみの行: 0)"));
    assert_eq!(equal_result["dependencies"][0], equal_first["id"]);
    assert_eq!(equal_result["dependencies"].as_array().unwrap().len(), 3);
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
fn table_conversion_explanation_pairs_original_and_output_cell_evidence() {
    for (header, source, to_json, has_header, escaped_cat) in [
        (
            "CSVをJSONに変換して（先頭行をヘッダーとして）:",
            CONVERSION_CSV,
            true,
            true,
            false,
        ),
        ("csv-to-json no-header:", CONVERSION_CSV, true, false, false),
        (
            "JSONをCSVに変換して:",
            CONVERSION_ESCAPED_JSON,
            false,
            true,
            true,
        ),
    ] {
        let scratch = Scratch::new();
        let request = format!("{header}\n{source}");
        let first = record(&execute(
            &scratch.0,
            "owner",
            &["--input", "conversion", &request],
        ));
        let operation = if to_json {
            "CSV_TO_JSON"
        } else {
            "JSON_TO_CSV"
        };
        let artifact =
            normal_document_artifact(&scratch.0, "owner", &first, &request, source, operation);
        assert_conversion_fixture_spans(&artifact, has_header, to_json, escaped_cat);
        let output = first["text"].as_str().unwrap();
        if to_json {
            let table: Value = serde_json::from_str(output).unwrap();
            assert_eq!(
                table,
                serde_json::json!({
                    "columns": if has_header { serde_json::json!(CONVERSION_VALUES[0]) } else { Value::Null },
                    "rows": if has_header { serde_json::json!(&CONVERSION_VALUES[1..]) } else { serde_json::json!(CONVERSION_VALUES) }
                })
            );
        } else {
            assert_eq!(output, CONVERSION_CSV);
        }
        let explanation = assert_csv_row_explanation(&scratch.0, &first, &artifact, "conversion");
        let text = explanation["text"].as_str().unwrap();
        for expected in [
            if to_json {
                "前回のCSV→JSON変換"
            } else {
                "前回のJSON→CSV変換"
            }
            .to_string(),
            format!("ヘッダー: {}", if has_header { "あり" } else { "なし" }),
            format!("データ行数: {}、列数: 3", if has_header { 2 } else { 3 }),
            format!(
                "変換結果 SHA-256: {}",
                minidora_domain::digest(output.as_bytes())
            ),
        ] {
            assert!(text.contains(&expected), "missing {expected:?} in {text}");
        }
        for physical_row in 0..3 {
            let is_header = has_header && physical_row == 0;
            for column in 0..3 {
                let (marker, pointer) = if is_header {
                    (
                        format!("見出し列{}", column + 1),
                        format!("/columns/{column}"),
                    )
                } else {
                    let data_row = physical_row - usize::from(has_header);
                    if data_row * 3 + column >= 8 {
                        continue; // The explanation displays at most eight data cells.
                    }
                    (
                        format!("元行{}・列{}", data_row + 1, column + 1),
                        format!("/rows/{data_row}/{column}"),
                    )
                };
                let line = text
                    .lines()
                    .find(|line| line.starts_with(&format!("{marker} ")))
                    .unwrap_or_else(|| panic!("missing {marker:?} in {text}"));
                assert!(line.contains(&pointer), "{line}");
                assert!(
                    line.contains(&format!(
                        "値 {:?}。",
                        CONVERSION_VALUES[physical_row][column]
                    )),
                    "{line}"
                );
                // The fixture locators use whole known rows, not report offsets
                // or ambiguous individual quote/empty-string occurrences.
                let (start, end) =
                    conversion_fixture_token(source, physical_row, column, !to_json, escaped_cat);
                let (output_start, output_end) =
                    conversion_fixture_token(output, physical_row, column, to_json, false);
                let source_hash = minidora_domain::digest(&source.as_bytes()[start..end]);
                let output_hash =
                    minidora_domain::digest(&output.as_bytes()[output_start..output_end]);
                assert!(
                    line.contains(&format!("元 byte[{start}..{end}) sha256={source_hash}")),
                    "{line}"
                );
                assert!(
                    line.contains(&format!(
                        "出力 byte[{output_start}..{output_end}) sha256={output_hash}"
                    )),
                    "{line}"
                );
            }
        }
    }
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

fn code_ref(name: &str) -> Value {
    serde_json::json!({"種別":"参照","名前":name})
}
fn code_constant(key: &str) -> Value {
    serde_json::json!({"種別":"定数参照","キー":key})
}
fn code_binary(kind: &str, operation: &str, a: Value, b: Value) -> Value {
    serde_json::json!({"種別":kind,"演算":operation,"左":a,"右":b})
}
fn code_return(expression: Value) -> Value {
    serde_json::json!({"種別":"返却","式":expression})
}
fn code_call(name: &str, arguments: Vec<Value>) -> Value {
    serde_json::json!({"種別":"呼出","関数":name,"引数":arguments})
}
fn code_document(name: &str, args: &[&str], body: Vec<Value>, constants: Value) -> Value {
    serde_json::json!({"仕様":{"名前":name,"引数":args,"手順":body},"定数":constants})
}
fn code_square_sum_document() -> Value {
    code_document(
        "条件付き二乗和",
        &["数列"],
        vec![
            serde_json::json!({"種別":"代入","名前":"累計","式":code_constant("初期値")}),
            serde_json::json!({"種別":"反復","変数":"数","対象":code_ref("数列"),"手順":[
                {"種別":"分岐","条件":code_binary("比較","超",code_ref("数"),code_constant("閾値")),"真":[
                    {"種別":"代入","名前":"累計","式":code_binary("算術","加算",code_ref("累計"),code_binary("算術","乗算",code_ref("数"),code_ref("数")))}
                ],"偽":[]}
            ]}),
            code_return(code_ref("累計")),
        ],
        serde_json::json!({"初期値":0,"閾値":2}),
    )
}

fn assert_generated_code_artifact(
    directory: &Path,
    result: &Value,
    request: &str,
    document: &str,
) -> Value {
    basic_raw_result(directory, result, request, Some(document));
    assert!(result["calculation"].is_null());
    assert_eq!(result["dependencies"].as_array().unwrap().len(), 1);
    let store = minidora_engine::store::Store::open(directory).unwrap();
    let read = |sha: &Value| -> Value {
        let sha = sha.as_str().unwrap();
        let bytes = store.get_bytes("owner", "normal-route", sha).unwrap();
        assert_eq!(minidora_domain::digest(&bytes), sha);
        serde_json::from_slice(&bytes).unwrap()
    };
    let report = read(&result["dependencies"][0]);
    assert_eq!(report["source"], result["text"]);
    assert_eq!(report["source_document"], document);
    assert_eq!(
        report["source_document_sha256"],
        minidora_domain::digest(document.as_bytes())
    );
    assert_eq!(report["source_sha256"], result["render"]["actual_sha256"]);
    assert_eq!(report["version"], "structured-python-generator-v1-ucd15");
    assert_eq!(
        report["target_runtime"],
        "Python 3.12 / Unicode 15.0.0 identifiers"
    );
    assert_eq!(
        report["validation"],
        "EXPLICIT_IR_STRUCTURE_AND_CONSTRUCTED_AST_BOUNDS"
    );
    for field in ["python_parse", "execution", "input_output_tests"] {
        assert_eq!(report[field], "NOT_RUN");
    }
    assert!(
        report["source"]
            .as_str()
            .unwrap()
            .starts_with("# Python 3.12; execution and input/output tests: NOT_RUN.\n")
    );
    let doc: Value = serde_json::from_str(document).unwrap();
    for (field, key) in [("specification", "仕様"), ("constants", "定数")] {
        let expected = serde_json::to_string(&doc[key]).unwrap();
        let start = document.find(&expected).unwrap();
        assert_eq!(
            report[field],
            serde_json::json!({"start":start,"end":start+expected.len(),"text":expected})
        );
    }
    let saved = inspect_normal_case(directory, "owner");
    assert_eq!(
        saved["bundle"]["meaning"]["intent"]["kind"],
        "GENERATE_CODE"
    );
    let graph = read(&result["execution_graph_ref"]);
    let observed = &graph[1];
    assert_eq!(
        read(&observed["compute"]),
        serde_json::json!([null, [report["source"]], [], result["dependencies"]])
    );
    assert_eq!(read(&observed["content"]), result["content_plan"]);
    assert_eq!(read(&observed["render"]), result["render"]);
    assert_eq!(graph[5]["compute"], serde_json::json!({}));
    assert_eq!(
        graph[5]["content"],
        serde_json::json!({"compute":observed["compute"]})
    );
    assert_eq!(
        graph[5]["render"],
        serde_json::json!({"content":observed["content"]})
    );
    let goal = read(&saved["goal_sha256"]);
    assert_eq!(goal["intent"], saved["bundle"]["meaning"]["intent"]);
    assert_eq!(read(&goal["required_graph_ref"]), graph[0]);
    if !saved["bundle"]["request"]["output_path"].is_null() {
        assert_eq!(
            graph[5]["file"],
            serde_json::json!({"render":observed["render"]})
        );
        assert_eq!(read(&observed["file"]), result["receipts"][0]);
    }
    assert!(
        store
            .get_bytes(
                "stranger",
                "normal-route",
                result["dependencies"][0].as_str().unwrap()
            )
            .is_err()
    );
    report
}

#[test]
fn structured_code_generation_route_headers_and_actual_artifacts() {
    let value = code_square_sum_document();
    let document = format!(" \r\n{} \t\n", serde_json::to_string(&value).unwrap());
    let expected = "# Python 3.12; execution and input/output tests: NOT_RUN.\ndef 条件付き二乗和(数列, 定数):\n    累計 = 定数[\"初期値\"]\n    for 数 in 数列:\n        if (数 > 定数[\"閾値\"]):\n            累計 = (累計 + (数 * 数))\n        else:\n            pass\n    return 累計\n";
    for header in ["コードを生成して:", "code-generate:", "CoDe-GeNeRaTe:"] {
        for newline in ["\n", "\r\n"] {
            let scratch = Scratch::new();
            let request = format!("{header}{newline}{document}");
            let result = record(&execute(
                &scratch.0,
                "owner",
                &["--max-actions", "3", &request],
            ));
            let report = assert_generated_code_artifact(&scratch.0, &result, &request, &document);
            assert_eq!(result["text"], expected);
            assert_eq!(report["function_name"], "条件付き二乗和");
            assert_eq!(report["parameters"], serde_json::json!(["数列", "定数"]));
            assert_eq!(report["ir_nodes"], 17);
            assert_eq!(report["constructed_ast_nodes"], 45);
            assert_eq!(report["constructed_ast_depth"], 8);
        }
    }
}

#[test]
fn generated_python_is_explained_from_the_prior_ir_and_keeps_its_lineage() {
    let scratch = Scratch::new();
    let value = code_document(
        "増分",
        &["x"],
        vec![code_return(code_binary(
            "算術",
            "加算",
            code_ref("x"),
            code_constant("幅"),
        ))],
        serde_json::json!({"幅": 1}),
    );
    let document = serde_json::to_string(&value).unwrap();
    let request = format!("code-generate:\n{document}");
    let generated = record(&execute(&scratch.0, "owner", &[&request]));
    assert_eq!(generated["lifecycle"], "COMMIT");
    assert!(generated["text"].as_str().unwrap().contains("x +"));
    let report_ref = generated["dependencies"][0].as_str().unwrap().to_owned();

    let explanation = record(&execute(
        &scratch.0,
        "owner",
        &["--input", "explain-generated-code", "詳しく説明して"],
    ));
    assert_eq!(explanation["lifecycle"], "COMMIT");
    assert_eq!(explanation["guarantee"], "COMPUTED");
    let explanation_text = explanation["text"].as_str().unwrap();
    assert!(explanation_text.contains("前回生成した関数「増分」"));
    assert!(explanation_text.contains("入力値「x」に定数辞書の「幅」を足した値を返します。"));
    assert!(
        explanation_text.contains("Python構文解析=NOT_RUN, 実行=NOT_RUN, 入出力テスト=NOT_RUN")
    );
    let dependencies = explanation["dependencies"].as_array().unwrap();
    assert!(dependencies.contains(&generated["id"]));
    assert!(dependencies.contains(&serde_json::json!(report_ref)));

    let saved = inspect_normal_case(&scratch.0, "owner");
    assert_eq!(
        saved["bundle"]["meaning"]["parser_version"],
        minidora_engine::input::PARSER_VERSION
    );
    assert_eq!(
        saved["bundle"]["meaning"]["context_result_ref"],
        generated["id"]
    );
}

#[test]
fn generated_code_is_consumed_by_finite_native_verification_and_keeps_lineage() {
    let scratch = Scratch::new();
    let value = code_document(
        "増分",
        &["x"],
        vec![code_return(code_binary(
            "算術",
            "加算",
            code_ref("x"),
            code_constant("幅"),
        ))],
        serde_json::json!({"幅": 1}),
    );
    let document = serde_json::to_string(&value).unwrap();
    let generate_request = format!("code-generate:\n{document}");
    let generated = record(&execute(&scratch.0, "owner", &[&generate_request]));
    assert_eq!(generated["lifecycle"], "COMMIT");
    assert_eq!(generated["guarantee"], "COMPUTED");
    let source_ref = generated["dependencies"][0].as_str().unwrap().to_owned();

    let cases = serde_json::json!({
        "試験":[
            {"引数":{"x":3,"定数":{"幅":1}},"期待値":4},
            {"引数":{"x":0,"定数":{"幅":-2}},"期待値":-2}
        ]
    });
    let request = format!(
        "生成コードを検証して:\n{}",
        serde_json::to_string(&cases).unwrap()
    );
    let verified = record(&execute(
        &scratch.0,
        "owner",
        &["--input", "verify-generated-code", &request],
    ));
    assert_eq!(verified["lifecycle"], "COMMIT");
    assert_eq!(verified["guarantee"], "COMPUTED");
    assert!(verified["text"].as_str().unwrap().contains("2件中2件一致"));
    assert!(verified["text"].as_str().unwrap().contains("試験1: 一致"));
    assert!(verified["text"].as_str().unwrap().contains("試験2: 一致"));
    assert!(
        verified["text"]
            .as_str()
            .unwrap()
            .contains("ネイティブ評価")
    );
    let dependencies = verified["dependencies"].as_array().unwrap();
    assert_eq!(dependencies.len(), 3);
    assert_eq!(dependencies[0], generated["id"]);
    assert_eq!(dependencies[1], source_ref);

    let store = minidora_engine::store::Store::open(&scratch.0).unwrap();
    let verification_ref = verified["dependencies"][2].as_str().unwrap();
    let verification_bytes = store
        .get_bytes("owner", "normal-route", verification_ref)
        .unwrap();
    assert_eq!(
        minidora_domain::digest(&verification_bytes),
        verification_ref
    );
    let report: Value = serde_json::from_slice(&verification_bytes).unwrap();
    assert_eq!(report["operation"], "VERIFY");
    assert_eq!(report["source"], generated["text"]);
    assert_eq!(
        report["source_sha256"],
        generated["render"]["actual_sha256"]
    );
    assert_eq!(report["verification"]["case_count"], 2);
    assert_eq!(report["verification"]["all_matched"], true);
    assert!(
        report["assurance"]
            .as_str()
            .unwrap()
            .contains("ホストPython、exec、eval")
    );

    let saved = inspect_normal_case(&scratch.0, "owner");
    assert_eq!(
        saved["bundle"]["meaning"]["parser_version"],
        minidora_engine::input::PARSER_VERSION
    );
    assert_eq!(
        saved["bundle"]["meaning"]["intent"]["kind"],
        "VERIFY_PREVIOUS_CODE"
    );
    assert_eq!(
        saved["bundle"]["meaning"]["context_result_ref"],
        generated["id"]
    );
}

#[test]
fn explicit_text_composition_normal_route_preserves_materials_and_artifact_lineage() {
    let material = "ねこ\n地元で開催\n雨天中止";
    let title_end = "ねこ".len();
    let event_start = "ねこ\n".len();
    let event_end = event_start + "地元で開催".len();
    let condition_start = "ねこ\n地元で開催\n".len();
    let body = serde_json::json!({
        "materials":[{"id":"source","text":material}],
        "units":[
            {"id":"title","kind":"heading","fragments":[{"material":"source","start":0,"end":title_end}],"with":[],"required":false,"protected":false},
            {"id":"event","kind":"bullet","fragments":[{"material":"source","start":event_start,"end":event_end}],"with":[],"required":false,"protected":false},
            {"id":"weather","kind":"condition","fragments":[{"material":"source","start":condition_start,"end":material.len()}],"with":[],"required":false,"protected":false}
        ],
        "order":["title","event"],
        "max_chars":128
    }).to_string();
    for header in ["文章を構成して:", "text-compose:"] {
        let scratch = Scratch::new();
        let request = format!("{header}\n{body}");
        let result = record(&execute(&scratch.0, "owner", &[&request]));
        assert_eq!(result["lifecycle"], "COMMIT");
        assert_eq!(result["guarantee"], "USER_PREMISE");
        assert_eq!(result["text"], "# ねこ\n\n- 地元で開催\n\n条件: 雨天中止");
        let saved = inspect_normal_case(&scratch.0, "owner");
        let intent = &saved["bundle"]["meaning"]["intent"];
        assert_eq!(intent["kind"], "COMPOSE_TEXT");
        assert_eq!(intent["document"], body);
        let dependencies = result["dependencies"].as_array().unwrap();
        assert_eq!(dependencies.len(), 1);
        let store = minidora_engine::store::Store::open(&scratch.0).unwrap();
        let bytes = store
            .get_bytes("owner", "normal-route", dependencies[0].as_str().unwrap())
            .unwrap();
        assert_eq!(
            minidora_domain::digest(&bytes),
            dependencies[0].as_str().unwrap()
        );
        let artifact: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(artifact["source_document"], body);
        assert_eq!(artifact["text"], result["text"]);
        assert_eq!(
            artifact["selected_unit_ids"],
            serde_json::json!(["title", "event", "weather"])
        );
        assert_eq!(artifact["omitted_unit_ids"], serde_json::json!([]));
        assert_eq!(artifact["operation"], "TEXT_COMPOSE_EXPLICIT_FRAGMENTS");
        assert!(
            artifact["spans"]
                .as_array()
                .unwrap()
                .iter()
                .any(|span| span["origin"]["kind"] == "MATERIAL_TEXT")
        );
        assert!(
            artifact["spans"]
                .as_array()
                .unwrap()
                .iter()
                .any(|span| span["origin"]["kind"] == "UNIT_DIRECTIVE")
        );
        assert_eq!(saved["result"], result);
        assert_eq!(saved["bundle"]["request"]["text"], request);
    }

    let scratch = Scratch::new();
    let too_short = serde_json::json!({
        "materials":[{"id":"source","text":"長い見出し"}],
        "units":[{"id":"title","kind":"heading","fragments":[{"material":"source","start":0,"end":"長い見出し".len()}],"with":[],"required":true,"protected":false}],
        "order":["title"],
        "max_chars":2
    }).to_string();
    let request = format!("text-compose:\n{too_short}");
    let result = stopped(&execute(&scratch.0, "owner", &[&request]), 2, "SUSPEND");
    assert!(
        result["reason"]
            .as_str()
            .unwrap()
            .contains("TEXT_COMPOSITION_CHAR_LIMIT")
    );
    assert!(result["dependencies"].as_array().unwrap().is_empty());
}

#[test]
fn text_composition_explanation_preserves_escaped_material_layout_and_effective_units() {
    let scratch = Scratch::new();
    let material = "猫🙂KeepOmit";
    let raw_material = r#""\u732b🙂KeepOmit""#;
    let declarations = serde_json::json!([
        {"id":"condition","kind":"condition","fragments":[{"material":"source","start":7,"end":11}],"with":[],"required":false,"protected":false},
        {"id":"optional","kind":"paragraph","fragments":[{"material":"source","start":11,"end":15}],"with":[],"required":false,"protected":false},
        {"id":"title","kind":"heading","fragments":[{"material":"source","start":0,"end":7}],"with":[],"required":false,"protected":false}
    ]);
    let body = format!(
        r#"{{"materials":[{{"id":"source","text":{raw_material}}}],"units":{declarations},"order":["title"],"max_chars":64}}"#
    );
    assert_eq!(
        serde_json::from_str::<String>(raw_material).unwrap(),
        material
    );
    assert_eq!(body.matches(raw_material).count(), 1);
    let material_start = body.find(raw_material).unwrap();
    let material_end = material_start + raw_material.len();
    let directive_range = |index: usize| {
        let raw = declarations[index].to_string();
        assert_eq!(body.matches(&raw).count(), 1);
        let start = body.find(&raw).unwrap();
        (start, start + raw.len())
    };
    let directive_origin = |index, id: &str| {
        let (start, end) = directive_range(index);
        serde_json::json!({"kind":"UNIT_DIRECTIVE","unit_id":id,"unit_json_start":start,"unit_json_end":end})
    };
    let material_origin = |start, end| serde_json::json!({"kind":"MATERIAL_TEXT","material_id":"source","material_json_start":material_start,"material_json_end":material_end,"text_start":start,"text_end":end});
    let expected_output = "# 猫🙂\n\n条件: Keep";
    let spans = [
        (0, 2, "# ", directive_origin(2, "title")),
        (2, 9, "猫🙂", material_origin(0, 7)),
        (9, 11, "\n\n", directive_origin(0, "condition")),
        (11, 19, "条件: ", directive_origin(0, "condition")),
        (19, 23, "Keep", material_origin(7, 11)),
    ];
    let expected_spans: Vec<_> = spans
        .iter()
        .map(|(start, end, text, origin)| {
            assert_eq!(&expected_output[*start..*end], *text);
            serde_json::json!({"output_start":start,"output_end":end,"text":text,"origin":origin})
        })
        .collect();
    let units = [
        ("title", "heading", false, false, 2, 0, 9),
        ("condition", "condition", true, true, 0, 11, 23),
    ];
    let expected_units: Vec<_> = units.iter().map(|&(id,role,required,protected,index,start,end)| {
        let (directive_start,directive_end)=directive_range(index);
        serde_json::json!({"id":id,"role":role,"required":required,"protected":protected,"directive_json_start":directive_start,"directive_json_end":directive_end,"output_start":start,"output_end":end})
    }).collect();
    let request = format!("文章を構成して:\r\n{body}");
    let first = record(&execute(
        &scratch.0,
        "owner",
        &["--input", "compose-before-explanation", &request],
    ));
    assert_eq!(first["lifecycle"], "COMMIT");
    assert_eq!(first["guarantee"], "USER_PREMISE");
    assert_eq!(first["text"], expected_output);
    assert_eq!(first["content_plan"]["text"], first["text"]);
    assert_eq!(first["render"]["status"], "RENDERED");
    assert_eq!(
        first["render"]["actual_sha256"],
        minidora_domain::digest(expected_output.as_bytes())
    );
    assert_eq!(first["receipts"], serde_json::json!([]));
    let store = minidora_engine::store::Store::open(&scratch.0).unwrap();
    let report = sequence_artifact(&store, &first["dependencies"][0]);
    let report_ref = minidora_domain::digest(&minidora_domain::canonical(&report).unwrap());
    assert_eq!(first["dependencies"], serde_json::json!([report_ref]));
    assert_eq!(report["operation"], "TEXT_COMPOSE_EXPLICIT_FRAGMENTS");
    assert_eq!(report["source_document"], body);
    assert_eq!(
        report["source_sha256"],
        minidora_domain::digest(body.as_bytes())
    );
    assert_eq!(report["text"], expected_output);
    assert_eq!(
        report["output_sha256"],
        minidora_domain::digest(expected_output.as_bytes())
    );
    assert_eq!(
        report["selected_unit_ids"],
        serde_json::json!(["title", "condition"])
    );
    assert_eq!(report["omitted_unit_ids"], serde_json::json!(["optional"]));
    assert_eq!(report["units"], serde_json::json!(expected_units));
    assert_eq!(report["spans"], serde_json::json!(expected_spans));
    let saved = inspect_normal_case(&scratch.0, "owner");
    assert_eq!(saved["bundle"]["request"]["text"], request);
    assert_eq!(saved["bundle"]["meaning"]["intent"]["document"], body);
    let explained = assert_document_explanation(
        &scratch.0,
        &first,
        &report,
        "previous-text-composition",
        "USER_PREMISE",
        "依頼JSON",
    );
    let text = explained["text"].as_str().unwrap();
    for expected in [
        "前回の文章構成".to_string(),
        "採用単位数: 2。未採用単位数: 1。出力区間数: 5。".to_string(),
        "未採用単位: \"optional\"".to_string(),
        format!(
            "構成後本文 SHA-256: {}",
            minidora_domain::digest(expected_output.as_bytes())
        ),
    ] {
        assert!(text.contains(&expected), "missing {expected:?} in {text}");
    }
    let lines: Vec<_> = text.lines().collect();
    let mut previous_unit = None;
    for (index, &(id, role, required, protected, _, start, end)) in units.iter().enumerate() {
        let expected = format!(
            "採用単位{} {id:?} 役割={role} 必須={required} 保護={protected} 出力 byte[{start}..{end})",
            index + 1
        );
        let position = lines
            .iter()
            .position(|line| *line == expected)
            .unwrap_or_else(|| panic!("missing {expected:?} in {text}"));
        if let Some(previous) = previous_unit {
            assert!(previous < position);
        }
        previous_unit = Some(position);
    }
    assert_eq!(
        lines
            .iter()
            .filter(|line| line.starts_with("出力区間"))
            .count(),
        spans.len()
    );
    let mut previous_span = None;
    for (index, (start, end, piece, origin)) in spans.iter().enumerate() {
        let expected = format!(
            "出力区間{} byte[{start}..{end}) sha256={} 出力={piece:?}",
            index + 1,
            minidora_domain::digest(piece.as_bytes())
        );
        let position = lines
            .iter()
            .position(|line| *line == expected)
            .unwrap_or_else(|| panic!("missing {expected:?} in {text}"));
        if let Some(previous) = previous_span {
            assert!(previous < position);
        }
        previous_span = Some(position);
        let source_line = if origin["kind"] == "MATERIAL_TEXT" {
            let start = origin["text_start"].as_u64().unwrap() as usize;
            let end = origin["text_end"].as_u64().unwrap() as usize;
            assert_eq!(&material[start..end], *piece);
            format!(
                "素材 \"source\" 復号後 byte[{start}..{end}) 依頼JSON byte[{material_start}..{material_end}) sha256={}",
                minidora_domain::digest(raw_material.as_bytes())
            )
        } else {
            let id = origin["unit_id"].as_str().unwrap();
            let start = origin["unit_json_start"].as_u64().unwrap() as usize;
            let end = origin["unit_json_end"].as_u64().unwrap() as usize;
            format!(
                "書式 単位 {id:?} 依頼JSON byte[{start}..{end}) sha256={}",
                minidora_domain::digest(body[start..end].as_bytes())
            )
        };
        assert_eq!(lines[position + 1], source_line);
    }
    assert!(store.actions("owner", "normal-route").unwrap().is_empty());
}

#[test]
fn explicit_text_composition_limits_retained_artifact_before_delivery() {
    let scratch = Scratch::new();
    let body = serde_json::json!({
        "materials":[{"id":"short","text":"x"},{"id":"unused","text":"z".repeat(4096)}],
        "units":[{"id":"tiny","kind":"heading","fragments":[{"material":"short","start":0,"end":1}],"with":[],"required":false,"protected":false}],
        "order":["tiny"],"max_chars":16
    }).to_string();
    let request = format!("text-compose:\n{body}");
    let target = scratch.0.join("budget-limited-composition.txt");
    let result = stopped(
        &execute(
            &scratch.0,
            "owner",
            &[
                "--input",
                "composition-report-budget",
                "--max-evidence-bytes",
                "128",
                "--output",
                target.to_str().unwrap(),
                "--allow-write",
                target.to_str().unwrap(),
                &request,
            ],
        ),
        2,
        "SUSPEND",
    );
    assert!(
        result["reason"]
            .as_str()
            .unwrap()
            .starts_with("RESOURCE_LIMIT:")
    );
    assert_eq!(result["guarantee"], "UNKNOWN");
    assert_eq!(result["dependencies"], serde_json::json!([]));
    assert_eq!(result["receipts"], serde_json::json!([]));
    assert!(result["content_plan"].is_null());
    assert!(!target.exists());
    let store = minidora_engine::store::Store::open(&scratch.0).unwrap();
    assert!(store.actions("owner", "normal-route").unwrap().is_empty());
    let saved = inspect_normal_case(&scratch.0, "owner");
    assert_eq!(saved["bundle"]["request"]["text"], request);
    assert_eq!(saved["result"], result);
}

#[test]
fn extractive_summary_candidate_normal_route_keeps_source_and_unverified_coverage() {
    let source = "猫たちは 川沿いで 走る。猫たちは 川沿いで 休む。雨の日は 近づかない。";
    for header in ["文章を要約して:", "text-summary:"] {
        let scratch = Scratch::new();
        let max_chars = "猫たちは 川沿いで 走る。".chars().count() * 2;
        let body =
            serde_json::json!({"text":source,"max_sentences":2,"max_chars":max_chars}).to_string();
        let request = format!("{header}\n{body}");
        let result = record(&execute(&scratch.0, "owner", &[&request]));
        assert_eq!(result["lifecycle"], "COMMIT");
        assert_eq!(result["guarantee"], "HYPOTHESIS");
        assert!(result["text"].as_str().unwrap().starts_with(
            "抽出候補です。文の頻度と位置で選択しました。目的・意味・条件・否定の網羅は検証していません。"
        ));
        assert!(
            result["text"]
                .as_str()
                .unwrap()
                .contains("猫たちは 川沿いで 走る。")
        );
        assert!(
            result["text"]
                .as_str()
                .unwrap()
                .contains("猫たちは 川沿いで 休む。")
        );
        assert!(
            !result["text"]
                .as_str()
                .unwrap()
                .contains("雨の日は 近づかない。")
        );

        let saved = inspect_normal_case(&scratch.0, "owner");
        assert_eq!(
            saved["bundle"]["meaning"]["intent"]["kind"],
            "TEXT_SUMMARY_CANDIDATES"
        );
        assert_eq!(saved["bundle"]["meaning"]["intent"]["document"], body);
        let dependencies = result["dependencies"].as_array().unwrap();
        assert_eq!(dependencies.len(), 1);
        let store = minidora_engine::store::Store::open(&scratch.0).unwrap();
        let bytes = store
            .get_bytes("owner", "normal-route", dependencies[0].as_str().unwrap())
            .unwrap();
        let artifact: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(
            artifact["operation"],
            "TEXT_EXTRACTIVE_SUMMARY_CANDIDATES_V4"
        );
        assert_eq!(artifact["format"], "bullets");
        assert_eq!(artifact["source_text"], source);
        assert_eq!(
            artifact["source_text_sha256"],
            minidora_domain::digest(source.as_bytes())
        );
        assert_eq!(
            artifact["selected_sentence_indices"],
            serde_json::json!([0, 1])
        );
        assert_eq!(artifact["omitted_sentence_indices"], serde_json::json!([2]));
        assert_eq!(artifact["coverage_verified"], false);
        assert_eq!(artifact["max_chars"], max_chars);
        assert_eq!(artifact["selected_character_count"], max_chars);
        assert_eq!(
            artifact["length_limited_sentence_indices"],
            serde_json::json!([])
        );
        for sentence in artifact["sentences"].as_array().unwrap() {
            let start = sentence["source_start"].as_u64().unwrap() as usize;
            let end = sentence["source_end"].as_u64().unwrap() as usize;
            assert_eq!(&source[start..end], sentence["text"].as_str().unwrap());
        }
        assert_eq!(saved["result"], result);
    }

    let scratch = Scratch::new();
    let body = serde_json::json!({"text":source,"max_sentences":9}).to_string();
    let result = stopped(
        &execute(&scratch.0, "owner", &[&format!("text-summary:\n{body}")]),
        2,
        "SUSPEND",
    );
    assert!(
        result["reason"]
            .as_str()
            .unwrap()
            .contains("TEXT_SUMMARY_SCHEMA")
    );
    assert!(result["dependencies"].as_array().unwrap().is_empty());

    let scratch = Scratch::new();
    let body = serde_json::json!({"text":source,"max_sentences":2,"max_chars":1}).to_string();
    let result = stopped(
        &execute(&scratch.0, "owner", &[&format!("text-summary:\n{body}")]),
        2,
        "SUSPEND",
    );
    assert!(
        result["reason"]
            .as_str()
            .unwrap()
            .contains("TEXT_SUMMARY_CHAR_LIMIT")
    );
    assert!(result["dependencies"].as_array().unwrap().is_empty());
}

#[test]
fn text_summary_candidate_explanation_preserves_required_rank_and_escaped_source_evidence() {
    const LABEL: &str = "抽出候補です。文の頻度と位置で選択しました。目的・意味・条件・否定の網羅は検証していません。";
    let source = "猫 shared alpha. shared beta. unique gamma. shared delta.";
    let raw_text = r#""\u732b shared alpha. shared beta. unique gamma. shared delta.""#;
    let body = format!(
        r#"{{"text":{raw_text},"max_sentences":3,"max_chars":64,"required_sentence_indices":[2]}}"#
    );
    assert_eq!(serde_json::from_str::<String>(raw_text).unwrap(), source);
    assert_eq!(body.matches(raw_text).count(), 1);
    let json_start = body.find(raw_text).unwrap();
    let json_end = json_start + raw_text.len();
    let expected_output = format!("{LABEL}\n- 猫 shared alpha.\n- unique gamma.\n- shared delta.");
    // Token frequencies: shared = 3; alpha/beta/unique/gamma/delta = 1.
    // The first sentence receives a 6/5 multiplier; required index 2 is
    // selected first. Equal-score fill prefers index 3 over omitted index 1.
    let selected = [
        (0, 2, 0, 17, "猫 shared alpha.", 24, 10),
        (2, 1, 31, 44, "unique gamma.", 2, 2),
        (3, 3, 45, 58, "shared delta.", 4, 2),
    ];
    let mut cursor = LABEL.len();
    let expected_sentences: Vec<_> = selected
        .iter()
        .map(|&(index, rank, start, end, text, numerator, denominator)| {
            assert_eq!(&source[start..end], text);
            let output_start = cursor + "\n- ".len();
            let output_end = output_start + text.len();
            assert_eq!(&expected_output[output_start..output_end], text);
            cursor = output_end;
            serde_json::json!({
                "sentence_index": index,
                "selection_rank": rank,
                "source_start": start,
                "source_end": end,
                "output_start": output_start,
                "output_end": output_end,
                "text": text,
                "score_numerator": numerator,
                "score_denominator": denominator,
            })
        })
        .collect();
    assert_eq!(cursor, expected_output.len());
    for header in ["文章を要約して:", "text-summary:"] {
        let scratch = Scratch::new();
        let request = format!("{header}\r\n{body}");
        let first = record(&execute(
            &scratch.0,
            "owner",
            &["--input", "summary-before-explanation", &request],
        ));
        assert_eq!(first["lifecycle"], "COMMIT");
        assert_eq!(first["guarantee"], "HYPOTHESIS");
        assert_eq!(first["text"], expected_output);
        assert_eq!(first["content_plan"]["text"], first["text"]);
        assert_eq!(first["render"]["status"], "RENDERED");
        assert_eq!(
            first["render"]["actual_sha256"],
            minidora_domain::digest(expected_output.as_bytes())
        );
        assert_eq!(first["receipts"], serde_json::json!([]));
        let store = minidora_engine::store::Store::open(&scratch.0).unwrap();
        let report = sequence_artifact(&store, &first["dependencies"][0]);
        let report_ref = minidora_domain::digest(&minidora_domain::canonical(&report).unwrap());
        assert_eq!(first["dependencies"], serde_json::json!([report_ref]));
        assert_eq!(report["operation"], "TEXT_EXTRACTIVE_SUMMARY_CANDIDATES_V4");
        assert_eq!(
            report["algorithm"],
            "TOKEN_FREQUENCY_FIRST_SENTENCE_LONG_SENTENCE_PENALTY_REQUIRED_CONTENT_OUTPUT_FORMAT_V1"
        );
        assert_eq!(report["source_document"], body);
        assert_eq!(
            report["source_sha256"],
            minidora_domain::digest(body.as_bytes())
        );
        assert_eq!(report["source_text"], source);
        assert_eq!(
            report["source_text_sha256"],
            minidora_domain::digest(source.as_bytes())
        );
        assert_eq!(report["source_text_json_start"], json_start);
        assert_eq!(report["source_text_json_end"], json_end);
        assert_eq!(report["text"], expected_output);
        assert_eq!(
            report["output_sha256"],
            minidora_domain::digest(expected_output.as_bytes())
        );
        assert_eq!(report["format"], "bullets");
        assert_eq!(report["sentence_count"], 4);
        assert_eq!(report["required_sentence_indices"], serde_json::json!([2]));
        assert_eq!(
            report["selected_sentence_indices"],
            serde_json::json!([0, 2, 3])
        );
        assert_eq!(report["omitted_sentence_indices"], serde_json::json!([1]));
        assert_eq!(
            report["length_limited_sentence_indices"],
            serde_json::json!([])
        );
        assert_eq!(report["selected_character_count"], 41);
        assert_eq!(report["coverage_verified"], false);
        assert_eq!(report["sentences"], serde_json::json!(expected_sentences));
        let saved = inspect_normal_case(&scratch.0, "owner");
        assert_eq!(saved["bundle"]["request"]["text"], request);
        assert_eq!(saved["bundle"]["meaning"]["intent"]["document"], body);
        let explained = assert_document_explanation(
            &scratch.0,
            &first,
            &report,
            "previous-text-summary-candidate",
            "HYPOTHESIS",
            "依頼JSON",
        );
        let text = explained["text"].as_str().unwrap();
        for expected in [
            "前回の抽出要約候補を、封印済みの原文から再確認しました。".to_string(),
            "候補文数: 4。選択文数: 3。未選択文数: 1。文字数制限で見送った文数: 0。".to_string(),
            format!(
                "元本文 SHA-256: {}",
                minidora_domain::digest(source.as_bytes())
            ),
            format!(
                "候補出力 SHA-256: {}",
                minidora_domain::digest(expected_output.as_bytes())
            ),
            format!(
                "依頼JSONの本文文字列トークン byte[{json_start}..{json_end}) SHA-256: {}",
                minidora_domain::digest(raw_text.as_bytes())
            ),
            "選択と原文の一致を再確認しました。目的・意味・条件・否定の網羅と事実性は未検証です。"
                .to_string(),
        ] {
            assert!(
                text.lines().any(|line| line == expected),
                "missing {expected:?} in {text}"
            );
        }
        let lines: Vec<_> = text.lines().collect();
        assert_eq!(
            lines
                .iter()
                .filter(|line| line.starts_with("選択文 元本文第"))
                .count(),
            3
        );
        assert!(!text.contains("選択文 元本文第2文"), "{text}");
        let mut previous_heading = None;
        for (expected, &(index, rank, start, end, raw, numerator, denominator)) in
            expected_sentences.iter().zip(&selected)
        {
            let reason = if index == 2 {
                "必須指定"
            } else {
                "頻度・位置による選択"
            };
            let heading = format!(
                "選択文 元本文第{}文 / 選択順{rank} / {reason} / 評点 {numerator}/{denominator}",
                index + 1
            );
            let position = lines
                .iter()
                .position(|line| *line == heading)
                .unwrap_or_else(|| panic!("missing {heading:?} in {text}"));
            if let Some(previous) = previous_heading {
                assert!(previous < position);
            }
            previous_heading = Some(position);
            assert_eq!(
                lines[position + 1],
                format!(
                    "  元本文 byte[{start}..{end}) → 候補出力 byte[{}..{}) / SHA-256: {} / 原文: {raw:?}",
                    expected["output_start"].as_u64().unwrap(),
                    expected["output_end"].as_u64().unwrap(),
                    minidora_domain::digest(raw.as_bytes())
                )
            );
        }
        let replayed = record(&execute(
            &scratch.0,
            "owner",
            &["--input", "explain", "詳しく説明して"],
        ));
        assert_eq!(replayed, explained);
        let saved = inspect_normal_case(&scratch.0, "owner");
        assert_eq!(saved["bundle"]["request"]["input_id"], "explain");
        assert_eq!(
            saved["bundle"]["meaning"]["parser_version"],
            "declarative-ja-en-v112-search-candidate-body-explanation"
        );
        assert_eq!(
            saved["bundle"]["meaning"]["context_result_ref"],
            first["id"]
        );
        assert_eq!(saved["result"], explained);
        assert!(store.actions("owner", "normal-route").unwrap().is_empty());
    }
}

#[test]
fn extractive_summary_candidate_normal_route_renders_explicit_paragraph_format() {
    let scratch = Scratch::new();
    let source = "猫は走る。猫は休む。";
    let body = serde_json::json!({
        "text":source,
        "max_sentences":2,
        "format":"paragraph"
    })
    .to_string();
    assert_eq!(
        body,
        r#"{"format":"paragraph","max_sentences":2,"text":"猫は走る。猫は休む。"}"#
    );
    let request = format!("text-summary:\n{body}");
    let result = record(&execute(&scratch.0, "owner", &[&request]));
    assert_eq!(result["lifecycle"], "COMMIT");
    assert_eq!(result["guarantee"], "HYPOTHESIS");
    assert_eq!(
        result["text"],
        "抽出候補です。文の頻度と位置で選択しました。目的・意味・条件・否定の網羅は検証していません。\n猫は走る。 猫は休む。"
    );

    let dependencies = result["dependencies"].as_array().unwrap();
    assert_eq!(dependencies.len(), 1);
    let store = minidora_engine::store::Store::open(&scratch.0).unwrap();
    let bytes = store
        .get_bytes("owner", "normal-route", dependencies[0].as_str().unwrap())
        .unwrap();
    let artifact: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(
        artifact["operation"],
        "TEXT_EXTRACTIVE_SUMMARY_CANDIDATES_V4"
    );
    assert_eq!(artifact["format"], "paragraph");
    assert_eq!(
        artifact["selected_sentence_indices"],
        serde_json::json!([0, 1])
    );
    assert_eq!(artifact["coverage_verified"], false);
    assert_eq!(
        artifact["output_sha256"],
        minidora_domain::digest(artifact["text"].as_str().unwrap().as_bytes())
    );
    for sentence in artifact["sentences"].as_array().unwrap() {
        let source_start = sentence["source_start"].as_u64().unwrap() as usize;
        let source_end = sentence["source_end"].as_u64().unwrap() as usize;
        let output_start = sentence["output_start"].as_u64().unwrap() as usize;
        let output_end = sentence["output_end"].as_u64().unwrap() as usize;
        let text = sentence["text"].as_str().unwrap();
        assert_eq!(&source[source_start..source_end], text);
        assert_eq!(
            &artifact["text"].as_str().unwrap()[output_start..output_end],
            text
        );
    }

    for format in [serde_json::json!("table"), serde_json::json!(1)] {
        let scratch = Scratch::new();
        let body = serde_json::json!({
            "text":source,
            "max_sentences":2,
            "format":format
        })
        .to_string();
        let result = stopped(
            &execute(&scratch.0, "owner", &[&format!("text-summary:\n{body}")]),
            2,
            "SUSPEND",
        );
        assert!(
            result["reason"]
                .as_str()
                .unwrap()
                .contains("TEXT_SUMMARY_SCHEMA")
        );
        assert!(result["dependencies"].as_array().unwrap().is_empty());
    }
}

#[test]
fn extractive_summary_candidate_normal_route_preserves_required_sentence_indices() {
    let scratch = Scratch::new();
    let source = "shared alpha. shared beta. unique gamma. shared delta.";
    let body = serde_json::json!({
        "text":source,
        "max_sentences":2,
        "max_chars":26,
        "required_sentence_indices":[2]
    })
    .to_string();
    let request = format!("text-summary:\n{body}");
    let result = record(&execute(&scratch.0, "owner", &[&request]));
    assert_eq!(result["lifecycle"], "COMMIT");
    assert_eq!(result["guarantee"], "HYPOTHESIS");
    assert!(result["text"].as_str().unwrap().contains("shared alpha."));
    assert!(result["text"].as_str().unwrap().contains("unique gamma."));
    assert!(!result["text"].as_str().unwrap().contains("shared beta."));
    assert!(!result["text"].as_str().unwrap().contains("shared delta."));

    let dependencies = result["dependencies"].as_array().unwrap();
    assert_eq!(dependencies.len(), 1);
    let store = minidora_engine::store::Store::open(&scratch.0).unwrap();
    let bytes = store
        .get_bytes("owner", "normal-route", dependencies[0].as_str().unwrap())
        .unwrap();
    let artifact: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(
        artifact["operation"],
        "TEXT_EXTRACTIVE_SUMMARY_CANDIDATES_V4"
    );
    assert_eq!(
        artifact["required_sentence_indices"],
        serde_json::json!([2])
    );
    assert_eq!(
        artifact["selected_sentence_indices"],
        serde_json::json!([0, 2])
    );
    assert_eq!(
        artifact["omitted_sentence_indices"],
        serde_json::json!([1, 3])
    );
    assert_eq!(artifact["selected_character_count"], 26);
    for sentence in artifact["sentences"].as_array().unwrap() {
        let source_start = sentence["source_start"].as_u64().unwrap() as usize;
        let source_end = sentence["source_end"].as_u64().unwrap() as usize;
        let output_start = sentence["output_start"].as_u64().unwrap() as usize;
        let output_end = sentence["output_end"].as_u64().unwrap() as usize;
        let text = sentence["text"].as_str().unwrap();
        assert_eq!(&source[source_start..source_end], text);
        assert_eq!(
            &artifact["text"].as_str().unwrap()[output_start..output_end],
            text
        );
    }

    for (body, code) in [
        (
            serde_json::json!({
                "text":"Only one.",
                "max_sentences":1,
                "required_sentence_indices":[1]
            }),
            "TEXT_SUMMARY_REQUIRED_INDEX",
        ),
        (
            serde_json::json!({
                "text":"One. Two.",
                "max_sentences":1,
                "required_sentence_indices":[0, 1]
            }),
            "TEXT_SUMMARY_REQUIRED_LIMIT",
        ),
        (
            serde_json::json!({
                "text":"Required sentence.",
                "max_sentences":1,
                "max_chars":4,
                "required_sentence_indices":[0]
            }),
            "TEXT_SUMMARY_REQUIRED_CHAR_LIMIT",
        ),
    ] {
        let scratch = Scratch::new();
        let stopped_result = stopped(
            &execute(&scratch.0, "owner", &[&format!("text-summary:\n{}", body)]),
            2,
            "SUSPEND",
        );
        assert!(stopped_result["reason"].as_str().unwrap().contains(code));
        assert!(
            stopped_result["dependencies"]
                .as_array()
                .unwrap()
                .is_empty()
        );
    }
}

#[test]
fn extractive_summary_candidate_route_applies_character_budget_without_truncation() {
    let scratch = Scratch::new();
    let source = "shared concept first. shared concept second. unique third.";
    let body = serde_json::json!({"text":source,"max_sentences":2,"max_chars":35}).to_string();
    let request = format!("text-summary:\n{body}");
    let result = record(&execute(&scratch.0, "owner", &[&request]));
    assert_eq!(result["lifecycle"], "COMMIT");
    assert_eq!(result["guarantee"], "HYPOTHESIS");
    let output = result["text"].as_str().unwrap();
    assert!(output.contains("shared concept first."));
    assert!(output.contains("unique third."));
    assert!(!output.contains("shared concept second."));

    let saved = inspect_normal_case(&scratch.0, "owner");
    assert_eq!(
        saved["bundle"]["meaning"]["intent"]["kind"],
        "TEXT_SUMMARY_CANDIDATES"
    );
    let dependencies = result["dependencies"].as_array().unwrap();
    let store = minidora_engine::store::Store::open(&scratch.0).unwrap();
    let bytes = store
        .get_bytes("owner", "normal-route", dependencies[0].as_str().unwrap())
        .unwrap();
    let artifact: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(artifact["max_chars"], 35);
    assert_eq!(artifact["selected_character_count"], 34);
    assert_eq!(
        artifact["selected_sentence_indices"],
        serde_json::json!([0, 2])
    );
    assert_eq!(artifact["omitted_sentence_indices"], serde_json::json!([1]));
    assert_eq!(
        artifact["length_limited_sentence_indices"],
        serde_json::json!([1])
    );
    assert_eq!(saved["result"], result);
}

fn text_patch(id: &str, start: usize, end: usize, expected: &str, replacement: &str) -> Value {
    serde_json::json!({"id":id,"start":start,"end":end,"expected":expected,"replacement":replacement,"reason":"explicit requested revision"})
}

fn text_edit_request(header: &str, base_sha: &str, text: &str, edits: Value) -> String {
    format!(
        "{header}\n{}",
        serde_json::json!({"base_artifact":base_sha,"expected_text_sha256":minidora_domain::digest(text.as_bytes()),"edits":edits})
    )
}

fn text_locate_request(
    header: &str,
    base_sha: &str,
    text: &str,
    search: &str,
    replacement: &str,
) -> String {
    format!(
        "{header}\n{}",
        serde_json::json!({
            "base_artifact":base_sha,
            "expected_text_sha256":minidora_domain::digest(text.as_bytes()),
            "search":search,
            "replacement":replacement,
            "id":"located-change",
            "reason":"explicitly requested literal replacement"
        })
    )
}

fn assert_text_document_artifact(directory: &Path, result: &Value, request: &str) -> Value {
    assert_eq!(result["lifecycle"], "COMMIT");
    assert_eq!(result["guarantee"], "USER_PREMISE");
    assert_eq!(result["content_plan"]["text"], result["text"]);
    assert_eq!(result["render"]["status"], "RENDERED");
    let saved = inspect_normal_case(directory, "owner");
    assert_eq!(saved["result"], *result);
    assert_eq!(saved["bundle"]["request"]["text"], request);
    let body = saved["bundle"]["meaning"]["intent"]["document"]
        .as_str()
        .unwrap();
    let store = minidora_engine::store::Store::open(directory).unwrap();
    let dependencies = result["dependencies"].as_array().unwrap();
    let report = sequence_artifact(&store, dependencies.last().unwrap());
    assert_eq!(report["source_document"], body);
    assert_eq!(
        report["source_sha256"],
        minidora_domain::digest(body.as_bytes())
    );
    assert_eq!(report["text"], result["text"]);
    let text = result["text"].as_str().unwrap();
    assert_eq!(
        report["output_sha256"],
        minidora_domain::digest(text.as_bytes())
    );
    assert_eq!(result["render"]["actual_sha256"], report["output_sha256"]);
    let mut cursor = 0usize;
    for span in report["spans"].as_array().unwrap() {
        let start = span["output_start"].as_u64().unwrap() as usize;
        let end = span["output_end"].as_u64().unwrap() as usize;
        let piece = span["text"].as_str().unwrap();
        assert_eq!(start, cursor);
        assert_eq!(text.get(start..end), Some(piece));
        let origin = &span["origin"];
        if origin["kind"] == "PARENT_TEXT" {
            let parent = sequence_artifact(&store, &origin["artifact_sha256"]);
            let old_start = origin["start"].as_u64().unwrap() as usize;
            let old_end = origin["end"].as_u64().unwrap() as usize;
            assert_eq!(
                parent["text"].as_str().unwrap().get(old_start..old_end),
                Some(piece)
            );
            assert!(dependencies.contains(&origin["artifact_sha256"]));
        } else {
            assert!(matches!(
                origin["kind"].as_str().unwrap(),
                "ORIGINAL_TEXT" | "REPLACEMENT_TEXT"
            ));
            let json_start = origin["json_start"].as_u64().unwrap() as usize;
            let json_end = origin["json_end"].as_u64().unwrap() as usize;
            let literal: String = serde_json::from_str(&body[json_start..json_end]).unwrap();
            let text_start = origin["text_start"].as_u64().unwrap() as usize;
            let text_end = origin["text_end"].as_u64().unwrap() as usize;
            assert_eq!(literal.get(text_start..text_end), Some(piece));
        }
        cursor = end;
    }
    assert_eq!(cursor, text.len());
    for lock in report["protected"].as_array().unwrap() {
        let start = lock["start"].as_u64().unwrap() as usize;
        let end = lock["end"].as_u64().unwrap() as usize;
        assert_eq!(text.get(start..end), lock["expected"].as_str());
    }
    let graph = sequence_artifact(&store, &result["execution_graph_ref"]);
    let compute = sequence_artifact(&store, &graph[1]["compute"]);
    assert_eq!(compute[3], result["dependencies"]);
    report
}

#[test]
fn text_editing_route_imports_actual_text_versions_and_confirms_file_once() {
    let original = "draft\n条件: 雨天中止\nend";
    let lock_start = "draft\n".len();
    let lock_end = lock_start + "条件: 雨天中止".len();
    let body = serde_json::json!({"text":original,"protected":[{"id":"condition","start":lock_start,"end":lock_end,"expected":"条件: 雨天中止"}],"max_chars":128}).to_string();
    for (import_header, edit_header) in [
        ("文章を取り込んで:", "文章を編集して:"),
        ("TeXt-ImPoRt:", "TeXt-EdIt:"),
    ] {
        let scratch = Scratch::new();
        let import_request = format!("{import_header}\r\n{body}");
        let imported = record(&execute(&scratch.0, "owner", &[&import_request]));
        let base = assert_text_document_artifact(&scratch.0, &imported, &import_request);
        assert_eq!(base["operation"], "TEXT_IMPORT_EXPLICIT_V1");
        assert_eq!(base["text"], original);
        let base_sha = imported["dependencies"][0].as_str().unwrap();
        let request = text_edit_request(
            edit_header,
            base_sha,
            original,
            serde_json::json!([
                text_patch("tail", original.len() - 3, original.len(), "end", "finish"),
                text_patch("insert", lock_start, lock_start, "", "Note\n"),
                text_patch("title", 0, 5, "draft", "final")
            ]),
        );
        let path = scratch.0.join("revised.txt");
        let path_text = path.to_str().unwrap();
        let args = [
            "--input",
            "revision",
            "--output",
            path_text,
            "--allow-write",
            path_text,
            &request,
        ];
        let first = record(&execute(&scratch.0, "owner", &args));
        let report = assert_text_document_artifact(&scratch.0, &first, &request);
        assert_eq!(first["text"], "final\nNote\n条件: 雨天中止\nfinish");
        assert_eq!(report["revision"], 1);
        assert_eq!(report["consumed_artifacts"], serde_json::json!([base_sha]));
        assert_eq!(report["protected"][0]["start"], lock_start + "Note\n".len());
        assert_eq!(first["receipts"][0]["phase"], "CONFIRMED");
        assert_eq!(
            std::fs::read(&path).unwrap(),
            first["text"].as_str().unwrap().as_bytes()
        );
        let modified = std::fs::metadata(&path).unwrap().modified().unwrap();
        assert_eq!(record(&execute(&scratch.0, "owner", &args)), first);
        assert_eq!(
            std::fs::metadata(&path).unwrap().modified().unwrap(),
            modified
        );
        let first_sha = first["dependencies"]
            .as_array()
            .unwrap()
            .last()
            .unwrap()
            .as_str()
            .unwrap();
        let first_text = first["text"].as_str().unwrap();
        let finish = first_text.find("finish").unwrap();
        let next = text_edit_request(
            "text-edit:",
            first_sha,
            first_text,
            serde_json::json!([text_patch("next", finish, finish + 6, "finish", "done")]),
        );
        let second = record(&execute(
            &scratch.0,
            "owner",
            &["--input", "revision", "--version", "2", &next],
        ));
        let report = assert_text_document_artifact(&scratch.0, &second, &next);
        assert_eq!(second["text"], "final\nNote\n条件: 雨天中止\ndone");
        assert_eq!(report["revision"], 2);
        assert_eq!(
            report["consumed_artifacts"],
            serde_json::json!([first_sha, base_sha])
        );
        assert_eq!(
            inspect_normal_case(&scratch.0, "owner")["prior_results"]
                [first["id"].as_str().unwrap()],
            first
        );
        let store = minidora_engine::store::Store::open(&scratch.0).unwrap();
        assert_eq!(store.actions("owner", "normal-route").unwrap().len(), 1);
    }
}

#[test]
fn text_edit_explanation_preserves_sorted_changes_zero_width_ranges_and_protection() {
    let scratch = Scratch::new();
    let original = "aaa|引用:「保持」|末尾";
    let output = "longaaa|前引用:「保持」";
    let protected = "引用:「保持」";
    assert_eq!(&original[4..23], protected);
    assert_eq!(&output[11..30], protected);
    let import_request = format!(
        "text-import:\n{}",
        serde_json::json!({"text":original,"protected":[{"id":"quote","start":4,"end":23,"expected":protected}],"max_chars":64})
    );
    let imported = record(&execute(&scratch.0, "owner", &[&import_request]));
    let base = assert_text_document_artifact(&scratch.0, &imported, &import_request);
    let base_ref = imported["dependencies"][0].as_str().unwrap();
    let declarations = serde_json::json!([
        text_patch("remove-tail", 23, 30, "|末尾", ""),
        text_patch("insert-before-lock", 4, 4, "", "前"),
        text_patch("replace", 0, 3, "aaa", "longaaa")
    ]);
    let request = text_edit_request("文章を編集して:", base_ref, original, declarations.clone());
    let body = request.split_once('\n').unwrap().1;
    let first = record(&execute(
        &scratch.0,
        "owner",
        &["--input", "edit-before-explanation", &request],
    ));
    assert_eq!(first["text"], output);
    assert_eq!(first["receipts"], serde_json::json!([]));
    let report = assert_text_document_artifact(&scratch.0, &first, &request);
    let report_ref = minidora_domain::digest(&minidora_domain::canonical(&report).unwrap());
    assert_eq!(
        first["dependencies"],
        serde_json::json!([base_ref, report_ref])
    );
    assert_eq!(report["base_artifact"], base_ref);
    assert_eq!(report["consumed_artifacts"], serde_json::json!([base_ref]));
    assert_eq!(report["revision"], 1);
    assert_eq!(
        report["protected"],
        serde_json::json!([{"id":"quote","start":11,"end":30,"expected":protected}])
    );
    let changes = [
        ("replace", "置換", 0, 3, "aaa", 0, 7, "longaaa"),
        ("insert-before-lock", "挿入", 4, 4, "", 8, 11, "前"),
        ("remove-tail", "削除", 23, 30, "|末尾", 30, 30, ""),
    ];
    let expected_changes: Vec<_> = changes.iter().map(|&(id,_,old_start,old_end,expected,new_start,new_end,replacement)| {
        assert_eq!(&original[old_start..old_end], expected);
        assert_eq!(&output[new_start..new_end], replacement);
        serde_json::json!({"id":id,"old_start":old_start,"old_end":old_end,"new_start":new_start,"new_end":new_end,"expected":expected,"replacement":replacement,"reason":"explicit requested revision"})
    }).collect();
    assert_eq!(report["changes"], serde_json::json!(expected_changes));
    // Replacement JSON offsets follow the original declaration indices,
    // independently of the source-order change records and output spans.
    let replacement_span = |declaration_index: usize, start, end, text: &str| {
        let edit = &declarations[declaration_index];
        let fragment = serde_json::to_string(edit).unwrap();
        assert_eq!(body.matches(&fragment).count(), 1);
        let raw = serde_json::to_string(text).unwrap();
        let marker = format!("\"replacement\":{raw}");
        let json_start = body.find(&fragment).unwrap()
            + fragment.find(&marker).unwrap()
            + "\"replacement\":".len();
        let json_end = json_start + raw.len();
        assert_eq!(&body[json_start..json_end], raw);
        serde_json::json!({"output_start":start,"output_end":end,"text":text,
            "origin":{"kind":"REPLACEMENT_TEXT","edit_id":edit["id"],"json_start":json_start,"json_end":json_end,"text_start":0,"text_end":text.len()}})
    };
    assert_eq!(
        report["spans"],
        serde_json::json!([
            replacement_span(2,0,7,"longaaa"),
            {"output_start":7,"output_end":8,"text":"|","origin":{"kind":"PARENT_TEXT","artifact_sha256":base_ref,"start":3,"end":4}},
            replacement_span(1,8,11,"前"),
            {"output_start":11,"output_end":30,"text":protected,"origin":{"kind":"PARENT_TEXT","artifact_sha256":base_ref,"start":4,"end":23}}
        ])
    );
    let explained = assert_document_explanation(
        &scratch.0,
        &first,
        &report,
        "previous-text-edit",
        "USER_PREMISE",
        "依頼JSON",
    );
    let text = explained["text"].as_str().unwrap();
    assert!(text.contains("前回の文章編集"), "{text}");
    assert!(
        text.contains("編集版: 1。変更数: 3。保護範囲数: 1。"),
        "{text}"
    );
    for expected in [
        format!("編集前本文の成果物 SHA-256: {base_ref}"),
        format!(
            "編集前本文 SHA-256: {}",
            minidora_domain::digest(original.as_bytes())
        ),
        format!(
            "編集後本文 SHA-256: {}",
            minidora_domain::digest(output.as_bytes())
        ),
    ] {
        assert!(text.contains(&expected), "missing {expected:?} in {text}");
    }
    let lines: Vec<_> = text.lines().collect();
    assert_eq!(
        lines.iter().filter(|line| line.starts_with("変更")).count(),
        changes.len()
    );
    let mut previous_heading = None;
    for (index, &(id, kind, old_start, old_end, old_raw, new_start, new_end, new_raw)) in
        changes.iter().enumerate()
    {
        let heading = format!("変更{} {id:?} ({kind})", index + 1);
        let position = lines
            .iter()
            .position(|line| *line == heading)
            .unwrap_or_else(|| panic!("missing {heading:?} in {text}"));
        if let Some(previous) = previous_heading {
            assert!(previous < position);
        }
        previous_heading = Some(position);
        assert_eq!(
            lines[position + 1],
            format!(
                "編集前 byte[{old_start}..{old_end}) sha256={} 原文={old_raw:?}",
                minidora_domain::digest(old_raw.as_bytes())
            )
        );
        assert_eq!(
            lines[position + 2],
            format!(
                "編集後 byte[{new_start}..{new_end}) sha256={} 出力={new_raw:?}",
                minidora_domain::digest(new_raw.as_bytes())
            )
        );
    }
    let store = minidora_engine::store::Store::open(&scratch.0).unwrap();
    assert_eq!(
        sequence_artifact(&store, &Value::String(base_ref.into())),
        base
    );
    assert!(store.actions("owner", "normal-route").unwrap().is_empty());
}

#[test]
fn text_editing_route_consumes_composition_and_enforces_artifact_case_scope() {
    let scratch = Scratch::new();
    let composition = serde_json::json!({
        "materials":[{"id":"m","text":"TitleKeepQuote"}],
        "units":[
            {"id":"title","kind":"heading","fragments":[{"material":"m","start":0,"end":5}],"with":[],"required":false,"protected":false},
            {"id":"condition","kind":"condition","fragments":[{"material":"m","start":5,"end":9}],"with":[],"required":false,"protected":false},
            {"id":"quotation","kind":"quote","fragments":[{"material":"m","start":9,"end":14}],"with":[],"required":false,"protected":false}
        ],"order":["title","quotation"],"max_chars":128
    });
    let produced = record(&execute(
        &scratch.0,
        "owner",
        &[&format!("text-compose:\n{composition}")],
    ));
    assert_eq!(produced["text"], "# Title\n\n> Quote\n\n条件: Keep");
    let base_sha = produced["dependencies"][0].as_str().unwrap();
    let request = text_edit_request(
        "text-edit:",
        base_sha,
        produced["text"].as_str().unwrap(),
        serde_json::json!([text_patch("title", 2, 7, "Title", "Updated")]),
    );
    let edited = record(&execute(&scratch.0, "owner", &[&request]));
    let artifact = assert_text_document_artifact(&scratch.0, &edited, &request);
    assert_eq!(edited["text"], "# Updated\n\n> Quote\n\n条件: Keep");
    assert_eq!(
        artifact["consumed_artifacts"],
        serde_json::json!([base_sha])
    );
    assert_eq!(artifact["protected"].as_array().unwrap().len(), 2);
    let edited_sha = edited["dependencies"]
        .as_array()
        .unwrap()
        .last()
        .unwrap()
        .as_str()
        .unwrap();
    let text = edited["text"].as_str().unwrap();
    let at = text.find("Quote").unwrap();
    let locked = text_edit_request(
        "文章を編集して:",
        edited_sha,
        text,
        serde_json::json!([text_patch("quote", at, at + 5, "Quote", "Changed")]),
    );
    let result = stopped(&execute(&scratch.0, "owner", &[&locked]), 2, "SUSPEND");
    assert!(
        result["reason"]
            .as_str()
            .unwrap()
            .starts_with("TEXT_EDIT_PROTECTED:")
    );
    assert_eq!(result["dependencies"], serde_json::json!([]));
    let other = stopped(
        &execute(&scratch.0, "owner", &["--case", "other-case", &request]),
        2,
        "SUSPEND",
    );
    assert!(
        other["reason"]
            .as_str()
            .unwrap()
            .starts_with("TEXT_EDIT_BASE_UNAVAILABLE:")
    );
    assert_eq!(other["dependencies"], serde_json::json!([]));
    let store = minidora_engine::store::Store::open(&scratch.0).unwrap();
    assert!(
        store
            .get_bytes("stranger", "normal-route", base_sha)
            .is_err()
    );
    assert!(store.get_bytes("owner", "other-case", base_sha).is_err());
    assert_eq!(
        sequence_artifact(&store, &Value::String(edited_sha.into()))["text"],
        edited["text"]
    );
}

#[test]
fn text_editing_route_persists_typed_atomic_schema_and_resource_stops() {
    let scratch = Scratch::new();
    let text = "猫abcLOCKend";
    let imported = record(&execute(
        &scratch.0,
        "owner",
        &[&format!(
            "text-import:\n{}",
            serde_json::json!({"text":text,"protected":[{"id":"lock","start":6,"end":10,"expected":"LOCK"}],"max_chars":32})
        )],
    ));
    let sha = imported["dependencies"][0].as_str().unwrap();
    let cases = [
        (
            serde_json::json!([
                text_patch("valid", 3, 4, "a", "z"),
                text_patch("bad", 10, 13, "old", "x")
            ]),
            "TEXT_EDIT_EXPECTED_TEXT:",
        ),
        (
            serde_json::json!([
                text_patch("one", 3, 5, "ab", "x"),
                text_patch("two", 4, 6, "bc", "y")
            ]),
            "TEXT_EDIT_OVERLAP:",
        ),
        (
            serde_json::json!([text_patch("lock", 7, 7, "", "x")]),
            "TEXT_EDIT_PROTECTED:",
        ),
        (
            serde_json::json!([text_patch("utf8", 1, 3, "猫", "x")]),
            "TEXT_EDIT_EXPECTED_TEXT:",
        ),
        (
            serde_json::json!([text_patch("noop", 3, 4, "a", "a")]),
            "TEXT_EDIT_INPUT:",
        ),
        (
            serde_json::json!([text_patch("expand", 3, 4, "a", &"x".repeat(64))]),
            "TEXT_EDIT_OUTPUT_LIMIT:",
        ),
    ];
    for (edits, code) in cases {
        let request = text_edit_request("text-edit:", sha, text, edits);
        let stopped_result = stopped(&execute(&scratch.0, "owner", &[&request]), 2, "SUSPEND");
        assert!(stopped_result["reason"].as_str().unwrap().starts_with(code));
        assert_eq!(stopped_result["dependencies"], serde_json::json!([]));
        let saved = inspect_normal_case(&scratch.0, "owner");
        assert_eq!(saved["bundle"]["request"]["text"], request);
        assert_eq!(saved["result"], stopped_result);
    }
    let valid = text_edit_request(
        "text-edit:",
        sha,
        text,
        serde_json::json!([text_patch("change", 3, 4, "a", "z")]),
    );
    let (_, body) = valid.split_once('\n').unwrap();
    let mut stale: Value = serde_json::from_str(body).unwrap();
    stale["expected_text_sha256"] = serde_json::json!("0".repeat(64));
    let mut extra = stale.clone();
    extra["unexpected"] = serde_json::json!(true);
    let mut missing: Value = serde_json::from_str(body).unwrap();
    missing["base_artifact"] = serde_json::json!("0".repeat(64));
    for (body, code) in [
        (stale.to_string(), "TEXT_EDIT_BASE_MISMATCH:"),
        (extra.to_string(), "TEXT_EDIT_INPUT:"),
        (missing.to_string(), "TEXT_EDIT_BASE_UNAVAILABLE:"),
        (format!("{body} trailing"), "INVALID_JSON_DOCUMENT:"),
        (
            body.replacen("\"edits\":", "\"edits\":[],\"edits\":", 1),
            "DUPLICATE_JSON_KEY:",
        ),
    ] {
        let result = stopped(
            &execute(&scratch.0, "owner", &[&format!("text-edit:\n{body}")]),
            2,
            "SUSPEND",
        );
        assert!(
            result["reason"].as_str().unwrap().starts_with(code),
            "{}",
            result["reason"]
        );
        assert_eq!(result["dependencies"], serde_json::json!([]));
    }
    for args in [
        vec!["--max-evidence-bytes", "1"],
        vec!["--max-actions", "2"],
        vec!["--deadline-ms", "0"],
    ] {
        let mut args = args;
        args.push(&valid);
        let result = stopped(&execute(&scratch.0, "owner", &args), 2, "SUSPEND");
        assert_eq!(result["dependencies"], serde_json::json!([]));
    }
    let final_result = record(&execute(&scratch.0, "owner", &[&valid]));
    assert_eq!(final_result["text"], "猫zbcLOCKend");
    assert_text_document_artifact(&scratch.0, &final_result, &valid);
}

#[test]
fn text_location_route_proposes_one_unicode_patch_and_requires_explicit_apply() {
    let scratch = Scratch::new();
    let original = "猫 Draft を確認";
    let imported = record(&execute(
        &scratch.0,
        "owner",
        &[&format!(
            "text-import:\n{}",
            serde_json::json!({"text":original,"protected":[],"max_chars":64})
        )],
    ));
    let base_sha = imported["dependencies"][0].as_str().unwrap();
    let locate = text_locate_request(
        "文章の置換箇所を特定して:",
        base_sha,
        original,
        "Draft",
        "Final",
    );
    let located = record(&execute(
        &scratch.0,
        "owner",
        &["--input", "locate", &locate],
    ));
    assert_eq!(located["lifecycle"], "COMMIT");
    assert_eq!(located["guarantee"], "USER_PREMISE");
    assert_eq!(located["receipts"], serde_json::json!([]));
    let saved = inspect_normal_case(&scratch.0, "owner");
    assert_eq!(
        saved["bundle"]["meaning"]["intent"]["kind"],
        "LOCATE_TEXT_REPLACEMENT"
    );
    assert_eq!(
        saved["bundle"]["meaning"]["intent"]["document"],
        locate.split_once('\n').unwrap().1
    );
    let report: Value = serde_json::from_str(located["text"].as_str().unwrap()).unwrap();
    assert_eq!(report["operation"], "TEXT_EDIT_LOCATE_UNIQUE_LITERAL_V1");
    assert_eq!(report["base_artifact"], base_sha);
    assert_eq!(report["consumed_artifacts"], serde_json::json!([base_sha]));
    assert_eq!(report["location"]["artifact_sha256"], base_sha);
    assert_eq!(report["location"]["start"], "猫 ".len());
    assert_eq!(report["location"]["end"], "猫 Draft".len());
    assert_eq!(report["location"]["text"], "Draft");
    assert_eq!(report["proposed_edit"]["edits"][0]["replacement"], "Final");
    let body = saved["bundle"]["meaning"]["intent"]["document"]
        .as_str()
        .unwrap();
    let search_start = report["location"]["search_json_start"].as_u64().unwrap() as usize;
    let search_end = report["location"]["search_json_end"].as_u64().unwrap() as usize;
    assert_eq!(
        serde_json::from_str::<String>(&body[search_start..search_end]).unwrap(),
        "Draft"
    );
    let dependencies = located["dependencies"].as_array().unwrap();
    assert_eq!(dependencies.len(), 2);
    assert_eq!(dependencies[0], base_sha);
    let location_sha = dependencies[1].as_str().unwrap();
    let store = minidora_engine::store::Store::open(&scratch.0).unwrap();
    let report_bytes = store
        .get_bytes("owner", "normal-route", location_sha)
        .unwrap();
    assert_eq!(minidora_domain::digest(&report_bytes), location_sha);
    assert_eq!(report_bytes, located["text"].as_str().unwrap().as_bytes());
    let base: Value =
        serde_json::from_slice(&store.get_bytes("owner", "normal-route", base_sha).unwrap())
            .unwrap();
    assert_eq!(base["text"], original);
    assert!(store.actions("owner", "normal-route").unwrap().is_empty());

    let apply = format!("text-edit:\n{}", report["proposed_edit"]);
    let applied = record(&execute(&scratch.0, "owner", &["--input", "apply", &apply]));
    assert_eq!(applied["text"], "猫 Final を確認");
    assert_eq!(applied["lifecycle"], "COMMIT");
    let edited = assert_text_document_artifact(&scratch.0, &applied, &apply);
    assert_eq!(edited["changes"][0]["old_start"], "猫 ".len());
    assert_eq!(edited["changes"][0]["expected"], "Draft");
}

#[test]
fn text_location_explanation_preserves_escaped_request_tokens_before_explicit_apply() {
    let scratch = Scratch::new();
    let original = "前🙂 猫 Draft を確認";
    let search = "猫 Draft";
    let replacement = "犬 Final";
    let imported_request = format!(
        "text-import:\n{}",
        serde_json::json!({"text":original,"protected":[],"max_chars":64})
    );
    let imported = record(&execute(&scratch.0, "owner", &[&imported_request]));
    let base = assert_text_document_artifact(&scratch.0, &imported, &imported_request);
    assert_eq!(base["text"], original);
    let base_ref = imported["dependencies"][0].as_str().unwrap();
    let raw_search = r#""\u732b Draft""#;
    let raw_replacement = r#""\u72ac Final""#;
    let locate = text_locate_request(
        "文章の置換箇所を特定して:",
        base_ref,
        original,
        search,
        replacement,
    )
    .replacen(&serde_json::to_string(search).unwrap(), raw_search, 1)
    .replacen(
        &serde_json::to_string(replacement).unwrap(),
        raw_replacement,
        1,
    );
    let body = locate.split_once('\n').unwrap().1;
    assert_eq!(body.matches(raw_search).count(), 1);
    assert_eq!(body.matches(raw_replacement).count(), 1);
    let search_start = body.find(raw_search).unwrap();
    let replacement_start = body.find(raw_replacement).unwrap();
    let start = "前🙂 ".len();
    let end = start + search.len();
    assert_eq!(&original[start..end], search);
    let first = record(&execute(
        &scratch.0,
        "owner",
        &["--input", "locate-escaped", &locate],
    ));
    assert_eq!(first["lifecycle"], "COMMIT");
    assert_eq!(first["guarantee"], "USER_PREMISE");
    assert_eq!(first["receipts"], serde_json::json!([]));
    assert_eq!(first["content_plan"]["text"], first["text"]);
    assert_eq!(first["render"]["status"], "RENDERED");
    let report: Value = serde_json::from_str(first["text"].as_str().unwrap()).unwrap();
    let report_bytes = minidora_domain::canonical(&report).unwrap();
    let report_ref = minidora_domain::digest(&report_bytes);
    assert_eq!(first["text"].as_str().unwrap().as_bytes(), report_bytes);
    assert_eq!(first["render"]["actual_sha256"], report_ref);
    assert_eq!(
        first["dependencies"],
        serde_json::json!([base_ref, report_ref])
    );
    assert_eq!(report["operation"], "TEXT_EDIT_LOCATE_UNIQUE_LITERAL_V1");
    assert_eq!(report["source_document"], body);
    assert_eq!(
        report["source_sha256"],
        minidora_domain::digest(body.as_bytes())
    );
    assert_eq!(report["base_artifact"], base_ref);
    assert_eq!(
        report["base_output_sha256"],
        minidora_domain::digest(original.as_bytes())
    );
    assert_eq!(report["consumed_artifacts"], serde_json::json!([base_ref]));
    assert_eq!(
        report["location"],
        serde_json::json!({
            "artifact_sha256":base_ref,"start":start,"end":end,"text":search,
            "search_json_start":search_start,"search_json_end":search_start+raw_search.len(),
            "replacement_json_start":replacement_start,"replacement_json_end":replacement_start+raw_replacement.len()
        })
    );
    let proposal = &report["proposed_edit"];
    assert_eq!(proposal["base_artifact"], base_ref);
    assert_eq!(
        proposal["expected_text_sha256"],
        minidora_domain::digest(original.as_bytes())
    );
    assert_eq!(
        proposal["edits"],
        serde_json::json!([{
            "id":"located-change","start":start,"end":end,"expected":search,
            "replacement":replacement,"reason":"explicitly requested literal replacement"
        }])
    );
    let saved = inspect_normal_case(&scratch.0, "owner");
    assert_eq!(saved["bundle"]["request"]["text"], locate);
    assert_eq!(saved["bundle"]["meaning"]["intent"]["document"], body);
    let explained = assert_document_explanation(
        &scratch.0,
        &first,
        &report,
        "previous-text-location",
        "USER_PREMISE",
        "依頼JSON",
    );
    let text = explained["text"].as_str().unwrap();
    for expected in [
        "前回の置換箇所の特定".to_string(),
        format!("対象本文の成果物 SHA-256: {base_ref}"),
        format!(
            "対象本文 SHA-256: {}",
            minidora_domain::digest(original.as_bytes())
        ),
        format!("置換案: {search:?} → {replacement:?}"),
        "適用するには、別途".to_string(),
    ] {
        assert!(text.contains(&expected), "missing {expected:?} in {text}");
    }
    for (label, at, raw) in [
        ("対象本文", start, search),
        ("依頼JSONの検索語", search_start, raw_search),
        ("依頼JSONの置換語", replacement_start, raw_replacement),
    ] {
        let expected = format!(
            "{label} byte[{at}..{}) sha256={} 原文={raw:?}",
            at + raw.len(),
            minidora_domain::digest(raw.as_bytes())
        );
        assert!(
            text.lines().any(|line| line == expected),
            "missing {expected:?} in {text}"
        );
    }
    let store = minidora_engine::store::Store::open(&scratch.0).unwrap();
    assert_eq!(
        sequence_artifact(&store, &Value::String(base_ref.into())),
        base
    );
    assert!(store.actions("owner", "normal-route").unwrap().is_empty());
    let apply = format!("text-edit:\n{proposal}");
    let applied = record(&execute(
        &scratch.0,
        "owner",
        &["--input", "apply-after-explanation", &apply],
    ));
    assert_eq!(applied["text"], "前🙂 犬 Final を確認");
    let edited = assert_text_document_artifact(&scratch.0, &applied, &apply);
    assert_eq!(edited["changes"][0]["old_start"], start);
    assert_eq!(edited["changes"][0]["old_end"], end);
    assert_eq!(edited["changes"][0]["expected"], search);
    assert_eq!(edited["changes"][0]["replacement"], replacement);
    assert_eq!(applied["receipts"], serde_json::json!([]));
    assert!(store.actions("owner", "normal-route").unwrap().is_empty());
}

#[test]
fn text_location_route_suspends_for_missing_ambiguous_or_protected_literals() {
    for (case, text, protected, search, code) in [
        (
            "missing",
            "draft",
            serde_json::json!([]),
            "absent",
            "TEXT_EDIT_LOCATION_NOT_FOUND:",
        ),
        (
            "ambiguous",
            "draft draft",
            serde_json::json!([]),
            "draft",
            "TEXT_EDIT_LOCATION_AMBIGUOUS:",
        ),
        (
            "protected",
            "quote",
            serde_json::json!([{"id":"quote","start":0,"end":5,"expected":"quote"}]),
            "quote",
            "TEXT_EDIT_PROTECTED:",
        ),
    ] {
        let scratch = Scratch::new();
        let imported = record(&execute(
            &scratch.0,
            "owner",
            &[&format!(
                "text-import:\n{}",
                serde_json::json!({"text":text,"protected":protected,"max_chars":64})
            )],
        ));
        let base_sha = imported["dependencies"][0].as_str().unwrap();
        let request = text_locate_request(
            "TeXt-LoCaTe-RePlAcEmEnT:",
            base_sha,
            text,
            search,
            "replacement",
        );
        let result = stopped(
            &execute(&scratch.0, "owner", &["--input", case, &request]),
            2,
            "SUSPEND",
        );
        assert!(result["reason"].as_str().unwrap().starts_with(code));
        assert_eq!(result["dependencies"], serde_json::json!([]));
    }

    let scratch = Scratch::new();
    let text = "draft";
    let imported = record(&execute(
        &scratch.0,
        "owner",
        &[&format!(
            "text-import:\n{}",
            serde_json::json!({"text":text,"protected":[],"max_chars":64})
        )],
    ));
    let request = text_locate_request(
        "text-locate-replacement:",
        imported["dependencies"][0].as_str().unwrap(),
        text,
        "draft",
        "final",
    );
    let result = stopped(
        &execute(
            &scratch.0,
            "owner",
            &["--max-evidence-bytes", "1", "--input", "budget", &request],
        ),
        2,
        "SUSPEND",
    );
    assert!(
        result["reason"]
            .as_str()
            .unwrap()
            .starts_with("RESOURCE_LIMIT:")
    );
    assert_eq!(result["dependencies"], serde_json::json!([]));
}

fn code_expression_documents() -> Vec<Value> {
    let mut expressions = Vec::new();
    for (kind, operations) in [
        ("算術", vec!["加算", "減算", "乗算", "整数商", "剰余"]),
        ("比較", vec!["等値", "非等値", "未満", "以下", "超", "以上"]),
    ] {
        for operation in operations {
            expressions.push(code_binary(
                kind,
                operation,
                code_ref("x"),
                code_constant("除数"),
            ));
        }
    }
    expressions.push(serde_json::json!({"種別":"選択","条件":code_binary("比較","未満",code_ref("x"),code_constant("除数")),"真":code_constant("真値"),"偽":code_constant("偽値")}));
    expressions.push(
        serde_json::json!({"種別":"添字","対象":code_ref("xs"),"位置":code_constant("位置")}),
    );
    let arithmetic = code_document(
        "算術一覧",
        &["x", "xs"],
        vec![code_return(
            serde_json::json!({"種別":"リスト","要素":expressions}),
        )],
        serde_json::json!({"除数":3,"真値":"yes","偽値":"no","位置":-1}),
    );
    let functions = vec![
        code_call("合計", vec![code_ref("xs")]),
        code_call("長さ", vec![code_ref("xs")]),
        code_call(
            "リスト化",
            vec![code_call("範囲", vec![code_constant("回数")])],
        ),
        code_call("絶対値", vec![code_ref("x")]),
        code_call("最小", vec![code_ref("xs")]),
        code_call("最大", vec![code_ref("xs")]),
        code_call("整列", vec![code_ref("xs")]),
        code_call("リスト化", vec![code_ref("xs")]),
        code_call("すべて", vec![code_ref("xs")]),
        code_call("いずれか", vec![code_ref("xs")]),
    ];
    let builtins = code_document(
        "組込一覧",
        &["x", "xs"],
        vec![code_return(
            serde_json::json!({"種別":"リスト","要素":functions}),
        )],
        serde_json::json!({"回数":3}),
    );
    let key = "引用\"\\\n]; import os #";
    let inert = code_document(
        "資料",
        &[],
        vec![code_return(code_constant(key))],
        serde_json::json!({key:"__import__('os').system('not executed')"}),
    );
    let empty = code_document("空処理", &[], vec![], serde_json::json!({}));
    let empty_branches = code_document(
        "空分岐",
        &["xs"],
        vec![
            serde_json::json!({"種別":"分岐","条件":code_constant("条件"),"真":[],"偽":[{"種別":"反復","変数":"x","対象":code_ref("xs"),"手順":[]}]}),
        ],
        serde_json::json!({"条件":false}),
    );
    let unbound = code_document(
        "未束縛",
        &[],
        vec![code_return(code_ref("未定義"))],
        serde_json::json!({}),
    );
    vec![arithmetic, builtins, inert, empty, empty_branches, unbound]
}

#[test]
fn structured_code_generation_route_all_ir_forms_and_separate_constants() {
    for value in code_expression_documents() {
        let scratch = Scratch::new();
        let document = serde_json::to_string(&value).unwrap();
        let request = format!("code-generate:\n{document}");
        let result = record(&execute(&scratch.0, "owner", &[&request]));
        let report = assert_generated_code_artifact(&scratch.0, &result, &request, &document);
        assert!(
            !report["source"]
                .as_str()
                .unwrap()
                .contains("system('not executed')")
        );
    }
    let scratch = Scratch::new();
    let first = code_square_sum_document();
    let mut changed = first.clone();
    changed["定数"]["閾値"] = serde_json::json!(731);
    let mut sources = Vec::new();
    for (version, value) in [("1", first), ("2", changed)] {
        let document = serde_json::to_string(&value).unwrap();
        let request = format!("code-generate:\n{document}");
        let result = record(&execute(
            &scratch.0,
            "owner",
            &["--input", "constants", "--version", version, &request],
        ));
        let report = assert_generated_code_artifact(&scratch.0, &result, &request, &document);
        assert!(!report["source"].as_str().unwrap().contains("731"));
        sources.push(report);
    }
    assert_eq!(sources[0]["source"], sources[1]["source"]);
    assert_eq!(sources[0]["source_sha256"], sources[1]["source_sha256"]);
    assert_ne!(sources[0]["constants"], sources[1]["constants"]);
}

#[test]
fn structured_code_generation_route_strict_inputs_and_budget_stops() {
    let valid = code_document(
        "増分",
        &["x"],
        vec![code_return(code_binary(
            "算術",
            "加算",
            code_ref("x"),
            code_constant("幅"),
        ))],
        serde_json::json!({"幅":1}),
    );
    let mut variants = vec![serde_json::json!([]), serde_json::json!({"仕様":{}})];
    for (pointer, replacement) in [
        ("/余剰", serde_json::json!(true)),
        ("/仕様/追加", serde_json::json!(true)),
        ("/仕様/引数", serde_json::json!(["x", "x"])),
        ("/仕様/引数", serde_json::json!(["sum"])),
        ("/仕様/名前", serde_json::json!("x\nimport os")),
        ("/仕様/名前", serde_json::json!("def")),
        ("/仕様/名前", serde_json::json!("__x")),
        ("/仕様/名前", serde_json::json!("Ａ")),
        ("/仕様/名前", serde_json::json!("a\u{200c}")),
        ("/仕様/名前", serde_json::json!("a・b")),
        ("/仕様/引数", serde_json::json!(false)),
        ("/仕様/手順", serde_json::json!({})),
        ("/仕様/手順/0/追加", serde_json::json!(1)),
        ("/仕様/手順/0/種別", serde_json::json!("実行")),
        ("/仕様/手順/0/式/種別", serde_json::json!("eval")),
        ("/仕様/手順/0/式/演算", serde_json::json!("累乗")),
        ("/仕様/手順/0/式/右/キー", serde_json::json!("不足")),
        ("/仕様/手順/0/式/左/名前", serde_json::json!("定数")),
        ("/定数", serde_json::json!([])),
        ("/定数/幅", serde_json::json!(1.0)),
        ("/定数/幅", serde_json::json!("x".repeat(8193))),
        ("/定数/幅", serde_json::json!(vec![0; 2049])),
    ] {
        let mut variant = valid.clone();
        if pointer == "/余剰" {
            variant["余剰"] = replacement;
        } else if pointer == "/仕様/追加" {
            variant["仕様"]["追加"] = replacement;
        } else if pointer == "/仕様/手順/0/追加" {
            variant["仕様"]["手順"][0]["追加"] = replacement;
        } else {
            *variant.pointer_mut(pointer).unwrap() = replacement;
        }
        variants.push(variant);
    }
    // Build mutations without deriving any expected source from the generator.
    let mut depth = code_ref("x");
    for _ in 0..25 {
        depth = code_binary("算術", "加算", depth, code_ref("x"));
    }
    variants.push(code_document(
        "深い式",
        &["x"],
        vec![code_return(depth)],
        serde_json::json!({}),
    ));
    variants.push(code_document(
        "多手順",
        &[],
        vec![code_return(code_constant("幅")); 129],
        serde_json::json!({"幅":1}),
    ));
    let mut dead = valid.clone();
    dead["仕様"]["手順"] = serde_json::json!([{"種別":"分岐","条件":code_constant("幅"),"真":[],"偽":[{"種別":"import"}]}]);
    variants.push(dead);
    for value in variants {
        let scratch = Scratch::new();
        let document = serde_json::to_string(&value).unwrap();
        let request = format!("code-generate:\n{document}");
        let result = stopped(&execute(&scratch.0, "owner", &[&request]), 2, "SUSPEND");
        assert!(
            result["reason"]
                .as_str()
                .unwrap()
                .starts_with("CODE_GENERATION_INPUT:"),
            "{}",
            result["reason"]
        );
        assert_eq!(result["guarantee"], "UNKNOWN");
        assert_eq!(result["dependencies"], serde_json::json!([]));
        let saved = inspect_normal_case(&scratch.0, "owner");
        assert_eq!(saved["bundle"]["request"]["text"], request);
        assert_eq!(saved["result"], result);
    }
    let document = serde_json::to_string(&valid).unwrap();
    for (body, code) in [
        (format!("{document} trailing"), "INVALID_JSON_DOCUMENT:"),
        (
            document.replacen("\"幅\":1", "\"幅\":1,\"\\u5e45\":2", 1),
            "DUPLICATE_JSON_KEY:",
        ),
    ] {
        let scratch = Scratch::new();
        let request = format!("code-generate:\n{body}");
        let result = stopped(&execute(&scratch.0, "owner", &[&request]), 2, "SUSPEND");
        assert!(result["reason"].as_str().unwrap().starts_with(code));
        assert_eq!(result["dependencies"], serde_json::json!([]));
        assert_eq!(inspect_normal_case(&scratch.0, "owner")["result"], result);
    }
    for (args, code) in [
        (vec!["--max-actions", "2"], "ACTION_BUDGET_EXHAUSTED"),
        (vec!["--deadline-ms", "0"], "RESOURCE_LIMIT"),
        (vec!["--max-evidence-bytes", "1"], "RESOURCE_LIMIT"),
    ] {
        let scratch = Scratch::new();
        let request = format!("code-generate:\n{document}");
        let mut args = args;
        args.push(&request);
        let result = stopped(&execute(&scratch.0, "owner", &args), 2, "SUSPEND");
        assert!(result["reason"].as_str().unwrap().starts_with(code));
        assert_eq!(result["dependencies"], serde_json::json!([]));
        assert_eq!(inspect_normal_case(&scratch.0, "owner")["result"], result);
    }
}

#[test]
fn structured_code_generation_route_file_retry_and_source_revision() {
    let scratch = Scratch::new();
    let path = scratch.0.join("generated.py");
    let path_text = path.to_str().unwrap();
    let mut value = code_document(
        "増分",
        &["x"],
        vec![code_return(code_binary(
            "算術",
            "加算",
            code_ref("x"),
            code_constant("幅"),
        ))],
        serde_json::json!({"幅":1}),
    );
    let document = serde_json::to_string(&value).unwrap();
    let request = format!("code-generate:\n{document}");
    let args = [
        "--input",
        "source-history",
        "--max-actions",
        "4",
        "--output",
        path_text,
        "--allow-write",
        path_text,
        &request,
    ];
    let first = record(&execute(&scratch.0, "owner", &args));
    let report = assert_generated_code_artifact(&scratch.0, &first, &request, &document);
    let bytes = std::fs::read(&path).unwrap();
    assert_eq!(bytes, report["source"].as_str().unwrap().as_bytes());
    assert_eq!(first["receipts"][0]["phase"], "CONFIRMED");
    assert_eq!(first["receipts"][0]["target"], path_text);
    assert_eq!(
        first["receipts"][0]["content_sha256"],
        report["source_sha256"]
    );
    assert_eq!(first["receipts"][0]["observed_bytes"], bytes.len());
    let modified = std::fs::metadata(&path).unwrap().modified().unwrap();
    assert_eq!(record(&execute(&scratch.0, "owner", &args)), first);
    assert_eq!(
        std::fs::metadata(&path).unwrap().modified().unwrap(),
        modified
    );
    value["仕様"]["手順"][0]["式"]["演算"] = serde_json::json!("減算");
    let changed_document = serde_json::to_string(&value).unwrap();
    let changed_request = format!("code-generate:\n{changed_document}");
    let second = record(&execute(
        &scratch.0,
        "owner",
        &[
            "--input",
            "source-history",
            "--version",
            "2",
            &changed_request,
        ],
    ));
    let second_report =
        assert_generated_code_artifact(&scratch.0, &second, &changed_request, &changed_document);
    assert_ne!(report["source_sha256"], second_report["source_sha256"]);
    assert_eq!(
        inspect_normal_case(&scratch.0, "owner")["prior_results"][first["id"].as_str().unwrap()],
        first
    );
    assert_eq!(record(&execute(&scratch.0, "owner", &args)), first);
    assert_eq!(inspect_normal_case(&scratch.0, "owner")["result"], second);
    assert_eq!(std::fs::read(&path).unwrap(), bytes);
    assert_eq!(
        std::fs::metadata(&path).unwrap().modified().unwrap(),
        modified
    );
    assert_eq!(
        minidora_engine::store::Store::open(&scratch.0)
            .unwrap()
            .actions("owner", "normal-route")
            .unwrap()
            .len(),
        1
    );
}

fn assert_code_capability_artifact(
    directory: &Path,
    result: &Value,
    request: &str,
    document: &str,
    source: &str,
) -> Value {
    basic_raw_result(directory, result, request, Some(document));
    assert_eq!(result["dependencies"].as_array().unwrap().len(), 1);
    let store = minidora_engine::store::Store::open(directory).unwrap();
    let sha = result["dependencies"][0].as_str().unwrap();
    let bytes = store.get_bytes("owner", "normal-route", sha).unwrap();
    assert_eq!(minidora_domain::digest(&bytes), sha);
    let report: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(
        report["version"],
        "bounded-python-function-reader-interpreter-v1"
    );
    assert_eq!(report["source"], source);
    assert_eq!(
        report["source_sha256"],
        minidora_domain::digest(source.as_bytes())
    );
    assert_eq!(
        report["assurance"],
        "限定Python AST読解・ネイティブ評価。ホストPython、exec、eval、コンパイラ、外部作用は使用していない"
    );
    let saved = inspect_normal_case(directory, "owner");
    let kind = match report["operation"].as_str().unwrap() {
        "READ" => "READ_CODE",
        "EVALUATE" => "EVALUATE_CODE",
        "VERIFY" => "VERIFY_CODE",
        other => panic!("unknown code capability operation {other}"),
    };
    assert_eq!(saved["bundle"]["meaning"]["intent"]["kind"], kind);
    assert_eq!(saved["result"], *result);
    let read = |sha: &Value| -> Value {
        let sha = sha.as_str().unwrap();
        let bytes = store.get_bytes("owner", "normal-route", sha).unwrap();
        assert_eq!(minidora_domain::digest(&bytes), sha);
        serde_json::from_slice(&bytes).unwrap()
    };
    let graph = read(&result["execution_graph_ref"]);
    let observed = &graph[1];
    assert_eq!(
        read(&observed["compute"]),
        serde_json::json!([null, [result["text"]], [], result["dependencies"]])
    );
    assert_eq!(read(&observed["content"]), result["content_plan"]);
    assert_eq!(read(&observed["render"]), result["render"]);
    assert!(store.get_bytes("stranger", "normal-route", sha).is_err());
    report
}

#[test]
fn bounded_code_reader_uses_normal_cli_and_reports_utf8_structure() {
    let source = "# bounded example\ndef 合計(xs):\n    \"合計を返す\"\n    total = 0\n    for x in xs:\n        if x > 0:\n            total += x\n    return total\n";
    let document = serde_json::json!({"情報源":source}).to_string();
    for (header, newline) in [("コードを読んで：", "\n"), ("CoDe-ReAd:", "\r\n")] {
        let scratch = Scratch::new();
        let request = format!("{header}{newline}{document}");
        let result = record(&execute(&scratch.0, "owner", &[&request]));
        let report =
            assert_code_capability_artifact(&scratch.0, &result, &request, &document, source);
        assert_eq!(report["operation"], "READ");
        assert_eq!(report["function_name"], "合計");
        assert_eq!(report["parameters"], serde_json::json!(["xs"]));
        assert!(
            report["ast_nodes"].as_u64().unwrap()
                > report["structure"].as_array().unwrap().len() as u64
        );
        let structure = report["structure"].as_array().unwrap();
        let branch = structure
            .iter()
            .find(|node| node["syntax"] == "If")
            .unwrap();
        assert_eq!(branch["line"], 6);
        assert_eq!(branch["utf8_column"], 8);
        assert!(
            branch["source_segment"]
                .as_str()
                .unwrap()
                .contains("if x > 0")
        );
        assert!(
            report["summary"]
                .as_str()
                .unwrap()
                .contains("型・値・仕様適合は未検証")
        );
    }
}

#[test]
fn bounded_code_evaluator_uses_native_python_subset_semantics() {
    let source = "def analyze(xs):\n    total = 0\n    for x in xs:\n        if x < 0:\n            total += x\n        else:\n            total += x // -2\n    return [total, -7 // 3, -7 % 3, True + True, sum(xs), len(xs), sorted(xs)]\n";
    let document = serde_json::json!({"情報源":source,"引数":{"xs":[1,-3,4]}}).to_string();
    let scratch = Scratch::new();
    let request = format!("code-evaluate:\n{document}");
    let result = record(&execute(&scratch.0, "owner", &[&request]));
    let report = assert_code_capability_artifact(&scratch.0, &result, &request, &document, source);
    assert_eq!(report["operation"], "EVALUATE");
    assert_eq!(report["evaluation"]["status"], "COMPUTED");
    assert_eq!(
        report["evaluation"]["actual_json"],
        "[-6,-3,2,2,2,3,[-3,1,4]]"
    );
    assert!(report["evaluation"]["steps"].as_u64().unwrap() > 10);

    let ordered_source = "def keys(d):\n    return list(d)\n";
    let ordered_document = format!(
        "{{\"情報源\":{},\"引数\":{{\"d\":{{\"z\":1,\"a\":2}}}}}}",
        serde_json::to_string(ordered_source).unwrap()
    );
    let ordered_request = format!("code-evaluate:\n{ordered_document}");
    let ordered = record(&execute(
        &scratch.0,
        "owner",
        &["--input", "dict-order", &ordered_request],
    ));
    let ordered_report = assert_code_capability_artifact(
        &scratch.0,
        &ordered,
        &ordered_request,
        &ordered_document,
        ordered_source,
    );
    assert_eq!(ordered_report["evaluation"]["actual_json"], "[\"z\",\"a\"]");
}

#[test]
fn bounded_code_verifier_checks_only_declared_cases_and_preserves_typed_results() {
    let source = "def odd(x):\n    return x % 2 != 0\n";
    let document = serde_json::json!({"情報源":source,"試験":[
        {"引数":{"x":3},"期待値":true}, {"引数":{"x":2},"期待値":false}
    ]})
    .to_string();
    let scratch = Scratch::new();
    let request = format!("コードを検証して:\n{document}");
    let result = record(&execute(&scratch.0, "owner", &[&request]));
    let report = assert_code_capability_artifact(&scratch.0, &result, &request, &document, source);
    assert_eq!(report["operation"], "VERIFY");
    assert_eq!(report["verification"]["case_count"], 2);
    assert_eq!(report["verification"]["all_matched"], true);
    assert_eq!(report["verification"]["cases"][0]["actual_json"], "true");
    assert!(
        report["verification"]["assurance"]
            .as_str()
            .unwrap()
            .contains("全入力")
    );

    let mismatch = serde_json::json!({"情報源":"def answer():\n    return True\n","試験":[
        {"引数":{},"期待値":1}
    ]})
    .to_string();
    let mismatch_request = format!("CoDe-VeRiFy:\n{mismatch}");
    let mismatch_result = record(&execute(
        &scratch.0,
        "owner",
        &["--input", "typed-mismatch", &mismatch_request],
    ));
    let mismatch_source = "def answer():\n    return True\n";
    let mismatch_report = assert_code_capability_artifact(
        &scratch.0,
        &mismatch_result,
        &mismatch_request,
        &mismatch,
        mismatch_source,
    );
    assert_eq!(mismatch_report["verification"]["all_matched"], false);
    assert_eq!(
        mismatch_report["verification"]["cases"][0]["actual_json"],
        "true"
    );
    assert_eq!(
        mismatch_report["verification"]["cases"][0]["expected_json"],
        "1"
    );
}

#[test]
fn bounded_code_route_rejects_unsafe_syntax_and_reports_finite_stops() {
    for source in [
        "def f(x):\n    if False:\n        import os\n    return x\n",
        "def f(x):\n    return x.__class__\n",
        "def f(xs):\n    for x in xs:\n        pass\n    else:\n        break\n    return 0\n",
        "@decorator\ndef f():\n    return 1\n",
    ] {
        let document = serde_json::json!({"情報源":source}).to_string();
        let request = format!("code-read:\n{document}");
        let output = execute(&Scratch::new().0, "owner", &[&request]);
        let result = stopped(&output, 2, "SUSPEND");
        assert!(
            result["reason"]
                .as_str()
                .unwrap()
                .starts_with("CODE_CAPABILITY_INPUT:")
        );
        assert_eq!(result["dependencies"], serde_json::json!([]));
    }
    let duplicate = "{\"情報源\":\"def f():\\n    return 1\\n\",\"引数\":{},\"\\u5f15\\u6570\":{}}";
    let request = format!("code-evaluate:\n{duplicate}");
    let result = stopped(
        &execute(&Scratch::new().0, "owner", &[&request]),
        2,
        "SUSPEND",
    );
    assert!(
        result["reason"]
            .as_str()
            .unwrap()
            .starts_with("DUPLICATE_JSON_KEY:")
    );

    let source =
        "def loop(xs):\n    total = 0\n    for x in xs:\n        total += x\n    return total\n";
    let document =
        serde_json::json!({"情報源":source,"引数":{"xs":[1,2,3]},"最大手数":2}).to_string();
    let step_request = format!("code-evaluate:\n{document}");
    let step_scratch = Scratch::new();
    let step_result = record(&execute(&step_scratch.0, "owner", &[&step_request]));
    let step_report = assert_code_capability_artifact(
        &step_scratch.0,
        &step_result,
        &step_request,
        &document,
        source,
    );
    assert_eq!(step_report["evaluation"]["status"], "STOPPED");
    assert_eq!(
        step_report["evaluation"]["diagnostic"],
        "STEP_LIMIT: evaluation step budget exceeded"
    );

    for (args, expected) in [
        (vec!["--max-actions", "2"], "ACTION_BUDGET_EXHAUSTED"),
        (vec!["--deadline-ms", "0"], "RESOURCE_LIMIT"),
        (vec!["--max-evidence-bytes", "1"], "RESOURCE_LIMIT"),
    ] {
        let body = serde_json::json!({"情報源":"def f():\n    return 1\n"}).to_string();
        let request = format!("code-read:\n{body}");
        let mut argv = args;
        argv.push(&request);
        let result = stopped(&execute(&Scratch::new().0, "owner", &argv), 2, "SUSPEND");
        assert!(result["reason"].as_str().unwrap().starts_with(expected));
        assert_eq!(result["dependencies"], serde_json::json!([]));
    }
}

#[test]
fn bounded_code_reader_retries_saved_results_and_versions_new_source() {
    let scratch = Scratch::new();
    let first_source = "def f(x):\n    return x + 1\n";
    let first_document = serde_json::json!({"情報源":first_source}).to_string();
    let first_request = format!("code-read:\n{first_document}");
    let args = [
        "--input",
        "reader-history",
        "--version",
        "1",
        &first_request,
    ];
    let first = record(&execute(&scratch.0, "owner", &args));
    let first_report = assert_code_capability_artifact(
        &scratch.0,
        &first,
        &first_request,
        &first_document,
        first_source,
    );
    assert_eq!(record(&execute(&scratch.0, "owner", &args)), first);

    let second_source = "def f(x):\n    return x * 2\n";
    let second_document = serde_json::json!({"情報源":second_source}).to_string();
    let second_request = format!("code-read:\n{second_document}");
    let second = record(&execute(
        &scratch.0,
        "owner",
        &[
            "--input",
            "reader-history",
            "--version",
            "2",
            &second_request,
        ],
    ));
    let report = assert_code_capability_artifact(
        &scratch.0,
        &second,
        &second_request,
        &second_document,
        second_source,
    );
    assert_ne!(report["source_sha256"], first_report["source_sha256"]);
    assert_eq!(record(&execute(&scratch.0, "owner", &args)), first);
    assert_eq!(
        inspect_normal_case(&scratch.0, "owner")["prior_results"][first["id"].as_str().unwrap()],
        first
    );
}
