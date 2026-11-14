# B29 — contextual correction source evidence

## 対象と工程

- 対象: `gatchimuchio/MINIDORA-BETA` Rust後継実装。
- 着工版: `main` / `079989ab5c28a423b47d01ab310d4c8d0e053101`（B28）。
- 目的: 既存の同一ケース直前結果を用いる日本語・英語の限定的な訂正経路で、原文上の役割とCoreが復元した対象Factとの関係を、両方の訂正Fact・保存・再開・後続consumerまで保つ。
- 入力: `[scope=biology]それは哺乳類ではなく鳥です。` と `[scope=biology]It is a bird, not a mammal.`。どちらも先に同じ範囲の旧前提を登録し、それに依存する確定質問結果を直前文脈とする。
- 実経路: 原文 → v82 sealed `CorrectPreviousPremise`/source roles → Coreが直前の確定結果IDと一意な直接 `USER_PREMISE` 依存を照合 → 1回のactor-memory CASで否定Factと置換Factを保存し、同じ `SealedSourceEvidence` を付与 → Engine再open時にStoreが不変sealed inputから再検証 → 両Factの `SUPPORTED` proof → 通常Core Askが置換Fact IDを消費。
- 期待観測: 代名詞は文字どおり保存し、解決した主語とは別に扱う。source byte rangesは原文に一致する。直前結果がない場合は `SUSPEND` し、source evidenceも訂正Factも作らない。
- 停止境界: 文脈未接続、直前結果の不一致、対象前提の不存在・曖昧さは既存のtyped stopを維持する。役割spanを再検証できない場合も `CORRECTION_SOURCE_EVIDENCE_UNAVAILABLE` でFact保存前に保留する。世界の真偽、一般的な照応理解、全体の要求受入を示す観測には広げない。

## 実装

- `input.rs`: correction source contractを直接訂正と文脈訂正で共有し、`/CorrectPreviousPremise/` の役割ルートを追加。日本語の代名詞・助詞・旧値・訂正標識・置換値・scope、および英語の代名詞・copula・冠詞・旧値・`not`・置換値・scopeを個別のUTF-8 byte rangesとして封印する。source rolesが文法再解析結果と完全一致する場合だけ投影する。
- `input.rs`: parser revisionを `declarative-ja-en-v82-contextual-correction-source-ranges` へ更新。以前のsealed inputは版付きのまま保持される。同一 `(case_id, input_id, input_version)` の再利用時にparser変更でidentity conflictとなる既存規則に従う。
- `core.rs`: 既存の `CorrectPreviousPremise` が実際の直前結果・同一範囲の提示Factを照合した後、sealed contextual source evidenceを既存 `apply_premise_correction` に渡す。役割spanが一意に検証できない場合は記憶を変更せず保留する。訂正対象の選択規則は変更していない。
- `relations.rs`: validated evidence familyとして `/CorrectPreviousPremise/` を許可。
- Storeの再検証機構、保存schema、依存crate、`Cargo.lock`は変更していない。

## 実観測と検証

新しいCore観測は日英2経路を通し、literal pronoun/source roles、実anchorと一致する `context_result_ref`、両訂正Factへの共有証拠、Engine再open後のStore再照合と二つの `SUPPORTED` proof、置換Factを使う後続質問を確認した。別ケースでは文脈なし訂正が `SUSPEND` し、Factを書かず証拠を投影しない。

| コマンド | 結果 |
|---|---|
| `cargo test -p minidora-engine --locked --lib contextual_premise_correction_source_roles -- --nocapture` | PASS: 1 test、0 failed（日英2実例と文脈欠落境界を含む） |
| `cargo test -p minidora-engine --locked` | PASS: 247 unit + 12 Cargo-launched integration cases = 259、0 failed、doc tests 0 |
| `cargo fmt --all -- --check` | PASS |
| `cargo check --workspace --all-targets --locked` | PASS |
| `cargo clippy --workspace --all-targets --locked -- -D warnings` | PASS |
| `cargo build --workspace --locked` | PASS |
| `git diff --check` | PASS |

環境はLinux `x86_64-unknown-linux-gnu`、`rustc/cargo 1.99.1`。最初のClippy実行ではtest-only route helperの引数数が警告になったため、同じ確認単位内でローカル `Scenario` 構造体にまとめ、再実行を通した。最終gateは全てexit 0。

全workspace all-features runtime、通常CLI/browser経路、UI/外部出力配送、他OS、独立QA、完全なA04/A15/E08と93要求全体の受入は `NOT_RUN` / `UNVERIFIED`。source roleは原文位置を示すだけで、利用者提示の前提の外界真偽は保証しない。releaseなし。

## 要件記録

`requirements.tsv` のA04/A15/E08へ経路と観測を追加した。各要求の `implementation_disposition=NEW` と `acceptance_status=UNVERIFIED` を保持した。観測内容は [B29 validation](../../bb-input-machine-observation-12-v82-contextual-premise-correction-source-validation.json) に記録した。

## 資産の5分類

| 分類 | 件数 | 内容 |
|---|---:|---|
| retained | 7 paths | `core.rs`、`input.rs`、`relations.rs`、`requirements.tsv`、`IMPLEMENTATION_STATUS.md`、B29 validation JSON、本記録 |
| archived | 12 files | focused route 2 logs、engine package 2 logs、format、workspace check 2 logs、Clippy pass/fail 2 logs、build、diff-check、toolchain/environment log。すべて `/workspace/.minidora-cloud/b29-contextual-correction-source-evidence/` に保存 |
| active | 3 directories | `/workspace/.cargo` 252M、`/workspace/.rustup` 617M、`/workspace/.minidora-cloud/rust-target` 5.9G の共有cache/toolchain/target |
| trash | 0 | 既存cache・原本・Git証拠の削除なし |
| pending | 0 | 今回生成・使用した記録を全分類 |

合計22項目。テスト生成SQLite状態は一時ディレクトリを削除しており、残存fixture DBなし。3点セットZIP・原本Markdown・凍結Python `src/` は変更していない。

## 変更パス

- `rust/crates/minidora-engine/src/input.rs`
- `rust/crates/minidora-engine/src/core.rs`
- `rust/crates/minidora-engine/src/relations.rs`
- `rust/audit/requirements.tsv`
- `rust/audit/IMPLEMENTATION_STATUS.md`
- `rust/audit/bb-input-machine-observation-12-v82-contextual-premise-correction-source-validation.json`
- 本 `BUILD_BATCH_29.md`
