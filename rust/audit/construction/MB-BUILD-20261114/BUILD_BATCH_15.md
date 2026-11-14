# BUILD_BATCH_15 — 中核の直接施工確認六

## 工事境界

- 対象: `gatchimuchio/MINIDORA-BETA` Rust Core、三点セット施工指示の「中核確認六」。
- 目的: 権限不足、有限予算、取消、不明な作用状態、未達成の必須成果が `COMMIT`/成功扱いにならないことを実行観測する。
- 実経路: 一時SQLite storeを使った通常 `Engine::execute`。入力は通常parserでsealed。
- 作用境界: ネットワーク・外部モデル・実ファイル書込なし。Unknown effectのケースは `Store` action ledgerを `PROPOSED → AUTHORIZED → PREPARED → ATTEMPTED` と進め、storeの読取が `UNKNOWN` と観測する状態fixture。物理書込を試みたとは扱わない。

## 実装と観測

1. zero deadlineの計算要求は `RESOURCE_LIMIT / SUSPEND`、内容計画・描画なし。
2. max_actions=2で必須グラフを完了できない計算要求は `ACTION_BUDGET_EXHAUSTED / SUSPEND`、内容計画・描画なし。
3. 計算・描画に成功しても、要求pathへの書込権限が無ければ `WRITE_PERMISSION_MISSING / SUSPEND`。required graphの `CONFIRMED_FILE` は未達、receipt/actionは空、実ファイルは作られず、resultは `COMMIT` にならない。
4. `ATTEMPTED`でreceipt未確認のactionをStoreが`UNKNOWN`として返すfixtureを作成。別の新規計算要求は`EXTERNAL_EFFECT_UNKNOWN / SUSPEND`となり、元の確定結果は変更されない。
5. その同じcaseで利用者の「キャンセル」を実行すると`CANCEL`を記録。不明な作用のreceipt観測を取消結果へ含め、actionは`UNKNOWN`のまま保持。出力先ファイルには触れない。
6. 作用がない新規caseの取消も `CANCEL` で終端し、内容計画は作らない。

各sealed原入力、parser/model/result/render hash、予算・permission状態、revision、ledger、停止理由を [B15 direct-Core validation](../../bb-core-construction-observation-06-v1-validation.json) に保存する。元の三点セットasset hashも検証記録に含む。

## 検証と保証範囲

- `cargo test -p minidora-engine --test core_construction_observation_06 --locked -- --nocapture`: PASS。専用観測1件。
- `cargo test -p minidora-engine --locked`: PASS。232 unit tests、B11統合観測1件、B12統合観測2件、B13/B14/B15統合観測各1件、doc tests 0件。
- `cargo fmt --all -- --check`、`cargo check --workspace --all-targets --locked`、`cargo clippy --workspace --all-targets --locked -- -D warnings`、`cargo build --workspace --locked`、`git diff --check`: すべてPASS。
- `cargo test --workspace --locked`: このbatchでは未実行。full workspace runtime acceptanceは未確立。
- Unknown effectはaction ledger状態fixtureであり、物理的なクラッシュや不明書込を発生させた証拠ではない。資源停止はdeadline/action数の限定例、権限停止はローカルファイル一種類。全系の中断・再開・remote effects・全93要件を証明しない。E02/E04/E05/E06/E10および全requirement acceptanceは`UNVERIFIED`。releaseなし。

## 次工程

三点セットの「中核確認七」へ進み、同一の入力・記憶・模型・資料版で結果が再現し、条件を変えた場合は差分の原因を版情報で説明できるかを観測する。
