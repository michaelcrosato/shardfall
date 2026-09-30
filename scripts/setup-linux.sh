#!/usr/bin/env bash
# One-time setup for a Linux/WSL2/cloud machine (Ubuntu 24.04): build deps, software Vulkan
# (lavapipe) for headless rendering, the Windows cross-compiler, and Wine/Xvfb for smoke tests.
set -euo pipefail
sudo_() { if [ "$(id -u)" = 0 ]; then "$@"; else sudo "$@"; fi; }
sudo_ apt-get update -q
sudo_ apt-get install -y -q build-essential pkg-config libasound2-dev libudev-dev \
  mesa-vulkan-drivers vulkan-tools gcc-mingw-w64-x86-64 g++-mingw-w64-x86-64 \
  xvfb x11-apps imagemagick wine64 libxkbcommon-x11-0 libx11-xcb1 libxcursor1 libxrandr2 libxi6
# The pinned toolchain (rust-toolchain.toml) installs itself on first cargo use, incl. the Windows target.
cargo --version
