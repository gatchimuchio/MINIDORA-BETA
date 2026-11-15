# BUILD_BATCH_51 — Core関係の時刻範囲を独立した識別子にする

- **基線HEAD:** `f789ff50ff9fb11ddc52fab7526a7d38888e875d`（B50）
- **対象:** R4のCore G01にある時間条件を、world `scope`と分離して関係の同一性・rule適用・proof照合に反映する。
- **施工単位:** 型付きCore関係の保存と説明、時間差を持つ比較の分類、alpha7/8からalpha9への明示store migration。
- **契約境界:** `temporal_scope`は入力元が渡す不透明な完全一致label。日付parser・区間演算・順序推論は追加しない。CORE-G01全成立述語を閉じたとは扱わず、入力機施工にも進まない。

## 変更

`Proposition`へ任意の`temporal_scope`を追加し、world `scope`と別のfieldとして保存した。値は非空・上限付き・制御文字なし・変数markerなしを要求する。値なしではserde出力からfieldを省略し、alpha7/8時代の正準Proposition形とidentityを維持する。Coreの関係shape照合、rule-premise/conclusion制約、proof queryと合成ruleの適用に完全一致時間を通し、関係表示にもlabelを付ける。型付きEngine API `assert_atomic_relation`と`add_atomic_relation_rule`は完成文を受けず、通常Core request/seal/persist経路を使う。

Coreの比較inventoryは複数時刻を含み得るため、比較group keyにも時間を含めた。異なる時刻の同型関係は別groupとなる。時間なしの比較unit keyは従来の4要素キーと同じhashを生成する回帰確認を追加し、alpha7/8からの旧比較artifact再生への影響を避ける。

`SCHEMA_VERSION`とworkspace packageを`0.7.0-alpha.9`に更新した。通常openはalpha7/8を拒否し、明示migrationは全体のintegrityを検証した後にmetadataだけをalpha9へ変更する。旧CaseRevisionのschema文字列・rows・immutable blobは変更しない。旧alpha7 migration APIの呼出互換とalpha8 migrationを加え、CLI/READMEのcurrent migration説明を更新した。

## producer → consumer → 実結果

隔離Core storeへ時刻`2026-11-15T10:00Z`の型付きrule `$x is ore → $x is refined`と`Nera is ore`を保存した。同じ時刻の`Nera is refined` queryはproof IDsを保持して`COMMIT / DERIVED`、ContentPlanとK7 renderも`RENDERED`となり、出力とsealed intentに同じ時刻labelが残った。`2026-11-15T18:00Z`の同じworld/subject/conclusion queryは`SUSPEND / MISSING_PREMISES`で止まり、ContentPlan/renderは作られなかった。

同じscopeでAsterの`rare ore`を10:00、Berylの同一関係を18:00として保存しCore比較を実行した。両時刻labelと左右の個別証拠が比較文に残り、「両側で確認した同じ関係」とは分類しなかった。詳細traceは[`b51-build-observation.json`](b51-build-observation.json)。

## focused verification

- `cargo test --locked -p minidora-engine --test core_atomic_relation_comparison_41 --test core_atomic_relation_comparison_43 --test core_temporal_scope_51 -- --nocapture`: **PASS**（3 integration tests）。V4 replay/K7、同一caseでのV1〜V4 coexistenceと再起動後replay、B51時刻一致/不一致と比較分類。
- `cargo test --locked -p minidora-engine --lib explicit_`: **PASS**（17 matching unit tests。alpha7/alpha8 migration testsを含む。233件はfilter対象外）。
- `cargo test --locked -p minidora-engine --lib untimed_comparison_key_identity_matches_the_legacy_shape`: **PASS**。
- `cargo test --locked -p minidora-domain unqualified_legacy_propositions_keep_their_canonical_bytes`: **PASS**（1 test）。初回はraw入力JSON順をcanonicalizerのsorted-key出力と同じと誤って期待したため失敗。元失敗を[`b51-initial-canonical-fixture-failure.log`](b51-initial-canonical-fixture-failure.log)へ保存し、正しいcanonical形を期待するtestに修正した。
- `cargo test --locked -p minidora-cli schema_migration_accepts_only_a_database_path_and_json_output`: **PASS**（引数契約の1 unit test）。
- `cargo check --workspace --all-targets --locked`: **PASS**。
- `cargo fmt --all -- --check`: **PASS**。
- `git diff --check`: 最終確認 **PASS**。初回は末尾の余分な空行を指摘したため修正し、初回結果も[`b51-git-diff-check.log`](b51-git-diff-check.log)として保持。
- 個別ログ: [Core比較/時刻観測](b51-engine-focused-tests-locked.log)、[store migration](b51-migration-tests-locked.log)、[canonical互換](b51-domain-compatibility-test-locked.log)、[CLI migration](b51-cli-migration-test-locked.log)、[旧比較key identity](b51-legacy-comparison-key-test.log)、[workspace check](b51-workspace-all-target-check.log)、[format](b51-format-check.log)、[最終diff check](b51-git-diff-check-final.log)。

`cargo test --workspace`、全workspace runtime suite、Clippy、通常の自然言語/CLI/HTTPによる時刻付き入力、日時妥当性やchronology、別OSと物理電源断は**NOT_RUN**。migrationはalpha7/8用隔離fixtureでの観測であり全既存DB・障害モデルの受入ではない。

## ゲート判定と資産

このbatchは完全一致の時間labelを型付きCore関係に通す一つの局所経路を示す。時刻間の照合停止と比較groupingの修正を確認したが、自然言語時間解釈・区間推論・R4のG01全述語・Coreの他ゲートを成立させない。`CORE-G01=PARTIAL`、`CORE-G03=PARTIAL`を維持する。ほかのゲートは本batchで再判定しない。3点セットR4本文とそのZIP/manifestは変更しない。

| 区分 | 対象 |
|---|---|
| keep | alpha9 code/doc changes、B51 integration/unit tests、observation/validation JSON、BUILD_BATCH_51、本記録の全focused logsと初回fixture failure logs |
| store | 外部保管なし |
| in-use | `/workspace/.cargo` Rust toolchain/cache、`rust/target` build artifacts、git local/remote refs |
| trash | 削除なし |
| hold | なし |

ユーザーのリポジトリへ保存する既存指示に従い、現在の`main`へcommit/pushする。新branch/PRは作らない。基線HEADは本記録先頭のSHAとし、最終HEADはcommit後のGit履歴で追跡する。
