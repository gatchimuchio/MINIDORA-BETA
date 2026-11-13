#!/usr/bin/env python3
"""Observe the ordinary CLI consumer for two-clause relation questions."""

import argparse
from datetime import datetime, timezone
import hashlib
import json
from pathlib import Path
import platform
import shutil
import subprocess
import tempfile


PARSER_VERSION = "declarative-ja-en-v59-conjunctive-relation-question"


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

    with tempfile.TemporaryDirectory(prefix="minidora-conjunction-question-") as temporary:
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

        owner = "fixture-conjunction-owner"
        rule = run(
            "same-case",
            owner,
            "biology-rule",
            "[scope=biology]Xは鳥なら[scope=biology]Xは動物",
        )
        bird = run("same-case", owner, "coco-bird", "[scope=biology]ココは鳥")
        supported = run(
            "same-case",
            owner,
            "both-supported",
            "[scope=biology]ココは鳥かつ動物ですか",
        )
        rule_id = rule["dependencies"][0]
        bird_id = bird["dependencies"][0]
        require(supported["lifecycle"] == "COMMIT", "Japanese supported conjunction did not commit")
        require(supported["guarantee"] == "DERIVED", "derived clause lost its guarantee")
        require("鳥である" in supported["text"] and "動物である" in supported["text"], "both clauses were not returned")
        require(rule_id in supported["dependencies"], "the derived rule is absent from the answer lineage")
        require(bird_id in supported["dependencies"], "the direct premise is absent from the answer lineage")

        negative_clause = run(
            "same-case",
            owner,
            "negative-clause",
            "[scope=biology]ココは鳥ではないかつ動物ですか",
        )
        require(negative_clause["lifecycle"] == "COMMIT", "the explicit negative clause was not answered")
        require("鳥である" in negative_clause["text"], "the polarity-flipped proof was not shown")
        require(bird_id in negative_clause["dependencies"], "the negative clause proof lineage is missing")

        not_mammal = run(
            "same-case",
            owner,
            "coco-not-mammal",
            "[scope=biology]ココは哺乳類ではない",
        )
        refuted = run(
            "same-case",
            owner,
            "one-clause-refuted",
            "[scope=biology]ココは鳥かつ哺乳類ですか",
        )
        require(refuted["lifecycle"] == "COMMIT", "a complete supported/refuted result did not commit")
        require("組み合わせを支持しません" in refuted["text"], "the false conjunction was not identified")
        require("ココは哺乳類ではない" in refuted["text"], "the refuting relation was not shown")
        require(not_mammal["dependencies"][0] in refuted["dependencies"], "the negative premise is absent from lineage")

        missing = run(
            "same-case",
            owner,
            "missing-clause",
            "[scope=biology]ココは鳥かつふわふわですか",
            expected_exit=2,
        )
        require(missing["lifecycle"] == "SUSPEND", "a missing clause was not stopped")
        require("MISSING_PREMISES" in str(missing["reason"]), "the missing clause stop is not typed")

        run("same-case", owner, "coco-mammal", "[scope=biology]ココは哺乳類")
        conflict = run(
            "same-case",
            owner,
            "conflicting-clause",
            "[scope=biology]ココは鳥かつ哺乳類ですか",
            expected_exit=2,
        )
        require(conflict["lifecycle"] == "SUSPEND", "a conflicting clause was not stopped")
        require("CONFLICTING_PREMISES" in str(conflict["reason"]), "the relation conflict is not typed")

        wrong_scope = run(
            "same-case",
            owner,
            "wrong-scope",
            "[scope=ecology]ココは鳥かつ動物ですか",
            expected_exit=2,
        )
        require(wrong_scope["lifecycle"] == "SUSPEND", "wrong-scope evidence was used")
        require("MISSING_PREMISES" in str(wrong_scope["reason"]), "the wrong-scope stop is not typed")

        english_rule = run(
            "english-case",
            owner,
            "english-rule",
            "If X is a bird then X is an animal",
        )
        english_bird = run("english-case", owner, "english-bird", "Coco is a bird")
        english = run(
            "english-case",
            owner,
            "english-conjunction",
            "Is Coco a bird and an animal?",
        )
        require(english["lifecycle"] == "COMMIT", "the English conjunction did not commit")
        require(english["guarantee"] == "DERIVED", "the English derived guarantee was lost")
        require("Cocoはanimalである" in english["text"], "the English relation was not answered")
        require(
            english_bird["dependencies"][0] in english["dependencies"],
            "the English direct premise is absent from lineage",
        )
        require(
            english_rule["dependencies"][0] in english["dependencies"],
            "the English rule is absent from lineage",
        )

        unsupported_inputs = [
            ("or", "ココは鳥または動物ですか"),
            ("three-clauses", "ココは鳥かつ動物かつ哺乳類ですか"),
            ("different-subjects", "ココは鳥かつミケは動物ですか"),
            ("unresolved-pronoun", "それは鳥かつ動物ですか"),
        ]
        for input_id, request_text in unsupported_inputs:
            result = run(
                "unsupported-conjunctions",
                owner,
                input_id,
                request_text,
                expected_exit=2,
            )
            require(result["lifecycle"] == "SUSPEND", f"{input_id} was not stopped")

        report = {
            "work_item": "BODY-1 BB-LANG ordinary two-clause relation question",
            "stage": "BODY-1_EXECUTION",
            "recorded_at": datetime.now(timezone.utc).isoformat(timespec="seconds"),
            "base_commit": command_output(
                ["git", "-C", str(repository_root), "rev-parse", "HEAD"]
            ),
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
                    "apps/cli/tests/normal_route.rs",
                    "tools/conjunctive_relation_question_route_test.py",
                ]
            ),
            "documentation_snapshot_sha256": hashes(
                [
                    "README.md",
                    "audit/README.md",
                    "audit/IMPLEMENTATION_STATUS.md",
                    "audit/requirements.tsv",
                    "audit/rust-requirement-observations.json",
                ]
            ),
            "execution_mode": "fresh CLI process for every request; one SQLite state across requests; Japanese and English relation proofs",
            "scope": "Exactly two explicit copular clauses joined by Japanese かつ/且つ or English and, with one named subject and a shared scope.",
            "producer_consumer": (
                "ordinary Japanese/English question -> sealed AskConjunction with two propositions -> Core queries both against one actor memory -> "
                "combined answer plus exact union of premise/rule dependencies -> normal renderer"
            ),
            "rule_id": rule_id,
            "direct_premise_id": bird_id,
            "english_rule_id": english_rule["dependencies"][0],
            "english_direct_premise_id": english_bird["dependencies"][0],
            "observations": observations,
            "outcome": "PASS",
            "commands": [
                "cargo fmt --all -- --check",
                "cargo test -p minidora-engine conjunctive_relation_questions_keep_both_same_subject_clauses --locked",
                "cargo test -p minidora-cli --test normal_route conjunctive_relation_questions_query_every_clause_and_keep_their_proofs --locked",
                "cargo check --workspace --all-targets --locked",
                "cargo clippy --workspace --all-targets --locked -- -D warnings",
                "cargo build --workspace --locked",
                "python3 tools/conjunctive_relation_question_route_test.py --binary target/debug/minidora-rust --evidence-output audit/bb-lang-conjunctive-relation-question-v59-route-validation.json",
            ],
            "not_run": [
                "full workspace runtime suite",
                "three-or-more clauses, alternatives, different subjects, pronoun co-reference and causal AskWhy",
                "open-domain natural-language question answering, general writing and full BB-LANG acceptance",
            ],
            "acceptance_boundary": (
                "This is a bounded same-subject two-clause relation question over caller-provided premises and rules. It evaluates every clause and "
                "preserves the combined proof lineage, but it does not establish open-domain Q&A, unrestricted compositional language, causal explanation, "
                "or full A02/A04/A05/A09/B01/A15 acceptance."
            ),
            "next_consumer": "Extend ordinary language relation composition to a two-premise rule whose bound inputs are consumed by a later new-subject query; broad open-domain Q&A and writing remain BUILD_GAP.",
        }
        arguments.evidence_output.parent.mkdir(parents=True, exist_ok=True)
        arguments.evidence_output.write_text(
            json.dumps(report, ensure_ascii=False, indent=2) + "\n", encoding="utf-8"
        )
        print(json.dumps(report, ensure_ascii=False, indent=2))


if __name__ == "__main__":
    main()
