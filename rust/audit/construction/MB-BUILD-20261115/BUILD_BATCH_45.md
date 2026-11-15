# BUILD_BATCH_45 — V1/V4 comparison proof invalidation isolation

**main基線:** `49cfb7ee24e25f0421962d8c5ec8eaf953623cd6`（B44）。

**主語・目的:** MINIDORA-BETAの唯一Coreが、同一actor/caseにV1/V4比較artifactを共存させたまま各1scopeのproofだけを変更し、対象の追問のみを停止しながら別scopeの保存比較を再利用できるか確認する。

**施工範囲:** engine integration testと観測記録。production code、artifact契約、入力文法、composer上限、依存、R4三文書は変更していない。`CORE-G03=PARTIAL`を維持し、全体ゲートへの昇格は主張しない。

## 実producer→consumer

一つの隔離actorにV1 unit-overflow scope二つ、V4 two-rule-chain scope二つを作成し、同一比較caseへ四artifactを保存した。V1/V4それぞれ対象scope一つから直接factを失効させ、Engineを再openしてresult IDを指定した追問を行った。

1. V1 fixed-report artifact二つとV4 compact-reference artifact二つを異なるscopeで比較producerが生成。四resultはContentPlan/K7 renderへ到達し、artifact refはすべて相異なる。
2. V1 targetとV4 targetの各scopeからAster直接fact一件ずつを通常の`invalidate <ID>`経路で失効。どちらの操作resultも選択したfact IDをdependencyとして保持した。
3. Engine再起動後、明示V1 target追問と明示V4 target追問はそれぞれ`SUSPEND / CONTEXT_PROOF_CHANGED`。どちらもContentPlan/renderを作らなかった。
4. 対象のSUSPEND後に、別scopeのV4 controlとV1 controlを過去の比較result IDでそれぞれ明示選択。両追問は選択したresult dependency、ContentPlan、`RENDERED`を保持して`COMMIT`した。
5. 四artifactを失効後に再読し、それぞれ元SHA256と一致することを確認した。

これはV1/V4 artifact二版、一actor、一case、二つの直接fact失効に限る。V2/V3の再検査、rule失効、任意の履歴長、schema migration、並行writer、outside-world truthは今回のfixtureでは検査していない。これは保存artifactの現在memory proof再照合であり、migration・並行性保証ではない。

## 検証結果

- `cargo test --locked -p minidora-engine --test core_atomic_relation_comparison_45 -- --nocapture`: **PASS**（1 test、exit 0、18.40s）。V1/V4 targetは両方`SUSPEND / CONTEXT_PROOF_CHANGED`、plan/renderなし。V4/V1 controlは両方`COMMIT / RENDERED`。失効前後に四artifact hash一致。
- `cargo fmt --all -- --check`: **PASS**。
- `git diff --cached --check`: **PASS**（6 staged files）。
- MANIFEST.tsvとR4三文書/ZIPのSHA256・バイト数・archive member照合: **PASS**。R4三文書とZIPは変更していない。
- 実行記録: [`b45-focused-tests.log`](b45-focused-tests.log)。構造化観測: [`b45-build-observation.json`](b45-build-observation.json)。
- 環境はLinux、`rustc 1.99.1 (58cf8f9c3 2026-10-13)` / `cargo 1.99.1 (5f94df478 2026-08-27)`。

全engine/workspace suite、Clippy、workspace全target check/build、全OS、通常CLI/HTTP、R2-17、rule invalidation、migration、並行writer、独立QA、release/deploymentは **NOT_RUN**。

## 資産状態

| 状態 | 個数 | 対象 |
|---|---:|---|
| keep | 6 | B45 integration test、`IMPLEMENTATION_CHOICES.md`、`IMPLEMENTATION_STATUS.md`、本BUILD記録、`b45-build-observation.json`、`b45-focused-tests.log` |
| store | 0 | 独立保管へ移動した資産なし |
| in-use | 3 | `/workspace/.minidora-cloud/rust-target`、`/workspace/.cargo`、`/workspace/.rustup`（共有build/cache/toolchain） |
| trash | 0 | 削除・破棄した資産なし |
| hold | 0 | 保留する資産なし |
| **合計** | **9** | keep 6 + store 0 + in-use 3 + trash 0 + hold 0 |

test temporary directoryはDrop後0件を確認する。R4三文書・archiveは変更していない。LinuxのためWindows storage routineはN/A。

## 次consumer

B45はV1/V4の現在memory proof再照合後に、対象scopeのみ停止し他scopeの異版controlが再利用される境界まで接続した。次は直接factではなく、同じscopeのruleを失効させた場合に、V4対象追問が停止し別scopeのV1 controlが再生できるかを一つの限定fixtureで確認する。現行範囲でV1/V4を再度網羅した保証や、全系移行・並行性保証には広げない。
