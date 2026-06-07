//! The only module that touches Zitadel APIs. Submodules are added across
//! Phase C: error (gRPC->HTTP mapping), token (SA JWT-bearer + cache), model,
//! users, grants, keys. Task 12 lands `error`.

pub mod error;
pub mod token;

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
