# BUILD_BATCH_22 — 同一NFKC値の日本語assertion roleを原文位置で分離

## 施工対象

- 親工程: 三点セット第六命令「中核の必要に従い入力機を構築」。
- 基点: B21を含む `db7a4f1f8cbd034f495bbed2d6f98cfe808e8d01`。
- 対象要求: A04/A15/E08のうち、直接日本語copular assertionのIntent roleと原文UTF-8 byte範囲の対応。
- producer → consumer: `RequestEnvelope`原文 → `input::interpret`の`Intent::Assert` → `input::source_role_spans`の`MeaningIR.source_roles` → sealed `InputBundle`を伴う通常`Engine::execute`と保存`CaseRevision`。
- 実行環境: Linux x86_64、Rust/Cargo 1.99.1、ローカルSQLite一時state。
- 外部通信・外部作用: なし。

## 欠陥と再現

v76の汎用source-role mapperはIntent文字列leafをNFKC化し、同一patternに一致した全source occurrenceをそのpatternの全roleへ配布する。`[scope=ｶﾞ]ｶﾞはｶﾞではない`ではscope・subject・objectがいずれも同じNFKC値`ガ`となり、修正前の通常Engine routeは各roleへ3個のspanを割り当てた。期待されたscope位置だけを選ぶ回帰テストはv76でFAILした。原文byte・各captureの位置情報をIntentのString leafだけから復元する設計が原因であり、v76のNFKC segmentation自体はこのケースの欠陥原因ではない。

## 是正

既存の限定日本語copular grammarと同じ構造captureから、直接`Intent::Assert`のscope、主語、助詞、目的語、否定語尾を原文の半開UTF-8範囲へ写す。Intentに無い合成default scopeはsource roleに出さない。構造captureが意図と一致しない入力は汎用mapperへ戻す。他Intentと英語・複文・transitive等のsource-role競合はこの単位では変えていない。

source-role出力が変わるためparser revisionを `declarative-ja-en-v77-japanese-assertion-source-ranges` に更新。DB schemaと依存crateは変更しない。旧入力identityのcross-version migration/replayは未実施。

## 観測結果

- 通常`Engine::execute`の`[scope=ｶﾞ]ｶﾞはｶﾞではない`は`COMMIT / USER_PREMISE`。保存された意図はscope・subject・objectをいずれも`ｶﾞ`、否定をtrueとして保持し、role spansはscope `7..13`、subject `14..20`、predicate `20..23`、object `23..29`、negation `29..41`となった。
- 通常`Engine::execute`の`defaultはdefaultではない`もcommitし、合成されたscope=`default`にはsource spanを作らなかった。
- Engine close/open後、両入力の保存`ResultRecord`と`CaseRevision`が完全一致で再生された。
- v76 baseline routeはscope roleの3候補を報告してFAIL。v77の専用route testは両ケースでPASS。
- `cargo test -p minidora-engine --locked`: unit 236、construction integrations 5、meaning-composition integrations 2、input-machine integrations 5、計248 PASS、doc tests 0。
- `cargo fmt --all -- --check`、`cargo check --workspace --all-targets --locked`、warnings-denied Clippy、`cargo build --workspace --locked`、`git diff --check`: PASS。
- `cargo test --workspace --all-features --locked`: B22ではNOT_RUN。

## 証拠境界・残件

`MeaningIR.source_roles`はsealed input bundleと`CaseRevision`へ保存されるが、現在のCore relation proof codeはtyped `Intent`を消費し、個々のsource-role spanをproof dependencyとして使わない。B21の検証記録にある「consumer」はbundleが通常Engine routeを通って保存される意味であり、role spanがCoreの推論判定へ入力される意味ではない。B21の保存/replay観測は維持するが、role spanの推論利用を実証したものとして読まない。

本単位は単純な日本語`Assert` grammarの欄別byte位置と保存/replayだけを確認した。A04/A15/E08および全要求受入は`UNVERIFIED`、全workspace runtime suite、全OS、全入力family、GPQA、公開・releaseは`NOT_RUN / NOT_RELEASED`のままとする。一般的な語句の意味帰属や、source-role spansをCore proof evidenceへ接続する工程は残る。

SHA-256付き試験ログ: `/workspace/.minidora-cloud/b22-source-role-disambiguation/`。機械可読記録: [B22 v77 validation](../../bb-input-machine-observation-05-v77-assertion-role-validation.json)。
