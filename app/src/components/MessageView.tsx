import { useMemo, useRef, useState } from "react";
import { Search } from "lucide-react";

export type ViewTab = "pretty" | "raw" | "hex";

type Props = {
  value: string;
  onChange?: (v: string) => void;
  title?: string;
  defaultTab?: ViewTab;
  extra?: React.ReactNode;
};

type Line = {
  kind: "line1" | "header" | "sep" | "body";
  text: string;
  name?: string;
  rest?: string;
};

function parseHttp(text: string): Line[] {
  const lines = text.split(/\r?\n/);
  const out: Line[] = [];
  let phase: "first" | "headers" | "body" = "first";
  for (const t of lines) {
    if (phase === "first") {
      out.push({ kind: "line1", text: t });
      phase = "headers";
      continue;
    }
    if (phase === "headers") {
      if (t === "") {
        out.push({ kind: "sep", text: t });
        phase = "body";
        continue;
      }
      const idx = t.indexOf(":");
      if (idx > 0 && idx < 80) {
        out.push({ kind: "header", text: t, name: t.slice(0, idx), rest: t.slice(idx) });
      } else {
        out.push({ kind: "body", text: t });
        phase = "body";
      }
      continue;
    }
    out.push({ kind: "body", text: t });
  }
  return out;
}

function tryPrettyJson(body: string): string | null {
  const t = body.trim();
  if (!t.startsWith("{") && !t.startsWith("[")) return null;
  try {
    return JSON.stringify(JSON.parse(t), null, 2);
  } catch {
    return null;
  }
}

function buildPretty(text: string): Line[] {
  const lines = parseHttp(text);
  const sepIdx = lines.findIndex((l) => l.kind === "sep");
  if (sepIdx < 0) return lines;
  const head = lines.slice(0, sepIdx + 1);
  const bodyLines = lines.slice(sepIdx + 1);
  const body = bodyLines.map((l) => l.text).join("\n");
  const pretty = tryPrettyJson(body);
  if (pretty == null) return lines;
  return [...head, ...pretty.split("\n").map((t) => ({ kind: "body" as const, text: t }))];
}

function hexDump(text: string): string[] {
  const bytes = new TextEncoder().encode(text);
  const rows: string[] = [];
  for (let off = 0; off < bytes.length; off += 16) {
    const slice = bytes.subarray(off, off + 16);
    const hex = Array.from(slice)
      .map((b) => b.toString(16).padStart(2, "0"))
      .join(" ")
      .padEnd(47, " ");
    let ascii = "";
    for (const b of slice) ascii += b >= 32 && b < 127 ? String.fromCharCode(b) : ".";
    rows.push(`${off.toString(16).padStart(8, "0")}  ${hex}  |${ascii}|`);
  }
  if (rows.length === 0) rows.push("(empty)");
  return rows;
}

function countMatches(hay: string, needle: string): number {
  if (!needle) return 0;
  const h = hay.toLowerCase();
  const n = needle.toLowerCase();
  let count = 0;
  let i = 0;
  for (;;) {
    const idx = h.indexOf(n, i);
    if (idx === -1) return count;
    count++;
    i = idx + n.length;
  }
}

function mark(text: string, q: string): React.ReactNode {
  if (!q) return text;
  const lower = text.toLowerCase();
  const ql = q.toLowerCase();
  const parts: React.ReactNode[] = [];
  let i = 0;
  for (;;) {
    const idx = lower.indexOf(ql, i);
    if (idx === -1) break;
    if (idx > i) parts.push(text.slice(i, idx));
    parts.push(<mark key={idx}>{text.slice(idx, idx + q.length)}</mark>);
    i = idx + q.length;
  }
  parts.push(text.slice(i));
  return parts;
}

export default function MessageView({ value, onChange, title, defaultTab = "pretty", extra }: Props) {
  const [tab, setTab] = useState<ViewTab>(defaultTab);
  const [query, setQuery] = useState("");
  const taRef = useRef<HTMLTextAreaElement>(null);
  const gutRef = useRef<HTMLDivElement>(null);

  const prettyLines = useMemo(() => buildPretty(value), [value]);
  const hexLines = useMemo(() => (tab === "hex" ? hexDump(value) : []), [value, tab]);
  const bytes = useMemo(() => new TextEncoder().encode(value).length, [value]);
  const matches = useMemo(() => countMatches(value, query), [value, query]);
  const lineCount = useMemo(() => value.split("\n").length, [value]);

  return (
    <div className="msgview">
      <div className="msgview-tabs">
        {title && <span className="msgview-title">{title}</span>}
        <div className="msgview-tablist" role="tablist">
          {(["pretty", "raw", "hex"] as ViewTab[]).map((t) => (
            <button
              key={t}
              role="tab"
              aria-selected={tab === t}
              className={tab === t ? "active" : ""}
              onClick={() => setTab(t)}
            >
              {t === "pretty" ? "Pretty" : t === "raw" ? "Raw" : "Hex"}
            </button>
          ))}
        </div>
        <span className="grow" />
        {extra}
        <span className="msgview-meta">
          {value.length} chars · {bytes} B
        </span>
      </div>

      <div className={`msgview-body ${tab === "raw" && onChange ? "no-scroll" : ""}`} style={tab === "raw" && onChange ? { overflow: "hidden" } : undefined}>
        {tab === "pretty" && (
          <pre className="msgview-pre">
            {prettyLines.map((l, i) =>
              l.kind === "line1" ? (
                <div key={i} className="ln hl-line1">
                  {mark(l.text, query)}
                </div>
              ) : l.kind === "header" ? (
                <div key={i} className="ln">
                  <span className="hl-hname">{mark(l.name ?? "", query)}</span>
                  <span>{mark(l.rest ?? "", query)}</span>
                </div>
              ) : (
                <div key={i} className={`ln ${l.kind === "body" ? "hl-body" : "hl-sep"}`}>
                  {l.text ? mark(l.text, query) : "\u00a0"}
                </div>
              )
            )}
          </pre>
        )}

        {tab === "raw" &&
          (onChange ? (
            <div className="editor">
              <div className="editor-gutter" ref={gutRef} aria-hidden>
                {Array.from({ length: lineCount }, (_, i) => (
                  <div key={i}>{i + 1}</div>
                ))}
              </div>
              <textarea
                ref={taRef}
                className="editor-text"
                value={value}
                onChange={(e) => onChange(e.target.value)}
                onScroll={() => {
                  if (gutRef.current && taRef.current)
                    gutRef.current.scrollTop = taRef.current.scrollTop;
                }}
                spellCheck={false}
                aria-label={title ? `${title} raw` : "raw message"}
              />
            </div>
          ) : (
            <pre className="msgview-pre">
              {value.split(/\r?\n/).map((t, i) => (
                <div key={i} className="ln">
                  {t ? mark(t, query) : "\u00a0"}
                </div>
              ))}
            </pre>
          ))}

        {tab === "hex" && (
          <pre className="msgview-pre msgview-hex">
            {hexLines.map((l, i) => (
              <div key={i} className="ln">
                {l}
              </div>
            ))}
          </pre>
        )}
      </div>

      <div className="searchrow">
        <button className="icon-btn" disabled title="Find options">
          <Search size={13} />
        </button>
        <input
          className="input"
          placeholder="Search in this message"
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          aria-label="Search in message"
        />
        <span>{query ? `${matches} highlight${matches === 1 ? "" : "s"}` : ""}</span>
        <span className="grow" />
        {tab === "raw" && onChange && <span>Line numbers synced</span>}
      </div>
    </div>
  );
}
