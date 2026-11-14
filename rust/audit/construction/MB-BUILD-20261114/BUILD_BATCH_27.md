# B27 — Japanese premise-correction source evidence

## 対象と範囲

- 入力: `[scope=biology]訂正: ココは哺乳類ではなく鳥です。`。同一actor/caseに先行して `[scope=biology]ココは哺乳類です。` を保存した。
- 目的: 入力機が訂正対象と置換値の原文位置を封印し、Coreの訂正で生まれる両Fact、Storeの再検査、proof、後続質問まで証拠リンクを保つ。
- 経路: `RequestEnvelope` → v80 `InputBundle` → `CorrectPremise`と6個のrole span → Coreが既存の同scope直接前提を選ぶ → 一回のmemory CASで旧関係の否定Factと新関係Factを保存 →両Factに同じ`SealedSourceEvidence` → Store再open検査 → 2つの`SUPPORTED` relation proof → 通常Core Askが置換Fact IDに依存。
- 停止境界: 既存の直接前提がない訂正は従来どおり停止し、記憶を変更しない。証拠spanは出所情報であって、外部世界の真偽ではない。

## 開始時同期

B26のcommit `b1a635b53b179e22ea3f068c609541cf47125876` を`origin/main`からfetchして確認し、`main`のworktreeはcleanだった。対象は引き続き工程表の第五工程「中核の必要条件から入力機を施工」。3点セットとR2の意味設計、DB schema、依存は変更しない。

## 実装

`input.rs`は、支持済みの直接日本語copular訂正について、共通subject、`は`/`が`、旧object、`ではなく`、置換object、明示scopeを元入力のUTF-8 byte rangeへ対応付ける。source spanがtyped `CorrectPremise`と一致し、訂正grammar全体を同じIntentとして再照合できる場合に限り投影する。parser revisionを`declarative-ja-en-v80-correction-source-ranges`へ更新した。

Coreはこの訂正証拠を旧関係の否定Factと置換Factの両方へ渡す。Relation verifierは`/CorrectPremise/` role familyを既存のoptional `SealedSourceEvidence`として許可し、Storeは再open後に保持された封印入力から証拠を再計算して照合する。2 Factは従来どおり一回のmemory CASで保存する。schema revision、依存、`Cargo.lock`の変更はない。

保存された範囲:

| 役割 | 原文 | UTF-8 byte range |
|---|---|---:|
| subject | `ココ` | 23–29 |
| subject particle | `は` | 29–32 |
| superseded object | `哺乳類` | 32–41 |
| correction marker | `ではなく` | 41–53 |
| replacement object | `鳥` | 53–56 |
| explicit scope | `biology` | 7–14 |

## 実観測と検証

seedされた前提を訂正した結果、旧前提を無効化し、否定された旧関係と置換関係を同じ訂正入力IDから記録した。両Factは同一の`raw_sha256`、input fingerprint、bundle fingerprint、semantic signature、role mapを持つ。Engineをclose/reopenした後にStoreがそれらを再検査し、両命題のproofが`SUPPORTED`を返す。通常の `[scope=biology]ココは鳥ですか` は置換Fact IDをdependencyに含む。

| コマンド | 結果 |
|---|---|
| `cargo test -p minidora-engine --locked --lib japanese_premise_correction -- --nocapture` | PASS: parser span検証とCore/Store reopen route 2件 |
| `cargo test -p minidora-engine --locked` | PASS: 244 unit + 12 integration = 256 passed、0 failed; doc tests 0 |
| `cargo fmt --all -- --check` | PASS |
| `cargo check --workspace --all-targets --locked` | PASS |
| `cargo clippy --workspace --all-targets --locked -- -D warnings` | PASS |
| `cargo build --workspace --locked` | PASS |
| `git diff --check` | PASS |

B25/B26の保存済みtest logをCargoが起動したtest harnessごとに再計数し、誤った`26 integration`表記を訂正した。正しい結果はB25 `241 + 12 = 253`、B26 `242 + 12 = 254`。14個のsubprocess worker resultをintegration testとして重複計上していたためであり、いずれのtest outcomeも変わらない。B27の最初の全件runはv79固定値を期待する既存testを発見したため、parser constant参照へ直して再実行した。warnings-denied Clippyで出た関数引数数もtuple境界へまとめて解消した。全成功・失敗ログを保持する。

## 保証の範囲

対象は一つの直接・明示scope付きcopular日本語訂正である。English correctionは現在のIntent解析には対応するが、B27でsource evidenceを投影するrouteには含めていない。任意の訂正文、全日本語の主語帰属、全A04/A15/E08、外部世界の真偽は示さない。全workspace all-features runtime、CLI/UI/output、他OS、独立QAは未実施。A04/A15/E08と要求93件全体は`UNVERIFIED`。releaseなし。

## 資産の5分類

| 分類 | 件数 | 内容 |
|---|---:|---|
| retained | 12 paths | 5 code/test paths、B25/B26記録の件数訂正、status、requirements、B27 validation、B27施工記録 |
| archived | 21 files | focused/full test、format、workspace check、Clippy、build、diff-checkの完全log。失敗した初回試行も含む |
| active | 3 directories | `/workspace/.cargo`、`/workspace/.rustup`、`/workspace/.minidora-cloud/rust-target`を既存cache/toolchainとして使用 |
| trash | 0 | trash移動なし、永久削除なし |
| pending | 0 | B27の生成・使用資産に未分類なし |

テストが作成する一時SQLite stateはfixture cleanupで除去した。3点セットarchiveとmanifest内の3文書は変更していない。

## 変更パス

- `rust/crates/minidora-engine/src/input.rs`
- `rust/crates/minidora-engine/src/core.rs`
- `rust/crates/minidora-engine/src/relations.rs`
- `rust/crates/minidora-engine/tests/input_machine_observation_01.rs`
- `rust/crates/minidora-engine/tests/input_machine_observation_02.rs`
- `rust/audit/requirements.tsv`
- `rust/audit/IMPLEMENTATION_STATUS.md`
- `rust/audit/bb-input-machine-observation-10-v80-premise-correction-source-validation.json`
- `rust/audit/construction/MB-BUILD-20261114/BUILD_BATCH_25.md`（engine test count correction）
- `rust/audit/construction/MB-BUILD-20261114/BUILD_BATCH_26.md`（engine test count correction）
- `rust/audit/bb-input-machine-observation-09-v79-contextual-halfwidth-negative-assert-validation.json`（engine test count correction）
- 本`BUILD_BATCH_27.md`
