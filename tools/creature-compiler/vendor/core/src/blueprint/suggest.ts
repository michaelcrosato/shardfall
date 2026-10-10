/** Damerau–Levenshtein (optimal string alignment) distance. */
export function editDistance(a: string, b: string): number {
  const n = a.length;
  const m = b.length;
  if (n === 0) return m;
  if (m === 0) return n;
  let prev2 = new Array<number>(m + 1).fill(0);
  let prev = Array.from({ length: m + 1 }, (_, j) => j);
  let cur = new Array<number>(m + 1).fill(0);
  for (let i = 1; i <= n; i++) {
    cur[0] = i;
    for (let j = 1; j <= m; j++) {
      const cost = a[i - 1] === b[j - 1] ? 0 : 1;
      let v = Math.min((prev[j] ?? 0) + 1, (cur[j - 1] ?? 0) + 1, (prev[j - 1] ?? 0) + cost);
      if (i > 1 && j > 1 && a[i - 1] === b[j - 2] && a[i - 2] === b[j - 1]) {
        v = Math.min(v, (prev2[j - 2] ?? 0) + 1);
      }
      cur[j] = v;
    }
    [prev2, prev, cur] = [prev, cur, prev2];
  }
  return prev[m] ?? 0;
}

/** The closest candidate to `input`, if it is close enough to be a likely typo. */
export function didYouMean(input: string, candidates: Iterable<string>): string | undefined {
  const lower = input.toLowerCase();
  let best: string | undefined;
  let bestScore = Number.POSITIVE_INFINITY;
  for (const c of candidates) {
    const cl = c.toLowerCase();
    let score = editDistance(lower, cl);
    // Ids like "horn.curved" also match on a segment ("hron" → "horn"), and a dotted guess on
    // its own segments ("wing.feathered" → "membrane.feather").
    if (cl.includes('.')) {
      for (const segment of cl.split('.')) {
        score = Math.min(score, editDistance(lower, segment) + 0.5);
        if (lower.includes('.'))
          for (const own of lower.split('.'))
            if (own.length >= 4) score = Math.min(score, editDistance(own, segment) + 1);
      }
    }
    // An abbreviation ("len" -> "length") is a stronger hint than a one-letter typo ("lean").
    if (lower.length >= 3 && cl.startsWith(lower)) score = Math.min(score, 0.5);
    // Prefix or containment matches ("leg" -> "foreleg") are good hints too.
    if (score > 2 && (cl.startsWith(lower) || lower.startsWith(cl) || cl.includes(lower))) {
      score = 2;
    }
    if (score < bestScore) {
      bestScore = score;
      best = c;
    }
  }
  const limit = Math.max(2, Math.floor(input.length / 3));
  return best !== undefined && bestScore <= limit ? best : undefined;
}
