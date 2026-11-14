# BUILD_BATCH_05 — Typed pronoun relation follow-up and recovery

## Work identity

- Repository: `gatchimuchio/MINIDORA-BETA`
- Branch: `main`
- Base HEAD: `39480523a82823b886e193ba4568383f383a8157`
- Construction set: the three inputs in `MANIFEST.tsv` under this directory. Their SHA-256 values matched the manifest before this batch; the source documents and manifest were not edited.
- Work type: exercise the Batch 04 retained typed relation meaning through its next consumer, recovery and legacy-artifact path.

## Observed route

The bounded route uses fresh CLI processes against one temporary SQLite store:

1. Same-case rule and premise results support `それは動物ですか`. The committed result retains a content-addressed `minidora.core-computation.relation-meaning.v1` with the resolved proposition and exact relation-proof dependency IDs.
2. A later `詳しく説明して` consumes that adopted proposition, recomputes its proof, and preserves the previous result and proof dependencies in its output.
3. A direct negative premise supports the opposite of the asked relation. The typed artifact records that supported opposite; the subsequent explanation keeps the same proof lineage.
4. A proof change in another case for the same actor makes the prior case's explanation suspend as `CONTEXT_PROOF_CHANGED`, with no `ContentPlan`.
5. One exact-authorized temporary local file is written. The test then simulates interruption in the isolated test database after the actual write. `reconcile` confirms `LOCAL_FILE_RECONCILED_NO_RESEND`; dependency IDs, file inode, modification time and size remain unchanged.
6. A focused engine unit test supplies `previous_relation_meaning` with an unversioned legacy computation artifact. It confirms that the legacy conclusion text and its proof dependency remain readable.

The route observer recorded 34 request and reconciliation observations and one temporary file write. Its detailed observations and source snapshot hashes are in [the route evidence](../../bb-context-pronoun-relation-followup-v1-route-validation.json).

## Changes

- Extended `rust/tools/contextual_relation_pronoun_test.py` with typed artifact checks, supported/refuted follow-ups, stale-proof denial, authorized file output and no-resend recovery.
- Added a test-only legacy artifact compatibility regression at the end of `rust/crates/minidora-engine/src/core.rs`. Production Core behavior did not change in this batch.
- Added the route evidence JSON.
- Added this batch report and appended the scoped route to `IMPLEMENTATION_STATUS.md` and the eight relevant rows in `requirements.tsv`. Their requirement-level acceptance statuses remain `UNVERIFIED`.
- Prior Batch 04 and v58 evidence remain preserved and unchanged.

## Checks

- `cargo fmt --all -- --check` — PASS.
- `cargo test -p minidora-engine previous_relation_meaning_reads_legacy_computation_artifact --locked` — PASS, 1 passed.
- `cargo check --workspace --all-targets --locked` — PASS.
- `cargo clippy --workspace --all-targets --locked -- -D warnings` — PASS.
- `cargo build --workspace --locked` — PASS.
- Python AST parse of the route observer — PASS.
- Fresh-process route smoke — PASS, 34 observations; typed producer/consumer, changed-proof stop, and local-file recovery checks passed.
- `git diff --check` — PASS.

The first Clippy run identified that the new test module preceded remaining Core functions. The test module was moved to the end of the file and Clippy was rerun successfully. The initial route assertions also encoded stale expectations for dependency count and punctuation; they were corrected to match the observed rule lineage and rendered wording, then the full route was rerun successfully.

## Acceptance boundary

This is scoped evidence for A04, A07, A08, A15, E08, E12, CAP-D04 and CAP-D11. Their requirement-level acceptance remains `UNVERIFIED`. The full workspace runtime suite, plural and multi-clause reference, general cross-topic co-reference, every legacy store history, and complete requirement acceptance were not run. This batch does not establish broader language understanding, factual learning quality, or full BB-CONTEXT acceptance.

## Asset disposition

- Held: 3 manifest-verified BUILD input documents, unchanged.
- Preserved: 2 prior items, `BUILD_BATCH_04.md` and the v58 pronoun route evidence, unchanged.
- Active: 6 batch files (the test observer, test-only Core regression, new route evidence, implementation status, requirements trace and this report).
- Trash: 0.
- Hold: 8 requirement-level acceptance items remain `UNVERIFIED`.

File-asset total across these categories: 11 (3 held + 2 preserved + 6 active + 0 trash). The 8 hold items are open acceptance obligations, not discarded files.
