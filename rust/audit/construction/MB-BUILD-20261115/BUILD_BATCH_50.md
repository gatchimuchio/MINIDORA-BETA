# BUILD_BATCH_50 — 多段proof説明でCore合成規則を区別する

- **基線HEAD:** `3326add8c66f86d15ec584be86888f50eff16007`（B49）
- **対象:** 唯一Core K3→K4→K7、`CORE-G03`の限定した多段説明経路。
- **主語と目的:** ユーザーが型付き関係と説明目的を与えたとき、Coreが証明から説明内容を組み立て、採用して実描画まで通す。
- **範囲:** 原子的関係説明のunit構成、computation artifact版、B34回帰期待値、B50直接Core観測と監査記録。入力文法・HTTP/CLI・R4三点セット・依存関係・一般要求受入は変更しない。

## 観測した問題と施工

変更前の多段fixtureでは、Coreが二つの宣言済み親規則をそれぞれ通常の`Rule` unitとして説明したうえで、同じ親規則から内部生成した短縮規則も通常の保存規則unitとして全文表示していた。選択proofの全IDは保持されていたが、利用者向け説明では内部合成物の由来が親規則と区別されず、同じ推論関係を重複して説明していた。変更前の実出力は[`b50-prechange-observation.log`](b50-prechange-observation.log)へ保存した。

`relation_atomic_explanation_request`を更新した。

- ユーザーが宣言した親規則は従来どおり`Rule` unitとして一度ずつ表示する。
- `source_input=derived:proof-composition-v1`の規則は、Coreが親から合成した短縮規則だと明記する`Detail` unitへ移した。
- 短縮規則のDetail unitは親規則unitをtyped dependencyに持つ。証拠IDには短縮規則IDと両親規則IDを保持する。
- 直接fact、親規則、短縮規則、導出targetの全proof ID集合はcomposer report、ContentPlan、ResultRecordへ維持する。
- 変わったcontent request／意味単位の生成を区別するため、computation artifact kindをv2からv3へ更新した。旧v1構造は従来の読取分岐を保ち、v2/v3 content artifactは共通field decoderからartifact kind別に読み取る。B50でV3の後続追問は実行確認した。旧V2実artifactの再生は本batchでは実行していない。

## producer → consumer → 実結果

隔離Core storeへ二規則と二つの直接factを入力した。

1. `[scope=multi-hop-explanation-lab]X is ore`なら`X is refined`。
2. 同scopeの`X is refined`かつ`X is inspected`なら`X is ready`。
3. 対象`Nera is ready`とExplain目的だけを`Engine::compose_atomic_explanation`へ渡した。完成説明文・期待文は入力しない。

Coreは二つのdirect fact、二つの宣言rule、proof-compositionが作った短縮ruleを含む6件のproof IDを照合した。K4 requestにはClaim・Evidence・Rule・Core-generated Detail・Limitationが入り、両親Ruleの後に短縮規則Detailを置いた。Composerはその実requestを決定的に再生し、ContentPlanに採用された文面はCoreによる短縮規則の由来と親IDを明示した。K7は同じContentPlan bytesを`RENDERED`で返した。

同じcaseで後続`Why?`をEngineへ渡し、保存したv3 computation artifactのdecode・composition再生成・現在proof照合を経て`COMMIT / DERIVED / RENDERED`となり、先行result IDへの依存を保持した。

実行観測と最終trace: [`b50-focused-tests.log`](b50-focused-tests.log)。
変更前の初回testはDetail unitをまだ生成しない旧挙動へのassertionで一度failした。該当log: [`b50-initial-fixture-assertion-failure.log`](b50-initial-fixture-assertion-failure.log)。fixtureを実観測に合わせて二親Ruleの到達を確認した後、旧出力の重複表示を実装修正した。初回v3 reader compile時の所有権エラーも修正履歴として保持: [`b50-intermediate-build-failure.log`](b50-intermediate-build-failure.log)。

## focused verification

- `cargo test --locked -p minidora-engine --test core_atomic_explanation_34`: **PASS**（1 test、既存単段derived explanation回帰）。
- `cargo test --locked -p minidora-engine --test core_atomic_explanation_50 -- --nocapture`: **PASS**（1 test、多段proofから新しい説明計画・K7描画・v3 artifact後続消費）。
- `cargo check --locked -p minidora-engine --all-targets`: **PASS**（全engine targetの型検査。runtime suiteではない）。
- `cargo fmt --all -- --check`: **PASS**。
- `git diff --check`: **PASS**。
- 検査ログ: [`engine all-target`](b50-engine-all-target-check.log)、[`format`](b50-format-check.log)、[`staged diff`](b50-git-diff-check.log)。
- Toolchain: Rust/Cargo `1.99.1`、Linux x86_64。build target=`/workspace/.minidora-cloud/rust-target`。

## 判定と未実施

このfixtureは、初見の型付き命題と目的からCoreがProof-derived content planを作り、内部合成ruleを親ruleと区別して採用し、K7実描画と保存artifact後続消費まで到達する一経路を示す。これは多段説明の限定実装改善であり、一般的な内容妥当性・全rule graph・通常CLI/HTTP利用者経路・Core gateの全成功述語は示さない。`CORE-G03=PARTIAL`を維持し、ほかの`CORE-G01`〜`CORE-G07`は本batchで再判定しない。R4が求める全ゲートPASS前なので入力機施工へ進まない。R2-17、outside-world truth、full workspace runtime、Clippy、全93要求受入、独立QA、release/deploymentは**NOT_RUN**。`CAP-D05=UNVERIFIED`を維持し、release=`NOT_RELEASED`。

## 変更資産と閉包

| 区分 | 数 | 対象 |
|---|---:|---|
| keep | 13 | `core.rs`、B34 regression変更、B50 integration test、status、implementation choices、本記録、最終focused log、変更前観測log、initial fixture failure log、intermediate compile failure log、engine all-target check、format check、staged diff check logs |
| store | 0 | 外部保管なし |
| in-use | 4 | `/workspace/.minidora-cloud/rust-target`、`rust/target`、Cargo cache、Rust toolchain |
| trash | 0 | 削除・ゴミ箱移動なし |
| hold | 0 | 保留なし |
| **合計** | **17** | keep 13 + store 0 + in-use 4 + trash 0 + hold 0 |

各test用temporary stateはDropで閉じて削除する。対象ディレクトリの残留がないことを終了時に確認する。両target、cache、toolchainは継続利用資産として保持し、削除・統合しない。

編集前のHEAD `3326add8c66f86d15ec584be86888f50eff16007`をGit履歴上のrollback targetとして残す。新規backup refは作らない。ユーザーの「ちゃんとリポにも残しておけ」という既存指示に従い、変更は`main`へcommit/pushする。別branch/PR、release/deployment、R4三点セットの本文とarchiveの改変は行わない。
