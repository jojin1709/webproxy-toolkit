import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import {
  HeldInfo,
  HttpExchange,
  headersToText,
  MatchRule,
  ProxyStatus,
  ScopeRule,
  statusClass,
} from "../types";
import { useContextMenu, MenuItem } from "../components/ContextMenu";
import MessageView from "../components/MessageView";
import SplitPane from "../components/SplitPane";
import { tabListKeyDown } from "../components/tabKeys";
import type { SendTool } from "../App";

type Props = {
  status: ProxyStatus;
  setStatus: (s: ProxyStatus) => void;
  setToast: (t: string) => void;
  sendTo: (tool: SendTool, msg: { raw: string; url: string; response?: string }) => void;
};

type Sub = "intercept" | "history" | "ws" | "options";

function respMime(ex: HttpExchange): string {
  const ct = (ex.resp_headers.find(([k]) => k.toLowerCase() === "content-type")?.[1] ?? "")
    .split(";")[0]
    .trim();
  if (ct.includes("html")) return "HTML";
  if (ct.includes("json")) return "JSON";
  if (ct.includes("javascript")) return "script";
  if (ct.startsWith("image/")) return "image";
  if (ct.includes("xml")) return "XML";
  if (ct.includes("css")) return "CSS";
  if (ct.includes("text/")) return "text";
  if (ct.startsWith("font/")) return "font";
  if (ct.includes("octet-stream")) return "bin";
  return ct.split("/").pop() || "—";
}

function byteLen(s: string | null): number {
  return s ? new TextEncoder().encode(s).length : 0;
}

function timeOf(iso: string): string {
  // started_at is sqlite "YYYY-MM-DD HH:MM:SS" (UTC) or ISO — take clock part
  const t = iso.includes("T") ? iso.slice(11, 19) : iso.slice(11, 19);
  return t || iso;
}

function rawRequest(ex: HttpExchange): string {
  return (
    `${ex.method} ${ex.url} HTTP/1.1\r\n` +
    ex.req_headers.map(([k, v]) => `${k}: ${v}`).join("\r\n") +
    "\r\n\r\n" +
    (ex.req_body ?? "")
  );
}

function rawResponse(ex: HttpExchange): string {
  return (
    `HTTP/1.1 ${ex.status ?? ""}\n` +
    headersToText(ex.resp_headers) +
    "\n\n" +
    (ex.resp_body ?? "")
  );
}

export default function ProxyPanel({ status, setStatus, setToast, sendTo }: Props) {
  const [sub, setSub] = useState<Sub>("history");
  const [host, setHost] = useState("127.0.0.1");
  const [port, setPort] = useState(status.port || 8080);
  const [filter, setFilter] = useState("");
  const [scopeOnly, setScopeOnly] = useState(false);
  const [history, setHistory] = useState<HttpExchange[]>([]);
  const [selected, setSelected] = useState<HttpExchange | null>(null);
  const [held, setHeld] = useState<HeldInfo | null>(null);
  const [heldEdit, setHeldEdit] = useState("");
  const lastHeldId = useRef<number | null>(null);
  const [rules, setRules] = useState<MatchRule[]>([]);
  const [scopeRules, setScopeRules] = useState<ScopeRule[]>([]);
  const [form, setForm] = useState({
    phase: "request",
    match_kind: "literal",
    match_val: "",
    replace_val: "",
    comment: "",
  });
  const { open: openCtx, node: ctxNode } = useContextMenu();

  const loadHistory = useCallback(async () => {
    try {
      setHistory(
        await invoke<HttpExchange[]>("history_list", {
          filter: { query: filter || null, limit: 500 },
        })
      );
    } catch (e) {
      setToast(String(e));
    }
  }, [filter, setToast]);

  useEffect(() => {
    loadHistory();
  }, [loadHistory, status.history_count]);

  useEffect(() => {
    if (sub === "options") {
      invoke<MatchRule[]>("match_list")
        .then(setRules)
        .catch((e) => setToast(String(e)));
      invoke<ScopeRule[]>("scope_list")
        .then(setScopeRules)
        .catch(() => {});
    }
  }, [sub, status.history_count, setToast]);

  // poll held message while interception is armed
  useEffect(() => {
    if (status.held_count === 0) {
      lastHeldId.current = null;
      setHeld(null);
      return;
    }
    let cancelled = false;
    const tick = async () => {
      try {
        const h = await invoke<HeldInfo | null>("intercept_peek");
        if (cancelled) return;
        if (!h) return;
        if (lastHeldId.current !== h.id) {
          lastHeldId.current = h.id;
          setHeld(h);
          setHeldEdit(h.raw_request);
        } else {
          setHeld(h);
        }
      } catch {
        /* ignore */
      }
    };
    tick();
    const id = window.setInterval(tick, 500);
    return () => {
      cancelled = true;
      window.clearInterval(id);
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [status.held_count]);

  async function startProxy() {
    try {
      const p = await invoke<number>("proxy_start", { host, port });
      setPort(p);
      setToast(`Proxy listening on ${host}:${p}`);
      setStatus(await invoke<ProxyStatus>("proxy_status"));
    } catch (e) {
      setToast(String(e));
    }
  }

  async function stopProxy() {
    try {
      await invoke("proxy_stop");
      setToast("Proxy stopped");
      setStatus(await invoke<ProxyStatus>("proxy_status"));
    } catch (e) {
      setToast(String(e));
    }
  }

  async function toggleIntercept() {
    setStatus(
      await invoke<ProxyStatus>("intercept_set", {
        enabled: !status.intercept,
        response: status.intercept_resp,
      })
    );
  }

  async function toggleInterceptResp() {
    setStatus(
      await invoke<ProxyStatus>("intercept_set", {
        enabled: status.intercept,
        response: !status.intercept_resp,
      })
    );
  }

  async function forwardHeld() {
    if (!held) return;
    try {
      await invoke("intercept_forward", { id: held.id, raw: heldEdit });
      setHeld(null);
    } catch (e) {
      setToast(String(e));
    }
  }

  async function dropHeld() {
    if (!held) return;
    try {
      await invoke("intercept_drop", { id: held.id });
      setHeld(null);
    } catch (e) {
      setToast(String(e));
    }
  }

  const visible = useMemo(
    () => (scopeOnly ? history.filter((h) => h.in_scope) : history),
    [history, scopeOnly]
  );

  function rowKeyDown(e: React.KeyboardEvent<HTMLTableRowElement>, idx: number) {
    if (e.key === "Enter" || e.key === " ") {
      e.preventDefault();
      setSelected(visible[idx]);
      return;
    }
    if (e.key !== "ArrowDown" && e.key !== "ArrowUp") return;
    const rows = Array.from(
      e.currentTarget.parentElement?.querySelectorAll<HTMLTableRowElement>("tr[tabindex]") ?? []
    );
    const i = rows.indexOf(e.currentTarget);
    const next = e.key === "ArrowDown" ? i + 1 : i - 1;
    if (next < 0 || next >= rows.length) return;
    e.preventDefault();
    rows[next].focus();
    setSelected(visible[next]);
  }

  function rowMenu(ex: HttpExchange): MenuItem[] {
    const addScope = () =>
      invoke("scope_add", { mode: "include", kind: "host", pattern: ex.host })
        .then(() => setToast(`Added ${ex.host} to scope`))
        .catch((e) => setToast(String(e)));
    const removeScope = () => {
      const rule = scopeRules.find((r) => r.mode === "include" && r.pattern === ex.host);
      if (rule)
        invoke("scope_delete", { id: rule.id })
          .then(() => setToast(`Removed ${ex.host} from scope`))
          .catch((e) => setToast(String(e)));
      else setToast("No matching scope rule");
    };
    return [
      { label: "Send to Repeater", hint: "Ctrl+R", onClick: () => sendTo("repeater", { raw: rawRequest(ex), url: ex.url }) },
      { label: "Send to Intruder", hint: "Ctrl+I", onClick: () => sendTo("intruder", { raw: rawRequest(ex), url: ex.url }) },
      { label: "Send to Comparer (request)", onClick: () => sendTo("comparer", { raw: rawRequest(ex), url: ex.url }) },
      { label: "Send to Comparer (response)", onClick: () => sendTo("comparer", { raw: rawResponse(ex), url: ex.url, response: rawRequest(ex) }) },
      { sep: true },
      { label: "Do an active scan", disabled: true, hint: "Phase 4" },
      { label: ex.in_scope ? "Remove from scope" : "Add to scope", onClick: ex.in_scope ? removeScope : addScope },
      { sep: true },
      { label: "Copy URL", onClick: () => navigator.clipboard.writeText(ex.url).then(() => setToast("URL copied")) },
      { label: "Copy request", onClick: () => navigator.clipboard.writeText(rawRequest(ex)).then(() => setToast("Request copied")) },
    ];
  }

  async function addRule() {
    try {
      await invoke("match_add", {
        phase: form.phase,
        matchKind: form.match_kind,
        matchVal: form.match_val,
        replaceVal: form.replace_val,
        comment: form.comment || null,
      });
      setRules(await invoke<MatchRule[]>("match_list"));
      setForm({ ...form, match_val: "", replace_val: "", comment: "" });
    } catch (e) {
      setToast(String(e));
    }
  }

  async function deleteRule(id: number) {
    await invoke("match_delete", { id });
    setRules(await invoke<MatchRule[]>("match_list"));
  }

  const SUBS: [Sub, string, string?][] = [
    ["intercept", "Intercept"],
    ["history", "HTTP history"],
    ["ws", "WebSockets history", "Phase 2"],
    ["options", "Options"],
  ];

  return (
    <div className="panel" role="tabpanel" aria-label="Proxy" id="panel-proxy">
      <div className="subtabs" role="tablist" aria-label="Proxy views">
        {SUBS.map(([id, label, hint], i) => (
          <button
            key={id}
            role="tab"
            aria-selected={sub === id}
            tabIndex={sub === id ? 0 : -1}
            disabled={hint != null}
            title={hint}
            className={sub === id ? "active" : ""}
            onClick={() => setSub(id)}
            onKeyDown={(e) =>
              tabListKeyDown(e, i, SUBS.map(([, , h]) => h != null), (next) =>
                setSub(SUBS[next][0])
              )
            }
          >
            {label}
          </button>
        ))}
      </div>

      {sub === "intercept" && (
        <div className="panel">
          <div className="toolbar">
            <button
              className={`btn big ${status.intercept || status.intercept_resp ? "danger" : ""}`}
              onClick={toggleIntercept}
            >
              Intercept is {status.intercept ? "on" : "off"}
            </button>
            <label className="switch">
              <input
                type="checkbox"
                checked={status.intercept_resp}
                onChange={toggleInterceptResp}
              />
              also responses
            </label>
            <span className="grow" />
            <button className="btn primary" disabled={!held} onClick={forwardHeld}>
              Forward
            </button>
            <button className="btn danger" disabled={!held} onClick={dropHeld}>
              Drop
            </button>
            <button className="btn" disabled title="Phase 2: follow-up actions">
              Action ▾
            </button>
          </div>
          {held ? (
            <div className="intercept-meta">
              Holding {held.direction} to <span className="mono">{held.host}:{held.port}</span>
              {held.tls ? " (HTTPS)" : ""}
            </div>
          ) : null}
          <div className="grow min-h-0">
            {held ? (
              <MessageView
                value={heldEdit}
                onChange={setHeldEdit}
                title="Held message"
                defaultTab="pretty"
              />
            ) : status.intercept || status.intercept_resp ? (
              <div className="blank" tabIndex={0}>
                <div className="blank-inner">
                  <div className="blank-art" aria-hidden>
                    <svg width="90" height="70" viewBox="0 0 90 70" fill="none">
                      <rect x="34" y="6" width="22" height="52" rx="5" fill="#2a78bd" />
                      <circle cx="45" cy="24" r="7" fill="#ff6a3d" />
                      <rect x="39" y="36" width="12" height="16" rx="3" fill="#0e1116" opacity="0.5" />
                      <path d="M12 20 L26 30 M12 50 L26 40 M78 20 L64 30 M78 50 L64 40" stroke="currentColor" strokeWidth="2.5" strokeLinecap="round" />
                    </svg>
                  </div>
                  <h2>Interception is on</h2>
                  <p>
                    Messages matching your interception rules are held here so you can analyze
                    and modify them before they continue. Browse through the proxy to trigger one.
                  </p>
                  <div className="blank-actions">
                    <button className="btn" onClick={() => setSub("history")}>
                      View HTTP history
                    </button>
                    <button
                      className="btn primary"
                      onClick={async () => {
                        await navigator.clipboard
                          .writeText(`127.0.0.1:${status.port}`)
                          .catch(() => {});
                        setToast(`Proxy address 127.0.0.1:${status.port} copied`);
                      }}
                    >
                      Copy proxy address
                    </button>
                  </div>
                </div>
              </div>
            ) : (
              <div className="blank" tabIndex={0}>
                <div className="blank-inner">
                  <div className="blank-art" aria-hidden>
                    <svg width="90" height="70" viewBox="0 0 90 70" fill="none">
                      <rect x="34" y="6" width="22" height="52" rx="5" fill="#2a78bd" />
                      <circle cx="45" cy="24" r="7" fill="#565660" />
                      <rect x="39" y="36" width="12" height="16" rx="3" fill="#0e1116" opacity="0.5" />
                      <path d="M12 20 L26 30 M12 50 L26 40 M78 20 L64 30 M78 50 L64 40" stroke="currentColor" strokeWidth="2.5" strokeLinecap="round" opacity="0.4" />
                    </svg>
                  </div>
                  <h2>Intercept is off</h2>
                  <p>
                    If you turn Intercept on, messages between your browser and the target
                    servers are held here. This lets you analyze and modify these messages
                    before you forward them.
                  </p>
                  <div className="blank-actions">
                    <button
                      className="btn primary"
                      onClick={async () => {
                        await navigator.clipboard
                          .writeText(`127.0.0.1:${status.port}`)
                          .catch(() => {});
                        setToast(`Proxy address 127.0.0.1:${status.port} copied`);
                      }}
                    >
                      Copy proxy address
                    </button>
                    <button className="btn" onClick={toggleIntercept}>
                      Turn Intercept on
                    </button>
                  </div>
                </div>
              </div>
            )}
          </div>
        </div>
      )}

      {sub === "history" && (
        <div className="panel grow min-h-0">
          <SplitPane direction="column" initial={52} min={20} max={80}>
            <>
              <div
                className="table-wrap"
                role="region"
                aria-label="HTTP history"
                tabIndex={0}
              >
                <table className="history">
                  <thead>
                    <tr>
                      <th>#</th>
                      <th>Method</th>
                      <th>Host</th>
                      <th>URL</th>
                      <th>Status</th>
                      <th>Length</th>
                      <th>MIME</th>
                      <th>Time</th>
                      <th>ms</th>
                      <th>TLS</th>
                      <th>Scope</th>
                    </tr>
                  </thead>
                  <tbody>
                    {visible.map((ex, i) => (
                      <tr
                        key={ex.id}
                        className={selected?.id === ex.id ? "selected" : ""}
                        tabIndex={0}
                        aria-selected={selected?.id === ex.id}
                        onClick={() => setSelected(ex)}
                        onKeyDown={(e) => rowKeyDown(e, i)}
                        onDoubleClick={() => sendTo("repeater", { raw: rawRequest(ex), url: ex.url })}
                        onContextMenu={(e) => openCtx(e, rowMenu(ex))}
                      >
                        <td>{ex.id}</td>
                        <td className={ex.method === "POST" ? "m-post" : "m-get"}>{ex.method}</td>
                        <td title={ex.host}>{ex.host}</td>
                        <td title={ex.url}>{ex.url}</td>
                        <td>
                          <span className={statusClass(ex.status)}>{ex.status ?? "—"}</span>
                        </td>
                        <td>{byteLen(ex.resp_body)}</td>
                        <td>{respMime(ex)}</td>
                        <td>{timeOf(ex.started_at)}</td>
                        <td>{ex.time_ms ?? "—"}</td>
                        <td>{ex.tls ? "TLS" : ""}</td>
                        <td>{ex.in_scope ? "✓" : ""}</td>
                      </tr>
                    ))}
                    {visible.length === 0 && (
                      <tr>
                        <td colSpan={11} className="muted">
                          {history.length === 0
                            ? "No traffic yet. Start the proxy, then browse through it."
                            : "No rows match the filter."}
                        </td>
                      </tr>
                    )}
                  </tbody>
                </table>
              </div>
              <div className="filterbar">
                <input
                  className="input grow"
                  placeholder="Filter — URL, host, method, status…"
                  value={filter}
                  onChange={(e) => setFilter(e.target.value)}
                  aria-label="Filter history"
                />
                <label className="switch">
                  <input
                    type="checkbox"
                    checked={scopeOnly}
                    onChange={(e) => setScopeOnly(e.target.checked)}
                  />
                  In-scope only
                </label>
                <span className="muted">
                  {visible.length}/{history.length}
                </span>
                <button className="btn" onClick={loadHistory} title="Reload history">
                  Refresh
                </button>
              </div>
            </>
            <>
              {selected ? (
                <SplitPane direction="row" initial={50} min={20} max={80}>
                  <div className="pane-flush">
                    <div className="pane-caption">
                      Request
                      <span className="grow" />
                      <button
                        className="btn mini"
                        onClick={() => sendTo("repeater", { raw: rawRequest(selected), url: selected.url })}
                      >
                        Send to Repeater
                      </button>
                    </div>
                    <div className="grow min-h-0">
                      <MessageView value={rawRequest(selected)} />
                    </div>
                  </div>
                  <div className="pane-flush">
                    <div className="pane-caption">Response</div>
                    <div className="grow min-h-0">
                      <MessageView value={rawResponse(selected)} />
                    </div>
                  </div>
                </SplitPane>
              ) : (
                <div className="empty">Select a request to view its message.</div>
              )}
            </>
          </SplitPane>
        </div>
      )}

      {sub === "ws" && (
        <div className="empty">WebSocket history arrives in Phase 2.</div>
      )}

      {sub === "options" && (
        <div className="pane-body options-body">
          <section>
            <h2>Proxy listeners</h2>
            <table className="history">
              <thead>
                <tr>
                  <th>Running</th>
                  <th>Interface</th>
                  <th>Port</th>
                  <th aria-label="Actions">
                    <span className="sr-only">Actions</span>
                  </th>
                </tr>
              </thead>
              <tbody>
                <tr>
                  <td>{status.running ? "✓" : ""}</td>
                  <td className="mono">{host}</td>
                  <td className="mono">{status.running ? status.port : port}</td>
                  <td>
                    {!status.running ? (
                      <button className="btn primary" onClick={startProxy}>
                        Start
                      </button>
                    ) : (
                      <button className="btn danger" onClick={stopProxy}>
                        Stop
                      </button>
                    )}
                  </td>
                </tr>
              </tbody>
            </table>
            {!status.running && (
              <div className="row" style={{ marginTop: 8 }}>
                <input
                  className="input"
                  style={{ width: 140 }}
                  value={host}
                  onChange={(e) => setHost(e.target.value)}
                  aria-label="Listen host"
                />
                <input
                  className="input"
                  style={{ width: 90 }}
                  value={port}
                  onChange={(e) => setPort(Number(e.target.value) || 0)}
                  aria-label="Listen port"
                  type="number"
                />
              </div>
            )}
          </section>

          <section>
            <h2>Intercept</h2>
            <div className="row">
              <label className="switch">
                <input
                  type="checkbox"
                  checked={status.intercept}
                  onChange={toggleIntercept}
                />
                Intercept client requests
              </label>
              <label className="switch">
                <input
                  type="checkbox"
                  checked={status.intercept_resp}
                  onChange={toggleInterceptResp}
                />
                Intercept server responses
              </label>
            </div>
          </section>

          <section>
            <h2>Match and replace rules</h2>
            <p className="muted">
              Rules rewrite traffic as it passes through the proxy. Content-Length is
              recalculated automatically.
            </p>
            <table className="history" style={{ marginBottom: 12 }}>
              <thead>
                <tr>
                  <th>On</th>
                  <th>Phase</th>
                  <th>Type</th>
                  <th>Match</th>
                  <th>Replace</th>
                  <th aria-label="Actions">
                    <span className="sr-only">Actions</span>
                  </th>
                </tr>
              </thead>
              <tbody>
                {rules.map((r) => (
                  <tr key={r.id}>
                    <td>{r.enabled ? "✓" : ""}</td>
                    <td>{r.phase}</td>
                    <td>{r.match_kind}</td>
                    <td title={r.match_val}>{r.match_val}</td>
                    <td title={r.replace_val}>{r.replace_val}</td>
                    <td>
                      <button className="btn" onClick={() => deleteRule(r.id)}>
                        Delete
                      </button>
                    </td>
                  </tr>
                ))}
                {rules.length === 0 && (
                  <tr>
                    <td colSpan={6} className="muted">
                      No rules yet.
                    </td>
                  </tr>
                )}
              </tbody>
            </table>
            <div className="row" style={{ flexWrap: "wrap" }}>
              <select
                className="select"
                value={form.phase}
                onChange={(e) => setForm({ ...form, phase: e.target.value })}
                aria-label="Phase"
              >
                <option value="request">Request</option>
                <option value="response">Response</option>
              </select>
              <select
                className="select"
                value={form.match_kind}
                onChange={(e) => setForm({ ...form, match_kind: e.target.value })}
                aria-label="Match type"
              >
                <option value="literal">Literal</option>
                <option value="regex">Regex</option>
                <option value="header">Header</option>
              </select>
              <input
                className="input grow"
                placeholder={
                  form.match_kind === "header" ? "Header name or Name: value-part" : "Match"
                }
                value={form.match_val}
                onChange={(e) => setForm({ ...form, match_val: e.target.value })}
                aria-label="Match value"
              />
              <input
                className="input grow"
                placeholder="Replace with"
                value={form.replace_val}
                onChange={(e) => setForm({ ...form, replace_val: e.target.value })}
                aria-label="Replace value"
              />
              <input
                className="input"
                placeholder="Comment"
                value={form.comment}
                onChange={(e) => setForm({ ...form, comment: e.target.value })}
                aria-label="Comment"
              />
              <button className="btn primary" onClick={addRule} disabled={!form.match_val}>
                Add rule
              </button>
            </div>
          </section>
        </div>
      )}

      {ctxNode}
    </div>
  );
}
