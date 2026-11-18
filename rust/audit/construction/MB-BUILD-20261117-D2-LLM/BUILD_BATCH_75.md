# BUILD_BATCH_75 — Saved comparison explanation Recovery current-head revalidation

**BASE HEAD:** `8317652ab43f933522e66cd13867f9cce6c64745` (`main` = `origin/main`, clean before this batch). **Version/schema:** `0.7.0-alpha.11` / `minidora-rust/0.7.0-alpha.11`. **実行日:** 2026-11-18 Asia/Tokyo. **Scope:** E1-W10 / CAP-D02の限定施工。**Tested source diff SHA-256:** `d934439f6cfe7cdb77811885376205b931a0713d96f08cde97cfc1d8f8ad2fa3`.

## 実装範囲

Saved `CompareRelationInventories`を説明する`ExplainPrevious`のRecovery current-head照合に、既存Core `verify_relation_comparison_memory_result_with_index`を接続した。current-head分岐はrevision-matched indexを先に使い、cold/stale indexの場合だけcurrent full snapshotへ戻る。

- exact retained-revision replayは従来どおり、渡されたimmutable snapshot上でfull-snapshot comparison proofを検証する。
- prior comparison input/result、artifact、ContentPlan、render、profile explanation dependency/content checksは保持。
- current recheckは sealed explanation budgetを使い、既存の`RECOVERY_CONTEXT_PROOF_CHANGED`停止境界を保持。
- schema、Core判断責任、出力bytesは変更なし。

## 回帰fixture

日本語でココとミケの保存関係を比較してから「詳しく説明して」を実行し、explanation `max_actions=32`で採用した。Engine再起動後のexact revision replayと、small snapshotでindex stateを削除した場合のfull-snapshot fallbackを照合した。

その後、80 scopesにoff-scope Factsを加え、current memoryを82 Facts / 0 Rulesにした。sealed 32-action full-snapshot current comparisonは`RELATION_BUDGET`で止まった。10,000-action full-snapshot referenceは通過し、同じ32-action予算のindexed Recovery comparisonも保存済みartifact、proof、dependencies、guaranteeを検証して通過した。直前Factを無効化すると`RECOVERY_CONTEXT_PROOF_CHANGED`で停止し、説明target bytesは不変だった。

## 検証

Linux x86_64 / kernel `6.18.57`、Rust `1.99.1 (58cf8f9c3 2026-10-13)`、Cargo `1.99.1 (5f94df478 2026-08-27)`。共有target cacheは`/workspace/.minidora-cloud/rust-target`。

| Command | 結果 | 保存log |
| --- | --- | --- |
| `cargo test -p minidora-engine --locked --lib recovery::tests::saved_relation_comparison_explanation_pins_and_rechecks_its_memory_revision -- --exact --nocapture` | PASS: 1 test | [focused Recovery log](b75-focused-recovery-test.log) |
| `cargo test -p minidora-engine --locked --lib explanation_pins_and_rechecks_its_memory_revision -- --nocapture` | PASS: 5 tests | [explanation Recovery tests](b75-explanation-recovery-tests-final.log) |
| `cargo check -p minidora-engine --all-targets --locked` | PASS | [engine check](b75-engine-check-final.log) |
| `cargo clippy -p minidora-engine --lib --locked -- -D warnings` | PASS | [engine library Clippy](b75-engine-library-clippy-final.log) |
| `cargo fmt --all -- --check` | PASS | [format check](b75-fmt-check-final.log) |
| `git diff --check` | PASS | [diff check](b75-diff-check-final.log) |

## CAP-D02 / W10判定と限界

このbatchが限定するのはsaved comparison explanationのpending file effect前 current-head recheckだけ。immutable large-history replay、large-history cold/stale fallback、same-subject scale、large condition closure、同時writer、他Recovery/learning consumers、full engine package runtime suite、workspace all-target Clippy、workspace all-features runtime/build、全CAP-D02 acceptanceは未確認。W10は`PARTIAL`、CAP-D02は`UNVERIFIED`。E1 exit、release、deploymentは主張しない。

今回照合したD2文書SHA-256:

- `01_施工仕様書.md`: `de618da9d50f248a7e984ad807aab4a306f17204eb9e722da387aa7d587573b0`
- `02_施工工程表.md`: `d1afc8a3e225fcbc21ef39eb3152ca9c64ffe79ee61d0702220802700570ce7a`
- `03_Codex施工指示書.md`: `14bcce642e3c6281f4922e1ac788dcef6e1328451aebf82d245922cb9d901d3d`

復元参照は開始時にlocal/tagのbase=`8317652ab43f933522e66cd13867f9cce6c64745`、previous=`295fec7f8b7e2e2adfad068256875e56fe502f4e`。remote backup branchesは`8c425bf28af09b0284101836da598d53fb8d6756` / `e9a275591d9f062d033c07d2a9a4e239414269a6`のまま。

## 生成物・資産の5分類

- **保持:** sealed comparison input/result, report artifact, original memory revision, explanation ContentPlan/render and existing output bytesを保持。
- **保存:** Core index connection、large-history Recovery regression、B75 report、machine observationと検査logを保存。
- **有効:** saved comparison `ExplainPrevious` current-head Recoveryにrevision-matched Core comparison verifierを利用。
- **廃棄:** 0 files / 0 records。移動・削除・cache cleanupなし。
- **保留:** large-history exact replay/fallback scale、同時writer、その他Recovery consumersと全体系受入。

Windows固有asset/storageはこのLinux実行では対象外。機械可読な詳細は[B75 machine observation](b75-build-observation.json)を参照。
