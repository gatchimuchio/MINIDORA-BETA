# BUILD_BATCH_53 — exact condition-reference identity through Core

**BASE:** `fec00c44a0792d847236f7d9812021665b93ab20` (B52, `main` = `origin/main`, clean before B53 edits).

**復旧refs:** B53編集前にCAS付き回転を実施。`codex/backup-main`はB52、`codex/backup-main-prev`はB51を指す。B52/B51のコード・記録は変更していない。

**工程・ゲート:** 中核K1〜K7の不足是正。`CORE-G01=PARTIAL`、`CORE-G03=PARTIAL`を維持。他のCOREゲートは再判定しない。

**目的:** R2のRelation identityにあるcondition_refsをalpha10のCore直接型付き経路へ通す。条件参照の真偽評価や自然言語条件parserを新設せず、exact reference matchingと不一致停止を観測する。

## 実装したproducer → consumer

1. `Proposition.condition_refs`を`BTreeSet<String>`として追加。空setはserde出力から省略するため、alpha9までの未修飾Relationのcanonical bytesとidentityを維持する。非空集合は関係の正準identityの一部。
2. 型付き`assert_atomic_relation`、`add_atomic_relation_rule`、`compose_atomic_explanation`、`correct_atomic_relation`がProposition全体をsealed InputBundleへ渡す。CoreのIntentが同じPropositionをFact/Rule/AskWhy/CorrectPremise consumerへ流し、Store再読後も正確なcondition_refsを保持する。
3. `relations::validate`は条件参照を件数・長さ・空値・制御文字で制限する。ruleの全前提と結論は同一条件参照集合を必要とし、違いはCoreでContentPlan/render前に`SUSPEND`。proof matcher、rule composition、query、shortcut適用scope判定は完全一致する参照だけを消費する。
4. 訂正は完全なProposition identityで旧Factを選び、既存の依存失効経路へつなぐ。比較map/keyはcondition_refsを区別し、非空時は新しいversion付きidentityを使う。空条件で既存comparison key identityは維持する。Content/Explainと比較labelはopaque condition referenceを表示する。
5. alpha10の通常openはalpha7/8/9を拒否。明示migrationは現行のintegrity verifier後にmetadataだけを更新し、旧revisionのschema記録とimmutable blob hashを維持する。

## 隔離Core観測

`condition:dry`付きの`Nera is ore`と同じ参照の`ore -> refined ore` ruleから`Nera is refined ore`が`COMMIT / DERIVED / RENDERED`。条件参照が欠落したqueryおよび`condition:wet`のqueryはどちらも`SUSPEND / MISSING_PREMISES`でplan/renderなし。条件参照だけをdryからwetへ訂正すると元Factと依存導出が無効化され、Engineの再評価は旧dry結論を`SUSPEND / MISSING_PREMISES`として止める。`condition:dry`と`condition:wet`を持つAster/Beryl relation比較は共通扱いせず、両条件参照を片側ラベルに表示した。比較値とproof IDsは実行観測JSONとCoreテストログに保持。

## 限定確認

- `cargo test --locked -p minidora-domain unqualified_legacy_propositions_keep_their_canonical_bytes -- --nocapture`: **PASS**, 1 test。
- `cargo test --locked -p minidora-engine --test core_condition_scope_53 -- --nocapture`: **PASS**, 1 test。
- `cargo test --locked -p minidora-engine explicit_alpha -- --nocapture`: **PASS**, alpha7/8/9 migration tests, 3 tests。
- `cargo test --locked -p minidora-engine --test core_temporal_scope_51 --test core_temporal_correction_52 -- --nocapture`: **PASS**, 2 tests。
- `cargo check --locked --workspace --all-targets`: **PASS**。
- `cargo fmt --all -- --check`, `git diff --check`: final check **PASS**.

初回のmigration/all-target compileではinput parser testに不正なpattern fieldを機械追加していたため`E0532`。patternをbind-and-check形式へ修正して同コマンドを再実行、PASS。Core mismatchの最初のtest期待値はRust `Err`だったが、実契約はResultRecordの`SUSPEND`/`CONDITION_SCOPE_MISMATCH`だったためfixtureをCoreの観測値に合わせた。初回失敗と修正後ログはこのディレクトリに保持。

## 判定と残り

これはopaque typed referenceの完全一致経路に限る。Coreはcondition_refsが指す条件自体の成立やworld間での条件評価を確認しない。自然言語parser、否定・適用scopeの全幅、一般入力経路は未実施であり、`CORE-G01=PARTIAL`、`CORE-G03=PARTIAL`。他ゲートは再判定せず、CORE-G01..G07全件PASS前の入力機施工には進まない。

**安全・権限:** 変更はlocal repositoryと一時隔離test DBに限定。外部送信・外部作用なし。R4 3 Markdown、ZIP、manifest、R2は変更していない。alpha10のRust schema/version更新はこのRust successor内だけで、凍結Python betaは触れていない。
