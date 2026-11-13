#!/usr/bin/env python3
"""Observe bounded common-relation questions through fresh CLI processes."""

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


PARSER_VERSION = "declarative-ja-en-v67-common-relation-questions"


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
    with tempfile.TemporaryDirectory(prefix="minidora-common-relations-") as temporary:
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

        actor, case = "common-direct", "common-direct-case"
        coco = run(actor, case, "coco-bird", "ココは鳥")
        mike = run(actor, case, "mike-bird", "ミケは鳥")
        coco_only = run(actor, case, "coco-feathers", "ココは羽毛持ち")
        direct = run(actor, case, "common-ja", "ココとミケの共通点は何ですか")
        if direct["guarantee"] != "USER_PREMISE":
            raise AssertionError("direct common relations changed their guarantee")
        for marker in ("ココは鳥である", "ミケは鳥である", "網羅的な比較でも"):
            if marker not in direct["text"]:
                raise AssertionError(f"direct common report omitted {marker}")
        if "羽毛持ち" in direct["text"]:
            raise AssertionError("a one-sided relation was presented as common")
        for premise in (coco["dependencies"][0], mike["dependencies"][0]):
            if premise not in direct["dependencies"]:
                raise AssertionError("direct shared proof lost a subject premise")
        if coco_only["dependencies"][0] in direct["dependencies"]:
            raise AssertionError("one-sided premise leaked into the common-proof ledger")

        actor, case = "common-derived", "common-derived-case"
        rule = run(actor, case, "bird-animal-rule", "もしXは鳥ならXは動物")
        coco = run(actor, case, "derived-coco-bird", "ココは鳥")
        mike = run(actor, case, "derived-mike-bird", "ミケは鳥")
        derived = run(actor, case, "common-derived-ja", "ココとミケの共通点は何ですか")
        if derived["guarantee"] != "DERIVED":
            raise AssertionError("derived common relation was not labeled as derived")
        for marker in (
            "ココは動物である",
            "ミケは動物である",
            "適用規則:",
            rule["dependencies"][0],
            coco["dependencies"][0],
            mike["dependencies"][0],
        ):
            if marker not in derived["text"] and marker not in derived["dependencies"]:
                raise AssertionError(f"derived common proof omitted {marker}")
        if rule["dependencies"][0] not in derived["dependencies"]:
            raise AssertionError("derived common proof dependencies omitted its rule")

        actor, case = "common-english", "common-english-case"
        run(actor, case, "coco-bird-en", "Coco is a bird")
        run(actor, case, "mike-bird-en", "Mike is a bird")
        english = run(
            actor,
            case,
            "common-en",
            "What do Coco and Mike have in common?",
        )
        for marker in ("Cocoはbirdである", "Mikeはbirdである"):
            if marker not in english["text"]:
                raise AssertionError(f"English common query omitted {marker}")

        actor, case = "common-negative", "common-negative-case"
        coco_negative = run(actor, case, "coco-not-mammal", "ココは哺乳類ではない")
        mike_negative = run(actor, case, "mike-not-mammal", "ミケは哺乳類ではない")
        negative = run(actor, case, "common-negative-ja", "ココとミケの共通点は何ですか")
        for marker in ("ココは哺乳類ではない", "ミケは哺乳類ではない"):
            if marker not in negative["text"]:
                raise AssertionError(f"negative common relation omitted {marker}")
        for premise in (coco_negative["dependencies"][0], mike_negative["dependencies"][0]):
            if premise not in negative["dependencies"]:
                raise AssertionError("negative common proof lost a premise")

        actor, case = "common-missing", "common-missing-case"
        run(actor, case, "coco-only-bird", "ココは鳥")
        run(actor, case, "mike-not-bird", "ミケは鳥ではない")
        missing = run(
            actor,
            case,
            "common-no-proof",
            "ココとミケの共通点は何ですか",
            lifecycle="SUSPEND",
            exit_code=2,
        )
        if not str(missing.get("reason", "")).startswith("MISSING_PREMISES:"):
            raise AssertionError("no common proof was not reported as missing")
        if "共通点が存在しないとは断定しません" not in missing["text"]:
            raise AssertionError("missing evidence was turned into a claim of no commonality")

        actor, case = "common-scope", "common-scope-case"
        run(actor, case, "coco-biology", "[scope=biology]ココは鳥")
        run(actor, case, "mike-default", "ミケは鳥")
        wrong_scope = run(
            actor,
            case,
            "common-biology",
            "[scope=biology]ココとミケの共通点は何ですか",
            lifecycle="SUSPEND",
            exit_code=2,
        )
        if not str(wrong_scope.get("reason", "")).startswith("MISSING_PREMISES:"):
            raise AssertionError("comparison crossed relation scopes")

        actor, case = "common-conflict", "common-conflict-case"
        run(actor, case, "coco-bird", "ココは鳥")
        run(actor, case, "coco-not-bird", "ココは鳥ではない")
        run(actor, case, "mike-bird", "ミケは鳥")
        conflict = run(
            actor,
            case,
            "common-conflict",
            "ココとミケの共通点は何ですか",
            lifecycle="SUSPEND",
            exit_code=2,
        )
        if not str(conflict.get("reason", "")).startswith("CONFLICTING_PREMISES:"):
            raise AssertionError("contradictory subject evidence was not stopped")

        actor_isolated = run(
            "common-other-actor",
            "common-other-actor-case",
            "common-empty-actor",
            "ココとミケの共通点は何ですか",
            lifecycle="SUSPEND",
            exit_code=2,
        )
        if not str(actor_isolated.get("reason", "")).startswith("MISSING_PREMISES:"):
            raise AssertionError("comparison read premises outside its actor memory")

    source_paths = [
        "crates/minidora-domain/src/lib.rs",
        "crates/minidora-engine/src/core.rs",
        "crates/minidora-engine/src/input.rs",
        "crates/minidora-engine/src/relations.rs",
    ]
    test_paths = [
        "crates/minidora-engine/src/input.rs",
        "apps/cli/tests/normal_route.rs",
        "tools/common_relation_question_route_test.py",
    ]
    evidence = {
        "work_item": "BODY-1 BB-LANG bounded common-relation comparison for two named subjects",
        "stage": "BODY-1_EXECUTION",
        "recorded_at": datetime.now(timezone.utc).isoformat(timespec="seconds"),
        "base_commit": command_output(["git", "-C", str(repository_root), "rev-parse", "HEAD"]),
        "requirement_ids": ["A02", "A04", "A05", "A09", "A12", "A13", "A15", "B01"],
        "software_version": "0.7.0-alpha.8",
        "schema_version": "minidora-rust/0.7.0-alpha.8",
        "parser_version": PARSER_VERSION,
        "toolchain": {
            "rustc": command_output([rustc, "--version"]),
            "cargo": command_output([cargo, "--version"]),
            "platform": platform.platform(),
        },
        "implementation_change": "Ordinary Japanese and English commonality questions now seal two explicit subjects and a scope. Core enumerates bounded relation candidates from relevant actor premises/rules, queries both subjects under finite action budgets, and reports only propositions supported for both with direct/derived proof lineage.",
        "binary_sha256": digest(args.binary.read_bytes()),
        "source_snapshot_sha256": {
            f"rust/{path}": digest((rust_root / path).read_bytes()) for path in source_paths
        },
        "test_and_tool_snapshot_sha256": {
            f"rust/{path}": digest((rust_root / path).read_bytes()) for path in test_paths
        },
        "documentation_snapshot_sha256": {
            "docs/redesign/01_SYSTEM_SPECIFICATION.md": digest(
                (repository_root / "docs/redesign/01_SYSTEM_SPECIFICATION.md").read_bytes()
            ),
            "rust/README.md": digest((rust_root / "README.md").read_bytes()),
        },
        "execution_mode": "fresh CLI process per request, one temporary actor-scoped SQLite store, ordinary input route, no fixture injection or external effect",
        "scope": "Two distinct named subjects; shared positive or negative relation claims in one exact scope; Japanese and English commonality phrasing.",
        "observations": observations,
        "summary": {
            "total_requests": len(observations),
            "lifecycle_counts": dict(sorted(Counter(item["lifecycle"] for item in observations).items())),
        },
        "targeted_checks": {
            "direct_shared_premises_and_one_sided_exclusion": "PASS",
            "derived_shared_relation_and_rule_lineage": "PASS",
            "english_commonality_question": "PASS",
            "negative_shared_relations": "PASS",
            "missing_evidence_not_reported_as_absence": "PASS",
            "exact_scope_and_actor_isolation": "PASS",
            "conflicting_evidence_typed_stop": "PASS",
        },
        "requirements_status": "This bounded evidence-backed comparison does not establish general semantic comparison, open-domain knowledge or A02/A04/A05/A09/A12/A13/A15/B01 acceptance; those remain UNVERIFIED.",
        "limitations": [
            "Only two explicit distinct subjects and relations already represented by actor premises or rules are compared.",
            "The result lists supported shared relations only; absence from this non-exhaustive list is not evidence of a difference or no commonality.",
            "Conflicting evidence suspends. Missing evidence does not become a negative fact.",
            "Candidate scan and each pair of relation queries use the request's finite action budget.",
            "Outside-world truth, unrestricted semantic comparison and the independent acceptance population were not tested.",
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
