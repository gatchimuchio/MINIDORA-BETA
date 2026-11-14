# BUILD_BATCH_17 — 入力機の短縮算術質問

## 工事境界

- 対象: 三点セット第六命令「中核の必要に従い入力機を構築」、既存の通常 `RequestEnvelope → input::seal → Engine::execute` 契約。
- 入力不足: `６＋７は？` は原文の終端疑問符と完全な算術式を含むが、既存の末尾句型に裸の話題助詞 `は` がなく `UNKNOWN` となっていた。
- 変更: 終端が `?` または `？` で、`は` より前が既存 `arithmetic_expression` 全体に適合する場合のみ `Intent::Calculate` へ接続。parser revisionを `declarative-ja-en-v73-arithmetic-topic-question` に更新。
- 境界: 疑問符なしの断片、式でない語、追加の別指示は計算へ取り込まない。原文・全角表記・input versionは既存sealed bundleに保持。Coreの計算、内容採用、描画・予算判定は変更しない。

## 実入力観測

1. `６＋７は？`、input version 1を通常の `Engine::execute` に渡す。
2. 入力機は元文15 UTF-8 bytesと範囲 `[0,15)` を保持し、parser v73で `Calculate("6+7")` として封印する。
3. 中核の通常計算経路が値13を `COMPUTED / COMMIT` し、model revision、ResultRecord、render SHA-256を保存する。
4. Engine objectを閉じて同じSQLite storeを再openし、同じ input ID/versionを再送。Storeが保存済み結果を返し、ResultRecordとCaseRevisionは完全一致してrevisionを増やさない。
5. 専用unit testの負例 `6+7は`、`謎語は？`、`6+7は？保存して` は計算intentにならない。

全recordと原本asset checksは [B17 input-machine validation](../../bb-input-machine-observation-01-v73-validation.json)。実process再起動や広範な日本語対応はこの観測範囲外。

## 検証と保証範囲

- 専用統合観測: `cargo test -p minidora-engine --test input_machine_observation_01 --locked -- --nocapture`: PASS、1件。
- Parser negative/regression testsと既存観測を含む `cargo test -p minidora-engine --locked`: PASS、233 unit tests、8 bounded integration tests、0 doc tests。
- `cargo fmt --all -- --check`、workspace all-target check、warnings-denied Clippy、locked workspace build: PASS。
- `cargo test --workspace --locked`: 未実行。A02/A04/A12/A15/E08および全requirement acceptanceは`UNVERIFIED`。一般日本語、開放語彙、自由な数学依頼の完成を主張しない。releaseなし。

## 次工程

第六命令を続け、[BUILD_BATCH_18](BUILD_BATCH_18.md) で選択されたmeaning roleと元のUTF-8位置を結ぶoptional source-role spanを追加した。未対応Intent群を引き続き棚卸しし、曖昧入力の保留とinput versionごとの一回封印を確認する。
