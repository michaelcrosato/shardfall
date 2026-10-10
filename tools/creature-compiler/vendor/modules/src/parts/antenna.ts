import { colorRef, definePart } from '@spawnforge/core';
import { Vector3 } from 'three';
import { z } from 'zod';
import { solidOn } from './_solid.ts';

const params = z.strictObject({
  length: z.number().min(0.05).max(2).default(0.5).describe('Length in torso lengths'),
  segments: z.number().int().min(2).max(12).default(6).describe('Jointed segments'),
  shape: z
    .enum(['thread', 'club', 'feather'])
    .default('thread')
    .describe(
      '"thread" plain, "club" thickened at the tip (a butterfly), "feather" combed (a moth)',
    ),
  curve: z.number().min(-180).max(180).default(40).describe('Degrees it curves back at rest'),
  stiffness: z.number().min(0).max(1).default(0.5).describe('How little it sways'),
  color: colorRef('accent').describe('Colour: a palette name or a colour'),
});
type Params = z.output<typeof params>;

const X = new Vector3(1, 0, 0);

/** Thickness along the antenna (0 root to 1 tip), as a share of its root radius. */
function profile(shape: Params['shape'], t: number): number {
  if (shape === 'club') return 0.75 - 0.35 * t + 1.6 * Math.max(0, (t - 0.78) / 0.22) ** 1.5;
  return 1 - 0.55 * t;
}

export default definePart({
  id: 'antenna',
  summary:
    'A jointed antenna that sways on springs: thread-like, clubbed or feathery; side "both" for a pair.',
  tags: ['head', 'sense', 'insect'],
  slot: 'surface',
  material: 'chitin',
  attach: { on: 'head', at: 0.3, angle: 30 },
  params,
  example: {
    id: 'antennae',
    type: 'antenna',
    attach: { on: 'head', at: 0.3, angle: 30, side: 'both' },
    params: { length: 0.6, shape: 'feather' },
  },
  describe: (p, { count }) =>
    `${count === 1 ? 'an ' : ''}${p.shape === 'thread' ? '' : `${p.shape === 'feather' ? 'feathery' : 'clubbed'} `}antenna${count === 1 ? '' : 'e'}`,
  hooks: {
    // One chain of jointed segments rising out of the head and curving back, on a spring
    // (docs/design/9.4-tentacles-parts.md).
    bones(ctx, raw) {
      const p = raw as Params;
      const socket = ctx.socket();
      const length = p.length * ctx.scale;
      const n = p.segments;
      const dir = new Vector3(0.3, 0.7, 0.65).normalize();
      const turn = (-p.curve * Math.PI) / 180 / n;
      const points = [new Vector3(0, -0.2 * socket.radius * 0.1, 0)];
      for (let k = 0; k < n; k++) {
        points.push((points[k] as Vector3).clone().addScaledVector(dir, length / n));
        dir.applyAxisAngle(X, turn);
      }
      const root = Math.max(0.004 * ctx.scale, 0.02 * length);
      return [
        {
          points: points.map((q) => ctx.toModel(socket, q)),
          radii: points.map((_, k) => root * profile(p.shape, k / n)),
          drive: 'spring',
          stiffness: 0.15 + 0.6 * p.stiffness,
        },
      ];
    },
    build(ctx, raw) {
      const p = raw as Params;
      const chain = ctx.chains[0];
      if (!chain) return;
      const length = p.length * ctx.scale;
      const root = Math.max(0.004 * ctx.scale, 0.02 * length);
      const color = ctx.color(p.color, '#3a2a1c');
      const sides = Math.max(5, Math.round(7 * ctx.detail));
      const shaft = ctx.geo.sweep(chain.points, (t) => root * profile(p.shape, t), {
        sides,
        ridges: p.segments,
        ridgeDepth: 0.15,
      });
      solidOn(ctx, shaft, { color, roughness: 0.4 }, { bones: chain.bones });
      // A moth's comb: thin barbs either side along the outer two thirds, each on its segment.
      if (p.shape === 'feather') {
        const n = chain.bones.length;
        const count = Math.max(6, Math.round(14 * ctx.detail));
        for (let i = 0; i < count; i++) {
          const t = 0.3 + (0.68 * i) / (count - 1);
          const k = Math.min(n - 1, Math.floor(t * n));
          const a = chain.points[k] as Vector3;
          const b = chain.points[k + 1] as Vector3;
          const at = a.clone().lerp(b, t * n - k);
          const axis = new Vector3().subVectors(b, a).normalize();
          const side = new Vector3().crossVectors(axis, new Vector3(0, 1, 0));
          if (side.lengthSq() < 1e-8) side.set(1, 0, 0);
          side.normalize();
          const reach = length * 0.12 * Math.sin(Math.PI * Math.min(1, (t - 0.25) / 0.75));
          for (const sign of [1, -1]) {
            const tip = at
              .clone()
              .addScaledVector(side, sign * reach)
              .addScaledVector(axis, -0.3 * reach);
            const barb = ctx.geo.sweep([at, tip], (u) => root * 0.2 * (1 - 0.7 * u), {
              sides: 4,
            });
            solidOn(ctx, barb, { color, roughness: 0.5 }, { bone: chain.bones[k] as number });
          }
        }
      }
      ctx.measure(length, 1);
    },
  },
});
