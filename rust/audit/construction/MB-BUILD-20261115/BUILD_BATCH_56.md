# BUILD_BATCH_56 — Reassess Core G02 with counterfactual evidence

**BASE / observed HEAD:** `6d9ec1d31d7541e2e70e1fc0a1681542a3fe549d` (B55, `main` = `origin/main`, clean before B56 changes). **Schema:** `minidora-rust/0.7.0-alpha.10`.

**工程:** Current HEADで`CORE-G02`の成功述語を再評価する。B55で追加した二前提typed Core観測と、既存のB11 direct Core/counterfactual観測を同じalpha10 HEADから再実行した。

## Current-HEAD re-observation

実行した限定コマンド:

`cargo test --locked -p minidora-engine --test core_construction_observation_02 --test core_conjunctive_proof_55 -- --nocapture`

結果は**2 integration tests PASS**。[実行log](b56-core-g02-current-head.log)と[機械可読評価](b56-core-g02-reassessment.json)を保存した。

- B55の直接型付き二前提ruleで、1 premiseでは`MISSING_PREMISES`停止、両FactがそろうとFact ID 2件とRule IDを含む`COMMIT / DERIVED`・ContentPlan・K7 `RENDERED`に到達。片方へのnegative Fact追加後は導出結論を採用せず、競合関係問も`CONFLICTING_PREMISES`でplan/render前に止まる。
- B11の`Engine::execute` direct Core fixtureで、二前提からの通常導出がRule＋両FactのIDを保持する。追加条件の不足は`MISSING_PREMISES`、導出への反対Factは`CONFLICTING_PREMISES`で止まり、内容計画を採用しない。
- 同じB11 test内の`Engine::evaluate_counterfactual_relation`は、activeな`USER_PREMISE`と正確に反対の仮定だけをscenario clone内で置き換え、`SUPPORTED / HYPOTHESIS`を返した。hypothesis、sensor Fact、Rule、派生Factのproof IDsを保持し、置換Factだけをscenario cloneで無効化した。Actorの保存MemoryStateは前後一致し、通常queryはcounterfactualの仮説を消費せず`MISSING_PREMISES`を維持した。
- 反対Factをscenario内で置き換えないcounterfactual要求は`CONFLICT / UNKNOWN`、無関係Factをoverrideする試みは`COUNTERFACTUAL_OVERRIDE_INVALID`。自然言語counterfactual入力は依然`CAPABILITY_UNAVAILABLE`で、仮説を通常回答へ混入させない。

これによりCoreは、複数前提からの通常導出、不足、通常世界での矛盾、明示hypothesisに限るcounterfactual結果を、根拠ID・保証区分・実依存つきで区別した。`CORE-G02=PASS`と再判定する。適用範囲はR4のconstruction gate predicateに限り、R2全要件、一般言語counterfactual、広い因果推論や93要件受入を意味しない。

**B55記録境界注記:** B55の`CORE-G02=PARTIAL`とcounterfactual未確認の記載は、そのbatch自身の新しい二前提fixtureに限った評価だった。リポジトリに既存のB11 typed counterfactual実装・観測があることをB55で十分に反映していなかった。本batchでB11を現行HEAD上で再実行し、B55と合わせたゲート判定を更新した。過去のB55記録は履歴としてそのまま保持する。

## 判定と境界

- `CORE-G02=PASS`。この値は二つの直接Core経路と明示仮説の隔離評価に対する施工gate判定。
- `CORE-G01=PARTIAL`、`CORE-G03=PARTIAL`を維持。`CORE-G04`、`CORE-G06`、`CORE-G07`は再判定していない。`CORE-G05`の既存限定判定は変更しない。
- B55の二前提テストはK7描画まで到達する。Counterfactual APIは設計どおりhypothesis reportのみを返し、通常のcontent adoptionは行わない。
- full workspace tests、Clippy、all-target compile check、通常言語counterfactualの実装/受入、全93要求、全体系QC/QA、release/deploymentは本batchで**NOT_RUN**。入力機施工への移行条件となる全ゲートPASSは未成立。

**変更範囲:** 実装・wire/schema・依存・R2/R4正本を変更せず、現行HEADのgate evidenceと評価記録だけを追加する。
