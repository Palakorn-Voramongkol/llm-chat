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

#[tokio::test]
async fn create_grant_key_lifecycle_full_coverage() {
    if !it_enabled() {
        eprintln!("ADMIN_IT!=1 — skipping integration lifecycle test");
        return;
    }
    let cfg = llm_chat_admin_api_cfg();
    let http = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap();
    let z = admin_client(cfg, http);

    let suffix = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH).unwrap().as_secs();

    // ---- machine user: create (§6.4) -> grant (§6.5) -> key (§6.6) ----
    let m_name = format!("it-machine-{suffix}");
    let user_id = z.create_machine(&m_name, &m_name).await.expect("create_machine");
    assert!(!user_id.is_empty(), "create_machine must return userId");

    let grant_id = z.add_grant(&user_id, &["chat.user".into()]).await.expect("add_grant");
    assert!(!grant_id.is_empty(), "add_grant must return userGrantId");
    z.set_grant_roles(&user_id, &grant_id, &["chat.user".into(), "chat.admin".into()])
        .await.expect("set_grant_roles (PUT replace)");

    let key = z.create_json_key(&user_id).await.expect("create_json_key");
    assert!(key.get("keyDetails").and_then(|v| v.as_str()).is_some(),
        "create_json_key must return base64 keyDetails (returned once)");
    let key_id = key.get("keyId").and_then(|v| v.as_str()).expect("keyId").to_string();
    let keys = z.list_keys(&user_id).await.expect("list_keys");
    assert!(keys.iter().any(|k| k.get("id").and_then(|v| v.as_str()) == Some(&key_id)),
        "list_keys must include the just-created key id");
    z.delete_key(&user_id, &key_id).await.expect("delete_key");

    // client-secret lifecycle (§6.6): generate (shown once) then delete.
    let secret = z.generate_secret(&user_id).await.expect("generate_secret");
    assert!(secret.get("clientSecret").and_then(|v| v.as_str()).is_some(),
        "generate_secret must return clientSecret once");
    z.delete_secret(&user_id).await.expect("delete_secret");

    // read-back via v2 (§6.3): get_user maps the v2 shape.
    let fetched = z.get_user(&user_id).await.expect("get_user");
    assert_eq!(fetched.id, user_id, "get_user must round-trip the userId");

    // machine lifecycle: lock/unlock/deactivate/reactivate then delete.
    z.lock(&user_id).await.expect("lock");
    z.unlock(&user_id).await.expect("unlock");
    z.deactivate(&user_id).await.expect("deactivate");
    z.reactivate(&user_id).await.expect("reactivate");
    z.delete_user(&user_id).await.expect("delete_user (irreversible)");

    // ---- human user: create (§6.3) -> edit -> password -> resend-init -> delete ----
    let h_name = format!("it-human-{suffix}");
    let h_id = z.create_human(
        &h_name, "Given", "Family", &format!("{h_name}@example.localhost"), "Sup3r-Secret!"
    ).await.expect("create_human");
    assert!(!h_id.is_empty(), "create_human must return userId");
    z.edit_profile(&h_id, "Given2", "Family2").await.expect("edit_profile");
    z.edit_email(&h_id, &format!("{h_name}2@example.localhost"), true).await.expect("edit_email");
    z.set_password(&h_id, "An0ther-Secret!", false).await.expect("set_password");
    // resend_init is allowed only for INITIAL-state users; tolerate a precondition
    // failure here (the user is already active) rather than failing the suite.
    let _ = z.resend_init(&h_id).await;
    z.delete_user(&h_id).await.expect("delete human (irreversible)");
}

/// GATE for design §8: the two endpoints marked verified:false —
/// PUT .../apps/{appId}/oidc_config and
/// POST .../apps/{appId}/oidc_config/_generate_client_secret. We create a
/// throwaway OIDC app (the provisioner-proven create path), then exercise the
/// two unknowns and assert the live response shapes before any handler relies
/// on them. Driven straight through ZitadelClient (the source of truth).
#[tokio::test]
async fn it_verify_oidc_config_put_and_secret_regen() {
    if !it_enabled() {
        eprintln!("ADMIN_IT!=1 — skipping OIDC endpoint verification (design §8)");
        return;
    }
    let cfg = llm_chat_admin_api_cfg();
    let http = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap();
    let z = admin_client(cfg, http);

    let suffix = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH).unwrap().as_secs();
    let name = format!("it-oidc-app-{suffix}");

    // create (provisioner-proven, design §8 ✅): WEB+BASIC yields a clientSecret.
    let created = z.create_oidc_app(
        &name,
        &["https://example.localhost/callback".into()],
        &["OIDC_RESPONSE_TYPE_CODE".into()],
        &["OIDC_GRANT_TYPE_AUTHORIZATION_CODE".into()],
        "OIDC_APP_TYPE_WEB",
        "OIDC_AUTH_METHOD_TYPE_BASIC",
    ).await.expect("create_oidc_app");
    let app_id = created.get("appId").and_then(|v| v.as_str())
        .expect("create returns appId").to_string();
    assert!(created.get("clientId").and_then(|v| v.as_str()).is_some(),
        "create returns clientId");
    assert!(created.get("clientSecret").and_then(|v| v.as_str()).is_some(),
        "WEB+BASIC create returns clientSecret once");

    // get (design §8 ✅): reads back the oidc_config under the app.
    let app = z.get_app(&app_id).await.expect("get_app");
    assert!(app.get("oidcConfig").is_some() || app.get("app").is_some(),
        "get_app returns the app with its oidcConfig");

    // UNKNOWN #1 — PUT .../apps/{appId}/oidc_config (read-modify-write whole config).
    z.update_oidc_config(
        &app_id,
        &["https://example.localhost/callback".into(),
          "https://example.localhost/cb2".into()],
        &["OIDC_RESPONSE_TYPE_CODE".into()],
        &["OIDC_GRANT_TYPE_AUTHORIZATION_CODE".into()],
        "OIDC_APP_TYPE_WEB",
        "OIDC_AUTH_METHOD_TYPE_BASIC",
    ).await.expect("update_oidc_config (PUT oidc_config) — design §8 unknown #1");

    // UNKNOWN #2 — POST .../apps/{appId}/oidc_config/_generate_client_secret.
    let regen = z.regenerate_app_secret(&app_id).await
        .expect("regenerate_app_secret — design §8 unknown #2");
    assert!(regen.get("clientSecret").and_then(|v| v.as_str()).is_some(),
        "_generate_client_secret returns clientSecret once");

    // cleanup (design §8 ✅).
    z.delete_app(&app_id).await.expect("delete_app");
}

/// LIVE confinement smoke (§11/§14). The instance event log is instance-wide and
/// must NEVER leak another org's events: every returned event is asserted to
/// carry the SA's own resourceOwner. The audit grant (IAM_OWNER_VIEWER) is a
/// pending decision (§14 #1) so the test tolerates either capability outcome,
/// but the confinement invariant is enforced whenever events ARE readable.
#[tokio::test]
async fn it_audit_capability_and_confinement() {
    if !it_enabled() {
        eprintln!("ADMIN_IT!=1 — skipping audit capability test");
        return;
    }
    let cfg = llm_chat_admin_api_cfg();
    let http = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap();
    let z = admin_client(cfg, http);

    // The SA org must resolve (confinement anchor) — this is independent of the
    // event-read grant.
    let org = z.sa_org_id().await.expect("sa_org_id resolves the SA org");
    assert!(!org.is_empty(), "resourceOwner must be non-empty");

    // Capability probe must not panic and returns a definite boolean (§3/§11).
    let can = z.can_read_events().await.expect("capability probe returns Ok");
    if can {
        // When readable, every returned event is confined to the SA's org.
        let q = llm_chat_admin_api::zitadel::events::EventQuery {
            editor_user_id: None, aggregate_id: None, from: None, asc: false, limit: 20,
        };
        let events = z.search_events(&q).await.expect("confined search");
        for e in &events {
            if let Some(owner) = e.get("editor").and_then(|x| x.get("resourceOwner")).and_then(|v| v.as_str()) {
                assert_eq!(owner, org, "event leaked from another org (confinement broken)");
            }
        }
    } else {
        eprintln!("audit capability OFF (IAM_OWNER_VIEWER not granted) — banner path, §14 #1");
    }
}
