import { useEffect, useState } from "react";
import { X, Trash2 } from "lucide-react";
import { scannerApi, sevClass } from "../lib/scanner";
import type { CheckInfo, ScanConfig } from "../types";

/** Editor for a scan config; doubles as the issue-definitions catalog (check list). */
export default function ScanConfigEditor({
  config,
  onClose,
  onSaved,
  setToast,
}: {
  config: ScanConfig | null; // null = new
  onClose: () => void;
  onSaved: () => void;
  setToast: (t: string) => void;
}) {
  const [name, setName] = useState(config?.name ?? "");
  const [passive, setPassive] = useState(config?.passive_enabled ?? true);
  const [activeSel, setActiveSel] = useState<Set<string>>(
    new Set(config?.active_checks ?? []),
  );
  const [pages, setPages] = useState(config?.crawl_max_pages ?? 50);
  const [depth, setDepth] = useState(config?.crawl_max_depth ?? 3);
  const [threads, setThreads] = useState(config?.audit_threads ?? 4);
  const [throttle, setThrottle] = useState(config?.throttle_ms ?? 50);
  const [isDefault, setIsDefault] = useState(config?.is_default ?? false);
  const [checks, setChecks] = useState<CheckInfo[]>([]);

  useEffect(() => {
    scannerApi
      .checks()
      .then((all) => setChecks(all.filter((c) => c.kind === "active")))
      .catch(() => setChecks([]));
  }, []);

  function toggleCheck(id: string) {
    setActiveSel((cur) => {
      const next = new Set(cur);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
  }

  async function save() {
    if (!name.trim()) {
      setToast("Config name is required");
      return;
    }
    try {
      await scannerApi.scanConfigSave({
        ...(config ? { id: config.id } : {}),
        name: name.trim(),
        passive_enabled: passive,
        active_checks: [...activeSel],
        crawl_max_pages: pages,
        crawl_max_depth: depth,
        audit_threads: threads,
        throttle_ms: throttle,
        is_default: isDefault,
      });
      setToast(`Saved config "${name.trim()}"`);
      onSaved();
      onClose();
    } catch (e) {
      setToast(String(e));
    }
  }

  async function remove() {
    if (!config) return;
    try {
      await scannerApi.scanConfigDelete(config.id);
      setToast(`Deleted config "${config.name}"`);
      onSaved();
      onClose();
    } catch (e) {
      setToast(String(e));
    }
  }

  return (
    <div className="dlg-backdrop" role="presentation" onClick={onClose}>
      <div
        className="dlg wide"
        role="dialog"
        aria-modal="true"
        aria-label={config ? `Edit config ${config.name}` : "New scan config"}
        onClick={(e) => e.stopPropagation()}
      >
        <div className="dlg-title">
          {config ? `Edit config — ${config.name}` : "New scan config"}
          <span className="grow" />
          <button className="icon-btn" aria-label="Close" onClick={onClose}>
            <X size={14} />
          </button>
        </div>
        <div className="dlg-body scroll">
          <label className="muted" htmlFor="cfg-name">
            Name
          </label>
          <input
            id="cfg-name"
            className="input"
            value={name}
            onChange={(e) => setName(e.target.value)}
            autoFocus
          />

          <label className="switch">
            <input type="checkbox" checked={passive} onChange={(e) => setPassive(e.target.checked)} />
            Passive checks (on responses seen by the proxy)
          </label>
          <label className="switch">
            <input
              type="checkbox"
              checked={isDefault}
              onChange={(e) => setIsDefault(e.target.checked)}
            />
            Default config for new scans
          </label>

          <fieldset className="cfg-checks">
            <legend className="muted">Active checks (issue definitions catalog)</legend>
            {checks.map((c) => (
              <label key={c.id} className="cfg-check" title={c.description}>
                <input
                  type="checkbox"
                  checked={activeSel.has(c.id)}
                  onChange={() => toggleCheck(c.id)}
                />
                <span className={`sev-chip ${sevClass(c.severity)}`}>{c.severity}</span>
                <span className="cfg-check-name">{c.name}</span>
                <span className="muted cfg-check-desc">{c.description}</span>
              </label>
            ))}
            {checks.length === 0 && (
              <span className="muted">No active checks available.</span>
            )}
            <span className="muted">
              None selected = run all active checks.
            </span>
          </fieldset>

          <div className="cfg-grid">
            <div>
              <label className="muted" htmlFor="cfg-pages">
                Max pages
              </label>
              <input
                id="cfg-pages"
                className="input"
                type="number"
                min={1}
                value={pages}
                onChange={(e) => setPages(Number(e.target.value) || 1)}
              />
            </div>
            <div>
              <label className="muted" htmlFor="cfg-depth">
                Max depth
              </label>
              <input
                id="cfg-depth"
                className="input"
                type="number"
                min={0}
                value={depth}
                onChange={(e) => setDepth(Number(e.target.value) || 0)}
              />
            </div>
            <div>
              <label className="muted" htmlFor="cfg-threads">
                Threads
              </label>
              <input
                id="cfg-threads"
                className="input"
                type="number"
                min={1}
                value={threads}
                onChange={(e) => setThreads(Number(e.target.value) || 1)}
              />
            </div>
            <div>
              <label className="muted" htmlFor="cfg-throttle">
                Throttle (ms)
              </label>
              <input
                id="cfg-throttle"
                className="input"
                type="number"
                min={0}
                value={throttle}
                onChange={(e) => setThrottle(Number(e.target.value) || 0)}
              />
            </div>
          </div>
        </div>
        <div className="dlg-foot">
          {config && (
            <>
              <button className="btn danger" onClick={() => void remove()}>
                <Trash2 size={14} /> Delete
              </button>
              <span className="grow" />
            </>
          )}
          <button className="btn" onClick={onClose}>
            Cancel
          </button>
          <button className="btn primary" onClick={() => void save()}>
            Save
          </button>
        </div>
      </div>
    </div>
  );
}
