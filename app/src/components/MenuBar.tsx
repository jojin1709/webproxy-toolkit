import { useEffect, useRef, useState } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { getVersion } from "@tauri-apps/api/app";
import { Zap } from "lucide-react";
import { ProxyStatus, TabId } from "../types";

type Props = {
  status: ProxyStatus;
  onStart: () => void;
  onStop: () => void;
  onToggleIntercept: () => void;
  onGoto: (t: TabId) => void;
  onAbout: () => void;
};

type MenuDef = { label: string; items: () => React.ReactNode };

export default function MenuBar({ status, onStart, onStop, onToggleIntercept, onGoto, onAbout }: Props) {
  const [open, setOpen] = useState<string | null>(null);
  const [version, setVersion] = useState<string>("");
  const bar = useRef<HTMLDivElement>(null);

  useEffect(() => {
    getVersion()
      .then(setVersion)
      .catch(() => setVersion(""));
  }, []);

  useEffect(() => {
    if (!open) return;
    const close = (e: MouseEvent) => {
      if (bar.current && bar.current.contains(e.target as Node)) return;
      setOpen(null);
    };
    const esc = (e: KeyboardEvent) => e.key === "Escape" && setOpen(null);
    window.addEventListener("mousedown", close);
    window.addEventListener("keydown", esc);
    return () => {
      window.removeEventListener("mousedown", close);
      window.removeEventListener("keydown", esc);
    };
  }, [open]);

  const item = (label: string, action?: () => void, disabled = false, hint?: string) => (
    <button
      key={label}
      className="menu-item"
      role="menuitem"
      disabled={disabled}
      onClick={() => {
        setOpen(null);
        action?.();
      }}
    >
      <span>{label}</span>
      {hint && <span className="menu-hint">{hint}</span>}
    </button>
  );

  const menus: MenuDef[] = [
    {
      label: "Project",
      items: () => (
        <>
          {item("Open project…", undefined, true, "Phase 5")}
          {item("Save project now", undefined, true, "Phase 5")}
          <div className="menu-sep" />
          {item("Exit", () => getCurrentWindow().close())}
        </>
      ),
    },
    {
      label: "Proxy",
      items: () => (
        <>
          {status.running ? item("Stop listener", onStop) : item("Start listener", onStart)}
          {item(
            status.intercept ? "Disable interception" : "Enable interception",
            onToggleIntercept
          )}
          <div className="menu-sep" />
          {item("HTTP history", () => onGoto("proxy"))}
          {item("Site map", () => onGoto("target"))}
        </>
      ),
    },
    {
      label: "View",
      items: () => (
        <>
          {item("Dashboard", () => onGoto("dashboard"))}
          {item("Repeater", () => onGoto("repeater"))}
          {item("Decoder", () => onGoto("decoder"))}
          {item("Settings", () => onGoto("settings"))}
        </>
      ),
    },
    {
      label: "Help",
      items: () => <>{item("About WebProxy Toolkit", onAbout)}</>,
    },
  ];

  return (
    <div className="menubar" ref={bar}>
      <button
        className="brand"
        onClick={() => setOpen(open === "brand" ? null : "brand")}
        aria-haspopup="menu"
        aria-expanded={open === "brand"}
      >
        <Zap size={14} className="brand-mark" fill="currentColor" />
        WebProxy
      </button>
      {open === "brand" && (
        <div className="menu-drop" style={{ left: 4 }} role="menu">
          {item("About WebProxy Toolkit", onAbout)}
          <div className="menu-sep" />
          {item("Exit", () => getCurrentWindow().close())}
        </div>
      )}
      {menus.map((m) => (
        <div key={m.label} className="menu-root">
          <button
            className={`menu-label ${open === m.label ? "active" : ""}`}
            aria-haspopup="menu"
            aria-expanded={open === m.label}
            onClick={() => setOpen(open === m.label ? null : m.label)}
            onMouseEnter={() => open && setOpen(m.label)}
          >
            {m.label}
          </button>
          {open === m.label && (
            <div className="menu-drop" role="menu">
              {m.items()}
            </div>
          )}
        </div>
      ))}
      <span className="menubar-title">
        WebProxy Toolkit{version ? ` v${version}` : ""} — local workspace
      </span>
    </div>
  );
}
