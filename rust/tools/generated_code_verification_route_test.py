#!/usr/bin/env python3
"""Exercise prior generated-code finite verification and adopted-file recovery."""

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


PARSER_VERSION = "declarative-ja-en-v52-search-candidate-body"
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
CASES = {
    "試験": [
        {"引数": {"x": 3, "定数": {"幅": 1}}, "期待値": 4},
        {"引数": {"x": 0, "定数": {"幅": -2}}, "期待値": -2},
    ]
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
        extra: tuple[str, ...] = (),
        expected_exit: int = 0,
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

    with tempfile.TemporaryDirectory(prefix="minidora-generated-code-verification-") as temporary:
        directory = Path(temporary)
        database = directory / "state"
        actor = "fixture-generated-code-verifier"
        case = "generated-code-verification"
        binary = arguments.binary.resolve()

        generate_request = "code-generate:\n" + json.dumps(
            DOCUMENT, ensure_ascii=False, separators=(",", ":")
        )
        generated = run(
            binary, database, actor, case, "generated-code", generate_request
        )
        assert generated["lifecycle"] == "COMMIT"
        assert generated["guarantee"] == "COMPUTED"
        assert "def 増分(x, 定数):" in generated["text"]
        source_ref = generated["dependencies"][0]

        output_path = directory / "verification.txt"
        verification_request = "生成コードを検証して:\n" + json.dumps(
            CASES, ensure_ascii=False, separators=(",", ":")
        )
        verified = run(
            binary,
            database,
            actor,
            case,
            "verify-generated-code-file",
            verification_request,
            extra=("--output", str(output_path), "--allow-write", str(output_path)),
        )
        expected_phrases = [
            "前回生成した関数「増分」",
            "指定した有限ケース: 2件中2件一致",
            "試験1: 一致",
            "実際値=4",
            "試験2: 一致",
            "実際値=-2",
            "ホストPython、exec、eval",
        ]
        assert verified["lifecycle"] == "COMMIT"
        assert verified["guarantee"] == "COMPUTED"
        assert all(phrase in verified["text"] for phrase in expected_phrases)
        assert generated["id"] in verified["dependencies"]
        assert source_ref in verified["dependencies"]
        assert len(verified["dependencies"]) == 3
        assert output_path.read_bytes() == verified["text"].encode("utf-8")
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
                    and saved.get("input_id") == "verify-generated-code-file"
                    and saved.get("render") is not None
                ):
                    assert state["bundle"]["meaning"]["parser_version"] == PARSER_VERSION
                    assert state["bundle"]["meaning"]["intent"]["kind"] == (
                        "VERIFY_PREVIOUS_CODE"
                    )
                    assert state["bundle"]["meaning"]["context_result_ref"] == generated["id"]
                    adopted = (revision_number, saved)
            assert adopted is not None, "Core did not retain the adopted verification result"
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
                    "verify-generated-code-file",
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
        assert recovered["guarantee"] == verified["guarantee"]
        assert recovered["dependencies"] == verified["dependencies"]
        assert receipt["phase"] == "CONFIRMED"
        assert receipt["reason"] == "LOCAL_FILE_RECONCILED_NO_RESEND"
        assert (
            recovered_stat.st_ino,
            recovered_stat.st_mtime_ns,
            recovered_stat.st_size,
        ) == (original_stat.st_ino, original_stat.st_mtime_ns, original_stat.st_size)

        relative_paths = [
            "crates/minidora-domain/src/lib.rs",
            "crates/minidora-engine/src/code_capability.rs",
            "crates/minidora-engine/src/code_generation.rs",
            "crates/minidora-engine/src/core.rs",
            "crates/minidora-engine/src/input.rs",
            "crates/minidora-engine/src/recovery.rs",
            "crates/minidora-engine/src/store.rs",
            "apps/cli/tests/normal_route.rs",
            "tools/generated_code_verification_route_test.py",
        ]
        source_hashes = {
            f"rust/{path}": hashlib.sha256((workspace_root / path).read_bytes()).hexdigest()
            for path in relative_paths[:7]
        }
        test_hashes = {
            f"rust/{path}": hashlib.sha256((workspace_root / path).read_bytes()).hexdigest()
            for path in relative_paths[7:]
        }
        verification_report_ref = verified["dependencies"][2]
        report = {
            "work_item": "BODY-1 bounded BB-CONTENT generated-code verification consumer",
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
            "source_snapshot_sha256": source_hashes,
            "test_and_tool_snapshot_sha256": test_hashes,
            "execution_mode": "fresh CLI process per request; temporary actor/case state",
            "producer_consumer": (
                "structured code request -> committed source and immutable CodeGenerationReport -> "
                "ordinary generated-code verification request -> same-case report/source revalidation -> "
                "bounded native finite-case evaluator -> verification report -> content/render/local-file receipt"
            ),
            "scope": (
                "Only a just-prior committed structured function generation in the same actor/case is consumed. "
                "The user supplies finite JSON test cases. Core reparses the stored generated source with the "
                "existing restricted Python AST reader and evaluates that AST using Rust's bounded native "
                "interpreter; it does not invoke host Python, exec, eval, a compiler, or external effects. "
                "Passing finite examples do not establish general correctness or run Python input/output tests."
            ),
            "generated_result": {
                "id": generated["id"],
                "source_sha256": hashlib.sha256(generated["text"].encode("utf-8")).hexdigest(),
                "report_artifact_ref": source_ref,
            },
            "verification_result": {
                "lifecycle": verified["lifecycle"],
                "guarantee": verified["guarantee"],
                "dependencies": verified["dependencies"],
                "report_artifact_ref": verification_report_ref,
                "text": verified["text"],
                "finite_cases": CASES["試験"],
                "observed_cases": [
                    {"actual": 4, "expected": 4, "matched": True},
                    {"actual": -2, "expected": -2, "matched": True},
                ],
                "local_file_sha256": hashlib.sha256(output_path.read_bytes()).hexdigest(),
            },
            "adopted_file_recovery": {
                "lifecycle": recovered["lifecycle"],
                "guarantee": recovered["guarantee"],
                "receipt_phase": receipt["phase"],
                "receipt_reason": receipt["reason"],
                "dependencies_unchanged": True,
                "file_inode_mtime_size_unchanged": True,
                "no_write_resent": True,
            },
            "outcome": "PASS",
            "commands": [
                "cargo fmt --all -- --check",
                "cargo check --workspace --all-targets --locked",
                "cargo test -p minidora-engine generated_code_verification_headers_preserve_finite_case_document --lib --locked",
                "cargo test -p minidora-engine previous_generated_source_is_consumed_by_native_finite_verification --lib --locked",
                "cargo test -p minidora-cli --test normal_route generated_code_is_consumed_by_finite_native_verification_and_keeps_lineage --locked",
                "python3 tools/generated_code_verification_route_test.py --binary target/debug/minidora-rust --evidence-output audit/bb-content-generated-code-verification-v52-route-validation.json",
            ],
            "not_run": [
                "full workspace runtime test suite and Clippy",
                "host Python parsing/execution or generated-Python I/O tests",
                "general-purpose verification, hidden-input correctness, free-form code repair, and full B07/F04 acceptance",
            ],
            "acceptance_boundary": (
                "This closes one same-case producer-to-consumer route for structured generated Python source and "
                "user-authored finite cases. The evaluator handles a restricted Python subset; its output is "
                "not proof of general program correctness. Full BB-CONTENT, B07, A07, A14 and F04 remain UNVERIFIED."
            ),
        }
        arguments.evidence_output.parent.mkdir(parents=True, exist_ok=True)
        arguments.evidence_output.write_text(
            json.dumps(report, ensure_ascii=False, indent=2) + "\n", encoding="utf-8"
        )
        print(json.dumps(report, ensure_ascii=False, indent=2))


if __name__ == "__main__":
    main()
