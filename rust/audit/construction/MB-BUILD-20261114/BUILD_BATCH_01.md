# BUILD_BATCH_01 — Scope-bound relation explanations

## Work identity

- Repository: `gatchimuchio/MINIDORA-BETA`
- Base HEAD: `b082b0b791cd41e695ecd4a47fb0d2d41af584d1`
- Construction set: this directory's `MANIFEST.tsv`; the preserved work order names `965b3d5e433bb375d86b45e06044d812a3cf6d98` as a reference SHA, while work began from the observed current `main`/`origin/main`.
- Work type: one Core content-preservation implementation slice; no release or deployment.

## Requirement and observed gap

R2 and the 2026-11-14 construction set require relation explanations to retain scope, polarity, provenance, and the distinction between evidence and real-world truth. The existing query and proof retain a typed scope, and relation rules reject silent scope changes. The explanation composer in `crates/minidora-engine/src/core.rs` omitted that scope from its adopted natural-language content.

Relevant public capability areas: A05 (negation/scope fidelity), A09 (typed relation constraints and provenance), A10 (claim/premise/inference/conclusion alignment), A15 (one authoritative meaning object), and B05 (explicit inference chains).

## Implementation

`relation_explanation_lines` now carries the target scope on the conclusion, each direct or derived fact, each applied rule, and the evidence-boundary disclosure. The contradicted-answer lead also names the queried scope. Core still owns the query, proof, dependencies, guarantee, and adoption; the existing non-neural sequence model can only rank the complete Core-approved surfaces. The normal `AskWhy` and relation explanation construction paths feed the unchanged content-plan and render checks.

This does not implement general unseen-language generation or the complete seven-function Core. Other answer/report paths are outside this batch.

## Checks

- `cargo fmt --all -- --check` — PASS, exit 0.
- `cargo check --workspace --all-targets --locked` — PASS, exit 0.
- Test execution — NOT_RUN for this batch.
- `cargo clippy --workspace --all-targets --locked -- -D warnings` — PASS, exit 0.
- End-to-end route observation — NOT_RUN; the route connection above is source inspection, not an executed behavior claim.

## Remaining gate

Run the construction-set's direct Core checks after the applicable Core content-generation path is implemented. Full R2 populations, 93 provisional requirements, all source/test census obligations, external QA, and release review remain unverified.
