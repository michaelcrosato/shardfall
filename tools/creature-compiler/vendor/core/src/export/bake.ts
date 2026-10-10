import type { CompiledCreature } from '../compile/compile.ts';
import type { Registry } from '../registry.ts';
import { hasWingLayers, shadeMembrane, shadeMouth, shadeSkin } from '../shading/compose.ts';
import { cpuKit } from '../shading/cpu.ts';
import type { Surface } from '../shading/kit.ts';

/** Linear-light colours (0 to 1, three per vertex) and roughness (one per vertex) for a mesh. */
export interface BakedColors {
  readonly color: Float32Array;
  readonly roughness: Float32Array;
  /**
   * The skin's brightest glow (emissive layers), which vertex colours leave out until texture
   * maps (milestone 11.1); 0 without any.
   */
  readonly glow?: number;
}

/** sRGB (0 to 1) to linear light, as glTF vertex colours expect. */
export function srgbToLinear(c: number): number {
  const x = Math.min(1, Math.max(0, c));
  return x <= 0.04045 ? x / 12.92 : ((x + 0.055) / 1.055) ** 2.4;
}

/** The eyes' roughness: wet, but not glass. The renderer's eye material uses it too. */
export const EYE_ROUGHNESS = 0.24;

const smooth = (e0: number, e1: number, x: number) => {
  const t = Math.min(1, Math.max(0, (x - e0) / (e1 - e0)));
  return t * t * (3 - 2 * t);
};

/**
 * The skin's pattern stack evaluated at every vertex on the CPU, with the same pattern functions
 * the renderer's shader runs (through the CPU kit). Albedo only: relief and glow need textures,
 * so they are left out (`glow` says how bright the glow would be). Inside the mouth is wet, from the same `shadeMouth` the shader runs.
 */
export function bakeSkinColors(compiled: CompiledCreature, registry: Registry): BakedColors {
  const { body } = compiled.skin;
  const n = compiled.skin.positions.length / 3;
  const color = new Float32Array(n * 3);
  const roughness = new Float32Array(n);
  let glow = 0;
  for (let i = 0; i < n; i++) {
    const surface = surfaceAt(compiled, i);
    const shade = shadeSkin(cpuKit, surface, compiled.material, registry);
    const mouth = shadeMouth(cpuKit, body[i * 4 + 2] as number, body[i * 4 + 3] as number);
    const inside = mouth.inside;
    const lin = [shade.r, shade.g, shade.b].map(srgbToLinear);
    const wet = [mouth.r, mouth.g, mouth.b];
    for (let c = 0; c < 3; c++)
      color[i * 3 + c] = (lin[c] as number) * (1 - inside) + (wet[c] as number) * inside;
    roughness[i] = shade.roughness * (1 - inside) + mouth.roughness * inside;
    glow = Math.max(glow, shade.er, shade.eg, shade.eb);
  }
  return { color, roughness, glow };
}

/**
 * The pattern stack's view of skin vertex `i` in the rest pose, with no screen (`pixel` 0) and the
 * clock at 0: what bakes and the parity test evaluate.
 */
export function surfaceAt(compiled: CompiledCreature, i: number): Surface<number> {
  const { positions, normals, body, region } = compiled.skin;
  const inv = 1 / compiled.scale;
  const y = (positions[i * 3 + 1] as number) * inv;
  return {
    x: (positions[i * 3] as number) * inv,
    y,
    z: (positions[i * 3 + 2] as number) * inv,
    nx: normals[i * 3] as number,
    ny: normals[i * 3 + 1] as number,
    nz: normals[i * 3 + 2] as number,
    spine: body[i * 4] as number,
    height: body[i * 4 + 1] as number,
    ...limbAndWings(body[i * 4 + 2] as number, region[i * 4 + 2] as number),
    crease: body[i * 4 + 3] as number,
    head: region[i * 4] as number,
    torso: region[i * 4 + 1] as number,
    tail: region[i * 4 + 3] as number,
    ground: y,
    pixel: 0,
    time: 0,
  };
}

/**
 * The skin's limb coordinate and its limbs and wings regions: wing and fin tubes carry `limb +
 * 2` (docs/design/9.3-wings-fins.md), and count as wings rather than limbs.
 */
export function limbAndWings(
  limb: number,
  limbs: number,
): { limb: number; limbs: number; wings: number } {
  const wings = limb >= 1.5 ? 1 : 0;
  return { limb: Math.max(limb - 2 * wings, 0), limbs: limbs * (1 - wings), wings: limbs * wings };
}

/**
 * The pattern stack's view of membrane vertex `i` (docs/design/9.3-wings-fins.md): its bind
 * position, `spine` from front to back of the creature's rest bounds, `height` from its normal,
 * `limb` along its spar, and the `wings` region alone.
 */
export function membraneSurfaceAt(compiled: CompiledCreature, i: number): Surface<number> {
  const { positions, normals, vein } = compiled.membranes;
  const inv = 1 / compiled.scale;
  const { min, max } = compiled.bounds;
  const y = (positions[i * 3 + 1] as number) * inv;
  const z = positions[i * 3 + 2] as number;
  return {
    x: (positions[i * 3] as number) * inv,
    y,
    z: z * inv,
    nx: normals[i * 3] as number,
    ny: normals[i * 3 + 1] as number,
    nz: normals[i * 3 + 2] as number,
    spine: Math.min(1, Math.max(0, (max[2] - z) / Math.max(1e-6, max[2] - min[2]))),
    height: normals[i * 3 + 1] as number,
    limb: Math.min(1, Math.max(0, vein[i * 2] as number)),
    crease: 0,
    head: 0,
    torso: 0,
    limbs: 0,
    tail: 0,
    wings: 1,
    ground: y,
    pixel: 0,
    time: 0,
  };
}

/**
 * Membranes' colours: each vertex's own, under the `wings` layers, its veins darkened as the
 * shader draws them (sampled per vertex, so they alias on small wings).
 */
export function bakeMembraneColors(compiled: CompiledCreature, registry: Registry): BakedColors {
  const { color: srgb, info, vein } = compiled.membranes;
  const n = srgb.length / 3;
  const color = new Float32Array(n * 3);
  const roughness = new Float32Array(n);
  const patterned = hasWingLayers(compiled.material);
  const line = (x: number, count: number) => {
    const f = Math.abs(((((x * count) % 1) + 1) % 1) - 0.5) * 2;
    return 1 - smooth(0, 0.1, 1 - f);
  };
  for (let i = 0; i < n; i++) {
    let rgb: number[] = [
      srgb[i * 3] as number,
      srgb[i * 3 + 1] as number,
      srgb[i * 3 + 2] as number,
    ];
    let rough = info[i * 4 + 2] as number;
    if (patterned) {
      const shade = shadeMembrane(
        cpuKit,
        membraneSurfaceAt(compiled, i),
        rgb as [number, number, number],
        rough,
        compiled.material,
        registry,
      );
      rgb = [shade.r, shade.g, shade.b];
      rough = shade.roughness;
    }
    const along = vein[i * 2] as number;
    const lines = Math.max(line(vein[i * 2 + 1] as number, 6), line(along, 3) * 0.6);
    const dark = 1 - (info[i * 4 + 3] as number) * lines * (1 - along * 0.5) * 0.55;
    for (let c = 0; c < 3; c++)
      color[i * 3 + c] = srgbToLinear(Math.min(1, Math.max(0, (rgb[c] as number) * dark)));
    roughness[i] = Math.min(1, Math.max(0.04, rough));
  }
  return { color, roughness };
}

/** Hard parts already carry an sRGB colour and roughness per vertex. */
export function bakePartColors(compiled: CompiledCreature): BakedColors {
  const { color: srgb, info } = compiled.parts;
  const n = srgb.length / 3;
  const color = new Float32Array(n * 3);
  const roughness = new Float32Array(n);
  for (let i = 0; i < n * 3; i++) color[i] = srgbToLinear(srgb[i] as number);
  for (let i = 0; i < n; i++) roughness[i] = info[i * 2 + 1] as number;
  return { color, roughness };
}

/** Sclera, iris and pupil per vertex, the eye shader's rules evaluated at each vertex. */
export function bakeEyeColors(compiled: CompiledCreature): BakedColors {
  const { eye, iris, sclera } = compiled.eyes;
  const n = eye.length / 4;
  const color = new Float32Array(n * 3);
  const roughness = new Float32Array(n).fill(EYE_ROUGHNESS);
  for (let i = 0; i < n; i++) {
    const [ex, ey, ez, kind] = [eye[i * 4], eye[i * 4 + 1], eye[i * 4 + 2], eye[i * 4 + 3]] as [
      number,
      number,
      number,
      number,
    ];
    eyeColor(ex, ey, ez, kind, iris, i * 4, sclera, i * 3, color, i * 3);
  }
  return { color, roughness };
}

/**
 * The eye shader's colour at one point (linear light): sclera, iris with a darker rim and a
 * round, slit or goat pupil, from the point's place on the unit eyeball (`ex`, `ey`, `ez`) and
 * pupil `kind`; `iris` holds sRGB colour and size from `irisAt`, `sclera` sRGB colour from
 * `scleraAt`. Writes three values to `out` at `at`.
 */
export function eyeColor(
  ex: number,
  ey: number,
  ez: number,
  kind: number,
  iris: ArrayLike<number>,
  irisAt: number,
  sclera: ArrayLike<number>,
  scleraAt: number,
  out: Float32Array,
  at: number,
): void {
  const r = (iris[irisAt + 3] as number) * 0.62;
  const dist = Math.hypot(ex, ey);
  const irisMask = (1 - smooth(r - 0.04, r, dist)) * (ez >= 0 ? 1 : 0);
  const round = 1 - smooth(r * 0.42 - 0.03, r * 0.42, dist);
  const slit =
    (1 - smooth(r * 0.13 - 0.02, r * 0.13, Math.abs(ex))) *
    (1 - smooth(r * 0.92 - 0.03, r * 0.92, Math.abs(ey)));
  const goat =
    (1 - smooth(r * 0.15 - 0.02, r * 0.15, Math.abs(ey))) *
    (1 - smooth(r * 0.92 - 0.03, r * 0.92, Math.abs(ex)));
  const pupil = (kind >= 1.5 ? goat : kind >= 0.5 ? slit : round) * irisMask;
  const ring = smooth(r * 0.6, r, dist);
  for (let c = 0; c < 3; c++) {
    const irisC = (iris[irisAt + c] as number) * (1.15 - ring * 0.45);
    const base = (sclera[scleraAt + c] as number) * (1 - irisMask) + irisC * irisMask;
    out[at + c] = srgbToLinear(base * (1 - pupil) + 0.02 * pupil);
  }
}

/** Vertex colours for the skin, the hard parts, the eyes and the membranes. */
export function bakeVertexColors(
  compiled: CompiledCreature,
  registry: Registry,
): { skin: BakedColors; parts: BakedColors; eyes: BakedColors; membranes: BakedColors } {
  return {
    skin: bakeSkinColors(compiled, registry),
    parts: bakePartColors(compiled),
    eyes: bakeEyeColors(compiled),
    membranes: bakeMembraneColors(compiled, registry),
  };
}
