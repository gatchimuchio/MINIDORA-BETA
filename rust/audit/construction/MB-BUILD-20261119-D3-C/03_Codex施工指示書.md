# MINIDORA-BETA｜Codex施工指示 D3-C／日本語意味基底・LLM圧縮

`ID=MINIDORA-ORDER-JP-20261119-D3-C`｜`用途=施工指示のみ`｜`対象=gatchimuchio/MINIDORA-BETA/rust`｜`責任={指示/権限/採否/公開:オーナー,設計/仕様整合:チャッピー,施工/局所検査:Codex,利用:エンドユーザー}`｜`言語=日本語意味基底`｜`目的=対象意味の保持・施工解釈幅縮小`｜`非目的=人間可読性/字面/IT慣習`。

`正本={R2:docs/redesign/01_SYSTEM_SPECIFICATION.md,仕様:rust/audit/construction/MB-BUILD-20261117-D2-LLM/01_施工仕様書.md,工程:同/02_施工工程表.md,要領:AGENTS.md,要件:docs/redesign/02_CAPABILITY_REGISTER.md+03_REQUIREMENT_TRACE.tsv+rust/audit/requirements.tsv}`。本書はD3施工指示圧縮版；**D2仕様/工程非改訂**。D1/R4/R2/旧D2指示/旧BUILDは当時の版・根拠・判断を保持。旧文書の「当時現行」は現在効力へ昇格禁止。文書の作成・提示≠repo編集/commit/push/施工開始/外部作用/公開許可。本書自身に新権限なし。

## 0｜唯一主語・不変・工程

`主語=唯一Engine/Core`；`成果=未見原文→Input[原本/意味/Goal封印]→Core[K1意味,K2関係,K3根拠採否,K4新意味内容+非NN新文面,K5記憶→推論→適応,K6元Goal未充足駆動,K7内容/実成果採否]→Output[実byte/許可作用]→実観測/receipt→**同Core元Goal充足判定**`。`Input/Output/Store/Port/UI/LLM/Rust/テスト/専門ライブラリ=従属`。非Transformer・非ニューラル・日本語意味基底・決定的再現性・論理契約のOS/CPU/形式/言語交換可能性を保持。外部NN/完成文の暗記/Gold/独立Learner/別Core/別Goal writerによる代行禁止。

`不変={原ユーザー指示と主語/目的/手段/条件/時点/版,元Goal固定,原根拠・原本・歴史,全93+新発見要件,継承全source意味+test,停止/許可/作用同一性/秘密/未観測,責任境界,公開境界}`。`証拠=実producer→実値→実consumer→実成果/正停止→元Goal必要述語変化`；`非証拠=struct/port/テスト数/コミット数/局所単体PASS/HTTP200/定型挨拶/保存件数/固定packet/旧Python313/過去GPQA点数`。`状態差=UNIMPLEMENTED≠CODE_ONLY≠WIRED_LOCAL≠NORMAL_PATH_PRESENT≠NOT_RUN≠BLOCKED_AUTHORITY≠FAIL≠UNKNOWN`。

`工程=E0[現物/権限]→E1[CORE_SHAPED]→E2[CORE-G01..07同一SHA全PASS]→E3[最小Input]→E4[最小Output]→E5[T01..03通常入口同一SHA全PASS]→E6[SYSTEM_SHAPED:93+発見+全source]→E7[体系QC]→E8[独立QA]→OWNER_GO`。前工程出口が未成立なら次工程新機能増築禁止。`W=一台Core内の依存位置`、W個別独立完成/QA禁止。`BUILD=必要な全体接続＋破壊/秘密/許可/二重作用の近接負例のみ`、`E7=全体系QC`、`E8=独立QA`。意味/実装/状態の同義置換禁止。

`観測基線=b1d94eb(2026-11-19 JST;着工時再観測)`。当時`Rust=0.7.0-alpha.11;ConditionDefinitionとFact分離/TRUE-FALSE-UNKNOWN-CONFLICT+AND-OR-NOT一部成立;B49証明学習/B56二前提・反事実限定;B81..B93=主にW10 clone-only/Recovery局所負例;B93=13関連テスト;E1-W10 PARTIAL;E1未成立;E2合同未PASS;E5=0/3;req93 acceptance=UNVERIFIED;source review=98/416(旧記述96との差)`。起点は観測の参照のみ、現在HEADの真理としない。既存正機能再利用、初期差分を再観測し不足のみ施工。

### 0-A｜作業選別・停滞遮断

`欠落→分類={CORE_SHAPE_BLOCKER:二循環の実値鎖/単一writer/元Goal/必須安全を壊す→最小根因施工;DEFER_E6_OR_E7:全体鎖を壊さないRecovery追加枝・clone-only細部・全互換・全lint・全源検査・任意cache/log→既知不成立保持し延期;DESIGN_GAP:仕様/参照先/値域/真理/更新許可が未定義→無断定義禁止・設計者へ対象/選択肢/影響帰還;BLOCKED_AUTHORITY:内部成立済みで外部許可だけ不足→対象作用のみ保留}`。

`1束=現物観測→最初の不正中間値特定→必要最小施工→直接正例+該当負例+隣接consumer検査→次の未接続W`。`同Wの新テスト増加だけでE1必須辺の接続数が変化しない→局所拡張停止、次必須consumerを施工`。古い成功/失敗・禁止・保証未了を消さない。正常経路を停止だけで代替禁止。全ローカル異常検査・全source監査・新媒体をE1出口に加算禁止。

## 1｜E0着工

`READ[毎独立着工]={ユーザー最新原文と現在許可/禁止,本書,AGENTS,現行版採用索引,D2仕様/D2工程,R2影響節,Git HEAD/status/remote/main/dirty/staged/別writer/未push,影響Rust現物,test,既存証拠}`。`関連台帳=docs/redesign/{02_CAPABILITY_REGISTER.md,03_REQUIREMENT_TRACE.tsv,04_SOURCE_CENSUS.tsv,05_VERIFICATION_AND_EVIDENCE.md,06_LINEAGE_AND_DECISIONS.md,07_R1_AUDIT_CLOSURE.md,08_WIRE_SCHEMA.json,09_TEST_CENSUS.tsv}+rust/audit/{requirements.tsv,source_contract_reviews.tsv,implementation-obligations.md,IMPLEMENTATION_STATUS.md}`。`現物={rust/Cargo.toml,Cargo.lock,rust-toolchain.toml,rust/docs/IMPLEMENTATION_CHOICES.md,DEPENDENCIES.md,rust/crates/minidora-domain/src/lib.rs,rust/crates/minidora-engine/src/{core,relations,store,recovery,language,input,output,scheduler,math,ports,search}.rs,rust/apps/cli/src/main.rs}`。型/パス改名は現物で正す。E0で414件全意味監査/全QC/GPQAを先行しない。

`保全={元NOTNN-LLM-MINIDORA:参照のみ,凍結Pythonβ/src,公開API,旧wheel/ZIP, SOURCE_LOCK.md,REPOSITORY_LOCK.md,RELEASE_MANIFEST.json,checksums.sha256,旧Git/旧証拠,他repo,非公開HDS/秘密/PII}`。`単一writer/actor-case-input/version/effect/model/goal ID`の既存所有者・権限・衝突を観測。`ConditionDefinition≠Fact.id`；condition refsの未定義は参照先/演算/欠落/矛盾/時点/版/producer/consumerまで分別。`E0_EXIT=現物版・唯一Core・必要値契約・権限/重大衝突が明確`。

## 2｜E1一台Core全体躯体

`E1_PRE=E0_EXIT`。`循環A=未見原子意味+Goal→K1→K2→K3→K4[新意味内容→六要素新系列]→K7採用→K6必要動作→実観測→元Goal判定`。`循環B=実観測/訂正→K5不変原本→推論/限定一般化→版付き適応→後続未見別要求のK2/K3/K4/K6実消費→実結果差/反証失効`。両循環＝**同じEngine/単一Goal判断権限**。W内品質全幅は後工程。

| W | 施工→必須次consumer／E1局所出口 |
|---|---|
| `W01` | `domain/core/store/recovery`：actor/case/input/goal/source/memory/model/plan/actionのID+revision単一writer・旧原本・重複/競合拒否→W02..18同版取得 |
| `W02` | `InputBundle/MeaningIR`へtyped原子{主語/述語/目的語/否定/条件/scope/時制/出所/Goal/許可/資源}封印→W03..18。完成文/Gold/第二判断器入力禁止 |
| `W03` | `core/scheduler`：固定`SUPPORTED_CONTENT/EXACT_RENDER/CONFIRMED_FILE`だけに依存するGoalを、初見指示固有{成果/必要述語/依存/禁止/権限/版/期限/予算/実証拠}へ→W14未充足とW16完了判定。元指示無断補完禁止 |
| `W04` | `ConditionDefinition≠Fact`；条件型/TRUE-FALSE-UNKNOWN-CONFLICT/AND-OR-NOT/否定/時点半開区間/scope/主語/引用/仮説/版・依存失効→W05/06/07。参照ID文字一致で真偽決定禁止 |
| `W05` | `relations/core`：型・変数/多前提/条件/scope/矛盾/反事実隔離/欠落・時点/依存→`meaning_id+proof_DAG+unmet+conflict`をW06/07/10へ |
| `W06` | 証拠型{観測/ユーザー前提/導出/仮説/競合/範囲外/未知/引用}×根拠/保証→`AdoptionDecision{class,scope,dependency,reason}`をW07/09/14へ。仮説/類似語を真理へ昇格禁止 |
| `W07` | `language/core`：未見原子関係/新条件/論証/比較/計算から新`meaning_plan{claim,proof,limit,order,format,required_units}`→W08/09。`unit.text`並替えだけは禁止 |
| `W08` | `TOKEN_SPACE→CONTEXT_STATE→PARAMETERIZED_TRANSFORM→CONDITIONAL_NEXT_TOKEN_DISTRIBUTION→SEQUENCE_OBJECTIVE→DECODING_IF`六実値＋有限/seed/EOS/model版/逆意味制約→W09新系列。外部NN/完成文候補/要求内一時学習だけで適応偽装禁止 |
| `W09` | K7：新意味計画×新文面×出所/否定/数量/時点/条件/必要units照合→版付き`ContentPlan`をW14/16へ。`採用≠描画≠送達≠Goal完了` |
| `W10` | `store/recovery`：実原文/根拠/結果/訂正/作用は不変、index/失効/要約は別版、関連subgraph有限選択→W11..13と**後続別要求K2/K4**。B81..B93局所Recovery反例増築≠E1成立 |
| `W11` | B49既存の型/変数/scope/condition/proof付き合成規則を同Core本番へ→W13/後続K2-K4-K6。別主体汎化、範囲外不発、根拠失効時標的取消 |
| `W12` | 観測標本/出所/時点/反例→事前母集団・scope・期限・採否規則なしなら`HYPOTHESIS_RETAINED`。証明と経験保証を分離→W13/K3/K4/K6 |
| `W13` | K5保存記憶/規則/`SequenceModel::learn`相当を本番版付き適応へ→未見別要求のK2/K4/K6で実差、旧版再現、再起動継承、scope外不発・依存失効 |
| `W14` | W03元Goal未充足DAG→必要演算/検索本文/新意味/許可作用の有限次選択、実引数連結→W15/16。検索回数/保存件数≠進捗 |
| `W15` | `ActionRecord:{PURE/READ/LOCAL_WRITE/EXTERNAL_WRITE/IRREVERSIBLE,actor,permission,target,hash,action_id,idempotency,budget}`；`PROPOSED→AUTHORIZED→PREPARED→ATTEMPTED→CONFIRMED/FAILED/UNKNOWN/CANCELLED`→W16実receipt。UNKNOWN再送禁止 |
| `W16` | 採用ContentPlan→`output::render`→実byte/hash/target/version/必要ファイル読戻し/送達別receipt→同Coreが元Goal**全必要述語**判定。未確認/欠落/取消=非COMMIT |
| `W17` | 有限budget/deadline/無進捗停止/再起動ID・版・作用既実行照合/取消・補償・証拠失効→W18有限正終端、二重作用0 |
| `W18` | W01..17の**同Engine本番結線**、循環A+Bの実producer値を実consumerへ→`CORE_SHAPED`候補。port/UI/Outputの独自採否禁止 |
| `W19` | 各Wは構文/型/直近正例・必要負例/保存互換・権限/秘密/二重作用だけ近接検査→次W。本格回帰・全lint・体系QC・独立QAはE7/E8 |

`W実順=01/02/04/05/06共通実値→03/07/08/09新意味文面→10/11/12/13後続適応→14/15/16/17 Goal成果→18合同結線→19限定smoke`。番号順の独立完成請負ではない。`I-A=未見原子/条件/二前提→意味採否→新内容+新文面→Goal実結果`；`I-B=観測/訂正→保存学習→未見別要求の実差→scope外/失効`；`I-C=説明/計算/実可逆ファイルGoal→計算実引数/byte読戻し→元Goal成果判定`；`I-N=別actor/条件FALSE-UNKNOWN-CONFLICT/無許可/作用UNKNOWN/取消/期限/旧版の正停止`。工事治具は正解生成しない。

`E1_EXIT=CORE_SHAPED iff{一台Engine,単一writer,循環A実値連結,循環B後続実消費,元Goal固有述語,実成果帰還,停止有限,無許可作用0,**二循環の必須未接続辺0**}`。`非要求=93全幅/全自然言語/Recovery全枝/全OS/全負例/QA`。`CORE_SHAPED≠E2_PASS`。

## 3｜E2同一Core七ゲート合同試運転

`E2_PRE=E1_CORE_SHAPED`；同一SHA+Engine/model/store/evidence/actor/Goal/permission/budget/seed。各Gは初見正例+一条件差/負例+実中間値→consumer因果確認。`PASS/PARTIAL/FAIL/NOT_RUN/BLOCKED_AUTHORITY/UNKNOWN`別。

| G | 正負→PASS／回帰W |
|---|---|
| `CORE-G01` | 別主体/否定/TRUE-FALSE-UNKNOWN-CONFLICT/時点/scope/引用差→依存K2/K3/K4/K7のみ正変化、文字ref誤採用0。W03/04 |
| `CORE-G02` | 多前提導出/欠落/矛盾/別scope/反事実/無関係Fact→proof DAG付き採否・本状態非汚染。W05/06 |
| `CORE-G03` | 初見説明/比較/条件/数値/書式→**新意味内容+六要素新文面+K7実採用**、既成文shuffle不可。W07/08/09/13 |
| `CORE-G04` | 実観測/訂正/記憶→後続別入力消費→失効/別session/restart、歴史非改変/依存だけ変化。W10..13 |
| `CORE-G05` | B49型証明学習前後/未見別主体/scope外/根拠失効/restart→K2/K4/K6で学習実差。経験採用は別条件。W10..13 |
| `CORE-G06` | 元Goal全成果/一欠落/無許可/取消/実効果UNKNOWN/ファイル→確認された必要成果だけCOMMIT、再送0。W03/14..17 |
| `CORE-G07` | 同状態fresh再生/証拠・条件・model版の各一点差→意味/採否/系列/plan/作用決定再現と依存差。W01/08/10/17 |

`E2_EXIT=∀G01..07 PASS∧same_SHA`。過去B49/B56局所成功、混在SHAの個別PASSを積算禁止。FAIL→根因W施工→影響Gと共通循環A+B再試験。`CORE-Gxx≠検索要件Cxx`。

## 4｜E3/E4/E5最小三機通常通電

`E3_PRE=E2_EXIT`；`raw日本語bytes→原本/hash/正規化差/span/actor/主体/否定/条件/時点/scope/引用/Goal/許可→sealed InputBundle/MeaningIR→同Core`。Input独自真偽/完成文/学習/Goal上書き禁止；資料内命令は権限0；未支持照応/条件はUNKNOWN。`E3_CHECK={初見説明/比較/訂正/引用/省略/前回答追問/無許可/別actor}`。`E3_EXIT=原文保全・型・未解決状態を同Coreが実消費`。

`E4_PRE=E3_EXIT`；`ContentPlan採用→実UTF-8 byte/許可可逆file→RenderObservation{plan_id,renderer_version,actual_bytes_hash,target,status}→同Core`。Goalにfile必要→実読戻し；送達必要→別DeliveryReceipt。表示成功≠保存/送達成功。失敗/欠落/版不一致/取消/効果UNKNOWN→非COMMIT。`E4_EXIT=実byte/失敗が同Core Goalに帰還`。

`E5_PRE=E2_PASS7∧E3∧E4`。試験＝実CLI/HTTP/browser**通常利用者原文入口**、内部typed治具/固定回答/HTTP200不可。

| T | 正例+負例／PASS |
|---|---|
| `T01` | 未見日本語の説明/比較/条件/数量→封印→新意味+新系列→K7→実表示byte→元Goal；定型/文shuffle不採用 |
| `T02` | 先行実観測/訂正/規則→後続未見別要求/別session/restart/scope外→記憶→推論→適応の実差と正停止 |
| `T03` | 許可実ファイル/表示→実読戻し/receiptと全述語COMMIT；未達/無許可/取消/作用UNKNOWNは非COMMIT・再送0 |

`E5_EXIT=T01..03全PASS∧same_SHA∧ordinary_route`。失敗→E1/E3/E4の最上流根因へ。T未PASSで新専門能力/媒体/チャネルを増築して迂回禁止。

## 5｜E6全93+新発見+全継承sourceを同Coreへ

`E6_PRE=E5_EXIT`。`母集団=Req93{A15,B13,C13,CAP-D14,E12,F14,G12}+NEW_REQ{出典/理由/正本関係/実consumer/検査}`。各IDの要求意味全幅は`R2/02_CAPABILITY_REGISTER/03_REQUIREMENT_TRACE/requirements.tsv`を正本とする；以下一行は削減許可でない。`違うID={CORE-Gxx[工事試験],Cxx[検索要件],CAP-Dxx[学習要件],ARCH-Dxx[設計区分],F01..14[製品],R1-F01..22[監査反証]}`。

`元分母={source原414,β416;test原359,β147,union360;旧Pythonβ313選抜}`。異なる集合の合算・単純差引禁止。`source_contract_reviews.tsv 98/416`は旧`IMPLEMENTATION_STATUS.md 96`と不一致→現行`path+SHA+実review記録`で再計数。`SOURCE_EACH={path,原hash,旧意味[入力/演算/正常結果/異常/副作用],旧producer/consumer,後継所有機械/実値/実consumer,意味等価/差分,処分+根拠,req/test/対象版}`。**全原ファイルの内容まで読取**、import/名前/型だけで継承PASS禁止。`R2第20節の全source意味検査はE6末尾で閉包`；E0事前化しないが要求削除禁止。

`REQ_EACH={ID,元成果/禁止,正常/異常,元入口,実producer,実consumer,元Goal帰還,source処分,R2実装区分,R2受入区分,実版,未達/権限}`。`implementation_disposition={RETAIN,REWIRE,REPLACE,NEW,EXTERNAL_PORT,REJECT_WITH_REASON,UNVERIFIED}`。`acceptance_status={NATIVE_AND_ACCEPTED,CONNECTED_PORT_AND_ACCEPTED,EXPLICITLY_UNAVAILABLE,NOT_IN_AUTHORIZED_RELEASE_SCOPE,FAILED,UNVERIFIED}`。**R2列の値域無断変更禁止**。別診断`{UNIMPLEMENTED,CODE_ONLY,WIRED_LOCAL,NORMAL_PATH_PRESENT,BLOCKED_AUTHORITY,NOT_RUN,FAIL}`を`acceptance_status`へ混入禁止。E6実接続≠E7/E8品質受入；無根拠`ACCEPTED`禁止。

### 5-A｜Req A01..A15：言語・意味・会話

`A01=非NN六要素Core生産→実model値；隠れ外部LLM不可`
`A02=有限token/系列/context→初見新文面+EOS`
`A03=条件付き分布/seed/tie/model版→再現/版差因果`
`A04=日本語主体/語/節/省略・照応→未支持UNKNOWN/別actor混入0`
`A05=否定/量化/例外/条件/様態/時点/scope→推論・文面保持`
`A06=定義/修飾先/関係節/元span→意味と原本文照合`
`A07=先行実回答/演算値の照応・訂正→後続要求で実使用`
`A08=長会話topic/Goal/format/style/evidence版→話題変更復帰/目的非混線`
`A09=型付き直接・導出関係/依存/矛盾→多資料の正当関係`
`A10=論説[対象/生成因果/前提/根拠/推論/結論/適用範囲]→対応論評/反証`
`A11=翻訳[日本語↔英語/中国語等]→主体/否定/条件/損失同値`
`A12=不足/未知型付け→有用支持済部分+未充足明示`
`A13=未見what/why/how/compare通常原文→非定型実質回答`
`A14=先行実計算/回答→換言/詳説/例/指定書式へ根拠継承`
`A15=raw封印→K1..7→Outputで同一MeaningIR/出所/Goal版`

### 5-B｜Req B01..B13：課題・計算・専門

`B01=未見目的/成果/制約→Coreが適port選択/不能理由`
`B02=算術/有理数/数量/単位→実数値をK4/後続演算へ`
`B03=代数/方程式/統計/確率→前提/計算手順/実結果`
`B04=表/JSON/CSV/文書の比較/集計/変換/検査→typed成果/警告`
`B05=複数資料の独立/支持/反例/欠落→Core根拠比較`
`B06=多段課題の先行実中間値→後続実引数/元Goal`
`B07=未見コード生成/解説/修理/有限実行診断→実行/未実行分離`
`B08=執筆/要約/編集/書式・長さ制約→新文章の意味適合`
`B09=科学/数学/専門知識→出所付きCore利用/専門保証分離`
`B10=候補生成→根拠比較→採否/保留/訂正；Gold隔離`
`B11=因果/反事実/類推の前提/scope→依存差/本状態非汚染`
`B12=権限/予算/Goalの行動計画→実状態変化/正停止`
`B13=検索×計算×文章等の複合port値→唯一Core Goal`

### 5-C｜Req C01..C13：検索・証拠

`C01=許可Web/局所検索→候補だけでなく実本文をCore根拠へ`
`C02=provider別timeout/error/rate-limit/retry/cost→有限実行`
`C03=URL/取得時刻/版/hash/頁/原byte→不変source同一性`
`C04=候補一覧packet≠本文取得packet→別再生`
`C05=fixed packet診断≠LIVE公式評価`
`C06=資料の独立/支持/反例/文字重複→K3採否`
`C07=百科/論文Crossref/Europe PMC等/局所provider→typed実取得・失敗`
`C08=HTML本文/見出し/ルビ/不可視文字/span→原byte照合/警告`
`C09=有限PDF文字抽出/頁/失敗→OCR/画像認識との保証分離`
`C10=根拠/取得版更新→依存cache/採否標的失効、原本保持`
`C11=引用/出所/実文字と原packet版照合→架空引用0`
`C12=不足資料だけ有限再取得→実本文consumer/空回り停止`
`C13=原入力→候補/実資料→根拠→Core→最終成果の系譜再生`

### 5-D｜Req CAP-D01..CAP-D14：記憶・学習

`CAP-D01=原文/観測/失敗/訂正不変保存→旧新復元`
`CAP-D02=Goal依存subgraphの作業記憶有限選択→実consumer`
`CAP-D03=同Core寿命の別要求→前記憶/経験を実利用`
`CAP-D04=fresh process/版跨ぎ復旧/冪等→実継続`
`CAP-D05=記憶→推論→適応→未見別要求の実結果差`
`CAP-D06=再利用関係/手順形成+一般化条件→別主体/範囲外停止`
`CAP-D07=規則と反例/根拠依存→標的失効・再学習`
`CAP-D08=適法shortcut+従来経路共存→意味/保証非退行`
`CAP-D09=不変原本と意味/依存index分離→原本へ追跡`
`CAP-D10=圧縮/要約はsource復元可能・原証拠不消去`
`CAP-D11=新session Goal/先行実回答/改稿参照→正継続`
`CAP-D12=学習/採否/Goal COMMITは唯一Core責任`
`CAP-D13=規則/採否/依存証拠の失効再評価→当時判断保持`
`CAP-D14=Gold/独立QAと訓練・生成・模型版の遮断`

### 5-E｜Req E01..E12：Goal・作用・復旧

`E01=actor/case/input/version同一性+衝突/重複拒否`
`E02=元Goal必要成果/権限/禁止/条件を版固定`
`E03=実演算/作用→依存結果→元Goal必要成果の実前進`
`E04=時間/費用/資源/進捗budget→有限続行・空回り停止`
`E05=COMMIT/SUSPEND/REJECT/FAIL/CANCEL型・理由・再開条件`
`E06=許可外部作用/配送/補償→実target/receiver/receipt`
`E07=typed port能力版交換/ON-OFF/合成→Coreの実差`
`E08=原入力→根拠→演算→結果/失敗/作用trace/hash真正性`
`E09=Input→唯一Core→Outputの元Goal帰還を実結線`
`E10=外部tool/app共通契約/取消/retry/費用→異種consumer`
`E11=未信頼資料の偽命令/秘密/PII/権限越境→誤作用0`
`E12=部分失敗/復旧/補償/既実行action識別→二重作用0`

### 5-F｜Req F01..F14：製品入口・媒体

`F01=凍結Pythonβ/API/日本語CLI/配布互換境界→旧版非改変`
`F02=local HTTP chat/trace/capabilities/health実起動+認証/拒否/異常`
`F03=browser chat/出力/根拠/trace→fresh複数turn`
`F04=前**実回答**の詳説/形式変更/改稿→実計算/実文面再消費`
`F05=検索/文書/数学/コード/執筆を通常UI一入口へ`
`F06=日本語意味正本↔外向けlocale→主体/条件/採否同一`
`F07=streaming provisional/invalidated/final→Core一元確定/取消`
`F08=typed音声入出力→話者/時刻/原転写/成果の出所`
`F09=typed画像/文書/動画入出力→認識不能の正未観測`
`F10=電話/メッセージ/端末/外部ソフト許可port→実受領`
`F11=konjac-Translation参照専用typed連携→他repo編集0`
`F12=D4Pocket/BLUE-TANUKI等異種consumer→Core主従/権限保持`
`F13=compiler/IR/記憶/計算基盤交換typed境界→意味/ABI/実値差`
`F14=デモ/英語説明/問題・再現根拠→実結果/未保証制限一致`

### 5-G｜Req G01..G12：検証契約

`G01=93+発見+全source/test意味対応台帳→未紐付け0`
`G02=原入力→意味/根拠/演算/Goal/作用/出力E2E実trace`
`G03=元359/β147/union360/旧313の別分母回帰/非適用説明`
`G04=独立未見日本語質問/文章/会話→Gold/漏洩契約`
`G05=Core単体/専門port ON-OFF同条件→保証差`
`G06=GPQA Diamond198 LIVE二方式正式評価`
`G07=fixed packet診断の別レーン→LIVE点数混入0`
`G08=意味/作用/traceの決定的再生＋simulate/real分離`
`G09=CPU/peak_RSS/8GB/p50/p95/時間/費用/保存/復旧の実測`
`G10=Windows/Linux/macOS/CPU/ABI/形式/導入の現物互換検査`
`G11=Cloud/認証/browser/公開環境の対象別境界検査`
`G12=出典/許諾/公開物/秘密/英語文書/immutable release検査`

### 5-H｜接続順・施工状態・出口

`実値結線=先行ResultRecord→次会話/編集;math.rs/document.rs/code_capability.rs実計算→K2/K3/K4;search.rs/ports.rs候補≠本文packet→K3;証明学習+観測仮説→後続別要求;ActionRecord/receipt→元Goal;CLI/HTTP/browser/他consumer→同Core;音声/画像/動画/端末/他repoはtyped port+権限・不能状態;Windows-first 8GBと他OS/ABIは交換契約`。`port自身=演算/取得/表示の従属`、Goal採否/世界真偽の独自writerは禁止。`他repo本体編集=禁止`。

`施工診断={UNIMPLEMENTED:必須演算欠落→施工,CODE_ONLY:型/stubのみ→結線,WIRED_LOCAL:直接呼出のみ→通常入口,NORMAL_PATH_PRESENT:通常実経路あり→E7品質候補,BLOCKED_AUTHORITY:内部完成+外部許可のみ不足→局所保留,NOT_RUN:実機/外部未実行→根拠未了,FAIL:必要成果と矛盾→根因修理}`。`R2受入列UNVERIFIEDを施工状態で上書き禁止`。

`E6_EXIT=SYSTEM_SHAPED iff{E5全PASS,Req93+発見の必須内在機能存在/通常経路へ結線,元全source意味+各実consumer全件照合,必須UNIMPLEMENTED=0,CODE_ONLY=0,未対応SOURCE=0,無承認削除=0,必要成果か正理由停止が元Goalへ帰還}`。外部資格待ち＝局所保留のみ、内部未施工をそこへ偽装禁止。`E6_SHAPED≠93_ACCEPTED≠全OS_PASS≠RELEASE`。候補`SHA/Req+source+test全分母/model/store/schema/入口/権限待ち`固定→E7。

## 6｜E7完成候補の全体系QC

`E7_PRE=E6_EXIT`；`FIX={Rust_SHA,toolchain,OS/CPU,model_revision,store_snapshot,schema,evidence_revision,permission,budget,seed,外部取得条件}`。E1各Wで全QC反復禁止、E7で体系一括実行。各検査は`{対象SHA,command,exit,正常/負例,実中間値,consumer,実結果,未実行理由,環境}`。`cwd=rust/`：

1. `cargo fmt --all -- --check`
2. `cargo check --workspace --all-targets --locked`
3. `cargo clippy --workspace --all-targets --locked -- -D warnings`
4. `cargo test --workspace --all-features --locked`
5. `cargo build --workspace --locked`

`実入口={CLI起動,HTTP chat/trace/capabilities/health,認証成功/拒否,SSE/browser,別actor/別session,可逆file読戻し,取消/異常}`。既存all-targets clippy負例含む現FAILを除外/ignore/allowで偽PASS禁止。工具/依存/network/実機不足は理由付きNOT_RUN、権限未取得は対象のみBLOCKED_AUTHORITY。`テスト通過≠ユーザー実経路成立`。

`QC負例軸={
K1:主体/否定/条件/時点/引用→誤照応/条件ref偽真理/actor越境,
K2-K3:多前提/反事実/独立証拠→欠落/矛盾/無効根拠/仮説採用,
K4-L6:初見新意味+新系列→旧文shuffle/外部NN/Gold/架空引用/意味逸脱,
K5:保存/学習/訂正/後続実利用→原本消去/誤scope/誤モデル版/再起動失敗,
K6-K7:実値駆動Goal成果→無進捗loop/元Goal固定述語偽COMMIT/必要成果欠落,
作用:許可target/receipt→効果UNKNOWN再送/二重作用/無許可補償,
複合:実検索本文→実計算→新説明→Goal→port独立採否/実値未使用,
復旧:別actor/並行更新/Unicode/schema/snapshot破損→誤上書き/半作用再実行,
外部資料:未信頼本文/偽命令/機密→権限昇格/secret漏洩,
資源:長履歴関連記憶/8GB/finite deadline→時間・RSS・費用逸脱
}`。`適用集合={Req93+発見,全系S01..S12,R1反証F01..F22,CORE-G01..07,T01..03,旧test359/β147/union360/旧313,全元source意味互換}`。集合と版が違うPASSを加算禁止。

`再現入力={原文/初期case/model/evidence/permission/budget/seed/時点/外部provider packet/OS}`。同条件→Core意味/導出/採否/内容/生成系列/作用計画一致；一条件差→依存部だけ変更。`物理対象={Windows 8GB優先,Linux,macOS,CPU/ABI/Unicode/FS/SQLite,実network/TLS/認証}`；実施した環境のみ`CPU/peak_RSS/p50/p95/時間/費用/保存量/復旧時間`を実測保証。未測定はNOT_RUN。`E7_FAIL→元Goal→最初の不正値→最上流Wへ帰還→新版で局所修理+影響G/T+必要全QC再検査`。`E7_EXIT=同一候補の必須正常路・負例・安全・兼容・再現の実対象別QCが成立；残る権限/物理未観測は別記`。

## 7｜E8独立QA・LIVE保証境界

`E8_PRE=E7_EXIT`。`QA独立={開発fixture/既成文/Gold/正解ラベルをK4/K5/推論/model初期状態から遮断;母集団/順序/採点/期間/seed/SHA/model/store/専門port/許可/OS/実取得条件を先に固定}`。試験中の恣意的除外/閾値変更禁止。

`QA_SET={一般初見24[what/why/how/compare],独立文章作成・編集12,会話10session×4..6turn,意味変換30,全系S01..S12,R1反証F01..F22,Req93+新発見}`。各分母/失敗/重複/未実行区分を保持。`採点=元Goalの正しい実成果×主語/意味/条件/根拠/版/実byte・作用/再現/時間資源`。自然文の類似や拒否のみ成功禁止。

`GPQA_READ={評価/評価契約_v3.md,評価/GPQA評価二系統契約_v1.json,tools/評価契約.py,現dataset/API}`。`GPQA_Diamond=198,index0..197,LIVE_ONLY,seed0,Gold=実行後採点専用`。provider歴史条件`Europe_PMC/Crossref=有効,Wikipedia(en)/OpenAlex=無効`は現在原契約と照合。`独立継承レーン=各問が同じ事前model/storeから独立起動;前問適応を次へ持込禁止`。`逐次学習レーン=同一Coreがindex順にK5記憶→推論→適応を次問で実消費`。各レーン`90分`等は元v3原典優先。`測定={問題別正誤/未知/根拠/取得packet/timeout/学習版/資源/実時間/専門ON-OFF/Core単体}`。fixed packet診断とLIVE正答を別集計。旧`40/198(Core),80/198(専門付き)`は別版歴史、現Rust結果へ転記禁止。外部LIVE不可＝NOT_RUN/対象権限待ち、擬似点数禁止。

`保証軸={Core単体/専門付き,Rust現版/凍結Pythonβ,CLI/HTTP/browser/他consumer,Windows/Linux/macOS,8GB/別機,local/Cloud,文字/音声/画像/動画,実LIVE/fixed_packet,通常/異常/復旧,許可/非許可}`。各実対象`{版,環境,データ,成立/失敗/未観測/権限待ち/対象外,根拠,制限}`。一環境の成功を全母集団に拡張禁止。Cloud Run/HF公開・実送信・課金・リリースは本人別許可。`E8_EXIT=成立/不成立/未測定/対象外の版付き保証境界確定`；`RELEASE=OWNER_GOのみ`。

## 8｜停止・帰還・変更・引継ぎ

`FAIL_MAP={主語/条件:W02/04,元Goal:W03,関係:W05,採否:W06,新意味:W07,新系列:W08,意味照合:W09,記憶/学習:W10..13,進捗/作用/COMMIT/回復:W14..17,全体断線:W18,原文封印:E3,実byte/receipt:E4,通常T:E1/E3/E4最上流,専門/元source:E6,物理QC/QA:E1..E7最上流}`。`帰還手順=元ユーザー目的→必須述語未達→最初の不正producer値→誤consumer→生成原因の観測→必要範囲施工→影響検査`。推測≠観測。上位意味/条件定義/採否基準/権限の追加が必要→DESIGN_GAPとして設計者/オーナー採否、Codex独断変更禁止。

`STOP={未許可外部/不可逆作用,secret/PII漏洩,元Goal無断変更,履歴消去,既発生効果UNKNOWNの再送,衝突writer/未解決正本矛盾}`。**影響する作用だけ停止**、認可済み読取/独立施工は継続。`禁止={未許可commit/push/PR/branch/tag/CI/force/reset,外部発信/公開/課金/Cloud配備,凍結β/旧manifest/他repo変更,安全迂回,別Core}`。施工命令の作成/提示/承認と、Git書込・送信・公開は別権限。輸入済みファイルを許可範囲で改修する場合のみ`checksums.repo.sha256`対象行と`DOCS_REVISION.md`版差を同期；`checksums.sha256/RELEASE_MANIFEST.json`は不変。旧根拠を新判断に上書き禁止。

`HANDOFF_MIN={repo,base_SHA,current_SHA,dirty/remote状態,現権限,適用仕様/工程/本指示/AGENTS,工程出口の成立/未成立,実producer→実値→consumer,近接検査正負/未実行,最上流根因,延期したE7/E8品質,安全/秘密/作用,次必須W}`。日報/チケット/独立監督/人工レビュー/同義台帳の追加禁止。必要版証拠は既存Git/監査台帳のみ。

## 9｜自己監査・唯一終了

`COUNT={E0..E8=9,W01..W19=19,CORE-G01..07=7,T01..03=3,A15+B13+C13+CAP-D14+E12+F14+G12=93,source原414/β416,test原359/β147/union360,旧313}`。各ID欠落/重複0。`COVER={唯一Core/Goal writer,日本語意味/主語/目的/条件/版,全W次consumer,六要素L6,未見新意味+新文面,記憶→推論→適応,局所W10≠全体E1,七G同版,三T通常入口,全93+全元source,UNKNOWN/権限/停止,体系QC/独立QA時期,Gold/秘密/他repo/旧配布,原歴史非改変,現在許可とGit操作分離,保証対象別実証}`。

`唯一施工目的=一台Core全体躯体→合同7ゲート→最小Input/Output→通常三機→全93/全source→体系QC→独立QA→オーナー採否`。進行判断=出口の実証拠のみ；局所テスト数/実装行数/報告/コミット数は代替不可。未成立は未成立と判定し実consumerの次施工へ戻す。
