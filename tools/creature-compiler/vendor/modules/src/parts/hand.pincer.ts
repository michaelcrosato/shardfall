import { colorRef, definePart, merge, type PartBuildContext } from '@spawnforge/core';
import { Vector3 } from 'three';
import { z } from 'zod';
import { solidOn } from './_solid.ts';

const params = z.strictObject({
  size: z.number().min(0.03).max(0.8).default(0.2).describe('Pincer length in torso lengths'),
  width: z.number().min(0.2).max(1.5).default(0.6).describe('How bulky the claw is'),
  teeth: z.number().int().min(0).max(12).default(4).describe('Serrations along the inner edge'),
  color: colorRef('base').describe('Claw colour: a palette name or a colour'),
  tipColor: colorRef('#1e1a16').describe('Colour at the finger tips'),
});
type Params = z.output<typeof params>;

/** How far the moving finger stands open at rest, and turns to shut (radians). */
const OPEN = 0.55;

/** The claw's frame at the limb's tip: its axis point, out along the limb, its up and size. */
function frameOf(ctx: PartBuildContext, p: Params) {
  const s = ctx.socket(1, 0);
  const along = s.forward.clone().negate().normalize();
  const up = s.normal.clone().addScaledVector(along, -s.normal.dot(along)).normalize();
  const side = new Vector3().crossVectors(up, along).normalize();
  const centre = s.position.clone().addScaledVector(s.normal, -s.radius);
  const size = p.size * ctx.scale;
  const bulk = 0.3 * size * p.width;
  const palmEnd = centre.clone().addScaledVector(along, 0.45 * size);
  // The moving finger is hinged on top of the palm's end and rests open, up off the fixed one.
  const hinge = palmEnd.clone().addScaledVector(up, 0.35 * bulk);
  const open = along.clone().multiplyScalar(Math.cos(OPEN)).addScaledVector(up, Math.sin(OPEN));
  const tip = hinge.clone().addScaledVector(open, 0.6 * size);
  let limbBone = s.weights[0]?.[0] ?? 0;
  let most = -1;
  for (const [b, w] of s.weights)
    if (w > most) {
      most = w;
      limbBone = b;
    }
  return { centre, along, up, side, size, bulk, palmEnd, hinge, tip, limbBone };
}

/**
 * A curved, tapering finger from `from` along `dir`, bending toward `bend`, teeth toward `bite`:
 * its base in the claw's colour and its last third (`tip`) apart, for the dark tip.
 */
function finger(
  ctx: PartBuildContext,
  from: Vector3,
  dir: Vector3,
  bend: Vector3,
  bite: Vector3,
  length: number,
  thick: number,
  teeth: number,
) {
  const steps = 10;
  const points = [from.clone()];
  for (let k = 0; k < steps; k++) {
    const u = (k + 0.5) / steps;
    const d = dir
      .clone()
      .addScaledVector(bend, 0.6 * u * u)
      .normalize();
    points.push((points[k] as Vector3).clone().addScaledVector(d, length / steps));
  }
  const radius = (t: number) => thick * (1 - 0.9 * t ** 1.3);
  const sides = Math.max(6, Math.round(10 * ctx.detail));
  const split = Math.round(0.65 * steps);
  const base = ctx.geo.sweep(points.slice(0, split + 1), (t) => radius((t * split) / steps), {
    sides,
  });
  const tip = ctx.geo.sweep(
    points.slice(split),
    (t) => radius((split + t * (steps - split)) / steps),
    {
      sides,
      tip: 'point',
    },
  );
  for (let k = 0; k < teeth; k++) {
    const t = 0.15 + (0.6 * k) / Math.max(1, teeth - 1);
    const i = Math.min(steps - 1, Math.round(t * steps));
    const at = points[i] as Vector3;
    merge(
      i < split ? base : tip,
      ctx.geo.sweep(
        [at, at.clone().addScaledVector(bite, radius(t) + 0.2 * thick)],
        (u) => radius(t) * 0.3 * (1 - u),
        { sides: 4, tip: 'point' },
      ),
    );
  }
  return { base, tip };
}

export default definePart({
  id: 'hand.pincer',
  summary: 'A crab or scorpion pincer: a heavy claw with one hinged finger that snaps shut.',
  tags: ['hand', 'weapon', 'chitin'],
  slot: 'foot',
  material: 'chitin',
  attach: { on: 'limb' },
  provides: ['pincer'],
  params,
  example: { type: 'hand.pincer', size: 0.25 },
  describe: (_, { count }) => (count === 1 ? 'a pincer' : 'pincers'),
  hooks: {
    // The moving finger on a bone of its own, which the grip shuts
    // (docs/design/9.4-tentacles-parts.md).
    bones(ctx, raw) {
      const f = frameOf(ctx, raw as Params);
      const along = new Vector3().subVectors(f.tip, f.hinge).normalize();
      return [
        {
          points: [f.hinge, f.tip],
          parent: f.limbBone,
          // Its `up` is the claw's: a turn about the bone's X (the claw's side) by the negative
          // pose shuts it onto the fixed finger below.
          up: new Vector3().crossVectors(along, f.side).normalize(),
          radii: [0.6 * f.bulk, 0.1 * f.bulk],
          drive: 'grip',
          pose: [-OPEN],
        },
      ];
    },
    build(ctx, raw) {
      const p = raw as Params;
      const f = frameOf(ctx, p);
      const color = ctx.color(p.color, '#5a3a24');
      const tipColor = ctx.color(p.tipColor, '#1e1a16');
      // The palm: a smooth swollen claw from the limb's tip.
      const palmPoints: Vector3[] = [];
      for (let k = 0; k <= 8; k++)
        palmPoints.push(
          f.centre.clone().addScaledVector(f.along, (-0.06 + (0.51 * k) / 8) * f.size),
        );
      const palm = ctx.geo.sweep(
        palmPoints,
        (t) => f.bulk * (0.55 + 0.45 * Math.sin(Math.PI * Math.min(1, 0.15 + 0.85 * t)) ** 0.6),
        { sides: Math.max(10, Math.round(14 * ctx.detail)), tip: 'round', cross: [1, 0.8] },
      );
      solidOn(ctx, palm, { color, roughness: 0.35 }, { bone: f.limbBone });
      // The fixed finger, below, and the moving one on its bone, above.
      const down = f.up.clone().negate();
      const fixed = finger(
        ctx,
        f.palmEnd.clone().addScaledVector(down, 0.3 * f.bulk),
        f.along,
        f.up.clone().multiplyScalar(0.3),
        f.up,
        0.6 * f.size,
        0.5 * f.bulk,
        p.teeth,
      );
      solidOn(ctx, fixed.base, { color, roughness: 0.35 }, { bone: f.limbBone });
      solidOn(ctx, fixed.tip, { color: tipColor, roughness: 0.3 }, { bone: f.limbBone });
      const bone = ctx.chains[0]?.bones[0];
      if (bone !== undefined) {
        const dir = new Vector3().subVectors(f.tip, f.hinge).normalize();
        const moving = finger(
          ctx,
          f.hinge,
          dir,
          down.clone().multiplyScalar(0.4),
          down,
          0.6 * f.size,
          0.42 * f.bulk,
          p.teeth,
        );
        solidOn(ctx, moving.base, { color, roughness: 0.35 }, { bone });
        solidOn(ctx, moving.tip, { color: tipColor, roughness: 0.3 }, { bone });
      }
      ctx.measure(f.size, 1);
    },
  },
});
