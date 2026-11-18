#!/usr/bin/env bash
# Builds Shardfall's page for 52in1 (docs/AI_HUB.md): the browser build, with the hub's head
# (catalogue and session tags for every way to play together, the session client), the line
# book and the fonts. The hub picks the online mode each round from the page's hub:multiplayer.
#
#   scripts/build-hub.sh --model "NAME" [--effort LEVEL]
#
# Out: target/hub/pages/shardfall.html. The package itself (games/ and covers/) is
# scripts/hub-package.sh's.
set -euo pipefail
cd "$(dirname "$0")/.."
model=""
effort=""
while (($#)); do
	case $1 in
	--model) model=$2; shift ;;
	--effort) effort=$2; shift ;;
	*) echo "build-hub.sh: unknown option $1" >&2; exit 2 ;;
	esac
	shift
done
[[ -n $model ]] || { echo "build-hub.sh: --model NAME (the model that wrote this build, as its maker names it)" >&2; exit 2; }
cargo build --locked --profile dist --target wasm32-unknown-unknown -p pav_app
wasm=${CARGO_TARGET_DIR:-target}/wasm32-unknown-unknown/dist/shardfall.wasm
bindgen=target/hub/bindgen
rm -rf "$bindgen"
mkdir -p "$bindgen" target/hub/pages
wasm-bindgen --target web --no-typescript --out-dir "$bindgen" --out-name shardfall "$wasm"
cargo run -q -p pav_tools --bin pav -- lines out="$bindgen/game-lines.json" >/dev/null
extra=()
[[ -n $effort ]] && extra+=(--effort "$effort")
python3 -I scripts/pack_single_html.py "$bindgen" --lines "$bindgen/game-lines.json" --fonts assets/fonts \
	--hub web/hub.json --model "$model" "${extra[@]}" --out "target/hub/pages/shardfall.html"
