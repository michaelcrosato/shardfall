import { colorRef, definePart } from '@spawnforge/core';
import { Vector3 } from 'three';
import { z } from 'zod';
import { onGround, scaled, toeCentre, turned, Y } from './_feet.ts';

const params = z.strictObject({
  toes: z.number().int().min(3).max(5).default(4).describe('Toes on the ground'),
  size: z.number().min(0.5).max(2).default(1).describe('Paw size relative to the leg tip'),
  claws: z
    .enum(['hidden', 'short', 'long'])
    .default('short')
    .describe('"hidden" like a cat at rest, "short" like a dog, "long" like a bear'),
  padColor: colorRef('#3a2e2a').describe('Colour of the pads: a palette name or a colour'),
  clawColor: colorRef('#2a221c').describe('Claw colour'),
});
type Params = z.output<typeof params>;

/** Claw length per setting, in tip radii, and in torso lengths for stats. */
const CLAW = { hidden: 0, short: 0.45, long: 0.95 } as const;
const CLAW_L = { hidden: 0, short: 0.02, long: 0.045 } as const;

export default definePart({
  id: 'foot.paw',
  summary: 'A padded paw with toes and visible or hidden claws, like a dog or a big cat.',
  tags: ['foot', 'paw'],
  slot: 'foot',
  material: 'skin',
  stance: 'digitigrade',
  attach: { on: 'limb' },
  params,
  example: { type: 'foot.paw', toes: 4, claws: 'short' },
  describe: (_, { count }) => (count === 1 ? 'a paw' : 'paws'),
  hooks: {
    // The leg stands on its toes: the ankle sits a paw's height up.
    footHeight: (ctx, raw) => 1.5 * ctx.tipRadius * (raw as Params).size,
    claws: (raw) => {
      const p = raw as Params;
      return { count: p.claws === 'hidden' ? 0 : p.toes, length: CLAW_L[p.claws] };
    },
    toes(ctx, raw) {
      const p = raw as Params;
      const r = ctx.tipRadius * p.size;
      const rho = r * (p.toes >= 5 ? 0.4 : p.toes === 4 ? 0.46 : 0.52);
      const chains = [];
      for (let i = 0; i < p.toes; i++) {
        const offset = (i - (p.toes - 1) / 2) * 18;
        if (ctx.role === 'leg') {
          // Short thick toes slope from the raised ankle to knuckles on the ground.
          const dir = turned(toeCentre(ctx), offset);
          chains.push({
            points: [
              ctx.ankle.clone().addScaledVector(dir, 0.1 * r),
              onGround(ctx, dir, 0.95 * r, rho * 1.05),
              onGround(ctx, dir, 1.7 * r, rho * 0.85),
            ],
            radii: [rho * 1.2, rho * 1.1, rho * 0.9],
          });
        } else {
          // On an arm, short fingers continue it, curling forward.
          const across = new Vector3().crossVectors(ctx.limbDir, ctx.forward);
          const dir = ctx.limbDir
            .clone()
            .applyAxisAngle(across.lengthSq() > 1e-8 ? ctx.forward : Y, (offset * Math.PI) / 180);
          const knuckle = ctx.ankle.clone().addScaledVector(dir, 0.8 * r);
          const tip = knuckle
            .clone()
            .addScaledVector(dir.clone().addScaledVector(ctx.forward, 0.7).normalize(), 0.6 * r);
          chains.push({
            points: [ctx.ankle.clone(), knuckle, tip],
            radii: [rho * 1.15, rho * 1.05, rho * 0.85],
          });
        }
      }
      return chains;
    },
    build(ctx, raw) {
      const p = raw as Params;
      const pad = ctx.color(p.padColor, '#3a2e2a');
      const claw = ctx.color(p.clawColor, '#2a221c');
      const down = new Vector3(0, -1, 0);
      let palm: Vector3 | undefined;
      let reach = 0;
      for (const toe of ctx.toes) {
        const tip = toe.points.at(-1) as Vector3;
        const before = toe.points.at(-2) as Vector3;
        const along = new Vector3().subVectors(tip, before).normalize();
        const rho = toe.toeRadius;
        // A pad under each toe, near its tip.
        const centre = tip.clone().addScaledVector(along, -0.35 * rho);
        centre.y = Math.max(0.3 * rho, centre.y - 0.55 * rho);
        ctx.emit(
          scaled(ctx.geo.sphere(0.62 * rho, 6, 10), 1, 0.45, 1.15),
          ctx.frame(centre, down, along, toe.bone),
          { color: pad, bone: toe.bone },
        );
        if (CLAW[p.claws] > 0) {
          const length = CLAW[p.claws] * rho * 2.4;
          const path = ctx.geo.arc(length, p.claws === 'long' ? 75 : 45, {
            segments: 5,
            lean: 30,
          });
          const base = rho * 0.42;
          const piece = ctx.geo.sweep(path, (t) => base * (1 - t * 0.9), {
            sides: 6,
            tip: 'point',
          });
          ctx.emit(piece, toe, { color: claw, tipColor: '#120e0b', sink: base, bone: toe.bone });
        }
        palm = (palm ?? new Vector3()).add(toe.points[0] as Vector3);
        reach = Math.max(reach, rho);
      }
      // The palm pad behind the toes, under the ankle; it lifts with the heel. A paw on an arm,
      // whose toes do not rest on the ground, has none.
      const first = ctx.toes[0];
      const grounded = ctx.toes.every((t) => (t.points.at(-1) as Vector3).y < 2 * t.toeRadius);
      if (!first || !palm || !grounded) return;
      palm.divideScalar(ctx.toes.length);
      const ahead = new Vector3()
        .subVectors(first.points.at(-1) as Vector3, first.points[0] as Vector3)
        .setY(0)
        .normalize();
      const centre = palm.clone().addScaledVector(ahead, 0.2 * reach);
      centre.y = 0.5 * reach;
      ctx.emit(
        scaled(ctx.geo.sphere(1.6 * reach, 6, 12), 1.15, 0.4, 1),
        ctx.frame(centre, down, ahead, first.limbBone),
        { color: pad, bone: first.limbBone },
      );
    },
  },
});
