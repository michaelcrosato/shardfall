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
Vercel does NOT auto-deploy from GitHub, on purpose. Pushing to GitHub never deploys.
Why: every Vercel build costs build minutes, and every deployment is stored and billed daily. I
only look at a tiny fraction of builds, so building on every push wasted money on deployments
nobody saw. The last deployment stays live until I ask for a new one.

Rules:
- Never deploy unless I explicitly ask in this session.
- Never re-enable automatic deployments or change `"deploymentEnabled"` in `vercel.json`.
- Never use the pause button, deploy hooks, or the Vercel CLI to deploy.

How to deploy (the only method): call the Vercel connector's `create_deployment` with:
- team: michaelcrosato-1122's projects (slug `michaelcrosato-1122s-projects`)
- project: `shardfall`
- gitSource: type `"github"`, org `"michaelcrosato"`, repo `"shardfall"`, ref `"main"`
- target: `"production"`

Then wait for the build to finish and report the result and the live URL
(production: https://shardfall-eight.vercel.app).
