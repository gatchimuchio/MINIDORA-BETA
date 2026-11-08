# Contributing and reviewing MINIDORA-BETA

For future engine architecture work, start with the [whole-system redesign register](docs/redesign/README.md). Its specification, complete capability and evidence population, construction plan and agent work order jointly define a **proposal** for a separately versioned successor. Do not infer permission to implement, release or deploy from the existence of those documents.

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

## Validate an implementation change

```bash
python -m pip install .
python tools/beta_acceptance.py
python -X utf8 examples/controlled_smoke.py
```

`tools/beta_acceptance.py` covers the **selected 313-case beta regression set**; it does not stand for all upstream test files. The optional multi-OS GitHub Actions workflow is manually triggered and must be assessed separately.

## Documentation changes

Write external-facing prose in **English**. Preserve original Japanese file paths, API keywords and exact output values in code examples. In the change record, identify whether you modified only explanatory files or changed package/runtime behavior. Update relevant checksum records; do not change the original frozen `RELEASE_MANIFEST.json` or `checksums.sha256` to make a new checkout appear identical to the original distribution.

See [English onboarding](docs/ENGLISH_ONBOARDING.md) and [API guide](docs/API_GUIDE.md) for reviewer-friendly context.
