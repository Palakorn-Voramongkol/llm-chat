//! kabytech-backend — end-user login gateway (OIDC Relying Party). The browser
//! holds only an opaque session cookie; this backend owns the OIDC flow + secret.
pub mod auth;
pub mod config;
pub mod provision;
pub mod session;
pub mod zitadel;

use std::sync::Arc;
use zitadel_auth::JwksCache;

/// Shared application state (cheap to clone).
#[derive(Clone)]
pub struct AppState {
    pub cfg: config::KabyConfig,
    pub jwks: JwksCache,
    pub http: Arc<reqwest::Client>,
    pub zitadel: zitadel::Zitadel,
}
