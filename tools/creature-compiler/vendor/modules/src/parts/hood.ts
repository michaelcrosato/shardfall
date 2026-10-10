import { colorRef, definePart, type PartBuildContext } from '@spawnforge/core';
import { Vector3 } from 'three';
import { z } from 'zod';
import { buildFan, type FanSpine, fanChains } from './_fan.ts';

const params = z.strictObject({
  width: z
    .number()
    .min(0.05)
    .max(1)
    .default(0.25)
    .describe('Hood width when spread, in torso lengths'),
  length: z.number().min(0.1).max(1).default(0.5).describe('Share of the neck the hood covers'),
  open: z.number().min(0).max(1).default(0.3).describe('How spread it is at rest'),
  markColor: colorRef('belly').describe('Colour of the eye marks on the back of the hood'),
});
type Params = z.output<typeof params>;

/** Ribs down each side of the neck, longest in the hood's upper middle. */
const RIBS = 6;

/**
 * The hood's ribs, left side first: each leaves the side of the neck and spreads out sideways;
 * at rest it lies back along the neck, so the hood is a narrow flap down each side, and it
 * swings forward to open in the hood's own plane, as a cobra's ribs do.
 */
function ribsOf(ctx: PartBuildContext, p: Params): FanSpine[] {
  const out: FanSpine[] = [];
  const end = Math.min(1, ctx.at + p.length);
  for (const side of [1, -1] as const)
    for (let k = 0; k < RIBS; k++) {
      const u = k / (RIBS - 1);
      const at = ctx.at + (end - ctx.at) * u;
      const socket = ctx.surface(at, side * 80);
      const open = socket.normal
        .clone()
        .addScaledVector(socket.forward, -socket.normal.dot(socket.forward))
        .normalize();
      out.push({
        socket,
        open,
        fold: socket.forward.clone().negate(),
        folded: 1 - p.open,
        length: p.width * ctx.scale * 0.5 * (0.2 + 0.8 * Math.sin(Math.PI * u ** 0.75)),
        radius: Math.max(0.0015 * ctx.scale, 0.006 * p.width * ctx.scale),
      });
    }
  return out;
}

/** A flat disc of `radius` at `centre`, facing `normal`, as a sheet on two bones. */
function disc(
  ctx: PartBuildContext,
  centre: Vector3,
  normal: Vector3,
  radius: number,
  weights: [number, number][],
  color: string,
) {
  const sides = 14;
  const a = new Vector3(0, 1, 0).cross(normal);
  if (a.lengthSq() < 1e-8) a.set(1, 0, 0);
  a.normalize();
  const b = new Vector3().crossVectors(normal, a);
  const positions = [centre.clone()];
  const along = [0];
  const across = [0];
  for (let i = 0; i < sides; i++) {
    const t = (i / sides) * Math.PI * 2;
    positions.push(
      centre
        .clone()
        .addScaledVector(a, Math.cos(t) * radius)
        .addScaledVector(b, Math.sin(t) * radius),
    );
    along.push(1);
    across.push(i / sides);
  }
  const indices: number[] = [];
  for (let i = 0; i < sides; i++) indices.push(0, 1 + i, 1 + ((i + 1) % sides));
  ctx.sheet(
    positions,
    positions.map(() => normal.clone()),
    indices,
    positions.map(() => weights),
    along,
    across,
    { color, opacity: 1, translucency: 0.2, roughness: 0.6, veins: 0 },
  );
}

export default definePart({
  id: 'hood',
  summary: "A cobra's hood: neck ribs that spread the skin into a flat shield in display.",
  tags: ['neck', 'display', 'snake'],
  slot: 'surface',
  material: 'skin',
  attach: { on: 'neck', at: 0.08, angle: 0 },
  provides: ['display'],
  params,
  example: { id: 'hood', type: 'hood', params: { width: 0.3 } },
  describe: () => 'a hood',
  hooks: {
    // Each rib on a flare-driven bone (docs/design/9.5-coverings.md).
    bones: (ctx, raw) => fanChains(ribsOf(ctx, raw as Params)),
    build(ctx, raw) {
      const p = raw as Params;
      const ribs = ribsOf(ctx, p);
      const pairs: [number, number][] = [];
      for (const side of [0, 1])
        for (let k = 0; k < RIBS - 1; k++) pairs.push([side * RIBS + k, side * RIBS + k + 1]);
      const skin = ctx.color('base', '#4a4a30');
      buildFan(ctx, ribs, pairs, {
        color: skin,
        tipColor: ctx.color('belly', '#c8b890'),
        spineColor: skin,
        scallop: 0.05,
        translucency: 0.25,
      });
      // An eye mark on the back of each side, on the skin between its two middle ribs.
      const mark = ctx.color(p.markColor, '#e8e0c0');
      const dark = '#1a1814';
      for (const side of [0, 1]) {
        const i = side * RIBS + Math.floor(RIBS / 2) - 1;
        const a = ctx.chains[i];
        const b = ctx.chains[i + 1];
        if (!a || !b) continue;
        const centre = (a.points[0] as Vector3)
          .clone()
          .lerp(a.points[1] as Vector3, 0.55)
          .lerp((b.points[0] as Vector3).clone().lerp(b.points[1] as Vector3, 0.55), 0.5);
        // The ribs turn about the hood plane's normal, so it holds open or folded.
        const rib = ribs[i] as (typeof ribs)[number];
        const normal = new Vector3().crossVectors(rib.socket.forward, rib.open).normalize();
        if (normal.dot(ctx.surface(ctx.at, 0).normal) < 0) normal.negate();
        const weights: [number, number][] = [
          [a.bones[0] as number, 0.5],
          [b.bones[0] as number, 0.5],
        ];
        const r = 0.14 * p.width * ctx.scale;
        disc(
          ctx,
          centre.clone().addScaledVector(normal, 0.005 * ctx.scale),
          normal,
          r,
          weights,
          mark,
        );
        disc(
          ctx,
          centre.clone().addScaledVector(normal, 0.008 * ctx.scale),
          normal,
          r * 0.45,
          weights,
          dark,
        );
      }
      ctx.measure(p.width * ctx.scale, 1);
    },
  },
});
