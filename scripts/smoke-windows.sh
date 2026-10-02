#!/usr/bin/env bash
# Runs the Windows .exe under Wine + Xvfb + lavapipe for a few seconds and saves the log and a
# screenshot to out/smoke/. Usage: scripts/smoke-windows.sh [seconds] [extra shardfall args...]
set -euo pipefail
cd "$(dirname "$0")/.."
secs="${1:-25}"
shift || true
out=out/smoke
rm -rf "$out"
mkdir -p "$out"
cp target/x86_64-pc-windows-gnu/dist/shardfall.exe "$out/"
export WINEDEBUG=-all DISPLAY=:97
Xvfb :97 -screen 0 1600x900x24 >/dev/null 2>&1 &
xvfb=$!
trap 'kill $xvfb 2>/dev/null || true' EXIT
sleep 1
[ -d "$HOME/.wine" ] || timeout 120 /usr/lib/wine/wine64 wineboot -i >/dev/null 2>&1 || true
(cd "$out" && timeout $((secs + 5)) /usr/lib/wine/wine64 shardfall.exe "$@" >stdout.txt 2>&1 &)
sleep "$secs"
xwd -root -silent | convert xwd:- "$out/screen.png"
sleep 6
echo "log: $out/shardfall.log  screenshot: $out/screen.png"
grep -E "boot|ERROR|CRASH" "$out/shardfall.log" | head -40
