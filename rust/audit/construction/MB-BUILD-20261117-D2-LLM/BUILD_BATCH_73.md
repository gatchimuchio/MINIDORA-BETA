# BUILD_BATCH_73 — Generic ExplainPrevious Recovery current-head revalidation

**BASE HEAD:** `ec1cabaaa00e17f27d42d5046df1b24c87db7f01` (`main` = `origin/main`, clean before this batch). **Version/schema:** `0.7.0-alpha.11` / `minidora-rust/0.7.0-alpha.11`. **実行日:** 2026-11-18 Asia/Tokyo. **Scope:** E1-W10 / CAP-D02の限定施工。**Tested source diff SHA-256:** `b36a52f18f4f06554a6cd71b8a8d247f103cc92849ac48b70aad474774499240`.

## 実装範囲

Generic prior-relation-answer `ExplainPrevious`のRecovery current-head照合を、revision-matched Store relation index経由の既存Core評価へ接続した。indexed proofでも既存の依存、guarantee、説明unitsを同じ照合関数で検証する。

- Indexがcold/staleなら従来のauthenticated current full-snapshot fallbackへ戻る。
- immutable adoption revision replayは変更せず、full snapshotを再生する。
- saved comparison / common-relations / relation-object / profileの専用Recovery分岐、schema、Core責任と保存済みbytesは変更なし。
- current-head invalidationは従来どおり`RECOVERY_CONTEXT_PROOF_CHANGED`で停止する。

## 回帰fixture

通常の日本語入力で前提「ココは鳥です」、直前の明示質問「ココは鳥ですか」、続く「詳しく説明して」を実行し、explanation `max_actions=32`でファイル結果を採用した。Engine再起動後に保持revisionの再生を照合し、小さい記憶でindex stateを削除した場合のfull-snapshot fallbackも確認した。

その後、別scopeのFactを80件追加し、current memoryを81 Facts / 0 Rulesにした。relation indexは対象Fact 1件を射影した。indexed proofと10,000-action full-snapshot referenceのstatus、Facts、Rules、condition evaluations/status、dependency IDs、guarantee、explanation unitsは一致し、実Recoveryのcurrent-head照合も通過した。前提Factを無効化するとRecoveryは`RECOVERY_CONTEXT_PROOF_CHANGED`で停止し、既存target bytesは不変だった。

最初のfixture案では、無関係Fact 80件を含むfull-snapshot queryが32-action予算で`RELATION_BUDGET`になると仮定した。この仮定はqueryの実コスト計上に合わずテストが失敗したため、そのassertだけを除去した。不成立ログは保持し、最終fixtureは全snapshotとの意味同値、1件への射影、実Recovery通過と無効化停止を直接確認する。

## 検証

Linux x86_64 / kernel `6.18.57`、Rust `1.99.1 (58cf8f9c3 2026-10-13)`、Cargo `1.99.1 (5f94df478 2026-08-27)`。共有target cacheは`/workspace/.minidora-cloud/rust-target`。

| Command | 結果 | 保存log |
| --- | --- | --- |
| `cargo test -p minidora-engine --locked --lib recovery::tests::previous_answer_explanation_pins_and_rechecks_its_memory_revision -- --exact --nocapture` | PASS: 1 test | [focused Recovery log](b73-focused-recovery-test.log) |
| `cargo test -p minidora-engine --locked --lib explanation_pins_and_rechecks_its_memory_revision -- --nocapture` | PASS: 5 tests | [explanation Recovery filter](b73-explanation-recovery-filter-final.log) |
| `cargo check -p minidora-engine --all-targets --locked` | PASS | [engine check](b73-engine-check-final.log) |
| `cargo clippy -p minidora-engine --lib --locked -- -D warnings` | PASS | [engine library Clippy](b73-engine-library-clippy-final.log) |
| `cargo fmt --all -- --check` | PASS | [format check](b73-fmt-check-final.log) |
| `git diff --check` | PASS | [diff check](b73-diff-check-final.log) |

## CAP-D02 / W10判定と限界

このbatchが限定するのは、generic relation-answer `ExplainPrevious`のpending file effect前 current-head recheckだけ。immutable large-history replay、stale-index large-history fallback、same-subject scale、large condition closure、同時writer、他の全Recovery/learning consumers、full engine package runtime suite、workspace all-target Clippy、workspace all-features runtime/build、全CAP-D02 acceptanceは未確認。W10は`PARTIAL`、CAP-D02は`UNVERIFIED`。E1 exit、release、deploymentは主張しない。

今回照合したD2文書SHA-256:

- `01_施工仕様書.md`: `de618da9d50f248a7e984ad807aab4a306f17204eb9e722da387aa7d587573b0`
- `02_施工工程表.md`: `d1afc8a3e225fcbc21ef39eb3152ca9c64ffe79ee61d0702220802700570ce7a`
- `03_Codex施工指示書.md`: `14bcce642e3c6281f4922e1ac788dcef6e1328451aebf82d245922cb9d901d3d`

復元参照は開始時にlocal/tagのbase=`d372bc8cac3d6aa6cd0329bdf913460e5d81626b`、previous=`4a116f505ea9d156261ebbbc7ff0b7cf409c2850`。今回開始前にbase=`ec1cabaaa00e17f27d42d5046df1b24c87db7f01`、previous=`d372bc8cac3d6aa6cd0329bdf913460e5d81626b`へ回転済み。remote backup branchesは`8c425bf28af09b0284101836da598d53fb8d6756` / `e9a275591d9f062d033c07d2a9a4e239414269a6`のまま。

## 生成物・資産の5分類

- **保持:** typed prior relation meaning、proof dependencies、guarantee、explanation units、immutable memory revisions、既存分岐とGit履歴を保持。
- **保存:** Recovery/Core connection、large-history regression、B73 report、machine observation、検査logと初回fixture失敗を保存。
- **有効:** generic relation-answer `ExplainPrevious` current-head Recoveryに既存Core indexed relation evaluatorを利用。
- **廃棄:** 0 files / 0 records。移動・削除・cache cleanupなし。
- **保留:** large-history exact replay/fallback scale、同時writer、その他Recovery consumersと全体系受入。

Windows固有asset/storageはこのLinux実行では対象外。機械可読な詳細は[B73 machine observation](b73-build-observation.json)を参照。
