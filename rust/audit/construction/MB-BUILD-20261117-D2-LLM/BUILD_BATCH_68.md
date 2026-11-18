# BUILD_BATCH_68 — Bounded current-memory revalidation for Ask/AskWhy Recovery

**BASE HEAD:** `c9f98154d63e7d7bffbd8cc19bf836fe370dce0d` (`main` = `origin/main`, clean before this batch). **Version/schema:** `0.7.0-alpha.11` / `minidora-rust/0.7.0-alpha.11`. **実行日:** 2026-11-18 Asia/Tokyo. **Scope:** E1-W10 / CAP-D02の限定施工。**Tested source diff SHA-256:** `8872784bb832cb7689c265e46741c7a5ad516b8a4523ab31f59d3c8936703b64`.

## 実装範囲

pending file effectの前に行う`Ask`/`AskWhy` proof current-recheckに、Core既存の`indexed_relation_evaluation`を呼ぶrevision-matched Store wrapperを追加した。Recoveryはheadと同一revisionのGoal-related relation/condition/dependency projectionからqueryし、Cold/stale indexではauthenticated full-snapshot fallbackを維持する。

- 元Goalの採用時に使ったimmutable actor-memory revision replayは引き続きfull snapshotで行う。
- Coreの既存`SUPPORTED`/`CONTRADICTED`判定、parser-version別legacy dependency、current proof dependencyとguaranteeを維持。新しい判断主体や保存schemaを追加しない。
- project後のquery outcomeを全memory queryと比較し、status・supporting Facts・Rules・dependencies・guaranteeの一致を確認する。

## 回帰fixture

日本語`Ask`から支持される`USER_PREMISE`の回答を採用。small snapshotのindexをcoldにしてfull-snapshot fallbackが通ることを確認後、82 Factsと1 Ruleを持つcurrent memoryに対して80件のoff-scope Factsを加え、headを更新した。

現行full-snapshot queryとindex queryは同じstatus・Facts・Rules・dependencies・guaranteeを返した。index queryが関係評価へ渡したprojectionは1 Fact / 0 Rulesで、82 Factsの現行memoryより小さい。Recoveryはcurrent headとの照合を通過し、adoption時のexact revision replayも維持した。counterexample Factの無効化後は`RECOVERY_PROOF_INVALIDATED`で停止し、保存済みfile bytesは不変だった。

## 検証

Linux x86_64 / kernel `6.18.57`、Rust `1.99.1 (58cf8f9c3 2026-10-13)`、Cargo `1.99.1 (5f94df478 2026-08-27)`。共有target cacheは`/workspace/.minidora-cloud/rust-target`。

| Command | 結果 | 保存log |
| --- | --- | --- |
| `cargo test -p minidora-engine --locked --lib recovery::tests::current_relation_answer_lineage_and_guarantee_replay_for_recovery -- --exact --nocapture` | PASS: exact revision replay、cold fallback、82-Fact current memoryから1-Fact projection、full/index proof一致、無効化停止 | [focused Recovery log](b68-focused-recovery-test.log) |
| `cargo test -p minidora-engine --locked --lib indexed_relation -- --nocapture` | PASS: 3 filtered projection tests | [indexed relation tests](b68-indexed-relation-tests.log) |
| `cargo check -p minidora-engine --all-targets --locked` | PASS | [engine check](b68-engine-check.log) |
| `cargo clippy -p minidora-engine --lib --locked -- -D warnings` | PASS | [engine library Clippy](b68-engine-library-clippy.log) |
| `cargo fmt --all -- --check` | PASS | [format check](b68-fmt-check.log) |
| `git diff --check` | PASS | [diff check](b68-diff-check.log) |

## CAP-D02 / W10判定と限界

このbatchがboundedにするのは、pending file effect直前に行う`Ask`/`AskWhy`の**現行head proof再照合だけ**。immutable retained revision replay、cold/stale large-history fallback、極端な同一scope/condition closure、同時writer、他Recovery/learning consumer、全CAP-D02 acceptanceは未確認。W10は`PARTIAL`、CAP-D02は`UNVERIFIED`のまま。E1 exit、release、deploymentは主張しない。

今回照合したD2文書SHA-256:

- `01_施工仕様書.md`: `de618da9d50f248a7e984ad807aab4a306f17204eb9e722da387aa7d587573b0`
- `02_施工工程表.md`: `d1afc8a3e225fcbc21ef39eb3152ca9c64ffe79ee61d0702220802700570ce7a`
- `03_Codex施工指示書.md`: `14bcce642e3c6281f4922e1ac788dcef6e1328451aebf82d245922cb9d901d3d`

## 生成物・資産の5分類

- **保持:** 既存source、履歴3点セット、保存schema、immutable actor-memory revisions、Git履歴を保持。
- **保存:** Core/Recovery実装、Recovery regression、BUILD_BATCH_68、machine observation、検証log、CAP-D02 traceを保存。
- **有効:** `Ask`/`AskWhy` current-recheckにrevision-matched relation projectionを利用。共有Rust target cacheを再利用。
- **廃棄:** 0 files / 0 records。移動・削除・cache cleanupなし。
- **保留:** immutable revision大規模history replay、cold fallback scaling、極端なclosure、同時writer、残るRecovery/Core consumerと全体系受入。

Windows固有asset/storageはこのLinux実行では対象外。機械可読な詳細は[B68 machine observation](b68-build-observation.json)を参照。
