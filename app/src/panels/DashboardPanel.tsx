import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { Pause, Settings, HelpCircle, Radio, Plus, X, Pencil } from "lucide-react";
import LogTable from "../components/LogTable";
import { IssueTable } from "../components/IssueTable";
import ScanConfigEditor from "../components/ScanConfigEditor";
import { tabListKeyDown } from "../components/tabKeys";
import { sevClass, scannerApi } from "../lib/scanner";
import type { Issue, IssueCounts, ScanConfig, ScanKind, ScanProgress, ScanTask } from "../types";
import { ProxyStatus } from "../types";

type Props = {
  status: ProxyStatus;
  setStatus: (s: ProxyStatus) => void;
  setToast: (t: string) => void;
};

const SUBS: [string, string, boolean][] = [
  ["summary", "Summary", false],
  ["audit-items", "Audit items", false],
  ["issues", "Issues", false],
  ["events", "Event log", false],
  ["audit-log", "Audit log", false],
];

const EMPTY_COUNTS: IssueCounts = { critical: 0, high: 0, medium: 0, low: 0, info: 0 };

function statusChip(s: ScanTask["status"]): string {
  return s === "running" ? "chip ok" : s === "error" ? "chip err" : "chip";
}

export default function DashboardPanel({ status, setStatus, setToast }: Props) {
  const [view, setView] = useState("summary");
  const [counts, setCounts] = useState<IssueCounts>(EMPTY_COUNTS);
  const [top, setTop] = useState<Issue[]>([]);
  const [tasks, setTasks] = useState<ScanTask[]>([]);
  const [live, setLive] = useState<ScanProgress[]>([]);
  const [showNew, setShowNew] = useState(false);
  const [newTarget, setNewTarget] = useState("");
  const [newKind, setNewKind] = useState<ScanKind>("crawl-audit");
  const [configs, setConfigs] = useState<ScanConfig[]>([]);
  const [newConfigId, setNewConfigId] = useState<number | "">("");
  const [editConfig, setEditConfig] = useState<ScanConfig | null>(null);
  const [showConfigEditor, setShowConfigEditor] = useState(false);

  const loadIssues = useCallback(async () => {
    try {
      setCounts(await scannerApi.counts());
      const { rows } = await scannerApi.list({
        severities: ["critical", "high", "medium"],
        limit: 8,
      });
      setTop(rows);
    } catch {
      // backend not ready (browser QA mock without handlers yet) — keep zeros
    }
  }, []);

  const loadTasks = useCallback(async () => {
    try {
      setTasks(await scannerApi.scanTasks());
      setLive((await scannerApi.scanState()).running);
    } catch {
      // mock/backend may not have scan handlers yet
    }
  }, []);

  const loadConfigs = useCallback(async () => {
    try {
      setConfigs(await scannerApi.scanConfigList());
    } catch {
      // backend not ready
    }
  }, []);

  useEffect(() => {
    if (showNew) void loadConfigs();
  }, [showNew, loadConfigs]);

  useEffect(() => {
    void loadIssues();
    void loadTasks();
    let offs: Array<() => void> = [];
    let disposed = false;
    void (async () => {
      const unsubs = await Promise.all([
        listen("issue-added", () => void loadIssues()),
        listen("issue-updated", () => void loadIssues()),
        listen("issue-deleted", () => void loadIssues()),
        listen<ScanProgress>("scan-progress", (ev) => {
          const p = ev.payload;
          setLive((cur) => {
            const rest = cur.filter((x) => x.task_id !== p.task_id);
            return p.status === "done" || p.status === "cancelled" || p.status === "error"
              ? rest
              : [...rest, p];
          });
          // refresh task list on terminal states
          if (p.status === "done" || p.status === "cancelled" || p.status === "error") {
            void loadTasks();
          }
        }),
      ]);
      if (disposed) unsubs.forEach((u) => u());
      else offs = unsubs;
    })();
    return () => {
      disposed = true;
      offs.forEach((u) => u());
    };
  }, [loadIssues, loadTasks]);

  async function startScan() {
    if (!newTarget.trim()) return;
    try {
      const id = await scannerApi.scanStart(
        newTarget.trim(),
        newKind,
        newConfigId === "" ? undefined : newConfigId,
      );
      setToast(`Scan #${id} started (${newKind})`);
      setShowNew(false);
      setNewTarget("");
      await loadTasks();
    } catch (e) {
      setToast(String(e));
    }
  }

  function openConfigEditor() {
    const sel = configs.find((c) => c.id === newConfigId);
    setEditConfig(sel ?? null);
    setShowConfigEditor(true);
  }

  async function cancelTask(id: number) {
    try {
      await scannerApi.scanCancel(id);
      setToast(`Scan #${id} cancelled`);
      await loadTasks();
    } catch (e) {
      setToast(String(e));
    }
  }

  async function togglePause(t: ScanTask) {
    try {
      if (t.status === "running") {
        await scannerApi.scanPause(t.id);
        setToast(`Scan #${t.id} paused`);
      } else if (t.status === "paused") {
        await scannerApi.scanResume(t.id);
        setToast(`Scan #${t.id} resumed`);
      }
      await loadTasks();
    } catch (e) {
      setToast(String(e));
    }
  }

  async function toggleCapture() {
    try {
      if (status.running) {
        await invoke("proxy_stop");
        setToast("Capture paused — proxy stopped");
      } else {
        await invoke("proxy_start", { host: "127.0.0.1", port: status.port || 8080 });
        setToast("Capture running — proxy listening");
      }
      setStatus(await invoke<ProxyStatus>("proxy_status"));
    } catch (e) {
      setToast(String(e));
    }
  }

  return (
    <div className="panel" role="tabpanel" aria-label="Dashboard" id="panel-dashboard">
      <div className="subtabs">
        <div className="subtabs-list" role="tablist" aria-label="Dashboard views">
          {SUBS.map(([id, label, disabled], i) => (
            <button
              key={id}
              role="tab"
              aria-selected={view === id}
              tabIndex={view === id ? 0 : -1}
              disabled={disabled}
              title={disabled ? "Phase 4" : undefined}
              aria-controls={disabled ? undefined : `dash-${id}`}
              className={view === id ? "active" : ""}
              onClick={() => setView(id)}
              onKeyDown={(e) =>
                tabListKeyDown(e, i, SUBS.map(([, , d]) => d), (next) => setView(SUBS[next][0]))
              }
            >
              {label}
            </button>
          ))}
        </div>
      </div>

      {view === "events" ? (
        <div
          className="dash-events"
          role="tabpanel"
          id="dash-events"
          aria-label="Event log"
          tabIndex={0}
        >
          <LogTable limit={100} />
        </div>
      ) : view === "issues" ? (
        <div
          className="grow min-h-0"
          role="tabpanel"
          id="dash-issues"
          aria-label="Issues"
          tabIndex={0}
        >
          <IssueTable setToast={setToast} />
        </div>
      ) : view === "audit-items" ? (
        <div
          className="panel"
          role="tabpanel"
          id="dash-audit-items"
          aria-label="Audit items"
          tabIndex={0}
          style={{ display: "flex", flexDirection: "column", minHeight: 0 }}
        >
          <div className="toolbar">
            <button className="btn primary" onClick={() => setShowNew(true)}>
              <Plus size={14} /> New scan
            </button>
            <span className="grow" />
            <button className="btn" onClick={() => void loadTasks()}>
              Refresh
            </button>
          </div>
          <div className="table-wrap grow min-h-0" role="region" aria-label="Scan tasks" tabIndex={0}>
            <table className="history">
              <thead>
                <tr>
                  <th>ID</th>
                  <th>Kind</th>
                  <th>Target</th>
                  <th>Status</th>
                  <th>Pages</th>
                  <th>Requests</th>
                  <th>Issues</th>
                  <th aria-label="Actions">
                    <span className="sr-only">Actions</span>
                  </th>
                </tr>
              </thead>
              <tbody>
                {tasks.map((t) => {
                  const prog = live.find((p) => p.task_id === t.id);
                  const st = prog ? (prog.status as ScanTask["status"]) : t.status;
                  return (
                    <tr key={t.id}>
                      <td className="mono">#{t.id}</td>
                      <td>{t.kind}</td>
                      <td className="ellipsis" title={t.target}>
                        {t.target}
                      </td>
                      <td>
                        <span className={statusChip(st)}>{st}</span>
                      </td>
                      <td className="mono">{prog ? prog.pages : t.pages_crawled}</td>
                      <td className="mono">{prog ? prog.requests : t.requests_sent}</td>
                      <td className="mono">{prog ? prog.issues : t.issues_found}</td>
                      <td>
                        {(st === "running" || st === "paused") && (
                          <>
                            <button className="btn mini" onClick={() => void togglePause(t)}>
                              {st === "running" ? "Pause" : "Resume"}
                            </button>{" "}
                            <button className="btn mini danger" onClick={() => void cancelTask(t.id)}>
                              Cancel
                            </button>
                          </>
                        )}
                      </td>
                    </tr>
                  );
                })}
                {tasks.length === 0 && (
                  <tr>
                    <td colSpan={8} className="muted empty-cell">
                      No scan tasks yet. Start a crawl or audit against a target to begin.
                    </td>
                  </tr>
                )}
              </tbody>
            </table>
          </div>
          {showNew && (
            <div className="dlg-backdrop" role="presentation" onClick={() => setShowNew(false)}>
              <div
                className="dlg"
                role="dialog"
                aria-modal="true"
                aria-label="New scan"
                onClick={(e) => e.stopPropagation()}
              >
                <div className="dlg-title">
                  New scan
                  <span className="grow" />
                  <button className="icon-btn" aria-label="Close" onClick={() => setShowNew(false)}>
                    <X size={14} />
                  </button>
                </div>
                <div className="dlg-body">
                  <label className="muted" htmlFor="scan-target">
                    Target URL
                  </label>
                  <input
                    id="scan-target"
                    className="input"
                    placeholder="https://app.example.com/"
                    value={newTarget}
                    onChange={(e) => setNewTarget(e.target.value)}
                    autoFocus
                  />
                  <label className="muted" htmlFor="scan-kind">
                    Kind
                  </label>
                  <select
                    id="scan-kind"
                    className="select"
                    value={newKind}
                    onChange={(e) => setNewKind(e.target.value as ScanKind)}
                  >
                    <option value="crawl-audit">Crawl + audit</option>
                    <option value="crawl">Crawl only</option>
                    <option value="audit">Audit only</option>
                  </select>
                  <label className="muted" htmlFor="scan-config">
                    Config
                  </label>
                  <div className="row">
                    <select
                      id="scan-config"
                      className="select grow"
                      value={newConfigId}
                      onChange={(e) =>
                        setNewConfigId(e.target.value === "" ? "" : Number(e.target.value))
                      }
                    >
                      <option value="">Engine defaults</option>
                      {configs.map((c) => (
                        <option key={c.id} value={c.id}>
                          {c.name}
                          {c.is_default ? " (default)" : ""}
                        </option>
                      ))}
                    </select>
                    <button className="btn" onClick={openConfigEditor}>
                      <Pencil size={14} /> Edit config…
                    </button>
                  </div>
                </div>
                <div className="dlg-foot">
                  <button className="btn" onClick={() => setShowNew(false)}>
                    Cancel
                  </button>
                  <button className="btn primary" onClick={() => void startScan()}>
                    Start scan
                  </button>
                </div>
              </div>
            </div>
          )}
          {showConfigEditor && (
            <ScanConfigEditor
              config={editConfig}
              onClose={() => setShowConfigEditor(false)}
              onSaved={() => void loadConfigs()}
              setToast={setToast}
            />
          )}
        </div>
      ) : view === "audit-log" ? (
        <div
          className="grow min-h-0"
          role="tabpanel"
          id="dash-audit-log"
          aria-label="Audit log"
          tabIndex={0}
        >
          <LogTable limit={100} tool="scanner" />
        </div>
      ) : (
      <div className="dash" role="tabpanel" id="dash-summary" aria-label="Summary">
        <aside className="dash-tasks">
          <h2>Tasks</h2>
          <div className="dash-task-buttons">
            <button className="btn primary" onClick={() => setShowNew(true)}>
              New scan
            </button>
            <button className="btn primary" onClick={() => setShowNew(true)}>
              New live task
            </button>
          </div>
          <div className="row">
            <button className="icon-btn" disabled title="Pause all">
              <Pause size={15} />
            </button>
            <button className="icon-btn" disabled title="Task settings">
              <Settings size={15} />
            </button>
            <button className="icon-btn" disabled title="Help">
              <HelpCircle size={15} />
            </button>
          </div>

          <div className="task-card">
            <div className="task-title">
              1. Live capture from Proxy (all traffic)
            </div>
            <div className="task-sub">
              Records proxied HTTP/HTTPS traffic, applies match-and-replace rules, feeds the
              tools, and runs the passive scanner on every in-scope response.
            </div>
            <div className="row" style={{ justifyContent: "space-between" }}>
              <label className="switch">
                <input
                  type="checkbox"
                  checked={status.running}
                  onChange={toggleCapture}
                />
                Capturing
              </label>
              <span className="row" style={{ gap: 4 }}>
                <Radio size={14} color={status.running ? "var(--color-success)" : "var(--color-text-muted)"} />
              </span>
            </div>
            <div className="issue-badges" style={{ marginTop: 8 }} aria-label="Issue counts by severity">
              Issues:
              <span className="issue-badge crit" title="Critical">{counts.critical}</span>
              <span className="issue-badge high" title="High">{counts.high}</span>
              <span className="issue-badge med" title="Medium">{counts.medium}</span>
              <span className="issue-badge info" title="Info">{counts.info}</span>
              {counts.low > 0 && (
                <span className="issue-badge low" title="Low">{counts.low}</span>
              )}
            </div>
          </div>
        </aside>

        <div className="dash-main">
          <div className="dash-main-head">
            <Radio size={16} color="var(--color-accent)" />
            <h2>1. Live capture from Proxy (all traffic)</h2>
          </div>

          <div className="dash-body">
            <section className="dash-card">
              <div className="dash-card-head">
                Most serious vulnerabilities found (live)
                <button className="link" onClick={() => setView("issues")}>
                  View all
                </button>
              </div>
              <table className="history" style={{ display: "table-header-group" }}>
                <thead>
                  <tr>
                    <th style={{ width: "18%" }}>Severity</th>
                    <th style={{ width: "47%" }}>Issue type</th>
                    <th>Host</th>
                    <th>Time</th>
                  </tr>
                </thead>
                <tbody>
                  {top.map((i) => (
                    <tr key={i.id}>
                      <td>
                        <span className={`sev-chip ${sevClass(i.severity)}`}>{i.severity}</span>
                      </td>
                      <td title={i.name}>{i.name}</td>
                      <td title={i.host}>{i.host}</td>
                      <td>{i.created_at.slice(11, 19)}</td>
                    </tr>
                  ))}
                  {top.length === 0 && (
                    <tr>
                      <td colSpan={4} className="muted empty-cell">
                        No issues yet — the passive scanner raises findings on in-scope traffic
                        as it flows through the proxy.
                      </td>
                    </tr>
                  )}
                </tbody>
              </table>
            </section>

            <section className="dash-card">
              <div className="dash-card-head">Task configuration</div>
              <div className="dash-card-body">
                <div className="task-config-row">
                  <span className="k">Task type:</span>
                  <span>Live capture</span>
                </div>
                <div className="task-config-row">
                  <span className="k">Scope:</span>
                  <span>Proxy (all traffic)</span>
                </div>
                <div className="task-config-row">
                  <span className="k">Configuration:</span>
                  <span>Record + match/replace + passive scan</span>
                </div>
                <div className="task-config-row">
                  <span className="k">Capturing:</span>
                  <label className="switch">
                    <input type="checkbox" checked={status.running} onChange={toggleCapture} />
                    {status.running ? "On" : "Off"}
                  </label>
                </div>
              </div>
            </section>

            <section className="dash-card">
              <div className="dash-card-head">Task progress</div>
              <div className="dash-card-body">
                <div className="progress-grid">
                  <div className="pg-row">
                    <span className="muted">Requests proxied</span>
                    <span className="mono">{status.history_count}</span>
                  </div>
                  <div className="pg-row">
                    <span className="muted">Held messages</span>
                    <span className="mono">{status.held_count}</span>
                  </div>
                  <div className="pg-row">
                    <span className="muted">Issues found</span>
                    <span className="mono">
                      {counts.critical + counts.high + counts.medium + counts.low + counts.info}
                    </span>
                  </div>
                  <div className="pg-row">
                    <span className="muted">Network errors</span>
                    <span className="mono">0</span>
                  </div>
                </div>
                {live.length > 0 && (
                  <div style={{ marginTop: 12 }}>
                    <h3 className="muted" style={{ margin: "0 0 6px", fontSize: "var(--text-xs)", textTransform: "uppercase" }}>
                      Active scans
                    </h3>
                    {live.map((p) => (
                      <div key={p.task_id} className="pg-row">
                        <span className="muted">
                          Scan #{p.task_id} — {p.pages} pages, {p.requests} requests
                        </span>
                        <span className="mono">{p.issues} issues</span>
                      </div>
                    ))}
                  </div>
                )}
              </div>
            </section>
          </div>
        </div>
      </div>
      )}
    </div>
  );
}
