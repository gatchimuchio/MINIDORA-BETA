# BUILD_BATCH_40 — B39 V3規則比較の実行確認

**main基線:** `a130d32eca93fac0b57747a4448646d9324346d0`（B39）。

**目的:** B39で追加した保存規則つき比較artifactを実際に生成し、typed composition、ContentPlan、K7描画、Engine再起動後の追問再生まで確認する。

**施工範囲:** B40の限定integration observationを追加。Core実装、入力文法、保存形式、他の機能は変更していない。

**ゲート:** `CORE-G03 = PARTIAL`を維持。観測は一つの保存規則・二対象・一scopeに限られる。

## 入口からconsumerまで

隔離したactor memoryに次を保存し、既定development budgetで型付き比較を実行した。

`Aster is a sensor`、`Beryl is a sensor`、`X is a sensor → X is reliable`を保存し、次の経路を実行した。

1. 左右のRelationInventoryとproof IDを再計算。
2. `minidora.relation-comparison-content.v3` artifactを生成。
3. 保存規則の前提・結論・scope・規則IDをEvidence unitとして`MeaningCompositionRequest`に保持。
4. `MeaningCompositionReport`・ContentPlanを採用。
5. K7が`RENDERED`を返す。
6. Engineを閉じて開き直し、比較結果を参照する`なぜ`追問でV3 requestとcompositionを再生成・照合。

B40のテストはartifactと再生成された左右inventoryから完全なproof ID集合を組み立て、`content_composition.evidence_ids`と全unitの`evidence_ids`の双方に同じ集合が残ることを確認する。描画bytesは採用ContentPlan本文と一致し、render hashも検証する。追問は`COMMIT`し、元比較result IDへの依存と新しい`RENDERED`結果を返す。

## 実行結果

- `cargo test --locked -p minidora-engine --test core_atomic_relation_comparison_40 -- --nocapture`: **PASS**（1 test）。V3 artifact、規則Evidence、proof ID、ContentPlan、K7描画、Engine再起動後の追問を確認。
- `cargo test --locked -p minidora-engine --test core_atomic_relation_comparison_38 -- --nocapture`: **PASS**（1 test）。V2直接前提比較、競合比較、再起動後V2追問とV1 report追問を回帰確認。
- `cargo fmt --all -- --check`: **PASS**。
- `cargo check --locked -p minidora-engine --all-targets`: **PASS**。test targetを含むcompile checkで、他のtest case実行を意味しない。
- `git diff --check`: **PASS**。
- 実行ログ: [`b40-focused-tests.log`](b40-focused-tests.log)。初期stress fixtureの失敗も、経緯を保つため削除せず同ログに記録した。

### fixture境界の観測

二段の規則連鎖を試した初回は、既定の`max_actions=32`で関係proof展開が`RESOURCE_LIMIT`により停止した。budgetを256にした再試行は比較結果をcommitしたが、本文は旧report形式のsurfaceになり、V3 assertionを通らなかった。該当試行ではartifact JSONのkindと失敗unit長を採取していない。現行builderがV3/V2のunit数・384-byte/unit等の上限後にV1 reportへfallbackするコードと、二段fixtureに複数rule dependency IDが含まれる観測から、意味単位境界がfallback理由だった可能性が高いと推定するが、失敗条件の個別特定は未実施。

最終B40 fixtureは一つの宣言規則と二つの直接premiseに限定し、既定budgetでV3生成と追問再生を通した。これは多段規則fixtureのV3対応、規則dependency chain全域、384-byte境界の網羅を示さない。stress出力は上記ログに残しており、失敗出力やtemporary build cacheを削除していない。

## 判定と未実施

B39のproducerは一つの実行例でV3 artifactを生成し、保存規則のEvidence unitをContentPlan/K7まで到達させた。プロセス再起動後にinventory・rule・request・compositionを再生成するfollow-upも`COMMIT`した。B38のV1/V2回帰は維持された。これは限定実行の実証であり、R4の全成功述語、初見入力全般、一般内容妥当性、文章品質、保存関係の網羅性、通常CLI/HTTP経路は確立しない。`CORE-G03=PARTIAL`を維持し、他ゲートは再判定しない。

- `cargo test -p minidora-engine`の全テスト、workspace runtime suite、Clippy、全機能build: **NOT_RUN**。
- R2-17の比較受入、通常三機T01–T03、独立QA、Windows/macOS/Linux以外、release/deployment: **NOT_RUN**。
- Linux / `rustc 1.99.1` / `cargo 1.99.1` / `CARGO_TARGET_DIR=/workspace/.minidora-cloud/rust-target`。
- R4三文書とそのarchiveは変更していない。以前の2026-11-14添付三点セットは展開・複製していない。

## 資産状態

各生成・変更資産は下表の一箇所にだけ分類する。

| 状態 | 個数 | 対象 |
|---|---:|---|
| keep | 5 | B40 integration test、`IMPLEMENTATION_STATUS.md`、本BUILD記録、`b40-build-observation.json`、`b40-focused-tests.log` |
| store | 0 | 独立保管へ移動した資産なし |
| in-use | 3 | `/workspace/.minidora-cloud/rust-target`、`/workspace/.cargo`、`/workspace/.rustup`（共有build/cache/toolchain） |
| trash | 0 | 削除・破棄した資産なし |
| hold | 0 | 保留する資産なし |
| **合計** | **8** | keep 5 + store 0 + in-use 3 + trash 0 + hold 0 |

試験後のB40 temporary directoryは0件。失敗attemptログはkeepに分類し、共有target/cacheはin-useとして残した。

## 次consumer

複数の保存規則と長いproof dependencyを含む比較が既存V3の16-unit/384-byte制約内に入らない境界を再現可能なfixtureで切り分ける。継続実装時は旧V1/V2/V3 artifact再生との互換性を保つversion方針と一緒に扱う。
