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
    repeater_send: () => ({
      status: 200,
      headers: [
        ["content-type", "text/html; charset=utf-8"],
        ["server", "mock/1.0"],
      ],
      body: "<!doctype html><html><body><h1>Mock response</h1></body></html>",
      time_ms: 91,
    }),
    init_ca: () => "C:\\Users\\qa\\AppData\\Local\\WebProxyToolkit\\ca.crt",
    log_list: () => [
      { id: 9, ts: "2026-10-11 09:22:31", tool: "repeater", level: "info", message: "GET https://shop.example.com/api/profile -> 500 in 74ms" },
      { id: 8, ts: "2026-10-11 09:21:17", tool: "proxy", level: "info", message: "Recorded GET fonts.gstatic.com/s/inter/v18/font.woff2 (200)" },
      { id: 7, ts: "2026-10-11 09:19:03", tool: "proxy", level: "info", message: "Recorded POST shop.example.com/api/cart (201)" },
      { id: 6, ts: "2026-10-11 09:16:42", tool: "scope", level: "warn", message: "Removed analytics.track.io from scope" },
      { id: 5, ts: "2026-10-11 09:15:35", tool: "proxy", level: "info", message: "Intercept off (requests+responses disabled)" },
      { id: 4, ts: "2026-10-11 09:14:28", tool: "repeater", level: "info", message: "Tab 1 saved" },
      { id: 3, ts: "2026-10-11 09:13:21", tool: "proxy", level: "warn", message: "Held request dropped by user" },
      { id: 2, ts: "2026-10-11 09:12:14", tool: "system", level: "error", message: "sequencer_sample: connection refused (skipped 1 of 20)" },
      { id: 1, ts: "2026-10-11 09:11:07", tool: "proxy", level: "info", message: "Listener started on 127.0.0.1:18080" },
    ],
    log_clear: () => null,
    log_add: () => null,
    sequencer_sample: () => [
      "aZ3kQ9pLmX7vB2nR", "wE5tY8uI1oP4aS6d", "fG0hJ2kL5zX8cV9b", "nM3qW6eR9tY0uI4o",
      "pA7sD0fG3hJ6kL1z", "xC9vB2nM5qW8eR4t", "yU6iO9pA2sD5fG8h", "jK1lZ4xC7vB0nM3q",
      "W5eR8tY1uI4oP7a", "S2dF5gH8jK0lZ3x", "C6vB9nM2qW5eR8t", "Y1uI4oP7aS0dF3g",
      "H5jK8lZ1xC4vB7n", "M0qW3eR6tY9uI2o", "P5aS8dF1gH4jK7l", "Z0xC3vB6nM9qW2e",
      "R5tY8uI1oP4aS7d", "F0gH3jK6lZ9xC2v", "B5nM8qW1eR4tY7u", "I0oP3aS6dF9gH2j",
    ],
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
