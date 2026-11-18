# BUILD_BATCH_90 — Revision-less prior-result Recovery rejects an existing misbound input version

**BASE HEAD:** d7d9577449f699f9f2d9545b3f6dad82f5839755 (main = origin/main; clean at batch start). **Implementation commit:** fafc6f0858e5fcaa5a76bdd46ec50a0846e6691e (Reject revisionless prior result bound to other input version). **Version/schema:** 0.7.0-alpha.11 / minidora-rust/0.7.0-alpha.11. **実行日:** 2026-11-19 JST. **Scope:** E1-W10, revision-less saved AskPreviousRelation prior-result input-version binding. **Source diff SHA-256:** 5db34464e2f6d212921b1157c04a5960f3516153669e86ab14b80c8d65e128ac.

## 実装・観測範囲

B90 adds one Engine-library regression for a revision-less saved AskPreviousRelation result whose input_id still resolves, but whose input_version is changed to another existing sealed request. The ordinary Japanese Engine route saves the same input_id at version 1 and version 2, then saves ExplainPrevious. After Engine reopen, valid Recovery inspect passes.

In a cloned CaseRevision only, the test clears `actor_memory_revision` and changes the saved version 2 prior result's `input_version` to version 1, preserving `input_id`, `ResultRecord.id`, lifecycle and map key. It confirms the version 1 bundle exists and `identity(request) != ResultRecord.id`; ExplainPrevious Recovery inspect stops with `RECOVERY_CONTEXT_PROOF_CHANGED`. Persisted Store latest revision and target bytes remain unchanged. B88's revision-less missing-input fallback and B89's wrong-existing-input_id case remain covered.

This is a test-only change. Production logic, persisted schema, dependencies, manifests and lockfile are unchanged. The malformed record is a synthetic clone; no historical SQLite/schema record or migration was tested.

| 因果位置 | 実値・consumer |
| --- | --- |
| Producer | Saved AskPreviousRelation ResultRecord for version 2; same input_id also has a different sealed version 1 request. Clone only clears actor_memory_revision and changes input_version from 2 to 1. |
| Consumer | `verify_previous_relation_context` → `Store::get_input_bundle(actor,case,input_id,input_version)` → sealed request identity check → ExplainPrevious Recovery inspect. |
| Negative case | The other-version bundle exists under the same input_id, but its request identity differs from ResultRecord.id; Recovery returns `RECOVERY_CONTEXT_PROOF_CHANGED`. |
| Compatibility | B88 revision-less missing-input fallback and B89 revision-less existing-but-misbound input_id rejection remain covered. |
| Persistence | Store latest revision and existing output bytes remain unchanged; only a cloned CaseRevision is modified. |

## 検証

Linux x86_64 / kernel 6.18.57, Rust 1.99.1 (58cf8f9c3 2026-10-13), Cargo 1.99.1 (5f94df478 2026-08-27). Shared target cache: `/workspace/.minidora-cloud/rust-target`. `rust/Cargo.toml` SHA-256 `3a54553a0c6d15836b00cf556a821bb011da7295bc11c15ad2ad9b0d25ae8785`; `rust/Cargo.lock` SHA-256 `b4e97fecdca8fd1fc863349b1a48eb99a925da7fed0ed6ce6cfe79bdd1cececc`; `rust/rust-toolchain.toml` SHA-256 `d13306be0ecda81ce779baa290ffb417380d525928477f16200884b5169cca33`.

| Command | 結果 | 保存log |
| --- | --- | --- |
| `cargo test --manifest-path rust/Cargo.toml -p minidora-engine --lib recovery::tests::saved_previous_relation_explanation_rejects_revisionless_result_bound_to_other_input_version --locked -- --exact --nocapture` | First invocation could not find system linker `cc` because the supplied PATH omitted `/usr/bin`; rerun with `/usr/bin:/bin` passed: 1 passed, 0 failed, 283 filtered out. | [focused test](b90-revisionless-wrong-input-version-test.log) |
| `cargo test --manifest-path rust/Cargo.toml -p minidora-engine --lib saved_previous_relation_explanation_ --locked -- --nocapture` | PASS: 10 passed, 0 failed, 274 filtered out; 4.19s. | [Recovery filter](b90-explain-previous-recovery-filter.log) |
| `cargo fmt --manifest-path rust/Cargo.toml --all -- --check` | PASS | [format check](b90-fmt-check.log) |
| `git diff --check` | PASS | [diff check](b90-diff-check.log) |

## 未実施・判定

Actual historical Rust-schema ResultRecord/SQLite reads, schema migration, persistent Store corruption/repair, full Engine/workspace runtime suites, workspace all-target Clippy, workspace all-features test/build, CLI/HTTP/browser, long-history replay, every Recovery consumer, all CAP-D02/E1 acceptance, other operating systems, independent QA, E2-E8, release and deployment were not run. This batch verifies one synthetic revision-less same-input_id/wrong-version clone only. E1-W10 remains PARTIAL, CAP-D02 remains UNVERIFIED; no E1 exit, release or deployment is claimed.

Batch-start restoration refs: main/origin/main/local `codex/backup-main` branch and tag = d7d9577449f699f9f2d9545b3f6dad82f5839755. Local `codex/backup-main-prev` branch/tag and remote tag = 41251a94fb27716486438e923d3dc69e6d3b8676. Remote backup branches are retained unchanged at `codex/backup-main=8c425bf28af09b0284101836da598d53fb8d6756` and `codex/backup-main-prev=e9a275591d9f062d033c07d2a9a4e239414269a6`. Rollback ref is batch-start main.

## B90資産の5分類

B90で生成・更新した現存資産は10件で、5区分の合計も10件：

- **保持: 2件** — existing IMPLEMENTATION_STATUS and REQUIREMENT_TRACE updated in place.
- **保管済み: 6件** — BUILD_BATCH report, machine observation, focused test/filter/format/diff logs.
- **稼働中: 2件** — active `recovery.rs` regression test and existing shared Cargo target cache.
- **ゴミ箱移動済み: 0件。** No cache cleanup.
- **保留: 0件。** Fixture database/output are owned and cleaned by the fixture; no fixture artifact remains in the repository.

Windows storage skill and OneDrive operations do not apply to this Linux execution. Git-managed records are retained in GitHub main and backup tags; no duplicate OneDrive copy is made. Machine-readable evidence is in [b90 machine observation](b90-build-observation.json).
