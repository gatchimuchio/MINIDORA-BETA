#!/usr/bin/env python3
"""Exercise a CSV statistics result as a source-linked next-turn explanation."""

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


SOURCE = "label,value\r\nnorth,0.1\r\nsouth,0.2\r\nwest,0.3\r\n"
REQUEST = f"CSVの列「value」の統計量を計算して:\n{SOURCE}"


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
            "csv-statistics-context",
            "--actor",
            "fixture-csv-statistics-reader",
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

    with tempfile.TemporaryDirectory(prefix="minidora-csv-statistics-context-") as temporary:
        directory = Path(temporary)
        database = directory / "state"
        initial = run(database, "csv-statistics", REQUEST)
        assert initial["lifecycle"] == "COMMIT"
        assert initial["guarantee"] == "COMPUTED"
        assert len(initial["dependencies"]) == 1
        report_ref = initial["dependencies"][0]
        report_bytes = (database / "blobs" / report_ref).read_bytes()
        assert hashlib.sha256(report_bytes).hexdigest() == report_ref
        report = json.loads(report_bytes)
        assert report["operation"] == "CSV_STATISTICS"
        assert report["source_document"] == SOURCE
        assert report["source_sha256"] == hashlib.sha256(SOURCE.encode()).hexdigest()
        assert report["output_sha256"] == hashlib.sha256(report["text"].encode()).hexdigest()
        assert report["text"] == initial["text"]
        numeric_spans = [
            span for span in report["spans"] if span["kind"] == "CSV_NUMERIC_OBSERVATION"
        ]
        assert len(numeric_spans) == 3
        for span in numeric_spans:
            assert SOURCE.encode()[span["source_start"] : span["source_end"]].decode() == span[
                "source_text"
            ]
        statistics = json.loads(initial["text"])["statistics"]
        assert statistics["mean"] == "1/5"

        output = directory / "csv-statistics-explanation.txt"
        explained = run(
            database,
            "explain-csv-statistics",
            "詳しく説明して",
            "--output",
            str(output),
            "--allow-write",
            str(output),
        )
        assert explained["lifecycle"] == "COMMIT"
        assert explained["guarantee"] == "COMPUTED"
        assert explained["dependencies"] == [initial["id"], report_ref]
        assert output.read_bytes() == explained["text"].encode("utf-8")
        assert report["source_sha256"] in explained["text"]
        assert report_ref in explained["text"]
        assert '"mean":"1/5"' in explained["text"]
        assert "計算に使った元セル: 3件" in explained["text"]
        assert "元セル 行2・列2" in explained["text"]
        original_stat = output.stat()

        with sqlite3.connect(database / "state.sqlite3") as connection:
            revisions = connection.execute(
                "SELECT revision,state_sha256 FROM case_revision "
                "WHERE case_id=? ORDER BY revision",
                ("csv-statistics-context",),
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
                    and result.get("input_id") == "explain-csv-statistics"
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
                ("csv-statistics-context", revision),
            )
            connection.execute("DELETE FROM action_history WHERE sequence>3")
            connection.execute(
                "UPDATE input_identity SET result_sha256=? "
                "WHERE case_id=? AND input_id=?",
                (result_sha, "csv-statistics-context", "explain-csv-statistics"),
            )
            connection.commit()

        reconciled = subprocess.run(
            [
                str(binary),
                "reconcile",
                "--db",
                str(database),
                "--case",
                "csv-statistics-context",
                "--actor",
                "fixture-csv-statistics-reader",
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
        assert (recovered_stat.st_ino, recovered_stat.st_mtime_ns, recovered_stat.st_size) == (
            original_stat.st_ino,
            original_stat.st_mtime_ns,
            original_stat.st_size,
        )

        source_paths = [
            "crates/minidora-engine/src/document.rs",
            "crates/minidora-engine/src/core.rs",
            "crates/minidora-engine/src/recovery.rs",
            "crates/minidora-engine/src/store.rs",
        ]
        test_paths = [
            "apps/cli/tests/normal_route.rs",
            "tools/csv_statistics_context_followup_route_test.py",
        ]
        documentation_paths = ["README.md", "audit/IMPLEMENTATION_STATUS.md", "audit/requirements.tsv"]

        def hashes(paths: list[str]) -> dict[str, str]:
            return {
                f"rust/{path}": hashlib.sha256((rust_root / path).read_bytes()).hexdigest()
                for path in paths
            }

        report = {
            "work_item": "BODY-1 BB-MATH-DATA CSV statistics to BB-CONTEXT source-cell explanation",
            "stage": "BODY-1_EXECUTION",
            "recorded_at": datetime.now(timezone.utc).isoformat(timespec="seconds"),
            "base_commit": command_output(["git", "-C", str(root), "rev-parse", "HEAD"]),
            "requirement_ids": ["A07", "A14", "B02", "B04", "E08", "E12", "F04"],
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
                "ordinary CSV input -> exact decimal statistics -> immutable DocumentResult with original-cell spans -> "
                "ordinary same-case 詳しく説明して -> report/source revalidation -> exact values and cell coordinates -> "
                "authorized local file -> recovery without resend"
            ),
            "scope": (
                "Only the existing CSV_STATISTICS producer is consumed. Core recomputes the report from its retained exact "
                "source, validates artifact and source/output hashes and every cell span, and explains the exact statistic "
                "plus up to eight source cells. Other CSV/JSON operation reports and general explanation remain outside this route."
            ),
            "statistics_result": {
                "lifecycle": initial["lifecycle"],
                "guarantee": initial["guarantee"],
                "result_id": initial["id"],
                "report_ref": report_ref,
                "source_sha256": report["source_sha256"],
                "output_sha256": report["output_sha256"],
                "statistics": statistics,
                "numeric_source_spans": numeric_spans,
            },
            "explanation_consumer": {
                "lifecycle": explained["lifecycle"],
                "guarantee": explained["guarantee"],
                "dependencies": explained["dependencies"],
                "text": explained["text"],
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
            "commands": [
                "cargo fmt --all -- --check",
                "cargo check --workspace --all-targets --locked",
                "cargo test -p minidora-cli --test normal_route csv_statistics_followup_explains_revalidated_source_cells --locked",
                "python3 tools/csv_statistics_context_followup_route_test.py --binary target/debug/minidora-rust --evidence-output audit/bb-math-data-csv-statistics-context-followup-route-validation.json",
            ],
            "not_run": [
                "full workspace runtime suite and Clippy during BODY-1",
                "other CSV/JSON document operations, grouped statistics, arbitrary explanation and complete BB-MATH-DATA/BB-CONTEXT acceptance",
            ],
            "acceptance_boundary": (
                "One exact CSV statistics report now feeds a source-cell-aware context explanation and no-resend file recovery. "
                "B02, B04, A07, A14 and F04 remain UNVERIFIED outside this bounded route."
            ),
        }
        args.evidence_output.parent.mkdir(parents=True, exist_ok=True)
        args.evidence_output.write_text(
            json.dumps(report, ensure_ascii=False, indent=2) + "\n", encoding="utf-8"
        )
        print(json.dumps(report, ensure_ascii=False, indent=2))


if __name__ == "__main__":
    main()
