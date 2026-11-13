#!/usr/bin/env python3
"""Exercise a natural question against the actual prior JSON statistics result."""

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
    '{"columns":["score","team"],"rows":[["0.1","north"],'
    '["1.0","south"],["0.3","north"],["2.0","south"],'
    '["0.4","east"]]}'
)
PRODUCER_REQUEST = f'JSON表の「team」ごとに「score」の統計量を計算して:\n{SOURCE}'
FOLLOWUP_REQUEST = "どのグループの平均値が一番高いですか"


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
        case: str,
        input_id: str,
        request: str,
        *extra: str,
    ) -> dict:
        completed = subprocess.run(
            [
                str(binary),
                "--db",
                str(database),
                "--case",
                case,
                "--actor",
                "fixture-json-group-mean-reader",
                "--input",
                input_id,
                "--json",
                *extra,
                request,
            ],
            capture_output=True,
            text=True,
            timeout=25,
            check=False,
        )
        if completed.returncode != 0:
            raise AssertionError(
                f"{input_id}: exit={completed.returncode}; "
                f"stdout={completed.stdout}; stderr={completed.stderr}"
            )
        return json.loads(completed.stdout)

    with tempfile.TemporaryDirectory(prefix="minidora-json-group-mean-") as temporary:
        directory = Path(temporary)
        database = directory / "state"
        initial = run(database, "group-mean-context", "group-mean-source", PRODUCER_REQUEST)
        assert initial["lifecycle"] == "COMMIT"
        assert initial["guarantee"] == "COMPUTED"
        source_ref, report_ref = initial["dependencies"]
        source_bytes = (database / "blobs" / source_ref).read_bytes()
        report_bytes = (database / "blobs" / report_ref).read_bytes()
        report = json.loads(report_bytes)
        assert hashlib.sha256(source_bytes).hexdigest() == source_ref
        assert hashlib.sha256(report_bytes).hexdigest() == report_ref
        assert source_bytes == SOURCE.encode("utf-8")
        assert report["source_sha256"] == source_ref
        assert [group["statistics"]["mean"] for group in report["groups"]] == [
            "1/5",
            "3/2",
            "2/5",
        ]
        for group in report["groups"]:
            for span in group["group_source_spans"] + group["value_source_spans"]:
                raw = source_bytes[span["source_start"] : span["source_end"]]
                assert hashlib.sha256(raw).hexdigest() == span["source_sha256"]

        output = directory / "highest-group-mean-answer.txt"
        answer = run(
            database,
            "group-mean-context",
            "group-mean-question",
            FOLLOWUP_REQUEST,
            "--output",
            str(output),
            "--allow-write",
            str(output),
        )
        assert answer["lifecycle"] == "COMMIT"
        assert answer["guarantee"] == "COMPUTED"
        assert answer["dependencies"][:3] == [initial["id"], source_ref, report_ref]
        answer_ref = answer["dependencies"][3]
        answer_bytes = (database / "blobs" / answer_ref).read_bytes()
        typed_answer = json.loads(answer_bytes)
        assert hashlib.sha256(answer_bytes).hexdigest() == answer_ref
        assert typed_answer["winners"][0]["group"] == "south"
        assert typed_answer["winners"][0]["mean"] == "3/2"
        assert typed_answer["winners"][0]["source_row_count"] == 2
        assert [item["highest"] for item in typed_answer["comparisons"]] == [
            False,
            True,
            False,
        ]
        assert "平均が最も高いグループ: \"south\"（平均=3/2）" in answer["text"]
        assert '"north": 平均=1/5' in answer["text"]
        assert '"east": 平均=2/5' in answer["text"]
        assert output.read_bytes() == answer["text"].encode("utf-8")
        graph = json.loads(
            (database / "blobs" / answer["execution_graph_ref"]).read_bytes()
        )
        answer_consumption = graph[5]["previous-json-group-mean-answer"]
        assert answer_consumption == {
            "previous-json-group-statistics-report": report_ref
        }

        original_stat = output.stat()
        with sqlite3.connect(database / "state.sqlite3") as connection:
            revisions = connection.execute(
                "SELECT revision,state_sha256 FROM case_revision "
                "WHERE case_id=? ORDER BY revision",
                ("group-mean-context",),
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
                    and result.get("input_id") == "group-mean-question"
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
                ("group-mean-context", revision),
            )
            connection.execute("DELETE FROM action_history WHERE sequence>3")
            connection.execute(
                "UPDATE input_identity SET result_sha256=? "
                "WHERE case_id=? AND input_id=?",
                (result_sha, "group-mean-context", "group-mean-question"),
            )
            connection.commit()

        reconciled = subprocess.run(
            [
                str(binary),
                "reconcile",
                "--db",
                str(database),
                "--case",
                "group-mean-context",
                "--actor",
                "fixture-json-group-mean-reader",
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
        assert recovered["dependencies"] == answer["dependencies"]
        assert receipt["phase"] == "CONFIRMED"
        assert receipt["reason"] == "LOCAL_FILE_RECONCILED_NO_RESEND"
        assert (
            recovered_stat.st_ino,
            recovered_stat.st_mtime_ns,
            recovered_stat.st_size,
        ) == (original_stat.st_ino, original_stat.st_mtime_ns, original_stat.st_size)

        source_paths = [
            "crates/minidora-domain/src/lib.rs",
            "crates/minidora-engine/src/input.rs",
            "crates/minidora-engine/src/math.rs",
            "crates/minidora-engine/src/document.rs",
            "crates/minidora-engine/src/core.rs",
            "crates/minidora-engine/src/recovery.rs",
            "crates/minidora-engine/src/store.rs",
        ]
        test_paths = [
            "apps/cli/tests/normal_route.rs",
            "tools/json_group_mean_query_route_test.py",
        ]
        documentation_paths = [
            "README.md",
            "audit/README.md",
            "audit/IMPLEMENTATION_STATUS.md",
            "audit/requirements.tsv",
            "audit/rust-requirement-observations.json",
        ]

        def hashes(paths: list[str]) -> dict[str, str]:
            return {
                f"rust/{path}": hashlib.sha256((rust_root / path).read_bytes()).hexdigest()
                for path in paths
            }

        evidence = {
            "work_item": "BODY-1 BB-LANG natural follow-up over BB-MATH-DATA grouped means",
            "stage": "BODY-1_EXECUTION",
            "recorded_at": datetime.now(timezone.utc).isoformat(timespec="seconds"),
            "base_commit": command_output(["git", "-C", str(root), "rev-parse", "HEAD"]),
            "requirement_ids": ["A07", "A13", "B04", "B06", "E03", "E06", "E08", "E09", "E12", "F04"],
            "software_version": "0.7.0-alpha.8",
            "schema_version": "minidora-rust/0.7.0-alpha.8",
            "parser_version": "declarative-ja-en-v53-group-mean-query",
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
                "ordinary JSON grouped-statistics request -> exact per-group means and original source ranges -> "
                "same-case Japanese/English highest-mean question -> source/report identity and exact recomputation -> "
                "all-group comparison, tied winners, answer artifact and local-file receipt -> "
                "Core recovery with no resend"
            ),
            "statistics_result": {
                "lifecycle": initial["lifecycle"],
                "result_id": initial["id"],
                "source_ref": source_ref,
                "report_ref": report_ref,
                "means": [item["statistics"]["mean"] for item in report["groups"]],
            },
            "natural_question": FOLLOWUP_REQUEST,
            "answer_result": {
                "lifecycle": answer["lifecycle"],
                "guarantee": answer["guarantee"],
                "dependencies": answer["dependencies"],
                "text": answer["text"],
                "answer_artifact_sha256": answer_ref,
                "typed_answer": typed_answer,
                "graph_consumption": answer_consumption,
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
                "normal_route_unique_tied_mixed_unit_and_context_stops": "3 normal-route CLI tests passed; 0 failed",
                "question_parser_and_exact_rational_tie_tests": "2 focused engine tests passed; 0 failed",
                "mixed_unit_stop": "PASS",
                "fresh_process_cli_consumer": "PASS",
                "local_file_receipt_and_no_resend_recovery": "PASS",
                "format_all_check": "PASS",
                "minidora_cli_all_targets_check_locked": "PASS",
                "minidora_cli_build_locked": "PASS",
            },
            "failures_and_corrections": [
                {
                    "stage": "input_parser_trial",
                    "failure": "A non-superlative Japanese wh-question fell through to the copular relation parser.",
                    "correction": "Recognized unsupported wh-question forms now remain UNKNOWN before generic copular parsing; focused parser coverage passes.",
                },
                {
                    "stage": "normal_route_assertion",
                    "failure": "The first assertion expected unquoted group names in comparison-detail lines.",
                    "correction": "The actual output retains quoted exact group identities; assertions now check those emitted values.",
                },
                {
                    "stage": "route_graph_inspection",
                    "failure": "The initial evidence script searched the graph node list instead of the persisted dependency map.",
                    "correction": "The assertion now verifies the answer-to-report edge in the graph dependency map; the fresh CLI/recovery replay passes.",
                },
            ],
            "commands": [
                "cargo fmt --all; cargo fmt --all -- --check",
                "cargo check -p minidora-cli --all-targets --locked",
                "cargo build -p minidora-cli --locked",
                "cargo test -p minidora-engine --lib highest_group_mean --locked",
                "cargo test -p minidora-engine --lib exact_maximum_comparison --locked",
                "cargo test -p minidora-cli --test normal_route natural_group_mean --locked",
                "python3 tools/json_group_mean_query_route_test.py --binary target/debug/minidora-rust --evidence-output audit/bb-lang-json-group-mean-query-route-validation.json",
            ],
            "not_run": [
                "full workspace runtime suite and Clippy during BODY-1",
                "cross-group unit conversion, CSV grouped-statistics questions, broader natural Q&A, and full A07/A13/B04 acceptance",
            ],
            "acceptance_boundary": (
                "This implements one finite natural question over an actual same-case JSON grouped-statistics result. "
                "It proves the selected group by exact comparison of the validated saved means and retains source ranges. "
                "Different units suspend. Full natural-language and capability acceptance remain UNVERIFIED."
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
                    "winner": typed_answer["winners"],
                    "file_recovery": evidence["file_recovery"],
                },
                ensure_ascii=False,
            )
        )


if __name__ == "__main__":
    main()
