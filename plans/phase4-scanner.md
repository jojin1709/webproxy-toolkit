# Phase 4: Scanner — Implementation Plan

Status: approved (user: "ok"). Execute 4a → 4b → 4c; each ends green (cargo test/build, tsc, vite build, shot.py, a11y.py).

## 4a — Issue framework + passive checks + Issues UI

### Schema (append to `replica/schema.sql`)
```sql
CREATE TABLE IF NOT EXISTS issues (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  check_id TEXT NOT NULL,
  name TEXT NOT NULL,
  severity TEXT NOT NULL,            -- info|low|medium|high|critical
  confidence TEXT NOT NULL,          -- tentative|firm|certain
  host TEXT NOT NULL DEFAULT '',
  url TEXT NOT NULL DEFAULT '',
  method TEXT NOT NULL DEFAULT '',
  path TEXT NOT NULL DEFAULT '',
  param TEXT NOT NULL DEFAULT '',
  description TEXT NOT NULL DEFAULT '',
  remediation TEXT NOT NULL DEFAULT '',
  evidence TEXT NOT NULL DEFAULT '', -- short snippet proving the finding
  exchange_id INTEGER,               -- link to exchanges when from proxy traffic
  status TEXT NOT NULL DEFAULT 'open', -- open|acknowledged|false-positive
  dedupe_key TEXT NOT NULL UNIQUE,   -- check_id|host|path|param
  created_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_issues_severity ON issues(severity);
CREATE INDEX IF NOT EXISTS idx_issues_host ON issues(host);
```

### Backend
- `db.rs`: `Issue` struct + `insert_issue` (INSERT OR IGNORE on dedupe_key → returns Option\<id\>), `list_issues` (filters: severity[], host, status, text search; offset/limit), `get_issue`, `update_issue_status`, `delete_issue`, `issues_counts` (GROUP BY severity), `clear_issues`; unit tests.
- `scanner.rs` (new module): check catalog `CHECKS: &[CheckDef]` (id, name, severity, passive/active, description, remediation) + `passive_scan(db, &HttpExchange) -> Vec<Issue>` — pure functions per check, body scan capped at `BODY_SCAN_LIMIT`.
  Initial passive library (~15):
  missing-csp, missing-hsts (https only), missing-xfo, missing-xcto, missing-referrer-policy,
  cookie-no-httponly, cookie-no-secure (https only), server-banner (version in Server), x-powered-by,
  directory-listing, sql-error-leak, stack-trace-leak, email-disclosure, internal-ip-disclosure,
  mixed-content (https page → http subresources), basic-auth-plaintext (http only), password-in-url.
- `proxy.rs`: hook in `record_exchange` after struct build — inside existing `tokio::spawn` after `insert_exchange`, call `scanner::passive_scan` when `in_scope && !websocket`; new issues emit `"issue-added"` + `log_event("scanner")`.
- `commands.rs`: `issues_list`, `issues_get`, `issues_update_status`, `issues_delete`, `issues_clear`, `issues_counts`, `checks_list` (catalog); register all; emit events.
- `lib.rs`: `pub mod scanner;`

### Frontend
- `types.ts`: `Issue`, `IssueSeverity`, `IssueCounts`, `CheckDef`.
- `lib/scanner.ts`: `scannerApi` (typed invokes) — model on `intruderApi`.
- `components/IssueTable.tsx`: shared table (severity badge, name, host/path, method, param, confidence, status, time) with filter row (severity chips, host, status, text) — reuses `.history` + `.issue-badge` CSS.
- `components/IssueDetail.tsx`: metadata header + description/remediation + evidence + linked request/response via `MessageView` when `exchange_id` set.
- `DashboardPanel.tsx`: enable Issues sub-tab → IssueTable+IssueDetail; wire "Most serious vulnerabilities" table + severity badge counts to live issues; listen `"issue-added"`.
- `TargetPanel.tsx`: enable Issues sub-tab → same IssueTable (all hosts).

### QA
- mock.js: seeded issues (one per severity), handlers for all issue commands; shot.py: `40-issues-table`, `41-issue-detail`; a11y: `dashboard-issues`, `target-issues`.

---

## 4b — Crawl + active audit engine + scan tasks

### Schema
```sql
CREATE TABLE IF NOT EXISTS scan_tasks (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  kind TEXT NOT NULL,                -- crawl|audit|crawl-audit
  target TEXT NOT NULL,
  config_id INTEGER,
  status TEXT NOT NULL DEFAULT 'running', -- running|paused|done|cancelled|error
  pages_crawled INTEGER NOT NULL DEFAULT 0,
  requests_sent INTEGER NOT NULL DEFAULT 0,
  issues_found INTEGER NOT NULL DEFAULT 0,
  error TEXT,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
);
```

### Backend
- `scanner.rs`: `ScanCtl { cancel: AtomicBool, paused: AtomicBool }`; `scan_tasks: Mutex<HashMap<i64, ScanCtl>>` field on `ProxyState` (like `intruder_runs`); `hydrate` marks running→cancelled (crash recovery).
- `run_scan_task(app, db, state, task_id, target, cfg)`: spawns `tokio::spawn`; BFS crawl same-origin/in-scope, regex link+form extraction (`href`, `src`, `action`), caps from config (max_pages default 50, max_depth 3); reuses proxy raw HTTP client for fetches; cooperative cancel/pause between requests; emits `"scan-progress"` throttled (~250ms); updates `scan_tasks` row.
- Active checks per insertion point (query params + form fields) — initial library (~5):
  sqli-error (`'`, `"'`, `1'--` → SQL error signatures), xss-reflected (canary `<wptXSScanary>`, check unencoded reflection), path-traversal (`../` payloads → /etc/passwd or win.ini markers), open-redirect (`//evil.example`, `https://evil.example` in params → Location check), sensitive-files (`/.git/config`, `/.env`, `/backup.zip` etc. → 200 = info issue).
  Each check = pure function on (req template, param, response) → Option\<issue draft\>; on hit `insert_issue` + `"issue-added"` event.
- `commands.rs`: `scan_start(target, kind, config_id?)`, `scan_cancel`, `scan_pause`, `scan_resume`, `scan_tasks_list`, `scan_state`; `hydrate` reset.

### Frontend
- `DashboardPanel.tsx`: enable "audit-items" sub-tab → scan tasks table (kind, target, status, pages, requests, issues, cancel); "New scan" button → dialog (target URL, kind select, config select); "Task progress" card wired to live task via `"scan-progress"`.
- `TargetPanel.tsx`: "Crawl paths view" button → starts crawl-audit from host root.

### QA
- mock.js: simulated scan task emitting progress + seeded task; shot.py: `42-scan-tasks`, `43-new-scan-dialog`; a11y: `dashboard-audit-items`.

---

## 4c — Scan configs + issue definitions catalog + polish

### Schema
```sql
CREATE TABLE IF NOT EXISTS scan_configs (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  name TEXT NOT NULL UNIQUE,
  passive_enabled INTEGER NOT NULL DEFAULT 1,
  active_checks TEXT NOT NULL DEFAULT '[]',  -- JSON check-id array; [] = all
  crawl_max_pages INTEGER NOT NULL DEFAULT 50,
  crawl_max_depth INTEGER NOT NULL DEFAULT 3,
  audit_threads INTEGER NOT NULL DEFAULT 4,
  throttle_ms INTEGER NOT NULL DEFAULT 50,
  is_default INTEGER NOT NULL DEFAULT 0,
  created_at TEXT NOT NULL
);
```
Seed default "Crawl + audit" config on first `Db::open`.

### Backend
- `db.rs`: scan_configs CRUD + seed; `commands.rs`: `scan_config_list/get/save/delete`.

### Frontend
- Scan config editor (from New scan dialog "Edit config…"): name, passive toggle, active-check checkboxes from catalog (with name+severity+description — doubles as the issue definitions catalog), crawl limits, threads, throttle.
- Dashboard "audit-log" sub-tab → `LogTable` filtered `tool="scanner"`.

### QA
- mock.js: config handlers; shot.py: `44-scan-config`; a11y: `scan-config-editor`.

---

## Verification gates (end of each sub-phase)
`cargo test` (lib+e2e), `cargo build`, `npx tsc --noEmit`, `npm run build`, `python qa\shot.py`, `python qa\a11y.py`.

## Explicit scope cuts
- Active check library is ~5 real checks (framework + catalog make expansion cheap); not Burp's ~100 — note in features.csv.
- Fuzzing (features.csv row 37, should) → out of scope; use Intruder.
- No scan-config import/export, no scheduled scans, no BApp-style custom checks.
