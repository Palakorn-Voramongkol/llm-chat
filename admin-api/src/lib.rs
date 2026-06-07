//! admin-api library surface — shared by the binary and the integration tests.
pub mod api;
pub mod auth;
pub mod config;
pub mod session;
pub mod zitadel;

use std::sync::Arc;

/// Shared handler state. Clone is cheap (Arc + reqwest::Client are ref-counted).
#[derive(Clone)]
pub struct AppState {
    pub cfg: config::AdminConfig,
    pub jwks: zitadel_auth::JwksCache,
    pub zitadel: Arc<zitadel::ZitadelClient>,
    pub http: reqwest::Client,
}
