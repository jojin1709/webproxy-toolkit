import { ReactNode, useEffect, useMemo, useRef, useState } from "react";
import { DiffRow, diffTexts } from "../lib/diff";
import { Transfer } from "../types";

type Props = {
  incoming: Transfer[];
  setToast: (t: string) => void;
};

function shortLabel(url: string): string {
  try {
    const u = new URL(url);
    const path = u.pathname === "/" ? "" : u.pathname;
    return `${u.host}${path}`;
  } catch {
    return "message";
  }
}

function renderLine(text: string, ranges: [number, number][]): ReactNode {
  if (ranges.length === 0) return text;
  const parts: ReactNode[] = [];
  let pos = 0;
  ranges.forEach(([a, b], i) => {
    if (a > pos) parts.push(<span key={`t${i}`}>{text.slice(pos, a)}</span>);
    parts.push(
      <mark key={`m${i}`}>{text.slice(a, b)}</mark>
    );
    pos = b;
  });
  if (pos < text.length) parts.push(<span key="tail">{text.slice(pos)}</span>);
  return parts;
}

function kindInfo(row: DiffRow): { label: string; chip: string } {
  if (row.left !== null && row.right === null) return { label: "Only left", chip: "chip err" };
  if (row.left === null && row.right !== null) return { label: "Only right", chip: "chip ok" };
  return { label: "Changed", chip: "chip warn" };
}

function snippet(text: string | null): ReactNode {
  if (text === null || text === "") return <span className="muted">—</span>;
  return (
    <span className="mono" title={text}>
      {text}
    </span>
  );
}

export default function ComparerPanel({ incoming, setToast }: Props) {
  const [left, setLeft] = useState("");
  const [right, setRight] = useState("");
  const [leftLabel, setLeftLabel] = useState("");
  const [rightLabel, setRightLabel] = useState("");
  const [mode, setMode] = useState<"line" | "word">("line");
  const [focus, setFocus] = useState<number | null>(null);
  const leftBody = useRef<HTMLDivElement>(null);
  const rightBody = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (incoming.length >= 2) {
      setLeft(incoming[0].raw);
      setLeftLabel(shortLabel(incoming[0].url));
      setRight(incoming[1].raw);
      setRightLabel(shortLabel(incoming[1].url));
      setFocus(null);
      setToast("Two messages loaded into Comparer");
    } else if (incoming.length === 1) {
      setLeft(incoming[0].raw);
      setLeftLabel(shortLabel(incoming[0].url));
      setFocus(null);
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [incoming]);

  const rows = useMemo(() => diffTexts(left, right, mode), [left, right, mode]);
  const changedIdx = useMemo(() => {
    const out: number[] = [];
    rows.forEach((r, i) => {
      if (r.kind === "changed") out.push(i);
    });
    return out;
  }, [rows]);
  const changed = changedIdx.length;
  const focusPos = focus === null ? -1 : changedIdx.indexOf(focus);
  const summary =
    left || right
      ? changed === 0
        ? "No differences"
        : `${changed} of ${rows.length} lines differ`
      : "Nothing to compare";

  useEffect(() => {
    if (focus === null) return;
    leftBody.current?.querySelector(".cmp-focus")?.scrollIntoView({ block: "center" });
    rightBody.current?.querySelector(".cmp-focus")?.scrollIntoView({ block: "center" });
  }, [focus]);

  function stepDiff(dir: 1 | -1) {
    if (changedIdx.length === 0) return;
    const pos = changedIdx.indexOf(focus ?? -1);
    const next =
      pos === -1
        ? dir === 1
          ? 0
          : changedIdx.length - 1
        : (pos + dir + changedIdx.length) % changedIdx.length;
    setFocus(changedIdx[next]);
  }

  function swap() {
    setLeft(right);
    setRight(left);
    setLeftLabel(rightLabel);
    setRightLabel(leftLabel);
    setFocus(null);
  }

  function clear() {
    setLeft("");
    setRight("");
    setLeftLabel("");
    setRightLabel("");
    setFocus(null);
  }

  function sideClass(row: (typeof rows)[number], side: "left" | "right"): string {
    if (row.kind === "same") return "";
    if (side === "left") return row.right === null ? "cmp-ln-del" : "cmp-ln-chg";
    return row.left === null ? "cmp-ln-add" : "cmp-ln-chg";
  }

  if (!left && !right) {
    return (
      <div className="panel" role="tabpanel" aria-label="Comparer" id="panel-comparer">
        <div className="blank" tabIndex={0}>
          <div className="blank-inner">
            <h2>Nothing to compare yet</h2>
            <p>
              Send two requests or responses from the Proxy history (right-click → Send to
              Comparer). The first message loads on the left, the second on the right. Line and
              word diffs are computed locally.
            </p>
          </div>
        </div>
      </div>
    );
  }

  return (
    <div className="panel" role="tabpanel" aria-label="Comparer" id="panel-comparer">
      <div className="toolbar">
        <div className="segmented" role="group" aria-label="Diff mode">
          <button
            className={mode === "line" ? "active" : ""}
            aria-pressed={mode === "line"}
            onClick={() => setMode("line")}
          >
            Line diff
          </button>
          <button
            className={mode === "word" ? "active" : ""}
            aria-pressed={mode === "word"}
            onClick={() => setMode("word")}
          >
            Word diff
          </button>
        </div>
        <button className="btn" onClick={swap} title="Swap left and right">
          Swap
        </button>
        <button className="btn" onClick={clear}>
          Clear
        </button>
        <button
          className="btn"
          onClick={() => stepDiff(-1)}
          disabled={changed === 0}
          aria-label="Previous difference"
        >
          ◀ Prev
        </button>
        <button
          className="btn"
          onClick={() => stepDiff(1)}
          disabled={changed === 0}
          aria-label="Next difference"
        >
          Next ▶
        </button>
        <span className="mono muted" aria-live="polite">
          {focusPos >= 0 ? `${focusPos + 1} / ${changed}` : "—"}
        </span>
        <span className="grow" />
        <span className={`chip ${changed === 0 ? "ok" : "warn"}`}>{summary}</span>
      </div>

      <div className="split side">
        <div className="pane">
          <div className="pane-title">
            Left {leftLabel && <span className="muted ellipsis">{leftLabel}</span>}
          </div>
          <div className="pane-body" ref={leftBody}>
            <pre className="msgview-pre cmp-diffs">
              {rows.map((r, i) => (
                <div
                  key={i}
                  className={`ln ${sideClass(r, "left")} ${focus === i ? "cmp-focus" : ""}`}
                  onClick={() => setFocus(i)}
                >
                  {r.left === null ? "" : renderLine(r.left, r.leftRanges)}
                </div>
              ))}
            </pre>
          </div>
        </div>
        <div className="pane">
          <div className="pane-title">
            Right {rightLabel && <span className="muted ellipsis">{rightLabel}</span>}
          </div>
          <div className="pane-body" ref={rightBody}>
            <pre className="msgview-pre cmp-diffs">
              {rows.map((r, i) => (
                <div
                  key={i}
                  className={`ln ${sideClass(r, "right")} ${focus === i ? "cmp-focus" : ""}`}
                  onClick={() => setFocus(i)}
                >
                  {r.right === null ? "" : renderLine(r.right, r.rightRanges)}
                </div>
              ))}
            </pre>
          </div>
        </div>
      </div>

      <div className="pane cmp-diffs-pane">
        <div className="pane-title">
          Differences <span className="muted">{changed} rows</span>
        </div>
        <div className="table-wrap">
          <table className="history">
            <thead>
              <tr>
                <th scope="col">#</th>
                <th scope="col">Kind</th>
                <th scope="col">Left</th>
                <th scope="col">Right</th>
              </tr>
            </thead>
            <tbody>
              {changedIdx.length === 0 ? (
                <tr>
                  <td colSpan={4} className="muted">
                    No differences
                  </td>
                </tr>
              ) : (
                changedIdx.map((rowIdx, i) => {
                  const row = rows[rowIdx];
                  const kind = kindInfo(row);
                  const wordNote =
                    mode === "word" && (row.leftRanges.length > 0 || row.rightRanges.length > 0)
                      ? `${row.leftRanges.length}w / ${row.rightRanges.length}w`
                      : "";
                  return (
                    <tr
                      key={rowIdx}
                      tabIndex={0}
                      className={focus === rowIdx ? "selected" : ""}
                      onClick={() => setFocus(rowIdx)}
                      onKeyDown={(e) => {
                        if (e.key === "Enter" || e.key === " ") {
                          e.preventDefault();
                          setFocus(rowIdx);
                        }
                      }}
                    >
                      <td>{i + 1}</td>
                      <td>
                        <span className={kind.chip}>{kind.label}</span>
                        {wordNote && <span className="muted"> {wordNote}</span>}
                      </td>
                      <td>{snippet(row.left)}</td>
                      <td>{snippet(row.right)}</td>
                    </tr>
                  );
                })
              )}
            </tbody>
          </table>
        </div>
      </div>
    </div>
  );
}
