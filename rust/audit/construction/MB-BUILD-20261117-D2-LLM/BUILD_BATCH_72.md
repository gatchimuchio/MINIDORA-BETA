# BUILD_BATCH_72 — Bounded InspectPremises Recovery current-head revalidation

**BASE HEAD:** `d372bc8cac3d6aa6cd0329bdf913460e5d81626b` (`main` = `origin/main`, clean before this batch). **Version/schema:** `0.7.0-alpha.11` / `minidora-rust/0.7.0-alpha.11`. **実行日:** 2026-11-18 Asia/Tokyo. **Scope:** E1-W10 / CAP-D02の限定施工。**Tested source diff SHA-256:** `f051cd9db841b1445754b6e100682e7e7ff66625f44ff1a6e566e7920f7a899b`.

## 実装範囲

revision-bearing `InspectPremises`のRecovery current-head照合が全memoryを走査する経路を、既存Core `active_relation_inventory_projection`を呼ぶrevision-matched Store wrapperへ接続した。Recoveryは従来どおりactive dependency IDsとguaranteeを採用値と比較する。

- current indexがcold/staleならauthenticated full-current-snapshot fallbackを保持。
- actor-memory revisionを持たないparser-specific legacy resultsは従来のfull-snapshot分岐を保持。
- adoption時のimmutable memory revision replay、schema、Core判断責任は変更なし。

## 回帰fixture

日本語`InspectPremises`を`max_actions=64`で採用し、Engine reopen後にindex stateを削除してsmall-snapshot fallbackを確認。その後80件のoff-scope Factsを加えて81 Facts / 1 Ruleのcurrent memoryを作った。

Core indexのinventoryは10,000-action full-snapshot referenceと一致。sealed 64-action full-snapshot inventoryは`RESOURCE_LIMIT`となり、Recovery current-head checkはindexを使って通過した。exact retained-revision replayも通過。supporting Ruleを無効化すると`RECOVERY_PREMISES_CHANGED`で止まり、保存済みtarget bytesは不変だった。

## 検証

Linux x86_64 / kernel `6.18.57`、Rust `1.99.1 (58cf8f9c3 2026-10-13)`、Cargo `1.99.1 (5f94df478 2026-08-27)`。共有target cacheは`/workspace/.minidora-cloud/rust-target`。

| Command | 結果 | 保存log |
| --- | --- | --- |
| `cargo test -p minidora-engine --locked --lib recovery::tests::premise_inventory_recovery_pins_and_rechecks_its_memory_revision -- --exact --nocapture` | PASS: 1 test | [focused Recovery log](b72-focused-recovery-test.log) |
| `cargo test -p minidora-engine --locked --lib premise_inventory -- --nocapture` | PASS: 2 filtered tests | [premise-inventory tests](b72-premise-inventory-tests.log) |
| `cargo check -p minidora-engine --all-targets --locked` | PASS | [engine check](b72-engine-check.log) |
| `cargo clippy -p minidora-engine --lib --locked -- -D warnings` | PASS | [engine library Clippy](b72-engine-library-clippy.log) |
| `cargo fmt --all -- --check` | PASS | [format check](b72-fmt-check.log) |
| `git diff --check` | PASS | [diff check](b72-diff-check.log) |

## CAP-D02 / W10判定と限界

このbatchがboundedにするのはrevision-bearing `InspectPremises`のpending file effect前 current-head recheckだけ。legacy no-revision branches、immutable retained-revision large-history replay、cold/stale large-history fallback、同subject scale、condition closure、同時writer、他Recovery/learning consumers、full engine package、workspace all-target Clippyと全CAP-D02 acceptanceは未確認。W10は`PARTIAL`、CAP-D02は`UNVERIFIED`。E1 exit、release、deploymentは主張しない。

今回照合したD2文書SHA-256:

- `01_施工仕様書.md`: `de618da9d50f248a7e984ad807aab4a306f17204eb9e722da387aa7d587573b0`
- `02_施工工程表.md`: `d1afc8a3e225fcbc21ef39eb3152ca9c64ffe79ee61d0702220802700570ce7a`
- `03_Codex施工指示書.md`: `14bcce642e3c6281f4922e1ac788dcef6e1328451aebf82d245922cb9d901d3d`

## 生成物・資産の5分類

- **保持:** Core inventory proof/guarantee、legacy parser recovery、schema、immutable revisionsとGit履歴を保持。
- **保存:** Core/Recovery connection、regression、B72 report、machine observation、logs、CAP-D02 traceを保存。
- **有効:** revision-bearing `InspectPremises` current-head Recoveryにsubject indexを利用。共有Rust target cacheを再利用。
- **廃棄:** 0 files / 0 records。移動・削除・cache cleanupなし。
- **保留:** legacy no-revision consumers、large replay/cold fallback scaling、same-subject/condition closure、同時writer、残るRecovery consumersと全体系受入。

Windows固有asset/storageはこのLinux実行では対象外。機械可読な詳細は[B72 machine observation](b72-build-observation.json)を参照。
