/** Samples a profile (values spread evenly from t=0 to t=1) with smooth interpolation. */
export function sampleProfile(values: readonly number[], t: number): number {
  const n = values.length;
  if (n === 0) return 0;
  if (n === 1) return values[0] as number;
  const x = Math.min(1, Math.max(0, t)) * (n - 1);
  const i = Math.min(n - 2, Math.floor(x));
  const f = x - i;
  // Catmull-Rom through the neighbours, clamped so it never overshoots the two values it joins.
  const p0 = values[Math.max(0, i - 1)] as number;
  const p1 = values[i] as number;
  const p2 = values[i + 1] as number;
  const p3 = values[Math.min(n - 1, i + 2)] as number;
  const f2 = f * f;
  const f3 = f2 * f;
  const v =
    0.5 *
    (2 * p1 +
      (-p0 + p2) * f +
      (2 * p0 - 5 * p1 + 4 * p2 - p3) * f2 +
      (-p0 + 3 * p1 - 3 * p2 + p3) * f3);
  return Math.min(Math.max(v, Math.min(p1, p2)), Math.max(p1, p2));
}
