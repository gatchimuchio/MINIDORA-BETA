# MINIDORA-BETA cloud startup

Use the existing checkout at /workspace/MINIDORA-BETA. Each cloud task is already isolated; do not create a Git worktree unless the user explicitly requests it. Read AGENTS.md. The frozen beta checkout must remain read-only; do not change source, tests, manifests, locks, or release hashes. Build and test disposable copies outside the checkout.

The prepared standard workflow uses Python 3.12, minidora-notnn 0.6.0b1, SudachiPy 0.6.11 and SudachiDict-core 20260723. No background service, external cloud credentials or cloud extras are required. Processes do not need to survive snapshot restoration.

Activate and verify from /workspace:

```bash
source /workspace/.minidora-cloud/venv/bin/activate
export PYTHONDONTWRITEBYTECODE=1
export PIP_CACHE_DIR=/workspace/.minidora-cloud/pip-cache
cd /workspace/.minidora-cloud
python --version
python -m pip check
minidora --help
minidora-product --help
```

If the environment is absent or the package needs refreshing, run the saved install_script. The tested local copy is /workspace/.minidora-cloud/install.sh. It builds a wheel from a disposable source copy, installs it in the external venv, runs the installed-package smoke and the selected beta suite, and preserves the original checkout.

For functional readiness and the scoped regression check, execute this Bash block from /workspace (with the venv activated):

```bash
set -euo pipefail
export PYTHONDONTWRITEBYTECODE=1
validation=$(mktemp -d /workspace/.minidora-cloud/validation.XXXXXX)
trap 'rm -rf -- "$validation"' EXIT
python - /workspace/MINIDORA-BETA "$validation/source" <<'PY'
import shutil, sys
shutil.copytree(sys.argv[1], sys.argv[2], ignore=shutil.ignore_patterns(
    '.git', '__pycache__', '*.pyc', '*.egg-info', '.venv', 'venv',
    'build', 'dist', '.pytest_cache', '.mypy_cache'))
PY
cd "$validation/source"
python -X utf8 examples/controlled_smoke.py
python tools/beta_acceptance.py
cd /workspace/.minidora-cloud
```

The installed-wheel smoke must complete with terminal COMMIT, a rendered completed result, and successful result lookup. The beta acceptance runner must complete with 313 tests and OK; it loads the source copy rather than the installed wheel. This is selected Linux/Python acceptance, not the full upstream suite or cross-platform/cloud acceptance. Preserve runner exit codes. No web preview is needed. Compare git status --short before/after any setup and preserve pre-existing changes. Logs from onboarding are in /workspace/.minidora-cloud/logs.


## Rust successor development

The independent Rust candidate is /workspace/MINIDORA-BETA/rust, version 0.7.0-alpha.1. It is not full R2/93-capability acceptance. Preserve the frozen Python source/release bytes. Use the existing isolated checkout; do not create a Git worktree unless explicitly requested.

```bash
export CARGO_HOME=/workspace/.cargo
export RUSTUP_HOME=/workspace/.rustup
export CARGO_TARGET_DIR=/workspace/.minidora-cloud/rust-target
export PATH="$CARGO_HOME/bin:$PATH"
cd /workspace/MINIDORA-BETA/rust
rustc --version --verbose
cargo check --workspace --all-targets --locked
cargo test --workspace --all-features --locked
cargo run --locked -p minidora-cli -- --db /workspace/.minidora-cloud/rust-state --case demo --input calculation-1 --json '2+3'
cargo run --locked -p minidora-cli -- --db /workspace/.minidora-cloud/rust-state --case demo --input explanation-1 --json 'その答えを詳しく説明して'
```

Rust CLI is normally one-shot. No background server is required. Optional loopback HTTP startup/authentication/readiness and exact local-effect permissions are in rust/README.md; live processes and local tokens must not be assumed to survive restoration. Use a newly generated undisplayed local token if starting HTTP and stop only servers started for the current task. Do not expose a user-facing localhost preview link. Search provider access is currently blocked by CONNECT403; required domain additions are documented in rust/audit/runtime-requirements.json and do not imply connectivity already works. Prepared dependencies are retained; run the saved install_script to reproduce/refresh if absent. Keep secrets out of files and evidence.
