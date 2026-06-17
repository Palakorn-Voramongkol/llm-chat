//! OIDC application (login client) CRUD within a project (design §8).
//! An "App" = a Zitadel Project; these are the OIDC *clients* under it.
//! v1 Management API. clientSecret is returned ONCE by create + regenerate and
//! is streamed straight through untouched — NEVER logged (design §3 invariant,
//! same contract as keys::create_json_key). The two endpoints marked
//! verified:false in §8 (oidc_config PUT, _generate_client_secret) are confirmed
//! live by tests/integration.rs::it_verify_oidc_config_put_and_secret_regen.

use serde_json::{json, Value};

use super::error::ZitadelError;
use super::ZitadelClient;

/// PURE: the create body — the provisioner-proven shape (provision.py
/// create_admin_oidc_app). accessTokenType is the OIDC app enum
/// OIDC_TOKEN_TYPE_JWT (NOT the machine ACCESS_TOKEN_TYPE_JWT — §enum trap).
fn oidc_create_body(
    name: &str,
    redirect_uris: &[String],
    response_types: &[String],
    grant_types: &[String],
    app_type: &str,
    auth_method: &str,
) -> Value {
    json!({
        "name": name,
        "redirectUris": redirect_uris,
        "responseTypes": response_types,
        "grantTypes": grant_types,
        "appType": app_type,
        "authMethodType": auth_method,
        "accessTokenType": "OIDC_TOKEN_TYPE_JWT",
        "devMode": true,
        "accessTokenRoleAssertion": true,
        "idTokenRoleAssertion": true,
    })
}

/// PURE: the PUT oidc_config body — read-modify-write the full config (design
/// §8). No `name` (that's an app-level field, not part of oidc_config).
fn oidc_update_body(
    redirect_uris: &[String],
    response_types: &[String],
    grant_types: &[String],
    app_type: &str,
    auth_method: &str,
) -> Value {
    json!({
        "redirectUris": redirect_uris,
        "responseTypes": response_types,
        "grantTypes": grant_types,
        "appType": app_type,
        "authMethodType": auth_method,
        "accessTokenType": "OIDC_TOKEN_TYPE_JWT",
        "devMode": true,
        "accessTokenRoleAssertion": true,
        "idTokenRoleAssertion": true,
    })
}

impl ZitadelClient {
    /// List the HOME project's apps (§8 ✅). Thin alias over `list_apps_for`.
    pub async fn list_apps(&self) -> Result<Vec<Value>, ZitadelError> {
        let pid = self.cfg.project_id.clone();
        self.list_apps_for(&pid).await
    }

    /// List ANY project's apps (login clients): POST
    /// /management/v1/projects/{pid}/apps/_search. Each application (project)
    /// has its own login clients in the multi-app model.
    pub async fn list_apps_for(&self, project_id: &str) -> Result<Vec<Value>, ZitadelError> {
        let url = format!("{}/management/v1/projects/{}/apps/_search", self.cfg.issuer, project_id);
        let v = self.post_json(&url, &json!({})).await?;
        Ok(v.get("result").and_then(Value::as_array).cloned().unwrap_or_default())
    }

    /// Create an OIDC app: POST /management/v1/projects/{pid}/apps/oidc (§8 ✅,
    /// provisioner-proven). Returns the FULL response — clientId + clientSecret
    /// (shown ONCE) live here; streamed straight through, never logged.
    pub async fn create_oidc_app(
        &self,
        name: &str,
        redirect_uris: &[String],
        response_types: &[String],
        grant_types: &[String],
        app_type: &str,
        auth_method: &str,
    ) -> Result<Value, ZitadelError> {
        let pid = &self.cfg.project_id;
        let url = format!("{}/management/v1/projects/{}/apps/oidc", self.cfg.issuer, pid);
        let body = oidc_create_body(name, redirect_uris, response_types, grant_types, app_type, auth_method);
        self.post_json(&url, &body).await
    }

    /// Get one app: GET /management/v1/projects/{pid}/apps/{appId} (§8 ✅).
    pub async fn get_app(&self, app_id: &str) -> Result<Value, ZitadelError> {
        let pid = &self.cfg.project_id;
        let url = format!("{}/management/v1/projects/{}/apps/{}", self.cfg.issuer, pid, app_id);
        self.get_json(&url).await
    }

    /// Replace the whole oidc_config: PUT
    /// /management/v1/projects/{pid}/apps/{appId}/oidc_config (§8 unknown #1,
    /// confirmed live by it_verify_oidc_config_put_and_secret_regen).
    pub async fn update_oidc_config(
        &self,
        app_id: &str,
        redirect_uris: &[String],
        response_types: &[String],
        grant_types: &[String],
        app_type: &str,
        auth_method: &str,
    ) -> Result<(), ZitadelError> {
        let pid = &self.cfg.project_id;
        let url = format!("{}/management/v1/projects/{}/apps/{}/oidc_config", self.cfg.issuer, pid, app_id);
        let body = oidc_update_body(redirect_uris, response_types, grant_types, app_type, auth_method);
        self.put_json(&url, &body).await.map(|_| ())
    }

    /// Regenerate the client secret: POST
    /// /management/v1/projects/{pid}/apps/{appId}/oidc_config/_generate_client_secret
    /// (§8 unknown #2, confirmed live). Returns clientSecret ONCE — straight
    /// through, never logged.
    pub async fn regenerate_app_secret(&self, app_id: &str) -> Result<Value, ZitadelError> {
        let pid = &self.cfg.project_id;
        let url = format!(
            "{}/management/v1/projects/{}/apps/{}/oidc_config/_generate_client_secret",
            self.cfg.issuer, pid, app_id
        );
        self.post_json(&url, &json!({})).await
    }

    /// Delete an app: DELETE /management/v1/projects/{pid}/apps/{appId} (§8 ✅).
    pub async fn delete_app(&self, app_id: &str) -> Result<(), ZitadelError> {
        let pid = &self.cfg.project_id;
        let url = format!("{}/management/v1/projects/{}/apps/{}", self.cfg.issuer, pid, app_id);
        self.delete(&url).await.map(|_| ())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn oidc_create_body_carries_the_provisioner_proven_fields() {
        let body = oidc_create_body(
            "Chat",
            &["https://x/cb".into()],
            &["OIDC_RESPONSE_TYPE_CODE".into()],
            &["OIDC_GRANT_TYPE_AUTHORIZATION_CODE".into()],
            "OIDC_APP_TYPE_WEB",
            "OIDC_AUTH_METHOD_TYPE_BASIC",
        );
        assert_eq!(body["name"], "Chat");
        assert_eq!(body["redirectUris"][0], "https://x/cb");
        assert_eq!(body["responseTypes"][0], "OIDC_RESPONSE_TYPE_CODE");
        assert_eq!(body["grantTypes"][0], "OIDC_GRANT_TYPE_AUTHORIZATION_CODE");
        assert_eq!(body["appType"], "OIDC_APP_TYPE_WEB");
        assert_eq!(body["authMethodType"], "OIDC_AUTH_METHOD_TYPE_BASIC");
        assert_eq!(body["accessTokenType"], "OIDC_TOKEN_TYPE_JWT");
    }

    #[test]
    fn oidc_update_body_omits_name_but_keeps_the_full_config() {
        let body = oidc_update_body(
            &["https://x/cb".into()],
            &["OIDC_RESPONSE_TYPE_CODE".into()],
            &["OIDC_GRANT_TYPE_AUTHORIZATION_CODE".into()],
            "OIDC_APP_TYPE_NATIVE",
            "OIDC_AUTH_METHOD_TYPE_NONE",
        );
        assert!(body.get("name").is_none(), "PUT oidc_config takes no name");
        assert_eq!(body["appType"], "OIDC_APP_TYPE_NATIVE");
        assert_eq!(body["authMethodType"], "OIDC_AUTH_METHOD_TYPE_NONE");
        assert_eq!(body["redirectUris"][0], "https://x/cb");
    }
}
