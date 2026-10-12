# Asset plan reference material

Planning artefacts for `docs/ASSET_SYSTEM_PLAN.md` (§14 there says which work item uses each).
They are starting points, not product code: the Rust implementation in the plan is the single
source of truth. Delete each file when the work item that uses it lands.

- `examples/format2/`: five format-2 assets and three semantic probes (the conformance corpus,
  §5.9). Already follow the plan's final rules.
- `examples/materials/`: example materials in the house unit (cm), including `pb/` presets.
- `examples/families/fence.f2.json`: a parametric family (§9.4).
- `prototypes/f2expand.py`: independent stdlib reference expander, format 2 → format 1
  (`python3 -I f2expand.py IN.f2.json --resolved OUT.json > OUT.v1.json`).
- `prototypes/materials/`: WGSL kernel library, diffs against the renderer at `a9ca003`, and the
  parity/bench harness with the CPU mirror.
- `prototypes/material_fit/`, `prototypes/import/`: numpy prototypes of the texture and mesh fits.
- `prototypes/lint4.py`, `prototypes/tools/`: lint rules and a field-guide draft.
- `prototypes/families/apply_params.py`: parameter application for families.
- `prototypes/bench/`: benchmark wrapper, token counters, agent prompts, the arm-A replay.
- `fixtures/`: the 23 error probes. The two format-1 dogfood assets used as baselines are in
  `crates/pav_core/tests/fixtures/props/`.

Paths inside the scripts that pointed at the planning scratch folder read `<scratch>`.
