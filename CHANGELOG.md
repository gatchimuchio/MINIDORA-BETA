# Changelog

## Architectural specification correction — 2026-11-09

- Replaced the preliminary architecture outline with **one concrete, normative unified-engine specification**. It decides the three-machine ownership, state/storage schema, non-neural language and relation core, memory/learning, goal scheduler, capability ports, authorization/effects, deterministic replay and acceptance gates.
- Retired the construction plan and implementation-agent work order from the current documentation tree. Their previous content remains available in Git history; implementation/design choices are not delegated to an agent.
- Preserved the inclusive capability, verification and lineage annexes. Corrected public links and described formerly unresolved decisions as resolved by the specification, with unverified implementation facts still labeled.
- Documentation changes only: no modifications to the frozen engine, its tests, benchmark results, archived distribution or read-only upstream.

## Design-only revision — 2026-11-09

- Added a **from-first-principles, whole-system next-generation design** under `docs/redesign/`, with a specification, inclusive capability register, integrated construction plan, future agent work order, verification contract and provenance/decision register.
- Integrated known existing capability families, historically reported failure modes, learning and memory requirements, current general-chat quality targets and emerging typed external-platform/translation/multimodal ports **as requirements**, not fabricated new implementation claims.
- Linked the new design from the main README, documentation guide and maintenance/contribution rules.
- Preserved the `0.6.0b1` executable snapshot, historical distribution hashes, selected beta acceptance results, source provenance, and read-only upstream repository. Updated the repository-local checksums for edited imported documentation files only.

## Documentation revision — 2026-11-03

- Made English the primary public documentation language of `MINIDORA-BETA`.
- Translated the repository overview, maintenance contract, beta acceptance, source and repository locks, publication boundary, license/attribution explanations, historical canonical notes, and legacy public design stubs.
- Added English onboarding, API glossary, and a controlled copy-and-run example.
- Preserved Japanese executable implementation, public identifiers, upstream provenance, and the original frozen ZIP/release manifest.
- Updated repository-source checksums for changed **documentation bytes**. This revision does **not** claim a new engine or benchmark result. See `DOCS_REVISION.md`.

## Engine 0.6.0b1 — 2026-11-03

- Isolated all 414 public source files from `gatchimuchio/NOTNN-LLM-MINIDORA` at upstream `9307d7e2...`.
- Added `ミニドラβ実行機` to wire the existing input machine, HDS execution core and output machine without duplicating their ownership.
- Bounded revalidation after explicit external-input updates, without fabricating goal progress.
- Preserved a narrowly scoped stage continuation inside the established action budget.
- Rejected repeat execution of the same input version.
- Aligned a legacy test assertion with the defensive-copy contract.
- Passed 313 selected regressions, built a wheel and verified an isolated installation.

The acceptance statement covers only the conditions in `BETA_READINESS.md`. Neither general capability scores nor full upstream test-suite conformance were remeasured. The source-history record for the original beta remains in `RELEASE_MANIFEST.json`.
