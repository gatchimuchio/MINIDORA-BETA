# BUILD_BATCH_21 — NFKC跨文字正規化と原文span保持

## 施工対象

- 親工程: 三点セット第六命令「中核の必要に従い入力機を構築」。
- 基点: B20を含む `2bad31cb7f9c70fdf2f4702a087afc2b8f86ce48`。
- 対象要求: A04/A15/E08のうち、入力解釈roleと原文出典spanの対応。
- 実行環境: Linux x86_64、Rust/Cargo 1.99.1、ローカルSQLite一時状態。

## 発見した欠陥と原因

v75の汎用role indexは原文をUnicode scalarごとにNFKC化して連結していた。NFKCは文字境界を跨いで合成するため、半角カナ `ｶﾞ` は候補側で `ガ` になる一方、原文側が `カ` と濁点のまま残り、`/Assert/proposition/subject` のspanが失われた。Hangul Jamoの合成可能な連続にも同じ問題がある。通常の `Engine::execute` で保存された意図にはsubjectがあるのに、source-role mapには範囲がないことを再現した。

## 是正

既存の `unicode-normalization` を使い、NFKDで次scalarが非starterになる位置と、CCC 0同士の正規化済み末尾から合成できない位置を区切りとして正規化segmentを作る。各segmentを全体NFKC化し、生成された正規化UTF-8 byteごとに元segmentの完全なbyte範囲を対応付ける。文書schema・DB schema・依存は変更しない。source-role出力が変わるためparser revisionを `declarative-ja-en-v76-nfkc-source-span-boundaries` に更新した。過去のv75 validation記録は履歴として変更しない。

## 観測

- 単体検査は `ｶﾞ→ガ`、Hangul Jamo `각→각`、NFDの `Cafe\u{301}→Café`、結合markの並べ替え、compatibility ligature `ﬃ→ffi` を全体NFKC結果と比較し、UTF-8 byteから原文範囲への写像を確認する。
- 通常 `Engine::execute` は `ｶﾞはsensor` と `가はsensor` を各々USER_PREMISEとしてCOMMITし、subject roleが原文の先頭6byteを指すことを確認する。
- 同一Engine storeを閉じて再openし、2件の保存ResultRecordとCaseRevisionがそのまま再生されることを確認する。ネットワーク・外部副作用はない。
- 変更後の `cargo test -p minidora-engine --locked` は236 unit tests、construction統合5、meaning composition統合2、input machine統合4の計247件PASS。doc testsは0件。
- `cargo fmt --all -- --check`、`cargo check --workspace --all-targets --locked`、warnings-denied Clippy、`cargo build --workspace --locked`、`git diff --check` はPASS。
- v75での再現FAILとv76の試験ログは `/workspace/.minidora-cloud/b21-nfkc-source-span/` に保存し、SHA-256は [B21 validation record](../../bb-input-machine-observation-04-v76-normalization-validation.json) に記録する。

## 適用範囲と残る境界

これはNFKC境界を跨ぐsource-span欠陥の修正であり、任意のUnicode意味同値・role意味帰属・一般的な日本語解析を保証しない。全workspaceのall-features runtime suite、全OS、GPQA、要求母集団全体のQC/QAはこの単位では実施していない。A04/A15/E08と全体受入は `UNVERIFIED`、releaseは `NOT_RELEASED` のままとする。

Validation metadata: [B21 v76 normalization and source-span record](../../bb-input-machine-observation-04-v76-normalization-validation.json)。
