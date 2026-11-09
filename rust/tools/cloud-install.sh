#!/usr/bin/env bash
set -euo pipefail
repo=/workspace/MINIDORA-BETA
state=/workspace/.minidora-cloud
mkdir -p "$state/logs" "$state/wheels" "$state/pip-cache"
export PYTHONDONTWRITEBYTECODE=1
export PIP_CACHE_DIR="$state/pip-cache"
export PIP_DISABLE_PIP_VERSION_CHECK=1
python3.12 -m venv "$state/venv"
py="$state/venv/bin/python"
stage=$(mktemp -d "$state/build.XXXXXX")
trap 'rm -rf -- "$stage"' EXIT
# Build and test a disposable copy; do not write to the frozen checkout.
"$py" - "$repo" "$stage/source" <<'PY'
import shutil, sys
shutil.copytree(sys.argv[1], sys.argv[2], ignore=shutil.ignore_patterns(
    '.git', '__pycache__', '*.pyc', '*.egg-info', '.venv', 'venv',
    'build', 'dist', '.pytest_cache', '.mypy_cache'))
PY
"$py" -m pip wheel --no-deps --wheel-dir "$state/wheels" "$stage/source"
"$py" -m pip install --force-reinstall "$state/wheels/minidora_notnn-0.6.0b1-py3-none-any.whl"
"$py" -m pip check
cd "$stage/source"
"$py" -X utf8 examples/controlled_smoke.py | tee "$state/logs/controlled-smoke.log"
"$py" tools/beta_acceptance.py 2>&1 | tee "$state/logs/beta-acceptance.log"
"$state/venv/bin/minidora" --help > "$state/logs/minidora-help.log"
"$state/venv/bin/minidora-product" --help > "$state/logs/minidora-product-help.log"

# Rust successor toolchain and locked dependency workflow.
export CARGO_HOME=/workspace/.cargo
export RUSTUP_HOME=/workspace/.rustup
export CARGO_TARGET_DIR=/workspace/.minidora-cloud/rust-target
export PATH="$CARGO_HOME/bin:$PATH"
if [ ! -x "$CARGO_HOME/bin/rustup" ]; then
    curl --fail --show-error --silent --location https://sh.rustup.rs --output /workspace/.minidora-cloud/rustup-init.sh
    sh /workspace/.minidora-cloud/rustup-init.sh -y --no-modify-path --profile minimal --default-toolchain 1.99.1
fi
rustup toolchain install 1.99.1 --profile minimal --component rustfmt,clippy
cd /workspace/MINIDORA-BETA/rust
cargo fetch --locked
cargo build --workspace --locked
cargo test --workspace --all-features --locked
