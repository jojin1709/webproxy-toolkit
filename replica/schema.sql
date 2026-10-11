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

CREATE TABLE IF NOT EXISTS match_replace (
  id        INTEGER PRIMARY KEY AUTOINCREMENT,
  enabled   INTEGER NOT NULL DEFAULT 1,
  phase     TEXT NOT NULL CHECK (phase IN ('request','response')),
  match_kind TEXT NOT NULL CHECK (match_kind IN ('literal','regex','header')),
  match_val TEXT NOT NULL,
  replace_val TEXT NOT NULL,
  comment   TEXT
);
