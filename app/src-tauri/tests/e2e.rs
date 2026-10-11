use rustls::client::danger::ServerCertVerifier;
use rustls::client::WebPkiServerVerifier;
use rustls::pki_types::{ServerName, UnixTime};
use std::sync::atomic::Ordering;
use webproxy_toolkit_lib::{ca, proxy};

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
