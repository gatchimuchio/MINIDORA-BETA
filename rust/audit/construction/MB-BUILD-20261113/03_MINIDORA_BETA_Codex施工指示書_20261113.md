# MINIDORA-BETA｜Codex全体系BUILD施工指示書

`文書ID=MB-CODEX-ORDER-20261113`／`指示形式=Codexへ単独投入可能`／`基底言語=日本語`／`主語=施工者Codex`／`施工対象=gatchimuchio/MINIDORA-BETA`／`施工目的=MINIDORA新Rust後継の全体系BUILDを局所テスト積上げより先に完了させる`／`同封=01_MINIDORA_BETA_BUILD仕様書_20261113.md+02_MINIDORA_BETA_BUILD工程表_20261113.md`／`設計権限=MINIDORA-UNIFIED-SPEC-R2およびオーナー採否`。

## 00. 施工命令／最重要

**本指示書が実施工として提示された場合、施工者はMINIDORA-BETAの現在状態を観測し、全体系BUILDを主仕事として自律継続せよ。**

`DO := 先に全ての機械・能力・入口・結果・記憶・実作用・異常経路を唯一のCoreで接続し、使用者が通常入力から実成果を得られる躯体を通す。` 

`DO_NOT := B03→全suite→B04→全suite→B08→全suiteのような個別部品型の反復工順、仕様書/テスト一覧/スキーマ/fixtureの個数増を主成果とする作業。` 

`SEQ := OBSERVE → BODY-0 → BODY-1 → BODY-2 → BODY-3 → QC-1 → QC-2 → QA-1 → QA-2`。

`AUTHORITY := オーナーによる本施工指示の実行依頼は、その明示範囲における通常のコード/仕様/試験/コミットを包括承認する。外部送信・本repo外変更・資格情報取得・不可逆操作・公開release等の別権限を創作してはならない。` GitHub mainへのpushは今回の実行依頼と接続がそれを許す場合に実施し、push不能・接続不能なら未pushとして止めずに差分を保存/報告する。**この文書を作ったこと自体はGitHubへのpush実行命令ではない**。

## 01. 起点の検査と同一性

1. `repository := gatchimuchio/MINIDORA-BETA`。`branch := main`。観測時HEAD参考値`d50a3f559793222f84eca8f2266d557b806de746`（2026-11-13 JST）、現物の`git status`, `git log`, `git ls-remote origin refs/heads/main`で再確定。writer競合・dirty差分・untracked原資料は保護し、他者差分へ無断上書きしない。
2. `READ FIRST`：root`AGENTS.md`全文、`docs/redesign/01_SYSTEM_SPECIFICATION.md`（R2論理正本）全文、同封仕様書と工程表全文。`READ SUPPORT`：`02_CAPABILITY_REGISTER.md`、`03_REQUIREMENT_TRACE.tsv`、`04_SOURCE_CENSUS.tsv`、`05_VERIFICATION_AND_EVIDENCE.md`、`06_LINEAGE_AND_DECISIONS.md`、`07_R1_AUDIT_CLOSURE.md`、`08_WIRE_SCHEMA.json`、`09_TEST_CENSUS.tsv`、`rust/audit/implementation-obligations.md`、`rust/audit/requirements.tsv`、`rust/audit/source_contract_reviews.tsv`、`rust/audit/IMPLEMENTATION_STATUS.md`、`rust/audit/validation.json`、実装`rust/README.md`/実ソース/テスト/`rust/Cargo.toml`/`Cargo.lock`/`rust-toolchain.toml`。関連して旧source/test適用性を調査。
3. 以降の`BUILD_BATCH`切れ目では関連する正本節・現物差分・次の実consumer・安全境界だけを再同期。毎回全資料の再朗読・完全フルテストをしない。独立した設計改訂/衝突発見のときだけ読取集合を拡張。
4. 旧Python`minidora-notnn==0.6.0b1`、`src/`、凍結配布・履歴・既存`checksums.sha256`は改変禁止。別repo`NOTNN-LLM-MINIDORA`、BLUE-TANUKI、D4-POCKET、AI-compiler、konjac-Translation、MARS等へ書かない。元414+beta2=416、359 upstream test +147 beta test（146 shared）=360 unique、旧313回帰、記名93件を**別母集団**で保持。新しい意味契約を発見したらIDを追加。
5. `CONFIRM_VERSION`：Rustの導入版/edition/依存/SQL/adapterは現物観測で固定。現HEADで`0.7.0-alpha.8`/Rust`1.99.1`の観測は旧版記録であり、最新差分があればそれを採用。ライブラリ追加・更新は必要性/ライセンス/動作差/撤去条件を点検し、安易な新dependencyとhidden neural fallbackを禁止。

## 02. 施工の判断関数

`SELECT_NEXT_BATCH := argmax(通常ユーザー要求での未接続producer→consumer経路を伸ばす効果) subject to R2責任境界・安全・権限・歴史保全・必要な依存関係`。

`PRIORITY`：

1. 一般的な初見日本語Q&A/比較/説明や自然な執筆/編集を**実行できない**全体系欠陥を最上流で解消する。限定構造化文/数値演算/明示原文抽出だけが成立している現状を出発点として捉える。
2. 言語/意味/判断の出力を同じcaseのCore memory/Goalへ繋ぎ、後続の質問・訂正・創作・コード・研究へ使用する。部品に閉じた独立状態や二重推論を作らない。
3. 既存数学・統計・CSV/JSON・有限code・evidence-source機能を、普通の自然文指示→必要入力→後続能力→実出力へ組み込む。個々の機能の網羅深掘りは全系配線後のQCで実施する。
4. 実検索のsource list/body・引用・矛盾・失敗を一本にし、記憶→推論→適応を次の未知問題に結合。型付きport/on-offと単一Coreを守る。
5. 実レンダリング・ファイル/外部効果・一時/永続記憶・CLI/HTTP/ブラウザ/多媒体の各consumerを組み込む。作用の権限/UNKNOWN/回復は同時に立てる。
6. 以上の全系が閉じてから、構成単位で負例/資源/実OSのQC、第三者条件・ベンチのQAへ進む。

`UNACCEPTABLE_PROGRESS := {追加された行数,局所テスト件数,スキーマ数,JSON例の数,仮の実装表,未見質問をSUSPENDさせた件数}`。それらは作業証拠ではあり得るが、`BUILD完了`の指標ではない。

`ACTUAL_PROGRESS := {普通の入力,元Goal,中間producer artifact,次consumerによる使用,正しいCore採否,実表示/ファイル,actual receipt,制約の成否,未解決残件,revision}`。この実経路を定量/列挙する。

## 03. BODY-0：全機械を通す最初のBUILD命令

`EXECUTE_BATCH(BB0)`：

1. 既存CLI/HTTP通常入力を入力機のsealed`RequestEnvelope/InputBundle/MeaningIR`へ入れ、user原文/actor/permission/版を保存する。新しい自然な依頼にも応答経路を持たせる。専門モードprefixがなくてもCoreが目的/依頼を選べるようにする。
2. Coreは単一`GoalContract`に元目的/成果要件/禁止効果/予算/受入述語を保存する。入力の解釈曖昧さは勝手に埋めず、Coreの扱う未解決として残す。
3. 全現有能力を一つの登録/選択/接続図に置き、実演算/推論/検索/文書/文章/コード/学習/出力のproducerが何を生成し、どのconsumerが**実値**を使うかを固定。単なるダミーport/wrapperは使わない。
4. Coreが採用した`ContentPlan`だけを出力機が実bytes/artifactにし、Coreが`RenderObservation`と必要な`DeliveryReceipt`を照合して元Goalを`COMMIT`する。意味・出典・条件・主語・数字・仮説をrendererで変更しない。
5. 正常だけでなく`SUSPEND/REJECT/FAIL/CANCEL/UNKNOWN`を同じ状態/履歴に接続。後続ターンと別processのresume・修正が過去の実結果を参照できる構成にする。

`BB0_MIN_GATE`：Rust workspaceの対象部を型/format/check、少数の通常CLI/HTTP smoke。writer/permission/記憶/効果の変更点には関係する拒否/復旧テスト必須。**この時点で24+12+10+30やGPQA198・全OS実測を要求しない**。

`BB0_EXIT`：一つの本物のcaseが普通の入口から機械を縦断し、実体を伴う結果/停止とrevisionを出す。終了後、その**実観測**を次の各BUILD_BATCHの基線に使用。

## 04. BODY-1：部分機能列挙をせず全能力を縦に伸ばす

施工者は工程表の`BB-LANG/BB-CONTEXT/BB-CONTENT/BB-MATH-DATA/BB-SOURCE/BB-LEARN/BB-GOAL`を**同じBUILD期間**で横断実装する。順番はproducerの依存で決めるが、A01から1件ずつ局所検査する逐次列を作らない。

`CONNECT(BB-LANG)`：非ニューラルCoreの六要素が普通の未見自然文の解釈/生成に実際に使われること。意図/照応/条件/主語/否定/時制/根拠/不確定を失わず、一般の比較・説明・質問に**内容のある**返答を作る。構造化phraseと一致する時だけ成功する実装では不足。無観測の一般能力を名乗らず、当該正能力を作る施工を続ける。

`CONNECT(BB-CONTEXT)`：元の`5`、ソースに基づく文章、編集済み成果、userの訂正を同じcase revisionから参照。「詳しく」「さっきの答え」「条件を変更」等が真の過去成果へ作用。認知Coreの修正前/後の証拠を保持。streamや別clientに新しい判断主体を作らない。

`CONNECT(BB-CONTENT)`：初見依頼の作成/文章改稿/要約/コード生成・説明・診断を実際のソース/意図/制約へ作用。出力の文字数・書式・必須事項/禁止条件を採用段と描画後に分けて検査。固定原文抜粋は出典に基づく要約として有用だが、初見自由作文B08の完了と混同しない。

`CONNECT(BB-MATH-DATA)`：既存算術/記号/統計/CSV/JSON/テーブルを共通`GoalContract`の処理にし、計算結果を実際の報告・比較・改訂・ファイルへ接続。部分構造の正しさは利用するが、部品ごとの全suiteはここではしない。

`CONNECT(BB-SOURCE)`：許可されたWeb/local sourceを検索し、listingとbodyを**別々に取得・保存**し、実本文を内容形成へ反映。hash/span/時刻/独立性/429・redirect/欠落を維持。取得候補の見出しやsnippetだけから「出典に裏付けられた」と言わない。固定packet診断はLIVEと別型。必要な外部ネットワークが不許可/不通なら他工程を進め、対象のみ未観測で保持。

`CONNECT(BB-LEARN)`：immutable履歴から関連根拠を集め、Coreが関係/手続きを形成・適用し、**次の未見問題**で自然経路の結果/操作が変わったことを実測可能にする。反例でactive採用のみ停止/改訂、old evidenceは保持。単なるルール保存や学習例の再現から汎化と宣言しない。

`CONNECT(BB-GOAL)`：有限budgetの必要成果/必要effect graphで実際に中間成果を後段がconsumeし、`ADOPTED→render→実receipt→Core goal COMMIT`を正しく閉じる。拒否/取消/権限不足/不明効果/再起動を同じ機械で扱う。providerやrendererへ最終goal判定を譲らない。

`BODY1_EXIT`：普通の日本語要求を入り口に、少なくとも言語/文章/計算/文書/証拠/記憶/実効果の個別producerが**真のconsumerで使われた**全系実例と、残るBUILD_GAPが揃う。未見品質の採点はQC/QAで実施するが、正の実装がない枝をSUSPENDで「機能完成」としない。

## 05. BODY-2：利用可能な形に総接続

`CONNECT(CLIENTS)`：CLI・HTTP・ブラウザの通常リクエストから実出力/traceと同じcaseを扱う。ブラウザ表示は実userが使えるUIで確認するための施工。独立consumer用versioned APIを定め、他repo編集なしで連携可能な範囲までBETA側に実装。streamのprovisional/invalidated/finalを意味採用から分離する。

`CONNECT(PORTS)`：検索/文書/PDFテキスト、翻訳、audio transcript、OCR/video frames、外部software/call等を型付きportとして組み込む。実providerがないものは**実入力/出力を偽造せず**、ローカルに成立可能なadapter/validation/errorを完成し、LIVEの証拠不足を別記。秘密データの外送・実メッセージ・実デバイス操作は事前に一致する権限がない限り実行禁止。外部portの成果を非ニューラルCoreの素点に含めない。

`CONNECT(MIGRATION)`：旧Python public入口との互換契約/版違い、新Rust schema等の明示migration、将来AI-compiler等でCore実装を置換するための抽象意味・refinementを保持。旧凍結配布は一切上書きしない。`F13`は代替backendの**検証可能な差し替え境界**と、実際に存在する対象のみの証拠を作る。存在しないAI-compiler実用性を捏造しない。

`CONNECT(DURABILITY)`：同一case writer、CAS/transaction、actor境界、journal/blob、multi-process、一時ファイル名の一意性/排他性、crash後の効果状態維持を設計/実装のまま崩さない。前回観測でshared DB busy/stage名衝突候補が存在するので、他機能へ感染するものは安全な手当てをBUILDで行い、負荷・異常の網羅はQCに渡す。

`CONNECT(PORTABILITY)`：Rust/native OSの差を抽象契約へ閉じ込め、同じ意味ベクトル/ABI/format/errorを記述。Windows-first CPU/8GBとLinux/macOS適合の実証ルートを用意する。Windows実機は本人の環境で確認する独立ゲートにでき、**すべてのOSの物理実機を施工者が用意する必要はない**。ただし未観測OSをQA保証済みと表示しない。

`BODY2_EXIT`：誰が何を送受信し、何が権限で止まり、実作用がどこまで届くかを同じR2の系統で追跡できる。未選定provider/外部権限待ちは本当の未成立として台帳化し、勝手に良い結果を生成しない。

## 06. BODY-3：全体系施工終了を判定（QCはその後）

`VERIFY_BUILD_STRUCTURE`：

1. 全93記名要求（`A01–A15,B01–B13,C01–C13,CAP-D01–D14,E01–E12,F01–F14,G01–G12`）と新発見分を現行Rust実責任/普通の入口/producer/consumer/artifact/receipt/型付き失敗へ対応。非採用・重複・対象外は根拠とオーナー採否範囲が必要。ID自体の削除/改名で隠さない。
2. source416とtest union360の個別意味/入口/互換/移植可否/必要な試験を照合。元blob identityだけ、静的importだけ、テスト存在だけで済ませない。
3. `R01–R12`（同封仕様書）とR2`S01–S12`の入力経路を通常public entryから実行できる構成にし、作用/記憶/作成結果がconsumerに渡るか見る。正式な負例全件の実行はQCへ回す。
4. `R2-01–R2-22`に対し、破綻し得る箇所を特定。危険な権限/作用境界はBUILD中すでに止める。独立fault-injection網羅はQC。
5. BUILD完成を宣言できないID・実consumer未実装・明示権限待ちを`BUILD_GAP/BLOCKED_AUTHORITY/NOT_RUN`で分離。構造上の未完は**次のBUILD_BATCHを施工**し、局所テスト追加へ逃げない。

`BODY3_GATE`：全系の必要な正機能が存在し、実入力/実出力/効果/履歴/異常を通し、検査可能な状態。まだ検査品質を「製品品質」とは呼ばない。権限外のLIVE確認は待機のまま全系QC対象を限定し、保証に持ち込まない。

## 07. QC指示（BODYが接続されてから一括実施）

`QC_RUN := cargo fmt --all -- --check; cargo check --workspace --all-targets --locked; cargo clippy --workspace --all-targets --locked -- -D warnings; cargo test --workspace --all-features --locked; cargo build --workspace --locked`。コマンドは**`rust/`をworkdirとして**各別実行し、exit/対象SHA/実環境/実件数を保存。追加で実CLI/HTTP/ブラウザの正常・異常・再起動・実ファイルを起動確認する。ソース内例示の`/workspace`絶対パスを全OSへ強制しない。

`QC_CASES`：既存・新規の実物試験、S01–S12、R2-01–R2-22、原文の負例/意味変換/先行結果/新規作文/コード/論説/複数段/実本文/記憶学習/counterexample/actor/secret/重複/timeout/crash/取消/再送/低資源/同時実行。安全作用は事前/実施時の明示権限を保持。修正で契約の生成原因に遡り、修正前後同条件で反証する。

`QC_PERFORMANCE`：Windows CPU/8GB、VM/各対象OSの定義・コンパイル/実起動、cold/warm、p50/p95、peak RSS、storage、長期履歴、並行、断電/プロセス中断、保管・再開のデータ完全性を計測。CPU/OS/形式中立性は実装を通す共通意味contractで比較。仮想環境の合格を実機全数と混同しない。

`QC_DOC_INTEGRITY`：`checksums.repo.sha256`現240行と輸入済581記述との不整合を履歴・対象ファイル・SHA・改訂範囲で解消し、文書変更なら`DOCS_REVISION.md`等の現行訂正を残す。**`checksums.sha256`、旧`RELEASE_MANIFEST.json`、旧配布物を改竄しない**。これはBUILD初手に置く障害ではなく、QA/公開前に必要な統制補修。

## 08. QA指示（独立評価・保証対象を固定）

`FIXED_POPULATION`：独立初見一般Q&A24、文章12、4–6turn会話10、意味変換30、必要な逆境ケース。旧historical datasetを復元・item-level rubric/gold境界を確定し、作成済の別診断を正式旧母集団として装わない。

`INHERITANCE`：416ソース/360 union tests/旧313 selected regressionの適用を説明し、元Pythonの旧PASSをRustへコピーしない。R1 22件修正のR2論理仕様も、実装結果の証明ではない。

`GPQA`：198件全数`0…197`、dataset CSV SHA-256=`41d1213cd7a4998605a26c2798500652572007161b3a92817ba46b35befcd305`、shuffle seed`0`、Europe PMC/Crossref/Wikipedia(en)、OpenAlex OFF、`LIVE_ONLY`、goldが推論/学習へ入らないこと。`PARALLEL_INHERITANCE(198,<=90min,history_floor=40)`、`SERIAL_LEARNING(198,<=90min)`を別々に実測し、その個別答え/時刻/実sourceと現在SHAを記録。元29→80/40は過去の別実験の値。全問未完を合格にしない。権限/接続条件が足りないなら正確な未実行とし、偽装入力/固定packetで穴埋めしない。

`QA_SCOPE`：native/with-ports、API/client、OS/ABI/format、resource、外部effects、配布/Cloudと対象条件を分離。`QA_SCOPED/QA_NOT_ESTABLISHED/OUT_OF_SCOPE`と根拠を記録。オーナーの公開/正式release/性能表現採否をCodexが代行しない。

## 09. 証拠台帳・Git・停止の作法

`BATCH_RECORD`は各回以下の1レコードで足りる：

```text
{repo,base_HEAD,final_HEAD,phase,BUILD_BATCH,original_requirement_IDs,
 source_path_and_identity,changed_paths,previous_producer_gap,
 actual_input+digest,real_producer_value+hash,consumer_used_that_value,
 final_artifact/actual_bytes,core_goal_state,render/effect_receipts,
 test_commands+exit+test_pop,NOT_RUN+reason,known_FAIL/SUSPEND,
 permissions/secret_check,remaining_BUILD_GAP,next_consumer,rollback_commit}
```

`SOURCE_TRACE`：元414/新beta2の各sourceの`path/blob/意味/公開入口/側面効果/責任機械/consumer/対応Rust/必要試験/証拠`、360 test pathの対応を実際に照合して整備。`rust/audit/requirements.tsv`の`implementation_disposition`と`acceptance_status`を独立保持。要求の局所成功を全要求の`ACCEPTED`にしない。理論の非公開内容をpublic repoのsource crosswalkへ写さない。

`GIT`：既定`main`一本。無断branch/PR/自動CI/workflow/タグ追加/履歴書換禁止。変更対象がdirtyまたは他writerに動いたら対象のblob差分を照合し、上書きせず安全に再同期。意味のあるBUILD_BATCHの完了時だけ、差分/secret/公開境界点検・局所検証・認可されたcommit/pushを行う。commitを手段以上に目的化しない。現物が遠隔更新されていれば再base/比較して衝突を報告。`git reset --hard`・無断clean・force pushは禁止。

`BLOCK`：外部送信・deploy・公開release・他repo書込・秘密取得・不可逆削除・実物の勝手な仕様変更/母集団縮小/保証範囲変更は正当な新権限が無ければ行わない。必要な権限/相手/理由/結果を特定して**その作用のみ**保留し、権限内の独立BUILDは継続。安全/権限の境界が侵害されそうな局所経路は試験を後回しにせず即時停止/検証する。

`CONTINUE`：全体BUILDが閉じない限り、局所smoke成功/200件以上のRustテストPASS/文書完成/固定fixture成功/段階報告を最終完了理由にしない。各BUILD_BATCHの終了時は、実producer→consumerの次の欠落へ進める。ユーザーの逐次承認を要求するのはオーナー判断/権限の実変更が必要な場合だけ。自己提案による設計変更やScope縮小は実行しない。

`REPORT_AT_GATE`：`BUILD`には`SYSTEM_SHAPED/BUILD_COVERED/BUILD_GAP/BLOCKED_AUTHORITY`、`QC`には各実試験のPASS/FAIL/NOT_RUN、`QA`には対象別保証状態を使用。完成率%を出す場合は分母と合格意味を固定し、全コード/branch/テスト件数を完成割合に読み替えない。報告は主語/目的/実経路/観測/次の不足を先頭に書く。

## 10. 最終完了命令

`EXPECTED_FINAL := {R2の三機・唯一Core,記名93+追加要件の構造/実経路網羅,普通の日本語/文章/検索/計算/コード/学習/実作用の本物の成果,利用者入口と実receipt,安全な停止/復旧,source/test由来と履歴保全,全系QCの結果,分離QAの適用範囲,オーナー判断に必要な未成立一覧}`。

`EXIT`：未達を隠さず、権限外でないBODY未達なら工程へ戻って施工を続ける。権限外の作用は勝手に突破せず、対応する局所の証拠と未観測を残す。**最初にMINIDORA-BETAという機械を一本完成させ、その実物を検査する。**局所テストの積上げを完成品と取り違えない。
