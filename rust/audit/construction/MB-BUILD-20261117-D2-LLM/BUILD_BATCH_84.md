# BUILD_BATCH_84 — Saved AskPreviousRelation Recovery binds the anchor to its sealed input

**BASE HEAD:** `39b4ddcfc00557dee1edfa073f48ab284ca6644b` (`main` = `origin/main`, clean at batch start). **Implementation commit:** `e34f26016ef512cb7a729f4ac87d3d98cf7d5cb4` (`Bind previous recovery anchors to sealed input`). **Version/schema:** `0.7.0-alpha.11` / `minidora-rust/0.7.0-alpha.11`. **実行日:** 2026-11-19 Asia/Tokyo. **Scope:** E1-W10 / CAP-D02 bounded saved-result Recovery identity guard. **Source diff SHA-256:** `b6af3c3b63cf336a2221b137d6ad986ecebb0927581bff772ce40ac98a7d4275`.

## 実装・観測範囲

通常のEngine日本語入力から前提、明示質問「ココは鳥ですか」、`AskPreviousRelation`「なぜそれは鳥ですか」、`ExplainPrevious`「詳しく説明して」を通し、許可された一時fixture出力先へ保存する。Engine再起動後、Store-backedな保存状態の`inspect`は成功する。

その後`CaseRevision`のcloneでdirect anchorのmap key、`ResultRecord.id`、`Lifecycle::Commit`を維持し、`input_id`だけを封印済み入力に結び付かない値へ変更すると、施工前はRecoveryが`Ok`と確定済みreceiptを返すことを新規回帰テストで確認した。欠けていた検査として、Recoveryがanchorの`input_id/input_version`から`Store::get_input_bundle(actor, case, ...)`で封印済み入力を再取得し、requestのactor/caseと`identity(request)`がanchor IDに一致することを要求する。結び付かないcloneは`RECOVERY_CONTEXT_PROOF_CHANGED`で停止する。停止後もfixture出力byte列とStoreの最新保存revisionは不変。

負例はRecoveryへ渡すcloneだけを変更し、SQLite/Storeの永続状態には触れていない。これは保存DBの物理破損、blob修復、他UI通常経路、長履歴全体の性能保証を示さない。

| 因果位置 | 実値・consumer |
| --- | --- |
| Producer | 封印済み`InputBundle.meaning.context_result_ref`に記録された直前の結果IDと、その結果の`input_id/input_version` |
| Consumer | `verify_saved_previous_relation_query_context`がStoreから同一actor/caseの封印入力を取得し、結果IDとの同一性を検査 |
| 正常 | 再起動後の有効な保存済み`AskPreviousRelation`→`ExplainPrevious`をRecoveryが再確認 |
| 負例 | map key・結果ID・COMMITを保ったままanchor `input_id`だけを未登録値へ変えると停止。Store revisionと出力byte列を保持 |

## 検証

Linux x86_64 / kernel `6.18.57`、Rust `1.99.1 (58cf8f9c3 2026-10-13)`、Cargo `1.99.1 (5f94df478 2026-08-27)`。共有target cacheは`/workspace/.minidora-cloud/rust-target`。`rust/Cargo.toml` SHA-256 `3a54553a0c6d15836b00cf556a821bb011da7295bc11c15ad2ad9b0d25ae8785`、`Cargo.lock` SHA-256 `b4e97fecdca8fd1fc863349b1a48eb99a925da7fed0ed6ce6cfe79bdd1cececc`、`rust-toolchain.toml` SHA-256 `d13306be0ecda81ce779baa290ffb417380d525928477f16200884b5169cca33`。

| Command | 結果 | 保存log |
| --- | --- | --- |
| `cargo test -p minidora-engine --lib recovery::tests::saved_previous_relation_explanation_rejects_unbound_anchor_input --locked -- --exact --nocapture`（ガード追加前） | 欠落を再現: cloneの不正anchorに対して`inspect`が`Ok`/confirmed receiptを返し、期待した拒否がなくテスト失敗 | [初期欠落再現](b84-unbound-anchor-initial-failure.log) |
| 同上（ガード追加後） | PASS: 1 passed, 0 failed, 277 filtered out | [focused test](b84-unbound-anchor-test.log) |
| `cargo test -p minidora-engine --lib saved_previous_relation_explanation_ --locked -- --nocapture` | PASS: 4 passed, 0 failed, 274 filtered out | [Recovery filter](b84-saved-prior-relation-recovery-filter.log) |
| `cargo check --workspace --all-targets --locked` | PASS: compile check only; testsは実行しない | [workspace check](b84-workspace-check.log) |
| `cargo clippy -p minidora-engine --lib --locked -- -D warnings` | PASS | [engine library Clippy](b84-engine-library-clippy.log) |
| `cargo fmt --all -- --check` | PASS | [format check](b84-fmt-check.log) |
| `git diff --check` | PASS | [diff check](b84-diff-check.log) |

## 未実施・判定

full Engine/workspace runtime suite、workspace all-target Clippy、CLI/HTTP/browser通常入口、永続Store/SQLite物理破損とrepair、長履歴の全consumer実行、全source意味閉包、全CAP-D02/E1受入、他OS実機、独立QA、E2-E8、release/deploymentは未実施。E1-W10は`PARTIAL`、CAP-D02は`UNVERIFIED`。E1 exit/release/deploymentを主張しない。

着工時`main`/`origin/main`と`codex/backup-main`のlocal branch/tagは`39b4ddcfc00557dee1edfa073f48ab284ca6644b`、`codex/backup-main-prev`のlocal branch/tagは`cd736e58d6702a64621adfa39e49dcdaa227ee91`。同名remote tagsもこれらの値。remote backup branchesは既存の`8c425bf28af09b0284101836da598d53fb8d6756` / `e9a275591d9f062d033c07d2a9a4e239414269a6`を保持。rollback refは着工時main `39b4ddcfc00557dee1edfa073f48ab284ca6644b`。

## ローカル資産の5分類

- **保持:** D2正本、既存Git履歴、凍結βと既存user assetsに変更なし。
- **保管済み:** B84 report、machine observation、初期欠落再現/focused test/Recovery filter/workspace check/Clippy/format/diffログ（計9記録ファイル）。
- **稼働中:** [`recovery.rs`](/workspace/MINIDORA-BETA/rust/crates/minidora-engine/src/recovery.rs)のRecovery anchor input binding guardと回帰テスト、既存shared Cargo target cache（test/check/Clippyで更新）。
- **ゴミ箱移動済み:** 0 files。手動削除・cache cleanupなし。
- **保留:** 0 local assets。fixtureが一時DBと出力targetを片付け、残留fixture pathなし。Windows storage handlingはLinux実行では非該当。

機械可読な詳細は[B84 machine observation](b84-build-observation.json)を参照。
