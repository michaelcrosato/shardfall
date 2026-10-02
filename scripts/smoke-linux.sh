#!/usr/bin/env bash
# Runs the native Linux game under Xvfb + lavapipe and saves a screenshot to out/smoke/.
# Usage: scripts/smoke-linux.sh [seconds] [extra shardfall args...]
set -euo pipefail
cd "$(dirname "$0")/.."
secs="${1:-10}"
shift || true
out=out/smoke
mkdir -p "$out"
cargo build -q -p pav_app
export DISPLAY=:98
Xvfb :98 -screen 0 1600x900x24 >/dev/null 2>&1 &
xvfb=$!
trap 'kill $xvfb 2>/dev/null || true' EXIT
sleep 1
(timeout $((secs + 5)) ./target/debug/shardfall "$@" >"$out/linux.log" 2>&1 &)
sleep "$secs"
xwd -root -silent | convert xwd:- "$out/linux.png"
echo "screenshot: $out/linux.png"
grep -E "boot|ERROR|CRASH" "$out/linux.log" | head -40
