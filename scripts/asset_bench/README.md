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
- `bench.py`: setup, build, prepare, score, collect, report, calib, votes, pairs, judgment,
  selftest (`bench.py -h`).
- `.claude/agents/asset-bench-author.md` and `asset-bench-judge.md`: the pinned runners (author:
  Sonnet at medium effort; judge: Opus at medium effort; since 2026-10-15 both start without
  `CLAUDE.md` and its imports, `omitClaudeMd`). A session that started before these files
  existed cannot launch them by name; use a general-purpose agent with the same model, effort
  and the file's prompt body, and record that (it also loads `CLAUDE.md` and its imports).
- `results/`: committed. `runs.tsv` (one row per run), `judgments.tsv` (one row per judge
  answer), `summary.json` (per gate and arm, and the calibration) and `assets/<gate>/<arm>/` (every
  final asset as text, for re-scoring and re-judging later), and `agent_tokens.tsv` (what each
  author run's agent read and wrote). Run folders, logs and images stay in `.cache/asset_bench/`
  (git-ignored).

## A gate

```sh
scripts/asset_bench/bench.py setup                      # venv with tiktoken==0.12.0
scripts/asset_bench/bench.py build --arm A              # release pav at a9ca003 (worktree, then removed)
scripts/asset_bench/bench.py prepare --gate GA --arm A --tasks authoring
# launch one asset-bench-author agent per run, one at a time: "RUN = <run folder>"
scripts/asset_bench/bench.py prepare --gate GA --arm A --tasks S8 --run 2
scripts/asset_bench/bench.py prepare --gate GA --arm A --tasks edits
# launch the remaining runs, record each runner's report with bench.py note RUN tok_run=...
scripts/asset_bench/bench.py votes --gate GA --arm A       # edit runs: before/after images
# three asset-bench-judge agents per image ("Your task: EDIT"), each answer recorded with
# bench.py judgment IMAGE JUDGE '{"done":true,...}'
scripts/asset_bench/bench.py pairs --gate G2 --arm F --vs GA/A   # authoring runs vs the baseline's
# three asset-bench-judge agents per image ("Your task:", the image and its brief), recorded with
# bench.py judgment IMAGE JUDGE '{"pref":1,"s1":[...],"s2":[...],...}'
scripts/asset_bench/bench.py score --gate GA
scripts/asset_bench/bench.py collect --gate GA
scripts/asset_bench/bench.py report --gate GA [--vs GA/A]   # --vs: the §10.2 ratios to a baseline
```

`report --vs` splits authoring and edit tasks, and for a gate with pass rules in `GATES` (G2 so
far) applies §10.2: every planned run present, the token ratios, acceptance against the
baseline's run 1 of the same tasks, the median of failed commands, and the pairs. Each pair is
decided by the majority of three distinct judges (a split or a majority for "equal" is a tie);
until all pairs have three, the pair checks fail and no kill criterion is called. The rubric
difference is the arm's median score minus the baseline's, averaged over the five criteria and
then over the pairs. A side that draws nothing loses its pair without judges. Pairs are read from
`judgments.tsv` alone (its target and order columns), so a fresh checkout reports the same.

Run author agents one at a time. They write scratch files to `/tmp` under short names, and in
G1's first batch eight concurrent runs overwrote each other's (three ran another task's asset),
so that batch was discarded.

## Judges

`bench.py calib` makes the calibration images: three templates and three Gate-A assets, each with
two planted defects (a part resting on another raised 10 cm, the object 1.8 times as tall, random
saturated colours, the largest part deleted, a supporting part pushed up 30% of its height through
what it holds, every part replaced by its bounding sphere), each pair stacked in both orders. Every
judged image is a sheet (`asset_preview views=true tile=320 elevation=20 figure=true`, newest
build) under a hashed name in `.cache/asset_bench/judge/img/`, so nothing in its path names the
arm. Each image gets three fresh `asset-bench-judge` agents; `bench.py calib --report` applies the
§10.1 rule (the intact asset wins at least 10 of 12 pairs in both orders, at most 2 order flips).
The judge prompt is frozen once calibration passes.

## Metrics

- `tok_x`: o200k tokens of every command and reply, plus image tokens (ceil(w/28)·ceil(h/28)),
  plus all of `RUN/docs`. Timing fields (`*_ms`) count as 0 and the run's own path as `RUN`, so a
  replay gives the same number (`bench.py selftest`).
- `tok_cum`: the context each wrapper call starts from, summed, plus the final context: what
  re-reading over turns costs.
- `tok_run`: what the runner reported (informational).
- `agent_tokens.tsv`: what each author run's agent read and wrote, from the session's transcripts
  (kept only while that session lasts): model calls, the first call's context (the agent's fixed
  prompt and tools, about 55k tokens), the last call's, their sum over calls (`processed`), the
  part not read from cache (`fresh`, which depends on timing) and the visible output in o200k
  tokens (`out_visible`: tool calls and text; the transcripts keep no hidden reasoning and no
  usable output count). `tok_x` and `tok_cum` leave out the fixed prompt that every call
  re-reads, so they miss changes in the number of calls: arm F2 cut `tok_cum` to 0.56 of arm
  A's, yet end to end its runs cost 0.91 (`docs/HISTORY.md`, 2026-10-12).
- Acceptance (`score`, with the newest engine): every asset loads; it stands on the floor (min y
  within ±0.5 cm), hangs (max y within ±2 cm of 0) or mounts (min z from -1 to +2 cm) as briefed;
  every axis is within tolerance; light tasks glow (emissive above 0.5, or a light); `check` finds
  no floating, unit_suspect, below_floor or not_grounded part (the floor codes are skipped for
  hanging and wall tasks); and an edit run also needs at least 2 of 3 judges to answer `done`.
- `lint_block` / `lint_other`: `check` findings that block acceptance / all the others, per run;
  `tris`: triangles from the `cost` line.

## The asset store's S1

`store_briefs.toml` holds six room-scale briefs (`tasks.toml` stays frozen). `bench.py
store-prepare --arm S1A|S1L` gives each run its own props (the committed packs), clip folders and
miss log; arm S1A sets `PAV_FIND=builtin` in `run.json`'s `env` (the templates only, no shelf).
`done.json` lists every item placed, forked or made with its role and position; `bench.py
store-score` resolves and checks them, `bench.py tokens` reads each run's model calls, tokens read
and weighted cost from the session's subagent transcripts (`--main`: the session's own), and
`bench.py store-report --run N` applies S1's rule (`docs/HISTORY.md`, 2026-10-15).

