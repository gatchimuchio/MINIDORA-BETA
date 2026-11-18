# BUILD_BATCH_59 — Goal-related projection for AskPreviousRelation

**BASE HEAD:** `8380ba7ac5bf1e66a01316f140dc6562532914f9` (`main` = `origin/main`, clean at start). **Version/schema:** `0.7.0-alpha.11` / `minidora-rust/0.7.0-alpha.11`. **実行日:** 2026-11-18 Asia/Tokyo. **Scope:** E1-W10の限定施工。**Tested Core/Recovery source diff SHA-256:** `ab9a031cef48f506059f77de84ca50ce9402093b37e47f1dcb110cda096b48da`.

## 実装範囲

通常の`AskPreviousRelation` Core経路を、既存のrevision-checked Store relation indexへ接続した。直前の同一case確定結果が参照するdependency IDだけをindexの同一transaction内で読み、直接`USER_PREMISE` Factを一意に解決する。解決したsubject/scopeのrelationからGoal-related Fact、Rule、Rule premise、condition evidence、transitive proof dependenciesを選び、既存Core proof evaluatorへ渡す。Core以外の判断主体や推論器は増設していない。

- prior dependency ID lookup、projection、proof actionを同じsealed request budgetで数える。
- CommitとResource limit stopで消費したactor-memory revisionをResultRecordへ保持する。回帰テストで低予算停止にrevisionを保存しない不具合も修正した。
- revision-matched indexが無い場合は、認証済みの現行memory snapshotからindexを一度再構築する。再構築の全snapshot読込は残る。再構築中にheadが進んだ競合経路では、ロード済みimmutable snapshotを使うfull-snapshot fallbackが残る。
- Recoveryは保存された正確なmemory revisionをfull snapshotから再実行して照合する。Recovery側の一覧・履歴投影をGoal-related selectorへ変更したとは主張しない。

## 観測結果

`indexed_previous_relation_resolves_only_prior_dependencies_before_projection`は73 Facts・1 Rule（無関係Fact 72件）のfixtureで、proof evaluatorへ1 Fact・1 Ruleだけが渡ることを確認する。支持状態とproof dependency IDsはfull-snapshot referenceと一致し、総action budget 40以下で完了する。

`previous_relation_recovery_pins_and_rechecks_its_memory_revision`は通常の日本語`Engine::execute`を通し、直前の関係結果の後に無関係Factを72件追加してから`それは鳥ですか`を実行する。回答はCommitしactor-memory revision 2を記録する。さらに3段のRule推論を11-action budgetで`AskPreviousRelation`へ通し、`SUSPEND / RESOURCE_LIMIT`にもrevision 2が保存されることを確認する。Recoveryは記録されたrevisionをEngine reopen後も読み、根拠Factを無効化した後は古い回答を再確認しない。

この回帰が直接証明するのは、通常Coreの`AskPreviousRelation`選択経路とexact-revision Recovery recheckだけである。cold-index rebuild / concurrent-write fallback、説明consumer、他のCore/Recovery消費者、広域history populationをB59で再検証したとは扱わない。

## 実行環境と検証

Linux x86_64 / kernel `6.18.57`、Rust `1.99.1 (58cf8f9c3 2026-10-13)`、Cargo `1.99.1 (5f94df478 2026-08-27)`、target cache `/workspace/.minidora-cloud/rust-target`。

| Command | 結果 | 保存log |
| --- | --- | --- |
| `cargo test -p minidora-engine --locked indexed_previous_relation_resolves_only_prior_dependencies_before_projection` | PASS: Goal projection / full-snapshot parity regression | [focused Core log](b59-focused-projection-test.log) |
| `cargo test -p minidora-engine --locked previous_relation_recovery_pins_and_rechecks_its_memory_revision` | PASS: normal Japanese route, 72 unrelated Facts, budget-stop revision 2, reopen and invalidation recheck | [focused Recovery log](b59-focused-recovery-test.log) |
| `cargo test -p minidora-engine --locked --quiet` | PASS: 268 unit tests, 37 integration cases across 33 harnesses, 0 doc tests | [full engine log](b59-engine-test.log) |
| `cargo fmt --all -- --check` | PASS | [format log](b59-format-check.log) |
| `cargo check --workspace --all-targets --locked` | PASS | [workspace check log](b59-workspace-check.log) |
| `cargo clippy -p minidora-engine --lib --locked -- -D warnings` | PASS | [engine library Clippy log](b59-engine-library-clippy.log) |
| `cargo clippy -p minidora-engine --all-targets --locked -- -D warnings` | BLOCKED, exit 101: existing `clippy::too_many_arguments` in unchanged `_45` and `_46` comparison test helpers | [all-target Clippy log](b59-engine-all-target-clippy.log) |
| `git diff --check` | PASS | [diff check log](b59-diff-check.log) |

施工中の失敗と修正も記録する。

- 最初のRecovery fixture buildは`BTreeSet` import不足でcompile error。test module importを追加した。
- noise Factを複数appendしたfixtureで、memory revisionを一時的に各Fact追加回数まで進めてStore保存が`REVISION_CONFLICT`となった。Storeの1 revision / 1 transaction契約に合わせ、保存snapshotを直前head + 1に設定した。
- budget 3のprobeはCore selectorへ入る前に外側制限で停止してrevisionを消費しなかった。推論鎖と11-action予算へ変更し、selectorの`RESOURCE_LIMIT`停止まで到達する経路で再検証した。
- 最初にselector内のbudget stopを見つけたとき、revisionをlocal ResultRecordへ設定してもstateへ保存しておらず、返却結果はrevision `None`だった。stop前に部分ResultRecordをstateへ保存し、Commit / SUSPEND双方でrevision 2を確認した。
- 最初のlibrary Clippyは`clippy::question_mark`で失敗。明示的なResult matchを`?`に直して再実行し、library ClippyはPASSした。初回Clippyのログは[保存済み](b59-initial-library-clippy.log)。

## CAP-D02 / W10 判定と限界

B59は通常`AskPreviousRelation` consumerのbounded Goal-selection sliceのみを閉じる。W10は`PARTIAL`、`CAP-D02`は`UNVERIFIED`のまま。冷間index構築では現行snapshot全体を1度読み、競合head時にはfull-snapshot fallbackを使う。Recovery全域や他consumerもGoal selectorを使うと立証していない。Cache/summary/model-adaptationの版管理、cross-process/session/model-revision利用、complete finite-subgraph、長履歴の広いresource populationも未検証。E1 exit、全体受入、release/deploymentを主張しない。

今回照合したD2文書SHA-256:

- `01_施工仕様書.md`: `de618da9d50f248a7e984ad807aab4a306f17204eb9e722da387aa7d587573b0`
- `02_施工工程表.md`: `d1afc8a3e225fcbc21ef39eb3152ca9c64ffe79ee61d0702220802700570ce7a`
- `03_Codex施工指示書.md`: `14bcce642e3c6281f4922e1ac788dcef6e1328451aebf82d245922cb9d901d3d`

## 生成物・資産の5分類

- **保持 (2 source files):** `rust/crates/minidora-engine/src/core.rs`; `rust/crates/minidora-engine/src/recovery.rs`。`main`へ直接commit/pushする。
- **保存:** B59 report / machine observation、CAP-D02 trace・status・implementation choice更新、focused 2 logs、full test log、format / workspace check / library Clippy / all-target Clippy / diff-check logs、initial library-Clippy failure log、pre-budget-stop full test logを保存する。
- **有効:** revision-matched prior-dependency and Goal projectionを通常Core `AskPreviousRelation` routeで使用可能にする。既存Store relation indexとCore proof/adoptionの責任境界を維持する。
- **廃棄:** 0 files / 0 records。移動・削除・ゴミ箱投入なし。
- **保留:** W10残consumer、CAP-D02 acceptance、cold-index rebuild費用、完全な長履歴population、all-target Clippy変更外lint。

機械可読な詳細は[b59-build-observation.json](b59-build-observation.json)を参照する。
