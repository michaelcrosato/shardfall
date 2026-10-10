import { colorRef, definePart } from '@spawnforge/core';
import { Vector3 } from 'three';
import { z } from 'zod';
import { onGround, scaled, toeCentre, turned } from './_feet.ts';

const params = z.strictObject({
  fingers: z.number().int().min(2).max(5).default(4).describe('Fingers beside the thumb'),
  fingerLength: z
    .number()
    .min(0.02)
    .max(0.5)
    .default(0.1)
    .describe('Finger length in torso lengths'),
  claws: z
    .number()
    .min(0)
    .max(0.2)
    .default(0.01)
    .describe('Claw length in torso lengths; 0 for nails'),
  clawColor: colorRef('#2a221c').describe('Claw or nail colour: a palette name or a colour'),
});
type Params = z.output<typeof params>;

/** Bones of a finger as shares of its length, and how far each bends toward the palm (degrees). */
const PHALANGES = [0.45, 0.32, 0.23];
const CURL = [14, 24, 28];

export default definePart({
  id: 'hand.grasp',
  summary: 'A grasping hand: fingers and an opposed thumb, with nails or claws; set it as "foot".',
  tags: ['hand'],
  slot: 'foot',
  material: 'skin',
  attach: { on: 'limb' },
  provides: ['hand'],
  params,
  example: { type: 'hand.grasp', fingers: 4, claws: 0.03 },
  describe: (_, { count }) => (count === 1 ? 'a grasping hand' : 'grasping hands'),
  hooks: {
    claws: (raw) => {
      const p = raw as Params;
      return { count: p.claws > 0 ? p.fingers + 1 : 0, length: p.claws };
    },
    toes(ctx, raw) {
      const p = raw as Params;
      const length = p.fingerLength * ctx.scale;
      const rho = Math.min(0.3 * ctx.tipRadius, 0.13 * length) * (4 / p.fingers) ** 0.25;
      const radii = [rho * 1.15, rho, rho * 0.9, rho * 0.75];
      const inward = ctx.outward.clone().negate();
      if (ctx.role === 'leg') {
        // A grasping foot (an ape's): long toes laid forward, the big toe turned inward.
        const centre = toeCentre(ctx);
        const toes = Array.from({ length: p.fingers }, (_, i) => {
          const dir = turned(centre, (i - (p.fingers - 1) / 2) * 12);
          return {
            points: [0.1, 0.45, 0.78, 1].map((f, k) =>
              k === 0
                ? ctx.ankle.clone().addScaledVector(dir, f * ctx.tipRadius)
                : onGround(ctx, dir, ctx.tipRadius * 0.5 + f * length, rho * (1.05 - 0.15 * k)),
            ),
            radii,
          };
        });
        const big = centre.clone().addScaledVector(inward, 0.9).normalize();
        toes.push({
          points: [0.1, 0.5, 1].map((f, k) =>
            k === 0
              ? ctx.ankle.clone().addScaledVector(big, f * ctx.tipRadius)
              : onGround(ctx, big, ctx.tipRadius * 0.3 + f * length * 0.7, rho * 1.1),
          ),
          radii: [rho * 1.3, rho * 1.15, rho * 0.9],
        });
        return toes;
      }
      // On an arm: fingers continue the hand, side by side across the palm, curling forward; the
      // thumb comes off the inner side and turns toward them.
      const down = ctx.limbDir.clone();
      let across = new Vector3().crossVectors(down, ctx.forward);
      if (across.lengthSq() < 1e-8) across = ctx.outward.clone();
      across.normalize();
      const palm = new Vector3().crossVectors(across, down).normalize();
      const finger = (base: Vector3, dir: Vector3, scale: number, curl: readonly number[]) => {
        const points = [base.clone()];
        const d = dir.clone();
        PHALANGES.forEach((share, k) => {
          // Turning about `across` toward `palm` (= across × down) curls the finger forward.
          d.applyAxisAngle(across, ((curl[k] as number) * Math.PI) / 180);
          points.push((points[k] as Vector3).clone().addScaledVector(d, share * length * scale));
        });
        return points;
      };
      const chains = Array.from({ length: p.fingers }, (_, i) => {
        const offset = i - (p.fingers - 1) / 2;
        const base = ctx.ankle
          .clone()
          .addScaledVector(down, 0.2 * ctx.tipRadius)
          .addScaledVector(across, offset * 2.3 * rho);
        const dir = down.clone().applyAxisAngle(palm, (offset * 6 * Math.PI) / 180);
        return { points: finger(base, dir, 1 - Math.abs(offset) * 0.08, CURL), radii };
      });
      const thumbBase = ctx.ankle
        .clone()
        .addScaledVector(inward, 0.8 * ctx.tipRadius)
        .addScaledVector(down, 0.05 * ctx.tipRadius);
      const thumbDir = down
        .clone()
        .addScaledVector(palm, 0.8)
        .addScaledVector(inward, 0.3)
        .normalize();
      chains.push({
        points: finger(thumbBase, thumbDir, 0.75, [10, 15, 15]),
        radii: [rho * 1.3, rho * 1.15, rho, rho * 0.85],
      });
      return chains;
    },
    build(ctx, raw) {
      const p = raw as Params;
      const color = ctx.color(p.clawColor, '#2a221c');
      for (const toe of ctx.toes) {
        const rho = toe.toeRadius;
        if (p.claws > 0) {
          const length = p.claws * ctx.scale;
          const base = Math.max(rho * 0.75, length * 0.12);
          const path = ctx.geo.arc(length, 60, { segments: 6, lean: 25 });
          const piece = ctx.geo.sweep(path, (t) => base * (1 - t * 0.9), {
            sides: 6,
            tip: 'point',
          });
          ctx.emit(piece, toe, { color, tipColor: '#120e0b', sink: base * 0.7, bone: toe.bone });
        } else {
          // A flat nail on the back of the fingertip.
          const nail = scaled(ctx.geo.sphere(rho * 0.75, 5, 10), 0.85, 1.1, 0.3);
          ctx.emit(ctx.geo.translate(nail, 0, -0.35 * rho, 0.6 * rho), toe, {
            color,
            bone: toe.bone,
          });
        }
      }
    },
  },
});
