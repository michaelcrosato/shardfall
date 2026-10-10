import type { MeshData } from '../compile/compile.ts';

/**
 * What `@spawnforge/bake` hands an exporter (docs/design/11.1-textures.md): plain data, so the
 * exporter needs no part of the bake.
 */

/** An 8-bit RGBA image, rows from the top (glTF's texture origin), square. */
export interface BakedMap {
  readonly size: number;
  readonly data: Uint8Array;
  /** Colour data stored in sRGB (albedo, emissive); otherwise linear (normal, ORM). */
  readonly srgb: boolean;
}

/** A mesh split along its atlas's seams, with its texture coordinates and maps. */
export interface TexturedMesh extends MeshData {
  /** Texture coordinates, 0 to 1, v down from the top. */
  readonly uvs: Float32Array;
  /** MikkTSpace tangents with glTF's handedness (four per vertex), where there is a normal map. */
  readonly tangents?: Float32Array;
  /** Base colour; its alpha is the opacity where `blend` is set. */
  readonly albedo: BakedMap;
  /** Tangent-space normals. */
  readonly normal?: BakedMap;
  /** Occlusion (R), roughness (G) and metalness (B). */
  readonly orm: BakedMap;
  /** Glow, where anything glows, scaled into the map by `emissiveStrength`. */
  readonly emissive?: BakedMap;
  /** The glow's brightest value: the emissive map's multiplier (`KHR_materials_emissive_strength` above 1). */
  readonly emissiveStrength?: number;
  /** See-through somewhere: blended by the albedo's alpha. */
  readonly blend?: boolean;
  /** The median texel's size on the surface (metres). */
  readonly texel?: number;
}

/** Every mesh of a creature with its maps; a mesh left out keeps vertex colours. */
export interface BakedTextures {
  /** The skin's map size; the others follow from it. */
  readonly size: number;
  readonly skin?: TexturedMesh;
  readonly parts?: TexturedMesh;
  readonly eyes?: TexturedMesh;
  readonly membranes?: TexturedMesh;
  /** What could not be baked, and why (the export lists them). */
  readonly notes: readonly string[];
  /** Milliseconds per stage, summed over the meshes. */
  readonly timings?: Readonly<Record<string, number>>;
}
