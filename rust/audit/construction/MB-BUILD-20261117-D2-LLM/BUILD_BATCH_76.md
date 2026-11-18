# BUILD_BATCH_76 — Saved common-relations explanation Recovery current-head revalidation

**BASE HEAD:** `e427b1919e4513e9038b9dedaa9743a1be23c457` (`main` = `origin/main`, clean before this batch). **Version/schema:** `0.7.0-alpha.11` / `minidora-rust/0.7.0-alpha.11`. **実行日:** 2026-11-18 Asia/Tokyo. **Scope:** E1-W10 / CAP-D02の限定施工。**Tested source diff SHA-256:** `6b93419c7dd7b8b41c7c30674dc121326bc4c388902b6a6b5b8e71b7dc707750`.

## 実装範囲

Saved `AskCommonRelations`を説明する`ExplainPrevious`のRecovery current-head照合に、Core内のrevision-matched relation index経路を追加した。

- 詳細説明のproof照合・依存・保証・説明単位の検査を、全snapshot queryとindexed projection queryで共用する。
- current-head branchは index が actor memory head revision と一致するときCore indexed queryを使う。cold/stale indexは従来の認証済みcurrent snapshotへ退避する。
- 保存済みResultRecord revisionの厳密再生は、引き続き不変snapshotで全snapshot queryを実行する。
- result/input identity、report artifact、ContentPlan、render、dependencies、guarantee、`RECOVERY_CONTEXT_PROOF_CHANGED`の停止境界は維持した。schemaと判断主体は変更なし。

## 回帰fixture

Rule「Xは鳥ならXは動物」とココ・ミケのFactから日本語の共通関係を保存し、「詳しく説明して」を`max_actions=32`で実行した。Engine再起動後の厳密な保存revision再生が通過し、small snapshotで`memory_index_state`を除去した場合もcurrent snapshot fallbackが通過した。

その後、対象scope内に無関係な独立Rule/Factを80組加え、current memoryを82 Facts / 81 Rulesにした。sealed 32-actionの全snapshot current recheckは`RELATION_BUDGET`で停止し、10,000-action全snapshot referenceと同じ32-action予算のindexed Recovery checkは通過した。ミケの支持Factを無効化すると`RECOVERY_CONTEXT_PROOF_CHANGED`で止まり、先に描画した説明targetのbyte列は不変だった。

## 検証

Linux x86_64 / kernel `6.18.57`、Rust `1.99.1 (58cf8f9c3 2026-10-13)`、Cargo `1.99.1 (5f94df478 2026-08-27)`。共有target cacheは`/workspace/.minidora-cloud/rust-target`。

| Command | 結果 | 保存log |
| --- | --- | --- |
| `cargo test -p minidora-engine --locked --lib recovery::tests::common_relations_explanation_pins_and_rechecks_its_memory_revision -- --exact --nocapture` | PASS: 1 test | [focused Recovery log](b76-focused-recovery-test.log) |
| `cargo test -p minidora-engine --locked --lib explanation_pins_and_rechecks_its_memory_revision -- --nocapture` | PASS: 5 tests | [explanation Recovery tests](b76-explanation-recovery-tests.log) |
| `cargo check -p minidora-engine --all-targets --locked` | PASS | [engine check](b76-engine-check.log) |
| `cargo clippy -p minidora-engine --lib --locked -- -D warnings` | PASS | [engine library Clippy](b76-engine-library-clippy.log) |
| `cargo fmt --all -- --check` | PASS | [format check](b76-fmt-check.log) |
| `git diff --check` | PASS | [diff check](b76-diff-check.log) |

## CAP-D02 / W10判定と限界

このbatchが検証した範囲はsaved common-relations explanationのRecovery current-head再確認のみ。大規模な厳密保存revision replayやcold/stale fallback、同一subjectの拡大、条件closure、多重writer、他Recovery/learning consumer、minidora-engine全runtime suite、workspace Clippy/all-features/build、全CAP-D02 acceptanceは未確認。W10は`PARTIAL`、CAP-D02は`UNVERIFIED`。E1 exit、全体系合格、release、deploymentは主張しない。

今回照合したD2文書SHA-256:

- `01_施工仕様書.md`: `de618da9d50f248a7e984ad807aab4a306f17204eb9e722da387aa7d587573b0`
- `02_施工工程表.md`: `d1afc8a3e225fcbc21ef39eb3152ca9c64ffe79ee61d0702220802700570ce7a`
- `03_Codex施工指示書.md`: `14bcce642e3c6281f4922e1ac788dcef6e1328451aebf82d245922cb9d901d3d`

復元参照は開始時にlocal/tagのbase=`e427b1919e4513e9038b9dedaa9743a1be23c457`、previous=`8317652ab43f933522e66cd13867f9cce6c64745`。remote backup branchesは`8c425bf28af09b0284101836da598d53fb8d6756` / `e9a275591d9f062d033c07d2a9a4e239414269a6`のまま。

## 生成物・資産の5分類

- **保持:** prior common-relations input/result、report artifact、保存memory revision、explanation ContentPlan/render、既存output bytesを保持。
- **保存:** Core indexed explanation evaluator、Recovery接続と回帰テスト、B76 report、machine observation、検査log。
- **有効:** saved common-relations `ExplainPrevious` Recoveryのcurrent-head再照合でrevision-matched Core indexを利用。
- **廃棄:** 0 repository files / 0 saved records。テストが作った一時fixtureは各test終了時に片付けた。移動・手動削除・cache cleanupなし。
- **保留:** 大規模なexact replay/fallback、同時writer、他Recovery consumerと全体系受入。

Windows固有asset/storageはこのLinux実行では対象外。機械可読な詳細は[B76 machine observation](b76-build-observation.json)を参照。
