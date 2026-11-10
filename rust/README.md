# MINIDORA Rust successor alpha

`0.7.0-alpha.8` is an independently versioned Rust development implementation of the public input → Core → output execution route. It contains working mathematical, declared-relation, bounded Japanese meaning-comparison, durable-state, source-reading and exact-file-output mechanisms. It is **not an accepted implementation of all 93 capability obligations**. The [implementation status](audit/IMPLEMENTATION_STATUS.md) and [full obligations](audit/implementation-obligations.md) keep missing evidence and functionality visible; they do not reduce the requested scope.

The frozen Python `minidora-notnn==0.6.0b1`, its public Japanese identifiers, release records and original distribution hashes remain unchanged. This workspace lives entirely under `rust/`. Its records use the separate schema `minidora-rust/0.7.0-alpha.8`; they are not a silent migration of the beta's saved data or duplicate-input guarantees. The alpha 8 schema is incompatible with alpha 7 Rust store metadata: ordinary opens of an alpha 7 database return `SCHEMA_MISMATCH` and leave its files unchanged. A separate explicit `migrate` command is available for alpha 7 Rust stores.

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
cargo fmt --all -- --check
cargo check --workspace --all-targets --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
cargo build --workspace --locked

minidora_bin="$CARGO_TARGET_DIR/debug/minidora-rust"
minidora_state=/workspace/.minidora-cloud/rust-demo-state
mkdir -p /workspace/.minidora-cloud
"$minidora_bin" --help
```

`Cargo.lock` fixes the dependency resolution. If the toolchain is absent, provision the official Rust toolchain with normal certificate/checksum verification before running these commands. Keep the environment's proxy and certificate trust. No API key or external provider is needed for the local arithmetic, relations, mathematical functions or file-output examples.

These commands are the reproducible verification procedure. The source alone does not establish that every command or end-to-end scenario passed; current-run evidence must identify the exact implementation revision, command and result.

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

Arithmetic uses exact integers/rationals, bounded expressions and recorded steps. Binary addition/subtraction preserves identical explicit units and converts mixed `mm`, `cm`, `m` and `km` exactly to metres. An explicit first/second operand correction creates the next input version, links it to the actual prior result and retains that original result. Corrections cover saved binary literals using `+`, `-`, `*`, `/`, `%`, `^` or `**`; unit-bearing correction remains limited to addition/subtraction, and nested or compound expressions suspend. A repeated explicit `--case`, `--input` and `--version` with the same complete request retrieves its saved result; changing the request under that identity is a conflict. Omit `--input` for the CLI to choose an identity from the next case revision. This successor behavior is separate from the frozen beta facade's in-instance duplicate rejection.

### Bounded calculation sequences

```bash
"$minidora_bin" --db "$minidora_state" --actor demo --case chained-calculation \
  $'計算手順を実行して:\nbase: 2+3\ndouble: @{base}*2\nfinal: @{double}+1'
"$minidora_bin" --db "$minidora_state" --actor demo --case chained-units \
  $'計算手順を実行して:\nbase: 1 km + 500 m\nfinal: @{base} + 500 m'
```

Each line declares one unique step ID and exact arithmetic expression. `@{earlier-id}` inserts the earlier step's exact rational result, with parentheses preserving scalar expression grouping. Exact unit-bearing results can also flow into the existing bounded addition/subtraction grammar, including mixed `mm`, `cm`, `m` and `km` conversion; dimensional multiplication or division is not added by this route. The Core adds each step to its finite required-artifact graph, stores its source expression and evaluated result, and records the upstream artifact SHA-256 actually consumed by each dependent step. Only earlier declared results can be referenced; forward or unknown references remain unresolved. At most 16 steps are admitted, and the caller's action budget must also cover every required graph node. This is a bounded calculation-chain route, not general multi-step reasoning acceptance.

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
"$minidora_bin" --db "$minidora_state" --actor demo --case negative 'ミナは鳥ではない'
"$minidora_bin" --db "$minidora_state" --actor demo --case negative 'ミナは鳥ですか'
```

These are scoped user premises and formal consequences, not independently verified facts about the world. Persistent premises/rules are actor-scoped across cases; cases identify result lineages, not separate knowledge databases. Use distinct explicit scopes or actors for isolated premise worlds. The negative example uses a different subject to avoid contradicting the earlier positive example. The supported copular grammar preserves polarity and optional `[scope=story]` labels. Rule variables use `X`, `Y`, `Z` or explicit `$` names. Proof-preserving rule composition, dependencies and invalidation belong to Core. Unknown grammar, unresolved contradictions and missing premises produce typed stops. General Japanese understanding, unrestricted quantified logic and induction from arbitrary examples are not established.

A stored fact/rule ID can be withdrawn using `invalidate ID` or `IDを無効化して`; inspect the returned JSON or saved case to obtain its actual ID. Original observations remain retained. This does not establish all required generalized-learning or long-history resource gates.

## CSV selection and explicit text editing

The first line names a supported operation; the body after its newline is retained exactly. CSV records use CRLF; the header/body separator may use a single LF. Bash ANSI quoting below makes those bytes explicit.

```bash
"$minidora_bin" --db "$minidora_state" --actor demo --case csv \
  $'CSVの列「price」を選択して:\nname,price\r\n猫,001\r\n犬,9007199254740993\r\n'
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
"$minidora_bin" --db "$minidora_state" --actor demo --case json-table-select \
  $'JSON表の列「status」を選択して:\n{"columns":["id","status"],"rows":[["1","open"],["2","closed"],["3","open"]]}'
"$minidora_bin" --db "$minidora_state" --actor demo --case json-table-project \
  $'JSON表から列["name","id"]を抽出して:\n{"columns":["id","name","status"],"rows":[["1","cat","open"],["2","bird","closed"]]}'
"$minidora_bin" --db "$minidora_state" --actor demo --case csv-compare \
  $'CSVを列「id」で2つの表を比較して:\n--- MINIDORA LEFT CSV ---\nid,status\r\n1,open\r\n2,closed\r\n--- MINIDORA RIGHT CSV ---\nid,status\r\n1,closed\r\n3,open\r\n'
"$minidora_bin" --db "$minidora_state" --actor demo --case csv-to-json \
  $'csv-to-json header:\nname,price\r\ncat,001\r\ndog,9007199254740993\r\n'
"$minidora_bin" --db "$minidora_state" --actor demo --case json-to-csv \
  $'json-to-csv:\n{"columns":["name","price"],"rows":[["cat","001"],["dog","9007199254740993"]]}'
"$minidora_bin" --db "$minidora_state" --actor demo --case length-stats \
  $'CSVの列「length」の統計量を計算して:\nplace,length\r\nA,1 km\r\nB,500 m\r\n'
"$minidora_bin" --db "$minidora_state" --actor demo --case edit \
  $'置換「猫」→「犬」 件数 2:\n猫の猫'
```

English equivalents are `csv-select price`, `csv-statistics price`, `csv-filter "status" equals "open"`, `csv-sort "name" ascending`, `csv-count-by "status"`, `csv-group-statistics "region" "sales"`, `csv-compare-by "id"`, `json-table-sort "name" ascending`, `json-table-count-by "status"`, `json-table-select "status"` or `json-table-project ["name","id"]`, followed by a newline and CSV or the exact JSON table body (CSV comparison uses the two reserved marker lines shown below). `json-table-filter "status" equals "open"` filters JSON string tables. Descending sort uses `descending`. A header containing spaces can be selected with a complete quoted JSON string, for example `csv-select "unit price"`, `csv-statistics "unit price"`, `csv-filter "order status" equals "open"`, `csv-sort "display name" descending`, `csv-count-by "order status"`, `csv-group-statistics "sales region" "unit price"`, `csv-compare-by "record id"`, `json-table-sort "display name" descending`, `json-table-count-by "order status"`, `json-table-select "order status"` or `json-table-project ["display name","record id"]`.

Selection uses the exact decoded named header and returns a JSON array of **strings**; `001` and huge numeric-looking fields are not converted to numbers. `json-table-select` returns every value in the exact named string column with source row indices and spans. `json-table-project` emits a string table containing only the requested unique columns, in the requested order, and retains the header and cell source spans. `csv-filter` keeps rows only when the named cell exactly equals the supplied string and returns the same string-table schema; it does not trim, normalize case or infer types. `json-table-filter` applies the same exact string equality to a strict `{"columns":[...],"rows":[...]}` JSON table, returns selected rows and source spans for every decision and emitted cell. `csv-sort` and `json-table-sort` return rows in stable exact-string order, with ties retaining their input order; they do not use locale collation, case folding, Unicode normalization or numeric interpretation. `csv-count-by` and `json-table-count-by` group exact decoded strings in first-seen order, return integer counts, and retain source locations for group members. `csv-compare-by` matches rows by unique exact-string key, requires identical decoded headers in the same order, and reports changed cells plus added and removed rows with source spans from both inputs. Comparison changes retain left row/column order followed by right-side additions. Its reserved source format is `--- MINIDORA LEFT CSV ---\n<left CSV>\r\n--- MINIDORA RIGHT CSV ---\n<right CSV>` after the operation header; marker text cannot occur in either CSV payload. These operations record original byte spans; filter evidence includes every predicate cell, including excluded rows. Quote escapes, empty cells and embedded quoted line breaks are preserved. Bare LF record separators, inconsistent widths and duplicate decoded header names are rejected. CSV-to-JSON conversion requires an explicit `header` or `no-header` mode (Japanese headers are `CSVをJSONに変換して（先頭行をヘッダーとして）:` and `CSVをJSONに変換して（ヘッダーなし）:`). It returns the string schema `{"columns":[...],"rows":[...]}` or `"columns":null` when headerless. JSON-to-CSV accepts exactly this string-table schema, preserves source spans in its artifact, and rejects duplicate/unknown fields, invalid widths, nonstring cells and formula-leading cells rather than silently sanitizing or executing them. Japanese JSON-to-CSV input uses `JSONをCSVに変換して:`. Headerless/indexed CSV column selection remains library-only in this alpha.

Named-column statistics explicitly interpret every selected cell as either a unitless exact arithmetic expression or as one decimal numeric literal followed by a unit. Mixed unit-bearing and unitless/non-numeric cells suspend. The exact statistics report identifies the source digest, decoded column, observation count and original header/cell byte spans; mixed `mm`, `cm`, `m` and `km` values normalize exactly to metres. These are descriptive statistics for supplied cells and do not imply a representative population.

Grouped CSV statistics use an exact decoded string key in first-seen order and compute exact descriptive statistics for a distinct value column. Each group must contain unitless exact expressions or unit-bearing decimal literals consistently; mixed length units convert exactly through the existing `mm`, `cm`, `m` and `km` rules. The report retains source record numbers plus group-key and measured-cell byte spans. It describes the supplied rows and does not infer a wider population.

Literal editing replaces original nonoverlapping matches only. A supplied count must match before any result is formed; omitting the count means all such matches. Unmatched text stays byte-for-byte unchanged; inserted text is not rescanned. Missing search text, a no-op, an empty search, a count mismatch or resource overflow stops. The original document/hash, exact parameters and source/output mappings are stored separately from the emitted result. This is not arbitrary editing-intent inference, unrestricted writing quality or a file write without the separate exact-target grant.

## Exact-target file output

```bash
minidora_report=/workspace/.minidora-cloud/rust-result.txt
"$minidora_bin" --db "$minidora_state" --actor demo --case file \
  --allow-write "$minidora_report" --output "$minidora_report" '17*23'
```

`--output` requests a file; **only** `--allow-write` grants that exact target. Core records the action identity and authorization before the physical writer runs, then checks actual bytes and its receipt before goal `COMMIT`. Parent directories must already exist and be trusted. Different existing bytes are not overwritten; a matching existing artifact can be verified. Symlinks, traversal and unconfirmed write/durability outcomes have explicit failure/unknown paths. The portable path checks are not an OS sandbox against hostile concurrent ancestor replacement.

## Inspect and reconcile existing effects

### Explicit alpha 7 store migration

Ordinary alpha 8 opens continue to reject `minidora-rust/0.7.0-alpha.7` metadata. To migrate an existing Rust store, run the dedicated command with its exact state directory:

```bash
"$minidora_bin" migrate --db /path/to/alpha7-state --json
```

Migration requires `--db`, accepts only alpha 7 metadata, and verifies the existing SQLite profile, foreign keys, referenced blob bytes, case revision chains, action journals and input-conflict chains before changing state. It then updates the single `store_metadata.schema` value. Case/effect rows and digest-addressed blob bytes remain intact; old case revisions retain their alpha 7 schema identity and remain readable. A valid alpha 8 store returns `"migrated":false` after verification. Unsupported versions, missing state files, a non-WAL store or any integrity failure stop before the metadata change. `MIGRATION_DURABILITY_UNKNOWN` means the commit or its final directory sync could not be confirmed; rerunning the command verifies the store and reports whether migration already completed. The frozen Python beta has a separate store and is not migrated by this command.

```bash
"$minidora_bin" inspect --db "$minidora_state" --actor demo --case file
"$minidora_bin" reconcile --db "$minidora_state" --actor demo --case file --json
```

`reconcile` inspects the saved original local-file operation, authority, adopted artifact and actual existing target. It accepts no new request, permissions or output path, performs no write/resend and creates no new action token. An already committed case returns its saved result. An eligible adopted/suspended file case can reach `COMMIT` only after the original content, proof, graph and actual file confirmation are verified. Unsupported or unconfirmed recovery remains suspended.

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
```

A network read requires both the exact application hostname grant and the environment's outbound policy. Redirect destinations require their own grants. The current port accepts bounded HTTPS:443 UTF-8 `text/plain`, `text/markdown` and `application/json`, preserving raw bytes, hashes, actual URI/status/failure and provider time. It uses inherited proxies and TLS trust. Private-address and malformed URL paths are rejected. An observed source report is not a verified factual conclusion.

This direct body port does not implement HTML/PDF extraction, arbitrary source comparison or open-domain summarization. Provider metadata search is a separate explicit operation described below. A URL plus additional unsupported instructions suspends instead of quietly dropping the extra task. Fixed packets and LIVE observations remain distinct evidence modes.

The JSON Pointer operation validates the complete document, rejects duplicate keys and invalid pointer syntax, and returns the exact selected source slice. `json-object-project ["metrics","name"]:` or `JSONオブジェクトからフィールド["metrics","name"]を抽出して:` selects a nonempty unique list of root-object fields and returns them in that order. Each selected value's original JSON bytes are preserved (including numeric lexemes), while the source-map artifact retains original key/value ranges, output ranges and JSON Pointers. Missing or repeated names, non-object roots, malformed JSON and limit overflow suspend. `json-statistics /scores` or `JSONの配列「/scores」の統計量を計算して:` explicitly selects a JSON array containing only number literals and computes exact descriptive statistics. The report retains the complete source digest, selected array bytes, each numeric lexeme and its byte range. Empty, mixed-type, nested, oversized or non-array selections suspend. No population inference is made.

`json-compare:` or `JSONを比較して:` accepts one JSON wrapper with exactly `left` and `right` values. It rejects duplicate keys, ignores object key order, compares array order and decoded strings exactly, and treats numerically equal JSON lexemes such as `1` and `1.0` as equal using bounded exact decimals. The difference report uses JSON Pointers and source byte ranges/digests for each side; absent fields are distinct from explicit `null`. Inputs, numeric magnitude, difference count and serialized output are bounded. This is a structural comparison and does not infer factual truth or general semantic equivalence.

`json-table-compare "id":` or `JSON表を列「id」で比較して:` accepts the same `left`/`right` wrapper with string-table objects (`columns` and `rows`). Both decoded column lists must match in the same order, and the selected key column must be unique on each side. The report lists added/removed rows and changed non-key cells in source order, with separate JSON Pointers, byte ranges and digests for each side. Keys and cells are exact strings; no trimming, normalization or numeric inference is applied.

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

See [the historical alpha 7 validation record](audit/validation.json), [the B02 unit-arithmetic validation](audit/unit-arithmetic-validation.json), [the unit-bearing sequence build record](audit/unit-sequence-validation.json), [the initial operand-correction validation](audit/unit-correction-validation.json), [the bounded operator-correction validation](audit/unit-binary-correction-validation.json), [the store migration validation](audit/schema-migration-validation.json), [the CSV statistics build record](audit/csv-statistics-validation.json), [the JSON statistics build record](audit/json-statistics-validation.json), [the JSON comparison build record](audit/json-comparison-validation.json), [the JSON table comparison build record](audit/json-table-comparison-validation.json), [the explicit evidence synthesis build record](audit/explicit-evidence-synthesis-validation.json), [the exact probability-weight normalization build record](audit/probability-normalization-validation.json), [the finite-distribution moments build record](audit/finite-probability-statistics-validation.json), [the finite-event probability build record](audit/finite-event-probability-validation.json), [the polynomial-evaluation build record](audit/polynomial-evaluation-validation.json), [the structured table-conversion build record](audit/structured-table-conversion-validation.json), [the CSV filter build record](audit/csv-filter-validation.json), [the CSV sort build record](audit/csv-sort-validation.json), [the CSV group-count build record](audit/csv-group-count-validation.json), [the CSV grouped-statistics build record](audit/csv-group-statistics-validation.json), [the CSV table-comparison build record](audit/csv-table-comparison-validation.json), [the JSON table-filter build record](audit/json-table-filter-validation.json), [the JSON table-sort build record](audit/json-table-sort-validation.json), [the JSON table-group-count build record](audit/json-table-group-count-validation.json), [the JSON table-select build record](audit/json-table-select-validation.json), [the JSON table-projection build record](audit/json-table-project-validation.json), [the JSON object-projection build record](audit/json-object-project-validation.json), [calculation-sequence build record](audit/calculation-sequence-validation.json), [all93 requirement observations](audit/rust-requirement-observations.json) and [implementation status](audit/IMPLEMENTATION_STATUS.md). The historical alpha 7 run passed 134 Rust tests and 8 separately labeled local TLS fixture cases; the preserved predecessor passed 313 selected tests separately. Later alpha 8 Linux runs passed 147 and 148 Rust tests in their separately identified snapshots. These scoped observations do not establish full R2/93-capability successor acceptance. Unrestricted natural-language work, the complete inherited semantic/regression audit, LIVE benchmark and physical Windows/8GB validation remain incomplete. No missing item is an approved release-scope exclusion.

Tested cloud reproduction and startup instructions are [cloud-install.sh](tools/cloud-install.sh) and [cloud-start.md](tools/cloud-start.md). Backend environment-draft saving is currently unconfirmed due to an API error; the scripts themselves were executed successfully.
