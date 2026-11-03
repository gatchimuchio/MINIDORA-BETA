# Documentation Revision — English Accessibility Layer

**Revision:** `docs-en-2026-11-03`  
**Applies to:** GitHub repository documentation and controlled example only  
**Underlying engine:** `minidora-notnn==0.6.0b1` (unchanged)

## Why this revision exists

The beta engine's internal implementation uses Japanese filenames, symbols and meaning-oriented identifiers. External readers should not have to know Japanese to understand the **public scope, intended architecture, runnable entry points, tests, limitations, provenance, licenses, and participation rules**.

English is therefore the default language for human-facing explanations. This is an explanatory projection of the public implementation, **not** a replacement of its Japanese conceptual/implementation contracts or a disclosure of private internal theory.

## Exactly what is — and is not — frozen

| Object | Fixed evidence | Impact of this revision |
|---|---|---|
| Upstream public source snapshot | `NOTNN-LLM-MINIDORA` at `9307d7e2b1799750ecef1957d85128fcbe403d02` | None |
| Beta engine implementation (`src/minidora/` Python files) | Original `0.6.0b1` distribution; see `RELEASE_MANIFEST.json` | **No source changes** |
| Original beta ZIP, wheel, release hashes | `MINIDORA_Engine_Beta_0.6.0b1_20261103.zip`; SHA256 `60e48b2a54c953166b9f8625a08bcd9573b13cee19fd69794e767f0aa921c7db` | **Not rewritten** |
| Previous GitHub repository documentation | Git commit `fe5822de63644bdaea9208a98046f71684dffae2` | Recoverable byte-for-byte from Git history |
| Current English-facing documentation | This documentation revision commit | Prose bytes and corresponding `checksums.repo.sha256` entries differ |
| Tests and benchmark evidence | `BETA_READINESS.md`, dated 2026-11-03 | No new engine acceptance/benchmark claim |

**Important:** A wheel freshly built from this documentation-localized checkout can have a different hash because the package reads `README.md`. The historical wheel digest in the original release manifest must not be represented as the digest of that rebuilt wheel.

## Integrity of the repository checksum file

`checksums.repo.sha256` indexes the **581 text files imported from the original beta ZIP**, excluding the historical wheel binary. The entries for edited explanatory files were updated for this English documentation revision. GitHub-only additions (e.g. the English guides) are tracked by Git but are not included in that imported-file checksum list.

`checksums.sha256` and `RELEASE_MANIFEST.json` are **historical ZIP-distribution records**. They have not been repurposed to verify the current GitHub checkout. To audit historical documentation bytes, fetch the files from commit `fe5822de63644bdaea9208a98046f71684dffae2`.

## Boundaries

- Japanese implementation and compatibility symbols are left intact.
- No technical generalization, AGI claim, or new benchmark evidence is introduced by translation.
- Internal/private HDS definitions and complete theory-to-implementation mappings remain outside the publication boundary.
- Future changes must label whether they affect documentation, engine code, package metadata, tests, or external deployment independently.
