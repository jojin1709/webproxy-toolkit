pub mod commands;
pub mod db;
pub mod ca;
pub mod intruder;
pub mod proxy;
pub mod scanner;

use proxy::SharedProxy;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let state = SharedProxy::default();

    let data_dir = std::env::var("LOCALAPPDATA")
        .or_else(|_| std::env::var("APPDATA"))
        .ok()
        .map(std::path::PathBuf::from)
        .map(|p| p.join("WebProxyToolkit"));
    if let Some(dir) = data_dir {
        match db::open(&dir.join("project.wpt")) {
            Ok(database) => proxy::hydrate(&state, &database),
            Err(e) => eprintln!("project db open failed: {e}"),
        }
    }

    commands::register(
        tauri::Builder::default().plugin(tauri_plugin_opener::init()),
        state,
    )
    .run(tauri::generate_context!())
    .expect("error while running WebProxy Toolkit");
}
