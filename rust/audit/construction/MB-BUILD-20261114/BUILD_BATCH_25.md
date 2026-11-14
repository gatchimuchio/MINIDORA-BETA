# BUILD_BATCH_25 — 文脈復元Assertの原文役割を証明Factへ接続

日付: 2026-11-14

## 対象と施工単位

- 対象: `gatchimuchio/MINIDORA-BETA`、`main`
- 着工時`HEAD` / `origin/main`: `6addf6116ccc1711c4519f9707f7d67b1649e9fc`（B24）
- 主語: MINIDORA本体の中核。
- 目的: 直前の同一case結果で主語を復元する日本語Assertにも、原文の役割spanを封印input由来のevidenceとして付け、CoreのFact・再開後のproof・後続Core問合せまで保つ。
- 実Consumer: `Engine::execute` → `input::seal` v79 → `MeaningIR.source_roles`と`context_result_ref` → Coreの直前ResultRecord/関係proof検証と主語復元 → `relations::assert_fact_with_source_evidence` → actor所有のsealed InputBundleに対するStore再検証 → 保存再開後の関係proof → 通常Core Askが同じFact IDを消費。
- 範囲: 既存parserが解釈する、単数の直前文脈主語を使う日本語`AssertPreviousRelation`の限定されたコピュラ・他動詞形。原文の`それ`等のpronoun spanは、復元後のFact subjectと別に保持する。文脈から複数の入力・証拠を一般合成する機能ではない。
- 権限: オーナーの既存実装・Git保持指示内。別repo、外部送信、配布、リリースは行わない。

## 開始時の同期

`AGENTS.md`、三点セット全文とマニフェスト、R2仕様の入力・来歴・Core・Store契約、現行実装と要求台帳を確認した。作業前ツリーはclean、`main`と`origin/main`は上記SHA。ローカル回復tagは`codex/backup-main=6addf61`、`codex/backup-main-prev=8c425bf`。既存タグは完了時にこのB25 commitとその直前mainへ進める。

ユーザー提供の3点セットZIPと追跡済みコピーはSHA-256 `1f65df9f67448406d0fe1e9d3b1e13981afa236f352223cbb61ce1541bc1371e` で一致。仕様書・工程表・Codex施工指示書も`MANIFEST.tsv`と一致した。三点セット資料は変更していない。

Linux x86_64、Rust/Cargo 1.99.1。共有の`/workspace/.minidora-cloud/rust-target`を利用し、既存cacheを削除していない。最初のformatコマンドは環境の`PATH`にCargoがなく実行できず、そのログを保存した。明示PATHで同コマンドを再実行して成功した。

## 変更前の欠落と実装

B24までに直接Assertのコピュラ・他動詞source-role mapはFactへ投影されたが、文脈からsubjectを解決する`AssertPreviousRelation`はFactへのsource evidence投影対象外だった。Coreは直前の同一case関係結果とproofを検証しsubject/scopeを復元できる一方、復元後のFactにその入力の原文role spanが残らなかった。

B25では限定された日本語コピュラ・他動詞形で`AssertPreviousRelation`専用root以下にrole spanを捕捉し、source evidence検証に単数の前文脈subjectとsealed `context_result_ref`を必須化した。Coreが従来通り直前結果・proofを検証し、subjectを復元した後、そのcontextual inputの証拠を`USER_PREMISE` Factへ添付する。role mapの一意性・byte範囲・typed intent整合を満たさない場合はevidenceを投影しない。アンカーがなければ既存Core動作通り`SUSPEND`しFactを作らない。

Parser versionは`declarative-ja-en-v79-contextual-assertion-source-ranges`へ更新した。parser versionはsealed request fingerprintの一部である。既存v78 bundleは旧版として残る。同じ`(case_id,input_id,input_version)`でversionの異なるparser bundleを登録するとidentity conflictとなり得るため、upgrade後の呼出し側は新しい`input_version`を使う。DB schemaやFact wire fieldは変更していない。

## 実観測

- seed premise `[scope=biology]ミナは毒を食べません。`とその同一case relation Askをcommitし、Askの実ResultRecordをanchorとして使用した。
- contextual input `[scope=biology]それは水を使う。`は同一anchor idを`context_result_ref`に持つ。source roleは文字通りのsubject `それ`、助詞`は`、object `水`、助詞`を`、predicate `使う`、明示scope `biology`の各原UTF-8範囲を記録した。
- Coreが復元したFact subjectは`ミナ`で、source evidence上のsubjectは`それ`のまま。raw SHA-256、input fingerprint、bundle fingerprint、semantic signature、parser versionと全role spanは同じ封印bundleに結び付く。
- Engineを閉じて再度開いた後、Storeはsealed inputに対して証拠を再検査した。関係proofは`SUPPORTED`と同じevidence付きFactを返し、通常Core follow-upのdependenciesに同じFact IDが含まれた。
- 別caseのコピュラ文脈assert `[scope=biology]それは動物です。`でもsubject復元・source roleのFact保持・後続queryによるFact消費を観測した。
- predicate roleを重複させたrole mapはFact evidenceへ昇格しなかった。直前結果のないassertは`SUSPEND`、空dependencies、sealed `context_result_ref=None`、evidence付きFactなしとなった。
- これはユーザー入力と保存状態のidentity/provenance linkであり、述語の外部世界の真偽を確認しない。Engine API/Core routeの観測で、CLI/UI、render/delivery receiptは通していない。

## 検証

| コマンド | 結果 |
|---|---|
| `cargo test -p minidora-engine --locked --lib contextual_japanese_assertion_source_roles_follow_the_fact_into_reopened_proofs -- --nocapture` | PASS: focused Core route。copular/transitive、evidence、reopen、proof、follow-up、ambiguous role、missing-contextを観測 |
| `cargo test -p minidora-engine --locked` | PASS: 241 unit + 12 integration = 253 passed、0 failed; doc tests 0 |
| `cargo fmt --all -- --check` | PASS |
| `cargo check --workspace --all-targets --locked` | PASS |
| `cargo clippy --workspace --all-targets --locked -- -D warnings` | PASS |
| `cargo build --workspace --locked` | PASS |
| `git diff --check` | PASS |

コマンドごとの出力は`/workspace/.minidora-cloud/b25-contextual-assert-source-evidence/`に保存した。初回PATH不備の失敗ログと成功した再実行のログを両方保持する。

Full workspace runtime/all-features、CLI/UI/output delivery、Windows/macOS、実機8GB条件、独立QA、全要求受入評価はNOT_RUN。A04/A15/E08と要求93件全体のacceptanceは`UNVERIFIED`のまま。全体系完成、三機全域通電、一般日本語理解、releaseは宣言しない。

詳細なroute JSONは[validation observation 08](../../bb-input-machine-observation-08-v79-contextual-assert-source-validation.json)。三点セットの原本・三文書は[MB-BUILD-20261114](.)に保持する。

## 変更パス

- `rust/crates/minidora-engine/src/input.rs`
- `rust/crates/minidora-engine/src/core.rs`
- `rust/crates/minidora-engine/src/relations.rs`
- `rust/crates/minidora-engine/tests/input_machine_observation_01.rs`
- `rust/crates/minidora-engine/tests/input_machine_observation_02.rs`
- `rust/audit/IMPLEMENTATION_STATUS.md`
- `rust/audit/requirements.tsv`
- `rust/audit/bb-input-machine-observation-08-v79-contextual-assert-source-validation.json`
- 本`BUILD_BATCH_25.md`

## B25資産の5分類

分類対象はB25で変更・生成・使用した有限資産。

| 分類 | 件数 | 内容 |
|---|---:|---|
| retained | 9 paths | 5 code/test paths、status、requirements、validation JSON、本施工記録をGitへ保持 |
| archived | 16 files | `/workspace/.minidora-cloud/b25-contextual-assert-source-evidence/`のformat/focused/package test/workspace check/Clippy/build/diff-checkログ。初回PATH失敗と再実行を含む |
| active | 3 directories | `/workspace/.minidora-cloud/rust-target`、`/workspace/.cargo`、既存`rust/target`。共有cacheとして再利用し、既存`rust/target`は変更しない |
| trash | 0 | trash移動なし、永久削除なし |
| pending | 0 | B25で生成・使用した資産の未分類なし |

統合テストが作る一時stateはfixture cleanupで除去された。三点セットは既存のtracked archive/memberとハッシュ一致を確認し、変更していない。
