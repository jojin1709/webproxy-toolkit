//! Scanner: issue catalog + passive check engine (Phase 4a).
//!
//! Passive checks are pure functions over one proxied exchange. Each returns
//! `Some(Finding)` when it fires; `passive_scan` maps findings through the
//! catalog and the caller inserts them (dedupe lives in `db::insert_issue`).

use crate::db::NewIssue;
use crate::proxy::HttpExchange;

/// Max response-body bytes a passive check may inspect (larger bodies are truncated).
const BODY_CAP: usize = 256 * 1024;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum CheckKind {
    Passive,
    Active,
}

#[derive(Clone, Copy)]
pub struct CheckDef {
    pub id: &'static str,
    pub name: &'static str,
    pub kind: CheckKind,
    pub severity: &'static str,
    pub description: &'static str,
    pub remediation: &'static str,
}

/// Issue definitions catalog — the single source of truth for check metadata.
/// Passive checks (Phase 4a) are evaluated on every in-scope proxied exchange.
pub const CHECKS: &[CheckDef] = &[
    CheckDef {
        id: "missing-csp",
        name: "Missing Content-Security-Policy",
        kind: CheckKind::Passive,
        severity: "info",
        description: "The response does not include a Content-Security-Policy header, so the browser cannot restrict which scripts, styles and other resources the page may load.",
        remediation: "Add a Content-Security-Policy response header with a policy appropriate for the application (start with report-only to observe breakage).",
    },
    CheckDef {
        id: "missing-hsts",
        name: "Missing HTTP Strict Transport Security",
        kind: CheckKind::Passive,
        severity: "low",
        description: "The HTTPS response omits Strict-Transport-Security, so browsers that have previously visited over HTTP can still be downgraded.",
        remediation: "Add Strict-Transport-Security: max-age=31536000; includeSubDomains on all HTTPS responses.",
    },
    CheckDef {
        id: "missing-xfo",
        name: "Missing X-Frame-Options",
        kind: CheckKind::Passive,
        severity: "info",
        description: "The response omits X-Frame-Options, so the page can be framed by third-party sites and may be exposed to clickjacking.",
        remediation: "Add X-Frame-Options: DENY (or SAMEORIGIN) — or an equivalent frame-ancestors directive in Content-Security-Policy.",
    },
    CheckDef {
        id: "missing-xcto",
        name: "Missing X-Content-Type-Options",
        kind: CheckKind::Passive,
        severity: "info",
        description: "The response omits X-Content-Type-Options: nosniff, so the browser may MIME-sniff responses and treat them as executable content.",
        remediation: "Add X-Content-Type-Options: nosniff to all responses.",
    },
    CheckDef {
        id: "missing-referrer-policy",
        name: "Missing Referrer-Policy",
        kind: CheckKind::Passive,
        severity: "info",
        description: "The response omits Referrer-Policy, so full URLs (including query strings) may leak to third-party origins via the Referer header.",
        remediation: "Add Referrer-Policy: strict-origin-when-cross-origin (or stricter) to all responses.",
    },
    CheckDef {
        id: "cookie-no-httponly",
        name: "Cookie without HttpOnly flag",
        kind: CheckKind::Passive,
        severity: "medium",
        description: "A cookie is set without the HttpOnly flag, so it is accessible to client-side JavaScript and can be stolen by XSS.",
        remediation: "Add the HttpOnly flag to all session and authentication cookies.",
    },
    CheckDef {
        id: "cookie-no-secure",
        name: "Cookie without Secure flag",
        kind: CheckKind::Passive,
        severity: "medium",
        description: "A cookie is set over HTTPS without the Secure flag, so the browser may also send it over unencrypted HTTP connections.",
        remediation: "Add the Secure flag to all cookies set on HTTPS responses.",
    },
    CheckDef {
        id: "server-banner",
        name: "Server version disclosure",
        kind: CheckKind::Passive,
        severity: "info",
        description: "The Server response header discloses the web server software and version, which helps attackers match known vulnerabilities.",
        remediation: "Suppress or genericise the Server header (remove version numbers) at the server or reverse proxy.",
    },
    CheckDef {
        id: "x-powered-by",
        name: "X-Powered-By disclosure",
        kind: CheckKind::Passive,
        severity: "info",
        description: "An X-Powered-By response header discloses the application framework or runtime.",
        remediation: "Remove X-Powered-By (and similar technology-identifying) headers.",
    },
    CheckDef {
        id: "directory-listing",
        name: "Directory listing enabled",
        kind: CheckKind::Passive,
        severity: "low",
        description: "The response body looks like a web-server directory listing ('Index of /'), exposing file names and structure.",
        remediation: "Disable automatic directory listing and serve an explicit index page instead.",
    },
    CheckDef {
        id: "sql-error-leak",
        name: "SQL error message disclosed",
        kind: CheckKind::Passive,
        severity: "high",
        description: "The response contains a database error message, indicating that user input may be reaching a SQL interpreter and disclosing backend details.",
        remediation: "Use parameterised queries / prepared statements and return generic error pages to users while logging details server-side.",
    },
    CheckDef {
        id: "stack-trace-leak",
        name: "Stack trace disclosed",
        kind: CheckKind::Passive,
        severity: "medium",
        description: "The response body contains an application stack trace or exception dump, disclosing internal file paths and code structure.",
        remediation: "Disable debug-mode error pages in production and return generic error responses.",
    },
    CheckDef {
        id: "email-disclosure",
        name: "Email address disclosed",
        kind: CheckKind::Passive,
        severity: "info",
        description: "The response body contains one or more email addresses, which may aid phishing or social-engineering targeting.",
        remediation: "Review whether the addresses are intended to be public; remove or obfuscate them if not.",
    },
    CheckDef {
        id: "internal-ip-disclosure",
        name: "Internal IP address disclosed",
        kind: CheckKind::Passive,
        severity: "info",
        description: "The response body references a private (RFC 1918) IP address, disclosing internal network topology.",
        remediation: "Remove internal IP references from public responses.",
    },
    CheckDef {
        id: "mixed-content",
        name: "Mixed content",
        kind: CheckKind::Passive,
        severity: "medium",
        description: "An HTTPS page references subresources over plain HTTP, so those requests can be intercepted or tampered with.",
        remediation: "Serve all subresources over HTTPS (or protocol-relative where safe) and consider a CSP upgrade-insecure-requests directive.",
    },
    CheckDef {
        id: "basic-auth-plaintext",
        name: "Basic authentication over HTTP",
        kind: CheckKind::Passive,
        severity: "high",
        description: "Credentials are sent via an Authorization: Basic header over an unencrypted HTTP connection and can be trivially decoded.",
        remediation: "Require HTTPS for any endpoint using Basic authentication.",
    },
    CheckDef {
        id: "password-in-url",
        name: "Password in URL",
        kind: CheckKind::Passive,
        severity: "high",
        description: "A password-like value appears in the request URL query string; URLs are logged in server logs, browser history and proxy caches.",
        remediation: "Move credentials into the request body (POST) over HTTPS; never place them in the URL.",
    },
    // --- active checks (Phase 4b) ---------------------------------------
    CheckDef {
        id: "sqli-error",
        name: "SQL injection (error-based)",
        kind: CheckKind::Active,
        severity: "high",
        description: "A SQL metacharacter injected into a parameter produced a database error signature in the response, indicating a possible injection point.",
        remediation: "Use parameterised queries / prepared statements for all database access; validate and encode user input.",
    },
    CheckDef {
        id: "xss-reflected",
        name: "Reflected cross-site scripting",
        kind: CheckKind::Active,
        severity: "high",
        description: "A canary string injected into a parameter was reflected in the response unencoded, so an attacker-controlled script could execute in a victim's browser.",
        remediation: "Context-encode all untrusted data at output (HTML entity, JS, URL, CSS context as appropriate); deploy a CSP as defence in depth.",
    },
    CheckDef {
        id: "path-traversal",
        name: "Path traversal",
        kind: CheckKind::Active,
        severity: "high",
        description: "A ../-style payload in a parameter caused the server to return contents of a sensitive system file (/etc/passwd or win.ini), indicating a directory traversal flaw.",
        remediation: "Canonicalise and validate paths server-side; never build filesystem paths directly from user input; run the service with least privilege.",
    },
    CheckDef {
        id: "open-redirect",
        name: "Open redirect",
        kind: CheckKind::Active,
        severity: "medium",
        description: "A parameter controlling a redirect accepted an attacker-controlled absolute URL, so the endpoint can be used to bounce victims to arbitrary sites (phishing).",
        remediation: "Validate redirect targets against an allow-list of paths/hosts; never redirect to a full URL supplied in user input.",
    },
    CheckDef {
        id: "sensitive-files",
        name: "Sensitive file or directory exposed",
        kind: CheckKind::Active,
        severity: "info",
        description: "A well-known sensitive path (e.g. /.git/config, /.env, /backup.zip) returned HTTP 200, exposing configuration or backups.",
        remediation: "Block access to VCS metadata, env files and backup archives at the web server / WAF layer.",
    },
];

pub fn check_def(id: &str) -> Option<&'static CheckDef> {
    CHECKS.iter().find(|c| c.id == id)
}

pub fn passive_check_ids() -> Vec<&'static str> {
    CHECKS
        .iter()
        .filter(|c| c.kind == CheckKind::Passive)
        .map(|c| c.id)
        .collect()
}

/// A raw finding from one check, before catalog metadata is attached.
struct Finding {
    check_id: &'static str,
    confidence: &'static str,
    param: String,
    evidence: String,
}

fn header<'a>(headers: &'a [(String, String)], name: &str) -> Option<&'a str> {
    headers
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case(name))
        .map(|(_, v)| v.as_str())
}

fn has_header(headers: &[(String, String)], name: &str) -> bool {
    header(headers, name).is_some()
}

fn sniff(body: &str) -> &str {
    if body.len() > BODY_CAP {
        &body[..BODY_CAP]
    } else {
        body
    }
}

/// Query-string parameter names and values from an absolute URL: `name=value` pairs.
fn query_pairs(url: &str) -> Vec<(String, String)> {
    let Some(q) = url.split_once('?').map(|(_, q)| q) else {
        return Vec::new();
    };
    let q = q.split('#').next().unwrap_or(q);
    q.split('&')
        .filter(|p| !p.is_empty())
        .map(|p| {
            let (k, v) = p.split_once('=').unwrap_or((p, ""));
            (k.to_string(), v.to_string())
        })
        .collect()
}

/// `path` component of an absolute URL (no query), falling back to "/".
fn url_path(url: &str) -> String {
    let rest = url
        .split_once("://")
        .map(|(_, r)| r)
        .unwrap_or(url);
    let path = match rest.find(['/', '?']) {
        Some(i) => &rest[i..],
        None => "/",
    };
    let path = path.split('?').next().unwrap_or(path);
    if path.is_empty() { "/".into() } else { path.to_string() }
}

fn is_https(url: &str) -> bool {
    url.starts_with("https://")
}

/// Evidence snippet of at most `max` chars around the first match of `needle`.
fn snippet_around(body: &str, needle: &str, max: usize) -> String {
    let lower = body.to_lowercase();
    let pos = lower.find(&needle.to_lowercase()).unwrap_or(0);
    let start = pos.saturating_sub(20);
    let end = (pos + needle.len() + 40).min(body.len());
    let cut = &body[start..end];
    let cut = cut.trim_matches(|c: char| c.is_control() && c != '\n');
    let mut s = cut.to_string();
    if start > 0 {
        s = format!("…{s}");
    }
    if end < body.len() {
        s.push('…');
    }
    if s.len() > max {
        s.truncate(max);
        s.push('…');
    }
    s
}

const SQL_SIGNS: &[&str] = &[
    "mysql_fetch",
    "mysql_num_rows",
    "you have an error in your sql syntax",
    "warning: mysql",
    "unclosed quotation mark",
    "microsoft ole db provider for sql server",
    "odbc sql server driver",
    "pg_query()",
    "pg_exec()",
    "postgresql query failed",
    "sqlite3::exception",
    "sqlite error",
    "sqlstate[",
    "ora-00933",
    "ora-01756",
    "sql command not properly ended",
    "quoted string not properly terminated",
    "syntax error at line",
];

const STACK_SIGNS: &[&str] = &[
    "at java.",
    "exception in thread",
    "traceback (most recent call last)",
    "php fatal error",
    "php warning:",
    "system.web.httpexception",
    "unhandled exception",
    "stack trace:",
    ".Exception: ",
    "java.lang.NullPointerException",
    "org.springframework",
];

/// Run every passive check against one exchange; returns issue drafts.
pub fn passive_scan(ex: &HttpExchange) -> Vec<NewIssue> {
    if ex.status.is_none() {
        return Vec::new(); // transport error — nothing to inspect
    }
    let https = is_https(&ex.url);
    let body = ex.resp_body.as_deref().unwrap_or("");
    let body = sniff(body);
    let path = url_path(&ex.url);
    let mut out: Vec<Finding> = Vec::new();
    let mut push = |check_id: &'static str, confidence: &'static str, param: &str, evidence: String| {
        out.push(Finding {
            check_id,
            confidence,
            param: param.to_string(),
            evidence,
        });
    };

    // --- response security headers -------------------------------------
    if !has_header(&ex.resp_headers, "content-security-policy") {
        push("missing-csp", "certain", "", "no Content-Security-Policy header".into());
    }
    if https && !has_header(&ex.resp_headers, "strict-transport-security") {
        push("missing-hsts", "certain", "", "no Strict-Transport-Security header".into());
    }
    if !has_header(&ex.resp_headers, "x-frame-options")
        && !ex
            .resp_headers
            .iter()
            .filter(|(k, _)| k.eq_ignore_ascii_case("content-security-policy"))
            .any(|(_, v)| v.to_lowercase().contains("frame-ancestors"))
    {
        push("missing-xfo", "certain", "", "no X-Frame-Options or frame-ancestors CSP".into());
    }
    if !has_header(&ex.resp_headers, "x-content-type-options") {
        push("missing-xcto", "certain", "", "no X-Content-Type-Options header".into());
    }
    if !has_header(&ex.resp_headers, "referrer-policy") {
        push("missing-referrer-policy", "certain", "", "no Referrer-Policy header".into());
    }

    // --- cookies --------------------------------------------------------
    for (k, v) in &ex.resp_headers {
        if !k.eq_ignore_ascii_case("set-cookie") {
            continue;
        }
        let name = v.split('=').next().unwrap_or("").to_string();
        let flags = v.to_lowercase();
        if !flags.contains("httponly") {
            push("cookie-no-httponly", "firm", &name, snippet_around(v, &name, 120));
        }
        if https && !flags.contains("secure") {
            push("cookie-no-secure", "firm", &name, snippet_around(v, &name, 120));
        }
    }

    // --- technology disclosure ------------------------------------------
    if let Some(server) = header(&ex.resp_headers, "server") {
        // version = a digit adjacent to another digit or a dot (e.g. "nginx/1.24.0", "Apache/2.4.57")
        let versioned = server
            .char_indices()
            .any(|(i, c)| c.is_ascii_digit() && (server[i..].starts_with(|d: char| d.is_ascii_digit() || d == '.') || (i > 0 && server.as_bytes()[i - 1] == b'/')));
        if versioned {
            push("server-banner", "certain", "", server.to_string());
        }
    }
    if let Some(powered) = header(&ex.resp_headers, "x-powered-by") {
        if !powered.trim().is_empty() {
            push("x-powered-by", "certain", "", powered.to_string());
        }
    }

    // --- body content ---------------------------------------------------
    let body_lower = body.to_lowercase();
    if body_lower.contains("index of /") || body_lower.contains("<title>index of /") {
        let ev = snippet_around(body, "index of /", 100);
        push("directory-listing", "firm", "", ev);
    }
    for sign in SQL_SIGNS {
        if body_lower.contains(sign) {
            let ev = snippet_around(body, sign, 120);
            push("sql-error-leak", "firm", "", ev);
            break;
        }
    }
    for sign in STACK_SIGNS {
        if body_lower.contains(&sign.to_lowercase()) {
            let ev = snippet_around(body, sign, 120);
            push("stack-trace-leak", "firm", "", ev);
            break;
        }
    }
    if let Some(ev) = first_email(body) {
        push("email-disclosure", "tentative", "", ev);
    }
    if let Some(ip) = first_private_ip(body) {
        push("internal-ip-disclosure", "tentative", "", ip);
    }
    if https {
        // subresource references over plain http://
        for (attr, _) in body_lower.match_indices("http://") {
            // skip "http://" inside the request's own absolute URL echoed back in comments is fine;
            // require it to sit inside an href/src/action-like attribute to cut noise
            let window_start = attr.saturating_sub(80);
            let window = &body_lower[window_start..attr.min(body_lower.len())];
            if window.contains("href=") || window.contains("src=") || window.contains("action=") || window.contains("url(") {
                let ev = snippet_around(body, "http://", 100);
                push("mixed-content", "firm", "", ev);
                break;
            }
        }
    }

    // --- request-side checks --------------------------------------------
    if !https {
        if let Some(auth) = header(&ex.req_headers, "authorization") {
            if auth.trim_start().to_lowercase().starts_with("basic ") {
                push("basic-auth-plaintext", "certain", "Authorization", "Authorization: Basic over HTTP".into());
            }
        }
    }
    for (k, v) in query_pairs(&ex.url) {
        let kl = k.to_lowercase();
        if kl.contains("pass") || kl.contains("pwd") || kl == "secret" || kl == "token" {
            if !v.trim_matches('"').is_empty() {
                push("password-in-url", "firm", &k, format!("{k}=[redacted value in URL query]"));
            }
        }
    }

    // --- map findings through the catalog --------------------------------
    out.into_iter()
        .filter_map(|f| {
            let def = check_def(f.check_id)?;
            Some(NewIssue {
                check_id: def.id.to_string(),
                name: def.name.to_string(),
                severity: def.severity.to_string(),
                confidence: f.confidence.to_string(),
                host: ex.host.clone(),
                url: ex.url.clone(),
                method: ex.method.clone(),
                path: path.clone(),
                param: f.param,
                description: def.description.to_string(),
                remediation: def.remediation.to_string(),
                evidence: f.evidence,
                exchange_id: Some(ex.id as i64),
            })
        })
        .collect()
}

fn first_email(body: &str) -> Option<String> {
    // simple pattern: word@word.tld — avoid pulling from obvious binary noise
    let bytes = body.as_bytes();
    let mut i = 0;
    while i + 5 < bytes.len() {
        if bytes[i].is_ascii_alphabetic() {
            if let Some(at) = body[i..].find('@') {
                let start = i;
                let at_abs = i + at;
                // walk back to token start
                let mut s = at_abs;
                while s > start && (bytes[s - 1].is_ascii_alphanumeric() || bytes[s - 1] == b'.' || bytes[s - 1] == b'_' || bytes[s - 1] == b'-') {
                    s -= 1;
                }
                // walk forward to token end
                let mut e = at_abs + 1;
                while e < bytes.len() && (bytes[e].is_ascii_alphanumeric() || bytes[e] == b'.' || bytes[e] == b'-') {
                    e += 1;
                }
                let candidate = &body[s..e];
                if candidate.contains('.')
                    && !candidate.ends_with('.')
                    && candidate.len() >= 5
                    && !candidate.contains("..")
                {
                    return Some(snippet_around(body, candidate, 80));
                }
                i = (at_abs + 1).max(i + 1);
                continue;
            }
        }
        i += 1;
    }
    None
}

fn first_private_ip(body: &str) -> Option<String> {
    // dotted-quad candidates; classify first RFC 1918 match
    let bytes = body.as_bytes();
    for start in 0..bytes.len() {
        if !bytes[start].is_ascii_digit() {
            continue;
        }
        // must begin a token (preceded by non-digit, non-dot)
        if start > 0 && (bytes[start - 1].is_ascii_digit() || bytes[start - 1] == b'.') {
            continue;
        }
        // scan up to 15 chars for a dotted quad
        let end = (start + 15).min(bytes.len());
        let cand = &body[start..end];
        let mut parts = [0u16; 4];
        let mut n_parts = 0usize;
        let mut val: Option<u16> = None;
        let mut ok = true;
        for c in cand.chars() {
            if c.is_ascii_digit() {
                let d = c as u16 - '0' as u16;
                val = Some(val.unwrap_or(0) * 10 + d);
                if val.unwrap() > 255 {
                    ok = false;
                    break;
                }
            } else if c == '.' {
                match val.take() {
                    Some(v) if n_parts < 4 => {
                        parts[n_parts] = v;
                        n_parts += 1;
                    }
                    _ => {
                        ok = false;
                        break;
                    }
                }
            } else {
                break;
            }
        }
        if !ok {
            continue;
        }
        // close the final octet only if followed by a non-digit (a real boundary)
        if let Some(v) = val.take() {
            if n_parts < 4 {
                parts[n_parts] = v;
                n_parts += 1;
            }
        }
        if n_parts != 4 {
            continue;
        }
        // verify the char right after the quad is a boundary, not a digit
        let quad_len = format!("{}.{}.{}.{}", parts[0], parts[1], parts[2], parts[3]).len();
        let after = start + quad_len;
        if after < bytes.len() && bytes[after].is_ascii_digit() {
            continue;
        }
        let private = parts[0] == 10
            || (parts[0] == 192 && parts[1] == 168)
            || (parts[0] == 172 && (16..=31).contains(&parts[1]));
        if private {
            return Some(format!("{}.{}.{}.{}", parts[0], parts[1], parts[2], parts[3]));
        }
    }
    None
}

/// Insert new issues from a passive scan, skipping duplicates; returns new ids.
/// Emits `issue-added` and writes a scanner log line per new issue when an app handle is given.
pub fn passive_scan_and_record(
    app: Option<&tauri::AppHandle>,
    state: &crate::proxy::SharedProxy,
    ex: &HttpExchange,
) -> Vec<i64> {
    use tauri::Emitter;
    let Some(db) = state.db.read().clone() else {
        return Vec::new();
    };
    let drafts = passive_scan(ex);
    let mut ids = Vec::new();
    for draft in drafts {
        match db.insert_issue(&draft) {
            Ok(Some(id)) => {
                ids.push(id);
                if let Ok(issue) = db.get_issue(id) {
                    if let Some(issue) = issue {
                        if let Some(app) = app {
                            let _ = app.emit("issue-added", issue.clone());
                            if issue.severity == "high" || issue.severity == "critical" {
                                crate::commands::log_event(
                                    app,
                                    state,
                                    "scanner",
                                    "warn",
                                    &format!("[{}] {} on {}{}", issue.name, issue.severity, issue.host, issue.path),
                                );
                            }
                        }
                    }
                }
            }
            Ok(None) => {} // duplicate
            Err(e) => eprintln!("issue insert failed: {e}"),
        }
    }
    ids
}

// ------------------------------------------------------- scan task engine (4b)

use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use parking_lot::Mutex;

/// Cooperative control for one running scan task.
pub struct ScanCtl {
    pub cancel: AtomicBool,
    pub paused: AtomicBool,
    pub pages: AtomicU64,
    pub requests: AtomicU64,
    pub issues: AtomicU64,
    pub started: Instant,
}

impl ScanCtl {
    pub fn new() -> Self {
        ScanCtl {
            cancel: AtomicBool::new(false),
            paused: AtomicBool::new(false),
            pages: AtomicU64::new(0),
            requests: AtomicU64::new(0),
            issues: AtomicU64::new(0),
            started: Instant::now(),
        }
    }
    pub fn cancelled(&self) -> bool {
        self.cancel.load(Ordering::SeqCst)
    }
    pub fn pause(&self) {
        self.paused.store(true, Ordering::SeqCst);
    }
    pub fn resume(&self) {
        self.paused.store(false, Ordering::SeqCst);
    }
    fn wait_if_paused(&self) {
        while self.paused.load(Ordering::SeqCst) && !self.cancelled() {
            std::thread::sleep(Duration::from_millis(100));
        }
    }
}

pub type ScanRuns = Mutex<HashMap<i64, Arc<ScanCtl>>>;

/// Limits for one scan run; defaults match the plan.
#[derive(Clone)]
pub struct ScanLimits {
    pub max_pages: usize,
    pub max_depth: usize,
    pub throttle_ms: u64,
    pub active: bool,
    /// Restrict active checks to these ids; None = run all built-ins.
    pub only_checks: Option<Vec<String>>,
}

impl Default for ScanLimits {
    fn default() -> Self {
        ScanLimits {
            max_pages: 50,
            max_depth: 3,
            throttle_ms: 50,
            active: true,
            only_checks: None,
        }
    }
}

/// A discovered link / insertion point during crawl.
#[derive(Clone, Debug)]
pub struct CrawlTarget {
    pub url: String,
    pub depth: usize,
}

/// Extract same-origin hrefs / srcs / actions from an HTML body.
pub fn extract_links(base: &str, body: &str) -> Vec<String> {
    let Ok(base_url) = url::Url::parse(base) else {
        return Vec::new();
    };
    let Some(origin) = base_url.origin().ascii_serialization().into() else {
        return Vec::new();
    };
    let origin: String = origin;
    let mut out: Vec<String> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    for attr in ["href", "src", "action"] {
        let pat = format!("{attr}=\"");
        let mut rest = body;
        while let Some(idx) = rest.find(&pat) {
            rest = &rest[idx + pat.len()..];
            if let Some(end) = rest.find('"') {
                let raw = &rest[..end];
                rest = &rest[end + 1..];
                if raw.is_empty() || raw.starts_with('#') || raw.starts_with("javascript:") || raw.starts_with("mailto:") {
                    continue;
                }
                let Ok(joined) = base_url.join(raw) else { continue };
                if joined.scheme() != "http" && joined.scheme() != "https" {
                    continue;
                }
                if joined.origin().ascii_serialization() != origin {
                    continue;
                }
                let u = joined.to_string();
                if seen.insert(u.clone()) {
                    out.push(u);
                }
            }
        }
    }
    out
}

/// Parse query-string insertion points from a URL.
pub fn query_params(url: &str) -> Vec<(String, String)> {
    url.split('?')
        .nth(1)
        .map(|q| {
            q.split('&')
                .filter_map(|pair| {
                    let mut it = pair.splitn(2, '=');
                    let k = it.next()?.to_string();
                    let v = it.next().unwrap_or("").to_string();
                    if k.is_empty() { None } else { Some((k, v)) }
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Replace one query parameter's value in a URL, leaving the others intact.
pub fn set_query_param(url: &str, key: &str, new_val: &str) -> String {
    match url.split_once('?') {
        Some((base, qs)) => {
            let mut found = false;
            let parts: Vec<String> = qs
                .split('&')
                .map(|pair| {
                    let mut it = pair.splitn(2, '=');
                    let k = it.next().unwrap_or("");
                    if k == key {
                        found = true;
                        format!("{}={}", k, urlencode(new_val))
                    } else {
                        pair.to_string()
                    }
                })
                .collect();
            if found {
                format!("{}?{}", base, parts.join("&"))
            } else {
                format!("{}?{}&{}={}", base, parts.join("&"), key, urlencode(new_val))
            }
        }
        None => {
            // no existing query — append the parameter
            format!("{}?{}={}", url, key, urlencode(new_val))
        }
    }
}

fn urlencode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(b as char),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// Fetch a URL with a plain GET (no redirect following — crawl wants to see the 30x).
async fn crawl_fetch(url: &str) -> Option<(u16, Vec<(String, String)>, String)> {
    let u = url::Url::parse(url).ok()?;
    let host = u.host_str()?.to_string();
    let tls = u.scheme() == "https";
    let port = u.port().unwrap_or(if tls { 443 } else { 80 });
    let path = format!("{}{}", u.path(), u.query().map(|q| format!("?{q}")).unwrap_or_default());
    let authority = match u.port() {
        Some(p) => format!("{host}:{p}"),
        None => host.clone(),
    };
    let raw = format!(
        "GET {path} HTTP/1.1\r\nHost: {authority}\r\nUser-Agent: WebProxy-Toolkit-Scanner/0.1\r\nAccept: */*\r\nConnection: close\r\n\r\n"
    );
    let (status, headers, body, _ms) = crate::proxy::send_raw_request(raw, host, port, tls).await.ok()?;
    Some((status, headers, body))
}

/// Active check: SQL error signatures in response body.
fn active_sqli(param: &str, resp_body: &str) -> Option<(&'static str, String)> {
    let lower = resp_body.to_lowercase();
    for sign in SQL_SIGNS {
        if lower.contains(sign) {
            let ev = snippet_around(resp_body, sign, 120);
            return Some(("sqli-error", format!("param={param}; {ev}")));
        }
    }
    None
}

/// Active check: unencoded canary reflection (XSS).
fn active_xss(param: &str, sent: &str, resp_body: &str) -> Option<(&'static str, String)> {
    // canary appears raw (not HTML-entity-encoded) anywhere in the response
    if resp_body.contains(sent) {
        let ev = snippet_around(resp_body, sent, 80);
        return Some(("xss-reflected", format!("param={param}; {ev}")));
    }
    None
}

/// Active check: path traversal payload exposing /etc/passwd or win.ini.
fn active_traversal(param: &str, resp_body: &str) -> Option<(&'static str, String)> {
    let lower = resp_body.to_lowercase();
    if lower.contains("root:x:") || lower.contains("[boot loader]") {
        let marker = if lower.contains("root:x:") { "root:x:" } else { "[boot loader]" };
        let ev = snippet_around(resp_body, marker, 100);
        return Some(("path-traversal", format!("param={param}; {ev}")));
    }
    None
}

/// Active check: open redirect — attacker URL accepted, Location points at it.
fn active_redirect(param: &str, sent: &str, headers: &[(String, String)]) -> Option<(&'static str, String)> {
    for (k, v) in headers {
        if k.eq_ignore_ascii_case("location") && v.contains(sent) {
            return Some(("open-redirect", format!("param={param}; Location: {v}")));
        }
    }
    None
}

/// Build a GET raw request for `url`.
fn build_get_raw(url: &str) -> Option<String> {
    let u = url::Url::parse(url).ok()?;
    let host = u.host_str()?.to_string();
    let authority = match u.port() {
        Some(p) => format!("{host}:{p}"),
        None => host,
    };
    let path = format!("{}{}", u.path(), u.query().map(|q| format!("?{q}")).unwrap_or_default());
    Some(format!(
        "GET {path} HTTP/1.1\r\nHost: {authority}\r\nUser-Agent: WebProxy-Toolkit-Scanner/0.1\r\nAccept: */*\r\nConnection: close\r\n\r\n"
    ))
}

/// Run one scan task to completion (crawl and/or active audit).
/// `app` optional for tests. Emits `scan-progress` (throttled ~250 ms) and updates the DB row.
pub async fn run_scan_task(
    app: Option<tauri::AppHandle>,
    state: crate::proxy::SharedProxy,
    task_id: i64,
    target: String,
    kind: String,
    limits: ScanLimits,
    ctl: Arc<ScanCtl>,
) {
    use tauri::Emitter;
    let db = state.db.read().clone();
    let do_crawl = kind.contains("crawl");
    let do_active = kind.contains("audit") && limits.active;

    let mut queue: VecDeque<CrawlTarget> = VecDeque::new();
    let mut visited: HashSet<String> = HashSet::new();
    // insertion points discovered during crawl: (url_with_param, param_name)
    let mut points: Vec<(String, String)> = Vec::new();
    let mut last_emit = Instant::now();
    let mut final_status = "done";
    let mut final_error: Option<String> = None;

    queue.push_back(CrawlTarget { url: target.clone(), depth: 0 });
    visited.insert(target.clone());

    let emit_progress = |status: &str, queued: usize| {
        if let Some(a) = app.as_ref() {
            let snap = serde_json::json!({
                "task_id": task_id,
                "pages": ctl.pages.load(Ordering::SeqCst),
                "requests": ctl.requests.load(Ordering::SeqCst),
                "issues": ctl.issues.load(Ordering::SeqCst),
                "status": status,
                "queued": queued,
            });
            let _ = a.emit("scan-progress", snap);
        }
    };

    // --- crawl phase -----------------------------------------------------
    while let Some(node) = queue.pop_front() {
        if ctl.cancelled() {
            final_status = "cancelled";
            break;
        }
        ctl.wait_if_paused();
        if ctl.cancelled() {
            final_status = "cancelled";
            break;
        }
        if ctl.pages.load(Ordering::SeqCst) as usize >= limits.max_pages {
            break;
        }
        if limits.throttle_ms > 0 {
            tokio::time::sleep(Duration::from_millis(limits.throttle_ms)).await;
        }
        match crawl_fetch(&node.url).await {
            Some((status, headers, body)) => {
                ctl.requests.fetch_add(1, Ordering::SeqCst);
                ctl.pages.fetch_add(1, Ordering::SeqCst);
                // record insertion points from query string
                for (k, _) in query_params(&node.url) {
                    points.push((node.url.clone(), k));
                }
                if do_crawl && status == 200 && node.depth < limits.max_depth {
                    let is_html = headers
                        .iter()
                        .any(|(k, v)| k.eq_ignore_ascii_case("content-type") && v.contains("text/html"));
                    if is_html {
                        for link in extract_links(&node.url, &body) {
                            if visited.insert(link.clone()) {
                                queue.push_back(CrawlTarget { url: link, depth: node.depth + 1 });
                            }
                        }
                    }
                }
            }
            None => {
                ctl.requests.fetch_add(1, Ordering::SeqCst);
            }
        }
        if last_emit.elapsed() >= Duration::from_millis(250) {
            emit_progress("running", queue.len());
            last_emit = Instant::now();
        }
    }

    // --- active audit phase ----------------------------------------------
    if do_active && !ctl.cancelled() {
        // dedupe points by (url,param)
        points.sort();
        points.dedup();
        for (url, param) in &points {
            if ctl.cancelled() {
                final_status = "cancelled";
                break;
            }
            ctl.wait_if_paused();
            if ctl.cancelled() {
                final_status = "cancelled";
                break;
            }
            if limits.throttle_ms > 0 {
                tokio::time::sleep(Duration::from_millis(limits.throttle_ms)).await;
            }
            let original_val = query_params(url)
                .into_iter()
                .find(|(k, _)| k == param)
                .map(|(_, v)| v)
                .unwrap_or_default();

            // payloads per check
            let payloads: &[(&str, &str)] = &[
                ("sqli-error", "'"),
                ("xss-reflected", "<wptXSScanary>"),
                ("path-traversal", "../../../../etc/passwd"),
                ("open-redirect", "https://evil.example"),
            ];
            for (check, payload) in payloads {
                if ctl.cancelled() { break; }
                if let Some(only) = limits.only_checks.as_ref() {
                    if !only.iter().any(|c| c == check) { continue; }
                }
                let test_url = set_query_param(url, param, payload);
                let Some(raw) = build_get_raw(&test_url) else { continue };
                let Some(u) = url::Url::parse(&test_url).ok() else { continue };
                let host = u.host_str().unwrap_or_default().to_string();
                let tls = u.scheme() == "https";
                let port = u.port().unwrap_or(if tls { 443 } else { 80 });
                match crate::proxy::send_raw_request(raw, host.clone(), port, tls).await {
                    Ok((status, headers, body, _ms)) => {
                        ctl.requests.fetch_add(1, Ordering::SeqCst);
                        let hit: Option<(&'static str, String)> = match *check {
                            "sqli-error" => active_sqli(param, &body),
                            "xss-reflected" => active_xss(param, payload, &body),
                            "path-traversal" => active_traversal(param, &body),
                            "open-redirect" => {
                                if matches!(status, 301 | 302 | 303 | 307 | 308) {
                                    active_redirect(param, payload, &headers)
                                } else {
                                    None
                                }
                            }
                            _ => None,
                        };
                        if let Some((check_id, evidence)) = hit {
                            let Some(def) = check_def(check_id) else { continue };
                            let draft = NewIssue {
                                check_id: def.id.to_string(),
                                name: def.name.to_string(),
                                severity: def.severity.to_string(),
                                confidence: "firm".into(),
                                host: host.clone(),
                                url: url.clone(),
                                method: "GET".into(),
                                path: u.path().to_string(),
                                param: param.clone(),
                                description: def.description.to_string(),
                                remediation: def.remediation.to_string(),
                                evidence,
                                exchange_id: None,
                            };
                            if let Some(db) = db.as_ref() {
                                match db.insert_issue(&draft) {
                                    Ok(Some(id)) => {
                                        ctl.issues.fetch_add(1, Ordering::SeqCst);
                                        if let Some(app) = app.as_ref() {
                                            if let Ok(Some(issue)) = db.get_issue(id) {
                                                let _ = app.emit("issue-added", issue);
                                            }
                                        }
                                    }
                                    Ok(None) => {}
                                    Err(e) => eprintln!("scan issue insert failed: {e}"),
                                }
                            }
                        }
                    }
                    Err(_) => {
                        ctl.requests.fetch_add(1, Ordering::SeqCst);
                    }
                }
            }
            let _ = original_val; // reserved for future baseline-diff logic
            if last_emit.elapsed() >= Duration::from_millis(250) {
                emit_progress("running", 0);
                last_emit = Instant::now();
            }
        }
    }

    // --- sensitive-files probe (always part of an audit) -------------------
    let files_enabled = limits
        .only_checks
        .as_ref()
        .map(|only| only.iter().any(|c| c == "sensitive-files"))
        .unwrap_or(true);
    if do_active && files_enabled && !ctl.cancelled() {
        if let Ok(base) = url::Url::parse(&target) {
            let origin = base.origin().ascii_serialization();
            for path in ["/.git/config", "/.env", "/backup.zip", "/.svn/entries", "/wp-config.php.bak"] {
                if ctl.cancelled() {
                    final_status = "cancelled";
                    break;
                }
                ctl.wait_if_paused();
                let probe = format!("{origin}{path}");
                if let Some(raw) = build_get_raw(&probe) {
                    if let Some(u) = url::Url::parse(&probe).ok() {
                        let host = u.host_str().unwrap_or_default().to_string();
                        let tls = u.scheme() == "https";
                        let port = u.port().unwrap_or(if tls { 443 } else { 80 });
                        if let Ok((status, _h, _b, _ms)) =
                            crate::proxy::send_raw_request(raw, host.clone(), port, tls).await
                        {
                            ctl.requests.fetch_add(1, Ordering::SeqCst);
                            if status == 200 {
                                let Some(def) = check_def("sensitive-files") else { continue };
                                let draft = NewIssue {
                                    check_id: def.id.to_string(),
                                    name: def.name.to_string(),
                                    severity: def.severity.to_string(),
                                    confidence: "firm".into(),
                                    host: host.clone(),
                                    url: probe.clone(),
                                    method: "GET".into(),
                                    path: path.to_string(),
                                    param: String::new(),
                                    description: def.description.to_string(),
                                    remediation: def.remediation.to_string(),
                                    evidence: format!("GET {path} → HTTP {status}"),
                                    exchange_id: None,
                                };
                                if let Some(db) = db.as_ref() {
                                    if let Ok(Some(id)) = db.insert_issue(&draft) {
                                        ctl.issues.fetch_add(1, Ordering::SeqCst);
                                        if let Some(app) = app.as_ref() {
                                            if let Ok(Some(issue)) = db.get_issue(id) {
                                                let _ = app.emit("issue-added", issue);
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                if limits.throttle_ms > 0 {
                    tokio::time::sleep(Duration::from_millis(limits.throttle_ms)).await;
                }
            }
        }
    }

    // --- finalize ----------------------------------------------------------
    if ctl.cancelled() && final_status != "cancelled" {
        final_status = "cancelled";
    }
    if let Some(db) = db.as_ref() {
        let _ = db.update_scan_task(
            task_id,
            final_status,
            ctl.pages.load(Ordering::SeqCst) as i64,
            ctl.requests.load(Ordering::SeqCst) as i64,
            ctl.issues.load(Ordering::SeqCst) as i64,
            final_error.as_deref(),
        );
    }
    emit_progress(final_status, 0);
    state.scan_runs.lock().remove(&task_id);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn exchange(url: &str, req: Vec<(String, String)>, resp: Vec<(String, String)>, body: &str) -> HttpExchange {
        HttpExchange {
            id: 1,
            started_at: "2026-10-13 10:00:00".into(),
            time_ms: Some(12),
            client_addr: None,
            host: "app.test".into(),
            method: "GET".into(),
            url: url.into(),
            req_headers: req,
            req_body: None,
            status: Some(200),
            resp_headers: resp,
            resp_body: Some(body.into()),
            tls: url.starts_with("https://"),
            websocket: false,
            in_scope: true,
        }
    }

    fn check_ids(ex: &HttpExchange) -> Vec<String> {
        passive_scan(ex).into_iter().map(|i| i.check_id).collect()
    }

    fn find<'a>(issues: &'a [NewIssue], id: &str) -> &'a NewIssue {
        issues.iter().find(|i| i.check_id == id).unwrap_or_else(|| panic!("expected {id}"))
    }

    const SEC_HEADERS: &[(&str, &str)] = &[
        ("Content-Security-Policy", "default-src 'self'"),
        ("Strict-Transport-Security", "max-age=31536000"),
        ("X-Frame-Options", "DENY"),
        ("X-Content-Type-Options", "nosniff"),
        ("Referrer-Policy", "strict-origin"),
    ];

    fn secured_resp() -> Vec<(String, String)> {
        SEC_HEADERS
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    #[test]
    fn catalog_has_unique_ids_and_nonempty_metadata() {
        let mut seen = std::collections::HashSet::new();
        for c in CHECKS {
            assert!(seen.insert(c.id), "duplicate check id {}", c.id);
            assert!(!c.name.is_empty());
            assert!(!c.description.is_empty());
            assert!(!c.remediation.is_empty());
            assert!(["info", "low", "medium", "high", "critical"].contains(&c.severity));
        }
        assert!(CHECKS.len() >= 15);
        assert!(passive_check_ids().len() >= 15);
    }

    #[test]
    fn secured_response_raises_no_header_issues() {
        let ex = exchange("https://app.test/", vec![], secured_resp(), "<html></html>");
        let issues = passive_scan(&ex);
        for id in ["missing-csp", "missing-hsts", "missing-xfo", "missing-xcto", "missing-referrer-policy"] {
            assert!(!issues.iter().any(|i| i.check_id == id), "{id} should not fire");
        }
    }

    #[test]
    fn bare_response_raises_all_missing_header_checks() {
        let ex = exchange("https://app.test/", vec![], vec![("Content-Type".into(), "text/html".into())], "<html></html>");
        let ids = check_ids(&ex);
        for id in ["missing-csp", "missing-hsts", "missing-xfo", "missing-xcto", "missing-referrer-policy"] {
            assert!(ids.contains(&id.to_string()), "{id} should fire");
        }
        // http URL should not raise HSTS
        let ex_http = exchange("http://app.test/", vec![], vec![], "");
        assert!(!check_ids(&ex_http).contains(&"missing-hsts".to_string()));
        // frame-ancestors in any CSP header suppresses XFO (headers combine)
        let mut resp = secured_resp();
        resp.retain(|(k, _)| k != "X-Frame-Options");
        resp.push(("Content-Security-Policy".into(), "frame-ancestors 'none'".into()));
        let ex2 = exchange("https://app.test/", vec![], resp, "");
        assert!(!check_ids(&ex2).contains(&"missing-xfo".to_string()));
    }

    #[test]
    fn cookie_flags_fire_per_attribute() {
        let resp = vec![
            ("Set-Cookie".into(), "sid=abc123; Path=/".into()),
            ("Set-Cookie".into(), "prefs=1; Path=/; HttpOnly; Secure".into()),
        ];
        let issues = passive_scan(&exchange("https://app.test/", vec![], resp, ""));
        let httponly = issues.iter().filter(|i| i.check_id == "cookie-no-httponly").collect::<Vec<_>>();
        assert_eq!(httponly.len(), 1);
        assert_eq!(httponly[0].param, "sid");
        // both cookies missing Secure on https
        let secure = issues.iter().filter(|i| i.check_id == "cookie-no-secure").collect::<Vec<_>>();
        assert_eq!(secure.len(), 1);
        assert_eq!(secure[0].param, "sid");
    }

    #[test]
    fn disclosure_checks_fire_on_body_and_headers() {
        let resp = vec![
            ("Server".into(), "nginx/1.24.0".into()),
            ("X-Powered-By".into(), "Express".into()),
        ];
        let body = "Index of /backup\n contact admin@example.com\n conn to 192.168.1.10:5432\nYou have an error in your SQL syntax near 'AND'";
        let issues = passive_scan(&exchange("http://app.test/", vec![], resp, body));
        let ids: Vec<&str> = issues.iter().map(|i| i.check_id.as_str()).collect();
        assert!(ids.contains(&"server-banner"));
        assert!(ids.contains(&"x-powered-by"));
        assert!(ids.contains(&"directory-listing"));
        assert!(ids.contains(&"email-disclosure"));
        assert!(ids.contains(&"internal-ip-disclosure"));
        assert!(ids.contains(&"sql-error-leak"));
        let sql = find(&issues, "sql-error-leak");
        assert_eq!(sql.severity, "high");
        assert!(!sql.evidence.is_empty());
    }

    #[test]
    fn stack_trace_and_mixed_content() {
        let body = r#"<html><script src="http://cdn.example.com/app.js"></script>
Traceback (most recent call last):</html>"#;
        let issues = passive_scan(&exchange("https://app.test/", vec![], vec![], body));
        let ids: Vec<&str> = issues.iter().map(|i| i.check_id.as_str()).collect();
        assert!(ids.contains(&"stack-trace-leak"));
        assert!(ids.contains(&"mixed-content"));
        // same http:// subresource on an http page is not mixed content
        let issues_http = passive_scan(&exchange("http://app.test/", vec![], vec![], body));
        assert!(!issues_http.iter().any(|i| i.check_id == "mixed-content"));
    }

    #[test]
    fn request_side_checks_basic_auth_and_password_in_url() {
        let req = vec![("Authorization".into(), "Basic YWRtaW46c2VjcmV0".into())];
        let issues = passive_scan(&exchange("http://app.test/", req, vec![], ""));
        assert!(issues.iter().any(|i| i.check_id == "basic-auth-plaintext"));
        // over https the same header is fine
        let issues2 = passive_scan(&exchange("https://app.test/", vec![("Authorization".into(), "Basic YWRtaW46c2VjcmV0".into())], secured_resp(), ""));
        assert!(!issues2.iter().any(|i| i.check_id == "basic-auth-plaintext"));

        let ex = exchange("https://app.test/search?q=test&password=hunter2", vec![], secured_resp(), "");
        let issues3 = passive_scan(&ex);
        let pw = find(&issues3, "password-in-url");
        assert_eq!(pw.param, "password");
        assert!(!pw.evidence.contains("hunter2"), "evidence must not leak the secret");
    }

    #[test]
    fn no_response_status_yields_nothing_and_metadata_is_attached() {
        let mut ex = exchange("https://app.test/", vec![], vec![], "");
        ex.status = None;
        assert!(passive_scan(&ex).is_empty());

        let issues = passive_scan(&exchange("https://app.test/", vec![], vec![], ""));
        let issue = find(&issues, "missing-csp");
        assert_eq!(issue.name, "Missing Content-Security-Policy");
        assert_eq!(issue.severity, "info");
        assert_eq!(issue.confidence, "certain");
        assert_eq!(issue.method, "GET");
        assert_eq!(issue.path, "/");
        assert_eq!(issue.exchange_id, Some(1));
        assert!(!issue.description.is_empty());
        assert!(!issue.remediation.is_empty());
    }

    #[test]
    fn url_path_and_query_helpers() {
        assert_eq!(url_path("https://a.test/x/y?z=1"), "/x/y");
        assert_eq!(url_path("https://a.test"), "/");
        assert_eq!(url_path("http://a.test/"), "/");
        assert_eq!(query_pairs("https://a.test/?a=1&b=2&c"), vec![("a".into(), "1".into()), ("b".into(), "2".into()), ("c".into(), String::new())]);
        assert!(query_pairs("https://a.test/").is_empty());
    }

    #[test]
    fn private_ip_classifier_accepts_rfc1918_only() {
        assert_eq!(first_private_ip("addr 10.0.0.8 done"), Some("10.0.0.8".to_string()));
        assert_eq!(first_private_ip("addr 192.168.5.5 done"), Some("192.168.5.5".to_string()));
        assert_eq!(first_private_ip("addr 172.16.0.9 done"), Some("172.16.0.9".to_string()));
        assert_eq!(first_private_ip("addr 172.32.0.9 done"), None);
        assert_eq!(first_private_ip("addr 8.8.8.8 done"), None);
        assert_eq!(first_private_ip("version 1.2.3.4"), None); // leading "1." boundary trick
        assert_eq!(first_private_ip("no ip here"), None);
    }

    // ------------------------------------------------------- crawl helpers (4b)

    #[test]
    fn extract_links_same_origin_only() {
        let body = r##"
            <a href="/about">About</a>
            <a href="https://app.test/contact">Contact</a>
            <a href="https://other.test/evil">External</a>
            <script src="/js/app.js"></script>
            <form action="/login" method="post"></form>
            <a href="#anchor">Skip</a>
            <a href="javascript:void(0)">JS</a>
            <a href="mailto:x@app.test">Mail</a>
        "##;
        let links = extract_links("https://app.test/", body);
        assert!(links.contains(&"https://app.test/about".to_string()), "{links:?}");
        assert!(links.contains(&"https://app.test/contact".to_string()));
        assert!(links.contains(&"https://app.test/js/app.js".to_string()));
        assert!(links.contains(&"https://app.test/login".to_string()));
        assert!(!links.iter().any(|l| l.contains("other.test")), "cross-origin must be skipped");
        assert!(!links.iter().any(|l| l.starts_with('#') || l.starts_with("javascript:") || l.starts_with("mailto:")));
    }

    #[test]
    fn query_params_parses_pairs() {
        assert_eq!(query_params("https://a.test/x?a=1&b=2"), vec![("a".into(), "1".into()), ("b".into(), "2".into())]);
        assert!(query_params("https://a.test/x").is_empty());
    }

    #[test]
    fn set_query_param_replaces_only_target() {
        let out = set_query_param("https://a.test/x?a=1&b=2", "a", "PAYLOAD");
        assert!(out.contains("a=PAYLOAD"), "{out}");
        assert!(out.contains("b=2"), "{out}");
        // no existing query → just append
        let out2 = set_query_param("https://a.test/x", "a", "PAYLOAD");
        assert!(out2.contains("a=PAYLOAD"), "{out2}");
    }

    #[test]
    fn active_sqli_detects_error_signature() {
        let (id, ev) = active_sqli("q", "You have an error in your SQL syntax near '''").expect("hit");
        assert_eq!(id, "sqli-error");
        assert!(ev.contains("param=q"), "{ev}");
        assert!(active_sqli("q", "all good here").is_none());
    }

    #[test]
    fn active_xss_requires_raw_canary() {
        let canary = "<wptXSScanary>";
        // raw reflection → hit
        assert!(active_xss("q", canary, &format!("echo {canary} back")).is_some());
        // entity-encoded reflection → no hit
        assert!(active_xss("q", canary, "echo &lt;wptXSScanary&gt; back").is_none());
    }

    #[test]
    fn active_traversal_detects_passwd_marker() {
        let (id, _) = active_traversal("file", "root:x:0:0:root:/root:/bin/bash").expect("hit");
        assert_eq!(id, "path-traversal");
        assert!(active_traversal("file", "nothing here").is_none());
    }

    #[test]
    fn active_redirect_requires_location_match() {
        let hdrs = vec![("Location".into(), "https://evil.example/landing".into())];
        let (id, _) = active_redirect("next", "https://evil.example", &hdrs).expect("hit");
        assert_eq!(id, "open-redirect");
        // Location pointing elsewhere → no hit
        let safe = vec![("Location".into(), "/dashboard".into())];
        assert!(active_redirect("next", "https://evil.example", &safe).is_none());
    }
}
