# Escalations

The log `docs/DOCTRINE.md` asks for: every escalation, every conflict reported after one went
unanswered, and every call made without a response. Newest first.

Each entry: the date, the principle or approval involved, the question, what was decided and by
whom (the user, or the agent after 15 minutes without a response), and the PR.

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
