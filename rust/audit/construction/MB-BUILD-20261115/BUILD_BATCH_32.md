# BUILD_BATCH_32 — Runtime smoke for typed atomic explanation

**Base implementation:** `fe39adbb0e7952536baccec5eecf32a2b1cc4eaa` (`feat(core): connect atomic explanations to K7`)

**Current gate:** `CORE-G03 = PARTIAL`; the other six gates were not reassessed in this batch.

**Purpose:** exercise the B31 direct typed relation route at runtime and observe its Core input, proof dependency, adopted content plan, actual K7 render, missing-evidence stop, and unauthorized file-output stop.

## Finite fixture and observations

- Seeded actor memory with `[scope=atomic-explanation-lab]Momo is a bird` through the existing Engine request route. The committed premise ID was retained as the expected proof root.
- Submitted a fresh `Proposition { subject: "Momo", predicate: "is", object: "bird", negative: false, scope: "atomic-explanation-lab" }` using `Engine::compose_atomic_explanation` with an empty request text.
- Confirmed the sealed request records `CORE_TYPED_ATOMIC_MEANING_V1` / `core-typed-atomic-meaning-v1`, an `AskWhy` meaning, `purpose=explain`, and the premise ID in the typed input's source IDs.
- Confirmed `COMMIT`, the source premise ID in both result and plan dependencies, a nonempty generated `ContentPlan`, and a `RENDERED` K7 observation whose bytes equal the adopted plan text. The result carries an execution graph reference.
- Submitted the unsupported atomic relation `Momo is mammal`. Core returned `SUSPEND` with `MISSING_PREMISES` before adopting a plan or render.
- Repeated the supported explanation with an output path and no write grant. Core produced the content plan and render, then returned `SUSPEND` with `WRITE_PERMISSION_MISSING`. The target file did not exist, no receipt was emitted, and the case action ledger remained empty.
- The integration test removes its isolated temporary database and target directory at teardown.

## Bounded checks

- `cargo fmt --all --check`: **PASS**.
- `cargo test --locked -p minidora-engine --test core_atomic_explanation_31 -- --nocapture`: **PASS**, 1 test, 0 failed, Rust `1.99.1`, Linux.
- `git diff --check`: **PASS**.
- Full workspace tests, output quality acceptance, proof-set mutation during a request, multi-relation composition, comparison, short composition, and ordinary CLI/HTTP exposure: **NOT_RUN** in this batch.

## Gate judgment

This is one finite runtime fixture and a focused safety check. It demonstrates a typed proposition flowing through the existing Core lifecycle to an adopted plan and actual render, with a retained premise dependency and two correct stopping outcomes. It does not establish general first-seen meaning coverage, independent semantic adequacy of the generated explanation, the full K4/K7 acceptance predicates, or the complete CORE-G03 population. `CORE-G03` therefore remains **PARTIAL**; no other gate status is inferred from the test count or result.
