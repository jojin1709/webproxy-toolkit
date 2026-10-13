import { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import MessageView from "./MessageView";
import SplitPane from "./SplitPane";
import { sevClass, scannerApi } from "../lib/scanner";
import type { HttpExchange, Issue, IssueSeverity } from "../types";
import { headersToText } from "../types";

type Props = {
  /** Restrict to one host (Target panel); empty = all hosts. */
  host?: string;
  setToast: (t: string) => void;
};

const ALL_SEV: IssueSeverity[] = ["critical", "high", "medium", "low", "info"];

function rawRequest(ex: HttpExchange): string {
  const path = ex.url.replace(/^https?:\/\/[^/]+/, "") || "/";
  return `${ex.method} ${path} HTTP/1.1\r\n${headersToText(ex.req_headers)}\r\n${ex.req_body ?? ""}`;
}

function rawResponse(ex: HttpExchange): string {
  const status = ex.status ?? 0;
  return `HTTP/1.1 ${status}\r\n${headersToText(ex.resp_headers)}\r\n${ex.resp_body ?? ""}`;
}

function timeOf(iso: string): string {
  const d = new Date(iso);
  return Number.isNaN(d.getTime()) ? iso.slice(11, 19) : d.toTimeString().slice(0, 8);
}

/** Shared issues table + detail pane (Dashboard + Target Issues tabs). */
export function IssueTable({ host = "", setToast }: Props) {
  const [rows, setRows] = useState<Issue[]>([]);
  const [selected, setSelected] = useState<Issue | null>(null);
  const [sev, setSev] = useState<IssueSeverity[]>([]);
  const [status, setStatus] = useState<"" | "open" | "acknowledged" | "false-positive">("");
  const [text, setText] = useState("");
  const [exchange, setExchange] = useState<HttpExchange | null>(null);
  const selectedRef = useRef<number | null>(null);
  selectedRef.current = selected?.id ?? null;

  const load = useCallback(async () => {
    try {
      const { rows } = await scannerApi.list({
        severities: sev.length ? sev : undefined,
        host: host || undefined,
        status: status || undefined,
        text: text || undefined,
        limit: 500,
      });
      setRows(rows);
      // keep selection if still present, else clear
      setSelected((cur) => (cur && rows.some((r) => r.id === cur.id) ? cur : null));
    } catch (e) {
      setToast(String(e));
    }
  }, [sev, host, status, text, setToast]);

  useEffect(() => {
    void load();
  }, [load]);

  // live updates from the passive scanner / status changes
  useEffect(() => {
    let offs: Array<() => void> = [];
    let disposed = false;
    void (async () => {
      const unsubs = await Promise.all([
        listen<Issue>("issue-added", (ev) => {
          const iss = ev.payload;
          if (host && iss.host !== host) return;
          if (sev.length && !sev.includes(iss.severity)) return;
          if (status && iss.status !== status) return;
          setRows((cur) => [iss, ...cur].slice(0, 500));
        }),
        listen<Issue>("issue-updated", (ev) => {
          const iss = ev.payload;
          setRows((cur) => cur.map((r) => (r.id === iss.id ? iss : r)));
          setSelected((cur) => (cur && cur.id === iss.id ? iss : cur));
        }),
        listen<number>("issue-deleted", (ev) => {
          const id = ev.payload;
          setRows((cur) => cur.filter((r) => r.id !== id));
          setSelected((cur) => (cur && cur.id === id ? null : cur));
        }),
      ]);
      if (disposed) {
        unsubs.forEach((u) => u());
      } else {
        offs = unsubs;
      }
    })();
    return () => {
      disposed = true;
      offs.forEach((u) => u());
    };
  }, [host, sev, status]);

  // fetch the linked exchange when the selection changes
  useEffect(() => {
    let live = true;
    setExchange(null);
    if (selected?.exchange_id) {
      invoke<HttpExchange | null>("history_get", { id: selected.exchange_id })
        .then((ex) => {
          if (live) setExchange(ex);
        })
        .catch(() => {
          if (live) setExchange(null);
        });
    }
    return () => {
      live = false;
    };
  }, [selected?.id, selected?.exchange_id]);

  const toggleSev = (s: IssueSeverity) =>
    setSev((cur) => (cur.includes(s) ? cur.filter((x) => x !== s) : [...cur, s]));

  const setStatusSafe = async (id: number, next: string) => {
    try {
      await scannerApi.setStatus(id, next);
    } catch (e) {
      setToast(String(e));
    }
  };

  const remove = async (id: number) => {
    try {
      await scannerApi.remove(id);
    } catch (e) {
      setToast(String(e));
    }
  };

  return (
    <div className="split side">
      <div className="pane">
        <div className="filterbar">
          {ALL_SEV.map((s) => (
            <button
              key={s}
              className={`sev-chip ${sevClass(s)}${sev.includes(s) ? " on" : ""}`}
              onClick={() => toggleSev(s)}
              aria-pressed={sev.includes(s)}
            >
              {s}
            </button>
          ))}
          <select
            className="input"
            value={status}
            onChange={(e) => setStatus(e.target.value as typeof status)}
            aria-label="Filter by status"
          >
            <option value="">Any status</option>
            <option value="open">Open</option>
            <option value="acknowledged">Acknowledged</option>
            <option value="false-positive">False positive</option>
          </select>
          <input
            className="input grow"
            placeholder="Filter — name, URL, param, evidence…"
            value={text}
            onChange={(e) => setText(e.target.value)}
            aria-label="Filter issues"
          />
          <span className="muted">{rows.length}</span>
          <button className="btn" onClick={() => void load()} title="Reload issues">
            Refresh
          </button>
        </div>
        <div className="table-wrap grow min-h-0" role="region" aria-label="Issues" tabIndex={0}>
          <table className="history">
            <thead>
              <tr>
                <th>Severity</th>
                <th>Issue</th>
                <th>Host</th>
                <th>Path</th>
                <th>Param</th>
                <th>Confidence</th>
                <th>Status</th>
                <th>Time</th>
              </tr>
            </thead>
            <tbody>
              {rows.map((r) => (
                <tr
                  key={r.id}
                  className={selected?.id === r.id ? "selected" : ""}
                  tabIndex={0}
                  aria-selected={selected?.id === r.id}
                  onClick={() => setSelected(r)}
                  onKeyDown={(e) => {
                    if (e.key === "Enter" || e.key === " ") {
                      e.preventDefault();
                      setSelected(r);
                    }
                  }}
                >
                  <td>
                    <span className={`sev-chip ${sevClass(r.severity)}`}>{r.severity}</span>
                  </td>
                  <td title={r.name}>{r.name}</td>
                  <td title={r.host}>{r.host}</td>
                  <td className="ellipsis" title={r.path}>
                    {r.path}
                  </td>
                  <td title={r.param}>{r.param || "—"}</td>
                  <td>{r.confidence}</td>
                  <td>{r.status}</td>
                  <td>{timeOf(r.created_at)}</td>
                </tr>
              ))}
              {rows.length === 0 && (
                <tr>
                  <td colSpan={8} className="muted empty-cell">
                    {host
                      ? `No issues recorded for ${host} yet.`
                      : "No issues recorded yet — the passive scanner raises findings as traffic flows through the proxy."}
                  </td>
                </tr>
              )}
            </tbody>
          </table>
        </div>
      </div>
      <div className="pane">
        <div className="pane-title">
          Issue detail
          {selected && (
            <>
              <span className="grow" />
              <button
                className={`btn mini${selected.status === "acknowledged" ? " primary" : ""}`}
                onClick={() => void setStatusSafe(selected.id, "acknowledged")}
                disabled={selected.status === "acknowledged"}
              >
                Acknowledge
              </button>
              <button
                className={`btn mini${selected.status === "false-positive" ? " primary" : ""}`}
                onClick={() => void setStatusSafe(selected.id, "false-positive")}
                disabled={selected.status === "false-positive"}
              >
                False positive
              </button>
              <button
                className="btn mini danger"
                onClick={() => void remove(selected.id)}
              >
                Delete
              </button>
            </>
          )}
        </div>
        <div className="pane-body">
          {!selected ? (
            <p className="muted">
              Select an issue on the left to see its description, remediation and the exact
              request/response evidence that raised it.
            </p>
          ) : (
            <div className="issue-detail">
              <div className="issue-detail-head">
                <span className={`sev-chip ${sevClass(selected.severity)}`}>
                  {selected.severity}
                </span>
                <h3>{selected.name}</h3>
                <span className="muted mono">{selected.check_id}</span>
              </div>
              <dl className="issue-meta">
                <dt>URL</dt>
                <dd className="mono ellipsis" title={selected.url}>
                  {selected.url}
                </dd>
                <dt>Method</dt>
                <dd>{selected.method}</dd>
                <dt>Parameter</dt>
                <dd>{selected.param || "—"}</dd>
                <dt>Confidence</dt>
                <dd>{selected.confidence}</dd>
                <dt>Status</dt>
                <dd>{selected.status}</dd>
                <dt>First seen</dt>
                <dd>{selected.created_at}</dd>
              </dl>
              <h4>Description</h4>
              <p>{selected.description}</p>
              <h4>Remediation</h4>
              <p>{selected.remediation}</p>
              <h4>Evidence</h4>
              <pre className="issue-evidence">{selected.evidence}</pre>
              {selected.exchange_id != null && (
                <>
                  <h4>
                    Linked request/response{" "}
                    <span className="muted">(exchange #{selected.exchange_id})</span>
                  </h4>
                  {exchange ? (
                    <SplitPane direction="row" initial={50} min={20} max={80}>
                      <div className="pane-flush">
                        <div className="pane-caption">Request</div>
                        <div className="grow min-h-0">
                          <MessageView value={rawRequest(exchange)} />
                        </div>
                      </div>
                      <div className="pane-flush">
                        <div className="pane-caption">Response</div>
                        <div className="grow min-h-0">
                          <MessageView value={rawResponse(exchange)} />
                        </div>
                      </div>
                    </SplitPane>
                  ) : (
                    <p className="muted">Loading linked exchange…</p>
                  )}
                </>
              )}
            </div>
          )}
        </div>
      </div>
    </div>
  );
}
