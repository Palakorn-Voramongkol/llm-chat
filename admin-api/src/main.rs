// llm-chat-admin-api — Backend-For-Frontend for the Zitadel user-management
// admin. Owns the operator OIDC session + the least-privilege admin service
// account; the browser only ever holds an opaque session cookie.

use llm_chat_admin_api::config::AdminConfig;
use llm_chat_admin_api::zitadel;

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

    let _client = zitadel::ZitadelClient::new(cfg.clone(), http);
    // Router + serve land in Task 18. For now, bind so the fail-fast guards are
    // exercised and the process stays up.
    let listener = tokio::net::TcpListener::bind(&cfg.bind_addr).await?;
    tracing::info!(target: "admin-api", addr = %cfg.bind_addr, "admin-api listening (router pending Task 18)");
    let app = axum::Router::new();
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
