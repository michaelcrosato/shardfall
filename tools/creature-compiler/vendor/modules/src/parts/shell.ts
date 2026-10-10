import { colorRef, definePart } from '@spawnforge/core';
import { Vector3 } from 'three';
import { z } from 'zod';
import { gridNormals, type PointGrid, sampleGrid, skinGrid, slab } from './_cover.ts';

const params = z.strictObject({
  dome: z
    .number()
    .min(0)
    .max(1)
    .default(0.6)
    .describe('How domed: 0 flat, 1 a high tortoise shell'),
  thickness: z.number().min(0.005).max(0.2).default(0.03).describe('Thickness in torso lengths'),
  overhang: z
    .number()
    .min(0)
    .max(0.5)
    .default(0.12)
    .describe('How far the rim reaches past the body'),
  scutes: z.number().int().min(0).max(30).default(13).describe('Plates on the shell; 0 for smooth'),
  color: colorRef('base').describe('Shell colour: a palette name or a colour'),
  seamColor: colorRef('accent').describe('Colour of the seams between scutes'),
});
type Params = z.output<typeof params>;

/** Rings of rim past the area's edge. */
const RIM = 2;

export default definePart({
  id: 'shell',
  summary:
    'A domed shell over an area of the body, divided into scutes, like a turtle or a tortoise.',
  tags: ['armor', 'shell'],
  slot: 'area',
  material: 'horn',
  attach: { on: 'torso', area: 'back', from: 0, to: 1 },
  params,
  example: { id: 'shell', type: 'shell', params: { dome: 0.7 } },
  describe: (p) => ((p.dome as number) > 0.5 ? 'a domed shell' : 'a flat shell'),
  hooks: {
    // A slab over the skin, domed in the middle, with a rim past the edge; scutes are raised
    // cells over a base in the seam colour (docs/design/9.5-coverings.md).
    build(ctx, raw) {
      const p = raw as Params;
      const band = ctx.area ?? { from: 0, to: 1, angles: [0, 70] as const };
      const rows = Math.max(8, Math.round(18 * ctx.detail));
      const cols = Math.max(10, Math.round(24 * ctx.detail));
      const skin = skinGrid(ctx, rows, cols, {
        from: band.from,
        to: band.to,
        angle: band.angles[1],
      });
      const thickness = p.thickness * ctx.scale;
      const stride = cols + 1;
      // The shell's outer surface over the area.
      const top = skin.sockets.map((s, k) => {
        const u = (2 * Math.floor(k / stride)) / rows - 1;
        const v = (2 * (k % stride)) / cols - 1;
        const bulge = p.dome * 0.8 * s.radius * (1 - u * u) ** 0.7 * (1 - v * v) ** 0.7;
        return s.position.clone().addScaledVector(s.normal, thickness + bulge);
      });
      // The rim: rings carried on past the edge along the shell's slope, drooping a little.
      const R = rows + 2 * RIM;
      const C = cols + 2 * RIM;
      const at = (r: number, c: number) => top[r * stride + c] as Vector3;
      const points: Vector3[] = [];
      const weights: (readonly (readonly [number, number])[])[] = [];
      const outward: Vector3[] = [];
      for (let i = 0; i <= R; i++)
        for (let j = 0; j <= C; j++) {
          const r = Math.min(rows, Math.max(0, i - RIM));
          const c = Math.min(cols, Math.max(0, j - RIM));
          const di = i - RIM - r;
          const dj = j - RIM - c;
          const socket = skin.sockets[r * stride + c] as (typeof skin.sockets)[number];
          const point = at(r, c).clone();
          if (di !== 0 || dj !== 0) {
            // The rim carries on past the edge along the skin there (not the dome, which is
            // steep at its edge), so on a rounded body it stands off it.
            const reach = 2 * p.overhang * socket.radius;
            const skinAt = (rr: number, cc: number) =>
              (skin.sockets[rr * stride + cc] as (typeof skin.sockets)[number]).position;
            const along = (d: Vector3) => {
              d.addScaledVector(socket.normal, -d.dot(socket.normal));
              return d.lengthSq() > 1e-16 ? d.normalize() : d;
            };
            if (di !== 0)
              point.addScaledVector(
                along(new Vector3().subVectors(skinAt(r, c), skinAt(r - Math.sign(di), c))),
                (Math.abs(di) / RIM) * reach,
              );
            if (dj !== 0)
              point.addScaledVector(
                along(new Vector3().subVectors(skinAt(r, c), skinAt(r, c - Math.sign(dj)))),
                (Math.abs(dj) / RIM) * reach,
              );
          }
          points.push(point);
          weights.push(socket.weights);
          outward.push(socket.normal);
        }
      const grid: PointGrid = { rows: R, cols: C, points, weights };
      const normals = gridNormals(grid, (k) => outward[k] as Vector3);
      const shell = slab(grid, normals, thickness, outward);
      const color = ctx.color(p.color, '#6a5a3a');
      const seam = ctx.color(p.seamColor, '#2a2418');
      ctx.solid(shell.positions, shell.normals, shell.indices, shell.weights, {
        color: p.scutes > 0 ? seam : color,
        roughness: 0.55,
      });
      // Scutes: a row down the middle and one each side, and marginal plates round the rim, as
      // raised cells over the whole shell, rim included.
      if (p.scutes > 0) {
        const lo = RIM / R;
        const hi = 1 - RIM / R;
        const left = RIM / C;
        const right = 1 - RIM / C;
        // Across the middle: -1 to 1 maps to the area's edges, past them the rim.
        const across = (w: number) => left + ((right - left) * (w + 1)) / 2;
        const middle = Math.max(1, Math.round((p.scutes * 5) / 13));
        const flank = Math.max(1, Math.round((p.scutes * 4) / 13));
        const cells: [number, number, number, number][] = [];
        const row = (n: number, u0: number, u1: number, v0: number, v1: number) => {
          for (let i = 0; i < n; i++)
            cells.push([u0 + ((u1 - u0) * i) / n, u0 + ((u1 - u0) * (i + 1)) / n, v0, v1]);
        };
        const edge = 0.8;
        row(middle, lo, hi, across(-0.28), across(0.28));
        row(flank, lo, hi, across(0.28), across(edge));
        row(flank, lo, hi, across(-edge), across(-0.28));
        if (p.scutes >= 8) {
          // Margins down each side over the rim, and across each end.
          row(middle + 3, 0, 1, across(edge), 1);
          row(middle + 3, 0, 1, 0, across(-edge));
          for (const [u0, u1] of [
            [0, lo],
            [hi, 1],
          ] as const)
            for (let i = 0; i < 4; i++) {
              const v0 = across(-edge) + ((across(edge) - across(-edge)) * i) / 4;
              cells.push([u0, u1, v0, v0 + (across(edge) - across(-edge)) / 4]);
            }
        }
        const n = Math.max(3, Math.round(5 * ctx.detail));
        const lift = 0.3 * thickness;
        for (const [u0, u1, v0, v1] of cells) {
          const gu = 0.06 * (u1 - u0) + 0.003;
          const gv = 0.06 * (v1 - v0) + 0.003;
          const place = (a: number, b: number) =>
            sampleGrid(
              grid,
              normals,
              u0 + gu + ((u1 - u0 - 2 * gu) * a) / n,
              v0 + gv + ((v1 - v0 - 2 * gv) * b) / n,
            );
          const cellPoints: Vector3[] = [];
          const cellWeights: (readonly (readonly [number, number])[])[] = [];
          const cellOut: Vector3[] = [];
          for (let a = 0; a <= n; a++)
            for (let b = 0; b <= n; b++) {
              const s = place(a, b);
              const x = (2 * a) / n - 1;
              const y = (2 * b) / n - 1;
              cellOut.push(s.normal.clone());
              cellPoints.push(
                s.point.addScaledVector(s.normal, lift * (1 + 0.8 * (1 - x * x) * (1 - y * y))),
              );
              cellWeights.push(s.weights);
            }
          const cell: PointGrid = { rows: n, cols: n, points: cellPoints, weights: cellWeights };
          const plate = slab(
            cell,
            gridNormals(cell, (k) => cellOut[k] as Vector3),
            lift * 1.4,
          );
          ctx.solid(plate.positions, plate.normals, plate.indices, plate.weights, {
            color,
            roughness: 0.5,
          });
        }
      }
      const first = skin.sockets[Math.floor(cols / 2)] as (typeof skin.sockets)[number];
      const last = skin.sockets[
        rows * stride + Math.floor(cols / 2)
      ] as (typeof skin.sockets)[number];
      ctx.measure(first.position.distanceTo(last.position), Math.max(1, p.scutes));
    },
  },
});
