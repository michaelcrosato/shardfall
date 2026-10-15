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
