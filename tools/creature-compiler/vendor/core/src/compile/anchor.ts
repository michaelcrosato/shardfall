import { Vector3 } from 'three';
import type { CompiledCreature, SectionData, Vec3 } from './compile.ts';
import { marchToSurface } from './parts.ts';
import { SdfEvaluator } from './sdf.ts';
import { aroundDirection, type PathSegment, samplePath } from './skeleton.ts';
import type { BoneDef } from './types.ts';

/**
 * Placing parts by a point on the skin (docs/design/12.1-placing.md): `anchorAt` is the inverse
 * of socket placement, `placeOnSkin` the placement itself, read from the compiled creature.
 */

/** Where a part attaches, as a blueprint's `attach` writes it. */
export interface Anchor {
  /** A body section (`head`), a limb pair (`foreleg`, both limbs), or one limb (`foreleg.L`). */
  readonly on: string;
  readonly at: number;
  readonly angle: number;
  /** The side the point is on; `both` on a limb pair, whose copies follow each limb. */
  readonly side: 'left' | 'right' | 'center' | 'both';
}

/** An anchor, and the instance it was read on: what `placeOnSkin` takes to land on the point. */
export interface AnchorResult extends Anchor {
  /** The section or limb instance the point is on (`foreleg.L`, `head.L1`). */
  readonly section: string;
  /** The copy's mirror on that instance (1, -1 or 0), as validation expands `side`. */
  readonly mirror: 1 | -1 | 0;
}

/** Points within this many degrees of the top or the belly sit on the midline. */
const CENTER = 3;
const DEG = Math.PI / 180;

const vec = (v: Vec3) => new Vector3(v[0], v[1], v[2]);

/** A section's bones as `samplePath` reads them, and its path. */
const sampled = new WeakMap<SectionData, { bones: BoneDef[]; path: PathSegment[] }>();
function geometry(section: SectionData) {
  let g = sampled.get(section);
  if (!g) {
    const bones: BoneDef[] = [];
    for (const s of section.segments)
      bones[s.bone] = {
        head: vec(s.head),
        tail: vec(s.tail),
        up: vec(s.up),
        r0: s.r0,
        r1: s.r1,
        cross: s.cross,
      } as BoneDef;
    g = { bones, path: section.segments.map((s) => ({ bone: s.bone, t0: s.t0, t1: s.t1 })) };
    sampled.set(section, g);
  }
  return g;
}

/** The section's frame at `at`, and the sign that turns `angle` toward a copy's side. */
function frameAt(section: SectionData, at: number, mirror: number) {
  const { bones, path } = geometry(section);
  const frame = samplePath(bones, path, at);
  // As part placement does: on limbs, angle 90 points away from the body on either side.
  let m = mirror;
  if (section.kind === 'limb') {
    const left = new Vector3().crossVectors(frame.up, frame.forward);
    m = (mirror || 1) * Math.sign(left.x * (section.mirror || 1) || 1);
  }
  return { frame, m };
}

const evaluators = new WeakMap<object, SdfEvaluator>();

/**
 * Where a part's socket lands: on `section` (an instance, `foreleg.L`) at `at` and `angle`, the
 * copy's `mirror` as validation expands it. Needs the distance field (`compile` with `field`).
 */
export function placeOnSkin(
  compiled: CompiledCreature,
  place: {
    readonly section: string;
    readonly at: number;
    readonly angle: number;
    readonly mirror: number;
  },
): { position: Vector3; normal: Vector3 } {
  const section = compiled.sections[place.section];
  if (!section) throw new Error(`no section "${place.section}"`);
  if (!compiled.field) throw new Error('placeOnSkin needs the distance field: compile with field');
  let evaluator = evaluators.get(compiled.field);
  if (!evaluator) {
    evaluator = new SdfEvaluator(compiled.field);
    evaluators.set(compiled.field, evaluator);
  }
  const { frame, m } = frameAt(section, place.at, place.mirror);
  const dir = aroundDirection(frame, place.angle, m);
  return marchToSurface(evaluator, frame.point, dir, frame.radius * Math.max(...frame.cross));
}

/** The instance a skin point belongs to: its nearest skin vertex's strongest bone's owner. */
function ownerOf(compiled: CompiledCreature, p: Vector3): string | undefined {
  const { positions, skinIndex, skinWeight } = compiled.skin;
  let best = -1;
  let bestD = Number.POSITIVE_INFINITY;
  for (let v = 0; v < positions.length / 3; v++) {
    const dx = (positions[v * 3] as number) - p.x;
    const dy = (positions[v * 3 + 1] as number) - p.y;
    const dz = (positions[v * 3 + 2] as number) - p.z;
    const d = dx * dx + dy * dy + dz * dz;
    if (d < bestD) {
      bestD = d;
      best = v;
    }
  }
  if (best < 0) return undefined;
  let bone = -1;
  let weight = -1;
  for (let k = 0; k < 4; k++)
    if ((skinWeight[best * 4 + k] as number) > weight) {
      weight = skinWeight[best * 4 + k] as number;
      bone = skinIndex[best * 4 + k] as number;
    }
  return compiled.bones.owners[bone];
}

/**
 * The point's place along each bone of a section: its nearest point on that bone's stretch of
 * the centreline, and how far off. A path's end bones run on past their ends, as `samplePath`
 * extrapolates there (a head's bone stops short of the snout's `at` 0).
 */
function along(section: SectionData, p: Vector3): { at: number; distance: number }[] {
  const ts = section.segments.flatMap((s) => [s.t0, s.t1]);
  const lo = Math.min(...ts);
  const hi = Math.max(...ts);
  const a = new Vector3();
  const b = new Vector3();
  const axis = new Vector3();
  return section.segments.map((s) => {
    a.set(...s.head);
    b.set(...s.tail);
    const ab = b.clone().sub(a);
    const raw = p.clone().sub(a).dot(ab) / Math.max(ab.lengthSq(), 1e-12);
    // How far past each end of this bone `at` may run: to 0 or 1, at the path's own ends.
    const past = (t: number) =>
      (t === lo ? lo : t === hi ? 1 - hi : 0) / Math.abs(s.t1 - s.t0 || 1);
    const f = Math.min(1 + past(s.t1), Math.max(-past(s.t0), raw));
    axis.copy(a).addScaledVector(ab, f);
    return { at: Math.min(1, Math.max(0, s.t0 + (s.t1 - s.t0) * f)), distance: axis.distanceTo(p) };
  });
}

/** The name a blueprint attaches by: a limb pair's id, the main head for every head. */
const baseOf = (section: string, data: SectionData) =>
  data.kind === 'limb'
    ? section.replace(/\.(L|R)$/, '')
    : section.replace(/^(neck|head|jaw|tail)\.[LR]\d+$/, '$1');

/**
 * The attachment that places a part at `point` (bind pose, model space, metres): on the section
 * the point's skin belongs to (or `options.on`'s, to keep one while dragging), `at` along its
 * centreline and `angle` around it. Placing a part there (`placeOnSkin` with the result's
 * `section` and `mirror`) lands on the point. Inside a bend (the back of a knee) a point lies
 * off two bones; with the distance field (`compile` with `field`) the one whose placement
 * reaches the point wins, else the nearest.
 */
export function anchorAt(
  compiled: CompiledCreature,
  point: Vec3 | Vector3,
  options: { readonly on?: string } = {},
): AnchorResult | undefined {
  const p = point instanceof Vector3 ? point.clone() : vec(point);
  const names = Object.keys(compiled.sections);
  let candidates: string[];
  if (options.on !== undefined) {
    const on = options.on;
    candidates = names.filter(
      (n) => n === on || baseOf(n, compiled.sections[n] as SectionData) === on,
    );
  } else {
    const owner = ownerOf(compiled, p);
    candidates = owner !== undefined && compiled.sections[owner] ? [owner] : names;
  }
  // Each bone's nearest centreline point, nearest first relative to the section's thickness.
  const hits = candidates
    .flatMap((section) => {
      const data = compiled.sections[section] as SectionData;
      return along(data, p).map((hit) => ({
        section,
        at: hit.at,
        score: hit.distance / Math.max(frameAt(data, hit.at, 1).frame.radius, 1e-4),
      }));
    })
    .sort((a, b) => a.score - b.score || (a.section < b.section ? -1 : 1));
  const first = hits[0];
  if (!first) return undefined;
  if (compiled.field) {
    const reach = 1e-3;
    for (const hit of hits.slice(0, 8)) {
      const anchor = anchorOn(compiled, hit.section, hit.at, p);
      if (placeOnSkin(compiled, anchor).position.distanceTo(p) < reach) return anchor;
    }
  }
  return anchorOn(compiled, first.section, first.at, p);
}

/** The anchor on `section` at `at` whose direction around the centreline points at `p`. */
function anchorOn(
  compiled: CompiledCreature,
  section: string,
  at: number,
  p: Vector3,
): AnchorResult {
  const data = compiled.sections[section] as SectionData;
  // Its direction around the centreline, in the frame placement turns angles in.
  const { frame } = frameAt(data, at, 1);
  const dir = p.clone().sub(frame.point);
  dir.addScaledVector(frame.forward, -dir.dot(frame.forward));
  if (dir.lengthSq() < 1e-14) dir.copy(frame.up);
  dir.normalize();
  const left = new Vector3().crossVectors(frame.up, frame.forward).normalize();
  const x = dir.dot(left);
  const y = dir.dot(frame.up);
  const round = (v: number) => Number(v.toFixed(4));
  const base = baseOf(section, data);
  // The sign `frameAt` gives a copy of mirror 1 on this instance.
  const s = data.kind === 'limb' ? Math.sign(left.x * (data.mirror || 1) || 1) : 1;
  const center = (a: number) => Math.abs(a) < CENTER || Math.abs(a) > 180 - CENTER;
  if (data.kind === 'limb' && data.mirror !== 0) {
    // A limb of a pair: copies on `base` follow each limb, turned by the limb's own mirror.
    const a = Math.atan2(x * data.mirror * s, y) / DEG;
    if (center(a))
      return {
        on: base,
        at: round(at),
        angle: Math.abs(a) < 90 ? 0 : 180,
        side: 'both',
        section,
        mirror: data.mirror,
      };
    if (a > 0)
      return {
        on: base,
        at: round(at),
        angle: round(a),
        side: 'both',
        section,
        mirror: data.mirror,
      };
    // The other half of the limb: only this limb, from the other side.
    const mirror = -data.mirror as 1 | -1;
    return {
      on: section,
      at: round(at),
      angle: round(-a),
      side: mirror === 1 ? 'left' : 'right',
      section,
      mirror,
    };
  }
  const a = Math.atan2(x * s, y) / DEG;
  if (center(a))
    return {
      on: base,
      at: round(at),
      angle: Math.abs(a) < 90 ? 0 : 180,
      side: 'center',
      section,
      mirror: 0,
    };
  return {
    on: base,
    at: round(at),
    angle: round(Math.abs(a)),
    side: a > 0 ? 'left' : 'right',
    section,
    mirror: a > 0 ? 1 : -1,
  };
}
