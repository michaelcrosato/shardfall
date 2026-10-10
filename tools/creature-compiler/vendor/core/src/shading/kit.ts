/**
 * The pattern language. A pattern is written once against `Kit<F>`, where F is a number on the
 * CPU (for export bakes and tests) and a TSL node on the GPU (for rendering), so the same
 * function drives both. Hashing is integer-exact on both backends, so noise lines up.
 */
export interface Kit<F> {
  num(x: number): F;
  /** A per-creature value that does not change the shader (a uniform on the GPU). */
  param(x: number): F;
  add(a: F, b: F): F;
  sub(a: F, b: F): F;
  mul(a: F, b: F): F;
  div(a: F, b: F): F;
  min(a: F, b: F): F;
  max(a: F, b: F): F;
  abs(a: F): F;
  floor(a: F): F;
  fract(a: F): F;
  sin(a: F): F;
  cos(a: F): F;
  sqrt(a: F): F;
  pow(a: F, b: F): F;
  mix(a: F, b: F, t: F): F;
  clamp(x: F, lo: F, hi: F): F;
  smoothstep(e0: F, e1: F, x: F): F;
  /** 1 where x >= edge, else 0. */
  step(edge: F, x: F): F;
  /**
   * Hash of three integer-valued inputs (lattice coordinates between -30000 and 30000) to
   * [0, 1). `salt` picks an independent hash.
   */
  hash3(x: F, y: F, z: F, salt: number): F;
}

/** What a pattern knows about a point on the skin. Positions are in torso lengths. */
export interface Surface<F> {
  /** Rest-pose position, in torso lengths. */
  readonly x: F;
  readonly y: F;
  readonly z: F;
  /** Rest-pose normal. */
  readonly nx: F;
  readonly ny: F;
  readonly nz: F;
  /** Along the main axis: 0 at the snout tip, 1 at the tail tip. */
  readonly spine: F;
  /** Around the body: -1 at the belly midline, 0 on the flank, 1 along the spine. */
  readonly height: F;
  /** Along a limb: 0 at the root, 1 at the tip (and on toes); 0 off limbs. */
  readonly limb: F;
  /** Depth into creases where sections join, 0 to 1. */
  readonly crease: F;
  /** Region weights, summing to 1. */
  readonly head: F;
  readonly torso: F;
  readonly limbs: F;
  readonly tail: F;
  /** 1 on wing and fin membranes and the tubes that carry them, 0 elsewhere (9.3). */
  readonly wings: F;
  /** Height above the ground in the rest pose, in torso lengths. */
  readonly ground: F;
  /**
   * Size of one screen pixel on the skin, in torso lengths (0 when there is no screen, as in
   * bakes). Patterns fade detail smaller than a few pixels so it never aliases into speckle.
   */
  readonly pixel: F;
  /**
   * The size relief fades by, when it differs from `pixel`: a bake's normal map takes slopes
   * over texels, which resolves finer relief than a bump from screen derivatives
   * (docs/design/11.1-textures.md). Unset, relief fades by `pixel`.
   */
  readonly reliefPixel?: F;
  /** Seconds, for pulses: the pose's clock live, 0 in stills and bakes. */
  readonly time: F;
}

/** One layer's contribution. */
export interface LayerOutput<F> {
  /** Where the layer's colour shows, 0 to 1. */
  readonly mask: F;
  /** A different colour than the layer's `color` param (hex). */
  readonly color?: string;
  /** Surface relief to add, in torso lengths. */
  readonly height?: F;
  /** Roughness where the mask is 1 (or where `coat` is, when given). */
  readonly roughness?: F;
  /**
   * Where the layer's roughness applies, when it covers more than its colour does: a wet coat
   * tints a little but shines everywhere. Defaults to `mask`.
   */
  readonly coat?: F;
  /**
   * Glow in the layer's colour, added on top of lighting (1 matches a lit surface). Live only:
   * bakes leave it out until texture maps (milestone 11.1).
   */
  readonly emissive?: F;
  /** A second colour laid before the main one, such as a rosette's centre. */
  readonly under?: { readonly mask: F; readonly color: string };
}

/** Hooks a pattern module provides. */
export interface PatternHooks {
  shade<F>(k: Kit<F>, s: Surface<F>, params: Record<string, unknown>, seed: number): LayerOutput<F>;
}
