//! /chat WebSocket bridge. The shell owns the socket; the webview drives it via
//! commands and receives the manager's frames as the Tauri event `chat://frame`.

use futures_util::{SinkExt, StreamExt};
use tauri::{AppHandle, Emitter, State};
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::connect_async;

use crate::auth::AppState;

/// Connect to the manager, drain `initialized`, and forward every frame to the
/// webview as `chat://frame`. Returns the backend session id.
#[tauri::command]
pub async fn chat_connect(app: AppHandle, state: State<'_, AppState>) -> Result<String, String> {
    let cfg = state.config.clone();
    let token = state.access_token().ok_or("not signed in")?;

    let mut request = cfg
        .manager_ws
        .as_str()
        .into_client_request()
        .map_err(|e| format!("bad manager URL {}: {e}", cfg.manager_ws))?;
    request.headers_mut().insert(
        "Authorization",
        format!("Bearer {token}")
            .parse()
            .map_err(|e| format!("bad auth header: {e}"))?,
    );

    let (ws, _) = connect_async(request)
        .await
        .map_err(|e| format!("cannot reach the manager: {e}"))?;
    let (sink, mut stream) = ws.split();
    *state.chat_sink.lock().await = Some(sink);

    // Reader task: forward frames to the webview by type.
    let app2 = app.clone();
    tokio::spawn(async move {
        while let Some(msg) = stream.next().await {
            match msg {
                Ok(Message::Text(t)) => {
                    if let Ok(v) = serde_json::from_str::<serde_json::Value>(&t) {
                        let _ = app2.emit("chat://frame", v);
                    }
                }
                Ok(Message::Binary(b)) => {
                    if let Ok(v) = serde_json::from_slice::<serde_json::Value>(&b) {
                        let _ = app2.emit("chat://frame", v);
                    }
                }
                Ok(Message::Close(_)) | Err(_) => break,
                _ => {}
            }
        }
        let _ = app2.emit("chat://closed", ());
    });

    Ok("connected".into())
}

/// Send a question frame `{type:q, id, text}`.
#[tauri::command]
pub async fn chat_send(state: State<'_, AppState>, id: String, text: String) -> Result<(), String> {
    let q = serde_json::json!({ "type": "q", "id": id, "text": text }).to_string();
    let mut guard = state.chat_sink.lock().await;
    let sink = guard.as_mut().ok_or("not connected")?;
    sink.send(Message::Text(q))
        .await
        .map_err(|e| format!("send failed: {e}"))?;
    Ok(())
}

#[tauri::command]
pub async fn chat_close(state: State<'_, AppState>) -> Result<(), String> {
    if let Some(mut sink) = state.chat_sink.lock().await.take() {
        let _ = sink.close().await;
    }
    Ok(())
}
