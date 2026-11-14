# BUILD_BATCH_07 — 代名詞前提の一段継承

## 工事識別

- 対象リポジトリ: `gatchimuchio/MINIDORA-BETA`
- 作業ブランチ: `main`
- 着工時 HEAD: `1e84a3e83a2d9a76f30e067ca433b203d35570d5`
- 施工資料: `MB-BUILD-20261114` の3文書と `MANIFEST.tsv`。着工時にZIPと3文書のSHA-256がmanifestと一致した。元資料とmanifestは変更していない。
- 作業種別: 直前の代名詞関係前提を、次の代名詞関係前提が同一ケースで一段だけ継承し、さらに次の通常質問でその実 fact を消費する経路。

## ⓪–④ 着工確認と単位定義

- ⓪ 権限と設計基準: 利用者の「続行」と採用済み三点セットに従い、リポジトリ `AGENTS.md` と R2 の意味・責任境界を確認した。私有HDS理論、他リポジトリ、旧版の内部対応表は書込み・公開対象にしていない。
- ① リポジトリ: `MINIDORA-BETA` の `main`。着工時 `HEAD` と `origin/main` はともに `1e84a3e83a2d9a76f30e067ca433b203d35570d5`、worktree は clean。
- ② 作業競合と復旧点: worktree は一つ、同時作業者なし。`/workspace` に約53 GiBの空きがあった。着工前の `codex/backup-main` は着工時HEAD、`codex/backup-main-prev` は `015f2750243a974b2d49313560e511b00bf63d4a`。prev のローカル/remote branch と tag は旧 SHA を指定した compare-and-swap で着工時HEADへ進めた。push 後は最新と前世代を再配置する。
- ③ 一単位: `AssertPreviousRelation` の直後の一文だけを使う。先行入力の sealed result と source input が一致する USER_PREMISE fact を一意に特定し、現在も有効な同一 fact が唯一の直接証拠であることを再確認して主語・scope を継承する。次の質問が二番目の fact ID だけを消費することを確認する。別ケースの訂正で一番目の fact が無効になった場合は二番目を保存せず停止する。
- ④ 境界: 無制限の照応、二段を超える連鎖の一般保証、複文/複数主語、外界の真偽、全要求の受入は対象外。失効・欠落時の停止は正機能完成とは数えない。

## 実経路

成功側は、規則・直接前提 → 代名詞質問の型付き関係回答 → 一番目の代名詞前提 → 二番目の代名詞前提 → 通常の代名詞質問、の順で13件中6件を実行した。Core は一番目の sealed input/result identity に結び付いた直接 fact を取り出し、現在の actor memory で再問い合わせして、その exact fact ID と保証が一致した場合にだけ二番目を保存する。二番目の結果は直前 result ID と新 fact ID を持ち、次の質問は新 fact ID だけに依存する。

拒否側では、別ケースの訂正が actor 共通 memory の一番目の fact を失効させた。元のケースで続けた二番目の代名詞前提は `CONTEXT_PROOF_CHANGED` で `SUSPEND` し、`ContentPlan` はなく、別ケースの確認質問も `MISSING_PREMISES` で停止した。前のBatch 06 producer/consumer/recovery 経路も新しい Core のまま49件で再実行した。観測は [一段連鎖経路](../../bb-context-pronoun-relation-assertion-chain-v1-route-validation.json) と [Batch 06 回帰再実行](../../bb-context-pronoun-relation-followup-v1-regression-after-assertion-chain.json) に保存した。

## 変更

- `core.rs`: 直前の `AssertPreviousRelation` の sealed input/result identity と USER_PREMISE source identity を照合し、active direct fact のみを文脈候補にする。現在の関係問合せが同じ fact ID と `USER_PREMISE` を返さなければ保存を止める。
- `input.rs`: 既存の singular previous-subject 判定を crate 内検査で再利用できるようにした。入力文法、parser version、保存schemaは変更していない。
- `contextual_relation_assertion_chain_test.py`: 通常CLIを新規プロセスで呼び、成功・次 consumer・訂正失効後の拒否・未保存確認を検査する経路を追加した。
- 実経路証跡、実装状況、六つの要件行への限定追跡、この報告を追加した。該当要件の受入状態は `UNVERIFIED` のまま。
- Batch 05/06 の元報告・元証跡・元ZIP・三文書・manifestは変更していない。

## 確認結果

- `cargo fmt --all -- --check` — PASS。
- `cargo test -p minidora-engine singular_pronoun_relation_assertions_keep_subject_unresolved_until_core --locked` — PASS、1 passed。
- `cargo test -p minidora-engine previous_relation_meaning_reads_legacy_computation_artifact --locked` — PASS、1 passed。
- `cargo check --workspace --all-targets --locked` — PASS。
- `cargo clippy --workspace --all-targets --locked -- -D warnings` — PASS。
- `cargo build --workspace --locked` — PASS。
- Python AST parse — PASS。
- 一段連鎖の通常CLI経路 — PASS、13観測。前Batch 06経路の再実行 — PASS、49観測。
- `git diff --check` — PASS。
- 全workspace runtime suite — NOT RUN。

## 保証境界

これは同一ケースの隣接した代名詞関係 assertion 一段と、その次の質問に限る。任意の日本語/英語照応、非隣接の会話参照、無制限の反復連鎖、学習による一般化、現実世界の真偽は示さない。A04/A07/A08/A15/CAP-D04/CAP-D11 の要件全体の受入状態は `UNVERIFIED`。

## GitHub保持

本バッチは `main` だけへ通常pushする。push後、`codex/backup-main` branch/tag は最終 Batch 07 revision、`codex/backup-main-prev` branch/tag は着工時HEADへ、実測した旧SHAを指定した compare-and-swap でそろえる。PR、workflow、別branchは作らない。

## 資産区分

- 保持: manifest確認済み入力4件（利用者ZIPと施工3文書）、すべて不変。
- 保存継続: 先行Batch 05/06の報告と証跡4件、不変。
- 稼働: 本バッチ8ファイル（Rust 2件、新規経路観測1件、新規証跡2件、実装状況、要件追跡、本報告）。
- 廃棄: 0件。
- 保留: 要件全体の受入6件、`UNVERIFIED`。

ファイル資産合計: 16件（保持4 + 保存継続4 + 稼働8 + 廃棄0）。保留6件は未解決の受入義務であり、削除ファイルではない。
