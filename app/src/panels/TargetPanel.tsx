import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { ScopeRule } from "../types";
import { tabListKeyDown } from "../components/tabKeys";
import { IssueTable } from "../components/IssueTable";
import { scannerApi } from "../lib/scanner";

type Sitemap = { host: string; children: { path: string; count: number }[] }[];

type Sub = "sitemap" | "scope" | "issues";

export default function TargetPanel({ setToast }: { setToast: (t: string) => void }) {
  const [sub, setSub] = useState<Sub>("sitemap");
  const [rules, setRules] = useState<ScopeRule[]>([]);
  const [sitemap, setSitemap] = useState<Sitemap>([]);
  const [filter, setFilter] = useState("");
  const [form, setForm] = useState({ mode: "include", kind: "host", pattern: "" });
  const [selectedHost, setSelectedHost] = useState("");

  async function loadRules() {
    try {
      setRules(await invoke<ScopeRule[]>("scope_list"));
    } catch (e) {
      setToast(String(e));
    }
  }

  async function loadSitemap() {
    try {
      const r = await invoke<{ roots: Sitemap }>("sitemap_tree");
      setSitemap(r.roots);
    } catch (e) {
      setToast(String(e));
    }
  }

  useEffect(() => {
    loadRules();
    loadSitemap();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  async function addRule() {
    if (!form.pattern) return;
    try {
      await invoke("scope_add", {
        mode: form.mode,
        kind: form.kind,
        pattern: form.pattern,
      });
      setForm({ ...form, pattern: "" });
      await loadRules();
    } catch (e) {
      setToast(String(e));
    }
  }

  async function removeRule(id: number) {
    await invoke("scope_delete", { id });
    await loadRules();
  }

  function includeHost(host: string) {
    setForm({ mode: "include", kind: "host", pattern: host });
    setSub("scope");
  }

  const SUBS: [Sub, string, boolean][] = [
    ["sitemap", "Site map", false],
    ["scope", "Scope", false],
    ["issues", "Issues", false],
  ];

  return (
    <div className="panel" role="tabpanel" aria-label="Target" id="panel-target">
      <div className="subtabs">
        <div className="subtabs-list" role="tablist" aria-label="Target views">
          {SUBS.map(([id, label, disabled], i) => (
            <button
              key={id}
              role="tab"
              aria-selected={sub === id}
              tabIndex={sub === id ? 0 : -1}
              disabled={disabled}
              title={disabled ? "Phase 4 — scanner" : undefined}
              className={sub === id ? "active" : ""}
              onClick={() => setSub(id)}
              onKeyDown={(e) =>
                tabListKeyDown(e, i, SUBS.map(([, , d]) => d), (next) => setSub(SUBS[next][0]))
              }
            >
              {label}
            </button>
          ))}
        </div>
        {sitemap.length > 0 && <div className="subtabs-end">{sitemap.length} hosts</div>}
      </div>

      {sub === "sitemap" && (
        <div className="panel">
          <div className="toolbar">
            <div className="segmented" role="group" aria-label="View mode">
              <button className="active" aria-pressed="true">
                URL view
              </button>
              <button
                onClick={async () => {
                  // start a crawl-audit from the first sitemap host root
                  const host = sitemap[0]?.host;
                  if (!host) {
                    setToast("No hosts in the site map yet — browse through the proxy first");
                    return;
                  }
                  const scheme = host.includes(":") ? "http" : "https";
                  try {
                    const id = await scannerApi.scanStart(`${scheme}://${host}/`, "crawl-audit");
                    setToast(`Crawl + audit started against ${host} (task #${id})`);
                  } catch (e) {
                    setToast(String(e));
                  }
                }}
              >
                Crawl paths view
              </button>
            </div>
            <input
              className="input grow"
              placeholder="Filter hosts and paths"
              value={filter}
              onChange={(e) => setFilter(e.target.value)}
              aria-label="Filter sitemap"
            />
            <button className="btn" onClick={loadSitemap}>
              Refresh
            </button>
          </div>

          {sitemap.length === 0 ? (
            <div className="blank" tabIndex={0}>
              <div className="blank-inner">
                <div className="blank-art" aria-hidden>
                  <svg width="96" height="72" viewBox="0 0 96 72" fill="none">
                    <rect x="8" y="14" width="34" height="24" rx="4" fill="#2a78bd" transform="rotate(-8 8 14)" />
                    <rect x="54" y="34" width="34" height="24" rx="4" fill="#2a78bd" transform="rotate(8 54 34)" />
                    <path d="M40 30 L56 40 M56 30 L40 40" stroke="currentColor" strokeWidth="2.5" strokeLinecap="round" />
                  </svg>
                </div>
                <h2>Site map URL view is empty</h2>
                <p>
                  The site map displays information about the contents of your target
                  applications. It is populated automatically as you browse through the
                  WebProxy Toolkit listener — point a browser at the proxy address below and
                  visit your targets.
                </p>
                <div className="blank-actions">
                  <button
                    className="btn primary"
                    onClick={async () => {
                      await navigator.clipboard.writeText("127.0.0.1:8080").catch(() => {});
                      setToast("Proxy address copied — set your browser proxy to it");
                    }}
                  >
                    Copy proxy address
                  </button>
                  <button
                    className="btn"
                    onClick={() =>
                      setToast(
                        "Start the listener in Proxy → Options, configure 127.0.0.1:8080 as your browser proxy, install ca.crt for HTTPS"
                      )
                    }
                  >
                    How to use
                  </button>
                </div>
              </div>
            </div>
          ) : (
            <div className="pane-body tree">
              <ul>
                {sitemap
                  .filter((h) => !filter || h.host.includes(filter))
                  .map((h) => (
                    <li key={h.host}>
                      <span className="host mono">{h.host}</span>{" "}
                      <span className="chip">
                        {h.children.reduce((a, c) => a + c.count, 0)}
                      </span>{" "}
                      <button className="btn mini" onClick={() => includeHost(h.host)}>
                        Add to scope
                      </button>{" "}
                      <button
                        className="btn mini"
                        onClick={() => {
                          setSelectedHost(h.host);
                          setSub("issues");
                        }}
                      >
                        View issues
                      </button>
                      <ul>
                        {h.children
                          .filter((c) => !filter || c.path.includes(filter))
                          .map((c) => (
                            <li key={c.path} className="mono muted">
                              {c.path} <span className="chip">{c.count}</span>
                            </li>
                          ))}
                      </ul>
                    </li>
                  ))}
              </ul>
            </div>
          )}
        </div>
      )}

      {sub === "scope" && (
        <div className="pane-body options-body">
          <section>
            <h2>Scope</h2>
            <p className="muted">
              Exclude rules win. With no include rules, everything is in scope. The scope
              drives the site map filter, in-scope history filter and (later) scanner targets.
            </p>
            <table className="history" style={{ marginBottom: 12 }}>
              <thead>
                <tr>
                  <th>Mode</th>
                  <th>Kind</th>
                  <th>Pattern</th>
                  <th aria-label="Actions">
                    <span className="sr-only">Actions</span>
                  </th>
                </tr>
              </thead>
              <tbody>
                {rules.map((r) => (
                  <tr key={r.id}>
                    <td>{r.mode}</td>
                    <td>{r.kind}</td>
                    <td className="mono" title={r.pattern}>
                      {r.pattern}
                    </td>
                    <td>
                      <button className="btn" onClick={() => removeRule(r.id)}>
                        Delete
                      </button>
                    </td>
                  </tr>
                ))}
                {rules.length === 0 && (
                  <tr>
                    <td colSpan={4} className="muted">
                      No scope rules — everything is in scope.
                    </td>
                  </tr>
                )}
              </tbody>
            </table>
            <div className="row" style={{ flexWrap: "wrap" }}>
              <select
                className="select"
                value={form.mode}
                onChange={(e) => setForm({ ...form, mode: e.target.value })}
                aria-label="Mode"
              >
                <option value="include">Include</option>
                <option value="exclude">Exclude</option>
              </select>
              <select
                className="select"
                value={form.kind}
                onChange={(e) => setForm({ ...form, kind: e.target.value })}
                aria-label="Kind"
              >
                <option value="host">Host</option>
                <option value="url_prefix">URL prefix</option>
                <option value="regex">Regex</option>
              </select>
              <input
                className="input grow"
                placeholder="example.com or https://host/path"
                value={form.pattern}
                onChange={(e) => setForm({ ...form, pattern: e.target.value })}
                aria-label="Pattern"
              />
              <button className="btn primary" onClick={addRule}>
                Add
              </button>
            </div>
          </section>
        </div>
      )}
      {sub === "issues" && (
        <div className="panel" style={{ display: "flex", flexDirection: "column", minHeight: 0 }}>
          <div className="toolbar">
            <label className="muted" htmlFor="target-issue-host">
              Host:
            </label>
            <select
              id="target-issue-host"
              className="select"
              value={selectedHost}
              onChange={(e) => setSelectedHost(e.target.value)}
              aria-label="Filter issues by host"
            >
              <option value="">All hosts</option>
              {sitemap.map((h) => (
                <option key={h.host} value={h.host}>
                  {h.host}
                </option>
              ))}
            </select>
          </div>
          <div className="grow min-h-0">
            <IssueTable host={selectedHost} setToast={setToast} />
          </div>
        </div>
      )}
    </div>
  );
}
