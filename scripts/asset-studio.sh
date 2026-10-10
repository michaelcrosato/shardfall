#!/usr/bin/env bash
# Build both native programs, then open the live object workspace.
set -euo pipefail
cd "$(dirname "$0")/.."
cargo build --locked -p pav_app -p pav_tools
exec ./target/debug/shardfall --asset-studio "$@"
