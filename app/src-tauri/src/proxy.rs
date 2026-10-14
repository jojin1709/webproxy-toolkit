use crate::ca::{mint_leaf, Ca};
use crate::db::{self, Db, MatchRule, ScopeRule};
use chrono::Utc;
use parking_lot::Mutex;
use rustls::pki_types::{PrivateKeyDer, PrivatePkcs8KeyDer};
use rustls::ClientConfig as TlsClientConfig;
use rustls::ServerConfig;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU16, AtomicU64, Ordering};
use std::sync::Arc;
use tauri::{AppHandle, Emitter};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio_rustls::TlsAcceptor;
use tokio_rustls::TlsConnector;

#[derive(Clone, Debug, serde::Serialize)]
pub struct HttpExchange {
    pub id: u64,
    pub started_at: String,
    pub time_ms: Option<u64>,
    pub client_addr: Option<String>,
    pub host: String,
    pub method: String,
    pub url: String,
    pub req_headers: Vec<(String, String)>,
    pub req_body: Option<String>,
    pub status: Option<u16>,
    pub resp_headers: Vec<(String, String)>,
    pub resp_body: Option<String>,
    pub tls: bool,
    pub websocket: bool,
    pub in_scope: bool,
}

pub struct HeldRequest {
    pub raw_request: String,
    pub host: String,
    pub port: u16,
    pub tls: bool,
    pub direction: String,
    pub pending: tokio::sync::oneshot::Sender<Option<String>>,
}

pub struct ProxyState {
    pub running: AtomicBool,
    pub port: AtomicU16,
    pub intercept: AtomicBool,
    pub intercept_resp: AtomicBool,
    pub next_id: AtomicU64,
    pub held: Mutex<HashMap<u64, HeldRequest>>,
    pub history: Mutex<Vec<HttpExchange>>,
    pub ca: parking_lot::RwLock<Option<Arc<Ca>>>,
    pub listener_shutdown: Mutex<Option<tokio::sync::watch::Sender<bool>>>,
    pub db: parking_lot::RwLock<Option<Arc<Db>>>,
    pub scope: Mutex<Vec<ScopeRule>>,
    pub match_replace: Mutex<Vec<MatchRule>>,
    pub app_handle: parking_lot::RwLock<Option<AppHandle>>,
    pub intruder_runs: crate::intruder::IntruderRuns,
    pub scan_runs: crate::scanner::ScanRuns,
    pub extensions: Arc<crate::extensions::ExtRuntime>,
    pub collab: crate::collaborator::CollabState,
}

impl Default for ProxyState {
    fn default() -> Self {
        Self {
            running: AtomicBool::new(false),
            port: AtomicU16::new(8080),
            intercept: AtomicBool::new(false),
            intercept_resp: AtomicBool::new(false),
            next_id: AtomicU64::new(1),
            held: Mutex::new(HashMap::new()),
            history: Mutex::new(Vec::new()),
            ca: parking_lot::RwLock::new(None),
            listener_shutdown: Mutex::new(None),
            db: parking_lot::RwLock::new(None),
            scope: Mutex::new(Vec::new()),
            match_replace: Mutex::new(Vec::new()),
            app_handle: parking_lot::RwLock::new(None),
            intruder_runs: crate::intruder::IntruderRuns::default(),
            scan_runs: crate::scanner::ScanRuns::default(),
            extensions: Arc::new(crate::extensions::ExtRuntime::new()),
            collab: crate::collaborator::CollabState::default(),
        }
    }
}

pub fn hydrate(state: &SharedProxy, database: &Arc<Db>) {
    *state.db.write() = Some(database.clone());
    // crash recovery: attacks cannot survive a restart
    if let Err(e) = database.reset_intruder_running() {
        eprintln!("intruder status reset failed: {e}");
    }
    // crash recovery: running scan tasks cannot survive a restart
    if let Err(e) = database.reset_scan_tasks_running() {
        eprintln!("scan task status reset failed: {e}");
    }
    if let Ok(rules) = database.list_scope() {
        *state.scope.lock() = rules;
    }
    if let Ok(rules) = database.list_match_replace() {
        *state.match_replace.lock() = rules;
    }
    if let Ok(mut rows) = database.load_exchanges(5000) {
        if let Ok(max_id) = database.max_exchange_id() {
            state.next_id.store(max_id + 1, Ordering::SeqCst);
        }
        state.history.lock().append(&mut rows);
    }
    if let Ok(Some(p)) = database.get_meta("listen_port") {
        if let Ok(v) = p.parse::<u16>() {
            state.port.store(v, Ordering::SeqCst);
        }
    }
    if let Ok(Some(v)) = database.get_meta("intercept") {
        state.intercept.store(v == "1", Ordering::SeqCst);
    }
    if let Ok(Some(v)) = database.get_meta("intercept_resp") {
        state.intercept_resp.store(v == "1", Ordering::SeqCst);
    }
}

pub type SharedProxy = Arc<ProxyState>;

fn split_message(raw: &str) -> Option<(String, Vec<(String, String)>, Vec<u8>)> {
    let idx = raw.find("\r\n\r\n")?;
    let head = &raw[..idx];
    let body = raw[idx + 4..].as_bytes().to_vec();
    let mut lines = head.split("\r\n");
    let request_line = lines.next()?.to_string();
    let mut headers = Vec::new();
    for line in lines {
        if let Some((k, v)) = line.split_once(':') {
            headers.push((k.trim().to_string(), v.trim().to_string()));
        }
    }
    Some((request_line, headers, body))
}

fn split_response_text(raw: &str) -> Option<(u16, Vec<(String, String)>, String)> {
    let idx = raw.find("\r\n\r\n")?;
    let head = &raw[..idx];
    let body = raw[idx + 4..].to_string();
    let mut lines = head.split("\r\n");
    let status_line = lines.next()?;
    let status: u16 = status_line.split_whitespace().nth(1)?.parse().ok()?;
    let mut headers = Vec::new();
    for l in lines {
        if let Some((k, v)) = l.split_once(':') {
            headers.push((k.trim().to_string(), v.trim().to_string()));
        }
    }
    Some((status, headers, body))
}

fn header_get(headers: &[(String, String)], name: &str) -> Option<String> {
    headers
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case(name))
        .map(|(_, v)| v.clone())
}

fn rebuild_request(request_line: &str, headers: &[(String, String)], body: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(request_line.as_bytes());
    out.extend_from_slice(b"\r\n");
    for (k, v) in headers {
        out.extend_from_slice(k.as_bytes());
        out.extend_from_slice(b": ");
        out.extend_from_slice(v.as_bytes());
        out.extend_from_slice(b"\r\n");
    }
    out.extend_from_slice(b"\r\n");
    out.extend_from_slice(body);
    out
}

fn rebuild_response(status: u16, headers: &[(String, String)], body: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    let reason = match status {
        200 => "OK",
        204 => "No Content",
        301 => "Moved Permanently",
        302 => "Found",
        304 => "Not Modified",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        500 => "Internal Server Error",
        502 => "Bad Gateway",
        503 => "Service Unavailable",
        _ => "",
    };
    out.extend_from_slice(format!("HTTP/1.1 {} {}\r\n", status, reason).as_bytes());
    for (k, v) in headers {
        if k.eq_ignore_ascii_case("content-length") || k.eq_ignore_ascii_case("transfer-encoding") {
            continue;
        }
        out.extend_from_slice(k.as_bytes());
        out.extend_from_slice(b": ");
        out.extend_from_slice(v.as_bytes());
        out.extend_from_slice(b"\r\n");
    }
    out.extend_from_slice(format!("Content-Length: {}\r\n", body.len()).as_bytes());
    out.extend_from_slice(b"\r\n");
    out.extend_from_slice(body);
    out
}

fn root_store() -> rustls::RootCertStore {
    let mut root = rustls::RootCertStore::empty();
    root.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    root
}

fn upstream_client_config() -> Result<TlsClientConfig, String> {
    Ok(TlsClientConfig::builder()
        .with_root_certificates(root_store())
        .with_no_client_auth())
}

enum Upstream {
    Plain(TcpStream),
    Tls(Box<tokio_rustls::client::TlsStream<TcpStream>>),
}

impl Upstream {
    async fn write_all(&mut self, data: &[u8]) -> std::io::Result<()> {
        match self {
            Upstream::Plain(s) => s.write_all(data).await,
            Upstream::Tls(s) => s.write_all(data).await,
        }
    }
    async fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        match self {
            Upstream::Plain(s) => s.read(buf).await,
            Upstream::Tls(s) => s.read(buf).await,
        }
    }
}

async fn read_until_headers(stream: &mut TcpStream, max: usize) -> std::io::Result<Vec<u8>> {
    let mut buf = Vec::new();
    let mut tmp = [0u8; 4096];
    loop {
        let n = stream.read(&mut tmp).await?;
        if n == 0 {
            break;
        }
        buf.extend_from_slice(&tmp[..n]);
        if buf.windows(4).any(|w| w == b"\r\n\r\n") || buf.len() >= max {
            break;
        }
    }
    Ok(buf)
}

fn find_chunk_end(buf: &[u8]) -> Option<usize> {
    let needle = b"0\r\n\r\n";
    buf.windows(needle.len())
        .position(|w| w == needle)
        .map(|i| i + needle.len())
}

fn decode_chunked(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut i = 0usize;
    while i < data.len() {
        let line_end = match data[i..].windows(2).position(|w| w == b"\r\n") {
            Some(p) => i + p,
            None => break,
        };
        let size_str = String::from_utf8_lossy(&data[i..line_end]);
        let size_hex = size_str.split(';').next().unwrap_or("").trim();
        let size = usize::from_str_radix(size_hex, 16).unwrap_or(0);
        i = line_end + 2;
        if size == 0 {
            break;
        }
        let end = (i + size).min(data.len());
        out.extend_from_slice(&data[i..end]);
        i = end + 2;
        if i > data.len() {
            break;
        }
    }
    out
}

async fn read_response(stream: &mut Upstream) -> std::io::Result<(u16, Vec<(String, String)>, Vec<u8>)> {
    let mut head_buf = Vec::new();
    let mut tmp = [0u8; 8192];
    loop {
        let n = stream.read(&mut tmp).await?;
        if n == 0 {
            break;
        }
        head_buf.extend_from_slice(&tmp[..n]);
        if head_buf.windows(4).any(|w| w == b"\r\n\r\n") || head_buf.len() > 128 * 1024 {
            break;
        }
    }
    let mut headers = [httparse::EMPTY_HEADER; 64];
    let mut resp = httparse::Response::new(&mut headers);
    let (status_code, header_list, body_start) = match resp.parse(&head_buf) {
        Ok(httparse::Status::Complete(n)) => (
            resp.code.unwrap_or(0),
            resp.headers
                .iter()
                .map(|h| (h.name.to_string(), String::from_utf8_lossy(h.value).to_string()))
                .collect::<Vec<_>>(),
            n,
        ),
        _ => return Ok((0, Vec::new(), head_buf)),
    };
    let mut body = head_buf[body_start.min(head_buf.len())..].to_vec();
    if status_code == 101 {
        return Ok((status_code, header_list, body));
    }
    if header_get(&header_list, "transfer-encoding")
        .map(|v| v.to_ascii_lowercase().contains("chunked"))
        .unwrap_or(false)
    {
        loop {
            if find_chunk_end(&body).is_some() {
                body = decode_chunked(&body);
                break;
            }
            let n = stream.read(&mut tmp).await?;
            if n == 0 {
                body = decode_chunked(&body);
                break;
            }
            body.extend_from_slice(&tmp[..n]);
        }
    } else if let Some(cl) = header_get(&header_list, "content-length") {
        let want: usize = cl.parse().unwrap_or(0);
        while body.len() < want {
            let n = stream.read(&mut tmp).await?;
            if n == 0 {
                break;
            }
            body.extend_from_slice(&tmp[..n]);
        }
        body.truncate(want);
    } else {
        loop {
            let n = stream.read(&mut tmp).await?;
            if n == 0 {
                break;
            }
            body.extend_from_slice(&tmp[..n]);
        }
    }
    Ok((status_code, header_list, body))
}

async fn connect_upstream(host: &str, port: u16, tls: bool) -> Result<Upstream, String> {
    let stream = TcpStream::connect((host, port))
        .await
        .map_err(|e| format!("connect {host}:{port}: {e}"))?;
    if !tls {
        return Ok(Upstream::Plain(stream));
    }
    let config = Arc::new(upstream_client_config()?);
    let connector = TlsConnector::from(config);
    let domain = rustls_pki_types::ServerName::try_from(host.to_string()).map_err(|e| e.to_string())?;
    let tls_stream = connector
        .connect(domain, stream)
        .await
        .map_err(|e| format!("tls upstream: {e}"))?;
    Ok(Upstream::Tls(Box::new(tls_stream)))
}

async fn connect_upstream_read_tls(host: &str, port: u16) -> Result<Upstream, String> {
    connect_upstream(host, port, true).await
}

pub async fn start_proxy(state: SharedProxy, host: &str, port: u16) -> Result<(), String> {
    if state.running.load(Ordering::SeqCst) {
        return Err("proxy already running".into());
    }
    let listener = TcpListener::bind((host, port))
        .await
        .map_err(|e| format!("bind {host}:{port}: {e}"))?;
    let actual = listener.local_addr().map_err(|e| e.to_string())?.port();
    state.port.store(actual, Ordering::SeqCst);
    state.running.store(true, Ordering::SeqCst);

    let (tx, mut rx) = tokio::sync::watch::channel(false);
    *state.listener_shutdown.lock() = Some(tx);

    let st = state.clone();
    tokio::spawn(async move {
        loop {
            tokio::select! {
                _ = rx.changed() => {
                    if *rx.borrow() {
                        break;
                    }
                }
                acc = listener.accept() => {
                    match acc {
                        Ok((stream, peer)) => {
                            let st2 = st.clone();
                            tokio::spawn(async move {
                                let _ = handle_conn(st2, stream, peer.to_string()).await;
                            });
                        }
                        Err(_) => break,
                    }
                }
            }
        }
        st.running.store(false, Ordering::SeqCst);
    });
    Ok(())
}

pub fn stop_proxy(state: &ProxyState) {
    if let Some(tx) = state.listener_shutdown.lock().take() {
        let _ = tx.send(true);
    }
    state.running.store(false, Ordering::SeqCst);
}

async fn maybe_intercept(
    state: &SharedProxy,
    raw: String,
    host: &str,
    port: u16,
    tls: bool,
    direction: &str,
) -> Result<Option<String>, String> {
    let enabled = if direction == "response" {
        state.intercept_resp.load(Ordering::SeqCst)
    } else {
        state.intercept.load(Ordering::SeqCst)
    };
    if !enabled {
        return Ok(Some(raw));
    }
    let id = state.next_id.fetch_add(1, Ordering::SeqCst);
    let (tx, rx) = tokio::sync::oneshot::channel();
    state.held.lock().insert(
        id,
        HeldRequest {
            raw_request: raw,
            host: host.to_string(),
            port,
            tls,
            direction: direction.to_string(),
            pending: tx,
        },
    );
    match rx.await {
        Ok(Some(edited)) => Ok(Some(edited)),
        _ => Ok(None),
    }
}

/// Parse `[[k, v], ...]` header pairs from extension output; None if malformed.
fn parse_header_pairs(v: &serde_json::Value) -> Option<Vec<(String, String)>> {
    let arr = v.as_array()?;
    Some(
        arr.iter()
            .filter_map(|p| {
                let p = p.as_array()?;
                Some((
                    p.first()?.as_str()?.to_string(),
                    p.get(1)?.as_str()?.to_string(),
                ))
            })
            .collect(),
    )
}

/// Build a message JSON for the request phase and run it through extension HTTP handlers.
/// Returns the (possibly modified) raw request string.
fn apply_ext_request_handlers(
    state: &SharedProxy,
    method: &str,
    url: &str,
    headers: &[(String, String)],
    body: &[u8],
) -> String {
    let msg = serde_json::json!({
        "request": {
            "method": method,
            "url": url,
            "headers": headers.iter().map(|(k, v)| serde_json::json!([k, v])).collect::<Vec<_>>(),
            "body": String::from_utf8_lossy(body),
        },
        "response": null,
    });
    let out = state.extensions.run_http_handlers(msg);
    let req = &out["request"];
    let method = req["method"].as_str().unwrap_or(method).to_string();
    let url = req["url"].as_str().unwrap_or(url).to_string();
    let headers: Vec<(String, String)> = parse_header_pairs(&req["headers"]).unwrap_or_else(|| headers.to_vec());
    let body = req["body"].as_str().map(|s| s.as_bytes().to_vec()).unwrap_or_else(|| body.to_vec());
    let target = url::Url::parse(&url)
        .ok()
        .map(|u| {
            format!(
                "{}{}",
                u.path(),
                u.query().map(|q| format!("?{q}")).unwrap_or_default()
            )
        })
        .unwrap_or_else(|| url.clone());
    String::from_utf8_lossy(&rebuild_request(
        &format!("{method} {target} HTTP/1.1"),
        &headers,
        &body,
    ))
    .into_owned()
}

/// Build a message JSON for the response phase and run it through extension HTTP handlers.
/// Returns the (possibly modified) raw response string.
fn apply_ext_response_handlers(
    state: &SharedProxy,
    status: u16,
    headers: &[(String, String)],
    body: &[u8],
    req_method: &str,
    req_url: &str,
    req_headers: &[(String, String)],
    req_body: &[u8],
) -> String {
    let msg = serde_json::json!({
        "request": {
            "method": req_method,
            "url": req_url,
            "headers": req_headers.iter().map(|(k, v)| format!("{k}: {v}")).collect::<Vec<_>>(),
            "body": String::from_utf8_lossy(req_body),
        },
        "response": {
            "status": status,
            "headers": headers.iter().map(|(k, v)| serde_json::json!([k, v])).collect::<Vec<_>>(),
            "body": String::from_utf8_lossy(body),
        },
    });
    let out = state.extensions.run_http_handlers(msg);
    let resp = &out["response"];
    if resp.is_null() {
        return String::from_utf8_lossy(&rebuild_response(status, headers, body)).into_owned();
    }
    let status = resp["status"].as_u64().unwrap_or(status as u64) as u16;
    let headers: Vec<(String, String)> = parse_header_pairs(&resp["headers"]).unwrap_or_else(|| headers.to_vec());
    let body = resp["body"].as_str().map(|s| s.as_bytes().to_vec()).unwrap_or_else(|| body.to_vec());
    String::from_utf8_lossy(&rebuild_response(status, &headers, &body)).into_owned()
}

fn record_exchange(
    state: &SharedProxy,
    peer: String,
    host: String,
    method: String,
    url: String,
    req_headers: Vec<(String, String)>,
    req_body: String,
    status: u16,
    resp_headers: Vec<(String, String)>,
    resp_body: String,
    time_ms: u64,
    tls: bool,
    websocket: bool,
) {
    let id = state.next_id.fetch_add(1, Ordering::SeqCst);
    let in_scope = {
        let rules = state.scope.lock();
        db::is_in_scope(&rules, &host, &url)
    };
    let ex = HttpExchange {
        id,
        started_at: Utc::now().to_rfc3339(),
        time_ms: Some(time_ms),
        client_addr: Some(peer),
        host,
        method,
        url,
        req_headers,
        req_body: Some(req_body),
        status: Some(status),
        resp_headers,
        resp_body: Some(resp_body),
        tls,
        websocket,
        in_scope,
    };
    if let Some(database) = state.db.read().clone() {
        let ex2 = ex.clone();
        let scan = ex.in_scope && !ex.websocket;
        let state2 = state.clone();
        let app = state.app_handle.read().clone();
        tokio::spawn(async move {
            let _ = database.insert_exchange(&ex2);
            if scan {
                crate::scanner::passive_scan_and_record(app.as_ref(), &state2, &ex2);
            }
        });
    }
    let proxy_payload = serde_json::json!({
        "id": id,
        "host": ex.host,
        "method": ex.method,
        "url": ex.url,
        "req_headers": ex.req_headers.iter().map(|(k, v)| format!("{k}: {v}")).collect::<Vec<_>>(),
        "req_body": ex.req_body.clone().unwrap_or_default(),
        "status": ex.status,
        "resp_headers": ex.resp_headers.iter().map(|(k, v)| format!("{k}: {v}")).collect::<Vec<_>>(),
        "resp_body": ex.resp_body.clone().unwrap_or_default(),
        "tls": ex.tls,
        "websocket": ex.websocket,
    });
    state.extensions.run_proxy_listeners(&proxy_payload);

    let mut h = state.history.lock();
    h.push(ex);
    if h.len() > 20_000 {
        let drop_n = h.len() - 20_000;
        h.drain(0..drop_n);
    }
}

const WS_PAYLOAD_CAP: usize = 4096;
const WS_MAX_FRAME: usize = 8 * 1024 * 1024;

fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn encode_ws_payload(opcode: u8, compressed: bool, payload: &[u8]) -> String {
    let mut encoded = if opcode == 0x1 && !compressed {
        String::from_utf8_lossy(payload).to_string()
    } else {
        hex_encode(payload)
    };
    if encoded.len() > WS_PAYLOAD_CAP {
        let mut cut = WS_PAYLOAD_CAP;
        if opcode != 0x1 || compressed {
            cut -= cut % 2;
        }
        while cut > 0 && !encoded.is_char_boundary(cut) {
            cut -= 1;
        }
        encoded.truncate(cut);
    }
    encoded
}

fn parse_ws_frame(
    buf: &[u8],
) -> Result<Option<(u8, bool, Vec<u8>, usize)>, ()> {
    if buf.len() < 2 {
        return Ok(None);
    }
    let rsv1 = buf[0] & 0x40 != 0;
    let opcode = buf[0] & 0x0f;
    let masked = buf[1] & 0x80 != 0;
    let len7 = (buf[1] & 0x7f) as usize;
    let mut pos = 2usize;
    let payload_len = match len7 {
        126 => {
            if buf.len() < 4 {
                return Ok(None);
            }
            pos = 4;
            u16::from_be_bytes([buf[2], buf[3]]) as usize
        }
        127 => {
            if buf.len() < 10 {
                return Ok(None);
            }
            pos = 10;
            let mut a = [0u8; 8];
            a.copy_from_slice(&buf[2..10]);
            if a[0] & 0x80 != 0 {
                return Err(());
            }
            let v = u64::from_be_bytes(a);
            if v > WS_MAX_FRAME as u64 {
                return Err(());
            }
            v as usize
        }
        n => n,
    };
    if payload_len > WS_MAX_FRAME {
        return Err(());
    }
    let mask_len = if masked { 4 } else { 0 };
    let total = pos + mask_len + payload_len;
    if buf.len() < total {
        return Ok(None);
    }
    let payload_start = pos + mask_len;
    let mut payload = buf[payload_start..total].to_vec();
    if masked {
        let key = &buf[pos..pos + 4];
        for (i, b) in payload.iter_mut().enumerate() {
            *b ^= key[i % 4];
        }
    }
    Ok(Some((opcode, rsv1, payload, total)))
}

fn emit_ws(state: &SharedProxy, conn_id: i64) {
    if let Some(app) = state.app_handle.read().clone() {
        let _ = app.emit("ws-updated", conn_id);
    }
}

fn ws_conn_start(state: &SharedProxy, url: &str, host: &str, port: u16, tls: bool) -> Option<i64> {
    let db = state.db.read().clone()?;
    let id = db.insert_ws_conn(url, host, port, tls).ok()?;
    emit_ws(state, id);
    Some(id)
}

fn ws_record_frame(state: &SharedProxy, conn_id: Option<i64>, dir: &str, opcode: u8, compressed: bool, payload: &[u8]) {
    let Some(conn_id) = conn_id else { return };
    let Some(db) = state.db.read().clone() else { return };
    let encoded = encode_ws_payload(opcode, compressed, payload);
    if db.insert_ws_frame(conn_id, dir, opcode, payload.len(), &encoded).is_ok() {
        emit_ws(state, conn_id);
    }
}

fn ws_warn_fallback(state: &SharedProxy, dir: &str) {
    if let Some(db) = state.db.read().clone() {
        if let Ok(entry) = db.insert_log(
            "proxy",
            "warn",
            &format!("WebSocket frame parse failed ({dir}) — falling back to transparent relay"),
        ) {
            if let Some(app) = state.app_handle.read().clone() {
                let _ = app.emit("log-added", entry);
            }
        }
    }
}

fn ws_conn_end(state: &SharedProxy, conn_id: Option<i64>) {
    let Some(conn_id) = conn_id else { return };
    if let Some(db) = state.db.read().clone() {
        let _ = db.finish_ws_conn(conn_id);
    }
    emit_ws(state, conn_id);
}

fn drain_ws_frames(state: &SharedProxy, conn_id: Option<i64>, dir: &str, buf: &mut Vec<u8>) -> Result<(), ()> {
    loop {
        match parse_ws_frame(buf) {
            Ok(Some((opcode, rsv1, payload, total))) => {
                ws_record_frame(state, conn_id, dir, opcode, rsv1, &payload);
                buf.drain(0..total);
            }
            Ok(None) => return Ok(()),
            Err(()) => return Err(()),
        }
    }
}

async fn ws_relay<C>(
    state: SharedProxy,
    conn_id: Option<i64>,
    client: &mut C,
    upstream: &mut Upstream,
) where
    C: AsyncReadExt + AsyncWriteExt + Unpin,
{
    let mut cbuf: Vec<u8> = Vec::new();
    let mut sbuf: Vec<u8> = Vec::new();
    let mut cparse = true;
    let mut sparse = true;
    loop {
        let mut ctmp = [0u8; 8192];
        let mut stmp = [0u8; 8192];
        tokio::select! {
            n = client.read(&mut ctmp) => {
                match n {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        let chunk = ctmp[..n].to_vec();
                        if upstream.write_all(&chunk).await.is_err() {
                            break;
                        }
                        if cparse {
                            cbuf.extend_from_slice(&chunk);
                            if drain_ws_frames(&state, conn_id, "c", &mut cbuf).is_err() {
                                cparse = false;
                                cbuf.clear();
                                ws_warn_fallback(&state, "c");
                            }
                        }
                    }
                }
            }
            n = upstream.read(&mut stmp) => {
                match n {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        let chunk = stmp[..n].to_vec();
                        if client.write_all(&chunk).await.is_err() {
                            break;
                        }
                        if sparse {
                            sbuf.extend_from_slice(&chunk);
                            if drain_ws_frames(&state, conn_id, "s", &mut sbuf).is_err() {
                                sparse = false;
                                sbuf.clear();
                                ws_warn_fallback(&state, "s");
                            }
                        }
                    }
                }
            }
        }
    }
    let _ = client.shutdown().await;
    ws_conn_end(&state, conn_id);
}

async fn handle_conn(state: SharedProxy, mut stream: TcpStream, peer: String) -> Result<(), String> {
    let head = read_until_headers(&mut stream, 64 * 1024)
        .await
        .map_err(|e| e.to_string())?;
    if head.is_empty() {
        return Ok(());
    }
    let head_str = String::from_utf8_lossy(&head).to_string();
    let first_line = head_str.split("\r\n").next().unwrap_or("").to_string();
    let mut parts = first_line.split_whitespace();
    let method = parts.next().unwrap_or("").to_string();
    let target = parts.next().unwrap_or("").to_string();

    if method.eq_ignore_ascii_case("CONNECT") {
        return handle_connect(state, stream, target, peer).await;
    }

    let url = if target.starts_with("http://") || target.starts_with("https://") {
        target.clone()
    } else {
        format!("http://{target}")
    };
    let parsed = url::Url::parse(&url).map_err(|e| e.to_string())?;
    let host = parsed.host_str().unwrap_or("").to_string();
    let port = parsed.port().unwrap_or(80);
    let path = format!(
        "{}{}",
        parsed.path(),
        parsed.query().map(|q| format!("?{q}")).unwrap_or_default()
    );

    let mut headers = [httparse::EMPTY_HEADER; 128];
    let mut req = httparse::Request::new(&mut headers);
    let _ = req.parse(&head);
    let header_list: Vec<(String, String)> = req
        .headers
        .iter()
        .map(|h| (h.name.to_string(), String::from_utf8_lossy(h.value).to_string()))
        .collect();

    let mut upstream_headers = header_list.clone();
    upstream_headers.retain(|(k, _)| !k.eq_ignore_ascii_case("proxy-connection"));
    if !upstream_headers.iter().any(|(k, _)| k.eq_ignore_ascii_case("host")) {
        let host_val = if port == 80 {
            host.clone()
        } else {
            format!("{host}:{port}")
        };
        upstream_headers.push(("Host".into(), host_val));
    }

    let body_start = head
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .map(|i| i + 4)
        .unwrap_or(head.len());
    let mut body = head[body_start.min(head.len())..].to_vec();
    if let Some(cl) = header_get(&header_list, "content-length") {
        let want: usize = cl.parse().unwrap_or(0);
        let mut tmp = [0u8; 8192];
        while body.len() < want {
            let n = stream.read(&mut tmp).await.map_err(|e| e.to_string())?;
            if n == 0 {
                break;
            }
            body.extend_from_slice(&tmp[..n]);
        }
        body.truncate(want);
    }

    let request_line = format!("{} {} HTTP/1.1", method, path);
    let raw_for_intercept =
        String::from_utf8_lossy(&rebuild_request(&request_line, &upstream_headers, &body)).to_string();

    let modified = maybe_intercept(&state, raw_for_intercept, &host, port, false, "request").await?;
    let Some(raw_out) = modified else {
        let _ = stream
            .write_all(b"HTTP/1.1 403 Forbidden\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
            .await;
        return Ok(());
    };
    let (rl2, headers2, body2) =
        split_message(&raw_out).unwrap_or((request_line, upstream_headers, body.clone()));
    let ws_upgrade_req = header_get(&headers2, "upgrade")
        .map(|v| v.to_ascii_lowercase().contains("websocket"))
        .unwrap_or(false);

    let ext_req_raw = apply_ext_request_handlers(&state, &method, &url, &headers2, &body2);
    let (rl2, headers2, body2) =
        split_message(&ext_req_raw).unwrap_or((rl2, headers2, body2));

    let outbound = {
        let rules = state.match_replace.lock();
        db::process_with_rules(&rules, "request", &rebuild_request(&rl2, &headers2, &body2))
    };

    let started = std::time::Instant::now();
    let mut up = connect_upstream(&host, port, false).await?;
    if let Err(e) = up.write_all(&outbound).await {
        let _ = stream
            .write_all(format!("HTTP/1.1 502 Bad Gateway\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", e.to_string().len(), e).as_bytes())
            .await;
        return Ok(());
    }
    let (status, resp_headers, resp_body) = read_response(&mut up).await.map_err(|e| e.to_string())?;
    let time_ms = started.elapsed().as_millis() as u64;

    let mut resp_bytes = Vec::new();
    resp_bytes.extend_from_slice(format!("HTTP/1.1 {}\r\n", status).as_bytes());
    for (k, v) in &resp_headers {
        if k.eq_ignore_ascii_case("transfer-encoding") || k.eq_ignore_ascii_case("content-length") {
            continue;
        }
        resp_bytes.extend_from_slice(format!("{k}: {v}\r\n").as_bytes());
    }
    if status == 101 {
        resp_bytes.extend_from_slice(b"\r\n");
    } else {
        resp_bytes.extend_from_slice(
            format!("Content-Length: {}\r\nConnection: close\r\n\r\n", resp_body.len()).as_bytes(),
        );
    }
    resp_bytes.extend_from_slice(&resp_body);
    let mut resp_out = {
        let rules = state.match_replace.lock();
        db::process_with_rules(&rules, "response", &resp_bytes)
    };
    let resp_text = String::from_utf8_lossy(&resp_out).to_string();
    let modified = maybe_intercept(&state, resp_text, &host, port, false, "response").await?;
    match modified {
        Some(edited) => {
            resp_out = edited.into_bytes();
        }
        None => {
            let _ = stream.shutdown().await;
            return Ok(());
        }
    }
    let ws_open = ws_upgrade_req && status == 101;
    if !ws_open {
        resp_out = apply_ext_response_handlers(
            &state,
            status,
            &resp_headers,
            &resp_body,
            &method,
            &url,
            &headers2,
            &body2,
        )
        .into_bytes();
    }
    stream.write_all(&resp_out).await.map_err(|e| e.to_string())?;
    if !ws_open {
        let _ = stream.shutdown().await;
    }

    let (rl3, h3, b3) = split_message(&String::from_utf8_lossy(&outbound)).unwrap_or((
        rl2,
        headers2,
        body2.clone(),
    ));
    let method2 = rl3.split_whitespace().next().unwrap_or("GET").to_string();
    let (r_status, r_headers, r_body) = split_response_text(&String::from_utf8_lossy(&resp_out))
        .unwrap_or((
            status,
            resp_headers,
            String::from_utf8_lossy(&resp_out).to_string(),
        ));
    record_exchange(
        &state,
        peer,
        host.clone(),
        method2,
        url.clone(),
        h3,
        String::from_utf8_lossy(&b3).to_string(),
        r_status,
        r_headers,
        r_body,
        time_ms,
        false,
        ws_open,
    );
    if ws_open {
        let conn_id = ws_conn_start(&state, &url, &host, port, false);
        ws_relay(state, conn_id, &mut stream, &mut up).await;
        return Ok(());
    }
    Ok(())
}

async fn handle_connect(
    state: SharedProxy,
    mut stream: TcpStream,
    target: String,
    peer: String,
) -> Result<(), String> {
    let (host, port) = if let Some((h, p)) = target.rsplit_once(':') {
        (h.to_string(), p.parse().unwrap_or(443))
    } else {
        (target.clone(), 443u16)
    };

    let ca = state.ca.read().clone().ok_or("CA not loaded")?;
    let (leaf_cert, leaf_key) = mint_leaf(&ca, &host)?;
    let leaf_key_der = leaf_key.serialize_der();
    let leaf_cert_der = leaf_cert.der().clone();

    let server_config = ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(
            vec![leaf_cert_der],
            PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(leaf_key_der)),
        )
        .map_err(|e| e.to_string())?;
    let acceptor = TlsAcceptor::from(Arc::new(server_config));

    stream
        .write_all(b"HTTP/1.1 200 Connection Established\r\n\r\n")
        .await
        .map_err(|e| e.to_string())?;

    let mut tls_stream = acceptor
        .accept(stream)
        .await
        .map_err(|e| format!("mitm tls: {e}"))?;

    let mut head_buf = Vec::new();
    let mut tmp = [0u8; 8192];
    loop {
        let n = tls_stream.read(&mut tmp).await.map_err(|e| e.to_string())?;
        if n == 0 {
            return Ok(());
        }
        head_buf.extend_from_slice(&tmp[..n]);
        if head_buf.windows(4).any(|w| w == b"\r\n\r\n") {
            break;
        }
        if head_buf.len() > 256 * 1024 {
            break;
        }
    }

    let mut headers = [httparse::EMPTY_HEADER; 128];
    let mut req = httparse::Request::new(&mut headers);
    let (request_line, header_list, body_start) = match req.parse(&head_buf) {
        Ok(httparse::Status::Complete(n)) => {
            let method = req.method.unwrap_or("GET");
            let path = req.path.unwrap_or("/");
            let version = req
                .version
                .map(|v| format!("HTTP/1.{v}"))
                .unwrap_or_else(|| "HTTP/1.1".into());
            let rl = format!("{method} {path} {version}");
            let hl = req
                .headers
                .iter()
                .map(|h| (h.name.to_string(), String::from_utf8_lossy(h.value).to_string()))
                .collect::<Vec<_>>();
            (rl, hl, n)
        }
        _ => return Ok(()),
    };

    let mut body = head_buf[body_start.min(head_buf.len())..].to_vec();
    if let Some(cl) = header_get(&header_list, "content-length") {
        let want: usize = cl.parse().unwrap_or(0);
        let mut tmp = [0u8; 8192];
        while body.len() < want {
            let n = tls_stream.read(&mut tmp).await.map_err(|e| e.to_string())?;
            if n == 0 {
                break;
            }
            body.extend_from_slice(&tmp[..n]);
        }
        body.truncate(want);
    }

    let path = request_line.split_whitespace().nth(1).unwrap_or("/").to_string();
    let url = format!("https://{host}:{port}{path}");
    let raw_for_intercept =
        String::from_utf8_lossy(&rebuild_request(&request_line, &header_list, &body)).to_string();
    let modified = maybe_intercept(&state, raw_for_intercept, &host, port, true, "request").await?;
    let Some(raw_out) = modified else {
        let _ = tls_stream
            .write_all(b"HTTP/1.1 403 Forbidden\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
            .await;
        return Ok(());
    };
    let (rl2, headers2, body2) =
        split_message(&raw_out).unwrap_or((request_line, header_list, body));
    let ws_upgrade_req = header_get(&headers2, "upgrade")
        .map(|v| v.to_ascii_lowercase().contains("websocket"))
        .unwrap_or(false);

    let method = rl2.split_whitespace().next().unwrap_or("GET").to_string();
    let ext_req_raw = apply_ext_request_handlers(&state, &method, &url, &headers2, &body2);
    let (rl2, headers2, body2) =
        split_message(&ext_req_raw).unwrap_or((rl2, headers2, body2));

    let started = std::time::Instant::now();
    let mut up = connect_upstream_read_tls(&host, port).await?;
    let outbound = {
        let rules = state.match_replace.lock();
        db::process_with_rules(&rules, "request", &rebuild_request(&rl2, &headers2, &body2))
    };
    up.write_all(&outbound).await.map_err(|e| e.to_string())?;
    let (status, resp_headers, resp_body) = read_response(&mut up).await.map_err(|e| e.to_string())?;
    let time_ms = started.elapsed().as_millis() as u64;

    let mut resp_bytes = Vec::new();
    resp_bytes.extend_from_slice(format!("HTTP/1.1 {}\r\n", status).as_bytes());
    for (k, v) in &resp_headers {
        if k.eq_ignore_ascii_case("transfer-encoding") || k.eq_ignore_ascii_case("content-length") {
            continue;
        }
        resp_bytes.extend_from_slice(format!("{k}: {v}\r\n").as_bytes());
    }
    if status == 101 {
        resp_bytes.extend_from_slice(b"\r\n");
    } else {
        resp_bytes.extend_from_slice(
            format!("Content-Length: {}\r\nConnection: close\r\n\r\n", resp_body.len()).as_bytes(),
        );
    }
    resp_bytes.extend_from_slice(&resp_body);
    let mut resp_out = {
        let rules = state.match_replace.lock();
        db::process_with_rules(&rules, "response", &resp_bytes)
    };
    let resp_text = String::from_utf8_lossy(&resp_out).to_string();
    let modified = maybe_intercept(&state, resp_text, &host, port, true, "response").await?;
    match modified {
        Some(edited) => {
            resp_out = edited.into_bytes();
        }
        None => {
            let _ = tls_stream.shutdown().await;
            return Ok(());
        }
    }
    let ws_open = ws_upgrade_req && status == 101;
    if !ws_open {
        resp_out = apply_ext_response_handlers(
            &state,
            status,
            &resp_headers,
            &resp_body,
            &method,
            &url,
            &headers2,
            &body2,
        )
        .into_bytes();
    }
    tls_stream.write_all(&resp_out).await.map_err(|e| e.to_string())?;
    if !ws_open {
        let _ = tls_stream.shutdown().await;
    }

    let (rl3, h3, b3) = split_message(&String::from_utf8_lossy(&outbound)).unwrap_or((
        rl2,
        headers2,
        body2.clone(),
    ));
    let method2 = rl3.split_whitespace().next().unwrap_or("GET").to_string();
    let (r_status, r_headers, r_body) = split_response_text(&String::from_utf8_lossy(&resp_out))
        .unwrap_or((
            status,
            resp_headers,
            String::from_utf8_lossy(&resp_out).to_string(),
        ));
    record_exchange(
        &state,
        peer,
        host.clone(),
        method2,
        url.clone(),
        h3,
        String::from_utf8_lossy(&b3).to_string(),
        r_status,
        r_headers,
        r_body,
        time_ms,
        true,
        ws_open,
    );
    if ws_open {
        let conn_id = ws_conn_start(&state, &url, &host, port, true);
        ws_relay(state, conn_id, &mut tls_stream, &mut up).await;
        return Ok(());
    }
    Ok(())
}

pub async fn send_raw_request(
    raw: String,
    host: String,
    port: u16,
    tls: bool,
) -> Result<(u16, Vec<(String, String)>, String, u64), String> {
    let (rl, headers, body) = split_message(&raw).ok_or("invalid request")?;
    let outbound = rebuild_request(&rl, &headers, &body);
    let started = std::time::Instant::now();
    let mut up = connect_upstream(&host, port, tls).await?;
    up.write_all(&outbound).await.map_err(|e| e.to_string())?;
    let (status, resp_headers, resp_body) = read_response(&mut up).await.map_err(|e| e.to_string())?;
    let time_ms = started.elapsed().as_millis() as u64;
    Ok((
        status,
        resp_headers,
        String::from_utf8_lossy(&resp_body).to_string(),
        time_ms,
    ))
}

fn absolute_url_from_request(raw: &str) -> Option<String> {
    raw.lines().find_map(|l| {
        let target = l.split_whitespace().nth(1)?;
        if target.starts_with("http://") || target.starts_with("https://") {
            Some(target.to_string())
        } else {
            None
        }
    })
}

fn resolve_location(base: &str, location: &str) -> Option<String> {
    if location.starts_with("http://") || location.starts_with("https://") {
        return Some(location.to_string());
    }
    let b = url::Url::parse(base).ok()?;
    let joined = b.join(location).ok()?;
    if joined.scheme() == "http" || joined.scheme() == "https" {
        Some(joined.to_string())
    } else {
        None
    }
}

fn rewrite_for_redirect(raw: &str, next_url: &str, status: u16) -> Result<String, String> {
    let (rl, headers, body) = split_message(raw).ok_or("invalid request")?;
    let mut parts = rl.split_whitespace();
    let method = parts.next().unwrap_or("GET").to_string();
    let version = parts.next_back().unwrap_or("HTTP/1.1").to_string();
    let u = url::Url::parse(next_url).map_err(|e| e.to_string())?;
    let path = format!(
        "{}{}",
        u.path(),
        u.query().map(|q| format!("?{q}")).unwrap_or_default()
    );
    let new_method = match status {
        303 if !method.eq_ignore_ascii_case("HEAD") => "GET".to_string(),
        301 | 302 if method.eq_ignore_ascii_case("POST") => "GET".to_string(),
        _ => method.clone(),
    };
    let authority = match u.port() {
        Some(p) => format!("{}:{}", u.host_str().unwrap_or_default(), p),
        None => u.host_str().unwrap_or_default().to_string(),
    };
    let mut new_headers: Vec<(String, String)> = vec![("Host".into(), authority)];
    for (k, v) in &headers {
        if k.eq_ignore_ascii_case("host") {
            continue;
        }
        if new_method != method
            && (k.eq_ignore_ascii_case("content-length")
                || k.eq_ignore_ascii_case("transfer-encoding"))
        {
            continue;
        }
        new_headers.push((k.clone(), v.clone()));
    }
    let body_out = if new_method == method { body } else { Vec::new() };
    let rl_new = format!("{new_method} {path} {version}");
    Ok(String::from_utf8_lossy(&rebuild_request(&rl_new, &new_headers, &body_out)).to_string())
}

#[derive(Clone, serde::Serialize)]
pub struct RawResponse {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: String,
    pub time_ms: u64,
    pub final_url: String,
    pub redirects: u32,
}

pub async fn send_raw_request_follow(
    raw: String,
    url_hint: Option<String>,
    follow: bool,
) -> Result<RawResponse, String> {
    let mut current_raw = raw;
    let mut current_url = match url_hint.filter(|u| !u.is_empty()) {
        Some(u) => u,
        None => absolute_url_from_request(&current_raw)
            .ok_or("cannot determine host — use absolute URL in request line")?,
    };
    let started = std::time::Instant::now();
    let mut redirects = 0u32;
    let max_hops = if follow { 5u32 } else { 0 };
    loop {
        let u = url::Url::parse(&current_url).map_err(|e| e.to_string())?;
        let host = u.host_str().ok_or("url has no host")?.to_string();
        let tls = match u.scheme() {
            "https" => true,
            "http" => false,
            other => return Err(format!("unsupported scheme: {other}")),
        };
        let port = u.port().unwrap_or(if tls { 443 } else { 80 });
        let (status, headers, body, _ms) =
            send_raw_request(current_raw.clone(), host, port, tls).await?;
        let redirect_to = if redirects < max_hops && matches!(status, 301 | 302 | 303 | 307 | 308)
        {
            headers
                .iter()
                .find(|(k, _)| k.eq_ignore_ascii_case("location"))
                .and_then(|(_, v)| resolve_location(&current_url, v))
        } else {
            None
        };
        match redirect_to {
            Some(next) => {
                current_raw = rewrite_for_redirect(&current_raw, &next, status)?;
                current_url = next;
                redirects += 1;
            }
            None => {
                return Ok(RawResponse {
                    status,
                    headers,
                    body,
                    time_ms: started.elapsed().as_millis() as u64,
                    final_url: current_url,
                    redirects,
                });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn masked_frame(opcode: u8, payload: &[u8]) -> Vec<u8> {
        let mask = [0x12u8, 0x34, 0x56, 0x78];
        let mut f = vec![0x80 | opcode];
        if payload.len() < 126 {
            f.push(0x80 | payload.len() as u8);
        } else {
            f.push(0x80 | 126);
            f.extend_from_slice(&(payload.len() as u16).to_be_bytes());
        }
        f.extend_from_slice(&mask);
        for (i, b) in payload.iter().enumerate() {
            f.push(b ^ mask[i % 4]);
        }
        f
    }

    #[test]
    fn parse_masked_client_frame() {
        let frame = masked_frame(0x1, b"hello");
        let (opcode, rsv1, payload, total) = parse_ws_frame(&frame).unwrap().unwrap();
        assert_eq!(opcode, 0x1);
        assert!(!rsv1);
        assert_eq!(payload, b"hello");
        assert_eq!(total, frame.len());
    }

    #[test]
    fn parse_unmasked_server_frame() {
        let frame = [0x82u8, 0x03, 0x01, 0x02, 0x03];
        let (opcode, _, payload, total) = parse_ws_frame(&frame).unwrap().unwrap();
        assert_eq!(opcode, 0x2);
        assert_eq!(payload, vec![1, 2, 3]);
        assert_eq!(total, 5);
    }

    #[test]
    fn parse_incomplete_returns_none() {
        assert!(parse_ws_frame(&[0x81]).unwrap().is_none());
        assert!(parse_ws_frame(&[0x81, 0x05, 0x61]).unwrap().is_none());
    }

    #[test]
    fn parse_multiple_frames_and_partial_tail() {
        let mut buf = masked_frame(0x1, b"hi");
        buf.extend_from_slice(&masked_frame(0x1, b"there"));
        buf.extend_from_slice(&[0x8a, 0x80, 0, 0]);
        let mut seen = Vec::new();
        loop {
            match parse_ws_frame(&buf).unwrap() {
                Some((op, _, payload, total)) => {
                    seen.push((op, payload));
                    buf.drain(0..total);
                }
                None => break,
            }
        }
        assert_eq!(seen.len(), 2);
        assert_eq!(seen[0].1, b"hi");
        assert_eq!(seen[1].1, b"there");
        assert_eq!(buf.len(), 4);
    }

    #[test]
    fn encode_payload_text_vs_binary() {
        assert_eq!(encode_ws_payload(0x1, false, "héllo".as_bytes()), "héllo");
        assert_eq!(encode_ws_payload(0x2, false, &[0xde, 0xad]), "dead");
        assert_eq!(encode_ws_payload(0x9, false, &[]), "");
        assert_eq!(encode_ws_payload(0x1, true, b"abc"), "616263");
    }

    #[test]
    fn encode_payload_truncates_long_hex_to_even() {
        let payload = vec![0xabu8; 3000];
        let enc = encode_ws_payload(0x2, false, &payload);
        assert_eq!(enc.len(), WS_PAYLOAD_CAP);
        assert!(enc.len() % 2 == 0);
    }

    #[test]
    fn rewrite_drops_body_on_303() {
        let raw = "POST /submit HTTP/1.1\r\nHost: a.example\r\nContent-Length: 5\r\n\r\nhello";
        let out = rewrite_for_redirect(raw, "https://a.example/result", 303).unwrap();
        assert!(out.starts_with("GET /result HTTP/1.1\r\n"));
        assert!(out.to_ascii_lowercase().contains("host: a.example"));
        assert!(!out.to_ascii_lowercase().contains("content-length"));
        assert!(!out.ends_with("hello"));
    }

    #[test]
    fn rewrite_preserves_body_on_307() {
        let raw = "POST /a HTTP/1.1\r\nHost: h\r\nContent-Length: 4\r\n\r\nbody";
        let out = rewrite_for_redirect(raw, "http://h/b", 307).unwrap();
        assert!(out.starts_with("POST /b HTTP/1.1\r\n"));
        assert!(out.ends_with("body"));
        assert!(out.contains("Content-Length: 4"));
    }

    #[test]
    fn resolve_relative_location() {
        assert_eq!(
            resolve_location("https://h.example/x/y", "/z").unwrap(),
            "https://h.example/z"
        );
        assert_eq!(
            resolve_location("http://h.example/x/y", "q").unwrap(),
            "http://h.example/x/q"
        );
        assert_eq!(
            resolve_location("https://h.example/x", "https://other.example/").unwrap(),
            "https://other.example/"
        );
    }
}
