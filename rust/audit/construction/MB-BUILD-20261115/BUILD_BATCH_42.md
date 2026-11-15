# BUILD_BATCH_42 — comparison unit-limit fallback

**main基線:** `82763c4eb6cfd9d980b415b31582ff7936cb6d19`（B41）。

**目的:** B41のV4 reader・再起動後consumer確認に続き、typed comparison候補が16-unit上限を超えるfixtureで、旧V1 report fallbackがinventory proofを保持して後続consumerまで届くかを観測する。

**施工範囲:** engine integration testと観測記録のみ。production code、composer上限、artifact形式、R4三文書は変更していない。`CORE-G03=PARTIAL`を維持。

## 入口からconsumerまで

隔離actor memoryへ単一rule（`sensor`→`reliable`）と左右6件の直接relation（各主体に3件）を保存した。比較inventoryには6件の直接factと2件の導出fact、5つのunique relation key、全9 proof ID（fact 8件・rule 1件）が含まれた。比較と再起動後追問にはfixtureを展開する明示的`max_actions=256`を使用した。

1. actor-scoped RelationInventoryから左右のfact、relation key、contested IDs、proof/dependency IDsを読み、全9 IDを照合。
2. 現行builderのunit構成に沿って候補数を再構成。V2はopening/subject 3 + relation key 5 + fact 8 + proof-reference 0 + limitation 1 = **17 units**。V3/V4は使用rule Evidenceを1件加えて**18 units**。両候補はcomposer上限16を超える。最長のV2形式fact evidence本文は309 bytesで、384-byte/unit上限以下。
3. 実際に保存されたartifactが`minidora.relation-comparison-report.v1`であること、`content_request`を持たないV1 report経路であること、左右inventoryに候補作成元の全9 proof IDが保持されることを確認。
4. V1 reportをContentPlan/K7へ送り、保存report hash・実render bytes・sha256一致と`RENDERED`を確認。
5. Engineを閉じて再openし、同じ256-action budgetで`なぜ`追問を実行。追問は元比較result IDに依存し、`COMMIT`と`RENDERED`を確認。

unit候補数はfixtureの保存inventoryと現行builder構造からテスト内で再構成した値であり、内部builder branchの直接計測ではない。観測は一つのoverflow形状に限る。V1表面での規則文表示、他のunit/byte/evidence overflow組合せ、一般的内容妥当性、現実世界の真偽は確認していない。

## 実行結果

- `cargo test --locked -p minidora-engine --test core_atomic_relation_comparison_42 -- --nocapture`: **PASS**（1 test）。17/18 candidate units、309-byte最大V2 fact本文、V1 report artifact、全9 inventory proof ID、K7 render、Engine reopen後follow-upを確認。
- `cargo fmt --all -- --check`: 最終確認結果を下記ログへ記録。
- `cargo check --locked -p minidora-engine --all-targets`: 最終確認結果を下記ログへ記録。compile checkであり全test suite実行ではない。
- `git diff --check`: 最終確認結果を下記ログへ記録。
- 観測値: [`b42-build-observation.json`](b42-build-observation.json)。実行記録: [`b42-focused-tests.log`](b42-focused-tests.log)。

`rustc 1.99.1 (58cf8f9c3 2026-10-13)` / `cargo 1.99.1 (5f94df478 2026-08-27)`、Linux。full engine/workspace test suites、Clippy、通常CLI/HTTP path、Windows/macOS、R2-17、独立QA、release/deploymentは **NOT_RUN**。R4三文書は変更していない。`CORE-G03=PARTIAL`を維持し、他gateを再判定しない。

## 資産状態

| 状態 | 個数 | 対象 |
|---|---:|---|
| keep | 6 | B42 integration test、`IMPLEMENTATION_CHOICES.md`、`IMPLEMENTATION_STATUS.md`、本BUILD記録、`b42-build-observation.json`、`b42-focused-tests.log` |
| store | 0 | 独立保管へ移動した資産なし |
| in-use | 3 | `/workspace/.minidora-cloud/rust-target`、`/workspace/.cargo`、`/workspace/.rustup`（共有build/cache/toolchain） |
| trash | 0 | 削除・破棄した資産なし |
| hold | 0 | 保留する資産なし |
| **合計** | **9** | keep 6 + store 0 + in-use 3 + trash 0 + hold 0 |

B42 temporary test directoryはtestのDrop後0件。生成物を別のstoreへ複製していない。

## 次consumer

V1 reportを使うunit overflowに限った次観測を終えた。次は保存済みversionの異なるcomparison artifactが同一memory上で共存するときのreader/replayを有限fixtureで確認し、未確認のoverflow停止条件と分けて記録する。全rule graph・全composer境界の受入は主張しない。
