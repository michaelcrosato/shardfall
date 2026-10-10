import { colorRef, definePart } from '@spawnforge/core';
import { Vector3 } from 'three';
import { z } from 'zod';
import { scaled, Y } from './_feet.ts';

const params = z.strictObject({
  width: z.number().min(0.5).max(2.5).default(1.3).describe('Foot width relative to the leg tip'),
  nails: z.number().int().min(0).max(5).default(4).describe('Blunt nails around the front'),
  nailColor: colorRef('#d8ccb0').describe('Nail colour: a palette name or a colour'),
});
type Params = z.output<typeof params>;

export default definePart({
  id: 'foot.pad',
  summary: 'A broad column foot with blunt nails, for heavy creatures like elephants and trolls.',
  tags: ['foot', 'heavy'],
  slot: 'foot',
  material: 'skin',
  stance: 'plantigrade',
  attach: { on: 'limb' },
  params,
  example: { type: 'foot.pad', width: 1.5, nails: 3 },
  describe: (_, { count }) => (count === 1 ? 'a padded foot' : 'broad padded feet'),
  hooks: {
    // The column foot is about as tall as it is wide.
    footHeight: (ctx, raw) => 1.15 * ctx.tipRadius * (raw as Params).width,
    claws: () => ({ count: 0, length: 0 }),
    toes(ctx, raw) {
      const p = raw as Params;
      const r = ctx.tipRadius;
      if (ctx.role !== 'leg') {
        // On an arm, a broad stump of a hand.
        const end = ctx.ankle.clone().addScaledVector(ctx.limbDir, 0.8 * r * p.width);
        return [{ points: [ctx.ankle.clone(), end], radii: [r, r * p.width * 0.9] }];
      }
      // One upright toe as wide as the foot, so the sole stays flat while the leg swings; its
      // rounded end rests on the ground.
      const bottom = r * p.width * 0.98;
      const sole = ctx.ankle.clone();
      sole.y = ctx.groundY + bottom * 0.9;
      return [{ points: [ctx.ankle.clone(), sole], radii: [r * 0.95, bottom] }];
    },
    build(ctx, raw) {
      const p = raw as Params;
      const toe = ctx.toes[0];
      if (!toe || p.nails === 0) return;
      const color = ctx.color(p.nailColor, '#d8ccb0');
      const sole = toe.points.at(-1) as Vector3;
      const top = toe.points[0] as Vector3;
      const down = new Vector3().subVectors(sole, top).normalize();
      // The way the foot faces: the creature's forward, off the toe's line.
      const ahead = new Vector3(0, 0, 1).addScaledVector(down, -down.z).normalize();
      const radius = toe.radius;
      for (let i = 0; i < p.nails; i++) {
        // Spread around the front of the sole, low on the foot.
        const deg = p.nails === 1 ? 0 : -65 + (130 * i) / (p.nails - 1);
        const out = ahead.clone().applyAxisAngle(down, (deg * Math.PI) / 180);
        // Low on the front of the foot, just above the ground.
        const at = sole.clone().addScaledVector(out, radius * 0.9);
        at.y = radius * 0.35;
        const nail = scaled(ctx.geo.sphere(radius * 0.22, 6, 10), 1.1, 0.45, 0.85);
        ctx.emit(nail, ctx.frame(at, out, Y, toe.bone, radius), { color, bone: toe.bone });
      }
    },
  },
});
