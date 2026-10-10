import { colorRef, definePart } from '@spawnforge/core';
import { Vector3 } from 'three';
import { z } from 'zod';
import { gridSheet } from './_sheet.ts';

const params = z.strictObject({
  shape: z
    .enum(['forked', 'crescent', 'rounded', 'flukes'])
    .default('forked')
    .describe('"forked", "crescent" (a tuna), "rounded" or "flukes" (flat, like a whale)'),
  size: z.number().min(0.05).max(1.5).default(0.35).describe('Fin height in torso lengths'),
  upper: z.number().min(0).max(1).default(0.6).describe('Share of the fin above the tail line'),
  color: colorRef('base').describe('Fin colour: a palette name or a colour'),
});
type Params = z.output<typeof params>;

/**
 * The fin's outline from its lower tip (u = 0) to its upper one (u = 1), as (back, up) in
 * metres from the tail's tip, for a fin `h` tall with `upper` of it above the line.
 */
function outline(shape: Params['shape'], u: number, h: number, upper: number): [number, number] {
  const top = upper * h;
  const bottom = -(1 - upper) * h;
  if (shape === 'rounded') {
    const a = Math.PI * (u - 0.5);
    return [0.6 * h * Math.cos(a), (top - bottom) * 0.5 * Math.sin(a) + (top + bottom) / 2];
  }
  // Two lobes swept back to points, meeting at a notch: deep when forked, shallow and hollow
  // when crescent (or flukes, seen from above).
  const [tipBack, notchBack] =
    shape === 'forked'
      ? [0.55 * h, 0.25 * h]
      : shape === 'crescent'
        ? [0.6 * h, 0.14 * h]
        : [0.3 * h, 0.16 * h];
  const notch: [number, number] = [notchBack, (top + bottom) * 0.1];
  const from: [number, number] = u < 0.5 ? [tipBack, bottom] : notch;
  const to: [number, number] = u < 0.5 ? notch : [tipBack, top];
  const t = u < 0.5 ? u * 2 : (u - 0.5) * 2;
  // The trailing edge hollows toward the notch.
  const hollow = (shape === 'forked' ? 0.04 : 0.1) * h * Math.sin(Math.PI * t);
  return [from[0] + (to[0] - from[0]) * t - hollow, from[1] + (to[1] - from[1]) * t];
}

export default definePart({
  id: 'fin.tail',
  summary:
    'A tail fin at the tail tip: forked like a shark, rounded like a carp, or flat like a whale.',
  tags: ['fin', 'aquatic', 'tail'],
  slot: 'surface',
  material: 'skin',
  attach: { on: 'tail', at: 1, angle: 0 },
  params,
  example: { id: 'tailfin', type: 'fin.tail', params: { shape: 'forked', size: 0.4 } },
  describe: (p) => `a ${p.shape as string} tail fin`,
  hooks: {
    build(ctx, raw) {
      const p = raw as Params;
      // Upright in the plane of the tail and its back, or flat across it for flukes; built from
      // a short base at the tail's tip out to the outline (docs/design/9.3-wings-fins.md).
      const top = ctx.socket(ctx.at, 0);
      const bottom = ctx.socket(ctx.at, 180);
      const centre = top.position.clone().add(bottom.position).multiplyScalar(0.5);
      const back = top.forward.clone().negate();
      const flat = p.shape === 'flukes';
      const up = flat ? top.side.clone() : top.normal.clone();
      const h = p.size * ctx.scale;
      const upper = flat ? 0.5 : p.upper;
      const base = Math.max(top.radius, 0.04 * h) * 1.1;
      const rows = Math.max(4, Math.round(8 * ctx.detail));
      const cols = Math.max(7, Math.round(17 * ctx.detail));
      const weights = top.weights.map(([b, w]) => [b, w] as [number, number]);
      const sheet = gridSheet(
        rows,
        cols,
        (v, u) => {
          const [x, y] = outline(p.shape, u, h, upper);
          // The base: a short line across the tail's tip, set a little into it.
          const b0 = centre
            .clone()
            .addScaledVector(back, -0.5 * base)
            .addScaledVector(up, (u - 0.5) * 2 * base);
          const edge = centre.clone().addScaledVector(back, x).addScaledVector(up, y);
          return b0.lerp(edge, v);
        },
        () => weights,
      );
      ctx.sheet(
        sheet.positions,
        sheet.normals,
        sheet.indices,
        sheet.weights,
        sheet.along,
        sheet.across,
        {
          color: ctx.color(p.color, '#5d6b74'),
          opacity: 1,
          translucency: 0.3,
          roughness: 0.5,
          veins: 0.35,
        },
      );
      const span = new Vector3(...outline(p.shape, 1, h, upper), 0).distanceTo(
        new Vector3(...outline(p.shape, 0, h, upper), 0),
      );
      ctx.measure(span, 1);
    },
  },
});
