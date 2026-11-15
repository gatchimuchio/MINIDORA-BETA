# BUILD_BATCH_37 — 原子的関係proofからCore内容計画を作りK7へ接続

**main基線:** `3b0103053136c01f49da1570cb601ddecf2261b2`（B36）。
**施工範囲:** 型付き直接Core説明のK2/K3 proofを新しいK4内容unitと意味順序へ変換し、既存ContentPlan・K7へ接続。
**現在ゲート:** `CORE-G03 = PARTIAL`。他のCOREゲートは再判定しない。

## 対象と値の流れ

対象はMINIDORA-BETAの唯一のCore。入力は`Engine::compose_atomic_explanation`が受け取る型付き`Proposition`とCore actor memoryの有効な関係proofで、完成済み説明文は渡さない。Coreが封印したproof IDを実行時に再照合した後、証明のFact/RuleをClaim、Evidence、Rule、Detail、Limitation役割の内容unitへ組み立てる。unitの依存と役割を`MeaningCompositionRequest`へ渡し、既存の非ニューラル構成器でContentPlan用テキストを作る。request/reportはversion 2の`CoreComputationArtifact`に保存し、最終ContentPlanとK7描画を接続する。従来artifact version 1のreaderは残した。

`typed Proposition + current proof` → proof ID再照合 → Core生成の意味unit/依存計画 → `MeaningCompositionReport` → `ContentPlan` → K7 `RenderObservation`。

保存されたComposition API report自身は`Guarantee::Unknown` / `TYPED_INPUT_UNVERIFIED`のままとした。このAPI単体はproposition/evidence labelの真偽を検証しない。Core結果の`DERIVED`保証は別の関係queryで検証したproofと依存IDに基づく。試験は保存artifactのtyped requestを再構成し、report・ContentPlan unit順・proof ID集合を照合する。version 2 artifact readerにも同じreplay/一致検査を追加し、version 1の読取り契約は保った。

## 観測と受入値

- 導出例は完成文のない型付き入力から新しい結論文、2つの直接前提、適用規則、範囲/真偽の制限を生成し、`COMMIT`した。resultとplanの保証は`DERIVED`。ContentPlanの依存は両前提Fact、派生Fact、Rule IDを保持した。
- Composition reportの役割は`CLAIM → EVIDENCE → RULE → LIMITATION`。reportの証拠ID集合はCore relation resultの完全なproof ID集合と一致し、同じrequestの再構成結果は保存reportと一致した。
- K7は`RENDERED`。render bytesはContentPlan本文と同じ。単一surfaceのK4 projectionは保存模型で再実行して保存emissionと一致した。ここでは単一候補のため候補順位付けを主張しない。
- 直接前提fixtureも`COMMIT`しContentPlan/K7へ到達した。未対応関係は`SUSPEND / MISSING_PREMISES`でContentPlan/renderなし。
- write grant 0の指定パスは`SUSPEND / WRITE_PERMISSION_MISSING`。fileなし、receipt 0、action record 0。外部ネットワーク作用なし。
- 開発中の初回導出テストは直接前提だけの説明で`Evidence`役割が不足することを検出した。Core plan builderが型付き前提をEvidence unitに含めるよう修正し、最終の直接/導出試験を再実行した。

## 有限確認

- `cargo fmt --all -- --check`: **PASS**。
- `cargo test --locked -p minidora-engine --test core_atomic_explanation_31`: **PASS**（1 passed / 0 failed）。
- `cargo test --locked -p minidora-engine --test core_atomic_explanation_34 -- --nocapture`: **PASS**（1 passed / 0 failed）。
- `cargo test --locked -p minidora-engine --test core_atomic_relation_comparison_36`: **PASS**（1 passed / 0 failed、既存比較consumerの回帰確認）。
- 実行観測は[b37-runtime-observation.json](b37-runtime-observation.json)（170,181 bytes、SHA-256 `31b35955b0e3a837d9ef94b7a2798f0f4632d39ac97980cf0c2329b95e80b851`）。Linux、Rust `1.99.1`。
- 全workspace runtime、複数段proofの母集団、内容妥当性の独立評価、通常CLI/HTTPからの型付き入力到達、Windows/他OS、R2-17モデル比較、独立QA、残るCORE-G03述語: **NOT_RUN**。

## 判断と残る境界

B37は、型付き原子関係と実proofからCoreが説明内容unitと意味順序を作り、通常の採用・描画consumerへ渡す一つの狭い日本語経路を示す。既成入力文の接続句選択だけではない。反面、出力はCoreが使う限定語彙/形式の一fixtureで、独立した意味妥当性、広い初見関係への適用、一般文章品質を確立しない。通常の自然言語入力経路も変更していない。したがって`CORE-G03 = PARTIAL`を維持し、入力機増築や専門能力工程へ進まない。
