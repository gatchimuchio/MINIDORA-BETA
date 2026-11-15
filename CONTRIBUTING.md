# Contributing and reviewing MINIDORA-BETA

For future engine architecture work, start with the single decided [unified engine specification](docs/redesign/01_SYSTEM_SPECIFICATION.md). The [capability register](docs/redesign/02_CAPABILITY_REGISTER.md), [verification contract](docs/redesign/05_VERIFICATION_AND_EVIDENCE.md) and [lineage](docs/redesign/06_LINEAGE_AND_DECISIONS.md) are coverage/evidence annexes, not separate plans or directions to an agent. The separately versioned Rust alpha is already under construction; its [current Japanese-base three-document work order (R4)](rust/audit/construction/MB-BUILD-20261115/README.md) and [root `AGENTS.md`](AGENTS.md) establish the current construction boundaries. R2 design documentation alone does not authorize implementation, release or deployment; the applicable **owner instruction** supplies any concrete permission.

MINIDORA-BETA is an independent, historically pinned **engine beta**, not an invitation to silently rewrite the upstream implementation.

## Ground rules

1. Read [AGENTS.md](AGENTS.md), [PUBLICATION_BOUNDARY.md](PUBLICATION_BOUNDARY.md), and [DOCS_REVISION.md](DOCS_REVISION.md) before proposing changes.
2. Treat the `0.6.0b1` **engine release** as immutable. Implementation changes must be proposed as a new version, with a reproducible diff, new hashes, explicit acceptance criteria, and regression evidence.
3. Preserve all public API/identifier compatibility unless a separately approved versioned migration justifies a change. Japanese names are intentional; add English explanation, not speculative aliases that break Python contracts.
4. Do not create competing learning, adoption or execution loops outside the MINIDORA core.
5. State each change's purpose, expected behavior, failure conditions, and test/regression evidence.
6. Report test outcomes only for the modules, platform, dependency versions, and date actually observed. Do not convert a controlled smoke test into a general-intelligence claim.
7. Never add private HDS theory, complete private theory-to-code mappings, or large derived semantic datasets to this public repository.
8. Preserve original attribution: **がっちむち♂**. Do not push changes back into the upstream repository without a separate instruction.

## Frozen Python beta: historical or scoped checks only

```bash
python -m pip install .
python tools/beta_acceptance.py
python -X utf8 examples/controlled_smoke.py
```

`tools/beta_acceptance.py` covers the **selected 313-case beta regression set**; it does not stand for all upstream test files or the Rust successor. The frozen Python implementation and its release artifacts are not modified by current Rust construction. The optional multi-OS GitHub Actions workflow is manually triggered and must be assessed separately.

## Rust successor: construction, QC and QA are distinct

For the active Rust successor, follow the [current R4 construction schedule](rust/audit/construction/MB-BUILD-20261115/02_施工工程表.md). The construction gates `CORE-G01`–`CORE-G07` are **not** the R2 retrieval/evidence requirements `C01`–`C13`. Establish the native Core gates before new input-machine work; connect minimum input/output and prove T01–T03 through the ordinary user route before expanding specialist capabilities. During each BUILD batch, verify only the changed wiring plus required safety/permission stops. Run the broader Rust workspace checks and system interaction suite as part of systematic QC on the complete candidate, then separate independent QA and any owner release decision. Do not treat the historical Python 313-case result, local Rust component passes or recorded observations as complete successor acceptance.

## Documentation changes

Write external-facing explanatory prose in **English**, but maintain `AGENTS.md`, active construction contracts, responsibility, semantics and agent work instructions **Japanese-first**. Preserve original Japanese file paths, API keywords and exact output values in code examples. In the change record, identify whether you modified only explanatory files or changed package/runtime behavior. Update relevant checksum records; do not change the original frozen `RELEASE_MANIFEST.json` or `checksums.sha256` to make a new checkout appear identical to the original distribution.

See [English onboarding](docs/ENGLISH_ONBOARDING.md) and [API guide](docs/API_GUIDE.md) for reviewer-friendly context.
