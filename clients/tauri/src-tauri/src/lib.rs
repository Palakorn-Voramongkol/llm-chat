//! Lumina — Tauri 2 shell. Owns secrets (keyring) and the network (OIDC token
//! calls + the /chat WebSocket); the webview talks to it over IPC commands and
//! receives chat frames as Tauri events.

mod auth;
mod config;
mod tokens;

use auth::AppState;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .manage(AppState::new())
        .invoke_handler(tauri::generate_handler![
            auth::get_config,
            auth::login,
            auth::restore,
            auth::logout,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Lumina");
}
