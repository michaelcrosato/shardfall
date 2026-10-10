/**
 * Names for several heads (and necks, jaws, mouths) or tails. The main one, the middle (the one
 * just left of the middle for an even count), keeps the plain name at every count, so its bones,
 * random streams and sockets never move; the others are named outward from it by side and rank:
 * `head.L1`, `head.R1`, `head.L2`, … Ranks start with a letter so they never read as a bone index
 * (`neck.2` is the neck's third bone).
 */
export function instanceSuffixes(count: number): string[] {
  const n = Math.max(1, Math.floor(count));
  const main = Math.floor((n - 1) / 2);
  // From the creature's left (+X) to its right.
  return Array.from({ length: n }, (_, i) =>
    i === main ? '' : i < main ? `.L${main - i}` : `.R${i - main}`,
  );
}

/** Instance names of a section with `count` copies: `["head.L1", "head", "head.R1"]`. */
export const instanceNames = (section: string, count: number): string[] =>
  instanceSuffixes(count).map((suffix) => `${section}${suffix}`);

/**
 * How close neighbouring heads come at rest, as the necks fan out over `spread` degrees from
 * roots spaced across the front of the torso, in torso lengths, against the room two heads
 * need. Ignores pitch; good enough to warn before 9.1 builds the fan.
 */
export function headSpacing(body: {
  readonly neck: { readonly count: number; readonly length: number; readonly spread: number };
  readonly head: { readonly length: number; readonly radius: number };
  /** Torso radius at the front, in torso lengths. */
  readonly chest: number;
}): { readonly gap: number; readonly needed: number } {
  const n = body.neck.count;
  if (n <= 1) return { gap: Number.POSITIVE_INFINITY, needed: 0 };
  const step = ((body.neck.spread / (n - 1)) * Math.PI) / 180;
  const rootStep = (1.2 * body.chest) / (n - 1);
  const reach = body.neck.length + body.head.length / 2;
  // Two neighbours, symmetric about the creature's axis.
  const x = (i: number) =>
    (i - (n - 1) / 2) * rootStep + reach * Math.sin((i - (n - 1) / 2) * step);
  const z = (i: number) => reach * Math.cos((i - (n - 1) / 2) * step);
  const a = Math.floor((n - 1) / 2);
  const gap = Math.hypot(x(a + 1) - x(a), z(a + 1) - z(a));
  return { gap, needed: 1.8 * body.head.radius };
}
