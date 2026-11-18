# BUILD_BATCH_89 — Revision-less prior-result Recovery rejects an existing misbound input

**BASE HEAD:** 41251a94fb27716486438e923d3dc69e6d3b8676 (main = origin/main; clean at batch start). **Implementation commit:** a4b50f158de98dd336c8a7ec177474744f10c231 (Reject misbound revisionless prior results). **Version/schema:** 0.7.0-alpha.11 / minidora-rust/0.7.0-alpha.11. **実行日:** 2026-11-19 JST. **Scope:** E1-W10, revision-less saved AskPreviousRelation prior-result input binding. **Source diff SHA-256:** 91adbbbff5c65fb68f6b6fba9319b3cc6e8d772e25f4d2bb689fb026361bf6c1.

## 実装・観測範囲

B89はrevision-less prior resultにsealed input bundleが存在する場合のidentity不一致を固定するEngine-library回帰テストを追加した。通常の日本語Engine経路で前提、直接質問、AskPreviousRelation、ExplainPreviousを保存し、Engine再起動後の正常Recovery inspectが成功することを先に確認する。

負例では保存済みCaseRevisionのclone内だけで、prior AskPreviousRelation ResultRecordのactor_memory_revisionをNoneにし、input_idを同じcase・同じversionに保存された別の直接質問へ差し替える。参照先bundleが実在し、identity(request) != ResultRecord.idであることを確認した後、ExplainPrevious Recovery inspectはRECOVERY_CONTEXT_PROOF_CHANGEDで停止する。Store latest revisionと出力bytesは変わらない。

B88で確認した「revision-lessかつinput bundleがない場合のfallback」は維持する。B89はexisting-but-misbound inputを拒否するだけのテスト追加で、本番ロジック・schema・依存は変更していない。fixtureはlegacy-shaped cloneであり、実際の旧SQLite/schemaやmigrationを検査したものではない。

| 因果位置 | 実値・consumer |
| --- | --- |
| Producer | 保存済みAskPreviousRelation ResultRecord。clone内でactor_memory_revision=None、input_idを別質問のsealed inputへ差し替え |
| Consumer | verify_previous_relation_context → Store::get_input_bundle → sealed request identity と prior ResultRecord.id の照合 → ExplainPrevious Recovery inspect |
| 負例 | 同case・同versionの別input bundleは存在するがidentityが異なるためRECOVERY_CONTEXT_PROOF_CHANGED |
| 互換 | B88のrevision-less missing-input fallbackは維持。B87のcurrent-format missing-input拒否も別テストで保持 |
| 保持 | 永続Store latest revisionと既存target bytesは不変。cloneだけを改変 |

## 検証

Linux x86_64 / kernel 6.18.57、Rust 1.99.1 (58cf8f9c3 2026-10-13)、Cargo 1.99.1 (5f94df478 2026-08-27)。共有target cacheは/workspace/.minidora-cloud/rust-target。rust/Cargo.toml SHA-256 3a54553a0c6d15836b00cf556a821bb011da7295bc11c15ad2ad9b0d25ae8785、Cargo.lock SHA-256 b4e97fecdca8fd1fc863349b1a48eb99a925da7fed0ed6ce6cfe79bdd1cececc、rust-toolchain.toml SHA-256 d13306be0ecda81ce779baa290ffb417380d525928477f16200884b5169cca33。

| Command | 結果 | 保存log |
| --- | --- | --- |
| cargo test -p minidora-engine --lib recovery::tests::saved_previous_relation_explanation_rejects_revisionless_result_bound_to_other_input --locked -- --exact --nocapture | PASS: 1 passed, 0 failed, 282 filtered out | [focused test](b89-revisionless-misbound-input-test.log) |
| cargo test -p minidora-engine --lib saved_previous_relation_explanation_ --locked -- --nocapture | PASS: 9 passed, 0 failed, 274 filtered out | [Recovery filter](b89-explain-previous-recovery-filter.log) |
| cargo fmt --all -- --check | PASS | [format check](b89-fmt-check.log) |
| git diff --check | PASS | [diff check](b89-diff-check.log) |

## 未実施・判定

実際に旧Rust schemaで保存されたprior ResultRecord/SQLiteの読込、schema migration、永続Storeへの破損注入・repair、full Engine/workspace runtime suite、workspace all-target Clippy、workspace all-features test/build、CLI/HTTP/browser、長履歴、全Recovery consumer、全CAP-D02/E1受入、他OS、独立QA、E2-E8、release/deploymentは未実施。B89は既存sealed inputとのidentity不一致をsynthetic cloneで検査しただけで、実履歴一般の互換保証ではない。E1-W10はPARTIAL、CAP-D02はUNVERIFIED。E1 exit、release、deploymentを主張しない。

着工時復旧参照：main/origin/main/local codex/backup-main branch/tag = 41251a94fb27716486438e923d3dc69e6d3b8676。local codex/backup-main-prev branch/tagと同名remote tag = bcd74f74df7938eeb6c7f5827526fb4dd7925d17。remote backup branchesは既存値8c425bf28af09b0284101836da598d53fb8d6756 / e9a275591d9f062d033c07d2a9a4e239414269a6を保持。rollback refは着工時main。

## B89資産の5分類

B89で生成・更新した現存資産は10件で、5区分合計も10件：

- **保持: 2件** — 既存IMPLEMENTATION_STATUSとREQUIREMENT_TRACEをその場で更新。
- **保管済み: 6件** — BUILD_BATCH、machine observation、focused test/filter/format/diffのlog。
- **稼働中: 2件** — recovery.rs内のmisbound legacy回帰テスト、共有Cargo target cache。
- **ゴミ箱移動済み: 0件。** 不要cache削除なし。
- **保留: 0件。** fixtureの一時DB/targetはcleanupされ、repo内に残留なし。

Windows保管skill・OneDrive操作はLinux実行では非該当。Git管理物はGitHub mainと遠隔バックアップタグへ保存し、OneDriveへ重複保存しない。機械可読記録は[b89 machine observation](b89-build-observation.json)を参照。
