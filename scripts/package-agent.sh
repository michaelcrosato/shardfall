#!/usr/bin/env bash
# Packages a Git checkout as a standalone source handoff for a Linux agent.
set -euo pipefail

if [[ ${1:-} == --help || ${1:-} == -h ]]; then
	printf 'Usage: %s [output.tar.gz]\n' "$0"
	exit 0
fi
if (($# > 1)); then
	printf 'Expected at most one archive destination.\n' >&2
	exit 1
fi

repo_dir=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
output=${1:-"$repo_dir/out/agent/shardfall-linux-agent.tar.gz"}
mkdir -p "$repo_dir/out/agent" "$(dirname "$output")"
manifest=$(mktemp "$repo_dir/out/agent/source-files.XXXXXX")
trap 'rm -f "$manifest"' EXIT

git -C "$repo_dir" ls-files -z >"$manifest"
# Include these handoff files even before their first commit.
for extra in START_HERE.md scripts/package-agent.sh; do
	if ! git -C "$repo_dir" ls-files --error-unmatch -- "$extra" >/dev/null 2>&1; then
		printf '%s\0' "$extra" >>"$manifest"
	fi
done

tar --create --gzip --file "$output" --directory "$repo_dir" \
	--transform 'flags=r;s|^|shardfall/|' \
	--null --verbatim-files-from --no-recursion --files-from="$manifest"
(
	cd "$(dirname "$output")"
	sha256sum "$(basename "$output")" >"$(basename "$output").sha256"
)
printf 'Archive: %s\n' "$output"
ls -lh "$output"
