# BUILD_BATCH_78 — Saved conjunction ExplainPrevious Recovery revalidation

**BASE HEAD:** `92fb7ed00c308b11c07fd7bf60ba94a5f0e55f22` (`main` = `origin/main`, clean before this batch). **実装commit:** `6a0beb8a17f8bf3fed8bb31a8ac0a13765082145` (`Revalidate saved conjunction explanations`). **Version/schema:** `0.7.0-alpha.11` / `minidora-rust/0.7.0-alpha.11`. **実行日:** 2026-11-18 Asia/Tokyo. **Scope:** E1-W10 / CAP-D02の限定施工。**施工source diff SHA-256:** `deca54c57d32aaf27abd4a4047c95daca852ccca2934012834558c429b29697c`.

## 実装範囲

保存済み`AskConjunction`結果を参照する`ExplainPrevious` Recoveryに、現actor-memory headでのproof再確認を接続した。

- prior input/resultのactor・case・ID・commit状態を照合し、prior ContentPlan、render artifact、保持されている採用時memory revisionを検証する。
- 現行memoryは既存`indexed_conjunction_relation_answers_from_store`を使って同一revisionのindexから節ごとに再評価する。indexがcold/staleなら認証済みcurrent snapshotへ退避する。
- 節のstatus、proof dependencies、guarantee、採用済みrequired unitsを保存結果と照合し、不一致を`RECOVERY_CONTEXT_PROOF_CHANGED`として停止する。
- Recovery対象の説明結果が前回結果そのものに結び付いていることも検証する。**Coreの既存表示内容を説明文へ拡張する施工ではない。**
- schema・公開API・別判断主体・保存形式の変更なし。

実経路：保存済み型付き連言入力とprior ResultRecord → 現行memory revisionの節別QueryOutcome（revision一致index、またはfull snapshot）→ `verify_conjunction_outcomes`でprior dependencies/guarantee/unitsと照合 → ExplainPrevious Recoveryが元の採用済み結果との同一性を照合 → proof変更時はRecovery停止。Outputの既存byteは再作用しない。

## 回帰fixture

「ココは鳥かつ動物ですか」のprior結果を保存し、「詳しく説明して」の通常Core経路からRecoveryを実行した。再起動後の再確認と、index stateを削除したsmall-memory full-snapshot fallbackが通過した。範囲外Factを80件加え、current memoryは82 Factsになった。current revisionのindex経路とfull snapshot経路は各節のstatus/facts/rules/dependencies/guaranteeが一致し、Explanation Recoveryも通過した。鳥のpremiseを無効化するとRecoveryが`RECOVERY_CONTEXT_PROOF_CHANGED`で止まり、既存target bytesは不変だった。

### 試験仮説の修正記録

最初のfocused試行は「80件の範囲外Factでfull snapshotが`RESOURCE_LIMIT`になる」と期待したが、full-snapshot再評価は`Ok(())`を返し、テストが失敗した。これは試験fixtureの予測が不成立だった観測で、性能上限や製品障害の証拠ではない。範囲外Factだけで予算超過を主張するassertionを外し、snapshot/index両経路の出力同値とindex revision一致を検査した。

次の試行は無効化要求のfixtureで`engine.execute`へ`alice`ではなく`invalidation-case`を渡し、`Lifecycle::Reject`となった。呼出actorを`alice`に直し、focused試験は最終的にPASSした。失敗出力は[`b78-focused-conjunction-explanation-attempt-2.log`](b78-focused-conjunction-explanation-attempt-2.log)に保持した。

記録をstageした最初の`git diff --check`は4つのtee log末尾の余分な空行を指摘した。ログ本文は維持して余分な末尾空行だけを整え、最終のstage済み`git diff --check`はPASSした。

## 検証

Linux x86_64 / kernel `6.18.57`、Rust `1.99.1 (58cf8f9c3 2026-10-13)`、Cargo `1.99.1 (5f94df478 2026-08-27)`。共有target cacheは`/workspace/.minidora-cloud/rust-target`。

| Command | 結果 | 保存log |
| --- | --- | --- |
| `cargo test -p minidora-engine --locked --lib recovery::tests::conjunction_explanation_rechecks_the_current_memory_revision -- --exact --nocapture` | PASS: 1 test | [focused Recovery log](b78-focused-conjunction-explanation.log) |
| `cargo test -p minidora-engine --locked --lib explanation_pins_and_rechecks_its_memory_revision -- --nocapture` | PASS: 5 tests | [existing explanation Recovery tests](b78-explanation-recovery-tests.log) |
| `cargo test -p minidora-engine --locked --lib recovery::tests::conjunction_answer_recovery_pins_and_rechecks_its_memory_revision -- --exact --nocapture` | PASS: 1 test | [conjunction Recovery regression](b78-conjunction-answer-regression.log) |
| `cargo check -p minidora-engine --all-targets --locked` | PASS | [engine check](b78-engine-check.log) |
| `cargo clippy -p minidora-engine --lib --locked -- -D warnings` | PASS | [engine library Clippy](b78-engine-library-clippy.log) |
| `cargo fmt --all -- --check` | PASS | [format check](b78-fmt-check.log) |
| `git diff --check` | PASS | [diff check](b78-diff-check.log) |

## E1-W10 / CAP-D02判定と限界

このbatchが確認した範囲は保存済み連言結果を参照するExplainPrevious Recoveryのcurrent-head再確認のみ。対象sourceの変更と関連試験は限定される。W10は`PARTIAL`、CAP-D02は`UNVERIFIED`。他の未対象Recovery consumer、同一subject拡大、条件closure、多重writer、workspace全runtime suite、E1/E2 acceptanceは未確認。E1 exit、全体系合格、release、deploymentは主張しない。

今回照合した現行D2文書SHA-256:

- `01_施工仕様書.md`: `de618da9d50f248a7e984ad807aab4a306f17204eb9e722da387aa7d587573b0`
- `02_施工工程表.md`: `d1afc8a3e225fcbc21ef39eb3152ca9c64ffe79ee61d0702220802700570ce7a`
- `03_Codex施工指示書.md`: `14bcce642e3c6281f4922e1ac788dcef6e1328451aebf82d245922cb9d901d3d`

着工時local refsは`codex/backup-main=e427b1919e4513e9038b9dedaa9743a1be23c457`、`codex/backup-main-prev=8317652ab43f933522e66cd13867f9cce6c64745`。remote同名tagsも同SHA、remote backup branchesは`8c425bf28af09b0284101836da598d53fb8d6756` / `e9a275591d9f062d033c07d2a9a4e239414269a6`のまま。rollback refは確認済みbase `92fb7ed00c308b11c07fd7bf60ba94a5f0e55f22`。

## ローカル資産の5分類

- **保持:** D2正本、既存Git履歴、凍結β/user assetsに変更なし。
- **保管済み:** B78 report、machine observation、8 verification/attempt logs。
- **稼働中:** [recovery.rs](/workspace/MINIDORA-BETA/rust/crates/minidora-engine/src/recovery.rs)、既存shared Cargo target cache（build/testで更新）。
- **ゴミ箱移動済み:** 0 files。
- **保留:** 0 local assets。test-owned TempDirはharness終了時に片付き、残存pathなし。手動削除・cache cleanupなし。Windows storage handlingはLinux実行では非該当。

機械可読な詳細は[B78 machine observation](b78-build-observation.json)を参照。
