# 06 — Lineage, Evidence and Open Decisions

**Record date:** 2026-11-09 (JST). Public implementation and behavior records only; no private HDS theoretical mappings.

## Repository identity and immutable baselines

| Object | Observed reference | Meaning |
|---|---|---|
| Read-only upstream | [`gatchimuchio/NOTNN-LLM-MINIDORA`](https://github.com/gatchimuchio/NOTNN-LLM-MINIDORA), `main` = `9307d7e2b1799750ecef1957d85128fcbe403d02` at this inspection | Reference for existing source and broader public documentation; **no writes authorized** |
| Isolated beta before this redesign documentation | [`gatchimuchio/MINIDORA-BETA`](https://github.com/gatchimuchio/MINIDORA-BETA), `main` = `0b6c7e51b3041f8183e44c3a021999fa670b853e` | English-documentation revision, frozen implementation `minidora-notnn==0.6.0b1` |
| Frozen source/extraction | [`SOURCE_LOCK.md`](../../SOURCE_LOCK.md), [`RELEASE_MANIFEST.json`](../../RELEASE_MANIFEST.json) | 414 inherited public files (413 byte-identical, one documented modified), two beta entry points |
| Frozen distribution | Original ZIP SHA-256 `60e48b2a54c953166b9f8625a08bcd9573b13cee19fd69794e767f0aa921c7db` | Historical original beta distribution only; not a new checkout/wheel digest |
| Isolated beta acceptance | [`BETA_READINESS.md`](../../BETA_READINESS.md) | 313 selected cases on Linux/Python 3.12, controlled installed-wheel wiring, not full user-facing acceptance |
| Historic GitHub import | `fe5822de63644bdaea9208a98046f71684dffae2` | Recoverable original Japanese documentation revision |

## Historical observations retained without promotion

| Date | Observation | What it does *not* prove |
|---|---|---|
| 2026-09-09 | Optional specialist capability experiment reported 29/198 → 80/198 on GPQA with +51 net and no item regression in that run | Not a current beta score, not a cross-version capability delta |
| 2026-09-27 | Separate core-only GPQA LIVE run reported 40/198 | Not the score of the October integrated implementation or beta1 |
| 2026-10-01 | Public three-system integrated goal-dependent execution baseline | Not a new full-product independent acceptance |
| 2026-10-04 | Prior development report described 9/27/81 capacity structure and 3,760 passed checks | Does not prove all current normal-route abilities; do not reconstruct private internal theory from these labels |
| 2026-10-07 | Learning/GPQA route and working-set cost failure audit described disconnected adoption, empty startup formation and redundant effort | Root-cause leads; current beta reproduction pending |
| 2026-10-08 | LIVE source drift invalidated a simplistic 42→38 causal explanation; source-packet crossover proposed | Historical causal correction; new code causality not established by historical scores |
| 2026-10-08 | Separate structured memory experiment: retained 140 source observations, 14 indexes, 18/18 local tests | Not integrated into generic beta normal inputs |
| 2026-10-13 | Historical Cloud Run/UI and evidence-backed response demos; incomplete novel-topic quality and direct-browser acceptance | Not production-ready general chat or a live service availability check today |
| 2026-10-13 | Conversation-quality specification targeted novel Q&A, writing, multistep memory/follow-up | An acceptance population proposal, not its pass result |
| 2026-11-03 | Beta1 extracted and documented in English, selected regressions passed | No new general performance, universal translation or multi-OS claim |
| 2026-11-09 | Fresh design register collects prior and emerging requirements | Documents only; engine implementation/evaluation remains untouched |
| 2026-11-09 (architecture amendment) | One concrete normative architecture replaces an outline plus construction-plan/agent-order bundle | Architecture design decisions are fixed, **not** implemented or performance-verified |
| 2026-11-09 (independent review) | R1 fails 22 design/contract/evidence checks; the earlier “design complete” assertion is **retracted** | Does not establish that original frozen beta runtime has these new R1 design defects |
| 2026-11-09 (R2 correction) | R2 defines source/abstract execution semantics separately from Python/compiler/OS/format, fixes goal/render/receipt state transitions and DDL contract, disambiguates requirement IDs and statuses | The corrected contract is **not yet a fully source-mapped or executable accepted successor** |

The upstream [`CURRENT_CANONICAL.md`](https://github.com/gatchimuchio/NOTNN-LLM-MINIDORA/blob/main/CURRENT_CANONICAL.md), [`README.md`](https://github.com/gatchimuchio/NOTNN-LLM-MINIDORA/blob/main/README.md), [`設計/README.md`](https://github.com/gatchimuchio/NOTNN-LLM-MINIDORA/blob/main/%E8%A8%AD%E8%A8%88/README.md), [`評価/評価契約_v3.md`](https://github.com/gatchimuchio/NOTNN-LLM-MINIDORA/blob/main/%E8%A9%95%E4%BE%A1/%E8%A9%95%E4%BE%A1%E5%A5%91%E7%B4%84_v3.md) and beta [`CURRENT_CANONICAL.md`](../../CURRENT_CANONICAL.md) preserve the public record. Historical private reports were used as investigative leads but are **not copied or linked** in this public repository.

## Decision precedence and disagreements

1. **Task definition:** Current owner instruction is to redesign from scratch, integrate old and emerging capabilities, save the design in `MINIDORA-BETA`, and leave upstream untouched. This supersedes any old assumption that the existing source tree must be accepted as final architecture.
2. **Baseline immutability:** The separate beta version is historically frozen, so the next implementation must carry a new identity. A greenfield design does not mean deleting verified inherited functions or retroactively changing acceptance.
3. **Language:** Internal semantic naming remains Japanese; English is the public documentation/accessibility layer for this beta repository. Public identifiers remain literal even when explained in English.
4. **Responsibility:** Current beta public contract has one-time input acceptance, a single core learning/execution/final-adoption authority, and an output/delivery component without a second judge. Older research arrangements involving an additional output gate were version-specific and **cannot silently be reinstated**. A material replacement requires a new explicit user decision and compatibility evidence.
5. **Learning:** The owner has specified learning as the core's general memory→inference→adaptation operation, not a specialist bolt-on or a failure-only recovery path. History reports disconnected routes; the new implementation must inspect whether they still exist.
6. **Capabilities:** Not all desired multimodal/translation/compiler/memory-compute functions are currently part of the beta engine. Their **integration ports and required behavioral obligations** belong in this overall design; independent model/application implementations and measured capabilities remain distinct objects.
7. **Evaluation:** Historic core-only, module-enabled, Cloud demo, beta regression and new target versions are separate objects. The existing public v3 GPQA acceptance population and no-gold/LIVE contracts cannot be revised for convenience.
8. **Public boundary:** All public descriptions stay at interface, capability, evidence and risk level. The private theory, full mapping and large transformed datasets stay excluded even when a design would be easier to explain with them.

## Current architectural contract versus historical R1 selections

`01_SYSTEM_SPECIFICATION.md` **R2** fixes three machine responsibilities, Core's sole final-goal and logical-state authority, an **OS/CPU/format-independent abstract execution contract**, persistent evidence/revision semantics, Core memory→inference→adaptation, scoped proof/evidence, the six Layer-0 non-neural requirements, actual goal/artifact/effect completion, capability ports and Core backend distinctions, deterministic recorded-evidence replay, and target-resource **measurement requirements**. It **does not fix Python/SQLite/JSON/sparse-trie/beam/120-second budgets as deduced necessities**: these were selected prematurely in R1 and must be comparatively justified. The historical Python beta remains a source/equivalence candidate; executable successor choices cannot be silently delegated to a construction work order.

The [R1 audit correction ledger](07_R1_AUDIT_CLOSURE.md) records all 22 findings, their R2 fixes and the different remaining empirical gates. A logical contradiction, requirement/identity conflict, new authorized constraint or observed failure **all** permit an explicit versioned architecture correction; a new physical measurement is not the sole acceptable cause.

## Remaining observations and external decisions

| Open item | Observable requirement or actual owner decision |
|---|---|
| Exact successor package/release label | User confirms independent version/release scope once executable changes are authorized |
| Exhaustive compatibility map | Inspect all 414 inherited public files and connected entry routes; test old Japanese public APIs and evidence persistence |
| Runtime/compiler semantic equivalence | Define observable operation/effect semantics and compare implementations across CPU, OS, encodings, exceptions and side-effect receipts; do not claim a high-level language is intrinsically platform-free |
| Local storage and resource viability | Select/justify physical adapter by alternatives; measure SQLite WAL reference DB+blob crash recovery, actor access, process RSS and latency on actual Windows CPU/8 GB |
| Generalized learning and language quality | Run natural unseen inputs, future-use/invalidations, memory-long-history and compare intrinsic core vs connected-port outcomes |
| External ports | Select and authorize actual providers/dependencies for speech, image, video, translation, Web, compiler and application operations; inspect license/privacy/cost and distinguish unavailable ports |
| Publication/demo deployment | User authorizes service, cost, credentials, IAM, audience and target artifact independently from design publication |
| Completion/acceptance claim | Evidence satisfies the entire declared register population and full-route tests; reduced scope requires explicit owner approval |

## Remaining source-verification work

**File/blob metadata is now enumerated, but semantic linkage is not.** [04_SOURCE_CENSUS.tsv](04_SOURCE_CENSUS.tsv) compares **all 414 original source paths** and two beta-only entries to the current GitHub tree, while [09_TEST_CENSUS.tsv](09_TEST_CENSUS.tsv) accounts for upstream **359** and beta **147** test paths. At the 2026-11-09 R1 tree the file identity differs from the original frozen extraction because **`src/README.md` was translated later**; engine source still differs in only one inherited implementation file. The [93-row trace](03_REQUIREMENT_TRACE.tsv) records **UNMAPPED** existing execution paths and **NOT_RUN** new-version acceptance accurately. This **does not** constitute exhaustive semantic code inspection, an E2E run or a new performance score. Finish the source-to-capability-to-test audit before claiming total functional inheritance; add discovered requirements without erasing prior IDs.

