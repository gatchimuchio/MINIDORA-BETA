# BUILD_BATCH_91 — Exact revision-less input binding still stops without the saved memory revision

**BASE HEAD:** ad0e7b8d531f88b4a1140e53de57a77ecc1ed36f (main = origin/main; clean at batch start). **Implementation commit:** 40724df99f190ab5baa65e5ae45d558aad5313cb (Record revisionless Recovery memory revision boundary). **Version/schema:** 0.7.0-alpha.11 / minidora-rust/0.7.0-alpha.11. **実行日:** 2026-11-19 JST. **Scope:** E1-W10, revision-less saved AskPreviousRelation exact input binding and memory-revision replay boundary. **Source diff SHA-256:** cafb643a59d0dde876a59e7a286d81a3f057c3f35ea35bb2f31f1c1cd18974d6.

## 実装・観測範囲

B91 adds one Engine-library clone-only regression at the sealed-input identity/replay boundary. The ordinary Japanese Engine path commits a premise, direct question, AskPreviousRelation and ExplainPrevious. After Engine reopen, the unmodified saved Recovery inspect passes.

In a cloned CaseRevision only, the test clears `actor_memory_revision`, leaving `input_id`, `input_version`, `ResultRecord.id`, lifecycle and map key unchanged. It loads the same actor/case/input_id/version bundle and confirms `identity(request) == ResultRecord.id`. ExplainPrevious Recovery inspect then stops with `RECOVERY_CONTEXT_PROOF_CHANGED`, wrapping `RECOVERY_MEMORY_REVISION_MISSING`, because the saved prior relation answer cannot be replayed without its retained actor-memory revision. Store latest revision and output bytes remain unchanged.

The initial acceptance probe expected the exact-bound revision-less result to pass; two attempts showed the missing-memory-revision refusal. The final regression records that fail-closed behavior. B88's fallback applies to the distinct missing-input case; B89 and B90 reject existing but misbound input IDs/versions. This is not a historical SQLite/schema migration test. No production behavior, schema, dependency, manifest or lockfile changed.

| 因果位置 | 実値・consumer |
| --- | --- |
| Producer | Saved AskPreviousRelation ResultRecord with an exact existing sealed input bundle. Clone only clears `actor_memory_revision`; all input/result identity fields remain unchanged. |
| Consumer | `verify_previous_relation_context` → `Store::get_input_bundle(actor,case,input_id,input_version)` → identity match → ExplainPrevious relation replay → missing retained actor-memory revision rejection. |
| Observed boundary | Exact input identity passes; relation replay returns `RECOVERY_MEMORY_REVISION_MISSING` under outer `RECOVERY_CONTEXT_PROOF_CHANGED`. |
| Persistence | Persisted Store latest revision and target bytes remain unchanged; only the cloned CaseRevision is modified. |

## 検証

Linux x86_64 / kernel 6.18.57, Rust 1.99.1 (58cf8f9c3 2026-10-13), Cargo 1.99.1 (5f94df478 2026-08-27). Shared target cache: `/workspace/.minidora-cloud/rust-target`. `rust/Cargo.toml` SHA-256 `3a54553a0c6d15836b00cf556a821bb011da7295bc11c15ad2ad9b0d25ae8785`; `rust/Cargo.lock` SHA-256 `b4e97fecdca8fd1fc863349b1a48eb99a925da7fed0ed6ce6cfe79bdd1cececc`; `rust/rust-toolchain.toml` SHA-256 `d13306be0ecda81ce779baa290ffb417380d525928477f16200884b5169cca33`.

| Command | 結果 | 保存log |
| --- | --- | --- |
| `cargo test --manifest-path rust/Cargo.toml -p minidora-engine --lib recovery::tests::saved_previous_relation_explanation_accepts_revisionless_result_with_exact_sealed_input --locked -- --exact --nocapture` | Initial acceptance assertion failed. Diagnostic rerun identified `RECOVERY_CONTEXT_PROOF_CHANGED` / `RECOVERY_MEMORY_REVISION_MISSING`. Both attempts are retained in the focused log; the test was then changed to assert the observed refusal. | [focused test and initial probes](b91-revisionless-exact-input-test.log) |
| `cargo test --manifest-path rust/Cargo.toml -p minidora-engine --lib recovery::tests::saved_previous_relation_explanation_rejects_revisionless_exact_input_without_memory_revision --locked -- --exact --nocapture` | PASS: 1 passed, 0 failed, 284 filtered out. | [focused test and initial probes](b91-revisionless-exact-input-test.log) |
| `cargo test --manifest-path rust/Cargo.toml -p minidora-engine --lib saved_previous_relation_explanation_ --locked -- --nocapture` | PASS: 11 passed, 0 failed, 274 filtered out; 5.17s. | [Recovery filter](b91-explain-previous-recovery-filter.log) |
| `cargo fmt --manifest-path rust/Cargo.toml --all -- --check` | PASS | [format check](b91-fmt-check.log) |
| `git diff --check` | PASS | [diff check](b91-diff-check.log) |

## 未実施・判定

Actual historical Rust-schema ResultRecord/SQLite reads, schema migration, persistent Store corruption/repair, full Engine/workspace runtime suites, workspace all-target Clippy, workspace all-features test/build, CLI/HTTP/browser, long-history replay, every Recovery consumer, complete CAP-D02/E1 acceptance, other operating systems, independent QA, E2-E8, release and deployment were not run. B91 verifies one synthetic revision-less clone with a correctly bound sealed input but no actor-memory revision. E1-W10 remains PARTIAL and CAP-D02 remains UNVERIFIED; no E1 exit, release or deployment is claimed.

Batch-start restoration refs: main/origin/main/local `codex/backup-main` branch and tag = ad0e7b8d531f88b4a1140e53de57a77ecc1ed36f. Local `codex/backup-main-prev` branch/tag and remote tag = d7d9577449f699f9f2d9545b3f6dad82f5839755. Remote backup branches are retained unchanged at `codex/backup-main=8c425bf28af09b0284101836da598d53fb8d6756` and `codex/backup-main-prev=e9a275591d9f062d033c07d2a9a4e239414269a6`. Rollback ref is batch-start main.

## B91資産の5分類

B91で生成・更新した現存資産は10件で、5区分の合計も10件：

- **保持: 2件** — existing IMPLEMENTATION_STATUS and REQUIREMENT_TRACE updated in place.
- **保管済み: 6件** — BUILD_BATCH report, machine observation, focused probes/test, filter, format and diff logs.
- **稼働中: 2件** — active `recovery.rs` regression test and existing shared Cargo target cache.
- **ゴミ箱移動済み: 0件。** No cache cleanup.
- **保留: 0件。** Fixture database/output are owned and cleaned by the fixture; no fixture artifact remains in the repository.

Windows storage skill and OneDrive operations do not apply to this Linux execution. Git-managed records are retained in GitHub main and backup tags; no duplicate OneDrive copy is made. Machine-readable evidence is in [b91 machine observation](b91-build-observation.json).
