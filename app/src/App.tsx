import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import "./index.css";
import MenuBar from "./components/MenuBar";
import ToolTabs from "./components/ToolTabs";
import SearchDialog from "./components/SearchDialog";
import ErrorBoundary from "./components/ErrorBoundary";
import ProxyPanel from "./panels/ProxyPanel";
import RepeaterPanel from "./panels/RepeaterPanel";
import DecoderPanel from "./panels/DecoderPanel";
import TargetPanel from "./panels/TargetPanel";
import SettingsPanel from "./panels/SettingsPanel";
import DashboardPanel from "./panels/DashboardPanel";
import ComparerPanel from "./panels/ComparerPanel";
import SequencerPanel from "./panels/SequencerPanel";
import IntruderPanel from "./panels/IntruderPanel";
import LoggerPanel from "./panels/LoggerPanel";
import ExtensionsPanel from "./panels/ExtensionsPanel";
import CollaboratorPanel from "./panels/CollaboratorPanel";
import { emptyStatus, ProxyStatus, SearchHit, TabId, Transfer } from "./types";

export type SendTool = "repeater" | "intruder" | "decoder" | "comparer";

export default function App() {
  const [tab, setTab] = useState<TabId>("proxy");
  const [status, setStatus] = useState<ProxyStatus>(emptyStatus);
  const [toast, setToast] = useState("");
  const [toRepeater, setToRepeater] = useState<Transfer | null>(null);
  const [toIntruder, setToIntruder] = useState<Transfer | null>(null);
  const [toDecoder, setToDecoder] = useState<Transfer | null>(null);
  const [toComparer, setToComparer] = useState<Transfer[]>([]);
  const [searchOpen, setSearchOpen] = useState(false);
  const [jump, setJump] = useState<{ exId: number; seq: number } | null>(null);
  const [logFocus, setLogFocus] = useState<{ id: number; seq: number } | null>(null);

  const refreshStatus = useCallback(async () => {
    try {
      setStatus(await invoke<ProxyStatus>("proxy_status"));
    } catch {
      /* ignore */
    }
  }, []);

  useEffect(() => {
    refreshStatus();
    const un = listen<ProxyStatus>("proxy-status", (ev) => {
      if (ev.payload && typeof ev.payload === "object") setStatus(ev.payload as ProxyStatus);
    });
    return () => {
      un.then((f) => f());
    };
  }, [refreshStatus]);

  useEffect(() => {
    if (!toast) return;
    const id = window.setTimeout(() => setToast(""), 5000);
    return () => window.clearTimeout(id);
  }, [toast]);

  useEffect(() => {
    const TAB_ORDER: TabId[] = [
      "dashboard",
      "target",
      "proxy",
      "intruder",
      "repeater",
      "comparer",
      "collaborator",
      "sequencer",
      "decoder",
      "logger",
      "extensions",
    ];
    function onKey(e: KeyboardEvent) {
      if ((e.ctrlKey || e.metaKey) && e.shiftKey && (e.key === "f" || e.key === "F")) {
        e.preventDefault();
        setSearchOpen((v) => !v);
        return;
      }
      if ((e.ctrlKey || e.metaKey) && !e.shiftKey && !e.altKey) {
        const n = Number(e.key);
        if (n >= 1 && n <= TAB_ORDER.length) {
          e.preventDefault();
          setTab(TAB_ORDER[n - 1]);
        }
      }
    }
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  function jumpToHit(hit: SearchHit) {
    if (hit.source === "history") {
      setJump({ exId: hit.id, seq: Date.now() });
      setTab("proxy");
      setToast(`Opened request #${hit.id} in Proxy history`);
    } else {
      setLogFocus({ id: hit.id, seq: Date.now() });
      setTab("logger");
      setToast(`Opened log entry #${hit.id} in Logger`);
    }
  }

  function sendTo(tool: SendTool, msg: { raw: string; url: string; response?: string }) {
    const t: Transfer = { ...msg, seq: Date.now() };
    if (tool === "repeater") setToRepeater(t);
    if (tool === "intruder") setToIntruder(t);
    if (tool === "decoder") setToDecoder(t);
    if (tool === "comparer") setToComparer((prev) => [...prev.slice(-1), t]);
    setTab(tool);
    setToast(`Sent to ${tool[0].toUpperCase()}${tool.slice(1)}`);
  }

  async function startProxy() {
    try {
      await invoke("proxy_start", { host: "127.0.0.1", port: status.port || 8080 });
      setStatus(await invoke<ProxyStatus>("proxy_status"));
      setToast("Proxy listener started");
    } catch (e) {
      setToast(String(e));
    }
  }

  async function stopProxy() {
    try {
      await invoke("proxy_stop");
      setStatus(await invoke<ProxyStatus>("proxy_status"));
      setToast("Proxy stopped");
    } catch (e) {
      setToast(String(e));
    }
  }

  async function toggleIntercept() {
    try {
      setStatus(
        await invoke<ProxyStatus>("intercept_set", {
          enabled: !status.intercept,
          response: status.intercept_resp,
        })
      );
      setToast(!status.intercept ? "Intercept on" : "Intercept off");
    } catch (e) {
      setToast(String(e));
    }
  }

  return (
    <div className="app">
      <header>
        <h1 className="sr-only">WebProxy Toolkit</h1>
        <MenuBar
          status={status}
          onStart={startProxy}
          onStop={stopProxy}
          onToggleIntercept={toggleIntercept}
          onGoto={setTab}
          onAbout={() => setToast("WebProxy Toolkit 0.1.0 — local web security workbench")}
        />

        <ToolTabs
          tab={tab}
          setTab={setTab}
          onSearch={() => setSearchOpen(true)}
          onSettings={() => setTab("settings")}
        />
      </header>

      <SearchDialog open={searchOpen} onClose={() => setSearchOpen(false)} onJump={jumpToHit} />

      <main className="main">
        <ErrorBoundary key={tab}>
          {tab === "proxy" && (
            <ProxyPanel
              status={status}
              setStatus={setStatus}
              setToast={setToast}
              sendTo={sendTo}
              jumpTo={jump}
            />
          )}
          {tab === "repeater" && <RepeaterPanel incoming={toRepeater} setToast={setToast} />}
          {tab === "decoder" && <DecoderPanel incoming={toDecoder} setToast={setToast} />}
          {tab === "target" && <TargetPanel setToast={setToast} />}
          {tab === "settings" && <SettingsPanel setToast={setToast} />}
          {tab === "dashboard" && (
            <DashboardPanel status={status} setStatus={setStatus} setToast={setToast} />
          )}

          {tab === "intruder" && <IntruderPanel incoming={toIntruder} setToast={setToast} />}
          {tab === "comparer" && <ComparerPanel incoming={toComparer} setToast={setToast} />}
          {tab === "sequencer" && <SequencerPanel setToast={setToast} />}
          {tab === "extensions" && <ExtensionsPanel setToast={setToast} />}
          {tab === "logger" && <LoggerPanel focus={logFocus} />}
          {tab === "collaborator" && <CollaboratorPanel setToast={setToast} />}
        </ErrorBoundary>
      </main>

      <footer className="statusbar">
        <span className="status-item">
          <span className={`dot ${status.running ? "up" : "down"}`} />
          {status.running ? `Listener 127.0.0.1:${status.port}` : "Listener stopped"}
        </span>
        <span className="status-sep">|</span>
        <span className="status-item">
          Intercept {status.intercept || status.intercept_resp ? "on" : "off"}
          {status.intercept && status.intercept_resp ? " (req+resp)" : status.intercept ? " (req)" : status.intercept_resp ? " (resp)" : ""}
        </span>
        <span className="status-sep">|</span>
        <span className="status-item">Held {status.held_count}</span>
        <span className="status-sep">|</span>
        <span className="status-item">All traffic ({status.history_count})</span>
        <span className="grow" />
        {toast && (
          <span className="status-toast" role="status" aria-live="polite">
            {toast}
          </span>
        )}
        <span className="status-item">WebProxy Toolkit v0.1.0</span>
      </footer>
    </div>
  );
}
