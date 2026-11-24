# MINIDORA STANDARD BETA｜BUILD-v5採用索引

**採用日：2026-11-24／対象：gatchimuchio/MINIDORA-BETAのRust後継標準ベータ。**

本版は**要求→設計→開発実装工程→Codex施工指示**の現行4文書と、共通作業要領AGENTS.mdの原資料を保持する。文書反映は実装完成ではない。

## 原文と同一性

| 文書 | 内容 | SHA-256 |
|---|---|---|
| [要求仕様書](MINIDORA_STANDARD_BETA_要求仕様書_v5_20261124_日本語JSONL.jsonl) | 284レコード。81件のBUILD要求＋旧93要求・70旧要求・22欠陥継承 | `cfb8d9011925e49dccfbae0fbeb4ecb471d393f50256a2a25548834c1e2b984c` |
| [設計仕様書](MINIDORA_STANDARD_BETA_設計仕様書_v5_20261124_日本語JSONL.jsonl) | 152レコード。Core・会話・記憶・Web・専門Port・標準API | `10ea3639b97eaa6e44b2ffd8ad1bd6160d9a78a47f07840ef1bc20e06bc1ce96` |
| [開発実装工程表](MINIDORA_STANDARD_BETA_開発実装工程表_v5_20261124_日本語JSONL.jsonl) | 25レコード。P00～P110のBUILD12工程 | `482123836ab012c82beaa17f0e472a507e7517985343b09b10f8224fcfbc6895` |
| [Codex施工指示書](MINIDORA_STANDARD_BETA_Codex施工指示書_v5_20261124_日本語JSONL.jsonl) | 29レコード。12工程に対応する施工命令 | `c38196bebd6954e91604cc0205aac06d26cf86d6eab249225423d6fb481786e4` |
| [AGENTS.md原案](MINIDORA_STANDARD_BETA_AGENTS案_v5_20261124.md) | 作成当時の未採用原案を証拠として保存。現行要領は[ルートAGENTS](../../../../AGENTS.md) | `7e6cb8fef7bad5325dc8160decba048585d59741de282bf2e450b54777c3d877` |
| [原資料の索引](MINIDORA_STANDARD_BETA_資料索引_v5_20261124.md) | 独立生成時点の記録 | — |

## 現行BUILD完成対象

過去の設計・改善を統合し、**git clone→簡単起動→ローカル日本語多ターンチャット→記憶/推論/非ニューラル生成→許可されたネット検索候補の自律発見・実本文取得→Coreの回答→既存数理/文書/コード能力→標準Native/CLI・版固定**を実体として成立させる。BLUE-TANUKIやkonjac-Translation等は後続consumerであり、今回は統合しない。

コンパイル・起動・通常チャット・Web取得・専門能力利用は施工物の直接確認。**QC、QA、独立試験、GPQA・性能評価、全OS保証、外部配布は今回の工程に含めない。** 必須の利用経路が未接続なら、既知制限に振り替えてBUILD完了としない。

## 版史と権限

[旧BUILD-v4](../MB-BUILD-20261124-V4-BUILD/README.md)および`50f6514`の記録は当時の限定的なビルド・計算CLI/API・作用観測として残す。今回のv5標準ベータの完成証明には使わない。R2、D2/D3-C、旧93要求、旧416ソース、旧360テスト、凍結Pythonβを保存し、旧版原文を遡及改変しない。

原文内の「repo未反映」「今回のGitHub変更は禁止」は**文書作成時点の状態・権限**を表す。本人の後続「リポに反映しといて」により本版の資料採用が認可されたが、**Rustの実装開発・他repo変更・Release/配布の権限は発生しない**。

[版索引](../README.md)／[現行AGENTS](../../../../AGENTS.md)／[施工履歴](../../IMPLEMENTATION_STATUS.md)
