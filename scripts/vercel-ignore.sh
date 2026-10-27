#!/usr/bin/env bash
# Vercel's Ignored Build Step (vercel.json "ignoreCommand"): exit 0 skips the build, 1 builds.
# Skips only when every file changed since the last successful deployment is one the browser
# build never reads: docs, notes, agent settings, tests and helper scripts. Anything else, or
# any doubt (no previous deployment, a commit missing from the shallow clone), builds.
# Try it locally: VERCEL_GIT_PREVIOUS_SHA=<sha> scripts/vercel-ignore.sh
set -uo pipefail
cd "$(dirname "$0")/.." || exit 1

base=${VERCEL_GIT_PREVIOUS_SHA:-}
if [[ -z $base ]]; then
	echo "vercel-ignore: no previous deployment on this branch: build"
	exit 1
fi
if ! git cat-file -e "$base^{commit}" 2>/dev/null &&
	! GIT_TERMINAL_PROMPT=0 timeout 60 git fetch --quiet --depth=1 origin "$base" 2>/dev/null; then
	echo "vercel-ignore: $base is not in the clone: build"
	exit 1
fi
if ! changed=$(git diff --name-only "$base" HEAD); then
	echo "vercel-ignore: can't diff $base..HEAD: build"
	exit 1
fi

if [[ -z $changed ]]; then
	echo "vercel-ignore: nothing changed since ${base:0:7}: skip"
	exit 0
fi

# Checked against the build's inputs (cargo's dep-info and pav_core/build.rs) on 2026-10-27.
while IFS= read -r path; do
	[[ -n $path ]] || continue
	case $path in
	scripts/build-vercel.sh | scripts/build-web.sh | scripts/vercel-ignore.sh) ;;
	crates/*/tests/*/*) ;; # fixtures: code embeds some of them
	docs/* | *.md | .claude/* | .github/* | pavilion-lite/* | scripts/* | crates/*/tests/*.rs | \
		.mcp*.json | .gitignore | rustfmt.toml)
		continue
		;;
	esac
	echo "vercel-ignore: $path changed since ${base:0:7}: build"
	exit 1
done <<<"$changed"
echo "vercel-ignore: only docs, notes, tests or scripts changed since ${base:0:7}: skip"
exit 0
