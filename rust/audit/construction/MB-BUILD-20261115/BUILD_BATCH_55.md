# BUILD_BATCH_55 — Core two-premise proof and contradiction boundary

**BASE:** `0925747d67a7792c1bfc594d00bd077eaa40a8f1` (B54, `main` = `origin/main`, clean before B55 changes).

**工程・ゲート:** 中核K1〜K7の不足是正。二前提の結合・不足停止・反証後の不採用を直接Core consumerで観測する。`CORE-G02=PARTIAL`。`CORE-G01=PARTIAL`、`CORE-G03=PARTIAL`を維持し、他ゲートは再判定しない。

**目的:** 新たに保存した二つの独立Factと一つのconjunctive RuleをCoreが必要な証拠として結合し、根拠不足・成立導出・矛盾後停止をContentPlanとK7まで区別して渡す。

## Producer → consumer

隔離actor `b55-conjunctive-proof`・case `b55-conjunctive-proof-case` のscope `b55-calibration-world`で、完成回答を入力せず、typed Core APIを使った。

1. `Engine::add_atomic_relation_rule`から `$x is calibrated` AND `$x is power stable` ⇒ `$x is safe` を保存した。
2. Velaの`calibrated` Factだけをassertして `safe` を質問すると、`SUSPEND / MISSING_PREMISES`となり、ContentPlan/renderを生成しなかった。
3. Velaの`power stable` Factもassertすると、同じ質問が`COMMIT / DERIVED`となった。ResultとContentPlanは両FactとRuleのproof dependencyを保持し、K7はContentPlanのbytesと完全一致する`RENDERED`結果を返した。
4. `power stable`のnegative Factを独立に追加した。`safe`の再質問は`SUSPEND / MISSING_PREMISES`でplan/render前に停止し、競合する`power stable`自体の質問は`SUSPEND / CONFLICTING_PREMISES`で停止した。Store上では元のpositive Fact、negative Fact、Ruleがそれぞれ有効のまま残り、推論時に競合根拠を採用しなかった。

実ID、result依存値、plan、render、状態の観測値は[機械可読記録](b55-build-observation.json)と[限定テストlog](b55-conjunctive-proof-test.log)に保存した。

## 限定確認

- `cargo test --locked -p minidora-engine --test core_conjunctive_proof_55 -- --nocapture`: **PASS**, 1 integration test。
- `cargo check --locked -p minidora-engine --all-targets`: **PASS**。
- `cargo fmt --all -- --check`: **PASS**。
- `git diff --check`: **PASS**。

Production relation semantics、wire/schema、依存、R2、R4三点セットには変更なし。条件参照の真偽評価はB55の対象外で、正本の参照先データ形式・評価規則が未定義な状態を推測で埋めていない。counterfactualの直接Core経路、別のpremise graph、複数subject/scope、一般入力入口、現実世界の真偽も未確認。

この限定fixtureは「前提二つを結合し、片方の不足と競合を止める」実consumer観測であり、CORE-G02の成立述語全体、とくにcounterfactual区別を満たさないため**`CORE-G02=PARTIAL`**。`CORE-G01=PARTIAL`、`CORE-G03=PARTIAL`を維持する。他のゲートは本batchで再判定していない。CORE-G01〜G07全件PASS前の入力機施工には進まない。release/deploymentは行わない。
