# BUILD_BATCH_96 — AskRelationObjects ExplainPrevious Recovery rejects a missing actor-memory snapshot

**BASE HEAD:** `d60bc3b2459aa7420551bd71df0e6de5f55bb387` (main = origin/main; clean at batch start). **Source commit:** `c8e3a732de58a5525f642a0a08474730ded9c6b6` (Test relation objects recovery missing memory revision). **Version/schema:** 0.7.0-alpha.11 / minidora-rust/0.7.0-alpha.11. **Observed date:** 2026-11-18 UTC. **Scope:** E1-W10, saved AskRelationObjects actor-memory snapshot binding for ExplainPrevious Recovery. **Source diff SHA-256:** `b97f9318a25ad611a2d11527b06b7b409bb2c38e1ec8709254e546d28a7958e1`.

## Implementation and observation

B96 adds one Engine-library clone-only regression. The ordinary Japanese Engine route commits `ミナはカッターを使う`, a rule deriving that she uses a tool, `ミナは何を使いますか`, and an ExplainPrevious request. The object-list result depends on both the direct fact and the rule and records an actor-memory revision whose snapshot is confirmed present. After reopening Engine, valid saved-state Recovery inspect passes.

The test chooses the next actor-memory revision and confirms that no snapshot exists there. In a cloned CaseRevision only, it rebinds the saved AskRelationObjects ResultRecord pointer while preserving its ID and exact sealed input identity. Recovery refuses the clone with `RECOVERY_MEMORY_REVISION_MISSING`. The persisted case, current actor-memory head and target bytes remain unchanged. The malformed clone is never stored.

No production behavior, schema, dependency, manifest or lockfile changed.

| Causal position | Value and consumer |
| --- | --- |
| Producer | Ordinary committed AskRelationObjects ResultRecord with a derived object list, direct-fact/rule dependencies, exact sealed input and retained actor-memory revision. |
| Consumer | ExplainPrevious Recovery -> `verify_result_memory_provenance` -> `verify_explain_previous_relation_memory_result` -> `verify_explain_previous_relation_objects_memory_result` -> `result_memory_snapshot` -> exact Store revision lookup. |
| Observed boundary | Valid result passes; a clone pointer to a verified absent revision stops with `RECOVERY_MEMORY_REVISION_MISSING`. |
| Persistence | Only the in-memory cloned CaseRevision pointer changes; persisted case, actor-memory head and target bytes remain unchanged. |

## Verification

Linux x86_64 / kernel 6.18.57, Rust 1.99.1 (58cf8f9c3 2026-10-13), Cargo 1.99.1 (5f94df478 2026-08-27). Shared target cache: `/workspace/.minidora-cloud/rust-target`. `rust/Cargo.toml` SHA-256 `3a54553a0c6d15836b00cf556a821bb011da7295bc11c15ad2ad9b0d25ae8785`; `rust/Cargo.lock` SHA-256 `b4e97fecdca8fd1fc863349b1a48eb99a925da7fed0ed6ce6cfe79bdd1cececc`; `rust/rust-toolchain.toml` SHA-256 `d13306be0ecda81ce779baa290ffb417380d525928477f16200884b5169cca33`.

| Command | Result | Saved log |
| --- | --- | --- |
| `cargo test --manifest-path rust/Cargo.toml -p minidora-engine --lib recovery::tests::relation_objects_explanation_rejects_missing_actor_memory_snapshot_revision --locked -- --exact --nocapture` | PASS: 1 passed, 0 failed, 289 filtered out; 1.51s. | [focused test](b96-relation-objects-missing-revision-test.log) |
| `cargo test --manifest-path rust/Cargo.toml -p minidora-engine --lib recovery::tests::relation_objects_explanation_ --locked -- --nocapture` | PASS: 2 passed, 0 failed, 288 filtered out; 3.03s. | [relation-object Recovery filter](b96-relation-objects-explanation-filter.log) |
| `cargo fmt --manifest-path rust/Cargo.toml --all -- --check` | PASS | [format check](b96-fmt-check.log) |
| `git diff --check` | PASS | [diff check](b96-diff-check.log) |

## Not run and status

Persistent Store corruption/repair, historical Rust-schema ResultRecord or SQLite reads, schema migration, full Engine/workspace runtime suites, workspace Clippy/build, CLI/HTTP/browser, long-history scale behavior, other operating systems, every Recovery consumer, complete CAP-D02/E1 acceptance, independent QA, E2-E8, release and deployment were not run. B96 verifies one synthetic clone-only pointer rejection. E1-W10 remains `PARTIAL`; CAP-D02 remains `UNVERIFIED`; no E1 exit, release or deployment is claimed.

## Batch-start refs

At start, main/origin/main/local `codex/backup-main` branch and tag were `d60bc3b2459aa7420551bd71df0e6de5f55bb387`. Local `codex/backup-main-prev` branch/tag and remote previous tag were `ebe38461cc2324356bc8bd11cdc72e74fcb3ef94`. Remote backup branches remain untouched at `codex/backup-main=8c425bf28af09b0284101836da598d53fb8d6756` and `codex/backup-main-prev=e9a275591d9f062d033c07d2a9a4e239414269a6`. Main rollback point is the batch-start `d60bc3b...`.

## B96 asset classification

B96 owns 10 current assets:

- **Retained: 2** — existing `rust/audit/IMPLEMENTATION_STATUS.md` and `docs/redesign/03_REQUIREMENT_TRACE.tsv`, updated in place; CAP-D02 acceptance stays `UNVERIFIED`.
- **Stored: 6** — this report, [machine observation](b96-build-observation.json), focused test, relation-object filter, format and diff logs.
- **Active: 2** — the Recovery regression in `rust/crates/minidora-engine/src/recovery.rs` and shared Cargo target cache.
- **Trash: 0. Hold: 0.** The fixture removes its temporary state and output; no fixture artifact remains in the repository.

Windows storage and OneDrive procedures do not apply to this Linux execution. Git records will be retained on GitHub main and the existing backup tags; remote backup branches remain unchanged.
