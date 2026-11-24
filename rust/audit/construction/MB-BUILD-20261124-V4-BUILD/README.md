# MINIDORA-BETA｜BUILD専用v4・2026-11-24採用索引

本版は**独立した汎用AI駆動エンジンの標準ベータを構築・固定するBUILD作業**の現行正本。品質検査（QC）、品質保証（QA）、独立試験、GPQA・性能評価、全OS実機保証、全93要求の個別受入は今回の施工工程・完了条件ではない。コンパイル、起動、実入力→Core→実出力の直接確認はBUILD成果の存在確認として扱う。

## 現行4文書（UTF-8／日本語意味基底／JSONL）

| 役割 | 文書 | レコード | 原文SHA-256 |
|---|---|---:|---|
| 要求仕様書 | [MINIDORA_BETA_要求仕様書_BUILD専用v4_20261124_日本語JSONL.jsonl](MINIDORA_BETA_要求仕様書_BUILD専用v4_20261124_日本語JSONL.jsonl) | 234 | `bf48d90da76f319ba3d29682d43108f8ca9b3789a4f984729adfc2b40ea60c88` |
| 設計仕様書 | [MINIDORA_BETA_設計仕様書_BUILD専用v4_20261124_日本語JSONL.jsonl](MINIDORA_BETA_設計仕様書_BUILD専用v4_20261124_日本語JSONL.jsonl) | 160 | `3add6a349c440e6452037e28e83280cf7cd5272089ff30173b0146c92df20d4b` |
| 開発実装工程表 | [MINIDORA_BETA_開発実装工程表_BUILD専用v4_20261124_日本語JSONL.jsonl](MINIDORA_BETA_開発実装工程表_BUILD専用v4_20261124_日本語JSONL.jsonl) | 30 | `2f03f47ace9ead33cdf4e7d3c8d60e788fb125d34f2ccba54496915e11b0d6b9` |
| Codex施工指示書 | [MINIDORA_BETA_Codex施工指示書_BUILD専用v4準拠_簡潔版_20261124_日本語JSONL.jsonl](MINIDORA_BETA_Codex施工指示書_BUILD専用v4準拠_簡潔版_20261124_日本語JSONL.jsonl) | 22 | `06778f2b28e1ebc91f776675bf3ecfa9f7123e85018cf6978483d9f0d4979f2e` |

効力：**本人の最新原文指示 → 要求仕様書 → 設計仕様書 → 開発実装工程表 → Codex施工指示書 → ルートAGENTS.md（作業要領）**。作業要領は上位の要求・設計・工程に上書きしない。仕様の施工対象は `rust/`、他repo・凍結Python `0.6.0b1`・既存リリース資産は不変更。

## 変更した責務と履歴との境界

- 旧R2/D2/D3-CのE0〜E8、CORE-G、T、93要求、416旧ソース、360旧テスト、B01〜B233の実装・評価記録は**履歴／設計参照として保存**。現在のBUILD-v4に全体系QC・独立QA・全件評価を再導入しない。
- 93要求・416旧ソース・360旧テストの履歴義務や未観測は消去しない。現行BUILDの成果は現提供能力と既知制限を明示し、未提供を提供済みと称さない。
- 起点 `27fc04ad12e3661952f466102b967403ddb6a381` と Rust `0.7.0-alpha.11` は2026-11-22時点の観測値。**毎着工時に最新HEAD・差分・実装版を再確認**する。文書採用＝現行Rust実装の完成宣言ではない。
- 2件の汎用v1.2標準は4文書中に名称とSHA-256を出典として保持する。外部参照資料であり、この索引は非公開原理の転載・一意復元表の公開を許可しない。
- GitHubへの文書反映の許可と、Rustコードの新規施工／公開／配布／外部送信の許可は別。追加の操作は具体的な本人指示・権限に従う。

現行の共通要領は [AGENTS.md](../../../../AGENTS.md)、旧版索引は [construction/README.md](../README.md)、施工時点の実装記録は [IMPLEMENTATION_STATUS.md](../../IMPLEMENTATION_STATUS.md)。
