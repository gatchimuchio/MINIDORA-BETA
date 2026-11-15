# BUILD_BATCH_33 — Retained runtime values for atomic explanation

**Main base:** `5fe4c8a36d64c99ea263e0a21654bb5d9261da83`.

**Core implementation under observation:** `fe39adbb0e7952536baccec5eecf32a2b1cc4eaa`; this batch changes only the integration-test trace and audit records.

**Current gate:** `CORE-G03 = PARTIAL`; no other gate was reassessed.

## Runtime producer → consumer values

- The existing Engine request route seeded `[scope=atomic-explanation-lab]Momo is a bird`. Core committed it as `USER_PREMISE`; the exact result and premise IDs are retained in [the runtime observation](b33-runtime-observation.json).
- `Engine::compose_atomic_explanation` received the typed proposition `{ subject: Momo, predicate: is, object: bird, negative: false, scope: atomic-explanation-lab }`, with `purpose=explain` and that premise ID in `source_ids`. The sealed input source/parser identities, digests, and semantic signature are in the JSON record.
- The existing `AskWhy` computation produced a `ContentPlan`; K7 adopted it and the UTF-8 renderer returned `RENDERED`. The result and plan both depend on the premise ID. The actual render is 474 bytes, SHA-256 `0c9598c8f401294da0123e19f8164c6825e0df7fad85d26bfdc4e48d5a2c7c9f`, and matches the adopted text.
- The exact output begins `提示された証拠が支える結論: Momoはbirdである` and then lists the same direct premise with its ID and scope. Thus the observed output provides source lineage and a truth-scope disclosure, but its asserted relation is the same relation as the direct premise. This fixture does **not** demonstrate a meaningfully new explanation or establish content quality; it reinforces `CORE-G03 = PARTIAL`.
- `Momo is mammal` stopped as `SUSPEND / MISSING_PREMISES` with no plan or render. A supported explanation requesting `unauthorized-output.txt` with zero write grants stopped after render as `SUSPEND / WRITE_PERMISSION_MISSING`; there was no file, receipt, or action record.

## Reproduction and retained artifact

- Test: `crates/minidora-engine/tests/core_atomic_explanation_31.rs` now prints the exact typed input, proof/result IDs, adopted units/text, model artifact references, render digest, and both terminal stops under `--nocapture`.
- `cargo fmt --all --check`: **PASS**.
- `cargo test --locked -p minidora-engine --test core_atomic_explanation_31 -- --nocapture`: **PASS**, 1 passed / 0 failed, `rustc 1.99.1 (58cf8f9c3 2026-10-13)`, Linux.
- `b33-runtime-observation.json`: **7,406 bytes**, SHA-256 `a7072e2ac45ea011dc1f5f3a77bfceaaf40ef9a3388424d1e84357be45a0b89e`.
- The test’s unique temporary database and unauthorized output target were removed/not created. No product code, input grammar, or permission behavior changed in this batch.
- Full-workspace tests, explanation-quality acceptance, ordinary CLI/HTTP exposure, and the remaining CORE-G03 population: **NOT_RUN**.

## Next connection

The next Core-side probe should explain a typed derived relation whose proof contains multiple premises and a rule, then inspect whether K4 constructs content beyond restating the conclusion and whether every generated claim retains its proof dependencies. This remains direct-Core G03 work; it does not authorize input-machine or specialist expansion.
