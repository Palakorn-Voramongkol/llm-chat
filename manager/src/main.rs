// llm-chat-manager — spawns N llm-chat backends and proxies a unified
// WebSocket API to them. Routes per-session traffic to the backend that owns
// that session.
//
// Endpoints (manager port, default 7777):
//   /                — JSON list of all session ids across all backends
//   /control         — JSON command channel (same vocabulary as a backend's
//                       /control, plus "instances" to inspect backends)
//   /s/<sessionId>   — bidirectional raw PTY bridge to the owning backend
//   /qa/<sessionId>  — read-only Q&A stream from the owning backend
//
// Configuration (env vars):
//   MANAGER_PORT          — manager WS port (default 7777)
//   MANAGER_INSTANCES     — number of backends to spawn (default 2)
//   MANAGER_START_PORT    — first backend port; consecutive ports are used
//                            (default 7878)
//   LLM_CHAT_EXE          — path to llm-chat.exe (default
//                            "../src-tauri/target/debug/llm-chat.exe"
//                            relative to manager.exe location)

use std::collections::HashMap;
use std::sync::Arc;

use futures_util::{SinkExt, StreamExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::Mutex;
use tokio_tungstenite::{
    connect_async,
    tungstenite::{
        client::IntoClientRequest,
        handshake::server::{ErrorResponse, Request, Response},
        http,
        Message,
    },
};

fn env_or<T: std::str::FromStr>(key: &str, default: T) -> T {
    std::env::var(key)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

fn random_token() -> String {
    use rand::RngCore;
    let mut bytes = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut bytes);
    bytes.iter().map(|b| format!("{:02x}", b)).collect()
}

fn auth_token_path() -> std::path::PathBuf {
    let dir = std::env::temp_dir().join("llm-chat-qa");
    let _ = std::fs::create_dir_all(&dir);
    dir.join("auth.token")
}

fn check_token_eq(provided: &str, expected: &str) -> bool {
    use subtle::ConstantTimeEq;
    provided.as_bytes().ct_eq(expected.as_bytes()).into()
}

fn extract_token(req: &Request) -> Option<String> {
    if let Some(auth) = req.headers().get("authorization") {
        if let Ok(s) = auth.to_str() {
            if let Some(t) = s.strip_prefix("Bearer ") {
                return Some(t.trim().to_string());
            }
        }
    }
    if let Some(query) = req.uri().query() {
        for kv in query.split('&') {
            if let Some(t) = kv.strip_prefix("token=") {
                return Some(t.to_string());
            }
        }
    }
    None
}

#[derive(Default)]
struct ManagerState {
    /// Backend ports, in spawn order.
    instance_ports: Vec<u16>,
    /// sessionId -> backend port that owns it.
    session_to_port: HashMap<String, u16>,
    /// Shared auth token used for both manager↔client and manager↔backend.
    auth_token: String,
}

type SharedState = Arc<Mutex<ManagerState>>;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let manager_port: u16 = env_or("MANAGER_PORT", 7777u16);
    let n_instances: usize = env_or("MANAGER_INSTANCES", 2usize);
    let start_port: u16 = env_or("MANAGER_START_PORT", 7878u16);
    let exe_path = std::env::var("LLM_CHAT_EXE").unwrap_or_else(|_| {
        let exe_dir = std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|p| p.to_path_buf()))
            .unwrap_or_else(|| std::path::PathBuf::from("."));
        exe_dir
            .join("..")
            .join("..")
            .join("..")
            .join("src-tauri")
            .join("target")
            .join("debug")
            .join("llm-chat.exe")
            .to_string_lossy()
            .into_owned()
    });

    eprintln!(
        "[manager] starting; manager_port={} instances={} start_port={} exe={}",
        manager_port, n_instances, start_port, exe_path
    );

    // Generate a single per-process auth token used for every WS connection
    // (manager↔backend and client↔manager). Persist it so local clients can
    // read it from a known file.
    let auth_token = random_token();
    let token_path = auth_token_path();
    std::fs::write(&token_path, &auth_token)?;
    eprintln!(
        "[manager] auth token written to {}",
        token_path.display()
    );

    let mut ports = Vec::new();
    for i in 0..n_instances {
        let port = start_port + i as u16;
        spawn_instance(&exe_path, port, &auth_token)?;
        ports.push(port);
    }

    eprintln!("[manager] waiting for backends to be ready...");
    for &p in &ports {
        wait_for_tcp(p, 90).await?;
        eprintln!("[manager]   :{} OK", p);
    }

    let state: SharedState = Arc::new(Mutex::new(ManagerState {
        instance_ports: ports.clone(),
        session_to_port: HashMap::new(),
        auth_token: auth_token.clone(),
    }));

    let listener = TcpListener::bind(("127.0.0.1", manager_port)).await?;
    eprintln!("[manager] listening on ws://127.0.0.1:{}", manager_port);

    loop {
        let (stream, peer) = listener.accept().await?;
        let state = state.clone();
        tokio::spawn(async move {
            if let Err(e) = handle_client(stream, state).await {
                eprintln!("[manager] client {peer} error: {e}");
            }
        });
    }
}

fn spawn_instance(exe: &str, port: u16, auth_token: &str) -> std::io::Result<()> {
    use std::process::Command;
    let path = std::path::Path::new(exe);
    let canon = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    eprintln!("[manager] spawning {} on port {}", canon.display(), port);
    Command::new(&canon)
        .env("LLM_CHAT_WS_PORT", port.to_string())
        .env("LLM_CHAT_AUTH_TOKEN", auth_token)
        .spawn()?;
    Ok(())
}

async fn wait_for_tcp(port: u16, retries: u32) -> Result<(), std::io::Error> {
    for _ in 0..retries {
        if TcpStream::connect(("127.0.0.1", port)).await.is_ok() {
            return Ok(());
        }
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    }
    Err(std::io::Error::new(
        std::io::ErrorKind::TimedOut,
        format!("backend on port {} did not come up", port),
    ))
}

async fn handle_client(
    stream: TcpStream,
    state: SharedState,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let expected_token = state.lock().await.auth_token.clone();
    let path_holder = Arc::new(std::sync::Mutex::new(String::new()));
    let path_capture = path_holder.clone();
    let cb = move |req: &Request, resp: Response| -> Result<Response, ErrorResponse> {
        *path_capture.lock().unwrap() = req.uri().path().to_string();
        if let Some(origin) = req.headers().get("origin") {
            let s = origin.to_str().unwrap_or("");
            if s.starts_with("http://") || s.starts_with("https://") {
                return Err(http::Response::builder()
                    .status(http::StatusCode::FORBIDDEN)
                    .body(Some("origin not allowed".to_string()))
                    .unwrap());
            }
        }
        let provided = match extract_token(req) {
            Some(t) => t,
            None => {
                return Err(http::Response::builder()
                    .status(http::StatusCode::UNAUTHORIZED)
                    .body(Some("missing auth token".to_string()))
                    .unwrap());
            }
        };
        if !check_token_eq(&provided, &expected_token) {
            return Err(http::Response::builder()
                .status(http::StatusCode::UNAUTHORIZED)
                .body(Some("invalid auth token".to_string()))
                .unwrap());
        }
        Ok(resp)
    };
    let ws = tokio_tungstenite::accept_hdr_async(stream, cb).await?;
    let req_path = path_holder.lock().unwrap().clone();

    if req_path == "/control" {
        return handle_control(ws, state).await;
    }
    if req_path.starts_with("/s/") {
        let sid = &req_path[3..];
        return bridge_session(ws, state, sid, "/s/").await;
    }
    if req_path.starts_with("/qa/") {
        let sid = &req_path[4..];
        return bridge_session(ws, state, sid, "/qa/").await;
    }
    if req_path == "/" || req_path.is_empty() {
        return handle_root(ws, state).await;
    }

    let (mut sink, _) = ws.split();
    let _ = sink
        .send(Message::Text(format!("unknown path: {}", req_path)))
        .await;
    Ok(())
}

// ---------- /control ----------

async fn handle_control(
    ws: tokio_tungstenite::WebSocketStream<TcpStream>,
    state: SharedState,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let (mut sink, mut stream) = ws.split();
    let _ = sink
        .send(Message::Text(
            serde_json::json!({"ok":true,"hello":"manager-control"}).to_string(),
        ))
        .await;

    while let Some(msg) = stream.next().await {
        let text = match msg {
            Ok(Message::Text(t)) => t,
            Ok(Message::Close(_)) => break,
            Ok(_) => continue,
            Err(_) => break,
        };
        let req: serde_json::Value = serde_json::from_str(&text).unwrap_or(serde_json::json!({}));
        let cmd = req.get("cmd").and_then(|v| v.as_str()).unwrap_or("");
        let reply = match cmd {
            "instances" => {
                let st = state.lock().await;
                let ports = st.instance_ports.clone();
                let mut counts = serde_json::Map::new();
                for p in &ports {
                    let c = st
                        .session_to_port
                        .values()
                        .filter(|x| **x == *p)
                        .count();
                    counts.insert(p.to_string(), serde_json::json!(c));
                }
                serde_json::json!({"ok":true,"ports":ports,"sessionsPerPort":counts})
            }
            "open" => match cmd_open(&state).await {
                Ok((sid, port)) => serde_json::json!({"ok":true,"sessionId":sid,"backendPort":port}),
                Err(e) => serde_json::json!({"ok":false,"error":e.to_string()}),
            },
            "close" => {
                let sid = req
                    .get("sessionId")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                match cmd_close(&state, &sid).await {
                    Ok(_) => serde_json::json!({"ok":true,"sessionId":sid}),
                    Err(e) => serde_json::json!({"ok":false,"error":e.to_string()}),
                }
            }
            "list" | "info" => {
                let ports = state.lock().await.instance_ports.clone();
                let mut all: Vec<String> = Vec::new();
                let mut per_backend = serde_json::Map::new();
                for p in &ports {
                    let resp = call_backend(*p, serde_json::json!({"cmd":"list"})).await;
                    match resp {
                        Ok(v) => {
                            if let Some(arr) = v.get("sessions").and_then(|x| x.as_array()) {
                                for s in arr {
                                    if let Some(s) = s.as_str() {
                                        all.push(s.to_string());
                                    }
                                }
                                per_backend.insert(p.to_string(), v);
                            }
                        }
                        Err(e) => {
                            per_backend.insert(p.to_string(), serde_json::json!({"error":e.to_string()}));
                        }
                    }
                }
                serde_json::json!({
                    "ok": true,
                    "count": all.len(),
                    "sessions": all,
                    "byBackend": per_backend,
                })
            }
            "history" => {
                let sid = req.get("sessionId").and_then(|v| v.as_str());
                let mut req_to_backend = serde_json::json!({"cmd": "history"});
                if let Some(sid) = sid {
                    let port = lookup_port(&state, sid).await;
                    match port {
                        Some(p) => {
                            req_to_backend["sessionId"] = serde_json::json!(sid);
                            match call_backend(p, req_to_backend).await {
                                Ok(v) => v,
                                Err(e) => serde_json::json!({"ok":false,"error":e.to_string()}),
                            }
                        }
                        None => {
                            serde_json::json!({"ok":false,"error":"unknown sessionId"})
                        }
                    }
                } else {
                    // Aggregate from every backend
                    let ports = state.lock().await.instance_ports.clone();
                    let mut all = serde_json::Map::new();
                    for p in &ports {
                        if let Ok(v) = call_backend(*p, serde_json::json!({"cmd":"history"})).await {
                            if let Some(map) = v.get("histories").and_then(|x| x.as_object()) {
                                for (k, val) in map {
                                    all.insert(k.clone(), val.clone());
                                }
                            }
                        }
                    }
                    serde_json::json!({"ok":true,"histories":all})
                }
            }
            "switch" | "clear" | "current" | "log" => {
                let sid = req.get("sessionId").and_then(|v| v.as_str()).map(|s| s.to_string());
                let port_opt = match &sid {
                    Some(s) => lookup_port(&state, s).await,
                    None => state.lock().await.instance_ports.first().copied(),
                };
                match port_opt {
                    Some(p) => match call_backend(p, req.clone()).await {
                        Ok(v) => v,
                        Err(e) => serde_json::json!({"ok":false,"error":e.to_string()}),
                    },
                    None => serde_json::json!({"ok":false,"error":"no backend available"}),
                }
            }
            other => serde_json::json!({"ok":false,"error":format!("unknown cmd: {}", other)}),
        };
        if sink.send(Message::Text(reply.to_string())).await.is_err() {
            break;
        }
    }
    Ok(())
}

async fn lookup_port(state: &SharedState, sid: &str) -> Option<u16> {
    state.lock().await.session_to_port.get(sid).copied()
}

async fn pick_least_loaded_port(state: &SharedState) -> Option<u16> {
    let st = state.lock().await;
    let mut counts: HashMap<u16, usize> = st
        .instance_ports
        .iter()
        .map(|p| (*p, 0usize))
        .collect();
    for &p in st.session_to_port.values() {
        *counts.entry(p).or_insert(0) += 1;
    }
    counts.into_iter().min_by_key(|(_, c)| *c).map(|(p, _)| p)
}

async fn cmd_open(
    state: &SharedState,
) -> Result<(String, u16), Box<dyn std::error::Error + Send + Sync>> {
    let port = pick_least_loaded_port(state)
        .await
        .ok_or("no backends configured")?;
    let resp = call_backend(port, serde_json::json!({"cmd":"open"})).await?;
    let sid = resp
        .get("sessionId")
        .and_then(|v| v.as_str())
        .ok_or("backend did not return sessionId")?
        .to_string();
    state.lock().await.session_to_port.insert(sid.clone(), port);
    Ok((sid, port))
}

async fn cmd_close(
    state: &SharedState,
    sid: &str,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let port = lookup_port(state, sid).await.ok_or("unknown sessionId")?;
    let _ = call_backend(
        port,
        serde_json::json!({"cmd":"close","sessionId":sid}),
    )
    .await?;
    state.lock().await.session_to_port.remove(sid);
    Ok(())
}

/// Build a tungstenite request to a backend with the auth token attached.
fn auth_request(
    url: &str,
    token: &str,
) -> Result<tokio_tungstenite::tungstenite::handshake::client::Request, Box<dyn std::error::Error + Send + Sync>>
{
    let mut req = url.into_client_request()?;
    req.headers_mut()
        .insert("Authorization", format!("Bearer {}", token).parse()?);
    Ok(req)
}

/// Open a fresh /control WS to a backend, send one command, read one reply,
/// close. Useful for one-shot RPC calls from the manager.
async fn call_backend(
    port: u16,
    req: serde_json::Value,
) -> Result<serde_json::Value, Box<dyn std::error::Error + Send + Sync>> {
    // Read the token from disk every call — manager wrote it at startup.
    // (For perf this could be cached on AppState; left simple for now.)
    let token = std::fs::read_to_string(auth_token_path())?
        .trim()
        .to_string();
    let url = format!("ws://127.0.0.1:{}/control", port);
    let (mut ws, _) = connect_async(auth_request(&url, &token)?).await?;
    // Discard the initial hello banner
    let _ = tokio::time::timeout(std::time::Duration::from_secs(2), ws.next()).await;
    ws.send(Message::Text(req.to_string())).await?;
    let reply = match tokio::time::timeout(std::time::Duration::from_secs(15), ws.next()).await {
        Ok(Some(Ok(Message::Text(t)))) => serde_json::from_str(&t)?,
        Ok(Some(Ok(other))) => return Err(format!("non-text reply: {:?}", other).into()),
        Ok(Some(Err(e))) => return Err(Box::new(e)),
        Ok(None) => return Err("backend closed".into()),
        Err(_) => return Err("backend timeout".into()),
    };
    let _ = ws.send(Message::Close(None)).await;
    Ok(reply)
}

// ---------- /s/<sid> and /qa/<sid> bridge ----------

async fn bridge_session(
    ws: tokio_tungstenite::WebSocketStream<TcpStream>,
    state: SharedState,
    sid: &str,
    base: &str,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let (mut client_sink, mut client_stream) = ws.split();
    let port = match lookup_port(&state, sid).await {
        Some(p) => p,
        None => {
            let _ = client_sink
                .send(Message::Text(format!("unknown sessionId: {}", sid)))
                .await;
            return Ok(());
        }
    };
    let url = format!("ws://127.0.0.1:{}{}{}", port, base, sid);
    let token = state.lock().await.auth_token.clone();
    let req_with_auth = match auth_request(&url, &token) {
        Ok(r) => r,
        Err(e) => {
            let _ = client_sink
                .send(Message::Text(format!("auth request build failed: {}", e)))
                .await;
            return Ok(());
        }
    };
    let (backend_ws, _) = match connect_async(req_with_auth).await {
        Ok(p) => p,
        Err(e) => {
            let _ = client_sink
                .send(Message::Text(format!("backend connect failed: {}", e)))
                .await;
            return Ok(());
        }
    };
    let (mut backend_sink, mut backend_stream) = backend_ws.split();

    // forward client -> backend
    let c2b = tokio::spawn(async move {
        while let Some(msg) = client_stream.next().await {
            match msg {
                Ok(Message::Close(_)) => break,
                Ok(m) => {
                    if backend_sink.send(m).await.is_err() {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
        let _ = backend_sink.close().await;
    });

    // forward backend -> client
    let b2c = tokio::spawn(async move {
        while let Some(msg) = backend_stream.next().await {
            match msg {
                Ok(Message::Close(_)) => break,
                Ok(m) => {
                    if client_sink.send(m).await.is_err() {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
        let _ = client_sink.close().await;
    });

    let _ = tokio::join!(c2b, b2c);
    Ok(())
}

// ---------- / (root) ----------

async fn handle_root(
    ws: tokio_tungstenite::WebSocketStream<TcpStream>,
    state: SharedState,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let (mut sink, _) = ws.split();
    let (ports, token) = {
        let st = state.lock().await;
        (st.instance_ports.clone(), st.auth_token.clone())
    };
    let mut all: Vec<String> = Vec::new();
    for p in &ports {
        let url = format!("ws://127.0.0.1:{}/", p);
        let req = match auth_request(&url, &token) {
            Ok(r) => r,
            Err(_) => continue,
        };
        if let Ok((mut bws, _)) = connect_async(req).await {
            if let Ok(Some(Ok(Message::Text(t)))) =
                tokio::time::timeout(std::time::Duration::from_secs(2), bws.next()).await
            {
                if let Ok(arr) = serde_json::from_str::<Vec<String>>(&t) {
                    all.extend(arr);
                }
            }
            let _ = bws.send(Message::Close(None)).await;
        }
    }
    let _ = sink
        .send(Message::Text(serde_json::to_string(&all).unwrap_or_default()))
        .await;
    Ok(())
}
