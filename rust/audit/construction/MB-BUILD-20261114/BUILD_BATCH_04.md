# BUILD_BATCH_04 — Retain pronoun-resolved relation meaning

## Work identity

- Repository: `gatchimuchio/MINIDORA-BETA`
- Base HEAD: `a92b5173d842973f27618d8a8edc4d2dfb3699fd`
- Construction set: `MANIFEST.tsv` in this directory.
- Work type: preserve Core's adopted typed conclusion for `AskPreviousRelation` follow-ups and recovery.

## Observed gap

`AskPreviousRelation` resolved an omitted subject from the immediately preceding same-case answer, then Core queried the resolved proposition. The committed result retained proof dependencies but not the resolved proposition. A later `ExplainPrevious` therefore had to reconstruct that proposition from Japanese output text.

## Implementation

For pronoun-resolved relation answers, Core now writes a tagged, versioned `CoreComputationArtifact` in the existing computation-artifact slot. It records the adopted proposition, the exact relation-proof dependency IDs, and the same calculation, output units, evidence, and result dependencies already recorded by the old computation artifact. Other intents keep the existing artifact format. No domain result, ContentPlan, or store schema changed.

`previous_relation_meaning` first reuses the sealed typed proposition for direct `Ask`/`AskWhy` results. It then reads and cross-checks the typed computation artifact for adopted conclusions against the committed result and content plan. Both normal follow-up execution and recovery use this same lookup, then re-run the current relation proof and compare its dependencies and guarantee. Old computation artifacts retain the text-based compatibility path; missing or unsupported tagged artifacts stop instead of being reinterpreted.

Implementation references: `crates/minidora-engine/src/core.rs` (`CoreComputationArtifact`, `AskPreviousRelation`, `previous_relation_meaning`, `ExplainPrevious`); `crates/minidora-engine/src/recovery.rs` (`verify_previous_relation_context`).

## Checks

- `cargo fmt --all -- --check` — PASS, exit 0.
- `cargo check --workspace --all-targets --locked` — PASS, exit 0.
- `cargo clippy --workspace --all-targets --locked -- -D warnings` — PASS, exit 0.
- Test execution — NOT_RUN; not requested.
- End-to-end normal-route and recovery observation — NOT_RUN.

## Remaining gate

Exercise supported and contradicted `AskPreviousRelation` results followed by `ExplainPrevious`, including crash recovery, and verify old-record compatibility. The full construction set, R2 acceptance populations, and release review remain unverified.
