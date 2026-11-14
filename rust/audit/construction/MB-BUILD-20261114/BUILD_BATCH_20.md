# BUILD_BATCH_20 — v75出典spanの通常経路・上限停止検証

## 施工対象

- 親工程: 三点セット第六命令「中核の必要に従い入力機を構築」。
- 基点: B19を含む `14072894f418c2bc5a49d284642b6c0ce947a6c1`。
- 実行環境: Linux x86_64、Rust/Cargo 1.99.1、ローカルSQLite一時状態。
- 目的: B19の全Intent文字列leaf source-role indexを、通常の `Engine::execute` と保存再生経路で観測し、重複候補の資源上限を有限の型付き停止にする。

## 施工内容

`input.rs` の重複一致走査を1,000,000候補イベントで停止する。上限到達時は元の要求と解析済みIntentを `MeaningIR` として封印し、未解決理由 `source role indexing stopped` を残し、source-role mapは空にする。`core.rs` は封印と `SEALED` 遷移の後でこの印を検出し、`SOURCE_SPAN_INDEX_LIMIT` として `SUSPEND` する。これにより、不完全な出典mapを使った処理を防ぎつつ、`RECEIVED → SUSPEND` の不正遷移を避ける。

追加した通常Engine統合テストは次を確認する。

- `1,10,3 の平均と分散を計算して` で統計値roleごとの正確なUTF-8 byte spanを確認し、`1` が `10` の部分文字列を誤取得しない。
- JSON Pointer要求のdocumentとpointerの原文範囲を確認する。
- `Unknown.reason` と原文が一致するケースでも、生成されたreasonをユーザー原文のsource roleとして記録しない。
- 長さ1〜350の連続した数字候補で100万超の重複候補イベントを発生させ、完全なsource-role mapを捨てて `SUSPEND` する。
- 4件すべての確定ケースをEngine再open後に再実行し、保存済み結果とcase revisionが一致する。

## 修正の経緯

最初の上限処理は `input::seal` から上限エラーを返し、Coreが `FAIL/INPUT_FAILURE` を保存した。これでは有限の資源不足停止という意図を表せない。Coreの封印前に直接 `SUSPEND` する案も状態機械に拒まれ、`INVALID_TRANSITION: Received -> Suspend` となったため撤回した。最終実装は上限状態を `MeaningIR.unresolved` に保持し、合法な `SEALED → SUSPEND` として確定する。最初の通常テストではreasonに停止コードが二重表示されたため、内部markerからコードprefixを除き、最終結果を `SOURCE_SPAN_INDEX_LIMIT: source role indexing stopped: ...` に統一した。初期FAILの再現ログはローカル検証資産に保持する。

## 検証結果

- `cargo fmt --all -- --check`: PASS。
- `cargo check --workspace --all-targets --locked`: PASS。
- `cargo clippy --workspace --all-targets --locked -- -D warnings`: PASS。
- `cargo test -p minidora-engine --test input_machine_observation_03 --locked -- --nocapture`: PASS、1件。
- `cargo test --workspace --all-features --locked`: PASS、400件、失敗0件。内訳はCLI unit 10、通常CLI経路141、domain 4、engine unit 235、construction統合5、meaning composition 2、input machine統合3。
- `cargo build --workspace --locked`: PASS。
- `git diff --check`: PASS。
- ログ本体は `/workspace/.minidora-cloud/b20-input-span-verification/` に保存し、SHA-256を [B20 validation record](../../bb-input-machine-observation-03-v75-runtime-validation.json) に記録する。

## 適用範囲と残る境界

検証したのは選択した統計・JSON Pointer・Unknown・資源上限の通常Engine経路と同一ストア再openでの保存再生である。文字列一致は意味的帰属を証明せず、32件/roleの保存上限も維持する。多様な言語・曖昧性母集団、全受入条件、全環境QA、GPQAおよびリリース受入は検証していない。A04/A15/E08と全体の受入statusは `UNVERIFIED`、releaseは `NOT_RELEASED` のままとする。

Validation metadata: [B20 runtime route and workspace verification](../../bb-input-machine-observation-03-v75-runtime-validation.json)。
