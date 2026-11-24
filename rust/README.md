# MINIDORA Rust successor

`0.7.0-alpha.11` is an independently versioned Rust development implementation of the public input → Core → output execution route. It contains bounded mathematical, declared-relation, Core-evaluated condition definitions, temporal-scope, Japanese meaning-comparison, durable-state, source-reading and exact-file-output mechanisms. It is **not an accepted implementation of all 93 capability obligations**. The [implementation status](audit/IMPLEMENTATION_STATUS.md) and [full obligations](audit/implementation-obligations.md) keep missing evidence and functionality visible; they do not reduce the requested scope.

The frozen Python `minidora-notnn==0.6.0b1`, its public Japanese identifiers, release records and original distribution hashes remain unchanged. This workspace lives entirely under `rust/`. Its records use the separate schema `minidora-rust/0.7.0-alpha.11`; they are not a silent migration of the beta's saved data or duplicate-input guarantees. Alpha 11 is incompatible with alpha 7 through alpha 10 Rust store metadata: ordinary opens return `SCHEMA_MISMATCH` and leave legacy files unchanged. The explicit `migrate` command verifies and upgrades alpha 7, alpha 8, alpha 9 or alpha 10 metadata while preserving old immutable records. Alpha 11 adds versioned Core condition semantics; legacy `condition_refs` remain present, and references without definitions evaluate to `UNKNOWN`.

**Current BUILD authority (2026-11-24):** [MINIDORA STANDARD BETA BUILD-v5 requirements, design, implementation schedule and work order](audit/construction/MB-BUILD-20261124-V5-STANDARD/README.md) together with [root AGENTS.md](../AGENTS.md). The BUILD integrates local Japanese conversation, durable state, specialist operations and permissioned Web retrieval under the same non-neural Rust Core. The `50f6514` record below describes the earlier scoped BUILD-v4 observation.

## STANDARD BETA v5 の起動

Rustを `rustup` で導入し、`cargo` を使えるようにする。初回ビルドは `rust-toolchain.toml` と `Cargo.lock` を使う。Linuxは対応するCコンパイラ、`pkg-config`、OpenSSL開発ヘッダーを用意する。WindowsのMSVC環境ではC++ビルドツールとWindows SDKを用意する。

```sh
git clone https://github.com/gatchimuchio/MINIDORA-BETA.git
cd MINIDORA-BETA
./start-minidora.sh
```

Windowsではクローンのルートから、PowerShellでは `.\start-minidora.cmd`、コマンドプロンプトでは `start-minidora.cmd` で起動する。既定の保存先は `rust/.data/standard-beta`。同じ `--db`、`--actor`、`--case` で起動すると会話を再開できる。ランチャーと直接起動したCLIで保存先を共有する場合は、`--db` に同じ絶対パスを指定する。`/new ID` で別の会話を始め、`/case ID` で切り替える。`/history` は履歴、`/status` は会話の保存版と処理状態、エンジン・API・schema・採用結果のモデル版を表示する。`/exit` で終了する。

以前の回答を使って追問や訂正をするには、`/history` に表示された結果IDを `/context 結果ID` で指定する。履歴には本文の冒頭と結果の状態が表示され、20件を超える場合は `/history page 2` などで続きが見られる。`/history 結果ID` で全文を確認できる。参照先は同じ会話の次の発話に適用され、`/context off` で解除できる。提示前提を説明するときは現在の記憶状態を照合し、失効した前提は履歴から参照できる。

例えば `こんにちは`、`もしXは鳥ならXは動物`、`ルリは鳥です`、`どうしてルリは動物なのですか` と順に入力する。表やコードなどの複数行の依頼は `/paste` で始め、本文の後の独立した行に `/end` と入力する。

`ミナは鳥です`、`ミナは鳥ですか`、`それは動物です` と続けた後、`詳しく説明して` と頼むと、主語を参照した結果と、保存した前提が現在も有効かを確認できる。

`それは動物です` の直後に `それは動物ですか` と聞くと、今追加した有効な前提から主語と範囲を引き継いで答える。別の発話を挟んだ後は、前提を追加したときの結果IDを `/context` で指定する。前提が後の訂正で失効していれば、その前提を使う新しい質問は保留する。

同じ会話で `それは動物です` の直後に `それは生物です` と続けると、直前の前提を照合して同じ主語を引き継ぐ。

`1 km + 500 m` と計算した後、`前回の計算結果をキロメートルに換算して` と入力すると `3/2 km` を返す。確定済みの長さの結果を照合し、`mm`、`cm`、`m`、`km` の間で正確に換算する。

`ミニドラとは何ですか？` は実行中のミニドラと標準APIの版を案内する。`例えば何ができますか` で対応する操作例を聞ける。

`Pythonで「(x + y) * 2」を返す関数 double_sum(x, y) を作ってください。` と入力すると、指定した引数を持つPython関数を生成する。式には引数、整数、`+`、`-`、`*`、`//`、`%` と単一の比較を使える。続けて `詳しく説明して` と聞くと、生成した関数と元の式に基づく説明を確認できる。

生成直後や、そのコードを `詳しく説明して` と頼んだ後に、`前に作ったコードで、x=3、y=4 の結果が14になるか確認して` と入力できる。名前付き整数引数と整数の期待値を1組指定すると、保存済みの関数を有限の式評価にかけ、実値と期待値を表示する。続けて別の引数で確認する場合も、同じ関数を参照できる。

確認結果の後に `詳しく説明して` と入力すると、保存された引数と実値・期待値の照合を説明する。その説明に続けて同じ関数を別の引数で確認できる。

貼り付けたPython関数を説明するには、次の行を順に入力する。対応構文を静的に読み、元のコードを解析記録に保持する。

```text
/paste
コードを説明して:
def square(x):
    return x * x
/end
```

文章を貼り付けて要約するには、同じように次の行を順に入力する。原文から最大3文を抽出する。続けて `詳しく説明して` と入力すると、選択理由と元本文の範囲を確認できる。

```text
/paste
次の文章を要約して:
図書館は火曜と木曜に開きます。
返却ポストは休館日も使えます。
/end
```

貼り付けた文章の文字列を置き換えるには、`/paste` の次の行に `文章の「猫」を「犬」に置き換えて:` と書き、その下に本文を貼る。対象が複数あるときは、`文章の「猫」を「犬」に2か所置き換えて:` のように件数を指定する。実際の一致数が指示と違う場合は保留する。置換後に `詳しく説明して` と入力すると、保存した置換記録から件数と原文・出力のバイト範囲を確認できる。

貼り付けたJSONから値を取り出すには、`/paste` の次の行に `JSONの「注文」の「配送先」の「市」を取り出して:` と書き、その下にJSONを貼る。キーは記載した順に照合し、選んだ値を原文のまま返す。`詳しく説明して` で選択した値のバイト範囲とハッシュを確認できる。配列の添字を指定するときは [JSON Pointer](#direct-https-source-bodies-and-exact-json-selection) を使う。

二つのCSV表を比較するには、同じ列構成の表を `変更前:` と `変更後:` の見出しで区切って貼り付ける。指定したキー列で対応する行を探し、変更・削除・追加を表示する。

```text
/paste
CSVを列「id」で2つの表を比較して:
変更前:
id,status
A,open
B,open
変更後:
id,status
A,closed
C,open
/end
```

CSVをJSON表に変換した後は、`前回のJSON表をCSVに変換して` で、保存済みのJSON表を使ってCSVを作れる。途中で別の話題を挟んだ場合は、JSON表の結果IDを `/context` で指定する。CSVに戻した結果の後に `詳しく説明して` と入力すると、元CSV・保存JSON・出力CSVの対応箇所を確認できる。

ローカル会話に外部LLMのキーは不要。Webを使うセッションは `./start-minidora.sh --allow-read-host ja.wikipedia.org` と明示し、`富士山について、出典を付けて教えてください。` と入力する。Windowsの起動方法でも同じオプションを使える。`/net off` で通信を停止し、`/net on` で起動時の許可を再適用する。本文は選んだ記事の観測範囲で使用する。記事の取得時刻で最新性や更新日時を確認できない場合は、回答を保留する。

検索一覧と本文の一部を保存した時点で取得量の上限に達した場合は、`1件目の本文を取得して` のように一覧の順位を指定できる。別の発話を挟んだ場合は `/context 結果ID` でその保留結果を参照する。元の一覧から選び、今回の通信許可と取得枠で本文を取得する。続く `詳しく説明して` で一覧と取得した本文の出典を確認できる。

許可したホストの対応本文は、`https://www.rfc-editor.org/rfc/rfc8259.txt を読んで` のようにURLを直接指定できる。全文が表示枠を超えると、取得した全文を保存し、出典・取得時刻・本文全体のハッシュ・冒頭の引用範囲を表示する。本文が取得予算に収まらない場合は保留する。

URLを直接指定して本文を取得した後に `詳しく説明して` と入力すると、保存した通信の結果と原文を照合し、取得先・全文の範囲・前回表示した範囲を説明する。通信を止めたまま、そこから同じ本文への質問や `前回の本文を要約して` に進める。

RFCのように改ページ文字を含む取得本文も、`前回の本文を要約して` で抜粋できる。続けて `詳しく説明して` と入力すると、選んだ文の原文バイト範囲とSHA-256を確認できる。

Web回答の後に `詳しく説明して` と聞くと、保存してある検索一覧、選んだ候補、取得本文、元の本文で使った範囲を照合する。その説明に続けて `前回の本文を要約して` と頼むと、同じ取得本文の冒頭から読み取れた文を抜粋し、対応するリンクや注記の記法を除いて表示する。抜粋は通信を止めたまま使え、元本文で使った範囲とハッシュを保存する。読み取れる文が指定の行数に足りない場合は保留する。

根拠説明の後に `本文では富士山について何と述べていますか` と聞き、その回答からさらに本文へ質問したり `前回の本文を2行で要約して` と頼んだりできる。別の話題を挟んだ後は、本文質問の回答を `/context` で指定する。質問に対応する記述が見つからず保留になった直後には、同じ本文への質問をやり直したり、要約を頼んだりできる。

`富士山の標高を出典付きで調べてキロメートルに換算して` では、同じ依頼の中で記事を探し、本文が「標高は … m」と述べる箇所を取り出して厳密に換算する。記事名・値・単位を確認できる場合に成立し、本文に別記された最高地点の値と区別する。否定・仮定・引用内の値は換算の根拠から外す。

ファイル出力を利用する場合は、起動時に `--allow-write` で信頼するローカルの絶対パスを指定する。対話で `/output PATH` を入力すると、同じパスを次の一発話の保存先に指定できる。`/output off` で取り消せる。保存を試みた発話では、結果本文に加えて対象パス・作用ID・受領状態を表示する。

Windowsの保存先には `C:\Users\利用者\Documents\結果.txt` のようなドライブ付き絶対パスを指定する。親ディレクトリは事前に用意する。UNC共有パス、特殊なデバイス名、ジャンクションを経由する保存先は保留または失敗になる。

対話中の `/reconcile` は現在の会話で元の保存作用を照合し、確定済みの場合は当時の受領記録を表示する。`中止` で取り消した会話は `/reconcile-effects` で残る作用を照合できる。照合に使うのは会話に記録された元のパス・内容・権限で、次の発話用の保存指定とは独立している。

### 標準API

Rustのconsumerは同じソースの `minidora-engine` と `minidora-domain` を使用する。`Engine::capabilities()` は `minidora-native/v1` のAPI版、保存schema、実装済みの提供能力と必要権限を返す。CLIでは `minidora-rust capabilities --json` から同じ情報を得られる。`Engine::execute(actor, RequestEnvelope)` で実行し、`ResultRecord` の `lifecycle`、`reason`、本文、出典、受領を利用する。`engine.case_status(actor, case)` は同じactorの最新結果と、採用済みの実モデル版を返す。

```rust
use minidora_domain::{RequestEnvelope, Result, ResultRecord};
use minidora_engine::Engine;

fn greet(engine: &mut Engine, actor: &str, input_id: &str) -> Result<ResultRecord> {
    engine.execute(actor, RequestEnvelope::new("demo", input_id, actor, "こんにちは"))
}
```

### v5 Linux実行基線（2026-11-24 JST）

| 固定対象 | 識別情報 |
|---|---|
| 実行コードと起動手順 | Git `fea2e0c9f7ad82fa492a2c7fb07d1a22090b6be6` |
| パッケージと契約 | `0.7.0-alpha.11`、API `minidora-native/v1`、保存schema `minidora-rust/0.7.0-alpha.11`。採用した模型の版は結果の `ContentPlan` に保存する。 |
| ビルド | Debian 13、`x86_64-unknown-linux-gnu`、Rust/Cargo `1.99.1`。`CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0` を設定し、上記ランチャーが `cargo run --locked` でビルドした。SQLite同梱、ネイティブTLSはシステムのOpenSSLを使用。 |
| 依存のSHA-256 | `Cargo.lock` は `b4e97fecdca8fd1fc863349b1a48eb99a925da7fed0ed6ce6cfe79bdd1cececc`。`rust-toolchain.toml` は `d13306be0ecda81ce779baa290ffb417380d525928477f16200884b5169cca33`。 |
| 実行物 | 作業環境の `/workspace/.minidora-cloud/build-v5-fea2e0c/minidora-rust`。SHA-256は `159f2e1f110175bfdc7b2b7c00de3092da7ac80fb1a656e5504f64ae8136b6b6`。 |

GitHubの新規クローンをLinuxで起動し、一つの対話で、日本語の挨拶と追問、保存規則からの推論、算術、JSON表、コード生成と説明を確認した。Web許可を切った発話は停止し、再度許可した発話では検索一覧・富士山の記事本文を実取得し、本文が「標高」と記した `3775.56 m` を `94389/25000 km` に換算した。回答と実ファイルの782バイト、保存受領、取消後の作用IDを照合し、再照合でファイルが再書込されないことを確認した。再起動確認で観測用の20秒制限に達した発話は取消で閉じ、同じ会話を再開して保存規則を別の対象に適用した。既存のUNKNOWN復旧の局所確認も実施した。

直接観測の対象はLinux x86_64。Windows起動スクリプトを同じソースに収録し、Windows実機起動は未実施。日本語の入力は対応文型に限られ、Web回答は許可された日本語Wikipediaの取得本文と照合できる内容に範囲を限る。提供機能と制限は `capabilities --json` に記載している。

### v5 継続修補版（2026-11-24 JST）

| 固定対象 | 識別情報 |
|---|---|
| 実行コード | Git `e67de00b40f5c5cf16879d4f3c95579830bb4aa7` |
| ビルド | Debian 13、Linux x86_64、Rust/Cargo `1.99.1`。`CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 cargo build --workspace --locked`。依存・API・保存schemaの識別は上記のv5基線と同一。 |
| 実行物 | 作業環境の `/workspace/.minidora-cloud/build-v5-e67de00/minidora-rust`。SHA-256は `4588670865a04255d93d3e4d4cbb82edafc80b3d48a30131ce4413dfd17534b1`。 |

Linuxの通常チャットで自己紹介と利用例の追問、貼り付けたPython関数の説明と許可付きファイル保存を確認した。解析記録に原文のCRLFとバイト範囲を保持し、保存した194バイトと受領記録のSHA-256を照合した。`reconcile` は同一結果を返した。既存のJSON形式のコード読取も成立した。対象外の `import` を含む説明依頼では作用が発生せず、同じ会話で計算を続けられた。

標高抽出では否定・仮定・引用内の値を採用しない回帰テストが通過した。日本語Wikipediaの検索一覧と富士山本文を実取得し、各HTTP 200と本文ハッシュ、記事の `3775.56 m` から `94389/25000 km` への換算を確認した。`UNKNOWN` から同一作用IDを照合する既存テストも通過した。

Windows用のディレクトリと既存ファイルの同期処理はWindows MSVC対象の標準ライブラリで型検査した。Windows実機での起動・保存・復旧と独立した実物QC・QAは未実施。

### v5 履歴参照・本文要約のLinux実行記録（2026-11-24 JST）

実行コードは `c98f763bfa965cef9481257b7b94f551c8a5cfee`。Debian 13のLinux x86_64で、上記の固定ツールチェーンと依存から `cargo build --workspace --locked` でビルドした。実行物は作業環境の `/workspace/.minidora-cloud/build-v5-c98f763/minidora-rust`、SHA-256は `76040cdcc0aa781d5c477b7369e983327ff4f8b73760c5cff1032be347a6c929`。APIと保存schemaの版は上記v5基線と同一。

再起動後に結果IDを指定し、直近の別結果を挟んだ過去の計算を説明した。過去の前提を指定した訂正では元発話が履歴に残り、有効な記憶だけが更新された。貼り付けた4文から3文を原文のまま抽出し、原文のCRLF、入力バイト範囲、保存した290バイトと受領記録を照合した。再起動後の要約への追問とCoreの再照合も成立した。既存のJSON要約は前の実行物と同一の `ResultRecord` を返した。

### v5 CSV改行対応後のLinux実行記録（2026-11-24 JST）

実行コードは `2235cc71c8d01148e1c2695cf2f2114610e16c05`。上記と同じLinux環境、依存、ツールチェーン、API、保存schemaでビルドした。実行物は作業環境の `/workspace/.minidora-cloud/build-v5-2235cc7/minidora-rust`、SHA-256は `fa5204666209f835c7b1b6f53b561d98eabb313df74fa9eea0a1f2df78af72d2`。

Linuxの端末でLFのCSVを貼り付け、正確な平均 `5/2` を得た。同じCoreのCSV入力10操作で、引用セル内の改行を含む元の本文、各処理の参照バイト範囲とハッシュを照合した。従来のCRLF入力10操作の `ResultRecord` は前版と一致した。統計・変換・比較・結合の追問では、元のCSVを再照合して説明し、許可付き保存の受領記録を確認した。

同じランチャーの一つの会話で、日本語の関係推論、計算、LFのCSV、本文要約、コード説明、通信の停止と再開を連続して使った。通信を再開した後、許可された日本語Wikipediaの検索一覧と実本文をHTTP 200で取得し、標高の換算回答を保存した。保存した782バイトと受領記録は、保持したWeb証拠・ファイルを使って再照合できた。Windows実機での起動・保存・復旧は環境未確保のため未実施。

### v5 前提説明・関数生成・出典要求のLinux実行記録（2026-11-24 JST）

実行コードは `0968238153666900bf78df5a53c14b6fa10fb327`。上記と同じLinux環境・依存・ツールチェーンでビルドした。APIと保存schemaの版は上記v5基線と同一。実行物は `/workspace/.minidora-cloud/build-v5-0968238/minidora-rust`、SHA-256は `3be74d534b98cc6187aeb32f9f56438b522f0cb64da1f951e0010cb9ed582ebc`。

再起動後のチャットで保存済み前提の説明を確認した。訂正で前提が失効した後の新しい追問は `CONTEXT_PROOF_CHANGED` で止まり、元の回答は履歴に残った。訂正前に保存した説明は、回答時の記憶版で既存ファイルを再照合できた。

通常チャットの日本語依頼で `double_sum(x, y)` を生成し、入力の式・引数、許可付きファイルと受領記録を照合した。保存済みソースの有限評価で、指定した2ケースの `22` と `-2` が一致した。従来の構造化コード生成は前版と同一の `ResultRecord` を返した。

「富士山について、出典を付けて教えてください。」で検索一覧と本文のHTTP 200、本文ハッシュ、回答を保存した535バイトと受領記録を照合した。未許可の通信と、記事の取得時刻では答えられない最新性の要求は保留になった。Windows実機での起動・保存・復旧は未実施。

### v5 取消・保存照合・Web根拠のLinux実行記録（2026-11-24 JST）

実行コードは `f10561c8577ee7f46500acb3c3b6e70731e7e757`。上記と同じLinux環境・依存・ツールチェーンでビルドした。実行物は `/workspace/.minidora-cloud/build-v5-f10561c/minidora-rust`、SHA-256は `0b4988fce49b002aa2c1af45bcf4782921f27b88f9bac2cc9ef43cf812ad7305`。

Linux CLIの並行実行で、保留された入力を同じIDで再送しても同じ `SUSPEND` が返ることを確認した。`停止` 後は元の実行と同じIDでの再送に同一の `CANCEL` が返り、新規入力で計算を続けられた。チャットで受領状態と元の作用IDを表示し、照合でファイルの再書き込みがなく、次の発話用の保存指定を消費しないことを確認した。

富士山の保存済み回答への追問では、新規通信なしで検索語・候補順位・取得時刻・本文の採用範囲とそのSHA-256を照合した。終了タグだけが候補になった本文要約は新規処理で停止し、以前の保存結果は同じIDで取得できた。

Windowsの保存と復旧でドライブ付きパスを扱う共通処理は、固定RustのWindows MSVC対象で型検査した。Linuxでパス文法を確認し、バックスラッシュを含むパスとシンボリックリンク経由の書き込みは拒否した。エンジン全体のWindows向けオフライン検査は未取得の `schannel v0.1.30` で停止し、Windows実機での起動・保存・復旧は未実施。

### v5 Wikipedia本文の冒頭抜粋とLinux起動の記録（2026-11-24 JST）

実行コードは `f01a10445e9f7bfa8d753c531aed344e15589aa6`。上記と同じLinux環境・依存・ツールチェーンを使い、`cargo build --workspace --locked` をオフラインで実行した。実行物は `/workspace/.minidora-cloud/build-v5-f01a104/minidora-rust`、SHA-256は `bc23f622ca4a1d8352bc45ff6c6a392145301441e9eb6fe0f213c53cfc57d1b5`。

保存済みの富士山本文から記事冒頭の3文をチャットで抜粋し、原本文と各範囲のSHA-256を照合した。新規通信なしで保存した728バイトは受領記録と一致し、再起動後の追問で同じ採用範囲を確認した。行数指定では2行と8行の出力を確認した。候補が5文に制限された状態で8行を求めると、理由を付けて保留した。従来の原文からの抽出結果は、保存済みの方式で再照合できた。

空白を含むパスに `c0624c5` を新規クローンし、依存キャッシュを使ったオフライン起動で日本語の挨拶と記憶の再開を確認した。`f01a104` に更新したクローンでも既定の保存先から同じ会話を再開し、標準APIの版と抜粋機能の表示を確認した。Windows実機での起動・保存・復旧は未実施。

### v5 Webの根拠説明から本文抜粋までのLinux実行記録（2026-11-24 JST）

実行コードは `82220c0b94b6306b88d5b8bb063cbc2a25f57196`。上記と同じLinux環境・依存・ツールチェーンで、オフラインの `cargo build --workspace --locked` を実行した。実行物は `/workspace/.minidora-cloud/build-v5-82220c0/minidora-rust`、SHA-256は `adc9c2ef787870422da892d2c434bc09ca929809a143344e6ea00c2543389a5b`。

一つの通常チャットで、保存前提と規則からの推論、`17*23`、LFのCSV集計、貼り付けた文章の要約、Python関数の生成と指定入力の有限評価を行った。通信を止めた依頼は許可不足で保留し、許可後は日本語Wikipediaの検索一覧と富士山本文をHTTP 200で実取得して標高を換算した。続けて通信を止め、根拠の説明、同じ本文の抜粋、抜粋箇所のSHA-256の再確認まで連続して行った。回答782バイトと抜粋728バイトは、保存ファイルと受領記録が一致した。

別のRustプログラムから同じNative APIを使い、保存済み結果の同一IDでの取得、文脈の明示指定、許可されていない保存の停止を確認した。同じケースで以前のWeb根拠説明を指定した要約も、通信なしで元の取得本文を使った。Nativeで作成した状態は同版のCLIから読めた。Windows実機での起動・保存・復旧は未実施。

### v5 Web本文への連続質問と保存照合のLinux実行記録

実行コードは `b9058a10c7a2d7cbe699c2288b0f76c3164ac9f2`。上記と同じLinux環境、依存、ツールチェーンで `cargo build --workspace --locked --offline` を実行した。実行物は `/workspace/.minidora-cloud/build-v5-b9058a1/minidora-rust`、SHA-256は `1ee7a38a5e4679c9e51a0de7827933cd76ab725b3efa717b7a3303ae4222ddd1`。

保存済みの富士山本文を根拠に、通信を停止したチャットで根拠説明から本文への質問を2回続け、同じ本文の通常要約と指定2行の要約をそれぞれ作成した。元本文と各採用範囲のSHA-256を照合した。

通常要約728バイトの保存ではファイル同期直後にプロセスを終了した。2行要約507バイトの保存では同期エラーを注入し、作用が `UNKNOWN` になることを確認した。両方とも、別プロセスの `reconcile` が元の作用ID・権限・パス・本文を照合して `COMMIT` に進んだ。ファイルのバイト列と更新時刻は同一で、照合前の新規保存と別actorからの照合は拒否された。復旧した要約の根拠説明と、前版の保存済み受領の再取得も確認した。Windows実機での起動・保存・復旧は未実施。

### v5 Linuxチャットの履歴・前提・本文・コード確認

実行コードは `e354491423cc5bf3f54cd8e1292bf8619ced239f`。固定依存とツールチェーンで `cargo build --workspace --locked --offline` を実行した。Linux x86_64の実行物は `/workspace/.minidora-cloud/build-v5-e354491/minidora-rust`、SHA-256は `3317fb30e9634216bb58795da336a71079275d746222e8b91c2c18b8cb8e9e97`。APIと保存schemaの版は上記のv5基線と同じ。

旧版の24発話の会話を開き、履歴の2ページ目から計算結果を選んで元の結果を説明できた。Linuxランチャーからの通常チャットで、計算、代名詞で記憶した前提の説明、生成関数の整数入力による確認を連続して行った。前提の訂正後は新しい説明を保留し、すでに書き込まれた未確定の説明は採用時の記憶版と同じ作用IDで照合できた。

保存済みの富士山本文への質問を結果IDで参照し、別の話題を挟んだ後も同じ本文を使った。本文で語句が見つからなかった後の質問と2行要約も追加通信なしで行い、元本文の採用範囲を照合した。2行要約の保存で同期エラーを注入し、`reconcile` で同じ作用IDと507バイトを再書き込みせず確定した。

生成コードは、自然文の整数引数と期待値を保存済みの関数に適用し、一致と不一致の両方を表示した。元の入力のUTF-8範囲を記録し、重複引数や未完の依頼は評価前に保留した。従来のJSON入力は前版と同じ結果を返した。

### v5 前提の引き継ぎと専門機能を連続使用したLinux実行記録

実行コードは `d41949d0c8becff3475ef5e1f958b08ae8b40dca`。固定依存とツールチェーンで `cargo build --workspace --locked --offline` を実行した。Linux x86_64の実行物は `/workspace/.minidora-cloud/build-v5-d41949d/minidora-rust`、SHA-256は `34303c1cb37acba50b8b7afc3e75f0a0c24632a734d469ce973c6f73b252e180`。

一つの通常チャットで、代名詞を使った二つ目の前提登録、後続質問への適用、算術、日本語見出し付きCSVの比較と根拠説明、同じ生成関数の連続確認、許可済みの本文取得を行った。CSVは変更・削除・追加を表示し、引用セル内の見出しをセル値として扱った。旧CSVの封印入力と比較レポートは前版の保存結果と一致した。

Native APIで旧版のコード検証結果を指定し、CLIからの次の確認でも同じ生成コードを使った。CSV比較と連続コード検証では、保存先の同期エラー後に同じ作用IDとファイル内容を照合し、再書き込みなしで確定した。

RFC 8259の対応平文をHTTP 200で取得し、保存した28,360バイト全体と表示引用の先頭512バイトをハッシュで照合した。富士山の取得本文では、UTF-8文字の途中を避けて511バイトを引用した。RFCの本文への質問は追加通信なしで成立し、改ページ文字を含む本文の要約は保留になった。本文表示を保存した989バイトは、同期エラー後に元の作用IDと引用範囲を照合し、再書き込みなしで確定した。短い本文は前版と同じ表示を返した。

### v5 改ページ付き本文・貼付置換・コード説明のLinux実行記録

実行コードは `010a9fe8458a1aa0d286e46f74cff4a4df53297f`。固定依存とツールチェーンで `cargo build --workspace --locked --offline` を実行した。Linux x86_64の実行物は `/workspace/.minidora-cloud/build-v5-010a9fe/minidora-rust`、SHA-256は `1cbbbec05015a8a79d302f8751fdea64bde2404e43984695552b504f62c82c16`。

一つの通常チャットで、前提の記憶と算術、日本語による貼付文章の置換、生成コードの説明から始めた二回の確認、RFCの実取得と通信停止後の要約・根拠説明を行った。RFC 8259の原文28,360バイトと16個の改ページを保持し、通常3文と指定2行の原文範囲・SHA-256を照合した。このRFCの行数指定は候補を走査する処理予算が必要で、`--max-actions 2048` で2行を確認した。315バイトの要約ファイルは、同期エラー後に元の作用IDと内容を照合して再書き込みなしで確定した。

貼付文章の置換はCRLFとUTF-8の範囲を保持し、件数を省いた曖昧な入力は保留になった。従来の置換形式は前版の同一入力と封印・結果が一致した。外部のRustプログラムからNative APIで保存済みのコード説明を参照し、続くCLIでも同じ生成コードを確認した。置換とコード確認のファイル出力も、同期エラー後に元の作用IDで照合できた。

### v5 保存済みの結果を引き継ぐLinux実行記録

実行コードは `e8660579a9440369fe33b773445c8f95c1e17dfb`。固定依存とツールチェーンで `cargo build --workspace --locked --offline` を実行した。Linux x86_64の実行物は `/workspace/.minidora-cloud/build-v5-e866057/minidora-rust`、SHA-256は `6671b579de183b950488a8d233727da4c3163d6910e3310a5b8d9fe86a4792ac`。

一つの通常チャットで、前提の記憶と算術、置換の根拠説明、保存したJSON表のCSV変換、生成コードの検証・説明・再検証、RFCの実取得と通信停止後の取得記録の説明を行った。置換説明は保存済み原文・出力のバイト範囲とSHA-256を照合し、詳細を先頭8箇所まで表示する。CSVの往復では `001` と `9007199254740993` を文字列として保持した。

RFC 1149の全文表示と、RFC 8259の28,360バイトの原文から表示した冒頭512バイトを、保存した通信記録と照合した。生成コードは、Native APIで回復済みの検証説明を指定し、通常CLIでも同じ関数を確認した。旧版の説明は同一入力で当時の結果を返した。

今回の置換説明・表変換・コード検証説明・取得記録説明のファイル出力で同期エラーを注入し、元の作用IDと内容を使って再書き込みなしで確定した。

### v5 単位換算と保存済み資料・表の追問のLinux実行記録

実行コードは `5e5d31e90af1788fdbd460568fdbf3e7413dd53a`。固定依存とツールチェーンで `cargo build --workspace --locked --offline` を実行した。Linux x86_64の実行物は `/workspace/.minidora-cloud/build-v5-5e5d31e/minidora-rust`、SHA-256は `74727569a7843af7b7587a0c29d8e46c483a35d5ff641daf2e4bd07152127d6b`。

一つの通常チャットで、前提の記憶、`1500 m` から `3/2 km` への換算、説明を挟んだ `150000 cm` への換算、CSVと保存JSONの往復・範囲説明、RFC 8259の実取得と通信停止後の説明・要約を行った。長さ以外の単位の換算は保留し、従来の計算訂正は前版と同じ結果を返した。表の説明では元CSV、JSON Pointer、出力CSVのバイト範囲を照合した。

RFCの取得説明を明示指定した本文質問では、最初に表示した512バイトより後ろの原文も参照できた。保存済みの旧入力は当時の結果を返した。換算・CSV往復の説明・本文要約の保存では同期エラー後の元作用IDと内容を照合し、再書き込みなしで確定した。

同じ版のNative APIとCLIの `/status` で、未作成の会話、結果未採用の実 `RUNNING`、確定後の `COMMIT` を区別した。未許可の保存では会話の状態と結果の保留理由を分けて確認し、別actorの読み出しは拒否された。

### v5 JSONキー抽出・検索候補・同時入力のLinux実行記録

実行コードは `250904fcc33a2bb6e6d5448b7046e7cbded508fc`。上記の固定ツールチェーンと依存で `cargo build --workspace --locked --offline` を実行した。Linux x86_64の実行物は `/workspace/.minidora-cloud/build-v5-250904f/minidora-rust`、SHA-256は `6989521f41c9414da926181565fd5af00cbaa7c7aac4f69d6b952e8e81ca1949`。APIは `minidora-native/v1`、保存schemaは `minidora-rust/0.7.0-alpha.11`。

通常チャットとNative APIで、貼り付けたJSONから指定キーの値を原文どおりに抽出し、追問で原文のバイト範囲とSHA-256を確認した。`~` と `/` を含むキー、指数表記 `1.2300e+40`、キー指定の途中で配列に行き当たる保留を確認した。同じチャットで関係の記憶を使った回答、長さの換算、RFC 8259の実取得と通信停止後の根拠説明も成立した。

富士山の検索一覧5,386バイトと部分本文2,806バイトを残した既存の保留履歴を使い、順位を指定して一覧を照合し、本文129,427バイトをHTTP 200で取得した。元の保留結果は同じ入力IDで取得できた。許可不足と今回の取得枠不足は保留し、本文は採用しなかった。ファイル出力の同期エラー後は、保存済みの内容と元の作用IDを別プロセスで照合し、348バイトを再書き込みせずに確定した。

同じ会話に別IDで同時入力したときは、先に受理された入力だけがファイルを出力し、競合した側は未実行の保留結果を保存した。両方の同一ID再送と、別IDで処理を続けられることを確認した。前提を代名詞で追加した直後に、同じ代名詞でその前提を問い直す経路には `AMBIGUOUS_RELATION_CONTEXT` の保留が残る。Windows実機は未実施。

### v5 代名詞で追加した前提を問い直すLinux実行記録

実行コードは `de592d4b7c958186a3cdca2cad0da6590c1e0c72`。固定依存とツールチェーンで `cargo build --workspace --locked --offline` を実行した。Linux x86_64の実行物は `/workspace/.minidora-cloud/build-v5-de592d4/minidora-rust`、SHA-256は `4c73aa89cbd71d6ca3c95a6d9d5378b427af3a5f6faa36dc6513ac7485f7a3ca`。APIは `minidora-native/v1`、保存schemaは `minidora-rust/0.7.0-alpha.11`。

通常チャットと再起動後のNative APIで、`それは動物です` の直後の問いから追加した前提のIDを照合し、主語「ミナ」と保存時の範囲を引き継いだ。明示した結果IDでも同じ前提を使い、範囲が異なる場合や前提が失効した場合は保留した。古い入力の同一ID再送は、保存済みの旧結果を返した。

関係質問と、それに続く根拠説明のファイル出力で同期結果を不明にした後、元の前提が有効なら同じ作用IDの実ファイルを再書き込みせずに確定した。同じ使用者が別会話から前提を訂正して失効させた場合は、出力済みのバイトと作用IDを保持したまま、再照合を保留した。

同じ通常ランチャーの一つの会話で、この問い直しに続けて分数計算、JSONのキー抽出、許可した日本語Wikipediaの検索と本文取得、標高の換算を確認した。通信を停止した状態で出典説明を保存し、1,544バイトの実ファイルと同じ作用IDの受領記録を照合した。

## 過去のBUILD-v4限定固定記録（2026-11-24）

以下は`50f6514`で記録した限定的なLinuxビルド、算術・Native/CLI、保存・許可付きローカルファイル出力の観測であり、通常日本語チャット、多ターン記憶、自律Web候補→実本文取得、専門Portの対話統合、clone後の簡易起動が一体成立したという観測ではない。v5の完成判定には使用できない。

MINIDORAを独立したRustエンジンとしてビルドし、同一ソース版のNative API、CLI実行物、保存契約と依存を固定した。固定対象のパッケージ版は `0.7.0-alpha.11`。

| 固定対象 | 識別情報 |
|---|---|
| 実行コードのGit基線 | `0d1a4c1929d806290497c87e3adfff0b2b7f3840` |
| 保存・正準化 | `minidora-rust/0.7.0-alpha.11`、`utf8-sorted-json-v1`。ローカルSQLite WALと内容ハッシュを使う。 |
| ツールチェーン | Rust/Cargo `1.99.1`、Rust edition `2024`。`rust-toolchain.toml` のSHA-256は `d13306be0ecda81ce779baa290ffb417380d525928477f16200884b5169cca33`。 |
| 依存 | `Cargo.lock` のSHA-256は `b4e97fecdca8fd1fc863349b1a48eb99a925da7fed0ed6ce6cfe79bdd1cececc`。SQLiteは同梱。観測したLinux実行物はOpenSSL 3とglibcに動的リンクする。 |
| Linux CLI | Debian 13、`x86_64-unknown-linux-gnu`。Cloud作業領域の `/workspace/.minidora-cloud/build-v4-0d1a4c1/minidora-rust`。SHA-256は `17584905d5fdc825701f9da661accbeefdd240de5958e7238f454646337eaaa3`。 |
| 生成・描画 | Coreが経路ごとに使った生成モデルの版と成果物参照を `ContentPlan` に記録する。通常経路の描画版は `utf8-bytes-v2`。 |

ビルドは上記Git基線の `rust/` で、[Cloud手順](#build-in-the-cloud-workspace)のツールチェーンを用いて実行した。確認した実行物のビルドコマンドは `CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 cargo build --workspace --locked`。実行物のハッシュはこの環境で生成し、観測に使用したファイルの識別値。

### 標準の呼出方法

CLIは原文引数とUTF-8の標準入力を受け、`run --json` で `ResultRecord` を返す。終了コードは `COMMIT=0`、`SUSPEND=2`、`REJECT=3`、`FAIL=4`、`CANCEL=5`。`--version` は入力の版を指定する。状態を継続するときは `--db` と `--actor` を共有し、結果の履歴には同じ `--case` を使う。

```bash
minidora_bin=/workspace/.minidora-cloud/build-v4-0d1a4c1/minidora-rust
minidora_state=$(mktemp -d)
printf '%s' '17*23' | "$minidora_bin" run --db "$minidora_state" \
  --actor demo --case arithmetic --input first --json
```

Native利用側は同じソース版の `minidora-domain` と `minidora-engine` を使用する。`RequestEnvelope` はactor、case、入力ID・版、原文、権限、予算を保持する。`Engine::execute` の `ResultRecord` には終端状態、理由、依存、成果物の該当項目が入る。戻り値が `Ok` の場合も、呼出側は `lifecycle` で元Goalの成立を判定する。

```rust
use minidora_domain::{RequestEnvelope, Result, ResultRecord};
use minidora_engine::Engine;
use std::path::Path;

fn calculate(state: &Path) -> Result<ResultRecord> {
    let actor = "native-demo";
    let mut engine = Engine::open(state)?;
    let request = RequestEnvelope::new("arithmetic", "first", actor, "17*23");
    engine.execute(actor, request)
}
```

### 直接確認と提供範囲

- 標準入力とNative APIで `17*23` が `391` を返した。Nativeで状態を開き直し、同じ結果IDと保存schemaを取得した。
- 別CLIプロセスで保存された2規則と新規の前提を使い、`どうしてマヒワは生物なのですか` が `DERIVED / COMMIT` となった。記憶版3、依存5件、描画1,342 UTF-8バイトを照合した。この入力で記録された `model_revision` は `d44a65503e38be3f4fd4e4f3eccaa47a60c60fd29012332d2fec18b848ffba83`。別actorによる同一caseの要求は `CASE_ACCESS_DENIED` で拒否され、別actor自身のcaseでは前提不足で停止した。
- 明示許可を付けたファイル出力は47バイトの実ファイルと `CONFIRMED` 受領記録が一致した。同じ入力ID・版の再実行で結果ID、ファイルのinode、更新時刻が一致した。許可を省いた要求は `WRITE_PERMISSION_MISSING` で停止し、要求先のファイルは作成されなかった。
- 現提供範囲は以下に記載する限定文法・データ形式の数理、宣言関係・条件、文書と表、コード生成・有限評価、資料取得と検索、許可付きローカル出力。既存の認証付きループバックHTTPとブラウザー画面も利用できる。外部資料アクセスは対象ホストの許可、ネットワーク、提供元に依存する。汎用翻訳、無制限の自然言語・コード実行、外部送達、音声・動画入力は標準ベータの提供範囲外。今回の直接観測環境はLinux x86_64。

## Build in the cloud workspace

The toolchain is pinned to **Rust 1.99.1**, edition **2024**, with rustfmt and Clippy. The instructions below use the installed cloud toolchain and put dependencies, compiled files and runtime state outside the frozen source areas. They assume Bash on the current Linux cloud host; they are not a Windows/macOS acceptance claim.

```bash
export CARGO_HOME=/workspace/.cargo
export RUSTUP_HOME=/workspace/.rustup
export CARGO_TARGET_DIR=/workspace/.minidora-cloud/rust-target
export PATH="$CARGO_HOME/bin:$PATH"
cd /workspace/MINIDORA-BETA/rust

rustc --version --verbose
cargo --version
cargo metadata --locked --format-version 1
cargo check --workspace --all-targets --locked
cargo build --workspace --locked

minidora_bin="$CARGO_TARGET_DIR/debug/minidora-rust"
minidora_state=/workspace/.minidora-cloud/rust-demo-state
mkdir -p /workspace/.minidora-cloud
"$minidora_bin" --help
```

`Cargo.lock` fixes the dependency resolution. If the toolchain is absent, provision the official Rust toolchain with normal certificate/checksum verification before running these commands. Keep the environment's proxy and certificate trust. No API key or external provider is needed for the local arithmetic, relations, mathematical functions or file-output examples.

These commands establish the Rust toolchain, compile the workspace and locate the runnable entry for **BUILD**. They are not independent quality tests or claims of QC/QA acceptance. Only report the command/OS/revision and actual execution evidence observed. Additional quality or benchmark work requires a separate explicit request.

## Arithmetic, actual-result context and persistence

Use the same actor, state directory and case to continue a conversation. Each command is a fresh process, so the second request uses the durable prior result rather than a hardcoded expected answer.

```bash
"$minidora_bin" --db "$minidora_state" --actor demo --case arithmetic '2+3'
"$minidora_bin" --db "$minidora_state" --actor demo --case arithmetic '詳しく説明して'
"$minidora_bin" --db "$minidora_state" --actor demo --case arithmetic '2+4'
"$minidora_bin" --db "$minidora_state" --actor demo --case unit-correction \
  --input measurement --version 1 '1 km + 500 m'
"$minidora_bin" --db "$minidora_state" --actor demo --case unit-correction \
  --input measurement --version 2 '第二項を600 mに変更して'
"$minidora_bin" --db "$minidora_state" --actor demo --case unit-correction '詳しく説明して'
"$minidora_bin" inspect --db "$minidora_state" --actor demo --case arithmetic
```

Arithmetic uses exact integers/rationals, bounded expressions and recorded steps. Binary addition/subtraction preserves identical explicit units and converts mixed `mm`, `cm`, `m` and `km` exactly to metres. An explicit first/second operand correction creates the next input version, links it to the actual prior result and retains that original result. Corrections cover saved binary literals using `+`, `-`, `*`, `/`, `%`, `^` or `**`; unit-bearing correction remains limited to addition/subtraction, and nested or compound expressions suspend. A repeated explicit `--case`, `--input` and `--version` with the same complete request retrieves its saved result; changing the request under that identity is a conflict. Omit `--input` for a fresh identity on each CLI invocation. A request blocked by an unresolved predecessor retains its suspension reason under its original identity. Submit a new input after resolving the predecessor. This successor behavior is separate from the frozen beta facade's in-instance duplicate rejection.

### Bounded calculation sequences

```bash
"$minidora_bin" --db "$minidora_state" --actor demo --case chained-calculation \
  $'計算手順を実行して:\nbase: 2+3\ndouble: @{base}*2\nfinal: @{double}+1'
"$minidora_bin" --db "$minidora_state" --actor demo --case chained-units \
  $'計算手順を実行して:\nbase: 1 km + 500 m\nfinal: @{base} + 500 m'
```

Each line declares one unique step ID and exact arithmetic expression. `@{earlier-id}` inserts the earlier step's exact rational result, with parentheses preserving scalar expression grouping. Exact unit-bearing results can also flow into the existing bounded addition/subtraction grammar, including mixed `mm`, `cm`, `m` and `km` conversion; dimensional multiplication or division is not added by this route. The Core adds each step to its finite required-artifact graph, stores its source expression and evaluated result, and records the upstream artifact SHA-256 actually consumed by each dependent step. Only earlier declared results can be referenced; forward or unknown references remain unresolved. At most 16 steps are admitted, and the caller's action budget must also cover every required graph node. This is a bounded calculation-chain route, not general multi-step reasoning acceptance.

A quantity reference must occupy a complete number-and-unit operand of the existing binary addition/subtraction grammar. Numeric prefixes, exponent fragments, unit suffixes and other operators suspend with `CALCULATION_REFERENCE_CONTEXT`. The minimum action budget is the number of declared steps plus three, or plus four when a file is required. Scoped CLI observations, artifact consumption, typed stops, version history and a confirmed local file receipt are recorded in [the B06 route audit](audit/b06-calculation-sequence-route-validation.json); B02 and B06 acceptance remain `UNVERIFIED`.

## Structured Python function generation

```bash
"$minidora_bin" --db "$minidora_state" --actor demo --case generated-function \
  'code-generate:
{"仕様":{"名前":"増分","引数":["x"],"手順":[{"種別":"返却","式":{"種別":"算術","演算":"加算","左":{"種別":"参照","名前":"x"},"右":{"種別":"定数参照","キー":"幅"}}}]},"定数":{"幅":1}}'
"$minidora_bin" --db "$minidora_state" --actor demo --case generated-function-verify \
  'code-generate:
{"仕様":{"名前":"増分","引数":["x"],"手順":[{"種別":"返却","式":{"種別":"算術","演算":"加算","左":{"種別":"参照","名前":"x"},"右":{"種別":"定数参照","キー":"幅"}}}]},"定数":{"幅":1}}'
"$minidora_bin" --db "$minidora_state" --actor demo --case generated-function-verify \
  '生成コードを検証して:
{"試験":[{"引数":{"x":3,"定数":{"幅":1}},"期待値":4},{"引数":{"x":0,"定数":{"幅":-2}},"期待値":-2}]}'
"$minidora_bin" --db "$minidora_state" --actor demo --case generated-function-explain \
  'code-generate:
{"仕様":{"名前":"増分","引数":["x"],"手順":[{"種別":"返却","式":{"種別":"算術","演算":"加算","左":{"種別":"参照","名前":"x"},"右":{"種別":"定数参照","キー":"幅"}}}]},"定数":{"幅":1}}'
"$minidora_bin" --db "$minidora_state" --actor demo --case generated-function-explain \
  '詳しく説明して'
```

`code-generate:` (ASCII case insensitive) and `コードを生成して:` accept the original JSON body with exactly `仕様` and `定数`. This ports the existing explicit algorithm-generation contract. The specification has exactly `名前`, `引数` and `手順`; it supplies the algorithm. The function receives the declared arguments followed by the separate `定数` dictionary. Changing constant values preserves the generated source. The output is Python source; an exact `--output` / `--allow-write` grant can save those same bytes as a file.

In the same actor and case, `詳しく説明して` after code generation reads its stored report and reconstructs the explanation from that exact algorithm IR. It describes assignments, branches, loops, expressions and returns, keeps the generated report/prior-result dependencies, and states the stored Python parse/execution/test status. The v52 route still does not execute the generated code for an explanation; see [the v52 code-context route evidence](audit/bb-content-code-context-explanation-v52-route-validation.json).

To check generated output, send `生成コードを検証して:` as the next request in that same actor and case after a committed code-generation result. The JSON body accepts only `試験` and optional `最大手数`; each case supplies `引数` and `期待値`. Core reloads and revalidates the previous generation report, then uses the existing restricted Python AST reader and bounded Rust interpreter on the stored source. It records the source result, generation-report reference, verification-report reference and case-level actual values. For example, the `generated-function-verify` commands above produce two finite checks. This does not invoke host Python, `exec`, `eval`, a compiler or external effects, and matching finite cases do not prove general program correctness. The generator's Python execution and I/O test statuses remain `NOT_RUN`; see [the v52 generated-code verification route evidence](audit/bb-content-generated-code-verification-v52-route-validation.json). General code creation, editing, unrestricted validation and full B07 acceptance remain `UNVERIFIED`.

After `検索 wikipedia: ...` stores its ranked listing, `1件目の本文を取得して` selects only that saved candidate and fetches the actual raw wikitext body. The request must grant the exact `ja.wikipedia.org` hostname again with `--allow-read-host`; a listing title or excerpt never counts as body evidence. A following `前回の本文を要約して` consumes the retained body packet and reports its source/hash and quoted candidate text. This is a narrow LIVE Wikipedia route; other providers or HTML/PDF bodies remain subject to the port's exact format and host limits. See [the v52 search-candidate body evidence](audit/bb-source-search-candidate-body-route-validation.json).

| IR category | Supported forms |
|---|---|
| Expressions | `参照` (`名前`), `定数参照` (`キー`), `算術`/`比較` (`演算`, `左`, `右`), `選択` (`条件`, `真`, `偽`), `リスト` (`要素`), `添字` (`対象`, `位置`), `呼出` (`関数`, `引数`) |
| Arithmetic / comparison | `加算`, `減算`, `乗算`, `整数商`, `剰余` / `等値`, `非等値`, `未満`, `以下`, `超`, `以上` |
| Calls | `合計`, `長さ`, `範囲`, `絶対値`, `最小`, `最大`, `整列`, `リスト化`, `すべて`, `いずれか` |
| Statements | `代入` (`名前`, `式`), `返却` (`式`), `分岐` (`条件`, `真`, `偽`), `反復` (`変数`, `対象`, `手順`) |

Every IR object has exactly its listed fields plus `種別`; empty blocks emit `pass`. The source target is Python 3.12 with canonical Unicode 15.0 identifier properties, at most 15 unique user parameters, and reserved-name/keyword checks. The report retains the exact input, specification/constants byte ranges, source hash and constructed syntax bounds. Constants retain original JSON lexemes and support null, booleans, 256-bit integers, strings, lists and string-keyed dictionaries; floats and duplicate decoded keys stop. Other finite limits are recorded in [the B07 audit](audit/b07-code-generation-route-validation.json).

The generated source and report explicitly mark host-Python parsing, execution and input/output tests `NOT_RUN`. `COMPUTED` describes generation from the supplied IR. Name binding, argument/result types and runtime success require separate checks; an unbound name can still generate source. Independent development QC parsed nine actual generated sources and checked 20 finite cases: 19 outputs matched, and one unbound-name case produced the expected `NameError`. General free-form code creation, explanation, repair, complete Python compatibility and full B07 acceptance remain `UNVERIFIED`.

## Bounded Python function reading and evaluation

```bash
"$minidora_bin" --db "$minidora_state" --actor demo --case code-reading \
  $'code-read:\n{"情報源":"def square(x):\\n    return x * x\\n"}'
"$minidora_bin" --db "$minidora_state" --actor demo --case code-evaluation \
  $'code-evaluate:\n{"情報源":"def square(x):\\n    return x * x\\n","引数":{"x":4}}'
"$minidora_bin" --db "$minidora_state" --actor demo --case code-check \
  $'code-verify:\n{"情報源":"def square(x):\\n    return x * x\\n","試験":[{"引数":{"x":4},"期待値":16}]}'
```

Japanese headers `コードを読んで:`, `コードを評価して:` and `コードを検証して:` are also supported. Each request contains one JSON document with `情報源`; evaluation adds `引数`, and verification adds a `試験` array. Both evaluation and verification accept an optional `最大手数` from 1 to 200,000 (default 50,000). The reader reports the function signature, bounded syntax counts and selected statement/call/comparison source spans. Evaluation and checking use a native Rust interpreter for one top-level Python-function subset: scalar/list/dictionary expressions, subscripting, arithmetic and comparisons, short-circuit logic, conditional expressions, assignments, `if`/`elif`/`else`, `for`/`else`, loop control, `return`, and an allowlist of `abs`, `len`, `min`, `max`, `sum`, `range`, `sorted`, `list`, `all` and `any`.

The parser validates the whole source before evaluation. Imports, attributes, nested functions, indirect calls, annotations, unsupported syntax, and out-of-envelope values stop with a diagnostic. Source is limited to 32,768 UTF-8 bytes; integers to 256 bits; verification to 1–32 cases. Reports retain the original source, digest, AST/source-span observations, actual evaluation status and finite case outcomes as an authorized artifact in the normal Core/content/render/history route. `STOPPED` records a step or request-deadline stop; `FAILED` records a bounded evaluation failure; unsupported source/settings suspend before evaluation. A finite verification result describes only the supplied cases and distinguishes `true` from integer `1`.

This evaluator does not invoke host Python, `exec`, `eval`, a compiler, subprocesses or external effects. Generated source is not executed by this route. The evaluator checks its step limit and request deadline, but does not poll an in-progress cancellation request. Eleven development-only comparisons matched the frozen evaluator's typed results/failure kinds and eleven CPython 3.11 AST structure/span comparisons; those finite observations do not establish general Python compatibility, static name/type analysis, all-input correctness or general code quality. Full B07 acceptance remains `UNVERIFIED`; see the [B07 continuation audit](audit/b07-bounded-code-capability-route-validation.json).

## Statistics, linear systems and probability

```bash
"$minidora_bin" --db "$minidora_state" --actor demo --case statistics \
  '統計 [0.1,0.2,0.3,0.4] 単位 m'
"$minidora_bin" --db "$minidora_state" --actor demo --case mixed-statistics \
  '統計 [1 km, 500 m]'
"$minidora_bin" --db "$minidora_state" --actor demo --case linear \
  '連立方程式 [[2,1],[1,-1]] = [7,-1]'
"$minidora_bin" --db "$minidora_state" --actor demo --case polynomial \
  '簡約 (x+1)*(x-1)'
"$minidora_bin" --db "$minidora_state" --actor demo --case polynomial \
  '微分 3*x**3+x*y-0.5*x+7 変数 x'
"$minidora_bin" --db "$minidora_state" --actor demo --case polynomial \
  '積分 x**2+y/2 変数 x'
```

Statistics retain supplied values and each input unit, then compute exact sum, mean, median, extrema and population/sample variance. Mixed `mm`, `cm`, `m` and `km` inputs are converted exactly to metres and reported through `normalized_values`; other mixed unit families suspend without a numeric result. Identical unit labels retain that label. One observation has no sample variance. The values do not establish a representative real-world population.

Linear systems use coefficient-column order rather than inferred variable names. Results distinguish a unique solution, a parameterized solution space and inconsistent equations, with original equations, row transformations and verified substitution/contradiction certificates. Rectangular systems, dimensions, scalar sizes and operations have explicit limits.

## Finite probability distributions

```bash
"$minidora_bin" --db "$minidora_state" --actor demo --case probabilities \
  '確率分布 [1, 2, 3] を正規化して'
"$minidora_bin" --db "$minidora_state" --actor demo --case probabilities-en \
  'normalize-probabilities [1, 2, 3]'
```

The operation accepts 1–4096 explicitly supplied nonnegative exact weights and divides each by their exact total. The example yields `1/6`, `1/3` and `1/2`, with an exact probability sum of `1`. Negative weights, an all-zero total, extra constraints and out-of-limit input stop with typed errors. The result describes only the supplied weights; it is not an empirical or real-world likelihood estimate.

### Expected value and variance

```bash
"$minidora_bin" --db "$minidora_state" --actor demo --case expected-value \
  '期待値 [0, 2, 4] 重み [1, 2, 1] を計算して'
"$minidora_bin" --db "$minidora_state" --actor demo --case expected-value-en \
  'expected-value [0, 2, 4] weights [1, 2, 1]'
```

Each outcome is paired with one weight. The weights are normalized exactly, then Core computes `Σ(value × probability)` and `Σ((value − expected value)² × probability)` as exact rationals; the example gives expected value `2` and variance `2`. The report retains each aligned probability and each contribution to both sums so the intermediate values can be inspected. Outcomes may be negative, while weights must be nonnegative with a positive total. Mismatched arrays stop. The report covers only the explicitly supplied finite distribution; it does not infer a population or real-world likelihood.

### Probability of an explicit event

```bash
"$minidora_bin" --db "$minidora_state" --actor demo --case event-probability \
  '事象確率 [1, 2, 3] 重み [1, 2, 1] 事象値 [1, 3] を計算して'
"$minidora_bin" --db "$minidora_state" --actor demo --case event-probability-en \
  'event-probability [1, 2, 3] weights [1, 2, 1] values [1, 3]'
```

The event values use exact numeric equality. The example returns probability `1/2`. The result includes an aligned membership flag and probability contribution for each outcome. Duplicate event values do not double-count; duplicate outcomes each contribute their own mass. An empty event-value array returns zero. The operation requires equal-length outcome and weight arrays, a positive total of nonnegative weights, and remains limited to the supplied finite distribution.

## Symbolic polynomials

Symbolic operations cover bounded rational-coefficient multivariate polynomials. Multiplication must be explicit. `**` and the documented Rust alias `^` take nonnegative literal integer exponents. Variable denominators, functions, implicit multiplication and trailing unparsed constraints fail. An integral is one representative antiderivative; an arbitrary term independent of its target variable remains possible. This is not a general computer algebra system or free-form mathematical-language claim.

### Exact single-variable linear equations

```bash
"$minidora_bin" --db "$minidora_state" --actor demo --case linear-equation \
  '一次方程式 2*x+1=0 を解いて'
"$minidora_bin" --db "$minidora_state" --actor demo --case linear-equation-en \
  'solve-linear 2*x+1=0'
```

This route accepts one explicit equality whose normalized difference contains exactly one variable and has degree at most one. The report returns the exact normalized coefficients and classifies a unique rational solution, an identity (`ALL_REAL_VALUES`) or a contradiction (`NO_SOLUTION`). Multiplication must be explicit; linear systems, multivariable equations, degree-two-or-higher equations and unrestricted natural-language notation are handled by separate contracts or rejected. Expression size, coefficient and operation limits remain active.

### Exact quadratic equations

```bash
"$minidora_bin" --db "$minidora_state" --actor demo --case quadratic \
  '二次方程式 2*x^2-3*x+1=0 を解いて'
"$minidora_bin" --db "$minidora_state" --actor demo --case quadratic-en \
  'solve-quadratic 2*x^2-3*x+1=0'
```

This bounded route accepts one explicit equality whose normalized difference has exactly one variable and degree two. It uses exact rational coefficients and returns the normalized coefficients, discriminant, root sum/product and exact roots: rational roots are substituted back exactly, irrational roots are represented by radicals, and a negative discriminant yields exact complex-conjugate forms. Multiplication must be explicit. It does not approximate decimals or solve linear, higher-degree, multivariable or unrestricted natural-language equations; parser size, degree, coefficient and operation limits still apply.

### Exact polynomial evaluation

```bash
"$minidora_bin" --db "$minidora_state" --actor demo --case polynomial-evaluation \
  '評価 x**2 + 2*x + 1 代入 x=3'
"$minidora_bin" --db "$minidora_state" --actor demo --case polynomial-evaluation-en \
  'evaluate x**2 + 2*x + 1 at x=3'
```

Evaluation uses the same bounded polynomial grammar as simplification and symbolic calculus. Every source variable needs one exact scalar binding; comma-separated bindings such as `x=3, y=1` are supported, and at most eight bindings are accepted. Missing variables, malformed values, unsupported expressions and duplicate bindings stop with typed errors. The JSON report preserves the expression and bindings, then lists each monomial value and contribution before the exact result. Extra valid bindings are retained but do not change the value.

## Declared premises, rules and negation

```bash
"$minidora_bin" --db "$minidora_state" --actor demo --case relations 'ルリは鳥です'
"$minidora_bin" --db "$minidora_state" --actor demo --case relations 'もしXは鳥ならXは動物'
"$minidora_bin" --db "$minidora_state" --actor demo --case relations 'ルリは動物ですか'
"$minidora_bin" --db "$minidora_state" --actor demo --case relations '詳しく説明して'
"$minidora_bin" --db "$minidora_state" --actor demo --case negative 'ミナは鳥ではない'
"$minidora_bin" --db "$minidora_state" --actor demo --case negative 'ミナは鳥ですか'
"$minidora_bin" --db "$minidora_state" --actor demo --case relations \
  'ココについて、何が分かっていますか'
"$minidora_bin" --db "$minidora_state" --actor demo --case why-questions \
  'どうしてココは動物なのですか'
"$minidora_bin" --db "$minidora_state" --actor demo --case why-questions-en \
  'If X is a bird then X is an animal'
"$minidora_bin" --db "$minidora_state" --actor demo --case why-questions-en \
  'Coco is a bird'
"$minidora_bin" --db "$minidora_state" --actor demo --case why-questions-en \
  'Why is Coco an animal?'
```

These are scoped user premises and formal consequences, not independently verified facts about the world. Persistent premises/rules are actor-scoped across cases; cases identify result lineages, not separate knowledge databases. Use distinct explicit scopes or actors for isolated premise worlds. The negative example uses a different subject to avoid contradicting the earlier positive example. The supported copular grammar preserves polarity and optional `[scope=story]` labels. Rule variables use `X`, `Y`, `Z` or explicit `$` names. Proof-preserving rule composition, dependencies and invalidation belong to Core. Unknown grammar and missing premises produce typed stops in proof requests; the natural relation inventory retains opposing recorded support without resolving it. General Japanese understanding, unrestricted quantified logic and induction from arbitrary examples are not established.

The natural `Xについて、何が分かっていますか` inventory lists active direct relations and conclusions supported by retained rules. It keeps their guarantee labels and proof dependencies, marks contradictory support without resolving it, and discloses that the saved relation list is non-exhaustive and not an outside-world fact check. The fresh-process derived-inventory route records 14 observations (11 `COMMIT`, 3 `SUSPEND`), including proof recovery and typed stops; see [the derived-relation inventory route evidence](audit/bb-lang-derived-relation-inventory-20261113-route-validation.json).

A bounded profile request such as `ココについて保存されている情報をもとに短い紹介文を書いて` (or `Write a short profile of Coco from saved facts`) selects current supported relation proofs for one explicit subject in the same actor's memory. Core writes a provenance line for each supported relation, preserving scope and premise/derivation IDs; conflicting records stay side by side. Two Core-approved Japanese surfaces vary only the transition wording, and the existing native `SequenceModel` ranks them. The result remains `HYPOTHESIS` and says that outside-world truth and record completeness were not checked. Missing support suspends with `MISSING_PREMISES`; another actor's memory is never used as fallback. This saved-record profile route is not general free-form writing or complete B08 acceptance. [Its fresh-process evidence](audit/bb-content-known-relation-profile-v69-route-validation.json) covers a local file receipt, interrupted-write reconciliation without resending, and a following previous-result summary.

The same profile can be requested through the authenticated loopback `POST /v1/execute` consumer. It uses the configured actor and the same Engine/Core result; a following CLI request in that case can summarize the exact HTTP result. The scoped local observation is recorded in [the HTTP consumer route evidence](audit/bb-content-known-relation-profile-v69-http-consumer-route-validation.json).

`minidora-rust serve` also serves a same-origin browser workspace at `/` and fixed `/app.js`, `/styles.css` and `/favicon.svg` assets. Its form sends the full request envelope to the authenticated API, then shows lifecycle, guarantee, answer, reason, result identity and the exact dependency IDs returned by Core. The bearer token stays in tab memory and is sent only to the loopback API; the page makes no external asset requests and receives a restrictive same-origin CSP. `POST /v1/execute/stream` requires `Accept: text/event-stream` and uses the `minidora-stream/v1` contract: `provisional` announces processing with no answer content and no authority, `invalidated` discards that status for any non-`COMMIT` Core result, and `final` carries the actual `ResultRecord`. The browser enforces this order before rendering. Disconnecting a stream does not cancel synchronous Core execution. A separate case-cancel action sends Core's native `キャンセル` intent in the same actor/case and retains prior result history; it does not reverse already committed work. [The browser route evidence](audit/bb-content-known-relation-profile-v69-browser-ui-route-validation.json) records Chromium handling of committed, suspended and cancelled streams, rendered proof dependencies, retained prior result after cancellation, and the wrong-token stop. Undo, public deployment, broad browser compatibility and complete B08/general writing acceptance remain unverified.

The v52 [declared-rule follow-up replay](audit/bb-learn-declared-rule-follow-up-v52-route-validation.json) uses one explicit rule and premise across fresh CLI processes, verifies the derived answer's premise/rule dependencies, suspends when a counterexample conflicts, then preserves the original records after explicit rule invalidation so the counterexample is the only remaining answer dependency. This is bounded user-directed proof reuse, not empirical or autonomous rule learning; full BB-LEARN remains `UNVERIFIED`.

The v58 [cross-case rule-transfer route](audit/bb-learn-cross-case-rule-transfer-v58-route-validation.json) exercises a new subject in a separate case through the normal CLI. Core derives the answer and explains its proof from the new subject's premise plus the retained explicitly scoped rule; the original subject's premise is absent from that proof. A scope mismatch and a separate actor both stop with missing premises. A counterexample suspends with a typed conflict, rule invalidation leaves only the direct counterexample as active support, a later subject cannot use the invalidated rule, and the original case result replays unchanged. This is explicit proof reuse, not empirical learning; full BB-LEARN remains `UNVERIFIED`.

The premise-listing question returns only active direct user premises for the named subject, including their scope and premise IDs. Derived conclusions are omitted from this list; it does not verify claims about the outside world. A subject with no active supplied premise produces a typed `MISSING_PREMISES` stop.

Natural forms such as `ココとは何ですか` and `What is Coco?` use that same actor-scoped lookup for the exact named subject. They report retained user-provided premises and their provenance; they do not supply an outside-world definition. Numeric expressions and unresolved pronouns are not reinterpreted as memory subjects.

Explicit corrections such as `ココは哺乳類ではなく鳥です` or `Coco is a bird, not a mammal` update only a matching active direct premise in the same actor and scope. Core retains the old record as inactive, stores both the negative old claim and the replacement as one memory revision, and preserves earlier result history. A singular subject pronoun such as `それ` or `it` can refer to a premise only when the selected committed result in the same case points to one exact active direct premise with the superseded relation. An explicit scope can disambiguate a result that lists multiple scopes; missing context, ambiguity, or a derived-only result suspends without changing memory. Both claims remain user-provided premises, not independently verified facts. See [the named-subject correction route](audit/bb-context-natural-premise-correction-route-validation.json) and [the prior-result correction route](audit/bb-context-prior-result-correction-route-validation.json); broader co-reference and learning acceptance remain unverified.

The bounded `それ`/`it` relation-question route resolves a singular pronoun only from active direct-premise IDs in the selected prior committed result for the same case. Core inherits that premise's subject and scope, or uses an explicit matching scope, before running the ordinary Ask/AskWhy proof query. Missing context, an unrelated latest result, a different case or actor, a wrong scope, and multiple candidate premises stop without falling back to actor memory. See [the v58 pronoun relation-query evidence](audit/bb-context-singular-pronoun-relation-query-v58-route-validation.json). Plural, multi-clause and broader cross-topic co-reference remain unverified.

A bounded two-clause question such as ココは鳥かつ動物ですか or Is Coco a bird and an animal? preserves both copular relations under one named subject and scope. Core queries every clause against the same actor memory, preserves explicit negation, returns each supported or refuted relation, and retains the union of the premise and rule dependencies. Missing or conflicting clauses suspend; alternatives, three-clause questions, differing subjects and pronoun subjects do not enter this route. See [the v59 conjunctive relation-query evidence](audit/bb-lang-conjunctive-relation-question-v59-route-validation.json). Open-domain Q&A and general writing remain unverified.

Ordinary Japanese and English conditional rules accept one or two copular premises. A shorthand second premise may inherit the first premise's subject and scope, as in `もしXは鳥かつ青ならXは動物`, interpreted by this bounded grammar as `Xは鳥` and `Xは青`. It remains a user-supplied rule, not an outside-world fact. When two scoped two-premise rules compose, Core can store an internal shortcut with three antecedents; it retains both parent rule IDs, every direct premise dependency, and the same-scope join. AskWhy shows the complete composed rule, query expansion charges each successful join prefix to its finite relation budget, and invalidating an antecedent or parent rule prevents later use. This does not enable user-declared three-premise rules or unrestricted rule composition. See [the v62 three-antecedent composition evidence](audit/bb-lang-conjunctive-relation-ternary-composition-v62-route-validation.json); full requirement acceptance remains unverified.

The ordinary Japanese relation grammar also accepts bounded transitive `食べる` and `使う` assertions and object questions such as `ミナは何を使いますか`. Core checks each candidate object against the same actor's active premises and rules in the requested scope, then returns only supported relations with their dependency IDs. The answer labels each result as directly supplied or rule-derived. For example, a supplied `ミナはカッターを使う` plus `もしXはカッターを使うならXは道具を使う` supports both the direct and derived relation in the answer. The output does not claim an exhaustive object list or outside-world truth. Past-tense forms, other verbs, unresolved pronouns, missing evidence and conflicting evidence remain unsupported or stop with a typed reason. See [the Japanese transitive object-question route evidence](audit/bb-lang-transitive-object-question-v65-route-validation.json); general Japanese Q&A remains unverified.

The same transitive propositions now accept Japanese yes/no questions such as `ユウはノートを使いますか`, `ユウは毒を使いませんか` and `ユウはノートを使う?`. They enter the ordinary typed relation query with their object, polarity and scope intact. Core reports direct or rule-derived support, returns an evidenced opposite when the asked claim is refuted, and suspends for missing or conflicting premises. Past tense and unsupported verbs remain unresolved. See [the transitive yes/no route evidence](audit/bb-lang-transitive-yes-no-question-v66-route-validation.json); this does not establish general Japanese question understanding.

Natural commonality questions such as `ココとミケの共通点は何ですか` and `What do Coco and Mike have in common?` now compare two distinct named subjects through the same actor-scoped relation proof engine. Core checks the same predicate, object, polarity and exact scope for both subjects, then lists only relations supported for each, with separate direct/derived labels and proof dependencies. The list is non-exhaustive and does not establish outside-world facts. Missing support suspends without concluding that no commonality exists; conflicting evidence also suspends. Candidate scanning and each relation query use the request's finite action budget. See [the common-relation comparison route evidence](audit/bb-lang-common-relation-question-v67-route-validation.json); general semantic comparison remains unverified.

To compare both shared and one-sided saved records, use `ココとミケの保存済みの関係を比較して` or `Compare saved relations for Coco and Mike`. Core rebuilds a bounded proof-backed inventory for each explicit subject in the requested scope, reports matching signed relations separately from relations whose support was found on one side only, and includes each fact/rule lineage in a typed comparison artifact. Conflicting positive and negative records remain marked and unresolved instead of being classified as an unambiguous common or one-sided relation. A one-sided entry does not say the other relation is false or absent. The result remains `HYPOTHESIS` because memory completeness and outside-world truth are unverified. The ordinary same-case previous-result summary can consume this comparison and retains its source dependencies. See [the saved-relation comparison route evidence](audit/bb-lang-saved-relation-comparison-v71-route-validation.json).

After a common-relation answer, `詳しく説明して` now consumes a canonical, case-bound typed report artifact tied to the exact prior result. It rechecks each stored relation separately for both subjects against current actor memory, confirms each side's guarantee and proof dependencies, then returns a distinct explanation with the left and right proof lineage. Missing or changed evidence suspends instead of replaying the old report. The list remains non-exhaustive and does not establish outside-world facts. See [the common-relation explanation route evidence](audit/bb-context-common-relation-explanation-v68-route-validation.json); this bounded follow-up does not establish contextual explanation acceptance.

After a committed copular relation answer, `詳しく説明して` rechecks the same conclusion against current memory and reports its direct premise, derived fact and applicable rule IDs. If the proof has changed, Core suspends with `CONTEXT_PROOF_CHANGED` instead of explaining stale support.

The bounded `なぜ` / `どうして` and `Why ...?` forms ask for an explanation of one explicit relation. Core queries the actor's current premises and rules and lists the supporting premise, derived fact and rule IDs. For this bounded route, Core provides two equivalent surfaces that preserve every proof and disclaimer unit; the native non-neural `SequenceModel` ranks those surfaces and Output renders the selected one. A supplied negative premise is explained as support for the opposite relation; missing or conflicting evidence suspends with a typed reason. The Japanese and English rules in these examples are separate declared premises. This is proof explanation over caller-supplied records, not open-domain writing, general causal understanding or a claim about outside-world truth. The earlier scoped route is recorded in [the why-question route evidence](audit/bb-lang-why-question-route-validation.json); the model-selection and recovered-file route is recorded in [the proof-backed native surface-selection evidence](audit/bb-lang-proof-backed-native-surface-selection-route-validation.json). Full BB-LANG/A01/A02/A07/A13 acceptance remains `UNVERIFIED`.

Elliptical follow-ups such as `なぜですか`, `どうして？` and `Why?` now reuse the same `ExplainPrevious` consumer. For a relation answer it rechecks the current proof and its dependencies; for a calculation it explains the retained operation. It uses only the immediately preceding committed result for the same actor and case. Missing prior output suspends, and another actor cannot read the case. This resolves a bounded previous-result question, not an open-ended causal explanation. See [the contextual why route evidence](audit/bb-lang-contextual-why-v66-route-validation.json).

A stored fact/rule ID can be withdrawn using `invalidate ID` or `IDを無効化して`; inspect the returned JSON or saved case to obtain its actual ID. Original observations remain retained. This does not establish all required generalized-learning or long-history resource gates.

## CSV selection and explicit text editing

The first line names a supported operation. Chat and the Engine accept LF or CRLF CSV record separators and retain the exact body bytes. The direct Rust functions in `minidora_engine::document` use strict CRLF framing. Bash ANSI quoting below shows the CRLF form.

```bash
"$minidora_bin" --db "$minidora_state" --actor demo --case csv \
  $'CSVの列「price」を選択して:\nname,price\r\n猫,001\r\n犬,9007199254740993\r\n'
"$minidora_bin" --db "$minidora_state" --actor demo --case csv-position \
  $'見出しなしCSVの第2列を選択して:\n猫,001\r\n犬,9007199254740993\r\n'
"$minidora_bin" --db "$minidora_state" --actor demo --case csv-position \
  '詳しく説明して'
"$minidora_bin" --db "$minidora_state" --actor demo --case csv-stats \
  $'CSVの列「price」の統計量を計算して:\nname,price\r\ncat,0.1\r\ndog,0.2\r\nfox,0.3\r\n'
"$minidora_bin" --db "$minidora_state" --actor demo --case csv-filter \
  $'CSVの列「status」が値「open」の行を選択して:\nid,status\r\n1,open\r\n2,closed\r\n3,open\r\n'
"$minidora_bin" --db "$minidora_state" --actor demo --case csv-sort \
  $'CSVの列「name」を昇順に並べ替えて:\nid,name\r\n1,cat\r\n2,bird\r\n3,cat\r\n'
"$minidora_bin" --db "$minidora_state" --actor demo --case csv-count \
  $'CSVを列「status」で件数集計して:\nid,status\r\n1,open\r\n2,closed\r\n3,open\r\n'
"$minidora_bin" --db "$minidora_state" --actor demo --case csv-group-stats \
  $'CSVの「region」ごとに「sales」の統計量を計算して:\nregion,sales\r\nnorth,0.1\r\nnorth,0.3\r\nsouth,1.0\r\n'
"$minidora_bin" --db "$minidora_state" --actor demo --case json-table-filter \
  $'JSON表の列「status」が値「open」の行を選択して:\n{"columns":["id","status"],"rows":[["1","open"],["2","closed"],["3","open"]]}'
"$minidora_bin" --db "$minidora_state" --actor demo --case json-table-sort \
  $'JSON表の列「name」を昇順に並べ替えて:\n{"columns":["id","name"],"rows":[["1","cat"],["2","bird"],["3","cat"]]}'
"$minidora_bin" --db "$minidora_state" --actor demo --case json-table-count \
  $'JSON表を列「status」で件数集計して:\n{"columns":["id","status"],"rows":[["1","open"],["2","closed"],["3","open"]]}'
"$minidora_bin" --db "$minidora_state" --actor demo --case json-table-group-stats \
  $'JSON表の「region」ごとに「sales」の統計量を計算して:\n{"columns":["region","sales"],"rows":[["north","0.1"],["north","0.3"],["south","1.0"]]}'
"$minidora_bin" --db "$minidora_state" --actor demo --case json-table-select \
  $'JSON表の列「status」を選択して:\n{"columns":["id","status"],"rows":[["1","open"],["2","closed"],["3","open"]]}'
"$minidora_bin" --db "$minidora_state" --actor demo --case json-table-project \
  $'JSON表から列["name","id"]を抽出して:\n{"columns":["id","name","status"],"rows":[["1","cat","open"],["2","bird","closed"]]}'
"$minidora_bin" --db "$minidora_state" --actor demo --case csv-compare \
  $'CSVを列「id」で2つの表を比較して:\n--- MINIDORA LEFT CSV ---\nid,status\r\n1,open\r\n2,closed\r\n--- MINIDORA RIGHT CSV ---\nid,status\r\n1,closed\r\n3,open\r\n'
"$minidora_bin" --db "$minidora_state" --actor demo --case csv-inner-join \
  $'csv-inner-join "id" "record_id":\n--- MINIDORA LEFT CSV ---\nid,name\r\n2,dog\r\n1,cat\r\n4,fox\r\n--- MINIDORA RIGHT CSV ---\nrecord_id,kind\r\n1,feline\r\n3,bird\r\n2,canine\r\n'
"$minidora_bin" --db "$minidora_state" --actor demo --case csv-to-json \
  $'csv-to-json header:\nname,price\r\ncat,001\r\ndog,9007199254740993\r\n'
"$minidora_bin" --db "$minidora_state" --actor demo --case json-to-csv \
  $'json-to-csv:\n{"columns":["name","price"],"rows":[["cat","001"],["dog","9007199254740993"]]}'
"$minidora_bin" --db "$minidora_state" --actor demo --case length-stats \
  $'CSVの列「length」の統計量を計算して:\nplace,length\r\nA,1 km\r\nB,500 m\r\n'
"$minidora_bin" --db "$minidora_state" --actor demo --case edit \
  $'置換「猫」→「犬」 件数 2:\n猫の猫'
```

English equivalents are `csv-select price`, `csv-statistics price`, `csv-filter "status" equals "open"`, `csv-sort "name" ascending`, `csv-count-by "status"`, `csv-group-statistics "region" "sales"`, `csv-compare-by "id"`, `csv-inner-join "id" "record_id"`, `json-table-inner-join "id" "record_id"`, `json-table-sort "name" ascending`, `json-table-count-by "status"`, `json-table-group-statistics "region" "sales"`, `json-table-select "status"` or `json-table-project ["name","id"]`, followed by a newline and CSV or the exact JSON table body (CSV comparison uses the two reserved marker lines shown below). `json-table-filter "status" equals "open"` filters JSON string tables. Descending sort uses `descending`. A header containing spaces can be selected with a complete quoted JSON string, for example `csv-select "unit price"`, `csv-statistics "unit price"`, `csv-filter "order status" equals "open"`, `csv-sort "display name" descending`, `csv-count-by "order status"`, `csv-group-statistics "sales region" "unit price"`, `csv-compare-by "record id"`, `csv-inner-join "record id" "source id"`, `json-table-inner-join "record id" "source id"`, `json-table-sort "display name" descending`, `json-table-count-by "order status"`, `json-table-group-statistics "sales region" "unit price"`, `json-table-select "order status"` or `json-table-project ["display name","record id"]`.

Selection uses the exact decoded named header and returns a JSON array of **strings**; `001` and huge numeric-looking fields are not converted to numbers. Numeric CSV statistics distinguish unitless exact expressions from explicitly unit-bearing decimal values. Complete scientific tokens and expressions such as `1e3`, `1e3/2` and `1e3-2e3` retain their numeric meaning; the quantity parser cannot shorten an exponent to invent a unit. Existing exponent and exact-value bounds still apply. Use whitespace for an intentionally overlapping unit label, for example `1 e3`; ordinary suffixes such as `1e3m` remain quantities. `json-table-select` returns every value in the exact named string column with source row indices and spans. `json-table-project` emits a string table containing only the requested unique columns, in the requested order, and retains the header and cell source spans. `csv-filter` keeps rows only when the named cell exactly equals the supplied string and returns the same string-table schema; it does not trim, normalize case or infer types. `json-table-filter` applies the same exact string equality to a strict `{"columns":[...],"rows":[...]}` JSON table, returns selected rows and source spans for every decision and emitted cell. `csv-sort` and `json-table-sort` return rows in stable exact-string order, with ties retaining their input order; they do not use locale collation, case folding, Unicode normalization or numeric interpretation. `csv-count-by` and `json-table-count-by` group exact decoded strings in first-seen order, return integer counts, and retain source locations for group members. `csv-compare-by` matches rows by unique exact-string key, requires identical decoded headers in the same order, and reports changed cells plus added and removed rows with source spans from both inputs. Comparison changes retain left row/column order followed by right-side additions. Place `--- MINIDORA LEFT CSV ---` above the first table and `--- MINIDORA RIGHT CSV ---` above the second. The marker lines accept LF or CRLF in chat and the Engine. Marker text is reserved across the whole body. These operations record original byte spans; filter evidence includes every predicate cell, including excluded rows. Quote escapes, empty cells and embedded quoted line breaks are preserved. A bare CR outside quoted fields, inconsistent widths and duplicate decoded header names are rejected. CSV-to-JSON conversion requires an explicit `header` or `no-header` mode (Japanese headers are `CSVをJSONに変換して（先頭行をヘッダーとして）:` and `CSVをJSONに変換して（ヘッダーなし）:`). It returns the string schema `{"columns":[...],"rows":[...]}` or `"columns":null` when headerless. JSON-to-CSV accepts exactly this string-table schema, preserves source spans in its artifact, and rejects duplicate/unknown fields, invalid widths, nonstring cells and formula-leading cells rather than silently sanitizing or executing them. Japanese JSON-to-CSV input uses `JSONをCSVに変換して:`.

For positional selection, use `csv-select-index no-header 2:` or `csv-select-index header 2:` followed by the CSV body. The Japanese forms are `見出しなしCSVの第2列を選択して:` and `見出しありCSVの第2列を選択して:`. Column numbering starts at 1, and the selected cells remain JSON strings. The `header` mode uses the first record as the header. The `no-header` mode includes it among the data rows. In the same case, `詳しく説明して` shows the selected column and the original byte ranges of the selected cells.

`csv-inner-join "id" "record_id":` or `CSVを「id」と「record_id」で内部結合して:` uses the reserved two-table CSV markers above. The two named keys must be unique in their own headered tables; exact matches produce JSON string-table rows in left-table order with `left.` and `right.` namespaced columns. Both keys remain in the output, unmatched keys are listed separately, and emitted header/cell values map to original byte ranges in the combined input. No type inference or one-to-many expansion is applied.

Named-column statistics explicitly interpret every selected cell as either a unitless exact arithmetic expression or as one decimal numeric literal followed by a unit. Mixed unit-bearing and unitless/non-numeric cells suspend. The exact statistics report identifies the source digest, decoded column, observation count and original header/cell byte spans; mixed `mm`, `cm`, `m` and `km` values normalize exactly to metres. These are descriptive statistics for supplied cells and do not imply a representative population.

After a committed `CSV_STATISTICS` result, `詳しく説明して` reloads its same-case `DocumentResult`, recomputes the exact report from the retained CSV, and checks source/output hashes and source-cell spans before explaining the recorded statistics and up to eight original cells. The follow-up carries both the prior result and report artifact references. An exact granted `--output` path can save the explanation; `reconcile` replays the same source-bound explanation and confirms the existing file without resending the write. CSV table comparison now has a separate bounded explanation route below. Grouped statistics and the remaining CSV/JSON report types retain their results but do not use these explanation consumers yet. See [the CSV statistics context-route evidence](audit/bb-math-data-csv-statistics-context-followup-route-validation.json).

After a committed `CSV_TABLE_COMPARE` result, `詳しく説明して` reloads the actor/case/input/version sealed bundle, recomputes `csv_compare_by`, and checks the canonical report artifact, prior result text, ContentPlan, RenderObservation and their stored bytes. The bounded explanation reports exact decoded-string equality and counts; up to eight changed cells or added/removed rows include the relevant left/right source rows, original byte ranges and per-range SHA-256. The prior result, sealed bundle and report are linked in the execution graph. An authorized local-file write can be reconciled against the same input/report/render without resending it. Numeric, semantic and factual equivalence are not inferred. See [the CSV table comparison explanation route](audit/bb-context-csv-table-comparison-explanation-v75-route-validation.json); B04, A07, A14 and E08 remain `UNVERIFIED` beyond this bounded route.

Grouped CSV statistics use an exact decoded string key in first-seen order and compute exact descriptive statistics for a distinct value column. Each group must contain unitless exact expressions or unit-bearing decimal literals consistently; mixed length units convert exactly through the existing `mm`, `cm`, `m` and `km` rules. The report retains source record numbers plus group-key and measured-cell byte spans. It describes the supplied rows and does not infer a wider population.

Literal editing replaces original nonoverlapping matches only. A supplied count must match before any result is formed; omitting the count means all such matches. Unmatched text stays byte-for-byte unchanged; inserted text is not rescanned. Missing search text, a no-op, an empty search, a count mismatch or resource overflow stops. The original document/hash, exact parameters and source/output mappings are stored separately from the emitted result. This is not arbitrary editing-intent inference, unrestricted writing quality or a file write without the separate exact-target grant.

### Explicit-source writing composition

```bash
"$minidora_bin" --db "$minidora_state" --actor demo --case composed-notice \
  $'text-compose:\n{"materials":[{"id":"notice","text":"Cat fair Nov 14"}],"units":[{"id":"title","kind":"heading","fragments":[{"material":"notice","start":0,"end":8}],"with":[],"required":true,"protected":false},{"id":"date","kind":"bullet","fragments":[{"material":"notice","start":9,"end":15}],"with":[],"required":false,"protected":false}],"order":["title","date"],"max_chars":256}'
```

`text-compose:` (ASCII case insensitive) and `文章を構成して:` accept one strict JSON body with exactly `materials`, `units`, `order` and `max_chars`. Each material has an `id` and literal `text`. A unit has an `id`, `kind`, `fragments`, `with`, `required` and `protected`; each fragment names a material and a nonempty `start`/`end` half-open UTF-8 byte range in its decoded text. Kinds are `heading`, `paragraph`, `bullet`, `numbered`, `quote`, `condition` and `caveat`. The listed order is explicit; companion units are inserted immediately after their parent, required units are retained, and condition/caveat units are always included. Unselected units are reported. Maximums are 128 materials/units, 32 fragments per unit, 100,000 Unicode scalar values and 256,000 input/output bytes. Invalid UTF-8 boundaries, duplicate keys/IDs, missing sources, ambiguous fields, span overflow or length overflow suspend without truncating text.

Markers are fixed by role: headings use `# `, bullets `- `, numbered blocks `1.`, quotes `> `, and condition/caveat blocks use `条件:`/`留保:` labels. Paragraphs have no prefix.

The output is limited to caller-selected literal fragments, fixed layout prefixes and blank-line separators. Every output segment maps either to its exact decoded material range and containing JSON string token or to the unit directive that generated the layout token. The stored artifact includes source/output hashes, selected and omitted units, protection/required flags and complete output spans. `USER_PREMISE` describes the supplied content; it does not assert that facts are true. This route does not write original prose, infer a purpose, select facts, prove semantic equivalence, validate writing quality or satisfy the twelve-task B08 acceptance population. A separate bounded extractive candidate operation is documented below; semantic summary quality remains open. See [the composition route audit](audit/b08-explicit-text-composition-route-validation.json).

### Extractive summary candidates

```bash
"$minidora_bin" --db "$minidora_state" --actor demo --case extractive-candidates \
  $'text-summary:\n{"text":"A source sentence. A second source sentence.","max_sentences":2,"max_chars":48,"required_sentence_indices":[1]}'
```

`text-summary:` (ASCII case insensitive) and `文章を要約して:` accept one strict JSON body with exactly `text` and `max_sentences`, plus optional `max_chars`, `format` and `required_sentence_indices`. `max_sentences` is an integer from 1 through 8; `max_chars`, when supplied, is 1 through 100,000 Unicode scalars and otherwise defaults to 100,000. `format` is `bullets` (the default) or `paragraph`; any other value suspends with `TEXT_SUMMARY_SCHEMA`. Bullet layout prefixes each sentence with `- `, while paragraph layout places one newline after the disclosure and joins the copied sentences with spaces. `required_sentence_indices` is an optional array of up to eight unique, zero-based indices in the sentence sequence produced by the operation's punctuation/line-break splitter. Every required sentence is selected first; remaining slots are filled by the existing extractive ranking, and final output stays in source order. An absent array means no required sentences. An out-of-range index suspends with `TEXT_SUMMARY_REQUIRED_INDEX`; required count above `max_sentences` suspends with `TEXT_SUMMARY_REQUIRED_LIMIT`; required text above `max_chars` suspends with `TEXT_SUMMARY_REQUIRED_CHAR_LIMIT`. Each stop creates no artifact. The operation preserves every selected sentence verbatim and ranks nonrequired sentences by the inherited extractive frequency score, a 6/5 first-sentence weight, a 3/4 penalty over 180 Unicode scalars, and the inherited reverse-source-order tie break. It skips a ranked sentence that would exceed the remaining character budget and continues to lower-ranked sentences that fit; it never truncates a sentence. The budget counts copied sentence characters, excluding the fixed disclosure label and either layout's markers and separators. The V4 artifact records the selected format, required, selected, omitted and length-limited sentence indices, selected character count, score fractions and exact source/output byte spans. It also identifies the exact JSON string token containing the decoded source text. If no sentence fits and none is required, Core suspends with `TEXT_SUMMARY_CHAR_LIMIT` and creates no artifact.

The visible output begins by stating that purpose, meaning, conditions, negation and coverage were not verified. Core records it as `HYPOTHESIS`. Input is limited to 100,000 Unicode scalars, 256,000 bytes and 4,096 sentence candidates; the serialized artifact is limited to 2,000,000 bytes and the request evidence budget. A required source index preserves an explicitly chosen sentence, not its semantic importance or truth. This is a deterministic extractive candidate ranking, not a semantic summary verifier or a claim that the user's content, purpose, length or format requirements were satisfied. The [candidate-route audit](audit/b08-extractive-summary-candidate-route-validation.json), [character-budget audit](audit/b08-extractive-summary-char-budget-route-validation.json), [output-format audit](audit/b08-extractive-summary-format-route-validation.json) and [required-content audit](audit/b08-extractive-summary-required-content-route-validation.json) record scoped normal-route evidence; the twelve-task B08 population remains open.

### Constrained previous-source summary consumer

After a successful fetched-body result, `前回の資料を2点以内、80字以内で要約して` or `Summarize the previous source in up to 2 bullets and up to 80 characters` carries those limits into the source-linked summary artifact and the normal Core output. This additive parser-v62 intent requires exactly one successful `FETCHED_BODY` packet in the committed same-case prior result; a missing prior result yields `MISSING_ACTUAL_RESULT`, a committed result without a body yields `SOURCE_BODY_UNAVAILABLE`, and multiple bodies yield `AMBIGUOUS_SOURCE_CONTEXT` without an artifact. Limits count copied sentence characters, excluding the candidate disclosure, bullet markers, separators and source metadata. Selected sentences remain verbatim; if no complete sentence fits, Core suspends without truncation. The report retains the exact prior result, packet reference, raw-body hash, extracted-text hash, URIs and observation data. Output continues to disclose that coverage is unverified and license information is unrecorded. The original unconstrained `前回の資料を要約して` route remains available and keeps its existing multi-packet behavior. The [constrained source-summary audit](audit/bb-source-summary-bounded-constraints-route-validation.json) records focused Linux fixture evidence; full B08 and BB-SOURCE acceptance remain unverified.

### Extractive summary of the previous result

After an ordinary committed result, `前回の比較を2文以内で要約して` or `Summarize the previous result in up to 2 sentences` uses the existing deterministic sentence extractor on the exact displayed text of the immediately previous committed result in the same actor and case. The bounded Japanese form also accepts a previous result, answer, comparison or explanation; the English form accepts a result, answer or comparison. Optional limits are 1–8 sentences/bullets and 1–100,000 Unicode scalar characters. Limits apply to copied sentence text; the disclosure and bullet markers are excluded. If no complete sentence fits, or the source scan exceeds the request action budget, the request suspends without a summary artifact.

Core rechecks the source ResultRecord, its identified ContentPlan, the actual render and their case-bound blobs before extraction. A typed report records both result IDs, the source content and render identities, source-text digest, exact selected source byte spans and output-unit digest. Those references and the original result dependencies remain on the new result. The next `詳しく説明して` rechecks the exact report and source, then identifies the selected sentences and their byte ranges. A summary and its explanation remain `HYPOTHESIS`: selection is extractive, and purpose, meaning, conditions, negation and coverage are unverified. Missing prior context and changed/missing source artifacts stop. The [previous-result summary route record](audit/bb-content-previous-result-summary-v68-route-validation.json) covers Japanese and English requests, direct and derived comparison sources, an exact local-file receipt and recovery without resending the write. It does not establish broad B08 or contextual-language acceptance.

### Protected text import and editing

The following ordinary CLI consumer uses the actual imported artifact and returned text hash as the next request's editing base:

```bash
python3 - "$minidora_bin" "$minidora_state" <<'PY'
import hashlib, json, subprocess, sys
binary, state = sys.argv[1:]
def run(header, body):
    request = header + "\n" + json.dumps(body, ensure_ascii=False)
    return json.loads(subprocess.check_output(
        [binary, "--db", state, "--case", "writing-edit", "--actor", "demo", "--json"],
        input=request, text=True))
base = run("text-import:", {"text": "Draft", "protected": [], "max_chars": 256})
located = run("text-locate-replacement:", {
    "base_artifact": base["dependencies"][-1],
    "expected_text_sha256": hashlib.sha256(base["text"].encode()).hexdigest(),
    "search": "Draft", "replacement": "Final", "id": "title",
    "reason": "Requested title update"})
proposal = json.loads(located["text"])["proposed_edit"]
edited = run("text-edit:", proposal)
print(edited["text"])
PY
```

`text-import:` / `文章を取り込んで:` accepts exactly `text`, `protected` and `max_chars`. Each protected range has exactly `id`, `start`, `end` and `expected`; coordinates are half-open UTF-8 byte offsets in the decoded original text. `text-edit:` / `文章を編集して:` accepts exactly `base_artifact`, `expected_text_sha256` and `edits`. Each edit has exactly `id`, `start`, `end`, `expected`, `replacement` and `reason`. The artifact digest identifies a stored document; the expected digest identifies its text. English headers are ASCII case insensitive.

Core reads bases only from the frontend's bound actor and current case. CLI actor configuration remains a trusted local identity. Supported bases are actual import, composition and earlier editing artifacts. Each base is regenerated from its original request and verified ancestry, then compared with its complete canonical stored bytes. Composition's protected quote/condition/caveat units remain protected. All edits use the same pre-edit coordinates and are validated before any replacement. Empty expected text inserts at a boundary; empty replacement text deletes. Adjacent edits are allowed; overlapping edits, equal starting positions, stale hashes, nonmatching original text, invalid UTF-8 boundaries and protected-text changes suspend. Insertions at a protected boundary preserve the protected text and update its location.

`text-locate-replacement:` / `文章の置換箇所を特定して:` accepts exactly `base_artifact`, `expected_text_sha256`, `search`, `replacement`, `id` and `reason`. It finds one exact, nonempty literal in a replay-verified artifact and emits a `TEXT_EDIT_LOCATE_UNIQUE_LITERAL_V1` proposal with UTF-8 byte coordinates and the source JSON token ranges for the search and replacement. Missing text, repeated text (including overlapping matches), and protected matches suspend. The proposal includes a complete `text-edit:` request; location does not apply it. Submit that request through `text-edit:` to make the change. This bounded operation does not infer a correction from prose or determine whether a requested replacement preserves meaning.

The result records changed and unchanged output ranges, actual base/ancestor artifact digests and source hashes. Unchanged text points to its parent artifact; inserted text points to the current request's replacement string. Limits are 64 edits per revision, 256 protected ranges, 16 editing revisions, 256,000 request/output bytes, inherited `max_chars` up to 100,000 scalar values, 2,000,000 bytes per artifact and 8,000,000 aggregate bytes during ancestry replay. Replay and patch application poll the request deadline; in-progress cancellation polling is not implemented. Exact request retries retain the saved result and file receipt; a new explicit input version retains prior results. Local file output uses the existing exact-target write grant.

These operations locate or apply explicit corrections and preserve provenance. Discovering a correction from a natural-language request, preserving arbitrary rewritten meaning, and producing a summary that satisfies audience, style, purpose or semantic coverage remain open requirements. The [protected-editing audit](audit/b08-protected-text-editing-route-validation.json), [unique-location audit](audit/b08-unique-literal-edit-location-route-validation.json) and [extractive candidate audit](audit/b08-extractive-summary-candidate-route-validation.json) record scoped normal-route observations; full B08 acceptance remains `UNVERIFIED`.

### Natural editing of the actual previous answer

After a committed result, `前回の回答の「old」を「new」に置き換えて` or `Replace "old" with "new" in the previous answer` requests one exact literal edit of that same-case result. Core verifies the saved ContentPlan and rendered bytes, requires one unique match including overlapping occurrences, and stores a new edit artifact with the parent result, content-plan, render, source-text hashes and UTF-8 source/output spans. The earlier result remains unchanged. Missing output, no match, or repeated/overlapping matches suspend without an edit artifact. The result is `USER_PREMISE`: the replacement is literal and user-directed; meaning, factual accuracy, citation validity and format suitability are not rechecked. A bounded fresh-process file-receipt recovery verified all parent/edit artifacts and reconciled the observed file as `LOCAL_FILE_RECONCILED_NO_RESEND`. See the [previous-output edit audit](audit/bb-context-previous-output-literal-edit-v63-route-validation.json). F04/B08 and full acceptance remain `UNVERIFIED`.

## Exact-target file output

```bash
minidora_report=/workspace/.minidora-cloud/rust-result.txt
"$minidora_bin" --db "$minidora_state" --actor demo --case file \
  --allow-write "$minidora_report" --output "$minidora_report" '17*23'
```

`--output` requests a file; **only** `--allow-write` grants that exact target. Core records the action identity and authorization before the physical writer runs, then checks actual bytes and its receipt before goal `COMMIT`. Parent directories must already exist and be trusted. Different existing bytes are not overwritten; a matching existing artifact can be verified. Symlinks, traversal and unconfirmed write/durability outcomes have explicit failure/unknown paths. The portable path checks are not an OS sandbox against hostile concurrent ancestor replacement.

## Inspect and reconcile existing effects

### Explicit legacy Rust store migration

Ordinary alpha 11 opens reject `minidora-rust/0.7.0-alpha.7` through `minidora-rust/0.7.0-alpha.10` metadata without changing the store. To migrate an existing Rust store, run the dedicated command with its exact state directory:

```bash
"$minidora_bin" migrate --db /path/to/legacy-state --json
```

Migration requires `--db`, accepts alpha 7, alpha 8, alpha 9 or alpha 10 metadata, and verifies the existing SQLite profile, foreign keys, referenced blob bytes, case revision chains, action journals and input-conflict chains before changing state. It then updates only `store_metadata.schema` to alpha 11. Case/effect rows and digest-addressed blob bytes remain intact; old case revisions retain their original schema identity and remain readable. A valid alpha 11 store returns `"migrated":false` after verification. Unsupported versions, missing state files, a non-WAL store or any integrity failure stop before the metadata change. `MIGRATION_DURABILITY_UNKNOWN` means the commit or its final directory sync could not be confirmed; rerunning the command verifies the store and reports whether migration already completed. The frozen Python beta has a separate store and is not migrated by this command.

```bash
"$minidora_bin" inspect --db "$minidora_state" --actor demo --case file
"$minidora_bin" reconcile --db "$minidora_state" --actor demo --case file --json
```

`reconcile` inspects the saved original local-file operation, authority, adopted artifact and actual existing target. It accepts no new request, permissions or output path, performs no write/resend and creates no new action token. An already committed case returns its saved result. An eligible adopted/suspended file case can reach `COMMIT` only after the original content, proof, graph and actual file confirmation are verified. Unsupported or unconfirmed recovery remains suspended.

同じ会話への別入力が先に保存版を進め、処理に入る前に競合した入力は `SUSPEND` の `CASE_CHANGED_BEFORE_ADMISSION` を返す。同じ入力IDで再送するとこの保留結果を取得できる。会話の状態を確認してから、必要な依頼を新しい入力IDで送る。

For a case already cancelled with an unresolved issued effect, the separate command is:

```bash
"$minidora_bin" reconcile-effects --db "$minidora_state" --actor demo \
  --case CANCELLED_CASE_ID --json
```

Replace `CANCELLED_CASE_ID` with that actual saved case identity. This command observes the cancelled case's original local-file actions and updates their ledger/receipts only; it **preserves case `CANCEL` and never commits or restarts the cancelled goal**. Its exit 0 means recorded effects are confirmed, failed or cancelled; exit 2 means an effect remains unresolved. Neither status is a new user-goal success. A cancellation request such as `停止` stops future work and retains previously observed or unknown effects; it does not make an issued effect disappear. No generic remote exactly-once guarantee follows from this local observation path.

## Direct HTTPS source bodies and exact JSON selection

```bash
"$minidora_bin" --db "$minidora_state" --actor demo --case source \
  --allow-read-host raw.githubusercontent.com \
  '取得 https://raw.githubusercontent.com/gatchimuchio/MINIDORA-BETA/1c546e77007afa0a9eeb3a44f07da99d84c383eb/src/minidora/engine_beta.py'

"$minidora_bin" --db "$minidora_state" --actor demo --case json \
  $'json-pointer /a~1b/1\n{"a/b":[1,9007199254740993]}'
"$minidora_bin" --db "$minidora_state" --actor demo --case json-stats \
  $'json-statistics /scores\n{"scores":[0.1,0.2,0.3]}'
"$minidora_bin" --db "$minidora_state" --actor demo --case json-project \
  $'json-object-project ["metrics","name"]:\n{"name":"sample","metrics":{"score":1.2300e+40},"private":"omit"}'
"$minidora_bin" --db "$minidora_state" --actor demo --case json-compare \
  $'json-compare:\n{"left":{"name":"cat","score":1},"right":{"score":1.0,"name":"cat"}}'
"$minidora_bin" --db "$minidora_state" --actor demo --case json-table-compare \
  $'json-table-compare "id":\n{"left":{"columns":["id","name"],"rows":[["1","cat"],["2","dog"]]},"right":{"columns":["id","name"],"rows":[["1","fox"],["3","owl"]]}}'
"$minidora_bin" --db "$minidora_state" --actor demo --case json-table-join \
  $'json-table-inner-join "id" "record_id":\n{"left":{"columns":["id","name"],"rows":[["2","dog"],["1","cat"],["4","fox"]]},"right":{"columns":["record_id","kind"],"rows":[["1","feline"],["3","bird"],["2","canine"]]}}'
```

A network read requires both the exact application hostname grant and the environment's outbound policy. Redirect destinations require their own grants. The current port accepts bounded HTTPS:443 UTF-8 `text/plain`, `text/markdown` and `application/json`, preserving raw bytes, hashes, actual URI/status/failure and provider time. It uses inherited proxies and TLS trust. Private-address and malformed URL paths are rejected. An observed source report is not a verified factual conclusion.

This direct body port does not implement HTML/PDF extraction, arbitrary source comparison or open-domain summarization. Provider metadata search is a separate explicit operation described below. A URL plus additional unsupported instructions suspends instead of quietly dropping the extra task. Fixed packets and LIVE observations remain distinct evidence modes.

The JSON Pointer operation validates the complete document, rejects duplicate keys and invalid pointer syntax, and returns the exact selected source slice. Its input body retains the original whitespace and numeric lexemes, and the source-preserving JSON parser performs the complete validation without a preliminary floating-point JSON conversion. A raw number such as `1e400` can be selected without evaluating its magnitude. Invalid document bodies suspend during the document operation; invalid pointer prefixes remain input failures. `json-object-project ["metrics","name"]:` or `JSONオブジェクトからフィールド["metrics","name"]を抽出して:` selects a nonempty unique list of root-object fields and returns them in that order. Each selected value's original JSON bytes are preserved (including numeric lexemes), while the source-map artifact retains original key/value ranges, output ranges and JSON Pointers. Missing or repeated names, non-object roots, malformed JSON and limit overflow suspend. `json-statistics /scores` or `JSONの配列「/scores」の統計量を計算して:` explicitly selects a JSON array containing only number literals and computes exact descriptive statistics. The report retains the complete source digest, selected array bytes, each numeric lexeme and its byte range. Empty, mixed-type, nested, oversized or non-array selections suspend. No population inference is made.

`json-compare:` or `JSONを比較して:` accepts one JSON wrapper with exactly `left` and `right` values. It rejects duplicate keys, ignores object key order, compares array order and decoded strings exactly, and treats numerically equal JSON lexemes such as `1` and `1.0` as equal using bounded exact decimals. The difference report uses JSON Pointers and source byte ranges/digests for each side; absent fields are distinct from explicit `null`. Inputs, numeric magnitude, difference count and serialized output are bounded. This is a structural comparison and does not infer factual truth or general semantic equivalence.

`json-table-compare "id":` or `JSON表を列「id」で比較して:` accepts the same `left`/`right` wrapper with string-table objects (`columns` and `rows`). Both decoded column lists must match in the same order, and the selected key column must be unique on each side. The report lists added/removed rows and changed non-key cells in source order, with separate JSON Pointers, byte ranges and digests for each side. Keys and cells are exact strings; no trimming, normalization or numeric inference is applied.

After a committed JSON table comparison, the next same-case `詳しく説明して` request replays the comparison from its saved input and checks the prior report and rendered result. It explains the equality or difference counts and up to eight changed cells/rows with their pointers and original byte ranges and hashes; the complete report stays in its artifact. This explains the exact string comparison and makes no claim about semantic equivalence or factual truth. An authorized local-file write can be reconciled from the same inputs without resending it. See the [JSON table comparison explanation route](audit/bb-context-json-table-comparison-explanation-v74-route-validation.json); B04, A07, A14 and E08 remain `UNVERIFIED` beyond this bounded route.

`json-table-inner-join "id" "record_id":` or `JSON表を「id」と「record_id」で内部結合して:` accepts a `left`/`right` wrapper with two strict JSON string tables. Each selected key must be unique in its own table. Exact matches produce rows in left-table order, with output columns named `left.<column>` and `right.<column>`, both key columns retained, and original row indices and cell ranges recorded for each side. Unmatched keys are listed separately; no trimming, normalization, numeric inference or one-to-many expansion is applied.

`json-table-group-statistics "region" "sales":` or `JSON表の「region」ごとに「sales」の統計量を計算して:` groups exact decoded string values in first-seen order and interprets only the explicitly selected value column as unitless exact expressions or unit-bearing decimal literals. Mixed interpretations within a group stop; mixed units convert only within the existing `mm`, `cm`, `m` and `km` family. The report retains each group and value cell byte range and digest, row index and exact statistics. Other columns remain strings, and no population inference is made.

## Explicit evidence ledger

```bash
"$minidora_bin" --db "$minidora_state" --actor demo --case evidence \
  $'evidence-synthesize:\n{"sources":[{"source_id":"doc-a","independence_group":"publisher-a","claims":[{"id":"c1","text":"The bridge opened in 2010.","stance":"SUPPORTS"}]},{"source_id":"doc-b","independence_group":"publisher-b","claims":[{"id":"c1","text":"The bridge opened in 2010.","stance":"REFUTES"}]}],"inferences":[{"id":"i1","text":"The date is disputed.","premises":["c1"]}]}'
```

`evidence-synthesize:` or `証拠を整理して:` consumes a strict JSON ledger with `sources` and `inferences`. Each source supplies a unique ID, an `independence_group` label and exact claim IDs/text/stances (`SUPPORTS` or `REFUTES`). Same-text claims with matching IDs are classified as `AGREEMENT`, `CONTRADICTION`, `SAME_GROUP_REPETITION` or `SINGLE_SOURCE`; citations retain zero-based source indexes and input byte ranges. The group label is user-supplied and is not independently checked. Declared inferences may reference claim IDs or earlier inference IDs; the route checks links and order only, and labels each inference unvalidated. It does not fetch documents, extract claims from prose, verify source identities, or prove entailment. The report is a structured-ledger operation, not full B05 acceptance.

## Explicit provider metadata search

```bash
"$minidora_bin" --db "$minidora_state" --actor demo --case search \
  --allow-read-host ja.wikipedia.org '検索 wikipedia: 重力'
"$minidora_bin" --db "$minidora_state" --actor demo --case search-crossref \
  --allow-read-host api.crossref.org 'search crossref: quantum gravity'
"$minidora_bin" --db "$minidora_state" --actor demo --case search-europepmc \
  --allow-read-host www.ebi.ac.uk '検索 europepmc: protein folding'
```

The current provider IDs select Japanese Wikipedia REST metadata, Crossref works or Europe PMC metadata. Both the application host grant and environment outbound policy must permit the destination; these examples can truthfully suspend when access is unavailable. A query is one bounded line after the provider/colon. A complete quoted JSON string can carry literal query words that would otherwise resemble an extra action; it does not authorize that action.

The result is a labeled ranked metadata listing, retaining provider/raw packet identity, source keys, rejected candidates and failures. **No article body is fetched merely by listing a candidate**, and a title, URL, snippet or abstract is not represented as having read the paper/page. The library has a separately authorized candidate-body observer; the ordinary CLI can also receive a supported explicit body URL as a distinct request. HTML/PDF parsing, source-body synthesis, independent factual inference and full search→calculation→report closure remain open obligations. Provider reachability and LIVE quality are separate from local synthetic/list-parser tests and the GPQA contract.

After a committed fetched-body result, a bounded Japanese/English source question can return exact lexical sentence candidates with source spans and packet hashes. New V2 answers group packets by exact `final_uri`, use the uniquely latest observed body/parser version, and retain older version references without treating them as separate current sources. Conflicting body/parser identities at the latest timestamp and mixed `LIVE`/`FIXTURE` packets suspend. The result remains `HYPOTHESIS`; matching words do not establish semantic relevance, source independence, factual truth, contradiction handling or completeness.

## Local authenticated HTTP

`serve` is a loopback-only, single configured actor interface to the same durable Core. It requires a process token of at least 32 printable ASCII bytes. The following single-shell example generates a temporary local token without displaying it, starts the server, sends a full request and stops only that server. Use a free local port if 8765 is already occupied.

```bash
read -r MINIDORA_API_TOKEN < <(python3 -c 'import secrets; print(secrets.token_hex(32))')
export MINIDORA_API_TOKEN
"$minidora_bin" serve --db "$minidora_state" --actor demo \
  --bind 127.0.0.1:8765 > /workspace/.minidora-cloud/rust-http.log 2>&1 &
minidora_server_pid=$!
trap 'kill "$minidora_server_pid" 2>/dev/null || true; wait "$minidora_server_pid" 2>/dev/null || true' EXIT
python3 - <<'PY'
import json, os, time, urllib.error, urllib.request
base = 'http://127.0.0.1:8765'
for attempt in range(50):
    try:
        with urllib.request.urlopen(base + '/health', timeout=1) as response:
            assert json.load(response)['status'] == 'ok'
        break
    except (OSError, urllib.error.URLError):
        if attempt == 49:
            raise
        time.sleep(0.1)
request = {
    'case_id': 'http-example', 'input_id': 'request-1', 'input_version': 1,
    'actor_id': 'demo', 'text': '2+3', 'locale': 'ja',
    'permissions': {'read_hosts': [], 'write_paths': []},
    'output_path': None, 'context_ref': None,
    'budget': {'max_input_bytes': 256000, 'max_evidence_bytes': 2000000,
               'max_actions': 32, 'deadline_ms': 30000},
}
message = urllib.request.Request(
    base + '/v1/execute', data=json.dumps(request).encode('utf-8'),
    headers={'Content-Type': 'application/json',
             'Authorization': 'Bearer ' + os.environ['MINIDORA_API_TOKEN']})
with urllib.request.urlopen(message, timeout=35) as response:
    print(response.read().decode('utf-8'))
PY
kill "$minidora_server_pid"
wait "$minidora_server_pid" 2>/dev/null || true
trap - EXIT
unset MINIDORA_API_TOKEN
```

`GET /health` is a non-mutating local readiness endpoint. `POST /v1/execute` requires the token, matching configured actor, permitted host/origin and application grants no broader than server `--allow-*` options. Request budgets are capped by server limits. Actor strings supplied to the local CLI/library are trusted host configuration, not remote authentication. Protect the state directory with host permissions. This plaintext loopback transport is not a deployed multi-user service, browser-chat UI or external-access acceptance claim.

CLI terminal exit codes are `COMMIT=0`, `SUSPEND=2`, `REJECT=3`, `FAIL=4`, `CANCEL=5`; frontend errors return 1. Content adoption alone is not goal completion.

## Evidence and remaining work

The [identity audit](audit/README.md) accounts for the original 414 source blobs plus two beta additions, all 359 upstream test/fixture paths plus the beta test, and all 93 named obligations. File identity, source inspection, local tests, normal-route integration, independent task acceptance and deployed acceptance are different levels.

The current model uses an inspected predecessor character-ngram family with exact conditional probabilities and constrained surface choices. It does not establish open-domain text generation quality or close the inherited-model-versus-alternative comparison gate. General language/writing/code/research quality, all semantic source routes, complete Rust equivalents of predecessor regressions, generalized learning, long-history measurements, external applications/modalities, full cross-target equivalence, Windows/8GB and GPQA LIVE acceptance remain open requirements. No historical beta or benchmark score is a new Rust score.

The original large-body deadline failure and its correction are kept separate in the evidence. The [same-source retest](audit/resource-same-source-retest.json) verified the exact original **10,452-byte** body and SHA-256, then returned typed `SUSPEND` after **6.699 seconds** at the explicit **8192-bit** probability-operation cap. This is finite-stop evidence, not successful rendering of that document. The [different-source admission test](audit/resource-failure-retest.json) fetched a **22,888-byte** upstream README and hit a separate 16,384-byte model admission cap; it is not a causal same-input reproduction.

The [target vector](audit/target-conformance.json) records matching domain canonical Unicode/exact-string records and lifecycle predicates on native Linux and Node WASI execution of `wasm32-wasip1`, with an independent Python hash check. This narrow domain-vector result does not establish whole-Core, storage/effect/backend equivalence or physical Windows/8GB behavior.

See [implementation choices](docs/IMPLEMENTATION_CHOICES.md), [dependencies](docs/DEPENDENCIES.md), [current status](audit/IMPLEMENTATION_STATUS.md) and the normative [R2 specification](../docs/redesign/01_SYSTEM_SPECIFICATION.md). No remote publication, deployment or frozen-release replacement follows from building this alpha.


## Current validation and open scope

The BODY-1 [saved-relation comparison explanation route](audit/bb-context-saved-relation-comparison-explanation-v72-route-validation.json) connects the v71 comparison artifact to the same-case `詳しく説明して` consumer. It rechecks the prior report and rendered output, recomputes both actor-scoped inventories and retains each proof dependency; changed support returns `CONTEXT_PROOF_CHANGED`. The fresh-process record covers 20 requests (18 `COMMIT`, 2 `SUSPEND`). This bounded route does not close general language, writing or full A07/A09/B01/F04 acceptance.

See [the historical alpha 7 validation record](audit/validation.json), [the B02 unit-arithmetic validation](audit/unit-arithmetic-validation.json), [the unit-bearing sequence build record](audit/unit-sequence-validation.json), [the initial operand-correction validation](audit/unit-correction-validation.json), [the bounded operator-correction validation](audit/unit-binary-correction-validation.json), [the store migration validation](audit/schema-migration-validation.json), [the CSV statistics build record](audit/csv-statistics-validation.json), [the JSON statistics build record](audit/json-statistics-validation.json), [the JSON comparison build record](audit/json-comparison-validation.json), [the JSON table comparison build record](audit/json-table-comparison-validation.json), [the JSON table inner-join build record](audit/json-table-inner-join-validation.json), [the CSV table inner-join build record](audit/csv-inner-join-validation.json), [the explicit evidence synthesis build record](audit/explicit-evidence-synthesis-validation.json), [the exact probability-weight normalization build record](audit/probability-normalization-validation.json), [the finite-distribution moments build record](audit/finite-probability-statistics-validation.json), [the finite-event probability build record](audit/finite-event-probability-validation.json), [the polynomial-evaluation build record](audit/polynomial-evaluation-validation.json), [the linear-equation build record](audit/linear-equation-validation.json), [the quadratic-equation build record](audit/quadratic-equation-validation.json), [the structured table-conversion build record](audit/structured-table-conversion-validation.json), [the CSV filter build record](audit/csv-filter-validation.json), [the CSV sort build record](audit/csv-sort-validation.json), [the CSV group-count build record](audit/csv-group-count-validation.json), [the CSV grouped-statistics build record](audit/csv-group-statistics-validation.json), [the CSV table-comparison build record](audit/csv-table-comparison-validation.json), [the JSON table-filter build record](audit/json-table-filter-validation.json), [the JSON table-sort build record](audit/json-table-sort-validation.json), [the JSON table-group-count build record](audit/json-table-group-count-validation.json), [the JSON table grouped-statistics build record](audit/json-table-group-statistics-validation.json), [the JSON table-select build record](audit/json-table-select-validation.json), [the JSON table-projection build record](audit/json-table-project-validation.json), [the JSON object-projection build record](audit/json-object-project-validation.json), [calculation-sequence build record](audit/calculation-sequence-validation.json), [bounded derived-relation inventory route](audit/bb-lang-derived-relation-inventory-20261113-route-validation.json), [all93 requirement observations](audit/rust-requirement-observations.json), [B51 temporal-scope validation](audit/construction/MB-BUILD-20261115/b51-temporal-scope-validation.json) and [implementation status](audit/IMPLEMENTATION_STATUS.md). The historical alpha 7 run passed 134 Rust tests and 8 separately labeled local TLS fixture cases; the preserved predecessor passed 313 selected tests separately. Later alpha 8 Linux runs passed 147 and 148 Rust tests in their separately identified snapshots. These scoped observations do not establish full R2/93-capability successor acceptance. Unrestricted natural-language work, the complete inherited semantic/regression audit, LIVE benchmark and physical Windows/8GB validation remain incomplete. No missing item is an approved release-scope exclusion.

Tested cloud reproduction and startup instructions are [cloud-install.sh](tools/cloud-install.sh) and [cloud-start.md](tools/cloud-start.md). Backend environment-draft saving is currently unconfirmed due to an API error; the scripts themselves were executed successfully.
