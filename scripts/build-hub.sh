#!/usr/bin/env bash
# Builds Shardfall's AI Hub pages (docs/AI_HUB.md): the browser build once, then one page per
# session mode with the hub's head (catalogue and session tags, the session client), the line
# book and the fonts.
#
#   scripts/build-hub.sh --model "NAME" [--effort LEVEL]
#
# Out: target/hub/pages/shardfall-{shared-world,ghost,parallel}.html. The package itself
# (games/ and covers/) is scripts/hub_package.py's.
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
for mode in shared-world ghost parallel; do
	python3 -I scripts/pack_single_html.py "$bindgen" --lines "$bindgen/game-lines.json" --fonts assets/fonts \
		--hub web/hub.json --session-mode "$mode" --model "$model" "${extra[@]}" --out "target/hub/pages/shardfall-$mode.html"
done
