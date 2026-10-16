#!/usr/bin/env bash
# Checks the handoff archive the way a receiving agent uses it: packages the checkout, unpacks
# the archive in an empty directory, builds `pav` there (no Git, nothing else from this
# checkout), plays the starter game with its bot, captures a frame and runs the games tests.
# Run it after changing what the archive carries, START_HERE.md or docs/MAKE_A_GAME.md.
# Usage: scripts/check-handoff.sh [--fresh-target]   (default: builds into target/handoff-check,
# cold the first time and quick after; --fresh-target builds in a temporary directory)
set -euo pipefail

repo_dir=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
work=$(mktemp -d "${TMPDIR:-/tmp}/pav-handoff.XXXXXX")
trap 'rm -rf "$work"' EXIT
target="$repo_dir/target/handoff-check"
[[ ${1:-} == --fresh-target ]] && target="$work/target"

"$repo_dir/scripts/package-agent.sh" "$work/shardfall-linux-agent.tar.gz" >/dev/null
mkdir "$work/unpacked"
tar -xzf "$work/shardfall-linux-agent.tar.gz" -C "$work/unpacked"
cd "$work/unpacked/shardfall"
for f in START_HERE.md docs/MAKE_A_GAME.md crates/pav_core/src/games/starter.rs; do
	[[ -f $f ]] || { echo "check-handoff: $f is missing from the archive" >&2; exit 1; }
done
[[ ! -e pavilion-lite ]] || { echo "check-handoff: pavilion-lite/ should stay out of the archive" >&2; exit 1; }

export CARGO_TARGET_DIR=$target CARGO_BUILD_JOBS=${CARGO_BUILD_JOBS:-4}
cargo build --locked -q -p pav_tools --bin pav
pav="$target/debug/pav"
result=$("$pav" play scene=starter bot=true seeds=3)
echo "$result"
grep -q '"wins":"3/3"' <<<"$result" || { echo "check-handoff: the starter's bot didn't win 3/3" >&2; exit 1; }
"$pav" capture scene=starter ticks=60 width=320 height=180 out=out/starter.png >/dev/null
[[ -s out/starter.png ]] || { echo "check-handoff: no capture" >&2; exit 1; }
cargo test --locked -q -p pav_core --test games
echo "check-handoff: OK ($(du -h "$work/shardfall-linux-agent.tar.gz" | cut -f1) archive)"
