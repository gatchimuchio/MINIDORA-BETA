# BUILD_BATCH_74 — Saved relation-profile explanation Recovery current-head revalidation

**BASE HEAD:** `295fec7f8b7e2e2adfad068256875e56fe502f4e` (`main` = `origin/main`, clean before this batch). **Version/schema:** `0.7.0-alpha.11` / `minidora-rust/0.7.0-alpha.11`. **実行日:** 2026-11-18 Asia/Tokyo. **Scope:** E1-W10 / CAP-D02の限定施工。**Tested source diff SHA-256:** `6f313c3cfbd77894572f02ce36998725ebf6c76b57c28ec48800a35f2d7490ec`.

## 実装範囲

Saved `DraftRelationProfile`を説明する`ExplainPrevious`のRecovery current-head照合に、既存Coreのrevision-matched `indexed_relation_profile_answer_from_store`を接続した。indexed answerと従来のfull-snapshot answerは、元profile snapshotとの同値、explanation dependencies、guarantee、units、budgetを共通の検査関数で確認する。

- Indexがcold/staleなら従来のauthenticated current full-snapshot fallbackを使う。
- immutable adoption revisionのreplayとCore採用処理は変更なし。
- generic relation answer、saved comparison、common-relations、relation-object分岐は変更なし。
- 異なるprofile proof/unitsは`RECOVERY_CONTEXT_PROOF_CHANGED`で停止し、保存済みtarget bytesを採用しない。

## 回帰fixture

日本語の保存profileを`HYPOTHESIS`で採用し、別出力先へ「詳しく説明して」を`max_actions=32`で実行した。Engine reopen後のexact revision replayを照合し、index stateを削除してsmall-snapshot fallbackを通した。

その後、元subject外の80 scopesへFactを追加し、current memoryを81 Facts / 1 Ruleにした。profileのcurrent indexed answerは10,000-action full-snapshot referenceと一致した。全snapshotをsealed 32-action予算で評価した場合は`RELATION_BUDGET`となり、indexed Recovery照合は通過した。Ruleを無効化すると`RECOVERY_CONTEXT_PROOF_CHANGED`で停止し、説明出力の既存bytesは不変だった。

初回Clippyで新しい共通照合関数の8引数が`too_many_arguments`に該当した。subjectとprofile answerを一組のtupleにまとめ、抑制attributeを追加せずに解消した。初回Clippy logは監査用に保持した。

## 検証

Linux x86_64 / kernel `6.18.57`、Rust `1.99.1 (58cf8f9c3 2026-10-13)`、Cargo `1.99.1 (5f94df478 2026-08-27)`。共有target cacheは`/workspace/.minidora-cloud/rust-target`。

| Command | 結果 | 保存log |
| --- | --- | --- |
| `cargo test -p minidora-engine --locked --lib recovery::tests::relation_profile_explanation_pins_and_rechecks_its_memory_revision -- --exact --nocapture` | PASS: 1 test | [focused Recovery log](b74-focused-recovery-test.log) |
| `cargo test -p minidora-engine --locked --lib relation_profile -- --nocapture` | PASS: 5 tests | [relation-profile tests](b74-relation-profile-tests-final.log) |
| `cargo check -p minidora-engine --all-targets --locked` | PASS | [engine check](b74-engine-check-final.log) |
| `cargo clippy -p minidora-engine --lib --locked -- -D warnings` | PASS | [engine library Clippy](b74-engine-library-clippy-final.log) |
| `cargo fmt --all -- --check` | PASS | [format check](b74-fmt-check-final.log) |
| `git diff --check` | PASS | [diff check](b74-diff-check-final.log) |

## CAP-D02 / W10判定と限界

このbatchが限定するのはsaved relation-profile explanationのpending file effect前 current-head recheckだけ。immutable large-history replay、large-history cold/stale fallback、同subject scale、large condition closure、同時writer、他Recovery/learning consumers、full engine package runtime suite、workspace all-target Clippy、workspace all-features runtime/build、全CAP-D02 acceptanceは未確認。W10は`PARTIAL`、CAP-D02は`UNVERIFIED`。E1 exit、release、deploymentは主張しない。

今回照合したD2文書SHA-256:

- `01_施工仕様書.md`: `de618da9d50f248a7e984ad807aab4a306f17204eb9e722da387aa7d587573b0`
- `02_施工工程表.md`: `d1afc8a3e225fcbc21ef39eb3152ca9c64ffe79ee61d0702220802700570ce7a`
- `03_Codex施工指示書.md`: `14bcce642e3c6281f4922e1ac788dcef6e1328451aebf82d245922cb9d901d3d`

復元参照は開始時にlocal/tagのbase=`295fec7f8b7e2e2adfad068256875e56fe502f4e`、previous=`ec1cabaaa00e17f27d42d5046df1b24c87db7f01`。remote backup branchesは`8c425bf28af09b0284101836da598d53fb8d6756` / `e9a275591d9f062d033c07d2a9a4e239414269a6`のまま。

## 生成物・資産の5分類

- **保持:** saved profile input/result, original actor-memory revision, profile dependencies, `HYPOTHESIS` guarantee, explanation plan/render and existing output bytesを保持。
- **保存:** current profile index Recovery connection、large-history regression、B74 report、machine observation、successful checksと初回Clippy findingを保存。
- **有効:** saved profile `ExplainPrevious` current-head RecoveryにCore indexed profile evaluatorを利用。
- **廃棄:** 0 files / 0 records。移動・削除・cache cleanupなし。
- **保留:** large-history exact replay/fallback scale、同時writer、その他Recovery consumersと全体系受入。

Windows固有asset/storageはこのLinux実行では対象外。機械可読な詳細は[B74 machine observation](b74-build-observation.json)を参照。
