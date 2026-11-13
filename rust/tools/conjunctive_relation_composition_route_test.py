#!/usr/bin/env python3
"""Observe proof-preserving composition when one rule has two premises."""

import argparse
from datetime import datetime, timezone
import hashlib
import json
from pathlib import Path
import platform
import shutil
import subprocess
import tempfile


PARSER_VERSION = "declarative-ja-en-v60-conjunctive-relation-rule"


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--evidence-output", type=Path, required=True)
    arguments = parser.parse_args()
    repository_root = Path(__file__).resolve().parents[2]
    rust_root = repository_root / "rust"
    cargo = shutil.which("cargo") or "/home/agent/.cargo/bin/cargo"
    rustc = shutil.which("rustc") or "/home/agent/.cargo/bin/rustc"
    observations: list[dict] = []

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

    def require(condition: bool, message: str) -> None:
        if not condition:
            raise AssertionError(message)

    with tempfile.TemporaryDirectory(prefix="minidora-conjunctive-compose-") as temporary:
        database = Path(temporary) / "state"

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
            observation = {
                "case_id": case_id,
                "actor_id": actor_id,
                "input_id": input_id,
                "request": request_text,
                "exit_code": completed.returncode,
                "lifecycle": result.get("lifecycle"),
                "guarantee": result.get("guarantee"),
                "text": result.get("text"),
                "reason": result.get("reason"),
                "dependencies": result.get("dependencies", []),
            }
            observations.append(observation)
            return observation

        conjunction = run(
            "rule-origin",
            "japanese-owner",
            "two-premise-rule",
            "もし[scope=biology]Xは鳥かつXは羽毛持ちなら[scope=biology]Xは温血動物",
        )
        conjunction_id = conjunction["dependencies"][0]
        follow_up = run(
            "rule-origin",
            "japanese-owner",
            "warm-blooded-rule",
            "[scope=biology]Xは温血動物なら[scope=biology]Xは動物",
        )
        follow_up_id = follow_up["dependencies"][0]
        bird = run(
            "new-subject",
            "japanese-owner",
            "osprey-bird",
            "[scope=biology]ミサゴは鳥",
        )
        feathered = run(
            "new-subject",
            "japanese-owner",
            "osprey-feathered",
            "[scope=biology]ミサゴは羽毛持ち",
        )
        intermediate = run(
            "new-subject",
            "japanese-owner",
            "osprey-warm-blooded",
            "[scope=biology]ミサゴは温血動物ですか",
        )
        require(intermediate["lifecycle"] == "COMMIT", "the two-premise rule did not derive its conclusion")
        final_answer = run(
            "new-subject",
            "japanese-owner",
            "osprey-animal",
            "[scope=biology]ミサゴは動物ですか",
        )
        require(final_answer["lifecycle"] == "COMMIT", "the composed answer did not commit")
        require(final_answer["guarantee"] == "DERIVED", "composition lost its derived guarantee")
        for identity in (
            conjunction_id,
            follow_up_id,
            bird["dependencies"][0],
            feathered["dependencies"][0],
        ):
            require(identity in final_answer["dependencies"], f"composed proof omits {identity}")
        composed_ids = [
            identity
            for identity in final_answer["dependencies"]
            if identity.startswith("rule:") and identity not in (conjunction_id, follow_up_id)
        ]
        require(composed_ids, "the answer did not consume a stored composition shortcut")

        explanation = run(
            "new-subject",
            "japanese-owner",
            "osprey-animal-why",
            "どうして[scope=biology]ミサゴは動物ですか",
        )
        require("ミサゴは鳥である" in explanation["text"], "composition AskWhy omitted first premise")
        require("ミサゴは羽毛持ちである" in explanation["text"], "composition AskWhy omitted second premise")
        require(
            "ならば $v0は動物である" in explanation["text"],
            "AskWhy did not show the two-premise composed shortcut",
        )

        run(
            "rule-origin",
            "japanese-owner",
            "invalidate-follow-up-rule",
            f"{follow_up_id}を無効化して",
        )
        stopped_answer = run(
            "new-subject",
            "japanese-owner",
            "osprey-animal-after-rule-invalidation",
            "[scope=biology]ミサゴは動物ですか",
            expected_exit=2,
        )
        require("MISSING_PREMISES" in str(stopped_answer["reason"]), "invalidated root rule still supported composition")
        intermediate_after_rule = run(
            "new-subject",
            "japanese-owner",
            "osprey-warm-after-rule-invalidation",
            "[scope=biology]ミサゴは温血動物ですか",
        )
        require(intermediate_after_rule["guarantee"] == "DERIVED", "invalidating the follow-up erased the valid antecedent rule")
        replayed = run(
            "new-subject",
            "japanese-owner",
            "osprey-animal",
            "[scope=biology]ミサゴは動物ですか",
        )
        require(replayed["text"] == final_answer["text"], "the previous composed answer changed")
        require(replayed["dependencies"] == final_answer["dependencies"], "previous composed lineage changed")

        run(
            "new-subject",
            "japanese-owner",
            "invalidate-feathered-premise",
            f"{feathered['dependencies'][0]}を無効化して",
        )
        no_intermediate = run(
            "new-subject",
            "japanese-owner",
            "osprey-warm-after-premise-invalidation",
            "[scope=biology]ミサゴは温血動物ですか",
            expected_exit=2,
        )
        require("MISSING_PREMISES" in str(no_intermediate["reason"]), "invalidated antecedent remained usable")

        first = run(
            "english-rule-origin",
            "english-owner",
            "bird-to-feathered",
            "If X is a bird then X is feathered",
        )
        second = run(
            "english-rule-origin",
            "english-owner",
            "feathered-and-mammal-to-animal",
            "If X is feathered and X is a mammal then X is an animal",
        )
        english_bird = run(
            "english-new-subject",
            "english-owner",
            "robin-bird",
            "Robin is a bird",
        )
        english_mammal = run(
            "english-new-subject",
            "english-owner",
            "robin-mammal",
            "Robin is a mammal",
        )
        english_answer = run(
            "english-new-subject",
            "english-owner",
            "robin-animal",
            "Is Robin an animal?",
        )
        require(english_answer["lifecycle"] == "COMMIT", "single-to-conjunctive composition failed")
        for identity in (
            first["dependencies"][0],
            second["dependencies"][0],
            english_bird["dependencies"][0],
            english_mammal["dependencies"][0],
        ):
            require(identity in english_answer["dependencies"], f"single-to-conjunctive proof omits {identity}")
        english_composed_ids = [
            identity
            for identity in english_answer["dependencies"]
            if identity.startswith("rule:") and identity not in (first["dependencies"][0], second["dependencies"][0])
        ]
        require(english_composed_ids, "single-to-conjunctive path did not use a shortcut")
        english_explanation = run(
            "english-new-subject",
            "english-owner",
            "robin-animal-why",
            "Why is Robin an animal?",
        )
        require("Robinはbirdである" in english_explanation["text"], "English composition AskWhy omitted the first premise")
        require("Robinはmammalである" in english_explanation["text"], "English composition AskWhy omitted the second premise")

        report = {
            "work_item": "BODY-1 BB-LANG two-premise proof composition",
            "stage": "BODY-1_EXECUTION",
            "recorded_at": datetime.now(timezone.utc).isoformat(timespec="seconds"),
            "base_commit": command_output(["git", "-C", str(repository_root), "rev-parse", "HEAD"]),
            "requirement_ids": ["A02", "A04", "A05", "A09", "B01", "A15"],
            "software_version": "0.7.0-alpha.8",
            "schema_version": "minidora-rust/0.7.0-alpha.8",
            "parser_version": PARSER_VERSION,
            "toolchain": {
                "rustc": command_output([rustc, "--version"]),
                "cargo": command_output([cargo, "--version"]),
                "platform": platform.platform(),
            },
            "pre_change_probe": {
                "binary_sha256": "NOT_CAPTURED_BEFORE_SOURCE_EDIT",
                "reason": "No pre-edit executable hash or route observation was captured.",
            },
            "source_snapshot_sha256": hashes(
                [
                    "crates/minidora-domain/src/lib.rs",
                    "crates/minidora-engine/src/input.rs",
                    "crates/minidora-engine/src/core.rs",
                    "crates/minidora-engine/src/relations.rs",
                ]
            ),
            "test_and_tool_snapshot_sha256": hashes(
                [
                    "crates/minidora-engine/src/relations.rs",
                    "apps/cli/tests/normal_route.rs",
                    "tools/conjunctive_relation_composition_route_test.py",
                ]
            ),
            "documentation_snapshot_sha256": {
                "docs/redesign/01_SYSTEM_SPECIFICATION.md": hashlib.sha256(
                    (repository_root / "docs/redesign/01_SYSTEM_SPECIFICATION.md").read_bytes()
                ).hexdigest()
            },
            "execution_mode": "fresh CLI process for every request; one temporary SQLite state; Japanese multi-to-single and English single-to-conjunctive rule composition",
            "scope": "Composition produces a one- or two-premise shortcut only when all antecedents remain explicit, same-scope and bound; two multi-premise parents remain in query closure.",
            "producer_consumer": (
                "ordinary scoped rules -> proof-preserving composition stores both antecedents and both parent rule IDs -> "
                "new-subject facts consume the shortcut through normal CLI -> answer and AskWhy retain the complete premise/rule proof DAG"
            ),
            "japanese_conjunctive_rule_id": conjunction_id,
            "japanese_follow_up_rule_id": follow_up_id,
            "english_single_rule_id": first["dependencies"][0],
            "english_conjunctive_rule_id": second["dependencies"][0],
            "observations": observations,
            "outcome": "PASS",
            "commands": [
                "cargo fmt --all -- --check",
                "cargo test -p minidora-engine relations:: --locked",
                "cargo test -p minidora-engine input::tests:: --locked",
                "cargo test -p minidora-cli --test normal_route conjunctive_rule_joins_two_new_subject_premises_and_explains_both --locked",
                "cargo test -p minidora-cli --test normal_route conditional_premises_form_reusable_scoped_proof_across_restarts_and_retract --locked",
                "cargo check --workspace --all-targets --locked",
                "cargo clippy --workspace --all-targets --locked -- -D warnings",
                "cargo build --workspace --locked",
                "python3 tools/conjunctive_relation_composition_route_test.py --binary target/debug/minidora-rust --evidence-output audit/bb-lang-conjunctive-relation-composition-v61-route-validation.json",
            ],
            "not_run": [
                "full workspace runtime suite",
                "composition of two two-premise parent rules into a three-premise shortcut",
                "open-domain Q&A, general writing and full A02/A04/A05/A09/B01/A15 acceptance",
            ],
            "acceptance_boundary": (
                "This batch preserves the complete proof when composing a two-premise rule with a single-premise rule, "
                "and a single-premise rule with a two-premise rule. Three-premise shortcuts and two-multi-premise "
                "composition remain unsupported, although ordinary query closure retains the source rules. Full requirement acceptance remains UNVERIFIED."
            ),
            "next_consumer": "Bound a three-antecedent proof shortcut without losing scope, proof DAGs or resource bounds; open-domain Q&A and writing remain BUILD_GAP.",
        }
        arguments.evidence_output.parent.mkdir(parents=True, exist_ok=True)
        arguments.evidence_output.write_text(
            json.dumps(report, ensure_ascii=False, indent=2) + "\n", encoding="utf-8"
        )
        print(json.dumps(report, ensure_ascii=False, indent=2))


if __name__ == "__main__":
    main()
