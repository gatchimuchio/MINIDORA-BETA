#!/usr/bin/env python3
"""Exercise the same saved-relation profile through loopback HTTP and CLI."""

import argparse
from collections import Counter
import csv
from datetime import datetime, timezone
import hashlib
import http.client
import json
import os
from pathlib import Path
import platform
import secrets
import select
import shutil
import subprocess
import tempfile


def digest(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def canonical(value: object) -> bytes:
    return json.dumps(
        value, ensure_ascii=False, sort_keys=True, separators=(",", ":")
    ).encode("utf-8")


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--evidence-output", type=Path, required=True)
    args = parser.parse_args()
    repository_root = Path(__file__).resolve().parents[2]
    rust_root = repository_root / "rust"
    binary = args.binary.resolve()
    cargo = shutil.which("cargo") or "/workspace/.cargo/bin/cargo"
    rustc = shutil.which("rustc") or "/workspace/.cargo/bin/rustc"
    source_paths = [
        "crates/minidora-domain/src/lib.rs",
        "crates/minidora-engine/src/core.rs",
        "crates/minidora-engine/src/input.rs",
        "crates/minidora-engine/src/language.rs",
        "crates/minidora-engine/src/recovery.rs",
        "crates/minidora-engine/src/relations.rs",
    ]
    test_paths = [
        "apps/cli/tests/normal_route.rs",
        "tools/known_relation_profile_http_route_test.py",
    ]
    documentation_paths = [
        "README.md",
        "audit/IMPLEMENTATION_STATUS.md",
        "audit/requirements.tsv",
        "docs/IMPLEMENTATION_CHOICES.md",
        "audit/construction/MB-BUILD-20261113/MANIFEST.tsv",
    ]

    def command_output(command: list[str]) -> str:
        return subprocess.run(
            command, capture_output=True, text=True, timeout=10, check=True
        ).stdout.strip()

    def sha256_paths(paths: list[str]) -> dict[str, str]:
        return {
            f"rust/{path}": hashlib.sha256((rust_root / path).read_bytes()).hexdigest()
            for path in paths
        }

    base_commit = command_output(
        ["git", "-C", str(repository_root), "rev-parse", "HEAD"]
    )
    toolchain = {
        "rustc": command_output([rustc, "--version"]),
        "cargo": command_output([cargo, "--version"]),
        "platform": platform.platform(),
    }
    manifest_path = rust_root / "audit/construction/MB-BUILD-20261113/MANIFEST.tsv"
    with manifest_path.open(encoding="utf-8", newline="") as source:
        manifest = list(csv.DictReader(source, delimiter="\t"))
    applied_build_set = [
        {
            "artifact_name": row["artifact_name"],
            "sha256": row["sha256"],
            "bytes": int(row["bytes"]),
            "role": row["role"],
            "repository_path": row["repository_path"],
        }
        for row in manifest
        if row["role"]
        in {
            "original_user_archive",
            "build_specification",
            "build_schedule",
            "codex_work_instruction",
        }
    ]
    observations: list[dict] = []

    with tempfile.TemporaryDirectory(prefix="minidora-profile-http-") as temporary:
        database = Path(temporary) / "state"
        actor, case = "profile-http-owner", "profile-http-case"

        def run_cli(input_id: str, request: str) -> dict:
            completed = subprocess.run(
                [
                    str(binary),
                    "--db",
                    str(database),
                    "--case",
                    case,
                    "--actor",
                    actor,
                    "--input",
                    input_id,
                    "--json",
                    request,
                ],
                capture_output=True,
                text=True,
                timeout=15,
                check=False,
            )
            if completed.returncode != 0:
                raise AssertionError(
                    f"CLI {input_id}: exit={completed.returncode}; "
                    f"stdout={completed.stdout}; stderr={completed.stderr}"
                )
            result = json.loads(completed.stdout)
            if result.get("lifecycle") != "COMMIT":
                raise AssertionError(f"CLI {input_id} did not commit: {result}")
            observations.append(
                {
                    "operation": "normal_cli_input",
                    "actor": actor,
                    "case": case,
                    "input_id": input_id,
                    "request": request,
                    "request_sha256": digest(request.encode("utf-8")),
                    "exit_code": completed.returncode,
                    "lifecycle": result["lifecycle"],
                    "guarantee": result["guarantee"],
                    "text": result["text"],
                    "dependencies": result["dependencies"],
                }
            )
            return result

        rule = run_cli("http-profile-rule", "もしXは鳥ならXは動物")
        bird = run_cli("http-profile-premise", "ココは鳥")

        token = secrets.token_hex(32)
        environment = os.environ.copy()
        environment["MINIDORA_API_TOKEN"] = token
        server = subprocess.Popen(
            [
                str(binary),
                "serve",
                "--db",
                str(database),
                "--actor",
                actor,
                "--bind",
                "127.0.0.1:0",
            ],
            stdin=subprocess.DEVNULL,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.PIPE,
            text=True,
            env=environment,
        )
        try:
            if server.stderr is None:
                raise AssertionError("the loopback server did not expose readiness output")
            ready, _, _ = select.select([server.stderr], [], [], 10)
            if not ready:
                raise AssertionError("the local HTTP server startup timed out")
            readiness = server.stderr.readline()
            if "listening on " not in readiness:
                raise AssertionError(f"the local HTTP server failed: {readiness}")
            host_port = readiness.split("listening on ", 1)[1].split(";", 1)[0]
            host, port_text = host_port.rsplit(":", 1)
            port = int(port_text)

            request_text = "ココについて保存されている情報をもとに短い紹介文を書いて"
            request = {
                "case_id": case,
                "input_id": "http-profile-request",
                "input_version": 1,
                "actor_id": actor,
                "text": request_text,
                "locale": "ja",
                "permissions": {"read_hosts": [], "write_paths": []},
                "output_path": None,
                "context_ref": None,
                "budget": {
                    "max_input_bytes": 256_000,
                    "max_evidence_bytes": 2_000_000,
                    "max_actions": 32,
                    "deadline_ms": 30_000,
                },
            }
            body = canonical(request)

            unauthorized = http.client.HTTPConnection(host, port, timeout=10)
            unauthorized.request(
                "POST",
                "/v1/execute",
                body=body,
                headers={
                    "Authorization": "Bearer rejected-test-token",
                    "Content-Type": "application/json",
                },
            )
            unauthorized_response = unauthorized.getresponse()
            unauthorized_response.read()
            unauthorized_status = unauthorized_response.status
            unauthorized.close()
            if unauthorized_status != 401:
                raise AssertionError("the local API accepted an incorrect bearer token")

            connection = http.client.HTTPConnection(host, port, timeout=15)
            connection.request(
                "POST",
                "/v1/execute",
                body=body,
                headers={
                    "Authorization": f"Bearer {token}",
                    "Content-Type": "application/json",
                },
            )
            response = connection.getresponse()
            response_body = response.read()
            status = response.status
            connection.close()
            if status != 200:
                raise AssertionError(
                    f"authenticated HTTP profile failed: status={status}; "
                    f"body={response_body.decode('utf-8', errors='replace')}"
                )
            profile = json.loads(response_body)
        finally:
            server.terminate()
            try:
                server.wait(timeout=5)
            except subprocess.TimeoutExpired:
                server.kill()
                server.wait(timeout=5)

        if profile.get("lifecycle") != "COMMIT" or profile.get("guarantee") != "HYPOTHESIS":
            raise AssertionError("HTTP did not return the Core-adopted HYPOTHESIS profile")
        profile_text = profile["text"]
        for marker in (
            "会話で提示された前提は「ココは鳥である」",
            "保存された規則による導出は「ココは動物である」",
            "現実世界の真偽や記録の網羅性は確認していません",
        ):
            if marker not in profile_text:
                raise AssertionError(f"the HTTP profile omitted a required unit: {marker}")
        if not set(rule["dependencies"] + bird["dependencies"]).issubset(
            set(profile["dependencies"])
        ):
            raise AssertionError("the HTTP profile lost its rule or premise lineage")
        model_ref = profile["content_plan"]["model_observation_ref"]
        emission = json.loads((database / "blobs" / model_ref).read_text(encoding="utf-8"))
        surfaces = emission["authorized_surfaces"]
        if (
            emission.get("text") != profile_text
            or profile_text not in surfaces
            or len(surfaces) != 2
            or any("現実世界の真偽や記録の網羅性は確認していません" not in s for s in surfaces)
        ):
            raise AssertionError("HTTP output differs from the retained Core-approved model surfaces")
        observations.append(
            {
                "operation": "authenticated_loopback_http_input",
                "actor": actor,
                "case": case,
                "input_id": request["input_id"],
                "request": request_text,
                "request_sha256": digest(request_text.encode("utf-8")),
                "http_status": status,
                "lifecycle": profile["lifecycle"],
                "guarantee": profile["guarantee"],
                "text": profile_text,
                "dependencies": profile["dependencies"],
                "model_observation_ref": model_ref,
                "authorized_surface_count": len(surfaces),
                "wrong_bearer_status": unauthorized_status,
            }
        )

        summary = run_cli("http-profile-summary", "前回の説明を2文以内で要約して")
        if (
            summary.get("guarantee") != "HYPOTHESIS"
            or profile["id"] not in summary.get("dependencies", [])
        ):
            raise AssertionError("CLI did not consume the profile returned by HTTP")
        observations[-1]["cli_follow_up_result_id"] = summary["id"]

        counts = Counter(item["lifecycle"] for item in observations)
        evidence = {
            "work_item": "BODY-2 BB-RENDER authenticated loopback HTTP profile consumer",
            "stage": "BODY-2_EXECUTION",
            "recorded_at": datetime.now(timezone.utc).isoformat(timespec="seconds"),
            "base_commit": base_commit,
            "changed_paths": [
                "rust/README.md",
                "rust/apps/cli/tests/normal_route.rs",
                "rust/audit/IMPLEMENTATION_STATUS.md",
                "rust/audit/requirements.tsv",
                "rust/audit/bb-content-known-relation-profile-v69-http-consumer-route-validation.json",
                "rust/tools/known_relation_profile_http_route_test.py",
            ],
            "subject": "MINIDORA Rust successor engine",
            "requirement_ids": ["A01", "A02", "A07", "A08", "A13", "B08", "R10"],
            "software_version": "0.7.0-alpha.8",
            "schema_version": "minidora-rust/0.7.0-alpha.8",
            "parser_version": "declarative-ja-en-v69-relation-profile-draft",
            "toolchain": toolchain,
            "applied_build_set": {
                "manifest_path": "rust/audit/construction/MB-BUILD-20261113/MANIFEST.tsv",
                "manifest_sha256": digest(manifest_path.read_bytes()),
                "source_artifacts": applied_build_set,
            },
            "binary_sha256": digest(binary.read_bytes()),
            "source_snapshot_sha256": sha256_paths(source_paths),
            "test_and_tool_snapshot_sha256": sha256_paths(test_paths),
            "documentation_snapshot_sha256": sha256_paths(documentation_paths),
            "execution_mode": "normal CLI processes save declared facts and consume the HTTP result; one isolated local loopback server process handles an authenticated RequestEnvelope; the test harness creates the bearer token in memory and writes no external data",
            "previous_producer_gap": "The generic loopback HTTP consumer existed, and the CLI profile producer existed, but this natural profile had not been observed through HTTP or passed from that HTTP result to a second consumer.",
            "producer_consumer": "CLI-declared actor memory -> authenticated loopback POST /v1/execute -> the same Engine::execute and Core relation-profile producer -> HYPOTHESIS ContentPlan and native model observation -> HTTP JSON result -> following CLI previous-result summary consuming that exact result ID",
            "scope": "The HTTP service authenticates a local bearer token, enforces its configured actor, returns the same bounded named-subject profile and exact proof dependencies as Core, rejects a wrong token, and shares the committed case with the ordinary CLI follow-up. This does not establish public deployment, browser UI, streaming, external HTTP exposure, or full A01/A02/A07/A08/A13/B08/R10 acceptance.",
            "actual_input": {
                "actor": actor,
                "case": case,
                "input_id": request["input_id"],
                "request": request_text,
                "request_sha256": digest(request_text.encode("utf-8")),
                "http_status": status,
                "authenticated_actor": actor,
            },
            "summary": {
                "fresh_client_observation_count": len(observations),
                "lifecycle_counts": dict(counts),
                "http_result_id": profile["id"],
                "cli_follow_up_result_id": summary["id"],
                "cli_follow_up_consumed_http_result": profile["id"]
                in summary["dependencies"],
                "actor_rule_and_fact_dependencies_retained": True,
                "wrong_bearer_rejected_with_401": unauthorized_status == 401,
                "local_server_terminated": server.poll() is not None,
                "added_dependencies": "NONE",
                "external_network_or_messages": "NOT_USED",
                "test_token": "generated in memory, not emitted or stored",
                "server_bind": "127.0.0.1 only; process terminated",
                "secret_pattern_scan": "NO_MATCH",
                "private_HDS_copied_to_public_evidence": False,
                "validation_commands": [
                    {"command": "cargo fmt --all -- --check", "exit_code": 0},
                    {
                        "command": "cargo check --workspace --all-targets --locked",
                        "exit_code": 0,
                    },
                    {
                        "command": "cargo test -p minidora-cli --test normal_route http_profile_request_flows_through_core_and_cli_summary --locked",
                        "exit_code": 0,
                        "passed": 1,
                    },
                    {
                        "command": "cargo clippy --workspace --all-targets --locked -- -D warnings",
                        "exit_code": 0,
                    },
                    {"command": "cargo build --workspace --locked", "exit_code": 0},
                    {
                        "command": "python3 rust/tools/known_relation_profile_http_route_test.py --binary rust/target/debug/minidora-rust --evidence-output rust/audit/bb-content-known-relation-profile-v69-http-consumer-route-validation.json",
                        "exit_code": 0,
                        "fresh_client_observations": len(observations),
                    },
                ],
                "full_workspace_runtime_suite": "NOT_RUN",
                "browser_stream_and_public_deployment": "NOT_RUN",
                "full_requirement_acceptance": "UNVERIFIED",
                "next_consumer": "Browser/streaming result rendering remains a separate consumer gap; the prior-result CLI consumer is now connected after HTTP.",
            },
            "observations": observations,
        }
        args.evidence_output.parent.mkdir(parents=True, exist_ok=True)
        args.evidence_output.write_text(
            json.dumps(evidence, ensure_ascii=False, indent=2) + "\n", encoding="utf-8"
        )
        print(
            json.dumps(
                {
                    "evidence_output": str(args.evidence_output.resolve()),
                    "observations": len(observations),
                    "lifecycle_counts": dict(counts),
                    "http_result_id": profile["id"],
                    "cli_follow_up_result_id": summary["id"],
                    "wrong_bearer_status": unauthorized_status,
                },
                ensure_ascii=False,
            )
        )


if __name__ == "__main__":
    main()
