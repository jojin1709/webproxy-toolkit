export type Verdict = "pass" | "warn" | "fail";

export type StatRow = { label: string; value: string };

export type TestResult = {
  name: string;
  detail: string;
  verdict: Verdict;
  verdictText: string;
};

export type PositionStat = {
  index: number;
  ratio: number;
  distinct: number;
  common: { ch: string; n: number };
  entropy: number;
};

export type Analysis = {
  samples: number;
  stats: StatRow[];
  tests: TestResult[];
  positions: PositionStat[];
};

const POSITION_CAP = 256;
const REPEAT_CAP = 64;

export function shannonEntropy(s: string): number {
  if (s.length === 0) return 0;
  const freq = new Map<string, number>();
  for (const ch of s) freq.set(ch, (freq.get(ch) ?? 0) + 1);
  let h = 0;
  for (const n of freq.values()) {
    const p = n / s.length;
    h -= p * Math.log2(p);
  }
  return h;
}

function normalCdf(z: number): number {
  const t = 1 / (1 + 0.2316419 * Math.abs(z));
  const d = 0.3989422804014327 * Math.exp((-z * z) / 2);
  const p =
    d *
    t *
    (0.31938153 +
      t * (-0.356563782 + t * (1.781477937 + t * (-1.821255978 + t * 1.330274429))));
  return z > 0 ? 1 - p : p;
}

function chiSquareUpperP(chi2: number, df: number): number {
  if (df <= 0) return 1;
  const c = 2 / (9 * df);
  const z = (Math.cbrt(chi2 / df) - (1 - c)) / Math.sqrt(c);
  return 1 - normalCdf(z);
}

export function uniformity(tokens: string[]): { chi2: number; df: number; p: number } {
  const all = tokens.join("");
  if (all.length === 0) return { chi2: 0, df: 0, p: 1 };
  const freq = new Map<string, number>();
  for (const ch of all) freq.set(ch, (freq.get(ch) ?? 0) + 1);
  const k = freq.size;
  if (k < 2) return { chi2: 0, df: 0, p: 1 };
  const expected = all.length / k;
  let chi2 = 0;
  for (const n of freq.values()) chi2 += ((n - expected) * (n - expected)) / expected;
  return { chi2, df: k - 1, p: chiSquareUpperP(chi2, k - 1) };
}

function allBytes(tokens: string[]): number[] {
  const bytes: number[] = [];
  for (const t of tokens) {
    for (let i = 0; i < t.length; i++) bytes.push(t.charCodeAt(i) & 0xff);
  }
  return bytes;
}

export function monteCarloPi(tokens: string[]): number | null {
  const bytes = allBytes(tokens);
  const uniq = Array.from(new Set(bytes)).sort((a, b) => a - b);
  const k = uniq.length;
  if (k < 2) return null;
  const rank = new Map<number, number>();
  uniq.forEach((v, i) => rank.set(v, (i + 0.5) / k));
  let inside = 0;
  let pairs = 0;
  for (let i = 0; i + 1 < bytes.length; i += 2) {
    pairs++;
    const u = rank.get(bytes[i]) ?? 0;
    const v = rank.get(bytes[i + 1]) ?? 0;
    const dx = u - 0.5;
    const dy = v - 0.5;
    if (dx * dx + dy * dy <= 0.25) inside++;
  }
  if (pairs < 10) return null;
  return (4 * inside) / pairs;
}

export function byteMean(tokens: string[]): number | null {
  const bytes = allBytes(tokens);
  if (bytes.length === 0) return null;
  let sum = 0;
  for (const b of bytes) sum += b;
  return sum / bytes.length;
}

export function longestRepeat(s: string): string {
  const max = Math.min(REPEAT_CAP, Math.floor(s.length / 2));
  if (max < 1) return "";
  const findDup = (len: number): string | null => {
    const seen = new Set<string>();
    for (let i = 0; i + len <= s.length; i++) {
      const sub = s.slice(i, i + len);
      if (sub.includes("\n")) continue;
      if (seen.has(sub)) return sub;
      seen.add(sub);
    }
    return null;
  };
  let lo = 1;
  let hi = max;
  let best = "";
  while (lo <= hi) {
    const mid = (lo + hi) >> 1;
    const found = findDup(mid);
    if (found) {
      best = found;
      lo = mid + 1;
    } else {
      hi = mid - 1;
    }
  }
  return best;
}

export function positionStats(tokens: string[]): PositionStat[] {
  let maxLen = 0;
  for (const t of tokens) if (t.length > maxLen) maxLen = t.length;
  maxLen = Math.min(maxLen, POSITION_CAP);
  const out: PositionStat[] = [];
  for (let i = 0; i < maxLen; i++) {
    const freq = new Map<string, number>();
    let n = 0;
    for (const t of tokens) {
      if (i < t.length) {
        freq.set(t[i], (freq.get(t[i]) ?? 0) + 1);
        n++;
      }
    }
    if (n === 0) break;
    let commonCh = "";
    let commonN = 0;
    let h = 0;
    for (const [ch, c] of freq) {
      if (c > commonN) {
        commonN = c;
        commonCh = ch;
      }
      const p = c / n;
      h -= p * Math.log2(p);
    }
    out.push({
      index: i,
      ratio: freq.size / n,
      distinct: freq.size,
      common: { ch: commonCh, n: commonN },
      entropy: h,
    });
  }
  return out;
}

function fmt(n: number, digits: number): string {
  return n.toFixed(digits);
}

export function analyzeTokens(tokens: string[]): Analysis {
  const all = tokens.join("");
  const lengths = tokens.map((t) => t.length).sort((a, b) => a - b);
  const mid = lengths.length >> 1;
  const med =
    lengths.length === 0 ? 0 : lengths.length % 2 ? lengths[mid] : (lengths[mid - 1] + lengths[mid]) / 2;
  const alphabet = new Set(all).size;
  const entropy = shannonEntropy(all);
  const maxEntropy = alphabet > 0 ? Math.log2(alphabet) : 0;
  const distinct = new Set(tokens).size;
  const u = uniformity(tokens);
  const pi = monteCarloPi(tokens);
  const mean = byteMean(tokens);
  const repeat = longestRepeat(tokens.join("\n"));
  const positions = positionStats(tokens);

  const pVerdict: Verdict = u.p >= 0.05 ? "pass" : u.p >= 0.01 ? "warn" : "fail";
  const posMean = positions.length
    ? positions.reduce((s, p) => s + p.ratio, 0) / positions.length
    : 0;
  const posVerdict: Verdict = posMean >= 0.9 ? "pass" : posMean >= 0.75 ? "warn" : "fail";
  const distVerdict: Verdict =
    tokens.length < 2 || distinct === tokens.length
      ? "pass"
      : distinct >= tokens.length * 0.95
        ? "warn"
        : "fail";

  const stats: StatRow[] = [
    { label: "Samples", value: String(tokens.length) },
    { label: "Distinct tokens", value: `${distinct} / ${tokens.length}` },
    {
      label: "Token length (min/mean/median/max)",
      value: `${lengths[0] ?? 0} / ${fmt(lengths.reduce((s, l) => s + l, 0) / Math.max(lengths.length, 1), 1)} / ${fmt(med, 1)} / ${lengths[lengths.length - 1] ?? 0}`,
    },
    { label: "Shannon entropy (bits/char)", value: `${fmt(entropy, 3)} of max ${fmt(maxEntropy, 3)}` },
    { label: "Character alphabet", value: String(alphabet) },
    { label: "Arithmetic mean of bytes", value: mean === null ? "—" : `${fmt(mean, 2)} (random ≈ 127.5)` },
    {
      label: "Monte Carlo π estimate",
      value: pi === null ? "—" : `${fmt(pi, 4)} (error ${fmt(Math.abs(Math.PI - pi), 4)})`,
    },
    { label: "Longest repeated sequence", value: repeat ? `${repeat.length} chars: ${JSON.stringify(repeat)}` : "none found" },
  ];

  const tests: TestResult[] = [
    {
      name: "Chi-square uniformity",
      detail: `χ² = ${fmt(u.chi2, 2)} on ${u.df} df, p = ${fmt(u.p, 3)}`,
      verdict: pVerdict,
      verdictText: pVerdict === "pass" ? "passed" : pVerdict === "warn" ? "close to random" : "not random",
    },
    {
      name: "Per-position uniqueness",
      detail: `${positions.length} positions sampled, mean uniqueness ${fmt(posMean * 100, 1)}%`,
      verdict: posVerdict,
      verdictText:
        posVerdict === "pass"
          ? "highly unique"
          : posVerdict === "warn"
            ? "some repeats"
            : "highly repetitive",
    },
    {
      name: "Sample distinctness",
      detail: `${distinct} of ${tokens.length} tokens are unique`,
      verdict: distVerdict,
      verdictText:
        distVerdict === "pass"
          ? "all samples unique"
          : distVerdict === "warn"
            ? "few duplicates"
            : "many duplicates",
    },
  ];

  return { samples: tokens.length, stats, tests, positions };
}
