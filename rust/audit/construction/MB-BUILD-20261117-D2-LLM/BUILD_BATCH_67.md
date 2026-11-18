# BUILD_BATCH_67 — Bounded current-memory revalidation for relation-object Recovery

**BASE HEAD:** `3884c8c758068597869bc1eef9a580e4f64a6f5a` (`main` = `origin/main`, clean before this batch). **Version/schema:** `0.7.0-alpha.11` / `minidora-rust/0.7.0-alpha.11`. **実行日:** 2026-11-18 Asia/Tokyo. **Scope:** E1-W10 / CAP-D02の限定施工。**Tested source diff SHA-256:** `74ee48f51f944e01e7a085a781708c3978252271de6a5f0b9b56e38f69cf9419`.

## 実装範囲

保留中ファイルeffectを確認するRecoveryの`AskRelationObjects` current-head再照合を、Core通常routeと共有するrevision-matched relation-object index evaluatorへ接続した。scope・subject・predicate・極性からGoal関連のFact/Ruleを投影し、既存のrelation-object answer evaluatorへ残りのsealed budgetを渡す。Recoveryは既存契約どおりdependencies、guarantee、採用済みcontent unitを再照合する。

- indexがcold/staleなら`None`となり、従来のauthenticated full-snapshot current-checkへ戻る。
- exact retained actor-memory revisionのreplayは従来どおりfull snapshotで行う。
- report/artifactや保存schemaは変更しない。Coreが唯一の関係評価・採用主体であり、effect確認のRecovery境界も維持する。

## 回帰fixture

日本語の通常`AskRelationObjects`結果を`max_actions=32`で作成し、Engine reopen後にderived index stateをcoldにしたsmall snapshotのRecovery fallbackを確認。current memoryへ80 Factsを80個のoff-scopeへ追加し、current headを保存した。

従来のfull-snapshot current verifierは全記録走査で`RESOURCE_LIMIT`となるが、同一current headのrevision-matched index recheckは成功した。採用時のimmutable revision replayは維持され、元Factを無効化すると`RECOVERY_PROOF_INVALIDATED`で止まり、保存済みfile bytesは不変だった。

## 検証

Linux x86_64 / kernel `6.18.57`、Rust `1.99.1 (58cf8f9c3 2026-10-13)`、Cargo `1.99.1 (5f94df478 2026-08-27)`。共有target cacheは`/workspace/.minidora-cloud/rust-target`。

| Command | 結果 | 保存log |
| --- | --- | --- |
| `cargo test -p minidora-engine --locked --lib recovery::tests::object_answer_recovery_rechecks_proof_before_confirming_saved_bytes -- --exact --nocapture` | PASS: cold fallback、80 off-scope Facts、full-scan budget stop、index再照合、exact revision replay、無効化停止 | [focused Recovery log](b67-focused-recovery-test.log) |
| `cargo test -p minidora-engine --locked --lib relation_object -- --nocapture` | PASS: 2 filtered library tests | [relation-object tests](b67-relation-object-library-tests.log) |
| `cargo check -p minidora-engine --all-targets --locked` | PASS | [engine check](b67-engine-check.log) |
| `cargo clippy -p minidora-engine --lib --locked -- -D warnings` | PASS | [engine library Clippy](b67-engine-library-clippy.log) |
| `cargo fmt --all -- --check` | PASS | [format check](b67-fmt-check.log) |
| `git diff --check` | PASS | [diff check](b67-diff-check.log) |

## CAP-D02 / W10判定と限界

このbatchがboundedにするのはpending file effect直前の`AskRelationObjects` current-head再照合だけ。exact immutable revision replay、大規模履歴cold/stale fallback、same-scope scale、同時writer、他Recovery/Core/learning consumer、全CAP-D02 acceptanceは未確認。W10は`PARTIAL`、CAP-D02は`UNVERIFIED`のまま。E1 exit、release、deploymentは主張しない。

今回照合したD2文書SHA-256:

- `01_施工仕様書.md`: `de618da9d50f248a7e984ad807aab4a306f17204eb9e722da387aa7d587573b0`
- `02_施工工程表.md`: `d1afc8a3e225fcbc21ef39eb3152ca9c64ffe79ee61d0702220802700570ce7a`
- `03_Codex施工指示書.md`: `14bcce642e3c6281f4922e1ac788dcef6e1328451aebf82d245922cb9d901d3d`

## 生成物・資産の5分類

- **保持:** 既存source、履歴3点セット、保存schema、immutable memory revisions、Git履歴を保持。
- **保存:** Core/Recovery実装、Recovery regression、BUILD_BATCH_67、machine observation、検証log、CAP-D02 traceを保存。
- **有効:** relation-object current recheckにrevision-matched derived indexを使用。Rust target cacheを再利用。
- **廃棄:** 0 files / 0 records。移動・削除・cache cleanupなし。
- **保留:** exact revision大規模history projection、cold fallback scaling、same-scope scale、同時writer、残るRecovery/Core consumerと全体系受入。

Windows固有asset/storageはこのLinux実行では対象外。機械可読な詳細は[B67 machine observation](b67-build-observation.json)を参照。
