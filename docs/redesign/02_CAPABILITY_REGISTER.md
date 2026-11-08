# 02 — Integrated Capability and Gap Register

**Population:** Historical capabilities + frozen beta's observable public surface + newly requested capabilities + future integration ports. Nothing is removed merely because it is not on the beta's primary smoke-test path.

## Evidence and status semantics

| Mark | Meaning | Prohibition |
|---|---|---|
| `B` | A named public implementation, test, interface or baseline mechanism was observed in the isolated beta's repository or acceptance record | Does **not** establish that arbitrary user requests reach it or that it works today |
| `H` | An older upstream version or dated experiment reported a result or design | Does **not** certify the frozen beta or next version |
| `C` | Proposed in prior discussions or research; candidate for integrated redesign | Does **not** mean implemented |
| `?` | Current connectivity, quality or result is **unverified** | Requires current code inspection and/or an actual measurement |

Rows list the **best available evidence type**, not an exclusive status; `B/H/?` means source/history exists and acceptance is pending. Treat any absent code path discovered during census as a recorded gap, not a reason to discard the requirement.

`E2E` below always means input from the **actual public route**, not a unit test that injects a handcrafted interpretation or a final answer. Future consumer-specific capabilities can be fulfilled through typed adapters, but the common engine must still retain source identity, goal, authorization, errors and trace.

## A. Language, meaning, relation and dialogue

| ID | Public capability contract | Evidence | Integration acceptance |
|---|---|---|---|
| A01 | Non-neural/non-Transformer language-model computation; no implicit neural-model substitution | B/H/? | Controlled distribution/decoding outputs, repeatability, actual user-facing consequence |
| A02 | Finite token/sequence/context handling without arbitrary prompt-specific templates | B/? | Novel phrase variants preserve scope and compose a useful result |
| A03 | Conditional language distribution, deterministic tie/selection ordering, explicit state | B/H/? | Fixed state+inputs reproducible; external variation separately explained |
| A04 | Japanese word, clause and subject/object attribution, including omitted subjects only when context supports restoration | B/H/? | Ambiguous and omitted subjects handled without inventing ownership |
| A05 | Negation, exception, quantifier, modality, temporal and scope fidelity | B/H/? | Meaning-preserving and polarity-flipping metamorphic pairs remain distinguishable |
| A06 | Definition target, explanatory predicate, modifier attachment and relative clause boundaries | B/H/? | Unknown real article definitions and predicate scopes traced to source spans |
| A07 | Referential/co-reference continuity across turns, output artifacts and corrections | B/H/? | `2+3` → `Explain that in detail` explains **the actual 5 and procedure**, not unjustified suspension |
| A08 | Persistent topic, goal, requested format, style and previous evidence under conversation changes | B/H/? | Multi-turn corrections, topic switches and return-to-topic obey latest authorized request |
| A09 | Typed relation constraints, direct vs inferred links, dependency and contradiction | B/H/? | Cross-document relation tests do not turn lexical overlap into factual implication |
| A10 | Proposition/discussion audit: claim, target, premises, reasons, inference, conclusion and valid scope | H/C/? | Identify invalid premise shifts, scope changes and argument-to-counterargument alignment |
| A11 | Multi-language Japanese/English/Chinese source-meaning fidelity with loss reports | B/H/? | Back/parallel translations preserve agency, polarity and conditionality |
| A12 | Unknown/insufficient input recognized explicitly, without filling unsupported details | B/H/? | Insufficient claims return typed residuals; enough evidence yields a substantive answer |
| A13 | Natural, useful answers to unseen what/why/how/compare questions | H/C/? | Novel independent chat-quality matrix, not greeting/keyword-only matches |
| A14 | Directed explanation, paraphrase, examples, citations, depth and format changes | B/H/? | Reformat earlier actual answer using its recorded evidence, with no irrelevant retrieval |
| A15 | Meaning representation stays one authoritative object across parsing, reasoning, execution and output | H/C/? | Repeated input is not reinterpreted by downstream stages into a different premise |

## B. Reasoning, composition and general work

| ID | Public capability contract | Evidence | Integration acceptance |
|---|---|---|---|
| B01 | Interpret task type, required output, restrictions and measurable completion | B/H/? | Unseen requests dispatch to correct capability through ordinary entry point |
| B02 | Exact arithmetic and unit/quantity relations; safe symbolic representations | B/H/? | Computation plus follow-up explanation and correction preserve operands and units |
| B03 | Algebra, linear equations, statistics, probability and mathematical notation | B/H/? | Independent structured and natural-language cases with verified intermediate values |
| B04 | Tables, JSON, CSV and structured-document comparison/aggregation/transformation | B/H/? | Real input documents yield schema-valid results, source-location and error records |
| B05 | Multi-document evidence synthesis and explicit inference chains | B/H/? | Distinguish agreement, independence, contradiction and inferred claims |
| B06 | Multi-step problem solving with intermediate outputs consumed by later steps | B/H/? | A downstream action actually uses an upstream result; no artificial progress credit |
| B07 | Coding, code explanation, revision and bounded executable checks | B/H/? | Novel code request produces usable output and scoped diagnostics; never asserts unrun tests |
| B08 | Writing, editing, rewriting, summaries and constrained-format composition | B/H/? | Twelve previously unseen writing tasks satisfy length, content, purpose and format |
| B09 | Source-grounded science, domain specialists and mathematical knowledge | B/H/? | Core alone vs named capability variants, same questions and actual evidence |
| B10 | Candidate generation, comparison, selection, deferred decision and correction | B/H/? | Alternative support/rejection is inspectable; correct answer never injected by evaluator |
| B11 | Causal, hypothetical and analogy-based reasoning with premise tracking | B/H/? | Counterfactual change alters only dependent claims, not unrelated facts |
| B12 | Planning a goal with permissible actions, dependencies and limited resources | B/H/? | Executed plan moves actual goal state and stops at correct boundary |
| B13 | Cross-capability composition with one responsible core | B/H/? | Same request uses retrieval+computation+writing without a hidden secondary judge |

## C. Sources, retrieval, evidence and knowledge

| ID | Public capability contract | Evidence | Integration acceptance |
|---|---|---|---|
| C01 | Public web/search and local reference access, with explicit deployment policy | B/H/? | Authorized search reaches actual retrieved **body**, not only title/snippet |
| C02 | Provider-specific failure, timeout, rate limit and alternative-path accounting | B/H/? | HTTP errors/retries preserve original request, resource costs and failure position |
| C03 | Exact source time, version, URL, hash, title, page and content lineage | B/H/? | Two different retrieval bytes never masquerade as identical evidence |
| C04 | Immutable retrieved **search-list** and **body** packets for diagnosis | H/C/? | Capture provider/order/status/retry and independently replay both stages |
| C05 | Strict separation of fixed-packet regression and official `LIVE_ONLY` benchmarks | H/C/? | Diagnostic replay is never used to inflate a LIVE performance claim |
| C06 | Source-independent positive support vs overlap, contradiction and negative evidence | B/H/? | Candidate scoring identifies unique supported difference; rejects generic lexical hits |
| C07 | Wikipedia, scholarly references (e.g., Crossref, Europe PMC), specialist and local sources | B/H/? | Real connector reachability and attribution measured per configured provider |
| C08 | HTML article main-text parsing, headings, ruby/hidden character boundaries | B/H/? | Source text, warnings and indices survive parsing without invented corrections |
| C09 | Bounded PDF **text** extraction, page retention and warning rejection | H/B/? | PDFs with extraction errors are not silently treated as understood; OCR is distinct |
| C10 | Source-change invalidation; cached earlier evidence and old conclusions retained | B/H/? | Changed source triggers targeted dependent re-evaluation, not historical erasure |
| C11 | Evidence-backed quotations/citations and explicit absence of supporting text | B/H/? | Each factual sentence or quotation maps to the actually used versioned source |
| C12 | Bounded targeted re-retrieval when actual evidence is insufficient | H/B/? | Search expansion happens for a measured gap, within allowance, without self-feeding |
| C13 | Real-data input lineage through both candidate and final answer formation | H/C/? | A replay pinpoints whether change came from source, code, state or provider |

## D. Learning, adaptation, memory and continuity

| ID | Public capability contract | Evidence | Integration acceptance |
|---|---|---|---|
| D01 | Immutable primary record of instructions, observations, failures and corrected views | B/H/? | Original and corrected records both recoverable with independent identities |
| D02 | Working memory limited to current goal and relevant causal dependencies | B/H/? | Long history increases storage without causing unbounded active computation |
| D03 | Core-lifetime retained cognition, references and adaptation state | B/? | Multiple actual requests reuse permitted earlier evidence in same process |
| D04 | Durable session and task state across process/revision restarts | C/? | Explicit restore, dedup, correction and identity tests across fresh instances |
| D05 | General learning cycle: memory → inference → adaptation in normal execution | B/H/? | Pre/post difference on a genuinely new task, via normal public route |
| D06 | Reusable relation/procedure formation with generalization restrictions | B/H/? | New relation applies to new subjects without over-generalized rule adoption |
| D07 | Original source, conditions and counterexamples attached to learned records | B/H/? | False rule revoked/limited; downstream dependent conclusions invalidated |
| D08 | Formation and inherited capability reconciliation with no duplicate computation | H/C/? | Added shortcut does not increase full-cost repeat evaluations for same task |
| D09 | Structured semantic memory/indices separate from full original observations | H/C/? | Archived 140-observation-type workloads retrieve relevant subgraph, retaining origin |
| D10 | Reversible compression and evidence replay rather than irreversible summaries | H/C/? | Compressed entry reopens to exact relevant original evidence |
| D11 | Cross-session goal/purpose/reference/previous actual output continuity | H/C/? | Session restore supports multi-turn content edits and justified refusal |
| D12 | Learning adoption controlled by the same core as action execution | B/H/? | No external module self-commits new rules or creates another success authority |
| D13 | Explicit forgetting/invalidation of inferences, never silent deletion of evidence | H/C/? | Counterexample changes active conclusion while preserving lineage |
| D14 | Evaluation-label leakage and training/evaluation boundary control | H/B/? | Gold unavailable before final scoring; tests detect contaminated input path |

## E. Control, governance, execution and audit

| ID | Public capability contract | Evidence | Integration acceptance |
|---|---|---|---|
| E01 | Case/input/version identity, one-time acceptance and duplicate prevention | B/? | Repeated version rejected; new version and result lookup behave as contracted |
| E02 | Original goal, required states, permissions, forbidden effects and success predicate retained | B/H/? | Goal contract cannot mutate as a byproduct of learning or a retrieved page |
| E03 | Executable operation chain with actual observed state deltas | B/H/? | Internal state churn alone cannot mark user goal as achieved |
| E04 | Finite action/time/cost budgets and progress-sensitive continuation | B/H/? | Irrelevant loop stops; valid multistep progress has bounded continuation |
| E05 | Distinct terminal success, suspension, rejection, failure and cancellation | B/H/? | Distinct signatures, retained evidence and resumability conditions exposed |
| E06 | Permissioned external effects, explicit output delivery and rollback/compensation | B/H/C/? | Controlled allow/deny tests; no implicit sending on rendering alone |
| E07 | Capability versioning, typed inputs/outputs, substitution and composition | B/H/? | Toggle capability with same goal, log changed behavior and regressions |
| E08 | Source/result/failure causal trace, independent replay and linked hashes | B/H/? | Replay shows actual decision inputs; hash alone never asserted as truth proof |
| E09 | Input → core → output responsibilities without duplicate decisions | B/? | Complete behavior from real request through terminal rendering and explicit delivery |
| E10 | External platform/tool boundary with cancellation, retry and cost policy | C/? | Two independent consumer applications share the same port contract |
| E11 | Security, input trust, prompt-content isolation, data release and privacy | B/H/C/? | Retrieved text cannot grant its own authority; forbidden writes/sends blocked |
| E12 | Failure recovery, partial rollback and continuation from real result state | B/H/? | Retry consumes existing evidence and budget; does not invent a new goal |

## F. Product experience and extensibility

| ID | Public capability contract | Evidence | Integration acceptance |
|---|---|---|---|
| F01 | Existing Python package, CLI and controlled beta facade | B | Reproduce declared public entrance and exact Japanese API without renaming |
| F02 | Product local HTTP chat/trace/capabilities/health entry points | H/B/? | Exercise real API from an installed runnable version, auth and error cases |
| F03 | Usable browser chat, real answer visibility, source and trace inspection | H/C/? | New browser session answers previously unseen multi-turn requests |
| F04 | Explanation/format refinement of the **actual** previous response | H/B/? | New instruction edits/reasons over stored output even after math or source task |
| F05 | General search, document, calculation, code and writing experiences | B/H/? | Representative unseen workflows from one normal user surface |
| F06 | Explicit multilingual text API and separate output localization | B/H/? | Japanese semantics, English-facing integration and language-fidelity tests |
| F07 | Streaming/chunked input and output, provisional/revoked/final state | C/? | Cancel, revise, finalize and replay a stream without multiple adoption authorities |
| F08 | Audio/speech interfaces via replaceable input/output adapters | C/? | Typed transcript/voice output retains speaker, time and evidence boundaries |
| F09 | Image/document/video interfaces via replaceable modality adapters | C/? | Unknown perception is reported; no claim that text-only PDF handles images |
| F10 | Calls, messaging, external software and device interactions through controlled ports | C/? | Effect audit and permissions carried from intake to actual transmission |
| F11 | `konjac-Translation` as independent multilingual/multimodal consumer | C/? | End-to-end typed translation handoff, but external model quality remains separate |
| F12 | Independent runtime integrations (e.g., D4Pocket/BLUE-TANUKI) without control inversion | C/? | Two heterogeneous consumers keep one goal/authority boundary and trace |
| F13 | Compiler/IR, alternative memory/compute layouts and model-analysis research | C/? | Compared behind a public port by measured effect; not silently declared inherited |
| F14 | Reviewer-ready real demo, English guide, concrete tasks, and reproducible evidence | B/H/C/? | Live interactive response plus trace and non-claim statement; GIF/video supplementary |

## G. Evaluation, performance and release operations

| ID | Public capability contract | Evidence | Integration acceptance |
|---|---|---|---|
| G01 | Exact source/test/requirement census before new code | B/H/? | Every relevant inherited public item dispositioned: retain, merge, revise or reject |
| G02 | Whole-engine E2E closure on a real input, before treating local modules as proof | B/H/? | Request → evidence → capability → goal → answer and replay demonstrated |
| G03 | All applicable upstream and beta regressions, negative tests and version compatibility | B/H/? | No unexamined 147-vs-359 test population gap; known exclusions explicit |
| G04 | Independent first-seen Japanese chat, document and multi-turn quality | H/C/? | Fixed rubric, individual examples, misses and no training-set contamination |
| G05 | Separate controlled capability ON/OFF vs current core-only benchmark comparisons | H/? | Same snapshot/conditions with individual correct→incorrect transitions reported |
| G06 | GPQA 198-question current core contract: LIVE-only; parallel inheritance and serial learning | H/? | Same revision, declared exact conditions, 90-minute acceptance and no gold leakage |
| G07 | Fixed source packet crossover as *diagnosis*, not official performance claim | H/C/? | Code × packet factorial comparison isolates observable cause of outcome change |
| G08 | Determinism, trace integrity and authentic evidence-vs-simulation tests | B/H/? | Exact-repeat internal behaviors and explicit divergence from changing Web/state |
| G09 | CPU/8GB resource envelope, scaling, p50/p95 latency, active memory and cost | H/C/? | Measured on real target hardware/config; no assumed zero-cost reasoning |
| G10 | Windows/Linux/macOS and installation/package portability | B/C/? | Installed binaries and actual runs on each claimed target environment |
| G11 | Cloud deployment, access control, browser acceptance and public reliability | H/C/? | Authenticated and unauthenticated UI/API flows observed independently |
| G12 | Publication, license, source/asset provenance, immutable releases and English docs | B | New version hashes, human-visible limitations, secret/large-data boundary audit |

## Mandatory population closure

For **every row** record these columns in a machine-readable implementation inventory before claiming completion:

`id | requirement_revision | baseline_presence | source_ref | entry_route | ownership | implementation_choice | test_id | observed_result | limits | disposition | release_revision`.

- `baseline_presence` is not a pass/fail judgment; exact source presence and execution connectivity are separate fields.
- `disposition` is one of `RETAIN`, `REWIRE`, `REPLACE`, `NEW`, `EXTERNAL_PORT`, `REJECT_WITH_REASON`, `UNVERIFIED`. For all required behavior, `REJECT_WITH_REASON` needs the user's explicit scope decision.
- Do not silently cut rows that appear difficult or duplicate another feature. Show the equivalence proof if merging entries.
- The register is **inclusive but provisional**: the complete source census may discover additional capabilities. Add them before engineering closes; do not claim the historical set is literally exhaustive from filenames alone.

## Reference groups (not a theory-to-code crosswalk)

- Beta [`README.md`](../../README.md), [`BETA_READINESS.md`](../../BETA_READINESS.md), [`SOURCE_LOCK.md`](../../SOURCE_LOCK.md), installed public source and 147 selected test files.
- Upstream read-only [`README.md`](https://github.com/gatchimuchio/NOTNN-LLM-MINIDORA/blob/main/README.md), [`設計/README.md`](https://github.com/gatchimuchio/NOTNN-LLM-MINIDORA/blob/main/%E8%A8%AD%E8%A8%88/README.md), [`評価/README.md`](https://github.com/gatchimuchio/NOTNN-LLM-MINIDORA/blob/main/%E8%A9%95%E4%BE%A1/README.md) and [`CURRENT_CANONICAL.md`](https://github.com/gatchimuchio/NOTNN-LLM-MINIDORA/blob/main/CURRENT_CANONICAL.md).
- Historical reports dated 2026-10-03, 10-07, 10-08, 10-13 and 2026-11-03 are **investigation leads**, not present-tense code diagnoses. Private reports are not linked or republished here.

