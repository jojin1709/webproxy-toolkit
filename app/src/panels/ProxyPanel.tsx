import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import {
  ExtHookInfo,
  ExtMenuItem,
  HeldInfo,
  HttpExchange,
  headersToText,
  MatchRule,
  ProxyListenerSlot,
  ProxyStatus,
  ScopeRule,
  statusClass,
  SessionMacro,
  SessionRule,
  SessionUpdate,
  UpstreamRule,
  WsConn,
  WsFrame,
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
  jumpTo?: { exId: number; seq: number } | null;
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

const WS_OPCODES: [number, string][] = [
  [0, "continuation"],
  [1, "text"],
  [2, "binary"],
  [8, "close"],
  [9, "ping"],
  [10, "pong"],
];

function wsOpcodeName(op: number): string {
  return WS_OPCODES.find(([n]) => n === op)?.[1] ?? `op${op}`;
}

function wsDirLabel(dir: string): string {
  return dir === "c" ? "→ sent" : dir === "s" ? "← received" : dir;
}

function wsFrameText(f: WsFrame): string {
  const head = `${wsDirLabel(f.dir)} · ${wsOpcodeName(f.opcode)} frame · ${f.len} bytes · ${timeOf(f.ts)}`;
  const body = f.payload.length ? f.payload : "(no payload)";
  return `${head}\n\n${body}`;
}

function wsHandshake(c: WsConn): string {
  let target = c.url;
  try {
    const u = new URL(c.url);
    target = `${u.pathname}${u.search}`;
  } catch {
    /* keep the raw URL when it is not parseable */
  }
  return (
    `GET ${target} HTTP/1.1\r\n` +
    `Host: ${c.host}${c.port === 80 || c.port === 443 ? "" : `:${c.port}`}\r\n` +
    `Connection: Upgrade\r\nUpgrade: websocket\r\nSec-WebSocket-Version: 13\r\n` +
    `Sec-WebSocket-Key: <redacted>\r\n\r\n` +
    `HTTP/1.1 101 Switching Protocols\r\nConnection: Upgrade\r\nUpgrade: websocket`
  );
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

export default function ProxyPanel({ status, setStatus, setToast, sendTo, jumpTo }: Props) {
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
  const { open: openCtx, close: closeCtx, node: ctxNode, isOpen: menuOpen } = useContextMenu();
  const [wsConns, setWsConns] = useState<WsConn[]>([]);
  const [wsSel, setWsSel] = useState<WsConn | null>(null);
  const [wsFrames, setWsFrames] = useState<WsFrame[]>([]);
  const [wsFrameSel, setWsFrameSel] = useState<WsFrame | null>(null);
  const [wsShowHs, setWsShowHs] = useState(false);
  const wsSelIdRef = useRef<number | null>(null);
  const [extMenus, setExtMenus] = useState<ExtMenuItem[]>([]);
  const [upstreams, setUpstreams] = useState<UpstreamRule[]>([]);
  const [passList, setPassList] = useState<string[]>([]);
  const [listeners, setListeners] = useState<ProxyListenerSlot[]>([]);
  const [upForm, setUpForm] = useState({ host_pattern: "", proxy_host: "", proxy_port: 3128, use_tls: false });
  const [passInput, setPassInput] = useState("");
  const [listenForm, setListenForm] = useState({ host: "127.0.0.1", port: 8081 });
  const [macros, setMacros] = useState<SessionMacro[]>([]);
  const [sessRules, setSessRules] = useState<SessionRule[]>([]);
  const [macroForm, setMacroForm] = useState({ name: "", requests: "" });
  const [ruleForm, setRuleForm] = useState({
    name: "",
    tool: "any",
    match_host: "",
    macro_id: "",
  });

  useEffect(() => {
    invoke<ExtHookInfo>("extension_hooks")
      .then((info) => setExtMenus(info.context_menus))
      .catch(() => setExtMenus([]));
  }, []);

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
      invoke<UpstreamRule[]>("upstream_list").then(setUpstreams).catch(() => {});
      invoke<string[]>("pass_through_list").then(setPassList).catch(() => {});
      invoke<ProxyListenerSlot[]>("listener_list").then(setListeners).catch(() => {});
      invoke<SessionMacro[]>("session_macro_list").then(setMacros).catch(() => {});
      invoke<SessionRule[]>("session_rule_list").then(setSessRules).catch(() => {});
    }
  }, [sub, status.history_count, setToast]);

  const wsConnsSeq = useRef(0);
  const loadWs = useCallback(async () => {
    const seq = ++wsConnsSeq.current;
    try {
      const conns = await invoke<WsConn[]>("ws_conns_list", { limit: 200 });
      if (seq !== wsConnsSeq.current) return;
      setWsConns(conns);
      setWsSel((prev) => (prev ? (conns.find((c) => c.id === prev.id) ?? null) : null));
    } catch (e) {
      if (seq === wsConnsSeq.current) setToast(String(e));
    }
  }, [setToast]);

  const wsSeq = useRef(0);

  useEffect(() => {
    if (sub !== "ws") return;
    let unlisten: (() => void) | undefined;
    let cancelled = false;
    let timer = 0;
    // Frame traffic arrives in bursts: coalesce into one refresh per 250ms.
    const scheduleRefresh = () => {
      window.clearTimeout(timer);
      timer = window.setTimeout(() => {
        if (cancelled) return;
        loadWs();
        const id = wsSelIdRef.current;
        if (id) loadFrames(id);
      }, 250);
    };
    loadWs();
    listen("ws-updated", scheduleRefresh)
      .then((un) => {
        if (cancelled) un();
        else unlisten = un;
      })
      .catch(() => {});
    return () => {
      cancelled = true;
      window.clearTimeout(timer);
      unlisten?.();
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [sub, loadWs]);

  async function loadFrames(connId: number) {
    const seq = ++wsSeq.current;
    try {
      const frames = await invoke<WsFrame[]>("ws_frames_list", { connId, limit: 500 });
      if (seq !== wsSeq.current) return;
      setWsFrames(frames);
      setWsFrameSel((prev) => (prev ? (frames.find((f) => f.id === prev.id) ?? null) : null));
    } catch (e) {
      if (seq === wsSeq.current) setToast(String(e));
    }
  }

  useEffect(() => {
    wsSelIdRef.current = wsSel?.id ?? null;
    if (!wsSel) {
      setWsFrames([]);
      setWsFrameSel(null);
      return;
    }
    setWsShowHs(false);
    loadFrames(wsSel.id);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [wsSel?.id]);

  useEffect(() => {
    if (!jumpTo) return;
    setSub("history");
    // The jumped-to row must actually be visible: drop any filter hiding it.
    setFilter("");
    setScopeOnly(false);
    invoke<HttpExchange | null>("history_get", { id: jumpTo.exId })
      .then((ex) => {
        if (!ex) {
          setToast("That request is no longer in history");
          return;
        }
        setSelected(ex);
        setHistory((prev) => (prev.some((p) => p.id === ex.id) ? prev : [ex, ...prev]));
      })
      .catch((e) => setToast(String(e)));
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [jumpTo]);

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

  function extMessageFor(ex: HttpExchange) {
    return {
      request: {
        method: ex.method,
        url: ex.url,
        headers: ex.req_headers,
        body: ex.req_body ?? "",
      },
      response:
        ex.status == null
          ? null
          : {
              status: ex.status,
              headers: ex.resp_headers,
              body: ex.resp_body ?? "",
            },
    };
  }

  function runExtAction(m: ExtMenuItem, message: unknown) {
    invoke("extension_context_invoke", {
      extId: m.ext_id,
      menuId: m.menu_id,
      message,
    })
      .then(() => setToast(`Ran extension action: ${m.label}`))
      .catch((e) => setToast(String(e)));
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
    const extItems: MenuItem[] =
      extMenus.length === 0
        ? []
        : [
            { sep: true },
            { label: "Extensions", disabled: true },
            ...extMenus.map((m) => ({
              label: m.label,
              onClick: () => runExtAction(m, extMessageFor(ex)),
            })),
          ];
    return [
      { label: "Send to Repeater", hint: "Ctrl+R", onClick: () => sendTo("repeater", { raw: rawRequest(ex), url: ex.url }) },
      { label: "Send to Intruder", hint: "Ctrl+I", onClick: () => sendTo("intruder", { raw: rawRequest(ex), url: ex.url }) },
      { label: "Send to Comparer (request)", onClick: () => sendTo("comparer", { raw: rawRequest(ex), url: ex.url }) },
      { label: "Send to Comparer (response)", onClick: () => sendTo("comparer", { raw: rawResponse(ex), url: ex.url, response: rawRequest(ex) }) },
      { sep: true },
      { label: "Do an active scan", disabled: true, hint: "Phase 4" },
      { label: ex.in_scope ? "Remove from scope" : "Add to scope", onClick: ex.in_scope ? removeScope : addScope },
      { sep: true },
      { label: "Copy URL", onClick: () => copyText(ex.url, "URL") },
      { label: "Copy request", onClick: () => copyText(rawRequest(ex), "Request") },
      ...extItems,
    ];
  }

  function copyText(text: string, label: string) {
    navigator.clipboard
      .writeText(text)
      .then(() => setToast(`${label} copied`))
      .catch(() => setToast(`Could not copy ${label.toLowerCase()} — clipboard unavailable`));
  }

  function heldRawUrl(): string {
    if (!held) return "";
    const firstLine = heldEdit.split(/\r?\n/)[0] ?? "";
    const target = firstLine.split(" ")[1] ?? "/";
    if (/^https?:\/\//.test(target)) return target;
    const proto = held.tls ? "https" : "http";
    const defaultPort = (held.tls && held.port === 443) || (!held.tls && held.port === 80);
    return `${proto}://${held.host}${defaultPort ? "" : `:${held.port}`}${target}`;
  }

  function actionMenu(e: React.MouseEvent<HTMLButtonElement>) {
    if (menuOpen) {
      closeCtx();
      return;
    }
    const rect = e.currentTarget.getBoundingClientRect();
    // Always anchor to the button itself so the menu never covers it.
    const anchor = { x: rect.left, y: rect.bottom + 4 };
    const armed = status.intercept || status.intercept_resp;
    openCtx(e, [
      { label: "Forward", hint: "Enter", disabled: !held, onClick: forwardHeld },
      { label: "Drop", disabled: !held, onClick: dropHeld },
      { sep: true },
      {
        label: armed ? "Turn interception off" : "Turn interception on",
        onClick: toggleIntercept,
      },
      {
        label: status.intercept_resp ? "Stop intercepting responses" : "Intercept responses too",
        onClick: toggleInterceptResp,
      },
      { sep: true },
      {
        label: "Open in Repeater",
        disabled: !held,
        onClick: () => held && sendTo("repeater", { raw: heldEdit, url: heldRawUrl() }),
      },
      {
        label: "Copy held message",
        disabled: !held,
        onClick: () => copyText(heldEdit, "Held message"),
      },
      { sep: true },
      { label: "Do an active scan", disabled: true, hint: "Phase 4" },
    ], anchor);
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

  async function addUpstream() {
    try {
      await invoke("upstream_add", {
        hostPattern: upForm.host_pattern,
        proxyHost: upForm.proxy_host,
        proxyPort: upForm.proxy_port,
        useTls: upForm.use_tls,
      });
      setUpstreams(await invoke<UpstreamRule[]>("upstream_list"));
      setUpForm({ host_pattern: "", proxy_host: "", proxy_port: 3128, use_tls: false });
    } catch (e) {
      setToast(String(e));
    }
  }

  async function deleteUpstream(id: number) {
    await invoke("upstream_delete", { id });
    setUpstreams(await invoke<UpstreamRule[]>("upstream_list"));
  }

  async function addPass() {
    const host = passInput.trim();
    if (!host) return;
    const next = [...passList, host];
    await invoke("pass_through_set", { hosts: next });
    setPassList(next);
    setPassInput("");
  }

  async function removePass(host: string) {
    const next = passList.filter((h) => h !== host);
    await invoke("pass_through_set", { hosts: next });
    setPassList(next);
  }

  async function addListener() {
    try {
      await invoke("listener_add", { host: listenForm.host, port: listenForm.port });
      setListeners(await invoke<ProxyListenerSlot[]>("listener_list"));
      setListenForm({ ...listenForm, port: listenForm.port + 1 });
    } catch (e) {
      setToast(String(e));
    }
  }

  async function removeListener(id: number) {
    await invoke("listener_remove", { id });
    setListeners(await invoke<ProxyListenerSlot[]>("listener_list"));
  }

  async function addMacro() {
    const requests = macroForm.requests.split("\n").map((s) => s.trim()).filter(Boolean);
    if (!macroForm.name || requests.length === 0) return;
    try {
      await invoke("session_macro_add", {
        input: { name: macroForm.name, requests },
      });
      setMacros(await invoke<SessionMacro[]>("session_macro_list"));
      setMacroForm({ name: "", requests: "" });
    } catch (e) {
      setToast(String(e));
    }
  }

  async function deleteMacro(id: number) {
    await invoke("session_macro_delete", { id });
    setMacros(await invoke<SessionMacro[]>("session_macro_list"));
  }

  async function addSessRule() {
    if (!ruleForm.name) return;
    const updates: SessionUpdate[] = [];
    try {
      await invoke("session_rule_add", {
        input: {
          name: ruleForm.name,
          tool: ruleForm.tool,
          match_host: ruleForm.match_host,
          macro_id: ruleForm.macro_id ? Number(ruleForm.macro_id) : null,
          updates,
        },
      });
      setSessRules(await invoke<SessionRule[]>("session_rule_list"));
      setRuleForm({ name: "", tool: "any", match_host: "", macro_id: "" });
    } catch (e) {
      setToast(String(e));
    }
  }

  async function deleteSessRule(id: number) {
    await invoke("session_rule_delete", { id });
    setSessRules(await invoke<SessionRule[]>("session_rule_list"));
  }

  const SUBS: [Sub, string, string?][] = [
    ["intercept", "Intercept"],
    ["history", "HTTP history"],
    ["ws", "WebSockets history"],
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
            <button
              className="btn"
              aria-haspopup="menu"
              aria-expanded={menuOpen}
              aria-label="Action"
              title="Action — follow-up options for the held message"
              onClick={actionMenu}
            >
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
                        <td colSpan={11} className="muted empty-cell">
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
        <div className="panel grow min-h-0">
          <SplitPane direction="column" initial={42} min={20} max={80}>
            <>
              <div
                className="table-wrap"
                role="region"
                aria-label="WebSocket connections"
                tabIndex={0}
              >
                <table className="history">
                  <thead>
                    <tr>
                      <th>#</th>
                      <th>Started</th>
                      <th>Host</th>
                      <th>URL</th>
                      <th>TLS</th>
                      <th>Sent</th>
                      <th>Received</th>
                      <th>State</th>
                    </tr>
                  </thead>
                  <tbody>
                    {wsConns.map((c) => (
                      <tr
                        key={c.id}
                        className={wsSel?.id === c.id ? "selected" : ""}
                        tabIndex={0}
                        aria-selected={wsSel?.id === c.id}
                        onClick={() => setWsSel(c)}
                        onKeyDown={(e) => {
                          if (e.key === "Enter" || e.key === " ") {
                            e.preventDefault();
                            setWsSel(c);
                          }
                        }}
                      >
                        <td>{c.id}</td>
                        <td>{timeOf(c.ts_start)}</td>
                        <td title={c.host}>{c.host}</td>
                        <td title={c.url}>{c.url}</td>
                        <td>{c.tls ? "TLS" : ""}</td>
                        <td>{c.msgs_out}</td>
                        <td>{c.msgs_in}</td>
                        <td>
                          {c.ts_end ? (
                            <span className="muted">closed</span>
                          ) : (
                            <span className="m-post">open</span>
                          )}
                        </td>
                      </tr>
                    ))}
                    {wsConns.length === 0 && (
                      <tr>
                        <td colSpan={8} className="muted empty-cell">
                          No WebSocket connections captured yet. Browse to a site that uses
                          WebSockets, then refresh.
                        </td>
                      </tr>
                    )}
                  </tbody>
                </table>
              </div>
              <div className="filterbar">
                <span className="muted">{wsConns.length} connection(s)</span>
                <span className="grow" />
                <button className="btn" onClick={loadWs} title="Reload connections">
                  Refresh
                </button>
              </div>
            </>
            <>
              {wsSel ? (
                <SplitPane direction="row" initial={55} min={20} max={80}>
                  <div className="pane-flush">
                    <div className="pane-caption">
                      Frames
                      <span className="grow" />
                      <button
                        className={`btn mini ${wsShowHs ? "primary" : ""}`}
                        aria-pressed={wsShowHs}
                        onClick={() => setWsShowHs((v) => !v)}
                        title="Show the reconstructed HTTP upgrade handshake"
                      >
                        Handshake
                      </button>
                      <span className="muted">
                        {wsFrames.length >= 500 ? "newest 500 frames" : `${wsFrames.length} frames`}
                      </span>
                    </div>
                    <div
                      className="grow min-h-0 table-wrap"
                      role="region"
                      aria-label="WebSocket frames"
                      tabIndex={0}
                    >
                      <table className="history">
                        <thead>
                          <tr>
                            <th>#</th>
                            <th>Time</th>
                            <th>Dir</th>
                            <th>Type</th>
                            <th>Length</th>
                            <th>Payload</th>
                          </tr>
                        </thead>
                        <tbody>
                          {wsFrames.map((f) => (
                            <tr
                              key={f.id}
                              className={!wsShowHs && wsFrameSel?.id === f.id ? "selected" : ""}
                              tabIndex={0}
                              aria-selected={!wsShowHs && wsFrameSel?.id === f.id}
                              onClick={() => {
                                setWsShowHs(false);
                                setWsFrameSel(f);
                              }}
                              onKeyDown={(e) => {
                                if (e.key === "Enter" || e.key === " ") {
                                  e.preventDefault();
                                  setWsShowHs(false);
                                  setWsFrameSel(f);
                                }
                              }}
                            >
                              <td>{f.id}</td>
                              <td>{timeOf(f.ts)}</td>
                              <td className={f.dir === "c" ? "m-post" : "m-get"}>
                                {wsDirLabel(f.dir)}
                              </td>
                              <td>{wsOpcodeName(f.opcode)}</td>
                              <td>{f.len}</td>
                              <td className="mono" title={f.payload}>
                                {f.payload.length > 80 ? `${f.payload.slice(0, 80)}…` : f.payload}
                              </td>
                            </tr>
                          ))}
                          {wsFrames.length === 0 && (
                            <tr>
                              <td colSpan={6} className="muted empty-cell">
                                No frames captured on this connection yet.
                              </td>
                            </tr>
                          )}
                        </tbody>
                      </table>
                    </div>
                  </div>
                  <div className="pane-flush">
                    <div className="pane-caption">
                      {wsShowHs ? "Handshake (reconstructed)" : "Message"}
                    </div>
                    <div className="grow min-h-0">
                      {wsShowHs ? (
                        <MessageView value={wsHandshake(wsSel)} />
                      ) : wsFrameSel ? (
                        <MessageView value={wsFrameText(wsFrameSel)} />
                      ) : (
                        <div className="empty">Select a frame to inspect it.</div>
                      )}
                    </div>
                  </div>
                </SplitPane>
              ) : (
                <div className="empty">Select a WebSocket connection to inspect its frames.</div>
              )}
            </>
          </SplitPane>
        </div>
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
            <h2>Upstream proxies</h2>
            <p className="muted">
              Route traffic through a chaining proxy. First matching host pattern wins
              (use * as a wildcard prefix, e.g. *.corp).
            </p>
            <table className="history" style={{ marginBottom: 12 }}>
              <thead>
                <tr>
                  <th>Host pattern</th>
                  <th>Proxy</th>
                  <th>TLS</th>
                  <th aria-label="Actions">
                    <span className="sr-only">Actions</span>
                  </th>
                </tr>
              </thead>
              <tbody>
                {upstreams.map((r) => (
                  <tr key={r.id}>
                    <td className="mono">{r.host_pattern}</td>
                    <td className="mono">
                      {r.proxy_host}:{r.proxy_port}
                    </td>
                    <td>{r.use_tls ? "TLS" : ""}</td>
                    <td>
                      <button className="btn" onClick={() => deleteUpstream(r.id)}>
                        Delete
                      </button>
                    </td>
                  </tr>
                ))}
                {upstreams.length === 0 && (
                  <tr>
                    <td colSpan={4} className="muted">
                      No upstream proxies — traffic connects directly.
                    </td>
                  </tr>
                )}
              </tbody>
            </table>
            <div className="row" style={{ flexWrap: "wrap" }}>
              <input
                className="input"
                placeholder="Host pattern (*.corp)"
                value={upForm.host_pattern}
                onChange={(e) => setUpForm({ ...upForm, host_pattern: e.target.value })}
                aria-label="Upstream host pattern"
              />
              <input
                className="input"
                placeholder="Proxy host"
                value={upForm.proxy_host}
                onChange={(e) => setUpForm({ ...upForm, proxy_host: e.target.value })}
                aria-label="Upstream proxy host"
              />
              <input
                className="input"
                style={{ width: 90 }}
                type="number"
                value={upForm.proxy_port}
                onChange={(e) => setUpForm({ ...upForm, proxy_port: Number(e.target.value) || 0 })}
                aria-label="Upstream proxy port"
              />
              <label className="switch">
                <input
                  type="checkbox"
                  checked={upForm.use_tls}
                  onChange={(e) => setUpForm({ ...upForm, use_tls: e.target.checked })}
                />
                TLS to proxy
              </label>
              <button
                className="btn primary"
                onClick={addUpstream}
                disabled={!upForm.host_pattern || !upForm.proxy_host}
              >
                Add upstream
              </button>
            </div>
          </section>

          <section>
            <h2>TLS pass-through</h2>
            <p className="muted">
              Hosts listed here are not decrypted — the proxy opens a raw tunnel instead of
              MITMing with its CA. Use for certificate-pinned targets.
            </p>
            <div className="row" style={{ flexWrap: "wrap", marginBottom: 8 }}>
              {passList.map((h) => (
                <span key={h} className="chip">
                  {h}
                  <button
                    className="chip-x"
                    aria-label={`Remove ${h}`}
                    onClick={() => removePass(h)}
                  >
                    ×
                  </button>
                </span>
              ))}
              {passList.length === 0 && <span className="muted">All hosts are decrypted (MITM).</span>}
            </div>
            <div className="row">
              <input
                className="input"
                placeholder="Host or *.domain"
                value={passInput}
                onChange={(e) => setPassInput(e.target.value)}
                onKeyDown={(e) => e.key === "Enter" && addPass()}
                aria-label="Pass-through host"
              />
              <button className="btn primary" onClick={addPass} disabled={!passInput.trim()}>
                Add
              </button>
            </div>
          </section>

          <section>
            <h2>Extra listeners</h2>
            <p className="muted">
              Additional proxy ports sharing all options (intercept, scope, match/replace).
            </p>
            <table className="history" style={{ marginBottom: 12 }}>
              <thead>
                <tr>
                  <th>Interface</th>
                  <th>Port</th>
                  <th aria-label="Actions">
                    <span className="sr-only">Actions</span>
                  </th>
                </tr>
              </thead>
              <tbody>
                {listeners.map((l) => (
                  <tr key={l.id}>
                    <td className="mono">{l.host}</td>
                    <td className="mono">{l.port}</td>
                    <td>
                      <button className="btn" onClick={() => removeListener(l.id)}>
                        Stop
                      </button>
                    </td>
                  </tr>
                ))}
                {listeners.length === 0 && (
                  <tr>
                    <td colSpan={3} className="muted">
                      No extra listeners.
                    </td>
                  </tr>
                )}
              </tbody>
            </table>
            <div className="row">
              <input
                className="input"
                style={{ width: 140 }}
                value={listenForm.host}
                onChange={(e) => setListenForm({ ...listenForm, host: e.target.value })}
                aria-label="Listener host"
              />
              <input
                className="input"
                style={{ width: 90 }}
                type="number"
                value={listenForm.port}
                onChange={(e) => setListenForm({ ...listenForm, port: Number(e.target.value) || 0 })}
                aria-label="Listener port"
              />
              <button className="btn primary" onClick={addListener} disabled={!listenForm.port}>
                Start listener
              </button>
            </div>
          </section>

          <section>
            <h2>Session handling</h2>
            <p className="muted">
              Rules auto-update requests before they leave the proxy (e.g. refresh a session
              token via a macro, then set the Cookie header).
            </p>

            <h3 style={{ margin: "12px 0 6px" }}>Macros</h3>
            <table className="history" style={{ marginBottom: 12 }}>
              <thead>
                <tr>
                  <th>Name</th>
                  <th>Requests</th>
                  <th aria-label="Actions">
                    <span className="sr-only">Actions</span>
                  </th>
                </tr>
              </thead>
              <tbody>
                {macros.map((m) => (
                  <tr key={m.id}>
                    <td>{m.name}</td>
                    <td>{m.requests.length}</td>
                    <td>
                      <button className="btn" onClick={() => deleteMacro(m.id)}>
                        Delete
                      </button>
                    </td>
                  </tr>
                ))}
                {macros.length === 0 && (
                  <tr>
                    <td colSpan={3} className="muted">
                      No macros.
                    </td>
                  </tr>
                )}
              </tbody>
            </table>
            <div className="row" style={{ flexWrap: "wrap", marginBottom: 16 }}>
              <input
                className="input"
                placeholder="Macro name"
                value={macroForm.name}
                onChange={(e) => setMacroForm({ ...macroForm, name: e.target.value })}
                aria-label="Macro name"
              />
              <input
                className="input grow"
                placeholder="Raw requests, one per line (GET /login HTTP/1.1 …)"
                value={macroForm.requests}
                onChange={(e) => setMacroForm({ ...macroForm, requests: e.target.value })}
                aria-label="Macro requests"
              />
              <button
                className="btn primary"
                onClick={addMacro}
                disabled={!macroForm.name || !macroForm.requests.trim()}
              >
                Add macro
              </button>
            </div>

            <h3 style={{ margin: "12px 0 6px" }}>Rules</h3>
            <table className="history" style={{ marginBottom: 12 }}>
              <thead>
                <tr>
                  <th>#</th>
                  <th>Name</th>
                  <th>Tool</th>
                  <th>Host</th>
                  <th>Macro</th>
                  <th aria-label="Actions">
                    <span className="sr-only">Actions</span>
                  </th>
                </tr>
              </thead>
              <tbody>
                {sessRules.map((r) => (
                  <tr key={r.id}>
                    <td>{r.position}</td>
                    <td>{r.name}</td>
                    <td>{r.tool}</td>
                    <td className="mono">{r.match_host || "*"}</td>
                    <td>
                      {r.macro_id
                        ? macros.find((m) => m.id === r.macro_id)?.name ?? r.macro_id
                        : "—"}
                    </td>
                    <td>
                      <button className="btn" onClick={() => deleteSessRule(r.id)}>
                        Delete
                      </button>
                    </td>
                  </tr>
                ))}
                {sessRules.length === 0 && (
                  <tr>
                    <td colSpan={6} className="muted">
                      No session rules.
                    </td>
                  </tr>
                )}
              </tbody>
            </table>
            <div className="row" style={{ flexWrap: "wrap" }}>
              <input
                className="input"
                placeholder="Rule name"
                value={ruleForm.name}
                onChange={(e) => setRuleForm({ ...ruleForm, name: e.target.value })}
                aria-label="Session rule name"
              />
              <select
                className="select"
                value={ruleForm.tool}
                onChange={(e) => setRuleForm({ ...ruleForm, tool: e.target.value })}
                aria-label="Rule tool"
              >
                <option value="any">Any tool</option>
                <option value="proxy">Proxy</option>
                <option value="repeater">Repeater</option>
                <option value="intruder">Intruder</option>
                <option value="scanner">Scanner</option>
              </select>
              <input
                className="input"
                placeholder="Host pattern (blank = any)"
                value={ruleForm.match_host}
                onChange={(e) => setRuleForm({ ...ruleForm, match_host: e.target.value })}
                aria-label="Rule host pattern"
              />
              <select
                className="select"
                value={ruleForm.macro_id}
                onChange={(e) => setRuleForm({ ...ruleForm, macro_id: e.target.value })}
                aria-label="Rule macro"
              >
                <option value="">No macro</option>
                {macros.map((m) => (
                  <option key={m.id} value={String(m.id)}>
                    {m.name}
                  </option>
                ))}
              </select>
              <button className="btn primary" onClick={addSessRule} disabled={!ruleForm.name}>
                Add rule
              </button>
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
