# Asset benchmark

Does a fresh agent, given only the docs it would normally get, make better assets for fewer
tokens? This harness answers it for each phase of `docs/ASSET_SYSTEM_PLAN.md` (§10): every gate
runs the same frozen tasks (`tasks.toml`) on a new arm and compares it with arm A, the engine at
`a9ca003`.

## Pieces

- `tasks.toml`: 24 authoring tasks (P01-P24), 6 edit tasks (E1-E6, each starting from the same
  arm's asset for its source task) and 6 import tasks (IT1-IT6, from gate G5). Frozen in Phase 0.
- `pav RUN`: the only way a run touches the engine. One `pav repl` of the run's arm per call,
  with cwd RUN and the run's own `props/` and `creatures/`, so saved assets persist and nothing
  else does. It prints replies unchanged, logs every command to `RUN/log.jsonl`, and refuses
  commands past the run's cap (default 30 commands, 6 images).
- `bench.py`: setup, build, prepare, score, collect, report, selftest (`bench.py -h`).
- `.claude/agents/asset-bench-author.md` and `asset-bench-judge.md`: the pinned runners (author:
  Sonnet at medium effort; judge: Opus at medium effort). A session that started before these
  files existed cannot launch them by name; use a general-purpose agent with the same model,
  effort and the file's prompt body, and record that.
- `results/`: committed. `runs.tsv` (one row per run), `summary.json` (per gate and arm) and
  `assets/<gate>/<arm>/` (every final asset as text, for re-scoring and re-judging later). Run
  folders, logs and images stay in `.cache/asset_bench/` (git-ignored).

## A gate

```sh
scripts/asset_bench/bench.py setup                      # venv with tiktoken==0.12.0
scripts/asset_bench/bench.py build --arm A              # release pav at a9ca003 (worktree, then removed)
scripts/asset_bench/bench.py prepare --gate GA --arm A --tasks authoring
# launch one asset-bench-author agent per run: "RUN = <run folder>"
scripts/asset_bench/bench.py prepare --gate GA --arm A --tasks S8 --run 2
scripts/asset_bench/bench.py prepare --gate GA --arm A --tasks edits
# launch the remaining runs, record each runner's report with bench.py note RUN tok_run=...
scripts/asset_bench/bench.py score --gate GA
scripts/asset_bench/bench.py collect --gate GA
scripts/asset_bench/bench.py report --gate GA
```

## Metrics

- `tok_x`: o200k tokens of every command and reply, plus image tokens (ceil(w/28)·ceil(h/28)),
  plus all of `RUN/docs`. Timing fields (`*_ms`) count as 0 and the run's own path as `RUN`, so a
  replay gives the same number (`bench.py selftest`).
- `tok_cum`: the context each wrapper call starts from, summed, plus the final context: what
  re-reading over turns costs.
- `tok_run`: what the runner reported (informational).
- Acceptance (`score`, with the newest engine): every asset loads; it stands on the floor (min y
  within ±0.5 cm), hangs (max y within ±2 cm of 0) or mounts (min z from -1 to +2 cm) as briefed;
  every axis is within tolerance; light tasks glow (emissive above 0.5, or a light); and `check`
  finds no floating, unit_suspect, below_floor or not_grounded part once the engine has `check`
  (`check=na` until then; rows are re-scored when it lands).
