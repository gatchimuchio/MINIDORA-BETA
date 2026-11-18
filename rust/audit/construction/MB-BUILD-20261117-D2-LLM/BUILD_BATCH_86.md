# BUILD_BATCH_86 — Saved AskPreviousRelation Recovery binds the exact anchor input ID

**BASE HEAD:** 5c826754a16a4d4713634c80b1865bf214da80e2 (main = origin/main, clean at batch start). **Test commit:** b1b84ef0879868b5142a21c788515d319bb86591 (Test recovery rejects rebound anchor input ID). **Version/schema:** 0.7.0-alpha.11 / minidora-rust/0.7.0-alpha.11. **実行日:** 2026-11-18 UTC. **Scope:** E1-W10 / CAP-D02 saved-result Recovery input-ID identity. **Source diff SHA-256:** 27511fd8158711294acfc850a133db43cb97262104d64f030cf754ea55b13032.

## 実装・観測範囲

B86はB84で加えたanchor sealed-input identity照合について、input_idが別の実在入力へ結び付けられた場合もRecoveryが拒否することを固定する。通常Engine経路で同じactor/caseにalternate-query version 2とexplicit-query version 1・2を保存する。alternate-queryとexplicit-queryは同じ質問文だが、異なるsealed input identityを持つ。AskPreviousRelationは直前のexplicit-query version 2をanchorとしてExplainPreviousへ進み、fixtureの許可済みtargetへ保存する。Engine再起動後、正しい保存状態のRecovery inspectは成功する。

負例ではcloneしたCaseRevision内のanchor map key、ResultRecord.id、COMMIT lifecycle、input_versionを保持し、input_idだけをexplicit-queryからalternate-queryへ変える。alternate-query version 2のsealed inputが同じactor/case内に実在し、lookupは成功するが、そのrequest identityはanchor.idと異なる。RecoveryはRECOVERY_CONTEXT_PROOF_CHANGEDで停止し、保存済みlatest revisionとtarget bytesは不変。

この施工はrecovery.rsのEngine library回帰テスト1件のみ。malformed CaseRevisionはclone上だけにあり、production code、API、schema、依存、lockfile、永続Store破損・修復、CLI/HTTP/browser経路を変更・検証しない。

| 因果位置 | 実値・consumer |
| --- | --- |
| Producer | 同じactor/caseのalternate-query version 2 sealed inputとexplicit-query version 2 anchor result |
| Consumer | Recoveryの既存Store::get_input_bundle lookupと、取得したsealed request identity == anchor.idの照合 |
| 正常 | explicit-query version 2を参照するAskPreviousRelation→ExplainPreviousがEngine再起動後に通る |
| 負例 | input_versionを固定したままanchor input_idをalternate-queryへ差し替え、別の実在sealed inputのidentity不一致で停止 |
| 実出力 | RECOVERY_CONTEXT_PROOF_CHANGED。persisted latest revisionとtarget bytesを保持 |

## 検証

Linux x86_64 / kernel 6.18.57、Rust 1.99.1 (58cf8f9c3 2026-10-13)、Cargo 1.99.1 (5f94df478 2026-08-27)。共有target cacheは/workspace/.minidora-cloud/rust-target。rust/Cargo.toml SHA-256 3a54553a0c6d15836b00cf556a821bb011da7295bc11c15ad2ad9b0d25ae8785、Cargo.lock SHA-256 b4e97fecdca8fd1fc863349b1a48eb99a925da7fed0ed6ce6cfe79bdd1cececc、rust-toolchain.toml SHA-256 d13306be0ecda81ce779baa290ffb417380d525928477f16200884b5169cca33。

| Command | 結果 | 保存log |
| --- | --- | --- |
| cargo test -p minidora-engine --lib recovery::tests::saved_previous_relation_explanation_rejects_anchor_bound_to_other_input_id --locked -- --exact --nocapture | PASS: 1 passed, 0 failed, 279 filtered out | [focused test](b86-anchor-other-input-id-test.log) |
| cargo test -p minidora-engine --lib saved_previous_relation_explanation_ --locked -- --nocapture | PASS: 6 passed, 0 failed, 274 filtered out | [Recovery filter](b86-anchor-recovery-filter.log) |
| cargo check --workspace --all-targets --locked | PASS: compile check only, no tests executed | [workspace check](b86-workspace-check.log) |
| cargo clippy -p minidora-engine --lib --locked -- -D warnings | PASS | [Engine library Clippy](b86-engine-library-clippy.log) |
| cargo fmt --all -- --check | PASS | [format check](b86-fmt-check.log) |
| git diff --check | PASS | [diff check](b86-diff-check.log) |

## 未実施・判定

full Engine/workspace runtime suite、workspace all-target Clippy、CLI/HTTP/browser入口、永続SQLite/Store物理破損と修復、長履歴、全Recovery consumer、全CAP-D02/E1受入、他OS実機、独立QA、E2-E8、release/deploymentは未実施。E1-W10はPARTIAL、CAP-D02はUNVERIFIED。E1 exit、release、deploymentを主張しない。

着工時の復旧参照：main/origin/main/local codex/backup-main branch/tag = 5c826754a16a4d4713634c80b1865bf214da80e2。local codex/backup-main-prev branch/tagと同名remote tag = c0194ce4c386af5744839b4795bb07db7f22077a。remote backup branchesは既存値8c425bf28af09b0284101836da598d53fb8d6756 / e9a275591d9f062d033c07d2a9a4e239414269a6を保持。rollback refは着工時main。

## B86資産の5分類

B86で生成・更新した現存資産は10件で、5区分合計も10件：

- **保持: 0件の新規更新資産。** D2正本、凍結β、既存履歴は保持し変更なし。
- **保管済み: 8件**（BUILD_BATCH、machine observation、6 logs）。
- **稼働中: 2件**（recovery.rsの回帰テスト1件、共有Cargo target cache）。
- **ゴミ箱移動済み: 0件。** ゴミ箱は空にせず、cache削除なし。
- **保留: 0件。** fixtureの一時DB/targetは自動cleanupされ、repo内に残留なし。

Windows用保管/OneDrive操作はLinux実行に非該当。Git管理物はGitHub mainと遠隔バックアップタグに保存し、OneDriveへ重複保存していない。機械可読記録は[b86 machine observation](b86-build-observation.json)を参照。
