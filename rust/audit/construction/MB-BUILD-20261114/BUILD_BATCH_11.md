# BUILD_BATCH_11 — 中核の直接施工確認二

## ⓪ 工事境界

- 対象: `gatchimuchio/MINIDORA-BETA` の Rust Core、三点セット第四工程「中核確認二」
- 目的: 複数前提の結合、矛盾、未成立条件、型付き反事実を区別し、根拠と保証を保持する。
- 入力: 合成された仮想labの二前提規則、直接センサー／較正 facts、別個の青色 fact、反事実の typed proposition。
- 実経路: `Engine::execute` → sealed input → actor memory → relation closure。反事実は `Engine::evaluate_counterfactual_relation` → actor所有case確認 → memory clone → 指定factとその依存だけclone内で無効化 → allow-listされた `HYPOTHESIS` premiseによる relation closure → typed report。
- 許可: 利用者が採用した三点セットの実装継続と、既存の「リポに保存」指示。変更対象は本repo内。隔離SQLite、Core API、合成入力を使用。
- 安全境界: 反事実評価は `&self` のread-only API。外部provider、network、file effect、content adoption、永続memory writeは行わない。通常の `relations::query` はhypothesisを引き続き除外する。
- 期待観測: 仮説を使う結論だけ `HYPOTHESIS`、rule・直接前提・仮説IDを依存へ保持。未指定の反対factは`CONFLICT`で停止。条件不足は`UNKNOWN`／通常経路の`MISSING_PREMISES`で停止。無関係factと保存memoryは不変。
- 停止条件: actor所有caseでない、型・scope・ground条件が不適合、明示差替えIDが直接かつ完全な反対前提でない、または上限予算超過ならtyped error/stop。自然文の反事実は通常入力で未対応なら成功扱いしない。

## ① 権威・同期

作業開始時に `origin/main` をfetchし、cleanな`main`、`HEAD == origin/main == 50a929d0c92a6583ecc23d66c2b24601ccf1420a`、既存二世代backup refs、単一worktreeを確認した。施工中はこのcheckoutだけを書き換えた。最後のcommit/push直前に再fetchし、同じ条件を再確認する。

採用済みBUILD文書とR2のtyped relation / hypothesis保証に従った。調査時点の欠陥は、既存relation queryが`HYPOTHESIS` premiseを除外しており、入力された仮説の保証を保った導出経路が無かったこと。これを観測だけで済ませず、直接Core construction APIをbounded実装してから観測した。

## ② 所有・容量

利用者は施工仕様・工程・施工指示と最終採否を所有する。Codexは許可されたCore実装、試験、hash、差分、Git同期を担当する。Gitは実装・失敗・観測の復元点を保持する。

反事実APIは1–8 assumptions、最大16個の明示override ID、`max_actions` 1–4096を受理する。観測は予算128、独立のactor/case、単一一時SQLiteで行った。memoryをcloneして局所評価し、Storeの事前・事後`MemoryState`を比較した。原本4件はmanifestと一致し、編集していない。

## ③ producer → consumer と実観測

19件のsealed requestを実`Engine::execute`へ投入した。二条件規則「sensorかつcalibratedならreliable」と、反対側の「sensorかつnot calibratedならnot reliable」を分けて登録した。条件不足は`MISSING_PREMISES`、成立した通常proofは`DERIVED`、反対の直接証拠を足した質問は`CONFLICTING_PREMISES`で停止した。Toviの未成立条件と別actorの入力は主actorの証明を変えなかった。自然文counterfactual requestは`CAPABILITY_UNAVAILABLE`、content planなしで停止した。

別actor/caseの保存済み`Nera is calibrated` fact IDを明示的にsupersedeし、仮定`Nera is not calibrated`から`Nera is not reliable`を評価した。reportは`SUPPORTED / HYPOTHESIS`で、規則・sensor fact・hypothesis IDを依存に保持した。clone内で無効になったのは指定較正factだけで、青色factは残った。actorの保存memoryは反事実評価前後で完全一致し、通常の再照会も`MISSING_PREMISES`のまま。hypothesisを保存済み`USER_PREMISE`へ昇格していない。反対のcalibrated factを指定せず残すケースはtyped `CONFLICT`、無関係factをoverride IDにする要求は`COUNTERFACTUAL_OVERRIDE_INVALID`だった。

すべての19件の実`ResultRecord`、typed report、前後memory、仮説・supersede・invalidated・dependency IDsは[直接Core観測証跡](../../bb-core-construction-observation-02-v1-validation.json)に保持する。B08の20件再実行とBatch 06の49件再実行はそれぞれ[assertion-depth回帰](../../bb-context-pronoun-relation-assertion-depth-v1-regression-after-b11.json)、[follow-up回帰](../../bb-context-pronoun-relation-followup-v1-regression-after-b11.json)に保存した。

## ④ 判定・残差

- `cargo fmt --all -- --check` — PASS
- `cargo test -p minidora-engine --locked` — PASS、231 unit testsと本施工integration test 1件。doc test 0件。
- focused direct Core test — PASS、19 sealed requests + 3 typed counterfactual API calls。
- `cargo check --workspace --all-targets --locked` — PASS
- `cargo clippy --workspace --all-targets --locked -- -D warnings` — PASS
- `cargo build --workspace --locked` — PASS
- B08 assertion-depth回帰 — PASS、20 observations。Batch 06 follow-up回帰 — PASS、49 observations。
- `git diff --check` — PASS。追加dependencyなし。full `cargo test --workspace --locked`、全93要件の受入、他の中核6観測は`NOT_RUN`／`UNVERIFIED`。

これは直接Core構築APIでのbounded observationで、通常の自然文counterfactual route、全体的な因果・反事実・類推推論、全R2 acceptanceを成立扱いしない。B11と全93要件の`acceptance_status`は`UNVERIFIED`のまま。自然言語入力からの仮説生成・差替え識別・質問条件の結合は次工程の残差として残す。

## 資産区分

- 保持: 利用者原本4件（BUILD set ZIP 1件、構成Markdown 3件）。`MANIFEST.tsv`のbyte数/hashと一致。
- 保存: 本単位で更新・追加した追跡ファイル9件（`core.rs`、`relations.rs`、integration test、Core観測JSON、二つの回帰JSON、requirements、implementation status、本報告）。
- 稼働: Cargo `rust/target` build output 1 set。生成物はGitへ追加しない。
- Trash: 初回fixture失敗ログ1件、最終pass観測ログ、engine/workspace検証ログ4件、失敗時の隔離SQLite scratch tree 1件を、各SHA-256／size／tree manifest取得後にLinux Trashへ移動。既存45件を保持し、今回7資産を追加。Trashは空にしていない。
- 保留: 0。full requirement acceptanceと残る6観測は後続施工範囲として記録。

## Git保存・復旧

施工開始時点は`50a929d0c92a6583ecc23d66c2b24601ccf1420a`。実装、試験、証跡、status、requirements、原本manifestをdiff/hash確認後、`main`へcommit/pushし、`codex/backup-main`と`codex/backup-main-prev`のbranch/tag二世代を更新する。PR、release、他repo変更は作成しない。
