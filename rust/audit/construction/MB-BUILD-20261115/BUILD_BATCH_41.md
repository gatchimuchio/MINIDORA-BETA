# BUILD_BATCH_41 — V4 compact dependency comparison

**main基線:** `08688315f3b82f0032ad4bcb16f1638438b6cb35`（B40）。

**目的:** B40で未確定だった二段rule proofのfallback境界を採取し、既存V1/V2/V3 artifactの再生を維持したまま、長いfact dependency本文を短縮して全IDを型付き参照に保持する比較consumerを追加する。

**施工範囲:** rule-bearing V4 comparison request / artifact / replayと、二段rule chainの限定integration observation。入力文法・composer上限・既存V1/V2/V3 wire形式は変更していない。`CORE-G03=PARTIAL`を維持。

## 入口からconsumerまで

隔離actor memoryに`Aster`と`Beryl`のsensor前提と二つの規則（sensor→inspected、inspected→reliable）を保存し、二対象比較を実行した。relation memoryには二つの保存規則に加えCoreが生成した合成規則があり、comparison proofは合計3規則、9 proof IDだった。proof内のderived factsについてV3形式のfact本文を再構成すると最大451 bytesとなり、composerの384-byte/unit上限を超える。同じfixtureのV4 fact本文は最大178 bytesで、exact dependency IDsは各Evidence unitの`evidence_ids`に残る。requestは上限16 unitをすべて使用した。

1. 左右のRelationInventory、derived facts、rule lineage、proof ID集合をCore memoryから計算。
2. V3の長いdependency表示がunit上限へ収まらない場合、V4 builderが本文にdependency件数を表示し、fact IDと全dependency IDをtyped `evidence_ids`へ保持。
3. 使用ruleごとのEvidence unitには前提・結論・scope・rule IDを保持し、利用可能なpredecessor rule-unit依存もtyped dependenciesに保持。
4. V4 artifact、`MeaningCompositionReport`、ContentPlanを保存・採用し、K7のrender bytesを採用本文と照合。
5. Engineを閉じて開き直し、比較resultを参照する`なぜ`追問を同じ256-action budgetで実行。V4 requestとcompositionを再生成・照合して`COMMIT`、元比較IDへの依存、`RENDERED`を確認。

比較・追問の`max_actions=256`はこのfixtureがproof展開を行うための明示的budgetである。既定32はB40の二段chain試行で`RESOURCE_LIMIT`となり、B41の最初の追問試行も32で同じ停止を再現した。ここで示すのは明示budget下の経路であり、既定budgetや全rule graphに対する資源保証ではない。

## 実行結果

- `cargo test --locked -p minidora-engine --test core_atomic_relation_comparison_41 -- --nocapture`: **PASS**（1 test）。V4、16 typed units、3 rule Evidence units、全9 proof ID、max V4 fact evidence 178 bytes、V3相当dependency本文の再構成最大451 bytes、ContentPlan、K7 render、再起動後V4追問を確認。
- `cargo test --locked -p minidora-engine --test core_atomic_relation_comparison_40 -- --nocapture`: **PASS**（1 test）。従来V3 artifact生成・rule Evidence・reopen後follow-upを確認。
- `cargo test --locked -p minidora-engine --test core_atomic_relation_comparison_38 -- --nocapture`: **PASS**（1 test）。従来V2直接比較とV1 legacy report追問を確認。
- `cargo fmt --all -- --check`: **PASS**。
- `cargo check --locked -p minidora-engine --all-targets`: **PASS**。all-target compile checkであり、全test suiteの実行を意味しない。
- `git diff --check`: **PASS**。
- 実行ログ: [`b41-focused-tests.log`](b41-focused-tests.log)。観測値: [`b41-build-observation.json`](b41-build-observation.json)。

初回B41試行では、期待rule unitを手動rule二件と決め打ちしたassertionに対し、実artifactはCore合成ruleも含む3件だったためfixture assertionを修正した。依存本文試験も比較unitではなくfact Evidence unitを選ぶよう修正した。さらに追問へ比較と同一の256-action budgetを明示した後に最終PASSとなった。これらはfixtureの観測条件を修正した記録で、Core artifactは初回試行でもV4を生成していた。

`rustc 1.99.1` / `cargo 1.99.1`、Linux。`cargo test -p minidora-engine`全suite、workspace runtime suite、Clippy、通常CLI/HTTP経路、Windows/macOS、R2-17、独立QA、release/deploymentは **NOT_RUN**。R4三文書とarchiveは変更していない。`CORE-G03=PARTIAL`を維持し、他ゲートを再判定しない。

## 資産状態

| 状態 | 個数 | 対象 |
|---|---:|---|
| keep | 7 | `core.rs`のV4 producer/replay、B41 integration test、`IMPLEMENTATION_CHOICES.md`、`IMPLEMENTATION_STATUS.md`、本BUILD記録、`b41-build-observation.json`、`b41-focused-tests.log` |
| store | 0 | 独立保管へ移動した資産なし |
| in-use | 3 | `/workspace/.minidora-cloud/rust-target`、`/workspace/.cargo`、`/workspace/.rustup`（共有build/cache/toolchain） |
| trash | 0 | 削除・破棄した資産なし |
| hold | 0 | 保留する資産なし |
| **合計** | **10** | keep 7 + store 0 + in-use 3 + trash 0 + hold 0 |

B41 temporary test directoryは最終試験後0件。コンパイル成果物と共有toolchain/cacheはin-useとして保持。生成物を別のstoreへ複製していない。

## 次consumer

V4 readerを持つ既存artifactとの実保存更新・後続version共存を別fixtureで確認し、16 unit上限でproofを保持できないケースの停止/fallback条件を記録する。今回の一つの長いdependency chainは広いrule graph受入を主張しない。
