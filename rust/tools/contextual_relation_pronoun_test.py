#!/usr/bin/env python3
"""Exercise subject-pronoun relation questions against exact prior proof context."""

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

        def run_with_file(
            case_id: str,
            actor_id: str,
            input_id: str,
            request_text: str,
            output_path: Path,
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
                    "--output",
                    str(output_path),
                    "--allow-write",
                    str(output_path),
                    request_text,
                ],
                capture_output=True,
                text=True,
                timeout=15,
                check=False,
            )
            if completed.returncode != 0:
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
                    "operation": "authorized temporary file output",
                    "exit_code": completed.returncode,
                    "lifecycle": result.get("lifecycle"),
                    "guarantee": result.get("guarantee"),
                    "text": result.get("text"),
                    "reason": result.get("reason"),
                    "dependencies": result.get("dependencies", []),
                }
            )
            return result

        def assert_typed_relation_artifact(
            result: dict,
            expected_subject: str,
            expected_object: str,
            expected_negative: bool,
        ) -> dict:
            plan = result["content_plan"]
            artifact_ref = plan["dependencies"][-1]
            artifact_bytes = (database / "blobs" / artifact_ref).read_bytes()
            assert hashlib.sha256(artifact_bytes).hexdigest() == artifact_ref
            artifact = json.loads(artifact_bytes)
            proposition = artifact["relation_meaning"]
            assert artifact["kind"] == "minidora.core-computation.relation-meaning.v1"
            assert artifact["calculation"] is None
            assert artifact["units"] == plan["required_units"]
            assert artifact["dependencies"] == result["dependencies"]
            assert artifact["relation_proof_dependencies"] == result["dependencies"]
            assert proposition["subject"] == expected_subject
            assert proposition["object"] == expected_object
            assert proposition["negative"] is expected_negative
            assert proposition["scope"]
            return artifact

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
        ecology_bird = run(
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
        assert biology_bird["dependencies"][0] in listed["dependencies"]
        assert ecology_bird["dependencies"][0] in listed["dependencies"]
        assert rule_id in listed["dependencies"]
        assert len(listed["dependencies"]) > 2
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

        typed_actor = "typed-pronoun-followup"
        typed_case = "typed-pronoun-supported"
        typed_rule = run(
            typed_case, typed_actor, "rule", "もしXは鳥ならXは動物"
        )
        typed_bird = run(typed_case, typed_actor, "bird", "ココは鳥")
        typed_question = run(
            typed_case, typed_actor, "pronoun-answer", "それは動物ですか"
        )
        assert typed_question["lifecycle"] == "COMMIT"
        assert typed_question["guarantee"] == "DERIVED"
        assert typed_rule["dependencies"][0] in typed_question["dependencies"]
        assert typed_bird["dependencies"][0] in typed_question["dependencies"]
        typed_artifact = assert_typed_relation_artifact(
            typed_question, "ココ", "動物", False
        )

        output_path = Path(temporary) / "typed-pronoun-explanation.txt"
        explanation = run_with_file(
            typed_case,
            typed_actor,
            "explain-pronoun-answer",
            "詳しく説明して",
            output_path,
        )
        assert explanation["lifecycle"] == "COMMIT"
        assert explanation["guarantee"] == typed_question["guarantee"]
        assert explanation["dependencies"] == [
            typed_question["id"],
            *typed_question["dependencies"],
        ]
        assert "前回の結論:" in explanation["text"]
        assert "ココは動物" in explanation["text"]
        assert "適用規則" in explanation["text"]
        assert output_path.read_bytes() == explanation["text"].encode("utf-8")
        original_stat = output_path.stat()

        with sqlite3.connect(database / "state.sqlite3") as connection:
            snapshots = connection.execute(
                "SELECT revision,state_sha256 FROM case_revision "
                "WHERE case_id=? ORDER BY revision",
                (typed_case,),
            ).fetchall()
            adopted = None
            for revision_number, state_sha in snapshots:
                saved_state = json.loads(
                    (database / "blobs" / state_sha).read_text(encoding="utf-8")
                )
                saved_result = saved_state.get("result")
                if (
                    saved_state["lifecycle"] == "ADOPTED"
                    and saved_result
                    and saved_result.get("input_id") == "explain-pronoun-answer"
                    and saved_result.get("render") is not None
                ):
                    adopted = (revision_number, saved_result)
            assert adopted is not None, "the explanation output must reach ADOPTED before file confirmation"
            adopted_revision, adopted_result = adopted
            result_bytes = json.dumps(
                adopted_result,
                ensure_ascii=False,
                sort_keys=True,
                separators=(",", ":"),
            ).encode("utf-8")
            result_sha = hashlib.sha256(result_bytes).hexdigest()

            action_rows = connection.execute(
                "SELECT action_id,sequence,record_sha256 FROM action_history "
                "WHERE case_id=? ORDER BY action_id,sequence",
                (typed_case,),
            ).fetchall()
            attempted_sequences: dict[str, int] = {}
            for action_id, sequence, record_sha in action_rows:
                record_bytes = (database / "blobs" / record_sha).read_bytes()
                assert hashlib.sha256(record_bytes).hexdigest() == record_sha
                action = json.loads(record_bytes)
                if action["phase"] == "ATTEMPTED":
                    attempted_sequences[action_id] = sequence
            assert attempted_sequences, "the output action must retain its actual attempted identity"

            connection.executescript(
                "PRAGMA foreign_keys=ON; DROP TRIGGER case_revision_no_delete; "
                "DROP TRIGGER action_history_no_delete;"
            )
            connection.execute(
                "DELETE FROM case_revision WHERE case_id=? AND revision>?",
                (typed_case, adopted_revision),
            )
            for action_id, sequence in attempted_sequences.items():
                connection.execute(
                    "DELETE FROM action_history WHERE case_id=? AND action_id=? AND sequence>?",
                    (typed_case, action_id, sequence),
                )
            updated = connection.execute(
                "UPDATE input_identity SET result_sha256=? "
                "WHERE case_id=? AND input_id=? AND input_version=1",
                (result_sha, typed_case, "explain-pronoun-answer"),
            )
            assert updated.rowcount == 1
            connection.commit()

        recovered_process = subprocess.run(
            [
                str(arguments.binary),
                "reconcile",
                "--db",
                str(database),
                "--case",
                typed_case,
                "--actor",
                typed_actor,
                "--json",
            ],
            capture_output=True,
            text=True,
            timeout=30,
            check=False,
        )
        if recovered_process.returncode != 0:
            raise AssertionError(
                f"typed explanation reconcile: exit={recovered_process.returncode}; "
                f"stdout={recovered_process.stdout}; stderr={recovered_process.stderr}"
            )
        recovered = json.loads(recovered_process.stdout)
        receipt = recovered["receipts"][-1]
        recovered_stat = output_path.stat()
        assert recovered["lifecycle"] == "COMMIT"
        assert recovered["dependencies"] == explanation["dependencies"]
        assert receipt["phase"] == "CONFIRMED"
        assert receipt["reason"] == "LOCAL_FILE_RECONCILED_NO_RESEND"
        assert (
            recovered_stat.st_ino,
            recovered_stat.st_mtime_ns,
            recovered_stat.st_size,
        ) == (original_stat.st_ino, original_stat.st_mtime_ns, original_stat.st_size)
        observations.append(
            {
                "case_id": typed_case,
                "actor_id": typed_actor,
                "input_id": "explain-pronoun-answer",
                "operation": "reconcile simulated interruption after actual local write",
                "exit_code": recovered_process.returncode,
                "lifecycle": recovered["lifecycle"],
                "guarantee": recovered["guarantee"],
                "receipt_phase": receipt["phase"],
                "receipt_reason": receipt["reason"],
                "dependencies": recovered["dependencies"],
                "typed_relation_meaning_sha256": hashlib.sha256(
                    json.dumps(
                        typed_artifact,
                        ensure_ascii=False,
                        sort_keys=True,
                        separators=(",", ":"),
                    ).encode("utf-8")
                ).hexdigest(),
                "output_file_rewritten": False,
            }
        )

        refuted_actor = "typed-pronoun-refuted"
        refuted_case = "typed-pronoun-refuted-case"
        negative_premise = run(
            refuted_case,
            refuted_actor,
            "negative-animal-premise",
            "ココは動物ではない",
        )
        refuted_question = run(
            refuted_case,
            refuted_actor,
            "refuted-pronoun-answer",
            "それは動物ですか",
        )
        assert refuted_question["lifecycle"] == "COMMIT"
        assert "問われた関係「ココは動物である」は支持されず" in refuted_question["text"]
        assert negative_premise["dependencies"][0] in refuted_question["dependencies"]
        assert_typed_relation_artifact(refuted_question, "ココ", "動物", True)
        refuted_explanation = run(
            refuted_case,
            refuted_actor,
            "explain-refuted-pronoun-answer",
            "詳しく説明して",
        )
        assert refuted_explanation["lifecycle"] == "COMMIT"
        assert refuted_explanation["guarantee"] == refuted_question["guarantee"]
        assert refuted_explanation["dependencies"] == [
            refuted_question["id"],
            *refuted_question["dependencies"],
        ]
        assert "前回の結論:" in refuted_explanation["text"]
        assert "ココは動物ではない" in refuted_explanation["text"]

        stale_actor = "typed-pronoun-proof-change"
        stale_case = "typed-pronoun-stale-case"
        run(stale_case, stale_actor, "stale-rule", "もしXは鳥ならXは動物")
        run(stale_case, stale_actor, "stale-bird", "ココは鳥")
        stale_answer = run(
            stale_case, stale_actor, "stale-pronoun-answer", "それは動物ですか"
        )
        assert stale_answer["lifecycle"] == "COMMIT"
        assert_typed_relation_artifact(stale_answer, "ココ", "動物", False)
        run(
            "changed-memory-case",
            stale_actor,
            "contradicting-premise",
            "ココは動物ではない",
        )
        changed_proof = run(
            stale_case,
            stale_actor,
            "explain-after-proof-change",
            "詳しく説明して",
            expected_exit=2,
        )
        assert changed_proof["lifecycle"] == "SUSPEND"
        assert changed_proof["reason"].startswith("CONTEXT_PROOF_CHANGED:")
        assert changed_proof.get("content_plan") is None

        report = {
            "work_item": "BODY-1 Core typed prior relation meaning across AskPreviousRelation, ExplainPrevious and recovery",
            "stage": "BODY-1_EXECUTION",
            "recorded_at": datetime.now(timezone.utc).isoformat(timespec="seconds"),
            "base_commit": command_output(
                ["git", "-C", str(repository_root), "rev-parse", "HEAD"]
            ),
            "requirement_ids": ["A04", "A07", "A08", "A15", "E08", "E12", "CAP-D04", "CAP-D11"],
            "software_version": "0.7.0-alpha.8",
            "schema_version": "minidora-rust/0.7.0-alpha.8",
            "parser_version": PARSER_VERSION,
            "core_computation_artifact_kind": "minidora.core-computation.relation-meaning.v1",
            "binary_sha256": hashlib.sha256(arguments.binary.read_bytes()).hexdigest(),
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
            "execution_mode": "fresh CLI process per request; one temporary SQLite state; actual temporary local file output and crash-state reconciliation",
            "scope": "Bounded singular pronoun relation queries; Core-retained proposition consumption; relation-proof change stop; recovery of an authorized temporary file without resending.",
            "producer_consumer": (
                "same-case direct premise/rule -> sealed AskPreviousRelation -> Core resolves and adopts the final typed proposition -> "
                "versioned computation artifact -> later sealed ExplainPrevious -> Core revalidates the exact proposition and proof dependencies -> "
                "render/local file -> crash-state reconcile revalidates the same typed relation without resending"
            ),
            "targeted_checks": {
                "supported_pronoun_answer_persists_typed_proposition_and_proof_ids": "PASS",
                "ExplainPrevious_consumes_supported_adopted_proposition": "PASS",
                "contradicted_pronoun_answer_persists_supported_opposite": "PASS",
                "ExplainPrevious_consumes_contradicted_query_opposite": "PASS",
                "changed_current_proof_suspends_without_content_plan": "PASS",
                "recovery_revalidates_typed_context_and_does_not_rewrite_file": "PASS",
                "legacy_unversioned_computation_artifact_uses_text_compatibility": "PASS",
            },
            "focused_tests": {
                "previous_relation_meaning_reads_legacy_computation_artifact": "PASS",
            },
            "summary": {
                "requests_and_reconcile_observations": len(observations),
                "temporary_file_write_count": 1,
                "file_rewrite_during_recovery": False,
            },
            "rule_id": rule_id,
            "direct_premise_id": bird_id,
            "new_case_subject_premise_id": biology_bird["dependencies"][0],
            "observations": observations,
            "outcome": "PASS",
            "commands": [
                "cargo fmt --all -- --check",
                "cargo test -p minidora-engine previous_relation_meaning_reads_legacy_computation_artifact --locked",
                "cargo check --workspace --all-targets --locked",
                "cargo clippy --workspace --all-targets --locked -- -D warnings",
                "CARGO_TARGET_DIR=/workspace/.minidora-cloud/rust-target cargo build --workspace --locked",
                "python3 tools/contextual_relation_pronoun_test.py --binary /workspace/.minidora-cloud/rust-target/debug/minidora-rust --evidence-output audit/bb-context-pronoun-relation-followup-v1-route-validation.json",
                "git diff --check",
            ],
            "not_run": [
                "full workspace runtime suite",
                "plural, multi-clause and general cross-topic co-reference",
                "full A04/A07/A08/A15/E08/E12/CAP-D04/CAP-D11 acceptance",
            ],
            "acceptance_boundary": (
                "This route establishes only a bounded singular-pronoun relation answer and its immediate typed explanation/recovery consumer. "
                "It does not establish broad semantic representation across every capability, general Japanese/English co-reference, learning quality, "
                "all failure recovery, or full A04/A07/A15/BB-CONTEXT acceptance."
            ),
            "next_consumer": "The retained proposition now reaches one relation-explanation consumer; other intent families and wider meaning changes still need their own typed lineage and observation.",
        }
        arguments.evidence_output.parent.mkdir(parents=True, exist_ok=True)
        arguments.evidence_output.write_text(
            json.dumps(report, ensure_ascii=False, indent=2) + "\n", encoding="utf-8"
        )
        print(json.dumps(report, ensure_ascii=False, indent=2))


if __name__ == "__main__":
    main()
