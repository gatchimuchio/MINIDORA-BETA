# BUILD_BATCH_85 — Saved AskPreviousRelation Recovery checks the exact anchor input version

**BASE HEAD:** `c0194ce4c386af5744839b4795bb07db7f22077a` (`main` = `origin/main`, clean at batch start). **Test commit:** `04f579703ba78de23873afcbe8f6a18916657c2d` (`Test recovery rejects wrong anchor input version`). **Version/schema:** `0.7.0-alpha.11` / `minidora-rust/0.7.0-alpha.11`. **実行日:** 2026-11-19 JST. **Scope:** E1-W10 / CAP-D02 saved-result Recovery version identity. **Source diff SHA-256:** `2778207d18d007a6c5f3920eb6e879ad030be6b994f8eb0785d94dbad12a3bd9`.

## 実装・観測範囲

B85はB84で追加したanchorの`input_id/input_version`照合について、`input_version`がStore lookupで実際に使われ、別版へ結び付かないことを試験で固定する。通常のEngine入力で同じactor/case/`input_id`の明示質問をversion 1とversion 2でcommitし、直後の日本語`AskPreviousRelation`結果がversion 2を参照することを確認する。`ExplainPrevious`を一時fixture出力先へ保存後にEngineを再起動すると、有効な保存状態のRecovery `inspect`は成功する。

負例ではclone上のanchor map key、`ResultRecord.id`、`Lifecycle::Commit`、`input_id`を保持し、`input_version`だけを2から1へ変更する。version 1の封印済み入力は同じactor/case/`input_id`で実在するが、そのrequest identityはversion 2のanchor結果IDと一致しないため、Recoveryは`RECOVERY_CONTEXT_PROOF_CHANGED`で停止する。Storeの最新revisionと既存出力byte列は不変。

負例はRecoveryへ渡す`CaseRevision` cloneだけを変更する。永続SQLiteを改変・破損しておらず、Store repair、CLI/HTTP/browser通常経路、長履歴全体を保証しない。

| 因果位置 | 実値・consumer |
| --- | --- |
| Producer | 同じcase/input IDのversion 1・2それぞれの封印済みRequestEnvelopeとResultRecord。AskPreviousRelationの`context_result_ref`はversion 2のResultRecord IDを指す |
| Consumer | B84 Recovery guardがanchorの`input_id/input_version`でStoreのsealed inputを取り、`identity(request) == anchor.id`を照合 |
| 正常 | version 2を参照する保存済みAskPreviousRelation→ExplainPreviousを再起動後にRecoveryが再確認 |
| 負例 | 実在するversion 1へanchorだけを誤結合し、ID不一致で停止。persisted revisionとtarget bytesを保持 |

## 検証

Linux x86_64 / kernel `6.18.57`、Rust `1.99.1 (58cf8f9c3 2026-10-13)`、Cargo `1.99.1 (5f94df478 2026-08-27)`。共有target cacheは`/workspace/.minidora-cloud/rust-target`。`rust/Cargo.toml` SHA-256 `3a54553a0c6d15836b00cf556a821bb011da7295bc11c15ad2ad9b0d25ae8785`、`Cargo.lock` SHA-256 `b4e97fecdca8fd1fc863349b1a48eb99a925da7fed0ed6ce6cfe79bdd1cececc`、`rust-toolchain.toml` SHA-256 `d13306be0ecda81ce779baa290ffb417380d525928477f16200884b5169cca33`。

| Command | 結果 | 保存log |
| --- | --- | --- |
| `cargo test -p minidora-engine --lib recovery::tests::saved_previous_relation_explanation_rejects_wrong_anchor_input_version --locked -- --exact --nocapture` | PASS: 1 passed, 0 failed, 278 filtered out | [wrong-version anchor test](b85-wrong-anchor-input-version-test.log) |
| `cargo test -p minidora-engine --lib saved_previous_relation_explanation_ --locked -- --nocapture` | PASS: 5 passed, 0 failed, 274 filtered out | [Recovery filter](b85-saved-prior-relation-recovery-filter.log) |
| `cargo check --workspace --all-targets --locked` | PASS: compile check only; testsは実行しない | [workspace check](b85-workspace-check.log) |
| `cargo clippy -p minidora-engine --lib --locked -- -D warnings` | PASS | [engine library Clippy](b85-engine-library-clippy.log) |
| `cargo fmt --all -- --check` | PASS | [format check](b85-fmt-check.log) |
| `git diff --check` | PASS | [diff check](b85-diff-check.log) |

## 未実施・判定

full Engine/workspace runtime suite、workspace all-target Clippy、CLI/HTTP/browser通常入口、永続Store/SQLite物理破損とrepair、長履歴の全consumer実行、全source意味閉包、全CAP-D02/E1受入、他OS実機、独立QA、E2-E8、release/deploymentは未実施。E1-W10は`PARTIAL`、CAP-D02は`UNVERIFIED`。E1 exit/release/deploymentを主張しない。

着工時`main`/`origin/main`とlocal `codex/backup-main` branch/tagは`c0194ce4c386af5744839b4795bb07db7f22077a`、`codex/backup-main-prev` branch/tagは`39b4ddcfc00557dee1edfa073f48ab284ca6644b`。同名remote tagsもこれらの値。remote backup branchesは既存の`8c425bf28af09b0284101836da598d53fb8d6756` / `e9a275591d9f062d033c07d2a9a4e239414269a6`を保持。rollback refは着工時main `c0194ce4c386af5744839b4795bb07db7f22077a`。

## B85資産の5分類

B85で生成・更新した現存資産は**10件**で、5区分の合計も10件：

- **保持: 0件の新規更新資産.** D2正本、凍結β、既存履歴は保持し変更なし。
- **保管済み: 8件**（BUILD_BATCH、machine observation、test/filter/check/Clippy/format/diffの6 log）。
- **稼働中: 2件**（[`recovery.rs`](/workspace/MINIDORA-BETA/rust/crates/minidora-engine/src/recovery.rs)の回帰テスト1件、共有Cargo target cache）。
- **ゴミ箱移動済み: 0件。** ゴミ箱は空にせず、cache削除なし。
- **保留: 0件。** fixtureの一時DB/targetはcleanupされ、残留なし。

Windows用保管skill・OneDrive操作はLinux実行では非該当。Git管理物はGitHub `main`と遠隔バックアップタグで保存し、OneDrive重複保存はしていない。機械可読な詳細は[B85 machine observation](b85-build-observation.json)を参照。
