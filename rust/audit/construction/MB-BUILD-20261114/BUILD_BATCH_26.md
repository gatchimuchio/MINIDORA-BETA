# BUILD_BATCH_26 — 文脈否定他動詞assertの半角カナ原文spanを再開proofまで接続

日付: 2026-11-14

## 対象と施工単位

- 対象: `gatchimuchio/MINIDORA-BETA`、`main`
- 着工時`HEAD` / `origin/main`: `1b17fb0fe7e551ecafded58be3e69ab25f8b706e`（B25）
- 主語: MINIDORA本体の中核。
- 目的: B25の文脈assert入力から証明Factへの経路について、別の実際の日本語入力表面（`が`・否定丁寧形・半角カナ目的語）でも原文span、否定意味、文脈結果、保存再開後のproofと後続利用が同じFactへつながることを確認して保持する。
- 実Consumer: `Engine::execute` → `input::seal` v79 → literal source-role mapと実際の`context_result_ref` → Core直前ResultRecord/proof照合と主語復元 → USER_PREMISE Factの`SealedSourceEvidence` → SQLite memory close/reopen後のStore再検証 → `relations::query` → 通常Core Askが同Fact IDを消費。
- 範囲: `[scope=biology]それがﾐｽﾞを使いません。`の一件。全て合成したローカルfixture。日本語全般、語形全般、NFKCによる保存値の同値化、外部世界の真偽は扱わない。
- 権限: オーナーの「3点セットで実装し、リポにも残す」指示の継続範囲。別repo、外部送信、課金、配布、releaseは行わない。

## 開始時の同期と工程

`AGENTS.md`全文と3点セット3文書全文を読み、archive・各member SHA-256は`MANIFEST.tsv`と一致することを確認した。ZIP SHA-256は`1f65df9f67448406d0fe1e9d3b1e13981afa236f352223cbb61ce1541bc1371e`。着工前worktree clean、`main=origin/main=1b17fb0`、別writerは観測されず、作業時点の`rust/rust-toolchain.toml`はRust 1.99.1。現在は工程表の第五工程「中核の必要条件から入力機を施工」の有限route確認であり、三点セットやR2の設計変更ではない。

開始時にlocal recovery branch refsがremote recovery tagsより一世代ずれていた（local branchesはB24/B23、remote tagsはB25/B24）。既存履歴と現在HEADを照合し、編集前にlocal `codex/backup-main=B25`、`codex/backup-main-prev=B24`へ合わせた。remote tagsは既に同じ二世代だった。B26完了時にB26/B25へ回転する。

## 実観測

同一caseで `[scope=biology]ミナは鳥です。` を登録し、その実際の結果から関係Askをcommitした。次入力 `[scope=biology]それがﾐｽﾞを使いません。` は直前結果IDを`context_result_ref`に束縛して封印された。

| 役割 | 原文表記 | 観測したUTF-8範囲 |
|---|---|---|
| subject | `それ` | start 15, end 21 |
| subject particle | `が` | start 21, end 24 |
| object | `ﾐｽﾞ` | start 24, end 33 |
| object particle | `を` | start 33, end 36 |
| predicate | `使いません` | start 36, end 51 |
| scope | `biology` | start 7, end 14 |

Coreは元のsubject表記`それ`を証拠spanに保ったまま、Fact上の主語を`ミナ`へ復元し、述語`use`と否定polarityを保存した。Factにはraw source digest、sealed bundle fingerprint、input fingerprint、semantic signature、parser v79、六つのliteral role spanが保存された。Engine再開時のStore読取で同じsealed evidenceが再検査され、relation proofは`SUPPORTED`と同じ証拠Factを返した。通常の後続Core AskはそのFact IDをdependencyとして消費した。

半角カナの目的語値`ﾐｽﾞ`自体は保存された表記のままである。この観測はNFKC意味同値化の確認ではなく、その原文byte rangeがCore Fact/proofの来歴に結び付くことの確認である。これは利用者提示前提の出所確認であり、世界の真偽を立証しない。Engine API経路でありCLI/UI、実描画、送達は通していない。

B26は、B25で既に実装されているv79経路の追加実観測・回帰testである。production parser、parser version、DB schema、依存関係、Cargo.lockに変更はない。

## 検証

| コマンド | 結果 |
|---|---|
| `cargo test -p minidora-engine --locked --lib contextual_negative_japanese_transitive_assertion_preserves_halfwidth_surface_roles_after_reopen -- --nocapture` | PASS: 同case anchor→seal→Fact→reopen→proof→後続Core Ask |
| `cargo test -p minidora-engine --locked` | PASS: 242 unit + 12 integration = 254 passed、0 failed; doc tests 0 |
| `cargo fmt --all -- --check` | PASS |
| `cargo check --workspace --all-targets --locked` | PASS |
| `cargo clippy --workspace --all-targets --locked -- -D warnings` | PASS |
| `cargo build --workspace --locked` | PASS |
| `git diff --check` | PASS |

コマンド別の完全ログは`/workspace/.minidora-cloud/b26-contextual-halfwidth-assert-source-evidence/`に8 files保存した。全てexit 0。focused recordの原文・scope・source hashes・dependenciesは[validation observation 09](../../bb-input-machine-observation-09-v79-contextual-halfwidth-negative-assert-validation.json)に記録した。

Full workspace all-features runtime、CLI/UI/output、Windows/macOS、実機8GB、独立QA、全A04/A15/E08評価はNOT_RUN。これらと要求93件全体は`UNVERIFIED`。中核施工観測や三機通電の完了、全体系躯体完成、品質検査合格、受入成立、公開可は宣言しない。

## 変更パス

- `rust/crates/minidora-engine/src/core.rs`（追加した一つのCore execution observation）
- `rust/audit/IMPLEMENTATION_STATUS.md`
- `rust/audit/requirements.tsv`（A04/A15/E08の観測追記。disposition=`NEW`、acceptance=`UNVERIFIED`維持）
- `rust/audit/bb-input-machine-observation-09-v79-contextual-halfwidth-negative-assert-validation.json`
- 本`BUILD_BATCH_26.md`

## B26資産の5分類

| 分類 | 件数 | 内容 |
|---|---:|---|
| retained | 5 paths | Core observation、status、requirements、validation JSON、本施工記録をGitへ保持 |
| archived | 8 files | format、focused Core route、engine package、workspace check/Clippy/build、diff-checkログ |
| active | 3 directories | `/workspace/.minidora-cloud/rust-target`、`/workspace/.cargo`、既存`rust/target`。共有・既存cacheとして再利用 |
| trash | 0 | trash移動なし、永久削除なし |
| pending | 0 | B26で生成・使用した資産の未分類なし |

テストが作る一時SQLite stateはcleanupで除去された。3点セットと全manifest memberはSHA一致を再確認し、変更していない。
