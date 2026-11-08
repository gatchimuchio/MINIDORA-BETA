# 01 — Whole-System Specification

**Contract class:** Public behavioral specification for a *future*, independently versioned MINIDORA implementation. **Not** an assertion that this behavior already runs in `0.6.0b1`.

## 1. Subject, purpose and success

**Subject:** MINIDORA, a non-neural/non-Transformer language-and-execution system with explicit capability, evidence, state, memory and output controls.  
**Purpose:** Accept a general request, preserve what the requester actually wants and what is authorized, compose existing and newly acquired capabilities, execute bounded operations, learn through usable experience, and deliver a useful, traceable result or an explicit non-success state.

Primary success is **real user-task completion** over diverse previously unseen requests, with faithful meaning, usable results and inspectable causal evidence. Line count, module count, benchmark score without its conditions, apparent response fluency and internal state activity are not success substitutes.

The system is not limited to chat. Conversation, research, computation, document transformation, planning, translation applications and external operations are distinct *consumers* of the common engine. Do not let a consumer redefine engine-level ownership.

## 2. Ground-up derivation and non-negotiable boundaries

Start from the observed task and its required state changes. Determine necessary data, relationships, operations, authorities, failure handling and verification before choosing language/framework/process topology. The current three-machine beta is a compatibility and responsibility reference, **not proof that its internal decomposition is universally optimal**. A proposed new decomposition must preserve the external single-interpretation/single-execution/single-delivery obligations unless an explicit, separately accepted contract replaces them.

Maintain these public invariants:

1. **Original request identity:** Raw input and its effective context remain separately recoverable from any interpretations or summaries. Subject, purpose, constraints, scope, temporal conditions and original evidence remain tied to source identity.
2. **One control authority:** One explicitly identified core owns execution, goal/state changes, learning/adoption, and final internal success/failure judgment. Adapters, retrieval tools and renderers do not independently claim completion or silently change purpose.
3. **Evidence before adoption:** Retrieval, claims, hypotheses, computations, actions and conclusions have distinct types and derivations. Evidence provenance and uncertainty remain inspectable.
4. **Bounded, meaningful progress:** Budget increases require observed progress toward the original goal or a justified bounded continuation; state churn, memory growth and self-generated learning activity alone cannot renew a budget.
5. **Versioned integrity:** Input versions, provenance, ability revisions, state revisions, stop/rollback events and output deliveries are distinguishable. Prior evidence remains historically recoverable.
6. **Safe effects:** External sending, device actions, credentials, publication and deployment use explicit authority and target boundaries. An operation is not implied by a successfully rendered answer.
7. **Non-regression by evidence:** Preserve verified inherited behavior across the declared supported population. Never use a local success to declare universal completion.
8. **Language and disclosure:** Japanese remains the implementation/semantic baseline; external-facing explanatory documents are English. Preserve existing Japanese public symbols. No private HDS reconstruction is published.

## 3. Logical end-to-end lifecycle

1. **Receive:** Bind `(case_id, input_id, input_version)` to raw content, language/channel, actor, permissions, time, source context and user goal. Invalid/duplicate versioned input has an explicit outcome.
2. **Interpret once:** Produce an immutable interpretation with explicit source spans, referents, subjects, relations, premises, negation, conditions, temporal/scope limits, intended output and unresolved ambiguity. Later operations may derive *new, versioned* hypotheses, not silently rewrite this source object.
3. **Establish work state:** The core owns goal contract, success/stop predicates, evidence references, working memory, capability availability, resources and allowed effects. Existing conversation references are resolved against actual prior results, not guessed from the latest prompt alone.
4. **Plan and select:** Form typed candidate operations from need and available evidence; distinguish direct answers, reading, computation, inference, code/document work, clarification, or authorized side effects. Unknown operation paths may be constructed, tested and then accepted; finite exploration is not a ban on novelty.
5. **Execute and observe:** Record actual inputs, outputs, failures and resource use for each invoked capability. No simulated result is silently promoted as an observed result.
6. **Evaluate consequences:** Compare actual state/output change against the original goal, allowed constraints, evidence and candidate alternatives. Preserve counterevidence and invalidated assumptions. Continue only within the defined budget and progress rule.
7. **Learn where applicable:** Connect observation → memory → inference → prospective adaptation → acceptance or rejection → future use within the same core control state. Do not treat repeated storage or answer-label memorization as generalized learning.
8. **Render and deliver:** Construct substantive, appropriately formatted text, data or media based on adopted results; preserve claim-to-evidence links. Separate rendering from externally authorized delivery. A refusal, clarification, suspension or failure is explicit and machine-readable.
9. **Archive and replay:** Preserve sufficient immutable evidence and revisions to reconstruct why an action or response occurred, while keeping the active computational working set bounded.

This is an **observable lifecycle contract**, not a complete implementation map or a description of unpublished HDS internals.

## 4. Minimum public data contracts

| Object | Required public fields/meaning | Update authority |
|---|---|---|
| `InputEnvelope` | Case/input/version identifiers; raw payload/fingerprint; source/channel/locale; initiating actor; applicable permissions and context pointer | Input boundary; immutable after seal |
| `Interpretation` | Original subject/purpose; relations; required and forbidden outcomes; unresolved references; scope/negation/time; source-span lineage; interpretation version | Input boundary initially; core only records distinct subsequent hypotheses |
| `GoalContract` | Original goal identity, required observable outcomes, bounds and permitted effects; success/stop predicates and change history | Core; user changes require a new authorized version |
| `WorkState` | Active goal, capabilities, resource budget, task frontier, working-set pointers, previous actual outputs, outstanding gaps and state revision | Core |
| `EvidencePacket` | Retrieval request; provider/config/version; rank; URL/document/page; response status; failures/retries; exact captured text/bytes or fingerprint; time; independence/lineage | Read-only observation port; core owns adoption |
| `ClaimRelation` | Subject-predicate-object or other typed relation; source span; polarity; modality; conditions; time/scope; derivation; support/refutation and dependency | Core derives; sources cannot self-adopt |
| `CapabilityContract` | What it can do; input/output type; version; requirements; effect/authority; time/cost; failure and cancellation semantics | Catalog/provider describes; core selects |
| `ActionRecord` | Invoked capability/version; input identity and fingerprint; actual outcome; measured state delta; resource use; evidence and stop/failure signature | Core records; append-only evidence |
| `LearningRecord` | Origin evidence; candidate rule/relationship; induction conditions; invalidation/counterexamples; adoption/rejection; scope and future-use outcomes | Core only |
| `OutputEnvelope` | Adopted result or explicit stop category; content and format; provenance/citations; output version; delivery intent/status | Output component renders; delivery port performs authorized transmission |
| `TraceEnvelope` | Source/version, action/evidence dependencies, interpretation and decisions, terminal cause, result checksum, redactions and replay pointers | Append-only audit writer; no truth claim from hash alone |

These are **logical contracts**, not mandates to create new Python classes, an exact private-theory schema or a complete mapping to historical files. Reuse compatible existing objects when the required behavior is already present.

## 5. Functional ownership and interface tests

| Boundary | Must guarantee | Must not do |
|---|---|---|
| Source and input | Source fidelity, version identity, permission metadata, one-time baseline interpretation | Reinterpret later and overwrite raw purpose; claim learning/adoption |
| Engine core | Unified goal, operation selection, evidence evaluation, learned state, progress and terminal control | Delegate final responsibility to a model/provider, output renderer or a second invisible judge |
| Memory | Immutable history, provenance, scoped retrieval and dependency invalidation; working-set efficiency | Discard old evidence on correction; feed all history into each computation |
| Capability/knowledge port | Typed operations, actual measurements, failures, explicit effect and data boundaries | Privately change user instruction, conceal cost, assert completion or update core learning alone |
| Presentation and delivery | Useful content; exact contract fields; pending/failed delivery status; source attribution | Re-run an operation or learn as a side effect of formatting; silently send to external targets |
| Independent acceptance | Reproduce relevant source, behavior, artifacts and comparisons | Conflate frozen-packet diagnostics, official LIVE tests, historical scores and product demos |

## 6. Required capability families

The mandatory implementation population is defined item-by-item in [02_CAPABILITY_REGISTER.md](02_CAPABILITY_REGISTER.md). It includes:

- Non-neural language-model and constrained decoding properties; Japanese and cross-language meaning/relationship preservation; original subject/argument scope and discourse continuity.
- Everyday answers; composition, revision, reasoning, analogies, comparisons, mathematical/structured-data work, code and multi-step operations.
- Purpose/plan/action execution, failure recovery, rollback, suspension/clarification, policy, budget and permission checks.
- External reference, full-document/PDF handling, provider diversity, independent support/contradiction, evidence/version identity, citations and causal trace.
- General learning, formation of reusable relationships, memory, unlearning/invalidating incorrect inferences, durable sessions and selective working memory.
- Capability discovery, compositional reuse and historical module-on/off comparisons without treating specialists as the core itself.
- CLI, local API, product chat, reviewable UI, future stream/multimodal typed ports and user-controlled integrations.
- Full end-to-end tests, novel-task quality, cost/latency/memory, independent evaluation, portable runtime and release/audit discipline.

## 7. Failure, unknown and correction semantics

Public terminal categories: **COMMIT / SUSPEND / REJECT / FAIL / CANCEL** (or an explicitly versioned compatible representation). Their exact historical API spellings may differ; do not silently rename executable enum values.

- `COMMIT`: Required observable goal state is actually achieved or accepted under the declared contract; not merely an action returning success or a text produced.
- `SUSPEND`: A resolvable missing input, permission, evidence or resource threshold prevents adoption; preserve what would resume the task.
- `REJECT`: Policy, contradiction, invalid evidence or forbidden operation prevents adoption; retain reason without emitting unsupported content.
- `FAIL`: Actual execution, dependency, serialization, provider or invariant failure, with root cause and recovery envelope.
- `CANCEL`: Explicit authorized stop and effect accounting.

Corrections append a new interpretation/evidence/result revision and invalidate dependent conclusions as needed; they do not erase the original record. Avoid eternal suspension when the core already has sufficient result and evidence for a bounded explanation, but do not manufacture support to force an answer.

## 8. Generalized learning acceptance

Learning is observable only when **memory → inference → adaptation** is demonstrated on new inputs:

1. A new relation/procedure/hypothesis is formed from actual prior evidence and stored with conditions and source.
2. The normal public input route and specialized task route can recover it through the same core authority.
3. A genuinely new task uses that formation to change an observable result or resource use, compared with the controlled pre-learning baseline.
4. Counterevidence can revise, restrict or invalidate the formation without deleting prior evidence.
5. No evaluation gold label, future answer, problem-specific shortcut or hidden assessment leak is used.
6. The new learning does not merely duplicate a prior method and inflate working cost.

If any element is missing, record the observed intermediate property, not `general learning accepted`.

## 9. Explicit constraints on implementation choice

Do not preselect Python for the next design merely because the beta is Python; verify host/runtime contracts, practical reuse and resource costs before changing language. Conversely, do not rewrite healthy code to demonstrate novelty. Keep non-neural/non-Transformer public target distinct from the choice of memory store, language frontend, API transport or external tools.

The frozen beta uses a three-machine facade and Japanese names; the next design may reuse or reorganize internals **only with a documented compatibility/authority proof and user approval for material semantic changes**. What may not be silently changed: original task semantics, source ownership, single control and final judgment, external effect authorization, public disclosure boundary, historical evidence and release identity.

## 10. Completion definition

The target is not complete until (a) all mandatory capability-register entries have a verified disposition, (b) all required paths can be reached through actual normal public inputs, (c) the end-to-end matrix and relevant non-regression measurements pass, (d) failures/stops/side effects and provenance are observed, (e) portability and performance claims cite actual runs, and (f) the new source/version/artifact boundary is independently auditable. Scope reduction requires the user's explicit decision and a versioned change record.

