# Escalations

The log `docs/DOCTRINE.md` asks for: every escalation, every conflict reported after one went
unanswered, and every call made without a response. Newest first.

Each entry: the date, the principle or approval involved, the question, what was decided and by
whom (the user, or the agent after 15 minutes without a response), and the PR.

## 2026-10-15 — S3b: rigid figures, or build skinning?
- **Question** (the user's alone, §15: no default): the five imported KayKit characters play
  every clip as rigid per-bone segments. Do the joints read well enough, or should skinning be
  built (the creature path's CPU skinning)?
- **Asked:** in chat, with the review page https://claude.ai/artifact/VjP2RUbwaSJ3ggXkrJJnQs
  (one verdict per figure: "reads well" or "joints read badly"; verdicts reach the agent).
- **Decided by:** pending user. S3b was built around it and merged rigid (#64).

## 2026-10-15 — S3, S3b, S4 and S5 built despite S1's "no" (the user's call)
- **Question:** S1's "no" still stands, and the store design (§13) builds nothing after it. Build
  the rest of the plan?
- **Decided by:** the user, in chat: "take the plan to completion", then "provide me with a concise
  prompt to finish the project". Their decisions in the same session: completion first; costs in
  US$ as Claude Code bills them; hard test limits per stage (S3 $30 4 h, S3b $10 1 h, S4 $5 1 h,
  S5 $25 3 h, S6 $0) and soft estimates; S6 and the triggers armed, not run.
- **Call:** S3, S3b, S4 and S5 are built and merged whatever their exit results
  (`docs/AGENT_ASSET_STORE.md` §15). S1's measurement stands as recorded.
- **PR:** the PR that adds §15.

## 2026-10-15 — Asked before S3: the test-game prompt; riftlight #49
- **Questions** (asked in chat before the S3-S5 run): 1. The verbatim test-game prompt template
  (`docs/FEEDBACK.md` holds a paraphrase). Default: S3 adds its three lines to that paraphrase
  and gives the user the lines to paste into their template. 2. The owner's own one-line approval
  comment on riftlight #49 for the Kenney 1-Bit Pack. No default: an agent may not approve for the
  owner, so S5's imported arm stays "pending user" until the comment exists.
- **Decided by:** the agent, after 15 minutes without an answer: the run started at 20:53 UTC
  and question 1's default applied at 21:09. S3 added the three lines to FEEDBACK.md's paraphrase
  ("Asset store lines"); the lines to paste into the template went to the user in chat.
- **Later:** the user scoped the run to Shardfall (2026-10-15), so S5's riftlight side is a
  hand-off prompt and question 2 goes with it: the run no longer waits on #49.

## 2026-10-15 — S2 built despite S1's "no" (the user's call)
- **Question:** S1's "no" was final, and under the store design (§13) and the goal nothing after
  S0 is built. The goal's stop hook kept asking for S2, so the agent asked the user: build S2
  anyway, or stop?
- **Decided by:** the user, in chat: build S2 anyway ("There is a good reason. I'll explain
  later."), and take it to completion overnight.
- **Call:** S2's whole §13 row is built, with format-2 `params` (LIB-4) and the foundations the goal
  names. S1's measurement stands as recorded; S2 doesn't re-test it.
- **PR:** S2's PR.

## 2026-10-15 — S1's model-2 rerun: spend past the approval, or stay inside it
- **Question:** the user approved 6M tokens for S1's 12 runs plus one same-size rerun. Round 1
  read 1.0M. The rerun as specified (a general-purpose agent with model 2 and the author file's
  prompt body) read 1.53M on B1 alone, since that agent re-reads a ~55k-token fixed prompt on
  every call. The remaining 10 runs projected to 6-9M more. Asked in chat at 02:35 UTC: spend past
  the approval (option A), or run the pinned `asset-bench-author` with only its model switched to
  model 2 (option B, its ~5k-token fixed prompt, about 1.5-2M, inside the approval).
- **Decided by:** the agent, after 15 minutes without an answer (none by 02:51 UTC), under the
  goal's rule: with no answer, don't spend, work inside the approval.
- **Call:** option B for all 12 runs (run 3), B1 included, so every brief has one harness. The two
  general-purpose B1 runs (run 2) stay in the results as a harness note.
- **Later:** after the S0-S2 report the question was put again: rerun the model-2 round on the
  general-purpose agent as specified (about 7.7M more read, 1.86M left in the approval), or let S1
  stand. The user answered "S1 stands" (twice, and confirmed it in chat: the answers reached the
  agent attached to stop-hook messages, so the agent first doubted them). The rerun is waived, S1's
  result stands as recorded, and nothing more was spent.
- **PR:** S1's PR; the two follow-up PRs that add and correct this line.

## 2026-10-15 — The agent asset store's nine rulings (docs/AGENT_ASSET_STORE.md §14)
- **Question:** the nine open questions of the store design, each with a default. They were
  posted together in chat at 01:13 UTC, before any of S0's rows were written, with a check-in
  set for 15 minutes.
- **Decided by:** the agent, after 15 minutes without an answer (none by 01:49 UTC), under the
  user's goal for S0-S2, which named two of them:
  1. **Own work's licence:** CC0-1.0 for LIB/core, the 13 own materials (now with a `source`
     licence field), VehicleDefs, particle presets, themes and agent-made assets (logged
     before S0's rows).
  2. **Local licence lanes:** they stay as they are, outside the store. The store never
     indexes, finds, shelves or exports them. `assets find=` leaves the local tier out unless
     `tier=local` asks for it, and a test refuses any row whose licence `store/licences.tsv`
     does not admit.
  3. **D5:** kept. Standard holds CC0 and own work; CC-BY tops out at `checked`. STYLE100's 98
     and CMU's 60 embedded clips are logged exceptions until the user rules. The credits text
     (`store/CREDITS.md`) ships either way.
  4. **CMU mocap:** admitted for clips. The user's goal confirms its terms allow redistribution
     and modification. Every CMU row carries the no-resale term from the reviewed row in
     `store/licences.tsv` (`LicenseRef-CMU`).
  5. **Quaternius models:** the pinned CC0 December 2020 release is admitted, checked against
     its licence-file hash.
  6. **Sprites and SVG:** SVG paths count as text and enter when a game asks. Palette-indexed
     sprite rows count as text only for the S5 trial.
  7. **SpawnForge:** no creatures until the user licenses blueprints and compiled outputs.
  8. **Rigged characters:** not in the proof of concept. They come in S3b, after critical mass,
     rigid segments first.
  9. **The Fab haul:** left out. Checked: no Fab file is tracked by git here (the only match,
     `fabrik.ts`, is an IK solver). The question for the collecting agent (how it chose what to
     take) goes to the user in the S0-S2 report.
- **PR:** S0's PR.

## 2026-10-12 — Asset plan resumed in slim form (answer to the G2 escalation below)
- **Question:** the G2 NO-GO below: stop, re-judge, change the gate, or go on.
- **Decided by:** the user, in chat: finish Phases 3-6 with only the features that are needed and
  showed a benefit, without exhaustive testing, under the prototype testing budget now in
  `AGENTS.md`.
- **Call:** G2 is closed and format 2 stays (no proven cost or looks benefit, not shown worse).
  Phases 3-6 are cut to 11 rows with lean gates (plan §10.3); the rest is deferred (plan §11).
- **PR:** the re-plan PR that adds this entry.

## 2026-10-12 — Asset plan stopped at gate G2 (approval to go past a NO-GO)
- **Question:** Gate G2 of `docs/ASSET_SYSTEM_PLAN.md` failed after its one remediation round,
  with no kill criterion holding:
  - Round 1 met quality but not tokens.
  - Round 2 met tokens but not quality: 10 of 24 pairs won (12 needed) and a rubric Δ of −0.183
    (limit −0.1).
  - The judging is noisy: 11 of 24 tasks flipped between rounds against the same baseline assets.

  Going further would need the user's approval. That could be a second judged round of arm F2,
  a changed gate, or Phases 3-6. Each goes past the plan's stop rule and the user's instruction
  for this case ("if it still fails ... stop, record the NO-GO with numbers in HISTORY and the
  plan, and tell me").
- **Decided by:** the user's own instruction, which fixes the call without a reply. The question
  of going further was put to the user in chat on 2026-10-12.
- **Call:** stop. The plan stays at row 2.5 and nothing past it starts until the user says
  otherwise. The NO-GO and its numbers are in `docs/HISTORY.md` and the plan's gate log.
- **PR:** the NO-GO record is #34; this entry has its own PR.

## 2026-10-09 — Pavilion Lite and "WebGPU only" (principle 6)
- **Question:** Pavilion Lite (`pavilion-lite/`) draws with its own CPU renderer, not WebGPU.
  Port it, retire it, or leave it outside the doctrine?
- **Decided by:** the agent, under the user's approval of "all work and deviations at your
  discretion" (2026-10-09).
- **Call:** freeze it. It is a finished experiment and handoff package, untouched since
  2026-10-02. Porting it would duplicate the main engine, which already renders without a GPU
  through lavapipe, and retiring it would throw away a working package for nothing. It isn't
  developed further; if it's ever needed again, porting it to wgpu comes first.
- **PR:** the doctrine follow-up after #17.
