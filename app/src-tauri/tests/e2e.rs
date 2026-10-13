use rustls::client::danger::ServerCertVerifier;
use rustls::client::WebPkiServerVerifier;
use rustls::pki_types::{ServerName, UnixTime};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use webproxy_toolkit_lib::{ca, commands, db, proxy};

fn init_crypto() {
    let _ = rustls::crypto::ring::default_provider().install_default();
}

async fn boot(name: &str) -> proxy::SharedProxy {
    let state = proxy::SharedProxy::default();
    let dir = std::env::temp_dir().join(format!("wpt_e2e_{}_{}", std::process::id(), name));
    let _ = std::fs::remove_dir_all(&dir);
    let ca = ca::load_or_create(&dir).expect("ca load");
    *state.ca.write() = Some(ca);
    proxy::start_proxy(state.clone(), "127.0.0.1", 0)
        .await
        .expect("proxy start");
    for _ in 0..50 {
        if state.port.load(Ordering::SeqCst) != 0 {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    state
}

async fn send_raw(addr: &str, payload: &str) -> String {
    let mut stream = tokio::net::TcpStream::connect(addr).await.expect("connect proxy");
    tokio::io::AsyncWriteExt::write_all(&mut stream, payload.as_bytes())
        .await
        .unwrap();
    let mut buf = Vec::new();
    let mut tmp = [0u8; 8192];
    loop {
        let n = tokio::io::AsyncReadExt::read(&mut stream, &mut tmp)
            .await
            .unwrap();
        if n == 0 {
            break;
        }
        buf.extend_from_slice(&tmp[..n]);
        if buf.windows(4).any(|w| w == b"\r\n\r\n") && buf.len() > 64 {
            break;
        }
    }
    String::from_utf8_lossy(&buf).to_string()
}

#[tokio::test]
async fn plain_http_through_proxy() {
    init_crypto();
    let state = boot("plain").await;
    let port = state.port.load(Ordering::SeqCst);
    let resp = send_raw(
        &format!("127.0.0.1:{port}"),
        "GET http://example.com/ HTTP/1.1\r\nHost: example.com\r\nConnection: close\r\n\r\n",
    )
    .await;
    assert!(
        resp.starts_with("HTTP/1.1 200") || resp.starts_with("HTTP/1.1 301"),
        "unexpected response: {}",
        &resp[..resp.len().min(200)]
    );
    assert_eq!(state.history.lock().len(), 1, "exchange should be recorded");
    proxy::stop_proxy(&state);
}

#[tokio::test]
async fn https_mitm_chain_verifies() {
    init_crypto();
    let state = boot("https").await;
    let port = state.port.load(Ordering::SeqCst);
    let signing_ca = state.ca.read().clone().expect("state ca");

    // CONNECT
    let mut stream = tokio::net::TcpStream::connect(("127.0.0.1", port))
        .await
        .expect("connect");
    tokio::io::AsyncWriteExt::write_all(
        &mut stream,
        b"CONNECT example.com:443 HTTP/1.1\r\nHost: example.com:443\r\n\r\n",
    )
    .await
    .unwrap();
    let mut buf = [0u8; 256];
    let n = tokio::io::AsyncReadExt::read(&mut stream, &mut buf)
        .await
        .unwrap();
    let head = String::from_utf8_lossy(&buf[..n]).to_string();
    assert!(
        head.starts_with("HTTP/1.1 200"),
        "CONNECT failed: {head:?}"
    );

    // TLS handshake: verify leaf against the SAME CA instance that signs
    let mut roots = rustls::RootCertStore::empty();
    roots
        .add(signing_ca.cert.der().clone())
        .expect("add ca to roots");
    let client_cfg = rustls::ClientConfig::builder()
        .with_root_certificates(roots)
        .with_no_client_auth();
    let connector = tokio_rustls::TlsConnector::from(std::sync::Arc::new(client_cfg));
    let server_name = ServerName::try_from("example.com".to_string()).unwrap();
    let mut tls = connector
        .connect(server_name, stream)
        .await
        .expect("TLS handshake with CA verification");

    tokio::io::AsyncWriteExt::write_all(
        &mut tls,
        b"GET / HTTP/1.1\r\nHost: example.com\r\nConnection: close\r\n\r\n",
    )
    .await
    .unwrap();
    let mut resp = Vec::new();
    let mut tmp = [0u8; 8192];
    loop {
        let n = tokio::io::AsyncReadExt::read(&mut tls, &mut tmp).await;
        match n {
            Ok(0) | Err(_) => break,
            Ok(n) => {
                resp.extend_from_slice(&tmp[..n]);
                if resp.len() > 64 && resp.windows(4).any(|w| w == b"\r\n\r\n") {
                    break;
                }
            }
        }
    }
    let text = String::from_utf8_lossy(&resp).to_string();
    assert!(
        text.starts_with("HTTP/1.1"),
        "no HTTP response over MITM: {text:?}"
    );
    assert_eq!(state.history.lock().len(), 1, "https exchange recorded");
    proxy::stop_proxy(&state);
}

// Restart path: app run #2 reloads ca.crt/ca.key from disk, signs a leaf;
// the leaf must verify against the ORIGINAL cert bytes (= what curl reads
// from ca.crt on disk).
#[test]
fn reload_path_signs_valid_leaf() {
    init_crypto();
    let dir = std::env::temp_dir().join(format!("wpt_e2e_{}_reload", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);

    let original = ca::load_or_create(&dir).expect("fresh ca");
    let reloaded = ca::load_or_create(&dir).expect("reload ca");

    let (leaf, _leaf_key) = ca::mint_leaf(&reloaded, "example.com").expect("mint");

    let mut roots = rustls::RootCertStore::empty();
    roots
        .add(original.cert.der().clone())
        .expect("add original ca");

    let verifier = WebPkiServerVerifier::builder_with_provider(
        roots.into(),
        std::sync::Arc::new(rustls::crypto::ring::default_provider()),
    )
    .build()
    .expect("verifier");

    verifier
        .verify_server_cert(
            leaf.der(),
            &[],
            &ServerName::try_from("example.com").unwrap(),
            &[],
            UnixTime::now(),
        )
        .expect("leaf signed by reloaded CA must verify against on-disk CA cert");
}

// -------------------------------------------------------------- intruder e2e

struct EchoServer {
    port: u16,
    hits: Arc<AtomicUsize>,
    handle: tokio::task::JoinHandle<()>,
}

/// Minimal HTTP server: counts requests, replies 200 PONG, closes the socket.
async fn echo_server() -> EchoServer {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind echo server");
    let port = listener.local_addr().expect("addr").port();
    let hits = Arc::new(AtomicUsize::new(0));
    let h = hits.clone();
    let handle = tokio::spawn(async move {
        loop {
            let Ok((mut sock, _)) = listener.accept().await else {
                break;
            };
            let h = h.clone();
            tokio::spawn(async move {
                let mut buf = vec![0u8; 8192];
                let _ = tokio::io::AsyncReadExt::read(&mut sock, &mut buf).await;
                h.fetch_add(1, Ordering::SeqCst);
                let body = "PONG";
                let resp = format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nContent-Type: text/plain\r\nConnection: close\r\n\r\n{}",
                    body.len(),
                    body
                );
                let _ = tokio::io::AsyncWriteExt::write_all(&mut sock, resp.as_bytes()).await;
                let _ = tokio::io::AsyncWriteExt::shutdown(&mut sock).await;
            });
        }
    });
    EchoServer { port, hits, handle }
}

fn intruder_state(name: &str) -> proxy::SharedProxy {
    let state = proxy::SharedProxy::default();
    let path = std::env::temp_dir().join(format!(
        "wpt_intruder_{}_{}.db",
        std::process::id(),
        name
    ));
    let _ = std::fs::remove_file(&path);
    let database = db::open(&path).expect("open intruder db");
    *state.db.write() = Some(database);
    state
}

fn make_campaign(
    state: &proxy::SharedProxy,
    name: &str,
    raw: &str,
    items: &str,
    threads: i64,
    delay_ms: i64,
) -> i64 {
    let database = state.db.read().clone().expect("db open");
    let cid = database
        .create_intruder_campaign(name, "sniper", raw, None)
        .expect("campaign");
    database
        .save_intruder_payload_set(cid, 0, "set 1", "list", items, None, None, None, "[]")
        .expect("payload set");
    let mut c = database
        .get_intruder_campaign(cid)
        .expect("get")
        .expect("exists");
    c.threads = threads;
    c.delay_ms = delay_ms;
    database.update_intruder_campaign(&c).expect("update");
    cid
}

async fn wait_for_attack_end(state: &proxy::SharedProxy, cid: i64) {
    for _ in 0..500 {
        if !state.intruder_runs.lock().contains_key(&cid) {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    panic!("attack did not finish within 10s");
}

#[tokio::test]
async fn intruder_sniper_attack_completes_against_local_server() {
    let server = echo_server().await;
    let state = intruder_state("sniper");
    let raw = format!(
        "GET http://127.0.0.1:{p}/attack?x=\u{a7}alpha\u{a7} HTTP/1.1\r\nHost: 127.0.0.1:{p}\r\nConnection: close\r\n\r\n",
        p = server.port
    );
    let cid = make_campaign(&state, "sniper", &raw, "one\ntwo\nthree", 2, 0);

    let snap = commands::start_intruder_attack(None, &state, cid)
        .await
        .expect("start attack");
    assert_eq!(snap["total"], 3, "3 payloads, 1 position");

    wait_for_attack_end(&state, cid).await;

    let database = state.db.read().clone().expect("db open");
    assert_eq!(database.count_intruder_results(cid).expect("count"), 3);
    assert_eq!(server.hits.load(Ordering::SeqCst), 3, "one hit per payload");

    let rows = database.list_intruder_results(cid, 0, 10).expect("rows");
    let combos: Vec<String> = rows.iter().map(|r| r.combo.clone()).collect();
    for payload in ["one", "two", "three"] {
        assert!(
            combos.iter().any(|c| c.contains(payload)),
            "missing payload {payload} in {combos:?}"
        );
        assert!(
            rows.iter().any(|r| r.status == Some(200)),
            "every result should be 200"
        );
    }
    let full = database
        .get_intruder_result(cid, 0)
        .expect("detail")
        .expect("row 0");
    assert!(full.req_raw.contains("one") || full.req_raw.contains("two"));
    assert!(
        full.resp_raw.unwrap_or_default().contains("PONG"),
        "response should be stored"
    );

    let c = database
        .get_intruder_campaign(cid)
        .expect("get")
        .expect("exists");
    assert_eq!(c.status, "done", "campaign marked done");
    server.handle.abort();
}

#[tokio::test]
async fn intruder_stop_mid_attack_leaves_stopped_status() {
    let server = echo_server().await;
    let state = intruder_state("stop");
    let items = (1..=50)
        .map(|i| i.to_string())
        .collect::<Vec<_>>()
        .join("\n");
    let raw = format!(
        "GET http://127.0.0.1:{p}/attack?x=\u{a7}1\u{a7} HTTP/1.1\r\nHost: 127.0.0.1:{p}\r\nConnection: close\r\n\r\n",
        p = server.port
    );
    let cid = make_campaign(&state, "stop", &raw, &items, 1, 30);

    commands::start_intruder_attack(None, &state, cid)
        .await
        .expect("start attack");
    tokio::time::sleep(std::time::Duration::from_millis(350)).await;
    {
        let runs = state.intruder_runs.lock();
        let ctl = runs.get(&cid).expect("attack should be running");
        ctl.cancel.store(true, Ordering::SeqCst);
    }

    wait_for_attack_end(&state, cid).await;

    let database = state.db.read().clone().expect("db open");
    let total = database.count_intruder_results(cid).expect("count");
    assert!(
        total >= 1 && total < 50,
        "stopped early with {total}/50 results"
    );
    let c = database
        .get_intruder_campaign(cid)
        .expect("get")
        .expect("exists");
    assert_eq!(c.status, "stopped", "campaign marked stopped");
    server.handle.abort();
}
