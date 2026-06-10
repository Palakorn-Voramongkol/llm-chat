//! The only module that touches Zitadel APIs. Submodules are added across
//! Phase C: error (gRPC->HTTP mapping), token (SA JWT-bearer + cache), model,
//! users, grants, keys, apps, policies, project, stats, and `events` (the
//! audit-log / capability-probe surface, design §11). Each submodule's `impl
//! ZitadelClient` block is only reachable once it is declared below, so this
//! list and the `pub mod` declarations must stay in sync.

pub mod apps;
pub mod error;
pub mod events;
pub mod grants;
pub mod keys;
pub mod model;
pub mod policies;
pub mod project;
pub mod stats;
pub mod token;
pub mod users;

use crate::config::AdminConfig;
use token::CachedToken;

/// The only struct that calls Zitadel write APIs. Holds the SA key (via cfg),
/// a shared reqwest client, and a cached Management-API token refreshed before
/// expiry. Submodule `impl` blocks add the actual API methods across Phase C.
pub struct ZitadelClient {
    pub cfg: AdminConfig,
    pub http: reqwest::Client,
    pub token: tokio::sync::RwLock<Option<CachedToken>>,
}

impl ZitadelClient {
    /// Two-arg constructor (infallible): the caller builds the redirect-less
    /// reqwest client once and shares it. Used by main.rs and the integration
    /// tests alike.
    pub fn new(cfg: AdminConfig, http: reqwest::Client) -> Self {
        Self { cfg, http, token: tokio::sync::RwLock::new(None) }
    }
}

use crate::zitadel::error::{map_status, path_has_traversal, ZitadelError};
use serde_json::Value;

impl ZitadelClient {
    async fn send_json(
        &self,
        method: reqwest::Method,
        url: &str,
        body: Option<&Value>,
    ) -> Result<Value, ZitadelError> {
        // Fail closed on any `.`/`..` path segment smuggled in via an
        // operator-supplied id before reqwest normalizes it away and re-points
        // the privileged request at another API path (see path_has_traversal).
        if path_has_traversal(url) {
            return Err(ZitadelError::Invalid("illegal path segment in request url".into()));
        }
        let token = self.valid_token().await?;
        let mut req = self
            .http
            .request(method, url)
            .bearer_auth(token)
            .header(reqwest::header::CONTENT_TYPE, "application/json");
        if let Some(b) = body {
            req = req.json(b);
        }
        let resp = req
            .send()
            .await
            .map_err(|e| ZitadelError::Transport(e.to_string()))?;
        let status = resp.status().as_u16();
        let text = resp.text().await.unwrap_or_default();
        if !(200..300).contains(&status) {
            return Err(map_status(status, &text));
        }
        if text.is_empty() {
            return Ok(Value::Null);
        }
        serde_json::from_str(&text)
            .map_err(|e| ZitadelError::Invalid(format!("response json: {e}")))
    }

    pub(crate) async fn post_json(&self, url: &str, body: &Value) -> Result<Value, ZitadelError> {
        self.send_json(reqwest::Method::POST, url, Some(body)).await
    }
    pub(crate) async fn put_json(&self, url: &str, body: &Value) -> Result<Value, ZitadelError> {
        self.send_json(reqwest::Method::PUT, url, Some(body)).await
    }
    pub(crate) async fn get_json(&self, url: &str) -> Result<Value, ZitadelError> {
        self.send_json(reqwest::Method::GET, url, None).await
    }
    pub(crate) async fn delete(&self, url: &str) -> Result<Value, ZitadelError> {
        self.send_json(reqwest::Method::DELETE, url, None).await
    }
}
