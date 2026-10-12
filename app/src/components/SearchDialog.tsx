import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { SearchHit } from "../types";

type Props = {
  open: boolean;
  onClose: () => void;
  onJump: (hit: SearchHit) => void;
};

function Highlight({ text, q }: { text: string; q: string }) {
  const needle = q.trim().toLowerCase();
  if (!needle) return <>{text}</>;
  const lower = text.toLowerCase();
  const parts: React.ReactNode[] = [];
  let i = 0;
  let n = 0;
  for (;;) {
    const at = lower.indexOf(needle, i);
    if (at === -1) break;
    if (at > i) parts.push(text.slice(i, at));
    parts.push(<mark key={n++}>{text.slice(at, at + needle.length)}</mark>);
    i = at + needle.length;
  }
  if (i < text.length) parts.push(text.slice(i));
  return <>{parts}</>;
}

export default function SearchDialog({ open, onClose, onJump }: Props) {
  const [q, setQ] = useState("");
  const [hits, setHits] = useState<SearchHit[]>([]);
  const [sel, setSel] = useState(0);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [retryTick, setRetryTick] = useState(0);
  const inputRef = useRef<HTMLInputElement>(null);
  const listRef = useRef<HTMLDivElement>(null);
  const dialogRef = useRef<HTMLDivElement>(null);
  const triggerRef = useRef<HTMLElement | null>(null);
  const seqRef = useRef(0);

  useEffect(() => {
    if (!open) return;
    triggerRef.current = document.activeElement as HTMLElement | null;
    setQ("");
    setHits([]);
    setSel(0);
    setError(null);
    const t = window.setTimeout(() => inputRef.current?.focus(), 0);
    return () => {
      window.clearTimeout(t);
      triggerRef.current?.focus?.();
    };
  }, [open]);

  useEffect(() => {
    if (!open) return;
    const query = q.trim();
    if (!query) {
      seqRef.current += 1;
      setHits([]);
      setSel(0);
      setError(null);
      setBusy(false);
      return;
    }
    setBusy(true);
    const seq = ++seqRef.current;
    const t = window.setTimeout(() => {
      invoke<SearchHit[]>("global_search", { query, limit: 200 })
        .then((r) => {
          if (seq !== seqRef.current) return;
          setHits(r);
          setSel(0);
          setError(null);
        })
        .catch(() => {
          if (seq !== seqRef.current) return;
          setHits([]);
          setSel(0);
          setError("Search failed — the local index is unavailable. Try again.");
        })
        .finally(() => {
          if (seq === seqRef.current) setBusy(false);
        });
    }, 180);
    return () => window.clearTimeout(t);
  }, [q, open, retryTick]);

  useEffect(() => {
    const node = listRef.current?.children[sel];
    if (node instanceof HTMLElement) node.scrollIntoView({ block: "nearest" });
  }, [sel]);

  if (!open) return null;

  function jump(h: SearchHit) {
    onJump(h);
    onClose();
  }

  function onInputKeyDown(e: React.KeyboardEvent<HTMLInputElement>) {
    if (e.key === "Escape") {
      e.preventDefault();
      onClose();
    } else if (e.key === "ArrowDown") {
      e.preventDefault();
      setSel((s) => Math.min(s + 1, Math.max(hits.length - 1, 0)));
    } else if (e.key === "ArrowUp") {
      e.preventDefault();
      setSel((s) => Math.max(s - 1, 0));
    } else if (e.key === "Enter") {
      e.preventDefault();
      const h = hits[sel];
      if (h) jump(h);
    }
  }

  function onDialogKeyDown(e: React.KeyboardEvent<HTMLDivElement>) {
    if (e.key === "Escape") {
      e.preventDefault();
      onClose();
      return;
    }
    if (e.key !== "Tab") return;
    const root = dialogRef.current;
    if (!root) return;
    const focusables = Array.from(
      root.querySelectorAll<HTMLElement>(
        'input, button, [href], [tabindex]:not([tabindex="-1"])',
      ),
    ).filter((el) => !el.hasAttribute("disabled"));
    if (focusables.length === 0) return;
    const first = focusables[0];
    const last = focusables[focusables.length - 1];
    if (e.shiftKey && document.activeElement === first) {
      e.preventDefault();
      last.focus();
    } else if (!e.shiftKey && document.activeElement === last) {
      e.preventDefault();
      first.focus();
    }
  }

  const meta = error
    ? error
    : busy
      ? "Searching…"
      : !q.trim()
        ? "Type to search"
        : `${hits.length} result${hits.length === 1 ? "" : "s"}`;

  return (
    <div
      className="searchdlg-overlay"
      role="presentation"
      onMouseDown={(e) => {
        if (e.target === e.currentTarget) onClose();
      }}
    >
      <div
        ref={dialogRef}
        className="searchdlg"
        role="dialog"
        aria-modal="true"
        aria-label="Search"
        onKeyDown={onDialogKeyDown}
      >
        <div className="searchdlg-head">
          <input
            ref={inputRef}
            className="input grow"
            placeholder="Search HTTP history and application log…"
            value={q}
            onChange={(e) => setQ(e.target.value)}
            onKeyDown={onInputKeyDown}
            role="combobox"
            aria-label="Search query"
            aria-expanded="true"
            aria-controls="searchdlg-options"
            aria-activedescendant={hits.length ? `opt-${sel}` : undefined}
            aria-describedby="searchdlg-meta"
          />
          <button className="btn" onClick={onClose} aria-label="Close search">
            Close
          </button>
        </div>
        <div
          id="searchdlg-meta"
          className={`searchdlg-meta muted ${error ? "searchdlg-err" : ""}`}
          aria-live="polite"
        >
          {meta}
        </div>
        <div className="searchdlg-list">
          <div
            ref={listRef}
            id="searchdlg-options"
            role="listbox"
            aria-label="Search results"
            className="searchdlg-options"
          >
            {hits.map((h, i) => (
              <div
                key={`${h.source}-${h.id}`}
                id={`opt-${i}`}
                role="option"
                aria-selected={i === sel}
                className={`searchdlg-hit ${i === sel ? "selected" : ""}`}
                onMouseEnter={() => setSel(i)}
                onClick={() => jump(h)}
              >
                <span className={`src-badge ${h.source === "history" ? "http" : "log"}`}>
                  {h.source === "history" ? "HTTP" : "LOG"}
                </span>
                <span className="hit-main">
                  <span className="hit-title mono">
                    <Highlight text={h.title} q={q} />
                  </span>
                  <span className="hit-detail muted">{h.detail}</span>
                  {h.snippet ? (
                    <span className="hit-snippet mono">
                      <Highlight text={h.snippet} q={q} />
                    </span>
                  ) : null}
                </span>
              </div>
            ))}
          </div>
          {error && (
            <div className="searchdlg-retry">
              <button
                className="btn mini"
                onClick={() => {
                  setError(null);
                  setRetryTick((t) => t + 1);
                }}
              >
                Retry
              </button>
            </div>
          )}
          {!busy && !error && q.trim() && hits.length === 0 && (
            <div className="empty">No matches for “{q.trim()}”.</div>
          )}
          {!q.trim() && (
            <div className="empty">
              Type to search URLs, hosts, methods, status codes and the application log.
            </div>
          )}
        </div>
        <div className="searchdlg-foot muted">
          Enter opens · ↑ ↓ move · Esc closes
        </div>
      </div>
    </div>
  );
}
