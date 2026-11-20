# Rust implementation status — development alpha

The current development identity is `0.7.0-alpha.11`, schema `minidora-rust/0.7.0-alpha.11`, pinned Rust `1.99.1`, edition 2024. Ordinary alpha 11 opens reject alpha 7 through alpha 10 Rust store metadata without changing it. The explicit `minidora-rust migrate --db DIRECTORY` command verifies an existing alpha 7, alpha 8, alpha 9 or alpha 10 store and updates only its schema metadata; immutable case/effect records and blobs retain their identities. See [schema-migration-validation.json](schema-migration-validation.json) for historical alpha 7→8 checks, [B51 validation](construction/MB-BUILD-20261115/b51-temporal-scope-validation.json) for alpha 7/8→9 migration checks and [B53 validation](construction/MB-BUILD-20261115/b53-build-observation.json) for alpha 7/8/9→10 migration checks. The frozen predecessor baseline remains `1c546e77007afa0a9eeb3a44f07da99d84c383eb`; the upstream extraction reference remains `9307d7e2b1799750ecef1957d85128fcbe403d02`.

## 2026-11-17 E1-W04 condition semantics

Alpha 11 adds immutable condition-definition records inside Core-owned actor memory. Facts remain evidence with separate IDs; condition evaluation returns `TRUE`, `FALSE`, `UNKNOWN` or `CONFLICT`, and bounded AND/OR/NOT expressions use four-valued logic. Conditions bind a subject, exact scope, optional half-open UTC validity interval and explicit condition dependencies. A dependent query proceeds only when every referenced condition is `TRUE`; false conditions reject, while unknown or conflicting conditions suspend before content adoption. Existing `condition_refs` and memory snapshots remain readable; alpha 10 and earlier stores require explicit metadata migration, and alpha 10 memory snapshots deserialize with no definitions so their references evaluate `UNKNOWN`. K1 evaluates, K2 infers and K3 adopts; no extra decision component was added. `cargo check --workspace --all-targets --locked` and focused tests in `core_condition_scope_53.rs`, `core_condition_evaluation_57.rs`, the alpha 10 store-migration test and the domain canonical-compatibility test pass. This scoped implementation is not full CORE-G01 acceptance; `CORE-G01` and `CORE-G03` remain `PARTIAL` pending the construction gate's complete success predicates.

### E1-W05 conditional counterfactual connection

The existing `Engine::evaluate_counterfactual_relation` now runs the same Core relation query used outside scenarios, so target conditions are evaluated before inference and an explicitly allowed scenario hypothesis may satisfy a Fact predicate. `CounterfactualRelationReport` retains the separate condition status, definition/evidence dependencies, and dependent proof IDs. `FALSE`, `UNKNOWN`, and `CONFLICT` conditions return an unknown scenario result with no target facts; hypotheses in a scope different from the target are rejected before scenario evaluation, and temporary invalidation/hypothesis projections are not saved. Core relation stops now retain the IDs actually evaluated, including condition dependencies when premises are missing and opposing Fact IDs on conflict. A direct active premise that matches every typed field except exact scope now produces `SUSPEND / OUT_OF_SCOPE` with its ID; no applicable premise and no such direct match remains `SUSPEND / MISSING_PREMISES`. The memory scan budgets both condition and out-of-scope checks. `core_condition_evaluation_57.rs` covers condition-true inference, all three stopping states, proof dependencies, unchanged actor memory, transitive condition invalidation through ordinary Core, a condition-gated two-premise join with exact-scope success/scope-disjoint missing-premise/conflict boundaries, and pre-mutation rejection of cross-scope counterfactual assumptions. `core_scope_classification_58.rs` distinguishes a matching direct Fact outside the requested scope from a relation with no such evidence. The updated `core_construction_observation_02.rs` regression passes with the conflict result retaining its positive/negative proof IDs; `core_conjunctive_proof_55.rs` also passes. This is a bounded set of direct typed routes; the full W05 operator set and E2 `CORE-G02` predicate are not established, so `CORE-G02` remains `PARTIAL`.

### E1-W06 condition-aware Core adoption boundary

The existing `ResultRecord` is the K3 producer for this bounded typed relation path: a supported direct or derived proof carries its existing guarantee and dependencies, while a false applicability condition yields `REJECT` and unknown/conflicting conditions or relation evidence yield `SUSPEND` with the specific reason and evaluated record IDs. An exact direct premise available only in another scope yields the distinct `OUT_OF_SCOPE` reason, with `Guarantee::Unknown` and the premise ID retained. No second adoption authority or writer was added. `core_condition_evaluation_57.rs` checks `FALSE / UNKNOWN / CONFLICT` condition decisions, guarantees, dependencies and absence of a ContentPlan/render; `core_scope_classification_58.rs` checks bounded `OUT_OF_SCOPE` versus `MISSING_PREMISES`; the B55/B56 regressions check derived, missing and contradictory relation outcomes. This does not establish the full W06 status/guarantee mapping or the complete CORE-G02 predicate; `CORE-G02` remains `PARTIAL`.

### E1-W07 typed relation meaning plan

W07 established `CoreRelationMeaningPlan` for the direct typed `AskWhy` path. The plan retains the verified propositions, condition evaluations, proof Fact/Rule records, exact dependencies and a scope-limited disclosure as typed units with no completed sentence. The initial adapter/composer implementation and v4 artifact are historical; v1–v4 readers remain for saved results.

### 2026-11-18 E1-W08 bounded grammar generation with W09 adoption guard

The direct typed `AskWhy` path now maps the sentence-free plan to Core-authored literal/choice grammar fragments. The non-neural character model fits individual typed fragments and local lexical alternatives, then computes exact conditional distributions restricted to the active grammar state. For each local choice it selects the complete alternative with the highest exact rational sequence probability, breaking ties by the lexicographically smallest alternative; each selected character is scored and recorded on its finite grammar path. It preserves required unit order/dependencies, emits explicit EOS, records per-token support/distribution/context and model revision/artifact reference, and checks unit spans, semantic digests, proof IDs and lossless UTF-8 reconstruction. Before adoption, a K7 boundary rebuilds the typed plan from the live relation proof and checks the asked/concluded propositions, condition state, guarantee and proof dependencies against the generated report. Immediately before saving `ContentPlan`, it checks the exact required unit strings, result plus computation dependencies, model references, serialized emission and rendered text. A mismatch suspends before content adoption. The generated report is stored in computation artifact v5; prior artifacts v1–v4 retain their readers. The normal Core result path then creates the `ContentPlan` and K7 render from the same generated bytes.

Runtime execution and replay have now been exercised. `cargo test -p minidora-engine --locked` passed 254 unit tests, 37 integration cases across 33 harnesses, and 0 doc tests. This includes typed one-rule and multi-hop explanations through K7, saved comparison replay/recovery, and the B04/B08/B09 regression routes. Formatting check, workspace all-target check, warnings-denied library Clippy and `git diff --check` pass. This remains one bounded typed relation explanation route with local K7 checks. It does not establish ordinary natural-language coverage, general surface quality, every W09 adoption predicate, the E1 two-cycle exit or `CORE-G03`; `CORE-G03` remains `PARTIAL`.

### E1-W10 immutable actor-memory history (bounded Store substep)

`Store::save_memory` now checks the latest actor-memory snapshot before publishing a changed revision. Existing Fact, Rule and Condition records must remain in their original order with the exact prior payload; only `valid=true` to `valid=false` tombstoning is allowed. New records append, and changed revisions must advance by exactly one. The transaction rechecks the observed head before inserting the new immutable snapshot. Validation failures occur before blob publication.

`store::tests::memory_revision_preserves_immutable_records_and_allows_tombstones` covers append plus invalidation, Store reopen, unchanged original snapshot bytes, and rejection of truncation, payload rewrite and reactivation without an extra revision or orphan blob. At base `c8a2a90`, the full engine package suite and the formatter/workspace-check/library-Clippy commands above passed.

### E1-W10 Goal-related relation projection (bounded Ask/AskWhy slice)

The current actor-memory relation index is a rebuildable Store read model. `save_memory` updates appended Fact/Rule/Condition rows and tombstones in the same SQLite transaction as the immutable memory revision; an index snapshot is usable only when its revision equals the actor head. Ask/AskWhy select the query's direct records, recursively relevant rule premises, exact record dependencies, condition definitions and condition-evidence Facts. Proof evaluation and adoption remain in the existing Core K1/K2/K3 path. Projection reads and proof expansion share the request action budget. Scope-miss diagnostic Facts are selected only after an `UNKNOWN` proof. A cold pre-index store rebuilds the derived index once from the loaded current snapshot.

`core::active_relation_projection_tests::indexed_ask_projection_keeps_only_the_goal_and_condition_closure` uses 68 Facts, 2 Rules and 1 Condition, confirms that a condition-gated relation proof uses 2 Facts, 1 Rule and 1 Condition with the same proof dependencies as the full-snapshot reference, executes a normal Japanese `Engine::execute` Ask, checks a supported query stays within a four-action budget despite 24 out-of-scope Facts, forces cold-index rebuild, and verifies an incremental premise tombstone changes the next query to `UNKNOWN`. The final `cargo test -p minidora-engine --locked --quiet` run passed 266 unit tests, 37 Cargo-launched integration tests across 33 harnesses, and 0 doc tests. Workspace all-target check, formatting, engine-library warnings-denied Clippy and `git diff --check` passed on Linux/Rust 1.99.1. All-target Clippy remains blocked by `clippy::too_many_arguments` in the unchanged `_45` and `_46` comparison integration helpers.

This closes only the ordinary typed Ask/AskWhy relation subgraph selection slice. Cold-index rebuild still reads the full current snapshot once; other Core consumers, Recovery, relation-profile/list inventories, cache/summary/model-state versioning, broad fresh-process/session/model-version consumption and large-history resource populations remain unverified. W10 and `CAP-D02` remain `PARTIAL`/`UNVERIFIED`; no E1 exit or release is claimed. See [B57](construction/MB-BUILD-20261115/BUILD_BATCH_57.md), [runtime log](construction/MB-BUILD-20261115/b57-engine-test.log) and [machine-readable observation](construction/MB-BUILD-20261115/b57-build-observation.json).

### E1-W10 Goal-related relation projection (bounded AskRelationObjects extension)

The ordinary Japanese `AskRelationObjects` path now uses the same revision-checked Store relation index as typed `Ask` and `AskWhy`. Core selects the query's direct relation records, applicable Rules, premise closure and record dependencies, then runs the existing relation-object candidate/proof evaluator only over that projection. Projection and candidate/proof work share the sealed action budget. A missing legacy index is rebuilt once from the authenticated current snapshot; that cold-start read still visits the full snapshot. Core records the exact actor-memory revision used by both committed answers and budget stops.

`core::active_relation_projection_tests::indexed_relation_object_query_uses_only_the_goal_and_rule_closure` seeds 64 Facts and one Rule, including 60 unrelated Facts, and compares a normal Japanese `Engine::execute` list query against the full-snapshot reference. The projected query returns the same text, guarantee and Fact/Rule dependencies under budget 20, where the previous full scan exceeds the same budget. At budget 4 the relation projection suspends as `RESOURCE_LIMIT` while retaining actor-memory revision 1. The test forces cold-index rebuild, confirms the projection has only the two relevant Facts and one Rule, and tombstones the proof closure to verify that the next query suspends on actor-memory revision 2 without stale answer/dependency reuse. The final engine-package run passed 267 unit tests, 37 integration cases across 33 harnesses and 0 doc tests. Formatting, workspace all-target check, warnings-denied engine-library Clippy and `git diff --check` pass. All-target Clippy remains blocked by unchanged `too_many_arguments` helpers in comparison tests `_45` and `_46`.

This closes only bounded Goal-related selection for the ordinary `AskRelationObjects` query. Relation-list Recovery, other Core consumers, derived summaries/caches/model adaptations, broad fresh-process/session/model-version reuse and large-history populations remain unverified; cold-index rebuild still reads the full current snapshot once. W10 remains `PARTIAL`, `CAP-D02` remains `UNVERIFIED`, and no E1 exit or release is claimed. See [B58](construction/MB-BUILD-20261117-D2-LLM/BUILD_BATCH_58.md) and its [machine-readable observation](construction/MB-BUILD-20261117-D2-LLM/b58-build-observation.json).

### E1-W10 Goal-related projection for AskPreviousRelation (B59)

The ordinary `AskPreviousRelation` route now resolves its pronoun only against Fact records referenced by the immediately preceding committed result, loading those record IDs from one revision-matched relation-index transaction. After resolving the subject and scope, Core projects the exact relation, Rule premise closure, condition evidence and transitive proof dependencies from that same index snapshot, then runs the existing proof evaluator. Prior-dependency lookups, projection and proof steps share the request's finite action budget. Budget stops and committed answers retain the actor-memory revision; cold-index rebuilding still loads the authenticated current snapshot once.

`indexed_previous_relation_resolves_only_prior_dependencies_before_projection` compares proof status and exact dependencies to a full-snapshot reference with 72 unrelated Facts, while the selected query memory contains only its one supporting Fact and one Rule. The ordinary Japanese recovery regression adds 72 unrelated Facts after the prior same-case answer, commits the pronoun query at actor-memory revision 2, reopens the exact snapshot, and still rejects recovery after premise invalidation. The final engine-package run passed 268 unit tests, 37 integration cases across 33 harnesses and 0 doc tests. Focused Core and Recovery tests, formatting, workspace all-target check, engine-library warnings-denied Clippy and `git diff --check` passed on Rust 1.99.1. All-target Clippy remains blocked by the existing `too_many_arguments` lint in unchanged comparison helpers `_45` and `_46`.

This closes only Goal-related selection for ordinary `AskPreviousRelation`. Previous-result explanation, other learning consumers, derived summary/cache/model-adaptation versioning, fresh-process/session/model-version reuse, complete finite-subgraph selection and large-history resource populations remain unverified. Cold-index rebuild still reads the full current snapshot once. W10 remains `PARTIAL`, `CAP-D02` remains `UNVERIFIED`, and no E1 exit or release is claimed. See [B59](construction/MB-BUILD-20261117-D2-LLM/BUILD_BATCH_59.md) and its [machine-readable observation](construction/MB-BUILD-20261117-D2-LLM/b59-build-observation.json).

Store now exposes exact retained memory revisions. The ordinary Core `Ask`, `AskWhy` and `AskRelationObjects` routes persist the actor-memory revision they consumed in `ResultRecord`; the optional field is omitted for older results and defaults to absent when reading them. Recovery reproduces the result against that exact immutable snapshot, then separately checks the current memory head before confirming any pending file effect. `object_answer_recovery_rechecks_proof_before_confirming_saved_bytes` verifies the recorded revision survives Engine reopen and that a later Fact invalidation still prevents recovery from confirming stale bytes. This is a bounded Core/recovery consumer integration, not W10 completion.

The pinned Rust 1.99.1 run at base `c8a2a90` passed `cargo fmt --all -- --check`, `cargo check -p minidora-engine --all-targets --locked`, `cargo clippy -p minidora-engine --lib --locked -- -D warnings`, the full `cargo test -p minidora-engine --locked` suite, and the legacy `ResultRecord` deserialization regression. All-target Clippy at that base reports `clippy::too_many_arguments` in unchanged helpers in `core_atomic_relation_comparison_45.rs` and `_46.rs`.

Core `AskConjunction` now records the exact actor-memory revision. Recovery checks the conjunction's clause results, proof dependencies, guarantee and required content units against that immutable revision, then repeats the check against current memory before confirming pending bytes. `conjunction_answer_recovery_pins_and_rechecks_its_memory_revision` verifies both Facts survive Engine reopen and that invalidating a premise prevents recovery from confirming the old result. On the pinned Rust 1.99.1 toolchain, `cargo fmt --all -- --check`, `cargo check --workspace --all-targets --locked`, `cargo clippy -p minidora-engine --lib --locked -- -D warnings`, that focused recovery test and `core_conjunctive_proof_55` all pass for this uncommitted change. This closes only the `AskConjunction` consumer path.

Core `InspectPremises` now records the exact actor-memory revision behind its bounded relation inventory. Recovery recomputes the inventory's dependency IDs and guarantee from that retained snapshot, then compares the same inventory with current actor memory before confirming any pending file effect. New revision-bearing records use the same derived-inventory replay as Core; legacy records keep their version-specific recovery path. `premise_inventory_recovery_pins_and_rechecks_its_memory_revision` verifies direct Fact and Rule dependencies, Engine reopen, and refusal to confirm after Rule invalidation. On the pinned Rust 1.99.1 toolchain, the focused test, `cargo fmt --all -- --check`, `cargo check --workspace --all-targets --locked`, and warnings-denied library Clippy pass for this change. This closes only the `InspectPremises` consumer path.

Core `AskCommonRelations` now records the exact actor-memory revision used by the ordinary Japanese common-point route. Core and Recovery share one bounded computation for the report text, canonical proof artifact, proof dependencies and guarantee. Recovery rebuilds them from the retained snapshot, checks the stored report artifact bytes, then repeats the same comparison against current actor memory before confirming pending file bytes. `common_relations_recovery_pins_and_rechecks_its_memory_revision` exercises the normal Japanese request with a derived shared relation, verifies the retained revision after Engine reopen, and confirms that invalidating a supporting Fact blocks recovery of the old result. On the pinned Rust 1.99.1 toolchain, that focused test, `cargo fmt --all -- --check`, `cargo check --workspace --all-targets --locked`, warnings-denied library Clippy, and `git diff --check` pass for this change. This paragraph records the earlier consumer/recovery state before B61: candidate generation then scanned retained Facts and Rules within the action budget. B61 replaces that ordinary-route scan with bounded indexed candidate selection, but does not establish every Goal-related subgraph, Recovery consumer or CAP-D02 acceptance; see the B61 observation below.

Core `DraftRelationProfile` now records the actor-memory revision used to build the ordinary Japanese saved-relation profile. Core and Recovery share `relation_profile_answer` for the bounded inventory, profile units, proof dependencies and HYPOTHESIS guarantee under the sealed action/evidence budgets. Recovery replays the exact retained revision to validate the adopted units and model-approved surfaces, then rebuilds the profile against current memory before confirming pending file bytes. Older results without a revision retain their prior recovery path. `relation_profile_recovery_pins_and_rechecks_its_memory_revision` verifies a normal Japanese profile with a derived relation, retained snapshot after Engine reopen, and refusal to confirm after invalidating a supporting Rule. The focused recovery test, the normal CLI follow-up test, `cargo fmt --all -- --check`, `cargo check --workspace --all-targets --locked`, warnings-denied engine library Clippy, and `git diff --check` pass on Rust 1.99.1. This closes only the `DraftRelationProfile` consumer path; subject inventory remains action-bounded and is not Goal-related subgraph selection or CAP-D02 acceptance.

Core `AskPreviousRelation` now records the exact actor-memory revision used to resolve the direct premise referenced by the immediately preceding same-case result. Core and Recovery share the same bounded premise resolver and the same supported/contradicted answer-unit construction. The route no longer labels this ordinary answer as a typed relation-meaning artifact without its required content plan, which previously produced `SUSPEND / CONTENT_PLAN_ARTIFACT_MISSING`. A temporary-state normal CLI replay now commits the seed, explicit relation query and prior-result pronoun query; the latter records actor-memory revision `1`. `previous_relation_recovery_pins_and_rechecks_its_memory_revision` verifies the normal answer and explanatory variant, exact replay after Engine reopen, and recovery rejection after invalidating the supporting premise while preserving the existing file bytes. The focused test, `cargo fmt --all -- --check`, `cargo check --workspace --all-targets --locked`, warnings-denied engine library Clippy and `git diff --check` pass on the pinned Rust 1.99.1 toolchain. This closes only the `AskPreviousRelation` consumer/recovery path; it does not establish Goal-related active-subgraph selection or CAP-D02 acceptance.

Core `CompareRelationInventories` now records the exact actor-memory revision used for both subject inventories. Core and Recovery share the bounded content-request/version selection; Recovery reconstructs both inventories, the canonical report artifact, typed composition, proof dependencies and adopted units from the retained snapshot. Before confirming pending file bytes it replays the comparison against current actor memory, and also requires a current-memory match for older results without a revision. Comparison Hypothesis content now passes the same Recovery content gate after its exact plan/render checks. `relation_comparison_recovery_pins_and_rechecks_its_memory_revision` exercises the normal Japanese request, Engine reopen and rejection after Fact invalidation while preserving the original bytes. The focused recovery test and existing artifact/replay harnesses `core_atomic_relation_comparison_38`, `_40`, `_41`, `_42` and `_43` pass. `cargo fmt --all -- --check`, `cargo check --workspace --all-targets --locked`, warnings-denied engine library Clippy and `git diff --check` pass on the pinned Rust 1.99.1 toolchain. This closes only the `CompareRelationInventories` consumer/recovery path; it does not establish Goal-related active-subgraph selection or CAP-D02 acceptance.

Core `ExplainPrevious`, when it explains a prior relation answer, now records the exact actor-memory revision used to rebuild the explanation. Recovery replays the prior relation meaning, proof, dependencies and exact required explanation units against that retained snapshot, then checks the same proof against current actor memory for every parser version before confirming pending file bytes. Successful result dependencies retain exactly the prior result followed by its proof dependencies. `previous_answer_explanation_pins_and_rechecks_its_memory_revision` exercises the normal Japanese follow-up, Engine reopen, and recovery rejection after Fact invalidation while preserving the existing file bytes. This focused test, `cargo fmt --all -- --check`, `cargo check --workspace --all-targets --locked`, warnings-denied engine library Clippy, and `git diff --check` pass on Rust 1.99.1. This closes only the prior-relation-answer `ExplainPrevious` path; explanations of saved comparisons and other prior-result contexts remain unverified, so W10 and CAP-D02 remain partial.

Core `ExplainPrevious` after a saved `CompareRelationInventories` result now records the actor-memory revision used to revalidate both inventories. Recovery validates the sealed prior comparison input/result identity, report artifact, stored ContentPlan/render bytes, proof dependencies and exact explanation units against the comparison's original snapshot, the explanation snapshot and current actor memory before confirming pending bytes. The `Hypothesis` guarantee is accepted at the common content gate only when the sealed prior result is this typed saved-comparison route and the specialized replay succeeds. `saved_relation_comparison_explanation_pins_and_rechecks_its_memory_revision` covers the normal Japanese compare/follow-up, Engine reopen, and recovery rejection after Fact invalidation while preserving the existing file bytes. That focused test, the saved-comparison recovery test, prior-relation explanation test, `core_atomic_relation_comparison_38`, formatting check, workspace all-target check, warnings-denied engine library Clippy and `git diff --check` pass on Rust 1.99.1. This closes only same-case saved-comparison explanation recovery; broader F04 and CAP-D11 session continuity and Goal-related subgraph selection remain unproven, and W10/CAP-D02 remain partial.

Core `ExplainPrevious` after an `AskCommonRelations` result now records the actor-memory revision used to rebuild the detailed explanation. Core and Recovery share the same bounded proof replay, typed subject/scope check, dependency list, guarantee and output-budget validation. Recovery checks the sealed prior request, report artifact, stored plan/render, original report revision when pinned, explanation snapshot and current actor memory before confirming pending bytes. `common_relations_explanation_pins_and_rechecks_its_memory_revision` covers the Japanese common-point/follow-up, Engine reopen and rejection after Fact invalidation while preserving existing file bytes. The focused common-relations recovery and explanation tests, formatting, workspace all-target check, warnings-denied engine library Clippy and `git diff --check` pass on Rust 1.99.1. This closes only same-case `AskCommonRelations` explanation recovery; broader F04/CAP-D11 session continuity and Goal-related subgraph selection remain unproven, and W10/CAP-D02 remain partial.

Core `ExplainPrevious` after an `AskRelationObjects` result now records the actor-memory revision used to rebuild the detailed explanation. It recomputes the exact sealed object-list request and retains each supported relation outcome so Core and Recovery share the same direct/derived proof lines, dependencies, guarantee and evidence budget. Recovery validates the prior typed input, content/render artifacts, its original snapshot when pinned, the explanation snapshot and current actor memory before confirming pending bytes. `relation_objects_explanation_pins_and_rechecks_its_memory_revision` covers the Japanese list/follow-up, Engine reopen and rejection after Fact invalidation while preserving existing file bytes; the existing normal CLI transitive-object route also passes. The focused tests, formatting, workspace all-target check, warnings-denied engine library Clippy and `git diff --check` pass on Rust 1.99.1. This closes only same-case `AskRelationObjects` explanation recovery; general paraphrase/examples, broader F04 acceptance, cross-session continuity and Goal-related subgraph selection remain unproven, and W10/CAP-D02 remain partial.

Core `ExplainPrevious` after `DraftRelationProfile` resolves the prior result through its sealed same-actor/case input, replays its adopted units and exact proof dependencies against the original actor-memory revision, then compares the profile against the explanation revision and current memory before confirming pending file bytes. Missing memory revisions, changed profile evidence and exhausted budgets suspend. The explanation preserves the prior `HYPOTHESIS` guarantee and links the prior result plus its proof dependencies. `natural_relation_profile_explanation_replays_the_prior_result_in_the_cli` verifies the normal Japanese follow-up in separate CLI processes. `relation_profile_explanation_pins_and_rechecks_its_memory_revision` verifies distinct profile/explanation file outputs, Engine reopen, exact retained-revision recovery and refusal after Rule invalidation while preserving the explanation bytes. Both focused runtime tests, formatting, workspace all-target locked check, warnings-denied engine library Clippy and `git diff --check` pass on Rust 1.99.1. This closes only same-case saved-profile explanation wiring. Broad A14/F04 acceptance, cross-session continuity and Goal-related subgraph selection remain unproven, and W10/CAP-D02 remain partial.

Core `AskConjunction` now evaluates each ordinary Japanese relation clause through the existing revision-matched Store relation index inside one transaction. The request action allowance is divided among clauses and shared within each clause by projection, proof evaluation and out-of-scope diagnosis. The `indexed_conjunction_projects_each_clause_within_the_shared_budget` regression compares a three-clause Japanese query with the full-snapshot reference using 76 Facts, one Rule and 72 unrelated Facts; projected proof state, guarantee and dependency IDs match. It also verifies an `OUT_OF_SCOPE` stop retaining the archive Fact and first-clause dependencies, and a `RESOURCE_LIMIT` stop retaining actor-memory revision 1. `conjunction_answer_recovery_pins_and_rechecks_its_memory_revision` verifies exact-revision replay after Engine reopen and rejection after Fact invalidation; existing `core_conjunctive_proof_55` passes. The final engine suite passed 269 unit tests, 37 integration cases across 33 harnesses and 0 doc tests; formatting, workspace all-target check, warnings-denied library Clippy and `git diff --check` pass on Rust 1.99.1. All-target Clippy remains blocked by unchanged `too_many_arguments` in comparison test helpers `_45` and `_46`. This closes only the ordinary `AskConjunction` projection slice; cold-index rebuild still reads the full snapshot once, and W10/CAP-D02 remain partial/unverified. See [B60](construction/MB-BUILD-20261117-D2-LLM/BUILD_BATCH_60.md) and its [machine-readable observation](construction/MB-BUILD-20261117-D2-LLM/b60-build-observation.json).

Core `AskCommonRelations` now selects exact-scope subject Facts and Rules, plus variable-subject Rules, through a Store index on `(actor_id, valid, scope, subject, record_kind)`. The selected Rule premise shapes and Fact/Rule conclusion shapes seed the existing per-subject relation projection; both proof evaluations run inside the same revision-matched index transaction. Candidate selection, projection and left/right proof queries consume the request's finite action budget. Core retains report construction and adoption, and the existing full-snapshot computation remains the Recovery replay path.

`indexed_common_relations_use_bounded_subject_candidates_and_preserve_artifacts` stores 82 Facts, one Rule and 80 unrelated Facts. The selector returns only the two named-subject Facts and the applicable variable-subject Rule; the Japanese `[scope=lab]NeraとMikoの共通点は何ですか` route commits at 64 actions without unrelated dependencies. Report text, canonical artifact bytes, proof dependencies and guarantee match the full-snapshot reference. The prior full-table route returns `RESOURCE_LIMIT` at the same budget, while a three-action request reaches indexed selection, suspends as `RESOURCE_LIMIT`, and retains actor-memory revision 1. The existing `common_relations_recovery_pins_and_rechecks_its_memory_revision` and normal CLI common-relation test pass.

The final engine package run passed 270 unit tests, 37 integration cases across 33 harnesses and 0 doc tests. The focused Core and Recovery tests, normal CLI regression, formatting, workspace all-target check, warnings-denied engine-library Clippy and `git diff --check` pass on Rust 1.99.1. All-target Clippy remains blocked by unchanged `too_many_arguments` in comparison test helpers `_45` and `_46`. This closes only the ordinary `AskCommonRelations` candidate-selection slice; cold-index rebuild and concurrent-head fallback retain full-snapshot costs, and Recovery continues to replay the exact retained snapshot. Other Core/learning consumers, separately versioned derived indexes/caches/summaries/model adaptations, complete finite-subgraph selection and broad fresh-process/session/model-version consumption remain unverified. W10 remains `PARTIAL`, CAP-D02 remains `UNVERIFIED`, and no E1 exit or release is claimed. See [B61](construction/MB-BUILD-20261117-D2-LLM/BUILD_BATCH_61.md) and its [machine-readable observation](construction/MB-BUILD-20261117-D2-LLM/b61-build-observation.json).

Core `DraftRelationProfile` now selects active Facts for the requested subject across scopes, plus exact-subject and variable-subject Rules, through a revision-matched Store subject index. Those seeds produce the existing finite relation shapes; each shape then uses the existing active relation projection and Core proof evaluator. Candidate selection, shape planning, projection and proof evaluation share the sealed action budget. The existing profile-unit construction, model-approved surfaces, `HYPOTHESIS` guarantee and Core adoption remain unchanged. A stopped profile retains the actor-memory revision observed by its indexed transaction.

`indexed_relation_profile_uses_bounded_subject_candidates_and_preserves_inventory` uses 84 Facts, one Rule and 80 unrelated Facts. It selects four subject Facts and one variable-subject Rule, generating five relation shapes across the `lab` and `archive` scopes. At 64 actions the ordinary Japanese profile commits with the same units, conflicts, proof dependencies and guarantee as the full-snapshot reference; the former full scan reaches `RESOURCE_LIMIT`. A cold index is deleted and rebuilt on the normal route. An eight-action request stops as `RESOURCE_LIMIT` and retains actor-memory revision 1. Existing profile Recovery tests replay the exact retained revision and refuse confirmation after invalidation; the ordinary CLI profile and its previous-result consumer pass.

The final engine suite passed 271 unit tests, 37 integration cases across 33 harnesses and 0 doc tests. The focused profile test, five filtered profile/recovery tests, normal CLI profile route, formatting, workspace all-target check/build, warnings-denied engine-library Clippy and `git diff --check` pass on Rust 1.99.1. Workspace all-target Clippy remains blocked by one unchanged `too_many_arguments` warning in `core_atomic_relation_comparison_46.rs`. This closes only the ordinary `DraftRelationProfile` bounded inventory slice. Recovery still replays the exact full snapshot; cold-index rebuild and concurrent-head fallback still load the full current snapshot. W10 remains `PARTIAL`, CAP-D02 remains `UNVERIFIED`, and no E1 exit or release is claimed. See [B62](construction/MB-BUILD-20261117-D2-LLM/BUILD_BATCH_62.md) and its [machine-readable observation](construction/MB-BUILD-20261117-D2-LLM/b62-build-observation.json).

Core `InspectPremises` now obtains its ordinary Japanese subject inventory through one revision-matched Store relation-index transaction, reusing bounded subject Facts, exact/variable-subject Rules, the existing proof projection, and the Core inventory evaluator. The ordinary Core route retains its previous conflict rendering, disclosure, dependencies, guarantee, and adoption behavior. A cold/stale index is rebuilt once; a concurrent head advance falls back to the already loaded immutable snapshot. The focused fixture selects four subject Facts and one variable-subject Rule from 84 Facts, one Rule, and 80 unrelated Facts. The indexed inventory matches a 10,000-action full-snapshot reference for facts, conflicts, dependencies, and guarantee; normal Japanese `Engine::execute` commits at 64 actions while the full scan reaches `RESOURCE_LIMIT`. A cold-index retry and an eight-action stop retaining revision 1 pass. `premise_inventory_recovery_pins_and_rechecks_its_memory_revision` still passes; Recovery remains exact full-snapshot replay. The new focused Core and Recovery tests, formatting, engine all-target check, engine library warnings-denied Clippy, and `git diff --check` pass on Rust 1.99.1. The full engine package and workspace all-target Clippy were not rerun. W10 and CAP-D02 remain partial/unverified; this does not claim E1 exit. See [B63](construction/MB-BUILD-20261117-D2-LLM/BUILD_BATCH_63.md) and its [machine-readable observation](construction/MB-BUILD-20261117-D2-LLM/b63-build-observation.json).

`CompareRelationInventories` now selects active Fact/Rule candidates from the exact requested scope through the revision-matched `memory_index_scope`. Core filters mixed-scope Rules and builds both inventories, condition/proof closures and v3/v4 supporting Rule payloads from one index revision; direct typed preflight and ordinary same-case comparison revalidation use the same path. The fixture selects four `lab` Facts from a history with 80 off-scope Facts: inventories equal the 10,000-action full-snapshot references, while the former full-snapshot path reaches `RELATION_BUDGET` at 64 actions and the normal cold-index Core comparison commits at total budget 128. Existing v3/v4 replay, invalidation and actor-isolation regressions pass. Recovery comparison verification still replays the full snapshot; same-scope scale, concurrent writers and broad CAP-D02 acceptance remain unverified. Focused projection tests (8), comparison/replay regressions (3), formatting, engine all-target check, engine library warnings-denied Clippy and `git diff --check` pass on Linux/Rust 1.99.1. Full engine-package tests and workspace all-target Clippy were not run. W10 stays `PARTIAL`; CAP-D02 stays `UNVERIFIED`. See [B64](construction/MB-BUILD-20261117-D2-LLM/BUILD_BATCH_64.md) and its [machine-readable observation](construction/MB-BUILD-20261117-D2-LLM/b64-build-observation.json).

The pending-file Recovery check for `CompareRelationInventories` now revalidates current actor memory through the revision-matched scope index while keeping exact retained-revision replay unchanged. The shared verifier still checks artifact identity, both inventories, content composition, dependencies and guarantee. A comparison with two supported Facts and an action budget of 32 passes current-head revalidation after 80 unrelated Facts are appended across other scopes, while the full-snapshot current-head reference stops with `RELATION_BUDGET`. The test also covers the cold-index full-snapshot fallback, exact retained-revision replay, Engine reopen and refusal after premise invalidation without changing the existing target bytes. Five filtered library tests, three comparison regression harnesses, formatting, engine all-target check, warnings-denied engine-library Clippy and `git diff --check` pass on Rust 1.99.1. Exact retained-revision Recovery replay still materializes its historical snapshot; large-history costs for cold/stale index fallback, same-scope scale, concurrent writers, full engine-package tests, workspace all-target Clippy and broad CAP-D02 acceptance remain unverified. W10 stays `PARTIAL`; CAP-D02 stays `UNVERIFIED`. See [B65](construction/MB-BUILD-20261117-D2-LLM/BUILD_BATCH_65.md) and its [machine-readable observation](construction/MB-BUILD-20261117-D2-LLM/b65-build-observation.json).

W10 remains `PARTIAL`: bounded Goal-related projection has been observed for ordinary typed `Ask` and `AskWhy`, Japanese `AskRelationObjects`, `AskConjunction`, `AskCommonRelations`, `DraftRelationProfile`, `InspectPremises`, ordinary `AskPreviousRelation`, `CompareRelationInventories`, and the pending-file comparison Recovery current-head recheck when the index matches the current revision. Exact retained-revision Recovery replay and cold/stale fallback remain full-snapshot paths. Other relation and learning consumers, separately versioned derived indexes/caches/summaries/model adaptations, complete finite-subgraph selection, and broad fresh-process/session/model-version consumption by K2/K3/K4 remain unproven. No W10 or E1 exit is claimed; `CORE-G01` and `CORE-G03` remain `PARTIAL`.

**Full requested successor acceptance: not established.** This is an implementation inventory for ongoing work, not permission to exclude requirements, a release acceptance certificate, a deployment claim or independent third-party audit. The [93-row baseline requirements](requirements.tsv) remain baseline observations; actual successor evidence must be recorded separately. The [full implementation obligations](implementation-obligations.md) remain controlling for coverage.

## 2026-11-15 現行施工へのゲート再判定（履歴は保持）

- 現行施工3点セット＝[`MB-BUILD-20261115`／日本語基底R4](construction/MB-BUILD-20261115/README.md)。上位論理正本＝R2。統治＝ルート`AGENTS.md`。施工ゲート`CORE-G01`〜`CORE-G07`と既存R2要求`C01`〜`C13`は**別の識別子と別の受入対象**。B01〜B30の当時のコード・試験・判断・SHAは消去・改竄しない。以下は**今回の施工工程移行の判定**であって過去試験の結果変更ではない。
- B12の`Engine::compose_typed_meaning`は既成の意味単位文面の順序・接続句を選ぶ限定構成であり、**初見原子的意味→新内容計画→新文面→通常中核K7採用**を示さない。`CORE-G03=PARTIAL`（2026-11-14基線HEAD `47106eca`の証拠による暫定）。
- B14時点ではR2が認める**証明保存型合成**について、規則合成・別主体への適用・反例後の失効は局所観測された一方、同じ直接中核入口の学習前後比較、領域外例、後続矛盾の証拠が揃わず、`CORE-G05=PARTIAL`だった。B49で不足していた直接Engine前後比較と短縮proof経路を再観測し、現在はこの限定した証明保存型合成経路について`CORE-G05=PASS`と評価する。観測起点の仮説形成は未実施であり、CAP-D05の通常利用者経路と全受入は別途未確認。二経路同時成立をCORE-G05の条件へ追加しない。
- B16の「七つの直接観測を完了」は**CORE-G01〜CORE-G07の成立述語全件PASSではない**。他五ゲートの全述語のPASSも現時点で断定しない。B17〜B30の入力文法・原文span・履歴・テストは実績として保存するが、工程移行の根拠にはしない。
- B31/B32では型付き原子命題から既存中核を通してContentPlan採用・K7実描画へ届く一つの実行例と、根拠不足・未許可書込の停止を観測した。証拠IDは入力・採用計画へ保持され、未許可パスにはファイルも作用記録も作られない。これは限定した一命題・一説明目的のスモークであり、初見意味全域・説明妥当性・K4/K7全受入述語を閉じないため、`CORE-G03=PARTIAL`を維持する。詳細は[B32](construction/MB-BUILD-20261115/BUILD_BATCH_32.md)。
- B33で原入力、証拠/plan/result ID、採用単位、実文面、RenderObservation、停止値を機械可読記録へ保存した。実文面の結論は直接提示された前提と同一であり、このケースから新しい説明内容の妥当性・網羅性は確認できない。`CORE-G03=PARTIAL`を維持する。詳細は[B33](construction/MB-BUILD-20261115/BUILD_BATCH_33.md)。
- B34では直接前提として存在しない導出結論に対し、2つの前提・適用規則・派生FactのIDをContentPlanと実描画まで保持した。出力にはその証明線と現実の真偽未確認が含まれる。これは一つの宣言規則fixtureに限られるため、一般的な新規説明/内容品質を確立せず、`CORE-G03=PARTIAL`を維持する。詳細は[B34](construction/MB-BUILD-20261115/BUILD_BATCH_34.md)。
- B35ではB34と同じ隔離ケースからactor/case認証付きで実際の`SequenceModel`と`ModelEmission`を読み戻した。Core許可済みの二候補を保存模型で厳密採点し、選択文・系列確率・ContentPlan・K7描画の一致と同条件の決定的再生を確認。候補差は接続文のみで、全根拠/説明単位は両候補に保持される。これは有限な表層候補の選択観測であり、初見意味から事実内容を作ることや一般的文章品質を確立しないため、`CORE-G03=PARTIAL`を維持する。詳細は[B35](construction/MB-BUILD-20261115/BUILD_BATCH_35.md)。
- B36では完成文を渡さず、二対象・scopeの型付き比較入力からCoreがactor memoryの左右別proof IDを封印し、比較実行時に再照合するAPIを追加した。共通関係・各側のみの関係・根拠4件を含むレポートartifactからContentPlanを採用し、K7描画bytesとの一致を観測。根拠不足は描画前に停止し、未許可書込ではファイル・receipt・action recordを作らない。構造化比較の一fixtureであり、一般的な初見文章品質や保存記録の網羅性を確立しないため、`CORE-G03=PARTIAL`を維持する。詳細は[B36](construction/MB-BUILD-20261115/BUILD_BATCH_36.md)。
- B37では型付き原子関係と実proofからCoreがClaim/Evidence/Rule/Limitationの内容unit・依存順序・文面を組み立て、入力時に完成説明文がないまま通常ContentPlan/K7へ接続した。version 2計算artifactにrequest/reportを保持し、従来version 1 readerを維持した。unit順・全proof ID・同条件再生、K7 render bytes一致を確認。直接前提と導出前提の各fixture、根拠不足停止、未許可書込（ファイル/receipt/actionなし）を限定試験した。狭い日本語説明経路のみで、複数段導出母集団、一般的内容妥当性、通常CLI/HTTP経路、R2-17比較や全G03成功述語は未成立のため、`CORE-G03=PARTIAL`を維持する。詳細は[B37](construction/MB-BUILD-20261115/BUILD_BATCH_37.md)。
- B38では二対象・scopeの型付き入力と左右別inventoryから、CoreがSubjectA/SubjectB/Commonality/Difference/Evidence/LimitationのCompare単位を生成し、`MeaningCompositionPurpose::Compare`→ContentPlan→K7へ接続した。全左右fact/proof ID、同一条件の再生要求/結果をversion 2比較artifactに保持し、追問時に現在memoryのinventory再計算・request再構成・構成再生・描画記録を照合した。旧version 1比較artifactも別経路で再読し追問を確認。B36の根拠不足停止・未許可書込停止とB31/B34説明経路の回帰smokeはPASS。型付きfixtureと小さいinventoryの範囲に限られ、16 unitのcomposer上限、一般内容妥当性、通常日本語比較の全域、CORE-G03の成功述語全体は未成立のため`CORE-G03=PARTIAL`を維持する。詳細は[B38](construction/MB-BUILD-20261115/BUILD_BATCH_38.md)。
- B39のコード変更では、左右inventory proofに含まれる保存規則を`Evidence`内容単位に写すversion 3比較artifact経路を追加した。規則の前提・追加前提・結論・範囲・規則IDをContentPlan候補へ組み、fact・規則dependency IDを保持する。従来version 1/2 artifactは各旧形式の再計算・再読経路へ分岐する。formatとengine全targetの型検査はPASSしたが、新しい規則比較・再起動後version 3追問の実行試験は`NOT_RUN`であり、このbatchのproducer→K7実値到達は未確認。`CORE-G03=PARTIAL`を維持する。詳細は[B39](construction/MB-BUILD-20261115/BUILD_BATCH_39.md)。
- B40で一つの宣言規則から左右の導出RelationInventoryを生成し、V3比較artifactの規則Evidence・全proof ID・ContentPlan・K7描画を実行観測した。Engine再起動後の追問は現在のinventory/rule/request/compositionを再生成して`COMMIT`し、元比較IDへの依存とrenderを保持。B38のV2直接比較とV1旧report追問も回帰PASS。二規則fixtureでは既定budgetで`RESOURCE_LIMIT`、budget 256では旧report surfaceへのfallbackを観測したが、artifact kindと厳密なreject上限は採取しておらず推定を分けて記録する。単一規則の実行例であり、多段proof、全R4成功述語、一般内容妥当性を示さないため`CORE-G03=PARTIAL`を維持する。詳細は[B40](construction/MB-BUILD-20261115/BUILD_BATCH_40.md)。
- B41では二段規則fixtureでV3の長いfact依存文が384-byte/unit上限を超える境界を再現し、compactな依存件数表示と型付き全dependency IDを持つV4比較artifactへ接続した。fixtureには手動二規則に加えてCoreの合成規則があり、16 units・全9 proof ID・ContentPlan・K7描画を確認。再起動後、同じ256 action budgetでV4追問の再生成・照合が`COMMIT`し、元比較IDへの依存と`RENDERED`を保持。V4は一つの2-rule chain fixtureの結果であり、多段rule全域やR4成功述語全件を示さないため`CORE-G03=PARTIAL`を維持する。詳細は[B41](construction/MB-BUILD-20261115/BUILD_BATCH_41.md)。
- B42では単一規則と追加の直接関係を持つ比較fixtureで、inventoryから再構成した型付き候補がV2=17 unit、V3/V4=18 unitとなり16-unit上限を超える境界を採取した。最長V2 fact evidence本文は309 bytes。実際の保存artifactはV1 reportで、左右inventoryの全9 proof IDを保持し、K7描画とEngine再起動後の追問`COMMIT`・`RENDERED`を確認した。これは一つのunit-overflow fixtureであり、V1 surfaceの規則文表示、他のoverflow形状、全R4成功述語を確立しないため`CORE-G03=PARTIAL`を維持する。詳細は[B42](construction/MB-BUILD-20261115/BUILD_BATCH_42.md)。
- B43では同一actor・同一caseへV1 fixed report、V2 direct content、V3 rule-bearing、V4 compact-referenceのartifactを保存し、四つすべてのblobを同時点でhash再照合した。Engine再起動後に各保存比較IDを明示した`なぜ`追問を順次行い、それぞれのartifact版で`COMMIT`・ContentPlan・`RENDERED`を確認した。これはscope分離した一actor・一caseの局所fixtureであり、migration、並行writer、changed-proof後のreplayや全履歴共存を確立しないため`CORE-G03=PARTIAL`を維持する。詳細は[B43](construction/MB-BUILD-20261115/BUILD_BATCH_43.md)。
- B44では同一actor・同一caseに保存したV3 rule-bearing比較とV2 direct比較のうち、V3側scopeの直接前提だけを失効させた。Engine再起動後の明示V3追問は`SUSPEND / CONTEXT_PROOF_CHANGED`でContentPlan/描画前に停止し、別scopeのV2保存artifactは同一hashで再読され、V3停止後の明示V2追問も`COMMIT / RENDERED`した。これはV2/V3二版・一actor・一case・一つの直接fact失効の局所fixtureであり、V1/V4への同じ失効適用、rule失効、全scope、migration、並行writerは未確認。production codeやartifact契約は変えておらず、`CORE-G03=PARTIAL`を維持する。詳細は[B44](construction/MB-BUILD-20261115/BUILD_BATCH_44.md)。
- B45では同一actor・同一caseにV1 reportを二scope、V4 contentを二scope保存し、各版の対象scopeから直接factを一つずつ失効させた。Engine再起動後、明示V1/V4追問は両方`SUSPEND / CONTEXT_PROOF_CHANGED`でplan/render前に停止し、別scopeのV4/V1 control追問は両方`COMMIT / RENDERED`した。4 blobはすべて元hashと一致し、target停止後も明示result IDでcontrolを選べた。これはV1/V4二版・四比較scope中の二target scope失効・一actor・一caseの局所fixtureであり、rule失効、migration、並行writer、全scopeは未確認。production codeやartifact契約は変えておらず、`CORE-G03=PARTIAL`を維持する。詳細は[B45](construction/MB-BUILD-20261115/BUILD_BATCH_45.md)。
- B46では同一actor・同一caseの別scopeにV4 rule-bearing targetとV1 report controlを保存し、通常の`Engine::execute("invalidate <rule-id>")`経路でtarget root ruleを失効させた。root ruleとCore合成の依存ruleは記録を保持したまま無効となり、revision 11→12を確認。Engine再起動後の明示V4追問は`SUSPEND / CONTEXT_PROOF_CHANGED`でplan/render前に停止し、別scopeのV1 control追問は元result IDを選んで`COMMIT / RENDERED`した。両artifact hashは不変。これは一actor・一case・二scopeのrule失効fixtureに限り、通常CLI/HTTP、全rule形状、migration、並行writer、outside-world truthを示さない。`CAP-D13`全受入は`UNVERIFIED`、`CORE-G03=PARTIAL`を維持する。詳細は[B46](construction/MB-BUILD-20261115/BUILD_BATCH_46.md)。
- B47では同一scopeの同型rule chainとV4比較proofを別actor・別caseへ保存し、actor Aのroot ruleだけを通常Engine経路で失効させた。Engine再起動後、Aのmemory revisionは4→5となりAの保存追問は`SUSPEND / CONTEXT_PROOF_CHANGED`でplan/render前に停止。Bはrevision 4、root/second/composed rulesとartifact hashを保持し、自分のresult IDによる追問を`COMMIT / RENDERED`した。両actorのrule/proof ID集合は分離し、B memoryは再生後も不変。これは同一store内二actor・二caseの限定fixtureで、並行writer、case共有、migration、通常CLI/HTTP、outside-world truthやCAP-D13全受入は示さず、`CAP-D13=UNVERIFIED`と`CORE-G03=PARTIAL`を維持する。詳細は[B47](construction/MB-BUILD-20261115/BUILD_BATCH_47.md)。
- B48では同一scope・別actor/別caseで保存したrelation結果について、actor AがEngine再起動後にactor Bのresult IDを明示contextへ渡すと`FAIL / INPUT_FAILURE: CONTEXT_MISMATCH`となり、ContentPlan/renderもB側関係文面の露出もなかった。双方のMemoryState値は変わらず、Aの履歴には自分の先行resultが残り、A/B各自のresult IDを選んだ再生は`COMMIT / RENDERED`した。これはCore integrationの一fixtureで通常CLI/HTTP経路やCAP-D11全受入を示さず、`CAP-D11=UNVERIFIED`と`CORE-G03=PARTIAL`を維持する。詳細は[B48](construction/MB-BUILD-20261115/BUILD_BATCH_48.md)と[実行観測](construction/MB-BUILD-20261115/b48-build-observation.json)。
- B49では同じ直接`Engine::execute`入口から同じAster質問を学習前後で実行し、前は`SUSPEND / MISSING_PREMISES`、二つのscope付き親規則の証明保存型合成後はEngine再起動を挟んで`COMMIT / DERIVED / RENDERED`へ変化した。直接facts・両親rule・shortcutを全てResultRecord依存に保持し、同一queryのroot-only経路2 relation stepsに対しcomposed経路は1 stepで成立。未見subject Cyraは派生回答、`other-lab`のRookはplan前に停止。Asterの後続訂正は元factを失効させ、同じqueryを再び停止させる一方、過去のcommit記録とCyraの独立証明を保持。明示中間矛盾でshortcutを使わないunit guardとB14/B40/B41回帰もPASS。この有限な証明保存型合成fixtureにより`CORE-G05=PASS`（同経路限定）、ただし`CORE-G03=PARTIAL`と通常入口/全`CAP-D05`受入`UNVERIFIED`を維持し、入力機施工へは進まない。現実世界の真偽、仮説学習、CLI/HTTP、workspace全試験・Clippy・releaseは未実施。詳細は[B49](construction/MB-BUILD-20261115/BUILD_BATCH_49.md)と[実行観測](construction/MB-BUILD-20261115/b49-build-observation.json)。
- B50では型付き原子命題とExplain目的だけを直接Coreへ渡し、二段の保存規則proofから新しい説明計画を作成した。従来は二つの親規則に加えてCore合成shortcutも通常の「保存規則」unitとして全文表示していた。親規則二件はRule unitとして残し、`derived:proof-composition-v1`は親規則unitを依存先とするCore生成短縮規則Detailへ分離。claim、直接facts、二親rule、shortcut、対象scopeに関する説明が全proof IDを保持したまま`ContentPlan`とK7 `RENDERED`へ到達し、後続`Why?`もv3計算artifactを再読して`COMMIT / DERIVED / RENDERED`した。内容形態変更に伴いcomputation artifactをv2からv3へ上げ、同じwire fieldsの旧v2 reader分岐とv1 readerを保持。B34 derived explanation回帰とB50多段経路は各1 test PASS、engine all-target compile / fmt / diff checks PASS。単一typed API fixtureに限り、一般的説明妥当性、通常CLI/HTTP、他のrule graph/目的、現実世界の真偽を示さないため`CORE-G03=PARTIAL`を維持し、他ゲートは再判定しない。詳細は[B50](construction/MB-BUILD-20261115/BUILD_BATCH_50.md)。
- B51ではCore関係IDにworld `scope`と独立した任意の`temporal_scope`を追加し、typed Engineの関係assertion・rule追加・proof query・説明文・比較groupingを完全一致の時刻範囲で照合する。alpha9で`None`時の旧正準JSON形を維持し、alpha7/8 storeは全参照blobとjournalを検証したうえでmetadataだけをalpha9へ移す。隔離Core観測では同時刻の保存fact＋ruleが`COMMIT / DERIVED / RENDERED`、別時刻のqueryが`SUSPEND / MISSING_PREMISES`でContentPlan/render前に止まり、別時刻にある左右同型relationを共通扱いしなかった。比較artifact V1〜V4再生、domain canonical形、alpha7/8移行、CLI migration引数契約、workspace全target checkを対象検証した。これは型付きAPIと完全一致labelの限定実装であり、自然言語時刻parser、区間比較・chronology、CORE-G01全成立述語を示さないため`CORE-G01=PARTIAL`、`CORE-G03=PARTIAL`を維持し、入力機施工へ進まない。詳細は[B51](construction/MB-BUILD-20261115/BUILD_BATCH_51.md)。
- B52ではCore工事用型付き入口に`correct_atomic_relation`を追加し、完全な`Proposition`同一性（subject/predicate/world scope/temporal scope/object）で既存の直接前提を特定して、既存の訂正・依存失効・一版保存経路へ接続した。隔離actorで時刻付き`Nera is ore`から同時刻規則による`refined ore`が`COMMIT / DERIVED / RENDERED`した後、前提時刻を08:00から12:00へ訂正しmemory revision 4→5、旧fact失効、否定旧factと新時刻replacement追加を観測。Engine再起動後、08:00派生問は`SUSPEND / MISSING_PREMISES`（plan/renderなし）、12:00のreplacementは`COMMIT / USER_PREMISE`、同主体18:00 controlと別主体08:00派生controlは維持。これは一actor・一predicate・一scope・一ruleの直接typed fixtureで、異なるworld scope、条件参照、独立した否定query、一般入力経路は未実施。したがって`CORE-G01=PARTIAL`、`CORE-G03=PARTIAL`を維持し、他ゲートは再判定しない。R4三文書/ZIPは未変更。詳細は[B52](construction/MB-BUILD-20261115/BUILD_BATCH_52.md)と[実行観測](construction/MB-BUILD-20261115/b52-build-observation.json)。
- B53ではalpha10の`Proposition.condition_refs`を正準化されたopaque reference集合として追加し、空集合はJSON出力から省いて従来のcanonical bytesを維持する。型付きAssertion/Rule/Explain/Correctionからsealed Core経路・Fact/Rule memoryへ保持し、rule scope、proof matching/composition、訂正identityと依存失効、比較grouping/key identity、説明labelで完全一致させた。条件欠落・別参照のExplainと参照違いruleはContentPlan/render前に`SUSPEND`し、同じ参照つきfact/ruleは`COMMIT / DERIVED / RENDERED`。条件参照を訂正した後は旧派生proofが`SUSPEND / MISSING_PREMISES`となり、異なる参照を持つ比較relationは共通扱いせず両labelを出す。alpha10通常openはalpha7/8/9を拒否し、明示移行はblob/journal検証後にschema metadataだけを更新し、immutable revision blob identityを維持する。domain canonical互換、alpha7/8/9 migration、B51/B52 temporal regression、Core条件参照fixture、workspace all-target checkがPASS。参照はopaqueで真偽評価・自然言語条件parserは実装せず、scope/negationの全幅やG01成立述語全件も未確認。よって`CORE-G01=PARTIAL`、`CORE-G03=PARTIAL`を維持し、他ゲートは再判定せず、入力機施工へ進まない。詳細は[B53](construction/MB-BUILD-20261115/BUILD_BATCH_53.md)と[実行観測](construction/MB-BUILD-20261115/b53-build-observation.json)。
- B54では既存の型付きCore producer→consumerを、極性とworld scopeを切り替える同一actor・同一caseのfixtureで観測した。同scopeの正Fact＋positive Ruleは依存Fact/Rule IDを持つContentPlanに採用され、K7 render bytesがplanと一致。scopeをまたぐruleは`SUSPEND / SCOPE_MISMATCH`、別scopeのqueryと負Factからpositive Ruleを発火させるqueryは`SUSPEND / MISSING_PREMISES`でplan/renderなし。positive questionとnegative Factの組合せはnegative Fact IDを保持した反対内容の`COMMIT / RENDERED`。Fact scopeをlabからarchiveへ訂正すると旧lab Factが失効し、旧派生結論はplan前に停止。replacement Factはarchive scopeで残るがlab Ruleは移動せず、無関係なMiraのnegative Factは有効のまま。これは直接typed APIの二scope・二主体・一rule fixtureであり、scope/conditionの意味評価、自然言語条件、時刻との同時変更、CORE-G01全成立述語は示さない。`CORE-G01=PARTIAL`と`CORE-G03=PARTIAL`を維持し、他ゲートは再判定しない。詳細は[B54](construction/MB-BUILD-20261115/BUILD_BATCH_54.md)と[実行観測](construction/MB-BUILD-20261115/b54-build-observation.json)。
- B55では同じ直接typed Core routeに二前提Ruleを保存し、新しいVela Factを一つだけ与えた段階では結論問が`SUSPEND / MISSING_PREMISES`、二つ目を与えると両Fact IDとRule IDを持つ`COMMIT / DERIVED`、ContentPlan採用、bytes一致のK7 `RENDERED`に到達した。その後、二前提の片方にnegative Factを追加すると結論問は`MISSING_PREMISES`で採用前停止し、競合した関係への問は`CONFLICTING_PREMISES`で停止した。保存された両極性FactとRuleは監査用に有効のまま残る。これは一actor・一subject・一scope・一つの二前提Ruleに限る。counterfactual経路とその他のpremise graphを含むCORE-G02の成立述語全体は未成立のため`CORE-G02=PARTIAL`。`CORE-G01=PARTIAL`、`CORE-G03=PARTIAL`を維持し、他ゲートは再判定しない。詳細は[B55](construction/MB-BUILD-20261115/BUILD_BATCH_55.md)と[B55観測](construction/MB-BUILD-20261115/b55-build-observation.json)。
- B56で現行HEAD `6d9ec1d` のままB55二前提fixtureと、既存B11 direct typed counterfactual fixtureを限定再実行し、2 tests PASSを確認した。二Fact＋ruleの通常導出・欠落premise停止・通常矛盾停止に加え、明示した反対Factだけをdisposable scenario内で置き換えるcounterfactualは`SUPPORTED / HYPOTHESIS`でhypothesis/rule/Fact proof IDsを保持し、保存MemoryStateを変更しなかった。矛盾をscenario内で除去しない場合は`CONFLICT / UNKNOWN`、無関係Factのoverrideは拒否、通常queryはhypothesisを使わず不足停止。construction gate predicateを満たしたため`CORE-G02=PASS`と更新する。これは直接Coreとtyped hypothesis reportに限定され、自然言語counterfactual、広い因果推論や全R2/93要求受入を示さない。`CORE-G01=PARTIAL`、`CORE-G03=PARTIAL`を維持し、G04/G06/G07は再判定しない。B55のPARTIAL評価は同batch内の観測境界であり、本batchで既存counterfactual経路を現行HEAD上に再実行して更新した。詳細は[B56](construction/MB-BUILD-20261115/BUILD_BATCH_56.md)と[再評価JSON](construction/MB-BUILD-20261115/b56-core-g02-reassessment.json)。
- 施工の現在工程＝**中核K1〜K7の未成立箇所の是正とCORE-G01〜CORE-G07全件ゲート判定**。七件PASS前の入力機局所拡張、通常三機T01〜T03前の新専門能力拡張、躯体未完成での体系QC/QAへの移行を停止。旧ソースを消さず、中核→必要最小入力→必要最小出力→通常三機→全体系→QC→QAの順で実consumerを接続する。
- ゲートの真値は次回施工開始時に**その時点のGit SHA・実コード・実入出力・版と契約**を再観測して更新する。R4導入時の文書整合改訂はRustコード変更や新しい実行試験を主張しなかった。B31/B32以降の実装・試験結果は個別記録へ追加し、七ゲートPASS・全要件受入は主張しない。

## Implemented mechanisms and current evidence boundary

| Mechanism | Actual implementation | What this establishes from source inspection | Remaining acceptance boundary |
|---|---|---|---|
| Versioned records and identity | `minidora-domain` | Separate input, lifecycle, goal, memory, result, render, evidence and effect records; explicit fingerprints and schema identity; canonicalization rejects binary floats before JSON projection | Complete R2 wire/semantic refinement, all compatibility mappings and cross-encoding equivalence |
| Input machine | `input.rs` | Original UTF-8 bytes/spans retained; one finite grammar lowers actual arithmetic, copular, mathematical, document, metadata-search, source and JSON requests into typed intents. Parser v79 keeps v76 NFKC segment mapping and captured original positions for supported direct Japanese assertions, including copular and transitive forms; B23/B24 evidence is joined by B25 for singular-pronoun Japanese `AssertPreviousRelation`: Core revalidates the immediately preceding same-case relation result, restores the typed subject, and stores the sealed input's original pronoun/role ranges on the resulting premise Fact. B26 observes a contextual negative transitive assertion with particle `が`, polite negative `使いません`, and half-width surface `ﾐｽﾞ`; all original role spans, negative polarity and the same Fact proof dependency survive Engine reopen. Store revalidates evidence against the actor-owned sealed InputBundle on memory read/write. These are origin links, not world truth or general Japanese coverage. B20 retains the 1,000,000 overlapping-event typed stop. Engine-package tests, workspace check, warning-denied Clippy and build pass on Linux/Rust 1.99.1. Wider language/source-family coverage and full acceptance remain unverified. | Full subject/negation/condition/time/quantifier/quotation coverage, broader contextual forms and replay populations, and general-language quality |
| Bounded Japanese meaning comparison | `meaning_compare.rs`, Core | Explicit A/B requests produce a typed report over recognized polarity, scope, quantification, modality, condition direction, agent/recipient roles, voice, time/aspect, attribution, causality, numeric/deadline units and normalized Arabic, fullwidth and Kanji counts, including exact decimal coefficients with Japanese scale units; unsupported differences suspend | Finite surface markers only; not general sentence entailment, unrestricted translation, or a first-seen holdout result |
| Core and scheduler | `core.rs`, `scheduler.rs` | Core binds goals, schedules observed prerequisites, adopts content, checks rendering and required write receipts before goal completion; typed terminal states; bounded calculation sequences add each step as a required computed-value node and record each node's exact consumed artifact IDs; a relation-answer follow-up recomputes the same proof and explains its direct premise, derived record and rule lineage | All required capability paths, general multi-step goal composition, complete R2 negative cases and task-bound resource validation; coverage beyond selected bounded calculation-sequence and relation-follow-up CLI cases remains unverified |
| Code generation, limited reading and bounded evaluation | `code_generation.rs`, `code_capability.rs`, `input.rs`, Core | Explicit inherited algorithm IR produces Python 3.12 source; a separate whole-source parser reads one bounded Python-function subset and a native Rust evaluator returns actual evaluation or finite-case observations through report/content/render/history artifacts; no host Python execution, `exec`, `eval`, compiler or external effect is introduced. Generated-source host-Python parse/execution/input-output tests remain `NOT_RUN` in the generation report | General language code creation/explanation/revision/repair, complete Python subset and runtime compatibility, static name/type analysis, mid-evaluation cancellation polling, complete resource and target/Unicode compatibility |
| Arithmetic and context | `language.rs`, Core | Exact rational calculations with steps; binary addition/subtraction preserves identical explicit units or converts mixed `mm`/`cm`/`m`/`km` exactly to metres; bounded binary literal corrections cover `+`, `-`, `*`, `/`, `%`, `^` and `**`, linked to the actual prior result; bounded `@{step-id}` substitution consumes earlier exact rationals or compatible exact quantities in existing addition/subtraction, retaining source offsets and artifact hashes | Full inherited quantities/units/numeric edge contracts, nested/compound correction, dimensional multiplication/division, unit-bearing sequence use outside existing addition/subtraction and independently evaluated multi-turn quality |
| Symbolic, linear and probability mathematics | `math.rs` | Exact bounded polynomial normalization/derivative/representative integral and evaluation; exact single-variable linear equations with unique, identity or contradiction classification; exact single-variable quadratic equations with rational, radical or complex roots; exact linear systems with invertible-operation/source-substitution certificates; exact supplied-weight normalization, finite weighted expectation/variance and explicit finite-event probability | Broader probability problems, ordinary-route execution evidence, all predecessor equivalent cases and general scientific reasoning |
| Descriptive statistics | `input.rs`, `document.rs`, `ports.rs`, `math.rs`, Core | Exact supplied-value sum/mean/median/extrema/variance; explicit named CSV column and JSON Pointer array consumers preserve source spans; exact mixed `mm`/`cm`/`m`/`km` normalization for CSV quantities | Broader independent document-statistics cases beyond the selected ordinary-route evidence, other mixed dimensions/units and real-world population interpretation |
| Declared relations and learning | `relations.rs`, Core | Copular user premises, typed polarity/scope/opaque condition references/temporal scope, exact condition-and-time rule application, proof-preserving composition, dependencies and invalidation; ordinary Japanese premise-listing returns active direct USER_PREMISE records with scope and IDs | Condition-reference truth evaluation, generalized learning from genuine new observations, empirical admission, derived-fact listing, out-of-domain/counterexample quality and large memory experiments |
| Non-neural sequence model | `language.rs`, Core | Character-ngram transform; exact conditional distribution restricted to current admissible tokens; deterministic role/dependency-constrained composition from typed meaning units, with model scores unable to establish factual authority | Full unseen-language quality, natural-language request route, cross-request learning, inherited-versus-alternative comparison under R2-17 and open-domain generation remain unverified |
| Durable store | `store.rs` | SQLite WAL/FULL/FK reference adapter, immutable digest-addressed blobs, actor checks, revision CAS, event chain and saved input/action identities; explicit integrity-checked alpha 7/8/9 metadata migration to current schema; blob staging skips stale PID/serial names without replacing orphan files; two-process CAS, exact replay, conflicting-input and case-owner races preserve one revision/registration and reject or retain the competing attempt | Physical power-loss guarantees on each target, complete fault-injection matrix, larger concurrency populations, every read/export access path and measured long-history bounds |
| Output and local file effect | `output.rs`, Core | Actual UTF-8 render observations; exact-target grant; exclusive/non-overwriting byte write; retained ambiguous outcome and reconciliation identity; pairwise process race to one target with differing bytes confirms only the complete winner and rejects the other; transactional action ID/idempotency ownership, exact replay is idempotent and conflicting key reuse is typed; startup verifies immutable action identity matches the saved action record | Hostile concurrent filesystem isolation, remote-delivery providers, once-only remote reconciliation and full receipt semantics |
| Direct source observation | `ports.rs` | Bounded permitted HTTPS:443 text/Markdown/JSON observations using inherited TLS/proxy trust; exact byte/status/URI/failure provenance | Supported HTML/PDF/browser/source connectors, claim-to-body synthesis and LIVE benchmark conditions |
| Provider metadata search | `search.rs`, Core | Explicit Wikipedia/Crossref/Europe PMC metadata listing with ranked candidate/source identities, rejection/failure accounting and separate body-observer boundary | Actual provider reachability/quality, full article body acquisition, source synthesis and unchanged GPQA provider contract |
| Explicit evidence ledger | `input.rs`, `ports.rs`, Core | Strict caller-supplied source/claim ledger; groups exact claim IDs/text and SUPPORTS/REFUTES stances into agreement, contradiction, same-group repetition or single-source records; validates ordered premise IDs for declared inferences and retains JSON source ranges | External document claim extraction, source identity/independence verification, entailment validity and broader independent normal-route acceptance beyond selected explicit-ledger examples |
| Explicit-source composition/editing, unique edit location and extractive summary candidates | `minidora-domain`, `input.rs`, `document.rs`, `text_editing.rs`, `text_summary.rs`, Core | Ordinary Japanese/English CLI requests compose literal material slices, import explicit text/locks, propose a unique literal replacement location without applying it, apply atomic original-checked patches to actual case-scoped artifacts, and emit ranked source-sentence candidates with byte maps, optional hard character budgets that never truncate a sentence, selectable bullets/paragraph output and caller-required source sentences in a V4 artifact, and an explicit unverified-coverage label | Free-form authoring, natural-language correction discovery, semantic editing/rewriting or summary verification, purpose/quality validation, in-progress editing cancellation polling and full twelve-task B08 acceptance |
| CSV conversion, comparison and literal editing | `minidora-domain`, `input.rs`, `document.rs`, `ports.rs`, `math.rs`, Core | Strict quoted CSV/source byte spans; named-column selection, exact-string row filtering, stable string sorting, first-seen exact-string group counts, and exact per-group descriptive statistics with original group/value cell spans; keyed two-table CSV comparison with exact unique string keys, schema checks, added/removed rows, changed cells and source spans on both inputs; exact-key CSV inner joins with unique keys, left-order rows, namespaced columns and source-mapped cells; ordinary CLI CSV↔JSON string-table conversions with explicit header mode and retained conversion artifacts; exact count-guarded nonoverlapping replacement | Normal-route evidence for remaining document operations; general schema/table comparison beyond bounded keyed comparisons and arbitrary writing/editing-intent/quality acceptance |
| Local effect reconciliation | `recovery.rs`, CLI | Observes an original pending local file without resend/new authority; cancelled-effect mode updates the action ledger while preserving case CANCEL | Remote provider-specific idempotency, complete crash/fault population and general exactly-once effects |
| Structured JSON selection/projection/comparison/filtering/sorting/aggregation/statistics | `input.rs`, `ports.rs`, `math.rs`, Core | Strict complete-document validation, duplicate-key rejection, JSON Pointer selection and root-object projection preserve source ranges; ordered multi-column table projection retains header/cell locations; exact named-column string-table filtering retains every predicate and emitted-cell source range; stable exact-string table sorting preserves source row order on ties and source locations on emitted cells; first-seen exact-string group counts retain every member source range; explicit JSON number arrays flow into exact descriptive statistics; two-sided structural comparison uses exact bounded decimal equality; keyed string-table comparison reports added/removed rows and changed cells with both source ranges/digests; exact unique-key inner joins preserve left order, namespace both output schemas and report unmatched rows with source spans; same-case inner-join follow-up revalidates the sealed input and report, explains matched rows and unmatched keys with source byte spans, and participates in no-resend recovery | Normal-route evidence for remaining operations, general schema transformations and complete structured-document capability inheritance |
| Narrow target comparison | Domain conformance example and `target-conformance.json` | One canonical Unicode/exact-string record and lifecycle predicates match native Linux and Node WASI, with an independent Python hash check | Full Core/storage/effect/backend semantic equivalence and physical Windows/8GB acceptance |
| CLI and local HTTP | `apps/cli` | Thin frontends share the same durable engine; distinct terminal exits; token-bound loopback HTTP actor/grants/budgets | Human browser UI, independent consumer apps, deployed multi-user authentication and external platform acceptance |
| Provenance and obligations | `rust/audit/` | Complete declared source/test file identity populations, explicit unknown semantic routes and every current requirement preserved | Exhaustive semantic-entry review, all applicable test translations and all actual observed requirement outcomes |

These rows describe source-visible mechanisms. Unit tests and route tests are different evidence levels; final command results must be linked to their actual current code and execution artifacts. No test counts are inferred from definitions, loops or historical passes.

## Bounded direct Core construction observation 01

An in-process test now drives `minidora_engine::Engine::execute` with a synthetic `virtual-lab` rule requiring both `ceramic` and `heated`. With only `Pico is a ceramic`, the conductive query suspends as `MISSING_PREMISES`; after adding the second premise it commits as `DERIVED`, carrying the two direct fact IDs, the rule ID and the derived query identity. `Luma` in the same scope and `Pico` in `default` scope both suspend. A negative `Fenn is not heated` premise is returned as the counterevidence to a positive heated query and does not satisfy the rule's positive heated condition. The saved actor memory retains the four directly supplied facts and the declared rule; the derived query identity remains in the result dependency record rather than being admitted as a persistent fact. The exact input/result records and final memory are in [the direct Core construction observation](bb-core-construction-observation-01-v1-validation.json).

This single relation observation enters through the public in-process `Engine` API, bypasses the CLI and does not invoke an external provider; the normal `input::seal` parser still runs. It covers only the first of the seven construction observations and one synthetic rule/query path. It does not verify typed-IR injection independent of the parser, derived-fact admission, generalized learning, the other six observations or complete R2 acceptance. Requirements A05 and A15 remain `UNVERIFIED`.

## Bounded saved-calculation explanation follow-up

The ordinary Japanese follow-up `詳しく説明して` can consume a prior committed arithmetic result and produce one of two Core-derived explanation surfaces. Both carry the same retained expression, all exact calculation steps and exact result; the local sequence model ranks those authorized surfaces and does not generate or validate arithmetic. The selected result, candidate set and model observation are retained as case artifacts, and file-effect inspection re-derives and replays the candidate set. See [calculation-explanation-followup-route-validation.json](calculation-explanation-followup-route-validation.json) for focused evidence. This is a narrow context/content/math route; unrestricted language generation, general writing, full B02 and full successor acceptance remain `UNVERIFIED`.

## Bounded fetched-source follow-up

The explicit Japanese/English `SummarizePreviousSource` intent can consume successful fetched-body packets from an actual committed prior result. Core verifies the exact packet blob in the same actor/case, stores the candidate report with source result/packet references, request/final URI, raw-body hash, extracted-text hash, parser version and observation time, then renders exact copied sentences with a disclosure that semantic coverage is unverified. The packet schema has no license evidence, so the output says that license information is unrecorded. The local-TLS fixture ran an actual fetch, a separate-process follow-up and a test-only interrupted file commit reconciled without resend; see [source-summary-follow-up-route-validation.json](source-summary-follow-up-route-validation.json).

Parser v62 adds Japanese/English maximum-bullet and copied-sentence-character limits for this follow-up as an additive intent. The constrained route requires exactly one eligible previous fetched body, verifies and records its exact actor/case packet reference, and stops with `AMBIGUOUS_SOURCE_CONTEXT` when more than one body could receive the total limit. The report records the effective limits; the local-TLS follow-up also wrote a confirmed temporary-file receipt and reconciled it without resending. [bb-source-summary-bounded-constraints-route-validation.json](bb-source-summary-bounded-constraints-route-validation.json) records the scoped observation. Existing unconstrained multi-packet behavior remains covered by the legacy intent. Search-list snippets, LIVE provider quality, claim truth, conflicting-source reasoning, source-license handling, semantic summary quality, full B08 and full BB-SOURCE/B05 acceptance remain `UNVERIFIED`.

## Bounded declared-rule follow-up

The ordinary CLI route stores an explicitly supplied one-premise rule and a new user premise, then applies that rule to a previously unseen subject in a later process. A counterexample to the resulting answer makes Core suspend on the conflict; invalidating the rule preserves the prior records and leaves the direct counterexample as the only answer dependency. The route exposed and corrected a guarantee-label defect for answers supported by a direct negative premise, and query dependencies now retain the root premise, derived fact and rule identities. Recovery verifies the v46-v49 proof lineage and guarantee while retaining the legacy v45 dependency contract for older sealed requests. The current Core/parser replay is recorded in [the v52 declared-rule route evidence](bb-learn-declared-rule-follow-up-v52-route-validation.json); [the original route record](bb-learn-declared-rule-follow-up-route-validation.json) remains historical. A separate v58 normal-CLI consumer route now transfers the explicitly scoped rule to a new subject in a new case, consumes the proof through `AskWhy`, excludes the original subject's premise from the new proof, checks scope and actor isolation, suspends on a counterexample, and verifies invalidation plus original-result replay. See [the v58 cross-case route evidence](bb-learn-cross-case-rule-transfer-v58-route-validation.json). This demonstrates bounded user-declared rule reuse, not empirical rule induction, general learning quality or full BB-LEARN acceptance; those remain `UNVERIFIED`.

## Bounded singular-pronoun relation question

The v58 Japanese/English CLI route resolves `それ`/`it` for a relation Ask or AskWhy only when the selected prior committed same-case result points to exactly one active direct user premise. Core uses that premise's subject and inherits its scope unless the question supplies one exact scope. The resulting answer and explanation use the ordinary relation proof query and retain its dependencies. No prior result, a non-relation topic change, another case or actor, a scope mismatch, or multiple candidate premises produces a typed stop. See [the v58 pronoun relation-query evidence](bb-context-singular-pronoun-relation-query-v58-route-validation.json). This is a bounded antecedent path; plural/multi-clause reference, general topic-return behavior and full A04/A07/A08/CAP-D04/CAP-D11 acceptance remain `UNVERIFIED`.

The ordinary Japanese question `ココについて、何が分かっていますか` reaches Core's actor-scoped memory across fresh CLI processes. The v47 route lists active direct premises with polarity, scope and fact IDs, excludes derived conclusions, and marks that the premises have not been checked against the outside world. A later invalidation is reflected in a new listing; a subject without an active premise receives `MISSING_PREMISES`. Focused parser, relation and normal-route tests passed. The actual file-effect replay also recovered the same dependency set as `CONFIRMED` with `LOCAL_FILE_RECONCILED_NO_RESEND` and unchanged file inode/mtime/size. See [the premise-inspection route evidence](bb-lang-premise-inspection-route-validation.json). This closes one bounded memory-retrieval consumer; general natural-language Q&A, unconstrained explanations/comparisons and full BB-LANG/A13 acceptance remain `BUILD_GAP` / `UNVERIFIED`.

The v49 `なぜ` / `どうして` and English `Why ...?` route queries one explicit copular proposition against current actor memory. The answer lists direct premise, derived fact and applied rule IDs; a direct negative premise explains the opposite relation, while missing or conflicting evidence suspends. Focused parser and normal-CLI tests passed, and fresh CLI processes exercised Japanese/English support, both typed stops, and file-effect recovery as `CONFIRMED` / `LOCAL_FILE_RECONCILED_NO_RESEND` with unchanged file inode/mtime/size. See [the why-question route evidence](bb-lang-why-question-route-validation.json). This is a bounded relation-proof explanation only; general causal understanding, unrestricted why/how questions and full BB-LANG/A13 acceptance remain `BUILD_GAP` / `UNVERIFIED`.

Parser v50 added `詳しく説明して` after structured Python generation; v52 preserves that route and the Japanese/English Why route. Core retrieves the same-case immutable `CodeGenerationReport`, verifies the report against the prior output and regenerates its strict structured IR before describing assignments, branches, loops, expressions and returns. The explanation carries the prior result and report artifact dependencies. A local file delivery was reconciled as `CONFIRMED` / `LOCAL_FILE_RECONCILED_NO_RESEND` with unchanged inode/mtime/size. The current replay is recorded in [the v52 code-context route evidence](bb-content-code-context-explanation-v52-route-validation.json), and [the v52 Why route record](bb-lang-why-question-v52-route-validation.json) confirms that the earlier question route remains available. The generator's Python execution and I/O test fields remain `NOT_RUN`; free-form code explanation/revision, general context/paraphrase and complete B07/F04 acceptance remain `BUILD_GAP` / `UNVERIFIED`.

For a committed relation answer, a fresh-process `詳しく説明して` request reparses only Core's exact prior answer surface, recomputes the same supported proof, and renders the conclusion with its directly supplied premise, derived record and applicable rule IDs. A same-actor rule invalidation in another case makes the follow-up suspend as `CONTEXT_PROOF_CHANGED`. A file-delivery recovery restored the relation proof and dependency list before reconciling the existing local write as `CONFIRMED` / `LOCAL_FILE_RECONCILED_NO_RESEND`; the file inode, modification time and size stayed unchanged. The finite route also covers a prior answer supported by a direct negative premise. See [the relation-answer context route evidence](bb-context-related-answer-explanation-route-validation.json). General explanations, paraphrase, examples, broader topic continuity, and full BB-CONTEXT/A07/A14 acceptance remain `BUILD_GAP` / `UNVERIFIED`.

## Current-run validation

The alpha 7 Linux development run passed formatting, whole-workspace/all-target checking, Clippy with warnings denied, all-feature tests (134 passed, 0 failed, 0 ignored) and build. See [validation.json](validation.json) and [executed-tests.json](executed-tests.json) for the exact source snapshot and outcomes. These checks do not establish full successor acceptance.

The later mixed-length-statistics slice was checked through the ordinary CLI route and exact component arithmetic. [unit-conversion-validation.json](unit-conversion-validation.json) identifies its source/test paths, Linux toolchain, command results and observed positive/negative route cases. It is partial B02 implementation evidence; B02 acceptance and the full 93-capability successor population remain `UNVERIFIED`.

The [probability normalization validation](probability-normalization-validation.json) records exact normalization of explicit finite weights and its initial workspace formatting/check/lint/build results. The later [B03 mathematics route follow-up](b03-mathematics-route-validation.json) observes Japanese and English normal-CLI cases, including exact normalized probabilities and a typed zero-total suspension. B03 and full successor acceptance remain `UNVERIFIED`.

The [finite probability statistics validation](finite-probability-statistics-validation.json) records exact expectation and variance over explicitly paired finite outcomes and weights, sharing exact weight normalization. Its later B03 route follow-up checks normalized probabilities, contribution arrays, expectation and variance for distinct Japanese and English inputs; broader B03 remains `UNVERIFIED`.

The [finite-event probability validation](finite-event-probability-validation.json) records exact probability mass for explicitly listed event values over a finite weighted outcome list, with aligned contribution evidence. Its later B03 route follow-up checks membership, per-outcome mass and duplicate event-value handling through Japanese and English inputs; broader B03 remains `UNVERIFIED`.

The [polynomial evaluation validation](polynomial-evaluation-validation.json) records the ordinary input/Core intent wiring to an existing bounded exact evaluator, with per-term value/contribution output and workspace formatting/check/lint/build results. Its later B03 route follow-up checks exact term values and contributions for Japanese and English requests; broader B03 remains `UNVERIFIED`.

The [quadratic equation validation](quadratic-equation-validation.json) records the bounded exact one-variable degree-two solver and its original build-only checks. A later combined follow-up in [linear-equation-validation.json](linear-equation-validation.json) ran the full Rust workspace tests and observed the rational-root quadratic request through the normal CLI route; repeated, radical and complex branches were checked at the component layer. B03 remains `UNVERIFIED`.

The [linear equation validation](linear-equation-validation.json) records the bounded exact one-variable degree-one solver, including exact rational substitution and identity/contradiction classification. Its follow-up ran 154 workspace tests, including normal-CLI checks for a rational solution, identity, contradiction and degree rejection; broader B03 acceptance remains `UNVERIFIED`.

The [B03 mathematics route follow-up](b03-mathematics-route-validation.json) adds normal-CLI evidence for descriptive statistics, weight normalization, finite expectation/variance, event probability and polynomial evaluation. The full workspace run passed 158 tests, including the new cases. These scoped examples do not establish an independent acceptance population or the complete B03 contract, which remains `UNVERIFIED`.

The subsequent unit-aware arithmetic slice adds exact binary addition/subtraction, preserves operands and units, and explains the persisted calculation through a new ordinary CLI process. [unit-arithmetic-validation.json](unit-arithmetic-validation.json) records that source snapshot and its 141-test Linux workspace run. The initial correction slice in [unit-correction-validation.json](unit-correction-validation.json) established versioned operand replacement for unit-aware addition/subtraction. [unit-binary-correction-validation.json](unit-binary-correction-validation.json) extends the ordinary route to bounded dimensionless `*`, `/`, `%`, and power corrections while preserving the prior result. [unit-sequence-validation.json](unit-sequence-validation.json) records the initial build-only consumption of exact unit-bearing upstream calculations by the existing addition/subtraction route; that historical slice ran no tests or ordinary CLI execution. [b06-calculation-sequence-route-validation.json](b06-calculation-sequence-route-validation.json) subsequently observes scalar and unit-bearing consumers, complete-operand guards, exact artifact identities, typed stops, version history and one confirmed local file effect through the CLI. Nested/compound correction, dimensional multiplication/division, referenced unit operations outside addition/subtraction, inherited quantity coverage and full B02 acceptance remain `UNVERIFIED`.

The [schema migration validation](schema-migration-validation.json) covers an alpha 7 state with a legacy case revision/blob, successful metadata-only migration, idempotent repeat, explicit CLI database-path handling and an alpha 8 CLI no-op. It does not establish power-loss behavior on other targets or change the 93-capability acceptance status. [CSV statistics validation](csv-statistics-validation.json) records the named-column producer-to-consumer implementation; [JSON statistics validation](json-statistics-validation.json) records the exact JSON-array path; [structured table conversion validation](structured-table-conversion-validation.json) preserves the initial build-only bidirectional CSV/JSON Core wiring record; [CSV table comparison validation](csv-table-comparison-validation.json) records keyed CSV-to-CSV matching and source locations; [CSV table inner-join validation](csv-inner-join-validation.json) preserves the initial build-only exact-key join record; [JSON table filter validation](json-table-filter-validation.json) records exact-string filtering of a JSON string table with per-row decisions and source ranges; [JSON table sort validation](json-table-sort-validation.json) records stable exact-string sorting with retained row/cell source locations; [JSON table group-count validation](json-table-group-count-validation.json) records first-seen exact-string aggregation with member locations; [JSON table select validation](json-table-select-validation.json) records exact named-column extraction with original row indices and cell ranges; [JSON table projection validation](json-table-project-validation.json) records ordered multi-column projection with retained cell source locations; [JSON object projection validation](json-object-project-validation.json) records selected root-object fields in caller order with original key/value and output ranges. [CSV grouped-statistics validation](csv-group-statistics-validation.json) preserves its initial build-only evidence. The [B04 CSV grouped-statistics route follow-up](b04-csv-group-statistics-route-validation.json) adds Japanese and English ordinary-CLI cases with exact per-group statistics, UTF-8 source-span verification and durable inspection, plus a typed mixed-value suspension with no result dependency; all 160 workspace tests passed. These selected author-created cases do not close B04, which remains `UNVERIFIED`. [JSON table grouped-statistics validation](json-table-group-statistics-validation.json) preserves its initial build-only evidence. The [JSON grouped-statistics ordinary-route follow-up](b04-json-table-group-statistics-route-validation.json) observes exact selected-column results, escaped/UTF-8 source-range hashes, durable inspection and a typed mixed-value stop in Japanese and English. It corrects the grouped CSV/JSON case-sensitive admission guards to match their existing case-insensitive header parsers and checks cased English through the normal CLI. Ten scoped tests passed (three CLI and seven input tests); full workspace runtime regression is `NOT_RUN` for that snapshot. [JSON table inner-join validation](json-table-inner-join-validation.json) preserves its initial build-only record. The [ordinary table-join follow-up](b04-table-inner-join-route-validation.json) observes CSV/JSON joins in Japanese and English, left-order rows, namespaced columns, unmatched keys, two-sided source ranges and durable duplicate-key stops. It records that CSV matched rows previously produced invalid JSON despite `COMMIT`, corrects the missing inter-side comma, and aligns join admission with the existing case-insensitive complete header parsers. Eighteen scoped tests passed; explicit versioned recomputation of a real prior invalid CSV result produced valid JSON while retaining the original result. Historical exact retries still preserve that old result. Full workspace runtime regression is `NOT_RUN` for this follow-up, and B04 remains `UNVERIFIED`.

The [ordinary table-conversion follow-up](b04-table-conversion-route-validation.json) observes CSV↔JSON roundtrips using actual previous CLI output as the next explicit input, in Japanese and English and both CSV header modes. The selected canonical fixture preserves string values, quotes, CRLF, escapes and exact nine-span source/output maps. Formula-leading strings remain inert JSON and then stop on CSV export; schema and missing-mode errors remain durable typed stops. Conversion admission now matches the existing case-insensitive complete header parsers. Nineteen scoped tests passed; full workspace runtime regression is `NOT_RUN` for this snapshot and B04 remains `UNVERIFIED`.

The [ordinary table filter/sort follow-up](b04-table-filter-sort-route-validation.json) observes exact-string CSV/JSON row filtering, stable ascending/descending string ordering with ties, zero-row results, predicate/decision evidence for rejected rows and full selected-cell source maps in Japanese and English. Actual strict CSV-filter output is consumed by a new JSON-sort CLI request; JSON reports retain their explicit projection boundary. CSV sort source-span kinds now follow the sort operation rather than the shared filter label; real before/after artifacts have identical table bytes and differ only in those labels. Twelve scoped tests passed; full workspace runtime regression is `NOT_RUN` and B04 remains `UNVERIFIED`.

The [ordinary table/object selection and projection follow-up](b04-table-projection-route-validation.json) observes named-column selection, caller-ordered table/root-field projection, empty rows, exact string/raw value preservation and complete selected source/output maps in Japanese and English. A real sort report first fails direct CSV export, then explicit columns/rows projection produces strict table data consumed by a new CSV request; the earlier failure remains recorded. Five focused normal-CLI tests passed. This batch changes tests/evidence only; full workspace runtime regression is `NOT_RUN` and B04 remains `UNVERIFIED`.

The [ordinary CSV/JSON group-count follow-up](b04-table-group-count-route-validation.json) observes first-seen exact-string counts, 14 rows across 10 groups, quoted/escaped key equality, distinct case/space/numeric-looking/Unicode-normalization forms, complete original-member locations and empty tables in Japanese and English. Actual count reports feed explicit groups projection, and nine malformed or unsupported requests retain typed stops. Four focused normal-CLI tests passed across 23 real stores and 25 execute requests. This batch changes tests/evidence only; full workspace runtime regression is `NOT_RUN` and B04 remains `UNVERIFIED`.

The [ordinary structured-comparison follow-up](b04-structured-comparison-route-validation.json) observes exact-decimal JSON structural equality/differences, keyed CSV/JSON table changes in declared source order, raw two-sided difference ranges and reordered equal/empty tables in Japanese and English, plus actual report-field projection and 22 English-header typed stops. A left-side CSV cell containing reserved LEFT marker text previously reached `COMMIT`; the missing symmetric admission guard is corrected. An exact retry preserves the historical result, while explicit input version 2 stops with the original result retained. Five focused tests passed; the affected CSV test also passed after a decoded-header map assertion was added. Full workspace runtime regression is `NOT_RUN` and B04 remains `UNVERIFIED`.

The [ordinary basic extraction/statistics follow-up](b04-basic-extraction-statistics-route-validation.json) observes named CSV extraction with complete source/output maps, exact CSV and JSON-array statistics, raw JSON Pointer selection and actual extraction-to-statistics/root-field projection. Scientific values previously became invented unit labels (`1e3 + 2e3` produced `3 e3`, and selected `1e3,2e3` yielded mean `3/2 e3`); shared numeric-token and maximal-prefix guards now preserve scalar meaning and exponent stops. Pointer input retains the original body and uses the source-preserving parser without a preliminary floating-point conversion, so raw `1e400` can be selected. Parser revision is v34. Seven new CLI tests and two new component tests passed, plus three distinct existing regression tests. Thirty-three English-header/input stop cases and eight real-state version-2 recomputations are recorded, with old results retained. Full workspace runtime regression is `NOT_RUN`; B04 remains `UNVERIFIED`.

The BODY-1 [CSV statistics context-follow-up route](bb-math-data-csv-statistics-context-followup-route-validation.json) now consumes an actual prior `CSV_STATISTICS` report on the next `詳しく説明して` request. Core recomputes the report from its retained CSV and rechecks the source hash, output hash, source cells and immutable artifact before rendering the exact statistics and cell coordinates. The result keeps both the prior result and report references; a local file interruption was reconciled as `CONFIRMED` / `LOCAL_FILE_RECONCILED_NO_RESEND` with unchanged inode/mtime/size. This connects one CSV exact-statistics producer to a source-aware explanation consumer. Grouped statistics and other CSV/JSON report types do not use this consumer; full BB-MATH-DATA, BB-CONTEXT/B02/B04/A07/A14/F04 acceptance remains `UNVERIFIED`.

The [ordinary explicit-evidence ledger follow-up](b05-evidence-ledger-route-validation.json) observes four classifications, supporting and refuting agreement, exact supplied independence-group labels, same-group contradiction and ordered unvalidated inference links, with complete reported source maps and actual projection/Pointer consumers. English headers previously stopped before the full parser because an 18-byte prefix was compared with the 19-byte command; literal-length admission corrects that route and advances the parser to v35. Four normal-CLI tests and one input test passed, with affected fixture/pipeline tests rerun after a premise-order oracle improvement. Twenty-nine English-header stops and explicit version-2 recomputation of three saved states preserve the original results. This is caller-supplied metadata classification; external identity/independence, source claim extraction, claim truth and inference entailment remain unverified. Full workspace runtime regression is `NOT_RUN`; B05 remains `UNVERIFIED`.

Earlier scoped actual-source checks of mathematical components are component evidence only. They do not close the 93-requirement population, count as independent evaluation or replace ordinary-route execution of the current workspace.

## Resource defect, correction and exact observation boundaries

The original failed observation remains unchanged in [resource-failure.json](resource-failure.json). An immutable predecessor README was fetched successfully (HTTP200, 10,452 bytes), then sustained CPU work exceeded the default 30-second deadline; the validation process was terminated after more than a minute. Its original RUNNING state remains retained.

The [same-source retest](resource-same-source-retest.json) verifies the **same original URI, 10,452-byte body and SHA-256** `5c7c93a8c5d98613b8971b5815843ccd6a1667bcc044e76e71e737987578f719`. The corrected binary returned `SUSPEND` in **6.699 seconds**, exit2, at the explicit **8192-bit exact-probability operation cap**, without triggering the protective subprocess timeout. This is successful observation of a finite typed resource stop, **not successful rendering or acceptance of the long document**. Original source/effect evidence was retained.

The [separate admission test](resource-failure-retest.json) used a different upstream README with **22,888 bytes** and a different digest. It stopped at a **16,384-byte model source/sequence admission limit**. This larger-source check is not a same-input causal reproduction and does not replace the exact-source retest. The shorter README example is another independent support check.

These observations do not close long-history resource, all-deadline, physical Windows/8GB, throughput/latency or full source-workflow requirements. Explicit inner polling and mathematical representation guards remain implementation contracts whose limits and target-specific behavior must stay visible.

## Source and test accounting

The [identity inventory](README.md) independently checks **416 beta source files** (414 upstream plus two beta additions) and the **360-path union of upstream/beta tests and fixtures** (359 upstream paths plus one beta-only path). The beta contains 147 of those paths; the other 213 upstream paths are available in a separate read-only upstream reference. Those are file counts, not executed test-case counts.

`source_contract_reviews.tsv` records scoped full-source contract inspection for 96 files by the population audit worker. Other workers' separately recorded inspections are not silently added to that count. `static_imports.tsv` and `static_callables.tsv` describe syntax-level references across all available Python modules; they do not prove runtime reachability or complete semantic inspection. Unreviewed semantic paths remain unknown.

The predecessor's 313 selected regression cases and any newly run frozen-beta checks are compatibility/non-mutation evidence for that exact predecessor. They are not Rust regressions until actual equivalent contracts are executed against the Rust route.

## Open full acceptance gates

The complete list and exact IDs are in [implementation-obligations.md](implementation-obligations.md). Material open gates include:

- All 93 obligations and source-discovered additions need actual routes and separate implementation/acceptance outcomes. Unmapped or unsupported obligations remain required.
- B06 has selected ordinary CLI observations of bounded scalar/unit calculation chains with actual upstream values and artifact hashes, typed stops, retained versions and a local file receipt. General multi-step problem solving, unseen task quality and broader goal composition remain unverified; B06 remains `UNVERIFIED`.
- S01–S12 and R2-01–R2-22 require actual intended-version observations. A local negative test cannot stand in for a remote effect, second backend or physical host observation.
- A new author-created 24-question, 12-writing-task, ten-session and 30-transformation diagnostic with item keys and a fixed rubric is available under [`quality-v0.1.0`](quality-v0.1.0/README.md). Alpha 1 suspended all 116 response units; alpha 2 committed the 30 meaning transformations and suspended the other 86. Output inspection found false obligation/condition and role labels, which alpha 3 corrected; alpha 4 clarifies absent-feature reporting and removes a duplicate annual-frequency label. Alpha 5 accepts additional explicit Japanese/English comparison directives with labelled A/B lines, while ambiguous and compound-action requests remain suspended. The route counts remain 30 `COMMIT` transformations and 86 `SUSPEND` responses. All runs remain unscored, the historical population is unrecovered, and no owner-approved aggregate acceptance threshold exists. General Q&A and unconstrained writing remain unimplemented.
- Memory → inference → adaptation must be demonstrated on genuinely new subsequent tasks, including counterexample invalidation, restarts and bounded long-history computation. Retaining rules or composing a declared proof is only part of that requirement.
- Search-list and fetched-body packets, contradictions, source-span grounding and changing LIVE sources need real connector/evidence acceptance. A plain body quotation is a narrower implemented operation.
- GPQA retains all 198 questions, its exact dataset identity, LIVE-only/no-gold-learning boundary, paired parallel/serial contracts and 90-minute limits. No new GPQA score is claimed.
- Full independently implemented target/backend projections and the physical Windows CPU/8GB resource profile require actual measurements. The recorded Linux/Node-WASI domain-vector comparison is narrow evidence only; it does not establish whole-Core, storage, effects or physical-target equivalence.
- Remote effects, modalities, translation/application consumers, browser/deployed authentication and any required protected-data encryption need their actual provider, authorization, failure, privacy and resource evidence.

No current omission is an accepted scope reduction. This development version remains inspectable while work toward the complete requested successor continues.


## Final current-instance validation record

[validation.json](validation.json) and [executed-tests.json](executed-tests.json) record the exact code snapshot and executed suite outcomes: **134 Rust tests passed, 0 failed, 0 ignored**, plus **8 separate local TLS fixture cases**. The frozen Python beta separately passed its selected **313** tests during the combined installation rehearsal. These predecessor/fixture counts are not added to the Rust suite or presented as full-capability acceptance. Formatting, whole-workspace/all-target checking, Clippy with warnings denied, all-feature tests and build succeeded.

[dependencies.cdx.json](dependencies.cdx.json) now contains the actual locked metadata inventory (155 packages and declared licenses); it is not a legal or advisory clearance. [rust-requirement-observations.json](rust-requirement-observations.json) retains all93 obligations with actual partial implementation locations where established, separate full acceptance gaps and explicit unimplemented/unverified rows. No required scope was removed.

The optional search-provider LIVE probes were rejected by the current Cloud proxy with CONNECT403; the required destinations and unconfirmed environment-draft persistence are recorded in [runtime-requirements.json](runtime-requirements.json). The tested install/start instructions are retained under `../tools/cloud-install.sh` and `../tools/cloud-start.md`; saving their draft through the environment API returned INVALID_ARGUMENT, so reusable backend configuration persistence and publication are not established.

## B06 bounded calculation-sequence route correction

[b06-calculation-sequence-route-validation.json](b06-calculation-sequence-route-validation.json) records two corrections: Core maps consumed step IDs into the scheduler node namespace; quantity substitutions must match complete operands in the existing unit addition/subtraction grammar. On the published base, referenced steps suspended because of the scheduler namespace mismatch. Only an uncommitted, unpublished namespace-only diagnostic exposed numeric/unit concatenation and exponent-prefix misinterpretation; these observations are kept separate.

Five focused CLI tests, one reference-boundary component test, six scheduler tests and four existing arithmetic/unit regressions passed. The final CLI pass uses 63 fresh stores and 68 execute requests; the earlier pass is retained separately. Exact retries in ten diagnostic stores preserve their original results, and explicit version 2 recomputes while retaining those originals. Source spans, repeated reference occurrences, actual step/report/compute/content/render graph artifacts, scoped file bytes/receipt and version history are checked. These authored cases are scoped QC; B02, B06, the 93-capability population and full R2 acceptance remain `UNVERIFIED`.

## B07 explicit algorithm generation route

[b07-code-generation-route-validation.json](b07-code-generation-route-validation.json) records the first Rust connection for the inherited structured function generator. Japanese and case-insensitive English headers seal the exact JSON document; Core produces the actual source and report, verifies rendering and any granted file receipt. Constants remain separate, all inherited expression/statement forms are represented, and unknown fields/operators/calls or unsupported values stop explicitly. The generated source and report mark Python parse, execution and input/output tests `NOT_RUN`; `COMPUTED` refers to generation.

Seven new scoped tests and six existing regressions passed. The final new CLI pass uses 46 stores and 50 execute requests. An initial test expected the wrong decoded-duplicate-key code; its failed run and state are retained, and the corrected expectation passed without a product-source change. Separate development QC parses nine actual generated sources and checks 20 finite cases: 19 matching outputs and one expected unbound-name `NameError`. This QC is not a product host runner, an execution grant, an independent held-out quality population or full B07 acceptance. Complete inherited reader/interpreter semantics, free-form creation/analysis/repair and broader source coverage remain required and `UNVERIFIED`.

The v50 follow-up [code-context explanation route](bb-content-code-context-explanation-route-validation.json) consumes a prior generated function's same-case report, explains its retained IR and carries both the source result and report SHA into the content plan. The v52 replay in [its current route record](bb-content-code-context-explanation-v52-route-validation.json) also checks no-resend file recovery. This explanation is limited to code generated by the existing structured IR contract.

The v51 [generated-code verification route](bb-content-generated-code-verification-route-validation.json) connects that producer to a bounded consumer. The v52 replay in [the current verification record](bb-content-generated-code-verification-v52-route-validation.json) repeats the same-case checks against the current parser and confirms that recovery reproduces the verification report before confirming an existing local write without resending. An ordinary immediate follow-up accepts user-authored finite cases, reloads and revalidates the committed generation report and source hash, and runs the stored generated source through the restricted Rust AST evaluator. This is an evaluation of a restricted Python subset, not host-Python execution or proof of arbitrary program correctness; generator Python parse, execution and input/output test fields remain `NOT_RUN`. Code revision, broader syntax, independent held-out quality and full B07 acceptance remain `UNVERIFIED`.

The v52 [search-candidate body route](bb-source-search-candidate-body-route-validation.json) connects a live ranked Wikipedia listing to the actual raw wikitext body of one user-selected candidate, using a separately repeated exact-host grant. The retained observation is revalidated from its immutable listing before the candidate URI is derived; its body packet, source bytes, result history and a later source-summary consumer are linked. This proves only the current Wikipedia/raw-wikitext path. Provider breadth, arbitrary HTML/PDF reading, source comparison and full BB-SOURCE/C01–C13 acceptance remain `BUILD_GAP` / `UNVERIFIED`.

## B07 bounded source reading and native evaluation continuation

[b07-bounded-code-capability-route-validation.json](b07-bounded-code-capability-route-validation.json) records the next B07 slice. The ordinary input route now recognizes Japanese and ASCII-case-insensitive English `code-read:`, `code-evaluate:` and `code-verify:` headers. The Rust parser validates the entire source as one top-level function before reading or evaluating it. The finite subset covers scalar/container expressions, simple assignments, branches, loops, loop control, returns, chained comparisons and allowlisted builtins. Unsupported Python syntax, imports, attributes, indirect calls and out-of-envelope inputs suspend.

Reading returns AST counts/depth, selected statement/call/comparison rows and UTF-8 source spans. Evaluation uses only the native bounded Rust interpreter and reports `COMPUTED`, `FAILED` or `STOPPED`; verification evaluates only the caller's 1–32 declared cases and distinguishes booleans from integers. Reports are stored as authorized artifacts and flow through the normal Core compute/content/render graph and saved input/version history. The step limit is 1–200,000 (default 50,000); source is at most 32,768 UTF-8 bytes. The evaluator observes the request deadline and step limit, but does not poll an in-progress cancellation request.

Six new component tests and five normal-CLI route tests passed. A development-only comparison matched eleven finite native results/failure kinds against the frozen Python evaluator and matched eleven AST counts, depths, structure rows and source spans against CPython 3.11 AST. Format, workspace/all-target check, Clippy with warnings denied and workspace build passed. Full workspace runtime tests remain `NOT_RUN`. These authored observations do not establish general Python compatibility, all-input correctness, free-form code quality, static name/type analysis or full B07 acceptance; B07 remains `UNVERIFIED` and `NOT_RELEASED`.

## B08 explicit-source writing composition route

[b08-explicit-text-composition-route-validation.json](b08-explicit-text-composition-route-validation.json) records one bounded composition slice. The normal input route accepts Japanese `文章を構成して:` and case-insensitive English `text-compose:` headers, seals the exact JSON body, and Core runs a strict bounded compositor over explicitly selected material byte ranges. Fixed prefixes and line separators come only from the declared layout role. Required/companion units and all condition/caveat units are included; omitted optional units are listed. Every output byte maps either to a decoded source-material range plus its containing JSON string token, or to the exact unit directive that supplied a generated layout token.

Three focused component/parser tests and one ordinary CLI test passed; an existing exact-replacement regression passed too. The CLI cases exercised both language headers, durable artifact references and source/output maps, plus a typed `TEXT_COMPOSITION_CHAR_LIMIT` suspension without a dependency. Workspace/all-target check and Clippy with warnings denied passed; the full workspace runtime suite was not run. This only composes caller-supplied fragments. Free-form drafting, editing/rewrite, summaries, purpose and style fidelity and the twelve previously unseen B08 tasks remain unimplemented or unverified; B08 remains `UNVERIFIED` and `NOT_RELEASED`.

## B08 protected import and editing continuation

[b08-protected-text-editing-route-validation.json](b08-protected-text-editing-route-validation.json) records explicit text import and atomic editing of actual case-scoped import/composition/editing artifacts. Each base is regenerated from its original request and bounded ancestry and compared with its complete canonical bytes. Expected text digests and original ranges are checked before all edits; protected quotations, conditions and explicit locks are retained with shifted byte positions. Unchanged output maps to the real parent artifact, and replacement output maps to the current request. Consumed ancestry hashes flow through Core's computation/content/render records.

Five component tests, one input test and three ordinary CLI tests passed. The CLI cases include actual composition-to-edit and edit-to-edit consumers, Unicode/source maps, typed atomic/schema/resource stops, case/actor isolation, explicit input version history, a confirmed local file receipt and an exact retry without another write. The initial Clippy run found a redundant clone in a test assertion; the assertion was corrected and the retained warning log is distinct from the passing run. The two existing composition/literal-replacement CLI regressions, formatting, workspace/all-target check, Clippy with warnings denied and build passed. Full workspace runtime testing is `NOT_RUN`; general correction discovery, rewritten meaning/style/summary quality and the twelve previously unseen B08 tasks remain `UNVERIFIED`.

## B08 unique literal edit-location proposal

[b08-unique-literal-edit-location-route-validation.json](b08-unique-literal-edit-location-route-validation.json) records a normal CLI operation that resolves one caller-supplied exact literal against a hash-verified, replayed text artifact. The report retains UTF-8 byte coordinates, actual base and ancestor hashes, and the search/replacement JSON token spans. It rejects missing, repeated (including overlapping) and protected matches. The report includes a ready-to-submit atomic edit request; it does not change the document. The existing editor rechecks the proposal if the caller submits it.

Three location component tests, one header/parser test and two normal CLI tests passed. The CLI run observed one Unicode location proposal followed by its explicit application, plus typed missing/ambiguous/protected and evidence-budget suspensions with no adopted dependencies. Eight text-editing component tests, two header/parser tests, three existing editing-route tests, two location-route tests and the composition/literal-replacement regressions passed in the scoped regression run. Formatting, workspace/all-target check, Clippy with warnings denied and workspace build passed. Full workspace runtime tests and all B08 requirements remain `UNVERIFIED`.

## B08 extractive summary candidate route

[b08-extractive-summary-candidate-route-validation.json](b08-extractive-summary-candidate-route-validation.json) records a bounded normal input/Core operation for explicit source text. Japanese `文章を要約して:` and case-insensitive `text-summary:` accept a strict `{text,max_sentences}` body. The deterministic frequency/position/length ranking retains exact sentence bytes, selected and omitted indices, score fractions, the source JSON token range and UTF-8 source/output offsets. The user-visible result explicitly says semantic coverage is unverified, and Core records `HYPOTHESIS`.

Three focused component tests, one input-header test and one ordinary CLI test passed. The CLI test exercised both headers, checked the persisted source artifact and output mappings, and confirmed that an omitted sentence does not become an implicit completeness claim. Workspace formatting, all-target check, Clippy with warnings denied and build passed. Full workspace runtime tests and all B08 requirements remain `UNVERIFIED`; this route does not verify task purpose, meaning, conditions, negation, audience, style or summary completeness.

## B08 extractive candidate character budget

[b08-extractive-summary-char-budget-route-validation.json](b08-extractive-summary-char-budget-route-validation.json) records the R2-derived output-length constraint added to the existing candidate route. Optional `max_chars` limits the combined Unicode-scalar count of copied sentence text; the fixed disclosure label, bullet markers and separators are excluded. A sentence that does not fit is omitted intact and recorded as length-limited; ranking continues to lower-scored sentences that fit. If no complete sentence fits, Core returns `TEXT_SUMMARY_CHAR_LIMIT` without adopting an artifact. The versioned `TEXT_EXTRACTIVE_SUMMARY_CANDIDATES_V2` artifact records the effective limit and selected character count.

Four summary component tests, one header/parser test and two normal-CLI tests passed. The CLI tests inspect the persisted V2 artifacts and typed no-fit stop. One composition CLI regression, eight text-editing component tests and three editing-route CLI regressions passed. Formatting, locked all-target check, warning-free Clippy and locked workspace build passed. Full workspace runtime tests, the twelve unseen B08 tasks and semantic coverage remain `NOT_RUN`/`UNVERIFIED`.

## B08 extractive summary output format

[b08-extractive-summary-format-route-validation.json](b08-extractive-summary-format-route-validation.json) records the explicit `bullets` (default) and `paragraph` layouts on the ordinary summary-candidate route. Both layouts retain complete source sentences, the V3 artifact's format identity and exact output byte ranges. Unsupported and null formats suspend with `TEXT_SUMMARY_SCHEMA` and produce no artifact. The `max_chars` budget continues to count copied sentence characters only, excluding the disclosure label and layout markers/separators.

Five summary component tests, one header/parser test, three normal-CLI summary tests, one composition CLI regression, eight text-editing component tests and three editing-route CLI regressions passed. Formatting, workspace/all-target check, Clippy with warnings denied and workspace build were checked. Full workspace runtime tests, the twelve unseen B08 tasks, independent semantic summary quality and Windows/8 GB acceptance remain `NOT_RUN`; B08 remains `UNVERIFIED`.

## B08 extractive summary required source sentences

[b08-extractive-summary-required-content-route-validation.json](b08-extractive-summary-required-content-route-validation.json) records the optional zero-based `required_sentence_indices` constraint. The request names source candidates produced by the existing bounded splitter; required candidates are reserved first, then ranked candidates fill remaining sentence and character budget, with final output restored to source order. Out-of-range, count-conflicting and character-conflicting requirements suspend without an artifact. The effective sorted indices are recorded in the V4 artifact with exact source/output byte maps. This preserves user-designated source sentences but does not infer which content is semantically required or verify the resulting summary.

Seven summary component tests, one header/parser test, four normal-CLI summary tests, one composition CLI regression, eight text-editing component tests and three editing-route CLI regressions passed. Formatting, workspace/all-target check, Clippy with warnings denied and workspace build are recorded separately. Full workspace runtime tests, the twelve unseen B08 tasks, semantic summary quality and Windows/8 GB acceptance remain `NOT_RUN`; B08 remains `UNVERIFIED`.


## Current BODY-1 bounded math/data route replay (2026-11-13)

The [current route replay record](bb-math-data-b03-b06-current-route-replay.json) binds seven selected B03 normal-CLI producer cases for exact descriptive statistics, finite-weight normalization, expectation/variance, finite-event probability, polynomial evaluation, linear/quadratic equations and a linear system, plus five B06 calculation-sequence tests. The B06 routes cover exact prior-step consumption, typed partial stops, saved-result explanation/version history and one explicitly authorized local-file receipt with unchanged retry. The B03 math context replay also covers same-case recomputation and explanation for eight exact report types. These bounded routes remain implementation evidence; B03, B06, B02/B04 broader acceptance, ordinary open-domain language use and full successor acceptance remain `UNVERIFIED`.


## B03 exact math report context follow-up (2026-11-13)

The ordinary same-case `詳しく説明して` consumer now covers nine retained exact B03 report types: descriptive statistics, normalized finite weights, finite expectation/variance, finite-event probability, polynomial evaluation, linear systems, linear equations, quadratic equations and symbolic simplify/derivative/antiderivative reports. Core identifies the exact report schema, recomputes it from retained inputs, and compares the complete saved JSON before producing a bounded explanation with the prior-result dependency. Lists, polynomial terms, symbolic trace steps and row operations are capped at 16 displayed items; polynomial/equation text is capped at 512 Unicode characters, and matrix proof previews at four rows. The full exact report remains in prior history. Tampered recognized report data stops with `MATH_CONTEXT_INVALID`. Recovery revalidates statistics, probability-weight, quadratic-equation and symbolic context before confirming an already-written file without resending it. The [B03 math context replay](bb-context-b03-math-reports-followup-v52-route-validation.json) records focused unit, normal-CLI and file-recovery evidence. The existing CSV source-cell follow-up and B06 sequence regressions also passed, along with formatting, targeted all-target check and CLI build. Full workspace tests, Clippy, radical/complex equation context follow-up and broad B03/B06 acceptance were not run; those acceptance states remain `UNVERIFIED`.

## JSON table grouped-statistics context consumer (2026-11-13)

The [JSON grouped-statistics context route](bb-math-data-json-group-statistics-context-route-validation.json) connects the ordinary JSON string-table statistics producer to the next same-case `詳しく説明して` consumer. Core retains the exact source bytes and typed report as separate immutable artifacts, recomputes grouped statistics from that source, compares the full canonical report and verifies every header/key/value source-range digest before explaining exact group statistics and bounded original-cell previews. The execution graph records the report consuming the retained source. The no-resend recovery path reconstructs that dependency graph and verifies the same report/explanation before confirming an existing local file receipt.

Three selected normal-CLI JSON grouped-statistics tests passed, including the new producer-to-explanation route; the authorized file recovery completed with `LOCAL_FILE_RECONCILED_NO_RESEND` and unchanged file identity. The CSV source-cell explanation, B03 description-statistics context and five B06 sequence regressions also passed. Formatting, targeted all-target checking and CLI build passed. Full workspace tests, Clippy, other JSON/CSV report consumers and broad B02/B03/B04/BB-CONTEXT acceptance remain `NOT_RUN`/`UNVERIFIED`.

## Natural source question over a prior fetched body (2026-11-13)

The ordinary Japanese/English topic question (`資料では「光合成」について何と述べていますか` / `What does the source say about photosynthesis?`) now consumes the exact committed same-case fetched-body result. Core verifies the retained packet bytes and hashes, selects up to five sentences by exact English-token or Japanese-substring overlap, and records each candidate's URL, packet/body identities, extracted-text byte span and span digest in a typed answer artifact and dependency graph. It labels the result `HYPOTHESIS`: the passages are source-text matches, not a verified semantic answer, completeness claim or source-trust assessment. A topic with no match suspends. The normal-route test writes only under an exact local path grant and observes the file receipt.

The bounded consumer tests use a committed synthetic `FETCHED_BODY` fixture through the ordinary CLI; the existing separate live candidate-body route remains the source producer evidence. Three parser/lexical component tests, one recovery revalidation test, two normal-route CLI tests and the crash-injected recovery replay passed. Recovery recomputed the same answer artifact and dependency list before reconciling the existing exact-grant local file receipt as `LOCAL_FILE_RECONCILED_NO_RESEND`; file inode, mtime and size remained unchanged. Provider breadth, semantic source QA, full C01-C13, full workspace tests and Clippy remain `NOT_RUN`/`UNVERIFIED`. See [the route record](bb-lang-source-question-followup-route-validation.json).

## Natural highest-mean question over prior JSON grouped statistics (2026-11-13)

The ordinary Japanese/English question `どのグループの平均値が一番高いですか` / `Which group has the highest mean?` now resolves against the actual committed same-case JSON grouped-statistics result. Core revalidates the immutable source and report, recomputes every group statistic, compares exact rational means, lists all compared groups, and retains all tied highest groups with original winner-cell source spans. A unit mismatch suspends; it is not converted or guessed. The answer artifact is a computed-value graph consumer of the prior report and source. The local output file is verified from its exact grant, and recovery rechecks the same answer artifact before confirming the existing write without resending it. See [the route record](bb-lang-json-group-mean-query-route-validation.json).

The question-parser test, exact-rational tie test and three normal-route CLI tests passed. The CLI cases cover a unique winner, tied winners, mixed units, missing prior result and an unrelated prior calculation. The separate fresh-process CLI route verified source/report/answer hashes, the producer-to-consumer dependency graph and confirmed local-file recovery with unchanged inode/mtime/size. The initial unsupported-wh parse and three assertion/checker mismatches are retained with their corrections in the route record. This is one bounded natural question over an actual prior math/data result; open-domain Q&A, cross-group unit conversion, the wider B04 population and full A07/A13 acceptance remain `UNVERIFIED`. Full workspace runtime tests and Clippy were not run for this BODY-1 unit.

## BB-SOURCE multi-candidate same-case continuation (2026-11-13)

Candidate body-fetch results now retain the exact validated `SEARCH_LIST` packet and earlier successful `FETCHED_BODY` packets from that same search case. The next rank selection revalidates the saved `SearchObservation`, checks each retained packet against its stored bytes and selected candidate URI, then appends the newly fetched body. The goal graph records the listing artifact as the prerequisite consumed by the body node. A maximum of 16 body packets is enforced before another body read. Distinct observation timestamps are retained as distinct packet artifacts even when their stable source packet IDs are the same.

The selected source-question consumer already accepts all retained body packets. Its ordinary CLI test now confirms two fixture packets reach one answer artifact in order, with each packet reference, mode, source URI, raw/extracted hash and UTF-8 span preserved. Eleven focused search tests and three source-question CLI tests passed; formatting and locked CLI all-target checking passed. These are offline fixture/component observations. The Core's repeated LIVE candidate-fetch path was not run in this batch, so actual multi-candidate provider continuity remains `NOT_RUN`; this does not establish semantic source synthesis, C01-C13 acceptance or overall successor acceptance. See [the multi-candidate continuation route record](bb-source-multi-candidate-continuation-route-validation.json).

## BB-SOURCE exact-URI body-version continuity (2026-11-13)

New source-question artifacts use `SOURCE_QUESTION_VERSIONED_EXTRACTIVE_CANDIDATES_V2`. Core groups intact body packets by exact recorded `final_uri` and selects the uniquely latest observed raw-body/extracted-text/parser identity for lexical matching. Older distinct versions remain named by hash and packet reference in the answer artifact and remain source dependencies; repeated observations of an identical body/parser version share one active source version. Conflicting versions at the latest timestamp and mixed `LIVE`/`FIXTURE` inputs suspend. The output stays `HYPOTHESIS`; URL grouping does not assert publisher independence or semantic support. V1 artifacts remain replayable by local-file recovery through their original derivation operation.

The normal CLI observed that an updated same-URI fixture contributes only the latest sentence while retaining both packet references and the old version hash in the graph and answer history. Same-time conflict, mixed-mode and repeated-identical-body rules passed focused engine tests. Nine engine source-question tests and four normal-route CLI tests passed; exact-file crash recovery confirmed the existing V2 output without resending. Formatting, locked CLI all-target check, package Clippy with warnings denied and CLI build passed. Repeated LIVE fetch, semantic source comparison, full C03/C10/C13, full workspace tests and complete successor acceptance remain `NOT_RUN`/`UNVERIFIED`. See [the route record](bb-source-body-version-invalidation-route-validation.json).

## BB-LANG natural known-premise questions (2026-11-13)

At the v55 initial route, ordinary Japanese `Xとは何ですか` / `Xって何` and English `What is X?` / `What are X?` questions reached the actor-scoped `USER_PREMISE` memory consumer. That implementation returned active direct premises only, retained premise IDs and polarity, and stated that outside-world truth was not checked. An absent subject suspended with `MISSING_PREMISES`; numeric expressions and unresolved pronouns did not enter the memory lookup. The parser identity advanced from v54 to v55 because the one-time interpretation changed.

The normal CLI route observed Japanese and English queries against separately supplied premises, exact dependency selection, the truthful premise disclosure and a typed missing-premise stop. Focused parser and CLI route checks passed. That evidence describes the v55 implementation at the time and does not validate the later derived-relation inventory below. General open-domain definitions, what/why/how coverage and full A07/A08/A12/A13 acceptance remain `BUILD_GAP` / `UNVERIFIED`. See [the natural premise question route record](bb-lang-known-premise-natural-question-route-validation.json).

## BB-LANG bounded derived-relation inventory (2026-11-13)

The existing ordinary named-subject question still seals as `InspectPremises`; Core now builds finite query shapes from that actor's active relation facts and valid rule conclusions, then asks the existing proof engine within the request's action budget. Its output identifies direct premises, computed/source records and rule-derived conclusions separately. Derived rows retain their result ID and transitive premise/rule dependency IDs. A contradiction is shown as competing support with both sides retained, never resolved by choosing one. Missing support, excessive candidates, proof-step exhaustion and output-byte exhaustion use typed stops. The answer states that it enumerates saved relations only and does not establish external-world truth or exhaustiveness. Reconciliation of v68 file outputs recomputes the same bounded inventory lineage and guarantee before it can confirm an earlier effect; older parser records retain their direct-premise recovery check.

Validation on 2026-11-13 passed `cargo fmt --all -- --check`, `cargo check --workspace --all-targets --locked`, `cargo clippy --workspace --all-targets --locked -- -D warnings`, `cargo build --workspace --locked`, and `cargo test --workspace --all-features --locked --quiet` (350 passed; 0 failed across 10 + 131 + 4 + 205 tests). The fresh-process route record captures 14 request observations plus a file delivery and reconciliation. It verifies Japanese and English memory queries, a six-ID direct/derived contradiction with transitive rule lineage, rule-result invalidation, actor isolation, candidate-budget and missing-premise stops, and recovery of a DERIVED contradictory listing with identical dependencies, `CONFIRMED` / `LOCAL_FILE_RECONCILED_NO_RESEND`, and unchanged file inode/mtime/size. See [the bounded derived-relation inventory route evidence](bb-lang-derived-relation-inventory-20261113-route-validation.json). This is evidence for one bounded memory-inventory route only; general open-domain definitions, unrestricted what/why/how answers and full A07/A08/A12/A13 acceptance remain `BUILD_GAP` / `UNVERIFIED`.

## BB-CONTEXT bounded natural premise correction (2026-11-13)

The v56 ordinary Japanese/English grammar accepts one explicit copular correction (`ココは哺乳類ではなく鳥です` / `Coco is a bird, not a mammal`) with a named subject, one superseded positive relation and one distinct replacement. Core loads only the current actor's memory and matches the exact relation and scope against active direct `USER_PREMISE` facts. A match is invalidated without erasing its record; Core stores the negative superseded claim and positive replacement together in one actor-memory CAS revision. A missing target or scope mismatch yields `MISSING_CORRECTION_TARGET` before any memory save. Both new claims disclose that external truth was not checked. Previous case results remain immutable history; a new query recomputes against the revised memory.

The normal-route regression verifies Japanese and English correction, same-actor/scope matching, unrelated and other-scope premise retention, post-correction natural listing and question behavior, missing/wrong-scope/other-actor stops, and preservation of the earlier derived `ResultRecord`. The first CLI run exposed a real `REVISION_CONFLICT` because multiple in-memory relation operations were counted as separate durable revisions; Core now commits that compound change as one revision and the same route test passes. One input-parser unit test, one natural-correction CLI test, premise-listing and definition-question regressions, the declared-rule/counterexample regression, the 16-observation fresh-process route, all-target CLI check, warning-free CLI Clippy and CLI build passed. The helper confirms the exact stored memory facts and prior case result. The complete workspace runtime suite and persisted descendant-fact invalidation beyond the exercised query-time path were `NOT_RUN`. General co-reference, open-world factual learning and full A07/A08/CAP-D04/CAP-D07/CAP-D11/CAP-D13/BB-LEARN acceptance remain `BUILD_GAP` / `UNVERIFIED`. See [the correction route record](bb-context-natural-premise-correction-route-validation.json).

## BB-CONTEXT prior-result-anchored premise correction (2026-11-13)

The v57 input grammar accepts the bounded singular forms `それは哺乳類ではなく鳥です` and `It is a bird, not a mammal.` as `CorrectPreviousPremise`. Core resolves the omitted subject only from active direct `USER_PREMISE` facts whose IDs occur in the selected, committed, same-case prior `ResultRecord.dependencies`. It requires exactly one unique subject/predicate/object/scope proposition. An explicit `[scope=...]` narrows the candidates; without that qualifier, multiple scopes or subjects suspend with `AMBIGUOUS_CORRECTION_CONTEXT`. No prior committed result suspends with `MISSING_CORRECTION_CONTEXT`, and a derived-only answer cannot authorize correction of its underlying support. A successful correction retains the old record, adds the negative old claim and positive replacement, and persists them in one actor-memory CAS revision.

The first scoped route test mixed the English stored object `mammal` with the Japanese correction object `哺乳類`; exact proposition matching correctly stopped. The fixture was corrected to use Japanese on both turns, and the same route then passed without changing product matching behavior. The parser unit test, named-subject correction regression, contextual normal-CLI route, formatting check, CLI all-target check, Clippy with warnings denied, CLI build, premise-listing fresh-process route, named-correction fresh-process route and 21-observation contextual fresh-process route passed. The contextual route verifies direct-result anchoring, absent cross-case context, ambiguous-scope stops, explicit scope correction, derived-only stops, one memory revision per accepted correction, actor memory contents and immutable previous answer history. Full workspace runtime tests, general co-reference, plural reference, empirical learning and full A07/A08/CAP-D04/CAP-D07/CAP-D11/CAP-D13/BB-CONTEXT/BB-LEARN acceptance were `NOT_RUN` / remain `UNVERIFIED`. See [the prior-result correction route record](bb-context-prior-result-correction-route-validation.json).

## BB-LANG two-clause relation question (2026-11-13)

The v59 ordinary parser accepts exactly two copular relation clauses joined by Japanese かつ / 且つ or English and, with one named subject and a shared scope. AskConjunction retains both propositions; Core runs both against the same actor memory under a shared finite inference budget. A complete answer distinguishes supported clauses from directly refuted clauses and unions their exact premise/rule dependencies. Missing or conflicting evidence suspends. Alternative questions, more than two clauses, different subjects and unresolved pronouns stay outside this route.

Focused parser and normal-CLI checks passed for Japanese and English, proof lineage, one explicitly negated clause, one refuted clause, missing and conflicting evidence, wrong scope and unsupported forms. The fresh-process route records 17 observations. Full workspace tests, open-domain Q&A, general writing, unrestricted composition and full A02/A04/A05/A09/B01/A15 acceptance remain NOT_RUN / UNVERIFIED. See [the v59 conjunctive relation-question route evidence](bb-lang-conjunctive-relation-question-v59-route-validation.json).


## BB-LANG two-premise relation rule (2026-11-13)

The v60 ordinary Japanese/English conditional grammar accepts exactly two explicitly joined copular antecedents with one subject and scope. Core stores both antecedents in the actor-scoped rule record, and the ordinary bounded relation query joins independently supplied facts for a new subject. Derived answers and AskWhy retain both direct premise IDs and the rule ID. Single-premise rule identities and persisted records remain compatible; multi-premise rules are not flattened into proof-composition shortcuts.

The 26-observation fresh-process CLI route verified Japanese and English registration, missing and wrong-scope stops, contradictory antecedent rejection, proof restoration after counterexample invalidation, removal after either premise is invalidated, immutable earlier answers, and both-premise AskWhy output, including shared-variable A(x) AND B(x,y) => C(y) binding the queried conclusion subject. Focused parser, relation and normal-CLI regressions, formatting, workspace all-target check, Clippy with warnings denied and locked workspace build passed. The full workspace runtime suite, open-domain Q&A/writing and complete A02/A04/A05/A09/B01/A15 acceptance remain NOT_RUN / UNVERIFIED. Multi-premise proof shortcut composition and broader shared-variable joins remain outside this batch. See [the v60 two-premise relation-rule route evidence](bb-lang-conjunctive-relation-rule-v60-route-validation.json).


## BB-LANG two-premise proof composition (2026-11-13)

Proof-preserving composition now accepts either a two-premise rule followed by a one-premise rule, or a one-premise rule followed by a two-premise rule, when unification leaves at most two distinct, bound, same-scope antecedents. The stored shortcut retains every antecedent and both parent rule IDs. Two two-premise parents are not flattened into a three-premise record; ordinary query closure still uses the original rules. The existing one-premise shortcut identity remains stable.

The 19-observation fresh-process CLI route exercised both composition directions in Japanese and English, new-subject answers, AskWhy with both direct facts, root-rule invalidation, fact invalidation, and immutable earlier results. Relation/input tests, normal CLI regressions, formatting, workspace all-target check, warnings-denied Clippy and locked build passed. The full workspace runtime suite, three-premise shortcuts, open-domain Q&A/writing and complete A02/A04/A05/A09/B01/A15 acceptance remain NOT_RUN / UNVERIFIED. See [the v61 proof-composition route evidence](bb-lang-conjunctive-relation-composition-v61-route-validation.json).

## BB-LANG three-antecedent proof composition (2026-11-13)

Composition between two explicitly two-premise scoped root rules now creates an internal shortcut with at most three antecedents. Query joins each additional premise in order, charges each newly matched prefix to the finite relation-step budget, and derives only after all premises match in the same scope. The shortcut retains both parent rule IDs and all direct premise dependencies; AskWhy renders all three premises. Root-rule invalidation and direct-premise invalidation both make new queries return `UNKNOWN`/`MISSING_PREMISES`, while earlier committed result records remain unchanged. User-authored rule inputs remain capped at two explicit premises, and single-/two-premise composition IDs remain compatible.

The ten-observation fresh-process Japanese CLI route consumed the three-premise shortcut for a new subject and verified all three fact IDs, both parent IDs, the shortcut ID, full AskWhy text, premise invalidation, and immutable result replay. Focused relation tests passed (15), input tests passed (22), both existing normal-route CLI regressions passed, and formatting, workspace all-target check, warnings-denied Clippy, and locked workspace build passed. Full workspace runtime tests, open-domain Q&A/writing, and complete A02/A04/A05/A09/B01/A15 acceptance remain NOT_RUN / UNVERIFIED. See [the v62 three-antecedent composition route evidence](bb-lang-conjunctive-relation-ternary-composition-v62-route-validation.json).

## BB-CONTEXT / BB-CONTENT exact edit of previous output (2026-11-13)

Parser v63 accepts one bounded Japanese or English request to replace an exact literal in the previous answer. Core resolves the selected same-case committed result, checks its retained ContentPlan identity and actual render bytes against the stored artifacts, and suspends when either source is unavailable or the literal is missing or nonunique (including overlaps). A new `PREVIOUS_OUTPUT_REPLACE_EXACT_V1` artifact records the parent result, content-plan/render references, hashes, and exact source/output spans; the earlier result is immutable. Output carries `USER_PREMISE` and states that semantic and factual correctness were not checked. Recovery recomputes the parent/result/edit relationship and reconciles the original local file without resending.

Two natural previous-output CLI tests passed for Japanese/English chaining, dependency graph lineage, a confirmed file receipt, and typed missing/no-match/ambiguous stops. One document component test and the full prior-output recovery script passed, along with four source-question CLI regressions, three existing text-editing CLI regressions, formatting, workspace all-target check, warnings-denied Clippy, and locked workspace build. The engine library suite ran 199 tests: 198 passed; `input::symbolic_and_clause_tests::unsupported_clause_qualifiers_remain_unresolved` failed on the `もしXは鳥かつ青ならXは動物` expectation. The failure's baseline status was not independently established. The complete workspace runtime suite was not run. Full F04/B08 acceptance, including the twelve-task writing population and semantic editing quality, remains `UNVERIFIED`. See [the previous-output edit route record](bb-context-previous-output-literal-edit-v63-route-validation.json).

## BB-LANG two/three-clause relation question (2026-11-13)

The v61 ordinary Japanese/English parser now accepts two or three relation clauses joined by a single homogeneous `かつ`, `且つ`, or `and` form. The first clause supplies one explicit named subject; supported shorthand clauses inherit that subject and all clauses share the sealed scope. The existing `AskConjunction` typed meaning object carries the propositions to Core. Core checks every clause against the same actor memory, divides the finite action budget across clauses, returns the combined support/refutation summary and exact union of premise/rule dependencies, and stops on incomplete or conflicting evidence. Four clauses, mixed connector forms, alternatives, different subjects and unresolved pronouns stay outside this bounded grammar.

Focused input and normal-CLI tests passed for Japanese and English three-clause queries, direct and derived premise lineage, an explicitly negated clause, third-clause conflict, missing evidence, wrong scope and unsupported forms. The fresh-process CLI route records 38 observations (25 `COMMIT`, 13 `SUSPEND`). Formatting, workspace all-target check, warnings-denied Clippy and locked workspace build passed. Full workspace runtime tests, open-domain Q&A/writing and complete A02/A04/A05/A09/B01/A15 acceptance remain `NOT_RUN` / `UNVERIFIED`. See [the v63 three-clause relation-question route evidence](bb-lang-conjunctive-relation-three-clause-v63-route-validation.json).

## BB-LANG conjunctive-rule shorthand expectation reconciliation (2026-11-13)

The parser behavior at base `701e37e` already supported the bounded shorthand second clause introduced with the v60 two-premise rule parser (`52406f55e00bc3e747f43a5bd7abb64233c6da10`). In `もしXは鳥かつ青ならXは動物`, the parser inherits the explicit first clause's subject and scope and seals the second premise as `Xは青`. The earlier unit and CLI negative cases incorrectly classified this supported two-premise form as unresolved. Before correction, both focused tests failed; the normal CLI output was `COMMIT` with `USER_PREMISE` and the two-premise rule. The change corrects those expectations, adds Japanese/English parser assertions, clarifies the parser comment and README, and keeps disjunction as a typed unsupported stop. No production inference behavior changed.

After correction, formatting passed, all 199 engine library tests passed, the ordinary CLI two-premise proof/AskWhy route passed, and the negative compound-condition/effect-stop regression passed. The fresh-process CLI observation in [the v64 shorthand route record](bb-lang-conjunctive-relation-rule-shorthand-v64-route-validation.json) records 26 requests (20 `COMMIT`, 6 `SUSPEND`). It starts from the exact Japanese shorthand input, then consumes the two declared user premises for a new subject; the derived answer and AskWhy retain both fact IDs and the rule ID. Missing second premise, wrong scope, contradictory premise and invalidation stop as expected; the old committed result remains unchanged. The rule/result are explicitly limited to user-provided premises and do not claim outside-world truth. Full workspace runtime tests and broad/open-domain language acceptance remain `NOT_RUN` / `UNVERIFIED`; A02/A04/A05/A09/B01/A15 acceptance statuses remain `UNVERIFIED`.

## BB-LANG Japanese transitive object questions (2026-11-13)

The v64 input grammar now represents Japanese `食べる` and `使う` positive/negative present and polite forms as typed subject-predicate-object propositions. Ordinary questions such as `ミナは何を使いますか` seal as `AskRelationObjects`. Core enumerates bounded candidate objects from the same actor's retained facts/rules in the exact scope, checks each requested polarity through the existing relation proof engine, and renders only supported values with the union of premise/rule dependency IDs. Each result is labeled as a direct premise or rule-derived conclusion. The answer states that its listed values are not exhaustive and that outside-world truth was not checked. Missing subjects/evidence, wrong scope, actor isolation, contradictory evidence and unsupported past tense stop without a factual answer.

The 14-observation fresh-process CLI route in [the v65 transitive object-question record](bb-lang-transitive-object-question-v65-route-validation.json) records 9 `COMMIT` and 5 `SUSPEND` requests. It verifies a direct `使う` premise plus a variable-bound rule and derived object, a negative-polarity query, same-scope stops, actor isolation, conflict handling, invalidation and immutable replay of the earlier answer. The parser unit and normal CLI route passed, along with the existing definition and conjunctive-rule CLI regressions, workspace formatting, all-target check, warnings-denied Clippy and locked build. The full workspace runtime suite, open-domain Q&A and independent unseen-question QA population were not run; A02/A04/A05/A09/A12/A13/A15/B01 acceptance remains `UNVERIFIED`.

## BB-LANG Japanese transitive yes/no questions (2026-11-13)

The v65 input grammar now seals bounded present/polite Japanese transitive `使う` yes/no forms ending in `か` or `?` as the existing typed `Ask` relation query, preserving the named subject, object, polarity and scope. Core reuses its ordinary actor-scoped proof path: a supported claim returns its direct premise or derived proof; a refuted claim returns the supported opposite premise; missing evidence or simultaneous positive/negative support suspends with a typed reason. This route adds no verb vocabulary and makes no outside-world truth claim. Past tense remains unresolved.

The fresh-process ordinary-CLI route in [the v66 transitive yes/no record](bb-lang-transitive-yes-no-question-v66-route-validation.json) records 18 requests (12 `COMMIT`, 6 `SUSPEND`). It verifies direct and rule-derived support with proof dependencies, negative support and refutation, exact-scope/missing-subject/actor stops, contradiction, invalidation, immutable prior-answer replay, question punctuation and unsupported past tense. The focused parser and CLI route tests, object-question/definition/conjunctive-rule regressions, formatting, workspace all-target check, warnings-denied Clippy and locked build passed. Full workspace runtime tests, independent 24-question QA, open-domain Q&A and complete A02/A04/A05/A09/A12/A13/A15/B01 acceptance were not run; those acceptance states remain `UNVERIFIED`.

## BB-LANG bounded natural comparison of two subjects' supported relations (2026-11-13)

The baseline saved separate subject premises but suspended the ordinary Japanese question `ココとミケの共通点は何ですか` as `CAPABILITY_UNAVAILABLE`. Parser v67 now seals that form and the bounded English `What do Coco and Mike have in common?` as `AskCommonRelations` with two distinct explicit subjects and one exact scope. Core collects candidate relation shapes from active actor-scoped facts for those subjects and applicable valid rules, then reuses the relation proof engine for each subject under a divided finite action budget. It reports only relations independently supported for both, preserves each side's direct/derived proof lineage and dependencies, and labels the output non-exhaustive without claiming outside-world truth. A missing proof is not treated as a difference or as proof that no commonality exists; conflicts suspend.

The fresh-process ordinary-CLI route in [the v67 common-relation record](bb-lang-common-relation-question-v67-route-validation.json) covers 25 requests (21 `COMMIT`, 4 `SUSPEND`). It verifies Japanese and English entry, one-sided-relation exclusion, direct and rule-derived shared claims, shared negative claims, missing evidence, wrong scope, conflict and actor-memory isolation. The focused input and CLI tests passed, along with the existing explicit-meaning-comparison, conjunctive-relation and transitive-object regressions, formatting, workspace all-target check, warnings-denied Clippy and locked build. Full workspace runtime tests, open-domain semantic comparison and complete A02/A04/A05/A09/A12/A13/A15/B01 acceptance were not run; those statuses remain `UNVERIFIED`.

The baseline `詳しく説明して` path committed the same common-relation report text again without expanding its evidence. Core now saves a canonical actor/case-bound report artifact tied to the comparison result ID, with each shared proposition pair, both guarantees and each side's exact proof dependencies. The existing `ExplainPrevious` consumer loads that artifact only from the same case, checks its canonical form and result identity, and re-runs each side through the relation proof engine. It confirms the guarantees and dependency sets still match before presenting separate left/right proof lineage; invalidated or otherwise changed support suspends as `CONTEXT_PROOF_CHANGED`. The expanded answer states that unlisted relations do not establish absence and retains the original non-exhaustive and no-outside-world-truth limits.

The fresh-process route in [the common-relation explanation record](bb-context-common-relation-explanation-v68-route-validation.json) records 15 requests (13 `COMMIT`, 2 `SUSPEND`). It verifies direct and derived left/right proof lineage across process restarts, prior-result and typed-artifact dependencies, a distinct explanation response, a typed stop after premise invalidation, and missing-prior suspension. The focused normal-CLI route test passed. Formatting, workspace all-target check, warnings-denied Clippy and locked build are recorded in that route evidence. The full workspace runtime suite and complete A07/A08/A12/A13/A14/A15/B01 acceptance were not run; those remain `UNVERIFIED`.

## BB-CONTEXT elliptical why follow-up over a prior result (2026-11-13)

The baseline accepted a Japanese transitive assertion and yes/no question, but `なぜですか` and `どうして？` then suspended as `CAPABILITY_UNAVAILABLE`. The v66 input parser now recognizes bounded elliptical Japanese `なぜですか` / `どうして？` and English `Why?` as the existing `ExplainPrevious` intent. Core consumes only the immediately previous committed result in the same actor/case. For a relation answer, it parses the exact prior conclusion, reruns the relation proof against current actor memory and confirms the dependency list and guarantee before explaining the premise/rule lineage. For a calculation result, it returns the retained expression and steps. No prior committed result suspends; cross-actor access is rejected by the case boundary.

The 14-observation fresh-process ordinary-CLI route in [the v66 contextual why record](bb-lang-contextual-why-v66-route-validation.json) records 12 `COMMIT`, 1 `SUSPEND` and 1 `REJECT`. It verifies direct and derived transitive conclusions, a yes/no query refuted by a supported negative premise, prior-result dependency links, exact calculation reuse, missing-case stops and actor isolation. The focused parser and CLI tests passed, as did the explicit why, relation explanation after restart, and singular-pronoun context regressions, formatting, workspace all-target check, warnings-denied Clippy and locked build. The full workspace runtime suite, broad conversation/reference QA and complete A04/A07/A08/A12/A13/A14/A15/B01 acceptance were not run; those states remain `UNVERIFIED`.

## BB-CONTENT / BB-CONTEXT previous-result extractive summary (2026-11-13)

Before this batch, a natural request such as `前回の比較を要約して` stopped with `CAPABILITY_UNAVAILABLE` after an ordinary same-case comparison. The v68 route now accepts bounded Japanese and English previous-result summary requests. It selects only the exact immediate committed same-case `ResultRecord`, verifies the stored `ContentPlan` identity, result text and actual render against the case-bound artifacts, then runs the existing deterministic extractive sentence selector over that exact displayed text. The typed candidate report links the summary result ID, source result ID, source ContentPlan ID, render hash, source-text hash, selected source byte spans and exact output-unit hash. Dependencies include that report, source result, original content/render artifacts and the source result's dependencies. The result stays `HYPOTHESIS`; extraction does not verify meaning, purpose, conditions, negation or coverage.

The following `詳しく説明して` consumer revalidates the exact summary artifact, source result, ContentPlan, render and selected sentence spans before explaining the candidate. Recovery reconstructs the same context and graph. Its focused test accepts the original report and rejects a changed source result. A source scan above the original request's action budget stops before storing a summary. The 18-operation fresh-process route in [the v68 previous-result summary record](bb-content-previous-result-summary-v68-route-validation.json) records 16 `COMMIT` and two `SUSPEND`; it covers Japanese and English summary requests, direct and derived comparison inputs, premise/rule dependency preservation, action-budget and cross-case stops, exact output-file receipt, simulated interrupted write reconciliation without resending, retained history and source-span explanation. The focused input, recovery and normal-CLI tests passed, as did the existing fetched-source summary regression. Formatting, workspace all-target check, warnings-denied Clippy and locked workspace build passed. The full workspace runtime suite and broad A07/A08/B08/E01/E02/F05 acceptance were not run; those remain `UNVERIFIED`.


## BB-LANG proof-backed explanation through native surface ranking (2026-11-13)

The bounded ordinary Japanese/English explicit-relation `AskWhy` route now sends a user-facing result through the existing non-neural `SequenceModel`. Core first recomputes the actor-scoped relation proof and builds exact explanation units. It admits two surfaces whose only difference is the transition sentence; each surface preserves the same conclusion, direct premise, any derived fact and applied rule, plus the truth-boundary disclaimer. The model scores the finite authorized set, Output renders its selected text, and Core checks the required content units. This is a real model effect on a bounded response, not free-form generation or general causal understanding.

Recovery now recomputes the current proof dependencies and guarantee, reconstructs the Core explanation units and requires the stored candidate set and selected text to match. The fresh-process ordinary-CLI route in [the native surface-selection record](bb-lang-proof-backed-native-surface-selection-route-validation.json) records 15 observations (13 `COMMIT`, 2 `SUSPEND`) across Japanese, English, refuted-premise and missing/conflict cases. It observes two authorized candidates and a finite exact sequence probability; the adopted file is 807 bytes with SHA-256 `c6a67e4ca450b919df68ad1673a682c7e0136bfb6f9cc10751dcc5479da250d3`, and its local recovery receipt is `CONFIRMED / LOCAL_FILE_RECONCILED_NO_RESEND` with unchanged dependencies. The focused surface unit test, normal-CLI test, formatting, workspace all-target check, warnings-denied Clippy, locked build, fresh-process route and prior-answer stale-proof regression passed. The full workspace all-features runtime suite and unrestricted causal why/how, open-domain writing, and complete A01/A02/A07/A13 acceptance were not run; those acceptance states remain `UNVERIFIED`.

## BB-CONTENT / BB-CONTEXT short profile from saved relations (2026-11-13)

Parser v69 recognizes a bounded Japanese or English request for a profile from saved information and captures its explicit subject string as `DraftRelationProfile`. Core builds a finite inventory from the current actor's supported relation proofs, writes a separate line for each direct or derived relation with scope and record/dependency IDs, and retains both sides of a conflict. The result is `HYPOTHESIS`; it discloses that outside-world truth and record completeness have not been checked. Two Core-authored output surfaces differ only in transition wording, which the existing native `SequenceModel` ranks. The following ordinary previous-result summary can consume the committed profile result and retain its identity as a dependency.

Recovery recomputes the current relation inventory, required units, dependencies and approved candidate set before confirming a local file effect. The first real interrupted-file attempt exposed that recovery rejected the new `HYPOTHESIS` intent; `DraftRelationProfile` is now in the bounded recovery allowlist, and the fresh-process retry passed without rewriting the file. The route in [the v69 profile evidence](bb-content-known-relation-profile-v69-route-validation.json) records nine observations (eight `COMMIT`, one `SUSPEND`), including direct/derived lineage, a confirmed local file receipt, interrupted-write reconciliation without resend, a following summary, actor isolation and retained conflicting supports. The two focused engine tests, profile normal-CLI route test, formatting, workspace all-target check, warnings-denied Clippy, locked build and independent fresh-process route passed. Full workspace runtime tests, broad profile quality, unrestricted writing and complete A01/A02/A07/A08/A13/B08 acceptance were not run; acceptance remains `UNVERIFIED`.

## BB-RENDER authenticated loopback HTTP consumer for a saved-relation profile (2026-11-13)

The ordinary loopback HTTP `/v1/execute` route already accepts a complete `RequestEnvelope`, authenticates its bearer token, checks that the request actor matches the configured server actor and calls the shared `Engine::execute`. The new ordinary HTTP observation sends a `DraftRelationProfile` request using facts saved through separate CLI processes, returns the same bounded `HYPOTHESIS` profile with its proof dependencies and authorized model surfaces, rejects an incorrect bearer token with HTTP 401, then hands the committed HTTP result to the existing CLI previous-result summary consumer in the same case. No second answer-producing engine was added.

The focused HTTP normal-route test and independent local-process observer passed. The observer records both CLI preconditions, the 401 negative case, the authenticated HTTP result and a CLI summary that depends on the exact HTTP result ID. It binds a test-only token in process memory, binds the server to `127.0.0.1`, and stops that server before exit. Workspace formatting, all-target checking, warnings-denied Clippy and locked build passed. Public HTTP exposure, browser display, streaming, full workspace runtime tests, and complete A01/A02/A07/A08/A13/B08/R10 acceptance remain `NOT_RUN` / `UNVERIFIED`. See [the v69 HTTP consumer route evidence](bb-content-known-relation-profile-v69-http-consumer-route-validation.json).

## BB-RENDER loopback browser UI for a saved-relation profile (2026-11-13)

The Rust `serve` process now serves an independent browser workspace from exact static paths under `rust/apps/cli/web/`; frozen predecessor assets are untouched. The page is same-origin only, has no external assets, uses a restrictive CSP and security headers, and sends the form's full `RequestEnvelope` to the existing authenticated `POST /v1/execute` endpoint. It retains no token in browser storage and uses `textContent` for Core output. The rendered result shows lifecycle, guarantee, text, reason, result and the returned dependency IDs.

The scoped fresh-process browser observation seeds one direct relation and one rule through ordinary CLI processes, starts a loopback-only API with an in-memory test token, and submits a profile through Chromium. `POST /v1/execute/stream` requires the versioned `minidora-stream/v1` media contract. It flushes a `provisional` event with no answer or adoption authority before calling the same synchronous Engine; a committed result follows as `final`, while `SUSPEND` and `CANCEL` first emit `invalidated` and then `final`. The browser checks version, stream identity, event order, lifecycle agreement and the final input identity before displaying Core's result. The page shows the committed `HYPOTHESIS` text and exact premise/rule dependencies; a no-support request visibly invalidates its provisional state and displays only the typed `SUSPEND`. Its separate case-cancel button sends Core's ordinary `キャンセル` input through that same API. Core records `CANCEL` and preserves the committed profile in prior case history; already committed effects remain intact. Disconnecting a stream is not Core cancellation, and no undo behavior is claimed. The focused fixed-asset/security-header and stream normal-route tests, workspace format check, all-target check, warnings-denied Clippy and locked build are recorded with the [browser route evidence](bb-content-known-relation-profile-v69-browser-ui-route-validation.json). Reversible undo, public exposure, broader browser/accessibility compatibility, complete B08/general writing quality, the full workspace runtime suite, and complete A01/A02/A07/A08/A13/B08 acceptance remain `NOT_RUN` / `UNVERIFIED`.

## BB-RECOVER / QC2-01 durable-store concurrency and stale stage names (2026-11-13)

Review of blob publication found that stage names combine the process ID with a process-local counter, but the counter resets after restart. A stale stage file left by abrupt termination could therefore block a later publisher if the OS reused that process ID. Publication now retries a new counter value on `AlreadyExists`, keeps the orphan untouched, and reports serial exhaustion as `STORE_TEMP_SERIAL_EXHAUSTED`. A deterministic test seeds a stale stage filename and confirms the next exclusive stage is created without changing the orphan. A separate-process race against the same case revision confirms SQLite serialization plus revision CAS allow one commit and reject the other writer with `REVISION_CONFLICT`.

The effect-ledger probe also found that reusing an existing idempotency key under a different action ID did not return the intended typed collision. `save_action` now checks key ownership while holding the SQLite immediate transaction, inserts a new identity only for a new action, and reports `ACTION_IDENTITY_COLLISION`; exact saved-action replay remains idempotent. Startup now verifies that every action record's idempotency key matches its `action_identity` row, and triggers prevent normal SQL updates or deletion. The guards are installed only after a valid alpha 8 schema is accepted; alpha 7 opens still stop with `SCHEMA_MISMATCH` without adding them, and explicit alpha 7 migration adds them after the schema transition. A recovery test removes the update trigger to simulate tampering, changes the stored key, then confirms reopening stops with `INTEGRITY_FAILURE`.

The minidora-engine library suite passed (222 tests). Its separate-process cases race a same-revision append (one `OK`, one `REVISION_CONFLICT`), identical input registration (`NEW` plus `REPLAY`), different bodies on one input identity (one retained `INPUT_IDENTITY_COLLISION`), two actors claiming one case (one `NEW`, one `ACCESS_DENIED`), identical action replay (both calls succeed with one saved action), and conflicting action IDs sharing one key (one `OK`, one `ACTION_IDENTITY_COLLISION`). Two immediate process exits also passed: after blob publication before any DB reference, restart finds one orphan and no case; after the durable `ATTEMPTED` action append, restart reports `UNKNOWN` with the same action and idempotency identity. Existing recovery checks cover fail-closed missing/corrupt referenced blobs and actor scoping. Workspace formatting, locked all-target checking and warnings-denied Clippy passed. See [the scoped store concurrency and recovery record](bb-store-multiprocess-cas-stage-collision-20261113-validation.json). This does not exercise process exit at every WAL/file-sync boundary, physical power loss, larger concurrency populations, external once-only delivery, multiple hosts or other operating systems; the full workspace runtime suite and the complete BB-RECOVER/QC2-01 population remain `NOT_RUN` / `UNVERIFIED`.

## QC2-02 pairwise process race for local file delivery (2026-11-13)

Two independent processes now race `output::write_file` against one temporary absolute target using distinct action IDs and different complete byte strings. One obtains `CONFIRMED`; the other gets typed `EXISTING_FILE_CONFLICT`. Reopening the target finds exactly one writer's complete bytes, never an overwrite or mixed payload. The test runs the actual output adapter in separate processes with a synchronized start barrier; it does not include Core authorization, persisted action history or a real external effect port. See [the local write race evidence](bb-local-write-multiprocess-conflict-20261113-validation.json). Hostile ancestor replacement, termination during write/fsync, physical power loss, other operating systems, remote delivery, full workspace runtime tests and complete QC2-02 acceptance remain `NOT_RUN` / `UNVERIFIED`.

## BODY-1 BB-LANG regular Japanese transitive forms (2026-11-13)

The v70 input seal keeps the existing `eat`/`use` predicate identities, and the same ordinary parser now preserves other explicitly marked Japanese subject–`を`–predicate relations as their own predicate identities. It normalizes the regular plain forms covered by the finite grammar and matching negative forms, then routes assertions, conditional rules, object questions and yes/no questions through the existing Core relation store and proof engine. Core output distinguishes direct premises from rule-derived results and retains the proof dependency IDs. The object-list evaluator is shared with local-write recovery: it recomputes the exact supported text, guarantee and dependency IDs before allowing the stored bytes to be reconciled. No object is supplied by a test fixture or answer table.

The fresh-process ordinary CLI route in [the v70 validation record](bb-lang-generic-transitive-v70-validation.json) contains 22 observations (15 `COMMIT`, 7 `SUSPEND`). It records `ミナは水を飲む`, a declared rule deriving `ミナは液体を飲む`, positive and negative object questions, a yes/no proof, the earlier `食べる`/`使う` regressions, actor/scope/conflict checks, and typed stops for the unnormalized polite form `飲みます` and the known intransitive object question `何を死ぬ`. The recovery test writes an authorized local answer, accepts its unchanged proof, then invalidates the supporting premise and confirms the shared recovery inspector rejects the prior proof while leaving the actual file bytes unchanged. Engine library tests passed 225/225; the focused regular-verb parser, object-answer recovery and ordinary CLI tests passed. Formatting, all-target locked checking, warnings-denied Clippy and locked build passed. This does not establish general Japanese parsing, lexical transitivity/valency, ichidan or past/potential conjugations, general question quality, or the independent QA population; full workspace runtime acceptance remains `NOT_RUN` / `UNVERIFIED`.

## BB-CONTEXT same-case JSON table transform explanation (2026-11-13)

The ordinary `詳しく説明して` follow-up now handles a committed JSON string-table filter, stable sort, named-column selection or ordered projection. Core loads the exact immutable input bundle by actor, case, input ID and version; verifies the registered request identity, raw-text digest, bundle fingerprint and sealed meaning; then recomputes the typed table report from that bundle's exact intent parameters and document. It accepts the explanation only when the original result's sole report dependency, canonical report artifact and pretty report text match. The explanation shows bounded decision/output rows with original JSON byte ranges, source-cell hashes, source-bundle hash and report hash. Its result dependencies bind the prior result, the exact sealed input bundle and report artifact.

The ordinary CLI route passed all four operations and verified the dependency order and source-range disclosure. The local-file recovery route revalidates the same sealed input/report lineage and exact explanation before observing the already-written file; it confirms the original receipt without resending or changing file bytes or modification time. Focused store retrieval, recovery and normal-route CLI tests passed. Formatting, workspace all-target locked check, warnings-denied workspace Clippy, workspace build and `git diff --check` passed. The full workspace runtime suite was not run. This is a bounded JSON string-table producer-to-consumer route; joins, comparisons, broader document operations, semantic QA, and full A07/A14/B04/E08 acceptance remain `UNVERIFIED`. See [the route evidence](bb-context-json-table-transform-explanation-v71-route-validation.json).

## BODY-1 BB-LANG saved-relation comparison to same-case summary (2026-11-13)

Parser v71 seals bounded Japanese and English requests comparing two explicit subjects' saved relations, with the optional exact `[scope=...]` prefix retained. Core builds a scope-filtered relation inventory for each subject from the same actor's active facts and valid rules under a divided finite action budget. It reports exact signed relations supported on both sides and separately labels one-sided saved support; it does not infer that an unlisted relation is false or absent. Direct/derived labels, contested-record markers, fact/rule dependencies and both scoped inventories are stored in a canonical comparison artifact. Opposing records for one subject remain contested; that relation is not presented as an unambiguous common or one-sided result. Missing support on both sides stops with `MISSING_PREMISES`; another actor's memory is not used. Comparison output remains `HYPOTHESIS` because saved memory completeness and outside-world truth are not established.

The new normal-route test exercised ordinary CLI processes through direct and rule-derived evidence, one-sided records, requested-scope filtering, durable artifact identity and a same-case previous-result summary consuming the comparison result with its original dependencies. A separate case verified conflicting positive/negative records are preserved and not classified as an unambiguous match; empty memory stops with a typed reason. The separate-process observer in [the v71 saved-relation comparison record](bb-lang-saved-relation-comparison-v71-route-validation.json) records the direct, derived, summary, conflict and empty-memory paths. The parser unit and normal-route tests, existing common-relation regression, formatting, locked workspace all-target check, and warnings-denied workspace Clippy passed. Initial test-only assertions were corrected to match the observed scoped inventory size and exact conflict wording. Full workspace runtime tests, broader open-domain comparison, unseen language/writing QA and complete A02/A09/B01/B08 acceptance remain `NOT_RUN` / `UNVERIFIED`.

## BODY-1 BB-CONTEXT saved-relation comparison explanation consumer (2026-11-13)

The ordinary same-case `詳しく説明して` follow-up now consumes the v71 saved-relation comparison. Core fetches the canonical comparison artifact from the same actor and case, verifies its result identity, report hash, adopted ContentPlan and actual rendered bytes, then recomputes both scoped relation inventories from current actor memory. The full inventories must match, including their supported facts, contested IDs, dependencies and guarantees; Core then rebuilds the report and checks its exact text and prior dependency order before explaining the common, one-sided and contested classifications. The response retains the comparison result plus every report, premise and rule dependency and remains `HYPOTHESIS`. If either inventory changes, the follow-up suspends with `CONTEXT_PROOF_CHANGED`; missing or mismatched result/output artifacts also stop without asserting world truth or absence.

The focused CLI route verifies direct and rule-derived roots, each side's lineage, exact prior-result dependencies, a distinct explanation output and a typed stop after a premise is invalidated in another same-actor case. The fresh-process observer in [the v72 explanation record](bb-context-saved-relation-comparison-explanation-v72-route-validation.json) records 20 observations (18 `COMMIT`, 2 `SUSPEND`) and also replays the v71 comparison, same-case summary, contested comparison and empty-memory stop. The explanation route, comparison-to-summary route, common-relation regression, format check, locked workspace all-target check, warnings-denied Clippy and locked CLI build passed. Full workspace runtime tests, broad relation explanation, general language/writing quality and complete A07/A09/B01/F04 acceptance remain `NOT_RUN` / `UNVERIFIED`.

## BODY-1 BB-CONTEXT same-case JSON inner-join explanation consumer (2026-11-13)

The ordinary `詳しく説明して` follow-up now consumes a committed strict JSON string-table inner join. It retrieves the exact retained input bundle by actor, case, input ID and version, recomputes the join from the sealed left/right tables and key columns, and accepts it only when the typed canonical report artifact and prior result text match. It also revalidates the prior ContentPlan identity/text/guarantee, RenderObservation, their stored artifact bytes and the actual render hash. The bounded explanation identifies the exact unique-key condition, maps each shown output row to its left and right source rows and source-cell byte ranges, and lists unmatched keys with their original byte ranges. The prior result, input bundle and report remain in the Core dependency graph. Duplicate keys and mismatched or unavailable lineage remain typed stops.

The ordinary CLI route verifies matching rows, one unmatched row on each side, the original result reference, exact three-item dependency order, and rendered explanation. Local-file recovery revalidates the same sealed input and report before confirming the already-written bytes; the test checks that bytes and modification time are unchanged and that the write is not resent. The existing filter/sort/select/project explanation routes remain covered. Focused CLI and recovery tests, formatting, all-target locked checking, warnings-denied Clippy and locked workspace build passed. One first Clippy run caught the larger join report enum variant; boxing that report removed the warning. The first route run also caught that the preexisting consumer gate admitted only `COMPUTED`; it now admits `SOURCE_REPORT` only for the sealed inner-join operation. Full workspace runtime tests and complete B04/A07/A14/E08 acceptance remain `NOT_RUN` / `UNVERIFIED`. See [the v73 route record](bb-context-json-table-join-explanation-v73-route-validation.json).

## BODY-1 BB-CONTEXT same-case JSON table comparison explanation consumer (2026-11-13)

The ordinary same-case `詳しく説明して` consumer now handles a committed `JSON_TABLE_COMPARE` report. It recognizes the exact comparison report shape and guarantee, reloads the actor/case/input/version sealed bundle, recomputes the strict keyed string-table comparison and requires the canonical report bytes, original result text, report dependency, ContentPlan identity and RenderObservation bytes to match. For each of up to eight reported changes it shows the exact key/column, decoded values, JSON Pointer and original byte span/hash on each applicable side; added and removed rows retain their one-sided row pointer and range. It reports equality and change counts as exact-string comparisons and makes no claim about semantic equivalence or factual truth. Core links the prior result, sealed bundle and report in its existing execution graph; historical graph node IDs stay unchanged so an already pending local-file recovery can use the same identity. Missing or changed input/report/render lineage and evidence-budget overflow remain typed stops.

The normal-route test covers a changed cell, one row present only on each side, escaped and decoded Unicode values, both source hashes, exact dependencies and the rendered graph; it also covers an equal table. The recovery test writes the actual explanation, then reconciles the pending effect while checking that the existing target bytes and modification time remain unchanged and the action is confirmed without a second write. Seven `json_table_` normal-route tests and the comparison/filter/join recovery test passed. `cargo fmt --all -- --check`, workspace all-target locked `cargo check`, warnings-denied workspace Clippy, locked workspace build and `git diff --check` passed. Full workspace runtime tests were `NOT_RUN`; B04, A07, A14 and E08 remain `UNVERIFIED` beyond this bounded JSON-table comparison route. See [the v74 route record](bb-context-json-table-comparison-explanation-v74-route-validation.json).

## BODY-1 BB-CONTEXT same-case CSV table comparison explanation consumer (2026-11-13)

The ordinary same-case `詳しく説明して` consumer now handles a committed `CSV_TABLE_COMPARE` report. It loads the exact actor/case/input/version sealed bundle, requires its `CsvCompare` intent, recomputes `csv_compare_by`, and verifies the sole report dependency, canonical report bytes, prior result text, source/output hashes, ContentPlan identity and actual RenderObservation bytes. It validates original CSV byte slices and output JSON string tokens before explaining up to eight changed cells or added/removed rows. Changed cells carry both source rows and cell ranges; one-sided rows show their keyed original cells. Every shown raw range carries its SHA-256. The report retains all changes. Equality is exact decoded-string equality only; numeric, semantic and factual equivalence are not inferred.

Core links the prior result, sealed input bundle and report through `previous-csv-table-comparison-input` and `previous-csv-table-comparison-report` graph nodes. The local-file recovery path replays the same lineage and confirms an already-written explanation without resending it. The normal-route case checks changed cells, removed/added rows, per-range hashes, three-item dependencies, graph edges and actual render hash, and also checks an equal-table explanation. The recovery regression confirms the existing bytes and modification time remain unchanged. The CSV explanation route, JSON comparison explanation regressions, CSV comparison producer route, and combined structured-table no-resend recovery test passed. Workspace formatting, all-target locked check, warnings-denied Clippy, locked build and `git diff --check` passed. Full workspace runtime tests remain `NOT_RUN`; B04, A07, A14 and E08 remain `UNVERIFIED` beyond these bounded routes. See [the v75 route record](bb-context-csv-table-comparison-explanation-v75-route-validation.json).

## BODY-1 Core typed pronoun relation follow-up and recovery (2026-11-14)

This scoped route verifies that the proposition resolved by `AskPreviousRelation` remains available to the next same-case `ExplainPrevious` consumer as typed Core data. A supported Japanese singular-pronoun relation query commits a versioned `minidora.core-computation.relation-meaning.v1` artifact containing the resolved proposition and exact relation-proof dependency IDs. The observer checks the artifact's content-addressed SHA-256, proposition, units and dependency agreement, then sends `詳しく説明して` in a fresh CLI process and checks the prior result, proof IDs, conclusion and applied rule in the rendered output. A direct negative premise exercises the contradicted-query branch; the retained typed meaning is the supported opposite and the following explanation preserves that lineage.

The route changes the same actor's proof in another case, then confirms that the original case's explanation suspends with `CONTEXT_PROOF_CHANGED` and has no `ContentPlan`. It also performs one exact-authorized temporary file write, simulates the interrupted state in the isolated test database after the write, and runs Core reconciliation. Recovery returns `CONFIRMED` / `LOCAL_FILE_RECONCILED_NO_RESEND` with the same dependencies; the existing file's inode, modification time and size remain unchanged. The isolated engine unit test also feeds `previous_relation_meaning` an unversioned legacy computation artifact and confirms that the old conclusion text and dependency IDs remain readable.

The fresh-process route records 34 request and reconciliation observations. Its focused legacy compatibility test passed (1/1); Python syntax, formatting, workspace/all-target locked check, Clippy with warnings denied, workspace locked build and `git diff --check` passed. The route and its six producer/consumer/recovery checks are recorded in [the v1 relation follow-up evidence](bb-context-pronoun-relation-followup-v1-route-validation.json). No production Rust behavior changed in this batch; the engine change is a test-only compatibility regression. This provides scoped observations for A04, A07, A08, A15, E08, E12, CAP-D04 and CAP-D11 while their requirement-level acceptance remains `UNVERIFIED`. Full workspace runtime tests, broad co-reference, all old-store histories and full requirement acceptance remain `NOT_RUN` / `UNVERIFIED`.

## BODY-1 Core contextual relation assertion from a typed prior result (2026-11-14)

The v72 parser preserves a singular-pronoun relation assertion as `AssertPreviousRelation`, with its unresolved source subject and any explicit scope still typed in the sealed input. Core accepts it only when the immediately preceding same-case result is committed and the sealed context reference matches that result. It reloads the result's typed relation meaning, recomputes the current actor-scoped proof and requires the proof dependencies and guarantee to match. On success, Core restores the typed subject and inherited or explicit scope, stores a `USER_PREMISE`, and records both the prior-result ID and new fact ID. A following ordinary relation question consumes the new fact ID alone.

The 49-observation fresh-process route checks this producer/consumer chain and the persisted unresolved input meaning. Missing and cross-case context suspend without a `ContentPlan`; a changed proof suspends before adoption; an unrelated actor is rejected at the case-access boundary. The focused input test checks singular, plural, Japanese negative and explicitly scoped English forms. The same route replays the preceding typed explanation, stale-proof, legacy compatibility and no-resend file recovery regressions. The [v1 contextual assertion evidence](bb-context-pronoun-relation-assertion-v1-route-validation.json) records the source hashes and observations.

This remains one immediate singular-pronoun relation assertion path. General co-reference, plural and multi-clause assertions, broad topic changes, complete old-store histories and the full A04/A07/A08/A15/E08/E12/CAP-D04/CAP-D11 acceptance remain `NOT_RUN` / `UNVERIFIED`. The corresponding eight requirement rows in [requirements.tsv](requirements.tsv) retain `UNVERIFIED` acceptance status.

## BODY-1 Core exact fact continuity across one chained contextual relation assertion (2026-11-14)

Core can consume one immediately preceding `AssertPreviousRelation` as the context for another singular-pronoun relation assertion. It loads that exact sealed input by actor/case/input/version, verifies the prior result identity and dependency pair, and selects the one direct `USER_PREMISE` fact whose source is the prior result ID. Core rejects an unresolved pronoun still present in the stored proposition, requires the fact to remain active, and reruns the current relation proof with exactly that fact ID and `USER_PREMISE` guarantee before restoring subject/scope. The second assertion is stored as a new direct fact with the prior result and new fact IDs; the next `AskPreviousRelation` consumes the new fact ID alone.

The 13-observation fresh-process route checks one two-assertion chain and the following query. A correction from another case invalidates the first fact in the actor's shared memory; the original case's next contextual assertion then suspends with `CONTEXT_PROOF_CHANGED`, and a separate query confirms that the unaccepted second premise was not stored. The [v1 chain route record](bb-context-pronoun-relation-assertion-chain-v1-route-validation.json) preserves input, dependency, memory and source-hash evidence. The prior Batch 06 route was rerun after this Core change and is retained separately in the [Batch 06 regression replay](bb-context-pronoun-relation-followup-v1-regression-after-assertion-chain.json); the original Batch 06 evidence remains unchanged.

This only covers one immediate assertion after another and one subsequent query. Arbitrary chains, non-immediate discourse references, plural/multi-clause subjects and general co-reference remain outside the route. A04/A07/A08/A15/CAP-D04/CAP-D11 acceptance remains `UNVERIFIED`.

## BODY-1 Core one-hop contextual relation assertion depth boundary (2026-11-14)

A review after Batch 07 found that its implementation revalidated each immediately preceding contextual assertion but did not enforce the report's one-hop boundary. On the published `c820916a78f3564f2fec518cb2b2f68787426204` binary, six fresh CLI requests committed a third consecutive singular-pronoun assertion and stored its `USER_PREMISE` fact. The pre-correction `SCOPE_BOUNDARY_MISMATCH` record is preserved in [the depth-limit pre-fix route evidence](bb-context-pronoun-relation-assertion-depth-pre-fix-validation.json).

Core now checks the sealed prior assertion's exact context result against actor memory. If that context result itself produced a direct `USER_PREMISE` fact, the current request is beyond the one allowed inherited assertion and suspends as `CONTEXT_CHAIN_DEPTH_LIMIT` before `assert_fact` or memory persistence. A stale source fact still stops first as `CONTEXT_PROOF_CHANGED`. The input grammar, parser version, stored schema and the existing two-assertion behavior are unchanged.

The 20-observation fresh-process route covers the allowed two-assertion chain and its next question, stale-proof denial, and the third-assertion boundary. It verifies the blocked fact is absent both from actor memory and a separate query. The earlier 49-observation Batch 06 route was replayed with this correction; its original Batch 06 and Batch 07 evidence remain unchanged. See [the depth-limit route record](bb-context-pronoun-relation-assertion-depth-v1-route-validation.json) and [the Batch 06 regression replay](bb-context-pronoun-relation-followup-v1-regression-after-depth-limit.json).

This is a bounded Core context guard, not general co-reference, long-chain memory, natural-language quality or requirement acceptance. A04/A07/A08/A15/CAP-D04/CAP-D11 remain `UNVERIFIED`.

The full workspace runtime command `cargo test --workspace --locked` exited 101: 10 CLI unit tests passed, while `normal_route` had 134 passed and 7 failed; later workspace test targets were not run. Each failing test was rerun with only `core.rs` restored byte-for-byte to baseline SHA `4d6804f964ac2bef0f4ec42a999589f4ca175de91d9e8553a1ffc1e6c682473d`, and all seven reproduced their same failure signatures. Six are existing assertions for scope-less `適用規則:` text while the current formatter includes scope; the seventh is the existing CSV follow-up being rejected by JSON-table operation dispatch. These failures were not changed in this bounded unit. See [the workspace failure comparison](bb-workspace-test-full-20261114-failure-baseline-comparison.json). This does not pass the full runtime suite; A04/A07/A08/A15/CAP-D04/CAP-D11 remain `UNVERIFIED`.

## BODY-1 CSV statistics follow-up dispatch and current scoped relation explanation tests (2026-11-14)

The B08 workspace run had exposed a CSV statistics explanation that was routed through the JSON-table result-shape detector. `previous_json_table_operation_explanation` now confirms that the retained sealed input intent is one of the supported JSON-table operations before recomputing it. A `CsvStatistics` result returns to its own existing revalidation consumer, preserving the source/report explanation path.

The six rule-explanation test cases that failed on B08 now check the formatter's explicit `default` or `biology` scope. Once those earlier assertions were corrected, `natural_why_question_explains_only_current_relation_proofs` also reached a stale refuted-answer phrase; that assertion now checks the current bounded output wording and retains the negative premise/dependency checks. Product relation wording was not changed.

All seven formerly failing B08 tests pass individually. The full `cargo test --workspace --locked` run passes: CLI unit tests 10, normal-route integration tests 141, domain tests 4 and engine tests 230; documentation tests completed with zero cases. `cargo fmt --all -- --check`, workspace all-target locked check, warnings-denied Clippy and locked build also pass. The earlier [B08 baseline failure comparison](bb-workspace-test-full-20261114-failure-baseline-comparison.json) remains unchanged as the before-fix record. E08 and all other full requirement acceptance remain `UNVERIFIED`.

## BODY-1 Core typed counterfactual relation construction observation (2026-11-14)

The previous direct Core observation confirmed conjunctive rules and explicit stops, but showed that the current relation query excluded `HYPOTHESIS` facts. Per the adopted BUILD process, implementation returned to Core construction rather than treating a typed stop or a separate actor fixture as a counterfactual success. `Engine::evaluate_counterfactual_relation` now accepts bounded ground `Proposition` assumptions, an actor-owned case, a target and exact `superseded_fact_ids`. It clones actor memory, permits superseding only active direct `USER_PREMISE` opposites explicitly named by the caller, invalidates their descendants only in the clone, and adds deterministic scenario-only `HYPOTHESIS` facts. The relation closure admits only this call's hypothesis IDs and carries `HYPOTHESIS` through dependent conclusions. It returns a typed status, guarantee, proof dependencies, hypothesis IDs, superseded/invalidated IDs and base/scenario revisions. The method reads but does not persist memory, execute effects or adopt content. Normal `relations::query` continues to exclude hypotheses.

The bounded `Engine::execute` fixture covers a two-premise rule, a supported derived conclusion, a direct contradiction stop, missing conditions, another actor's separate premises, and the unsupported natural-language counterfactual request (`CAPABILITY_UNAVAILABLE`, no `ContentPlan`). A second actor/case supplies `sensor` and `calibrated`, with an independent `blue` fact. Explicitly replacing only the calibrated fact by typed `not calibrated` yields a `SUPPORTED / HYPOTHESIS` `not reliable` proof carrying the rule, sensor and hypothesis IDs. Only the named fact is invalidated in the scenario. Unrelated blue remains supported, before/after stored `MemoryState` values are identical, and an ordinary follow-up stays `MISSING_PREMISES`. Leaving the opposing premise active yields typed `CONFLICT`; selecting an unrelated fact as an override yields `COUNTERFACTUAL_OVERRIDE_INVALID`.

The [direct Core observation evidence](bb-core-construction-observation-02-v1-validation.json) contains all 19 sealed `ResultRecord`s, direct typed API reports, memory snapshots and source/runtime hashes. The existing [B08 assertion-depth replay](bb-context-pronoun-relation-assertion-depth-v1-regression-after-b11.json) passed 20 observations; the [Batch 06 follow-up replay](bb-context-pronoun-relation-followup-v1-regression-after-b11.json) passed 49. The engine suite passed 231 unit tests plus the new integration test; formatting, workspace all-target check, warnings-denied Clippy, locked build and `git diff --check` passed.

This is one direct typed-Core construction observation only. It does not add a normal natural-language counterfactual route, establish broad causal or analogical reasoning, or complete the seven observations/full-system acceptance. B11 and all 93 capability acceptance statuses remain `UNVERIFIED`; full workspace runtime tests and the other six construction observations are `NOT_RUN`.

## B12 — bounded direct Core meaning composition observation (2026-11-14)

The existing finite-surface ranker could not satisfy the adopted construction instruction: it ranked complete Core-authored response candidates and did not build a sequence from meaning roles. `Engine::compose_typed_meaning` now accepts explicit typed semantic units for explanation, comparison and short composition, with purpose, neutral/concise style, paragraph/bullet format, evidence labels, dependencies, prior sequence and byte/token budgets. Core enforces role coverage and order, preserves each supplied unit once, and chooses among admissible unit/connector segments with a deterministic constrained decoder.

The composer uses the existing character n-gram family with a new support-aware exact conditional distribution. For each trie node, it backs off to the longest context with an observed transition among currently admissible tokens, then normalizes exact rational probabilities over that finite support. The earlier generic backoff could stop at a context with transitions but none of the allowed candidate tokens, making the choice uniform; B12 corrected that behavior. A declared one-sentence boundary grammar requires one terminating sentence mark at each unit's end; role order, connectors and output format are separately constrained. Reports record the purpose/style/format state hash, training corpus and model revisions, transform/version parameters, model counts, per-step candidate support and probability, grammar/objective, decoder, tie rule, lossless token adapter and `Guarantee::Unknown`. Per-request model fitting consumes only the supplied typed unit text and versioned role/style connective lexicon; there is no cross-request learning.

The [B12 direct-Core validation record](bb-core-construction-observation-03-v1-validation.json) contains three purpose observations (explanation, comparison and short composition), a concise-style explanation follow-up, a changed comparison input, exact rerun evidence and absent case state before/after. The explanation chose evidence and rule order from admissible same-rank units; concise style changed that order. The short composition honored its caller-supplied prior prefix and chose between independent remaining body units. Changing a comparison value changed both its context hash and model revision. Missing semantic roles and an undersized output budget returned typed failures. Supplied evidence IDs and propositions remain unverified; no natural-language parser, normal `Engine::execute` route, `ContentPlan` adoption, rendering or persistence is claimed.

`cargo test -p minidora-engine --locked` passed 232 unit tests, the B11 integration observation and two B12 integration tests. The focused support-backoff unit test observed exact normalized probabilities `3/5` and `2/5` when the longest history had no admissible transitions. Workspace all-target check, warnings-denied Clippy, locked workspace build, formatting and `git diff --check` passed. Full workspace runtime tests and all acceptance evaluations remain `NOT_RUN`; A01, A03, R2-17 and all 93 acceptance rows remain `UNVERIFIED`. No release was created.
## B13 — bounded direct Core calculation/relation version observation (2026-11-14)

The dedicated `Engine::execute` observation records a same-input-ID calculation correction from version 1 (`8+5 = 13`) to version 2 (`8+3 = 11`), verifies the sealed v2 context and dependency point to the v1 result, and confirms the v1 `ResultRecord` remains in `prior_results`. `Why?` then consumes the saved v2 calculation. A separate actual two-premise relation result is consumed by a same-input-ID v2 pronoun assertion; Core revalidates the previous proof before saving a resolved `Pico is monitored` USER_PREMISE, and a subsequent query consumes that new Fact ID. Full JSON and asset checks: [B13 direct-Core validation](bb-core-construction-observation-04-v1-validation.json). Batch details: [BUILD_BATCH_13](construction/MB-BUILD-20261114/BUILD_BATCH_13.md).

This adds an observation test, not a new normal-route capability. The test uses the natural-language input parser and `Engine::execute` with an isolated temporary SQLite store. It proves only this bounded calculation/contextual relation path. Full A01/A03/A04/A07/E08 and all other requirement acceptance remain `UNVERIFIED`; this is not a release or full workspace runtime claim.

## B14 — bounded Core rule-composition/counterexample observation (2026-11-14)

The new observation drives two explicitly declared root rules through ordinary `Engine::execute`. Core composes a proof-preserving shortcut; a subject absent from the rules then receives a committed `DERIVED` result whose dependency list includes both direct user premises and both root rules plus the shortcut. A later scope-specific source correction retains the original fact with `valid=false` and records the stale proof root in the correction result. Repeating the relation query suspends `MISSING_PREMISES` instead of reusing the historical COMMIT. The result history, source/hash/model traces and three-point-set asset checks are in [B14 direct-Core validation](bb-core-construction-observation-05-v1-validation.json); batch detail is [BUILD_BATCH_14](construction/MB-BUILD-20261114/BUILD_BATCH_14.md).

The dedicated B14 observation and full `minidora-engine` suite passed (232 unit tests; B11/B12/B13/B14 integrations 1/2/1/1; 0 doc tests); formatting, workspace all-target check, warnings-denied Clippy, locked build and `git diff --check` passed. This is bounded rule composition from user-declared rules, not induction from raw examples or broad learning acceptance. All affected requirement statuses remain `UNVERIFIED`; no release or full workspace runtime acceptance is claimed.

## B15 — bounded Core permission/budget/cancellation stop observation (2026-11-14)

The direct Core observation covers zero deadline, insufficient action budget, a missing local-write grant, an attempted-but-unconfirmed action record, follow-on work blocked as `EXTERNAL_EFFECT_UNKNOWN`, and explicit cancellation. The missing grant leaves the required file predicate unfulfilled, produces no receipt/file, and remains `SUSPEND`; cancellation leaves the unknown action recorded and terminates as `CANCEL`. The unknown action is a storage fixture; no real output write is claimed. See [B15 direct-Core validation](bb-core-construction-observation-06-v1-validation.json) and [BUILD_BATCH_15](construction/MB-BUILD-20261114/BUILD_BATCH_15.md).

The dedicated B15 observation and full `minidora-engine` suite passed (232 unit tests; B11/B12/B13/B14/B15 integrations 1/2/1/1/1; 0 doc tests). Formatting, workspace all-target check, warnings-denied Clippy, locked build and `git diff --check` passed.

This is a bounded in-process stop-path observation. Physical crash/power-loss behavior, remote effects, complete goal recovery, and E02/E04/E05/E06/E10 or all-requirement acceptance remain `UNVERIFIED`; no full workspace runtime or release claim.

## B16 — bounded Core deterministic-replay and version-cause observation (2026-11-14)

Two independently initialized temporary SQLite states received the same sealed rule, fact and question inputs under the same actor/case. Initial ResultRecords, InputBundle, CaseRevision, MemoryState, model revisions, dependencies and render hashes matched exactly. After reopening each Engine, version 2 corrected the `sensor` source to `polymer`; Core invalidated the prior fact, advanced memory revision 3→4, retained the historical derived result and suspended the same relation question as `MISSING_PREMISES`. Corrected input, result and final state also matched exactly between the two snapshots. The observation has no document input, so document-version variation is not measured. Full traces, environment and asset checks are in [B16 direct-Core validation](bb-core-construction-observation-07-v1-validation.json); batch detail is [BUILD_BATCH_16](construction/MB-BUILD-20261114/BUILD_BATCH_16.md).

The dedicated B16 test, 232-test engine unit suite and construction observations passed; formatting, workspace all-target check, warnings-denied Clippy and locked build passed. Full workspace runtime tests were not run. This is one host/toolchain deterministic replay observation and does not establish cross-process/platform determinism or complete A03/A15/CAP-D01/CAP-D04/E08/G08 or all-requirement acceptance. All acceptance statuses remain `UNVERIFIED`; no release.

## B17 — bounded Japanese arithmetic input and exact replay (2026-11-14)

The input grammar now accepts a terminal Japanese topic question such as `６＋７は？` when the complete preceding text is accepted by the existing arithmetic parser. It seals as `Calculate("6+7")` under parser v73 while retaining the exact original full-width request and input version. An ordinary `Engine::execute` run computes 13 and renders it; after Engine reopen, replay of the same input ID/version returns the exact saved ResultRecord without advancing the case revision. No-question, non-arithmetic and compound-request negative controls remain unadopted. See [B17 input-machine validation](bb-input-machine-observation-01-v73-validation.json) and [BUILD_BATCH_17](construction/MB-BUILD-20261114/BUILD_BATCH_17.md).

The dedicated route, engine suite (233 unit tests and eight bounded integrations), formatting, all-target check, warnings-denied Clippy and locked build passed. This is one deliberately narrow language form. It does not complete the sixth instruction's input-machine scope or establish broad A02/A04/A12/A15/E08 acceptance; all acceptance remains `UNVERIFIED`. Full workspace runtime tests were not run; no release.

## B18 — bounded meaning-role source-span mapping (2026-11-14)

`MeaningIR` now optionally maps selected extracted values to exact half-open UTF-8 spans in the untouched request. Parser v74 records relation subject, object, scope and negative marker, uses a relation-particle span only between subject and object, and maps arithmetic expression spans through an NFKC lookup to their original full-width characters. The saved negative relation bundle/result replays exactly after Engine reopen. When the optional role map is empty, serde omits it; a legacy MeaningIR canonical-byte compatibility test passed without changing the DB schema revision. See [B18 input-role validation](bb-input-machine-observation-02-v74-validation.json) and [BUILD_BATCH_18](construction/MB-BUILD-20261114/BUILD_BATCH_18.md).

The dedicated relation route, arithmetic replay, 235-test engine suite, formatting, workspace all-target check, warnings-denied Clippy and locked build passed. Source-role maps cover selected bounded Intent families and are not a complete field-level trace for every input. A04/A15/E08 and all requirement acceptance remain `UNVERIFIED`; full workspace runtime tests were not run; no release.

## B21 — NFKC source-span preservation (2026-11-14)

The v75 source-role mapper normalized each Unicode scalar separately. That missed composed forms across scalars, including Japanese halfwidth kana plus dakuten and Hangul Jamo. Parser v76 normalizes safe NFKC segments and maps normalized bytes to the full original segment. Ordinary `Engine::execute` relations for `ｶﾞはsensor` and `가はsensor` committed with exact six-byte subject spans; both saved cases replayed unchanged after store reopen. A unit corpus covers compatibility composition, Hangul, decomposed Latin, combining-mark reordering and a ligature. The dedicated engine package passed 236 unit tests and 11 integrations (247 total); formatting, workspace all-target check, warning-denied Clippy, build and `git diff --check` passed. Full workspace all-features runtime tests, other OS and wider route populations were not run in B21. A04/A15/E08 and all requirement acceptance remain `UNVERIFIED`; no release. See [B21 v76 validation](bb-input-machine-observation-04-v76-normalization-validation.json) and [BUILD_BATCH_21](construction/MB-BUILD-20261114/BUILD_BATCH_21.md).

## B22 — direct Japanese assertion role-span disambiguation (2026-11-14)

An ordinary negative assertion with the same NFKC-equivalent string in explicit scope, subject and object exposed that the v76 lexical index assigned all three source occurrences to all three roles. Parser v77 now uses the parser's bounded Japanese copular captures for supported direct `Assert` inputs, preserving exact original UTF-8 ranges for scope, subject, particle, object and negation. It omits an implicit `default` scope from source evidence. Other Intent families and assertion grammars retain the generic lexical index pending their own bounded observations.

The normal `Engine::execute` route committed `[scope=ｶﾞ]ｶﾞはｶﾞではない` and `defaultはdefaultではない`. The first saved bundle mapped the five observed roles to their individual original byte ranges; the second did not invent a span for its parser-generated default scope. Results and CaseRevisions replayed unchanged after Engine reopen. A v76 baseline test reproduced three matching scope spans for the first source and failed as expected; the v77 route passes. The full engine package passed 248 tests, with workspace all-target check, warnings-denied Clippy, locked build and formatting passing.

Evidence boundary: `MeaningIR.source_roles` is sealed and persisted with the input bundle, but current Core relation proof code consumes the typed `Intent` and does not use these role spans as proof dependencies. B22 establishes exact source-role attribution and storage/replay for this bounded grammar, not role-span consumption by inference, general source ownership, all input families, A04/A15/E08 acceptance or a release. Full-workspace all-features runtime, other OS and wider QA remain `NOT_RUN`; requirement acceptance remains `UNVERIFIED`. See [B22 validation](bb-input-machine-observation-05-v77-assertion-role-validation.json) and [BUILD_BATCH_22](construction/MB-BUILD-20261114/BUILD_BATCH_22.md).

## B23 — direct assertion role evidence on Core proof facts (2026-11-14)

The bounded direct Japanese copular `Assert` path now validates the sealed role map against the typed proposition and exact original UTF-8 slices. A complete, unique map becomes versioned `SealedSourceEvidence` on its direct `USER_PREMISE` Fact, linked by the registered request fingerprint, the sealed-bundle fingerprint, raw-text digest, parser version and semantic signature. Ambiguous/malformed role maps are not projected. The store resolves evidence to its actor-owned sealed input and rechecks the exact bundle metadata and role map on memory reads and writes; the optional Fact field defaults absent and is omitted when serializing legacy records.

The direct Core observation committed `[scope=ｶﾞ]ｶﾞはｶﾞではない`, retained five individual role spans in the memory Fact, reopened the state, and returned the same provenance-bearing Fact from the relation proof query. A normal follow-up question consumed that Fact ID. The implicit-default assertion `defaultはdefaultではない` retained subject/particle/object/negation spans without inventing a scope span. A deliberately duplicated role map was not projected, and a tampered raw digest was rejected by the store without changing retained memory. See [B23 validation](bb-input-machine-observation-06-v77-fact-source-validation.json) and [BUILD_BATCH_23](construction/MB-BUILD-20261114/BUILD_BATCH_23.md).

The `minidora-engine` package passed 250 tests; workspace formatting, all-target check, warnings-denied Clippy and locked build passed. Full-workspace runtime tests, other OS, broad source families and independent QA were not run. The source spans provide linked origin metadata; they do not themselves establish world truth or add separate inference premises. Context-restored assertions and non-copular assertion families are outside this observation. A04/A15/E08 and all requirement acceptance remain `UNVERIFIED`; no release.

## B24 — Japanese transitive assertion roles on Core proof facts (2026-11-14)

Parser v78 now captures source roles for supported direct Japanese transitive assertions: subject, `は`/`が`, object, `を`, the complete surface verb (including negative inflection), and an explicit scope when present. The parser checks the captured source fields against the already sealed typed proposition and its existing transitive grammar. Core projects only an exact match into the existing versioned `SealedSourceEvidence`; ambiguous or altered role maps remain unprojected. No fact schema or database revision changed. Context-restored assertions and non-Japanese/non-transitive families remain outside this route.

The focused in-process `Engine::execute` observation committed `[scope=biology]ミナは毒を食べません。`, persisted its original byte ranges on the direct `USER_PREMISE` Fact, reopened the engine, and returned the same evidence from a `SUPPORTED` relation proof. The subsequent same-scope ordinary Core question consumed that Fact ID. The source role preserves the negative verb form and explicit scope; it does not establish an outside-world claim. An input-unit corpus also covers implicit `default` scope without a fabricated span, identical subject/object text, the `が` particle and a polite verb form. See [B24 validation](bb-input-machine-observation-07-v78-transitive-assert-source-validation.json) and [BUILD_BATCH_24](construction/MB-BUILD-20261114/BUILD_BATCH_24.md).

`cargo test -p minidora-engine --locked` passed 240 unit and 12 integration tests. Formatting, workspace all-target locked check, warnings-denied Clippy, locked workspace build and `git diff --check` passed on Linux / Rust 1.99.1. Full-workspace runtime tests, CLI/UI/output delivery, other OS, broad source families and independent QA were not run. The global parser version change is part of request fingerprinting; old sealed bundles remain versioned and intact, and clients reusing the same `(case_id, input_id, input_version)` after a parser revision may need a new input version. A04/A15/E08 and all requirement acceptance remain `UNVERIFIED`; no release.

## B25 — contextual Japanese assertion source evidence through Core proof (2026-11-14)

Parser v79 captures source-role ranges for supported singular-pronoun Japanese copular and transitive `AssertPreviousRelation` inputs. For `[scope=biology]それは水を使う。`, the sealed bundle keeps the literal `それ` span and binds `context_result_ref` to the immediately preceding committed same-case relation result. Core rechecks that result and its premise proof, restores `ミナ` into the typed proposition, and puts the contextual request's sealed source evidence on the resulting `USER_PREMISE` Fact. The resolved subject and source pronoun remain distinct. Closing and reopening Engine preserves the input/context link; the relation proof returns the same evidence-bearing Fact and a normal subsequent Core Ask depends on that Fact ID. A copular contextual assertion follows the same bounded path. Duplicated role ranges do not project, and a request without a same-case anchor suspends without a proof Fact. The span records where the assertion came from and does not validate outside-world truth.

The parser revision changes sealed-input fingerprints. Previously sealed v78 bundles remain versioned; clients that register a request under an already-used `(case_id, input_id, input_version)` tuple after upgrading the parser may receive an identity conflict and should advance `input_version`.

The focused Core observation and all `minidora-engine` package tests passed (241 unit and 12 integration cases; 253 total). The earlier report counted 14 subprocess worker results as integration cases; the corrected count follows the 12 test harness cases launched by Cargo. `cargo fmt --all -- --check`, workspace all-target check, warnings-denied Clippy, and locked workspace build also passed on Linux/Rust 1.99.1. Full workspace runtime/all-features tests, CLI/UI/output delivery, other operating systems, independent QA and full A04/A15/E08 acceptance were not run; those requirements and overall acceptance remain `UNVERIFIED`. See [B25 validation](bb-input-machine-observation-08-v79-contextual-assert-source-validation.json) and [BUILD_BATCH_25](construction/MB-BUILD-20261114/BUILD_BATCH_25.md).

## B26 — half-width surface provenance on contextual negative transitive proof (2026-11-14)

The B26 Engine observation extends the supported B25 contextual path with `[scope=biology]それがﾐｽﾞを使いません。`. The sealed v79 bundle binds `context_result_ref` to the actual immediately preceding same-case relation result and retains exactly six literal source-role spans: pronoun `それ`, particle `が`, half-width object surface `ﾐｽﾞ`, particle `を`, polite negative predicate `使いません`, and explicit scope `biology`. Core restores subject `ミナ` while preserving the literal source subject and negative polarity on the `USER_PREMISE` Fact. The exact source evidence and linked Fact survive Engine reopen, the proof returns `SUPPORTED`, and a normal Core question consumes that same Fact ID. This remains a bounded source/proof observation; it does not normalize the stored object spelling or establish outside-world truth.

This batch adds the actual Core execution observation and regression test; it does not change the parser grammar, parser version, database schema, dependencies, or production implementation. The existing v79 code already supported these forms, and B26 verifies that their combined particle, polarity, surface spelling, provenance, persistence, proof and follow-up path stays connected. The focused test and all engine package tests passed (242 unit and 12 integration cases; 254 total). The earlier report counted 14 subprocess worker results as integration cases; the corrected count follows the 12 test harness cases launched by Cargo. Formatting, workspace all-target check, warnings-denied Clippy, locked build and `git diff --check` passed on Linux/Rust 1.99.1. Full workspace all-features runtime, CLI/UI/output delivery, other OS, independent QA and full A04/A15/E08 acceptance were not run; they remain `UNVERIFIED`. See [B26 validation](bb-input-machine-observation-09-v79-contextual-halfwidth-negative-assert-validation.json) and [BUILD_BATCH_26](construction/MB-BUILD-20261114/BUILD_BATCH_26.md).

## B27 — source evidence for Japanese premise correction (2026-11-14)

Parser v80 captures exact source ranges for the supported direct Japanese `CorrectPremise` grammar: shared subject, `は`/`が`, superseded object, `ではなく`, replacement object and explicit scope. It projects evidence only when the complete sealed Intent and every field match the original source. Core attaches the same sealed evidence to the correction's negative superseded Fact and positive replacement Fact in one memory CAS. Store reopens the immutable input and revalidates the evidence on memory read; both relation proofs return the same source-backed facts, and an ordinary follow-up depends on the replacement Fact ID.

The B27 observation covers `[scope=biology]訂正: ココは哺乳類ではなく鳥です。` after a matching directly stated premise. It is one bounded Japanese copular correction route. English correction source spans are not projected to Facts here; this does not establish general Japanese correction parsing, world truth, or complete A04/A15/E08 acceptance. The focused tests and `minidora-engine` package passed (244 unit + 12 Cargo-launched integration cases = 256; 0 failures). Formatting, workspace all-target check, warnings-denied Clippy, locked build and `git diff --check` passed on Linux/Rust 1.99.1. Full workspace all-features runtime, CLI/UI/output, other OS, independent QA and all 93 requirement acceptance remain unverified. See [B27 validation](bb-input-machine-observation-10-v80-premise-correction-source-validation.json) and [BUILD_BATCH_27](construction/MB-BUILD-20261114/BUILD_BATCH_27.md).

## B28 — source evidence for English premise correction (2026-11-14)

Parser v81 adds exact source-role spans to the already supported bounded English `CorrectPremise` form, including the subject, `is`/`are`, correction marker `not`, both objects, optional `a`/`an`/`the` article spans and explicit scope. Core attaches the validated map to both the negative superseded Fact and replacement Fact. Store reopens the sealed input and revalidates the same evidence; proofs for both Facts remain `SUPPORTED`, and a normal English relation question consumes the replacement Fact ID.

The B28 observation covers `[scope=biology]Correction: Coco is a bird, not a mammal.` after a matching direct premise. It does not establish arbitrary English corrections, broad language coverage, or world truth. The focused tests and `minidora-engine` package passed (246 unit + 12 Cargo-launched integration cases = 258; 0 failures). Formatting, workspace all-target check, warnings-denied Clippy, locked build and `git diff --check` passed on Linux/Rust 1.99.1. Full workspace all-features runtime, CLI/UI/output, other OS, independent QA and full A04/A15/E08 or 93-requirement acceptance were not run. Those acceptance states remain `UNVERIFIED`. See [B28 validation](bb-input-machine-observation-11-v81-english-premise-correction-source-validation.json) and [BUILD_BATCH_28](construction/MB-BUILD-20261114/BUILD_BATCH_28.md).

## B29 — contextual Japanese and English correction source evidence (2026-11-14)

Parser v82 now captures exact source-role byte ranges for the already-supported immediate singular-pronoun `CorrectPreviousPremise` forms: Japanese `それは哺乳類ではなく鳥です` and English `It is a bird, not a mammal`, with explicit scope. The literal pronoun remains in the sealed input while Core resolves the subject only from the immediately preceding committed same-case result and its unique direct-premise dependency. The input's `context_result_ref` must equal that actual result before Core writes memory. One actor-memory CAS attaches the same sealed evidence to both the negative superseded Fact and replacement Fact; Store reloads the immutable bundle and revalidates the evidence after Engine reopen.

The bounded Core observation verifies original UTF-8 ranges for the pronoun, copula/particle, old and new objects, English articles and explicit scope. Both Facts have `SUPPORTED` reopened proofs with the same source evidence, a normal follow-up consumes the replacement Fact ID, and an absent context suspends without evidence projection or Fact writes. If Core cannot uniquely validate source-role evidence, the contextual correction also suspends before writing either Fact. This records source attribution and a bounded Core lineage; it does not prove user-provided facts about the outside world or general co-reference.

The focused contextual route passed, as did `cargo test -p minidora-engine --locked` (247 unit + 12 Cargo-launched integration cases = 259; 0 failures). Formatting, workspace all-target locked check, warnings-denied Clippy, locked workspace build and `git diff --check` passed on Linux / Rust 1.99.1. Full-workspace all-features runtime, CLI/browser/output delivery, other operating systems, independent QA, and complete A04/A15/E08 or 93-requirement acceptance were not run; acceptance remains `UNVERIFIED`. See [B29 validation](bb-input-machine-observation-12-v82-contextual-premise-correction-source-validation.json) and [BUILD_BATCH_29](construction/MB-BUILD-20261114/BUILD_BATCH_29.md).

## B30 — direct English Assertion source evidence (2026-11-14)

Parser v83 now maps the already-supported direct English copular Assertion grammar to exact original-source byte ranges for the subject, `is`/`are`, optional lowercase `a`/`an` article, object, explicit scope, and optional `not`. Core projects a `SealedSourceEvidence` bundle onto the direct `USER_PREMISE` Fact only when reparsing the original request and the complete source-role map agree. An incomplete article-role map is not projected as evidence. After Engine reopen, Store retains the evidence, the relation proof returns it, and a normal same-case English Ask consumes the Fact ID. The negative `are/not` route also persists its source roles after reopen.

The bounded observation covers `[scope=biology]Coco is a bird` and `Mice are not mammals`. This records the user's source text and the engine's Fact/proof lineage; it does not verify the outside-world truth of either statement or establish broad English parsing. See [B30 validation](bb-input-machine-observation-13-v83-direct-english-assertion-source-validation.json).

The focused route passed. `cargo test -p minidora-engine --locked` passed with 248 unit tests and 12 Cargo-launched integration cases (260 total; 0 failures). Formatting, workspace all-targets check, warnings-denied Clippy, locked workspace build and `git diff --check` passed on Linux / Rust 1.99.1. Full workspace all-features runtime, CLI/browser/output delivery, other operating systems, independent QA, and complete A04/A15/E08 or 93-requirement acceptance were not run; acceptance remains `UNVERIFIED`. See [BUILD_BATCH_30](construction/MB-BUILD-20261114/BUILD_BATCH_30.md).

## E1-W10 B66 — Common-relations Recovery current-head revalidation (2026-11-18)

Pending-file Recovery for revision-bearing `AskCommonRelations` now uses the shared Core indexed evaluator against a revision-matched Store relation index. Recovery still checks the canonical report bytes, proof dependencies, guarantee and exact content unit. A cold or stale index retains the prior full-current-snapshot fallback; replay of the immutable adoption revision still uses its exact full snapshot. The regression adds 80 unrelated scoped Facts after adoption: with `max_actions=32`, full-snapshot current revalidation stops as `RESOURCE_LIMIT`, while the current-index check passes. Deleting the small fixture's index verifies fallback compatibility. Invalidating a supporting Fact still returns `RECOVERY_PROOF_INVALIDATED` without changing saved target bytes.

The focused Recovery test, three filtered common-relations library tests, engine all-target check, warnings-denied engine-library Clippy, formatting and `git diff --check` pass on Linux / Rust 1.99.1. The full engine package suite, workspace all-target Clippy, workspace all-features runtime/build, large-history immutable-revision projection and full acceptance were not run. This closes only the common-relations Recovery current-head recheck slice; E1-W10 remains `PARTIAL`, CAP-D02 remains `UNVERIFIED`, and no E1 exit or release is claimed. See [BUILD_BATCH_66](construction/MB-BUILD-20261117-D2-LLM/BUILD_BATCH_66.md) and [machine observation](construction/MB-BUILD-20261117-D2-LLM/b66-build-observation.json).

## E1-W10 B67 — Relation-object Recovery current-head revalidation (2026-11-18)

Pending-file Recovery for `AskRelationObjects` now shares the Core indexed evaluator over a revision-matched Store relation index. It projects the Goal's scoped subject/predicate candidates and shares `max_actions` with the existing answer proof evaluator. A cold/stale index retains the full-snapshot current fallback; exact adoption-revision replay remains full-snapshot. The regression adds 80 unrelated scoped Facts at `max_actions=32`: the full-snapshot current check stops as `RESOURCE_LIMIT`, the indexed current check passes, and invalidating the supporting Fact returns `RECOVERY_PROOF_INVALIDATED` without changing the saved file bytes.

The focused Recovery test, two filtered relation-object library tests, engine all-target check, warnings-denied engine-library Clippy, formatting and `git diff --check` pass on Linux / Rust 1.99.1. The full engine package suite, workspace all-target Clippy, workspace all-features runtime/build, large-history immutable-revision projection and full acceptance were not run. This closes only the relation-object Recovery current-head recheck slice; E1-W10 remains `PARTIAL`, CAP-D02 remains `UNVERIFIED`, and no E1 exit or release is claimed. See [BUILD_BATCH_67](construction/MB-BUILD-20261117-D2-LLM/BUILD_BATCH_67.md) and [machine observation](construction/MB-BUILD-20261117-D2-LLM/b67-build-observation.json).

## E1-W10 B68 — Ask/AskWhy Recovery current-head revalidation (2026-11-18)

Pending-file Recovery for `Ask`/`AskWhy` now rechecks the proof through a revision-matched Goal relation projection, with the previous authenticated full-snapshot path for cold/stale indexes. On a current memory with 82 Facts and one Rule, the Japanese Ask regression compares the indexed proof to the full-memory reference and returns identical status, Facts, Rules, dependencies and guarantee; the indexed evaluator uses a projection of one Fact and zero Rules. Exact adoption-revision replay and small-snapshot cold fallback still pass. Invalidating the adopted counterexample stops recovery without modifying the saved file bytes.

The focused Recovery test, three filtered indexed-relation tests, engine all-target check, warnings-denied engine-library Clippy, formatting and `git diff --check` pass on Linux / Rust 1.99.1. The full engine package suite, workspace all-target Clippy, workspace all-features runtime/build, large-history immutable-revision replay and complete acceptance were not run. This closes only the Ask/AskWhy Recovery current-head slice; E1-W10 remains `PARTIAL`, CAP-D02 remains `UNVERIFIED`, and no E1 exit or release is claimed. See [BUILD_BATCH_68](construction/MB-BUILD-20261117-D2-LLM/BUILD_BATCH_68.md) and [machine observation](construction/MB-BUILD-20261117-D2-LLM/b68-build-observation.json).


## E1-W10 B69 — AskConjunction Recovery current-head revalidation (2026-11-18)

Pending-file Recovery for `AskConjunction` now rechecks all clauses through the shared Core indexed evaluator in one revision-matched Store transaction. Each clause retains the existing sealed per-relation budget, and Recovery checks its supported/contradicted status, required content units, proof dependencies and guarantee. A cold or stale index keeps the authenticated full-current-snapshot fallback; exact adoption-revision replay remains unchanged.

The Japanese two-clause regression starts with the two adopted premises, forces a small-snapshot cold-index fallback, then adds 80 off-scope Facts. On an 82-Fact current head, each clause projection contains one Fact; the full-snapshot and indexed outcomes match for status, Facts, Rules, dependencies and guarantee. Recovery still passes current-head revalidation and refuses after premise invalidation with `RECOVERY_PROOF_INVALIDATED`, leaving target bytes unchanged.

The focused Recovery test and two filtered conjunction tests, engine all-target check, warnings-denied engine-library Clippy, formatting and `git diff --check` pass on Linux / Rust 1.99.1. The full engine package suite, workspace all-target Clippy, workspace all-features runtime/build, large-history immutable-revision replay and complete acceptance were not run. This closes only the `AskConjunction` Recovery current-head slice; E1-W10 remains `PARTIAL`, CAP-D02 remains `UNVERIFIED`, and no E1 exit or release is claimed. See [B69](construction/MB-BUILD-20261117-D2-LLM/BUILD_BATCH_69.md) and [machine observation](construction/MB-BUILD-20261117-D2-LLM/b69-build-observation.json).


## E1-W10 B70 — AskPreviousRelation Recovery current-head revalidation (2026-11-18)

Pending-file Recovery for `AskPreviousRelation` now uses the same Core indexed query as the ordinary prior-result route when the Store index matches current actor-memory head. Inside one transaction it resolves only the Fact IDs referenced by the immediately preceding committed ResultRecord, restores the typed subject/scope, and projects the current proof. Prior-ID lookup, projection and query share the sealed `max_actions`. Recovery still verifies the exact prior-result context, resolved proposition, result units, proof dependencies and guarantee. Cold/stale indexes retain full-current-snapshot fallback; adoption-revision replay remains full-snapshot.

The regression cold-starts Recovery on a 73-Fact history, then adds 80 off-scope Facts and compares the indexed answer against the 153-Fact full-memory answer. Resolution, status, Facts, Rules, dependencies, guarantee and units match, and the indexed proof memory contains fewer Facts. Invalidating the adopted premise still returns `RECOVERY_PROOF_INVALIDATED` without changing target bytes.

The focused Recovery test and three filtered `previous_relation` tests, engine all-target check, warnings-denied engine-library Clippy, formatting and `git diff --check` pass on Linux / Rust 1.99.1. A temporary fixture-only assertion expected three Rules but observed five; that non-contract count assertion was removed, and its failed output is retained in [the adjustment log](construction/MB-BUILD-20261117-D2-LLM/b70-initial-fixture-assertion-failure.log). The final tests pass. The full engine package suite, workspace all-target Clippy, workspace all-features runtime/build, large-history immutable-revision replay and complete acceptance were not run. This closes only the `AskPreviousRelation` Recovery current-head slice; E1-W10 remains `PARTIAL`, CAP-D02 remains `UNVERIFIED`, and no E1 exit or release is claimed. See [B70](construction/MB-BUILD-20261117-D2-LLM/BUILD_BATCH_70.md) and [machine observation](construction/MB-BUILD-20261117-D2-LLM/b70-build-observation.json).


## E1-W10 B71 — DraftRelationProfile Recovery current-head revalidation (2026-11-18)

Pending-file Recovery for `DraftRelationProfile` now uses the existing Core relation-profile evaluator over the revision-matched Store subject index. It retains the sealed `max_actions` and `max_evidence_bytes`, then checks the exact profile units, dependencies and `HYPOTHESIS` guarantee. Cold/stale indexes keep the authenticated full-current-snapshot fallback; replay of the immutable adoption revision remains full-snapshot.

The Recovery regression adopts the ordinary Japanese profile at 64 actions, reopens the Engine, and verifies cold-index fallback on the small current memory. It then appends 80 off-scope Facts, creating an 81-Fact head. The indexed profile matches a 10,000-action full-snapshot reference; the full-snapshot profile at the sealed 64-action budget returns `RESOURCE_LIMIT`, while Recovery passes through the index. Rule invalidation still returns `RECOVERY_PROOF_INVALIDATED` and preserves target bytes.

The focused Recovery test and five filtered profile tests, engine all-target check, warnings-denied engine-library Clippy, formatting and `git diff --check` pass on Linux / Rust 1.99.1. A temporary fixture assertion expected 82 Facts but observed 81 because the initial actor memory contains one premise Fact; the corrected assertion and final tests pass. The failed output remains in [the adjustment log](construction/MB-BUILD-20261117-D2-LLM/b71-initial-fixture-fact-count-failure.log). The full engine package suite, workspace all-target Clippy, workspace all-features runtime/build, large-history immutable-revision replay and full acceptance were not run. This closes only the `DraftRelationProfile` Recovery current-head slice; E1-W10 remains `PARTIAL`, CAP-D02 remains `UNVERIFIED`, and no E1 exit or release is claimed. See [B71](construction/MB-BUILD-20261117-D2-LLM/BUILD_BATCH_71.md) and [machine observation](construction/MB-BUILD-20261117-D2-LLM/b71-build-observation.json).


## E1-W10 B72 — InspectPremises Recovery current-head revalidation (2026-11-18)

Revision-bearing `InspectPremises` Recovery now gets the current inventory from the existing Core subject index in one revision-matched Store transaction. It rechecks the active dependency IDs and guarantee before confirming pending bytes. A cold/stale index uses the authenticated current full-snapshot fallback. Parser-version-specific legacy results without a memory revision retain their previous full-snapshot handling, and exact adoption-revision replay is unchanged.

The Japanese regression adopts an inventory at `max_actions=64`, reopens Engine, and forces a cold-index fallback on the small memory. It then adds 80 off-scope Facts, creating an 81-Fact current memory. The indexed inventory equals a 10,000-action full-snapshot reference; a 64-action full snapshot stops with `RESOURCE_LIMIT`, while Recovery revalidation passes. Invalidating the supporting Rule returns `RECOVERY_PREMISES_CHANGED` and the target bytes remain unchanged.

The focused Recovery test and two filtered premise-inventory tests, engine all-target check, warnings-denied engine-library Clippy, formatting and `git diff --check` pass on Linux / Rust 1.99.1. The full engine package suite, workspace all-target Clippy, workspace all-features runtime/build, large-history immutable-revision replay and full acceptance were not run. This closes only the revision-bearing `InspectPremises` Recovery current-head slice; E1-W10 remains `PARTIAL`, CAP-D02 remains `UNVERIFIED`, and no E1 exit or release is claimed. See [B72](construction/MB-BUILD-20261117-D2-LLM/BUILD_BATCH_72.md) and [machine observation](construction/MB-BUILD-20261117-D2-LLM/b72-build-observation.json).

Generic relation-answer `ExplainPrevious` Recovery current-head revalidation now uses the existing revision-matched Core relation index and shares the existing proof, dependency, guarantee and explanation-unit check. Cold/stale-index fallback and exact retained-revision replay remain full-snapshot; dedicated comparison/common-relations/object/profile branches are unchanged. `previous_answer_explanation_pins_and_rechecks_its_memory_revision` compares the current indexed proof over 81 Facts (one relevant and 80 off-scope) with the 10,000-action full-snapshot reference, checks cold-index fallback and Engine reopen, then confirms premise invalidation returns `RECOVERY_CONTEXT_PROOF_CHANGED` without changing the existing target bytes. Its focused test, the five-test explanation Recovery filter, engine all-target check, warnings-denied engine-library Clippy, formatting and `git diff --check` pass on Linux / Rust 1.99.1. An initial unsupported fixture assertion about full-snapshot `RELATION_BUDGET` was removed and retained in the audit logs. Large-history replay/fallback scale, workspace-wide checks and full acceptance remain unverified; E1-W10 remains `PARTIAL`, CAP-D02 `UNVERIFIED`, and no release is claimed. See [B73](construction/MB-BUILD-20261117-D2-LLM/BUILD_BATCH_73.md) and [machine observation](construction/MB-BUILD-20261117-D2-LLM/b73-build-observation.json).

Saved-profile `ExplainPrevious` Recovery current-head revalidation now uses the existing revision-matched Core profile index. A shared check compares indexed/current and exact prior profiles, dependencies, guarantee, explanation units and sealed budgets; cold/stale-index fallback and immutable exact-revision replay remain unchanged. `relation_profile_explanation_pins_and_rechecks_its_memory_revision` exercises Engine reopen, cold-index fallback, 80 off-scope Facts, an indexed profile equal to a 10,000-action full-snapshot reference where a 32-action full scan reaches `RELATION_BUDGET`, and refusal after supporting-Rule invalidation while preserving the explanation bytes. The focused Recovery test, five filtered profile tests, engine all-target check, warnings-denied engine-library Clippy, formatting and `git diff --check` pass on Linux / Rust 1.99.1. An initial Clippy `too_many_arguments` finding was resolved by grouping the subject and answer tuple; its log is retained. Large-history replay/fallback scale, workspace-wide checks and full acceptance remain unverified; E1-W10 remains `PARTIAL`, CAP-D02 `UNVERIFIED`, and no release is claimed. See [B74](construction/MB-BUILD-20261117-D2-LLM/BUILD_BATCH_74.md) and [machine observation](construction/MB-BUILD-20261117-D2-LLM/b74-build-observation.json).

Saved comparison `ExplainPrevious` Recovery current-head revalidation now uses Core's revision-matched comparison index. The current context branch uses the index when current and falls back to authenticated full current memory when cold/stale; exact retained-revision replay still receives its immutable snapshot and uses the original full-snapshot comparison verifier. `saved_relation_comparison_explanation_pins_and_rechecks_its_memory_revision` tests Engine reopen and cold-index fallback, then adds 80 off-scope Facts: the current 32-action full scan reaches `RELATION_BUDGET`, the 10,000-action reference passes, and indexed Recovery passes at the sealed 32-action budget. Invalidating a supporting Fact returns `RECOVERY_CONTEXT_PROOF_CHANGED` without changing explanation bytes. Its focused Recovery test, five-test explanation filter, engine all-target check, warnings-denied engine-library Clippy, formatting and `git diff --check` pass on Linux / Rust 1.99.1. Large-history exact replay/fallback scale, workspace-wide checks and full acceptance remain unverified; E1-W10 remains `PARTIAL`, CAP-D02 `UNVERIFIED`, and no release is claimed. See [B75](construction/MB-BUILD-20261117-D2-LLM/BUILD_BATCH_75.md) and [machine observation](construction/MB-BUILD-20261117-D2-LLM/b75-build-observation.json).

## E1-W10 B76 — Saved common-relations explanation Recovery current-head revalidation

The Recovery check for `ExplainPrevious` after a saved `AskCommonRelations` result uses the revision-matched current-head relation index and compares the recomputed report/proof with the saved result. A cold/stale index falls back to authenticated current memory; immutable saved-revision replay remains unchanged. The recorded focused and filtered Recovery tests passed, along with engine all-target check, library Clippy, formatting and diff checks. This closes only that saved common-relations explanation slice; W10 remains `PARTIAL` and CAP-D02 `UNVERIFIED`. See [B76](construction/MB-BUILD-20261117-D2-LLM/BUILD_BATCH_76.md).

## E1-W10 B77 — Saved relation-object explanation Recovery current-head revalidation

The saved `AskRelationObjects` explanation path now reuses Core's indexed object-list evaluator for current-head Recovery, with authenticated full-snapshot fallback when the index is cold/stale and exact saved-revision replay unchanged. The report records a passing focused test and explanation Recovery filter, plus compile, library Clippy, format and diff checks. This closes only that explanation consumer; W10 stays `PARTIAL` and CAP-D02 `UNVERIFIED`. See [B77](construction/MB-BUILD-20261117-D2-LLM/BUILD_BATCH_77.md).

## E1-W10 B78 — Saved conjunction explanation Recovery current-head revalidation

Recovery now rechecks the saved conjunction explanation through the current revision's indexed clause evaluator and retains authenticated full-snapshot fallback. The recorded focused/recovery tests and static checks passed. The initial fixture assumption that off-scope facts would force `RESOURCE_LIMIT` was removed after observation; no such performance claim is made. This closes only the saved conjunction explanation slice; W10 stays `PARTIAL` and CAP-D02 `UNVERIFIED`. See [B78](construction/MB-BUILD-20261117-D2-LLM/BUILD_BATCH_78.md).

## E1-W10 B79 — Saved premise inventory explanation Recovery current-head revalidation

The saved `InspectPremises` explanation path rechecks the typed inventory at current actor-memory head through the indexed subject inventory, with authenticated snapshot fallback; its exact saved-revision replay remains full-snapshot. B79 deliberately added and ran no functional tests, so runtime behavior was not verified. Its workspace all-target compile, engine library Clippy, formatting and diff checks passed. This closes only that consumer path; W10 remains `PARTIAL` and CAP-D02 `UNVERIFIED`. See [B79](construction/MB-BUILD-20261117-D2-LLM/BUILD_BATCH_79.md).

## E1-W10 B80 — Saved AskPreviousRelation explanation anchor Recovery revalidation

`ExplainPrevious` Recovery now recognizes a retained typed `AskPreviousRelation` result, verifies the exact sealed input and committed anchor, replays the saved answer from its retained actor-memory revision, then checks current head through the revision-matched prior-dependency index or authenticated full-snapshot fallback. The resolved proposition, proof dependencies, guarantee and exact answer units must still match. When the saved answer text has no parseable relation meaning, the explanation must preserve the exact prior text, dependency and guarantee. Mismatches stop with `RECOVERY_CONTEXT_PROOF_CHANGED`. Workspace all-target compile, warnings-denied engine library Clippy, formatting and diff checks pass; no functional tests were added or run, so this path's runtime behavior is unverified. W10 remains `PARTIAL`, CAP-D02 remains `UNVERIFIED`, and no E1 exit or release is claimed. See [B80](construction/MB-BUILD-20261117-D2-LLM/BUILD_BATCH_80.md) and [machine observation](construction/MB-BUILD-20261117-D2-LLM/b80-build-observation.json).

## E1-W10 B81 — Saved AskPreviousRelation explanation Recovery runtime regression (2026-11-18)

B81 adds one targeted Engine library regression test for B80's saved typed `AskPreviousRelation` -> `ExplainPrevious` Recovery recheck. The normal Japanese path commits both results and preserves the prior explanation text, dependency ID and guarantee. After Engine reopen, a forced cold index exercises the authenticated snapshot fallback. With 80 off-scope Facts, indexed current-head resolution/status/dependencies/guarantee/answer units match a 10,000-action full-snapshot reference. Invalidating the premise stops Recovery with `RECOVERY_CONTEXT_PROOF_CHANGED` while preserving the existing target bytes. The focused test passed (1/1), along with workspace all-target compile check, warnings-denied Engine library Clippy, formatting and `git diff --check`. This is a test-only change and covers one route; the full engine/workspace suites, all-target Clippy, large-history replay, all consumers and complete E1/CAP-D02 acceptance remain unverified. W10 remains `PARTIAL`, CAP-D02 `UNVERIFIED`, and no E1 exit or release is claimed. See [B81](construction/MB-BUILD-20261117-D2-LLM/BUILD_BATCH_81.md) and [machine observation](construction/MB-BUILD-20261117-D2-LLM/b81-build-observation.json).

## E1-W10 B82 — Saved AskPreviousRelation Recovery anchor-integrity negative cases (2026-11-18)

B82 adds one Engine-library Recovery regression test at the saved typed `AskPreviousRelation` -> `ExplainPrevious` direct-result anchor boundary. The ordinary Japanese request chain commits; after Engine reopen the saved explanation result is retrieved and a valid Recovery inspection passes. Removing the exact anchor from a cloned `CaseRevision.prior_results`, or retaining the map key while changing the contained `ResultRecord.id`, each stops with `RECOVERY_CONTEXT_PROOF_CHANGED`; the existing output bytes remain unchanged. The targeted test passed (1/1), and the B81+B82 Recovery filter passed (2/2), alongside workspace all-target compile check, warnings-denied Engine library Clippy, formatting and `git diff --check`. This is test-only and injects malformed cloned recovery snapshots; it does not test physical Store corruption or repair. W10 remains `PARTIAL`, CAP-D02 `UNVERIFIED`, and no E1 exit or release is claimed. See [B82](construction/MB-BUILD-20261117-D2-LLM/BUILD_BATCH_82.md) and [machine observation](construction/MB-BUILD-20261117-D2-LLM/b82-build-observation.json).

## E1-W10 B83 — Saved AskPreviousRelation Recovery rejects an uncommitted anchor (2026-11-19)

B83 adds one Engine-library Recovery test for the remaining direct-result anchor lifecycle guard. The ordinary Japanese `AskPreviousRelation` -> `ExplainPrevious` chain commits; after Engine reopen the valid saved explanation passes `inspect`. Keeping the anchor map key and `ResultRecord.id` correct while changing only its cloned lifecycle to `Received` stops Recovery with `RECOVERY_CONTEXT_PROOF_CHANGED`; existing output bytes remain unchanged. The focused test passed (1/1) and the B81+B82+B83 Recovery filter passed (3/3), along with workspace all-target compile check, warnings-denied Engine library Clippy, formatting and `git diff --check`. This test injects a cloned `CaseRevision` state and does not mutate or physically corrupt Store. W10 remains `PARTIAL`, CAP-D02 `UNVERIFIED`, and no E1 exit or release is claimed. See [B83](construction/MB-BUILD-20261117-D2-LLM/BUILD_BATCH_83.md) and [machine observation](construction/MB-BUILD-20261117-D2-LLM/b83-build-observation.json).

## E1-W10 B84 — Saved AskPreviousRelation Recovery binds the anchor to its sealed input (2026-11-19)

B84 closes one direct-result anchor input-identity gap in the saved typed `AskPreviousRelation` -> `ExplainPrevious` Recovery consumer. The Japanese Engine request chain commits, the Engine is reopened, and valid saved-state `inspect` passes. A clone retaining the anchor map key, `ResultRecord.id` and `COMMIT` lifecycle but changing only `input_id` to an unbound value was accepted before the guard; the test preserved this initial finding. Recovery now loads the anchor's exact sealed input using the stored `input_id/input_version` and checks same actor/case plus `identity(request) == anchor.id`. The malformed clone stops with `RECOVERY_CONTEXT_PROOF_CHANGED`; Store latest revision and output bytes remain unchanged. The focused test passed (1/1), the B81-B84 Recovery filter passed (4/4), and workspace all-target compile check, warnings-denied Engine library Clippy, formatting and `git diff --check` passed. This test changes a cloned `CaseRevision` only and does not test persistent Store corruption or repair. W10 remains `PARTIAL`, CAP-D02 `UNVERIFIED`, and no E1 exit or release is claimed. See [B84](construction/MB-BUILD-20261117-D2-LLM/BUILD_BATCH_84.md) and [machine observation](construction/MB-BUILD-20261117-D2-LLM/b84-build-observation.json).

## E1-W10 B85 — Saved AskPreviousRelation Recovery verifies the exact anchor input version (2026-11-19)

B85 adds one Engine-library Recovery regression for the exact `input_version` of a direct-result anchor. The fixture commits explicit-question versions 1 and 2 under the same actor/case/input ID, then the ordinary Japanese `AskPreviousRelation` -> `ExplainPrevious` chain anchors version 2 and commits. After Engine reopen, valid saved-state `inspect` passes. A clone retaining the anchor map key, `ResultRecord.id`, `COMMIT` lifecycle and `input_id` but rebinding only `input_version` to the existing version-1 sealed input stops with `RECOVERY_CONTEXT_PROOF_CHANGED`; persisted Store latest revision and output bytes remain unchanged. The focused test passed (1/1), the B81-B85 Recovery filter passed (5/5), and workspace all-target compile check, warnings-denied Engine library Clippy, formatting and `git diff --check` passed. This test modifies a cloned `CaseRevision` only and does not test persistent Store corruption or repair. W10 remains `PARTIAL`, CAP-D02 `UNVERIFIED`, and no E1 exit or release is claimed. See [B85](construction/MB-BUILD-20261117-D2-LLM/BUILD_BATCH_85.md) and [machine observation](construction/MB-BUILD-20261117-D2-LLM/b85-build-observation.json).


## E1-W10 B86 — Saved AskPreviousRelation Recovery rejects an anchor rebound to another existing input ID (2026-11-18)

B86 adds one Engine-library Recovery regression after the B84 sealed-input identity guard. The ordinary Japanese route commits alternate-query version 2 and explicit-query versions 1/2 under the same actor/case, anchors AskPreviousRelation to explicit-query version 2, saves ExplainPrevious, reopens Engine and accepts valid inspect. A clone retaining the anchor map key, ResultRecord.id, COMMIT lifecycle and input_version but changing only input_id to the existing alternate-query version-2 record stops with RECOVERY_CONTEXT_PROOF_CHANGED because the loaded request identity differs from anchor.id. Persisted latest revision and output bytes remain unchanged. The focused test passed (1/1), the B81-B86 Recovery filter passed (6/6), and workspace all-target compile check, warnings-denied Engine library Clippy, formatting and git diff check passed. This test modifies a cloned CaseRevision only; persistent Store corruption/repair, CLI/HTTP/browser and full Engine runtime suites were not exercised. W10 remains PARTIAL, CAP-D02 UNVERIFIED, and no E1 exit or release is claimed. See [B86](construction/MB-BUILD-20261117-D2-LLM/BUILD_BATCH_86.md) and [machine observation](construction/MB-BUILD-20261117-D2-LLM/b86-build-observation.json).


## E1-W10 B87 — ExplainPrevious Recovery rejects an unbound current-format prior result (2026-11-18)

B87 fixes a bypass in the saved AskPreviousRelation→ExplainPrevious Recovery path. The normal Japanese Engine chain commits a premise, AskPreviousRelation and ExplainPrevious; valid Recovery inspect passes after Engine reopen. An initial negative test changed only the prior AskPreviousRelation ResultRecord input_id in a cloned CaseRevision to an unbound ID. Despite actor_memory_revision being present, inspect returned Ok because a missing input bundle fell through the legacy optional-input path; the initial failure is preserved in the B87 log. Recovery now requires identity(request)==previous.id whenever the sealed bundle exists, and rejects a missing bundle for current-format results with actor_memory_revision using RECOVERY_CONTEXT_PROOF_CHANGED. Revision-less legacy fallback remains in code but was not independently runtime-tested. The focused test passes (1/1), the B81-B87 Recovery filter passes (7/7), and workspace all-target compile check, warnings-denied Engine library Clippy, format and diff check pass. The malformed state exists only in a clone; persisted latest revision and output bytes remain unchanged. W10 remains PARTIAL, CAP-D02 UNVERIFIED, and no E1 exit or release is claimed. See [B87](construction/MB-BUILD-20261117-D2-LLM/BUILD_BATCH_87.md) and [machine observation](construction/MB-BUILD-20261117-D2-LLM/b87-build-observation.json).

## E1-W10 B88 — Revision-less prior-result Recovery fallback compatibility (2026-11-19)

B88 adds one Engine-library compatibility regression for the existing revision-less AskPreviousRelation → ExplainPrevious Recovery fallback. The ordinary Japanese Engine route commits a premise, explicit question, AskPreviousRelation and ExplainPrevious; after Engine reopen, valid saved-state inspect passes. In a cloned CaseRevision only, the prior result's actor_memory_revision is cleared and input_id is changed to an unbound ID. Recovery inspect still succeeds through the revision-less missing-input fallback; persisted Store latest revision and output bytes remain unchanged. This synthetic clone is not a historical SQLite/schema migration test. No production behavior, schema or dependency changed. The focused test passed (1/1), the saved ExplainPrevious Recovery filter passed (8/8), and formatting and git diff check passed. Persistent legacy-store compatibility, full Engine/workspace suites and full CAP-D02 acceptance remain unverified. W10 remains PARTIAL, CAP-D02 UNVERIFIED, and no E1 exit or release is claimed. See [B88](construction/MB-BUILD-20261117-D2-LLM/BUILD_BATCH_88.md) and [machine observation](construction/MB-BUILD-20261117-D2-LLM/b88-build-observation.json).

## E1-W10 B89 — Revision-less prior-result Recovery rejects an existing misbound input (2026-11-19)

B89 adds one Engine-library clone-only regression for a revision-less AskPreviousRelation result whose input_id resolves to another existing sealed request. The normal Japanese Engine chain commits and, after Engine reopen, valid ExplainPrevious Recovery inspect passes. In a cloned CaseRevision only, actor_memory_revision is cleared and input_id is rebound to a different saved direct-question input with the same version. The test confirms that bundle exists and identity(request) differs from ResultRecord.id; Recovery stops with RECOVERY_CONTEXT_PROOF_CHANGED while persisted latest revision and output bytes remain unchanged. B88's missing-input revision-less fallback remains accepted. No production logic, schema or dependency changed. The focused test passed (1/1), the saved ExplainPrevious Recovery filter passed (9/9), and formatting and git diff check passed. This synthetic clone does not test actual historical SQLite/schema migration. W10 remains PARTIAL, CAP-D02 UNVERIFIED, and no E1 exit or release is claimed. See [B89](construction/MB-BUILD-20261117-D2-LLM/BUILD_BATCH_89.md) and [machine observation](construction/MB-BUILD-20261117-D2-LLM/b89-build-observation.json).

## E1-W10 B90 — Revision-less prior-result Recovery rejects an existing misbound input version (2026-11-19)

B90 adds one Engine-library clone-only regression for a revision-less saved AskPreviousRelation result bound to the wrong version of an existing sealed input. The Japanese Engine route commits version 1 and version 2 under the same input_id, then commits ExplainPrevious; after Engine reopen, valid Recovery inspect passes. In a cloned CaseRevision only, actor_memory_revision is cleared and input_version is changed from 2 to the existing version 1 while input_id, ResultRecord.id and lifecycle remain fixed. The test confirms the version 1 bundle exists and identity(request) differs from ResultRecord.id; Recovery stops with RECOVERY_CONTEXT_PROOF_CHANGED while persisted latest revision and output bytes remain unchanged. B88's missing-input fallback and B89's wrong-input_id rejection remain covered. No production logic, schema or dependency changed. The focused test passed (1/1), the saved ExplainPrevious Recovery filter passed (10/10), and format and diff checks passed. The first focused invocation could not find system linker `cc` because its PATH omitted `/usr/bin`; the rerun with system paths passed. This synthetic clone does not test actual historical SQLite/schema migration. W10 remains PARTIAL, CAP-D02 UNVERIFIED, and no E1 exit or release is claimed. See [B90](construction/MB-BUILD-20261117-D2-LLM/BUILD_BATCH_90.md) and [machine observation](construction/MB-BUILD-20261117-D2-LLM/b90-build-observation.json).

## E1-W10 B91 — Exact revision-less input binding still stops without the saved memory revision (2026-11-19)

B91 adds one Engine-library clone-only regression at the exact-input identity/replay boundary. The normal Japanese Engine chain commits a premise, direct question, AskPreviousRelation and ExplainPrevious; after Engine reopen, valid Recovery inspect passes. In a cloned CaseRevision only, actor_memory_revision is cleared while input_id, input_version, ResultRecord.id and lifecycle remain unchanged. The test confirms the existing sealed bundle's identity(request) equals ResultRecord.id, then Recovery stops with RECOVERY_CONTEXT_PROOF_CHANGED / RECOVERY_MEMORY_REVISION_MISSING because the prior answer cannot be replayed without its retained actor-memory revision. Persisted latest revision and output bytes remain unchanged. This differs from B88's missing-input revision-less fallback; B89/B90 continue to reject existing but misbound inputs. The focused test passed (1/1), the saved ExplainPrevious Recovery filter passed (11/11), and format and diff checks passed. Two initial acceptance probes exposed the missing-revision rejection; B91 preserves that observed fail-closed behavior rather than expanding compatibility. No production logic, schema or dependency changed, and no historical SQLite/schema migration was tested. W10 remains PARTIAL, CAP-D02 UNVERIFIED, and no E1 exit or release is claimed. See [B91](construction/MB-BUILD-20261117-D2-LLM/BUILD_BATCH_91.md) and [machine observation](construction/MB-BUILD-20261117-D2-LLM/b91-build-observation.json).

## E1-W10 B92 — ExplainPrevious Recovery rejects a revision pointer to an invalidated snapshot (2026-11-19)

B92 adds one Engine-library clone-only regression at the saved AskPreviousRelation actor-memory revision boundary. The Japanese Engine route commits a premise, direct question, AskPreviousRelation and ExplainPrevious; after reopen, the ordinary saved Recovery inspect passes against its retained memory snapshot. A separate temporary-fixture case then invalidates that premise, creating a different retained actor-memory snapshot. In a cloned CaseRevision only, the prior result's actor_memory_revision is rebound to that invalidated snapshot while its result ID, sealed input ID/version and bundle identity remain fixed. Recovery stops with RECOVERY_CONTEXT_PROOF_CHANGED because the prior relation no longer resolves to one direct premise in the recorded snapshot. The original case's latest revision and output bytes remain unchanged.

The focused test passed (1/1), the saved ExplainPrevious Recovery filter passed (12/12), and formatting and git diff checks passed on Rust 1.99.1. The first compile attempt exposed a missing mutable binding on the reopened fixture Engine; after correction, a test run passed with an unrelated unnecessary-mut warning, that unrelated change was removed, and the final focused run passed without warnings. The initial failure and clean retry are retained in the focused log. No production logic, schema, dependency, manifest or lockfile changed; the cloned malformed CaseRevision is never persisted. This does not test historical SQLite/schema migration. Full engine/workspace suites and complete CAP-D02 acceptance were not run. E1-W10 remains PARTIAL, CAP-D02 remains UNVERIFIED, and no E1 exit or release is claimed. See [B92](construction/MB-BUILD-20261117-D2-LLM/BUILD_BATCH_92.md) and [machine observation](construction/MB-BUILD-20261117-D2-LLM/b92-build-observation.json).

## E1-W10 B93 — ExplainPrevious Recovery rejects a missing actor-memory snapshot (2026-11-18)

B93 adds one Engine-library clone-only regression for a saved AskPreviousRelation result whose actor_memory_revision is set to a revision number with no retained snapshot. The normal Japanese Engine route commits a premise, direct question, AskPreviousRelation and ExplainPrevious; after Engine reopen, valid saved-state inspect passes, and the originally pinned snapshot is confirmed present. The test identifies the next actor-memory revision as absent, then changes only actor_memory_revision in a cloned CaseRevision while preserving the result ID and exact sealed input ID/version/identity. Recovery rejects the clone with RECOVERY_CONTEXT_PROOF_CHANGED wrapping RECOVERY_MEMORY_REVISION_MISSING. The persisted case revision, current actor-memory snapshot and output bytes remain unchanged.

The focused test passed (1/1), the saved ExplainPrevious Recovery filter passed (13/13), and formatting and git diff checks passed on Rust 1.99.1. No production logic, schema, dependency, manifest or lockfile changed; the malformed clone is never persisted and no historical SQLite/schema corruption or migration was tested. Full engine/workspace suites and complete CAP-D02 acceptance were not run. E1-W10 remains PARTIAL, CAP-D02 remains UNVERIFIED, and no E1 exit or release is claimed. See [B93](construction/MB-BUILD-20261117-D2-LLM/BUILD_BATCH_93.md) and [machine observation](construction/MB-BUILD-20261117-D2-LLM/b93-build-observation.json).


## E1-W10 B94 — AskConjunction ExplainPrevious Recovery rejects a missing actor-memory snapshot

B94 adds one Engine-library clone-only regression for the saved AskConjunction result consumed by ExplainPrevious Recovery. The ordinary Japanese Engine route commits the bird and animal premises, a conjunction answer and its explanation; after Engine reopen, the valid saved-state inspect passes and the conjunction's pinned actor-memory snapshot is confirmed present. The test then selects the next actor-memory revision, verifies that no snapshot exists there and changes only the saved conjunction result's pointer in a cloned CaseRevision. Recovery rejects the clone with RECOVERY_CONTEXT_PROOF_CHANGED wrapping RECOVERY_MEMORY_REVISION_MISSING. The persisted case revision, current actor-memory snapshot and target bytes remain unchanged.

The focused test passed (1/1), the conjunction explanation filter passed (2/2), and formatting and git diff checks passed on Rust 1.99.1. No production behavior, schema, dependency, manifest or lockfile changed; the malformed clone is never persisted. This does not test historical SQLite/schema migration, persistent Store corruption, full engine/workspace runtime suites or complete CAP-D02 acceptance. E1-W10 remains PARTIAL, CAP-D02 remains UNVERIFIED, and no E1 exit or release is claimed. See [B94](construction/MB-BUILD-20261117-D2-LLM/BUILD_BATCH_94.md) and [machine observation](construction/MB-BUILD-20261117-D2-LLM/b94-build-observation.json).


## E1-W10 B95 — AskCommonRelations ExplainPrevious Recovery rejects a missing actor-memory snapshot

B95 adds one Engine-library clone-only regression for the saved AskCommonRelations result consumed by ExplainPrevious Recovery. The ordinary Japanese Engine route commits a derived common-relations answer with a retained actor-memory revision, then an explanation; after reopening Engine, valid saved-state inspect passes. The test selects the next actor-memory revision, confirms that no snapshot exists there and changes only the common-relations result pointer in a cloned CaseRevision. Recovery rejects the clone directly with RECOVERY_MEMORY_REVISION_MISSING. The persisted case revision, current actor-memory snapshot and target bytes remain unchanged.

The first probe expected the higher-level RECOVERY_CONTEXT_PROOF_CHANGED wrapper, but this route returns the typed missing-revision error directly from result-memory provenance validation before the later context wrapper runs. The initial assertion output is retained; the final regression asserts the observed fail-closed error. No production behavior, schema or dependency changed. The focused test passed (1/1), the common-relations explanation filter passed (2/2), and formatting and git diff checks passed on Rust 1.99.1. This does not test persistent Store corruption, historical schema migration or full CAP-D02 acceptance. E1-W10 remains PARTIAL, CAP-D02 remains UNVERIFIED, and no E1 exit or release is claimed. See [B95](construction/MB-BUILD-20261117-D2-LLM/BUILD_BATCH_95.md) and [machine observation](construction/MB-BUILD-20261117-D2-LLM/b95-build-observation.json).


## E1-W10 B96 — AskRelationObjects ExplainPrevious Recovery rejects a missing actor-memory snapshot

B96 adds one Engine-library clone-only regression for the saved AskRelationObjects result consumed by ExplainPrevious Recovery. The ordinary Japanese Engine route commits a direct relation, a rule, a derived object-list result and its explanation; after reopening Engine, valid saved-state inspect passes and the result's pinned actor-memory revision is confirmed present. The test selects the next revision, verifies that no snapshot exists there and changes only the saved object-list result pointer in a cloned CaseRevision. Recovery refuses the clone with RECOVERY_MEMORY_REVISION_MISSING. The persisted case revision, current actor-memory state and target bytes remain unchanged.

The focused test passed (1/1), the relation-object explanation filter passed (2/2), and formatting and git diff checks passed on Rust 1.99.1. No production behavior, schema or dependency changed; the malformed clone is never persisted. This synthetic check does not test persistent Store corruption, historical SQLite/schema migration or full CAP-D02 acceptance. E1-W10 remains PARTIAL, CAP-D02 remains UNVERIFIED, and no E1 exit or release is claimed. See [B96](construction/MB-BUILD-20261117-D2-LLM/BUILD_BATCH_96.md) and [machine observation](construction/MB-BUILD-20261117-D2-LLM/b96-build-observation.json).


## 2026-11-19 JST B97 — D3-C E1-W03 算術Goalの厳密値を採用・復旧へ接続

D2仕様・D2工程・D3-C指示を照合した着工点は `57943960342e14e9cdc334d52163429f01125f86`。唯一のEngine/Coreは通常の `Intent::Calculate` から式と `EXACT_CALCULATION_V1` を元Goalに封印する。封印式の厳密計算を照合し、式・値・演算過程を持つ `Calculation` と採用文面が一致する場合に、実計算artifactを消費したContentPlanを採用する。既存の描画検査と、ファイルが必要な場合の権限・実バイト・receipt検査を経てCoreが終端を決める。復旧でも新述語を検証し、保存済みの従来述語列を受理する。新しい述語値はV1とし、保存スキーマとグラフの行動数は既存形式で継続する。

Linux x86_64 6.18.57、Rust/Cargo 1.99.1、対象ソース差分SHA-256 `c14bf7d5038ce7fc151301a36b8347dd258d9aa0a74dce9921d6dfe0ec9a0ec8`。以下は `rust/` から実行し、すべてexit 0。

| 検査 | 観測 |
|---|---|
| `cargo test -p minidora-engine --lib recovery::tests::exact_arithmetic_goal_requires_the_sealed_value --locked -- --exact --nocapture` | 1 PASS。全角の算術要求から3行動枠で厳密値 `7/2` と描画を確認。ファイルの未確定journalを用いた検査で、計算値の不一致を `GOAL_CALCULATION_MISMATCH` で停止。従来述語列の再構成fixtureを受理し、新述語の正しい記録は同一作用IDで再送せず確定。 |
| `cargo test -p minidora-engine --lib file --locked` | 13 PASS。無許可、欠落・異なるファイル、取消、symlinkの既存境界を含む。 |
| `cargo test -p minidora-engine --lib interrupted_actual_write_reconciles_same_identity_without_resending --locked` | 1 PASS。同一作用の復旧を確認。 |
| `cargo fmt --all -- --check`、`cargo check -p minidora-engine --all-targets --locked`、`git diff --check` | PASS。 |

算術Goalの局所結線を確認。W03全幅とE1出口は未成立。E2合同ゲート、体系QC、独立QA、過去バージョンの実DB移行は `NOT_RUN`。W10の追加Recovery枝はD3-Cに従い後工程に保持する。次のE1接続候補は、通常の `AskWhy` が選んだ証明を意味計画とL6生成へ渡すW07〜W09の経路。W13の永続モデル適応も未接続。

復旧2世代は `5794396` と `e0e40cc`。資産は計5件。保持2件（この台帳と要求トレース）、保管済み0件、稼働中3件（Core、Recoveryと付属テスト、共有Cargoキャッシュ）、ゴミ箱移動0件、保留0件。Windows保管は非該当。ファイル作用検査は一時fixture内で実施。


## 2026-11-19 JST B98 — D3-C E1-W07〜W09 合成証明から通常の説明を生成

着工点は `43df4b19bbfaffd1daa20b7801071ec39161e457`。D2仕様・D2工程・D3-C指示の採用SHAを照合。唯一のEngine/Coreは、通常の `AskWhy` で選んだ証明が保存済み合成規則を使った場合に、その証明から文を含まない型付き意味計画を作る。既存の非ニューラル文法生成へ渡し、K7が結論、条件評価、保証、証明ID、文面と出力記録を再照合して採用する。V5計算成果物を保存し、描画と必要なファイルの実観測を元Goalに帰還させる。復旧は封印された質問・予算、生成時の記憶版、現在の証明を照合し、認証済みV5のモデルと出力系列を再生して同じ作用を確定する。従来形式の復旧は既存の照合を使う。

Linux x86_64 6.18.57、Rust/Cargo 1.99.1、検査対象差分SHA-256 `f9f77a48e646ff73b5c1f17c1082118e5c553f63ac233a404afdd435e016896c`。`rust/` での最終検査はすべてexit 0。

| 検査 | 結果 |
|---|---|
| `cargo test -p minidora-engine --lib recovery::tests::ordinary_why_generates_from_a_saved_composed_proof_and_replays_it --locked -- --exact --nocapture` | 1 PASS。規則形成前は保留。形成後は通常の日本語入力からV5生成・ファイル描画。中断journalで再起動後の同一作用ID確定と書込時刻の保持を確認。追問が元結果を消費し、別スコープの追加後も元版と現在版で同じ意味を復旧。範囲外で保留し、親規則失効後は復旧を拒否。 |
| `cargo test -p minidora-engine --test core_proof_composition_learning_49 --test core_atomic_explanation_50 --locked` | 2 PASS。既存の規則合成と直結型付き生成。 |
| `cargo test -p minidora-engine --lib interrupted_actual_write_reconciles_same_identity_without_resending --locked` | 1 PASS。算術の中断復旧。 |
| `cargo test -p minidora-cli --test normal_route why_question --locked` | 2 PASS。既存の説明と省略追問。 |
| `cargo test -p minidora-cli --test normal_route conjunctive_rule_joins_two_new_subject_premises_and_explains_both --locked` | 1 PASS。両前提、結論、二親規則が型付き計画と実文面に残ることをCLIで確認。失効の既存検査を継続。 |
| `cargo fmt --all -- --check`、`cargo check -p minidora-engine --all-targets --locked`、`git diff --check` | PASS。 |

中断検査の再利用時にfixtureのSQLite入力版が `u64` で結合できず、`i64` への検査付き変換で是正した。CLIの初回検査では旧文面の固定文字列が不一致だったため、置換された文面とV5を検査し、同じ前提・結論・親規則・失効の成立を再確認した。

保存された規則の適応が後続のK2とK4へ届く範囲を観測した。現在の文法生成は要求内モデルを使用し、W13の永続モデル適応とE1出口は未成立。一般の長大な証明と過去版実DBの移行、E2合同ゲート、体系QC・独立QAは `NOT_RUN`。復旧2世代は `43df4b1` と `5794396`。資産計6件。保持2件（この台帳と要求トレース）、保管済み0件、稼働中4件（Core、Recovery、CLI回帰、共有Cargoキャッシュ）、ゴミ箱移動0件、保留0件。Windows保管は非該当。テストの一時状態とファイルはfixtureが処理。


## 2026-11-19 JST B99 — D3-C E1-W13 保存済み合成規則から文面モデルを適応

着工点は `a8aeb3bcc7db8a2ebe7bf56751312c5a0bf83119`。D2仕様・D2工程・D3-C指示の採用SHAを照合。唯一のEngine/Coreは、同じactor・caseのInputが固定した確定済み通常 `AskWhy` を一件参照する。後続の証明と完全に一致する合成規則と二つの親規則だけを選び、範囲・時点・条件参照を照合して、保存済みの型付き規則から文法断片を再構成する。前回の完成文、主体固有の事実と主張は学習材料に含めない。既存の `SequenceModel::learn_until` で今回の文法を学び、適応したモデルで実際の候補分布を計算する。V6は出所結果ID・計算参照・学習に使った規則ID・モデル・トークン観測を保持する。復旧は同じ出所と現在も有効な証明を照合し、元のモデルと出力記録を再生する。学習材料の選択は一つの元記録に限り、過去の適応モデルを再帰的に合成しない。

Linux x86_64 6.18.57、Rust/Cargo 1.99.1。対象ソース差分SHA-256 `487cdaa274568c9352bc6fdffe8e11291814b0409369ae496d314a1956cb0d30`。最終検査はすべてexit 0。

| 検査 | 観測 |
|---|---|
| `cargo test -p minidora-engine --lib recovery::tests::ordinary_why_ --locked -- --nocapture` | 2 PASS。新しい主体の同一質問・同一意味計画・同一証明で、未適応V5と再起動後の適応V6を比較。候補集合が同じ14箇所で確率分布が変化。実文面は同じ。モデル語彙に前回の主体を表す文字がないことを確認。別スコープはV5で成立し、元版と現行版の証明を照合してV6を再生。親規則失効後は既存ファイルを確定しない。B98のV5復旧も確認。 |
| `cargo test -p minidora-engine --test core_atomic_explanation_50 --locked` | 1 PASS。従来の直接型付きV5生成と再利用。 |
| `cargo test --manifest-path rust/Cargo.toml -p minidora-cli --test normal_route conjunctive_rule_joins_two_new_subject_premises_and_explains_both --locked` | 1 PASS。通常の複合規則・文面・失効経路。 |
| `cargo check -p minidora-engine --all-targets --locked`、`cargo fmt --all -- --check`、`git diff --check` | PASS。 |

W13は同一ケース内での実分布差と版再生まで `PARTIAL`。ケースを越えた適応、出力文面の差、K6の動作差、E1出口は未成立。E2合同ゲート・体系QC・独立QAは `NOT_RUN`。次は元Goalの未充足から必要計算を選び、実引数を作用と成果判定につなぐW14〜W16の境界を照合する。復旧2世代は `a8aeb3b` と `43df4b1`。資産計6件。保持2件（この台帳と要求トレース）、保管済み0件、稼働中4件（Core、language、Recoveryと付属検査、共有Cargoキャッシュ）、ゴミ箱移動0件、保留0件。Windows保管は非該当。


## 2026-11-19 JST B100 — D3-C E1-W14〜W16 連続計算のGoal駆動と中断復旧

着工点は `2a97d7444ce7663e8fc9166b29ce793c575948b9`。D2仕様・D2工程・D3-C指示の採用SHAを照合。単一Engine/Coreは `CalculateSequence` の元Goalに `EXACT_CALCULATION_SEQUENCE_V1` を記録する。封印したグラフの未充足ノードから実行可能な次段を選び、当該段が宣言した先行段の厳密値と成果物参照を渡す。全段の実観測後に、宣言された最終段の値と全段の表示を照合して採用する。既存の権限とファイル実観測による確定を使う。

中断復旧も同じ計算を通し、封印原文の範囲、参照の各出現、各段の保存バイト、v1報告、最終値、採用文面を照合する。指定先の実バイトが一致すると、同じ作用IDを再実行せず確定し、段ごとに消費した成果物参照を実行グラフに残す。従来のGoal述語列も同じ照合で受理する。

Linux x86_64 6.18.57、Rust/Cargo 1.99.1。対象ソース差分SHA-256 `07332f660b1ac55ae3f77974b4f27cc8db4a0a7cda600e804b131c39c907791c`。以下は `rust/` から実行し、すべてexit 0。

| 検査 | 観測 |
|---|---|
| `cargo test -p minidora-engine --lib recovery::tests::calculation_sequence_reconciles_observed_steps_without_rewriting_file --locked -- --exact --nocapture` | 1 PASS。3段と採用・実作用を7行動枠で実行。最終値 `20`。書込後・確定前のjournalから再起動し、復旧後も作用ID、ファイル内容、更新時刻を照合。依存順の不一致を拒否し、従来形式のGoalを再構成したfixtureを照合。 |
| `cargo test -p minidora-cli --test normal_route calculation_sequence_route --locked` | 5 PASS。順序、容量、繰返し参照、単位、途中失敗、権限、既確定ファイルへの再実行防止。 |
| `cargo test -p minidora-engine --lib exact_arithmetic_goal_requires_the_sealed_value --locked` | 1 PASS。B97の単式Goalと復旧。 |
| `cargo check -p minidora-engine --all-targets --locked`、`cargo fmt --all -- --check`、`git diff --check` | PASS。 |

確認範囲は連続計算と許可済みローカルファイル。W14〜W16全幅とE1出口は未成立。実際の旧版DB移行、E2合同ゲート、体系QC・独立QAは `NOT_RUN`。次はW17の停止・取消・既発生作用の照合を、同じ元Goalと実作用の境界から確認する。復旧2世代は `2a97d74` と `a8aeb3b`。資産計5件。保持2件（この台帳と要求トレース）、保管済み0件、稼働中3件（Core、Recoveryと付属検査、共有Cargoキャッシュ）、ゴミ箱移動0件、保留0件。Windows保管は非該当。


## 2026-11-19 JST B101 — D3-C E1-W17 ローカルファイル作用の取消と発行境界

着工点は `f95f3b196ea53e477372620d90b7b037dc543c73`。D2仕様・D2工程・D3-C指示の採用SHAを照合。Coreが元Goalの取消を保存すると、同じ入力が所有するローカルファイル作用の `PROPOSED`・`AUTHORIZED`・`PREPARED` を同一作用IDの `CANCELLED` に閉じる。停止後の明示取消と、取消保存後に中断した場合の再起動照合も同じ閉鎖を使う。試行記録がある作用は、元権限・同一作用IDの実物照合へ引き継ぐ。

Coreが通常のファイル作用を進めるとき、Storeは採用済みのケース版、元入力、描画SHAを作用記録と同じSQLite書込トランザクションで照合する。元版を失効させた後の発行と、同じ試行の二重取得は拒否する。保存形式・依存関係の変更なし。

Linux x86_64 6.18.57、Rust/Cargo 1.99.1。対象ソース差分SHA-256 `b4eedd6bbc818cebf386625e03c84e97034f5248626dc7e33f551075bec05e88`。以下は `rust/` から実行し、すべてexit 0。

| 検査 | 観測 |
|---|---|
| `cargo test -p minidora-engine --lib recovery::tests::<filter> --locked` の5フィルタ `cancel`、`authorized_cancel`、`sequence_cancel`、`local_file_attempt`、`calculation_sequence_reconciles` | 8 PASS。試行前3段階の取消、停止からの取消、再起動照合、旧版発行・二重発行の拒否を確認。試行前に実バイトが一致していてもreceiptは付かず、既に試行した作用は元IDで照合される。 |
| `cargo test -p minidora-engine --test core_construction_observation_06 --locked` | 1 PASS。権限、予算、取消、結果不明による停止の既存境界。 |
| `cargo test -p minidora-cli --test normal_route calculation_sequence_route --locked` | 5 PASS。通常CLIの連続計算と一度だけのファイル確定を含む。 |
| `cargo check -p minidora-engine --all-targets --locked`、`cargo fmt --all -- --check`、`git diff --check` | PASS。 |

検査対象はローカルファイルで、journalの中断点を再現した隔離fixtureと別Store接続を使用。描画後から実書込までの時間予算の再検査が残るため、W17とE1は `PARTIAL`、E12は `UNVERIFIED`。E2合同ゲート・体系QC・独立QAは `NOT_RUN`。

復旧2世代のローカル参照・GitHubタグは `f95f3b1` と `2a97d74`。資産計6件。保持2件（この台帳と要求トレース）、保管済み0件、稼働中4件（Core、Store、Recoveryと付属検査、共有Cargoキャッシュ）、ゴミ箱移動0件、保留0件。Windows保管は非該当。


## 2026-11-19 JST B102 — D3-C E1-W17 元Goalの締切をローカルファイル作用まで適用

着工点は `222288d7283a1b35422404117bdcea3842f067b0`。D2仕様・D2工程・D3-C指示の採用SHAを照合。単一Coreが、元Goalの時間予算から求めた単調時計上の同じ締切を描画、作用履歴、ローカルファイルの作成直前、完了判定に使う。StoreはSQLite書込ロック取得後と作用履歴の確定直前に締切を照合する。発行前に失効した準備は同一IDで `CANCELLED` とし、発行記録後に実出力開始前の失効を観測した場合は同一IDで理由付き `FAILED` を保持する。

既に始めた書込みの結果は実観測に従って記録する。確認済みのファイルと受領は元Goalの `SUSPEND/DEADLINE` でも保持する。中断時に試行結果が不明な作用は、既存の同一ID照合を使う。保存形式と依存関係の版は `0.7.0-alpha.11` のまま。

Linux x86_64 6.18.57、Rust/Cargo 1.99.1。対象ソース差分SHA-256 `dfd7da82a123114a4c993f7162cf672dacbfa785de8c35e3fa24d58cd6405bbb`。以下は `rust/` から実行し、すべてexit 0。

| 検査 | 観測 |
|---|---|
| `cargo test -p minidora-engine --lib local_file --locked` | 2 PASS。通常の算術要求から実描画。描画後、作用提案前、試行記録前、試行記録後の作成前、確認済み受領後の5点でテスト時計を進めた。実バイト・受領・永続journalを再起動後に照合し、同じ入力の再送で作用が増えないことを確認。二重取得の拒否も確認。 |
| `cargo test -p minidora-engine --lib output::tests --locked` | 5 PASS。権限、パス、symlink、実バイト、別プロセス間の上書き拒否。 |
| `cargo test -p minidora-engine --lib recovery::tests::sequence_cancel --locked`、`cargo test -p minidora-engine --lib recovery::tests::calculation_sequence_reconciles --locked` | 各1 PASS。取消と同一作用の復旧。 |
| `cargo test -p minidora-engine --test core_construction_observation_06 --locked`、`cargo test -p minidora-cli --test normal_route calculation_sequence_route --locked` | 1 PASSと5 PASS。権限・予算・取消・未知結果の停止、通常CLIの連続計算と一度だけのファイル確定。 |
| `cargo check --workspace --all-targets --locked`、`cargo fmt --all -- --check`、`git diff --check` | PASS。 |

今回の接続は通常のローカルファイル経路。OSやSQLiteの同期I/O呼出しには実行中の割込み保証がなく、実時間の上限測定は `NOT_RUN`。W17全幅とE1出口は `PARTIAL`、E04は `UNVERIFIED`。E2合同ゲート、体系QC、独立QAは `NOT_RUN`。次のE1結合点はW18で循環AとBの本番経路を照合し、K5の適応が後続別要求のK4・K6で実消費される境界を確認する。

復旧2世代のローカル参照・GitHubタグは `222288d` と `f95f3b1`。資産計7件。保持2件（この台帳と要求トレース）、保管済み0件、稼働中5件（Core、Store、Output、Recoveryと付属検査、共有Cargoキャッシュ）、ゴミ箱移動0件、保留0件。Windows保管は非該当。


## 2026-11-19 JST B103 — D3-C E1-W13/W18 保存済み合成規則を別ケースの生成と実作用へ接続

着工点は `aab4a869adde33bc09c85e31cc484c614bcdaec3`。D2仕様・D2工程・D3-C指示の採用SHAを照合。Coreは通常の `AskWhy` が利用者の保存済み合成規則を証明に使うとき、当該規則と親規則の型付き文法断片を生成モデルに渡す。別ケースへの継承元は利用者の規則記憶であり、渡す断片を現在の証明に含まれる規則に限定する。断片から要求ごとにモデルを構成し、V7の計算成果物に規則IDを残す。

採用後はV7を消費したContentPlanから描画し、許可先の実ファイルと受領を元Goalへ戻す。復旧は当時と現在の証明を照合し、固定された規則IDから同じモデルと系列を再生する。型付き生成のV5と、封印済みの同一ケースの先行説明を使うV6も検査した。SQLiteスキーマ・依存版変更なし。

Linux x86_64 6.18.57、Rust/Cargo 1.99.1。対象ソース差分SHA-256 `e2c32ec827e765f6226e4361dffe34769f124fa2cef254dd5114af2c89a55227`。最終検査は `rust/` から実行し、すべてexit 0。

| 検査 | 観測 |
|---|---|
| `cargo test -p minidora-engine --lib recovery::tests::ordinary_why_ --locked -- --nocapture` | 2 PASS。規則が揃う前は保留し作用0。規則形成とCore再起動後は、別ケースの初回入力からV7を生成し、実ファイル・同一作用IDの受領・Goal COMMITまで到達。同一意味計画・証明を用いたV5の型付き対照と比べ、同じ候補集合の14箇所で実分布が変わり、最終文面は同じ。別利用者には適用せず、別scopeではそのscopeの規則だけを使用。親規則失効後は再使用と復旧を停止。中断したV7の実ファイルを同一IDで再送せず照合し、V6も再生した。 |
| `cargo test -p minidora-engine --test core_proof_composition_learning_49 --test core_atomic_explanation_50 --test core_construction_observation_06 --locked` | 3 PASS。証明保存型合成、従来型付き生成、権限・予算・取消・未知結果の停止。 |
| `cargo test -p minidora-cli --test normal_route conjunctive_rule_joins_two_new_subject_premises_and_explains_both --locked`、同 `why_question` フィルタ | 1 PASSと2 PASS。通常CLIの複合規則・二前提・失効と、既存の説明・省略追問。 |
| `cargo test -p minidora-engine --lib local_file --locked` | 2 PASS。元Goalの締切停止と作用の二重取得拒否。 |
| `cargo check --workspace --all-targets --locked`、`cargo fmt --all -- --check`、`git diff --check` | PASS。 |

初回の検査コンパイルでは新しい対照用fixtureの `input::seal` の参照が未解決だった。crate内の正しい参照に修正し、上記を再検査した。

W18は証明保存型学習から別ケースの推論、生成、ファイル作用、元Goalまでの結合を確認した範囲で `PARTIAL`。K6の経路差は規則形成による保留から実作用への差であり、同じ証明への文法適応で観測した差は生成候補の分布。観測から形成した仮説の保持・区別を含むW12との結合とE1出口は未確認。CAP-D05は `UNVERIFIED`。E2合同ゲート、体系QC、独立QAは `NOT_RUN`。

復旧2世代のローカル参照・GitHubタグは `aab4a86` と `222288d`。資産計6件。保持2件（この台帳と要求トレース）、保管済み0件、稼働中4件（Core、Recoveryの検査、CLIの検査、共有Cargoキャッシュ）、ゴミ箱移動0件、保留0件。Windows保管は非該当。


## 2026-11-19 JST B104 — D3-C E1-W12 観測標本から仮説を保持し後続の説明へ接続

着工点は `4fffd49582f77cbf829c993162d0d78ecb1065da`。D2仕様・D2工程・D3-C指示の採用SHAを照合。利用者がJSON表のグループ別統計を確定した同じケースで「前回のグループ別統計から仮説を保持して」と要求すると、Coreが封印入力・元表・確定済みの統計成果物を再照合する。同じ単位で二群以上を比較でき、標本の平均が単独で最大の群があり、その順位が同条件の再標本でも得られる可能性を、版付き成果物に `HYPOTHESIS_RETAINED` として保持する。

成果物は元結果ID、標本と統計のSHA-256、群・測定項目・単位、標本での実数値、反例になる条件、未提示の母集団・観測時点・採用契約を保持する。通常の関係推論が参照するFact・Ruleとは別のケース成果物に置く。「前回の仮説を説明して」は保存済み成果物と元標本を再照合し、保証区分 `HYPOTHESIS` で採用保留の理由を説明する。許可された説明ファイルは実バイトと同一作用IDを照合してGoalへ帰還する。

Linux x86_64 6.18.57、Rust/Cargo 1.99.1。対象ソース差分SHA-256 `47f4172454eaf96eb7c847a62e91be8b7bab8ace1001fdd758a19494d05e372c`。最終検査は `rust/` から実行し、すべてexit 0。

| 検査 | 観測 |
|---|---|
| `cargo test -p minidora-engine --lib group_mean_hypothesis --locked -- --nocapture` | 2 PASS。乙の平均8を保持。前回結果なし、単群、同率、混合単位、別利用者を拒否。再起動後に同じ仮説を説明し、中断した作用を同一IDで再送せず照合。通常の事実照会は保留し、利用者のFact・Ruleは増えない。 |
| `cargo test -p minidora-engine --test core_condition_evaluation_57 --test core_construction_observation_02 --locked` | 5 PASS。通常の条件評価と反事実の隔離を含む。 |
| `cargo test -p minidora-engine --lib recovery::tests::ordinary_why_ --locked` | 2 PASS。B103の証明保存型学習と生成・復旧。 |
| `cargo test -p minidora-cli --test normal_route natural_group_mean_question --locked`、同 `agreeing_examples_do_not_promote_a_universal_empirical_rule` フィルタ | 3 PASSと1 PASS。既存の数値比較と一般化の抑止。 |
| `cargo build -p minidora-cli --locked`、続けて `minidora-rust --db … --case sample --actor alice --json --input …` を3プロセスで起動 | PASS。観測の計算、仮説保持、許可ファイルへの説明を順に実行し、ファイルと受領を同時に確認。 |
| `cargo check --workspace --all-targets --locked`、`cargo fmt --all -- --check`、`git diff --check` | PASS。 |

初回検査ではfixtureの表を配列で渡して既存契約の形状検査で停止した。`columns` と `rows` を持つ入力に修正して再検査した。

観測由来の仮説を別の通常要求で保持・説明する範囲を接続。再標本での実反例照合は未実装で、経験的採用契約は未提示。W12・W18・E1出口は `PARTIAL`、CAP-D05は `UNVERIFIED`、E2合同ゲート・体系QC・独立QAは `NOT_RUN`。

復旧2世代のローカル参照・GitHubタグは `4fffd49` と `aab4a86`。資産計7件。保持2件（この台帳と要求トレース）、保管済み0件、稼働中5件（domain、Input、Core、Recoveryと付属検査、共有Cargoキャッシュ）、ゴミ箱移動0件、保留0件。Windows保管は非該当。


## 2026-11-19 JST B105 — D3-C E1-W12 再標本を保持済み仮説と照合して反例を記録

着工点は `dc0e7e5158c8a87ab0c3eb51f3b49af1abd4e496`。D2仕様・D2工程・D3-C指示の採用SHAを照合。利用者が同じケースで保持したグループ平均の仮説に対し、「前回の仮説を次の標本で照合して:」と改行後のJSON表を受け付ける。Coreが元の封印入力、統計結果、仮説を再照合し、新標本の同じ列・群集合・単位を確認する。各群の厳密値と件数の組が元標本と等しい場合は、行順や数値表記の違いにかかわらず再利用として停止する。

同じ群が単独首位の場合は `MATCHED_ON_SAMPLE`、同率または別の群が首位の場合は `COUNTEREXAMPLE_ON_SAMPLE` を別の版付き成果物に記録する。元仮説への参照、新標本・集計への参照、実際の平均、反例となった集計への参照を保持する。後続の「前回の仮説を説明して」は直前の照合結果と標本を再計算して表示する。独立採取は未確認で、元仮説は `HYPOTHESIS_RETAINED`、結果の保証は `HYPOTHESIS`。通常の関係推論のFact・Ruleとは分けて保持する。

Linux x86_64 6.18.57、Rust/Cargo 1.99.1。対象ソース差分SHA-256 `6b89de75324f3f0a4d1c3acd14548a8d69f35beb3cc3ff60b02371425bbdd140`。最終検査は `rust/` から実行し、すべてexit 0。

| 検査 | 観測 |
|---|---|
| `cargo test -p minidora-engine --lib group_mean_hypothesis --locked -- --nocapture` | 4 PASS。単独首位の一致、厳密値 `17/2` と `8.5` の同率反例、再梱包した元標本・群と列と単位の不一致・出力権限不足を確認。照合成果物の差し替えは復旧で拒否。同一作用IDで既存ファイルを読み戻し、更新時刻・元仮説・Fact/Ruleを保持。 |
| `cargo test -p minidora-engine --lib recovery::tests::ordinary_why_ --locked` | 2 PASS。証明保存型の合成規則、生成、復旧を再確認。 |
| `cargo test -p minidora-engine --test core_construction_observation_06 --locked`、`cargo test -p minidora-engine --lib local_file --locked` | 1 PASSと2 PASS。権限・予算・取消・未知結果の停止と、発行期限・二重取得の拒否。 |
| `cargo test -p minidora-cli --test normal_route natural_group_mean_question --locked`、同 `agreeing_examples_do_not_promote_a_universal_empirical_rule` フィルタ | 3 PASSと1 PASS。既存の群平均比較と経験的な一般化の抑止。 |
| `cargo build -p minidora-cli --locked`、続けて `minidora-rust --db … --case sample --actor alice --json --input …` を4プロセスで起動 | PASS。提示した標本で首位が乙から甲へ変わる反例を計算し、許可ファイルの実バイトと受領を照合。別プロセスの説明でも同じ結果を再生。 |
| `cargo check --workspace --all-targets --locked`、`cargo fmt --all -- --check`、`git diff --check` | PASS。 |

W12のグループ平均経路は、観測、仮説保持、新標本による反例、再起動後の後続説明まで接続。母集団・独立採取・時点・期限・経験的採用契約は未提示。CAP-D05は `UNVERIFIED`。W18のE1出口に必要な同一版での循環A/Bと停止経路の結合判定、E2合同ゲート、体系QC、独立QAは `NOT_RUN`。

復旧2世代のローカル参照・GitHubタグは `dc0e7e5` と `4fffd49`。資産計8件。保持2件（この台帳と要求トレース）、保管済み0件、稼働中6件（domain、Input、Core、Document、Recoveryと付属検査、共有Cargoキャッシュ）、ゴミ箱移動0件、保留0件。Windows保管は非該当。


## 2026-11-19 JST B106 — D3-C E1-W18 結合判定 CORE_SHAPED

着工点は `75ac7b39c4e9d66fec8580438a784d81398a9d7f`。D2仕様・D2工程・D3-C指示の採用SHAを照合。Coreは関係説明の元Goalに `RELATION_PROOF_EXPLANATION_V1`、グループ平均の仮説結果に `GROUP_MEAN_HYPOTHESIS_RESULT_V1` を封印する。採用時に実際の証明・標本の計算から得た保証区分、参照、必須内容を計算成果物とContentPlanへ結び付ける。復旧では同じ証明または標本を再検証し、従来の述語列も受理する。

通常実行の最終 `COMMIT` では、元Goalの単調時計の締切をSQLiteの書込トランザクション取得後と確定直前に照合する。期限到達時は `SUSPEND/DEADLINE` とし、既に確認した実ファイルと受領を保持する。同期I/O呼出し中の割込みと実時間上限の測定は後工程の対象。

Linux x86_64 6.18.57、Rust/Cargo 1.99.1。検査対象ソース差分SHA-256 `7e345287511cc5b5cf6082b8759feb8349bb520a0c24288fe552179056c90ede`。同一の差分で以下を実行し、すべてexit 0。

| 結合点 | 実値の接続と検査 |
|---|---|
| I-A | `cargo test -p minidora-engine --test core_condition_evaluation_57 … --locked` の条件付き二前提1件と残り3件、`core_construction_observation_02` 1件。TRUEの条件と両前提が、型付き証明・意味単位・候補分布・生成文面を通り、説明Goalを封印した実ファイルの受領へ到達。別scopeと競合時は採用前に停止。 |
| I-B | `cargo test -p minidora-engine --test core_proof_composition_learning_49 --locked` 1件と、同 `--lib` の `recovery::tests::ordinary_why_` 2件、`group_mean_hypothesis` 4件。規則形成前後で計算経路・候補分布・実作用に差があり、再起動後の別ケースで使用し、失効後の再使用を停止。観測由来の仮説は別経路で反例照合と説明を実行し、旧述語のfixtureも同一作用で照合。 |
| I-C | 同 `--lib` の `recovery::tests::calculation_sequence_reconciles` と `recovery::tests::exact_arithmetic_goal_requires_the_sealed_value` 各1件。通常CLIの `cargo test -p minidora-cli --test normal_route calculation_sequence_route --locked` 5件、同 `why_question` 2件。途中の厳密値を後段が実消費し、指定ファイルの実バイト・受領で元Goalを確定。 |
| I-N | `cargo test -p minidora-engine --test core_construction_observation_06 --locked` 1件と、同 `--lib` の `recovery::tests::sequence_cancel`、`local_file_deadline_stops`、`recovery::tests::local_file_attempt_cannot_be_acquired_twice` 各1件。条件検査、別actor、許可不足、結果不明、取消、二重取得、期限を照合。6つの期限到達点を再現し、最終保存前に失効しても `COMMIT` 行0、確認済み受領は保持。 |

固有述語の封印、既存SQLite、Core内の二循環、実結果帰還、正停止を同版で検査した25件がPASS。同じactorと同じSQLiteを共有するCLI 11回の独立起動でも、規則形成前の保留から別ケースの説明ファイル確定、仮説への反例、連続計算の読み戻し・再送防止、無許可停止を確認した。3件の許可案件で保存された固有述語と各作用1件を照合し、保留・拒否の2件では作用0。 `cargo check --workspace --all-targets --locked`、`cargo fmt --all -- --check`、`git diff --check` もPASS。

D3-CのE1出口を `CORE_SHAPED` と判定。同じEngineに二循環と必要な停止経路が接続した観測に基づく。E2の7合同ゲートはこの版で次に実行する。全要求の受入、同期I/Oの実時間、経験的採用契約、体系QC、独立QAは未判定。E02・E04・CAP-D05の受入列は `UNVERIFIED`。

復旧2世代のローカル参照・GitHubタグは `75ac7b3` と `dc0e7e5`。資産計7件。保持2件（この台帳と要求トレース）、保管済み0件、稼働中5件（Core、Store、Recoveryと付属検査、条件の結合検査、共有Cargoキャッシュ）、ゴミ箱移動0件、保留0件。Windows保管は非該当。


## 2026-11-19 JST B107 — D3-C E2 同版合同判定 6/7

着工点は `4c29d141bfd436f02fdffa0fc10d4920e22cf9e0`。E1の `CORE_SHAPED` とD2仕様・D2工程・D3-C指示の採用SHAを照合。E2の再判定で二つの欠陥を再現した。scope付き引用が通常前提に昇格し、許可ファイルに出力できた。二前提の関係照会を復旧すると、元の証明が有効な場合でも、以前の依存形式を使って `RECOVERY_PROOF_INVALIDATED` としていた。旧版で再現した二件の回帰テストはいずれもexit 101。

新規の解析は `declarative-ja-en-v84-quoted-relation-boundary`。scope除去後の原子と規則内の引用を通常前提として解釈せず、原文を封印して停止する。復旧は記憶版を固定した関係結果の全証明依存を照合し、同じ作用IDで既存ファイルを確認する。初回の条件版差fixtureに不足した述語引数を補い、最終検査を実行した。

Linux x86_64 6.18.57、Rust/Cargo 1.99.1。最終検査のソース差分SHA-256は `9d815facf0a14199a0328f2858a1dc44c5d7cbe69e7860438ffb91afe11cf2a3`。同一ソース版、隔離SQLite、固定入力・actor・権限・予算で判定した。G07の再生用コピーでは元のGoal・証明・モデル・系列と作用を共通にした。

| ゲート | 判定 | 最終版の観測と根拠 |
|---|---|---|
| CORE-G01 | PASS | 主体、極性、時点、scope、条件の四値と引用を変動。scope付き引用と規則内引用は採用・描画・作用なし。同じ非引用の前提を明示すると、そのFactと規則に依存する説明が成立。別actorの先行結果を拒否。 |
| CORE-G02 | PASS | 二前提・欠落・矛盾・別scope・反事実を比較。同じscopeに無関係Factを加えても証明ID、採否、ContentPlan、描画が一致。反事実は一時評価で、通常記憶を保持。 |
| CORE-G03 | PARTIAL | 初見の条件付き証明は型付き意味とL6の新しい文面を経てK7で採用。通常CLIの関係比較では実成果物に文面化済みの `content_request.units[].text` と構成結果が入り、算術では厳密値 `7/2` を定型に整形していた。比較・数量・書式を型付き意味からL6へ通す条件が未達。 |
| CORE-G04 | PASS | 訂正と失効で依存する結果だけが変わり、旧結果と修正前の根拠を保持。別ケースの後続入力と再起動から規則・仮説の参照を実消費。 |
| CORE-G05 | PASS | 合成前後の新しい主体で証明経路が短縮し、別ケースで生成分布と実作用に差。scope外・失効は適用を停止。標本の反例は保持済み仮説と照合し、経験的採用は保留。 |
| CORE-G06 | PASS | 封印した厳密値、後段の実消費、物理fileとreceiptを確認。成果欠落、無許可、取消、UNKNOWN、期限で停止。同じ作用を再実行せず、固定記憶版の二前提回答も復旧。 |
| CORE-G07 | PASS | 同一の実作用journalの中断点を二つの隔離SQLiteに複製し、CLIを別プロセスで起動してGoal・採否・依存・生成系列・計画・作用IDを再照合。実ファイルの内容と更新時刻を保持。証拠版、条件記録だけの失効、同じ証明でのモデル差を個別に検査。異なる実モデル参照だけへの差し替えを復旧で拒否。 |

`rust/` から実行した最終検査はすべてexit 0。Cargoのテスト結果は以下の同版92件。G03の未達は実成果物で別途観測した。

- `cargo test -p minidora-engine --locked --no-fail-fast` に `--test` を指定したG01系7ターゲットで11 PASS。 `core_condition_evaluation_57`、`core_condition_scope_53`、`core_scope_polarity_54`、`core_temporal_scope_51`、`core_temporal_correction_52`、`core_scope_classification_58`、`core_actor_context_boundary_48`。
- 同命令で10ターゲット11 PASS。 `core_construction_observation_02`、`core_conjunctive_proof_55`、`core_atomic_explanation_50`、`core_atomic_relation_comparison_36`、`core_meaning_composition_03`、`core_construction_observation_04`、`core_construction_observation_05`、`core_proof_composition_learning_49`、`core_construction_observation_06`、`core_construction_observation_07`。
- `cargo test -p minidora-engine --lib <filter> --locked` は `recovery::tests::` の11フィルタで15 PASS。 `scoped_quotes`、`live_relation_query_reconciles`、`ordinary_why_`、`group_mean_hypothesis`、`calculation_sequence_reconciles`、`exact_arithmetic_goal_requires_the_sealed_value`、`sequence_cancel`、`local_file_deadline_stops`、`local_file_attempt_cannot_be_acquired_twice`、`missing_and_different_files_stay_unknown`、`authorized_cancel_stops_pending`。同じ命令で `input::tests` 39 PASS、`meaning_compare::tests` 9 PASS。
- `cargo test -p minidora-cli --test normal_route <filter> --locked` は `calculation_sequence_route` 5 PASS、`why_question` 2 PASS。 `cargo build -p minidora-cli --locked` 後、CLIの `reconcile` を独立した2プロセスで実行し同一結果を照合し、通常の関係比較と厳密算術の保存成果物を確認した。
- `cargo check --workspace --all-targets --locked`、`cargo fmt --all -- --check`、`git diff --check` がPASS。

E2は6/7のため未通過。G03はW07〜W09へ戻し、まず関係比較で左右の型付き根拠と比較結果を意味計画からL6へ渡す。数量・書式も同じ要件で再判定する。E3、体系QC、独立QAは `NOT_RUN`。

復旧2世代のローカル参照・GitHubタグは `4c29d14` と `75ac7b3`。資産計7件。保持2件（この台帳と要求トレース）、保管済み0件、稼働中5件（Input、Recoveryと付属検査、条件と二前提の検査2件、共有Cargoキャッシュ）、ゴミ箱移動0件、保留0件。Windows保管は非該当。

## 2026-11-19 JST B108 — D3-C E2差戻し W07〜W09 関係比較の意味生成

着工点は `5443b970fb59be6658d8a0d18425fbccac9fb9d8`。D2仕様・D2工程・D3-C指示の採用SHAを照合した。唯一のCoreが、通常の保存関係比較で左右の有効なinventoryから、共通・左側の支持・右側の支持・競合を型付き計画に保持する。各行は左右別のFact IDと証明依存を持つ。比較軸は述語、目的語、極性、範囲、時点、条件参照で区別する。条件参照の値と、評価に使った保存記録のIDをそれぞれ保持する。

比較の封印版は `declarative-ja-en-v85-generated-relation-comparison`、元Goal述語は `GENERATED_RELATION_COMPARISON_V1`。Coreは型付き比較計画から意味単位と局所語彙の選択肢を作り、既存L6で箇条書きを生成する。K7は必須単位・左右の証明参照・生成モデル・出力観測を照合し、`minidora.relation-comparison-content.v5` を採用計画と実描画へ接続する。保証区分は保存記録の範囲を表す `HYPOTHESIS`。競合は保留し、片側の支持未確認から関係の不存在を導かない。通常の追問では両側の根拠を再計算し、再生した比較と保存モデルを確かめる。旧v1〜v4および直接型付きAPIの保存形式を読み取れる。

比較生成は既存モデルの16,384バイト・トークンの上限と要求の証拠予算で停止する。説明向けの16単位・1単位384バイトを超える実比較を確認した。実CLIで14軸のすべてを、384バイトを超える共通関係の両値とともに採用した。規則と複数前提を含むCLI fixtureは既定の予算32で上流の証明投影が停止した。32での非採用を明示検査し、成功側は128、14軸は256を使用した。製品の既定値は32。

Linux x86_64 6.18.57、Rust/Cargo 1.99.1。最終検査のソース差分SHA-256は `58e36831b7e21543bc57885e48a82e53a274ffd88e810f20fdd8ffa77e5d99d6`。隔離SQLiteを使い、CLIの実表示、保存された比較計画と生成系列、許可したローカル出力を確認した。

- `cargo test -p minidora-cli --test normal_route relation_comparison --locked` で通常比較3件がPASS。両側の支持、片側の支持、競合、別actor、範囲外、予算不足、要約、詳説、証拠失効、14軸と長い共通関係を検査した。
- `cargo test -p minidora-engine --lib relation_comparison --locked` で5件がPASS。実ファイル作成後に中断点を再現して再起動し、同じ作用ID・採用計画・実ファイルの更新時刻を確認した。別の真正なモデル参照への差し替えと前提失効は拒否した。
- `cargo test -p minidora-engine --lib input::tests --locked` で39件、同 `ordinary_why_` で2件がPASS。 `cargo test -p minidora-engine --locked --no-fail-fast` の対象6統合ターゲットで6件がPASS。対象は `core_atomic_relation_comparison_36/38/43`、`core_temporal_scope_51`、`core_condition_scope_53`、`core_atomic_explanation_50`。通常比較で同じ述語・対象でも別条件を左右に分け、条件記録IDが生成の依存に入ることを確認した。
- `cargo test -p minidora-cli --test normal_route calculation_sequence_route --locked` で5件がPASS。 `cargo check --workspace --all-targets --locked`、`cargo fmt --all -- --check`、`git diff --check` がPASS。

局所判定は関係比較のL6とK7接続が `NORMAL_PATH_PRESENT`。G03は数量と利用者指定書式の型付き生成が残るため `PARTIAL`。B107の6/7は当該版の履歴であり、B108のE2合同7ゲート再判定は `NOT_RUN`。E3、E7体系QC、E8独立QA、公開・配布も `NOT_RUN`。次は算術の封印済み厳密値を生成・採用へ渡す。

復旧2世代のローカル参照・GitHubタグは `5443b97` と `4c29d14`。資産は計9件。保持2件（実装台帳と要求トレース）、稼働中7件（Core、Input、Language、Recovery、通常CLI検査、条件付き検査、既存の共有Cargoキャッシュ）、保管済み・ゴミ箱移動・保留は各0件。Windows保管と回収は非該当。

## 2026-11-19 JST B109 — D3-C E2差戻し W07〜W09 算術の厳密値の意味生成

着工点は `cafcf1ccbfff07e8a9cb410b78a85429e26718a5`。現行D2仕様・D2工程・D3-C指示の採用SHAを照合した。通常の単一式 `Calculate` は `declarative-ja-en-v86-generated-exact-calculation` で封印し、`EXACT_CALCULATION_V2` を元Goalに求める。唯一Coreが厳密計算した `Calculation` の式・有理数・単位を別々の必須値としてL6へ渡す。表示は従来の等式を先頭に保ち、出力予算に収まる場合は符号と整数・分数・単位付き数量の区分も生成する。語句選択では複数候補の条件付き分布を記録し、式・数値・単位は各一つの確定トークンとしてバイトを保持する。等式だけが入る予算では、厳密値の後にEOSで終わる。等式も入らない場合は停止する。

K7は封印式から再計算した `Calculation`、式・有理数・単位のトークン、生成モデルと採用出力を照合する。計算成果物 `minidora.exact-calculation-content.v1` は意味計画と生成系列を保存し、`ContentPlan` と実描画がその成果物を消費する。復旧は同じ生成系列と参照を再検証して既存の作用IDを照合する。旧 `EXACT_CALCULATION_V1` と従来の必要述語列の保存結果は旧出力で復旧できる。詳説・訂正・連続計算は保存された型付き計算値の既存経路を使う。文字列置換の検査は実際の前回出力を使って対象文字列以外のバイトも保全した。

Linux x86_64 6.18.57、Rust/Cargo 1.99.1。対象差分SHA-256は `d2fe3ab3d2e4173a513ff7b5792e59e2cd914ce154aa0142a14361b47b9ebc22`。通常CLIと隔離SQLiteで、負の有理数・単位付きの負の数量・式中のフォームフィード・8,001桁の整数について実ファイル、必要値トークン、語句選択、EOS、確認済みreceiptを照合した。確定値をまとめて記録するため、桁数に比例して詳細な次トークン分布を複製しない。既存モデルの16,384バイト上限を適用する。

- `cargo test -p minidora-cli --test normal_route <filter> --locked` の `scalar_arithmetic_generation`、`calculation_followup`、`mixed_unit_arithmetic`、`explicit_operand_correction`、`calculation_sequence_route`、`natural_previous_answer_edit`、`authenticated_http_and_cli_share_actual_case_result`、`relation_comparison` で16件PASS。
- `cargo test -p minidora-engine --lib recovery::tests:: --locked -- --test-threads=5` で64件PASS。数値・モデルの差し替えとGoalの格下げを拒否し、旧版と新版の作成済みファイルを同一作用IDで照合。期限、取消、無許可、結果不明、二重作用の境界も確認した。
- `cargo test -p minidora-engine --test input_machine_observation_01 --locked` で1件、`cargo test -p minidora-engine --lib <filter> --locked` の `input::tests` 39件と `language::tests` 13件がPASS。
- `cargo build -p minidora-cli --locked` 後、`python3 tools/previous_text_edit_route_recovery_test.py --binary <built-cli>` がPASS。別CLIプロセスで前回算術出力から作った置換結果を復旧し、確認済みreceiptとファイルのinode・更新時刻・サイズの一致を確認した。 `cargo fmt --all -- --check`、`cargo check --workspace --all-targets --locked`、`git diff --check` がPASS。

通常の単一式算術のL6とK7接続は `NORMAL_PATH_PRESENT`。数値は本束の範囲で成立し、利用者指定書式が残るためG03は `PARTIAL`。E2の7ゲート合同再判定、E3、E7体系QC、E8独立QA、公開・配布は `NOT_RUN`。

復旧2世代のローカル参照・GitHubタグは `cafcf1c` と `5443b97`。資産は計10件。保持2件（実装台帳と要求トレース）、稼働中8件（Core、Input、Language、Recovery、CLI検査、入力観測検査、前回答編集の復旧検査、既存共有Cargoキャッシュ）、保管済み・ゴミ箱移動・保留は各0件。Windows保管と回収は非該当。

## 2026-11-19 JST B110 — D3-C E2差戻し W07〜W09 関係比較の指定書式

着工点は `b31fad20563e20af513add86c47946f4248abf7e`。現行D2仕様・D2工程・D3-C指示の採用SHAを照合した。旧CLIでは同じ二対象・範囲の比較を指定なしで採用し、「段落で」「箇条書きで」を加えた要求はexit 2で停止した。

通常の日本語比較「甲と乙の保存済みの関係を段落で比較して」と同じ箇所に「箇条書きで」を指定する構文を受け付ける。封印版 `declarative-ja-en-v87-formatted-relation-comparison` は書式の原文バイト範囲と型付き `Intent.format` を保存する。元GoalのIntentとCoreの比較意味計画の両方に同じ指定を保持する。同一Actor記憶版・範囲・左右の証明から、L6は一段落または一行一項の箇条書きを生成する。K7は意味・根拠・生成モデル、指定の書式と実バイトを照合する。明示指定の保存形式は `minidora.relation-comparison-content.v6`。指定なしのv5は既存箇条書きとして再生し、v1〜v4も既存経路で復旧できる。

段落からの詳説は検証済みの保存単位を消費し、左右の関係と根拠を保持する。旧CLIで未対応だった書式を両方確認し、表形式、送信を含む複合指示、実データのUnicode行区切りで指定を満たせない場合は非採用・無作用とした。復旧は元Goalの書式で系列を再生成し、Goalまたは比較成果物の書式差し替えを拒否する。許可済み段落ファイルの中断を同一作用IDで照合し、ファイルの内容・更新時刻を保持する。

Linux x86_64 6.18.57、Rust/Cargo 1.99.1。検査ソース差分SHA-256は `dc7d126b93ebd9dd08e5a5b9488b5b49d9e42af0c192f6567c6794f5d5f33ac0`。隔離SQLiteと許可した一時ファイルを使い、両書式の比較軸・左右の記録・根拠IDの一致と、L6の状態・モデル・文面の差を観測した。通常の実出力と受領済みreceiptを照合した。

- `cargo test -p minidora-cli --test normal_route requested_comparison_layout --locked` で1件PASS。通常の二書式、同じ証明、出力ファイル、段落の詳説、未対応の日本語指示、Unicodeの行区切りを検査した。同ターゲットの `relation_comparison` は3件、 `scalar_arithmetic_generation` は1件がPASS。
- `cargo test -p minidora-domain --locked` は7件、 `cargo test -p minidora-engine --lib input::tests --locked` は39件がPASS。
- `cargo test -p minidora-engine --lib recovery::tests:: --locked -- --test-threads=5` で64件PASS。段落の元Goalと保存書式の変更、モデル差し替え、前提失効を拒否。正しい元状態では生成済み内容と同一作用IDを再照合して処理を確定した。
- `cargo test -p minidora-engine --locked --no-fail-fast` の `core_atomic_relation_comparison_36/38/43`、 `core_temporal_scope_51`、 `core_condition_scope_53`、 `core_atomic_explanation_50` で6件PASS。旧保存形式、時間と条件、証明からの説明の回帰を確認した。 `cargo fmt --all -- --check`、 `cargo check --workspace --all-targets --locked`、 `git diff --check` がPASS。

G03の指定書式は日本語の保存関係比較の二書式で `NORMAL_PATH_PRESENT`。任意書式、他用途の書式変更、英語の明示書式は検査範囲外。説明・条件・比較・数値・書式の関連経路を当該版で検査した。次はG01〜G07を同一版で合同判定する。B110のE2合同判定、E3、E7体系QC、E8独立QA、公開・配布は `NOT_RUN`。

復旧2世代のローカル参照・GitHubタグは `b31fad2` と `cafcf1c`。資産は計10件。保持2件（実装台帳と要求トレース）、稼働中8件（Domain、Input、Core、Language、Recovery、CLI検査、旧形式の直接比較検査、既存共有Cargoキャッシュ）、保管済み・ゴミ箱移動・保留は各0件。Windows保管と回収は非該当。

## 2026-11-19 JST B111 — D3-C E2 同版合同判定 7/7

着工点は `1ae9099661b7653775b57ed084930a473c9ec031`。採用記録 `270b6482dff22cef7559363776f52806a0074fb0fd7fd0081b7772e62c2b9ef5` と D2仕様 `de618da9d50f248a7e984ad807aab4a306f17204eb9e722da387aa7d587573b0`、D2工程 `d1afc8a3e225fcbc21ef39eb3152ca9c64ffe79ee61d0702220802700570ce7a`、D3-C指示 `fa4bfbb6d4c897df943fcebe64e2eb1e766834fabcf7187d9e78598d7e7445e3` のSHA-256を照合した。適用範囲はE1 `CORE_SHAPED` を前提とするD3-C第3節の合同試運転と、その未達箇所の施工。

着工時の実CLIで「もしXは鳥ならXは動物」と「弥生は鳥」を保存した。「なぜ弥生は動物ですか」は旧Goalで出力を採用し、保存モデル `bounded-finite-surface-literal-exact-v1` は完成文候補2件を保持していた。旧Coreは合成規則を消費する通常質問だけを型付き生成へ渡していた。新規 `AskWhy` は `declarative-ja-en-v88-generated-relation-explanation` で封印し、元Goalに `RELATION_PROOF_EXPLANATION_V2` を要求する。直接根拠、規則導出、反対命題を支持する証明を、それぞれ型付き命題・証明・根拠IDとしてL6に渡す。K7が実系列と証明を照合し、同じ採用計画から描画と許可ファイルを確定する。保存したモデル・意味計画・EOSまでの系列を復旧で検証する。旧V1の完成文候補を使った実ファイルは保存済み採用計画と同一作用IDで復旧した。新規説明は既存モデルの16,384バイト上限を使い、旧封印の8,192バイト上限を維持した。

訂正後一覧の初回回帰検査は既定32で `RESOURCE_LIMIT`、exit 101。合計23アクションの一覧で、最初の証明投影に必要な4が均等配分で3に制限されていた。関係インデックスの一覧は封印済み合計から投影と評価の実消費を差し引き、全照会が揃ってから返す。既定32で訂正後の4件を採用し、不足する20では計画・受領なしで停止した。旧前提とそれに依存した導出が外れ、別scopeの同じ前提と無関係な色の前提は残った。旧訂正結果と履歴を保持した。

Linux x86_64 6.18.57、Rust/Cargo 1.99.1、製品版 `0.7.0-alpha.11`。最終ソース差分SHA-256は `6b672188aee71b9a94f06862765403930d75e18aae36e18877d06cb4060137f5`。六つのソース・試験ファイルの `git diff --binary` を対象とする。隔離SQLiteを使い、対照の原入力、主体、モデル、証拠版、元Goal、権限、予算を固定した。実出力はfixtureで許可したローカルファイルに限定した。

| ゲート | 判定 | 同版の実観測 |
|---|---|---|
| CORE-G01 | PASS | 主体・極性・範囲・時点・引用を分別した。条件IDを証拠Factと区別し、`TRUE`、`FALSE`、`UNKNOWN`、`CONFLICT` と `AND`、`OR`、`NOT` を評価した。未成立・未確定の条件に依存する採用を停止し、適用範囲内の証拠と条件記録IDを採用・描画に渡した。別actorの先行結果と引用由来の作用を拒否した。 |
| CORE-G02 | PASS | 二前提の根拠両方と規則IDを証明に保持し、欠落・矛盾・別scopeで依存する推論を停止した。無関係Factの追加で採否・採用計画・描画は同一。反事実の一時状態を通常記憶から隔離して証明を照合した。 |
| CORE-G03 | PASS | 初見の通常説明と条件付き証明、両側の根拠による比較、式・有理数・単位を含む算術に型付き意味とL6の生成系列を観測した。日本語の保存関係比較で同じ証拠から段落と箇条書きを生成し、Goalの指定とK7の採用、出力の実バイトを照合した。選択分布とEOSを含む保存モデルを再生した。 |
| CORE-G04 | PASS | 訂正・失効後の別入力と再起動で現行の記憶を消費し、旧記録と依存外の結果を保持した。訂正後の一覧は元の省略されていない証拠集合を読み戻し、作成済みファイルと同一作用で復旧した。 |
| CORE-G05 | PASS | 新しい主体で保存規則の合成により証明が2段から1段へ短縮した。別ケース・再起動で合成規則と親規則を消費し、同じ証明を使う対照との生成分布差と確認済み出力を観測した。根拠失効とscope外で停止し、標本の反例は保持中の仮説へ結び付けた。経験的採用は保留。 |
| CORE-G06 | PASS | 連続計算で厳密な途中結果を後段が消費し、封印された全必要成果と実ファイル受領が揃って確定した。一成果欠落、期限、無許可、取消、結果不明では停止し、重複作用を防いだ。 |
| CORE-G07 | PASS | 説明・段落比較・訂正後一覧で、実作用の `ATTEMPTED` までの履歴を複製し、それぞれ2プロセスでGoal、意味、依存、採用計画、描画、作用IDを照合した。説明と比較の生成系列を検証し、ファイルのinode・実バイト・更新時刻を保持した。証拠のみ、条件記録のみ、モデル参照のみを変えた対照では依存側だけの差または復旧拒否を確認した。 |

`rust/` で実行した最終検査はexit 0。Rustテスト計168件と独立CLI復旧を確認した。

- `cargo test -p minidora-engine --locked --no-fail-fast` の17統合ターゲットで22件。対象は `core_condition_evaluation_57`、`core_condition_scope_53`、`core_scope_polarity_54`、`core_temporal_scope_51`、`core_temporal_correction_52`、`core_scope_classification_58`、`core_actor_context_boundary_48`、`core_construction_observation_02/04/05/06/07`、`core_conjunctive_proof_55`、`core_atomic_explanation_50`、`core_atomic_relation_comparison_36`、`core_meaning_composition_03`、`core_proof_composition_learning_49`。
- `cargo test -p minidora-engine --lib --locked` の `recovery::tests::`、`input::tests`、`meaning_compare::tests`、`language::tests`、`indexed_premise_inventory`、`indexed_relation_profile` で127件。旧結果の読み戻し、固定記憶版、条件・モデル差、取消・無許可・結果不明、期限と二重作用を検査した。
- `cargo test -p minidora-cli --test normal_route --locked` の `why_question`、`ordinary_why_generates_a_long_premise`、`requested_comparison_layout`、`relation_comparison`、`scalar_arithmetic_generation`、`calculation_sequence_route`、`declared_rule_transfers`、`conjunctive_rule_joins`、`conditional_premises_form_reusable_scoped_proof`、`natural_premise_correction_invalidates`、`premise_inventory`、`relation_profile` で19件。450バイトの原子前提が根拠IDと実描画に残ることも確認した。
- `cargo build -p minidora-cli --locked` 後、独立CLIを3経路で各2回起動し、真正な履歴接頭部と出力から回復した。初回の照合スクリプトで成果物の参照位置を誤り、保存スキーマを読み直して最終再実行した。 `cargo fmt --all -- --check`、`cargo check --workspace --all-targets --locked`、`git diff --check` もPASS。

D3-CのE2出口を `CORE_GATES_PASSED` と判定した。次工程はE3の最小Input。今回の書式検査は日本語の保存関係比較の二書式。大きな履歴や並行更新下での固定版検査とインデックスの予算差は体系QCの範囲に残る。R2要求の受入列は `UNVERIFIED`。E3、E7体系QC、E8独立QA、公開・配布は `NOT_RUN`。

復旧2世代のローカル参照・GitHubタグは `1ae9099` と `b31fad2`。資産は計9件。保持2件（実装台帳と要求トレース）、稼働中7件（Core、Input、Language、Recovery、関係投影、CLI検査、既存共有Cargoキャッシュ）、保管済み・ゴミ箱移動・保留は各0件。Windows保管と回収は非該当。

## 2026-11-19 JST B112 — D3-C E3 最小Inputの封印と実接続

着工点 `9b95adb13c93325e62bcd0d0a9251b97b8208351` と同版のE2出口を確認した。現行正本のSHA-256はD2仕様 `de618da9d50f248a7e984ad807aab4a306f17204eb9e722da387aa7d587573b0`、D2工程 `d1afc8a3e225fcbc21ef39eb3152ca9c64ffe79ee61d0702220802700570ce7a`、D3-C指示 `fa4bfbb6d4c897df943fcebe64e2eb1e766834fabcf7187d9e78598d7e7445e3`。採用記録 `270b6482dff22cef7559363776f52806a0074fb0fd7fd0081b7772e62c2b9ef5` と第4節のE3出口を照合した。通常入力の原本・意味・許可を封印し、唯一Coreの採否へ接続する施工。

着工時CLIでは `〈ネラは測定器〉`、範囲指定後の `《…》`、全角引用符、引用ラベル、Markdown引用が前提として保存され、許可対象への実ファイルを作成した。「これら」と `They` も名前として保存した。比較・説明では範囲と主体または目的語が同じ場合、文字列から検索した位置が両方の役割に入った。`[time=…]` と `[condition=…]` が関係の主体の一部として解釈される経路も確認した。

新規入力は `declarative-ja-en-v89-sealed-relation-boundary`、関係説明は `declarative-ja-en-v90-explanation-source-roles`、関係比較は `declarative-ja-en-v91-comparison-source-roles` で封印する。比較の対象・範囲・書式と、日本語の原子質問・説明の命題・修飾は構文上の位置を原文のUTF-8バイト範囲に結び付ける。原文、原文SHA-256、actor、同一ケースの先行結果、要求した出力先と権限を保持する。日英混在の訂正では冠詞と対象語を分け、原文と対応しない訂正は記憶変更前に停止する。算術の全角入力から正規化した式と元バイト範囲を確認した。

対象を明記した単一の日本語コピュラ関係に `[scope=…]`、`[time=YYYY-MM-DDThh:mm:ssZ]`、`[condition=…]` を保持できる。時点は既存のUTC検証を使う。明示条件IDは既存Coreの条件定義と証拠に渡す。Inputは定義の生成も成立判定も行わない。定義・有効期間内の証拠・否定前提をCoreが実消費した説明は、型付き `CONTRADICTED`、実描画、正確な時点と根拠依存で確定した。期間外は `CONDITION_FALSE / REJECT`、定義不明は `CONDITION_UNKNOWN / SUSPEND`。どちらも計画・描画前に停止した。未対応の修飾付き訂正と不正日時も封印された未解決状態で停止した。

引用・資料内の命令と未解決の複数照応は通常の関係や実行命令に昇格せず、原文と診断を残して `UNKNOWN / SUSPEND`。許可済みファイルを要求しても、該当15入力の前提・計画・作用履歴・ファイルは0件。単数照応と「なぜですか」は同一ケースの確定済み先行回答を実消費。別ケースでは照応の根拠がなく停止し、別actorのケース参照は拒否した。明示書式の段落と箇条書きは同じ根拠で実出力し、未許可の出力先ではファイルを作成しなかった。

旧封印の出典は当時の型と原文範囲で検証する。着工点の実バイナリで保存したv84の前提と訂正を保持した状態で、新規の通常前提を採用できた。旧v88説明とv87段落比較は着工点のCLIで実ファイルを作成し、保存履歴の `ATTEMPTED` までを取り出して新版CLIで復旧した。採用計画、作用ID、ファイル内容・inode・更新時刻が一致した。最初の条件付き説明テストは固定表現による文面比較でexit 101。実際の否定命題と生成系列を成果物で確認する検査へ直し、合格述語を実データで照合した。

Linux x86_64 6.18.57、Rust/Cargo 1.99.1、製品 `0.7.0-alpha.11`。検査対象6ファイルの `git diff --binary` のSHA-256は `09438b9b61a175dbb0bd72484fc29c7fba5c6d3b15c150ec213a34fe4551776c`。隔離SQLite、各fixtureのactorとローカルファイル許可を使った。最終版のRustテストは計131件PASS。

- `cargo test -p minidora-engine --locked --no-fail-fast` の `input_machine_observation_01`〜`05` と `core_actor_context_boundary_48` で7件PASS。生の日本語、原文範囲、原文ハッシュ、正規化された厳密値、同版再起動、条件の期間と未定義、別actorを確認した。
- `cargo test -p minidora-engine --locked --lib` の `input::tests` と `recovery::tests::` で103件PASS。旧保存形式、引用、原文の保持、計算・証明・モデル・復旧の近接回帰を確認した。
- `cargo test -p minidora-cli --locked --test normal_route` の関連フィルタで21件PASS。初見説明、比較、訂正、引用、照応、前回答詳説・編集、実ファイル許可、同じ証拠を使う関係質問・紹介文を検査した。
- `cargo fmt --all -- --check`、`cargo check --workspace --all-targets --locked`、`cargo build -p minidora-cli --locked`、`git diff --check` がPASS。着工点と新版の二つの実CLIを使う旧封印復旧もexit 0。

原文保全・型・未解決状態を同Coreが実消費するD3-CのE3出口は `PASS`。時点と条件の新規修飾は対象を明示した日本語の単一関係に適用する。修飾付き訂正は停止を確認し、一般の日時・照応表現は未検証。次工程はE4最小Output。体系QC、独立QA、公開・配布は `NOT_RUN`。R2受入列は `UNVERIFIED`。

復旧2世代のローカル参照・GitHubタグは `9b95adb` と `1ae9099`。終了時資産は計9件。保持2件（実装台帳と要求トレース）、稼働中7件（Input、Core、Recovery、関係型検証、CLI検査、入力観測検査、既存共有Cargoキャッシュ）。保管済み・ゴミ箱移動・保留は各0件。隔離した旧版の複製と検査用状態は終了済み。Windows保管と容量回収は非該当。

## 2026-11-19 JST B113 — D3-C E4 最小Outputの描画と実受領

着工点は `63944c454a820b2e4d34ce2754ae6c97beadea4d`。E3出口と現行3点セットを照合した。SHA-256はD2仕様 `de618da9d50f248a7e984ad807aab4a306f17204eb9e722da387aa7d587573b0`、D2工程 `d1afc8a3e225fcbc21ef39eb3152ca9c64ffe79ee61d0702220802700570ce7a`、D3-C指示 `fa4bfbb6d4c897df943fcebe64e2eb1e766834fabcf7187d9e78598d7e7445e3`。採用記録は `270b6482dff22cef7559363776f52806a0074fb0fd7fd0081b7772e62c2b9ef5`。第4節に沿い、採用内容の実UTF-8描画と許可したローカルファイルの実受領を同Coreの元Goal判定へ渡す。

着工点の実CLIと認証付きループバックHTTPで、実バイト、指定ファイル、確認済み受領は一致した。描画観測には対象がなく、着工点の実バイナリと実採用計画による隔離検査で、`renderer_version` を空文字や未知の版へ変えても検証が成功した。根因はCoreの照合が計画ID、状態、実バイト、ハッシュまでだったこと。

新規の元Goalは `utf8-bytes-v2` を封印する。Outputは採用計画のUTF-8、バイトのSHA-256、計画ID、版、状態、および `DISPLAY` または正確な `LOCAL_FILE` 対象を返す。Coreは描画と元Goalの版・対象を照合し、表示用の描画は `EXACT_RENDER` の観測として使う。ファイルが必要なGoalは同じ実バイトに結び付いた作用ID・許可・実ファイル読み戻し・確認済み `DeliveryReceipt` を必須とする。作用発行時のStoreも採用済み描画の正確な対象とハッシュを確認する。既存の前回答編集は共有の描画検証を使う。

有効なハッシュで別途保存した描画観測について、未知の版、別ファイル、同じファイルを指す別表記、対象欠落、新規Goalへの旧版観測の差し替え、描画失敗の6例で `RECOVERY_RENDER_MISMATCH / SUSPEND` を確認した。元Goal、同じ作用ID、未確認状態、ファイルの内容と更新時刻を保持した。誤った対象の作用発行、既存の異なるファイル、無許可、予算不足、期限、取消、結果不明を検査し、未充足のGoalを確定しなかった。取消後に既発生の作用を読み戻せる場合も、元Goalの取消を保持した。

旧保存版 `utf8-bytes-v1` は、旧Goalの封印先と独立した受領を使う。旧版の省略項目を再シリアライズ時も省略し、既存の成果物ハッシュを保持する。着工点と修正版それぞれの通常CLIで作成した日本語説明ファイルを用い、実履歴の `ATTEMPTED` までの複製を各2プロセスで復旧した。同じ元Goal、採用計画、描画、作用ID、ファイル内容・inode・更新時刻を照合し、再送と新規作用は0件。

Linux x86_64 6.18.57、Rust/Cargo 1.99.1、製品 `0.7.0-alpha.11`。最終検査対象7ファイルの `git diff --binary` のSHA-256は `8559e54136cbbf1d9ec3ec027a2134652e665144e0893bda9b65756121939c2c`。隔離SQLite、許可済み一時ファイル、認証付きループバックHTTPを用いた。Rustテスト計124件PASS。

- `cargo test -p minidora-domain --locked` 7件、`cargo test -p minidora-engine --locked --test core_construction_observation_06` 1件、`cargo test -p minidora-engine --locked --lib` の `output::tests::`、`recovery::tests::`、`store::tests::` 101件がPASS。ファイルの排他作成、既存内容保護、予算・期限・受領・取消・復旧を検査した。
- `cargo test -p minidora-cli --locked --test normal_route` の近接15件がPASS。許可済みCLI・HTTPの実ファイル、CLIの表示、前回答との接続、比較の二書式、厳密値と連続計算、既存ファイル競合、暫定ストリームと最終結果を確認した。
- `cargo fmt --all -- --check`、`cargo check --workspace --all-targets --locked`、`cargo build -p minidora-cli --locked`、`git diff --check`、実CLIの旧版・新版の復旧がexit 0。復旧再実行の初回は旧版アーカイブの参照ディレクトリ指定で失敗し、リポジトリルートを指定して再実行した。

D3-CのE4出口は `PASS`。日本語UTF-8の採用内容の描画とローカルファイルの実観測が同Coreの元Goal判定へ帰還した。次工程はE5通常三機の合同検査。体系QC、独立QA、公開・配布は `NOT_RUN`。R2受入列は `UNVERIFIED`。

復旧2世代のローカル参照・GitHubタグは `63944c4` と `9b95adb`。終了時資産は計10件。保持2件（実装台帳と要求トレース）、稼働中8件（Domain、Core、前回答編集、Output、Recovery、Store、CLI検査、既存共有Cargoキャッシュ）。保管済み・ゴミ箱移動・保留は各0件。隔離した旧版複製と検査用状態は終了済み。Windows保管と容量回収は非該当。

## 2026-11-19 JST B114 — D3-C E5 通常入口の三機合同判定 3/3

着工点は `219e6a91147c5ac1aa648f397a543108850c841c`。E2合同7ゲート、E3最小Input、E4最小Outputの成立記録を確認した。D2仕様 `de618da9d50f248a7e984ad807aab4a306f17204eb9e722da387aa7d587573b0`、D2工程 `d1afc8a3e225fcbc21ef39eb3152ca9c64ffe79ee61d0702220802700570ce7a`、D3-C指示 `fa4bfbb6d4c897df943fcebe64e2eb1e766834fabcf7187d9e78598d7e7445e3`、採用記録 `270b6482dff22cef7559363776f52806a0074fb0fd7fd0081b7772e62c2b9ef5` を照合した。第4節に沿い、同一Coreの通常日本語入口でT01〜T03を合同判定する施工。

着工点の通常CLIで初見説明と段落比較は生成・実表示まで到達した。一方、明示条件IDの正例は `CONDITION_DEFINITION_MISSING` で実採用に届かず、条件定義は直接型付きAPIに限られていた。数学エンジンが扱うUnicodeマイナス `−` は単位付き算術のInputで停止していた。実Chromiumでは前後の全角空白を含む依頼が、送信前の `trim` により変更されていた。

通常の `条件定義: [scope=lab][condition=calibrated]ユリは校正済み` を `declarative-ja-en-v92-condition-definition` で封印し、既存の `Intent::DefineCondition` と唯一Coreの条件記憶に渡す。定義の対象は明示主体の単一コピュラ関係とその否定。条件ID、範囲、主体、対象語を原文の別々のバイト範囲に保持し、Coreが条件記録ID・証拠Fact・真偽を扱う。条件の複合式と有効期間の通常文による定義はこの施工範囲外。不正な修飾、重複した条件ID欄、引用、疑問形は定義前に停止する。単位算術の字句検査は `−` を通し、既存の厳密演算と元式を消費する。ブラウザーは空入力の判定に空白除去を使い、送信する依頼文には入力欄の値をそのまま使う。

Linux x86_64 6.18.57、Rust/Cargo 1.99.1、製品 `0.7.0-alpha.11`。最終検査対象3ファイルの `git diff --binary` SHA-256は `330f1dacb74100ccc48c022df280c9c885f64717cb3c56701bc143cf0583f137`。通常CLI・認証付きループバックHTTP・Chromiumは同じ実行ファイルを使い、そのSHA-256 `9f207a4261812e3f8a1a27ea1cc0dd9d5256c2d5ae79e7cc4e0431ade76feadf` が各検査の前後で一致した。隔離SQLite、明示した一時ファイルの許可、日本語入力を使用した。

| ゲート | 判定 | 同版の通常入口からの実観測 |
|---|---|---|
| T01 | PASS | 独立CLI 15起動。葉月の規則説明は760 UTF-8バイト、葉月と楓の段落比較は843、明示した否定条件の説明は948、全角数字・`−` を含む厳密量 `-3/2 m` は79。型付き意味成果物、新しい文面の選択分布、EOS、実stdout、描画ハッシュ、元Goalの `COMMIT` を照合した。別の正例で通常定義の登録前は未定義停止、登録後は `TRUE` と証拠依存を含む実説明、同じ範囲の証拠訂正後は `CONDITION_FALSE` で描画前に停止した。Chromiumで前後の全角空白を含む依頼の原文・SHA-256・バイト範囲と、生成・描画・DOMの一致も確認した。 |
| T02 | PASS | 独立CLI 25起動。規則なしと親規則1件では未成立、2件保存後は別ケース・未見主体セイタカシギで `DERIVED / COMMIT`。実成果物V7に合成規則と親2件、対象のFact、生成モデルの実使用を確認した。802ステップのうち23で複数候補があり、EOSへ到達。別範囲・別actor・訂正後は依存する推論だけが停止し、無関係な証明と旧結果を保持した。Chromiumの別ケースとサーバー再起動でもV7を消費した。標本由来の河口13の仮説は、別標本の干潟20を反例として記録し、後続説明がその観測を消費した。仮説は保持状態のままで、事実照会は停止した。 |
| T03 | PASS | 独立CLI 20起動。日本語の純表示と許可済み実ファイル45バイトをそれぞれ確定し、描画・受領のハッシュ、元Goalの必要成果4件、作用1件を照合した。許可不足や既存内容の競合では `SUSPEND`。真正な `ATTEMPTED` までの履歴で、対象不明は `UNKNOWN` のまま停止し、取消は確定可能な既発生効果を再照合しても `CANCEL` を保持した。新規作用・再送0。認証付きHTTPの無許可停止と許可ファイルの実読戻し、再試行でのinodeと更新時刻の一致、Chromiumの取消も確認した。 |

最終版のRustテスト計140件がexit 0。`cargo test -p minidora-cli --locked --test normal_route` の近接27件、`cargo test -p minidora-engine --locked --no-fail-fast` の `input_machine_observation_05`、`core_proof_composition_learning_49`、`core_construction_observation_06` 計4件と、同 `--lib` の `input::tests`、`recovery::tests::`、`output::tests::` 計109件。`cargo fmt --all -- --check`、`cargo check --workspace --all-targets --locked`、`cargo build -p minidora-cli --locked`、`git diff --check` がPASS。独立CLI検査とChromium・HTTP観測もexit 0。ローカルファイル作用は通常CLIと認証付きHTTPで検査した。ブラウザー操作は表示・学習・取消を対象とした。

D3-CのE5出口は同一版の通常入口で `PASS 3/3`。次工程はE6の全体系接続と要求・元ソースの意味照合。体系QC、独立QA、公開・配布は `NOT_RUN`。R2受入列は `UNVERIFIED`。

復旧2世代のローカル参照・GitHubタグは `219e6a9` と `63944c4`。終了時資産は計6件。保持2件（実装台帳と要求トレース）、稼働中4件（Input、CLI検査、ブラウザーの入力画面、既存共有Cargoキャッシュ）。保管済み・ゴミ箱移動・保留は各0件。隔離SQLite、一時検査スクリプト・出力、ブラウザープロファイルは残っていない。Windows保管と容量回収は非該当。

## 2026-11-19 JST B115 — D3-C E6 検索候補と取得本文の復旧

着工点は `d7b543c71fbc3d53273a9b631dba0515087efef6`。E5の3ゲート成立とD3-C §5・§5-C・§5-H、R2 §19・§20を照合した。今回の施工はC04・C13のうち、通常検索と同一ケースの順位指定本文取得から許可済みファイルを出力した際の復旧を対象とする。

着工点の実CLIでは、検索一覧、1件目と2件目の本文、ファイル出力までは確定した。実作用履歴を `ATTEMPTED` まで残し、別プロセスで `reconcile` すると終了2の `RECOVERY_EVIDENCE_MISMATCH`。復旧が本文1件と単独の本文ノードを前提とし、通常処理が保存していた一覧と複数本文を扱えなかった。

復旧は元Goalに封印されたグラフから当時の保存形式を判定する。検索一覧の出典と元順位、引き継いだ本文の正規バイト列と順序、今回の選択本文の参照・URI・読取許可・証拠上限・出力単位を照合する。復旧成果物に `SearchObservation → 選択本文 → compute` の実参照と、読み戻したファイルの受領を保存する。旧形式の本文単独グラフも扱う。移行済みのケースでは、新しく追加する復旧版に現行スキーマを付け、元のケース版・封印入力・Goalを保持する。

元ソース照合は、原版 `9307d7e2b1799750ecef1957d85128fcbe403d02` の公開ツリーとアーカイブからソース414件、テスト359件のパス・Git blob・内容SHA-256を確認した。凍結ベータ `1c546e77007afa0a9eeb3a44f07da99d84c383eb` のソース416件、テスト147件も現物と照合した。テストの和集合は360パス。旧選抜313回帰は今回未実行。全文読解台帳は過去96件にB08の2件を加えた98件から、版・要求・候補順・既存本文を検証する `知識取得継続.py` と、当回の通信開始を含む資源を計上する `取得実行制御.py` を追加して100件になった。元の続行カーソル、原要求を保持した累積上限、候補消費の不成立記録、検索と本文に共通の通信開始計数は別途照合が必要。`03_REQUIREMENT_TRACE.tsv` のC04・C13に今回の部分経路を記録した。

Linux x86_64 6.18.57、Rust/Cargo 1.99.1、製品 `0.7.0-alpha.11`、Python 3.12.14、OpenSSL 3.5.7。実装と試験2ファイルの `git diff --binary` SHA-256は `df12a1c8f083d3a3ebbd7259cb1f2e6bfd9e15aa6c49f33ddac7be0ff9fa8b36`、検査実CLIのSHA-256は `7eba9eea2addb270643ade7e7d1e85badcb89a6ecd33f6f8c47f2e2d50d5898f`。隔離SQLite、明示した読取ホストと出力先の許可、専用CAで検証するループバックTLSを使用した。外部プロバイダーへの通信は0件。

- `cargo fmt --all -- --check`、`cargo check --workspace --all-targets --locked`、`cargo build -p minidora-cli --locked` はexit 0。Rustテストは `minidora-engine --lib recovery::tests::` 65件、`search::tests::` 11件、`minidora-cli --test normal_route` の `natural_source_question` 4件と `constrained_previous_source_summary` 1件、計81件PASS。
- `python3 tools/local_tls_observer_test.py --binary <compiled-cli>` はexit 0。既存のTLS 8件と資料要約復旧に加え、通常検索→1件目本文→2件目本文出力→別プロセス復旧の再現手順を収録した。元の実行グラフと全消費参照を照合し、復旧のファイルノードは実際の読戻しで得た受領を指す。初回は受領が変わってもグラフ全体のハッシュが同じと仮定して検査が失敗したため、元のノード・消費参照と復旧受領をそれぞれ照合する検査に修正した。
- 最終CLIで引き継ぎ本文が0件と1件のいずれも `COMMIT`。元の採用計画・証拠順・作用IDと、ファイルの内容・inode・更新時刻が一致した。TLS観測は初回2件、続行3件のGETのみで、復旧時の追加取得・再送は0件。保存一覧を欠落させた場合と、選択本文の参照を別順位に差し替えた場合は、保存物のハッシュを整合させても `RECOVERY_EVIDENCE_MISMATCH` で終了2。保存済み一覧の実バイトが壊れた場合は完全性検査で終了1。負例でも現物の書換えは0件。
- 旧版は `9f11d2a1f0e41f1fe8bfd30e87c542ff8505c075` のCLIをビルドし、単独本文の実ファイルとalpha.8ジャーナルを作成した。実行ファイルSHA-256は `3bca68263aee688add91a6c3846853326edf4cc4647a4b86da03323bc36a708c`。隔離コピーで現行CLIの `migrate` 後、最初の復旧は新しいケース版に旧スキーマが残って終了1の `ACCESS_DENIED`。版の生成元を修正し、新しい隔離コピーで終了0。旧 `ADOPTED` の保存バイト、元Goal、封印入力、元の作用IDを保持した。同じ作用の `ATTEMPTED → UNKNOWN → CONFIRMED` を確認し、現物の内容・inode・更新時刻と2件の元GETを照合した。

C04・C13は今回の通常経路と復旧を `PARTIAL` と記録。外部プロバイダー実通信、全障害・再試行の組合せ、広い要求の受入、E6出口、体系QCと独立QAは未検証。次の欠落は、通常検索の読み取りが元Goalで必須観測として計上されない経路。検索と本文の共有資源計数は別途照合する。

復旧2世代のローカル参照・GitHubタグは `d7b543c` と `219e6a9`。終了時資産は7件。保持4件（要求トレース、実装記録、ソース読解台帳と監査索引）、稼働中3件（復旧実装、TLS検査、既存共有Cargoキャッシュ）。保管済み・ゴミ箱移動・保留は各0件。一時fixtureと旧CLIの隔離ビルドは削除済み。Linux上での検査でWindows実機・配布・公開は `NOT_RUN`。

## 2026-11-19 JST B116 — D3-C E6 通常検索の実行予算と一覧のGoal照合

着工点は `f6458147a2ca8c4f44afa244f74756407abfb6b2`。D2仕様・工程とD3-C指示の採用ハッシュ、R2 §9・§13・§16・§19・§20、B115の先行復旧を照合した。唯一Coreの通常検索を元Goalの必須成果へ接続するC04・C13・E03・E04の部分施工。

着工点では表示の3枠で検索のホスト許可判定に到達し、許可なしで `SEARCH_UNAVAILABLE / READ_PERMISSION_REQUIRED` となった。許可済みファイルの保存後に実履歴の `ATTEMPTED` までを取り出すと、復旧は一覧を未定義の証拠として扱い `RECOVERY_EVIDENCE_MISMATCH` で終了2。通常検索が保存する一覧全体は `SearchObservation` であり、従来の元Goalグラフに読取ノードがなかった。

新しい通常検索は元Goalに `search-listing` を封印する。表示4枠、ファイル5枠を満たす場合に、残り予算と元Goalの許可で検索を呼ぶ。Coreは成功した検索一覧の格納参照だけを `compute` に渡す。通信や解析の失敗は取得済みバイト・状態・ハッシュを残して停止する。復旧は、元Goalのプロバイダー・検索語・読取許可・証拠量と、保存一覧から再現した順位・出力単位を照合する。旧グラフも当時の必要枠と実成果で復旧する。検索パケットの形式と入力封印版は維持した。新旧とも復旧は外部の再検索を行わない。

Linux x86_64 6.18.57、Rust/Cargo 1.99.1、製品 `0.7.0-alpha.11`。実装と試験4ファイルの `git diff --binary` SHA-256は `d897c2db7335a510799684704fd019098d5d0f440b52105beb9e80afafb5fbd3`、実CLIのSHA-256は `ca495fb7601f26f4e15fe2ec2796565a08c9c9de73ace3bfa7fc32a4bba5d2d6`。隔離SQLite、専用CAと認証済みループバックTLS、明示ホスト・ファイル許可で検査した。実プロバイダー通信は0件。

- `cargo fmt --all -- --check`、`cargo check --workspace --all-targets --locked`、`cargo build -p minidora-cli --locked` は終了0。Rustテストは `minidora-engine --lib` の検索11件・復旧65件、`minidora-cli --test normal_route` の通常検索1件・本文質問4件・要約1件、計82件PASS。
- `python3 tools/local_tls_observer_test.py --binary <compiled-cli>` は終了0。表示3枠・ファイル4枠・許可なし・締切0の4例は接続0件で停止。表示4枠とファイル5枠はそれぞれ1GETで確定した。HTTP 404とHTTP 200の不正一覧形状は、原バイトと失敗を保存して `SUSPEND`。初回検査で壊れたJSONを一覧形状エラーと仮定したが、実ポートは先にJSON解読で停止した。正しいJSONで一覧型だけが違う入力を使用し、同じ一覧形状の判定を検査した。既存のTLS 8件、資料要約、複数本文の継続・復旧もPASS。
- 旧CLI `7eba9eea2addb270643ade7e7d1e85badcb89a6ecd33f6f8c47f2e2d50d5898f` と現行CLIで別々に出力した実ファイルを、現行CLIの独立プロセスで `ATTEMPTED` から復旧し、それぞれ終了0。旧版の元Goal、封印入力、一覧原本、表示順位1・3と重複で除外した順位2を保持した。新しいグラフは一覧参照を実際のcompute入力として保持した。両版で元の採用計画・描画・作用IDとファイル内容・inode・更新時刻が一致し、再取得・再送0件。別の検索語で取得した正規の一覧に差し替えた隔離複製では、保存ハッシュを再整合しても `RECOVERY_SEARCH_CONTEXT_MISMATCH` で終了2。追加通信・作用履歴・ファイル変更は0件。

原版台帳・凍結β・現物のblobが一致する `src/minidora/製品版/検索.py` の222行を全文確認し、読解台帳を101件に更新した。候補と本文の分離、順位と失敗の保持を照合した。SearXNG・固定供給器の選択、日本語から英語へのWikipedia続行、共有資源台帳、HTTP開始・再試行の個別計数、`Retry-After` ・利用条件、候補ゼロ時の旧保留は差が残る。全受入列は `UNVERIFIED`。E6出口、実プロバイダー、体系QC、独立QAは未検証。次の対象は元の取得制御が共有していた通信開始・本文・期限の実計数と、その正常経路・停止の照合。

復旧2世代は `f645814` と `d7b543c`。終了時資産は計9件。保持4件（要求トレース、実装記録、ソース読解台帳、監査索引）、稼働中5件（Core、検索、復旧、TLS検査、既存共有Cargoキャッシュ）。保管済み・ゴミ箱移動・保留は各0件。Linuxの隔離DBと一時CLI、証明書、試験スクリプトは除去済み。Windows保管・容量回収は非該当。公開・配布は未実施。

## 2026-11-19 JST B117 — D3-C E6 取得経路の通信枠・期限・本文上限

着工点は `47f7d2c2eaf5f2037768123050f32bd0d5f0ad72`。D2仕様・工程とD3-C指示、R2 §9・§13・§16、C02・E04を照合した。唯一Coreの通常検索・候補本文・URL本文取得に元Goalへの共通の資源境界を接続した。

着工点のCLI `ca495fb7601f26f4e15fe2ec2796565a08c9c9de73ace3bfa7fc32a4bba5d2d6` で2件を再現した。二つのURLを作用予算5で取得すると一方のリダイレクトを含む3 GETで確定した。長さ未指定の128バイト応答を本文上限64バイトで取得すると、最初のパケットには本文0バイトが残り、次のURLへの接続も発生した。

元Goalの計算・採用内容・描画・ファイル・保存一覧の必要枠を確保し、残りを当回のHTTP開始に使う。リダイレクト先も権限照合後、DNSや接続準備の前に1枠を確保する。Core開始時の単調時計から定めた同じ締切を、DNS、送信直前、応答ヘッダーと本文に適用する。通信ライブラリの自動再試行を無効にした。上限までの受信本文と先行パケットを停止結果に保持する。長さ未指定時は超過検出に1バイトを読み得る。保持本文は上限内で、余分なデータは後続取得の予算に戻さない。TCP試行回数や回線上の受信量の測定は対象外。保存パケット、元Goal、復旧の形式は既存形式を使う。

Linux x86_64 6.18.57、Rust/Cargo 1.99.1、製品 `0.7.0-alpha.11`。実装・試験4ファイルの `git diff --binary` SHA-256は `aeb4dc78f026697630ae0c9c3ae5b38a213ed3ecbedfb4d13f09ee583a461f15`。検査CLIのSHA-256は `557a43286921ed5f3c98087fc26680558766e4a2a39ba1e3c8c2e73a88a2d553` で、TLS検査の前後で一致した。隔離SQLite、専用CA、ホスト許可を使い、外部プロバイダーへの通信は0件。

- `cargo fmt --all -- --check`、`cargo check --workspace --all-targets --locked`、`cargo build -p minidora-cli --locked` は終了0。Rustテストは `minidora-engine --lib` のポート7件、検索11件、復旧65件と、`minidora-cli --test normal_route` の通常検索1件、前回本文4件、資料要約1件で計89件PASS。公開ポートの締切0は従来の `RESOURCE_LIMIT` を確認した。
- `python3 tools/local_tls_observer_test.py --binary <compiled-cli>` は終了0。本文取得の予算5ではリダイレクトを含む2 GETの後に `HTTP_START_BUDGET_EXHAUSTED / SUSPEND`。予算6では第2資料まで3 GETで確定した。検索のリダイレクトは4枠だと1 GETで停止し、5枠で2 GETが成立した。候補本文は5枠で1 GETの後に停止し、6枠で2 GETが成立した。ホスト許可がない候補取得は、先に確定した一覧を保持して接続0件で停止した。
- 長さ未指定の本文では、先行64バイトと次の先頭32バイトを合計96バイト保持した。応答本文の途中で1,200 msの締切に達したケースは `HTTP_DEADLINE_EXCEEDED` とともに先行本文と次の24バイトを保持した。両例とも第3 URLの接続・採用・ファイル作用は0件。通常検索・候補取得の既存ファイル復旧と資料要約も再取得・再送0件で通過した。

原版台帳、凍結β、現物のblobが一致する `src/minidora/公開本文取得.py` の530行を全文確認し、読解台帳を102件に更新した。HTTP開始の予約、共有締切、受信済み本文の保持を照合した。元の最小通信間隔と取消、429・503の `Retry-After` 共有待機、HTML・PDF・旧文字コードの処理は差が残る。許可済みリダイレクト先が非公開IPに解決された場合は接続を拒否し、先行リダイレクトを同じ失敗パケットに残す経路は未接続。C02・E04の受入列は `UNVERIFIED`。次の照合対象は先行429・503の出所と待機条件を保持する経路。E6出口、外部プロバイダー、体系QC・独立QA、公開・配布は未実施。

復旧2世代は `47f7d2c` と `f645814`。終了時資産は8件。保持3件（要求トレース、実装記録、ソース読解台帳）、稼働中5件（Core、取得ポート、検索、TLS検査、既存共有Cargoキャッシュ）。保管済み・ゴミ箱移動・保留は各0件。一時CLI・隔離DB・TLS証明書・試験補助スクリプトは除去済み。Windows保管と容量回収は非該当。

## 2026-11-19 JST B118 — D3-C E6 Retry-Afterの出所保存と同一Goalの待機

着工点は `30561f5b242a30a94f224b4f780d92403c3f160d`。D2仕様・工程とD3-C指示、R2 §9・§13・§16、C02・E04を照合した。着工点の実CLI `557a43286921ed5f3c98087fc26680558766e4a2a39ba1e3c8c2e73a88a2d553` では、429と503の応答本体・状態は保持されたが、`Retry-After` は失われ、同じ許可ホストへの次のGETも発生した。

429またはヘッダー付き503の実応答を受けた時点で、秒数・IMF-fixdate・欠落・不正・重複・過大を型で保存する。凍結版のJSON取得器に合わせ、0秒と過去の日時は最低1秒、欠落した429と不正値・重複値は60秒、12桁を超える秒数は有限時刻なしとした。旧式HTTP日付は不正値と同じ保留を適用する。不正なヘッダーの任意文字列や他の応答ヘッダーは記録対象外。本文の上限や締切で応答の取込が止まっても、受信した待機情報を失敗パケットへ結び付ける。新しいパケットIDは旧式失敗IDと待機観測を含み、検索一覧は元取得IDを通して同じ観測を保持する。既存パケットは待機欄を省略して従来の正規バイト列とIDを保つ。

同一Goalの許可済みホストごとに単調時計で接続を制御する。期限前の対象ホストは次の通信枠の消費とDNS・接続の前に停止する。待機は元Goalの締切内で働き、自動再試行、待ち時間分の延長、待機中のスリープ、未送信URLの擬似パケット作成は0件。ヘッダーなし503は従来どおり通常の失敗として保持する。先に指定された別ホストの取得はそのホストの許可・予算に従う。

Linux x86_64 6.18.57、Rust/Cargo 1.99.1、製品 `0.7.0-alpha.11`、依存lock SHA-256 `b4e97fecdca8fd1fc863349b1a48eb99a925da7fed0ed6ce6cfe79bdd1cececc`。実装・試験7ファイルの `git diff --binary` SHA-256は `130c4cd138e654310551431e4071a6087c645ef6519128cad151cbd7a15b62b3`。実CLIのSHA-256 `5cac6476dc7c8d7d1c0f76463a8f7012e92842ce9a5b0cbb6ee4e8e9919e8e07` がTLS検査の前後で一致した。

- `cargo fmt --all -- --check`、`cargo check --workspace --all-targets --locked`、`cargo build -p minidora-cli --locked` は終了0。Rustテストはドメイン7件、ポート9件、検索11件、要約13件、復旧65件、通常CLI6件で計111件PASS。
- `python3 tools/local_tls_observer_test.py --binary <compiled-cli>` は終了0。通常取得19例と、別枠の検索・候補本文・ファイル復旧・資料要約がPASS。429と503の秒数、IMF-fixdate、待機時刻と元パケットを照合し、同一ホストの後続接続は0件。本文上限に達した429、重複、欠落、過大秒数、ヘッダーなし503も検査した。検索失敗の `SearchObservation` と候補本文の失敗artifactで待機情報が一致した。
- 独立の旧版・新版CLI検査11件PASS。正規JSONの失敗ID、待機付きID、検索一覧IDを再計算して照合した。未来と過去のIMF-fixdateを検査し、過去の日付は受信から1秒。混在ホストの実TLSで、429後に許可済み別ホストへ接続でき、次の元ホストには接続しなかった。旧版で保存した429を新版の `inspect` と同じ入力IDの `run` で取得し、旧版のCaseRevision、ResultRecord、パケットの正規バイト列・ID・履歴を保持した。追加GETは0件。両検査は隔離SQLiteと専用CAのループバックのみで、実プロバイダー通信は0件。

原版台帳・凍結β・現物blobが一致する `src/minidora/製品版/Google検索.py` 181行を全文確認し、読解台帳を103件に更新した。凍結版では同一供給器の待機が別要求へ継承され、認証と送信予約より先に判定される。今回のRustの接続制御は同一Goal内で、別Goalをまたぐ供給器の待機、旧式HTTP日付の解釈、認証・課金予約、外部プロバイダーの実動作は未検証。次の照合は別Goalに及ぶ待機の範囲と、既存のactor・ケース分離との整合。C02・E04受入は `UNVERIFIED`。E6出口、体系QC、独立QA、公開・配布は未実施。

復旧2世代は `30561f5` と `47f7d2c`。終了時資産は11件。保持3件（要求トレース、実装記録、読解台帳）、稼働中8件（実装・試験7ファイルと既存共有Cargoキャッシュ）。保管済み・ゴミ箱移動・保留は各0件。一時CLI・隔離DB・TLS証明書・試験補助資料は除去済み。Windows保管と容量回収は非該当。

## 2026-11-19 JST B119 — D3-C E6 次のGoalへの有限なRetry-After引き継ぎ

着工点は `f003975c9650455fbb401379304105316d17bdd4`。D2仕様・工程とD3-C指示、R2の履歴・権限制約、C02・E04、凍結版の供給器の待機を照合した。着工点のCLI `5cac6476dc7c8d7d1c0f76463a8f7012e92842ce9a5b0cbb6ee4e8e9919e8e07` では、429と120秒の待機を保存した後、同じactor・ケースの別入力IDから同じ許可ホストへ接続し、確定した。

唯一Coreは新しい取得前に、認証済みケース履歴の停止結果に残った429・503を確認する。最新結果や会話文脈が変わっても同じケースの有効な観測を照合する。実取得元のホストが新しいGoalで明示許可され、保存済みの有限待機時刻が未経過の場合が対象。本文の正規パケットはケース内artifact、検索一覧は元の `SearchObservation` と取得IDを照合する。履歴照合にも元Goalの締切を適用する。保存した絶対時刻から今回の単調時計の待機を組み立て、時計が受信時刻より前を示しても最初の待機長を超えて復元しない。

同じホストはDNS・接続・通信枠を使う前に `HTTP_RETRY_DEFERRED` で停止し、理由には保存パケットIDを示す。待機時刻、元の観測・結果・履歴を保持し、未送信先の擬似パケットや受領は作らない。期限経過後は新しい入力ID・許可・元Goalの資源枠で通常取得する。B118で有限時刻を算出できなかった待機は当時のGoal内で扱う。

Linux x86_64 6.18.57、Rust/Cargo 1.99.1、製品 `0.7.0-alpha.11`。実装・試験3ファイルの `git diff --binary` SHA-256は `f945df95c1b09fe2dc256cac223a4130d9911a5c292965e905e542826bcece18`。実CLIのSHA-256は `5f17682086009354ab1d43ffaac866501439f506591b534d760ffe2e1debecb6` でTLS検査の前後で一致した。

- `cargo fmt --all -- --check`、`cargo check --workspace --all-targets --locked`、`cargo build -p minidora-cli --locked` は終了0。Rustテストは取得ポート9件、検索11件、復旧65件、Store31件、通常CLI6件で計122件PASS。Storeの別プロセス書込検査を含む。
- `python3 tools/local_tls_observer_test.py --binary <compiled-cli>` は終了0。既存の通常取得19例、検索・候補・復旧・資料要約と、別プロセスの有限待機を検査した。無関係な演算を挟んだ取得、検索一覧を出所とする取得、複数の新規入力はいずれも元パケットIDを示して停止し追加接続0件。現在の許可がない場合と別actorから同じケースを指定した場合は権限で停止し、保存パケットIDを返さない。別ケース・別actor自身のケース、期限経過、有限時刻なしの新しいGoalでは許可済み接続が成立した。
- 新旧CLIで別々に作った429履歴を新CLIの新しいGoalから照合する独立検査2件がPASS。各ケースは元のGET1件のみで、次のGoalは終了2。パケットIDを再計算して照合し、旧・新とも元のパケット、ResultRecord、CaseRevisionの正規バイト列を保持した。同一入力IDの再実行で同じ結果を得て追加GET0件。すべて専用CA・隔離SQLite・ループバックで実施し、実プロバイダーへの通信は0件。

凍結βと原版台帳のblobが一致する `src/minidora/知識取得接続.py` 84行を全文確認し、読解台帳を104件に更新した。今回の待機引き継ぎは同じactor・ケース・許可ホストに適用する。旧版の供給器インスタンス全体の待機、型付き供給器登録、対象語条件と継続カーソル、旧権限の再評価を含む別体系の履歴検証は未検証。C02・E04受入は `UNVERIFIED`。次は取得先の変化で停止した場合の先行観測と元Goalの根拠を照合する。E6出口、体系QC、独立QA、公開・配布は未実施。

復旧2世代は `f003975` と `30561f5`。終了時資産は7件。保持3件（要求トレース、実装記録、読解台帳）、稼働中4件（Core、取得ポート、TLS検査、既存共有Cargoキャッシュ）。保管済み・ゴミ箱移動・保留は各0件。一時CLI・隔離DB・証明書・試験補助資料は除去済み。Windows保管と容量回収は非該当。

## 2026-11-19 JST B120 — D3-C E6 リダイレクト先の直接DNS拒否と先行観測の保持

着工点は `6f5a2d5e8489b1c3764dbb86ee66c2e5ff900361`。D2仕様・工程とD3-C指示、R2の由来・許可・停止条件、C02・E11を照合した。着工点の実CLI `5f17682086009354ab1d43ffaac866501439f506591b534d760ffe2e1debecb6` では、初回URLに対する実302を受信した後、許可済みの転送先がDNSで非公開アドレスに解決されると `PRIVATE_READ_TARGET` の停止だけが保存され、302・応答URL・経路の観測が失われた。

宛先の直接DNS検査で停止した場合、HTTP応答を実受信した経路があれば、元の要求URL、最後に応答したURL・状態、応答済み経路、`PRIVATE_READ_TARGET` を正規の失敗パケットにする。未接続の転送先を応答済み経路に含めない。3xxの本文や `Location` の任意文字列は取得・保存の対象外。初回宛先で拒否した場合は先行応答が存在せず、従来のエラーを返す。通常の複数URL取得は保全したパケットを保存してから元Goalを停止し、後続URLの取得、採用、描画、ファイル作用へ進まない。保存スキーマと正規ID式で互換を保つ。

Linux x86_64 6.18.57、Rust/Cargo 1.99.1、製品 `0.7.0-alpha.11`。実装・試験2ファイルの `git diff --binary` SHA-256は `725eca51e328b1853f9a32009aae60ac2227511dff6a19a9409931ecd4996d71`。実CLIのSHA-256は `f33acab7eac4d0b12fca62a1f69e0e20aeab99216d31aebc54b76c0904fae7bc` でTLS検査の前後で一致した。

- `cargo fmt --all -- --check`、`cargo check --workspace --all-targets --locked`、`cargo build -p minidora-cli --locked` は終了0。Rustテストは取得ポート10件、検索11件、復旧65件、通常CLI6件で計92件PASS。新しいポート回帰で初回拒否と、先行応答がある場合の307・失敗ID・経路・本文なしを照合した。
- 既存 `python3 tools/local_tls_observer_test.py --binary <compiled-cli>` は終了0。通常取得19例に加え、検索・候補・有限待機の後続Goal・資料要約・ファイル復旧がPASS。
- 独立の実CLI検査は、子プロセスに限り試験専用ホスト1件のDNS応答をループバックへ固定し、接続先は検証済みローカルTLSの試験ポートだけに制限した。実302を受けた取得は終了2の `SUSPEND` となり、起点URL・状態302・応答済み経路・失敗パケットID・保存artifactの正規バイト列が一致した。先行GETは1件で、拒否先・後続URLへの接続は0件。初回拒否ではGET・パケットとも0件。両例で採用・描画・ファイル・受領0件。同一入力IDの再実行は保存結果の正規バイト列とIDを保ち、DNS・接続の追加0件。外部プロバイダー・外部DNSへの通信は0件。

凍結βと原版台帳でblobが一致する `src/minidora/ブラウザ通信.py` 114行を全文確認し、読解台帳を105件に更新した。旧ブラウザー資源取得は検査済みURLを使い、転送を拒否した実状態を保持する。ブラウザー資源の同一生成元・明示URL・資源種別の契約は今回の一般取得とは別経路で、実consumerは未照合。C02・E11受入は `UNVERIFIED`。次は代替取得の経路・予算・停止条件を照合する。E6出口、体系QC、独立QA、公開・配布は未実施。

復旧2世代は `6f5a2d5` と `f003975`。終了時資産は6件。保持3件（要求トレース、実装記録、読解台帳）、稼働中3件（Core、取得ポート、既存共有Cargoキャッシュ）。保管済み・ゴミ箱移動・保留は各0件。一時CLI、子プロセス用DNS試験コード、隔離DB、証明書は除去済み。Windows保管と容量回収は非該当。

## 2026-11-19 JST B121 — D3-C E6 明示した順序での代替メタデータ検索

着工点は `07db5778ff0d67b55616c88cbdab45f47e592950`。D2仕様・工程とD3-C指示の採用記録、R2 §9・§13、C02・E04を照合した。着工点の実CLI `f33acab7eac4d0b12fca62a1f69e0e20aeab99216d31aebc54b76c0904fae7bc` は `検索 wikipedia -> crossref: trial` を未対応として終了2の `SUSPEND` とし、接続許可・取得証拠はなかった。

通常入口で `検索 wikipedia -> crossref: 検索語` のように2つの異なる供給器と同一検索語を指定できる。Coreは実受信した429または503の失敗を照合して保存し、ケース版の確認後に、代替先の許可と元Goalの残り予算内で次の取得を認める。正常な空一覧を含む初回成功で選択を終える。両方を試行した場合は失敗と採用一覧を試行順に保持し、格納参照の順序を `compute` の `search-listing` 証拠に封印する。復旧は元Goalの順序・検索語・認可済み転送経路・HTTP開始数・原バイトと出力を再照合する。順位を指定した本文取得は採用一覧を使用し、初回の有限 `Retry-After` は代替成功や初回失敗の保存直後の取消を経ても、同じactor・ケースの次のGoalで照合する。単一供給器の入力と既存証拠の形式を継承した。新規の `SEARCH_WITH_FALLBACK` は v93 の入力解釈を扱う版で読む。

Linux x86_64 6.18.57、Rust/Cargo 1.99.1、製品 `0.7.0-alpha.11`。実装と試験7ファイルの `git diff --binary` SHA-256は `204d1e44fea9b25792f334683a76c71ab8150832faf7a25769919280104a58bb`。実CLIのSHA-256は `233ef70e1220a46669a14a4e15a0ffd106b4a558f40205324ad44ec2261a6170` でTLS検査の前後で一致した。

- `cargo fmt --all -- --check`、`cargo check --workspace --all-targets --locked`、`cargo build -p minidora-cli --locked` は終了0。Rustテストはドメイン7件、入力39件、取得ポート10件、検索11件、復旧65件、通常CLI6件で計138件PASS。
- `python3 tools/local_tls_observer_test.py --binary <compiled-cli>` は終了0。従来の通常取得19例と検索・本文・要約・復旧に加え、代替指定11例および429からの代替採用、保存済みファイル復旧、採用一覧からの本文取得を検査した。許可欠落・予算不足・不正一覧・転送拒否を実経路で確認した。確定後と初回429保存直後の取消後は元の失敗証拠を示して待機し、追加接続0件。専用CAのループバックTLS、隔離SQLite、外部転送0件で検査した。実プロバイダーLIVE、全workspace実行試験、E7 QC・E8 QAは `NOT_RUN`。

凍結βと原版台帳でblobが一致する `src/minidora/参照.py` 345行と `src/minidora/標準参照.py` 82行を全文確認し、読解台帳を107件に更新した。旧版の構成による供給器選択、複数結果の集約、並列取得、供給器内の再試行は今回の実経路で未照合。C02・E04の受入は `UNVERIFIED`、E6は継続中。配布とリリースは未実施。

復旧2世代は `07db577` と `6f5a2d5`。終了時資産は11件。保持3件（要求トレース、実装記録、読解台帳）、稼働中8件（施工5ファイル、ドメイン型、TLS検査、既存共有Cargoキャッシュ）。保管済み・ゴミ箱移動・保留は各0件。隔離DB・証明書・出力試験ファイルは除去済み。Windows保管と容量回収は非該当。

## 2026-11-19 JST B122 — D3-C E6 複数URL取得の中断時に受信済み証拠を保持

着工点は `f0b507ad851b22e1c0aeebf36f0067721e84cf5a`。D2仕様・工程とD3-C指示の採用SHA、R2 §9・§13・§16、C02・E05・E12を照合した。着工点の実CLI `233ef70e1220a46669a14a4e15a0ffd106b4a558f40205324ad44ec2261a6170` は、異なる許可ホストへの複数URL取得で初回429を受信し、2件目のGET中に強制終了すると `RUNNING` に結果参照がなかった。同じ入力IDの再実行は証拠なしの終了2、取消は終了5、同じケースで元ホストを再取得する新しいGoalは待機中にGETして終了0だった。専用CA付きループバックTLSで `/429`、`/slow-body`、`/ok` の3GETを観測した。

Coreは受信済みパケットと依存IDの列を元ケースの `RUNNING` リビジョンに保存してから次のURLへ進み、その前にケースの現行版を照合する。同じ入力IDの再実行は保存済み部分を `INCOMPLETE_PRIOR_ATTEMPT` として返す。取り消された取得の待機は、元の封印入力・URL順・許可経路・パケットのIDと正規バイト列を照合し、同じactor・ケースで新たに許可された応答ホストに適用する。受信済み証拠があっても元Goalの完了は要求済みの本文と出力の成立で決める。1件の取得の内部で転送先へ進む際の並行取消は別の残件。

Linux x86_64 6.18.57、Rust/Cargo 1.99.1、製品 `0.7.0-alpha.11`。実装と試験2ファイルの `git diff --binary` SHA-256は `08182e436a9a81b602096f78262ba59c34d85b3cfc868d71681714d3fb6f1258`。実CLIのSHA-256は `f1aa4479080ecd10fd4e0406734786348e1ec91121eb23a77d78ef62b16279fe` でTLS検査の前後で一致した。

- `cargo fmt --all -- --check`、`cargo check --workspace --all-targets --locked`、`cargo build -p minidora-cli --locked` は終了0。関連Rustテストは取得ポート10件、検索11件、復旧65件、Store31件、通常CLI7件の計124件PASS。
- `python3 tools/local_tls_observer_test.py --binary <compiled-cli>` は終了0。従来の通常取得19例、検索・本文・要約・復旧と、新しい強制終了・別プロセス取消の両経路を検査した。実429の原バイトと待機期限を保存済みケースから読み出し、同じ入力の再実行と取消後の新しいGoalで追加接続0件。別ケースは同一ホストの読取に成功した。最初の取得中に取消を確定した例では次のURLへの接続0件。実プロバイダーへの接続と外部転送は0件。全workspace実行試験、E7 QC・E8 QAは `NOT_RUN`。

凍結β・原版台帳・現物のblobが一致する `src/minidora/知識取得.py` 445行を全文確認し、読解台帳を108件に更新した。旧版の候補取得前後の取消、採用済み資料と予算付きの継続カーソルを照合した。順位付き候補の継続と予算台帳の同等性は未検証。C02・E05・E12の受入は `UNVERIFIED`、E6は継続中。リリースと配布は未実施。

復旧2世代は `f0b507a` と `07db577`。終了時資産は6件。保持3件（要求トレース、実装記録、読解台帳）、稼働中3件（Core、TLS検査、既存共有Cargoキャッシュ）。保管済み・ゴミ箱移動・保留は各0件。隔離DB・証明書・一時出力は試験終了時に消滅。Windows保管、容量回収、指示者処分待ちは非該当。

## 2026-11-19 JST B123 — D3-C E6 HTTPSリダイレクト先への接続前に取消を確認

着工点は `6a0df536ee620482ac68d8e812a0398051a487bd`。D2仕様・工程とD3-C指示の採用SHA、R2 §9・§13・§16、C02・E05・E12を照合した。着工点の実CLI `f1aa4479080ecd10fd4e0406734786348e1ec91121eb23a77d78ef62b16279fe` で、認証済みループバックTLSの初回応答を保留し、別プロセスの「中止」が終了5で確定してからHTTP 302を返した。元のプロセスは転送先をGETした後に終了1。両ホストに計2回のCONNECTを観測した。

通常の直接URL取得、検索と明示代替、順位指定の候補本文取得で、Coreが実行中のケース版を共通取得ポートへ渡す。ポートは各ホップのDNS・接続準備の前と、送信の直前にCoreの照合を呼ぶ。照合はactorとケースの所有権、および既に検証されたケース版の正規バイト列のSHA-256を現在の参照と比較する。取消で版が変わった場合は、元入力に保存された `CANCEL` と、その時点でケースに保存済みの観測を返す。転送先の失敗パケットや追加の採用・描画は生成しない。既に開始を認めた通信は元の締切の範囲で処理する。

Linux x86_64 6.18.57、Rust/Cargo 1.99.1、製品 `0.7.0-alpha.11`。実装と試験5ファイルの `git diff --binary` SHA-256は `ef759da4ed35b645e65fd957a03af49fd582ddbd1c17f396b3da20e92991da53`。実CLIのSHA-256は `13a2512afbbcd8e01cb996a2a17b1d51323e48aa1f3203fcde150b1c65c66433` でTLS検査の前後で一致した。初回の施工版はID用のハッシュを保存バイト列のSHA-256と照合して正常通信を拒否した。保存側と同じ正規バイト列のSHA-256へ修正し、同じ正常例を含む検査を再実行した。

- `cargo fmt --all -- --check`、`cargo check --workspace --all-targets --locked`、`cargo build -p minidora-cli --locked` は終了0。関連Rustテストは取得ポート10件、検索11件、復旧65件、Store31件、通常CLI7件の計124件PASS。
- `python3 tools/local_tls_observer_test.py --binary <compiled-cli>` は終了0。従来の通常取得19例と検索・本文・復旧に加え、直接取得・検索・候補本文の3経路で、別プロセスの取消が確定してから実302を返した。元の入力は保存済みの `CANCEL` を返し、同一入力IDを再実行しても同じ結果で、追加CONNECTとGETは全経路0件。直接取得では前のURLの証拠を保持し、要求済みの出力ファイルは生成されなかった。別actorによる取消が終了3で拒否された例では、両ホストへのGETと通常の確定を観測した。外部転送と実プロバイダー接続は0件。全workspace実行試験、E7 QC・E8 QAは `NOT_RUN`。

凍結β・原版台帳・現物のblobが一致する `src/minidora/HTTP参照.py` 321行を全文確認し、読解台帳を109件に更新した。旧版の取得ごとの時間制限、本文の成功キャッシュ、本文失敗時の候補保持を照合した。キャッシュ・HTML抽出・旧版の取得方針全体の実経路同等性は未検証。C02・E05・E12の受入は `UNVERIFIED`、E6は継続中。リリースと配布は未実施。

復旧2世代は `6a0df53` と `f0b507a`。終了時資産は9件。保持3件（要求トレース、実装記録、読解台帳）、稼働中6件（Core、取得ポート、検索、Store、TLS検査、既存共有Cargoキャッシュ）。保管済み・ゴミ箱移動・保留は各0件。隔離DB・証明書・一時ファイルは試験終了時に消滅。Windows保管、容量回収、指示者処分待ちは非該当。

## 2026-11-19 JST B124 — D3-C E6 リダイレクトの Retry-After を取得停止に反映

着工点は `34b8de4ca6fcd492770e3f3871e8017cb05bca05`。D2仕様・工程とD3-C指示の採用SHA、R2 §9・§13、C02・E04とRFC 9110 §10.2.3の転送待機の範囲を照合した。着工点の実CLI `13a2512afbbcd8e01cb996a2a17b1d51323e48aa1f3203fcde150b1c65c66433` は、ローカルTLSで `Retry-After: 120` 付き307の後すぐ転送先をGETして終了0。証拠には最終200だけがあり、待機条件は保存されなかった。

対象は通常のHTTPS取得で追跡する301・302・303・307・308。転送先の権限・経路・回数を照合し、正の待機時間が残る場合は実3xx、応答元と応答済み経路、待機指示と算出可能な期限を失敗パケットに保存する。未接続の転送先は応答済み経路に含めない。指定なし、ゼロ秒、期限を過ぎたIMF-fixdateでは従来の許可済み転送を行う。重複、不正、旧HTTP日付形式、数値上限超過の指定は期限を生成せず停止する。Coreは `HTTP_REDIRECT_RETRY_DEFERRED` で元Goalを `SUSPEND` とし、そのGoalの代替プロバイダーや後続URLへ進まない。自動待機・再送・期限延長は行わない。別の許可済み取得に対する429・503のホスト単位の待機には、この3xxを登録しない。

Linux x86_64 6.18.57、Rust/Cargo 1.99.1、製品 `0.7.0-alpha.11`。実装と試験3ファイルの `git diff --binary` SHA-256は `ab549c1877fbf103782278b27f6c9a93df7f782758e2661dc64c2fdc53a2da40`。実CLIのSHA-256は `0ebe6231867f5dc454c7c7bf285966d60f2b75655e8cd02c4dd5f470aa88c7e6` でTLS検査の前後で一致した。

- `cargo fmt --all -- --check`、`cargo check --workspace --all-targets --locked`、`cargo build -p minidora-cli --locked` は終了0。関連Rustテストは取得ポート10件、検索11件、復旧65件、通常CLI6件の計92件PASS。
- `python3 tools/local_tls_observer_test.py --binary <compiled-cli>` は終了0。従来の通常取得19例と検索・本文・取消・復旧、および待機指定の直接取得7例を検査した。転送先未許可の例は従来の権限拒否で停止し、検索と候補本文の待機でも転送先への追加接続0件。元のケースで別入力として独立に要求した応答元・転送先への2取得は成功した。同一入力IDでは保存済み停止結果が一致し追加通信0件。外部転送と実プロバイダー接続は0件。全workspace実行試験、E7 QC・E8 QAは `NOT_RUN`。

凍結版 `src/minidora/公開本文取得.py` の転送とJSON用429・503の待機分岐を再照合した。元版は3xxから転送先を選び、JSON用ホスト待機は終端応答の別処理にある。既存の全読解記録は109件。C02・E04の受入は `UNVERIFIED`、E6は継続中。リリースと配布は未実施。

復旧2世代は `34b8de4` と `6a0df53`。終了時資産は6件。保持2件（要求トレース、実装記録）、稼働中4件（Core、取得ポート、TLS検査、既存共有Cargoキャッシュ）。保管済み・ゴミ箱移動・保留は各0件。隔離DB・証明書・一時ファイルは試験終了時に消滅。Windows保管、容量回収、指示者処分待ちは非該当。

## 2026-11-19 JST B125 — D3-C E6 同じ出典URLの最新版を要約に使用

着工点は `5cb242e3369a82142fd38c9b5329592989706ea6`。D2仕様・工程とD3-C指示の採用SHA、R2 §12・§13、C10・C11を照合した。着工点の実CLI `0ebe6231867f5dc454c7c7bf285966d60f2b75655e8cd02c4dd5f470aa88c7e6` で検索後に同じ順位の本文を2回取得し、同じ最終URLに旧版の「測定値は10です。」と新版の「測定値は30です。」を保存した。通常要約は両方を引用して終了0。文字数指定付き要約は1つの出典を複数の資料として扱い、`AMBIGUOUS_SOURCE_CONTEXT` で終了2だった。

封印版 `declarative-ja-en-v94-versioned-source-summary` の本文要約では、Coreが確定済みの前回結果にある全取得本文の格納参照を照合し、本文質問と共通の規則で最終URLごとに最新の観測版を選ぶ。版は原バイト・抽出本文のハッシュと抽出版で区別する。最新時刻で異なる版が競合すると停止する。文字数や件数の制約は選択後に評価し、引用は採用したパケットの原文範囲に結び付ける。旧版のパケットも格納参照を保持し、実行グラフの `compute` が版選択に使った全パケットを示す。ファイル復旧は封印版に従い同じ選択、引用と元の書込IDを照合する。旧封印版の復旧は元の引用を再現する。判定対象は封印された前回結果内の観測であり、外部サイトの更新監視や過去の確定結果への変更は別範囲。

Linux x86_64 6.18.57、Rust/Cargo 1.99.1、製品 `0.7.0-alpha.11`。実装と試験5ファイルの `git diff --binary` SHA-256は `2be42eb41e762b8b1357f4042c219bdf81c66f31c537e1213f89a4b6e77c7051`。実CLIのSHA-256は `d5f2feaa51ad96a53a145a23db9bbf3776e306de1874192c262b3266738f9bc4` でTLS検査の前後で一致した。

- `cargo fmt --all -- --check`、`cargo check --workspace --all-targets --locked`、`cargo build -p minidora-cli --locked` は終了0。関連Rustテストは入力39件、本文・要約13件、検索11件、復旧65件、通常CLI8件の計136件PASS。同時刻の異版は新版で拒否し、旧版では元の二本文を選ぶ差を照合した。
- `python3 tools/local_tls_observer_test.py --binary <compiled-cli>` は終了0。従来の通常取得19例と検索・本文・取消・復旧に加え、実取得した同一URL二版から通常要約と制約付き要約を作成した。両経路とも新版のみを引用し、旧版の格納参照は版選択グラフに残った。制約付き要約は実ファイルの試行記録から復旧し、引用と書込ID、ファイル実バイト、inode、更新時刻が一致した。旧CLIで作った二版引用の実ファイルも新CLIで独立復旧し、旧引用・依存・書込IDと実ファイルが一致した。要約と復旧の追加接続は0件。外部転送・実プロバイダー通信は0件。全workspace実行試験、E7 QC・E8 QAは `NOT_RUN`。

凍結β・原版台帳・現物のblobが一致する `src/minidora/資料読解.py` 204行を全文確認し、読解台帳を110件に更新した。旧版の原文範囲・本文ハッシュ・未解釈箇所と元要求からの再計算を照合した。意味構造の抽出と同等性は未検証。C10・C11の受入は `UNVERIFIED`、E6は継続中。リリースと配布は未実施。

復旧2世代は `5cb242e` と `34b8de4`。終了時資産は9件。保持3件（要求トレース、実装記録、読解台帳）、稼働中6件（Core、入力封印、要約、復旧、TLS検査、既存共有Cargoキャッシュ）。保管済み・ゴミ箱移動・保留は各0件。比較用CLI複製、隔離DB、証明書、試験出力は除去済み。Windows保管、容量回収、指示者処分待ちは非該当。

## 2026-11-19 JST B126 — D3-C E6 候補本文の取得失敗後に別順位を指定

着工点は `51172a94a38d5158bede4c572a60711d5955fab8`。D2仕様・工程とD3-C指示の採用SHA、R2 §9・§13・§16、C04・C12・C13を照合した。着工点の実CLI `d5f2feaa51ad96a53a145a23db9bbf3776e306de1874192c262b3266738f9bc4` は、候補1の本文取得後に候補2の404を保存したが、候補3の明示指定は `MISSING_SEARCH_RESULT_CONTEXT` で終了2となった。ケースには元の検索一覧と候補1の本文が残っていた。専用CAのループバックTLSで検索・候補1・候補2の3件だけを観測した。

新しい別順位の通常入力に明示文脈がなく、直前が候補本文の取得失敗であれば、Coreは失敗の封印入力・文脈ハッシュ・順位・失敗パケットの正規バイト列と保存順を照合する。その失敗が参照した同じケースの確定結果を元の一覧として新しい入力に封印する。新しい取得には現入力の許可・予算を適用し、429・503の保存済み待機は引き続き有効。成功した本文を持ち越し、失敗は元の履歴に保持する。同じ順位、取得前の権限拒否、旧形式で失敗に一覧が保存されていない場合は、この文脈の再利用の対象外。元の検索や失敗した取得は自動再送しない。既存の候補取得の証拠形式を用い、ファイル復旧は封印時に確定した文脈ハッシュも照合する。

Linux x86_64 6.18.57、Rust/Cargo 1.99.1、製品 `0.7.0-alpha.11`。実装と試験4ファイルの `git diff --binary` SHA-256は `9a6fe58d490b535fc24712f414f2cbc644980d68b63752ce3cbd4fa960097bac`。実CLIのSHA-256は `cf67b2e4aa2f36b58ea5fe59ba8116d005667ea70daf9532dc02c37c4130f6c4` でTLS検査の前後で一致した。保存欠落の初回検査では物理ファイル欠落をStoreが起動時に検出した。継続処理も直接検証するため、専用試験DBのケースと失敗パケットの関連付けを一時的に外して検査した。

- `cargo fmt --all -- --check`、`cargo check --workspace --all-targets --locked`、`cargo build -p minidora-cli --locked` は終了0。関連Rustテストは入力39件、取得ポート10件、検索11件、復旧65件、Store31件、通常CLI6件の計162件PASS。
- `python3 tools/local_tls_observer_test.py --binary <compiled-cli>` は終了0。従来の通常取得19例ほかに加え、候補1の本文、候補2と3の実404、明示した候補4の本文を検査した。検索1件と本文4件の実GETを観測し、同一入力IDの照会とファイル復旧の追加通信0件。ファイル本体、inode、更新時刻、書込IDが一致した。同順位、権限なし、有効な429待機で追加接続0件。失敗パケットのケース関連付け欠落は封印前に `FAIL` を保存し、権限範囲を復元した試験ケースでは次の独立した演算を受け付けた。外部転送・実プロバイダー通信は0件。全workspace実行試験、E7 QC・E8 QAは `NOT_RUN`。

凍結β・原版台帳・現物のblobが一致する `src/minidora/能力結果復元.py` 30行を全文確認し、読解台帳を111件に更新した。旧版の型付き参照と成功・保留の復元、元版継続処理の確定済み出所の照合を確認した。旧版の汎用能力結果復元と継続カーソルの同等性は未検証。C04・C12・C13の受入は `UNVERIFIED`、E6は継続中。リリースと配布は未実施。

復旧2世代は `51172a9` と `5cb242e`。終了時資産は8件。保持3件（要求トレース、実装記録、読解台帳）、稼働中5件（Core、検索、復旧、TLS検査、既存共有Cargoキャッシュ）。保管済み・ゴミ箱移動・保留は各0件。隔離DB、証明書、試験ファイルは除去済み。Windows保管、容量回収、指示者処分待ちは非該当。

## 2026-11-19 JST B127 — D3-C E6 候補取得失敗後の出典質問と本文要約

着工点は `85a803013702bf904a1c9e15a89a3f34031e5a4f`。D2仕様・工程とD3-C指示の採用SHA、R2 §12・§13・§14、C11・C13を照合した。着工点の実CLI `cf67b2e4aa2f36b58ea5fe59ba8116d005667ea70daf9532dc02c37c4130f6c4` は、候補1の本文を取得し候補2の404を保存した直後、同じケースの出典質問・通常要約・制約付き要約をすべて `MISSING_ACTUAL_RESULT` で停止した。候補1の取得本文は三ケースとも保存されていた。

出典質問と両方の本文要約で参照先が省略された場合、Coreは直前の候補取得失敗からB126の順位・封印文脈・一覧・証拠順の検証を使い、その失敗が参照した確定済み結果を照合する。取得済み本文があれば、その確定結果の識別子とハッシュで新しい入力を封印する。質問・要約は保存済みの成功本文から出典付きの引用を計算し、失敗の記録は元のケース履歴に保持する。実際の取得本文がない場合や、その本文に該当箇所がない場合は停止する。再取得は行わない。既存の入力解釈と出力・証拠形式を用い、ファイル復旧では文脈ハッシュと引用を再照合する。一般の「前の回答を要約して」、明示文脈、決定済みの順位指定経路は従来の契約に従う。

Linux x86_64 6.18.57、Rust/Cargo 1.99.1、製品 `0.7.0-alpha.11`。実装と試験4ファイルの `git diff --binary` SHA-256は `ebadecb48a38dcb1ace9e951036bf01d805432f573db982bf13b96fee3c055e0`。実CLIのSHA-256は `547f21823c2ef98513893fd552094fdeee55641f5df1df1416b772b896cf65f5` でTLS検査の前後で一致した。

- `cargo fmt --all -- --check`、`cargo check --workspace --all-targets --locked`、`cargo build -p minidora-cli --locked` は終了0。関連Rustテストは入力39件、本文・要約13件、検索11件、復旧65件、通常CLI8件の計136件PASS。同じ識別子で文脈だけ変えた復旧は照合エラーとなった。
- `python3 tools/local_tls_observer_test.py --binary <compiled-cli>` は終了0。従来の通常取得19例ほかに加え、先に取得した「実測値は42です。」を出典質問・通常要約・制約付き要約で引用した。404応答本文の「実測値は999です。」と失敗にしか含まれない語は採用されなかった。成功本文がない例と、質問語が失敗応答にしかない例は停止した。読取許可を付けない後続の質問・要約でも追加接続0件。実ファイル2件を別プロセスで復旧し、引用、依存、書込ID、バイト列、inode、更新時刻を保持した。外部転送と実プロバイダー通信は0件。全workspace実行試験、E7 QC・E8 QAは `NOT_RUN`。

凍結β・原版台帳・現物のblobが一致する `src/minidora/文脈命題回答.py` 41行を全文確認し、読解台帳を112件に更新した。旧版の元取得の再構成・取得範囲・出典時刻と最終URL・命題の条件付き表現を照合した。命題判定と複数解釈の同等性は未検証。C11・C13の受入は `UNVERIFIED`、E6は継続中。リリースと配布は未実施。

復旧2世代は `85a8030` と `51172a9`。終了時資産は8件。保持3件（要求トレース、実装記録、読解台帳）、稼働中5件（Core、検索、復旧、TLS検査、既存共有Cargoキャッシュ）。保管済み・ゴミ箱移動・保留は各0件。隔離DB、証明書、試験ファイルは除去済み。Windows保管、容量回収、指示者処分待ちは非該当。

## 2026-11-19 JST B128 — D3-C E6 出典回答と本文要約からの同一資料の続行

着工点は `dff0f1839b2b4c5d0cb9cb485fe0f1f3e649d316`。D2仕様・工程とD3-C指示の採用SHA、R2 §13・§14、C11・C13を照合した。着工点の実CLI `547f21823c2ef98513893fd552094fdeee55641f5df1df1416b772b896cf65f5` で、出典質問の確定直後の追加質問と通常本文要約がいずれも `SOURCE_BODY_UNAVAILABLE` で終了2となった。元のケースには二文の取得本文が保存され、直前の回答は第一文を引用していた。

出典質問・通常本文要約・制約付き本文要約で参照を省略し、直前が同じケースの確定済み出典質問または本文要約なら、Coreはその封印入力、Goal、採用内容、出典依存、報告成果を既存の版別検証で照合する。直前結果が指した確定済み取得本文の識別子と文脈ハッシュで新しい要求を封印し、元の全文から今回の引用を作る。続行の各入力が同じ取得結果を直接指す。計算予算がゼロなら再照合に入らず、既存の予算停止に従う。文脈の明示指定、封印済み直接入力、一般の前回回答要約は従来の入口に従う。直前の無関係な確定結果で文脈が切れ、出典報告のケース関連付け欠落では `INPUT_FAILURE` が記録される。追加の本文取得は0件。

Linux x86_64 6.18.57、Rust/Cargo 1.99.1、製品 `0.7.0-alpha.11`。実装と試験3ファイルの `git diff --binary` SHA-256は `0698604200f25d85601628f6adb6a2ca8ca40d4769186484f5ae4621ef2aca09`。実CLIのSHA-256は `835fb0a41d33eb03814a7e66bc45b08a5dd588c4406e0e16b535eb5dd8935863`。

- `cargo fmt --all -- --check`、`cargo check --workspace --all-targets --locked`、`cargo build -p minidora-cli --locked` は終了0。関連Rustテストは入力39件、本文・要約13件、検索11件、復旧65件、通常CLI8件の計136件PASS。
- `python3 tools/local_tls_observer_test.py --binary <compiled-cli>` は終了0。通常取得19例ほかに加え、一度のGETから質問2件、通常要約、実ファイルへの制約付き要約、復旧後の追加質問を検査した。第2問は第1問に含まれなかった「予備値は7です。」を原文から引用した。全派生結果が同じ本文パケットに結び付き、復旧は同じ書込IDの `NO_RESEND` を記録し、バイト列、inode、更新時刻が一致した。予算ゼロ、無関係な確定結果、出典報告のケース関連付け欠落で停止した。検査前後のCLIハッシュは一致した。
- 旧CLIで確定した質問・通常要約・制約付き要約の3ケースを新CLIの別プロセスで開き、元の保存結果と本文依存を保持して追加質問を確定した。両試験の後続質問・要約・復旧に伴う追加接続は0件。外部転送、実プロバイダー通信は0件。全workspace実行試験、E7 QC・E8 QAは `NOT_RUN`。

凍結β・原典台帳・現物のblobが一致する `src/minidora/文脈照応.py` の226行を全文確認し、読解台帳を113件に更新した。旧版のセッション・所有スコープ、採用済み応答の識別子とハッシュ、曖昧な出力参照の停止条件を照合した。汎用の会話照応と保存形式の同等性は未検証。C11・C13の受入は `UNVERIFIED`。E6の要求・原典・試験の全数照合は継続中。リリースと配布は未実施。

復旧2世代は `dff0f18` と `85a8030`。終了時資産は7件。保持3件（要求トレース、実装記録、読解台帳）、稼働中4件（Core、復旧、TLS検査、既存共有Cargoキャッシュ）。保管済み・ゴミ箱移動・保留は各0件。比較用CLI複製、隔離DB、証明書、試験ファイルは除去済み。Windows保管、容量回収、指示者処分待ちは非該当。

## 2026-11-19 JST B129 — D3-C E6 出典質問・本文要約後の順位指定

着工点は `37955653a42a731e11e982107dc557eb2cd06b22`。D2仕様・工程とD3-C指示の採用SHA、R2 §12・§13・§14・§16、C04・C12・C13を照合した。着工点の実CLI `835fb0a41d33eb03814a7e66bc45b08a5dd588c4406e0e16b535eb5dd8935863` で、検索と1件目の本文取得のあと、出典質問・通常要約・制約付き要約を挟む3ケースを実行した。いずれも「2件目の本文を取得して」が `MISSING_SEARCH_RESULT_CONTEXT` で終了2となった。一覧と1件目の本文が保存され、2件目への接続は0件だった。

順位指定の通常入力で参照を省略し、直前が同じケースの確定済み出典質問または本文要約であれば、CoreがB128の封印入力・報告成果の照合で元の取得結果を特定する。その取得結果に検索一覧が残るときだけ、新しい入力をその結果へ結び付ける。既存の順位取得経路が一覧の保存バイトと順位を検証し、今回の読取許可・予算で指定本文を読み取る。元一覧は再検索せず、先に成功した本文と直前の質問・要約を保持する。照合対象は直前の確定結果が指す取得結果の一段。直接URL取得から一覧を推定しない。ファイル復旧は保存済みの一覧、順位、本文依存、書込IDを既存の検証で照合する。

Linux x86_64 6.18.57、Rust/Cargo 1.99.1、製品 `0.7.0-alpha.11`。実装と試験2ファイルの `git diff --binary` SHA-256は `79804dc3752a44a6482b0dcdd70f135b52fd1b88ffcab5cf15fae072e447e0c0`。実CLIのSHA-256は `daf69c83bb12aacf83bda7dcae9f26dd0dae795c729be6597ee4ff412a7df6d9`。

- `cargo fmt --all -- --check`、`cargo check --workspace --all-targets --locked`、`cargo build -p minidora-cli --locked` は終了0。関連Rustテストは入力39件、本文・要約13件、検索11件、取得ポート10件、復旧65件、通常CLI8件の計146件PASS。
- `python3 tools/local_tls_observer_test.py --binary <compiled-cli>` は終了0。通常取得19例ほかに加え、出典質問と両要約のあとに順位2を取得し、次の質問は順位2の原文を引用した。各経路の接続は一覧と本文2件だけ。実ファイルを別プロセスで復旧し、一覧から順位2への依存、書込ID、`NO_RESEND`、バイト列、inode、更新時刻を照合した。同一入力IDの再照会は同じ結果を返した。読取許可なし、一覧外の順位、期限ゼロ、元一覧のない直接取得からの順位指定は追加接続0件で停止した。検査前後のCLIハッシュは一致した。実プロバイダー通信・外部転送は0件。全workspace実行試験、E7 QC・E8 QAは `NOT_RUN`。

凍結β・原典台帳・現物のblobが一致する `src/minidora/hds参照計画.py` の224行を全文確認し、読解台帳を114件に更新した。旧版の問い署名と参照版、残利用回数、失効、正本の指定順での再読を照合した。旧索引圧縮と同一要求内の利用回数の同等性は未検証。C04・C12・C13の受入は `UNVERIFIED`。E6の要求・原典・試験の全数照合は継続中。リリースと配布は未実施。

復旧2世代は `3795565` と `dff0f18`。終了時資産は6件。保持3件（要求トレース、実装記録、読解台帳）、稼働中3件（Core、TLS検査、既存共有Cargoキャッシュ）。保管済み・ゴミ箱移動・保留は各0件。隔離DB、証明書、試験ファイルは検査終了時に消滅。Windows保管、容量回収、指示者処分待ちは非該当。

## 2026-11-19 JST B130 — D3-C E6 該当箇所がない質問のあとも元本文を使用

着工点は `6b225a89683ae10745a625ac2baa21e613b2a519`。D2仕様・工程とD3-C指示の採用SHA、R2 §13・§14、A12・C11・C13を照合した。着工点の実CLI `daf69c83bb12aacf83bda7dcae9f26dd0dae795c729be6597ee4ff412a7df6d9` で、取得本文にない「火山」の質問は `SOURCE_QUESTION_NO_MATCH` で終了2となった。封印入力は元の取得結果を保持していたが、続けて本文に含まれる「実測値」を尋ねても `MISSING_ACTUAL_RESULT` で終了2となった。取得から直接同じ質問をした対照は「実測値は42です。」を引用した。

参照を省略した同じケースの新しい出典質問・通常要約・制約付き要約では、Coreが直前の `SOURCE_QUESTION_NO_MATCH` に限り、停止の封印入力・確定取得結果のIDと文脈ハッシュ・本文パケット実体を照合する。停止時の話題で版別の語句照合を再演算し、保存済みの停止理由全文に一致する場合に元の取得結果を新しい入力へ封印する。新しい質問・要約は元の全文を使用する。旧停止、元本文、出力形式は保持し、停止した要求に答えや書込receiptを追加しない。予算ゼロの出典質問は本文照合へ入らず `RESOURCE_LIMIT` で停止する。対象外の停止理由からの再利用、順位取得の拡張、本文再取得は今回の施工範囲外。

Linux x86_64 6.18.57、Rust/Cargo 1.99.1、製品 `0.7.0-alpha.11`。実装と試験3ファイルの `git diff --binary` SHA-256は `857b74b86f5559f7aafea6ea66d635961d9e51d32655c5e1b341cf81a4973d35`。実CLIのSHA-256は `35a309e8923c4de1a005ab87c94481b163bbd752a3f07efcad881e538945c1cc`。

- `cargo fmt --all -- --check`、`cargo check --workspace --all-targets --locked`、`cargo build -p minidora-cli --locked` は終了0。関連Rustテストは入力39件、本文・要約13件、検索11件、復旧65件、通常CLI8件の計136件PASS。
- `python3 tools/local_tls_observer_test.py --binary <compiled-cli>` は終了0。取得19例ほかに加え、互いに独立したケースで追加質問と両要約が元本文の出典を保持した。同一入力IDの再照会で旧停止が一致し、停止に指定したファイルは生成されなかった。新しい質問の実ファイルは別プロセスで復旧し、書込ID、`NO_RESEND`、バイト列、inode、更新時刻が一致した。未対応の語、期限ゼロ、本文のケース関連付け欠落で停止した。後続処理の追加接続は0件。CLIハッシュは検査前後で一致した。
- 着工前CLIで停止を保存した別の3ケースを新CLIで開き、新しい質問と両要約、旧停止の保存結果一致を観測した。初期V1の互換分岐は原実装の停止理由と照合し、V1実CLIによる旧記録生成は未実施。実プロバイダー通信と外部転送は0件。全workspace実行試験、E7 QC・E8 QAは `NOT_RUN`。

凍結β・原典台帳・現物のblobが一致する `src/minidora/HDS候補再照合.py` の151行を全文確認し、読解台帳を115件に更新した。旧版の出典単位の集約、同等の支持の分離、根拠不足と候補競合による停止を照合した。複数候補の証拠評価と採否の同等性は未検証。A12・C11・C13の受入は `UNVERIFIED`。E6の要求・原典・試験の全数照合は継続中。リリースと配布は未実施。

復旧2世代は `6b225a8` と `3795565`。終了時資産は7件。保持3件（要求トレース、実装記録、読解台帳）、稼働中4件（Core、復旧検証、TLS検査、既存共有Cargoキャッシュ）。保管済み・ゴミ箱移動・保留は各0件。比較用CLI複製、隔離DB、証明書、試験ファイルは除去済み。Windows保管、容量回収、指示者処分待ちは非該当。

## 2026-11-19 JST B131 — D3-C E6 出典質問の該当なしから検索順位を指定

着工点は `07df4062852aedf462f9659949e71f80f1457f8e`。D2仕様・工程とD3-C指示の採用SHA、R2 §13・§14・§16、C04・C12・C13を照合した。着工点の実CLI `35a309e8923c4de1a005ab87c94481b163bbd752a3f07efcad881e538945c1cc` で、検索と1件目の本文取得に続いて「予備値」を尋ねると `SOURCE_QUESTION_NO_MATCH` で停止した。その直後の「2件目の本文を取得して」は、読取許可があっても `MISSING_SEARCH_RESULT_CONTEXT` で終了2となった。接続は一覧と1件目だけだった。1件目で一致する質問を挟んだ対照では、同じ一覧の2件目を取得した。

新しい順位指定で参照が省略された場合、Coreは直前の出典質問の該当なしをB130の封印入力と版別照合で再検証する。その質問が参照した確定取得結果に検索一覧があるときだけ、B129と共通の条件でその取得結果を新しい入力に封印する。既存の順位取得経路が保存済みの一覧と順位を再照合し、今回の読取権限・予算・保存済み待機条件で指定本文を取得する。元の検索は再実行せず、旧停止と元の取得本文を履歴に保持する。取得を試みて失敗した直後に適用されるB126の別順位条件は引き続き有効。ファイル復旧には既存の一覧・本文・書込IDの照合を使用した。

Linux x86_64 6.18.57、Rust/Cargo 1.99.1、製品 `0.7.0-alpha.11`。実装と試験2ファイルの `git diff --binary` SHA-256は `e13d359c559c5ad8b40fd591aa12b7a1ba550f541386b7a3e0eb3e903893323b`。実CLIのSHA-256は `c6fec2fd9f6b75e305151226fb9da2dd9f7070ea7bb6a0e1c36d64ce50819472`。

- `cargo fmt --all -- --check`、`cargo check --workspace --all-targets --locked`、`cargo build -p minidora-cli --locked` は終了0。関連Rustテストは入力39件、本文・要約13件、検索11件、取得ポート10件、復旧65件、通常CLI8件の計146件PASS。
- `python3 tools/local_tls_observer_test.py --binary <compiled-cli>` は終了0。通常取得19例ほかに加え、該当なしの停止後に同じ一覧の順位2を取得し、次の質問でその本文を引用した。接続は一覧と本文2件だけで、同一入力IDは保存済みの停止結果と取得結果をそれぞれ再現した。実ファイルの別プロセス復旧で、元の一覧への依存、書込ID、`NO_RESEND`、バイト列、inode、更新時刻が一致した。一覧外の順位、元の一覧がない直接取得、現入力の読取許可なし、有効な429の待機条件では追加接続0件。検査前後のCLIハッシュは一致した。実プロバイダー通信と外部転送は0件。全workspace実行試験、E7 QC・E8 QAは `NOT_RUN`。
- 同じCLIの隔離TLS検査では、ケースの所有者以外からの順位指定を接続前に拒否した。所有者が新しい入力として同じ順位を明示した場合は、変更された同じURLの本文を1回取得して旧本文も保持した。その入力IDの再照会による追加接続は0件だった。

凍結β・原典台帳・現物のblobが一致する `src/minidora/hds局所再照合.py` の159行を全文確認し、読解台帳を116件に更新した。旧版の語句による本文範囲候補、出典同一性、範囲候補がない場合の元本文参照を照合した。旧版の局所範囲選択の同等性は未検証。C04・C12・C13の受入は `UNVERIFIED`。E6の要求・原典・試験の全数照合は継続中。リリースと配布は未実施。

復旧2世代は `07df406` と `6b225a8`。終了時資産は6件。保持3件（要求トレース、実装記録、読解台帳）、稼働中3件（Core、TLS検査、既存共有Cargoキャッシュ）。保管済み・ゴミ箱移動・保留は各0件。隔離DB、証明書、試験ファイルは検査終了時に消滅。Windows保管、容量回収、指示者処分待ちは非該当。

## 2026-11-19 JST B132 — D3-C E6 複数URL取得の一部失敗後に成功本文を使用

着工点は `bd9fc1422c61dd22d8b5b5e5fc1b864fcee0ef6d`。D2仕様・工程とD3-C指示の採用SHA、R2 §13・§14・§16、C11・C13・E12を照合した。着工点の実CLI `c6fec2fd9f6b75e305151226fb9da2dd9f7070ea7bb6a0e1c36d64ce50819472` で、直接指定した2件のURLから200の本文と404の失敗を順に保存した。元の取得は `FETCH_OR_PARSER_FAILURE` で停止し、後続の出典質問・本文要約はいずれも `MISSING_ACTUAL_RESULT` で終了2だった。200本文だけを取得した対照は同じ質問に「実測値は42です。」を引用した。

通常の複数URL直接取得で全URLのパケットを観測し、成功本文と失敗の両方が保存された `FETCH_OR_PARSER_FAILURE` が対象。Coreは同じケースに登録された封印入力と停止結果、元URL順、元の許可経路、パケットの識別子と正規バイト列を照合する。新しい質問・通常要約・制約付き要約は成功本文を使い、元の取得が一部失敗したことを出力に表示する。元の取得は `SUSPEND` のまま保存し、失敗応答を後続の出典に含めない。元の取得で要求されたファイルは作らない。質問や要約を重ねる場合も元の停止結果を参照し、該当なしからの続行とファイル復旧の際は同じ証拠を再照合する。取消・中断・転送待機など、全件観測に至らなかった取得は今回の対象外。本文の文字範囲・版・内容の検証は既存の契約に従う。

Linux x86_64 6.18.57、Rust/Cargo 1.99.1、製品 `0.7.0-alpha.11`。実装と試験5ファイルの `git diff --binary` SHA-256は `193d13b483db4f2a9220c40f0f608430dbab23ac2353e7d8df9b168da2abc200`。実CLIのSHA-256は `15acd667f219783ca9cf686ec1ddcd4c0e2fcf6c5b467b7c77f2512984075167`。

- `cargo fmt --all -- --check`、`cargo check --workspace --all-targets --locked`、`cargo build -p minidora-cli --locked` は終了0。関連Rustテストは入力39件、本文・要約13件、検索11件、取得ポート10件、復旧65件、通常CLI8件の計146件PASS。
- `python3 tools/local_tls_observer_test.py --binary <compiled-cli>` は終了0。通常取得19例ほかに加え、200と404を保存した元の停止結果から質問・要約への継続を検査した。404応答にのみ含まれる語では `SOURCE_QUESTION_NO_MATCH` となり、次の質問では同じ200本文を引用した。本文が一件も成功しない場合と、保存パケットのケース関連付けを欠く場合は停止した。出典質問と本文要約の実ファイル2件を別プロセスで復旧し、書込ID、`NO_RESEND`、バイト列、inode、更新時刻が一致した。同一入力IDで元の停止結果を再現した。後続入力と復旧の追加接続は0件。CLIハッシュは検査前後で一致した。
- 着工前CLIで一部失敗を保存した2ケースを新CLIで開き、元の停止結果が一致し、質問と要約で成功本文を引用することを確認した。実プロバイダー通信と外部転送は0件。全workspace実行試験、E7 QC・E8 QAは `NOT_RUN`。

凍結β・原典台帳・現物のblobが一致する `src/minidora/取得意味境界.py` の27行を全文確認し、読解台帳を117件に更新した。旧版の本文型・上限・指紋・抽出時除外による意味欠落の検査を照合した。抽出除外から命題化までの同等性は未検証。C11・C13・E12の受入は `UNVERIFIED`。E6の要求・原典・試験の全数照合は継続中。リリースと配布は未実施。

復旧2世代は `bd9fc14` と `07df406`。終了時資産は9件。保持3件（要求トレース、実装記録、読解台帳）、稼働中6件（Core、入力封印、復旧、本文要約、TLS検査、既存共有Cargoキャッシュ）。保管済み・ゴミ箱移動・保留は各0件。比較用CLI複製、隔離DB、証明書、試験ファイルは除去済み。Windows保管、容量回収、指示者処分待ちは非該当。

## 2026-11-19 JST B133 — D3-C E6 保存済みの待機条件で転送と後続取得を停止

着工点は `689caebfdabd8afc3936e51d6b35c006b4318bcc`。D2仕様・工程とD3-C指示の採用SHA、R2 §13・§16、C02・C03・E04を照合した。着工点の実CLI `15acd667f219783ca9cf686ec1ddcd4c0e2fcf6c5b467b7c77f2512984075167` で、429または503の `Retry-After: 120` が残るホストへの直接取得は保存済みパケットIDを含む待機理由で止まった。同じURLへの302を挟むと、転送先には接続せず302を保存したが、結果は待機元IDのない `FETCH_OR_PARSER_FAILURE` となり、後続URLをGETした。

取得ポートは、許可済みの転送先が保存済みの待機条件で接続前に止まったとき、実際の転送応答と待機判定を同時にCoreへ返す。Coreは直接取得、メタデータ検索、順位指定本文の既存パケット形式で最後の観測済み3xxまでを保存し、待機元のパケットIDを含む `HTTP_RETRY_DEFERRED` で `SUSPEND` とする。その要求内の後続URLと代替検索は開始しない。3xx自身に待機指示がある場合は `HTTP_REDIRECT_RETRY_DEFERRED` として扱う。今回の転送先権限を先に照合し、権限がない場合は待機元IDを結果に出さない。次の明示的な別順位取得は、実際の転送が保存された失敗から元の一覧を再照合し、今回の権限と待機条件で判定する。

Linux x86_64 6.18.57、Rust/Cargo 1.99.1、製品 `0.7.0-alpha.11`。実装と試験4ファイルの `git diff --binary` SHA-256は `611136caea73f6da585ed3a528d0904f9ea58991b1275bbc5906eefd00eb682f`。実CLIのSHA-256は `b59e60124702d00f78929900f461e3cde527b7474fa51185cc64a30d2f3abfa4`。

- `cargo fmt --all -- --check`、`cargo check --workspace --all-targets --locked`、`cargo build -p minidora-cli --locked` は終了0。関連Rustテストは入力39件、本文・要約13件、検索11件、取得ポート10件、復旧65件、通常CLI8件の計146件PASS。
- `python3 tools/local_tls_observer_test.py --binary <compiled-cli>` は終了0。通常取得19例ほかに加え、429と503それぞれの待機元IDが直接取得の停止理由に入り、実302までの経路と先行200が保持された。3xxへの元待機指示の転記は0件。同一入力IDの再照会、転送先、後続URLへの追加接続は0件。検索・順位指定本文も実302と同じ待機元IDを保存し、別順位の許可済み取得は成功した。権限がない転送では待機元IDが出ず、別ケースは待機中のホストから取得できた。CLIハッシュは検査前後で一致した。
- 3ホストの隔離TLSでも、待機対象外のEurope PMCを代替先に指定した検索が302で止まり、代替先への接続0件だった。Europe PMCを単独で指定した対照は取得に成功した。初回の対照は治具URLの `resultType=core` 欠落で404となり、治具を修正して同条件で再検査した。実プロバイダー通信と外部転送は0件。全workspace実行試験、E7 QC・E8 QAは `NOT_RUN`。

凍結β・原典台帳・現物のblobが一致する `src/minidora/EuropePMC参照.py` の180行を全文確認し、読解台帳を118件に更新した。旧版の取得例外・空候補・異常応答の区別と、参照側での診断利用を照合した。旧版の供給器状態保存とRustの取得証拠契約の同等性は未検証。C02・C03・E04の受入は `UNVERIFIED`。E6の要求・原典・試験の全数照合は継続中。リリースと配布は未実施。

復旧2世代は `689caeb` と `bd9fc14`。終了時資産は8件。保持3件（要求トレース、実装記録、読解台帳）、稼働中5件（Core、取得ポート、検索、TLS検査、既存共有Cargoキャッシュ）。保管済み・ゴミ箱移動・保留は各0件。隔離DB、証明書、試験ファイルは除去済み。Windows保管、容量回収、指示者処分待ちは非該当。

## 2026-11-19 JST B134 — D3-C E6 転送待機で停止した取得から保存済み本文を使用

着工点は `df0f47e7cc72ab0ea7e9ce427e91b81fd4065dd6`。D2仕様・工程とD3-C指示の採用SHA、R2 §13・§14・§16、C11・C13・E12を照合した。着工点の実CLI `b59e60124702d00f78929900f461e3cde527b7474fa51185cc64a30d2f3abfa4` は、複数URLの直接取得で先行200と実際の302を保存し、保存済み429・503の待機または302自身の待機指示で停止した。後続の質問と要約は `MISSING_ACTUAL_RESULT` だった。転送先と後続URLへの接続は0件だった。

通常の複数URL直接取得で、保存済み成功本文があり、最後に観測した応答が停止理由に対応する3xxである場合を対象とした。Coreは元の封印入力、停止結果、観測済みURL順、各パケットのケース関連付け・内容・元の権限を照合する。末尾の3xxが自身の待機指示を返した場合はその指示の記録を求め、別の保存済み待機で止まった場合は元の指示を3xxの属性として扱わない。新しい質問・通常要約・制約付き要約は照合できた本文だけを使い、元の取得が転送待機で止まったことを表示する。元の取得は `SUSPEND` のまま保持する。転送応答のない初回接続前の待機、成功本文のない取得、ケースで照合できない記録では続行を停止する。

Linux x86_64 6.18.57、Rust/Cargo 1.99.1、製品 `0.7.0-alpha.11`。実装と試験5ファイルの `git diff --binary` SHA-256は `d97b9de36e4dc3be07f49af9d9705d1d788e47ae14bb4a66a16d9c67257deac7`。実CLIのSHA-256は `535fe3374e0d513610eed177022c3d6e1c2ad1f859aa32c9e2d86097c8875613`。

- `cargo fmt --all -- --check`、`cargo check --workspace --all-targets --locked`、`cargo build -p minidora-cli --locked` は終了0。関連Rustテストは入力39件、本文・要約13件、検索11件、取得ポート10件、復旧65件、通常CLI8件の計146件PASS。
- `python3 tools/local_tls_observer_test.py --binary <compiled-cli>` は終了0。通常取得19例ほかに加え、302自身の待機、保存済み429、保存済み503の3ケースで成功本文から後続の質問・通常要約・制約付き要約を作成した。新しいファイル3件を別プロセスで復旧し、書込ID、`NO_RESEND`、バイト列、inode、更新時刻が一致した。元の入力IDでは同じ停止結果を返した。再取得、未取得URLへの接続、元のファイル作成は0件。保存済みホスト待機は新しい本文出力の確定後も接続前に適用された。末尾パケットのケース関連付けがない場合も停止した。CLIハッシュは検査前後で一致した。
- 着工前CLIで保存した2種類の転送待機停止を新CLIで読み、元の停止結果を保持したまま成功本文を要約した。実プロバイダー通信と外部転送は0件。全workspace実行試験、E7 QC・E8 QAは `NOT_RUN`。

凍結β・原典台帳・現物のblobが一致する `src/minidora/純粋再利用.py` の34行を全文確認し、読解台帳を119件に更新した。旧版の入力全体と能力版に基づく純粋処理の再利用、および実取得との分離を照合した。旧版のキャッシュ契約と性能の同等性は未検証。C11・C13・E12の受入は `UNVERIFIED`。E6の要求・原典・試験の全数照合は継続中。リリースと配布は未実施。

復旧2世代は `df0f47e` と `689caeb`。初回のローカル復旧参照更新では旧SHA引数が欠け、更新が拒否された。遠隔とローカルの実値を再照合し、旧SHAを指定してローカルの二参照を同期した。終了時資産は9件。保持3件（要求トレース、実装記録、読解台帳）、稼働中6件（Core、入力封印、復旧、本文要約、TLS検査、既存共有Cargoキャッシュ）。保管済み・ゴミ箱移動・保留は各0件。比較用CLI複製、隔離DB、証明書、試験ファイルは除去済み。Windows保管、容量回収、指示者処分待ちは非該当。

## 2026-11-19 JST B135 — D3-C E6 次のURLの接続前に待機で止まった取得から保存済み本文を使用

着工点は `6db9dff89beafc9f5b9accc5a21f84cdd224fc6e`。D2仕様・工程とD3-C指示の採用SHA、R2 §13・§14・§16、C11・C13・E12を照合した。着工点の実CLI `535fe3374e0d513610eed177022c3d6e1c2ad1f859aa32c9e2d86097c8875613` は、複数URLの取得で200本文を保存した後、次のURLへの初回接続前に `EVIDENCE_UNAVAILABLE: HTTP_RETRY_DEFERRED` で止まった。以前の429・503から引き継いだ待機と、同一取得で受け取った429・503による待機をそれぞれ観測した。後続の出典質問と要約はいずれも `MISSING_ACTUAL_RESULT`、待機中のURLと後続URLへの接続は0件だった。

Coreは元の停止リビジョン、封印済みURL順、観測済みパケットの関連付けと内容、元の読取権限を照合する。観測済み件数の次にある許可済みURLのホストと停止理由を照合し、保存パケットID付きの待機は元の停止時点の同一ケース履歴にある有限な429・503から確認する。同一取得の待機は観測済み列にある直近の同一ホストの待機応答で確認する。質問、通常要約、制約付き要約は成功本文を根拠として使い、次のURLの接続前に待機で停止したことを表示する。元の停止と待機元を履歴に保持し、未接続URLに応答パケットを追加しない。再利用とファイル復旧は現在時刻による待機の再判定や通信を行わず、新しい外部取得はその時点の権限と待機条件に従う。

Linux x86_64 6.18.57、Rust/Cargo 1.99.1、製品 `0.7.0-alpha.11`。実装と試験6ファイルの `git diff --binary` SHA-256は `18dc4d7e97a37881180fa4a5240bb3a86bf3383dfea66828f0194a195b018358`。実CLIのSHA-256は `50705c8aaa86734d01a9385f00f5307ea57089459313b635cd3bf75bd2dbbc83`。

- `cargo fmt --all -- --check`、`cargo check --workspace --all-targets --locked`、`cargo build -p minidora-cli --locked` は終了0。関連Rustテストは入力39件、本文・要約13件、検索11件、取得ポート10件、復旧65件、Store31件、通常CLI8件の計177件PASS。
- `python3 tools/local_tls_observer_test.py --binary <compiled-cli>` は終了0。通常取得19例ほかに加え、取得と検索の保存済み待機、同一取得の503待機から後続の出典質問と両方の本文要約を検証した。新しいファイル3件を別プロセスで復旧し、書込ID、`NO_RESEND`、バイト列、inode、更新時刻が一致した。同一入力IDは元の停止結果を返した。新しい要求で待機中のホストを指定すると接続前に止まり、権限拒否と待機元パケットのケース関連付け欠落からの本文再利用も停止した。元の出力ファイルの作成、質問・要約と復旧の追加接続、再書き込みは0件。CLIハッシュは検査前後で一致した。
- 着工前CLIが保存したID付きの待機と同一取得内のIDなし待機から、新CLIで停止結果を保持したまま成功本文を要約した。ID付きのケースは期限が過ぎた後に検査した。実プロバイダー通信、外部転送は0件。全workspace実行試験、E7 QC・E8 QAは `NOT_RUN`。

凍結β・原典台帳・現物のblobが一致する `src/minidora/公開送信条件.py` の136行を全文確認し、読解台帳を120件に更新した。旧版の送信前停止、内部記録、公開表示の分離を照合した。公開送信判定の同等性は未検証。C11・C13・E12の受入は `UNVERIFIED`。E6の要求・原典・試験の全数照合は継続中。リリースと配布は未実施。

復旧2世代は `6db9dff` と `df0f47e`。終了時資産は10件。保持3件（要求トレース、実装記録、読解台帳）、稼働中7件（Core、取得ポート、復旧、Store、本文要約、TLS検査、既存共有Cargoキャッシュ）。保管済み・ゴミ箱移動・保留は各0件。比較用CLI複製、隔離DB、証明書、試験ファイルは除去済み。Windows保管、容量回収、指示者処分待ちは非該当。

## 2026-11-19 JST B136 — D3-C E6 取得期限で停止した結果から保存済み本文を使用

着工点は `ad1f9bac8e33ebf1846fa917db7e0633bbb1a606`。D2仕様・工程とD3-C指示の採用SHA、R2 §9・§13・§14・§16・§17、C11・C13・E04・E12を照合した。着工点の実CLI `50705c8aaa86734d01a9385f00f5307ea57089459313b635cd3bf75bd2dbbc83` は、次のURLへの接続前に `EVIDENCE_UNAVAILABLE: HTTP_DEADLINE_EXCEEDED: The retrieval deadline elapsed` で停止した。先行の200本文のみを保存した場合と、先行200本文に加えて次の200応答の24バイトを失敗パケットとして保存した場合を確認した。後続の出典質問・要約は `MISSING_ACTUAL_RESULT` だった。

Coreは停止時点の元ケースリビジョン、封印済みのURL順、次の未観測URLに対する元の読取許可、保存パケットのケース関連付けと内容を照合する。新しい出典質問・通常要約・制約付き要約は照合できた成功本文だけを使い、元の取得が期限で停止したことを表示する。新しい要求とファイル復旧は保存済みの停止を参照する。再取得と期限の再判定は0件。元の停止結果、受信途中で止まった実応答、未接続のURLをそれぞれ履歴で追跡できる。

Linux x86_64 6.18.57、Rust/Cargo 1.99.1、OpenSSL 3.5.7、製品 `0.7.0-alpha.11`。実装・試験4ファイルの `git diff --binary` SHA-256は `d022ef8abd664c552da7bf2d584b277d28856b613ade0c8e1881afa36bd4302d`。実CLIのSHA-256は `335634f9a7e42b6c43876685f1cfb3d770580a7b68f3913499493a6e7fdc8146`。

- `cargo fmt --all -- --check`、`cargo check --workspace --all-targets --locked`、`cargo build -p minidora-cli --locked` は終了0。関連Rustテストは入力39件、本文・要約13件、検索11件、取得ポート10件、復旧65件、Store31件、通常CLI8件の計177件PASS。
- `python3 tools/local_tls_observer_test.py --binary <compiled-cli>` は終了0。通常取得19例と検索・取得・復旧の既存経路に加え、新しい出典質問と両方の要約で元の停止結果IDと成功本文artifactを照合した。部分受信バイトと未接続URLが出力根拠に現れる件数は0件。各ファイルを別プロセスで復旧し、同一書込ID、`NO_RESEND`、バイト列、inode、更新時刻、サイズを確認した。元のファイルの作成は0件。同一入力IDで元の停止結果が一致した。成功本文が0件のケースは停止した。CLIハッシュは検査前後で一致した。
- 旧CLIで保存した停止4件で、新CLIの同一入力IDから同じ停止結果がバイト単位で返った。成功本文がある3件では質問・両方の要約とファイル復旧が成立し、成功本文がない1件では停止した。新CLIで先行200のみを保存した接続前の期限切れも再現し、要約を確認した。次のホストの読取許可不足、証拠バイト上限の2件では元の停止理由と後続要約の停止を確認した。後続要求と復旧による追加接続・再書き込み、実プロバイダーへの通信は0件。全workspace実行試験、E7 QC・E8 QAは `NOT_RUN`。

凍結β・原典台帳・現物のblobが一致する `src/minidora/製品版/検索送信予算.py` の263行を全文確認し、読解台帳を121件に更新した。旧版の取得制御と送信前予約、送信時の一回消費を照合した。旧版の提供者固有の累積予算、UTC期間、クラウド保存先の条件付き更新は未検証。C11・C13・E04・E12の受入は `UNVERIFIED`。E6の要求・原典・試験の全数照合は継続中。リリースと配布は未実施。

復旧2世代は `ad1f9ba` と `6db9dff`。終了時資産は8件。保持3件（要求トレース、実装記録、読解台帳）、稼働中5件（取得ポート、復旧、本文要約、TLS検査、既存共有Cargoキャッシュ）。保管済み・ゴミ箱移動・保留は各0件。比較用CLI複製、隔離DB、証明書、試験ファイルは除去済み。Windows保管、容量回収、指示者処分待ちは非該当。

## 2026-11-19 JST B137 — D3-C E6 通信開始回数・証拠バイト数の上限で止まった取得から保存済み本文を使用

着工点は `6e75b2c9871ec0a9ce507e3532184f19c3055466`。D2仕様・工程とD3-C指示の採用SHA、R2 §9・§13・§14・§16・§17、C11・C13・E04・E12を照合した。着工点の実CLI `335634f9a7e42b6c43876685f1cfb3d770580a7b68f3913499493a6e7fdc8146` は、通信開始回数または証拠バイト数の上限に達し、次のURLへの接続前に停止した。転送込みの成功本文、転送元の302を伴う停止、成功本文でバイト枠を使い切った停止、部分受信32バイトを保持した停止を観測した。4件の後続質問・要約は `MISSING_ACTUAL_RESULT` だった。

Coreは元の停止リビジョンと封印済みのURL順、次のURLの元の読取許可、ケースに紐づいた観測済みパケットを照合する。通信回数は元Goalの必須グラフで残された通信枠と保存済み経路の開始数を、証拠バイト数は元の上限と各パケットの生データの合計を照合する。質問と両方の要約は照合済みの成功本文を根拠にし、該当する上限を表示する。元の停止、先行した302や部分受信の失敗パケット、残っていた未接続URLを履歴で追跡できる。

Linux x86_64 6.18.57、Rust/Cargo 1.99.1、OpenSSL 3.5.7、製品 `0.7.0-alpha.11`。実装・試験4ファイルの `git diff --binary` SHA-256は `92cbf7e9b051d90e3c147e9d563488a32cd112140af46f4d8876406ee2672e3c`。実CLIのSHA-256は `b42ee875cb5240e6a51eae0ade0060b96c4e98cf1938c494cdc9539e7979a182`。

- `cargo fmt --all -- --check`、`cargo check --workspace --all-targets --locked`、`cargo build -p minidora-cli --locked` は終了0。関連Rustテストは入力39件、本文・要約13件、検索11件、取得ポート10件、復旧65件、Store31件、通常CLI8件の計177件PASS。
- `python3 tools/local_tls_observer_test.py --binary <compiled-cli>` は終了0。通常取得19例と検索・取得・復旧の既存経路が通過した。追加の上限停止4件から質問・通常要約・制約付き要約が成立し、失敗パケットと未接続URLが採用される件数は0件だった。後続の4ファイルを別プロセスで復旧し、同一の書込ID、`NO_RESEND`、バイト列、inode、更新時刻、サイズを確認した。元のファイルの作成は0件。読取許可の欠落と成功本文0件は停止した。CLIハッシュは検査前後で一致した。
- 旧CLIが保存した9件が、新CLIで同じ入力IDからバイト単位で一致した。上限停止の4件は保存済み本文による続行とファイル復旧が成立した。従来の転送待機・ホスト待機の2件も本文を利用できた。権限不足の2件と成功本文がない1件は停止した。元Goalの必須グラフまたは部分受信パケットがケースに紐づいていない2件では内容採用と作用を拒否した。後続処理・復旧の追加接続と再書き込み、実プロバイダー通信は0件。全workspace実行試験、E7 QC・E8 QAは `NOT_RUN`。

凍結β・原典台帳・現物のblobが一致する `src/minidora/製品版/Google認証.py` の327行を全文確認し、読解台帳を122件に更新した。旧版の当回取得制御、接続前の通信枠予約、受信チャンクの累積計上を照合した。認証SDKとクラウド通信の同等性は未検証。C11・C13・E04・E12の受入は `UNVERIFIED`。E6の全数照合は継続中。リリースと配布は未実施。

復旧2世代は `6e75b2c` と `ad1f9ba`。終了時資産は8件。保持3件（要求トレース、実装記録、読解台帳）、稼働中5件（取得ポート、復旧、本文要約、TLS検査、既存共有Cargoキャッシュ）。保管済み・ゴミ箱移動・保留は各0件。比較用CLI複製、隔離DB、証明書、試験ファイルは除去済み。Windows保管、容量回収、指示者処分待ちは非該当。

## 2026-11-19 JST B138 — D3-C E6 停止した直接取得の次の未接続URLを明示的に取得

着工点は `ebf6852c403dc35c49fcec0aa0f0763c8086f641`。D2仕様・工程とD3-C指示の採用SHA、R2 §9・§13・§14・§16・§17、C12・C13・E04・E12を照合した。着工点CLI `b42ee875cb5240e6a51eae0ade0060b96c4e98cf1938c494cdc9539e7979a182` では通信枠を使い切った直接取得の未接続URLを明示的に求めても `CAPABILITY_UNAVAILABLE` だった。検索順位を指定すると `MISSING_SEARCH_RESULT_CONTEXT` になり、URLを手入力して取得した場合は元の停止結果への依存を持たなかった。

「前回の取得で未接続の次のURLを取得して」は、新しい入力として封印する。Coreは直前の直接取得の認証済み停止、成功本文を含む観測済みパケット列、元のURL順を照合し、最初に未接続のURLを一つ導く。適用対象は初回接続前の待機、期限、通信開始数、証拠バイト数で停止した取得。新しいGoalが持つ許可・予算と既存の待機条件を適用し、元の転送経路で観測済みのURLへの再接続を拒否する。新しい結果の取得ノードは、元の停止結果の正準artifactに依存する。元の取得は `SUSPEND` として履歴に保持する。

Linux x86_64 6.18.57、Rust/Cargo 1.99.1、OpenSSL 3.5.7、製品 `0.7.0-alpha.11`。実装・試験6ファイルの `git diff --binary` SHA-256は `f0883e896e1c22e4281a6755f5e40d187dd35d63aefec56a79e90a4519dcda9a`。実CLIのSHA-256は `d497bba324e738e24dd9f29390ad624a70dcc3f25c29ccfcdf6be61f93fbbabc`。

- `cargo fmt --all -- --check`、`cargo check --workspace --all-targets --locked`、`cargo build -p minidora-cli --locked` は終了0。関連Rustテストはドメイン7件、入力39件、本文・要約13件、検索11件、取得ポート10件、復旧65件、Store31件、通常CLI8件の計184件PASS。
- `python3 tools/local_tls_observer_test.py --binary <compiled-cli>` は終了0。通常取得19例、検索・復旧の既存経路と、追加の直接取得続行7ケースが通過した。元結果、取得ノード、実取得パケット、最終描画の依存を照合し、出典質問は新規本文を使用した。同一入力の再生とファイル復旧で追加通信・再書き込みは0件。書込ID、`NO_RESEND`、バイト列、inode、更新時刻、サイズが一致した。確定直後の再要求、現在の読取許可不足、時間枠なし、全URL観測済み、保存済み待機での再接続は停止した。取得済み経路への転送では新しい302を保持した時点で停止した。CLIハッシュは検査前後で一致した。
- 旧CLIで保存した7件を新CLIで検査した。通信枠と証拠バイト枠で停止した2件から次の本文を取得・復旧し、元の停止結果も再生した。保存済み待機、読取許可不足、全件観測済み、元Goalグラフのケース紐づけ欠落では追加接続が0件だった。新しい指示種別を記録した混在DBを旧CLIで開くと、同DBの別ケースへの操作も `unknown variant FETCH_NEXT_UNOBSERVED_SOURCE` で終了1。旧CLI専用DBは正常に開けた。新しい履歴があるDBを扱うには新CLIが必要。

凍結β・原典台帳・現物のblobが一致する `src/minidora/HDS運用/資料補充.py` の314行を全文確認し、読解台帳を123件に更新した。旧版での不足判定、未消費候補の選択、失敗試行の保持と復旧時の再導出を照合した。同一発話内の追加検索と累積予算共有は未検証。C12・C13・E04・E12の受入は `UNVERIFIED`。E6の全数照合は継続中。実プロバイダー通信、全workspace実行試験、E7 QC・E8 QA、配布は `NOT_RUN`。

復旧2世代は `ebf6852` と `6e75b2c`。終了時資産は10件。保持3件（要求トレース、実装記録、読解台帳）、稼働中7件（ドメイン、Core、入力封印、取得ポート、復旧、TLS検査、既存共有Cargoキャッシュ）。保管済み・ゴミ箱移動・保留は各0件。比較用CLI複製、隔離DB、証明書、試験ファイルは除去済み。Windows保管、容量回収、指示者処分待ちは非該当。

## 2026-11-19 JST B139 — D3-C E6 取得に成功した続行から次の未接続URLへ進む

着工点は `5bcd26259d694410fe5589d5c68402e531ad8ff5`。D2仕様・工程とD3-C指示の採用SHA、R2 §9・§13・§14・§16・§17、C12・C13・E04・E12を照合した。着工点CLI `d497bba324e738e24dd9f29390ad624a70dcc3f25c29ccfcdf6be61f93fbbabc` は、最初の続行で1件の本文を取得したあと、次に残っていたURLを求める要求を `NO_PENDING_DIRECT_URL` で停止した。実接続は元の3ホップと最初の続行先の計4件だった。

「前回の取得で未接続の次のURLを取得して」は、直前に成功した続行からも元の停止記録へたどれる。Coreは各続行で封印された直前結果と、保存済みの成功パケット、その続行の読取許可、実行グラフを照合する。元のURL順に確認済みの件数を進め、次の1件だけを新しい要求の権限・予算で取得する。元の取得と成功済みの続行で観測した経路を転送時にも除外する。対象が尽きた場合と、次のURLがすでに観測済みの経路と一致する場合は停止する。元の停止結果は保存した状態で再生できる。適用対象は認証済みの元の停止、またはその元に結び付く直前の成功結果。最新結果がB138当時の未取得停止である場合は対象条件を満たさない。

Linux x86_64 6.18.57、Rust/Cargo 1.99.1、OpenSSL 3.5.7、製品 `0.7.0-alpha.11`。封印版は `direct-source-continuation-v2`、B138の封印版 `direct-source-continuation-v1` を読み取れる。実装・試験5ファイルの `git diff --binary` SHA-256は `ecb18b1fca6bcb6319f065b287537d91d723322b6a3d90d2395354c2ab88e9a4`。実CLIのSHA-256は `720485ab7c72520f4e9e0ed3724247914d291c5ea9c3cdc897486d45ec2cd6e8`。

- `cargo fmt --all -- --check`、`cargo check --workspace --all-targets --locked`、`cargo build -p minidora-cli --locked` は終了0。関連Rustテストは入力39件、本文・要約13件、検索11件、取得ポート10件、復旧65件、Store31件、通常CLI8件の計177件PASS。
- `python3 tools/local_tls_observer_test.py --binary <compiled-cli>` は終了0。通常取得19例と検索・復旧の既存経路、続行の関連10ケース（接続とGET各37件）が通過した。直前結果と新規取得パケットの依存を照合し、最初と次の続行のファイルを同じ書込ID・バイト列・inode・更新時刻で確認した。読取許可不足、残存URLなし、最初または直前の観測済み経路への転送は停止した。独立検査でも未接続URL3件を元の順に各1回取得し、そのあとの要求は接続0件だった。2件目で初めて許可したホストに対し、その要求の許可だけで接続することも確認した。CLIハッシュは検査前後で一致した。
- B138の保存済み8ケースを新CLIで検査した。続行・ファイル確認・出典質問が成立し、前のファイルを再照合済みのケースも次へ進めた。中間の実行グラフとケースの紐づけを外すと、`SOURCE_CONTEXT_INVALID` の終了4で追加接続・書込0件だった。新CLIの続行を保存した混在DBはB138の `inspect` でも開けた。履歴再生とファイル復旧の追加通信・再書き込みは0件。

凍結β・原典台帳・現物のblobが一致する `src/minidora/hds参照拡張.py` の99行を全文確認し、読解台帳を124件に更新した。旧版の取得済み資料の統合、不足分への追加観測、進展しない場合の停止を照合した。候補ラベルの被覆判定と検索式拡張の同等性は未検証。C12・C13・E04・E12の受入は `UNVERIFIED`。E6の全数照合は継続中。実プロバイダー通信、全workspace実行試験、E7 QC・E8 QA、配布は `NOT_RUN`。

復旧2世代は `5bcd262` と `ebf6852`。終了時資産は9件。保持3件（要求トレース、実装記録、読解台帳）、稼働中6件（Core、入力封印、取得ポート、復旧、TLS検査、既存共有Cargoキャッシュ）。保管済み・ゴミ箱移動・保留は各0件。比較用CLI複製、隔離DB、証明書、試験ファイルは除去済み。Windows保管、容量回収、指示者処分待ちは非該当。

## 2026-11-19 JST B140 — D3-C E6 読取許可不足で止まった続行を新しい許可で再開

着工点は `9097e69bd995e4352076de260390f030fc7bec05`。D2仕様・工程とD3-C指示の採用SHA、R2 §9・§13・§14・§16・§17、C12・C13・E11・E12を照合した。着工点CLI `720485ab7c72520f4e9e0ed3724247914d291c5ea9c3cdc897486d45ec2cd6e8` は最初の続行と、1件取得したあとの続行で、ホストの読取許可不足を実接続前に記録した。新しい入力IDから対象ホストを許可しても、同じ指示は `NO_PENDING_DIRECT_URL` で停止した。

「前回の取得で未接続の次のURLを取得して」は、直前の続行が接続前の `READ_PERMISSION_REQUIRED` で止まった場合にも同じURLを選べる。Coreは保存済み停止リビジョン、元Goal、依存する直前結果、当時の許可を照合し、取得パケット・採用内容・レンダリング・書込受領がないことを検証する。停止した分だけ元のURL順を進めず、新しい要求の対象ホスト許可、予算、保存済みの待機条件で取得する。旧入力IDでの再生は元の停止を返す。実際の転送応答を伴う停止や通常の直接取得に対する許可拒否は既存の停止条件に従う。

Linux x86_64 6.18.57、Rust/Cargo 1.99.1、OpenSSL 3.5.7、製品 `0.7.0-alpha.11`。封印版は `direct-source-continuation-v3`。実装・試験4ファイルの `git diff --binary` SHA-256は `c278279a9673324b9bf1fa8078dc6b9e666783726686e67c9f398af314bbc0ef`。実CLIのSHA-256は `302b64fa3d0b0389d48e2962e9597178a06ca38e9d287752c27e2c392e4b8d64`。

- `cargo fmt --all -- --check`、`cargo check --workspace --all-targets --locked`、`cargo build -p minidora-cli --locked` は終了0。関連Rustテストは入力39件、本文・要約13件、検索11件、取得ポート10件、復旧65件、Store31件、通常CLI8件の計177件PASS。
- `python3 tools/local_tls_observer_test.py --binary <compiled-cli>` は終了0。通常取得19例と検索・復旧の既存経路、追加5ケース（接続とGET各19件）が通過した。先頭と後続の許可不足、許可不足の繰り返し、同じ入力IDで許可だけ変えたときの衝突、再開後の次URLと終端を確認した。再開の結果は後続の出典質問に使われた。ファイルは同じ作用ID、`NO_RESEND`、バイト列、inode、更新時刻、サイズで再照合できた。別の検査では新しい許可を付けても保存済みの待機が有効なため、対象ホストへの追加接続は0件だった。CLIハッシュは検査前後で一致した。
- B139で保存した7ケースを新CLIで確認した。許可不足の記録からの再開により許可済み3件だけが追加GETとなり、元の停止結果は保持された。元Goalまたは直前の正準結果のケース紐づけを欠く2ケースでは終了4となり、追加接続・出力は0件。転送失敗や別の指示の後は停止条件が働いた。履歴再生・ファイル復旧は追加通信と再書き込み0件。

凍結β・原典台帳・現物のblobが一致する `src/minidora/ブラウザ接続.py` の63行を全文確認し、読解台帳を125件に更新した。旧版の明示要求の復元と外部読取許可の適用を照合した。旧版のブラウザ閲覧モジュール全体の同等性は未検証。C12・C13・E11・E12の受入は `UNVERIFIED`。E6の全数照合は継続中。実プロバイダー通信、全workspace実行試験、E7 QC・E8 QA、配布は `NOT_RUN`。

復旧2世代は `9097e69` と `5bcd262`。終了時資産は8件。保持3件（要求トレース、実装記録、読解台帳）、稼働中5件（入力封印、取得ポート、復旧、TLS検査、既存共有Cargoキャッシュ）。保管済み・ゴミ箱移動・保留は各0件。比較用CLI複製、隔離DB、証明書、試験ファイルは除去済み。Windows保管、容量回収、指示者処分待ちは非該当。

## 2026-11-19 JST B141 — D3-C E6 有限のホスト待機で止まった続行から再開

着工点は `b6b535432f66390b458f9e5de7aad0eca24da4e0`。D2仕様・工程とD3-C指示の採用SHA、R2 §9・§13・§14・§16・§17、C12・C13・E04・E12を照合した。着工点CLI `302b64fa3d0b0389d48e2962e9597178a06ca38e9d287752c27e2c392e4b8d64` は、最初の続行と成功後の続行で、有限な503応答のホスト待機により接続前に停止した。待機期限を過ぎて同じ入力IDを再生すると元の停止が返った。新しい入力IDでは `NO_PENDING_DIRECT_URL` となり、同じURLへの接続は0件だった。

「前回の取得で未接続の次のURLを取得して」は、保存済み応答による `HTTP_RETRY_DEFERRED` のため初回接続前に止まった続行も参照できる。停止リビジョンと元Goal、未接続URL、当時の読取許可、元の同一ケース履歴にある有限な待機パケットを照合する。待機停止は元のURL順を進めない。新しい入力IDの明示要求が、自身の予算と許可、その時点で有効なホスト待機を使って接続可否を判定する。観測済みの転送応答を伴う待機は従来の停止経路を適用する。履歴再生とファイル復旧は保存した観測を使う。

Linux x86_64 6.18.57、Rust/Cargo 1.99.1、OpenSSL 3.5.7、製品 `0.7.0-alpha.11`。封印版は `direct-source-continuation-v4`。実装・試験3ファイルの `git diff --binary` SHA-256は `f3f099dc54c5cf13f3fa70a6b3201e09a6832f9c9ff64a9221eb4143ad46f8e4`。実CLIのSHA-256は `0bbc81856700263095d0b9768b180c2a425a3a98f6288afd44a0d48f8b627f24`。

- `cargo fmt --all -- --check`、`cargo check --workspace --all-targets --locked`、`cargo build -p minidora-cli --locked` は終了0。関連Rustテストは入力39件、本文・要約13件、検索11件、取得ポート10件、復旧65件、Store31件、通常CLI8件の計177件PASS。
- `python3 tools/local_tls_observer_test.py --binary <compiled-cli>` は終了0。通常取得19例、検索・復旧の既存経路、追加3ケース（接続とGET各17件）が通過した。初回と成功後の続行で期限前と期限後を実測した。期限後に権限がない場合は停止し、許可した新要求は同じ未接続URLを取得した。次のURLへ進む経路、残り0件の停止、実302を伴う待機の停止も確認した。保存済み待機の入力IDは期限前後で同じ結果を返した。別プロセスでファイルを復旧し、同じ書込ID、`NO_RESEND`、バイト列、inode、更新時刻、サイズを確認した。出典質問は再開後に取得した本文を使用した。CLIハッシュは検査前後で一致した。
- B140で保存した5ケースを新CLIで検証した。有効な待機中の新しい入力IDは同じ保存済みパケットを指して停止し、追加接続は0件。期限後の新しい許可済み要求では3件だけ追加GETとなった。期限後に元の503パケットのケース紐づけを外した場合は `SOURCE_CONTEXT_INVALID` の終了4で停止し、追加接続・出力は0件だった。旧版の `NO_PENDING_DIRECT_URL` が最新のケースと観測済み転送待機も停止した。元の入力再生とファイル復旧による追加通信・再書き込みは0件。

凍結β・原典台帳・現物のblobが一致する `src/minidora/会話実行監督.py` の244行を全文確認し、読解台帳を126件に更新した。元の目的、資料、作用契約を固定する旧版の再試行と進展なしの停止を照合した。旧版の自動再計画・再試行の同等性は未検証。C12・C13・E04・E12の受入は `UNVERIFIED`。E6の全数照合は継続中。実プロバイダー通信、全workspace実行試験、E7 QC・E8 QA、配布は `NOT_RUN`。

復旧2世代は `b6b5354` と `9097e69`。終了時資産は7件。保持3件（要求トレース、実装記録、読解台帳）、稼働中4件（入力封印、復旧、TLS検査、既存共有Cargoキャッシュ）。保管済み・ゴミ箱移動・保留は各0件。比較用CLI複製、隔離DB、証明書、試験ファイルは除去済み。Windows保管、容量回収、指示者処分待ちは非該当。

## 2026-11-19 JST B142 — D3-C E6 接続前に予算不足で止まった続行を新しい予算で再開

着工点は `b61f65c782818c312bfc72c931d2868dd884ee24`。D2仕様・工程とD3-C指示の採用SHA、R2 §9・§13・§14・§16・§17、C12・C13・E04・E12を照合した。着工点CLI `0bbc81856700263095d0b9768b180c2a425a3a98f6288afd44a0d48f8b627f24` は、初回と成功後の続行で、時間枠ゼロ・作用数ゼロ・必須グラフに対する作用数不足・証拠バイト枠ゼロの8例を接続前に停止した。予算を補った新しい続行はいずれも `NO_PENDING_DIRECT_URL` で停止した。

Coreは元の停止理由、封印された要求と予算、必須成果の依存グラフ、直前結果を照合する。時間枠または作用数がゼロでURL解決前に止まった要求は、受付直前のケースリビジョンの結果も照合する。必須グラフに対する作用数不足は未観測の残件を照合する。証拠バイト枠ゼロの停止は当時のURL読取許可と、取得前に記録した直前結果を確認する。これらの停止では元の順序を進めず、新しい入力IDの要求が自身の予算、許可、ホスト待機を使って同じ未接続URLを取得する。元の入力IDは保存した停止を再生する。

Linux x86_64 6.18.57、Rust/Cargo 1.99.1、OpenSSL 3.5.7、製品 `0.7.0-alpha.11`。封印版は `direct-source-continuation-v5`。実装・試験5ファイルの `git diff --binary` SHA-256は `2ef6e50dc96b7947593722a537572f41afbd5968488fd1277612a0f286f51747`。実CLIのSHA-256は `d06cda6aa3660968e673e030cf1c4ed44f166110713b2a7c9eb5a40511d66d9b`。

- `cargo fmt --all -- --check`、`cargo check --workspace --all-targets --locked`、`cargo build -p minidora-cli --locked` は終了0。関連Rustテストは入力39件、本文・要約13件、検索11件、取得ポート10件、復旧65件、Store31件、通常CLI8件の計177件PASS。
- `python3 tools/local_tls_observer_test.py --binary <compiled-cli>` は終了0。通常取得19例と検索・復旧の既存経路、追加8ケース（接続とGET各33件）が通過した。4種類の予算停止、複数の停止を経由した取得、現在の権限不足、保存済みホスト待機、次URLと取得終了の動作を確認した。ファイル復旧で同じ書込ID、`NO_RESEND`、バイト列、inode、更新時刻、サイズを確認した。B141で保存したゼロ計算予算とゼロ証拠枠の2件は同じ入力IDで停止結果が一致し、新しい入力IDで各1件だけ対象URLを取得した。履歴再生とファイル復旧の追加通信・再書き込みは0件。CLIハッシュは検査前後で一致した。
- 認証付きローカルHTTPの `/v1/execute` で作用数ゼロを指定し、受付時点の最新結果を明示参照した場合は次の1件を取得した。すでに続行を済ませたケースで、それより古い停止結果を明示参照した場合は、新しい続行が `SOURCE_CONTEXT_INVALID` となった。取得済みURLへの再接続と効果の発生は0件だった。
- 別の実TLS検査で、転送元の302を観測したあとに通信開始数の上限に達した。302のパケットを保持し、新しい予算で同じ指示を続けた場合も接続0件で停止した。

凍結β・原典台帳・現物のblobが一致する `src/minidora/実行回復.py` の127行を全文確認し、読解台帳を127件に更新した。旧版の記録済み失敗に対する回復対象の選択、明示契約、同一作用の再試行上限を照合した。旧版の自動再試行と回数契約の同等性は未検証。C12・C13・E04・E12の受入は `UNVERIFIED`。E6の全数照合は継続中。実プロバイダー通信、全workspace実行試験、E7 QC・E8 QA、配布は `NOT_RUN`。

復旧2世代は `b61f65c` と `b6b5354`。終了時資産は9件。保持3件（要求トレース、実装記録、読解台帳）、稼働中6件（Core、入力封印、復旧、Store、TLS検査、既存共有Cargoキャッシュ）。保管済み・ゴミ箱移動・保留は各0件。比較用CLI複製、隔離DB、証明書、試験ファイルは除去済み。Windows保管、容量回収、指示者処分待ちは非該当。

## 2026-11-20 JST B143 — D3-C E6 旧版の未接続URLが残っていた停止から再開

着工点は `2818e6b75c152ca67a07c5a12d80f48b3e742040`。D2仕様・工程とD3-C指示の採用SHA、R2 §9・§13・§14・§16・§17、C12・C13・E04・E12を照合した。B138 `5bcd26259d694410fe5589d5c68402e531ad8ff5` から当時のCLIを再ビルドし、最初の続行に成功したあとの `NO_PENDING_DIRECT_URL` を通常入口から保存した。対象が残る場合も全件取得済みの場合も、当時の封印は `direct-source-continuation-v1` で、履歴はGoal作成前の `RECEIVED → SEALED → SUSPEND` だった。対象が残る保存データの複製では、着工点CLIも追加接続0件で停止した。

新しい明示要求を受けると、Coreはv1の停止リビジョン、入力受付時点の直前結果、最初の続行の取得グラフとパケット、元の直接取得のURL順を照合する。最初の続行に成功した直後の該当停止はURL順を進めず、未観測の次の1件だけを新しい要求の許可、予算、既存の待機条件で取得する。残件がない場合と、次が観測済みの転送経路の場合は停止する。元の入力IDでは保存済み結果を再生する。今回の履歴互換は、直前に最初の成功を持つB138のv1停止1件を対象とする。

Linux x86_64 6.18.57、Rust/Cargo 1.99.1、OpenSSL 3.5.7、製品 `0.7.0-alpha.11`。新規封印版は `direct-source-continuation-v6`。実装3ファイルの `git diff --binary` SHA-256は `2c0186c6f9cbd7b5700a979ed66badadf1b655c3498e6cd9d63056f293f1de96`、`Cargo.lock` は `b4e97fecdca8fd1fc863349b1a48eb99a925da7fed0ed6ce6cfe79bdd1cececc`。実CLIのSHA-256は新CLI `c8e8f6ebf8550c613f2380c18a87834e45c44008e28300531e52a46604fb0873`、B138再ビルド `4bc063683649c06c482e678169c0eceb5bfb08be473ac52b79471b7988bd8f10`、着工点CLI `d06cda6aa3660968e673e030cf1c4ed44f166110713b2a7c9eb5a40511d66d9b`。

- `cargo fmt --all -- --check`、`cargo check --workspace --all-targets --locked`、`cargo build -p minidora-cli --locked` は終了0。関連Rustテストは入力39件、本文・要約13件、検索11件、取得ポート10件、復旧65件、Store31件、通常CLI8件の計177件PASS。
- `python3 tools/local_tls_observer_test.py --binary <compiled-cli>` は終了0。通常取得19例と、出典質問・検索・ファイル復旧・直接取得の続行を含む既存経路が通過した。TLS検証は有効で外部転送は0件。CLIハッシュは検査前後で一致した。
- 旧B138で保存した未接続URLありの履歴を新CLIで開き、残る1件だけを接続・GET各1回で取得して `COMMIT` した。最初の取得、最初の続行、停止を各入力IDで再生し、保存値と照合した。新規結果の取得ノードは旧停止の正準artifactに依存する。出力ファイルを同じ書込ID、`NO_RESEND`、バイト列、inode、更新時刻、サイズで復旧し、追加取得と再書き込みは0件。全件取得済み、元の転送で観測済み、直前が別の計算、着工点CLIが保存したv5停止の4例も追加接続・GETは0件だった。
- 認証付きローカルHTTPの `/v1/execute` で、B138が実際の直前結果より古い成功を明示参照して停止した履歴を保存した。新CLIで同じ入力を再生すると元の停止が返り、新しい要求でこの停止を明示参照すると `SOURCE_CONTEXT_INVALID` になった。さらに古い成功を明示参照した要求も停止し、新しい2要求の接続・GETは0件だった。

凍結β・原典台帳・現物のblobが一致する `src/minidora/HDS運用/保存移行.py` の292行を全文確認し、読解台帳を128件に更新した。登録済みの旧保存版の検証、原記録の保全、依存関係と履歴を照合した。Pythonセッション全体の移行との同等性は未検証。C12・C13・E04・E12の受入は `UNVERIFIED`。E6の全数照合は継続中。実プロバイダー通信、全workspace実行試験、E7 QC・E8 QA、配布は `NOT_RUN`。

復旧2世代は `2818e6b` と `b61f65c`。終了時資産は7件。保持3件（要求トレース、実装記録、読解台帳）、稼働中4件（Core、入力封印、復旧、既存共有Cargoキャッシュ）。保管済み・ゴミ箱移動・保留は各0件。比較用CLI複製、旧版ビルド領域、隔離DB、証明書、試験ファイルは除去済み。Windows保管、容量回収、指示者処分待ちは非該当。

## 2026-11-20 JST B144 — D3-C E6 旧版の連続停止から元のURL順で再開

着工点は `1fb9cd1f8f9f115b912aca0d1eeae526a6fb79c5`。D2仕様・工程とD3-C指示の採用SHA、R2 §9・§13・§14・§16・§17、C12・C13・E04・E12を照合した。B138 `5bcd26259d694410fe5589d5c68402e531ad8ff5` の再ビルドで、最初の続行に成功したあとにv1の `NO_PENDING_DIRECT_URL` が3回続いた履歴を通常CLIから保存した。各停止は `RECEIVED → SEALED → SUSPEND` で、取得パケットとGoalは空だった。着工点CLIは元のURLが2件未接続のまま、新しい明示要求を `NO_PENDING_DIRECT_URL` で停止した。接続・GETは0件。

Coreは元の直接取得から初回の続行成功を照合し、その直後から連続するB138の停止を順に検証する。各停止で封印された文脈、保存済み結果、受付時点の直前結果を照合し、停止中に取得していないURLの順序を保つ。新しい明示要求が自身の許可、予算、現在のホスト待機を使って次の未観測URLを1件取得する。元のURLが尽きた場合や、次のURLを転送ですでに観測した場合は停止する。

Linux x86_64 6.18.57、Rust/Cargo 1.99.1、OpenSSL 3.5.7、製品 `0.7.0-alpha.11`。新規封印版は `direct-source-continuation-v7`。実装と検査の3ファイルを含む `git diff --binary` のSHA-256は `d5b74ce90dc0539a22ab64551484b86246bd6e0cb2ed76b9340ae61ca6a7ae37`、`Cargo.lock` は `b4e97fecdca8fd1fc863349b1a48eb99a925da7fed0ed6ce6cfe79bdd1cececc`。実CLIのSHA-256は新CLI `09801734540be88c1a6ba74abe975bee39ed181c2cc0113429a2f01b15cb4ce7`、B138再ビルド `e6279e54b82d5f308a52d5e5caef13d526a54060d99aacf0ddc7ed06dc7585fa`、着工点CLI `c8e8f6ebf8550c613f2380c18a87834e45c44008e28300531e52a46604fb0873`。

- `cargo fmt --all -- --check`、`cargo check --workspace --all-targets --locked`、`cargo build -p minidora-cli --locked` は終了0。関連Rustテストは入力39件、本文・要約13件、検索11件、取得ポート10件、復旧65件、Store31件、通常CLI8件の計177件PASS。
- 比較用の実TLS検査では、B138のv1停止3件を経た履歴から新CLIが未接続URLを正しい順番で1件ずつ `COMMIT` した。接続とGETは計2回ずつ。初回の新規取得は直前の旧停止に依存し、同一入力IDでは元の取得、初回の続行、旧停止3件、新規結果を保存どおり再生した。全件取得済み、次が転送で観測済み、直前が別の計算、受付時点より古い結果の明示参照を含む履歴では接続とGETが0件だった。着工点CLIが同じ履歴に対して保存したv6の停止もそのまま再生され、新しい要求の接続は0件だった。現在の読取許可がない要求も接続前に停止し、新しい許可を付けた要求から残る2件を順に取得できた。ファイルの復旧は同じ書込ID、`NO_RESEND`、バイト列、inode、更新時刻、サイズで一致し、追加通信も再書き込みも0件だった。
- `python3 tools/local_tls_observer_test.py --binary <compiled-cli> --legacy-b138-binary <compiled-b138-cli>` は終了0。通常取得19例と既存の検索・復旧経路に、旧CLIで停止を2回続けた履歴の回帰を追加した。旧結果の再生、最後のURLだけの1回接続、取得済み履歴の停止を検証した。追加した2ケース全体で接続とGETは各6回。TLS検証は有効で外部転送は0件。新旧CLIのハッシュは実行前後で一致した。
- `cargo clippy -p minidora-engine -p minidora-cli --all-targets --locked -- -D warnings` は終了101。着工点にもある関係比較の `needless_borrow` 3件と `cloned_ref_to_slice_refs` 1件が出た。これら2種類だけの除外後も、関係比較の統合テストで既存の `too_many_arguments` 1件により終了101。この3種類を検査コマンドの引数で除外すると終了0。

凍結β・原典台帳・現物のblobが一致する `src/minidora/HDS運用/原記録.py` の212行を全文確認し、読解台帳を129件に更新した。原バイトと版ごとの検証、同一セッション内の原記録・依存の再導出、採用前の照合を確認した。凍結Pythonの原記録形式全体との同等性は未検証。C12・C13・E04・E12の受入は `UNVERIFIED`。E6の全数照合は継続中。実プロバイダー通信、全workspace実行試験、E7 QC・E8 QA、配布は `NOT_RUN`。

復旧2世代は `1fb9cd1` と `2818e6b`。終了時資産は8件。保持3件（要求トレース、実装記録、読解台帳）、稼働中4件（入力封印、復旧、TLS検査、既存共有Cargoキャッシュ）、ゴミ箱移動1件（単独検査用ディレクトリ、139ファイル、592,722バイト）。保管済み・保留は各0件。旧版の隔離ビルドと比較用CLI複製は除去済み。単独検査用ディレクトリは原位置から移動済みで復元可能。Windows保管は非該当。

## 2026-11-20 JST B145 — D3-C E6 旧版の連続停止に重なったv6・v7停止から再開

着工点は `ae6c0e5fe6918ca8bf51b2601a3822f3e40b38a4`。D2仕様・工程とD3-C指示の採用SHA、R2 §9・§13・§14・§16・§17、C12・C13・E04・E12を照合した。B138 `5bcd26259d694410fe5589d5c68402e531ad8ff5` の再ビルドで、最初の続行のあとにv1停止を2件保存した。次にB143 `1fb9cd1f8f9f115b912aca0d1eeae526a6fb79c5` の再ビルドでv6停止を2件保存した。着工点B144は元のURLが2件残る同じ履歴から、v7の `NO_PENDING_DIRECT_URL` を2件保存した。各停止の追加接続・GETは0件だった。

Coreは、元の直接取得から最初のv1続行成功へ直結し、そのあとにv1停止が2件以上続いた履歴を確認する。その履歴上に連続するv6停止と、そのv6停止に続くv7停止を対象とする。各停止で封印された文脈、保存済み結果、受付時の直前結果、Goalと作用がないことを確認する。新しい明示要求は、元の取得順で次の未観測URLを1件選び、自身の許可、予算、現在のホスト待機を適用する。元のURLが尽きた場合と、次が観測済みの転送経路の場合は停止し、同じ入力IDでは保存済みの結果を再生する。

Linux x86_64 6.18.57、Rust/Cargo 1.99.1、OpenSSL 3.5.7、製品 `0.7.0-alpha.11`。新規封印版は `direct-source-continuation-v8`。実装・検査3ファイルの `git diff --binary` SHA-256は `fa1690253e8b5869b5e1a9548ded43b34f2e1adc68fdc89dd4b2320ad4f77daa`、`Cargo.lock` は `b4e97fecdca8fd1fc863349b1a48eb99a925da7fed0ed6ce6cfe79bdd1cececc`。実CLIのSHA-256は新CLI `493e95620ca8b1a7eaf325811d4ad6110a6442b62a613b4cc58639306020d201`、B138再ビルド `b1a72901926448b27ee786127c4ed62ed84cf818000c195eeecd25e4376a1a05`、B143再ビルド `7e8e7264e674ca4c6c846ae5790edd3a01c7a32c42efc533a2fd065158a2a43e`、着工点CLI `09801734540be88c1a6ba74abe975bee39ed181c2cc0113429a2f01b15cb4ce7`。

- `cargo fmt --all -- --check`、`cargo check --workspace --all-targets --locked`、`cargo build -p minidora-cli --locked` は終了0。関連Rustテストは入力39件、本文・要約13件、検索11件、取得ポート10件、復旧65件、Store31件、通常CLI8件の計177件PASS。

- 実TLSの独立検査は終了0。v1停止2件にv6停止2件が続いた履歴と、さらにv7停止2件を保存した履歴で、残るURLを新しい要求ごとに1件ずつ `COMMIT` した。各履歴で接続・GETは計2回ずつ。最初の新規結果は直前の旧停止に依存し、旧版と新版の同一入力IDの再生値はすべて一致した。全件取得済み、次が観測済みの転送経路、無関係な直前結果では接続0件。受付時より古い結果を明示参照した旧v1・v6・v7の停止を使うと `SOURCE_CONTEXT_INVALID` となり、接続0件だった。B142 `2818e6b` の再ビルド（CLI SHA-256 `f96bef45221349b623c4648125f3345cb967f427fb7d00ab95682986bf724e67`）が保存したv5停止は、その上にv6・v7停止が続く場合も接続0件で停止し、旧結果の再生値は一致した。現在の読取許可がない場合は接続前に停止し、許可した新しい要求で残るURLを順に取得した。ファイル復旧の書込ID、`NO_RESEND`、バイト列、inode、更新時刻、サイズは同一で、追加取得と再書き込みは0件。

- `python3 tools/local_tls_observer_test.py --binary <compiled-cli> --legacy-b138-binary <b138-cli> --legacy-b143-binary <b143-cli> --legacy-b144-binary <b144-cli>` は終了0。通常取得19例と既存の検索・復旧経路に加え、旧v1・v6・v7が混在する履歴2例で取得再開と終端を確認した。追加2ケース全体の接続とGETは各6回。TLS検証は有効で外部転送は0件。新旧4本のCLIのSHA-256は実行前後で一致した。
- `cargo clippy -p minidora-engine -p minidora-cli --all-targets --locked -- -D warnings` に、B144で確認した既存の `needless_borrow`・`cloned_ref_to_slice_refs`・`too_many_arguments` のコマンド引数による除外を加えた検査は終了0。

凍結β・原典台帳・現物のblobが一致する `src/minidora/長文脈管理.py` の390行を全文確認し、読解台帳を130件に更新した。保存版ごとの復元、追記と失効の再演算、履歴と資料選択の照合を確認した。凍結Pythonの長文脈復元・選択全体との同等性は未検証。C12・C13・E04・E12の受入は `UNVERIFIED`。E6の全数照合は継続中。実プロバイダー通信、全workspace実行試験、E7 QC・E8 QA、配布は `NOT_RUN`。

復旧2世代は `ae6c0e5` と `1fb9cd1`。今回の終了時資産は8件。保持3件（要求トレース、実装記録、読解台帳）、稼働中4件（入力封印、復旧、TLS検査、既存共有Cargoキャッシュ）、ゴミ箱移動1件（単独検査用ディレクトリ、305ファイル、1,351,258バイト）。保管済み・保留は各0件。旧版の隔離ビルドと比較バイナリを除去した。単独検査用ディレクトリは原位置から移動済みで復元可能。Windows保管は非該当。

## 2026-11-20 JST B146 — D3-C E6 v5停止とその後の保存停止から取得再開

着工点は `d8ad3eb724d4fd9c73327d1caf6348476cb78073`。D2仕様・工程とD3-C指示の採用SHA、R2 §9・§13・§14・§16・§17、C12・C13・E04・E12を照合した。B142 `2818e6b75c152ca67a07c5a12d80f48b3e742040` のv5は、B138 `5bcd26259d694410fe5589d5c68402e531ad8ff5` が保存したv1の `NO_PENDING_DIRECT_URL` から残るURLを解決せず、接続前に同じ停止理由を保存する。

Coreは、元の直接取得から最初のv1続行成功に直結し、v1停止が1件以上、続いてv5停止が1件以上ある履歴を対象とする。そのあとに版順で重なったv6・v7・v8停止も各版の条件と実際の受付前の結果で照合する。接続前の停止ではURL順を進めない。新しい明示要求が自身の許可・予算・現在の待機条件で次の未観測URLを1件選ぶ。元のURLが尽きた場合や次が観測済みの経路なら停止する。既存のv1から直接v6へ進む履歴にはv1停止2件以上の条件を適用する。

Linux x86_64 6.18.57、Rust/Cargo 1.99.1、OpenSSL 3.5.7、製品 `0.7.0-alpha.11`。新規封印版は `direct-source-continuation-v9`。実装・検査3ファイルの `git diff --binary` SHA-256は `9eb11a5ce0f2fd7cf8aa95433d86484a9698c238ab86be2cf3f7c210ca13b0f6`、`Cargo.lock` は `b4e97fecdca8fd1fc863349b1a48eb99a925da7fed0ed6ce6cfe79bdd1cececc`。実CLIのSHA-256は新CLI `9db622f84407b4a80f08a9e9a128be48cea4956a7de463149ccefc37104d9cbd`、B138再ビルド `20f9b70406abb3fed38550cbd76c1edf95a0d2cdcd8e7fd80583324f39517657`、B142再ビルド `adcf72993ce7ed480c1d488ea6965e9b201a429684befa93e1ea65ea615c6284`、B143再ビルド `4298214971146722a3b67c5d2f585b4fb9bce4ed838e30d58d9080ac937bf806`、B144再ビルド `b5526dce412cfe7571dd11c32d9a307c3f1765b9cafc5fd356a7202dfd409249`、着工点CLI `493e95620ca8b1a7eaf325811d4ad6110a6442b62a613b4cc58639306020d201`。

- `cargo fmt --all -- --check`、`cargo check --workspace --all-targets --locked`、`cargo build -p minidora-cli --locked` は終了0。関連Rustテストは入力39件、本文・要約13件、検索11件、取得ポート10件、復旧65件、Store31件、通常CLI8件の計177件PASS。

- 実TLSの独立検査は終了0。v1停止1件からv5停止1件に進む履歴、複数のv1・v5停止、v6またはv7が途中にない履歴を含む5種類を保存した。着工点CLIは各履歴にv8停止を2件記録し、接続・GETは0件だった。新CLIではv8停止を加える前と加えた後のどちらでも、未取得の2件を元の順で取得した。各履歴の接続・GETは2回ずつで、その後の要求は0回。旧版・新版の同一入力IDは以前の結果を返した。v5のない既存のv1→v6→v7の履歴でも、残る1件の取得を確認した。
- 全件取得済み、転送で観測済み、直前の結果が無関係、旧版と新版を逆順に戻した履歴、実際の直前結果より古い結果を参照したv5・v8停止はいずれも接続0件。古い参照は `SOURCE_CONTEXT_INVALID` になった。現在の読取許可を持たない要求も接続前に停止し、新しく許可した要求で残る2件を順に取得できた。混在履歴で出力したファイルは同じ書込ID、`NO_RESEND`、100バイトの内容、inode、更新時刻で復旧し、追加通信・再書き込みは0件だった。
- ClippyはB144で確認した既存の `needless_borrow`・`cloned_ref_to_slice_refs`・`too_many_arguments` をコマンド引数で除外し、`cargo clippy -p minidora-engine -p minidora-cli --all-targets --locked -- -D warnings` の対象を検査して終了0。

- `python3 tools/local_tls_observer_test.py --binary <compiled-cli>` に `--legacy-b138-binary`、`--legacy-b142-binary`、`--legacy-b143-binary`、`--legacy-b144-binary`、`--legacy-b145-binary` を指定した通し検査は終了0。通常取得19例と既存の検索・復旧経路に加え、旧v1・v5〜v8の履歴2例で取得再開と終端を確認した。追加2例全体の接続・GETは各6回。TLS検証は有効で外部転送は0件。新旧6本の実CLIのSHA-256は実行前後で一致した。初回は検査側のv5→v7入力を許容する修正のため中断し、最終版で通し検査をやり直した。

凍結β・原典台帳・現物のblobが一致する `src/minidora/HDS運用/会話文脈.py` の130行を全文確認し、読解台帳を131件に更新した。保存文脈の新旧形式、同一セッションの証明、資料の識別子・hash・文字範囲を照合した。凍結Pythonの会話証明処理全体との同等性は未検証。C12・C13・E04・E12の受入は `UNVERIFIED`。E6の全数照合は継続中。実プロバイダー通信、全workspace実行試験、E7 QC・E8 QA、配布は `NOT_RUN`。

復旧2世代は `d8ad3eb` と `ae6c0e5`。今回の終了時資産は9件。保持3件（要求トレース、実装記録、読解台帳）、稼働中4件（入力封印、復旧、TLS検査、既存共有Cargoキャッシュ）、ゴミ箱移動2件（単独検査用ディレクトリ、計634ファイル、2,835,674バイト）。保管済み・保留は各0件。旧版の隔離ビルドと比較バイナリを除去した。単独検査用ディレクトリは原位置から移動済みで復元可能。Windows保管は非該当。

## 2026-11-20 JST B147 — D3-C E6 v2〜v4の取得停止と後続版の停止から再開

着工点は `750a628c1acd2f5a9f29e76ed1e9a97cd5ce2ef2`。D2仕様・工程とD3-C指示を全読して採用SHAを照合し、R2 §9・§13・§14・§16・§17、C12・C13・E04・E12の該当契約を確認した。B139 `9097e69bd995e4352076de260390f030fc7bec05`、B140 `b6b535432f66390b458f9e5de7aad0eca24da4e0`、B141 `b61f65c782818c312bfc72c931d2868dd884ee24` は、B138 `5bcd26259d694410fe5589d5c68402e531ad8ff5` が保存した `NO_PENDING_DIRECT_URL` をたどれず、未取得URLが残る履歴に同じ停止を保存する。着工点B146でもその履歴からの接続・GETは0件で、v9停止が保存された。

Coreは元の直接取得に直結する最初のv1成功と、それに続く停止履歴を照合する。検証済みのv2・v3・v4の停止を含む履歴では、旧版に戻る操作が間にあっても、続くv2〜v9の停止を実際の直前結果と照合できる。停止の理由、封印された入力、実際の受け付け順、Goalと作用の不在を各件で確認する。新しい明示要求が現在の許可・予算・待機条件の下で、元の順番から次の未観測URLを1件選ぶ。

Linux x86_64 6.18.57、Rust/Cargo 1.99.1、OpenSSL 3.5.7、製品 `0.7.0-alpha.11`。封印版は `direct-source-continuation-v10`。実装と検査3ファイルの `git diff --binary` SHA-256は `763cd33461aa51d2c7e63059b9d3464fb802c6b97562150ffb3db39029a7aee5`、`Cargo.lock` は `b4e97fecdca8fd1fc863349b1a48eb99a925da7fed0ed6ce6cfe79bdd1cececc`。実CLIのSHA-256は新CLI `11b2b74d639a3ca7242b5461f18036182e3400c26d7231ca78b3b2a23edf1bbf`、着工点B146 `9db622f84407b4a80f08a9e9a128be48cea4956a7de463149ccefc37104d9cbd`、B138 `805c790409b5b7fa771367afb57717a6984e5a6ce47c802fcb710c7da5939786`、B139 `b13dad7eaaeb825d6ace9017d4863d7207fad7fa1db00783e3d84dcd7b5c13f3`、B140 `3aa7cb88bda328efb8b373c0c74009cc418cb55e928eabcbeca2e391abdfbc02`、B141 `db2948d30ec6163555219c69f065d9853886d93371acb4f92abd57f6a23b8558`。旧4版はコミットごとに分けたビルド先で `cargo build -p minidora-cli --locked --offline` が終了0。

- `cargo fmt --all -- --check`、`cargo check --workspace --all-targets --locked`、`cargo build -p minidora-cli --locked` は終了0。

- Rustの入力39件、復旧65件、保存31件、通常CLIの出典・本文経路8件の限定回帰はすべて終了0。計143件PASS。
- 実TLSの独立検査は終了0。v1成功後のv1停止からv2、v3、v4へそれぞれ進んだ3履歴と、停止を反復・混在させた履歴で、残るURLだけを元の順に取得した。着工点CLIは11履歴で各2回のv9停止を保存し、接続・GETは0回だった。新CLIではそのv9停止がある場合も、続行1回ごとに未観測URLを1件取得した。v4からv9に進んだ後にv2・v3へ戻った履歴と、v5が先にある履歴でも再開した。補助検査のB142 `2818e6b` は同じ独立ビルドで終了0、SHA-256は `348b4698b5255359d42b1856c8f75f2b97b9381ed81c0c599d11224354775e37`。
- 同じ入力IDは保存済み結果を再生した。全件取得済み、転送で次が観測済み、直前が無関係、v2〜v4の停止を含まないv9履歴は接続0回で停止した。実際の直前結果より古いものを指定したv2・v9の停止履歴は `SOURCE_CONTEXT_INVALID` で接続0回。現行の読取許可なしでも接続0回だった。新しい許可で再開し、ファイル復旧は同じ書込ID、`NO_RESEND`、100バイトの内容、inode、更新時刻を保持した。追加取得・再書き込みは0回。
- `tools/local_tls_observer_test.py` の `check_legacy_b138_direct_budget` を使い、実CLIでv2・v3・v4の単独3組、v2→v4→v3→v2→v9、v4→v2→v3→v9、従来のv1のみの計6組を実行した。終了0。各組で最後の未取得URLへの接続は1回、終端対照の追加接続は0回。TLS検証は有効で外部転送は0件。CLI引数の版違い、必要な旧版指定の欠落を含む7負例と引数の解決4例も通過した。Python通し検査と厳格Clippy再検査は、変更した復旧と隣接のCLI経路に検査を限定したため `NOT_RUN`。

凍結β・原典台帳・現物のblobが一致する `src/minidora/HDS構文化履歴.py` の85行を全文確認し、読解台帳を132件に更新した。直前のIRと現行IRの意味差分、返却する座標、構文化器の利用先を照合した。凍結Pythonの意味差分処理との同等性は未検証。C12・C13・E04・E12の受入は `UNVERIFIED`。E6の全数照合は継続中。実プロバイダー通信、全workspace実行試験、E7 QC・E8 QA、配布は `NOT_RUN`。

復旧2世代は `750a628` と `d8ad3eb`。今回の終了時資産は10件。保持3件（実装記録、要求トレース、読解台帳）、稼働中4件（入力封印、復旧、TLS検査、共有Cargoキャッシュ）、LinuxのGIOゴミ箱移動3件、保管済み・保留は各0件。移動元は `/workspace/.minidora-cloud/b147-legacy-review.5Lq3Bz`（隔離ビルド・比較用保存履歴、18,450ファイル、8,557,611,854論理バイト）、`/workspace/.minidora-cloud/b147-legacy-smoke-gphqtsic`（577ファイル、2,588,095バイト）、`/workspace/.minidora-cloud/b147-scoped-fixture-e5_hvxe9`（296ファイル、1,364,208バイト）。各元パスの不存在とTrashの実体・復元情報を照合した。合計19,323ファイル、8,561,564,157論理バイト、容量回収0、指示者の最終処分待ち3件。仮設TLS鍵は各検査プロセスの一時領域で扱った。Windows保管とオンライン専用化は非該当。

## 2026-11-20 JST B148 — D3-C E6 CSV内部結合の結果を元の行とセルに結び付けて説明

着工点は `7835beee946d9c3223e87c9a1f9426e653c27c99`。D2仕様・工程とD3-C指示を全読して採用SHAを照合し、R2 §14・§16とA14・B04・E08・E12・F04の該当契約を確認した。実CLIでCSV内部結合を確定してから「詳しく説明して」と続けると、着工点では結合結果のJSONが同一バイトで返った。依存は直前結果1件だった。対照のJSON表内部結合は左右の元行と元バイトを説明した。

新しい `ExplainPrevious` の封印版は `declarative-ja-en-v95-csv-join-explanation`。同じケースの確定済みCSV内部結合を参照すると、Coreは封印済みの両側CSVで結合を再計算し、保存済みの結果とレポート、元バイト・出力トークンの範囲、描画を照合する。説明には完全一致した列、左右の元行番号と元セル、未一致キー、元資料・入力・レポートのハッシュを含める。表示は一致と左右それぞれの未一致を先頭8行までとし、一致行は片側の先頭8列と結合キーまで出す。元セル全体は保存レポートに残る。Graphは封印入力→レポート→説明を結ぶ。着工点が封印したv89の説明は、その入力ID・文面・作用IDで復旧できる。

Linux x86_64 6.18.57、Rust/Cargo 1.99.1、製品 `0.7.0-alpha.11`。実装・検査5ファイルの `git diff --binary` SHA-256は `953f13ead7e942249c9f4b3f6216fb027cc79981c0a2a0a04aa3a63366f1f462`、`Cargo.lock` は `b4e97fecdca8fd1fc863349b1a48eb99a925da7fed0ed6ce6cfe79bdd1cececc`。実CLIのSHA-256は着工点 `11b2b74d639a3ca7242b5461f18036182e3400c26d7231ca78b3b2a23edf1bbf`、新CLI `4a41f77db30721209332180d68e30ed17afab88238508ce163547de79f5e0766`。

- `cargo fmt --all -- --check`、`cargo check --workspace --all-targets --locked`、`cargo build -p minidora-cli --locked` は終了0。Rustの入力39、文書11、通常CLIの説明11・CSV結合2の計63件PASS。追加検査で引用符・UTF-8・左右で異なる行順、未一致、原範囲とハッシュ、依存3件、計算Graph、描画を照合した。証拠予算不足と書込許可不足は型付き停止になり、新規書込と作用記録は0。
- 着工点の実CLIで保存した結合の後に、新CLIから説明をファイル出力した。新旧の同一入力IDはそれぞれ以前の結果と一致した。旧v89と新v95の実保存状態をコピーし、書込が物理的に済んだ後の `ADOPTED/ATTEMPTED` を再現した。別プロセスの `reconcile` がいずれも終了0で `LOCAL_FILE_RECONCILED_NO_RESEND` を返した。両ファイルの作用ID、バイト列、inode、更新時刻、サイズは同一で、再書き込みは0回。
- 一致行0件の実CLI説明も確定した。ケースからのレポート参照を外した隔離状態は `CSV_TABLE_CONTEXT_INVALID` で終了2となり、説明と作用記録は作られなかった。参照先ファイル自体を退避した場合はStore整合性検査で終了1、ケースの他主体からの操作は終了3。CSV絞り込み・並べ替えの通常説明には元の表を返す経路が残ることも観測した。

凍結β・原典台帳・現物のblobが一致する `src/minidora/構造化文書操作.py` 262行を全文確認し、読解台帳を133件に更新した。指定順の選択、元の値と見出しの位置、保存版からの再構成、実consumerを照合した。凍結Pythonの操作群との同等性は未検証。A14・B04・E08・E12・F04の受入は `UNVERIFIED`。E6の全数照合は継続中。全workspace実行試験、厳格Clippyの再検査、E7 QC・E8 QA、配布は `NOT_RUN`。

復旧2世代は `7835bee` と `750a628`。今回の終了時資産は12件。保持3件（実装記録、要求トレース、読解台帳）、稼働中6件（4実装ファイル、通常CLI検査、共有Cargoキャッシュ）、LinuxのGIOゴミ箱移動3件、保管済み・保留は各0件。移動元は `/workspace/.minidora-cloud/b148-baseline.80elTj`（692ファイル、4,595,469バイト）、`/workspace/.minidora-cloud/b148-join-discovery-7mr_2jag`（91ファイル、1,483,181バイト）、`/workspace/.minidora-cloud/b148-table-row-discovery-7exz4ury`（180ファイル、2,540,329バイト）。元パスの不存在とTrashの実体・復元情報を照合した。合計963ファイル、8,618,979バイト。容量回収0、指示者の最終処分待ち3件。Windows保管とオンライン専用化は非該当。

## 2026-11-20 JST B149 — D3-C E6 前回のCSV絞り込みと並べ替えを元の行から説明

着工点は `558c6fc5199056a30ddc6d5486c5d67c83d8e1e1`。現行D2仕様・工程とD3-C指示を全読し、採用索引のSHA-256とR2 §14・§16の追問・作用契約を照合した。着工点の実CLIでは、CSV絞り込み・並べ替えの直後の「詳しく説明して」が、それぞれ元の結果JSONを同一バイトで返した。

同じケースで確定した絞り込み・並べ替えを、Coreが封印済みの元CSVと演算条件から再計算する。再計算したレポートを保存済みの原バイト、確定結果、採用済みの内容計画と実描画に照合する。絞り込みでは完全一致条件と一致・不一致の件数、並べ替えではUnicode順と同値行の元の順序を説明する。出力行を元行に対応させ、選択列の元バイト範囲とハッシュを示す。絞り込みの判定行と出力行はそれぞれ先頭8行を表示し、全行の元位置を保存レポートで保持する。Graphは元入力→計算レポート→説明を結ぶ。

新規 `ExplainPrevious` は `declarative-ja-en-v96-csv-row-explanation` で封印する。着工点が封印したv95の絞り込み・並べ替え説明は当時のJSONで再生し、v95の結合説明は当時の元行・元セル付き文面で再生する。新規v96の結合説明も同じ結果と依存関係を使用する。実CLIは着工点 `4a41f77db30721209332180d68e30ed17afab88238508ce163547de79f5e0766`、新CLI `722f422a0f371cd6769fc946a8b7772f0af2f9e971bd098d469ebf8f2d7d4d68`。Linux x86_64 6.18.57、Rust/Cargo 1.99.1、製品 `0.7.0-alpha.11`、`Cargo.lock` SHA-256 `b4e97fecdca8fd1fc863349b1a48eb99a925da7fed0ed6ce6cfe79bdd1cececc`。

実装と通常CLI検査3ファイルの `git diff --binary` SHA-256は `fbba72b905a041e9f9c576064a62b7eb79c4c6c53e4d48ac6c30c2928aa2297a`。追加2検査の初回は、テストが封印済み入力を通常のケース成果物として取得したため失敗した。テストを通常CLIの同一ID再生と、実Graph上の封印入力ハッシュの照合へ修正した。

- `cargo fmt --all -- --check`、`cargo check --workspace --all-targets --locked`、`cargo build -p minidora-cli --locked` は終了0。入力39件、文書11件、通常CLIの説明13件・行操作4件・結合1件の計68件がPASS。追加2件は単独と説明群の両方で再検証した。通常CLIの昇順と降順で `01`、`10`、`2` の文字列順と同値行の元順序を照合した。
- 旧実CLIの4ケースを新実CLIで開いた。完全一致、0件、引用の有無が異なる空欄の一致、重複キーを持つ文字列順について、原範囲とハッシュ、実描画、入力とレポートのGraph依存を照合した。10行の結果では先頭8行と残り2行の案内、保存された全10行を確認した。別主体、別ケース、不一致の書込許可、証拠予算1バイト、ケースのレポート参照欠落の5件は新規ファイル0件で停止した。
- 旧v95・新v96それぞれ4件の実ファイルを採用後かつ作用試行後の状態から別プロセスで復旧した。8件とも `LOCAL_FILE_RECONCILED_NO_RESEND`。元の作用ID、物理ファイルのバイト、inode、更新時刻を保持し、再書き込み0件。旧入力の同一ID再実行も保存済み結果と一致した。

凍結β・原典台帳・今回の着工点の同一blob `eafce5ea96c4d4377fdc420d73127ef8ec73c2ee` に基づき、`src/minidora/会話数量.py` 全235行を実consumerと照合した。原資料を再構成して行条件から一意の数量を選び、元位置・単位・条件・時点を保持し、数量比較前に両方の記録を再計算する。読解台帳は134/416。凍結版の実行と複数行操作としての同等性は未検証。要件93件の受入は `UNVERIFIED`。E6全数接続は継続、全workspace実行試験、厳格Clippy再検査、E7体系QC・E8独立QA、公開・配布は `NOT_RUN`。

復旧2世代は `558c6fc` と `7835bee`。終了時資産は計8件。保持3件（実装記録、要求トレース、読解台帳）、稼働中4件（2実装ファイル、通常CLI検査、共有Cargoキャッシュ）、保管済み・保留は各0件。検証用 `/workspace/.minidora-cloud/b149-legacy-review.aw1xcU` 1件をLinuxのGIOゴミ箱に移動し、元パスの不存在、収容物、復元情報を照合した。1,854ファイル、168,594,490論理バイト。指示者の最終処分待ち1件、容量回収0。Windows保管とオンライン専用化は非該当。

## 2026-11-20 JST B150 — D3-C E6 前回のCSV集計を元のグループ・行・計測値から説明

着工点は `e1f4c9d15ee2dbf8e6a3f7f26681aa456419aa09`。D2仕様・工程、D3-C指示の全読と採用SHA-256照合、R2 §14・§16の追問および作用の契約照合を実施した。着工点の通常CLIでCSVグループ件数、見出しだけの件数、グループ別統計を確定した直後の「詳しく説明して」は、先行結果のJSONを同一バイトで返した。

既存Coreが、同じケースで確定した `CsvGroupCount` と `CsvGroupStatistics` を封印済み入力から再計算し、保存レポートの原バイト、確定結果、採用済み内容計画と実描画を照合する。説明は完全一致するキーと初出順、件数、元行と元のキーセルを示す。統計では計測セル、単位をそろえた各値、合計・平均・中央値・範囲・分散を厳密値で示す。表示は先頭8グループ、各グループ先頭4行までで、全行の根拠は保存レポートで保持する。Graphは封印入力→計算レポート→説明を結ぶ。

新規 `ExplainPrevious` は `declarative-ja-en-v97-csv-group-explanation` で封印する。既存v96の集計説明は保存当時のJSONで再生し、v96のフィルター説明は保存当時の元セル付き文面で再生する。実CLI SHA-256は着工点 `722f422a0f371cd6769fc946a8b7772f0af2f9e971bd098d469ebf8f2d7d4d68`、新CLI `c91c43c1b2d907519735e69a75f9521ad0410139584fd60b18906a47aea8f592`。Linux x86_64 6.18.57、Rust/Cargo 1.99.1、製品 `0.7.0-alpha.11`。`Cargo.lock` SHA-256 `b4e97fecdca8fd1fc863349b1a48eb99a925da7fed0ed6ce6cfe79bdd1cececc`。実装と通常CLI検査3ファイルの `git diff --binary` SHA-256 `d90701ad38194b17c345398912af450ff13f35943cbf8b5d5ba572bb425c6025`。

- `cargo fmt --all -- --check`、`cargo check --workspace --all-targets --locked`、`cargo build -p minidora-cli --locked` は終了0。入力39件、文書11件、通常CLIの説明15件・件数4件・隣接統計4件の計73件がPASS。追加2件は通常CLIの同一ID再生、保存レポートの原バイト、原入力とレポートのGraph依存、結果の描画を確認した。
- 独立した実CLI検査では、空文字の引用あり・なしが同じグループに入る一方、原バイト範囲の長さ0と2を区別した。1 m・50 cm・2 mのグループは合計 `7/2` m、平均 `7/6` m、母分散 `7/18` m²、標本分散 `7/12` m²。単位なしの分数値と、1件での標本分散の扱いも照合した。10グループ計15行で、先頭8グループと先頭グループの4行を表示し、残りの件数と保存レポート全15行を確認した。書込許可の対象不一致、証拠予算1バイト、ケースのレポート参照欠落、別主体は、新規ファイルと作用受領記録0件で停止した。
- 旧v96・新v97それぞれ3件の実ファイルについて、保存された `ADOPTED/ATTEMPTED` 状態から別プロセスで復旧した。全6件が `LOCAL_FILE_RECONCILED_NO_RESEND`。元の作用ID、ファイルのバイト、inode、更新時刻を保持し、再書き込み0件。旧入力の同一ID再実行も当時の結果と一致した。

凍結β・原典台帳・着工点でblob `0a84d28018225612d3625b93ea4ce4ca8d1864ca` が一致する `src/minidora/会話回答.py` 全131行と通常呼出経路を照合した。前回の実成果と根拠・留保を再構成して表示条件を適用し、汎用文書結果は本文引用として扱う。読解台帳は135/416。凍結版の実行と同等性は未検証。93要件の受入は `UNVERIFIED`。E6は継続し、JSON表のグループ件数からの詳説が残る。全workspace実行試験、厳格Clippy再検査、E7体系QC、E8独立QA、公開・配布は `NOT_RUN`。

復旧2世代は `e1f4c9d` と `558c6fc`。終了時資産は計8件。保持3件（実装記録、要求トレース、読解台帳）、稼働中4件（2実装ファイル、通常CLI検査、共有Cargoキャッシュ）、保管済み・保留は各0件。検証用 `/workspace/.minidora-cloud/b150-legacy-review.Adp80F` 1件をLinuxのGIOゴミ箱へ移動し、元パスの不存在・収容物・復元情報を照合した。1,406ファイル、185,213,247論理バイト。指示者の最終処分待ち1件、容量回収0。Windows保管とオンライン専用化は非該当。

## 2026-11-20 JST B151 — D3-C E6 JSON表のグループ件数を元のセルから説明

着工点は `6ce080e63c46844e495341884ba38b7bb3abc7cb`。現行D2仕様・工程とD3-C指示を全読し、採用SHA-256とR2 §14・§16を照合した。着工点の通常CLIでは、JSON表のグループ件数を確定したあとの「詳しく説明して」が、行のある表と空表の両方で元の結果JSONを同一バイトで返した。

既存Coreは同じケースの確定済み `JsonTableGroupCount` を封印済みの元入力から再計算し、先行結果、保存レポート、採用内容計画と実描画を照合する。説明は完全一致するデコード済み文字列の初出順と件数、列見出しと各元セルのJSON Pointer・バイト範囲・ハッシュを示す。表示は先頭8グループ、各グループ先頭4行。保存レポートは全元行の範囲とハッシュを保持する。既存のJSON表操作のGraphで、封印入力→再計算レポート→説明を結ぶ。

新規 `ExplainPrevious` は `declarative-ja-en-v98-json-group-count-explanation` で封印する。旧v97のJSON件数の追問は当時のJSON、旧v97のCSV集計・JSON絞り込み説明は当時の根拠付き文面と依存関係で再生した。新規v98でもこの隣接経路の文面と根拠は一致した。Linux x86_64 6.18.57、Rust/Cargo 1.99.1、製品 `0.7.0-alpha.11`。実CLIのSHA-256は旧 `c91c43c1b2d907519735e69a75f9521ad0410139584fd60b18906a47aea8f592`、新 `59c533806daeb9066818690b8bf8ed31bda2f7b85af35c60434b93ec83c0ff37`。`Cargo.lock` は `b4e97fecdca8fd1fc863349b1a48eb99a925da7fed0ed6ce6cfe79bdd1cececc`、実装と通常CLI検査5ファイルの `git diff --binary` は `5a4c36477ee7f0dbdf9ce3b7ec1b83e3aaeb744a67ad957cdcd2444040f61bdf`。

- `cargo fmt --all -- --check`、`cargo check --workspace --all-targets --locked`、`cargo build -p minidora-cli --locked` は終了0。入力39件、文書11件、通常CLIの説明16件・件数4件・隣接統計2件の計72件がPASS。追加1件では日英の集計指示と空表を含む4ケースで、入力原文の既知の行・キーのトークンから位置とハッシュを独立に算出し、説明の元行とJSON Pointer、三つの依存、同一ID再生と実描画に照合した。
- 実CLIで11行8グループと空表0行を照合した。エスケープと直接表記はデコード後の文字列が一致する場合に同じグループ、大文字・小文字、末尾空白、Unicodeの合成済み文字・分解表記は異なる文字列として計数した。10グループ15行の試験では表示範囲と残件案内、保存レポート全15行を確認した。書込許可の対象不一致、証拠予算1バイト、ケースのレポート参照欠落、別主体は、新規ファイルと受領記録0件で停止した。
- 旧v97・新v98それぞれ2件の実ファイルを、保存された `ADOPTED/ATTEMPTED` から別プロセスで復旧した。4件とも `LOCAL_FILE_RECONCILED_NO_RESEND`。元の作用ID、ファイルのバイト、inode、更新時刻を保持し、再書き込み0件。同一IDによる再実行も各保存結果と一致した。

凍結版・原典台帳・着工点でblob `7b0630436ae1f6a2a42a651aea1613a2668b2f74` が一致する `src/minidora/構造化文書.py` 全264行を、原文と明示設定を渡す入口および再構成する操作側と照合した。JSONの数値字句・配列順・プロパティ順、キーと値の原文範囲、重複キー・余剰入力の停止を確認した。読解台帳は136/416。凍結版の実行、文字位置とバイト範囲の対応および集計の同等性は未検証。93要件の受入は `UNVERIFIED`。E6は継続中で、CSVの列選択や形式変換後の追問が残っている。全workspace実行試験、厳格Clippy再検査、E7体系QC、E8独立QA、公開・配布は `NOT_RUN`。

復旧2世代は `6ce080e` と `e1f4c9d`。終了時資産は計10件。保持3件（実装記録、要求トレース、読解台帳）、稼働中6件（4実装ファイル、通常CLI検査、共有Cargoキャッシュ）、保管済み・保留は各0件。検証用 `/workspace/.minidora-cloud/b151-legacy-review.D8LGen` 1件をLinuxのGIOゴミ箱に移動し、元パスの不存在、収容物、復元情報を照合した。1,129ファイル、175,855,115論理バイト。指示者の最終処分待ち1件、容量回収0。Windows保管とオンライン専用化は非該当。

## 2026-11-20 JST B152 — D3-C E6 CSVの列名選択を元の見出し・セルから説明

着工点は `d1c933044a14dfd880995aae6d507a3439bc1415`。現行D2仕様・工程とD3-C指示を全読し、採用SHA-256とR2 §14・§16を照合した。着工点の通常CLIでは、列名によるCSV選択後の「詳しく説明して」が、選択済みの文字列配列を同一バイトで返した。`csv_select_index` はライブラリ関数で、通常CLIの選択入力は `CsvSelect` の列名指定。

既存Coreは同じケースの確定済み `CsvSelect` を封印済みの原入力から再計算し、保存レポート、先行結果、採用内容計画、実描画を照合する。説明は完全一致した見出し名と元列番号、選択件数、元のデータ行順に並んだ文字列、見出しと元セルのバイト範囲・ハッシュを示す。先頭8行を表示し、保存レポートは全選択値の元位置と出力位置を保持する。既存Graphで封印入力→計算レポート→説明を結ぶ。新規 `ExplainPrevious` は `declarative-ja-en-v99-csv-select-explanation`。保存済みv98の列選択追問は当時の配列で再生する。

Linux x86_64 6.18.57、Rust/Cargo 1.99.1、製品 `0.7.0-alpha.11`。実CLIのSHA-256は旧 `59c533806daeb9066818690b8bf8ed31bda2f7b85af35c60434b93ec83c0ff37`、新 `0be09a22b2b79d35d4a77b0ffd06c8111399a42c0b03d1f0aa9868d4783079ae`。`Cargo.lock` は `b4e97fecdca8fd1fc863349b1a48eb99a925da7fed0ed6ce6cfe79bdd1cececc`、実装と通常CLI検査3ファイルの `git diff --binary` は `0214e13a984d762c07e9ad33d5c1154b66e7e5c2941abee25a9e8dbbcbaa7c40`。初回の追加試験では、空欄の行を探すテスト側の `d,` が見出し `id,` にも一致した。CRLFで始まる元データ行に位置照合を修正した。

- `cargo fmt --all -- --check`、`cargo check --workspace --all-targets --locked`、`cargo build -p minidora-cli --locked` は終了0。入力39件、文書11件、通常CLIの説明17件・選択2件・後続計算と投影1件の計70件がPASS。追加1件では日英の入力、数に見える列名、見出しのみの4ケースで、既知の入力トークンから元バイトとハッシュを独立に算出し、元の出力位置、説明の行位置、三つの依存、同一ID再生、実描画に照合した。
- 独立した実CLI検査で、引用符、引用内CRLF、引用の有無が異なる空欄、同じ文字列の複数出現、数に見える名前・値、見出しのみの0件を照合した。10行では先頭8行と残り2件の表示、保存レポート全10行を確認した。既存のCSV・JSON件数説明はv98再生とv99新規実行で文面・根拠が一致した。書込許可の対象不一致、証拠予算1バイト、ケースのレポート参照欠落、別主体は新規ファイルと受領記録0件で停止した。
- 旧v98・新v99それぞれ4件の実ファイルを `ADOPTED/ATTEMPTED` から別プロセスで復旧した。全8件が `LOCAL_FILE_RECONCILED_NO_RESEND`。元の作用ID、ファイルのバイト、inode、更新時刻を保持し、再書き込み0件。同一IDによる再実行も各保存結果と一致した。

凍結版・原典台帳・着工点でblob `180e26bf91dfc0ea716d529dd7144ceb78bce69a` が一致する `src/minidora/構造化文書接続.py` 全58行を、合成入力の供給元と文書操作の呼出先に照合した。上流の成功結果一件と明示設定を検査し、読取には原文、操作には上流成果全体を渡す。読解台帳は137/416。凍結版の実行・通常入口と同等性は未検証。93要件の受入は `UNVERIFIED`。E6は継続中で、CSVとJSONの形式変換後の追問が残っている。全workspace実行試験、厳格Clippy再検査、E7体系QC、E8独立QA、公開・配布は `NOT_RUN`。

復旧2世代は `d1c9330` と `6ce080e`。終了時資産は計8件。保持3件（実装記録、要求トレース、読解台帳）、稼働中4件（2実装ファイル、通常CLI検査、共有Cargoキャッシュ）、保管済み・保留は各0件。検証用 `/workspace/.minidora-cloud/b152-legacy-review.CJbFyN` 1件をLinuxのGIOゴミ箱へ移動し、元パスの不存在、収容物、復元情報を照合した。1,876ファイル、168,714,707論理バイト。指示者の最終処分待ち1件、容量回収0。Windows保管とオンライン専用化は非該当。

## 2026-11-20 JST B153 — D3-C E6 CSVとJSONの変換後に元セルと出力セルを説明

着工点は `3dd5f8d689ab487e61274466047f701ebe106774`。D2仕様・工程、D3-C指示を全読し、採用SHA-256とR2 §14・§16を照合した。着工点の通常CLIでは、CSV→JSONとJSON→CSVを確定した直後の「詳しく説明して」が、ヘッダーあり・なしの両方で変換済み成果を同一バイトで返した。

既存Coreが同一ケースの封印済み `CsvToJson` または `JsonToCsv` を再計算し、保存レポート、先行結果、採用内容計画、実描画を照合する。追問では変換方向、ヘッダー有無、データ行数・列数、JSON側のPointer、同一のデコード済み文字列と原文・出力双方のバイト範囲とSHA-256を示す。見出し・データセルをそれぞれ先頭8件まで表示し、全範囲は保存レポートに残る。Graphで封印入力→再計算レポート→説明を結ぶ。新規 `ExplainPrevious` は `declarative-ja-en-v100-table-conversion-explanation`。旧v99の変換追問は当時の変換済み成果で再生する。

Linux x86_64 6.18.57、Rust/Cargo 1.99.1、製品 `0.7.0-alpha.11`。実CLIのSHA-256は旧 `0be09a22b2b79d35d4a77b0ffd06c8111399a42c0b03d1f0aa9868d4783079ae`、新 `0ea38adc9e6b07f8b1014a348d067b75ad0a1f59e5a0ed90f972cbed90232887`。`Cargo.lock` は `b4e97fecdca8fd1fc863349b1a48eb99a925da7fed0ed6ce6cfe79bdd1cececc`、実装と通常CLI検査3ファイルの `git diff --binary` は `bf64e92b0ace9656b65ce442e6f75ec5e7b305f6ce4ebfadfe094c6b066b8c10`。

- `cargo fmt --all -- --check`、`cargo check --workspace --all-targets --locked`、`cargo build -p minidora-cli --locked` は終了0。入力39件、文書11件、通常CLIの変換5件・説明18件（追加1件は共通）の計72件がPASS。追加1件では日英の入力と3通りの変換で、原文と出力の既知の行全体からセル範囲を独立に特定し、対応位置・ハッシュ・三つの依存・同一ID再生・実描画を照合した。数式として解釈され得る文字列のCSV出力停止も再検査した。
- 独立した実CLI検査では、変換両方向・ヘッダーありなしの着工点4ケースから、元と出力のセル位置・文字列・原バイトを照合した。引用されていない空欄と引用付き空文字は異なる元範囲を持ち、どちらもJSON側では2バイトの空文字列になる。見出しのみのCSV・JSONは0データ行として説明した。幅のある表では先頭8件ずつを表示し、保存レポート全20セルを確認した。両方向のレポート参照欠落、書込許可の対象不一致、証拠予算1バイト、別主体は新規ファイルと受領記録0件で停止した。v98のJSON件数説明を再生し、v99とv100ではJSON件数・CSV列選択の文面と根拠が一致した。
- 旧v99・新v100それぞれ4件の実ファイルを `ADOPTED/ATTEMPTED` から別プロセスで復旧した。全8件が `LOCAL_FILE_RECONCILED_NO_RESEND`。元の作用ID、ファイルのバイト、inode、更新時刻を保持し、再書き込み0件。同一IDによる再実行も各保存結果と一致した。

凍結版・原典台帳・着工点でblob `d771ddca5a907e7816b06936f461f136a7add146` が一致する `src/minidora/HDS運用/資料回答共有保存.py` 全501行を、保存の呼出元と再読込の実利用先に照合した。版付きの正規JSON・可逆圧縮から元データを再構成し、子回答の意味・条件・出典依存の検証に使う。読解台帳は138/416。凍結版の実行・通常入口・形式変換との同等性は未検証。93要件の受入は `UNVERIFIED`。全体のE6は継続中。全workspace実行試験、厳格Clippy再検査、E7体系QC、E8独立QA、公開・配布は `NOT_RUN`。

復旧2世代は `3dd5f8d` と `d1c9330`。終了時資産は計8件。保持3件（実装記録、要求トレース、読解台帳）、稼働中4件（2実装ファイル、通常CLI検査、共有Cargoキャッシュ）、保管済み・保留は各0件。検証用 `/workspace/.minidora-cloud/b153-legacy-review.tm9Mhk` 1件をLinuxのGIOゴミ箱へ移動し、元パスの不存在、収容物、復元情報を照合した。2,119ファイル、175,795,430論理バイト。指示者の最終処分待ち1件、容量回収0。Windows保管とオンライン専用化は非該当。

## 2026-11-20 JST B154 — D3-C E6 JSON Pointer抽出とJSON統計を元の値から説明

着工点は `463b8b6da54d981eef07411b96e03e3633f0e306`。D2仕様・工程とD3-C指示を全読し、採用SHA-256とR2 §14・§16を照合した。着工点の通常CLIでは、`JsonPointer` または `JsonStatistics` の直後に「詳しく説明して」と聞くと、前回の抽出値または統計レポートを同一バイトで返した。

既存Coreは封印済み原入力に記録されたPointerから値を再抽出する。抽出結果と元の採用内容計画・描画記録を照合し、Pointer、JSON型、元の字句、元バイト範囲とSHA-256を説明する。統計では元配列から再計算したレポートをケース内の保存レポート・先行結果・実描画に照合し、件数・合計・平均・中央値・範囲・分散と各数値の元の表記および厳密値を示す。個別に表示する数値は先頭8件で、保存レポートは全件を保持する。長い抽出値の表示は先頭128文字を使い、ハッシュと範囲は値全体に対応する。Graphは封印入力から元の描画または再計算レポートを経て新しい説明に接続する。

新規 `ExplainPrevious` は `declarative-ja-en-v101-json-value-explanation`。v100の旧説明は当時の抽出値・統計レポートで再生する。Linux x86_64 6.18.57、Rust/Cargo 1.99.1、製品 `0.7.0-alpha.11`。実CLI SHA-256は旧 `0ea38adc9e6b07f8b1014a348d067b75ad0a1f59e5a0ed90f972cbed90232887`、新 `e7703c99ad322dcd94b69a3abf20f90d216555f7dd3a1f3554eb7c4f5b828ae2`。`Cargo.lock` は `b4e97fecdca8fd1fc863349b1a48eb99a925da7fed0ed6ce6cfe79bdd1cececc`、実装と通常CLI検査6ファイルの `git diff --binary` は `dc500a76db0ad90205f92ce902e5e76c1092c19460d2c7f91fc4e5222ce80292`。

- `cargo fmt --all -- --check`、`cargo check --workspace --all-targets --locked`、`cargo build -p minidora-cli --locked` は終了0。入力39件、文書11件、JSON解釈1件、通常CLIの説明20件・基礎文書操作7件の計78件がPASS。追加2件では `1e400`、エスケープ付き文字列、ルートの原文と保持した描画、入れ子とルート配列の各数値の元位置、合計 `7/2` と平均 `7/8`、Graph・三つの依存・同一ID再生を照合した。
- 独立した実CLIで全6種のJSON型とルート、`~0`・`~1` を含むPointer、複数箇所に同一の字句がある場合の指定位置を照合した。統計では入れ子の2番目の同一配列を選び、平均 `1/3`、母分散 `19/450`、標本分散 `19/300`、1件時の標本分散の扱いを検証した。10数値の保存レポートには全件があり、説明は先頭8件を表示した。抽出の描画記録欠落、統計レポート欠落、書込許可の対象不一致、証拠予算1バイト、別主体は新規ファイルと受領記録0件で停止した。v100の形式変換・JSON件数は、同一ID再生とv101新規実行で文面と根拠を照合した。
- 旧v100・新v101各5件の実ファイルを `ADOPTED/ATTEMPTED` から別プロセスで復旧した。全10件が `LOCAL_FILE_RECONCILED_NO_RESEND`。元の作用ID、バイト、inode、更新時刻を保持し、再書き込み0件。同一IDによる再実行も対応する保存済み結果と一致した。

凍結版・原典台帳・着工点でblob `a5b33701229eb7c1d9e7d7e26f258274763d25f0` が一致する `src/minidora/汎用会話.py` 全397行を、製品チャット入口、資料の読取と数量集合の実利用先に照合した。原典読解は139/416件。凍結版の実行とRustとの同等性は未検証。93要件の受入は `UNVERIFIED`、E6は継続中。全workspace実行試験、厳格Clippy再検査、E7体系QC、E8独立QA、公開・配布は `NOT_RUN`。

復旧2世代は `463b8b6` と `3dd5f8d`。終了時資産は計11件。保持3件（実装記録、要求トレース、読解台帳）、稼働中7件（5実装ファイル、通常CLI検査、共有Cargoキャッシュ）、保管済み・保留は各0件。検証用 `/workspace/.minidora-cloud/b154-legacy-review.fhJAJr` 1件をLinuxのGIOゴミ箱へ移動し、元パスの不存在、収容物、復元情報を照合した。2,396ファイル、177,211,694論理バイト。最終処分待ち1件、容量回収0。Windows保管とオンライン専用化は非該当。

## 2026-11-20 JST B155 — D3-C E6 JSONオブジェクトの項目選択を元のキーと値から説明

着工点は `8e1ff99c91dcec3079d764eb33c0ca6e9b5a01b4`。D2仕様・工程とD3-C指示を全読し、採用SHA-256とR2 §14・§16を照合した。着工点の通常CLIでは、`JsonObjectProject` の直後に「詳しく説明して」と聞くと、選択済みJSONを同一バイトで返した。

既存Coreは同じ主体・ケースの封印入力から項目選択を再計算し、ケース内の保存レポート、先行結果、描画記録と照合する。指定された項目順でJSON Pointerを示し、キーと値それぞれの元・出力バイト範囲、SHA-256、原文・出力表記を対応付ける。キーはエスケープ表記が変わっても復号後の名前を照合し、値は字句のバイト一致を照合する。本文は先頭8項目、各プレビューは先頭128文字で、保存レポートには全項目を保持する。Graphは封印入力から再計算レポートを経て説明につなぎ、先行結果・封印入力・レポートを説明の依存として記録する。

新規 `ExplainPrevious` は `declarative-ja-en-v102-json-object-project-explanation`。保存済みv101の項目選択説明は当時の文面で再生する。Linux x86_64 6.18.57、Rust/Cargo 1.99.1、製品 `0.7.0-alpha.11`。実CLI SHA-256は旧 `e7703c99ad322dcd94b69a3abf20f90d216555f7dd3a1f3554eb7c4f5b828ae2`、新 `ae154eaf5459782e2a48e08dd286aa872945e38b2806d2be149eb113e187c207`。`Cargo.lock` は `b4e97fecdca8fd1fc863349b1a48eb99a925da7fed0ed6ce6cfe79bdd1cececc`、実装と通常CLI検査3ファイルの `git diff --binary` は `4ab3bf4e1a8d726987a9ed01abbb1f82f381271ea5e5e236213c4aaea555d154`。

- `cargo fmt --all -- --check`、`cargo check --workspace --all-targets --locked`、`cargo build -p minidora-cli --locked` は終了0。入力39件、文書11件、通常CLIの説明21件・列と項目の選択5件の計76件がPASS。追加の通常CLI回帰検査は日英の入力で、元と逆の項目順、エスケープされたキーと出力の表記、JSON Pointerの `~0`・`~1`、整数 `9007199254740993` と入れ子の原文、保存レポート、Graph、三つの依存、同一ID再生を照合した。
- 独立した実CLI検査では、着工点の旧版で出力した2件を新版で説明した。空のキー、改行・引用符・スラッシュ・チルダを含むキー、`1.2300e+40`、前方に同じ値があるときの実際の選択位置を照合した。10項目の保存レポートに全項目があり、説明は8項目を表示した。レポート参照欠落、書込許可の対象不一致、証拠予算1バイト、別主体は新規ファイル・受領記録ともに0件で停止した。v101のJSON Pointer・JSON統計は、同一ID再生とv102新規実行の文面と根拠が一致した。
- 旧v101・新v102各2件の実ファイルを `ADOPTED/ATTEMPTED` から別プロセスで復旧した。全4件が `LOCAL_FILE_RECONCILED_NO_RESEND`。元の作用ID、バイト、inode、更新時刻を保持し、再書き込み0件。同一IDの再実行も対応する保存結果と一致した。

凍結版・原典台帳・着工点でblob `b3c43385d134d85526358bfc28678ebb0370395a` が一致する `src/minidora/能力意味カタログ.py` 全134行を、目的計画・HDS目的射影と構造化文書の実利用先に照合した。宣言された位置・型を操作設定に束縛し、登録された具体的な読取処理へつなぐ。原典読解は140/416件。凍結版の実行とRustとの同等性は未検証。93要件の受入は `UNVERIFIED`、E6は継続中。全workspace実行試験、厳格Clippy再検査、E7体系QC、E8独立QA、公開・配布は `NOT_RUN`。

復旧2世代は `8e1ff99` と `463b8b6`。終了時資産は計8件。保持3件（実装記録、要求トレース、読解台帳）、稼働中4件（2実装ファイル、通常CLI検査、共有Cargoキャッシュ）、保管済み・保留は各0件。検証用 `/workspace/.minidora-cloud/b155-legacy-review.LetbBc` 1件をLinuxのGIOゴミ箱へ移動し、元パスの不存在、収容物、復元情報を照合した。1,127ファイル、166,759,924論理バイト。最終処分待ち1件、容量回収0。Windows保管とオンライン専用化は非該当。

## 2026-11-20 JST B156 — D3-C E6 JSON構造比較を左右の原文から説明

着工点は `509c0e6203efc1eb23922c36fa769be323af615c`。D2仕様・工程とD3-C指示を全読し、採用SHA-256とR2 §14・§16・§20を照合した。着工点の通常CLIでは、`JsonCompare` の直後に「詳しく説明して」と聞くと、前回の比較レポートを同一バイトで返した。

既存Coreは同じ主体・ケースの封印入力から構造比較を再計算し、正規化したレポートをケース内の保存レポート、先行結果、描画記録と照合する。比較の一致・不一致、件数、左右の値のSHA-256を示す。キー順を無視するオブジェクト、位置で比較する配列、復号した文字列、厳密な十進数値の各比較規則を説明する。差分は左のみ・右のみ・変更を区別し、各対象を起点とするJSON Pointerと、元のラッパー全文を基準にした左右のバイト範囲・SHA-256・原文を示す。未存在は `null` と区別する。本文は先頭8件、原文・Pointerは先頭128文字で、保存レポートには全件のPointerと範囲を保持する。Graphは封印入力から再計算レポートを経て説明に接続し、先行結果・封印入力・レポートを説明の依存として記録する。

新規 `ExplainPrevious` は `declarative-ja-en-v103-json-structural-comparison-explanation`。保存済みv102の構造比較説明は当時のレポート表示で再生する。Linux x86_64 6.18.57、Rust/Cargo 1.99.1、製品 `0.7.0-alpha.11`。実CLI SHA-256は旧 `ae154eaf5459782e2a48e08dd286aa872945e38b2806d2be149eb113e187c207`、新 `3f02d601085a64f7afd32481cc0a000c3a7693a5b40a1df5eeedf68ada27dffb`。`Cargo.lock` は `b4e97fecdca8fd1fc863349b1a48eb99a925da7fed0ed6ce6cfe79bdd1cececc`、実装と通常CLI検査3ファイルの `git diff --binary` は `f6014a693f111d92ccff763f6a65e181b89089ac2e9d261c67b91da7d928deb9`。

- `cargo fmt --all -- --check`、`cargo check --workspace --all-targets --locked`、`cargo build -p minidora-cli --locked` は終了0。入力39件、文書11件、通常CLIの説明22件・構造比較5件の計77件がPASS。追加の通常CLI回帰検査は日英の比較依頼で、入れ子の変更・削除・追加の順序、左右別の原文と範囲、キー順・復号前の表記・厳密な数値表記が異なる場合の一致、Graph、三つの依存、同一ID再生を照合した。表示順と件数の検査を強化した最終版でも当該1件を再実行しPASS。
- 独立した実CLI検査では、旧版で作成した比較結果3件を新版で説明した。入れ子と空のキーを含む7差分、表記が異なる一致、配列の順序を入れ替えた2差分を照合した。さらに右の値が先に記された入力で、ルートのPointer `""` と左右の原文 `null`／`{"x":1e400}` を確認した。10差分の保存レポートに全件があり、説明は8件を表示した。レポート参照欠落、書込許可の対象不一致、証拠予算1バイト、別主体は新規ファイル・受領記録ともに0件で停止した。v102のJSONオブジェクト項目選択も後続説明を照合した。
- 旧v102・新v103各3件の実ファイルを `ADOPTED/ATTEMPTED` から別プロセスで復旧した。全6件が `LOCAL_FILE_RECONCILED_NO_RESEND`。元の作用ID、バイト、inode、更新時刻を保持し、再書き込み0件。同一IDの再実行も対応する保存結果と一致した。

凍結版・原典台帳・着工点でblob `58197a1d62f6f9ec79b6245373a24a615708bb31` が一致する `src/minidora/駆動系/構造.py` 全148行を、関係駆動の構造要求、変換、射影、採否経路に照合した。型・単位・固定項を保つ関係束の比較で、探索未完了と探索完了後の不一致を分ける。原典読解は141/416件。凍結版の実行、通常入口、RustのJSON構造比較との同等性は未検証。93要件の受入は `UNVERIFIED`、E6は継続中。全workspace実行試験、厳格Clippy再検査、E7体系QC、E8独立QA、公開・配布は `NOT_RUN`。

復旧2世代は `509c0e6` と `8e1ff99`。終了時資産は計8件。保持3件（実装記録、要求トレース、読解台帳）、稼働中4件（2実装ファイル、通常CLI検査、共有Cargoキャッシュ）、保管済み・保留は各0件。検証用 `/workspace/.minidora-cloud/b156-legacy-review.R4z59L` 1件をLinuxのGIOゴミ箱へ移動し、元パスの不存在、収容物、復元情報を照合した。1,503ファイル、179,230,339論理バイト。最終処分待ち1件、容量回収0。Windows保管とオンライン専用化は非該当。

## 2026-11-20 JST B157 — D3-C E6 CSVの列番号選択と元セルの説明を通常入力へ接続

着工点は `ea8262b1607dd2250bf2cf88184d18e55f1df8ad`。D2仕様・工程とD3-C指示を全読し、採用SHA-256とR2 §14・§16・§20を照合した。着工点では `csv_select_index` が明示的な見出し設定を処理する一方、日英の列番号指定は通常CLIで `CAPABILITY_UNAVAILABLE` になった。

通常入力の `見出しなしCSVの第2列を選択して:`、`見出しありCSVの第2列を選択して:`、`csv-select-index no-header 2:`、`csv-select-index header 2:` を封印し、既存Coreから既存の `csv_select_index` につないだ。依頼の列番号は1始まりで、保存するインデックスは0始まり。見出しの扱いは利用者の明示指定を使い、見出しなしでは先頭レコードもデータとする。セルは文字列として選択し、保存レポートに元と出力のバイト範囲を保持する。列の範囲、CSVの形状、出力予算は作用前に検査する。

「詳しく説明して」は同じ主体・ケースの封印入力から再計算し、先行結果、保存レポート、描画記録と照合する。選択列番号、見出しの扱い、データ行順のセル原文・バイト範囲・SHA-256を示す。表示は先頭8件で、保存レポートには全セルの範囲を保持する。見出し付きの本文で示す行番号はデータ行の順序。Graphは封印入力から選択レポートを経て説明に接続し、先行結果・封印入力・レポートを依存に記録する。READMEに通常入力例を追加した。

新規 `CsvSelectIndex` と `ExplainPrevious` は `declarative-ja-en-v104-csv-positional-selection`。既存の見出し名指定は名前の完全一致で、数字の見出し名も名前として扱う。Linux x86_64 6.18.57、Rust/Cargo 1.99.1、製品 `0.7.0-alpha.11`。実CLI SHA-256は旧 `3f02d601085a64f7afd32481cc0a000c3a7693a5b40a1df5eeedf68ada27dffb`、新 `6961078e4eaeeea3be60939fd64d932ee05a8e966ea82feb3713ed82b5d89e91`。`Cargo.lock` は `b4e97fecdca8fd1fc863349b1a48eb99a925da7fed0ed6ce6cfe79bdd1cececc`、実装・通常CLI検査・READMEの6ファイルの `git diff --binary` は `5556de43b697dbe3dc8b222361437514afc64ab04f11cb1865f5915ff9ce6d79`。

- `cargo fmt --all -- --check`、`cargo check --workspace --all-targets --locked`、`cargo build -p minidora-cli --locked` は終了0。入力39件、構造化入力7件、文書11件、通常CLIの説明23件・列番号選択2件の計81件がPASS（重複1件を除く）。追加の正例は日英で同じCRLF文書を使い、引用符・セル内改行・引用付き空文字・未引用空欄の範囲と出力、三つの依存、Graph、同一ID再生を検証した。列番号0、見出しの扱い未指定、範囲外、送信指示の追記は停止した。
- 独立した実CLIでは、日英と両見出し設定、第1列と最終列、見出しのみのCSVを確認した。10値のレポートに全件があり、説明は8件を表示した。見出しの重複は見出し付きで停止し、見出しなしでは先頭行のデータとして選択した。番号のオーバーフロー、日英の見出し設定不足、レポート参照欠落、書込許可の対象不一致、証拠予算1バイト、別主体を確認し、異常な要求からの新規ファイル・受領記録は0件。v103の名前指定の説明とJSON構造比較の説明を照合した。着工点で停止した4入力は同一IDで当時の停止を再生し、新規IDでは列を選択した。
- 旧v103の名前指定説明1件、新v104の列番号指定の説明7件、直接選択結果1件を `ADOPTED/ATTEMPTED` から別プロセスで復旧した。全9件が `LOCAL_FILE_RECONCILED_NO_RESEND`。作用ID・バイト・inode・更新時刻は一致し、再書き込みは0件。同一IDの再実行も対応する保存結果と一致した。

凍結版・原典台帳・着工点でblob `ae86969b28b26bf5e3974de0d3e9ab00c0ba02f4` が一致する `src/minidora/HDS目的射影.py` 全253行を、目的会話・解釈・能力カタログ・構造化文書の消費先に照合した。凍結版の目的射影は名前を指定したCSVを文書読取と変換へ渡し、見出しなしの位置指定は下位文書APIに存在する。原典読解は142/416件。凍結版の実行およびRustの通常入口との同等性は未検証。93要件の受入は `UNVERIFIED`、E6は継続中。全workspace実行試験、厳格Clippy再検査、E7体系QC、E8独立QA、公開・配布は `NOT_RUN`。

復旧2世代は `ea8262b` と `509c0e6`。終了時資産は計11件。保持4件（README、実装記録、要求トレース、読解台帳）、稼働中6件（実装4ファイル、通常CLI検査、共有Cargoキャッシュ）、保管済み・保留は各0件。検証用 `/workspace/.minidora-cloud/b157-legacy-review.VCKaNT` 1件をLinuxのGIOゴミ箱へ移動し、元パスの不存在、収容物、復元情報を照合した。1,749ファイル、170,953,592論理バイト。最終処分待ち1件、容量回収0。Windows保管とオンライン専用化は非該当。

## 2026-11-20 JST B158 — D3-C E6 置換箇所の特定を対象本文と依頼JSONから説明

着工点は `14b98e355391061efde3c544555b33f934809d41`。D2仕様・工程とD3-C指示を全読し、採用SHA-256とR2 §14・§16・§20を照合した。着工点の通常CLIでは `LocateTextReplacement` に続く「詳しく説明して」が、前回の位置特定レポートを同一バイトで返していた。

既存Coreは同じ主体・ケースの封印済み依頼と保存した対象本文・祖先を用い、検索語が保護範囲の外に一箇所だけ完全一致することを再計算する。正規形式で保存したレポートと先行結果の文面・依存順序・描画を照合する。説明は対象本文内のUTF-8バイト範囲と、依頼JSON内の検索語・置換語それぞれの生のトークン範囲、SHA-256、原文を区別する。表示する原文は各先頭128文字。説明は置換案と別途の明示適用を案内し、実際の適用条件は編集依頼時に検査する。Graphは封印入力から位置特定レポートを経て説明に接続し、先行結果・封印入力・レポートを依存として保持する。

新規 `ExplainPrevious` は `declarative-ja-en-v105-text-location-explanation`。旧v104の位置特定説明は当時のレポート表示で再生する。Linux x86_64 6.18.57、Rust/Cargo 1.99.1、製品 `0.7.0-alpha.11`。実CLI SHA-256は旧 `6961078e4eaeeea3be60939fd64d932ee05a8e966ea82feb3713ed82b5d89e91`、新 `face92d797ed344fb04466f19f7d840a4260f7504a89aa519834ff59ef3b4e48`。`Cargo.lock` は `b4e97fecdca8fd1fc863349b1a48eb99a925da7fed0ed6ce6cfe79bdd1cececc`、実装・通常CLI検査5ファイルの `git diff --binary` は `bdca367c326c0efd930403f238f57e5fece648da2483335e89d062de3de0c542`。

- `cargo fmt --all -- --check`、`cargo check --workspace --all-targets --locked`、`cargo build -p minidora-cli --locked` は終了0。構造化入力7件、文書11件、テキスト編集8件、通常CLIの位置特定3件・テキスト編集3件・説明24件の計55件がPASS（重複1件を除く）。追加の通常CLI検査では、対象本文の非ASCII文字と依頼JSONのUnicodeエスケープ、対象本文・JSONトークンそれぞれの範囲とハッシュ、三つの依存、Graph、同一ID再生を照合した。説明後の元本文と作用件数を確認し、別の明示的編集依頼で置換案を適用した。
- 独立した実CLIでは、旧版で保存した日英の位置特定結果を新版で説明した。CRLFと複数バイトを含む本文、理由欄に同じ字句が先行するときの検索語、空の置換語を照合した。同じレポートを本文として取り込んだケースは通常の本文として扱われた。対象本文と物理ファイルのバイト・inode・更新時刻を照合し、別の明示編集依頼で両方の置換案を適用した。対象本文・レポートの参照欠落、レポートの実バイト不一致、書込許可対象の不一致、証拠予算1バイト、別主体の6件では新規ファイル・受領記録・作用が0件。v104のCSV列番号説明は同一ID再生と新規v105の文面・根拠が一致した。
- 旧v104・新v105の説明ファイル各2件を `ADOPTED/ATTEMPTED` から別プロセスで復旧した。全4件が `LOCAL_FILE_RECONCILED_NO_RESEND`。作用ID、バイト、inode、更新時刻は一致し、再書き込みは0件。同一IDによる再実行も保存結果と一致した。

凍結版・原典台帳・着工点でblob `9e719b10d0d6cee345996adca109c66c3a3e5d84` が一致する `src/minidora/入力系/変換.py` 全80行を、入力統合、射影、Coreの実利用先に照合した。凍結版は文字インデックスの順序・範囲を診断し、診断を封印入力に保持する。原典読解は143/416件。凍結版の実行、今回の編集提案・UTF-8バイト範囲との同等性は未検証。93要件の受入は `UNVERIFIED`、E6は継続中。全workspace実行試験、厳格Clippy再検査、E7体系QC、E8独立QA、公開・配布は `NOT_RUN`。

復旧2世代は `14b98e3` と `ea8262b`。終了時資産は計10件。保持3件（実装記録、要求トレース、読解台帳）、稼働中6件（実装4ファイル、通常CLI検査、共有Cargoキャッシュ）、保管済み・保留は各0件。検証用 `/workspace/.minidora-cloud/b158-legacy-review.09UA0X` 1件をLinuxのGIOゴミ箱へ移動し、元パスの不存在、収容物、復元情報を照合した。1,990ファイル、176,180,844論理バイト。最終処分待ち1件、容量回収0。Windows保管とオンライン専用化は非該当。

## 2026-11-20 JST B159 — D3-C E6 適用済みの文章編集を前後のバイト範囲から説明

着工点は `01a66a132e3c74368b5bbe89d8b02cf92a3f448a`。D2仕様・工程とD3-C指示を全読し、採用SHA-256とR2 §14・§16・§20を照合した。着工点の通常CLIでは、`EditText` に続く「詳しく説明して」が編集後本文を同一バイトで返した。

既存Coreが同じ主体・ケースの封印済み依頼、編集前本文と文書の祖先を使って編集を再計算し、保存レポートの正規形式、依存順、先行結果の編集後本文、描画を照合する。説明では編集版、変更数、保護範囲数と、元本文の位置順の挿入・削除・置換を示す。各変更は編集前後の半開UTF-8バイト範囲、SHA-256、該当文字列を区別する。挿入の編集前、削除の編集後は長さ0の範囲。本文の表示は先頭8変更、各プレビューは先頭128文字で、レポートは全変更を保持する。Graphは封印入力から編集レポートを経て説明へ接続し、先行結果・封印入力・レポートを依存として記録する。

新規 `ExplainPrevious` は `declarative-ja-en-v106-text-edit-explanation`。保存済みv105の編集説明は当時の編集後本文で再生する。置換箇所だけを特定した場合の詳説はv105の文面を維持する。Linux x86_64 6.18.57、Rust/Cargo 1.99.1、製品 `0.7.0-alpha.11`。実CLI SHA-256は旧 `face92d797ed344fb04466f19f7d840a4260f7504a89aa519834ff59ef3b4e48`、新 `92bba904545a352936f34a0e0b8f7ce3c4c93872fb47b5df596cabcdb96d7c79`。`Cargo.lock` は `b4e97fecdca8fd1fc863349b1a48eb99a925da7fed0ed6ce6cfe79bdd1cececc`。

実装・通常CLI検査3ファイルの `git diff --binary` SHA-256は `a20989eb5ae24b0837c134bc1e147cc040767511aa3e17fbbabe555a5dd2baf0`。

- `cargo fmt --all -- --check`、`cargo check --workspace --all-targets --locked`、`cargo build -p minidora-cli --locked` は終了0。構造化入力7件、文書11件、テキスト編集8件、通常CLIの編集4件・位置特定3件・説明25件の計56件がPASS（重複2件を除く）。追加の通常CLI検査では、依頼順と異なる元本文の位置順、置換・挿入・削除の各範囲とハッシュ、保護した文字列が同じ内容のまま移動すること、レポート、三つの説明依存、Graph、同一ID再生を照合した。ハッシュの検査を強化した最終版でも当該1件を再実行した。
- 独立した実CLIでは、取り込んだ原文からの第2版と、構成済み文章からの第1版を説明した。第2版の置換・挿入・削除と、保護した範囲を実バイトで照合した。10変更の保存レポートに全件があり、説明は8件を表示した。レポート・親・起点・描画・封印入力の参照欠落、対象外の書込権限、他主体、1バイトの証拠予算、レポート実バイトの不一致の9件で、新規ファイル・作用が0件。同じ形のJSONを本文として取り込んだ場合は通常本文として扱った。v105の置換箇所説明は保存済み再生と新規v106の文面・根拠が一致した。
- 旧版で作った編集本文2件、v105説明ファイル2件、新v106説明ファイル2件を `ADOPTED/ATTEMPTED` から別プロセスで復旧した。全6件が `LOCAL_FILE_RECONCILED_NO_RESEND`。作用ID、バイト、inode、更新時刻は一致し、再書き込みは0件。同一IDの再実行も保存結果と一致した。独立検査の初回は直接編集に説明用のv105を期待していたため、直接編集の保存済みv89封印を照合するよう検査を直し、同条件で両方の直接編集を再検証した。

凍結版・原典台帳・着工点でblob `2f2de9e983c419bbe78b8c2420588a095555d23d` が一致する `src/minidora/HDS運用/内容構成.py` 全176行を、一般接続と工程の実利用先に照合した。凍結版は原資料の指定部分を保護し、保存済み回答を元資料・構造から再生成して照合する。原典読解は144/416件。凍結版の実行、Rustの編集適用・承認・UTF-8位置との同等性は未検証。93要件の受入は `UNVERIFIED`、E6は継続中。全workspace実行試験、厳格Clippy再検査、E7体系QC、E8独立QA、公開・配布は `NOT_RUN`。

復旧2世代は `01a66a1` と `14b98e3`。終了時資産は計8件。保持3件（実装記録、要求トレース、読解台帳）、稼働中4件（実装2ファイル、通常CLI検査、共有Cargoキャッシュ）、保管済み・保留は各0件。検証用 `/workspace/.minidora-cloud/b159-legacy-review.flCcrp` 1件をLinuxのGIOゴミ箱へ移動し、元パスの不存在、収容物、復元情報を照合した。3,097ファイル、175,240,562論理バイト。最終処分待ち1件、容量回収0。Windows保管とオンライン専用化は非該当。

## 2026-11-20 JST B160 — D3-C E6 構成した文章を素材と書式の出典から説明

着工点は `975650e6ce83be531e2016ebf95c63fb63ca8544`。D2仕様・工程とD3-C指示を全読し、採用SHA-256とR2 §14・§16・§20を照合した。着工点の通常CLIでは `ComposeText` に続く「詳しく説明して」が構成後本文を同一バイトで返した。

既存Coreが同じ主体・ケースの封印済み依頼から文章を再構成し、保存レポート、先行結果の本文、描画を照合する。採用単位の順序と実効の必須・保護フラグ、未採用単位を示す。出力区間は、素材から取り出した部分と書式として付いた部分に分けて説明する。前者は復号後の素材内の範囲と依頼JSONにある文字列トークン全体の範囲、後者は書式を決めた単位のJSON範囲に対応する。出力と依頼JSONの範囲はそれぞれSHA-256を示す。単位と区間の各表示は先頭8件、出力断片は先頭128文字で、保存レポートには全件を保持する。Graphは封印入力から構成レポートを経て説明へ接続し、先行結果・封印入力・レポートを依存として記録する。

旧版では証拠予算128バイトで、完成文3バイトと未使用の素材を含む構成レポート4,393バイトが確定した。通常の文章構成は、保存前に正規形式のレポート全体を予算と照合するよう修正した。新規 `ExplainPrevious` は `declarative-ja-en-v107-text-composition-explanation`。保存済みv106の構成説明は当時の本文表示で再生する。Linux x86_64 6.18.57、Rust/Cargo 1.99.1、製品 `0.7.0-alpha.11`。実CLI SHA-256は旧 `92bba904545a352936f34a0e0b8f7ce3c4c93872fb47b5df596cabcdb96d7c79`、新 `63a3ef2dfe8ea70b3d864b01165d0209d41703e03647c9d1b5aeaac1abeb1d7f`。`Cargo.lock` は `b4e97fecdca8fd1fc863349b1a48eb99a925da7fed0ed6ce6cfe79bdd1cececc`。

実装・通常CLI検査4ファイルの `git diff --binary` SHA-256は `1914dea826cc390713e899f50ce8f720ac54fd0dff05ce8b1a3e32c5791adafc`。

- `cargo fmt --all -- --check`、`cargo check --workspace --all-targets --locked`、`cargo build -p minidora-cli --locked` は終了0。構造化入力7件、文書11件、テキスト編集8件、通常CLIの構成4件・説明26件の計55件がPASS（重複1件を除く）。追加検査では、Unicodeエスケープ付き素材、書式の出所、区間、未採用単位、条件単位の実効フラグ、三つの依存、Graph、同一ID再生を照合した。生成本文3バイトの予算超過ケースは、依存・受領記録・作用が0件で停止した。
- 独立した実CLIで、日英の保存済み構成結果を新版で説明した。CRLFを含む複数行引用、同じ字句が先にある場合の正しい素材トークン、条件と段落間の空行の書式根拠、10区間の保存と8区間の表示を照合した。構成レポート・描画・内容計画・封印入力の欠落、別対象の書込許可、別主体、予算不足、レポート実バイトの破損の8件は、新規出力・作用を行わず停止した。レポートと同じJSONを普通の素材として取り込んだ結果は通常の本文として扱い、v106の編集説明は保存済み再生と新規v107の文面・根拠が一致した。
- 予算違反を再現した入力の同一IDは、旧版の保存結果で再生した。同じ入力を新規IDで実行すると、証拠予算128バイト・4,392バイトでは構成レポート、依存、内容計画、作用、ファイルが作成されず停止し、4,393バイトでは同じレポートと許可済みの3バイトのファイルを確認した。
- 旧版の構成本文2件、旧v106の説明2件、新v107の説明2件を `ADOPTED/ATTEMPTED` から別プロセスで復旧した。全6件が `LOCAL_FILE_RECONCILED_NO_RESEND`。作用ID、バイト、inode、更新時刻は一致し、再書き込みは0件。同一IDの再実行も保存結果と一致した。

凍結版・原典台帳・着工点でblob `e7feefdf927341bf20570f77dfd00779ef4517a7` が一致する `src/minidora/HDS運用/会話回答束.py` 全389行を、一般依頼・一般接続・工程・セッションの実利用先に照合した。凍結版は各子回答の全文を必須・保護の段落として構成し、本文と保護位置を再構成して照合する。原典読解は145/416件。凍結版の実行とRustの構成・保護範囲との同等性は未検証。93要件の受入は `UNVERIFIED`、E6は継続中。全workspace実行試験、厳格Clippy再検査、E7体系QC、E8独立QA、公開・配布は `NOT_RUN`。

復旧2世代は `975650e` と `01a66a1`。終了時資産は計9件。保持3件（実装記録、要求トレース、読解台帳）、稼働中5件（実装3ファイル、通常CLI検査、共有Cargoキャッシュ）、保管済み・保留は各0件。検証用 `/workspace/.minidora-cloud/b160-legacy-review.sUFhFr` 1件をLinuxのGIOゴミ箱へ移動し、元パスの不存在、収容物、復元情報を照合した。1,583ファイル、172,830,042論理バイト。最終処分待ち1件、容量回収0。Windows保管とオンライン専用化は非該当。

## 2026-11-20 JST B161 — D3-C E6 抽出要約候補の選択と原文位置を追問で説明

着工点は `81887fc7bbc0e8c94645a5ac4fbfe3e62eb80dc7`。D2仕様・工程とD3-C指示を全読し、採用SHA-256とR2 §14・§16・§20を照合した。着工点の通常CLIでは、日英の抽出要約依頼後の「詳しく説明して」が候補本文を同一バイトで返した。

既存Coreは、同じ主体・ケースの封印済み抽出依頼を再計算し、選択レポート、先行結果本文、保存した内容計画と実描画を照合する。通常の追問では、候補・選択・未選択・文字数上限で見送った文の件数、出力書式、必須指定と実際の選択順を示す。各選択文について、復号後の元本文と候補出力のUTF-8バイト範囲、文のSHA-256、先頭128文字までの抜粋を示す。依頼JSONの本文文字列トークン全体には別の範囲とSHA-256を示す。出力は元本文の順を保持する。既存Graphを通じて、先行結果・封印入力・レポートを依存に記録する。保証区分は `HYPOTHESIS` を保持し、目的・意味・条件・否定の網羅と事実性は未検証と表示する。

新規の候補説明は `declarative-ja-en-v108-text-summary-candidate-explanation`。旧v107の完了済みの候補本文表示と直接要約は、当時と同じ結果を同一IDで返す。既存の関係証明テストで、旧通常CLIが導出結果の追問にも文書処理のv107を記録し、証明のv89という期待値に反することを発見した。導出結果の追問をv89へ戻し、元の関係証明テストを変更せずに同条件で再検査した。

Linux x86_64 6.18.57、Rust/Cargo 1.99.1、製品 `0.7.0-alpha.11`。実CLI SHA-256は旧 `63a3ef2dfe8ea70b3d864b01165d0209d41703e03647c9d1b5aeaac1abeb1d7f`、最終 `9f859d00438040fb040d017a12d493c6a962bb3677a3c987c32bad51c6f38a3c`。`Cargo.lock` は `b4e97fecdca8fd1fc863349b1a48eb99a925da7fed0ed6ce6cfe79bdd1cececc`。実装・通常CLI検査5ファイルの `git diff --binary` SHA-256は `85b4e0c54277fc25af1c35d2113da351a7183eb9d0f27057698e0193018d3623`。

- `cargo fmt --all -- --check`、`cargo check --workspace --all-targets --locked`、`cargo build -p minidora-cli --locked` は終了0。Core側は抽出・出典13件、文書11件、Input39件、復旧65件。通常CLIは直接候補4件、説明27件、関係証明の追問と先行結果要約の再利用が各1件。計161件がPASS。新回帰では日英ヘッダー、CRLFとUnicodeエスケープ、同点の採用順位、未選択、元本文・依頼JSON・候補出力の別範囲、Graphと同一追問ID再生を照合した。
- 独立した実CLIで、旧版に保存した日英2件を新版で説明した。箇条書きと段落、同じ字句が複数回現れる本文、出力順より先に採用された必須文、長さで見送った文の件数を照合した。内容が候補警告と同じ通常取り込み文は従来の本文表示となり、旧v107の文章構成説明も再生できた。レポート・描画・内容計画・封印入力の欠落、レポート破損、別対象の書込許可、別主体、予算不足の8件は新規作用・出力を行わず停止した。
- 新v108の追問2件と、旧v107・新v89の関係証明追問各1件を `ADOPTED/ATTEMPTED` から別プロセスで復旧した。全4件が `LOCAL_FILE_RECONCILED_NO_RESEND`。作用ID、実バイト、inode、更新時刻は一致し、再書き込みは0件。候補の保存レポートを失った新v108の中断復旧は受領記録・書き込みなしで停止した。旧候補追問2件と直接要約2件の中断復旧は従来どおり `RECOVERY_CONTENT_MISMATCH` で停止した。この4件の復旧接続は残件。
- 関係証明の実CLIは旧v107から新v89へ戻り、導出保証、実出力832バイト、先行結論・前提・規則への依存が一致した。先行結果なし、別主体、書込許可の対象違いは旧新とも新規出力なしで停止した。

凍結版・原典台帳・着工点でblob `5649024426d1911376b02e49e873c5166d3f3ded` が一致する `src/minidora/能力合成_局所接続.py` 全61行を、要求解釈、文脈要求、能力合成、製品版要約の実利用先に照合した。凍結版の接続は合成した素材本文と参照、別保存した行数設定を要約器へ渡す。原典読解は146/416件。凍結版の実行とRustの原文位置・選択根拠との同等性は未検証。93要件の受入は `UNVERIFIED`、E6は継続中。全workspace実行試験、厳格Clippy再検査、E7体系QC、E8独立QA、公開・配布は `NOT_RUN`。

復旧2世代は `81887fc` と `975650e`。終了時資産は計11件。保持3件（実装記録、要求トレース、読解台帳）、稼働中6件（実装4ファイル、通常CLI検査、共有Cargoキャッシュ）、保管済み・保留は各0件。検証用 `/workspace/.minidora-cloud/b161-legacy-review.jXPHfb` と `/workspace/.minidora-cloud/b161-derived-review.O6pDUk` をLinuxのGIOゴミ箱へ移動した。元パスの不存在、収容物、復元情報を照合した。計4,452ファイル、188,512,288論理バイト、最終処分待ち2件。容量回収0。Windows保管とオンライン専用化は非該当。

## 2026-11-20 JST B162 — D3-C E6 直接要約候補と旧版の要約追問を中断から復旧

着工点は `33430fbe049858d9fc444e4ce764897f14a0d065`。D2仕様・工程とD3-C指示を全読し、採用SHA-256とR2 §14・§16・§20を照合した。正本CLIでは、直接候補と旧v107追問の真正な `ADOPTED/ATTEMPTED` 各2件が、実ファイルを持つ状態でも `RECOVERY_CONTENT_MISMATCH` で停止した。B161のv108追問は同条件で復旧した。

直接候補の復旧は、登録済み封印入力と通常ヘッダー・原文範囲を照合し、依頼JSONから候補を再計算する。元の依頼に記録された証拠予算と、同じケースに保存された正規レポート・依存・採用文面・`HYPOTHESIS` が一致する場合に、既存の作用再照合へ進む。旧版の追問は、封印済みの照応先と以前の確定候補、当時の内容計画・描画・計算記録を照合する。v89およびv95〜v107で作成された追問は、V4候補レポートの正規バイトと旧版の本文・単一の先行結果依存が一致する場合に、当時の形で復旧する。作用ID、許可、対象、実バイトが一致した場合にのみ受領記録を確定する。

凍結版・原典台帳・着工点でblob `94490a6f4352afbb090c76ed9f43ade031e510c7` が一致する `src/minidora/純粋結果庫.py` 全58行を、統合実行・統合能力・純粋再利用の実利用先に照合した。凍結版は版と実行文脈の鍵で成功結果をセッション内に再利用する。原典読解は147/416件。凍結版の実行、永続的な中断復旧との同等性は未検証。93要件の受入は `UNVERIFIED`、E6は継続中。全workspace実行試験、厳格Clippy再検査、E7体系QC、E8独立QA、公開・配布は `NOT_RUN`。

Linux x86_64 6.18.57、Rust/Cargo 1.99.1、製品 `0.7.0-alpha.11`。実CLI SHA-256は基線 `9f859d00438040fb040d017a12d493c6a962bb3677a3c987c32bad51c6f38a3c`、最終 `d0f5023a7cc1cb9e37210770d2c4837788ce6db0f69767609fdabfc3c50bf2d2`。`Cargo.lock` は `b4e97fecdca8fd1fc863349b1a48eb99a925da7fed0ed6ce6cfe79bdd1cececc`。Core実装と復旧回帰を含む3ファイルの `git diff --binary` SHA-256は `43c10c78242fe60d756e0a86403c9fffe34822c1fab3cbd558849fa7ba1f62bf`。

- `cargo fmt --all -- --check`、`cargo check --workspace --all-targets --locked`、`cargo build -p minidora-cli --locked` は終了0。復旧66件、抽出・出典13件、通常CLI6件の計85件がPASS。追加のRust回帰では、通常Engineで許可先に書き込んだ直接候補を中断時点へ戻し、同一作用での復旧と、レポートのケース内参照を失った場合の停止を確認した。
- 独立した最終CLIの近接検査では、旧直接候補、旧v107追問、新v108追問、および同じ表示を持つ別候補が存在する場合の計8ファイルが `LOCAL_FILE_RECONCILED_NO_RESEND`。同一IDの再実行結果、本文、作用ID、inode、更新時刻が一致した。正しい根拠の欠落3件、別の同じ表示を持つ候補だけが残った場合2件、別主体3件は受領記録の追加と書き込みなしで停止した。復旧した直接要約を元に、新規v108の根拠付き説明も成立した。
- 追加の互換検査では、B146のv89とB148〜B160のv95〜v107の固定CLIから、それぞれ真正な確定結果と中断記録を作成した。v107だけを接続した途中版ではv89・v95〜v106の13件が `RECOVERY_CONTENT_MISMATCH` で停止した。最終版は全14版・14ファイルで、当時の版、結果ID、本文、作用ID、実バイト、inode、更新時刻を保持し、再書き込みなく復旧した。近接検査との合計は実ファイル22件の復旧と8件の停止。V4以外の旧候補形式は今回の対象外。

復旧2世代は `33430fb` と `81887fc`。終了時資産は計9件。保持3件（実装記録、要求トレース、読解台帳）、稼働中4件（復旧回帰を含む実装3ファイル、共有Cargoキャッシュ）、保管済み・保留は各0件。検証用 `/workspace/.minidora-cloud/b162-legacy-review.YGORth` と `/workspace/.minidora-cloud/b162-history-review.aT8TBj` をLinuxのGIOゴミ箱へ移動した。元パスの不存在、収容物、復元情報を照合した。計9,223ファイル、531,740,936論理バイト、最終処分待ち2件。容量回収0。Windows保管とオンライン専用化は非該当。

## 2026-11-20 JST B163 — D3-C E6 先行回答の要約説明を中断から復旧

着工点は `0ac53221f7208ce0049e6ba22ea826a4c0cacd9d`。D2仕様・工程とD3-C指示を全読し、採用SHA-256、R2 §14・§16・§20、E5合同判定を照合した。着工点では、前回回答の要約を詳しく説明すると元回答の範囲を表示した。許可ファイルへの要約自体は復旧したが、その説明の中断記録は `RECOVERY_CONTENT_MISMATCH` で停止した。

Coreの通常説明と中断復旧が、元回答・要約の封印入力、当時の回答本文と出力記録、元の文数・文字数条件、正規レポートを同じ手順で照合する。保存済みの要約からは選んだ文の元回答内の範囲を説明し、従来の本文、依存順、`HYPOTHESIS` を維持する。中断した説明の採用内容が再計算と一致すれば、元の許可・作用ID・対象・実バイトを照合して受領を確定する。着工点で通っていた元回答・要約の封印登録欠落は停止する。同一見出しの通常取り込み文は、ユーザー前提としてその本文を返す。

凍結版・原典台帳・着工点でblob `cdf2e2104c783e434f308caf5bcaa4b28266717f` が一致する `src/minidora/文脈要求.py` 全143行を、要求解釈・文脈照応・要求計画実行・多言語要求接続の実利用先に照合した。凍結版は同じ文脈の採用済み回答を要約の実入力に束縛し、実行後に行数条件を確認する。原典読解は148/416件。凍結版の実行と永続的な中断復旧との等価性は未検証。93要件の受入は `UNVERIFIED`、E6は継続中。全workspace実行試験、厳格Clippy再検査、E7体系QC、E8独立QA、公開・配布は `NOT_RUN`。

Linux x86_64 6.18.57、Rust/Cargo 1.99.1、製品 `0.7.0-alpha.11`。実CLI SHA-256は基線 `d0f5023a7cc1cb9e37210770d2c4837788ce6db0f69767609fdabfc3c50bf2d2`、最終 `a1d7bcbcfe4a7c0ee6715c6a3f0efa59124189cb63dbb59f1c86f0c6db87ecb9`。`Cargo.lock` は `b4e97fecdca8fd1fc863349b1a48eb99a925da7fed0ed6ce6cfe79bdd1cececc`。実装と通常CLI回帰4ファイルの `git diff --binary` SHA-256は `87fcf72211770403c04048902a3c7ca208fb7351cc9ca4bfeab446365dbfb102`。

- `cargo fmt --all -- --check`、`cargo check --workspace --all-targets --locked`、`cargo build -p minidora-cli --locked` は終了0。復旧66件、抽出・出典13件、通常CLI8件で計87件がPASS。通常CLIでは元回答・要約・説明の別記録、依存の順序、元保証、実ファイルの受領、同一追問IDの再実行を照合した。同一見出しの通常文は `USER_PREMISE`、元結果への単一依存、受領なしを確認した。
- 独立の最終CLIで、旧v89要約2件、真正旧v107説明1件、着工点のv108説明2件、新v108説明2件、隣接する直接候補のv108説明1件を中断記録から復旧した。8件とも `LOCAL_FILE_RECONCILED_NO_RESEND`。同一IDの再生、本文・依存順・元作用ID、実バイト・inode・更新時刻が一致した。日英の依頼、Unicode・CRLF、制限付き要約、複数の証拠依存を含む。
- 通常実行では正規レポート・計画・描画・封印入力の欠落、別主体、別対象の許可、予算不足の計10件が新規ファイルなしで停止した。中断後の復旧ではレポート、元回答・要約の描画、元回答・要約・説明の封印入力の欠落、別主体の計7件が、新規作用と受領なしで停止した。着工点で誤って説明を採用した封印登録欠落2件と、同一見出しで停止した通常取り込み文の挙動を是正した。

復旧2世代は `0ac5322` と `33430fb`。終了時資産は計9件。保持3件（実装記録、要求トレース、読解台帳）、稼働中5件（実装3ファイル、通常CLI検査、共有Cargoキャッシュ）、保管済み・保留は各0件。検証用 `/workspace/.minidora-cloud/b163-legacy-review.zpEe19` 1件をLinuxのGIOゴミ箱へ移動し、元パスの不存在、収容物、復元情報を照合した。6,774ファイル、209,558,326論理バイト。最終処分待ち1件、容量回収0。Windows保管とオンライン専用化は非該当。

## 2026-11-20 JST B164 — D3-C E6 取得資料の要約を保存済みの出典から説明

着工点は `209c67ec557ca15196b1e93969db6c6ab1239392`。D2仕様・工程とD3-C指示の採用SHA-256、R2 §13・§14・§16・§20、E5合同判定を照合した。着工点では取得資料を要約した後の「詳しく説明して」が要約本文を再表示した。許可ファイルに書き込み済みの旧v108追問も、中断後は `RECOVERY_CONTENT_MISMATCH` で停止した。

新規の追問v109は、元の取得・要約の封印入力、対象版、本文packet、生成当時の文数・文字数条件、要約レポートと表示記録を照合する。採用した資料について、要求URLと最終URL、取得方式、時刻、packet、原本文と抽出本文のSHA-256、抽出器を表示する。選択文には抽出本文と要約本文の各UTF-8範囲、順位、先頭128文字までの抜粋を示す。同一URLの採用版は元要約の版規則から再現し、部分取得では保存済みの停止理由を引き継ぐ。説明の保証は `HYPOTHESIS` で、本文の網羅性・意味・条件保持・事実性は未検証。旧追問は採用済み本文と単一依存が一致する場合に、元の形で復旧する。

凍結版・原典台帳・着工点でblob `260ecedc6b0fea04d3b81e78be4d07f79480ff18` が一致する `src/minidora/HDS運用/一般接続.py` 全154行を、一般依頼、意味資料、工程の実利用先に照合した。凍結版は取得元・参照と資料本文の整合を照合し、本文を資料選択や保存済み回答の再表現へ渡す。原典読解は149/416件。凍結版の実行とRustの要約追問との等価性は未検証。93要件の受入は `UNVERIFIED`、E6は継続中。全workspace実行試験、厳格Clippy再検査、E7体系QC、E8独立QA、公開・配布は `NOT_RUN`。

Linux x86_64 6.18.57、Rust/Cargo 1.99.1、製品 `0.7.0-alpha.11`。実CLI SHA-256は基線 `a1d7bcbcfe4a7c0ee6715c6a3f0efa59124189cb63dbb59f1c86f0c6db87ecb9`、最終 `5f442d6f178f2b582d364a9db5025a43ac63754de63dfe38c0b787591eaae419`。`Cargo.lock` は `b4e97fecdca8fd1fc863349b1a48eb99a925da7fed0ed6ce6cfe79bdd1cececc`。本体と通常CLI回帰6ファイルの `git diff --binary` SHA-256は `2b1b00c2ef3fd7f41c9bdb3293860190a26b4a9c734ad760ade87078913d1014`。

- `cargo fmt --all -- --check`、`cargo check --workspace --all-targets --locked`、`cargo build -p minidora-cli --locked` は終了0。復旧66件、抽出・出典13件、通常CLI12件で計91件がPASS。制限付き取得資料要約の追問で文の順位、両byte範囲、実表記、packetの取得方法・時刻、SHA-256、元依存、同一ID再実行を照合した。v109の既存JSON集計、CSV列選択、文章構成・編集も通常経路で再検査した。
- 独立した最終CLIで、単一・複数・同一URLの最新版・保存済み部分取得の4ケースを照合した。旧v94要約、旧v108追問、新v109追問の計12ファイルは `LOCAL_FILE_RECONCILED_NO_RESEND`。本文、依存、同一入力IDの再生、元作用ID、バイト、inode、更新時刻が一致した。新規追問の9負例と中断復旧の4負例では、根拠・許可・予算を欠く場合に新規書き込み・受領なしで停止した。
- 単一・複数・最新版は `FIXTURE`。部分取得は、過去に隔離したローカルTLSで作成したB138の `SUSPEND`、取得済み本文、通信開始上限の停止記録を読み直した。取得方式は保存された値を表示した。今回はネットワーク取得・プロキシ・外部プロバイダを使用せず、外部LIVE取得の検査は `NOT_RUN`。

復旧2世代は `209c67e` と `0ac5322`。終了時資産は計11件。保持3件（実装記録、要求トレース、読解台帳）、稼働中7件（実装5ファイル、通常CLI検査、共有Cargoキャッシュ）、保管済み・保留は各0件。検証用 `/workspace/.minidora-cloud/b164-legacy-review.4zvMcP` 1件をLinuxのGIOゴミ箱へ移動し、元パスの不存在、収容物、復元情報を照合した。6,336ファイル、206,783,956論理バイト。最終処分待ち1件、容量回収0。Windows保管とオンライン専用化は非該当。

## 2026-11-20 JST B165 — D3-C E6 取得資料への質問で選んだ候補の根拠を説明

着工点は `7e763400ff7334961e26d4afcde4ee55dc438b5a`。D2仕様・工程とD3-C指示の採用SHA-256、R2 §13・§14・§16・§20、E5合同判定を照合した。着工点の通常CLIでは取得資料への質問に続く「詳しく説明して」が元回答を再表示し、旧v109追問の許可ファイルは中断復旧で `RECOVERY_CONTENT_MISMATCH` になった。

新規追問v110は、封印済みの元質問・取得入力、取得結果、保存packet、回答レポート、表示記録を照合する。回答当時のV1/V2で語句照合と候補順位を再計算し、照合語、採用対象、候補ごとの一致語、取得方法・日時、出典、原本文・抽出本文・引用範囲のSHA-256、抽出済み本文のUTF-8範囲と抜粋を説明する。複数資料、同じ最終URLの採用版、部分取得の停止理由を当時の記録から扱う。保証区分は `HYPOTHESIS` で、回答の意味・網羅性・資料の信頼性と事実性は未検証。旧追問は元の本文と依存が一致した場合にその形で復旧する。語句不一致で停止した質問は採用済み回答の説明へ進まない。

凍結版・原典台帳・着工点でblob `1b775399939e0a8b155474d82790d8bfb31eddff` が一致する `src/minidora/文脈命題接続.py` 全169行を、汎用会話・文脈命題回答・会話回答の実利用先に照合した。凍結版は取得記録、参照、全文境界、抜粋の文字位置と必要語を再検査し、全文を命題判定へ渡す。原典読解は150/416件。凍結版の実行とRustとの等価性は未検証。93要件の受入は `UNVERIFIED`、E6は継続中。全workspace実行試験、厳格Clippy再検査、E7体系QC、E8独立QA、公開・配布は `NOT_RUN`。

Linux x86_64 6.18.57、Rust/Cargo 1.99.1、製品 `0.7.0-alpha.11`。実CLI SHA-256は基線 `5f442d6f178f2b582d364a9db5025a43ac63754de63dfe38c0b787591eaae419`、最終 `4bf78f8286c9252f079412811198f366a600fd85fabed4680c0e94a645b2cfe9`。`Cargo.lock` は `b4e97fecdca8fd1fc863349b1a48eb99a925da7fed0ed6ce6cfe79bdd1cececc`。本体と通常CLI回帰5ファイルの `git diff --binary` SHA-256は `185e1a69f36c19974f43e27e09b943eea4d12fde915c36ee843a0fc7ddcd3b8d`。

- `cargo fmt --all -- --check`、`cargo check --workspace --all-targets --locked`、`cargo build -p minidora-cli --locked` は終了0。復旧66件、抽出・出典13件、通常CLI10件で計89件がPASS。通常の質問から詳説へ進み、2候補の実表記・固定UTF-8範囲・SHA-256・順位・依存・同一入力ID再生を照合した。V1/V2の既存回答レポート再計算、v110での資料要約・回答要約・直接候補・JSON集計・文章構成も検査した。
- 独立した実CLIの単一・複数・同一URLの最新版・保存済み部分取得の4ケースで、V2の直接回答4件、旧v109追問4件、新v110詳説4件を復旧した。既存v109の取得資料要約の詳説互換1件を含め、計13ファイルは `LOCAL_FILE_RECONCILED_NO_RESEND`。本文、依存、同一入力ID再生、元作用ID、バイト、inode、更新時刻を照合した。根拠や登録入力の欠落、別主体・許可・予算等の通常9負例と中断4負例は新規書き込み・受領なしで停止した。
- 最初の3ケースは `FIXTURE`、部分取得は過去の隔離ローカルTLSで保存した取得 `SUSPEND` を使用した。今回は外部通信・再取得0件。外部LIVE取得は `NOT_RUN`。

復旧2世代は `7e76340` と `209c67e`。終了時資産は計10件。保持3件（実装記録、要求トレース、読解台帳）、稼働中6件（実装4ファイル、通常CLI検査、共有Cargoキャッシュ）、保管済み・保留は各0件。検証用 `/workspace/.minidora-cloud/b165-legacy-review.4vxiid` 1件をLinuxのGIOゴミ箱へ移動し、元パスの不存在、収容物、復元情報を照合した。6,809ファイル、227,424,518論理バイト。最終処分待ち1件、容量回収0。Windows保管とオンライン専用化は非該当。

## 2026-11-20 JST B166 — D3-C E6 検索一覧の掲載順位と代替検索の根拠を説明

着工点は `996cd3e852db0e821839881537dd7e37e83e0a75`。D2仕様・工程とD3-C指示の採用SHA-256、R2 §13・§14・§16・§20、E5合同判定を照合した。着工点の通常CLIは「詳しく説明して」に元の検索一覧を再表示した。旧追問の中断復旧自体は成立したが、採用済み検索のレポート参照を失った隔離例でも受領を確定した。

新規追問v111は、元の検索語・検索先・許可・予算に基づき、保存済みの検索一覧を照合する。検索先を切り替えた記録、要求URLと最終URL、取得時刻、packet、原応答と採用記録のSHA-256、候補の元掲載順位と除外理由を示す。一覧の保証は `SOURCE_REPORT`。候補本文の取得や事実性の確認は別段階であることを表示する。旧追問は当時の本文と単一依存のまま検証し、元検索レポートを復元できない場合は受領を確定しない。実本文を取得した結果の追問は従来の経路を通る。

凍結版・原典台帳・着工点でblob `690148aa120730bac684c26d0ad0ec94e11ab65d` が一致する `src/minidora/Crossref参照.py` 全207行を、標準参照と複合参照の実利用先に照合した。凍結版は候補の題名・要旨、出典、取得失敗を複合検索へ渡す。別の検索が失敗した後のキャッシュ命中で、前のエラーが残り得る。原典読解は151/416件。凍結版の実行とRustとの等価性は未検証。93要件の受入は `UNVERIFIED`、E6は継続中。全workspace実行試験、厳格Clippy再検査、E7体系QC、E8独立QA、公開・配布は `NOT_RUN`。

Linux x86_64 6.18.57、Rust/Cargo 1.99.1、製品 `0.7.0-alpha.11`。実CLI SHA-256は基線 `4bf78f8286c9252f079412811198f366a600fd85fabed4680c0e94a645b2cfe9`、隔離試験対象 `2841cb6e29971b47609f093e70328251a5699f8e375bfef7d75d9d671fed8196`、同じソースの終了前再ビルド `e162f2a955c101c57d83899f2d5863cbec1cd58710f35150024c05e088630fbd`。`Cargo.lock` は `b4e97fecdca8fd1fc863349b1a48eb99a925da7fed0ed6ce6cfe79bdd1cececc`。本体と回帰検査8ファイルの `git diff --binary` SHA-256は `6cdab3a2ac121e5a4974647a33bab271dfa3b05ac9be134d0a4f38217e86d961`。

- `cargo fmt --all -- --check`、`cargo check --workspace --all-targets --locked`、`cargo build -p minidora-cli --locked` は終了0。通常CLI9件、検索11件、復旧66件の計86件がPASS。通常CLIでは検索メタデータと読み取り権限、既存の資料質問・要約、JSON集計、文章構成の追問を再検査した。
- 隔離ローカルTLSの `check_ordered_search` は、既存11ケースと追加の代替検索一覧の追問・同一ID再実行がPASS。HTTP 429で切り替え、先頭候補を除外して掲載順位2を保持した。全17接続・HTTP要求はループバック内で処理し、追問・再実行は追加通信0件。TLSの証明書検証を維持した。
- 独立した実CLIで単一検索、代替検索、空の検索一覧、取得本文の各1ケースを検証した。元結果4件、旧v110追問4件、新v111追問4件の計12ファイルを `LOCAL_FILE_RECONCILED_NO_RESEND` で復旧し、本文・依存・同一ID再実行・実バイト・inode・更新時刻を照合した。根拠・許可・予算を欠く通常9負例と中断復旧4負例は、新規書き込みと受領なしで停止した。初回の検索・取得6件のみループバックで通信し、追問と復旧は接続閉鎖後に実行した。外部LIVE取得は `NOT_RUN`。

復旧2世代は `996cd3e` と `7e76340`。終了時資産は計13件。保持3件（実装記録、要求トレース、読解台帳）、稼働中9件（実装6ファイル、回帰検査2ファイル、共有Cargoキャッシュ）、保管済み・保留は各0件。検証用 `/workspace/.minidora-cloud/b166-legacy-review.5NSBV8` 1件をLinuxのGIOゴミ箱へ移動し、元パスの不存在、収容物、復元情報を照合した。4,009ファイル、184,611,612論理バイト。最終処分待ち1件、容量回収0。Windows保管とオンライン専用化は非該当。

## 2026-11-20 JST B167 — D3-C E6 取得した検索候補本文を原掲載順位と取得記録から説明

着工点は `14e485f16929a607c16706fc008576b2b5a8ebc9`。D2仕様・工程とD3-C指示の採用SHA-256、R2 §13・§14・§16・§20、E5合同判定を照合した。着工点では、検索候補本文を取得した後の「詳しく説明して」が取得完了メッセージを再表示した。旧追問は元の検索失敗観測または選択本文の保存参照が欠けても、中断後に受領を確定した。

新規追問v112は、封印済みの候補順位、元検索の候補と検索先、取得時の許可・予算、選択した本文の保存packet、実行グラフと表示を照合する。原掲載順位・候補ID・一覧packetを、要求URL・最終URL・実観測時刻・読み取り方式・原本文と抽出本文のSHA-256へ結び付けて説明する。続けて本文を取得したケースでも、説明対象は追問で参照した取得結果が採用した本文。過去の本文は依存に保持する。保証は `SOURCE_REPORT` で、本文の事実性は未検証。旧追問は当時の本文と単一依存の形を保ち、元検索・本文の根拠を再検証できる場合に復旧する。

凍結版・原典台帳・着工点のblob `bbda5af1b4078f34864fc5c532e34468333c9558` が一致する `src/minidora/HDS参照.py` 全357行を確認した。凍結版は観測要求を起点に検索を組み、観測・関係・選択肢の由来を推論と再生記録へ渡す。初回結果が件数上限を埋めると、代替検索の新規候補が最終結果に入らない場合がある。原典読解は152/416件。凍結版の実行とRustとの等価性は未検証。93要件の受入は `UNVERIFIED`、E6は継続中。全workspace実行試験、厳格Clippy再検査、E7体系QC、E8独立QA、公開・配布は `NOT_RUN`。

Linux x86_64 6.18.57、Rust/Cargo 1.99.1、製品 `0.7.0-alpha.11`。実CLI SHA-256は基線 `e162f2a955c101c57d83899f2d5863cbec1cd58710f35150024c05e088630fbd`、最終 `d3543b798a6a789c11a095ffa30562aa8007db06efb877c70b50651f1fede9c9`。`Cargo.lock` は `b4e97fecdca8fd1fc863349b1a48eb99a925da7fed0ed6ce6cfe79bdd1cececc`。本体と回帰検査8ファイルの `git diff --binary` SHA-256は `77cd44c5404038f33e9d33299f3cb59caf912c94cd8999d313cfa02c3a4ae4a1`。

- `cargo fmt --all -- --check`、`cargo check --workspace --all-targets --locked`、`cargo build -p minidora-cli --locked` は終了0。通常CLI9件、検索11件、復旧66件の計86件がPASS。
- 隔離ローカルTLSの `check_ordered_search` は既存11ケースと一覧・候補本文の追問がPASS。429後の代替検索と取得本文の表示、元順位、URL・ハッシュ、証拠依存、同一ID再実行を照合した。試験全体で17接続・17 HTTP要求をループバック内で処理し、追問・再実行の追加通信は0件。
- 独立した実CLIで単独検索の順位3と本文リダイレクト、503後の代替検索の順位2、同じ一覧から順位1と3の本文を順に取得したケースを検証した。旧取得結果3件、旧v111追問3件、新v112追問3件、B166の旧v111一覧追問1件の計10ファイルを `LOCAL_FILE_RECONCILED_NO_RESEND` で復旧した。本文、元作用ID、実バイト・inode・更新時刻、同一IDの再実行を照合した。通常13負例と中断6負例は新規書き込み・受領なしで停止した。取得に使った10接続・10 HTTP要求のみ隔離ループバックで通信し、試験のTLS証明書検証を維持した。外部LIVE取得は `NOT_RUN`。

復旧2世代は `14e485f` と `996cd3e`。終了時資産は計13件。保持3件（実装記録、要求トレース、読解台帳）、稼働中9件（実装6ファイル、回帰検査2ファイル、共有Cargoキャッシュ）、保管済み・保留は各0件。検証用 `/workspace/.minidora-cloud/b167-legacy-review.9hYzZ5` 1件をLinuxのGIOゴミ箱へ移動し、元パスの不存在、収容物、復元情報を照合した。5,480ファイル、199,356,675論理バイト。最終処分待ち1件、容量回収0。Windows保管とオンライン専用化は非該当。

## 2026-11-20 JST B168 — D3-C E6 検索候補本文の詳説から同じ本文の要約・質問へ接続

着工点は `eb7cf8cb7ac504e0a9ec57e39b92333abd1ee026`。D2仕様・工程とD3-C指示の採用SHA-256、R2 §13・§14・§16・§20、E5合同判定を照合した。着工点の単独検索・代替検索・候補本文を続けて取得したケースでは、取得直後の要約・質問は成立した。本文の取得根拠を詳説した後の同じ依頼は6件とも `SOURCE_BODY_UNAVAILABLE` で停止した。

v113は、前回の候補本文の詳説を封印した文脈として保持し、その登録済み入力・実結果・描画・元検索・本文取得を再検証する。説明した取得結果が選択した本文packetだけを既存の要約と出典質問へ渡す。複数の候補本文が保持されていても、元のpacket位置と版をそのまま使う。成果は、詳説、取得結果、本文packet、生成したartifactの順で依存を保持する。要約と質問は `HYPOTHESIS`。同じ本文への後続要約・質問は、採用済み成果または検証済みの語句不一致から詳説の文脈へ戻る。対象は取得結果を直接説明したv112詳説1段で、一覧の説明と詳説の再表示は本文の文脈にならない。

凍結版・原典台帳・着工点のblob `529a7745cb3389eb331622cfda411e90b0e20d14` が一致する `src/minidora/HDS運用/一般依頼.py` 全607行を確認した。凍結版は先行回答の採用状態と主題を照合し、原資料本文と前回の取得記録を後続の資料選択・構成へ渡す。新たな資料取得にはその依頼の許可を要求する。原典読解は153/416件。凍結版の実行とRustとの等価性は未検証。93要件の受入は `UNVERIFIED`、E6は継続中。全workspace実行試験、厳格Clippy再検査、E7体系QC、E8独立QA、公開・配布は `NOT_RUN`。

Linux x86_64 6.18.57、Rust/Cargo 1.99.1、製品 `0.7.0-alpha.11`。実CLI SHA-256は基線 `d3543b798a6a789c11a095ffa30562aa8007db06efb877c70b50651f1fede9c9`、最終 `1ad806a95bd047184cdd645fcaeae0d7765f863cd63991062c924be3af013c29`。`Cargo.lock` は `b4e97fecdca8fd1fc863349b1a48eb99a925da7fed0ed6ce6cfe79bdd1cececc`。本体と回帰検査6ファイルの `git diff --binary` SHA-256は `64c82addffd8d006ef09291d35c7fa028d632e7477b6822b1702b7c5f252798f`。

- `cargo fmt --all -- --check`、`cargo check --workspace --all-targets --locked`、`cargo build -p minidora-cli --locked` は終了0。通常CLI9件、検索11件、復旧66件、要約・出典13件の計99件がPASS。
- 隔離ローカルTLSの `check_ordered_search` は11ケースと詳説からの要約・質問がPASS。暗黙の要約とその後の暗黙の質問が同じ詳説を参照し、実本文、引用、保存artifact、描画、同一ID再実行を照合した。全17接続・17 HTTP要求はループバック内で処理し、追問による追加通信は0件。
- 独立したCLIで単独検索、代替検索、連続取得を検証した。先に取得した別本文にしかない語句の質問は `SOURCE_QUESTION_NO_MATCH`。その後の12文字制限付き要約と選択本文への質問は元の詳説に戻った。v112詳説3件とv113要約・質問6件の計9ファイルは `LOCAL_FILE_RECONCILED_NO_RESEND`。通常10負例と中断4負例は新規書き込み・受領なしで停止した。初回取得の10接続・10 HTTP要求のみ隔離ループバックで通信し、TLS検証を維持した。外部LIVE取得は `NOT_RUN`。

復旧2世代は `eb7cf8c` と `14e485f`。終了時資産は計11件。保持3件（実装記録、要求トレース、読解台帳）、稼働中7件（実装5ファイル、回帰検査1ファイル、共有Cargoキャッシュ）、保管済み・保留は各0件。検証用 `/workspace/.minidora-cloud/b168-legacy-review.FJDgIC` 1件をLinuxのGIOゴミ箱へ移動し、元パスの不存在、収容物、復元情報を照合した。9,254ファイル、244,283,923論理バイト。最終処分待ち1件、容量回収0。Windows保管とオンライン専用化は非該当。

## 2026-11-20 JST B169 — D3-C E6 取得資料の要約・質問の詳説を挟んで原本文に戻る

着工点は `e21b0addbc68e27658f602c0a7929cb4ee48c874`。D2仕様・工程とD3-C指示の採用SHA-256、R2 §13・§14・§16・§20、E5合同判定を照合した。着工点では取得資料の要約や回答を詳説した後、その詳説が保持する出典から次の本文質問・要約へ戻る経路がなかった。

本文への暗黙の追問では、直前の要約・質問の詳説を、その登録済み入力、目的、元演算、採用済み内容、保存された描画から再検証する。続く質問・要約の参照先は、詳説された要約・質問が封印した実出典の文脈とする。履歴の詳説を挟んだままでも、各依頼は同じ出典の文脈へ直接結び付く。B168の選択済み候補本文はv113の参照を引き継ぐ。直接取得の複数資料と部分取得は既存の本文選択・停止通知を使う。対象は証拠を再検証できる資料要約・質問の詳説と、暗黙の本文質問・要約。復旧では新規依頼に封印した参照先と実出典を照合する。

凍結版・原典台帳・着工点のblob `a9de9be1c6063aa2f33924d35d2627e115258e28` が一致する `src/minidora/HDS運用/意味資料.py` 全318行を確認した。凍結版は取得記録、資料本文、順序・区間を照合し、後続の構成と採否へ渡す。原典読解は154/416件。凍結版の実行とRustとの等価性は未検証。93要件の受入は `UNVERIFIED`、E6は継続中。全workspace実行試験、厳格Clippy再検査、E7体系QC、E8独立QA、公開・配布は `NOT_RUN`。

Linux x86_64 6.18.57、Rust/Cargo 1.99.1、製品 `0.7.0-alpha.11`。実CLI SHA-256は基線 `1ad806a95bd047184cdd645fcaeae0d7765f863cd63991062c924be3af013c29`、最終 `2d505ffc507c2e3697527b591b4de0aad285c51ca546cf0628eae8f28ccec9f0`。`Cargo.lock` は `b4e97fecdca8fd1fc863349b1a48eb99a925da7fed0ed6ce6cfe79bdd1cececc`。本体と回帰検査4ファイルの `git diff --binary` SHA-256は `ddb293ace34b96534ef9552b86d0cd4ae78c512211c49adc27ecdddd220a6ebf`。

- `cargo fmt --all -- --check`、`cargo check --workspace --all-targets --locked`、`cargo build -p minidora-cli --locked` は終了0。通常CLI9件、検索11件、復旧66件、要約・出典13件の計99件がPASS。
- 独立した実CLIで、直接の複数資料2件、選択済み候補本文2件、保存済みの部分取得1件を検証した。着工点の詳説後は5件とも `SOURCE_BODY_UNAVAILABLE`。修正版は5件とも成立し、詳説前に実行した同条件の比較結果と、本文、依存、必須内容、封印した参照先が一致した。選択していない候補だけに一致する質問は `SOURCE_QUESTION_NO_MATCH`。その後も選択した本文で別の質問が成立した。部分取得の通知も照合した。
- 旧詳説5件と新しい要約・質問5件の計10ファイルを `LOCAL_FILE_RECONCILED_NO_RESEND` で復旧し、同一入力IDで再実行した。修正版の復旧では要約・質問に封印した出典を使い、中間詳説の登録を外した隔離例でも本文・元作用・実ファイルを照合した。
- 保存済みv108〜v111の説明5件を同一IDで再実行した。出典付きのv109要約詳説、v110・v111質問詳説は、2資料の元範囲で次の質問・要約へ進んだ。元の文面を再表示する旧版2件からの本文追問は `SOURCE_BODY_UNAVAILABLE` で停止した。
- 隔離ローカルTLSの `check_ordered_search` は11ケースがPASS。選択候補に対する要約→詳説→質問→詳説→要約の各依頼で、同じv113参照、原本文の範囲とSHA-256、実描画、同一ID再実行を照合した。全17接続・17 HTTP要求はループバック内で処理し、追問の追加通信は0件。外部LIVE取得は `NOT_RUN`。
- 独立の負例12件で、根拠の欠落、権限・主体・予算不足に対する9停止、別候補だけに一致する質問、詳説の再表示、追加候補取得の境界を照合した。認証付きループバックHTTPの明示参照3件では、詳説、元の複数資料、選択済み本文の各参照を指定し、指定先に応じた停止または成立を確認した。

復旧2世代は `e21b0ad` と `eb7cf8c`。終了時資産は計9件。保持3件（実装記録、要求トレース、読解台帳）、稼働中5件（実装3ファイル、回帰検査1ファイル、共有Cargoキャッシュ）、保管済み・保留は各0件。検証用 `/workspace/.minidora-cloud/b169-legacy-review.gQYzzh` 1件をLinuxのGIOゴミ箱へ移動し、元パスの不存在、収容物、復元情報を照合した。14,529ファイル、333,316,979論理バイト。最終処分待ち1件、容量回収0。Windows保管とオンライン専用化は非該当。

## 2026-11-20 JST B170 — D3-C E6 過去の詳説を明示参照して元本文で要約・質問する

着工点は `f8331024a08d939e8a4ab155e8bb60b0f32559a9`。D2仕様・工程とD3-C指示の採用SHA-256、R2 §13・§14・§16・§20、E5合同判定を照合した。着工点の認証付きループバックHTTPでは、同じ出典を使った暗黙の追問5件が成立し、詳説IDを明示した5件は `SOURCE_BODY_UNAVAILABLE` で停止した。

CLIの `--context RESULT_ID` と既存HTTPの `context_ref` を、同一ケースの保存済み詳説に対する要約・本文質問へ接続した。v114は指定された詳説IDを原依頼と意味の参照に保持する。封印入力と中間の要約・質問を元の出典から順に再検証し、表示、依存、選択済み本文packetを照合する。連続した明示参照の履歴は反復してたどる。参照を省略した後続依頼は検証済みの元の出典文脈を使う。保証は `HYPOTHESIS`。対象は証拠を再検証できる資料要約・本文質問の詳説。複数資料、部分取得通知、B168の選択済み本文の範囲を各々維持する。

凍結版・原典台帳・着工点のblob `43c1e10f598419d99b4b6f3fd5fefb357667a868` が一致する `src/minidora/要求解釈.py` 全385行を確認した。凍結版では指定された応答番号・出力番号を採用済みの結果と会話状態に結び付けてから、対応する素材を構成工程へ渡す。原典読解は155/416件。凍結版の実行とRustとの等価性は未検証。93要件の受入は `UNVERIFIED`、E6は継続中。全workspace実行試験、厳格Clippy再検査、E7体系QC、E8独立QA、公開・配布は `NOT_RUN`。

Linux x86_64 6.18.57、Rust/Cargo 1.99.1、製品 `0.7.0-alpha.11`。実CLI SHA-256は基線 `2d505ffc507c2e3697527b591b4de0aad285c51ca546cf0628eae8f28ccec9f0`、最終 `67f1ec39f72582e30f40b0ee227b0da1d7f247a43593876afd8e150fb62233ca`。`Cargo.lock` は `b4e97fecdca8fd1fc863349b1a48eb99a925da7fed0ed6ce6cfe79bdd1cececc`。本体と回帰検査5ファイルの `git diff --binary` SHA-256は `dce054f333542aa3a82928c3489557b1e84b95695e2164f546ce814675653bc3`。

- `cargo fmt --all -- --check`、`cargo check --workspace --all-targets --locked`、`cargo build -p minidora-cli --locked` は終了0。通常CLI9件、CLIバイナリ10件、検索11件、復旧66件、要約・出典13件の計109件がPASS。空の結果ID、`inspect` と `reconcile` での `--context` は3件とも終了1。
- 独立の実CLI3件と認証付きループバックHTTP2件で、元の複数資料・選択本文・部分取得の5件が成立した。元本文への直接入力と成果の本文・必須内容が一致し、明示した詳説ID、文脈SHA-256、元取得結果、対象packet、導出artifactを照合した。旧版で停止した保存結果2件は同一IDで再生した。旧出力10件と新v114出力5件の計15ファイルは、同一ID再実行と `LOCAL_FILE_RECONCILED_NO_RESEND` を確認した。
- 保存済みv109〜v111の出典付き詳説3件は元本文へ進み、v108・v109の再表示2件は本文不足で停止した。既存の元取得・v113・部分取得を直接指定した対照例とCLIのモード制限を検査した。
- 隔離ローカルTLSの `check_ordered_search` は11ケースがPASS。別の最新ターンを挟みながら過去の質問詳説を指定して要約し、その要約の詳説を改めて指定した本文質問も成立した。全17接続・17 HTTP要求はループバック内で処理し、要約・詳説・質問の追加通信は0件。外部LIVE取得は `NOT_RUN`。
- 負例15ケースで、別主体・ケース、未登録ID、元入力・表示・packet・報告の欠落、権限・予算不足、同一入力IDの別文脈、復旧時のpacket欠落を検査した。新規の出力・受領は0件。認証HTTPでは主体と許可範囲の2境界を個別に検査した。
- デバッグ実行で複数資料の明示参照を5回連続で確定し、各成果の依存は5〜6件だった。6回目の詳説はアプリ設定30秒に対し試験側が22秒で打ち切った。保存状態は `RUNNING` で、同一IDの再実行は `INCOMPLETE_PRIOR_ATTEMPT`、作用・受領は0件。6回目の完走と長履歴の性能は未確認。選択本文の系列では4回連続で成立し、依存は各4件。暗黙の追問と語句不一致後の追問も同じ候補本文に戻った。

復旧2世代は `f833102` と `e21b0ad`。終了時資産は計10件。保持3件（実装記録、要求トレース、読解台帳）、稼働中6件（実装4ファイル、回帰検査1ファイル、共有Cargoキャッシュ）、保管済み・保留は各0件。検証用 `/workspace/.minidora-cloud/b170-legacy-review.WGfBNP` 1件をLinuxのGIOゴミ箱へ移動し、元パスの不存在、収容物、復元情報を照合した。16,737ファイル、393,127,926論理バイト。最終処分待ち1件、容量回収0。Windows保管とオンライン専用化は非該当。

## 2026-11-20 JST B171 — D3-C E6 明示参照の履歴照合を一つの読み取り状態にまとめる

着工点は `3df497b039a00c8f3a6529df60a84e237316872d`。D2仕様・工程とD3-C指示の採用SHA-256、R2の履歴・復旧・出典の契約、E5合同判定を照合した。B170の長い明示参照では、過去の入力・結果を個別に読むたびに `verify_case` が全リビジョンの連鎖を走査していた。B170で22秒打ち切りとなった6回目の詳説は、同じ直前状態、製品側30秒、試験側90秒で再実行すると完走した。

v114の履歴照合では読み取り用SQLiteトランザクションを開き、主体・ケースとリビジョン連鎖を照合した状態から、説明や元の導出に必要な入力・結果を読む。個々の封印入力・結果blob、実資料・表示・導出のSHA-256、依存関係は元の検証経路を通る。読み取りの終了とトランザクション解除の後にCoreが状態を進める。書き込み側は全履歴の照合、最新リビジョンの照合と既存の作用・復旧条件を適用する。

凍結版・原典台帳・着工点のblob `b4b007d96878283e38c15cc740309f469bd801dc` が一致する `src/minidora/製品版/監査.py` 全181行を確認した。凍結版は各応答のイベント順、ハッシュ、前応答のハッシュを記録し、監査APIで指定された応答を検証する。原典読解は156/416件。凍結版の実行とRustとの等価性は未検証。93要件の受入は `UNVERIFIED`、E6は継続中。全workspace実行試験、厳格Clippy再検査、E7体系QC、E8独立QA、公開・配布は `NOT_RUN`。

Linux x86_64 6.18.57、Rust/Cargo 1.99.1、製品 `0.7.0-alpha.11`。実CLI SHA-256は基線 `67f1ec39f72582e30f40b0ee227b0da1d7f247a43593876afd8e150fb62233ca`、最終 `6ea5033b4d0f55952e0dd93e842a31cbc18243b56029b128b51b716c96adf072`。`Cargo.lock` は `b4e97fecdca8fd1fc863349b1a48eb99a925da7fed0ed6ce6cfe79bdd1cececc`。

| デバッグCLIの同一履歴・同一入力 | B170実時間 | B171実時間 |
|---|---:|---:|
| 複数資料の6回目の詳説（リビジョン116起点） | 28.1719秒 | 12.6660秒 |
| 複数資料の明示参照（108起点） | 20.4839秒 | 7.1901秒 |
| 選択本文の暗黙参照（119起点） | 20.0677秒 | 13.3390秒 |
| 選択本文の明示参照（119起点） | 17.0676秒 | 8.9172秒 |

全4ケースで製品設定は30秒。各条件を保存済み履歴の独立複製で実行し、`ResultRecord`、`InputBundle`、目標記録、描画、ファイル作用0件を照合した。数値はこのLinuxデバッグ環境での試行値。

- `cargo fmt --all -- --check`、`cargo check --workspace --all-targets --locked`、`cargo build -p minidora-cli --locked` は終了0。通常CLI9件、Store32件、検索11件、復旧66件、要約・出典13件の計131件がPASS。Storeの回帰で、別の接続が保存した新入力・結果は実行中の読み取りから見えず、次の読み取りで元版・新版の両方を照合できた。別主体の拒否、結果blob破損の検出、失敗時のトランザクション終了を確認した。本体・回帰3ファイルの `git diff --binary` SHA-256は `a664ea0651defcd38947d9f19f24f1a86ac04750e240ab75527a7c1411f70165`。
- 同一HTTPプロセスで、履歴ハッシュの破損前に成立、破損後に `INTEGRITY_FAILURE` で拒否、復元後に同じ本文と依存で成立。詳説結果・表示・本文packetの保存結線欠落3件と別主体1件も停止した。以前の別候補にのみ一致する質問が `SOURCE_QUESTION_NO_MATCH` になった後、暗黙参照は選択本文に戻った。
- B170・B171で作成したv114の出力ファイル各1件（各1,724バイト）を `LOCAL_FILE_RECONCILED_NO_RESEND` で復旧し、実ファイル、inode、更新時刻、作用を照合した。選択本文を欠く復旧2件は受領を確定しなかった。隔離ローカルTLSの `check_ordered_search` は11ケースがPASS。17接続・17 HTTP要求はループバック内で、明示追問の追加通信は0件。外部LIVE取得は `NOT_RUN`。

復旧2世代は `3df497b` と `f833102`。終了時資産は計8件。保持3件（実装記録、要求トレース、読解台帳）、稼働中4件（実装・回帰3ファイル、共有Cargoキャッシュ）、保管済み・保留は各0件。検証用 `/workspace/.minidora-cloud/b171-legacy-review.nn0egx` 1件をLinuxのGIOゴミ箱へ移動し、元パスの不存在、収容物、復元情報を照合した。9,176ファイル、361,466,165論理バイト。最終処分待ち1件、容量回収0。Windows保管とオンライン専用化は非該当。

## 2026-11-20 JST B172 — D3-C E6 選択本文の根拠照合で同じ読み取り状態を使う

着工点は `f751c69b6c0af3314902f5635d98252325549880`。D2仕様・工程とD3-C指示の採用SHA-256、R2の履歴・復旧・出典の契約、E5合同判定を照合した。B171でv114の詳説履歴を一つの読み取りにまとめた後も、v113の選択本文に戻る際に、詳説の入力・結果、候補取得、先行取得、元検索の各読取がケース全履歴を繰り返し検証していた。

選択本文を照合するv113の追問と検索・本文の詳説は、認証済みケースの読み取り状態を1回開く。v114の履歴がv113へ戻る場合は、その読み取り状態を引き継ぐ。元の登録入力と検索一覧、元順位、取得本文、観測・描画、依存と認可をこれまでの判定で照合する。再取得の判断を持つ通常の候補取得と復旧の書込み境界では既存の経路を使う。対象は保存済み検索・候補本文の証明に伴う重複した履歴走査。

凍結版・原典台帳・着工点のblob `3b549abbbec7acd833df405815c55d36fdfe2e44` が一致する `src/minidora/HDS再生記録.py` 全152行を確認した。凍結版は候補別の検索経路、出典ID、資料の中間表現を収録し、オフライン評価で使う。通常製品での収録とRustとの等価性は未確認。原典読解は157/416件。93要件の受入は `UNVERIFIED`、E6は継続中。全workspace実行試験、厳格Clippy再検査、E7体系QC、E8独立QA、公開・配布は `NOT_RUN`。

Linux x86_64 6.18.57、Rust/Cargo 1.99.1、製品 `0.7.0-alpha.11`。実CLI SHA-256は基線 `6ea5033b4d0f55952e0dd93e842a31cbc18243b56029b128b51b716c96adf072`、最終 `e5a7627f6a55e1d0b2d0e3492fed70ceb8349c788f3bc90b8b8d114527055504`。`Cargo.lock` は `b4e97fecdca8fd1fc863349b1a48eb99a925da7fed0ed6ce6cfe79bdd1cececc`。本体3ファイルの `git diff --binary` SHA-256は `c47a3807fa2d80a8eef0e12a4bcab1d71e1f07e281e127144868c699926d3ac2`。

- `cargo fmt --all -- --check`、`cargo check --workspace --all-targets --locked`、`cargo build -p minidora-cli --locked` は終了0。通常CLI9件、Store32件、検索11件、復旧66件、要約・出典13件の計131件がPASS。

| 同一履歴・同一入力のLinuxデバッグCLI | B171実時間 | B172実時間 |
|---|---:|---:|
| 長い履歴から選択本文へ戻る暗黙参照 | 13.4859秒 | 10.8748秒 |
| 長い履歴からの選択本文の明示参照（v114） | 8.9268秒 | 6.3663秒 |
| リビジョン119で過去の選択本文の詳説を指定（v113） | 8.1134秒 | 5.7509秒 |
| 取得・詳説に続く選択本文参照（v113） | 0.8337秒 | 0.7566秒 |
| 選択候補を使わない複数資料の明示参照（対照） | 7.2316秒 | 7.5030秒 |

同じ保存状態の独立複製で5件を比較した。すべて `ResultRecord`、封印入力、目標、実行グラフがバイト単位で一致した。数値はこの環境での試行値で、対照条件も記録した。

- 先行候補を持つ検索と代替検索の保存状態で、v112の候補本文詳説2件の結果・目標・実行グラフが旧版と一致した。v113・v114の根拠欠落、別主体、予算の11負例でファイル作用は0件。認証済みの同一HTTPプロセスで、元検索一覧の表示記録を外すと停止し、結線を戻すと同じ本文・依存で成立した。先行する別候補だけに一致する質問から、暗黙参照で選択本文に戻る経路も確認した。
- B171・B172で作成したv113・v114の出力ファイル各1件、合計4件を `LOCAL_FILE_RECONCILED_NO_RESEND` で復旧した。元検索一覧の表示記録を欠く4件は `LOCAL_RECONCILIATION_UNCONFIRMED` で停止した。復旧・停止のいずれも新規ファイル作用は0件。
- 隔離ローカルTLSの `check_ordered_search` は11ケースがPASS。17接続・17 HTTP要求はループバック内で、明示追問による追加通信は0件。外部LIVE取得は `NOT_RUN`。

復旧2世代は `f751c69` と `3df497b`。終了時資産は計8件。保持3件（実装記録、要求トレース、読解台帳）、稼働中4件（実装3ファイル、共有Cargoキャッシュ）、保管済み・保留は各0件。検証用 `/workspace/.minidora-cloud/b172-legacy-review.bkzqhj` 1件をLinuxのGIOゴミ箱へ移動し、元パスの不存在、収容物、復元情報を照合した。15,044ファイル、510,284,137論理バイト。最終処分待ち1件、容量回収0。Windows保管とオンライン専用化は非該当。

## 2026-11-20 JST B173 — D3-C E6 部分取得の根拠と停止リビジョンを同じ読み取り状態で照合

着工点は `9fb0b1c5b769763ae02f9a3eff3dff213a827a16`。D2仕様・工程とD3-C指示の採用SHA-256、R2の履歴・復旧・出典の契約、E5合同判定を照合した。部分取得からの本文質問・要約では、封印入力と登録結果を読むたびにケースの全履歴を照合していた。

部分取得からの本文質問・要約では、認証済みSQLiteの読み取り状態内で元入力・登録結果を照合する。v114の履歴照合中は外側の読み取り状態を引き継ぐ。最初の接続前に止まった場合は、登録結果が完全一致した後、履歴を昇順で読み、最初に一致した `SUSPEND` リビジョンを使う。停止時点の根拠、本文と失敗packet、URL順・権限・予算、再試行待ちの照合は従来の条件で判定する。新たな取得を行う経路は引き続き取得側で検証する。

凍結版・原典台帳・着工点のblob `ec6d0e9d61a9917714151a9e2823e8e446efcebd` が一致する `src/minidora/実行系_HDS_v1.py` 全140行を確認した。凍結版は保留時に採用回答を消去しても、取得済み参照・供給器の診断・作用履歴を公開結果に保持する。本文追問との等価性と凍結版の実行は未確認。原典読解は158/416件。93要件の受入は `UNVERIFIED`、E6は継続中。全workspace実行試験、厳格Clippy再検査、E7体系QC、E8独立QA、公開・配布は `NOT_RUN`。

Linux x86_64 6.18.57、Rust/Cargo 1.99.1、製品 `0.7.0-alpha.11`。実CLI SHA-256は基線 `e5a7627f6a55e1d0b2d0e3492fed70ceb8349c788f3bc90b8b8d114527055504`、最終 `4ff19ac47a6816eee9a4e924d9e027c151516aab0d6b884c611272864d5e8ee7`。`Cargo.lock` は `b4e97fecdca8fd1fc863349b1a48eb99a925da7fed0ed6ce6cfe79bdd1cececc`。本体・回帰3ファイルの `git diff --binary` SHA-256は `ed25058959d5e4e455cf5453a71a1bce9fdc7a11509ebc8e2ca8a2035a2e81d2`。

| 同一履歴・同一入力のLinuxデバッグCLI | B172実時間 | B173実時間 |
|---|---:|---:|
| 部分取得直後の要約 | 0.1425秒 | 0.1528秒 |
| 部分取得直後の質問 | 0.1971秒 | 0.2804秒 |
| 84リビジョンからの詳説の明示参照（v114） | 4.6193秒 | 3.8738秒 |
| 同じ履歴の暗黙参照（v89） | 6.5777秒 | 5.7460秒 |
| 同じ履歴にある元の停止を明示参照（v94） | 4.2586秒 | 3.8266秒 |

取得回数の上限に達したB138の保存結果と、同じ本文を使った連続追問の保存状態を独立複製して比較した。全5件の結果・封印入力・目標・実行グラフ・出力SHA-256は一致した。数値はこの環境での単回試行値。

- `cargo fmt --all -- --check`、`cargo check --workspace --all-targets --locked`、`cargo build -p minidora-cli --locked` は終了0。通常CLI9件、Store32件、検索11件、復旧66件、要約・出典13件の計131件がPASS。Storeの回帰で、現在の読み取りから別接続が保存した新停止は見えず、次の読み取りでは元停止・新停止の両方を照合できた。同じ入力ID・版でも本文が異なる結果は停止リビジョンに一致しなかった。
- 保存済み入力・登録済み停止・当初の目標・本文packetの欠落、別主体、予算不足の9負例は、ファイル作用なしで停止した。
- 隔離ローカルTLSの部分取得5群24ケースがPASS。照合した停止は全URL処理後、リダイレクト時の待ち、初回接続前の待ち、期限、取得回数・原文バイトの予算。45接続・45 HTTP要求はTLSを検証したループバック内で処理し、出力15件を `LOCAL_FILE_RECONCILED_NO_RESEND` で復旧した。外部LIVE取得は `NOT_RUN`。
- B172・B173で作成した通常・v114の出力各1件、計4件を同一入力IDで再実行し、復旧した。1ファイル555バイトで、実バイト・inode・更新時刻・作用IDが一致した。元の登録済み停止を欠く4件は、既存ファイルが完全でも受領を確定しなかった。保存済み履歴の比較と復旧に通信は発生しなかった。

復旧2世代は `9fb0b1c` と `f751c69`。終了時資産は計8件。保持3件（実装記録、要求トレース、読解台帳）、稼働中4件（実装・回帰3ファイル、共有Cargoキャッシュ）、保管済み・保留は各0件。検証用 `/workspace/.minidora-cloud/b173-legacy-review.dgYSl9` 1件をLinuxのGIOゴミ箱へ移動し、元パスの不存在、収容物、復元情報を照合した。11,332ファイル、324,609,499論理バイト。最終処分待ち1件、容量回収0。Windows保管とオンライン専用化は非該当。
