# AGENTS.md — MINIDORA Engine Beta Maintenance Contract

## Subject and purpose

Treat MINIDORA as an **HDS-driven execution engine**, not as an LLM-only or chatbot-only project. Preserve the existing responsibilities of the input machine, execution core, and output machine. Prioritize goal retention, bounded execution and stopping, capability inheritance, and non-regression.

## Change and version discipline

- The **whole-system next-version design candidate** is maintained under `docs/redesign/`. Read its six-part specification/plan/work-order/evidence set and complete capability register before proposing a next-version engine edit. It describes a **future** version, not permission to alter the frozen beta. Keep the original repository strictly **read-only** under the current instruction.
- The new design starts from the full task, identity/authority/evidence and actual user-facing acceptance; do not assume existing implementation placement is the only possible solution. Preserve verified public behavior, and resolve any material ownership/compatibility change explicitly rather than creating a second execution/judgment authority.
- `0.6.0b1` is a frozen **engine/distribution snapshot**. Do not silently change executable implementation, public API contracts, saved formats, or the upstream repository. An implementation change requires a separately identified release with its diff, evidence, and new hashes.
- Documentation localization may be revised separately **only** when it is explicitly identified as a documentation-only revision, with the original release evidence and historical bytes recoverable. See `DOCS_REVISION.md`.
- Maintain auditable provenance for all 414 inherited public source files and the isolated beta changes. Do not confuse GitHub documentation hashes with the original frozen ZIP/wheel hashes.
- Do not mutate upstream `main`, GitHub Actions, branches, or external services unless authorized. Perform checks in a separate workspace. Repository writes follow the user's active authorization.
- The **input machine** owns input acceptance, one-time interpretation, and the sealed input bundle. It must not independently drive, learn, or adopt the final outcome.
- The **MINIDORA core** is the sole owner of action execution, goal/state changes, learning, and final adoption.
- The **output machine** owns rendering, invalidation and explicit delivery. Do not introduce a second execution, judgment, or learning cycle there.
- Do not remove or silently break legacy entry points, established capabilities, stop conditions, compatibility paths, version boundaries, dependencies, or publication rules.
- Do not promote a failure to a successful `COMMIT`, fabricate goal progress, infer benchmark scores, or disguise untested conditions as verified.

## Documentation and language rules

- Write **public explanatory documentation, contribution guidance, examples, and change notes in English**, even where Python identifiers and historical filenames remain Japanese.
- Preserve Japanese identifiers, literal enum values, source messages, and protocol fields when changing them would alter an executable contract. Explain them in English with an explicit mapping; do not silently rename them.
- English translations describe **public behavior and interfaces**, not an equivalence proof for private theory. Do not publish hidden definitions, reconstruction mappings, or large transformed semantic datasets.
- Keep historical records anchored to their original version, date, environment, and evidence. Never overwrite past claims with current assumptions.

## Acceptance and publication

- Separate `source present`, `local test passed`, `normal-route integrated`, `independent user-task accepted` and `external platform accepted` per capability. A new-version completion claim requires the population and observable evidence in `docs/redesign/02_CAPABILITY_REGISTER.md` and `docs/redesign/05_VERIFICATION_AND_EVIDENCE.md`.
- For the full redesign, connect the required whole-system routes in one coordinated implementation pass, inspect real input-to-output behavior, repair defects at their actual owner, and then run affected local and full acceptance. Do not treat a series of isolated component tests as product completion.
- Before accepting a new version, check affected tests, interface boundaries, regressions, version/dependency changes, provenance and hashes.
- The selected beta regression command is `python tools/beta_acceptance.py`; it is **not** a full upstream test suite or benchmark.
- The GitHub Actions acceptance workflow is **manually dispatched only**. Do not assert cross-platform results unless runs were actually performed and inspected.
- Follow `PUBLICATION_BOUNDARY.md` for all external content. Keep unverifiable claims explicitly unverified and do not generalize local success to universal capability.
