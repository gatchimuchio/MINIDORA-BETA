# MINIDORA-BETA｜全体系BUILD主導施工工程表

`文書ID=MB-BUILD-PLAN-20261113`／`対象=MINIDORA-BETA Rust successor`／`基準HEAD=d50a3f559793222f84eca8f2266d557b806de746`／`基準日=2026-11-13 JST`／`基底=MB-BUILD-SPEC-20261113+MINIDORA-UNIFIED-SPEC-R2`／`主語=施工工程の順序・依存・実施/停止/帰還`／`目的=全系躯体を最短の合理的順序で閉じてからQC/QAへ進める`／`不採用=局所機能1件→全suite→局所報告→次機能を93回繰り返す工程`。

## 0. 工程の固定規則

`ORDER := QE設計確認→BUILD全体躯体→QC全系/局所原因検査→QA対象別保証→オーナー採否`。

`工程の最小単位=BUILD_BATCH`。一単位の価値は実コード量ではなく、`通常公開input→本物producer→Core採否→実consumer→artifact/receipt→元Goalの閉包`が**どこまで延長されたか**。同種の能力を別々に作り、連結を最後へ先送りしない。既存機能を実consumerへ繋ぐ施工、生成能力と意味保全を同時に立てる施工、学習と新問題への利用を同時に立てる施工を優先する。

`BUILD中の必須検査=コンパイル/型/format/配線直後の限定smoke + 安全・権限・復旧の変更点に必要な拒否/停止試験のみ`。**全テスト/全OS/独立品質/長時間ベンチ/長期耐久/全報告を毎小単位の前提にしない**。既存必須契約や安全ゲートを恒久skipする意味ではない。

`工程依存図`：

```text
OBSERVE(current HEAD, R2, source/test identity)
  -> BODY-0(全責任・状態・consumerの接続)
  -> BODY-1(意味・推論・生成・文章・数学・証拠・記憶を横断実装)
  -> BODY-2(実作用・多媒体・他consumer・回復・可搬の接続)
  -> BODY-3(全93+追加要件を一つの全系として閉包)
  -> QC-1(正常/負例/結合) -> QC-2(耐久/同時/資源/可搬/安全)
  -> QA-1(固定独立母集団+GPQA等) -> QA-2(保証範囲の判定)
  -> OWNER_RELEASE_DECISION（自動実施禁止）
```

`BODY-0～BODY-3`間は局所実装の並行連結・前後入替えを許容する。外部権限が必要な系の待機は独立BUILDを停止しない。BODY各期に**先に存在する局所数学/構造文書だけを磨く独立工程は設けない**。

## 1. 現物起点（OBSERVE / 事前点検）

| 契約 | 観測・措置 | 完了条件/出力 |
|---|---|---|
| `O01` | `remote/main` HEAD・working tree・変更中writer・対象Cargo/lock・Rust toolchain・権限・秘密境界を再確認。基準HEADは時点付きであり固定のまま盲信しない | 実作業のbase SHAと変更保護範囲 |
| `O02` | `AGENTS.md`、R2本体、能力台帳・出所・反証・過去欠陥・`rust/audit`を読んで現在要求ID/owner/公開entry/実consumer/観測を突合 | `capability→producer→consumer`未接続一覧。設計を再検討するために施工を止めない |
| `O03` | 416 source identities、test union 360、93 named obligations、歴史313回帰、R1監査22、S01–12、R2-01–22を実母集団として固定。既存`UNVERIFIED`を勝手にAcceptedへ変えない | 要求IDと監査ファイルの参照版。源ファイル意味の未完を追跡 |
| `O04` | 実際の一般Q&A/文章/会話は未成立、構造化テキスト/数学等局所経路は成立という現況を確認。最新観測と古い診断（257 tests、86 suspended等）を分離 | 最初に塞ぐべき**全体系上の欠落**と使用可能な既存部品の分離 |

`OBSERVE_EXIT`：既存ファイル保全・対象同一性・R2責任境界・安全/書込権限に重大不明なし。元414ファイルの全文意味レビューや全360テスト適用性判定をBODY前の**待機ゲートにしない**。当該機能に触る都度意味を観測し、全数の最終対応はBODY-3/QCまでに閉じる。

## 2. BODY-0：全体系の一枚の実働躯体（最優先）

`主語=MINIDORA後継エンジンの全系責任配線`。`前提=R2は論理設計正本。物理構成は既存Rustを第一候補として再利用。`

**同一BUILD_BATCH群で結合する接点：**

1. `ENTRY`: 普通のユーザー自然文、編集指示、計算、資料、コード、出力形式、複数段依頼を公開CLI/APIへ入れ、内部JSON操作名や専門コマンドをユーザーに強制しない。専用明示形式は補助APIとして残してよい。
2. `INPUT`: 原文/出所/actor/permission/spanを不可逆に失わず、目的/主語/関係/制約/時間/否定/参照を一回型付き`MeaningIR`へseal。解釈不足を残件として保持。
3. `CORE`: `GoalContract`の一意の主語/目的・成果/必要作用を保存、実能力のproducer-consumer graphとbudgetで実行、証拠/仮説を採否、memory/learningを同一revisionへ接続。
4. `OUTPUT`: 採用した内容を実bytesと表示可能artifactへ変換、Coreの実際の意味照合と必要receiptを通る。出力やCLI/HTTPに第二の最終判断を設けない。
5. `STATE`: 同一actor/caseの複数依頼・訂正・再起動を版で連続させる。先の実結果が後続の対象となる。
6. `CONTROL`: 正常`COMMIT`だけでなく本物の拒否・保留・失敗・取消・不明作用、復旧入口を同じ縦断経路で扱う。
7. `CONSUMER`: CLIとHTTPで実出力を見られる状態、ブラウザ・stream・独立consumerの共通Core接続位置を空論ではなく明示契約として配線する。

`BODY-0_EXIT`：`ユーザー→入力機→Core→少なくとも既存nativeの実演算/内容→出力機→CLI/HTTP実結果→Coreの観測`が**同じcase**で動き、別の通常要求や失敗/訂正も同じ配線に入る。まだ一般言語/文章生成/外部連携の個別品質が不十分でもよいが、未実装の本質機能を成功扱いするstub禁止。BODY-0を「設計文書作成済み」「型定義完成」で代替禁止。

## 3. BODY-1：全能力横断の機能接続（細部精度向上より先）

BODY-0の線上へ**横断的に**能力群を立ち上げる。下記`BATCH`は単一ソース単位ではなく、少なくとも通常入力・Core・必要consumer・保存観測までを含む束。並列に着手できる範囲を並べ、依存は明示する。

| 横断BUILD_BATCH | 同時に接続する要求・生産者→消費者 | 物理的成果/実際の次consumer | BUILDの局所判定 |
|---|---|---|---|
| `BB-LANG` | `A01–A06,A09–A10,A12–A15,B01,B05,B10–B11`。既存有限grammarと非ニューラルLayer0→意味保持→関係/推論→自然な説明・比較/一般応答→Core内容採用と出力検査 | 初見の普通の問いから**実内容の回答**。意味不確定な場合は名付けた不足のみ停止 | 定型文やキーワード拾いの増設だけなら`BUILD_GAP` |
| `BB-CONTEXT` | `A07–A08,A14,CAP-D01–D04,CAP-D09–D11,E01–E02,F04`。前回結果/議題/根拠→参照解決→訂正→新内容→同じcore ledger | 計算/資料/文章を説明・改稿する次ターン、別process再開 | context不在の拒否だけでは`WIRED`ではない |
| `BB-CONTENT` | `B07–B08,F05–F06,A11`。要求→文章/編集/要約/コード生成/検査→真のartifact、出典・許可した書込・形式を保持 | 初見の文章とコードの実成果、制約検査、CLI/HTTPで表示/取得 | 摘録/テンプレの正常運転を自由文章作成と混同しない |
| `BB-MATH-DATA` | `B02–B04,B06,B13`。自然文→数式/単位/表/CSV/JSON→実中間演算→後続報告/ファイル→結果説明 | 実計算と原資料を利用した次の能力への出力。既存数学を**組み込む** | 既存B03等の枝の試験件数だけを増やすと未達 |
| `BB-SOURCE` | `C01–C13,B05,B09`。search list→選択body fetch→本文span/hash/license→相反/欠落推論→Core採否→引用付き回答/研究文書 | 実取得された本文からの答え/比較、429等で根拠付き停止 | URL/検索snippets/固定文章のみならLIVEは未成立 |
| `BB-LEARN` | `CAP-D05–D08,CAP-D12–D14,A01–A03,B10–B11`。現物観測→memory→推論→scope付き新能力→**後続未知問題の行動/回答変化**→反例と依存改訂 | 元例とは異なる実問へ結果が使用され、以後の訂正/再起動で保持 | 記憶登録や同一例再生だけでは未達 |
| `BB-GOAL` | `E03–E12,B12–B13,F07`。目標/効果の有限graph→実能力の逐次消費→render→許可された作用→receipt→Coreの一意commit/再開 | 実効果まで達成された複数段要求、失敗時の残件/再試行 | Core内部進捗やActionResult注入だけなら未達 |

`BODY-1_EXECUTION`：全BATCHの最初の実出力を繋げ、欠陥を局所保有して後続を延長する。B08だけを延々深掘りしてBB-LANG/BB-LEARN/BB-SOURCEが空のままという工程を禁止。既存が実装済みなら新規作成でなく接続/再利用。**縦断実装を先に、テストの十分性は後段QC**。ただし危険な誤送信/actor越境/破損を観測した経路はその場で止める。

## 4. BODY-2：外部作用・複数consumer・実世界境界を結合

| 横断BUILD_BATCH | 施工と結合条件 | 非許可時の状態 |
|---|---|---|
| `BB-PORT` | 機能の型/版/権限/予算/出所/失敗/効果をport contract化。外部専門家on/offはCore独自能力と別。provider検索・本文取得は許可host/redirect/TLSで実施 | 対象の正確なportだけ`BLOCKED_AUTHORITY`。独立して開発できる仕様・local adapterは続行 |
| `BB-RENDER` | ブラウザ使用可能UI、実際に表示された回答とtrace/source、stream/undo/cancel、artifactの形式と差分検証。HTTP/CLIと同じcase/actorを使用 | 外部ホスト公開は別許可。ローカルUIを構築・検査できるなら続ける |
| `BB-MODAL` | `F08–F09,F06,S12`。audio/transcript/OCR/video framesを各々source/time/provider identity付き入力portで運ぶ。出力媒体変換も採用結果に紐付く | provider未選定・権限外なら未実測境界を明記し、native能力を偽装しない |
| `BB-INTEGRATE` | `F01,F10–F13`。凍結Pythonとの版付き互換、konjac-Translation/BLUE-TANUKI/D4Pocket等から利用できる公開契約、AI-compiler/代替backend候補との意味同値境界 | **他repoの実際の編集/デプロイはしない**。対象repoへの実接続検査に新権限が必要なら限定停止 |
| `BB-RECOVER` | crashの前後、multi-process、保存/再開、blob欠落、`ATTEMPTED→UNKNOWN`、同一actor/異actor、delivery receipt、fail-closed | 実不整合/危険を発見した作用を隔離。他の要求経路は進める |
| `BB-TARGET` | 共通抽象意味/入出力/エラーvector、独立projections、Windows優先の配布起動・Linux/macOSの規格/仮想適合を結合。特定OSライブラリにCoreの意味を閉じ込めない | 未確保OS/実装先は`NOT_RUN`。全OS実機網羅が前提にはならない |

`BODY-2_EXIT`：後継Coreと全公開consumer/必要port/効果/回復が単一の有向実経路に載り、各要求について成功か型付き未成立/権限待ちが示せる。UIや研究adapter等の実性能はQC/QA対象として保持。

## 5. BODY-3：総組立・検査引き渡し（BUILD終了ゲート）

1. `TRACE_FULL`：93名義IDに現物のsource/route/producer/consumer/主機owner/出力/観測/テスト予定/実装disposition/acceptanceを対応。source416を全数意味・使用/統合/置換/不採用理由へ突合し、test union360を実行/移植/不適用/未観測の型へ分ける。新発見能力のIDを登録。ID欠落禁止。
2. `BODY_CHECK`：R01–R12およびS01–S12に対し、現在公開入力から必要な結果/停止までのproducer→consumerのグラフを追う。`本物の中間出力→次の処理`がない枝、形式だけのport、二重判断、隠れたneural fallback、phantom deliveryを`BUILD_GAP`で残す。
3. `ACTUAL_ENTRY`：CLI/HTTP/ブラウザ等で適用する通常公開APIの入力・結果・履歴を観測。テスト専用スキャフォールドであれば`CODE_PRESENT`に留める。
4. `LEGACY_SEPARATION`：旧Python betaは改変なし、versioned別Rustのmigration/互換経路、既存識別子、データ・配布証拠の帰属を守る。
5. `QC_HANDOFF`：QAと区別した全系BUILD成果版・実動作・未成立ID・外部権限待ち・安全停止経路・必要な故障注入を一括で引渡す。

`BODY-3_EXIT`：必須正機能の`BUILD_GAP`が残る間は「全体系躯体完成」と宣言せず、**施工を継続**する。外部権限/物理検証のみで止まる枝はその理由と範囲を分け、権限内の実装/接続は閉じて次のQCへ進める。完成判定を得るために要求を縮小しない。

## 6. QC-1：全系を通した局所・相互作用検査

`START=BODY-3の単一候補版が存在`。`UNIT_OF_INSPECTION=原文入力/Goal/能力連結/実出力/receiptを通る一仕事`。

- `QC1-01`: 全S01–S12・R2-01–22、先に凍結した回帰・未見正常/逆転/曖昧/長文/数値/時制/条件/否定・再訂正の入力。
- `QC1-02`: Core唯一writer、input one-seal、sourceとclaimの別ID、Coreとoutputの意味保持。二重writer、代替portの無断昇格、暗黙再解釈を反証。
- `QC1-03`: 論説/数学/コード/文書/検索/文章/複数段の同一前提での整合・正当・一貫・実用妥当の区別。正解らしさと根拠を混同しない。
- `QC1-04`: 出力のbytes/hash/shape/引用/内容単位、受入先の表示、render失敗時のgoal停止、許可外write禁止、取消後の効果残存。
- `QC1-05`: 93件と新規要件のnegative/population対応。不足が発見されたら**上流の責任配置/全体施工へ戻す**。小局所修正だけで原因を隠さない。

`QC1_EXIT`：対象版、全試験母集団、実行された件数、失敗/残件、同一条件での是正再実施結果が記録済み。未合格を「全体躯体はあるからQAできる」と言い換えない。

## 7. QC-2：堅牢性・可搬性・実資源

- `QC2-01`: 同時writer・multi-process SQLite lock、stage-file一時識別子衝突、重複入力/port result、CAS、actor越境、再起動、lost blob、unexpected termination、idempotency replay。
- `QC2-02`: `ATTEMPTED`後timeout/UNKNOWN、同一再送防止、remote reconcile、file write race、出力/効果不一致。正規の権限を伴う対象のみLIVEで実施。
- `QC2-03`: input/active memory/evidence/steps/CPU/RSS/storage/時刻/コストの有限budget。Windows CPU/8GBの規格値と実測を区別。cold/warm・長履歴・長時間・エラー復旧でp50/p95を計測。
- `QC2-04`: 論理意味vectorを二つ以上の独立したtarget/backend/projectionで実行。Windows/Linux/macOS配布を対象OSのツールチェーン/ABI/仮想環境で照合し、実機で未観測の範囲は別記。Windows実機確認は利用可能なユーザー環境で最終確認するが、そこで仕様を恣意変更しない。
- `QC2-05`: 承認済provider host/redirect/TLS/credential/data-egress、コード・PDF・HTML処理、dependency/license/秘密公開と利用者説明の監査。
- `QC2-06`: 文書完全性（581対現行240 checksums）、source review数/文書数の齟齬、更新版配布証拠の再生成。`checksums.sha256`/凍結releaseは触らない。

`QC2_EXIT`：正常時より異常時を含む対象範囲、再現、停止、修復、保存、残件が観測され、外部権限無しの未走行は明記される。

## 8. QA-1 / QA-2：保証対象の成立判定

`QA1-01`: 旧原典・現行Rust・実行入口を固定した同一SHA。source416/test360/旧313を混同せず、必要な旧テストの適用可能性・実際の実行を確定。

`QA1-02`: 独立母集団`24 unseen Q&A + 12 writing + 10 sessions×4–6 turns + 30 meaning transformations`、事前固定rubric/gold非漏洩、source/effect追跡、未見正誤のitem-by-item記録。過去作成の116件独自診断は**履歴であって認可済み歴史母集団の代替ではない**。

`QA1-03`: 学習前後の別問題・counterexample・restart、native modelの六要素・非ニューラル性、port ON/OFFの同条件、LIVEと固定packetの別計測。

`QA1-04`: GPQA Diamond**198問全件、正規dataset SHA、seed0、LIVE_ONLY、gold遮断**。並列独立stateと逐次同Core学習を別々に各90分。歴史40/198は並列継承の旧下限、現在のRust成績と混同しない。LIVE provider未認可/到達不能なら`NOT_RUN/BLOCKED_AUTHORITY`のまま。

`QA1-05`: 使用する配布/OS/CPU/API/外部providerの実動作証跡。仮想テストと物理確認の保証範囲を表示、無観測のOSまで約束しない。

`QA2_DECISION`: `QA_SCOPED / QA_NOT_ESTABLISHED / OUT_OF_SCOPE`を、オーナーが採用した目標母集団/閾値・SHA・環境・媒体・有効な権限に対応づけて判定。母集団の割引や設定後出しの採点変更は禁止。総合百分率を出す場合は分母と判定条件を明示する。オーナーの公開/正式リリースGOは工程外の独立判断。

## 9. 工程監視と更新・終了条件

`STATUS_LEDGER := {stage,base_sha,final_sha,latest_batch,requirement_ids,touched_components,real_input,producer_output,consumer_artifact,effect_receipts,route_status,BUILD_GAP,authority_blockers,tests_lightweight,QC_tests,QA_population,NOT_RUN,known_FAIL,next_consumer}`。

各`BUILD_BATCH`では`次consumerに何を渡せたか`を1行で記録。関係する安全/権限以外でフルテストを実行しない理由は「不要な遅延回避」であり、検査省略の恒久化ではない。BATCHを跨いで進行できるなら中間報告のために施工を停止しない。

`REPLAN`：新たな契約矛盾・実装上の物理不能・安全違反・資源測定・新要求を観測したら、目的/母集団を固定して手順と組合せを再評価。時間/コミットを増やすことだけで妥当性としない。実装媒体はRustから将来AI-compilerへ交換可能な契約を保つ。

`BLOCK_LOCAL`：権限・外部送信・資格情報・actor隔離・消去・意味正本競合・不確かな不可逆作用を生む**その経路**だけ安全停止。安全なBUILDは別経路で続ける。全体を止めるのは原目的と母集団の確定不能、Core責任破綻、全系汚染/破壊のおそれなど実行継続自体が不適切な場合。

`FINAL_EXIT`: 全BODY実装→全QC実行→対象別QA判定→履歴/残件の説明がそろったとき、オーナーへ判断材料を提示。工程表の完了を自動的な製品公開・配布・品質完全保証と同義にしない。
