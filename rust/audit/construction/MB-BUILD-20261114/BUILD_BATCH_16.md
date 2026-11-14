# BUILD_BATCH_16 — 中核の直接施工確認七

## 工事境界

- 対象: `gatchimuchio/MINIDORA-BETA` Rust Core、三点セット施工指示の「中核確認七」。
- 目的: 同一条件のCore実行が同じsealed入力・記憶・模型revision・case/result状態を再現し、前提を版更新した後の結果差を原因付きで説明できるか観測する。
- 実経路: 二つの独立した一時SQLite状態で、同一actor・case・入力列を `Engine::execute` に渡した。訂正前にEngineを閉じて同じDBを再度開き、既存入力IDのversion 2を実行。
- 作用境界: 文書入力、外部意味生成、ネットワーク、外部作用なし。内容計画の固定Core模型revisionは記録した。

## 実装と観測

追加した統合観測は、最初の規則・前提・照会の全ResultRecord、sealed InputBundle、CaseRevision、MemoryStateを二つの独立状態間で比較する。さらに訂正後の入力束・結果・再照会・最終CaseRevision・MemoryStateも比較する。

1. `[scope=repeatability-lab]X is a sensor and calibrated -> X is reliable` という宣言規則、Neraのsensor/calibrated前提を保存する。
2. 同一の `[scope=repeatability-lab]Nera is reliable?` は二状態で同一の `DERIVED / COMMIT` になり、前提と規則のproof dependencyを持つ。全4件の初期内容計画model revision、raw入力hash、parser version、result・render情報も一致。
3. Engineを再openし、既存の `sensor-observation` input IDをversion 2へ進め、「Nera is a polymer, not a sensor」と明示訂正する。Coreは旧sensor Factを失効させ、Memory revisionを3から4へ進める。訂正のparser versionとmodel revisionも保存。
4. 同じ意味のrelation questionを再投入すると、有効なsensor premiseがなくなったため `MISSING_PREMISES / SUSPEND` となる。古いderived ResultRecordは `prior_results` に保持される。
5. version 2訂正後の全記録と停止結果も、独立状態間で完全一致した。

実入力、初期・訂正後model revision、raw hash、proof dependencies、history、停止理由、実行環境、asset hashesは [B16 direct-Core validation](../../bb-core-construction-observation-07-v1-validation.json) に保存する。三点セットの原本と3文書のhash/byte数もmanifestと照合した。

## 検証と保証範囲

- 専用観測: `cargo test -p minidora-engine --test core_construction_observation_07 --locked -- --nocapture`: PASS、1件。
- `cargo test -p minidora-engine --locked`: PASS。232 unit tests、B11/B13/B14/B15/B16直接観測各1件、B12 meaning-composition統合観測2件、doc tests 0件。
- `cargo fmt --all -- --check`、`cargo check --workspace --all-targets --locked`、warnings-denied Clippy、locked workspace build: すべてPASS。
- `cargo test --workspace --locked`: 未実行。full workspace runtime acceptanceは確立していない。
- 資料入力はなく、資料版違いの再現性は対象外。独立DBは同一host/process/toolchain内の比較であり、process scheduling差、別OS/toolchain、外部模型、全93要件のacceptanceを証明しない。A03/A15/CAP-D01/CAP-D04/E08/G08および全requirement statusは`UNVERIFIED`。releaseなし。

## 次工程

七つの中核施工観測を記録した。三点セットの第六命令「中核の必要に従い入力機を構築」に移り、実中核が要求する原文・意味役割・文脈・出所・権限・条件・不足・前回成果と既存`RequestEnvelope`/`MeaningIR`/`InputBundle`契約との差を調べる。入力機は解釈を入力版ごと一度だけ封印し、曖昧さを保留し、学習採否や行動計画を決めない。
