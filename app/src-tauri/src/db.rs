use crate::proxy::HttpExchange;
use parking_lot::Mutex;
use rusqlite::{params, Connection, OptionalExtension};
use std::path::{Path, PathBuf};
use std::sync::Arc;

const SCHEMA: &str = include_str!("../../../replica/schema.sql");
const MAX_INLINE_BODY: usize = 256 * 1024;

#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub struct ScopeRule {
    pub id: i64,
    pub mode: String,   // include | exclude
    pub kind: String,   // host | url_prefix | regex
    pub pattern: String,
    pub enabled: bool,
}

#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub struct UpstreamRule {
    pub id: i64,
    pub position: i64,
    pub host_pattern: String,
    pub proxy_host: String,
    pub proxy_port: u16,
    pub use_tls: bool,
    pub enabled: bool,
}

#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub struct ProxyListenerSlot {
    pub id: i64,
    pub host: String,
    pub port: u16,
    pub running: bool,
}

#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub struct MatchRule {
    pub id: i64,
    pub enabled: bool,
    pub phase: String,      // request | response
    pub match_kind: String, // literal | regex | header
    pub match_val: String,
    pub replace_val: String,
    pub comment: Option<String>,
}

#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub struct LogEntry {
    pub id: i64,
    pub ts: String,
    pub tool: String,
    pub level: String,
    pub message: String,
}

#[derive(Clone, serde::Serialize)]
pub struct RepeaterHistoryEntry {
    pub id: i64,
    pub tab_id: i64,
    pub sent_at: String,
    pub req_raw: String,
    pub resp_raw: Option<String>,
    pub status: Option<i64>,
    pub time_ms: Option<i64>,
}

#[derive(Clone, serde::Serialize)]
pub struct WsConn {
    pub id: i64,
    pub ts_start: String,
    pub ts_end: Option<String>,
    pub url: String,
    pub host: String,
    pub port: i64,
    pub tls: bool,
    pub msgs_out: i64,
    pub msgs_in: i64,
}

#[derive(Clone, serde::Serialize)]
pub struct WsFrame {
    pub id: i64,
    pub conn_id: i64,
    pub ts: String,
    pub dir: String,
    pub opcode: i64,
    pub len: i64,
    pub payload: String,
}

#[derive(Clone, serde::Serialize)]
pub struct IntruderCampaign {
    pub id: i64,
    pub name: String,
    pub attack_type: String, // sniper | battering_ram | pitchfork | cluster_bomb
    pub req_raw: String,     // may contain §…§ position markers
    pub url_hint: Option<String>,
    pub follow: bool,
    pub set_map: String,     // JSON int[]: payload set index per position
    pub threads: i64,
    pub delay_ms: i64,
    pub max_requests: i64,   // 0 = unlimited
    pub extractors: String,  // JSON [{name, pattern}]
    pub status: String,      // draft | running | paused | done | stopped | error
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Clone, serde::Serialize)]
pub struct IntruderCampaignSummary {
    pub id: i64,
    pub name: String,
    pub attack_type: String,
    pub status: String,
    pub positions: i64,
    pub updated_at: String,
}

#[derive(Clone, serde::Serialize)]
pub struct IntruderPayloadSet {
    pub id: i64,
    pub campaign_id: i64,
    pub set_index: i64,
    pub name: String,
    pub kind: String, // list | numbers
    pub items: String,
    pub start_i: Option<i64>,
    pub end_i: Option<i64>,
    pub step_i: Option<i64>,
    pub processors: String, // JSON [{kind, a, b}]
}

#[derive(Clone, serde::Serialize)]
pub struct IntruderResultRow {
    pub seq: i64,
    pub combo: String, // JSON string[]
    pub status: Option<i64>,
    pub resp_len: i64,
    pub time_ms: i64,
    pub error: Option<String>,
    pub extracted: String, // JSON {name: value}
}

#[derive(Clone, serde::Serialize)]
pub struct IntruderResultFull {
    pub seq: i64,
    pub combo: String,
    pub status: Option<i64>,
    pub resp_len: i64,
    pub time_ms: i64,
    pub error: Option<String>,
    pub req_raw: String,
    pub resp_raw: Option<String>,
    pub extracted: String,
}

pub struct NewIntruderResult {
    pub seq: i64,
    pub combo: String,
    pub status: Option<i64>,
    pub resp_len: i64,
    pub time_ms: i64,
    pub error: Option<String>,
    pub req_raw: String,
    pub resp_raw: Option<String>,
    pub extracted: String,
}

#[derive(Clone, serde::Serialize)]
pub struct Issue {
    pub id: i64,
    pub check_id: String,
    pub name: String,
    pub severity: String,   // info | low | medium | high | critical
    pub confidence: String, // tentative | firm | certain
    pub host: String,
    pub url: String,
    pub method: String,
    pub path: String,
    pub param: String,
    pub description: String,
    pub remediation: String,
    pub evidence: String,
    pub exchange_id: Option<i64>,
    pub status: String, // open | acknowledged | false-positive
    pub created_at: String,
}

pub struct NewIssue {
    pub check_id: String,
    pub name: String,
    pub severity: String,
    pub confidence: String,
    pub host: String,
    pub url: String,
    pub method: String,
    pub path: String,
    pub param: String,
    pub description: String,
    pub remediation: String,
    pub evidence: String,
    pub exchange_id: Option<i64>,
}

/// One scanner run (crawl / audit / crawl-audit).
#[derive(Clone, Debug, serde::Serialize)]
pub struct ScanTask {
    pub id: i64,
    pub kind: String,  // crawl | audit | crawl-audit
    pub target: String,
    pub config_id: Option<i64>,
    pub status: String, // running | paused | done | cancelled | error
    pub pages_crawled: i64,
    pub requests_sent: i64,
    pub issues_found: i64,
    pub error: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

/// Reusable scanner configuration (crawl limits, enabled checks).
#[derive(Clone, Debug, serde::Serialize)]
pub struct ScanConfig {
    pub id: i64,
    pub name: String,
    pub passive_enabled: bool,
    pub active_checks: Vec<String>, // empty = all
    pub crawl_max_pages: i64,
    pub crawl_max_depth: i64,
    pub audit_threads: i64,
    pub throttle_ms: i64,
    pub is_default: bool,
    pub created_at: String,
}

/// One installed JS extension.
#[derive(Clone, Debug, serde::Serialize)]
pub struct ExtensionRow {
    pub id: i64,
    pub name: String,
    pub path: String,
    pub enabled: bool,
    pub error: Option<String>,
    pub loaded_at: Option<String>,
    pub created_at: String,
}

pub struct Db {
    conn: Mutex<Connection>,
    blob_dir: PathBuf,
}

pub fn open(project_path: &Path) -> Result<Arc<Db>, String> {
    if let Some(parent) = project_path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let conn = Connection::open(project_path).map_err(|e| e.to_string())?;
    conn.pragma_update(None, "journal_mode", "WAL").ok();
    conn.pragma_update(None, "synchronous", "NORMAL").map_err(|e| e.to_string())?;
    conn.execute_batch(SCHEMA).map_err(|e| e.to_string())?;
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS logs (
           id      INTEGER PRIMARY KEY AUTOINCREMENT,
           ts      TEXT NOT NULL,
           tool    TEXT NOT NULL,
           level   TEXT NOT NULL,
           message TEXT NOT NULL
         );
         CREATE INDEX IF NOT EXISTS idx_logs_tool  ON logs(tool);
         CREATE INDEX IF NOT EXISTS idx_logs_level ON logs(level);",
    )
    .map_err(|e| e.to_string())?;
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS ws_conns (
           id       INTEGER PRIMARY KEY AUTOINCREMENT,
           ts_start TEXT NOT NULL,
           ts_end   TEXT,
           url      TEXT NOT NULL,
           host     TEXT NOT NULL,
           port     INTEGER NOT NULL,
           tls      INTEGER NOT NULL DEFAULT 0
         );
         CREATE INDEX IF NOT EXISTS idx_ws_conns_start ON ws_conns(ts_start DESC);
         CREATE TABLE IF NOT EXISTS ws_frames (
           id      INTEGER PRIMARY KEY AUTOINCREMENT,
           conn_id INTEGER NOT NULL,
           ts      TEXT NOT NULL,
           dir     TEXT NOT NULL,
           opcode  INTEGER NOT NULL,
           len     INTEGER NOT NULL,
           payload TEXT
         );
         CREATE INDEX IF NOT EXISTS idx_ws_frames_conn ON ws_frames(conn_id, id);",
    )
    .map_err(|e| e.to_string())?;
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS repeater_history (
           id        INTEGER PRIMARY KEY AUTOINCREMENT,
           tab_id    INTEGER NOT NULL,
           sent_at   TEXT NOT NULL,
           req_raw   TEXT NOT NULL,
           resp_raw  TEXT,
           status    INTEGER,
           time_ms   INTEGER
         );
         CREATE INDEX IF NOT EXISTS idx_repeater_history_tab ON repeater_history(tab_id, id);",
    )
    .map_err(|e| e.to_string())?;
    // seed default scan config on first open (idempotent)
    conn.execute(
        "INSERT OR IGNORE INTO scan_configs
           (name, passive_enabled, active_checks, crawl_max_pages, crawl_max_depth,
            audit_threads, throttle_ms, is_default, created_at)
         SELECT 'Crawl + audit', 1, '[]', 50, 3, 4, 50, 1, ?1
         WHERE NOT EXISTS (SELECT 1 FROM scan_configs)",
        params![chrono::Utc::now().to_rfc3339()],
    )
    .map_err(|e| e.to_string())?;
    conn.execute(
        "INSERT OR IGNORE INTO meta(key,value) VALUES('schema_version','1')",
        [],
    )
    .map_err(|e| e.to_string())?;
    let blob_dir = project_path
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join("blobs");
    std::fs::create_dir_all(&blob_dir).map_err(|e| e.to_string())?;
    Ok(Arc::new(Db {
        conn: Mutex::new(conn),
        blob_dir,
    }))
}

fn headers_json(h: &[(String, String)]) -> String {
    serde_json::to_string(h).unwrap_or_else(|_| "[]".into())
}

fn headers_from_json(s: &str) -> Vec<(String, String)> {
    serde_json::from_str(s).unwrap_or_default()
}

fn body_cell(
    db: &Db,
    id: u64,
    side: &str,
    body: &str,
) -> Result<(Option<String>, Option<Vec<u8>>), String> {
    let bytes = body.as_bytes();
    if bytes.len() <= MAX_INLINE_BODY {
        Ok((None, Some(bytes.to_vec())))
    } else {
        let rel = format!("ex_{id}_{side}.bin");
        let path = db.blob_dir.join(&rel);
        std::fs::write(&path, bytes).map_err(|e| e.to_string())?;
        Ok((Some(format!("blobs/{rel}")), None))
    }
}

fn read_body(db: &Db, path: &Option<String>, blob: &Option<Vec<u8>>) -> String {
    if let Some(p) = path {
        let full = db.blob_dir.parent().unwrap_or(Path::new(".")).join(p);
        return std::fs::read(&full)
            .map(|b| String::from_utf8_lossy(&b).to_string())
            .unwrap_or_default();
    }
    blob.as_ref()
        .map(|b| String::from_utf8_lossy(b).to_string())
        .unwrap_or_default()
}

impl Db {
    pub fn insert_exchange(&self, ex: &HttpExchange) -> Result<(), String> {
        let (req_path, req_inline) =
            body_cell(self, ex.id, "req", ex.req_body.as_deref().unwrap_or(""))?;
        let (resp_path, resp_inline) =
            body_cell(self, ex.id, "resp", ex.resp_body.as_deref().unwrap_or(""))?;
        let conn = self.conn.lock();
        conn.execute(
            "INSERT OR REPLACE INTO exchanges
             (id, started_at, time_ms, client_addr, host, method, url,
              req_headers, req_body_path, req_body, status, resp_headers,
              resp_body_path, resp_body, tls, websocket, in_scope)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17)",
            params![
                ex.id as i64,
                ex.started_at,
                ex.time_ms.map(|v| v as i64),
                ex.client_addr,
                ex.host,
                ex.method,
                ex.url,
                headers_json(&ex.req_headers),
                req_path,
                req_inline,
                ex.status.map(|s| s as i64),
                headers_json(&ex.resp_headers),
                resp_path,
                resp_inline,
                ex.tls as i64,
                ex.websocket as i64,
                ex.in_scope as i64,
            ],
        )
        .map_err(|e| e.to_string())?;
        Ok(())
    }

    pub fn load_exchanges(&self, limit: usize) -> Result<Vec<HttpExchange>, String> {
        let conn = self.conn.lock();
        let mut stmt = conn
            .prepare(
                "SELECT id, started_at, time_ms, client_addr, host, method, url,
                        req_headers, req_body_path, req_body, status, resp_headers,
                        resp_body_path, resp_body, tls, websocket, in_scope
                 FROM exchanges ORDER BY id DESC LIMIT ?1",
            )
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map(params![limit as i64], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, Option<i64>>(2)?,
                    row.get::<_, Option<String>>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, String>(5)?,
                    row.get::<_, String>(6)?,
                    row.get::<_, String>(7)?,
                    row.get::<_, Option<String>>(8)?,
                    row.get::<_, Option<Vec<u8>>>(9)?,
                    row.get::<_, Option<i64>>(10)?,
                    row.get::<_, String>(11)?,
                    row.get::<_, Option<String>>(12)?,
                    row.get::<_, Option<Vec<u8>>>(13)?,
                    row.get::<_, i64>(14)?,
                    row.get::<_, i64>(15)?,
                    row.get::<_, i64>(16)?,
                ))
            })
            .map_err(|e| e.to_string())?;
        let mut out = Vec::new();
        for r in rows {
            let r = r.map_err(|e| e.to_string())?;
            out.push(HttpExchange {
                id: r.0 as u64,
                started_at: r.1,
                time_ms: r.2.map(|v| v as u64),
                client_addr: r.3,
                host: r.4,
                method: r.5,
                url: r.6,
                req_headers: headers_from_json(&r.7),
                req_body: Some(read_body(self, &r.8, &r.9)),
                status: r.10.map(|s| s as u16),
                resp_headers: headers_from_json(&r.11),
                resp_body: Some(read_body(self, &r.12, &r.13)),
                tls: r.14 != 0,
                websocket: r.15 != 0,
                in_scope: r.16 != 0,
            });
        }
        out.reverse();
        Ok(out)
    }

    pub fn max_exchange_id(&self) -> Result<u64, String> {
        let conn = self.conn.lock();
        conn.query_row("SELECT COALESCE(MAX(id),0) FROM exchanges", [], |r| {
            r.get::<_, i64>(0)
        })
        .map(|v| v as u64)
        .map_err(|e| e.to_string())
    }

    pub fn clear_exchanges(&self) -> Result<usize, String> {
        let conn = self.conn.lock();
        conn.execute("DELETE FROM exchanges", []).map_err(|e| e.to_string())
    }


    pub fn list_scope(&self) -> Result<Vec<ScopeRule>, String> {
        let conn = self.conn.lock();
        let mut stmt = conn
            .prepare("SELECT id, mode, kind, pattern, enabled FROM scope_rules ORDER BY id")
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map([], |r| {
                Ok(ScopeRule {
                    id: r.get(0)?,
                    mode: r.get(1)?,
                    kind: r.get(2)?,
                    pattern: r.get(3)?,
                    enabled: r.get::<_, i64>(4)? != 0,
                })
            })
            .map_err(|e| e.to_string())?;
        rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())
    }

    pub fn insert_scope(&self, mode: &str, kind: &str, pattern: &str) -> Result<i64, String> {
        let now = chrono::Utc::now().to_rfc3339();
        let conn = self.conn.lock();
        conn.execute(
            "INSERT INTO scope_rules (mode, kind, pattern, enabled, created_at)
             VALUES (?1,?2,?3,1,?4)",
            params![mode, kind, pattern, now],
        )
        .map_err(|e| e.to_string())?;
        Ok(conn.last_insert_rowid())
    }

    pub fn delete_scope(&self, id: i64) -> Result<(), String> {
        let conn = self.conn.lock();
        conn.execute("DELETE FROM scope_rules WHERE id = ?1", params![id])
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    pub fn list_match_replace(&self) -> Result<Vec<MatchRule>, String> {
        let conn = self.conn.lock();
        let mut stmt = conn
            .prepare(
                "SELECT id, enabled, phase, match_kind, match_val, replace_val, comment
                 FROM match_replace ORDER BY id",
            )
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map([], |r| {
                Ok(MatchRule {
                    id: r.get(0)?,
                    enabled: r.get::<_, i64>(1)? != 0,
                    phase: r.get(2)?,
                    match_kind: r.get(3)?,
                    match_val: r.get(4)?,
                    replace_val: r.get(5)?,
                    comment: r.get(6)?,
                })
            })
            .map_err(|e| e.to_string())?;
        rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())
    }

    pub fn insert_match_replace(
        &self,
        phase: &str,
        match_kind: &str,
        match_val: &str,
        replace_val: &str,
        comment: Option<&str>,
    ) -> Result<i64, String> {
        let conn = self.conn.lock();
        conn.execute(
            "INSERT INTO match_replace (enabled, phase, match_kind, match_val, replace_val, comment)
             VALUES (1,?1,?2,?3,?4,?5)",
            params![phase, match_kind, match_val, replace_val, comment],
        )
        .map_err(|e| e.to_string())?;
        Ok(conn.last_insert_rowid())
    }

    pub fn delete_match_replace(&self, id: i64) -> Result<(), String> {
        let conn = self.conn.lock();
        conn.execute("DELETE FROM match_replace WHERE id = ?1", params![id])
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    pub fn save_repeater_tab(
        &self,
        name: &str,
        req_raw: &str,
        last_status: Option<i64>,
        last_time_ms: Option<i64>,
        last_resp_raw: Option<&str>,
        sort_order: i64,
    ) -> Result<i64, String> {
        let now = chrono::Utc::now().to_rfc3339();
        let conn = self.conn.lock();
        conn.execute(
            "INSERT INTO repeater_tabs (name, req_raw, last_status, last_time_ms, last_resp_raw, dirty, sort_order, updated_at)
             VALUES (?1,?2,?3,?4,?5,0,?6,?7)",
            params![name, req_raw, last_status, last_time_ms, last_resp_raw, sort_order, now],
        )
        .map_err(|e| e.to_string())?;
        Ok(conn.last_insert_rowid())
    }

    pub fn update_repeater_tab(
        &self,
        id: i64,
        name: &str,
        req_raw: &str,
        last_status: Option<i64>,
        last_time_ms: Option<i64>,
        last_resp_raw: Option<&str>,
    ) -> Result<(), String> {
        let now = chrono::Utc::now().to_rfc3339();
        let conn = self.conn.lock();
        conn.execute(
            "UPDATE repeater_tabs SET name=?1, req_raw=?2, last_status=?3, last_time_ms=?4,
             last_resp_raw=?5, dirty=0, updated_at=?6 WHERE id=?7",
            params![name, req_raw, last_status, last_time_ms, last_resp_raw, now, id],
        )
        .map_err(|e| e.to_string())?;
        Ok(())
    }

    pub fn list_repeater_tabs(&self) -> Result<Vec<serde_json::Value>, String> {
        let conn = self.conn.lock();
        let mut stmt = conn
            .prepare(
                "SELECT id, name, req_raw, last_status, last_time_ms, last_resp_raw, sort_order
                 FROM repeater_tabs ORDER BY sort_order, id",
            )
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map([], |r| {
                Ok(serde_json::json!({
                    "id": r.get::<_, i64>(0)?,
                    "name": r.get::<_, String>(1)?,
                    "req_raw": r.get::<_, String>(2)?,
                    "last_status": r.get::<_, Option<i64>>(3)?,
                    "last_time_ms": r.get::<_, Option<i64>>(4)?,
                    "last_resp_raw": r.get::<_, Option<String>>(5)?,
                    "sort_order": r.get::<_, i64>(6)?,
                }))
            })
            .map_err(|e| e.to_string())?;
        rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())
    }

    pub fn delete_repeater_tab(&self, id: i64) -> Result<(), String> {
        let conn = self.conn.lock();
        conn.execute("DELETE FROM repeater_history WHERE tab_id = ?1", params![id])
            .map_err(|e| e.to_string())?;
        conn.execute("DELETE FROM repeater_tabs WHERE id = ?1", params![id])
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    pub fn insert_repeater_history(
        &self,
        tab_id: i64,
        req_raw: &str,
        resp_raw: Option<&str>,
        status: Option<i64>,
        time_ms: Option<i64>,
    ) -> Result<i64, String> {
        let now = chrono::Utc::now().to_rfc3339();
        let conn = self.conn.lock();
        conn.execute(
            "INSERT INTO repeater_history (tab_id, sent_at, req_raw, resp_raw, status, time_ms)
             VALUES (?1,?2,?3,?4,?5,?6)",
            params![tab_id, now, req_raw, resp_raw, status, time_ms],
        )
        .map_err(|e| e.to_string())?;
        let id = conn.last_insert_rowid();
        conn.execute(
            "DELETE FROM repeater_history WHERE tab_id = ?1 AND id NOT IN
             (SELECT id FROM repeater_history WHERE tab_id = ?1 ORDER BY id DESC LIMIT 100)",
            params![tab_id],
        )
        .map_err(|e| e.to_string())?;
        Ok(id)
    }

    pub fn list_repeater_history(
        &self,
        tab_id: i64,
        limit: usize,
    ) -> Result<Vec<RepeaterHistoryEntry>, String> {
        let conn = self.conn.lock();
        let mut stmt = conn
            .prepare(
                "SELECT id, tab_id, sent_at, req_raw, resp_raw, status, time_ms
                 FROM repeater_history WHERE tab_id = ?1 ORDER BY id DESC LIMIT ?2",
            )
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map(params![tab_id, limit as i64], |r| {
                Ok(RepeaterHistoryEntry {
                    id: r.get(0)?,
                    tab_id: r.get(1)?,
                    sent_at: r.get(2)?,
                    req_raw: r.get(3)?,
                    resp_raw: r.get(4)?,
                    status: r.get(5)?,
                    time_ms: r.get(6)?,
                })
            })
            .map_err(|e| e.to_string())?;
        rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())
    }

    pub fn get_meta(&self, key: &str) -> Result<Option<String>, String> {
        let conn = self.conn.lock();
        conn.query_row("SELECT value FROM meta WHERE key = ?1", params![key], |r| {
            r.get::<_, String>(0)
        })
        .optional()
        .map_err(|e| e.to_string())
    }

    pub fn set_meta(&self, key: &str, value: &str) -> Result<(), String> {
        let conn = self.conn.lock();
        conn.execute(
            "INSERT INTO meta(key,value) VALUES(?1,?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![key, value],
        )
        .map_err(|e| e.to_string())?;
        Ok(())
    }

    pub fn insert_log(&self, tool: &str, level: &str, message: &str) -> Result<LogEntry, String> {
        let now = chrono::Utc::now().to_rfc3339();
        let conn = self.conn.lock();
        conn.execute(
            "INSERT INTO logs (ts, tool, level, message) VALUES (?1,?2,?3,?4)",
            params![now, tool, level, message],
        )
        .map_err(|e| e.to_string())?;
        Ok(LogEntry {
            id: conn.last_insert_rowid(),
            ts: now,
            tool: tool.to_string(),
            level: level.to_string(),
            message: message.to_string(),
        })
    }

    pub fn list_logs(
        &self,
        tool: Option<&str>,
        level: Option<&str>,
        query: Option<&str>,
        limit: usize,
    ) -> Result<Vec<LogEntry>, String> {
        let conn = self.conn.lock();
        let mut sql = String::from("SELECT id, ts, tool, level, message FROM logs");
        let mut wheres: Vec<String> = Vec::new();
        let mut params: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
        if let Some(t) = tool.filter(|s| !s.is_empty()) {
            wheres.push(format!("tool = ?{}", params.len() + 1));
            params.push(Box::new(t.to_string()));
        }
        if let Some(l) = level.filter(|s| !s.is_empty()) {
            wheres.push(format!("level = ?{}", params.len() + 1));
            params.push(Box::new(l.to_string()));
        }
        if let Some(q) = query.filter(|s| !s.is_empty()) {
            let n = params.len() + 1;
            wheres.push(format!(
                "(message LIKE ?{n} OR tool LIKE ?{n2} OR level LIKE ?{n3})",
                n = n,
                n2 = n + 1,
                n3 = n + 2
            ));
            let like = format!("%{q}%");
            params.push(Box::new(like.clone()));
            params.push(Box::new(like.clone()));
            params.push(Box::new(like));
        }
        if !wheres.is_empty() {
            sql.push_str(" WHERE ");
            sql.push_str(&wheres.join(" AND "));
        }
        sql.push_str(&format!(" ORDER BY id DESC LIMIT ?{}", params.len() + 1));
        params.push(Box::new(limit as i64));
        let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map(rusqlite::params_from_iter(params.iter().map(|b| b.as_ref())), |r| {
                Ok(LogEntry {
                    id: r.get(0)?,
                    ts: r.get(1)?,
                    tool: r.get(2)?,
                    level: r.get(3)?,
                    message: r.get(4)?,
                })
            })
            .map_err(|e| e.to_string())?;
        rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())
    }

    pub fn clear_logs(&self) -> Result<usize, String> {
        let conn = self.conn.lock();
        conn.execute("DELETE FROM logs", []).map_err(|e| e.to_string())
    }

    pub fn insert_ws_conn(&self, url: &str, host: &str, port: u16, tls: bool) -> Result<i64, String> {
        let now = chrono::Utc::now().to_rfc3339();
        let conn = self.conn.lock();
        conn.execute(
            "INSERT INTO ws_conns (ts_start, url, host, port, tls) VALUES (?1,?2,?3,?4,?5)",
            params![now, url, host, port as i64, tls as i64],
        )
        .map_err(|e| e.to_string())?;
        Ok(conn.last_insert_rowid())
    }

    pub fn finish_ws_conn(&self, id: i64) -> Result<(), String> {
        let now = chrono::Utc::now().to_rfc3339();
        let conn = self.conn.lock();
        conn.execute(
            "UPDATE ws_conns SET ts_end = ?1 WHERE id = ?2",
            params![now, id],
        )
        .map_err(|e| e.to_string())?;
        Ok(())
    }

    pub fn insert_ws_frame(
        &self,
        conn_id: i64,
        dir: &str,
        opcode: u8,
        len: usize,
        payload: &str,
    ) -> Result<i64, String> {
        let now = chrono::Utc::now().to_rfc3339();
        let conn = self.conn.lock();
        conn.execute(
            "INSERT INTO ws_frames (conn_id, ts, dir, opcode, len, payload) VALUES (?1,?2,?3,?4,?5,?6)",
            params![conn_id, now, dir, opcode as i64, len as i64, payload],
        )
        .map_err(|e| e.to_string())?;
        Ok(conn.last_insert_rowid())
    }

    pub fn list_ws_conns(&self, limit: usize) -> Result<Vec<WsConn>, String> {
        let conn = self.conn.lock();
        let mut stmt = conn
            .prepare(
                "SELECT c.id, c.ts_start, c.ts_end, c.url, c.host, c.port, c.tls,
                        (SELECT COUNT(*) FROM ws_frames f WHERE f.conn_id = c.id AND f.dir = 'c'),
                        (SELECT COUNT(*) FROM ws_frames f WHERE f.conn_id = c.id AND f.dir = 's')
                 FROM ws_conns c ORDER BY c.id DESC LIMIT ?1",
            )
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map(params![limit as i64], |r| {
                Ok(WsConn {
                    id: r.get(0)?,
                    ts_start: r.get(1)?,
                    ts_end: r.get(2)?,
                    url: r.get(3)?,
                    host: r.get(4)?,
                    port: r.get(5)?,
                    tls: r.get::<_, i64>(6)? != 0,
                    msgs_out: r.get(7)?,
                    msgs_in: r.get(8)?,
                })
            })
            .map_err(|e| e.to_string())?;
        rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())
    }

    /// Newest `limit` frames for a connection, returned oldest-first.
    pub fn list_ws_frames(&self, conn_id: i64, limit: usize) -> Result<Vec<WsFrame>, String> {
        let conn = self.conn.lock();
        let mut stmt = conn
            .prepare(
                "SELECT id, conn_id, ts, dir, opcode, len, payload
                 FROM ws_frames WHERE conn_id = ?1 ORDER BY id DESC LIMIT ?2",
            )
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map(params![conn_id, limit.max(1) as i64], |r| {
                Ok(WsFrame {
                    id: r.get(0)?,
                    conn_id: r.get(1)?,
                    ts: r.get(2)?,
                    dir: r.get(3)?,
                    opcode: r.get(4)?,
                    len: r.get(5)?,
                    payload: r.get(6)?,
                })
            })
            .map_err(|e| e.to_string())?;
        let mut frames: Vec<WsFrame> = rows
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())?;
        frames.reverse();
        Ok(frames)
    }

    // ---- Intruder ----

    pub fn create_intruder_campaign(
        &self,
        name: &str,
        attack_type: &str,
        req_raw: &str,
        url_hint: Option<&str>,
    ) -> Result<i64, String> {
        let now = chrono::Utc::now().to_rfc3339();
        let conn = self.conn.lock();
        conn.execute(
            "INSERT INTO intruder_campaigns
             (name, attack_type, req_raw, url_hint, follow, set_map, threads, delay_ms,
              max_requests, extractors, status, created_at, updated_at)
             VALUES (?1,?2,?3,?4,0,'[]',4,0,0,'[]','draft',?5,?5)",
            params![name, attack_type, req_raw, url_hint, now],
        )
        .map_err(|e| e.to_string())?;
        Ok(conn.last_insert_rowid())
    }

    /// Light rows for campaign tabs (no request text).
    pub fn list_intruder_campaigns(&self) -> Result<Vec<IntruderCampaignSummary>, String> {
        let conn = self.conn.lock();
        let mut stmt = conn
            .prepare(
                "SELECT id, name, attack_type, req_raw, status, updated_at
                 FROM intruder_campaigns ORDER BY updated_at DESC, id DESC",
            )
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map([], |r| {
                let req_raw: String = r.get(3)?;
                Ok(IntruderCampaignSummary {
                    id: r.get(0)?,
                    name: r.get(1)?,
                    attack_type: r.get(2)?,
                    positions: (req_raw.matches('§').count() / 2) as i64,
                    status: r.get(4)?,
                    updated_at: r.get(5)?,
                })
            })
            .map_err(|e| e.to_string())?;
        rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())
    }

    pub fn get_intruder_campaign(&self, id: i64) -> Result<Option<IntruderCampaign>, String> {
        let conn = self.conn.lock();
        conn.query_row(
            "SELECT id, name, attack_type, req_raw, url_hint, follow, set_map, threads,
                    delay_ms, max_requests, extractors, status, created_at, updated_at
             FROM intruder_campaigns WHERE id = ?1",
            params![id],
            |r| {
                Ok(IntruderCampaign {
                    id: r.get(0)?,
                    name: r.get(1)?,
                    attack_type: r.get(2)?,
                    req_raw: r.get(3)?,
                    url_hint: r.get(4)?,
                    follow: r.get::<_, i64>(5)? != 0,
                    set_map: r.get(6)?,
                    threads: r.get(7)?,
                    delay_ms: r.get(8)?,
                    max_requests: r.get(9)?,
                    extractors: r.get(10)?,
                    status: r.get(11)?,
                    created_at: r.get(12)?,
                    updated_at: r.get(13)?,
                })
            },
        )
        .optional()
        .map_err(|e| e.to_string())
    }

    pub fn update_intruder_campaign(&self, c: &IntruderCampaign) -> Result<(), String> {
        let now = chrono::Utc::now().to_rfc3339();
        let conn = self.conn.lock();
        conn.execute(
            "UPDATE intruder_campaigns SET name=?1, attack_type=?2, req_raw=?3, url_hint=?4,
              follow=?5, set_map=?6, threads=?7, delay_ms=?8, max_requests=?9, extractors=?10,
              status=?11, updated_at=?12 WHERE id=?13",
            params![
                c.name,
                c.attack_type,
                c.req_raw,
                c.url_hint,
                c.follow as i64,
                c.set_map,
                c.threads,
                c.delay_ms,
                c.max_requests,
                c.extractors,
                c.status,
                now,
                c.id
            ],
        )
        .map_err(|e| e.to_string())?;
        Ok(())
    }

    pub fn set_intruder_campaign_status(&self, id: i64, status: &str) -> Result<(), String> {
        let now = chrono::Utc::now().to_rfc3339();
        let conn = self.conn.lock();
        conn.execute(
            "UPDATE intruder_campaigns SET status=?1, updated_at=?2 WHERE id=?3",
            params![status, now, id],
        )
        .map_err(|e| e.to_string())?;
        Ok(())
    }

    pub fn delete_intruder_campaign(&self, id: i64) -> Result<(), String> {
        let conn = self.conn.lock();
        conn.execute("DELETE FROM intruder_results WHERE campaign_id = ?1", params![id])
            .map_err(|e| e.to_string())?;
        conn.execute("DELETE FROM intruder_payloads WHERE campaign_id = ?1", params![id])
            .map_err(|e| e.to_string())?;
        conn.execute("DELETE FROM intruder_campaigns WHERE id = ?1", params![id])
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    /// Crash recovery: anything left `running`/`paused` from a previous session becomes `stopped`.
    pub fn reset_intruder_running(&self) -> Result<usize, String> {
        let now = chrono::Utc::now().to_rfc3339();
        let conn = self.conn.lock();
        conn.execute(
            "UPDATE intruder_campaigns SET status='stopped', updated_at=?1
             WHERE status IN ('running','paused')",
            params![now],
        )
        .map_err(|e| e.to_string())
    }

    pub fn save_intruder_payload_set(
        &self,
        campaign_id: i64,
        set_index: i64,
        name: &str,
        kind: &str,
        items: &str,
        start_i: Option<i64>,
        end_i: Option<i64>,
        step_i: Option<i64>,
        processors: &str,
    ) -> Result<(), String> {
        let conn = self.conn.lock();
        conn.execute(
            "INSERT INTO intruder_payloads
               (campaign_id, set_index, name, kind, items, start_i, end_i, step_i, processors)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9)
             ON CONFLICT(campaign_id, set_index) DO UPDATE SET
               name=excluded.name, kind=excluded.kind, items=excluded.items,
               start_i=excluded.start_i, end_i=excluded.end_i, step_i=excluded.step_i,
               processors=excluded.processors",
            params![campaign_id, set_index, name, kind, items, start_i, end_i, step_i, processors],
        )
        .map_err(|e| e.to_string())?;
        Ok(())
    }

    pub fn clear_intruder_payloads(&self, campaign_id: i64) -> Result<usize, String> {
        let conn = self.conn.lock();
        conn.execute(
            "DELETE FROM intruder_payloads WHERE campaign_id = ?1",
            params![campaign_id],
        )
        .map_err(|e| e.to_string())
    }

    pub fn list_intruder_payload_sets(
        &self,
        campaign_id: i64,
    ) -> Result<Vec<IntruderPayloadSet>, String> {
        let conn = self.conn.lock();
        let mut stmt = conn
            .prepare(
                "SELECT id, campaign_id, set_index, name, kind, items, start_i, end_i, step_i, processors
                 FROM intruder_payloads WHERE campaign_id = ?1 ORDER BY set_index",
            )
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map(params![campaign_id], |r| {
                Ok(IntruderPayloadSet {
                    id: r.get(0)?,
                    campaign_id: r.get(1)?,
                    set_index: r.get(2)?,
                    name: r.get(3)?,
                    kind: r.get(4)?,
                    items: r.get(5)?,
                    start_i: r.get(6)?,
                    end_i: r.get(7)?,
                    step_i: r.get(8)?,
                    processors: r.get(9)?,
                })
            })
            .map_err(|e| e.to_string())?;
        rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())
    }

    /// Batch insert (single transaction) — called from the attack engine per join batch.
    pub fn insert_intruder_results(
        &self,
        campaign_id: i64,
        rows: &[NewIntruderResult],
    ) -> Result<(), String> {
        if rows.is_empty() {
            return Ok(());
        }
        let now = chrono::Utc::now().to_rfc3339();
        let conn = self.conn.lock();
        let tx = conn.unchecked_transaction().map_err(|e| e.to_string())?;
        {
            let mut stmt = tx
                .prepare(
                    "INSERT INTO intruder_results
                       (campaign_id, seq, combo, status, resp_len, time_ms, error,
                        req_raw, resp_raw, extracted, created_at)
                     VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)",
                )
                .map_err(|e| e.to_string())?;
            for r in rows {
                stmt.execute(params![
                    campaign_id,
                    r.seq,
                    r.combo,
                    r.status,
                    r.resp_len,
                    r.time_ms,
                    r.error,
                    r.req_raw,
                    r.resp_raw,
                    r.extracted,
                    now
                ])
                .map_err(|e| e.to_string())?;
            }
        }
        tx.commit().map_err(|e| e.to_string())
    }

    /// Attack-order rows (oldest first) without the message bodies.
    pub fn list_intruder_results(
        &self,
        campaign_id: i64,
        offset: i64,
        limit: i64,
    ) -> Result<Vec<IntruderResultRow>, String> {
        let conn = self.conn.lock();
        let mut stmt = conn
            .prepare(
                "SELECT seq, combo, status, resp_len, time_ms, error, extracted
                 FROM intruder_results WHERE campaign_id = ?1 ORDER BY seq LIMIT ?2 OFFSET ?3",
            )
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map(params![campaign_id, limit, offset], |r| {
                Ok(IntruderResultRow {
                    seq: r.get(0)?,
                    combo: r.get(1)?,
                    status: r.get(2)?,
                    resp_len: r.get(3)?,
                    time_ms: r.get(4)?,
                    error: r.get(5)?,
                    extracted: r.get(6)?,
                })
            })
            .map_err(|e| e.to_string())?;
        rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())
    }

    pub fn count_intruder_results(&self, campaign_id: i64) -> Result<i64, String> {
        let conn = self.conn.lock();
        conn.query_row(
            "SELECT COUNT(*) FROM intruder_results WHERE campaign_id = ?1",
            params![campaign_id],
            |r| r.get::<_, i64>(0),
        )
        .map_err(|e| e.to_string())
    }

    pub fn get_intruder_result(
        &self,
        campaign_id: i64,
        seq: i64,
    ) -> Result<Option<IntruderResultFull>, String> {
        let conn = self.conn.lock();
        conn.query_row(
            "SELECT seq, combo, status, resp_len, time_ms, error, req_raw, resp_raw, extracted
             FROM intruder_results WHERE campaign_id = ?1 AND seq = ?2",
            params![campaign_id, seq],
            |r| {
                Ok(IntruderResultFull {
                    seq: r.get(0)?,
                    combo: r.get(1)?,
                    status: r.get(2)?,
                    resp_len: r.get(3)?,
                    time_ms: r.get(4)?,
                    error: r.get(5)?,
                    req_raw: r.get(6)?,
                    resp_raw: r.get(7)?,
                    extracted: r.get(8)?,
                })
            },
        )
        .optional()
        .map_err(|e| e.to_string())
    }

    pub fn clear_intruder_results(&self, campaign_id: i64) -> Result<usize, String> {
        let conn = self.conn.lock();
        conn.execute(
            "DELETE FROM intruder_results WHERE campaign_id = ?1",
            params![campaign_id],
        )
        .map_err(|e| e.to_string())
    }

    /// Insert a scanner finding. Dedupe on (check_id, host, path, param):
    /// returns Some(new id) when inserted, None when an identical finding exists.
    pub fn insert_issue(&self, n: &NewIssue) -> Result<Option<i64>, String> {
        let now = chrono::Utc::now().to_rfc3339();
        let dedupe_key = format!(
            "{}|{}|{}|{}",
            n.check_id, n.host, n.path, n.param
        );
        let conn = self.conn.lock();
        let changed = conn
            .execute(
                "INSERT OR IGNORE INTO issues
                   (check_id, name, severity, confidence, host, url, method, path, param,
                    description, remediation, evidence, exchange_id, status, dedupe_key, created_at)
                 VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,'open',?14,?15)",
                params![
                    n.check_id,
                    n.name,
                    n.severity,
                    n.confidence,
                    n.host,
                    n.url,
                    n.method,
                    n.path,
                    n.param,
                    n.description,
                    n.remediation,
                    n.evidence,
                    n.exchange_id,
                    dedupe_key,
                    now
                ],
            )
            .map_err(|e| e.to_string())?;
        if changed == 0 {
            return Ok(None);
        }
        Ok(conn.last_insert_rowid().into())
    }

    /// Filtered issue list (newest first). `severities` empty = all; host/status/text empty = all.
    pub fn list_issues(
        &self,
        severities: &[String],
        host: &str,
        status: &str,
        text: &str,
        offset: i64,
        limit: i64,
    ) -> Result<Vec<Issue>, String> {
        let conn = self.conn.lock();
        let mut sql = String::from(
            "SELECT id, check_id, name, severity, confidence, host, url, method, path, param,
                    description, remediation, evidence, exchange_id, status, created_at
             FROM issues WHERE 1=1",
        );
        let mut args: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
        if !severities.is_empty() {
            let holes = severities
                .iter()
                .enumerate()
                .map(|(i, _)| format!("?{}", i + 1))
                .collect::<Vec<_>>()
                .join(",");
            sql.push_str(&format!(" AND severity IN ({holes})"));
            for s in severities {
                args.push(Box::new(s.clone()));
            }
        }
        if !host.is_empty() {
            sql.push_str(&format!(" AND host = ?{}", args.len() + 1));
            args.push(Box::new(host.to_string()));
        }
        if !status.is_empty() {
            sql.push_str(&format!(" AND status = ?{}", args.len() + 1));
            args.push(Box::new(status.to_string()));
        }
        if !text.is_empty() {
            sql.push_str(&format!(
                " AND (name LIKE ?{} OR url LIKE ?{} OR param LIKE ?{} OR evidence LIKE ?{})",
                args.len() + 1,
                args.len() + 2,
                args.len() + 3,
                args.len() + 4
            ));
            let pat = format!("%{text}%");
            args.push(Box::new(pat.clone()));
            args.push(Box::new(pat.clone()));
            args.push(Box::new(pat.clone()));
            args.push(Box::new(pat));
        }
        sql.push_str(" ORDER BY created_at DESC, id DESC LIMIT ?");
        args.push(Box::new(limit));
        sql.push_str(" OFFSET ?");
        args.push(Box::new(offset));
        let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map(rusqlite::params_from_iter(args.iter().map(|b| b.as_ref())), row_to_issue)
            .map_err(|e| e.to_string())?;
        rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())
    }

    pub fn get_issue(&self, id: i64) -> Result<Option<Issue>, String> {
        let conn = self.conn.lock();
        conn.query_row(
            "SELECT id, check_id, name, severity, confidence, host, url, method, path, param,
                    description, remediation, evidence, exchange_id, status, created_at
             FROM issues WHERE id = ?1",
            params![id],
            row_to_issue,
        )
        .optional()
        .map_err(|e| e.to_string())
    }

    pub fn update_issue_status(&self, id: i64, status: &str) -> Result<usize, String> {
        let conn = self.conn.lock();
        conn.execute(
            "UPDATE issues SET status = ?2 WHERE id = ?1",
            params![id, status],
        )
        .map_err(|e| e.to_string())
    }

    pub fn delete_issue(&self, id: i64) -> Result<usize, String> {
        let conn = self.conn.lock();
        conn.execute("DELETE FROM issues WHERE id = ?1", params![id])
            .map_err(|e| e.to_string())
    }

    /// Counts per severity for the dashboard badges (all statuses).
    pub fn issues_counts(&self) -> Result<Vec<(String, i64)>, String> {
        let conn = self.conn.lock();
        let mut stmt = conn
            .prepare("SELECT severity, COUNT(*) FROM issues GROUP BY severity")
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))
            .map_err(|e| e.to_string())?;
        rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())
    }

    /// Open (not false-positive) issue ids for a host — used to link sitemap hosts.
    pub fn count_issues_open(&self) -> Result<i64, String> {
        let conn = self.conn.lock();
        conn.query_row(
            "SELECT COUNT(*) FROM issues WHERE status != 'false-positive'",
            [],
            |r| r.get::<_, i64>(0),
        )
        .map_err(|e| e.to_string())
    }

    // ----------------------------------------------------------- scan tasks

    pub fn insert_scan_task(
        &self,
        kind: &str,
        target: &str,
        config_id: Option<i64>,
    ) -> Result<i64, String> {
        let now = chrono::Utc::now().to_rfc3339();
        let conn = self.conn.lock();
        conn.execute(
            "INSERT INTO scan_tasks (kind, target, config_id, status, created_at, updated_at)
             VALUES (?1,?2,?3,'running',?4,?4)",
            params![kind, target, config_id, now],
        )
        .map_err(|e| e.to_string())?;
        Ok(conn.last_insert_rowid())
    }

    pub fn list_scan_tasks(&self) -> Result<Vec<ScanTask>, String> {
        let conn = self.conn.lock();
        let mut stmt = conn
            .prepare(
                "SELECT id, kind, target, config_id, status, pages_crawled, requests_sent,
                        issues_found, error, created_at, updated_at
                 FROM scan_tasks ORDER BY id DESC",
            )
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map([], row_to_scan_task)
            .map_err(|e| e.to_string())?;
        rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())
    }

    pub fn get_scan_task(&self, id: i64) -> Result<Option<ScanTask>, String> {
        let conn = self.conn.lock();
        conn.query_row(
            "SELECT id, kind, target, config_id, status, pages_crawled, requests_sent,
                    issues_found, error, created_at, updated_at
             FROM scan_tasks WHERE id = ?1",
            params![id],
            row_to_scan_task,
        )
        .optional()
        .map_err(|e| e.to_string())
    }

    /// Update counters / status for a running scan task.
    pub fn update_scan_task(
        &self,
        id: i64,
        status: &str,
        pages_crawled: i64,
        requests_sent: i64,
        issues_found: i64,
        error: Option<&str>,
    ) -> Result<(), String> {
        let now = chrono::Utc::now().to_rfc3339();
        let conn = self.conn.lock();
        conn.execute(
            "UPDATE scan_tasks SET status=?2, pages_crawled=?3, requests_sent=?4,
                    issues_found=?5, error=?6, updated_at=?7
             WHERE id=?1",
            params![id, status, pages_crawled, requests_sent, issues_found, error, now],
        )
        .map_err(|e| e.to_string())?;
        Ok(())
    }

    /// Crash recovery: any task still marked running is marked cancelled.
    pub fn reset_scan_tasks_running(&self) -> Result<usize, String> {
        let now = chrono::Utc::now().to_rfc3339();
        let conn = self.conn.lock();
        conn.execute(
            "UPDATE scan_tasks SET status='cancelled', updated_at=?1 WHERE status IN ('running','paused')",
            params![now],
        )
        .map_err(|e| e.to_string())
    }

    // ---------------------------------------------------------- scan configs

    pub fn list_scan_configs(&self) -> Result<Vec<ScanConfig>, String> {
        let conn = self.conn.lock();
        let mut stmt = conn
            .prepare(
                "SELECT id, name, passive_enabled, active_checks, crawl_max_pages,
                        crawl_max_depth, audit_threads, throttle_ms, is_default, created_at
                 FROM scan_configs ORDER BY is_default DESC, name",
            )
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map([], row_to_scan_config)
            .map_err(|e| e.to_string())?;
        rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())
    }

    pub fn get_scan_config(&self, id: i64) -> Result<Option<ScanConfig>, String> {
        let conn = self.conn.lock();
        conn.query_row(
            "SELECT id, name, passive_enabled, active_checks, crawl_max_pages,
                    crawl_max_depth, audit_threads, throttle_ms, is_default, created_at
             FROM scan_configs WHERE id = ?1",
            params![id],
            row_to_scan_config,
        )
        .optional()
        .map_err(|e| e.to_string())
    }

    /// Insert or update (by id); clears is_default on all others when is_default is true.
    pub fn save_scan_config(
        &self,
        id: Option<i64>,
        name: &str,
        passive_enabled: bool,
        active_checks: &[String],
        crawl_max_pages: i64,
        crawl_max_depth: i64,
        audit_threads: i64,
        throttle_ms: i64,
        is_default: bool,
    ) -> Result<i64, String> {
        let checks_json = serde_json::to_string(active_checks).unwrap_or_else(|_| "[]".into());
        let now = chrono::Utc::now().to_rfc3339();
        let conn = self.conn.lock();
        if is_default {
            conn.execute("UPDATE scan_configs SET is_default=0", [])
                .map_err(|e| e.to_string())?;
        }
        let id = match id {
            Some(existing) => {
                conn.execute(
                    "UPDATE scan_configs SET name=?2, passive_enabled=?3, active_checks=?4,
                            crawl_max_pages=?5, crawl_max_depth=?6, audit_threads=?7,
                            throttle_ms=?8, is_default=?9
                     WHERE id=?1",
                    params![
                        existing,
                        name,
                        passive_enabled,
                        checks_json,
                        crawl_max_pages,
                        crawl_max_depth,
                        audit_threads,
                        throttle_ms,
                        is_default
                    ],
                )
                .map_err(|e| e.to_string())?;
                existing
            }
            None => {
                conn.execute(
                    "INSERT INTO scan_configs
                       (name, passive_enabled, active_checks, crawl_max_pages, crawl_max_depth,
                        audit_threads, throttle_ms, is_default, created_at)
                     VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9)",
                    params![
                        name,
                        passive_enabled,
                        checks_json,
                        crawl_max_pages,
                        crawl_max_depth,
                        audit_threads,
                        throttle_ms,
                        is_default,
                        now
                    ],
                )
                .map_err(|e| e.to_string())?;
                conn.last_insert_rowid()
            }
        };
        Ok(id)
    }

    pub fn delete_scan_config(&self, id: i64) -> Result<usize, String> {
        let conn = self.conn.lock();
        conn.execute("DELETE FROM scan_configs WHERE id = ?1", params![id])
            .map_err(|e| e.to_string())
    }

    // ----------------------------------------------------------- extensions

    pub fn list_extensions(&self) -> Result<Vec<ExtensionRow>, String> {
        let conn = self.conn.lock();
        let mut stmt = conn
            .prepare(
                "SELECT id, name, path, enabled, error, loaded_at, created_at
                 FROM extensions ORDER BY id",
            )
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map([], row_to_extension)
            .map_err(|e| e.to_string())?;
        rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())
    }

    pub fn get_extension(&self, id: i64) -> Result<Option<ExtensionRow>, String> {
        let conn = self.conn.lock();
        conn.query_row(
            "SELECT id, name, path, enabled, error, loaded_at, created_at
             FROM extensions WHERE id = ?1",
            params![id],
            row_to_extension,
        )
        .optional()
        .map_err(|e| e.to_string())
    }

    pub fn insert_extension(&self, name: &str, path: &str) -> Result<i64, String> {
        let now = chrono::Utc::now().to_rfc3339();
        let conn = self.conn.lock();
        conn.execute(
            "INSERT INTO extensions (name, path, enabled, created_at) VALUES (?1,?2,1,?3)",
            params![name, path, now],
        )
        .map_err(|e| e.to_string())?;
        Ok(conn.last_insert_rowid())
    }

    pub fn set_extension_enabled(&self, id: i64, enabled: bool) -> Result<(), String> {
        let conn = self.conn.lock();
        conn.execute(
            "UPDATE extensions SET enabled = ?2 WHERE id = ?1",
            params![id, enabled as i64],
        )
        .map_err(|e| e.to_string())?;
        Ok(())
    }

    pub fn set_extension_error(&self, id: i64, error: Option<&str>) -> Result<(), String> {
        let now = chrono::Utc::now().to_rfc3339();
        let conn = self.conn.lock();
        conn.execute(
            "UPDATE extensions SET error = ?2, loaded_at = ?3 WHERE id = ?1",
            params![id, error, now],
        )
        .map_err(|e| e.to_string())?;
        Ok(())
    }

    pub fn delete_extension(&self, id: i64) -> Result<usize, String> {
        let conn = self.conn.lock();
        conn.execute("DELETE FROM extensions WHERE id = ?1", params![id])
            .map_err(|e| e.to_string())
    }

    // ----------------------------------------------------------- upstream (8a)

    pub fn list_upstream(&self) -> Result<Vec<UpstreamRule>, String> {
        let conn = self.conn.lock();
        let mut stmt = conn
            .prepare(
                "SELECT id, position, host_pattern, proxy_host, proxy_port, use_tls, enabled
                 FROM upstream_rules ORDER BY position",
            )
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map([], |r| {
                Ok(UpstreamRule {
                    id: r.get(0)?,
                    position: r.get(1)?,
                    host_pattern: r.get(2)?,
                    proxy_host: r.get(3)?,
                    proxy_port: r.get(4)?,
                    use_tls: r.get::<_, i64>(5)? != 0,
                    enabled: r.get::<_, i64>(6)? != 0,
                })
            })
            .map_err(|e| e.to_string())?;
        rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())
    }

    pub fn insert_upstream(
        &self,
        host_pattern: &str,
        proxy_host: &str,
        proxy_port: u16,
        use_tls: bool,
    ) -> Result<i64, String> {
        let conn = self.conn.lock();
        let pos: i64 = conn
            .query_row("SELECT COALESCE(MAX(position),0)+1 FROM upstream_rules", [], |r| r.get(0))
            .unwrap_or(1);
        conn.execute(
            "INSERT INTO upstream_rules (position, host_pattern, proxy_host, proxy_port, use_tls, enabled)
             VALUES (?1,?2,?3,?4,?5,1)",
            params![pos, host_pattern, proxy_host, proxy_port, use_tls as i64],
        )
        .map_err(|e| e.to_string())?;
        Ok(conn.last_insert_rowid())
    }

    pub fn delete_upstream(&self, id: i64) -> Result<usize, String> {
        let conn = self.conn.lock();
        conn.execute("DELETE FROM upstream_rules WHERE id = ?1", params![id])
            .map_err(|e| e.to_string())
    }

    // ----------------------------------------------------------- settings (8a)

    pub fn get_proxy_setting(&self, key: &str) -> Result<Option<String>, String> {
        let conn = self.conn.lock();
        conn.query_row(
            "SELECT value FROM proxy_settings WHERE key = ?1",
            params![key],
            |r| r.get(0),
        )
        .optional()
        .map_err(|e| e.to_string())
    }

    pub fn set_proxy_setting(&self, key: &str, value: &str) -> Result<(), String> {
        let conn = self.conn.lock();
        conn.execute(
            "INSERT OR REPLACE INTO proxy_settings (key, value) VALUES (?1,?2)",
            params![key, value],
        )
        .map_err(|e| e.to_string())?;
        Ok(())
    }

    // ----------------------------------------------------------- session (8b)

    pub fn list_session_macros(&self) -> Result<Vec<crate::session::SessionMacro>, String> {
        let conn = self.conn.lock();
        let mut stmt = conn
            .prepare("SELECT id, name, requests FROM session_macros ORDER BY id")
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map([], |r| {
                let id: i64 = r.get(0)?;
                let name: String = r.get(1)?;
                let requests_json: String = r.get(2)?;
                let requests: Vec<String> =
                    serde_json::from_str(&requests_json).unwrap_or_default();
                Ok(crate::session::SessionMacro { id, name, requests })
            })
            .map_err(|e| e.to_string())?;
        rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())
    }

    pub fn get_session_macro(&self, id: i64) -> Result<Option<crate::session::SessionMacro>, String> {
        let conn = self.conn.lock();
        conn.query_row(
            "SELECT id, name, requests FROM session_macros WHERE id = ?1",
            params![id],
            |r| {
                let id: i64 = r.get(0)?;
                let name: String = r.get(1)?;
                let requests_json: String = r.get(2)?;
                let requests: Vec<String> =
                    serde_json::from_str(&requests_json).unwrap_or_default();
                Ok(crate::session::SessionMacro { id, name, requests })
            },
        )
        .optional()
        .map_err(|e| e.to_string())
    }

    pub fn insert_session_macro(&self, name: &str, requests: &[String]) -> Result<i64, String> {
        let conn = self.conn.lock();
        let json = serde_json::to_string(requests).map_err(|e| e.to_string())?;
        conn.execute(
            "INSERT INTO session_macros (name, requests) VALUES (?1,?2)",
            params![name, json],
        )
        .map_err(|e| e.to_string())?;
        Ok(conn.last_insert_rowid())
    }

    pub fn delete_session_macro(&self, id: i64) -> Result<usize, String> {
        let conn = self.conn.lock();
        conn.execute("DELETE FROM session_macros WHERE id = ?1", params![id])
            .map_err(|e| e.to_string())
    }

    pub fn list_session_rules(&self) -> Result<Vec<crate::session::SessionRule>, String> {
        let conn = self.conn.lock();
        let mut stmt = conn
            .prepare(
                "SELECT id, position, name, tool, match_host, macro_id, updates, enabled
                 FROM session_rules ORDER BY position",
            )
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map([], |r| {
                let updates_json: String = r.get(6)?;
                let updates: Vec<crate::session::SessionUpdate> =
                    serde_json::from_str(&updates_json).unwrap_or_default();
                Ok(crate::session::SessionRule {
                    id: r.get(0)?,
                    position: r.get(1)?,
                    name: r.get(2)?,
                    tool: r.get(3)?,
                    match_host: r.get(4)?,
                    macro_id: r.get(5)?,
                    updates,
                    enabled: r.get::<_, i64>(7)? != 0,
                })
            })
            .map_err(|e| e.to_string())?;
        rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())
    }

    pub fn insert_session_rule(
        &self,
        name: &str,
        tool: &str,
        match_host: &str,
        macro_id: Option<i64>,
        updates: &[crate::session::SessionUpdate],
    ) -> Result<i64, String> {
        let conn = self.conn.lock();
        let pos: i64 = conn
            .query_row("SELECT COALESCE(MAX(position),0)+1 FROM session_rules", [], |r| r.get(0))
            .unwrap_or(1);
        let updates_json = serde_json::to_string(updates).map_err(|e| e.to_string())?;
        conn.execute(
            "INSERT INTO session_rules (position, name, tool, match_host, macro_id, updates, enabled)
             VALUES (?1,?2,?3,?4,?5,?6,1)",
            params![pos, name, tool, match_host, macro_id, updates_json],
        )
        .map_err(|e| e.to_string())?;
        Ok(conn.last_insert_rowid())
    }

    pub fn delete_session_rule(&self, id: i64) -> Result<usize, String> {
        let conn = self.conn.lock();
        conn.execute("DELETE FROM session_rules WHERE id = ?1", params![id])
            .map_err(|e| e.to_string())
    }

    // ----------------------------------------------------------- collaborator

    pub fn insert_collab_payload(
        &self,
        label: &str,
        domain: &str,
        kind: &str,
        value: &str,
        created_at: &str,
    ) -> Result<crate::collaborator::CollabPayload, String> {
        let conn = self.conn.lock();
        conn.execute(
            "INSERT INTO collab_payloads (label, domain, kind, value, created_at)
             VALUES (?1,?2,?3,?4,?5)",
            params![label, domain, kind, value, created_at],
        )
        .map_err(|e| e.to_string())?;
        Ok(crate::collaborator::CollabPayload {
            id: conn.last_insert_rowid(),
            label: label.to_string(),
            domain: domain.to_string(),
            kind: kind.to_string(),
            value: value.to_string(),
            created_at: created_at.to_string(),
        })
    }

    pub fn list_collab_payloads(
        &self,
    ) -> Result<Vec<crate::collaborator::CollabPayload>, String> {
        let conn = self.conn.lock();
        let mut stmt = conn
            .prepare(
                "SELECT id, label, domain, kind, value, created_at
                 FROM collab_payloads ORDER BY id DESC",
            )
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map([], |r| {
                Ok(crate::collaborator::CollabPayload {
                    id: r.get(0)?,
                    label: r.get(1)?,
                    domain: r.get(2)?,
                    kind: r.get(3)?,
                    value: r.get(4)?,
                    created_at: r.get(5)?,
                })
            })
            .map_err(|e| e.to_string())?;
        rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())
    }

    pub fn delete_collab_payload(&self, id: i64) -> Result<usize, String> {
        let conn = self.conn.lock();
        conn.execute("DELETE FROM collab_payloads WHERE id = ?1", params![id])
            .map_err(|e| e.to_string())
    }

    pub fn insert_collab_interaction(
        &self,
        payload_id: Option<&i64>,
        subdomain: &str,
        protocol: &str,
        source: &str,
        detail: &serde_json::Value,
        seen_at: &str,
    ) -> Result<crate::collaborator::CollabInteraction, String> {
        let conn = self.conn.lock();
        conn.execute(
            "INSERT INTO collab_interactions (payload_id, subdomain, protocol, source, detail, seen_at)
             VALUES (?1,?2,?3,?4,?5,?6)",
            params![payload_id, subdomain, protocol, source, detail.to_string(), seen_at],
        )
        .map_err(|e| e.to_string())?;
        Ok(crate::collaborator::CollabInteraction {
            id: conn.last_insert_rowid(),
            payload_id: payload_id.copied(),
            subdomain: subdomain.to_string(),
            protocol: protocol.to_string(),
            source: source.to_string(),
            detail: detail.clone(),
            seen_at: seen_at.to_string(),
        })
    }

    pub fn list_collab_interactions(
        &self,
        payload_id: Option<i64>,
    ) -> Result<Vec<crate::collaborator::CollabInteraction>, String> {
        let conn = self.conn.lock();
        let (sql, p): (&str, Option<i64>) = match payload_id {
            Some(pid) => (
                "SELECT id, payload_id, subdomain, protocol, source, detail, seen_at
                 FROM collab_interactions WHERE payload_id = ?1 ORDER BY id DESC",
                Some(pid),
            ),
            None => (
                "SELECT id, payload_id, subdomain, protocol, source, detail, seen_at
                 FROM collab_interactions ORDER BY id DESC",
                None,
            ),
        };
        let mut stmt = conn.prepare(sql).map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map([p], |r| {
                let detail_str: String = r.get(5)?;
                Ok(crate::collaborator::CollabInteraction {
                    id: r.get(0)?,
                    payload_id: r.get(1)?,
                    subdomain: r.get(2)?,
                    protocol: r.get(3)?,
                    source: r.get(4)?,
                    detail: serde_json::from_str(&detail_str).unwrap_or(serde_json::Value::Null),
                    seen_at: r.get(6)?,
                })
            })
            .map_err(|e| e.to_string())?;
        rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())
    }

    pub fn clear_collab_interactions(&self) -> Result<usize, String> {
        let conn = self.conn.lock();
        conn.execute("DELETE FROM collab_interactions", [])
            .map_err(|e| e.to_string())
    }
}

fn row_to_extension(r: &rusqlite::Row<'_>) -> rusqlite::Result<ExtensionRow> {
    Ok(ExtensionRow {
        id: r.get(0)?,
        name: r.get(1)?,
        path: r.get(2)?,
        enabled: r.get::<_, i64>(3)? != 0,
        error: r.get(4)?,
        loaded_at: r.get(5)?,
        created_at: r.get(6)?,
    })
}

fn row_to_scan_config(r: &rusqlite::Row<'_>) -> rusqlite::Result<ScanConfig> {
    let checks_json: String = r.get(3)?;
    Ok(ScanConfig {
        id: r.get(0)?,
        name: r.get(1)?,
        passive_enabled: r.get::<_, i64>(2)? != 0,
        active_checks: serde_json::from_str(&checks_json).unwrap_or_default(),
        crawl_max_pages: r.get(4)?,
        crawl_max_depth: r.get(5)?,
        audit_threads: r.get(6)?,
        throttle_ms: r.get(7)?,
        is_default: r.get::<_, i64>(8)? != 0,
        created_at: r.get(9)?,
    })
}

fn row_to_scan_task(r: &rusqlite::Row<'_>) -> rusqlite::Result<ScanTask> {
    Ok(ScanTask {
        id: r.get(0)?,
        kind: r.get(1)?,
        target: r.get(2)?,
        config_id: r.get(3)?,
        status: r.get(4)?,
        pages_crawled: r.get(5)?,
        requests_sent: r.get(6)?,
        issues_found: r.get(7)?,
        error: r.get(8)?,
        created_at: r.get(9)?,
        updated_at: r.get(10)?,
    })
}

fn row_to_issue(r: &rusqlite::Row<'_>) -> rusqlite::Result<Issue> {
    Ok(Issue {
        id: r.get(0)?,
        check_id: r.get(1)?,
        name: r.get(2)?,
        severity: r.get(3)?,
        confidence: r.get(4)?,
        host: r.get(5)?,
        url: r.get(6)?,
        method: r.get(7)?,
        path: r.get(8)?,
        param: r.get(9)?,
        description: r.get(10)?,
        remediation: r.get(11)?,
        evidence: r.get(12)?,
        exchange_id: r.get(13)?,
        status: r.get(14)?,
        created_at: r.get(15)?,
    })
}

pub fn scope_matches(rule: &ScopeRule, host: &str, url: &str) -> bool {
    if !rule.enabled {
        return false;
    }
    match rule.kind.as_str() {
        "host" => {
            let p = rule.pattern.trim().trim_start_matches("*.");
            host.eq_ignore_ascii_case(p)
                || host
                    .to_lowercase()
                    .ends_with(&format!(".{}", p.to_lowercase()))
        }
        "url_prefix" => url.to_lowercase().starts_with(&rule.pattern.to_lowercase()),
        "regex" => regex_find(&rule.pattern, url),
        _ => false,
    }
}

pub fn is_in_scope(rules: &[ScopeRule], host: &str, url: &str) -> bool {
    let has_include = rules.iter().any(|r| r.enabled && r.mode == "include");
    let excluded = rules
        .iter()
        .any(|r| r.enabled && r.mode == "exclude" && scope_matches(r, host, url));
    if excluded {
        return false;
    }
    if !has_include {
        return true;
    }
    rules
        .iter()
        .any(|r| r.enabled && r.mode == "include" && scope_matches(r, host, url))
}

fn regex_find(pattern: &str, text: &str) -> bool {
    regex::Regex::new(pattern)
        .map(|re| re.is_match(text))
        .unwrap_or(false)
}

pub fn apply_match_rules(rules: &[MatchRule], phase: &str, raw: &str) -> String {
    let mut out = raw.to_string();
    for r in rules.iter().filter(|r| r.enabled && r.phase == phase) {
        match r.match_kind.as_str() {
            "literal" => {
                if r.match_val.is_empty() {
                    continue;
                }
                out = out.replace(&r.match_val, &r.replace_val);
            }
            "regex" => {
                if let Ok(re) = regex::Regex::new(&r.match_val) {
                    out = re.replace_all(&out, r.replace_val.as_str()).into_owned();
                }
            }
            "header" => {
                let name = r.match_val.split(':').next().unwrap_or("").trim();
                if name.is_empty() {
                    continue;
                }
                let want = r.match_val.splitn(2, ':').nth(1).map(|s| s.trim().to_string());
                let mut lines: Vec<String> = out.split("\r\n").map(|s| s.to_string()).collect();
                for line in lines.iter_mut() {
                    if let Some((k, v)) = line.split_once(':') {
                        if k.trim().eq_ignore_ascii_case(name) {
                            let replace = match &want {
                                Some(w) => v.trim().contains(w.as_str()),
                                None => true,
                            };
                            if replace {
                                *line = format!("{}: {}", name, r.replace_val);
                            }
                        }
                    }
                }
                out = lines.join("\r\n");
            }
            _ => {}
        }
    }
    out
}

pub fn process_with_rules(rules: &[MatchRule], phase: &str, raw: &[u8]) -> Vec<u8> {
    if !rules.iter().any(|r| r.enabled && r.phase == phase) {
        return raw.to_vec();
    }
    let text = String::from_utf8_lossy(raw);
    let out = apply_match_rules(rules, phase, &text);
    fix_content_length(&out).into_bytes()
}

pub fn fix_content_length(raw: &str) -> String {
    let Some(idx) = raw.find("\r\n\r\n") else {
        return raw.to_string();
    };
    let head = &raw[..idx];
    let body = &raw[idx + 4..];
    if head.to_ascii_lowercase().contains("transfer-encoding:") {
        return raw.to_string();
    }
    let mut lines: Vec<String> = head.split("\r\n").map(|s| s.to_string()).collect();
    let mut found = false;
    for line in lines.iter_mut().skip(1) {
        if line.to_ascii_lowercase().starts_with("content-length:") {
            *line = format!("Content-Length: {}", body.len());
            found = true;
        }
    }
    if !found {
        return raw.to_string();
    }
    let mut out = lines.join("\r\n");
    out.push_str("\r\n\r\n");
    out.push_str(body);
    out
}
#[cfg(test)]
mod tests {
    use super::*;

    fn temp_db(tag: &str) -> Arc<Db> {
        let path = std::env::temp_dir().join(format!(
            "wt-db-test-{}-{}.db",
            tag,
            std::process::id()
        ));
        let _ = std::fs::remove_file(&path);
        open(&path).expect("open temp db")
    }

    #[test]
    fn repeater_history_prunes_to_100_newest_first() {
        let db = temp_db("hist");
        let tab = db
            .save_repeater_tab("t", "GET / HTTP/1.1", None, None, None, 0)
            .unwrap();
        for i in 0..105 {
            db.insert_repeater_history(
                tab,
                &format!("GET /{i} HTTP/1.1"),
                None,
                Some(200),
                Some(i),
            )
            .unwrap();
        }
        let rows = db.list_repeater_history(tab, 100).unwrap();
        assert_eq!(rows.len(), 100);
        assert_eq!(rows[0].req_raw, "GET /104 HTTP/1.1");
        assert_eq!(rows[99].req_raw, "GET /5 HTTP/1.1");
    }

    #[test]
    fn repeater_history_cascades_on_tab_delete() {
        let db = temp_db("cascade");
        let tab = db
            .save_repeater_tab("t", "GET / HTTP/1.1", None, None, None, 0)
            .unwrap();
        db.insert_repeater_history(tab, "GET /a", None, Some(200), Some(1))
            .unwrap();
        assert_eq!(db.list_repeater_history(tab, 100).unwrap().len(), 1);
        db.delete_repeater_tab(tab).unwrap();
        assert!(db.list_repeater_history(tab, 100).unwrap().is_empty());
        assert!(db.list_repeater_tabs().unwrap().is_empty());
    }

    #[test]
    fn ws_frames_limit_keeps_newest_in_original_order() {
        let db = temp_db("wsframes");
        let conn = db
            .insert_ws_conn("wss://example.com/ws", "example.com", 443, true)
            .unwrap();
        for i in 0..5 {
            db.insert_ws_frame(conn, "out", 1, i, &format!("p{i}")).unwrap();
        }
        let rows = db.list_ws_frames(conn, 3).unwrap();
        assert_eq!(rows.len(), 3);
        assert_eq!(
            rows.iter().map(|f| f.payload.as_str()).collect::<Vec<_>>(),
            vec!["p2", "p3", "p4"]
        );
        assert!(db.list_ws_frames(conn, 500).unwrap().len() == 5);
    }

    #[test]
    fn intruder_campaign_crud_and_status_reset() {
        let db = temp_db("invcamp");
        let id = db
            .create_intruder_campaign(
                "login",
                "sniper",
                "POST /login HTTP/1.1\r\n\r\nuser=§admin§",
                Some("http://shop.test/login"),
            )
            .unwrap();
        let c = db.get_intruder_campaign(id).unwrap().expect("campaign");
        assert_eq!(c.name, "login");
        assert_eq!(c.attack_type, "sniper");
        assert_eq!(c.status, "draft");
        assert_eq!(c.threads, 4);
        assert!(!c.follow);

        let summaries = db.list_intruder_campaigns().unwrap();
        assert_eq!(summaries.len(), 1);
        assert_eq!(summaries[0].positions, 1);

        db.set_intruder_campaign_status(id, "running").unwrap();
        assert_eq!(
            db.get_intruder_campaign(id).unwrap().unwrap().status,
            "running"
        );
        // crash recovery
        assert_eq!(db.reset_intruder_running().unwrap(), 1);
        assert_eq!(
            db.get_intruder_campaign(id).unwrap().unwrap().status,
            "stopped"
        );

        let mut c = db.get_intruder_campaign(id).unwrap().unwrap();
        c.name = "login2".into();
        c.threads = 8;
        c.delay_ms = 50;
        c.set_map = "[0,1]".into();
        db.update_intruder_campaign(&c).unwrap();
        let c2 = db.get_intruder_campaign(id).unwrap().unwrap();
        assert_eq!(c2.name, "login2");
        assert_eq!(c2.threads, 8);
        assert_eq!(c2.set_map, "[0,1]");

        db.delete_intruder_campaign(id).unwrap();
        assert!(db.get_intruder_campaign(id).unwrap().is_none());
    }

    #[test]
    fn intruder_payload_sets_upsert_by_set_index() {
        let db = temp_db("invsets");
        let id = db
            .create_intruder_campaign("c", "sniper", "GET / HTTP/1.1\r\n\r\n", None)
            .unwrap();
        db.save_intruder_payload_set(id, 0, "users", "list", "alice\nbob", None, None, None, "[]")
            .unwrap();
        db.save_intruder_payload_set(id, 1, "ids", "numbers", "", Some(1), Some(5), Some(2), "[]")
            .unwrap();
        // upsert set 0 again
        db.save_intruder_payload_set(
            id,
            0,
            "users",
            "list",
            "alice\nbob\ncarol",
            None,
            None,
            None,
            r#"[{"kind":"md5"}]"#,
        )
        .unwrap();
        let sets = db.list_intruder_payload_sets(id).unwrap();
        assert_eq!(sets.len(), 2);
        assert_eq!(sets[0].items, "alice\nbob\ncarol");
        assert_eq!(sets[0].processors, r#"[{"kind":"md5"}]"#);
        assert_eq!(sets[1].start_i, Some(1));
        assert_eq!(sets[1].step_i, Some(2));
    }

    #[test]
    fn intruder_results_paging_count_and_cascade() {
        let db = temp_db("invres");
        let id = db
            .create_intruder_campaign("c", "sniper", "GET / HTTP/1.1\r\n\r\n", None)
            .unwrap();
        let rows: Vec<NewIntruderResult> = (0..7)
            .map(|i| NewIntruderResult {
                seq: i,
                combo: format!(r#"["p{i}"]"#),
                status: Some(200 + i),
                resp_len: 100 + i,
                time_ms: 10 + i,
                error: None,
                req_raw: format!("GET /p{i} HTTP/1.1\r\n\r\n"),
                resp_raw: Some(format!("HTTP/1.1 {} OK\r\n\r\n", 200 + i)),
                extracted: format!(r#"{{"tok":"t{i}"}}"#),
            })
            .collect();
        db.insert_intruder_results(id, &rows).unwrap();
        assert_eq!(db.count_intruder_results(id).unwrap(), 7);

        let page = db.list_intruder_results(id, 0, 3).unwrap();
        assert_eq!(
            page.iter().map(|r| r.seq).collect::<Vec<_>>(),
            vec![0, 1, 2]
        );
        let page2 = db.list_intruder_results(id, 3, 3).unwrap();
        assert_eq!(page2.iter().map(|r| r.seq).collect::<Vec<_>>(), vec![3, 4, 5]);

        let full = db.get_intruder_result(id, 4).unwrap().expect("row");
        assert_eq!(full.req_raw, "GET /p4 HTTP/1.1\r\n\r\n");
        assert!(full.resp_raw.unwrap().contains("204"));
        assert_eq!(full.extracted, r#"{"tok":"t4"}"#);
        assert!(db.get_intruder_result(id, 99).unwrap().is_none());

        assert_eq!(db.clear_intruder_results(id).unwrap(), 7);
        assert_eq!(db.count_intruder_results(id).unwrap(), 0);

        // cascade on campaign delete
        db.insert_intruder_results(id, &rows).unwrap();
        db.save_intruder_payload_set(id, 0, "s", "list", "a", None, None, None, "[]")
            .unwrap();
        db.delete_intruder_campaign(id).unwrap();
        assert_eq!(db.count_intruder_results(id).unwrap(), 0);
        assert!(db.list_intruder_payload_sets(id).unwrap().is_empty());
    }

    #[test]
    fn issue_insert_dedupes_and_returns_none_for_duplicate() {
        let db = temp_db("issdedupe");
        let mk = |param: &str| NewIssue {
            check_id: "missing-csp".into(),
            name: "Missing CSP".into(),
            severity: "info".into(),
            confidence: "certain".into(),
            host: "shop.test".into(),
            url: "https://shop.test/".into(),
            method: "GET".into(),
            path: "/".into(),
            param: param.into(),
            description: "desc".into(),
            remediation: "fix".into(),
            evidence: "no header".into(),
            exchange_id: None,
        };
        let id1 = db.insert_issue(&mk("")).unwrap();
        assert!(id1.is_some());
        // same check/host/path/param → deduped
        assert!(db.insert_issue(&mk("")).unwrap().is_none());
        // different param → new issue
        assert!(db.insert_issue(&mk("q")).unwrap().is_some());
        assert_eq!(db.issues_counts().unwrap(), vec![("info".to_string(), 2)]);
    }

    #[test]
    fn issue_list_filters_status_delete_and_counts() {
        let db = temp_db("isslist");
        let mk = |check: &str, sev: &str, host: &str, param: &str| NewIssue {
            check_id: check.into(),
            name: format!("{check} on {host}"),
            severity: sev.into(),
            confidence: "firm".into(),
            host: host.into(),
            url: format!("https://{host}/x"),
            method: "GET".into(),
            path: "/x".into(),
            param: param.into(),
            description: "d".into(),
            remediation: "r".into(),
            evidence: format!("ev-{check}"),
            exchange_id: Some(7),
        };
        let a = db
            .insert_issue(&mk("cookie-no-httponly", "medium", "a.test", ""))
            .unwrap()
            .unwrap();
        db.insert_issue(&mk("missing-csp", "info", "b.test", ""))
            .unwrap();
        db.insert_issue(&mk("sql-error-leak", "high", "a.test", "id"))
            .unwrap();

        // severity filter
        let high = vec!["high".to_string()];
        let rows = db.list_issues(&high, "", "", "", 0, 100).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].check_id, "sql-error-leak");
        assert_eq!(rows[0].exchange_id, Some(7));

        // host + text filter
        let rows = db.list_issues(&[], "a.test", "", "", 0, 100).unwrap();
        assert_eq!(rows.len(), 2);
        let rows = db.list_issues(&[], "", "", "httponly", 0, 100).unwrap();
        assert_eq!(rows.len(), 1);

        // status update + filter
        db.update_issue_status(a, "false-positive").unwrap();
        let open = db.list_issues(&[], "", "open", "", 0, 100).unwrap();
        assert_eq!(open.len(), 2);
        let fp = db.list_issues(&[], "", "false-positive", "", 0, 100).unwrap();
        assert_eq!(fp.len(), 1);
        assert_eq!(db.count_issues_open().unwrap(), 2);

        // paging newest first + get + delete
        let page = db.list_issues(&[], "", "", "", 0, 1).unwrap();
        assert_eq!(page.len(), 1);
        let got = db.get_issue(page[0].id).unwrap().expect("issue");
        assert_eq!(got.evidence, format!("ev-{}", got.check_id));
        assert!(db.get_issue(9999).unwrap().is_none());
        assert_eq!(db.delete_issue(page[0].id).unwrap(), 1);
    }

    #[test]
    fn scan_task_insert_list_update_reset() {
        let db = temp_db("scan_task");
        let id = db
            .insert_scan_task("crawl-audit", "https://app.test/", None)
            .unwrap();
        assert!(id > 0);

        let tasks = db.list_scan_tasks().unwrap();
        assert_eq!(tasks.len(), 1);
        assert_eq!(tasks[0].id, id);
        assert_eq!(tasks[0].kind, "crawl-audit");
        assert_eq!(tasks[0].status, "running");
        assert_eq!(tasks[0].pages_crawled, 0);

        db.update_scan_task(id, "running", 12, 34, 3, None).unwrap();
        let t = db.get_scan_task(id).unwrap().expect("task");
        assert_eq!(t.pages_crawled, 12);
        assert_eq!(t.requests_sent, 34);
        assert_eq!(t.issues_found, 3);

        // crash recovery: running/paused → cancelled
        db.insert_scan_task("crawl", "https://b.test/", None).unwrap();
        db.reset_scan_tasks_running().unwrap();
        let all = db.list_scan_tasks().unwrap();
        assert!(all.iter().all(|t| t.status == "cancelled"), "{all:?}");
    }

    #[test]
    fn scan_config_seed_save_delete() {
        let db = temp_db("scan_config");
        // default "Crawl + audit" seeded on first open
        let configs = db.list_scan_configs().unwrap();
        assert_eq!(configs.len(), 1);
        assert_eq!(configs[0].name, "Crawl + audit");
        assert!(configs[0].is_default);
        assert!(configs[0].passive_enabled);
        assert!(configs[0].active_checks.is_empty());
        assert_eq!(configs[0].crawl_max_pages, 50);

        // insert new, update it, verify
        let id = db
            .save_scan_config(
                None,
                "Audit only",
                false,
                &["sqli-error".into(), "xss-reflected".into()],
                10,
                2,
                2,
                100,
                false,
            )
            .unwrap();
        let c = db.get_scan_config(id).unwrap().expect("config");
        assert_eq!(c.name, "Audit only");
        assert!(!c.passive_enabled);
        assert_eq!(c.active_checks, vec!["sqli-error", "xss-reflected"]);
        assert_eq!(c.crawl_max_pages, 10);

        db.save_scan_config(
            Some(id),
            "Audit v2",
            true,
            &[],
            20,
            3,
            4,
            50,
            true,
        )
        .unwrap();
        let c2 = db.get_scan_config(id).unwrap().expect("config");
        assert_eq!(c2.name, "Audit v2");
        assert!(c2.is_default);
        // is_default moved: old default cleared
        let all = db.list_scan_configs().unwrap();
        let defaults: Vec<_> = all.iter().filter(|c| c.is_default).collect();
        assert_eq!(defaults.len(), 1);
        assert_eq!(defaults[0].id, id);

        assert_eq!(db.delete_scan_config(id).unwrap(), 1);
        assert!(db.get_scan_config(id).unwrap().is_none());
        assert_eq!(db.list_scan_configs().unwrap().len(), 1); // seed remains
    }

    #[test]
    fn extension_crud() {
        let db = temp_db("ext");
        let id = db.insert_extension("My ext", "C:/exts/my.js").unwrap();
        let list = db.list_extensions().unwrap();
        assert_eq!(list.len(), 1);
        assert!(list[0].enabled);
        assert_eq!(list[0].name, "My ext");

        db.set_extension_error(id, Some("boom")).unwrap();
        let e = db.get_extension(id).unwrap().expect("ext");
        assert_eq!(e.error.as_deref(), Some("boom"));
        assert!(e.loaded_at.is_some());

        db.set_extension_enabled(id, false).unwrap();
        assert!(!db.get_extension(id).unwrap().expect("ext").enabled);

        assert_eq!(db.delete_extension(id).unwrap(), 1);
        assert!(db.list_extensions().unwrap().is_empty());
    }
}
