# BUILD_BATCH_13 — 中核の直接施工確認四

## 工事境界

- 対象: `gatchimuchio/MINIDORA-BETA` Rust Core、三点セット施工指示の「中核確認四」。
- 目的: 実際に確定・保存された計算結果と関係結果を後続要求が参照し、修正後の値を同じ入力IDの新しいversionとして保存するproducer→consumer経路を観測する。
- 実経路: 通常の `input::seal` と `Engine::execute`。直接意味注入や固定回答の注入は使わない。
- 永続化: 各versionは独立した封印済み入力・結果として記録される。前versionの確定 `ResultRecord` は `prior_results` に残る。ケースrevisionはライフサイクル遷移も記録するため、入力versionと同じ数にはならない。
- 作用境界: 隔離一時SQLite actor/case。外部モデル、ネットワーク、外部副作用なし。提示前提は現実世界で検証済みとは扱わない。

## 実装と観測

既存Coreに計算訂正 (`CorrectCalculation`)、過去計算の `ExplainPrevious`、関係結果を文脈に使う `AssertPreviousRelation` が存在していた。今回は新しい回答処理を足さず、不足していた施工観測を追加した。専用試験は実際の自然文入力、sealed `MeaningIR`、保存結果・依存ID、memory state、後続質問を一つの隔離caseで追跡する。

1. 入力ID `calculation` のversion 1で `8+5` を計算し、確定値 `13` を保存。
2. 同じ入力IDのversion 2で「第二項を3に変更してください」を処理。封印文脈はversion 1の結果IDを指し、Coreはその保存式から `8 + 3 = 11` を再計算。version 2結果はversion 1結果IDに依存し、旧version結果は `prior_results` に保持。
3. 次の「Why?」がversion 2の実計算・式・値を説明し、その結果IDを依存に含める。
4. 二条件規則とPicoの前提を保存し、実際の関係照会 `[scope=virtual-lab]Pico is conductive?` を `DERIVED` として確定。証明依存は規則と両前提を保持。
5. 同じ入力ID `relation-question` のversion 2に `[scope=virtual-lab]it is monitored` を送る。Coreは直前の確定関係結果IDをsealed contextから確認し、現在のActor memory内で同一の関係proof、依存、保証を再検証してから主語Pico/scope `virtual-lab` を復元。`Pico is monitored` を新しい `USER_PREMISE` として保存し、version 1の関係結果IDと新Fact IDをversion 2結果の依存に記録。
6. 後続照会 `[scope=virtual-lab]Pico is monitored?` が保存Fact IDを使って確定。

完全な要求・結果JSON、case revision、依存ID、保持されたversion 1結果、memory Fact、toolchain、test log hashと三点セットasset検証を [B13 direct-Core validation](../../bb-core-construction-observation-04-v1-validation.json) に保存した。

## 検証と保証範囲

- `cargo test -p minidora-engine --test core_construction_observation_04 --locked -- --nocapture`: PASS。単独観測1件。
- `cargo test -p minidora-engine --locked`: PASS。232 unit tests、B11統合観測1件、B12統合観測2件、B13統合観測1件、doc tests 0件。
- `cargo fmt --all -- --check`、`cargo check --workspace --all-targets --locked`、`cargo clippy --workspace --all-targets --locked -- -D warnings`、`cargo build --workspace --locked`、`git diff --check`: すべてPASS。
- `cargo test --workspace --locked`: このbatchでは未実行。full workspace runtime acceptanceは未確立。
- 本観測は一つの計算訂正・一種類の型付き関係結果・同一入力IDのversion 1/2に限定する。一般的な照応・一般学習・現実世界の真偽・三機品質・全93要件を証明しない。A01/A03/A04/A07/E08および全要件のacceptance statusは `UNVERIFIED` のまま。releaseなし。

## 次工程

三点セットの「中核確認五」は [BUILD_BATCH_14](BUILD_BATCH_14.md) で観測した。宣言済み規則の証明付き合成が未見主体への実結果を変え、訂正後に古い根拠を使った結論を再生せず停止するかを確認した。
