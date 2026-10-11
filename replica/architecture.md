# Architecture: WebProxy Toolkit (a clean-room rebuild of Burp Suite Professional feature set)

Local-only desktop app. No account, no license server, no telemetry.
Clean-room: features/UX from public PortSwigger docs only; zero code from the installer.
Scope decision (user): **full parity** — all tools incl. Intruder, exhaustive Scanner,
session handling, extensions (own JS API), Collaborator (local OOB server).

## Stack

| layer | choice | why |
| --- | --- | --- |
| shell / UI | Tauri 2 + React + TypeScript + Tailwind | dense multi-pane desktop UI without Java; small binary; Rust backend |
| UI tables | TanStack Table (virtualized) | history can grow to 100k+ rows |
| editor | CodeMirror 6 (HTTP raw) | line numbers, find, large buffers |
| proxy runtime | tokio (multi-thread) | async MITM, TLS, many concurrent connections |
| HTTP parse | `httparse` + own body/chunked handling | low-level control needed for intercept |
| TLS MITM | rustls + rcgen | generate CA, mint leaf certs per host; no OpenSSL |
| HTTP client (Repeater/Intruder) | raw tokio TLS + own client | full header control, no hyper needed |
| HTTP/2 | `h2` crate behind ALPN | Burp parity for h2 sites |
| database | SQLite via `rusqlite` (bundled) | single-user local project file; no server |
| project format | `.wpt` = SQLite file (zip optional later) | open/save like a project file |
| diff engine | own line/word/byte differ (Myers) | Comparer |
| entropy tests | own + `statrs`-style math (Shannon, chi-square, entropy per bit, arithmetic-coding/Monobit/Frequency/Runs/Serial/Correlation) | Sequencer |
| extensions | `rquickjs` (embedded ES2020 JS) | our own plugin API; no Burp Java/Python ecosystem |
| OOB server | tokio UDP/DNS + HTTP listeners on localhost | local Collaborator equivalent |
| packaging | MSI / portable zip | Windows first (user platform) |

No Postgres, no Supabase, no Stripe, no Resend, no Vercel — this is not a web SaaS.

## Schema

Base tables (5): `meta`, `exchanges`, `scope_rules`, `repeater_tabs`, `match_replace`.
Full-parity tables (added in later phases):

| table | purpose |
| --- | --- |
| `intruder_campaigns` | attack type, positions, resource pool config |
| `intruder_payloads` | payload sets + processors per campaign |
| `intruder_results` | per-request results + extractor output |
| `issues` | scanner findings (severity, confidence, evidence) |
| `scan_tasks` | crawl/audit/fuzz task state |
| `scan_configs` | named active/passive check selections |
| `session_rules` | session handling rules + macros |
| `extensions` | installed JS plugins + enabled state |
| `oob_payloads` / `oob_interactions` | Collaborator payload registry + callback log |
| `logger_entries` | proxy/extension/event log lines |
| `sequencer_samples` | captured token sets for entropy analysis |
| `comparer_items` | saved diff inputs |
| `settings` (in `meta`) | listeners, upstream rules, TLS pass-through, intercept rules |

Site map is a **query** over `exchanges` (host + path segments), not a stored tree — avoids sync bugs.

See `replica/schema.sql`.

Hard constraints:
- intercept queue is **in-memory only** (not SQLite) so a hung browser request does not bloat the DB
- bodies over N MB stored as sidecar blobs under `project_dir/blobs/` with path in row (keep DB snappy)
- `in_scope` computed on insert from `scope_rules` (exclude wins)
- scanner runs on a bounded worker pool (`scan_tasks` row per task, cooperative cancel)

## API (Tauri commands — local IPC, not HTTP)

| method | does | who | input | output | flow |
| --- | --- | --- | --- | --- | --- |
| `proxy_start` | bind listener | local UI | addr, port | ok / err (port in use) | F01 |
| `proxy_stop` | unbind | local UI | — | ok | F01 |
| `proxy_status` | up/down, held count | local UI | — | status | S01 bar |
| `intercept_toggle` | on/off request hold | local UI | bool | ok | F02 |
| `intercept_forward` | send held req (maybe edited) | local UI | id, raw or patches | ok | F02 |
| `intercept_drop` | kill held | local UI | id | ok | F02 |
| `history_list` | paged + filter | local UI | filter, offset, limit | rows | S03 |
| `history_get` | full exchange | local UI | id | headers+bodies | S03→S04 |
| `history_send_to` | copy request into tool | local UI | id, tool=repeater\|decoder | tab id | F03 |
| `repeater_send` | perform request | local UI | tab id or raw request | response + timing | F03 |
| `repeater_save_tab` / `list` / `close` | manage tabs | local UI | … | … | S05 |
| `decoder_apply` | encode/decode chain | local UI | input, ops[] | output or error | F04 |
| `scope_list` / `scope_upsert` / `scope_delete` | rules | local UI | rule | id | F05 |
| `scope_from_url` | add include from history row | local UI | exchange id | rule | F05 |
| `sitemap_tree` | aggregated hosts/paths | local UI | filter? | tree | S08 |
| `ca_export` | write CA cert (+ optional import helper path) | local UI | path | ok | F01 |
| `settings_get` / `settings_set` | port, theme, intercept defaults | local UI | … | … | S09 |
| `project_save` / `project_open` | checkpoint SQLite | local UI | path | ok | should-have |
| `comparer_diff` | side-by-side byte/line diff | local UI | two exchange ids or texts | diff rows | should-have |
| `sequencer_analyze` | entropy report for a token set | local UI | samples/source | stats | parity |
| `intruder_start` / `pause` / `stop` | run attack | local UI | campaign id | progress | parity |
| `intruder_campaign_*` | CRUD positions/payloads | local UI | … | … | parity |
| `scanner_task_*` | start/cancel crawl+audit, list tasks | local UI | target/config | task id | parity |
| `issues_list` / `issues_get` / `issues_delete` | findings | local UI | filter | rows | parity |
| `session_rule_*` | rules + macro replay | local UI | … | … | parity |
| `extension_*` | install/enable/disable JS plugins, invoke hooks | local UI | path | state | parity |
| `oob_payload_create` / `oob_interactions` | Collaborator local | local UI | label | payload, log | parity |
| `logger_entries` | proxy/event log tail | local UI | filter | rows | parity |
| `settings_get` / `settings_set` | listeners, upstream, TLS, intercept rules | local UI | … | … | parity |
| `project_save` / `project_open` | checkpoint SQLite | local UI | path | ok | parity |

Webhooks in/out: **none**.
Jobs: **none** (all event-driven on the proxy tokio runtime). Optional: prune history when over `max_history` (in-process interval).

## The parts that bite

- **TLS MITM:** correct SNI, ALPN (http/1.1 + optional h2 later), cert cache per host, revoke/rebuild CA; Windows trust install is optional UX (export PEM + docs), do not silently mutate the trust store.
- **HTTP correctness:** Content-Length vs chunked vs close-delimited; absolute-form request line from browsers; CONNECT tunnel; WebSocket Upgrade pass-through (v1 can pipe bytes after 101).
- **Intercept races:** only one held request at a time for v1 (queue behind it); never block the accept loop — park the connection task.
- **Large bodies / binary:** hex view toggle; do not UTF-8-lossy decode without marking.
- **UI perf:** history list virtualized; bodies loaded on demand (`history_get`).
- **Repeater vs proxy connection reuse:** v1 = fresh connection per send (simpler, slightly slower).
- **No stealth vs PortSwigger:** product name, icons, and copy are original; do not use "Burp" branding.

## Build order (full parity, user decision: everything)

1. **Vertical slice (done/compiling):** proxy listener + CA + intercept + history + Repeater raw send + Decoder + Target + Settings shell. Screens: S01, S02, S03, S04, S05, S06, S08, S09.
2. **Phase 1 finish (parity core):** SQLite persistence (`.wpt`), multi-listener + match/replace + upstream + TLS pass-through + intercept rules, scope rules UI, Repeater tabs + layout + follow-redirects, Decoder hash/smart-chain, WebSocket pass-through, virtualized history.
3. **Phase 2:** Comparer, Sequencer, Logger, global Search.
4. **Phase 3:** Intruder (4 attack types, payload sets/processors, resource pool, results + extractors).
5. **Phase 4:** Scanner — issue framework + passive checks first, then crawl + exhaustive active check library (~100+ checks), scan configs, Dashboard (tasks + issue activity), issue detail view.
6. **Phase 5:** Session handling rules + macros; project save/open UI.
7. **Phase 6:** Extensions — JS plugin API (rquickjs): HTTP handler, proxy-listener hook, custom tabs, context-menu actions, message-editor custom columns.
8. **Phase 7:** Collaborator local — DNS+HTTP OOB listeners, payload generation UI, interaction log; wire OOB payload insertion into Scanner checks.

Each phase ends green: `cargo check` + `npm run build` + smoke test (`tauri dev`, curl through proxy).

## Summary

- **Stack:** Tauri 2 + React/TS + tokio + rustls + SQLite, all local.
- **Tables:** 5. **Commands (routes):** ~20 Tauri commands.
- **Riskiest parts:** TLS MITM + CA UX; HTTP message fidelity (chunked/CONNECT/WS); intercept scheduling without deadlocking the proxy.
- **Next:** `/replica-design`
