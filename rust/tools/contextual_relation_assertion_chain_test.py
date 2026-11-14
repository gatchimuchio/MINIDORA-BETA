#!/usr/bin/env python3
"""Exercise one typed, proof-checked singular-pronoun assertion chain."""

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

PARSER_VERSION = "declarative-ja-en-v72-contextual-relation-assertion"


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--evidence-output", type=Path, required=True)
    arguments = parser.parse_args()
    repository_root = Path(__file__).resolve().parents[2]
    rust_root = repository_root / "rust"
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

    def hashes(paths: list[str]) -> dict[str, str]:
        return {
            f"rust/{path}": hashlib.sha256((rust_root / path).read_bytes()).hexdigest()
            for path in paths
        }

    with tempfile.TemporaryDirectory(prefix="minidora-contextual-assertion-chain-") as temporary:
        database = Path(temporary) / "state"
        observations: list[dict] = []

        def run(
            case_id: str,
            actor_id: str,
            input_id: str,
            request_text: str,
            expected_exit: int = 0,
        ) -> dict:
            completed = subprocess.run(
                [
                    str(arguments.binary),
                    "--db",
                    str(database),
                    "--case",
                    case_id,
                    "--actor",
                    actor_id,
                    "--input",
                    input_id,
                    "--json",
                    request_text,
                ],
                capture_output=True,
                text=True,
                timeout=15,
                check=False,
            )
            if completed.returncode != expected_exit:
                raise AssertionError(
                    f"{case_id}/{input_id}: exit={completed.returncode}; "
                    f"stdout={completed.stdout}; stderr={completed.stderr}"
                )
            result = json.loads(completed.stdout)
            observations.append(
                {
                    "case_id": case_id,
                    "actor_id": actor_id,
                    "input_id": input_id,
                    "request": request_text,
                    "input_sha256": hashlib.sha256(request_text.encode("utf-8")).hexdigest(),
                    "exit_code": completed.returncode,
                    "lifecycle": result.get("lifecycle"),
                    "guarantee": result.get("guarantee"),
                    "result_id": result.get("id"),
                    "text": result.get("text"),
                    "reason": result.get("reason"),
                    "dependencies": result.get("dependencies", []),
                    "content_plan_id": (result.get("content_plan") or {}).get("id"),
                }
            )
            return result

        def memory(actor_id: str) -> tuple[int, dict]:
            with sqlite3.connect(database / "state.sqlite3") as connection:
                row = connection.execute(
                    "SELECT revision,record_sha256 FROM memory_revision "
                    "WHERE actor_id=? ORDER BY revision DESC LIMIT 1",
                    (actor_id,),
                ).fetchone()
            if row is None:
                return 0, {"facts": [], "rules": []}
            value = json.loads(
                (database / "blobs" / row[1]).read_text(encoding="utf-8")
            )
            return row[0], value

        actor = "typed-pronoun-assertion-chain"
        case = "typed-pronoun-assertion-chain-case"
        run(
            case,
            actor,
            "chain-rule",
            "[scope=biology]Xは鳥なら[scope=biology]Xは動物",
        )
        run(case, actor, "chain-bird", "[scope=biology]ココは鳥")
        anchor = run(case, actor, "chain-anchor", "それは動物ですか")
        assert anchor["lifecycle"] == "COMMIT"
        first_assertion = run(case, actor, "chain-assertion-one", "それは温血動物です")
        assert first_assertion["lifecycle"] == "COMMIT"
        assert first_assertion["guarantee"] == "USER_PREMISE"
        assert first_assertion["dependencies"][0] == anchor["id"]
        first_fact_id = first_assertion["dependencies"][1]
        assert "主語「ココ」とscope「biology」" in first_assertion["text"]

        second_assertion = run(case, actor, "chain-assertion-two", "それは哺乳類です")
        assert second_assertion["lifecycle"] == "COMMIT"
        assert second_assertion["guarantee"] == "USER_PREMISE"
        assert second_assertion["dependencies"][0] == first_assertion["id"]
        assert len(second_assertion["dependencies"]) == 2
        second_fact_id = second_assertion["dependencies"][1]
        assert "主語「ココ」とscope「biology」" in second_assertion["text"]
        assert "ココは哺乳類" in second_assertion["text"]

        with sqlite3.connect(database / "state.sqlite3") as connection:
            (bundle_sha,) = connection.execute(
                "SELECT bundle_sha256 FROM input_identity "
                "WHERE case_id=? AND input_id=? AND input_version=1",
                (case, "chain-assertion-two"),
            ).fetchone()
            saved_bundle = json.loads(
                (database / "blobs" / bundle_sha).read_text(encoding="utf-8")
            )
        assert saved_bundle["meaning"]["parser_version"] == PARSER_VERSION
        assert saved_bundle["meaning"]["context_result_ref"] == first_assertion["id"]
        assert saved_bundle["meaning"]["intent"]["kind"] == "ASSERT_PREVIOUS_RELATION"
        assert saved_bundle["meaning"]["intent"]["proposition"]["subject"] == "それ"

        next_query = run(case, actor, "chain-next-query", "それは哺乳類ですか")
        assert next_query["lifecycle"] == "COMMIT"
        assert next_query["guarantee"] == "USER_PREMISE"
        assert next_query["dependencies"] == [second_fact_id]
        assert "ココは哺乳類である（範囲: biology）" in next_query["text"]
        _, chain_memory = memory(actor)
        chain_facts = {fact["id"]: fact for fact in chain_memory["facts"]}
        assert chain_facts[first_fact_id]["valid"] is True
        assert chain_facts[second_fact_id]["valid"] is True
        assert chain_facts[second_fact_id]["proposition"]["subject"] == "ココ"
        assert chain_facts[second_fact_id]["source_input"] == second_assertion["id"]

        stale_actor = "typed-pronoun-assertion-chain-stale"
        stale_case = "typed-pronoun-assertion-chain-stale-case"
        run(
            stale_case,
            stale_actor,
            "stale-chain-rule",
            "[scope=biology]Xは鳥なら[scope=biology]Xは動物",
        )
        run(stale_case, stale_actor, "stale-chain-bird", "[scope=biology]ココは鳥")
        stale_anchor = run(stale_case, stale_actor, "stale-chain-anchor", "それは動物ですか")
        stale_first = run(
            stale_case,
            stale_actor,
            "stale-chain-assertion-one",
            "それは温血動物です",
        )
        assert stale_first["lifecycle"] == "COMMIT"
        stale_fact_id = stale_first["dependencies"][1]
        correction = run(
            "stale-chain-correction-case",
            stale_actor,
            "stale-chain-correction",
            "[scope=biology]訂正: ココは温血動物ではなく哺乳類です",
        )
        assert correction["lifecycle"] == "COMMIT"
        _, corrected_memory = memory(stale_actor)
        corrected_facts = {fact["id"]: fact for fact in corrected_memory["facts"]}
        assert corrected_facts[stale_fact_id]["valid"] is False

        stale_assertion = run(
            stale_case,
            stale_actor,
            "assert-after-prior-fact-invalidation",
            "それは魚です",
            expected_exit=2,
        )
        assert stale_assertion["lifecycle"] == "SUSPEND"
        assert stale_assertion["reason"].startswith("CONTEXT_PROOF_CHANGED:")
        assert stale_assertion.get("content_plan") is None
        missing_new_fact = run(
            "stale-chain-no-new-fact-probe",
            stale_actor,
            "stale-chain-fish-was-not-stored",
            "[scope=biology]ココは魚ですか",
            expected_exit=2,
        )
        assert missing_new_fact["lifecycle"] == "SUSPEND"
        assert missing_new_fact["reason"].startswith("MISSING_PREMISES:")
        assert missing_new_fact.get("content_plan") is None
        assert stale_anchor["id"] in stale_first["dependencies"]

        source_paths = [
            "crates/minidora-domain/src/lib.rs",
            "crates/minidora-engine/src/input.rs",
            "crates/minidora-engine/src/core.rs",
            "crates/minidora-engine/src/relations.rs",
            "crates/minidora-engine/src/store.rs",
            "tools/contextual_relation_assertion_chain_test.py",
        ]
        report = {
            "work_item": "BODY-1 Core exact fact continuity across one chained contextual relation assertion",
            "stage": "BODY-1_EXECUTION",
            "recorded_at": datetime.now(timezone.utc).isoformat(timespec="seconds"),
            "base_commit": command_output(
                ["git", "-C", str(repository_root), "rev-parse", "HEAD"]
            ),
            "requirement_ids": ["A04", "A07", "A08", "A15", "CAP-D04", "CAP-D11"],
            "software_version": "0.7.0-alpha.8",
            "schema_version": "minidora-rust/0.7.0-alpha.8",
            "parser_version": PARSER_VERSION,
            "binary_sha256": hashlib.sha256(arguments.binary.read_bytes()).hexdigest(),
            "toolchain": {
                "rustc": command_output([rustc, "--version"]),
                "cargo": command_output([cargo, "--version"]),
                "platform": platform.platform(),
            },
            "source_snapshot_sha256": hashes(source_paths),
            "documentation_snapshot_sha256": hashes(
                [
                    "README.md",
                    "audit/README.md",
                    "audit/IMPLEMENTATION_STATUS.md",
                    "audit/requirements.tsv",
                    "audit/rust-requirement-observations.json",
                ]
            ),
            "execution_mode": "fresh CLI process per request; one temporary SQLite state; direct actor memory revision inspection",
            "scope": (
                "One immediate chained singular-pronoun relation assertion consumes the prior contextual assertion's exact active USER_PREMISE fact, rechecks that fact through the current relation proof, and is consumed by one following ordinary relation question. "
                "A corrected/inactivated prior fact suspends the chain without a ContentPlan."
            ),
            "producer_consumer": (
                "typed relation answer -> first sealed AssertPreviousRelation -> resolved USER_PREMISE fact -> "
                "second sealed AssertPreviousRelation -> exact source-input fact lookup and current proof revalidation -> "
                "second USER_PREMISE fact -> next AskPreviousRelation consumes only the second fact; "
                "a correction that invalidates the first fact blocks the second assertion"
            ),
            "targeted_checks": {
                "chained_assertion_reuses_previous_typed_subject_and_scope": "PASS",
                "sealed_source_keeps_unresolved_pronoun_and_exact_context_result": "PASS",
                "following_query_consumes_only_second_assertion_fact": "PASS",
                "cross_case_correction_invalidates_exact_source_fact": "PASS",
                "invalidated_source_fact_suspends_without_content_plan": "PASS",
                "failed_second_assertion_is_not_stored": "PASS",
            },
            "summary": {
                "fresh_process_observations": len(observations),
                "positive_chain_observations": 6,
                "invalidated_chain_observations": 7,
            },
            "first_assertion_fact_id": first_fact_id,
            "second_assertion_fact_id": second_fact_id,
            "invalidated_source_fact_id": stale_fact_id,
            "observations": observations,
            "outcome": "PASS",
            "commands": [
                "cargo fmt --all -- --check",
                "cargo check --workspace --all-targets --locked",
                "cargo clippy --workspace --all-targets --locked -- -D warnings",
                "cargo build --workspace --locked",
                "python3 tools/contextual_relation_assertion_chain_test.py --binary /workspace/.minidora-cloud/rust-target/debug/minidora-rust --evidence-output audit/bb-context-pronoun-relation-assertion-chain-v1-route-validation.json",
                "git diff --check",
            ],
            "not_run": [
                "full workspace runtime suite",
                "arbitrary multi-turn and multi-clause co-reference",
                "full A04/A07/A08/A15/CAP-D04/CAP-D11 acceptance",
            ],
            "acceptance_boundary": (
                "This verifies one same-case consecutive assertion chain over an exact active direct premise, plus one subsequent relation query and an invalidation denial. "
                "It does not establish arbitrary Japanese/English co-reference, unbounded assertion chains, general discourse resolution, truth verification, or full requirement acceptance."
            ),
        }
        arguments.evidence_output.parent.mkdir(parents=True, exist_ok=True)
        arguments.evidence_output.write_text(
            json.dumps(report, ensure_ascii=False, indent=2) + "\n", encoding="utf-8"
        )
        print(json.dumps(report, ensure_ascii=False, indent=2))


if __name__ == "__main__":
    main()
