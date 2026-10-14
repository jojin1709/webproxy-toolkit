# Phase 7: Collaborator — local OOB interaction server — Implementation Plan

Status: approved (user: "ok go"). Execute 7a → 7b; each ends green
(cargo test/build, tsc, vite build, shot.py, a11y.py).

Clean-room: own local OOB server (features.csv 43–44). No PortSwigger cloud, no SMTP.
Lab model: target must resolve payload hostnames via our DNS (point the lab resolver at
127.0.0.1:<dns_port> or add hosts entries). Payload domain configurable (default `oob.wpt.local`).

## 7a — Server + schema + commands

### Schema (replica/schema.sql)
```sql
CREATE TABLE IF NOT EXISTS collab_payloads (
  id         INTEGER PRIMARY KEY AUTOINCREMENT,
  label      TEXT NOT NULL DEFAULT '',
  domain     TEXT NOT NULL,
  kind       TEXT NOT NULL,              -- url | html | host
  value      TEXT NOT NULL,              -- full payload text
  created_at TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS collab_interactions (
  id         INTEGER PRIMARY KEY AUTOINCREMENT,
  payload_id INTEGER,                    -- FK, matched by first label; null if unknown
  subdomain  TEXT NOT NULL,
  protocol   TEXT NOT NULL,              -- dns | http
  source     TEXT NOT NULL DEFAULT '',
  detail     TEXT NOT NULL DEFAULT '{}', -- JSON (dns: {name,qtype}; http: {method,path,headers,body})
  seen_at    TEXT NOT NULL
);
```

### Backend (`collaborator.rs`, new)
- `CollabState`: `running: AtomicBool`, `dns_port/http_port: AtomicU16`, `domain: RwLock<String>`,
  `bind_ip: RwLock<Ipv4Addr>`, shutdown watch channels; `Default` impl; added to `SharedProxy`.
- `start_collab(state, domain, bind_ip, dns_port, http_port)`:
  - UDP DNS on `bind_ip:dns_port`, TCP HTTP on `bind_ip:http_port`; tokio tasks (proxy.rs pattern).
  - Minimal DNS: parse header+question; if QNAME ends with our domain → A (→bind_ip, TTL 60)
    for A, empty NOERROR for AAAA, NXDOMAIN otherwise; log every query (any qtype).
    Hand-rolled wire format (~100 lines; no new deps).
  - HTTP: httparse request → log (method, path, headers, body ≤ 4 KB) → 200 `ok`.
- Interactions: persist via db + emit tauri event `collab-interaction`; matched to payload by
  first DNS label == payload subdomain (payload value contains `<rand>.<domain>`).
- `stop_collab(state)`: shutdown channels.
- Payload gen: `<8 hex>.<domain>` (uuid-based, no rand dep); kinds: `url` →
  `http://<host>/`, `html` → `<img src="http://<host>/x">`, `host` → bare hostname.
- Commands (register all): `collab_status`, `collab_start`, `collab_stop`,
  `collab_new_payload` (kind, label), `collab_payload_list`, `collab_delete_payload`,
  `collab_interactions_list` (payload_id?), `collab_clear_interactions`.
- db.rs: `insert_collab_payload`, `list_collab_payloads`, `delete_collab_payload`,
  `insert_collab_interaction`, `list_collab_interactions`, `clear_collab_interactions`,
  `reset_collab_running`-equivalent not needed (running is in-memory only).
- Unit tests: DNS A response build/parse roundtrip, suffix matching, payload value shape,
  interaction→payload matching by first label.

## 7b — Panel + QA + docs/ship

### Frontend
- `CollaboratorPanel.tsx` replaces placeholder: server bar (domain, bind IP, DNS port, HTTP
  port, Start/Stop, status chips), "Generate payload" (kind select + label + button), payload
  table (label, kind, value + Copy, created, delete), interactions table for selected payload
  (time, protocol chip, source, detail) + "All payloads" view, live `listen("collab-interaction")`,
  Clear button. types: `CollabPayload`, `CollabInteraction`, `CollabStatus`.
- App.tsx: wire `<CollaboratorPanel setToast={setToast}/>`.

### QA
- mock.js: 2 payloads + 4 interactions (dns+http) seeds + handlers for all 8 commands;
  shot.py `49-collaborator`; a11y `collaborator`.

### Docs / ship
- README status row 7 → done; Collaborator feature bullet; features.csv rows 43–44 clone=yes.
- Commit + push.

## Verification gates (end of each sub-phase)
`cargo test` (lib), `npx tsc --noEmit`, `npm run build`, `python qa\shot.py`, `python qa\a11y.py`.

## Explicit scope cuts
- No SMTP/other protocols (DNS + HTTP only).
- No TLS on the OOB HTTP listener (plain HTTP; SSRF labs don't need it).
- No wildcard public domain / no cloud polling (server is local, events are push).
- No per-payload "report" formatting beyond the interaction log.
- Ports <1024 need admin on Windows — defaults 5354 (DNS) / 8081 (HTTP); UI hint only.
