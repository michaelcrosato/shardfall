import { Vector3 } from 'three';
import type { CompiledCreature } from './compile.ts';

/** Kilograms per cubic metre: creatures are about as dense as water. */
export const DENSITY = 1000;

/** Volume and centroid of the closed skin mesh (signed tetrahedra from the origin). */
export function volumeOf(c: Pick<CompiledCreature, 'skin' | 'bones'>): {
  volume: number;
  centre: Vector3;
} {
  const p = c.skin.positions;
  const idx = c.skin.indices;
  let volume = 0;
  const centre = new Vector3();
  // Eyelids are open shells over the eyes, not part of the body's closed surface.
  const lid = (v: number) => c.bones.sections[c.skin.skinIndex[v * 4] as number] === 'lid';
  for (let i = 0; i < idx.length; i += 3) {
    if (lid(idx[i] as number)) continue;
    const a = (idx[i] as number) * 3;
    const b = (idx[i + 1] as number) * 3;
    const d = (idx[i + 2] as number) * 3;
    const ax = p[a] as number;
    const ay = p[a + 1] as number;
    const az = p[a + 2] as number;
    const bx = p[b] as number;
    const by = p[b + 1] as number;
    const bz = p[b + 2] as number;
    const cx = p[d] as number;
    const cy = p[d + 1] as number;
    const cz = p[d + 2] as number;
    const v = (ax * (by * cz - bz * cy) - ay * (bx * cz - bz * cx) + az * (bx * cy - by * cx)) / 6;
    volume += v;
    centre.x += (v * (ax + bx + cx)) / 4;
    centre.y += (v * (ay + by + cy)) / 4;
    centre.z += (v * (az + bz + cz)) / 4;
  }
  if (Math.abs(volume) > 1e-12) centre.divideScalar(volume);
  return { volume: Math.abs(volume), centre };
}
