# BUILD_BATCH_47 — Actor-scoped invalidation of saved V4 proofs

**main基線:** `3108c733bbbfda9943fbbdc504201ec50162ffc7`（B46）。

**主語・目的:** Coreがactorごとに保持するrule memoryと、保存済みV4比較proofの分離を確認する。対象要求はR2 `I06` / `CAP-D13`。B46は一actor・二scopeを確認し、actor間の独立性は未観測だった。

**施工範囲:** 隔離SQLite DBを使う`minidora-engine` integration test一件と局所証跡のみ。production code、保存契約、R4三文書は変更しない。`CAP-D13=UNVERIFIED`、`CORE-G03=PARTIAL`を維持する。

## 実producer→consumer

同じscopeに同型の二段rule chain（sensor→inspected→reliable）とAster/Berylのpremiseを、actor A / actor Bの別memoryと別caseへ保存した。Coreが両actorで個別にV4 comparison artifactをContentPlan/K7 renderまで生成し、root rule ID、composed rule/proof ID、result ID、artifact SHA-256はactor間で異なる。

actor Aの通常`Engine::execute("invalidate <rule-id>")`がAのrootと依存ruleを保持したまま無効化し、Aのmemory revisionは4→5になった。Engine再起動後、Aが自分の保存result IDを選ぶ追問は`SUSPEND / CONTEXT_PROOF_CHANGED`となり、ContentPlan/renderを作らない。一方、Bのmemoryはrevision 4のまま全ruleが有効で、保存artifact hashも元のまま。Bが自分の保存result IDを選ぶ追問は`COMMIT / RENDERED`し、Bのmemoryは変化しなかった。

これは同一Core store内の二actor・二case・同一scope・一root-rule失効のfixtureである。並行writer、case共有、migration、通常CLI/HTTP入口、outside-world truth、CAP-D13全述語は確認しない。全記録は[実行観測JSON](b47-build-observation.json)と[focused test log](b47-focused-tests.log)にある。

## 限定確認

- `cargo test --locked -p minidora-engine --test core_atomic_relation_comparison_47 -- --nocapture`: **PASS**（1 test、exit 0）。
- `cargo fmt --all -- --check`: **PASS**。
- `cargo metadata --locked --no-deps --format-version 1`: **PASS**（3 workspace members）。
- R4 manifestと3文書・ZIP member/hash/sizeの再照合: **PASS**。
- 最初のtest実行時はログ保存先の相対path誤りで`tee`だけが失敗したため、正しい絶対pathで同じtestを再実行し、本記録のlogに保存した。test本体の失敗ではない。
- workspace全test、Clippy、全workspace check/build、migration、concurrent writer、CLI/HTTP、全CAP-D13受入、独立QA、release/deployment: **NOT_RUN**。

環境はLinux x86_64、Rust/Cargo 1.99.1。実行targetは`/workspace/.minidora-cloud/rust-target`。scope終了時の空き容量は43G。LinuxのためWindows storage routineはN/A。専用temp stateはtestが削除し、外部保管/ゴミ箱移動/OneDrive操作なし。

## 資産閉包

| 状態 | 個数 | 対象 |
|---|---:|---|
| keep | 7 | B47 integration test、CAP-D13台帳差分、status、choices、本記録、観測JSON、focused log |
| store | 0 | 外部保管へ移動した資料なし |
| in-use | 3 | Rust target（7.8G）、Cargo cache（252M）、Rust toolchain（617M） |
| trash | 0 | 削除・ゴミ箱移動なし |
| hold | 0 | 保留資産なし |
| **合計** | **10** | keep 7 + store 0 + in-use 3 + trash 0 + hold 0 |

## 復旧・次consumer

編集前にCASでlocal recovery branch/tagとremote recovery tagを`backup-main=3108c733`（B46）、`backup-main-prev=c2595ca`（B45）へ進めた。remote tagの旧値をleaseで照合し、他のlegacy remote branchは変更していない。次consumerは別actorの保存proofを用いる通常入口と、並行/再起動条件の体系施工。B47単独からactor分離全般、CAP-D13全受入、`CORE-G03`の進展は推定しない。
