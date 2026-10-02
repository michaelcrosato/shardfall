#!/usr/bin/env bash
# Installs the pinned browser toolchain when needed, then builds Vercel's static output.
set -euo pipefail
cd "$(dirname "$0")/.."

export PATH="$HOME/.cargo/bin:$PATH"
if ! command -v rustup >/dev/null; then
	curl --proto '=https' --tlsv1.2 -fsSL https://sh.rustup.rs |
		sh -s -- -y --no-modify-path --profile minimal --default-toolchain none
fi
toolchain=$(sed -n 's/^channel = "\([^"]*\)"/\1/p' rust-toolchain.toml)
rustup toolchain install "$toolchain" --profile minimal --target wasm32-unknown-unknown

bindgen_version=$(awk '
  /^name = "wasm-bindgen"$/ { found = 1; next }
  found && /^version = / { gsub(/"/, "", $3); print $3; exit }
' Cargo.lock)
if ! command -v wasm-bindgen >/dev/null ||
	[[ $(wasm-bindgen --version) != "wasm-bindgen $bindgen_version" ]]; then
	tools_dir="$PWD/target/vercel-tools"
	mkdir -p "$tools_dir"
	asset="wasm-bindgen-$bindgen_version-x86_64-unknown-linux-musl"
	release="https://github.com/wasm-bindgen/wasm-bindgen/releases/download/$bindgen_version"
	curl -fsSL --retry 3 "$release/$asset.tar.gz" -o "$tools_dir/$asset.tar.gz"
	curl -fsSL --retry 3 "$release/$asset.tar.gz.sha256sum" -o "$tools_dir/$asset.tar.gz.sha256sum"
	(
		cd "$tools_dir"
		checksum=$(awk 'NR == 1 { print $1 }' "$asset.tar.gz.sha256sum")
		printf '%s  %s\n' "$checksum" "$asset.tar.gz" | sha256sum --check
		tar -xzf "$asset.tar.gz" --strip-components=1 "$asset/wasm-bindgen"
	)
	export PATH="$tools_dir:$PATH"
fi

export CARGO_BUILD_JOBS="${CARGO_BUILD_JOBS:-4}"
scripts/build-web.sh
