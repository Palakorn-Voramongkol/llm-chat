//! Integration tests vs a RUNNING Zitadel v3.4.10. Gated on ADMIN_IT=1 so the
//! default `cargo test` stays offline. Discharges appendix §6 items against the
//! real instance (the source of truth) instead of mocking shapes.

use llm_chat_admin_api::config::AdminConfig;
use llm_chat_admin_api::zitadel::ZitadelClient;

fn it_enabled() -> bool {
    std::env::var("ADMIN_IT").as_deref() == Ok("1")
}

fn llm_chat_admin_api_cfg() -> AdminConfig {
    AdminConfig::from_env().expect("admin config from env (ADMIN_IT run)")
}

fn admin_client(cfg: AdminConfig, http: reqwest::Client) -> ZitadelClient {
    ZitadelClient::new(cfg, http)
}

#[tokio::test]
async fn it_mint_management_token() {
    if !it_enabled() {
        eprintln!("skipping (set ADMIN_IT=1 + Zitadel env to run) — appendix §6.2");
        return;
    }
    let cfg = llm_chat_admin_api_cfg();
    let http = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap();
    let client = admin_client(cfg, http);
    let tok = client.mint_management_token().await.expect("mint ok (§6.2)");
    assert!(!tok.token.is_empty(), "access_token present");
    assert!(tok.exp > 0, "expires_in mapped to absolute exp");
}
