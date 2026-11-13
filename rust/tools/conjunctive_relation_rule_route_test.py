#!/usr/bin/env python3
"""Observe ordinary CLI rule joins for two explicitly supplied premises."""

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

    with tempfile.TemporaryDirectory(prefix="minidora-conjunctive-rule-") as temporary:
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

        rule = run(
            "rule-origin",
            "japanese-owner",
            "two-premise-rule",
            "もし[scope=biology]Xは鳥かつXは羽毛持ちなら[scope=biology]Xは温血動物",
        )
        rule_id = rule["dependencies"][0]
        require(rule["lifecycle"] == "COMMIT", "Japanese two-premise rule was not committed")

        bird = run(
            "new-subject",
            "japanese-owner",
            "osprey-bird",
            "[scope=biology]ミサゴは鳥",
        )
        missing = run(
            "new-subject",
            "japanese-owner",
            "one-premise-question",
            "[scope=biology]ミサゴは温血動物ですか",
            expected_exit=2,
        )
        require(missing["lifecycle"] == "SUSPEND", "single antecedent was treated as a join")
        require("MISSING_PREMISES" in str(missing["reason"]), "missing premise stop is untyped")
        wrong_scope = run(
            "new-subject",
            "japanese-owner",
            "wrong-scope-question",
            "[scope=ecology]ミサゴは温血動物ですか",
            expected_exit=2,
        )
        require("MISSING_PREMISES" in str(wrong_scope["reason"]), "wrong scope used rule evidence")

        feathered = run(
            "new-subject",
            "japanese-owner",
            "osprey-feathered",
            "[scope=biology]ミサゴは羽毛持ち",
        )
        supported = run(
            "new-subject",
            "japanese-owner",
            "osprey-warm-blooded",
            "[scope=biology]ミサゴは温血動物ですか",
        )
        require(supported["lifecycle"] == "COMMIT", "joined Japanese proof did not commit")
        require(supported["guarantee"] == "DERIVED", "joined proof lost derived guarantee")
        for identity in (rule_id, bird["dependencies"][0], feathered["dependencies"][0]):
            require(identity in supported["dependencies"], f"joined proof omits {identity}")

        why = run(
            "new-subject",
            "japanese-owner",
            "osprey-why",
            "どうして[scope=biology]ミサゴは温血動物ですか",
        )
        require("ミサゴは鳥である" in why["text"], "AskWhy omitted its first premise")
        require("ミサゴは羽毛持ちである" in why["text"], "AskWhy omitted its second premise")
        require("かつ" in why["text"], "AskWhy omitted the conjunctive rule structure")

        counterexample = run(
            "new-subject",
            "japanese-owner",
            "osprey-not-bird",
            "[scope=biology]ミサゴは鳥ではない",
        )
        contested = run(
            "new-subject",
            "japanese-owner",
            "osprey-contested-result",
            "[scope=biology]ミサゴは温血動物ですか",
            expected_exit=2,
        )
        require(contested["lifecycle"] == "SUSPEND", "contradictory antecedent drove a conclusion")
        run(
            "new-subject",
            "japanese-owner",
            "invalidate-counterexample",
            f"{counterexample['dependencies'][0]}を無効化して",
        )
        restored = run(
            "new-subject",
            "japanese-owner",
            "osprey-restored-result",
            "[scope=biology]ミサゴは温血動物ですか",
        )
        require(restored["lifecycle"] == "COMMIT", "valid proof did not return after retraction")
        run(
            "new-subject",
            "japanese-owner",
            "invalidate-second-premise",
            f"{feathered['dependencies'][0]}を無効化して",
        )
        invalidated = run(
            "new-subject",
            "japanese-owner",
            "osprey-after-invalidation",
            "[scope=biology]ミサゴは温血動物ですか",
            expected_exit=2,
        )
        require("MISSING_PREMISES" in str(invalidated["reason"]), "invalid proof remained usable")
        replayed = run(
            "new-subject",
            "japanese-owner",
            "osprey-warm-blooded",
            "[scope=biology]ミサゴは温血動物ですか",
        )
        require(replayed["text"] == supported["text"], "historical committed answer changed")
        require(replayed["dependencies"] == supported["dependencies"], "historical lineage changed")

        english_rule = run(
            "english-rule-origin",
            "english-owner",
            "two-premise-rule",
            "If X is a bird and X is feathered then X is warm-blooded",
        )
        english_bird = run(
            "english-new-subject",
            "english-owner",
            "robin-bird",
            "Robin is a bird",
        )
        english_feathered = run(
            "english-new-subject",
            "english-owner",
            "robin-feathered",
            "Robin is feathered",
        )
        english_answer = run(
            "english-new-subject",
            "english-owner",
            "robin-question",
            "Is Robin warm-blooded?",
        )
        require(english_answer["lifecycle"] == "COMMIT", "English joined proof did not commit")
        require(english_answer["guarantee"] == "DERIVED", "English proof lost derived guarantee")
        for identity in (
            english_rule["dependencies"][0],
            english_bird["dependencies"][0],
            english_feathered["dependencies"][0],
        ):
            require(identity in english_answer["dependencies"], f"English proof omits {identity}")
        english_why = run(
            "english-new-subject",
            "english-owner",
            "robin-why",
            "Why is Robin warm-blooded?",
        )
        require("Robinはbirdである" in english_why["text"], "English AskWhy omitted first premise")
        require("Robinはfeatheredである" in english_why["text"], "English AskWhy omitted second premise")

        shared_variable_rule = run(
            "shared-variable-rule-origin",
            "shared-variable-owner",
            "shared-variable-rule",
            "If X is a bird and X is Y then Y is an animal",
        )
        joined_subject_bird = run(
            "shared-variable-rule-consumer",
            "shared-variable-owner",
            "osprey-bird",
            "Osprey is a bird",
        )
        bound_relation = run(
            "shared-variable-rule-consumer",
            "shared-variable-owner",
            "osprey-robin",
            "Osprey is Robin",
        )
        bound_subject_answer = run(
            "shared-variable-rule-consumer",
            "shared-variable-owner",
            "robin-animal",
            "Is Robin an animal?",
        )
        require(
            bound_subject_answer["lifecycle"] == "COMMIT"
            and bound_subject_answer["guarantee"] == "DERIVED",
            "shared-variable rule did not derive a new conclusion subject",
        )
        for identity in (
            shared_variable_rule["dependencies"][0],
            joined_subject_bird["dependencies"][0],
            bound_relation["dependencies"][0],
        ):
            require(identity in bound_subject_answer["dependencies"], f"shared-variable proof omits {identity}")
        bound_subject_why = run(
            "shared-variable-rule-consumer",
            "shared-variable-owner",
            "robin-why-animal",
            "Why is Robin an animal?",
        )
        require("Ospreyはbirdである" in bound_subject_why["text"], "shared-variable AskWhy omitted first premise")
        require("OspreyはRobinである" in bound_subject_why["text"], "shared-variable AskWhy omitted second premise")

        unsupported = run(
            "english-unsupported",
            "english-owner",
            "different-subject-rule",
            "If X is a bird and Y is feathered then X is warm-blooded",
            expected_exit=2,
        )
        require(unsupported["lifecycle"] == "SUSPEND", "different-subject rule was accepted")
        too_many = run(
            "english-unsupported",
            "english-owner",
            "three-premise-rule",
            "If X is a bird and X is feathered and X is a mammal then X is warm-blooded",
            expected_exit=2,
        )
        require(too_many["lifecycle"] == "SUSPEND", "three-premise rule was accepted")

        report = {
            "work_item": "BODY-1 BB-LANG ordinary two-premise relation rule",
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
                    "crates/minidora-engine/src/input.rs",
                    "crates/minidora-engine/src/relations.rs",
                    "apps/cli/tests/normal_route.rs",
                    "tools/conjunctive_relation_rule_route_test.py",
                ]
            ),
            "documentation_snapshot_sha256": {
                "docs/redesign/01_SYSTEM_SPECIFICATION.md": hashlib.sha256(
                    (repository_root / "docs/redesign/01_SYSTEM_SPECIFICATION.md").read_bytes()
                ).hexdigest()
            },
            "execution_mode": "fresh CLI process for every request; one temporary SQLite state; Japanese and English rule, premise, query, and AskWhy routes",
            "scope": "Exactly two explicit same-subject, same-scope copular premises joined by Japanese かつ/且つ or English and; rule conclusions retain both premise lineages.",
            "producer_consumer": (
                "ordinary conditional rule -> sealed AddConjunctiveRule -> Core saves both antecedents -> "
                "two facts for a new subject join through ordinary relation query -> the shared-variable form "
                "A(x) AND B(x,y) also binds a new conclusion subject -> answer and AskWhy preserve both premise IDs, rule ID, and history"
            ),
            "rule_id": rule_id,
            "english_rule_id": english_rule["dependencies"][0],
            "observations": observations,
            "outcome": "PASS",
            "commands": [
                "cargo fmt --all -- --check",
                "cargo test -p minidora-engine conjunctive_rule --locked",
                "cargo test -p minidora-engine input::tests:: --locked",
                "cargo test -p minidora-engine relations::tests:: --locked",
                "cargo test -p minidora-cli --test normal_route conjunctive_rule_joins_two_new_subject_premises_and_explains_both --locked",
                "cargo test -p minidora-cli --test normal_route conditional_premises_form_reusable_scoped_proof_across_restarts_and_retract --locked",
                "cargo test -p minidora-cli --test normal_route ordinary_question_lists_active_direct_premises_with_provenance_after_restart --locked",
                "cargo check --workspace --all-targets --locked",
                "cargo clippy --workspace --all-targets --locked -- -D warnings",
                "cargo build --workspace --locked",
                "python3 tools/conjunctive_relation_rule_route_test.py --binary target/debug/minidora-rust --evidence-output audit/bb-lang-conjunctive-relation-rule-v60-route-validation.json",
            ],
            "not_run": [
                "full workspace runtime suite",
                "multi-premise proof shortcut composition (query closure retains the complete proof instead)",
                "unbounded open-domain language, causal explanation, and full A02/A04/A05/A09/B01/A15 acceptance",
            ],
            "acceptance_boundary": (
                "This is bounded two-premise rule application over explicit caller-provided facts and rules. It does not establish unrestricted "
                "conjunction, open-domain reasoning, multi-premise proof shortcut composition, causal explanation, or full requirement acceptance."
            ),
            "next_consumer": "Extend proof-composition shortcuts to preserve all premises and bound joins across more varied shared-variable shapes; broad open-domain Q&A and writing remain BUILD_GAP.",
        }
        arguments.evidence_output.parent.mkdir(parents=True, exist_ok=True)
        arguments.evidence_output.write_text(
            json.dumps(report, ensure_ascii=False, indent=2) + "\n", encoding="utf-8"
        )
        print(json.dumps(report, ensure_ascii=False, indent=2))


if __name__ == "__main__":
    main()
