# BUILD_BATCH_36 — 型付き原子的関係比較をK7へ接続

**main基線:** `ff220dc068ac6332bfa21558a2e86d91fe15ee0a`。
**施工範囲:** Coreの型付き比較API、左右別の証拠ID封印と再照合、既存比較consumer、ContentPlan、K7描画。
**現在ゲート:** `CORE-G03 = PARTIAL`。この束では他ゲートを判定しない。

## 要求とデータフロー

完成文を受け取らず、二つの対象とscopeだけから保存根拠を比較し、その内容をCoreが組み立ててK7まで届ける。実経路は次のとおり。

`Engine::compose_atomic_relation_comparison` → actorメモリの左右別`RelationInventory` → 版付き`CORE_TYPED_RELATION_COMPARISON_V1` / `CompareRelationInventories`入力束 → Coreによる左右の証拠ID再照合 → 比較レポートartifactと根拠付き本文 → `ContentPlan`採用 → K7 `RenderObservation` → 結果確定。

型付き入力は原文欄が空であることを要求し、scopeと二対象を保存する。Coreが計算した左右別proof IDを封印し、比較実行時に再計算したID集合と不一致なら内容生成前に停止する。自然文文法やCLI/HTTP入口は変更していない。

## 有限ケースと受入結果

fixtureでAsterとBerylに共通の`pilot`、Asterのみの`musician`、Berylのみの`geologist`をactor memoryへ保存した。初見の型付き比較からCoreは保存記録の共通点・片側のみの関係・根拠IDを含むレポートを作り、両inventoryとともにartifactへ保存した。K7のrender bytesは採用ContentPlanの本文と一致した。比較結果とContentPlanの保証は`HYPOTHESIS`で、保存記録の網羅性や現実世界の真偽を主張しない。

- 根拠ID：Aster側2件、Beryl側2件。封印値・実行artifactの両方で側ごとに一致。
- 比較結果：`COMMIT`、共通関係・両側の片側のみ関係・4記録IDを含む。
- ContentPlan / K7：採用済み。render状態`RENDERED`、plan IDと本文bytesが一致。
- 両対象の支持記録がない依頼：`SUSPEND / MISSING_PREMISES`、ContentPlan/renderなし。
- write grant 0での出力要求：`SUSPEND / WRITE_PERMISSION_MISSING`、対象fileなし、receipt 0、action record 0。

## 限定確認

- `cargo fmt --all -- --check`: **PASS**。
- `cargo test --locked -p minidora-engine --test core_atomic_relation_comparison_36 -- --nocapture`: **PASS**（1 passed / 0 failed）、Rust `1.99.1 (58cf8f9c3 2026-10-13)`、Linux。
- 全実行値・sealed input・source ID・比較artifact・ContentPlan・render・停止値は[実行観測JSON](b36-runtime-observation.json)に保存（21,341 bytes、SHA-256 `4117d759b3a893f143bd0aaa8d777a04a2c5c4ce16a7c9688a23068ae84b01cd`)。
- 全workspace runtime、一般的な内容品質、複数scope/複雑な導出/競合の母集団、通常CLI/HTTP入口、他OS、独立QA: **NOT_RUN**。

## 判断と次consumer

B36は、原子的な二対象の意味入力からCoreが比較を実行し、根拠付き比較レポートを既存採用経路へ流す一ケースを示した。出力は現行の構造化レポート生成であり、比較対象の保存記録を超えた事実追加や一般的な初見文章品質を示さない。したがって`CORE-G03 = PARTIAL`を維持する。入力機や専門能力の工程へ進まない。次はCore内で原子的入力から複数の意味に沿った内容unitを生成・採用する不足を、同じK1–K7境界で切り分ける。
