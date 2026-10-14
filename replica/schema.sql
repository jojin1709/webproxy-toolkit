-- WebProxy Toolkit project file schema (SQLite)
-- Local single-user; no RLS. One file = one project (.wpt).

PRAGMA foreign_keys = ON;

CREATE TABLE IF NOT EXISTS meta (
  key   TEXT PRIMARY KEY,
  value TEXT NOT NULL
);
-- keys: schema_version, listen_host, listen_port, intercept_requests,
--       intercept_responses, theme, max_history, ca_cert_path

CREATE TABLE IF NOT EXISTS exchanges (
  id            INTEGER PRIMARY KEY AUTOINCREMENT,
  started_at    TEXT    NOT NULL, -- RFC3339 UTC
  time_ms       INTEGER,
  client_addr   TEXT,
  host          TEXT    NOT NULL,
  method        TEXT    NOT NULL,
  url           TEXT    NOT NULL, -- absolute URL
  req_headers   TEXT    NOT NULL, -- JSON array of [name, value]
  req_body_path TEXT,             -- sidecar path if large, else NULL
  req_body      BLOB,             -- inline if small
  status        INTEGER,
  resp_headers  TEXT,             -- JSON array
  resp_body_path TEXT,
  resp_body     BLOB,
  tls           INTEGER NOT NULL DEFAULT 0,
  websocket     INTEGER NOT NULL DEFAULT 0,
  in_scope      INTEGER NOT NULL DEFAULT 1,
  note          TEXT
);
CREATE INDEX IF NOT EXISTS idx_exchanges_started ON exchanges(started_at DESC);
CREATE INDEX IF NOT EXISTS idx_exchanges_host    ON exchanges(host);
CREATE INDEX IF NOT EXISTS idx_exchanges_method  ON exchanges(method);
CREATE INDEX IF NOT EXISTS idx_exchanges_status  ON exchanges(status);

CREATE TABLE IF NOT EXISTS scope_rules (
  id       INTEGER PRIMARY KEY AUTOINCREMENT,
  mode     TEXT NOT NULL CHECK (mode IN ('include','exclude')),
  kind     TEXT NOT NULL CHECK (kind IN ('host','url_prefix','regex')),
  pattern  TEXT NOT NULL,
  enabled  INTEGER NOT NULL DEFAULT 1,
  created_at TEXT NOT NULL
);
-- evaluation: exclude matches win over include; empty includes = all in scope

CREATE TABLE IF NOT EXISTS repeater_tabs (
  id           INTEGER PRIMARY KEY AUTOINCREMENT,
  name         TEXT NOT NULL,
  req_raw      TEXT NOT NULL,
  last_status  INTEGER,
  last_time_ms INTEGER,
  last_resp_raw TEXT,
  dirty        INTEGER NOT NULL DEFAULT 0,
  sort_order   INTEGER NOT NULL DEFAULT 0,
  updated_at   TEXT NOT NULL
);

-- one row per request sent from a Repeater tab (its per-tab back/forward history)
CREATE TABLE IF NOT EXISTS repeater_history (
  id        INTEGER PRIMARY KEY AUTOINCREMENT,
  tab_id    INTEGER NOT NULL,
  sent_at   TEXT    NOT NULL,
  req_raw   TEXT    NOT NULL,
  resp_raw  TEXT,
  status    INTEGER,
  time_ms   INTEGER
);
CREATE INDEX IF NOT EXISTS idx_repeater_history_tab ON repeater_history(tab_id, id);

CREATE TABLE IF NOT EXISTS match_replace (
  id       INTEGER PRIMARY KEY AUTOINCREMENT,
  enabled  INTEGER NOT NULL DEFAULT 1,
  phase    TEXT NOT NULL CHECK (phase IN ('request','response')),
  match_kind TEXT NOT NULL CHECK (match_kind IN ('literal','regex','header')),
  match_val TEXT NOT NULL,
  replace_val TEXT NOT NULL,
  comment  TEXT
);

-- Intruder attack campaign: request template may contain §…§ position markers
CREATE TABLE IF NOT EXISTS intruder_campaigns (
  id           INTEGER PRIMARY KEY AUTOINCREMENT,
  name         TEXT NOT NULL,
  attack_type  TEXT NOT NULL CHECK (attack_type IN ('sniper','battering_ram','pitchfork','cluster_bomb')),
  req_raw      TEXT NOT NULL,
  url_hint     TEXT,
  follow       INTEGER NOT NULL DEFAULT 0,
  set_map      TEXT NOT NULL DEFAULT '[]',  -- JSON int[]: payload set index per position
  threads      INTEGER NOT NULL DEFAULT 4,
  delay_ms     INTEGER NOT NULL DEFAULT 0,
  max_requests INTEGER NOT NULL DEFAULT 0,  -- 0 = unlimited (total combos)
  extractors   TEXT NOT NULL DEFAULT '[]',  -- JSON [{name, pattern}]
  status       TEXT NOT NULL DEFAULT 'draft'
               CHECK (status IN ('draft','running','paused','done','stopped','error')),
  created_at   TEXT NOT NULL,
  updated_at   TEXT NOT NULL
);

-- one row per payload set of a campaign
CREATE TABLE IF NOT EXISTS intruder_payloads (
  id          INTEGER PRIMARY KEY AUTOINCREMENT,
  campaign_id INTEGER NOT NULL REFERENCES intruder_campaigns(id) ON DELETE CASCADE,
  set_index   INTEGER NOT NULL,
  name        TEXT NOT NULL,
  kind        TEXT NOT NULL CHECK (kind IN ('list','numbers')),
  items       TEXT NOT NULL DEFAULT '',        -- newline separated payloads (kind=list)
  start_i     INTEGER,                         -- numbers kind: start
  end_i       INTEGER,                         -- numbers kind: end (inclusive)
  step_i      INTEGER,                         -- numbers kind: step
  processors  TEXT NOT NULL DEFAULT '[]',      -- JSON [{kind, a, b}]
  UNIQUE (campaign_id, set_index)
);

-- one row per request the attack engine sent
CREATE TABLE IF NOT EXISTS intruder_results (
  id          INTEGER PRIMARY KEY AUTOINCREMENT,
  campaign_id INTEGER NOT NULL REFERENCES intruder_campaigns(id) ON DELETE CASCADE,
  seq         INTEGER NOT NULL,               -- attack order (0-based)
  combo       TEXT NOT NULL DEFAULT '[]',     -- JSON string[] payload values used
  status      INTEGER,                        -- HTTP status, NULL on transport error
  resp_len    INTEGER NOT NULL DEFAULT 0,
  time_ms     INTEGER NOT NULL DEFAULT 0,
  error       TEXT,
  req_raw     TEXT NOT NULL,
  resp_raw    TEXT,
  extracted   TEXT NOT NULL DEFAULT '{}',     -- JSON {extractor_name: value}
  created_at  TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_intruder_results_campaign ON intruder_results(campaign_id, seq);
CREATE INDEX IF NOT EXISTS idx_intruder_campaigns_updated ON intruder_campaigns(updated_at DESC);

-- Scanner finding: one row per unique (check, host, path, param)
CREATE TABLE IF NOT EXISTS issues (
  id           INTEGER PRIMARY KEY AUTOINCREMENT,
  check_id     TEXT NOT NULL,
  name         TEXT NOT NULL,
  severity     TEXT NOT NULL CHECK (severity IN ('info','low','medium','high','critical')),
  confidence   TEXT NOT NULL CHECK (confidence IN ('tentative','firm','certain')),
  host         TEXT NOT NULL DEFAULT '',
  url          TEXT NOT NULL DEFAULT '',
  method       TEXT NOT NULL DEFAULT '',
  path         TEXT NOT NULL DEFAULT '',
  param        TEXT NOT NULL DEFAULT '',
  description  TEXT NOT NULL DEFAULT '',
  remediation  TEXT NOT NULL DEFAULT '',
  evidence     TEXT NOT NULL DEFAULT '',  -- short snippet proving the finding
  exchange_id  INTEGER,                   -- exchanges.id when raised from proxy traffic
  status       TEXT NOT NULL DEFAULT 'open'
               CHECK (status IN ('open','acknowledged','false-positive')),
  dedupe_key   TEXT NOT NULL UNIQUE,      -- check_id|host|path|param
  created_at   TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_issues_severity ON issues(severity);
CREATE INDEX IF NOT EXISTS idx_issues_host     ON issues(host);
CREATE INDEX IF NOT EXISTS idx_issues_created  ON issues(created_at DESC);

-- scanner scan tasks (crawl / audit / crawl-audit)
CREATE TABLE IF NOT EXISTS scan_tasks (
  id              INTEGER PRIMARY KEY AUTOINCREMENT,
  kind            TEXT NOT NULL,            -- crawl|audit|crawl-audit
  target          TEXT NOT NULL,
  config_id       INTEGER,
  status          TEXT NOT NULL DEFAULT 'running', -- running|paused|done|cancelled|error
  pages_crawled   INTEGER NOT NULL DEFAULT 0,
  requests_sent   INTEGER NOT NULL DEFAULT 0,
  issues_found    INTEGER NOT NULL DEFAULT 0,
  error           TEXT,
  created_at      TEXT NOT NULL,
  updated_at      TEXT NOT NULL
);

-- scanner scan configs (reusable crawl/audit settings + enabled check ids)
CREATE TABLE IF NOT EXISTS scan_configs (
  id               INTEGER PRIMARY KEY AUTOINCREMENT,
  name             TEXT NOT NULL UNIQUE,
  passive_enabled  INTEGER NOT NULL DEFAULT 1,
  active_checks    TEXT NOT NULL DEFAULT '[]',   -- JSON check-id array; [] = all
  crawl_max_pages  INTEGER NOT NULL DEFAULT 50,
  crawl_max_depth  INTEGER NOT NULL DEFAULT 3,
  audit_threads    INTEGER NOT NULL DEFAULT 4,
  throttle_ms      INTEGER NOT NULL DEFAULT 50,
  is_default       INTEGER NOT NULL DEFAULT 0,
  created_at       TEXT NOT NULL
);

-- installed JS extensions (Phase 6)
CREATE TABLE IF NOT EXISTS extensions (
  id         INTEGER PRIMARY KEY AUTOINCREMENT,
  name       TEXT NOT NULL,
  path       TEXT NOT NULL,
  enabled    INTEGER NOT NULL DEFAULT 1,
  error      TEXT,
  loaded_at  TEXT,
  created_at TEXT NOT NULL
);


-- Collaborator payloads (Phase 7)
CREATE TABLE IF NOT EXISTS collab_payloads (
  id         INTEGER PRIMARY KEY AUTOINCREMENT,
  label      TEXT NOT NULL DEFAULT '',
  domain     TEXT NOT NULL,
  kind       TEXT NOT NULL,
  value      TEXT NOT NULL,
  created_at TEXT NOT NULL
);

-- Collaborator interactions (Phase 7)
CREATE TABLE IF NOT EXISTS collab_interactions (
  id         INTEGER PRIMARY KEY AUTOINCREMENT,
  payload_id INTEGER,
  subdomain  TEXT NOT NULL,
  protocol   TEXT NOT NULL,
  source     TEXT NOT NULL DEFAULT '',
  detail     TEXT NOT NULL DEFAULT '{}',
  seen_at    TEXT NOT NULL
);
