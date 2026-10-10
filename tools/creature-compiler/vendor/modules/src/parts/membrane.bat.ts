import { colorRef, type DigitChain, definePart, type Spar } from '@spawnforge/core';
import type { Vector3 } from 'three';
import { z } from 'zod';

const params = z.strictObject({
  fingers: z.number().int().min(3).max(5).default(4).describe('Finger bones spanning the membrane'),
  span: z
    .number()
    .min(0.5)
    .max(2)
    .default(1)
    .describe('Finger length relative to the arm: larger spreads a bigger wing'),
  trailing: z
    .enum(['leg', 'body'])
    .default('leg')
    .describe('Where the trailing edge runs: to the nearest leg behind the wing, or to the body'),
  scallop: z
    .number()
    .min(0)
    .max(1)
    .default(0.4)
    .describe('How deeply the edge dips between fingers'),
  color: colorRef('base').describe('Membrane colour: a palette name or a colour'),
  translucency: z.number().min(0).max(1).default(0.4).describe('How much light shows through'),
});
type Params = z.output<typeof params>;

/** Finger lengths, leading first, as shares of span × arm length. */
const LENGTHS = [0.72, 0.95, 0.88, 0.78, 0.68];

export default definePart({
  id: 'membrane.bat',
  summary:
    'Leathery skin stretched between long finger bones, the body and the hind leg: bat and dragon wings.',
  tags: ['wing', 'membrane'],
  slot: 'membrane',
  material: 'skin',
  attach: { on: 'limb' },
  provides: ['glide'],
  params,
  example: { type: 'membrane.bat', fingers: 4, span: 1.2 },
  describe: (_, { count }) => (count === 1 ? 'a leathery wing' : 'leathery wings'),
  hooks: {
    // Fingers fan back from the wrist in the wing's plane: the first continues the hand, the
    // rest root at the wrist on metacarpals of their own (docs/design/9.3-wings-fins.md).
    digits(ctx, raw) {
      const p = raw as Params;
      const angleOf = (v: Vector3) => Math.atan2(v.dot(ctx.lead), v.dot(ctx.out));
      const dir = (a: number) =>
        ctx.out.clone().multiplyScalar(Math.cos(a)).addScaledVector(ctx.lead, Math.sin(a));
      const hand = angleOf(ctx.hand);
      const fan = (100 * Math.PI) / 180;
      const chains: DigitChain[] = [];
      for (let i = 0; i < p.fingers; i++) {
        const leading = i === 0;
        const length = (LENGTHS[i] ?? 0.6) * p.span * ctx.armLength;
        const shares = leading ? [0.55, 0.45] : [0.4, 0.33, 0.27];
        const a = leading ? hand - 0.05 : hand - (fan * i) / (p.fingers - 1);
        // Each finger bows a little toward the trailing edge along its length.
        const start = leading ? ctx.tip.clone() : ctx.wrist.clone();
        const points = [start];
        const radii = [ctx.tipRadius * (leading ? 0.55 : 0.5)];
        shares.forEach((share, k) => {
          const d = dir(a - 0.06 * k);
          points.push((points[k] as Vector3).clone().addScaledVector(d, share * length));
          radii.push(ctx.tipRadius * (0.45 - (0.3 * (k + 1)) / shares.length));
        });
        chains.push({ points, radii, root: leading ? 'tip' : 'wrist' });
      }
      return chains;
    },
    build(ctx, raw) {
      const p = raw as Params;
      const wing = ctx.wing;
      if (!wing) return;
      const look = {
        color: p.color,
        translucency: p.translucency,
        roughness: 0.65,
        scallop: p.scallop,
      };
      // Between the fingers, leading to trailing.
      for (let i = 0; i + 1 < wing.digits.length; i++)
        ctx.panel(wing.digits[i] as Spar, wing.digits[i + 1] as Spar, {
          ...look,
          rows: 14,
          cols: 6,
        });
      // From the arm and the last finger to the body, and the leg behind when it trails there.
      const last = wing.digits.at(-1);
      const toward = p.trailing === 'leg' ? wing.flank : wing.body;
      const [humerus, forearm] = wing.arm.bones;
      if (toward && humerus !== undefined && forearm !== undefined) {
        const armToWrist: Spar = {
          points: [
            wing.arm.points[0] as Vector3,
            wing.arm.points[1] as Vector3,
            wing.arm.points[2] as Vector3,
          ],
          bones: [humerus, forearm],
        };
        const outer: Spar = last
          ? {
              points: [...armToWrist.points, ...last.points.slice(1)],
              bones: [...armToWrist.bones, ...last.bones],
            }
          : armToWrist;
        ctx.panel(outer, toward, { ...look, rows: 18, cols: 8 });
      }
      // A narrow leading strip in front of the arm, shoulder to wrist.
      if (humerus !== undefined && forearm !== undefined) {
        const forearmLength = (wing.arm.points[1] as Vector3).distanceTo(
          wing.arm.points[2] as Vector3,
        );
        const points = wing.arm.points.slice(0, 3) as Vector3[];
        const front: Spar = {
          points: points.map((q, k) =>
            q
              .clone()
              .addScaledVector(wing.lead, k === 1 ? 0.12 * forearmLength : 0.02 * forearmLength),
          ),
          bones: [humerus, forearm],
        };
        ctx.panel({ points, bones: [humerus, forearm] }, front, {
          ...look,
          scallop: 0,
          rows: 8,
          cols: 2,
        });
      }
    },
  },
});
