import type { FurSpec, Region } from '../blueprint/creature.ts';
import type { CompiledCreature } from '../compile/compile.ts';
import { allEyes } from '../compile/types.ts';
import type { Kit } from './kit.ts';

/**
 * Where shell fur grows and how long it is (docs/design/8.4-materials.md), written once against
 * the kit: the renderer's fur and skin shaders run it as TSL, bakes as numbers
 * (docs/design/11.1-textures.md).
 */

/** An eye the fur keeps clear of: its rest centre and radius, in metres. */
export interface FurEye {
  readonly x: number;
  readonly y: number;
  readonly z: number;
  readonly radius: number;
}

/** A skin vertex's `body` and `region` attributes. */
export interface FurInputs<F> {
  readonly body: { readonly x: F; readonly y: F; readonly z: F; readonly w: F };
  readonly region: { readonly x: F; readonly y: F; readonly z: F; readonly w: F };
  /** Rest position in metres. */
  readonly position: { readonly x: F; readonly y: F; readonly z: F };
}

/** Where fur grows, 0 to 1, from a layer region's mask. */
function furRegion<F>(k: Kit<F>, region: Region, s: FurInputs<F>): F {
  const height = s.body.y;
  const notLimb = k.sub(k.num(1), s.region.z);
  switch (region) {
    case 'all':
      return k.num(1);
    case 'back':
      return k.mul(k.smoothstep(k.num(-0.15), k.num(0.35), height), notLimb);
    case 'belly':
      return k.mul(k.sub(k.num(1), k.smoothstep(k.num(-0.35), k.num(0.15), height)), notLimb);
    case 'head':
      return s.region.x;
    case 'torso':
      return s.region.y;
    case 'limbs':
      return s.region.z;
    case 'tail':
      return s.region.w;
    case 'wings':
      return k.num(0);
  }
}

/**
 * How long the fur is at a skin point, relative to `length`: its regions, shorter in creases
 * (and so along the lip line) and on the head, and none inside the mouth.
 */
export function furReach<F>(
  k: Kit<F>,
  fur: FurSpec,
  s: FurInputs<F>,
  eyes: readonly FurEye[] = [],
): F {
  let where = k.num(0);
  for (const r of fur.region) where = k.max(where, furRegion(k, r, s));
  const { body, region } = s;
  const inMouth = k.sub(k.num(1), k.step(k.num(-0.5), body.z));
  // Short on the toes, as on a paw; none on wing and fin tubes (`limb + 2`).
  const wing = k.step(k.num(1.5), body.z);
  const toes = k.sub(
    k.num(1),
    k.mul(k.smoothstep(k.num(0.8), k.num(1), k.sub(body.z, k.mul(wing, k.num(2)))), k.num(0.7)),
  );
  let reach = k.mul(
    k.mul(
      k.mul(
        k.mul(
          k.mul(where, k.sub(k.num(1), k.mul(k.clamp(body.w, k.num(0), k.num(1)), k.num(0.8)))),
          k.sub(k.num(1), k.mul(region.x, k.num(0.4))),
        ),
        k.sub(k.num(1), inMouth),
      ),
      toes,
    ),
    k.sub(k.num(1), wing),
  );
  // Clear round each eye, so the lids and the eye show.
  for (const eye of eyes) {
    const dx = k.sub(s.position.x, k.num(eye.x));
    const dy = k.sub(s.position.y, k.num(eye.y));
    const dz = k.sub(s.position.z, k.num(eye.z));
    const d = k.sqrt(k.add(k.add(k.mul(dx, dx), k.mul(dy, dy)), k.mul(dz, dz)));
    reach = k.mul(reach, k.smoothstep(k.num(eye.radius * 1.15), k.num(eye.radius * 2), d));
  }
  return reach;
}

/** Each eye's rest centre (its bone) and radius (its farthest vertex). */
export function furEyes(compiled: CompiledCreature): FurEye[] {
  const { positions, skinIndex } = compiled.eyes;
  const bones = compiled.bones.positions;
  return allEyes(compiled.rig).map((bone) => {
    const x = bones[bone * 3] as number;
    const y = bones[bone * 3 + 1] as number;
    const z = bones[bone * 3 + 2] as number;
    let radius = 0;
    for (let v = 0; v < positions.length / 3; v++)
      if (skinIndex[v * 4] === bone)
        radius = Math.max(
          radius,
          Math.hypot(
            (positions[v * 3] as number) - x,
            (positions[v * 3 + 1] as number) - y,
            (positions[v * 3 + 2] as number) - z,
          ),
        );
    return { x, y, z, radius };
  });
}

/**
 * The coat's mean look over the skin, for a creature shown without its shells (an export, and
 * the round trip that checks it): the shells' shade averaged from root (0.55) to tip (1), and
 * their roughness.
 */
export const COAT = { shade: 0.78, roughness: 0.85 } as const;
