//! Embedded JS extension engine (rquickjs / QuickJS, ES2020).
//! Sandboxed: extensions talk to the app only through the `wpt` global API.
//! The engine lives on a dedicated worker thread; `ExtRuntime` is a `Send + Sync` handle.

use parking_lot::Mutex;
use rquickjs::{Array, Context, Ctx, Function, Object, Runtime};
use std::sync::mpsc;
use std::sync::Arc;

pub type LogSink = Arc<dyn Fn(&str, &str) + Send + Sync>;

// ---------------------------------------------------------------- public types

/// Hook metadata surfaced to the UI.
#[derive(Clone, Debug, serde::Serialize)]
pub struct ExtHookInfo {
    pub context_menus: Vec<ExtMenuItem>,
    pub tabs: Vec<ExtTabInfo>,
}

#[derive(Clone, Debug, serde::Serialize)]
pub struct ExtMenuItem {
    pub ext_id: i64,
    pub menu_id: u32,
    pub label: String,
    pub scope: String,
}

#[derive(Clone, Debug, serde::Serialize)]
pub struct ExtTabInfo {
    pub ext_id: i64,
    pub tab_id: u32,
    pub title: String,
}

// ---------------------------------------------------------------- commands

enum Cmd {
    SetLogSink(LogSink),
    Load {
        ext_id: i64,
        source: String,
        resp: mpsc::Sender<Result<ExtHookInfo, String>>,
    },
    Unload {
        ext_id: i64,
    },
    HookInfo {
        resp: mpsc::Sender<ExtHookInfo>,
    },
    RunProxy {
        payload: String,
    },
    RunHttp {
        message: String,
        resp: mpsc::Sender<String>,
    },
    RunMenu {
        ext_id: i64,
        menu_id: u32,
        payload: String,
        resp: mpsc::Sender<Result<(), String>>,
    },
    RunTab {
        ext_id: i64,
        tab_id: u32,
        resp: mpsc::Sender<Result<String, String>>,
    },
    ErrorCounts {
        resp: mpsc::Sender<Vec<(i64, u32)>>,
    },
}

// ---------------------------------------------------------------- worker state

struct ExtHooks {
    proxy_listeners: Vec<u32>,
    http_handlers: Vec<u32>,
    /// (label, scope, index)
    context_menus: Vec<(String, String, u32)>,
    /// (title, index)
    tabs: Vec<(String, u32)>,
    consecutive_errors: u32,
    /// Set after `MAX_CONSECUTIVE_ERRORS` consecutive failures; hooks stop
    /// running until the extension is reloaded/re-enabled. Logged once.
    latched: bool,
}

const MAX_CONSECUTIVE_ERRORS: u32 = 5;

/// Record a hook failure. On the 5th consecutive error the extension latches
/// (disabled) and a single "disabled" log line is emitted.
fn note_hook_error(state: &mut WorkerState, ext_id: i64, msg: &str) -> bool {
    let mut latched_now = false;
    if let Some(h) = state.hooks.iter_mut().find(|(id, _)| *id == ext_id) {
        h.1.consecutive_errors += 1;
        if h.1.consecutive_errors >= MAX_CONSECUTIVE_ERRORS && !h.1.latched {
            h.1.latched = true;
            latched_now = true;
        }
    }
    if let Some(sink) = state.log_sink.as_ref() {
        sink("error", &format!("extension {ext_id}: {msg}"));
        if latched_now {
            sink(
                "error",
                &format!(
                    "extension {ext_id}: disabled after {MAX_CONSECUTIVE_ERRORS} consecutive errors (reload to re-enable)"
                ),
            );
        }
    }
    latched_now
}

fn is_latched(state: &WorkerState, ext_id: i64) -> bool {
    state
        .hooks
        .iter()
        .find(|(id, _)| *id == ext_id)
        .map(|(_, h)| h.latched)
        .unwrap_or(false)
}

struct WorkerState {
    context: Context,
    #[allow(dead_code)]
    runtime: Runtime,
    hooks: Vec<(i64, ExtHooks)>,
    log_sink: Option<LogSink>,
}

// ---------------------------------------------------------------- bootstrap JS

const BOOTSTRAP: &str = r#"
(() => {
  const hooks = { proxy: [], http: [], menu: [], tabs: [] };
  globalThis.wpt = {
    log(msg, level) {
      __wptLog(String(msg), level === "warn" || level === "error" ? level : "info");
    },
    registerProxyListener(fn) {
      if (typeof fn === "function") hooks.proxy.push(fn);
    },
    registerHttpHandler(fn) {
      if (typeof fn === "function") hooks.http.push(fn);
    },
    registerContextMenu(opts, fn) {
      const label = typeof opts === "string" ? opts : String((opts && opts.label) || "Extension action");
      const scope = (opts && opts.scope) === "request" ? "request" : "message";
      if (typeof fn === "function") {
        __wptRegMenu(label, scope);
        hooks.menu.push(fn);
      }
    },
    registerTab(title, fn) {
      if (typeof fn === "function") {
        __wptRegTab(String(title));
        hooks.tabs.push(fn);
      }
    },
  };
  globalThis.__wptHooks = hooks;
  globalThis.__wptInvokeProxy = (idx, json) => {
    const fn = hooks.proxy[idx];
    if (typeof fn !== "function") return;
    fn(JSON.parse(json));
  };
  globalThis.__wptInvokeHttp = (idx, json) => {
    const fn = hooks.http[idx];
    if (typeof fn !== "function") return json;
    const out = fn(JSON.parse(json));
    return JSON.stringify(out === undefined ? null : out);
  };
  globalThis.__wptInvokeMenu = (idx, json) => {
    const fn = hooks.menu[idx];
    if (typeof fn !== "function") return;
    fn(JSON.parse(json));
  };
  globalThis.__wptInvokeTab = (idx) => {
    const fn = hooks.tabs[idx];
    if (typeof fn !== "function") return "";
    const out = fn();
    return typeof out === "string" ? out : "";
  };
})();
"#;

// ---------------------------------------------------------------- helpers

fn stub_log(_msg: String, _level: String) {}
fn stub_reg_menu(_label: String, _scope: String) {}
fn stub_reg_tab(_title: String) {}

fn arr_len(obj: &Object, key: &str) -> Result<u32, String> {
    let arr: Array = obj.get(key).map_err(|e| format!("{key}: {e}"))?;
    Ok(arr.len() as u32)
}

fn get_hook_indices(ctx: &Ctx) -> Result<(u32, u32, u32, u32), String> {
    let hooks_obj: Object = ctx
        .globals()
        .get("__wptHooks")
        .map_err(|e| format!("__wptHooks missing: {e}"))?;
    Ok((
        arr_len(&hooks_obj, "proxy")?,
        arr_len(&hooks_obj, "http")?,
        arr_len(&hooks_obj, "menu")?,
        arr_len(&hooks_obj, "tabs")?,
    ))
}

// ---------------------------------------------------------------- public handle

pub struct ExtRuntime {
    tx: Mutex<mpsc::Sender<Cmd>>,
}

impl Default for ExtRuntime {
    fn default() -> Self {
        ExtRuntime::new()
    }
}

impl ExtRuntime {
    pub fn new() -> Self {
        let (tx, rx) = mpsc::channel::<Cmd>();
        std::thread::spawn(move || {
            let runtime = match Runtime::new() {
                Ok(rt) => rt,
                Err(e) => {
                    eprintln!("quickjs runtime init failed: {e}");
                    return;
                }
            };
            let context = match Context::full(&runtime) {
                Ok(ctx) => ctx,
                Err(e) => {
                    eprintln!("quickjs context init failed: {e}");
                    return;
                }
            };
            let _ = context.with(|ctx| {
                let f = Function::new(ctx.clone(), stub_log as fn(String, String))
                    ?;
                ctx.globals().set("__wptLog", f)?;
                let f = Function::new(ctx.clone(), stub_reg_menu as fn(String, String))
                    ?;
                ctx.globals().set("__wptRegMenu", f)?;
                let f = Function::new(ctx.clone(), stub_reg_tab as fn(String))
                    ?;
                ctx.globals().set("__wptRegTab", f)?;
                ctx.eval::<(), _>(BOOTSTRAP)?;
                Ok::<(), rquickjs::Error>(())
            });
            let mut state = WorkerState {
                context,
                runtime,
                hooks: Vec::new(),
                log_sink: None,
            };
            while let Ok(cmd) = rx.recv() {
                match cmd {
                    Cmd::SetLogSink(sink) => set_log_sink_inner(&mut state, sink),
                    Cmd::Load { ext_id, source, resp } => {
                        let _ = resp.send(load_extension_inner(&mut state, ext_id, &source));
                    }
                    Cmd::Unload { ext_id } => {
                        state.hooks.retain(|(id, _)| *id != ext_id);
                    }
                    Cmd::HookInfo { resp } => {
                        let _ = resp.send(hook_info_inner(&state));
                    }
                    Cmd::RunProxy { payload } => {
                        run_proxy_listeners_inner(&mut state, &payload);
                    }
                    Cmd::RunHttp { message, resp } => {
                        let _ = resp.send(run_http_handlers_inner(&mut state, &message));
                    }
                    Cmd::RunMenu { ext_id, menu_id, payload, resp } => {
                        let _ = resp.send(run_menu_inner(&mut state, ext_id, menu_id, &payload));
                    }
                    Cmd::RunTab { ext_id, tab_id, resp } => {
                        let _ = resp.send(run_tab_inner(&mut state, ext_id, tab_id));
                    }
                    Cmd::ErrorCounts { resp } => {
                        let _ = resp.send(
                            state
                                .hooks
                                .iter()
                                .map(|(id, h)| (*id, h.consecutive_errors))
                                .collect(),
                        );
                    }
                }
            }
        });
        ExtRuntime { tx: Mutex::new(tx) }
    }

    fn send_reply<T>(&self, make: impl FnOnce(mpsc::Sender<T>) -> Cmd) -> Result<T, String> {
        let (tx_resp, rx_resp) = mpsc::channel();
        self.tx
            .lock()
            .send(make(tx_resp))
            .map_err(|_| "extension engine worker stopped".to_string())?;
        rx_resp
            .recv()
            .map_err(|_| "extension engine worker stopped".to_string())
    }

    fn send_noreply(&self, cmd: Cmd) -> Result<(), String> {
        self.tx
            .lock()
            .send(cmd)
            .map_err(|_| "extension engine worker stopped".to_string())
    }

    /// Install the real log sink; rebinds `__wptLog`.
    pub fn set_log_sink(&self, sink: LogSink) {
        let _ = self.send_noreply(Cmd::SetLogSink(sink));
    }

    /// Evaluate an extension's source. Returns registered hook metadata.
    pub fn load_extension(&self, ext_id: i64, source: &str) -> Result<ExtHookInfo, String> {
        self.send_reply(|resp| Cmd::Load {
            ext_id,
            source: source.to_string(),
            resp,
        })?
    }

    /// Drop all hooks for an extension (disable / delete / reload).
    pub fn unload_extension(&self, ext_id: i64) {
        let _ = self.send_noreply(Cmd::Unload { ext_id });
    }

    /// All registered menus/tabs across loaded extensions.
    pub fn hook_info(&self) -> ExtHookInfo {
        self.send_reply(|resp| Cmd::HookInfo { resp }).unwrap_or_else(|_| ExtHookInfo {
            context_menus: Vec::new(),
            tabs: Vec::new(),
        })
    }

    /// Invoke proxy-listener hooks with a JSON payload (observe-only).
    pub fn run_proxy_listeners(&self, payload: &serde_json::Value) {
        let _ = self.send_noreply(Cmd::RunProxy {
            payload: payload.to_string(),
        });
    }

    /// Run HTTP handlers over a message JSON; returns the (possibly modified) message.
    pub fn run_http_handlers(&self, message: serde_json::Value) -> serde_json::Value {
        let out = self
            .send_reply(|resp| Cmd::RunHttp {
                message: message.to_string(),
                resp,
            })
            .unwrap_or_else(|_| message.to_string());
        serde_json::from_str(&out).unwrap_or(message)
    }

    /// Invoke a context-menu callback with a JSON payload.
    pub fn run_context_menu(
        &self,
        ext_id: i64,
        menu_id: u32,
        payload: &serde_json::Value,
    ) -> Result<(), String> {
        self.send_reply(|resp| Cmd::RunMenu {
            ext_id,
            menu_id,
            payload: payload.to_string(),
            resp,
        })?
    }

    /// Render a registered tab; returns an HTML string.
    pub fn run_tab(&self, ext_id: i64, tab_id: u32) -> Result<String, String> {
        self.send_reply(|resp| Cmd::RunTab { ext_id, tab_id, resp })?
    }

    /// Snapshot of extension error counters (for the UI console).
    pub fn error_counts(&self) -> Vec<(i64, u32)> {
        self.send_reply(|resp| Cmd::ErrorCounts { resp }).unwrap_or_default()
    }
}

// ---------------------------------------------------------------- worker impl

fn set_log_sink_inner(state: &mut WorkerState, sink: LogSink) {
    let sink2 = sink.clone();
    let _ = state.context.with(|ctx| {
        let f = Function::new(
            ctx.clone(),
            move |msg: String, level: String| {
                sink2(&level, &msg);
            },
        )
        ?;
        ctx.globals().set("__wptLog", f)?;
        Ok::<(), rquickjs::Error>(())
    });
    state.log_sink = Some(sink);
}

fn load_extension_inner(
    state: &mut WorkerState,
    ext_id: i64,
    source: &str,
) -> Result<ExtHookInfo, String> {
    let pending_menus: Arc<Mutex<Vec<(String, String)>>> = Arc::new(Mutex::new(Vec::new()));
    let pending_tabs: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    state
        .context
        .with(|ctx| {
            let pm = pending_menus.clone();
            let f = Function::new(
                ctx.clone(),
                move |label: String, scope: String| {
                    pm.lock().push((label, scope));
                },
            )
            ?;
            ctx.globals().set("__wptRegMenu", f)?;
            let pt = pending_tabs.clone();
            let f = Function::new(ctx.clone(), move |title: String| {
                pt.lock().push(title);
            })
            ?;
            ctx.globals().set("__wptRegTab", f)?;
            Ok::<(), rquickjs::Error>(())
        })
        .map_err(|e| format!("register hooks failed: {e}"))?;

    let (proxy_start, http_start, menu_start, tabs_start) =
        state.context.with(|ctx| get_hook_indices(&ctx))?;

    state.hooks.retain(|(id, _)| *id != ext_id);

    state
        .context
        .with(|ctx| {
            ctx.eval::<(), _>(source).map_err(|e| {
                let detail = ctx
                    .catch()
                    .as_exception()
                    .map(|exc| exc.to_string())
                    .unwrap_or_default();
                if detail.is_empty() {
                    format!("eval error: {e}")
                } else {
                    format!("eval error: {e} - {detail}")
                }
            })
        })?;

    let (proxy_end, http_end, menu_end, tabs_end) =
        state.context.with(|ctx| get_hook_indices(&ctx))?;

    let proxy_listeners: Vec<u32> = (proxy_start..proxy_end).collect();
    let http_handlers: Vec<u32> = (http_start..http_end).collect();

    let menu_meta = pending_menus.lock().clone();
    let tab_meta = pending_tabs.lock().clone();

    let mut context_menus = Vec::new();
    let mut menus_info = Vec::new();
    for (i, (label, scope)) in menu_meta.into_iter().enumerate() {
        let idx = menu_start + i as u32;
        menus_info.push(ExtMenuItem {
            ext_id,
            menu_id: idx,
            label: label.clone(),
            scope: scope.clone(),
        });
        context_menus.push((label, scope, idx));
    }

    let mut tabs = Vec::new();
    let mut tabs_info = Vec::new();
    for (i, title) in tab_meta.into_iter().enumerate() {
        let idx = tabs_start + i as u32;
        tabs_info.push(ExtTabInfo {
            ext_id,
            tab_id: idx,
            title: title.clone(),
        });
        tabs.push((title, idx));
    }

    // end indices are consumed by the ranges above; silence unused warnings
    let _ = (proxy_end, http_end, menu_end, tabs_end);

    state.hooks.push((
        ext_id,
        ExtHooks {
            proxy_listeners,
            http_handlers,
            context_menus,
            tabs,
            consecutive_errors: 0,
            latched: false,
        },
    ));

    Ok(ExtHookInfo {
        context_menus: menus_info,
        tabs: tabs_info,
    })
}

fn hook_info_inner(state: &WorkerState) -> ExtHookInfo {
    let mut menus = Vec::new();
    let mut tabs = Vec::new();
    for (ext_id, h) in &state.hooks {
        for (label, scope, mid) in &h.context_menus {
            menus.push(ExtMenuItem {
                ext_id: *ext_id,
                menu_id: *mid,
                label: label.clone(),
                scope: scope.clone(),
            });
        }
        for (title, tid) in &h.tabs {
            tabs.push(ExtTabInfo {
                ext_id: *ext_id,
                tab_id: *tid,
                title: title.clone(),
            });
        }
    }
    ExtHookInfo {
        context_menus: menus,
        tabs,
    }
}

fn run_proxy_listeners_inner(state: &mut WorkerState, payload_json: &str) {
    let ext_ids: Vec<i64> = state
        .hooks
        .iter()
        .filter(|(_, h)| !h.proxy_listeners.is_empty() && !h.latched)
        .map(|(id, _)| *id)
        .collect();
    for ext_id in ext_ids {
        let indices: Vec<u32> = state
            .hooks
            .iter()
            .find(|(id, _)| *id == ext_id)
            .map(|(_, h)| h.proxy_listeners.clone())
            .unwrap_or_default();
        let mut errors = 0u32;
        for idx in indices {
            let result = state.context.with(|ctx| {
                let f: Function = ctx
                    .globals()
                    .get("__wptInvokeProxy")
                    .map_err(|e| format!("__wptInvokeProxy missing: {e}"))?;
                let _: () = f
                    .call((idx as i32, payload_json.to_string()))
                    .map_err(|e| format!("proxy listener: {e}"))?;
                Ok::<(), String>(())
            });
            if let Err(e) = result {
                errors += 1;
                if note_hook_error(state, ext_id, &e) {
                    break;
                }
            }
        }
        if errors == 0 {
            if let Some(h) = state.hooks.iter_mut().find(|(id, _)| *id == ext_id) {
                h.1.consecutive_errors = 0;
            }
        }
    }
}

fn run_http_handlers_inner(state: &mut WorkerState, message_json: &str) -> String {
    let ext_ids: Vec<i64> = state
        .hooks
        .iter()
        .filter(|(_, h)| !h.http_handlers.is_empty() && !h.latched)
        .map(|(id, _)| *id)
        .collect();
    let mut current = message_json.to_string();
    for ext_id in ext_ids {
        let indices: Vec<u32> = state
            .hooks
            .iter()
            .find(|(id, _)| *id == ext_id)
            .map(|(_, h)| h.http_handlers.clone())
            .unwrap_or_default();
        let mut any_error = false;
        for idx in indices {
            let result = state.context.with(|ctx| {
                let f: Function = ctx
                    .globals()
                    .get("__wptInvokeHttp")
                    .map_err(|e| format!("__wptInvokeHttp missing: {e}"))?;
                let out: String = f
                    .call((idx as i32, current.clone()))
                    .map_err(|e| format!("http handler: {e}"))?;
                Ok::<String, String>(out)
            });
            match result {
                Ok(v) => current = v,
                Err(e) => {
                    note_hook_error(state, ext_id, &e);
                    any_error = true;
                    break;
                }
            }
        }
        if !any_error {
            if let Some(h) = state.hooks.iter_mut().find(|(id, _)| *id == ext_id) {
                h.1.consecutive_errors = 0;
            }
        }
    }
    current
}

fn run_menu_inner(
    state: &mut WorkerState,
    ext_id: i64,
    menu_id: u32,
    payload_json: &str,
) -> Result<(), String> {
    if is_latched(state, ext_id) {
        return Err(format!(
            "extension {ext_id} is disabled after {MAX_CONSECUTIVE_ERRORS} consecutive errors (reload to re-enable)"
        ));
    }
    let idx = state
        .hooks
        .iter()
        .find(|(id, _)| *id == ext_id)
        .and_then(|(_, h)| {
            h.context_menus
                .iter()
                .find(|(_, _, mid)| *mid == menu_id)
                .map(|(_, _, idx)| *idx)
        })
        .ok_or_else(|| format!("extension {ext_id} menu {menu_id} not registered"))?;
    let result = state.context.with(|ctx| {
        let f: Function = ctx
            .globals()
            .get("__wptInvokeMenu")
            .map_err(|e| format!("__wptInvokeMenu missing: {e}"))?;
        let _: () = f
            .call((idx as i32, payload_json.to_string()))
            .map_err(|e| format!("context menu hook: {e}"))?;
        Ok::<(), String>(())
    });
    match result {
        Ok(()) => {
            if let Some(h) = state.hooks.iter_mut().find(|(id, _)| *id == ext_id) {
                h.1.consecutive_errors = 0;
            }
            Ok(())
        }
        Err(e) => {
            note_hook_error(state, ext_id, &e);
            Err(e)
        }
    }
}

fn run_tab_inner(state: &mut WorkerState, ext_id: i64, tab_id: u32) -> Result<String, String> {
    if is_latched(state, ext_id) {
        return Err(format!(
            "extension {ext_id} is disabled after {MAX_CONSECUTIVE_ERRORS} consecutive errors (reload to re-enable)"
        ));
    }
    let idx = state
        .hooks
        .iter()
        .find(|(id, _)| *id == ext_id)
        .and_then(|(_, h)| {
            h.tabs
                .iter()
                .find(|(_, tid)| *tid == tab_id)
                .map(|(_, idx)| *idx)
        })
        .ok_or_else(|| format!("extension {ext_id} tab {tab_id} not registered"))?;
    let result = state.context.with(|ctx| {
        let f: Function = ctx
            .globals()
            .get("__wptInvokeTab")
            .map_err(|e| format!("__wptInvokeTab missing: {e}"))?;
        let html: String = f
            .call((idx as i32,))
            .map_err(|e| format!("tab hook: {e}"))?;
        Ok::<String, String>(html)
    });
    match result {
        Ok(html) => {
            if let Some(h) = state.hooks.iter_mut().find(|(id, _)| *id == ext_id) {
                h.1.consecutive_errors = 0;
            }
            Ok(html)
        }
        Err(e) => {
            note_hook_error(state, ext_id, &e);
            Err(e)
        }
    }
}

// ---------------------------------------------------------------- tests

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bootstrap_defines_wpt_api() {
        let rt = ExtRuntime::new();
        let info = rt
            .load_extension(1, "wpt.log('hello from test');")
            .expect("load");
        assert!(info.context_menus.is_empty());
        assert!(info.tabs.is_empty());
    }

    #[test]
    fn registers_menus_and_tabs() {
        let rt = ExtRuntime::new();
        let src = r#"
            wpt.registerContextMenu({ label: 'Do thing', scope: 'message' }, function (m) { wpt.log('invoked'); });
            wpt.registerTab('Report', function () { return '<h1>ok</h1>'; });
        "#;
        let info = rt.load_extension(7, src).expect("load");
        assert_eq!(info.context_menus.len(), 1);
        assert_eq!(info.context_menus[0].label, "Do thing");
        assert_eq!(info.context_menus[0].scope, "message");
        assert_eq!(info.tabs.len(), 1);
        assert_eq!(info.tabs[0].title, "Report");
        let all = rt.hook_info();
        assert_eq!(all.context_menus.len(), 1);
        assert_eq!(all.tabs.len(), 1);
    }

    #[test]
    fn http_handler_mutates_message() {
        let rt = ExtRuntime::new();
        rt.load_extension(
            1,
            "wpt.registerHttpHandler(function (m) { m.request.method = 'PUT'; return m; });",
        )
        .expect("load");
        let msg = serde_json::json!({
            "request": { "method": "GET", "url": "https://x.test/", "headers": [], "body": "" },
            "response": null
        });
        let out = rt.run_http_handlers(msg);
        assert_eq!(out["request"]["method"], "PUT");
    }

    #[test]
    fn unload_removes_hooks() {
        let rt = ExtRuntime::new();
        rt.load_extension(1, "wpt.registerTab('A', function () { return 'x'; });")
            .expect("load");
        assert_eq!(rt.hook_info().tabs.len(), 1);
        rt.unload_extension(1);
        assert!(rt.hook_info().tabs.is_empty());
    }

    #[test]
    fn broken_extension_reports_error() {
        let rt = ExtRuntime::new();
        let err = rt.load_extension(1, "throw new Error('boom');").unwrap_err();
        assert!(err.contains("boom"), "{err}");
    }

    #[test]
    fn latch_disables_after_five_consecutive_errors() {
        let rt = ExtRuntime::new();
        rt.load_extension(
            1,
            "wpt.registerProxyListener(function () { throw new Error('x'); });",
        )
        .expect("load");
        for _ in 0..5 {
            rt.run_proxy_listeners(&serde_json::json!({}));
        }
        assert_eq!(rt.error_counts(), vec![(1, 5)]);
        // latched: further invocations are skipped, count does not grow
        rt.run_proxy_listeners(&serde_json::json!({}));
        assert_eq!(rt.error_counts(), vec![(1, 5)]);
    }

    #[test]
    fn context_menu_invokes() {
        let rt = ExtRuntime::new();
        rt.load_extension(
            1,
            "wpt.registerContextMenu({ label: 'L' }, function (m) { wpt.log('got ' + m.request.url); });",
        )
        .expect("load");
        let payload = serde_json::json!({ "request": { "url": "https://t/" } });
        rt.run_context_menu(1, 0, &payload).expect("invoke");
    }

    #[test]
    fn tab_renders_html() {
        let rt = ExtRuntime::new();
        rt.load_extension(1, "wpt.registerTab('R', function () { return '<b>hi</b>'; });")
            .expect("load");
        let html = rt.run_tab(1, 0).expect("render");
        assert_eq!(html, "<b>hi</b>");
    }

    #[test]
    fn proxy_listener_observes() {
        let rt = ExtRuntime::new();
        rt.load_extension(
            1,
            "wpt.registerProxyListener(function (m) { wpt.log('saw ' + m.request.url); });",
        )
        .expect("load");
        let payload = serde_json::json!({ "request": { "url": "https://obs/" } });
        rt.run_proxy_listeners(&payload);
    }
}
