# BUILD_BATCH_06 — Contextual relation assertion from a typed prior result

## Work identity

- Repository: `gatchimuchio/MINIDORA-BETA`
- Branch: `main`
- Base HEAD: `015f2750243a974b2d49313560e511b00bf63d4a`
- Construction set: the three source documents and `MANIFEST.tsv` under `MB-BUILD-20261114`. Their SHA-256 values were rechecked against the manifest before this batch; the three source documents and manifest are unchanged.
- Work type: connect one committed singular-pronoun relation answer to one following assertion, with current-proof revalidation and a subsequent exact-fact query.

## ⓪–④ Preflight and scope

- ⓪ Authority: repository `AGENTS.md`, the complete three-piece BUILD set and manifest, Core/R2 implementation status, requirement mappings and current Rust source were inspected. The BUILD documents define the implementation target; this report and route evidence record observations without changing those source documents.
- ① Repository: `MINIDORA-BETA`, `main`, base `015f2750243a974b2d49313560e511b00bf63d4a`. The batch began with a clean worktree and `origin/main` at the same base.
- ② Writers and recovery: one main worktree, no concurrent writer, and sufficient free disk were observed. Before edits, local `main` and `origin/main` were at the base and local recovery refs were rotated with expected-old checks. Fresh pre-push inspection found `origin/codex/backup-main` at `d8361afb57dd443dd919733fd35205211f92d23b` and `origin/codex/backup-main-prev` at `b6dc924aeaad9256d9e258b78517ea3d0f59186f`; backup tags remained at the base. After the Batch 06 implementation commit was pushed, the backup branch and tag were advanced with compare-and-swap; the previous backup branch and tag point at the pre-edit base.
- ③ Bounded unit: immediately after a committed same-case typed relation answer, accept one singular-pronoun relation assertion by rechecking its current proof, restoring subject/scope, and storing a user premise; then verify that a following relation question consumes the new fact. Missing, cross-case, cross-actor and changed-proof cases must stop without adoption.
- ④ Exclusions: no general co-reference, plural or multi-clause assertion, unrelated previous-result interpretation, truth verification, full workspace runtime suite, release decision or requirement acceptance is claimed.

## Observed route

The normal sealed input records `AssertPreviousRelation` with the unresolved pronoun and optional explicit scope. Core requires the actual immediately preceding committed same-case result and matching `context_result_ref`. It loads its typed proposition, re-runs the actor-scoped relation proof, and checks the exact dependency set and guarantee against the retained result. If they still match, Core restores the subject and scope and stores a `USER_PREMISE` whose dependencies identify the prior result and new fact. The subsequent ordinary question returns that fact ID alone.

The fresh-process route records 49 requests and reconciliation observations, including a successful derived-relation case, the next query consumer, no-prior and cross-case stops, changed-proof stop, cross-actor rejection, persisted unresolved parser meaning, and the previously implemented explanation and file-recovery regressions. Details and source hashes are in [the route evidence](../../bb-context-pronoun-relation-assertion-v1-route-validation.json).

## Changes

- Added `AssertPreviousRelation` to the sealed intent and bumped the parser version to `declarative-ja-en-v72-contextual-relation-assertion`.
- Added Core proof revalidation, typed subject/scope restoration and USER_PREMISE storage for the bounded immediate assertion.
- Extended `rust/tools/contextual_relation_pronoun_test.py` with the producer/consumer route and missing, cross-case, changed-proof and cross-actor denial checks.
- Added the route evidence, this batch report, the scoped implementation-status entry and route pointers on eight requirement rows. Requirement acceptance remains `UNVERIFIED`.
- Prior BUILD source documents, manifest, Batch 05 report and Batch 05 evidence remain unchanged.

## Checks

- `cargo fmt --all -- --check` — PASS.
- `cargo test -p minidora-engine singular_pronoun_relation_assertions_keep_subject_unresolved_until_core --locked` — PASS, 1 passed.
- `cargo test -p minidora-engine previous_relation_meaning_reads_legacy_computation_artifact --locked` — PASS, 1 passed.
- `cargo check --workspace --all-targets --locked` — PASS.
- `cargo clippy --workspace --all-targets --locked -- -D warnings` — PASS.
- `cargo build --workspace --locked` — PASS.
- Python AST parse of the route observer — PASS.
- Fresh-process route smoke — PASS, 49 observations; unresolved input, adoption, exact following-fact query and denial checks passed.
- `git diff --check` — PASS.
- Full workspace runtime suite — NOT RUN.

## GitHub retention

The implementation commit was pushed to `main` without force. `codex/backup-main` branch/tag and `codex/backup-main-prev` branch/tag were synchronized to the final Batch 06 commit and the pre-edit base respectively using exact expected-old OIDs.

## Acceptance boundary

This is evidence for one immediate singular-pronoun relation assertion and its following explicit relation query. It does not establish arbitrary Japanese or English co-reference, broader semantic interpretation, outside-world truth, general learning quality or full A04/A07/A08/A15/E08/E12/CAP-D04/CAP-D11 acceptance.

## Asset disposition

- Held: 4 manifest-verified input assets (the original uploaded ZIP and 3 BUILD source documents), unchanged.
- Preserved: 2 prior records, `BUILD_BATCH_05.md` and `bb-context-pronoun-relation-followup-v1-route-validation.json`, unchanged.
- Active: 8 batch files (three Rust source files, the route observer, route evidence, `IMPLEMENTATION_STATUS.md`, `requirements.tsv` and this report).
- Trash: 0.
- Hold: 8 requirement-level acceptance obligations remain `UNVERIFIED`.

File-asset total across these categories: 14 (4 held + 2 preserved + 8 active + 0 trash). The 8 hold obligations are open acceptance items, not discarded files.
