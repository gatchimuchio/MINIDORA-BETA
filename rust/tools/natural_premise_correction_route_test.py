#!/usr/bin/env python3
"""Exercise exact natural premise correction through fresh CLI processes."""

import argparse
import hashlib
import json
from pathlib import Path
import sqlite3
import subprocess
import tempfile

PARSER_VERSION = "declarative-ja-en-v56-natural-premise-correction"


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--evidence-output", type=Path)
    arguments = parser.parse_args()

    with tempfile.TemporaryDirectory(prefix="minidora-premise-correction-") as temporary:
        database = Path(temporary) / "state"
        case = "natural-premise-correction"
        actor = "fixture-premise-owner"
        observations = []

        def run(
            input_id: str,
            request: str,
            expected_exit: int = 0,
            *,
            request_actor: str = actor,
            request_case: str = case,
        ) -> dict:
            completed = subprocess.run(
                [
                    str(arguments.binary),
                    "--db",
                    str(database),
                    "--case",
                    request_case,
                    "--actor",
                    request_actor,
                    "--input",
                    input_id,
                    "--json",
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
            result = json.loads(completed.stdout)
            observations.append(
                {
                    "input_id": input_id,
                    "actor_id": request_actor,
                    "case_id": request_case,
                    "request": request,
                    "input_sha256": hashlib.sha256(request.encode("utf-8")).hexdigest(),
                    "exit_code": completed.returncode,
                    "lifecycle": result.get("lifecycle"),
                    "guarantee": result.get("guarantee"),
                    "text": result.get("text"),
                    "text_sha256": hashlib.sha256(
                        (result.get("text") or "").encode("utf-8")
                    ).hexdigest(),
                    "reason": result.get("reason"),
                    "result_id": result.get("id"),
                    "content_plan_id": (result.get("content_plan") or {}).get("id"),
                    "render_sha256": (result.get("render") or {}).get("actual_sha256"),
                    "dependencies": result.get("dependencies", []),
                }
            )
            return result

        old_default = run("coco-mammal-default", "ココは哺乳類")
        old_story = run("coco-mammal-story", "[scope=story]ココは哺乳類")
        gray = run("coco-gray", "ココは灰色")
        rule = run("mammal-implies-animal", "もしXは哺乳類ならXは動物")
        derived = run("coco-animal-before-correction", "ココは動物ですか")
        assert derived["guarantee"] == "DERIVED"

        old_default_id = old_default["dependencies"][0]
        rule_id = rule["dependencies"][0]
        assert old_default_id in derived["dependencies"]
        assert rule_id in derived["dependencies"]

        correction = run(
            "coco-correction-ja",
            "訂正: ココは哺乳類ではなく鳥です",
        )
        assert correction["lifecycle"] == "COMMIT"
        assert correction["guarantee"] == "USER_PREMISE"
        assert "現実世界の真偽を検証したものではありません" in correction["text"]
        assert old_default_id in correction["dependencies"]
        assert rule_id not in correction["dependencies"]
        assert old_story["dependencies"][0] not in correction["dependencies"]
        assert gray["dependencies"][0] not in correction["dependencies"]

        with sqlite3.connect(database / "state.sqlite3") as connection:
            memory_sha = connection.execute(
                "SELECT record_sha256 FROM memory_revision WHERE actor_id=? ORDER BY revision DESC LIMIT 1",
                (actor,),
            ).fetchone()[0]
        memory = json.loads(
            (database / "blobs" / memory_sha).read_text(encoding="utf-8")
        )
        facts = {fact["id"]: fact for fact in memory["facts"]}
        assert facts[old_default_id]["valid"] is False
        assert facts[old_story["dependencies"][0]]["valid"] is True
        assert facts[gray["dependencies"][0]]["valid"] is True
        assert any(
            fact["valid"]
            and fact["proposition"]["subject"] == "ココ"
            and fact["proposition"]["object"] == "哺乳類"
            and fact["proposition"]["scope"] == "default"
            and fact["proposition"]["negative"] is True
            for fact in memory["facts"]
        )
        assert any(
            fact["valid"]
            and fact["proposition"]["subject"] == "ココ"
            and fact["proposition"]["object"] == "鳥"
            and fact["proposition"]["scope"] == "default"
            and fact["proposition"]["negative"] is False
            for fact in memory["facts"]
        )

        with sqlite3.connect(database / "state.sqlite3") as connection:
            revisions = connection.execute(
                "SELECT state_sha256 FROM case_revision WHERE case_id=? ORDER BY revision",
                (case,),
            ).fetchall()
        historical_derived = None
        for (state_sha,) in revisions:
            state = json.loads(
                (database / "blobs" / state_sha).read_text(encoding="utf-8")
            )
            saved_result = state.get("result")
            if (
                saved_result
                and saved_result.get("input_id") == "coco-animal-before-correction"
            ):
                historical_derived = saved_result
        assert historical_derived == derived, "the original derived result was rewritten"

        negative_answer = run("coco-is-mammal-after-correction", "ココは哺乳類ですか")
        assert negative_answer["lifecycle"] == "COMMIT"
        assert negative_answer["guarantee"] == "USER_PREMISE"
        assert "ココは哺乳類ではない" in negative_answer["text"]

        animal_answer = run(
            "coco-is-animal-after-correction",
            "ココは動物ですか",
            expected_exit=2,
        )
        assert animal_answer["lifecycle"] == "SUSPEND"
        assert animal_answer["reason"].startswith("MISSING_PREMISES:")

        listing = run("coco-after-correction", "ココとは何ですか")
        assert listing["lifecycle"] == "COMMIT"
        assert listing["guarantee"] == "USER_PREMISE"
        assert "ココは鳥である" in listing["text"]
        assert "ココは哺乳類ではない" in listing["text"]
        assert "ココは哺乳類である（範囲: story" in listing["text"]
        assert "ココは灰色である" in listing["text"]
        assert old_default_id not in listing["dependencies"]
        assert old_story["dependencies"][0] in listing["dependencies"]
        assert gray["dependencies"][0] in listing["dependencies"]

        wrong_scope = run(
            "coco-wrong-scope-correction",
            "ココは哺乳類ではなく魚です",
            expected_exit=2,
        )
        assert wrong_scope["lifecycle"] == "SUSPEND"
        assert wrong_scope["reason"].startswith("MISSING_CORRECTION_TARGET:")
        unmatched = run(
            "mike-missing-correction-target",
            "ミケは哺乳類ではなく猫です",
            expected_exit=2,
        )
        assert unmatched["lifecycle"] == "SUSPEND"
        assert unmatched["reason"].startswith("MISSING_CORRECTION_TARGET:")
        other_actor = run(
            "other-actor-correction-target",
            "ココは哺乳類ではなく鳥です",
            expected_exit=2,
            request_actor="separate-actor",
            request_case="separate-actor-correction-case",
        )
        assert other_actor["lifecycle"] == "SUSPEND"
        assert other_actor["reason"].startswith("MISSING_CORRECTION_TARGET:")
        unchanged = run("coco-after-rejected-corrections", "ココとは何ですか")
        assert unchanged["dependencies"] == listing["dependencies"]

        english_case = "natural-premise-correction-english"
        english_old = run(
            "coco-mammal-en",
            "Coco is a mammal",
            request_case=english_case,
        )
        english_correction = run(
            "coco-correction-en",
            "Correction: Coco is a bird, not a mammal.",
            request_case=english_case,
        )
        english_listing = run(
            "coco-after-correction-en",
            "What is Coco?",
            request_case=english_case,
        )
        assert english_old["guarantee"] == "USER_PREMISE"
        assert english_correction["lifecycle"] == "COMMIT"
        assert english_correction["guarantee"] == "USER_PREMISE"
        assert english_listing["lifecycle"] == "COMMIT"
        assert "Cocoはbirdである" in english_listing["text"]
        assert "Cocoはmammalではない" in english_listing["text"]
        assert english_old["dependencies"][0] not in english_listing["dependencies"]

        report = {
            "work_item": "BODY-1 BB-CONTEXT/BB-LEARN exact natural premise correction",
            "stage": "BODY-1_EXECUTION",
            "parser_version": PARSER_VERSION,
            "requirement_ids": ["A07", "A08", "CAP-D04", "CAP-D07", "CAP-D11", "CAP-D13"],
            "execution_mode": "one ordinary CLI request per fresh process; actor-scoped durable memory; temporary SQLite/blob store",
            "producer_consumer": (
                "ordinary Japanese/English correction -> sealed v56 CorrectPremise intent -> Core matches exact active direct USER_PREMISE in same actor and scope -> "
                "invalidation retains old fact -> Core records negative old claim and positive replacement in one memory revision -> fresh-process query/listing consumes revised memory while the previous result remains in case history"
            ),
            "scope": (
                "Supports one explicit copular correction with a named subject, one prior positive proposition and one distinct replacement in the same scope. "
                "Only matching active direct USER_PREMISE records are invalidated by the existing relation mechanism. The fixture shows a query-time derived answer no longer reproduces after its support is removed; prior result revisions remain historical. "
                "Persisted descendant fact/rule invalidation beyond this exercised path is NOT_RUN. No exact active target, wrong scope or another actor suspends without changing actor memory. Both new claims remain USER_PREMISE and are not checked against the outside world."
            ),
            "observations": observations,
            "failures_and_corrections": [
                {
                    "stage": "memory_revision_cas",
                    "failure": "The first normal-route attempt returned REVISION_CONFLICT because several relation mutations were being saved as multiple actor-memory revisions within one user request.",
                    "correction": "The Core now persists the compound correction as one actor-memory CAS revision; the same normal-route test passed.",
                },
                {
                    "stage": "dependent_result_validation",
                    "failure": "The first assertion treated a query-time derived answer as a persisted MemoryState fact.",
                    "correction": "Validation now checks that the prior derived ResultRecord remains in immutable case history and that a new post-correction query suspends.",
                },
            ],
            "state_checks": {
                "old_direct_premise_retained_but_inactive": True,
                "explicit_negative_old_claim_and_positive_replacement_persisted": True,
                "a_new_query_does_not_reproduce_the_prior_derived_answer": True,
                "previous_derived_result_remains_in_immutable_case_history": True,
                "unrelated_fact_preserved": True,
                "other_scope_fact_preserved": True,
                "natural_query_observes_negative_and_replacement_claims_after_restart": True,
                "unmatched_wrong_scope_and_other_actor_corrections_do_not_mutate_memory": True,
                "english_and_japanese_routes": "PASS",
            },
            "effect_receipts": "No external effect or user file write. Test requests use a temporary local SQLite/blob store removed on exit.",
            "outcome": "PASS scoped natural correction route",
            "acceptance_boundary": (
                "This is a bounded memory correction route over explicit user premises, not general co-reference resolution, open-world factual learning, "
                "or full A07/A08/CAP-D04/CAP-D07/CAP-D11/CAP-D13/BB-LEARN acceptance. Those broader obligations remain BUILD_GAP/UNVERIFIED."
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
