import { colorRef, definePart, type PartBuildContext, type Spar } from '@spawnforge/core';
import { Vector3 } from 'three';
import { z } from 'zod';
import { alongSpar } from './_sheet.ts';

const params = z.strictObject({
  feathers: z.number().int().min(6).max(40).default(18).describe('Flight feathers per wing'),
  length: z
    .number()
    .min(0.3)
    .max(2)
    .default(1)
    .describe('Feather length relative to the wing bones: larger makes a broader wing'),
  tips: z
    .enum(['rounded', 'pointed', 'fingered'])
    .default('fingered')
    .describe('Wing tip: rounded, pointed (a falcon) or fingered (an eagle)'),
  color: colorRef('base').describe('Feather colour: a palette name or a colour'),
  tipColor: colorRef('accent').describe('Colour at the feather tips'),
});
type Params = z.output<typeof params>;

/** Cards per feather group: each group is one bone that folds back with the wing. */
const GROUP = 1;

interface Feather {
  /** Where it roots on the arm, and the arm bone there. */
  readonly root: Vector3;
  readonly bone: number;
  /** Unit direction in the wing plane, its length and width (metres). */
  readonly dir: Vector3;
  readonly length: number;
  readonly width: number;
  /** Share of the width ahead of the shaft (a primary's narrow outer vane). */
  readonly outer: number;
  /** Narrowed toward the tip, an eagle's emarginated primaries. */
  readonly finger: boolean;
  /** Offset along the wing's normal, so ranks and neighbours never z-fight. */
  readonly lift: number;
}

export default definePart({
  id: 'membrane.feather',
  summary: 'Overlapping flight feathers along the wing bones, for birds and griffins.',
  tags: ['wing', 'feather', 'bird'],
  slot: 'membrane',
  material: 'skin',
  attach: { on: 'limb' },
  provides: ['glide'],
  params,
  example: { type: 'membrane.feather', feathers: 20, tips: 'fingered' },
  describe: (_, { count }) => (count === 1 ? 'a feathered wing' : 'feathered wings'),
  hooks: {
    // A bird folds its arm in a Z against its side, as a bat does, with no fingers to close
    // (docs/design/9.3-wings-fins.md); its feathers fold back on their own bones.
    wing: () => ({
      bind: [-8, 12, -6],
      fold: { sweep: 90, droop: 0, lie: 0, joints: [165, -160, 0], digits: 0, flex: 0 },
      thickness: 0.01,
      // A bird's wing half-folds less than a bat's on the upstroke, so its feathers stay fanned.
      stroke: { amplitude: 45, flex: 0.25, twist: 12, plane: 0 },
    }),
    build(ctx, raw) {
      const p = raw as Params;
      const wing = ctx.wing;
      if (!wing) return;
      const arm = wing.arm;
      const n = arm.bones.length;
      if (n < 2) return;
      const unit = p.length * wing.armLength;
      const back = wing.lead.clone().negate();
      // The hand: the last arm bone; the inner arm: the bones from the shoulder to the wrist.
      const hand: Spar = {
        points: arm.points.slice(n - 1) as Vector3[],
        bones: arm.bones.slice(n - 1) as number[],
      };
      const forearm: Spar = {
        points: arm.points.slice(0, n) as Vector3[],
        bones: arm.bones.slice(0, n - 1) as number[],
      };
      const handDir = new Vector3()
        .subVectors(hand.points[1] as Vector3, hand.points[0] as Vector3)
        .normalize();
      const behind = back.clone().addScaledVector(handDir, -back.dot(handDir)).normalize();
      const fan = (deg: number) =>
        handDir
          .clone()
          .multiplyScalar(Math.cos((deg * Math.PI) / 180))
          .addScaledVector(behind, Math.sin((deg * Math.PI) / 180));
      const primaries = Math.max(4, Math.round(p.feathers * 0.4));
      const secondaries = Math.max(2, p.feathers - primaries);
      const step = 0.0012 * ctx.scale;
      const feathers: Feather[][] = [[], [], []];
      // Primaries: from the hand's tip (outermost, along the hand) to the wrist (pointing back).
      for (let k = 0; k < primaries; k++) {
        const u = k / (primaries - 1);
        const { point, weights } = alongSpar(hand, 1 - 0.92 * u);
        const outermost = Math.max(0, 1 - k / 5);
        const shape =
          p.tips === 'pointed'
            ? 1.05 - 0.35 * u + 0.1 * Math.max(0, 1 - Math.abs(k - 1))
            : p.tips === 'rounded'
              ? 0.85 + 0.15 * Math.sin(Math.PI * Math.min(1, 0.2 + u)) - 0.15 * u
              : 0.95 - 0.25 * u + 0.05 * outermost;
        const spread = p.tips === 'fingered' ? 6 * outermost : 0;
        (feathers[0] as Feather[]).push({
          root: point,
          bone: (weights[0] as [number, number])[0],
          dir: fan(12 + spread * (1 - u) + 66 * u ** 1.2),
          length: 0.58 * unit * shape,
          width: 0.075 * unit * (p.tips === 'pointed' ? 0.85 : 1),
          outer: 0.32,
          finger: p.tips === 'fingered' && k < 5,
          lift: (primaries - k) * step,
        });
      }
      // Secondaries along the forearm and tertials along the humerus, pointing back, shorter
      // toward the body.
      for (let k = 0; k < secondaries; k++) {
        const u = k / Math.max(1, secondaries - 1);
        const { point, weights } = alongSpar(forearm, 1 - 0.92 * u);
        (feathers[0] as Feather[]).push({
          root: point,
          bone: (weights[0] as [number, number])[0],
          dir: back
            .clone()
            .addScaledVector(handDir, 0.25 * (1 - u))
            .normalize(),
          length: 0.42 * unit * (1 - 0.35 * u * u),
          width: 0.085 * unit,
          outer: 0.4,
          finger: false,
          lift: (primaries + k) * step,
        });
      }
      // Two rows of coverts over the shafts' roots, on the hand and the forearm.
      for (const [rank, share] of [
        [1, 0.42],
        [2, 0.22],
      ] as const) {
        const count = Math.max(4, Math.round((primaries + secondaries) * 0.6));
        for (let k = 0; k < count; k++) {
          const u = k / (count - 1);
          const onHand = u < 0.35;
          const spar = onHand ? hand : forearm;
          const s = onHand ? 1 - u / 0.35 : 1 - (u - 0.35) / 0.65;
          const { point, weights } = alongSpar(spar, s);
          const dir = onHand ? fan(20 + 60 * (u / 0.35)) : back.clone();
          const base = onHand
            ? (feathers[0] as Feather[])[Math.round((u / 0.35) * (primaries - 1))]
            : (feathers[0] as Feather[])[
                primaries + Math.round(((u - 0.35) / 0.65) * (secondaries - 1))
              ];
          (feathers[rank] as Feather[]).push({
            root: point,
            bone: (weights[0] as [number, number])[0],
            dir,
            length: share * (base?.length ?? 0.4 * unit),
            width: 0.08 * unit,
            outer: 0.45,
            finger: false,
            lift: (rank * 6 + 1) * step * 3 + k * step * 0.5,
          });
        }
      }
      buildCards(ctx, wing.normal, feathers, {
        color: ctx.color(p.color, '#6b5a44'),
        tipColor: ctx.color(p.tipColor, ctx.color(p.color, '#6b5a44')),
      });
      ctx.measure(0.58 * unit, primaries + secondaries);
    },
  },
});

/** Each rank's feathers in groups of GROUP, each group on a bone, each feather a curved card. */
function buildCards(
  ctx: PartBuildContext,
  normal: Vector3,
  ranks: readonly Feather[][],
  look: { color: string; tipColor: string },
): void {
  const rows = Math.max(3, Math.round(6 * ctx.detail));
  for (const rank of ranks) {
    for (let g = 0; g < rank.length; g += GROUP) {
      const group = rank.slice(g, g + GROUP);
      const head = new Vector3();
      const dir = new Vector3();
      let length = 0;
      for (const f of group) {
        head.add(f.root);
        dir.add(f.dir);
        length += f.length;
      }
      head.divideScalar(group.length);
      dir.normalize();
      length /= group.length;
      const parent = (group[0] as Feather).bone;
      const bone = ctx.featherBone(parent, head, head.clone().addScaledVector(dir, length), normal);
      const positions: Vector3[] = [];
      const normals: Vector3[] = [];
      const indices: number[] = [];
      const along: number[] = [];
      const across: number[] = [];
      for (const f of group) {
        const base = positions.length;
        const side = new Vector3().crossVectors(normal, f.dir).normalize();
        for (let i = 0; i < rows; i++) {
          const s = i / (rows - 1);
          // A leaf: full width from a fifth of the way, rounding off at the tip; fingered
          // primaries narrow from 60% on. It curves down a little toward its tip.
          let half = f.width * Math.min(1, 0.45 + 2.75 * s) * Math.sqrt(Math.max(0, 1 - s ** 4));
          if (f.finger && s > 0.6) half *= 1 - 0.55 * Math.min(1, (s - 0.6) / 0.25);
          const centre = f.root
            .clone()
            .addScaledVector(f.dir, s * f.length)
            .addScaledVector(normal, f.lift - 0.05 * f.length * s * s);
          for (const [c, w] of [
            [0, f.outer],
            [0.5, 0],
            [1, 1 - f.outer],
          ] as const) {
            const offset = c === 0 ? -w : c === 1 ? w : 0;
            positions.push(
              centre
                .clone()
                .addScaledVector(side, -offset * half * 2)
                .addScaledVector(normal, c === 0.5 ? 0.004 * f.length : 0),
            );
            normals.push(normal.clone());
            along.push(s);
            across.push(c);
          }
        }
        for (let i = 0; i + 1 < rows; i++)
          for (let c = 0; c < 2; c++) {
            const a = base + i * 3 + c;
            const b = base + (i + 1) * 3 + c;
            indices.push(a, b, b + 1, a, b + 1, a + 1);
          }
      }
      // Wound to face the wing's normal.
      const p0 = positions[indices[0] as number] as Vector3;
      const p1 = positions[indices[1] as number] as Vector3;
      const p2 = positions[indices[2] as number] as Vector3;
      const facing = new Vector3()
        .subVectors(p1, p0)
        .cross(new Vector3().subVectors(p2, p0))
        .dot(normal);
      const wound =
        facing < 0 ? indices.map((_, i, a) => a[i - (i % 3) + (2 - (i % 3))] as number) : indices;
      ctx.sheet(
        positions,
        normals,
        wound,
        positions.map(() => [[bone, 1]] as [number, number][]),
        along,
        across,
        { ...look, opacity: 1, translucency: 0.15, roughness: 0.85, veins: 0.5 },
      );
    }
  }
}
