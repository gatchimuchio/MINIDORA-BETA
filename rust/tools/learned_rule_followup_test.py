#!/usr/bin/env python3
"""Exercise declared-rule inference, contradiction and invalidation via CLI."""

import argparse
import hashlib
import json
from pathlib import Path
import re
import sqlite3
import subprocess
import tempfile


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--legacy-binary", type=Path)
    parser.add_argument("--evidence-output", type=Path)
    arguments = parser.parse_args()

    with tempfile.TemporaryDirectory(prefix="minidora-learned-rule-") as temporary:
        database = Path(temporary) / "state"
        case = "learned-rule-follow-up"
        actor = "fixture-learning"
        observations = []

        def run(input_id: str, request_text: str, expected_exit: int = 0) -> dict:
            completed = subprocess.run(
                [str(arguments.binary), "--db", str(database), "--case", case,
                 "--actor", actor, "--input", input_id, "--json", request_text],
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
            result = json.loads(completed.stdout)
            observations.append({
                "input_id": input_id,
                "request": request_text,
                "exit_code": completed.returncode,
                "lifecycle": result.get("lifecycle"),
                "guarantee": result.get("guarantee"),
                "text": result.get("text"),
                "reason": result.get("reason"),
                "dependencies": result.get("dependencies", []),
            })
            return result

        rule = run("rule", "もしXは鳥ならXは動物")
        assert rule["lifecycle"] == "COMMIT"
        assert rule["guarantee"] == "USER_PREMISE"
        rule_match = re.search(r"規則ID: (rule:[0-9a-f]{64})", rule["text"])
        assert rule_match, "Core did not return the retained rule identity"
        rule_id = rule_match.group(1)

        bird = run("new-observation", "ココは鳥です")
        assert bird["lifecycle"] == "COMMIT"
        assert bird["guarantee"] == "USER_PREMISE"
        fact_match = re.search(r"前提ID: (fact:[0-9a-f]{64})", bird["text"])
        assert fact_match, "Core did not return the retained observation identity"
        fact_id = fact_match.group(1)

        first_answer = run("first-unseen-question", "ココは動物ですか")
        assert first_answer["lifecycle"] == "COMMIT"
        assert first_answer["guarantee"] == "DERIVED"
        assert "ココは動物である" in first_answer["text"]
        assert rule_id in first_answer["dependencies"]
        assert fact_id in first_answer["dependencies"]
        assert any(
            dependency.startswith("fact:") and dependency != fact_id
            for dependency in first_answer["dependencies"]
        ), "Core did not retain the derived result identity"

        restarted_answer = run("restart-unseen-question", "ココは動物ですか")
        assert restarted_answer["lifecycle"] == "COMMIT"
        assert restarted_answer["guarantee"] == "DERIVED"
        assert restarted_answer["dependencies"] == first_answer["dependencies"]

        counterexample = run("counterexample", "ココは動物ではない")
        assert counterexample["lifecycle"] == "COMMIT"
        assert counterexample["guarantee"] == "USER_PREMISE"
        counterexample_match = re.search(
            r"前提ID: (fact:[0-9a-f]{64})", counterexample["text"]
        )
        assert counterexample_match
        counterexample_id = counterexample_match.group(1)

        contested = run("contested-question", "ココは動物ですか", expected_exit=2)
        assert contested["lifecycle"] == "SUSPEND"
        assert contested["reason"].startswith("CONFLICTING_PREMISES:")

        invalidated = run("invalidate-rule", f"{rule_id}を無効化して")
        assert invalidated["lifecycle"] == "COMMIT"
        assert rule_id in invalidated["dependencies"]

        remaining_answer = run("answer-after-invalidation", "ココは動物ですか")
        assert remaining_answer["lifecycle"] == "COMMIT"
        assert remaining_answer["guarantee"] == "USER_PREMISE"
        assert "ココは動物ではない" in remaining_answer["text"]
        assert remaining_answer["dependencies"] == [counterexample_id]

        legacy_recovery = None
        if arguments.legacy_binary:
            legacy_database = Path(temporary) / "legacy-state"
            legacy_case = "legacy-rule-follow-up"
            legacy_actor = "fixture-learning"

            def legacy_run(input_id: str, request_text: str) -> dict:
                completed = subprocess.run(
                    [str(arguments.legacy_binary), "--db", str(legacy_database),
                     "--case", legacy_case, "--actor", legacy_actor, "--input", input_id,
                     "--json", request_text],
                    capture_output=True,
                    text=True,
                    timeout=15,
                    check=False,
                )
                if completed.returncode != 0:
                    raise AssertionError(
                        f"legacy {input_id}: exit={completed.returncode}; "
                        f"stdout={completed.stdout}; stderr={completed.stderr}"
                    )
                return json.loads(completed.stdout)

            old_rule = legacy_run("rule", "もしXは鳥ならXは動物")
            old_rule_id = old_rule["dependencies"][0]
            legacy_run("new-observation", "ココは鳥です")
            legacy_run("counterexample", "ココは動物ではない")
            legacy_conflict = subprocess.run(
                [str(arguments.legacy_binary), "--db", str(legacy_database), "--case",
                 legacy_case, "--actor", legacy_actor, "--input", "conflict", "--json",
                 "ココは動物ですか"],
                capture_output=True,
                text=True,
                timeout=15,
                check=False,
            )
            assert legacy_conflict.returncode == 2
            legacy_run("invalidate-rule", f"{old_rule_id}を無効化して")
            legacy_output = Path(temporary) / "legacy-answer.txt"
            delivered = subprocess.run(
                [str(arguments.legacy_binary), "--db", str(legacy_database), "--case",
                 legacy_case, "--actor", legacy_actor, "--input", "legacy-answer",
                 "--json", "--output", str(legacy_output), "--allow-write",
                 str(legacy_output), "ココは動物ですか"],
                capture_output=True,
                text=True,
                timeout=15,
                check=False,
            )
            assert delivered.returncode == 0, (
                f"legacy file delivery: exit={delivered.returncode}; "
                f"stdout={delivered.stdout}; stderr={delivered.stderr}"
            )
            legacy_result = json.loads(delivered.stdout)
            assert legacy_result["lifecycle"] == "COMMIT"
            assert legacy_result["guarantee"] == "DERIVED"
            original_stat = legacy_output.stat()

            legacy_store = legacy_database
            connection = sqlite3.connect(legacy_store / "state.sqlite3")
            snapshots = connection.execute(
                "SELECT revision,state_sha256 FROM case_revision ORDER BY revision"
            ).fetchall()
            adopted = None
            for revision_number, state_sha in snapshots:
                state = json.loads(
                    (legacy_store / "blobs" / state_sha).read_text(encoding="utf-8")
                )
                saved = state.get("result")
                if (state["lifecycle"] == "ADOPTED" and saved
                        and saved.get("input_id") == "legacy-answer"
                        and saved.get("render") is not None):
                    adopted = (revision_number, saved, state["bundle"])
            assert adopted is not None, "legacy Core did not retain the adopted v45 result"
            revision_number, saved_result, saved_bundle = adopted
            assert saved_bundle["meaning"]["parser_version"] == (
                "declarative-ja-en-v45-previous-source-summary"
            )
            result_bytes = json.dumps(
                saved_result, ensure_ascii=False, sort_keys=True, separators=(",", ":")
            ).encode("utf-8")
            connection.executescript(
                "PRAGMA foreign_keys=ON; DROP TRIGGER case_revision_no_delete; "
                "DROP TRIGGER action_history_no_delete;"
            )
            connection.execute(
                "DELETE FROM case_revision WHERE revision>?", (revision_number,)
            )
            connection.execute("DELETE FROM action_history WHERE sequence>3")
            connection.execute(
                "UPDATE input_identity SET result_sha256=? WHERE case_id=? AND input_id=?",
                (hashlib.sha256(result_bytes).hexdigest(), legacy_case, "legacy-answer"),
            )
            connection.commit()
            connection.close()

            recovered_process = subprocess.run(
                [str(arguments.binary), "reconcile", "--db", str(legacy_database),
                 "--case", legacy_case, "--actor", legacy_actor, "--json"],
                capture_output=True,
                text=True,
                timeout=15,
                check=False,
            )
            assert recovered_process.returncode == 0, (
                f"legacy v45 recovery: exit={recovered_process.returncode}; "
                f"stdout={recovered_process.stdout}; stderr={recovered_process.stderr}"
            )
            recovered = json.loads(recovered_process.stdout)
            recovered_stat = legacy_output.stat()
            assert recovered["lifecycle"] == "COMMIT"
            assert recovered["receipts"][-1]["phase"] == "CONFIRMED"
            assert "NO_RESEND" in recovered["receipts"][-1]["reason"]
            assert (recovered_stat.st_ino, recovered_stat.st_mtime_ns, recovered_stat.st_size) == (
                original_stat.st_ino, original_stat.st_mtime_ns, original_stat.st_size
            )
            legacy_recovery = {
                "producer_parser_version": saved_bundle["meaning"]["parser_version"],
                "producer_guarantee": legacy_result["guarantee"],
                "reconciled_lifecycle": recovered["lifecycle"],
                "recovery_receipt": recovered["receipts"][-1]["reason"],
                "file_inode_mtime_size_unchanged": True,
            }

        report = {
            "work_item": "BB-LEARN bounded declared-rule follow-up",
            "parser_version": "declarative-ja-en-v46-proof-lineage-guarantee",
            "execution_mode": "fresh CLI process for each request; one durable actor/case store",
            "scope": "User-declared rules and premises only; no empirical rule induction or world-truth claim.",
            "rule_id": rule_id,
            "new_observation_id": fact_id,
            "counterexample_id": counterexample_id,
            "observations": observations,
            "legacy_v45_recovery": legacy_recovery,
            "outcome": "PASS",
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
