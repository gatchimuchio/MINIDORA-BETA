# BUILD_BATCH_62 — Goal-related candidate selection for ordinary DraftRelationProfile

**BASE HEAD:** `6809f31fad1eeacee85e619d220e6757e032da96` (`main` = `origin/main`, clean at start). **Version/schema:** `0.7.0-alpha.11` / `minidora-rust/0.7.0-alpha.11`. **実行日:** 2026-11-18 Asia/Tokyo. **Scope:** E1-W10の限定施工。**Tested Core/Store source diff SHA-256:** `cb74c456634bd56acab170e9512ce6edc98e0e637be3df00e0322bc8ec1a6acf`.

## 実装範囲

通常の日本語`DraftRelationProfile`で、Fact/Rule全件を走査してプロフィール候補を作る経路をStoreのsubject索引へ置き換えた。Storeは対象subjectのactive Factと、同subjectまたはvariable-subjectのRuleを一つのrevision-matched index transactionから選ぶ。選択候補から既存の有限relation shapeを作り、各shapeを既存`active_relation_projection`とCoreのproof evaluatorへ通す。プロフィール本文・競合併記・依存ID・HYPOTHESIS保証・model候補の作成と採用は従来どおりCoreが所有する。

- 派生索引`memory_index_profile_subject(actor_id, valid, subject, record_kind)`を追加。永続memory schema、Fact/Condition ID、保存済みpayload、parser grammar/version、外部API、判断主体は変更していない。
- subject candidate選択、relation shape計画、各proof projection、proof evaluationが同じ`max_actions`予算を共有する。予算超過は`RESOURCE_LIMIT`で停止し、選択revisionを正常COMMITと予算停止の両方で`ResultRecord.actor_memory_revision`へ保持する。
- indexがcold/staleなら認証済みcurrent snapshotから一度再構築してretryする。再構築中にheadが進んでindexを読めない場合は、既に取得したimmutable snapshotで従来の全snapshot referenceを使用する。
- Recoveryは従来のexact retained snapshot replayを維持する。この施工でRecoveryの走査はbounded化していない。

## 観測結果

`indexed_relation_profile_uses_bounded_subject_candidates_and_preserves_inventory`は84 Facts、1 Rule（うち無関係Fact 80件）を保存する。subject selectorはNeraのFact 4件とvariable-subject Rule 1件を返し、`lab`/`archive`の5 relation shapesを既存proof projectionへ渡す。対象には直接Fact、派生Fact、同一scopeの肯定/否定競合、別scopeのFactが含まれる。

同じ64-action allowanceで全snapshot referenceは85 Fact/Ruleを走査して`RELATION_BUDGET`となる一方、通常日本語プロフィール経路はCOMMITする。indexed routeのrequired units、依存ID、競合扱い、保証は10,000-action full-snapshot `relation_profile_answer`と一致し、80無関係Factは依存に入らない。cold indexを明示的に削除した後の通常route再構築も通る。8-actionのCore budget stopは`RESOURCE_LIMIT`となり、actor-memory revision 1を結果へ保持する。

既存の`relation_profile_recovery_pins_and_rechecks_its_memory_revision`と`relation_profile_explanation_pins_and_rechecks_its_memory_revision`はexact revision replayとFact/Rule invalidation後のconfirmation拒否を確認する。通常CLI testはprofile結果を後続のprevious-result consumerへ渡す。

## 実行環境と検証

Linux x86_64 / kernel `6.18.57`、Rust `1.99.1 (58cf8f9c3 2026-10-13)`、Cargo `1.99.1 (5f94df478 2026-08-27)`、target cache `/workspace/.minidora-cloud/rust-target`。開始時HEADはGitHub `main`と一致し、working treeはcleanだった。開始時の復旧refはlocal `refs/heads/codex/backup-main=6809f31`、`refs/heads/codex/backup-main-prev=b8429c4`。既存のremote backup refsとtagは変更していない。

| Command | 結果 | 保存log |
| --- | --- | --- |
| `cargo test -p minidora-engine --lib indexed_relation_profile_uses_bounded_subject_candidates_and_preserves_inventory --locked -- --nocapture` | PASS: indexed candidate count、full-snapshot parity、normal Japanese Core route、cold rebuild、revision-bearing budget stop | [focused Core log](b62-focused-core-test.log) |
| `cargo test -p minidora-engine relation_profile --locked` | PASS: 5 filtered unit tests。Core selector、profile/explanation Recovery、profile output surface、input | [profile/Recovery log](b62-profile-recovery-tests.log) |
| `cargo test -p minidora-cli --test normal_route natural_relation_profile_uses_current_proofs_and_feeds_a_follow_up_consumer --locked -- --exact` | PASS: ordinary Japanese CLI profile and next consumer | [CLI log](b62-normal-cli-profile-test.log) |
| `cargo test -p minidora-engine --locked` | PASS: 271 unit tests, 37 integration cases across 33 harnesses, 0 doc tests | [full engine log](b62-engine-tests.log) |
| `cargo fmt --all -- --check` | PASS | [format log](b62-format-check.log) |
| `cargo check --workspace --all-targets --locked` | PASS | [workspace check log](b62-workspace-check.log) |
| `cargo clippy -p minidora-engine --lib --locked -- -D warnings` | PASS | [engine library Clippy log](b62-engine-library-clippy.log) |
| `cargo build --workspace --locked` | PASS | [workspace build log](b62-workspace-build.log) |
| `cargo clippy --workspace --all-targets --locked -- -D warnings` | BLOCKED, exit 101: unchanged `clippy::too_many_arguments` in `core_atomic_relation_comparison_46.rs:147` | [all-target Clippy log](b62-all-target-clippy.log) |
| `git diff --check` | PASS | [diff-check log](b62-diff-check.log) |

## CAP-D02 / W10 判定と限界

B62 closes only ordinary Japanese `DraftRelationProfile` subject candidate selection and proof projection. W10 remains `PARTIAL`; `CAP-D02` remains `UNVERIFIED`. All variable-subject Rules remain profile candidates across scopes and are themselves bounded by `max_actions`. Cold/stale index rebuilding still reads the full current snapshot once; concurrent-head fallback uses that loaded snapshot. Recovery intentionally still replays the exact retained snapshot. Other relation and learning consumers, derived summaries/caches/model adaptations, broad fresh-process/session/model-version consumption, resource envelopes and full CAP-D02 acceptance remain unverified. No E1 exit, release or deployment is claimed.

今回照合したD2文書SHA-256:

- `01_施工仕様書.md`: `de618da9d50f248a7e984ad807aab4a306f17204eb9e722da387aa7d587573b0`
- `02_施工工程表.md`: `d1afc8a3e225fcbc21ef39eb3152ca9c64ffe79ee61d0702220802700570ce7a`
- `03_Codex施工指示書.md`: `14bcce642e3c6281f4922e1ac788dcef6e1328451aebf82d245922cb9d901d3d`

## 生成物・資産の5分類

- **保持:** baseの1,142 tracked pathsのうち1,134 pathsは不変。施工コード3 paths（`core.rs`, `relations.rs`, `store.rs`）は変更後も保持。
- **保存:** 既存のstatus/choice/requirement/trace/evidence記録5 pathsを更新。B62 report・JSON・検査log 10 pathsを新規保存。
- **有効:** 通常Core `DraftRelationProfile`のrevision-matched subject selection/proof projection。Rust build cacheは`/workspace/.minidora-cloud/rust-target`。
- **廃棄:** 0 files / 0 records。移動・削除・ゴミ箱投入なし。
- **保留:** W10残consumer、CAP-D02全体受入、cold-index rebuild/fallback、Recovery走査、広い長履歴・session/model version population。

機械可読な詳細は[B62 machine observation](b62-build-observation.json)を参照する。
