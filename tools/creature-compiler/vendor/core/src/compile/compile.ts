import { Matrix4, Quaternion, Vector3 } from 'three';
import type { CreatureSpec, Stance } from '../blueprint/creature.ts';
import type { Issue } from '../blueprint/issues.ts';
import { buildable, notBuilt } from '../blueprint/planned.ts';
import { sweep } from '../geometry/kit.ts';
import type { MotionData } from '../motion/controller.ts';
import { motionData } from '../motion/gaits.ts';
import { Pose } from '../motion/pose.ts';
import { applyStations } from '../motion/wings.ts';
import type { Registry } from '../registry.ts';
import { type SkinMaterialSpec, skinMaterialSpec } from '../shading/compose.ts';
import { flightOf, lagOf, strokeOf } from './flight.ts';
import { type MouthShape, refineHeads } from './head.ts';
import { type CutResult, cutMouth, lineY, type MouthLine, mouthInside } from './mouth.ts';
import { buildParts, PartSink, type SpanLimb } from './parts.ts';
import { buildSdf, primBone, type Sdf, SdfEvaluator } from './sdf.ts';
import { buildSkeleton } from './skeleton.ts';
import {
  applyHelpers,
  boneDistance,
  computeWeights,
  packTop4,
  type WeightOptions,
  WeightTable,
  weightsAt,
} from './skin.ts';
import { fitGrid, surfaceNets } from './surface-nets.ts';
import {
  allEyes,
  type BoneDef,
  type DrivenChain,
  type HeadRig,
  type LimbChainRig,
  type StationPanel,
  type TailRig,
  type WingRig,
} from './types.ts';
import { DENSITY, volumeOf } from './volume.ts';
import { foldWings } from './wings.ts';

declare const performance: { now(): number };

export type Quality = 'low' | 'medium' | 'high';
/** Grid cells along the creature's longest axis per quality. */
export const QUALITY_CELLS: Record<Quality, number> = { low: 48, medium: 96, high: 128 };
/** Most skin triangles per quality (medium is the plan's 30k budget, less the mouth and tubes). */
/** Most triangles a head's refinement aims for (docs/design/8.3-heads.md). */
export const HEAD_TRIANGLES: Record<Quality, number> = { low: 1_500, medium: 4_000, high: 9_000 };
/** Most skin triangles in all, which the heads' refinement keeps to (medium is the plan's). */
export const SKIN_LIMIT: Record<Quality, number> = { low: 10_000, medium: 30_000, high: 66_000 };
/** Skin triangles an eye's lids add, counted before they are built. */
const LID_TRIANGLES = 600;
export const TRIANGLE_BUDGET: Record<Quality, number> = {
  low: 9_000,
  medium: 27_000,
  high: 60_000,
};

export type Vec3 = [number, number, number];

export interface MeshData {
  readonly positions: Float32Array;
  readonly normals: Float32Array;
  readonly indices: Uint32Array;
  readonly skinIndex: Uint16Array;
  readonly skinWeight: Float32Array;
}

export interface SkinMeshData extends MeshData {
  /** Per vertex: along the spine (0 snout tip → 1 tail tip), height (-1 belly → 1 back), along the limb (-1 inside the mouth), crease depth. */
  readonly body: Float32Array;
  /** Per vertex: head, torso, limbs, tail weights. */
  readonly region: Float32Array;
}

export interface PartMeshData extends MeshData {
  /** sRGB colour per vertex. */
  readonly color: Float32Array;
  /** Per vertex: t (root → tip), roughness. */
  readonly info: Float32Array;
}

export interface EyeMeshData extends MeshData {
  /** Per vertex: position on the unit eyeball in eye space (+Z looks out), pupil shape (0 round, 1 slit, 2 goat). */
  readonly eye: Float32Array;
  /** Per vertex: iris sRGB colour and size. */
  readonly iris: Float32Array;
  readonly sclera: Float32Array;
}

export interface MembraneMeshData extends MeshData {
  /** sRGB colour per vertex. */
  readonly color: Float32Array;
  /** Per vertex: opacity, translucency, roughness, vein strength. */
  readonly info: Float32Array;
  /** Per vertex: along (root 0 → tip 1) and across (0 → 1) the membrane. */
  readonly vein: Float32Array;
}

export interface BonesData {
  readonly names: readonly string[];
  readonly parents: Int16Array;
  readonly sections: readonly string[];
  readonly owners: readonly string[];
  /** Bind pose, model space: head position per bone. */
  readonly positions: Float32Array;
  /** Bind pose, model space: orientation per bone (+Y along the bone, +Z its `up`). */
  readonly rotations: Float32Array;
  readonly lengths: Float32Array;
  readonly radii: Float32Array;
  /**
   * The rest pose, where it differs from the bind pose: a local rotation (x, y, z, w) per bone,
   * relative to its parent. Folded wings are built spread and rest folded
   * (docs/design/9.3-wings-fins.md); without wings it is absent and rest is bind.
   */
  readonly rest?: Float32Array;
}

export interface LegRigData {
  readonly id: string;
  readonly pair: number;
  readonly side: 'left' | 'right';
  readonly bones: readonly number[];
  readonly lengths: readonly number[];
  readonly bends: readonly number[];
  readonly restFoot: Vec3;
  readonly pole: Vec3;
  readonly reach: number;
  readonly toes: readonly (readonly number[])[];
  /** The leg's stance: a planted foot rolls (none keeps plan 1's flat feet). */
  readonly stance?: Stance;
}

export interface ArmRigData extends Omit<LegRigData, 'pair' | 'restFoot' | 'side' | 'stance'> {
  readonly side: 'left' | 'right' | 'center';
}

/** The rig as plain data (see `Rig` in types.ts): lists of heads, tails and driven chains. */
export interface RigData {
  readonly root: number;
  readonly spine: readonly number[];
  readonly heads: readonly HeadRig[];
  /** Index into `heads` of the main head. */
  readonly main: number;
  readonly tails: readonly TailRig[];
  readonly chains: readonly DrivenChain[];
  readonly legs: readonly LegRigData[];
  readonly arms: readonly ArmRigData[];
  readonly wings: readonly WingRig[];
  readonly fins: readonly LimbChainRig[];
  readonly tentacles: readonly LimbChainRig[];
  readonly stations: readonly StationPanel[];
  readonly helpers: readonly (readonly [number, number, number])[];
  readonly hipHeight: number;
  readonly posture: 'upright' | 'sprawl' | 'legless';
}

/** A named attachment point for gameplay: effects, projectiles, hit detection. */
export interface GameSocket {
  readonly name: string;
  readonly bone: number;
  /** Offset from the bone's head, in the bone's rest frame. */
  readonly offset: Vec3;
}

/** One bone's stretch of a section's centreline, in the bind pose (metres, model space). */
export interface SectionSegment {
  readonly bone: number;
  /** The section's `at` at the bone's head and tail. */
  readonly t0: number;
  readonly t1: number;
  readonly head: Vec3;
  readonly tail: Vec3;
  /** Perpendicular to the bone: dorsal on the body, the front face on limbs. */
  readonly up: Vec3;
  readonly r0: number;
  readonly r1: number;
  /** Cross-section scale across and up the bone. */
  readonly cross: readonly [number, number];
}

/** A body section's or limb's centreline, as part placement samples it. */
export interface SectionData {
  /** A body section (`torso`, `head`, …) or a limb (`foreleg.L`), whose angle 90 faces out. */
  readonly kind: 'body' | 'limb';
  /** A limb's mirror (1 left, -1 right, 0 centre); 0 for body sections. */
  readonly mirror: 1 | -1 | 0;
  readonly segments: readonly SectionSegment[];
}

export interface CompiledCreature {
  readonly name: string;
  readonly seed: number;
  readonly scale: number;
  readonly quality: Quality;
  readonly bones: BonesData;
  readonly skin: SkinMeshData;
  readonly parts: PartMeshData;
  readonly eyes: EyeMeshData;
  /** Wing and fin membranes, feathers and fins: one double-sided mesh (9.3); empty without. */
  readonly membranes: MembraneMeshData;
  readonly material: SkinMaterialSpec;
  readonly rig: RigData;
  /** Gait timing and temperament for the motion controller. */
  readonly motion: MotionData;
  readonly sockets: readonly GameSocket[];
  /**
   * What parts attach to, by name (`torso`, `head`, `foreleg.L`, …): each body section's and
   * limb's centreline, for placing parts by a point on the skin (`anchorAt`, 12.1).
   */
  readonly sections: Readonly<Record<string, SectionData>>;
  readonly bounds: { readonly min: Vec3; readonly max: Vec3 };
  /**
   * Labelled points for debug renders: every part and limb by id, and the body sections, in the
   * bind pose; `bone`, when known, carries a marker into any pose (folded wings).
   */
  readonly markers: readonly {
    readonly id: string;
    readonly kind: 'part' | 'limb' | 'section';
    readonly position: Vec3;
    readonly bone?: number;
  }[];
  /** The bind pose's bounds, where they differ from the rest pose's (spread wings). */
  readonly spreadBounds?: { readonly min: Vec3; readonly max: Vec3 };
  /** Body chains as capsules (bone, radius), free hit volumes for games. */
  readonly hitCapsules: readonly { readonly bone: number; readonly radius: number }[];
  /**
   * What each part built, by its id in the blueprint: its largest piece's size (metres) and how
   * many pieces in a row, as the module reported them. Stats read these.
   */
  readonly partSizes: Readonly<Record<string, { readonly size: number; readonly count: number }>>;
  readonly stats: {
    readonly triangles: {
      readonly skin: number;
      readonly parts: number;
      readonly eyes: number;
      readonly membranes: number;
    };
    readonly vertices: number;
    readonly bones: number;
    readonly cell: number;
    readonly timings: Readonly<Record<string, number>>;
  };
  readonly warnings: readonly Issue[];
  /** The distance field the skin was meshed from, in metres, when compiled with `field`. */
  readonly field?: Sdf;
}

export interface CompileOptions {
  readonly quality?: Quality;
  /**
   * Keep the skin's distance field in the result (`field`), for bakes that sample it
   * (docs/design/11.1-textures.md). Off by default: the live creature never needs it.
   */
  readonly field?: boolean;
}

const v3 = (v: Vector3): Vec3 => [v.x, v.y, v.z];

/** Whether any of the body or its limbs has muscle (which reshapes the skin). */
const muscled = (spec: CreatureSpec) =>
  spec.body.muscle > 0 || spec.limbs.some((l) => l.muscle > 0);

/** The same creature with no muscle anywhere: plan 1's tubes. */
const withoutMuscle = (spec: CreatureSpec): CreatureSpec => ({
  ...spec,
  body: { ...spec.body, muscle: 0 },
  limbs: spec.limbs.map((l) => ({ ...l, muscle: 0 })),
});

/** Compiles a creature spec into meshes, a skeleton and a rig. Pure: same input, same output. */
export function compileCreature(
  input: CreatureSpec,
  registry: Registry,
  options: CompileOptions = {},
): CompiledCreature {
  // Format 0.2 holds plan 2's whole vocabulary; build what exists and report the rest.
  const spec = buildable(input);
  const quality = options.quality ?? 'medium';
  const cells = QUALITY_CELLS[quality];
  if (cells === undefined)
    throw new Error(`unknown quality "${String(quality)}"; use low, medium or high`);
  const timings: Record<string, number> = {};
  let clock = performance.now();
  const lap = (name: string) => {
    const now = performance.now();
    timings[name] = Math.round((now - clock) * 100) / 100;
    clock = now;
  };
  const L = spec.scale;
  // What the format holds but the pipeline cannot draw yet is skipped below; say so.
  const warnings: Issue[] = notBuilt(input, registry);

  // 1. Skeleton.
  const skeleton = buildSkeleton(spec, registry);
  const bones = skeleton.bones;
  // Wings fold against the body as a field at any radius, the arms and the ground, so the fold
  // is the same at every quality (docs/design/9.3-wings-fins.md).
  const field =
    skeleton.wingFrames.length > 0
      ? (() => {
          const body = new SdfEvaluator(buildSdf(bones, skeleton.chains, 0));
          return (p: Vector3) => body.eval(p.x, p.y, p.z);
        })()
      : undefined;
  const fold =
    field !== undefined
      ? (() => {
          const armCapsules = skeleton.rig.arms.flatMap((arm) =>
            [...arm.bones, ...arm.toes.flat()].map((id) => {
              const bone = bones[id] as BoneDef;
              return { a: bone.head, b: bone.tail, radius: Math.max(bone.r0, bone.r1) };
            }),
          );
          return foldWings(bones, skeleton.wingFrames, field, armCapsules, L);
        })()
      : undefined;
  for (const id of fold?.blocked ?? [])
    warnings.push({
      severity: 'warning',
      path: `limbs[id=${id.replace(/\.[LR]$/, '')}]`,
      code: 'wing_clearance',
      message: 'the folded wing cannot clear the body, the arms or the ground',
      fix: "attach it higher (a lower attach.angle) or further forward, or make it shorter (length, or the membrane's span)",
    });
  for (const note of skeleton.notes)
    warnings.push({ severity: 'warning', path: note.path, code: note.code, message: note.message });
  lap('skeleton');

  // 2. Signed distance field; bones thinner than about a cell become swept tubes.
  const extent = new Vector3();
  {
    const min = new Vector3(Infinity, Infinity, Infinity);
    const max = new Vector3(-Infinity, -Infinity, -Infinity);
    for (const b of bones) {
      // Wing and fin tubes stay out of the grid's extent, so a wingspan never coarsens the body.
      if (!b.skin || b.tube) continue;
      const r = Math.max(b.r0, b.r1);
      min.min(b.head.clone().subScalar(r)).min(b.tail.clone().subScalar(r));
      max.max(b.head.clone().addScalar(r)).max(b.tail.clone().addScalar(r));
    }
    extent.subVectors(max, min);
  }
  const roughCell = Math.max(extent.x, extent.y, extent.z) / cells;
  let sdf = buildSdf(bones, skeleton.chains, roughCell * 0.9);
  // The grid follows the same creature without muscle, so muscle reshapes the skin without
  // resampling what it leaves alone (docs/design/8.1-anatomy.md).
  const plain = muscled(spec) ? buildSkeleton(withoutMuscle(spec), registry) : undefined;
  const latticeFor = (minRadius: number) =>
    plain ? buildSdf(plain.bones, plain.chains, minRadius) : undefined;
  let lattice = latticeFor(roughCell * 0.9);
  lap('sdf');

  // 3. Mesh. Bulky creatures have more surface per cell; if the skin comes out over the
  // triangle budget, mesh once more on a grid coarse enough to fit.
  let grid = fitGrid(sdf, cells, lattice);
  let surface = surfaceNets(sdf, grid);
  const budget = TRIANGLE_BUDGET[quality];
  let gridCells = cells;
  for (let pass = 0; pass < 3 && surface.indices.length / 3 > budget; pass++) {
    gridCells = Math.floor(gridCells * Math.sqrt((budget * 0.85) / (surface.indices.length / 3)));
    const minRadius = (Math.max(extent.x, extent.y, extent.z) / gridCells) * 0.9;
    sdf = buildSdf(bones, skeleton.chains, minRadius);
    lattice = latticeFor(minRadius);
    grid = fitGrid(sdf, gridCells, lattice);
    surface = surfaceNets(sdf, grid);
  }
  lap('mesh');

  // 4. Skin weights from the same geometry.
  const children: number[][] = bones.map(() => []);
  bones.forEach((b, i) => {
    if (b.parent >= 0) (children[b.parent] as number[]).push(i);
  });
  const culling = surface.culling;
  // Bones near each grid block, worked out once per block.
  const blockBones = new Map<number, number[]>();
  const nearbyBones = (p: Vector3) => {
    const block = culling.blockAt(p.x, p.y, p.z);
    let list = blockBones.get(block);
    if (!list) {
      const set = new Set<number>();
      for (const prim of culling.primsOf(block)) set.add(primBone(sdf, prim));
      list = [...set];
      blockBones.set(block, list);
    }
    return list;
  };
  const weightOptions: WeightOptions = { bones, children, nearbyBones };
  let positions = surface.positions;
  let normals = surface.normals;
  let indices = surface.indices;
  let table = computeWeights(positions, indices, weightOptions);
  lap('weights');

  // 4b. Heads: refined toward their own edge length, so their details show at any size.
  {
    const eyes = spec.parts.filter((p) => registry.get('part', p.type)?.material === 'eye');
    const refined = refineHeads({
      positions,
      normals,
      indices,
      table,
      bones,
      heads: skeleton.rig.heads,
      chains: skeleton.chains,
      mouths: skeleton.mouths,
      sdf,
      culling,
      cell: surface.grid.cell,
      perRadius: { low: 8, medium: 12, high: 16 }[quality],
      allowance: HEAD_TRIANGLES[quality],
      limit: SKIN_LIMIT[quality],
      later: 60 * sdf.thinBones.length + LID_TRIANGLES * eyes.length,
    });
    positions = refined.positions;
    normals = refined.normals;
    indices = refined.indices;
    table = refined.table;
  }
  lap('refine');

  // 5. Mouths: cut each head with a jaw exactly along its mouth line; below it follows the jaw.
  const mouths: (MouthLine | undefined)[] = skeleton.rig.heads.map(() => undefined);
  const edges: (CutResult['boundary'] | undefined)[] = skeleton.rig.heads.map(() => undefined);
  for (const [i, { head, jaw }] of skeleton.rig.heads.entries()) {
    const shape = skeleton.mouths[i];
    if (jaw < 0 || !shape) continue;
    mouths[i] = shape.line;
    const own = (bone: number) => {
      const field = buildSdf(
        bones,
        [
          {
            id: 'own',
            section: 'head',
            owner: 'head',
            bones: [bone],
            parentBone: -1,
            blend: 0,
            masses: [],
          },
        ],
        0,
      );
      const e = new SdfEvaluator(field);
      return (x: number, y: number, z: number) => e.eval(x, y, z);
    };
    const dHead = own(head);
    const dJaw = own(jaw);
    const cut = cutMouth(
      positions,
      normals,
      indices,
      table,
      head,
      jaw,
      shape.line,
      (bones[head] as BoneDef).r0,
      (x, y, z) => dJaw(x, y, z) - dHead(x, y, z),
    );
    positions = cut.positions;
    normals = cut.normals;
    indices = cut.indices;
    table = cut.table;
    edges[i] = cut.boundary;
  }
  lap('mouth');

  // 6. Swept tubes for bones too thin for the grid (toes, tail tips).
  const extraPos: number[] = [];
  const extraNrm: number[] = [];
  const extraIdx: number[] = [];
  const extraWeights: [number, number][][] = [];
  const extraFlag: number[] = [];
  const thin = new Set(sdf.thinBones);
  for (const chain of skeleton.chains) {
    let run: number[] = [];
    const flush = () => {
      if (run.length === 0) return;
      const first = bones[run[0] as number] as BoneDef;
      const points = [first.head.clone()];
      const radii = [first.profile && first.shaped ? (first.profile[0] as number) : first.r0];
      for (const id of run) {
        const bone = bones[id] as BoneDef;
        // A profile anatomy shaped (chitin segments, narrow joints) shows on thin tubes too.
        const profile = bone.shaped ? bone.profile : undefined;
        const spans = profile ? profile.length - 1 : 1;
        for (let k = 1; k <= spans; k++) {
          points.push(
            k === spans
              ? bone.tail.clone()
              : new Vector3().lerpVectors(bone.head, bone.tail, k / spans),
          );
          radii.push(profile ? (profile[k] as number) : bone.r1);
        }
      }
      const lens = [0];
      for (let i = 1; i < points.length; i++)
        lens.push(
          (lens[i - 1] as number) + (points[i] as Vector3).distanceTo(points[i - 1] as Vector3),
        );
      const total = lens.at(-1) || 1;
      const radius = (t: number) => {
        const d = t * total;
        let i = 0;
        while (i < lens.length - 2 && (lens[i + 1] as number) < d) i++;
        const f = (d - (lens[i] as number)) / ((lens[i + 1] as number) - (lens[i] as number) || 1);
        return (
          (radii[i] as number) +
          ((radii[i + 1] as number) - (radii[i] as number)) * Math.min(1, Math.max(0, f))
        );
      };
      // Thin bones get 7 sides, as always; thick tubes (wings, fins) as many as their size shows,
      // and a flat bone's cross-section lies across its plane.
      const widest = Math.max(...radii);
      const sides = first.tube
        ? Math.max(7, Math.min(16, Math.round((2 * Math.PI * widest) / (1.2 * surface.grid.cell))))
        : 7;
      const flat = first.tube === true && first.cross[0] !== first.cross[1];
      const piece = sweep(points, radius, {
        sides,
        tip: 'round',
        capRoot: true,
        ...(flat ? { cross: [first.cross[1], first.cross[0]] as const, up: first.up } : {}),
      });
      const base = (positions.length + extraPos.length) / 3;
      const runOptions: WeightOptions = { bones, children, nearbyBones: () => run };
      const p = new Vector3();
      for (let v = 0; v < piece.positions.length; v += 3) {
        extraPos.push(
          piece.positions[v] as number,
          piece.positions[v + 1] as number,
          piece.positions[v + 2] as number,
        );
        extraNrm.push(
          piece.normals[v] as number,
          piece.normals[v + 1] as number,
          piece.normals[v + 2] as number,
        );
        p.set(
          piece.positions[v] as number,
          piece.positions[v + 1] as number,
          piece.positions[v + 2] as number,
        );
        extraWeights.push(weightsAt(p, runOptions));
        extraFlag.push(0);
      }
      for (const i of piece.indices) extraIdx.push(base + i);
      run = [];
    };
    for (const id of chain.bones) {
      if (thin.has(id)) run.push(id);
      else flush();
    }
    flush();
  }
  // The inside of each mouth: lips' inner faces, gums, the cavity and the tongue.
  const extraDepth: number[] = [];
  const extraKind: number[] = [];
  for (const [i, { head, jaw }] of skeleton.rig.heads.entries()) {
    const shape = skeleton.mouths[i];
    const edge = edges[i];
    if (!shape || !edge) continue;
    const base = (positions.length + extraPos.length) / 3;
    const inside = mouthInside(
      {
        line: shape.line,
        centre: shape.centre,
        lips: spec.body.head.lips,
        tongue: spec.body.head.tongue,
        radius: shape.radiusAt,
        room: shape.room,
        head,
        jaw,
      },
      edge,
      positions,
      base,
    );
    extraPos.push(...inside.positions);
    extraNrm.push(...inside.normals);
    extraIdx.push(...inside.indices);
    for (const w of inside.weights) extraWeights.push(w);
    for (let k = 0; k < inside.depth.length; k++) {
      extraFlag.push(1);
      extraDepth[extraFlag.length - 1] = inside.depth[k] as number;
      extraKind[extraFlag.length - 1] = inside.kind[k] as number;
    }
  }
  if (extraPos.length > 0) {
    const n0 = positions.length / 3;
    const pos = new Float32Array(positions.length + extraPos.length);
    pos.set(positions);
    pos.set(extraPos, positions.length);
    const nrm = new Float32Array(normals.length + extraNrm.length);
    nrm.set(normals);
    nrm.set(extraNrm, normals.length);
    const idx = new Uint32Array(indices.length + extraIdx.length);
    idx.set(indices);
    idx.set(extraIdx, indices.length);
    const next = new WeightTable(pos.length / 3);
    for (let v = 0; v < n0; v++) next.set(v, table.entries(v));
    extraWeights.forEach((w, i) => {
      next.set(n0 + i, w);
    });
    positions = pos;
    normals = nrm;
    indices = idx;
    table = next;
  }
  const vertexCount = positions.length / 3;
  // Per vertex inside a mouth: its depth (0 at the lips, 1 at the throat) and kind; -1 outside.
  const inMouth = new Float32Array(vertexCount).fill(-1);
  const mouthKind = new Uint8Array(vertexCount);
  extraFlag.forEach((f, i) => {
    if (!f) return;
    const v = vertexCount - extraFlag.length + i;
    inMouth[v] = extraDepth[i] ?? 0;
    mouthKind[v] = extraKind[i] ?? 0;
  });
  lap('tubes');

  // 7. Body coordinates, read by textures and part placement instead of UVs.
  let { body, region } = bodyCoordinates(
    positions,
    normals,
    table,
    bones,
    skeleton.paths.get('spine') ?? [],
    sdf,
    culling,
    inMouth,
    mouthKind,
    spec,
    lipLine(skeleton.mouths, spec.body.head.lips),
  );
  lap('coords');

  // 8. Helper bones take half a joint's rotation.
  applyHelpers(table, skeleton.helpers);
  let skinPack = packTop4(table);
  lap('pack');

  // 9. Parts and eyes, snapped onto the skin.
  const sink = new PartSink();
  const toeMap = new Map<string, readonly (readonly number[])[]>();
  const limbMirror = new Map<string, number>();
  for (const leg of skeleton.rig.legs) toeMap.set(leg.id, leg.toes);
  for (const arm of skeleton.rig.arms) toeMap.set(arm.id, arm.toes);
  for (const [id, toes] of skeleton.spanToes) toeMap.set(id, toes);
  for (const limb of spec.limbs) limbMirror.set(limb.id, limb.mirror);
  const feet = [
    ...spec.limbs
      .filter((l) => l.foot !== null)
      .map((l) => ({
        limbId: l.id,
        mirror: l.mirror,
        type: (l.foot as NonNullable<typeof l.foot>).type,
        params: (l.foot as NonNullable<typeof l.foot>).params,
      })),
    // Membranes after feet, so a wing's thumb is built before its membrane.
    ...spec.limbs
      .filter((l) => l.membrane !== null && skeleton.spans.has(l.id))
      .map((l) => ({
        limbId: l.id,
        mirror: l.mirror,
        type: (l.membrane as NonNullable<typeof l.membrane>).type,
        params: (l.membrane as NonNullable<typeof l.membrane>).params,
        membrane: true,
      })),
  ];
  // Each wing's nearest leg behind on its side, which a membrane may run to.
  const spans = new Map<string, SpanLimb>();
  for (const [id, span] of skeleton.spans) {
    const limb = spec.limbs.find((l) => l.id === id);
    const behind = spec.limbs
      .filter(
        (l) =>
          l.role === 'leg' &&
          l.on === span.on &&
          l.mirror === span.mirror &&
          l.at > span.at &&
          limb !== undefined,
      )
      .sort((a, b) => a.at - b.at)[0];
    const leg = behind ? skeleton.rig.legs.find((l) => l.id === behind.id) : undefined;
    spans.set(id, {
      ...span,
      legBehind: behind && leg ? { at: behind.at, bones: leg.bones } : undefined,
    });
  }
  const builtParts = buildParts(
    spec.parts,
    feet,
    {
      bones,
      paths: skeleton.paths as Map<string, readonly import('./skeleton.ts').PathSegment[]>,
      sdf,
      culling,
      weightOptions,
      heads: skeleton.rig.heads.map((h, i) => ({
        id: h.id,
        head: h.head,
        jaw: h.jaw,
        mouth: h.jaw >= 0 ? skeleton.mouths[i] : undefined,
      })),
      main: skeleton.rig.main,
      palette: spec.skin.palette,
      lips: spec.body.head.lips,
      scale: L,
      seed: spec.seed,
      registry,
      toes: toeMap,
      limbMirror,
      wings: spans,
      rest: fold?.rest ?? new Map(),
      detail: quality === 'low' ? 0.5 : quality === 'high' ? 1.4 : 1,
    },
    sink,
  );
  for (const note of builtParts.notes)
    warnings.push({
      severity: 'warning',
      path: note.path,
      code: note.code ?? 'part_failed',
      message: note.message,
      ...(note.fix ? { fix: note.fix } : {}),
    });
  lap('parts');

  const packWeights = (list: [number, number][][]) => {
    const t = new WeightTable(list.length);
    list.forEach((w, i) => {
      t.set(i, w);
    });
    return packTop4(t);
  };
  const partsPack = packWeights(sink.parts.weights);
  const eyesPack = packWeights(sink.eyes.weights);
  const membranesPack = packWeights(sink.membranes.weights);

  // Eyelids join the skin, each vertex on its lid's bone, coloured like the nearest skin of the
  // head (docs/design/8.3-heads.md).
  if (sink.lids.indices.length > 0) {
    const lid = sink.lids;
    const n0 = positions.length / 3;
    const added = lid.positions.length / 3;
    const lo = new Vector3(Infinity, Infinity, Infinity);
    const hi = new Vector3(-Infinity, -Infinity, -Infinity);
    const p = new Vector3();
    for (let v = 0; v < added; v++) {
      p.fromArray(lid.positions, v * 3);
      lo.min(p);
      hi.max(p);
    }
    const pad = hi.distanceTo(lo) * 0.25;
    lo.subScalar(pad);
    hi.addScalar(pad);
    const near: number[] = [];
    for (let v = 0; v < n0; v++) {
      if ((body[v * 4 + 2] as number) <= -0.5 || (region[v * 4] as number) < 0.5) continue;
      p.fromArray(positions, v * 3);
      if (p.x >= lo.x && p.x <= hi.x && p.y >= lo.y && p.y <= hi.y && p.z >= lo.z && p.z <= hi.z)
        near.push(v);
    }
    const grow = <T extends Float32Array | Uint32Array | Uint16Array>(
      a: T,
      extra: number,
      make: (n: number) => T,
    ) => {
      const out = make(a.length + extra);
      out.set(a);
      return out;
    };
    positions = grow(positions, added * 3, (n) => new Float32Array(n));
    normals = grow(normals, added * 3, (n) => new Float32Array(n));
    positions.set(lid.positions, n0 * 3);
    normals.set(lid.normals, n0 * 3);
    const idx = grow(indices, lid.indices.length, (n) => new Uint32Array(n));
    lid.indices.forEach((i, k) => {
      idx[indices.length + k] = n0 + i;
    });
    indices = idx;
    body = grow(body, added * 4, (n) => new Float32Array(n));
    region = grow(region, added * 4, (n) => new Float32Array(n));
    skinPack = {
      skinIndex: grow(skinPack.skinIndex, added * 4, (n) => new Uint16Array(n)),
      skinWeight: grow(skinPack.skinWeight, added * 4, (n) => new Float32Array(n)),
    };
    // The nearest candidate for each lid vertex, the lowest index on a tie: candidates sorted
    // along x, searched outward from the vertex's x until x alone is further than the best, so
    // several heads' lids (a cerberus's) do not compare every pair (gate 9).
    const q = new Vector3();
    const byX = [...near].sort(
      (a, b) => (positions[a * 3] as number) - (positions[b * 3] as number) || a - b,
    );
    const xs = byX.map((s) => positions[s * 3] as number);
    for (let v = 0; v < added; v++) {
      p.fromArray(lid.positions, v * 3);
      let best = -1;
      let bestD = Infinity;
      let lo = 0;
      let hi = xs.length;
      while (lo < hi) {
        const m = (lo + hi) >> 1;
        if ((xs[m] as number) < p.x) lo = m + 1;
        else hi = m;
      }
      const consider = (k: number) => {
        const s = byX[k] as number;
        const d = q.fromArray(positions, s * 3).distanceToSquared(p);
        if (d < bestD || (d === bestD && s < best)) {
          bestD = d;
          best = s;
        }
      };
      for (let k = lo; k < xs.length; k++) {
        const dx = (xs[k] as number) - p.x;
        if (dx * dx > bestD) break;
        consider(k);
      }
      for (let k = lo - 1; k >= 0; k--) {
        const dx = p.x - (xs[k] as number);
        if (dx * dx > bestD) break;
        consider(k);
      }
      const w = n0 + v;
      if (best >= 0) {
        body.set(body.subarray(best * 4, best * 4 + 4), w * 4);
        region.set(region.subarray(best * 4, best * 4 + 4), w * 4);
      } else region[w * 4] = 1;
      skinPack.skinIndex[w * 4] = lid.bones[v] as number;
      skinPack.skinWeight[w * 4] = 1;
    }
  }

  // 10. Bones as plain data; folded wings make a rest pose apart from the bind pose.
  const bindData = bonesToData(bones);
  const stations = sink.membranes.stations;
  const unfolded: WingRig[] = skeleton.rig.wings.map((wing, i) => {
    const frame = skeleton.wingFrames.findIndex((f) => f.wing === i);
    const locals = frame >= 0 ? (fold?.locals[frame] ?? []) : [];
    const f = skeleton.wingFrames[frame];
    return {
      ...wing,
      feathers: sink.feathers.get(wing.id) ?? [],
      area: sink.membranes.area.get(wing.id) ?? 0,
      poses: { folded: locals.flatMap((q) => [q.x, q.y, q.z, q.w]) },
      lift: !f?.style.shell,
      stroke: strokeOf(f, lagOf(skeleton.wingFrames, frame)),
    };
  });
  const restPose =
    unfolded.length > 0
      ? restOf(bindData, unfolded, stations, sideOf(unfolded), field, L)
      : undefined;
  const wings = restPose?.wings ?? unfolded;
  const bonesData: BonesData = restPose ? { ...bindData, rest: restPose.rest } : bindData;
  const headOf = (bone: number): number => {
    for (let b = bone, n = 0; b >= 0 && n < bones.length; b = bones[b]?.parent ?? -1, n++) {
      const i = skeleton.rig.heads.findIndex((h) => h.head === b || h.jaw === b);
      if (i >= 0) return i;
    }
    return skeleton.rig.main;
  };
  const rig: RigData = {
    root: skeleton.rig.root,
    spine: skeleton.rig.spine,
    // Each eye belongs to the head whose bones it hangs from (the main head's when none).
    heads: skeleton.rig.heads.map((h, i) => ({
      ...h,
      eyes: builtParts.eyeBones.filter((eye) => headOf(eye) === i),
    })),
    main: skeleton.rig.main,
    tails: skeleton.rig.tails,
    chains: [...skeleton.rig.chains, ...sink.lids.chains, ...sink.partChains],
    legs: skeleton.rig.legs.map((l) => ({ ...l, restFoot: v3(l.restFoot), pole: v3(l.pole) })),
    arms: skeleton.rig.arms.map((a) => ({ ...a, pole: v3(a.pole) })),
    wings,
    fins: skeleton.rig.fins,
    tentacles: skeleton.rig.tentacles,
    stations,
    helpers: skeleton.helpers,
    hipHeight: skeleton.rig.hipHeight,
    posture: skeleton.rig.posture,
  };

  const sockets = gameSockets(bones, rig, mouths);
  const markers: CompiledCreature['markers'][number][] = [];
  const mid = (id: number) =>
    v3((bones[id] as BoneDef).head.clone().lerp((bones[id] as BoneDef).tail, 0.5));
  for (const h of skeleton.rig.heads)
    markers.push({
      id: h.id,
      kind: 'section',
      position: v3((bones[h.head] as BoneDef).tail),
      bone: h.head,
    });
  const torsoBone = skeleton.rig.spine[Math.floor(skeleton.rig.spine.length / 2)] as number;
  markers.push({ id: 'torso', kind: 'section', position: mid(torsoBone), bone: torsoBone });
  for (const tail of skeleton.rig.tails)
    markers.push({
      id: tail.id,
      kind: 'section',
      position: v3((bones[tail.bones.at(-1) as number] as BoneDef).tail),
      bone: tail.bones.at(-1) as number,
    });
  for (const limb of [
    ...skeleton.rig.legs,
    ...skeleton.rig.arms,
    ...skeleton.rig.wings,
    ...skeleton.rig.fins,
  ]) {
    const bone = limb.bones[Math.floor(limb.bones.length / 2)] as number;
    markers.push({ id: limb.id, kind: 'limb', position: mid(bone), bone });
  }
  for (const [id, position] of sink.markers) {
    const bone = sink.markerBones.get(id);
    if (!id.endsWith('.foot'))
      markers.push({ id, kind: 'part', position, ...(bone !== undefined ? { bone } : {}) });
  }
  const min = new Vector3(Infinity, Infinity, Infinity);
  const max = new Vector3(-Infinity, -Infinity, -Infinity);
  const meshes: [ArrayLike<number>, ArrayLike<number>, ArrayLike<number>][] = [
    [positions, skinPack.skinIndex, skinPack.skinWeight],
    [sink.parts.positions, partsPack.skinIndex, partsPack.skinWeight],
    [sink.eyes.positions, eyesPack.skinIndex, eyesPack.skinWeight],
    [sink.membranes.positions, membranesPack.skinIndex, membranesPack.skinWeight],
  ];
  for (const [list] of meshes) {
    for (let i = 0; i < list.length; i += 3) {
      min.min(new Vector3(list[i], list[i + 1], list[i + 2]));
      max.max(new Vector3(list[i], list[i + 1], list[i + 2]));
    }
  }
  // With folded wings, what stands there is the rest pose: its bounds frame renders and checks.
  const spreadBounds = restPose ? { min: v3(min), max: v3(max) } : undefined;
  if (restPose) {
    const rest = posedBounds(meshes, bindData, restPose.pose);
    min.copy(rest.min);
    max.copy(rest.max);
  }
  if (min.y < -0.05 * L)
    warnings.push(belowGround(bones, min.y, L, skeleton.rig.posture === 'legless'));
  lap('finish');

  const triangles = {
    skin: indices.length / 3,
    parts: sink.parts.indices.length / 3,
    eyes: sink.eyes.indices.length / 3,
    membranes: sink.membranes.indices.length / 3,
  };
  if (quality === 'medium' && triangles.skin > 30_000) {
    warnings.push({
      severity: 'warning',
      path: '',
      code: 'over_budget',
      message: `${triangles.skin} skin triangles at medium quality (budget 30000)`,
    });
  }
  return {
    name: spec.name,
    seed: spec.seed,
    scale: L,
    quality,
    bones: bonesData,
    skin: {
      positions,
      normals,
      indices,
      skinIndex: skinPack.skinIndex,
      skinWeight: skinPack.skinWeight,
      body,
      region,
    },
    parts: {
      positions: new Float32Array(sink.parts.positions),
      normals: new Float32Array(sink.parts.normals),
      indices: new Uint32Array(sink.parts.indices),
      skinIndex: partsPack.skinIndex,
      skinWeight: partsPack.skinWeight,
      color: new Float32Array(sink.parts.color),
      info: new Float32Array(sink.parts.info),
    },
    eyes: {
      positions: new Float32Array(sink.eyes.positions),
      normals: new Float32Array(sink.eyes.normals),
      indices: new Uint32Array(sink.eyes.indices),
      skinIndex: eyesPack.skinIndex,
      skinWeight: eyesPack.skinWeight,
      eye: new Float32Array(sink.eyes.eye),
      iris: new Float32Array(sink.eyes.iris),
      sclera: new Float32Array(sink.eyes.sclera),
    },
    membranes: {
      positions: new Float32Array(sink.membranes.positions),
      normals: new Float32Array(sink.membranes.normals),
      indices: new Uint32Array(sink.membranes.indices),
      skinIndex: membranesPack.skinIndex,
      skinWeight: membranesPack.skinWeight,
      color: new Float32Array(sink.membranes.color),
      info: new Float32Array(sink.membranes.info),
      vein: new Float32Array(sink.membranes.vein),
    },
    partSizes: Object.fromEntries(sink.sizes),
    material: skinMaterialSpec(
      spec.skin.palette.base as string,
      spec.skin.material,
      spec.skin.layers,
      spec.seed,
      spec.skin.fur,
    ),
    rig,
    motion: {
      ...motionData(spec, registry, {
        hipHeight: skeleton.rig.hipHeight,
        posture: skeleton.rig.posture,
      }),
      ...(wings.length > 0
        ? {
            flight: flightOf(
              wings,
              sink.membranes.outline,
              volumeOf({
                skin: { positions, indices, skinIndex: skinPack.skinIndex },
                bones: bonesData,
              } as never).volume * DENSITY,
              spreadBounds ?? { min: v3(min), max: v3(max) },
            ),
          }
        : {}),
    },
    sockets,
    sections: sectionsOf(skeleton.paths, bones, limbMirror),
    markers,
    bounds: { min: v3(min), max: v3(max) },
    ...(spreadBounds ? { spreadBounds } : {}),
    hitCapsules: bones
      .map((b, i) => ({ bone: i, radius: Math.max(b.r0, b.r1), skin: b.skin, section: b.section }))
      .filter((b) => b.skin && b.section !== 'toe')
      .map(({ bone, radius }) => ({ bone, radius })),
    stats: {
      triangles,
      vertices: positions.length / 3,
      bones: bones.length,
      cell: grid.cell,
      timings,
    },
    warnings,
    ...(options.field ? { field: sdf } : {}),
  };
}

/**
 * The rest pose of a creature with wings (docs/design/9.3-wings-fins.md): each wing at its
 * folded pose, then the membrane stations aimed across their panels. Local rotations for
 * `BonesData.rest`, and the posed `Pose` itself.
 */
function restOf(
  data: BonesData,
  wings: readonly WingRig[],
  stations: readonly StationPanel[],
  sides: readonly number[],
  field: ((p: Vector3) => number) | undefined,
  scale: number,
): { rest: Float32Array; pose: Pose; wings: WingRig[] } {
  const pose = new Pose(data);
  for (const wing of wings) {
    const folded = wing.poses.folded ?? [];
    [...wing.bones, ...wing.digits.flat()].forEach((bone, i) => {
      if (folded.length >= (i + 1) * 4) (pose.rot[bone] as Quaternion).fromArray(folded, i * 4);
    });
  }
  pose.solve();
  // Feathers fold back along the body (docs/design/9.3-wings-fins.md): each group lies against
  // the body where it rests, its upper face out and its shafts pointing back, like shingles.
  const h = 0.01 * scale;
  const outward = (p: Vector3, fallback: Vector3): Vector3 => {
    if (!field) return fallback.clone();
    const g = new Vector3(
      field(new Vector3(p.x + h, p.y, p.z)) - field(new Vector3(p.x - h, p.y, p.z)),
      field(new Vector3(p.x, p.y + h, p.z)) - field(new Vector3(p.x, p.y - h, p.z)),
      field(new Vector3(p.x, p.y, p.z + h)) - field(new Vector3(p.x, p.y, p.z - h)),
    );
    return g.lengthSq() > 1e-12 ? g.normalize() : fallback.clone();
  };
  const done = wings.map((wing, w) => {
    if (wing.feathers.length === 0) return wing;
    const humerus = wing.bones[0] as number;
    const plane = new Vector3(0, 0, 1).applyQuaternion(pose.worldRot[humerus] as Quaternion);
    const extra: number[] = [];
    const want0 = new Vector3(0.1 * (sides[w] ?? 0), -0.25, -0.95);
    const tangent = (normal: Vector3) =>
      want0.clone().addScaledVector(normal, -want0.dot(normal)).normalize();
    for (const bone of wing.feathers) {
      const parent = pose.parents[bone] as number;
      pose.solveBone(bone);
      const at = pose.worldPos[bone] as Vector3;
      const length = data.lengths[bone] ?? 0;
      // The body's normal where the group roots and halfway along it.
      let normal = outward(at, plane);
      const half = at.clone().addScaledVector(tangent(normal), 0.5 * Math.min(length, scale));
      normal = normal.add(outward(half, normal)).normalize();
      const want = tangent(normal);
      const world = new Quaternion().setFromRotationMatrix(
        new Matrix4().makeBasis(new Vector3().crossVectors(want, normal), want, normal),
      );
      const local = (pose.worldRot[parent] as Quaternion).clone().invert().multiply(world);
      (pose.rot[bone] as Quaternion).copy(local);
      pose.solveBone(bone);
      extra.push(local.x, local.y, local.z, local.w);
    }
    return { ...wing, poses: { ...wing.poses, folded: [...(wing.poses.folded ?? []), ...extra] } };
  });
  pose.solve();
  applyStations(pose, stations);
  const rest = new Float32Array(pose.count * 4);
  pose.rot.forEach((q, i) => {
    rest.set([q.x, q.y, q.z, q.w], i * 4);
  });
  return { rest, pose, wings: done };
}

/** +1 for a left wing, -1 for a right one, 0 for one in the middle. */
function sideOf(wings: readonly WingRig[]): number[] {
  return wings.map((w) => (w.side === 'left' ? 1 : w.side === 'right' ? -1 : 0));
}

/** Bounds of skinned meshes in a pose (linear blend skinning on the CPU). */
function posedBounds(
  meshes: readonly (readonly [ArrayLike<number>, ArrayLike<number>, ArrayLike<number>])[],
  bind: BonesData,
  pose: Pose,
): { min: Vector3; max: Vector3 } {
  const n = pose.count;
  const matrices = Array.from({ length: n }, (_, i) => {
    const bindM = new Matrix4().compose(
      new Vector3().fromArray(bind.positions, i * 3),
      new Quaternion().fromArray(bind.rotations, i * 4),
      new Vector3(1, 1, 1),
    );
    return new Matrix4()
      .compose(pose.worldPos[i] as Vector3, pose.worldRot[i] as Quaternion, new Vector3(1, 1, 1))
      .multiply(bindM.invert());
  });
  const min = new Vector3(Infinity, Infinity, Infinity);
  const max = new Vector3(-Infinity, -Infinity, -Infinity);
  const p = new Vector3();
  const q = new Vector3();
  const sum = new Vector3();
  for (const [positions, index, weight] of meshes) {
    for (let v = 0; v * 3 < positions.length; v++) {
      p.set(
        positions[v * 3] as number,
        positions[v * 3 + 1] as number,
        positions[v * 3 + 2] as number,
      );
      sum.set(0, 0, 0);
      let total = 0;
      for (let k = 0; k < 4; k++) {
        const w = weight[v * 4 + k] as number;
        if (w <= 0) continue;
        q.copy(p).applyMatrix4(matrices[index[v * 4 + k] as number] as Matrix4);
        sum.addScaledVector(q, w);
        total += w;
      }
      if (total > 0) sum.divideScalar(total);
      else sum.copy(p);
      min.min(sum);
      max.max(sum);
    }
  }
  return { min, max };
}

/** The skeleton's paths that parts attach to, as plain data (toes, digits and the spine left out). */
function sectionsOf(
  paths: ReadonlyMap<string, readonly { bone: number; t0: number; t1: number }[]>,
  bones: readonly BoneDef[],
  limbMirror: ReadonlyMap<string, number>,
): Record<string, SectionData> {
  const sections: Record<string, SectionData> = {};
  for (const [name, path] of [...paths].sort(([a], [b]) => (a < b ? -1 : a > b ? 1 : 0))) {
    if (name === 'spine' || /\.(toe|d)\d+$/.test(name)) continue;
    sections[name] = {
      kind: limbMirror.has(name) ? 'limb' : 'body',
      mirror: (limbMirror.get(name) ?? 0) as 1 | -1 | 0,
      segments: path.map((seg) => {
        const b = bones[seg.bone] as BoneDef;
        return {
          bone: seg.bone,
          t0: seg.t0,
          t1: seg.t1,
          head: v3(b.head),
          tail: v3(b.tail),
          up: v3(b.up),
          r0: b.r0,
          r1: b.r1,
          cross: [b.cross[0], b.cross[1]] as const,
        };
      }),
    };
  }
  return sections;
}

function bonesToData(bones: readonly BoneDef[]): BonesData {
  const n = bones.length;
  const positions = new Float32Array(n * 3);
  const rotations = new Float32Array(n * 4);
  const lengths = new Float32Array(n);
  const radii = new Float32Array(n);
  const parents = new Int16Array(n);
  const m = new Matrix4();
  const q = new Quaternion();
  bones.forEach((b, i) => {
    positions.set([b.head.x, b.head.y, b.head.z], i * 3);
    parents[i] = b.parent;
    const dir = new Vector3().subVectors(b.tail, b.head);
    lengths[i] = dir.length();
    radii[i] = Math.max(b.r0, b.r1);
    if (b.section === 'root' || dir.lengthSq() < 1e-14) {
      q.identity();
    } else {
      dir.normalize();
      const z = b.up.clone().addScaledVector(dir, -b.up.dot(dir));
      if (z.lengthSq() < 1e-10) z.set(0, 0, 1).addScaledVector(dir, -dir.z);
      z.normalize();
      const x = new Vector3().crossVectors(dir, z).normalize();
      m.makeBasis(x, dir, z);
      q.setFromRotationMatrix(m);
    }
    rotations.set([q.x, q.y, q.z, q.w], i * 4);
  });
  return {
    names: bones.map((b) => b.name),
    parents,
    sections: bones.map((b) => b.section),
    owners: bones.map((b) => b.owner),
    positions,
    rotations,
    lengths,
    radii,
  };
}

/** Mouth, eyes, head, claw tips and centre of mass as named sockets. */
function gameSockets(
  bones: readonly BoneDef[],
  rig: RigData,
  mouths: readonly (MouthLine | undefined)[],
): GameSocket[] {
  const local = (bone: number, p: Vector3): Vec3 => {
    const b = bones[bone] as BoneDef;
    const d = new Vector3().subVectors(b.tail, b.head);
    const len = d.length();
    const y = len > 1e-9 ? d.divideScalar(len) : new Vector3(0, 1, 0);
    const z = b.up.clone().addScaledVector(y, -b.up.dot(y)).normalize();
    const x = new Vector3().crossVectors(y, z);
    const o = new Vector3().subVectors(p, b.head);
    return [o.dot(x), o.dot(y), o.dot(z)];
  };
  const sockets: GameSocket[] = [];
  // The main head answers to plain `head` and `mouth`; the others add their instance
  // (`head.L1`, `mouth.L1`) from 9.1.
  for (const [i, h] of rig.heads.entries()) {
    const suffix = i === rig.main ? '' : h.id.slice('head'.length);
    const head = bones[h.head] as BoneDef;
    const mouth = mouths[i];
    sockets.push({ name: `head${suffix}`, bone: h.head, offset: [0, 0, 0] });
    if (mouth && h.jaw >= 0) {
      const tip = mouth.origin
        .clone()
        .addScaledVector(mouth.forward, mouth.tip)
        .addScaledVector(mouth.up, mouth.tipY);
      sockets.push({ name: `mouth${suffix}`, bone: h.jaw, offset: local(h.jaw, tip) });
    } else {
      sockets.push({
        name: `mouth${suffix}`,
        bone: h.head,
        offset: local(h.head, head.tail.clone()),
      });
    }
  }
  allEyes(rig).forEach((id) => {
    sockets.push({ name: (bones[id] as BoneDef).name, bone: id, offset: [0, 0, 0] });
  });
  for (const limb of [...rig.legs, ...rig.arms]) {
    limb.toes.forEach((toe, i) => {
      const last = toe.at(-1);
      if (last === undefined) return;
      const b = bones[last] as BoneDef;
      sockets.push({ name: `claw.${limb.id}.${i}`, bone: last, offset: local(last, b.tail) });
    });
    if (limb.toes.length === 0) {
      const last = limb.bones.at(-1) as number;
      sockets.push({
        name: `tip.${limb.id}`,
        bone: last,
        offset: local(last, (bones[last] as BoneDef).tail),
      });
    }
  }
  // A wing's tip, at the end of its hand (docs/design/9.3-wings-fins.md).
  for (const wing of rig.wings) {
    const last = wing.bones.at(-1);
    if (last === undefined) continue;
    sockets.push({
      name: `tip.${wing.id}`,
      bone: last,
      offset: local(last, (bones[last] as BoneDef).tail),
    });
  }
  const mid = rig.spine[Math.floor(rig.spine.length / 2)] as number;
  sockets.push({ name: 'centerOfMass', bone: mid, offset: [0, 0, 0] });
  return sockets;
}

/**
 * How much a point lies on a mouth's line (0 to 1): within half the lips' thickness of the cut,
 * in front of the corner, fading in over a tenth of the skull's radius behind it.
 */
function lipLine(
  mouths: readonly (MouthShape | undefined)[],
  lips: number,
): (p: Vector3) => number {
  const shapes = mouths.filter((m): m is MouthShape => m !== undefined);
  const d = new Vector3();
  return (p) => {
    let best = 0;
    for (const shape of shapes) {
      const m = shape.line;
      d.subVectors(p, m.origin);
      const z = d.dot(m.forward);
      const r = shape.radiusAt(z);
      const fade = smoothstep(m.corner - 0.1 * r, m.corner, z);
      if (fade <= 0 || z > m.tip + 0.1 * r) continue;
      const width = 0.5 * (0.04 + 0.2 * lips) * r;
      const off = Math.abs(d.dot(m.up) - lineY(m, z));
      best = Math.max(best, 0.85 * fade * (1 - smoothstep(0, width, off)));
    }
    return best;
  };
}

const smoothstep = (e0: number, e1: number, x: number) => {
  const t = Math.min(1, Math.max(0, (x - e0) / (e1 - e0)));
  return t * t * (3 - 2 * t);
};

/** Body coordinates per vertex, blended by skin weight. */
function bodyCoordinates(
  positions: Float32Array,
  normals: Float32Array,
  table: WeightTable,
  bones: readonly BoneDef[],
  spinePath: readonly { bone: number; t0: number; t1: number }[],
  sdf: import('./sdf.ts').Sdf,
  culling: import('./surface-nets.ts').PrimCulling,
  mouthDepth: Float32Array,
  mouthKind: Uint8Array,
  spec: CreatureSpec,
  lip: (p: Vector3) => number,
): { body: Float32Array; region: Float32Array } {
  const n = positions.length / 3;
  const body = new Float32Array(n * 4);
  const region = new Float32Array(n * 4);
  const L = spec.scale;

  // Axis coordinate (0 snout tip → 1 tail tip) at each bone's head and tail.
  const axis = new Float64Array(bones.length * 2).fill(Number.NaN);
  const spineLen = spinePath.reduce(
    (a, s) => a + (bones[s.bone] as BoneDef).head.distanceTo((bones[s.bone] as BoneDef).tail),
    0,
  );
  // The main head (plain `head` at every count; docs/design/9.1-heads-tails.md).
  const headIdx = Math.max(
    0,
    bones.findIndex((b) => b.name === 'head'),
  );
  const headBone = bones[headIdx] as BoneDef;
  const headDir = new Vector3().subVectors(headBone.tail, headBone.head).normalize();
  const tip = headBone.tail.clone().addScaledVector(headDir, headBone.r1);
  const firstSpine = spinePath[0];
  const neckEnd = firstSpine
    ? (bones[firstSpine.bone] as BoneDef).section === 'tail'
      ? (bones[firstSpine.bone] as BoneDef).head
      : (bones[firstSpine.bone] as BoneDef).tail
    : headBone.head;
  const headSpan = tip.distanceTo(neckEnd);
  const total = headSpan + spineLen || 1;
  for (const s of spinePath) {
    axis[s.bone * 2] = (headSpan + s.t0 * spineLen) / total;
    axis[s.bone * 2 + 1] = (headSpan + s.t1 * spineLen) / total;
  }
  bones.forEach((b, i) => {
    if (b.section === 'head' || b.section === 'jaw') {
      axis[i * 2] = tip.distanceTo(b.head) / total;
      axis[i * 2 + 1] = tip.distanceTo(b.tail) / total;
    }
  });
  // Extra heads and tails (`head.L1`, `neck.L1.2`, `tail.R1.5`) take their main counterpart's
  // values, bone for bone, so patterns lie the same way on every one.
  const byName = new Map(bones.map((b, i) => [b.name, i]));
  bones.forEach((b, i) => {
    const main = byName.get(b.name.replace(/^(neck|head|jaw|tail)\.[LR]\d+/, '$1'));
    if (main === undefined || main === i) return;
    axis[i * 2] = axis[main * 2] as number;
    axis[i * 2 + 1] = axis[main * 2 + 1] as number;
  });
  // Limbs, toes and others take the axis value where they attach.
  bones.forEach((b, i) => {
    if (!Number.isNaN(axis[i * 2] as number)) return;
    let parent = b.parent;
    let at = b.head;
    while (parent >= 0 && Number.isNaN(axis[parent * 2] as number)) {
      at = (bones[parent] as BoneDef).head;
      parent = (bones[parent] as BoneDef).parent;
    }
    if (parent < 0) {
      axis[i * 2] = axis[i * 2 + 1] = 0.5;
      return;
    }
    const { t } = boneDistance(bones[parent] as BoneDef, at);
    const value =
      (axis[parent * 2] as number) +
      ((axis[parent * 2 + 1] as number) - (axis[parent * 2] as number)) * t;
    axis[i * 2] = axis[i * 2 + 1] = value;
  });

  const tentacles = new Set(spec.limbs.filter((l) => l.role === 'tentacle').map((l) => l.id));
  /** Around a bone: 1 on its `up` side, -1 opposite, 0 across. */
  const aroundBone = (b: BoneDef, p: Vector3, t: number) => {
    const off = p.clone().sub(b.head.clone().lerp(b.tail, t));
    const dir = new Vector3().subVectors(b.tail, b.head).normalize();
    off.addScaledVector(dir, -off.dot(dir));
    const len = off.length();
    return len > 1e-9 ? off.dot(b.up) / len : 0;
  };
  const sectionOf = (b: BoneDef): BoneDef['section'] => {
    if (b.section !== 'helper') return b.section;
    const owner = bones.find((x) => x.owner === b.owner && x.section !== 'helper');
    return owner ? owner.section : 'torso';
  };
  const evaluator = new SdfEvaluator(sdf);
  const p = new Vector3();
  const nrm = new Vector3();
  const blend = Math.max(1e-6, sdf.maxBlend * 0.35);
  for (let v = 0; v < n; v++) {
    p.set(
      positions[v * 3] as number,
      positions[v * 3 + 1] as number,
      positions[v * 3 + 2] as number,
    );
    nrm.set(normals[v * 3] as number, normals[v * 3 + 1] as number, normals[v * 3 + 2] as number);
    let spineCoord = 0;
    let height = 0;
    let limb = 0;
    let wing = 0;
    const reg = [0, 0, 0, 0];
    const entries = table.entries(v);
    let sum = 0;
    for (const [id, w] of entries) {
      const b = bones[id] as BoneDef;
      const { t } = boneDistance(b, p);
      spineCoord +=
        w *
        ((axis[id * 2] as number) + ((axis[id * 2 + 1] as number) - (axis[id * 2] as number)) * t);
      const section = sectionOf(b);
      if (section === 'limb' || section === 'toe' || section === 'digit') {
        if (tentacles.has(b.owner)) {
          // A tentacle's belly is the inside of its curl, where suckers go (bone `up` points
          // out of the curl; docs/design/9.4-tentacles-parts.md).
          height += w * aroundBone(b, p, t);
        } else {
          const outward = new Vector3(Math.sign(p.x) || 1, 0, 0);
          // Limbs read like the flank outside and the belly inside.
          height += w * (nrm.dot(outward) * 0.45 + nrm.y * 0.35 + 0.1);
        }
        limb += w * (section === 'limb' ? b.t0 + (b.t1 - b.t0) * t : 1);
        reg[2] = (reg[2] as number) + w;
        if (b.tube) wing += w;
      } else {
        height += w * aroundBone(b, p, t);
        const k = section === 'head' || section === 'jaw' ? 0 : section === 'tail' ? 3 : 1;
        reg[k] = (reg[k] as number) + w;
      }
      sum += w;
    }
    if (sum > 0) {
      spineCoord /= sum;
      height /= sum;
      limb /= sum;
      wing /= sum;
      for (let k = 0; k < 4; k++) reg[k] = (reg[k] as number) / sum;
    }
    // Wing and fin tubes carry `limb + 2`, so the `wings` region finds them and fur skips them
    // (docs/design/9.3-wings-fins.md).
    if (wing >= 0.5) limb += 2;
    // Inside a mouth, `limb` holds -1 - depth and `crease` the kind (docs/design/8.3-heads.md).
    const depth = mouthDepth[v] as number;
    let crease = 0;
    if (depth < 0) {
      const prims = culling.primsOf(culling.blockAt(p.x, p.y, p.z));
      const d = evaluator.eval(p.x, p.y, p.z, prims);
      crease = Math.min(1, Math.max(0, (evaluator.union - d) / blend));
      // The lips' line reads as a crease, so a shut mouth shows where it opens.
      crease = Math.max(crease, lip(p));
    }
    body.set(
      depth < 0
        ? [spineCoord, height, limb, crease]
        : [spineCoord, height, -1 - depth, mouthKind[v] as number],
      v * 4,
    );
    region.set(reg, v * 4);
  }
  void L;
  return { body, region };
}

/** The ArrayBuffers in a compiled creature, to transfer (not copy) it out of a worker. */
export function compiledTransferables(c: CompiledCreature): ArrayBuffer[] {
  const out = new Set<ArrayBuffer>();
  const add = (v: ArrayBufferView) => {
    if (v.buffer instanceof ArrayBuffer) out.add(v.buffer);
  };
  for (const mesh of [c.skin, c.parts, c.eyes, c.membranes] as const) {
    for (const value of Object.values(mesh)) if (ArrayBuffer.isView(value)) add(value);
  }
  for (const value of Object.values(c.bones)) if (ArrayBuffer.isView(value)) add(value);
  return [...out];
}

/** Names the lowest section and how to lift it, for a creature that sinks into the ground. */
function belowGround(
  bones: readonly BoneDef[],
  lowest: number,
  L: number,
  legless: boolean,
): Issue {
  let owner = 'torso';
  let low = Infinity;
  for (const b of bones) {
    if (!b.skin) continue;
    const y = Math.min(b.head.y - b.r0, b.tail.y - b.r1);
    if (y < low) {
      low = y;
      // Extra heads and tails (`head.L1`) answer to their section's fields.
      owner = b.owner.replace(/^(head|jaw|neck|tail)\.[LR]\d+$/, '$1');
    }
  }
  const section = ['head', 'jaw', 'neck', 'torso', 'tail'].includes(owner);
  const fixes: Record<string, string> = {
    tail: 'raise body.tail.pitch or give it a positive curl',
    head: 'raise body.neck.pitch or body.head.pitch, or shorten the neck',
    jaw: 'raise body.neck.pitch or body.head.pitch, or shorten the neck',
    neck: 'raise body.neck.pitch or shorten the neck',
    torso: legless
      ? 'lower body.torso.pitch (a legless body lies on the ground)'
      : 'lengthen the legs or make body.torso.radius smaller',
  };
  return {
    severity: 'warning',
    path: section ? `body.${owner}` : `limbs[id=${owner.replace(/\.[LR]$/, '')}]`,
    code: 'below_ground',
    message: `the ${section ? owner : `limb ${owner}`} reaches ${(-lowest / L).toFixed(2)} torso lengths below the ground`,
    fix: fixes[owner] ?? 'raise its attach point or shorten it',
  };
}

/**
 * FNV-1a over a quantized copy of the meshes and skeleton (0.1 mm): the golden-test identity of
 * a compiled creature, stable across runs, Node and browsers.
 */
export function fingerprint(c: CompiledCreature): string {
  let h = 0x811c9dc5;
  const feed = (values: ArrayLike<number>, quantum: number) => {
    for (let i = 0; i < values.length; i++) {
      h ^= Math.round((values[i] as number) / quantum) | 0;
      h = Math.imul(h, 0x01000193) >>> 0;
    }
  };
  feed(c.skin.positions, 1e-4);
  feed(c.skin.indices, 1);
  feed(c.skin.skinIndex, 1);
  feed(c.parts.positions, 1e-4);
  feed(c.eyes.positions, 1e-4);
  feed(c.bones.positions, 1e-4);
  feed(c.bones.rotations, 1e-4);
  // Only creatures with membranes or a rest pose of their own hash these, so others keep their
  // fingerprints (docs/design/9.3-wings-fins.md).
  feed(c.membranes.positions, 1e-4);
  if (c.bones.rest) feed(c.bones.rest, 1e-4);
  return h.toString(16).padStart(8, '0');
}
