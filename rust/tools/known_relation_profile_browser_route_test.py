#!/usr/bin/env python3
"""Validate the local browser UI against a fresh MINIDORA process."""

import argparse
import csv
from collections import Counter
from datetime import datetime, timezone
import hashlib
import json
import os
from pathlib import Path
import platform
import secrets
import shutil
import subprocess
import tempfile

from playwright.sync_api import expect, sync_playwright


def digest(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def command_output(command: list[str], cwd: Path, env: dict[str, str]) -> str:
    return subprocess.run(
        command, cwd=cwd, env=env, capture_output=True, text=True, timeout=300, check=True
    ).stdout.strip()


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, default=Path("rust/target/debug/minidora-rust"))
    parser.add_argument("--chromium", type=Path, default=Path("/usr/bin/chromium"))
    parser.add_argument("--evidence-output", type=Path, required=True)
    parser.add_argument("--screenshot-output", type=Path)
    args = parser.parse_args()

    repository_root = Path(__file__).resolve().parents[2]
    rust_root = repository_root / "rust"
    binary = (repository_root / args.binary).resolve() if not args.binary.is_absolute() else args.binary
    chromium = args.chromium.resolve()
    cargo = shutil.which("cargo") or "/workspace/.cargo/bin/cargo"
    rustc = shutil.which("rustc") or "/workspace/.cargo/bin/rustc"
    env = os.environ.copy()
    env["PATH"] = f"/workspace/.cargo/bin:{env.get('PATH', '')}"
    env.setdefault("CARGO_HOME", "/workspace/.cargo")
    env.setdefault("RUSTUP_HOME", "/workspace/.rustup")

    validation_commands: list[dict[str, object]] = []

    def validate(command: list[str], cwd: Path) -> None:
        completed = subprocess.run(
            command, cwd=cwd, env=env, capture_output=True, text=True, timeout=300
        )
        validation_commands.append(
            {
                "command": " ".join(command),
                "exit_code": completed.returncode,
            }
        )
        if completed.returncode != 0:
            raise RuntimeError(
                f"validation failed ({completed.returncode}): {' '.join(command)}\n"
                f"stdout={completed.stdout}\nstderr={completed.stderr}"
            )

    base_commit = command_output(
        ["git", "rev-parse", "HEAD"], repository_root, env
    )
    cargo_commands = [
        [cargo, "fmt", "--all", "--", "--check"],
        [cargo, "check", "--workspace", "--all-targets", "--locked"],
        [
            cargo,
            "test",
            "-p",
            "minidora-cli",
            "--test",
            "normal_route",
            "loopback_browser_assets_are_fixed_and_security_headered",
            "--locked",
        ],
        [cargo, "clippy", "--workspace", "--all-targets", "--locked", "--", "-D", "warnings"],
        [cargo, "build", "--workspace", "--locked"],
    ]
    for command in cargo_commands:
        validate(command, rust_root)

    actor, case = "profile-browser-owner", "profile-browser-case"
    observations: list[dict[str, object]] = []
    screenshot_path = (
        (repository_root / args.screenshot_output).resolve()
        if args.screenshot_output is not None and not args.screenshot_output.is_absolute()
        else args.screenshot_output
    )
    screenshot_record: dict[str, object] | None = None
    browser_errors: list[str] = []
    origins: set[str] = set()
    local_token = secrets.token_hex(32)

    with tempfile.TemporaryDirectory(prefix="minidora-profile-browser-") as temporary:
        database = Path(temporary) / "state"

        def run_cli(input_id: str, request_text: str) -> dict:
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
                    request_text,
                ],
                cwd=repository_root,
                env=env,
                capture_output=True,
                text=True,
                timeout=15,
            )
            if completed.returncode != 0:
                raise RuntimeError(
                    f"CLI input failed: {input_id}; stderr={completed.stderr}; stdout={completed.stdout}"
                )
            result = json.loads(completed.stdout)
            if result.get("lifecycle") != "COMMIT":
                raise AssertionError(f"CLI precondition did not commit: {result}")
            observations.append(
                {
                    "operation": "normal_cli_input",
                    "actor": actor,
                    "case": case,
                    "input_id": input_id,
                    "request": request_text,
                    "request_sha256": digest(request_text.encode("utf-8")),
                    "lifecycle": result["lifecycle"],
                    "guarantee": result["guarantee"],
                    "result_id": result["id"],
                    "dependencies": result["dependencies"],
                }
            )
            return result

        rule = run_cli("browser-profile-rule", "もしXは鳥ならXは動物")
        premise = run_cli("browser-profile-premise", "ココは鳥")

        server_env = env.copy()
        server_env["MINIDORA_API_TOKEN"] = local_token
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
            cwd=repository_root,
            env=server_env,
            stdin=subprocess.DEVNULL,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.PIPE,
            text=True,
        )
        try:
            if server.stderr is None:
                raise AssertionError("loopback server has no readiness stream")
            import selectors

            with selectors.DefaultSelector() as selector:
                selector.register(server.stderr, selectors.EVENT_READ)
                if not selector.select(timeout=10):
                    raise AssertionError("local HTTP server startup timed out")
            readiness = server.stderr.readline()
            if "listening on " not in readiness:
                raise AssertionError(f"local server failed to start: {readiness}")
            host_port = readiness.split("listening on ", 1)[1].split(";", 1)[0]
            host, port_text = host_port.rsplit(":", 1)
            port = int(port_text)
            base_url = f"http://{host}:{port}"

            request_text = "ココについて保存されている情報をもとに短い紹介文を書いて"
            with sync_playwright() as playwright:
                browser = playwright.chromium.launch(
                    headless=True,
                    executable_path=str(chromium),
                    args=["--no-sandbox", "--disable-dev-shm-usage"],
                )
                page = browser.new_page(viewport={"width": 1440, "height": 1100})
                page.on(
                    "request",
                    lambda request: origins.add(
                        request.url.split("/", 3)[0] + "//" + request.url.split("/", 3)[2]
                    ),
                )
                page.on("pageerror", lambda error: browser_errors.append(str(error)))
                page.on(
                    "console",
                    lambda message: browser_errors.append(message.text)
                    if message.type == "error"
                    else None,
                )
                response = page.goto(base_url + "/", wait_until="networkidle", timeout=15_000)
                if response is None or response.status != 200:
                    raise AssertionError("loopback browser page did not return HTTP 200")
                headers = response.all_headers()
                for header in [
                    "content-security-policy",
                    "x-content-type-options",
                    "x-frame-options",
                    "referrer-policy",
                    "permissions-policy",
                ]:
                    if not headers.get(header):
                        raise AssertionError(f"missing browser security header: {header}")
                if "default-src 'self'" not in headers["content-security-policy"]:
                    raise AssertionError("browser CSP is not same-origin-only")
                expect(page.locator("#connection-status")).to_contain_text("Core 接続中")
                if page.title() != "MINIDORA — 対話ワークスペース":
                    raise AssertionError("browser UI title did not load")

                page.locator("#actor-id").fill(actor)
                page.locator("#case-id").fill(case)
                page.locator("#api-token").fill(local_token)
                page.locator("#request-text").fill(request_text)
                page.locator("#submit-button").click()
                expect(page.locator("#result-state")).to_have_text("COMMIT", timeout=15_000)
                page.locator(".record-details summary").click()
                result = json.loads(page.locator("#record-json").inner_text())
                displayed_dependencies = page.locator("#dependency-list li").all_inner_texts()

                if result.get("lifecycle") != "COMMIT" or result.get("guarantee") != "HYPOTHESIS":
                    raise AssertionError("browser did not render the committed HYPOTHESIS profile")
                if not all(
                    phrase in page.locator("#answer-text").inner_text()
                    for phrase in ["ココは動物である", "ココは鳥である", "現実世界の真偽"]
                ):
                    raise AssertionError("browser profile text lost a claim or its disclaimer")
                if displayed_dependencies != result.get("dependencies"):
                    raise AssertionError("browser dependency list differs from the Core result")
                if not {rule["dependencies"][0], premise["dependencies"][0]}.issubset(
                    set(result.get("dependencies", []))
                ):
                    raise AssertionError("browser result omitted the direct premise or rule")
                browser_script = (rust_root / "apps/cli/web/app.js").read_text(encoding="utf-8")
                if any(
                    marker in browser_script
                    for marker in ["localStorage", "sessionStorage", "document.cookie"]
                ):
                    raise AssertionError("browser token script must not persist local state")

                page.locator("#api-token").fill("")
                if screenshot_path is not None:
                    screenshot_path.parent.mkdir(parents=True, exist_ok=True)
                    page.locator(".record-details summary").click()
                    page.screenshot(path=str(screenshot_path), full_page=True)
                    screenshot_bytes = screenshot_path.read_bytes()
                    screenshot_record = {
                        "path": str(screenshot_path.relative_to(repository_root)),
                        "bytes": len(screenshot_bytes),
                        "sha256": digest(screenshot_bytes),
                        "viewport": {"width": 1440, "height": 1100},
                    }

                observations.append(
                    {
                        "operation": "chromium_browser_ui_request",
                        "actor": actor,
                        "case": case,
                        "request": request_text,
                        "request_sha256": digest(request_text.encode("utf-8")),
                        "http_status": 200,
                        "lifecycle": result["lifecycle"],
                        "guarantee": result["guarantee"],
                        "input_id": result["input_id"],
                        "result_id": result["id"],
                        "text": result["text"],
                        "text_sha256": digest(result["text"].encode("utf-8")),
                        "dependencies": result["dependencies"],
                        "dependency_list_rendered_exactly": True,
                        "security_headers": sorted(
                            key
                            for key in [
                                "content-security-policy",
                                "x-content-type-options",
                                "x-frame-options",
                                "referrer-policy",
                                "permissions-policy",
                            ]
                            if headers.get(key)
                        ),
                    }
                )

                bad_token_page = browser.new_page()
                bad_token_page.goto(base_url + "/", wait_until="networkidle", timeout=15_000)
                bad_token_page.locator("#actor-id").fill(actor)
                bad_token_page.locator("#case-id").fill(case)
                bad_token_page.locator("#api-token").fill("invalid-browser-test-token")
                bad_token_page.locator("#request-text").fill("2+2")
                bad_token_page.locator("#submit-button").click()
                expect(bad_token_page.locator("#result-state")).to_have_text(
                    "HTTP ERROR", timeout=15_000
                )
                if "authentication required" not in bad_token_page.locator("#reason-text").inner_text():
                    raise AssertionError("the browser did not surface the HTTP 401 response")
                observations.append(
                    {
                        "operation": "chromium_browser_ui_wrong_token",
                        "http_status": 401,
                        "authentication_rejected": True,
                    }
                )
                if origins != {base_url}:
                    raise AssertionError(f"browser requested an external origin: {sorted(origins)}")
                if browser_errors:
                    raise AssertionError(f"browser console errors: {browser_errors}")
                browser.close()
        finally:
            if server.poll() is None:
                server.terminate()
            try:
                server.wait(timeout=5)
            except subprocess.TimeoutExpired:
                server.kill()
                server.wait(timeout=5)

    manifest_path = rust_root / "audit/construction/MB-BUILD-20261113/MANIFEST.tsv"
    with manifest_path.open(encoding="utf-8", newline="") as manifest_file:
        manifest_rows = list(csv.DictReader(manifest_file, delimiter="\t"))
    applied_build_set = [
        {
            "artifact_name": row["artifact_name"],
            "sha256": row["sha256"],
            "bytes": int(row["bytes"]),
            "role": row["role"],
            "repository_path": row["repository_path"],
        }
        for row in manifest_rows
        if row["role"]
        in {
            "original_user_archive",
            "build_specification",
            "build_schedule",
            "codex_work_instruction",
        }
    ]

    def snapshot(paths: list[str], root: Path) -> dict[str, str]:
        return {
            str((root / path).relative_to(repository_root)): digest((root / path).read_bytes())
            for path in paths
        }

    source_paths = [
        "apps/cli/src/main.rs",
        "apps/cli/web/index.html",
        "apps/cli/web/app.js",
        "apps/cli/web/styles.css",
        "apps/cli/web/favicon.svg",
    ]
    test_paths = [
        "apps/cli/tests/normal_route.rs",
        "tools/known_relation_profile_browser_route_test.py",
    ]
    documentation_paths = [
        "README.md",
        "audit/IMPLEMENTATION_STATUS.md",
        "audit/requirements.tsv",
        "docs/IMPLEMENTATION_CHOICES.md",
        "audit/construction/MB-BUILD-20261113/MANIFEST.tsv",
    ]
    evidence = {
        "work_item": "BODY-3 BB-RENDER loopback browser UI consumer for a saved-relation profile",
        "stage": "BODY-3_EXECUTION",
        "recorded_at": datetime.now(timezone.utc).isoformat(),
        "base_commit": base_commit,
        "changed_paths": [],
        "subject": "MINIDORA Rust successor engine",
        "requirement_ids": ["A01", "A02", "A07", "A08", "A13", "B08"],
        "software_version": "0.7.0-alpha.8",
        "schema_version": "minidora-rust/0.7.0-alpha.8",
        "parser_version": "declarative-ja-en-v69-relation-profile-draft",
        "toolchain": {
            "rustc": command_output([rustc, "--version"], repository_root, env),
            "cargo": command_output([cargo, "--version"], repository_root, env),
            "python": platform.python_version(),
            "chromium": command_output([str(chromium), "--version"], repository_root, env),
            "platform": platform.platform(),
        },
        "applied_build_set": {
            "manifest_path": "rust/audit/construction/MB-BUILD-20261113/MANIFEST.tsv",
            "manifest_sha256": digest(manifest_path.read_bytes()),
            "source_artifacts": applied_build_set,
        },
        "binary_sha256": digest(binary.read_bytes()),
        "source_snapshot_sha256": snapshot(source_paths, rust_root),
        "test_and_tool_snapshot_sha256": snapshot(test_paths, rust_root),
        "documentation_snapshot_sha256": snapshot(documentation_paths, rust_root),
        "execution_mode": "fresh local CLI producer processes -> 127.0.0.1-only authenticated Rust HTTP process -> Chromium Playwright form submission -> same Engine/Core result -> browser rendered result and dependency list; wrong-token request rejected; no external requests or data",
        "previous_consumer_gap": "The authenticated loopback HTTP route had been validated directly, but no browser form or rendered dependency list was connected to that route.",
        "producer_consumer": "CLI-saved actor premise/rule -> same-origin static loopback browser UI -> authenticated POST /v1/execute -> same Engine/Core relation-profile producer -> HYPOTHESIS result -> browser renders lifecycle, guarantee, text, disclosure, result ID and exact dependency list",
        "scope": "Confirms one local Chromium browser submission, same-origin-only resource requests, security headers, displayed proof dependencies and a wrong-token 401. This does not establish public deployment, streaming, broad browser compatibility, all UI workflows, full workspace runtime regressions or complete requirement acceptance.",
        "actual_input": {
            "actor": "profile-browser-owner",
            "case": "profile-browser-case",
            "request": "ココについて保存されている情報をもとに短い紹介文を書いて",
            "request_sha256": digest("ココについて保存されている情報をもとに短い紹介文を書いて".encode("utf-8")),
            "browser": "Chromium headless via Playwright",
            "server_bind": "127.0.0.1 only; ephemeral port",
            "test_token": "generated randomly in process memory; not recorded or persisted",
        },
        "summary": {
            "fresh_client_observation_count": len(observations),
            "lifecycle_counts": dict(
                Counter(
                    observation.get("lifecycle", "HTTP_ERROR")
                    for observation in observations
                )
            ),
            "browser_result_id": next(
                item["result_id"]
                for item in observations
                if item["operation"] == "chromium_browser_ui_request"
            ),
            "wrong_token_rejected_with_401": True,
            "browser_requests_same_origin_only": True,
            "browser_console_errors": browser_errors,
            "browser_server_terminated": True,
            "added_dependencies": "NONE to the Rust application; browser verification uses the preinstalled cloud Playwright/Chromium",
            "external_network_or_messages": "NOT_USED",
            "screenshot": screenshot_record,
            "validation_commands": validation_commands,
            "full_workspace_runtime_suite": "NOT_RUN",
            "streaming_public_deployment_and_broad_browser_matrix": "NOT_RUN",
            "full_requirement_acceptance": "UNVERIFIED",
            "next_consumer": "Streaming remains unverified; broad browser acceptance and general B08 writing quality remain outside this scoped observation.",
        },
        "observations": observations,
    }
    evidence["changed_paths"] = [
        "rust/README.md",
        "rust/apps/cli/src/main.rs",
        "rust/apps/cli/tests/normal_route.rs",
        "rust/apps/cli/web/index.html",
        "rust/apps/cli/web/app.js",
        "rust/apps/cli/web/styles.css",
        "rust/apps/cli/web/favicon.svg",
        "rust/audit/IMPLEMENTATION_STATUS.md",
        "rust/audit/requirements.tsv",
        "rust/audit/bb-content-known-relation-profile-v69-browser-ui-route-validation.json",
        "rust/tools/known_relation_profile_browser_route_test.py",
    ]
    if screenshot_record is not None:
        evidence["changed_paths"].append(screenshot_record["path"])

    evidence_path = args.evidence_output
    if not evidence_path.is_absolute():
        evidence_path = repository_root / evidence_path
    evidence_path.parent.mkdir(parents=True, exist_ok=True)
    evidence_path.write_text(
        json.dumps(evidence, ensure_ascii=False, indent=2) + "\n", encoding="utf-8"
    )
    print(
        json.dumps(
            {
                "evidence": str(evidence_path.relative_to(repository_root)),
                "browser_result_id": evidence["summary"]["browser_result_id"],
                "observation_count": len(observations),
                "screenshot": screenshot_record,
            },
            ensure_ascii=False,
        )
    )


if __name__ == "__main__":
    main()
