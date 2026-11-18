# BUILD_BATCH_94 — AskConjunction ExplainPrevious Recovery rejects a missing actor-memory snapshot

**BASE HEAD:** `b1d94eb17d43b4d46aedb22a1bb7e274dfd26713` (main = origin/main; clean at batch start). **Source commit:** `f51424932ad5b6168c8121790c48eb6448667ea9` (Test conjunction recovery missing memory revision). **Version/schema:** 0.7.0-alpha.11 / minidora-rust/0.7.0-alpha.11. **Observed date:** 2026-11-18 UTC. **Scope:** E1-W10, saved AskConjunction actor-memory snapshot binding for ExplainPrevious Recovery. **Source diff SHA-256:** `61c3302a89eeb18b2fb96de99e9e14798ccebd484e68b11cfc5e6cace6a229a3`.

## Implementation and observation

B94 adds one Engine-library clone-only regression. The ordinary Japanese Engine route commits the `ココは鳥です` and `ココは動物です` premises, `ココは鳥かつ動物ですか`, then an ExplainPrevious request. After reopening Engine, valid saved-state Recovery inspect passes and the saved conjunction result's actor-memory revision is confirmed to have a retained snapshot.

The test chooses the next actor-memory revision after the current head and confirms that no snapshot exists there. In a cloned CaseRevision only, it changes the prior conjunction ResultRecord's `actor_memory_revision`, preserving the result identity and exact sealed input bundle. Recovery refuses the clone with `RECOVERY_CONTEXT_PROOF_CHANGED` wrapping `RECOVERY_MEMORY_REVISION_MISSING`. The persisted case, current actor-memory state and output bytes remain unchanged. The malformed clone is never written to Store.

No production behavior, schema, dependency, manifest or lockfile changed. This exercises the specialized conjunction ExplainPrevious Recovery consumer; it does not claim persistent corruption or historical schema compatibility.

| Causal position | Value and consumer |
| --- | --- |
| Producer | Ordinary committed AskConjunction ResultRecord with an existing actor-memory revision and exact sealed input. |
| Consumer | ExplainPrevious Recovery -> `verify_previous_relation_context` -> `verify_explain_previous_conjunction_memory_result` -> `result_memory_snapshot` -> exact Store revision lookup. |
| Observed boundary | Valid saved result passes; a cloned pointer to a verified absent revision stops with `RECOVERY_MEMORY_REVISION_MISSING` under `RECOVERY_CONTEXT_PROOF_CHANGED`. |
| Persistence | Only the in-memory cloned CaseRevision pointer changes; persisted case, actor-memory head and target bytes remain unchanged. |

## Verification

Linux x86_64 / kernel 6.18.57, Rust 1.99.1 (58cf8f9c3 2026-10-13), Cargo 1.99.1 (5f94df478 2026-08-27). Shared target cache: `/workspace/.minidora-cloud/rust-target`. `rust/Cargo.toml` SHA-256 `3a54553a0c6d15836b00cf556a821bb011da7295bc11c15ad2ad9b0d25ae8785`; `rust/Cargo.lock` SHA-256 `b4e97fecdca8fd1fc863349b1a48eb99a925da7fed0ed6ce6cfe79bdd1cececc`; `rust/rust-toolchain.toml` SHA-256 `d13306be0ecda81ce779baa290ffb417380d525928477f16200884b5169cca33`.

| Command | Result | Saved log |
| --- | --- | --- |
| `cargo test --manifest-path rust/Cargo.toml -p minidora-engine --lib recovery::tests::conjunction_explanation_rejects_missing_actor_memory_snapshot_revision --locked -- --exact --nocapture` | PASS: 1 passed, 0 failed, 287 filtered out; 1.50s. | [focused test](b94-conjunction-missing-revision-test.log) |
| `cargo test --manifest-path rust/Cargo.toml -p minidora-engine --lib recovery::tests::conjunction_explanation_ --locked -- --nocapture` | PASS: 2 passed, 0 failed, 286 filtered out; 2.62s. | [conjunction Recovery filter](b94-conjunction-explanation-filter.log) |
| `cargo fmt --manifest-path rust/Cargo.toml --all -- --check` | PASS | [format check](b94-fmt-check.log) |
| `git diff --check` | PASS | [diff check](b94-diff-check.log) |

## Not run and status

Persistent Store corruption/repair, historical Rust-schema ResultRecord or SQLite reads, schema migration, full Engine/workspace runtime suites, workspace Clippy/build, CLI/HTTP/browser, long-history scale behavior, other operating systems, every Recovery consumer, complete CAP-D02/E1 acceptance, independent QA, E2-E8, release and deployment were not run. B94 verifies one synthetic clone-only pointer rejection. E1-W10 remains `PARTIAL`; CAP-D02 remains `UNVERIFIED`; no E1 exit, release or deployment is claimed.

## Batch-start refs

At start, main/origin/main/local `codex/backup-main` branch and tag were `b1d94eb17d43b4d46aedb22a1bb7e274dfd26713`. Local `codex/backup-main-prev` branch/tag and remote previous tag were `0da702f24e2243ffd88702573013dc596140572f`. Remote backup branches remain untouched at `codex/backup-main=8c425bf28af09b0284101836da598d53fb8d6756` and `codex/backup-main-prev=e9a275591d9f062d033c07d2a9a4e239414269a6`. Main rollback point is the batch-start `b1d94eb...`.

## B94 asset classification

B94 owns 10 current assets:

- **Retained: 2** — existing `rust/audit/IMPLEMENTATION_STATUS.md` and `docs/redesign/03_REQUIREMENT_TRACE.tsv`, updated in place; CAP-D02 acceptance stays `UNVERIFIED`.
- **Stored: 6** — this report, [machine observation](b94-build-observation.json), focused test, conjunction filter, formatting and diff logs.
- **Active: 2** — the Recovery regression in `rust/crates/minidora-engine/src/recovery.rs` and the shared Cargo target cache.
- **Trash: 0. Hold: 0.** The fixture removes its temporary state and output; no fixture artifact remains in the repository.

Windows storage and OneDrive procedures do not apply to this Linux execution. Git records are retained on GitHub main and the existing backup tags; remote backup branches are left as found.
