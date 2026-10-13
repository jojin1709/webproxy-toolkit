import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import MessageView from "../components/MessageView";
import SplitPane from "../components/SplitPane";
import {
  IntruderAttack,
  IntruderCampaignSummary,
  IntruderProcessor,
  IntruderResultFull,
  IntruderResultRow,
  IntruderRunState,
  Transfer,
  statusClass,
} from "../types";
import {
  ATTACKS,
  MARK,
  PROCESSORS,
  countPositions,
  estimateTotal,
  intruderApi,
  parseCombo,
  parseJsonArray,
  parseJsonObject,
  payloadCount,
  positionTexts,
  processorDef,
  stripMarkers,
  wrapSelection,
} from "../lib/intruder";

type Props = {
  incoming: Transfer | null;
  setToast: (t: string) => void;
};

type SubTab = "positions" | "payloads" | "extractors" | "results";

type Slot = {
  name: string;
  kind: "list" | "numbers";
  items: string;
  start: number | null;
  end: number | null;
  step: number | null;
  processors: { kind: string; a: string; b: string }[];
};

type Draft = {
  id: number;
  name: string;
  attackType: IntruderAttack;
  reqRaw: string;
  urlHint: string;
  follow: boolean;
  setMap: number[];
  threads: number;
  delayMs: number;
  maxRequests: number;
  extractors: { name: string; pattern: string }[];
};

const DEFAULT_REQ =
  "GET http://shop.example.com/ HTTP/1.1\r\n" +
  "Host: shop.example.com\r\n" +
  "User-Agent: Mozilla/5.0 (Windows NT 10.0; Win64; x64)\r\n" +
  "Accept: */*\r\n" +
  "Connection: close\r\n\r\n";

const emptySlot = (n: number): Slot => ({
  name: `Set ${n}`,
  kind: "list",
  items: "",
  start: null,
  end: null,
  step: 1,
  processors: [],
});

const STATUS_LABEL: Record<string, string> = {
  draft: "Draft",
  running: "Running",
  paused: "Paused",
  done: "Done",
  stopped: "Stopped",
  error: "Error",
};

function statusChipClass(status: string): string {
  if (status === "running" || status === "done") return "chip ok";
  if (status === "error") return "chip err";
  if (status === "paused" || status === "stopped") return "chip warn";
  return "chip";
}

export default function IntruderPanel({ incoming, setToast }: Props) {
  const [campaigns, setCampaigns] = useState<IntruderCampaignSummary[]>([]);
  const [activeId, setActiveId] = useState<number | null>(null);
  const [draft, setDraft] = useState<Draft | null>(null);
  const [slots, setSlots] = useState<Slot[]>([emptySlot(1)]);
  const [selSlot, setSelSlot] = useState(0);
  const [subtab, setSubtab] = useState<SubTab>("positions");
  const [run, setRun] = useState<IntruderRunState | null>(null);
  const [rows, setRows] = useState<IntruderResultRow[]>([]);
  const [total, setTotal] = useState(0);
  const [selSeq, setSelSeq] = useState<number | null>(null);
  const [detail, setDetail] = useState<IntruderResultFull | null>(null);
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState("");
  const [confirmDel, setConfirmDel] = useState(false);
  const editorRef = useRef<HTMLTextAreaElement | null>(null);
  const activeIdRef = useRef<number | null>(null);
  activeIdRef.current = activeId;

  const running = !!run && (run.status === "running" || run.status === "paused");
  const status = running ? run.status : (run?.status ?? campaigns.find((c) => c.id === activeId)?.status ?? "draft");

  const loadList = useCallback(async () => {
    try {
      const list = await intruderApi.list();
      setCampaigns(Array.isArray(list) ? list : []);
    } catch (e) {
      setErr(String(e));
    }
  }, []);

  const loadResults = useCallback(async (cid: number) => {
    try {
      const got = await intruderApi.results(cid, 0, 500);
      setRows(Array.isArray(got?.rows) ? got.rows : []);
      setTotal(typeof got?.total === "number" ? got.total : 0);
    } catch {
      /* results may not be readable mid-write */
    }
  }, []);

  const openCampaign = useCallback(
    async (cid: number) => {
      setBusy(true);
      setErr("");
      try {
        const d = await intruderApi.get(cid);
        const c = d.campaign;
        const loaded: Slot[] = (d.payloads ?? [])
          .slice()
          .sort((a, b) => a.set_index - b.set_index)
          .map((p) => ({
            name: p.name,
            kind: p.kind === "numbers" ? "numbers" : "list",
            items: p.items,
            start: p.start_i,
            end: p.end_i,
            step: p.step_i,
            processors: parseJsonArray<IntruderProcessor>(p.processors, []).map((x) => ({
              kind: x.kind,
              a: x.a ?? "",
              b: x.b ?? "",
            })),
          }));
        setActiveId(cid);
        setDraft({
          id: c.id,
          name: c.name,
          attackType: c.attack_type as IntruderAttack,
          reqRaw: c.req_raw,
          urlHint: c.url_hint ?? "",
          follow: c.follow,
          setMap: parseJsonArray<number>(c.set_map, []),
          threads: c.threads,
          delayMs: c.delay_ms,
          maxRequests: c.max_requests,
          extractors: parseJsonArray<{ name: string; pattern: string }>(c.extractors, []),
        });
        setSlots(loaded.length > 0 ? loaded : [emptySlot(1)]);
        setSelSlot(0);
        setRun(d.state ?? null);
        setSelSeq(null);
        setDetail(null);
        setConfirmDel(false);
        await loadResults(cid);
      } catch (e) {
        setErr(String(e));
        setToast(String(e));
      } finally {
        setBusy(false);
      }
    },
    [loadResults, setToast]
  );

  useEffect(() => {
    void (async () => {
      await loadList();
    })();
  }, [loadList]);

  useEffect(() => {
    if (campaigns.length === 0) {
      if (activeId != null) {
        setActiveId(null);
        setDraft(null);
        setRun(null);
        setRows([]);
        setTotal(0);
      }
      return;
    }
    if (activeId == null || !campaigns.some((c) => c.id === activeId)) {
      void openCampaign(campaigns[0].id);
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [campaigns]);

  useEffect(() => {
    const un = listen<IntruderRunState>("intruder-progress", (ev) => {
      const s = ev.payload;
      if (!s || typeof s.campaign_id !== "number") return;
      if (s.campaign_id !== activeIdRef.current) return;
      setRun(s);
      if (s.done) {
        void loadList();
        void loadResults(s.campaign_id);
      }
    });
    return () => {
      un.then((f) => f());
    };
  }, [loadList, loadResults]);

  const live = !!run && (run.status === "running" || run.status === "paused");
  useEffect(() => {
    if (!live || activeId == null) return;
    const t = window.setInterval(() => void loadResults(activeId), 1000);
    return () => window.clearInterval(t);
  }, [live, activeId, loadResults]);

  useEffect(() => {
    if (!incoming || incoming.seq === 0) return;
    void (async () => {
      try {
        const cid = await intruderApi.create(incoming.raw, incoming.url || null);
        await loadList();
        await openCampaign(cid);
        setSubtab("positions");
      } catch (e) {
        setToast(String(e));
      }
    })();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [incoming?.seq]);

  const positions = useMemo(() => (draft ? positionTexts(draft.reqRaw) : []), [draft?.reqRaw]);

  const setMap = useMemo(() => {
    const n = positions.length;
    const stored = draft?.setMap ?? [];
    return Array.from({ length: n }, (_, i) => {
      const v = stored[i];
      if (typeof v === "number" && v >= 0 && v < Math.max(slots.length, 1)) return v;
      return Math.min(i, Math.max(slots.length - 1, 0));
    });
  }, [positions.length, draft?.setMap, slots.length]);

  const setSizes = useMemo(
    () => slots.map((s) => payloadCount(s.kind, s.items, s.start, s.end, s.step)),
    [slots]
  );

  const estimate = useMemo(
    () => estimateTotal(draft?.attackType ?? "sniper", setSizes, positions.length, setMap),
    [draft?.attackType, setSizes, positions.length, setMap]
  );

  function patch(p: Partial<Draft>) {
    setDraft((d) => (d ? { ...d, ...p } : d));
  }

  function patchSlot(i: number, p: Partial<Slot>) {
    setSlots((list) => list.map((s, idx) => (idx === i ? { ...s, ...p } : s)));
  }

  async function newCampaign() {
    setBusy(true);
    try {
      const cid = await intruderApi.create(DEFAULT_REQ, null);
      await loadList();
      await openCampaign(cid);
      setSubtab("positions");
      setToast("New Intruder campaign created");
    } catch (e) {
      setToast(String(e));
    } finally {
      setBusy(false);
    }
  }

  async function deleteCampaign() {
    if (!draft) return;
    if (!confirmDel) {
      setConfirmDel(true);
      window.setTimeout(() => setConfirmDel(false), 4000);
      return;
    }
    try {
      await intruderApi.remove(draft.id);
      setConfirmDel(false);
      setDraft(null);
      setActiveId(null);
      await loadList();
      setToast("Campaign deleted");
    } catch (e) {
      setToast(String(e));
    }
  }

  async function saveAll(): Promise<boolean> {
    if (!draft) return false;
    setErr("");
    try {
      await intruderApi.save({
        id: draft.id,
        name: draft.name,
        attackType: draft.attackType,
        reqRaw: draft.reqRaw,
        urlHint: draft.urlHint.trim() || null,
        follow: draft.follow,
        setMap,
        threads: draft.threads,
        delayMs: draft.delayMs,
        maxRequests: draft.maxRequests,
        extractors: draft.extractors,
      });
      const inputs = slots.map((s, i) => ({
        index: i,
        name: s.name.trim() || `Set ${i + 1}`,
        kind: s.kind,
        items: s.kind === "list" ? s.items : "",
        start: s.kind === "numbers" ? s.start : null,
        end: s.kind === "numbers" ? s.end : null,
        step: s.kind === "numbers" ? s.step : null,
        processors: s.processors.map((p) => ({
          kind: p.kind,
          a: p.a === "" ? null : p.a,
          b: p.b === "" ? null : p.b,
        })),
      }));
      await intruderApi.savePayloads(draft.id, inputs);
      await loadList();
      return true;
    } catch (e) {
      setErr(String(e));
      setToast(String(e));
      return false;
    }
  }

  async function start() {
    if (!draft) return;
    setBusy(true);
    try {
      if (!(await saveAll())) return;
      const snap = await intruderApi.start(draft.id);
      setRun(snap);
      setRows([]);
      setTotal(0);
      setSelSeq(null);
      setDetail(null);
      setToast(`Attack started — ${snap.total} requests queued`);
    } catch (e) {
      setErr(String(e));
      setToast(String(e));
    } finally {
      setBusy(false);
    }
  }

  async function act(kind: "pause" | "resume" | "stop") {
    if (!draft) return;
    try {
      const snap = await intruderApi[kind](draft.id);
      setRun(snap);
      if (kind === "stop") setToast("Stop requested — finishing in-flight requests");
      if (kind === "pause") setToast("Attack paused");
      if (kind === "resume") setToast("Attack resumed");
    } catch (e) {
      setToast(String(e));
    }
  }

  async function autoDetect() {
    if (!draft) return;
    try {
      const next = await intruderApi.autoPositions(draft.reqRaw);
      if (next === draft.reqRaw) setToast("No new insertion points found");
      else {
        patch({ reqRaw: next });
        setToast(`Detected ${countPositions(next)} insertion points`);
      }
    } catch (e) {
      setToast(String(e));
    }
  }

  function wrapSel() {
    const ta = editorRef.current;
    if (!ta || !draft) return;
    const start = ta.selectionStart;
    const end = ta.selectionEnd;
    patch({ reqRaw: wrapSelection(draft.reqRaw, start, end) });
    window.setTimeout(() => {
      ta.focus();
      const p = start + MARK.length * 2 + (end - start);
      ta.setSelectionRange(p, p);
    }, 0);
  }

  function clearMarkers() {
    if (!draft) return;
    patch({ reqRaw: stripMarkers(draft.reqRaw) });
    setToast("Position markers cleared");
  }

  function selectResult(seq: number) {
    if (!draft) return;
    setSelSeq(seq);
    void (async () => {
      try {
        const full = await intruderApi.result(draft.id, seq);
        setDetail(full ?? null);
      } catch (e) {
        setToast(String(e));
      }
    })();
  }

  async function clearResults() {
    if (!draft || running) return;
    try {
      await intruderApi.clearResults(draft.id);
      setRows([]);
      setTotal(0);
      setSelSeq(null);
      setDetail(null);
      setToast("Results cleared");
    } catch (e) {
      setToast(String(e));
    }
  }

  function addSet() {
    if (slots.length >= 8) return;
    setSlots((l) => [...l, emptySlot(l.length + 1)]);
    setSelSlot(slots.length);
  }

  function removeSet(i: number) {
    if (slots.length <= 1) return;
    setSlots((l) => l.filter((_, idx) => idx !== i));
    setSelSlot((s) => Math.max(0, Math.min(s, slots.length - 2)));
  }

  if (campaigns.length === 0) {
    return (
      <div className="panel" role="tabpanel" aria-label="Intruder" id="panel-intruder">
        <div className="blank" tabIndex={0}>
          <div className="blank-inner">
            <h2>No Intruder campaigns</h2>
            <p>
              Campaigns pair a request template with insertion points (§…§), payload sets with
              processors, and an attack type — sniper, battering ram, pitchfork or cluster bomb.
              Create one, or send a request over from Proxy history with Send to Intruder.
            </p>
            <div className="blank-actions">
              <button className="btn primary" onClick={newCampaign} disabled={busy}>
                New campaign
              </button>
            </div>
          </div>
        </div>
      </div>
    );
  }

  if (!draft) {
    return (
      <div className="panel" role="tabpanel" aria-label="Intruder" id="panel-intruder">
        <div className="blank" tabIndex={0}>
          <div className="blank-inner">
            <h2>Loading campaign…</h2>
          </div>
        </div>
      </div>
    );
  }

  const slot = slots[Math.min(selSlot, slots.length - 1)];

  return (
    <div className="panel" role="tabpanel" aria-label="Intruder" id="panel-intruder">
      <div className="tabstrip">
        <div className="rtab-list" role="tablist" aria-label="Intruder campaigns">
          {campaigns.map((c) => (
            <button
              key={c.id}
              role="tab"
              aria-selected={c.id === activeId}
              className={`rtab ${c.id === activeId ? "active" : ""}`}
              onClick={() => void openCampaign(c.id)}
              title={`${c.name} — ${STATUS_LABEL[c.status] ?? c.status}, ${c.positions} positions`}
            >
              {c.name}
              {c.status !== "draft" && c.status !== "done" && (
                <span className={`rtab-status ${c.status === "error" ? "err" : ""}`}>
                  {STATUS_LABEL[c.status] ?? c.status}
                </span>
              )}
            </button>
          ))}
        </div>
        <button
          className="rtab plus"
          title="New campaign"
          aria-label="New campaign"
          onClick={() => void newCampaign()}
          disabled={busy}
        >
          +
        </button>
      </div>

      <div className="toolbar">
        <div className="segmented" role="group" aria-label="Attack type">
          {ATTACKS.map((a) => (
            <button
              key={a.id}
              className={draft.attackType === a.id ? "active" : ""}
              aria-pressed={draft.attackType === a.id}
              disabled={running}
              title={a.hint}
              onClick={() => patch({ attackType: a.id })}
            >
              {a.label}
            </button>
          ))}
        </div>

        <button className="btn primary send" onClick={() => void start()} disabled={busy || running}>
          Start
        </button>
        <button
          className="btn"
          onClick={() => void act("pause")}
          disabled={!running || run.status !== "running"}
        >
          Pause
        </button>
        <button className="btn" onClick={() => void act("resume")} disabled={run?.status !== "paused"}>
          Resume
        </button>
        <button className="btn danger" onClick={() => void act("stop")} disabled={!running}>
          Stop
        </button>

        <span className="muted" title="Concurrent requests in flight (1-64)">
          Threads
        </span>
        <input
          className="input intruder-num"
          type="number"
          min={1}
          max={64}
          value={draft.threads}
          disabled={running}
          aria-label="Threads"
          onChange={(e) => {
            const v = parseInt(e.target.value, 10);
            patch({ threads: Number.isNaN(v) ? 1 : Math.min(64, Math.max(1, v)) });
          }}
        />
        <span className="muted" title="Delay between request starts in milliseconds">
          Delay
        </span>
        <input
          className="input intruder-num"
          type="number"
          min={0}
          max={60000}
          value={draft.delayMs}
          disabled={running}
          aria-label="Delay in milliseconds"
          onChange={(e) => {
            const v = parseInt(e.target.value, 10);
            patch({ delayMs: Number.isNaN(v) ? 0 : Math.min(60000, Math.max(0, v)) });
          }}
        />
        <span className="muted" title="Maximum requests to send (0 = unlimited)">
          Max
        </span>
        <input
          className="input intruder-num"
          type="number"
          min={0}
          value={draft.maxRequests}
          disabled={running}
          aria-label="Maximum requests"
          onChange={(e) => {
            const v = parseInt(e.target.value, 10);
            patch({ maxRequests: Number.isNaN(v) ? 0 : Math.max(0, v) });
          }}
        />
        <label className="switch" title="Follow 3xx responses (max 5 hops)">
          <input
            type="checkbox"
            checked={draft.follow}
            disabled={running}
            onChange={(e) => patch({ follow: e.target.checked })}
          />
          Follow redirects
        </label>

        <button className="btn" onClick={() => void saveAll()} disabled={running || busy}>
          Save
        </button>
        <button
          className={`btn danger ${confirmDel ? "armed" : ""}`}
          onClick={() => void deleteCampaign()}
          disabled={running || busy}
        >
          {confirmDel ? "Confirm delete?" : "Delete"}
        </button>

        <span className="grow" />

        <span className="chip" title="Insertion points, payload sets and estimated request count">
          {positions.length} pos · {slots.length} set{slots.length === 1 ? "" : "s"} · ≈{estimate} req
        </span>
        {live && (
          <>
            <progress
              className="intruder-progress"
              aria-label="Attack progress"
              max={Math.max(run.total, 1)}
              value={run.sent}
            />
            <span className="chip">
              {run.sent}/{run.total}
            </span>
            <span className="chip" title="Requests per second">
              {run.rps} req/s
            </span>
            {run.errors > 0 && <span className="chip err">{run.errors} err</span>}
          </>
        )}
        <span className={statusChipClass(status)}>{STATUS_LABEL[status] ?? status}</span>
        {err && <span className="chip err intruder-err">{err}</span>}
      </div>

      <div className="subtabs">
        <div className="subtabs-list" role="tablist" aria-label="Intruder views">
          <button
            role="tab"
            aria-selected={subtab === "positions"}
            className={subtab === "positions" ? "active" : ""}
            onClick={() => setSubtab("positions")}
          >
            Positions
          </button>
          <button
            role="tab"
            aria-selected={subtab === "payloads"}
            className={subtab === "payloads" ? "active" : ""}
            onClick={() => setSubtab("payloads")}
          >
            Payloads
          </button>
          <button
            role="tab"
            aria-selected={subtab === "extractors"}
            className={subtab === "extractors" ? "active" : ""}
            onClick={() => setSubtab("extractors")}
          >
            Extractors
          </button>
          <button
            role="tab"
            aria-selected={subtab === "results"}
            className={subtab === "results" ? "active" : ""}
            onClick={() => {
              setSubtab("results");
              if (draft) void loadResults(draft.id);
            }}
          >
            Results{total > 0 ? ` (${total})` : ""}
          </button>
        </div>
        <div className="subtabs-end" title={draft.urlHint || "Target taken from the request line"}>
          {draft.urlHint || "target from request line"}
        </div>
      </div>

      {subtab === "positions" && (
        <div className="split side">
          <div className="pane">
            <div className="pane-title">
              Request template
              <span className="grow" />
              <button className="btn mini" onClick={() => void autoDetect()} disabled={running}>
                Auto-detect
              </button>
              <button className="btn mini" onClick={wrapSel} disabled={running} title="Wrap the selected text in §…§ markers">
                Wrap selection
              </button>
              <button className="btn mini" onClick={clearMarkers} disabled={running}>
                Clear markers
              </button>
            </div>
            <textarea
              className="intruder-editor"
              ref={editorRef}
              value={draft.reqRaw}
              onChange={(e) => patch({ reqRaw: e.target.value })}
              spellCheck={false}
              aria-label="Request template with position markers"
            />
          </div>
          <div className="pane">
            <div className="pane-title">
              Positions <span className="muted">{positions.length} detected</span>
            </div>
            <div className="pane-body">
              {positions.length === 0 ? (
                <p className="muted">
                  No insertion points yet. Click Auto-detect to mark query string, cookie and
                  body parameters — or select text in the request and press Wrap selection.
                  Markers look like <span className="mono">{MARK}value{MARK}</span>.
                </p>
              ) : (
                <>
                  {positions.map((text, i) => (
                    <div className="intruder-row" key={`${i}-${text}`}>
                      <span className="k mono ellipsis" title={text}>
                        {MARK}
                        {text || "(empty)"}
                        {MARK}
                      </span>
                      <select
                        className="select"
                        aria-label={`Payload set for position ${i + 1}`}
                        value={setMap[i] ?? 0}
                        disabled={running}
                        onChange={(e) => {
                          const next = setMap.slice();
                          next[i] = parseInt(e.target.value, 10);
                          patch({ setMap: next });
                        }}
                      >
                        {slots.map((s, si) => (
                          <option key={si} value={si}>
                            Set {si + 1} — {s.name || `Set ${si + 1}`}
                          </option>
                        ))}
                      </select>
                      <span className="chip" title="Payloads in this position's set">
                        {setSizes[setMap[i] ?? 0] ?? 0}
                      </span>
                    </div>
                  ))}
                  <p className="muted">
                    Sniper attacks each position in turn with its payload set; pitchfork and
                    cluster bomb combine every position's set. Switch to Payloads to edit them.
                  </p>
                </>
              )}
            </div>
          </div>
        </div>
      )}

      {subtab === "payloads" && (
        <div className="split side">
          <div className="pane">
            <div className="pane-title">
              Payload sets <span className="muted">{slots.length} of 8</span>
              <span className="grow" />
              <button className="btn mini" onClick={addSet} disabled={slots.length >= 8}>
                + Add set
              </button>
            </div>
            <div className="pane-body intruder-setlist">
              {slots.map((s, i) => (
                <div className={`intruder-setcard ${selSlot === i ? "active" : ""}`} key={i}>
                  <button
                    className="intruder-setcard-main"
                    onClick={() => setSelSlot(i)}
                    aria-pressed={selSlot === i}
                    title={`Edit ${s.name || `Set ${i + 1}`}`}
                  >
                    <b>
                      {i + 1}. {s.name || `Set ${i + 1}`}
                    </b>
                    <span className="row">
                      <span className="chip">{setSizes[i]} payloads</span>
                      <span className="chip">{s.kind === "numbers" ? "numbers" : "list"}</span>
                      {s.processors.length > 0 && (
                        <span className="chip ok" title={s.processors.map((p) => processorDef(p.kind).label).join(" → ")}>
                          {s.processors.length} proc
                        </span>
                      )}
                    </span>
                  </button>
                  <button
                    className="icon-btn"
                    title={`Remove ${s.name || `Set ${i + 1}`}`}
                    aria-label={`Remove set ${i + 1}`}
                    disabled={slots.length <= 1 || running}
                    onClick={() => removeSet(i)}
                  >
                    ✕
                  </button>
                </div>
              ))}
            </div>
          </div>
          <div className="pane">
            <div className="pane-title">
              Set {selSlot + 1} — {slot.name || `Set ${selSlot + 1}`}
              <span className="grow" />
              <span className="chip">{setSizes[selSlot] ?? 0} payloads</span>
            </div>
            <div className="pane-body">
              <div className="intruder-row">
                <span className="k">Name</span>
                <input
                  className="input grow"
                  value={slot.name}
                  disabled={running}
                  aria-label="Payload set name"
                  onChange={(e) => patchSlot(selSlot, { name: e.target.value })}
                />
                <div className="segmented" role="group" aria-label="Payload set kind">
                  <button
                    className={slot.kind === "list" ? "active" : ""}
                    aria-pressed={slot.kind === "list"}
                    disabled={running}
                    onClick={() => patchSlot(selSlot, { kind: "list" })}
                  >
                    Simple list
                  </button>
                  <button
                    className={slot.kind === "numbers" ? "active" : ""}
                    aria-pressed={slot.kind === "numbers"}
                    disabled={running}
                    onClick={() => patchSlot(selSlot, { kind: "numbers" })}
                  >
                    Numbers
                  </button>
                </div>
              </div>

              {slot.kind === "list" ? (
                <textarea
                  className="intruder-payloads"
                  value={slot.items}
                  disabled={running}
                  aria-label="Payload list, one per line"
                  placeholder={"admin\nroot\n' OR 1=1--"}
                  spellCheck={false}
                  onChange={(e) => patchSlot(selSlot, { items: e.target.value })}
                />
              ) : (
                <div className="intruder-row">
                  <span className="k">From</span>
                  <input
                    className="input intruder-num"
                    type="number"
                    value={slot.start ?? ""}
                    disabled={running}
                    aria-label="Number range start"
                    onChange={(e) =>
                      patchSlot(selSlot, { start: e.target.value === "" ? null : parseInt(e.target.value, 10) })
                    }
                  />
                  <span className="k">to</span>
                  <input
                    className="input intruder-num"
                    type="number"
                    value={slot.end ?? ""}
                    disabled={running}
                    aria-label="Number range end"
                    onChange={(e) =>
                      patchSlot(selSlot, { end: e.target.value === "" ? null : parseInt(e.target.value, 10) })
                    }
                  />
                  <span className="k">step</span>
                  <input
                    className="input intruder-num"
                    type="number"
                    min={1}
                    value={slot.step ?? 1}
                    disabled={running}
                    aria-label="Number range step"
                    onChange={(e) => {
                      const v = parseInt(e.target.value, 10);
                      patchSlot(selSlot, { step: Number.isNaN(v) || v < 1 ? 1 : v });
                    }}
                  />
                </div>
              )}

              <div className="pane-title intruder-subtitle">
                Payload processors
                <span className="grow" />
                <button
                  className="btn mini"
                  disabled={running || slot.processors.length >= PROCESSORS.length}
                  onClick={() =>
                    patchSlot(selSlot, {
                      processors: [...slot.processors, { kind: "url_encode", a: "", b: "" }],
                    })
                  }
                >
                  + Add processor
                </button>
              </div>
              {slot.processors.length === 0 ? (
                <p className="muted">
                  Payloads are used as-is. Add a processor to encode, hash, case-change or pad
                  each value before it is placed in the request — they run in order.
                </p>
              ) : (
                slot.processors.map((p, i) => {
                  const def = processorDef(p.kind);
                  return (
                    <div className="intruder-row" key={i}>
                      <span className="k">{i + 1}.</span>
                      <select
                        className="select"
                        aria-label={`Processor ${i + 1} type`}
                        value={p.kind}
                        disabled={running}
                        onChange={(e) => patchSlot(selSlot, {
                          processors: slot.processors.map((x, idx) =>
                            idx === i ? { ...x, kind: e.target.value, a: "", b: "" } : x
                          ),
                        })}
                      >
                        {PROCESSORS.map((x) => (
                          <option key={x.kind} value={x.kind}>
                            {x.label}
                          </option>
                        ))}
                      </select>
                      {def.args.includes("a") && (
                        <input
                          className="input grow"
                          placeholder={p.kind === "replace" ? "find" : "value"}
                          value={p.a}
                          disabled={running}
                          aria-label={`Processor ${i + 1} first argument`}
                          onChange={(e) =>
                            patchSlot(selSlot, {
                              processors: slot.processors.map((x, idx) =>
                                idx === i ? { ...x, a: e.target.value } : x
                              ),
                            })
                          }
                        />
                      )}
                      {def.args.includes("b") && (
                        <input
                          className="input grow"
                          placeholder="replace with"
                          value={p.b}
                          disabled={running}
                          aria-label={`Processor ${i + 1} second argument`}
                          onChange={(e) =>
                            patchSlot(selSlot, {
                              processors: slot.processors.map((x, idx) =>
                                idx === i ? { ...x, b: e.target.value } : x
                              ),
                            })
                          }
                        />
                      )}
                      <button
                        className="icon-btn"
                        aria-label={`Remove processor ${i + 1}`}
                        title="Remove processor"
                        disabled={running}
                        onClick={() =>
                          patchSlot(selSlot, {
                            processors: slot.processors.filter((_, idx) => idx !== i),
                          })
                        }
                      >
                        ✕
                      </button>
                    </div>
                  );
                })
              )}
              {slot.processors.length > 0 && (
                <p className="muted">
                  Order:{" "}
                  <span className="mono">
                    {slot.processors.map((p) => processorDef(p.kind).label).join(" → ")}
                  </span>
                </p>
              )}
              <p className="muted">
                Preview: <span className="mono">{previewPayload(slot, setSizes[selSlot] ?? 0)}</span>
              </p>
            </div>
          </div>
        </div>
      )}

      {subtab === "extractors" && (
        <div className="split side">
          <div className="pane">
            <div className="pane-title">
              Grep extractors
              <span className="grow" />
              <button
                className="btn mini"
                disabled={running}
                onClick={() =>
                  patch({ extractors: [...draft.extractors, { name: "", pattern: "" }] })
                }
              >
                + Add extractor
              </button>
            </div>
            <div className="pane-body">
              <p className="muted">
                Each extractor runs against every stored response; the first capture group — or
                the whole match — is saved with the result and shown in the results table.
              </p>
              {draft.extractors.length === 0 ? (
                <div className="empty">No extractors yet — add one to surface tokens, IDs or CSRF values per response.</div>
              ) : (
                draft.extractors.map((x, i) => (
                  <div className="intruder-row" key={i}>
                    <span className="k">{i + 1}.</span>
                    <input
                      className="input"
                      placeholder="Name"
                      value={x.name}
                      disabled={running}
                      aria-label={`Extractor ${i + 1} name`}
                      onChange={(e) =>
                        patch({
                          extractors: draft.extractors.map((y, idx) =>
                            idx === i ? { ...y, name: e.target.value } : y
                          ),
                        })
                      }
                    />
                    <input
                      className="input grow mono"
                      placeholder={'regex, e.g. csrf" value="([^"]+)"'}
                      value={x.pattern}
                      disabled={running}
                      aria-label={`Extractor ${i + 1} pattern`}
                      spellCheck={false}
                      onChange={(e) =>
                        patch({
                          extractors: draft.extractors.map((y, idx) =>
                            idx === i ? { ...y, pattern: e.target.value } : y
                          ),
                        })
                      }
                    />
                    <button
                      className="icon-btn"
                      aria-label={`Remove extractor ${i + 1}`}
                      title="Remove extractor"
                      disabled={running}
                      onClick={() =>
                        patch({ extractors: draft.extractors.filter((_, idx) => idx !== i) })
                      }
                    >
                      ✕
                    </button>
                  </div>
                ))
              )}
            </div>
          </div>
          <div className="pane">
            <div className="pane-title">Example patterns</div>
            <div className="pane-body">
              <div className="intruder-row">
                <span className="k">CSRF token</span>
                <span className="mono">csrf" value="([^"]+)"</span>
              </div>
              <div className="intruder-row">
                <span className="k">Set-Cookie value</span>
                <span className="mono">Set-Cookie: session=([^;]+)</span>
              </div>
              <div className="intruder-row">
                <span className="k">JSON id</span>
                <span className="mono">"id":\s*(\d+)</span>
              </div>
              <div className="intruder-row">
                <span className="k">Title</span>
                <span className="mono">&lt;title&gt;([^&lt;]+)&lt;/title&gt;</span>
              </div>
              <p className="muted">
                Extractors are validated when the campaign is saved — invalid patterns are
                reported before any request is sent. Results keep the full response, so you can
                open any row and inspect it even without an extractor.
              </p>
            </div>
          </div>
        </div>
      )}

      {subtab === "results" && (
        <div className="split side">
          <div className="pane">
            <div className="pane-title">
              Results <span className="muted">{rows.length} of {total}</span>
              <span className="grow" />
              <button className="btn mini" onClick={() => void loadResults(draft.id)}>
                Refresh
              </button>
              <button className="btn mini" onClick={() => void clearResults()} disabled={running}>
                Clear
              </button>
            </div>
            <div className="table-wrap" tabIndex={0}>
              <table className="history intruder-results">
                <thead>
                  <tr>
                    <th scope="col">#</th>
                    <th scope="col">Status</th>
                    <th scope="col">Payloads</th>
                    <th scope="col">Extracted</th>
                    <th scope="col">Length</th>
                    <th scope="col">Time</th>
                  </tr>
                </thead>
                <tbody>
                  {rows.map((r) => {
                    const combo = parseCombo(r.combo).join(" · ");
                    const extracted = Object.entries(parseJsonObject(r.extracted))
                      .map(([k, v]) => `${k}=${v}`)
                      .join(" · ");
                    return (
                      <tr
                        key={r.seq}
                        className={selSeq === r.seq ? "selected" : ""}
                        onClick={() => selectResult(r.seq)}
                        tabIndex={0}
                        onKeyDown={(e) => {
                          if (e.key === "Enter" || e.key === " ") {
                            e.preventDefault();
                            selectResult(r.seq);
                          }
                        }}
                        aria-selected={selSeq === r.seq}
                      >
                        <td className="mono">{r.seq + 1}</td>
                        <td>
                          {r.error ? (
                            <span className="chip err" title={r.error}>
                              err
                            </span>
                          ) : (
                            <span className={statusClass(r.status)}>{r.status ?? "—"}</span>
                          )}
                        </td>
                        <td className="mono ellipsis" title={combo}>
                          {combo}
                        </td>
                        <td className="mono ellipsis" title={extracted}>
                          {extracted || "—"}
                        </td>
                        <td className="mono">{r.resp_len}</td>
                        <td className="mono">{r.time_ms} ms</td>
                      </tr>
                    );
                  })}
                  {rows.length === 0 && (
                    <tr>
                      <td colSpan={6} className="empty-cell">
                        <div className="empty">
                          {running
                            ? "Attack running — results stream in as requests finish…"
                            : "No results yet. Press Start to run the attack."}
                        </div>
                      </td>
                    </tr>
                  )}
                </tbody>
              </table>
            </div>
          </div>
          <div className="pane">
            <div className="pane-title">
              Result detail
              {detail && (
                <>
                  <span className="chip">#{detail.seq + 1}</span>
                  <span className={statusClass(detail.status)}>{detail.status ?? "err"}</span>
                  <span className="chip">{detail.time_ms} ms</span>
                  <span className="grow" />
                  <span className="chip" title={parseCombo(detail.combo).join(" · ")}>
                    {parseCombo(detail.combo).join(" · ")}
                  </span>
                </>
              )}
            </div>
            {detail ? (
              <SplitPane direction="column" initial={50} min={20} max={80}>
                <div className="pane-flush">
                  <MessageView value={detail.req_raw} title="Request" />
                </div>
                <div className="pane-flush">
                  <MessageView
                    value={detail.resp_raw ?? (detail.error ? `Error: ${detail.error}` : "")}
                    title="Response"
                    extra={
                      Object.keys(parseJsonObject(detail.extracted)).length > 0 ? (
                        <span className="row" style={{ gap: 6 }}>
                          {Object.entries(parseJsonObject(detail.extracted)).map(([k, v]) => (
                            <span className="chip ok" key={k} title={`${k}: ${v}`}>
                              {k}: {String(v).slice(0, 24)}
                            </span>
                          ))}
                        </span>
                      ) : undefined
                    }
                  />
                </div>
              </SplitPane>
            ) : (
              <div className="blank" tabIndex={0}>
                <div className="blank-inner">
                  <h2>No result selected</h2>
                  <p>
                    Pick a row on the left to inspect the exact request that was sent — with each
                    payload substituted — alongside its full response.
                  </p>
                </div>
              </div>
            )}
          </div>
        </div>
      )}
    </div>
  );
}

function previewPayload(slot: Slot, size: number): string {
  if (size === 0) return "(no payloads)";
  if (slot.kind === "numbers") {
    const s = slot.start ?? 0;
    const e = slot.end ?? 0;
    const st = slot.step ?? 1;
    return `${s} … ${e} step ${st} (${size} values)`;
  }
  const first = slot.items.split("\n")[0] ?? "";
  return `${first || "(empty)"} … (${size} values)`;
}
