#!/usr/bin/env python3
"""Exercise prior generated-code explanation and adopted-file recovery."""

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


PARSER_VERSION = "declarative-ja-en-v50-code-context-explanation"
DOCUMENT = {
    "仕様": {
        "名前": "増分",
        "引数": ["x"],
        "手順": [
            {
                "種別": "返却",
                "式": {
                    "種別": "算術",
                    "演算": "加算",
                    "左": {"種別": "参照", "名前": "x"},
                    "右": {"種別": "定数参照", "キー": "幅"},
                },
            }
        ],
    },
    "定数": {"幅": 1},
}


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--evidence-output", type=Path, required=True)
    arguments = parser.parse_args()
    repository_root = Path(__file__).resolve().parents[2]
    workspace_root = repository_root / "rust"
    cargo = shutil.which("cargo") or "/home/agent/.cargo/bin/cargo"
    rustc = shutil.which("rustc") or "/home/agent/.cargo/bin/rustc"

    def command_output(command: list[str]) -> str:
        return subprocess.run(
            command,
            capture_output=True,
            text=True,
            timeout=10,
            check=True,
        ).stdout.strip()

    def run(
        binary: Path,
        database: Path,
        actor: str,
        case: str,
        input_id: str,
        request: str,
        expected_exit: int = 0,
        extra: tuple[str, ...] = (),
    ) -> dict:
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
                *extra,
                request,
            ],
            capture_output=True,
            text=True,
            timeout=15,
            check=False,
        )
        if completed.returncode != expected_exit:
            raise AssertionError(
                f"{input_id}: exit={completed.returncode}; "
                f"stdout={completed.stdout}; stderr={completed.stderr}"
            )
        return json.loads(completed.stdout)

    with tempfile.TemporaryDirectory(prefix="minidora-code-context-") as temporary:
        directory = Path(temporary)
        database = directory / "state"
        actor = "fixture-code-context"
        case = "generated-code-explanation"
        binary = arguments.binary.resolve()
        request = "code-generate:\n" + json.dumps(
            DOCUMENT, ensure_ascii=False, separators=(",", ":")
        )
        generated = run(binary, database, actor, case, "generated-code", request)
        assert generated["lifecycle"] == "COMMIT"
        assert generated["guarantee"] == "COMPUTED"
        source_ref = generated["dependencies"][0]
        assert "def 増分(x, 定数):" in generated["text"]

        output_path = directory / "code-explanation.txt"
        delivered = run(
            binary,
            database,
            actor,
            case,
            "code-explanation-file",
            "詳しく説明して",
            extra=("--output", str(output_path), "--allow-write", str(output_path)),
        )
        expected_phrases = [
            "前回生成した関数「増分」",
            "引数: x、定数",
            "入力値「x」に定数辞書の「幅」を足した値を返します。",
            "Python構文解析=NOT_RUN, 実行=NOT_RUN, 入出力テスト=NOT_RUN",
        ]
        assert delivered["lifecycle"] == "COMMIT"
        assert delivered["guarantee"] == "COMPUTED"
        assert all(phrase in delivered["text"] for phrase in expected_phrases)
        assert generated["id"] in delivered["dependencies"]
        assert source_ref in delivered["dependencies"]
        assert output_path.read_bytes() == delivered["text"].encode("utf-8")
        original_stat = output_path.stat()

        with sqlite3.connect(database / "state.sqlite3") as connection:
            rows = connection.execute(
                "SELECT revision,state_sha256 FROM case_revision "
                "WHERE case_id=? ORDER BY revision",
                (case,),
            ).fetchall()
            adopted = None
            for revision_number, state_sha in rows:
                state = json.loads(
                    (database / "blobs" / state_sha).read_text(encoding="utf-8")
                )
                saved = state.get("result")
                if (
                    state["lifecycle"] == "ADOPTED"
                    and saved
                    and saved.get("input_id") == "code-explanation-file"
                    and saved.get("render") is not None
                ):
                    assert state["bundle"]["meaning"]["parser_version"] == PARSER_VERSION
                    assert state["bundle"]["meaning"]["context_result_ref"] == generated["id"]
                    adopted = (revision_number, saved)
            assert adopted is not None, "Core did not retain the adopted code explanation"
            revision_number, saved_result = adopted
            result_bytes = json.dumps(
                saved_result, ensure_ascii=False, sort_keys=True, separators=(",", ":")
            ).encode("utf-8")
            connection.executescript(
                "PRAGMA foreign_keys=ON; DROP TRIGGER case_revision_no_delete; "
                "DROP TRIGGER action_history_no_delete;"
            )
            connection.execute(
                "DELETE FROM case_revision WHERE case_id=? AND revision>?",
                (case, revision_number),
            )
            connection.execute("DELETE FROM action_history WHERE sequence>3")
            connection.execute(
                "UPDATE input_identity SET result_sha256=? "
                "WHERE case_id=? AND input_id=?",
                (
                    hashlib.sha256(result_bytes).hexdigest(),
                    case,
                    "code-explanation-file",
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
                case,
                "--actor",
                actor,
                "--json",
            ],
            capture_output=True,
            text=True,
            timeout=15,
            check=False,
        )
        if reconciled.returncode != 0:
            raise AssertionError(
                f"reconcile: exit={reconciled.returncode}; "
                f"stdout={reconciled.stdout}; stderr={reconciled.stderr}"
            )
        recovered = json.loads(reconciled.stdout)
        receipt = recovered["receipts"][-1]
        recovered_stat = output_path.stat()
        assert recovered["lifecycle"] == "COMMIT"
        assert recovered["dependencies"] == delivered["dependencies"]
        assert receipt["phase"] == "CONFIRMED"
        assert receipt["reason"] == "LOCAL_FILE_RECONCILED_NO_RESEND"
        assert (
            recovered_stat.st_ino,
            recovered_stat.st_mtime_ns,
            recovered_stat.st_size,
        ) == (original_stat.st_ino, original_stat.st_mtime_ns, original_stat.st_size)

        relative_paths = [
            "crates/minidora-engine/src/code_generation.rs",
            "crates/minidora-engine/src/core.rs",
            "crates/minidora-engine/src/recovery.rs",
            "crates/minidora-engine/src/store.rs",
            "crates/minidora-engine/src/input.rs",
            "apps/cli/tests/normal_route.rs",
            "tools/code_context_explanation_route_test.py",
        ]
        code_hashes = {
            f"rust/{path}": hashlib.sha256((workspace_root / path).read_bytes()).hexdigest()
            for path in relative_paths[:5]
        }
        test_hashes = {
            f"rust/{path}": hashlib.sha256((workspace_root / path).read_bytes()).hexdigest()
            for path in relative_paths[5:]
        }
        report = {
            "work_item": "BODY-1 bounded BB-CONTEXT/BB-CONTENT generated-code explanation",
            "stage": "BODY-1_EXECUTION",
            "recorded_at": datetime.now(timezone.utc).isoformat(timespec="seconds"),
            "base_commit": command_output(
                ["git", "-C", str(repository_root), "rev-parse", "HEAD"]
            ),
            "requirement_ids": ["A07", "A14", "B07", "F04", "E08", "E12"],
            "software_version": "0.7.0-alpha.8",
            "schema_version": "minidora-rust/0.7.0-alpha.8",
            "parser_version": PARSER_VERSION,
            "toolchain": {
                "rustc": command_output([rustc, "--version"]),
                "cargo": command_output([cargo, "--version"]),
                "platform": platform.platform(),
            },
            "source_snapshot_sha256": code_hashes,
            "test_and_tool_snapshot_sha256": test_hashes,
            "execution_mode": "fresh CLI process per request; temporary actor/case state",
            "producer_consumer": (
                "structured code request -> generated source plus immutable code report -> "
                "ExplainPrevious loads the same-case report -> IR-derived explanation -> "
                "content/render/local-file receipt -> Core recovery replay"
            ),
            "scope": (
                "Explains only code produced by the existing structured algorithm IR generator. "
                "Reconstructs each assignment, branch, loop, expression and return from its retained "
                "specification; links the prior result and report artifact. Python parsing, execution "
                "and input/output tests remain explicitly NOT_RUN."
            ),
            "generated_result": {
                "id": generated["id"],
                "source_sha256": hashlib.sha256(generated["text"].encode("utf-8")).hexdigest(),
                "report_artifact_ref": source_ref,
            },
            "explanation_result": {
                "lifecycle": delivered["lifecycle"],
                "guarantee": delivered["guarantee"],
                "text": delivered["text"],
                "dependencies": delivered["dependencies"],
                "local_file_sha256": hashlib.sha256(output_path.read_bytes()).hexdigest(),
            },
            "adopted_file_recovery": {
                "lifecycle": recovered["lifecycle"],
                "guarantee": recovered["guarantee"],
                "receipt_phase": receipt["phase"],
                "receipt_reason": receipt["reason"],
                "dependencies_unchanged": True,
                "file_inode_mtime_size_unchanged": True,
            },
            "outcome": "PASS",
            "commands": [
                "cargo fmt --all -- --check",
                "cargo check --workspace --all-targets --locked",
                "cargo test -p minidora-engine code_explanation_ --lib",
                "cargo test -p minidora-cli --test normal_route generated_python_is_explained_from_the_prior_ir_and_keeps_its_lineage",
                "python3 tools/code_context_explanation_route_test.py --binary target/debug/minidora-rust --evidence-output audit/bb-content-code-context-explanation-route-validation.json",
            ],
            "not_run": [
                "full workspace runtime suite, Clippy and full code-capability QA",
                "parsing or executing generated Python; verification is not claimed",
                "general free-form code explanation, revision and full B07/F04 acceptance",
            ],
            "acceptance_boundary": (
                "This closes one structured-IR code-generation-to-context-explanation route. "
                "General code understanding, execution/test validation and full BB-CONTENT/BB-CONTEXT acceptance remain UNVERIFIED."
            ),
        }
        arguments.evidence_output.parent.mkdir(parents=True, exist_ok=True)
        arguments.evidence_output.write_text(
            json.dumps(report, ensure_ascii=False, indent=2) + "\n", encoding="utf-8"
        )
        print(json.dumps(report, ensure_ascii=False, indent=2))


if __name__ == "__main__":
    main()
