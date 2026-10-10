# Run from a Windows source checkout with Node 22.18+ and the pinned Rust toolchain.
$ErrorActionPreference = "Stop"
Set-Location (Join-Path $PSScriptRoot "..")
npm ci --ignore-scripts --no-audit --no-fund --prefix tools/creature-compiler
if ($LASTEXITCODE -ne 0) { throw "Cannot install the pinned compiler dependencies." }
npm run build --prefix tools/creature-compiler
if ($LASTEXITCODE -ne 0) { throw "Cannot build the creature compiler." }
cargo build --locked -p pav_app -p pav_tools
if ($LASTEXITCODE -ne 0) { throw "Cannot build Shardfall Studio." }
& ./target/debug/shardfall.exe --creature-studio @args
