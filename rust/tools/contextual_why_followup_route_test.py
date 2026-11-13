#!/usr/bin/env python3
"""Observe elliptical why follow-ups consuming the exact prior CLI result."""

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


PARSER_VERSION = "declarative-ja-en-v66-contextual-why-followup"


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

    def command_output(command: list[str]) -> str:
        return subprocess.run(
            command, capture_output=True, text=True, timeout=10, check=True
        ).stdout.strip()

    observations: list[dict] = []
    with tempfile.TemporaryDirectory(prefix="minidora-contextual-why-") as temporary:
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

        actor = "contextual-why-direct"
        case = "contextual-why-direct-case"
        direct = run(actor, case, "robot-notebook", "ロボットはノートを使う")
        direct_answer = run(
            actor, case, "robot-notebook-question", "ロボットはノートを使いますか"
        )
        if direct["dependencies"][0] not in direct_answer["dependencies"]:
            raise AssertionError("direct relation answer lost its premise dependency")
        direct_why = run(actor, case, "why-direct-answer", "なぜですか")
        if direct_answer["id"] not in direct_why["dependencies"]:
            raise AssertionError("elliptical why did not depend on the exact prior result")
        if "前回の結論: ロボットはノートを使う" not in direct_why["text"]:
            raise AssertionError("direct follow-up omitted the prior conclusion")
        if "直接提示された前提: ロボットはノートを使う" not in direct_why["text"]:
            raise AssertionError("direct follow-up omitted its current proof")

        actor = "contextual-why-derived"
        case = "contextual-why-derived-case"
        rule = run(
            actor,
            case,
            "robot-tool-rule",
            "もしXはノートを使うならXは道具を使う",
        )
        notebook = run(actor, case, "robot-notebook", "ロボットはノートを使う")
        derived_answer = run(
            actor, case, "robot-tool-question", "ロボットは道具を使いますか"
        )
        derived_why = run(actor, case, "why-derived-answer", "どうして？")
        if derived_answer["guarantee"] != "DERIVED" or derived_why["guarantee"] != "DERIVED":
            raise AssertionError("derived follow-up lost its guarantee")
        if derived_answer["id"] not in derived_why["dependencies"]:
            raise AssertionError("derived follow-up did not consume its prior result")
        for marker in (
            "前回の結論: ロボットは道具を使う",
            "直接提示された前提: ロボットはノートを使う",
            "適用規則:",
            rule["dependencies"][0],
            notebook["dependencies"][0],
        ):
            if marker not in derived_why["text"]:
                raise AssertionError(f"derived proof lineage omitted {marker}")

        actor = "contextual-why-refuted"
        case = "contextual-why-refuted-case"
        negative = run(actor, case, "robot-not-poison", "ロボットは毒を使わない")
        refuted_answer = run(
            actor, case, "robot-poison-question", "ロボットは毒を使いますか"
        )
        if "ロボットは毒を使わない" not in refuted_answer["text"]:
            raise AssertionError("refuted yes/no answer omitted the supported opposite")
        refuted_why = run(actor, case, "why-refuted-answer", "Why?")
        if refuted_answer["id"] not in refuted_why["dependencies"]:
            raise AssertionError("refuted follow-up did not consume its prior result")
        if negative["dependencies"][0] not in refuted_why["text"]:
            raise AssertionError("refuted follow-up omitted the negative premise proof")

        actor = "contextual-why-calculation"
        case = "contextual-why-calculation-case"
        calculation = run(actor, case, "sum", "2+3")
        calculation_why = run(actor, case, "why-sum", "Why?")
        if calculation["id"] not in calculation_why["dependencies"]:
            raise AssertionError("calculation follow-up did not consume its prior result")
        if "2 + 3 = 5" not in calculation_why["text"]:
            raise AssertionError("calculation follow-up omitted the exact operation")

        missing = run(
            "contextual-why-direct",
            "contextual-why-no-prior-case",
            "why-without-prior",
            "なぜですか",
            lifecycle="SUSPEND",
            exit_code=2,
        )
        if not str(missing.get("reason", "")).startswith("MISSING_ACTUAL_RESULT:"):
            raise AssertionError("missing prior context did not produce a typed stop")
        other_actor = run(
            "contextual-why-other-actor",
            "contextual-why-direct-case",
            "why-other-actor",
            "なぜですか",
            lifecycle="REJECT",
            exit_code=3,
        )
        if not str(other_actor.get("reason", "")).startswith("CASE_ACCESS_DENIED:"):
            raise AssertionError("another actor could access the prior result")

    source_paths = [
        "crates/minidora-domain/src/lib.rs",
        "crates/minidora-engine/src/core.rs",
        "crates/minidora-engine/src/input.rs",
    ]
    test_paths = [
        "crates/minidora-engine/src/input.rs",
        "apps/cli/tests/normal_route.rs",
        "tools/contextual_why_followup_route_test.py",
    ]
    source_hashes = {
        f"rust/{path}": digest((rust_root / path).read_bytes()) for path in source_paths
    }
    test_hashes = {
        f"rust/{path}": digest((rust_root / path).read_bytes()) for path in test_paths
    }
    counts = Counter(item["lifecycle"] for item in observations)
    evidence = {
        "work_item": "BODY-1 BB-CONTEXT/BB-LANG elliptical why follow-up over actual prior results",
        "stage": "BODY-1_EXECUTION",
        "recorded_at": datetime.now(timezone.utc).isoformat(timespec="seconds"),
        "base_commit": command_output(["git", "-C", str(repository_root), "rev-parse", "HEAD"]),
        "requirement_ids": ["A04", "A07", "A08", "A12", "A13", "A14", "A15", "B01"],
        "software_version": "0.7.0-alpha.8",
        "schema_version": "minidora-rust/0.7.0-alpha.8",
        "parser_version": PARSER_VERSION,
        "toolchain": {
            "rustc": command_output([rustc, "--version"]),
            "cargo": command_output([cargo, "--version"]),
            "platform": platform.platform(),
        },
        "implementation_change": "Exact elliptical Japanese/English why forms now enter the existing ExplainPrevious consumer. It checks the immediate committed same-case result, revalidates relation proofs against current actor memory or explains the saved computation, and stops when no actual prior result exists.",
        "binary_sha256": digest(args.binary.read_bytes()),
        "source_snapshot_sha256": source_hashes,
        "test_and_tool_snapshot_sha256": test_hashes,
        "documentation_snapshot_sha256": {
            "docs/redesign/01_SYSTEM_SPECIFICATION.md": digest(
                (repository_root / "docs/redesign/01_SYSTEM_SPECIFICATION.md").read_bytes()
            ),
            "rust/README.md": digest((rust_root / "README.md").read_bytes()),
        },
        "execution_mode": "fresh CLI process for every request; one temporary SQLite store; ordinary inputs; no test-only result injection or file/network effects",
        "scope": "Immediate-result why follow-up over a supported relation answer, including derived and refuted-query answers, plus an exact prior calculation result.",
        "observations": observations,
        "summary": {
            "total_requests": len(observations),
            "lifecycle_counts": dict(sorted(counts.items())),
        },
        "targeted_checks": {
            "direct_relation_followup_and_prior_result_dependency": "PASS",
            "derived_relation_followup_and_fact_rule_lineage": "PASS",
            "refuted_question_explains_supported_opposite": "PASS",
            "calculation_followup_uses_exact_previous_result": "PASS",
            "missing_prior_result_typed_stop": "PASS",
            "actor_and_case_context_isolation": "PASS",
        },
        "requirements_status": "This bounded context route does not establish broad conversation/reference quality or A04/A07/A08/A12/A13/A14/A15/B01 acceptance; those remain UNVERIFIED.",
        "limitations": [
            "Only an immediate committed result in the same actor/case is eligible as context.",
            "Relation explanations revalidate saved proof dependencies against current memory; they do not claim outside-world truth or general causality.",
            "Non-relation result types are handled only by existing ExplainPrevious capabilities; unsupported result types remain subject to their typed stop behavior.",
            "Full workspace runtime suite, full context QA, broad language QA, and the independent user-study population were not run.",
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
