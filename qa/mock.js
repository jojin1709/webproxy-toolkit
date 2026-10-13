// Tauri IPC mock — runs before app scripts so the UI can be exercised in a plain browser.
(() => {
  const scenario = window.__SCENARIO__ || {};

  let status = {
    running: scenario.stopped ? false : true,
    port: 18080,
    intercept: !!scenario.intercept,
    intercept_resp: !!scenario.interceptResp,
    held_count: scenario.held ? 1 : 0,
    history_count: 12,
  };

  const H = (k, v) => [k, v];

  function ex(id, method, host, path, code, mime, tls, inScope, opts = {}) {
    const scheme = tls ? "https" : "http";
    const url = `${scheme}://${host}${path}`;
    const reqHeaders = [
      H("Host", host),
      H("User-Agent", "Mozilla/5.0 (Windows NT 10.0; Win64; x64) WebProxy-Toolkit/0.1"),
      H("Accept", mime === "JSON" ? "application/json" : "*/*"),
      H("Accept-Language", "en-US,en;q=0.9"),
      H("Connection", "keep-alive"),
    ];
    if (opts.post)
      reqHeaders.push(H("Content-Type", "application/x-www-form-urlencoded"), H("Content-Length", String(opts.post.length)));
    const respHeaders = [
      H("Content-Type", mimeContentType(mime)),
      H("Server", opts.server || "nginx/1.24.0"),
      H("Cache-Control", code === 304 ? "max-age=3600" : "no-store"),
      H("Date", "Sun, 11 Oct 2026 09:12:0" + (id % 10) + " GMT"),
    ];
    if (opts.setCookie) respHeaders.push(H("Set-Cookie", opts.setCookie));
    return {
      id,
      started_at: `2026-10-11 09:${String(10 + (id % 40)).padStart(2, "0")}:${String((id * 7) % 60).padStart(2, "0")}`,
      time_ms: opts.time || 30 + ((id * 37) % 400),
      client_addr: "127.0.0.1:" + (50000 + id),
      host,
      method,
      url,
      req_headers: reqHeaders,
      req_body: opts.post ?? null,
      status: code,
      resp_headers: respHeaders,
      resp_body: opts.body ?? mockBody(mime, code),
      tls,
      websocket: false,
      in_scope: inScope,
    };
  }

  function mimeContentType(m) {
    return (
      {
        HTML: "text/html; charset=utf-8",
        JSON: "application/json; charset=utf-8",
        script: "application/javascript; charset=utf-8",
        CSS: "text/css; charset=utf-8",
        image: "image/png",
        text: "text/plain; charset=utf-8",
        bin: "application/octet-stream",
        font: "font/woff2",
        XML: "application/xml",
      }[m] || "application/octet-stream"
    );
  }

  function mockBody(mime, code) {
    if (code === 304 || code === 204) return "";
    if (mime === "JSON") return '{"ok":true,"items":[{"id":1,"name":"Widget"},{"id":2,"name":"Gadget"}],"total":2}';
    if (mime === "HTML")
      return "<!doctype html>\n<html lang=\"en\">\n<head><meta charset=\"utf-8\"><title>Demo shop</title></head>\n<body>\n<h1>Demo shop</h1>\n<p>Sample page body used for UI review.</p>\n</body>\n</html>";
    if (mime === "script") return "(()=>{const e={};export default e;})();";
    if (mime === "CSS") return ":root{--x:1px}.a{color:#333}";
    if (mime === "image") return "PNG-BINARY-DATA...";
    if (mime === "font") return "WOFF2-BINARY...";
    if (mime === "XML") return "<?xml version='1.0'?><rss/>";
    return "plain text body";
  }

  const history = [
    ex(1, "GET", "shop.example.com", "/", 200, "HTML", true, true, { setCookie: "sid=abc123; HttpOnly" }),
    ex(2, "GET", "shop.example.com", "/api/products?page=2", 200, "JSON", true, true),
    ex(3, "POST", "shop.example.com", "/login", 302, "text", true, true, {
      post: "user=alice&pass=hunter2",
      body: "",
    }),
    ex(4, "GET", "cdn.vendor.net", "/js/app.4f2c1d.js", 200, "script", true, false),
    ex(5, "GET", "shop.example.com", "/static/css/main.css", 304, "CSS", true, true),
    ex(6, "GET", "shop.example.com", "/admin", 403, "HTML", true, true, { server: "Apache/2.4.57" }),
    ex(7, "GET", "analytics.track.io", "/collect?e=pageview", 204, "text", true, false),
    ex(8, "GET", "shop.example.com", "/checkout", 200, "HTML", true, true),
    ex(9, "POST", "shop.example.com", "/api/cart", 201, "JSON", true, true, { post: "sku=42&qty=1" }),
    ex(10, "GET", "shop.example.com", "/favicon.ico", 404, "image", true, true),
    ex(11, "GET", "fonts.gstatic.com", "/s/inter/v18/font.woff2", 200, "font", true, false),
    ex(12, "GET", "shop.example.com", "/api/profile", 500, "JSON", true, true, {
      body: '{"error":"internal","trace":"mock"}',
    }),
  ];

  const wsConns = [
    {
      id: 1,
      ts_start: "2026-10-11 09:14:02",
      ts_end: null,
      url: "/realtime",
      host: "shop.example.com",
      port: 443,
      tls: true,
      msgs_out: 3,
      msgs_in: 4,
    },
    {
      id: 2,
      ts_start: "2026-10-11 09:18:40",
      ts_end: "2026-10-11 09:18:52",
      url: "/ws/notifications",
      host: "analytics.track.io",
      port: 443,
      tls: true,
      msgs_out: 1,
      msgs_in: 1,
    },
  ];

  const wsFrames = {
    1: [
      { id: 1, conn_id: 1, ts: "2026-10-11 09:14:03", dir: "c", opcode: 1, len: 27, payload: '{"type":"subscribe","ch":"cart"}' },
      { id: 2, conn_id: 1, ts: "2026-10-11 09:14:03", dir: "s", opcode: 1, len: 31, payload: '{"type":"ack","ch":"cart","ok":true}' },
      { id: 3, conn_id: 1, ts: "2026-10-11 09:14:11", dir: "s", opcode: 1, len: 42, payload: '{"type":"cart","items":2,"total":"49.90"}' },
      { id: 4, conn_id: 1, ts: "2026-10-11 09:14:19", dir: "c", opcode: 9, len: 0, payload: "" },
      { id: 5, conn_id: 1, ts: "2026-10-11 09:14:19", dir: "s", opcode: 10, len: 0, payload: "" },
      { id: 6, conn_id: 1, ts: "2026-10-11 09:14:27", dir: "s", opcode: 2, len: 16, payload: "0001 0203 0405 0607 0809 0a0b 0c0d 0e0f" },
    ],
    2: [
      { id: 7, conn_id: 2, ts: "2026-10-11 09:18:41", dir: "c", opcode: 1, len: 24, payload: '{"type":"hello","v":1}' },
      { id: 8, conn_id: 2, ts: "2026-10-11 09:18:52", dir: "s", opcode: 8, len: 2, payload: "\u0003e8" },
    ],
  };

  const LOGS = [
    { id: 9, ts: "2026-10-11 09:22:31", tool: "repeater", level: "info", message: "GET https://shop.example.com/api/profile -> 500 in 74ms" },
    { id: 8, ts: "2026-10-11 09:21:17", tool: "proxy", level: "info", message: "Recorded GET fonts.gstatic.com/s/inter/v18/font.woff2 (200)" },
    { id: 7, ts: "2026-10-11 09:19:03", tool: "proxy", level: "info", message: "Recorded POST shop.example.com/api/cart (201)" },
    { id: 6, ts: "2026-10-11 09:16:42", tool: "scope", level: "warn", message: "Removed analytics.track.io from scope" },
    { id: 5, ts: "2026-10-11 09:15:35", tool: "proxy", level: "info", message: "Intercept off (requests+responses disabled)" },
    { id: 4, ts: "2026-10-11 09:14:28", tool: "repeater", level: "info", message: "Tab 1 saved" },
    { id: 3, ts: "2026-10-11 09:13:21", tool: "proxy", level: "warn", message: "Held request dropped by user" },
    { id: 2, ts: "2026-10-11 09:12:14", tool: "system", level: "error", message: "sequencer_sample: connection refused (skipped 1 of 20)" },
    { id: 1, ts: "2026-10-11 09:11:07", tool: "proxy", level: "info", message: "Listener started on 127.0.0.1:18080" },
  ];

  const repeaterHistory = {};
  let repeaterHistorySeq = 1;

  const held = {
    id: 1,
    raw_request:
      "POST /api/session HTTP/1.1\r\nHost: shop.example.com\r\nContent-Type: application/x-www-form-urlencoded\r\nContent-Length: 31\r\nOrigin: https://shop.example.com\r\n\r\nusername=alice&password=test123",
    host: "shop.example.com",
    port: 443,
    tls: true,
    direction: "request",
  };

  const REQ =
    "GET https://shop.example.com/api/products?page=2 HTTP/1.1\r\nHost: shop.example.com\r\nUser-Agent: Mozilla/5.0 (Windows NT 10.0; Win64; x64)\r\nAccept: application/json\r\nCookie: sid=abc123\r\nConnection: close\r\n\r\n";
  const RESP =
    'HTTP/1.1 200 OK\r\nContent-Type: application/json; charset=utf-8\r\nServer: nginx/1.24.0\r\n\r\n{"ok":true,"items":[{"id":1,"name":"Widget"},{"id":2,"name":"Gadget"}],"total":2}';

  const sitemapEmpty = !!scenario.sitemapEmpty;
  const sitemap = sitemapEmpty
    ? { roots: [] }
    : {
        roots: [
          {
            host: "shop.example.com",
            children: [
              { path: "/", count: 12 },
              { path: "/api/products?page=2", count: 5 },
              { path: "/login", count: 3 },
              { path: "/checkout", count: 2 },
              { path: "/admin", count: 1 },
            ],
          },
          { host: "cdn.vendor.net", children: [{ path: "/js/app.4f2c1d.js", count: 2 }] },
          { host: "fonts.gstatic.com", children: [{ path: "/s/inter/v18/font.woff2", count: 1 }] },
        ],
      };

  // ---------------- intruder (Phase 3) ----------------
  const MARKC = "§";

  function countPairs(s) {
    let open = false;
    let n = 0;
    for (const ch of s) {
      if (ch !== MARKC) continue;
      if (!open) open = true;
      else {
        n++;
        open = false;
      }
    }
    return n;
  }

  function autoMark(raw) {
    let s = raw;
    if (s.indexOf(MARKC) !== -1) return s;
    const q = s.search(/^[A-Z]+ [^ ]*\?/m);
    if (q !== -1) {
      s = s.replace(/^([A-Z]+ [^ ]*\?)([^\s]*)/m, (m, pre, qs) => pre + qs.replace(/([^&=\s]+)=([^&=\s]+)/g, (p, k, v) => k + "=" + MARKC + v + MARKC));
    }
    s = s.replace(/(Cookie:\s*[^;\r\n]*?=)([^;\r\n]+)/i, (m, pre, v) => pre + MARKC + v + MARKC);
    const br = s.indexOf("\r\n\r\n");
    if (br !== -1) {
      const head = s.slice(0, br + 4);
      let body = s.slice(br + 4);
      if (body && body.indexOf(MARKC) === -1)
        body = body.replace(/([^&=\s]+)=([^&=\s]+)/g, (p, k, v) => k + "=" + MARKC + v + MARKC);
      s = head + body;
    }
    return s;
  }

  function fillReq(req, combo) {
    let i = 0;
    let out = "";
    let open = false;
    let buf = "";
    for (const ch of req) {
      if (ch === MARKC) {
        if (!open) {
          open = true;
          buf = "";
        } else {
          out += combo[i] !== undefined && combo[i] !== "" ? combo[i] : buf;
          i++;
          open = false;
        }
      } else if (open) buf += ch;
      else out += ch;
    }
    return out;
  }

  function payloadValues(s) {
    if (s.kind === "numbers") {
      const st = s.step_i || 1;
      const out = [];
      for (let v = s.start_i ?? 0; v <= (s.end_i ?? -1) && out.length < 500; v += st) out.push(String(v));
      return out;
    }
    return String(s.items || "")
      .split("\n")
      .map((x) => x.trim())
      .filter((x) => x.length > 0);
  }

  function buildCombos(c, sets) {
    const n = countPairs(c.req_raw);
    const vals = sets.map(payloadValues);
    const pick = (p) => {
      const m = c.set_map[p] || 0;
      return vals[m < vals.length ? m : 0] || [];
    };
    const out = [];
    if (n === 0) return out;
    if (c.attack_type === "battering_ram") {
      const v = pick(0);
      for (let i = 0; i < v.length; i++) out.push(Array(n).fill(v[i]));
    } else if (c.attack_type === "pitchfork") {
      let len = Math.min(...Array.from({ length: n }, (_, p) => pick(p).length));
      if (!Number.isFinite(len)) len = 0;
      for (let i = 0; i < len; i++) out.push(Array.from({ length: n }, (_, p) => pick(p)[i]));
    } else if (c.attack_type === "cluster_bomb") {
      const lens = Array.from({ length: n }, (_, p) => pick(p).length);
      const total = lens.reduce((a, b) => a * b, 1);
      for (let t = 0; t < total && t < 500; t++) {
        let x = t;
        const idx = [];
        for (let p = 0; p < n; p++) {
          const L = lens[p] || 1;
          idx.push(x % L);
          x = Math.floor(x / L);
        }
        out.push(idx.map((ix, p) => pick(p)[ix] ?? ""));
      }
    } else {
      for (let p = 0; p < n; p++) {
        const v = pick(p);
        for (let i = 0; i < v.length; i++)
          out.push(Array.from({ length: n }, (_, q) => (q === p ? v[i] : "")));
      }
    }
    const cap = c.max_requests > 0 ? Math.min(out.length, c.max_requests) : out.length;
    return out.slice(0, Math.min(cap, 500));
  }

  function makeResult(c, seq, combo, isErr) {
    const status = seq % 7 === 6 ? 200 : 401;
    const base = {
      seq,
      combo: JSON.stringify(combo),
      status: isErr ? null : status,
      resp_len: isErr ? 0 : 180 + ((seq * 13) % 400),
      time_ms: isErr ? 0 : 25 + ((seq * 37) % 300),
      error: isErr ? "connection reset by peer" : null,
      extracted: JSON.stringify(!isErr && status === 200 ? { session: "s" + (1000 + seq) } : {}),
    };
    return {
      ...base,
      req_raw: fillReq(c.req_raw, combo),
      resp_raw: isErr
        ? null
        : `HTTP/1.1 ${status} ${status === 200 ? "OK" : "Unauthorized"}\r\nContent-Type: application/json; charset=utf-8\r\nServer: nginx/1.24.0\r\n\r\n${
            status === 200 ? '{"ok":true,"user":"admin"}' : '{"error":"invalid credentials"}'
          }`,
    };
  }

  const ic = [
    {
      id: 1,
      name: "Admin brute",
      attack_type: "sniper",
      req_raw:
        "POST https://shop.example.com/api/login HTTP/1.1\r\n" +
        "Host: shop.example.com\r\n" +
        "Content-Type: application/x-www-form-urlencoded\r\n" +
        "Cookie: sid=" + MARKC + "abc123" + MARKC + "\r\n" +
        "Connection: close\r\n\r\n" +
        "user=" + MARKC + "admin" + MARKC + "&pass=" + MARKC + "x" + MARKC,
      url_hint: "https://shop.example.com/api/login",
      follow: false,
      set_map: [0, 0, 1],
      threads: 4,
      delay_ms: 0,
      max_requests: 0,
      extractors: JSON.stringify([
        { name: "session", pattern: "Set-Cookie: (\\w+)=" },
        { name: "title", pattern: "<title>([^<]+)</title>" },
      ]),
      status: "done",
      created_at: "2026-10-13 10:02:00",
      updated_at: "2026-10-13 10:14:00",
    },
    {
      id: 2,
      name: "IDOR probe",
      attack_type: "sniper",
      req_raw:
        "GET https://shop.example.com/api/orders?page=1 HTTP/1.1\r\n" +
        "Host: shop.example.com\r\n" +
        "Cookie: session=deadbeef\r\n" +
        "Connection: close\r\n\r\n",
      url_hint: "https://shop.example.com/api/orders",
      follow: false,
      set_map: [0],
      threads: 4,
      delay_ms: 0,
      max_requests: 0,
      extractors: "[]",
      status: "draft",
      created_at: "2026-10-13 10:20:00",
      updated_at: "2026-10-13 10:20:00",
    },
  ];

  const iSets = {
    1: [
      {
        id: 1, campaign_id: 1, set_index: 0, name: "Usernames", kind: "list",
        items: "admin\nroot\nguest\ntest", start_i: null, end_i: null, step_i: null,
        processors: JSON.stringify([{ kind: "url_encode" }]),
      },
      {
        id: 2, campaign_id: 1, set_index: 1, name: "Passwords", kind: "list",
        items: "password\n123456\nadmin123", start_i: null, end_i: null, step_i: null,
        processors: "[]",
      },
    ],
    2: [
      {
        id: 3, campaign_id: 2, set_index: 0, name: "Order ids", kind: "numbers",
        items: "", start_i: 100, end_i: 110, step_i: 1, processors: "[]",
      },
    ],
  };

  const iResults = { 1: [], 2: [] };
  (function seedResults() {
    const c = ic[0];
    const combos = buildCombos(c, iSets[1]);
    iResults[1] = combos.slice(0, 8).map((combo, seq) => makeResult(c, seq, combo, seq === 5));
  })();

  const intruderRuns = {};

  function intruderSnap(cid) {
    const r = intruderRuns[cid];
    if (!r) return null;
    const elapsed = Date.now() - r.t0;
    return {
      campaign_id: cid,
      sent: r.sent,
      total: r.total,
      errors: r.errors,
      status: r.status,
      done: r.status === "done" || r.status === "stopped" || r.status === "error",
      elapsed_ms: elapsed,
      rps: r.sent > 0 && elapsed > 0 ? Math.round((r.sent / (elapsed / 1000)) * 10) / 10 : 0,
    };
  }

  function intruderIdle(c) {
    return {
      campaign_id: c.id,
      status: c.status,
      sent: 0,
      total: 0,
      errors: 0,
      done: c.status !== "running" && c.status !== "paused",
      elapsed_ms: 0,
      rps: 0,
    };
  }

  function intruderFinish(cid, status) {
    const r = intruderRuns[cid];
    if (!r) return;
    clearInterval(r.timer);
    r.status = status;
    const c = ic.find((x) => x.id === cid);
    if (c) c.status = status;
    emit("intruder-progress", intruderSnap(cid));
  }

  const handlers = {
    proxy_status: () => ({ ...status }),
    proxy_start: (a) => {
      status = { ...status, running: true, port: (a && a.port) || status.port };
      emit("proxy-status", { ...status });
      return status.port;
    },
    proxy_stop: () => {
      status = { ...status, running: false };
      emit("proxy-status", { ...status });
      return null;
    },
    intercept_set: (a) => {
      status = { ...status, intercept: !!(a && a.enabled), intercept_resp: !!(a && a.response) };
      emit("proxy-status", { ...status });
      return { ...status };
    },
    intercept_peek: () => (status.held_count > 0 ? { ...held } : null),
    intercept_forward: () => {
      status = { ...status, held_count: 0 };
      emit("proxy-status", { ...status });
      return null;
    },
    intercept_drop: () => {
      status = { ...status, held_count: 0 };
      emit("proxy-status", { ...status });
      return null;
    },
    history_list: () => history,
    history_get: (a) => history.find((e) => e.id === (a && a.id)) || null,
    ws_conns_list: () => wsConns,
    ws_frames_list: (a) => (wsFrames[(a && a.connId) || 1] || []),
    global_search: (a) => {
      const q = String((a && a.query) || "").trim().toLowerCase();
      if (!q) return [];
      const out = [];
      for (let i = history.length - 1; i >= 0 && out.length < 200; i--) {
        const e = history[i];
        const hay = [
          e.url,
          e.host,
          e.method,
          String(e.status ?? ""),
          e.req_headers.map(([k, v]) => `${k}: ${v}`).join("\n"),
          e.req_body || "",
          e.resp_headers.map(([k, v]) => `${k}: ${v}`).join("\n"),
          e.resp_body || "",
        ].join("\n");
        const at = hay.toLowerCase().indexOf(q);
        if (at === -1) continue;
        const flat = hay.replace(/[\r\n]+/g, " ");
        const fat = flat.toLowerCase().indexOf(q);
        const s = Math.max(0, fat - 48);
        out.push({
          source: "history",
          id: e.id,
          title: `${e.method} ${e.url}`,
          detail: `${e.host} · ${e.status ?? "-"} · ${e.time_ms} ms`,
          snippet: flat.slice(s, s + 140),
          tab: "proxy",
        });
      }
      for (const l of LOGS) {
        if (out.length >= 200) break;
        if (!l.message.toLowerCase().includes(q)) continue;
        out.push({
          source: "log",
          id: l.id,
          title: l.message,
          detail: `${l.tool} · ${l.level} · ${l.ts}`,
          snippet: l.message,
          tab: "logger",
        });
      }
      return out;
    },
    repeater_history_add: (a) => {
      const tid = (a && a.tabId) || 1;
      repeaterHistory[tid] = repeaterHistory[tid] || [];
      repeaterHistory[tid].unshift({
        id: repeaterHistorySeq++,
        tab_id: tid,
        sent_at: "2026-10-11 09:30:00",
        req_raw: (a && a.reqRaw) || "",
        resp_raw: (a && a.respRaw) || null,
        status: (a && a.status) ?? null,
        time_ms: (a && a.timeMs) ?? null,
      });
      repeaterHistory[tid] = repeaterHistory[tid].slice(0, 100);
      return null;
    },
    repeater_history_list: (a) => repeaterHistory[(a && a.tabId) || 1] || [],
    match_list: () => [
      {
        id: 1,
        enabled: true,
        phase: "request",
        match_kind: "literal",
        match_val: "X-Old-Token: 1",
        replace_val: "X-New-Token: 2",
        comment: "demo rule",
      },
    ],
    match_add: () => null,
    match_delete: () => null,
    scope_list: () => [{ id: 1, mode: "include", kind: "host", pattern: "shop.example.com", enabled: true }],
    scope_add: () => null,
    scope_delete: () => null,
    sitemap_tree: () => sitemap,
    repeater_tabs_list: () => [
      {
        id: 1,
        name: "1",
        req_raw: REQ,
        last_status: 200,
        last_time_ms: 143,
        last_resp_raw: RESP,
        sort_order: 0,
      },
    ],
    repeater_tab_save: (a) => (a && a.id) || 1,
    repeater_tab_delete: () => null,
    repeater_send: (a) =>
      a && a.follow
        ? {
            status: 200,
            headers: [
              ["content-type", "text/html; charset=utf-8"],
              ["server", "mock/1.0"],
            ],
            body: "<!doctype html><html><body><h1>Mock response</h1></body></html>",
            time_ms: 91,
            redirects: 2,
            final_url: "https://shop.example.com/checkout",
          }
        : {
            status: 302,
            headers: [
              ["location", "https://shop.example.com/checkout"],
              ["content-length", "0"],
            ],
            body: "",
            time_ms: 44,
            redirects: 0,
            final_url: null,
          },
    init_ca: () => "C:\\Users\\qa\\AppData\\Local\\WebProxyToolkit\\ca.crt",
    log_list: () => LOGS,
    log_clear: () => null,
    log_add: () => null,
    sequencer_sample: () => [
      "aZ3kQ9pLmX7vB2nR", "wE5tY8uI1oP4aS6d", "fG0hJ2kL5zX8cV9b", "nM3qW6eR9tY0uI4o",
      "pA7sD0fG3hJ6kL1z", "xC9vB2nM5qW8eR4t", "yU6iO9pA2sD5fG8h", "jK1lZ4xC7vB0nM3q",
      "W5eR8tY1uI4oP7a", "S2dF5gH8jK0lZ3x", "C6vB9nM2qW5eR8t", "Y1uI4oP7aS0dF3g",
      "H5jK8lZ1xC4vB7n", "M0qW3eR6tY9uI2o", "P5aS8dF1gH4jK7l", "Z0xC3vB6nM9qW2e",
      "R5tY8uI1oP4aS7d", "F0gH3jK6lZ9xC2v", "B5nM8qW1eR4tY7u", "I0oP3aS6dF9gH2j",
    ],
    intruder_campaign_list: () =>
      ic.map((c) => ({
        id: c.id,
        name: c.name,
        attack_type: c.attack_type,
        status: c.status,
        positions: countPairs(c.req_raw),
        updated_at: c.updated_at,
      })),
    intruder_campaign_new: (a) => {
      const id = Math.max(...ic.map((c) => c.id), 0) + 1;
      ic.push({
        id,
        name: (a && a.name) || "Campaign " + id,
        attack_type: "sniper",
        req_raw: autoMark((a && a.raw) || ""),
        url_hint: (a && a.urlHint) || null,
        follow: false,
        set_map: [],
        threads: 4,
        delay_ms: 0,
        max_requests: 0,
        extractors: "[]",
        status: "draft",
        created_at: "2026-10-13 10:30:00",
        updated_at: "2026-10-13 10:30:00",
      });
      iSets[id] = [];
      iResults[id] = [];
      return id;
    },
    intruder_campaign_get: (a) => {
      const c = ic.find((x) => x.id === ((a && a.campaignId) || 0));
      if (!c) throw new Error("campaign not found");
      return {
        campaign: c,
        payloads: iSets[c.id] || [],
        state: intruderRuns[c.id] ? intruderSnap(c.id) : intruderIdle(c),
      };
    },
    intruder_campaign_save: (a) => {
      const c = ic.find((x) => x.id === ((a && a.campaignId) || 0));
      if (!c) throw new Error("campaign not found");
      if (a.name && a.name.trim()) c.name = a.name.trim();
      c.attack_type = a.attackType || c.attack_type;
      c.req_raw = a.reqRaw;
      c.url_hint = a.urlHint || null;
      c.follow = !!a.follow;
      c.set_map = Array.isArray(a.setMap) ? a.setMap.slice() : [];
      c.threads = a.threads || 4;
      c.delay_ms = a.delayMs || 0;
      c.max_requests = a.maxRequests || 0;
      const exts = Array.isArray(a.extractors) ? a.extractors : [];
      for (const e of exts) {
        try {
          new RegExp(e.pattern);
        } catch (err) {
          throw new Error('extractor "' + e.name + '": ' + String(err));
        }
      }
      c.extractors = JSON.stringify(exts);
      c.updated_at = "2026-10-13 10:31:00";
      return null;
    },
    intruder_campaign_delete: (a) => {
      const id = (a && a.campaignId) || 0;
      const at = ic.findIndex((x) => x.id === id);
      if (at >= 0) ic.splice(at, 1);
      delete iSets[id];
      delete iResults[id];
      return null;
    },
    intruder_positions_auto: (a) => autoMark((a && a.reqRaw) || ""),
    intruder_payloads_save: (a) => {
      const id = (a && a.campaignId) || 0;
      iSets[id] = (a && Array.isArray(a.sets) ? a.sets : []).map((s, i) => ({
        id: id * 100 + (s.index ?? i) + 1,
        campaign_id: id,
        set_index: s.index ?? i,
        name: s.name || "Set " + ((s.index ?? i) + 1),
        kind: s.kind === "numbers" ? "numbers" : "list",
        items: s.items || "",
        start_i: s.start ?? null,
        end_i: s.end ?? null,
        step_i: s.step ?? null,
        processors: JSON.stringify(s.processors || []),
      }));
      return null;
    },
    intruder_start: (a) => {
      const cid = (a && a.campaignId) || 1;
      const c = ic.find((x) => x.id === cid);
      if (!c) throw new Error("campaign not found");
      if (countPairs(c.req_raw) === 0)
        throw new Error("no insertion points - use Auto-detect or Add marker before starting");
      const sets = iSets[cid] || [];
      if (sets.length === 0)
        throw new Error("no payload sets - open Payloads and add payloads first");
      const combos = buildCombos(c, sets);
      if (combos.length === 0)
        throw new Error("attack would send 0 requests - check payload set sizes");
      c.status = "running";
      iResults[cid] = [];
      const r = (intruderRuns[cid] = {
        sent: 0,
        errors: 0,
        total: combos.length,
        status: "running",
        t0: Date.now(),
        timer: null,
        combos,
        rows: [],
        cid,
      });
      r.timer = setInterval(() => {
        if (r.status !== "running") return;
        for (let k = 0; k < 2 && r.sent < r.total; k++) {
          const seq = r.sent;
          const isErr = seq === 5;
          r.rows.push(makeResult(c, seq, r.combos[seq] || [], isErr));
          if (isErr) r.errors++;
          r.sent = seq + 1;
        }
        iResults[cid] = r.rows;
        if (r.sent >= r.total) intruderFinish(cid, "done");
        else emit("intruder-progress", intruderSnap(cid));
      }, 180);
      return intruderSnap(cid);
    },
    intruder_pause: (a) => {
      const cid = (a && a.campaignId) || 1;
      const r = intruderRuns[cid];
      if (!r) throw new Error("no active attack for this campaign");
      r.status = "paused";
      const c = ic.find((x) => x.id === cid);
      if (c) c.status = "paused";
      const snap = intruderSnap(cid);
      emit("intruder-progress", snap);
      return snap;
    },
    intruder_resume: (a) => {
      const cid = (a && a.campaignId) || 1;
      const r = intruderRuns[cid];
      if (!r) throw new Error("no active attack for this campaign");
      r.status = "running";
      const c = ic.find((x) => x.id === cid);
      if (c) c.status = "running";
      const snap = intruderSnap(cid);
      emit("intruder-progress", snap);
      return snap;
    },
    intruder_stop: (a) => {
      const cid = (a && a.campaignId) || 1;
      if (!intruderRuns[cid]) throw new Error("no active attack for this campaign");
      intruderFinish(cid, "stopped");
      return intruderSnap(cid);
    },
    intruder_state: (a) => {
      const cid = (a && a.campaignId) || 1;
      if (intruderRuns[cid] && !intruderRuns[cid].status.match(/done|stopped/))
        return intruderSnap(cid);
      const c = ic.find((x) => x.id === cid);
      return c ? intruderIdle(c) : null;
    },
    intruder_results_list: (a) => {
      const cid = (a && a.campaignId) || 1;
      const all = iResults[cid] || [];
      const off = (a && a.offset) || 0;
      const lim = (a && a.limit) || 500;
      return {
        rows: all.slice(off, off + lim).map(({ req_raw, resp_raw, ...r }) => r),
        total: all.length,
      };
    },
    intruder_result_get: (a) => {
      const cid = (a && a.campaignId) || 1;
      const seq = (a && a.seq) || 0;
      return (iResults[cid] || []).find((r) => r.seq === seq) || null;
    },
    intruder_results_clear: (a) => {
      const cid = (a && a.campaignId) || 1;
      const n = (iResults[cid] || []).length;
      iResults[cid] = [];
      return n;
    },
  };

  const callbacks = {};
  const eventHandlers = {}; // event name -> [callback ids]
  let cbSeq = 1;

  function emit(event, payload) {
    (eventHandlers[event] || []).forEach((id) => {
      const cb = callbacks[id];
      if (cb) cb({ event, id: 0, payload });
    });
  }

  window.__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener: () => {} };

  window.__TAURI_INTERNALS__ = {
    invoke: async (cmd, args) => {
      if (cmd === "plugin:event|listen") {
        const ev = args.event;
        const cbId = args.handler;
        (eventHandlers[ev] = eventHandlers[ev] || []).push(cbId);
        return cbId; // eventId
      }
      if (cmd === "plugin:event|unlisten") return null;
      if (cmd === "plugin:event|emit") {
        emit(args.event, args.payload);
        return null;
      }
      const h = handlers[cmd];
      return h ? h(args) : [];
    },
    transformCallback: (cb) => {
      const id = cbSeq++;
      callbacks[id] = cb;
      return id;
    },
    metadata: { currentWindow: { label: "main" } },
  };
})();
