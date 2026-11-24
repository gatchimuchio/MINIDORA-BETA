# Contributing and reviewing MINIDORA-BETA

For current Rust successor work, use [MINIDORA STANDARD BETA BUILD-v5 source documents](rust/audit/construction/MB-BUILD-20261124-V5-STANDARD/README.md) and [AGENTS.md](AGENTS.md). Required construction includes easy local clone-to-chat startup, Japanese multi-turn conversation, Core-owned generation/memory/inference/adaptation, permissioned autonomous network discovery and real page retrieval, existing specialist Ports, effects and reusable versioned APIs. Earlier BUILD-v4 `50f6514` local CLI/API observations are historical and do not prove completion. Prior [R2](docs/redesign/01_SYSTEM_SPECIFICATION.md), D2/D3-C and audit data remain dated reference evidence. Document adoption gives no independent permission for implementation, deployment or release.

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

## Rust successor: MINIDORA STANDARD BETA v5 BUILD-only scope

Follow [BUILD-v5](rust/audit/construction/MB-BUILD-20261124-V5-STANDARD/README.md) and root [AGENTS](AGENTS.md). Construct and connect the required ordinary-user paths before claiming BUILD complete. An arithmetic CLI, Native API, old source checksum, README entry, Port stub, synthetic web result or fixed greeting is insufficient. Build/launch and actual chat/network/specialist-use checks establish the built machine's existence, not independent product QA. No compulsory QC, QA, benchmark/GPQA, all-OS performance certification, future application integration or release step is added.

## Documentation changes

Write external-facing explanatory prose in **English**, but maintain `AGENTS.md`, active construction contracts, responsibility, semantics and agent work instructions **Japanese-first**. Preserve original Japanese file paths, API keywords and exact output values in code examples. In the change record, identify whether you modified only explanatory files or changed package/runtime behavior. Update relevant checksum records; do not change the original frozen `RELEASE_MANIFEST.json` or `checksums.sha256` to make a new checkout appear identical to the original distribution.

See [English onboarding](docs/ENGLISH_ONBOARDING.md) and [API guide](docs/API_GUIDE.md) for reviewer-friendly context.
