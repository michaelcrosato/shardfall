import { colorRef, definePart } from '@spawnforge/core';
import { Vector3 } from 'three';
import { z } from 'zod';
import { alongSpar, gridSheet } from './_sheet.ts';

const params = z.strictObject({
  dome: z.number().min(0).max(1).default(0.6).describe('How domed the case is'),
  ridges: z.number().int().min(0).max(12).default(3).describe('Ridges along the case'),
  color: colorRef('base').describe('Case colour: a palette name or a colour'),
  sheen: z.number().min(0).max(1).default(0.6).describe('How glossy the case is'),
});
type Params = z.output<typeof params>;

export default definePart({
  id: 'membrane.case',
  summary:
    "A beetle's hard wing case (elytron): a shell that covers the same-side wing behind it and lifts in flight.",
  tags: ['wing', 'shell', 'insect', 'chitin'],
  slot: 'membrane',
  material: 'chitin',
  attach: { on: 'limb' },
  provides: ['cover'],
  params,
  example: { type: 'membrane.case', dome: 0.7 },
  describe: (_, { count }) => (count === 1 ? 'a wing case' : 'wing cases'),
  hooks: {
    // Straight; folded flat on top of the wings behind it, its hinge along the midline.
    wing: () => ({
      bind: [-4, 0, 0],
      fold: { sweep: 92, droop: -3, lie: 90, joints: [0, 0, 0], digits: 0, flex: 0 },
      stack: 2,
      thickness: 0.03,
      shell: true,
      // In flight a case lifts and swings forward, clear of the wings beating under it.
      stroke: { amplitude: 0, flex: 0, twist: 0, plane: 0, hold: { lift: 35, forward: 20 } },
    }),
    build(ctx, raw) {
      const p = raw as Params;
      const wing = ctx.wing;
      if (!wing) return;
      // Shaped where it rests: from the hinge (its inner edge, near the midline) out over the
      // side and settled onto the body below, then carried back to the spread pose it is built
      // in (docs/design/9.3-wings-fins.md).
      const length = wing.armLength;
      const rows = Math.max(6, Math.round(18 * ctx.detail));
      const cols = Math.max(4, Math.round(9 * ctx.detail));
      const thick = 0.025 * length;
      const gap = 0.01 * length + thick;
      const down = new Vector3(0, -1, 0);
      const tipRound = (s: number) =>
        Math.sqrt(Math.max(0, 1 - Math.max(0, (s - 0.78) / 0.22) ** 2));
      // Each grid point once: where it rests (from the skin found below it) and how it is
      // carried back; the inner shell lies a plate's thickness under the outer one.
      const memo = new Map<
        string,
        { outer: Vector3; inner: Vector3; weights: [number, number][] }
      >();
      const place = (s: number, f: number) => {
        const key = `${s.toFixed(5)},${f.toFixed(5)}`;
        const known = memo.get(key);
        if (known) return known;
        const { point, weights } = alongSpar(wing.arm, s);
        const bone = (weights[0] as [number, number])[0];
        const hinge = ctx.toRest(point, bone);
        const out = ctx.toRest(point.clone().add(wing.lead), bone).sub(hinge).normalize();
        const up = ctx.toRest(point.clone().add(wing.normal), bone).sub(hinge).normalize();
        const width = (Math.abs(hinge.x) + 0.38 * length) * Math.min(1, 0.6 + 2 * s) * tipRound(s);
        const flat = hinge.clone().addScaledVector(out, f * width);
        const skin = ctx.skinAlong(
          flat.clone().addScaledVector(up, 0.5 * length),
          down,
          2 * length,
        );
        const settle = Math.min(1, f / 0.2);
        const target = skin
          ? flat
              .clone()
              .lerp(skin.clone().addScaledVector(up, gap), settle * settle * (3 - 2 * settle))
          : flat.addScaledVector(up, -p.dome * width * 0.5 * f * f);
        const ridge = p.ridges > 0 ? 0.004 * length * Math.cos(f * p.ridges * Math.PI) ** 8 : 0;
        target.addScaledVector(up, ridge);
        const placed = {
          outer: ctx.fromRest(target, bone),
          inner: ctx.fromRest(target.clone().addScaledVector(up, -thick), bone),
          weights,
        };
        memo.set(key, placed);
        return placed;
      };
      const shell = (inside: boolean) =>
        gridSheet(
          rows,
          cols,
          (s, f) => (inside ? place(s, f).inner : place(s, f).outer).clone(),
          (s, f) => place(s, f).weights,
        );
      const outer = shell(false);
      const inner = shell(true);
      const color = ctx.color(p.color, '#2b2420');
      const roughness = 0.55 - 0.45 * p.sheen;
      // The shell is hard: it goes in the parts mesh, rigid on its bone, the outer face up and
      // the inner one turned in. A grid winds the way its normals face, which a mirror reverses.
      for (const [sheet, inside] of [
        [outer, false],
        [inner, true],
      ] as const) {
        const facing = sheet.normals.reduce((sum, n) => sum + n.dot(wing.normal), 0);
        const flip = facing < 0 !== inside;
        if (flip) for (const n of sheet.normals) n.negate();
        const indices = flip
          ? sheet.indices.map((_, i, a) => a[i - (i % 3) + (2 - (i % 3))] as number)
          : sheet.indices;
        ctx.solid(sheet.positions, sheet.normals, indices, sheet.weights, { color, roughness });
      }
      ctx.measure(length, 1);
    },
  },
});
