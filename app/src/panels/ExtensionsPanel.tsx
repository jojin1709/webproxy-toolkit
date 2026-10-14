import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { ExtensionView, ExtHookInfo, ExtTabInfo } from "../types";
import { tabListKeyDown } from "../components/tabKeys";

export default function ExtensionsPanel({ setToast }: { setToast: (t: string) => void }) {
  const [rows, setRows] = useState<ExtensionView[]>([]);
  const [hooks, setHooks] = useState<ExtHookInfo>({ context_menus: [], tabs: [] });
  const [path, setPath] = useState("");
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState("");
  const [selected, setSelected] = useState<number | null>(null);
  const [activeTab, setActiveTab] = useState<string | null>(null);
  const [tabHtml, setTabHtml] = useState("");
  const [tabErr, setTabErr] = useState("");

  const refresh = useCallback(async () => {
    try {
      const [list, info] = await Promise.all([
        invoke<ExtensionView[]>("extension_list"),
        invoke<ExtHookInfo>("extension_hooks"),
      ]);
      setRows(Array.isArray(list) ? list : []);
      setHooks(info ?? { context_menus: [], tabs: [] });
    } catch (e) {
      setErr(String(e));
    }
  }, []);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  async function load() {
    if (!path.trim()) {
      setErr("Enter the path to a .js extension file");
      return;
    }
    setBusy(true);
    setErr("");
    try {
      await invoke<number>("extension_load", { path: path.trim(), extId: null });
      setPath("");
      setToast("Extension loaded");
      await refresh();
    } catch (e) {
      setErr(String(e));
    } finally {
      setBusy(false);
    }
  }

  async function reload(ext: ExtensionView) {
    setBusy(true);
    setErr("");
    try {
      await invoke<number>("extension_load", { path: ext.path, extId: ext.id });
      setToast(`Reloaded ${ext.name}`);
      await refresh();
    } catch (e) {
      setErr(String(e));
    } finally {
      setBusy(false);
    }
  }

  async function toggle(ext: ExtensionView, next: boolean) {
    setErr("");
    try {
      await invoke<void>("extension_set_enabled", { extId: ext.id, enabled: next });
      setToast(next ? `Enabled ${ext.name}` : `Disabled ${ext.name}`);
      await refresh();
    } catch (e) {
      setErr(String(e));
    }
  }

  async function remove(ext: ExtensionView) {
    setErr("");
    try {
      await invoke<void>("extension_delete", { extId: ext.id });
      setToast(`Removed ${ext.name}`);
      await refresh();
    } catch (e) {
      setErr(String(e));
    }
  }

  const tabKey = (t: ExtTabInfo) => `${t.ext_id}:${t.tab_id}`;

  async function openTab(t: ExtTabInfo) {
    const key = tabKey(t);
    setActiveTab(key);
    setTabErr("");
    try {
      const html = await invoke<string>("extension_tab_render", {
        extId: t.ext_id,
        tabId: t.tab_id,
      });
      setTabHtml(html ?? "");
    } catch (e) {
      setTabHtml("");
      setTabErr(String(e));
    }
  }

  const tabs = hooks.tabs ?? [];

  // open the first registered tab by default
  useEffect(() => {
    if (tabs.length === 0) return;
    if (activeTab != null && tabs.some((t) => tabKey(t) === activeTab)) return;
    void openTab(tabs[0]);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [tabs, activeTab]);

  const menus = hooks.context_menus ?? [];

  return (
    <div className="panel" role="tabpanel" aria-label="Extensions" id="panel-extensions">
      <div className="toolbar">
        <label className="grow" style={{ display: "flex", gap: 8, alignItems: "center" }}>
          <span className="sr-only">Extension file path</span>
          <input
            className="input grow"
            placeholder="Path to extension .js file (e.g. C:\\ext\\hello.js)"
            value={path}
            onChange={(e) => setPath(e.target.value)}
            aria-label="Extension file path"
            onKeyDown={(e) => {
              if (e.key === "Enter") void load();
            }}
          />
        </label>
        <button className="btn primary" onClick={() => void load()} disabled={busy}>
          Load extension…
        </button>
        <button className="btn" onClick={() => void refresh()} disabled={busy}>
          Refresh
        </button>
      </div>

      {err && (
        <div style={{ padding: "8px 12px" }} role="alert">
          <span className="chip err">{err}</span>
        </div>
      )}

      {rows.length === 0 ? (
        <div className="blank" tabIndex={0}>
          <div className="blank-inner">
            <h2>No extensions loaded</h2>
            <p>
              Extensions are JavaScript files that use the <code>wpt</code> API to register
              proxy listeners, HTTP handlers, context-menu items and custom tabs. Load one above,
              or drop a <code>.js</code> file path in and press Enter.
            </p>
            <p>
              Example: <code>wpt.registerTab(&apos;Hello&apos;, function () &#123; return
              &apos;&lt;h1&gt;Hi&lt;/h1&gt;&apos;; &#125;);</code>
            </p>
          </div>
        </div>
      ) : (
        <div className="table-wrap grow min-h-0" role="region" aria-label="Extensions" tabIndex={0}>
        <table className="history" aria-label="Extensions">
          <thead>
            <tr>
              <th scope="col">Name</th>
              <th scope="col">Status</th>
              <th scope="col">Hooks</th>
              <th scope="col">Loaded</th>
              <th scope="col">Actions</th>
            </tr>
          </thead>
          <tbody>
            {rows.map((ext) => {
              const menuCount = menus.filter((m) => m.ext_id === ext.id).length;
              const tabCount = tabs.filter((t) => t.ext_id === ext.id).length;
              return (
                <tr
                  key={ext.id}
                  className={selected === ext.id ? "selected" : ""}
                  onClick={() => setSelected(ext.id)}
                >
                  <td>
                    <div title={ext.path}>{ext.name}</div>
                  </td>
                  <td>
                    {ext.error ? (
                      <span className="chip err" title={ext.error}>
                        error
                      </span>
                    ) : ext.enabled ? (
                      <span className="chip ok">enabled</span>
                    ) : (
                      <span className="chip">disabled</span>
                    )}
                  </td>
                  <td>
                    {menuCount} menu{menuCount === 1 ? "" : "s"}, {tabCount} tab
                    {tabCount === 1 ? "" : "s"}
                    {ext.hook_errors > 0 && (
                      <span className="chip err" style={{ marginLeft: 6 }} title={ext.error ?? ""}>
                        {ext.hook_errors} error{ext.hook_errors === 1 ? "" : "s"}
                      </span>
                    )}
                  </td>
                  <td>{ext.loaded_at ?? "—"}</td>
                  <td>
                    <label className="switch" style={{ marginRight: 8 }}>
                      <input
                        type="checkbox"
                        checked={ext.enabled}
                        onChange={(e) => void toggle(ext, e.target.checked)}
                        aria-label={`Enable ${ext.name}`}
                      />
                      Enabled
                    </label>
                    <button
                      className="btn"
                      onClick={() => void reload(ext)}
                      disabled={busy}
                      style={{ marginRight: 6 }}
                    >
                      Reload
                    </button>
                    <button className="btn" onClick={() => void remove(ext)} disabled={busy}>
                      Remove
                    </button>
                  </td>
                </tr>
              );
            })}
          </tbody>
        </table>
        </div>
      )}

      {menus.length > 0 && (
        <div style={{ padding: "12px 12px 0" }}>
          <h3 style={{ margin: "0 0 8px", fontSize: 13, opacity: 0.85 }}>
            Registered context menus
          </h3>
          <ul style={{ margin: 0, paddingLeft: 18 }}>
            {menus.map((m) => (
              <li key={`m-${m.ext_id}-${m.menu_id}`}>
                Menu: {m.label} <span style={{ opacity: 0.6 }}>(scope: {m.scope})</span>
              </li>
            ))}
          </ul>
        </div>
      )}

      {tabs.length > 0 && (
        <div
          className="subtabs"
          role="tablist"
          aria-label="Extension tabs"
          style={{ marginTop: 12 }}
          onKeyDown={(e) => {
            const idx = tabs.findIndex((t) => tabKey(t) === activeTab);
            tabListKeyDown(e, Math.max(idx, 0), tabs.map(() => false), (next) =>
              void openTab(tabs[next])
            );
          }}
        >
          {tabs.map((t) => {
            const key = tabKey(t);
            return (
              <button
                key={key}
                role="tab"
                aria-selected={activeTab === key}
                tabIndex={activeTab === key ? 0 : -1}
                className={activeTab === key ? "active" : ""}
                onClick={() => void openTab(t)}
              >
                {t.title}
              </button>
            );
          })}
        </div>
      )}

      {tabErr && (
        <div style={{ padding: "8px 12px 0" }} role="alert">
          <span className="chip err">{tabErr}</span>
        </div>
      )}

      {activeTab != null && tabs.length > 0 && (
        <iframe
          title={`Extension tab: ${activeTab}`}
          sandbox=""
          srcDoc={tabHtml}
          style={{
            margin: "8px 12px 12px",
            width: "calc(100% - 24px)",
            height: 260,
            border: "1px solid var(--color-border)",
            borderRadius: 4,
            background: "#fff",
          }}
        />
      )}

      {rows.some((r) => r.error) && (
        <div
          style={{ padding: "0 12px 12px" }}
          role="region"
          aria-label="Extension error console"
        >
          <h3 style={{ margin: "12px 0 6px", fontSize: 13 }}>Errors</h3>
          {rows
            .filter((r) => r.error)
            .map((r) => (
              <div key={r.id} style={{ marginBottom: 4 }}>
                <b>{r.name}</b>: <span className="chip err">{r.error}</span>
              </div>
            ))}
        </div>
      )}
    </div>
  );
}
