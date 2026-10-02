#!/usr/bin/env bash
# Cross-compiles the Windows build. Output: target/x86_64-pc-windows-gnu/dist/{shardfall,pav}.exe
set -euo pipefail
cd "$(dirname "$0")/.."
cargo build --profile dist --target x86_64-pc-windows-gnu -p pav_app -p pav_tools
ls -la target/x86_64-pc-windows-gnu/dist/{shardfall,pav}.exe
