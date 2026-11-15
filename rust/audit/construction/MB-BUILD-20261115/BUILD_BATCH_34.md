# BUILD_BATCH_34 — Derived atomic explanation through K7

**Main base:** `f1b41b98e5f6d6dbae6435506e2a58677d04a459`.

**Core implementation under observation:** `fe39adbb0e7952536baccec5eecf32a2b1cc4eaa`; this batch adds a focused integration test and retained trace, with no product-source changes.

**Current gate:** `CORE-G03 = PARTIAL`; no other gate was reassessed.

## Finite fixture and producer → consumer

- Seeded one user-declared two-premise rule: `X is a sensor AND X is calibrated ⇒ X is reliable`, then stored the direct premises `Vela is a sensor` and `Vela is calibrated` in the same scope.
- The typed Core input was `Vela is reliable`, purpose `explain`. The test confirmed no direct `USER_PREMISE` fact for that conclusion existed. Core resolved a derived proof containing the derived fact ID, both direct premise IDs, and the rule ID.
- `Engine::compose_atomic_explanation` sealed those four proof references into `CORE_TYPED_ATOMIC_MEANING_V1`, ran the existing `AskWhy` computation, adopted a `DERIVED` `ContentPlan`, and returned a K7 `RENDERED` observation. Result and plan dependencies retained the proof IDs.
- The actual generated units identify the conclusion, both direct premises, the derived fact with its dependency IDs, the applied rule with its rule ID, and the limit that outside-world truth was not checked. The exact output and all artifact IDs are retained in [the B34 runtime observation](b34-derived-atomic-explanation-observation.json).
- The render was 1,310 bytes with SHA-256 `09918d063c451ec988bcb6c1822bec0dd660004f957ac8a3a3b7c482530a7d28`; rendered UTF-8 text matched the adopted plan.

## Negative and permission observations

- Unsupported `Vela is conductive` returned `SUSPEND / MISSING_PREMISES`, with no content plan or render.
- Requesting `derived-unauthorized-output.txt` with zero write grants produced a render and then `SUSPEND / WRITE_PERMISSION_MISSING`. No target file, receipt, or action record was created.
- The isolated temporary database directory was removed at test teardown.

## Bounded checks and judgment

- `cargo fmt --all --check`: **PASS**.
- `cargo test --locked -p minidora-engine --test core_atomic_explanation_34 -- --nocapture`: **PASS**, 1 passed / 0 failed, `rustc 1.99.1 (58cf8f9c3 2026-10-13)`, Linux.
- `b34-derived-atomic-explanation-observation.json`: **12,305 bytes**, SHA-256 `bb84b96c7b49420a45d21777d519f13809740a73ae43062968ac0068ef88abc0`.
- Full workspace tests, broad explanation/content-quality acceptance, general first-seen meaning coverage, ordinary CLI/HTTP exposure, and remaining `CORE-G03` predicates: **NOT_RUN**.

This derived case demonstrates more than the direct-premise B33 fixture: the adopted explanation contains the actual multi-premise proof and applied rule, and the same dependencies reach the render. It remains one bounded fixture based on declared rules and does not establish independent explanation quality, all six language-model obligations, or the full `CORE-G03` population. Keep `CORE-G03 = PARTIAL`.
