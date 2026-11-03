# MINIDORA-BETA — GitHub Import and Repository Lock

## Frozen upstream and distribution

- Original source repository: `gatchimuchio/NOTNN-LLM-MINIDORA`
- Upstream source commit: `9307d7e2b1799750ecef1957d85128fcbe403d02`
- Original beta archive: `MINIDORA_Engine_Beta_0.6.0b1_20261103.zip`
- Original ZIP SHA256: `60e48b2a54c953166b9f8625a08bcd9573b13cee19fd69794e767f0aa921c7db`
- Import baseline commit (Japanese documentation): `fe5822de63644bdaea9208a98046f71684dffae2`

The original distribution includes all 414 upstream public source files (with 1 documented source modification), 2 added entry points, selected regression tests, acceptance documentation and a prebuilt wheel. The GitHub repository imports the source, tests and explanatory text but **does not commit the original `dist/` wheel binary**. A wheel can be built from source with:

```bash
python -m pip wheel --no-deps .
```

## Two distinct integrity scopes

1. **Original beta archive:** `RELEASE_MANIFEST.json` and `checksums.sha256` describe the **frozen original ZIP distribution**. The wheel digest recorded there belongs to the original wheel; they must not be treated as hashes of the current GitHub checkout.
2. **Imported GitHub files:** `checksums.repo.sha256` covers the 581 text files imported from the beta ZIP. Its entries reflect the **current repository's documentation localization**, while the engine source files remain unchanged. Additional GitHub-only documentation and workflow files are tracked by Git history.

English localization changes the bytes of `README.md` and other public text files. Consequently, a newly built wheel can have a **different byte hash** (README is package metadata) even though the Python engine implementation has not been changed. Use `DOCS_REVISION.md` and the previous Git commit to audit this difference.

## What was actually verified

On 2026-11-03, **313 selected regression cases passed on Linux/Python 3.12**, and the original wheel's build and isolated install/smoke succeeded. This is **not** proof that all upstream public tests passed, that new performance was measured, or that the manually dispatched cross-platform workflow has run.

## Change rule

Keep original `0.6.0b1` release artifacts immutable. Separate documentation-only changes, new executable releases, external environment acceptance and benchmark evidence; never silently promote one to another. Do not push changes to the upstream source repository without explicit authorization.
