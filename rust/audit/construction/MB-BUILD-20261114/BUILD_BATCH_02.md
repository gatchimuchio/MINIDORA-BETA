# BUILD_BATCH_02 — Scope-bound relation answers and follow-up

## Work identity

- Repository: `gatchimuchio/MINIDORA-BETA`
- Base HEAD: `3f57f1c58c7328e974932d7f61be798b56389d39`
- Construction set: `MANIFEST.tsv` in this directory.
- Work type: extend BUILD_BATCH_01's Core content-preservation slice to direct relation answers and their normal follow-up path.

## Implementation

The Core now uses one relation-answer surface composer for supported and contradicted results in direct questions, pronoun-resolved relation follow-ups, and conjunctive relation questions. It retains the queried proposition's scope and polarity; for contradicted results it names both the unsupported query and the opposite relation supported by the saved premises. Conflict and missing-premise stops identify the affected scoped relation. Relation-object answers also label their requested scope in both the summary and each supported relation.

`previous_relation_answer` now reconstructs the scope-qualified answer, including the contradicted form, while retaining support for the earlier `提示された前提の範囲では、{relation}。` record format. This keeps later explanation requests connected to the actual previously adopted relation without changing the saved record schema.

Core remains the sole source of the support/contradiction decision. The sequence model receives only the resulting Core-approved content. Implementation references: `crates/minidora-engine/src/core.rs` (`relation_answer_surface`, `previous_relation_answer`, relation question branches).

## Checks

- `cargo fmt --all -- --check` — PASS, exit 0.
- `cargo check --workspace --all-targets --locked` — PASS, exit 0.
- `cargo clippy --workspace --all-targets --locked -- -D warnings` — PASS, exit 0.
- Test execution — NOT_RUN for this batch.
- End-to-end normal-route observation — NOT_RUN; connectivity and compatibility are source-inspected only.

## Remaining gate

Exercise first-seen positive, negative, and changed-scope relation questions plus a later explanation request through the normal input-to-render route. Broader Core language generation, memory adaptation, remaining construction checks, full R2 acceptance, and release review remain unverified.
