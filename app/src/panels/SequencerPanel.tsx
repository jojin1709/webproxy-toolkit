import { useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { analyzeTokens, Verdict } from "../lib/entropy";

type Source = "cookie" | "header" | "regex";

const SOURCES: [Source, string, string][] = [
  ["cookie", "Cookie value", "Cookie name (e.g. SESSIONID)"],
  ["header", "Response header", "Header name (e.g. X-Request-Id)"],
  ["regex", "Regex on body", "Regex with one capture group (e.g. csrf\" value=\"([^\"]+))"],
];

function verdictChip(v: Verdict): string {
  if (v === "pass") return "chip ok";
  if (v === "warn") return "chip warn";
  return "chip err";
}

function showChar(ch: string): string {
  if (ch === "") return "—";
  if (ch === " ") return "space";
  const code = ch.charCodeAt(0);
  if (code < 33 || code === 127) return `0x${code.toString(16).padStart(2, "0")}`;
  return ch;
}

export default function SequencerPanel({ setToast }: { setToast: (t: string) => void }) {
  const [mode, setMode] = useState<"request" | "paste">("request");
  const [url, setUrl] = useState("http://127.0.0.1/");
  const [source, setSource] = useState<Source>("cookie");
  const [pattern, setPattern] = useState("");
  const [count, setCount] = useState(20);
  const [paste, setPaste] = useState("");
  const [busy, setBusy] = useState(false);
  const [tokens, setTokens] = useState<string[]>([]);
  const [err, setErr] = useState("");
  const report = useMemo(() => (tokens.length > 0 ? analyzeTokens(tokens) : null), [tokens]);
  const pasted = useMemo(
    () =>
      paste
        .split("\n")
        .map((t) => t.trim())
        .filter((t) => t.length > 0),
    [paste]
  );

  const placeholder = SOURCES.find((s) => s[0] === source)?.[2] ?? "";

  async function start() {
    setBusy(true);
    setErr("");
    try {
      const got = await invoke<string[]>("sequencer_sample", { url, source, pattern, count });
      if (!Array.isArray(got) || got.length === 0) {
        setTokens([]);
        setErr("No samples returned");
        return;
      }
      setTokens(got);
      setToast(`Collected ${got.length} samples`);
    } catch (e) {
      setTokens([]);
      setErr(String(e));
    } finally {
      setBusy(false);
    }
  }

  function analyzePaste() {
    setTokens(pasted);
    setErr("");
    setToast(`Analyzed ${pasted.length} pasted tokens`);
  }

  function clear() {
    setTokens([]);
    setErr("");
  }

  return (
    <div className="panel" role="tabpanel" aria-label="Sequencer" id="panel-sequencer">
      <div className="toolbar toolbar-top">
        <div className="segmented" role="group" aria-label="Sequencer mode">
          <button
            className={mode === "request" ? "active" : ""}
            aria-pressed={mode === "request"}
            onClick={() => setMode("request")}
          >
            Request URL
          </button>
          <button
            className={mode === "paste" ? "active" : ""}
            aria-pressed={mode === "paste"}
            onClick={() => setMode("paste")}
          >
            Paste tokens
          </button>
        </div>
        {mode === "request" ? (
          <>
            <span className="muted">URL</span>
            <input
              className="input grow"
              value={url}
              onChange={(e) => setUrl(e.target.value)}
              placeholder="https://example.com/"
              aria-label="URL to sample"
            />
            <span className="muted">Token source</span>
            <select
              className="select"
              aria-label="Token source"
              value={source}
              onChange={(e) => setSource(e.target.value as Source)}
            >
              {SOURCES.map(([v, l]) => (
                <option key={v} value={v}>
                  {l}
                </option>
              ))}
            </select>
            <input
              className="input grow"
              value={pattern}
              onChange={(e) => setPattern(e.target.value)}
              placeholder={placeholder}
              aria-label="Sample pattern"
            />
            <span className="muted">Samples</span>
            <input
              className="input"
              type="number"
              min={1}
              max={500}
              value={count}
              onChange={(e) => {
                const v = parseInt(e.target.value, 10);
                setCount(Number.isNaN(v) ? 1 : Math.min(500, Math.max(1, v)));
              }}
              aria-label="Number of samples"
            />
            <button className="btn primary" onClick={start} disabled={busy}>
              {busy ? "Sampling…" : "Start"}
            </button>
          </>
        ) : (
          <>
            <textarea
              className="seq-paste"
              rows={8}
              value={paste}
              onChange={(e) => setPaste(e.target.value)}
              placeholder="Paste one token per line"
              aria-label="Paste one token per line"
              spellCheck={false}
            />
            <button className="btn primary" onClick={analyzePaste} disabled={pasted.length === 0}>
              Analyze
            </button>
            <span className="chip">{pasted.length} lines</span>
          </>
        )}
        <button className="btn" onClick={clear} disabled={tokens.length === 0}>
          Clear
        </button>
        {err && <span className="chip err">{err}</span>}
      </div>

      {!report ? (
        <div className="blank" tabIndex={0}>
          <div className="blank-inner">
            <h2>No samples yet</h2>
            <p>
              Collect samples from a URL — pick where the token lives (a cookie, a response
              header, or a regex on the body) and press Start — or switch to Paste tokens and
              drop in one token per line. Either way the same randomness battery runs: chi-square
              uniformity, per-position uniqueness, entropy estimates, Monte Carlo π and
              arithmetic mean.
            </p>
          </div>
        </div>
      ) : (
        <div className="split side">
          <div className="pane">
            <div className="pane-title">
              Samples <span className="muted">{report.samples} collected</span>
            </div>
            <div className="table-wrap">
              <table className="history">
                <thead>
                  <tr>
                    <th>#</th>
                    <th>Token</th>
                  </tr>
                </thead>
                <tbody>
                  {tokens.map((t, i) => (
                    <tr key={i}>
                      <td>{i + 1}</td>
                      <td className="mono seq-token">{t}</td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          </div>
          <div className="pane">
            <div className="pane-title">Randomness analysis</div>
            <div className="pane-body">
              <section className="card">
                <h2>Statistics</h2>
                {report.stats.map((s) => (
                  <div className="task-config-row" key={s.label}>
                    <span className="k">{s.label}</span>
                    <span className="mono">{s.value}</span>
                  </div>
                ))}
              </section>
              <section className="card">
                <h2>Randomness tests</h2>
                {report.tests.map((t) => (
                  <div className="task-config-row" key={t.name}>
                    <span className="k">{t.name}</span>
                    <span className="row">
                      <span className="mono muted">{t.detail}</span>
                      <span className={verdictChip(t.verdict)}>{t.verdictText}</span>
                    </span>
                  </div>
                ))}
              </section>
              <section className="card">
                <h2>Byte-level (per position)</h2>
                <p className="muted">
                  Distinct values, dominant character and entropy observed at each token
                  position — a random token stays near 100% uniqueness at every position.
                </p>
                {report.positions.length === 0 ? (
                  <div className="empty">No position data</div>
                ) : (
                  <table className="history">
                    <thead>
                      <tr>
                        <th scope="col">Position</th>
                        <th scope="col">Distinct values</th>
                        <th scope="col">Most common</th>
                        <th scope="col">Entropy (bits)</th>
                        <th scope="col">Uniqueness</th>
                      </tr>
                    </thead>
                    <tbody>
                      {report.positions.map((p) => (
                        <tr key={p.index}>
                          <td className="mono">#{p.index + 1}</td>
                          <td className="mono">{p.distinct}</td>
                          <td className="mono">
                            {showChar(p.common.ch)} × {p.common.n}
                          </td>
                          <td className="mono">{p.entropy.toFixed(2)}</td>
                          <td>
                            <div className="seq-meter-row">
                              <meter
                                className="seq-meter"
                                min={0}
                                max={1}
                                low={0.75}
                                high={0.9}
                                optimum={1}
                                value={p.ratio}
                              >
                                {Math.round(p.ratio * 100)}%
                              </meter>
                              <span className="mono muted">{(p.ratio * 100).toFixed(1)}%</span>
                            </div>
                          </td>
                        </tr>
                      ))}
                    </tbody>
                  </table>
                )}
              </section>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}
