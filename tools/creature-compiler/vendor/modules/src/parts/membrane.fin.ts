import { colorRef, definePart } from '@spawnforge/core';
import type { Vector3 } from 'three';
import { z } from 'zod';
import { alongSpar, gridSheet } from './_sheet.ts';

const params = z.strictObject({
  rays: z.number().int().min(3).max(16).default(7).describe('Rays spanning the fin'),
  width: z.number().min(0.2).max(2).default(0.8).describe('Fin width relative to its length'),
  color: colorRef('base').describe('Fin colour: a palette name or a colour'),
  translucency: z.number().min(0).max(1).default(0.5).describe('How much light shows through'),
});
type Params = z.output<typeof params>;

export default definePart({
  id: 'membrane.fin',
  summary: 'A fin of thin rays with skin between them, on a fin limb (pectoral and pelvic fins).',
  tags: ['fin', 'membrane', 'aquatic'],
  slot: 'membrane',
  material: 'skin',
  attach: { on: 'limb' },
  params,
  example: { type: 'membrane.fin', rays: 8 },
  describe: (_, { count }) => (count === 1 ? 'a fin' : 'fins'),
  hooks: {
    build(ctx, raw) {
      const p = raw as Params;
      const wing = ctx.wing;
      if (!wing) return;
      // Rays fan back from the root through `width` × 90°, the first along the limb itself
      // (docs/design/9.3-wings-fins.md), each shorter than the one before; the skin between
      // them dips a little at the edge. Fins never fold, so the sheet rides the limb's bones.
      const root = wing.arm.points[0] as Vector3;
      const rays = p.rays;
      const angleOf = (v: Vector3) => Math.atan2(v.dot(wing.lead), v.dot(wing.out));
      // The last ray stays short of pointing back along the body, or it would turn into it.
      const tip = (wing.arm.points.at(-1) as Vector3).clone().sub(root);
      const fan = Math.max(0.2, Math.min(p.width * (Math.PI / 2), angleOf(tip) + 1.45));
      const rows = Math.max(4, Math.round(9 * ctx.detail));
      const cols = (rays - 1) * Math.max(1, Math.round(2 * ctx.detail)) + 1;
      const sheet = gridSheet(
        rows,
        cols,
        (s, f) => {
          const arm = alongSpar(wing.arm, s).point.sub(root);
          const a = angleOf(arm) - f * fan;
          const ray = f * (rays - 1);
          const dip = 1 - 0.08 * Math.sin(Math.PI * (ray - Math.floor(ray))) ** 2 * s ** 4;
          const r = arm.length() * (1 - 0.45 * f ** 1.3) * dip;
          return root
            .clone()
            .addScaledVector(wing.out, r * Math.cos(a))
            .addScaledVector(wing.lead, r * Math.sin(a));
        },
        (s) => alongSpar(wing.arm, s).weights,
      );
      // Rays fall on the shader's vein lines, six to a unit across.
      const across = sheet.across.map((f) => (f * (rays - 1)) / 6);
      ctx.sheet(sheet.positions, sheet.normals, sheet.indices, sheet.weights, sheet.along, across, {
        color: p.color,
        opacity: 1,
        translucency: p.translucency,
        roughness: 0.45,
        veins: 0.7,
      });
      ctx.measure(wing.armLength, rays);
    },
  },
});
