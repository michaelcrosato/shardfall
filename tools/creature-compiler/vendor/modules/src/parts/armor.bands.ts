import { colorRef, definePart } from '@spawnforge/core';
import type { Vector3 } from 'three';
import { z } from 'zod';
import { gridNormals, type PointGrid, skinGrid, slab } from './_cover.ts';

const params = z.strictObject({
  bands: z.number().int().min(2).max(30).default(9).describe('Bands from front to back'),
  thickness: z.number().min(0.005).max(0.15).default(0.025).describe('Thickness in torso lengths'),
  overlap: z.number().min(0).max(0.8).default(0.3).describe('How much each band overlaps the next'),
  scales: z.boolean().default(false).describe('Break the bands into scales, like a pangolin'),
  color: colorRef('base').describe('Armour colour: a palette name or a colour'),
});
type Params = z.output<typeof params>;

export default definePart({
  id: 'armor.bands',
  summary:
    'Overlapping bands of armour across an area of the body, like an armadillo or a pangolin.',
  tags: ['armor'],
  slot: 'area',
  material: 'horn',
  attach: { on: 'torso', area: 'back', from: 0, to: 1 },
  params,
  example: { id: 'armor', type: 'armor.bands', params: { bands: 11 } },
  describe: (p) => ((p.scales as boolean) ? 'overlapping scales of armour' : 'bands of armour'),
  hooks: {
    // Strips across the area, each lying over the one behind it with its back edge raised, so
    // they shingle from front to back (docs/design/9.5-coverings.md).
    build(ctx, raw) {
      const p = raw as Params;
      const band = ctx.area ?? { from: 0, to: 1, angles: [0, 70] as const };
      const thickness = p.thickness * ctx.scale;
      const step = (band.to - band.from) / p.bands;
      const cols = Math.max(8, Math.round(20 * ctx.detail));
      const rows = Math.max(2, Math.round(3 * ctx.detail));
      // Scales: cells round each band, staggered from band to band.
      const cells = p.scales ? Math.max(4, Math.round(cols / 2)) : 1;
      const color = ctx.color(p.color, '#7a6a52');
      for (let i = 0; i < p.bands; i++) {
        const from = Math.max(band.from, band.from + i * step - (p.overlap * step) / 2);
        const to = Math.min(band.to, band.from + (i + 1) * step + (p.overlap * step) / 2);
        const skin = skinGrid(ctx, rows, cols, { from, to, angle: band.angles[1] });
        const stride = cols + 1;
        // Later bands sit a little higher, and each rises toward its back edge.
        const points = skin.sockets.map((s, k) => {
          const u = Math.floor(k / stride) / rows;
          return s.position
            .clone()
            .addScaledVector(s.normal, thickness * (1 + (0.15 * i) / p.bands + 0.8 * u));
        });
        const shift = p.scales && i % 2 === 1 ? 0.5 : 0;
        for (let c = 0; c < cells; c++) {
          // Columns this cell covers, with a gap between scales.
          const c0 = Math.round(((c + shift) * cols) / cells);
          const c1 = Math.min(cols, Math.round(((c + 1 + shift) * cols) / cells));
          if (c1 - c0 < 1) continue;
          const width = c1 - c0;
          const take = (k: number) => {
            const r = Math.floor(k / (width + 1));
            const col = c0 + (k % (width + 1));
            return r * stride + col;
          };
          const count = (rows + 1) * (width + 1);
          const grid: PointGrid = {
            rows,
            cols: width,
            points: Array.from({ length: count }, (_, k) => (points[take(k)] as Vector3).clone()),
            weights: Array.from({ length: count }, (_, k) => skin.sockets[take(k)]?.weights ?? []),
          };
          if (p.scales) {
            // Pull each scale's side edges in a little, leaving a seam.
            const inset = 0.08;
            for (let r = 0; r <= rows; r++) {
              const a = grid.points[r * (width + 1)] as Vector3;
              const b = grid.points[r * (width + 1) + width] as Vector3;
              const pa = a.clone();
              a.lerp(b, inset);
              b.lerp(pa, inset);
            }
          }
          const normals = gridNormals(
            grid,
            (k) => (skin.sockets[take(k)] as (typeof skin.sockets)[number]).normal,
          );
          const piece = slab(grid, normals, thickness);
          ctx.solid(piece.positions, piece.normals, piece.indices, piece.weights, {
            color,
            roughness: 0.5,
          });
        }
      }
      ctx.measure(thickness, p.bands);
    },
  },
});
