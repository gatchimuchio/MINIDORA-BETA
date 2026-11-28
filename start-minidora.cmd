@echo off
setlocal DisableDelayedExpansion
where cargo.exe >nul 2>&1
if errorlevel 1 (
    echo Cargo was not found. Install Rust with rustup and make Cargo available on PATH. 1>&2
    exit /b 1
)

if not exist "%~dp0rust\Cargo.toml" (
    echo MINIDORA: rust\Cargo.toml is missing. Use the launcher from the complete source directory. 1>&2
    exit /b 1
)

pushd "%~dp0rust" || exit /b 1
echo MINIDORA: preparing local chat. The first Rust build can take a few minutes. 1>&2
cargo run --locked -p minidora-cli -- chat --db .data\standard-beta %*
set "launch_status=%errorlevel%"
popd
exit /b %launch_status%
