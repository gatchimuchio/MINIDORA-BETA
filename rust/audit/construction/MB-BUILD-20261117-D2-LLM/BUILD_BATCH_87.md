# BUILD_BATCH_87 — Saved ExplainPrevious Recovery rejects an unbound current-format prior result

**BASE HEAD:** 04bc2473b6821fa90954b39d0f8b33598863c73d (main = origin/main, clean at batch start). **Implementation commit:** e9a7a7cd12853cb7aacc3acf485ef73dd3a22e76 (Bind saved prior results to sealed inputs). **Version/schema:** 0.7.0-alpha.11 / minidora-rust/0.7.0-alpha.11. **実行日:** 2026-11-18 UTC. **Scope:** E1-W10 / CAP-D02 saved ExplainPrevious prior-result input binding. **Source diff SHA-256:** f20717069b0951d11d8492ff93a106e8ff2a7760d0d4fa5f7f89bd44617e702c.

## 実装・観測範囲

B87は保存済みAskPreviousRelation→ExplainPreviousのRecovery経路で、説明元のResultRecord自体が封印入力へ結び付かない場合の通過を修正した。通常の日本語Engine入力で直接質問→AskPreviousRelation→ExplainPreviousを保存し、Engine再起動後に有効なRecovery inspectが成功することを確認した。

負例の初回検査は失敗し、説明元AskPreviousRelation ResultRecordのinput_idを未存在IDへ変えたCaseRevision cloneでもinspectがOkを返す問題を再現した。原因はverify_previous_relation_contextが sealed input lookupの欠落を旧形式の任意fallbackへ流し、actor_memory_revisionを持つ現行結果の入力同一性を要求していなかったこと。

既存Recovery consumerへ局所ガードを追加した。取得できたprior bundleはidentity(request)==previous.idを要求し、bundleがなくactor_memory_revisionがある結果はRECOVERY_CONTEXT_PROOF_CHANGEDで停止する。actor_memory_revisionがない旧結果のmissing-input fallbackは維持した。修正後の同一テストとB81-B87 filterが通過。負例はcloneのみを改変し、保存済みlatest revisionとtarget bytesを保持する。永続DB/Storeの破損・修復は行っていない。

| 因果位置 | 実値・consumer |
| --- | --- |
| Producer | actor_memory_revisionを持つ保存済みAskPreviousRelation ResultRecordと、その sealed RequestEnvelope |
| Consumer | verify_previous_relation_context → Store::get_input_bundle → identity(request)==previous.id の照合 → ExplainPrevious Recovery |
| 欠陥 | 入力bundleが見つからない場合、既存コードが検査を飛ばして旧形式fallbackへ進み、正常扱いした |
| 正常 | Engine再起動後、正しいAskPreviousRelation→ExplainPreviousの保存状態はRecovery inspectを通る |
| 負例 | ResultRecord.id/map key/lifecycle/input_versionを保持し、input_idだけを未存在値へ変更。現行形式markerありなのでRECOVERY_CONTEXT_PROOF_CHANGED |
| 保持 | persisted latest revisionと既存出力bytesは不変 |

## 検証

Linux x86_64 / kernel 6.18.57、Rust 1.99.1 (58cf8f9c3 2026-10-13)、Cargo 1.99.1 (5f94df478 2026-08-27)。共有target cacheは/workspace/.minidora-cloud/rust-target。rust/Cargo.toml SHA-256 3a54553a0c6d15836b00cf556a821bb011da7295bc11c15ad2ad9b0d25ae8785、Cargo.lock SHA-256 b4e97fecdca8fd1fc863349b1a48eb99a925da7fed0ed6ce6cfe79bdd1cececc、rust-toolchain.toml SHA-256 d13306be0ecda81ce779baa290ffb417380d525928477f16200884b5169cca33。

| Command | 結果 | 保存log |
| --- | --- | --- |
| exact targeted test, initial run | FAIL reproduced: malformed current-format prior result was accepted; 0 passed, 1 failed | [initial failure](b87-initial-missing-prior-input-gap.log) |
| exact targeted test, after guard | PASS: 1 passed, 0 failed, 280 filtered out | [focused test](b87-missing-prior-result-input-test.log) |
| cargo test -p minidora-engine --lib saved_previous_relation_explanation_ --locked -- --nocapture | PASS: 7 passed, 0 failed, 274 filtered out | [Recovery filter](b87-anchor-recovery-filter.log) |
| cargo check --workspace --all-targets --locked | PASS: compile check only; no tests executed | [workspace check](b87-workspace-check.log) |
| cargo clippy -p minidora-engine --lib --locked -- -D warnings | PASS | [Engine library Clippy](b87-engine-library-clippy.log) |
| cargo fmt --all -- --check | PASS | [format check](b87-fmt-check.log) |
| git diff --check | PASS | [diff check](b87-diff-check.log) |

## 未実施・判定

revision-less legacy fallbackの実行試験、full Engine/workspace runtime suite、workspace all-target Clippy、workspace all-features test/build、CLI/HTTP/browser、永続Store/SQLite破損とrepair、長履歴、全Recovery consumer、全CAP-D02/E1受入、他OS実機、独立QA、E2-E8、release/deploymentは未実施。E1-W10はPARTIAL、CAP-D02はUNVERIFIED。E1 exit、release、deploymentを主張しない。

着工時復旧参照：main/origin/main/local codex/backup-main branch/tag = 04bc2473b6821fa90954b39d0f8b33598863c73d。local codex/backup-main-prev branch/tagと同名remote tag = 5c826754a16a4d4713634c80b1865bf214da80e2。remote backup branchesは既存値8c425bf28af09b0284101836da598d53fb8d6756 / e9a275591d9f062d033c07d2a9a4e239414269a6を保持。rollback refは着工時main。

## B87資産の5分類

B87で生成・更新した現存資産は13件で、5区分合計も13件：

- **保持: 2件** — 既存IMPLEMENTATION_STATUSとREQUIREMENT_TRACEをその場で更新し保持。
- **保管済み: 9件** — BUILD_BATCH、machine observation、初回不成立log、修正版test/filter/check/Clippy/format/diffのlog。
- **稼働中: 2件** — recovery.rsのRecovery guard＋回帰テスト、共有Cargo target cache。
- **ゴミ箱移動済み: 0件。** ゴミ箱を空にせず、不要cache削除なし。
- **保留: 0件。** fixtureの一時DB/targetは自動cleanupされ、repo内に残留なし。

Windows保管skill・OneDrive操作はLinux実行では非該当。Git管理物はGitHub mainと遠隔バックアップタグへ保存し、OneDriveへ重複保存しない。機械可読記録は[b87 machine observation](b87-build-observation.json)を参照。
