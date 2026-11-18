# BUILD_BATCH_64 — Goal-related scope selection for relation comparison

**BASE HEAD:** `d8d86e1601fd6e2f3b943931ce93119e7e2c47c5` (`main` = `origin/main`, clean before this batch). **Version/schema:** `0.7.0-alpha.11` / `minidora-rust/0.7.0-alpha.11`. **実行日:** 2026-11-18 Asia/Tokyo. **Scope:** E1-W10の限定施工。**Tested source diff SHA-256:** `b383c3fbac8378c8e259c4e70e09a365fe665e407f4e3828e6d810a484ef7993`.

## 実装範囲

Storeに`memory_index_scope`を追加し、通常の`CompareRelationInventories`経路が指定scope内の有効Fact/Rule候補だけをrevision-matched index transactionから取得するようにした。Coreは従来どおり比較inventory、conflict/dependency、v2/v3/v4 content選択、結果採用を担当する。左右両方のinventoryは同一index revisionから投影し、条件と証拠は既存のactive relation projection/evaluatorで解決する。v3/v4内容生成用Ruleも各projectionの証明閉包から保持する。

- 指定scopeにある全候補を有限`max_actions`内で選び、CoreでRuleの全premise/conclusionが同じscopeかを確認する。無関係scopeの履歴は読まず、candidate rowsも同じbudgetで制限する。
- Direct typed comparisonのsource-ID preflight、通常比較、通常Core内の保存比較再検証がrevision-matched索引を使用する。左右の結果は一つのrevisionに揃える。
- cold/stale indexは認証済みcurrent snapshotから一度再構築して再試行する。再構築中にheadが進んだ場合は読込み済みimmutable snapshotへfallbackする。
- 永続Fact/Rule/Condition、比較artifact、Core採否責任、schema versionは変更していない。`Recovery`のexact retained-snapshot verificationは引き続きfull snapshot pathである。

## 観測結果

`indexed_relation_comparison_selects_only_the_requested_scope_on_the_normal_route`は`lab`に4 Facts（左右各2 relation）、別々の80 scopesに80 unrelated Factsを保持した。scope selectorは4候補だけを返し、左右のindexed inventoriesは10,000-action full-snapshot referenceと完全一致した。旧full-snapshot inventoryは同じ64-action per-subject budgetで`RELATION_BUDGET`、indexed normal typed Core comparisonはbudget 128で`COMMIT`し、4つのsource Fact IDsと`likes`/`owns`の比較内容を保持した。事前にindex headを削除し、cold-index rebuild経由でもrevision 1を記録した。

Rule payloadを含む既存comparison regressionsも通過した。v4 saved comparisonの再生/証拠失効停止、旧v1 control replay、actor間のRule失効分離を確認した。focused projection module内8件がPASSした。

## 実行環境と検証

Linux x86_64 / kernel `6.18.57`、Rust `1.99.1 (58cf8f9c3 2026-10-13)`、Cargo `1.99.1 (5f94df478 2026-08-27)`。全engine package testおよびworkspace all-target Clippyは実行していない。

| Command | 結果 | 保存log |
| --- | --- | --- |
| `cargo test -p minidora-engine --locked --lib indexed_relation_comparison_selects_only_the_requested_scope_on_the_normal_route` | PASS: scope candidate selection, full-snapshot parity, normal Core compare, cold-index rebuild | [focused Core log](b64-focused-core-test.log) |
| `cargo test -p minidora-engine --locked --lib active_relation_projection_tests` | PASS: 8 focused relation projection tests | [projection regression log](b64-projection-regression-test.log) |
| `cargo test -p minidora-engine --locked --test core_atomic_relation_comparison_42 --test core_atomic_relation_comparison_46 --test core_atomic_relation_comparison_47` | PASS: 3 existing comparison/replay/invalidation/actor-isolation tests | [comparison replay log](b64-comparison-replay-test.log) |
| `cargo fmt --all -- --check` | PASS | [format log](b64-fmt-check.log) |
| `cargo check -p minidora-engine --all-targets --locked` | PASS | [engine check log](b64-engine-check.log) |
| `cargo clippy -p minidora-engine --lib --locked -- -D warnings` | PASS | [engine library Clippy log](b64-engine-library-clippy.log) |
| `git diff --check` | PASS | [diff check log](b64-diff-check.log) |

## CAP-D02 / W10 判定と限界

このbatchが閉じるのはCoreの通常比較経路と通常保存比較再検証のscope-scoped candidate selectionだけである。指定scope内に多数の無関係subjectがあるpopulation、同時writer、full-snapshot cold-index rebuild / concurrent-head fallback cost、Recovery側comparison replay、全体の長履歴scaleは未検証。よってW10は`PARTIAL`、CAP-D02は`UNVERIFIED`のまま。E1 exit、release、deploymentは主張しない。

今回照合したD2文書SHA-256:

- `01_施工仕様書.md`: `de618da9d50f248a7e984ad807aab4a306f17204eb9e722da387aa7d587573b0`
- `02_施工工程表.md`: `d1afc8a3e225fcbc21ef39eb3152ca9c64ffe79ee61d0702220802700570ce7a`
- `03_Codex施工指示書.md`: `14bcce642e3c6281f4922e1ac788dcef6e1328451aebf82d245922cb9d901d3d`

## 生成物・資産の5分類

- **保持:** baseのtracked paths、immutable memory history、既存比較artifact契約を保持。
- **保存:** Core / relations / Store実装、CAP-D02追跡、選択検証log、BUILD_BATCH_64とmachine observationを追加。
- **有効:** 通常Core relation comparisonのrequested-scope indexed inventory projection。
- **廃棄:** 0 files / 0 records。移動・削除なし。
- **保留:** scope内広履歴、Recovery projection、cold-index全snapshot構築、同時writer、広範囲CAP-D02受入。

機械可読な詳細は[B64 machine observation](b64-build-observation.json)を参照する。
