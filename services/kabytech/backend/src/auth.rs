//! Hand-rolled OIDC Authorization Code + PKCE for the end-user login, ported
//! from admin-api/src/auth.rs (gate changed to chat.user in Task 4). Not the
//! openidconnect crate: it rejects the plain-HTTP dev issuer. The callback JWT
//! is verified by the SHARED zitadel_auth::JwksCache and gated on `chat.user`.

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use sha2::{Digest, Sha256};

use crate::config::KabyConfig;

fn b64url(raw: &[u8]) -> String {
    URL_SAFE_NO_PAD.encode(raw)
}

/// Deterministic (verifier, challenge) with S256. `seed` is random per-login at
/// the call site; deterministic here so it is unit-testable.
pub fn pkce_pair(seed: &str) -> (String, String) {
    let verifier = b64url(Sha256::digest(format!("verifier:{seed}").as_bytes()).as_slice());
    let challenge = b64url(Sha256::digest(verifier.as_bytes()).as_slice());
    (verifier, challenge)
}

/// Build the /oauth/v2/authorize URL with PKCE + the project-aud + roles scopes.
/// redirect_uri uses cfg.public_origin (the FRONTEND origin) + /callback.
pub fn build_authorize_url(cfg: &KabyConfig, challenge: &str, state: &str, nonce: &str) -> String {
    let scope = format!(
        "openid profile email offline_access \
         urn:zitadel:iam:org:project:id:{}:aud \
         urn:zitadel:iam:org:projects:roles",
        cfg.project_id
    );
    let redirect_uri = format!("{}/callback", cfg.public_origin);
    let q = url::form_urlencoded::Serializer::new(String::new())
        .append_pair("client_id", &cfg.oidc_client_id)
        .append_pair("redirect_uri", &redirect_uri)
        .append_pair("response_type", "code")
        .append_pair("scope", &scope)
        .append_pair("code_challenge", challenge)
        .append_pair("code_challenge_method", "S256")
        .append_pair("state", state)
        .append_pair("nonce", nonce)
        .append_pair("prompt", "login")
        .finish();
    format!("{}/oauth/v2/authorize?{}", cfg.issuer, q)
}

// ---------------- handlers (network; verified by the Task 6 gated smoke) ----------------

use axum::{
    extract::{Query, State},
    http::{header, StatusCode},
    response::{IntoResponse, Json, Redirect, Response},
};
use serde::Deserialize;
use tower_sessions::Session;

use crate::session::EndUser;
use crate::AppState;

pub async fn login_start(State(st): State<AppState>, session: Session) -> Response {
    let seed = b64url(&rand::random::<[u8; 16]>());
    let (verifier, challenge) = pkce_pair(&seed);
    let state = b64url(&rand::random::<[u8; 16]>());
    let nonce = b64url(&rand::random::<[u8; 16]>());
    let _ = session.insert("pkce_verifier", &verifier).await;
    let _ = session.insert("oauth_state", &state).await;
    let _ = session.insert("oidc_nonce", &nonce).await;
    Redirect::to(&build_authorize_url(&st.cfg, &challenge, &state, &nonce)).into_response()
}

#[derive(Deserialize)]
pub struct CallbackQuery {
    code: Option<String>,
    state: Option<String>,
    error: Option<String>,
}

pub async fn callback(
    State(st): State<AppState>,
    session: Session,
    Query(q): Query<CallbackQuery>,
) -> Response {
    if let Some(err) = q.error {
        return (StatusCode::FORBIDDEN, format!("login failed: {err}")).into_response();
    }
    let want_state: Option<String> = session.get("oauth_state").await.ok().flatten();
    if want_state.is_none() || q.state != want_state {
        return (StatusCode::FORBIDDEN, "state mismatch (possible CSRF)").into_response();
    }
    let verifier: String = match session.get("pkce_verifier").await.ok().flatten() {
        Some(v) => v,
        None => return (StatusCode::BAD_REQUEST, "no PKCE verifier in session").into_response(),
    };
    let code = match q.code {
        Some(c) => c,
        None => return (StatusCode::BAD_REQUEST, "no authorization code").into_response(),
    };
    let token = match exchange_code(&st, &code, &verifier).await {
        Ok(t) => t,
        Err(e) => return (StatusCode::BAD_GATEWAY, e).into_response(),
    };
    let principal = match st.jwks.verify_sync(&token) {
        Ok(p) => p,
        Err(e) => return (StatusCode::UNAUTHORIZED, e.to_string()).into_response(),
    };
    if !principal.has("chat.user") {
        return (StatusCode::FORBIDDEN, "not a chat.user").into_response();
    }
    let display_name = fetch_display_name(&st, &token).await;
    let user = EndUser {
        user_id: principal.user_id.clone(),
        name: display_name
            .or_else(|| principal.email.clone())
            .unwrap_or_else(|| principal.user_id.clone()),
        roles: principal.roles.clone(),
    };
    let _ = session.remove::<String>("pkce_verifier").await;
    // Fixation defense across the unauth -> authed elevation.
    let _ = session.cycle_id().await;
    if session.insert("end_user", &user).await.is_err() {
        return (StatusCode::INTERNAL_SERVER_ERROR, "session write failed").into_response();
    }
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let _ = session.insert("login_at", now).await;
    // Best-effort: materialize this user's app sandbox on first login. NEVER
    // blocks login — log and continue regardless (idempotent: re-login retries).
    if let Some(url) = st.cfg.manager_provision_url.clone() {
        let app = st.cfg.app_code.clone();
        let tok = token.clone();
        tokio::spawn(async move {
            match crate::provision::provision_app_box(&url, &tok, &app).await {
                Ok(()) => tracing::info!(target: "kaby::provision", app = %app, "provisioned user sandbox"),
                Err(e) => tracing::warn!(target: "kaby::provision", error = %e, "first-login provision failed (ignored)"),
            }
        });
    }
    Redirect::to(&format!("{}/", st.cfg.allowed_origin)).into_response()
}

pub async fn logout(State(st): State<AppState>, session: Session) -> Response {
    let _ = session.delete().await;
    let url = format!(
        "{}/oidc/v1/end_session?post_logout_redirect_uri={}/",
        st.cfg.issuer, st.cfg.allowed_origin
    );
    Redirect::to(&url).into_response()
}

/// Authenticated identity for the frontend. 401 when no valid chat.user session
/// (the EndUser extractor enforces the gate + absolute-lifetime).
pub async fn api_me(user: EndUser) -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "userId": user.user_id, "name": user.name, "roles": user.roles
    }))
}

async fn fetch_display_name(st: &AppState, access_token: &str) -> Option<String> {
    let url = format!("{}/oidc/v1/userinfo", st.cfg.issuer);
    let v: serde_json::Value = st.http.get(&url).bearer_auth(access_token)
        .send().await.ok()?.json().await.ok()?;
    v.get("name").and_then(|x| x.as_str())
        .or_else(|| v.get("preferred_username").and_then(|x| x.as_str()))
        .map(|s| s.to_string())
}

async fn exchange_code(st: &AppState, code: &str, verifier: &str) -> Result<String, String> {
    let redirect_uri = format!("{}/callback", st.cfg.public_origin);
    let basic = b64_basic(&st.cfg.oidc_client_id, &st.cfg.oidc_client_secret);
    let resp = st.http
        .post(format!("{}/oauth/v2/token", st.cfg.issuer))
        .header(header::AUTHORIZATION, format!("Basic {basic}"))
        .form(&[
            ("grant_type", "authorization_code"),
            ("code", code),
            ("redirect_uri", &redirect_uri),
            ("code_verifier", verifier),
        ])
        .send().await.map_err(|e| format!("token endpoint unreachable: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("token endpoint returned {}", resp.status()));
    }
    let body: serde_json::Value = resp.json().await.map_err(|e| format!("token not JSON: {e}"))?;
    body.get("access_token").and_then(|v| v.as_str()).map(String::from)
        .ok_or_else(|| "no access_token in token response".into())
}

fn b64_basic(id: &str, secret: &str) -> String {
    let enc = |s: &str| url::form_urlencoded::byte_serialize(s.as_bytes()).collect::<String>();
    base64::engine::general_purpose::STANDARD.encode(format!("{}:{}", enc(id), enc(secret)))
}

// ---------------- invitation + registration (identity UX Phase 1) ----------------

use crate::session::Operator;

#[derive(Deserialize)]
pub struct InviteReq {
    pub email: String,
    pub given: Option<String>,
    pub family: Option<String>,
}

/// chat.admin only (the Operator extractor enforces it). Creates the invited
/// user (Zitadel emails the link) + grants chat.user.
pub async fn api_invite(_op: Operator, State(st): State<AppState>, Json(req): Json<InviteReq>) -> Response {
    let email = req.email.trim();
    if email.is_empty() || !email.contains('@') {
        return (StatusCode::BAD_REQUEST, "a valid email is required").into_response();
    }
    let token = match st.zitadel.mint_token().await {
        Ok(t) => t,
        Err(e) => return (StatusCode::BAD_GATEWAY, e).into_response(),
    };
    // Zitadel requires non-empty given/family (1–200 runes). When the operator
    // leaves them blank, fall back to the email local-part (the user can edit
    // their profile later).
    let local = email.split('@').next().filter(|s| !s.is_empty()).unwrap_or("user");
    let given = req.given.as_deref().map(str::trim).filter(|s| !s.is_empty()).unwrap_or(local);
    let family = req.family.as_deref().map(str::trim).filter(|s| !s.is_empty()).unwrap_or(local);
    let uid = match st
        .zitadel
        .create_invited_user(&token, email, given, family, &st.cfg.public_origin)
        .await
    {
        Ok(u) => u,
        Err(e) => return (StatusCode::CONFLICT, e).into_response(),
    };
    if let Err(e) = st.zitadel.grant_chat_user(&token, &uid).await {
        return (StatusCode::BAD_GATEWAY, e).into_response();
    }
    Json(serde_json::json!({ "ok": true, "userId": uid, "email": email })).into_response()
}

#[derive(Deserialize)]
pub struct AcceptReq {
    pub user_id: String,
    pub code: String,
    pub password: String,
}

/// Unauthenticated by necessity, but fail-closed: the emailed code MUST verify
/// (proves email ownership) before any password is set.
pub async fn api_accept(State(st): State<AppState>, Json(req): Json<AcceptReq>) -> Response {
    if req.password.len() < 8 {
        return (StatusCode::BAD_REQUEST, "password must be at least 8 characters").into_response();
    }
    let token = match st.zitadel.mint_token().await {
        Ok(t) => t,
        Err(e) => return (StatusCode::BAD_GATEWAY, e).into_response(),
    };
    if let Err(e) = st.zitadel.verify_email(&token, &req.user_id, &req.code).await {
        return (StatusCode::FORBIDDEN, e).into_response(); // bad/expired code → reject
    }
    if let Err(e) = st.zitadel.set_password(&token, &req.user_id, &req.password).await {
        return (StatusCode::BAD_GATEWAY, e).into_response();
    }
    Json(serde_json::json!({ "ok": true })).into_response()
}

// ---------------- custom login (identity UX Phase 2 — Session API) ----------------

#[derive(Deserialize)]
pub struct LoginReq {
    pub auth_request: String,
    pub login_name: String,
    pub password: String,
}

/// Custom-login: create a Zitadel session (password check), finalize the OIDC
/// auth request, and return the callback URL the browser follows to finish the
/// code flow (-> /callback -> token exchange -> chat.user gate -> cookie).
pub async fn api_login(State(st): State<AppState>, Json(req): Json<LoginReq>) -> Response {
    let token = match st.zitadel.mint_token().await {
        Ok(t) => t,
        Err(e) => return (StatusCode::BAD_GATEWAY, e).into_response(),
    };
    let (sid, stok) = match st.zitadel.create_session(&token, req.login_name.trim(), &req.password).await {
        Ok(s) => s,
        Err(_) => return (StatusCode::UNAUTHORIZED, "invalid credentials").into_response(),
    };
    match st.zitadel.finalize_auth_request(&token, &req.auth_request, &sid, &stok).await {
        Ok(callback) => Json(serde_json::json!({ "callbackUrl": callback })).into_response(),
        Err(e) => (StatusCode::BAD_GATEWAY, e).into_response(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::KabyConfig;
    use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
    use sha2::{Digest, Sha256};

    fn cfg() -> KabyConfig {
        KabyConfig {
            issuer: "http://h:8080".into(),
            project_id: "p1".into(),
            audience: "p1".into(),
            oidc_client_id: "c1".into(),
            oidc_client_secret: "s1".into(),
            bind_addr: "0.0.0.0:7670".into(),
            public_origin: "http://localhost:3001".into(),
            allowed_origin: "http://localhost:3001".into(),
            session_key: "k".into(),
            sa_key_path: "/x".into(),
            cookie_secure: true,
            manager_provision_url: None,
            app_code: "kabytech".into(),
        }
    }

    #[test]
    fn pkce_challenge_is_s256_of_verifier_and_url_safe() {
        let (verifier, challenge) = pkce_pair("seed-abc");
        assert_eq!(challenge, URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes())));
        assert!(!verifier.contains('=') && !verifier.contains('+') && !verifier.contains('/'));
    }

    #[test]
    fn pkce_is_deterministic_per_seed() {
        assert_eq!(pkce_pair("s1"), pkce_pair("s1"));
        assert_ne!(pkce_pair("s1").0, pkce_pair("s2").0);
    }

    #[test]
    fn authorize_url_carries_pkce_state_nonce_scopes_and_frontend_redirect() {
        let url = build_authorize_url(&cfg(), "CHAL", "STATE", "NONCE");
        assert!(url.starts_with("http://h:8080/oauth/v2/authorize?"));
        assert!(url.contains("client_id=c1"));
        assert!(url.contains("response_type=code"));
        assert!(url.contains("code_challenge=CHAL"));
        assert!(url.contains("code_challenge_method=S256"));
        assert!(url.contains("state=STATE"));
        assert!(url.contains("nonce=NONCE"));
        // redirect_uri is the FRONTEND origin (:3001), URL-encoded
        assert!(url.contains("redirect_uri=http%3A%2F%2Flocalhost%3A3001%2Fcallback"));
        assert!(url.contains("urn%3Azitadel%3Aiam%3Aorg%3Aproject%3Aid%3Ap1%3Aaud"));
        assert!(url.contains("urn%3Azitadel%3Aiam%3Aorg%3Aprojects%3Aroles"));
    }
}
