import { type Quaternion, Vector3 } from 'three';
import { hexToRgb, toHex } from '../blueprint/colors.ts';
import type { Area, PartSpec } from '../blueprint/creature.ts';
import { type GeometryKit, geometryKit, type MeshPiece, mirrorX } from '../geometry/kit.ts';
import type { PartMaterial, PartModule, Registry } from '../registry.ts';
import { createRng, type Rng } from '../rng.ts';
import { gumPoint, type MouthShape, outlineAt } from './head.ts';
import { lidAngles, lidMesh } from './lids.ts';
import {
  type MembraneLook,
  MembraneSink,
  rigidSheet,
  type Spar,
  stationPanel,
} from './membranes.ts';
import { type Sdf, SdfEvaluator } from './sdf.ts';
import { aroundDirection, type PathSegment, samplePath } from './skeleton.ts';
import { type WeightOptions, weightsAt } from './skin.ts';
import type { PrimCulling } from './surface-nets.ts';
import type { BoneDef, DrivenChain } from './types.ts';
import { bindRotation as bindRotationOf, type WingHooks } from './wings.ts';

/** A place on the creature where a part sits, with its frame and skin weights. */
export interface Socket {
  readonly position: Vector3;
  /** Out of the skin. */
  readonly normal: Vector3;
  /** Toward the section's `at` = 0 end (snout-ward on the body, toward the root on limbs). */
  readonly forward: Vector3;
  /** normal × forward: the creature's left on the body. */
  readonly side: Vector3;
  /** Section radius here (metres). */
  readonly radius: number;
  /** Skin weights here, so the part moves with the skin. */
  readonly weights: readonly (readonly [number, number])[];
}

export interface EyeOptions {
  readonly iris: string;
  readonly sclera: string;
  readonly pupil: 'round' | 'slit' | 'goat';
  /** Iris size as a share of the visible eye (0.2–1). */
  readonly irisSize: number;
  /** Eyeball radius (metres). */
  readonly radius: number;
  /** Eyelids that blink (docs/design/8.3-heads.md); none without. */
  readonly lids?: boolean;
  /** How far the upper lid hangs over the eye at rest (0–1). */
  readonly squint?: number;
}

export interface EmitOptions {
  readonly color?: string;
  /** Colour at the tip (t = 1); blends from `color` along t². */
  readonly tipColor?: string;
  readonly material?: PartMaterial;
  /** How far to sink the piece into the skin along -normal (metres). */
  readonly sink?: number;
  /** Bind rigidly to this bone instead of the socket's weights. */
  readonly bone?: number;
  /** Emit an eye: it goes in the eye mesh, on a bone of its own that looks around. */
  readonly eye?: EyeOptions;
  /** Register this piece's centreline (local points and radii) so other parts can attach to it. */
  readonly path?: { readonly points: readonly Vector3[]; readonly radii: readonly number[] };
  /**
   * Emit into the membrane mesh instead (double-sided, lit through from behind): fins, frills,
   * insect wings (docs/design/9.3-wings-fins.md). `color` and `tipColor` still apply.
   */
  readonly membrane?: Omit<MembraneLook, 'color' | 'tipColor'>;
}

/** What a membrane module sees of its wing or fin (docs/design/9.3-wings-fins.md), bind pose. */
export interface WingContext {
  readonly role: 'wing' | 'fin';
  /** The arm: root, elbow, wrist, the hand's tip, on the arm bones. */
  readonly arm: Spar;
  /**
   * Each digit from its root to its tip: from the wrist, or (digit 0) the hand and its
   * phalanges from the wrist on.
   */
  readonly digits: readonly Spar[];
  /** The body's side from the root back, then down the nearest leg behind to the knee. */
  readonly flank: Spar | undefined;
  /** The same without the leg. */
  readonly body: Spar | undefined;
  /** The wing's plane: straight out, toward the leading edge, its normal. */
  readonly out: Vector3;
  readonly lead: Vector3;
  readonly normal: Vector3;
  readonly armLength: number;
  readonly tipRadius: number;
}

/** A wing or fin as the skeleton built it (bind pose), for its membrane's context. */
export interface SpanLimb {
  readonly role: 'wing' | 'fin';
  readonly on: string;
  readonly at: number;
  readonly angle: number;
  readonly mirror: 1 | -1 | 0;
  readonly out: Vector3;
  readonly lead: Vector3;
  readonly normal: Vector3;
  readonly bones: readonly number[];
  readonly digits: readonly { readonly bones: readonly number[]; readonly root: 'wrist' | 'tip' }[];
  readonly armLength: number;
  readonly tipRadius: number;
  /** The nearest leg behind on the same side: its `at` on the torso and its bones. */
  readonly legBehind: { readonly at: number; readonly bones: readonly number[] } | undefined;
}

/** Options for a membrane panel between two spars. */
export interface PanelLook extends MembraneLook {
  /** Vertices along and across (scaled down at low quality). */
  readonly rows?: number;
  readonly cols?: number;
  /** How far the free edge dips between the spars, 0 to 1. */
  readonly scallop?: number;
}

/** What a part module's `build` hook gets. */
export interface PartBuildContext {
  readonly id: string;
  readonly baseId: string;
  readonly type: string;
  /**
   * How many copies of this part the creature has, one per head it is copied to (1 for most):
   * a part on five heads may build each copy coarser to stay within the part budget.
   */
  readonly copies: number;
  /** Metres per torso length. */
  readonly scale: number;
  readonly mirror: 1 | -1 | 0;
  readonly at: number;
  readonly from: number;
  readonly to: number;
  readonly angle: number;
  readonly rng: Rng;
  readonly geo: GeometryKit;
  /** Resolves a colour parameter: a palette name or a colour, to `#rrggbb`. */
  color(value: unknown, fallback: string): string;
  /** A socket on the part's target (defaults to its own `at` and `angle`). */
  socket(at?: number, angle?: number): Socket;
  /**
   * A socket on the gums: t = 0 at the tip, 1 at the corner (by length along them); side ±1.
   * Its normal points into the opening, and its radius is the skull's, so teeth size to the
   * head. Mouth parts only; undefined without a jaw.
   */
  mouth(t: number, row: 'upper' | 'lower', side: number): Socket | undefined;
  /**
   * The skin around the snout at mouth position `t` (0 tip, 1 corner): `angle` 0 is the lip
   * line on the head's left, 90 the top of the snout (upper) or the chin (lower), 180 the lip
   * line on the right. Its normal is the skin's; it moves with the head (upper) or the jaw
   * (lower), and its radius is the skull's. Mouth parts only (a beak); undefined without a jaw.
   */
  around(t: number, row: 'upper' | 'lower', angle: number): Socket | undefined;
  /** Toe tips, for foot parts: normal along the toe, forward up. */
  readonly toes: readonly (Socket & {
    readonly bone: number;
    readonly length: number;
    readonly toeRadius: number;
    /** The toe's bones, root to tip, and its joints (rest pose, model space; ground at y = 0). */
    readonly bones: readonly number[];
    readonly points: readonly Vector3[];
    /** The limb bone the toe grows from (the ankle's). */
    readonly limbBone: number;
  })[];
  /**
   * A socket at any point, moving rigidly with `bone`: for foot parts that place pads, nails and
   * sheaths themselves. `forward` is projected off `normal`.
   */
  frame(
    position: Vector3,
    normal: Vector3,
    forward: Vector3,
    bone: number,
    radius?: number,
  ): Socket;
  /** Places a piece built in socket space (+Y out of the skin, +Z forward, +X side). */
  emit(piece: MeshPiece, socket: Socket, options?: EmitOptions): void;
  /**
   * Reports what the part built, for stats: its largest piece's size (metres; a tooth's or
   * horn's length, an eye's radius) and how many in a row. Sizes may follow the head, so stats
   * read these rather than the parameters.
   */
  measure(size: number, count: number): void;
  /** Membrane modules: the wing or fin they cover. */
  readonly wing?: WingContext;
  /** Membrane modules: a sheet between two spars, carried by stations so it folds exactly. */
  panel(a: Spar, b: Spar, look: PanelLook): void;
  /**
   * Membrane modules: a rigid sheet in model space (bind pose) with weights per vertex, for
   * insect wings and feather cards. `along` and `across` (0 to 1) place veins and colour.
   */
  sheet(
    positions: readonly Vector3[],
    normals: readonly Vector3[],
    indices: readonly number[],
    weights: readonly (readonly [number, number][])[],
    along: readonly number[],
    across: readonly number[],
    look: MembraneLook,
  ): void;
  /**
   * Hard geometry in model space (bind pose) with weights per vertex, into the parts mesh: a
   * beetle's wing case, which rides its wing's bones.
   */
  solid(
    positions: readonly Vector3[],
    normals: readonly Vector3[],
    indices: readonly number[],
    weights: readonly (readonly [number, number][])[],
    look: { readonly color: string; readonly roughness: number },
  ): void;
  /**
   * Where a bind-pose point riding `bone` sits when the wings rest folded, and back: for parts
   * shaped to fit the folded pose (a wing case lying on the abdomen). Identity off wings.
   */
  toRest(point: Vector3, bone: number): Vector3;
  fromRest(point: Vector3, bone: number): Vector3;
  /** The skin first met from `from` along `dir` (unit), or undefined within `reach` metres. */
  skinAlong(from: Vector3, dir: Vector3, reach: number): Vector3 | undefined;
  /** Membrane modules: a bone for a group of feathers, folding with the wing. */
  featherBone(parent: number, head: Vector3, tail: Vector3, up: Vector3): number;
  /**
   * A socket-space point (+X side, +Y out of the skin, +Z forward; mirrored on a right-hand copy)
   * in model space, for placing a part's own bones.
   */
  toModel(socket: Socket, point: Vector3): Vector3;
  /**
   * Parts with bones (docs/design/9.4-tentacles-parts.md): each chain the `bones` hook declared,
   * its bone ids and its joints in model space (one more than bones), root first.
   */
  readonly chains: readonly {
    readonly bones: readonly number[];
    readonly points: readonly Vector3[];
  }[];
  /** Rows and columns a membrane may use at this quality (scale counts by it). */
  readonly detail: number;
  /**
   * The skin at a signed `angle` round what the part sits on (negative to the right), whatever
   * its side: area parts cover both flanks with it (docs/design/9.5-coverings.md). `near`, a
   * socket close by (the last one on a grid), makes finding the skin much cheaper.
   */
  surface(at: number, angle: number, near?: Socket): Socket;
  /** Area-slot parts: the band of skin they cover. */
  readonly area?: AreaBand;
  /**
   * Area-slot parts: points over the area no closer than `spacing` metres (Poisson-disk, by dart
   * throwing from the part's own stream), measured along the body and round it, at most `count`.
   */
  scatter(spacing: number, options?: { readonly count?: number }): ScatterPoint[];
}

/** Hooks a part module provides (membrane modules add `WingHooks`). */
/** A chain of bones a part declares for itself (docs/design/9.4-tentacles-parts.md). */
/**
 * The skin an area-slot part covers (docs/design/9.5-coverings.md): its area, `from` and `to`
 * along what it sits on, and the angles from the top (0) toward the belly (180) the area spans
 * on each side.
 */
export interface AreaBand {
  readonly area: Area;
  readonly from: number;
  readonly to: number;
  readonly angles: readonly [number, number];
}

/** Each area's band of angles from the top, on both sides. */
export const AREA_ANGLES: Readonly<Record<Area, readonly [number, number]>> = {
  back: [0, 70],
  sides: [50, 130],
  belly: [110, 180],
  all: [0, 180],
};

/** A scattered point: its socket on the skin and where it is, `at` along and signed `angle`. */
export type ScatterPoint = Socket & { readonly at: number; readonly angle: number };

export interface PartChain {
  /** Joints in model space, root first (one more than bones); place them with `ctx.toModel`. */
  readonly points: readonly Vector3[];
  /** The bone it hangs from; the part's socket's bone when left out. */
  readonly parent?: number;
  /** Bones' radius (metres) at each joint, for their capsules; 1% of the torso when left out. */
  readonly radii?: readonly number[];
  /** Each bone's local Z; the chain's own plane when left out. */
  readonly up?: Vector3;
  /**
   * What moves it: `spring` sways (antennae), `jaw` opens with the jaw (mandibles), `grip`
   * closes with the `grip` goal (a pincer's finger), `flare` opens with the `flare` goal (frills,
   * hoods, quills and sails, docs/design/9.5-coverings.md). Left out, it holds still.
   */
  readonly drive?: 'spring' | 'jaw' | 'grip' | 'flare';
  /** Springs: how hard it is pulled back toward rest per step, 0 to 1. */
  readonly stiffness?: number;
  /** jaw, grip and flare: each bone's turn about its local X at full drive (radians). */
  readonly pose?: readonly number[];
}

export interface PartHooks extends WingHooks {
  /** Bones of the part's own, built before `build`, which gets them as `ctx.chains`. */
  bones?(ctx: PartBuildContext, params: Record<string, unknown>): readonly PartChain[];
  /**
   * A foot's height: how far above the ground it holds the leg's tip (metres). Without it the
   * stance decides (docs/design/8.2-feet.md).
   */
  footHeight?(ctx: import('./types.ts').FootContext, params: Record<string, unknown>): number;
  toes?(
    ctx: import('./types.ts').ToeContext,
    params: Record<string, unknown>,
  ): import('./types.ts').ToeChain[];
  build?(ctx: PartBuildContext, params: Record<string, unknown>): void;
  /** A foot's or hand's claws, for stats: how many and how long (torso lengths). */
  claws?(params: Record<string, unknown>): { readonly count: number; readonly length: number };
}

const ROUGHNESS: Record<PartMaterial, number> = {
  bone: 0.55,
  horn: 0.42,
  chitin: 0.3,
  enamel: 0.3,
  eye: 0.08,
  skin: 0.7,
};
const DEFAULT_COLOR: Record<PartMaterial, string> = {
  bone: '#e2d8be',
  horn: '#d4c6a2',
  chitin: '#2b2420',
  enamel: '#efe8d0',
  eye: '#e8e2cc',
  skin: '#7a6a50',
};

/** Something worth telling the blueprint's author about a part. */
export interface PartNote {
  readonly path: string;
  readonly message: string;
  readonly code?: string;
  readonly fix?: string;
}

/** Accumulates part and eye geometry in model space. */
export class PartSink {
  /** Where each part instance sits, for labels on debug renders. */
  readonly markers = new Map<string, [number, number, number]>();
  /** The bone a part's marker rides, when it should follow a pose (a wing's membrane). */
  readonly markerBones = new Map<string, number>();
  readonly parts = {
    positions: [] as number[],
    normals: [] as number[],
    indices: [] as number[],
    color: [] as number[],
    info: [] as number[],
    weights: [] as [number, number][][],
  };
  readonly eyes = {
    positions: [] as number[],
    normals: [] as number[],
    indices: [] as number[],
    eye: [] as number[],
    iris: [] as number[],
    sclera: [] as number[],
    weights: [] as [number, number][][],
  };
  /** What each part reported building, by its id in the blueprint (largest over its copies). */
  readonly sizes = new Map<string, { size: number; count: number }>();
  /** Driven chains of parts' own bones (docs/design/9.4-tentacles-parts.md). */
  readonly partChains: DrivenChain[] = [];
  /** Membranes, feathers and their stations (docs/design/9.3-wings-fins.md). */
  readonly membranes = new MembraneSink();
  /** Feather group bones by the wing they fold with. */
  readonly feathers = new Map<string, number[]>();
  /** Eyelids, which join the skin: their geometry, the bone of each vertex, and their chains. */
  readonly lids = {
    positions: [] as number[],
    normals: [] as number[],
    indices: [] as number[],
    bones: [] as number[],
    chains: [] as DrivenChain[],
  };
}

export interface PartsInput {
  readonly bones: BoneDef[];
  readonly paths: Map<string, readonly PathSegment[]>;
  readonly sdf: Sdf;
  /** The skin grid's primitives per block, which reach every point near the skin. */
  readonly culling: PrimCulling;
  readonly weightOptions: WeightOptions;
  /** Every head with its jaw (-1 without) and mouth line; `main` indexes the main one. */
  readonly heads: readonly {
    readonly id: string;
    readonly head: number;
    readonly jaw: number;
    readonly mouth: MouthShape | undefined;
  }[];
  readonly main: number;
  readonly palette: Readonly<Record<string, string>>;
  /** `body.head.lips` (0–1): how far in from the lips the gums sit. */
  readonly lips: number;
  readonly scale: number;
  readonly seed: number;
  readonly registry: Registry;
  /** Toe chains per limb instance id. */
  readonly toes: ReadonlyMap<string, readonly (readonly number[])[]>;
  /** Mirror sign per limb instance id. */
  readonly limbMirror: ReadonlyMap<string, number>;
  /** Wings and fins, by limb instance id, for their membranes. */
  readonly wings: ReadonlyMap<string, SpanLimb>;
  /** Membrane rows and columns scale (1 at medium). */
  readonly detail: number;
  /** World transforms of folded wing bones at rest: bind to rest, by bone (9.3). */
  readonly rest: ReadonlyMap<number, { readonly rotation: Quaternion; readonly position: Vector3 }>;
}

const Y = new Vector3(0, 1, 0);

/**
 * March from the axis point along `dir` to the skin (or the section radius for thin bones):
 * where a part on a section sits (and, inverted, `anchorAt` in anchor.ts).
 */
export function marchToSurface(
  evaluator: SdfEvaluator,
  point: Vector3,
  dir: Vector3,
  radius: number,
): { position: Vector3; normal: Vector3 } {
  const reach = radius * 3 + 1e-4;
  const f = (s: number) =>
    evaluator.eval(point.x + dir.x * s, point.y + dir.y * s, point.z + dir.z * s);
  let lo = 0;
  let hi = -1;
  const steps = 30;
  for (let i = 1; i <= steps; i++) {
    const s = (reach * i) / steps;
    if (f(s) >= 0) {
      hi = s;
      lo = (reach * (i - 1)) / steps;
      break;
    }
  }
  if (hi < 0 || f(0) >= 0) {
    return { position: point.clone().addScaledVector(dir, radius), normal: dir.clone() };
  }
  for (let i = 0; i < 24; i++) {
    const mid = (lo + hi) / 2;
    if (f(mid) >= 0) hi = mid;
    else lo = mid;
  }
  const position = point.clone().addScaledVector(dir, hi);
  const normal = evaluator.gradient(position.x, position.y, position.z, radius * 0.05);
  if (normal.dot(dir) < 0.2) normal.copy(dir);
  return { position, normal };
}

/** Builds every part and foot, appending eye bones to `input.bones`. Returns eye bone ids. */
export function buildParts(
  parts: readonly PartSpec[],
  feet: readonly {
    readonly limbId: string;
    readonly mirror: 1 | -1 | 0;
    readonly type: string;
    readonly params: Readonly<Record<string, unknown>>;
    /** A wing's or fin's membrane rather than a foot. */
    readonly membrane?: boolean;
  }[],
  input: PartsInput,
  sink: PartSink,
): { eyeBones: number[]; notes: PartNote[] } {
  const evaluator = new SdfEvaluator(input.sdf);
  // The field from the primitives of the skin grid's block around the point: the others are too
  // far to bring the skin near it. For tests of which side of the skin a point is on.
  const skinField = (x: number, y: number, z: number) =>
    evaluator.eval(x, y, z, input.culling.primsOf(input.culling.blockAt(x, y, z)));
  const eyeBones: number[] = [];
  const notes: PartNote[] = [];
  /** Per part instance: sampled vertices, and how many of them are outside the skin. */
  const exposure = new Map<string, { total: number; outside: number }>();
  const rng = createRng(input.seed);
  const partPaths = new Map<
    string,
    {
      points: Vector3[];
      radii: number[];
      normal: Vector3;
      bone: number;
      weights: readonly (readonly [number, number])[];
    }
  >();

  // Copies of a part, one per head (a mirrored pair counts once).
  const copies = new Map<string, number>();
  for (const part of parts)
    if (part.mirror !== -1) copies.set(part.baseId, (copies.get(part.baseId) ?? 0) + 1);

  const resolveColor = (value: unknown, fallback: string) => {
    if (typeof value === 'string') return input.palette[value] ?? toHex(value) ?? fallback;
    return fallback;
  };

  const toSurface = (point: Vector3, dir: Vector3, radius: number) =>
    marchToSurface(evaluator, point, dir, radius);

  /**
   * `toSurface` near a known hit: neighbouring points on a grid lie at about the same depth, so a
   * bracket round `guess` (metres from `point`) and a short bisection find the skin with far fewer
   * field evaluations. Falls back to the full march when the bracket misses.
   */
  const toSurfaceNear = (point: Vector3, dir: Vector3, radius: number, guess: number) => {
    const f = (s: number) =>
      evaluator.eval(point.x + dir.x * s, point.y + dir.y * s, point.z + dir.z * s);
    let lo = Math.max(0, guess - 0.15 * radius);
    let hi = guess + 0.15 * radius;
    if (!(f(lo) < 0 && f(hi) >= 0)) return toSurface(point, dir, radius);
    for (let i = 0; i < 16; i++) {
      const mid = (lo + hi) / 2;
      if (f(mid) >= 0) hi = mid;
      else lo = mid;
    }
    const position = point.clone().addScaledVector(dir, hi);
    const normal = evaluator.gradient(position.x, position.y, position.z, radius * 0.05);
    if (normal.dot(dir) < 0.2) normal.copy(dir);
    return { position, normal };
  };

  const frameOf = (
    position: Vector3,
    normal: Vector3,
    forwardRef: Vector3,
    radius: number,
    weights: readonly (readonly [number, number])[],
  ): Socket => {
    const forward = forwardRef.clone().addScaledVector(normal, -forwardRef.dot(normal));
    if (forward.lengthSq() < 1e-10) forward.set(0, 0, 1).addScaledVector(normal, -normal.z);
    forward.normalize();
    const side = new Vector3().crossVectors(normal, forward).normalize();
    return { position, normal, forward, side, radius, weights };
  };

  const socketOn = (
    target: string,
    at: number,
    angle: number,
    mirror: number,
    near?: Socket,
  ): Socket => {
    const partPath = partPaths.get(target);
    if (partPath) {
      // On another part: around its centreline.
      const n = partPath.points.length;
      const x = Math.min(1, Math.max(0, at)) * (n - 1);
      const i = Math.min(n - 2, Math.floor(x));
      const a = partPath.points[i] as Vector3;
      const b = partPath.points[i + 1] as Vector3;
      const point = new Vector3().lerpVectors(a, b, x - i);
      const along = new Vector3().subVectors(a, b).normalize();
      const up = partPath.normal
        .clone()
        .addScaledVector(along, -partPath.normal.dot(along))
        .normalize();
      const r =
        (partPath.radii[i] as number) +
        ((partPath.radii[i + 1] as number) - (partPath.radii[i] as number)) * (x - i);
      const dir = aroundDirection({ forward: along, up }, angle, mirror || 1);
      return frameOf(point.clone().addScaledVector(dir, r), dir, along, r, partPath.weights);
    }
    const path = input.paths.get(target);
    if (!path) throw new Error(`no attachment target "${target}"`);
    const frame = samplePath(input.bones, path, at);
    // On limbs, make angle 90 point away from the body whichever side the limb is on.
    let m = mirror;
    const limbMirror = input.limbMirror.get(target.replace(/\.toe\d+$/, ''));
    if (limbMirror !== undefined) {
      const left = new Vector3().crossVectors(frame.up, frame.forward);
      m = (mirror || 1) * Math.sign(left.x * (limbMirror || 1) || 1);
    }
    const dir = aroundDirection(frame, angle, m);
    const reach = frame.radius * Math.max(frame.cross[0], frame.cross[1]);
    const { position, normal } = near
      ? toSurfaceNear(frame.point, dir, reach, near.position.distanceTo(frame.point))
      : toSurface(frame.point, dir, reach);
    // Parts follow what they attach to: weights come only from that section's bones (the head,
    // not the jaw, which is a section of its own).
    const weights = weightsAt(position, input.weightOptions, new Set(path.map((seg) => seg.bone)));
    return frameOf(position, normal, frame.forward, frame.radius, weights);
  };

  /** Poisson-disk points over an area of `target` (docs/design/9.5-coverings.md). */
  const scatterOn = (
    target: string,
    band: AreaBand,
    spacing: number,
    count: number,
    stream: Rng,
  ): ScatterPoint[] => {
    const path = input.paths.get(target);
    if (!path || spacing <= 0 || count <= 0) return [];
    // The section's length and radius along the band, to measure distances on the body.
    const steps = 32;
    const along: number[] = [0];
    const radius: number[] = [];
    let previous: Vector3 | undefined;
    for (let i = 0; i <= steps; i++) {
      const frame = samplePath(input.bones, path, band.from + ((band.to - band.from) * i) / steps);
      radius.push(frame.radius * Math.max(frame.cross[0], frame.cross[1]));
      if (previous) along.push((along[i - 1] as number) + previous.distanceTo(frame.point));
      previous = frame.point.clone();
    }
    const at = (u: number, values: readonly number[]) => {
      const x = Math.min(1, Math.max(0, u)) * steps;
      const i = Math.min(steps - 1, Math.floor(x));
      return (values[i] as number) + ((values[i + 1] as number) - (values[i] as number)) * (x - i);
    };
    const widest = Math.max(...radius);
    const [lo, hi] = band.angles;
    const kept: { u: number; s: number; angle: number; r: number }[] = [];
    let misses = 0;
    while (kept.length < count && misses < 60) {
      const u = stream.next();
      const r = at(u, radius);
      // Thicker stretches hold more of the area.
      if (stream.next() * widest > r) continue;
      const side = stream.next() < 0.5 ? -1 : 1;
      const angle = side * (lo + (hi - lo) * stream.next());
      const s = at(u, along);
      const near = kept.some((k) => {
        let turn = Math.abs(k.angle - angle);
        if (turn > 180) turn = 360 - turn;
        const round = ((k.r + r) / 2) * ((turn * Math.PI) / 180);
        return Math.hypot(k.s - s, round) < spacing;
      });
      if (near) {
        misses++;
        continue;
      }
      misses = 0;
      kept.push({ u, s, angle, r });
    }
    return kept.map((k) => {
      const t = band.from + (band.to - band.from) * k.u;
      return { ...socketOn(target, t, k.angle, 1), at: t, angle: k.angle };
    });
  };

  const emitInto = (
    piece: MeshPiece,
    socket: Socket,
    mirror: number,
    options: EmitOptions,
    material: PartMaterial,
    id: string,
    inMouth = false,
  ) => {
    const local = mirror < 0 ? mirrorX(clonePiece(piece)) : piece;
    if (!sink.markers.has(id)) {
      // A piece built in model space (a socket at the origin, like a beak's) marks its middle.
      const atOrigin = socket.position.lengthSq() === 0 && local.positions.length >= 3;
      const n = local.positions.length / 3;
      const middle = (axis: number) => {
        let sum = 0;
        for (let v = 0; v < n; v++) sum += local.positions[v * 3 + axis] as number;
        return sum / n;
      };
      sink.markers.set(
        id,
        atOrigin
          ? [middle(0), middle(1), middle(2)]
          : [socket.position.x, socket.position.y, socket.position.z],
      );
    }
    const sinkBy = options.sink ?? 0;
    const origin = socket.position.clone().addScaledVector(socket.normal, -sinkBy);
    const toWorld = (x: number, y: number, z: number, out: Vector3) =>
      out
        .copy(origin)
        .addScaledVector(socket.side, x)
        .addScaledVector(socket.normal, y)
        .addScaledVector(socket.forward, z);
    const dirWorld = (x: number, y: number, z: number, out: Vector3) =>
      out
        .set(0, 0, 0)
        .addScaledVector(socket.side, x)
        .addScaledVector(socket.normal, y)
        .addScaledVector(socket.forward, z)
        .normalize();
    const p = new Vector3();
    const nrm = new Vector3();
    if (options.path) {
      const points = options.path.points.map((q) =>
        toWorld(mirror < 0 ? -q.x : q.x, q.y, q.z, new Vector3()),
      );
      partPaths.set(id, {
        points,
        radii: [...options.path.radii],
        normal: socket.forward.clone(),
        bone: -1,
        weights: socket.weights,
      });
    }
    if (options.eye) {
      const eye = options.eye;
      // The eye looks mostly forward, a little out of the skin.
      const look = socket.normal
        .clone()
        .multiplyScalar(0.45)
        .add(new Vector3(0, 0, 1).multiplyScalar(0.55));
      look.addScaledVector(Y, -look.y * 0.5).normalize();
      const dominant =
        [...socket.weights].sort((a, b) => b[1] - a[1])[0]?.[0] ??
        (input.heads[input.main]?.head as number);
      const centre = origin.clone();
      const eyeUp = Y.clone().addScaledVector(look, -look.y).normalize();
      const eyeSide = new Vector3().crossVectors(eyeUp, look).normalize();
      input.bones.push({
        name: `eye.${id}`,
        parent: dominant,
        section: 'eye',
        owner: id,
        head: centre.clone(),
        tail: centre.clone().addScaledVector(look, eye.radius),
        up: eyeUp,
        r0: eye.radius,
        r1: eye.radius,
        cross: [1, 1],
        t0: 0,
        t1: 1,
        skin: false,
        chain: -1,
      });
      const bone = input.bones.length - 1;
      eyeBones.push(bone);
      if (eye.lids) {
        // The lids' frame leans toward the skin's normal, so both corners of the opening sit at
        // the skin's depth; they hang from what the eye hangs from, not from the turning eye.
        const out = socket.normal
          .clone()
          .multiplyScalar(0.7)
          .addScaledVector(look, 0.3)
          .normalize();
        const up = Y.clone().addScaledVector(out, -out.y);
        if (up.lengthSq() < 1e-8) up.copy(eyeUp);
        up.normalize();
        const frame = {
          centre,
          side: new Vector3().crossVectors(up, out).normalize(),
          up,
          out,
          radius: eye.radius,
        };
        const squint = eye.squint ?? 0.15;
        const angles = lidAngles(squint);
        const lidBones = (['upper', 'lower'] as const).map((which) => {
          input.bones.push({
            name: `eye.${id}.${which}`,
            parent: dominant,
            section: 'lid',
            owner: id,
            head: centre.clone(),
            tail: centre.clone().addScaledVector(out, eye.radius),
            up: up.clone(),
            r0: eye.radius,
            r1: eye.radius,
            cross: [1, 1],
            t0: 0,
            t1: 1,
            skin: false,
            chain: -1,
          });
          return input.bones.length - 1;
        });
        // A bone's local X is (along × up), the frame's -side, and a turn by a about +side
        // takes an edge's angle from φ to φ - a: so each lid turns by (meet - its edge) about
        // its local X (the upper one down, the lower one up).
        sink.lids.chains.push({
          owner: id,
          bones: lidBones,
          drive: 'blink',
          poses: { closed: [angles.meet - angles.upper, angles.meet - angles.lower] },
        });
        const mesh = lidMesh(frame, squint);
        const start = sink.lids.positions.length / 3;
        sink.lids.positions.push(...mesh.positions);
        sink.lids.normals.push(...mesh.normals);
        for (const i of mesh.indices) sink.lids.indices.push(start + i);
        for (const w of mesh.which) sink.lids.bones.push(lidBones[w] as number);
      }
      const base = sink.eyes.positions.length / 3;
      const iris = hexToRgb(eye.iris);
      const sclera = hexToRgb(eye.sclera);
      const pupil = eye.pupil === 'round' ? 0 : eye.pupil === 'slit' ? 1 : 2;
      for (let v = 0; v < local.positions.length; v += 3) {
        toWorld(
          local.positions[v] as number,
          local.positions[v + 1] as number,
          local.positions[v + 2] as number,
          p,
        );
        dirWorld(
          local.normals[v] as number,
          local.normals[v + 1] as number,
          local.normals[v + 2] as number,
          nrm,
        );
        sink.eyes.positions.push(p.x, p.y, p.z);
        sink.eyes.normals.push(nrm.x, nrm.y, nrm.z);
        const d = p.clone().sub(centre).divideScalar(eye.radius);
        sink.eyes.eye.push(d.dot(eyeSide), d.dot(eyeUp), d.dot(look), pupil);
        sink.eyes.iris.push(iris[0], iris[1], iris[2], eye.irisSize);
        sink.eyes.sclera.push(sclera[0], sclera[1], sclera[2]);
        sink.eyes.weights.push([[bone, 1]]);
      }
      for (const i of local.indices) sink.eyes.indices.push(base + i);
      return;
    }
    if (options.membrane) {
      const world: Vector3[] = [];
      const normals: Vector3[] = [];
      const along: number[] = [];
      for (let v = 0; v < local.positions.length; v += 3) {
        world.push(
          toWorld(
            local.positions[v] as number,
            local.positions[v + 1] as number,
            local.positions[v + 2] as number,
            new Vector3(),
          ),
        );
        normals.push(
          dirWorld(
            local.normals[v] as number,
            local.normals[v + 1] as number,
            local.normals[v + 2] as number,
            new Vector3(),
          ),
        );
        along.push(local.t[v / 3] ?? 0);
      }
      const weights: [number, number][] =
        options.bone !== undefined ? [[options.bone, 1]] : socket.weights.map(([b, w]) => [b, w]);
      rigidSheet(
        world,
        normals,
        local.indices,
        world.map(() => weights),
        along,
        world.map(() => 0),
        sink.membranes,
        {
          ...options.membrane,
          color: options.color ?? DEFAULT_COLOR[material],
          ...(options.tipColor ? { tipColor: options.tipColor } : {}),
        },
        hexToRgb,
      );
      return;
    }
    const base = sink.parts.positions.length / 3;
    const c0 = hexToRgb(options.color ?? DEFAULT_COLOR[material]);
    const c1 = hexToRgb(options.tipColor ?? options.color ?? DEFAULT_COLOR[material]);
    const weights: [number, number][] =
      options.bone !== undefined ? [[options.bone, 1]] : socket.weights.map(([b, w]) => [b, w]);
    for (let v = 0; v < local.positions.length; v += 3) {
      toWorld(
        local.positions[v] as number,
        local.positions[v + 1] as number,
        local.positions[v + 2] as number,
        p,
      );
      dirWorld(
        local.normals[v] as number,
        local.normals[v + 1] as number,
        local.normals[v + 2] as number,
        nrm,
      );
      sink.parts.positions.push(p.x, p.y, p.z);
      sink.parts.normals.push(nrm.x, nrm.y, nrm.z);
      const t = local.t[v / 3] ?? 0;
      const k = t * t;
      sink.parts.color.push(
        c0[0] + (c1[0] - c0[0]) * k,
        c0[1] + (c1[1] - c0[1]) * k,
        c0[2] + (c1[2] - c0[2]) * k,
      );
      sink.parts.info.push(t, ROUGHNESS[material]);
      sink.parts.weights.push(weights);
    }
    for (const i of local.indices) sink.parts.indices.push(base + i);
    // How much of the piece shows: sample its vertices against the skin (not for mouth parts,
    // which stand inside the mouth by design).
    if (inMouth) return;
    const seen = exposure.get(id) ?? { total: 0, outside: 0 };
    exposure.set(id, seen);
    const step = Math.max(3, Math.floor(local.positions.length / 3 / 64) * 3);
    for (let v = base * 3; v < sink.parts.positions.length; v += step) {
      seen.total++;
      const x = sink.parts.positions[v] as number;
      const y = sink.parts.positions[v + 1] as number;
      const z = sink.parts.positions[v + 2] as number;
      if (skinField(x, y, z) > 0.002 * input.scale) seen.outside++;
    }
  };

  // The head a mouth part sits on (`head.L1`, `jaw.L1`), else the main head.
  const headOf = (on: string) => {
    const instance = /^(?:head|jaw)(\.[LR]\d+)/.exec(on)?.[1];
    return (
      input.heads.find((x) => instance !== undefined && x.id === `head${instance}`) ??
      input.heads[input.main]
    );
  };

  const contextFor = (
    id: string,
    baseId: string,
    type: string,
    module: PartModule,
    mirror: 1 | -1 | 0,
    place: {
      on: string;
      at: number;
      from: number;
      to: number;
      angle: number;
      area?: Area | undefined;
    },
    toes: PartBuildContext['toes'],
    wing?: WingContext,
  ): PartBuildContext => ({
    id,
    baseId,
    type,
    copies: copies.get(baseId) ?? 1,
    scale: input.scale,
    mirror,
    at: place.at,
    from: place.from,
    to: place.to,
    angle: place.angle,
    rng: rng.stream(`part:${baseId}`),
    geo: geometryKit,
    color: resolveColor,
    socket: (at = place.at, angle = place.angle) => socketOn(place.on, at, angle, mirror),
    surface: (at, angle, near) => socketOn(place.on, at, angle, 1, near),
    ...(module.slot === 'area'
      ? {
          area: {
            area: place.area ?? 'all',
            from: place.from,
            to: place.to,
            angles: AREA_ANGLES[place.area ?? 'all'],
          },
        }
      : {}),
    scatter: (spacing, options = {}) =>
      scatterOn(
        place.on,
        {
          area: place.area ?? 'all',
          from: place.from,
          to: place.to,
          angles: AREA_ANGLES[place.area ?? 'all'],
        },
        spacing,
        options.count ?? 400,
        rng.stream(`part:${baseId}:scatter`),
      ),
    around: (t, row, angle) => {
      const h = headOf(place.on);
      if (!h?.mouth || h.jaw < 0) return undefined;
      const shape = h.mouth;
      const u = Math.min(1, Math.max(0, t));
      const left = outlineAt(shape, u, 1).point;
      const right = outlineAt(shape, u, -1).point;
      const middle = new Vector3().addVectors(left, right).multiplyScalar(0.5);
      const across = new Vector3().subVectors(left, middle);
      const half = across.length();
      // At the very tip the two sides meet: across is then the head's left.
      if (half < 1e-9) across.copy(shape.line.side);
      across.normalize();
      const a = (angle * Math.PI) / 180;
      const up = shape.line.up.clone().multiplyScalar(row === 'upper' ? 1 : -1);
      const dir = across.multiplyScalar(Math.cos(a)).addScaledVector(up, Math.sin(a)).normalize();
      const { position, normal } = toSurface(middle, dir, Math.max(half, shape.radiusAt(0)));
      return frameOf(position, normal, shape.line.forward, shape.radiusAt(0), [
        [row === 'upper' ? h.head : h.jaw, 1],
      ]);
    },
    mouth: (t, row, side) => {
      const h = headOf(place.on);
      if (!h?.mouth || h.jaw < 0) return undefined;
      const m = h.mouth;
      // On the gums, set in from the lips (docs/design/8.3-heads.md).
      const gum = gumPoint(m, input.lips, Math.min(1, Math.max(0, t)), row, side);
      return frameOf(gum.position, gum.normal, m.line.forward, gum.radius, [
        [row === 'upper' ? h.head : h.jaw, 1],
      ]);
    },
    toes,
    frame: (position, normal, forward, bone, radius = 0) =>
      frameOf(position.clone(), normal.clone().normalize(), forward, radius, [[bone, 1]]),
    emit: (piece, socket, options = {}) =>
      emitInto(piece, socket, mirror, options, module.material, id, module.slot === 'mouth'),
    measure: (size, count) => {
      const seen = sink.sizes.get(baseId);
      sink.sizes.set(baseId, {
        size: Math.max(size, seen?.size ?? 0),
        count: Math.max(count, seen?.count ?? 0),
      });
    },
    ...(wing ? { wing } : {}),
    detail: input.detail,
    panel: (a, b, look) => {
      const panels = sink.membranes.stations.length;
      stationPanel(
        a,
        b,
        {
          name: `${place.on}.p${panels}`,
          owner: place.on,
          scale: input.scale,
          rows: Math.max(3, Math.round((look.rows ?? 16) * input.detail)),
          cols: Math.max(2, Math.round((look.cols ?? 8) * input.detail)),
          ...(look.scallop !== undefined ? { scallop: look.scallop } : {}),
          spacing: 0.15 * input.scale,
        },
        input.bones,
        sink.membranes,
        { ...look, color: resolveColor(look.color, '#7a6a50') },
        hexToRgb,
      );
    },
    sheet: (positions, normals, indices, weights, along, across, look) => {
      if (!sink.markers.has(id) && positions.length > 0) {
        const at = Math.floor(positions.length / 2);
        const mid = positions[at] as Vector3;
        sink.markers.set(id, [mid.x, mid.y, mid.z]);
        const bone = (weights[at] ?? [])[0]?.[0];
        if (bone !== undefined) sink.markerBones.set(id, bone);
      }
      rigidSheet(
        positions,
        normals,
        indices,
        weights,
        along,
        across,
        sink.membranes,
        {
          ...look,
          color: resolveColor(look.color, '#7a6a50'),
          ...(look.tipColor ? { tipColor: resolveColor(look.tipColor, look.color) } : {}),
        },
        hexToRgb,
        place.on,
      );
    },
    toRest: (point, bone) => {
      const r = input.rest.get(bone);
      const b = input.bones[bone] as BoneDef;
      if (!r) return point.clone();
      const bind = bindRotationOf(b);
      return point
        .clone()
        .sub(b.head)
        .applyQuaternion(bind.clone().invert())
        .applyQuaternion(r.rotation)
        .add(r.position);
    },
    fromRest: (point, bone) => {
      const r = input.rest.get(bone);
      const b = input.bones[bone] as BoneDef;
      if (!r) return point.clone();
      const bind = bindRotationOf(b);
      return point
        .clone()
        .sub(r.position)
        .applyQuaternion(r.rotation.clone().invert())
        .applyQuaternion(bind)
        .add(b.head);
    },
    skinAlong: (from, dir, reach) => {
      const steps = 32;
      const field = (t: number) =>
        skinField(from.x + dir.x * t, from.y + dir.y * t, from.z + dir.z * t);
      let prev = field(0);
      if (prev < 0) return from.clone();
      for (let i = 1; i <= steps; i++) {
        const t = (reach * i) / steps;
        const v = field(t);
        if (v < 0) {
          let lo = (reach * (i - 1)) / steps;
          let hi = t;
          for (let k = 0; k < 14; k++) {
            const mid = (lo + hi) / 2;
            if (field(mid) < 0) hi = mid;
            else lo = mid;
          }
          return from.clone().addScaledVector(dir, (lo + hi) / 2);
        }
        prev = v;
      }
      return undefined;
    },
    solid: (positions, normals, indices, weights, look) => {
      if (!sink.markers.has(id)) {
        const p0 = positions[0];
        if (p0) sink.markers.set(id, [p0.x, p0.y, p0.z]);
        const bone = (weights[0] ?? [])[0]?.[0];
        if (bone !== undefined) sink.markerBones.set(id, bone);
      }
      const base = sink.parts.positions.length / 3;
      const c = hexToRgb(look.color);
      positions.forEach((p, v) => {
        const n = normals[v] as Vector3;
        sink.parts.positions.push(p.x, p.y, p.z);
        sink.parts.normals.push(n.x, n.y, n.z);
        sink.parts.color.push(c[0], c[1], c[2]);
        sink.parts.info.push(0, look.roughness);
        sink.parts.weights.push([...(weights[v] ?? [])]);
      });
      for (const i of indices) sink.parts.indices.push(base + i);
    },
    toModel: (socket, point) =>
      socket.position
        .clone()
        .addScaledVector(socket.side, mirror < 0 ? -point.x : point.x)
        .addScaledVector(socket.normal, point.y)
        .addScaledVector(socket.forward, point.z),
    chains: [],
    featherBone: (parent, head, tail, up) => {
      const list = sink.feathers.get(place.on) ?? [];
      input.bones.push({
        name: `${place.on}.f${list.length}`,
        parent,
        section: 'feather',
        owner: place.on,
        head: head.clone(),
        tail: tail.clone(),
        up: up.clone(),
        r0: 0.005 * input.scale,
        r1: 0.005 * input.scale,
        cross: [1, 1],
        t0: 0,
        t1: 1,
        skin: false,
        chain: -1,
      });
      list.push(input.bones.length - 1);
      sink.feathers.set(place.on, list);
      return input.bones.length - 1;
    },
  });

  /** A wing's spars in the bind pose: its arm, digits, and the body's side and leg behind it. */
  const wingContext = (span: SpanLimb): WingContext => {
    const bone = (id: number) => input.bones[id] as BoneDef;
    const chainSpar = (ids: readonly number[]): Spar => ({
      points: [bone(ids[0] as number).head.clone(), ...ids.map((id) => bone(id).tail.clone())],
      bones: [...ids],
    });
    const arm = chainSpar(span.bones);
    const hand = span.bones.at(-1) as number;
    const digits = span.digits.map((d) =>
      d.root === 'tip' ? chainSpar([hand, ...d.bones]) : chainSpar(d.bones),
    );
    // Along the skin at the wing's height, from its root back to the hip of the leg behind (or
    // 0.6 of the body), then down that leg to the knee.
    let body: Spar | undefined;
    let flank: Spar | undefined;
    if (input.paths.has(span.on)) {
      const end = span.legBehind ? span.legBehind.at : Math.min(1, span.at + 0.6);
      const points: Vector3[] = [];
      const bones: number[] = [];
      const steps = 6;
      for (let k = 0; k <= steps; k++) {
        const at = span.at + ((end - span.at) * k) / steps;
        const s = socketOn(span.on, at, span.angle, span.mirror);
        points.push(s.position.clone());
        if (k > 0) {
          const top = [...s.weights].sort((x, y) => y[1] - x[1])[0];
          bones.push(top?.[0] ?? (bone(span.bones[0] as number).parent as number));
        }
      }
      body = { points, bones };
      if (span.legBehind) {
        const thigh = span.legBehind.bones[0] as number;
        flank = { points: [...points, bone(thigh).tail.clone()], bones: [...bones, thigh] };
      } else flank = body;
    }
    return {
      role: span.role,
      arm,
      digits,
      flank,
      body,
      out: span.out.clone(),
      lead: span.lead.clone(),
      normal: span.normal.clone(),
      armLength: span.armLength,
      tipRadius: span.tipRadius,
    };
  };

  /**
   * A part's own bones (docs/design/9.4-tentacles-parts.md): its `bones` hook's chains become
   * bones after the body's, named `<part>.<chain>.<k>`, hanging from the socket's bone; driven
   * ones join the rig's chains. Returns the context `build` gets, with them as `chains`.
   */
  const withBones = (
    ctx: PartBuildContext,
    hooks: PartHooks,
    params: Record<string, unknown>,
  ): PartBuildContext => {
    if (!hooks.bones) return ctx;
    const declared = hooks.bones(ctx, params);
    if (declared.length === 0) return ctx;
    const weights = ctx.socket().weights;
    let main = weights[0]?.[0] ?? 0;
    let most = -1;
    for (const [b, w] of weights)
      if (w > most) {
        most = w;
        main = b;
      }
    const built = declared.map((chain, c) => {
      const bones: number[] = [];
      const points = chain.points.map((p) => p.clone());
      for (let k = 0; k + 1 < points.length; k++) {
        const head = points[k] as Vector3;
        const tail = points[k + 1] as Vector3;
        const along = new Vector3().subVectors(tail, head).normalize();
        const up = chain.up?.clone() ?? new Vector3(0, 1, 0);
        if (Math.abs(up.dot(along)) > 0.99) up.set(1, 0, 0);
        const r = (i: number) => chain.radii?.[i] ?? 0.01 * input.scale;
        input.bones.push({
          name: `${ctx.id}.${c}.${k}`,
          parent: k === 0 ? (chain.parent ?? main) : (bones[k - 1] as number),
          section: 'part',
          owner: ctx.id,
          head: head.clone(),
          tail: tail.clone(),
          up,
          r0: r(k),
          r1: r(k + 1),
          cross: [1, 1],
          t0: k / (points.length - 1),
          t1: (k + 1) / (points.length - 1),
          skin: false,
          chain: -1,
        });
        bones.push(input.bones.length - 1);
      }
      if (chain.drive && bones.length > 0)
        sink.partChains.push({
          owner: ctx.id,
          bones,
          drive: chain.drive,
          ...(chain.stiffness !== undefined ? { stiffness: chain.stiffness } : {}),
          ...(chain.pose ? { poses: { full: [...chain.pose] } } : {}),
        });
      return { bones, points };
    });
    return { ...ctx, chains: built };
  };

  for (const part of parts) {
    const module = input.registry.get('part', part.type) as PartModule | undefined;
    const hooks = module?.hooks as PartHooks | undefined;
    if (!module || !hooks?.build) continue;
    try {
      const params = part.params as Record<string, unknown>;
      hooks.build(
        withBones(
          contextFor(
            part.id,
            part.baseId,
            part.type,
            module,
            part.mirror,
            {
              on: part.on,
              at: part.at,
              from: part.from,
              to: part.to,
              angle: part.angle,
              area: part.area,
            },
            [],
          ),
          hooks,
          params,
        ),
        params,
      );
    } catch (error) {
      notes.push({
        path: `parts[id=${part.baseId}]`,
        message: `could not be built: ${(error as Error).message}`,
      });
    }
  }

  // Parts that barely show above the skin (teeth live inside the mouth, so they don't count).
  for (const part of parts) {
    const seen = exposure.get(part.id);
    if (!seen || seen.total < 6 || part.mirror < 0) continue;
    if (input.registry.get('part', part.type)?.slot === 'mouth') continue;
    const shown = seen.outside / seen.total;
    if (shown < 0.25)
      notes.push({
        path: `parts[id=${part.baseId}]`,
        code: 'part_buried',
        message: `only ${Math.round(shown * 100)}% of the part shows above the skin`,
        fix: 'make it longer or larger, or attach it where the body is thinner',
      });
  }

  for (const foot of feet) {
    const module = input.registry.get('part', foot.type) as PartModule | undefined;
    const hooks = module?.hooks as PartHooks | undefined;
    if (!module || !hooks?.build) continue;
    if (foot.membrane) {
      const span = input.wings.get(foot.limbId);
      if (!span) continue;
      const wing = wingContext(span);
      try {
        hooks.build(
          contextFor(
            `${foot.limbId}.membrane`,
            `${foot.limbId}.membrane`,
            foot.type,
            module,
            foot.mirror,
            { on: foot.limbId, at: 1, from: 0, to: 1, angle: 0 },
            [],
            wing,
          ),
          foot.params as Record<string, unknown>,
        );
      } catch (error) {
        notes.push({
          path: `limbs[id=${foot.limbId.replace(/\.[LR]$/, '')}].membrane`,
          message: `could not be built: ${(error as Error).message}`,
        });
      }
      continue;
    }
    const chains = input.toes.get(foot.limbId) ?? [];
    const toeSockets = chains.map((chain) => {
      const last = input.bones[chain.at(-1) as number] as BoneDef;
      const first = input.bones[chain[0] as number] as BoneDef;
      const along = new Vector3().subVectors(last.tail, last.head).normalize();
      const s = frameOf(last.tail.clone(), along, Y, last.r1, [[chain.at(-1) as number, 1]]);
      const length = chain.reduce(
        (a, id) =>
          a + (input.bones[id] as BoneDef).head.distanceTo((input.bones[id] as BoneDef).tail),
        0,
      );
      return {
        ...s,
        bone: chain.at(-1) as number,
        length,
        toeRadius: (first.r0 + last.r1) / 2,
        bones: chain,
        points: [
          first.head.clone(),
          ...chain.map((id) => (input.bones[id] as BoneDef).tail.clone()),
        ],
        limbBone: first.parent,
      };
    });
    const params = foot.params as Record<string, unknown>;
    hooks.build(
      withBones(
        contextFor(
          `${foot.limbId}.foot`,
          `${foot.limbId}.foot`,
          foot.type,
          module,
          foot.mirror,
          { on: foot.limbId, at: 1, from: 0, to: 1, angle: 0 },
          toeSockets,
        ),
        hooks,
        params,
      ),
      params,
    );
  }
  return { eyeBones, notes };
}

function clonePiece(p: MeshPiece): MeshPiece {
  return {
    positions: [...p.positions],
    normals: [...p.normals],
    indices: [...p.indices],
    t: [...p.t],
  };
}
