# BUILD_BATCH_57 — Goal-related relation projection for typed Ask/AskWhy

**BASE HEAD:** `62f411fe99be4f993e385ac37c6030a69dbc1653` (`main` = `origin/main`, clean before this batch). **Version/schema:** `0.7.0-alpha.11` / `minidora-rust/0.7.0-alpha.11`. **実行日:** 2026-11-18 UTC.

## 実装範囲

E1-W10の限定施工として、通常のtyped Core `Ask` / `AskWhy` relation queryにGoal関連recordの選択を追加した。別の判断主体は設けず、Storeのrevision整合済みrelation indexを読み取り用projectionとして使う。条件評価・proof・採否は既存CoreのK1/K2/K3に残す。

- Storeにcurrent-state relation indexを追加。record payloadのchecksumと、Fact/Rule/Conditionの検索キーを保持する。
- `save_memory` のrevision publishとindex append/tombstone更新を同じSQLite transactionで行う。index revisionがActor headと一致しなければ利用しない。
- CoreのAsk/AskWhyはquery propositionからdirect evidence、適用可能なRule、そのpremise形、proof dependency、condition定義とcondition evidenceを選び、projectionとproofに同じaction budgetを使う。
- Scope外の直接Factは、通常proofが`UNKNOWN`になったときだけ残予算内で調べる。
- 既存DBにindexがない場合、認証済みcurrent snapshotから一度indexを再構築してから照会する。冷間再構築時にsnapshot全体を読むコストはこのbatchでは除去していない。

回帰fixtureは68 Facts、2 Rules、1 Conditionを保存し、condition-gated queryが2 Facts、1 Rule、1 Conditionを選ぶこと、full-snapshot referenceとproof dependencyが一致することを確認する。通常の日本語`Engine::execute` Ask、24件のscope外Factがある状態でのaction budget 4のsupported query、cold-index rebuild、incremental premise tombstone後の`UNKNOWN`も確認した。

## 検証記録

実行環境はLinux x86_64、Rust `1.99.1 (58cf8f9c3 2026-10-13)`、Cargo `1.99.1 (5f94df478 2026-08-27)`。変更したRust source diffのSHA-256は`343b78f3e4fac98ae671e79c9d961055b46fb0c23524d2ce438e7629c9858d39`。

| Command | 結果 | 保存log |
| --- | --- | --- |
| `cargo test -p minidora-engine --locked --quiet` | PASS: 266 unit tests、37 integration tests / 33 harnesses、0 doc tests | [b57-engine-test.log](b57-engine-test.log) |
| `cargo fmt --all -- --check` | PASS | [b57-format-check.log](b57-format-check.log) |
| `cargo check --workspace --all-targets --locked` | PASS | [b57-workspace-check.log](b57-workspace-check.log) |
| `cargo clippy -p minidora-engine --lib --locked -- -D warnings` | PASS | [b57-engine-lib-clippy.log](b57-engine-lib-clippy.log) |
| `cargo clippy -p minidora-engine --all-targets --locked -- -D warnings` | BLOCKED: 既存のtest helper `_45` / `_46` に`clippy::too_many_arguments` | [b57-engine-all-target-clippy.log](b57-engine-all-target-clippy.log) |
| `git diff --check` | PASS | [b57-diff-check.log](b57-diff-check.log) |

自動生成ログをリポジトリに残すため、`.gitignore`の`*.log`規則に一致するB57ログは明示的に追跡対象へ追加する。

## 境界と未確認事項

このbatchが示すのは、通常typed Core Ask/AskWhyにおけるGoal関連subgraph選択の限定sliceだけ。冷間index構築はcurrent snapshot全体を一度読む。他のCore consumerとRecovery、relation-profile/list inventory、cache/summary/model adaptationの版管理、fresh-process/session/model-versionをまたぐ利用、大規模historyのresource populationは未確認である。したがってE1-W10は`PARTIAL`、`CAP-D02`は`UNVERIFIED`のまま。E1 exit、全体受入、release/deploymentは主張しない。

## 成果物の取扱い

- **保持:** Store/Coreの実装変更をmainのソースとして保存する。
- **保存:** 本batch記録、機械可読観測、検証logを監査成果物として保存する。
- **有効:** Ask/AskWhyのGoal関連projection sliceを現行Coreで使用可能にする。
- **廃棄:** 本batchではファイル、履歴、evidenceを削除しない。
- **保留:** W10の残り境界、CAP-D02の受入、all-target Clippyの既存lint解消は未完了。

機械可読な詳細は[b57-build-observation.json](b57-build-observation.json)を参照。
