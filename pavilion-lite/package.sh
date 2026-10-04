#!/usr/bin/env bash
# Packs this folder (source, docs, Cargo.lock; no build output) into dist/pavilion-lite.tar.gz
# and dist/pavilion-lite.zip, each with a pavilion-lite/ root folder, plus SHA-256 sums.
set -euo pipefail
here=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
name=pavilion-lite
stage=$(mktemp -d)
trap 'rm -rf "$stage"' EXIT
mkdir -p "$stage/$name" "$here/dist"
(
	cd "$here"
	for f in AGENTS.md CLAUDE.md README.md Cargo.toml Cargo.lock rustfmt.toml package.sh .mcp.json .gitignore src; do
		cp -R "$f" "$stage/$name/"
	done
)
rm -f "$here/dist/$name.tar.gz" "$here/dist/$name.zip"
tar -czf "$here/dist/$name.tar.gz" -C "$stage" "$name"
if command -v zip >/dev/null; then
	(cd "$stage" && zip -qr "$here/dist/$name.zip" "$name")
else
	python3 -c "import shutil,sys; shutil.make_archive(sys.argv[1], 'zip', sys.argv[2], sys.argv[3])" \
		"$here/dist/$name" "$stage" "$name"
fi
(cd "$here/dist" && sha256sum "$name.tar.gz" "$name.zip" >"$name.sha256")
ls -l "$here/dist"
