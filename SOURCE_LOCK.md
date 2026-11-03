# MINIDORA Engine Beta — Source Lock

- **Frozen engine distribution:** `0.6.0b1`
- **Upstream repository:** `gatchimuchio/NOTNN-LLM-MINIDORA`
- **Upstream `main` at extraction:** `9307d7e2b1799750ecef1957d85128fcbe403d02`
- **Purpose:** Isolate and fix the public input machine, MINIDORA execution core and output machine as one independently usable HDS-driven engine beta.
- **Inherited source comparison:** 414 upstream public files preserved; 413 Git blobs identical, 1 modified, 2 new files, 0 deletions.
- **Modified source:** `src/minidora/統合駆動_v2/循環.py`
- **New beta entry points:** `src/minidora/β実行.py`, `src/minidora/engine_beta.py`
- **Adjusted legacy test:** `tests/test_HDS正本継承循環_v1.py` (defensive-copy assertion)
- **Original distribution evidence:** `RELEASE_MANIFEST.json` and `checksums.sha256` (original ZIP/wheel); see `BETA_READINESS.md` for bounded acceptance and exclusions.
- **Repository documentation revision:** Public explanatory files were later translated into English without modifying the engine implementation. See `DOCS_REVISION.md` and the repository-local `checksums.repo.sha256`; original GitHub documentation is recoverable at commit `fe5822de63644bdaea9208a98046f71684dffae2`.
- **Rule:** Preserve the original release artifacts. Publish implementation changes only under a new release/version with provenance and newly measured evidence.
