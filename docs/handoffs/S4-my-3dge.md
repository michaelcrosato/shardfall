# Hand-off: the asset store's S4 in my-3dge (clips from Shardfall's store rows)

Start a Claude Code session in **michaelcrosato/my-3dge** and give it everything below the line.
Shardfall's side of S4 is merged (`docs/AGENT_ASSET_STORE.md` §13, §15 in michaelcrosato/shardfall);
this session runs the second engine's side. Its result is recorded in Shardfall as "pending:
my-3dge session" until a Shardfall session copies it into `docs/HISTORY.md`.

---

**Task: read ten CC0 clips from Shardfall's asset store rows and check that my-3dge decodes them
to the same body points Shardfall does.**

Shardfall (public: https://github.com/michaelcrosato/shardfall) keeps a store of clips, props and
materials, one row per entry in `store/STORE.tsv`. Your repo's `tools/anim/shardfall.ts` already
reads 8 of Shardfall's clip set files, pinned at Shardfall commit `fa2dab6`.

1. **Move the pin** to Shardfall's newest `main` commit (`git ls-remote
   https://github.com/michaelcrosato/shardfall main`); every commit since S4's merge carries the
   fixture. Fetch only the files you need from
   `https://raw.githubusercontent.com/michaelcrosato/shardfall/<commit>/<path>`: `store/STORE.tsv`,
   `store/fixtures/clip_points.json`, and the set files named in STORE.tsv's `# Clip sets:` line
   (`anim/mesh2motion.json` is new to you; `anim/quaternius.json` you already read). Pin each by
   SHA-256 the way your reader pins the others.
2. **Read the ten rows** the fixture lists (`clips[].id`): six MESH2MOTION clips (new to you) and
   four QUATERNIUS ones. For each, go id → set file (STORE.tsv's `# Clip sets:` line maps
   `SET` → `anim/<file>`) → clip. The rows say `clip` and `CC0-1.0`; keep each set's `credit` and
   `sources[].license` with what you import. Set files are format 1 (Shardfall's `clips.rs`
   documents it; its `legend` field explains every number): angles are whole degrees, hips are in
   percent of standing hip height. Your decoder converts degrees; refuse unknown formats and
   fields as your reader already does.
3. **Decode and compare.** For each clip and each of the fixture's sample times `t`, decode the
   key pose at `t` (interpolated between keys as your reader does; looping clips wrap) onto the
   body the clip was measured on: the set file's `sources.<clip.src>.rest`. That gives the 36
   body points the fixture names (`points`), in mm, in the rig's frame (forward, right, up).
   Shardfall's decoder (`crates/pav_tools/src/mocap/readable.rs`, `decode_pose`) is a port of
   my-3D2dge's `src/mocap/readable.js`, so the numbers should agree closely.
4. **The rule (fixed when S4 started):** per clip, the mean distance between your points and the
   fixture's over all samples and points is at most **1 mm**, and the worst at most **5 mm**
   (`fit_matches` and `tolerance_mm` in the fixture). Report per clip: mean and worst mm, pass or
   fail; for a failure, the first point and sample that differ most and your best guess why
   (axis order, sign, degree conversion, interpolation, rest frame).
5. **Play them.** Load the ten clips into your clip library through the same path as the 8 sets,
   so they appear wherever your engine lists clips, with their credit lines.
6. Add a test that holds the comparison (so a later pin move re-checks it), commit, and open a PR
   in my-3dge following its own conventions.

**Report back** (paste into a Shardfall session, which records it in its `docs/HISTORY.md`): the
Shardfall commit you pinned, the ten clips' mean/worst mm and pass/fail, how many passed, what you
changed in the reader, and the PR link.

Rules: write only to my-3dge. Don't clone Shardfall; fetch the named files only. Credit: the
clips are CC0 1.0 (Mesh2Motion's contributors; Quaternius, Universal Animation Library).
