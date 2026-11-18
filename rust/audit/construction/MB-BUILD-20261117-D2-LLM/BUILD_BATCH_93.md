# BUILD_BATCH_93 — ExplainPrevious Recovery rejects a missing actor-memory snapshot

**BASE HEAD:** 0da702f24e2243ffd88702573013dc596140572f (main = origin/main; clean at batch start). **Source commit:** 7a6d9d48edc844e6a0a4c5cd6820ba87be5df6c (Test Recovery for missing actor memory revision). **Version/schema:** 0.7.0-alpha.11 / minidora-rust/0.7.0-alpha.11. **Observed date:** 2026-11-18 UTC. **Scope:** E1-W10, saved AskPreviousRelation actor-memory snapshot binding for ExplainPrevious Recovery. **Source diff SHA-256:** ee2103ccf26e11d9bb92e57dc4d0cfc2e23fe5ad8caa95113720ffb8dbf6b591.

## 実装・観測範囲

B93 adds one Engine-library clone-only regression. The ordinary Japanese Engine route commits a premise, a direct question, AskPreviousRelation and ExplainPrevious. After reopening Engine, valid saved-state Recovery inspect passes, and the actor-memory revision recorded by AskPreviousRelation is confirmed to have a retained snapshot.

The test selects the next actor-memory revision after the current head and verifies that no snapshot exists there. It changes only the prior AskPreviousRelation result's actor_memory_revision in a cloned CaseRevision, preserving its result ID, sealed input ID/version and exact input identity. ExplainPrevious Recovery refuses the clone with RECOVERY_CONTEXT_PROOF_CHANGED wrapping RECOVERY_MEMORY_REVISION_MISSING. The persisted case revision, actor-memory head and output bytes remain unchanged. The malformed clone is never stored.

No production behavior, schema, dependency, manifest or lockfile changed. This does not test persistent Store corruption or historical SQLite/schema migration.

| 因果位置 | 実値・consumer |
| --- | --- |
| Producer | Saved AskPreviousRelation ResultRecord with an existing actor-memory revision and exact sealed input bundle. |
| Consumer | ExplainPrevious Recovery -> verify_previous_relation_context -> verify_saved_previous_relation_query_context -> result_memory_snapshot -> exact Store revision lookup. |
| Observed boundary | Original pin and saved result pass; clone pointer to a verified absent revision stops with RECOVERY_MEMORY_REVISION_MISSING under RECOVERY_CONTEXT_PROOF_CHANGED. |
| Persistence | The fixture remains at its original persisted case and memory head; only the cloned CaseRevision pointer changes; target bytes remain identical. |

## 検証

Linux x86_64 / kernel 6.18.57, Rust 1.99.1 (58cf8f9c3 2026-10-13), Cargo 1.99.1 (5f94df478 2026-08-27). Shared target cache: /workspace/.minidora-cloud/rust-target. rust/Cargo.toml SHA-256 3a54553a0c6d15836b00cf556a821bb011da7295bc11c15ad2ad9b0d25ae8785; rust/Cargo.lock SHA-256 b4e97fecdca8fd1fc863349b1a48eb99a925da7fed0ed6ce6cfe79bdd1cececc; rust/rust-toolchain.toml SHA-256 d13306be0ecda81ce779baa290ffb417380d525928477f16200884b5169cca33.

| Command | 結果 | 保存log |
| --- | --- | --- |
| cargo test --manifest-path rust/Cargo.toml -p minidora-engine --lib recovery::tests::saved_previous_relation_explanation_rejects_missing_actor_memory_snapshot_revision --locked -- --exact --nocapture | PASS: 1 passed, 0 failed, 286 filtered out; 1.81s. | [focused test](b93-missing-memory-revision-test.log) |
| cargo test --manifest-path rust/Cargo.toml -p minidora-engine --lib saved_previous_relation_explanation_ --locked -- --nocapture | PASS: 13 passed, 0 failed, 274 filtered out; 6.65s. | [Recovery filter](b93-explain-previous-recovery-filter.log) |
| cargo fmt --manifest-path rust/Cargo.toml --all -- --check | PASS | [format check](b93-fmt-check.log) |
| git diff --check | PASS | [diff check](b93-diff-check.log) |

## 未実施・判定

Actual historical Rust-schema ResultRecord/SQLite reads, persistent Store corruption/repair, schema migration, full Engine/workspace runtime suites, workspace all-target Clippy, workspace all-features test/build, CLI/HTTP/browser, long-history replay, every Recovery consumer, complete CAP-D02/E1 acceptance, other operating systems, independent QA, E2-E8, release and deployment were not run. B93 validates the missing-retained-snapshot guard in one synthetic clone. E1-W10 remains PARTIAL; CAP-D02 remains UNVERIFIED; no E1 exit, release or deployment is claimed.

Batch-start restoration refs: main/origin/main/local codex/backup-main branch and tag = 0da702f24e2243ffd88702573013dc596140572f. Local codex/backup-main-prev branch/tag and remote tag = 0ec4b66b9e304d2c4edbeb3d0a5e772a3d6aab17. Remote backup branches remain unchanged at codex/backup-main=8c425bf28af09b0284101836da598d53fb8d6756 and codex/backup-main-prev=e9a275591d9f062d033c07d2a9a4e239414269a6. The source commit is on main; the evidence commit and remote fast-forward are recorded by Git refs after batch close.

## B93資産の5分類

B93で生成・更新した現存資産は10件で、5区分の合計も10件：

- **保持: 2件** — existing IMPLEMENTATION_STATUS and REQUIREMENT_TRACE updated in place.
- **保管済み: 6件** — BUILD_BATCH report, machine observation, focused test, Recovery filter, format and diff logs.
- **稼働中: 2件** — active recovery.rs regression test and existing shared Cargo target cache.
- **ゴミ箱移動済み: 0件。** No cache cleanup.
- **保留: 0件。** Fixture database/output are owned and cleaned by the fixture; no fixture artifact remains in the repository.

Windows storage skill and OneDrive operations do not apply to this Linux execution. Git-managed records are retained on GitHub main and backup tags; no duplicate OneDrive copy is made. Machine-readable evidence is in [b93 machine observation](b93-build-observation.json).
