# Recon map: WebProxy Toolkit (desktop, original clone)

Scope: Community-edition core loop only — HTTPS intercepting proxy, history, Repeater, Decoder, Target scope
For: local authorized testing and learning; no license server, no PortSwigger account
Date: 2026-10-11

Rules: clean-room from public PortSwigger docs only. No decompile of burpsuite.jar or any file under the installer extract. Stack: Rust (not Java).

## Sources

| # | source | URL | notes |
| --- | --- | --- | --- |
| 1 | docs home (desktop) | https://portswigger.net/burp/documentation/desktop | tool list, workflow |
| 2 | tools overview | https://portswigger.net/burp/documentation/desktop/tools | Proxy, Repeater, Decoder, Target, Comparer |
| 3 | Community vs Pro | https://portswigger.net/burp/communitydownload | gates: scanner, full Intruder, Collaborator = Pro |
| 4 | Proxy | https://portswigger.net/burp/documentation/desktop/tools/proxy | MITM, intercept, history, CA cert |
| 5 | Getting started steps 2–5 | https://portswigger.net/burp/documentation/desktop/getting-started | intercept, modify, scope, reissue |
| 6 | Repeater | https://portswigger.net/burp/documentation/desktop/tools/repeater | tabs, edit, resend |
| 7 | Decoder | https://portswigger.net/burp/documentation/desktop/tools/decoder | encode/decode chains |
| 8 | Target / scope | https://portswigger.net/burp/documentation/desktop/tools/target | site map, scope |

## Core loop

Browse a target through a local HTTPS proxy, hold and edit requests, replay them in Repeater, and decode opaque values — all offline from any account.

## Screens

| ID | screen | route / how to reach | purpose | key components | states seen |
| --- | --- | --- | --- | --- | --- |
| S01 | Shell + top tabs | app launch | navigate tools | tab bar: Proxy, Repeater, Decoder, Target, (Comparer) | empty project, loaded project |
| S02 | Proxy Intercept | Proxy tab, Intercept subtab | hold/modify live request | on/off toggle, request editor, Forward / Drop / Cancel | no traffic, held request, held response (optional) |
| S03 | HTTP history | Proxy tab, HTTP history | review all proxied exchanges | table (time, host, method, URL, status, length), filter bar | empty, filtered empty, filled |
| S04 | Message editor | embedded in S02/S03/Repeater | raw HTTP view | request pane + response pane, hex/pretty toggle | request only, request+response, error |
| S05 | Repeater | Repeater tab | manual replay | tab strip, request editor, Send, response pane, timing/status | empty tab, sent, network error |
| S06 | Decoder | Decoder tab | transform data | input area, op list (URL/Base64/HTML/Hex), output area | empty, encoded, decoded, invalid input |
| S07 | Target scope | Target tab → Scope | limit what proxy holds | include/exclude URL patterns, add-from-history | empty scope, rules listed |
| S08 | Site map | Target tab → Site map | tree of hosts/paths seen | host tree + request table | empty, filled |
| S09 | Settings / CA | menu → settings | proxy port, install CA, theme | port input, CA export/install, theme | defaults, custom |

States: empty / filled / error / loading are all in scope for v1 except background crawl loading (no crawler).

## Flows

```
F01 First-run: trust CA and browse through proxy
    S09 (export CA, set port) -> OS trust store -> browser via 127.0.0.1:port -> S03 fills
    happy path clicks: ~6
    edge: user skips CA (HTTPS fails), port in use, loopback blocked

F02 Intercept and modify a request
    S03 or browser -> S02 held request -> edit body/headers -> Forward -> response optional -> S03
    happy path clicks: 4
    edge: long body, binary body, redirect chain, websocket upgrade

F03 Send to Repeater and replay
    S03 row context menu -> S05 tab prefilled -> edit -> Send -> compare response
    happy path clicks: 3
    edge: connection refused, TLS error, slow server, repeated sends

F04 Decode a token
    copy from S03/S05 -> S06 paste -> pick URL/Base64/HTML/Hex -> read output -> copy
    happy path clicks: 3
    edge: invalid base64, double-encoding, unicode

F05 Scope to one host
    S07 add include rule (host or URL) -> browse -> only in-scope in S03
    happy path clicks: 4
    edge: over-broad pattern, exclude wins, websocket host
```

## Components

| component | variants | states | used on |
| --- | --- | --- | --- |
| Tab bar | tool-level tabs | active, inactive | S01 |
| Data table | history, site map, (later Intruder results) | empty, rows, selected, multi-select | S03, S08 |
| Filter bar | text + method/status filters | empty query, active query | S03, S08 |
| Message editor | request, response; pretty/raw | focused, readonly, invalid | S02–S05 |
| Button | primary (Send/Forward), danger (Drop) | default, hover, disabled, loading | S02, S05 |
| Toggle | intercept on/off | on, off | S02 |
| Context menu | Send to Repeater / Decoder / Comparer | open | S03, S05 |
| Tab strip (Repeater) | numbered tabs | single, many, dirty | S05 |
| Pattern list | scope include/exclude | empty, rows | S07 |
| Status bar | proxy up/down, listener port, held count | up, down, error | S01 |
| Toast / status line | errors, CA installed | success, error | all |

## Inferred data model

```
ProxyConfig   listen_addr, https_enabled, ca_cert_path, intercept_request(bool),
              intercept_response(bool)
              evidence: S02, S09; docs Proxy + settings
              confidence: high

HttpExchange  id, started_at, client, host, method, url, request_headers, request_body,
              status, response_headers, response_body, time_ms, tls(bool), ws(bool),
              in_scope(bool)
              evidence: S03 columns; docs HTTP history
              confidence: high

ScopeRule     mode (include|exclude), kind (host|url_prefix|regex), pattern
              evidence: S07; docs Target scope
              confidence: high

RepeaterTab   id, name, request (raw or structured), last_response, dirty
              evidence: S05; docs Repeater managing tabs
              confidence: high

DecodeOp      kind (url|base64|html|hex|...), direction (encode|decode)
              evidence: S06; docs Decoder
              confidence: high

SiteMapNode   host, path, children[], exchange_ids[]
              evidence: S08; docs site map
              confidence: medium (tree shape inferred)
```

Relationships: ProxyConfig 1; HttpExchange n-1 host; ScopeRule applies to HttpExchange; RepeaterTab holds a copy of an HttpExchange request; SiteMapNode aggregates HttpExchange by host/path.

## Feature matrix

See `features.csv`. Must: 8, should: 6, could: 3, skip: 4.

## Out of scope (cannot or should not be cloned)

- PortSwigger license activation, account login, update servers, Burp AI / Burp AT
- Collaborator / OAST platform (their network)
- BApp Store and extension marketplace
- Scanner crawlers/audit engines (Pro; also huge surface)
- Any code, assets, or strings from the official installer JARs
- Bundled Chromium (we use the system browser or a minimal embedded option later)

## Size

Screens 9, flows 5, entities 6. Hard parts: trustworthy HTTPS MITM + CA UX, correct raw HTTP parse/serialize (incl. chunked/WebSocket upgrade), dense desktop UI in Rust. Size: **M**.
