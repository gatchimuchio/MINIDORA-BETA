# BUILD_BATCH_83 — Saved AskPreviousRelation Recovery rejects an uncommitted anchor

**BASE HEAD:** `cd736e58d6702a64621adfa39e49dcdaa227ee91` (`main` = `origin/main`, clean before this batch). **Test commit:** `9defea69ae2001fefd06a8e93fa86cbf9f4b9370` (`Test recovery rejects uncommitted prior anchors`). **Version/schema:** `0.7.0-alpha.11` / `minidora-rust/0.7.0-alpha.11`. **実行日:** 2026-11-19 Asia/Tokyo. **Scope:** E1-W10 / CAP-D02の限定Recovery停止境界。**Source diff SHA-256:** `d9c4a56b9a5dc09795c3ad430fcc1d35d08f5a3d144f8505a3e6f91b30db751a`.

## 実装・観測範囲

B80の保存済み型付き`AskPreviousRelation`→`ExplainPrevious` Recoveryで、直前アンカーのlifecycleが`COMMIT`以外の場合に停止することを検証する回帰テストを1件追加した。本番ロジック/API/schema/manifest/dependency/lockfileは変更していない。

通常の日本語入力から前提、明示質問、`なぜそれは鳥ですか`、`詳しく説明して`を通し、許可済みfixture出力先に説明を保存する。Engineを再起動後、Storeから得た保存状態の`inspect`は成功する。次に、その状態のcloneでanchorのmap keyと`ResultRecord.id`を維持したままlifecycleだけ`RECEIVED`へ変えると、同じRecovery consumerが`RECOVERY_CONTEXT_PROOF_CHANGED`（正確なcommit済み結果ではない）で停止し、保存済み出力byte列は変わらない。

負例はRecoveryへ渡す`CaseRevision`のclone上だけで作り、Store/SQLiteの永続状態は書き換えていない。このテストはアンカーlifecycle境界の局所実行であり、DB物理破損・自動修復・CLI/browser全経路・CAP-D02全体の受入を示さない。

## 検証

Linux x86_64 / kernel `6.18.57`、Rust `1.99.1 (58cf8f9c3 2026-10-13)`、Cargo `1.99.1 (5f94df478 2026-08-27)`。共有target cacheは`/workspace/.minidora-cloud/rust-target`。`rust/Cargo.toml` SHA-256 `3a54553a0c6d15836b00cf556a821bb011da7295bc11c15ad2ad9b0d25ae8785`、`Cargo.lock` SHA-256 `b4e97fecdca8fd1fc863349b1a48eb99a925da7fed0ed6ce6cfe79bdd1cececc`、`rust-toolchain.toml` SHA-256 `d13306be0ecda81ce779baa290ffb417380d525928477f16200884b5169cca33`。

| Command | 結果 | 保存log |
| --- | --- | --- |
| `cargo test -p minidora-engine --lib recovery::tests::saved_previous_relation_explanation_rejects_uncommitted_anchor --locked -- --exact --nocapture` | PASS: 1 passed, 0 failed, 276 filtered out | [uncommitted anchor test](b83-uncommitted-anchor-test.log) |
| `cargo test -p minidora-engine --lib saved_previous_relation_explanation_ --locked -- --nocapture` | PASS: 3 passed, 0 failed, 274 filtered out | [related Recovery filter](b83-saved-prior-relation-recovery-filter.log) |
| `cargo check --workspace --all-targets --locked` | PASS: compile check only | [workspace check](b83-workspace-check.log) |
| `cargo clippy -p minidora-engine --lib --locked -- -D warnings` | PASS | [engine library Clippy](b83-engine-library-clippy.log) |
| `cargo fmt --all -- --check` | PASS | [format check](b83-fmt-check.log) |
| `git diff --check` | PASS | [diff check](b83-diff-check.log) |

全engine/workspace runtime、workspace all-target Clippy、CLI/HTTP/browser、Store物理破損復旧、長履歴スケール、他OS、independent QA、完全なE1/CAP-D02受入は実施していない。W10は`PARTIAL`、CAP-D02は`UNVERIFIED`。E1 exit/release/deploymentを主張しない。

今回照合したD2文書SHA-256:

- `01_施工仕様書.md`: `de618da9d50f248a7e984ad807aab4a306f17204eb9e722da387aa7d587573b0`
- `02_施工工程表.md`: `d1afc8a3e225fcbc21ef39eb3152ca9c64ffe79ee61d0702220802700570ce7a`
- `03_Codex施工指示書.md`: `14bcce642e3c6281f4922e1ac788dcef6e1328451aebf82d245922cb9d901d3d`

着工時`main` / `origin/main`は`cd736e58d6702a64621adfa39e49dcdaa227ee91`。local backup refs/tags `codex/backup-main`は同SHA、`codex/backup-main-prev`は`6567cc44503b664ac2cc47b88b2874f4a13ee393`。remote同名tagsもそれぞれ同SHA。remote backup branchesは着工時点で`8c425bf28af09b0284101836da598d53fb8d6756` / `e9a275591d9f062d033c07d2a9a4e239414269a6`のまま。rollback refは確認済みbase `cd736e58d6702a64621adfa39e49dcdaa227ee91`。

## ローカル資産の5分類

- **保持:** D2正本、既存Git履歴、凍結βと既存user assetsに変更なし。
- **保管済み:** B83 report、machine observation、focused test/Recovery filter/workspace check/Clippy/format/diffログ（計8記録ファイル）。
- **稼働中:** [`recovery.rs`](/workspace/MINIDORA-BETA/rust/crates/minidora-engine/src/recovery.rs)のregression test、既存shared Cargo target cache（test/check/Clippyで更新）。
- **ゴミ箱移動済み:** 0 files。手動削除・cache cleanupなし。
- **保留:** 0 local assets。Engine fixtureが一時DBと出力targetを片付け、残留fixture pathなし。Windows storage handlingはLinux実行では非該当。

機械可読な詳細は[B83 machine observation](b83-build-observation.json)を参照。
