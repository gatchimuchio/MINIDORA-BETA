#!/bin/sh
set -eu

if ! command -v cargo >/dev/null 2>&1; then
    printf '%s\n' 'Cargo was not found. Install Rust with rustup and make Cargo available on PATH.' >&2
    exit 1
fi

project_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
cd "$project_dir/rust"
exec cargo run --locked --quiet -p minidora-cli -- chat --db .data/standard-beta "$@"
