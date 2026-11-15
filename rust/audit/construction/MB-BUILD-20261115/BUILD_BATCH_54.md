# BUILD_BATCH_54 — Core relation scope and polarity boundary

**BASE:** `ed1063dc363672ea6c01d1f32da080e603368c16` (B53, `main` = `origin/main`, clean before B54 changes).

**復旧refs:** 変更前CAS回転を実施。local branch/tag `codex/backup-main` はB53、`codex/backup-main-prev` はB52。remote tagsも期待old SHAを指定したatomic leased push後に一致確認。

**工程・ゲート:** 中核K1〜K7の不足是正とCORE-G01の追加観測。`CORE-G01=PARTIAL`、`CORE-G03=PARTIAL`を維持。他ゲートは再判定しない。

**目的:** R2 Relation identityで区別されるpolarityとworld scopeを、B53 condition-ref alpha10以降の直接Core proof・render・correct-premise経路で観測する。自然言語parserや新input機は対象外。

## 変更前の不足と実consumer

B51/B52は同一scope内の時間差・訂正と主体control、B53はopaque condition_refsの完全一致・訂正を観測した。直前までのCORE-G01記録に、negative premiseがpositive ruleを発火しないこと、別world scopeがproofを共有しないことを一つの直接Core実行で記録した結果がなかった。現行production codeのexact shape matcherはsubject-template / polarity / scope / time / conditionをすでに比較するため、本batchはその実consumer接続を評価するfixtureを追加した。production semanticsは変更していない。

1. `Engine::add_atomic_relation_rule`へpremise world=`b54-lab-world`、conclusion world=`b54-archive-world`のruleを渡す。Coreは`SUSPEND`し、`SCOPE_MISMATCH`を返し、ContentPlan/renderを作らない。同scope版ruleは通常Fact/Rule memoryへ保存。
2. typed `assert_atomic_relation`でNeraのpositive ore FactとMiraのnegative ore Factを同じlab scopeへ保存。Neraのpositive premiseから同scope ruleが派生し、`DERIVED` ContentPlanへFact ID/Rule IDを保持。K7の`RENDERED` bytesはplan textと一致。
3. lab Factを使ったarchive queryは`SUSPEND / MISSING_PREMISES`。Miraのnegative Factもpositive refinement Ruleを満たさず同じ終端で止まる。Miraについてpositive oreを質問するとCoreはnegative Factの依存IDを保持した反対内容を採用し、planとrenderを生成。
4. `correct_atomic_relation`でNeraのpositive ore Factをlab scopeからarchive scopeへ移す。old lab Factと依存導出が失効し、再照会はplan/render前に停止。replacement Factはarchiveに保存されるがlab Ruleは移動しないため、archive refinementも停止。Mira negative Factおよびlab Ruleは有効なまま。

## 限定確認

- `cargo test --locked -p minidora-engine --test core_scope_polarity_54 -- --nocapture`: **PASS**, 1 integration test。観測JSONとproof IDsは[b54-build-observation.json](b54-build-observation.json)と[b54-scope-polarity-test.log](b54-scope-polarity-test.log)。
- `cargo check --locked -p minidora-engine --all-targets`: **PASS**。
- `cargo fmt --all -- --check`: **PASS**。
- `git diff --check`: **PASS**。

このbatchはproduction relation engineの意味を変更せず、直接型付きCore routeの実値と下流消費を拡張観測した。これは`CORE-G01`全成立述語のPASSではない。condition_refsの真偽/同一world評価、未知condition、時間・否定・複数主体・適用scopeの結合母集団、自然言語経路は未成立/未観測。`CORE-G01=PARTIAL`、`CORE-G03=PARTIAL`を維持し、CORE-G01..G07全件PASS前に入力機施工へ進まない。

**安全・権限:** 変更はRust後継内の隔離test fixtureと施工記録に限定。外部作用・送信なし。凍結Python betaとR4三点セット/R2は未変更。今回の許可は通常のmain commit/pushを含む継続施工であり、release・配布・正式受入宣言は行わない。
