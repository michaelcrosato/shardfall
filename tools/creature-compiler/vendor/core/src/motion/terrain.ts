import { createRng } from '../rng.ts';
import type { Ground } from './controller.ts';

/**
 * A rolling test course: smooth hills and dips from a few seeded sine waves, with a flat start.
 * Returns the height and normal at (x, z).
 */
export function testCourse(
  seed = 1,
  amplitude = 0.25,
  flatRadius = 1.5,
): Ground & { height(x: number, z: number): number } {
  const rng = createRng(seed).stream('terrain');
  const waves = Array.from({ length: 5 }, () => ({
    kx: rng.float(-1.2, 1.2),
    kz: rng.float(-1.2, 1.2),
    phase: rng.float(0, Math.PI * 2),
    amp: rng.float(0.3, 1),
  }));
  const total = waves.reduce((a, w) => a + w.amp, 0);
  const height = (x: number, z: number) => {
    let h = 0;
    for (const w of waves) h += Math.sin(w.kx * x + w.kz * z + w.phase) * w.amp;
    const r = Math.hypot(x, z);
    // No flat start (radius 0) fades in nothing, even at the origin (0 / 0).
    const fade = flatRadius > 0 ? Math.min(1, Math.max(0, (r - flatRadius) / flatRadius)) : 1;
    return (h / total) * amplitude * fade * fade;
  };
  const ground = ((x: number, z: number) => {
    const e = 0.02;
    const dx = (height(x + e, z) - height(x - e, z)) / (2 * e);
    const dz = (height(x, z + e) - height(x, z - e)) / (2 * e);
    const len = Math.hypot(dx, 1, dz);
    return { height: height(x, z), normal: [-dx / len, 1 / len, -dz / len] as const };
  }) as Ground & { height(x: number, z: number): number };
  ground.height = height;
  return ground;
}

/**
 * Ground rising at `degrees` toward `toward` (degrees, 0 is +Z), flat until `from` metres that
 * way: the slope a scenario lands a flyer on (10.4).
 */
export function slope(degrees: number, toward = 0, from = 0): Ground {
  const rise = Math.tan((degrees * Math.PI) / 180);
  const dx = Math.sin((toward * Math.PI) / 180);
  const dz = Math.cos((toward * Math.PI) / 180);
  const len = Math.hypot(rise, 1);
  const normal = [(-rise * dx) / len, 1 / len, (-rise * dz) / len] as const;
  const flat = [0, 1, 0] as const;
  return (x, z) => {
    const along = x * dx + z * dz - from;
    return along > 0 || from === 0 ? { height: rise * along, normal } : { height: 0, normal: flat };
  };
}

/**
 * Open sea for a creature `scale` metres long: the surface at y 0 and the bed four body lengths
 * and 2 m below, deep enough that nothing touches it. Swimming filmstrips, scenarios' `"sea"`
 * and baked swim cycles use it.
 */
export function openSea(scale: number): {
  ground: Ground;
  water: (x: number, z: number) => { surface: number };
} {
  const bed = -(4 * scale + 2);
  return { ground: () => ({ height: bed }), water: () => ({ surface: 0 }) };
}

/** A round lake: its centre, radius and depth at the middle (m), and its surface height. */
export interface Lake {
  readonly x: number;
  readonly z: number;
  readonly radius: number;
  readonly depth: number;
  readonly surface?: number;
}

/**
 * Carves a lake into a ground: a smooth bowl down to `depth` below the surface at the middle,
 * rising to the shore at `radius` and a little beyond, with water over it (10.3). Returns the new
 * ground and the water, to pass to `update` together.
 */
export function withLake(
  ground: Ground,
  lake: Lake,
): { ground: Ground; water: (x: number, z: number) => { surface: number } | null } {
  const surface = lake.surface ?? 0;
  const shore = lake.radius * 1.25;
  const bowl = (x: number, z: number) => {
    const r = Math.hypot(x - lake.x, z - lake.z);
    if (r >= shore) return 0;
    const u = r / shore;
    return (1 - u * u) ** 2;
  };
  const carved = (x: number, z: number) => {
    const g = ground(x, z).height;
    const b = bowl(x, z);
    // The bed: the ground lowered so the middle sits `depth` under the surface.
    return b > 0 ? Math.min(g, g + (surface - lake.depth - g) * b) : g;
  };
  return {
    ground: (x, z) => {
      const e = 0.02;
      const dx = (carved(x + e, z) - carved(x - e, z)) / (2 * e);
      const dz = (carved(x, z + e) - carved(x, z - e)) / (2 * e);
      const len = Math.hypot(dx, 1, dz);
      return { height: carved(x, z), normal: [-dx / len, 1 / len, -dz / len] as const };
    },
    water: (x, z) => (carved(x, z) < surface ? { surface } : null),
  };
}
