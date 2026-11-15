# BUILD_BATCH_43 — coexisting comparison artifact versions

**main基線:** `a029cb99c880f45b1b59e926890e72770cda4f40`（B42）。

**主語・目的:** MINIDORA-BETAの唯一Coreが、異なる比較artifact版を同一caseに保持し、再起動後に指定版を選んで後続consumerへ渡せるかを一つの限定fixtureで確認する。

**施工範囲:** engine integration testと観測記録。production code、artifact契約、composer上限、R4三文書は変更していない。`CORE-G03=PARTIAL`を維持し、全体ゲートへの昇格を主張しない。

## 実経路

一つの隔離actorのmemoryにscopeを分けて四種類の入力を保存した。直接factのみのV2、単一rule由来のV3、二段rule chainのV4、B42同型のunit overflowによるV1である。Aster/Berylの比較四件を、異なるinput IDを使いながら同一actor・同一caseへ順次commitした。

1. Coreの比較producerがV2 `minidora.relation-comparison-content.v2`、V3 `minidora.relation-comparison-content.v3`、V4 `minidora.relation-comparison-content.v4`、V1 `minidora.relation-comparison-report.v1`を一つのcaseへ保存した。
2. 四比較それぞれのresult ID、artifact ref/kind/hash、inventory proof数、adopted ContentPlan、K7 renderを記録。四artifact refが相異なり、四件目の生成後にも全blobを同時に再読してhash一致を照合した。V2/V3/V4 typed composition evidence IDは各inventory proof集合と一致し、V1 reportにも左右inventory proofが保持された。
3. Engineを閉じて一度だけ再openした後、各比較result IDを明示する`なぜ`追問を同じcaseへ順次入力。各追問は意図したprior resultをdependencyとして消費し、版ごとの保存artifactを再生して`COMMIT`、ContentPlan、`RENDERED`まで到達した。

観測は一つのprocess-local fixture、actor、case、scope分離に限る。無関係scopeをまたぐ競合、同じscopeのrule/fact変更、壊れたblob、並行case writer、schema migration、過去全履歴の同時共存、outside-world truthは検査していない。

## 検証結果

- `cargo test --locked -p minidora-engine --test core_atomic_relation_comparison_43 -- --nocapture`: **PASS**（1 test）。V1/V2/V3/V4の四artifactが同一caseで同時に読め、再起動後の四追問が各々意図したresultを消費して`COMMIT`・`RENDERED`。
- `cargo fmt --all -- --check`: 最終結果は実行ログとJSONに記録。
- `cargo check --locked -p minidora-engine --all-targets`: 最終結果は実行ログとJSONに記録。compile checkであり全test suite実行ではない。
- `git diff --check`: 最終結果は実行ログとJSONに記録。
- 実測: [`b43-build-observation.json`](b43-build-observation.json)。コマンド記録: [`b43-focused-tests.log`](b43-focused-tests.log)。

環境はLinux、`rustc 1.99.1 (58cf8f9c3 2026-10-13)` / `cargo 1.99.1 (5f94df478 2026-08-27)`。全engine/workspace suite、Clippy、全OS、通常CLI/HTTP、R2-17、独立QA、release/deploymentは **NOT_RUN**。R4三文書とarchiveは変更していない。

## 資産状態

| 状態 | 個数 | 対象 |
|---|---:|---|
| keep | 6 | B43 integration test、`IMPLEMENTATION_CHOICES.md`、`IMPLEMENTATION_STATUS.md`、本BUILD記録、`b43-build-observation.json`、`b43-focused-tests.log` |
| store | 0 | 独立保管へ移動した資産なし |
| in-use | 3 | `/workspace/.minidora-cloud/rust-target`、`/workspace/.cargo`、`/workspace/.rustup`（共有build/cache/toolchain） |
| trash | 0 | 削除・破棄した資産なし |
| hold | 0 | 保留する資産なし |
| **合計** | **9** | keep 6 + store 0 + in-use 3 + trash 0 + hold 0 |

test temporary directoryはDrop後0件を確認する。

## 次consumer

同一caseに保存された比較artifactの指定版reader/replayを確認した。次は、この共存状態で一つのscopeのproofだけが変わった場合に、対象resultのfollow-upが`CONTEXT_PROOF_CHANGED`で停止し、別scope・別版の保存artifactを巻き込まないかを限定して確かめる。これは現memory revisionのproof invalidation境界であり、migrationや並行処理の保証とは分ける。
