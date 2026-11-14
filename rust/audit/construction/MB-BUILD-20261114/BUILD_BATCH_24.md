# BUILD_BATCH_24 — 日本語他動詞Assertの原文役割を証明Factへ接続

日付: 2026-11-14

## 対象と施工単位

- 対象: `gatchimuchio/MINIDORA-BETA`、`main`
- 着工時`HEAD` / `origin/main`: `8c425bf28af09b0284101836da598d53fb8d6756`（B23）
- 主語: MINIDORA本体の中核。
- 目的: 日本語の直接他動詞Assertで封印された原文の主語・助詞・目的語・活用動詞・明示scopeを、Coreの同じUSER_PREMISE Factへ結び、保存再開後の通常関係問合せまで運ぶ。
- 実Consumer: `input::seal` → `MeaningIR.source_roles` → `input::assertion_source_evidence` → `Core::execute(Intent::Assert)` → `relations::assert_fact_with_source_evidence` → `Store::save_memory/load_memory`の封印InputBundle再検査 → `relations::query` → 後続Core Askが同Fact IDを消費。
- 範囲: 既存parserが解釈する直接日本語他動詞Assertのみ。文脈から主語を復元する`AssertPreviousRelation`は複数入力の来歴を必要とするため、この束では扱わない。
- 権限: オーナーの既存実装・Git保持指示内。別repo、外部送信、配布、リリースは行わない。

## 開始時の同期

開始時に`AGENTS.md`全文、三点セット全文・マニフェスト、R2の入力機／関係／記憶／保存契約、現行実装と要求台帳を再確認した。作業前ツリーはclean、`main`とGitHub `origin/main`は上記SHA、worktreeは`/workspace/MINIDORA-BETA`の一つで、別writerプロセスは観測されなかった。ローカル回復branchとGitHub回復tagの開始値は`codex/backup-main=8c425bf`、`codex/backup-main-prev=e9a2755`。GitHubには同じ名前の従来branch refも存在したが、この束では変更しない。

三点セットと`MANIFEST.tsv`のハッシュは一致した。元ZIP・三文書は変更していない。

| 資料 | SHA-256 |
|---|---|
| ZIP | `1f65df9f67448406d0fe1e9d3b1e13981afa236f352223cbb61ce1541bc1371e` |
| 施工仕様書 | `2e6ce6e181555cebd66ff2f7a57bc01188981ecf116c6b2555e3f1b406b8edbc` |
| 工程表 | `d776f6a3ee57f0b8ba84a59461cc539e1888836d9de2ef08405830f7daa724ae` |
| Codex施工指示書 | `e348c05716a3bf2f7541be29096dc71c139f5c9d44efba49da93e02c1bbb37e8` |

Windows専用skillは対象環境がLinuxのため非該当。toolchainはRust/Cargo 1.99.1、`x86_64-unknown-linux-gnu`。

## 変更前の欠落と実装

B23は直接日本語コピュラAssertだけをFactの出典根拠に投影していた。既存の他動詞Intentは`use`等へ正規化された述語を持ち、原文には`使う`等の活用形があるため、既定の語句索引だけでは同じ役割の一意なbyte spanを構成できない。Coreが作るFactにはそのため`source_evidence`が付かなかった。

B24では入力機にparserの限定構文と同じ他動詞役割捕捉を追加し、subject、`は`/`が`、object、`を`、活用・否定を含むpredicate、実際に書かれた場合だけscopeを原UTF-8範囲へ対応させる。捕捉値を既に封印されたtyped propositionと照合し、役割map全体が同一の場合だけ`SealedSourceEvidence`を作る。重複／改変されたrole mapや別の文法は来歴へ昇格しない。

parser revisionを`declarative-ja-en-v78-japanese-transitive-assertion-source-ranges`へ更新した。parser versionは入力fingerprintの一部である。旧sealed bundleは旧版のまま保持し、同じ`(case_id,input_id,input_version)`を異なるparser revisionで再登録する場合は識別衝突として拒否され得るため、新しい入力versionを使う。Factのoptional fieldとDB schema versionは変更していない。Storeの現行actor ownership・sealed bundle照合経路をそのまま使う。

## 実観測

入力: `[scope=biology]ミナは毒を食べません。`

- v78 `MeaningIR`は6役割を原文の一意なUTF-8範囲へ写した。scope=`biology`、subject=`ミナ`、subject particle=`は`、object=`毒`、object particle=`を`、predicate=`食べません`。
- Coreは`USER_PREMISE` Factをcommitし、出典fingerprint、sealed-bundle fingerprint、原文SHA-256、semantic signature、parser versionと役割spanをFactへ保存した。
- Engineを閉じて再度開くとStoreは同じactorの保存済みsealed InputBundleとFact evidenceを照合し、同じ証拠付きFactを復元した。
- 通常の関係proofは`SUPPORTED`を返し、証拠付き同Factを返した。後続入力`[scope=biology]ミナは毒を食べませんか`のCore結果はそのFact IDに依存した。
- 重複predicate spanを加えたInputBundleはevidenceへ投影されなかった。
- parser単体観測は明示scope付き否定形、scope省略時に架空scope spanを作らない形、同一文字列をsubject/objectに使う形、`が`と丁寧形を確認した。

これは出所・表現範囲のlinkであり、`ミナ`や`毒`についての外部世界の真偽を確認していない。Engine API内の観測で、CLI/UI、出力機、表示・配送receiptは通していない。

## 検証

| コマンド | 結果 |
|---|---|
| `cargo test -p minidora-engine --locked` | PASS: 240 unit tests、12 integration tests、0 failures |
| focused Core route observer (`--lib ... -- --nocapture`) | PASS: save → reopen → proof → follow-up Fact dependency |
| `cargo fmt --all -- --check` | PASS |
| `cargo check --workspace --all-targets --locked` | PASS |
| `cargo clippy --workspace --all-targets --locked -- -D warnings` | PASS |
| `cargo build --workspace --locked` | PASS |
| `git diff --check` | PASS |

初回のtest compileは新規fixtureで`String`と`&str`が合わず停止し、修正後に上記全件を実行した。最初のformat checkは整形差を検出し、format後はPASS。初回Clippyは不要なborrowを検出し修正、warnings-denied再実行はPASS。失敗ログも保持している。

全workspace runtime / all-features test、CLI/UI/output delivery、Windows・macOS・別OS、実機8GB条件、独立QA・受入評価はNOT_RUN。`A04`/`A15`/`E08`および要求全体のacceptance statusは`UNVERIFIED`のまま。full acceptance、三機全域通電、全体系完成、リリースを宣言しない。

詳細なhashとroute JSONは[validation observation 07](../bb-input-machine-observation-07-v78-transitive-assert-source-validation.json)。

## 変更パス

- `rust/crates/minidora-engine/src/input.rs`
- `rust/crates/minidora-engine/src/core.rs`（検査用route observer）
- `rust/crates/minidora-engine/tests/input_machine_observation_01.rs`
- `rust/crates/minidora-engine/tests/input_machine_observation_02.rs`
- `rust/audit/IMPLEMENTATION_STATUS.md`
- `rust/audit/requirements.tsv`
- `rust/audit/bb-input-machine-observation-07-v78-transitive-assert-source-validation.json`
- 本`BUILD_BATCH_24.md`

## B24資産の5分類

分類対象はB24で変更・生成・使用した有限資産。変更一覧8 pathsをGit保持、隔離ログ15件を検査記録として保持。既存cache 3 directoryは今回の変更物ではなく再利用するactive資産とし、他は未分類なし。

| 分類 | 件数 | 内容 |
|---|---:|---|
| retained | 8 paths | 上記ソース・台帳・validation record・施工記録をGitへ保持 |
| archived | 15 files | `/workspace/.minidora-cloud/b24-japanese-transitive-source-evidence/`のformat/test/build/check/Clippy/routeログ。初回失敗と修正後再実行を両方保持 |
| active | 3 directories | `/workspace/.minidora-cloud/rust-target`、`/workspace/.cargo`、既存`rust/target`。共有/既存cacheとして再利用し、既存`rust/target`は変更しない |
| trash | 0 | trash移動なし、永久削除なし |
| pending | 0 | B24で生成・使用した資産の未分類なし |

テストの一時stateはfixture cleanupで除去された。三点セットは別の既存retained資産としてハッシュ一致を再確認し、変更していない。
