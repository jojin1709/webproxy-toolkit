// Phase 7: Collaborator — local out-of-band interaction server (DNS + HTTP).
//
// Minimal DNS responder (A/AAAA/NXDOMAIN) + plain-HTTP listener. Every query or
// request is persisted as an interaction and pushed to the UI via the
// `collab-interaction` event. Payloads embed `<8-hex>.<domain>` hostnames.

use std::net::Ipv4Addr;
use std::sync::atomic::{AtomicBool, AtomicU16, Ordering};

use parking_lot::{Mutex, RwLock};
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream, UdpSocket};

use crate::proxy::SharedProxy;

// ---------------------------------------------------------------- state

pub struct CollabState {
    pub running: AtomicBool,
    pub dns_port: AtomicU16,
    pub http_port: AtomicU16,
    pub domain: RwLock<String>,
    pub bind_ip: RwLock<Ipv4Addr>,
    shutdown: Mutex<Option<tokio::sync::watch::Sender<bool>>>,
}

impl Default for CollabState {
    fn default() -> Self {
        Self {
            running: AtomicBool::new(false),
            dns_port: AtomicU16::new(5354),
            http_port: AtomicU16::new(8081),
            domain: RwLock::new("oob.wpt.local".into()),
            bind_ip: RwLock::new(Ipv4Addr::LOCALHOST),
            shutdown: Mutex::new(None),
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct CollabStatus {
    pub running: bool,
    pub dns_port: u16,
    pub http_port: u16,
    pub domain: String,
    pub bind_ip: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct CollabPayload {
    pub id: i64,
    pub label: String,
    pub domain: String,
    pub kind: String,
    pub value: String,
    pub created_at: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct CollabInteraction {
    pub id: i64,
    pub payload_id: Option<i64>,
    pub subdomain: String,
    pub protocol: String,
    pub source: String,
    pub detail: serde_json::Value,
    pub seen_at: String,
}

// ---------------------------------------------------------------- payload gen

pub fn gen_subdomain() -> String {
    let u = uuid::Uuid::new_v4().simple().to_string();
    u[..8].to_string()
}

pub fn make_payload_value(kind: &str, host: &str) -> String {
    match kind {
        "url" => format!("http://{host}/"),
        "html" => format!("<img src=\"http://{host}/x\">"),
        _ => host.to_string(),
    }
}

pub fn first_label(name: &str) -> &str {
    name.split('.').next().unwrap_or("")
}

/// True if the interaction hostname's first label belongs to this payload
/// (`<sub>.<domain>` appears in the payload value).
pub fn matches_payload(value: &str, subdomain: &str, domain: &str) -> bool {
    value.contains(&format!("{subdomain}.{domain}"))
}

// ---------------------------------------------------------------- DNS wire

/// Parse the question section of a DNS query (starting after the 12-byte
/// header) → (lowercased qname, qtype).
pub fn parse_query_name(q: &[u8]) -> Option<(String, u16)> {
    let mut i = 0usize;
    let mut labels = Vec::new();
    loop {
        let len = *q.get(i)? as usize;
        if len == 0 {
            i += 1;
            break;
        }
        if len > 63 || i + 1 + len > q.len() {
            return None;
        }
        labels.push(String::from_utf8_lossy(&q[i + 1..i + 1 + len]).to_lowercase());
        i += 1 + len;
    }
    let qtype = u16::from_be_bytes([*q.get(i)?, *q.get(i + 1)?]);
    Some((labels.join("."), qtype))
}

/// Build a DNS response for `query`. Matching domain → A record (bind_ip,
/// TTL 60) or empty NOERROR (AAAA/other); non-matching → NXDOMAIN.
pub fn build_dns_response(query: &[u8], domain: &str, bind_ip: Ipv4Addr) -> Option<Vec<u8>> {
    if query.len() < 12 {
        return None;
    }
    let id = u16::from_be_bytes([query[0], query[1]]);
    let qdcount = u16::from_be_bytes([query[4], query[5]]);
    if qdcount == 0 {
        return None;
    }
    let (name, qtype) = parse_query_name(&query[12..])?;
    // walk the question to find its end (name + qtype + qclass)
    let mut i = 12usize;
    loop {
        let len = *query.get(i)? as usize;
        i += 1;
        if len == 0 {
            break;
        }
        if len > 63 {
            return None;
        }
        i += len;
    }
    let qend = i + 4;
    if qend > query.len() {
        return None;
    }
    let domain = domain.to_lowercase();
    let matches = name == domain || name.ends_with(&format!(".{domain}"));
    let mut flags: u16 = 0x8180; // QR | RD | RA
    let mut ancount = 0u16;
    let mut answer: Vec<u8> = Vec::new();
    if matches {
        if qtype == 1 {
            // A: pointer to the question name at offset 12
            answer.extend_from_slice(&[0xC0, 0x0C]);
            answer.extend_from_slice(&1u16.to_be_bytes()); // type A
            answer.extend_from_slice(&1u16.to_be_bytes()); // class IN
            answer.extend_from_slice(&60u32.to_be_bytes()); // TTL
            answer.extend_from_slice(&4u16.to_be_bytes()); // rdlength
            answer.extend_from_slice(&bind_ip.octets());
            ancount = 1;
        }
        // AAAA and others: NOERROR with empty answer (fast fallback)
    } else {
        flags = 0x8183; // NXDOMAIN
    }
    let mut resp = Vec::with_capacity(qend + answer.len() + 16);
    resp.extend_from_slice(&id.to_be_bytes());
    resp.extend_from_slice(&flags.to_be_bytes());
    resp.extend_from_slice(&1u16.to_be_bytes()); // qdcount
    resp.extend_from_slice(&ancount.to_be_bytes());
    resp.extend_from_slice(&0u16.to_be_bytes()); // nscount
    resp.extend_from_slice(&0u16.to_be_bytes()); // arcount
    resp.extend_from_slice(&query[12..qend]);
    resp.extend_from_slice(&answer);
    Some(resp)
}

// ---------------------------------------------------------------- interaction log

fn record_interaction(
    state: &SharedProxy,
    protocol: &str,
    source: &str,
    subdomain_hint: &str,
    detail: serde_json::Value,
) {
    let domain = state.collab.domain.read().clone();
    let subdomain = first_label(subdomain_hint).to_string();
    if subdomain.is_empty() {
        return;
    }
    let db = state.db.read().clone();
    let Some(db) = db else { return };
    let payload_id = match db.list_collab_payloads() {
        Ok(list) => list
            .iter()
            .find(|p| matches_payload(&p.value, &subdomain, &domain))
            .map(|p| p.id),
        Err(_) => None,
    };
    let seen_at = chrono::Utc::now().to_rfc3339();
    let entry = match db.insert_collab_interaction(
        payload_id.as_ref(),
        &subdomain,
        protocol,
        source,
        &detail,
        &seen_at,
    ) {
        Ok(row) => row,
        Err(_) => return,
    };
    if let Some(app) = state.app_handle.read().clone() {
        use tauri::Emitter;
        let _ = app.emit("collab-interaction", &entry);
    }
}

// ---------------------------------------------------------------- servers

pub async fn start_collab(
    state: &SharedProxy,
    domain: &str,
    bind_ip: &str,
    dns_port: u16,
    http_port: u16,
) -> Result<(), String> {
    if state.collab.running.load(Ordering::SeqCst) {
        return Err("Collaborator server already running".into());
    }
    let ip: Ipv4Addr = bind_ip
        .parse()
        .map_err(|_| format!("invalid bind IP: {bind_ip}"))?;
    let domain = domain.trim().trim_end_matches('.').to_lowercase();
    if domain.is_empty() {
        return Err("domain must not be empty".into());
    }

    let udp = UdpSocket::bind((ip, dns_port))
        .await
        .map_err(|e| format!("DNS bind {ip}:{dns_port}: {e}"))?;
    let tcp = TcpListener::bind((ip, http_port))
        .await
        .map_err(|e| format!("HTTP bind {ip}:{http_port}: {e}"))?;
    let actual_dns = udp.local_addr().map_err(|e| e.to_string())?.port();
    let actual_http = tcp.local_addr().map_err(|e| e.to_string())?.port();

    *state.collab.domain.write() = domain;
    *state.collab.bind_ip.write() = ip;
    state.collab.dns_port.store(actual_dns, Ordering::SeqCst);
    state.collab.http_port.store(actual_http, Ordering::SeqCst);
    state.collab.running.store(true, Ordering::SeqCst);

    let (tx, mut rx) = tokio::sync::watch::channel(false);
    *state.collab.shutdown.lock() = Some(tx);

    let st = state.clone();
    tokio::spawn(async move {
        let dns = dns_loop(st.clone(), udp);
        let http = http_loop(st.clone(), tcp);
        tokio::select! {
            _ = rx.changed() => {}
            _ = dns => {}
            _ = http => {}
        }
        st.collab.running.store(false, Ordering::SeqCst);
    });
    Ok(())
}

pub fn stop_collab(state: &SharedProxy) {
    if let Some(tx) = state.collab.shutdown.lock().take() {
        let _ = tx.send(true);
    }
    state.collab.running.store(false, Ordering::SeqCst);
}

pub fn status_of(state: &SharedProxy) -> CollabStatus {
    CollabStatus {
        running: state.collab.running.load(Ordering::SeqCst),
        dns_port: state.collab.dns_port.load(Ordering::SeqCst),
        http_port: state.collab.http_port.load(Ordering::SeqCst),
        domain: state.collab.domain.read().clone(),
        bind_ip: state.collab.bind_ip.read().to_string(),
    }
}

async fn dns_loop(state: SharedProxy, socket: UdpSocket) {
    let mut buf = [0u8; 1500];
    loop {
        let (n, peer) = match socket.recv_from(&mut buf).await {
            Ok(v) => v,
            Err(_) => break,
        };
        let q = &buf[..n];
        if q.len() >= 12 {
            if let Some((name, qtype)) = parse_query_name(&q[12..]) {
                let sub = first_label(&name).to_string();
                record_interaction(
                    &state,
                    "dns",
                    &peer.ip().to_string(),
                    &sub,
                    serde_json::json!({ "name": name, "qtype": qtype }),
                );
            }
            let domain = state.collab.domain.read().clone();
            let bind_ip = *state.collab.bind_ip.read();
            if let Some(resp) = build_dns_response(q, &domain, bind_ip) {
                let _ = socket.send_to(&resp, peer).await;
            }
        }
    }
}

fn find_header_end(buf: &[u8]) -> Option<usize> {
    buf.windows(4).position(|w| w == b"\r\n\r\n")
}

async fn http_loop(state: SharedProxy, listener: TcpListener) {
    loop {
        match listener.accept().await {
            Ok((stream, peer)) => {
                let st = state.clone();
                tokio::spawn(async move {
                    let _ = handle_http_conn(st, stream, peer.to_string()).await;
                });
            }
            Err(_) => break,
        }
    }
}

async fn handle_http_conn(
    state: SharedProxy,
    mut stream: TcpStream,
    peer: String,
) -> std::io::Result<()> {
    let mut buf: Vec<u8> = Vec::with_capacity(2048);
    let mut tmp = [0u8; 1024];
    let header_end = loop {
        if let Some(p) = find_header_end(&buf) {
            break p;
        }
        let n = stream.read(&mut tmp).await?;
        if n == 0 {
            return Ok(());
        }
        buf.extend_from_slice(&tmp[..n]);
        if buf.len() > 64 * 1024 {
            return Ok(());
        }
    };
    let head = &buf[..header_end + 4];
    let mut header_storage = [httparse::EMPTY_HEADER; 32];
    let mut req = httparse::Request::new(&mut header_storage);
    if req.parse(head).is_err() {
        return Ok(());
    }
    let method = req.method.unwrap_or("?").to_string();
    let path = req.path.unwrap_or("/").to_string();
    let headers: Vec<(String, String)> = req
        .headers
        .iter()
        .map(|h| {
            (
                h.name.to_string(),
                String::from_utf8_lossy(h.value).to_string(),
            )
        })
        .collect();
    let content_len: usize = headers
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case("content-length"))
        .and_then(|(_, v)| v.parse().ok())
        .unwrap_or(0);
    let body_start = header_end + 4;
    let want = body_start + content_len.min(4096);
    while buf.len() < want {
        let n = stream.read(&mut tmp).await?;
        if n == 0 {
            break;
        }
        buf.extend_from_slice(&tmp[..n]);
        if buf.len() > 64 * 1024 {
            break;
        }
    }
    let body = String::from_utf8_lossy(&buf[body_start.min(buf.len())..buf.len().min(want)]).to_string();

    // hostname: Host header, else absolute-form request target
    let host_from_uri = path
        .strip_prefix("http://")
        .and_then(|r| r.split(['/', '?']).next())
        .map(|h| h.to_string());
    let host = headers
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case("host"))
        .map(|(_, v)| v.clone())
        .or(host_from_uri)
        .unwrap_or_default();
    // strip :port for label matching
    let host_name = host.split(':').next().unwrap_or("").to_string();
    let sub = first_label(&host_name).to_string();
    record_interaction(
        &state,
        "http",
        &peer,
        &sub,
        serde_json::json!({
            "method": method,
            "path": path,
            "headers": headers,
            "body": body,
        }),
    );

    let resp = b"HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: 3\r\nConnection: close\r\n\r\nok\n";
    let _ = stream.write_all(resp).await;
    let _ = stream.shutdown().await;
    Ok(())
}

// ---------------------------------------------------------------- tests

#[cfg(test)]
mod tests {
    use super::*;

    fn build_query(name: &str, qtype: u16) -> Vec<u8> {
        let mut q = vec![0x12, 0x34, 0x01, 0x00, 0x00, 0x01, 0, 0, 0, 0, 0, 0];
        for label in name.split('.') {
            q.push(label.len() as u8);
            q.extend_from_slice(label.as_bytes());
        }
        q.push(0);
        q.extend_from_slice(&qtype.to_be_bytes());
        q.extend_from_slice(&1u16.to_be_bytes()); // IN
        q
    }

    #[test]
    fn dns_a_response_roundtrip() {
        let q = build_query("abc12345.oob.wpt.local", 1);
        let ip = Ipv4Addr::new(127, 0, 0, 1);
        let resp = build_dns_response(&q, "oob.wpt.local", ip).expect("response");
        assert_eq!(&resp[0..2], &[0x12, 0x34]); // id echoed
        let flags = u16::from_be_bytes([resp[2], resp[3]]);
        assert_eq!(flags & 0x8000, 0x8000); // QR
        assert_eq!(flags & 0x000F, 0); // NOERROR
        assert_eq!(u16::from_be_bytes([resp[6], resp[7]]), 1); // ancount
        // answer rdata is the last 4 bytes
        assert_eq!(&resp[resp.len() - 4..], &ip.octets());
    }

    #[test]
    fn dns_nxdomain_for_other_domain() {
        let q = build_query("abc.evil.example", 1);
        let resp = build_dns_response(&q, "oob.wpt.local", Ipv4Addr::LOCALHOST).expect("response");
        let flags = u16::from_be_bytes([resp[2], resp[3]]);
        assert_eq!(flags & 0x000F, 3); // NXDOMAIN
        assert_eq!(u16::from_be_bytes([resp[6], resp[7]]), 0);
    }

    #[test]
    fn dns_aaaa_gets_empty_noerror() {
        let q = build_query("abc12345.oob.wpt.local", 28);
        let resp = build_dns_response(&q, "oob.wpt.local", Ipv4Addr::LOCALHOST).expect("response");
        let flags = u16::from_be_bytes([resp[2], resp[3]]);
        assert_eq!(flags & 0x000F, 0);
        assert_eq!(u16::from_be_bytes([resp[6], resp[7]]), 0);
    }

    #[test]
    fn payload_values_shape() {
        let sub = gen_subdomain();
        assert_eq!(sub.len(), 8);
        let host = format!("{sub}.oob.wpt.local");
        assert_eq!(make_payload_value("url", &host), format!("http://{host}/"));
        assert_eq!(make_payload_value("host", &host), host);
        assert!(make_payload_value("html", &host).contains(&host));
    }

    #[test]
    fn interaction_matches_by_first_label() {
        let host = "aabbccdd.oob.wpt.local";
        let value = make_payload_value("url", host);
        assert!(matches_payload(&value, "aabbccdd", "oob.wpt.local"));
        assert!(!matches_payload(&value, "ffffffff", "oob.wpt.local"));
        assert_eq!(first_label("aabbccdd.oob.wpt.local"), "aabbccdd");
    }
}
