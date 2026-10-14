use crate::proxy::SharedProxy;
use serde::{Deserialize, Serialize};

#[derive(Clone, Serialize, Deserialize)]
pub struct SessionMacro {
    pub id: i64,
    pub name: String,
    pub requests: Vec<String>,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct SessionExtract {
    pub from: String, // cookie | header | body
    #[serde(default)]
    pub name: String,  // cookie/header name
    #[serde(default)]
    pub regex: String, // body regex (first capture group or whole match)
}

#[derive(Clone, Serialize, Deserialize)]
pub struct SessionUpdate {
    pub kind: String, // set-header | set-cookie | set-param
    pub name: String,
    #[serde(default)]
    pub value: Option<String>,
    #[serde(default)]
    pub extract: Option<SessionExtract>,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct SessionRule {
    pub id: i64,
    pub position: i64,
    pub name: String,
    pub tool: String,     // any | proxy | repeater | intruder | scanner
    pub match_host: String, // "" = any
    pub macro_id: Option<i64>,
    pub updates: Vec<SessionUpdate>,
    pub enabled: bool,
}

/// Extract a value from a raw HTTP response using the extract spec.
pub fn extract_value(resp_raw: &str, ex: &SessionExtract) -> Option<String> {
    let (_rl, headers, body) = crate::proxy::split_message(resp_raw)?;
    let body_text = String::from_utf8_lossy(&body).to_string();
    match ex.from.as_str() {
        "cookie" => {
            // find Set-Cookie: name=value
            for (k, v) in &headers {
                if k.eq_ignore_ascii_case("set-cookie") {
                    if let Some((nv, _rest)) = v.split_once(';') {
                        let (n, val) = nv.split_once('=')?;
                        if n.trim() == ex.name {
                            return Some(val.trim().to_string());
                        }
                    }
                }
            }
            None
        }
        "header" => {
            for (k, v) in &headers {
                if k.eq_ignore_ascii_case(&ex.name) {
                    return Some(v.clone());
                }
            }
            None
        }
        _ => {
            // body regex
            let re = regex::Regex::new(&ex.regex).ok()?;
            let cap = re.captures(&body_text)?;
            let m = cap.get(1).or_else(|| cap.get(0))?;
            Some(m.as_str().to_string())
        }
    }
}

/// Replace or add a header in a raw request. Returns the new raw request.
pub fn set_header(raw: &str, name: &str, value: &str) -> Option<String> {
    let (rl, headers, body) = crate::proxy::split_message(raw)?;
    let mut out_headers: Vec<(String, String)> = Vec::new();
    let mut replaced = false;
    for (k, v) in headers {
        if k.eq_ignore_ascii_case(name) {
            if !replaced {
                out_headers.push((k, value.to_string()));
                replaced = true;
            }
        } else {
            out_headers.push((k, v));
        }
    }
    if !replaced {
        out_headers.push((name.to_string(), value.to_string()));
    }
    Some(String::from_utf8_lossy(&crate::proxy::rebuild_request(&rl, &out_headers, &body)).to_string())
}

/// Set a cookie in the Cookie request header (add or replace by name).
pub fn set_cookie(raw: &str, name: &str, value: &str) -> Option<String> {
    let (rl, headers, body) = crate::proxy::split_message(raw)?;
    let mut cookie_val = String::new();
    let mut found = false;
    for (k, v) in &headers {
        if k.eq_ignore_ascii_case("cookie") {
            for part in v.split(';') {
                let part = part.trim();
                if part.is_empty() {
                    continue;
                }
                let (n, val) = match part.split_once('=') {
                    Some(x) => x,
                    None => (part, ""),
                };
                if n.trim() == name {
                    cookie_val.push_str(&format!("{name}={value}; "));
                    found = true;
                } else {
                    cookie_val.push_str(&format!("{part}; "));
                }
            }
        }
    }
    if !found {
        cookie_val.push_str(&format!("{name}={value}; "));
    }
    let cookie_val = cookie_val.trim_end_matches("; ").to_string();
    set_header(raw, "Cookie", &cookie_val)
}

/// Set a query parameter in the request-line (or a form body param for POST).
pub fn set_param(raw: &str, name: &str, value: &str) -> Option<String> {
    let (rl, headers, body) = crate::proxy::split_message(raw)?;
    let parts: Vec<&str> = rl.splitn(3, ' ').collect();
    if parts.len() != 3 {
        return None;
    }
    let (method, path, version) = (parts[0], parts[1], parts[2]);
    // split path/query
    if let Some((base, query)) = path.split_once('?') {
        let mut pairs: Vec<(String, String)> = query
            .split('&')
            .filter(|p| !p.is_empty())
            .map(|p| match p.split_once('=') {
                Some((k, v)) => (k.to_string(), v.to_string()),
                None => (p.to_string(), String::new()),
            })
            .collect();
        let mut replaced = false;
        for (k, v) in pairs.iter_mut() {
            if k == name {
                *v = value.to_string();
                replaced = true;
            }
        }
        if !replaced {
            pairs.push((name.to_string(), value.to_string()));
        }
        let q = pairs
            .iter()
            .map(|(k, v)| format!("{k}={v}"))
            .collect::<Vec<_>>()
            .join("&");
        let new_rl = format!("{method} {base}?{q} {version}");
        return Some(String::from_utf8_lossy(&crate::proxy::rebuild_request(&new_rl, &headers, &body)).to_string());
    }
    // no query — set form body param if form-encoded
    let ct = headers
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case("content-type"))
        .map(|(_, v)| v.clone())
        .unwrap_or_default();
    if ct.contains("x-www-form-urlencoded") {
        let body_text = String::from_utf8_lossy(&body).to_string();
        let mut pairs: Vec<(String, String)> = body_text
            .split('&')
            .filter(|p| !p.is_empty())
            .map(|p| match p.split_once('=') {
                Some((k, v)) => (k.to_string(), v.to_string()),
                None => (p.to_string(), String::new()),
            })
            .collect();
        let mut replaced = false;
        for (k, v) in pairs.iter_mut() {
            if k == name {
                *v = value.to_string();
                replaced = true;
            }
        }
        if !replaced {
            pairs.push((name.to_string(), value.to_string()));
        }
        let new_body = pairs
            .iter()
            .map(|(k, v)| format!("{k}={v}"))
            .collect::<Vec<_>>()
            .join("&");
        return Some(String::from_utf8_lossy(&crate::proxy::rebuild_request(&rl, &headers, new_body.as_bytes())).to_string());
    }
    None
}

/// Apply a single resolved update (value already extracted or literal) to the request.
pub fn apply_update(raw: &str, upd: &SessionUpdate, value: &str) -> Option<String> {
    match upd.kind.as_str() {
        "set-header" => set_header(raw, &upd.name, value),
        "set-cookie" => set_cookie(raw, &upd.name, value),
        "set-param" => set_param(raw, &upd.name, value),
        _ => None,
    }
}

/// True if a rule matches the tool and host.
pub fn rule_matches(rule: &SessionRule, tool: &str, host: &str) -> bool {
    if !rule.enabled {
        return false;
    }
    let tool_ok = rule.tool == "any" || rule.tool == tool;
    let host_ok =
        rule.match_host.is_empty() || crate::proxy::host_matches(&rule.match_host, host);
    tool_ok && host_ok
}

/// Run matching session rules against a request, replaying macros to resolve extracts.
/// Returns the (possibly rewritten) request. Macro replay is best-effort; on failure the
/// literal update values are used.
pub async fn run_session_rules(
    state: &SharedProxy,
    tool: &str,
    host: &str,
    port: u16,
    tls: bool,
    request_raw: &str,
) -> Result<String, String> {
    let rules = {
        let db = state.db.read().clone();
        match db {
            Some(db) => db.list_session_rules().unwrap_or_default(),
            None => Vec::new(),
        }
    };
    if rules.is_empty() {
        return Ok(request_raw.to_string());
    }
    let mut current = request_raw.to_string();
    for rule in rules.iter().filter(|r| rule_matches(r, tool, host)) {
        // resolve macro (last response) if present
        let mut macro_resp: Option<String> = None;
        if let Some(mid) = rule.macro_id {
            let db_opt = state.db.read().clone();
            if let Some(db) = db_opt {
                if let Ok(Some(m)) = db.get_session_macro(mid) {
                    let mut last: Option<String> = None;
                    for req in &m.requests {
                        match crate::proxy::send_raw_request(req.clone(), host.to_string(), port, tls).await
                        {
                            Ok((status, rh, rb, _ms)) => {
                                let mut resp = format!("HTTP/1.1 {status} X\r\n");
                                for (k, v) in &rh {
                                    resp.push_str(&format!("{k}: {v}\r\n"));
                                }
                                resp.push_str("\r\n");
                                resp.push_str(&rb);
                                last = Some(resp);
                            }
                            Err(_) => break,
                        }
                    }
                    macro_resp = last;
                }
            }
        }
        for upd in &rule.updates {
            let value = if let Some(ex) = &upd.extract {
                macro_resp
                    .as_ref()
                    .and_then(|r| extract_value(r, ex))
                    .or_else(|| upd.value.clone())
            } else {
                upd.value.clone()
            };
            if let Some(v) = value {
                if let Some(next) = apply_update(&current, upd, &v) {
                    current = next;
                }
            }
        }
    }
    Ok(current)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rule(tool: &str, host: &str) -> SessionRule {
        SessionRule {
            id: 1,
            position: 1,
            name: "r".into(),
            tool: tool.into(),
            match_host: host.into(),
            macro_id: None,
            updates: vec![],
            enabled: true,
        }
    }

    #[test]
    fn rule_matches_tool_and_host() {
        assert!(rule_matches(&rule("any", ""), "repeater", "h.example"));
        assert!(rule_matches(&rule("repeater", ""), "repeater", "h.example"));
        assert!(!rule_matches(&rule("repeater", ""), "proxy", "h.example"));
        assert!(rule_matches(&rule("any", "*.corp"), "proxy", "api.corp"));
        assert!(!rule_matches(&rule("any", "*.corp"), "proxy", "elsewhere"));
        let mut disabled = rule("any", "");
        disabled.enabled = false;
        assert!(!rule_matches(&disabled, "proxy", "h"));
    }

    #[test]
    fn extract_cookie_header_body() {
        let resp = "HTTP/1.1 200 X\r\nSet-Cookie: sid=abc123; Path=/\r\nX-Token: tok99\r\n\r\n{\"csrf\":\"zz\"}";
        let cookie = extract_value(
            resp,
            &SessionExtract { from: "cookie".into(), name: "sid".into(), regex: "".into() },
        )
        .unwrap();
        assert_eq!(cookie, "abc123");
        let header = extract_value(
            resp,
            &SessionExtract { from: "header".into(), name: "X-Token".into(), regex: "".into() },
        )
        .unwrap();
        assert_eq!(header, "tok99");
        let body = extract_value(
            resp,
            &SessionExtract { from: "body".into(), name: "".into(), regex: "\"csrf\":\"(\\w+)\"".into() },
        )
        .unwrap();
        assert_eq!(body, "zz");
    }

    #[test]
    fn set_header_replaces_or_adds() {
        let req = "GET / HTTP/1.1\r\nHost: h\r\n\r\n";
        let out = set_header(req, "X-New", "v1").unwrap();
        assert!(out.contains("X-New: v1"));
        let out2 = set_header(&out, "X-New", "v2").unwrap();
        assert!(out2.contains("X-New: v2"));
        assert_eq!(out2.matches("X-New").count(), 1);
    }

    #[test]
    fn set_cookie_adds_and_replaces() {
        let req = "GET / HTTP/1.1\r\nCookie: a=1; b=2\r\n\r\n";
        let out = set_cookie(req, "a", "9").unwrap();
        assert!(out.contains("a=9"));
        assert!(out.contains("b=2"));
        let out2 = set_cookie(req, "c", "3").unwrap();
        assert!(out2.contains("c=3"));
        assert!(out2.contains("a=1"));
    }

    #[test]
    fn set_param_query_and_form() {
        let req = "GET /x?a=1&b=2 HTTP/1.1\r\nHost: h\r\n\r\n";
        let out = set_param(req, "a", "9").unwrap();
        assert!(out.contains("a=9"));
        assert!(out.contains("b=2"));
        let post = "POST /login HTTP/1.1\r\nContent-Type: application/x-www-form-urlencoded\r\nContent-Length: 7\r\n\r\nu=old&p=x";
        let out2 = set_param(post, "u", "new").unwrap();
        assert!(out2.contains("u=new"));
        assert!(out2.contains("p=x"));
    }
}
