#!/usr/bin/env python3
"""Exercise premise listing, scope, provenance and invalidation via fresh CLI processes."""

import argparse
import hashlib
import json
from pathlib import Path
import sqlite3
import subprocess
import tempfile

PARSER_VERSION = "declarative-ja-en-v68-previous-result-summary"


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--evidence-output", type=Path)
    arguments = parser.parse_args()

    with tempfile.TemporaryDirectory(prefix="minidora-premise-inspection-") as temporary:
        database = Path(temporary) / "state"
        case = "premise-inspection"
        actor = "fixture-premise-reader"
        observations = []

        def run(
            input_id: str,
            request: str,
            expected_exit: int = 0,
            run_actor: str = actor,
            run_case: str = case,
            extra_args: tuple[str, ...] = (),
        ) -> dict:
            completed = subprocess.run(
                [
                    str(arguments.binary), "--db", str(database), "--case", run_case,
                    "--actor", run_actor, "--input", input_id, "--json",
                    *extra_args, request,
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
            result = json.loads(completed.stdout)
            observations.append(
                {
                    "input_id": input_id,
                    "request": request,
                    "request_sha256": hashlib.sha256(request.encode("utf-8")).hexdigest(),
                    "actor": run_actor,
                    "case": run_case,
                    "exit_code": completed.returncode,
                    "lifecycle": result.get("lifecycle"),
                    "guarantee": result.get("guarantee"),
                    "text": result.get("text"),
                    "reason": result.get("reason"),
                    "dependencies": result.get("dependencies", []),
                }
            )
            return result

        bird = run("coco-bird", "ココは鳥")
        gray = run("coco-gray", "[scope=story]ココは灰色")
        negative = run("coco-negative", "ココは哺乳類ではない")
        negative_animal = run("coco-negative-animal", "ココは動物ではない")
        rule = run("bird-rule", "もしXは鳥ならXは動物")
        listed = run("coco-listing", "ココについて、何が分かっていますか")
        assert listed["lifecycle"] == "COMMIT"
        assert listed["guarantee"] == "DERIVED"
        assert "現実世界の真偽や全件網羅性を確認したものではありません" in listed["text"]
        assert "規則導出" in listed["text"]
        assert "導出ID:" in listed["text"]
        assert "対立根拠として" in listed["text"]
        assert "ココは鳥である" in listed["text"]
        assert "ココは哺乳類ではない" in listed["text"]
        assert "ココは動物である" in listed["text"]
        assert "ココは動物ではない" in listed["text"]
        assert "範囲: story" in listed["text"]
        assert len(listed["dependencies"]) == 6
        for source in (bird, gray, negative, negative_animal):
            assert source["dependencies"][0] in listed["dependencies"]
        assert rule["dependencies"][0] in listed["dependencies"]
        derived_line = next(line for line in listed["text"].splitlines() if "導出ID: " in line)
        derived_id = derived_line.split("導出ID: ", 1)[1].split(";", 1)[0]
        assert derived_id in listed["dependencies"]

        english_premise = run("coco-mammal-en", "Coco is a mammal")
        english_listing = run("coco-listing-en", "What is Coco?")
        assert english_listing["lifecycle"] == "COMMIT"
        assert english_listing["guarantee"] == "USER_PREMISE"
        assert "Cocoはmammalである" in english_listing["text"]
        assert "現実世界の真偽や全件網羅性を確認したものではありません" in english_listing["text"]
        assert english_premise["dependencies"][0] in english_listing["dependencies"]
        assert all(
            source["dependencies"][0] not in english_listing["dependencies"]
            for source in (bird, gray, negative, negative_animal)
        )

        bird_id = bird["dependencies"][0]
        invalidated = run("invalidate-coco-bird", f"{bird_id}を無効化して")
        assert bird_id in invalidated["dependencies"]
        after_invalidation = run(
            "coco-listing-after", "ココとは何ですか"
        )
        assert after_invalidation["lifecycle"] == "COMMIT"
        assert after_invalidation["guarantee"] == "USER_PREMISE"
        assert len(after_invalidation["dependencies"]) == 3
        assert bird_id not in after_invalidation["dependencies"]
        assert "ココは鳥である" not in after_invalidation["text"]
        assert "ココは動物ではない" in after_invalidation["text"]
        assert "規則導出" not in after_invalidation["text"]

        other_actor = run(
            "other-actor-listing",
            "ココについて、何が分かっていますか",
            expected_exit=2,
            run_actor="separate-premise-reader",
            run_case="separate-actor-case",
        )
        assert other_actor["lifecycle"] == "SUSPEND"
        assert other_actor["reason"].startswith("MISSING_PREMISES:")

        candidate_budget = run(
            "candidate-budget",
            "ココについて、何が分かっていますか",
            expected_exit=2,
            extra_args=("--max-actions", "6"),
        )
        assert candidate_budget["lifecycle"] == "SUSPEND"
        assert candidate_budget["reason"].startswith("RESOURCE_LIMIT:")

        missing = run(
            "unknown-subject-listing",
            "What is a unicorn?",
            expected_exit=2,
        )
        assert missing["lifecycle"] == "SUSPEND"
        assert missing["reason"].startswith("MISSING_PREMISES:")

        recovery_case = "premise-inspection-recovery"
        restored_bird = run(
            "coco-bird-restored",
            "ココは鳥",
            run_case=recovery_case,
        )
        assert restored_bird["lifecycle"] == "COMMIT"
        output_path = Path(temporary) / "premise-listing.txt"
        delivered = subprocess.run(
            [
                str(arguments.binary), "--db", str(database), "--case", recovery_case,
                "--actor", actor, "--input", "recovery-listing-file", "--json",
                "--output", str(output_path), "--allow-write", str(output_path),
                "ココについて、何が分かっていますか",
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
        assert "規則導出" in delivered_result["text"]
        assert "対立根拠として" in delivered_result["text"]
        assert len(delivered_result["dependencies"]) == 6
        assert output_path.is_file()
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
                    and saved.get("input_id") == "recovery-listing-file"
                    and saved.get("render") is not None
                ):
                    adopted = (revision_number, saved)
            assert adopted is not None, "Core did not retain the adopted premise-listing result"
            revision_number, saved_result = adopted
            result_bytes = json.dumps(
                saved_result,
                ensure_ascii=False,
                sort_keys=True,
                separators=(",", ":"),
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
                    "recovery-listing-file",
                ),
            )
            connection.commit()

        reconciled = subprocess.run(
            [
                str(arguments.binary), "reconcile", "--db", str(database), "--case",
                recovery_case, "--actor", actor, "--json",
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

        report = {
            "work_item": "BODY-1 BB-LANG bounded derived-relation inventory",
            "parser_version": PARSER_VERSION,
            "requirement_ids": ["A07", "A08", "A12", "A13"],
            "execution_mode": "fresh CLI process for each request; one durable actor/case store",
            "producer_consumer": (
                "ordinary Japanese/English subject question -> sealed v68 InspectPremises intent -> "
                "actor-scoped relation facts/rules -> bounded Core proof queries -> content/render/result dependencies"
            ),
            "scope": (
                "Lists supported active direct relations and rule-derived conclusions for the named subject, "
                "preserving guarantee, scope, derived result IDs and transitive premise/rule dependencies. "
                "Conflicting support is rendered without arbitrary resolution. The listing is bounded by "
                "candidate/action/proof/evidence budgets, is not exhaustive and does not verify outside-world truth."
            ),
            "observations": observations,
            "adopted_file_recovery": {
                "lifecycle": recovered["lifecycle"],
                "guarantee": recovered["guarantee"],
                "output_guarantee": delivered_result["guarantee"],
                "derived_listing_present": "規則導出" in delivered_result["text"],
                "conflict_rendered": "対立根拠として" in delivered_result["text"],
                "dependencies": delivered_result["dependencies"],
                "file_sha256": delivered_result["render"]["actual_sha256"],
                "file_bytes": output_path.stat().st_size,
                "receipt_phase": receipt["phase"],
                "receipt_reason": receipt["reason"],
                "dependencies_unchanged": True,
                "file_inode_mtime_size_unchanged": True,
            },
            "outcome": "PASS",
            "acceptance_boundary": (
                "This validates a bounded actor-memory inventory route only. It does not provide open-world "
                "definitions or general what/why/how answers. Broader A07/A08/A12/A13 and BB-LANG acceptance "
                "remain BUILD_GAP/UNVERIFIED."
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
