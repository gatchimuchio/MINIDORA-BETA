# MINIDORA-BETA｜Codex施工命令 D2-LLM

`ID=MINIDORA-ORDER-JP-20261117-D2-LLM`／`仕様=01_施工仕様書.md`／`工程=02_施工工程表.md`／`作業要領=repo:/AGENTS.md`／`対象=MINIDORA-BETA/rust`／`起点HEAD=9bd02c1e3791d574ef4e414c984d8eab6f21870c(2026-11-17監査時;着工時再取得)`／`実装者=Codex`／`設計/整合=チャッピー`／`採否/許可/公開=オーナー`／`状態=D2現行施工指示`。本書は施工命令であって新規権限・実PASS・リリース証拠ではない。

## O0：最上位機械命令

1. `SUBJECT=唯一のEngine/Core`；`GOAL=原文→Input封印→Core{元Goal/意味/関係/根拠/推論/記憶→適応/新内容/新文面/許可作用/採否}→Output実byte/作用→実観測receipt→同Core元Goal判定`。非Transformer/非ニューラル、日本語意味基底。言語模型/入力機/出力機/専門port/UI/Rust/LLM/テストを主語へ昇格禁止。
2. `ORDER=E0→E1{CORE_SHAPED}→E2{CORE-G01..07 PASS×7 同一版}→E3→E4→E5{T01..03 PASS×3 通常入口同一版}→E6{SYSTEM_SHAPED,全93+全source}→E7{体系的QC}→E8{独立QA}→OWNER_GO`。先行段未成立で次段の**新規増築**禁止。既存正機能/歴史は保全。工程のW番号は**一台のCoreを組む依存順**、Wごとの独立製品完成/品質合格プロジェクトではない。
3. `EVIDENCE=実producerの実中間値を実consumerが引数として使用→実出力/失敗→元Goalの必要述語が更新されたこと`。`反例=source存在,struct/port追加,unit test本数,HTTP200,固定挨拶,既成文連結,単独SUSPEND,局所PASS積算,旧Python313/GPQA点数`。それらを`CORE_SHAPED/GATE_PASS/T_PASS/SYSTEM_SHAPED/QC/QA`へ自動昇格禁止。
4. `不変=R2既存93要件+新発見要件/元source-test全数/元Goal主語・目的・手段・条件・版/安全・停止・回復/公開境界`。`R2第20節改訂=全source意味照合完了はE0事前でなくE6末;要求水準・対象は削除しない`。歴史R2/R4/D1/B01..B56は当時の版と証拠として不改竄。
5. `許可=ユーザー現在指示に限定`。編集許可のある本repoのRust後継/新版契約・検査・証拠のみ。元`NOTNN-LLM-MINIDORA`、他repo、凍結Python`src/`・`minidora-notnn==0.6.0b1`・旧ZIP/wheel・`SOURCE_LOCK.md`・`REPOSITORY_LOCK.md`・`RELEASE_MANIFEST.json`・`checksums.sha256`・秘密/HDS非公開理論を変更/転載禁止。`main`一本運用、PR/new branch/CI/tag/force push/reset/外部配送/Cloud配備/課金を自発生成禁止。commit/push・外部作用は今回の許可とAGENTSで判定。
6. `STOP_LOCAL_IF={設計正本衝突,元要求縮減,Core責任の違法移転,書込競合,未認可送信/破壊/不可逆作用,秘密/個人情報漏洩,外部効果UNKNOWN}`。影響する作用だけ停止、認可済み独立の読取/診断/施工は続行。上位仕様変更は設計者/オーナー採否へ送る；通常実装内の根因修正は完遂。機械の動機や感情に原因帰属しない。
7. `報告=観測結果のみ`、日報・チケット・別監督・重複証拠帳票を追加禁止。`未実装≠未接続≠未検査≠権限待ち≠FAIL`。再評価した現在版でのみPASSを更新。

## O1：E0 現HEAD／現物／権限を再同期せよ

`E0-W01` 読取：`pwd;git status --short;git rev-parse HEAD;remote/main;AGENTS.md;本D2仕様/工程/指示と採用索引;R2;D1/R4歴史;docs/redesign/{02_CAPABILITY_REGISTER.md,03_REQUIREMENT_TRACE.tsv,04_SOURCE_CENSUS.tsv,05_VERIFICATION_AND_EVIDENCE.md,06_LINEAGE_AND_DECISIONS.md,07_R1_AUDIT_CLOSURE.md,08_WIRE_SCHEMA.json,09_TEST_CENSUS.tsv}`。既存dirty/未push/別writer/未確定作用を保全。`rust/{Cargo.toml,Cargo.lock,rust-toolchain.toml,README.md};rust/docs/{IMPLEMENTATION_CHOICES.md,DEPENDENCIES.md};rust/audit/{requirements.tsv,source_contract_reviews.tsv,IMPLEMENTATION_STATUS.md}`の現物値を観測。`Rust 1.99.1/edition2024/0.7.0-alpha.10`は起点情報であり現物優先。`cargo/rustc/OS/network`の可否を取り、無ければ`NOT_RUN:TOOLCHAIN_UNAVAILABLE`等を記録。環境整備の無限反復禁止。

`E0-W02` 実物を追え：`rust/crates/minidora-domain/src/lib.rs{InputBundle,MeaningIR,GoalContract,ContentPlan,RenderObservation,DeliveryReceipt,CaseRevision}`、`rust/crates/minidora-engine/src/{core.rs,relations.rs,store.rs,scheduler.rs,language.rs,recovery.rs,input.rs,output.rs,ports.rs}`、`rust/apps/cli/src/main.rs`、関連通常入口・test。既存B49証明保存型G05、B56二前提/反事実G02を**限定成功として再利用**；G01/G03 PARTIAL、G04/G06/G07総合未判定、T未PASS、K4の既成文候補選択/要求内一時訓練、Goal固定述語/実効果照合不足を調べ、実不足のみ施工。E0で93全行受入/元414逐行意味レビュー/全workspace QC/外部LIVEをしない。`E0_EXIT=単一Core/実値契約/既存再利用/着工権限/重大衝突なし`。衝突は対象のみ上流採否へ。

## O2：E1 一台のCore全体躯体を結線せよ（W01..W19）

`E1_EXIT=CORE_SHAPED`；同一Engineで`循環A:初見原子意味→K1/K2/K3→K4新内容+六要素新文面→K7採用→元Goal実成果照合`と`循環B:観測/訂正→不変記憶→推論→適応→後続別要求のK2/K4/K6実consumer`を正常・不足・停止の実状態で結線。工事治具は非判断型付き、正解生成の別Core禁止。Wは実施工の内部依存点であり**個別全QAを繰返さない**。

| W | 現物・操作命令（不変・禁止を含む） | 実consumer／完了観測 |
|---|---|---|
| 01 単一writer/ID | `domain/lib.rs;core.rs;store.rs;recovery.rs`。actor/case/input_id+version/source_revision/goal_revision/memory_revision/model_revision/plan_id/effect_action_idを衝突なく保有。`CaseRevision`単一writer・不変原本・版付き原子更新。異actor/許可/原文同ID衝突拒否。重複公開型/無根拠改名を避ける | W02..W18が**同一case revision**を読む；保存器・Output・HTTPの独自Core更新不可 |
| 02 工事用型付き入口 | 既存`MeaningIR/InputBundle`を使用。`raw_id/meaning_id/actor/case/subject/predicate/object/polarity/scope/condition/temporal/provenance/goal/permission/budget`相当の概念を非判断性治具から本番Coreメソッドへ。新struct全項目追加を義務にしない | W03..W18へ実値。完成文/expected_output/正解fixture/第二判断器を通常Coreに入れない |
| 03 Goal固有述語 | `core.rs;scheduler.rs;Goal graph`。固定`SUPPORTED_CONTENT/EXACT_RENDER/CONFIRMED_FILE`だけの述語を改め、元指示の説明/比較/文章/適応/数値/ファイル/送達/複合目的ごと`requirement_id,必要値,依存,実producer,consumer,禁止,権限,証拠版,充足条件`を版固定。未知は保留 | K6が未充足から次動作、K7が実成果でのみ充足。表示≠ファイル≠外部配送；禁止作用を成功へ変換しない |
| 04 K1意味評価 | `domain/lib.rs;core.rs;relations.rs`。既存`condition_refs/temporal_scope/scope/polarity/SealedSourceEvidence`を使用し、条件の定義/真偽/未判定/主体/期限/依存を実評価。主語/述語/目的語/否定/例外/量化/時制/帰属/引用/仮説/範囲を分別；未支持照応はUNKNOWN | 条件真偽・主体・時点・引用だけを替え、K2推論/K3採否/K4文面/K7に因果差。文字署名差だけ不可 |
| 05 K2関係 | `relations.rs;core.rs`。B56既存二前提/証明IDを再用。取得/結合/型付き連鎖/scope交差/条件評価/矛盾/欠落/反事実別scope/訂正失効を実計算 | `conclusion_meaning_id,proof_ids,scope,conditions,source,revision,unmet/conflict`→K3/K5/K4。文字一致による採用・反事実本状態混入禁止 |
| 06 K3採否 | `USER_PREMISE/OBSERVED/DERIVED_PROOF/HYPOTHESIS/CONTRADICTED/OUT_OF_SCOPE/UNKNOWN`相当を実enumへ対応。出所数でなく有効根拠/主体/条件/時点/禁止/保証で採否；資料の引用と確定知識を分別 | 版・根拠ID・不足・適用範囲付き`AdoptionDecision`→K4許容集合/K6必要資料/K7採用。検索/Output独自採否禁止 |
| 07 K4意味内容生成 | `language.rs;core.rs;compose_typed_meaning;ContentPlan`を調査。完成済み`unit.text`を入力とせず、初見原子命題+複数関係+元Goalから新主張/説明順/比較軸/条件短文/計算解説/有用部分回答の**意味単位**をCoreが構成 | `meaning_plan={claim,proof,limitations,dependency,format,required_units}`→W08/W09。既存文面shuffle・固定接続句だけを成功扱い禁止 |
| 08 K4非ニューラル新文面 | `language.rs:SequenceModel/train_until/learn/realize_supported_until;core.rs`。`TOKEN_SPACE→CONTEXT_STATE→PARAMETERIZED_TRANSFORM→CONDITIONAL_NEXT_TOKEN_DISTRIBUTION→SEQUENCE_OBJECTIVE→DECODING_IF`を本番同一Coreの連続実値に接続。意味計画→許容文法/表現候補→状態条件付分布→有限選択/EOS→意味逆照合→W09 | 初見新意味から全文列挙にない実文面。model_revision/artifact_ref/分布/状態/seed/停止理由を追跡；外部LLM/Gold/暗記/単なる既成文候補選択禁止 |
| 09 K7意味採用 | W07新意味計画×W08新文面×K3根拠/否定/数量/帰属/出所を照合。`ContentPlan.required_units/dependencies/model`へ実計算値を封印。不足は理由と再取得/再生成/部分保留 | 採用内容→Output、未充足→K6。`CONTENT_ADOPTED≠RENDERED≠DELIVERED≠GOAL_COMMIT`。Outputの新事実補充禁止 |
| 10 K5不変記憶 | `store.rs;core.rs;recovery.rs`。原文/bytes/span/観測/根拠/作用/失敗を不変保存、採否/訂正/失効/依存index/cache/要約/model適応は別版。Goal関連subgraphを有限に選択、原本へ遡れる | K2/K3/K4が実保存値を後続消費。同Core寿命、fresh process、別session/モデル版で継承；件数や検索ヒットのみは不可 |
| 11 K5証明保存学習 | 既存`core_proof_composition_learning_49.rs;core.rs;relations.rs`を再使用。型/変数/scope/条件/証明集合付き規則をCore合成、再起動で再適用。B49結果は旧版・限定 | 学習前の多段→後続未見別主体にshortcut→scope外停止→証拠失効で依存のみ取消。K2/K4/K6へ**実使用**；別Learner増築禁止 |
| 12 K5観測起点仮説 | 出所/標本/時点/反例を保持→Coreが条件付き仮説形成。事前`母集団/範囲/採否規則/期限/反証`なければ`HYPOTHESIS_RETAINED`、契約済み限定のみ`EMPIRICAL_SCOPED_ADOPTED`。矛盾で影響分だけ失効 | K3の断定範囲/K4文言/K6行動へ版付き実使用。G05証明保存型PASSでもE6の経験的CAP-D要求を削除しない |
| 13 K5→生成の後続適応 | `SequenceModel::learn`と要求内一時`compose_typed_meaning`の生産Core未接続を是正。適応元/イベント/有効条件/依存/モデル差分/seedをCore記憶で管理 | snapshot A→学習→B→**未見別要求**で候補/結果/経路に有意差、A復元ならA再現、scope外誤適用なし、証拠取消でBの依存のみ失効、fresh process B継承 |
| 14 K6元Goal駆動 | `core.rs;scheduler.rs;Goal graph`。W03の未充足必要成果→次の有限意味/資料/専門計算/内容/許可作用→実中間値→必要述語更新。各nodeにproducer/consumer/result_id/permission/time/cost | K7へ実結果。計算した5→説明で5を消費、Web本文→根拠比較、適応→新問題計算。変数名更新/同検索反復/メモリ増だけで進捗や予算増禁止 |
| 15 作用・権限・UNKNOWN | `ActionRecord;EffectPhase;DeliveryReceipt`を保全。`PURE/READ/LOCAL_WRITE/EXTERNAL_WRITE/IRREVERSIBLE`、対象/actor/許可/入力hash/action_id/idempotency_key/費用/receipt。`PROPOSED→AUTHORIZED→PREPARED→ATTEMPTED→CONFIRMED/FAILED/UNKNOWN` | K7が実receiptを照合；UNKNOWNは再読取優先・無確認再送禁止。E1で許可のない外部送信/課金/実削除/公開しない。取消の補償は新作用 |
| 16 K7成果照合 | `core.rs;output.rs;store.rs;recovery.rs`。`ContentPlan→render→RenderObservation→実bytes/hash/target/renderer_version照合→Goal graph`を既存から接続。依頼が表示+ファイルなら両実観測必須 | `SEMANTIC_SUPPORTED→CONTENT_ADOPTED→RENDER_CONFIRMED→(必要時)DELIVERY_CONFIRMED→GOAL_COMMIT`を別状態評価。未送達/不明/取消は非偽COMMIT |
| 17 予算/停止/復旧 | Input/関係/生成/資料/学習/作用の有限budget/deadline/停止位置。actor/case/input_version/action_idを再開時照合。訂正は原文/Goal/根拠新版、旧原本と失敗・誤採否を保持 | 無進捗反復停止。中断後、既作用の二重実行なし、必要な計算から復帰。UNKNOWN/破損/取消を有限終端 |
| 18 **同一Engine本番接続** | W01..17を同じ`Engine`の本番Core入口へ統合：封印→Goal/K1→K2/K3/記憶→K5適応→K4意味/L6文→K7採用→K6必要作用→実観測→元Goal判定。論理循環であり関数一列を強制しない | **一台**で上記循環A/Bの実中間値が後段に入る。結果`COMMIT/SUSPEND/REJECT/FAIL/CANCEL`の根拠あり。**CORE_SHAPEDは形状のみ** |
| 19 BUILD中検査 | 変更Rust構文/型・改修した実結線smoke・保存互換・安全/権限/重複/秘密/復旧の必要負例に限定。各Wで全workspace/全母集団/独立QAを反復禁止 | 引継ぎはbase/現SHA・変更した実値→consumer・根因・次先のみ。日報/チケット/別監督/全台帳再作成禁止 |

`E1_STOP=主語や責任主体が入力/出力/外部portへ移る,Goldや完成文でK4を代用,未確認外部作用,元Goalの暗黙改変,不可逆な旧履歴消去`。該当因果を直せ。`E1_NEXT`は上記Core実循環が成立した場合だけE2；GATE全PASSとは別。

## O3：E2 同一Core七ゲート合同試運転

`E2_PRE=E1.CORE_SHAPED`。工事治具の初見入力は`Coreに渡すのは原子意味/条件/根拠/Goal/許可だけ`、完成回答/expected_output/Goldは隔離。`MEASURE={Git SHA,Rust/schema,Engine構成,store snapshot,model revision,evidence版,権限,予算,seed,入力,実producer,中間値,consumer,採否,実結果,根因}`。各ゲートは必要正例+反例で因果確認；状態`PASS/PARTIAL/FAIL/NOT_RUN/BLOCKED_AUTHORITY/UNKNOWN`。

| G | 正例+反例の施工検査 | PASS述語/失敗回帰先 |
|---|---|---|
| `CORE-G01` | 未見主体二組、否定、条件真/偽/不明、時点内外、引用vs命令、出所だけ差 | 依存する推論・新文面・採否のみ適切変化。条件ref存在だけ不可。W03/W04 |
| `CORE-G02` | 二前提導出、前提一欠落、矛盾同居、disposable反事実、無関係fact | proof ID付きで導出/不足/矛盾/仮定を分別・主状態不変。B56旧結果のみ不可。W05/W06 |
| `CORE-G03` | 完成文のない初見原子関係説明/比較/条件文/指定書式、事前回答列挙なし | W07新意味→W08六要素による新系列→W09支持採用→K7実consumer。既成文並替え/外部模型不可。W07..09/W13 |
| `CORE-G04` | 実計算/根拠/訂正保存→別要求で実使用→訂正依存失効→fresh process | 原本/旧採否保持、実依存する部分だけ新結果。件数増/固定再生不可。W10..13 |
| `CORE-G05` | B49型合成の学習前後・未見別主体・scope外・根拠撤回・再起動；経験系は事前採用契約別評価 | Core内記憶→推論→適応が次問題へ因果的に作用し安全に失効。単なるモデル更新/同問題再生不可。W10..13 |
| `CORE-G06` | 元Goal全充足/一成果欠落/無許可/取消/期限/実効果UNKNOWN/可逆実ファイル | 要求固有必要値+実receiptだけでCOMMIT。禁止・未知・未達は非偽COMMIT、再送なし。W03/W14..17 |
| `CORE-G07` | 同初期状態fresh復数、条件/根拠/model版を各一箇所変更 | 同条件で意味/根拠/採否/生成選択/作用計画一致、版差原因を再現。表示一致だけ不可。W01/W08/W10/W17 |

`GATE=同一SHAの7件全成功述語PASS`；混在コミット/以前のB49限定G05/B56限定G02の足し算禁止。G01/G03当時PARTIAL、G04/G06/G07総合未判定。FAIL/PARTIALは根因W修理→同一新版で影響ゲート+共通Core再検査。E2 PASS×7のみE3移行。`CORE-Gxx`とR2検索要求`C01..C13`混同禁止。

## O4：E3/E4/E5 最小三機接続と通常通電

`E3_PRE=E2 PASS×7`。対象`engine/src/input.rs;domain/src/lib.rs;core.rs;CLI`。既存`input::seal/MeaningIR/InputBundle/source span/semantic signature`を使い、`通常日本語原文→原bytes+正規化差保全→出所/主語/否定/条件/時点/版/Goal/許可/照応→sealed→同Core`。入力機独自の真偽/学習/完成文禁止；外部本文の偽命令は非権限。最小検査入力={初見説明,初見比較,先行実`ResultRecord`追問,未見適応,訂正新版,曖昧主語/無許可/不明文法}。`E3_EXIT=raw→sealed→same Engine`と失敗理由の保持。全言語文法/全媒体はE6へ、既存機能は退行禁止。

`E4_PRE=E3成立`。対象`engine/src/output.rs;core.rs;domain/src/lib.rs;CLI/HTTP表示`。`K7 ADOPTED ContentPlan→実UTF-8 byte/必要な可逆ファイル→RenderObservation{plan_id,renderer_version,bytes,actual_sha256,status}→Core同版照合`。要求ファイルは許可範囲に書き、実読戻し。要求ない不要ファイル作成禁止。外部送達は別`DeliveryReceipt`、未確認送達と描画を混同禁止。失敗/文字欠落/対象違い/取消/UNKNOWN/拒否をCoreへ渡しGoal判定が実変化。`E4_EXIT=実結果と失敗を同Coreが実消費`。PDF/画像/音声/動画全媒体はE6。

`E5_PRE=E2 PASS×7 ∧ E3 ∧ E4`。入口＝実CLIまたは実HTTP/browserの**通常利用者原文入口**（工事用API/内部fixture不可）。

| T | 実施 | 期待実consumer/成立述語 |
|---|---|---|
| `T01` | 完成文未保存の初見新意味説明/二対象比較/条件文を原文で入力 | Input→K2/K3→K4新意味＋新文面→K7採用→実表示byte/receipt→元Goalの正しい充足。定型文/単純shuffle不可 |
| `T02` | 通常原文の学習前→規則/経験形成→未見別主体追問、scope外、訂正、再起動 | K5適応版が次要求K2/K4/K6を実変化→実表示。漏洩/無根拠汎化/固定回答再生不可 |
| `T03` | 安全な許可表示/実ファイルを指示→読み戻し；別caseで未達/取消/効果UNKNOWN/無許可 | 元Goal固有述語→実Action/Output→receipt→同Core終端。確認された必要成果のみCOMMIT、偽COMMIT/二重作用0 |

`E5_GATE=通常入口の同一Rust版でT01..03全PASS`。工事治具/HTTP200/定型挨拶/内部ActionResult/安全停止だけは非代替。失敗部位＝Input→E3、Core→E1/E2、実出力→E4。`T全PASS前の新専門モジュール増築禁止`。

## O5：E6 全93＋追加要件／source全数／専門能力を同一Coreへ施工

`E6_PRE=E5 T01..03 PASS×3`。各行を**施工命令＋実consumer**として扱う；IDの原義は`docs/redesign/02_CAPABILITY_REGISTER.md;03_REQUIREMENT_TRACE.tsv;R2`と照合。表現圧縮は要求削除ではない。`CORE-Gxx`は検査ID、検索要求`Cxx`は要求ID、`CAP-Dxx`は能力で`ARCH-Dxx`と別、`F01..F14`製品要求と`R1 F01..F22`反証も別。

`E6-W01` 母集団を物理確認：`Req=93{A15,B13,C13,CAP-D14,E12,F14,G12}+新発見(出典/理由/別ID)`；`src=元414,β416`；`test=元359,β147,union360`；`旧Pythonβ選抜=313`。すべて別分母。`docs/redesign/04_SOURCE_CENSUS.tsv;09_TEST_CENSUS.tsv;rust/audit/{requirements.tsv,source_contract_reviews.tsv,implementation-obligations.md,IMPLEMENTATION_STATUS.md}`を対照。D1起点では`full_reviewed=98/416`、残`318`（資料中96表記との不一致は原pathで照合）。現時点の実物が違えば再計数し由来を保持。全sourceを**内容まで読取**、各件`{path,SHA,旧入口/引数/演算/意味/正常成果/失敗,現所有機械,実producer,実consumer,等価証拠,処分}`。`disposition∈{RETAIN,PORT,MERGE,REPLACE,NOT_APPLICABLE[承認理由必須],UNMAPPED}`。単なるパス/静的import/名前一致を継承と扱わない。`各req={入口,producer,consumer,出力/実作用,正常/負例,implementation_disposition,acceptance_status,対象SHA,未解決}`。`implementation_disposition`と`acceptance_status`は別；D1起点の全93`UNVERIFIED`を実装数でPASSにしない。**E6末尾で全数意味閉包**（R2第20節の採用改訂）。

`E6-W02` 下記93の実装位置/現型を最初に探索→既存正機能再使用→足りない実結線のみ施工→実producerの値を同じCore/Goalが消費→状態と未達を記録。表は`ID|実施工→下流実consumer`；元要件の文言・適用全幅はR2原典で照合し、狭い例だけに置換禁止。

### A：言語・意味・会話（15）

| ID | 施工→実consumer |
|---|---|
| A01 | 非ニューラルCore L6/隠れた外部NN排除→本番回答の実model値 |
| A02 | 有限token/系列/文脈を初見関係へ→要求別固定テンプレ外の新文面 |
| A03 | 条件付き分布/決定的タイブレーク/model revision→同条件再現・版差因果 |
| A04 | 日本語語/節/主体・目的語/省略復元を根拠付き→未支持主語はUNKNOWN、他actor混入なし |
| A05 | 否定/例外/量化/様態/時点/scopeを意味→出力へ保全→同義/反転対の適切な実差 |
| A06 | 定義/修飾先/関係節/出所span→未見記事の条件保持・原文照合 |
| A07 | 会話の実回答/照応/訂正参照→`2+3`後の「それを詳しく」で実計算5を使う |
| A08 | 長会話のtopic/Goal/format/style/evidence版→話題変更/復帰/訂正後も旧Goal混線なし |
| A09 | 型付き直接/導出関係・依存/矛盾→複数資料の語一致を含意へ偽変換しない |
| A10 | 論説の対象/前提/根拠/推論/結論/適用範囲を関係演算→対応反論・論点ずれ判定 |
| A11 | 日英中翻訳で原文意味/否定/受身/条件/主体/損失→訳の意味同値と差分 |
| A12 | 未知/不足を型付け、支持済み部分は保持→有用部分回答+具体的未充足 |
| A13 | 未見what/why/how/compareの通常原文→K4の実質自然回答（挨拶定型不可） |
| A14 | 実回答の説明深度/例/換言/引用/形式→直前の実計算・実文面を根拠付き再生成 |
| A15 | 原文seal→全三機で同一MeaningIR版→下流の無断再解釈なし |

### B：課題解決・専門計算（13）

| ID | 施工→実consumer |
|---|---|
| B01 | 初見要求の目的/成果/制約でCoreルーティング→適切portの実値・未知理由 |
| B02 | 算術/単位/数量/有理数/安全な記号→正しい実結果を説明/訂正へ |
| B03 | 代数/一次方程式/統計/確率/数式表記→数値・前提・計算手順を実文面へ |
| B04 | 表/JSON/CSV/文書の比較/集計/変換/検査→実行位置・警告付き成果 |
| B05 | 複数資料の独立/一致/反例と根拠合成→Coreが実取得本文の関係を比較採否 |
| B06 | 多段課題の依存実値→先行結果を後続実引数とし元Goalへ |
| B07 | コード生成/解説/修正/有限実行診断→未見要求の成果と実行/未実行を区分 |
| B08 | 執筆/編集/要約/制約付き書式→未見文章12件の目的・長さ・含意・形式 |
| B09 | 科学/専門/数学知識の出所付き利用→Core単体と専門付きの保証分離 |
| B10 | 候補生成→根拠比較→採否/保留/訂正→候補別理由・Gold遮断 |
| B11 | 因果/仮定/類推の前提・scope→反事実の対象依存だけ変更・本状態不汚染 |
| B12 | Goal許可/前提/予算から行動計画→実行による状態差・目的充足 |
| B13 | 検索+計算+文章などを同一Core Goalへ→能力間実値連結、第二採否者なし |

### C：検索・外部根拠（13、CORE-Gとは別）

| ID | 施工→実consumer |
|---|---|
| C01 | 許可別Web/局所検索→検索一覧だけでなく**実本文**をCore証拠へ |
| C02 | provider別error/timeout/rate-limit/retry/budget→失敗位置と資源 |
| C03 | URL/取得時刻/版/hash/頁/タイトル/原byte→資料差を別根拠に |
| C04 | search-list packet/本文packetを不変分離→status/retry/提供順の再生 |
| C05 | 固定packet診断≠正式LIVE評価→診断PASSをLIVE点数へ混入禁止 |
| C06 | 根拠の支持/独立/反例/文字重複を区分→実根拠差に従う採否 |
| C07 | Wikipedia/論文Crossref/Europe PMC等/専門局所portを対象別接続→実到達/失敗/出所 |
| C08 | HTML主本文/見出し/ルビ/不可視文字抽出の境界→span/警告付き原本文 |
| C09 | 有限PDF**文字抽出**/頁/警告、OCRは別範囲→不能時の正しい保留 |
| C10 | 取得本文・証拠版変更に依存する知識の標的失効→旧根拠/旧判断保全 |
| C11 | 引用/実文面を使った原根拠版と照合→不存在の引用を生成しない |
| C12 | 欠落根拠だけ有限再取得→実本文consumerの追加、自己引用/増殖停止 |
| C13 | 候補→資料→Core→最終出力の原系統→資料/model/記憶/提供元へ差の原因分解 |

### CAP-D：記憶・学習・継続（14、ARCH-Dと別）

| ID | 施工→実consumer |
|---|---|
| CAP-D01 | 指示/観測/失敗/訂正の不変原本を別ID保存→旧新双方復元 |
| CAP-D02 | Goal依存subgraphの作業記憶選択/予算→長履歴の有限集合 |
| CAP-D03 | 同Core寿命中の認識/記憶/適応継承→複数要求の旧実値消費 |
| CAP-D04 | fresh process/版跨ぎの復旧/重複防止/訂正→実際の続行 |
| CAP-D05 | **記憶→推論→適応**をCore通常路に→未見別問題の前後実差 |
| CAP-D06 | 再利用関係/手順の形成+一般化条件→未見主体適用、scope外停止 |
| CAP-D07 | 学習変換に根拠/条件/反例依存→誤規則と依存結論の標的失効 |
| CAP-D08 | shortcutと従来能力を共存→不要全再計算を回避、同条件で妥当 |
| CAP-D09 | 原本と意味/依存indexを分離→relevant subgraphから原本へ復帰 |
| CAP-D10 | 圧縮/要約のsource復元性→永続要約が原証拠を消さない |
| CAP-D11 | 新sessionのGoal/参照/実回答/改稿継続→正しい先行ResultRecord参照 |
| CAP-D12 | 学習採否/作用採否を唯一Coreへ→外部Learner/Storeの独自COMMITなし |
| CAP-D13 | 規則/証拠/推論の明示失効・再評価→歴史は保全し現採否だけ更新 |
| CAP-D14 | 評価Goldと学習/生成の混線遮断→漏洩の識別・混入防止 |

### E：Goal・作用・安全・復旧（12）

| ID | 施工→実consumer |
|---|---|
| E01 | case/input/version固有性/重複拒否→再送と新版を分離 |
| E02 | 元Goalの必要成果/権限/禁止/判定を版固定→外部資料/学習による書換なし |
| E03 | 実演算/作用/中間値の依存鎖→真の成果前進だけGoalへ加算 |
| E04 | 時間/費用/資源/実進捗の有限継続→空回り停止、多段は有界続行 |
| E05 | COMMIT/SUSPEND/REJECT/FAIL/CANCELを型分離→理由/復旧条件/履歴 |
| E06 | 明示許可の外部作用/配送/補償→実target/receipt、描画だけで送信成功なし |
| E07 | 能力版/型付きport/交換・合成→ON/OFF/別実装のGoal影響差 |
| E08 | 出所→演算→結果/失敗のtrace/hash→判断入力まで独立再生 |
| E09 | Input→唯一Core→Outputの決定境界→通常要求の実表示/必要送達 |
| E10 | 外部tool/appの共通契約/取消/retry/費用→異種二consumerで同port |
| E11 | 未信頼資料/外部偽命令/秘密/権限/私有情報→本文から自動許可生成なし |
| E12 | 部分失敗/復帰/補償/既知作用の同一性→confirmed効果の二重実行なし |

### F：製品入口・媒体・接続（14）

| ID | 施工→実consumer |
|---|---|
| F01 | 凍結Python配布/API/日本語CLIとRust後継の版・互換境界→旧版不改変の対照 |
| F02 | local HTTP chat/trace/capabilities/healthの実起動/認証/異常応答→Core実経路 |
| F03 | browser chat/回答表示/evidence/trace→fresh sessionの未見複数turn |
| F04 | 先行**実回答**の詳説/形式変更/改稿→実計算・実文面の再消費 |
| F05 | 検索/文書/算術/コード/執筆を一つの通常UIへ→代表初見実出力 |
| F06 | 日本語意味正本↔英語等外向けlocale→主体/条件/採否同一性 |
| F07 | streaming暫定/失効/finalをCore一元化→取消/再生で二重確定なし |
| F08 | typed音声入出力adapter→話者/時刻/原転写/音声成果の出所 |
| F09 | typed画像/文書/動画入出力port→認識不能を未観測、PDF文字≠画像意味 |
| F10 | 電話/メッセージ/外部software/端末の許可port→本人許可/実作用/受領/Goal |
| F11 | konjac-Translationを**参照のみ**独立consumer契約→typed翻訳値、他repo編集なし |
| F12 | D4Pocket/BLUE-TANUKI等の異種consumer非主従逆転→同Core Goal/権限/trace |
| F13 | compiler/IR/記憶・計算基盤交換のtyped境界→意味差の実値照合 |
| F14 | 実デモ/英語導入/課題・再現根拠→実応答と未保証制限を明示 |

### G：検査契約と保証の施工物（12、E6で偽PASS禁止）

| ID | 施工→E7/E8で使う実観測 |
|---|---|
| G01 | 93+発見要件/元source-test全数台帳と意味処分→未紐付け・無承認削除0 |
| G02 | 通常E2E入力→根拠→能力→Goal→出力/効果/再生→実結果の追跡 |
| G03 | 元359/β147/union360/旧313の回帰適用・理由分離→互換非退行 |
| G04 | 独立未見の日本語chat/文章/会話評価契約→Gold/漏洩/失敗確認 |
| G05 | Core単体vs専門port ON/OFF同条件比較→問題別因果・保証差 |
| G06 | GPQA Diamond198 LIVE評価契約→seed0/Gold遮断/継承vs逐次学習 |
| G07 | 固定packet×Core版を別診断レーン→LIVE得点へ混入なし |
| G08 | 決定的再生/trace真正性/simulate vs real→同条件/差分の帰因 |
| G09 | CPU/RSS/8GB/p50/p95/時間/費用測定系→実機値のみ物理保証 |
| G10 | Windows/Linux/macOS/導入/ABI/format検査経路→実install/起動/互換 |
| G11 | Cloud/認証/browser/公開環境の限定検査→実提供先条件の可否 |
| G12 | 由来/許諾/公開資料/immutable release/英語Docs/秘密検査→配布境界 |

### E6-W03..W05：全接続・反証・工程出口

`E6-W03` 実配線順（順序は依存であり個別独立完成工事でない）：

1. `会話/編集=InputのResultRecord参照/原計算/出所/訂正/話題変更→Core K4初見説明/書換→実出力`。`A04..A15/B08/F04`をIntentラベルだけで閉じない。
2. `数学/表/文書/コード=既存math.rs/document.rs/code_capability.rsとtyped port→実数値/単位/元行/型付error→Core K2/K3/K4説明/次Goal`。port自身のCOMMIT禁止。
3. `検索/HTML/PDF=既存search.rs/ports.rs→search-listと取得本文を別不変packet→頁/span/版/取得時点→Core根拠/有限追加取得`。本文の引用≠世界真理；外部取得の未許可は対象別停止。
4. `記憶/学習=証明保存型+観測起点の両経路→scope/反証/適応版/再起動→通常会話/推論/生成の実consumer`。原本を要約へ置換禁止。
5. `Goal/外部作用/回復=各portにActionRecord/DeliveryReceipt/権限/実target/冪等/取消/UNKNOWN→元Goal再判定`。安全だけで正機能不在を穴埋め禁止。
6. `CLI/HTTP/browser/会話/他consumer=同一Engine case/actor/revision`。UIごとの別回答機/別Goal判定/actor混線禁止。
7. `音声/画像/動画/外部端末/他repo連携=typed port{実入力,状態,成果,失敗,許可}`。実providerなし/利用者資格なし/内部未実装を別状態；他repoを書換えない。
8. `実装交換=Windows優先8GB/他OS/CPU/ABI/Unicode/path/保存/復旧/予算を抽象契約化`。物理合格はE7/E8の実機ごと、全環境自動保証なし。

`E6-W04` `R1監査F01..F22;全系S01..S12`の原文/元目的/正常系/負例を参照し、正機能が正常に通り異常で誤作用・誤COMMITしないよう同Core通常経路に配線。製品`F01..F14`とR1反証`F01..F22`を分母・意味で別扱い。試験の最終実行はE7/E8へ。

`E6-W05` 各要求の施工状態を次で区分（受入品質とは別）：

| 状態 | 意味→処置 |
|---|---|
| `UNIMPLEMENTED` | 必須内在演算なし→Rust実施工。`BLOCKED_AUTHORITY`への偽分類禁止 |
| `CODE_ONLY` | stub/型/関数あるが実consumerなし→通電。完成不可 |
| `WIRED_LOCAL` | 限定direct/port接続だけ→残る通常入口/適用範囲を施工 |
| `NORMAL_PATH_PRESENT` | 通常入口の正成果/必要作用へ接続→E6候補。E7品質は未判定 |
| `BLOCKED_AUTHORITY` | 必要内部施工・安全制御あり、外部資格/本人許可だけ不足→正確なtarget/必要権限を特定し実送信を保留 |
| `NOT_RUN` | 対象環境・物理試験未実行→実測未了をE7/E8に持越し、PASS不可 |
| `FAIL` | 実経路が必要成果に反する→最上流根因へ帰還 |

`E6_EXIT=SYSTEM_SHAPED` iff `∀(93+発見+全継承) 必須内部正機能の現物存在∧原入口→実producer→実consumer→実成果または正理由停止→Core帰還 ∧ 必須未実装0 ∧ 必須CODE_ONLY0 ∧ 意味未対応UNMAPPED0`。外部権限待ちのみ`BLOCKED_AUTHORITY`で局所保留；「内部実装されていないこと」をそれへ移すな。`各行={実source/入口/所有主体/consumer/実値/異常/処分/検査対象/証拠SHA/未成立}`。ここで完成版`Git SHA;93+分母;source/test census;入口/ports;model/schema/dependency;許可待ち;未実測物理対象`を固定→E7へ。`SYSTEM_SHAPED≠93件PASS≠全OS合格≠release`。

## O6：E7 全体系QC（E6完成候補の実検査）

`E7_PRE=E6 SYSTEM_SHAPED`。対象`Rust SHA/model/store初期版/依存/OS/入力母集団`を固定。ソース修正は新版として旧FAIL保持。検査が環境上不可能なら`NOT_RUN`を明示し、同条件でない別OS/仮想passへ転記禁止。

`E7-W01` `cwd=rust/`で環境・必要対象に対し実行、各`command/exit/SHA/target/toolchain/OS/NOT_RUN_reason`を保存：

`cargo fmt --all -- --check`  
`cargo check --workspace --all-targets --locked`  
`cargo clippy --workspace --all-targets --locked -- -D warnings`  
`cargo test --workspace --all-features --locked`  
`cargo build --workspace --locked`。

実`CLI`起動・`HTTP`起動・認証成功/拒否・`SSE/browser`通常入口も接続実検査。`cargo test PASS`≠実UI/実配備。ツール不在は`NOT_RUN:TOOLCHAIN_UNAVAILABLE`、外部条件不足は正確な対象を明示。TLS無効化/権限偽装/無断依存すり替え不可。

`E7-W02` 正常×異常（全93+追加、全系S01..12、R1反証F01..22、E2七ゲート、T01..03、適用される旧source/test）：

| 系 | 必須正常→故障注入 |
|---|---|
| K1 | 主語/目的/否定/条件/時制を正回答まで保持→条件IDだけ/帰属誤り/過大照応 |
| K2/K3 | 多前提/反事実/因果依存の新導出→欠落/矛盾/古資料/仮説本状態混入 |
| K4/L6 | 初見新意味/新文章/比較/執筆→shuffle偽生成/Gold流用/架空引用/意味変質 |
| K5 | 根拠保存/後続実使用/未見適応/fresh再起動→scope外発火/誤訂正/版混同/履歴消去 |
| K6/K7 | 必要な実成果を全達成→表示で送達COMMIT/効果UNKNOWN再送/無前進loop |
| 複合 | 検索実本文→数値計算→新文章→必要作用→Goal→固定中間値/port独自判断 |
| 一意性 | 異actor/同時case/重複/再送/訂正競合→状態越境/ID上書き/二重writer |
| 未信頼入力 | 正当資料引用/可逆許可作用→偽命令HTML/PDF/秘密漏洩/権限越境 |
| 実出力 | 実byte/実ファイル/正target/実受領→hash不一致/表示欠落/送達UNKNOWN/取消再送 |
| 回復 | 中断後原本/予算/効果/版の正復元→破損event/半作用/schema/Unicode/path不整合 |
| 有限資源 | 長履歴の関連subgraph/有界Goal達成→無限再試行/メモリ肥大/deadline逸脱 |

`E7-W03` 再現条件=`input/initial_case_state/model_revision/evidence_version/permission/budget/seed`。Coreの意味/根拠/作用計画/採否/系列選択を再生し変動要因を分離。`Windows8GB優先;Linux/macOS;CPU/ABI;日本語UTF-8;filesystem/SQLite;実network認証`を実対象別検査。`CPU,peak_RSS,p50/p95,時間,費用,保存量,復旧時間`は実測に基づき保証範囲固定。既存旧Python313や別Rust版のPASSを合算禁止。

`E7-W04` FAIL原因を`元目的/述語/原入力/根拠/最初の不正中間値/consumer/状態/版`で分解→最上流Wへ戻す→新版実施工→局所正負→影響G/T→必要全体系回帰。`PASS/PARTIAL/FAIL/NOT_RUN/BLOCKED_AUTHORITY/UNKNOWN`を母集団・環境・SHA付きで報告。`QC_SCOPED_PASS`は実合格した限定範囲のみ。未修復の必須正機能FAILをQA合格へ進めない。

## O7：E8 独立QA／実測保証境界

`E8_PRE=同一候補SHAで必要QC合格`。Gold/目標文は生成器と学習器から遮断。評価前に`母集団/版/model/記憶/専門port/外部LIVE条件/権限/時間/seed/採点法`を固定し、測定中に都合よく変えない。

`E8-W01` 独立：`一般初見24(what/why/how/compare);文章生成・編集12;会話10session×4..6turn;意味変換30;全系S01..S12(12);R1反証F01..F22(22);Req93+発見`。Gold漏洩・重複・除外理由・適用母集団を固定。採点=元Goalの実正結果、根拠、条件/主体、必要完結性、作用、再現性、資源、理由の要求別実観測。自然文の似た語数・停止成功だけで合格判定不可。

`E8-W02 GPQA`：元`評価/評価契約_v3.md`・`評価/GPQA評価二系統契約_v1.json`・`tools/評価契約.py`の**現在の原文**とデータSHA/提供元を再照合。`GPQA Diamond=198問index0..197;LIVE_ONLY;seed0;Goldは実行後採点にのみ使用;Europe PMC/Crossref有効;Wikipedia(en)/OpenAlex無効(履歴条件は原典で再確認)`。`並列独立継承=各問題同じ事前モデル/記憶を独立初期化;各問題の適応は他問題へ漏らさない`。`同一Core逐次学習=同じCoreでindex順;正規の記憶→推論→適応を後続問題で実消費`。各モード90分の歴史v3境界に従い、Core単体/専門ON/OFF、OS/資源/検索取得/timeout/問題別正誤を別記。固定packet/Gold混入/別機能の正答代行不可。旧`40/198`Core・`80/198`専門付きは歴史であり新Rustに加算禁止。再現不能モードは`NOT_RUN`または条件差つき別測定。

`E8-W03` 保証軸=`Core単体/専門ON;Rust/凍結Pythonβ;CLI/HTTP/browser/他consumer;Windows/Linux/macOS;8GB実機/大容量機;local/Cloud;実LIVE/packet診断;言語/媒体;正常成果/拒否/復旧`。各組を`成立/不成立/未観測/権限待ち/対象外`でSHA/実case/環境/制限付き判定。一環境の成功で全OS/全母集団保証禁止。新Cloud Run/HF配備・本番公開・課金・外部送信は別ユーザー許可。E8 FAIL→最上流根因→新版QC/QA再実施。`E8_EXIT=保証できる対象・できない対象・未試験対象の版付き確定`、releaseとは別。

## O8：差戻し・変更統制・最終点検

`FAIL_MAP={主語/Goal/条件→E0採否またはE1-W03/04;関係/採否→E1-W05/06;新意味/文面→E1-W07/08/09;記憶/適応→E1-W10..13;Goal/作用/偽COMMIT→E1-W03/W14..17;原文封印→E3;実byte/receipt→E4;T不通→該当E1/E3/E4;必要専門能力/継承→E6;安全/再現/資源/QA→最上流E1..E7}`。修正に仕様意味変更/母集団縮小/権限移転が必要なら設計者/オーナー採否；承認内の実修理は続行。原因未観測は`UNKNOWN`と記し推定で証拠塗替え禁止。

`Git`：対象HEAD/dirty/他writerを確認→必要範囲編集→差分/秘密/テスト結果を検査→commit/pushの明示委任があればその範囲。無断PR/branch/CI/tag/force/削除/他repo変更なし。D2本書・AGENTS・施工索引の版関係を保持。GitHub輸入テキスト変更時のみ`checksums.repo.sha256`の対象行を更新、`DOCS_REVISION.md`に版の理由と保持物記録。旧`checksums.sha256/RELEASE_MANIFEST.json/旧配布`改変禁止。

`引継ぎ最小={repo/base_SHA/current_SHA/許可,現工程/出口述語,変更した実ファイルと理由,原入力→実producer→中間値→consumer→元Goalの結果,確認した正負/NOT_RUN/根因,権限/秘密/外部作用,次に接続する実consumer/差戻し先}`。意味ある施工束にのみ実測値記録；日報・独立監督・全資産再棚卸し・重複チケットは禁止。`FAIL≠PARTIAL≠NOT_RUN≠BLOCKED_AUTHORITY≠UNKNOWN`、対象版と範囲を保持。

`SELF_CHECK={S01:唯一CoreがGoal/採否/学習/状態writerを所有,S02:原文/主語/目的/手段/条件/出所/版の保持,S03:新意味+L6新文面が本番Coreで生成,S04:記憶→推論→適応が未見別問題で実消費,S05:採用/描画/送達/元Goal完了を別判定,S06:全実producerに次consumer,S07:E2 G×7同版PASS,S08:E5 T×3通常経路PASS,S09:E6 93+全source義務保持,S10:内部未実装と外部権限待ち分別,S11:E7/E8対象版別保証,S12:凍結β/秘密/他repo/公開権限保全,S13:R2/R4/D1/D2の版差保全,S14:実証拠に比例したPASS判定}`。各判定は**現在工程で実観測可能な対象のみ**評価し、未到達を合格と偽るな。

`END_ORDER=まず一台のCoreを成立させる→七条件を同一版で合同検査→最小Input/Output→通常三機通電→全93+新発見+全source意味/consumerの躯体→全体系QC→独立QA。既存局所成果は再利用し、他の欠陥を理由に成立済み能力を無意味に再造しない。失敗は元Goalと実consumerから生成根因へ戻せ。工程未完は未完と判定し施工を閉じるな。`
