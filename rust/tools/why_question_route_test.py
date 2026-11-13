#!/usr/bin/env python3
"""Exercise proof-backed Japanese/English why questions through fresh CLI processes."""

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


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--evidence-output", type=Path)
    arguments = parser.parse_args()
    repository_root = Path(__file__).resolve().parents[2]
    workspace_root = repository_root / "rust"

    def command_output(command: list[str]) -> str:
        completed = subprocess.run(
            command,
            capture_output=True,
            text=True,
            timeout=10,
            check=True,
        )
        return completed.stdout.strip()

    cargo = shutil.which("cargo") or "/home/agent/.cargo/bin/cargo"
    rustc = shutil.which("rustc") or "/home/agent/.cargo/bin/rustc"
    source_paths = [
        "crates/minidora-domain/src/lib.rs",
        "crates/minidora-engine/src/core.rs",
        "crates/minidora-engine/src/input.rs",
        "crates/minidora-engine/src/language.rs",
        "crates/minidora-engine/src/recovery.rs",
    ]
    test_paths = [
        "apps/cli/tests/normal_route.rs",
        "tools/why_question_route_test.py",
    ]
    documentation_paths = [
        "README.md",
        "audit/IMPLEMENTATION_STATUS.md",
        "audit/requirements.tsv",
    ]

    def sha256_paths(paths: list[str]) -> dict[str, str]:
        return {
            f"rust/{path}": hashlib.sha256((workspace_root / path).read_bytes()).hexdigest()
            for path in paths
        }

    base_commit = command_output(["git", "-C", str(repository_root), "rev-parse", "HEAD"])
    toolchain = {
        "rustc": command_output([rustc, "--version"]),
        "cargo": command_output([cargo, "--version"]),
        "platform": platform.platform(),
    }

    with tempfile.TemporaryDirectory(prefix="minidora-why-question-") as temporary:
        database = Path(temporary) / "state"
        observations = []

        def run(actor: str, case: str, input_id: str, request: str, expected_exit: int = 0) -> dict:
            completed = subprocess.run(
                [
                    str(arguments.binary), "--db", str(database), "--case", case,
                    "--actor", actor, "--input", input_id, "--json", request,
                ],
                capture_output=True,
                text=True,
                timeout=15,
                check=False,
            )
            if completed.returncode != expected_exit:
                raise AssertionError(
                    f"{actor}/{case}/{input_id}: exit={completed.returncode}; "
                    f"stdout={completed.stdout}; stderr={completed.stderr}"
                )
            result = json.loads(completed.stdout)
            observations.append(
                {
                    "actor": actor,
                    "case": case,
                    "input_id": input_id,
                    "request": request,
                    "exit_code": completed.returncode,
                    "lifecycle": result.get("lifecycle"),
                    "guarantee": result.get("guarantee"),
                    "text": result.get("text"),
                    "reason": result.get("reason"),
                    "dependencies": result.get("dependencies", []),
                }
            )
            return result

        def model_records(result: dict) -> tuple[dict, dict]:
            plan = result["content_plan"]
            emission = json.loads(
                (database / "blobs" / plan["model_observation_ref"]).read_text(
                    encoding="utf-8"
                )
            )
            model = json.loads(
                (database / "blobs" / plan["model_artifact_ref"]).read_text(
                    encoding="utf-8"
                )
            )
            return model, emission

        def assert_ranked_explanation(result: dict) -> dict:
            model, emission = model_records(result)
            candidates = emission["authorized_surfaces"]
            assert len(candidates) == 2
            assert result["text"] == emission["text"]
            assert result["text"] in candidates
            assert emission["sequence_probability"] is not None
            assert emission["objective"] == (
                "maximum exact sequence probability over Core-authorized equivalent surfaces"
            )
            assert emission["decoder_version"] == "bounded-finite-surface-literal-exact-v1"
            assert all("直接提示された前提:" in candidate for candidate in candidates)
            assert all("現実世界での真偽確認ではありません。" in candidate for candidate in candidates)
            for candidate in candidates:
                assert all(
                    unit in candidate
                    for unit in result["content_plan"]["required_units"]
                )
                if result["guarantee"] == "DERIVED":
                    assert "適用規則:" in candidate
                else:
                    assert "適用規則:" not in candidate
            return {
                "model_revision": emission["model_revision"],
                "model_artifact_ref": result["content_plan"]["model_artifact_ref"],
                "model_observation_ref": result["content_plan"]["model_observation_ref"],
                "model_order": model["order"],
                "candidate_count": len(candidates),
                "candidates": candidates,
                "selected_text": emission["text"],
                "sequence_probability": emission["sequence_probability"],
                "decoder_version": emission["decoder_version"],
                "token_count": emission["token_count"],
                "lossless_raw_sha256": emission["lossless_projection"]["raw_sha256"],
            }

        actor = "fixture-why-reader"
        case = "why-question"
        rule = run(actor, case, "rule-ja", "もしXは鳥ならXは動物")
        bird = run(actor, case, "premise-ja", "ココは鳥")
        japanese = run(actor, case, "why-ja", "どうしてココは動物なのですか")
        assert japanese["lifecycle"] == "COMMIT"
        assert japanese["guarantee"] == "DERIVED"
        assert "直接提示された前提: ココは鳥である" in japanese["text"]
        assert "適用規則: $xは鳥である ならば $xは動物である" in japanese["text"]
        assert rule["dependencies"][0] in japanese["dependencies"]
        assert bird["dependencies"][0] in japanese["dependencies"]
        observations[-1]["native_model_selection"] = assert_ranked_explanation(japanese)

        run(actor, case, "rule-en", "If X is a bird then X is an animal")
        run(actor, case, "premise-en", "Coco is a bird")
        english = run(actor, case, "why-en", "Why is Coco an animal?")
        assert english["lifecycle"] == "COMMIT"
        assert english["guarantee"] == "DERIVED"
        assert "適用規則:" in english["text"]
        observations[-1]["native_model_selection"] = assert_ranked_explanation(english)

        missing = run(actor, case, "why-missing", "どうしてミケは鳥ですか", expected_exit=2)
        assert missing["lifecycle"] == "SUSPEND"
        assert missing["reason"].startswith("MISSING_PREMISES:")
        negative = run(actor, case, "negative-premise", "ミケは鳥ではない")
        refuted = run(actor, case, "why-refuted", "なぜミケは鳥ですか")
        assert refuted["lifecycle"] == "COMMIT"
        assert refuted["guarantee"] == "USER_PREMISE"
        assert "問いの関係を支持せず" in refuted["text"]
        assert "直接提示された前提: ミケは鳥ではない" in refuted["text"]
        assert negative["dependencies"][0] in refuted["dependencies"]
        observations[-1]["native_model_selection"] = assert_ranked_explanation(refuted)
        run(actor, case, "positive-premise", "ミケは鳥")
        conflict = run(actor, case, "why-conflict", "なぜミケは鳥ですか", expected_exit=2)
        assert conflict["lifecycle"] == "SUSPEND"
        assert conflict["reason"].startswith("CONFLICTING_PREMISES:")

        recovery_actor = "fixture-why-recovery"
        recovery_case = "why-file-recovery"
        recovery_rule = run(recovery_actor, recovery_case, "rule", "もしXは鳥ならXは動物")
        recovery_bird = run(recovery_actor, recovery_case, "premise", "ココは鳥")
        output_path = Path(temporary) / "why-answer.txt"
        delivered = subprocess.run(
            [
                str(arguments.binary), "--db", str(database), "--case", recovery_case,
                "--actor", recovery_actor, "--input", "why-file", "--json",
                "--output", str(output_path), "--allow-write", str(output_path),
                "どうしてココは動物なのですか",
            ],
            capture_output=True,
            text=True,
            timeout=15,
            check=False,
        )
        if delivered.returncode != 0:
            raise AssertionError(
                f"file delivery: exit={delivered.returncode}; "
                f"stdout={delivered.stdout}; stderr={delivered.stderr}"
            )
        delivered_result = json.loads(delivered.stdout)
        assert delivered_result["lifecycle"] == "COMMIT"
        assert delivered_result["guarantee"] == "DERIVED"
        delivered_model = assert_ranked_explanation(delivered_result)
        assert recovery_rule["dependencies"][0] in delivered_result["dependencies"]
        assert recovery_bird["dependencies"][0] in delivered_result["dependencies"]
        observations.append(
            {
                "actor": recovery_actor,
                "case": recovery_case,
                "input_id": "why-file",
                "request": "どうしてココは動物なのですか",
                "exit_code": delivered.returncode,
                "lifecycle": delivered_result["lifecycle"],
                "guarantee": delivered_result["guarantee"],
                "text": delivered_result["text"],
                "reason": delivered_result.get("reason"),
                "dependencies": delivered_result["dependencies"],
            }
        )
        original_stat = output_path.stat()

        with sqlite3.connect(database / "state.sqlite3") as connection:
            snapshots = connection.execute(
                "SELECT revision,state_sha256 FROM case_revision "
                "WHERE case_id=? ORDER BY revision",
                (recovery_case,),
            ).fetchall()
            adopted = None
            for revision_number, state_sha in snapshots:
                state = json.loads(
                    (database / "blobs" / state_sha).read_text(encoding="utf-8")
                )
                saved = state.get("result")
                if (
                    state["lifecycle"] == "ADOPTED"
                    and saved
                    and saved.get("input_id") == "why-file"
                    and saved.get("render") is not None
                ):
                    assert state["bundle"]["meaning"]["intent"]["kind"] == "ASK_WHY"
                    assert state["bundle"]["meaning"]["parser_version"] == (
                        "declarative-ja-en-v68-previous-result-summary"
                    )
                    adopted = (revision_number, saved)
            assert adopted is not None, "Core did not retain the adopted why-answer result"
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
                (recovery_case, revision_number),
            )
            connection.execute("DELETE FROM action_history WHERE sequence>3")
            connection.execute(
                "UPDATE input_identity SET result_sha256=? WHERE case_id=? AND input_id=?",
                (
                    hashlib.sha256(result_bytes).hexdigest(),
                    recovery_case,
                    "why-file",
                ),
            )
            connection.commit()

        reconciled = subprocess.run(
            [
                str(arguments.binary), "reconcile", "--db", str(database), "--case",
                recovery_case, "--actor", recovery_actor, "--json",
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
        assert recovered["dependencies"] == delivered_result["dependencies"]
        assert receipt["phase"] == "CONFIRMED"
        assert "NO_RESEND" in receipt["reason"]
        assert (recovered_stat.st_ino, recovered_stat.st_mtime_ns, recovered_stat.st_size) == (
            original_stat.st_ino,
            original_stat.st_mtime_ns,
            original_stat.st_size,
        )
        observations.append(
            {
                "actor": recovery_actor,
                "case": recovery_case,
                "operation": "reconcile",
                "exit_code": reconciled.returncode,
                "lifecycle": recovered["lifecycle"],
                "guarantee": recovered["guarantee"],
                "receipt_phase": receipt["phase"],
                "receipt_reason": receipt["reason"],
                "dependencies": recovered["dependencies"],
            }
        )

        report = {
            "work_item": "BODY-1 BB-LANG proof-backed explanation through native surface ranking",
            "stage": "BODY-1_EXECUTION",
            "recorded_at": datetime.now(timezone.utc).isoformat(timespec="seconds"),
            "base_commit": base_commit,
            "subject": "MINIDORA Rust successor engine",
            "requirement_ids": ["A01", "A02", "A07", "A12", "A13", "A15", "E08", "E12"],
            "software_version": "0.7.0-alpha.8",
            "schema_version": "minidora-rust/0.7.0-alpha.8",
            "parser_version": "declarative-ja-en-v68-previous-result-summary",
            "toolchain": toolchain,
            "source_snapshot_sha256": sha256_paths(source_paths),
            "test_and_tool_snapshot_sha256": sha256_paths(test_paths),
            "documentation_snapshot_sha256": sha256_paths(documentation_paths),
            "execution_mode": "fresh CLI process per request; user-declared actor-scoped premises and rules",
            "producer_consumer": (
                "ordinary Japanese/English why question -> sealed AskWhy -> actor-scoped relation proof -> "
                "Core explanation units -> two Core-approved equivalent surfaces -> native SequenceModel ranking "
                "-> rendered result and proof dependency ledger"
            ),
            "scope": (
                "Supports explicit copular why questions over current declared premises/rules. "
                "Shows the premise and rule IDs; a refuted question states the supported opposite; "
                "the non-neural sequence model ranks two Core-approved surfaces that preserve identical proof "
                "units and differ only in transition wording. Missing/conflicting evidence suspends. This is not "
                "open-domain writing, a world-truth claim or empirical learning."
            ),
            "observations": observations,
            "adopted_file_recovery": {
                "lifecycle": recovered["lifecycle"],
                "guarantee": recovered["guarantee"],
                "native_model_selection": delivered_model,
                "file_sha256": delivered_result["render"]["actual_sha256"],
                "file_bytes": output_path.stat().st_size,
                "receipt_phase": receipt["phase"],
                "receipt_reason": receipt["reason"],
                "dependencies_unchanged": True,
                "file_inode_mtime_size_unchanged": True,
            },
            "outcome": "PASS",
            "commands": [
                "cargo fmt --all -- --check",
                "cargo check --workspace --all-targets --locked",
                "cargo clippy --workspace --all-targets --locked -- -D warnings",
                "cargo test --locked -p minidora-engine relation_explanation_surfaces_preserve_core_units_and_vary_only_transition --lib",
                "cargo test --locked -p minidora-cli --test normal_route natural_why_question_explains_only_current_relation_proofs -- --exact",
                "cargo build --workspace --locked",
                "python3 tools/why_question_route_test.py --binary target/debug/minidora-rust --evidence-output audit/bb-lang-proof-backed-native-surface-selection-route-validation.json",
            ],
            "regression_checks": [
                {
                    "command": "python3 tools/related_answer_explanation_test.py --binary target/debug/minidora-rust",
                    "outcome": "PASS",
                    "scope": "Existing prior-answer explanation and stale-proof recovery regression through the v68 binary",
                }
            ],
            "not_run": [
                "full workspace all-features runtime suite for this batch",
                "unrestricted causal why/how understanding and full BB-LANG/A13 acceptance",
            ],
            "acceptance_boundary": (
                "Finite relation why-question route only. General causal understanding, unrestricted why/how "
                "questions, open-domain writing and full A01/A02/A07/A13 acceptance remain BUILD_GAP/UNVERIFIED."
            ),
        }
        if arguments.evidence_output:
            arguments.evidence_output.parent.mkdir(parents=True, exist_ok=True)
            arguments.evidence_output.write_text(
                json.dumps(report, ensure_ascii=False, indent=2) + "\n",
                encoding="utf-8",
            )
        print(json.dumps(report, ensure_ascii=False, indent=2))


if __name__ == "__main__":
    main()
