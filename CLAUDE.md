# CLAUDE.md

<!-- Claude Code expands these imports at launch, so every session (and every subagent that
doesn't set omitClaudeMd) starts with them. Other agents read AGENTS.md, which names the other
two files. -->
@docs/DOCTRINE.md
@AGENTS.md
@docs/PROGRESS.md

## Claude Code
- Subagents load this file and its imports. A subagent that must not see the repository's docs
  (a benchmark author or judge, a blind brief writer) sets `omitClaudeMd: true` in its
  `.claude/agents/` definition, as the two asset-bench agents do.

## Builds for playing and testing (from 2026-11-02)
There is no hosted site: Vercel is retired. The user plays and tests three builds, each one
file made here and handed over (`SendUserFile`), never committed:
- Browser: `scripts/build-web.sh --single-file` -> `target/web/shardfall.html`, one ~10 MB page
  with the gzipped wasm inside; it runs from `file://` in Chrome or Edge.
- Windows: `scripts/build-windows.sh` -> `target/x86_64-pc-windows-gnu/dist/shardfall.exe`.
- Linux: `cargo build --profile dist -p pav_app` -> `target/dist/shardfall`.

Keep them current: when a merged change can alter the game (anything beyond docs, notes, tests
and helper scripts), rebuild the HTML and send it, and the `.exe` or Linux binary when the user
plays that one or asks.

`vercel.json` stays only as a guard: `"git": { "deploymentEnabled": false }` stops the Vercel
project, if it is still connected, from building any push. Don't deploy to Vercel, re-enable
its builds or add a build config back unless the user asks.
