# B28 — English correction source evidence

## 対象と範囲

- 入力: `[scope=biology]Correction: Coco is a bird, not a mammal.`。同一actor/caseに `[scope=biology]Coco is a mammal.` を先に保存した。
- 目的: 既存の英語訂正文法も原文上の意味役割とFact/proofの証拠リンクを維持する。
- 経路: `RequestEnvelope` → v81 `InputBundle` → `CorrectPremise`とsource spans → Coreが同scope旧前提を選択 → 1回のmemory CASで否定旧Factと置換Factを保存 → 両Factに同一の封印source evidence → Engine reopen時Storeが再照合 → 二つの`SUPPORTED` proof → 通常Core Askが置換Fact IDを消費。
- 入力機は訂正対象の採択判断を変更しない。訂正対象がない場合は既存規則どおり停止し、記憶を変更しない。

## 実装

B27で追加した`/CorrectPremise/`証拠familyへ、既に解析可能だった英語の限定形を接続した。`subject`、`is`/`are`、`not`、旧object、置換object、明示scopeを個別に記録する。`a`/`an`/`the`はoptional article roleとして名詞surfaceと分ける。元文のbyte rangesを対象とし、Evidenceを投影する前にtyped `CorrectPremise`全体を再解析結果と照合する。parser revisionを`declarative-ja-en-v81-english-correction-source-ranges`へ更新した。

訂正によりCoreが作る否定Factと置換Factの両方へ同じsource evidenceを渡す。StoreはB27と同じくreopen時にimmutable bundleから再計算して検証する。DB schema、依存、`Cargo.lock`は変更していない。

| 役割 | 原文 | UTF-8 byte range |
|---|---|---:|
| subject | `Coco` | 27–31 |
| copula | `is` | 32–34 |
| replacement article | `a` | 35–36 |
| replacement object | `bird` | 37–41 |
| correction marker | `not` | 43–46 |
| superseded article | `a` | 47–48 |
| superseded object | `mammal` | 49–55 |
| explicit scope | `biology` | 7–14 |

## 実観測と検証

先行Fact `Coco is a mammal`を訂正し、同じ訂正文のfingerprints・raw hash・semantic signature・全role spansを否定旧Factと置換Factへ保持した。Engine reopen後にStoreが両Factの証拠を照合し、両命題のproofが`SUPPORTED`を返す。`[scope=biology]Coco is a bird?`の後続Core Askは置換Fact IDをdependencyに含む。

| コマンド | 結果 |
|---|---|
| `cargo test -p minidora-engine --locked --lib english_premise_correction -- --nocapture` | PASS: parser span検証とCore/Store reopen route 2件 |
| `cargo test -p minidora-engine --locked` | PASS: 246 unit + 12 integration = 258 passed、0 failed; doc tests 0 |
| `cargo fmt --all -- --check` | PASS |
| `cargo check --workspace --all-targets --locked` | PASS |
| `cargo clippy --workspace --all-targets --locked -- -D warnings` | PASS |
| `cargo build --workspace --locked` | PASS |
| `git diff --check` | PASS |

対象は`is/are ... , not ...`を受ける直接・明示scope付きcopular訂正の一例と、そこに現れた二つの`a`冠詞である。すべての英語の節構造・照応・修辞的訂正、世界の真偽、要求93件の受入は示さない。全workspace all-features runtime、CLI/UI/output、他OS、独立QAは未実施。A04/A15/E08と要求93件全体は`UNVERIFIED`。releaseなし。

## 資産の5分類

| 分類 | 件数 | 内容 |
|---|---:|---|
| retained | 6 paths | input/Core実装・test、requirements、status、B28 validation、B28施工記録 |
| archived | 8 files | format、focused/full engine、workspace check、Clippy、build、diff-checkの完全log |
| active | 3 directories | `/workspace/.cargo`、`/workspace/.rustup`、`/workspace/.minidora-cloud/rust-target`を共有cache/toolchainとして使用 |
| trash | 0 | trash移動なし、永久削除なし |
| pending | 0 | B28で生成・使用した資産の未分類なし |

合計17項目。

テストが作成するSQLite stateはfixture cleanupで除去した。3点セットarchiveと3文書は変更していない。

## 変更パス

- `rust/crates/minidora-engine/src/input.rs`
- `rust/crates/minidora-engine/src/core.rs`
- `rust/audit/requirements.tsv`
- `rust/audit/IMPLEMENTATION_STATUS.md`
- `rust/audit/bb-input-machine-observation-11-v81-english-premise-correction-source-validation.json`
- 本`BUILD_BATCH_28.md`
