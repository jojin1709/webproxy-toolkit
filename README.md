<div align="center">

<img src="app/src-tauri/icons/128x128@2x.png" alt="WebProxy Toolkit" width="96" height="96">

# WebProxy Toolkit

### A Burp Suite–style web security workbench that runs entirely on your machine.

Clean-room implementation — original UI and code, built from observed behavior only.

**[Website](https://jojin1709.github.io/webproxy-toolkit/)** · **[Download](https://github.com/jojin1709/webproxy-toolkit/releases/latest)**

<sub>Local MITM proxy · Repeater · Intruder · Scanner · Collaborator · Extensions — all offline.</sub>

---

<img src="docs/screenshot-dashboard.png" alt="WebProxy Toolkit dashboard" width="100%">

</div>

> [!NOTE]
> **Authorized testing only.** WebProxy Toolkit actively intercepts and rewrites traffic. Run it only against systems you own or have explicit permission to test.

---

## Table of Contents

- [Table of Contents](#table-of-contents)
- [What is WebProxy Toolkit?](#what-is-webproxy-toolkit)
  - [Why It Exists](#why-it-exists)
  - [Clean-room by design](#clean-room-by-design)
- [Features](#features)
- [Download](#download)
- [Architecture](#architecture)
- [Status](#status)
- [Scope \& Limitations](#scope--limitations)
- [Repository](#repository)
- [License](#license)
- [Acknowledgements](#acknowledgements)
- [Developed by](#developed-by)

## What is WebProxy Toolkit?

WebProxy Toolkit is a full web security workbench — a local interception proxy with the tool suite you'd expect from a professional testing platform — that runs entirely on your own machine. Point your browser at it, intercept and rewrite traffic, replay and fuzz requests, crawl and scan targets, spin up a local out-of-band interaction server, and extend it with JavaScript — all offline.

<a id="why-it-exists"></a>
<details>
<summary><strong>Why It Exists</strong></summary>

Professional proxy suites are powerful but heavy: license servers, accounts, telemetry, and a footprint that doesn't fit every environment. WebProxy Toolkit is a self-contained desktop app that gives you the core interception, replay, fuzzing, and scanning workflow with zero external dependencies — your data stays in a single local project file.

</details>

<a id="clean-room-by-design"></a>
<details>
<summary><strong>Clean-room by design</strong></summary>

No PortSwigger code, assets, or copied text. The UI and implementation are original, built from observed behavior of the workflow it emulates. Icons are Lucide; the font is Inter.

</details>

## Features

- **Proxy** — MITM HTTP/HTTPS proxy with a locally generated CA, HTTP history with detail panes (Pretty / Raw / Hex, find bar), request & response interception with an inline held-message editor, scope and match/replace rules, options.
- **Target** — site map with URL/HTTP-method views, crawl paths, in-scope filtering, issue list.
- **Repeater** — multi-tab request editor with parsed request inspector (line, headers, parameters, body), send/cancel, save/close.
- **Intruder** — attack engine with §…§ positions (auto-detect / wrap selection), 4 attack types (sniper, battering ram, pitchfork, cluster bomb), up to 8 payload sets (simple list / numbers) with 14 payload processors (url/base64 encode+decode, md5/sha1/sha256, replace, prefix/suffix, upper/lower, trim, reverse), threads/delay/max-request resource pool, live results with payload combos, status codes, grep extractors and full request/response inspection.
- **Comparer** — side-by-side line and word diffs of any two messages, differences table with prev/next navigation.
- **Sequencer** — token randomness analysis: sampling from a live URL (cookie / header / regex) or pasted samples, entropy, chi-square uniformity, Monte Carlo π, byte-level per-position stats.
- **Logger** — persistent event log (proxy, repeater, scope, scanner, system) with tool/level/text filters, also embedded in the Dashboard (Audit log tab filters to scanner).
- **Decoder** — encode/decode transforms (URL, Base64, hex, …).
- **Scanner** — passive checks on every in-scope response (17 header/cookie/banner/info-leak checks), active audit engine that crawls a target and tests parameters (SQL injection, reflected XSS, path traversal, open redirect, sensitive files), issue framework with dedupe/severity/confidence, issue detail with linked exchange, named scan configs (limits, threads, throttle, enabled checks) with an issue-definitions catalog, and Dashboard scan tasks with live progress + pause/resume/cancel.
- **Extensions** — embedded QuickJS engine with our own `wpt` JavaScript API: proxy listeners, HTTP handlers (request/response rewriting in the relay), context-menu actions in Proxy and Repeater, and custom report tabs rendered in sandboxed iframes; per-extension error latch (disable after 5 consecutive failures) with an error console in the Extensions panel.
- **Collaborator** — local out-of-band interaction server: configurable DNS (UDP) and HTTP listeners, payload generation (URL / HTML img / hostname with unique subdomains), live interaction log (DNS queries with qtype, HTTP requests with method/path/headers/body) pushed to the UI in real time, payload filtering and clipboard copy.
- **Upstream & TLS options** — upstream proxy chaining (host-pattern rules, plain or TLS to the proxy), TLS pass-through hosts (raw tunnel, no MITM — for cert-pinned targets), and extra proxy listeners sharing all options.
- **Session handling** — rule engine with macros: replay a macro (ordered raw requests), extract cookie/header/body values, and auto-set headers/cookies/params on matching traffic before it leaves the proxy.
- **Shortcuts** — global keyboard shortcuts: Ctrl+Shift+F search, Ctrl+1…0 tool-tab switching, Esc to close dialogs.
- **Dashboard** — project summary, scan tasks (crawl/audit) with live progress, issues with severity counts, event log, scanner audit log.
- **A11y & theme** — full keyboard navigation, ARIA roles, WCAG AA contrast, dark Burp-like theme, responsive down to ~1000 px.

## Download

Grab the latest Windows installer (x64, NSIS) from the releases page:

**[Download for Windows →](https://github.com/jojin1709/webproxy-toolkit/releases/latest)**

You can also find it on the **[website](https://jojin1709.github.io/webproxy-toolkit/)**, where the download button always points to the newest release.

## Architecture

```
app/
  src/            React + TypeScript UI (Vite)
    components/   shell pieces: tabs, menus, message viewer, error boundary
    panels/       one file per tool tab
    lib/          pure logic: diff, entropy/analysis
  src-tauri/      Rust backend (Tauri 2)
    ca.rs         local CA generation (rcgen/rustls)
    proxy.rs      MITM proxy: listeners, TLS, intercept, relay
    db.rs         SQLite persistence (project file, history, logs)
    commands.rs   Tauri command surface (the only UI ↔ backend API)
tests/e2e.rs      end-to-end proxy tests (own CA per test run)
qa/               Playwright harness: Tauri IPC mock + screenshots
replica/          parity inventory (features.csv), design tokens, recon notes
```

- Project data lives in `%LOCALAPPDATA%\WebProxyToolkit\` (`project.wpt`, `ca.crt`, `ca.key`, body blobs).
- The UI talks to Rust only through `#[tauri::command]` handlers registered in `lib.rs`.

## Status

| Phase | Feature | State |
|-------|---------|-------|
| 1 | App shell, Proxy (history / intercept / options), Target, Repeater, Decoder, Settings | done |
| 2a | Comparer, Sequencer, Logger (+ Dashboard event log) | done |
| 2b | WebSocket history, global search, Repeater per-tab history & follow-redirects, intercept Action menu | done |
| 3 | Intruder (attack engine, positions, payload sets, results) | done |
| 4 | Passive scanner, Issues view, active audit, crawling, scan configs | done |
| 6 | Extensions (embedded JS engine, Burp-like extension API) | done |
| 7 | Collaborator (local out-of-band interaction server) | done |
| 8 | Session handling & macros, upstream proxy, TLS options, multi-listener, shortcuts | done |

## Scope & Limitations

- Local-only: no license validation, no cloud, no Burp compatibility claims.
- Clean-room: no PortSwigger code, assets, or copied text; icons are Lucide, font is Inter.
- For authorized testing of systems you own or have permission to test.

## Repository

https://github.com/jojin1709/webproxy-toolkit

## License

Licensed under the **Business Source License 1.1 (BUSL-1.1)** — see [LICENSE](LICENSE).

In short: the source is viewable, but you may not reuse, rebrand, resell, or offer it as a hosted/managed service, or build a competing commercial product. On **2030-10-14** it converts to **AGPL-3.0**. See the Additional Use Grant in [LICENSE](LICENSE) for the exact terms.

## Acknowledgements

- [Tauri](https://tauri.app) — desktop app framework.
- [Lucide](https://lucide.dev) — icon set.
- [Inter](https://rsms.me/inter/) — typeface.
- [Playwright](https://playwright.dev) — browser automation for the QA harness.

## Developed by

**[JOJIN JOHN](https://www.linkedin.com/in/jojin-john/)**
