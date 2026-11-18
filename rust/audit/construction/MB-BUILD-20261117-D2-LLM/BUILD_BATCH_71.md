# BUILD_BATCH_71 — Bounded DraftRelationProfile Recovery current-head revalidation

**BASE HEAD:** `4a116f505ea9d156261ebbbc7ff0b7cf409c2850` (`main` = `origin/main`, clean before this batch). **Version/schema:** `0.7.0-alpha.11` / `minidora-rust/0.7.0-alpha.11`. **実行日:** 2026-11-18 Asia/Tokyo. **Scope:** E1-W10 / CAP-D02の限定施工。**Tested source diff SHA-256:** `b824a4c0844c2f2acf86ece6042bfe09d41a08e90f8e48660f6bb211ad15577b`.

## 実装範囲

pending file effect前のRecovery current-head照合に、通常Coreが使うsubject indexと`relation_profile_answer_from_index`を公開せず内部wrapper経由で共有した。Recoveryは既存のsubject、`max_actions`、`max_evidence_bytes`を使い、profile units、dependencies、`HYPOTHESIS` guaranteeを照合する。

- adoption時のimmutable actor-memory revision replayは引き続きfull snapshot。
- cold/stale current indexは`None`となり、Recoveryがauthenticated full-current-snapshot fallbackを使う。
- 保存schema、profileの意味、判断責任は変更なし。

## 回帰fixture

日本語profileを`max_actions=64`で採用し、Engine reopen後、small snapshotでindexをcoldにしてfallbackを確認。さらに80件のoff-scope Factsを追加し、81 Facts / 1 Ruleのcurrent memoryを作った。

indexed profileは10,000-action full-snapshot referenceとunits、dependencies、guaranteeが一致。sealed 64-action予算での全memory profile referenceは`RESOURCE_LIMIT`となり、Recoveryのcurrent-head checkはindexを使って通過した。exact retained-revision replayも通過。支援Ruleを無効化すると`RECOVERY_PROOF_INVALIDATED`で停止し、保存済みtarget bytesは不変だった。

## 検証

Linux x86_64 / kernel `6.18.57`、Rust `1.99.1 (58cf8f9c3 2026-10-13)`、Cargo `1.99.1 (5f94df478 2026-08-27)`。共有target cacheは`/workspace/.minidora-cloud/rust-target`。

| Command | 結果 | 保存log |
| --- | --- | --- |
| `cargo test -p minidora-engine --locked --lib recovery::tests::relation_profile_recovery_pins_and_rechecks_its_memory_revision -- --exact --nocapture` | PASS: 1 test | [focused Recovery log](b71-focused-recovery-test.log) |
| `cargo test -p minidora-engine --locked --lib relation_profile -- --nocapture` | PASS: 5 filtered tests | [profile tests](b71-relation-profile-tests.log) |
| `cargo check -p minidora-engine --all-targets --locked` | PASS | [engine check](b71-engine-check.log) |
| `cargo clippy -p minidora-engine --lib --locked -- -D warnings` | PASS | [engine library Clippy](b71-engine-library-clippy.log) |
| `cargo fmt --all -- --check` | PASS | [format check](b71-fmt-check.log) |
| `git diff --check` | PASS | [diff check](b71-diff-check.log) |

検査中の仮fixture件数assertは82 Factsを期待したが、実際は81 Factsだった。初期actor memoryは1 premise Factであり、仕様条件ではないcountだけを81へ修正した。失敗ログは[adjustment log](b71-initial-fixture-fact-count-failure.log)、修正後はfocused/filtered tests PASS。

## CAP-D02 / W10判定と限界

今回boundedにしたのはpending file effect前の`DraftRelationProfile` current-head照合だけ。large same-subject scope history、condition closure、同時writer、immutable revision large-history replay、cold/stale large-history fallback、他Recovery/learning consumers、full engine package、workspace all-target Clippy、全CAP-D02 acceptanceは未確認。W10は`PARTIAL`、CAP-D02は`UNVERIFIED`。E1 exit、release、deploymentは主張しない。

今回照合したD2文書SHA-256:

- `01_施工仕様書.md`: `de618da9d50f248a7e984ad807aab4a306f17204eb9e722da387aa7d587573b0`
- `02_施工工程表.md`: `d1afc8a3e225fcbc21ef39eb3152ca9c64ffe79ee61d0702220802700570ce7a`
- `03_Codex施工指示書.md`: `14bcce642e3c6281f4922e1ac788dcef6e1328451aebf82d245922cb9d901d3d`

## 生成物・資産の5分類

- **保持:** Core profile semantic/result contracts、保存schema、immutable revisionsとGit履歴を保持。
- **保存:** indexed Core/Recovery接続、Recovery回帰、B71 report、machine observation、成功/補正logs、CAP-D02 traceを保存。
- **有効:** `DraftRelationProfile` current-head Recoveryにrevision-matched subject profile indexを利用。共有Rust target cacheを再利用。
- **廃棄:** 0 files / 0 records。移動・削除・cache cleanupなし。
- **保留:** same-subject scale、condition closure、同時writer、immutable replay/cold fallback scaling、残るRecovery consumersと全体系受入。

Windows固有asset/storageはこのLinux実行では対象外。機械可読な詳細は[B71 machine observation](b71-build-observation.json)を参照。
