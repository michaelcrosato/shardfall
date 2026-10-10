import { colorRef, definePart } from '@spawnforge/core';
import { z } from 'zod';

const params = z.strictObject({
  scale: z
    .number()
    .min(0.2)
    .max(3)
    .default(1)
    .describe('Tooth size relative to the head: 1 is typical, 2 big, 0.5 small'),
  fangScale: z
    .number()
    .min(0.2)
    .max(3)
    .default(1)
    .describe('Fang size relative to the head: 1 is typical, 2 sabre-like'),
  spacing: z
    .number()
    .min(0)
    .max(2)
    .default(0.15)
    .describe('Gap between neighbouring teeth as a share of a tooth’s width; 0 packs them tight'),
  incisors: z
    .number()
    .int()
    .min(0)
    .max(4)
    .default(2)
    .describe('Small front teeth on each side before the fangs (not used when `count` is set)'),
  fangs: z.number().int().min(0).max(4).default(1).describe('Long fangs at the front of each row'),
  count: z
    .number()
    .int()
    .min(1)
    .max(40)
    .optional()
    .describe('Teeth per row on each side; without it, as many as `spacing` fits'),
  length: z
    .number()
    .min(0.005)
    .max(0.2)
    .optional()
    .describe('Tooth length in torso lengths; when set it wins over `scale`'),
  fangLength: z
    .number()
    .min(0.01)
    .max(0.4)
    .optional()
    .describe('Fang length in torso lengths; when set it wins over `fangScale`'),
  upper: z.boolean().default(true).describe('Teeth in the upper row'),
  lower: z.boolean().default(true).describe('Teeth in the lower row'),
  color: colorRef('#efe8d0'),
});
type Params = z.output<typeof params>;

/** Lengths as shares of the head's radius at the tooth, at `scale` 1. */
const INCISOR = 0.1;
const CHEEK_FRONT = 0.16;
const CHEEK_BACK = 0.1;
const FANG = 0.45;
/** How far a tooth leans out of the mouth over its length (fangs clear the other jaw's lip). */
const LEAN = { incisor: 0.05, fang: 0.3, cheek: 0.08 } as const;
/** A tooth's base radius as a share of its length: fangs are slender, cheek teeth broad. */
const BASE = { incisor: 0.3, fang: 0.2, cheek: 0.35 } as const;

type Kind = keyof typeof BASE;

export default definePart({
  id: 'teeth.row',
  summary:
    'Teeth standing in the gums along the mouth, sized to the head: incisors, fangs, then cheek teeth. The upper row moves with the head, the lower with the jaw.',
  tags: ['head', 'mouth', 'weapon'],
  slot: 'mouth',
  material: 'enamel',
  attach: { on: 'head' },
  params,
  example: { id: 'teeth', type: 'teeth.row', params: { fangs: 1, fangScale: 1.3 } },
  describe(p) {
    const fangs = (p.fangs as number) > 0;
    const long =
      p.fangLength !== undefined
        ? (p.fangLength as number) >= 0.08
        : (p.fangScale as number) >= 1.5;
    if (fangs && long) return 'long fangs';
    return fangs ? 'teeth and fangs' : 'teeth';
  },
  hooks: {
    build(ctx, raw) {
      const p = raw as Params;
      const color = ctx.color(p.color, '#efe8d0');
      const rows: ('upper' | 'lower')[] = [];
      if (p.upper) rows.push('upper');
      if (p.lower) rows.push('lower');
      if (rows.length === 0 || !ctx.mouth(0.5, 'upper', 1)) return;
      // The row's length along the gums, tip to corner.
      let rowLength = 0;
      for (let i = 1; i <= 20; i++) {
        const a = ctx.mouth((i - 1) / 20, 'upper', 1);
        const b = ctx.mouth(i / 20, 'upper', 1);
        if (a && b) rowLength += a.position.distanceTo(b.position);
      }
      let largest = 0;
      let perRow = 0;
      for (const row of rows) {
        for (const side of [1, -1]) {
          const teeth =
            p.count !== undefined
              ? counted(p, row, ctx.scale)
              : packed(p, row, rowLength, ctx.scale, ctx);
          perRow = Math.max(perRow, teeth.length);
          for (const tooth of teeth) {
            const socket = ctx.mouth(tooth.t, row, side);
            if (!socket) return;
            // A little variety from the part's own stream, so a packed row never looks
            // stamped (a written count keeps plan 1's exact lengths).
            const jitter = p.count === undefined ? (ctx.rng.next() - 0.5) * 0.16 : 0;
            const length = tooth.length(socket.radius) * (1 + jitter);
            largest = Math.max(largest, length);
            const base = length * BASE[tooth.kind];
            const bend = tooth.kind === 'fang' ? 18 : tooth.kind === 'incisor' ? 4 : 8;
            // Out of the mouth is the socket's -X on the upper row and +X on the lower, times
            // the side of the head.
            const outward = (row === 'upper' ? -1 : 1) * side * LEAN[tooth.kind] * length;
            // On several heads each row is a little coarser, so a hydra's teeth stay within the
            // part budget (gate 9).
            const coarse = ctx.copies > 1;
            const path = ctx.geo
              .arc(length, bend, { segments: coarse ? 3 : 4 })
              .map((q, i, all) => q.clone().setX(q.x + (outward * i) / (all.length - 1)));
            const piece = ctx.geo.sweep(path, (u) => base * (1 - u * 0.95), {
              sides: coarse ? 5 : 6,
              tip: 'point',
            });
            // A quarter of each tooth stands in the gum.
            ctx.emit(piece, socket, { color, tipColor: color, sink: length / 4 });
          }
        }
      }
      ctx.measure(largest, perRow);
    },
  },
});

interface Tooth {
  /** Along the gums: 0 at the tip, 1 at the corner. */
  readonly t: number;
  readonly kind: Kind;
  /** Length (metres) given the head's radius there. */
  length(radius: number): number;
}

/** A written `count`: evenly along the row, fangs first, as plan 1 placed them. */
function counted(p: Params, row: 'upper' | 'lower', L: number): Tooth[] {
  const count = p.count as number;
  return Array.from({ length: count }, (_, i) => {
    const t = 0.05 + ((i + 0.5) / count) * 0.85;
    const fang = i < p.fangs;
    return {
      t,
      kind: fang ? 'fang' : 'cheek',
      length: (r: number) => lengthOf(p, fang ? 'fang' : 'cheek', row, t, r, L),
    } satisfies Tooth;
  });
}

/** As many as fit: incisors, fangs, then cheek teeth shrinking toward the corner. */
function packed(
  p: Params,
  row: 'upper' | 'lower',
  rowLength: number,
  L: number,
  ctx: { mouth(t: number, row: 'upper' | 'lower', side: number): { radius: number } | undefined },
): Tooth[] {
  const out: Tooth[] = [];
  if (rowLength <= 0) return out;
  let s = rowLength * 0.03;
  const kinds: Kind[] = [
    ...Array<Kind>(p.incisors).fill('incisor'),
    ...Array<Kind>(p.fangs).fill('fang'),
  ];
  for (let i = 0; out.length < 40; i++) {
    const kind: Kind = kinds[i] ?? 'cheek';
    const t0 = s / rowLength;
    const r = ctx.mouth(Math.min(1, t0), row, 1)?.radius ?? 0;
    const length = lengthOf(p, kind, row, t0, r, L);
    const width = 2 * length * BASE[kind];
    const t = (s + width / 2) / rowLength;
    if (t > 0.95) break;
    out.push({ t, kind, length: (radius) => lengthOf(p, kind, row, t, radius, L) });
    s += width * (1 + p.spacing);
  }
  return out;
}

/**
 * A tooth's length (metres) at `t` along the row, where the head's radius is `r`: written
 * lengths are torso lengths `L` (plan 1's), otherwise shares of the head's radius.
 */
function lengthOf(
  p: Params,
  kind: Kind,
  row: 'upper' | 'lower',
  t: number,
  r: number,
  L: number,
): number {
  if (kind === 'fang') {
    const fang = p.fangLength !== undefined ? p.fangLength * L : r * p.fangScale * FANG;
    // Lower fangs are shorter, as in most jaws.
    return fang * (row === 'lower' ? 0.7 : 1);
  }
  if (kind === 'incisor') return r * p.scale * INCISOR;
  return p.length !== undefined
    ? p.length * L * (1 - t * 0.4)
    : r * p.scale * (CHEEK_FRONT + (CHEEK_BACK - CHEEK_FRONT) * t);
}
