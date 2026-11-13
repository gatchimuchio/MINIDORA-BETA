#!/usr/bin/env python3
"""Observe exact, same-case previous-result extractive summaries in fresh CLI processes."""

import argparse
from collections import Counter
from datetime import datetime, timezone
import hashlib
import json
from pathlib import Path
import platform
import shutil
import sqlite3
import subprocess
import tempfile


def digest(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def canonical(value: object) -> bytes:
    return json.dumps(
        value, ensure_ascii=False, sort_keys=True, separators=(",", ":")
    ).encode("utf-8")


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--evidence-output", type=Path, required=True)
    args = parser.parse_args()
    repository_root = Path(__file__).resolve().parents[2]
    rust_root = repository_root / "rust"
    binary = args.binary.resolve()
    cargo = shutil.which("cargo") or "/home/agent/.cargo/bin/cargo"
    rustc = shutil.which("rustc") or "/home/agent/.cargo/bin/rustc"
    observations: list[dict] = []

    def command_output(command: list[str]) -> str:
        return subprocess.run(
            command, capture_output=True, text=True, timeout=10, check=True
        ).stdout.strip()

    with tempfile.TemporaryDirectory(prefix="minidora-previous-result-summary-") as temporary:
        root = Path(temporary)
        database = root / "state"

        def run(
            actor: str,
            case: str,
            input_id: str,
            request: str,
            lifecycle: str = "COMMIT",
            exit_code: int = 0,
            extra: tuple[str, ...] = (),
        ) -> dict:
            completed = subprocess.run(
                [
                    str(binary),
                    "--db",
                    str(database),
                    "--case",
                    case,
                    "--actor",
                    actor,
                    "--input",
                    input_id,
                    "--json",
                    *extra,
                    request,
                ],
                capture_output=True,
                text=True,
                timeout=15,
                check=False,
            )
            if completed.returncode != exit_code:
                raise AssertionError(
                    f"{actor}/{case}/{input_id}: exit={completed.returncode}; "
                    f"stdout={completed.stdout}; stderr={completed.stderr}"
                )
            result = json.loads(completed.stdout)
            if result.get("lifecycle") != lifecycle:
                raise AssertionError(
                    f"{input_id}: expected {lifecycle}, got {result.get('lifecycle')}"
                )
            observations.append(
                {
                    "operation": "normal_input",
                    "actor": actor,
                    "case": case,
                    "input_id": input_id,
                    "request": request,
                    "exit_code": completed.returncode,
                    "lifecycle": result.get("lifecycle"),
                    "guarantee": result.get("guarantee"),
                    "text": result.get("text"),
                    "reason": result.get("reason"),
                    "dependencies": result.get("dependencies", []),
                }
            )
            return result

        def rollback_to_attempted_summary(
            actor: str, case: str, summary: dict
        ) -> tuple[Path, int]:
            state_path = database / "state.sqlite3"
            with sqlite3.connect(state_path) as connection:
                rows = connection.execute(
                    "SELECT revision,state_sha256 FROM case_revision "
                    "WHERE case_id=? ORDER BY revision",
                    (case,),
                ).fetchall()
                adopted = None
                for revision, state_sha in rows:
                    state = json.loads(
                        (database / "blobs" / state_sha).read_text(encoding="utf-8")
                    )
                    result = state.get("result")
                    if (
                        state.get("lifecycle") == "ADOPTED"
                        and result
                        and result.get("input_id") == summary["input_id"]
                        and result.get("id") == summary["id"]
                    ):
                        adopted = (revision, result)
                if adopted is None:
                    raise AssertionError("the retained ADOPTED summary snapshot is missing")
                adopted_revision, adopted_result = adopted
                result_sha = digest(canonical(adopted_result))
                action_ids = connection.execute(
                    "SELECT action_id FROM action_identity WHERE case_id=?",
                    (case,),
                ).fetchall()
                if len(action_ids) != 1:
                    raise AssertionError("the summary should have exactly one local file action")
                (action_id,) = action_ids[0]
                action_rows = connection.execute(
                    "SELECT sequence,record_sha256 FROM action_history "
                    "WHERE case_id=? AND action_id=? ORDER BY sequence",
                    (case, action_id),
                ).fetchall()
                attempted_sequence = None
                for sequence, record_sha in action_rows:
                    action = json.loads(
                        (database / "blobs" / record_sha).read_text(encoding="utf-8")
                    )
                    if action.get("phase") == "ATTEMPTED":
                        attempted_sequence = sequence
                if attempted_sequence is None:
                    raise AssertionError("the actual file attempt was not retained")

                connection.executescript(
                    "PRAGMA foreign_keys=ON; "
                    "DROP TRIGGER case_revision_no_delete; "
                    "DROP TRIGGER action_history_no_delete;"
                )
                connection.execute(
                    "DELETE FROM case_revision WHERE case_id=? AND revision>?",
                    (case, adopted_revision),
                )
                connection.execute(
                    "DELETE FROM action_history WHERE case_id=? AND action_id=? "
                    "AND sequence>?",
                    (case, action_id, attempted_sequence),
                )
                updated = connection.execute(
                    "UPDATE input_identity SET result_sha256=? "
                    "WHERE case_id=? AND input_id=? AND input_version=?",
                    (result_sha, case, summary["input_id"], summary["input_version"]),
                )
                if updated.rowcount != 1:
                    raise AssertionError("the exact summary input identity was not retained")
                connection.commit()
            output_path = Path(summary["receipts"][0]["target"])
            if not output_path.is_file():
                raise AssertionError("the actual summary output disappeared before recovery")
            return output_path, attempted_sequence

        def reconcile(actor: str, case: str, expected: dict, output_path: Path) -> None:
            before = output_path.stat()
            completed = subprocess.run(
                [
                    str(binary),
                    "reconcile",
                    "--db",
                    str(database),
                    "--case",
                    case,
                    "--actor",
                    actor,
                    "--json",
                ],
                capture_output=True,
                text=True,
                timeout=15,
                check=False,
            )
            if completed.returncode != 0:
                raise AssertionError(
                    f"reconcile failed: exit={completed.returncode}; "
                    f"stdout={completed.stdout}; stderr={completed.stderr}"
                )
            result = json.loads(completed.stdout)
            if result.get("lifecycle") != "COMMIT" or result.get("id") != expected["id"]:
                raise AssertionError("recovery did not commit the exact adopted summary result")
            after = output_path.stat()
            if output_path.read_bytes() != expected["text"].encode("utf-8"):
                raise AssertionError("recovery changed the original rendered summary bytes")
            if (before.st_ino, before.st_mtime_ns, before.st_size) != (
                after.st_ino,
                after.st_mtime_ns,
                after.st_size,
            ):
                raise AssertionError("recovery rewrote the already observed summary file")
            observations.append(
                {
                    "operation": "reconcile_existing_file_without_resend",
                    "actor": actor,
                    "case": case,
                    "input_id": expected["input_id"],
                    "lifecycle": result.get("lifecycle"),
                    "reason": result.get("reason"),
                    "result_id_unchanged": result.get("id") == expected["id"],
                    "file_inode_mtime_size_unchanged": True,
                }
            )

        def verify_summary_artifact(
            result: dict, source: dict, expected_database: Path
        ) -> None:
            if result.get("guarantee") != "HYPOTHESIS":
                raise AssertionError("the extractive candidate must remain HYPOTHESIS")
            dependencies = result.get("dependencies", [])
            if len(dependencies) < 4 or dependencies[1] != source["id"]:
                raise AssertionError("the summary lost its exact prior-result dependency")
            report_ref, _, content_ref, render_ref = dependencies[:4]
            report_bytes = (expected_database / "blobs" / report_ref).read_bytes()
            if digest(report_bytes) != report_ref:
                raise AssertionError("the report blob is not content addressed")
            report = json.loads(report_bytes)
            plan = source["content_plan"]
            render = source["render"]
            if report.get("summary_result_id") != result["id"]:
                raise AssertionError("the candidate report belongs to a different summary result")
            if report.get("source_result_id") != source["id"]:
                raise AssertionError("the candidate report belongs to a different prior result")
            if report.get("source_content_plan_id") != plan["id"]:
                raise AssertionError("the candidate report lost its exact source ContentPlan")
            if report.get("source_render_sha256") != render["actual_sha256"]:
                raise AssertionError("the candidate report lost its actual rendered source")
            if report["summary"].get("source_text") != source["text"]:
                raise AssertionError("the extractor did not preserve the prior displayed text")
            if report["summary"].get("source_text_sha256") != digest(
                source["text"].encode("utf-8")
            ):
                raise AssertionError("the extractor source hash differs from the prior result")
            content_bytes = (expected_database / "blobs" / content_ref).read_bytes()
            render_bytes = (expected_database / "blobs" / render_ref).read_bytes()
            if digest(content_bytes) != content_ref or json.loads(content_bytes) != plan:
                raise AssertionError("the retained source ContentPlan blob differs from the result")
            if digest(render_bytes) != render_ref or json.loads(render_bytes) != render:
                raise AssertionError("the retained actual render blob differs from the result")
            summary_unit = result["content_plan"]["required_units"][0]
            if report.get("summary_unit_sha256") != digest(summary_unit.encode("utf-8")):
                raise AssertionError("the stored output unit differs from its typed candidate report")
            for dependency in source.get("dependencies", []):
                if dependency not in dependencies:
                    raise AssertionError("the summary dropped a source result dependency")

        direct_actor, direct_case = "summary-direct-owner", "summary-direct-case"
        coco = run(direct_actor, direct_case, "coco-bird", "ココは鳥")
        mike = run(direct_actor, direct_case, "mike-bird", "ミケは鳥")
        direct = run(
            direct_actor,
            direct_case,
            "common-direct",
            "ココとミケの共通点は何ですか",
        )
        if not set((coco["dependencies"][0], mike["dependencies"][0])).issubset(
            set(direct["dependencies"])
        ):
            raise AssertionError("the direct comparison did not retain both premise IDs")
        direct_output = root / "direct-summary.txt"
        direct_summary = run(
            direct_actor,
            direct_case,
            "summary-direct",
            "前回の比較を2文以内で要約して",
            extra=(
                "--output",
                str(direct_output),
                "--allow-write",
                str(direct_output),
            ),
        )
        verify_summary_artifact(direct_summary, direct, database)
        direct_text = direct_summary["text"]
        for marker in (
            "前回結果からの抽出候補",
            "目的・意味・条件・否定の網羅は検証していません",
        ):
            if marker not in direct_text:
                raise AssertionError(f"summary disclosure omitted {marker}")
        if direct_text == direct["text"]:
            raise AssertionError("the summary did not produce a separate candidate result")
        if not direct_output.is_file() or direct_output.read_text(encoding="utf-8") != direct_text:
            raise AssertionError("the exact approved file does not contain the rendered summary")
        if not direct_summary.get("receipts") or direct_summary["receipts"][0].get(
            "phase"
        ) != "CONFIRMED":
            raise AssertionError("the direct summary did not retain its confirmed file receipt")

        output_path, attempted_sequence = rollback_to_attempted_summary(
            direct_actor, direct_case, direct_summary
        )
        reconcile(direct_actor, direct_case, direct_summary, output_path)
        explanation = run(
            direct_actor, direct_case, "explain-direct-summary", "詳しく説明して"
        )
        explanation_text = explanation["text"]
        for marker in (
            "前回結果の要約説明",
            direct["id"],
            "選択文",
            "元結果byte",
            "要約の網羅性・意味・条件保持は未検証",
        ):
            if marker not in explanation_text:
                raise AssertionError(f"the follow-up omitted source-span evidence: {marker}")
        if explanation["guarantee"] != "HYPOTHESIS":
            raise AssertionError("the explanation changed the candidate's unverified guarantee")
        for dependency in direct_summary["dependencies"]:
            if dependency not in explanation["dependencies"]:
                raise AssertionError("the explanation lost a summary or source dependency")
        with sqlite3.connect(database / "state.sqlite3") as connection:
            latest_revision = connection.execute(
                "SELECT state_sha256 FROM case_revision WHERE case_id=? "
                "ORDER BY revision DESC LIMIT 1",
                (direct_case,),
            ).fetchone()
            latest = json.loads(
                (database / "blobs" / latest_revision[0]).read_text(encoding="utf-8")
            )
        if direct["id"] not in latest["prior_results"] or direct_summary["id"] not in latest[
            "prior_results"
        ]:
            raise AssertionError("summary and source history were not retained after explanation")

        derived_actor, derived_case = "summary-derived-owner", "summary-derived-case"
        rule = run(
            derived_actor,
            derived_case,
            "bird-animal-rule",
            "もしXは鳥ならXは動物",
        )
        derived_coco = run(derived_actor, derived_case, "coco-bird", "ココは鳥")
        derived_mike = run(derived_actor, derived_case, "mike-bird", "ミケは鳥")
        derived = run(
            derived_actor,
            derived_case,
            "common-derived",
            "ココとミケの共通点は何ですか",
        )
        if derived["guarantee"] != "DERIVED":
            raise AssertionError("the source report did not exercise derived proof lineage")
        derived_summary = run(
            derived_actor,
            derived_case,
            "summary-derived",
            "前回の比較を要約して",
        )
        verify_summary_artifact(derived_summary, derived, database)
        if not set((rule["dependencies"][0], derived_coco["dependencies"][0], derived_mike[
            "dependencies"
        ][0])).issubset(set(derived_summary["dependencies"])):
            raise AssertionError("the derived comparison's rule and premise lineage was lost")
        derived_explanation = run(
            derived_actor, derived_case, "explain-derived-summary", "詳しく説明して"
        )
        if derived["id"] not in derived_explanation["text"]:
            raise AssertionError("derived summary explanation lost its exact source result")
        for dependency in derived_summary["dependencies"]:
            if dependency not in derived_explanation["dependencies"]:
                raise AssertionError("derived explanation lost a source dependency")

        english_actor, english_case = "summary-english-owner", "summary-english-case"
        calculation = run(english_actor, english_case, "calculation", "2+3")
        english_summary = run(
            english_actor,
            english_case,
            "summary-english",
            "Summarize the previous result in up to 2 sentences",
        )
        verify_summary_artifact(english_summary, calculation, database)
        if english_summary["guarantee"] != "HYPOTHESIS":
            raise AssertionError("the English request did not retain HYPOTHESIS")
        english_explanation = run(
            english_actor, english_case, "explain-english-summary", "詳しく説明して"
        )
        if calculation["id"] not in english_explanation["text"]:
            raise AssertionError("the English request's source result was lost on follow-up")

        budget_actor, budget_case = "summary-budget-owner", "summary-budget-case"
        budget_source = run(
            budget_actor,
            budget_case,
            "source-candidates",
            'text-summary:\n{"text":"First sentence. Second sentence.","max_sentences":2}',
        )
        budget_stop = run(
            budget_actor,
            budget_case,
            "summary-over-budget",
            "前回の結果を要約して",
            lifecycle="SUSPEND",
            exit_code=2,
            extra=("--max-actions", "1"),
        )
        if not str(budget_stop.get("reason", "")).startswith("RESOURCE_LIMIT:"):
            raise AssertionError("a summary exceeding its source-scan budget was adopted")
        if budget_stop.get("dependencies"):
            raise AssertionError("an over-budget summary adopted an output dependency")

        missing = run(
            direct_actor,
            "summary-empty-case",
            "cross-case-summary",
            "前回の結果を要約して",
            lifecycle="SUSPEND",
            exit_code=2,
        )
        if not str(missing.get("reason", "")).startswith("MISSING_ACTUAL_RESULT:"):
            raise AssertionError("a prior result from another case was read")

    source_paths = [
        "crates/minidora-domain/src/lib.rs",
        "crates/minidora-engine/src/input.rs",
        "crates/minidora-engine/src/core.rs",
        "crates/minidora-engine/src/text_summary.rs",
        "crates/minidora-engine/src/recovery.rs",
    ]
    test_paths = [
        "apps/cli/tests/normal_route.rs",
        "crates/minidora-engine/src/recovery.rs",
        "tools/previous_result_summary_route_test.py",
    ]
    documentation_paths = [
        "README.md",
        "audit/IMPLEMENTATION_STATUS.md",
        "audit/requirements.tsv",
    ]
    evidence = {
        "work_item": "BODY-1 BB-CONTENT/BB-CONTEXT exact previous-result extractive summary and source-span follow-up",
        "stage": "BODY-1_EXECUTION",
        "recorded_at": datetime.now(timezone.utc).isoformat(timespec="seconds"),
        "base_commit": command_output(["git", "-C", str(repository_root), "rev-parse", "HEAD"]),
        "requirement_ids": ["A07", "A08", "B08", "E01", "E02", "F05"],
        "software_version": "0.7.0-alpha.8",
        "schema_version": "minidora-rust/0.7.0-alpha.8",
        "parser_version": "declarative-ja-en-v68-previous-result-summary",
        "toolchain": {
            "rustc": command_output([rustc, "--version"]),
            "cargo": command_output([cargo, "--version"]),
            "platform": platform.platform(),
        },
        "implementation_change": "Ordinary bounded Japanese and English previous-result summary requests now consume only the exact immediate same-case committed result, revalidate its ContentPlan and actual render, and run the existing deterministic sentence extractor over the prior displayed text. A typed report binds both result identities, source content/render, source-text hash, selected source byte spans and the exact output unit. The result remains HYPOTHESIS. The next 詳しく説明して rechecks the source and report, explains selected spans, and retains every dependency. File reconciliation recomputes the same context and observes the already written exact bytes without resending.",
        "binary_sha256": digest(binary.read_bytes()),
        "source_snapshot_sha256": {
            f"rust/{path}": digest((rust_root / path).read_bytes()) for path in source_paths
        },
        "test_and_tool_snapshot_sha256": {
            f"rust/{path}": digest((rust_root / path).read_bytes()) for path in test_paths
        },
        "documentation_snapshot_sha256": {
            f"rust/{path}": digest((rust_root / path).read_bytes())
            for path in documentation_paths
        },
        "execution_mode": "fresh CLI process per input with temporary actor/case-scoped SQLite and content-addressed blobs; no fixture injection into language/core routes; one controlled SQLite rollback models a crash after the actual summary-file attempt and before confirmation",
        "scope": "Direct and derived common-relation results, one bounded Japanese request, same-case span explanation, exact local file receipt, no-resend reconciliation, history retention and cross-case stop. This is scoped implementation evidence only.",
        "observations": observations,
        "summary": {
            "total_fresh_cli_operations": len(observations),
            "lifecycle_counts": dict(
                sorted(
                    Counter(item["lifecycle"] for item in observations if "lifecycle" in item).items()
                )
            ),
            "crash_recovery_retained_attempt_sequence": attempted_sequence,
        },
        "targeted_checks": {
            "direct_report_result_content_render_and_source_text_hash_bound": "PASS",
            "english_previous_result_summary_normal_route": "PASS",
            "derived_comparison_preserves_rule_and_both_premises": "PASS",
            "natural_previous_comparison_summary_is_separate_and_hypothesis": "PASS",
            "selected_source_spans_explained_on_following_turn": "PASS",
            "exact_local_file_receipt_and_crash_reconcile_without_resend": "PASS",
            "file_inode_mtime_and_size_unchanged_during_reconcile": "PASS",
            "previous_result_history_preserved_after_explanation": "PASS",
            "cross_case_previous_result_rejected": "PASS",
            "source_sentence_scan_budget_stop_without_artifact": "PASS",
        },
        "quality_gates": {
            "previous_result_summary_input_test": "PASS",
            "previous_result_summary_normal_cli_and_exact_file_receipt_test": "PASS",
            "previous_result_summary_action_budget_stop_test": "PASS",
            "previous_result_summary_recovery_context_test": "PASS",
            "legacy_previous_source_summary_input_test": "PASS",
            "legacy_constrained_previous_source_summary_cli_regression": "PASS",
            "formatting": "PASS: cargo fmt --all -- --check",
            "workspace_all_targets_check": "PASS: cargo check --workspace --all-targets --locked",
            "warnings_denied_clippy": "PASS: cargo clippy --workspace --all-targets --locked -- -D warnings",
            "workspace_build": "PASS: cargo build --workspace --locked",
            "full_workspace_runtime_suite": "NOT_RUN: scoped BUILD validation only",
        },
        "requirements_status": "The scoped route does not establish A07, A08, B08, E01, E02 or F05 acceptance. All remain UNVERIFIED.",
        "limitations": [
            "The route summarizes only the immediately previous committed result in the same actor and case; it does not search earlier history or other cases.",
            "Sentence selection is deterministic and extractive. Purpose, meaning, conditions, negation and coverage remain unverified.",
            "The selected byte spans refer to UTF-8 bytes in the exact prior displayed result text, not to a verified semantic model of its claims.",
            "The follow-up covers selected source sentences and does not assert that omitted text is irrelevant or that the source result is true.",
            "Full workspace runtime tests and broad A07/A08/B08/E01/E02/F05 acceptance were not run.",
        ],
    }
    args.evidence_output.parent.mkdir(parents=True, exist_ok=True)
    args.evidence_output.write_text(
        json.dumps(evidence, ensure_ascii=False, indent=2) + "\n", encoding="utf-8"
    )
    print(
        json.dumps(
            {
                "evidence": str(args.evidence_output),
                "summary": evidence["summary"],
                "targeted_checks": evidence["targeted_checks"],
            },
            ensure_ascii=False,
            indent=2,
        )
    )


if __name__ == "__main__":
    main()
