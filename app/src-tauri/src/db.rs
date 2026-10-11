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
        conn.execute("DELETE FROM repeater_tabs WHERE id = ?1", params![id])
            .map_err(|e| e.to_string())?;
        Ok(())
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

    pub fn list_ws_frames(&self, conn_id: i64) -> Result<Vec<WsFrame>, String> {
        let conn = self.conn.lock();
        let mut stmt = conn
            .prepare(
                "SELECT id, conn_id, ts, dir, opcode, len, payload
                 FROM ws_frames WHERE conn_id = ?1 ORDER BY id",
            )
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map(params![conn_id], |r| {
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
        rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())
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