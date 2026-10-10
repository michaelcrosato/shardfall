import { colorRef, definePart } from '@spawnforge/core';
import { z } from 'zod';

const params = z.strictObject({
  length: z.number().min(0.02).max(0.6).default(0.12).describe('Length in torso lengths'),
  width: z
    .number()
    .min(0.01)
    .max(0.25)
    .default(0.05)
    .describe('Widest half-width in torso lengths'),
  thickness: z
    .number()
    .min(0.1)
    .max(1)
    .default(0.3)
    .describe('Thickness as a share of the width: low is a flat leaf, 1 a round cone'),
  curve: z
    .number()
    .min(-120)
    .max(120)
    .default(15)
    .describe('Degrees the ear bends; positive sweeps back toward the tail, negative forward'),
  lean: z
    .number()
    .min(-90)
    .max(90)
    .default(0)
    .describe('Degrees the root tilts forward (+) or back (-)'),
  droop: z
    .number()
    .min(0)
    .max(1)
    .default(0)
    .describe('How much the ear flops outward and down, 0 upright to 1 hanging'),
  color: colorRef('base').describe('Colour at the root: a palette name or a colour'),
  tipColor: colorRef('base').describe('Colour at the tip'),
});
type Params = z.output<typeof params>;

/**
 * A pointed, leaf-shaped ear: a flattened tube along a gentle arc, widest a third of the way up.
 * Written as one file to show that a new part type needs no change to the core.
 */
export default definePart({
  id: 'ear.pointed',
  summary: 'Leaf-shaped ear, upright or drooping; use side "both" for a pair.',
  tags: ['head', 'sense', 'soft'],
  slot: 'surface',
  material: 'skin',
  attach: { on: 'head', at: 0.85, angle: 45 },
  params,
  example: {
    id: 'ears',
    type: 'ear.pointed',
    attach: { on: 'head', at: 0.85, angle: 45, side: 'both' },
    params: { length: 0.14, width: 0.05 },
  },
  describe: (p, { count }) => {
    const shape = (p.droop as number) > 0.5 ? 'drooping' : 'pointed';
    return count === 1 ? `a ${shape} ear` : `${shape} ears`;
  },
  hooks: {
    build(ctx, raw) {
      const p = raw as Params;
      const socket = ctx.socket();
      const length = p.length * ctx.scale;
      const width = p.width * ctx.scale;
      // Drooping ears turn their bend outward and bend further.
      const path = ctx.geo.arc(length, p.curve + p.droop * 110, {
        lean: p.lean,
        heading: -p.droop * 90,
        segments: 10,
      });
      // Narrow root, widest about a third of the way up, pointed tip.
      const radius = (t: number) => width * (0.55 + 1.6 * t) * (1 - t) ** 1.2 + width * 0.03;
      const piece = ctx.geo.sweep(path, radius, {
        sides: 10,
        tip: 'point',
        cross: [p.thickness, 1],
      });
      ctx.emit(piece, socket, {
        color: ctx.color(p.color, '#808080'),
        tipColor: ctx.color(p.tipColor, '#808080'),
        sink: width * 0.4,
      });
    },
  },
});
