# Changelog

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
