# BUILD_BATCH_82 — Saved AskPreviousRelation Recovery anchor-integrity negative cases

**BASE HEAD:** `6567cc44503b664ac2cc47b88b2874f4a13ee393` (`main` = `origin/main`, clean before this batch). **Test commit:** `12e9072951ef79c0e3e7c1bf14110e0f937c0ca9` (`Test recovery anchor integrity for previous relations`). **Version/schema:** `0.7.0-alpha.11` / `minidora-rust/0.7.0-alpha.11`. **実行日:** 2026-11-18 Asia/Tokyo. **Scope:** E1-W10 / CAP-D02の限定Recovery境界。**Source diff SHA-256:** `b141d60fe7a21862c59608b0b82df6491e10f43f4c455265ccba39e82221519a`.

## 実装・観測範囲

B80で追加した保存済み型付き`AskPreviousRelation`→`ExplainPrevious` Recovery再検証の、直前ResultRecordアンカー同一性に関する負例を確認する回帰テストを1件追加した。本番ロジック/API/schema/manifest/dependency/lockfileは変更していない。

テストは通常の日本語入力で前提・明示質問・`なぜそれは鳥ですか`・`詳しく説明して`を実行し、説明結果を許可済みfixture出力先へ保存する。Engineを再起動後の正常な`inspect`が通ることを基準に、保存状態から取得した`CaseRevision`のcloneを二通りだけ変化させて同じRecovery consumerに渡す。

1. `prior_results`から保存済みanchorを欠落させると、`RECOVERY_CONTEXT_PROOF_CHANGED`（anchorが保持されていない）で停止する。
2. map keyは維持したまま、その値の`ResultRecord.id`を別IDへ差し替えると、`RECOVERY_CONTEXT_PROOF_CHANGED`（正確なcommit済み結果ではない）で停止する。

各負例の後も既存出力ファイルのbyte列は一致した。この検査はRecoveryへ渡る`CaseRevision` snapshotの不整合を注入する単体境界確認であり、SQLite上の物理破損・DB復旧・CLI/browser経路の試験ではない。CAP-D02やE1全体の受入を示さない。

## 検証

Linux x86_64 / kernel `6.18.57`、Rust `1.99.1 (58cf8f9c3 2026-10-13)`、Cargo `1.99.1 (5f94df478 2026-08-27)`。共有target cacheは`/workspace/.minidora-cloud/rust-target`。`rust/Cargo.toml` SHA-256 `3a54553a0c6d15836b00cf556a821bb011da7295bc11c15ad2ad9b0d25ae8785`、`Cargo.lock` SHA-256 `b4e97fecdca8fd1fc863349b1a48eb99a925da7fed0ed6ce6cfe79bdd1cececc`、`rust-toolchain.toml` SHA-256 `d13306be0ecda81ce779baa290ffb417380d525928477f16200884b5169cca33`。

| Command | 結果 | 保存log |
| --- | --- | --- |
| `cargo test -p minidora-engine --lib recovery::tests::saved_previous_relation_explanation_rejects_missing_or_misbound_anchor --locked -- --exact --nocapture` | PASS: 1 passed, 0 failed, 275 filtered out | [anchor integrity test](b82-anchor-integrity-test.log) |
| `cargo test -p minidora-engine --lib saved_previous_relation_explanation_ --locked -- --nocapture` | PASS: 2 passed, 0 failed, 274 filtered out | [related Recovery filter](b82-saved-prior-relation-recovery-filter.log) |
| `cargo check --workspace --all-targets --locked` | PASS: compile check only | [workspace check](b82-workspace-check.log) |
| `cargo clippy -p minidora-engine --lib --locked -- -D warnings` | PASS | [engine library Clippy](b82-engine-library-clippy.log) |
| `cargo fmt --all -- --check` | PASS | [format check](b82-fmt-check.log) |
| `git diff --check` | PASS | [diff check](b82-diff-check.log) |

全engine/workspace runtime、workspace all-target Clippy、CLI/HTTP/browser、store物理破損復旧、長履歴スケール、他OS、independent QA、完全なE1/CAP-D02受入は実施していない。W10は`PARTIAL`、CAP-D02は`UNVERIFIED`。E1 exit/release/deploymentを主張しない。

今回照合したD2文書SHA-256:

- `01_施工仕様書.md`: `de618da9d50f248a7e984ad807aab4a306f17204eb9e722da387aa7d587573b0`
- `02_施工工程表.md`: `d1afc8a3e225fcbc21ef39eb3152ca9c64ffe79ee61d0702220802700570ce7a`
- `03_Codex施工指示書.md`: `14bcce642e3c6281f4922e1ac788dcef6e1328451aebf82d245922cb9d901d3d`

着工時`main` / `origin/main`は`6567cc44503b664ac2cc47b88b2874f4a13ee393`。local backup refs/tags `codex/backup-main`は同SHA、`codex/backup-main-prev`は`664c4e1cce027ef891953a94585720363ac01e9d`。remote同名tagsもそれぞれ同SHA。remote backup branchesは着工時点で`8c425bf28af09b0284101836da598d53fb8d6756` / `e9a275591d9f062d033c07d2a9a4e239414269a6`のまま。rollback refは確認済みbase `6567cc44503b664ac2cc47b88b2874f4a13ee393`。

## ローカル資産の5分類

- **保持:** D2正本、既存Git履歴、凍結βと既存user assetsに変更なし。
- **保管済み:** B82 report、machine observation、focused test/Recovery filter/workspace check/Clippy/format/diffログ（計8記録ファイル）。
- **稼働中:** [`recovery.rs`](/workspace/MINIDORA-BETA/rust/crates/minidora-engine/src/recovery.rs)のRegression test、既存shared Cargo target cache（test/check/Clippyで更新）。
- **ゴミ箱移動済み:** 0 files。手動削除・cache cleanupなし。
- **保留:** 0 local assets。Engine fixtureはtest-owned cleanupで処理し残留fixture/output pathなし。Windows storage handlingはLinux実行では非該当。

機械可読な詳細は[B82 machine observation](b82-build-observation.json)を参照。
