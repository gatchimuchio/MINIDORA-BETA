#!/usr/bin/env python3
"""Crash-reconcile a previous-output text edit without resending its file write."""

import argparse
from datetime import datetime, timezone
import hashlib
import json
import os
from pathlib import Path
import shutil
import sqlite3
import subprocess
import tempfile


def canonical(value: object) -> bytes:
    return json.dumps(
        value, ensure_ascii=False, sort_keys=True, separators=(",", ":")
    ).encode("utf-8")


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, required=True)
    args = parser.parse_args()
    root = Path(__file__).resolve().parents[2]
    rust_root = root / "rust"
    binary = args.binary.resolve()
    cargo = shutil.which("cargo") or "/home/agent/.cargo/bin/cargo"

    with tempfile.TemporaryDirectory(prefix="minidora-previous-text-edit-recovery-") as temporary:
        state_dir = Path(temporary) / "state"
        environment = os.environ.copy()
        environment["MINIDORA_KEEP_TEST_STATE"] = "1"
        environment["MINIDORA_PREVIOUS_TEXT_EDIT_STATE_DIR"] = str(state_dir)
        fixture = subprocess.run(
            [
                cargo,
                "test",
                "--locked",
                "-p",
                "minidora-cli",
                "--test",
                "normal_route",
                "natural_previous_answer_edit_consumes_and_chains_actual_rendered_output",
                "--",
                "--exact",
                "--nocapture",
            ],
            cwd=rust_root,
            env=environment,
            capture_output=True,
            text=True,
            timeout=180,
            check=False,
        )
        if fixture.returncode != 0:
            raise AssertionError(
                f"fixture route failed: exit={fixture.returncode}; "
                f"stdout={fixture.stdout}; stderr={fixture.stderr}"
            )

        output_path = state_dir / "edited-answer.txt"
        original_stat = output_path.stat()
        database = state_dir / "state.sqlite3"
        with sqlite3.connect(database) as connection:
            revisions = connection.execute(
                "SELECT revision,state_sha256 FROM case_revision "
                "WHERE case_id=? ORDER BY revision",
                ("normal-route",),
            ).fetchall()
            adopted = None
            for revision, state_sha in revisions:
                saved_state = json.loads(
                    (state_dir / "blobs" / state_sha).read_text(encoding="utf-8")
                )
                result = saved_state.get("result")
                if (
                    saved_state["lifecycle"] == "ADOPTED"
                    and result
                    and result.get("input_id") == "replace-ja"
                    and result.get("render") is not None
                ):
                    adopted = (revision, result, saved_state)
            assert adopted is not None, "the edit file write must follow an adopted answer"
            revision, result, saved_state = adopted
            assert result["guarantee"] == "USER_PREMISE"
            assert result["evidence"] == []
            assert output_path.read_bytes() == result["text"].encode("utf-8")
            prior = saved_state["prior_results"][result["dependencies"][0]]
            assert prior["lifecycle"] == "COMMIT"
            plan_ref = hashlib.sha256(canonical(prior["content_plan"])).hexdigest()
            render_ref = hashlib.sha256(canonical(prior["render"])).hexdigest()
            assert result["dependencies"][1:3] == [plan_ref, render_ref]
            edit_ref = result["dependencies"][3]
            edit_bytes = (state_dir / "blobs" / edit_ref).read_bytes()
            assert hashlib.sha256(edit_bytes).hexdigest() == edit_ref
            edit = json.loads(edit_bytes)
            assert edit["operation"] == "PREVIOUS_OUTPUT_REPLACE_EXACT_V1"
            assert edit["source_result_id"] == prior["id"]
            assert edit["source_content_plan_sha256"] == plan_ref
            assert edit["source_render_sha256"] == render_ref
            assert edit["edit"]["source_document"] == prior["text"]
            assert edit["edit"]["text"] == "4+1 = 5"
            result_sha = hashlib.sha256(canonical(result)).hexdigest()
            actions = connection.execute(
                "SELECT sequence FROM action_history WHERE case_id=? ORDER BY sequence",
                ("normal-route",),
            ).fetchall()
            assert actions and any(sequence == 3 for (sequence,) in actions)
            connection.executescript(
                "PRAGMA foreign_keys=ON; DROP TRIGGER case_revision_no_delete; "
                "DROP TRIGGER action_history_no_delete;"
            )
            connection.execute(
                "DELETE FROM case_revision WHERE case_id=? AND revision>?",
                ("normal-route", revision),
            )
            connection.execute(
                "DELETE FROM action_history WHERE case_id=? AND sequence>3",
                ("normal-route",),
            )
            connection.execute(
                "UPDATE input_identity SET result_sha256=? "
                "WHERE case_id=? AND input_id=? AND input_version=1",
                (result_sha, "normal-route", "replace-ja"),
            )
            connection.commit()

        recovered = subprocess.run(
            [
                str(binary),
                "reconcile",
                "--db",
                str(state_dir),
                "--case",
                "normal-route",
                "--actor",
                "owner",
                "--json",
            ],
            capture_output=True,
            text=True,
            timeout=30,
            check=False,
        )
        if recovered.returncode != 0:
            raise AssertionError(
                f"reconcile failed: exit={recovered.returncode}; "
                f"stdout={recovered.stdout}; stderr={recovered.stderr}"
            )
        record = json.loads(recovered.stdout)
        assert record["lifecycle"] == "COMMIT"
        assert record["dependencies"] == result["dependencies"]
        receipts = record["receipts"]
        assert receipts and receipts[-1]["phase"] == "CONFIRMED"
        assert receipts[-1]["reason"] == "LOCAL_FILE_RECONCILED_NO_RESEND"
        after_stat = output_path.stat()
        before_identity = (
            original_stat.st_ino,
            original_stat.st_mtime_ns,
            original_stat.st_size,
        )
        after_identity = (after_stat.st_ino, after_stat.st_mtime_ns, after_stat.st_size)
        assert before_identity == after_identity

        print(
            json.dumps(
                {
                    "recorded_at": datetime.now(timezone.utc).isoformat(timespec="seconds"),
                    "outcome": "PASS",
                    "fixture": "actual prior calculation output and exact edit; no external source",
                    "parent_content_render_and_edit_artifacts_revalidated": True,
                    "edit_dependencies_unchanged": True,
                    "receipt_phase": receipts[-1]["phase"],
                    "receipt_reason": receipts[-1]["reason"],
                    "file_inode_mtime_size_unchanged": True,
                    "write_resent": False,
                },
                ensure_ascii=False,
                indent=2,
            )
        )


if __name__ == "__main__":
    main()
