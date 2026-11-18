#!/usr/bin/env bash
# Makes Shardfall's package for 52in1 (AI Hub Catalogue v1, docs/AI_HUB.md) and checks it:
#
#   scripts/hub-package.sh --model "NAME" [--effort LEVEL] [--build v2]
#
# target/hub/package/
#   games/shardfall.html        the game in one file, with the hub's head (scripts/build-hub.sh);
#                               --build v2 names it shardfall-v2.html, a new build of the game the
#                               wall already holds (the checker refuses a slug the wall has)
#   covers/shardfall/0..2.jpg   the opening view (the plaza) and two moments of play (scripts/hub_covers.py)
# then runs the hub's checker (catalogue-check.mjs, fetched into .cache) on it. Needs Node 18+
# and Python Playwright with Chromium (the screenshots run the page with WebGPU on SwiftShader).
set -euo pipefail
cd "$(dirname "$0")/.."
model=""
effort=""
build=""
while (($#)); do
	case $1 in
	--model) model=$2; shift ;;
	--effort) effort=$2; shift ;;
	--build) build=$2; shift ;;
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
page="$pkg/games/shardfall${build:+-$build}.html"
cp target/hub/pages/shardfall.html "$page"
python3 -P scripts/hub_covers.py "$page" "$pkg/covers/shardfall"
mkdir -p .cache
curl -fsSL -o .cache/catalogue-check.mjs https://52in1.com/lib/hub/catalogue-check.mjs
node .cache/catalogue-check.mjs "$pkg"
