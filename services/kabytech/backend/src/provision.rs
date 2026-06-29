//! Best-effort WS client that asks the manager to materialize THIS user's app
//! sandbox template at first login. Mirrors admin-api/src/manager.rs: the token
//! rides the Authorization: Bearer header (never the URL). The manager's
//! /provision verifies the JWT (chat.user) and self-scopes to the token's user.

use futures_util::{SinkExt, StreamExt};
use serde_json::json;
use tokio_tungstenite::tungstenite::Message;

/// Open the manager /provision WS, send one provision request, read one reply.
/// Best-effort: a transport/timeout/err reply is returned as Err for the caller
/// to log — it must NOT block login. 8s budget per phase.
pub async fn provision_app_box(url: &str, token: &str, app: &str) -> Result<(), String> {
    use tokio_tungstenite::tungstenite::client::IntoClientRequest;
    let mut req = url
        .into_client_request()
        .map_err(|e| format!("bad provision url {url}: {e}"))?;
    req.headers_mut().insert(
        "Authorization",
        format!("Bearer {token}")
            .parse()
            .map_err(|e| format!("bad auth header: {e}"))?,
    );
    let connect = tokio_tungstenite::connect_async(req);
    let (mut ws, _) = tokio::time::timeout(std::time::Duration::from_secs(8), connect)
        .await
        .map_err(|_| "provision connect timeout".to_string())?
        .map_err(|e| format!("provision connect: {e}"))?;
    ws.send(Message::Text(json!({"type":"provision","app":app}).to_string()))
        .await
        .map_err(|e| format!("provision send: {e}"))?;
    let reply = match tokio::time::timeout(std::time::Duration::from_secs(8), ws.next()).await {
        Ok(Some(Ok(Message::Text(t)))) => t,
        Ok(Some(Ok(Message::Binary(b)))) => String::from_utf8_lossy(&b).into_owned(),
        Ok(_) => return Err("provision: closed early".into()),
        Err(_) => return Err("provision reply timeout".into()),
    };
    let _ = ws.close(None).await;
    let v: serde_json::Value =
        serde_json::from_str(&reply).map_err(|e| format!("provision reply: {e}"))?;
    if v.get("type").and_then(|t| t.as_str()) == Some("provision")
        && v.get("ok").and_then(|t| t.as_bool()) == Some(true)
    {
        Ok(())
    } else {
        Err(format!("provision rejected: {reply}"))
    }
}
