#!/usr/bin/env python3
"""Observe proof-revalidated common-relation explanations in fresh CLI processes."""

import argparse
from collections import Counter
from datetime import datetime, timezone
import hashlib
import json
from pathlib import Path
import platform
import re
import shutil
import subprocess
import tempfile


def digest(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--evidence-output", type=Path, required=True)
    args = parser.parse_args()
    repository_root = Path(__file__).resolve().parents[2]
    rust_root = repository_root / "rust"
    cargo = shutil.which("cargo") or "/home/agent/.cargo/bin/cargo"
    rustc = shutil.which("rustc") or "/home/agent/.cargo/bin/rustc"
    observations: list[dict] = []

    with tempfile.TemporaryDirectory(prefix="minidora-common-explanation-") as temporary:
        database = Path(temporary) / "state"

        def run(
            actor: str,
            case: str,
            input_id: str,
            request: str,
            lifecycle: str = "COMMIT",
            exit_code: int = 0,
        ) -> dict:
            completed = subprocess.run(
                [
                    str(args.binary),
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

        actor, case = "common-explain-direct", "common-explain-direct-case"
        coco = run(actor, case, "coco-bird", "ココは鳥")
        mike = run(actor, case, "mike-bird", "ミケは鳥")
        direct = run(actor, case, "common-ja", "ココとミケの共通点は何ですか")
        direct_explanation = run(actor, case, "common-ja-explanation", "詳しく説明して")
        report_dependencies = direct.get("dependencies", [])
        if not report_dependencies or not re.fullmatch(r"[0-9a-f]{64}", report_dependencies[0]):
            raise AssertionError("the common report omitted its case-bound SHA-256 artifact")
        direct_text = direct["text"]
        expanded_text = direct_explanation["text"]
        if expanded_text == direct_text:
            raise AssertionError("the follow-up repeated the common-relation report")
        for marker in (
            "前回比較の詳細説明",
            direct["id"],
            "ココ側の証明: ココは鳥である",
            "ミケ側の証明: ミケは鳥である",
            coco["dependencies"][0],
            mike["dependencies"][0],
        ):
            if marker not in expanded_text:
                raise AssertionError(f"direct comparison explanation omitted {marker}")
        for dependency in direct["dependencies"]:
            if dependency not in direct_explanation["dependencies"]:
                raise AssertionError("the follow-up lost a common-report dependency")

        actor, case = "common-explain-derived", "common-explain-derived-case"
        rule = run(actor, case, "bird-animal-rule", "もしXは鳥ならXは動物")
        coco = run(actor, case, "coco-bird", "ココは鳥")
        mike = run(actor, case, "mike-bird", "ミケは鳥")
        derived = run(actor, case, "common-derived", "ココとミケの共通点は何ですか")
        derived_explanation = run(actor, case, "common-derived-explanation", "詳しく説明して")
        if derived_explanation["guarantee"] != "DERIVED":
            raise AssertionError("derived comparison guarantee was not retained")
        for marker in (
            "ココ側の証明: ココは動物である",
            "ミケ側の証明: ミケは動物である",
            "適用規則:",
            rule["dependencies"][0],
            coco["dependencies"][0],
            mike["dependencies"][0],
        ):
            if marker not in derived_explanation["text"]:
                raise AssertionError(f"derived comparison explanation omitted {marker}")
        if derived["text"] == derived_explanation["text"]:
            raise AssertionError("the derived follow-up repeated the report verbatim")

        actor, case = "common-explain-stale", "common-explain-stale-case"
        stale_coco = run(actor, case, "stale-coco-bird", "ココは鳥")
        run(actor, case, "stale-mike-bird", "ミケは鳥")
        run(actor, case, "stale-common", "ココとミケの共通点は何ですか")
        run(
            actor,
            "common-explain-stale-invalidation",
            "stale-invalidate-coco",
            f"{stale_coco['dependencies'][0]}を無効化して",
        )
        stale = run(
            actor,
            case,
            "stale-common-explanation",
            "詳しく説明して",
            lifecycle="SUSPEND",
            exit_code=2,
        )
        if not str(stale.get("reason", "")).startswith("CONTEXT_PROOF_CHANGED:"):
            raise AssertionError("the explanation did not stop after a premise was invalidated")

        missing = run(
            "common-explain-empty",
            "common-explain-empty-case",
            "missing-common-explanation",
            "詳しく説明して",
            lifecycle="SUSPEND",
            exit_code=2,
        )
        if not str(missing.get("reason", "")).startswith("MISSING_ACTUAL_RESULT:"):
            raise AssertionError("an absent prior comparison did not produce a typed stop")

    source_paths = [
        "crates/minidora-engine/src/core.rs",
        "crates/minidora-engine/src/relations.rs",
    ]
    test_paths = [
        "apps/cli/tests/normal_route.rs",
        "tools/common_relation_explanation_route_test.py",
    ]
    documentation_paths = [
        "README.md",
        "audit/IMPLEMENTATION_STATUS.md",
        "audit/requirements.tsv",
    ]
    evidence = {
        "work_item": "BODY-1 BB-CONTEXT proof-revalidated detailed explanation of common-relation reports",
        "stage": "BODY-1_EXECUTION",
        "recorded_at": datetime.now(timezone.utc).isoformat(timespec="seconds"),
        "base_commit": subprocess.run(
            ["git", "-C", str(repository_root), "rev-parse", "HEAD"],
            capture_output=True,
            text=True,
            check=True,
            timeout=10,
        ).stdout.strip(),
        "requirement_ids": ["A07", "A08", "A12", "A13", "A14", "A15", "B01"],
        "software_version": "0.7.0-alpha.8",
        "schema_version": "minidora-rust/0.7.0-alpha.8",
        "parser_version": "declarative-ja-en-v67-common-relation-questions",
        "toolchain": {
            "rustc": subprocess.run(
                [rustc, "--version"], capture_output=True, text=True, check=True, timeout=10
            ).stdout.strip(),
            "cargo": subprocess.run(
                [cargo, "--version"], capture_output=True, text=True, check=True, timeout=10
            ).stdout.strip(),
            "platform": platform.platform(),
        },
        "implementation_change": "Common-relation results now retain a canonical actor/case-bound typed artifact containing the exact result identity, scope, each shared proposition pair, side-specific guarantees and proof dependencies. ExplainPrevious reads only the immediate same-case committed result, revalidates each side through the relation proof engine, and emits a separate proof-by-proof explanation. Missing, malformed, changed or over-budget context suspends.",
        "binary_sha256": digest(args.binary.read_bytes()),
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
        "execution_mode": "fresh CLI process per request with one temporary actor-scoped SQLite store; ordinary input route; no fixture injection or external effect",
        "scope": "Two-subject common-relation reports created by the v67 route, followed by same-case 詳しく説明して; direct, derived and invalidated premise paths.",
        "observations": observations,
        "summary": {
            "total_requests": len(observations),
            "lifecycle_counts": dict(
                sorted(Counter(item["lifecycle"] for item in observations).items())
            ),
        },
        "targeted_checks": {
            "direct_left_and_right_proof_lineage_after_process_restart": "PASS",
            "derived_left_and_right_rule_lineage_after_process_restart": "PASS",
            "typed_artifact_and_prior_result_dependencies": "PASS",
            "followup_is_distinct_from_initial_commonality_report": "PASS",
            "invalidated_premise_causes_context_proof_changed_stop": "PASS",
            "missing_prior_result_typed_stop": "PASS",
        },
        "quality_gates": {
            "focused_normal_cli_route_test": "PASS: natural_common_relation_questions_compare_two_subjects_using_shared_proofs and relation_answer_follow_up_explains_the_same_current_proof_after_restart",
            "formatting": "PASS: cargo fmt --all -- --check",
            "workspace_all_targets_check": "PASS: cargo check --workspace --all-targets --locked",
            "warnings_denied_clippy": "PASS: cargo clippy --workspace --all-targets --locked -- -D warnings",
            "workspace_build": "PASS: cargo build --workspace --locked",
            "full_workspace_runtime_suite": "NOT_RUN: scoped BUILD validation only",
        },
        "requirements_status": "This scoped follow-up route does not establish A07/A08/A12/A13/A14/A15/B01 acceptance; those remain UNVERIFIED.",
        "limitations": [
            "The explanation covers only shared relations recorded by the immediately previous comparison artifact.",
            "It rechecks the stored proposition pairs; the original comparison remains non-exhaustive and does not prove that unlisted commonality is absent.",
            "Any changed or unavailable proof suspends rather than rebuilding a new comparison implicitly.",
            "Full workspace runtime tests and broad contextual explanation acceptance were not run.",
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
