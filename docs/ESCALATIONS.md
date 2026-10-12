# Escalations

The log `docs/DOCTRINE.md` asks for: every escalation, every conflict reported after one went
unanswered, and every call made without a response. Newest first.

Each entry: the date, the principle or approval involved, the question, what was decided and by
whom (the user, or the agent after 15 minutes without a response), and the PR.

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
