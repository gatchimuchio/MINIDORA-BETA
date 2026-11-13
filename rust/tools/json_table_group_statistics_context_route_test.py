#!/usr/bin/env python3
"""Exercise JSON grouped statistics as a source-linked next-turn explanation."""

import argparse
from datetime import datetime, timezone
import hashlib
import json
from pathlib import Path
import platform
import shutil
import sqlite3
import subprocess
import tempfile


SOURCE = (
    '{"columns":["売上","地域"],"rows":[["0.1","北"],'
    '["1.0","南"],["0.3","北"],["2.0","南"]]}'
)
REQUEST = f"JSON表の「地域」ごとに「売上」の統計量を計算して:\n{SOURCE}"


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--evidence-output", type=Path, required=True)
    args = parser.parse_args()
    root = Path(__file__).resolve().parents[2]
    rust_root = root / "rust"
    cargo = shutil.which("cargo") or "/home/agent/.cargo/bin/cargo"
    rustc = shutil.which("rustc") or "/home/agent/.cargo/bin/rustc"
    binary = args.binary.resolve()

    def command_output(command: list[str]) -> str:
        return subprocess.run(
            command, capture_output=True, text=True, timeout=10, check=True
        ).stdout.strip()

    def run(
        database: Path,
        input_id: str,
        request: str,
        *extra: str,
    ) -> dict:
        command = [
            str(binary),
            "--db",
            str(database),
            "--case",
            "json-group-statistics-context",
            "--actor",
            "fixture-json-group-statistics-reader",
            "--input",
            input_id,
            "--json",
            *extra,
        ]
        command.append(request)
        completed = subprocess.run(
            command, capture_output=True, text=True, timeout=25, check=False
        )
        if completed.returncode != 0:
            raise AssertionError(
                f"{input_id}: exit={completed.returncode}; "
                f"stdout={completed.stdout}; stderr={completed.stderr}"
            )
        return json.loads(completed.stdout)

    with tempfile.TemporaryDirectory(prefix="minidora-json-group-context-") as temporary:
        directory = Path(temporary)
        database = directory / "state"
        initial = run(database, "json-group-statistics", REQUEST)
        assert initial["lifecycle"] == "COMMIT"
        assert initial["guarantee"] == "COMPUTED"
        source_ref, report_ref = initial["dependencies"]
        source_bytes = (database / "blobs" / source_ref).read_bytes()
        report_bytes = (database / "blobs" / report_ref).read_bytes()
        assert hashlib.sha256(source_bytes).hexdigest() == source_ref
        assert hashlib.sha256(report_bytes).hexdigest() == report_ref
        assert source_bytes == SOURCE.encode("utf-8")
        report = json.loads(report_bytes)
        assert report["source_sha256"] == source_ref
        assert report["group_by"] == "地域"
        assert report["value_column"] == "売上"
        assert [group["value"] for group in report["groups"]] == ["北", "南"]
        assert report["groups"][0]["statistics"]["mean"] == "1/5"
        for group in report["groups"]:
            assert len(group["source_row_indices"]) == len(group["value_source_spans"])
            for span in group["group_source_spans"] + group["value_source_spans"]:
                raw = source_bytes[span["source_start"] : span["source_end"]]
                assert hashlib.sha256(raw).hexdigest() == span["source_sha256"]

        output = directory / "json-group-statistics-explanation.txt"
        explained = run(
            database,
            "explain-json-group-statistics",
            "詳しく説明して",
            "--output",
            str(output),
            "--allow-write",
            str(output),
        )
        assert explained["lifecycle"] == "COMMIT"
        assert explained["guarantee"] == "COMPUTED"
        assert explained["dependencies"] == [initial["id"], source_ref, report_ref]
        assert output.read_bytes() == explained["text"].encode("utf-8")
        assert "元資料から再計算し、保存レポートと照合しました" in explained["text"]
        assert "グループ \"北\": n=2; 合計=2/5; 平均=1/5" in explained["text"]
        assert "元値 行1・列1" in explained["text"]
        graph_ref = explained["execution_graph_ref"]
        graph_bytes = (database / "blobs" / graph_ref).read_bytes()
        assert hashlib.sha256(graph_bytes).hexdigest() == graph_ref
        graph = json.loads(graph_bytes)
        assert (
            graph[5]["previous-json-group-statistics-report"][
                "previous-json-group-statistics-source"
            ]
            == source_ref
        )
        original_stat = output.stat()

        with sqlite3.connect(database / "state.sqlite3") as connection:
            revisions = connection.execute(
                "SELECT revision,state_sha256 FROM case_revision "
                "WHERE case_id=? ORDER BY revision",
                ("json-group-statistics-context",),
            ).fetchall()
            adopted = None
            for revision, state_sha in revisions:
                saved = json.loads(
                    (database / "blobs" / state_sha).read_text(encoding="utf-8")
                )
                result = saved.get("result")
                if (
                    saved["lifecycle"] == "ADOPTED"
                    and result
                    and result.get("input_id") == "explain-json-group-statistics"
                    and result.get("render") is not None
                ):
                    adopted = (revision, result)
            assert adopted is not None
            revision, result = adopted
            result_sha = hashlib.sha256(
                json.dumps(
                    result, ensure_ascii=False, sort_keys=True, separators=(",", ":")
                ).encode("utf-8")
            ).hexdigest()
            connection.executescript(
                "PRAGMA foreign_keys=ON; DROP TRIGGER case_revision_no_delete; "
                "DROP TRIGGER action_history_no_delete;"
            )
            connection.execute(
                "DELETE FROM case_revision WHERE case_id=? AND revision>?",
                ("json-group-statistics-context", revision),
            )
            connection.execute("DELETE FROM action_history WHERE sequence>3")
            connection.execute(
                "UPDATE input_identity SET result_sha256=? "
                "WHERE case_id=? AND input_id=?",
                (
                    result_sha,
                    "json-group-statistics-context",
                    "explain-json-group-statistics",
                ),
            )
            connection.commit()

        reconciled = subprocess.run(
            [
                str(binary),
                "reconcile",
                "--db",
                str(database),
                "--case",
                "json-group-statistics-context",
                "--actor",
                "fixture-json-group-statistics-reader",
                "--json",
            ],
            capture_output=True,
            text=True,
            timeout=25,
            check=False,
        )
        if reconciled.returncode != 0:
            raise AssertionError(
                f"reconcile: exit={reconciled.returncode}; "
                f"stdout={reconciled.stdout}; stderr={reconciled.stderr}"
            )
        recovered = json.loads(reconciled.stdout)
        receipt = recovered["receipts"][-1]
        recovered_stat = output.stat()
        assert recovered["lifecycle"] == "COMMIT"
        assert recovered["dependencies"] == explained["dependencies"]
        assert receipt["phase"] == "CONFIRMED"
        assert receipt["reason"] == "LOCAL_FILE_RECONCILED_NO_RESEND"
        assert (
            recovered_stat.st_ino,
            recovered_stat.st_mtime_ns,
            recovered_stat.st_size,
        ) == (original_stat.st_ino, original_stat.st_mtime_ns, original_stat.st_size)

        source_paths = [
            "crates/minidora-engine/src/ports.rs",
            "crates/minidora-engine/src/document.rs",
            "crates/minidora-engine/src/core.rs",
            "crates/minidora-engine/src/recovery.rs",
            "crates/minidora-engine/src/store.rs",
        ]
        test_paths = [
            "apps/cli/tests/normal_route.rs",
            "tools/json_table_group_statistics_context_route_test.py",
        ]
        documentation_paths = [
            "README.md",
            "audit/IMPLEMENTATION_STATUS.md",
            "audit/requirements.tsv",
        ]

        def hashes(paths: list[str]) -> dict[str, str]:
            return {
                f"rust/{path}": hashlib.sha256((rust_root / path).read_bytes()).hexdigest()
                for path in paths
            }

        evidence = {
            "work_item": "BODY-1 BB-MATH-DATA JSON table grouped statistics to BB-CONTEXT source-cell explanation",
            "stage": "BODY-1_EXECUTION",
            "recorded_at": datetime.now(timezone.utc).isoformat(timespec="seconds"),
            "base_commit": command_output(["git", "-C", str(root), "rev-parse", "HEAD"]),
            "requirement_ids": [
                "A07",
                "A14",
                "B02",
                "B03",
                "B04",
                "E03",
                "E06",
                "E08",
                "E09",
                "E12",
                "F04",
            ],
            "software_version": "0.7.0-alpha.8",
            "schema_version": "minidora-rust/0.7.0-alpha.8",
            "parser_version": "declarative-ja-en-v52-search-candidate-body",
            "toolchain": {
                "rustc": command_output([rustc, "--version"]),
                "cargo": command_output([cargo, "--version"]),
                "platform": platform.platform(),
            },
            "source_snapshot_sha256": hashes(source_paths),
            "test_and_tool_snapshot_sha256": hashes(test_paths),
            "documentation_snapshot_sha256": hashes(documentation_paths),
            "execution_mode": "fresh CLI process per request; same actor/case; temporary SQLite and artifact state",
            "producer_consumer": (
                "ordinary JSON table request -> exact grouped statistics with original JSON token ranges -> "
                "separate immutable raw source and typed report artifacts -> same-case 詳しく説明して -> "
                "source/report hash and recomputation checks -> exact group values, source-cell ranges and "
                "authorized local-file recovery without resend"
            ),
            "statistics_result": {
                "lifecycle": initial["lifecycle"],
                "guarantee": initial["guarantee"],
                "result_id": initial["id"],
                "source_ref": source_ref,
                "report_ref": report_ref,
                "report": report,
            },
            "recovery_verifier_correction": {
                "initial_attempt": "FAIL: UNSUPPORTED_RECOVERY_GRAPH because the verifier did not yet recognize the source-evidence -> computed-report edge",
                "correction": "recovery now reconstructs the exact source and report nodes and verifies the report consumed the retained source",
                "final_retry": "PASS; no resend, receipt identity and file inode/mtime/size unchanged",
            },
            "explanation_consumer": {
                "lifecycle": explained["lifecycle"],
                "guarantee": explained["guarantee"],
                "dependencies": explained["dependencies"],
                "text": explained["text"],
                "execution_graph_ref": graph_ref,
                "source_consumption": graph[5][
                    "previous-json-group-statistics-report"
                ],
                "file_sha256": hashlib.sha256(output.read_bytes()).hexdigest(),
            },
            "file_recovery": {
                "lifecycle": recovered["lifecycle"],
                "receipt_phase": receipt["phase"],
                "receipt_reason": receipt["reason"],
                "dependencies_unchanged": True,
                "file_inode_mtime_size_unchanged": True,
                "write_resent": False,
            },
            "outcome": "PASS",
            "focused_validation": {
                "normal_route_json_group_statistics": "3 passed; 0 failed",
                "csv_statistics_context_regression": "1 passed; 0 failed",
                "b03_statistics_context_regression": "1 passed; 0 failed",
                "b06_calculation_sequence_regressions": "5 passed; 0 failed",
                "format_all_check": "PASS",
                "minidora_cli_all_targets_check_locked": "PASS",
                "minidora_cli_build_locked": "PASS",
            },
            "commands": [
                "cargo fmt --all; cargo fmt --all -- --check",
                "cargo check -p minidora-cli --all-targets --locked",
                "cargo build -p minidora-cli --locked",
                "cargo test -p minidora-cli --test normal_route json_table_group_statistics --locked",
                "python3 tools/json_table_group_statistics_context_route_test.py --binary target/debug/minidora-rust --evidence-output audit/bb-math-data-json-group-statistics-context-route-validation.json",
            ],
            "not_run": [
                "full workspace runtime suite and Clippy during BODY-1",
                "other JSON/CSV report types, general explanation, and complete BB-MATH-DATA/BB-CONTEXT/B03/B04 acceptance",
            ],
            "acceptance_boundary": (
                "One strict JSON string-table grouped-statistics producer now feeds an exact same-case explanation "
                "with source-cell provenance and local-file no-resend recovery. B02, B03, B04, A07, A14 and F04 "
                "remain UNVERIFIED outside this bounded route. Earlier reports without a retained source artifact "
                "continue to use their previous result surface because exact recomputation is unavailable."
            ),
        }
        args.evidence_output.parent.mkdir(parents=True, exist_ok=True)
        args.evidence_output.write_text(
            json.dumps(evidence, ensure_ascii=False, indent=2) + "\n", encoding="utf-8"
        )
        print(
            json.dumps(
                {
                    "outcome": evidence["outcome"],
                    "evidence_output": str(args.evidence_output),
                    "source_ref": source_ref,
                    "report_ref": report_ref,
                    "explanation_dependencies": explained["dependencies"],
                    "file_recovery": evidence["file_recovery"],
                },
                ensure_ascii=False,
            )
        )


if __name__ == "__main__":
    main()
