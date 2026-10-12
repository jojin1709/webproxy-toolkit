import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { LogEntry, LogFilter } from "../types";

const TOOL_FILTERS: [string, string][] = [
  ["", "All tools"],
  ["proxy", "Proxy"],
  ["repeater", "Repeater"],
  ["sequencer", "Sequencer"],
  ["scope", "Scope"],
  ["match", "Match / replace"],
  ["settings", "Settings"],
  ["system", "System"],
];

const LEVEL_FILTERS: [string, string][] = [
  ["", "All levels"],
  ["info", "Info"],
  ["warn", "Warning"],
  ["error", "Error"],
];

function levelChip(level: string): string {
  if (level === "error") return "chip err";
  if (level === "warn") return "chip warn";
  return "chip ok";
}

function fmtTs(ts: string): string {
  const d = new Date(ts);
  return Number.isNaN(d.getTime()) ? ts : d.toLocaleTimeString();
}

export default function LogTable({
  limit = 200,
  focus = null,
}: {
  limit?: number;
  focus?: { id: number; seq: number } | null;
}) {
  const [entries, setEntries] = useState<LogEntry[]>([]);
  const [toolF, setToolF] = useState("");
  const [levelF, setLevelF] = useState("");
  const [query, setQuery] = useState("");
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState("");

  const load = useCallback(async () => {
    try {
      const filter: LogFilter = {
        tool: toolF || null,
        level: levelF || null,
        query: query || null,
        limit,
      };
      const rows = await invoke<LogEntry[]>("log_list", { filter });
      setEntries(Array.isArray(rows) ? rows : []);
      setErr("");
    } catch (e) {
      setErr(String(e));
    }
  }, [toolF, levelF, query, limit]);

  useEffect(() => {
    load();
  }, [load]);

  // Bring a search-jumped-to entry into view once it has loaded.
  useEffect(() => {
    if (!focus) return;
    const el = document.querySelector(`tr[data-log-id="${focus.id}"]`);
    if (el instanceof HTMLElement) el.scrollIntoView({ block: "center" });
  }, [focus, entries]);

  useEffect(() => {
    const un = listen<LogEntry>("log-added", (ev) => {
      const e = ev.payload;
      if (!e) return;
      if (toolF && e.tool !== toolF) return;
      if (levelF && e.level !== levelF) return;
      if (query && !e.message.toLowerCase().includes(query.toLowerCase())) return;
      setEntries((prev) => [e, ...prev.filter((p) => p.id !== e.id)].slice(0, limit));
    });
    const id = window.setInterval(() => {
      load();
    }, 5000);
    return () => {
      un.then((f) => f());
      window.clearInterval(id);
    };
  }, [load, toolF, levelF, query, limit]);

  async function clear() {
    setBusy(true);
    try {
      await invoke("log_clear");
      await load();
    } catch (e) {
      setErr(String(e));
    } finally {
      setBusy(false);
    }
  }

  return (
    <>
      <div className="toolbar">
        <span className="muted">Tool</span>
        <select
          className="select"
          aria-label="Filter by tool"
          value={toolF}
          onChange={(e) => setToolF(e.target.value)}
        >
          {TOOL_FILTERS.map(([v, l]) => (
            <option key={v} value={v}>
              {l}
            </option>
          ))}
        </select>
        <span className="muted">Level</span>
        <select
          className="select"
          aria-label="Filter by level"
          value={levelF}
          onChange={(e) => setLevelF(e.target.value)}
        >
          {LEVEL_FILTERS.map(([v, l]) => (
            <option key={v} value={v}>
              {l}
            </option>
          ))}
        </select>
        <input
          className="input grow"
          placeholder="Filter messages"
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          aria-label="Filter log messages"
        />
        <button className="btn" onClick={load}>
          Refresh
        </button>
        <button className="btn" onClick={clear} disabled={busy}>
          Clear
        </button>
        {err && <span className="chip err">{err}</span>}
        <span className="muted">{entries.length} entries</span>
      </div>
      <div className="table-wrap">
        {entries.length === 0 ? (
          <div className="empty">No log entries yet — tool activity will appear here as you work.</div>
        ) : (
          <table className="history">
            <thead>
              <tr>
                <th>Time</th>
                <th>Tool</th>
                <th>Level</th>
                <th>Message</th>
              </tr>
            </thead>
            <tbody>
              {entries.map((e) => (
                <tr
                  key={e.id}
                  data-log-id={e.id}
                  className={focus?.id === e.id ? "focused" : ""}
                >
                  <td>{fmtTs(e.ts)}</td>
                  <td>{e.tool}</td>
                  <td>
                    <span className={levelChip(e.level)}>{e.level}</span>
                  </td>
                  <td className="mono">{e.message}</td>
                </tr>
              ))}
            </tbody>
          </table>
        )}
      </div>
    </>
  );
}
