# BUILD_BATCH_79 — Saved premise inventory ExplainPrevious Recovery revalidation

**BASE HEAD:** `512287891e7d91f4021053b10c90d98a65cc6143` (`main` = `origin/main`, clean before this batch). **実装commit:** `48fc7a1e3d4cd2ef6202db550bf73be6ff185ecf` (`Revalidate saved premise inventory explanations`). **Version/schema:** `0.7.0-alpha.11` / `minidora-rust/0.7.0-alpha.11`. **実行日:** 2026-11-18 Asia/Tokyo. **Scope:** E1-W10 / CAP-D02の限定施工。**施工source diff SHA-256:** `4d399df9d5a16942bb6fb8a3730b13fa879e8789f857de66f3ccc5bfb5d82403`.

## 実装範囲

保存済み`InspectPremises`結果を参照する`ExplainPrevious` Recoveryに、現actor-memory headでの一覧再確認を接続した。

- prior input/resultのactor・case・ID・commit状態を照合し、prior ContentPlan、render artifact、採用時actor-memory revisionを検証する。
- CoreとRecoveryで前提一覧の表示単位生成を共有し、採用時inventoryのfacts・競合表示・依存・guaranteeと保存済み単位を照合する。Core側は遅延生成を保ち、既存の証拠byte予算による逐次停止を維持する。
- 現行memoryは`indexed_premise_inventory_from_store`のrevision一致indexで再評価する。indexがcold/staleなら`Store::load_memory`の認証済みsnapshotへ退避する。
- 現inventoryと採用時inventoryの全構造を比較し、不一致を`RECOVERY_CONTEXT_PROOF_CHANGED`として止める。ExplainPreviousの依存・本文・guaranteeも厳密な前回結果へ結び付ける。
- 既存のExplainPrevious表示内容、schema、公開API、判断主体、保存形式は変更していない。

実経路：型付きInspectPremises入力とprior ResultRecord → 採用時actor-memory revisionのinventory・表示単位を再構成 → revision一致indexまたは認証済みcurrent snapshotで再評価 → inventory・依存・guarantee・ExplainPrevious出力の同一性を照合 → proof変更時はRecovery停止。新しい説明文やCore機能を追加する施工ではない。

## 検証

Linux x86_64 / kernel `6.18.57`、Rust `1.99.1 (58cf8f9c3 2026-10-13)`、Cargo `1.99.1 (5f94df478 2026-08-27)`。共有target cacheは`/workspace/.minidora-cloud/rust-target`。`rust/Cargo.toml` SHA-256 `3a54553a0c6d15836b00cf556a821bb011da7295bc11c15ad2ad9b0d25ae8785`、`Cargo.lock` SHA-256 `b4e97fecdca8fd1fc863349b1a48eb99a925da7fed0ed6ce6cfe79bdd1cececc`、`rust-toolchain.toml` SHA-256 `d13306be0ecda81ce779baa290ffb417380d525928477f16200884b5169cca33`。

| Command | 結果 | 保存log |
| --- | --- | --- |
| `cargo check -p minidora-engine --all-targets --locked` | PASS: compileのみ、test実行なし | [engine check](b79-engine-check.log) |
| `cargo clippy -p minidora-engine --lib --locked -- -D warnings` | PASS | [engine library Clippy](b79-engine-library-clippy.log) |
| `cargo fmt --all -- --check` | PASS | [format check](b79-fmt-check.log) |
| `git diff --check` | PASS | [diff check](b79-diff-check.log) |

このbatchでは機能テストを追加・実行していない。従って通常実行、再起動、cold/stale fallback、head変更時の停止、target bytes不変は未観測であり、挙動PASSを主張しない。

## E1-W10 / CAP-D02判定と限界

このbatchが施工した範囲は保存済み前提一覧を参照するExplainPrevious Recoveryのcurrent-head再確認のみ。W10は`PARTIAL`、CAP-D02は`UNVERIFIED`。実行時Recovery fixture、他の未対象consumer、同一subject拡大、条件closure、多重writer、workspace runtime suite、E1/E2 acceptanceは未確認。E1 exit、全体系合格、release、deploymentは主張しない。

今回照合した現行D2文書SHA-256:

- `01_施工仕様書.md`: `de618da9d50f248a7e984ad807aab4a306f17204eb9e722da387aa7d587573b0`
- `02_施工工程表.md`: `d1afc8a3e225fcbc21ef39eb3152ca9c64ffe79ee61d0702220802700570ce7a`
- `03_Codex施工指示書.md`: `14bcce642e3c6281f4922e1ac788dcef6e1328451aebf82d245922cb9d901d3d`

着工時local refsは`codex/backup-main=e427b1919e4513e9038b9dedaa9743a1be23c457`、`codex/backup-main-prev=8317652ab43f933522e66cd13867f9cce6c64745`。同名local tagsも同SHA。remote同名tagsは`e427b1919e4513e9038b9dedaa9743a1be23c457` / `8317652ab43f933522e66cd13867f9cce6c64745`、remote backup branchesは`8c425bf28af09b0284101836da598d53fb8d6756` / `e9a275591d9f062d033c07d2a9a4e239414269a6`。rollback refは確認済みbase `512287891e7d91f4021053b10c90d98a65cc6143`。

## ローカル資産の5分類

- **保持:** D2正本、既存Git履歴、凍結β・user assetsに変更なし。
- **保管済み:** B79 report、machine observation、4 verification logs。
- **稼働中:** [core.rs](/workspace/MINIDORA-BETA/rust/crates/minidora-engine/src/core.rs)、[recovery.rs](/workspace/MINIDORA-BETA/rust/crates/minidora-engine/src/recovery.rs)、既存shared Cargo target cache（compile/Clippyで更新）。
- **ゴミ箱移動済み:** 0 files。手動削除・cache cleanupなし。
- **保留:** 0 local assets。test-owned temporary fixtureは作成していない。Windows storage handlingはLinux実行では非該当。

機械可読な詳細は[B79 machine observation](b79-build-observation.json)を参照。
