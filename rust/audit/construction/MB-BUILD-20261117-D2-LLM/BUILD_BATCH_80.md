# BUILD_BATCH_80 — Saved AskPreviousRelation explanation anchor Recovery revalidation

**BASE HEAD:** `fd29c62b8346bff4aab98a9cea66be98e8477750` (`main` = `origin/main`, clean before this batch). **Implementation commit:** `73bfcbe99a7d995c1774e44cd5b99bedd8bc486d` (`Revalidate AskPreviousRelation explanation anchors`). **Version/schema:** `0.7.0-alpha.11` / `minidora-rust/0.7.0-alpha.11`. **実行日:** 2026-11-18 Asia/Tokyo. **Scope:** E1-W10 / CAP-D02の限定施工。**Source diff SHA-256:** `30c46143e487400ac3bb1b6f3c8136da8874102b32c030117952c099aa112e03`.

## 実装範囲

`AskPreviousRelation`の保存済み回答を参照する`ExplainPrevious` Recoveryに、元回答が依存した直前結果アンカーの再検証を追加した。

- 保存済みprior input/resultのactor・case・identity・commit状態と、直前アンカーResultRecordを照合する。
- priorが記録したactor-memory revisionから回答を再構成し、prior ContentPlan/render、依存、guarantee、回答単位を照合する。revisionが欠落している結果は、この専用経路では検証済み扱いにしない。
- current headでは`indexed_previous_relation_query_from_store`を使い、revision一致indexから同じ直接前提アンカーと関係proofを再評価する。indexがcold/staleなら認証済みfull snapshotへ退避する。
- 解決されたproposition、proof dependencies、guarantee、回答単位が元回答から変化した場合、Recoveryを`RECOVERY_CONTEXT_PROOF_CHANGED`で停止する。
- 保存回答の文章からrelation meaningを復元できない場合は、ExplainPreviousがその前回本文、ResultRecord dependency、guaranteeをそのまま引き継いでいることを照合する。
- Core通常経路、API、schema、既存保存形式、判断主体は変更していない。

この変更は、保存済み型付きAskPreviousRelationのanchorとproofをRecoveryで再確認する実装である。Coreの回答内容や説明表現を追加するものではない。

## 検証

Linux x86_64 / kernel `6.18.57`、Rust `1.99.1 (58cf8f9c3 2026-10-13)`、Cargo `1.99.1 (5f94df478 2026-08-27)`。共有target cacheは`/workspace/.minidora-cloud/rust-target`。`rust/Cargo.toml` SHA-256 `3a54553a0c6d15836b00cf556a821bb011da7295bc11c15ad2ad9b0d25ae8785`、`Cargo.lock` SHA-256 `b4e97fecdca8fd1fc863349b1a48eb99a925da7fed0ed6ce6cfe79bdd1cececc`、`rust-toolchain.toml` SHA-256 `d13306be0ecda81ce779baa290ffb417380d525928477f16200884b5169cca33`。

| Command | 結果 | 保存log |
| --- | --- | --- |
| `cargo check --workspace --all-targets --locked` | PASS: compile only; tests were not executed | [workspace check](b80-workspace-check.log) |
| `cargo clippy -p minidora-engine --lib --locked -- -D warnings` | PASS | [engine library Clippy](b80-engine-library-clippy.log) |
| `cargo fmt --all -- --check` | PASS | [format check](b80-fmt-check.log) |
| `git diff --cached --check` | PASS | [staged diff check](b80-diff-check.log) |

機能テストは追加・実行していない。したがって通常ExplainPreviousとの組合せ、Engine reopen、current-head invalidation、cold-index fallback、target bytes不変などの実行時挙動は未検証であり、挙動PASSを主張しない。Clippyの初回指摘とtoolchain/log保存時の調整は[adjustment log](b80-adjustments.log)に記録した。

## E1-W10 / CAP-D02判定と限界

このbatchが対象にしたのは、保存済み型付き`AskPreviousRelation`を説明するRecoveryのcurrent-head recheckのみ。保存済みinput bundleと元memory revisionが必要であり、legacy/missing-bundle resultの既存一般処理はこの施工で変更・検証していない。実行時Recovery fixture、cold/stale fallback、proof invalidation、target-byte保持、全engine runtime suite、workspace all-features、同一subject拡大、多重writer、condition closureおよび完全なCAP-D02受入は未確認。E1-W10は`PARTIAL`、CAP-D02は`UNVERIFIED`。E1 exit、E2、release、deploymentは主張しない。

今回照合したD2文書SHA-256:

- `01_施工仕様書.md`: `de618da9d50f248a7e984ad807aab4a306f17204eb9e722da387aa7d587573b0`
- `02_施工工程表.md`: `d1afc8a3e225fcbc21ef39eb3152ca9c64ffe79ee61d0702220802700570ce7a`
- `03_Codex施工指示書.md`: `14bcce642e3c6281f4922e1ac788dcef6e1328451aebf82d245922cb9d901d3d`

着工時`main` / `origin/main`は`fd29c62b8346bff4aab98a9cea66be98e8477750`。local `codex/backup-main`と同名tagは同SHA、local `codex/backup-main-prev`と同名tagは`512287891e7d91f4021053b10c90d98a65cc6143`。remote同名tagsも同じ二つのSHA。remote backup branchesは着工時点で`8c425bf28af09b0284101836da598d53fb8d6756` / `e9a275591d9f062d033c07d2a9a4e239414269a6`のまま。rollback refは確認済みbase `fd29c62b8346bff4aab98a9cea66be98e8477750`。

## ローカル資産の5分類

- **保持:** D2正本、既存Git履歴、凍結βと既存user assetsに変更なし。
- **保管済み:** B80 report、machine observation、compile/Clippy/format/diffログ、adjustment log。
- **稼働中:** [recovery.rs](/workspace/MINIDORA-BETA/rust/crates/minidora-engine/src/recovery.rs)、既存shared Cargo target cache（compile/Clippyで更新）。
- **ゴミ箱移動済み:** 0 files。手動削除・cache cleanupなし。
- **保留:** 0 local assets。テストfixtureは作成していない。Windows storage handlingはLinux実行では非該当。

機械可読な詳細は[B80 machine observation](b80-build-observation.json)を参照。
