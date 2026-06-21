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
    let doc: serde_json::Value = http
        .get(&url)
        .send()
        .await
        .map_err(|e| format!("discovery fetch {url}: {e}"))?
        .json()
        .await
        .map_err(|e| format!("discovery json: {e}"))?;
    let discovered = doc["issuer"].as_str().unwrap_or_default();
    if !issuer_matches(&cfg.issuer, discovered) {
        return Err(format!(
            "issuer mismatch: configured {} but discovery {}",
            cfg.issuer, discovered
        ));
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
