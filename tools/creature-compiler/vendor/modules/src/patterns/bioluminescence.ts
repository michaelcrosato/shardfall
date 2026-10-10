import { cells, colorName, colorRef, definePattern } from '@spawnforge/core';
import { z } from 'zod';

/** Heights of the glowing lines along each flank, from the belly (-1) to the spine (1). */
const LINES = [-0.3, 0.15, 0.55];

export default definePattern({
  id: 'bioluminescence',
  summary: 'Glowing spots or lines that light up in the dark and pulse slowly.',
  tags: ['glow', 'deep-sea'],
  params: z.strictObject({
    color: colorRef('#7fffd0').describe('Glow colour: a palette name or a colour'),
    shape: z
      .enum(['spots', 'lines'])
      .default('spots')
      .describe('"spots" scattered over the skin, or "lines" of dots along the body'),
    size: z
      .number()
      .min(0.005)
      .max(0.3)
      .default(0.03)
      .describe('Spot radius or line width in torso lengths'),
    density: z.number().min(0).max(1).default(0.5).describe('Share of possible spots that glow'),
    brightness: z
      .number()
      .min(0)
      .max(4)
      .default(1.5)
      .describe('Emissive strength; 1 matches a lit surface'),
    pulse: z.number().min(0).max(2).default(0.3).describe('Pulses per second; 0 glows steadily'),
  }),
  example: { type: 'bioluminescence', color: '#60ffd0', shape: 'spots', size: 0.03, density: 0.6 },
  describe: (p) => `glowing ${colorName(p.color as string)} ${p.shape as string}`,
  hooks: {
    shade(k, s, p, seed) {
      const size = p.size as number;
      const density = k.param(p.density as number);
      const pulse = p.pulse as number;
      // Each light on its own phase, from its random id.
      const beat = (id: (typeof s)['x']) =>
        pulse > 0
          ? k.add(
              k.num(0.65),
              k.mul(
                k.sin(k.mul(k.add(k.mul(s.time, k.num(pulse)), id), k.num(Math.PI * 2))),
                k.num(0.35),
              ),
            )
          : k.num(1);
      let core: (typeof s)['x'];
      let halo: (typeof s)['x'];
      let id: (typeof s)['x'];
      if (p.shape === 'lines') {
        // Rows of dots along each flank, spaced three sizes apart down the body.
        const count = Math.max(4, Math.round(2 / (size * 3)));
        const u = k.mul(s.spine, k.num(count));
        const along = k.abs(k.sub(k.fract(u), k.num(0.5)));
        // Height units are about a third of a torso length on a typical body.
        const half = (size / 0.3) * 0.5;
        const side = k.step(k.num(0), s.x);
        core = k.num(0);
        halo = k.num(0);
        id = k.num(0);
        LINES.forEach((at, i) => {
          const off = k.abs(k.sub(s.height, k.num(at)));
          const lit = k.step(k.hash3(k.floor(u), k.num(i), side, seed), density);
          const dot = k.mul(
            k.sub(k.num(1), k.smoothstep(k.num(half * 0.4), k.num(half), off)),
            k.sub(k.num(1), k.smoothstep(k.num(0.18), k.num(0.32), along)),
          );
          const glow = k.mul(
            k.sub(k.num(1), k.smoothstep(k.num(half * 0.6), k.num(half * 2.2), off)),
            k.sub(k.num(1), k.smoothstep(k.num(0.2), k.num(0.5), along)),
          );
          core = k.max(core, k.mul(dot, lit));
          halo = k.max(halo, k.mul(glow, lit));
          id = k.max(id, k.mul(k.hash3(k.floor(u), k.num(i), side, seed + 1), lit));
        });
        // Along the body and tail, not the limbs or the head.
        const body = k.mul(k.sub(k.num(1), s.limbs), k.sub(k.num(1), s.head));
        core = k.mul(core, body);
        halo = k.mul(halo, body);
      } else {
        const f = k.num(1 / (size * 3));
        const c = cells(
          k,
          k.mul(s.x, f),
          k.mul(s.y, f),
          k.add(k.mul(s.z, f), k.num(seed)),
          0.9,
          seed,
        );
        const lit = k.step(c.id, density);
        core = k.mul(k.sub(k.num(1), k.smoothstep(k.num(0.14), k.num(0.33), c.distance)), lit);
        halo = k.mul(k.sub(k.num(1), k.smoothstep(k.num(0.2), k.num(0.5), c.distance)), lit);
        id = c.id;
      }
      return {
        mask: k.mul(core, k.num(0.6)),
        emissive: k.mul(
          k.mul(k.add(core, k.mul(halo, k.num(0.35))), beat(id)),
          k.param(p.brightness as number),
        ),
      };
    },
  },
});
