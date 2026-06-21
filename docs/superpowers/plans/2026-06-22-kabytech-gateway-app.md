# kabytech Gateway App — Auth/Login MVP Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A standalone end-user login app — `services/kabytech/backend` (Rust/axum OIDC Relying Party gating `chat.user`) + `services/kabytech/frontend` (Next.js 16 + Tailwind v4) — that completes a browser login round-trip against Zitadel and establishes an authenticated session.

**Architecture:** Port the proven `admin-api` ↔ `admin-web` pattern: the backend owns the OIDC Auth Code + PKCE flow, the client secret, and a signed-cookie session; the frontend is UI + a same-origin proxy. The browser only ever holds an opaque session cookie. Gate on `chat.user` (vs admin-api's `chat.admin`).

**Tech Stack:** Rust (axum 0.8, tower-sessions 0.15, the workspace `zitadel-auth` crate), Next.js 16.2.7 + React 19 + Tailwind v4, Docker Compose.

## Global Constraints

- **Gate strictly on `chat.user`**, fail closed (403) — never a weaker fallback.
- **Config is fail-fast**: every required var resolved at startup via a `require_var`-style helper that names the first missing one; no silent defaults. `cookie_secure` is secure-by-default (only `false`/`0`/`no` disables it).
- **Same-origin-proxy redirect rule:** `KABY_PUBLIC_ORIGIN` is the **frontend** origin (`http://localhost:3001`), so the OIDC `redirect_uri = {KABY_PUBLIC_ORIGIN}/callback = http://localhost:3001/callback`. The browser must land on the proxied frontend origin or the `SameSite=Lax` cookie is dropped. (This corrects the design doc, which loosely said the backend origin.)
- **Identity + authz ride the verified JWT** (`zitadel_auth::JwksCache::verify_sync` → `Principal`), never client input. The client secret never leaves the backend.
- **Ports:** backend `127.0.0.1:7670`, frontend `127.0.0.1:3001` — loopback-only in compose, like every other service.
- **Workspace:** `services/kabytech/backend` is a Cargo workspace member; its compose Dockerfile builds it in the full workspace (copy every member's manifest+src), mirroring `admin-api.Dockerfile`.
- **Scopes requested at login:** `openid profile email offline_access`, `urn:zitadel:iam:org:project:id:{project_id}:aud`, `urn:zitadel:iam:org:projects:roles`.
- **MVP only:** no chat forwarding, no `/chat` WS, no token-refresh loop, no upstream-IdP federation in app code.

---

### Task 1: Backend crate skeleton + fail-fast config

Create the `services/kabytech/backend` crate as a workspace member with `KabyConfig`, ported from `admin-api/src/config.rs` (trimmed: no `sa_key_path`, no `manager_control_url`; OIDC vars renamed `KABY_*`).

**Files:**
- Modify: `Cargo.toml` (root) — add `"services/kabytech/backend"` to `members`
- Create: `services/kabytech/backend/Cargo.toml`
- Create: `services/kabytech/backend/src/lib.rs`
- Create: `services/kabytech/backend/src/config.rs`
- Create: `services/kabytech/backend/src/main.rs` (minimal stub this task; fleshed out in Task 4)

**Interfaces:**
- Produces: `kabytech_backend::config::{require_var, parse_cookie_secure, KabyConfig}`. `KabyConfig` fields: `issuer, project_id, audience, oidc_client_id, oidc_client_secret, bind_addr, public_origin, allowed_origin, session_key, cookie_secure: bool`. `KabyConfig::from_map(get: &dyn Fn(&str)->Option<String>) -> Result<KabyConfig, String>` and `from_env()`.

- [ ] **Step 1: Add the crate to the workspace**

In root `Cargo.toml`, change the members line to:

```toml
members = ["manager", "worker", "crates/zitadel-auth", "admin-api", "clients/rust", "services/kabytech/backend"]
```

- [ ] **Step 2: Create `services/kabytech/backend/Cargo.toml`**

```toml
[package]
name = "kabytech-backend"
version = "0.1.0"
edition = "2021"

[lib]
name = "kabytech_backend"
path = "src/lib.rs"

[[bin]]
name = "kabytech-backend"
path = "src/main.rs"

[dependencies]
tokio = { version = "1", features = ["rt-multi-thread", "macros", "net", "sync"] }
serde = { workspace = true }
serde_json = { workspace = true }
reqwest = { workspace = true }
tracing = { workspace = true }
zitadel-auth = { path = "../../../crates/zitadel-auth" }
axum = "0.8"
tower = "0.5"
tower-http = { version = "0.6", features = ["trace", "set-header"] }
tower-sessions = "0.15"
tracing-subscriber = { version = "0.3", features = ["env-filter"] }
base64 = "0.22"
sha2 = "0.10"
rand = "0.8"
url = "2"
time = "0.3"
```

- [ ] **Step 3: Write the failing config tests**

Create `services/kabytech/backend/src/config.rs` with ONLY this test module first (the impl in Step 5 makes it pass):

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn getter(m: HashMap<&'static str, &'static str>) -> impl Fn(&str) -> Option<String> {
        move |k: &str| m.get(k).map(|s| s.to_string())
    }

    fn full_map() -> HashMap<&'static str, &'static str> {
        HashMap::from([
            ("ZITADEL_ISSUER", "http://host.docker.internal:8080/"),
            ("ZITADEL_PROJECT_ID", "p1"),
            ("ZITADEL_AUDIENCE", "p1"),
            ("KABY_OIDC_CLIENT_ID", "cid"),
            ("KABY_OIDC_CLIENT_SECRET", "csecret"),
            ("KABY_BIND_ADDR", "0.0.0.0:7670"),
            ("KABY_PUBLIC_ORIGIN", "http://localhost:3001/"),
            ("KABY_ALLOWED_ORIGIN", "http://localhost:3001"),
            ("KABY_SESSION_KEY", "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"),
        ])
    }

    #[test]
    fn require_var_trims_and_rejects_empty() {
        assert_eq!(require_var("X", Some("  v  ".into())), Ok("v".into()));
        assert_eq!(require_var("X", None), Err("X must be set (no default)".into()));
        assert_eq!(require_var("X", Some("  ".into())), Err("X must be set (no default)".into()));
    }

    #[test]
    fn cookie_secure_defaults_true_only_falsey_disables() {
        assert!(parse_cookie_secure(None));
        assert!(parse_cookie_secure(Some("anything".into())));
        assert!(!parse_cookie_secure(Some("false".into())));
        assert!(!parse_cookie_secure(Some("0".into())));
        assert!(!parse_cookie_secure(Some(" NO ".into())));
    }

    #[test]
    fn from_map_ok_trims_issuer_and_public_origin() {
        let cfg = KabyConfig::from_map(&getter(full_map())).expect("ok");
        assert_eq!(cfg.issuer, "http://host.docker.internal:8080");
        assert_eq!(cfg.public_origin, "http://localhost:3001");
        assert_eq!(cfg.project_id, "p1");
        assert_eq!(cfg.bind_addr, "0.0.0.0:7670");
    }

    #[test]
    fn from_map_names_first_missing_var() {
        let mut m = full_map();
        m.remove("KABY_OIDC_CLIENT_SECRET");
        assert_eq!(
            KabyConfig::from_map(&getter(m)),
            Err("KABY_OIDC_CLIENT_SECRET must be set (no default)".into())
        );
    }
}
```

- [ ] **Step 4: Run to verify it fails**

Run: `cargo test -p kabytech-backend`
Expected: FAIL to compile (`require_var`, `KabyConfig` not found).

- [ ] **Step 5: Write the config impl (prepend above the test module)**

```rust
//! Startup configuration for kabytech-backend — env-driven, validated fail-fast.
//! Ported from admin-api/src/config.rs (trimmed for the gateway login MVP).

/// PURE: require a non-empty config var (trims surrounding whitespace).
pub fn require_var(name: &str, raw: Option<String>) -> Result<String, String> {
    match raw.map(|s| s.trim().to_string()).filter(|s| !s.is_empty()) {
        Some(v) => Ok(v),
        None => Err(format!("{name} must be set (no default)")),
    }
}

/// PURE: parse the session-cookie `Secure` toggle. Secure BY DEFAULT; only an
/// explicit false/0/no (a plain-HTTP dev stack) disables it. Fail closed.
pub fn parse_cookie_secure(raw: Option<String>) -> bool {
    !matches!(
        raw.map(|s| s.trim().to_ascii_lowercase()).as_deref(),
        Some("false") | Some("0") | Some("no")
    )
}

/// Resolved, validated kabytech-backend config. Every field required.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KabyConfig {
    pub issuer: String,
    pub project_id: String,
    pub audience: String,
    pub oidc_client_id: String,
    pub oidc_client_secret: String,
    pub bind_addr: String,
    /// The FRONTEND origin (e.g. http://localhost:3001). The OIDC redirect_uri is
    /// {public_origin}/callback so the browser lands on the proxied frontend and
    /// keeps the SameSite=Lax cookie.
    pub public_origin: String,
    pub allowed_origin: String,
    pub session_key: String,
    pub cookie_secure: bool,
}

impl KabyConfig {
    pub fn from_map(get: &dyn Fn(&str) -> Option<String>) -> Result<KabyConfig, String> {
        let issuer = require_var("ZITADEL_ISSUER", get("ZITADEL_ISSUER"))?
            .trim_end_matches('/')
            .to_string();
        let public_origin = require_var("KABY_PUBLIC_ORIGIN", get("KABY_PUBLIC_ORIGIN"))?
            .trim_end_matches('/')
            .to_string();
        Ok(KabyConfig {
            issuer,
            project_id: require_var("ZITADEL_PROJECT_ID", get("ZITADEL_PROJECT_ID"))?,
            audience: require_var("ZITADEL_AUDIENCE", get("ZITADEL_AUDIENCE"))?,
            oidc_client_id: require_var("KABY_OIDC_CLIENT_ID", get("KABY_OIDC_CLIENT_ID"))?,
            oidc_client_secret: require_var("KABY_OIDC_CLIENT_SECRET", get("KABY_OIDC_CLIENT_SECRET"))?,
            bind_addr: require_var("KABY_BIND_ADDR", get("KABY_BIND_ADDR"))?,
            public_origin,
            allowed_origin: require_var("KABY_ALLOWED_ORIGIN", get("KABY_ALLOWED_ORIGIN"))?,
            session_key: require_var("KABY_SESSION_KEY", get("KABY_SESSION_KEY"))?,
            cookie_secure: parse_cookie_secure(get("KABY_COOKIE_SECURE")),
        })
    }

    pub fn from_env() -> Result<KabyConfig, String> {
        Self::from_map(&|k| std::env::var(k).ok())
    }
}
```

- [ ] **Step 6: Create `services/kabytech/backend/src/lib.rs`**

```rust
//! kabytech-backend — end-user login gateway (OIDC Relying Party). The browser
//! holds only an opaque session cookie; this backend owns the OIDC flow + secret.
pub mod auth;
pub mod config;
pub mod session;

use std::sync::Arc;
use zitadel_auth::JwksCache;

/// Shared application state (cheap to clone).
#[derive(Clone)]
pub struct AppState {
    pub cfg: config::KabyConfig,
    pub jwks: JwksCache,
    pub http: Arc<reqwest::Client>,
}
```

- [ ] **Step 7: Create stub `auth.rs`, `session.rs`, `main.rs` so the crate compiles**

`services/kabytech/backend/src/auth.rs`: `// filled in Task 2/4`
`services/kabytech/backend/src/session.rs`: `// filled in Task 3`
`services/kabytech/backend/src/main.rs`:

```rust
fn main() {
    eprintln!("kabytech-backend: not yet wired (Task 4)");
}
```

(Empty `auth.rs`/`session.rs` are valid modules. `lib.rs` references them — keep these files present from this task so `cargo build` succeeds.)

- [ ] **Step 8: Run to verify pass**

Run: `cargo test -p kabytech-backend`
Expected: PASS (config tests green; crate compiles).

- [ ] **Step 9: Commit**

```bash
git add Cargo.toml Cargo.lock services/kabytech/backend
git commit -m "feat(kabytech-backend): crate skeleton + fail-fast KabyConfig"
```

---

### Task 2: OIDC pure helpers (PKCE + authorize URL)

Port the pure, unit-testable OIDC helpers from `admin-api/src/auth.rs`, requesting the same scopes.

**Files:**
- Modify: `services/kabytech/backend/src/auth.rs`

**Interfaces:**
- Consumes: `crate::config::KabyConfig`.
- Produces: `auth::pkce_pair(seed: &str) -> (String, String)`, `auth::build_authorize_url(cfg: &KabyConfig, challenge: &str, state: &str, nonce: &str) -> String`.

- [ ] **Step 1: Write the failing tests**

Replace `services/kabytech/backend/src/auth.rs` with the test module first:

```rust
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
            cookie_secure: true,
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
```

- [ ] **Step 2: Run to verify fail**

Run: `cargo test -p kabytech-backend pkce`
Expected: FAIL (`pkce_pair`/`build_authorize_url` not found).

- [ ] **Step 3: Prepend the implementation**

```rust
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
```

- [ ] **Step 4: Run to verify pass**

Run: `cargo test -p kabytech-backend`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add services/kabytech/backend/src/auth.rs
git commit -m "feat(kabytech-backend): PKCE + authorize-URL OIDC helpers"
```

---

### Task 3: Session model + fail-closed `EndUser` extractor

Port `admin-api/src/session.rs`, gating on `chat.user` (not `chat.admin`).

**Files:**
- Modify: `services/kabytech/backend/src/session.rs`

**Interfaces:**
- Produces: `session::EndUser { user_id: String, name: String, roles: Vec<String> }` with `has(&self, role) -> bool` and an axum `FromRequestParts` impl that returns `Ok(EndUser)` only when the session has an `EndUser` with `chat.user` and a fresh `login_at`; else 401/403.

- [ ] **Step 1: Write the failing test**

Put this test module in `services/kabytech/backend/src/session.rs` (impl follows in Step 3):

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn has_checks_membership() {
        let u = EndUser { user_id: "u1".into(), name: "U".into(), roles: vec!["chat.user".into()] };
        assert!(u.has("chat.user"));
        assert!(!u.has("chat.admin"));
    }
}
```

- [ ] **Step 2: Run to verify fail**

Run: `cargo test -p kabytech-backend has_checks`
Expected: FAIL (`EndUser` not found).

- [ ] **Step 3: Prepend the implementation**

```rust
//! The end-user session model + a fail-closed axum extractor: loads the
//! tower-sessions session and REJECTS unless roles contains "chat.user".
//! Ported from admin-api/src/session.rs (gate chat.admin -> chat.user).

use axum::{
    extract::FromRequestParts,
    http::{request::Parts, StatusCode},
};
use serde::{Deserialize, Serialize};
use tower_sessions::Session;

/// Absolute max session lifetime regardless of activity (idle expiry is on the
/// layer). Fail closed: a session with no login_at is treated as expired.
const SESSION_ABSOLUTE_MAX_SECS: u64 = 12 * 3600;

fn now_unix_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EndUser {
    pub user_id: String,
    pub name: String,
    pub roles: Vec<String>,
}

impl EndUser {
    pub fn has(&self, role: &str) -> bool {
        self.roles.iter().any(|r| r == role)
    }
}

impl<S> FromRequestParts<S> for EndUser
where
    S: Send + Sync,
{
    type Rejection = (StatusCode, &'static str);

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let session = Session::from_request_parts(parts, state)
            .await
            .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, "session layer missing"))?;
        let user: Option<EndUser> = session
            .get("end_user")
            .await
            .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, "session read failed"))?;
        match user {
            Some(u) if u.has("chat.user") => {
                let login_at: Option<u64> = session
                    .get("login_at")
                    .await
                    .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, "session read failed"))?;
                match login_at {
                    Some(t) if now_unix_secs().saturating_sub(t) < SESSION_ABSOLUTE_MAX_SECS => Ok(u),
                    _ => {
                        let _ = session.delete().await;
                        Err((StatusCode::UNAUTHORIZED, "session expired; re-authenticate"))
                    }
                }
            }
            Some(_) => Err((StatusCode::FORBIDDEN, "user lacks chat.user")),
            None => Err((StatusCode::UNAUTHORIZED, "no session")),
        }
    }
}
```

- [ ] **Step 4: Run to verify pass**

Run: `cargo test -p kabytech-backend`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add services/kabytech/backend/src/session.rs
git commit -m "feat(kabytech-backend): EndUser session model + fail-closed chat.user extractor"
```

---

### Task 4: OIDC handlers + `main.rs` wiring (runnable backend)

Add the network handlers (`login`, `callback`, `logout`, `api_me`) to `auth.rs`, and wire the router/state/JWKS/session layer in `main.rs` — ported from `admin-api/src/{auth.rs,main.rs}`, gating `chat.user`, redirecting to the frontend origin. Handlers are network code (verified by the Task 6 gated live smoke + the pure tests already passing).

**Files:**
- Modify: `services/kabytech/backend/src/auth.rs` (append handlers)
- Modify: `services/kabytech/backend/src/main.rs` (full wiring)

**Interfaces:**
- Consumes: `crate::AppState`, `crate::config::KabyConfig`, `crate::session::EndUser`, `crate::auth::{pkce_pair, build_authorize_url}`, `zitadel_auth::{JwksCache, ZitadelConfig}`.
- Produces: handlers `auth::{login, callback, logout, api_me}`; a `main()` that binds `cfg.bind_addr`.

- [ ] **Step 1: Append the handlers to `auth.rs`**

```rust
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

pub async fn login(State(st): State<AppState>, session: Session) -> Response {
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
```

- [ ] **Step 2: Replace `main.rs` with the full wiring**

```rust
// kabytech-backend — end-user login gateway (OIDC Relying Party). Ported from
// admin-api/src/main.rs (trimmed: no zitadel admin client; gate chat.user).

use std::sync::Arc;

use axum::routing::get;
use axum::Router;
use kabytech_backend::config::KabyConfig;
use kabytech_backend::{auth, AppState};
use tower_http::trace::TraceLayer;
use tower_sessions::{Expiry, MemoryStore, SessionManagerLayer};
use zitadel_auth::{JwksCache, ZitadelConfig};

fn issuer_matches(configured: &str, discovered: &str) -> bool {
    configured == discovered.trim_end_matches('/')
}

async fn assert_issuer_match(http: &reqwest::Client, cfg: &KabyConfig) -> Result<(), String> {
    let url = format!("{}/.well-known/openid-configuration", cfg.issuer);
    let doc: serde_json::Value = http.get(&url).send().await
        .map_err(|e| format!("discovery fetch {url}: {e}"))?
        .json().await.map_err(|e| format!("discovery json: {e}"))?;
    let discovered = doc["issuer"].as_str().unwrap_or_default();
    if !issuer_matches(&cfg.issuer, discovered) {
        return Err(format!(
            "issuer mismatch: configured {} but discovery {}", cfg.issuer, discovered));
    }
    Ok(())
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .with_writer(std::io::stderr)
        .init();

    let cfg = KabyConfig::from_env().map_err(|e| -> Box<dyn std::error::Error> {
        tracing::error!(target: "kaby::config", error = %e, "config invalid");
        e.into()
    })?;
    tracing::info!(target: "kaby", issuer = %cfg.issuer, bind = %cfg.bind_addr, "kabytech-backend starting");

    let http = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()?;
    assert_issuer_match(&http, &cfg).await.map_err(|e| -> Box<dyn std::error::Error> {
        tracing::error!(target: "kaby::startup", error = %e, "issuer-match guard failed");
        e.into()
    })?;

    let jwks = JwksCache::new(ZitadelConfig {
        issuer: cfg.issuer.clone(),
        audience: vec![cfg.audience.clone()],
        jwks_uri: format!("{}/oauth/v2/keys", cfg.issuer),
        project_id: cfg.project_id.clone(),
    });
    match jwks.refresh().await {
        Ok(n) => tracing::info!(target: "kaby::startup", keys = n, "JWKS preloaded"),
        Err(e) => tracing::error!(target: "kaby::startup", error = %e, "JWKS preload failed"),
    }
    {
        let bg = jwks.clone();
        tokio::spawn(async move {
            let mut t = tokio::time::interval(std::time::Duration::from_secs(3600));
            t.tick().await;
            loop {
                t.tick().await;
                if let Err(e) = bg.refresh().await {
                    tracing::warn!(target: "kaby::startup", error = %e, "JWKS refresh failed");
                }
            }
        });
    }

    let state = AppState { cfg: cfg.clone(), jwks, http: Arc::new(http) };

    let session_layer = SessionManagerLayer::new(MemoryStore::default())
        .with_name("id")
        .with_same_site(tower_sessions::cookie::SameSite::Lax)
        .with_secure(cfg.cookie_secure)
        .with_expiry(Expiry::OnInactivity(time::Duration::hours(8)));

    let app = Router::new()
        .route("/login", get(auth::login))
        .route("/callback", get(auth::callback))
        .route("/logout", get(auth::logout))
        .route("/api/me", get(auth::api_me))
        .layer(session_layer)
        .layer(TraceLayer::new_for_http())
        .with_state(state);

    let listener = tokio::net::TcpListener::bind(&cfg.bind_addr).await?;
    tracing::info!(target: "kaby", addr = %cfg.bind_addr, "kabytech-backend listening");
    axum::serve(listener, app).await?;
    Ok(())
}

#[cfg(test)]
mod startup_tests {
    use super::issuer_matches;
    #[test]
    fn issuer_match_trims_discovery_slash() {
        assert!(issuer_matches("http://h:8080", "http://h:8080/"));
        assert!(!issuer_matches("http://h:8080", "http://other:8080"));
    }
}
```

- [ ] **Step 3: Build + test the whole crate**

Run: `cargo test -p kabytech-backend && cargo build -p kabytech-backend`
Expected: PASS — all unit tests (config, pkce, authorize-url, session, issuer-match) green; binary builds.

- [ ] **Step 4: Commit**

```bash
git add services/kabytech/backend/src/auth.rs services/kabytech/backend/src/main.rs Cargo.lock
git commit -m "feat(kabytech-backend): OIDC login/callback/logout/me handlers + runnable wiring"
```

---

### Task 5: Frontend scaffold (Next.js 16 + Tailwind v4) + login/home page

Create `services/kabytech/frontend` by copying `admin-web`'s build config (Next 16 is version-sensitive — copy, don't hand-write), trimmed to the MVP: one page with two states driven by `/api/me`, plus the same-origin proxy.

**Files:**
- Create: `services/kabytech/frontend/package.json`, `next.config.ts`, `tsconfig.json`, `postcss.config.mjs`, `app/layout.tsx`, `app/globals.css`, `app/page.tsx`, `app/page.test.tsx`, `vitest.config.ts`, `vitest.setup.ts`, `.gitignore`, `eslint.config.mjs`

**Interfaces:**
- Consumes: backend routes `/login`, `/logout`, `/api/me` via the proxy.
- Produces: a deployable Next standalone app on `:3001`.

- [ ] **Step 1: Copy the build/config scaffold from admin-web**

```bash
mkdir -p services/kabytech/frontend/app
cp admin-web/tsconfig.json services/kabytech/frontend/tsconfig.json
cp admin-web/postcss.config.mjs services/kabytech/frontend/postcss.config.mjs
cp admin-web/eslint.config.mjs services/kabytech/frontend/eslint.config.mjs
cp admin-web/.gitignore services/kabytech/frontend/.gitignore
cp admin-web/vitest.config.ts services/kabytech/frontend/vitest.config.ts
cp admin-web/vitest.setup.ts services/kabytech/frontend/vitest.setup.ts
```

Do **NOT** copy `admin-web/app/globals.css` — it `@import`s `tw-animate-css` and
`shadcn/tailwind.css`, which kabytech does not depend on (the build would fail).
Write a minimal Tailwind v4 stylesheet instead:

`services/kabytech/frontend/app/globals.css`:

```css
@import "tailwindcss";
```

- [ ] **Step 2: Write `services/kabytech/frontend/package.json`** (trimmed — no shadcn/tanstack/recharts)

```json
{
  "name": "kabytech-frontend",
  "version": "0.1.0",
  "private": true,
  "scripts": {
    "dev": "next dev",
    "build": "next build",
    "start": "next start",
    "lint": "eslint",
    "test": "vitest run"
  },
  "dependencies": {
    "next": "16.2.7",
    "react": "19.2.7",
    "react-dom": "19.2.7"
  },
  "devDependencies": {
    "@tailwindcss/postcss": "^4",
    "@testing-library/jest-dom": "^6.9.1",
    "@testing-library/react": "^16.3.2",
    "@types/node": "^20",
    "@types/react": "^19",
    "@types/react-dom": "^19",
    "@vitejs/plugin-react": "^6.0.2",
    "eslint": "^9",
    "eslint-config-next": "16.2.7",
    "jsdom": "^29.1.1",
    "tailwindcss": "^4",
    "typescript": "^5",
    "vitest": "^4.1.8"
  }
}
```

- [ ] **Step 3: Write `services/kabytech/frontend/next.config.ts`** (proxy → backend; security headers)

```ts
import type { NextConfig } from "next";

const KABY_BACKEND_ORIGIN = process.env.KABY_BACKEND_ORIGIN ?? "http://localhost:7670";

const nextConfig: NextConfig = {
  output: "standalone",
  async headers() {
    return [{
      source: "/:path*",
      headers: [
        { key: "X-Frame-Options", value: "DENY" },
        { key: "X-Content-Type-Options", value: "nosniff" },
        { key: "Referrer-Policy", value: "no-referrer" },
      ],
    }];
  },
  async rewrites() {
    return [
      { source: "/api/:path*", destination: `${KABY_BACKEND_ORIGIN}/api/:path*` },
      { source: "/login", destination: `${KABY_BACKEND_ORIGIN}/login` },
      { source: "/callback", destination: `${KABY_BACKEND_ORIGIN}/callback` },
      { source: "/logout", destination: `${KABY_BACKEND_ORIGIN}/logout` },
    ];
  },
};

export default nextConfig;
```

- [ ] **Step 4: Write `services/kabytech/frontend/app/layout.tsx`** (minimal — no shadcn)

```tsx
import type { Metadata } from "next";
import "./globals.css";

export const metadata: Metadata = {
  title: "kabytech",
  description: "kabytech gateway",
};

export default function RootLayout({ children }: Readonly<{ children: React.ReactNode }>) {
  return (
    <html lang="en" className="h-full antialiased">
      <body className="min-h-full bg-slate-50 text-slate-900">{children}</body>
    </html>
  );
}
```

- [ ] **Step 5: Write the failing component test `services/kabytech/frontend/app/page.test.tsx`**

```tsx
import { describe, it, expect, vi, afterEach } from "vitest";
import { render, screen } from "@testing-library/react";
import Page from "./page";

afterEach(() => vi.restoreAllMocks());

describe("kabytech login page", () => {
  it("shows Sign in when /api/me is 401", async () => {
    vi.stubGlobal("fetch", vi.fn(async () => new Response("", { status: 401 })));
    render(<Page />);
    expect(await screen.findByRole("link", { name: /sign in/i })).toHaveAttribute("href", "/login");
  });

  it("shows the user and Logout when /api/me returns a user", async () => {
    vi.stubGlobal("fetch", vi.fn(async () =>
      new Response(JSON.stringify({ userId: "u1", name: "Ada", roles: ["chat.user"] }),
        { status: 200, headers: { "content-type": "application/json" } })));
    render(<Page />);
    expect(await screen.findByText("Ada")).toBeInTheDocument();
    expect(screen.getByRole("link", { name: /log out/i })).toHaveAttribute("href", "/logout");
  });
});
```

- [ ] **Step 6: Run to verify fail**

Run: `cd services/kabytech/frontend && pnpm install && pnpm test`
Expected: FAIL (`./page` has no default export yet).

- [ ] **Step 7: Write `services/kabytech/frontend/app/page.tsx`**

```tsx
"use client";
import { useEffect, useState } from "react";

type Me = { userId: string; name: string; roles: string[] };

export default function Page() {
  const [me, setMe] = useState<Me | null>(null);
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    fetch("/api/me")
      .then((r) => (r.ok ? r.json() : null))
      .then((d) => setMe(d))
      .catch(() => setMe(null))
      .finally(() => setLoading(false));
  }, []);

  return (
    <main className="flex min-h-screen items-center justify-center p-6">
      <div className="w-full max-w-sm rounded-xl border border-slate-200 bg-white p-8 shadow-sm">
        <h1 className="mb-6 text-xl font-semibold">kabytech</h1>
        {loading ? (
          <p className="text-slate-500">Loading…</p>
        ) : me ? (
          <div className="space-y-4">
            <p className="text-sm text-slate-500">Signed in as</p>
            <p className="text-lg font-medium">{me.name}</p>
            <a href="/logout"
              className="inline-block rounded-md bg-slate-900 px-4 py-2 text-sm font-medium text-white">
              Log out
            </a>
          </div>
        ) : (
          <a href="/login"
            className="inline-block rounded-md bg-indigo-600 px-4 py-2 text-sm font-medium text-white">
            Sign in
          </a>
        )}
      </div>
    </main>
  );
}
```

- [ ] **Step 8: Run to verify pass + build**

Run: `cd services/kabytech/frontend && pnpm test && pnpm run build`
Expected: PASS (2 tests) and a successful standalone build.

- [ ] **Step 9: Commit**

```bash
git add services/kabytech/frontend
git commit -m "feat(kabytech-frontend): Next.js 16 login/home page + same-origin proxy"
```

---

### Task 6: Compose services + provisioned dev redirect URI + live smoke

Add Dockerfiles + compose services for both halves, register the kabytech-gateway client's **dev** redirect URI, and manually verify the login round-trip.

**Files:**
- Create: `deploy/compose/kabytech-backend.Dockerfile`
- Create: `deploy/compose/kabytech-backend-entrypoint.sh`
- Create: `deploy/compose/kabytech-frontend.Dockerfile`
- Modify: `docker-compose.yml` (two services)

**Interfaces:**
- Consumes: `secrets/kabytech_oidc_client_id` / `kabytech_oidc_client_secret`, `/out/manager.generated.env` (project_id/audience).

- [ ] **Step 1: Backend Dockerfile** (full-workspace build, mirrors admin-api.Dockerfile)

```dockerfile
# syntax=docker/dockerfile:1
FROM rust:1-bookworm AS build
WORKDIR /src
COPY Cargo.toml Cargo.lock ./
COPY crates ./crates
COPY manager/Cargo.toml ./manager/Cargo.toml
COPY manager/src ./manager/src
COPY worker/Cargo.toml worker/build.rs ./worker/
COPY worker/src ./worker/src
COPY admin-api/Cargo.toml ./admin-api/Cargo.toml
COPY admin-api/src ./admin-api/src
COPY clients/rust/Cargo.toml ./clients/rust/Cargo.toml
COPY clients/rust/src ./clients/rust/src
COPY services/kabytech/backend/Cargo.toml ./services/kabytech/backend/Cargo.toml
COPY services/kabytech/backend/src ./services/kabytech/backend/src
RUN cargo build --release --locked -p kabytech-backend

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*
COPY --from=build /src/target/release/kabytech-backend /usr/local/bin/kabytech-backend
COPY deploy/compose/kabytech-backend-entrypoint.sh /usr/local/bin/kabytech-backend-entrypoint.sh
RUN chmod +x /usr/local/bin/kabytech-backend-entrypoint.sh
EXPOSE 7670
ENTRYPOINT ["/usr/local/bin/kabytech-backend-entrypoint.sh"]
```

- [ ] **Step 2: Backend entrypoint** (sources generated env + resolves secret files)

`deploy/compose/kabytech-backend-entrypoint.sh`:

```sh
#!/bin/sh
set -eu
# project_id + audience written by the provisioner into the shared /out volume.
. /out/manager.generated.env
export ZITADEL_PROJECT_ID ZITADEL_AUDIENCE
# OIDC client id/secret come from mounted secret files (never baked into the image).
export KABY_OIDC_CLIENT_ID="$(cat /secrets/kabytech_oidc_client_id)"
export KABY_OIDC_CLIENT_SECRET="$(cat /secrets/kabytech_oidc_client_secret)"
exec /usr/local/bin/kabytech-backend
```

- [ ] **Step 3: Frontend Dockerfile** (mirrors admin-web.Dockerfile)

```dockerfile
# syntax=docker/dockerfile:1
FROM node:20-alpine AS build
WORKDIR /app
RUN corepack enable && corepack prepare pnpm@9.15.9 --activate
COPY package.json pnpm-lock.yaml ./
RUN pnpm install --frozen-lockfile
COPY . .
ARG KABY_BACKEND_ORIGIN=http://kabytech-backend:7670
ENV KABY_BACKEND_ORIGIN=${KABY_BACKEND_ORIGIN}
RUN pnpm run build

FROM node:20-alpine
WORKDIR /app
ENV NODE_ENV=production
COPY --from=build /app/.next/standalone ./
COPY --from=build /app/.next/static ./.next/static
COPY --from=build /app/public ./public
EXPOSE 3000
CMD ["node", "server.js"]
```

(Generate the lockfile first: `cd services/kabytech/frontend && pnpm install` so `pnpm-lock.yaml` exists for `--frozen-lockfile`. If `services/kabytech/frontend/public` does not exist, create it: `mkdir -p services/kabytech/frontend/public && touch services/kabytech/frontend/public/.gitkeep`.)

- [ ] **Step 4: Add the two compose services** to `docker-compose.yml`

```yaml
  kabytech-backend:
    build:
      context: .
      dockerfile: deploy/compose/kabytech-backend.Dockerfile
    environment:
      ZITADEL_ISSUER: http://host.docker.internal:8080
      KABY_BIND_ADDR: 0.0.0.0:7670
      KABY_PUBLIC_ORIGIN: http://localhost:3001
      KABY_ALLOWED_ORIGIN: http://localhost:3001
      KABY_SESSION_KEY: ${KABY_SESSION_KEY}
      KABY_COOKIE_SECURE: "false"
      RUST_LOG: ${RUST_LOG:-info}
    ports:
      - "127.0.0.1:7670:7670"
    depends_on:
      zitadel-init:
        condition: service_completed_successfully
    volumes:
      - genenv:/out:ro
      - ./secrets:/secrets:ro
    restart: unless-stopped

  kabytech-frontend:
    build:
      context: ./services/kabytech/frontend
      dockerfile: ../../deploy/compose/kabytech-frontend.Dockerfile
    environment:
      NODE_ENV: production
      KABY_BACKEND_ORIGIN: http://kabytech-backend:7670
    ports:
      - "127.0.0.1:3001:3000"
    depends_on:
      kabytech-backend:
        condition: service_started
    restart: unless-stopped
```

Add `KABY_SESSION_KEY=<32-byte hex>` to `.env` (generate: `openssl rand -hex 32`).

- [ ] **Step 5: Re-provision so the kabytech client carries the dev redirect URI**

The kabytech-gateway OIDC client must register `http://localhost:3001/callback`. Set it before a clean provision, then bring the stack up:

```bash
# clean-boot contract: wipe Zitadel + secrets so the client is recreated
docker compose down -v && rm -rf ./secrets
KABYTECH_OIDC_REDIRECT_URI=http://localhost:3001/callback \
KABYTECH_OIDC_POST_LOGOUT_URI=http://localhost:3001/ \
  docker compose up -d --build
```

(These two env vars are read by the provisioner's `create_kabytech_oidc_app`. Without them the client registers the prod placeholder and the dev callback 400s with `redirect_uri mismatch`.)

- [ ] **Step 6: Live smoke (manual) — the login round-trip**

1. Open `http://localhost:3001/` → the **Sign in** card renders.
2. Click **Sign in** → redirected to Zitadel's login.
3. Log in as a `chat.user` (e.g. `chatter` — creds in `secrets/chatter_user` / `chatter_password`).
4. Land back on `http://localhost:3001/` showing the user's name + **Log out**.
5. Confirm `GET http://localhost:3001/api/me` returns `{userId,name,roles:[…"chat.user"…]}`.
6. Click **Log out** → back to the **Sign in** state; `/api/me` is now 401.

Expected: all six succeed. A `403 not a chat.user` means the logged-in account lacks the role (use `chatter`). A `redirect_uri mismatch` means Step 5's env vars were not set during provisioning.

- [ ] **Step 7: Commit**

```bash
git add deploy/compose/kabytech-backend.Dockerfile deploy/compose/kabytech-backend-entrypoint.sh deploy/compose/kabytech-frontend.Dockerfile docker-compose.yml
git commit -m "feat(kabytech): compose services + dev redirect URI + login smoke"
```

---

## Final verification (after all tasks)

1. **Backend unit tests green:** `cargo test -p kabytech-backend` → config, pkce, authorize-url, session, issuer-match all pass.
2. **Frontend tests + build green:** `cd services/kabytech/frontend && pnpm test && pnpm run build`.
3. **Login round-trip works** (Task 6 Step 6): sign in as `chatter` → see the user → `/api/me` 200 → log out → `/api/me` 401.
4. **Per-user attribution still holds:** after a kabytech login chats (later phase), the Console attributes usage to that user's `sub` — out of scope for this MVP but unblocked by it.

The real upstream-IdP federation and chat forwarding are deliberately **not** in this MVP — they are the next phases on top of this authenticated session.
