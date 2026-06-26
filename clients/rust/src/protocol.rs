//! Async client for the manager's typed `/chat` WebSocket. Port of `protocol.py`.
//!
//! Protocol (per message):
//!   client  -> {"type":"q","id":<id>,"text":<text>}
//!   manager -> {"type":"ack","id","seq"}            (receipt)
//!   manager -> {"type":"a","id","seq","text",...}   (the answer)
//!   client  -> {"type":"confirm","seq":<seq>}       (we got it)
//!   manager -> {"type":"err",...}                   (on failure)
//!
//! `ChatClient` keeps ONE connection (one backend session, so claude retains
//! context across `ask()` calls). If the socket drops it transparently
//! reconnects on the next `ask()`.

use std::sync::Arc;
use std::time::{Duration, Instant};

use futures_util::{SinkExt, StreamExt};
use serde_json::json;
use tokio::net::TcpStream;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::protocol::WebSocketConfig;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{connect_async_with_config, MaybeTlsStream, WebSocketStream};

use crate::errors::{Error, Result};

type Ws = WebSocketStream<MaybeTlsStream<TcpStream>>;

/// A callable returning a fresh access token (used for connect + reconnect).
/// Blocking (it does sync HTTP), so it is invoked via `spawn_blocking`.
pub type TokenProvider = Arc<dyn Fn() -> Result<String> + Send + Sync>;

/// A settled answer for one question.
#[derive(Debug, Clone)]
pub struct Answer {
    pub text: String,
    pub seq: i64,
    pub id: String,
    pub time_in: Option<String>,
    pub time_out: Option<String>,
    pub latency_ms: Option<i64>,
}

impl Answer {
    /// Latency in seconds. Prefers the manager's `latencyMs` (computed from the
    /// same timeIn/timeOut the Python client parses), so the displayed number
    /// matches without a date-parsing dependency.
    pub fn latency_s(&self) -> Option<f64> {
        self.latency_ms.filter(|&ms| ms >= 0).map(|ms| ms as f64 / 1000.0)
    }
}

enum Awaited {
    Answer(Answer),
    Closed,
}

pub struct ChatClient {
    url: String,
    token_provider: TokenProvider,
    max_reconnects: u32,
    ws: Option<Ws>,
    counter: u64,
    pub session_id: Option<String>,
}

impl ChatClient {
    pub fn new(manager_url: &str, token_provider: TokenProvider) -> Self {
        ChatClient {
            url: manager_url.to_string(),
            token_provider,
            max_reconnects: 1,
            ws: None,
            counter: 0,
            session_id: None,
        }
    }

    pub fn connected(&self) -> bool {
        self.ws.is_some()
    }

    /// Re-mint (machine) or refresh (human) the access token — used by display
    /// surfaces like the REPL's `/status`, which decode its claims.
    pub async fn current_token(&self) -> Result<String> {
        self.fetch_token().await
    }

    /// Request THIS user's own usage and return the manager's `usage` reply
    /// object. Sends `{"type":"usage"}` and awaits the `usage` frame, skipping
    /// any interleaved ack/answer frames.
    pub async fn usage(&mut self, timeout: Duration) -> Result<serde_json::Value> {
        if !self.connected() {
            self.connect().await?;
        }
        let req = json!({"type": "usage"}).to_string();
        match self.ws.as_mut() {
            Some(ws) => ws
                .send(Message::Text(req))
                .await
                .map_err(|e| Error::ManagerUnavailable(format!("usage send failed: {e}")))?,
            None => return Err(Error::ManagerUnavailable("not connected".into())),
        }
        let deadline = Instant::now() + timeout;
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(Error::AnswerTimeout("no usage reply within timeout".into()));
            }
            let ws = match self.ws.as_mut() {
                Some(w) => w,
                None => return Err(Error::ManagerUnavailable("connection closed".into())),
            };
            let frame = match tokio::time::timeout(remaining, ws.next()).await {
                Err(_) => return Err(Error::AnswerTimeout("no usage reply within timeout".into())),
                Ok(None) | Ok(Some(Ok(Message::Close(_)))) | Ok(Some(Err(_))) => {
                    return Err(Error::ManagerUnavailable("connection closed".into()))
                }
                Ok(Some(Ok(Message::Text(t)))) => t,
                Ok(Some(Ok(Message::Binary(b)))) => String::from_utf8_lossy(&b).into_owned(),
                Ok(Some(Ok(_))) => continue,
            };
            let msg: serde_json::Value = serde_json::from_str(&frame)
                .map_err(|_| Error::Protocol(format!("manager sent non-JSON frame: {frame}")))?;
            match msg.get("type").and_then(|t| t.as_str()) {
                Some("usage") => return Ok(msg),
                Some("err") => {
                    return Err(Error::Protocol(
                        msg.get("text").and_then(|v| v.as_str()).unwrap_or("usage error").to_string(),
                    ))
                }
                _ => continue, // skip ack / a / initialized
            }
        }
    }

    async fn fetch_token(&self) -> Result<String> {
        let p = self.token_provider.clone();
        tokio::task::spawn_blocking(move || p())
            .await
            .map_err(|e| Error::Auth(format!("token task failed: {e}")))?
    }

    /// Open the WebSocket and drain the manager's `initialized` frame.
    pub async fn connect(&mut self) -> Result<()> {
        let token = self.fetch_token().await?;
        let mut request = self
            .url
            .as_str()
            .into_client_request()
            .map_err(|e| Error::ManagerUnavailable(format!("bad manager URL {}: {e}", self.url)))?;
        request.headers_mut().insert(
            "Authorization",
            format!("Bearer {token}")
                .parse()
                .map_err(|e| Error::ManagerUnavailable(format!("bad auth header: {e}")))?,
        );
        let config = WebSocketConfig {
            max_message_size: None, // Python sets max_size=None
            max_frame_size: None,
            ..Default::default()
        };
        let connect = connect_async_with_config(request, Some(config), false);
        let (ws, _resp) = match tokio::time::timeout(Duration::from_secs(15), connect).await {
            Err(_) => {
                return Err(Error::ManagerUnavailable(format!(
                    "could not connect to {}: open timed out",
                    self.url
                )))
            }
            Ok(Err(e)) => {
                return Err(Error::ManagerUnavailable(format!(
                    "could not connect to {}: {e}",
                    self.url
                )))
            }
            Ok(Ok(pair)) => pair,
        };
        self.ws = Some(ws);

        // The manager leads with an `initialized` frame; capture the sid.
        match tokio::time::timeout(Duration::from_secs(10), self.recv_text()).await {
            Ok(Ok(Some(text))) => {
                if let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) {
                    if v.get("type").and_then(|t| t.as_str()) == Some("initialized") {
                        self.session_id =
                            v.get("sid").and_then(|s| s.as_str()).map(String::from);
                        tracing::debug!(
                            "connected sid={:?} backendPort={:?}",
                            self.session_id,
                            v.get("backendPort")
                        );
                    } else {
                        tracing::warn!(
                            "expected 'initialized', got {:?}",
                            v.get("type")
                        );
                    }
                }
                Ok(())
            }
            _ => {
                self.close().await;
                Err(Error::ManagerUnavailable(format!(
                    "no 'initialized' frame from {}",
                    self.url
                )))
            }
        }
    }

    pub async fn close(&mut self) {
        if let Some(mut ws) = self.ws.take() {
            let _ = ws.close(None).await;
        }
    }

    /// Receive the next text-ish message, or None if the socket closed/erred.
    async fn recv_text(&mut self) -> Result<Option<String>> {
        let ws = match self.ws.as_mut() {
            Some(w) => w,
            None => return Ok(None),
        };
        loop {
            match ws.next().await {
                Some(Ok(Message::Text(t))) => return Ok(Some(t)),
                Some(Ok(Message::Binary(b))) => {
                    return Ok(Some(String::from_utf8_lossy(&b).into_owned()))
                }
                Some(Ok(Message::Close(_))) | None => return Ok(None),
                Some(Ok(_)) => continue, // ping/pong/frame — keep reading
                Some(Err(_)) => return Ok(None),
            }
        }
    }

    /// Send a question and return its settled `Answer`. Transparently reconnects
    /// (up to `max_reconnects`) if the socket dropped.
    pub async fn ask(&mut self, text: &str, timeout: Duration) -> Result<Answer> {
        let mut attempts = 0u32;
        loop {
            if !self.connected() {
                self.connect().await?;
            }
            self.counter += 1;
            let msg_id = format!("m{}", self.counter);
            let q = json!({"type": "q", "id": msg_id, "text": text}).to_string();

            if let Some(ws) = self.ws.as_mut() {
                if ws.send(Message::Text(q)).await.is_err() {
                    self.close().await;
                    attempts += 1;
                    if attempts > self.max_reconnects {
                        return Err(Error::ManagerUnavailable(
                            "connection closed during ask()".into(),
                        ));
                    }
                    tracing::warn!("connection dropped, reconnecting (attempt {attempts})");
                    continue;
                }
            }

            match self.await_answer(&msg_id, Instant::now() + timeout).await? {
                Awaited::Answer(a) => return Ok(a),
                Awaited::Closed => {
                    self.close().await;
                    attempts += 1;
                    if attempts > self.max_reconnects {
                        return Err(Error::ManagerUnavailable(
                            "connection closed during ask()".into(),
                        ));
                    }
                    tracing::warn!("connection dropped, reconnecting (attempt {attempts})");
                    continue;
                }
            }
        }
    }

    async fn await_answer(&mut self, msg_id: &str, deadline: Instant) -> Result<Awaited> {
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(Error::AnswerTimeout(format!(
                    "no answer for {msg_id} within the timeout"
                )));
            }
            // Scope the &mut borrow so we can re-borrow self.ws to send `confirm`.
            let frame = {
                let ws = match self.ws.as_mut() {
                    Some(w) => w,
                    None => return Ok(Awaited::Closed),
                };
                match tokio::time::timeout(remaining, ws.next()).await {
                    Err(_) => {
                        return Err(Error::AnswerTimeout(format!(
                            "no answer for {msg_id} within the timeout"
                        )))
                    }
                    Ok(None) | Ok(Some(Ok(Message::Close(_)))) | Ok(Some(Err(_))) => {
                        return Ok(Awaited::Closed)
                    }
                    Ok(Some(Ok(Message::Text(t)))) => t,
                    Ok(Some(Ok(Message::Binary(b)))) => String::from_utf8_lossy(&b).into_owned(),
                    Ok(Some(Ok(_))) => continue, // ping/pong
                }
            };

            let msg: serde_json::Value = serde_json::from_str(&frame)
                .map_err(|_| Error::Protocol(format!("manager sent non-JSON frame: {frame}")))?;
            match msg.get("type").and_then(|t| t.as_str()) {
                Some("a") if msg.get("id").and_then(|i| i.as_str()) == Some(msg_id) => {
                    let seq = msg.get("seq").and_then(|s| s.as_i64()).unwrap_or(-1);
                    let confirm = json!({"type": "confirm", "seq": seq}).to_string();
                    if let Some(ws) = self.ws.as_mut() {
                        let _ = ws.send(Message::Text(confirm)).await;
                    }
                    return Ok(Awaited::Answer(Answer {
                        text: msg.get("text").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                        seq,
                        id: msg
                            .get("id")
                            .and_then(|v| v.as_str())
                            .unwrap_or(msg_id)
                            .to_string(),
                        time_in: msg.get("timeIn").and_then(|v| v.as_str()).map(String::from),
                        time_out: msg.get("timeOut").and_then(|v| v.as_str()).map(String::from),
                        latency_ms: msg.get("latencyMs").and_then(|v| v.as_i64()),
                    }));
                }
                Some("err") => {
                    return Err(Error::Protocol(
                        msg.get("text")
                            .and_then(|v| v.as_str())
                            .unwrap_or("manager returned an error")
                            .to_string(),
                    ))
                }
                // `initialized`, `ack`, or an unrelated `a` → keep waiting.
                other => {
                    tracing::debug!("skip frame type={:?} id={:?}", other, msg.get("id"));
                }
            }
        }
    }
}
