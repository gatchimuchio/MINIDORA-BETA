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

## Decisions settled by the normative specification

`01_SYSTEM_SPECIFICATION.md` fixes the three machine responsibilities, the sole core authority, Python 3.11+ initial runtime, SQLite WAL/event journal/immutable blobs, core memory→inference→adaptation, typed relation and non-neural generation contracts, artifact-based scheduling, evidence packets, versioned capability ports, effect ledger, deterministic replay and Windows/8 GB resource **targets**. These are no longer deferred to a construction schedule or an implementation agent. A future contradictory measurement requires a recorded specification revision, not an implicit substitution.

## Remaining observations and external decisions

| Open item | Observable requirement or actual owner decision |
|---|---|
| Exact successor package/release label | User confirms independent version/release scope once executable changes are authorized |
| Exhaustive compatibility map | Inspect all 414 inherited public files and connected entry routes; test old Japanese public APIs and evidence persistence |
| Local storage and resource viability | Measure atomic replay, crash recovery, process RSS and latency on actual Windows CPU/8 GB hardware; adjust the specification explicitly if targets conflict |
| Generalized learning and language quality | Run natural unseen inputs, future-use/invalidations, memory-long-history and compare intrinsic core vs connected-port outcomes |
| External ports | Select and authorize actual providers/dependencies for speech, image, video, translation, Web, compiler and application operations; inspect license/privacy/cost and distinguish unavailable ports |
| Publication/demo deployment | User authorizes service, cost, credentials, IAM, audience and target artifact independently from design publication |
| Completion/acceptance claim | Evidence satisfies the entire declared register population and full-route tests; reduced scope requires explicit owner approval |

## Remaining source-verification work

This register is grounded in source listings, public contracts and selected implementations. It is **not a completed 414-file code audit or a new full-coverage test result**. An authorized implementation must finish the exhaustive tree/entry-route/test census and update [02_CAPABILITY_REGISTER.md](02_CAPABILITY_REGISTER.md) before claiming total functional coverage. Preserve discoverable new requirements rather than restricting the target to only the names currently listed.

