# BUILD_BATCH_58 — Goal-related projection for ordinary AskRelationObjects

**BASE HEAD:** `465fb86aa784a8183474d4501df47fbe171ad959` (`main` = `origin/main`, clean before this batch). **Version/schema:** `0.7.0-alpha.11` / `minidora-rust/0.7.0-alpha.11`. **実行日:** 2026-11-18 Asia/Tokyo. **Scope:** E1-W10の限定施工。

## 実装範囲

通常の日本語`AskRelationObjects` Core経路を、既存Storeのrevision-checked relation indexへ接続した。Goal propositionから選択したFact/Rule/condition/dependency projectionを、既存のCore relation-object evaluatorに渡す。候補生成、projection、proof queryは要求の有限action budget内で実行する。判断主体、推論機械、保存責任は増設していない。

- index revisionがActor memory headと一致する場合だけindexを使う。欠損・古いindexは認証済みの現行snapshotから一度再構築する。再構築時の全snapshot読込は残る。
- 成功結果とrelation-object予算停止の両方で、使用したActor memory revisionを`ResultRecord`に残す。
- 回帰fixtureは64 Facts、1 Rule（うち60 Factsは無関係）を保存する。通常の日本語`Engine::execute` queryで、Goal closureの2 Facts・1 Ruleだけを選び、full-snapshot referenceとtext、guarantee、dependenciesが一致する。
- action budget 20では従来のfull scanは`RESOURCE_LIMIT`となる一方、Goal projectionはCOMMITする。予算4では投影で`RESOURCE_LIMIT`となっても停止結果にrevision 1を保持する。index冷間再構築を強制し、Fact/Rule tombstone後の次queryがrevision 2を記録してSUSPENDし、前回の回答とdependenciesを再利用しないことも確認した。

## 検証記録

実行環境はLinux x86_64、Rust `1.99.1 (58cf8f9c3 2026-10-13)`、Cargo `1.99.1 (5f94df478 2026-08-27)`。実装Rust source diffのSHA-256は`5778cb992a5fd2027d66ab6cd66809c830154961a89941b1e1204450ceb57cf8`。

| Command | 結果 | 保存log |
| --- | --- | --- |
| `cargo test -p minidora-engine --locked indexed_relation_object_query_uses_only_the_goal_and_rule_closure -- --nocapture` | PASS: focused Core regression | [b58-focused-test.log](b58-focused-test.log) |
| `cargo test -p minidora-engine --locked --quiet` | PASS: 267 unit tests、37 integration tests / 33 harnesses、0 doc tests | [b58-engine-test.log](b58-engine-test.log) |
| `cargo fmt --all -- --check` | PASS | [b58-format-check.log](b58-format-check.log) |
| `cargo check --workspace --all-targets --locked` | PASS | [b58-workspace-check.log](b58-workspace-check.log) |
| `cargo clippy -p minidora-engine --lib --locked -- -D warnings` | PASS | [b58-engine-library-clippy.log](b58-engine-library-clippy.log) |
| `cargo clippy -p minidora-engine --all-targets --locked -- -D warnings` | BLOCKED: 変更外の比較test helper `_45` / `_46` に`clippy::too_many_arguments` | [b58-engine-all-target-clippy.log](b58-engine-all-target-clippy.log) |
| `git diff --check` | PASS | [b58-diff-check.log](b58-diff-check.log) |

施工中に検出した失敗と修正は記録を保持する。最初のtest buildは新fixtureが`Result`をunwrapせずに参照していたためcompile errorとなり、修正後のfocused testはpassした。次の回帰testでは無効化後のSUSPEND結果にmemory revisionが記録されない欠陥が見つかり、stop前に部分`ResultRecord`を保存して解消した。予算1の追加probeはCore queryに入る前に外側の必須処理予算で`ACTION_BUDGET_EXHAUSTED`となったため、relation projectionまで届く予算4で再試験した。最初のlibrary Clippyでは今回追加したhelper二つの引数数が閾値を超えたため、既存`RelationObjectsReplay`を使う形にまとめ、最終library Clippyはpassした。失敗時のtestログは[build failure](b58-initial-focused-build-failure.log)と[revision stop failure](b58-initial-stop-revision-failure.log)に残す。

## 境界と未確認事項

このbatchが示すのは通常Coreの`AskRelationObjects`に対するbounded Goal-related projectionの限定sliceだけ。cold-index rebuildは現行snapshot全体を一度読む。Recoveryの一覧再計算、他のCore consumer、derived summary/cache/model adaptationの版管理、fresh process/session/model revisionをまたぐ利用、大規模historyのresource populationは未確認。E1-W10は`PARTIAL`、`CAP-D02`は`UNVERIFIED`のまま。E1 exit、全体受入、release/deploymentは主張しない。

## 成果物の取扱い

- **保持:** Coreの実装変更を既存`main`に保存する。
- **保存:** 本batch記録、機械可読観測、検証log、修正前の失敗記録を保持する。
- **有効:** revision-checked projectionを通常の`AskRelationObjects` Core経路で使用可能にする。
- **廃棄:** 本batchではファイル、履歴、evidenceを削除・ゴミ箱移動しない。
- **保留:** W10の残scope、`CAP-D02`受入、all-target Clippyの変更外lintは未完了。

機械可読な詳細は[b58-build-observation.json](b58-build-observation.json)を参照する。
