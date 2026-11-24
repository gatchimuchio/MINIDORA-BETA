# Contributing and reviewing MINIDORA-BETA

For the present Rust successor work, use the [current Japanese-base BUILD-v4 requirements, design, implementation schedule and concise Codex order](rust/audit/construction/MB-BUILD-20261124-V4-BUILD/README.md) with [root `AGENTS.md`](AGENTS.md). The frozen Python `0.6.0b1` remains a separate accepted historical beta. The prior [R2 logical specification](docs/redesign/01_SYSTEM_SPECIFICATION.md), [D2/D3-C construction set](rust/audit/construction/MB-BUILD-20261119-D3-C/README.md), capability/verification registers and past B observations remain available as dated context, not the current BUILD completion or QC/QA obligations. Source documents and prior implementation history do not themselves grant permission for a new code change, release or deployment.

MINIDORA-BETA is an independent, historically pinned **engine beta**, not an invitation to silently rewrite the upstream implementation.

## Ground rules

1. Read [AGENTS.md](AGENTS.md), [PUBLICATION_BOUNDARY.md](PUBLICATION_BOUNDARY.md), and [DOCS_REVISION.md](DOCS_REVISION.md) before proposing changes.
2. Treat the `0.6.0b1` **engine release** as immutable. Implementation changes must be proposed as a new version, with a reproducible diff, new hashes, explicit acceptance criteria, and regression evidence.
3. Preserve all public API/identifier compatibility unless a separately approved versioned migration justifies a change. Japanese names are intentional; add English explanation, not speculative aliases that break Python contracts.
4. Do not create competing learning, adoption or execution loops outside the MINIDORA core.
5. State each change's purpose, actual producer→consumer connection, changed files, observed build result and known limitations. Test/regression evidence is reported only when actually obtained or separately requested; it is not an automatic BUILD gate.
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

## Rust successor: current BUILD-only scope

Follow [BUILD-v4](rust/audit/construction/MB-BUILD-20261124-V4-BUILD/README.md) and [AGENTS.md](AGENTS.md). The task is to construct, connect and pin the independent general-purpose Rust beta, using the existing Core, input/output, persistent state and declared specialist capabilities. Compilation, launch and actual I/O are construction observations. **Systematic QC, independent QA, benchmarking, Windows/8GB quality qualification and the complete historic 93-capability/416-source/360-test evaluation are not construction gates.** Keep past evidence and unknowns intact. An explicit, separate owner requirement would be needed to undertake quality work or publish/distribute artifacts.

## Documentation changes

Write external-facing explanatory prose in **English**, but maintain `AGENTS.md`, active construction contracts, responsibility, semantics and agent work instructions **Japanese-first**. Preserve original Japanese file paths, API keywords and exact output values in code examples. In the change record, identify whether you modified only explanatory files or changed package/runtime behavior. Update relevant checksum records; do not change the original frozen `RELEASE_MANIFEST.json` or `checksums.sha256` to make a new checkout appear identical to the original distribution.

See [English onboarding](docs/ENGLISH_ONBOARDING.md) and [API guide](docs/API_GUIDE.md) for reviewer-friendly context.
