#!/usr/bin/env python3
"""Exercise scoped declared-rule reuse across cases through fresh CLI processes."""

import argparse
from datetime import datetime, timezone
import hashlib
import json
from pathlib import Path
import platform
import re
import shutil
import subprocess
import tempfile


PARSER_VERSION = "declarative-ja-en-v57-contextual-premise-correction"


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

    with tempfile.TemporaryDirectory(prefix="minidora-cross-case-learning-") as temporary:
        database = Path(temporary) / "state"
        owner = "fixture-learning-owner"
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
            "learning-origin",
            owner,
            "declared-scoped-rule",
            "[scope=biology]Xは鳥なら[scope=biology]Xは動物",
        )
        assert rule["lifecycle"] == "COMMIT"
        assert rule["guarantee"] == "USER_PREMISE"
        rule_match = re.search(r"規則ID: (rule:[0-9a-f]{64})", rule["text"])
        assert rule_match
        rule_id = rule_match.group(1)

        original_bird = run(
            "learning-origin", owner, "original-subject-bird", "[scope=biology]ココは鳥"
        )
        original_bird_id = original_bird["dependencies"][0]
        original_answer = run(
            "learning-origin",
            owner,
            "original-subject-answer",
            "[scope=biology]ココは動物ですか",
        )
        assert original_answer["guarantee"] == "DERIVED"

        new_bird = run(
            "learning-transfer", owner, "new-subject-bird", "[scope=biology]ミサゴは鳥"
        )
        new_bird_id = new_bird["dependencies"][0]
        new_answer = run(
            "learning-transfer",
            owner,
            "new-subject-answer",
            "[scope=biology]ミサゴは動物ですか",
        )
        assert new_answer["lifecycle"] == "COMMIT"
        assert new_answer["guarantee"] == "DERIVED"
        answer_dependencies = new_answer["dependencies"]
        assert rule_id in answer_dependencies
        assert new_bird_id in answer_dependencies
        assert original_bird_id not in answer_dependencies
        derived_ids = [
            dependency
            for dependency in answer_dependencies
            if dependency.startswith("fact:") and dependency != new_bird_id
        ]
        assert len(derived_ids) == 1, "the transfer proof must retain a derived fact identity"

        why = run(
            "learning-transfer",
            owner,
            "new-subject-why",
            "どうして[scope=biology]ミサゴは動物なのですか",
        )
        assert why["lifecycle"] == "COMMIT"
        assert why["guarantee"] == "DERIVED"
        assert why["dependencies"] == answer_dependencies
        assert "直接提示された前提: ミサゴは鳥である" in why["text"]
        assert "適用規則: $xは鳥である ならば $xは動物である" in why["text"]
        assert original_bird_id not in why["dependencies"]

        scope_mismatch = run(
            "scope-isolation", owner, "other-scope-bird", "[scope=ecology]ペンギンは鳥"
        )
        assert scope_mismatch["guarantee"] == "USER_PREMISE"
        scope_stop = run(
            "scope-isolation",
            owner,
            "other-scope-question",
            "[scope=ecology]ペンギンは動物ですか",
            expected_exit=2,
        )
        assert scope_stop["lifecycle"] == "SUSPEND"
        assert scope_stop["reason"].startswith("MISSING_PREMISES:")

        isolated_bird = run(
            "actor-isolation", "separate-actor", "other-actor-bird", "[scope=biology]カモメは鳥"
        )
        assert isolated_bird["guarantee"] == "USER_PREMISE"
        actor_stop = run(
            "actor-isolation",
            "separate-actor",
            "other-actor-question",
            "[scope=biology]カモメは動物ですか",
            expected_exit=2,
        )
        assert actor_stop["lifecycle"] == "SUSPEND"
        assert actor_stop["reason"].startswith("MISSING_PREMISES:")

        counterexample = run(
            "learning-transfer",
            owner,
            "new-subject-counterexample",
            "[scope=biology]ミサゴは動物ではない",
        )
        counterexample_id = counterexample["dependencies"][0]
        contested = run(
            "learning-transfer",
            owner,
            "contested-new-subject-question",
            "[scope=biology]ミサゴは動物ですか",
            expected_exit=2,
        )
        assert contested["lifecycle"] == "SUSPEND"
        assert contested["reason"].startswith("CONFLICTING_PREMISES:")

        invalidated = run(
            "learning-transfer", owner, "invalidate-transferred-rule", f"{rule_id}を無効化して"
        )
        assert invalidated["lifecycle"] == "COMMIT"
        assert rule_id in invalidated["dependencies"]
        after_invalidation = run(
            "learning-transfer",
            owner,
            "answer-after-invalidation",
            "[scope=biology]ミサゴは動物ですか",
        )
        assert after_invalidation["guarantee"] == "USER_PREMISE"
        assert after_invalidation["dependencies"] == [counterexample_id]

        run(
            "later-new-task", owner, "later-subject-bird", "[scope=biology]ノスリは鳥"
        )
        later_stop = run(
            "later-new-task",
            owner,
            "later-subject-question",
            "[scope=biology]ノスリは動物ですか",
            expected_exit=2,
        )
        assert later_stop["lifecycle"] == "SUSPEND"
        assert later_stop["reason"].startswith("MISSING_PREMISES:")

        replayed_original = run(
            "learning-origin",
            owner,
            "original-subject-answer",
            "[scope=biology]ココは動物ですか",
        )
        assert replayed_original == original_answer

        report = {
            "work_item": "BODY-1 BB-LEARN scoped rule consumer across a new case and subject",
            "stage": "BODY-1_EXECUTION",
            "recorded_at": datetime.now(timezone.utc).isoformat(timespec="seconds"),
            "base_commit": command_output(
                ["git", "-C", str(repository_root), "rev-parse", "HEAD"]
            ),
            "requirement_ids": ["CAP-D05", "CAP-D06", "CAP-D07", "CAP-D12", "CAP-D13"],
            "software_version": "0.7.0-alpha.8",
            "schema_version": "minidora-rust/0.7.0-alpha.8",
            "parser_version": PARSER_VERSION,
            "toolchain": {
                "rustc": command_output([rustc, "--version"]),
                "cargo": command_output([cargo, "--version"]),
                "platform": platform.platform(),
            },
            "source_snapshot_sha256": hashes(
                [
                    "crates/minidora-domain/src/lib.rs",
                    "crates/minidora-engine/src/input.rs",
                    "crates/minidora-engine/src/core.rs",
                    "crates/minidora-engine/src/relations.rs",
                    "crates/minidora-engine/src/recovery.rs",
                    "crates/minidora-engine/src/store.rs",
                ]
            ),
            "test_and_tool_snapshot_sha256": hashes(
                [
                    "apps/cli/tests/normal_route.rs",
                    "tools/learned_rule_cross_case_transfer_test.py",
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
            "execution_mode": "fresh CLI process per request; one durable store, multiple cases and isolated actors",
            "scope": "Explicit caller-declared biology rule and premises only; no empirical induction or world-truth claim.",
            "producer_consumer": (
                "Core adopts a scoped rule and original-subject premise in one case -> a fresh process in a new case "
                "consumes the same rule with a new subject -> AskWhy reports only that proof lineage -> a counterexample "
                "causes a typed conflict -> explicit invalidation retains history and stops later transfer; different "
                "scopes and actors remain isolated"
            ),
            "rule_id": rule_id,
            "original_subject_premise_id": original_bird_id,
            "new_subject_premise_id": new_bird_id,
            "new_subject_derived_fact_id": derived_ids[0],
            "counterexample_id": counterexample_id,
            "observations": observations,
            "outcome": "PASS",
            "commands": [
                "cargo fmt --all -- --check",
                "cargo test -p minidora-cli --test normal_route declared_rule_transfers_to_new_subject_in_new_case_with_bounded_lineage --locked",
                "cargo check --workspace --all-targets --locked",
                "cargo clippy -p minidora-cli --all-targets --locked -- -D warnings",
                "cargo build -p minidora-cli --locked",
                "python3 tools/learned_rule_cross_case_transfer_test.py --binary target/debug/minidora-rust --evidence-output audit/bb-learn-cross-case-rule-transfer-v58-route-validation.json",
            ],
            "not_run": [
                "full workspace runtime suite",
                "empirical rule induction from unlabelled examples",
                "generalized causal learning, broad transfer quality, and full BB-LEARN/CAP-D05-D14 acceptance",
            ],
            "acceptance_boundary": (
                "This route verifies proof-preserving reuse of one explicitly supplied scoped rule across a new case "
                "and subject, exact dependency lineage, counterexample handling, invalidation, actor/scope boundaries, "
                "and original result replay. It does not establish autonomous learning or full BB-LEARN acceptance."
            ),
        }
        arguments.evidence_output.parent.mkdir(parents=True, exist_ok=True)
        arguments.evidence_output.write_text(
            json.dumps(report, ensure_ascii=False, indent=2) + "\n", encoding="utf-8"
        )
        print(json.dumps(report, ensure_ascii=False, indent=2))


if __name__ == "__main__":
    main()
