#!/usr/bin/env python3
"""Rebuild the blank scoring sheet and integrity manifest for this population."""

from __future__ import annotations

import csv
import hashlib
import json
from collections import Counter
from pathlib import Path


ROOT = Path(__file__).resolve().parent
FIELDS = [
    "suite",
    "item_id",
    "score_max",
    "points_earned",
    "writing_purpose_0_2",
    "writing_fidelity_0_2",
    "writing_voice_0_2",
    "writing_format_0_2",
    "writing_clarity_0_2",
    "code_sha256",
    "binary_sha256",
    "dependency_identity",
    "input_sha256",
    "output_sha256",
    "terminal_state",
    "elapsed_ms",
    "critical_error",
    "primary_rater",
    "secondary_rater",
    "adjudication_note",
]


def sha256(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def main() -> None:
    rows: list[list[str | int]] = []
    for item in range(1, 25):
        rows.append(["questions", f"Q{item:02}", 4])
    for item in range(1, 13):
        rows.append(["writing", f"W{item:02}", 10])
    for session in range(1, 11):
        for turn in range(1, 6):
            rows.append(["conversation", f"C{session:02}-T{turn:02}", 4])
    for item in range(1, 31):
        rows.append(["meaning_transformation", f"M{item:02}", 4])

    sheet = ROOT / "scoring-sheet-template.tsv"
    with sheet.open("w", encoding="utf-8", newline="") as output:
        writer = csv.writer(output, delimiter="\t", lineterminator="\n")
        writer.writerow(FIELDS)
        for suite, item_id, score_max in rows:
            writer.writerow([suite, item_id, score_max, ""] + [""] * (len(FIELDS) - 4))

    observation_paths = sorted(ROOT.glob("diagnostic-alpha*-observations.jsonl"))
    run_summaries = []
    observation_counts = Counter()
    for observation_path in observation_paths:
        observations = [json.loads(line) for line in observation_path.read_text(encoding="utf-8").splitlines()]
        run_manifest = observations[0]
        observation_records = observations[1:]
        tag = observation_path.stem.removeprefix("diagnostic-").removesuffix("-observations")
        alpha_sheet = ROOT / f"scoring-sheet-{tag}.tsv"
        with alpha_sheet.open("w", encoding="utf-8", newline="") as output:
            writer = csv.DictWriter(output, fieldnames=FIELDS, delimiter="\t", lineterminator="\n")
            writer.writeheader()
            for record in observation_records:
                writer.writerow({
                    "suite": record["suite"],
                    "item_id": record["item_id"],
                    "score_max": 10 if record["suite"] == "writing" else 4,
                    "points_earned": "",
                    "code_sha256": run_manifest["source_snapshot_sha256"],
                    "binary_sha256": run_manifest["binary_sha256"],
                    "dependency_identity": run_manifest["cargo_lock_sha256"],
                    "input_sha256": record["input_sha256"],
                    "output_sha256": record["user_visible_text_sha256"],
                    "terminal_state": record["terminal_state"],
                    "elapsed_ms": round(float(record["elapsed_seconds"]) * 1000),
                })
        counts = Counter(str(record["terminal_state"]) for record in observation_records)
        observation_counts.update(counts)
        run_summaries.append({
            "artifact": observation_path.name,
            "score_sheet": alpha_sheet.name,
            "source_commit": run_manifest["source_commit"],
            "source_snapshot_sha256": run_manifest["source_snapshot_sha256"],
            "binary_sha256": run_manifest["binary_sha256"],
            "observed_items": len(observation_records),
            "scored": False,
            "terminal_state_counts": dict(sorted(counts.items())),
        })
    population_status = "DIAGNOSTIC_RUN_NOT_SCORED" if run_summaries else "AUTHORED_NOT_RUN"

    population = {
        "suite_id": "minidora-japanese-quality-diagnostic",
        "version": "0.1.0",
        "created_date": "2026-11-09",
        "provenance": "Newly authored at the owner's request; not a reconstruction of the historical population or its unknown rubric.",
        "historical_population_recovered": False,
        "status": population_status,
        "public_fixed_diagnostic": True,
        "owner_approved_acceptance_threshold": None,
        "counts": {
            "natural_questions": 24,
            "writing_tasks": 12,
            "conversation_sessions": 10,
            "turns_per_conversation": 5,
            "scored_conversation_turns": 50,
            "meaning_transformations": 30,
            "top_level_items": 76,
            "scored_responses": 116,
            "maximum_points": 536,
        },
        "category_maximum_points": {
            "questions": 96,
            "writing": 120,
            "conversation_turns": 200,
            "meaning_transformations": 120,
        },
        "diagnostic_observations": {
            "latest_artifact": run_summaries[-1]["artifact"] if run_summaries else None,
            "scored": False,
            "cumulative_terminal_state_counts": dict(sorted(observation_counts.items())),
            "runs": run_summaries,
        },
    }
    files = [
        "README.md",
        "questions-24.md",
        "writing-12.md",
        "conversations-10.md",
        "meaning-transformations-30.md",
        "scoring-sheet-template.tsv",
        "build_assets.py",
        "run_diagnostic.py",
    ]
    for observation_path in observation_paths:
        tag = observation_path.stem.removeprefix("diagnostic-").removesuffix("-observations")
        files.extend([observation_path.name, f"scoring-sheet-{tag}.tsv"])
    population["file_sha256"] = {name: sha256(ROOT / name) for name in files}
    (ROOT / "manifest.json").write_text(
        json.dumps(population, ensure_ascii=False, indent=2) + "\n",
        encoding="utf-8",
    )


if __name__ == "__main__":
    main()
