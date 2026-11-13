#!/usr/bin/env python3
"""Observe scoped saved-relation comparison through fresh CLI processes."""

from collections import Counter
from datetime import datetime, timezone
import argparse
import hashlib
import json
from pathlib import Path
import platform
import shutil
import subprocess
import tempfile


PARSER_VERSION = "declarative-ja-en-v71-saved-relation-comparison"


def digest(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--evidence-output", type=Path, required=True)
    args = parser.parse_args()
    binary = args.binary.resolve()
    repository_root = Path(__file__).resolve().parents[2]
    git = shutil.which("git") or "git"
    base_commit = subprocess.run(
        [git, "rev-parse", "HEAD"],
        cwd=repository_root,
        capture_output=True,
        text=True,
        timeout=10,
        check=True,
    ).stdout.strip()
    snapshot_path = repository_root / "rust" / "audit" / "code-snapshot.json"
    source_snapshot_sha256 = json.loads(snapshot_path.read_text(encoding="utf-8"))["sha256"]
    observations = []

    with tempfile.TemporaryDirectory(prefix="minidora-saved-relation-comparison-") as temporary:
        database = Path(temporary) / "state"

        def run(actor: str, case: str, input_id: str, request: str, lifecycle: str) -> dict:
            expected_exit = 0 if lifecycle == "COMMIT" else 2
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
                    request,
                ],
                capture_output=True,
                text=True,
                timeout=15,
                check=False,
            )
            if completed.returncode != expected_exit:
                raise AssertionError(
                    f"{actor}/{case}/{input_id}: exit={completed.returncode}; "
                    f"stdout={completed.stdout}; stderr={completed.stderr}"
                )
            result = json.loads(completed.stdout)
            if result.get("lifecycle") != lifecycle:
                raise AssertionError(
                    f"{input_id}: expected {lifecycle}, got {result.get('lifecycle')}"
                )
            observation = {
                "actor": actor,
                "case": case,
                "input_id": input_id,
                "request": request,
                "request_sha256": digest(request.encode("utf-8")),
                "exit_code": completed.returncode,
                "stdout_sha256": digest(completed.stdout.encode("utf-8")),
                "lifecycle": result.get("lifecycle"),
                "guarantee": result.get("guarantee"),
                "text": result.get("text"),
                "reason": result.get("reason"),
                "dependencies": result.get("dependencies", []),
                "result_id": result.get("id"),
            }
            observations.append(observation)
            return result

        actor, case = "relation-compare-owner", "relation-compare-case"
        rule = run(
            actor,
            case,
            "biology-rule",
            "もし[scope=biology]Xは鳥なら[scope=biology]Xは動物",
            "COMMIT",
        )
        coco_bird = run(
            actor, case, "coco-bird", "[scope=biology]ココは鳥", "COMMIT"
        )
        mike_bird = run(
            actor, case, "mike-bird", "[scope=biology]ミケは鳥", "COMMIT"
        )
        coco_feathers = run(
            actor,
            case,
            "coco-feathers",
            "[scope=biology]ココは羽毛持ち",
            "COMMIT",
        )
        mike_cat = run(
            actor, case, "mike-cat", "[scope=biology]ミケは猫", "COMMIT"
        )
        run(
            actor,
            case,
            "other-scope",
            "[scope=fiction]ココは王族",
            "COMMIT",
        )
        comparison = run(
            actor,
            case,
            "compare-relation-inventories",
            "[scope=biology]ココとミケの保存済みの関係を比較して",
            "COMMIT",
        )
        comparison_text = comparison["text"]
        for marker in (
            "保存記録の網羅性",
            "ココは鳥である",
            "ミケは鳥である",
            "ココは動物である",
            "ミケは動物である",
            "羽毛持ち",
            "ミケは猫である",
            "規則からの導出",
        ):
            if marker not in comparison_text:
                raise AssertionError(f"comparison omitted {marker}")
        if "王族" in comparison_text:
            raise AssertionError("comparison crossed the requested relation scope")
        for source in (rule, coco_bird, mike_bird, coco_feathers, mike_cat):
            if not set(source["dependencies"]).issubset(comparison["dependencies"]):
                raise AssertionError("comparison lost a source fact or rule dependency")

        summary = run(
            actor,
            case,
            "summarize-relation-comparison",
            "前回の比較を2文以内で要約して",
            "COMMIT",
        )
        if comparison["id"] not in summary["dependencies"]:
            raise AssertionError("previous-result summary lost its comparison source")
        if not set(comparison["dependencies"]).issubset(summary["dependencies"]):
            raise AssertionError("previous-result summary lost comparison evidence")

        conflict_actor, conflict_case = (
            "relation-compare-conflict-owner",
            "relation-compare-conflict-case",
        )
        coco_positive = run(
            conflict_actor,
            conflict_case,
            "coco-bird-positive",
            "[scope=biology]ココは鳥",
            "COMMIT",
        )
        coco_negative = run(
            conflict_actor,
            conflict_case,
            "coco-bird-negative",
            "[scope=biology]ココは鳥ではない",
            "COMMIT",
        )
        mike_positive = run(
            conflict_actor,
            conflict_case,
            "mike-bird-positive",
            "[scope=biology]ミケは鳥",
            "COMMIT",
        )
        conflicting_comparison = run(
            conflict_actor,
            conflict_case,
            "compare-conflicting-relations",
            "[scope=biology]ココとミケの関係を比較して",
            "COMMIT",
        )
        conflict_text = conflicting_comparison["text"]
        for marker in (
            "競合は解消せず",
            "競合根拠を含む関係",
            "ココは鳥である",
            "ココは鳥ではない",
        ):
            if marker not in conflict_text:
                raise AssertionError(f"conflict report omitted {marker}")
        if "両側で確認した同じ関係: ココは鳥である" in conflict_text:
            raise AssertionError("conflicting support was reported as unambiguous")
        for source in (coco_positive, coco_negative, mike_positive):
            if not set(source["dependencies"]).issubset(
                conflicting_comparison["dependencies"]
            ):
                raise AssertionError("conflict comparison lost a premise dependency")

        missing = run(
            "relation-compare-other-actor",
            "relation-compare-empty-case",
            "no-saved-relations",
            "ココとミケの関係を比較して",
            "SUSPEND",
        )
        if not str(missing.get("reason", "")).startswith("MISSING_PREMISES:"):
            raise AssertionError("empty actor memory did not stop with MISSING_PREMISES")

    counts = Counter(observation["lifecycle"] for observation in observations)
    args.evidence_output.parent.mkdir(parents=True, exist_ok=True)
    evidence = {
        "work_item": "BODY-1 BB-LANG saved-relation comparison and previous-result summary",
        "recorded_at": datetime.now(timezone.utc).isoformat(),
        "base_commit": base_commit,
        "source_snapshot_sha256": source_snapshot_sha256,
        "subject": "MINIDORA-BETA engine",
        "purpose": "Compare two actor-scoped scoped relation inventories in ordinary Japanese, retain both proof lineages, and pass the committed result to the same-case previous-result summary consumer.",
        "parser_version": PARSER_VERSION,
        "execution_mode": "each request ran as a separate process of the supplied CLI binary against one temporary SQLite state directory; the directory was removed on exit",
        "platform": platform.platform(),
        "binary_sha256": digest(binary.read_bytes()),
        "observation_count": len(observations),
        "lifecycle_counts": dict(sorted(counts.items())),
        "route_assertions": {
            "requested_scope_only": True,
            "shared_direct_and_rule_derived_relations": True,
            "one_sided_saved_support_is_labeled_without_claiming_absence": True,
            "source_fact_and_rule_dependencies_retained": True,
            "same_case_previous_result_summary_consumed_comparison": True,
            "conflicting_polarities_are_retained_without_unambiguous_classification": True,
            "other_actor_without_memory_suspended": True,
        },
        "observations": observations,
        "requirements": ["A02", "A09", "B01", "B08"],
        "acceptance_status": "UNVERIFIED",
        "limitations": [
            "Only explicit Japanese and English saved-relation comparison forms are supported.",
            "One-sided output means support was not found in the other bounded scoped inventory; it does not prove that the relation is false or absent.",
            "Relation records and rules are user-supplied premises; outside-world truth, memory completeness, open-domain comparison and general writing quality are not established.",
            "Full workspace runtime tests and full R2 acceptance were not run.",
        ],
    }
    args.evidence_output.write_text(
        json.dumps(evidence, ensure_ascii=False, indent=2) + "\n", encoding="utf-8"
    )
    print(
        json.dumps(
            {
                "evidence": str(args.evidence_output),
                "observations": len(observations),
                "lifecycle_counts": dict(sorted(counts.items())),
                "comparison_result_id": comparison["id"],
                "comparison_artifact_sha256": comparison["dependencies"][0],
                "summary_result_id": summary["id"],
            },
            ensure_ascii=False,
            indent=2,
        )
    )


if __name__ == "__main__":
    main()
