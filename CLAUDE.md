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

## Deploying to Vercel
A merge to `main` deploys to production by itself; nothing else builds (from 2026-10-27).
- Other branches never build: `vercel.json` sets `git.deploymentEnabled` to
  `{ "main": true, "**": false }`, so a PR has no preview to wait on.
- Before building, Vercel runs `scripts/vercel-ignore.sh`: if every file changed since the last
  successful deployment is docs, notes (`*.md`), agent settings, top-level test files or helper
  scripts, it skips the build and the live site stays as it is. Anything else builds.
Why: every Vercel build costs build minutes, and every deployment is stored and billed daily.
Before, every push to every branch built: two thirds of builds were previews nobody opened,
and over a third of the merges to `main` change only docs.

Rules:
- Never turn branch or preview deployments back on, or widen `"deploymentEnabled"`.
- Never use the pause button, deploy hooks, or the Vercel CLI to deploy.
- If the browser build starts reading a file the skip list covers, take it off the list in
  `scripts/vercel-ignore.sh` (the list was checked against the build's inputs on 2026-10-27).

To redeploy without a merge (only when I ask), call the Vercel connector's `create_deployment` with:
- team: michaelcrosato-1122's projects (slug `michaelcrosato-1122s-projects`)
- project: `shardfall`
- gitSource: type `"github"`, org `"michaelcrosato"`, repo `"shardfall"`, ref `"main"`
- target: `"production"`

Then wait for the build to finish and report the result and the live URL
(production: https://shardfall-eight.vercel.app). The skip check runs here too: redeploying the
commit that is already live, or one that changed only skip-list files, builds nothing.
