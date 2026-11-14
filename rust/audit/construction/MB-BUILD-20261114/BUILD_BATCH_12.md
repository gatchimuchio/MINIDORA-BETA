# BUILD_BATCH_12 — 中核の直接施工確認三

## 工事境界

- 対象: `gatchimuchio/MINIDORA-BETA` Rust Core、三点セット施工指示の「中核確認三」
- 目的: 初見の説明・比較・短文依頼について、型付き意味役割・根拠ラベル・出力条件を受け取り、中核の非ニューラル演算で実際の文列を組み立てる。
- 入力: 工事用の型付き `Claim` / `Evidence` / `Rule`、比較対象・差異、短文要素、依存ID、prior sequence、文体、形式、予算。
- 実経路: `Engine::compose_typed_meaning` → typed request validation → per-request `SequenceModel` fit → purpose/role/dependency eligibility → finite admissible-token trie → support-aware exact rational conditional distribution → deterministic composition report.
- 許可: 利用者が採用した三点セットに従う本repo内のCore実装、観測、試験、Git保存。
- 作用境界: read-only construction API。自然文解析、Store書込、`ContentPlan`採用、Renderer呼出し、外部モデル・外部通信・副作用を実行しない。
- 停止条件: 意味役割不足、ID/依存の不整合、文列順序の矛盾、空の候補支持、資源・出力予算超過はtyped errorで停止する。

## 実装と原因修正

既存の `realize_supported` はCore作成済みの全文候補を順位づけするだけで、型付き意味状態から文を構成しなかった。今回、`Engine::compose_typed_meaning` を追加した。三目的（explain / compare / short composition）、二文体、段落・箇条書き形式、意味役割、依存関係、最大16要素、prior sequenceを受け付ける。入力単位は終端記号を持つ一文として検査する。各単位の文面は完全保持し、全入力単位を一度ずつ出力する。Coreは役割順・依存順と一文/境界の限定grammarをhard constraintとして使い、合法な接続句と同順位単位だけを有限候補として選ぶ。

初回実装の観測では、通常のn-gram backoffが履歴には遷移を持つが、許可候補には一つも遷移を持たない文脈に留まり、候補確率を一様化した。原因を修正し、候補支持に実測遷移がある最長文脈を選び直し、許可済み次token上でexact rational smoothingを正規化する `exact-rational-support-backoff-char-trie-v2` を追加した。出力ごとに意味role・候補connector・確率・context hash・model/corpus revision・変換parameter・objective・decoder・tie policyを記録する。

モデルは入力意味単位と版付きrole×style接続句語彙から要求ごとに一時fitされ、次の要求へ学習状態を持ち越さない。記載された出所IDと意味文は独立検証しないため、保証は `Guarantee::Unknown`、`factual_status=TYPED_INPUT_UNVERIFIED` とする。一般言語の文章生成、学習、自然文入力経路、`ContentPlan` adoptionとして扱わない。

## 新規意味入力の観測

| 目的・形式 | Core出力の要約 | 観測 |
|---|---|---|
| 説明・neutral paragraph | 降水量の主張 → 観測票の根拠 → 適用規則 → 対象記録の制限 | EvidenceとRuleは同順位候補からモデルがEvidenceを先に選択。全4単位と出所IDを保持。 |
| 説明・concise paragraph | 同じ4意味単位を、規則先行の順序で構成 | style/context hashが異なり、合法候補と選択順が変化。 |
| 比較・paragraph | Aの測定値 → Bの測定値 → 差の意味 | 入力配列順でなくSubjectA/SubjectB/Difference roleから構成。 |
| 短文・bullets | 申請書の氏名・連絡先・日付欄と提出前確認を箇条書き化 | 明示prior sequenceを先頭に置き、同roleの独立要素二つは残る候補からCoreが選択。 |

比較入力の測定値を3グラムから5グラムに変更した追加観測で、出力文、context state hash、model revisionが変更した。同一の比較requestを再実行したreportは完全一致した。隔離Storeの対象caseは実行前後とも存在せず、永続状態の変更は無い。生request/Resultを含むJSON記録は [B12 direct-Core validation](../../bb-core-construction-observation-03-v1-validation.json) に保存した。

## 検証と保証範囲

- `cargo test -p minidora-engine --locked`: 232 unit tests、B11 integration 1、B12 integration 2 passed。新規説明・比較・短文、文体差、順序・依存、同一再実行、内容変更、欠落role停止、出力budget停止、Store不変を確認。別の言語unit testは高次文脈に許可候補が無い場合のsupport backoffを確認し、確率`3/5`・`2/5`の正規化を照合。
- `cargo fmt --all -- --check`、`cargo check --workspace --all-targets --locked`、`cargo clippy --workspace --all-targets --locked -- -D warnings`、`cargo build --workspace --locked`、`git diff --check`: 全てPASS。
- `cargo test --workspace --locked`: NOT_RUN。B12の証拠範囲は直接Core APIとengine suiteに限定。
- 残る実行確認はB12完了時点で追記する。`A01`、`A03`、`R2-17`のacceptanceは `UNVERIFIED` のまま。
- 自然文の説明・比較・短文入力、外部証拠の検証、multi-request学習、完全な一般文章生成、通常 `Engine::execute` / CLI / browser経路への接続は未実装または未観測。
- 同一repoの基線の三点セット4 assetは変更しない。releaseなし。

## 次工程

三点セットの「中核確認四」へ進む。実際に保存された計算・関係結果を次の要求で参照し、変更を新しいversionへ保存するproducer→consumer経路を確認する。
