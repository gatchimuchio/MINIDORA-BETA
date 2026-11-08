# 03 — Integrated Construction Plan

**Purpose:** Build the entire next-generation MINIDORA execution path before optimizing its parts.  
**Authorization:** Design and plan only. Implementation, release, deployment and external access still require the relevant user instruction.  
**Target repository:** `gatchimuchio/MINIDORA-BETA` only; original `NOTNN-LLM-MINIDORA` remains read-only.

## Construction principle

The complete product is the inspection object. A list of individually working subsystems is not a working product. Complete the interfaces and integrated functionality across the **whole requested scope**, then inspect component defects against the live whole system, then perform full acceptance. A necessary compile/smoke check during construction is fault prevention, **not a claim that the associated capability is complete**.

## F0 — Baseline and complete population census

**Subject:** Frozen `0.6.0b1` beta, all 414 inherited public source files, beta changes, all accessible upstream specs/tests, current entry points, external ports and historical evidence.  
**Purpose:** Determine what already exists and what is disconnected or missing before creating alternatives.

Operations:

1. Pin beta Git `main`, upstream read-only Git `main`, beta source and artifact digests, Python/dependencies, public API names and expected selection of 313 regression cases.
2. Construct a source/test/package inventory from the *actual* Git tree, not only file names in a report. Beta retains 147 tests; upstream history reports 359 public tests. Account for missing and non-portable tests individually.
3. Trace actual entry routes for general natural text, selection/science, learning, retrieval, calculation, conversation, documents, code, memory, external actions and renderers. Identify an existing implementation, an unused implementation, a missing connector or true missing capability.
4. Attach each item in the [capability register](02_CAPABILITY_REGISTER.md) to its source proof/test and unresolved gap. Add any uncovered public feature to the population. Keep internal theory mapping outside this public repository.
5. Read past success **and failure** cases with their exact dates, code revisions, datasets and evidence modes. Treat previous defects as hypotheses until reproduced in the selected current version.

**Deliverable:** A versioned machine-readable register, dependency/authority graph, reproducible baseline smoke, actual test coverage matrix, and explicit unavailable-evidence list.  
**Exit:** No known inherited capability is unaccounted for; unknown remains `UNVERIFIED`, not silently omitted.

## F1 — Derive one coherent target architecture from observable contracts

**Subject:** Entire input → meaning → core action → evidence → learning/state → result → delivery → audit system.  
**Purpose:** Optimize responsibility and operation once for the whole system, regardless of historic module boundaries.

Operations:

1. Formalize requested goal, input identity, relation/claim representation, capability contracts, evidence packets, state/learning ownership, external effects, output and terminal semantics from [01](01_SYSTEM_SPECIFICATION.md).
2. Draw actual present authority and data-flow edges; find multiple owners, premature narrowing, repeated interpretations, missing returns and dead-end modules.
3. Compare `RETAIN`, `REWIRE`, `REPLACE`, `NEW` and `EXTERNAL_PORT` for each function, with explicit effect on compatibility, determinism, public/private boundary, time, memory and actual result quality.
4. Specify **one** end-to-end state transition discipline, with enough memory to preserve old observations yet a bounded active working set. Preserve original goal until a new authorized request version.
5. Include all product consumers and future typed modality ports in the full system design, without claiming a speech, image or translation model exists where only an adapter is proposed.
6. Resolve conflicting historical design decisions using their version and newer explicit instruction. No old experimental gate silently becomes today's second decision authority.
7. Define testing and observability *as part of the architecture*, not a separate post hoc reporting system.

**Deliverable:** Complete target responsibility graph; logical data schemas; compatibility/migration contract; source/ownership inventory; effect/stop matrix; explicit rejected alternatives; acceptance mapping for **every** capability row.  
**Exit:** A complete connected design, not a set of unrelated proposals. A structural gap requires returning to F1, not hiding it with a downstream exception.

## F2 — Construct the complete vertical system

**Subject:** The target engine and all accepted capability routes.  
**Purpose:** Make the common lifecycle and full requested feature population work through their real entry points in **one coordinated implementation pass**.

Construction order inside this phase:

1. Establish versioned working boundary and reconstruction procedure while leaving the frozen beta code and artifacts recoverable. Do not edit the upstream repository.
2. Implement/reconnect all shared contracts and capability ports, resource/effect constraints, state/memory, learning path and output semantics across one connected engine.
3. Integrate general text, relevant reference, arithmetic, relations, multi-step action, writing/editing, code, specialized abilities, conversation, persistence and auditable source-based answers. Do not leave a capability accessible *only* through a handcrafted test API when the requirement is normal user input.
4. Complete external capability boundaries (search, document, API, authorized effects, multimodal/translation consumers) with explicit typed behavior, known limitations and no invented provider quality claims.
5. Enforce the unique owner of action success and learning/adoption and a separate display/delivery boundary. Recovery operates on real failed state with bounded budgets.
6. Connect trace/evidence/restore paths throughout. Ensure results can be consumed by the next stage and by follow-up messages, not merely stored in logs.
7. Perform minimal compile/import/smoke checks as needed to avoid propagating obvious faults. No subsystem is marked final just because its isolated tests pass.

**Deliverable:** One executable system with fully wired required feature routes and truthful status of any external dependencies.  
**Exit:** Public input-to-output flow is integrated for the accepted population; unsupported live providers terminate explicitly. A demo with supplied actions is a wiring check only.

## F3 — Inspect the whole, diagnose locally, verify the whole

**Subject:** The constructed whole system and its actual user-facing outputs.  
**Purpose:** Find and correct root causes with controlled evidence, without fragmenting acceptance into disconnected feature completions.

1. Execute the **whole-engine E2E matrix first**: novel questions, purpose, reference, exact computation, multi-step result consumption, learning, memory reuse, corrections, writing, cancellation, failure and delivery.
2. For every failure, retain input, code version, state, source/evidence packet, actual intermediate outputs, stop signature, test environment and resource use. Classify failure as interpretation, evidence identity, capability absence, miswiring, state/learning, budget, judgment, rendering or external effect.
3. Reproduce locally with controlled fixed packets when source drift would confound causation. Distinguish from independent LIVE performance acceptance.
4. Correct the responsible owner (including F1 design if needed). No keyword-specific patch or duplicate lower-layer judge unless the root contract requires it.
5. Re-run focused tests relevant to the change and then the **entire affected E2E/regression population**. Count correct→incorrect regressions by item.
6. Execute full applicable upstream tests, independent new-task quality, process persistence, hostile/ambiguous inputs, CPU/memory/time, cross-OS and external-system checks **only when those claims are intended**. Keep unperformed checks unaccepted.

**Deliverable:** Failure-to-root-cause ledger, source-linked traces, unchanged failure population, complete non-regression diff, independent answer-quality individual records and performance measurements.  
**Exit:** All mandatory rows pass their specified contracts or the owner explicitly changes scope/version with a recorded reason; no softening thresholds to make a run pass.

## F4 — Freeze the new version and publish accurately

**Subject:** Target source, runtime, package, docs, evidence and release assets.  
**Purpose:** Make the newly accepted object reproducible and distinct from `0.6.0b1`.

1. Fix new version identity, exact source tree and dependency lock. Produce diff against frozen beta, preserving historical commit and ZIP evidence.
2. Freeze new test environment, artifacts, per-file evidence and SHA-256; audit licenses, attribution, external data/credentials and publication boundary.
3. Update English-facing overview, API guides, examples, scope, caveats, `AGENTS.md`, change history and source/release records **for the new version**. Record Japanese contract names without changing runtime symbols.
4. Run authorized target-environment/release acceptance. GitHub Actions is an acceptance tool, not a substitute for ordinary local checks. No unrequested deployment, release publication or service-wide permission change.
5. Record accepted, rejected, unmeasured and deferred capabilities separately; attach measurable improvement to the actual same-condition baseline.

**Deliverable:** New immutable version with independently reproducible artifact identity and evidence.  
**Exit:** User accepts the observed target object and scope. The previous beta remains fully recoverable.

## Decision and time policy

- No calendar dates, language choice or implementation percentages are fabricated before the F0 census and resource observation.
- F0 and F1 must cover the **whole target** before F2. Within F2, build the **full integrated product**; use local tests to control risk without turning each tiny component into an independent completion milestone.
- If full-source evidence contradicts the proposed system boundary, revise the system model and update the entire three-document contract. Do not silently lower a required quality population.
- Every phase can return upstream after a contrary observation. A pass is evidence for exactly the executed version, environment, inputs and task population.

