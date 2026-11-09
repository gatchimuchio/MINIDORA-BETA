#!/usr/bin/env python3
"""Run the public Japanese quality prompts through a local minidora-rust CLI."""

from __future__ import annotations

import argparse
import hashlib
import json
import platform
import re
import subprocess
import sys
import tempfile
import time
from pathlib import Path


ROOT = Path(__file__).resolve().parent
REPO = ROOT.parents[2]


def digest(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def sections(text: str, prefix: str, heading_level: int = 3) -> list[tuple[str, str]]:
    heading = "#" * heading_level
    matches = list(re.finditer(rf"^{heading} ({prefix}\d{{2}}) — .*?$", text, re.M))
    result = []
    for index, match in enumerate(matches):
        end = matches[index + 1].start() if index + 1 < len(matches) else len(text)
        result.append((match.group(1), text[match.end():end]))
    return result


def build_items() -> list[dict[str, object]]:
    items: list[dict[str, object]] = []

    questions = (ROOT / "questions-24.md").read_text(encoding="utf-8")
    for item_id, body in sections(questions, "Q"):
        match = re.search(r"\*\*Prompt:\*\*\s*(.*?)(?=\n\n\*\*Required points:\*)", body, re.S)
        if not match:
            raise ValueError(f"missing question prompt: {item_id}")
        items.append({"suite": "questions", "item_id": item_id, "case_id": item_id, "actor": f"benchmark-{item_id}", "input": match.group(1).strip()})

    writing = (ROOT / "writing-12.md").read_text(encoding="utf-8")
    for item_id, body in sections(writing, "W"):
        match = re.search(r"\*\*Prompt:\*\*\s*(.*?)(?=\n\n\*\*Item checks:\*)", body, re.S)
        if not match:
            raise ValueError(f"missing writing prompt: {item_id}")
        items.append({"suite": "writing", "item_id": item_id, "case_id": item_id, "actor": f"benchmark-{item_id}", "input": match.group(1).strip()})

    conversations = (ROOT / "conversations-10.md").read_text(encoding="utf-8")
    for session_id, body in sections(conversations, "C", heading_level=2):
        turns = list(re.finditer(r"^\*\*Turn (\d+) — User:\*\* (.*?)\s*$", body, re.M))
        if len(turns) != 5:
            raise ValueError(f"expected five user turns in {session_id}, found {len(turns)}")
        for turn in turns:
            turn_id = f"{session_id}-T{int(turn.group(1)):02}"
            items.append({"suite": "conversation", "item_id": turn_id, "case_id": session_id, "actor": f"benchmark-{session_id}", "input": turn.group(2).strip()})

    transforms = (ROOT / "meaning-transformations-30.md").read_text(encoding="utf-8")
    instruction = "文Aと文Bの違いを、変わった意味要素と両方に共通して残る要素に分けて説明してください。文から導けない事実は補わないでください。"
    for item_id, body in sections(transforms, "M"):
        match = re.search(r"\*\*A:\*\*\s*(.*?)\s+\*\*B:\*\*\s*(.*?)\s+\*\*Required observation:\*\*", body, re.S)
        if not match:
            raise ValueError(f"missing transformation pair: {item_id}")
        request = f"{instruction}\nA: {match.group(1).strip()}\nB: {match.group(2).strip()}"
        items.append({"suite": "meaning_transformation", "item_id": item_id, "case_id": item_id, "actor": f"benchmark-{item_id}", "input": request})

    expected = {"questions": 24, "writing": 12, "conversation": 50, "meaning_transformation": 30}
    actual: dict[str, int] = {}
    for item in items:
        suite = str(item["suite"])
        actual[suite] = actual.get(suite, 0) + 1
    if actual != expected:
        raise ValueError(f"population mismatch: {actual}")
    return items


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--binary", type=Path, default=Path("/workspace/.minidora-cloud/rust-target/debug/minidora-rust"))
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--timeout", type=float, default=40.0)
    args = parser.parse_args()
    binary = args.binary.resolve(strict=True)
    items = build_items()
    binary_bytes = binary.read_bytes()
    commit = subprocess.run(["git", "rev-parse", "HEAD"], cwd=REPO, check=True, capture_output=True, text=True).stdout.strip()
    worktree_status = subprocess.run(["git", "status", "--short"], cwd=REPO, check=True, capture_output=True, text=True).stdout
    code_snapshot = json.loads((REPO / "rust/audit/code-snapshot.json").read_text(encoding="utf-8"))
    lockfile = REPO / "rust/Cargo.lock"
    manifest = {
        "record_type": "run_manifest",
        "suite": "minidora-japanese-quality-diagnostic",
        "version": "0.1.0",
        "run_status": "DIAGNOSTIC_ONLY_NOT_ACCEPTANCE",
        "source_commit": commit,
        "working_tree_dirty": bool(worktree_status.strip()),
        "snapshot_binding": "source_snapshot_sha256 identifies the first-party code/test/tool snapshot used for this run; source_commit is the repository HEAD for context",
        "source_snapshot_sha256": code_snapshot["sha256"],
        "cargo_lock_sha256": digest(lockfile.read_bytes()),
        "binary_sha256": digest(binary_bytes),
        "platform": platform.platform(),
        "scoring": "not performed; use the authored rubric and independent raters",
        "external_effects": "none granted; no retrieval or provider access requested",
        "item_count": len(items),
        "started_at_utc": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
    }
    records: list[dict[str, object]] = [manifest]
    with tempfile.TemporaryDirectory(prefix="minidora-quality-alpha-") as temp:
        db = Path(temp) / "state"
        for item in items:
            prompt = str(item["input"])
            encoded = prompt.encode("utf-8")
            command = [
                str(binary),
                "--db", str(db),
                "--actor", str(item["actor"]),
                "--case", str(item["case_id"]),
                "--input", str(item["item_id"]),
                "--json",
                prompt,
            ]
            started = time.monotonic()
            try:
                completed = subprocess.run(command, cwd=REPO / "rust", capture_output=True, timeout=args.timeout)
                elapsed = time.monotonic() - started
                stdout = completed.stdout
                stderr = completed.stderr
                try:
                    result = json.loads(stdout.decode("utf-8"))
                    lifecycle = result.get("lifecycle")
                    visible_text = result.get("text")
                except (json.JSONDecodeError, UnicodeDecodeError):
                    result = None
                    lifecycle = "INVALID_OUTPUT"
                    visible_text = None
                return_code: int | str = completed.returncode
            except subprocess.TimeoutExpired as error:
                elapsed = time.monotonic() - started
                stdout = error.stdout if isinstance(error.stdout, bytes) else b""
                stderr = error.stderr if isinstance(error.stderr, bytes) else b""
                result = None
                lifecycle = "TIMEOUT"
                visible_text = None
                return_code = "TIMEOUT"
            records.append({
                "record_type": "item_observation",
                **item,
                "input_bytes": len(encoded),
                "input_sha256": digest(encoded),
                "user_visible_text_sha256": digest(str(visible_text or "").encode("utf-8")),
                "stdout_sha256": digest(stdout),
                "stderr_sha256": digest(stderr),
                "stdout_utf8": stdout.decode("utf-8", errors="replace"),
                "stderr_utf8": stderr.decode("utf-8", errors="replace"),
                "parsed_result": result,
                "terminal_state": lifecycle,
                "user_visible_text": visible_text,
                "return_code": return_code,
                "elapsed_seconds": round(elapsed, 6),
                "score": None,
                "scoring_status": "NOT_SCORED",
            })

    args.output.parent.mkdir(parents=True, exist_ok=True)
    with args.output.open("w", encoding="utf-8", newline="\n") as output:
        for record in records:
            output.write(json.dumps(record, ensure_ascii=False, separators=(",", ":")) + "\n")
    if args.output.name.startswith("diagnostic-alpha") and args.output.name.endswith("-observations.jsonl"):
        subprocess.run([sys.executable, str(ROOT / "build_assets.py")], cwd=ROOT, check=True)
    counts: dict[str, int] = {}
    for record in records[1:]:
        state = str(record["terminal_state"])
        counts[state] = counts.get(state, 0) + 1
    print(json.dumps({"output": str(args.output), "observed": len(records) - 1, "terminal_states": counts}, ensure_ascii=False))


if __name__ == "__main__":
    main()
