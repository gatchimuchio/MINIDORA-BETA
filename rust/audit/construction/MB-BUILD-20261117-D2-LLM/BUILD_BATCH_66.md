# BUILD_BATCH_66 — Bounded current-memory revalidation for common-relations Recovery

**BASE HEAD:** `e481718da8a14fe2b5688086dfd754cabce119be` (`main` = `origin/main`, clean before this batch). **Version/schema:** `0.7.0-alpha.11` / `minidora-rust/0.7.0-alpha.11`. **実行日:** 2026-11-18 Asia/Tokyo. **Scope:** E1-W10 / CAP-D02の限定施工。**Tested source diff SHA-256:** `a9d0497409a980410c28cad0b32caab3666a4f1c895d8a16fe5a85da923f4d70`.

## 実装範囲

保留中ファイルeffectの確認時に行う、revision-bearing `AskCommonRelations` の**現行actor-memory再照合**をrevision-matched Store共通関係indexから実行する。Coreの通常routeとRecoveryで同じindexed evaluatorを使い、Recoveryは既存の検証関数でreport本文、canonical artifact bytes、proof dependencies、guarantee、required content unitを再照合する。

- Exact-scopeのnamed-subject Facts/Rulesとvariable-subject Rulesを既存selectorで取得し、左右のbounded proof queryと合わせてsealed `max_actions`を使う。
- indexがcold/staleなら`None`を返し、現行snapshotをロードする従来のfull-snapshot検証へ戻る。small-history cold fallbackを保持する。
- immutable retained actor-memory revisionの正確なreplayは引き続きfull snapshotを使う。このhistory projectionは今回の対象外。actor-memory revisionを持たない旧結果の挙動も変えない。
- artifact、永続schema、Coreの証明・採否責任、effect境界は変更しない。独立moduleや判断主体を追加しない。

## 回帰fixture

通常の日本語共通点質問で2 Factsと1 Ruleから導出済みreportを採用し、`max_actions=32`でRecoveryを検査した。Engine reopen後にindex stateをcoldにし、小さな現行snapshotでfull-snapshot fallbackが通ることを確認。その後、異なる80個のscopeに各1 Factを加え、current headを更新した。

従来のcurrent full-snapshot verifierは全記録走査が32 actionを超え`RESOURCE_LIMIT`で停止した。一方、Recoveryのrevision-matched index経路は同じ現行headで通った。採用時のimmutable revision replayは維持され、支持Factの無効化後は`RECOVERY_PROOF_INVALIDATED`で停止し、既存target bytesを変更しなかった。

## 検証

Linux x86_64 / kernel `6.18.57`、Rust `1.99.1 (58cf8f9c3 2026-10-13)`、Cargo `1.99.1 (5f94df478 2026-08-27)`。共有target cacheは`/workspace/.minidora-cloud/rust-target`。

| Command | 結果 | 保存log |
| --- | --- | --- |
| `cargo test -p minidora-engine --locked --lib recovery::tests::common_relations_recovery_pins_and_rechecks_its_memory_revision -- --exact --nocapture` | PASS: cold fallback、80 off-scope Facts、full-scan budget stop、index再照合、exact revision replay、無効化停止 | [focused Recovery log](b66-focused-recovery-test.log) |
| `cargo test -p minidora-engine --locked --lib common_relations -- --nocapture` | PASS: 3 filtered library tests | [common-relations tests](b66-common-relations-library-tests.log) |
| `cargo check -p minidora-engine --all-targets --locked` | PASS | [engine check](b66-engine-check.log) |
| `cargo clippy -p minidora-engine --lib --locked -- -D warnings` | PASS | [engine library Clippy](b66-engine-library-clippy.log) |
| `cargo fmt --all -- --check` | PASS | [format check](b66-fmt-check.log) |
| `git diff --check` | PASS | [diff check](b66-diff-check.log) |

## CAP-D02 / W10判定と限界

このbatchがboundedにするのは、`AskCommonRelations` のpending file effectを確認する直前の**現行headとの再照合だけ**。immutable retained revision replay、大規模履歴でのcold/stale full-snapshot fallback、同一scopeの大量履歴、同時writer、他Recovery consumer、残るCore/learning consumer、全CAP-D02 acceptanceは未確認。W10は`PARTIAL`、CAP-D02は`UNVERIFIED`のまま。E1 exit、release、deploymentは主張しない。

今回照合したD2文書SHA-256:

- `01_施工仕様書.md`: `de618da9d50f248a7e984ad807aab4a306f17204eb9e722da387aa7d587573b0`
- `02_施工工程表.md`: `d1afc8a3e225fcbc21ef39eb3152ca9c64ffe79ee61d0702220802700570ce7a`
- `03_Codex施工指示書.md`: `14bcce642e3c6281f4922e1ac788dcef6e1328451aebf82d245922cb9d901d3d`

## 生成物・資産の5分類

- **保持:** 既存source、履歴3点セット、artifact契約、immutable retained revisions、Git履歴を保持。
- **保存:** Core/Recovery実装、Recovery regression、BUILD_BATCH_66、machine observation、検証log、CAP-D02 traceを保存。
- **有効:** 現行headとの共通関係再照合でrevision-matched indexを使用。共有Rust target cacheは再利用。
- **廃棄:** 0 files / 0 records。移動・削除・cache cleanupなし。
- **保留:** immutable revisionの大規模履歴projection、large-history cold fallback、same-scope scale、同時writer、全Recovery consumerと全体受入。

Windows固有asset/storageはこのLinux実行では対象外。機械可読な詳細は[B66 machine observation](b66-build-observation.json)を参照。
