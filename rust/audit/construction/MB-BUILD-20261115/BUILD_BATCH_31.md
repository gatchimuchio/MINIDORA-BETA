# BUILD_BATCH_31 — Direct atomic explanation to K7

**Base:** `2bb7ca1e5f5378e9b1ad50d8127bf69fa1701f8d`

**Current gate:** `CORE-G03 = PARTIAL`; the other six gates were not reassessed in this batch.

**Purpose:** connect an atomic, typed relation input to Core-generated explanation and the existing K7 adoption path, without extending the natural-language input machine.

## Contract and route

- **Input:** one grounded `Proposition`; no completed claim or explanation text.
- **Producer:** `Engine::compose_atomic_explanation` resolves the actor-memory proof IDs and seals the typed request with a versioned direct-Core source identity.
- **Core consumer:** the existing `AskWhy` computation re-queries the proposition and compares its sorted proof IDs with the sealed IDs. A changed proof set suspends before content generation.
- **Content and adoption:** proof-derived explanation units and the bounded `SequenceModel` feed the existing `ContentPlan`, K7 adoption, renderer, and result lifecycle.
- **Scope:** Japanese, one relation, explanation purpose. Multi-relation composition, comparison and short composition from atoms, and the ordinary CLI/HTTP input route remain open.

## Changes

- Added a direct typed Core entry that supplies a `Proposition` to the existing execution lifecycle.
- Bound the selected proof IDs to the sealed direct input and rechecked them immediately before explanation generation.
- Linked this direct relation explanation to the existing typed computation artifact so its meaning and proof dependencies travel with the adopted plan.
- Documented the route and its limits in `rust/docs/IMPLEMENTATION_CHOICES.md`.

## Observations

- `cargo fmt --all --check`: **PASS**.
- `cargo check --locked -p minidora-engine`: **PASS** on Rust `1.99.1`.
- Runtime execution of a fresh direct typed request: **NOT_RUN**.
- Test suites: **NOT_RUN**.
- Gate result: `CORE-G03` remains **PARTIAL**. This compile result does not show a runtime value reaching K7 and is not gate-pass evidence.

## Safety and next connection

The entry uses the authenticated actor's memory, keeps source dependencies in the Core result, and leaves output effects under the existing permission and receipt checks. It stops when the requested proposition is unresolved, conflicting, or its proof IDs change. Next, run an unseen direct input through this exact route and inspect the generated intermediate artifacts, adopted `ContentPlan`, rendered bytes, and terminal result; then extend atomic composition only as the observations require.
