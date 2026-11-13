#!/usr/bin/env bash
# Makes Shardfall's AI Hub package (AI Hub Catalogue v1, docs/AI_HUB.md) and checks it:
#
#   scripts/hub-package.sh --model "NAME" [--effort LEVEL] [--mode shared-world|ghost|parallel]
#
# target/hub/package/
#   games/shardfall.html        the game in one file, with the hub's head (scripts/build-hub.sh)
#   covers/shardfall/0..2.jpg   the opening view (the plaza) and two moments of play (scripts/hub-covers.js)
# then runs the hub's checker (catalogue-check.mjs, fetched into .cache) on it. Needs Node 18+
# and Playwright with Chromium (the screenshots run the page with WebGPU on SwiftShader).
set -euo pipefail
cd "$(dirname "$0")/.."
model=""
effort=""
mode="shared-world"
while (($#)); do
	case $1 in
	--model) model=$2; shift ;;
	--effort) effort=$2; shift ;;
	--mode) mode=$2; shift ;;
	*) echo "hub-package.sh: unknown option $1" >&2; exit 2 ;;
	esac
	shift
done
[[ -n $model ]] || { echo "hub-package.sh: --model NAME (the model that wrote this build, as its maker names it)" >&2; exit 2; }
args=(--model "$model")
[[ -n $effort ]] && args+=(--effort "$effort")
scripts/build-hub.sh "${args[@]}"
pkg=target/hub/package
rm -rf "$pkg"
mkdir -p "$pkg/games" "$pkg/covers/shardfall"
cp "target/hub/pages/shardfall-$mode.html" "$pkg/games/shardfall.html"
node scripts/hub-covers.js "$pkg/games/shardfall.html" "$pkg/covers/shardfall"
mkdir -p .cache
curl -fsSL -o .cache/catalogue-check.mjs https://raw.githubusercontent.com/michaelcrosato/ai-hub/main/lib/hub/catalogue-check.mjs
node .cache/catalogue-check.mjs "$pkg"
