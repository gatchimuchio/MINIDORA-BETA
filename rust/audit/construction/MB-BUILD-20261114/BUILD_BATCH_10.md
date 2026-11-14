# BUILD_BATCH_10 — 中核の直接施工確認一

## 工事識別

- 対象: `gatchimuchio/MINIDORA-BETA` の Rust Core
- 着工時 `main` / `origin/main`: `07d4e9e074c0dcc0000fbe41995da9f99f6ba7a8`
- 作業単位: 三点セット第四工程の中核確認一。仮想関係の前提条件を一つ追加したときに結論が変わり、主語・scope・否定・出所を保持することを、Core API から一経路だけ確認する。
- 対象者と目的: `subject=MINIDORA Core`、`purpose=提示前提と条件の組合せから、根拠を失わず関係を導出または停止する`。
- producer → consumer: `Engine::execute` → `input::seal` → Core の規則・前提保存と relation query → `relations` の proof closure → `ResultRecord` / `ContentPlan` / actor memory。
- 許可と境界: 利用者が採用した三点セットの施工を継続。通常CLIを介さないin-process API確認、隔離した一時SQLite state、既存コード・証跡・台帳の更新。provider、network、local file effect、別repoは使わない。
- 期待結果: 欠落条件・主語差・scope差・反対極性では支持を作らず型付き停止または反証を返す。必要な同一scope条件が揃ったときだけ、直接前提と規則を依存記録に保持した導出を返す。

## 実行と観測

追加した施工用unit testは公開in-process `minidora_engine::Engine::execute` を使用した。通常の `RequestEnvelope` テキストは `input::seal` を通るため、これはCLIを通さない Core API 観測であり、parserを切り離した `MeaningIR` 直接注入試験ではない。relation経路から外部providerや専門機能を呼び出していない。

11件の入力で仮想lab内に「ceramicかつheatedならconductive」という二条件ruleを登録した。`Pico is a ceramic`だけでは`conductive`が`SUSPEND / MISSING_PREMISES`となった。`Pico is heated`を追加した後は`COMMIT / DERIVED`となり、二つの直接fact ID、rule ID、照会結果のderived identityが依存に残った。`Luma`への主語変更と`Pico`の`default` scopeへの変更はいずれも`MISSING_PREMISES`だった。

別subject `Fenn`には`ceramic`と`not heated`を提示した。肯定のheated質問は、否定前提のfact IDを保った`COMMIT / USER_PREMISE`として「Fennはheatedではない」を返した。`Fenn is conductive?`は欠落した肯定条件を満たしたことにせず`MISSING_PREMISES`で停止した。

導出fact identityは照会の`ResultRecord.dependencies`に現れた一方、永続actor memoryには自動採用されなかった。最後のmemory snapshotは直接提示の4 factsと宣言rule 1件で、全factsが`USER_PREMISE`だった。これは実観測として記録し、永続学習や導出factのmemory admissionとは解釈しない。

全11件の実`ResultRecord`、最終memory、出所fingerprint、render/content-plan識別子と依存は[直接Core観測証跡](../../bb-core-construction-observation-01-v1-validation.json)にある。失敗した初回テスト仮説も、derived identityがmemoryに保存されないと分かった実ログhashとともに同証跡に保持した。テストの期待を観測された契約へ直し、製品コードの振る舞いは変更していない。

## 確認結果

- `cargo fmt --all -- --check` — PASS
- focused direct Core test — PASS、1 test
- `cargo test -p minidora-engine --locked` — PASS、231 passed / 0 failed。doc tests 0件。
- `cargo check --workspace --all-targets --locked` — PASS
- `cargo clippy --workspace --all-targets --locked -- -D warnings` — PASS
- `cargo build --workspace --locked` — PASS
- 既存B08 assertion-depth経路を現行source・文書hashで再実行 — PASS、20 observations。証跡: [B08再実行](../../bb-context-pronoun-relation-assertion-depth-v1-regression-after-b10.json)
- Batch 06 follow-up経路を現行source・文書hashで再実行 — PASS、49 observations。証跡: [Batch 06再実行](../../bb-context-pronoun-relation-followup-v1-regression-after-b10.json)
- `git diff --check` — PASS。変更7ファイルのcredential-pattern scanは0件。追加依存なし、合成関係入力だけで、個人情報・外部出典データ・非公開HDS本文は含めていない。
- full `cargo test --workspace` — NOT_RUN。この単位は製品Core実装を変更せず、engine全231件、全target check/Clippy/buildに加え、影響する既存20件・49件の経路回帰を実施した。

実行版・source/doc hashes・実出力・試験ログhashは[検証証跡JSON](../../bb-core-construction-observation-01-v1-validation.json)に固定した。

## 要件・保証境界

この観測は三点セットの中核確認七件中の一件のみ。未知仮想資料の一般化、typed-IR直接注入、derived-fact admission、学習、外部情報との照合、他の六観測、三機経路、full R2 acceptanceを証明しない。関連するA05/A15行にはこのbounded observationだけ記録し、受入statusはいずれも`UNVERIFIED`。全93要件も未受入のまま。

## 変更対象

- `rust/crates/minidora-engine/src/core.rs` — `Engine::execute`を駆動する施工用unit testのみ。
- `rust/audit/bb-core-construction-observation-01-v1-validation.json` — 11入力、実結果・memory、環境、hash、ログ記録。
- `rust/audit/bb-context-pronoun-relation-assertion-depth-v1-regression-after-b10.json` — B08の現行source/doc hash付き回帰。
- `rust/audit/bb-context-pronoun-relation-followup-v1-regression-after-b10.json` — Batch 06の現行source/doc hash付き回帰。
- `rust/audit/IMPLEMENTATION_STATUS.md`、`rust/audit/requirements.tsv` — bounded観測と証跡参照のみ追記。
- 本報告。

三点セット原本4件と`MANIFEST.tsv`は不変。R2正本、Cargo依存、実装version、runtime APIは変更していない。

## 資産区分

- 保持: 利用者原本4件（ZIP 1件、施工Markdown 3件）。ハッシュとbyte lengthは既存manifestと一致し、原本は変更なし。
- 保存: 本単位で追加・更新した追跡対象7ファイル（上記のCore test、検証証跡、二つの回帰証跡、implementation status、requirements、B10報告）。
- 稼働: `rust/target/debug/minidora-rust`とengine test binaryの2 build出力。Cargo cacheとして利用中で、Gitには追加しない。
- Trash: hashを保存したB10一時ログ9件と、予備試験でassertion panicが起きてcleanupを通らなかった隔離SQLite scratch tree 3件をLinux Trashへ移動。先行Trash 33件（B06–B09由来32件と、11/13付の先行証跡1件）も保持され、永久消去していない。
- 保留: 新しい資産0件。受入statusと残る6観測は未完了の作業scopeとして継承する。
- 一時state: 最終successful testのSQLite directory tree 1件はtest終了時に閉じて自動削除。予備試験で残った3 treeは各101 files / 574,457 bytesで、内容hashを証跡化した後にTrashへ移動。利用者原本・既存stateは使っていない。

この単位の資産勘定: 保持4 + 保存7 + 稼働2 + Trash12 + 保留0 = 25件。成功試験の隔離scratch 1 treeはtest終了時にcleanupされた。先行Trashは空にしていない。

## GitHub保存と復旧

`main`のみでcommit/pushする。作業前の復旧点は`07d4e9e074c0dcc0000fbe41995da9f99f6ba7a8`。公開境界確認とdiff検査はPASS。commit直前にremote/main競合を再確認し、fast-forward後に`main`と二世代の復旧参照を照合する。PR・別branch・releaseは作成しない。
