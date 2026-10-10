#!/usr/bin/env bash
# Build the pinned local compiler and both native programs, then open Creature Studio.
set -euo pipefail
cd "$(dirname "$0")/.."
npm ci --ignore-scripts --no-audit --no-fund --prefix tools/creature-compiler
npm run build --prefix tools/creature-compiler
cargo build --locked -p pav_app -p pav_tools
exec ./target/debug/shardfall --creature-studio "$@"
