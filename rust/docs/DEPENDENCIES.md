# Dependencies and reproducibility

The workspace pins **Rust 1.99.1** in `rust-toolchain.toml`, selects edition 2024, and commits one `Cargo.lock`. Manifest version ranges describe compatible dependency families; the lock records the concrete resolution. Build and verification commands use `--locked`; signature/checksum and TLS verification must remain enabled.

The final generated package/license inventory is written to `rust/audit/dependencies.cdx.json` by the primary validation task. It is derived from `cargo metadata --locked --format-version 1` and the lockfile, not inferred from this prose. Its creation and exact contents must be confirmed by the final validation record. License fields and dependency presence are inventory evidence, not legal clearance, vulnerability clearance or executed platform acceptance.

| Direct dependency family | Purpose | Important boundary |
|---|---|---|
| `serde`, `serde_json` | Typed records, canonical transport projections and local API encoding | JSON shape is not logical authority; exact JSON selection uses a source-span parser to preserve original number lexemes |
| `serde-value` | Inspect the serialization value tree before canonical JSON encoding | Rejects binary floats, NaN and infinity instead of allowing lossy/null numeric canonicalization; exact noninteger data uses explicit strings |
| `sha2` | Original/result digests and linked identity records | Integrity relative to recorded bytes is not factual truth or authorization |
| `thiserror` | Typed I/O/JSON/contract errors | Different failure, suspension and unknown outcomes must remain distinguishable |
| `num-bigint`, `num-rational`, `num-traits` | Exact arithmetic, model probabilities, polynomial/linear/statistical operations | Explicit bit/expression/action bounds remain necessary; no silent float substitution |
| `unicode-normalization` | Explicit normalization in supported arithmetic/model views and identifier validation | Exact original bytes/spans are retained; normalization cannot erase source distinctions |
| `regex` | Bounded declarative input patterns | Matching a finite grammar is not general Japanese/English understanding |
| `rusqlite` with `bundled` | Local transactional reference store | Bundled SQLite C/FFI is a transitive native dependency; WAL does not atomically perform external effects |
| `reqwest` with `blocking`, `native-tls` | Bounded HTTPS source observations through inherited proxy/CA settings | No disabled certificate checks, implicit broad host permission or source-as-instruction promotion |
| `url` | Typed URL parsing and origin/redirect checks | Valid syntax does not itself grant destination/data access |

## Native prerequisites

Bundled SQLite needs a supported C compiler and standard build tooling. On this Linux reference host, native TLS uses the installed OpenSSL/toolchain integration. Other operating systems may select a different native TLS backend. Required native libraries, trust, VFS operations and target linkers must be observed per target; an available Rust target triple alone does not establish executable portability.

No first-party Rust crate introduces `unsafe`; native transitive code still belongs to the supply-chain and portability review. See the [alternatives record](IMPLEMENTATION_CHOICES.md) for the bounded reasons behind the reference choices.

## Cloud locations

The documented development commands use:

```bash
export CARGO_HOME=/workspace/.cargo
export RUSTUP_HOME=/workspace/.rustup
export CARGO_TARGET_DIR=/workspace/.minidora-cloud/rust-target
export PATH="$CARGO_HOME/bin:$PATH"
cd /workspace/MINIDORA-BETA/rust
cargo metadata --locked --format-version 1
cargo build --workspace --locked
```

These locations keep dependency cache, compiled files and runtime state separate from frozen predecessor source/release areas. Use the existing isolated cloud checkout; do not create a Git worktree unless the user explicitly requests one. The predecessor's pinned Python dependencies are separately prepared by onboarding and are not hidden Rust runtime dependencies.

Local mathematical/relation/file workflows need no external service credential. The optional loopback HTTP mode uses a locally configured ephemeral token. Actual external providers require their own verified network/identity/permission prerequisites; missing requirements are recorded without inserting credentials into source, examples, state memories or audit artifacts.

## Limits of current dependency evidence

A generated dependency/license inventory identifies packages and declared licenses under the actual locked feature graph. It does not establish that every transitive native build works on Windows/macOS, that protected data is encrypted, that all advisories have been reviewed, or that the full model/source/regression population has passed. Record any such validation as separate current-run evidence with its tool version and results. No remote publication or dependency upgrade is authorized merely by generating metadata.
