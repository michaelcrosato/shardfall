import {
  colorRef,
  definePart,
  type MeshPiece,
  type PartBuildContext,
  type PartChain,
  type ScatterPoint,
} from '@spawnforge/core';
import { Vector3 } from 'three';
import { z } from 'zod';
import { mainBone } from './_fan.ts';

const params = z.strictObject({
  density: z.number().min(0).max(1).default(0.5).describe('How closely the quills grow'),
  length: z.number().min(0.02).max(1).default(0.2).describe('Quill length in torso lengths'),
  lie: z.number().min(0).max(90).default(60).describe('Degrees the quills lie back at rest'),
  color: colorRef('#e8e0d0').describe('Quill colour: a palette name or a colour'),
  tipColor: colorRef('#2a2018').describe('Colour at the tips'),
});
type Params = z.output<typeof params>;

/**
 * Quills share bones in groups: this many along the area by this many round it. A group turns
 * about an axis across the body, so a quill away from its group's middle along the body moves
 * with the turn; thin bands keep that under a centimetre or two.
 */
const ALONG = 24;
const ROUND = 5;
/** How upright display stands them: this many degrees short of straight up. */
const STAND = 12;

/** Where the quills grow: spaced by density, at most 400. */
function pointsOf(ctx: PartBuildContext, p: Params): ScatterPoint[] {
  const spacing = ctx.scale * p.length * (0.5 - 0.38 * p.density);
  return ctx.scatter(Math.max(0.004 * ctx.scale, spacing), { count: 400 });
}

/** Each point's group, by where it is in the area, and the groups that have quills. */
function groupsOf(ctx: PartBuildContext, points: readonly ScatterPoint[]) {
  const band = ctx.area ?? { from: 0, to: 1, angles: [0, 180] as const };
  const hi = band.angles[1];
  const of = points.map((q) => {
    const a = Math.min(
      ALONG - 1,
      Math.floor(((q.at - band.from) / Math.max(1e-6, band.to - band.from)) * ALONG),
    );
    const r = Math.min(ROUND - 1, Math.floor(((q.angle + hi) / (2 * hi)) * ROUND));
    return Math.max(0, a) * ROUND + Math.max(0, r);
  });
  const used = [...new Set(of)].sort((a, b) => a - b);
  return { of, used };
}

/**
 * A group's bone: standing out of the skin at the quill nearest its middle, with its X across
 * the body, so turning about X tips its quills from lying back toward upright.
 */
function groupChain(members: readonly ScatterPoint[], p: Params, scale: number): PartChain {
  const middle = new Vector3();
  for (const q of members) middle.add(q.position);
  middle.divideScalar(members.length);
  let near = members[0] as ScatterPoint;
  for (const q of members)
    if (q.position.distanceTo(middle) < near.position.distanceTo(middle)) near = q;
  const forward = near.forward
    .clone()
    .addScaledVector(near.normal, -near.forward.dot(near.normal))
    .normalize();
  return {
    points: [
      near.position.clone(),
      near.position.clone().addScaledVector(near.normal, 0.3 * p.length * scale),
    ],
    parent: mainBone(near),
    up: forward,
    radii: [0.01 * scale, 0.005 * scale],
    drive: 'flare',
    pose: [(Math.max(0, p.lie - STAND) * Math.PI) / 180],
  };
}

export default definePart({
  id: 'quills',
  summary:
    'Long sharp quills scattered over an area of the body, raised in display, like a porcupine.',
  tags: ['back', 'display', 'weapon'],
  slot: 'area',
  material: 'horn',
  attach: { on: 'spine', area: 'back', from: 0.3, to: 0.9 },
  provides: ['display'],
  params,
  example: {
    id: 'quills',
    type: 'quills',
    attach: { on: 'spine', area: 'back', from: 0.3, to: 0.9 },
    params: { length: 0.25 },
  },
  describe: () => 'quills',
  hooks: {
    // Groups of quills on flare-driven bones (docs/design/9.5-coverings.md).
    bones(ctx, raw) {
      const p = raw as Params;
      const points = pointsOf(ctx, p);
      const { of, used } = groupsOf(ctx, points);
      return used.map((g) =>
        groupChain(
          points.filter((_, i) => of[i] === g),
          p,
          ctx.scale,
        ),
      );
    },
    build(ctx, raw) {
      const p = raw as Params;
      const points = pointsOf(ctx, p);
      const { of, used } = groupsOf(ctx, points);
      const color = ctx.color(p.color, '#e8e0d0');
      const tipColor = ctx.color(p.tipColor, '#2a2018');
      const length = p.length * ctx.scale;
      const sides = Math.max(3, Math.round(4 * ctx.detail));
      const parts: { piece: MeshPiece; weights: [number, number][][]; tip: boolean }[] = [];
      points.forEach((q, i) => {
        const bone = ctx.chains[used.indexOf(of[i] as number)]?.bones[0];
        if (bone === undefined) return;
        // Lying back toward the tail, a little off true at random.
        const lie = ((p.lie + (ctx.rng.next() - 0.5) * 16) * Math.PI) / 180;
        const back = q.forward
          .clone()
          .addScaledVector(q.normal, -q.forward.dot(q.normal))
          .normalize()
          .negate();
        const dir = q.normal
          .clone()
          .multiplyScalar(Math.cos(lie))
          .addScaledVector(back, Math.sin(lie))
          .addScaledVector(q.side, (ctx.rng.next() - 0.5) * 0.25)
          .normalize();
        const l = length * (0.8 + 0.4 * ctx.rng.next());
        const r = Math.max(0.0015 * ctx.scale, 0.025 * l);
        const base = q.position.clone().addScaledVector(q.normal, -0.5 * r);
        const at = (t: number) => base.clone().addScaledVector(dir, l * t);
        // The shaft, then the dark tip: two pieces, each on the same weights by t.
        for (const [t0, t1, tip] of [
          [0, 0.75, false],
          [0.75, 1, true],
        ] as const) {
          const piece = ctx.geo.sweep(
            [at(t0), at((t0 + t1) / 2), at(t1)],
            (u) => r * (1 - 0.9 * (t0 + (t1 - t0) * u) ** 2),
            { sides, ...(tip ? { tip: 'point' as const } : {}) },
          );
          // Rigid on its group's bone: a quill stays straight as it rises.
          const weights = piece.t.map((): [number, number][] => [[bone, 1]]);
          parts.push({ piece, weights, tip });
        }
      });
      for (const tip of [false, true]) {
        const positions: Vector3[] = [];
        const normals: Vector3[] = [];
        const indices: number[] = [];
        const weights: [number, number][][] = [];
        for (const part of parts) {
          if (part.tip !== tip) continue;
          const offset = positions.length;
          for (let v = 0; v < part.piece.positions.length / 3; v++) {
            positions.push(new Vector3().fromArray(part.piece.positions, v * 3));
            normals.push(new Vector3().fromArray(part.piece.normals, v * 3));
            weights.push(part.weights[v] as [number, number][]);
          }
          for (const i of part.piece.indices) indices.push(i + offset);
        }
        if (positions.length > 0)
          ctx.solid(positions, normals, indices, weights, {
            color: tip ? tipColor : color,
            roughness: 0.4,
          });
      }
      ctx.measure(length, points.length);
    },
  },
});
