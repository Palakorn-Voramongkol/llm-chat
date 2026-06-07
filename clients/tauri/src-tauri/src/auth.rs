//! Authentication — Zitadel Authorization Code + PKCE (hosted login).
//!
//! Zitadel's password grant (ROPC) is disabled for this client, so we use the
//! secure standard flow: open the system browser to Zitadel's login, capture
//! the loopback redirect, and exchange the code for tokens. The webview never
//! sees the refresh token (kept in the OS keyring); the access token lives only
//! in this process; the frontend gets a sanitized `Identity`.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use base64::Engine;
use serde::Serialize;
use sha2::{Digest, Sha256};
use tauri::State;

use crate::config::Config;
use crate::tokens;

const LOOPBACK_PORT: u16 = 8477; // must match the OIDC app's registered redirect

#[derive(Clone, Serialize, Default)]
pub struct Identity {
    pub sub: String,
    pub email: Option<String>,
    pub name: Option<String>,
    pub roles: Vec<String>,
}

#[derive(Default)]
pub struct Session {
    pub access_token: Option<String>,
    pub refresh_token: Option<String>,
    pub identity: Option<Identity>,
}

pub struct AppState {
    pub config: Config,
    pub session: Mutex<Session>,
}

impl AppState {
    pub fn new() -> Self {
        AppState {
            config: Config::load(),
            session: Mutex::new(Session::default()),
        }
    }
    /// The current access token, for the chat WS bridge.
    pub fn access_token(&self) -> Option<String> {
        self.session.lock().unwrap().access_token.clone()
    }
}

struct Endpoints {
    authorize: String,
    token: String,
    revoke: String,
}

// ---------------- pure helpers ----------------

fn b64url_nopad(raw: &[u8]) -> String {
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(raw)
}

fn make_pkce() -> (String, String) {
    use rand::RngCore;
    let mut bytes = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut bytes);
    let verifier = b64url_nopad(&bytes);
    let challenge = b64url_nopad(&Sha256::digest(verifier.as_bytes()));
    (verifier, challenge)
}

fn make_state() -> String {
    use rand::RngCore;
    let mut bytes = [0u8; 16];
    rand::thread_rng().fill_bytes(&mut bytes);
    b64url_nopad(&bytes)
}

fn scope(project: &Option<String>) -> String {
    let mut s = String::from("openid profile email offline_access");
    if let Some(p) = project {
        s.push_str(&format!(
            " urn:zitadel:iam:org:project:id:{p}:aud urn:zitadel:iam:org:projects:roles"
        ));
    }
    s
}

fn build_authorize_url(
    authorize: &str,
    client_id: &str,
    redirect_uri: &str,
    scope: &str,
    challenge: &str,
    state: &str,
) -> String {
    let q = url::form_urlencoded::Serializer::new(String::new())
        .append_pair("client_id", client_id)
        .append_pair("redirect_uri", redirect_uri)
        .append_pair("response_type", "code")
        .append_pair("scope", scope)
        .append_pair("code_challenge", challenge)
        .append_pair("code_challenge_method", "S256")
        .append_pair("state", state)
        .append_pair("prompt", "login")
        .finish();
    format!("{authorize}?{q}")
}

fn parse_callback(path: &str) -> HashMap<String, String> {
    match url::Url::parse(&format!("http://localhost{path}")) {
        Ok(u) => {
            let mut m = HashMap::new();
            for (k, v) in u.query_pairs() {
                m.entry(k.into_owned()).or_insert_with(|| v.into_owned());
            }
            m
        }
        Err(_) => HashMap::new(),
    }
}

fn decode_claims(token: &str) -> serde_json::Map<String, serde_json::Value> {
    let parts: Vec<&str> = token.split('.').collect();
    if parts.len() < 2 {
        return Default::default();
    }
    let mut payload = parts[1].to_string();
    while payload.len() % 4 != 0 {
        payload.push('=');
    }
    base64::engine::general_purpose::URL_SAFE
        .decode(payload.as_bytes())
        .ok()
        .and_then(|b| serde_json::from_slice::<serde_json::Value>(&b).ok())
        .and_then(|v| v.as_object().cloned())
        .unwrap_or_default()
}

fn identity_from_token(token: &str) -> Identity {
    let claims = decode_claims(token);
    let sub = claims.get("sub").and_then(|v| v.as_str()).unwrap_or("").to_string();
    let email = claims.get("email").and_then(|v| v.as_str()).map(String::from);
    let name = claims
        .get("name")
        .and_then(|v| v.as_str())
        .or_else(|| claims.get("preferred_username").and_then(|v| v.as_str()))
        .map(String::from);
    let mut roles = Vec::new();
    for (k, v) in &claims {
        if k.ends_with(":roles") {
            if let Some(obj) = v.as_object() {
                roles.extend(obj.keys().cloned());
            }
        }
    }
    roles.sort();
    roles.dedup();
    Identity { sub, email, name, roles }
}

// ---------------- network ----------------

async fn discover(issuer: &str) -> Endpoints {
    let base = issuer.trim_end_matches('/').to_string();
    let fallback = Endpoints {
        authorize: format!("{base}/oauth/v2/authorize"),
        token: format!("{base}/oauth/v2/token"),
        revoke: format!("{base}/oauth/v2/revoke"),
    };
    let client = match reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
    {
        Ok(c) => c,
        Err(_) => return fallback,
    };
    let url = format!("{base}/.well-known/openid-configuration");
    let body: serde_json::Value = match client.get(&url).send().await {
        Ok(r) => match r.json().await {
            Ok(b) => b,
            Err(_) => return fallback,
        },
        Err(_) => return fallback,
    };
    match (
        body.get("authorization_endpoint").and_then(|v| v.as_str()),
        body.get("token_endpoint").and_then(|v| v.as_str()),
    ) {
        (Some(a), Some(t)) => Endpoints {
            authorize: a.to_string(),
            token: t.to_string(),
            revoke: body
                .get("revocation_endpoint")
                .and_then(|v| v.as_str())
                .map(String::from)
                .unwrap_or(fallback.revoke),
        },
        _ => fallback,
    }
}

async fn token_post(token_endpoint: &str, form: &[(&str, &str)]) -> Result<serde_json::Value, String> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(20))
        .build()
        .map_err(|e| e.to_string())?;
    let resp = client
        .post(token_endpoint)
        .form(form)
        .send()
        .await
        .map_err(|e| format!("cannot reach the sign-in server: {e}"))?;
    let status = resp.status();
    let text = resp.text().await.unwrap_or_default();
    if !status.is_success() {
        let msg = serde_json::from_str::<serde_json::Value>(&text)
            .ok()
            .and_then(|v| {
                v.get("error_description")
                    .or_else(|| v.get("error"))
                    .and_then(|e| e.as_str())
                    .map(String::from)
            })
            .unwrap_or_else(|| format!("sign-in failed (HTTP {})", status.as_u16()));
        return Err(msg);
    }
    serde_json::from_str(&text).map_err(|_| "sign-in server returned a non-JSON response".to_string())
}

/// Serve one redirect on 127.0.0.1:port (blocking — runs on a worker thread).
fn capture_redirect(port: u16, timeout: Duration) -> HashMap<String, String> {
    let server = match tiny_http::Server::http(("127.0.0.1", port)) {
        Ok(s) => s,
        Err(_) => return HashMap::new(),
    };
    let deadline = Instant::now() + timeout;
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return HashMap::new();
        }
        match server.recv_timeout(remaining) {
            Ok(Some(req)) => {
                let params = parse_callback(req.url());
                if params.contains_key("code") || params.contains_key("error") {
                    let ok = !params.contains_key("error");
                    let title = if ok { "Signed in ✓" } else { "Sign-in failed" };
                    let html = format!(
                        "<html><body style='font-family:system-ui;background:#0f172a;color:#e2e8f0;text-align:center;padding-top:4rem'>\
                         <h2>{title}</h2><p>You can close this tab and return to Lumina.</p></body></html>"
                    );
                    let header = tiny_http::Header::from_bytes(
                        &b"Content-Type"[..],
                        &b"text/html; charset=utf-8"[..],
                    )
                    .expect("static header");
                    let _ = req.respond(tiny_http::Response::from_string(html).with_header(header));
                    return params;
                }
                let _ = req.respond(tiny_http::Response::empty(404));
            }
            Ok(None) | Err(_) => return HashMap::new(),
        }
    }
}

fn set_session(state: &AppState, access: String, refresh: Option<String>, identity: Identity) {
    let mut s = state.session.lock().unwrap();
    s.access_token = Some(access);
    s.refresh_token = refresh;
    s.identity = Some(identity);
}

fn finish(state: &AppState, cfg: &Config, body: serde_json::Value) -> Result<Identity, String> {
    let access = body
        .get("access_token")
        .and_then(|v| v.as_str())
        .ok_or("sign-in response had no access token")?
        .to_string();
    let refresh = body.get("refresh_token").and_then(|v| v.as_str()).map(String::from);
    let identity = identity_from_token(&access);
    if let Some(rt) = &refresh {
        tokens::save_refresh(&cfg.issuer, rt);
    }
    set_session(state, access, refresh, identity.clone());
    Ok(identity)
}

// ---------------- Tauri commands ----------------

#[tauri::command]
pub fn get_config(state: State<'_, AppState>) -> Config {
    state.config.clone()
}

/// Run the full browser PKCE login and return the signed-in Identity.
#[tauri::command]
pub async fn login(state: State<'_, AppState>) -> Result<Identity, String> {
    let cfg = state.config.clone();
    let client_id = cfg
        .client_id
        .clone()
        .ok_or("no OIDC client id configured (set LUMINA_OIDC_CLIENT_ID)")?;
    let endpoints = discover(&cfg.issuer).await;
    let (verifier, challenge) = make_pkce();
    let st = make_state();
    let redirect_uri = format!("http://localhost:{LOOPBACK_PORT}/callback");
    let url = build_authorize_url(
        &endpoints.authorize,
        &client_id,
        &redirect_uri,
        &scope(&cfg.project),
        &challenge,
        &st,
    );

    // Capture the redirect on a worker thread; await it without blocking the runtime.
    let (tx, rx) = tokio::sync::oneshot::channel();
    std::thread::spawn(move || {
        let _ = tx.send(capture_redirect(LOOPBACK_PORT, Duration::from_secs(300)));
    });

    if open::that(&url).is_err() {
        return Err(format!("couldn't open a browser. Open this URL to sign in:\n{url}"));
    }

    let params = rx.await.map_err(|_| "sign-in was interrupted".to_string())?;
    if params.is_empty() {
        return Err("timed out waiting for the browser sign-in".into());
    }
    if let Some(e) = params.get("error") {
        let desc = params.get("error_description").cloned().unwrap_or_default();
        return Err(format!("sign-in failed: {e} {desc}").trim().to_string());
    }
    if params.get("state").map(String::as_str) != Some(st.as_str()) {
        return Err("state mismatch on the sign-in callback (possible CSRF) — aborting".into());
    }
    let code = params
        .get("code")
        .cloned()
        .filter(|c| !c.is_empty())
        .ok_or("no authorization code returned")?;

    let body = token_post(
        &endpoints.token,
        &[
            ("grant_type", "authorization_code"),
            ("client_id", &client_id),
            ("code", &code),
            ("redirect_uri", &redirect_uri),
            ("code_verifier", &verifier),
        ],
    )
    .await?;
    finish(&state, &cfg, body)
}

/// Restore a session from the keyring (refresh-token grant). None if not signed in.
#[tauri::command]
pub async fn restore(state: State<'_, AppState>) -> Result<Option<Identity>, String> {
    let cfg = state.config.clone();
    let client_id = match cfg.client_id.clone() {
        Some(c) => c,
        None => return Ok(None),
    };
    let refresh = match tokens::load_refresh(&cfg.issuer) {
        Some(r) => r,
        None => return Ok(None),
    };
    let endpoints = discover(&cfg.issuer).await;
    let body = match token_post(
        &endpoints.token,
        &[
            ("grant_type", "refresh_token"),
            ("refresh_token", &refresh),
            ("client_id", &client_id),
            ("scope", &scope(&cfg.project)),
        ],
    )
    .await
    {
        Ok(b) => b,
        Err(_) => {
            tokens::clear_refresh(&cfg.issuer);
            return Ok(None);
        }
    };
    // Reuse the old refresh token if Zitadel didn't rotate it.
    let mut body = body;
    if body.get("refresh_token").is_none() {
        if let Some(obj) = body.as_object_mut() {
            obj.insert("refresh_token".into(), serde_json::Value::String(refresh));
        }
    }
    finish(&state, &cfg, body).map(Some)
}

#[tauri::command]
pub async fn logout(state: State<'_, AppState>) -> Result<(), String> {
    let cfg = state.config.clone();
    let refresh = state.session.lock().unwrap().refresh_token.clone();
    if let (Some(rt), Some(cid)) = (refresh, cfg.client_id.clone()) {
        let endpoints = discover(&cfg.issuer).await;
        let _ = reqwest::Client::new()
            .post(&endpoints.revoke)
            .form(&[("token", rt.as_str()), ("client_id", cid.as_str())])
            .send()
            .await;
    }
    tokens::clear_refresh(&cfg.issuer);
    *state.session.lock().unwrap() = Session::default();
    Ok(())
}
