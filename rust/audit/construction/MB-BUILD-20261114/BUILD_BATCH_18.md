# BUILD_BATCH_18 — 入力meaning roleと原文byte spanの結合

## 工事境界

- 対象: 三点セット第六命令の原文・役割・出所保持。
- 問題: `InputBundle`は全文と全体byte span、parserが採用した`Intent`を保持するが、relationの抽出subject/object/negationや計算式が原文のどのbyte範囲に対応するかを1つの意味束として直接参照できない。
- 変更: `MeaningIR.source_roles` に選択されたsemantic roleから元原文の半開UTF-8 byte spanへの索引を追加。NFKC比較用文字列にはsource byte rangeの対応表を作り、出力Span.textは必ず未変更の原文sliceを使う。
- version/互換性: parser revisionを `declarative-ja-en-v74-role-source-spans` とする。新fieldはserde defaultで読み、空ならserialize時省略する。旧MeaningIR fixtureのcanonical bytesが変わらないことをテストし、DB schema revisionは変更しない。

## 実入力観測

1. `[scope=materials-lab]Neraはsensorではない` を通常の `Engine::execute` に渡す。Core結果は `USER_PREMISE / COMMIT`。
2. `MeaningIR` は同じsourceに対して `/Assert/proposition/subject`, `/predicate`, `/object`, `/scope`, `/negation` を記録する。それぞれの `start..end` を原文へ当てると `Nera`, `は`, `sensor`, `materials-lab`, `ではない` が一致する。
3. predicate粒子の候補範囲をsubject end〜object startへ絞り、否定語内の「は」をpredicate spanと誤記録しない。
4. Engineを閉じて同じDBへ再openし、同じinput ID/versionを実行すると保存ResultRecordとCaseRevisionが完全一致。B17 arithmetic routeもv74下で `/Calculate/expression` → 全角原文 `６＋７` のspanを保存して再生する。
5. 旧`MeaningIR`にsource_roles fieldがないJSONでもcanonical byte列を保持する。

検査・hash・完全なsource-role map・原本manifest checkは [B18 input-role validation](../../bb-input-machine-observation-02-v74-validation.json) に保存。

## 検証と保証範囲

- B18 relation source-role統合観測とB17 arithmetic replay route: PASS、各1件。
- `cargo test -p minidora-engine --locked`: PASS、235 unit tests、9 bounded integration tests、doc tests 0件。正確なrange testと旧canonical compatibility testを含む。
- `cargo fmt --all -- --check`、workspace all-target check、warnings-denied Clippy、locked workspace build: PASS。
- `cargo test --workspace --locked`: 未実行。source-role mappingは一部Intent familyだけであり、roleを抽出できない意味値もある。すべての引用・時間・人物・権限・document variantの専用role span、A04/A15/E08全体、全93要件acceptanceは未確立で`UNVERIFIED`。releaseなし。

## 次工程

第六命令の未対応Intentを棚卸しし、時間・数量・人物・引用・source/document・permissionに対する元入力と版・中間値の対応を必要に応じて段階的に追加する。曖昧な意味をrole spanの存在だけで確定しない。
