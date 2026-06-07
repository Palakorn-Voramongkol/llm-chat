//! Headless worker entry point.
//!
//! Runs the WebSocket relay + stream-json Claude sessions with NO Tauri window,
//! so it works on a CLI-only Linux server (no X11/Wayland display). On Linux it
//! still links libwebkit2gtk (install the package), but never opens a window.
//!
//!   LLM_CHAT_WS_BIND=0.0.0.0 LLM_CHAT_WS_PORT=7878 \
//!   LLM_CHAT_AUTH_TOKEN=<shared> ./llm-chat-headless

fn main() {
    llm_chat_lib::run_headless();
}
