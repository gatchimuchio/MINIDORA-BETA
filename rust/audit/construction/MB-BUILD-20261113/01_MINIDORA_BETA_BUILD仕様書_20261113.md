# MINIDORA-BETA｜全体系BUILD施工仕様書

`文書ID=MB-BUILD-SPEC-20261113`／`文書種別=施工仕様・既存論理設計の実装要求への射影`／`言語=日本語基底`／`基準日=2026-11-13 JST`／`対象=gatchimuchio/MINIDORA-BETA`／`観測HEAD=d50a3f559793222f84eca8f2266d557b806de746`／`対象実装=Rust successor 0.7.0-alpha.8以降`／`状態=Codexへ渡す施工候補・実装済みまたはQA済みとの宣言ではない`。

## 0. この文書の権限・正本・対象

`主語=MINIDORA-BETAの新規Rust後継実装`。`目的=MINIDORAの汎用言語・意味・判断・実行・記憶エンジンの全体系躯体を、通常の入力から実際の成果・作用・観測・帰還まで一度で通すこと`。`施工主体=Codex`。`設計主体=チャッピーが構成した施工仕様、採否権限はオーナー`。`成果の使用者=後継エンジンを利用する人・アプリケーション`。`最終責任・保証範囲の採否=オーナー`。

`仕様権限を分離`：

1. オーナー原文指示・安全/権限規則・本リポの`AGENTS.md`を保持する。
2. **唯一の論理アーキテクチャ正本**は`docs/redesign/01_SYSTEM_SPECIFICATION.md`（`MINIDORA-UNIFIED-SPEC-R2`）。本書はR3を宣言せず、R2を黙って改訂しない。
3. 能力・母集団・反証条件は`02_CAPABILITY_REGISTER.md`、`03_REQUIREMENT_TRACE.tsv`、`04_SOURCE_CENSUS.tsv`、`05_VERIFICATION_AND_EVIDENCE.md`、`06_LINEAGE_AND_DECISIONS.md`、`07_R1_AUDIT_CLOSURE.md`、`08_WIRE_SCHEMA.json`、`09_TEST_CENSUS.tsv`、`rust/audit/implementation-obligations.md`および現行監査台帳が参照正本。具体的な施工優先順だけを本書と同封工程表で固定する。
4. R2と施工文書が矛盾したときは、対象・版・衝突条項・論理的帰結を記録し、設計採否が必要な部分だけ停止。施工者が契約を縮小して辻褄を合わせてはならない。

`書込対象=本repoの認可された後継実装領域（原則rust/と新設の施工証跡）`。元の`NOTNN-LLM-MINIDORA`、他プロダクト、外部公開サービスは**参照のみ**。凍結`minidora-notnn==0.6.0b1`、元`src/`、旧テスト・配布・`SOURCE_LOCK.md`、`REPOSITORY_LOCK.md`、`RELEASE_MANIFEST.json`、`checksums.sha256`を保存。Git記録の不可逆な消去・私有HDSの露出・秘密の転載・外部送信は施工目的に含まれない。

## 1. 目的関数と禁止する代替目的

`P:=一般ユーザーの自然な要求→入力機→唯一Core→必要なnative機能/許可されたport→出力機→実consumer→レンダリング/効果のreceipt→Coreの元Goal判定→実用的成果または根拠付き停止`。

`最適化対象=Pの全系成立と、未成立条件を追跡できること`。`非目的=ファイル数/コミット数/テスト件数/局所機能件数/形式的PASS/既存IT慣習の再現/短いデモの成功`。

施工の第一到達点`SYSTEM_SHAPED`：通常の公開入口で、既存の要求・能力群を唯一の状態と責任体系へ接続し、必要な処理の生産者→消費者が実際に接続され、通常・拒否・停止・履歴・修正・復旧の全系経路を持つ状態。`BUILD_COVERED`：対象要求ごとに実装配置・実consumer・観測可能な出力/失敗と担当・出典が特定され、未完や権限待ちが型付きで残る状態。両者は`QC_PASS`でも`QA_SCOPED`でもない。正常系の代わりに`SUSPEND`だけができる実装は、その**正の能力要求**についてBUILD成立ではない。

`先行順序=設計の保持→全系BUILD→全系QC→保証対象別QA→オーナー公開判断`。BUILD中も安全/権限/不可逆効果の最小限の即時ゲートは保持する。局所試験の反復で全系BUILDを後回しにしてはならない。

## 2. 基本構造と責任境界（変更禁止）

| 構造 | 所有する仕事 | 所有してはならない仕事 | 外部可観測出力 |
|---|---|---|---|
| `INPUT_MACHINE` | 原文/bytes/span/出所/actor/版/許可ラベルの取り込み、意味差を保持した正規化、入力版ごと一回の型付き解釈、`InputBundle` seal | ゴール採否、学習、Core状態変更、外部作用、最終回答の採否 | 元入力・`MeaningIR`・曖昧性/不支持の診断 |
| `CORE_SINGLE_WRITER` | `GoalContract`、全意味/関係/推論、非ニューラル言語模型、記憶→推論→適応、行動計画、内容採用、唯一のcase revision、実効果の照合、最終`COMMIT` | port/CLI/HTTP/出力機へ同じ認知判断・学習・状態writerを分散 | 版付き`CaseRevision`・`ContentPlan`・`ActionRecord`・`EffectLedger`・型付き終端 |
| `OUTPUT_MACHINE` | 採用済み内容の媒体/bytesへの変換、意味保全観測、正確なlocal artifact、明示許可された配送、結果のreceipt | 内容の再判断、出所の捏造、目的変更、権限拡大、goalの勝手な`COMMIT` | 実際のbytes/hash/意味投影/ファイル/配送`RenderObservation`・`DeliveryReceipt` |
| `CAPABILITY_PORT` | 指定の型付き計算・観測・許可作用、資源量/出所/失敗の報告 | 隠れた第二のCore、秘密のLLMへのフォールバック、独自の学習/最終判断 | 観測値・証拠・効果receipt・`UNKNOWN/FAIL` |
| `STATE_ADAPTER` | Coreが所有する不変journal、blob、revision/CAS、active projectionの物理保存・回復 | 記憶原本の要約への置換、actor跨ぎの閲覧、外部作用との偽原子性 | 保存/再開/照合証跡・拒否/破損検知 |

機械間の真の正常経路：

`RequestEnvelope(original bytes,actor,permission)` → `InputBundle(sealed MeaningIR)` → `Core.bind(immutable GoalContract)` → `Core.plan(required artifacts/effects)` → `Core.execute(native/port)` → `Core.adopt(ContentPlan,evidence)` → `Output.render(actual bytes/artifact)` → `Core.validate(render meaning/required receipts)` → `Output.deliver(if separately authorized)` → `Core.reconcile()` → `COMMIT`。要件が未成立なら`SUSPEND/REJECT/FAIL/CANCEL`を原Goalと必要残件付きで返す。`ADOPTED != COMMIT`、`RENDERED != DELIVERED`、`ATTEMPTED != CONFIRMED`。

`case_id`ごと単一論理writer、input/evidence/goal/model/rule/capability/case/content/output/deliveryを**別々に版管理**。同一入力キー・異内容/actor/permissionは衝突拒否。古い結果の不正上書き防止。参照不能・無効化・中断は履歴を消さない。

## 3. 必須の実能力範囲（93記名IDは暫定・縮小不可）

`MUST_COVER_ALL=rust/audit/implementation-obligations.md §All 93 named obligations`。本節は横断配置であり、下記に列挙されない詳細義務も原台帳から消滅しない。発見済み416ソース・360 unique test pathの**意味契約**から追加要件が現れた場合はID増設、証拠と優先度を持ってBUILDへ組み入れる。

| 結合系 | 要求集合 | BUILDで実際に成立させる生産者→消費者 |
|---|---|---|
| `LANGUAGE` | `A01–A15` | 初見自然言語/多言語入力、主語・否定・条件・数量/範囲・引用/時制・照応・根拠/仮説区分→非ニューラルCore六要素→論理的意味と有用な説明/比較/変換→出力検査。構造化キーワードとテンプレ応答のみは未達。 |
| `TASK` | `B01–B13` | 質問/計算/統計/確率/数学/表/JSON/CSV/文書/論説/因果/コード/創作・編集/計画/研究を通常入口で解釈→能力を選択/合成→制約を満たす**実成果物**。B08は固定原文抜粋だけでなく未見の文章生成・改稿に至ること。 |
| `EVIDENCE` | `C01–C13` | 許可されたsource search listing→実本文fetch→body span/hash/時刻/independence→claim/contradiction/引用→採用または停止。LIVEと固定packetは混ぜず、429・timeout・drift・本文なしを証拠と区別。 |
| `MEMORY_LEARNING` | `CAP-D01–CAP-D14` | 永続原文/immutable ledger→bounded active memory→関係・推論→条件付きルール形成→**新規後続問題で適応が使用**→反例・訂正で依存だけ無効化→再起動後にも正しく追跡。保存しただけ/同一例再生は学習成立ではない。 |
| `CONTROL_EFFECT` | `E01–E12` | 唯一Core状態・goal・budget・行動グラフ→純粋計算・読取・許可書込→実effect receipt→停止/失敗/取消/UNKNOWN/再開→正しいgoal終端。外部効果は権限のない状態で起こさない。 |
| `CONSUMERS` | `F01–F14` | 凍結Pythonとの明示互換、CLI/HTTP/ブラウザ/会話/stream/音声・画像・動画の型付きadapter/独立アプリ連携/必要なcompiler/backend/研究port/現物demo→**一つのCore case/state**と各媒体の実観測。外部サービスや他repoの必要作業は権限境界で分ける。 |
| `ACCEPTANCE` | `G01–G12` | 元source/testの意味対応/実経路・品質・学習・非ニューラル性/証拠・再現・移植/資源・Cloud・公開の保証範囲→後工程QC/QAで別個に成立判定。BUILD中は測定可能な構造と経路を完成させる。 |

`LAYER0=TOKEN_SPACE / CONTEXT_STATE / PARAMETERIZED_TRANSFORM / CONDITIONAL_NEXT_TOKEN_DISTRIBUTION / SEQUENCE_OBJECTIVE / DECODING_IF`を**native非ニューラルCore内**に保持。候補選択のラッパーや既成回答の順序付けを全一般言語能力と誤認しない。外部LLM/専門家portを使う任意経路は`CORE_WITH_PORTS`として別計測し、`CORE_ONLY`と絶対に混ぜない。providerを無効化しても隠れた代替へ切替えない。

`PORTABILITY=OS/CPU/serialization/compiler/言語を論理上の固定条件にしない`。現時点のRust/SQLite/JSON等は物理適応。新backendと旧backendの演算・数値/Unicode・例外・actor・作用許可・source identity・状態遷移・副作用同値は実物のrefinementで示す。無観測OSへ保証を拡張しない。配布/起動可能性はOSごとにビルド/仮想適合試験、物理Windows 8GBについての別途確認/計測を持つ。全OSの物理実機網羅を工事必須にしない。

## 4. 観測可能な全系必須ルート

`ROUTE[R01]`: 自然な日本語の初見what/why/how/比較→意味IR→Coreの関係/言語/証拠→具体的な説明文→実consumer表示。単なる挨拶や既成定型文字列では不合格。

`ROUTE[R02]`: `2+3`→出力された`5`→「その計算を詳しく説明して」→同じ計算の手順→「3を4に変えて」→元の演算/入力と改訂の版差。出力機/別CLIインスタンスを跨いでも同一case系譜。

`ROUTE[R03]`: 相反する二主張・条件の向き・否定・主語差を持つ文章→前提・結論・根拠の分離→誤り/不確定を限定した比較→後続の前提改訂で依存項だけ更新。

`ROUTE[R04]`: 初見のユーザー原文からの文章作成・既存文章の意味条件付き編集・要約→purpose/style/format/字数/必須事項→実際に編集/生成したartifact→意味・引用・制約観測。原文だけの再掲/抜粋の正常動作は別能力の証拠に限る。

`ROUTE[R05]`: コードの説明/生成/変更/検査要求→該当ソース・制約→実コード/差分/実行可能範囲の正直な診断→成果。未実行試験をPASSと宣言しない。

`ROUTE[R06]`: 実ドキュメント/表・CSV/JSON/PDFの読取→計算/統合/文書合成→最終ファイル/画面→元ファイルと計算値の因果追跡。PDFテキストとOCR/画像認識を区別。

`ROUTE[R07]`: LIVE search listing→許可body取得→引用可能span/不一致→回答/判断→出典と取得失敗を記録。LIVE providerが使えなければ正直に`BLOCKED_AUTHORITY/NOT_RUN`を残し、固定packetで経路の実装接続は確認できるがLIVE成立と宣言しない。

`ROUTE[R08]`: 原文・観測・関係の保存→新規問題への適応→実際の行動/回答変化→反例で無効化→旧原本保存→復旧。gold/テスト期待値はCoreから隔離。

`ROUTE[R09]`: 現在のユーザー許可と一致するローカルファイル書込→実bytes/hash/receipt→Coreが元Goalを閉じる。権限拒否・書込失敗・重複/不明効果・取消は成功と分離。

`ROUTE[R10]`: CLI・HTTP・ブラウザ・他の適用可能consumerが同じactor/case revisionを再開→実際の回答と訂正が一致。streamのprovisional/final/invalidated、modalityの出所/時刻を保持。

`ROUTE[R11]`: crash/同時アクセス/二重入力/actor越境/外部書込`ATTEMPTED`後のUNKNOWN→安全停止・持続・reconcile・再開、禁止事項を厳守。multi-processでbusy/衝突の再現・改善条件を確認。

`ROUTE[R12]`: 少なくとも二つの独立した意味実装・対象projectionの固定ベクトル比較、release用ターゲットのインストール/起動/資源の観測、外部port ON/OFFの帰属比較。未実測対象は未実測。

`R01–R12`は施工時の統合ルート束であり、R2が指定する`S01–S12`および`R2-01–R2-22`を**置換しない**。BUILDの実装は両方の試験で観測できる必要がある。

## 5. セーフティ・同一性・停止契約

`COMMIT`条件：元のimmutably bound `GoalContract`が要求した実成果と必要receiptをCoreが受領・照合し、レンダリング後の命題/役割/否定/条件/数量/引用を対象verifier領域で確認したときのみ。rendererの自己申告・内部state変化・単なる`ContentPlan`採用・HTTP 200だけでは不可。

`EFFECT_CLASS=PURE|READ|LOCAL_WRITE|EXTERNAL_WRITE|IRREVERSIBLE`。効果記録：`actor,target,operation,goal,action_id,version,fingerprint,permission,scope,time,count,cost,egress,idempotency/reconcile key,status`。`PROPOSED→AUTHORIZED→PREPARED→ATTEMPTED→CONFIRMED|FAILED|UNKNOWN|CANCELLED`。実送信後のtimeoutは`UNKNOWN`、安全な同一識別子のreconcileができるまで**再送禁止**。`CANCEL`は将来作用を止めるが既作用を消さない。DB原子性は外部サービスへのexactly-once保証ではない。

`TRUST`：取得文書・引用・provider応答・コードコメントはデータ、命令/権限の源泉ではない。actor/case/source visibilityと秘密の伝搬を全読み・リプレイ・export・log経路で検査。入力・成果・秘密をproviderへ渡す際は許可した送信範囲だけ。衝突・意味変質・非許可作用・観測不能な不可逆効果は局所停止、独立した安全作業は続行。

`DURABILITY`：immutable原文/根拠/失敗/反例/履歴、active view、内容hash、Core revision、SQLite等の物理adapterを分離。失敗復旧ではstage/blob/DB commit間の切断、未参照blob、破損した参照、actor越境、同時writer、再実行の副作用を検査。

`BUDGET`：入力/evidence/steps/時間/外部作用/CPU/RSS/storageすべて有限・版付き。限界時は残件を示して型付き停止し、入力切捨て・証拠削除・成功偽装をしない。既存短時間予算をGPQA 90分等の別契約へ無断適用しない。

## 6. 既存実装の使用方法と技術負債の場所

`SOURCE_OBSERVATION(2026-11-13)`：現行HEADの`rust/`は型/保存/Core/限定数学/構造化資料/明示ソース要約/限定関係/CLI・loopback HTTP/非ニューラル有限模型を持ち、Rust `fmt/check/clippy/build`および`cargo test --workspace --all-features --locked`は**257件PASS（前回監査環境）**。独立診断では24一般質問・12文章・50会話turnの**計86応答がSUSPEND**、30明示意味比較のみCOMMIT（未採点）。この観測は「全系BUILD未成立」を示すが、最新版を新実行環境で再検査するまでは現在のPASS数と同一視しない。

`施工判断`：既存の局所数学やB08細目を再度一件ずつ完成→試験→報告する方式を禁止。価値ある既存コードを**同じCoreの一般要求実経路**へ再利用・統合し、欠けた自然言語/生成/意味/consumer/学習/実作用を横断してBUILDする。将来置換可能なport/IR/論理契約を守り、Rust選択を論理原理へ昇格しない。

`KNOWN_DEBT`：`checksums.repo.sha256`が240索引に減っている一方、文書は輸入済581テキスト資産を述べる。履歴上343行削除・2行変更の差がある。原凍結`checksums.sha256`ではなく現行文書索引の問題として証拠保全し、適切な文書完全性修復を**全体系BUILDの前提として直列化しない**。最終的なQC/QA公開整合前には対処必須。`source_contract_reviews.tsv`の行数/進捗表示差も定義と母集団を照合する。共有DBのbusyとstage一時パスIDの衝突はR11と全体系QCの欠陥候補。安全に関わる実施動作が確認された場合はその経路だけ即時隔離する。

## 7. 検証と完成宣言の階層

| 階層 | 意味 | 施工者に許される主張 |
|---|---|---|
| `CODE_PRESENT` | コード/契約/portが存在 | 存在のみ。能力成立を主張しない |
| `WIRED` | producerの実物をconsumerが受理 | 実際に観測した経路だけ |
| `SYSTEM_SHAPED` | 全系の責任/入出力/状態/作用/失敗が一つの躯体へ結合 | 後工程QC対象の躯体が成立（未検証を残す） |
| `BUILD_COVERED` | 全要求IDに実経路または正確な未成立/権限待ちの席がある | 要求の実装網羅の状態。positive欠如ならBUILD_GAPが残る |
| `QC_PASS` | 実物の正常/異常/結合/性能/堅牢性検査が必要範囲で通った | 固定版・環境・母集団の局所品質だけ |
| `QA_SCOPED` | 事前の合否規則/環境/対象版で保証可能 | 対象版・媒体・OS・機能を明示した限定品質 |
| `RELEASE_GO` | オーナーの別途判断 | Codexに自動付与されない |

`implementation_disposition`と`acceptance_status`を二軸で管理し、`UNVERIFIED`を一括PASSしない。93要求の`G01–G12`、S01–S12、R2-01–R2-22、独立24/12/10会話/30変換、GPQA Diamond198の固定条件は保持。GPQAは`LIVE_ONLY`、並列継承198問90分/旧基準40問、同Core逐次198問90分、goldは採点時のみ。別問題・固定packet・部分走行で歴史的正式成績を作らない。合否閾値はオーナーが決めるべき箇所を発明しない。

## 8. 施工仕様の充足定義

`SPEC_EXIT`＝(1) R2責任境界と全93記名要求＋発見要件の対応を保持、(2) 公開通常入口から各能力系へ**必要な正の経路**を実装して実consumerへ戻す、(3) 意味/作用/記憶/停止/復旧・権限境界の全系経路を配線、(4) すべての要求に`path+owner+producer+consumer+actual_observation_or_BUILD_GAP`を付与、(5) その全系実物を一括QCへ渡す。権限を新たに要する外部作用だけは`BLOCKED_AUTHORITY`として当該作用を止め、他のBUILDを続行。未完IDがある間、全93完全実装・完成率100%・QA/製品完成を宣言しない。

`CHANGE_POLICY`：観測に基づき施工方法/実装媒体/工順は更新可能。目的・保証母集団・Core唯一性・不変履歴・許可範囲をCodex判断で下げない。仕様本書は暫定であり、更新時は旧版と変更因果・影響・残件を版として残す。
