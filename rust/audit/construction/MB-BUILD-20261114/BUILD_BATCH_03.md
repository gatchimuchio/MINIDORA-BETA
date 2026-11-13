# BUILD_BATCH_03 — Reuse sealed relation meaning for follow-up

## Work identity

- Repository: `gatchimuchio/MINIDORA-BETA`
- Base HEAD: `46cbdf97186a68c004e4e802f993b195497eeca5`
- Construction set: `MANIFEST.tsv` in this directory.
- Work type: preserve one authoritative relation meaning through previous-answer explanation and recovery.

## Observed gap

For a direct relation question followed by `ExplainPrevious`, Core reconstructed the prior proposition by parsing the displayed answer text. This duplicated input interpretation and coupled follow-up meaning to Japanese wording, despite the sealed `InputBundle` retaining the original `Ask` or `AskWhy` proposition.

## Implementation

The normal `ExplainPrevious` path now loads the prior sealed bundle by its actor, case, input ID, and version through the store's verified input-bundle lookup. It checks that the sealed request identity matches the committed result, then reuses its typed proposition and revalidates the current proof. A contradicted query is explained using the opposite proposition supported by the same query outcome. No record schema or saved-state version changed.

Recovery uses the same typed prior meaning for direct `Ask`/`AskWhy` results, verifies the prior proof dependencies and guarantee as well as the explanation result, and recognizes the current v71 follow-up parser. Legacy records and unresolved pronoun-based relation results retain the existing text-based fallback.

Implementation references: `crates/minidora-engine/src/core.rs` (`ExplainPrevious`, `relation_question_proposition`, `relation_explanation_target`); `crates/minidora-engine/src/recovery.rs` (`verify_previous_relation_context`).

## Checks

- `cargo fmt --all -- --check` — PASS, exit 0.
- `cargo check --workspace --all-targets --locked` — PASS, exit 0.
- `cargo clippy --workspace --all-targets --locked -- -D warnings` — PASS, exit 0.
- Test execution — NOT_RUN for this batch.
- End-to-end normal-route and recovery observation — NOT_RUN; source connectivity only was inspected.

## Remaining gate

Exercise direct supported and contradicted prior relation answers, then explain them after a new input through normal execution and effect recovery. Pronoun-resolved prior relation meanings and older records still use the legacy text parser. The full construction set, R2 acceptance populations, and release review remain unverified.
