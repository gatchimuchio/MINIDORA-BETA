# BUILD_BATCH_69 — Bounded AskConjunction Recovery current-head revalidation

**BASE HEAD:** `5afbdb676216e546f25b39123a37c8129e1370eb` (`main` = `origin/main`, clean before this batch). **Version/schema:** `0.7.0-alpha.11` / `minidora-rust/0.7.0-alpha.11`. **実行日:** 2026-11-18 Asia/Tokyo. **Scope:** E1-W10 / CAP-D02の限定施工。**Tested source diff SHA-256:** `1f85fefac6542168dbaa69d366eac81c2cf0e7f7d746938b8d4959c006d93739`.

## 実装範囲

pending file effectのRecovery current-head照合で、`AskConjunction`の各clauseをrevision-matched Store indexの同一transactionから評価するCore wrapperを追加した。Recoveryは既存のclause数制約とper-relation budget splitを使い、clause status、required content units、proof dependencies、guaranteeを引き続き照合する。

- adoption時のimmutable actor-memory revision replayは従来どおりfull snapshot。
- current indexがcold/staleならauthenticated current full-snapshot fallback。
- 新しい判断主体、保存形式、schema versionは追加していない。

## 回帰fixture

日本語の二clause `AskConjunction`を二つのdirect premiseから採用し、`max_actions=32`で照合した。Engine reopen後にindexをcoldにしてsmall-snapshot fallbackを確認し、80件のoff-scope Factsを追加してcurrent memoryを82 Factsにした。

各clauseのindex projectionは1 Factずつだった。full-snapshotとindexed outcomesはstatus、Facts、Rules、dependencies、guaranteeが一致。Recovery current-head照合とexact retained-revision replayが通り、前提Fact無効化後は`RECOVERY_PROOF_INVALIDATED`で停止し、保存済みtarget bytesは変化しなかった。

## 検証

Linux x86_64 / kernel `6.18.57`、Rust `1.99.1 (58cf8f9c3 2026-10-13)`、Cargo `1.99.1 (5f94df478 2026-08-27)`。共有target cacheは`/workspace/.minidora-cloud/rust-target`。

| Command | 結果 | 保存log |
| --- | --- | --- |
| `cargo test -p minidora-engine --locked --lib recovery::tests::conjunction_answer_recovery_pins_and_rechecks_its_memory_revision -- --exact --nocapture` | PASS: 1 test | [focused Recovery log](b69-focused-recovery-test.log) |
| `cargo test -p minidora-engine --locked --lib conjunction -- --nocapture` | PASS: 2 filtered tests | [conjunction tests](b69-conjunction-tests.log) |
| `cargo check -p minidora-engine --all-targets --locked` | PASS | [engine check](b69-engine-check.log) |
| `cargo clippy -p minidora-engine --lib --locked -- -D warnings` | PASS | [engine library Clippy](b69-engine-library-clippy.log) |
| `cargo fmt --all -- --check` | PASS | [format check](b69-fmt-check.log) |
| `git diff --check` | PASS | [diff check](b69-diff-check.log) |

## CAP-D02 / W10判定と限界

このbatchがboundedにするのは、pending file effect直前の`AskConjunction` current-head再照合だけ。大規模なimmutable revision replay、cold/stale large-history fallback、同一scope/condition closure、同時writer、他Recovery/learning consumer、full engine package、workspace all-target Clippyと全CAP-D02 acceptanceは未確認。W10は`PARTIAL`、CAP-D02は`UNVERIFIED`のまま。E1 exit、release、deploymentは主張しない。

今回照合したD2文書SHA-256:

- `01_施工仕様書.md`: `de618da9d50f248a7e984ad807aab4a306f17204eb9e722da387aa7d587573b0`
- `02_施工工程表.md`: `d1afc8a3e225fcbc21ef39eb3152ca9c64ffe79ee61d0702220802700570ce7a`
- `03_Codex施工指示書.md`: `14bcce642e3c6281f4922e1ac788dcef6e1328451aebf82d245922cb9d901d3d`

## 生成物・資産の5分類

- **保持:** 既存Core責務、保存schema、採用時immutable actor-memory revisions、Git履歴を保持。
- **保存:** Core/Recovery実装、Recovery regression、BUILD_BATCH_69、machine observation、検証log、CAP-D02 traceを保存。
- **有効:** `AskConjunction` current-head Recoveryにrevision-matched clause projectionsを利用。共有Rust target cacheを再利用。
- **廃棄:** 0 files / 0 records。移動・削除・cache cleanupなし。
- **保留:** immutable revisionの大規模再生、cold/stale large-history fallback、極端なcondition closure、同時writer、残るRecovery/Core consumersと全体系受入。

Windows固有asset/storageはこのLinux実行では対象外。機械可読な詳細は[B69 machine observation](b69-build-observation.json)を参照。
