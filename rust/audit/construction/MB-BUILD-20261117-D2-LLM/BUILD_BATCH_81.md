# BUILD_BATCH_81 — Saved AskPreviousRelation explanation Recovery runtime regression

**BASE HEAD:** `664c4e1cce027ef891953a94585720363ac01e9d` (`main` = `origin/main`, clean before this batch). **Test commit:** `70411f2881a245c1dbbfd950e41cc15a072515c9` (`Test previous relation explanation recovery`). **Version/schema:** `0.7.0-alpha.11` / `minidora-rust/0.7.0-alpha.11`. **実行日:** 2026-11-18 Asia/Tokyo. **Scope:** E1-W10 / CAP-D02の限定ランタイム確認。**Source diff SHA-256:** `d025050a55e733cac3c02f2431b82560ec8c2042c1cbd3474ee4fbac7e5586f0`.

## 実装・観測範囲

B80で追加された保存済み型付き`AskPreviousRelation`から`ExplainPrevious`へのRecovery再検証を、1件のEngine library回帰テストで実行確認した。変更は`recovery.rs`のテストのみで、本番ロジック/API/schema/manifest/lockfileは変更していない。

- 通常の日本語入力で前提を保存し、直前の関係回答を参照する`なぜそれは鳥ですか`が型付き`AskPreviousRelation`としてCommitする。
- 続く`詳しく説明して`がCommitし、前回回答の本文・依存Result ID・guaranteeをそのまま保持する。
- Engineを閉じて再度開き、index stateをcoldにしてRecovery inspectを行う。current-memory snapshot fallbackで検査が通る。
- さらに範囲外のFactを80件追加する。current-head indexで得た解決結果、status、依存、guarantee、回答単位が10,000-action full-snapshot参照と一致し、index projectionは全件snapshotより小さい。
- 前提Factを通常Core経路で無効化した後はRecoveryが`RECOVERY_CONTEXT_PROOF_CHANGED`で停止し、既存出力ファイルのbyte列は変わらない。

このテストは当該1経路の正例・cold-index・current-head照合・無効化負例を実証する。CAP-D02全体、全Consumer、任意規模の履歴性能の受入証明ではない。

## 検証

Linux x86_64 / kernel `6.18.57`、Rust `1.99.1 (58cf8f9c3 2026-10-13)`、Cargo `1.99.1 (5f94df478 2026-08-27)`。共有target cacheは`/workspace/.minidora-cloud/rust-target`。`rust/Cargo.toml` SHA-256 `3a54553a0c6d15836b00cf556a821bb011da7295bc11c15ad2ad9b0d25ae8785`、`Cargo.lock` SHA-256 `b4e97fecdca8fd1fc863349b1a48eb99a925da7fed0ed6ce6cfe79bdd1cececc`、`rust-toolchain.toml` SHA-256 `d13306be0ecda81ce779baa290ffb417380d525928477f16200884b5169cca33`。

| Command | 結果 | 保存log |
| --- | --- | --- |
| `cargo test -p minidora-engine --lib recovery::tests::saved_previous_relation_explanation_rechecks_anchor_and_current_head --locked -- --exact --nocapture` | PASS: 1 passed, 0 failed, 274 filtered out | [focused Recovery test](b81-focused-recovery-test.log) |
| `cargo check --workspace --all-targets --locked` | PASS: compile check only | [workspace check](b81-workspace-check.log) |
| `cargo clippy -p minidora-engine --lib --locked -- -D warnings` | PASS | [engine library Clippy](b81-engine-library-clippy.log) |
| `cargo fmt --all -- --check` | PASS | [format check](b81-fmt-check.log) |
| `git diff --check` | PASS | [diff check](b81-diff-check.log) |

全`minidora-engine` suite、workspace all-features runtime、workspace all-target Clippy、CLI/UI、他OS、independent QA、large-history exact replayと完全なE1/CAP-D02受入は実施していない。W10は`PARTIAL`、CAP-D02は`UNVERIFIED`。E1 exit/release/deploymentを主張しない。

今回照合したD2文書SHA-256:

- `01_施工仕様書.md`: `de618da9d50f248a7e984ad807aab4a306f17204eb9e722da387aa7d587573b0`
- `02_施工工程表.md`: `d1afc8a3e225fcbc21ef39eb3152ca9c64ffe79ee61d0702220802700570ce7a`
- `03_Codex施工指示書.md`: `14bcce642e3c6281f4922e1ac788dcef6e1328451aebf82d245922cb9d901d3d`

着工時`main` / `origin/main`は`664c4e1cce027ef891953a94585720363ac01e9d`。local backup refs/tags `codex/backup-main`は同SHA、`codex/backup-main-prev`は`fd29c62b8346bff4aab98a9cea66be98e8477750`。remote同名tagsもそれぞれ同SHA。remote backup branchesは着工時点で`8c425bf28af09b0284101836da598d53fb8d6756` / `e9a275591d9f062d033c07d2a9a4e239414269a6`のまま。rollback refは確認済みbase `664c4e1cce027ef891953a94585720363ac01e9d`。

## ローカル資産の5分類

- **保持:** D2正本、既存Git履歴、凍結βと既存user assetsに変更なし。
- **保管済み:** B81 report、machine observation、focused test/workspace check/Clippy/format/diffログ。
- **稼働中:** [`recovery.rs`](/workspace/MINIDORA-BETA/rust/crates/minidora-engine/src/recovery.rs)、既存shared Cargo target cache（test/check/Clippyで更新）。
- **ゴミ箱移動済み:** 0 files。手動削除・cache cleanupなし。
- **保留:** 0 local assets。テストfixtureはDropで自身の一時領域を片付け、残留pathなし。Windows storage handlingはLinux実行では非該当。

機械可読な詳細は[B81 machine observation](b81-build-observation.json)を参照。
