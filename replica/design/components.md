# Components — WebProxy Toolkit

Dense desktop security tool. Dark-first. Open icons only (Lucide). Copy is original — do not use PortSwigger strings.

Shared: focus-visible = 2px `accent` ring, offset 1px. Keyboard: Tab order follows visual order; Esc closes menus/modals. `prefers-reduced-motion`: disable non-essential transitions.

---

## TabBar (app shell)

- variants: tool tabs (Proxy, Repeater, Decoder, Target, Comparer)
- sizes: height `layout.tab` (32px), padding-x 12
- states: active (surface-2 + accent bottom 2px), inactive (muted text), hover
- tokens: bg surface, text text-muted; active text text
- a11y: `role="tablist"` / `role="tab"` / `aria-selected`; arrows move focus
- used on: S01

## StatusStrip

- variants: proxy up (success dot), down (danger), held count badge, listen port mono
- sizes: height `layout.statusbar` (28px), text xs
- states: up, down, error tooltip
- tokens: bg surface, text text-muted
- a11y: `role="status"` for live held-count; don't rely on colour alone (include "Listening"/"Stopped" label)
- used on: S01

## Button

- variants: primary (Send, Forward), secondary (Cancel), ghost (toolbar), danger (Drop)
- sizes: sm 24px, md 28px (dense default), lg 32px
- states: default, hover, active, focus-visible, disabled, loading (spinner + label kept)
- tokens: primary bg accent text on-accent; danger bg danger text on-danger; ghost transparent text text
- a11y: real `<button>`; loading uses `aria-busy`
- used on: S02, S05, S07, S09

## Toggle (intercept on/off)

- variants: switch with label "Intercept requests"
- sizes: track 36×20, knob 16
- states: on (accent track), off (surface-2), focus-visible, disabled
- tokens: on accent, off border on surface-2
- a11y: `role="switch"` `aria-checked`; Space toggles
- used on: S02

## IconButton

- variants: context (Send to Repeater, etc.), toolbar
- sizes: 28×28 hit target ≥24 visual
- states: default, hover, focus-visible, disabled; tooltip after 400ms
- tokens: ghost; hover bg surface-2
- a11y: `aria-label` required; tooltip not the only name
- used on: S02–S05, S03

## DataTable (history, results)

- variants: history columns time | method | host | path | status | length | TLS
- sizes: row `layout.row` (28px), header xs/600, cell padding-x 8, mono for method/status/url path
- states: empty ("No traffic yet — start the proxy and browse"), loading skeleton rows, filled, row selected (surface-2), multi-select, keyboard row focus
- tokens: header bg surface, row bg bg, selected surface-2, hover surface
- a11y: `role="grid"`; arrow keys; Enter opens message editor; screen-reader summary "23 requests"
- used on: S03, S08, (later results)

## FilterBar

- variants: text query, method multi-select chips, status filter, "in scope only"
- sizes: height 32, input sm
- states: empty query, active query with clear (×), invalid regex
- tokens: input bg surface, border-input, placeholder text-muted
- a11y: label "Filter history"; clear button named
- used on: S03, S08

## MessageEditor

- variants: request pane, response pane; modes Pretty | Raw | Hex
- sizes: min-height flexible split; font mono base; line numbers gutter muted
- states: focused, readonly (history), dirty (tab dot), binary (hex default), invalid UTF-8 badge
- tokens: bg bg, caret accent; selection surface-2
- a11y: CodeMirror a11y; announce mode change; don't trap Tab (provide Esc-exit chrome shortcut docs)
- used on: S02, S04, S05

## SplitPanes

- variants: vertical (req/resp), horizontal optional
- sizes: drag handle 6px, min pane 80px
- states: default, dragging, collapsed
- tokens: handle border; hover accent
- a11y: handle is `role="separator"` with arrow-key resize
- used on: S02, S05

## ContextMenu

- variants: history row, repeater tab
- sizes: min-width 200, item height 28
- states: open, item hover, shortcut hint mono xs
- tokens: bg surface-2, shadow-pop, border
- a11y: focus first item on open; Up/Down/Esc
- used on: S03, S05

## RepeaterTabStrip

- variants: numbered tabs, dirty indicator, close ×
- sizes: height tab 32, max tab width 160 truncate
- states: active, inactive, dirty, overflow menu
- tokens: same as TabBar but secondary level (surface)
- a11y: tablist; middle-click close optional; Close has aria-label
- used on: S05

## PatternList (scope)

- variants: include / exclude sections; rows kind+pattern+enabled
- sizes: row 28, add-row form inline
- states: empty ("All traffic in scope"), rows, invalid pattern
- tokens: include success left border 2px, exclude danger left border 2px
- a11y: list + labelled inputs; delete requires confirm or undo toast
- used on: S07

## SiteMapTree

- variants: host → path segments; badge count
- sizes: indent 16, row 28
- states: collapsed, expanded, leaf selected, empty
- tokens: selected surface-2; count text-muted xs
- a11y: `role="tree"` / `treeitem` `aria-expanded`
- used on: S08

## DecoderPanel

- variants: single op or chain list; direction encode/decode
- sizes: textarea min-h 120 mono; op chips
- states: empty, valid output, error ("Not valid Base64"), chain step failed
- tokens: input surface; error danger text
- a11y: label each op; error via `aria-live="polite"`
- used on: S06

## SettingsForm

- variants: proxy port, theme (dark default), max history, CA export path
- sizes: label column 160, field input md
- states: default, dirty, saved toast, port conflict error
- tokens: inputs as FilterBar; save primary button
- a11y: fieldset + legend per section; errors linked `aria-describedby`
- used on: S09

## Toast

- variants: success, error, info
- sizes: min-width 240, max 2; auto-dismiss 4s errors sticky
- states: enter, visible, exit
- tokens: bg surface-2, left border 3px success/danger/accent
- a11y: `role="status"` / `role="alert"` for errors
- used on: all

## CertificateCallout (first-run)

- variants: informational panel on S09 / first proxy start
- sizes: padding 16, max-width 480 in modal or full in settings
- states: needs export, exported, user dismissed
- tokens: bg surface, border warning left; primary action Export CA
- a11y: heading + body; never auto-install into OS trust without explicit button + docs
- used on: S09, F01

---

Empty-state copy (original):
- History: "No traffic yet. Start the proxy, then browse through it."
- Repeater: "Open a tab or send a request from history."
- Decoder: "Paste a value to encode or decode."
- Scope: "No rules — all traffic is treated as in scope."
