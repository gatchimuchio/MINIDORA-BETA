# BUILD_BATCH_46 — Saved V4 rule-proof invalidation with V1 control

**main基線:** `c2595ca19405c873faf12bef571e7210e2ba4dbd`（B45）。

**主語・目的:** MINIDORA-BETAの唯一Coreが保存した比較proof中のruleを明示失効させ、再起動後に古いV4 targetだけを停止し、別scopeのV1保存比較を利用できることを確かめる。関連要求はR2 `I06` と `CAP-D13`。

**施工範囲:** `minidora-engine` integration testとCAP-D13への局所観測記録。production code、artifact契約、入力文法、依存、R4三文書は変更していない。`CAP-D13=UNVERIFIED`、`CORE-G03=PARTIAL`を維持する。

## 実producer→consumer

隔離DBの一actorにV4 target用の二段rule chainとV1 control用の別scope facts/ruleを`Engine::execute`で保存し、同じcomparison caseへ両artifactを作成した。

1. `Engine::compose_atomic_relation_comparison`はtarget scopeの二段rule proofを含む`minidora.relation-comparison-content.v4`と、別scopeの`minidora.relation-comparison-report.v1`をそれぞれContentPlan/K7 renderまで生成した。V4 artifact proofには失効対象root rule `rule:0757506f8b7560d5bf4e6b8cd1d9e9d9d08e2997345d7f9a8c620ff11d37f769`とCore合成rule `rule:738bf07a232e436d2f1cca240d977374b64ff5fe7dd71bebbca604c1f0a952b1`が含まれる。
2. 通常の`Engine::execute("invalidate <rule-id>")`経路はroot ruleと依存する合成ruleの両記録を残して`valid=false`にし、actor memory revisionを11から12へ進めた。invalidate resultは両IDをdependencyとして保持した。
3. Engine再起動後、保存V4 result IDを明示した追問は`SUSPEND / CONTEXT_PROOF_CHANGED`となり、ContentPlan/renderを作らなかった。別scopeの保存V1 result IDを明示した追問は`COMMIT / RENDERED`となった。
4. V4 target artifact (`a7707ce702ea9571db23cb491c631b32da7d8edd94cd89e8094d7568953d0855`) とV1 control artifact (`bbf0b6bf57a6413302af2b8f35050b1e61c04fba57f0619cd6f031f8f9233321`) は、失効後・再起動後も元hashで読み戻せた。V1 control render SHA256は`ef893784b68b592f11f3b247afc1ad00c360509cf9c8d49a824ba2cb45af5308`。

これは一actor・一case・二scope・一rule失効の局所Core fixtureである。counterexampleでactive conclusionを置換する経路、通常CLI/HTTP利用、全rule形状、他actor、全scope、migration、並行writer、outside-world truth、独立QC/QAは成立を主張しない。`CAP-D13`の全受入は未成立。

## 検証

- `cargo test --locked -p minidora-engine --test core_atomic_relation_comparison_46 -- --nocapture`: **PASS**（1 test、exit 0、5.00秒）。全traceは[`b46-focused-tests.log`](b46-focused-tests.log)。
- `cargo fmt --all -- --check`: **PASS**。
- `cargo metadata --locked --no-deps --format-version 1`: **PASS**（3 workspace members）。
- R4 `MANIFEST.tsv`、三文書、ZIP、archive member、各サイズ/SHA256照合: **PASS**。三文書・ZIPは変更していない。
- 構造化観測: [`b46-build-observation.json`](b46-build-observation.json)。
- `git diff --cached --check`: **PASS**（7 staged files）。
- 観測JSON parse: **PASS**。
- secret-pattern scan: **PASS**（一致0件）。
- test temporary directory cleanup: `/tmp`の該当directory **0件**。
- 環境: Linux、`rustc 1.99.1 (58cf8f9c3 2026-10-13)`、`cargo 1.99.1 (5f94df478 2026-08-27)`、workspace edition 2024。

Full engine/workspace runtime suite、Clippy、workspace all-target check/build、全OS/Windows 8GB、通常CLI/HTTP、R2-17、CAP-D13全受入、migration、parallel writer、independent QA、release/deploymentは **NOT_RUN**。

## 資産閉包

| 状態 | 個数 | 対象 |
|---|---:|---|
| keep | 7 | B46 integration test、CAP-D13 `requirements.tsv`追記、`IMPLEMENTATION_STATUS.md`、`IMPLEMENTATION_CHOICES.md`、本BUILD記録、観測JSON、focused-test log |
| store | 0 | 外部保管へ移した資料なし |
| in-use | 3 | `/workspace/.minidora-cloud/rust-target`（7.7G）、`/workspace/.cargo`（252M）、`/workspace/.rustup`（617M） |
| trash | 0 | 削除/ゴミ箱移動なし |
| hold | 0 | 保留資産なし |
| **合計** | **10** | keep 7 + store 0 + in-use 3 + trash 0 + hold 0 |

free spaceは施工終了確認時43G。Windows storage routineはLinuxのためN/A。独立OneDrive保管、オンライン専用化、ゴミ箱処分は行っていない。

## 復旧・次consumer

初期確認時はmain=`c2595ca`、recovery refs=`B44/B43`。意味ある編集前にCASでlocal `codex/backup-main` とremote同名tagをB45 (`c2595ca`) へ、`prev`をB44 (`49cfb7e`) へ更新済み。この単位終了までは回転しない。既存の同名remote backup branchesはmainの祖先で、legacy branch/local remote-tracking tipに差があるため変更していない。

B46は保存V4 proofに含まれるrule失効後の停止と、V1 controlの再利用まで接続した。次は同型のrule chainと比較を別actorのmemoryへ独立保存し、元target actorのrule失効後も他actorの明示追問がそのactor自身のproofだけで再生される境界を一つの隔離fixtureで確かめる。CAP-D13全体受入や並行writer保証には広げない。
