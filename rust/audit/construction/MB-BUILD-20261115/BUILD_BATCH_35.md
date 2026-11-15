# BUILD_BATCH_35 — K4 候補選択から K7 描画までの実値確認

**main 基線:** `a4bddcd2b103538a2eb61c55b44d201bda61c3f4`。

**現在工程:** 中核 K1〜K7 の未成立箇所を施工。`CORE-G03 = PARTIAL`。この束では他ゲートを再判定しない。

## 対象・実経路

B34 の導出説明経路で参照された actor/case 範囲の保存物を、`Store::get_bytes` で読み戻した。対象は保存済み `SequenceModel` と `ModelEmission`。改変なしでそれぞれのartifact refとSHA-256を照合し、実モデルを検証した後、Coreが許可した二つの等価表層候補を再採点した。

実経路は `Engine::compose_atomic_explanation` → K2/K3 の導出proofと根拠付き説明units → Core許可済み表層候補 → 保存 `SequenceModel` による厳密系列確率の順位付け → `ModelEmission` → `ContentPlan` → K7 → 実 `RenderObservation`。

模型の実値は `inherited-char-ngram-v1`、order 2、smoothing 1、6 documents、vocabulary 104、context 104、revision `f3e47b80f36df9976b6d9581e28194772c3f282f8565cd87bf948d0c4328b3a1`。モデルartifact refは `43d2d8fd575593e4abdd39504c13a609858fecfda15cc224bb07a35681d5a144`（5,008 bytes）。emission refは `05b0b5e412e0373b5daa56d9d05f2eed430d40320177be3e341fff0724718ee6`（108,789 bytes）。両artifactの完全な保存JSON、候補別の厳密な有理数スコア、選択値、lossless projection、context hashesを[実行観測](b35-model-selection-observation.json)へ保持した。

最高スコアは候補0で、選択文字列は `提示された証拠が支える結論: Velaはreliableである` に続く「根拠のつながりは次のとおりです。」。同じ保存模型と候補集合を再生した `ModelEmission` は保存値と一致し、ContentPlan本文とK7の描画bytesも一致した。二候補は接続文だけが異なり、結論、両前提、派生Fact、規則、範囲・真偽の但し書きを保つ。

## 安全境界

- 前提を欠く `Vela is conductive` は `SUSPEND / MISSING_PREMISES`。ContentPlanとrenderを作らない。
- write grant 0で指定した出力は `SUSPEND / WRITE_PERMISSION_MISSING`。描画後に停止し、対象fileなし、receipt 0、action record 0。
- DBはテスト専用一時ディレクトリ内でのみ作成し、終了時に削除する。ネットワーク作用なし。

## 限定確認と判断

- `cargo fmt --all -- --check`: **PASS**。
- `cargo test --locked -p minidora-engine --test core_atomic_explanation_34 -- --nocapture`: **PASS**（1 passed / 0 failed、14.14 s）。Rust `1.99.1 (58cf8f9c3 2026-10-13)`、Linux。
- 観測JSON: 221,656 bytes、SHA-256 `50dda87d8fec95385e4c83e457e6b5f74db2b20f06bd51e5585b675ffa71ca4e`。
- 全workspace実行試験、一般的な新内容計画・文章品質、通常CLI/HTTPへの当該入力経路、残りの CORE-G03 成立述語: **NOT_RUN**。

保存モデルはCoreが作った説明unitsで学習し、候補もCoreが許可した有限の二表層だけである。この観測は模型が候補を選びK7へ渡した実作用を示すが、新しい事実内容の生成や一般言語能力を示さない。したがって `CORE-G03 = PARTIAL` を維持する。

**次consumer:** 初見原子的意味から、proofに対応する複数の新しい内容unit/計画をCore自身が作る経路の不足へ戻る。入力機や専門機能の工程へは進まない。
