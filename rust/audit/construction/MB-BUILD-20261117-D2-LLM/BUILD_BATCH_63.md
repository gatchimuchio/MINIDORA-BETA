# BUILD_BATCH_63 — Goal-related inventory selection for ordinary InspectPremises

**BASE HEAD:** `3394d20e7480888dad8d7a9e3829443b6d018750` (`main` = `origin/main`, clean at start). **Version/schema:** `0.7.0-alpha.11` / `minidora-rust/0.7.0-alpha.11`. **実行日:** 2026-11-18 Asia/Tokyo. **Scope:** E1-W10の限定施工。**Tested Core source diff SHA-256:** `57c85c0d0e6626b35b442ea6f4992474861fdd8b8ff10cef2d479db8217c3ae8`.

## 実装範囲

通常日本語`InspectPremises`（例：「Neraについて、何が分かっていますか」）のCore経路を、Fact/Rule全件走査から既存Store subject index経由のbounded relation inventory projectionへ接続した。revision-matched index transactionから対象subjectの有効Factとexact/variable-subject Ruleを選び、既存の有限relation shape、relation proof projection、proof evaluatorを再利用する。Coreは引き続き一覧文面、scope、競合併記、依存ID、保証、adoptionを所有する。

- 候補選択、shape計画、proof projection/evaluationは`max_actions`内で制限する。成功と資源停止のどちらでも、選択revisionを`ResultRecord.actor_memory_revision`へ保持する。
- cold/stale indexは認証済みcurrent snapshotから一度再構築して再試行する。再構築中にheadが進んだ場合は、読込み済みimmutable snapshotを安全なfallbackに使う。
- Store schema、保存済みFact/Rule/Condition、入力parser、通常Intent/API、Core以外の採否責任は変更していない。
- Recoveryは既存のexact retained snapshot replayを継続する。この施工でRecovery自体のscanはbounded化していない。

## 観測結果

`indexed_premise_inventory_uses_subject_candidates_and_preserves_the_full_inventory`は84 Facts、1 Rule（80 unrelated Facts）を保持し、Nera用候補4 Facts+1 variable-subject Ruleを2 scopeから選ぶ。候補にはdirect premise、rule derivation、肯定/否定の競合、別scope Factを含む。indexed inventoryは10,000-action full-snapshot referenceとfacts、競合ID、依存ID、guaranteeが一致した。全snapshot `inventory_about`は同じ64-action予算では`RELATION_BUDGET`となる一方、通常日本語`Engine::execute`はCOMMITした。強制cold-index rebuildと、8-action停止時にrevision 1を記録することも確認した。

最初のfocused test実行は、自然文の最終`result.text`に内部の全inventory行が出ると仮定したassertionで失敗した。検査箇所をCoreが実際に採用した`ContentPlan.required_units`へ合わせ、専用ログに初回失敗と通過した再実行の両方を残した。Recoveryの既存`premise_inventory_recovery_pins_and_rechecks_its_memory_revision`も通過したが、これは従来どおりexact snapshot replay/invalidity checkを検証し、Recoveryのprojectionをbounded化する証拠ではない。

## 実行環境と検証

Linux x86_64 / kernel `6.18.57`、Rust `1.99.1 (58cf8f9c3 2026-10-13)`、Cargo `1.99.1 (5f94df478 2026-08-27)`、target cache `/workspace/.minidora-cloud/rust-target`。開始時HEADはGitHub `main`と一致し、working treeはcleanだった。

| Command | 結果 | 保存log |
| --- | --- | --- |
| `cargo test -p minidora-engine --locked indexed_premise_inventory_uses_subject_candidates_and_preserves_the_full_inventory -- --nocapture` | PASS: normal Japanese Core route, indexed/full inventory parity, candidate bounds, cold rebuild, 8-action revision-bearing stop | [focused Core log](b63-premise-inventory-focused-test.log) |
| `cargo test -p minidora-engine --locked premise_inventory_recovery_pins_and_rechecks_its_memory_revision -- --nocapture` | PASS: exact-revision Recovery and invalidation regression | [Recovery log](b63-premise-inventory-recovery-test.log) |
| Initial focused attempt and corrected rerun | First assertion targeted rendered surface text; corrected assertion checks adopted `ContentPlan.required_units`; subsequent run PASS | [attempts log](b63-premise-inventory-focused-attempts.log) |
| `cargo fmt --all -- --check` | PASS | [format log](b63-fmt-check.log) |
| `cargo check -p minidora-engine --all-targets --locked` | PASS | [engine check log](b63-engine-check.log) |
| `cargo clippy -p minidora-engine --lib --locked -- -D warnings` | PASS | [engine library Clippy log](b63-engine-library-clippy.log) |
| `git diff --check` | PASS | [diff-check log](b63-diff-check.log) |

This batch did not rerun the full engine package or workspace all-target Clippy. B62's latest all-target Clippy remains blocked by an unchanged `clippy::too_many_arguments` finding in `core_atomic_relation_comparison_46.rs:147`.

## CAP-D02 / W10 判定と限界

B63 closes only ordinary Japanese `InspectPremises` subject candidate selection and bounded inventory proof projection. W10 remains `PARTIAL`; `CAP-D02` remains `UNVERIFIED`. The test demonstrates an 80-Fact unrelated-history case, not an unbounded population envelope. Cold-index rebuild and concurrent-head fallback still read the loaded full snapshot. Recovery continues exact retained-snapshot replay. Derived summary/cache/model adaptation versioning, broad fresh-process/session/model-version reuse, remaining relation/learning consumers, and full CAP-D02 acceptance remain unverified. No E1 exit, release or deployment is claimed.

今回照合したD2文書SHA-256:

- `01_施工仕様書.md`: `de618da9d50f248a7e984ad807aab4a306f17204eb9e722da387aa7d587573b0`
- `02_施工工程表.md`: `d1afc8a3e225fcbc21ef39eb3152ca9c64ffe79ee61d0702220802700570ce7a`
- `03_Codex施工指示書.md`: `14bcce642e3c6281f4922e1ac788dcef6e1328451aebf82d245922cb9d901d3d`

## 生成物・資産の5分類

- **保持:** baseの1,154 tracked pathsを保持。施工ソース`rust/crates/minidora-engine/src/core.rs`を変更後も保持。
- **保存:** trace/status/requirement/observation/choiceの既存5記録を更新。B63 report・JSON・7検査logを追加保存。
- **有効:** 通常Core `InspectPremises`のrevision-matched subject candidate selectionと既存Core proof inventory。
- **廃棄:** 0 files / 0 records。移動・削除・ゴミ箱投入なし。
- **保留:** W10残consumer、CAP-D02全体受入、cold-index rebuild/fallback、Recovery scan、広い長履歴・session/model version population。

機械可読な詳細は[B63 machine observation](b63-build-observation.json)を参照する。
