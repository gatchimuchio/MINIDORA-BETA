#!/usr/bin/env python3
"""Observe three-antecedent shortcuts from two explicitly conjunctive rules."""

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

    with tempfile.TemporaryDirectory(prefix="minidora-ternary-compose-") as temporary:
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

        rule_a = run(
            "japanese-rule-origin",
            "japanese-owner",
            "bird-feathered-to-warm-blooded",
            "もし[scope=biology]Xは鳥かつXは羽毛持ちなら[scope=biology]Xは温血動物",
        )
        rule_a_id = rule_a["dependencies"][0]
        rule_b = run(
            "japanese-rule-origin",
            "japanese-owner",
            "warm-blooded-mammal-to-animal",
            "もし[scope=biology]Xは温血動物かつXは哺乳類なら[scope=biology]Xは動物",
        )
        rule_b_id = rule_b["dependencies"][0]
        bird = run(
            "japanese-new-subject",
            "japanese-owner",
            "osprey-bird",
            "[scope=biology]ミサゴは鳥",
        )
        feathered = run(
            "japanese-new-subject",
            "japanese-owner",
            "osprey-feathered",
            "[scope=biology]ミサゴは羽毛持ち",
        )
        mammal = run(
            "japanese-new-subject",
            "japanese-owner",
            "osprey-mammal",
            "[scope=biology]ミサゴは哺乳類",
        )
        answer = run(
            "japanese-new-subject",
            "japanese-owner",
            "osprey-animal",
            "[scope=biology]ミサゴは動物ですか",
        )
        require(answer["lifecycle"] == "COMMIT", "the three-antecedent answer did not commit")
        require(answer["guarantee"] == "DERIVED", "the shortcut lost its derived guarantee")
        for identity in (
            rule_a_id,
            rule_b_id,
            bird["dependencies"][0],
            feathered["dependencies"][0],
            mammal["dependencies"][0],
        ):
            require(identity in answer["dependencies"], f"the answer omits {identity}")
        composed_ids = [
            identity
            for identity in answer["dependencies"]
            if identity.startswith("rule:") and identity not in (rule_a_id, rule_b_id)
        ]
        require(composed_ids, "the answer did not consume the three-antecedent shortcut")

        explanation = run(
            "japanese-new-subject",
            "japanese-owner",
            "osprey-animal-why",
            "どうして[scope=biology]ミサゴは動物ですか",
        )
        for phrase in (
            "ミサゴは鳥である",
            "ミサゴは羽毛持ちである",
            "ミサゴは哺乳類である",
        ):
            require(phrase in explanation["text"], f"AskWhy omits {phrase}")
        require(
            explanation["text"].count("かつ") >= 2
            and "ならば $v0は動物である" in explanation["text"],
            "AskWhy did not display the complete three-antecedent shortcut",
        )

        run(
            "japanese-new-subject",
            "japanese-owner",
            "invalidate-osprey-mammal",
            f"{mammal['dependencies'][0]}を無効化して",
        )
        stopped = run(
            "japanese-new-subject",
            "japanese-owner",
            "osprey-animal-after-mammal-invalidation",
            "[scope=biology]ミサゴは動物ですか",
            expected_exit=2,
        )
        require("MISSING_PREMISES" in str(stopped["reason"]), "invalidated premise still supports the shortcut")
        replayed = run(
            "japanese-new-subject",
            "japanese-owner",
            "osprey-animal",
            "[scope=biology]ミサゴは動物ですか",
        )
        require(replayed["text"] == answer["text"], "the prior committed answer changed after invalidation")
        require(
            replayed["dependencies"] == answer["dependencies"],
            "the prior committed proof lineage changed after invalidation",
        )

        report = {
            "work_item": "BODY-1 BB-LANG two two-premise rules to a three-antecedent shortcut",
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
                    "tools/conjunctive_relation_ternary_composition_route_test.py",
                ]
            ),
            "documentation_snapshot_sha256": {
                "docs/redesign/01_SYSTEM_SPECIFICATION.md": hashlib.sha256(
                    (repository_root / "docs/redesign/01_SYSTEM_SPECIFICATION.md").read_bytes()
                ).hexdigest()
            },
            "execution_mode": "fresh CLI process for every request; one temporary SQLite state; Japanese normal route",
            "scope": (
                "Declared rules and natural-language parsing stay limited to one or two explicit premises. "
                "Internal composition of two two-premise root rules may create a three-antecedent shortcut. "
                "Each successful join prefix consumes the bounded relation-step budget."
            ),
            "producer_consumer": (
                "two explicitly conjunctive scoped rules -> three-premise shortcut with both parent rule IDs -> "
                "three independent facts for a new subject -> ordinary CLI answer and AskWhy preserve all "
                "antecedents and the composed proof DAG; premise invalidation stops later queries while "
                "the prior committed result remains immutable"
            ),
            "parent_rule_ids": [rule_a_id, rule_b_id],
            "shortcut_rule_ids_observed": composed_ids,
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
                "python3 tools/conjunctive_relation_ternary_composition_route_test.py --binary /workspace/.minidora-cloud/rust-target/debug/minidora-rust --evidence-output audit/bb-lang-conjunctive-relation-ternary-composition-v62-route-validation.json",
            ],
            "not_run": [
                "full workspace runtime suite",
                "normal CLI registration of a three-explicit-premise rule",
                "open-domain Q&A, general writing and full A02/A04/A05/A09/B01/A15 acceptance",
            ],
            "acceptance_boundary": (
                "The observed path covers a bounded internal three-antecedent shortcut produced from two explicit "
                "two-premise rules. Explicit user input remains capped at two premises. Full requirement acceptance "
                "and the workspace runtime suite remain NOT_RUN / UNVERIFIED."
            ),
            "next_consumer": "Continue the ordinary natural-language producer-to-consumer path; general open-domain Q&A and writing remain BUILD_GAP.",
        }
        arguments.evidence_output.parent.mkdir(parents=True, exist_ok=True)
        arguments.evidence_output.write_text(
            json.dumps(report, ensure_ascii=False, indent=2) + "\n", encoding="utf-8"
        )
        print(json.dumps(report, ensure_ascii=False, indent=2))


if __name__ == "__main__":
    main()
