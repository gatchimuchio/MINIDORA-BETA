# BUILD_BATCH_39 — 導出関係比較へ保存規則の根拠単位を接続

**main基線:** `2124e6cd3e85fa373b4c2959b174dbb47447c3f5`（B38）。
**主語/目的:** 唯一Coreが保存関係を比較する際、導出された関係の適用規則を比較内容計画に保持し、旧比較artifactの再読経路を保つ。
**施工範囲:** `Intent::CompareRelationInventories`のCore内容要求生成と比較artifact再読/再生。入力文法、保存形式の意味、出力作用は変更しない。
**ゲート:** `CORE-G03 = PARTIAL`を維持。実行経路の新しいV3 producer→K7到達は未観測。他のCOREゲートは再判定しない。

## 変更したproducerと次consumer

B38のV2比較構成は各RelationInventoryのfact IDとdependency IDをContentPlan候補に保持していたが、規則IDの条件・結論は本文上で参照IDとしてしか見えなかった。B39では次を追加した。

`左右RelationInventoryのfact/dependency IDs`
→ Core actor memoryから当該proof IDsに含まれる有効な保存規則を選択
→ 規則の前提・追加前提・結論・範囲・規則IDを`MeaningRole::Evidence`単位として追加し、規則間dependencyを型付きunitの依存順に反映
→ `MeaningCompositionPurpose::Compare`の既存K4構成器
→ ContentPlan/K7の既存経路

これはコード上の接続を確認した状態であり、このbatchでは新しいV3比較を実行していないため、値がK7へ実際に届いたとは報告しない。

### 互換・境界

- 直接前提だけの比較は従来のV2 artifactを生成する。
- 規則がproof内にあり追加単位が現在のcomposer制限内に入る場合はV3 artifactを生成する。
- V3の規則単位が16 unit、384 byte/unit、16 evidence ID/unit、8192 byteの制限に入らない場合は従来V2構成を試し、それも構成不能なら従来V1比較レポート経路へ戻る。意味単位を黙って落とさない。
- 既存V1レポートとV2内容artifactのdecode/replayは形式別の旧経路を保つ。V3は同じrecord fieldsを使う別`kind`として読み、追問時には現行inventory・保存規則からV3 requestを再生成して照合する。
- `MeaningRole::Evidence`を使い、新しい判断主体・学習器・入力解釈経路は増やしていない。

## 限定確認

- `cargo fmt --all -- --check`: **PASS**。
- `cargo check --locked -p minidora-engine --all-targets`: **PASS**（test targetsを含むcompile check。テスト関数は実行していない）。
- `git diff --check`: **PASS**。
- B39の導出比較V3、V3再起動後追問、旧V1/V2追問: **NOT_RUN**。実行試験を追加・実行していない。
- workspace runtime test suite、Clippy、独立内容妥当性、通常CLI/HTTP経路、Windows/macOS実機、一般文章品質、T01–T03、CORE-G03残余成功述語: **NOT_RUN**。
- Linux / Rust `1.99.1` / `CARGO_TARGET_DIR=/workspace/.minidora-cloud/rust-target`。

## 判定

変更後のengine all-targets型検査は通過した。V3はCoreの比較内容要求生成とartifact replayへ配線されたが、実行試験をしていないため、規則条件・結論が出力・K7まで到達することも、V3追問が成功することも未確認である。R4の中核成功述語、比較内容の独立妥当性、一般入力・文章品質を満たす根拠はなく、`CORE-G03 = PARTIAL`を維持する。七ゲート成立、入力機工程移行、三機通電は主張しない。

## 資産状態

| 状態 | 個数 | 対象 |
|---|---:|---|
| keep | 5 | `core.rs`、`IMPLEMENTATION_STATUS.md`、`IMPLEMENTATION_CHOICES.md`、本BUILD記録、`b39-build-observation.json` |
| store | 0 | 独立保管への移動なし |
| in-use | 3 | `/workspace/.minidora-cloud/rust-target` 6.7G、`/workspace/.cargo` 252M、`/workspace/.rustup` 617M（共有build/cache/toolchain。削除せず） |
| trash | 0 | B39の一時ファイル・scratch directoryは未作成 |
| hold | 0 | 保留する生成物なし |
| **合計** | **8** | keep 5 + store 0 + in-use 3 + trash 0 + hold 0 |

添付された2026-11-14三点セットZIPは既存の履歴版とSHA-256が一致した。展開・複製はしていない。R4三文書と同ZIP/MANIFESTは変更していない。

## 次consumer

導出根拠を含む比較を実行し、V3のContentPlan、K7 RenderObservation、保存artifact、プロセス再起動後のinventory/rule再計算と追問照合を一つの実測で確認する。既存V1/V2再読も同じ限定経路で保持されるかを確認する。
