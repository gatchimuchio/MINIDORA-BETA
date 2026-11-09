# 05 — Verification and Evidence Contract

**Object:** Next-version behavior under actual user inputs, separately from the frozen beta's documented 313 selected regressions. This R2 annex adds **test and falsification requirements**, not observed passing results. The [source census](04_SOURCE_CENSUS.tsv), [93-capability trace](03_REQUIREMENT_TRACE.tsv), [upstream-test census](09_TEST_CENSUS.tsv) and [22-finding correction ledger](07_R1_AUDIT_CLOSURE.md) are distinct evidence levels.

## 1. Evidence classification and immutable record

For every accepted observation, record `code SHA`, `test-case ID`, `input bytes/hash`, `input/context/state version`, `capability versions`, `external provider and raw evidence packet`, `environment/dependencies`, `elapsed time/resource use`, `observed output`, `terminal state`, `decision/trace`, `test expectation`, `comparison identity`, and `result`. Missing fields should have explicit unknown/null markers.

Five distinct evidence levels are useful:

1. **Source/interface presence**: a declaration, file, import, configuration or API route exists.
2. **Component behavior**: a unit/integration test executed a bounded local contract.
3. **Actual route behavior**: previously unseen request traversed the public input→core→output route and produced the expected real result.
4. **Environment/deployment behavior**: installed or remotely deployed consumer works under the claimed OS, authentication and resource conditions.
5. **Independent external evaluation**: unchanged population and declared evaluation contract, with actual results and provenance.

Evidence at one level must not be promoted to another. In particular, a deterministic supplied-action smoke test proves wiring, not independent problem solving. A hash chain establishes record integrity relative to its inputs, not truth.

## 2. First whole-system acceptance matrix

| Case family | Real input / transformation | Required observation |
|---|---|---|
| Meaning | New Japanese `what/why/how/comparison` prompts | Substantive answer; preserved subject, conditions and original scope |
| Relation audit | Two conflicting arguments, changed premises and quoted claims | Consistent target/premise/evidence/inference/conclusion relationships |
| Negation and time | Polarity, exception, version and time modifications | Dependent conclusion adjusts; unrelated assertions preserved |
| Grounded research | New public article/document with multiple sources | Retrieved **body**, source/version identity, support and contradiction |
| Evidence insufficiency | Empty/snippet-only/broken/429/PDF-warning sources | Accurate reason and typed gap; no fabricated source-derived answer |
| Exact computation | Arithmetic, algebra, table/units, follow-up explanation | Correct result, operand provenance and actual result reused next turn |
| Composition | Information retrieval → calculation → document/answer | Later stage consumes the actual earlier output; trace reflects causality |
| Writing/editing | Unseen document with constraints and revision request | Finished text fits user purpose, length, format and retained source |
| Code | New code generation/revision with constrained checks | Usable code, faithful constraints; tests claimed only when run |
| Capability switch | Same task with optional specialist ON/OFF | Per-item contribution and regressions recorded, no core substitution |
| Learning | Prior evidence → formation → new unseen application | In-core adaptation visible and effective without gold leakage |
| Counterexample | New evidence contradicts previously accepted rule | Inference invalidated with source preserved and dependent results updated |
| Durable memory | Same task after clean process/instance restart | Correct recovery, versioned dedup and accurate prior-result references |
| Budget/stop | No-progress loops, partial completion, blocked permission, cancellation | Correct finite terminal state and actual resource/effect accounting |
| Render/goal identity | Adopted report with actual file write failure, invalid semantic rendering and safe reconciliation | `ADOPTED` is **not** goal `COMMIT` until physical output and receipts satisfy the original goal |
| Streaming/adapters | Multiple chunks, revisions and two distinct external consumers | Stable identity, typed intermediate/final state and authorized effects only |
| UI/API | Human uses CLI, HTTP and browser via installed target | Authentic answers displayed, trace/citation and error handling visible |

Every case must start at the entrance it purports to validate. Tests that directly insert gold, hidden intermediates or a handcrafted `ActionResult` are **component evidence only**.

## 3. Independent conversation quality population

Use the historical quality specification as a proposed population; reconstruct its exact source and evaluator contract before replacing or adopting test data. This is an observational prerequisite, not a separately delegated architecture/design phase:

- **24 unseen natural general questions** across definitions, causal explanation, mechanism, comparison and bounded uncertainty.
- **12 unseen writing/editing tasks** including revisions with explicit target, voice, length, formatting and prohibited content.
- **10 conversations of 4–6 turns each**, testing follow-up, pronoun resolution, actual-result explanation, correction, source reuse, subject change and return.
- **30 meaning transformations**, including negation, scope, agency, condition, tense and quotation changes.
- Additional adversarial/reference ambiguity and external-action denial cases as needed by the actual product contract.

Collect real outputs, item-by-item expectations and failure signatures; do not fabricate quality scores or silently choose easier samples. A historical demonstration such as `2+3 → 詳しく説明して` once reaching `SUSPEND` is a regression probe, **not** a statement that today's beta still fails or a test key to hardcode.

## 4. Learning, memory and resource experiments

Run controlled **before/after** tests under the same source/code conditions wherever possible:

1. Baseline new problem before learning; observe output and entire active record.
2. Provide real evidence that can support a new relation or reusable procedure, but not the future test answer.
3. Reopen through normal public input, then test a different unseen example and an out-of-scope counterexample.
4. Repeat after controlled process restart if durable-memory capability is in scope.
5. Introduce contradicting evidence; require versioned invalidation and retention of old evidence.
6. Compare working-memory size, signature/check effort, redundant actions, latency p50/p95, peak RAM and answer correctness across short/long histories.

Historical investigation cues: a separate prototype reportedly kept **140 original observations** and built **14 indexes** with **18/18** checks; one older formation path increased attempts **86→90**. These numbers are test design inputs from older versions, not new beta measurements or universal operating bounds.

## 5. Retrieval and code causality

**Problem:** Two independent LIVE search attempts can return different candidates, ranks, documents, retry patterns and 429s; their result score difference is not an algorithm-only causal measurement.

Use **separate** suites:

- **Fixed-packet diagnosis:** Capture both search listing and retrieved document bytes, metadata, rank, timestamps, HTTP status and retry/failure location. With two code versions A/B and two captured packets P/Q, compare `A×P`, `B×P`, `A×Q`, `B×Q`. Inspect changed answers, first divergence and changes caused by provider evidence versus algorithm/state.
- **Independent LIVE product/benchmark run:** Actual provider fetch for each attempt; preserve all returned evidence and variability; do not inject saved retrieval packets into official metrics.

Old observations such as **`NO_UNIQUE_POSITIVE_DIFFERENCE` 91** and **`NO_REFERENCE_CONTRIBUTION` 22** are historical failure classes. Validate their definitions and current frequency; do not retrofit a synthetic scoring improvement. New counterevidence may legitimately overturn a previous correct-looking answer, so evidence addition is **not** required to make all answers monotonically more confident.

## 6. Exact GPQA evaluation separation

The original public upstream [`評価/評価契約_v3.md`](https://github.com/gatchimuchio/NOTNN-LLM-MINIDORA/blob/main/%E8%A9%95%E4%BE%A1/%E8%A9%95%E4%BE%A1%E5%A5%91%E7%B4%84_v3.md) controls claims made under that existing benchmark name. Before running, check its actual latest revision and runner implementation. Recorded conditions include:

| Field | Recorded v3 constraint |
|---|---|
| Population | GPQA Diamond 198, all cases 0…197, fixed dataset identity and options shuffle seed 0 |
| Gold boundary | Answers visible only for scoring after execution, **never** as learning input |
| Source mode | **`LIVE_ONLY`**, not packet replay; OpenAlex disabled; Europe PMC/Crossref enabled; Wikipedia `en` |
| Core route | Single input problem bundle; core selection route; no specialist module or external judging in *core-only* series |
| Parallel inheritance | Independent task states, 198 complete within **90 minutes**, previous baseline **40/198** as stated inheritance floor |
| Serial learning | Same core across index order, permitted state inheritance, complete within **90 minutes**, gold excluded |
| Comparison | Same repository version, dataset, route, source configuration; report actual LIVE source drift and wall time |

Historical score series are distinct: **2026-09-09** optional specialist-module comparison **29/198 OFF → 80/198 ON** in its own controlled run; **2026-09-27** separate core-only **40/198**; **2026-10-01** integrated implementation and **2026-11-03** isolated beta were **not** remeasured on all 198. Do not cite those old numbers as new beta or future-version performance. Do not use a newly designed test to weaken the original v3 minimum.

## 7. Regression and compatibility population

- Reconstruct exact **historical extraction** baseline: **414 inherited public files** (413 identical, one documented code modification), plus two beta entry points. In the **current GitHub tree**, the later localized `src/README.md` also differs: **412 identical, one engine-modified, one documentation-modified and two beta additions**. Preserve dated identity rather than conflating snapshots.
- Re-run the declared beta selected regression procedure when a new implementation affects it. Historic **313 passed** does not imply the new version passed.
- Account for **359 upstream test paths** and **147 beta test paths**: 146 shared paths, 213 upstream-only paths and one beta-only path in [09_TEST_CENSUS.tsv](09_TEST_CENSUS.tsv). Applicability, exclusions and actual test behavior remain **unreviewed** by the path/hash census.
- Test both public beta facade identity/version/dedup and existing CLI/API/consumer routes. Account for Japanese method names, serialization and saved-data migration.
- Check original successful cases, known historical failures, new capability effects, security and no unauthorized external deliveries. Record regressions as **item-level changes**.

## 8. Acceptance decision

| Decision | Necessary evidence |
|---|---|
| `ACCEPTED` | Complete target population, executed real routes, recorded environments and outcomes, no unresolved required failure or unapproved regression |
| `CONDITIONALLY_ACCEPTED` | Explicitly named reduced scope/version accepted by owner, with exclusions visible; never means the full original scope passed |
| `REJECTED` | Observed violated requirement, unsafe effect, invalid result or non-regression failure |
| `UNVERIFIED` | Missing code-path observation, test run, dataset, provider access, platform or reproducibility evidence |

Publish percentages only if numerator, denominator, selection rules and what a "pass" means are fixed and inspectable. Evidence from a future external deployment or run must carry its own date, revision and permissions record. No test may rewrite the meaning of an older result.

## 9. R2 contract-falsification matrix

These are required **future** tests, not executed results. Passing SQL syntax or documentation review provides static evidence only, not a completed engine. Each future case must preserve real input, actual outputs/receipts, environment, release SHA and failure signatures.

| ID | Counterexample / adversarial setup | Required observable invariant |
|---|---|---|
| `R2-01` | Core adopts report text; requested file creation fails | Content may be `ADOPTED`; user-goal cannot `COMMIT` |
| `R2-02` | Repeated input identity changes actor/body/permission | `RECEIVED → REJECT` with original collision evidence |
| `R2-03` | Suspended request receives a new input/goal version | `SUSPEND → RECEIVED → SEALED`; no bypass |
| `R2-04` | Crash/restart at `RECEIVED` before goal binding | Legitimate nullable pre-goal revision; goal immutable after binding |
| `R2-05` | Join architectural `ARCH-D04` with capability `CAP-D04` | No ambiguous identifier or disposition collision |
| `R2-06` | Insert absent referenced case/artifact into reference DB | FK/CHECKs reject invalid reference when enabled |
| `R2-07` | Crash between staging/publishing blobs and DB commit | Orphans recognized; missing references block; no phantom commit |
| `R2-08` | Remote effect times out after submission; restart | Saved request/authority/idempotency identity; `UNKNOWN` until reconciliation; no duplicate |
| `R2-09` | Actor B knows Actor A's `case_id` and artifact digest | Read/replay/export denied through all derived references |
| `R2-10` | Renderer changes negation, scope, number or role | Core refuses semantic-equivalence claim and goal completion |
| `R2-11` | Two valid incomparable candidates yield distinct answers | Alternatives/typed suspense; no arbitrary ID-based selection |
| `R2-12` | Duplicate citations, quotations or incorrect initial formalization | Provenance retained; no unlicensed factual promotion |
| `R2-13` | Induced pattern has examples but no scoped empirical rule | Remains hypothesis, never universal fact |
| `R2-14` | Cyclic dependency, no progress, budget exhaustion | Typed bounded stop and real unmet nodes |
| `R2-15` | Concurrent external effects complete in distinct orders | Logical replay limited to recorded observations; actual effects serialized if needed |
| `R2-16` | Same abstract program on two different target backends | Measured semantic/effect equivalence or failed adapter, never assumed |
| `R2-17` | Inherited native language model versus sparse/beam candidate | Comparable novelty, meaning, coverage, resource and regressions before selecting |
| `R2-18` | Windows CPU/8 GB workload; original GPQA 90-minute contract | Measured task-bound budget, not arbitrary 120-second/64-step universal default |
| `R2-19` | Turn external translation OFF/ON; swap core compiler/memory backend | Peripheral versus core backend attribution remains distinct |
| `R2-20` | Compare upstream 414 sources / 359 tests with current beta | File/blob census plus separate **semantic-route** and applicability review |
| `R2-21` | Trace 93 current capabilities and all source-discovered ones | No `UNMAPPED` required route in full-version acceptance |
| `R2-22` | Introduce pure logical contradiction without new measurement | Explicit versioned architecture correction permitted |

**Two closure levels:** `SPEC_CORRECTED` requires one coherent normative rule and a falsifiable condition. `PRODUCT_ACCEPTED` requires actual test observation of the intended version. A historical frozen-beta pass does not satisfy the latter for R2. Validate wire structure against [08_WIRE_SCHEMA.json](08_WIRE_SCHEMA.json) as one narrow check; passing that syntax/schema check does **not** satisfy effect, equivalence, permission, semantic or end-to-end tests.

