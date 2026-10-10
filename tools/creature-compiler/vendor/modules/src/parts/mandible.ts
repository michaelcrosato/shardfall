import {
  colorRef,
  definePart,
  merge,
  type PartBuildContext,
  type PartChain,
  type Socket,
} from '@spawnforge/core';
import { Vector3 } from 'three';
import { z } from 'zod';
import { solidOn } from './_solid.ts';

const params = z.strictObject({
  length: z.number().min(0.02).max(0.6).default(0.15).describe('Mandible length in torso lengths'),
  curve: z.number().min(0).max(180).default(70).describe('Degrees each mandible curves inward'),
  teeth: z.number().int().min(0).max(8).default(2).describe('Teeth along the inner edge'),
  shape: z
    .enum(['mandible', 'fang'])
    .default('mandible')
    .describe('"mandible" like an ant, "fang" like a spider\'s chelicerae'),
  color: colorRef('#2a2018').describe('Colour: a palette name or a colour'),
  tipColor: colorRef('#100a06').describe('Colour at the tips'),
});
type Params = z.output<typeof params>;

/** How far the mandibles swing apart with the jaw wide open (radians). */
const OPEN = 0.6;
const STEPS = 10;

interface Blade {
  readonly side: 1 | -1;
  readonly points: Vector3[];
  /** Toward the midline, at each point, for teeth. */
  readonly inward: Vector3[];
  readonly hinge: Vector3;
}

/**
 * Each blade's path in model space: from the mouth corner, forward and curving in toward the
 * other (`mandible`), or hanging down and curving back (`fang`).
 */
function blades(socketAt: (side: 1 | -1) => Socket | undefined, p: Params, scale: number) {
  const out: Blade[] = [];
  const length = p.length * scale;
  for (const side of [1, -1] as const) {
    const s = socketAt(side);
    if (!s) return [];
    const forward = s.forward.clone().normalize();
    const outward = s.normal.clone().addScaledVector(forward, -s.normal.dot(forward)).normalize();
    // The head's up: forward × left.
    const left = outward.clone().multiplyScalar(side);
    const up = new Vector3().crossVectors(forward, left).normalize();
    const inward = outward.clone().negate();
    const curve = (p.curve * Math.PI) / 180;
    const points = [s.position.clone().addScaledVector(outward, 0.15 * length)];
    const inwards: Vector3[] = [];
    for (let k = 0; k < STEPS; k++) {
      const u = (k + 0.5) / STEPS;
      const dir =
        p.shape === 'fang'
          ? up
              .clone()
              .multiplyScalar(-Math.cos(0.4 - curve * u))
              .addScaledVector(forward, Math.sin(0.4 - curve * u))
              .addScaledVector(inward, 0.25)
          : forward
              .clone()
              .multiplyScalar(Math.cos(0.35 - curve * u))
              .addScaledVector(outward, Math.sin(0.35 - curve * u));
      dir.normalize();
      points.push((points[k] as Vector3).clone().addScaledVector(dir, length / STEPS));
      inwards.push(inward.clone());
    }
    inwards.push(inward.clone());
    // They swing apart about the head's up (mandibles) or its forward line (fangs).
    out.push({ side, points, inward: inwards, hinge: p.shape === 'fang' ? forward : up });
  }
  return out;
}

/** Each mandible's socket: at the mouth corner, just above the lip line, moving with the head. */
const cornerOf = (ctx: PartBuildContext) => (side: 1 | -1) =>
  ctx.around(0.82, 'upper', side > 0 ? 12 : 168);

export default definePart({
  id: 'mandible',
  summary: 'A pair of hinged mandibles or fangs at the mouth corners that close with the bite.',
  tags: ['head', 'mouth', 'insect', 'weapon'],
  slot: 'mouth',
  material: 'chitin',
  attach: { on: 'head' },
  provides: ['mandibles'],
  params,
  example: { id: 'mandibles', type: 'mandible', params: { length: 0.2 } },
  describe: (p) => (p.shape === 'fang' ? 'fangs' : 'mandibles'),
  hooks: {
    // One hinged bone each, driven by the jaw: open, they swing apart
    // (docs/design/9.4-tentacles-parts.md).
    bones(ctx, raw) {
      const p = raw as Params;
      return blades(cornerOf(ctx), p, ctx.scale).map((b) => {
        const root = b.points[0] as Vector3;
        const tip = b.points.at(-1) as Vector3;
        const along = new Vector3().subVectors(tip, root).normalize();
        return {
          points: [root, tip],
          up: new Vector3().crossVectors(b.hinge, along).normalize(),
          radii: [0.09 * p.length * ctx.scale, 0.02 * p.length * ctx.scale],
          drive: 'jaw',
          pose: [b.side * OPEN],
        } satisfies PartChain;
      });
    },
    build(ctx, raw) {
      const p = raw as Params;
      const length = p.length * ctx.scale;
      const color = ctx.color(p.color, '#2a2018');
      blades(cornerOf(ctx), p, ctx.scale).forEach((b, i) => {
        const bone = ctx.chains[i]?.bones[0];
        if (bone === undefined) return;
        const radius = (t: number) => length * 0.09 * (1 - 0.85 * t ** 1.3);
        const blade = ctx.geo.sweep(b.points, radius, {
          sides: Math.max(6, Math.round(9 * ctx.detail)),
          tip: 'point',
        });
        // Teeth on the inner edge.
        for (let k = 0; k < p.teeth; k++) {
          const t = 0.35 + (0.45 * k) / Math.max(1, p.teeth - 1 || 1);
          const i0 = Math.min(STEPS - 1, Math.floor(t * STEPS));
          const at = (b.points[i0] as Vector3).clone();
          const toward = (b.inward[i0] as Vector3).clone();
          const tooth = ctx.geo.sweep(
            [at, at.clone().addScaledVector(toward, radius(t) + 0.12 * length)],
            (u) => radius(t) * 0.45 * (1 - u),
            { sides: 5, tip: 'point' },
          );
          merge(blade, tooth);
        }
        solidOn(ctx, blade, { color, roughness: 0.35 }, { bone });
      });
      ctx.measure(length, 2);
    },
  },
});
