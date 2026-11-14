# B30 — direct English Assertion source evidence

## Scope and permission

- Repository: `gatchimuchio/MINIDORA-BETA`, Rust successor.
- Base: `main` / `4a71d1bff750810193217797f49d9bb004b01e6c` (B29); work began on a clean tree with `origin/main` at the same commit.
- User authorization: continue implementing the retained three-document BUILD set and preserve work in this repository. Direct `main` commit/push and the existing B29/B28 recovery refs remain in scope.
- Bounded goal: preserve exact source attribution for the already-supported direct English copular Assertion from sealed input through Core Fact, Store reopen, relation proof and ordinary same-case Ask.
- Input: `[scope=biology]Coco is a bird` and `Mice are not mammals`.
- Stop boundary: no expansion to contextual assertions, general English grammar, truth verification, external network, output delivery, or release. Evidence spans establish what source text the user supplied, not whether its claim is true.

## Route and implementation

`RequestEnvelope` original text → parser v83 sealed `MeaningIR` source roles → `Core::Assert` validates a complete map against the reparsed proposition → direct `USER_PREMISE` Fact stores `SealedSourceEvidence` → Store revalidates the immutable input when Engine reopens → relation proof returns the same evidence → ordinary English Ask consumes the same Fact ID.

`input.rs` adds a direct-English copular source-role projector for `is`/`are`, optional lowercase `a`/`an`, object, explicit scope and optional `not`. The article gets its own role because the parser removes it from the semantic object. The Core evidence path only accepts the recomputed complete map for direct `Assert`; an incomplete or changed role map is not projected. Parser revision is `declarative-ja-en-v83-direct-english-assertion-source-ranges`, so new parser identity is versioned. No schema, dependency, lockfile, relation-proof or contextual-assertion behavior changed.

The Core observation verifies exact original UTF-8 ranges, an incomplete article map being rejected as evidence, Fact persistence and Store reopen for positive and negative premises, a `SUPPORTED` reopened proof, and an ordinary follow-up consuming the positive Fact. Both assertions remain user premises; no outside-world truth is inferred.

## Verification

| Command | Result |
|---|---|
| `cargo test -p minidora-engine --locked direct_english_assertion_source_roles_follow_the_fact_into_reopened_proofs -- --nocapture` | PASS: 1 focused test |
| `cargo test -p minidora-engine --locked` | PASS: 248 unit tests + 12 Cargo-launched integration cases = 260; 0 failed; 0 doc tests |
| `cargo fmt --all --check` | PASS |
| `cargo check --workspace --all-targets --locked` | PASS |
| `cargo clippy --workspace --all-targets --locked -- -D warnings` | PASS |
| `cargo build --workspace --locked` | PASS |
| `git diff --check` | PASS |

Environment: Linux `x86_64-unknown-linux-gnu`, `rustc 1.99.1`, `cargo 1.99.1`. Logs are external to Git under `/workspace/.minidora-cloud/b30-english-assertion-source/`. The complete route record and source-role ranges are in [B30 validation](../../bb-input-machine-observation-13-v83-direct-english-assertion-source-validation.json).

Full workspace all-features runtime, CLI/browser/UI/output delivery, other operating systems, independent QA, and full A04/A15/E08 or 93-requirement acceptance were not run. A04/A15/E08 and all 93 acceptance states remain `UNVERIFIED`; release state is `NOT_RELEASED`.

## Asset accounting

| Class | Count | Items |
|---|---:|---|
| retained | 6 paths | `input.rs`, `core.rs`, `requirements.tsv`, `IMPLEMENTATION_STATUS.md`, B30 validation JSON, this BUILD record |
| archived | 9 files | Initial focused log, final focused log, engine package test log, formatting, workspace check, Clippy, build, diff-check and environment logs under `/workspace/.minidora-cloud/b30-english-assertion-source/` |
| active | 3 directories | `/workspace/.cargo` (252M), `/workspace/.rustup` (617M), `rust/target` (6.5G) |
| trash | 0 | No pre-existing cache, user source, repository history or evidence deleted |
| pending | 0 | All files created or used for this batch are classified above |

Total: 18 classified items. The isolated SQLite state created by the Core observation was removed after the passing test. The original three-document archive and extracted source documents are unchanged.

## Changed paths

- `rust/crates/minidora-engine/src/input.rs`
- `rust/crates/minidora-engine/src/core.rs`
- `rust/audit/requirements.tsv`
- `rust/audit/IMPLEMENTATION_STATUS.md`
- `rust/audit/bb-input-machine-observation-13-v83-direct-english-assertion-source-validation.json`
- `rust/audit/construction/MB-BUILD-20261114/BUILD_BATCH_30.md`
