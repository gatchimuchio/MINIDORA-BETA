# BUILD_BATCH_09 — CSV説明dispatchとworkspace回帰の修復

## 工事識別

- 対象リポジトリ: `gatchimuchio/MINIDORA-BETA`
- 作業ブランチ: `main`
- 着工時 HEAD: `05aa22f529fd69d1ee54134e43852acc92585207`
- 施工資料: `MB-BUILD-20261114` の三文書、利用者添付ZIP、`MANIFEST.tsv`。着工時にmanifest記載の4件すべてでSHA-256一致を再確認した。資料は変更していない。
- 作業種別: B08終了時に基準revisionでも確認した7件のworkspace回帰失敗を、CSV説明の経路誤判定と既存テスト期待値の古さに分けて修復する。

## ⓪–④ 着工確認と単位定義

- ⓪ 権限と設計基準: 利用者の「続行」と採用済み三点セットに基づく施工を継続。適用`AGENTS.md`、三点セット、R2のCore責任境界を確認した。workspace失敗記録は観測証拠であり、別のユーザー命令として扱っていない。
- ① リポジトリ: `MINIDORA-BETA` の `main`。着工時`HEAD`と`origin/main`はともに`05aa22f529fd69d1ee54134e43852acc92585207`、worktreeはclean。
- ② 作業競合と復旧点: worktreeは一つ、同時writerなし。`/workspace`に約53 GiBの空き。着工時の`codex/backup-main`は`05aa22f529fd69d1ee54134e43852acc92585207`、`codex/backup-main-prev`は`c820916a78f3564f2fec518cb2b2f68787426204`。
- ③ 一単位: committed `ExplainPrevious` がCSV statistics結果を受ける場合、Coreが保存済みsealed inputのintentを基に正しいCSV consumerへ渡す。JSON-table operation consumerは、形が似たCSV reportをJSONとして再計算しない。既存relation説明テストはscope付きの実出力を検査する。
- ④ 境界: parser、schema、version、relation説明の製品出力、保証を変更しない。JSON-table各operationの再計算・recoveryを広げない。全要求受入やrelease完了を主張しない。

## 着工時の失敗と修復

B08のworkspace testではCLI unit 10件が通り、`normal_route`は134/141件、7件が失敗した。7件はB08のCore差分を着工時baselineへ一時復帰しても同じfailure signatureで再現した。この前状態は[workspace failure比較](../../bb-workspace-test-full-20261114-failure-baseline-comparison.json)に残してある。

CSV statisticsの同一ケース説明は、専用consumerでsource/reportを再検証する前に、CoreからJSON-table operation consumerも呼ばれていた。JSON consumerは結果reportのfield shapeからoperationを推定し、sealed input intentがCSV statisticsでも`JSON_TABLE_CONTEXT_INVALID`を返していた。`document.rs`でJSON-table shapeとsealed input bundleを確認した後、intentが6種類の対応JSON-table operationのいずれかでなければ`None`を返すようにした。これによりCSV resultは既存のCSV statistics consumerへ進み、JSON-table operation固有のrecomputeと誤ったintent時の停止は保つ。

別の6 failing test casesでは、期待文字列が`適用規則:`のままだった。現行formatterは`適用規則（範囲: ...）:`を出す。`normal_route.rs`の9個のscope-less assertionを、`biology`または`default`を明示する期待値へ更新した。先行assertionを直したことで露出した`natural_why_question_explains_only_current_relation_proofs`の反証文も、既存Coreの「問われた関係…は支持されず」という実出力に合わせて更新し、反証前提とdependencyの確認は維持した。Coreの製品文言は変えていない。

## 確認結果

- 以前失敗した7件を個別に再実行 — PASS、各1件。
- `cargo fmt --all -- --check` — PASS。
- `cargo check --workspace --all-targets --locked` — PASS。
- `cargo clippy --workspace --all-targets --locked -- -D warnings` — PASS。
- `cargo build --workspace --locked` — PASS。
- `cargo test --workspace --locked` — PASS。CLI unit 10、`normal_route` 141、domain 4、engine 230。domain/engine documentation testsは0件で正常終了。
- `git diff --check` — PASS。
- 実行版、source hash、focused test結果、workspace test出力hashは[workspace regression repair証跡](../../bb-workspace-runtime-regression-repair-20261114-validation.json)に記録した。

## 保証境界と受入状態

この単位はCSV statisticsの既存ExplainPrevious consumerへの誤dispatchを塞ぎ、既存のworkspace回帰を修復した範囲に限る。CSV機能全般、JSON-table機能全般、自然言語の一般能力、E08や他要件の全受入を証明しない。E08の受入は`UNVERIFIED`。

## GitHub保持

`main`へ通常fast-forwardで保存し、`codex/backup-main`をこの単位の確定revision、`codex/backup-main-prev`を着工時HEADへ更新する。各local/remote branch/tagは実測旧SHAを指定したcompare-and-swapで同期する。PR、別branch、worktreeは作らない。

## 資産区分

- 保持: 利用者原本4件（ZIPと施工三文書）、manifest照合済み・不変。
- 保存継続: 着工前のB08 workspace失敗比較1件。書き換えず保持。
- 稼働: 本単位の6ファイル（`document.rs`、`normal_route.rs`、workspace修復証跡、実装状況、要件追跡、本報告）。
- リポジトリ資産の廃棄: 0件。B09の一時テストログ8件は証跡化後、`/home/agent/.local/share/Trash`へ移動した。
- 保留: E08および全要件の受入。E08は`UNVERIFIED`。

本単位の参照資産は11件（原本4 + 着工前失敗比較1 + 稼働6）。先行Batchのコード・報告・原証跡は不変。
