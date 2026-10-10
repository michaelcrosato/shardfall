import { colorRef, definePart, type MeshPiece, type Socket } from '@spawnforge/core';
import { Vector3 } from 'three';
import { z } from 'zod';

const params = z.strictObject({
  shape: z
    .enum(['hooked', 'straight', 'broad'])
    .default('hooked')
    .describe('"hooked" like an eagle, "straight" like a heron, "broad" like a duck'),
  length: z
    .number()
    .min(0.2)
    .max(2)
    .default(1)
    .describe('How far the beak runs on past the snout, relative to the snout'),
  depth: z.number().min(0.2).max(2).default(1).describe('How deep and heavy the beak is'),
  color: colorRef('#d8b040').describe('Beak colour: a palette name or a colour'),
  tipColor: colorRef('#3a3020').describe('Colour at the tip'),
});
type Params = z.output<typeof params>;

/** Mouth positions the beak covers, from its back edge to near the snout's tip. */
const RINGS = [0.85, 0.66, 0.48, 0.32, 0.19, 0.09];
/** Rings past the snout. */
const PAST = 6;
/** Points across a ring, from the left lip line round to the right. */
const ACROSS = 13;

export default definePart({
  id: 'beak',
  summary:
    'A horny beak in two halves over the snout, upper on the head and lower on the jaw, for birds and griffins.',
  tags: ['head', 'mouth', 'bird'],
  slot: 'mouth',
  material: 'horn',
  attach: { on: 'head' },
  params,
  example: { id: 'beak', type: 'beak', params: { shape: 'hooked' } },
  describe: (p) => `a ${p.shape as string} beak`,
  hooks: {
    build(ctx, raw) {
      const p = raw as Params;
      const color = ctx.color(p.color, '#d8b040');
      const tipColor = ctx.color(p.tipColor, '#3a3020');
      let largest = 0;
      for (const row of ['upper', 'lower'] as const) {
        const half = shell(ctx, p, row);
        if (!half) return;
        largest = Math.max(largest, half.length);
        ctx.emit(half.piece, half.socket, { color, tipColor });
      }
      ctx.measure(largest, 1);
    },
  },
});

/**
 * One half of the beak, in model space: a closed shell whose cross-sections are crescents, an
 * outer arc standing proud of the skin and an inner one just inside it, so the half is solid
 * past the snout where nothing lies under it.
 */
function shell(
  ctx: {
    around(t: number, row: 'upper' | 'lower', angle: number): Socket | undefined;
  },
  p: Params,
  row: 'upper' | 'lower',
): { piece: MeshPiece; socket: Socket; length: number } | undefined {
  const sockets = RINGS.map((t) =>
    Array.from({ length: ACROSS }, (_, i) => ctx.around(t, row, (180 * i) / (ACROSS - 1))),
  );
  const first = sockets[0]?.[0];
  if (!first || sockets.some((ring) => ring.some((s) => !s))) return undefined;
  const r = first.radius;
  const forward = first.forward.clone();
  const thick = 0.12 * r * p.depth;
  // Each ring: outer and inner arcs, and the ring's middle.
  const outer: Vector3[][] = [];
  const inner: Vector3[][] = [];
  sockets.forEach((ring, k) => {
    // Thickest over the middle of the snout, thinner toward the back edge.
    const grow = 0.5 + 0.5 * Math.min(1, (RINGS.length - 1 - k) / 2 + 0.4);
    outer.push(
      ring.map((s) =>
        (s as Socket).position.clone().addScaledVector((s as Socket).normal, thick * grow),
      ),
    );
    inner.push(
      ring.map((s) =>
        (s as Socket).position.clone().addScaledVector((s as Socket).normal, -thick * 0.4),
      ),
    );
  });
  // Past the snout: rings shrink about their middles toward the tip and, hooked, curl down.
  const tipOuter = outer.at(-1) as Vector3[];
  const tipInner = inner.at(-1) as Vector3[];
  const middle = tipOuter.reduce((a, v) => a.add(v), new Vector3()).divideScalar(ACROSS);
  const back = (outer[0] as Vector3[])
    .reduce((a, v) => a.add(v), new Vector3())
    .divideScalar(ACROSS);
  const snout = Math.max(r * 0.5, middle.clone().sub(back).dot(forward));
  // The lower half ends under the upper one's hook.
  const reach =
    snout * 0.6 * p.length * (row === 'lower' ? (p.shape === 'hooked' ? 0.4 : 0.85) : 1);
  const up = new Vector3().subVectors(tipOuter[Math.floor(ACROSS / 2)] as Vector3, middle);
  up.addScaledVector(forward, -up.dot(forward)).normalize();
  for (let j = 1; j <= PAST; j++) {
    const u = j / PAST;
    const shrink = p.shape === 'broad' ? 1 - 0.55 * u : (1 - u) ** 1.1 * 0.95 + 0.05;
    const curl = p.shape === 'hooked' && row === 'upper' ? u * u * reach * 0.7 : 0;
    const ahead = p.shape === 'hooked' && row === 'upper' ? reach * (u - 0.25 * u * u) : reach * u;
    const centre = middle.clone().addScaledVector(forward, ahead).addScaledVector(up, -curl);
    const at = (v: Vector3) => {
      const off = new Vector3().subVectors(v, middle);
      const vertical = off.dot(up);
      const flat = off.addScaledVector(up, -vertical);
      const wide = p.shape === 'broad' ? 1 + 0.35 * u : 1;
      const low = p.shape === 'broad' ? 1 - 0.4 * u : 1;
      return centre
        .clone()
        .addScaledVector(flat, shrink * wide)
        .addScaledVector(up, vertical * shrink * low);
    };
    outer.push(tipOuter.map(at));
    inner.push(tipInner.map(at));
  }
  const piece = crescentTube(outer, inner);
  // Built in model space: a socket at the origin with model axes, moving with the half's bone.
  const socket: Socket = {
    position: new Vector3(),
    normal: new Vector3(0, 1, 0),
    forward: new Vector3(0, 0, 1),
    side: new Vector3(1, 0, 0),
    radius: r,
    weights: first.weights,
  };
  return { piece, socket, length: snout + reach };
}

/**
 * A closed tube through rings of crescents (outer arc, then the inner arc back), capped at both
 * ends. `t` runs 0 at the back to 1 at the tip, for the colour.
 */
function crescentTube(outer: Vector3[][], inner: Vector3[][]): MeshPiece {
  const piece: MeshPiece = { positions: [], normals: [], indices: [], t: [] };
  const rings = outer.length;
  const loop = (k: number) => [
    ...(outer[k] as Vector3[]),
    ...[...(inner[k] as Vector3[])].reverse(),
  ];
  const size = 2 * ACROSS;
  for (let k = 0; k < rings; k++)
    for (const v of loop(k)) {
      piece.positions.push(v.x, v.y, v.z);
      piece.normals.push(0, 0, 0);
      piece.t.push(k / (rings - 1));
    }
  const at = (k: number, i: number) => k * size + (i % size);
  for (let k = 0; k + 1 < rings; k++)
    for (let i = 0; i < size; i++)
      piece.indices.push(
        at(k, i),
        at(k + 1, i),
        at(k + 1, i + 1),
        at(k, i),
        at(k + 1, i + 1),
        at(k, i + 1),
      );
  // Caps: fans round each end's middle.
  for (const [k, flip] of [
    [0, true],
    [rings - 1, false],
  ] as const) {
    const c = piece.positions.length / 3;
    const mid = loop(k)
      .reduce((a, v) => a.add(v), new Vector3())
      .divideScalar(size);
    piece.positions.push(mid.x, mid.y, mid.z);
    piece.normals.push(0, 0, 0);
    piece.t.push(k / (rings - 1));
    for (let i = 0; i < size; i++)
      if (flip) piece.indices.push(c, at(k, i + 1), at(k, i));
      else piece.indices.push(c, at(k, i), at(k, i + 1));
  }
  orientOutward(piece);
  return piece;
}

/** Winds the piece so its faces point away from its centre, then smooths normals. */
function orientOutward(piece: MeshPiece): void {
  const P = (i: number) =>
    new Vector3(
      piece.positions[i * 3] as number,
      piece.positions[i * 3 + 1] as number,
      piece.positions[i * 3 + 2] as number,
    );
  const centre = new Vector3();
  const n = piece.positions.length / 3;
  for (let i = 0; i < n; i++) centre.add(P(i));
  centre.divideScalar(n);
  // Signed volume about the centre: negative means the faces point inward.
  let volume = 0;
  for (let t = 0; t < piece.indices.length; t += 3) {
    const a = P(piece.indices[t] as number).sub(centre);
    const b = P(piece.indices[t + 1] as number).sub(centre);
    const c = P(piece.indices[t + 2] as number).sub(centre);
    volume += a.dot(b.cross(c));
  }
  if (volume < 0)
    for (let t = 0; t < piece.indices.length; t += 3) {
      const x = piece.indices[t + 1] as number;
      piece.indices[t + 1] = piece.indices[t + 2] as number;
      piece.indices[t + 2] = x;
    }
  for (let t = 0; t < piece.indices.length; t += 3) {
    const ia = piece.indices[t] as number;
    const ib = piece.indices[t + 1] as number;
    const ic = piece.indices[t + 2] as number;
    const a = P(ia);
    const nrm = P(ib).sub(a).cross(P(ic).sub(a));
    for (const v of [ia, ib, ic]) {
      piece.normals[v * 3] = (piece.normals[v * 3] as number) + nrm.x;
      piece.normals[v * 3 + 1] = (piece.normals[v * 3 + 1] as number) + nrm.y;
      piece.normals[v * 3 + 2] = (piece.normals[v * 3 + 2] as number) + nrm.z;
    }
  }
  for (let v = 0; v < n; v++) {
    const len =
      Math.hypot(
        piece.normals[v * 3] as number,
        piece.normals[v * 3 + 1] as number,
        piece.normals[v * 3 + 2] as number,
      ) || 1;
    for (let k = 0; k < 3; k++)
      piece.normals[v * 3 + k] = (piece.normals[v * 3 + k] as number) / len;
  }
}
