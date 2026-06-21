//! OAuth2 Authorization Code flow with PKCE — interactive human login.
//!
//! Port of `oidc.py`. A native/public client (no secret): prove possession with
//! PKCE (S256) + a CSRF `state`, catch the redirect on a loopback HTTP server,
//! and exchange the code for access + refresh + id tokens. `offline_access` is
//! requested so the session survives the short access-token lifetime.
//!
//! HTTP is synchronous (`reqwest::blocking`), matching the Python client — call
//! these from a sync context or via `tokio::task::spawn_blocking`.

use std::collections::HashMap;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use base64::Engine;
use sha2::{Digest, Sha256};

use crate::errors::{Error, Result};

const REQUEST_TIMEOUT: Duration = Duration::from_secs(20);

fn now_epoch() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}

// ---------------- token model ----------------

#[derive(Debug, Clone)]
pub struct TokenSet {
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub id_token: Option<String>,
    pub expires_at: f64, // epoch seconds
}

impl TokenSet {
    pub fn is_expired(&self) -> bool {
        self.is_expired_skew(30.0)
    }
    pub fn is_expired_skew(&self, skew: f64) -> bool {
        now_epoch() >= (self.expires_at - skew)
    }

    pub fn from_response(body: &serde_json::Value, now: f64) -> Result<TokenSet> {
        let access_token = body
            .get("access_token")
            .and_then(|v| v.as_str())
            .ok_or_else(|| Error::Auth("token response had no access_token".into()))?
            .to_string();
        let expires_in = body.get("expires_in").and_then(|v| v.as_i64()).unwrap_or(300);
        Ok(TokenSet {
            access_token,
            refresh_token: body
                .get("refresh_token")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string()),
            id_token: body
                .get("id_token")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string()),
            expires_at: now + expires_in as f64,
        })
    }
}

// ---------------- pure helpers ----------------

fn b64url(raw: &[u8]) -> String {
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(raw)
}

/// (code_verifier, code_challenge) using S256.
pub fn make_pkce() -> (String, String) {
    use rand::RngCore;
    let mut bytes = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut bytes);
    let verifier = b64url(&bytes);
    let challenge = b64url(&Sha256::digest(verifier.as_bytes()));
    (verifier, challenge)
}

pub fn make_state() -> String {
    use rand::RngCore;
    let mut bytes = [0u8; 16];
    rand::thread_rng().fill_bytes(&mut bytes);
    b64url(&bytes)
}

pub fn build_scope(project: &str) -> String {
    format!(
        "openid profile email offline_access \
         urn:zitadel:iam:org:project:id:{project}:aud \
         urn:zitadel:iam:org:projects:roles"
    )
}

pub fn build_authorize_url(
    authorize_endpoint: &str,
    client_id: &str,
    redirect_uri: &str,
    scope: &str,
    code_challenge: &str,
    state: &str,
) -> String {
    let q = url::form_urlencoded::Serializer::new(String::new())
        .append_pair("client_id", client_id)
        .append_pair("redirect_uri", redirect_uri)
        .append_pair("response_type", "code")
        .append_pair("scope", scope)
        .append_pair("code_challenge", code_challenge)
        .append_pair("code_challenge_method", "S256")
        .append_pair("state", state)
        .append_pair("prompt", "login")
        .finish();
    format!("{authorize_endpoint}?{q}")
}

/// Extract query params from a redirect path (e.g. "/callback?code=..&state=..").
pub fn parse_callback(path: &str) -> HashMap<String, String> {
    let full = format!("http://localhost{path}");
    match url::Url::parse(&full) {
        Ok(u) => {
            let mut out = HashMap::new();
            for (k, v) in u.query_pairs() {
                // first value per key wins (matches Python's parse_qs[..][0])
                out.entry(k.into_owned()).or_insert_with(|| v.into_owned());
            }
            out
        }
        Err(_) => HashMap::new(),
    }
}

#[derive(Debug, Clone)]
pub struct Endpoints {
    pub authorize: String,
    pub token: String,
    pub revoke: String,
}

/// Read endpoints from the OIDC discovery document, with a Zitadel-shaped
/// fallback if discovery is unreachable.
pub fn discover(issuer: &str) -> Endpoints {
    let base = issuer.trim_end_matches('/').to_string();
    let url = format!("{base}/.well-known/openid-configuration");
    let fallback = || Endpoints {
        authorize: format!("{base}/oauth/v2/authorize"),
        token: format!("{base}/oauth/v2/token"),
        revoke: format!("{base}/oauth/v2/revoke"),
    };
    let client = match reqwest::blocking::Client::builder()
        .timeout(REQUEST_TIMEOUT)
        .build()
    {
        Ok(c) => c,
        Err(_) => return fallback(),
    };
    let body: serde_json::Value = match client.get(&url).send().and_then(|r| r.json()) {
        Ok(b) => b,
        Err(e) => {
            tracing::debug!("discovery failed ({e}); using Zitadel default endpoints");
            return fallback();
        }
    };
    let authorize = body.get("authorization_endpoint").and_then(|v| v.as_str());
    let token = body.get("token_endpoint").and_then(|v| v.as_str());
    match (authorize, token) {
        (Some(a), Some(t)) => Endpoints {
            authorize: a.to_string(),
            token: t.to_string(),
            revoke: body
                .get("revocation_endpoint")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
                .unwrap_or_else(|| format!("{base}/oauth/v2/revoke")),
        },
        _ => fallback(),
    }
}

// ---------------- loopback redirect capture ----------------

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
                    let msg = if ok {
                        "Login complete — you can close this tab and return to the terminal."
                            .to_string()
                    } else {
                        format!("Login failed: {}", params.get("error").cloned().unwrap_or_default())
                    };
                    let html = format!("<html><body><h3>{msg}</h3></body></html>");
                    let header = tiny_http::Header::from_bytes(
                        &b"Content-Type"[..],
                        &b"text/html; charset=utf-8"[..],
                    )
                    .expect("static header");
                    let _ = req.respond(tiny_http::Response::from_string(html).with_header(header));
                    return params;
                }
                // Unrelated request (e.g. /favicon.ico) → 404 and keep waiting.
                let _ = req.respond(tiny_http::Response::empty(404));
            }
            Ok(None) => return HashMap::new(), // timed out
            Err(_) => return HashMap::new(),
        }
    }
}

// ---------------- flows ----------------

/// Run the full Auth Code + PKCE login and return a `TokenSet`.
pub fn login(
    issuer: &str,
    client_id: &str,
    project: &str,
    redirect_port: u16,
    open_browser: bool,
    timeout: Duration,
) -> Result<TokenSet> {
    let endpoints = discover(issuer);
    let (verifier, challenge) = make_pkce();
    let state = make_state();
    // RFC 8252: 127.0.0.1 loopback literal (not "localhost", which a hosts-file
    // entry could hijack). The callback server binds 127.0.0.1.
    let redirect_uri = format!("http://127.0.0.1:{redirect_port}/callback");
    let scope = build_scope(project);
    let url = build_authorize_url(
        &endpoints.authorize,
        client_id,
        &redirect_uri,
        &scope,
        &challenge,
        &state,
    );

    println!("Opening browser to sign in:\n  {url}");
    if open_browser {
        if open::that(&url).is_err() {
            println!("(could not open a browser automatically — open the URL above)");
        }
    }

    let params = capture_redirect(redirect_port, timeout);
    if params.is_empty() {
        return Err(Error::Auth(format!(
            "timed out waiting for the login redirect on :{redirect_port}"
        )));
    }
    if let Some(err) = params.get("error") {
        let desc = params.get("error_description").cloned().unwrap_or_default();
        return Err(Error::Auth(format!("login denied/failed: {err} {desc}").trim().to_string()));
    }
    if params.get("state").map(String::as_str) != Some(state.as_str()) {
        return Err(Error::Auth(
            "state mismatch on the OAuth callback (possible CSRF) — aborting".into(),
        ));
    }
    let code = params
        .get("code")
        .filter(|c| !c.is_empty())
        .ok_or_else(|| Error::Auth("no authorization code in the callback".into()))?;

    exchange_code(&endpoints.token, client_id, code, &redirect_uri, &verifier)
}

pub fn exchange_code(
    token_endpoint: &str,
    client_id: &str,
    code: &str,
    redirect_uri: &str,
    code_verifier: &str,
) -> Result<TokenSet> {
    let body = post_token(
        token_endpoint,
        &[
            ("grant_type", "authorization_code"),
            ("client_id", client_id),
            ("code", code),
            ("redirect_uri", redirect_uri),
            ("code_verifier", code_verifier),
        ],
    )?;
    TokenSet::from_response(&body, now_epoch())
}

pub fn refresh(token_endpoint: &str, client_id: &str, refresh_token: &str) -> Result<TokenSet> {
    let body = post_token(
        token_endpoint,
        &[
            ("grant_type", "refresh_token"),
            ("client_id", client_id),
            ("refresh_token", refresh_token),
        ],
    )?;
    let mut ts = TokenSet::from_response(&body, now_epoch())?;
    // Zitadel may not echo a new refresh token; keep the old one if so.
    if ts.refresh_token.is_none() {
        ts.refresh_token = Some(refresh_token.to_string());
    }
    Ok(ts)
}

/// Best-effort refresh-token revocation at logout.
pub fn revoke(revoke_endpoint: &str, client_id: &str, token: &str) {
    let client = match reqwest::blocking::Client::builder()
        .timeout(REQUEST_TIMEOUT)
        .build()
    {
        Ok(c) => c,
        Err(_) => return,
    };
    if let Err(e) = client
        .post(revoke_endpoint)
        .form(&[("client_id", client_id), ("token", token)])
        .send()
    {
        tracing::debug!("revoke failed (ignored): {e}");
    }
}

fn post_token(token_endpoint: &str, form: &[(&str, &str)]) -> Result<serde_json::Value> {
    let client = reqwest::blocking::Client::builder()
        .timeout(REQUEST_TIMEOUT)
        .build()
        .map_err(|e| Error::Auth(format!("http client error: {e}")))?;
    let resp = client.post(token_endpoint).form(form).send().map_err(|e| {
        Error::Auth(format!("could not reach the token endpoint {token_endpoint}: {e}"))
    })?;
    let status = resp.status();
    let text = resp.text().unwrap_or_default();
    if status.as_u16() != 200 {
        let snippet: String = text.chars().take(300).collect();
        return Err(Error::Auth(format!(
            "token endpoint returned {}: {snippet}",
            status.as_u16()
        )));
    }
    serde_json::from_str(&text).map_err(|_| {
        let snippet: String = text.chars().take(200).collect();
        Error::Protocol(format!("token response was not JSON: {snippet}"))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pkce_is_s256_and_unpadded() {
        let (verifier, challenge) = make_pkce();
        assert!(!verifier.contains('='));
        assert!(!challenge.contains('='));
        // challenge == b64url(sha256(verifier))
        let expected = b64url(&Sha256::digest(verifier.as_bytes()));
        assert_eq!(challenge, expected);
    }

    #[test]
    fn state_is_random() {
        assert_ne!(make_state(), make_state());
    }

    #[test]
    fn build_scope_has_project_offline_and_openid() {
        let s = build_scope("PROJ123");
        assert!(s.contains("openid"));
        assert!(s.contains("offline_access"));
        assert!(s.contains("PROJ123:aud"));
        assert!(s.contains("projects:roles"));
    }

    #[test]
    fn authorize_url_has_pkce_state_and_code_response() {
        let u = build_authorize_url("https://i/auth", "cid", "http://localhost:8477/callback", "openid", "chal", "st");
        assert!(u.contains("code_challenge=chal"));
        assert!(u.contains("code_challenge_method=S256"));
        assert!(u.contains("state=st"));
        assert!(u.contains("response_type=code"));
    }

    #[test]
    fn parse_callback_extracts_code_and_state() {
        let p = parse_callback("/callback?code=abc&state=xyz");
        assert_eq!(p.get("code").unwrap(), "abc");
        assert_eq!(p.get("state").unwrap(), "xyz");
    }
}
