import { colorName, colorRef, definePattern, detail, relief } from '@spawnforge/core';
import { z } from 'zod';

/** Skin area of a typical body in square torso lengths, to turn `count` into a spacing. */
const AREA = 2.5;

export default definePattern({
  id: 'scars',
  summary: 'Old healed scars: pale, raised streaks and claw rakes across the skin.',
  tags: ['detail', 'battle'],
  params: z.strictObject({
    color: colorRef('belly').describe('Scar colour: a palette name or a colour'),
    count: z
      .number()
      .int()
      .min(1)
      .max(40)
      .default(6)
      .describe('About how many scars over the whole body'),
    length: z.number().min(0.02).max(0.6).default(0.15).describe('Scar length in torso lengths'),
    rake: z
      .number()
      .int()
      .min(1)
      .max(5)
      .default(1)
      .describe('Parallel cuts per scar: 3 or 4 read as claw marks'),
    depth: z.number().min(0).max(1).default(0.5).describe('How raised and creased the scars are'),
  }),
  example: { type: 'scars', color: 'belly', count: 5, rake: 3, region: 'torso' },
  describe: (p) => `${colorName(p.color as string)} scars`,
  hooks: {
    shade(k, s, p, seed) {
      const length = p.length as number;
      const rake = p.rake as number;
      const depth = p.depth as number;
      // Scar centres on a jittered lattice; each one within half a spacing of the skin, along
      // its normal, is projected onto it like a decal. That is one per `spacing²` of skin, so
      // about `count` over a typical body; never closer than a scar's length.
      const spacing = Math.max(length * 1.05, Math.sqrt(AREA / (p.count as number)));
      const width = Math.max(0.004, length * 0.09);
      const gap = Math.min(0.03, length * 0.25);
      const f = k.num(1 / spacing);
      const x = k.mul(s.x, f);
      const y = k.mul(s.y, f);
      const z = k.add(k.mul(s.z, f), k.num(seed));
      const half = k.num(0.5);
      const corner = (v: typeof x) => k.floor(k.sub(v, half));
      const [bx, by, bz] = [corner(x), corner(y), corner(z)];
      const one = k.num(1);
      let mark = k.num(0);
      let lip = k.num(0);
      for (let dz = 0; dz <= 1; dz++)
        for (let dy = 0; dy <= 1; dy++)
          for (let dx = 0; dx <= 1; dx++) {
            const cx = k.add(bx, k.num(dx));
            const cy = k.add(by, k.num(dy));
            const cz = k.add(bz, k.num(dz));
            const h = (salt: number) => k.hash3(cx, cy, cz, seed + salt);
            // From the scar's centre to the point, in torso lengths.
            const at = (c: typeof x, v: typeof x, salt: number) =>
              k.mul(k.sub(v, k.add(k.add(c, half), k.sub(h(salt), half))), k.num(spacing));
            const qx = at(cx, x, 11);
            const qy = at(cy, y, 23);
            const qz = at(cz, z, 37);
            // Near the skin along its normal, and the rest of the offset in the skin's plane.
            const qn = k.add(k.add(k.mul(qx, s.nx), k.mul(qy, s.ny)), k.mul(qz, s.nz));
            const near = k.sub(
              one,
              k.smoothstep(k.num(spacing * 0.4), k.num(spacing * 0.5), k.abs(qn)),
            );
            const tx = k.sub(qx, k.mul(qn, s.nx));
            const ty = k.sub(qy, k.mul(qn, s.ny));
            const tz = k.sub(qz, k.mul(qn, s.nz));
            // A random direction, laid into the skin's plane.
            const rx = k.sub(k.mul(h(61), k.num(2)), one);
            const ry = k.sub(k.mul(h(67), k.num(2)), one);
            const rz = k.sub(k.mul(h(71), k.num(2)), one);
            const rn = k.add(k.add(k.mul(rx, s.nx), k.mul(ry, s.ny)), k.mul(rz, s.nz));
            const ux = k.sub(rx, k.mul(rn, s.nx));
            const uy = k.sub(ry, k.mul(rn, s.ny));
            const uz = k.sub(rz, k.mul(rn, s.nz));
            const ul = k.max(
              k.num(1e-4),
              k.sqrt(k.add(k.add(k.mul(ux, ux), k.mul(uy, uy)), k.mul(uz, uz))),
            );
            // Across the scar: the in-plane direction at right angles to it (n × u).
            const vx = k.div(k.sub(k.mul(s.ny, uz), k.mul(s.nz, uy)), ul);
            const vy = k.div(k.sub(k.mul(s.nz, ux), k.mul(s.nx, uz)), ul);
            const vz = k.div(k.sub(k.mul(s.nx, uy), k.mul(s.ny, ux)), ul);
            const along = k.div(k.add(k.add(k.mul(tx, ux), k.mul(ty, uy)), k.mul(tz, uz)), ul);
            const across = k.add(k.add(k.mul(tx, vx), k.mul(ty, vy)), k.mul(tz, vz));
            // Rake: parallel cuts either side of the middle one.
            const index = k.clamp(
              k.floor(k.add(k.div(across, k.num(gap)), k.num(rake / 2))),
              k.num(0),
              k.num(rake - 1),
            );
            const dist = k.abs(
              k.sub(across, k.mul(k.sub(index, k.num((rake - 1) / 2)), k.num(gap))),
            );
            // Tapered at both ends.
            const t = k.min(one, k.abs(k.div(along, k.num(length / 2))));
            const w = k.mul(k.num(width), k.sqrt(k.sub(one, k.mul(t, t))));
            const core = k.sub(one, k.smoothstep(k.mul(w, k.num(0.55)), w, dist));
            const edge = k.mul(
              k.smoothstep(k.mul(w, k.num(0.6)), w, dist),
              k.sub(one, k.smoothstep(w, k.mul(w, k.num(1.8)), dist)),
            );
            mark = k.max(mark, k.mul(core, near));
            lip = k.max(lip, k.mul(k.mul(edge, near), k.step(t, k.num(0.999))));
          }
      return {
        mask: k.mul(mark, detail(k, s, width * 4)),
        roughness: k.num(0.45),
        // Raised along the middle, creased at the edges.
        height: k.mul(
          k.mul(k.sub(k.mul(mark, k.num(0.5)), k.mul(lip, k.num(0.35))), k.num(width * depth)),
          relief(k, s, width * 3),
        ),
      };
    },
  },
});
