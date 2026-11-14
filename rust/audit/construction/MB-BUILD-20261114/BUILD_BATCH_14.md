# BUILD_BATCH_14 — 中核の直接施工確認五

## 工事境界

- 対象: `gatchimuchio/MINIDORA-BETA` Rust Core、三点セット施工指示の「中核確認五」。
- 目的: 原記録に根ざす条件規則の合成が、別主体の新しい問題へ実際に適用されること、根拠訂正後に古い依存結論を再利用しないことを観測する。
- 実経路: 自然文規則・前提・照会・訂正を通常の `input::seal` と `Engine::execute` へ渡す。外部意味生成なし。
- 既存機構: `relations::add_rule_with_premises` は新しい宣言済み規則と有効なroot規則を、親規則IDを保持する限定shortcutへ合成する。queryは実subject factsとshortcutを組み合わせる。`CorrectPremise` は訂正対象の直接前提とその依存メモリ記録を失効させ、旧記録を保持する。
- 作用境界: 隔離一時SQLite actor/case、外部モデル・ネットワーク・副作用なし。すべての関係は `USER_PREMISE` / `DERIVED` として扱い、現実世界の真偽を保証しない。

## 実装と観測

今回、不足していた統合構築観測を追加した。新しい生成機能は作らず、Coreの関係規則合成、proof dependency lineage、訂正による履歴維持と後続再照会を同じcaseで確認する。

1. Coreへ二つの独立したroot規則を入力: ceramic → fragile、および fragile ∧ struck → cracked。
2. Core memoryに両root IDをdependencyとして持つ限定shortcut ceramic ∧ struck → cracked が形成される。Rivaはroot規則の変数subjectとしてしか現れない。
3. 新subject Rivaについてceramic、struckの二つの直接前提を追加。Rivaを使った正解や出力文は実装・fixtureに書かず、保存済み規則から初回の `[scope=materials-lab]Riva is cracked?` が `DERIVED / COMMIT` となり、双方のroot rule、shortcut、二前提のIDを依存に持つ。
4. Query closureの導出Factはactor memoryへ勝手に追加しない。Coreが確定した `ResultRecord` と `ContentPlan` に実出力とproof IDs、model revision、render hashが保存される。
5. Scopeを明示した訂正「Riva is a polymer, not a ceramic」を実行。元のceramic Factは `valid=false` のまま履歴に保持され、訂正結果は失効対象IDを依存として記録。前のderived ResultRecordも履歴に保持される。
6. 新しい同一質問は `MISSING_PREMISES` で `SUSPEND` し、旧COMMIT内容を再生しない。

実入力・確定結果・証明ID・規則合成状態・変更前後memory・停止理由・model/render hashを [B14 direct-Core validation](../../bb-core-construction-observation-05-v1-validation.json) に保存する。元の三点セットmanifest asset hashはすべて再照合した。

## 検証と保証範囲

- `cargo test -p minidora-engine --test core_construction_observation_05 --locked -- --nocapture`: PASS。専用観測1件。
- `cargo test -p minidora-engine --locked`: PASS。232 unit tests、B11統合観測1件、B12統合観測2件、B13統合観測1件、B14統合観測1件、doc tests 0件。
- `cargo fmt --all -- --check`、`cargo check --workspace --all-targets --locked`、`cargo clippy --workspace --all-targets --locked -- -D warnings`、`cargo build --workspace --locked`、`git diff --check`: すべてPASS。
- `cargo test --workspace --locked`: このbatchでは未実行。full workspace runtime acceptanceは未確立。
- これは宣言済み規則二つを合成する bounded rule adaptation であり、生のunstructured examplesから一般規則を帰納するものではない。学習全般、未知領域の一般化、仮説規則の広域採用、outside-world truth、全93要件のacceptanceを証明しない。CAP-D05/D06/D07/D12/D13、A04/A07/A15/E08を含む全requirement acceptanceは `UNVERIFIED`。releaseなし。

## 次工程

三点セットの「中核確認六」は [BUILD_BATCH_15](BUILD_BATCH_15.md) で観測した。資源・権限・取消・不明作用・未達成の必須成果に対する停止を確認した。
