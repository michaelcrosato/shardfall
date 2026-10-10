import { colorName, colorRef, definePart } from '@spawnforge/core';
import { z } from 'zod';

const params = z.strictObject({
  scale: z
    .number()
    .min(0.2)
    .max(3)
    .default(1)
    .describe('Eye size relative to the head: 1 is typical, 2 big, 0.5 small'),
  size: z
    .number()
    .min(0.005)
    .max(0.2)
    .optional()
    .describe('Eyeball radius in torso lengths; when set it wins over `scale`'),
  pupil: z.enum(['round', 'slit', 'goat']).default('round').describe('Pupil shape'),
  irisColor: colorRef('#c8a030').describe('Iris colour: a palette name or a colour'),
  scleraColor: colorRef('#e8e2cc').describe('Colour of the eyeball around the iris'),
  iris: z.number().min(0.2).max(1).default(0.7).describe('Iris size as a share of the visible eye'),
  bulge: z.number().min(0).max(1).default(0.5).describe('How far the eye stands out of the skin'),
  lids: z
    .boolean()
    .default(true)
    .describe('Eyelids that blink; snakes, fish and insects have none'),
  squint: z
    .number()
    .min(0)
    .max(1)
    .default(0.15)
    .describe('How far the upper lid hangs over the eye at rest: 0 wide open, 1 a menacing squint'),
});
type Params = z.output<typeof params>;

/** Eyeball radius as a share of the radius of what the eye sits on, at `scale` 1. */
const RELATIVE = 0.18;

export default definePart({
  id: 'eye.basic',
  summary:
    'Round eyeball with an iris and pupil, sized to the head, with eyelids that blink (or none).',
  tags: ['head', 'sense'],
  slot: 'surface',
  material: 'eye',
  attach: { on: 'head', at: 0.4, angle: 60 },
  params,
  example: {
    id: 'eyes',
    type: 'eye.basic',
    attach: { on: 'head', at: 0.4, angle: 60, side: 'both' },
    params: { scale: 1.2, pupil: 'slit', squint: 0.4 },
  },
  describe(p) {
    const size = p.size as number | undefined;
    const scale = p.scale as number;
    const big =
      size !== undefined
        ? size >= 0.05
          ? 'big '
          : size <= 0.015
            ? 'small '
            : ''
        : scale >= 1.6
          ? 'big '
          : scale <= 0.7
            ? 'small '
            : '';
    const pupil =
      p.pupil === 'slit' ? 'slit-pupilled ' : p.pupil === 'goat' ? 'goat-pupilled ' : '';
    return `${big}${colorName(p.irisColor as string)} ${pupil}eyes`;
  },
  hooks: {
    build(ctx, raw) {
      const p = raw as Params;
      const socket = ctx.socket();
      const radius = p.size !== undefined ? p.size * ctx.scale : RELATIVE * p.scale * socket.radius;
      ctx.measure(radius, 1);
      ctx.emit(ctx.geo.sphere(radius, 10, 18), socket, {
        sink: radius * (0.85 - p.bulge * 0.6),
        eye: {
          iris: ctx.color(p.irisColor, '#c8a030'),
          sclera: ctx.color(p.scleraColor, '#e8e2cc'),
          pupil: p.pupil,
          irisSize: p.iris,
          radius,
          lids: p.lids,
          squint: p.squint,
        },
      });
    },
  },
});
