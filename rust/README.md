# MINIDORA Rust successor alpha

`0.7.0-alpha.3` is an independently versioned Rust development implementation of the public input → Core → output execution route. It contains working mathematical, declared-relation, bounded Japanese meaning-comparison, durable-state, source-reading and exact-file-output mechanisms. It is **not an accepted implementation of all 93 capability obligations**. The [implementation status](audit/IMPLEMENTATION_STATUS.md) and [full obligations](audit/implementation-obligations.md) keep missing evidence and functionality visible; they do not reduce the requested scope.

The frozen Python `minidora-notnn==0.6.0b1`, its public Japanese identifiers, release records and original distribution hashes remain unchanged. This workspace lives entirely under `rust/`. Its records use the separate schema `minidora-rust/0.7.0-alpha.3`; they are not a silent migration of the beta's saved data or duplicate-input guarantees.

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
"$minidora_bin" inspect --db "$minidora_state" --actor demo --case arithmetic
```

Arithmetic uses exact integers/rationals, bounded expressions and recorded steps. A repeated explicit `--case`, `--input` and `--version` with the same complete request retrieves its saved result; changing the request under that identity is a conflict. Omit `--input` for the CLI to choose an identity from the next case revision. This successor behavior is separate from the frozen beta facade's in-instance duplicate rejection.

## Statistics, linear systems and symbolic polynomials

```bash
"$minidora_bin" --db "$minidora_state" --actor demo --case statistics \
  '統計 [0.1,0.2,0.3,0.4] 単位 m'
"$minidora_bin" --db "$minidora_state" --actor demo --case linear \
  '連立方程式 [[2,1],[1,-1]] = [7,-1]'
"$minidora_bin" --db "$minidora_state" --actor demo --case polynomial \
  '簡約 (x+1)*(x-1)'
"$minidora_bin" --db "$minidora_state" --actor demo --case polynomial \
  '微分 3*x**3+x*y-0.5*x+7 変数 x'
"$minidora_bin" --db "$minidora_state" --actor demo --case polynomial \
  '積分 x**2+y/2 変数 x'
```

Statistics retain supplied values and compute exact sum, mean, median, extrema and population/sample variance. Units must match explicitly; they are not silently converted. One observation has no sample variance. The values do not establish a representative real-world population.

Linear systems use coefficient-column order rather than inferred variable names. Results distinguish a unique solution, a parameterized solution space and inconsistent equations, with original equations, row transformations and verified substitution/contradiction certificates. Rectangular systems, dimensions, scalar sizes and operations have explicit limits.

Symbolic operations cover bounded rational-coefficient multivariate polynomials. Multiplication must be explicit. `**` and the documented Rust alias `^` take nonnegative literal integer exponents. Variable denominators, functions, implicit multiplication and trailing unparsed constraints fail. An integral is one representative antiderivative; an arbitrary term independent of its target variable remains possible. This is not a general computer algebra system or free-form mathematical-language claim.

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
"$minidora_bin" --db "$minidora_state" --actor demo --case edit \
  $'置換「猫」→「犬」 件数 2:\n猫の猫'
```

English equivalents are `csv-select price` followed by a newline and CSV, or `replace "old" -> "new" count 1:` followed by a newline and the exact source body. A header containing spaces can be selected with a complete quoted JSON string, for example `csv-select "unit price"`.

Selection uses the exact decoded named header and returns a JSON array of **strings**; `001` and huge numeric-looking fields are not converted to numbers. Quote escapes, empty cells, embedded quoted line breaks and source byte spans are retained. Bare LF record separators, inconsistent widths and duplicate decoded header names are rejected. The library additionally has explicit headerless/indexed selection and string-table CSV↔JSON functions; those extra library functions do not have an ordinary CLI intent in this alpha. CSV export rejects formula-leading cells rather than silently sanitizing or executing them.

Literal editing replaces original nonoverlapping matches only. A supplied count must match before any result is formed; omitting the count means all such matches. Unmatched text stays byte-for-byte unchanged; inserted text is not rescanned. Missing search text, a no-op, an empty search, a count mismatch or resource overflow stops. The original document/hash, exact parameters and source/output mappings are stored separately from the emitted result. This is not arbitrary editing-intent inference, unrestricted writing quality or a file write without the separate exact-target grant.

## Exact-target file output

```bash
minidora_report=/workspace/.minidora-cloud/rust-result.txt
"$minidora_bin" --db "$minidora_state" --actor demo --case file \
  --allow-write "$minidora_report" --output "$minidora_report" '17*23'
```

`--output` requests a file; **only** `--allow-write` grants that exact target. Core records the action identity and authorization before the physical writer runs, then checks actual bytes and its receipt before goal `COMMIT`. Parent directories must already exist and be trusted. Different existing bytes are not overwritten; a matching existing artifact can be verified. Symlinks, traversal and unconfirmed write/durability outcomes have explicit failure/unknown paths. The portable path checks are not an OS sandbox against hostile concurrent ancestor replacement.

## Inspect and reconcile existing effects

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
```

A network read requires both the exact application hostname grant and the environment's outbound policy. Redirect destinations require their own grants. The current port accepts bounded HTTPS:443 UTF-8 `text/plain`, `text/markdown` and `application/json`, preserving raw bytes, hashes, actual URI/status/failure and provider time. It uses inherited proxies and TLS trust. Private-address and malformed URL paths are rejected. An observed source report is not a verified factual conclusion.

This direct body port does not implement HTML/PDF extraction, arbitrary source comparison or open-domain summarization. Provider metadata search is a separate explicit operation described below. A URL plus additional unsupported instructions suspends instead of quietly dropping the extra task. Fixed packets and LIVE observations remain distinct evidence modes.

The JSON Pointer operation validates the complete document, rejects duplicate keys and invalid pointer syntax, and returns the exact selected source slice. It preserves numeric lexemes rather than converting large values through floating point. It is one structured-document operation, not the whole table/CSV/document capability population.

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

See [the validation record](audit/validation.json), [all93 requirement observations](audit/rust-requirement-observations.json) and [implementation status](audit/IMPLEMENTATION_STATUS.md). The current Linux development run passed123 Rust tests and8 separately labeled local TLS fixture cases; the preserved predecessor passed313 selected tests separately. Full R2/93-capability successor acceptance, unrestricted natural-language work, the complete inherited semantic/regression audit, LIVE benchmark and physical Windows/8GB validation remain incomplete. No missing item is an approved release-scope exclusion.

Tested cloud reproduction and startup instructions are [cloud-install.sh](tools/cloud-install.sh) and [cloud-start.md](tools/cloud-start.md). Backend environment-draft saving is currently unconfirmed due to an API error; the scripts themselves were executed successfully.
