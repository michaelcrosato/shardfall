import type { PartBuildContext, PartChain, Socket } from '@spawnforge/core';
import { Vector3 } from 'three';
import { solidOn } from './_solid.ts';

/**
 * A spine of a fan (a frill, a hood, a sail), on a bone of its own that the flare turns open
 * (docs/design/9.5-coverings.md).
 */
export interface FanSpine {
  /** Where it leaves the skin, and the skin's weights there. */
  readonly socket: Socket;
  /** The way it points fully open (unit). */
  readonly open: Vector3;
  /** The way it folds at rest, back along the body (unit, across `open`). */
  readonly fold: Vector3;
  /** How far it is folded at rest, as a share of a right angle (0 open). */
  readonly folded: number;
  readonly length: number;
  /** Radius at its base (metres). */
  readonly radius: number;
}

/** The bone a socket's skin moves with most. */
export function mainBone(socket: Socket): number {
  let best = socket.weights[0]?.[0] ?? 0;
  let most = -1;
  for (const [b, w] of socket.weights)
    if (w > most) {
      most = w;
      best = b;
    }
  return best;
}

/** `fold` made square to `open`. */
function foldOf(s: FanSpine): Vector3 {
  const f = s.fold.clone().addScaledVector(s.open, -s.fold.dot(s.open));
  return f.lengthSq() > 1e-12 ? f.normalize() : new Vector3(0, 0, -1);
}

/** A spine's direction at rest: turned from `open` toward `fold` by `folded` of a right angle. */
function restDirection(s: FanSpine): Vector3 {
  const a = (s.folded * Math.PI) / 2;
  return s.open
    .clone()
    .multiplyScalar(Math.cos(a))
    .addScaledVector(foldOf(s), Math.sin(a))
    .normalize();
}

/**
 * One bone per spine, from its base along its rest direction, turning open about the line across
 * `fold` and `open` at full flare.
 */
export function fanChains(spines: readonly FanSpine[]): PartChain[] {
  return spines.map((s) => {
    const dir = restDirection(s);
    // The turn's axis is the bone's X: fold × open. Z = X × Y makes X = Y × Z.
    const axis = new Vector3().crossVectors(foldOf(s), s.open).normalize();
    return {
      points: [s.socket.position.clone(), s.socket.position.clone().addScaledVector(dir, s.length)],
      parent: mainBone(s.socket),
      up: new Vector3().crossVectors(axis, dir).normalize(),
      radii: [s.radius, s.radius * 0.3],
      drive: 'flare',
      pose: [(s.folded * Math.PI) / 2],
    } satisfies PartChain;
  });
}

/** How the skin between spines looks. */
export interface FanLook {
  readonly color: string;
  readonly tipColor?: string;
  readonly spineColor: string;
  /** How far the edge between two spine tips dips toward the base, 0 to 1. */
  readonly scallop: number;
  readonly translucency: number;
}

/**
 * Builds the spines (rigid on their bones) and the skin between each pair in `pairs` as sheets
 * in the membrane mesh, held to the body at the base and carried by the two spines' bones
 * further out.
 */
export function buildFan(
  ctx: PartBuildContext,
  spines: readonly FanSpine[],
  pairs: readonly (readonly [number, number])[],
  look: FanLook,
): void {
  const chains = ctx.chains;
  if (chains.length < spines.length) return;
  const sides = Math.max(4, Math.round(6 * ctx.detail));
  spines.forEach((s, i) => {
    const chain = chains[i];
    if (!chain) return;
    const piece = ctx.geo.sweep(chain.points, (t) => s.radius * (1 - 0.75 * t), {
      sides,
      tip: 'point',
    });
    solidOn(ctx, piece, { color: look.spineColor, roughness: 0.5 }, { bone: chain.bones[0] });
  });
  const rows = Math.max(4, Math.round(8 * ctx.detail));
  const cols = Math.max(2, Math.round(4 * ctx.detail));
  for (const [i, j] of pairs) {
    const a = chains[i];
    const b = chains[j];
    const sa = spines[i];
    const sb = spines[j];
    if (!a || !b || !sa || !sb) continue;
    const a0 = a.points[0] as Vector3;
    const a1 = a.points[1] as Vector3;
    const b0 = b.points[0] as Vector3;
    const b1 = b.points[1] as Vector3;
    const positions: Vector3[] = [];
    const along: number[] = [];
    const across: number[] = [];
    const weights: [number, number][][] = [];
    for (let r = 0; r <= rows; r++) {
      for (let c = 0; c <= cols; c++) {
        const v = c / cols;
        // The edge between two tips dips toward the base.
        const u = (r / rows) * (1 - look.scallop * Math.sin(Math.PI * v));
        const pa = a0.clone().lerp(a1, u);
        const pb = b0.clone().lerp(b1, u);
        positions.push(pa.lerp(pb, v));
        along.push(u);
        across.push(v);
        // Held by the body at the base, by the spines further out.
        const held = (1 - Math.min(1, u / 0.25)) ** 2;
        const w: [number, number][] = [
          [a.bones[0] as number, (1 - v) * (1 - held)],
          [b.bones[0] as number, v * (1 - held)],
        ];
        const skin = v < 0.5 ? sa.socket.weights : sb.socket.weights;
        for (const [bone, k] of skin) w.push([bone, k * held]);
        weights.push(w.filter(([, k]) => k > 1e-6));
      }
    }
    const indices: number[] = [];
    const stride = cols + 1;
    for (let r = 0; r < rows; r++)
      for (let c = 0; c < cols; c++) {
        const p = r * stride + c;
        indices.push(p, p + stride, p + 1, p + 1, p + stride, p + stride + 1);
      }
    const normals = positions.map((_, k) => {
      const r = Math.floor(k / stride);
      const c = k % stride;
      const du = new Vector3().subVectors(
        positions[Math.min(rows, r + 1) * stride + c] as Vector3,
        positions[Math.max(0, r - 1) * stride + c] as Vector3,
      );
      const dv = new Vector3().subVectors(
        positions[r * stride + Math.min(cols, c + 1)] as Vector3,
        positions[r * stride + Math.max(0, c - 1)] as Vector3,
      );
      const n = new Vector3().crossVectors(dv, du);
      return n.lengthSq() > 1e-16 ? n.normalize() : new Vector3(0, 1, 0);
    });
    ctx.sheet(positions, normals, indices, weights, along, across, {
      color: look.color,
      ...(look.tipColor ? { tipColor: look.tipColor } : {}),
      opacity: 1,
      translucency: look.translucency,
      roughness: 0.6,
      veins: 0.25,
    });
  }
}
