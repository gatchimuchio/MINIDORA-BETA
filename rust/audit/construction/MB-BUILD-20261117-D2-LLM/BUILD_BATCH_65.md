# BUILD_BATCH_65 — Bounded current-memory revalidation for comparison Recovery

**BASE HEAD:** `266d0575b0e9c04ab9768ce2bf31b656ebaa62eb` (`main` = `origin/main`, clean before this batch). **Version/schema:** `0.7.0-alpha.11` / `minidora-rust/0.7.0-alpha.11`. **実行日:** 2026-11-18 Asia/Tokyo. **Scope:** E1-W10 / CAP-D02の限定施工。**Tested source diff SHA-256:** `677fcde46eb0a998ef15f93682cfd836b964c20fb60c0888d6e7f5706e5cefc6`.

## 実装範囲

保留中ファイル出力を確定する直前の、`CompareRelationInventories`の**現行actor-memory再照合**をStoreのrevision-matched `memory_index_scope`から行う。左右inventoryとv3/v4比較に必要なsupporting Rulesは一つのindex transactionから取り、Coreの共通比較検証器がartifact identity、inventory、内容合成、proof dependencies、plan units、保証を引き続き照合する。

- 比較scopeとsealed `max_actions`からGoal関連候補だけを投影し、左右とも既存のper-subject allowanceを適用する。
- indexがhead revisionに一致しない場合は`None`を返し、従来どおりcurrent snapshotをロードしてfull-snapshot検証する。古い状態の互換経路を残す。
- 採用時に保存されたimmutable actor-memory revisionの厳密な再生経路は変更しない。これは引き続きfull snapshotを読み、今回のbounded sliceには含まれない。
- artifact形式、永続memory schema、Coreの証明評価と採否責任、Recoveryのeffect確認境界は変更しない。

## 回帰fixture

日本語の保存済み比較を`max_actions=32`で採用し、Engineを開き直した。小さな現行snapshotでindex-stateをcoldにしてRecoveryが旧full-snapshot fallbackを使えることを確認した。その後、比較scope外の80 Factsを80個の別scopeに追加してcurrent memoryを更新した。

旧current full-snapshot検証器は除外履歴を含む全走査を32 action内に収められず、`RELATION_BUDGET`で停止した。同じcurrent headでRecoveryのindex検証は成功した。採用時のexact revision replayは維持され、比較元Factの無効化後は`RECOVERY_PROOF_INVALIDATED`で停止し、既存の出力bytesも変更しなかった。

## 検証

Linux x86_64 / kernel `6.18.57`、Rust `1.99.1 (58cf8f9c3 2026-10-13)`、Cargo `1.99.1 (5f94df478 2026-08-27)`。共有target cacheは`/workspace/.minidora-cloud/rust-target`。

| Command | 結果 | 保存log |
| --- | --- | --- |
| `cargo test -p minidora-engine --locked --lib recovery::tests::relation_comparison_recovery_pins_and_rechecks_its_memory_revision -- --exact --nocapture` | PASS: cold fallback、80 off-scope Facts、full-scan budget stop、index再照合、exact revision再生、無効化停止 | [focused Recovery log](b65-focused-recovery-test.log) |
| `cargo test -p minidora-engine --locked --lib relation_comparison -- --nocapture` | PASS: 5 filtered library tests | [library tests](b65-relation-comparison-library-tests.log) |
| `cargo test -p minidora-engine --locked --test core_atomic_relation_comparison_42 --test core_atomic_relation_comparison_46 --test core_atomic_relation_comparison_47` | PASS: 3 regression harnesses | [comparison regressions](b65-comparison-regressions.log) |
| `cargo check -p minidora-engine --all-targets --locked` | PASS | [engine check](b65-engine-check.log) |
| `cargo clippy -p minidora-engine --lib --locked -- -D warnings` | PASS | [engine library Clippy](b65-engine-library-clippy.log) |
| `cargo fmt --all -- --check` | PASS | [format check](b65-fmt-check.log) |
| `git diff --check` | PASS | [diff check](b65-diff-check.log) |

初回compileでは比較inventoryのowned/reference比較で`E0308`が出たため参照比較へ修正し、上記の最終検証を通した。最初のshellはCargoのPATHが未設定だったため、既存のRust 1.99.1を明示して実行した。経過log: [compile diagnostic](b65-initial-compile-error.log)、[toolchain path](b65-toolchain-path-resolution.log)。

## CAP-D02 / W10 判定と限界

このbatchがboundedにするのは、pending comparison file effectの確認直前に行う**現行headとの再照合だけ**である。immutable retained revisionのfull-snapshot replay、cold/stale index fallbackの大規模履歴コスト、同一scopeに履歴が集中した場合、同時writer、全Recovery consumer、全CAP-D02 acceptanceは未確認。よってW10は`PARTIAL`、CAP-D02は`UNVERIFIED`のまま。E1 exit、release、deploymentは主張しない。

今回照合したD2文書SHA-256:

- `01_施工仕様書.md`: `de618da9d50f248a7e984ad807aab4a306f17204eb9e722da387aa7d587573b0`
- `02_施工工程表.md`: `d1afc8a3e225fcbc21ef39eb3152ca9c64ffe79ee61d0702220802700570ce7a`
- `03_Codex施工指示書.md`: `14bcce642e3c6281f4922e1ac788dcef6e1328451aebf82d245922cb9d901d3d`

## 生成物・資産の5分類

- **保持:** baseのtracked paths、immutable memory revisionと比較artifact契約を保持。
- **保存:** Core/Recovery実装、Recovery regression、CAP-D02追跡、BUILD_BATCH_65とmachine observation、検証logを追加。
- **有効:** revision-matched current-head comparison revalidation through the existing scope index。
- **廃棄:** 0 files / 0 records。移動・削除なし。
- **保留:** exact historical replay projection、large-history cold fallback、同一scope広履歴、同時writer、全体CAP-D02 acceptance。

機械可読な詳細は[B65 machine observation](b65-build-observation.json)を参照する。
