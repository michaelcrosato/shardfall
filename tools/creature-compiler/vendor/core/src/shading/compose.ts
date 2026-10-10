import { hexToRgb } from '../blueprint/colors.ts';
import type { FurSpec, LayerSpec, Region, SkinMaterial } from '../blueprint/creature.ts';
import type { PatternModule, Registry } from '../registry.ts';
import { deriveSeed } from '../rng.ts';
import type { Kit, PatternHooks, Surface } from './kit.ts';
import { cells, fbm, valueNoise } from './noise.ts';

/** Everything the skin shader needs, as plain data. Colours are sRGB hex. */
export interface SkinMaterialSpec {
  readonly base: string;
  readonly material: SkinMaterial;
  readonly layers: readonly (LayerSpec & { readonly seed: number })[];
  /** Shell fur over the skin, or none. */
  readonly fur: FurSpec | null;
}

export interface SkinShade<F> {
  /** sRGB colour, 0 to 1. */
  readonly r: F;
  readonly g: F;
  readonly b: F;
  readonly roughness: F;
  /** Relief in torso lengths, for bump mapping. */
  readonly height: F;
  /** Glow in linear light, added on top of lighting (live only; bakes leave it out). */
  readonly er: F;
  readonly eg: F;
  readonly eb: F;
}

export function skinMaterialSpec(
  base: string,
  material: SkinMaterial,
  layers: readonly LayerSpec[],
  seed: number,
  fur: FurSpec | null = null,
): SkinMaterialSpec {
  return {
    base,
    material,
    layers: layers.map((l) => ({ ...l, seed: deriveSeed(seed, `layer:${l.id}`) % 997 })),
    fur,
  };
}

/** 1 where a feature of `size` (torso lengths) spans several pixels, fading to 0 below that. */
export function detail<F>(k: Kit<F>, s: Surface<F>, size: number): F {
  return k.sub(k.num(1), k.smoothstep(k.num(size * 0.12), k.num(size * 0.45), s.pixel));
}

/**
 * Like `detail`, for relief. Bump mapping takes slopes per 2×2 pixel block, so relief needs
 * features about 25 pixels across to look smooth and is gone below about 7.
 */
export function relief<F>(k: Kit<F>, s: Surface<F>, size: number): F {
  return k.sub(
    k.num(1),
    k.smoothstep(k.num(size * 0.04), k.num(size * 0.15), s.reliefPixel ?? s.pixel),
  );
}

function regionMask<F>(k: Kit<F>, s: Surface<F>, region: Region): F {
  switch (region) {
    case 'all':
      return k.num(1);
    case 'back':
      return k.mul(
        k.smoothstep(k.num(-0.15), k.num(0.35), s.height),
        k.sub(k.num(1), k.add(s.limbs, s.wings)),
      );
    case 'belly':
      return k.mul(
        k.sub(k.num(1), k.smoothstep(k.num(-0.35), k.num(0.15), s.height)),
        k.sub(k.num(1), k.add(s.limbs, s.wings)),
      );
    case 'head':
      return s.head;
    case 'torso':
      return s.torso;
    case 'limbs':
      return s.limbs;
    case 'tail':
      return s.tail;
    case 'wings':
      // Wing and fin membranes, and the tubes that carry them (docs/design/9.3-wings-fins.md).
      return s.wings;
  }
}

/**
 * How a base material meets the light, beside the surface `shadeSkin` draws for it. Both
 * backends read it: bakes take the roughness, the renderer the rest (docs/design/8.4-materials.md).
 */
export interface MaterialLook {
  readonly roughness: number;
  /** Wrap lighting: how far past the terminator diffuse light reaches (0 none, 1 all round). */
  readonly wrap: number;
  /** Tint of the wrapped light (linear RGB), which reads as light scattered under the skin. */
  readonly scatter: readonly [number, number, number];
  /** A lacquer layer over the surface, 0 to 1, and its roughness. */
  readonly clearcoat: number;
  readonly clearcoatRoughness: number;
}

export const MATERIAL_LOOK: Readonly<Record<SkinMaterial, MaterialLook>> = {
  skin: {
    roughness: 0.6,
    wrap: 0.35,
    scatter: [1, 0.66, 0.56],
    clearcoat: 0,
    clearcoatRoughness: 0,
  },
  hide: {
    roughness: 0.82,
    wrap: 0.2,
    scatter: [1, 0.76, 0.66],
    clearcoat: 0,
    clearcoatRoughness: 0,
  },
  scales: {
    roughness: 0.42,
    wrap: 0.12,
    scatter: [1, 0.85, 0.75],
    clearcoat: 0,
    clearcoatRoughness: 0,
  },
  chitin: {
    roughness: 0.32,
    wrap: 0,
    scatter: [1, 1, 1],
    clearcoat: 0.7,
    clearcoatRoughness: 0.18,
  },
};

/** The material's fine scales: the size, in torso lengths. */
const MATERIAL_SCALE = 0.014;
/** Chitin plates along the snout-to-tail axis, and along each limb. */
const CHITIN_PLATES = 14;
const CHITIN_LIMB_PLATES = 4;

/** One scale of an overlapping row: where in it a point is, and its relief. */
export interface Shingle<F> {
  /** 0 at the visible part's front (just behind the scale ahead) rising to 1 at its free rear edge. */
  readonly plate: F;
  /** Distance to the scale's rim, as a share of its radius (0 on it, 1 at its centre). */
  readonly edge: F;
  /** A random value per scale in [0, 1). */
  readonly id: F;
  /**
   * The relief, 0 to 1: the highest of the scales covering the point, each rising toward its
   * rear edge and falling to nothing at its rim. Continuous, so bump mapping never sees a step.
   */
  readonly height: F;
}

/** Scale radius in lattice cells: big enough that the scales overlap everywhere. */
const SHINGLE_RADIUS = 0.85;

/**
 * Overlapping scales of `size` torso lengths, like a fish's or a snake's: round scales on a
 * staggered lattice, each rising along the body's tail-ward direction (down the limbs on limbs)
 * to a free rear edge and falling away at its rim. A point shows the highest scale there, so each
 * raised rear edge lies over the low front of the scale behind it, in scalloped rows.
 */
export function shingles<F>(k: Kit<F>, s: Surface<F>, size: number, salt: number): Shingle<F> {
  const f = k.num(1 / size);
  const x = k.mul(s.x, f);
  const y = k.mul(s.y, f);
  const z = k.mul(s.z, f);
  const half = k.num(0.5);
  const one = k.num(1);
  // Tail-ward: -Z on the body, -Y down a limb. The two are orthogonal, so normalize the blend.
  const back = k.sub(one, s.limbs);
  const norm = k.sqrt(k.add(k.mul(back, back), k.mul(s.limbs, s.limbs)));
  const ty = k.div(k.sub(k.num(0), s.limbs), norm);
  const tz = k.div(k.sub(k.num(0), back), norm);
  const R = k.num(SHINGLE_RADIUS);
  const jitter = k.num(0.25);
  let height = k.num(0);
  let plate = k.num(0);
  let edge = k.num(0);
  let id = k.num(0);
  const bz = k.floor(z);
  for (let dz = -1; dz <= 1; dz++) {
    const cz = dz === 0 ? bz : k.add(bz, k.num(dz));
    // Rows along z are offset half a cell in x and y, so both the back and the flanks see
    // staggered rows.
    const shift = k.fract(k.mul(cz, half));
    const bx = k.floor(k.sub(x, shift));
    const by = k.floor(k.sub(y, shift));
    for (let dy = -1; dy <= 1; dy++) {
      const cy = dy === 0 ? by : k.add(by, k.num(dy));
      for (let dx = -1; dx <= 1; dx++) {
        const cx = dx === 0 ? bx : k.add(bx, k.num(dx));
        const jx = k.mul(k.sub(k.hash3(cx, cy, cz, salt + 11), half), jitter);
        const jy = k.mul(k.sub(k.hash3(cx, cy, cz, salt + 23), half), jitter);
        const jz = k.mul(k.sub(k.hash3(cx, cy, cz, salt + 37), half), jitter);
        // From the point to the scale's centre, in cells.
        const ox = k.sub(k.add(k.add(k.add(cx, half), shift), jx), x);
        const oy = k.sub(k.add(k.add(k.add(cy, half), shift), jy), y);
        const oz = k.sub(k.add(k.add(cz, half), jz), z);
        const d = k.sqrt(k.add(k.add(k.mul(ox, ox), k.mul(oy, oy)), k.mul(oz, oz)));
        // How far toward the tail the point lies from the scale's centre, as a share of R.
        const along = k.div(k.add(k.mul(oy, ty), k.mul(oz, tz)), R);
        const rise = k.smoothstep(k.num(-0.9), k.num(0.7), along);
        const rim = k.div(k.sub(R, d), R);
        const cap = k.mul(
          k.add(k.num(0.25), k.mul(rise, k.num(0.75))),
          k.smoothstep(k.num(0), k.num(0.3), rim),
        );
        const higher = k.step(height, cap);
        height = k.max(height, cap);
        plate = k.mix(plate, rise, higher);
        edge = k.mix(edge, k.max(k.num(0), rim), higher);
        id = k.mix(id, k.hash3(cx, cy, cz, salt + 53), higher);
      }
    }
  }
  return { plate, edge, id, height };
}

/** sRGB (0 to 1) to linear light. */
function toLinear(c: number): number {
  return c <= 0.04045 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4;
}

/** The base material's own surface: relief, a darkening of the albedo and a roughness shift. */
function materialSurface<F>(
  k: Kit<F>,
  s: Surface<F>,
  material: SkinMaterial,
): { height: F; dark: F; roughness: F } {
  const one = k.num(1);
  const scaled = (v: F, by: number) => k.mul(v, k.num(by));
  switch (material) {
    case 'skin': {
      // Faint pores in the relief, and a little unevenness in the sheen.
      const pores = valueNoise(k, scaled(s.x, 250), scaled(s.y, 250), scaled(s.z, 250), 401);
      const sheen = valueNoise(k, scaled(s.x, 40), scaled(s.y, 40), scaled(s.z, 40), 409);
      return {
        height: k.mul(k.mul(k.sub(pores, k.num(0.5)), k.num(0.00012)), relief(k, s, 0.004)),
        dark: k.num(0),
        roughness: k.mul(k.sub(sheen, k.num(0.5)), k.num(0.08)),
      };
    }
    case 'hide': {
      // A network of wrinkles at two sizes, deeper where sections join.
      const groove = (size: number, salt: number, reach: 2 | 3, width: number) => {
        const f = k.num(1 / size);
        const c = cells(k, k.mul(s.x, f), k.mul(s.y, f), k.mul(s.z, f), 0.9, salt, { reach });
        return k.sub(one, k.smoothstep(k.num(0), k.num(width), k.sub(c.second, c.distance)));
      };
      const coarse = groove(0.05, 701, 3, 0.12);
      const fine = groove(0.018, 709, 2, 0.15);
      const depth = k.add(k.num(0.4), k.mul(s.crease, k.num(0.8)));
      const cut = k.add(
        k.mul(k.mul(coarse, k.num(0.0016)), relief(k, s, 0.05)),
        k.mul(k.mul(fine, k.num(0.0006)), relief(k, s, 0.018)),
      );
      const seen = k.add(
        k.mul(k.mul(coarse, k.num(0.7)), detail(k, s, 0.05)),
        k.mul(k.mul(fine, k.num(0.3)), detail(k, s, 0.018)),
      );
      return {
        height: k.mul(k.sub(k.num(0), cut), depth),
        dark: k.mul(k.mul(seen, k.num(0.18)), k.min(one, depth)),
        roughness: k.num(0),
      };
    }
    case 'scales': {
      const sh = shingles(k, s, MATERIAL_SCALE, 503);
      // The low front of each scale, tucked under the one ahead, lies in its shadow.
      const low = k.sub(one, sh.height);
      return {
        height: k.mul(k.mul(sh.height, k.num(MATERIAL_SCALE * 0.12)), relief(k, s, MATERIAL_SCALE)),
        dark: k.mul(k.mul(k.mul(low, low), k.num(0.3)), detail(k, s, MATERIAL_SCALE)),
        roughness: k.num(0),
      };
    }
    case 'chitin': {
      // Plates across the body and along the limbs, and a seam down the back.
      const period = (u: F) => {
        const f = k.fract(u);
        return k.min(f, k.sub(one, f));
      };
      const across = k.mix(
        period(scaled(s.spine, CHITIN_PLATES)),
        period(scaled(s.limb, CHITIN_LIMB_PLATES)),
        s.limbs,
      );
      const seamAcross = k.sub(one, k.smoothstep(k.num(0.025), k.num(0.09), across));
      const back = k.mul(k.smoothstep(k.num(0.985), k.num(0.997), s.height), k.sub(one, s.limbs));
      const seam = k.mul(k.max(seamAcross, back), k.sub(one, s.head));
      const dome = k.smoothstep(k.num(0.02), k.num(0.5), across);
      const sheen = fbm(k, scaled(s.x, 14), scaled(s.y, 14), scaled(s.z, 14), 2, 607);
      return {
        height: k.add(
          k.mul(k.sub(k.mul(dome, k.num(0.0016)), k.mul(seam, k.num(0.0018))), relief(k, s, 0.06)),
          k.mul(sheen, k.num(0.001)),
        ),
        dark: k.mul(k.mul(seam, k.num(0.45)), detail(k, s, 0.03)),
        roughness: k.num(0),
      };
    }
  }
}

/** Colour, roughness and relief before or after a stack of layers, and the glow they add. */
interface Layered<F> {
  readonly r: F;
  readonly g: F;
  readonly b: F;
  readonly roughness: F;
  readonly height: F;
  readonly er: F;
  readonly eg: F;
  readonly eb: F;
}

/** Every layer over a surface, bottom first, each masked by its region. */
function layerStack<F>(
  k: Kit<F>,
  s: Surface<F>,
  layers: readonly (LayerSpec & { readonly seed: number })[],
  registry: Registry,
  start: Omit<Layered<F>, 'er' | 'eg' | 'eb'>,
): Layered<F> {
  let { r, g, b, roughness, height } = start;
  let er = k.num(0);
  let eg = k.num(0);
  let eb = k.num(0);
  for (const layer of layers) {
    const module = registry.get('pattern', layer.type) as PatternModule | undefined;
    const hooks = module?.hooks as PatternHooks | undefined;
    if (!hooks) continue;
    const out = hooks.shade(k, s, layer.params as Record<string, unknown>, layer.seed);
    const where = k.mul(k.param(layer.strength), regionMask(k, s, layer.region));
    if (out.under) {
      const [ur, ug, ub] = hexToRgb(out.under.color);
      const w = k.mul(k.clamp(out.under.mask, k.num(0), k.num(1)), where);
      r = k.mix(r, k.param(ur), w);
      g = k.mix(g, k.param(ug), w);
      b = k.mix(b, k.param(ub), w);
    }
    const weight = k.mul(k.clamp(out.mask, k.num(0), k.num(1)), where);
    const hex =
      out.color ??
      (typeof layer.params.color === 'string' ? (layer.params.color as string) : undefined);
    if (hex) {
      const [lr, lg, lb] = hexToRgb(hex);
      r = k.mix(r, k.param(lr), weight);
      g = k.mix(g, k.param(lg), weight);
      b = k.mix(b, k.param(lb), weight);
      if (out.emissive !== undefined) {
        const glow = k.mul(k.max(out.emissive, k.num(0)), where);
        er = k.add(er, k.mul(glow, k.param(toLinear(lr))));
        eg = k.add(eg, k.mul(glow, k.param(toLinear(lg))));
        eb = k.add(eb, k.mul(glow, k.param(toLinear(lb))));
      }
    }
    if (out.roughness !== undefined)
      roughness = k.mix(
        roughness,
        out.roughness,
        out.coat === undefined ? weight : k.mul(k.clamp(out.coat, k.num(0), k.num(1)), where),
      );
    if (out.height !== undefined) height = k.add(height, k.mul(out.height, where));
  }

  return { r, g, b, roughness, height, er, eg, eb };
}

/**
 * A membrane's colour (docs/design/9.3-wings-fins.md): its module's own colour (sRGB, 0 to 1)
 * under the layers whose region is `wings`. Other layers belong to the skin.
 */
export function shadeMembrane<F>(
  k: Kit<F>,
  s: Surface<F>,
  base: readonly [F, F, F],
  roughness: F,
  spec: SkinMaterialSpec,
  registry: Registry,
): Layered<F> {
  return layerStack(
    k,
    s,
    spec.layers.filter((l) => l.region === 'wings'),
    registry,
    { r: base[0], g: base[1], b: base[2], roughness, height: k.num(0) },
  );
}

/** Whether any layer draws on wings, so membranes need the pattern stack at all. */
export function hasWingLayers(spec: SkinMaterialSpec): boolean {
  return spec.layers.some((l) => l.region === 'wings');
}

/** Base surface plus every layer, bottom first. */
export function shadeSkin<F>(
  k: Kit<F>,
  s: Surface<F>,
  spec: SkinMaterialSpec,
  registry: Registry,
): SkinShade<F> {
  const [br, bg, bb] = hexToRgb(spec.base);
  // A little low-frequency variation keeps large areas from looking flat.
  const variation = k.sub(
    fbm(k, k.mul(s.x, k.num(6)), k.mul(s.y, k.num(6)), k.mul(s.z, k.num(6)), 2, 401),
    k.num(0.5),
  );
  const base = materialSurface(k, s, spec.material);
  const tone = k.mul(k.add(k.num(1), k.mul(variation, k.num(0.16))), k.sub(k.num(1), base.dark));
  const { r, g, b, roughness, height, er, eg, eb } = layerStack(k, s, spec.layers, registry, {
    r: k.mul(k.param(br), tone),
    g: k.mul(k.param(bg), tone),
    b: k.mul(k.param(bb), tone),
    roughness: k.add(k.num(MATERIAL_LOOK[spec.material].roughness), base.roughness),
    height: base.height,
  });

  // Creases where sections join read a little darker, like ambient occlusion.
  const shade = k.sub(k.num(1), k.mul(s.crease, k.num(0.25)));
  return {
    r: k.mul(r, shade),
    g: k.mul(g, shade),
    b: k.mul(b, shade),
    roughness: k.clamp(roughness, k.num(0.04), k.num(1)),
    height,
    er,
    eg,
    eb,
  };
}

/** Inside the mouth: wet colours in linear light, and where they apply. */
export interface MouthShade<F> {
  /** 1 inside the mouth (cavity, lips' inner side, gums, tongue), else 0. */
  readonly inside: F;
  /** Linear-light colour. */
  readonly r: F;
  readonly g: F;
  readonly b: F;
  readonly roughness: F;
}

/**
 * The mouth's colours from the skin's body coordinates (docs/design/8.3-heads.md): inside
 * vertices carry `limb = -1 - depth` (0 at the lips, 1 at the throat) and their kind in
 * `crease` (0 cavity, 1 lips and gums, 2 tongue). The cavity goes from wet red to near black
 * toward the throat; gums are pink, and the tongue a lighter pink that darkens with depth.
 */
export function shadeMouth<F>(k: Kit<F>, limb: F, crease: F): MouthShade<F> {
  const inside = k.sub(k.num(1), k.step(k.num(-0.5), limb));
  const depth = k.clamp(k.sub(k.num(-1), limb), k.num(0), k.num(1));
  const gum = k.mul(k.step(k.num(0.5), crease), k.sub(k.num(1), k.step(k.num(1.5), crease)));
  const tongue = k.step(k.num(1.5), crease);
  const dim = (c: number, keep: number) =>
    k.mul(k.num(c), k.sub(k.num(1), k.mul(depth, k.num(1 - keep))));
  const channel = (cavity: number, throat: number, gums: number, tip: number) => {
    const wall = k.mix(k.num(cavity), k.num(throat), k.smoothstep(k.num(0), k.num(1), depth));
    const withGum = k.mix(wall, dim(gums, 0.55), gum);
    return k.mix(withGum, dim(tip, 0.45), tongue);
  };
  return {
    inside,
    r: channel(0.3, 0.025, 0.42, 0.52),
    g: channel(0.055, 0.004, 0.12, 0.17),
    b: channel(0.06, 0.005, 0.13, 0.18),
    roughness: k.num(0.3),
  };
}
