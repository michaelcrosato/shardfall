/**
 * Levels of detail (docs/design/11.2-lod.md): what `@spawnforge/bake/lod` hands the exporter and
 * the runtime, as plain data, and the choice of a level by screen size.
 */

/** One level of a mesh's chain: fewer triangles over the full mesh's own vertices. */
export interface LodLevel {
  /** The share of the full mesh's triangles it was simplified toward (0.5, 0.25, 0.1). */
  readonly ratio: number;
  readonly indices: Uint32Array;
  readonly triangles: number;
  /** How far the simplified surface strays from the full one, in metres, in the bind pose. */
  readonly error: number;
}

/** The levels below full detail, coarsest last, for the meshes that have them. */
export interface LodChain {
  readonly skin?: readonly LodLevel[];
  readonly parts?: readonly LodLevel[];
}

/** The levels a chain holds, as shares of the full mesh's triangles. */
export const LOD_RATIOS = [0.5, 0.25, 0.1] as const;

/** A camera as the choice needs it: a perspective field of view, or an orthographic height. */
export type LodView =
  | { readonly fov: number; readonly distance: number }
  | { readonly height: number };

/**
 * How many pixels an error of `error` metres covers on screen: `pixels` is the viewport's height,
 * `fov` the vertical field of view in degrees, `height` an orthographic view's height in metres.
 */
export function projectedError(error: number, view: LodView, pixels: number): number {
  if ('height' in view) return view.height > 0 ? (error * pixels) / view.height : 0;
  const d = Math.max(view.distance, 1e-6);
  return (error * pixels) / (2 * d * Math.tan((view.fov * Math.PI) / 360));
}

/**
 * The coarsest level whose error projects to under one pixel: 0 is full detail, `n` the chain's
 * `n`th level. Levels are tried coarsest first; errors rise level by level.
 */
export function pickLevel(
  levels: readonly Pick<LodLevel, 'error'>[],
  view: LodView,
  pixels = 1080,
): number {
  for (let k = levels.length; k >= 1; k--)
    if (projectedError((levels[k - 1] as LodLevel).error, view, pixels) < 1) return k;
  return 0;
}

/**
 * The share of the viewport's height an object `size` metres tall covers below which a level's
 * error stays under one pixel (glTF's `MSFT_screencoverage`), at `pixels` pixels.
 */
export function screenCoverage(error: number, size: number, pixels = 1080): number {
  return error > 0 ? Math.min(1, size / (error * pixels)) : 0;
}
