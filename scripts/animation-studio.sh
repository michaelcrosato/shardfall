#!/usr/bin/env bash
# Build both native programs, then open the live animation workspace.
set -euo pipefail
cd "$(dirname "$0")/.."
cargo build --locked -p pav_app -p pav_tools
exec ./target/debug/shardfall --animation-studio "$@"
