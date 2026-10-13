//! Intruder attack engine: §…§ position markers, payload sets + processors,
//! attack-type combo planning, and the paced/concurrent run loop.

use crate::db::{IntruderPayloadSet, NewIntruderResult};
use crate::proxy::{send_raw_request_follow, RawResponse, SharedProxy};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tauri::Emitter;

pub const MARKER: char = '§';
const RESULT_BATCH: usize = 25;
const MAX_PAYLOADS_PER_SET: usize = 1_000_000;

// ---------------------------------------------------------------- positions

#[derive(Clone, Debug, PartialEq)]
pub struct Position {
    /// byte offset of the opening §
    pub open: usize,
    /// byte offset of the closing §
    pub close: usize,
    /// text between the markers (as it appears in the template)
    pub text: String,
}

/// Paired `§…§` markers in the template. An unclosed trailing marker is ignored.
pub fn parse_positions(raw: &str) -> Vec<Position> {
    let bytes = raw.as_bytes();
    let mut marks: Vec<usize> = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == 0xC2 && i + 1 < bytes.len() && bytes[i + 1] == 0xA7 {
            marks.push(i);
            i += 2;
        } else {
            i += 1;
        }
    }
    let mut out = Vec::new();
    for pair in marks.chunks_exact(2) {
        let (open, close) = (pair[0], pair[1]);
        if close < open + 2 {
            continue; // overlapping / malformed pair
        }
        out.push(Position {
            open,
            close,
            text: raw[open + 2..close].to_string(),
        });
    }
    out
}

/// Template with all markers removed (inner text kept).
pub fn strip_positions(raw: &str) -> String {
    raw.replace(MARKER, "")
}

/// Splice payload values into the template.
/// `values[i]` = `Some((set_index, payload_index))` → substitute, `None` → keep original text.
pub fn build_request(
    raw: &str,
    positions: &[Position],
    values: &[Option<(usize, usize)>],
    sets: &[Vec<String>],
) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut last = 0usize;
    for (i, p) in positions.iter().enumerate() {
        out.push_str(&raw[last..p.open]);
        match values.get(i).copied().flatten() {
            Some((s, pi)) => out.push_str(sets.get(s).and_then(|v| v.get(pi)).map(|x| x.as_str()).unwrap_or(&p.text)),
            None => out.push_str(&p.text),
        }
        last = p.close + 2;
    }
    out.push_str(&raw[last..]);
    out
}

/// Wrap parameter values in `§…§`: query string, urlencoded/JSON body, cookies.
/// Values that already contain a marker are left alone.
pub fn auto_positions(raw: &str) -> String {
    let mut ranges: Vec<(usize, usize)> = Vec::new();

    // request line query string
    if let Some(line_end) = raw.find("\r\n") {
        let line = &raw[..line_end];
        if let Some(q) = line.find('?') {
            if let Some(target_end) = line[q..].find(' ') {
                let qs = &line[q + 1..q + target_end];
                collect_kv_value_ranges(qs, q + 1, &mut ranges);
            }
        }
    }

    // headers: Cookie values
    if let Ok(re) = regex::Regex::new(r"(?i)\r\ncookie:[^\r\n]*") {
        for m in re.find_iter(raw) {
            let text = m.as_str();
            let Some(colon) = text.find(':') else { continue };
            let mut vs = m.start() + colon + 1;
            while vs < m.end() && raw.as_bytes()[vs].is_ascii_whitespace() {
                vs += 1;
            }
            if vs >= m.end() {
                continue;
            }
            collect_cookie_ranges(&raw[vs..m.end()], vs, &mut ranges);
        }
    }

    // body
    if let Some(idx) = raw.find("\r\n\r\n") {
        let body_start = idx + 4;
        let body = &raw[body_start..];
        let head = raw[..idx].to_ascii_lowercase();
        let is_json = head.contains("content-type:") && head.contains("application/json")
            || body.trim_start().starts_with('{')
            || body.trim_start().starts_with('[');
        if is_json {
            if let Ok(re) = regex::Regex::new(r#""(?:[^"\\]|\\.)*"\s*:\s*"([^"]*)""#) {
                for m in re.captures_iter(body) {
                    if let Some(g) = m.get(1) {
                        if !g.as_str().contains(MARKER) {
                            ranges.push((body_start + g.start(), body_start + g.end()));
                        }
                    }
                }
            }
        } else if body.contains('=') {
            collect_kv_value_ranges(body, body_start, &mut ranges);
        }
    }

    // apply from the end so earlier offsets stay valid
    ranges.sort();
    ranges.dedup();
    let mut out = raw.to_string();
    for (s, e) in ranges.into_iter().rev() {
        if s >= e || e > out.len() || s >= out.len() {
            continue;
        }
        // skip if this slice straddles an already-marked area
        if out[..s].matches(MARKER).count() % 2 == 1 {
            continue;
        }
        out.insert_str(e, &MARKER.to_string());
        out.insert_str(s, &MARKER.to_string());
    }
    out
}

/// byte ranges of values in `k=v&k2=v2` text located at `base` offset in the full message
fn collect_kv_value_ranges(text: &str, base: usize, out: &mut Vec<(usize, usize)>) {
    for part in text.split('&') {
        let Some(eq) = part.find('=') else { continue };
        let value = &part[eq + 1..];
        if value.is_empty() || value.contains(MARKER) {
            continue;
        }
        let vs = base + (part.as_ptr() as usize - text.as_ptr() as usize) + eq + 1;
        out.push((vs, vs + value.len()));
    }
}

fn collect_cookie_ranges(text: &str, base: usize, out: &mut Vec<(usize, usize)>) {
    for part in text.split(';') {
        let Some(eq) = part.find('=') else { continue };
        let value = &part[eq + 1..];
        if value.is_empty() || value.contains(MARKER) {
            continue;
        }
        let trimmed_start = part.len() - part.trim_start().len();
        let _ = trimmed_start;
        let vs = base + (part.as_ptr() as usize - text.as_ptr() as usize) + eq + 1;
        out.push((vs, vs + value.len()));
    }
}

// ----------------------------------------------------------- attack planning

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AttackType {
    Sniper,
    BatteringRam,
    Pitchfork,
    ClusterBomb,
}

impl AttackType {
    pub fn parse(s: &str) -> AttackType {
        match s {
            "battering_ram" => AttackType::BatteringRam,
            "pitchfork" => AttackType::Pitchfork,
            "cluster_bomb" => AttackType::ClusterBomb,
            _ => AttackType::Sniper,
        }
    }
    pub fn as_str(&self) -> &'static str {
        match self {
            AttackType::Sniper => "sniper",
            AttackType::BatteringRam => "battering_ram",
            AttackType::Pitchfork => "pitchfork",
            AttackType::ClusterBomb => "cluster_bomb",
        }
    }
    pub fn label(&self) -> &'static str {
        match self {
            AttackType::Sniper => "Sniper",
            AttackType::BatteringRam => "Battering ram",
            AttackType::Pitchfork => "Pitchfork",
            AttackType::ClusterBomb => "Cluster bomb",
        }
    }
}

fn set_for(set_map: &[usize], position: usize, set_count: usize) -> usize {
    if set_count == 0 {
        return 0;
    }
    set_map
        .get(position)
        .copied()
        .filter(|s| *s < set_count)
        .unwrap_or(0)
        .min(set_count - 1)
}

/// Total requests for an attack (before the `max_requests` cap).
pub fn plan_total(
    attack: AttackType,
    sets: &[Vec<String>],
    position_count: usize,
    set_map: &[usize],
) -> usize {
    if sets.is_empty() || position_count == 0 {
        return 0;
    }
    match attack {
        AttackType::Sniper => (0..position_count)
            .map(|p| sets[set_for(set_map, p, sets.len())].len())
            .sum(),
        AttackType::BatteringRam => sets[set_for(set_map, 0, sets.len())].len(),
        AttackType::Pitchfork => (0..position_count)
            .map(|p| sets[set_for(set_map, p, sets.len())].len())
            .min()
            .unwrap_or(0),
        AttackType::ClusterBomb => (0..position_count).fold(1usize, |acc, p| {
            acc.saturating_mul(sets[set_for(set_map, p, sets.len())].len())
        }),
    }
}

/// Payload selection for request `idx`: one `Some((set, index))` per position.
pub fn plan_at(
    attack: AttackType,
    sets: &[Vec<String>],
    position_count: usize,
    set_map: &[usize],
    idx: usize,
) -> Vec<Option<(usize, usize)>> {
    let n_sets = sets.len();
    match attack {
        AttackType::Sniper => {
            let mut offset = 0usize;
            (0..position_count)
                .map(|p| {
                    let s = set_for(set_map, p, n_sets);
                    let size = sets.get(s).map(|v| v.len()).unwrap_or(0);
                    let local = idx.checked_sub(offset).unwrap_or(usize::MAX);
                    offset += size;
                    if local < size {
                        Some((s, local))
                    } else {
                        None
                    }
                })
                .collect()
        }
        AttackType::BatteringRam => {
            let s = set_for(set_map, 0, n_sets);
            let size = sets.get(s).map(|v| v.len()).unwrap_or(0);
            (0..position_count)
                .map(|p| {
                    let _ = p;
                    if idx < size {
                        Some((s, idx))
                    } else {
                        None
                    }
                })
                .collect()
        }
        AttackType::Pitchfork => (0..position_count)
            .map(|p| {
                let s = set_for(set_map, p, n_sets);
                let size = sets.get(s).map(|v| v.len()).unwrap_or(0);
                if idx < size {
                    Some((s, idx))
                } else {
                    None
                }
            })
            .collect(),
        AttackType::ClusterBomb => {
            // mixed-radix decomposition, most significant position first
            let sizes: Vec<usize> = (0..position_count)
                .map(|p| sets[set_for(set_map, p, n_sets)].len())
                .collect();
            let mut rest = idx;
            let mut slots: Vec<Option<(usize, usize)>> = vec![None; position_count];
            for p in 0..position_count {
                let size = sizes[p];
                let digit = if size == 0 { 0 } else { rest % size };
                rest /= size.max(1);
                if size > 0 {
                    slots[p] = Some((set_for(set_map, p, n_sets), digit));
                }
            }
            slots
        }
    }
}

// -------------------------------------------------------- payload expansion

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Processor {
    pub kind: String,
    #[serde(default)]
    pub a: Option<String>,
    #[serde(default)]
    pub b: Option<String>,
}

/// Expand a payload set to its raw values (before processors).
pub fn expand_payloads(set: &IntruderPayloadSet) -> Result<Vec<String>, String> {
    let raw: Vec<String> = match set.kind.as_str() {
        "numbers" => {
            let start = set.start_i.ok_or("numbers payload needs a start value")?;
            let end = set.end_i.ok_or("numbers payload needs an end value")?;
            let step = set.step_i.unwrap_or(1);
            if step <= 0 {
                return Err("numbers payload step must be positive".into());
            }
            if start > end {
                return Err("numbers payload start must be <= end".into());
            }
            let count = ((end - start) / step + 1) as usize;
            if count > MAX_PAYLOADS_PER_SET {
                return Err(format!("payload set too large ({count} values)"));
            }
            (0..count).map(|i| (start + (i as i64) * step).to_string()).collect()
        }
        _ => {
            let mut lines: Vec<String> = set
                .items
                .split('\n')
                .map(|l| l.trim_end_matches('\r').to_string())
                .collect();
            if lines.last().map(|l| l.is_empty()).unwrap_or(false) {
                lines.pop();
            }
            lines
        }
    };
    if raw.len() > MAX_PAYLOADS_PER_SET {
        return Err(format!("payload set too large ({} values)", raw.len()));
    }
    Ok(raw)
}

pub fn apply_processors(payload: &str, procs: &[Processor]) -> String {
    let mut out = payload.to_string();
    for p in procs {
        out = match p.kind.as_str() {
            "url_encode" => url_encode(&out),
            "url_decode" => url_decode(&out),
            "base64_encode" => base64_encode(out.as_bytes()),
            "base64_decode" => base64_decode(&out),
            "md5" => hex_digest(&md5_digest(out.as_bytes())),
            "sha1" => hex_digest(&sha1_digest(out.as_bytes())),
            "sha256" => hex_digest(&sha256_digest(out.as_bytes())),
            "replace" => {
                if let (Some(a), Some(b)) = (&p.a, &p.b) {
                    if !a.is_empty() {
                        out.replace(a.as_str(), b.as_str())
                    } else {
                        out
                    }
                } else {
                    out
                }
            }
            "prefix" => format!("{}{}", p.a.clone().unwrap_or_default(), out),
            "suffix" => format!("{}{}", out, p.a.clone().unwrap_or_default()),
            "upper" => out.to_uppercase(),
            "lower" => out.to_lowercase(),
            "trim" => out.trim().to_string(),
            "reverse" => out.chars().rev().collect(),
            _ => out,
        };
    }
    out
}

/// Process all sets; returns `sets[set_index]` payloads, index order preserved.
pub fn processed_sets(sets: &[IntruderPayloadSet]) -> Result<Vec<Vec<String>>, String> {
    let mut by_index: Vec<Vec<String>> = Vec::new();
    for s in sets {
        let idx = s.set_index.max(0) as usize;
        while by_index.len() <= idx {
            by_index.push(Vec::new());
        }
        let procs: Vec<Processor> = serde_json::from_str(&s.processors).unwrap_or_default();
        let mut vals = expand_payloads(s)?;
        if vals.is_empty() {
            return Err(format!(
                "payload set {} ({}) is empty - add payloads before starting",
                s.set_index + 1,
                s.name
            ));
        }
        for v in vals.iter_mut() {
            *v = apply_processors(v, &procs);
        }
        by_index[idx] = vals;
    }
    if by_index.is_empty() || by_index.iter().all(|v| v.is_empty()) {
        return Err("no payload sets configured".into());
    }
    // fill gaps (sparse set indexes) with a single placeholder so indexing stays valid
    for v in by_index.iter_mut() {
        if v.is_empty() {
            v.push(String::new());
        }
    }
    Ok(by_index)
}

fn url_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for &b in s.as_bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

fn url_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(v) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

const B64_ALPHABET: &[u8; 64] =
    b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

fn base64_encode(data: &[u8]) -> String {
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let n = ((b[0] as u32) << 16) | ((b[1] as u32) << 8) | b[2] as u32;
        out.push(B64_ALPHABET[(n >> 18) as usize & 63] as char);
        out.push(B64_ALPHABET[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 {
            B64_ALPHABET[(n >> 6) as usize & 63] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            B64_ALPHABET[n as usize & 63] as char
        } else {
            '='
        });
    }
    out
}

fn base64_decode(s: &str) -> String {
    let val = |c: u8| -> Option<u32> {
        match c {
            b'A'..=b'Z' => Some((c - b'A') as u32),
            b'a'..=b'z' => Some((c - b'a') as u32 + 26),
            b'0'..=b'9' => Some((c - b'0') as u32 + 52),
            b'+' => Some(62),
            b'/' => Some(63),
            _ => None,
        }
    };
    let mut acc = 0u32;
    let mut bits = 0u32;
    let mut out: Vec<u8> = Vec::new();
    for &c in s.as_bytes() {
        if c == b'=' {
            break;
        }
        let Some(v) = val(c) else { continue };
        acc = (acc << 6) | v;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn hex_digest(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn md5_digest(data: &[u8]) -> Vec<u8> {
    use md5::Digest as _;
    md5::Md5::digest(data).to_vec()
}
fn sha1_digest(data: &[u8]) -> Vec<u8> {
    use sha1::Digest as _;
    sha1::Sha1::digest(data).to_vec()
}
fn sha256_digest(data: &[u8]) -> Vec<u8> {
    use sha2::Digest as _;
    sha2::Sha256::digest(data).to_vec()
}

// ------------------------------------------------------------- run control

pub struct RunControl {
    pub cancel: AtomicBool,
    pub paused: AtomicBool,
    pub sent: AtomicUsize,
    pub errors: AtomicUsize,
    pub total: usize,
    pub started: Instant,
}

impl RunControl {
    pub fn new(total: usize) -> Self {
        RunControl {
            cancel: AtomicBool::new(false),
            paused: AtomicBool::new(false),
            sent: AtomicUsize::new(0),
            errors: AtomicUsize::new(0),
            total,
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
    pub fn snapshot(&self, campaign_id: i64, phase: &str) -> serde_json::Value {
        let sent = self.sent.load(Ordering::SeqCst);
        let elapsed = self.started.elapsed().as_secs_f64();
        serde_json::json!({
            "campaign_id": campaign_id,
            "sent": sent,
            "total": self.total,
            "errors": self.errors.load(Ordering::SeqCst),
            "status": phase,
            "done": phase == "done" || phase == "stopped" || phase == "error",
            "elapsed_ms": (elapsed * 1000.0) as u64,
            "rps": if elapsed > 0.0 { (sent as f64 / elapsed * 10.0).round() / 10.0 } else { 0.0 },
        })
    }
}

pub type IntruderRuns = Mutex<HashMap<i64, Arc<RunControl>>>;

// ----------------------------------------------------------------- helpers

fn reason_phrase(status: u16) -> &'static str {
    match status {
        200 => "OK",
        201 => "Created",
        204 => "No Content",
        301 => "Moved Permanently",
        302 => "Found",
        303 => "See Other",
        304 => "Not Modified",
        307 => "Temporary Redirect",
        308 => "Permanent Redirect",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        409 => "Conflict",
        410 => "Gone",
        413 => "Payload Too Large",
        429 => "Too Many Requests",
        500 => "Internal Server Error",
        502 => "Bad Gateway",
        503 => "Service Unavailable",
        504 => "Gateway Timeout",
        _ => "",
    }
}

pub fn render_response(r: &RawResponse) -> String {
    let mut out = format!("HTTP/1.1 {} {}\r\n", r.status, reason_phrase(r.status));
    for (k, v) in &r.headers {
        out.push_str(k);
        out.push_str(": ");
        out.push_str(v);
        out.push_str("\r\n");
    }
    out.push_str("\r\n");
    out.push_str(&r.body);
    out
}

/// Compile extractors: (name, pattern). Invalid patterns are skipped.
pub fn compile_extractors(json: &str) -> Vec<(String, regex::Regex)> {
    let raw: Vec<serde_json::Value> = serde_json::from_str(json).unwrap_or_default();
    raw.into_iter()
        .filter_map(|v| {
            let name = v.get("name")?.as_str()?.to_string();
            let pattern = v.get("pattern")?.as_str()?;
            let re = regex::Regex::new(pattern).ok()?;
            Some((name, re))
        })
        .collect()
}

/// Grep a response with compiled extractors → `{name: value|null}`.
pub fn extract(resp_raw: &str, extractors: &[(String, regex::Regex)]) -> serde_json::Value {
    let mut map = serde_json::Map::new();
    for (name, re) in extractors {
        let value = match re.captures(resp_raw) {
            Some(c) => c
                .get(1)
                .or_else(|| c.get(0))
                .map(|m| m.as_str().to_string()),
            None => None,
        };
        map.insert(name.clone(), value.map_or(serde_json::Value::Null, serde_json::Value::String));
    }
    serde_json::Value::Object(map)
}

// ------------------------------------------------------------ the run loop

pub struct AttackJob {
    pub campaign_id: i64,
    pub req_raw: String,
    pub url_hint: Option<String>,
    pub follow: bool,
    pub attack: AttackType,
    pub sets: Vec<Vec<String>>,
    pub positions: Vec<Position>,
    pub set_map: Vec<usize>,
    pub threads: usize,
    pub delay: Duration,
    pub extractors: Vec<(String, regex::Regex)>,
}

/// Drive one attack to completion (or stop). Runs as a spawned tokio task.
/// `app` is optional so tests can drive the engine without a Tauri app.
pub async fn run_attack(
    app: Option<tauri::AppHandle>,
    state: SharedProxy,
    job: AttackJob,
    ctl: Arc<RunControl>,
) {
    let campaign_id = job.campaign_id;
    let total = ctl.total;
    let threads = job.threads.max(1);
    let mut join: tokio::task::JoinSet<NewIntruderResult> = tokio::task::JoinSet::new();
    let mut idx = 0usize;
    let mut batch: Vec<NewIntruderResult> = Vec::new();
    let mut last_emit = Instant::now();

    let emit = |phase: &str| {
        let snap = ctl.snapshot(campaign_id, phase);
        if let Some(a) = app.as_ref() {
            let _ = a.emit("intruder-progress", snap);
        }
    };

    let flush = |batch: &mut Vec<NewIntruderResult>| {
        if batch.is_empty() {
            return;
        }
        if let Some(db) = state.db.read().clone() {
            let rows: Vec<NewIntruderResult> = batch.drain(..).collect();
            if let Err(e) = db.insert_intruder_results(campaign_id, &rows) {
                eprintln!("intruder result insert failed: {e}");
            }
        }
    };

    while idx < total {
        if ctl.cancelled() {
            break;
        }
        while ctl.paused.load(Ordering::SeqCst) && !ctl.cancelled() {
            tokio::time::sleep(Duration::from_millis(100)).await;
            if last_emit.elapsed() >= Duration::from_millis(250) {
                emit("paused");
                last_emit = Instant::now();
            }
        }
        if ctl.cancelled() {
            break;
        }
        // bounded concurrency: wait for a free slot
        while join.len() >= threads {
            match join.join_next().await {
                Some(Ok(row)) => {
                    if row.error.is_some() {
                        ctl.errors.fetch_add(1, Ordering::SeqCst);
                    }
                    ctl.sent.fetch_add(1, Ordering::SeqCst);
                    batch.push(row);
                    if batch.len() >= RESULT_BATCH {
                        flush(&mut batch);
                    }
                }
                Some(Err(_)) => {
                    ctl.sent.fetch_add(1, Ordering::SeqCst);
                    ctl.errors.fetch_add(1, Ordering::SeqCst);
                }
                None => break,
            }
        }
        if !job.delay.is_zero() {
            tokio::time::sleep(job.delay).await;
        }
        if ctl.cancelled() {
            break;
        }

        let sets = job.sets.clone();
        let positions = job.positions.clone();
        let raw = job.req_raw.clone();
        let url_hint = job.url_hint.clone();
        let follow = job.follow;
        let attack = job.attack;
        let set_map = job.set_map.clone();
        let extractors = job.extractors.clone();
        let n_positions = positions.len();
        let my_idx = idx;
        idx += 1;

        join.spawn(async move {
            let values = plan_at(attack, &sets, n_positions, &set_map, my_idx);
            let built = build_request(&raw, &positions, &values, &sets);
            let fixed = crate::db::fix_content_length(&built);
            let combo: Vec<String> = values
                .iter()
                .map(|v| match v {
                    Some((s, p)) => sets
                        .get(*s)
                        .and_then(|set| set.get(*p))
                        .cloned()
                        .unwrap_or_default(),
                    None => String::new(),
                })
                .collect();
            match send_raw_request_follow(fixed.clone(), url_hint, follow).await {
                Ok(r) => {
                    let resp_raw = render_response(&r);
                    let extracted = extract(&resp_raw, &extractors);
                    NewIntruderResult {
                        seq: my_idx as i64,
                        combo: serde_json::to_string(&combo).unwrap_or_else(|_| "[]".into()),
                        status: Some(r.status as i64),
                        resp_len: resp_raw.len() as i64,
                        time_ms: r.time_ms as i64,
                        error: None,
                        req_raw: fixed,
                        resp_raw: Some(resp_raw),
                        extracted: extracted.to_string(),
                    }
                }
                Err(e) => NewIntruderResult {
                    seq: my_idx as i64,
                    combo: serde_json::to_string(&combo).unwrap_or_else(|_| "[]".into()),
                    status: None,
                    resp_len: 0,
                    time_ms: 0,
                    error: Some(e),
                    req_raw: fixed,
                    resp_raw: None,
                    extracted: "{}".into(),
                },
            }
        });

        if last_emit.elapsed() >= Duration::from_millis(250) {
            emit("running");
            last_emit = Instant::now();
        }
    }

    // drain in-flight requests
    while let Some(res) = join.join_next().await {
        match res {
            Ok(row) => {
                if row.error.is_some() {
                    ctl.errors.fetch_add(1, Ordering::SeqCst);
                }
                ctl.sent.fetch_add(1, Ordering::SeqCst);
                batch.push(row);
                if batch.len() >= RESULT_BATCH {
                    flush(&mut batch);
                }
            }
            Err(_) => {
                ctl.sent.fetch_add(1, Ordering::SeqCst);
                ctl.errors.fetch_add(1, Ordering::SeqCst);
            }
        }
    }
    flush(&mut batch);

    let phase = if ctl.cancelled() { "stopped" } else { "done" };
    if let Some(db) = state.db.read().clone() {
        let status = if phase == "stopped" { "stopped" } else { "done" };
        if let Err(e) = db.set_intruder_campaign_status(campaign_id, status) {
            eprintln!("intruder status update failed: {e}");
        }
    }
    let snap = ctl.snapshot(campaign_id, phase);
    if let Some(a) = app.as_ref() {
        let _ = a.emit("intruder-progress", snap.clone());
    }
    let sent = snap["sent"].as_u64().unwrap_or(0);
    let errors = snap["errors"].as_u64().unwrap_or(0);
    let secs = snap["elapsed_ms"].as_u64().unwrap_or(0) as f64 / 1000.0;
    let level = if errors > 0 { "error" } else { "info" };
    let msg = if phase == "stopped" {
        format!("Intruder attack on campaign {campaign_id} stopped after {sent} requests ({secs:.1}s)")
    } else {
        format!("Intruder attack on campaign {campaign_id} finished: {sent} requests, {errors} errors ({secs:.1}s)")
    };
    if let Some(a) = app.as_ref() {
        crate::commands::log_event(a, &state, "intruder", level, &msg);
    }
    state.intruder_runs.lock().remove(&campaign_id);
}

// ---------------------------------------------------------------- tests

#[cfg(test)]
mod tests {
    use super::*;

    fn set(kind: &str, items: &str) -> IntruderPayloadSet {
        IntruderPayloadSet {
            id: 1,
            campaign_id: 1,
            set_index: 0,
            name: "s".into(),
            kind: kind.into(),
            items: items.into(),
            start_i: None,
            end_i: None,
            step_i: None,
            processors: "[]".into(),
        }
    }

    #[test]
    fn parse_positions_pairs_and_ignores_unclosed() {
        let raw = "GET /?a=§1§&b=2 HTTP/1.1\r\nX-H: §y§\r\n\r\n";
        let pos = parse_positions(raw);
        assert_eq!(pos.len(), 2);
        assert_eq!(pos[0].text, "1");
        assert_eq!(pos[1].text, "y");
        // offsets reconstruct the template
        assert_eq!(&raw[pos[0].open..pos[0].close + 2], "§1§");
        // unclosed marker ignored
        let open_only = "abc §def";
        assert!(parse_positions(open_only).is_empty());
        assert_eq!(strip_positions(open_only), "abc def");
    }

    #[test]
    fn build_request_substitutes_and_keeps_others() {
        let raw = "GET /?a=§1§&b=2 HTTP/1.1\r\n\r\n";
        let pos = parse_positions(raw);
        let sets = vec![vec!["xx".to_string(), "yy".to_string()]];
        let out = build_request(raw, &pos, &[Some((0, 1))], &sets);
        assert_eq!(out, "GET /?a=yy&b=2 HTTP/1.1\r\n\r\n");
        let out2 = build_request(raw, &pos, &[None], &sets);
        assert_eq!(out2, "GET /?a=1&b=2 HTTP/1.1\r\n\r\n");
    }

    #[test]
    fn plan_total_for_all_four_attack_types() {
        let sets = vec![vec!["a".into(), "b".into()], vec!["1".into(), "2".into(), "3".into()]];
        let map = vec![0usize, 1usize];
        assert_eq!(plan_total(AttackType::Sniper, &sets, 2, &map), 5);
        assert_eq!(plan_total(AttackType::BatteringRam, &sets, 2, &map), 2);
        assert_eq!(plan_total(AttackType::Pitchfork, &sets, 2, &map), 2);
        assert_eq!(plan_total(AttackType::ClusterBomb, &sets, 2, &map), 6);
        assert_eq!(plan_total(AttackType::Sniper, &sets, 0, &map), 0);
    }

    #[test]
    fn plan_at_sniper_walks_positions_sequentially() {
        let sets = vec![vec!["a".into(), "b".into()], vec!["1".into(), "2".into(), "3".into()]];
        let map = vec![0usize, 1usize];
        // idx 0,1 → position 0 set0; idx 2..5 → position 1 set1
        assert_eq!(plan_at(AttackType::Sniper, &sets, 2, &map, 0), vec![Some((0, 0)), None]);
        assert_eq!(plan_at(AttackType::Sniper, &sets, 2, &map, 1), vec![Some((0, 1)), None]);
        assert_eq!(plan_at(AttackType::Sniper, &sets, 2, &map, 2), vec![None, Some((1, 0))]);
        assert_eq!(plan_at(AttackType::Sniper, &sets, 2, &map, 4), vec![None, Some((1, 2))]);
    }

    #[test]
    fn plan_at_battering_pitchfork_cluster() {
        let sets = vec![vec!["a".into(), "b".into()], vec!["1".into(), "2".into(), "3".into()]];
        let map = vec![0usize, 1usize];
        // battering ram: same value in both positions (set 0)
        assert_eq!(
            plan_at(AttackType::BatteringRam, &sets, 2, &map, 1),
            vec![Some((0, 1)), Some((0, 1))]
        );
        // pitchfork: lockstep
        assert_eq!(
            plan_at(AttackType::Pitchfork, &sets, 2, &map, 1),
            vec![Some((0, 1)), Some((1, 1))]
        );
        // cluster bomb: mixed radix (set0 size 2, set1 size 3)
        assert_eq!(
            plan_at(AttackType::ClusterBomb, &sets, 2, &map, 0),
            vec![Some((0, 0)), Some((1, 0))]
        );
        assert_eq!(
            plan_at(AttackType::ClusterBomb, &sets, 2, &map, 2),
            vec![Some((0, 0)), Some((1, 1))]
        );
        assert_eq!(
            plan_at(AttackType::ClusterBomb, &sets, 2, &map, 3),
            vec![Some((0, 1)), Some((1, 1))]
        );
        assert_eq!(
            plan_at(AttackType::ClusterBomb, &sets, 2, &map, 5),
            vec![Some((0, 1)), Some((1, 2))]
        );
    }

    #[test]
    fn set_map_fallback_and_clamp() {
        // map shorter than positions or pointing past available sets
        assert_eq!(set_for(&[], 5, 1), 0);
        assert_eq!(set_for(&[7], 0, 1), 0);
        assert_eq!(set_for(&[0], 0, 1), 0);
    }

    #[test]
    fn expand_list_and_numbers_payloads() {
        let mut s = set("list", "a\nb\r\nc\n");
        assert_eq!(expand_payloads(&s).unwrap(), vec!["a", "b", "c"]);
        s = set("list", "");
        assert!(expand_payloads(&s).unwrap().is_empty());
        s.kind = "numbers".into();
        s.start_i = Some(1);
        s.end_i = Some(5);
        s.step_i = Some(2);
        assert_eq!(expand_payloads(&s).unwrap(), vec!["1", "3", "5"]);
        s.step_i = Some(0);
        assert!(expand_payloads(&s).is_err());
        s.step_i = Some(1);
        s.start_i = Some(10);
        s.end_i = Some(1);
        assert!(expand_payloads(&s).is_err());
    }

    #[test]
    fn processors_hash_encode_replace() {
        assert_eq!(apply_processors("abc", &[Processor { kind: "md5".into(), a: None, b: None }]), "900150983cd24fb0d6963f7d28e17f72");
        assert_eq!(apply_processors("abc", &[Processor { kind: "sha1".into(), a: None, b: None }]), "a9993e364706816aba3e25717850c26c9cd0d89d");
        assert_eq!(apply_processors("abc", &[Processor { kind: "sha256".into(), a: None, b: None }]), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
        let enc = Processor { kind: "url_encode".into(), a: None, b: None };
        assert_eq!(apply_processors("a b&c", &[enc.clone()]), "a%20b%26c");
        assert_eq!(apply_processors("a%20b%26c", &[Processor { kind: "url_decode".into(), a: None, b: None }]), "a b&c");
        assert_eq!(apply_processors("hello", &[Processor { kind: "base64_encode".into(), a: None, b: None }]), "aGVsbG8=");
        assert_eq!(apply_processors("aGVsbG8=", &[Processor { kind: "base64_decode".into(), a: None, b: None }]), "hello");
        let rep = Processor { kind: "replace".into(), a: Some("admin".into()), b: Some("root".into()) };
        assert_eq!(apply_processors("x=admin", &[rep]), "x=root");
        let chain = vec![
            Processor { kind: "upper".into(), a: None, b: None },
            Processor { kind: "prefix".into(), a: Some("<".into()), b: None },
            Processor { kind: "suffix".into(), a: Some(">".into()), b: None },
        ];
        assert_eq!(apply_processors("hi", &chain), "<HI>");
        assert_eq!(apply_processors("  pad  ", &[Processor { kind: "trim".into(), a: None, b: None }]), "pad");
        assert_eq!(apply_processors("abc", &[Processor { kind: "reverse".into(), a: None, b: None }]), "cba");
    }

    #[test]
    fn auto_positions_query_body_cookie_json() {
        // query string
        let raw = "GET /login?user=admin&pass=secret HTTP/1.1\r\n\r\n";
        let auto = auto_positions(raw);
        assert_eq!(auto, "GET /login?user=§admin§&pass=§secret§ HTTP/1.1\r\n\r\n");

        // urlencoded body
        let raw = "POST /x HTTP/1.1\r\nContent-Type: application/x-www-form-urlencoded\r\n\r\nuser=bob&role=admin";
        let auto = auto_positions(raw);
        assert!(auto.contains("user=§bob§&role=§admin§"), "{auto}");

        // JSON body string values
        let raw = r#"POST /x HTTP/1.1"#.to_string()
            + "\r\nContent-Type: application/json\r\n\r\n{\"user\":\"bob\",\"n\":5}";
        let auto = auto_positions(raw.as_str());
        assert!(auto.contains("\"user\":\"§bob§\""), "{auto}");
        assert!(!auto.contains("§5§"), "{auto}");

        // cookie header
        let raw = "GET / HTTP/1.1\r\nCookie: sid=abc123; theme=dark\r\n\r\n";
        let auto = auto_positions(raw);
        assert_eq!(auto, "GET / HTTP/1.1\r\nCookie: sid=§abc123§; theme=§dark§\r\n\r\n");

        // idempotent: already marked values are skipped
        let once = auto_positions("GET /?a=1 HTTP/1.1\r\n\r\n");
        let twice = auto_positions(&once);
        assert_eq!(once, twice);
    }

    #[test]
    fn render_response_and_extract() {
        let r = RawResponse {
            status: 200,
            headers: vec![("Content-Type".into(), "text/plain".into())],
            body: "token=abc123".into(),
            time_ms: 5,
            final_url: "http://x/".into(),
            redirects: 0,
        };
        let resp = render_response(&r);
        assert!(resp.starts_with("HTTP/1.1 200 OK\r\n"));
        assert!(resp.ends_with("\r\n\r\ntoken=abc123"));

        let extractors = compile_extractors(r#"[{"name":"tok","pattern":"token=([a-z0-9]+)"}]"#);
        assert_eq!(extractors.len(), 1);
        let out = extract(&resp, &extractors);
        assert_eq!(out["tok"], "abc123");
        // no match → null
        let out2 = extract("nothing here", &extractors);
        assert!(out2["tok"].is_null());
        // bad pattern is skipped at compile time
        assert!(compile_extractors(r#"[{"name":"bad","pattern":"(["}] "#).is_empty());
    }

    #[test]
    fn processed_sets_apply_processors_and_validate() {
        let mut s = set("list", "abc\nsecond");
        s.processors = r#"[{"kind":"md5"}]"#.into();
        let out = processed_sets(&[s]).unwrap();
        assert_eq!(out[0][0], "900150983cd24fb0d6963f7d28e17f72");
        assert_eq!(out[0][1].len(), 32);
        // empty set → error
        let empty = set("list", "");
        assert!(processed_sets(&[empty]).is_err());
        // no sets → error
        assert!(processed_sets(&[]).is_err());
    }

    #[test]
    fn run_control_snapshot_phases() {
        let ctl = RunControl::new(10);
        ctl.sent.store(4, Ordering::SeqCst);
        ctl.errors.store(1, Ordering::SeqCst);
        let snap = ctl.snapshot(7, "running");
        assert_eq!(snap["campaign_id"], 7);
        assert_eq!(snap["sent"], 4);
        assert_eq!(snap["total"], 10);
        assert_eq!(snap["errors"], 1);
        assert_eq!(snap["status"], "running");
        assert_eq!(snap["done"], false);
        let done = ctl.snapshot(7, "done");
        assert_eq!(done["done"], true);
        ctl.pause();
        assert!(ctl.paused.load(Ordering::SeqCst));
        ctl.resume();
        assert!(!ctl.paused.load(Ordering::SeqCst));
        ctl.cancel.store(true, Ordering::SeqCst);
        assert!(ctl.cancelled());
    }
}
