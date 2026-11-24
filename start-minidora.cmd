@echo off
setlocal DisableDelayedExpansion
where cargo.exe >nul 2>&1
if errorlevel 1 (
    echo Cargo was not found. Install Rust with rustup and make Cargo available on PATH. 1>&2
    exit /b 1
)

pushd "%~dp0rust" || exit /b 1
cargo run --locked --quiet -p minidora-cli -- chat --db .data\standard-beta %*
set "launch_status=%errorlevel%"
popd
exit /b %launch_status%
