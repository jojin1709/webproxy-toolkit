# Phase 6: Extensions — JS plugin API (rquickjs) — Implementation Plan

Status: approved (user: "ok do it go for phase 6"). Execute 6a → 6b → 6c; each ends green
(cargo test/build, tsc, vite build, shot.py, a11y.py).

Clean-room: our own `wpt` API surface; no Burp Java/BApp compatibility (features.csv cut).
Engine: `rquickjs` 0.14 (embedded ES2020, sandboxed — no fs/net from JS).

## 6a — Engine + store + Extensions panel

### Schema
```sql
CREATE TABLE IF NOT EXISTS extensions (
  id         INTEGER PRIMARY KEY AUTOINCREMENT,
  name       TEXT NOT NULL,
  path       TEXT NOT NULL,
  enabled    INTEGER NOT NULL DEFAULT 1,
  error      TEXT,
  loaded_at  TEXT,
  created_at TEXT NOT NULL
);
```

### Backend (`extensions.rs`, new)
- `ExtRuntime { inner: Mutex<Inner> }` on `ProxyState` — one QuickJS `Runtime`+`Context`.
- Bootstrap defines global `wpt`: `log(msg, level?)`, `registerProxyListener(fn)`,
  `registerHttpHandler(fn)`, `registerContextMenu({label,scope}, fn)`, `registerTab(title, fn)`.
  (6a ships `log` + registry stubs; hooks fire in 6b.)
- Hooks stored as `Persistent<Function>` per extension id; load = eval source, call nothing;
  unload/remove = drop persistents.
- `load_extension(path)` → read file, eval, capture top-level error → `error` column.
- Enable/disable: toggle runs/removes hooks from the active registry (re-eval on enable).
- `wpt.log` → Logger row tool=`extensions` via `log_event`, level default info.
- Commands: `extension_list`, `extension_load` (path), `extension_set_enabled`,
  `extension_delete`, `extension_read_file` (load .js into editor). Register all.
- Unit tests: bootstrap eval + log capture (headless runtime), enable/disable registry.

### Frontend
- `ExtensionsPanel.tsx` replaces placeholder: table (name, enabled switch, error, loaded_at,
  actions: Enable/Disable, Reload, Remove), "Load extension…" (path input + file picker via
  `extension_read_file`), embedded JS source viewer (read-only textarea) for selected row.
- types: `ExtensionInfo`; lib invoke wrappers.

### QA
- mock.js: extension seeds (one enabled ok, one errored) + handlers; shot.py `46-extensions`;
  a11y `extensions`.

## 6b — Hook wiring (http handler, proxy listener, context menu)

### Backend
- `proxyListener` fire-and-forget in `record_exchange` (spawn_blocking; observe-only payload:
  exchange JSON; errors logged, never panic the proxy).
- `httpHandler` synchronous in relay path (where match/replace applies): request phase before
  upstream, response phase before client. Contract: `fn(message) -> message`, message =
  `{ request: {method,url,headers:[[k,v]],body}, response: {...}|null }`. Return value rebuilds
  raw message; invalid/error → original message (proxy never breaks).
- `contextMenu`: registry list exposed to UI (`extension_list` returns items);
  command `extension_context_invoke(ext_id, menu_id, message)` runs the callback with the
  message JSON (fire result to log).
- Throttle: each hook invocation guarded by per-extension error latch (disable after N=5
  consecutive failures, log once).

### Frontend
- ProxyPanel context menu: "Extensions" section with registered items (labels from registry);
  click → `extension_context_invoke` with raw request/response of the selected exchange.
- Repeater "Custom actions" button enabled → same items for the active tab request.

### QA
- mock: context-menu items seed + invoke handler; shot.py `47-ext-context-menu`;
  a11y `ext-context-menu`.

## 6c — Custom tabs + polish + docs

### Frontend
- `registerTab(title, fn)` → Extensions panel sub-tabs; content = HTML string from `fn()`
  rendered in sandboxed `<iframe sandbox srcdoc>` (no script exec unless `sandbox` allows —
  keep srcdoc with scripts disabled: pure HTML report tabs).
- Extension error console section (last errors per extension, from `extension_list.error`).

### Docs / ship
- README status row 6 → done; Extensions feature bullet; features.csv rows 41–42 clone=yes
  ("our own JS API; no BApp/Java compatibility"); custom columns row 46 note updated.
- Commit + push.

## Verification gates (end of each sub-phase)
`cargo test` (lib), `cargo build`, `npx tsc --noEmit`, `npm run build`, `python qa\shot.py`,
`python qa\a11y.py`.

## Explicit scope cuts
- No BApp/Java/Python API compatibility (impossible to clone legally/technically — own API).
- No message-editor custom columns / custom scanner checks in JS (v1: http handler +
  listener + context menu + tabs cover the core surface; catalog stays Rust).
- No async/promises in hook API (sync return only; `wpt.log` for output).
- No extension store/marketplace, no update checker.
- Tabs are static HTML reports (no JS inside the iframe).
- No per-extension settings UI (v1: source + enable state only).
