# BUILD_BATCH_38 — 型付き比較のCore内容計画をK7へ接続

**main基線:** `eb8162c30d1e3414933d533a71802bd6edfce791`（B37）。
**主語/目的:** MINIDORA-BETA唯一のCoreが、左右の保存proofに基づく比較を新しい意味単位へ構成し、採用ContentPlanとK7実描画へ渡す。
**施工範囲:** `Intent::CompareRelationInventories`のCore経路、比較artifact読取/再生、既存回帰test期待値。入力文法・外部作用・新専門機能は変更しない。
**ゲート:** `CORE-G03 = PARTIAL`を維持。他のCOREゲートはこのbatchで再判定しない。

## producer → consumer

完成済み比較文を入力せず、型付きの2対象/scopeとCore actor memoryの左右別`RelationInventory`を使った。

`subject_a + subject_b + scope + current left/right proofs`
→ Coreが関係keyごとの共通/片側/競合単位、各factの保証区分とID、依存proof参照、限定文を生成
→ `MeaningCompositionPurpose::Compare`の`MeaningCompositionRequest/Report`
→ version 2比較artifact
→ `ContentPlan.required_units`と採用文面
→ K7 `RenderObservation`
→ 同じケースの再起動後追問で左右inventory再計算・request再構成・モデル再生・前回result/render照合。

片側で支持がない結果を関係の不存在と扱わず、競合する記録は共通/片側のみへ確定しない。すべてのinventory dependency IDとfact IDを構成traceに保持した。16 unit/8192 byte/各384 byteの既存composer境界に入らない場合は保存済みversion 1比較レポート経路を維持し、黙って単位を落とさない。採用可能なrequestのcomposer失敗は`SUSPEND`する。

version 1の既存比較artifactは独立してdecode/replayする。B36のfixtureは現在version 2になるため、B36 regression testのartifact-kind期待値を更新した。初回のB36回帰実行は旧version 1 kind期待値との不一致でFAILした。この実測ログは[`b38-b36-initial-expectation-mismatch.log`](b38-b36-initial-expectation-mismatch.log)へ保持し、test更新後のPASSはfocused logに追記した。別fixtureではcomposer境界を越えるsubject文字列を使い、version 1 artifactを実生成し、プロセス再起動後の追問で旧readerが`COMMIT`することを確認した。

## 限定確認

- `cargo fmt --all -- --check`: **PASS**。
- `cargo test --locked -p minidora-engine --test core_atomic_relation_comparison_38 -- --nocapture`: **PASS**（proof ID、typed Compare roles、ContentPlan/render一致、肯定/否定競合の保留、再起動後のversion 2追問、version 1 fallback/recovery）。
- `cargo test --locked -p minidora-engine --test core_atomic_relation_comparison_36 --test core_atomic_explanation_31 --test core_atomic_explanation_34`: **PASS**（B36根拠不足/未許可書込停止、B31/B34のK7説明回帰を含む）。
- 同一の限定実行結果ログ: [`b38-focused-tests.log`](b38-focused-tests.log)、8,205 bytes、SHA-256 `b26a0579085a8bb0555d7bf44315caa2c29b48aa17dfcb52bad8c072f0e32eea`。
- 初回B36期待値不一致ログ: [`b38-b36-initial-expectation-mismatch.log`](b38-b36-initial-expectation-mismatch.log)、1,186 bytes、SHA-256 `40d84118b7b4e3d8d38fda4502c79a0f5c3a5e14dd1ccb4199f16620024960ce`。
- 実行trace: [`b38-runtime-observation.json`](b38-runtime-observation.json)、6,692 bytes、SHA-256 `a7305ebc786da79a382c96fa4e38d21fa7d52214bf9c2cd4ed099ea532aeba3b`。
- Linux / Rust `1.99.1` / `CARGO_TARGET_DIR=/workspace/.minidora-cloud/rust-target`。全workspace test、独立した比較内容妥当性、自然文入力全域、Windows/macOS実機、一般文章品質、CORE-G03残り成功述語、T01–T03: **NOT_RUN**。

## 判定

本batchはCoreの左右proof inventoryからCompare目的の意味単位を生成し、既存系列構成器を通してContentPlan/K7へ値を渡した一つの型付き比較fixtureを実測した。V1 artifact読取、保存内容の再生、前回比較の継続説明もこの範囲で保持した。一方でcomposer容量を越える比較は従来V1レポート経路へ残り、母集団・一般言語・独立意味品質・通常入口の比較能力は確認していない。よって`CORE-G03 = PARTIAL`であり、七ゲート成立・入力機工程移行・三機通電は主張しない。

## 資産状態

| 状態 | 個数 | 対象 |
|---|---:|---|
| keep | 9 | `core.rs`、B36/B38テスト、`IMPLEMENTATION_STATUS.md`、`IMPLEMENTATION_CHOICES.md`、本BUILD記録、runtime observation、focused tests log、初回B36期待値不一致ログ |
| store | 0 | 独立保管への移動なし |
| in-use | 6 | `rust/target` 7.1G、`/workspace/.minidora-cloud/rust-target` 6.6G、`/workspace/.cargo` 252M、`/workspace/.rustup` 617M、`/home/agent/.cargo` 155M、`/home/agent/.rustup` 617M（共有Rust toolchain/cache。削除せず） |
| trash | 13 | `/tmp/minidora-b38-*`の実行scratch logs。証拠対象ログは上のkeep済み記録へ複写後、Gio Trashへ移動し元パス不在とTrash metadataを確認 |
| hold | 0 | 保留資産なし |
| **合計** | **28** | keep 9 + store 0 + in-use 6 + trash 13 + hold 0 |

テスト用`/tmp/minidora-core-atomic-relation-comparison-38-*`ディレクトリはdrop cleanup後に0件。CARGO target/cache/toolchainは継続使用中の共有依存資産として保持した。
