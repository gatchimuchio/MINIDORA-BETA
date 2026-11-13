#!/usr/bin/env python3
"""Crash-reconcile a source-question file write without resending it."""

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


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, required=True)
    args = parser.parse_args()
    root = Path(__file__).resolve().parents[2]
    rust_root = root / "rust"
    binary = args.binary.resolve()
    cargo = shutil.which("cargo") or "/home/agent/.cargo/bin/cargo"

    with tempfile.TemporaryDirectory(prefix="minidora-source-question-recovery-") as temporary:
        state_dir = Path(temporary) / "state"
        environment = os.environ.copy()
        environment["MINIDORA_KEEP_TEST_STATE"] = "1"
        environment["MINIDORA_SOURCE_QUESTION_STATE_DIR"] = str(state_dir)
        fixture = subprocess.run(
            [
                cargo,
                "test",
                "-p",
                "minidora-cli",
                "--test",
                "normal_route",
                "natural_source_question_consumes_the_actual_prior_fetched_body",
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

        output_path = state_dir / "source-answer.txt"
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
                    and result.get("input_id") == "source-question"
                    and result.get("render") is not None
                ):
                    adopted = (revision, result, saved_state)
            assert adopted is not None, "the output write must follow an adopted answer"
            revision, result, saved_state = adopted
            assert result["guarantee"] == "HYPOTHESIS"
            prior = saved_state["prior_results"][result["dependencies"][0]]
            prior_packet_refs = {
                hashlib.sha256(
                    json.dumps(
                        packet,
                        ensure_ascii=False,
                        sort_keys=True,
                        separators=(",", ":"),
                    ).encode("utf-8")
                ).hexdigest()
                for packet in prior["evidence"]
            }
            assert result["evidence"] == []
            assert result["dependencies"][1] in prior_packet_refs
            answer_ref = result["dependencies"][-1]
            answer_bytes = (state_dir / "blobs" / answer_ref).read_bytes()
            assert hashlib.sha256(answer_bytes).hexdigest() == answer_ref
            answer = json.loads(answer_bytes)
            assert answer["source_result_id"] == prior["id"]
            assert output_path.read_bytes() == result["text"].encode("utf-8")
            result_sha = hashlib.sha256(
                json.dumps(
                    result,
                    ensure_ascii=False,
                    sort_keys=True,
                    separators=(",", ":"),
                ).encode("utf-8")
            ).hexdigest()
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
                (result_sha, "normal-route", "source-question"),
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
        receipts = record["receipts"]
        assert record["lifecycle"] == "COMMIT"
        assert record["dependencies"] == result["dependencies"]
        assert receipts and receipts[-1]["phase"] == "CONFIRMED"
        assert receipts[-1]["reason"] == "LOCAL_FILE_RECONCILED_NO_RESEND"
        after_stat = output_path.stat()
        before_identity = (original_stat.st_ino, original_stat.st_mtime_ns, original_stat.st_size)
        after_identity = (after_stat.st_ino, after_stat.st_mtime_ns, after_stat.st_size)
        assert before_identity == after_identity

        print(
            json.dumps(
                {
                    "recorded_at": datetime.now(timezone.utc).isoformat(timespec="seconds"),
                    "outcome": "PASS",
                    "fixture": "same-case synthetic committed FETCHED_BODY; no live fetch",
                    "answer_dependencies_unchanged": True,
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
