import type { CreatureSpec } from '../blueprint/creature.ts';
import type { GaitModule, Registry } from '../registry.ts';
import type { GaitInfo, MotionData } from './controller.ts';

/** A setting that is one number or `[slowest, fastest]`, as a pair. */
function profile(value: unknown, fallback: readonly [number, number]): [number, number] {
  if (typeof value === 'number') return [value, value];
  if (Array.isArray(value) && value.length === 2 && value.every((v) => typeof v === 'number'))
    return [value[0] as number, value[1] as number];
  return [fallback[0], fallback[1]];
}

/** Where each leg's foot lands, by `2 × pair + (right ? 1 : 0)`, from the wave or the module. */
function phasesOf(
  module: GaitModule,
  pairs: number,
  wave: number,
  params: Readonly<Record<string, unknown>>,
): number[] {
  const out: number[] = [];
  for (let pair = 0; pair < pairs; pair++)
    for (const side of ['left', 'right'] as const) {
      const raw = module.offsets
        ? module.offsets({ pair, side, pairs }, params)
        : pair * wave + (side === 'right' ? 0.5 : 0);
      out.push(((raw % 1) + 1) % 1);
    }
  return out;
}

/**
 * Whether every foot leaves the ground at once some time in the cycle, at the fast end of the
 * gait's duty: a gap of more than 2% of the cycle with no foot planted.
 */
function hasFlight(phases: readonly number[], duty: number): boolean {
  if (phases.length === 0) return false;
  let gap = 0;
  // Twice round, so a gap across the end of the cycle counts whole.
  for (let i = 0; i < 800; i++) {
    const phase = (i % 400) / 400;
    const planted = phases.some((o) => (((phase - o) % 1) + 1) % 1 < duty);
    gap = planted ? 0 : gap + 1 / 400;
    if (gap > 0.02) return true;
  }
  return false;
}

/**
 * Resolves a creature's gaits (module timing plus its own params) for the motion controller, as
 * plain numbers that can leave a worker. With `hipHeight` and `posture` (once the skeleton is
 * built), gaits whose hip range or postures the creature is outside are left out: a horse does
 * not bound, and a tortoise does not gallop.
 */
export function motionData(
  spec: CreatureSpec,
  registry: Registry,
  options: { readonly hipHeight?: number; readonly posture?: string } = {},
): MotionData {
  const pairs = spec.limbs.filter((l) => l.role === 'leg' && l.mirror === 1).length;
  // Long fins are flippers, and a body that has them and little tail beats them rather than
  // waving; a fish's short fins only steer (docs/design/10.3-swimming.md).
  const flippers = spec.limbs.some((l) => l.role === 'fin' && l.length >= 0.4);
  const tail = spec.body.tail.length >= 0.5;
  const gaits: GaitInfo[] = [];
  for (const ref of spec.motion.gaits) {
    const module = registry.get('gait', ref.type) as GaitModule | undefined;
    // Stubs (`planned`) validate but have nothing to run yet.
    if (!module || module.planned) continue;
    const hip = options.hipHeight;
    if (hip !== undefined && module.hip && (hip < module.hip[0] || hip > module.hip[1])) continue;
    const posture = options.posture as 'upright' | 'sprawl' | undefined;
    if (posture && module.postures && !module.postures.includes(posture)) continue;
    if (module.swim === 'fins' && !flippers) continue;
    if (module.swim === 'body' && flippers && !tail) continue;
    const p = ref.params as Record<string, unknown>;
    const num = (key: string, fallback: number) =>
      typeof p[key] === 'number' ? (p[key] as number) : fallback;
    const spine = module.legPairs !== 'any' && module.legPairs.includes(0);
    const base = typeof module.duty === 'function' ? module.duty(Math.max(1, pairs)) : module.duty;
    const [duty, dutyFast] = profile(p.duty, typeof base === 'number' ? [base, base] : base);
    const [stride, strideFast] = profile(p.stride, [1, 1]);
    const wave = module.wave(Math.max(1, pairs));
    const legs = spine ? [] : phasesOf(module, pairs, wave, p);
    const flex = num('flex', module.flex ?? 0);
    const lean = num('lean', 0);
    // Only land gaits can have a suspension phase; an air gait has no planted feet at all.
    const flight =
      (module.medium ?? 'land') === 'land' && hasFlight(legs, Math.min(duty, dutyFast));
    gaits.push({
      id: module.id,
      wave,
      duty,
      froude: module.froude,
      stepHeight: num('stepHeight', 0.15),
      stride,
      spine,
      amplitude: num('amplitude', 0.18),
      waves: num('waves', 1.5),
      // Only what plan 1's gaits never had is written, so their data stays as it was.
      ...(module.offsets ? { phases: legs } : {}),
      ...(dutyFast !== duty ? { dutyFast } : {}),
      ...(strideFast !== stride ? { strideFast } : {}),
      ...(module.natural !== undefined ? { natural: module.natural } : {}),
      ...(flex > 0 ? { flex } : {}),
      ...(lean > 0 ? { lean } : {}),
      ...(flight ? { flight } : {}),
      ...(module.medium && module.medium !== 'land' ? { medium: module.medium } : {}),
      ...(module.swim ? { swim: module.swim } : {}),
      ...(module.air
        ? {
            air: module.air,
            ...(typeof p.stroke === 'number' ? { stroke: p.stroke } : {}),
            ...(typeof p.bank === 'number' ? { bank: p.bank } : {}),
            ...(typeof p.sink === 'number' ? { sink: p.sink } : {}),
            ...(typeof p.rate === 'number' ? { rate: p.rate } : {}),
          }
        : {}),
    });
  }
  // Slowest first, so the controller starts walking.
  gaits.sort((a, b) => a.froude[0] - b.froude[0]);
  const actions = spec.motion.actions.map((ref) => ({ id: ref.type, params: ref.params }));
  return { temperament: spec.motion.temperament, gaits, actions };
}
