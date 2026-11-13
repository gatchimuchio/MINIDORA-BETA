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

The natural `Xについて、何が分かっていますか` inventory lists active direct relations and conclusions supported by retained rules. It keeps their guarantee labels and proof dependencies, marks contradictory support without resolving it, and discloses that the saved relation list is non-exhaustive and not an outside-world fact check. The derived-inventory follow-up is implemented but still awaits a new normal-route observation.

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

After a common-relation answer, `詳しく説明して` now consumes a canonical, case-bound typed report artifact tied to the exact prior result. It rechecks each stored relation separately for both subjects against current actor memory, confirms each side's guarantee and proof dependencies, then returns a distinct explanation with the left and right proof lineage. Missing or changed evidence suspends instead of replaying the old report. The list remains non-exhaustive and does not establish outside-world facts. See [the common-relation explanation route evidence](audit/bb-context-common-relation-explanation-v68-route-validation.json); this bounded follow-up does not establish contextual explanation acceptance.

After a committed copular relation answer, `詳しく説明して` rechecks the same conclusion against current memory and reports its direct premise, derived fact and applicable rule IDs. If the proof has changed, Core suspends with `CONTEXT_PROOF_CHANGED` instead of explaining stale support.

The bounded `なぜ` / `どうして` and `Why ...?` forms ask for an explanation of one explicit relation. Core queries the actor's current premises and rules and lists the supporting premise, derived fact and rule IDs. A supplied negative premise is explained as support for the opposite relation; missing or conflicting evidence suspends with a typed reason. The Japanese and English rules in these examples are separate declared premises. This is proof explanation over caller-supplied records, not general causal understanding or a claim about outside-world truth. The scoped ordinary-CLI, adopted-file recovery and no-resend observation is recorded in [the why-question route evidence](audit/bb-lang-why-question-route-validation.json); full BB-LANG/A13 acceptance remains `UNVERIFIED`.

Elliptical follow-ups such as `なぜですか`, `どうして？` and `Why?` now reuse the same `ExplainPrevious` consumer. For a relation answer it rechecks the current proof and its dependencies; for a calculation it explains the retained operation. It uses only the immediately preceding committed result for the same actor and case. Missing prior output suspends, and another actor cannot read the case. This resolves a bounded previous-result question, not an open-ended causal explanation. See [the contextual why route evidence](audit/bb-lang-contextual-why-v66-route-validation.json).

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

Selection uses the exact decoded named header and returns a JSON array of **strings**; `001` and huge numeric-looking fields are not converted to numbers. Numeric CSV statistics distinguish unitless exact expressions from explicitly unit-bearing decimal values. Complete scientific tokens and expressions such as `1e3`, `1e3/2` and `1e3-2e3` retain their numeric meaning; the quantity parser cannot shorten an exponent to invent a unit. Existing exponent and exact-value bounds still apply. Use whitespace for an intentionally overlapping unit label, for example `1 e3`; ordinary suffixes such as `1e3m` remain quantities. `json-table-select` returns every value in the exact named string column with source row indices and spans. `json-table-project` emits a string table containing only the requested unique columns, in the requested order, and retains the header and cell source spans. `csv-filter` keeps rows only when the named cell exactly equals the supplied string and returns the same string-table schema; it does not trim, normalize case or infer types. `json-table-filter` applies the same exact string equality to a strict `{"columns":[...],"rows":[...]}` JSON table, returns selected rows and source spans for every decision and emitted cell. `csv-sort` and `json-table-sort` return rows in stable exact-string order, with ties retaining their input order; they do not use locale collation, case folding, Unicode normalization or numeric interpretation. `csv-count-by` and `json-table-count-by` group exact decoded strings in first-seen order, return integer counts, and retain source locations for group members. `csv-compare-by` matches rows by unique exact-string key, requires identical decoded headers in the same order, and reports changed cells plus added and removed rows with source spans from both inputs. Comparison changes retain left row/column order followed by right-side additions. Its reserved source format is `--- MINIDORA LEFT CSV ---\n<left CSV>\r\n--- MINIDORA RIGHT CSV ---\n<right CSV>` after the operation header; marker text cannot occur in either CSV payload. These operations record original byte spans; filter evidence includes every predicate cell, including excluded rows. Quote escapes, empty cells and embedded quoted line breaks are preserved. Bare LF record separators, inconsistent widths and duplicate decoded header names are rejected. CSV-to-JSON conversion requires an explicit `header` or `no-header` mode (Japanese headers are `CSVをJSONに変換して（先頭行をヘッダーとして）:` and `CSVをJSONに変換して（ヘッダーなし）:`). It returns the string schema `{"columns":[...],"rows":[...]}` or `"columns":null` when headerless. JSON-to-CSV accepts exactly this string-table schema, preserves source spans in its artifact, and rejects duplicate/unknown fields, invalid widths, nonstring cells and formula-leading cells rather than silently sanitizing or executing them. Japanese JSON-to-CSV input uses `JSONをCSVに変換して:`. Headerless/indexed CSV column selection remains library-only in this alpha.

`csv-inner-join "id" "record_id":` or `CSVを「id」と「record_id」で内部結合して:` uses the reserved two-table CSV markers above. The two named keys must be unique in their own headered tables; exact matches produce JSON string-table rows in left-table order with `left.` and `right.` namespaced columns. Both keys remain in the output, unmatched keys are listed separately, and emitted header/cell values map to original byte ranges in the combined input. No type inference or one-to-many expansion is applied.

Named-column statistics explicitly interpret every selected cell as either a unitless exact arithmetic expression or as one decimal numeric literal followed by a unit. Mixed unit-bearing and unitless/non-numeric cells suspend. The exact statistics report identifies the source digest, decoded column, observation count and original header/cell byte spans; mixed `mm`, `cm`, `m` and `km` values normalize exactly to metres. These are descriptive statistics for supplied cells and do not imply a representative population.

After a committed `CSV_STATISTICS` result, `詳しく説明して` reloads its same-case `DocumentResult`, recomputes the exact report from the retained CSV, and checks source/output hashes and source-cell spans before explaining the recorded statistics and up to eight original cells. The follow-up carries both the prior result and report artifact references. An exact granted `--output` path can save the explanation; `reconcile` replays the same source-bound explanation and confirms the existing file without resending the write. This route covers CSV statistics only; grouped statistics and other CSV/JSON operations retain their existing results but do not use this explanation consumer yet. See [the CSV statistics context-route evidence](audit/bb-math-data-csv-statistics-context-followup-route-validation.json).

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
"$minidora_bin" --db "$minidora_state" --actor demo --case json-table-join \
  $'json-table-inner-join "id" "record_id":\n{"left":{"columns":["id","name"],"rows":[["2","dog"],["1","cat"],["4","fox"]]},"right":{"columns":["record_id","kind"],"rows":[["1","feline"],["3","bird"],["2","canine"]]}}'
```

A network read requires both the exact application hostname grant and the environment's outbound policy. Redirect destinations require their own grants. The current port accepts bounded HTTPS:443 UTF-8 `text/plain`, `text/markdown` and `application/json`, preserving raw bytes, hashes, actual URI/status/failure and provider time. It uses inherited proxies and TLS trust. Private-address and malformed URL paths are rejected. An observed source report is not a verified factual conclusion.

This direct body port does not implement HTML/PDF extraction, arbitrary source comparison or open-domain summarization. Provider metadata search is a separate explicit operation described below. A URL plus additional unsupported instructions suspends instead of quietly dropping the extra task. Fixed packets and LIVE observations remain distinct evidence modes.

The JSON Pointer operation validates the complete document, rejects duplicate keys and invalid pointer syntax, and returns the exact selected source slice. Its input body retains the original whitespace and numeric lexemes, and the source-preserving JSON parser performs the complete validation without a preliminary floating-point JSON conversion. A raw number such as `1e400` can be selected without evaluating its magnitude. Invalid document bodies suspend during the document operation; invalid pointer prefixes remain input failures. `json-object-project ["metrics","name"]:` or `JSONオブジェクトからフィールド["metrics","name"]を抽出して:` selects a nonempty unique list of root-object fields and returns them in that order. Each selected value's original JSON bytes are preserved (including numeric lexemes), while the source-map artifact retains original key/value ranges, output ranges and JSON Pointers. Missing or repeated names, non-object roots, malformed JSON and limit overflow suspend. `json-statistics /scores` or `JSONの配列「/scores」の統計量を計算して:` explicitly selects a JSON array containing only number literals and computes exact descriptive statistics. The report retains the complete source digest, selected array bytes, each numeric lexeme and its byte range. Empty, mixed-type, nested, oversized or non-array selections suspend. No population inference is made.

`json-compare:` or `JSONを比較して:` accepts one JSON wrapper with exactly `left` and `right` values. It rejects duplicate keys, ignores object key order, compares array order and decoded strings exactly, and treats numerically equal JSON lexemes such as `1` and `1.0` as equal using bounded exact decimals. The difference report uses JSON Pointers and source byte ranges/digests for each side; absent fields are distinct from explicit `null`. Inputs, numeric magnitude, difference count and serialized output are bounded. This is a structural comparison and does not infer factual truth or general semantic equivalence.

`json-table-compare "id":` or `JSON表を列「id」で比較して:` accepts the same `left`/`right` wrapper with string-table objects (`columns` and `rows`). Both decoded column lists must match in the same order, and the selected key column must be unique on each side. The report lists added/removed rows and changed non-key cells in source order, with separate JSON Pointers, byte ranges and digests for each side. Keys and cells are exact strings; no trimming, normalization or numeric inference is applied.

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

See [the historical alpha 7 validation record](audit/validation.json), [the B02 unit-arithmetic validation](audit/unit-arithmetic-validation.json), [the unit-bearing sequence build record](audit/unit-sequence-validation.json), [the initial operand-correction validation](audit/unit-correction-validation.json), [the bounded operator-correction validation](audit/unit-binary-correction-validation.json), [the store migration validation](audit/schema-migration-validation.json), [the CSV statistics build record](audit/csv-statistics-validation.json), [the JSON statistics build record](audit/json-statistics-validation.json), [the JSON comparison build record](audit/json-comparison-validation.json), [the JSON table comparison build record](audit/json-table-comparison-validation.json), [the JSON table inner-join build record](audit/json-table-inner-join-validation.json), [the CSV table inner-join build record](audit/csv-inner-join-validation.json), [the explicit evidence synthesis build record](audit/explicit-evidence-synthesis-validation.json), [the exact probability-weight normalization build record](audit/probability-normalization-validation.json), [the finite-distribution moments build record](audit/finite-probability-statistics-validation.json), [the finite-event probability build record](audit/finite-event-probability-validation.json), [the polynomial-evaluation build record](audit/polynomial-evaluation-validation.json), [the linear-equation build record](audit/linear-equation-validation.json), [the quadratic-equation build record](audit/quadratic-equation-validation.json), [the structured table-conversion build record](audit/structured-table-conversion-validation.json), [the CSV filter build record](audit/csv-filter-validation.json), [the CSV sort build record](audit/csv-sort-validation.json), [the CSV group-count build record](audit/csv-group-count-validation.json), [the CSV grouped-statistics build record](audit/csv-group-statistics-validation.json), [the CSV table-comparison build record](audit/csv-table-comparison-validation.json), [the JSON table-filter build record](audit/json-table-filter-validation.json), [the JSON table-sort build record](audit/json-table-sort-validation.json), [the JSON table-group-count build record](audit/json-table-group-count-validation.json), [the JSON table grouped-statistics build record](audit/json-table-group-statistics-validation.json), [the JSON table-select build record](audit/json-table-select-validation.json), [the JSON table-projection build record](audit/json-table-project-validation.json), [the JSON object-projection build record](audit/json-object-project-validation.json), [calculation-sequence build record](audit/calculation-sequence-validation.json), [bounded derived-relation inventory route](audit/bb-lang-derived-relation-inventory-20261113-route-validation.json), [all93 requirement observations](audit/rust-requirement-observations.json) and [implementation status](audit/IMPLEMENTATION_STATUS.md). The historical alpha 7 run passed 134 Rust tests and 8 separately labeled local TLS fixture cases; the preserved predecessor passed 313 selected tests separately. Later alpha 8 Linux runs passed 147 and 148 Rust tests in their separately identified snapshots. These scoped observations do not establish full R2/93-capability successor acceptance. Unrestricted natural-language work, the complete inherited semantic/regression audit, LIVE benchmark and physical Windows/8GB validation remain incomplete. No missing item is an approved release-scope exclusion.

Tested cloud reproduction and startup instructions are [cloud-install.sh](tools/cloud-install.sh) and [cloud-start.md](tools/cloud-start.md). Backend environment-draft saving is currently unconfirmed due to an API error; the scripts themselves were executed successfully.
