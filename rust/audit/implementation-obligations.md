# Full implementation and acceptance obligations

Audit date: 2026-11-09 JST. Baseline: `1c546e77007afa0a9eeb3a44f07da99d84c383eb`. This is an obligations ledger for the independently versioned Rust successor. It does not grant permissions, revise the normative R2 architecture, remove requirements, or certify the developing implementation.

The reference contract is [R2](../../docs/redesign/01_SYSTEM_SPECIFICATION.md), with its [capability trace](../../docs/redesign/03_REQUIREMENT_TRACE.tsv), [verification contract](../../docs/redesign/05_VERIFICATION_AND_EVIDENCE.md) and [R1 correction ledger](../../docs/redesign/07_R1_AUDIT_CLOSURE.md). The attached specification and construction documents are user-supplied reference material; instructions quoted inside them do not independently authorize external actions or override higher-priority/user instructions.

## Evidence status

Each obligation below remains open until an actual successor result links the exact requirement, implementation revision, ordinary input route, independent expectation, observed output/artifact, terminal state, environment, test command and evidence record. Individual component observations may satisfy part of a gate; they do not close the entire capability by implication. This document is the initial obligation population, not a live aggregate of tests currently running elsewhere.

Record the two axes separately: `implementation_disposition` describes the implemented method; `acceptance_status` describes observed acceptance. `UNVERIFIED` is the default without sufficient evidence. A truthful unsupported-capability suspension prevents a false answer; it does not satisfy the positive capability requirement. A reduced release scope requires an explicit owner decision and cannot silently replace this full requested scope.

## Population and inheritance gates

- Inspect the actual semantics, callable/public routes, input/output contracts, side effects and ownership of all **414 upstream public source paths**, plus **two beta additions**: **416 source rows**. Add newly discovered capabilities with source evidence; the named 93 are provisional, not a final exhaustive total.
- Account for **359 upstream test/fixture paths** and **147 beta test/fixture paths**: **146 shared + 213 upstream-only + one beta-only = 360 union paths**. The separate read-only upstream reference now supplies the 213 paths absent from the beta checkout. Determine applicability by actual contracts, and distinguish a ported equivalent test from an executed original test.
- Preserve frozen Python source, tests, API literals, release manifests and historical distribution hashes. The old beta's **313 selected test cases** are a distinct population, not 313 source files or evidence that the Rust successor passed.
- Retain path/commit/blob identity, semantic review scope, real entry route, requirement IDs, Rust responsibility/adapter, original contract, observed input/output and actual test evidence per inherited item. Metadata/AST/import presence alone cannot populate a passing route.
- The current identity audit in this directory establishes file availability and matching identities only. `requirements.tsv` preserves all original IDs and explicit unverified statuses; semantic reviews and successor executions require separate records.

## All 93 named obligations

The requirement and completion-observation columns below preserve the baseline trace. Every row requires a successor implementation/route/evidence mapping. `CAP-Dxx` identifiers are capability IDs, distinct from architecture decisions `ARCH-Dxx`.

| ID | Required public capability | Observable completion obligation |
|---|---|---|
| `A01` | Non-neural/non-Transformer language-model computation; no implicit neural-model substitution | Controlled distribution/decoding outputs, repeatability, actual user-facing consequence |
| `A02` | Finite token/sequence/context handling without arbitrary prompt-specific templates | Novel phrase variants preserve scope and compose a useful result |
| `A03` | Conditional language distribution, deterministic tie/selection ordering, explicit state | Fixed state+inputs reproducible; external variation separately explained |
| `A04` | Japanese word, clause and subject/object attribution, including omitted subjects only when context supports restoration | Ambiguous and omitted subjects handled without inventing ownership |
| `A05` | Negation, exception, quantifier, modality, temporal and scope fidelity | Meaning-preserving and polarity-flipping metamorphic pairs remain distinguishable |
| `A06` | Definition target, explanatory predicate, modifier attachment and relative clause boundaries | Unknown real article definitions and predicate scopes traced to source spans |
| `A07` | Referential/co-reference continuity across turns, output artifacts and corrections | `2+3` → `Explain that in detail` explains **the actual 5 and procedure**, not unjustified suspension |
| `A08` | Persistent topic, goal, requested format, style and previous evidence under conversation changes | Multi-turn corrections, topic switches and return-to-topic obey latest authorized request |
| `A09` | Typed relation constraints, direct vs inferred links, dependency and contradiction | Cross-document relation tests do not turn lexical overlap into factual implication |
| `A10` | Proposition/discussion audit: claim, target, premises, reasons, inference, conclusion and valid scope | Identify invalid premise shifts, scope changes and argument-to-counterargument alignment |
| `A11` | Multi-language Japanese/English/Chinese source-meaning fidelity with loss reports | Back/parallel translations preserve agency, polarity and conditionality |
| `A12` | Unknown/insufficient input recognized explicitly, without filling unsupported details | Insufficient claims return typed residuals; enough evidence yields a substantive answer |
| `A13` | Natural, useful answers to unseen what/why/how/compare questions | Novel independent chat-quality matrix, not greeting/keyword-only matches |
| `A14` | Directed explanation, paraphrase, examples, citations, depth and format changes | Reformat earlier actual answer using its recorded evidence, with no irrelevant retrieval |
| `A15` | Meaning representation stays one authoritative object across parsing, reasoning, execution and output | Repeated input is not reinterpreted by downstream stages into a different premise |
| `B01` | Interpret task type, required output, restrictions and measurable completion | Unseen requests dispatch to correct capability through ordinary entry point |
| `B02` | Exact arithmetic and unit/quantity relations; safe symbolic representations | Computation plus follow-up explanation and correction preserve operands and units |
| `B03` | Algebra, linear equations, statistics, probability and mathematical notation | Independent structured and natural-language cases with verified intermediate values |
| `B04` | Tables, JSON, CSV and structured-document comparison/aggregation/transformation | Real input documents yield schema-valid results, source-location and error records |
| `B05` | Multi-document evidence synthesis and explicit inference chains | Distinguish agreement, independence, contradiction and inferred claims |
| `B06` | Multi-step problem solving with intermediate outputs consumed by later steps | A downstream action actually uses an upstream result; no artificial progress credit |
| `B07` | Coding, code explanation, revision and bounded executable checks | Novel code request produces usable output and scoped diagnostics; never asserts unrun tests |
| `B08` | Writing, editing, rewriting, summaries and constrained-format composition | Twelve previously unseen writing tasks satisfy length, content, purpose and format |
| `B09` | Source-grounded science, domain specialists and mathematical knowledge | Core alone vs named capability variants, same questions and actual evidence |
| `B10` | Candidate generation, comparison, selection, deferred decision and correction | Alternative support/rejection is inspectable; correct answer never injected by evaluator |
| `B11` | Causal, hypothetical and analogy-based reasoning with premise tracking | Counterfactual change alters only dependent claims, not unrelated facts |
| `B12` | Planning a goal with permissible actions, dependencies and limited resources | Executed plan moves actual goal state and stops at correct boundary |
| `B13` | Cross-capability composition with one responsible core | Same request uses retrieval+computation+writing without a hidden secondary judge |
| `C01` | Public web/search and local reference access, with explicit deployment policy | Authorized search reaches actual retrieved **body**, not only title/snippet |
| `C02` | Provider-specific failure, timeout, rate limit and alternative-path accounting | HTTP errors/retries preserve original request, resource costs and failure position |
| `C03` | Exact source time, version, URL, hash, title, page and content lineage | Two different retrieval bytes never masquerade as identical evidence |
| `C04` | Immutable retrieved **search-list** and **body** packets for diagnosis | Capture provider/order/status/retry and independently replay both stages |
| `C05` | Strict separation of fixed-packet regression and official `LIVE_ONLY` benchmarks | Diagnostic replay is never used to inflate a LIVE performance claim |
| `C06` | Source-independent positive support vs overlap, contradiction and negative evidence | Candidate scoring identifies unique supported difference; rejects generic lexical hits |
| `C07` | Wikipedia, scholarly references (e.g., Crossref, Europe PMC), specialist and local sources | Real connector reachability and attribution measured per configured provider |
| `C08` | HTML article main-text parsing, headings, ruby/hidden character boundaries | Source text, warnings and indices survive parsing without invented corrections |
| `C09` | Bounded PDF **text** extraction, page retention and warning rejection | PDFs with extraction errors are not silently treated as understood; OCR is distinct |
| `C10` | Source-change invalidation; cached earlier evidence and old conclusions retained | Changed source triggers targeted dependent re-evaluation, not historical erasure |
| `C11` | Evidence-backed quotations/citations and explicit absence of supporting text | Each factual sentence or quotation maps to the actually used versioned source |
| `C12` | Bounded targeted re-retrieval when actual evidence is insufficient | Search expansion happens for a measured gap, within allowance, without self-feeding |
| `C13` | Real-data input lineage through both candidate and final answer formation | A replay pinpoints whether change came from source, code, state or provider |
| `CAP-D01` | Immutable primary record of instructions, observations, failures and corrected views | Original and corrected records both recoverable with independent identities |
| `CAP-D02` | Working memory limited to current goal and relevant causal dependencies | Long history increases storage without causing unbounded active computation |
| `CAP-D03` | Core-lifetime retained cognition, references and adaptation state | Multiple actual requests reuse permitted earlier evidence in same process |
| `CAP-D04` | Durable session and task state across process/revision restarts | Explicit restore, dedup, correction and identity tests across fresh instances |
| `CAP-D05` | General learning cycle: memory → inference → adaptation in normal execution | Pre/post difference on a genuinely new task, via normal public route |
| `CAP-D06` | Reusable relation/procedure formation with generalization restrictions | New relation applies to new subjects without over-generalized rule adoption |
| `CAP-D07` | Original source, conditions and counterexamples attached to learned records | False rule revoked/limited; downstream dependent conclusions invalidated |
| `CAP-D08` | Formation and inherited capability reconciliation with no duplicate computation | Added shortcut does not increase full-cost repeat evaluations for same task |
| `CAP-D09` | Structured semantic memory/indices separate from full original observations | Archived 140-observation-type workloads retrieve relevant subgraph, retaining origin |
| `CAP-D10` | Reversible compression and evidence replay rather than irreversible summaries | Compressed entry reopens to exact relevant original evidence |
| `CAP-D11` | Cross-session goal/purpose/reference/previous actual output continuity | Session restore supports multi-turn content edits and justified refusal |
| `CAP-D12` | Learning adoption controlled by the same core as action execution | No external module self-commits new rules or creates another success authority |
| `CAP-D13` | Explicit forgetting/invalidation of inferences, never silent deletion of evidence | Counterexample changes active conclusion while preserving lineage |
| `CAP-D14` | Evaluation-label leakage and training/evaluation boundary control | Gold unavailable before final scoring; tests detect contaminated input path |
| `E01` | Case/input/version identity, one-time acceptance and duplicate prevention | Repeated version rejected; new version and result lookup behave as contracted |
| `E02` | Original goal, required states, permissions, forbidden effects and success predicate retained | Goal contract cannot mutate as a byproduct of learning or a retrieved page |
| `E03` | Executable operation chain with actual observed state deltas | Internal state churn alone cannot mark user goal as achieved |
| `E04` | Finite action/time/cost budgets and progress-sensitive continuation | Irrelevant loop stops; valid multistep progress has bounded continuation |
| `E05` | Distinct terminal success, suspension, rejection, failure and cancellation | Distinct signatures, retained evidence and resumability conditions exposed |
| `E06` | Permissioned external effects, explicit output delivery and rollback/compensation | Controlled allow/deny tests; no implicit sending on rendering alone |
| `E07` | Capability versioning, typed inputs/outputs, substitution and composition | Toggle capability with same goal, log changed behavior and regressions |
| `E08` | Source/result/failure causal trace, independent replay and linked hashes | Replay shows actual decision inputs; hash alone never asserted as truth proof |
| `E09` | Input → core → output responsibilities without duplicate decisions | Complete behavior from real request through terminal rendering and explicit delivery |
| `E10` | External platform/tool boundary with cancellation, retry and cost policy | Two independent consumer applications share the same port contract |
| `E11` | Security, input trust, prompt-content isolation, data release and privacy | Retrieved text cannot grant its own authority; forbidden writes/sends blocked |
| `E12` | Failure recovery, partial rollback and continuation from real result state | Retry consumes existing evidence and budget; does not invent a new goal |
| `F01` | Existing Python package, CLI and controlled beta facade | Reproduce declared public entrance and exact Japanese API without renaming |
| `F02` | Product local HTTP chat/trace/capabilities/health entry points | Exercise real API from an installed runnable version, auth and error cases |
| `F03` | Usable browser chat, real answer visibility, source and trace inspection | New browser session answers previously unseen multi-turn requests |
| `F04` | Explanation/format refinement of the **actual** previous response | New instruction edits/reasons over stored output even after math or source task |
| `F05` | General search, document, calculation, code and writing experiences | Representative unseen workflows from one normal user surface |
| `F06` | Explicit multilingual text API and separate output localization | Japanese semantics, English-facing integration and language-fidelity tests |
| `F07` | Streaming/chunked input and output, provisional/revoked/final state | Cancel, revise, finalize and replay a stream without multiple adoption authorities |
| `F08` | Audio/speech interfaces via replaceable input/output adapters | Typed transcript/voice output retains speaker, time and evidence boundaries |
| `F09` | Image/document/video interfaces via replaceable modality adapters | Unknown perception is reported; no claim that text-only PDF handles images |
| `F10` | Calls, messaging, external software and device interactions through controlled ports | Effect audit and permissions carried from intake to actual transmission |
| `F11` | `konjac-Translation` as independent multilingual/multimodal consumer | End-to-end typed translation handoff, but external model quality remains separate |
| `F12` | Independent runtime integrations (e.g., D4Pocket/BLUE-TANUKI) without control inversion | Two heterogeneous consumers keep one goal/authority boundary and trace |
| `F13` | Compiler/IR, alternative memory/compute layouts and model-analysis research | Compared behind a public port by measured effect; not silently declared inherited |
| `F14` | Reviewer-ready real demo, English guide, concrete tasks, and reproducible evidence | Live interactive response plus trace and non-claim statement; GIF/video supplementary |
| `G01` | Exact source/test/requirement census before new code | Every relevant inherited public item dispositioned: retain, merge, revise or reject |
| `G02` | Whole-engine E2E closure on a real input, before treating local modules as proof | Request → evidence → capability → goal → answer and replay demonstrated |
| `G03` | All applicable upstream and beta regressions, negative tests and version compatibility | No unexamined 147-vs-359 test population gap; known exclusions explicit |
| `G04` | Independent first-seen Japanese chat, document and multi-turn quality | Fixed rubric, individual examples, misses and no training-set contamination |
| `G05` | Separate controlled capability ON/OFF vs current core-only benchmark comparisons | Same snapshot/conditions with individual correct→incorrect transitions reported |
| `G06` | GPQA 198-question current core contract: LIVE-only; parallel inheritance and serial learning | Same revision, declared exact conditions, 90-minute acceptance and no gold leakage |
| `G07` | Fixed source packet crossover as *diagnosis*, not official performance claim | Code × packet factorial comparison isolates observable cause of outcome change |
| `G08` | Determinism, trace integrity and authentic evidence-vs-simulation tests | Exact-repeat internal behaviors and explicit divergence from changing Web/state |
| `G09` | CPU/8GB resource envelope, scaling, p50/p95 latency, active memory and cost | Measured on real target hardware/config; no assumed zero-cost reasoning |
| `G10` | Windows/Linux/macOS and installation/package portability | Installed binaries and actual runs on each claimed target environment |
| `G11` | Cloud deployment, access control, browser acceptance and public reliability | Authenticated and unauthenticated UI/API flows observed independently |
| `G12` | Publication, license, source/asset provenance, immutable releases and English docs | New version hashes, human-visible limitations, secret/large-data boundary audit |

## Required whole-route scenarios

These scenarios must start from their actual public route, with original input bytes and real downstream consumption. A supplied final action/result, test-only interpretation, or expected answer injected ahead of the normal path is component evidence only.

| ID | Scenario | Required evidence |
|---|---|---|
| `S01` Meaning | Novel Japanese request with omitted subject, negation, conditions, date | Source spans preserved; correct referent and scope in answer |
| `S02` Exact follow-up | `2+3` then `詳しく説明して`, then change one operand | Actual saved `5`, explanation of same computation, versioned correction |
| `S03` Argument | Two opposed claims with changed shared premise | Dependencies explicit, only affected conclusions invalidated |
| `S04` Research | Search with contradictory actual fetched documents; inject snippet-only and 429 | Body-span support, conflict/stop, source and failure provenance |
| `S05` Writing/code | Unseen edit and repair requests with file/result constraints | Complete modified artifact; tests claimed only if observed |
| `S06` Learning | Valid rule composition, unseen future task, counterexample | Evidence-backed formation, future effect, invalidation and old lineage |
| `S07` Integration | Web → calculation → structured report → authorized file write | Real consumed intermediate artifacts and exact write receipt |
| `S08` Crash/effect | Stop process after external `ATTEMPTED` before receipt | `UNKNOWN` until reconcilable, no duplicate external mutation |
| `S09` Cross-client | Same case resumed via CLI and browser after restart | One revision lineage, no fabricated or contradictory second answer |
| `S10` Resource | Long case history with constrained working set | Bounded RSS, original provenance retained, typed budget stop |
| `S11` Port attribution | Same task with external specialist OFF/ON | Separate intrinsic and provider-enhanced result labels |
| `S12` Modality | Transcript/OCR/video-frame-derived input from a labeled provider | Source/provider/time identity, no claim of native sensory perception |

## Required R2 falsification scenarios

All **R2-01 through R2-22** require successor observations. The R1 ledger's `SPEC_CORRECTED` status certifies corrected normative text, not product acceptance. Preserve the negative cases even when a happy path passes.

| ID | Counterexample | Required invariant |
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

## Non-neural language and learning gates

`A01–A03` and `R2-17` require all six native Core components together: finite versioned `TOKEN_SPACE`, bounded `CONTEXT_STATE`, non-neural `PARAMETERIZED_TRANSFORM`, normalized `CONDITIONAL_NEXT_TOKEN_DISTRIBUTION`, declared `SEQUENCE_OBJECTIVE` and deterministic `DECODING_IF`. Reusing an inherited algorithm still needs inspected guarantees and comparable unseen-input/meaning/learning/resource evidence. A model that merely scores an already-fixed answer must not be presented as proven general natural-language composition. Prompt-specific response templates and external neural substitution cannot certify native capability.

`CAP-D05–CAP-D08`, `CAP-D12–CAP-D14`, `S06` and `R2-13` require memory → inference → adaptation through ordinary execution, future use on an unseen task, out-of-domain/counterexample behavior, restart and dependency invalidation. A user-declared premise or rule must retain that guarantee class; storing a rule or replaying its training example is not observed generalization. Empirical promotion needs a predeclared scoped validation contract, not an arbitrary agreeing-example count.

## Independent quality population prerequisite

Reconstruct the exact historical source/evaluator contract and fix item-level expectations before replacing/adopting data. The required populations are **24 unseen general questions**, **12 unseen writing/editing tasks**, **10 conversations of 4–6 turns**, and **30 meaning transformations**, plus contract-derived adverse inputs. The identity audit has not established an exact item-level historical quality dataset. Its absence remains an unresolved evidence prerequisite; it is not permission to choose easier examples or delete these requirements. Independent evaluator status, data lineage and potential answer leakage must be explicit.

## Exact GPQA contract and prerequisites

The recovered upstream references are `評価/評価契約_v3.md`, `評価/GPQA評価二系統契約_v1.json` and `tools/評価契約.py` at extraction commit `9307d7e2b1799750ecef1957d85128fcbe403d02`. Check the actual current contract/runner revision before an official new measurement and retain any difference from the extraction reference.

| Field | Unchanged acceptance obligation |
|---|---|
| Dataset | GPQA Diamond all **198**, indices **0…197**, CSV SHA-256 `41d1213cd7a4998605a26c2798500652572007161b3a92817ba46b35befcd305` |
| Choice ordering | Shuffle seed **0** |
| Intake | One problem bundle formed once per case; same bundle reused downstream |
| Native route | Versioned successor equivalent of the Core selection route; no specialist/external judging contamination in core-only measurement |
| Evidence | **LIVE_ONLY** freshly acquired sources; saved packets allowed only in separately labeled diagnosis |
| Providers | Europe PMC and Crossref enabled; Wikipedia **en**; OpenAlex disabled |
| Gold | Scoring only after Core execution; no label/score learning |
| Parallel inheritance | Independent case states, all198 complete within **90min**, at least **40/198** as inheritance floor |
| Serial learning | Same Core in index order0…197, permitted state inheritance, all198 complete within **90min**, gold excluded |
| Comparison | Same repository/data/route/source configuration; retain actual LIVE drift and timings |

The official dataset is obtained by the historical runner from `raw.githubusercontent.com/idavidrein/gpqa/main/dataset.zip`; verify the exact CSV digest after acquisition. A different source is acceptable only if it reproduces the identical permitted dataset and boundary. No dataset acquisition, score, 90-minute viability or Rust inheritance result is established by this obligations document. The historical 29→80 and40/198 observations are not new Rust scores. Do not replace a full measurement with partial results or reinterpret a timeout as a passing score.

## External and platform prerequisites

These are requirements to inspect/test for their named operation, not assertions that a credential or network path is missing. Use existing environment bindings and the approved HTTPS proxy, preserve TLS verification, and test the exact read-only operation before requesting any new configuration.

| Requirement IDs | Concrete prerequisite and verification boundary |
|---|---|
| `C01–C07`, `C11–C13`, `S04`, `S07`, `G06` | Public-provider access to `www.ebi.ac.uk` (Europe PMC search), `api.crossref.org`, `en.wikipedia.org`, and actually selected body/redirect destinations. `europepmc.org` and `doi.org` may be article/link destinations. Record exact permitted hosts and actual fetch outcomes, including403/429/timeouts. Reaching a package registry or GitHub does not establish these paths. |
| `C03–C05`, `C10`, `C13`, `G07` | Separate listing and body packet capture, actual bytes/spans/times/status/provider/version, replay provenance and a fixed-vs-LIVE contamination guard. An article URL or snippet is not a fetched body. |
| `C08`, `C09`, `F09` | Chosen HTML/PDF parsers and representative licensed public documents; preserve source offsets/page boundaries, warning/size failures and separate OCR attribution. Passing text extraction does not prove native image understanding. |
| `C01`, `C07`, `E10`, `G11` if Google is the selected route | Configured project/location/engine and usable scoped Google authentication; selected flows may require `discoveryengine.googleapis.com`, `oauth2.googleapis.com`, and `iamcredentials.googleapis.com`. These are optional provider-specific requirements, not reasons to ask for raw secrets before checking existing bindings. |
| `E06`, `E10–E12`, `F10`, `S08` | A specifically authorized effect target/provider with tested idempotency or reconciliation, exact persisted request/actor/scope/identity, cancellation and genuine receipts. Without such a provider, unknown effects must suspend; an injected observation cannot establish actual once-only remote delivery. No new external-send authorization comes from this ledger. |
| `F02–F04`, `F07`, `F12`, `S09` | Installed CLI and HTTP/browser consumers sharing one authenticated actor/case/revision lineage; actual restart, auth/error, streaming/revocation/cancel and displayed-answer observations. Two URLs into one untested client do not establish two heterogeneous consumer contracts. |
| `F06`, `F08–F12`, `S12` | Explicitly selected multilingual/speech/image/video/application adapters, runtime availability, licenses, model/provider attribution, source/time identity, allowed data egress and any actual credentials. Do not invent provider output or count provider quality as native Core quality. |
| `F13`, `R2-16`, `R2-19` | Independently implemented execution/encoding/backend projections and common abstract-operation/effect vectors. Compiler or memory-side Core substitution is a backend replacement requiring semantic equivalence, not a peripheral-port label. |
| `G09`, `G10`, `R2-18` | Actual target/compiler/linkage availability and measured CPU/RSS/storage/latency/headroom, including installed Windows CPU/**8GB** profile. At least two independent OS/CPU/encoding projections are required before cross-target semantic equivalence is claimed. Linux compilation alone does not establish Windows/macOS behavior. |
| `G11`, `G12` | Separately authorized deployment/audience/cost/credentials/IAM/release target and actual independent authenticated/unauthenticated flows where required. Local source work is not permission to deploy, publish a release or send to another repository. |

## Durability, authority and effect acceptance

For the selected storage adapter, verify single Core writer/CAS revision continuity, authentic actor-to-case checks through all reads/replay/exports, immutable originals and source-specific sensitivity/egress propagation. If using SQLite, observe foreign-key enforcement on every connection, actual declared relationships, transactional revision/journal identity, and the selected durability profile. Inject failures before/after blob flush/publication and logical commit. Recompute referenced blob hashes and event chains on recovery; orphans must not create phantom revisions, and missing/corrupt referenced originals must block adoption.

An action persisted as `ATTEMPTED` or `UNKNOWN` must retain its exact actor/authorization/request/idempotency/reconciliation identity across restart. Neither a timeout nor a fresh token proves failure/safe retry. `ADOPTED` content cannot reach user-goal `COMMIT` until required actual rendering, file/effect/delivery receipts and meaning/format predicates are observed by Core. Output/ports report observations and cannot independently reinterpret, learn or close a goal.

## Reproducibility and closure record

Run the applicable pinned Rust formatting/check/lint/test/build commands using `--locked`, retain actual counts and exit status, and record dependency/license/security findings. Then attach ordinary-input observations, adverse cases, predecessor-equivalence/application records, resource/target evidence and unresolved IDs. Compilation/unit tests are necessary engineering evidence but cannot replace this full acceptance population.

Each final observation requires: `case_id`, `requirement_id`, `source_commit`, test/implementation versions, input digest, expected predicate, actual artifact hash, provider/source packet, process/toolchain/OS/target, outcome, trace, measurement, failure/root cause and regression effect. Mark `PASS`, `FAIL`, `UNVERIFIED`, `SKIPPED` and `NOT_RUN` separately. A required unresolved route or missing positive behavior blocks a full completion claim; preserve it as a concrete next obligation rather than labeling it an approved exclusion.
