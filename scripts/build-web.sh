#!/usr/bin/env bash
# Builds the browser version (WebAssembly + WebGPU). Output: target/web/{index.html,manifest.webmanifest,shardfall.js,shardfall_bg.wasm}
# Serve that folder over HTTP (e.g. `python3 -m http.server -d target/web`) and open it in Chrome or Edge.
# --single-file writes only target/web/<title>.html (shardfall.html): one page with the glue and gzipped wasm inside
#   that runs from file:// or any host (scripts/pack_single_html.py; also --title, --description, --theme-color,
#   --scene NAME for the start scene when the address names none, --out PATH).
# Needs: rustup target add wasm32-unknown-unknown
#        cargo install wasm-bindgen-cli --version <the wasm-bindgen version in Cargo.lock>
set -euo pipefail
caller=$PWD
cd "$(dirname "$0")/.."
single=false
pack=()
while (($#)); do
	case $1 in
	--single-file) single=true ;;
	--title | --description | --theme-color | --scene | --out)
		(($# > 1)) || { echo "build-web.sh: $1 needs a value" >&2; exit 2; }
		value=$2
		[[ $1 == --out && $value != /* ]] && value=$caller/$value
		pack+=("$1" "$value")
		shift
		;;
	*) echo "build-web.sh: unknown option $1 (options are listed at the top of $0)" >&2; exit 2 ;;
	esac
	shift
done
if ((${#pack[@]})) && ! $single; then
	echo "build-web.sh: ${pack[0]} needs --single-file" >&2
	exit 2
fi
cargo build --locked --profile dist --target wasm32-unknown-unknown -p pav_app
wasm=${CARGO_TARGET_DIR:-target}/wasm32-unknown-unknown/dist/shardfall.wasm
out=target/web
if $single; then
	bindgen=$(mktemp -d)
	trap 'rm -rf "$bindgen"' EXIT
	wasm-bindgen --target web --no-typescript --out-dir "$bindgen" --out-name shardfall "$wasm"
	python3 -I scripts/pack_single_html.py "$bindgen" "${pack[@]}"
	exit
fi
rm -rf "$out"
mkdir -p "$out"
wasm-bindgen --target web --no-typescript --out-dir "$out" --out-name shardfall "$wasm"
cp web/index.html web/manifest.webmanifest "$out/"
ls -la "$out"
