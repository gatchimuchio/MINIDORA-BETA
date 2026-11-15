# BUILD_BATCH_49 — Proof-preserving rule-composition path selection

**main基線:** `798cee4cfe9a338c7e628e0a16641d5cacdf04ef`（B48）。

**対象:** R2 §10.1 proof-preserving composition/path deduplication、`CORE-G05`、`CAP-D05`。B48はactor/caseをまたぐprior-result context拒否を確認した。R4施工三文書、R2、store schema、公開APIとCLIは変更していない。

**施工範囲:** `minidora-engine::relations`の限定したisolated composition scheduling修正、直接Engine integration test一件、関係unit/regression、状態・要求・施工選択の記録。通常CLI/HTTPへ接続せず、full `CAP-D05` acceptanceは`UNVERIFIED`。

## 同じ直接中核入口での前後比較

隔離Core storeで同一actor/case/scopeにAsterの`ceramic`と`struck`を直接入力し、同一意味・同一原文の`[scope=materials-lab]Aster is cracked?`を二つの宣言規則を保存する前に`Engine::execute`へ送った。Coreは`SUSPEND / MISSING_PREMISES`を返し、ContentPlanを作らなかった。

`ceramic → fragile`と`fragile AND struck → cracked`の二root rulesを同じCoreへ保存した。Core生成shortcutは両root IDをdependencyとして保持する。孤立したscope内でshortcutと二parentがある場合、二parentを同じquery内で重ねて展開せずshortcutを一回選択する。root-onlyでのrelation proof costは2 steps、shortcutは1 step。Engineをclose/reopenした後に同じ質問を再送し、`COMMIT / DERIVED / RENDERED`を確認。result dependencyにAsterの両direct fact、両root rule、shortcutが残る。

未見subject Cyraは同scopeの同じshortcutから`COMMIT / DERIVED`となる。Rookへ`other-lab` scopeでceramic/struckを与えても、同scopeに規則がない質問は`SUSPEND / MISSING_PREMISES`、planなしで停止する。後続のAster `polymer, not ceramic`訂正は元ceramic factを失効させる。Asterの同じ質問は再度`SUSPEND / MISSING_PREMISES`となる一方、前の派生commitはcase historyに完全保持し、Cyraの別proofは派生のまま。shortcutの中間`fragile`を明示否定するunit caseではshortcutを無効化し、矛盾した中間を飛び越えない。

詳細traceと実IDは[実行観測JSON](b49-build-observation.json)。

## 実行・検証

- `cargo test --locked -p minidora-engine --lib conjunctive_rule_tests -- --nocapture`: **PASS**（9 tests）。明示矛盾guard、variable join、二前提合成、root dependency、旧rule互換を含む。[unit log](b49-relation-unit-tests.log)
- `cargo test --locked -p minidora-engine --test core_proof_composition_learning_49 -- --nocapture`: **PASS**（1 integration test）。[Core log](b49-core-integration-pass.log)
- `cargo test --locked -p minidora-engine --test core_construction_observation_05 --test core_atomic_relation_comparison_40 --test core_atomic_relation_comparison_41 -- --nocapture`: **PASS**（3 integration tests）。[回帰log](b49-relation-regressions.log)
- `cargo check --locked -p minidora-engine --all-targets`: **PASS**。[all-target log](b49-engine-all-target-check.log)
- `cargo fmt --all -- --check`: **PASS**。[format log](b49-format-check.log)
- `git diff --check`: **PASS**。[diff log](b49-git-diff-check.log)
- 初回integration fixtureはRequestEnvelopeの`max_actions=1`で試し、Core requestの`compute/content/render`必須actionを予算に含めず`SUSPEND / ACTION_BUDGET_EXHAUSTED`となった。これは本体不具合ではなくfixture budgetの不足。Core commitはdefault request budgetで行い、relation query stepだけを別に1/2で比較して再実行した。[初回失敗log](b49-initial-action-budget-failure.log)
- 初回9 unit実行は一度未使用field warningを含んだが、そのfieldを除去。最終unit logにwarningはない。初回記録は[初回focused log](b49-initial-focused-tests.log)。
- cargoが見えない環境PATHまたはworkspace rootでの初期command呼出しはtestを開始していない。正しい`rust/` workspace rootとRust 1.99.1 toolchain PATHを指定した再実行結果だけを上記のtest結果として数える。

環境: Linux x86_64、Rust/Cargo 1.99.1、3 workspace members。`release=NOT_RELEASED`。

## 判定と未実施

この直接Core経路のproof-preserving compositionについて、前後変化、未見subject適応、scope外停止、後続矛盾による失効、過去resultの回復可能なlineage、2→1 relation-step deduplicationを観測したため、R4述語に対する`CORE-G05=PASS`と判定する。これはこのcomposition経路についての判定であり、観測起点の経験仮説形成を実証したものではない。`CORE-G03=PARTIAL`は維持。残るゲートも本batchで全再評価しておらず、CORE-G01〜07全件PASSになるまで入力機施工へ移らない。`CAP-D05=UNVERIFIED`。

通常CLI/HTTP、新規利用者経路、empirical hypothesis learning、outside-world truth、full `CAP-D05`/BB-LEARN acceptance、full workspace runtime suite、workspace Clippy/build、migration、concurrent writer、independent QA、release/deployment: **NOT_RUN**。

## 資産閉包

| 状態 | 個数 | 対象 |
|---|---:|---|
| keep | 15 | production relations code + integration test（2）、CAP-D05台帳差分（1）、status・choices（2）、本記録・観測JSON（2）、focused pass/check logs（6）、初回budget failure・初回warning logs（2） |
| store | 0 | 外部保管へ移動した資料なし |
| in-use | 4 | `/workspace/.minidora-cloud/rust-target`（8.1G）、`rust/target`（7.5G）、Cargo cache（155M）、Rust toolchain（617M） |
| trash | 0 | 削除・ゴミ箱移動なし |
| hold | 0 | 保留資産なし |
| **合計** | **19** | keep 15 + store 0 + in-use 4 + trash 0 + hold 0 |

編集前のHEAD `798cee4cfe9a338c7e628e0a16641d5cacdf04ef`をrollback targetとしてGit履歴に保持する。新しいbackup branch/tagは作らない。実装とこの記録をmainへcommit/pushする。release/deploymentは行わない。
