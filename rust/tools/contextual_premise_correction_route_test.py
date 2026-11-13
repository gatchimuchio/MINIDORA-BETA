#!/usr/bin/env python3
"""Exercise prior-result-anchored premise correction through fresh CLI processes."""

import argparse
import hashlib
import json
from pathlib import Path
import sqlite3
import subprocess
import tempfile

PARSER_VERSION = "declarative-ja-en-v57-contextual-premise-correction"


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--evidence-output", type=Path)
    arguments = parser.parse_args()

    with tempfile.TemporaryDirectory(prefix="minidora-contextual-correction-") as temporary:
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
                    str(arguments.binary),
                    "--db",
                    str(database),
                    "--case",
                    case,
                    "--actor",
                    actor,
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
                    "actor_id": actor,
                    "case_id": case,
                    "input_id": input_id,
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
                    "render_sha256": (result.get("render") or {}).get("actual_sha256"),
                    "dependencies": result.get("dependencies", []),
                }
            )
            return result

        def memory(actor: str) -> tuple[int, dict]:
            with sqlite3.connect(database / "state.sqlite3") as connection:
                row = connection.execute(
                    "SELECT revision, record_sha256 FROM memory_revision "
                    "WHERE actor_id=? ORDER BY revision DESC LIMIT 1",
                    (actor,),
                ).fetchone()
            if row is None:
                return 0, {"facts": [], "rules": []}
            value = json.loads((database / "blobs" / row[1]).read_text(encoding="utf-8"))
            return row[0], value

        def proposition(fact: dict, subject: str, object_: str, scope: str, negative: bool) -> bool:
            value = fact["proposition"]
            return (
                value["subject"] == subject
                and value["object"] == object_
                and value["scope"] == scope
                and value["negative"] is negative
            )

        first_premise = run(
            "owner",
            "natural-ja",
            "coco-mammal",
            "ココは哺乳類",
        )
        first_answer = run(
            "owner",
            "natural-ja",
            "ask-coco-mammal",
            "ココは哺乳類ですか",
        )
        old_id = first_premise["dependencies"][0]
        assert old_id in first_answer["dependencies"]
        revision_before_cross_case = memory("owner")[0]
        contextual = run(
            "owner",
            "natural-ja",
            "pronoun-correction",
            "それは哺乳類ではなく鳥です",
        )
        assert contextual["lifecycle"] == "COMMIT"
        assert contextual["guarantee"] == "USER_PREMISE"
        assert old_id in contextual["dependencies"]
        assert "直前の回答が参照した提示前提ID" in contextual["text"]

        with sqlite3.connect(database / "state.sqlite3") as connection:
            revisions = connection.execute(
                "SELECT state_sha256 FROM case_revision WHERE case_id=? ORDER BY revision",
                ("natural-ja",),
            ).fetchall()
        historical_answer = None
        for (state_sha,) in revisions:
            state = json.loads((database / "blobs" / state_sha).read_text(encoding="utf-8"))
            values = list(state.get("prior_results", {}).values())
            if state.get("result"):
                values.append(state["result"])
            for saved in values:
                if saved.get("input_id") == "ask-coco-mammal":
                    historical_answer = saved
        assert historical_answer == first_answer

        revision_after_context = memory("owner")[0]
        assert revision_after_context == revision_before_cross_case + 1
        actor_memory, actor_state = memory("owner")
        facts_by_id = {fact["id"]: fact for fact in actor_state["facts"]}
        assert facts_by_id[old_id]["valid"] is False
        assert any(
            fact["valid"] and proposition(fact, "ココ", "哺乳類", "default", True)
            for fact in actor_state["facts"]
        )
        assert any(
            fact["valid"] and proposition(fact, "ココ", "鳥", "default", False)
            for fact in actor_state["facts"]
        )
        assert actor_memory == revision_after_context

        listing = run("owner", "natural-ja", "coco-premises-after", "ココとは何ですか")
        assert "ココは鳥である" in listing["text"]
        assert "ココは哺乳類ではない" in listing["text"]
        assert old_id not in listing["dependencies"]
        revision_before_no_context = memory("owner")[0]
        no_context = run(
            "owner",
            "different-case",
            "pronoun-without-case-result",
            "It is a fish, not a mammal.",
            expected_exit=2,
        )
        assert no_context["lifecycle"] == "SUSPEND"
        assert no_context["reason"].startswith("MISSING_CORRECTION_CONTEXT:")
        assert memory("owner")[0] == revision_before_no_context

        default = run(
            "scope-owner",
            "scoped-ja",
            "default-premise",
            "ココは哺乳類",
        )
        story = run(
            "scope-owner",
            "scoped-ja",
            "story-premise",
            "[scope=story]ココは哺乳類",
        )
        scopes = run("scope-owner", "scoped-ja", "list-scopes", "ココとは何ですか")
        assert default["dependencies"][0] in scopes["dependencies"]
        assert story["dependencies"][0] in scopes["dependencies"]
        revision_before_ambiguous = memory("scope-owner")[0]
        ambiguous = run(
            "scope-owner",
            "scoped-ja",
            "ambiguous-pronoun",
            "それは哺乳類ではなく鳥です",
            expected_exit=2,
        )
        assert ambiguous["lifecycle"] == "SUSPEND"
        assert ambiguous["reason"].startswith("AMBIGUOUS_CORRECTION_CONTEXT:")
        assert memory("scope-owner")[0] == revision_before_ambiguous
        scoped_list = run(
            "scope-owner",
            "scoped-ja",
            "relist-scopes",
            "ココとは何ですか",
        )
        revision_before_scoped_commit = memory("scope-owner")[0]
        scoped = run(
            "scope-owner",
            "scoped-ja",
            "scoped-pronoun",
            "[scope=story]それは哺乳類ではなく鳥です",
        )
        assert scoped["lifecycle"] == "COMMIT"
        assert memory("scope-owner")[0] == revision_before_scoped_commit + 1
        assert story["dependencies"][0] in scoped["dependencies"]
        assert default["dependencies"][0] not in scoped["dependencies"]
        _, scoped_memory = memory("scope-owner")
        scoped_facts = {fact["id"]: fact for fact in scoped_memory["facts"]}
        assert scoped_facts[default["dependencies"][0]]["valid"] is True
        assert scoped_facts[story["dependencies"][0]]["valid"] is False
        scoped_result = run(
            "scope-owner",
            "scoped-ja",
            "list-after-scoped-correction",
            "ココとは何ですか",
        )
        assert "ココは哺乳類である" in scoped_result["text"]
        assert "ココは哺乳類ではない（範囲: story" in scoped_result["text"]
        assert "ココは鳥である（範囲: story" in scoped_result["text"]
        assert default["dependencies"][0] in scoped_result["dependencies"]
        assert story["dependencies"][0] not in scoped_result["dependencies"]
        assert scoped_list["dependencies"] == scopes["dependencies"]

        derived_premise = run(
            "derived-owner",
            "derived",
            "derived-coco-mammal",
            "Coco is a mammal",
        )
        rule = run(
            "derived-owner",
            "derived",
            "mammal-rule",
            "If X is a mammal then X is an animal",
        )
        derived_answer = run(
            "derived-owner",
            "derived",
            "coco-animal",
            "Is Coco an animal?",
        )
        assert derived_answer["guarantee"] == "DERIVED"
        assert derived_premise["dependencies"][0] in derived_answer["dependencies"]
        assert rule["dependencies"][0] in derived_answer["dependencies"]
        revision_before_derived = memory("derived-owner")[0]
        derived_correction = run(
            "derived-owner",
            "derived",
            "pronoun-derived-only",
            "It is a fish, not an animal.",
            expected_exit=2,
        )
        assert derived_correction["lifecycle"] == "SUSPEND"
        assert derived_correction["reason"].startswith("MISSING_CORRECTION_TARGET:")
        assert memory("derived-owner")[0] == revision_before_derived
        retained = run(
            "derived-owner",
            "derived",
            "still-mammal",
            "Is Coco a mammal?",
        )
        assert retained["guarantee"] == "USER_PREMISE"
        assert derived_premise["dependencies"][0] in retained["dependencies"]

        english_premise = run(
            "english-owner",
            "natural-en",
            "coco-mammal-en",
            "Coco is a mammal",
        )
        english_answer = run(
            "english-owner",
            "natural-en",
            "ask-mammal-en",
            "Is Coco a mammal?",
        )
        assert english_premise["dependencies"][0] in english_answer["dependencies"]
        english_correction = run(
            "english-owner",
            "natural-en",
            "pronoun-correction-en",
            "It is a bird, not a mammal.",
        )
        assert english_correction["lifecycle"] == "COMMIT"
        assert english_correction["guarantee"] == "USER_PREMISE"
        english_listing = run(
            "english-owner",
            "natural-en",
            "list-coco-en",
            "What is Coco?",
        )
        assert "Cocoはbirdである" in english_listing["text"]
        assert "Cocoはmammalではない" in english_listing["text"]

        report = {
            "work_item": "BODY-1 BB-CONTEXT prior-result-anchored natural premise correction",
            "stage": "BODY-1_EXECUTION",
            "parser_version": PARSER_VERSION,
            "requirement_ids": ["A07", "A08", "CAP-D04", "CAP-D07", "CAP-D11", "CAP-D13"],
            "execution_mode": "ordinary CLI request per fresh process; same-case ResultRecord dependency anchoring; actor-scoped durable memory; temporary SQLite/blob store",
            "producer_consumer": (
                "ordinary Japanese/English correction with a supported singular pronoun -> sealed v57 CorrectPreviousPremise intent -> Core selects only active direct USER_PREMISE facts referenced by the selected same-case committed ResultRecord -> exact unique proposition/scope match -> one-revision invalidation and replacement -> fresh-process listing observes the update while old result history remains"
            ),
            "scope": (
                "The finite forms it/this/that and これ/それ/あれ can refer only to one unique active direct premise with the exact superseded predicate/object among the selected same-case prior committed result dependencies. An explicit scope further narrows candidates; without it, multiple scopes or subjects suspend. Missing same-case context, no matching direct premise, and derived-only prior conclusions suspend without changing memory. This is not general co-reference, plural reference, empirical learning, or outside-world truth verification."
            ),
            "observations": observations,
            "failures_and_corrections": [
                {
                    "stage": "scope-route-fixture",
                    "failure": "The first normal-route scoped example supplied an English stored object (mammal) and then tried to correct it with a Japanese object (哺乳類); exact proposition matching correctly suspended with MISSING_CORRECTION_TARGET.",
                    "correction": "The scope test now uses Japanese for both premise and correction. A same-language retry passed; the product matching rule was unchanged.",
                },
            ],
            "state_checks": {
                "prior_result_dependencies_anchor_subject_and_scope": True,
                "old_fact_retained_but_inactive": True,
                "replacement_and_explicit_negative_claim_persisted": True,
                "previous_answer_remains_in_immutable_case_history": True,
                "cross_case_context_does_not_resolve_actor_global_memory": True,
                "multiple_scopes_suspend_without_mutation": True,
                "explicit_scope_selects_only_prior_supported_target": True,
                "derived_only_context_cannot_rewrite_its_support_fact": True,
                "english_and_japanese_routes": "PASS",
            },
            "effect_receipts": "No external effect. CLI requests use a temporary SQLite/blob store removed on exit.",
            "outcome": "PASS bounded prior-result contextual correction route",
            "acceptance_boundary": (
                "This extends one evidence-backed correction form; broad A07/A08/CAP-D04/CAP-D07/CAP-D11/CAP-D13 and BB-CONTEXT/BB-LEARN acceptance remain BUILD_GAP/UNVERIFIED."
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
