# BUILD_BATCH_23 — 直接Assertの出典範囲をCore証明Factへ接続

日付: 2026-11-14

## 対象と工程

- 対象リポジトリ: `gatchimuchio/MINIDORA-BETA`、`main`
- 作業開始版: `e9a275591d9f062d033c07d2a9a4e239414269a6`（B22）
- 現工程: 中核の必要条件を先行施工。直接日本語コピュラAssertに限り、入力封印の出典役割をCoreの関係Factへつなぐ。
- 主語: MINIDORA本体の中核。
- 目的: 原入力から封印された意味・出典を同じCore関係Factへ束縛し、後続問合せと再開で同じ根拠へ辿る。
- 実行者: Codex。論理正本R2と11月14日付3点セットに従って施工。全体設計の採否、全体系受入および公開承認を行ったとは扱わない。

## 開始時同期と参照資料

作業前に`AGENTS.md`全文、対象`main`、作業状態、回復参照、施工指示書全文、同封仕様書・工程表全文を確認した。作業開始時の`HEAD`、`origin/main`、GitHub `main`はいずれも`e9a275591d9f062d033c07d2a9a4e239414269a6`で、作業ツリーは清浄、writerは観測されなかった。別worktreeは作成せず、凍結旧Python・他repoへ変更を加えていない。

三点セットのSHA-256は同封マニフェストと一致した。

| 資料 | SHA-256 |
|---|---|
| ZIP | `1f65df9f67448406d0fe1e9d3b1e13981afa236f352223cbb61ce1541bc1371e` |
| 論理仕様R2 | `2e6ce6e181555cebd66ff2f7a57bc01188981ecf116c6b2555e3f1b406b8edbc` |
| 施工仕様書 | `d776f6a3ee57f0b8ba84a59461cc539e1888836d9de2ef08405830f7daa724ae` |
| Codex施工指示書 | `e348c05716a3bf2f7541be29096dc71c139f5c9d44efba49da93e02c1bbb37e8` |

現行のRust README、実装選択・依存記録、Cargo設定、`IMPLEMENTATION_STATUS.md`、A04/A15/E08、該当する実装義務とR2の入力・関係・記憶・保存節も照合した。R2 §4、§7、§8、§11、§12が本束の根拠である。

## 欠落と変更内容

B22では直接日本語Assertの`MeaningIR.source_roles`へ、元UTF-8入力内の役割範囲を保存できた。一方、Coreはtyped `Intent`から`Fact`を作り、Factから入力ID・命題・保証・依存しか残していなかった。したがって通常問合せの証明Factから、正確な役割スパンと封印版へ辿れなかった。

B23では、以下の一本を接続した。

`input::seal`の`MeaningIR.source_roles` → `input::assertion_source_evidence`の役割・元byte範囲照合 → `Core::execute`の直接`Assert` → version付き`Fact.source_evidence` → `Store::validate_memory_scope`による所有・封印InputBundle再照合 → `relations::query`のFact → 通常の後続AskのFact ID依存

照合処理は封印済みtyped propositionを再解釈しない。必要な各役割が一つだけ存在し、元文字列へ正確にスライスでき、互いに重ならず、原文内でscope→subject→particle→object→negationの順を保ち、NFKC上の主語・対象・明示scopeと一致し、粒子が`は`または`が`、否定表示がtyped polarityと一致する場合だけ来歴を付ける。明示`[scope=...]`の範囲欠落、曖昧、役割交換など不整合なら追加来歴を生成しない。Coreがその来歴を保存する際、Storeは登録済みrequest fingerprintから同じactorの封印済みInputBundleを取り出し、bundle fingerprint・raw digest・parser/semantic署名・完全な役割マップを再照合する。改変値は保存・復元時に拒否する。

`Fact.source_evidence`はserdeの任意項目で、旧Factは欠損を`None`として読め、空の場合には出力wireから省略する。DBのschema versionを変更する必要はない。Derived Fact自体へ直接Factのスパンを複写せず、従来のproof dependency ID経由で原Factへ辿る。役割スパンを世界の真偽や独立した推論前提とは扱わない。

変更ソース:

- `crates/minidora-domain/src/lib.rs` — version付き`SealedSourceEvidence`と互換なFact任意項目。
- `crates/minidora-engine/src/input.rs` — 封印済み日本語Assert役割表の保守的検査。
- `crates/minidora-engine/src/relations.rs` — source evidence付き直接Fact生成、構造検査、同一Fact IDの来歴競合拒否、旧Fact互換確認。
- `crates/minidora-engine/src/core.rs` — 通常Assert処理から上記の根拠を渡し、再起動・後続Ask・曖昧役割・改ざん拒否の直接Core観測を追加。
- `crates/minidora-engine/src/store.rs` — memory読書き時にactor所有のsealed InputBundleとの完全一致を再検査。
- `docs/IMPLEMENTATION_CHOICES.md`、`audit/IMPLEMENTATION_STATUS.md`、`audit/requirements.tsv`、本記録とvalidation JSON — 適用範囲・証拠・未確認境界。

## 実測した入力・中間値・消費・出力

実入力は`[scope=ｶﾞ]ｶﾞはｶﾞではない`。正規化後はscope・subject・objectが同値でも、元入力で各々のbyte範囲はscope`7..13`、subject`14..20`、粒子`20..23`、object`23..29`、negation`29..41`に分かれた。parser versionは`declarative-ja-en-v77-japanese-assertion-source-ranges`。登録入力fingerprint、InputBundle fingerprint、raw SHA-256、semantic署名もFact上に保存した。

Coreの返却内容:

```text
前提を保存しました: ｶﾞはｶﾞではない。
前提ID: fact:8069fdbfb4838e0f0d722e88b0a220bee3fcfe0891a5500d77e467a646d9c631
保証範囲: 使用者が提示した前提。現実世界の真偽を検証したものではありません。
```

Engine再起動後、通常経路の後続入力`[scope=ｶﾞ]ｶﾞはｶﾞではない?`を処理した。保存済みFactの同じsource evidenceが関係証明のFactとして返り、AskのResultRecordは上記Fact IDを依存に保持した。返却内容は`提示された前提の範囲では、ｶﾞはｶﾞではない（範囲: ｶﾞ）。`。どちらも`COMMIT`、保証は`USER_PREMISE`。画面表示・配信・ファイル送信は行わず、Engine返却の実文字列を記録した。外部provider/network/作用はない。

`defaultはdefaultではない`も通常Coreで封印・保存した。subject・particle・object・negationの範囲は保持され、parserが生成したdefault scopeの範囲は存在しないままである。duplicated subject map、同じ正規化値のsubject/object位置交換、および明示scopeマーカーがあるのにscope範囲がない構築確認はsource evidenceを作らず、保存前のraw digestを改変したmemoryは拒否され、保存memoryは不変だった。旧wire Factの`source_evidence`欠損も読み取り・再serializeできた。

## 検証

| コマンド | 結果 |
|---|---|
| `cargo test -p minidora-engine --locked` | 250 passed、0 failed、0 ignored。238 library tests、5 construction integrations、2 meaning-composition integrations、5 input-machine integrations。 |
| `cargo fmt --all -- --check` | PASS |
| `cargo check --workspace --all-targets --locked` | PASS |
| `cargo clippy --workspace --all-targets --locked -- -D warnings` | PASS |
| `cargo build --workspace --locked` | PASS |
| `git diff --check` | PASS |

全workspace runtime/all-features試験は今回未実施。Windows/他OS、広いassertion-family母集団、独立評価も未実施。これらの局所試験結果からA04/A15/E08またはR2全体の受入を推定しない。

## 工程判定と次の依存

- B23局所観測: 成立。封印役割範囲が直接Factに束縛され、Storeによるsealed InputBundle照合、保存後の再開、通常relation queryでの同一Fact消費を確認。
- 入力機→Coreの当該経路: in-process Engineで成立。CoreのFactからResult dependencyまで確認。
- 出力機の実描画/外部送達・元目的終端: 未観測。本要求は送達作用を持たず、画面・HTTP経路を通していない。
- 中核施工確認七項目・六言語機構、三機全域通電、全体系躯体、品質検査、独立受入、公開承認: 未完了/未実施。
- A04/A15/E08: `UNVERIFIED`のまま。
- 制限: 直接日本語コピュラAssertのみ。context-restored Assertと他parser familyは対象外。スパンは原Factへリンクされた由来情報であり、真偽の裏付けではない。
- 次の依存: 同じ来歴接続を、R2に適合する必要範囲で別Assertion family・Contextual Assertの実際のConsumerへ一つずつ広げる。一般言語能力の成立や受入を先取りしない。

## 資産・保全

| 分類 | B23終了時 |
|---|---|
| Gitに保持する変更パス | 10 |
| `.minidora-cloud/b23-source-role-fact-lineage`の検査ログ | 7 |
| 共有・既存cache | `/workspace/.minidora-cloud/rust-target` 5.9G、`/workspace/.cargo` 252M、`rust/target` 3.7G（既存・未変更） |
| trashへ移動 | 0 |
| 未分類・保留 | 0 |

テスト専用stateはRAIIで削除され、B23終了時に一時stateは残っていない。ZIPと三文書の同封版は変更していない。Recovery refsは変更をmainへcommit/pushした後、B22と同じローテーション規則で更新する。リリース・デプロイは行わない。

詳細値・コマンドログの照合値は[BB_INPUT_MACHINE_OBSERVATION_06 validation](../../bb-input-machine-observation-06-v77-fact-source-validation.json)に記録した。
