# Hand-off: the asset store's S5 sprite trial in riftlight

Start a Claude Code session in **michaelcrosato/riftlight** and give it everything below the line.
Shardfall's side of S5 is merged (`docs/AGENT_ASSET_STORE.md` §11, §13, §15 in
michaelcrosato/shardfall): the three briefs and Shardfall's arm, 3D props drawn through the
`pixel_art` filter. This session builds the sprite side and runs the trial. Its result is recorded
in Shardfall as "pending: riftlight session" until a Shardfall session copies it into
`docs/HISTORY.md`.

---

**Task: run the sprite trial. Can sprites stored as text work without looking odd, against
code-drawn art and against Shardfall's pixel-filtered 3D props, on the same three briefs?**

Read Shardfall's §11 ("2D"); fetch only that file:
`https://raw.githubusercontent.com/michaelcrosato/shardfall/main/docs/AGENT_ASSET_STORE.md` (pin the
commit you read: `git ls-remote https://github.com/michaelcrosato/shardfall main`).
The briefs are in `scripts/asset_bench/s5_briefs.toml` at the same commit: P1 a dungeon treasure
nook, P2 a village well corner, P3 a forest camp, each a 5 x 5 m object scene seen from a high
camera, characters left out.

Build in riftlight (its own conventions, tests and PR flow):
1. **Sprite rows and a reader.** Palette-indexed pixel rows with a legend pointing at tone slots;
   per game one pixels-per-metre value, one palette, nearest filtering, integer scale, pivot at the
   feet. Measure the tokens of a 32x32 sprite this way (o200k); §11 estimated about 424-451.
   Unknown formats and fields are refused.
2. **Edit operations, each checked by a render diff:** recolour, mirror, outline, shift, paste,
   remap palette, integer scale. An agent edits sprites only through these.
3. **The mixing check** with 5 planted faults: mixed pixel densities side by side, a non-integer
   scale, bilinear filtering, a clashing palette, and one more you choose. Pass: 5 of 5 caught.
4. **A harness** whose runs use a blind author definition written with it (a `.claude/agents/`
   file with `omitClaudeMd: true`), recording each run's model calls and US$ (riftlight's own cost
   method, or Shardfall's `bench.py cost` approach: transcript usage priced per model).
5. **Three arms on the three briefs:**
   - **imported sprites:** Kenney 1-Bit Pack (CC0). This arm waits for the repository owner's own
     approval comment on riftlight #49. An agent never posts, drafts or links that approval; until
     it exists, record the arm "pending user" and build the rest;
   - **code-drawn art:** the agent draws each scene's objects in code (riftlight's own drawing);
   - **Shardfall's arm:** already rendered. The images are on the user's S5 review page
     (https://claude.ai/artifact/NAqUvr4ZyFVw3yfTmkeekr, private to the user); ask the user for the three PNGs (or a Shardfall session can regenerate them
     with `scripts/asset_bench/bench.py store-render --gate S5` after `store-prepare --gate S5 --briefs-file
     scripts/asset_bench/s5_briefs.toml` and the three author runs).
   Agent edits: 10 edit tasks through the operations; pass when the render diff holds in at least
   8 of 10.
6. **A both-orders review page** for the user: per brief, each pair of arms shown left/right and
   then right/left (sprites against each other arm), verdicts left, right or same. Leave fine
   visual judgement to the user; no agent judges.

**Success (all four, §11):** sprites win at least 2 of 3 briefs against each other arm in both
orders on the user's page; they need no more model calls than the cheaper other arm; the import
check catches 5 of 5 planted mixing faults; agent edits pass the render diff in at least 8 of 10.
**Kill:** any one fails. The code merges either way; on a kill, sprites stay out of any store
defaults and shelf.

**Test limit:** S5's whole testing budget is $25 and 3 hours (Shardfall spent $0.11 of it on its three author runs).
State each batch's US$ before it runs; at the limit, stop testing, record the result on the runs
done, and keep building.

**Report back** (paste into a Shardfall session): the four criteria with their numbers, the arms'
model calls and US$, whether #49's approval existed, the page link, and the PR link.

Rules: write only to riftlight. Don't clone Shardfall; fetch the named files only. Keep each
source's licence and credit (Kenney, CC0).
