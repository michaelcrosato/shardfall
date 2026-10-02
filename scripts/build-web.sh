#!/usr/bin/env bash
# Builds the browser version (WebAssembly + WebGPU). Output: target/web/{index.html,shardfall.js,shardfall_bg.wasm}
# Serve that folder over HTTP (e.g. `python3 -m http.server -d target/web`) and open it in Chrome or Edge.
# Needs: rustup target add wasm32-unknown-unknown
#        cargo install wasm-bindgen-cli --version <the wasm-bindgen version in Cargo.lock>
set -euo pipefail
cd "$(dirname "$0")/.."
cargo build --locked --profile dist --target wasm32-unknown-unknown -p pav_app
out=target/web
rm -rf "$out"
mkdir -p "$out"
wasm-bindgen --target web --no-typescript --out-dir "$out" --out-name shardfall target/wasm32-unknown-unknown/dist/shardfall.wasm
cp web/index.html "$out/"
ls -la "$out"
