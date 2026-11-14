# BUILD_BATCH_19 — 全Intent文字列leafの原文span索引

## 施工対象

- 親工程: 三点セット第六命令「中核の必要に従い入力機を構築」。
- 基点: B18 `81a6f3c23c51a1f30d26bfab5c6dac10e03d9632`。
- 変更版: parser `declarative-ja-en-v75-all-intent-source-spans`。
- 目的: B18のIntentごとの手作業候補表をやめ、封印されたIntentのserde構造から全String leafを再帰的に拾い、意味役割パスから元入力UTF-8範囲へ索引する。

## 実装

`input.rs`はserdeの`kind`タグから役割rootを作り、配列indexとRFC 6901 escape済みfield名を連結して`/Variant/field/0`形式のroleを作る。文書本文、検索query/URL、統計値と単位、確率値、数式、規則命題、CSV/JSON操作など、Intent内の文字列leafを同じ経路で候補化する。`Unknown`全体と`reason`は除外し、パーサが作った説明文をユーザー原文の出所として扱わない。

候補文字列と元入力の双方をNFKCで比較するが、spanのbyte範囲と`text`は変更前の原文から返す。Aho-Corasickでまとめて走査し、ASCII単語・数値の境界を見て部分語や`1`と`10`の取り違えを減らす。既存の日本語predicate粒子fallback（subjectとobjectの間の`は`/`が`）と否定marker aliasは維持した。各roleのspanは最大32件。

B18と同じ`MeaningIR.source_roles`を使うためschema migrationはなく、空map省略と旧JSON互換契約は変えない。parser revisionが変わるので封印fingerprintも新版で計算される。role rootをIntent variant名で名前空間化した。

## 確認範囲

- `cargo fmt --all`を実行。整形のみで、コンパイル確認ではない。
- `git diff --check`はPASS。
- 今回はcargo check、cargo test、Clippy、build、通常入口のruntime routeを実行していない。既存テストのparser-version期待値だけv75へ更新し、新規テストは追加していない。
- したがってv75の型検査、実行時の各Intent source map、再open replayは未検証。requirements A04/A15/E08および全体受入は`UNVERIFIED`のまま。

## 残る境界

文字列一致は意味の正しさや一意な出現元を証明しない。同じ文字列が複数箇所にあれば一致候補を複数返し、32件で打ち切る。日本語predicate粒子fallbackも限定的で、一般言語の主体復元や引用・時刻・許可の完全な帰属を証明しない。次はv75を通常のEngine経路で観測し、代表的Intentと旧データreplayを実行してから、必要な修正を行う。専門モジュールは追加しない。

Validation metadata: [B19 source span index record](../../bb-input-machine-observation-03-v75-validation.json)。
