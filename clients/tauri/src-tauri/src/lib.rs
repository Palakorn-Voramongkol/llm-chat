//! Lumina — Tauri 2 shell. Owns secrets (keyring) and the network (OIDC token
//! calls + the /chat WebSocket); the webview talks to it over IPC commands and
//! receives chat frames as Tauri events.

mod auth;
mod chat;
mod config;
mod tokens;

use auth::AppState;

/// Open a URL in the system browser (links must not navigate the app webview).
/// Scheme-restricted as defense in depth — never hand `open` a `file:`/`javascript:`
/// /arbitrary-scheme URL from claude's markdown.
#[tauri::command]
fn open_external(url: String) -> Result<(), String> {
    let lower = url.to_ascii_lowercase();
    let allowed = lower.starts_with("http://")
        || lower.starts_with("https://")
        || lower.starts_with("mailto:");
    if !allowed {
        return Err("refusing to open a non-http(s)/mailto URL".into());
    }
    open::that(&url).map_err(|e| e.to_string())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .manage(AppState::new())
        .invoke_handler(tauri::generate_handler![
            auth::get_config,
            auth::login,
            auth::restore,
            auth::logout,
            chat::chat_connect,
            chat::chat_send,
            chat::chat_close,
            open_external,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Lumina");
}
