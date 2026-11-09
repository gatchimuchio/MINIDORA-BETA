# R1 Independent Audit — R2 Correction and Evidence Ledger

**Date:** 2026-11-09 (JST)  
**Target:** `MINIDORA-BETA` future-version **documentation**, not the frozen `0.6.0b1` executable.  
**Baseline:** R1 at repository commit `0cae46398fa17607270a07655c308bd080b31b71`.  
**Purpose:** Preserve the negative audit, identify each changed normative assumption, and distinguish **fixed design text** from **unobserved implementation validity**. The source-of-truth for current architecture remains [01_SYSTEM_SPECIFICATION.md](01_SYSTEM_SPECIFICATION.md).

## Classification and evidence rules

- `SPEC_CORRECTED`: A concrete contradictory/unsupported normative clause is replaced by an explicit, independently falsifiable rule. It **does not** mean implementation passes.
- `SOURCE_ROUTE_PENDING`: A file/hash exists, but source semantics, actual normal-input reachability, behavioral equivalence and requirement owner have **not** been established.
- `IMPLEMENTATION_PENDING`: Normative contract exists, but no executable successor or real proof of effect is available.
- `OBSERVATION_PENDING`: Validity of a chosen physical adapter, benchmark, resource budget, model quality or external provider requires an actual run.
- `REJECTED_R1`: Earlier architecture-completion claim withdrawn; retaining Git history does not preserve the old claim as current accepted truth.

## Findings, corrections, non-claims

| Finding | R1 problem | R2 correction site | Spec closure | Still required |
|---|---|---|---|---|
| F01 | `COMMIT` before required output/write/delivery | §3, §6, §9, §16 | `SPEC_CORRECTED` | R2-01 actual file/remote effect test |
| F02 | `RECEIVED` could not reject; new version bypassed re-seal | §6–7 | `SPEC_CORRECTED` | R2-02/03 state machine tests |
| F03 | Non-null goal digest before goal existed | §5, §12 | `SPEC_CORRECTED` | R2-04 restart/schema test |
| F04 | Architectural and capability `D01–D14` collisions | §2 `ARCH-Dxx`; 02 `CAP-Dxx` | `SPEC_CORRECTED` | R2-05 join lint |
| F05 | Method disposition conflated with acceptance status | §19, 02, [03 trace](03_REQUIREMENT_TRACE.tsv) | `SPEC_CORRECTED` | Populate actual two-axis results |
| F06 | “Complete 93” despite provisional/open inventory | §19–20, 02 | `SPEC_CORRECTED` | `SOURCE_ROUTE_PENDING`; discover additional obligations |
| F07 | `foreign_keys ON` but DDL had no foreign-key declarations | §12 relational DDL | `SPEC_CORRECTED` | R2-06 real SQLite connection checks |
| F08 | Effects ledger omitted authority/scope/idempotency identity | §12, §16 | `SPEC_CORRECTED` | R2-08 real port reconciliation |
| F09 | File blobs and DB claimed atomic outcome without full crash protocol | §12 | `SPEC_CORRECTED` | `OBSERVATION_PENDING`: R2-07 fault injection/OS behavior |
| F10 | Output owned mutable case state despite single core writer | §3–6, §16 | `SPEC_CORRECTED` | Render/effect event wiring test |
| F11 | Actor/Case isolation not represented as a checked relation | §12 | `SPEC_CORRECTED` | `OBSERVATION_PENDING`: R2-09 unauthorized read attempts |
| F12 | Layer-0 necessities used to decree one sparse/beam equation | §2, §8.4 | `SPEC_CORRECTED` | `SOURCE_ROUTE_PENDING`: inherited model comparative selection |
| F13 | Core/output both appeared to own semantic text generation | §3–4, §8.3, §14 | `SPEC_CORRECTED` | R2-10 final-byte meaning check |
| F14 | Provenance/formal derivation promoted beyond truth guarantees | §1 `I12`, §8, §14 | `SPEC_CORRECTED` | Verifier scope and source accuracy probes |
| F15 | Candidate scoring left incomparable cases undecided | §8.2 | `SPEC_CORRECTED` | R2-11 implemented selection tests |
| F16 | “Enough” observations without scoped learning admission | §10 | `SPEC_CORRECTED` | R2-13 controlled unseen/counterexample |
| F17 | Scheduler named unspecified choices and pseudo-terminal | §9 | `SPEC_CORRECTED` | R2-14 real graph/budget tests |
| F18 | Stable journal merge implied too much about external effects | §17 | `SPEC_CORRECTED` | R2-15 concurrent physical effect observation |
| F19 | Python/SQLite/JSON/model/budget values fixed without derivation | §2, §8.4, §12, §17 | `SPEC_CORRECTED` | `OBSERVATION_PENDING`: alternatives and measured profiles |
| F20 | Only measurements could justify architecture correction | §2 | `SPEC_CORRECTED` | R2-22 change-policy exercise |
| F21 | 414 inherited files not individually mapped to engine abilities | §18–20, [04 census](04_SOURCE_CENSUS.tsv) | **Metadata only** | `SOURCE_ROUTE_PENDING`: semantic inspection all relevant files/routes |
| F22 | Internal compiler/memory-compute substitution treated as peripheral port | §15, §18 | `SPEC_CORRECTED` | R2-16/19 backend-equivalence tests |

## Exact file populations — dated, non-conflated

- The fixed upstream `9307d7e2b1799750ecef1957d85128fcbe403d02` contains **414 blobs under `src/`** (413 under `src/minidora/`, and `src/README.md`). The source manifest contains one upstream row per path.
- The current beta `0cae46398fa17607270a07655c308bd080b31b71` contains all 414 paths, with **412 identical blobs**, **one modified engine source** `src/minidora/統合駆動_v2/循環.py`, **one later-translated document** `src/README.md`, and the **two beta-only entry points** `src/minidora/β実行.py` and `src/minidora/engine_beta.py`. This current-tree comparison is not the extraction-time `SOURCE_LOCK.md` identity statement of 413 identical + 1 changed.
- Fixed upstream `tests/` contains **359 blob paths**; current beta contains **147**, of which **146 paths overlap**, **213 upstream paths are absent from the beta tree**, and **one beta path is additional**. The [test census](09_TEST_CENSUS.tsv) preserves every path/digest. Applicable tests and their executed results are still unexamined at R2.
- The [93-row capability trace](03_REQUIREMENT_TRACE.tsv) has separate machine-owner *proposals*, exact inherited-entry mapping status, implementation disposition and acceptance status. **Every source path is listed, but no uninspected path has been falsely declared semantically mapped.**

## Design and release conclusion

### R2 static-document checks actually performed (2026-11-09 JST)

- JSON Schema 2020-12: schema validity checked; all 11 principal record shapes accepted under the generated structural fixtures. A pre-goal `RECEIVED` revision with null goal passes; a `RUNNING` revision with null goal fails. **These are synthetic schema checks, not normal-request acceptance tests.**
- Reference SQL: all eight `CREATE TABLE` statements executed on an in-memory SQLite connection with foreign keys enabled. Nonexistent case/artifact references, a `RUNNING` state without a bound goal and an unknown effect authorization reference were rejected. **This does not verify crash durability, transactions on a physical host or access control.**
- Trace integrity: 93 distinct registered capability IDs (14 `D` entries normalized to `CAP-Dxx` in the machine-readable trace), 416 source census rows and 360 test census rows accounted for. Each current capability remains `UNVERIFIED` on both implementation and acceptance axes.
- Document integrity: each of the 22 R1 findings has one correction record and each `R2-01` through `R2-22` has one falsification scenario. New annex links were checked against local additions and preexisting paths in the observed GitHub tree. Root README digest is updated in `checksums.repo.sha256`.

These limited checks confirm **document and representation consistency only**. None changes `SOURCE_ROUTE_PENDING`, `IMPLEMENTATION_PENDING` or `OBSERVATION_PENDING` into a passing successor-engine claim.

**R1 architecture completion: REJECTED.**  
**R2 known document-level contradictions: corrected at the normative contract level, not accepted by implementation evidence.**  
**R2 overall specification exit: NOT YET SATISFIED**, because inherited source-to-ability mapping, exact interface schema completeness, algorithm selection comparisons, machine/OS equivalence and executable observations remain pending.

Future design revisions must keep this ledger and R1 original in Git history, trace exact changed conditions, and never turn a metadata audit, old beta regression, or a formal-source proof into a claimed new engine acceptance.
