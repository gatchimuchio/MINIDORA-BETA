#!/usr/bin/env python3
"""Exercise live search-list selection, raw body fetch, summary and file recovery."""

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


PARSER_VERSION = "declarative-ja-en-v52-search-candidate-body"
QUERY = "石灰水"
HOST_GRANT = "ja.wikipedia.org"


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--evidence-output", type=Path, required=True)
    arguments = parser.parse_args()
    repository_root = Path(__file__).resolve().parents[2]
    workspace_root = repository_root / "rust"
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

    def run(
        binary: Path,
        database: Path,
        actor: str,
        case: str,
        input_id: str,
        request: str,
        extra: tuple[str, ...] = (),
        expected_exit: int = 0,
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
            timeout=25,
            check=False,
        )
        if completed.returncode != expected_exit:
            raise AssertionError(
                f"{input_id}: exit={completed.returncode}; "
                f"stdout={completed.stdout}; stderr={completed.stderr}"
            )
        return json.loads(completed.stdout)

    with tempfile.TemporaryDirectory(prefix="minidora-search-candidate-body-") as temporary:
        directory = Path(temporary)
        database = directory / "state"
        actor = "fixture-search-candidate-reader"
        case = "wikipedia-candidate-body"
        binary = arguments.binary.resolve()

        searched = run(
            binary,
            database,
            actor,
            case,
            "wikipedia-search",
            f"検索 wikipedia: {QUERY}",
            extra=("--allow-read-host", HOST_GRANT),
        )
        assert searched["lifecycle"] == "COMMIT"
        assert searched["guarantee"] == "SOURCE_REPORT"
        assert "本文は未取得" in searched["text"]
        search_ref = searched["dependencies"][0]

        with sqlite3.connect(database / "state.sqlite3") as connection:
            search_state_sha = connection.execute(
                "SELECT state_sha256 FROM case_revision WHERE case_id=? "
                "ORDER BY revision DESC LIMIT 1",
                (case,),
            ).fetchone()[0]
            search_state = json.loads(
                (database / "blobs" / search_state_sha).read_text(encoding="utf-8")
            )
        assert search_state["bundle"]["meaning"]["parser_version"] == PARSER_VERSION
        assert search_state["bundle"]["meaning"]["intent"]["kind"] == "SEARCH"

        output_path = directory / "candidate-body.txt"
        fetched = run(
            binary,
            database,
            actor,
            case,
            "fetch-first-candidate-file",
            "1件目の本文を取得して",
            extra=(
                "--allow-read-host",
                HOST_GRANT,
                "--output",
                str(output_path),
                "--allow-write",
                str(output_path),
            ),
        )
        assert fetched["lifecycle"] == "COMMIT"
        assert fetched["guarantee"] == "SOURCE_REPORT"
        assert searched["id"] in fetched["dependencies"]
        assert search_ref in fetched["dependencies"]
        assert len(fetched["evidence"]) == 1
        packet = fetched["evidence"][0]
        assert packet["packet_kind"] == "FETCHED_BODY"
        assert packet["provider_id"] == "https-direct-url-v1"
        assert packet["http_status"] == 200
        assert packet["failure"] is None
        assert packet["request_uri"].startswith(
            "https://ja.wikipedia.org/wiki/"
        )
        assert "action=raw" in packet["request_uri"]
        assert packet["raw_sha256"] == hashlib.sha256(bytes(packet["raw_body"])).hexdigest()
        assert packet["extracted_text"] == bytes(packet["raw_body"]).decode("utf-8")
        assert packet["extracted_text"].strip()
        assert packet["raw_body"]
        assert "検索候補 1:" in fetched["text"]
        assert packet["raw_sha256"] in fetched["text"]
        assert output_path.read_bytes() == fetched["text"].encode("utf-8")
        body_ref = fetched["dependencies"][2]
        assert len(fetched["dependencies"]) == 3
        original_stat = output_path.stat()

        with sqlite3.connect(database / "state.sqlite3") as connection:
            rows = connection.execute(
                "SELECT revision,state_sha256 FROM case_revision "
                "WHERE case_id=? ORDER BY revision",
                (case,),
            ).fetchall()
            adopted = None
            for revision_number, state_sha in rows:
                state = json.loads(
                    (database / "blobs" / state_sha).read_text(encoding="utf-8")
                )
                saved = state.get("result")
                if (
                    state["lifecycle"] == "ADOPTED"
                    and saved
                    and saved.get("input_id") == "fetch-first-candidate-file"
                    and saved.get("render") is not None
                ):
                    assert state["bundle"]["meaning"]["parser_version"] == PARSER_VERSION
                    assert state["bundle"]["meaning"]["intent"]["kind"] == (
                        "FETCH_PREVIOUS_SEARCH_CANDIDATE"
                    )
                    assert state["bundle"]["meaning"]["intent"]["rank"] == 1
                    assert state["bundle"]["meaning"]["context_result_ref"] == searched["id"]
                    adopted = (revision_number, saved)
            assert adopted is not None, "Core did not retain the adopted candidate-body result"
            revision_number, saved_result = adopted
            result_bytes = json.dumps(
                saved_result, ensure_ascii=False, sort_keys=True, separators=(",", ":")
            ).encode("utf-8")
            connection.executescript(
                "PRAGMA foreign_keys=ON; DROP TRIGGER case_revision_no_delete; "
                "DROP TRIGGER action_history_no_delete;"
            )
            connection.execute(
                "DELETE FROM case_revision WHERE case_id=? AND revision>?",
                (case, revision_number),
            )
            connection.execute("DELETE FROM action_history WHERE sequence>3")
            connection.execute(
                "UPDATE input_identity SET result_sha256=? "
                "WHERE case_id=? AND input_id=?",
                (
                    hashlib.sha256(result_bytes).hexdigest(),
                    case,
                    "fetch-first-candidate-file",
                ),
            )
            connection.commit()

        reconciled = subprocess.run(
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
            timeout=25,
            check=False,
        )
        if reconciled.returncode != 0:
            raise AssertionError(
                f"reconcile: exit={reconciled.returncode}; "
                f"stdout={reconciled.stdout}; stderr={reconciled.stderr}"
            )
        recovered = json.loads(reconciled.stdout)
        receipt = recovered["receipts"][-1]
        recovered_stat = output_path.stat()
        assert recovered["lifecycle"] == "COMMIT"
        assert recovered["dependencies"] == fetched["dependencies"]
        assert receipt["phase"] == "CONFIRMED"
        assert receipt["reason"] == "LOCAL_FILE_RECONCILED_NO_RESEND"
        assert (
            recovered_stat.st_ino,
            recovered_stat.st_mtime_ns,
            recovered_stat.st_size,
        ) == (original_stat.st_ino, original_stat.st_mtime_ns, original_stat.st_size)

        summarized = run(
            binary,
            database,
            actor,
            case,
            "summarize-fetched-candidate",
            "前回の本文を要約して",
        )
        assert summarized["lifecycle"] == "COMMIT"
        assert summarized["guarantee"] == "HYPOTHESIS"
        assert fetched["id"] in summarized["dependencies"]
        assert body_ref in summarized["dependencies"]
        assert packet["raw_sha256"] in summarized["text"]
        assert "ライセンス情報: 未記録" in summarized["text"]
        assert "網羅は検証していません" in summarized["text"]
        assert summarized["evidence"] == []

        with sqlite3.connect(database / "state.sqlite3") as connection:
            latest_sha = connection.execute(
                "SELECT state_sha256 FROM case_revision WHERE case_id=? "
                "ORDER BY revision DESC LIMIT 1",
                (case,),
            ).fetchone()[0]
            latest = json.loads(
                (database / "blobs" / latest_sha).read_text(encoding="utf-8")
            )
        assert latest["bundle"]["meaning"]["context_result_ref"] == fetched["id"]
        assert latest["bundle"]["meaning"]["intent"]["kind"] == (
            "SUMMARIZE_PREVIOUS_SOURCE"
        )

        relative_paths = [
            "crates/minidora-domain/src/lib.rs",
            "crates/minidora-engine/src/input.rs",
            "crates/minidora-engine/src/search.rs",
            "crates/minidora-engine/src/ports.rs",
            "crates/minidora-engine/src/core.rs",
            "crates/minidora-engine/src/recovery.rs",
            "crates/minidora-engine/src/text_summary.rs",
            "crates/minidora-engine/src/store.rs",
            "apps/cli/src/main.rs",
            "tools/search_candidate_body_route_test.py",
            "apps/cli/tests/normal_route.rs",
        ]
        source_hashes = {
            f"rust/{path}": hashlib.sha256((workspace_root / path).read_bytes()).hexdigest()
            for path in relative_paths[:8]
        }
        test_hashes = {
            f"rust/{path}": hashlib.sha256((workspace_root / path).read_bytes()).hexdigest()
            for path in relative_paths[8:]
        }
        docs = ["README.md", "audit/IMPLEMENTATION_STATUS.md", "audit/requirements.tsv"]
        documentation_hashes = {
            f"rust/{path}": hashlib.sha256((workspace_root / path).read_bytes()).hexdigest()
            for path in docs
        }
        report = {
            "work_item": "BODY-1 BB-SOURCE live search-list to selected body to summary",
            "stage": "BODY-1_EXECUTION",
            "recorded_at": datetime.now(timezone.utc).isoformat(timespec="seconds"),
            "base_commit": command_output(
                ["git", "-C", str(repository_root), "rev-parse", "HEAD"]
            ),
            "requirement_ids": ["A07", "A14", "B05", "C01", "C02", "C03", "C04", "C07", "C11", "C13", "E08", "E12"],
            "software_version": "0.7.0-alpha.8",
            "schema_version": "minidora-rust/0.7.0-alpha.8",
            "parser_version": PARSER_VERSION,
            "toolchain": {
                "rustc": command_output([rustc, "--version"]),
                "cargo": command_output([cargo, "--version"]),
                "platform": platform.platform(),
            },
            "source_snapshot_sha256": source_hashes,
            "test_and_tool_snapshot_sha256": test_hashes,
            "documentation_snapshot_sha256": documentation_hashes,
            "execution_mode": "fresh CLI process per request; LIVE public Wikipedia read; temporary actor/case state",
            "permissions": {
                "read_host_granted_on_search": HOST_GRANT,
                "read_host_granted_again_on_body_fetch": HOST_GRANT,
                "write_path_granted_for_body_receipt": True,
                "provider_or_body_instructions_do_not_add_permissions": True,
            },
            "producer_consumer": (
                "ordinary Japanese metadata search -> immutable ranked live listing -> user-selected rank -> "
                "revalidated SearchObservation and candidate -> exact-host-authorized raw Wikipedia body fetch -> "
                "immutable FETCHED_BODY packet -> following Japanese source-summary request -> source-linked extractive candidates"
            ),
            "scope": (
                "Observed Wikipedia's public search API and a separately fetched raw wikitext response. The search listing "
                "does not count as article text. The chosen body's bytes, URI, response time, packet/source hashes and exact "
                "host permission are retained. The later summary quotes extracted source text but does not establish truth, "
                "coverage or license. Raw wikitext is accepted as text/x-wiki; rendered HTML and PDF remain unsupported."
            ),
            "search_result": {
                "lifecycle": searched["lifecycle"],
                "guarantee": searched["guarantee"],
                "query": QUERY,
                "search_observation_ref": search_ref,
                "text": searched["text"],
            },
            "selected_body_result": {
                "lifecycle": fetched["lifecycle"],
                "guarantee": fetched["guarantee"],
                "context_result_ref": searched["id"],
                "dependencies": fetched["dependencies"],
                "candidate_title": search_state["result"]["text"].splitlines()[1],
                "candidate_uri": packet["request_uri"],
                "body_packet_ref": body_ref,
                "body_packet_id": packet["id"],
                "body_mode": packet["mode"],
                "body_http_status": packet["http_status"],
                "body_sha256": packet["raw_sha256"],
                "body_bytes": len(bytes(packet["raw_body"])),
                "displayed_fetch_result": fetched["text"],
            },
            "candidate_body_file_recovery": {
                "lifecycle": recovered["lifecycle"],
                "receipt_phase": receipt["phase"],
                "receipt_reason": receipt["reason"],
                "dependencies_unchanged": True,
                "file_inode_mtime_size_unchanged": True,
                "no_write_resent": True,
            },
            "summary_consumer": {
                "lifecycle": summarized["lifecycle"],
                "guarantee": summarized["guarantee"],
                "context_result_ref": fetched["id"],
                "dependencies": summarized["dependencies"],
                "text": summarized["text"],
                "exact_body_sha_referenced": packet["raw_sha256"] in summarized["text"],
            },
            "outcome": "PASS",
            "commands": [
                "cargo fmt --all -- --check",
                "cargo check --workspace --all-targets --locked",
                "cargo test -p minidora-engine search_candidate_body_request_names_one_prior_list_rank --lib --locked",
                "cargo test -p minidora-engine stored_live_listing_revalidates_candidates_and_selects_raw_wikitext_route --lib --locked",
                "cargo test -p minidora-engine unsupported_interpretations_are_not_body_success --lib --locked",
                "python3 tools/search_candidate_body_route_test.py --binary target/debug/minidora-rust --evidence-output audit/bb-source-search-candidate-body-route-validation.json",
            ],
            "not_run": [
                "full workspace runtime suite and Clippy during BODY-1",
                "Crossref and Europe PMC candidate body formats",
                "arbitrary HTML/PDF parsing, factual support/contradiction analysis and full BB-SOURCE/C01-C13 acceptance",
            ],
            "acceptance_boundary": (
                "One LIVE Wikipedia raw-wikitext candidate was fetched under explicit repeated host grants, persisted, "
                "recovered without resending a file write and consumed by the following source-summary request. Provider breadth, "
                "general source synthesis and full BB-SOURCE acceptance remain BUILD_GAP/UNVERIFIED."
            ),
        }
        arguments.evidence_output.parent.mkdir(parents=True, exist_ok=True)
        arguments.evidence_output.write_text(
            json.dumps(report, ensure_ascii=False, indent=2) + "\n", encoding="utf-8"
        )
        print(json.dumps(report, ensure_ascii=False, indent=2))


if __name__ == "__main__":
    main()
