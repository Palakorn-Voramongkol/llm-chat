//! Zitadel JWT verification for the manager.
//!
//! Two paths:
//!   * `JwksCache::refresh()` — async; fetches the JWKS from Zitadel and
//!     replaces the cache atomically. Run at startup + every hour by the
//!     background refresher.
//!   * `JwksCache::verify_sync()` — sync; the WS handshake callback is sync,
//!     so we can't await JWKS fetches inline. The cache is preloaded; if the
//!     `kid` isn't there, reject (and trust the background refresher to pick
//!     up new keys soon).
//!
//! Required env on the manager process:
//!   ZITADEL_ISSUER       — e.g. https://id.palakorn.com
//!   ZITADEL_AUDIENCE     — comma-separated; the project_id (Zitadel encodes it as `aud` claim)
//!   ZITADEL_PROJECT_ID   — the project id; used to find roles claim

use std::collections::HashMap;
use std::fmt;
use std::sync::{Arc, RwLock};
use std::time::Instant;

use jsonwebtoken::{decode, decode_header, Algorithm, DecodingKey, Validation};
use serde::Deserialize;
use tokio_tungstenite::tungstenite::handshake::server::Request;

#[derive(Debug, Clone)]
pub struct Principal {
    pub user_id: String,
    pub org_id: String,
    pub roles: Vec<String>,
    pub email: Option<String>,
}

impl Principal {
    pub fn has(&self, role: &str) -> bool {
        self.roles.iter().any(|r| r == role)
    }
}

#[derive(Debug)]
pub enum AuthError {
    Missing,
    Malformed,
    UnknownKey(String),
    Invalid(String),
    JwksFetch(String),
    Forbidden(String),
}

impl fmt::Display for AuthError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AuthError::Missing       => write!(f, "missing Authorization header"),
            AuthError::Malformed     => write!(f, "malformed Authorization header"),
            AuthError::UnknownKey(k) => write!(f, "unknown signing key: {}", k),
            AuthError::Invalid(e)    => write!(f, "token validation failed: {}", e),
            AuthError::JwksFetch(e)  => write!(f, "JWKS fetch failed: {}", e),
            AuthError::Forbidden(s)  => write!(f, "forbidden: {}", s),
        }
    }
}

impl std::error::Error for AuthError {}

#[derive(Clone)]
pub struct ZitadelConfig {
    pub issuer: String,
    pub audience: Vec<String>,
    pub jwks_uri: String,
    pub project_id: String,
}

impl ZitadelConfig {
    pub fn from_env() -> Result<Self, String> {
        let issuer = std::env::var("ZITADEL_ISSUER")
            .map_err(|_| "ZITADEL_ISSUER not set".to_string())?
            .trim_end_matches('/')
            .to_string();
        let audience: Vec<String> = std::env::var("ZITADEL_AUDIENCE")
            .map_err(|_| "ZITADEL_AUDIENCE not set".to_string())?
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
        if audience.is_empty() {
            return Err("ZITADEL_AUDIENCE is empty after parse".into());
        }
        let project_id = std::env::var("ZITADEL_PROJECT_ID")
            .map_err(|_| "ZITADEL_PROJECT_ID not set".to_string())?;
        let jwks_uri = format!("{}/oauth/v2/keys", issuer);
        Ok(Self { issuer, audience, jwks_uri, project_id })
    }
}

#[derive(Deserialize)]
struct Jwks { keys: Vec<JwkInner> }

#[derive(Deserialize)]
struct JwkInner { kid: String, n: String, e: String }

struct CacheInner {
    keys: HashMap<String, DecodingKey>,
    fetched_at: Instant,
}

/// JWKS cache. Cheap to clone; wraps an Arc internally.
#[derive(Clone)]
pub struct JwksCache {
    cfg: ZitadelConfig,
    inner: Arc<RwLock<Option<CacheInner>>>,
}

impl JwksCache {
    pub fn new(cfg: ZitadelConfig) -> Self {
        Self { cfg, inner: Arc::new(RwLock::new(None)) }
    }

    pub fn cfg(&self) -> &ZitadelConfig { &self.cfg }

    /// Fetch JWKS from Zitadel and replace the cache atomically.
    pub async fn refresh(&self) -> Result<usize, AuthError> {
        let body: Jwks = reqwest::get(&self.cfg.jwks_uri)
            .await
            .map_err(|e| AuthError::JwksFetch(e.to_string()))?
            .json()
            .await
            .map_err(|e| AuthError::JwksFetch(e.to_string()))?;
        let mut keys = HashMap::with_capacity(body.keys.len());
        for k in body.keys {
            let dk = DecodingKey::from_rsa_components(&k.n, &k.e)
                .map_err(|e| AuthError::JwksFetch(format!("bad key {}: {}", k.kid, e)))?;
            keys.insert(k.kid, dk);
        }
        let n = keys.len();
        *self.inner.write().unwrap() = Some(CacheInner { keys, fetched_at: Instant::now() });
        Ok(n)
    }

    /// Verify a JWT using the currently-cached keys. Sync — no network.
    /// Returns the verified Principal or an AuthError. Caller must enforce
    /// any further authorization (role checks, scope, etc).
    pub fn verify_sync(&self, token: &str) -> Result<Principal, AuthError> {
        let header = decode_header(token).map_err(|e| AuthError::Invalid(e.to_string()))?;
        let kid = header.kid.ok_or_else(|| AuthError::Invalid("missing kid".into()))?;

        let key = {
            let guard = self.inner.read().unwrap();
            let inner = guard.as_ref().ok_or_else(|| AuthError::JwksFetch("cache empty".into()))?;
            inner.keys.get(&kid).cloned().ok_or_else(|| AuthError::UnknownKey(kid.clone()))?
        };

        let mut validation = Validation::new(Algorithm::RS256);
        validation.set_issuer(&[self.cfg.issuer.as_str()]);
        let aud_refs: Vec<&str> = self.cfg.audience.iter().map(|s| s.as_str()).collect();
        validation.set_audience(&aud_refs);

        let data: jsonwebtoken::TokenData<serde_json::Value> = decode(token, &key, &validation)
            .map_err(|e| AuthError::Invalid(e.to_string()))?;
        let claims = data.claims;

        let user_id = claims.get("sub").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let org_id = claims
            .get("urn:zitadel:iam:user:resourceowner:id")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let email = claims.get("email").and_then(|v| v.as_str()).map(|s| s.to_string());

        // Zitadel encodes project roles under
        //   urn:zitadel:iam:org:project:<projectid>:roles
        let roles_key = format!("urn:zitadel:iam:org:project:{}:roles", self.cfg.project_id);
        let roles: Vec<String> = claims
            .get(&roles_key)
            .and_then(|v| v.as_object())
            .map(|m| m.keys().cloned().collect())
            .unwrap_or_default();

        Ok(Principal { user_id, org_id, roles, email })
    }
}

/// Pull `Authorization: Bearer <token>` from a tungstenite handshake request.
/// Falls back to `?token=…` query param.
pub fn extract_bearer(req: &Request) -> Result<String, AuthError> {
    if let Some(auth) = req.headers().get("authorization") {
        if let Ok(s) = auth.to_str() {
            if let Some(t) = s.strip_prefix("Bearer ") {
                return Ok(t.trim().to_string());
            }
            return Err(AuthError::Malformed);
        }
        return Err(AuthError::Malformed);
    }
    if let Some(query) = req.uri().query() {
        for kv in query.split('&') {
            if let Some(t) = kv.strip_prefix("token=") {
                return Ok(t.to_string());
            }
        }
    }
    Err(AuthError::Missing)
}
