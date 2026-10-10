import { colorRef, definePart } from '@spawnforge/core';
import { Vector3 } from 'three';
import { z } from 'zod';
import { sheared, Y } from './_feet.ts';

const params = z.strictObject({
  cloven: z.boolean().default(false).describe('Split into two toes, like a goat or a boar'),
  size: z.number().min(0.5).max(2).default(1).describe('Hoof size relative to the leg tip'),
  height: z.number().min(0.2).max(2).default(1).describe('How tall the hoof wall is'),
  color: colorRef('#2a221c').describe('Hoof colour: a palette name or a colour'),
});
type Params = z.output<typeof params>;

/** The hoof wall's height in tip radii (the pastern above it is 0.55). */
const wall = (p: Params) => 1.05 * p.height;

export default definePart({
  id: 'foot.hoof',
  summary: 'A hoof, single (a horse) or cloven (a goat or boar); set it as a leg\'s "foot".',
  tags: ['foot', 'hoof'],
  slot: 'foot',
  material: 'horn',
  stance: 'unguligrade',
  attach: { on: 'limb' },
  params,
  example: { type: 'foot.hoof', cloven: true },
  describe: (p, { count }) =>
    `${count === 1 ? 'a ' : ''}${p.cloven ? 'cloven ' : ''}hoof${count === 1 ? '' : 's'}`,
  hooks: {
    // The leg stands on the tip of its hoof: the ankle sits a pastern and a hoof up.
    footHeight: (ctx, raw) => {
      const p = raw as Params;
      return ctx.tipRadius * p.size * (0.55 + wall(p));
    },
    claws: () => ({ count: 0, length: 0 }),
    toes(ctx, raw) {
      const p = raw as Params;
      const r = ctx.tipRadius;
      // One upright toe (two side by side when cloven), so the hoof stays flat while the leg
      // above it swings.
      const across = new Vector3().crossVectors(ctx.forward, Y).normalize();
      const sides = p.cloven ? [-1, 1] : [0];
      return sides.map((side) => {
        // Exactly upright, so the hoof's socket faces the way the creature does.
        const top = ctx.ankle.clone().addScaledVector(across, side * 0.3 * r * p.size);
        const sole = top.clone();
        sole.y = ctx.groundY;
        // A slim pastern, which the hoof's wall then stands well outside of.
        const thick = p.cloven ? 0.5 : 0.7;
        return { points: [top, sole], radii: [r * thick, r * p.size * thick * 0.75] };
      });
    },
    build(ctx, raw) {
      const p = raw as Params;
      const color = ctx.color(p.color, '#2a221c');
      for (const toe of ctx.toes) {
        // The wall: wider at the sole, its front sloping back toward the top. Built standing on
        // +Y with the front toward +Z, then turned so +Y runs down the toe into the ground.
        // Well outside the pastern's skin (`toe.radius` is the toe's radius at the sole).
        const base = toe.radius * (p.cloven ? 1.7 : 1.9);
        const height = Math.min(toe.length * 0.9, (toe.length * wall(p)) / (0.55 + wall(p)));
        const piece = ctx.geo.lathe(
          [
            [0, 0],
            [base, 0],
            [base * 1.02, height * 0.1],
            [base * 0.8, height],
            [0, height],
          ],
          14,
        );
        const standing = sheared(piece, -0.38);
        ctx.emit(ctx.geo.rotate(standing, new Vector3(0, 0, 1), 180), toe, {
          color,
          tipColor: color,
          bone: toe.bone,
        });
      }
    },
  },
});
