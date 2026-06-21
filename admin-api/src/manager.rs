//! Read-only BFF client for the llm-chat manager's /control WebSocket — the
//! Sessions page's "active chat sessions" panel.
//!
//! Authn/z: the manager requires a Zitadel JWT with chat.user at the handshake
//! and chat.admin for /control (the ops surface). The SA holds both via its
//! project user-grant; `ZitadelClient::mint_chat_token` mints the token with
//! the project audience + asserted roles. This module only ever sends
//! read-only commands ("list", "instances") — never "open"/"close".

use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use tokio_tungstenite::tungstenite::Message;

/// PURE: combine the manager's `list`, `instances` and `clients` replies into
/// the Sessions-panel payload. Any reply may be an error object; pass each
/// through under its own key so one failing query never blanks the others.
pub fn combine_control_replies(list: Value, instances: Value, clients: Value) -> Value {
    let ok = list.get("ok").and_then(Value::as_bool).unwrap_or(false)
        || instances.get("ok").and_then(Value::as_bool).unwrap_or(false)
        || clients.get("ok").and_then(Value::as_bool).unwrap_or(false);
    json!({
        "configured": true,
        "ok": ok,
        "list": list,
        "instances": instances,
        "clients": clients,
    })
}

/// Open the manager /control WS, send one command, read one reply.
/// The hello frame ({"ok":true,"hello":"manager-control"}) is consumed first.
/// The token rides the `Authorization: Bearer` header (never the URL, so it
/// can't leak into access/proxy logs) — same as the python/rust chat clients.
pub async fn control_query(url: &str, token: &str, cmd: &str) -> Result<Value, String> {
    use tokio_tungstenite::tungstenite::client::IntoClientRequest;
    let mut request = url
        .into_client_request()
        .map_err(|e| format!("bad manager control URL {url}: {e}"))?;
    request.headers_mut().insert(
        "Authorization",
        format!("Bearer {token}")
            .parse()
            .map_err(|e| format!("bad auth header: {e}"))?,
    );
    let (mut ws, _) = tokio_tungstenite::connect_async(request)
        .await
        .map_err(|e| format!("manager connect: {e}"))?;

    // Consume the hello frame (or treat a missing one as the reply).
    let hello = read_text(&mut ws).await?;
    let hello_v: Value = serde_json::from_str(&hello).unwrap_or(json!({}));
    let is_hello = hello_v.get("hello").is_some();

    ws.send(Message::Text(json!({ "cmd": cmd }).to_string()))
        .await
        .map_err(|e| format!("manager send: {e}"))?;

    let reply = if is_hello { read_text(&mut ws).await? } else { hello };
    let _ = ws.close(None).await;
    serde_json::from_str(&reply).map_err(|e| format!("manager reply parse: {e}"))
}

async fn read_text(
    ws: &mut tokio_tungstenite::WebSocketStream<
        tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
    >,
) -> Result<String, String> {
    let deadline = std::time::Duration::from_secs(8);
    loop {
        let msg = tokio::time::timeout(deadline, ws.next())
            .await
            .map_err(|_| "manager read timeout".to_string())?
            .ok_or("manager closed early")?
            .map_err(|e| format!("manager read: {e}"))?;
        match msg {
            Message::Text(t) => return Ok(t),
            Message::Close(_) => return Err("manager closed early".into()),
            _ => continue,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn combine_carries_all_replies_and_overall_ok() {
        let out = combine_control_replies(
            json!({"ok": true, "count": 2, "sessions": ["s1", "s2"]}),
            json!({"ok": true, "ports": [7878], "sessionsPerPort": {"7878": 2}}),
            json!({"ok": true, "count": 1, "clients": [{"sid": "s1", "userId": "u1", "backendPort": 7878}]}),
        );
        assert_eq!(out["configured"], true);
        assert_eq!(out["ok"], true);
        assert_eq!(out["list"]["count"], 2);
        assert_eq!(out["instances"]["ports"][0], 7878);
        assert_eq!(out["clients"]["clients"][0]["userId"], "u1");
    }

    #[test]
    fn combine_degrades_per_reply_not_whole() {
        let out = combine_control_replies(
            json!({"ok": false, "error": "backend down"}),
            json!({"ok": true, "ports": [7878]}),
            json!({"ok": false, "error": "backend down"}),
        );
        assert_eq!(out["ok"], true); // one good reply still renders
        assert_eq!(out["list"]["error"], "backend down");
    }
}
