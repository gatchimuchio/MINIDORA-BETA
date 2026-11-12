#!/usr/bin/env python3
"""Exercise proof-grounded relation explanations and stale-context recovery."""

import argparse
import hashlib
import json
from pathlib import Path
import sqlite3
import subprocess
import tempfile


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--evidence-output", type=Path)
    arguments = parser.parse_args()

    with tempfile.TemporaryDirectory(prefix="minidora-related-explanation-") as temporary:
        database = Path(temporary) / "state"
        observations = []

        def run(
            actor: str,
            case: str,
            input_id: str,
            request: str,
            expected_exit: int = 0,
        ) -> dict:
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

        actor = "fixture-relation-reader"
        main_case = "relation-follow-up"
        rule = run(actor, main_case, "bird-rule", "もしXは鳥ならXは動物")
        bird = run(actor, main_case, "coco-bird", "ココは鳥")
        answer = run(actor, main_case, "coco-question", "ココは動物ですか")
        explanation = run(actor, main_case, "coco-explanation", "詳しく説明して")
        assert answer["guarantee"] == "DERIVED"
        assert explanation["lifecycle"] == "COMMIT"
        assert explanation["guarantee"] == "DERIVED"
        assert "前回の結論: ココは動物である" in explanation["text"]
        assert "直接提示された前提: ココは鳥である" in explanation["text"]
        assert "適用規則: $xは鳥である ならば $xは動物である" in explanation["text"]
        assert "現実世界での真偽確認ではありません" in explanation["text"]
        assert answer["id"] in explanation["dependencies"]
        assert rule["dependencies"][0] in explanation["dependencies"]
        assert bird["dependencies"][0] in explanation["dependencies"]

        stale_case = "stale-relation-follow-up"
        stale_answer = run(actor, stale_case, "stale-question", "ココは動物ですか")
        invalidated = run(
            actor,
            "rule-invalidation",
            "invalidate-bird-rule",
            f"{rule['dependencies'][0]}を無効化して",
        )
        assert invalidated["lifecycle"] == "COMMIT"
        stale = run(
            actor,
            stale_case,
            "stale-explanation",
            "詳しく説明して",
            expected_exit=2,
        )
        assert stale_answer["guarantee"] == "DERIVED"
        assert stale["lifecycle"] == "SUSPEND"
        assert stale["reason"].startswith("CONTEXT_PROOF_CHANGED:")

        negative = run(actor, main_case, "mike-negative", "ミケは鳥ではない")
        contradicted = run(actor, main_case, "mike-question", "ミケは鳥ですか")
        negative_explanation = run(actor, main_case, "mike-explanation", "詳しく説明して")
        assert contradicted["guarantee"] == "USER_PREMISE"
        assert "ミケは鳥ではない" in contradicted["text"]
        assert negative["dependencies"][0] in negative_explanation["dependencies"]
        assert "直接提示された前提: ミケは鳥ではない" in negative_explanation["text"]

        recovery_actor = "fixture-explanation-recovery"
        recovery_case = "explanation-file-recovery"
        recovery_rule = run(
            recovery_actor, recovery_case, "rule", "もしXは鳥ならXは動物"
        )
        recovery_bird = run(recovery_actor, recovery_case, "bird", "ココは鳥")
        recovery_answer = run(
            recovery_actor, recovery_case, "answer", "ココは動物ですか"
        )
        output_path = Path(temporary) / "explanation.txt"
        delivered = subprocess.run(
            [
                str(arguments.binary), "--db", str(database), "--case", recovery_case,
                "--actor", recovery_actor, "--input", "explanation-file", "--json",
                "--output", str(output_path), "--allow-write", str(output_path),
                "詳しく説明して",
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
        assert recovery_answer["id"] in delivered_result["dependencies"]
        assert recovery_rule["dependencies"][0] in delivered_result["dependencies"]
        assert recovery_bird["dependencies"][0] in delivered_result["dependencies"]
        observations.append(
            {
                "actor": recovery_actor,
                "case": recovery_case,
                "input_id": "explanation-file",
                "request": "詳しく説明して",
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
                    and saved.get("input_id") == "explanation-file"
                    and saved.get("render") is not None
                ):
                    adopted = (revision_number, saved)
            assert adopted is not None, "Core did not retain the adopted explanation result"
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
                    "explanation-file",
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
        assert recovered["guarantee"] == "DERIVED"
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
            "work_item": "BODY-1 bounded BB-CONTEXT relation-answer explanation",
            "parser_version": "declarative-ja-en-v48-related-context-explanation",
            "execution_mode": "fresh CLI process for each request; actor-scoped memory; same-case prior result",
            "producer_consumer": (
                "committed relation answer -> exact conclusion recovery -> active proof replay -> "
                "premise/rule explanation -> content/render/result lineage"
            ),
            "scope": (
                "Re-explains supported copular relation answers only when the same current proof, "
                "dependencies and guarantee still hold. Shows direct premise and applicable rule IDs; "
                "a changed proof suspends rather than explaining stale support."
            ),
            "observations": observations,
            "adopted_file_recovery": {
                "lifecycle": recovered["lifecycle"],
                "guarantee": recovered["guarantee"],
                "receipt_phase": receipt["phase"],
                "receipt_reason": receipt["reason"],
                "dependencies_unchanged": True,
                "file_inode_mtime_size_unchanged": True,
            },
            "outcome": "PASS",
            "acceptance_boundary": (
                "This is a bounded relation-answer context route. General explanation, paraphrase, "
                "examples, broad topic continuity and complete BB-CONTEXT/A07/A14 acceptance remain BUILD_GAP/UNVERIFIED."
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
