# Run from PowerShell with the native Windows Rust toolchain installed.
$ErrorActionPreference = "Stop"
Set-Location (Join-Path $PSScriptRoot "..")
cargo build --locked -p pav_app -p pav_tools
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
& .\target\debug\shardfall.exe --asset-studio @args
exit $LASTEXITCODE
