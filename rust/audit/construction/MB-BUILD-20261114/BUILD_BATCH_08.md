# BUILD_BATCH_08 — 代名詞前提の一段継承深度制限

## 工事識別

- 対象リポジトリ: `gatchimuchio/MINIDORA-BETA`
- 作業ブランチ: `main`
- 着工時 HEAD: `c820916a78f3564f2fec518cb2b2f68787426204`
- 施工資料: `MB-BUILD-20261114` の三文書、保存済みZIP、`MANIFEST.tsv`。着工時に利用者添付ZIPと三文書のSHA-256をmanifestおよび記録済み原本と照合し、一致を確認した。資料は変更していない。
- 作業種別: Batch 07が報告した「型付き関係回答に続く一段だけ」の上限をCoreで実際に強制する。

## ⓪–④ 着工確認と単位定義

- ⓪ 権限と設計基準: 利用者の「続行」と採用済み三点セットに従い、リポジトリの`AGENTS.md`、施工仕様・工程・Codex指示、およびR2の意味・責任境界を再確認した。保存済み資料と、製品への実装指示を区別して扱った。
- ① リポジトリ: `MINIDORA-BETA` の `main`。着工時 `HEAD` と `origin/main` はともに `c820916a78f3564f2fec518cb2b2f68787426204`、worktreeはclean。
- ② 作業競合と復旧点: worktreeは一つで同時編集なし。`/workspace`には約53 GiBの空きがあった。着工時の`codex/backup-main`は`c820916a78f3564f2fec518cb2b2f68787426204`、`codex/backup-main-prev`は`1e84a3e83a2d9a76f30e067ca433b203d35570d5`。remoteの同名branch/tagも同じ世代だった。
- ③ 一単位: 直前の封印済み`AssertPreviousRelation`が既に別のcontextual assertion結果を継承している場合、三つ目の連続assertionをfact保存より前に止める。許される二つのassertion、次の通常質問、失効した元証拠による停止を確認する。
- ④ 境界: 入力文法・parser version・保存schemaを変えない。一般的な照応、長い連鎖、複文、外界の真偽、全要求の受入はこの単位の保証外。失効・欠落・深度超過での停止は正経路の完成とは数えない。

## 着工前の不一致

Batch 07の宣言境界と実装を照合した結果、Coreは直前のassertionごとに証拠を再検証していたが、連鎖深度を止めていなかった。公開済み着工時revisionの通常CLIで、規則・直接前提・型付き関係質問・三つの連続assertionを実行すると、6件すべてが`COMMIT`し、三つ目の「それは爬虫類です」も`USER_PREMISE` fact `fact:c6e435d528bbd34c122f6aca0a02876d7e2c94ea8ce4116d6286f38f781ea5b2`として保存された。修正前の実行、result ID、fact ID、依存を[着工前証跡](../../bb-context-pronoun-relation-assertion-depth-pre-fix-validation.json)に固定した。

## 実装

- `rust/crates/minidora-engine/src/core.rs`: 前のcontextual assertionの封印済み結果がactor memory内の直接`USER_PREMISE` factになっているかを記録する。現在の証明を再検証した後、その親assertion factが既にあるときは`CONTEXT_CHAIN_DEPTH_LIMIT`で停止し、`assert_fact`・`ContentPlan`・保存処理へ進めない。無効な証拠は従来どおり`CONTEXT_PROOF_CHANGED`で先に停止する。
- `rust/tools/contextual_relation_assertion_chain_test.py`: 許可される二段のchainと後続質問、証拠失効後の拒否、三つ目の保存境界、別問い合わせでの`MISSING_PREMISES`を通常CLIのfresh processで検査する。
- parser、schema、version、他の言語経路は変更していない。

修正後の[20観測経路](../../bb-context-pronoun-relation-assertion-depth-v1-route-validation.json)では、許可された二つ目は`COMMIT`し、三つ目は`CONTEXT_CHAIN_DEPTH_LIMIT`で`SUSPEND`する。三つ目に`ContentPlan`がなく、そのfactがmemoryに存在せず、別問い合わせも`MISSING_PREMISES`になることを確認した。前Batch 06の[49観測回帰](../../bb-context-pronoun-relation-followup-v1-regression-after-depth-limit.json)も通過した。

## 確認結果

- `cargo fmt --all -- --check` — PASS。
- parser focused test `singular_pronoun_relation_assertions_keep_subject_unresolved_until_core` — PASS、1件。
- legacy computation artifact focused test `previous_relation_meaning_reads_legacy_computation_artifact` — PASS、1件。
- `cargo check --workspace --all-targets --locked` — PASS。
- `cargo clippy --workspace --all-targets --locked -- -D warnings` — PASS。
- `cargo build --workspace --locked` — PASS。baseline比較後、B08版Coreを再適用して再buildした。
- Python AST parse — PASS。
- B08深度境界の通常CLI — PASS、20観測。Batch 06回帰 — PASS、49観測。
- `git diff --check` — PASS。
- `cargo test --workspace --locked` — **FAIL**。CLI unit testsは10 passed。`normal_route`は134 passed / 7 failedでexit 101となり、後続workspace test targetは実行されなかった。
- 失敗7件をB08 Coreだけ着工時のbaseline SHA `4d6804f964ac2bef0f4ec42a999589f4ca175de91d9e8553a1ffc1e6c682473d`へ一時復帰して個別再実行したところ、すべて同じfailure signatureで再現した。6件は既存テストがscopeなしの`適用規則:`を期待する一方、既存formatterがscopeを含める表示差。残るCSV statistics follow-upはJSON-table operation dispatchから`JSON_TABLE_CONTEXT_INVALID`で拒否される既存不具合。いずれもB08の深度変更では修正していない。[workspace実行とbaseline比較](../../bb-workspace-test-full-20261114-failure-baseline-comparison.json)に集約した。

## 保証境界と受入状態

この単位で確認したのは、同一ケースの型付き関係回答に続く一段の代名詞assertionと、その次の通常質問、および深度・証拠失効時の保存前停止だけである。一般的な共参照、長い連鎖、自然言語品質は評価していない。A04/A07/A08/A15/CAP-D04/CAP-D11の要件全体の受入状態は`UNVERIFIED`。workspace runtime suiteもFAILのため、全面合格とは扱わない。

## GitHub保持

この単位は`main`への通常fast-forwardで保存し、`codex/backup-main`を確定revision、`codex/backup-main-prev`を着工時HEADへ更新する。各local/remote branch/tagは、観測した旧SHAを使うcompare-and-swapで同期する。PR、別branch、worktreeは作らない。

## 資産区分

- 保持: 利用者原本4件（ZIPと施工三文書）、manifest照合済み・不変。
- 保存継続: 修正前の範囲不一致観測1件。着工後に書き換えず保持。
- 稼働: 本単位の8ファイル（Core、CLI観測ツール、修正後20観測、Batch 06回帰49観測、workspace失敗比較、実装状況、要件追跡、本報告）。
- リポジトリ資産の廃棄: 0件。B06/B07/B08の一時CLIログ・パッチ24件は対応する確定証跡を保存した後、`/home/agent/.local/share/Trash`へ移動した。無関係な一時ディレクトリには触れていない。
- 保留: A04/A07/A08/A15/CAP-D04/CAP-D11の要件全体の受入。6件とも`UNVERIFIED`。

本単位の参照資産は13件（原本4 + 修正前証跡1 + 稼働8）。先行Batchの保存資料は従来どおり不変。
