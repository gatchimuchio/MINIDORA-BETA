# BUILD_BATCH_44 — scope-specific proof invalidation across saved comparison versions

**main基線:** `87f26a978a1ca7e46de1428d1a10a9763a105fea`（B43）。

**主語・目的:** MINIDORA-BETAの唯一Coreが、同一actor/caseに異なる比較artifact版が保存された状態で、一つのscopeのproof変更を対象追問だけに反映し、別scopeの保存結果を使い続けられるかを確認する。

**施工範囲:** engine integration testと観測記録。production code、artifact契約、入力文法、composer上限、R4三文書は変更していない。`CORE-G03=PARTIAL`を維持し、全体ゲートへの昇格を主張しない。

## 実producer→consumer

一つの隔離actorのmemoryに異なるscopeの記録を作り、同一比較caseへV3 rule-bearing artifactとV2 direct artifactを保存した。V3のAster側proofに含まれる直接fact IDだけを通常の`invalidate <ID>`経路で失効させ、Engineを再openして明示context参照で追問した。

1. Coreの比較producerがV3 `minidora.relation-comparison-content.v3`（`b44-rule-proof-target`）とV2 `minidora.relation-comparison-content.v2`（`b44-direct-proof-control`）を同じactor/caseへ保存。双方のartifact refは別で、ContentPlanとK7 renderがある。
2. 対象V3 proof中のAster直接factを失効。失効操作のCOMMIT結果が選んだfact IDをdependencyとして保持した。
3. Engine再起動後、V3 result IDを明示する`なぜ`追問は`SUSPEND`、reason=`CONTEXT_PROOF_CHANGED`となり、ContentPlanもrenderも作らなかった。
4. 直後にcaseの最新resultがV3のSUSPENDとなった状態で、過去のV2 result IDを明示する追問を実行。V2 artifactは元hashのまま読まれ、追問は選択したV2 resultへのdependencyを保ち`COMMIT`・`RENDERED`へ到達した。
5. 両artifactを失効後に再読し、元のSHA256と一致することを確認した。

これはV2/V3の二版、一actor、一case、一つの直接fact失効に限る。V1/V4、rule失効、全scopeや全履歴、schema migration、並行case writer、outside-world truthは検査していない。これは現memory revision上のproof invalidation境界であり、移行・並行性保証ではない。

## 検証結果

- `cargo test --locked -p minidora-engine --test core_atomic_relation_comparison_44 -- --nocapture`: **PASS**（1 test、exit 0）。対象V3 follow-upは`SUSPEND / CONTEXT_PROOF_CHANGED`、plan/renderなし。別scope V2 follow-upは`COMMIT / RENDERED`。両artifact hashは失効前後で一致。
- `cargo fmt --all -- --check`: 最終結果 **PASS**。初回確認の書式差分に`cargo fmt --all`を適用して再確認。
- `git diff --cached --check`: **PASS**。6変更ファイルのstaged diffにwhitespace errorなし。
- JSON parseとR4三文書/同梱archiveのSHA256再照合: **PASS**。R4三文書とZIPは現行指示どおり不変。
- 変更対象のsecret-pattern scan: 一致なし。test temporary directory: Drop後0件。
- 実行記録: [`b44-focused-tests.log`](b44-focused-tests.log)。構造化観測: [`b44-build-observation.json`](b44-build-observation.json)。
- 環境はLinux、`rustc 1.99.1 (58cf8f9c3 2026-10-13)` / `cargo 1.99.1 (5f94df478 2026-08-27)`。

全engine/workspace suite、Clippy、workspace全target check/build、全OS、通常CLI/HTTP、R2-17、migration、並行writer、独立QA、release/deploymentは **NOT_RUN**。これらを当該限定fixtureの必要条件とはしない。

記録用の再実行では初回`tee`の相対path誤りでラッパーだけexit 1となったため、正しいpathで同じfocused testを再実行しexit 0のログを保存した。失敗したラッパー出力自体は保存物に混入させていない。Cargo test本体は両回ともPASSだった。

## 資産状態

| 状態 | 個数 | 対象 |
|---|---:|---|
| keep | 6 | B44 integration test、`IMPLEMENTATION_CHOICES.md`、`IMPLEMENTATION_STATUS.md`、本BUILD記録、`b44-build-observation.json`、`b44-focused-tests.log` |
| store | 0 | 独立保管へ移動した資産なし |
| in-use | 3 | `/workspace/.minidora-cloud/rust-target`、`/workspace/.cargo`、`/workspace/.rustup`（共有build/cache/toolchain） |
| trash | 0 | 削除・破棄した資産なし |
| hold | 0 | 保留する資産なし |
| **合計** | **9** | keep 6 + store 0 + in-use 3 + trash 0 + hold 0 |

test temporary directoryはDrop後0件を確認する。R4三文書・archiveは変更していない。LinuxのためWindows storage routineはN/A。

## 次consumer

B44は、一つのscope proof変更で選択されたartifactだけが追問停止し、別scopeの異版artifactは保存・再読・描画される境界を接続した。次はV1/V4 artifact種別でも同じ局所失効境界が成り立つかを必要範囲で確認する。ただし全履歴・schema migration・並行性保証へ拡張したものとは扱わず、別scope controlを保った限定fixtureにする。
