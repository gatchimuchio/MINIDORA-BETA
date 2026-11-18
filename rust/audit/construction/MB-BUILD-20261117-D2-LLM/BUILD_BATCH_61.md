# BUILD_BATCH_61 — Goal-related candidate selection for ordinary AskCommonRelations

**BASE HEAD:** `62f39338517d74152e787615a5fd0f0243d39735` (`main` = `origin/main`, clean at start). **Version/schema:** `0.7.0-alpha.11` / `minidora-rust/0.7.0-alpha.11`. **実行日:** 2026-11-18 Asia/Tokyo. **Scope:** E1-W10の限定施工。**Tested Core/Store source diff SHA-256:** `3b8575e05f3892299fa87768eb5120eeec105607b73bb08f0a5e624d95297c4d`.

## 実装範囲

通常の日本語`AskCommonRelations`で、Fact/Rule全件走査による候補生成をStoreのsubject/scope index selectionへ置き換えた。選んだ候補形状を、左右それぞれ既存のrelation proof projectionへ渡す。片側ずつ同じrevision-matched Store index transactionを読み、既存のK1条件評価、K2推論、K3採否とCommonRelations report/adoptionを維持する。parser grammar、parser version、外部API、別の判断主体は追加していない。

- `memory_index_common_subject_scope(actor_id, valid, scope, subject, record_kind)`を派生indexとして追加した。Coreは正確なscopeと2つのsubjectに一致するFact/Rule、ならびにvariable-subject Ruleを選び、Ruleのpremise・additional premise・conclusionとFactから比較候補を作る。
- 候補選択のrecord allowanceと両対象へのprojection/proof allowanceをrequestの有限`max_actions`内に割り当てる。どのcandidate/proofが予算を超えたかを`RESOURCE_LIMIT`として停止する。
- 成功結果とindexed budget stopは、選択したindex revisionを`ResultRecord.actor_memory_revision`に保持する。
- 冷間またはstale indexは認証済みloaded snapshotから一度再構築してretryする。再構築中にmemory headが進んだ場合は、そのrequestで取得済みのimmutable snapshotをfallbackに使う。Recoveryは従来どおり保存revision全体でexact replayし、現行headも再確認する。

## 観測結果

`indexed_common_relations_use_bounded_subject_candidates_and_preserve_artifacts`は82 Facts、1 Rule、うち無関係Fact 80件を保存する。selectorは2つのnamed-subject Factsと1つのvariable-subject Ruleだけを返し、比較候補は2形状（`rock`と`mineral`）となる。通常の`[scope=lab]NeraとMikoの共通点は何ですか`は64 actionsでcommitし、Nera/MikoのFactとRule以外をdependenciesに含めない。report text、canonical artifact bytes、proof dependency IDs、guaranteeは1万actionのfull-snapshot referenceと一致する。

同じ64-action allowanceで従来のfull-snapshot関数は83件のFact/Rule走査時点で`RESOURCE_LIMIT`となる。通常Core routeは候補3件をindexから選択し、2候補×左右のproof queryに各15 actionsを割り当てて完了する。3-action requestはschedulerの必要成果minimumを満たしたうえでindexed candidate selectionまで進み、`RESOURCE_LIMIT`停止時にactor-memory revision 1を記録する。冷間indexを強制的に消した後の通常route成功も確認した。

`common_relations_recovery_pins_and_rechecks_its_memory_revision`は既存Recoveryがrevisionを再生し、premise invalidation後に古い回答を再確認しないことを確認する。`natural_common_relation_questions_compare_two_subjects_using_shared_proofs`は既存CLI正常系を確認する。この変更はAskCommonRelationsの候補選択sliceだけを閉じ、冷間index再構築や並行head fallbackの全件snapshot readをなくすものではない。

## 実行環境と検証

Linux x86_64 / kernel `6.18.57`、Rust `1.99.1 (58cf8f9c3 2026-10-13)`、Cargo `1.99.1 (5f94df478 2026-08-27)`、target cache `/workspace/.minidora-cloud/rust-target`。

| Command | 結果 | 保存log |
| --- | --- | --- |
| `cargo test -p minidora-engine --lib indexed_common_relations_use_bounded_subject_candidates_and_preserve_artifacts --locked -- --nocapture` | PASS: indexed selection、normal Japanese Core route、snapshot/artifact parity、cold rebuild、budget stop | [focused Core log](b61-focused-test.log) |
| `cargo test -p minidora-engine --lib common_relations_recovery_pins_and_rechecks_its_memory_revision --locked -- --nocapture` | PASS: exact revision replay and stale-answer invalidation check | [Recovery log](b61-common-relations-recovery-test.log) |
| `cargo test -p minidora-cli --test normal_route natural_common_relation_questions_compare_two_subjects_using_shared_proofs --locked -- --exact` | PASS: ordinary Japanese CLI common-relations consumer | [CLI log](b61-normal-cli-common-relations-test.log) |
| `cargo test -p minidora-engine --locked` | PASS: 270 unit tests, 37 integration cases across 33 harnesses, 0 doc tests | [full engine log](b61-engine-tests.log) |
| `cargo fmt --all -- --check` | PASS | [format log](b61-format-check.log) |
| `cargo check --workspace --all-targets --locked` | PASS | [workspace check log](b61-workspace-check.log) |
| `cargo clippy -p minidora-engine --lib --locked -- -D warnings` | PASS | [engine library Clippy log](b61-engine-library-clippy-pass.log) |
| `cargo clippy -p minidora-engine --all-targets --locked -- -D warnings` | BLOCKED, exit 101: unchanged `clippy::too_many_arguments` in comparison test helpers `_45` and `_46` | [all-target Clippy log](b61-all-target-clippy.log) |
| `git diff --check` | PASS | [diff-check log](b61-diff-check.log) |

Iteration evidence is retained. A `max_actions=2` probe stopped at the scheduler's required-product minimum before reaching relation selection; the final fixture uses 3 actions and verifies a `RESOURCE_LIMIT` at indexed selection with revision 1 ([initial budget log](b61-initial-budget-stop.log)). Initial library Clippy found two new helper argument-count warnings; a typed replay-input struct removed them and final warnings-denied library Clippy passed ([initial Clippy log](b61-initial-engine-library-clippy.log)).

## CAP-D02 / W10 判定と限界

B61 closes only ordinary `AskCommonRelations` Goal-related candidate selection. W10 remains `PARTIAL`; `CAP-D02` remains `UNVERIFIED`. Cold/stale index recovery still loads the full current snapshot once; a concurrent head advance uses immutable full-snapshot fallback. Exact-snapshot Recovery intentionally retains its existing full replay. Other relation/learning consumers, separately versioned derived indexes/caches/summaries/model adaptations, complete finite-subgraph selection, broad fresh-process/session/model-version reuse, large-history populations and full CAP-D02 acceptance remain unverified. No E1 exit, release or deployment is claimed.

今回照合したD2文書SHA-256:

- `01_施工仕様書.md`: `de618da9d50f248a7e984ad807aab4a306f17204eb9e722da387aa7d587573b0`
- `02_施工工程表.md`: `d1afc8a3e225fcbc21ef39eb3152ca9c64ffe79ee61d0702220802700570ce7a`
- `03_Codex施工指示書.md`: `14bcce642e3c6281f4922e1ac788dcef6e1328451aebf82d245922cb9d901d3d`

## 生成物・資産の5分類

- **保持 (1,129 tracked paths):** baseから変更のない1,127 pathsと、更新した`core.rs`・`store.rs`の2 paths。
- **保存 (18 tracked paths):** 更新したstatus/choices/trace/requirements/machine observationの5 pathsと、新規B61 report・JSON・11 logsの13 paths。
- **有効:** 通常Core `AskCommonRelations`のrevision-matched indexed candidate/proof route。Rust build cacheは`/workspace/.minidora-cloud/rust-target`。
- **廃棄:** 0 files / 0 records。移動・削除・ゴミ箱投入なし。
- **保留:** W10残consumerとCAP-D02全体受入、cold-index rebuild/fallback cost、長履歴population、model/cache versioning。

機械可読な詳細は[B61 machine observation](b61-build-observation.json)を参照する。
