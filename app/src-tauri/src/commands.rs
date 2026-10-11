use crate::proxy::{send_raw_request, HttpExchange, SharedProxy};
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;
use tauri::{AppHandle, Emitter, State};

fn log_event(app: &AppHandle, state: &SharedProxy, tool: &str, level: &str, message: &str) {
    if let Ok(db) = db_of(state) {
        if let Ok(entry) = db.insert_log(tool, level, message) {
            let _ = app.emit("log-added", entry);
        }
    }
}

#[tauri::command]
pub async fn proxy_start(
    app: AppHandle,
    state: State<'_, SharedProxy>,
    host: String,
    port: u16,
) -> Result<u16, String> {
    let st = state.inner().clone();
    match crate::proxy::start_proxy(st, &host, port).await {
        Ok(()) => {}
        Err(e) => {
            log_event(&app, &state, "proxy", "error", &format!("Listener start failed on {host}:{port}: {e}"));
            return Err(e);
        }
    }
    if let Some(db) = state.db.read().clone() {
        let _ = db.set_meta("listen_port", &port.to_string());
    }
    let actual = state.port.load(Ordering::SeqCst);
    log_event(&app, &state, "proxy", "info", &format!("Listener started on {host}:{actual}"));
    let _ = app.emit("proxy-status", proxy_status_payload(&state));
    Ok(actual)
}

#[tauri::command]
pub fn proxy_stop(app: AppHandle, state: State<'_, SharedProxy>) -> Result<(), String> {
    crate::proxy::stop_proxy(&state);
    log_event(&app, &state, "proxy", "info", "Listener stopped");
    let _ = app.emit("proxy-status", proxy_status_payload(&state));
    Ok(())
}

#[derive(serde::Serialize, Clone)]
pub struct ProxyStatus {
    pub running: bool,
    pub port: u16,
    pub intercept: bool,
    pub intercept_resp: bool,
    pub held_count: usize,
    pub history_count: usize,
}

fn proxy_status_payload(state: &SharedProxy) -> ProxyStatus {
    ProxyStatus {
        running: state.running.load(Ordering::SeqCst),
        port: state.port.load(Ordering::SeqCst),
        intercept: state.intercept.load(Ordering::SeqCst),
        intercept_resp: state.intercept_resp.load(Ordering::SeqCst),
        held_count: state.held.lock().len(),
        history_count: state.history.lock().len(),
    }
}

#[tauri::command]
pub fn proxy_status(state: State<'_, SharedProxy>) -> ProxyStatus {
    proxy_status_payload(&state)
}

#[tauri::command]
pub fn intercept_set(
    app: AppHandle,
    state: State<'_, SharedProxy>,
    enabled: bool,
    response: Option<bool>,
) -> ProxyStatus {
    state.intercept.store(enabled, Ordering::SeqCst);
    if let Some(r) = response {
        state.intercept_resp.store(r, Ordering::SeqCst);
    }
    if let Some(db) = state.db.read().clone() {
        let _ = db.set_meta("intercept", if enabled { "1" } else { "0" });
        if response.is_some() {
            let _ = db.set_meta(
                "intercept_resp",
                if state.intercept_resp.load(Ordering::SeqCst) { "1" } else { "0" },
            );
        }
    }
    let s = proxy_status_payload(&state);
    let mode = if s.intercept_resp { "requests and responses" } else { "requests" };
    log_event(
        &app,
        &state,
        "proxy",
        "info",
        &format!("Intercept turned {} ({})", if enabled { "on" } else { "off" }, mode),
    );
    let _ = app.emit("proxy-status", s.clone());
    s
}

#[derive(serde::Serialize, Clone)]
pub struct HeldInfo {
    pub id: u64,
    pub raw_request: String,
    pub host: String,
    pub port: u16,
    pub tls: bool,
    pub direction: String,
}

#[tauri::command]
pub fn intercept_peek(state: State<'_, SharedProxy>) -> Option<HeldInfo> {
    let held = state.held.lock();
    held.iter().next().map(|(id, h)| HeldInfo {
        id: *id,
        raw_request: h.raw_request.clone(),
        host: h.host.clone(),
        port: h.port,
        tls: h.tls,
        direction: h.direction.clone(),
    })
}

#[tauri::command]
pub fn intercept_forward(
    app: AppHandle,
    state: State<'_, SharedProxy>,
    id: u64,
    raw: Option<String>,
) -> Result<(), String> {
    let item = state.held.lock().remove(&id).ok_or("no held request")?;
    let _ = item.pending.send(raw.or(Some(item.raw_request.clone())));
    log_event(&app, &state, "proxy", "info", &format!("Held request {id} forwarded"));
    let _ = app.emit("proxy-status", proxy_status_payload(&state));
    Ok(())
}

#[tauri::command]
pub fn intercept_drop(
    app: AppHandle,
    state: State<'_, SharedProxy>,
    id: u64,
) -> Result<(), String> {
    let item = state.held.lock().remove(&id).ok_or("no held request")?;
    let _ = item.pending.send(None);
    log_event(&app, &state, "proxy", "warn", &format!("Held request {id} dropped"));
    let _ = app.emit("proxy-status", proxy_status_payload(&state));
    Ok(())
}

#[derive(serde::Deserialize)]
pub struct HistoryFilter {
    pub query: Option<String>,
    pub method: Option<String>,
    pub limit: Option<usize>,
}

#[tauri::command]
pub fn history_list(state: State<'_, SharedProxy>, filter: Option<HistoryFilter>) -> Vec<HttpExchange> {
    let hist = state.history.lock();
    let filter = filter.unwrap_or(HistoryFilter {
        query: None,
        method: None,
        limit: Some(500),
    });
    let q = filter.query.unwrap_or_default().to_lowercase();
    let m = filter.method.unwrap_or_default();
    let limit = filter.limit.unwrap_or(500);
    hist.iter()
        .rev()
        .filter(|e| {
            if !m.is_empty() && !e.method.eq_ignore_ascii_case(&m) {
                return false;
            }
            if q.is_empty() {
                return true;
            }
            e.url.to_lowercase().contains(&q)
                || e.host.to_lowercase().contains(&q)
                || e.method.to_lowercase().contains(&q)
        })
        .take(limit)
        .cloned()
        .collect()
}

#[tauri::command]
pub fn history_get(state: State<'_, SharedProxy>, id: u64) -> Option<HttpExchange> {
    state.history.lock().iter().find(|e| e.id == id).cloned()
}

fn parse_host_port(url: &str, default_tls: bool) -> Result<(String, u16, bool), String> {
    let u = url::Url::parse(url).map_err(|e| e.to_string())?;
    let host = u.host_str().ok_or("no host")?.to_string();
    let tls = match u.scheme() {
        "https" => true,
        "http" => false,
        _ => default_tls,
    };
    let port = u.port().unwrap_or(if tls { 443 } else { 80 });
    Ok((host, port, tls))
}

#[tauri::command]
pub async fn repeater_send(
    app: AppHandle,
    state: State<'_, SharedProxy>,
    raw: String,
    url_hint: Option<String>,
) -> Result<serde_json::Value, String> {
    let (host, port, tls) = if let Some(u) = url_hint.clone() {
        parse_host_port(&u, false)?
    } else {
        let first = raw
            .split("\r\n")
            .find_map(|l| {
                let mut it = l.split_whitespace();
                let _method = it.next()?;
                let target = it.next()?;
                if target.starts_with("http://") || target.starts_with("https://") {
                    parse_host_port(target, false).ok()
                } else {
                    None
                }
            })
            .ok_or("cannot determine host — use absolute URL in request line")?;
        first
    };
    let target = url_hint
        .clone()
        .unwrap_or_else(|| format!("http{}://{host}:{port}", if tls { "s" } else { "" }));
    match send_raw_request(raw, host, port, tls).await {
        Ok((status, headers, body, time_ms)) => {
            log_event(
                &app,
                &state,
                "repeater",
                "info",
                &format!("Request sent to {target} — status {status}, {time_ms} ms"),
            );
            Ok(serde_json::json!({
                "status": status,
                "headers": headers,
                "body": body,
                "time_ms": time_ms,
            }))
        }
        Err(e) => {
            log_event(&app, &state, "repeater", "error", &format!("Request to {target} failed: {e}"));
            Err(e)
        }
    }
}

#[tauri::command]
pub fn ca_export_path() -> Result<String, String> {
    let dir = app_data_dir().map_err(|e| e.to_string())?;
    Ok(crate::ca::ca_export_path(&dir).to_string_lossy().to_string())
}

#[tauri::command]
pub fn sitemap_tree(state: State<'_, SharedProxy>) -> serde_json::Value {
    let hist = state.history.lock();
    let mut hosts: std::collections::BTreeMap<String, std::collections::BTreeMap<String, usize>> =
        std::collections::BTreeMap::new();
    for e in hist.iter() {
        let path = url::Url::parse(&e.url)
            .ok()
            .map(|u| u.path().to_string())
            .unwrap_or_else(|| "/".into());
        *hosts.entry(e.host.clone()).or_default().entry(path).or_default() += 1;
    }
    let tree: Vec<serde_json::Value> = hosts
        .into_iter()
        .map(|(host, paths)| {
            let children: Vec<serde_json::Value> = paths
                .into_iter()
                .map(|(p, n)| serde_json::json!({ "path": p, "count": n }))
                .collect();
            serde_json::json!({ "host": host, "children": children })
        })
        .collect();
    serde_json::json!({ "roots": tree })
}

fn app_data_dir() -> Result<std::path::PathBuf, String> {
    let base = std::env::var("LOCALAPPDATA")
        .or_else(|_| std::env::var("APPDATA"))
        .map_err(|_| "no app data dir")?;
    Ok(std::path::PathBuf::from(base).join("WebProxyToolkit"))
}

fn db_of(state: &SharedProxy) -> Result<Arc<crate::db::Db>, String> {
    state
        .db
        .read()
        .clone()
        .ok_or_else(|| "project database not open".to_string())
}

fn refresh_scope(state: &SharedProxy) {
    if let Ok(rules) = db_of(state).and_then(|db| db.list_scope()) {
        *state.scope.lock() = rules;
    }
}

fn refresh_match(state: &SharedProxy) {
    if let Ok(rules) = db_of(state).and_then(|db| db.list_match_replace()) {
        *state.match_replace.lock() = rules;
    }
}

#[tauri::command]
pub fn scope_list(state: State<'_, SharedProxy>) -> Result<Vec<crate::db::ScopeRule>, String> {
    db_of(&state)?.list_scope()
}

#[tauri::command]
pub fn scope_add(
    app: AppHandle,
    state: State<'_, SharedProxy>,
    mode: String,
    kind: String,
    pattern: String,
) -> Result<i64, String> {
    let id = db_of(&state)?.insert_scope(&mode, &kind, &pattern)?;
    refresh_scope(&state);
    log_event(
        &app,
        &state,
        "scope",
        "info",
        &format!("Scope rule added ({mode} {kind}) {pattern}"),
    );
    Ok(id)
}

#[tauri::command]
pub fn scope_delete(app: AppHandle, state: State<'_, SharedProxy>, id: i64) -> Result<(), String> {
    db_of(&state)?.delete_scope(id)?;
    refresh_scope(&state);
    log_event(&app, &state, "scope", "warn", &format!("Scope rule {id} deleted"));
    Ok(())
}

#[tauri::command]
pub fn match_list(state: State<'_, SharedProxy>) -> Result<Vec<crate::db::MatchRule>, String> {
    db_of(&state)?.list_match_replace()
}

#[tauri::command]
pub fn match_add(
    app: AppHandle,
    state: State<'_, SharedProxy>,
    phase: String,
    match_kind: String,
    match_val: String,
    replace_val: String,
    comment: Option<String>,
) -> Result<i64, String> {
    let id = db_of(&state)?.insert_match_replace(
        &phase,
        &match_kind,
        &match_val,
        &replace_val,
        comment.as_deref(),
    )?;
    refresh_match(&state);
    log_event(
        &app,
        &state,
        "match",
        "info",
        &format!("Match/replace rule added for {phase} phase ({match_kind})"),
    );
    Ok(id)
}

#[tauri::command]
pub fn match_delete(app: AppHandle, state: State<'_, SharedProxy>, id: i64) -> Result<(), String> {
    db_of(&state)?.delete_match_replace(id)?;
    refresh_match(&state);
    log_event(&app, &state, "match", "warn", &format!("Match/replace rule {id} deleted"));
    Ok(())
}

#[tauri::command]
pub fn settings_get(
    state: State<'_, SharedProxy>,
    key: String,
) -> Result<Option<String>, String> {
    db_of(&state)?.get_meta(&key)
}

#[tauri::command]
pub fn settings_set(
    app: AppHandle,
    state: State<'_, SharedProxy>,
    key: String,
    value: String,
) -> Result<(), String> {
    db_of(&state)?.set_meta(&key, &value)?;
    log_event(&app, &state, "settings", "info", &format!("Setting {key} updated"));
    Ok(())
}

#[tauri::command]
pub fn repeater_tabs_list(state: State<'_, SharedProxy>) -> Result<Vec<serde_json::Value>, String> {
    db_of(&state)?.list_repeater_tabs()
}

#[tauri::command]
pub fn repeater_tab_save(
    app: AppHandle,
    state: State<'_, SharedProxy>,
    id: Option<i64>,
    name: String,
    req_raw: String,
    last_status: Option<i64>,
    last_time_ms: Option<i64>,
    last_resp_raw: Option<String>,
) -> Result<i64, String> {
    let db = db_of(&state)?;
    let saved = match id {
        Some(id) => {
            db.update_repeater_tab(id, &name, &req_raw, last_status, last_time_ms, last_resp_raw.as_deref())?;
            id
        }
        None => {
            let order = db.list_repeater_tabs().map(|t| t.len() as i64).unwrap_or(0);
            db.save_repeater_tab(
                &name,
                &req_raw,
                last_status,
                last_time_ms,
                last_resp_raw.as_deref(),
                order,
            )?
        }
    };
    log_event(
        &app,
        &state,
        "repeater",
        "info",
        &format!("Tab \"{name}\" saved (#{saved})"),
    );
    Ok(saved)
}

#[tauri::command]
pub fn repeater_tab_delete(
    app: AppHandle,
    state: State<'_, SharedProxy>,
    id: i64,
) -> Result<(), String> {
    db_of(&state)?.delete_repeater_tab(id)?;
    log_event(&app, &state, "repeater", "warn", &format!("Tab #{id} deleted"));
    Ok(())
}

#[tauri::command]
pub fn init_ca(state: State<'_, SharedProxy>) -> Result<String, String> {
    let dir = app_data_dir()?;
    let ca = crate::ca::load_or_create(&dir)?;
    let path = crate::ca::ca_export_path(&dir);
    *state.ca.write() = Some(ca);
    Ok(path.to_string_lossy().to_string())
}

#[tauri::command]
pub fn hash_compute(alg: String, data: String) -> Result<String, String> {
    use md5::Digest as _;
    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|b| format!("{b:02x}")).collect()
    }
    match alg.as_str() {
        "md5" => {
            use md5::Md5;
            Ok(hex(&Md5::digest(data.as_bytes())))
        }
        "sha1" => {
            use sha1::Sha1;
            use sha1::Digest as _;
            Ok(hex(&Sha1::digest(data.as_bytes())))
        }
        "sha256" => {
            use sha2::Sha256;
            use sha2::Digest as _;
            Ok(hex(&Sha256::digest(data.as_bytes())))
        }
        "sha512" => {
            use sha2::Sha512;
            use sha2::Digest as _;
            Ok(hex(&Sha512::digest(data.as_bytes())))
        }
        other => Err(format!("unknown hash algorithm: {other}")),
    }
}

#[derive(Default, serde::Deserialize)]
#[serde(default)]
pub struct LogFilter {
    pub tool: Option<String>,
    pub level: Option<String>,
    pub query: Option<String>,
    pub limit: Option<usize>,
}

#[tauri::command]
pub fn log_add(
    app: AppHandle,
    state: State<'_, SharedProxy>,
    tool: String,
    level: String,
    message: String,
) -> Result<crate::db::LogEntry, String> {
    let entry = db_of(&state)?.insert_log(&tool, &level, &message)?;
    let _ = app.emit("log-added", entry.clone());
    Ok(entry)
}

#[tauri::command]
pub fn log_list(
    state: State<'_, SharedProxy>,
    filter: Option<LogFilter>,
) -> Result<Vec<crate::db::LogEntry>, String> {
    let f = filter.unwrap_or_default();
    db_of(&state)?.list_logs(
        f.tool.as_deref(),
        f.level.as_deref(),
        f.query.as_deref(),
        f.limit.unwrap_or(1000).min(5000),
    )
}

#[tauri::command]
pub fn log_clear(app: AppHandle, state: State<'_, SharedProxy>) -> Result<usize, String> {
    let n = db_of(&state)?.clear_logs()?;
    log_event(&app, &state, "system", "info", &format!("Log cleared ({n} entries removed)"));
    Ok(n)
}

fn sequencer_loc(
    u: &url::Url,
) -> Result<(String, u16, bool, String), String> {
    let scheme = u.scheme();
    if scheme != "http" && scheme != "https" {
        return Err(format!("unsupported scheme: {scheme} (use http or https)"));
    }
    let tls = scheme == "https";
    let host = u.host_str().ok_or("URL has no host")?.to_string();
    let port = u.port().unwrap_or(if tls { 443 } else { 80 });
    let path = if u.query().is_some() {
        format!("{}?{}", u.path(), u.query().unwrap())
    } else {
        u.path().to_string()
    };
    let path = if path.is_empty() { "/".to_string() } else { path };
    Ok((host, port, tls, path))
}

async fn sequencer_once(
    base: &url::Url,
    source: &str,
    pattern: &str,
) -> Result<String, String> {
    let mut current = base.clone();
    for _ in 0..5 {
        let (host, port, tls, path) = sequencer_loc(&current)?;
        let authority = match (tls, port) {
            (true, 443) | (false, 80) => host.clone(),
            _ => format!("{host}:{port}"),
        };
        let raw = format!(
            "GET {path} HTTP/1.1\r\nHost: {authority}\r\nUser-Agent: webproxy-toolkit/0.1\r\nAccept: */*\r\nConnection: close\r\n\r\n"
        );
        let (status, headers, body, _ms) = send_raw_request(raw, host, port, tls).await?;
        if matches!(status, 301 | 302 | 303 | 307 | 308) {
            if let Some(loc) = headers
                .iter()
                .find(|(k, _)| k.eq_ignore_ascii_case("location"))
            {
                if let Ok(next) = current.join(&loc.1) {
                    if matches!(next.scheme(), "http" | "https") {
                        current = next;
                        continue;
                    }
                }
            }
        }
        return match source {
            "cookie" => headers
                .iter()
                .filter(|(k, _)| k.eq_ignore_ascii_case("set-cookie"))
                .find_map(|(_, v)| {
                    let (name, rest) = v.split_once('=')?;
                    if name.trim() == pattern {
                        rest.split(';').next().map(|s| s.trim().to_string())
                    } else {
                        None
                    }
                })
                .ok_or_else(|| format!("no cookie named \"{pattern}\" in response")),
            "header" => headers
                .iter()
                .find(|(k, _)| k.eq_ignore_ascii_case(pattern))
                .map(|(_, v)| v.trim().to_string())
                .ok_or_else(|| format!("no header \"{pattern}\" in response")),
            _ => {
                let re =
                    regex::Regex::new(pattern).map_err(|e| format!("invalid regex: {e}"))?;
                let hay = body.as_str();
                let found = re.captures(hay).map(|c| {
                    c.get(1).or_else(|| c.get(0)).map(|m| m.as_str().to_string())
                });
                match found.flatten() {
                    Some(s) => Ok(s),
                    None => {
                        let header_blob = headers
                            .iter()
                            .map(|(k, v)| format!("{k}: {v}"))
                            .collect::<Vec<_>>()
                            .join("\r\n");
                        re.captures(&header_blob)
                            .and_then(|c| {
                                c.get(1).or_else(|| c.get(0)).map(|m| m.as_str().to_string())
                            })
                            .ok_or_else(|| format!("pattern did not match response"))
                    }
                }
            }
        };
    }
    Err("too many redirects".to_string())
}

#[tauri::command]
pub async fn sequencer_sample(
    app: AppHandle,
    state: State<'_, SharedProxy>,
    url: String,
    source: String,
    pattern: Option<String>,
    count: usize,
) -> Result<Vec<String>, String> {
    if !matches!(source.as_str(), "cookie" | "header" | "regex") {
        return Err(format!("unknown source: {source}"));
    }
    let pattern = pattern.unwrap_or_default();
    if pattern.is_empty() {
        return Err("sample pattern must not be empty".to_string());
    }
    if source == "regex" {
        regex::Regex::new(&pattern).map_err(|e| format!("invalid regex: {e}"))?;
    }
    let base = url::Url::parse(&url).map_err(|e| e.to_string())?;
    sequencer_loc(&base)?;
    let total = count.clamp(1, 500);
    let mut tokens: Vec<String> = Vec::with_capacity(total);
    let mut failed = 0usize;
    for _ in 0..total {
        match tokio::time::timeout(
            Duration::from_secs(10),
            sequencer_once(&base, &source, &pattern),
        )
        .await
        {
            Ok(Ok(tok)) => tokens.push(tok),
            _ => failed += 1,
        }
    }
    if tokens.is_empty() {
        let msg = format!(
            "Sequencer: all {total} attempts against {url} failed — no samples collected"
        );
        log_event(&app, &state, "sequencer", "error", &msg);
        return Err(msg);
    }
    if failed > 0 {
        log_event(
            &app,
            &state,
            "sequencer",
            "warn",
            &format!(
                "Sequencer: collected {} samples from {url} ({failed} of {total} attempts failed)",
                tokens.len()
            ),
        );
    } else {
        log_event(
            &app,
            &state,
            "sequencer",
            "info",
            &format!("Sequencer: collected {} samples from {url}", tokens.len()),
        );
    }
    Ok(tokens)
}

pub fn register(
    builder: tauri::Builder<tauri::Wry>,
    state: SharedProxy,
) -> tauri::Builder<tauri::Wry> {
    builder
        .manage(state)
        .setup(|app| {
            use tauri::Manager;
            let handle = app.handle().clone();
            let st = handle.state::<SharedProxy>();
            if let Ok(dir) = app_data_dir() {
                match crate::ca::load_or_create(&dir) {
                    Ok(ca) => *st.ca.write() = Some(ca),
                    Err(e) => eprintln!("ca init failed: {e}"),
                }
            }
            if let Ok(p) = std::env::var("WEBPROXY_AUTOSTART") {
                let port: u16 = p.parse().unwrap_or(0);
                let st2 = st.inner().clone();
                tauri::async_runtime::spawn(async move {
                    if let Err(e) = crate::proxy::start_proxy(st2, "127.0.0.1", port).await {
                        eprintln!("proxy autostart failed: {e}");
                    }
                });
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            proxy_start,
            proxy_stop,
            proxy_status,
            intercept_set,
            intercept_peek,
            intercept_forward,
            intercept_drop,
            history_list,
            history_get,
            repeater_send,
            ca_export_path,
            init_ca,
            sitemap_tree,
            scope_list,
            scope_add,
            scope_delete,
            match_list,
            match_add,
            match_delete,
            settings_get,
            settings_set,
            repeater_tabs_list,
            repeater_tab_save,
            repeater_tab_delete,
            hash_compute,
            log_add,
            log_list,
            log_clear,
            sequencer_sample,
        ])
}
