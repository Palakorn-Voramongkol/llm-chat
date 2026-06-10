//! Hand-rolled OIDC Authorization Code + PKCE for the operator login, mirroring
//! clients/python/llm_chat/oidc.py (design §4.2). Not the openidconnect crate:
//! that rejects the plain-HTTP dev issuer (appendix §6.7). The callback JWT is
//! verified by the SHARED zitadel_auth::JwksCache and gated on `chat.admin`.
//! Handler success path (real code exchange + chat.admin in the access-token JWT
//! for a HUMAN login) is verified by Task 23's gated smoke — appendix §6.1.

use axum::{
    extract::{Query, State},
    http::{header, StatusCode},
    response::{IntoResponse, Redirect, Response},
};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use tower_sessions::Session;

use crate::config::AdminConfig;
use crate::session::Operator;
use crate::AppState;

// ---------------- pure helpers (unit-tested, no network) ----------------

fn b64url(raw: &[u8]) -> String {
    URL_SAFE_NO_PAD.encode(raw)
}

/// Deterministic (verifier, challenge) using S256: challenge = b64url(sha256(verifier)).
/// `seed` is fresh per-login (random) at the call site; deterministic here so it
/// is unit-testable. Mirrors oidc.py::make_pkce but seedable.
pub fn pkce_pair(seed: &str) -> (String, String) {
    let verifier = b64url(Sha256::digest(format!("verifier:{seed}").as_bytes()).as_slice());
    let challenge = b64url(Sha256::digest(verifier.as_bytes()).as_slice());
    (verifier, challenge)
}

/// Build the /oauth/v2/authorize URL with PKCE + WEB-app scopes (appendix §1.2/§1.3).
/// redirect_uri uses cfg.public_origin (the admin-api's own origin).
pub fn build_authorize_url(cfg: &AdminConfig, challenge: &str, state: &str, nonce: &str) -> String {
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

// ---------------- handlers (network; verified by Task 19/23) ----------------

pub async fn login(State(st): State<AppState>, session: Session) -> Response {
    let seed = b64url(&rand::random::<[u8; 16]>());
    let (verifier, challenge) = pkce_pair(&seed);
    let state = b64url(&rand::random::<[u8; 16]>());
    // nonce is sent per OIDC convention; replay protection via the id_token nonce
    // is not enforced because authz uses the access-token JWKS path (CSRF is
    // enforced via `state`).
    let nonce = b64url(&rand::random::<[u8; 16]>());
    // pre-auth session: persist verifier+state+nonce across /login -> /callback (§1.6)
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

    // Exchange code + verifier, authenticating with client_id:client_secret via
    // HTTP Basic (§1.4). Then verify the JWT via the SHARED JwksCache and require
    // chat.admin (§1.5) — zero new parsing logic.
    let token = match exchange_code(&st, &code, &verifier).await {
        Ok(t) => t,
        Err(e) => return (StatusCode::BAD_GATEWAY, e).into_response(),
    };
    let principal = match st.jwks.verify_sync(&token) {
        Ok(p) => p,
        Err(e) => return (StatusCode::UNAUTHORIZED, e.to_string()).into_response(),
    };
    if !principal.has("chat.admin") {
        return (StatusCode::FORBIDDEN, "not a chat.admin operator").into_response();
    }

    // Resolve a human display name via standard OIDC userinfo (verified live:
    // returns {name, preferred_username, email?}). DISPLAY-ONLY and best-effort:
    // identity + authorization above stay on the VERIFIED JWT; any userinfo
    // failure just falls back to email/user_id.
    let display_name = fetch_display_name(&st, &token).await;
    let op = Operator {
        user_id: principal.user_id.clone(),
        name: display_name
            .or_else(|| principal.email.clone())
            .unwrap_or_else(|| principal.user_id.clone()),
        roles: principal.roles.clone(),
    };
    let _ = session.remove::<String>("pkce_verifier").await;
    if session.insert("operator", &op).await.is_err() {
        return (StatusCode::INTERNAL_SERVER_ERROR, "session write failed").into_response();
    }
    // Stamp the login time so the Operator extractor can enforce an ABSOLUTE
    // max session lifetime (independent of idle expiry). A session with no
    // login_at is treated as expired — fail closed.
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let _ = session.insert("login_at", now).await;
    // 302 to the web origin (the browser talks to admin-web :3000).
    Redirect::to(&format!("{}/", st.cfg.allowed_origin)).into_response()
}

pub async fn logout(State(st): State<AppState>, session: Session) -> Response {
    let _ = session.delete().await;
    // best-effort end_session (appendix §1.6); cookie/session already cleared.
    let url = format!("{}/oidc/v1/end_session?post_logout_redirect_uri={}/",
        st.cfg.issuer, st.cfg.allowed_origin);
    Redirect::to(&url).into_response()
}

/// Best-effort OIDC userinfo lookup for a DISPLAY name (never authz). Standard
/// endpoint: GET {issuer}/oidc/v1/userinfo with the access token; prefers
/// `name`, then `preferred_username`. Any failure → None (caller falls back).
async fn fetch_display_name(st: &AppState, access_token: &str) -> Option<String> {
    let url = format!("{}/oidc/v1/userinfo", st.cfg.issuer);
    let v: serde_json::Value = st
        .http
        .get(&url)
        .bearer_auth(access_token)
        .send()
        .await
        .ok()?
        .json()
        .await
        .ok()?;
    v.get("name")
        .and_then(|x| x.as_str())
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
    // HTTP Basic per §1.4: base64(urlencode(id):urlencode(secret)).
    let enc = |s: &str| url::form_urlencoded::byte_serialize(s.as_bytes()).collect::<String>();
    base64::engine::general_purpose::STANDARD.encode(format!("{}:{}", enc(id), enc(secret)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
    use sha2::{Digest, Sha256};
    use crate::config::AdminConfig;

    fn test_cfg() -> AdminConfig {
        AdminConfig {
            issuer: "http://h:8080".into(),
            project_id: "p1".into(),
            audience: "p1".into(),
            sa_key_path: "/x".into(),
            oidc_client_id: "c1".into(),
            oidc_client_secret: "s1".into(),
            bind_addr: "0.0.0.0:7676".into(),
            public_origin: "http://localhost:7676".into(),
            allowed_origin: "http://localhost:3000".into(),
            session_key: "k".into(),
            cookie_secure: true,
            manager_control_url: None,
        }
    }

    #[test]
    fn pkce_pair_challenge_is_s256_of_verifier() {
        let (verifier, challenge) = pkce_pair("seed-abc");
        let want = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
        assert_eq!(challenge, want);
        assert!(!verifier.contains('=') && !verifier.contains('+') && !verifier.contains('/'));
    }

    #[test]
    fn pkce_pair_is_deterministic_per_seed() {
        assert_eq!(pkce_pair("s1"), pkce_pair("s1"));
        assert_ne!(pkce_pair("s1").0, pkce_pair("s2").0);
    }

    #[test]
    fn build_authorize_url_carries_pkce_state_nonce_and_scopes() {
        let cfg = test_cfg();
        let url = build_authorize_url(&cfg, "CHAL", "STATE", "NONCE");
        assert!(url.starts_with("http://h:8080/oauth/v2/authorize?"));
        assert!(url.contains("client_id=c1"));
        assert!(url.contains("response_type=code"));
        assert!(url.contains("code_challenge=CHAL"));
        assert!(url.contains("code_challenge_method=S256"));
        assert!(url.contains("state=STATE"));
        assert!(url.contains("nonce=NONCE"));
        // redirect_uri uses the admin-api public origin (:7676), URL-encoded
        assert!(url.contains("redirect_uri=http%3A%2F%2Flocalhost%3A7676%2Fcallback"));
        // project-audience + roles scopes (URL-encoded ':' = %3A)
        assert!(url.contains("urn%3Azitadel%3Aiam%3Aorg%3Aproject%3Aid%3Ap1%3Aaud"));
        assert!(url.contains("urn%3Azitadel%3Aiam%3Aorg%3Aprojects%3Aroles"));
    }
}
