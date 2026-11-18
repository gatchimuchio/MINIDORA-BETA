# BUILD_BATCH_60 — Goal-related projection for ordinary AskConjunction

**BASE HEAD:** `5eb15653bff34c93162b3834257ca46068451b28` (`main` = `origin/main`, clean at start). **Version/schema:** `0.7.0-alpha.11` / `minidora-rust/0.7.0-alpha.11`. **実行日:** 2026-11-18 Asia/Tokyo. **Scope:** E1-W10の限定施工。**Tested Core source diff SHA-256:** `25d7b3a149e32feb6ebcb3ee0434decd81f538e4d3d56635389a065de35e19a8`.

## 実装範囲

通常の日本語`AskConjunction`（関係節2〜3個）を、既存Store relation indexによるbounded projectionへ接続した。各節を一つのrevision-matched `Store::with_relation_index` transaction内で射影し、節のproof結果を既存Coreの結合処理へ渡す。K1の条件評価、K2の推論、K3の採否という責任境界を維持し、独立した判断器やparser grammarは追加していない。

- 節数で分割したrequest action allowanceを、各節のprojection・proof evaluation・out-of-scope診断で共有する。
- 成功結果とprojection起因の予算停止は、使用したactor-memory revisionを`ResultRecord`に保持する。
- 節ごとにStore index上の候補だけを選び、out-of-scope Factは証明不成立時の限定診断へ渡す。既存のcondition判定、proof dependency、guarantee、停止挙動は再利用する。
- indexが欠損・古い場合の再構築は認証済みloaded snapshot全体を読む。再構築中にheadが進んだ場合はimmutable full-snapshot fallbackが残る。このbatchではその競合経路や全consumerを受入済みとは扱わない。
- Recoveryの正確なmemory revision再生とcurrent-memory proof再確認は既存経路を維持する。

## 観測結果

`indexed_conjunction_projects_each_clause_within_the_shared_budget`は76 Facts・1 Rule（無関係Fact 72件）のfixtureを使用する。3節の日本語queryでは、鉱物節に1 Fact・1 Rule、鉱石節と生物節に各1 Factだけが選ばれる。normal Japanese `Engine::execute`のproof状態、guarantee、dependency IDsはfull-snapshot referenceと一致し、無関係Factはdependenciesに入らない。

同じ回帰で、`[scope=lab]Neraは鉱物かつ希少ですか`は`SUSPEND / OUT_OF_SCOPE`となり、archive scopeのFactと最初の節の支持Fact/Rule dependenciesを保持する。`max_actions=4`の有限予算では射影経路まで進んで`SUSPEND / RESOURCE_LIMIT`となり、actor-memory revision 1を記録する。`conjunction_answer_recovery_pins_and_rechecks_its_memory_revision`はexact revisionのEngine reopen後の再生と、根拠Fact invalidation後に古い回答を再確認しない境界を確認する。既存`core_conjunctive_proof_55`もpassした。

この回帰が直接確認する範囲は通常の日本語`AskConjunction`、節別projectionとproof parity、limited out-of-scope診断、budget-stop revision、既存Recovery recheckである。冷間index再構築と競合fallbackの資源上限、Recovery全consumer、他のCore/learning consumerをB60で検証したとは扱わない。

## 実行環境と検証

Linux x86_64 / kernel `6.18.57`、Rust `1.99.1 (58cf8f9c3 2026-10-13)`、Cargo `1.99.1 (5f94df478 2026-08-27)`、target cache `/workspace/.minidora-cloud/rust-target`。

| Command | 結果 | 保存log |
| --- | --- | --- |
| `cargo test -p minidora-engine --locked indexed_conjunction_projects_each_clause_within_the_shared_budget -- --nocapture` | PASS: 3節projection、full-snapshot parity、out-of-scopeとbudget stop | [focused Core log](b60-focused-test.log) |
| `cargo test -p minidora-engine --locked conjunction_answer_recovery_pins_and_rechecks_its_memory_revision -- --nocapture` | PASS: exact revision reopen replayとinvalidation recheck | [Recovery log](b60-conjunction-recovery-test.log) |
| `cargo test -p minidora-engine --test core_conjunctive_proof_55 --locked -- --nocapture` | PASS: existing conjunction proof integration | [proof log](b60-conjunctive-proof-test.log) |
| `cargo test -p minidora-engine --locked` | PASS: 269 unit tests、37 integration cases / 33 harnesses、0 doc tests | [full engine log](b60-engine-tests.log) |
| `cargo fmt --all -- --check` | PASS | [format log](b60-format.log) |
| `cargo check --workspace --all-targets --locked` | PASS | [workspace check log](b60-workspace-check.log) |
| `cargo clippy -p minidora-engine --lib --locked -- -D warnings` | PASS | [engine library Clippy log](b60-engine-library-clippy.log) |
| `cargo clippy -p minidora-engine --all-targets --locked -- -D warnings` | BLOCKED, exit 101: unchanged `clippy::too_many_arguments` in comparison test helpers `_45` and `_46` | [all-target Clippy log](b60-all-target-clippy.log) |
| `git diff --check` | PASS | [diff-check log](b60-diff-check.log) |

実装中のiteration evidenceも保持する。初回の`max_actions=2` probeはCoreの節projectionへ達する前にscheduler minimumで停止したため、`b60-focused-test-initial-failure.log`を残し、projectionまで進むbudget 4で確認した。先行する2節fixtureのfocused/full-suite passは`b60-focused-test-two-clause-pass.log`と`b60-engine-tests-two-clause-baseline.log`に保存し、最終fixtureを3節へ拡張してfocused testと全engine suiteを再実行した。all-target Clippyの初回iteration logも、最終logとともに保存した。

## CAP-D02 / W10 判定と限界

B60は通常`AskConjunction` consumerのbounded relation projection sliceのみを閉じる。W10は`PARTIAL`、`CAP-D02`は`UNVERIFIED`のまま。cold-index rebuild時にはloaded snapshot全体を一度読み、競合head時にはfull-snapshot fallbackがある。他のrelation/learning consumer、derived index/cache/summary/model-adaptationの版管理、complete finite-subgraph、fresh-process/session/model-revision利用、長履歴の広いresource populationは未確認。E1 exit、全体受入、release/deploymentは主張しない。

今回照合したD2文書SHA-256:

- `01_施工仕様書.md`: `de618da9d50f248a7e984ad807aab4a306f17204eb9e722da387aa7d587573b0`
- `02_施工工程表.md`: `d1afc8a3e225fcbc21ef39eb3152ca9c64ffe79ee61d0702220802700570ce7a`
- `03_Codex施工指示書.md`: `14bcce642e3c6281f4922e1ac788dcef6e1328451aebf82d245922cb9d901d3d`

## 生成物・資産の5分類

- **保持 (1,110 tracked paths):** baseから変更のない1,109 pathsと、更新した`rust/crates/minidora-engine/src/core.rs` 1 path。
- **保存 (19 tracked paths):** 更新したCAP-D02 trace/status/requirements/machine observationの4 pathsと、新規report・JSON・13 B60 logsの15 paths。
- **有効:** revision-matched clause projectionを通常Core `AskConjunction` routeで使用可能にする。Store indexとCore K1/K2/K3責務を維持する。
- **廃棄:** 0 files / 0 records。移動・削除・ゴミ箱投入なし。
- **保留:** W10残consumer、CAP-D02 acceptance、cold-index rebuild/fallback costs、wide history population、変更外all-target Clippy findings。

機械可読な詳細は[b60-build-observation.json](b60-build-observation.json)を参照する。
