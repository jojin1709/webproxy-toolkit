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
}
