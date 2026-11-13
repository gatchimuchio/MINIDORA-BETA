#!/usr/bin/env python3
"""Observe a saved-relation profile, local effect and follow-up in fresh CLI processes."""

import argparse
from collections import Counter
import csv
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
    cargo = shutil.which("cargo") or "/workspace/.cargo/bin/cargo"
    rustc = shutil.which("rustc") or "/workspace/.cargo/bin/rustc"
    source_paths = [
        "crates/minidora-domain/src/lib.rs",
        "crates/minidora-engine/src/core.rs",
        "crates/minidora-engine/src/input.rs",
        "crates/minidora-engine/src/language.rs",
        "crates/minidora-engine/src/recovery.rs",
        "crates/minidora-engine/src/relations.rs",
    ]
    test_paths = [
        "apps/cli/tests/normal_route.rs",
        "tools/known_relation_profile_route_test.py",
    ]
    documentation_paths = [
        "README.md",
        "audit/IMPLEMENTATION_STATUS.md",
        "audit/requirements.tsv",
        "docs/IMPLEMENTATION_CHOICES.md",
        "audit/construction/MB-BUILD-20261113/MANIFEST.tsv",
    ]

    def command_output(command: list[str]) -> str:
        return subprocess.run(
            command, capture_output=True, text=True, timeout=10, check=True
        ).stdout.strip()

    def sha256_paths(paths: list[str]) -> dict[str, str]:
        return {
            f"rust/{path}": hashlib.sha256((rust_root / path).read_bytes()).hexdigest()
            for path in paths
        }

    base_commit = command_output(
        ["git", "-C", str(repository_root), "rev-parse", "HEAD"]
    )
    toolchain = {
        "rustc": command_output([rustc, "--version"]),
        "cargo": command_output([cargo, "--version"]),
        "platform": platform.platform(),
    }
    manifest_path = rust_root / "audit/construction/MB-BUILD-20261113/MANIFEST.tsv"
    with manifest_path.open(encoding="utf-8", newline="") as source:
        build_manifest = list(csv.DictReader(source, delimiter="\t"))
    build_set = [
        {
            "artifact_name": row["artifact_name"],
            "sha256": row["sha256"],
            "bytes": int(row["bytes"]),
            "role": row["role"],
            "repository_path": row["repository_path"],
        }
        for row in build_manifest
        if row["role"] in {
            "original_user_archive",
            "build_specification",
            "build_schedule",
            "codex_work_instruction",
        }
    ]
    observations: list[dict] = []

    with tempfile.TemporaryDirectory(prefix="minidora-known-relation-profile-") as temporary:
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
                    "request_sha256": digest(request.encode("utf-8")),
                    "exit_code": completed.returncode,
                    "lifecycle": result.get("lifecycle"),
                    "guarantee": result.get("guarantee"),
                    "text": result.get("text"),
                    "reason": result.get("reason"),
                    "dependencies": result.get("dependencies", []),
                }
            )
            return result

        def rollback_to_attempted_file(
            actor: str, case: str, result: dict
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
                    saved = state.get("result")
                    if (
                        state.get("lifecycle") == "ADOPTED"
                        and saved
                        and saved.get("input_id") == result["input_id"]
                        and saved.get("id") == result["id"]
                    ):
                        adopted = (revision, saved)
                if adopted is None:
                    raise AssertionError("the retained ADOPTED profile snapshot is missing")
                adopted_revision, adopted_result = adopted
                result_sha = digest(canonical(adopted_result))
                action_ids = connection.execute(
                    "SELECT action_id FROM action_identity WHERE case_id=?",
                    (case,),
                ).fetchall()
                if len(action_ids) != 1:
                    raise AssertionError("the profile should have exactly one local file action")
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
                    raise AssertionError("the actual local file attempt was not retained")

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
                    (
                        result_sha,
                        case,
                        result["input_id"],
                        result["input_version"],
                    ),
                )
                if updated.rowcount != 1:
                    raise AssertionError("the exact profile input identity was not retained")
                connection.commit()
            output_path = Path(result["receipts"][0]["target"])
            if not output_path.is_file():
                raise AssertionError("the actual profile output disappeared before recovery")
            return output_path, attempted_sequence

        actor, case = "profile-owner", "known-profile-case"
        rule = run(actor, case, "bird-animal-rule", "もしXは鳥ならXは動物")
        bird = run(actor, case, "coco-bird", "ココは鳥")
        output_path = root / "coco-profile.txt"
        profile = run(
            actor,
            case,
            "coco-profile",
            "ココについて保存されている情報をもとに短い紹介文を書いて",
            extra=("--output", str(output_path), "--allow-write", str(output_path)),
        )
        text = profile["text"]
        if profile.get("guarantee") != "HYPOTHESIS":
            raise AssertionError("a saved-fact profile must remain HYPOTHESIS")
        for marker in (
            "会話で提示された前提は「ココは鳥である」",
            "保存された規則による導出は「ココは動物である」",
            "現実世界の真偽や記録の網羅性は確認していません",
        ):
            if marker not in text:
                raise AssertionError(f"the profile omitted a source or disclosure unit: {marker}")
        if bird["dependencies"][0] not in profile["dependencies"]:
            raise AssertionError("the profile lost its direct premise dependency")
        if rule["dependencies"][0] not in profile["dependencies"]:
            raise AssertionError("the profile lost its rule dependency")
        derived_line = next(line for line in text.splitlines() if "導出ID: " in line)
        derived_id = derived_line.split("導出ID: ", 1)[1].split(";", 1)[0]
        if derived_id not in profile["dependencies"]:
            raise AssertionError("the profile lost its derived-fact identity")
        if not output_path.is_file() or output_path.read_bytes() != text.encode("utf-8"):
            raise AssertionError("the exact approved file does not contain the rendered profile")
        receipts = profile.get("receipts", [])
        if not receipts or receipts[0].get("phase") != "CONFIRMED":
            raise AssertionError("the profile did not retain a confirmed local file receipt")

        plan = profile["content_plan"]
        emission = json.loads(
            (database / "blobs" / plan["model_observation_ref"]).read_text(
                encoding="utf-8"
            )
        )
        surfaces = emission["authorized_surfaces"]
        if (
            len(surfaces) != 2
            or emission.get("text") != text
            or text not in surfaces
            or emission.get("decoder_version") != "bounded-finite-surface-literal-exact-v1"
        ):
            raise AssertionError("native model output did not select a retained Core-approved surface")
        required_units = plan["required_units"]
        if any(
            not all(unit in surface for unit in required_units) for surface in surfaces
        ):
            raise AssertionError("a candidate changed a claim, provenance or disclosure unit")
        observations[-1]["native_model_selection"] = {
            "model_revision": emission["model_revision"],
            "model_artifact_ref": plan["model_artifact_ref"],
            "model_observation_ref": plan["model_observation_ref"],
            "candidate_count": len(surfaces),
            "authorized_surfaces": surfaces,
            "selected_text": text,
            "decoder_version": emission["decoder_version"],
        }
        observations[-1]["local_file_effect"] = {
            "phase": receipts[0]["phase"],
            "sha256": digest(output_path.read_bytes()),
            "bytes": output_path.stat().st_size,
        }

        output_path, attempted_sequence = rollback_to_attempted_file(actor, case, profile)
        before = output_path.stat()
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
            timeout=15,
            check=False,
        )
        if reconciled.returncode != 0:
            raise AssertionError(
                f"profile reconcile failed: exit={reconciled.returncode}; "
                f"stdout={reconciled.stdout}; stderr={reconciled.stderr}"
            )
        recovered = json.loads(reconciled.stdout)
        after = output_path.stat()
        if (
            recovered.get("lifecycle") != "COMMIT"
            or recovered.get("id") != profile["id"]
            or output_path.read_bytes() != text.encode("utf-8")
            or (before.st_ino, before.st_mtime_ns, before.st_size)
            != (after.st_ino, after.st_mtime_ns, after.st_size)
        ):
            raise AssertionError("recovery did not confirm the exact file without resending it")
        observations.append(
            {
                "operation": "reconcile_existing_file_without_resend",
                "actor": actor,
                "case": case,
                "input_id": profile["input_id"],
                "exit_code": reconciled.returncode,
                "lifecycle": recovered.get("lifecycle"),
                "reason": recovered.get("reason"),
                "result_id_unchanged": recovered.get("id") == profile["id"],
                "rollback_stopped_after_attempted_sequence": attempted_sequence,
                "file_inode_mtime_size_unchanged": True,
            }
        )

        summary = run(
            actor,
            case,
            "profile-follow-up-summary",
            "前回の説明を2文以内で要約して",
        )
        if summary.get("guarantee") != "HYPOTHESIS" or profile["id"] not in summary.get(
            "dependencies", []
        ):
            raise AssertionError("the bounded summary did not consume the actual profile result")

        other_actor = run(
            "profile-other-actor",
            "profile-isolation-case",
            "profile-no-cross-actor-fallback",
            "ココについて保存されている情報をもとに短い紹介文を書いて",
            lifecycle="SUSPEND",
            exit_code=2,
        )
        if not str(other_actor.get("reason", "")).startswith("MISSING_PREMISES:"):
            raise AssertionError("a separate actor unexpectedly read the profile owner's facts")

        conflict_actor, conflict_case = "profile-conflict-owner", "profile-conflict-case"
        positive = run(conflict_actor, conflict_case, "mike-positive", "ミケは鳥")
        negative = run(conflict_actor, conflict_case, "mike-negative", "ミケは鳥ではない")
        conflicted = run(
            conflict_actor,
            conflict_case,
            "mike-conflicted-profile",
            "ミケについて保存されている情報をもとに短い紹介文を書いて",
        )
        conflict_text = conflicted["text"]
        for marker in ("肯定と否定の根拠", "対立根拠の一方", "ミケは鳥である", "ミケは鳥ではない"):
            if marker not in conflict_text:
                raise AssertionError(f"the conflicted draft resolved or omitted a side: {marker}")
        if not set(positive["dependencies"] + negative["dependencies"]).issubset(
            set(conflicted["dependencies"])
        ):
            raise AssertionError("the conflicted profile lost one of its competing records")

        counts = Counter(item["lifecycle"] for item in observations if "lifecycle" in item)
        profile_delivery = observations[2]["local_file_effect"]
        evidence = {
            "work_item": "BODY-1 BB-CONTENT/BB-CONTEXT saved-relation profile draft",
            "stage": "BODY-1_EXECUTION",
            "recorded_at": datetime.now(timezone.utc).isoformat(timespec="seconds"),
            "base_commit": base_commit,
            "changed_paths": [
                "rust/README.md",
                "rust/apps/cli/tests/normal_route.rs",
                "rust/audit/IMPLEMENTATION_STATUS.md",
                "rust/audit/requirements.tsv",
                "rust/audit/bb-content-known-relation-profile-v69-route-validation.json",
                "rust/crates/minidora-domain/src/lib.rs",
                "rust/crates/minidora-engine/src/core.rs",
                "rust/crates/minidora-engine/src/input.rs",
                "rust/crates/minidora-engine/src/language.rs",
                "rust/crates/minidora-engine/src/recovery.rs",
                "rust/crates/minidora-engine/src/relations.rs",
                "rust/docs/IMPLEMENTATION_CHOICES.md",
                "rust/tools/known_relation_profile_route_test.py",
            ],
            "subject": "MINIDORA Rust successor engine",
            "requirement_ids": ["A01", "A02", "A07", "A08", "A13", "B08"],
            "software_version": "0.7.0-alpha.8",
            "schema_version": "minidora-rust/0.7.0-alpha.8",
            "parser_version": "declarative-ja-en-v69-relation-profile-draft",
            "toolchain": toolchain,
            "applied_build_set": {
                "manifest_path": "rust/audit/construction/MB-BUILD-20261113/MANIFEST.tsv",
                "manifest_sha256": digest(manifest_path.read_bytes()),
                "source_artifacts": build_set,
            },
            "binary_sha256": digest(binary.read_bytes()),
            "source_snapshot_sha256": sha256_paths(source_paths),
            "test_and_tool_snapshot_sha256": sha256_paths(test_paths),
            "documentation_snapshot_sha256": sha256_paths(documentation_paths),
            "execution_mode": "fresh CLI process per request with temporary actor-scoped SQLite and content-addressed blobs; the profile is saved as a local file, then one controlled SQLite rollback models a crash after the file attempt and before confirmation",
            "previous_producer_gap": "Before this batch the parser had no natural saved-relation profile intent; the relation inventory and previous-result summary existed as separate bounded consumers.",
            "producer_consumer": "ordinary named-subject request -> sealed DraftRelationProfile -> bounded same-actor relation proofs -> Core-authored claims, provenance and disclosure -> two Core-approved transition variants -> native SequenceModel ranking -> Output-rendered local file receipt -> crash reconciliation without resend -> previous-result summary consumer",
            "scope": "Short Japanese profile drafts from current saved direct and derived relations for one explicit subject. Dependencies include the exact premise, derived fact and rule IDs. The result remains HYPOTHESIS, discloses that outside-world truth and record completeness were not checked, preserves conflicts, stops when no actor-scoped supported facts exist, and never falls back across actors. This does not establish general free-form writing, semantic adequacy across unseen tasks, source-backed open-domain facts or complete B08 acceptance.",
            "actual_input": {
                "actor": actor,
                "case": case,
                "input_id": profile["input_id"],
                "request": "ココについて保存されている情報をもとに短い紹介文を書いて",
                "request_sha256": digest(
                    "ココについて保存されている情報をもとに短い紹介文を書いて".encode("utf-8")
                ),
            },
            "producer_output_and_core_state": {
                "result_id": profile["id"],
                "lifecycle": profile["lifecycle"],
                "guarantee": profile["guarantee"],
                "text_sha256": digest(profile["text"].encode("utf-8")),
                "dependencies": profile["dependencies"],
                "consumer_result_id": summary["id"],
                "consumer_contains_profile_dependency": profile["id"]
                in summary["dependencies"],
            },
            "local_effect": {
                "profile_file_sha256": profile_delivery["sha256"],
                "profile_file_bytes": profile_delivery["bytes"],
                "confirmed_receipt_before_recovery": True,
                "reconciled_without_resend": True,
            },
            "summary": {
                "fresh_process_observation_count": len(observations),
                "lifecycle_counts": dict(counts),
                "native_model_candidate_count": 2,
                "all_core_required_units_retained_in_each_candidate": True,
                "direct_derived_rule_lineage_retained": True,
                "previous_result_summary_consumed_profile_id": True,
                "actor_isolation_missing_premise_stop": True,
                "conflicting_supports_retained_without_resolution": True,
                "crash_reconcile_no_resend_and_file_identity_unchanged": True,
                "permissions_secret_check": {
                    "local_file_write": "only the exact temporary path granted by this test harness",
                    "external_network_or_messages": "NOT_USED",
                    "other_repository_writes": "NONE",
                    "added_dependencies": "NONE",
                    "secret_pattern_scan": "NO_MATCH",
                    "private_HDS_copied_to_public_evidence": False,
                },
                "validation_commands": [
                    {"command": "cargo fmt --all -- --check", "exit_code": 0},
                    {
                        "command": "cargo check --workspace --all-targets --locked",
                        "exit_code": 0,
                    },
                    {
                        "command": "cargo clippy --workspace --all-targets --locked -- -D warnings",
                        "exit_code": 0,
                    },
                    {"command": "cargo build --workspace --locked", "exit_code": 0},
                    {
                        "command": "cargo test -p minidora-engine relation_profile --locked",
                        "exit_code": 0,
                        "passed": 2,
                    },
                    {
                        "command": "cargo test -p minidora-cli --test normal_route natural_relation_profile_uses_current_proofs_and_feeds_a_follow_up_consumer --locked",
                        "exit_code": 0,
                        "passed": 1,
                    },
                    {
                        "command": "python3 rust/tools/known_relation_profile_route_test.py --binary rust/target/debug/minidora-rust --evidence-output rust/audit/bb-content-known-relation-profile-v69-route-validation.json",
                        "exit_code": 0,
                        "fresh_process_observations": len(observations),
                    },
                ],
                "known_fail_and_resolution": "The first fresh-process file recovery stopped with RECOVERY_CONTENT_MISMATCH because DraftRelationProfile was absent from the bounded HYPOTHESIS recovery allowlist. Core's result and file were retained; recovery now admits this intent and recomputes its proof inventory/units/candidate set. The repeated route reconciled without resending.",
                "expected_suspends": [
                    "A separate actor with no saved facts receives MISSING_PREMISES.",
                ],
                "full_workspace_runtime_suite": "NOT_RUN",
                "general_free_form_writing_and_full_B08_acceptance": "UNVERIFIED",
                "next_consumer": "The existing previous-result summary consumer used the profile. The next BUILD gap is general constrained writing quality across the preserved unseen-task population.",
                "rollback_commit": None,
            },
            "observations": observations,
        }
        args.evidence_output.parent.mkdir(parents=True, exist_ok=True)
        args.evidence_output.write_text(
            json.dumps(evidence, ensure_ascii=False, indent=2) + "\n", encoding="utf-8"
        )
        print(
            json.dumps(
                {
                    "evidence_output": str(args.evidence_output.resolve()),
                    "observation_count": len(observations),
                    "lifecycle_counts": dict(counts),
                    "profile_result_id": profile["id"],
                    "profile_file_sha256": profile_delivery["sha256"],
                    "summary_result_id": summary["id"],
                    "conflicted_profile_result_id": conflicted["id"],
                },
                ensure_ascii=False,
            )
        )


if __name__ == "__main__":
    main()
