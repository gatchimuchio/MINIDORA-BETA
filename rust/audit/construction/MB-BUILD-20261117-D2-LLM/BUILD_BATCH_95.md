# BUILD_BATCH_95 — AskCommonRelations ExplainPrevious Recovery rejects a missing actor-memory snapshot

**BASE HEAD:** `ebe38461cc2324356bc8bd11cdc72e74fcb3ef94` (main = origin/main; clean at batch start). **Source commit:** `743da4b4d37482d700895da60903b9600dc21e61` (Test common relations recovery missing memory revision). **Version/schema:** 0.7.0-alpha.11 / minidora-rust/0.7.0-alpha.11. **Observed date:** 2026-11-18 UTC. **Scope:** E1-W10, saved AskCommonRelations actor-memory snapshot binding for ExplainPrevious Recovery. **Source diff SHA-256:** `7d88f3fbe626f71d2b2adc25976db0a3ea174ad18529bbc381639f4680ed5e94`.

## Implementation and observation

B95 adds one Engine-library clone-only regression. The ordinary Japanese Engine route commits a rule, two bird premises, a derived `ココとミケの共通点は何ですか` result and its ExplainPrevious response. The common result's pinned actor-memory snapshot is confirmed present. After Engine reopen, valid saved-state Recovery inspect passes.

The test chooses the next actor-memory revision after the current head and confirms that no snapshot exists there. In a cloned CaseRevision only, it rebinds the saved common-relations ResultRecord's `actor_memory_revision`, preserving its result identity and exact sealed input. Recovery refuses the clone with the typed `RECOVERY_MEMORY_REVISION_MISSING` error. The persisted case, actor-memory head and target bytes remain unchanged. The malformed clone is never stored.

The first probe expected `RECOVERY_CONTEXT_PROOF_CHANGED` to wrap the error. It instead observed `RECOVERY_MEMORY_REVISION_MISSING` directly because `verify_result_memory_provenance` replays the saved common-relations result before the later `verify_previous_relation_context` wrapper. The first assertion output is retained; the final test asserts the observed fail-closed error. No production behavior, schema, dependency, manifest or lockfile changed.

| Causal position | Value and consumer |
| --- | --- |
| Producer | Ordinary committed AskCommonRelations ResultRecord with a derived report, exact sealed input and retained actor-memory revision. |
| Consumer | ExplainPrevious Recovery -> `verify_result_memory_provenance` -> `verify_explain_previous_relation_memory_result` -> `verify_explain_previous_common_relations_memory_result` -> `result_memory_snapshot` -> exact Store revision lookup. |
| Observed boundary | Valid result passes; clone pointer to a verified absent revision stops directly with `RECOVERY_MEMORY_REVISION_MISSING`. |
| Persistence | Only the in-memory cloned CaseRevision pointer changes; persisted case, actor-memory head and target bytes remain unchanged. |

## Verification

Linux x86_64 / kernel 6.18.57, Rust 1.99.1 (58cf8f9c3 2026-10-13), Cargo 1.99.1 (5f94df478 2026-08-27). Shared target cache: `/workspace/.minidora-cloud/rust-target`. `rust/Cargo.toml` SHA-256 `3a54553a0c6d15836b00cf556a821bb011da7295bc11c15ad2ad9b0d25ae8785`; `rust/Cargo.lock` SHA-256 `b4e97fecdca8fd1fc863349b1a48eb99a925da7fed0ed6ce6cfe79bdd1cececc`; `rust/rust-toolchain.toml` SHA-256 `d13306be0ecda81ce779baa290ffb417380d525928477f16200884b5169cca33`.

| Command | Result | Saved log |
| --- | --- | --- |
| `cargo test --manifest-path rust/Cargo.toml -p minidora-engine --lib recovery::tests::common_relations_explanation_rejects_missing_actor_memory_snapshot_revision --locked -- --exact --nocapture` | PASS after correcting the expected error boundary: 1 passed, 0 failed, 288 filtered out; 3.03s. | [focused test](b95-common-missing-revision-test.log) |
| `cargo test --manifest-path rust/Cargo.toml -p minidora-engine --lib recovery::tests::common_relations_explanation_ --locked -- --nocapture` | PASS: 2 passed, 0 failed, 287 filtered out; 7.19s. | [common-relations Recovery filter](b95-common-explanation-filter.log) |
| `cargo fmt --manifest-path rust/Cargo.toml --all -- --check` | PASS | [format check](b95-fmt-check.log) |
| `git diff --check` | PASS | [diff check](b95-diff-check.log) |
| Initial focused assertion probe | Expected-wrapper assertion failed; observed typed `RECOVERY_MEMORY_REVISION_MISSING`. The test now asserts that actual fail-closed result. | [initial assertion output](b95-first-wrapper-assertion-failure.log) |

## Not run and status

Persistent Store corruption/repair, historical Rust-schema ResultRecord or SQLite reads, schema migration, full Engine/workspace runtime suites, workspace Clippy/build, CLI/HTTP/browser, long-history scale behavior, other operating systems, every Recovery consumer, complete CAP-D02/E1 acceptance, independent QA, E2-E8, release and deployment were not run. B95 verifies one synthetic clone-only pointer rejection. E1-W10 remains `PARTIAL`; CAP-D02 remains `UNVERIFIED`; no E1 exit, release or deployment is claimed.

## Batch-start refs

At start, main/origin/main/local `codex/backup-main` branch and tag were `ebe38461cc2324356bc8bd11cdc72e74fcb3ef94`. Local `codex/backup-main-prev` branch/tag and remote previous tag were `b1d94eb17d43b4d46aedb22a1bb7e274dfd26713`. Remote backup branches remain untouched at `codex/backup-main=8c425bf28af09b0284101836da598d53fb8d6756` and `codex/backup-main-prev=e9a275591d9f062d033c07d2a9a4e239414269a6`. Main rollback point is the batch-start `ebe3846...`.

## B95 asset classification

B95 owns 11 current assets:

- **Retained: 2** — existing `rust/audit/IMPLEMENTATION_STATUS.md` and `docs/redesign/03_REQUIREMENT_TRACE.tsv`, updated in place; CAP-D02 acceptance stays `UNVERIFIED`.
- **Stored: 7** — this report, [machine observation](b95-build-observation.json), focused test, common-relations filter, format check, diff check and the initial assertion output.
- **Active: 2** — the Recovery regression in `rust/crates/minidora-engine/src/recovery.rs` and shared Cargo target cache.
- **Trash: 0. Hold: 0.** The fixture removes its temporary state and output; no fixture artifact remains in the repository.

Windows storage and OneDrive procedures do not apply to this Linux execution. Git records will be retained on GitHub main and the existing backup tags; remote backup branches remain unchanged.
