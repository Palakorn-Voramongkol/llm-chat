// llm-chat-admin-api — Backend-For-Frontend for the Zitadel user-management
// admin. Owns the operator OIDC session + the least-privilege admin service
// account; the browser only ever holds an opaque session cookie.

use std::sync::Arc;

use llm_chat_admin_api::config::AdminConfig;
use llm_chat_admin_api::{api, zitadel, AppState};
use tower_http::trace::TraceLayer;
use tower_sessions::{Expiry, MemoryStore, SessionManagerLayer};
use zitadel_auth::{JwksCache, ZitadelConfig};

/// PURE: the two issuer strings must match byte-for-byte (single-issuer
/// linchpin, design §8). `configured` is already trailing-slash-trimmed by
/// AdminConfig::from_map; trim the discovery value the same way before compare.
fn issuer_matches(configured: &str, discovered: &str) -> bool {
    configured == discovered.trim_end_matches('/')
}

async fn assert_issuer_match(
    http: &reqwest::Client,
    cfg: &AdminConfig,
) -> Result<(), String> {
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
            "issuer mismatch: configured ZITADEL_ISSUER={} but discovery issuer={} \
             (a single literal issuer must match byte-for-byte, design §8)",
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

    let cfg = AdminConfig::from_env().map_err(|e| -> Box<dyn std::error::Error> {
        tracing::error!(target: "admin-api::config", error = %e, "config invalid");
        e.into()
    })?;
    tracing::info!(
        target: "admin-api",
        issuer = %cfg.issuer,
        project_id = %cfg.project_id,
        bind = %cfg.bind_addr,
        "admin-api starting"
    );

    let http = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()?;

    assert_issuer_match(&http, &cfg).await.map_err(|e| -> Box<dyn std::error::Error> {
        tracing::error!(target: "admin-api::startup", error = %e, "issuer-match guard failed");
        e.into()
    })?;
    tracing::info!(target: "admin-api::startup", "issuer-match guard passed");

    let zitadel_client = Arc::new(zitadel::ZitadelClient::new(cfg.clone(), http.clone()));
    let jwks = JwksCache::new(ZitadelConfig {
        issuer: cfg.issuer.clone(),
        audience: vec![cfg.audience.clone()],
        jwks_uri: format!("{}/oauth/v2/keys", cfg.issuer),
        project_id: cfg.project_id.clone(),
    });
    // Preload the JWKS so the first /callback can verify the operator's token.
    // verify_sync is sync (reads the cache, never fetches), so without a preload
    // the cache stays empty and every verify fails "JWKS fetch failed: cache
    // empty". Mirror the manager: preload now, then refresh hourly in the
    // background.
    match jwks.refresh().await {
        Ok(n) => tracing::info!(target: "admin-api::startup", keys = n, "JWKS preloaded"),
        Err(e) => tracing::error!(target: "admin-api::startup", error = %e,
            "JWKS preload failed — operators will be rejected until refresh succeeds"),
    }
    {
        let bg = jwks.clone();
        tokio::spawn(async move {
            let mut t = tokio::time::interval(std::time::Duration::from_secs(3600));
            t.tick().await; // skip the immediate tick
            loop {
                t.tick().await;
                if let Err(e) = bg.refresh().await {
                    tracing::warn!(target: "admin-api::startup", error = %e, "JWKS refresh failed");
                } else {
                    tracing::debug!(target: "admin-api::startup", "JWKS refreshed");
                }
            }
        });
    }
    let state = AppState {
        cfg: cfg.clone(),
        jwks,
        zitadel: zitadel_client,
        http: http.clone(),
    };

    // tower-sessions: in-memory store, signed cookie, SameSite=Lax (same-origin
    // proxy means Lax survives the Zitadel 302 back). secure=false for the
    // plain-HTTP dev origin; flip to true behind TLS.
    let session_layer = SessionManagerLayer::new(MemoryStore::default())
        .with_name("id")
        .with_same_site(tower_sessions::cookie::SameSite::Lax)
        .with_secure(false)
        .with_expiry(Expiry::OnInactivity(time::Duration::hours(8)));

    let app = api::router(state)
        .layer(session_layer)
        .layer(TraceLayer::new_for_http());

    let listener = tokio::net::TcpListener::bind(&cfg.bind_addr).await?;
    tracing::info!(target: "admin-api", addr = %cfg.bind_addr, "admin-api listening");
    axum::serve(listener, app).await?;
    Ok(())
}

#[cfg(test)]
mod startup_tests {
    use super::issuer_matches;

    #[test]
    fn issuer_match_trims_discovery_slash() {
        assert!(issuer_matches("http://h:8080", "http://h:8080/"));
        assert!(issuer_matches("http://h:8080", "http://h:8080"));
    }

    #[test]
    fn issuer_mismatch_detected() {
        assert!(!issuer_matches("http://h:8080", "http://other:8080"));
    }
}
