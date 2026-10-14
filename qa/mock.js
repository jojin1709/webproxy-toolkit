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

  const SCANNER_CHECKS = [
    { id: "missing-csp", name: "Content Security Policy not set", kind: "passive", severity: "medium", description: "The response does not include a Content-Security-Policy header.", remediation: "Add a Content-Security-Policy header restricting script, style and object sources." },
    { id: "missing-hsts", name: "HTTP Strict Transport Security not set", kind: "passive", severity: "medium", description: "The HTTPS response lacks a Strict-Transport-Security header.", remediation: "Add Strict-Transport-Security: max-age=31536000; includeSubDomains." },
    { id: "missing-xfo", name: "X-Frame-Options not set", kind: "passive", severity: "low", description: "The response lacks X-Frame-Options, allowing clickjacking.", remediation: "Add X-Frame-Options: DENY or SAMEORIGIN (or frame-ancestors in CSP)." },
    { id: "missing-xcto", name: "X-Content-Type-Options not set", kind: "passive", severity: "info", description: "The response lacks X-Content-Type-Options: nosniff.", remediation: "Add X-Content-Type-Options: nosniff." },
    { id: "missing-referrer-policy", name: "Referrer-Policy not set", kind: "passive", severity: "info", description: "The response lacks a Referrer-Policy header.", remediation: "Add Referrer-Policy: strict-origin-when-cross-origin." },
    { id: "cookie-no-httponly", name: "Cookie without HttpOnly", kind: "passive", severity: "medium", description: "A cookie is set without the HttpOnly flag.", remediation: "Set HttpOnly on all session cookies." },
    { id: "cookie-no-secure", name: "Cookie without Secure", kind: "passive", severity: "low", description: "A cookie is set without the Secure flag.", remediation: "Set the Secure flag on cookies delivered over HTTPS." },
    { id: "server-banner", name: "Server banner discloses software", kind: "passive", severity: "info", description: "The Server header discloses the web server product and version.", remediation: "Suppress or genericise the Server header." },
    { id: "x-powered-by", name: "X-Powered-By discloses framework", kind: "passive", severity: "info", description: "The X-Powered-By header discloses the application framework.", remediation: "Remove the X-Powered-By header." },
    { id: "directory-listing", name: "Directory listing enabled", kind: "passive", severity: "medium", description: "The response appears to be an auto-generated directory index.", remediation: "Disable directory listing on the web server." },
    { id: "sql-error-leak", name: "SQL error message leaked", kind: "passive", severity: "critical", description: "The response body contains a database error string, suggesting a possible injection point.", remediation: "Use parameterised queries and generic error pages; never return raw database errors to clients." },
    { id: "stack-trace-leak", name: "Stack trace leaked", kind: "passive", severity: "high", description: "The response body contains a stack trace or exception dump.", remediation: "Return generic error pages in production; log exceptions server-side only." },
    { id: "email-disclosure", name: "Email address disclosed", kind: "passive", severity: "info", description: "The response body contains one or more email addresses.", remediation: "Review whether the address should be public; consider contact forms instead." },
    { id: "internal-ip-disclosure", name: "Internal IP address disclosed", kind: "passive", severity: "low", description: "The response body contains a private RFC1918 IP address.", remediation: "Remove internal addressing details from public responses." },
    { id: "mixed-content", name: "Mixed content on HTTPS page", kind: "passive", severity: "medium", description: "An HTTPS page references insecure http:// resources.", remediation: "Serve all subresources over HTTPS." },
    { id: "basic-auth-plaintext", name: "HTTP Basic authentication over plaintext", kind: "passive", severity: "high", description: "Credentials are sent via Basic auth without TLS.", remediation: "Enforce HTTPS for all authentication." },
    { id: "password-in-url", name: "Password in URL", kind: "passive", severity: "high", description: "A password appears in the request URL.", remediation: "Move credentials into the request body and use POST over HTTPS." },

    { id: "sqli-error", name: "SQL injection (error-based)", kind: "active", severity: "high", description: "A SQL metacharacter injected into a parameter produced a database error signature in the response, indicating a possible injection point.", remediation: "Use parameterised queries / prepared statements for all database access; validate and encode user input." },
    { id: "xss-reflected", name: "Reflected cross-site scripting", kind: "active", severity: "high", description: "A canary string injected into a parameter was reflected in the response unencoded, so an attacker-controlled script could execute in a victim's browser.", remediation: "Context-encode all untrusted data at output (HTML entity, JS, URL, CSS context as appropriate); deploy a CSP as defence in depth." },
    { id: "path-traversal", name: "Path traversal", kind: "active", severity: "high", description: "A ../-style payload in a parameter caused the server to return contents of a sensitive system file (/etc/passwd or win.ini), indicating a directory traversal flaw.", remediation: "Canonicalise and validate paths server-side; never build filesystem paths directly from user input; run the service with least privilege." },
    { id: "open-redirect", name: "Open redirect", kind: "active", severity: "medium", description: "A parameter controlling a redirect accepted an attacker-controlled absolute URL, so the endpoint can be used to bounce victims to arbitrary sites (phishing).", remediation: "Validate redirect targets against an allow-list of paths/hosts; never redirect to a full URL supplied in user input." },
    { id: "sensitive-files", name: "Sensitive file or directory exposed", kind: "active", severity: "info", description: "A well-known sensitive path (e.g. /.git/config, /.env, /backup.zip) returned HTTP 200, exposing configuration or backups.", remediation: "Block access to VCS metadata, env files and backup archives at the web server / WAF layer." },
  ];

  let issues = [
    {
      id: 1,
      check_id: "sql-error-leak",
      name: "SQL error message leaked",
      severity: "critical",
      confidence: "firm",
      host: "shop.example.com",
      url: "https://shop.example.com/login",
      method: "POST",
      path: "/login",
      param: "user",
      description: "The response to the login request contains a database error string, indicating the input may reach a SQL query unsanitised.",
      remediation: "Use parameterised queries and generic error pages; never return raw database errors to clients.",
      evidence: "You have an error in your SQL syntax; check the manual that corresponds to your MySQL server version",
      exchange_id: 3,
      status: "open",
      created_at: "2026-10-11 09:12:21",
    },
    {
      id: 2,
      check_id: "stack-trace-leak",
      name: "Stack trace leaked",
      severity: "high",
      confidence: "certain",
      host: "shop.example.com",
      url: "https://shop.example.com/api/profile",
      method: "GET",
      path: "/api/profile",
      param: "",
      description: "The 500 response body contains a stack trace, exposing internal implementation details.",
      remediation: "Return generic error pages in production; log exceptions server-side only.",
      evidence: 'trace": "mock',
      exchange_id: 12,
      status: "open",
      created_at: "2026-10-11 09:49:07",
    },
    {
      id: 3,
      check_id: "missing-csp",
      name: "Content Security Policy not set",
      severity: "medium",
      confidence: "certain",
      host: "shop.example.com",
      url: "https://shop.example.com/",
      method: "GET",
      path: "/",
      param: "",
      description: "The HTML response does not include a Content-Security-Policy header, leaving the page open to XSS amplification.",
      remediation: "Add a Content-Security-Policy header restricting script, style and object sources.",
      evidence: "no Content-Security-Policy header present",
      exchange_id: 1,
      status: "open",
      created_at: "2026-10-11 09:12:10",
    },
    {
      id: 4,
      check_id: "missing-hsts",
      name: "HTTP Strict Transport Security not set",
      severity: "medium",
      confidence: "certain",
      host: "shop.example.com",
      url: "https://shop.example.com/",
      method: "GET",
      path: "/",
      param: "",
      description: "The HTTPS response lacks a Strict-Transport-Security header, allowing protocol-downgrade attacks.",
      remediation: "Add Strict-Transport-Security: max-age=31536000; includeSubDomains.",
      evidence: "no Strict-Transport-Security header present",
      exchange_id: 1,
      status: "open",
      created_at: "2026-10-11 09:12:10",
    },
    {
      id: 5,
      check_id: "cookie-no-secure",
      name: "Cookie without Secure",
      severity: "low",
      confidence: "firm",
      host: "shop.example.com",
      url: "https://shop.example.com/",
      method: "GET",
      path: "/",
      param: "",
      description: "The session cookie 'sid' is set without the Secure flag.",
      remediation: "Set the Secure flag on cookies delivered over HTTPS.",
      evidence: "Set-Cookie: sid=abc123; HttpOnly",
      exchange_id: 1,
      status: "open",
      created_at: "2026-10-11 09:12:10",
    },
    {
      id: 6,
      check_id: "server-banner",
      name: "Server banner discloses software",
      severity: "info",
      confidence: "certain",
      host: "shop.example.com",
      url: "https://shop.example.com/admin",
      method: "GET",
      path: "/admin",
      param: "",
      description: "The Server header discloses Apache/2.4.57, revealing the product and exact version.",
      remediation: "Suppress or genericise the Server header.",
      evidence: "Server: Apache/2.4.57",
      exchange_id: 6,
      status: "open",
      created_at: "2026-10-11 09:30:42",
    },
    {
      id: 7,
      check_id: "missing-xfo",
      name: "X-Frame-Options not set",
      severity: "low",
      confidence: "certain",
      host: "shop.example.com",
      url: "https://shop.example.com/checkout",
      method: "GET",
      path: "/checkout",
      param: "",
      description: "The response lacks X-Frame-Options, allowing the page to be framed by third-party origins (clickjacking).",
      remediation: "Add X-Frame-Options: DENY or SAMEORIGIN (or frame-ancestors in CSP).",
      evidence: "no X-Frame-Options header present",
      exchange_id: 8,
      status: "acknowledged",
      created_at: "2026-10-11 09:35:18",
    },
  ];

  let scanTaskSeq = 1;
  const scanTasks = [
    {
      id: 1,
      kind: "crawl-audit",
      target: "https://shop.example.com/",
      config_id: null,
      status: "done",
      pages_crawled: 14,
      requests_sent: 42,
      issues_found: 7,
      error: null,
      created_at: "2026-10-11 09:20:00",
      updated_at: "2026-10-11 09:21:30",
    },
  ];

  let scanConfigSeq = 2;
  const scanConfigs = [
    {
      id: 1,
      name: "Crawl + audit",
      passive_enabled: true,
      active_checks: [],
      crawl_max_pages: 50,
      crawl_max_depth: 3,
      audit_threads: 4,
      throttle_ms: 50,
      is_default: true,
      created_at: "2026-10-11 09:00:00",
    },
  ];

  let extSeq = 2;
  const extensions = [
    {
      id: 1,
      name: "hello",
      path: "C:\\ext\\hello.js",
      enabled: true,
      error: null,
      loaded_at: "2026-10-11 09:05:00",
      created_at: "2026-10-11 09:05:00",
      hook_errors: 0,
    },
    {
      id: 2,
      name: "broken",
      path: "C:\\ext\\broken.js",
      enabled: true,
      error: "eval error: boom",
      loaded_at: null,
      created_at: "2026-10-11 09:06:00",
      hook_errors: 1,
    },
  ];
  const extHooks = {
    context_menus: [{ ext_id: 1, menu_id: 0, label: "Hash selection", scope: "message" }],
    tabs: [{ ext_id: 1, tab_id: 0, title: "Hello tab" }],
  };

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
    { id: 13, ts: "2026-10-11 09:24:52", tool: "scanner", level: "info", message: "Crawl of https://shop.example.com/ finished: 14 pages, 42 requests" },
    { id: 12, ts: "2026-10-11 09:24:40", tool: "scanner", level: "warn", message: "Reflected XSS candidate on /search?q= (param q)" },
    { id: 11, ts: "2026-10-11 09:24:38", tool: "scanner", level: "warn", message: "SQL error leak on /api/products?id= (param id)" },
    { id: 10, ts: "2026-10-11 09:24:12", tool: "scanner", level: "info", message: "Audit started against https://shop.example.com/ (crawl-audit)" },
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
    log_list: (a) => {
      const f = (a && a.filter) || {};
      return LOGS.filter((e) => {
        if (f.tool && e.tool !== f.tool) return false;
        if (f.level && e.level !== f.level) return false;
        if (f.query && !e.message.toLowerCase().includes(String(f.query).toLowerCase())) return false;
        return true;
      }).slice(0, f.limit || LOGS.length);
    },
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

    // ---- scanner (phase 4a: issues + passive checks) ----
    checks_list: () => SCANNER_CHECKS.slice(),
    issues_list: (a) => {
      const sevs = (a && a.severities) || null;
      const host = (a && a.host) || "";
      const status = (a && a.status) || "";
      const text = ((a && a.text) || "").toLowerCase();
      const off = (a && a.offset) || 0;
      const lim = (a && a.limit) || 500;
      let rows = issues.slice();
      if (sevs && sevs.length) rows = rows.filter((i) => sevs.includes(i.severity));
      if (host) rows = rows.filter((i) => i.host === host);
      if (status) rows = rows.filter((i) => i.status === status);
      if (text)
        rows = rows.filter(
          (i) =>
            i.name.toLowerCase().includes(text) ||
            i.url.toLowerCase().includes(text) ||
            i.param.toLowerCase().includes(text) ||
            i.evidence.toLowerCase().includes(text)
        );
      return { rows: rows.slice(off, off + lim), total: rows.length };
    },
    issues_get: (a) => issues.find((i) => i.id === (a && a.id)) || null,
    issues_update_status: (a) => {
      const i = issues.find((x) => x.id === (a && a.id));
      if (!i) return 0;
      i.status = (a && a.status) || i.status;
      emit("issue-updated", i);
      return 1;
    },
    issues_delete: (a) => {
      const id = (a && a.id) || 0;
      const idx = issues.findIndex((x) => x.id === id);
      if (idx < 0) return 0;
      issues.splice(idx, 1);
      emit("issue-deleted", id);
      return 1;
    },
    issues_counts: () => {
      const c = { critical: 0, high: 0, medium: 0, low: 0, info: 0 };
      issues.forEach((i) => {
        c[i.severity] = (c[i.severity] || 0) + 1;
      });
      return c;
    },

    // ---- scan tasks (phase 4b) ----
    scan_start: (a) => {
      const target = (a && a.target) || "https://shop.example.com/";
      const kind = (a && a.kind) || "crawl-audit";
      const id = scanTaskSeq++;
      const now = new Date().toISOString().replace("T", " ").slice(0, 19);
      scanTasks.push({
        id,
        kind,
        target,
        config_id: (a && a.configId) ?? null,
        status: "running",
        pages_crawled: 0,
        requests_sent: 0,
        issues_found: 0,
        error: null,
        created_at: now,
        updated_at: now,
      });
      // simulate progress
      const t = scanTasks.find((x) => x.id === id);
      const timer = setInterval(() => {
        if (!t || t.status !== "running") {
          clearInterval(timer);
          return;
        }
        t.pages_crawled += 2;
        t.requests_sent += 5;
        t.issues_found += t.pages_crawled === 2 ? 1 : 0;
        t.updated_at = new Date().toISOString().replace("T", " ").slice(0, 19);
        if (t.pages_crawled >= 20) {
          t.status = "done";
          clearInterval(timer);
        }
        emit("scan-progress", {
          task_id: id,
          pages: t.pages_crawled,
          requests: t.requests_sent,
          issues: t.issues_found,
          status: t.status,
          queued: Math.max(0, 10 - t.pages_crawled / 2),
        });
      }, 400);
      return id;
    },
    scan_cancel: (a) => {
      const id = (a && a.taskId) || 0;
      const t = scanTasks.find((x) => x.id === id);
      if (!t) return null;
      t.status = "cancelled";
      emit("scan-progress", {
        task_id: id,
        pages: t.pages_crawled,
        requests: t.requests_sent,
        issues: t.issues_found,
        status: "cancelled",
      });
      return null;
    },
    scan_pause: (a) => {
      const id = (a && a.taskId) || 0;
      const t = scanTasks.find((x) => x.id === id);
      if (t) t.status = "paused";
      return null;
    },
    scan_resume: (a) => {
      const id = (a && a.taskId) || 0;
      const t = scanTasks.find((x) => x.id === id);
      if (t) t.status = "running";
      return null;
    },
    scan_tasks_list: () => scanTasks.slice(),
    scan_state: () => ({
      running: scanTasks
        .filter((t) => t.status === "running" || t.status === "paused")
        .map((t) => ({
          task_id: t.id,
          pages: t.pages_crawled,
          requests: t.requests_sent,
          issues: t.issues_found,
          paused: t.status === "paused",
          elapsed_ms: 1200,
        })),
    }),

    // ---- scan configs (phase 4c) ----
    scan_config_list: () =>
      scanConfigs
        .slice()
        .sort((a, b) => (b.is_default ? 1 : 0) - (a.is_default ? 1 : 0) || a.name.localeCompare(b.name)),
    scan_config_get: (a) => {
      const id = (a && a.configId) || 0;
      const c = scanConfigs.find((x) => x.id === id);
      if (!c) throw new Error(`scan config ${id} not found`);
      return c;
    },
    scan_config_save: (a) => {
      const cfg = (a && a.config) || {};
      const name = (cfg.name || "").trim();
      if (!name) throw new Error("config name is required");
      if (cfg.is_default) scanConfigs.forEach((c) => (c.is_default = false));
      if (cfg.id) {
        const c = scanConfigs.find((x) => x.id === cfg.id);
        if (!c) throw new Error(`scan config ${cfg.id} not found`);
        Object.assign(c, {
          name,
          passive_enabled: !!cfg.passive_enabled,
          active_checks: cfg.active_checks || [],
          crawl_max_pages: cfg.crawl_max_pages ?? 50,
          crawl_max_depth: cfg.crawl_max_depth ?? 3,
          audit_threads: cfg.audit_threads ?? 4,
          throttle_ms: cfg.throttle_ms ?? 50,
          is_default: !!cfg.is_default,
        });
        return c.id;
      }
      const id = scanConfigSeq++;
      scanConfigs.push({
        id,
        name,
        passive_enabled: !!cfg.passive_enabled,
        active_checks: cfg.active_checks || [],
        crawl_max_pages: cfg.crawl_max_pages ?? 50,
        crawl_max_depth: cfg.crawl_max_depth ?? 3,
        audit_threads: cfg.audit_threads ?? 4,
        throttle_ms: cfg.throttle_ms ?? 50,
        is_default: !!cfg.is_default,
        created_at: new Date().toISOString().replace("T", " ").slice(0, 19),
      });
      return id;
    },
    scan_config_delete: (a) => {
      const id = (a && a.configId) || 0;
      const i = scanConfigs.findIndex((x) => x.id === id);
      if (i < 0) throw new Error(`scan config ${id} not found`);
      scanConfigs.splice(i, 1);
      return null;
    },
    extension_list: () => extensions.map((e) => ({ ...e })),
    extension_hooks: () => ({
      context_menus: extHooks.context_menus.map((m) => ({ ...m })),
      tabs: extHooks.tabs.map((t) => ({ ...t })),
    }),
    extension_load: (a) => {
      const path = (a && a.path) || "";
      const extId = a && a.extId;
      if (extId) {
        const row = extensions.find((x) => x.id === extId);
        if (!row) throw new Error(`extension ${extId} not found`);
        row.error = null;
        row.loaded_at = new Date().toISOString().replace("T", " ").slice(0, 19);
        row.enabled = true;
        row.hook_errors = 0;
        return extId;
      }
      const name = path.split(/[\\/]/).pop().replace(/\.js$/, "") || path;
      const id = extSeq++;
      extensions.push({
        id,
        name,
        path,
        enabled: true,
        error: null,
        loaded_at: new Date().toISOString().replace("T", " ").slice(0, 19),
        created_at: new Date().toISOString().replace("T", " ").slice(0, 19),
        hook_errors: 0,
      });
      return id;
    },
    extension_set_enabled: (a) => {
      const row = extensions.find((x) => x.id === ((a && a.extId) || 0));
      if (!row) throw new Error(`extension ${(a && a.extId) || 0} not found`);
      row.enabled = !!(a && a.enabled);
      return null;
    },
    extension_delete: (a) => {
      const id = (a && a.extId) || 0;
      const i = extensions.findIndex((x) => x.id === id);
      if (i < 0) throw new Error(`extension ${id} not found`);
      extensions.splice(i, 1);
      return null;
    },
    extension_read_file: (a) => `// mock source for ${(a && a.path) || "?"}`,
    extension_context_invoke: () => null,
    extension_tab_render: (a) =>
      a && a.extId === 1 && a.tabId === 0
        ? "<h1>Hello tab</h1><p>Mock extension tab.</p>"
        : "<p>No tab</p>",
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
