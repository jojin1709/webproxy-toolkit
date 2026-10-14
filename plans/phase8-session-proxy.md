# Phase 8: Session handling, upstream proxy, TLS options, multi-listener, shortcuts — Plan

Status: approved (user: "ok"). Execute 8a → 8b → 8c; each ends green
(cargo test/build, tsc, vite build, shot.py, a11y.py).

Covers features.csv rows 15, 17, 18, 38, 45 (+ Repeater layout 22 already partially shipped —
stacked h/v toggle). Cuts noted per sub-phase.

## 8a — Upstream proxy chain + TLS pass-through + multi-listener

### Schema
```sql
CREATE TABLE IF NOT EXISTS upstream_rules (
  id       INTEGER PRIMARY KEY AUTOINCREMENT,
  position INTEGER NOT NULL,
  host_pattern TEXT NOT NULL,   -- substring / wildcard match on target host
  proxy_host  TEXT NOT NULL,
  proxy_port  INTEGER NOT NULL,
  use_tls     INTEGER NOT NULL DEFAULT 0, -- HTTPS to the upstream proxy itself
  enabled   INTEGER NOT NULL DEFAULT 1
);
CREATE TABLE IF NOT EXISTS proxy_settings (
  key   TEXT PRIMARY KEY,
  value TEXT NOT NULL
);
-- keys: pass_through_hosts (JSON array of host patterns), multi_listeners (JSON array)
```

### Backend
- `upstream_rules` CRUD + `proxy_settings` get/set (db.rs).
- Relay: on outbound connect (both `handle_conn` plain and `handle_connect` tunnel), walk
  enabled upstream rules in `position` order; first host-pattern match → connect to that
  proxy (plain or TLS) and issue `CONNECT host:port` for tunnels, or absolute-URI forward
  for plain. No match → direct connect (current behaviour).
- TLS pass-through: `pass_through_hosts` list; `handle_connect` for a matching host skips
  MITM (no leaf mint) and instead raw-TCP tunnels client↔upstream (with upstream rule
  applied). Non-matching → current MITM.
- Multi-listener: `ProxyState.listeners: Mutex<Vec<ListenerSlot>>` where each slot has its
  own bound port + shutdown channel. `proxy_start` keeps the primary; new commands
  `listener_add(host,port)`, `listener_remove(id)`, `listener_list`. All listeners share one
  `handle_conn`/`handle_connect` (same history/intercept/scope). Primary `running`/`port`
  stay as-is for status bar.
- Commands: `upstream_list/add/delete`, `proxy_setting_get/set`, `pass_through_list/set`,
  `listener_add/remove/list`. Register all.
- Unit tests: upstream host-pattern match, pass-through decision, listener slot add/remove.

### Frontend
- Proxy Options gains sections: "Upstream proxies" (table + add form), "TLS pass-through"
  (host-pattern chips + add/remove), "Extra listeners" (table + add/remove). types for each.

### QA
- mock.js: upstream rule + pass-through + extra-listener seeds + handlers; shot 51-options-upstream;
  a11y `proxy-options-upstream`.

## 8b — Session handling rules + macros

### Schema
```sql
CREATE TABLE IF NOT EXISTS session_macros (
  id    INTEGER PRIMARY KEY AUTOINCREMENT,
  name  TEXT NOT NULL,
  requests TEXT NOT NULL      -- JSON array of raw request strings
);
CREATE TABLE IF NOT EXISTS session_rules (
  id       INTEGER PRIMARY KEY AUTOINCREMENT,
  position INTEGER NOT NULL,
  name     TEXT NOT NULL,
  tool     TEXT NOT NULL DEFAULT 'any',   -- any|repeater|intruder|scanner|proxy
  match_host TEXT NOT NULL DEFAULT '',    -- '' = any
  macro_id INTEGER,                        -- FK session_macros.id
  updates  TEXT NOT NULL DEFAULT '[]',     -- JSON array of {kind, name, value_or_extract}
  enabled  INTEGER NOT NULL DEFAULT 1
);
```
`updates[].kind` ∈ `set-header` | `set-cookie` | `set-param`; each has `name` and either a
literal `value` or an `extract` = `{from: "cookie"|"header"|"body", name|regex}` pulled from
the last macro response.

### Backend
- db.rs CRUD for macros + rules.
- `session.rs` (new): `run_session_rules(state, tool, host, request_raw) -> request_raw` —
  for each enabled rule matching tool+host (position order), if it has a macro: replay
  macro requests (via the same outbound path as repeater_send), take the last response,
  run each update's extract (cookie/header/regex) → apply set-header/cookie/param to the
  incoming request. No macro → apply literal updates only.
- Wire into the relay: before sending upstream in `handle_conn`/`handle_connect` (and in
  `repeater_send`), call `run_session_rules`.
- Commands: `session_macro_list/add/delete`, `session_rule_list/add/delete`, `session_rule_run`
  (dry-run preview). Register all.
- Unit tests: rule match by tool/host, extract-from-cookie/header/body, header/param apply.

### Frontend
- New "Session" sub-tab in Proxy Options (or a dedicated Settings section): macros table
  (name, request count, edit), rules table (position, name, tool, host, macro, updates,
  enable), add-rule form with update rows (kind + name + value/extract). types added.

### QA
- mock.js: 1 macro + 2 rules seeds + handlers; shot 52-session-rules; a11y `session-rules`.

## 8c — Shortcuts + layout + docs/ship

### Frontend
- Global shortcut registry (App-level keydown): Ctrl+Shift+F search (exists), Ctrl+1..9
  switch tool tab, Ctrl+R send-to-Repeater (where applicable), Esc close dialogs. A
  "Keyboard" note row in Settings listing them. (Row 45 "should" — ship a solid subset.)
- Repeater layout: already has stacked h/v toggle (row 22 partial). Add a "tabs" hint only
  if cheap; otherwise leave and note cut.

### Docs / ship
- README status row 8 → done; bullets for upstream/TLS/multi-listener/session/shortcuts.
- features.csv rows 15/17/18/38 clone=yes; row 45 clone=yes (subset); row 22 note.
- Commit + push.

## Verification gates (end of each sub-phase)
`cargo test` (lib), `npx tsc --noEmit`, `npm run build`, `python qa\shot.py`, `python qa\a11y.py`.

## Explicit scope cuts
- Upstream: no per-rule auth (no upstream proxy username/password in v1).
- TLS pass-through: host-pattern only (no client-cert / protocol-version matrix).
- Multi-listener: listeners share all options; no per-port match/replace or scope.
- Session: macros are ordered request lists (no conditional/loop logic); extract regex is
  first-match only; no "check session still valid" loop.
- Shortcuts: fixed subset, no user rebinding UI.
- Repeater "tabs" layout mode (row 22) not shipped; h/v stacked toggle exists.
