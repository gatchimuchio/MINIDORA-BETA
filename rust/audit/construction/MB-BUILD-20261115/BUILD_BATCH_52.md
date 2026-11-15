# BUILD_BATCH_52 — exact temporal premise correction through Core

**BASE:** `9d1c59296e9cb4cfddd985a96bc06ccdc3e682cc` (B51, `main` = `origin/main`, clean before edits).

**工程・ゲート:** 中核K1〜K7の不足是正。`CORE-G01=PARTIAL`、`CORE-G03=PARTIAL`。本batchでは他のCOREゲートを再判定しない。

**目的:** B51でCoreの型付き関係・rule・proof queryへ完全一致`temporal_scope`を通した後、時刻の訂正も同じ直接Core経路でsealed input→依存失効→再照会へ流す。自然言語parserや新しい入力機は対象にしない。

## 実装したproducer → consumer

1. `Engine::correct_atomic_relation`が、旧/新`Proposition`と通常のrequest identityを受け、canonical JSONとして`CORE_TYPED_RELATION_CORRECTION_V1`のsealed `InputBundle`を作る。置換前提はpositive・ground・同じsubject/predicateに限定し、object/world scope/timeは明示値で変更可能。自然文をparseせず、自由文を受け取らない。
2. sealed `Intent::CorrectPremise`は既存Core correction consumerで再検証され、完全一致するactive direct `USER_PREMISE`だけを対象にする。`relations::invalidate`がそのfactと依存するrelation recordsを無効化し、旧否定factとreplacement factを追記し、memoryを一つの新revisionとして保存する。原recordは削除しない。
3. Engineを再openし、通常のtyped explanation consumerで同じ旧時刻質問と訂正後質問を実行。旧時刻の派生経路は`SUSPEND / MISSING_PREMISES`でplan/render前に止まり、新時刻replacementは`COMMIT / USER_PREMISE`。別時刻/別主体controlsは旧依存を消費せず、従来どおり成立。

隔離fixtureでの実値：08:00の`Nera is ore`＋同時刻rule→`Nera is refined ore`が`COMMIT / DERIVED / RENDERED`し、両proof IDがResultRecord/ContentPlanへ入った。旧前提を12:00へ訂正するとmemory revisionは4→5、旧factは`valid=false`、18:00の同主体factと08:00の別主体factは`valid=true`を維持。再起動後、08:00派生問は`SUSPEND / MISSING_PREMISES`（ContentPlan/renderなし）、12:00 replacement queryは`COMMIT / USER_PREMISE`、18:00の同主体queryと08:00の別主体derived queryは`COMMIT`した。観測詳細は[b52-build-observation.json](b52-build-observation.json)、実行ログは[`b52-focused-tests.log`](b52-focused-tests.log)。

## 限定確認

- `cargo test --locked -p minidora-engine --test core_temporal_correction_52 -- --nocapture`: **PASS**、1 test。
- `cargo test --locked -p minidora-engine --test core_temporal_scope_51 --test core_construction_observation_07 -- --nocapture`: **PASS**、2 tests。
- `cargo check --locked -p minidora-engine --all-targets`: **PASS**。
- `cargo fmt --all -- --check`: **PASS**。
- `git diff --check`: **PASS**。

初回の`cargo fmt --all`呼出しは実行shellのPATHに`cargo`がなくexit 127となった。コードは実行されず、失敗出力を[`b52-initial-format-path-failure.log`](b52-initial-format-path-failure.log)へ保持。固定toolchain `1.99.1`のbinをPATHへ明示した再実行とformat checkはPASS。環境と試験記録は[構造化観測](b52-build-observation.json)に保持。

## 判定と残り

今回の直接型付きfixtureは「時間変更→完全一致の旧premise invalidation→後続派生結論の再判定」を接続した。条件参照型の独立field、異なるworld scopeでの比較、独立した否定query、幅広い初見入力の成立は観測していない。よって`CORE-G01=PARTIAL`を維持し、自然言語時刻parserや時系列/interval演算を追加したとは扱わない。`CORE-G03=PARTIAL`も維持。G02/G04/G05/G06/G07、通常入力機、T01〜T03、全体系QC/QAは本batchで再判定/実施していない。入力機施工へは移行しない。

**安全・権限:** 変更はlocal Core memoryとisolated temporary test DBに限定。外部送信/外部作用なし。実行時の一時DBはtest cleanup対象。R4三Markdown、ZIP、manifestとR2は不変。R4 manifestの4つのSHA256は施工前の値と一致。
