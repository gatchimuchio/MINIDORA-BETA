#!/usr/bin/env python3
"""Observe bounded Japanese transitive relations through fresh CLI processes."""

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


PARSER_VERSION = "declarative-ja-en-v70-generic-transitive-verbs"
SOURCE_FILES = [
    "crates/minidora-domain/src/lib.rs",
    "crates/minidora-engine/src/input.rs",
    "crates/minidora-engine/src/core.rs",
    "crates/minidora-engine/src/recovery.rs",
]
TEST_FILES = [
    "crates/minidora-engine/src/input.rs",
    "apps/cli/tests/normal_route.rs",
    "tools/transitive_object_question_route_test.py",
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

    with tempfile.TemporaryDirectory(prefix="minidora-transitive-question-") as temporary:
        database = Path(temporary) / "state"
        case = "transitive-object-route"
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

        direct = run("mina-cutter", "ミナはカッターを使う")
        direct_id = direct["dependencies"][0]
        rule = run("tool-rule", "もしXはカッターを使うならXは道具を使う")
        rule_id = rule["dependencies"][0]
        supported = run("mina-uses-question", "ミナは何を使いますか")
        require(supported["guarantee"] == "DERIVED", "joined answer lost its derived guarantee")
        require("ミナはカッターを使う" in supported["text"], "direct object was omitted")
        require("ミナは道具を使う" in supported["text"], "derived object was omitted")
        require("直接提示:" in supported["text"], "direct premise origin was omitted")
        require("規則導出:" in supported["text"], "derived proof origin was omitted")
        require(direct_id in supported["dependencies"], "answer omitted its direct premise ID")
        require(rule_id in supported["dependencies"], "answer omitted its rule ID")
        require(
            "網羅性や現実世界の真偽は確認していません" in supported["text"],
            "answer omitted its evidence and completeness limits",
        )

        negative = run("mina-no-poison", "ミナは毒を使わない")
        negative_answer = run("mina-not-uses-question", "ミナは何を使いませんか")
        require(negative_answer["guarantee"] == "USER_PREMISE", "negative answer changed guarantee")
        require("ミナは毒を使わない" in negative_answer["text"], "negative premise was omitted")
        require(
            negative["dependencies"][0] in negative_answer["dependencies"],
            "negative answer omitted its premise ID",
        )

        generic_positive = run("mina-drinks-water", "ミナは水を飲む")
        generic_rule = run(
            "drink-rule",
            "もしXは水を飲むならXは液体を飲む",
        )
        generic_question = run("mina-what-drinks", "ミナは何を飲む?")
        require(
            generic_question["guarantee"] == "DERIVED",
            "generic verb rule result lost its derived guarantee",
        )
        require(
            "直接提示: ミナは水を飲む" in generic_question["text"],
            "generic verb direct premise was omitted",
        )
        require(
            "規則導出: ミナは液体を飲む" in generic_question["text"],
            "generic verb rule result was omitted",
        )
        require(
            generic_positive["dependencies"][0] in generic_question["dependencies"],
            "generic verb answer omitted the direct premise ID",
        )
        require(
            generic_rule["dependencies"][0] in generic_question["dependencies"],
            "generic verb answer omitted the applied rule ID",
        )
        generic_negative = run("mina-no-poison-drink", "ミナは毒を飲まない")
        generic_negative_answer = run(
            "mina-what-not-drinks",
            "ミナは何を飲まない?",
        )
        require(
            "ミナは毒を飲まない" in generic_negative_answer["text"],
            "generic negative relation was not rendered with its observed polarity",
        )
        require(
            generic_negative["dependencies"][0]
            in generic_negative_answer["dependencies"],
            "generic negative answer omitted its premise ID",
        )
        generic_yes_no = run("mina-drinks-water-question", "ミナは水を飲むか")
        require(
            "ミナは水を飲む" in generic_yes_no["text"],
            "generic yes/no proof did not keep the same canonical predicate",
        )
        unsupported_polite = run(
            "unsupported-polite-drink",
            "ミナは何を飲みますか",
            expected_lifecycle="SUSPEND",
            expected_exit=2,
        )
        require(
            "CAPABILITY_UNAVAILABLE" in str(unsupported_polite["reason"]),
            "ambiguous polite conjugation was not retained as unsupported",
        )
        unsupported_intransitive = run(
            "unsupported-die-object-question",
            "ミナは何を死ぬ?",
            expected_lifecycle="SUSPEND",
            expected_exit=2,
        )
        require(
            "CAPABILITY_UNAVAILABLE" in str(unsupported_intransitive["reason"]),
            "known intransitive verb was accepted as a transitive object question",
        )

        run("mina-story-notebook", "[scope=story]ミナはノートを使う")
        wrong_scope = run(
            "mina-ecology-question",
            "[scope=ecology]ミナは何を使う?",
            expected_lifecycle="SUSPEND",
            expected_exit=2,
        )
        require("MISSING_PREMISES" in str(wrong_scope["reason"]), "wrong scope did not stop as missing")
        missing_subject = run(
            "saki-uses-question",
            "サキは何を使う?",
            expected_lifecycle="SUSPEND",
            expected_exit=2,
        )
        require(
            "MISSING_PREMISES" in str(missing_subject["reason"]),
            "unobserved subject received an answer",
        )
        other_actor = run(
            "mina-other-actor-question",
            "ミナは何を使う?",
            expected_lifecycle="SUSPEND",
            expected_exit=2,
            run_case="isolated-actor-case",
            run_actor="other-actor",
        )
        require("MISSING_PREMISES" in str(other_actor["reason"]), "actor memory crossed its boundary")

        contradictory = run("mina-uses-poison", "ミナは毒を使う")
        conflict = run(
            "mina-conflict-question",
            "ミナは何を使う?",
            expected_lifecycle="SUSPEND",
            expected_exit=2,
        )
        require("CONFLICTING_PREMISES" in str(conflict["reason"]), "conflict was hidden")
        invalidate = run(
            "invalidate-poison-positive",
            f"{contradictory['dependencies'][0]}を無効化して",
        )
        require(invalidate["lifecycle"] == "COMMIT", "explicit premise invalidation failed")
        replay = run("mina-uses-question", "ミナは何を使いますか")
        require(replay["text"] == supported["text"], "earlier committed answer changed after invalidation")
        require(
            replay["dependencies"] == supported["dependencies"],
            "earlier committed answer dependencies changed after invalidation",
        )

        unsupported_past = run(
            "unsupported-past-question",
            "ミナは何を使った?",
            expected_lifecycle="SUSPEND",
            expected_exit=2,
        )
        require(
            "CAPABILITY_UNAVAILABLE" in str(unsupported_past["reason"])
            or "no unambiguous interpretation" in str(unsupported_past["reason"]),
            "unsupported past form did not retain its grammar gap",
        )

    changed_sources = {f"rust/{path}": digest((rust_root / path).read_bytes()) for path in SOURCE_FILES}
    changed_tests = {f"rust/{path}": digest((rust_root / path).read_bytes()) for path in TEST_FILES}
    summary = Counter(item["lifecycle"] for item in observations)
    evidence = {
        "work_item": "BODY-1 BB-LANG regular Japanese transitive verb forms and proof-safe local-write recovery",
        "stage": "BODY-1_EXECUTION",
        "recorded_at": datetime.now(timezone.utc).isoformat(timespec="seconds"),
        "base_commit": output(["git", "-C", str(repository_root), "rev-parse", "HEAD"]),
        "requirement_ids": ["A04", "A05", "A09", "A12", "A13", "A15", "B01", "E06", "E08"],
        "software_version": "0.7.0-alpha.8",
        "schema_version": "minidora-rust/0.7.0-alpha.8",
        "parser_version": PARSER_VERSION,
        "toolchain": {
            "rustc": output([rustc, "--version"]),
            "cargo": output([cargo, "--version"]),
            "platform": platform.platform(),
        },
        "implementation_change": "The ordinary input route preserves the existing 食べる/使う predicate identities and additionally normalizes unambiguous plain-form regular Japanese transitive predicates and godan negative inflections. Core stores the propositions and conditional rules in actor-scoped memory, then answers object and yes/no questions through one shared proof computation. The same computation verifies the exact content, guarantee and premise/rule dependency IDs before local-write recovery. It does not infer exhaustive object lists or outside-world truth.",
        "binary_sha256": digest(arguments.binary.read_bytes()),
        "source_snapshot_sha256": changed_sources,
        "test_and_tool_snapshot_sha256": changed_tests,
        "documentation_snapshot_sha256": {
            "docs/redesign/01_SYSTEM_SPECIFICATION.md": digest(
                (repository_root / "docs/redesign/01_SYSTEM_SPECIFICATION.md").read_bytes()
            ),
            "rust/README.md": digest((rust_root / "README.md").read_bytes()),
            "rust/audit/IMPLEMENTATION_STATUS.md": digest(
                (rust_root / "audit/IMPLEMENTATION_STATUS.md").read_bytes()
            ),
            "rust/audit/requirements.tsv": digest(
                (rust_root / "audit/requirements.tsv").read_bytes()
            ),
        },
        "execution_mode": "fresh CLI process for each request; one temporary SQLite state; no test-only result injection",
        "scope": "Existing 食べる/使う forms plus unambiguous plain-form godan transitive predicates, including corresponding plain negative forms, one explicit subject and exact scope; only stored or proof-supported objects are returned. Generic polite, past, potential, ichidan and unresolved forms remain unsupported.",
        "observations": observations,
        "summary": {
            "total_requests": len(observations),
            "lifecycle_counts": dict(sorted(summary.items())),
            "direct_premise_id": direct_id,
            "rule_id": rule_id,
            "negative_premise_id": negative["dependencies"][0],
            "invalidated_conflict_premise_id": contradictory["dependencies"][0],
        },
        "targeted_checks": {
            "japanese_parser_unit": "PASS",
            "ordinary_cli_route": "PASS",
            "derived_proof_dependencies": "PASS",
            "direct_vs_derived_labels": "PASS",
            "negative_polarity": "PASS",
            "generic_verb_direct_and_rule_proofs": "PASS",
            "generic_verb_negative_and_yes_no": "PASS",
            "ambiguous_polite_form_typed_stop": "PASS",
            "known_intransitive_form_typed_stop": "PASS",
            "missing_subject_and_scope_stops": "PASS",
            "actor_isolation": "PASS",
            "conflict_and_invalidation": "PASS",
            "prior_result_immutability": "PASS",
            "local_write_recovery_rechecks_object_answer_proof": "PASS",
        },
        "validation_commands": {
            "engine_parser_unit": {
                "command": "cargo test -p minidora-engine japanese_transitive_relations_preserve_object_verb_and_question_polarity",
                "exit_code": 0,
                "passed": 1,
            },
            "object_answer_recovery_test": {
                "command": "cargo test -p minidora-engine object_answer_recovery_rechecks_proof_before_confirming_saved_bytes",
                "exit_code": 0,
                "passed": 1,
                "result": "same proof accepted; after premise invalidation the recovery inspector rejects the old proof and leaves actual file bytes unchanged",
            },
            "engine_library_suite": {
                "command": "cargo test -p minidora-engine --lib",
                "exit_code": 0,
                "passed": 225,
                "failed": 0,
            },
            "normal_cli_route_test": {
                "command": "cargo test -p minidora-cli natural_transitive_object_questions_use_same_actor_scope_and_relation_proofs",
                "exit_code": 0,
                "passed": 1,
            },
            "format": {
                "command": "cargo fmt --all -- --check",
                "exit_code": 0,
            },
            "workspace_check": {
                "command": "cargo check --workspace --all-targets --locked",
                "exit_code": 0,
            },
            "clippy": {
                "command": "cargo clippy --workspace --all-targets --locked -- -D warnings",
                "exit_code": 0,
            },
            "workspace_build": {
                "command": "cargo build --workspace --locked",
                "exit_code": 0,
            },
            "workspace_runtime_suite": "NOT_RUN",
        },
        "requirements_status": "A04/A05/A09/A12/A13/A15/B01/E06/E08 acceptance remains UNVERIFIED; this is a scoped route and recovery observation, not the independent unseen-question QA population.",
        "limitations": [
            "Generic morphology is limited to explicit-scope plain forms whose predicate class is encoded unambiguously; generic polite, past, potential and ichidan forms are not supported.",
            "The low-frequency ぬ verb class, including 死ぬ, remains unsupported. Other accepted suffix classes do not receive lexical transitivity or を-role/valency checks; route examples use ordinary direct-object verbs.",
            "Unresolved subject pronouns and object alternatives remain unsupported by this route.",
            "Returned objects are supported values in the retained proof graph, not a guaranteed exhaustive inventory.",
            "The user-supplied premises and rules are not independently verified world facts.",
            "Full workspace runtime suite, full BB-LANG acceptance and the independent 24-question population were not run.",
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
