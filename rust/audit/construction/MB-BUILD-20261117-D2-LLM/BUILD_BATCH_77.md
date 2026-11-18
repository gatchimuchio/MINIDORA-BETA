# BUILD_BATCH_77 — Saved relation-object explanation Recovery current-head revalidation

**BASE HEAD:** `092e88fde3998e6ded338a56d27ef1180b3ab782` (`main` = `origin/main`, clean before this batch). **Version/schema:** `0.7.0-alpha.11` / `minidora-rust/0.7.0-alpha.11`. **実行日:** 2026-11-18 Asia/Tokyo. **Scope:** E1-W10 / CAP-D02の限定施工。**Tested source diff SHA-256:** `adae9bcbfd3270a0b8dd89d3af5e4833604215b4ffc9f1a6d774538eb9fb256c`.

## 実装範囲

Saved `AskRelationObjects`を説明する`ExplainPrevious`のRecovery current-head照合に、既存Core `indexed_relation_objects_answer_from_store`を使う説明評価経路を追加した。

- 全snapshot と index projection のどちらも、同じ Core 説明評価で保存回答・proof・説明行・依存・保証・証拠予算を検査する。
- current-head branchは actor-memory head revision と一致した index projection を使う。projection に使ったaction budgetは既存のindexed answerで回答 budgetから差し引く。
- cold/stale indexは従来どおり認証済みcurrent snapshotへ退避する。保存済みResultRecord revisionの厳密再生は不変snapshot経路を維持する。
- input/result identity、ContentPlan/render、dependency lineageと`RECOVERY_CONTEXT_PROOF_CHANGED`停止境界を保持。schemaと判断責任は変更なし。

## 回帰fixture

日本語の「ミナは何を使いますか」に対し、保存FactとRuleから関係一覧を作り、「詳しく説明して」を`max_actions=32`で実行した。Engine再起動後の保存revision再生が通過し、small snapshotで`memory_index_state`を削除した場合もsnapshot fallbackが通過した。

current memoryへ範囲外の独立Factを80件加え、81 Facts / 1 Ruleにした。sealed 32-actionの全snapshot current recheckは`RESOURCE_LIMIT`で止まり、10,000-actionの全snapshot referenceと同じ32-action予算のindexed Recovery checkは通過した。元Factを無効化すると`RECOVERY_CONTEXT_PROOF_CHANGED`で止まり、説明targetの既存byte列は不変だった。

## 検証

Linux x86_64 / kernel `6.18.57`、Rust `1.99.1 (58cf8f9c3 2026-10-13)`、Cargo `1.99.1 (5f94df478 2026-08-27)`。共有target cacheは`/workspace/.minidora-cloud/rust-target`。

| Command | 結果 | 保存log |
| --- | --- | --- |
| `cargo test -p minidora-engine --locked --lib recovery::tests::relation_objects_explanation_pins_and_rechecks_its_memory_revision -- --exact --nocapture` | PASS: 1 test | [focused Recovery log](b77-focused-recovery-test.log) |
| `cargo test -p minidora-engine --locked --lib explanation_pins_and_rechecks_its_memory_revision -- --nocapture` | PASS: 5 tests | [explanation Recovery tests](b77-explanation-recovery-tests.log) |
| `cargo check -p minidora-engine --all-targets --locked` | PASS | [engine check](b77-engine-check.log) |
| `cargo clippy -p minidora-engine --lib --locked -- -D warnings` | PASS | [engine library Clippy](b77-engine-library-clippy.log) |
| `cargo fmt --all -- --check` | PASS | [format check](b77-fmt-check.log) |
| `git diff --check` | PASS | [diff check](b77-diff-check.log) |

## CAP-D02 / W10判定と限界

このbatchが確認した範囲はsaved relation-object explanationのRecovery current-head再確認のみ。大規模な保存revision replayやcold/stale fallback、同一subjectの拡大、条件closure、多重writer、他Recovery/learning consumer、engine全runtime suite、workspace Clippy/all-features/build、全CAP-D02 acceptanceは未確認。W10は`PARTIAL`、CAP-D02は`UNVERIFIED`。E1 exit、全体系合格、release、deploymentは主張しない。

今回照合したD2文書SHA-256:

- `01_施工仕様書.md`: `de618da9d50f248a7e984ad807aab4a306f17204eb9e722da387aa7d587573b0`
- `02_施工工程表.md`: `d1afc8a3e225fcbc21ef39eb3152ca9c64ffe79ee61d0702220802700570ce7a`
- `03_Codex施工指示書.md`: `14bcce642e3c6281f4922e1ac788dcef6e1328451aebf82d245922cb9d901d3d`

開始時のlocal refsは`codex/backup-main=e427b1919e4513e9038b9dedaa9743a1be23c457`、`codex/backup-main-prev=8317652ab43f933522e66cd13867f9cce6c64745`。remote同名tagsも同SHA、remote backup branchesは`8c425bf28af09b0284101836da598d53fb8d6756` / `e9a275591d9f062d033c07d2a9a4e239414269a6`のまま。rollbackには確認済み`origin/main=092e88fde3998e6ded338a56d27ef1180b3ab782`も保持。

## ローカル資産の5分類

- **保持:** 変更前のD2正本・既存Git履歴・利用者資産に変更なし。
- **保管済み:** B77 report、machine observation、6検査log（8 repo paths）。
- **稼働中:** `core.rs`と`recovery.rs`（2 repo paths）、共有Cargo target cache（既存build cacheを更新）。
- **ゴミ箱移動済み:** 0 files。Linux作業のためWindowsゴミ箱操作は非該当。
- **保留:** 0 local assets。テストの一時DB/出力はtest-owned TempDirが終了時に自動片付けし、残存pathなし。手動削除・cache cleanupなし。

Windows固有asset/storageはこのLinux実行では非該当。機械可読な詳細は[B77 machine observation](b77-build-observation.json)を参照。
