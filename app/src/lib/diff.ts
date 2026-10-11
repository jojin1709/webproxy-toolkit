export type DiffRow = {
  kind: "same" | "changed";
  left: string | null;
  right: string | null;
  leftRanges: [number, number][];
  rightRanges: [number, number][];
};

type Op = { kind: "same" | "del" | "add"; text: string };

const LCS_CELL_LIMIT = 4_000_000;

function splitLines(text: string): string[] {
  return text.length === 0 ? [""] : text.split("\n");
}

function positionalOps(a: string[], b: string[]): Op[] {
  const ops: Op[] = [];
  const max = Math.max(a.length, b.length);
  for (let k = 0; k < max; k++) {
    const inA = k < a.length;
    const inB = k < b.length;
    if (inA && inB && a[k] === b[k]) {
      ops.push({ kind: "same", text: a[k] });
    } else {
      if (inA) ops.push({ kind: "del", text: a[k] });
      if (inB) ops.push({ kind: "add", text: b[k] });
    }
  }
  return ops;
}

function lcsOps(a: string[], b: string[]): Op[] {
  const n = a.length;
  const m = b.length;
  if (n === 0) return b.map((t) => ({ kind: "add", text: t }));
  if (m === 0) return a.map((t) => ({ kind: "del", text: t }));
  const width = m + 1;
  if ((n + 1) * width > LCS_CELL_LIMIT) return positionalOps(a, b);
  const dp = new Int32Array((n + 1) * width);
  for (let i = n - 1; i >= 0; i--) {
    for (let j = m - 1; j >= 0; j--) {
      dp[i * width + j] =
        a[i] === b[j]
          ? dp[(i + 1) * width + j + 1] + 1
          : Math.max(dp[(i + 1) * width + j], dp[i * width + j + 1]);
    }
  }
  const ops: Op[] = [];
  let i = 0;
  let j = 0;
  while (i < n && j < m) {
    if (a[i] === b[j]) {
      ops.push({ kind: "same", text: a[i] });
      i++;
      j++;
    } else if (dp[(i + 1) * width + j] >= dp[i * width + j + 1]) {
      ops.push({ kind: "del", text: a[i] });
      i++;
    } else {
      ops.push({ kind: "add", text: b[j] });
      j++;
    }
  }
  while (i < n) ops.push({ kind: "del", text: a[i++] });
  while (j < m) ops.push({ kind: "add", text: b[j++] });
  return ops;
}

function groupRows(ops: Op[]): DiffRow[] {
  const rows: DiffRow[] = [];
  let i = 0;
  while (i < ops.length) {
    if (ops[i].kind === "same") {
      rows.push({
        kind: "same",
        left: ops[i].text,
        right: ops[i].text,
        leftRanges: [],
        rightRanges: [],
      });
      i++;
      continue;
    }
    const dels: string[] = [];
    const adds: string[] = [];
    while (i < ops.length && ops[i].kind === "del") dels.push(ops[i++].text);
    while (i < ops.length && ops[i].kind === "add") adds.push(ops[i++].text);
    const pairs = Math.max(dels.length, adds.length);
    for (let k = 0; k < pairs; k++) {
      rows.push({
        kind: "changed",
        left: k < dels.length ? dels[k] : null,
        right: k < adds.length ? adds[k] : null,
        leftRanges: [],
        rightRanges: [],
      });
    }
  }
  return rows;
}

function tokens(s: string): { text: string; start: number; end: number }[] {
  const out: { text: string; start: number; end: number }[] = [];
  const re = /\S+/g;
  let m: RegExpExecArray | null;
  while ((m = re.exec(s)) !== null) {
    out.push({ text: m[0], start: m.index, end: m.index + m[0].length });
  }
  return out;
}

function wordDiff(
  left: string,
  right: string
): { leftRanges: [number, number][]; rightRanges: [number, number][] } {
  const lt = tokens(left);
  const rt = tokens(right);
  const ops = lcsOps(
    lt.map((t) => t.text),
    rt.map((t) => t.text)
  );
  const leftRanges: [number, number][] = [];
  const rightRanges: [number, number][] = [];
  let li = 0;
  let ri = 0;
  for (const op of ops) {
    if (op.kind === "same") {
      li++;
      ri++;
    } else if (op.kind === "del") {
      const t = lt[li++];
      if (t) leftRanges.push([t.start, t.end]);
    } else {
      const t = rt[ri++];
      if (t) rightRanges.push([t.start, t.end]);
    }
  }
  return { leftRanges, rightRanges };
}

export function diffTexts(left: string, right: string, mode: "line" | "word"): DiffRow[] {
  const rows = groupRows(lcsOps(splitLines(left), splitLines(right)));
  if (mode === "line") return rows;
  return rows.map((r) => {
    if (r.kind !== "changed" || r.left === null || r.right === null) return r;
    const { leftRanges, rightRanges } = wordDiff(r.left, r.right);
    return { ...r, leftRanges, rightRanges };
  });
}
