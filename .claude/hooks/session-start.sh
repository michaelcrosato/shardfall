#!/bin/bash
# Cloud-session setup: native build deps, software Vulkan (lavapipe) for captures, and a
# prebuilt `pav` so the shardfall MCP server (.mcp.json) starts without a cold compile.
set -euo pipefail

if [ "${CLAUDE_CODE_REMOTE:-}" != "true" ]; then
  exit 0
fi

cd "$CLAUDE_PROJECT_DIR"

pkgs=(build-essential pkg-config libasound2-dev libudev-dev mesa-vulkan-drivers vulkan-tools
  xvfb libxkbcommon-x11-0 libx11-xcb1 libxcursor1 libxrandr2 libxi6)
missing=()
for p in "${pkgs[@]}"; do
  dpkg -s "$p" >/dev/null 2>&1 || missing+=("$p")
done
if [ ${#missing[@]} -gt 0 ]; then
  sudo_() { if [ "$(id -u)" = 0 ]; then "$@"; else sudo "$@"; fi; }
  sudo_ apt-get update -q
  sudo_ env DEBIAN_FRONTEND=noninteractive apt-get install -y -q "${missing[@]}"
fi

echo 'export CARGO_BUILD_JOBS=4' >> "${CLAUDE_ENV_FILE:-/dev/null}"
CARGO_BUILD_JOBS=4 cargo build --locked -q -p pav_tools --bin pav
