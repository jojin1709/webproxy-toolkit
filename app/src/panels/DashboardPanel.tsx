import { useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Pause, Settings, HelpCircle, Radio } from "lucide-react";
import Placeholder from "../components/Placeholder";
import LogTable from "../components/LogTable";
import { tabListKeyDown } from "../components/tabKeys";
import { ProxyStatus } from "../types";

type Props = {
  status: ProxyStatus;
  setStatus: (s: ProxyStatus) => void;
  setToast: (t: string) => void;
};

const SUBS: [string, string, boolean][] = [
  ["summary", "Summary", false],
  ["audit-items", "Audit items", true],
  ["issues", "Issues", true],
  ["events", "Event log", false],
  ["audit-log", "Audit log", true],
];

export default function DashboardPanel({ status, setStatus, setToast }: Props) {
  const [view, setView] = useState("summary");
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
      ) : (
      <div className="dash" role="tabpanel" id="dash-summary" aria-label="Summary">
        <aside className="dash-tasks">
          <h2>Tasks</h2>
          <div className="dash-task-buttons">
            <button className="btn primary" disabled title="Phase 4 — scanner">
              New scan
            </button>
            <button className="btn primary" disabled title="Phase 4 — scanner">
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
              Records proxied HTTP/HTTPS traffic, applies match-and-replace rules, and feeds
              tools. Issues arrive with the scanner in Phase 4.
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
            <div className="issue-badges" style={{ marginTop: 8 }}>
              Issues:
              <span className="issue-badge crit">0</span>
              <span className="issue-badge high">0</span>
              <span className="issue-badge med">0</span>
              <span className="issue-badge info">0</span>
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
                <button className="link" disabled style={{ opacity: 0.5 }}>
                  View all
                </button>
              </div>
              <table className="history" style={{ display: "table-header-group" }}>
                <thead>
                  <tr>
                    <th style={{ width: "55%" }}>Issue type</th>
                    <th>Host</th>
                    <th>Time</th>
                  </tr>
                </thead>
              </table>
              <Placeholder
                title="No issues to show"
                description="Any issues found during the audit will be displayed here. Passive and active audit checks arrive with the scanner in Phase 4 — traffic capture is already running through them."
              />
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
                  <span>Record + match and replace rules</span>
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
                    <span className="muted">Audit items pending</span>
                    <span className="mono">0</span>
                  </div>
                  <div className="pg-row">
                    <span className="muted">Network errors</span>
                    <span className="mono">0</span>
                  </div>
                </div>
              </div>
            </section>
          </div>
        </div>
      </div>
      )}
    </div>
  );
}
