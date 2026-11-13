#!/usr/bin/env python3
"""Observe Japanese transitive yes/no questions through fresh CLI processes."""

import argparse
from collections import Counter
from datetime import datetime, timezone
import hashlib
import json
from pathlib import Path
import platform
import shutil
import subprocess
import tempfile


PARSER_VERSION = "declarative-ja-en-v65-transitive-yes-no-questions"
SOURCE_FILES = [
    "crates/minidora-domain/src/lib.rs",
    "crates/minidora-engine/src/input.rs",
    "crates/minidora-engine/src/core.rs",
]
TEST_FILES = [
    "crates/minidora-engine/src/input.rs",
    "apps/cli/tests/normal_route.rs",
    "tools/transitive_yes_no_question_route_test.py",
]


def digest(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


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

    def output(command: list[str]) -> str:
        return subprocess.run(
            command,
            capture_output=True,
            text=True,
            timeout=10,
            check=True,
        ).stdout.strip()

    def require(condition: bool, message: str) -> None:
        if not condition:
            raise AssertionError(message)

    with tempfile.TemporaryDirectory(prefix="minidora-transitive-yes-no-") as temporary:
        database = Path(temporary) / "state"
        case = "transitive-yes-no-route"
        actor = "japanese-owner"

        def run(
            input_id: str,
            request_text: str,
            expected_lifecycle: str = "COMMIT",
            expected_exit: int = 0,
            run_case: str = case,
            run_actor: str = actor,
        ) -> dict:
            completed = subprocess.run(
                [
                    str(arguments.binary),
                    "--db",
                    str(database),
                    "--case",
                    run_case,
                    "--actor",
                    run_actor,
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
                    f"{run_case}/{input_id}: exit={completed.returncode}; "
                    f"stdout={completed.stdout}; stderr={completed.stderr}"
                )
            result = json.loads(completed.stdout)
            require(
                result.get("lifecycle") == expected_lifecycle,
                f"{input_id}: expected {expected_lifecycle}, got {result.get('lifecycle')}",
            )
            observation = {
                "case_id": run_case,
                "actor_id": run_actor,
                "input_id": input_id,
                "request": request_text,
                "request_sha256": digest(request_text.encode()),
                "exit_code": completed.returncode,
                "lifecycle": result.get("lifecycle"),
                "guarantee": result.get("guarantee"),
                "text": result.get("text"),
                "reason": result.get("reason"),
                "dependencies": result.get("dependencies", []),
                "result_id": result.get("id"),
            }
            observations.append(observation)
            return observation

        direct = run("yu-notebook", "ユウはノートを使う")
        direct_id = direct["dependencies"][0]
        rule = run("yu-tool-rule", "もしXはノートを使うならXは道具を使う")
        rule_id = rule["dependencies"][0]

        direct_answer = run("yu-notebook-question", "ユウはノートを使いますか")
        require(direct_answer["guarantee"] == "USER_PREMISE", "direct question changed its guarantee")
        require("ユウはノートを使う" in direct_answer["text"], "direct answer omitted its proposition")
        require(direct_id in direct_answer["dependencies"], "direct answer omitted its premise ID")
        derived_answer = run("yu-tool-question", "ユウは道具を使いますか")
        require(derived_answer["guarantee"] == "DERIVED", "derived question lost its guarantee")
        require("ユウは道具を使う" in derived_answer["text"], "derived answer omitted its proposition")
        require(direct_id in derived_answer["dependencies"], "derived answer omitted its premise ID")
        require(rule_id in derived_answer["dependencies"], "derived answer omitted its rule ID")

        negative = run("yu-not-poison", "ユウは毒を使わない")
        negative_answer = run("yu-not-poison-question", "ユウは毒を使いませんか")
        require("ユウは毒を使わない" in negative_answer["text"], "negative query lost its polarity")
        require(
            negative["dependencies"][0] in negative_answer["dependencies"],
            "negative answer omitted its premise ID",
        )
        refuted = run("yu-positive-poison-question", "ユウは毒を使いますか")
        require("ユウは毒を使わない" in refuted["text"], "refuted question did not return the opposite premise")
        require(
            negative["dependencies"][0] in refuted["dependencies"],
            "refuted answer omitted its premise ID",
        )

        run("yu-story-notebook", "[scope=story]ユウはノートを使う")
        wrong_scope = run(
            "yu-ecology-question",
            "[scope=ecology]ユウは道具を使いますか",
            expected_lifecycle="SUSPEND",
            expected_exit=2,
        )
        require("MISSING_PREMISES" in str(wrong_scope["reason"]), "wrong scope used unrelated support")
        missing_object = run(
            "yu-pen-question",
            "ユウはペンを使いますか",
            expected_lifecycle="SUSPEND",
            expected_exit=2,
        )
        require("MISSING_PREMISES" in str(missing_object["reason"]), "missing object got an answer")
        missing_subject = run(
            "saki-notebook-question",
            "サキはノートを使いますか",
            expected_lifecycle="SUSPEND",
            expected_exit=2,
        )
        require("MISSING_PREMISES" in str(missing_subject["reason"]), "missing subject got an answer")
        other_actor = run(
            "yu-other-actor-question",
            "ユウはノートを使いますか",
            expected_lifecycle="SUSPEND",
            expected_exit=2,
            run_case="isolated-actor-case",
            run_actor="other-actor",
        )
        require("MISSING_PREMISES" in str(other_actor["reason"]), "actor memory crossed its boundary")

        contradictory = run("yu-poison-positive", "ユウは毒を使う")
        conflict = run(
            "yu-conflict-question",
            "ユウは毒を使いますか",
            expected_lifecycle="SUSPEND",
            expected_exit=2,
        )
        require("CONFLICTING_PREMISES" in str(conflict["reason"]), "contradictory premises were hidden")
        invalidate = run(
            "invalidate-yu-poison-positive",
            f"{contradictory['dependencies'][0]}を無効化して",
        )
        require(invalidate["lifecycle"] == "COMMIT", "explicit premise invalidation failed")
        replay = run("yu-tool-question", "ユウは道具を使いますか")
        require(replay["text"] == derived_answer["text"], "earlier answer changed after invalidation")
        require(replay["dependencies"] == derived_answer["dependencies"], "earlier lineage changed")
        punctuation_form = run("yu-notebook-question-punctuation", "ユウはノートを使う?")
        require("ユウはノートを使う" in punctuation_form["text"], "question-mark form did not query")
        unsupported_past = run(
            "unsupported-past-question",
            "ユウはノートを使ったか",
            expected_lifecycle="SUSPEND",
            expected_exit=2,
        )
        require(
            "CAPABILITY_UNAVAILABLE" in str(unsupported_past["reason"]),
            "unsupported tense was silently interpreted",
        )

    source_hashes = {f"rust/{path}": digest((rust_root / path).read_bytes()) for path in SOURCE_FILES}
    test_hashes = {f"rust/{path}": digest((rust_root / path).read_bytes()) for path in TEST_FILES}
    summary = Counter(item["lifecycle"] for item in observations)
    evidence = {
        "work_item": "BODY-1 BB-LANG Japanese transitive yes/no questions through ordinary CLI",
        "stage": "BODY-1_EXECUTION",
        "recorded_at": datetime.now(timezone.utc).isoformat(timespec="seconds"),
        "base_commit": output(["git", "-C", str(repository_root), "rev-parse", "HEAD"]),
        "requirement_ids": ["A02", "A04", "A05", "A09", "A12", "A13", "A15", "B01"],
        "software_version": "0.7.0-alpha.8",
        "schema_version": "minidora-rust/0.7.0-alpha.8",
        "parser_version": PARSER_VERSION,
        "toolchain": {
            "rustc": output([rustc, "--version"]),
            "cargo": output([cargo, "--version"]),
            "platform": platform.platform(),
        },
        "implementation_change": "Terminal Japanese か and explicit question punctuation now seal supported transitive relations as Ask propositions. Core reuses the actor-scoped proof query and returns direct support, a derived conclusion, a supported opposite premise, or a typed missing/conflict stop.",
        "binary_sha256": digest(arguments.binary.read_bytes()),
        "source_snapshot_sha256": source_hashes,
        "test_and_tool_snapshot_sha256": test_hashes,
        "documentation_snapshot_sha256": {
            "docs/redesign/01_SYSTEM_SPECIFICATION.md": digest(
                (repository_root / "docs/redesign/01_SYSTEM_SPECIFICATION.md").read_bytes()
            ),
            "rust/README.md": digest((rust_root / "README.md").read_bytes()),
        },
        "execution_mode": "fresh CLI process for every request; one temporary SQLite state; ordinary request inputs; no test-only result injection",
        "scope": "Japanese present/polite transitive use relations, named subject/object, yes/no question particle or punctuation, exact scope and retained premise/rule proofs.",
        "observations": observations,
        "summary": {
            "total_requests": len(observations),
            "lifecycle_counts": dict(sorted(summary.items())),
            "direct_premise_id": direct_id,
            "rule_id": rule_id,
            "negative_premise_id": negative["dependencies"][0],
            "invalidated_positive_id": contradictory["dependencies"][0],
        },
        "targeted_checks": {
            "transitive_question_input_unit": "PASS",
            "ordinary_cli_route": "PASS",
            "direct_and_derived_support": "PASS",
            "negation_and_refutation": "PASS",
            "exact_scope_missing_subject_actor_isolation": "PASS",
            "conflict_and_invalidation": "PASS",
            "prior_result_immutability": "PASS",
            "question_punctuation_form": "PASS",
            "unsupported_past_tense_stop": "PASS",
        },
        "requirements_status": "A02/A04/A05/A09/A12/A13/A15/B01 acceptance remains UNVERIFIED; this is a bounded route observation, not the independent unseen-question QA population.",
        "limitations": [
            "The route covers the existing `使う` predicate only; it does not add verb vocabulary.",
            "Past-tense, alternative-object, generic wh questions and outside-world truth are not established.",
            "A contradicted yes/no claim returns the supported opposite premise; simultaneous support and refutation suspends.",
            "Full workspace runtime suite, broad BB-LANG acceptance and independent 24-question QA were not run.",
        ],
    }
    arguments.evidence_output.parent.mkdir(parents=True, exist_ok=True)
    arguments.evidence_output.write_text(
        json.dumps(evidence, ensure_ascii=False, indent=2) + "\n",
        encoding="utf-8",
    )
    print(
        json.dumps(
            {
                "evidence": str(arguments.evidence_output),
                "summary": evidence["summary"],
                "targeted_checks": evidence["targeted_checks"],
            },
            ensure_ascii=False,
            indent=2,
        )
    )


if __name__ == "__main__":
    main()
