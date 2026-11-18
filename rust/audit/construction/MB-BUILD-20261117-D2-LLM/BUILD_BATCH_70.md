# BUILD_BATCH_70 — Bounded AskPreviousRelation Recovery current-head revalidation

**BASE HEAD:** `272691be2a1951382ccbf7caa74c51c78451e204` (`main` = `origin/main`, clean before this batch). **Version/schema:** `0.7.0-alpha.11` / `minidora-rust/0.7.0-alpha.11`. **実行日:** 2026-11-18 Asia/Tokyo. **Scope:** E1-W10 / CAP-D02の限定施工。**Tested source diff SHA-256:** `1f7f9204bbfeca5597aba48170a8239629b39304b4d44b906f90e0b7201263d3`.

## 実装範囲

pending file effectのRecovery current-head照合に、Core通常経路が使う`indexed_previous_relation_query`を共有した。revision-matched Store index transaction内で、直前の確定ResultRecordに含まれるFact IDだけを起点にして主語/scopeを復元し、関係proof closureを評価する。prior dependency lookup、projection、queryは従来の`max_actions`枠を共有する。

- Recoveryは現在のresolved proposition、回答単位、proof dependencies、guaranteeを照合する。
- cold/stale indexではauthenticated full-current-snapshot fallbackを維持。
- adoption時immutable actor-memory revision replayはfull snapshotのまま。schemaや新しい判断主体は追加しない。

## 回帰fixture

既存の直前結果にひもづく日本語`AskPreviousRelation`をEngine reopen後に検証。current memory 73 Facts / 5 Rulesでindex stateを削除し、Recoveryのfull-snapshot fallbackが通ることを確認してから、80件のoff-scope Factsを追加して153 Factsのcurrent headを作った。

従来の全memory queryとindexed queryは、直前結果から解決された命題、status、Facts、Rules、dependencies、guarantee、required content unitsが一致。indexed proof memoryは153 Factsのfull memoryより小さい。exact adoption-revision replayも通過し、前提Fact無効化後は`RECOVERY_PROOF_INVALIDATED`で停止し、保存済みtarget bytesは変わらなかった。

## 検証

Linux x86_64 / kernel `6.18.57`、Rust `1.99.1 (58cf8f9c3 2026-10-13)`、Cargo `1.99.1 (5f94df478 2026-08-27)`。共有target cacheは`/workspace/.minidora-cloud/rust-target`。

| Command | 結果 | 保存log |
| --- | --- | --- |
| `cargo test -p minidora-engine --locked --lib recovery::tests::previous_relation_recovery_pins_and_rechecks_its_memory_revision -- --exact --nocapture` | PASS: 1 test | [focused Recovery log](b70-focused-recovery-test.log) |
| `cargo test -p minidora-engine --locked --lib previous_relation -- --nocapture` | PASS: 3 filtered tests | [previous-relation tests](b70-previous-relation-tests.log) |
| `cargo check -p minidora-engine --all-targets --locked` | PASS | [engine check](b70-engine-check.log) |
| `cargo clippy -p minidora-engine --lib --locked -- -D warnings` | PASS | [engine library Clippy](b70-engine-library-clippy.log) |
| `cargo fmt --all -- --check` | PASS | [format check](b70-fmt-check.log) |
| `git diff --check` | PASS | [diff check](b70-diff-check.log) |

検査中、fixtureのRule件数を3と仮定した一時assertが実数5で失敗した。固定件数は受入条件ではないため除去し、失敗ログを[adjustment log](b70-initial-fixture-assertion-failure.log)として保存した。Proof/outcome比較、projection縮小、最終focused testとfiltered testsはPASS。

## CAP-D02 / W10判定と限界

今回boundedにしたのはpending file effect直前の`AskPreviousRelation` current-head再照合だけ。immutable retained-revision大規模再生、cold/stale large-history scaling、極端な同一scope/condition closure、同時writer、他Recovery/learning consumer、full engine package、workspace all-target Clippyおよび全CAP-D02 acceptanceは未確認。W10は`PARTIAL`、CAP-D02は`UNVERIFIED`のまま。E1 exit、release、deploymentは主張しない。

今回照合したD2文書SHA-256:

- `01_施工仕様書.md`: `de618da9d50f248a7e984ad807aab4a306f17204eb9e722da387aa7d587573b0`
- `02_施工工程表.md`: `d1afc8a3e225fcbc21ef39eb3152ca9c64ffe79ee61d0702220802700570ce7a`
- `03_Codex施工指示書.md`: `14bcce642e3c6281f4922e1ac788dcef6e1328451aebf82d245922cb9d901d3d`

## 生成物・資産の5分類

- **保持:** Coreの単一判断責任、保存schema、採用時immutable revisionsとGit履歴を保持。
- **保存:** Core/Recovery変更、回帰fixture、B70 report、machine observation、成功/補正ログ、CAP-D02 traceを保存。
- **有効:** `AskPreviousRelation` current-head Recoveryにrevision-matched prior-dependency projectionを利用。共有Rust target cacheを再利用。
- **廃棄:** 0 files / 0 records。移動・削除・cache cleanupなし。
- **保留:** immutable revisionの大規模再生、cold/stale fallback scaling、同時writer、残るRecovery/Core consumersと全体系受入。

Windows固有asset/storageはこのLinux実行では対象外。機械可読な詳細は[B70 machine observation](b70-build-observation.json)を参照。
