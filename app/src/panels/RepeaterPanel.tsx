import { useEffect, useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { ChevronLeft, ChevronRight, HelpCircle, Pencil, Settings } from "lucide-react";
import { ExtHookInfo, ExtMenuItem, RepeaterHistoryEntry, RepeaterTab, Transfer } from "../types";
import MessageView from "../components/MessageView";
import SplitPane from "../components/SplitPane";
import { useContextMenu } from "../components/ContextMenu";

type Props = {
  incoming: Transfer | null;
  setToast: (t: string) => void;
};

type Draft = {
  id: number | null;
  name: string;
  req: string;
  resp: string;
  status: number | null;
  timeMs: number | null;
  finalUrl: string | null;
  redirectCount: number;
};

const NEW_DRAFT: Draft = {
  id: null,
  name: "1",
  req: "",
  resp: "",
  status: null,
  timeMs: null,
  finalUrl: null,
  redirectCount: 0,
};

export default function RepeaterPanel({ incoming, setToast }: Props) {
  const [tabs, setTabs] = useState<RepeaterTab[]>([]);
  const [draft, setDraft] = useState<Draft>({
    ...NEW_DRAFT,
    req:
      "GET http://example.com/ HTTP/1.1\r\nHost: example.com\r\nUser-Agent: WebProxy-Toolkit/0.1\r\nAccept: */*\r\nConnection: close\r\n\r\n",
  });
  const [busy, setBusy] = useState(false);
  const [stacked, setStacked] = useState(false);
  const [urlHint, setUrlHint] = useState("http://example.com/");
  const [editingTarget, setEditingTarget] = useState(false);
  const [railOpen, setRailOpen] = useState(true);
  const [follow, setFollow] = useState(false);
  const [hist, setHist] = useState<RepeaterHistoryEntry[]>([]);
  const [histIdx, setHistIdx] = useState<number | null>(null);
  const [extMenus, setExtMenus] = useState<ExtMenuItem[]>([]);
  const { open: openCtx, close: closeCtx, node: ctxNode, isOpen: menuOpen } = useContextMenu();

  useEffect(() => {
    invoke<ExtHookInfo>("extension_hooks")
      .then((info) => setExtMenus(info.context_menus))
      .catch(() => setExtMenus([]));
  }, []);

  async function loadHistory(tabId: number | null) {
    if (tabId == null) {
      setHist([]);
      setHistIdx(null);
      return;
    }
    try {
      const entries = await invoke<RepeaterHistoryEntry[]>("repeater_history_list", {
        tabId,
        limit: 50,
      });
      setHist(entries);
      setHistIdx((prev) => (prev != null && prev < entries.length ? prev : null));
    } catch (e) {
      setToast(String(e));
    }
  }

  useEffect(() => {
    loadHistory(draft.id ?? null);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [draft.id]);

  async function loadTabs() {
    try {
      setTabs(await invoke<RepeaterTab[]>("repeater_tabs_list"));
    } catch (e) {
      setToast(String(e));
    }
  }

  useEffect(() => {
    loadTabs();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  useEffect(() => {
    if (!incoming || incoming.seq === 0) return;
    setHistIdx(null);
    setDraft({ ...NEW_DRAFT, name: nextName(tabs), req: incoming.raw });
    setUrlHint(incoming.url);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [incoming?.seq]);

  function nextName(list: RepeaterTab[]): string {
    const nums = list
      .map((t) => parseInt(t.name, 10))
      .filter((n) => !Number.isNaN(n));
    return String(nums.length ? Math.max(...nums) + 1 : list.length + 1);
  }

  function openTab(t: RepeaterTab) {
    setHistIdx(null);
    setDraft({
      id: t.id,
      name: t.name,
      req: t.req_raw,
      resp: t.last_resp_raw ?? "",
      status: t.last_status,
      timeMs: t.last_time_ms,
      finalUrl: null,
      redirectCount: 0,
    });
  }

  async function saveTab() {
    try {
      const id = await invoke<number | null>("repeater_tab_save", {
        id: draft.id,
        name: draft.name || "1",
        reqRaw: draft.req,
        lastStatus: draft.status,
        lastTimeMs: draft.timeMs,
        lastRespRaw: draft.resp || null,
      });
      setDraft((d) => ({ ...d, id }));
      await loadTabs();
      setToast("Tab saved");
    } catch (e) {
      setToast(String(e));
    }
  }

  async function closeTab() {
    if (draft.id != null) {
      await invoke("repeater_tab_delete", { id: draft.id });
      await loadTabs();
    }
    setHistIdx(null);
    setDraft({ ...NEW_DRAFT, name: nextName(tabs), req: draft.req });
  }

  async function send() {
    setBusy(true);
    try {
      const r = await invoke<{
        status: number;
        headers: [string, string][];
        body: string;
        time_ms: number;
        redirects?: number;
        final_url?: string;
      }>("repeater_send", { raw: draft.req, urlHint: urlHint || null, follow });
      const resp =
        `HTTP/1.1 ${r.status}\n` +
        r.headers.map(([k, v]) => `${k}: ${v}`).join("\n") +
        "\n\n" +
        r.body;

      // A response only has somewhere to live once the tab exists — create it
      // with the response attached so the first send costs one write, not two.
      const isNew = draft.id == null;
      const tabId =
        draft.id ??
        (await invoke<number | null>("repeater_tab_save", {
          id: null,
          name: draft.name,
          reqRaw: draft.req,
          lastStatus: null,
          lastTimeMs: null,
          lastRespRaw: null,
        })) ??
        null;
      if (tabId == null) throw new Error("Could not create a Repeater tab for this request");

      await invoke("repeater_history_add", {
        tabId,
        reqRaw: draft.req,
        respRaw: resp,
        status: r.status,
        timeMs: r.time_ms,
      });
      await invoke("repeater_tab_save", {
        id: tabId,
        name: draft.name,
        reqRaw: draft.req,
        lastStatus: r.status,
        lastTimeMs: r.time_ms,
        lastRespRaw: resp,
      });

      setHistIdx(null);
      setDraft((d) => ({
        ...d,
        id: tabId,
        resp,
        status: r.status,
        timeMs: r.time_ms,
        finalUrl: r.final_url ?? null,
        redirectCount: r.redirects ?? 0,
      }));
      await Promise.all([loadTabs(), loadHistory(tabId)]);
      if (r.status >= 300 && r.status < 400 && !follow) {
        setToast(`Stopped at ${r.status} — enable Follow redirects to follow it`);
      } else if (isNew) {
        setToast("New Repeater tab created for this request");
      }
    } catch (e) {
      setDraft((d) => ({ ...d, resp: `Error: ${e}`, finalUrl: null, redirectCount: 0 }));
    } finally {
      setBusy(false);
    }
  }

  function showEntry(i: number) {
    const entry = hist[i];
    if (!entry) return;
    setHistIdx(i);
    setDraft((d) => ({
      ...d,
      req: entry.req_raw,
      resp: entry.resp_raw ?? "",
      status: entry.status,
      timeMs: entry.time_ms,
    }));
  }

  function goOlder() {
    // hist is newest-first and a null index means "live view of the newest",
    // so the first step back lands on the entry before it.
    const next = histIdx == null ? 1 : histIdx + 1;
    if (next < hist.length) showEntry(next);
  }

  function goNewer() {
    if (histIdx == null || histIdx === 0) return;
    showEntry(histIdx - 1);
  }

  const inspector = useMemo(() => parseRequest(draft.req), [draft.req]);

  function customActionsMenu(e: React.MouseEvent<HTMLButtonElement>) {
    if (menuOpen) {
      closeCtx();
      return;
    }
    if (extMenus.length === 0) {
      setToast("No extension actions registered");
      return;
    }
    const parsed = parseRequest(draft.req);
    const lines = draft.req.split(/\r?\n/);
    let bi = 1;
    for (; bi < lines.length; bi++) {
      if (lines[bi] === "") break;
    }
    const body = lines.slice(bi + 1).join("\n");
    let base = urlHint || "";
    if (base && !base.endsWith("/")) base += "/";
    const url = /^https?:\/\//.test(parsed.target)
      ? parsed.target
      : base + parsed.target.replace(/^\//, "");
    const message = {
      request: {
        method: parsed.method,
        url,
        headers: parsed.headers,
        body,
      },
      response: null,
    };
    const rect = e.currentTarget.getBoundingClientRect();
    openCtx(
      e,
      [
        { label: "Extensions", disabled: true },
        ...extMenus.map((m) => ({
          label: m.label,
          onClick: () =>
            invoke("extension_context_invoke", {
              extId: m.ext_id,
              menuId: m.menu_id,
              message,
            })
              .then(() => setToast(`Ran extension action: ${m.label}`))
              .catch((err) => setToast(String(err))),
        })),
      ],
      { x: rect.left, y: rect.bottom + 4 }
    );
  }

  return (
    <div className="panel" role="tabpanel" aria-label="Repeater" id="panel-repeater">
      <div className="tabstrip">
        <div className="rtab-list" role="tablist" aria-label="Repeater tabs">
          {tabs.map((t) => (
            <button
              key={t.id}
              role="tab"
              aria-selected={draft.id === t.id}
              className={`rtab ${draft.id === t.id ? "active" : ""}`}
              onClick={() => openTab(t)}
              title={`Tab ${t.name}${t.last_status ? ` — last status ${t.last_status}` : ""}`}
            >
              {t.name}
              {t.last_status ? <span className="rtab-status">{t.last_status}</span> : null}
            </button>
          ))}
        </div>
        <button
          className="rtab plus"
          title="New tab"
          onClick={() => setDraft({ ...NEW_DRAFT, name: nextName(tabs), req: draft.req, resp: "" })}
        >
          +
        </button>
      </div>

      <div className="toolbar">
        <button className="btn primary send" onClick={send} disabled={busy}>
          {busy ? "Sending…" : "Send"}
        </button>
        <button className="icon-btn" title="Request settings" disabled>
          <Settings size={15} />
        </button>
        <button className="btn" disabled title="No request in flight">
          Cancel
        </button>
        <button
          className="icon-btn"
          title={histIdx == null ? "Previous request in this tab (newest shown)" : "Older request in this tab"}
          disabled={hist.length < 2 || (histIdx != null && histIdx + 1 >= hist.length)}
          onClick={goOlder}
        >
          <ChevronLeft size={15} />
        </button>
        <button
          className="icon-btn"
          title="Newer request in this tab"
          disabled={histIdx == null || histIdx === 0}
          onClick={goNewer}
        >
          <ChevronRight size={15} />
        </button>
        <span className="muted" title="Sent requests stored for this tab">
          {hist.length === 0 ? "History: empty" : `History: ${hist.length} sent`}
          {histIdx != null ? ` · ${hist.length - histIdx}/${hist.length}` : ""}
        </span>
        <label className="switch" title="Follow 3xx responses automatically (max 5 hops)">
          <input
            type="checkbox"
            checked={follow}
            onChange={(e) => setFollow(e.target.checked)}
          />
          Follow redirects
        </label>
        <button
          className="btn"
          onClick={() => setStacked(!stacked)}
          title="Toggle layout"
        >
          {stacked ? "Side by side" : "Stacked"}
        </button>
        <button className="btn" onClick={saveTab}>
          Save
        </button>
        <button className="btn" onClick={closeTab} title="Close tab">
          Close
        </button>
        <span className="grow" />
        <span className="target-pill">
          Target:{" "}
          {editingTarget ? (
            <input
              className="input"
              style={{ width: 300 }}
              autoFocus
              defaultValue={urlHint}
              placeholder="scheme://host[:port]"
              aria-label="Target URL"
              onBlur={(e) => {
                setUrlHint(e.target.value);
                setEditingTarget(false);
              }}
              onKeyDown={(e) => {
                if (e.key === "Enter") (e.target as HTMLInputElement).blur();
                if (e.key === "Escape") setEditingTarget(false);
              }}
            />
          ) : (
            <b>{urlHint || "Not specified"}</b>
          )}
          <button
            className="icon-btn"
            title="Edit target"
            onClick={() => setEditingTarget(true)}
          >
            <Pencil size={13} />
          </button>
        </span>
        <button
          className="icon-btn"
          title="Help"
          onClick={() => setToast("Repeater replays an edited request through the proxy")}
        >
          <HelpCircle size={15} />
        </button>
      </div>

      <div className="with-rail">
        <div className="rail-main">
          <SplitPane direction={stacked ? "column" : "row"} initial={50} min={20} max={80}>
            <div className="pane-flush">
              <MessageView
                value={draft.req}
                onChange={(v) => setDraft({ ...draft, req: v })}
                title="Request"
              />
            </div>
            <div className="pane-flush">
              <MessageView
                value={draft.resp}
                title="Response"
                extra={
                  <span className="row" style={{ gap: 6 }}>
                    {draft.status != null && <span className="chip">{draft.status}</span>}
                    {draft.timeMs != null && <span className="chip">{draft.timeMs} ms</span>}
                    {draft.status != null &&
                      draft.status >= 300 &&
                      draft.status < 400 &&
                      draft.redirectCount === 0 && (
                        <span className="chip" title="Redirect — enable Follow redirects to follow it">
                          redirect
                        </span>
                      )}
                    {draft.redirectCount > 0 && (
                      <span className="chip" title={draft.finalUrl ?? ""}>
                        ↻ {draft.redirectCount} hop{draft.redirectCount === 1 ? "" : "s"}
                      </span>
                    )}
                    {histIdx != null && (
                      <span className="chip" title="Historical send for this tab">
                        history
                      </span>
                    )}
                  </span>
                }
              />
            </div>
          </SplitPane>
        </div>

        {railOpen && (
          <aside className="rail-panel">
            <div className="rail-panel-head">
              Inspector
              <span className="grow" />
              <button className="icon-btn" title="Close inspector" onClick={() => setRailOpen(false)}>
                ✕
              </button>
            </div>
            <div className="rail-panel-body">
              <section className="insp-section">
                <h2>Request line</h2>
                <table className="kv">
                  <tbody>
                    <tr>
                      <td>Method</td>
                      <td>{inspector.method || "—"}</td>
                    </tr>
                    <tr>
                      <td>Target</td>
                      <td>{inspector.target || "—"}</td>
                    </tr>
                    <tr>
                      <td>Version</td>
                      <td>{inspector.version || "—"}</td>
                    </tr>
                  </tbody>
                </table>
              </section>

              <section className="insp-section">
                <h2>Headers ({inspector.headers.length})</h2>
                {inspector.headers.length === 0 ? (
                  <p className="muted" style={{ fontSize: 11 }}>
                    No headers.
                  </p>
                ) : (
                  <table className="kv">
                    <tbody>
                      {inspector.headers.map(([k, v], i) => (
                        <tr key={i}>
                          <td>{k}</td>
                          <td>{v}</td>
                        </tr>
                      ))}
                    </tbody>
                  </table>
                )}
              </section>

              <section className="insp-section">
                <h2>Parameters ({inspector.params.length})</h2>
                {inspector.params.length === 0 ? (
                  <p className="muted" style={{ fontSize: 11 }}>
                    No query or body parameters detected.
                  </p>
                ) : (
                  <table className="kv">
                    <tbody>
                      {inspector.params.map(([k, v], i) => (
                        <tr key={i}>
                          <td>{k}</td>
                          <td>{v}</td>
                        </tr>
                      ))}
                    </tbody>
                  </table>
                )}
              </section>

              <section className="insp-section">
                <h2>Body</h2>
                <p className="muted" style={{ fontSize: 11, margin: 0 }}>
                  {inspector.bodyLen} bytes
                  {inspector.bodyPreview ? (
                    <>
                      <br />
                      <span className="mono">{inspector.bodyPreview}</span>
                    </>
                  ) : (
                    " (empty)"
                  )}
                </p>
              </section>
            </div>
          </aside>
        )}

        <nav className="vrail" aria-label="Side panels">
          <button className={railOpen ? "active" : ""} onClick={() => setRailOpen(!railOpen)}>
            Inspector
          </button>
          <button disabled title="Notes arrive in Phase 2">
            Notes
          </button>
          <button
            title={
              extMenus.length === 0
                ? "No extension actions registered"
                : "Run extension context-menu actions on the current request"
            }
            onClick={customActionsMenu}
          >
            Custom actions
          </button>
        </nav>
      </div>
      {ctxNode}
    </div>
  );
}

type Parsed = {
  method: string;
  target: string;
  version: string;
  headers: [string, string][];
  params: [string, string][];
  bodyLen: number;
  bodyPreview: string;
};

function parseRequest(raw: string): Parsed {
  const lines = raw.split(/\r?\n/);
  const first = (lines[0] ?? "").split(" ");
  const headers: [string, string][] = [];
  let i = 1;
  for (; i < lines.length; i++) {
    if (lines[i] === "") break;
    const c = lines[i].indexOf(":");
    if (c > 0) headers.push([lines[i].slice(0, c), lines[i].slice(c + 1).trim()]);
    else break;
  }
  const body = lines.slice(i + 1).join("\n");
  const target = first[1] ?? "";

  const params: [string, string][] = [];
  const qs = target.includes("?") ? target.slice(target.indexOf("?") + 1).split("#")[0] : "";
  params.push(...parseUrlEncoded(qs));
  const ct = headers.find(([k]) => k.toLowerCase() === "content-type")?.[1] ?? "";
  if (ct.includes("x-www-form-urlencoded")) params.push(...parseUrlEncoded(body));
  const cookie = headers.find(([k]) => k.toLowerCase() === "cookie")?.[1];
  if (cookie)
    for (const part of cookie.split(";")) {
      const c = part.indexOf("=");
      if (c > 0) params.push([part.slice(0, c).trim(), part.slice(c + 1).trim()]);
    }

  return {
    method: first[0] ?? "",
    target,
    version: first[2] ?? "",
    headers,
    params,
    bodyLen: new TextEncoder().encode(body).length,
    bodyPreview: body.slice(0, 400),
  };
}

function parseUrlEncoded(s: string): [string, string][] {
  if (!s) return [];
  const out: [string, string][] = [];
  for (const pair of s.split("&")) {
    if (!pair) continue;
    const c = pair.indexOf("=");
    try {
      if (c === -1) out.push([decodeURIComponent(pair), ""]);
      else out.push([decodeURIComponent(pair.slice(0, c)), decodeURIComponent(pair.slice(c + 1))]);
    } catch {
      out.push([pair, ""]);
    }
  }
  return out;
}
