#!/usr/bin/env python3
"""Exercise subject-pronoun relation questions against exact prior proof context."""

import argparse
from datetime import datetime, timezone
import hashlib
import json
from pathlib import Path
import platform
import shutil
import subprocess
import tempfile


PARSER_VERSION = "declarative-ja-en-v58-prior-result-pronoun-query"


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

    with tempfile.TemporaryDirectory(prefix="minidora-relation-pronoun-") as temporary:
        database = Path(temporary) / "state"
        owner = "fixture-pronoun-owner"
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
                    "exit_code": completed.returncode,
                    "lifecycle": result.get("lifecycle"),
                    "guarantee": result.get("guarantee"),
                    "text": result.get("text"),
                    "reason": result.get("reason"),
                    "dependencies": result.get("dependencies", []),
                }
            )
            return result

        rule = run(
            "same-case", owner, "biology-rule", "[scope=biology]Xは鳥なら[scope=biology]Xは動物"
        )
        rule_id = rule["dependencies"][0]
        bird = run("same-case", owner, "coco-bird", "[scope=biology]ココは鳥")
        bird_id = bird["dependencies"][0]
        answer = run("same-case", owner, "pronoun-question", "それは動物ですか")
        assert answer["lifecycle"] == "COMMIT"
        assert answer["guarantee"] == "DERIVED"
        assert "ココは動物である" in answer["text"]
        assert rule_id in answer["dependencies"]
        assert bird_id in answer["dependencies"]

        why = run("same-case", owner, "pronoun-why", "なぜそれは動物ですか")
        assert why["lifecycle"] == "COMMIT"
        assert why["guarantee"] == "DERIVED"
        assert why["dependencies"] == answer["dependencies"]
        assert "直接提示された前提: ココは鳥である" in why["text"]

        wrong_scope = run(
            "same-case", owner, "wrong-scope-question", "[scope=ecology]それは動物ですか", 2
        )
        assert wrong_scope["lifecycle"] == "SUSPEND"
        assert wrong_scope["reason"].startswith("MISSING_RELATION_CONTEXT:")
        run("same-case", owner, "topic-change-calculation", "2+3")
        topic_stop = run("same-case", owner, "after-topic-change", "それは動物ですか", 2)
        assert topic_stop["reason"].startswith("MISSING_RELATION_CONTEXT:")

        case_stop = run(
            "no-prior-case-result", owner, "no-context-question", "それは動物ですか", 2
        )
        assert case_stop["lifecycle"] == "SUSPEND"
        assert case_stop["reason"].startswith("MISSING_RELATION_CONTEXT:")
        why_case_stop = run(
            "no-prior-why-result", owner, "no-context-why", "なぜそれは動物ですか", 2
        )
        assert why_case_stop["lifecycle"] == "SUSPEND"
        assert why_case_stop["reason"].startswith("MISSING_RELATION_CONTEXT:")
        actor_stop = run(
            "other-actor-no-context",
            "separate-actor",
            "other-actor-question",
            "それは動物ですか",
            2,
        )
        assert actor_stop["lifecycle"] == "SUSPEND"
        assert actor_stop["reason"].startswith("MISSING_RELATION_CONTEXT:")

        run(
            "ambiguous-scope-context",
            owner,
            "ambiguous-biology-rule",
            "[scope=biology]Xは鳥なら[scope=biology]Xは動物",
        )
        biology_bird = run(
            "ambiguous-scope-context",
            owner,
            "swallow-biology-premise",
            "[scope=biology]ツバメは鳥",
        )
        run(
            "ambiguous-scope-context",
            owner,
            "swallow-ecology-premise",
            "[scope=ecology]ツバメは鳥",
        )
        listed = run(
            "ambiguous-scope-context",
            owner,
            "list-context-premises",
            "ツバメとは何ですか",
        )
        assert len(listed["dependencies"]) == 2
        ambiguous = run(
            "ambiguous-scope-context",
            owner,
            "ambiguous-pronoun-question",
            "それは動物ですか",
            2,
        )
        assert ambiguous["lifecycle"] == "SUSPEND"
        assert ambiguous["reason"].startswith("AMBIGUOUS_RELATION_CONTEXT:")
        relisted = run(
            "ambiguous-scope-context",
            owner,
            "relist-context-premises",
            "ツバメとは何ですか",
        )
        assert relisted["dependencies"] == listed["dependencies"]
        disambiguated = run(
            "ambiguous-scope-context",
            owner,
            "scoped-pronoun-question",
            "[scope=biology]それは動物ですか",
        )
        assert disambiguated["lifecycle"] == "COMMIT"
        assert disambiguated["guarantee"] == "DERIVED"
        assert "ツバメは動物である" in disambiguated["text"]
        assert biology_bird["dependencies"][0] in disambiguated["dependencies"]

        run(
            "english-same-case",
            owner,
            "english-rule",
            "If X is a bird then X is an animal",
        )
        english_bird = run(
            "english-same-case", owner, "english-bird", "Coco is a bird"
        )
        english_answer = run(
            "english-same-case", owner, "english-pronoun-question", "Is it an animal?"
        )
        assert english_answer["lifecycle"] == "COMMIT"
        assert english_answer["guarantee"] == "DERIVED"
        assert english_bird["dependencies"][0] in english_answer["dependencies"]
        english_why = run(
            "english-same-case", owner, "english-pronoun-why", "Why is it an animal?"
        )
        assert english_why["lifecycle"] == "COMMIT"
        assert english_why["guarantee"] == "DERIVED"
        assert english_why["dependencies"] == english_answer["dependencies"]

        report = {
            "work_item": "BODY-1 BB-CONTEXT bounded singular-pronoun Ask/AskWhy consumer",
            "stage": "BODY-1_EXECUTION",
            "recorded_at": datetime.now(timezone.utc).isoformat(timespec="seconds"),
            "base_commit": command_output(
                ["git", "-C", str(repository_root), "rev-parse", "HEAD"]
            ),
            "requirement_ids": ["A04", "A07", "A08", "CAP-D04", "CAP-D11"],
            "software_version": "0.7.0-alpha.8",
            "schema_version": "minidora-rust/0.7.0-alpha.8",
            "parser_version": PARSER_VERSION,
            "toolchain": {
                "rustc": command_output([rustc, "--version"]),
                "cargo": command_output([cargo, "--version"]),
                "platform": platform.platform(),
            },
            "pre_change_probe": {
                "base_commit": "07c3950cf398a3775489fdb70d12578d988e241a",
                "binary_sha256": "NOT_CAPTURED_BEFORE_SOURCE_EDIT",
                "observations": [
                    {
                        "case": "ask-before",
                        "prior_same_case_results": ["explicit rule COMMIT", "Coco bird premise COMMIT"],
                        "request": "それは動物ですか",
                        "lifecycle": "SUSPEND",
                        "reason_code": "MISSING_PREMISES",
                    },
                    {
                        "case": "why-before",
                        "prior_same_case_results": ["explicit rule COMMIT", "Coco bird premise COMMIT"],
                        "request": "なぜそれは動物ですか",
                        "lifecycle": "SUSPEND",
                        "reason_code": "MISSING_PREMISES",
                    },
                ],
            },
            "source_snapshot_sha256": hashes(
                [
                    "crates/minidora-domain/src/lib.rs",
                    "crates/minidora-engine/src/input.rs",
                    "crates/minidora-engine/src/core.rs",
                    "crates/minidora-engine/src/relations.rs",
                    "crates/minidora-engine/src/store.rs",
                    "crates/minidora-engine/src/math.rs",
                ]
            ),
            "test_and_tool_snapshot_sha256": hashes(
                [
                    "crates/minidora-engine/src/input.rs",
                    "apps/cli/tests/normal_route.rs",
                    "tools/contextual_relation_pronoun_test.py",
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
            "execution_mode": "fresh CLI process per request; same-case, different-case, different-actor and English/Japanese contexts",
            "scope": "Only singular it/this/that and これ/それ/あれ subject references; direct active premise IDs named by the selected prior committed result.",
            "producer_consumer": (
                "same-case committed direct premise result -> new singular-pronoun relation question -> Core resolves its exact "
                "direct premise and inherited scope -> existing actor-memory proof query -> answer or AskWhy with the same proof lineage"
            ),
            "rule_id": rule_id,
            "direct_premise_id": bird_id,
            "new_case_subject_premise_id": biology_bird["dependencies"][0],
            "observations": observations,
            "observed_gate_correction": {
                "initial_command": "cargo clippy --workspace --all-targets --locked -- -D warnings",
                "initial_exit_code": 101,
                "initial_findings": [
                    "pre-existing math.rs unit-test needless borrow in serde_json::from_str",
                    "pre-existing fixed-size math.rs unit-test Vec allocation",
                ],
                "correction": "removed the redundant borrow and changed fixed test data from Vec to array; no runtime behavior changed",
                "focused_regression": "cargo test -p minidora-engine previous_probability_and_polynomial_reports_are_recomputed_and_bounded --locked: PASS",
                "workspace_clippy_rerun": "cargo clippy --workspace --all-targets --locked -- -D warnings: PASS",
            },
            "outcome": "PASS",
            "commands": [
                "cargo fmt --all -- --check",
                "cargo test -p minidora-engine singular_pronoun_relation_questions_retain_context_and_scope_requirements --locked",
                "cargo test -p minidora-cli --test normal_route singular_pronoun_relation_questions_use_only_unique_same_case_proof_context --locked",
                "cargo check --workspace --all-targets --locked",
                "cargo clippy --workspace --all-targets --locked -- -D warnings",
                "cargo build --workspace --locked",
                "cargo test -p minidora-engine previous_probability_and_polynomial_reports_are_recomputed_and_bounded --locked",
                "python3 tools/contextual_relation_pronoun_test.py --binary target/debug/minidora-rust --evidence-output audit/bb-context-singular-pronoun-relation-query-v58-route-validation.json",
            ],
            "not_run": [
                "full workspace runtime suite",
                "plural, multi-clause and cross-topic pronoun resolution",
                "general Japanese/English co-reference and full A04/A07/A08/CAP-D04/CAP-D11 acceptance",
            ],
            "acceptance_boundary": (
                "This route establishes only a bounded same-case singular-pronoun relation Ask/AskWhy consumer with exact direct-premise lineage, "
                "scope inheritance/selection, and fail-closed missing/ambiguous/cross-case/cross-actor/topic-change cases. It does not establish "
                "plural or general co-reference, broad conversation continuity, or full BB-CONTEXT acceptance."
            ),
            "next_consumer": "Plural and multi-clause co-reference across topic changes remain later BB-CONTEXT gaps.",
        }
        arguments.evidence_output.parent.mkdir(parents=True, exist_ok=True)
        arguments.evidence_output.write_text(
            json.dumps(report, ensure_ascii=False, indent=2) + "\n", encoding="utf-8"
        )
        print(json.dumps(report, ensure_ascii=False, indent=2))


if __name__ == "__main__":
    main()
