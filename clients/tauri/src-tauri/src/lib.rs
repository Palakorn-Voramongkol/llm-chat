//! Lumina — Tauri 2 shell. Owns secrets (keyring) and the network (OIDC token
//! calls + the /chat WebSocket); the webview talks to it over IPC commands and
//! receives chat frames as Tauri events.

#[tauri::command]
fn health() -> &'static str {
    "ok"
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![health])
        .run(tauri::generate_context!())
        .expect("error while running Lumina");
}
