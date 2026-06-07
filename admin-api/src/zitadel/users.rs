//! User search/get (v2 reads) + create/edit/lifecycle (v1+v2 writes).
//! Read shape -> model::user_from_v2; write shapes per appendix §3.2/§3.6.
//! Exact response keys verified by tests/integration.rs (ADMIN_IT=1).

use serde_json::{json, Value};

use super::error::ZitadelError;
use super::model::{user_from_v2, User};
use super::ZitadelClient;

impl ZitadelClient {
    /// List/search users via v2: POST /v2/users (§3.1). `type`/`state` optional.
    pub async fn search_users(&self, queries: Vec<Value>) -> Result<Vec<User>, ZitadelError> {
        let url = format!("{}/v2/users", self.cfg.issuer);
        let v = self.post_json(&url, &json!({ "queries": queries })).await?;
        let result = v.get("result").and_then(Value::as_array).cloned().unwrap_or_default();
        Ok(result.iter().map(user_from_v2).collect())
    }

    /// Get one user via v2: GET /v2/users/{id} -> {user:{...}} (§3.1).
    pub async fn get_user(&self, id: &str) -> Result<User, ZitadelError> {
        let url = format!("{}/v2/users/{}", self.cfg.issuer, id);
        let v = self.get_json(&url).await?;
        let user = v.get("user").unwrap_or(&v);
        Ok(user_from_v2(user))
    }

    /// Create a human via v2 (the repo's working path, §3.2): nested password
    /// {password,changeRequired:false} + email{isVerified} = immediately active.
    pub async fn create_human(
        &self, username: &str, given: &str, family: &str, email: &str, password: &str,
    ) -> Result<String, ZitadelError> {
        let url = format!("{}/v2/users/human", self.cfg.issuer);
        let body = json!({
            "username": username,
            "profile": { "givenName": given, "familyName": family },
            "email": { "email": email, "isVerified": true },
            "password": { "password": password, "changeRequired": false },
        });
        let v = self.post_json(&url, &body).await?;
        Ok(v.get("userId").and_then(Value::as_str).unwrap_or_default().to_string())
    }

    /// Create a machine user via v1 (§3.2). ACCESS_TOKEN_TYPE_JWT (machine USER,
    /// not the OIDC-app enum) so the manager can verify the token via JWKS.
    pub async fn create_machine(&self, username: &str, name: &str) -> Result<String, ZitadelError> {
        let url = format!("{}/management/v1/users/machine", self.cfg.issuer);
        let body = json!({
            "userName": username, "name": name,
            "accessTokenType": "ACCESS_TOKEN_TYPE_JWT",
        });
        let v = self.post_json(&url, &body).await?;
        Ok(v.get("userId").and_then(Value::as_str).unwrap_or_default().to_string())
    }

    /// Edit human profile (v1): PUT /management/v1/users/{id}/profile (§5 table).
    pub async fn edit_profile(&self, id: &str, given: &str, family: &str) -> Result<(), ZitadelError> {
        let url = format!("{}/management/v1/users/{}/profile", self.cfg.issuer, id);
        let body = json!({ "firstName": given, "lastName": family });
        self.put_json(&url, &body).await.map(|_| ())
    }

    /// Edit human email (v1): PUT /management/v1/users/{id}/email (§5 table).
    pub async fn edit_email(&self, id: &str, email: &str, verified: bool) -> Result<(), ZitadelError> {
        let url = format!("{}/management/v1/users/{}/email", self.cfg.issuer, id);
        let body = json!({ "email": email, "isEmailVerified": verified });
        self.put_json(&url, &body).await.map(|_| ())
    }

    /// Set a human password (v1): PUT /management/v1/users/{id}/password (§3.2/§5).
    pub async fn set_password(&self, id: &str, password: &str, change_required: bool) -> Result<(), ZitadelError> {
        let url = format!("{}/management/v1/users/{}/password", self.cfg.issuer, id);
        let body = json!({ "newPassword": { "password": password, "changeRequired": change_required } });
        self.put_json(&url, &body).await.map(|_| ())
    }

    /// Resend the initialization mail (v1): POST .../{id}/_resend_initialization (§5).
    pub async fn resend_init(&self, id: &str) -> Result<(), ZitadelError> {
        let url = format!("{}/management/v1/users/{}/_resend_initialization", self.cfg.issuer, id);
        self.post_json(&url, &json!({})).await.map(|_| ())
    }

    pub async fn deactivate(&self, id: &str) -> Result<(), ZitadelError> { self.lifecycle(id, "_deactivate").await }
    pub async fn reactivate(&self, id: &str) -> Result<(), ZitadelError> { self.lifecycle(id, "_reactivate").await }
    pub async fn lock(&self, id: &str) -> Result<(), ZitadelError> { self.lifecycle(id, "_lock").await }
    pub async fn unlock(&self, id: &str) -> Result<(), ZitadelError> { self.lifecycle(id, "_unlock").await }

    async fn lifecycle(&self, id: &str, verb: &str) -> Result<(), ZitadelError> {
        let url = format!("{}/management/v1/users/{}/{}", self.cfg.issuer, id, verb);
        self.post_json(&url, &json!({})).await.map(|_| ())
    }

    /// IRREVERSIBLE delete (§3.6): DELETE /management/v1/users/{id}.
    pub async fn delete_user(&self, id: &str) -> Result<(), ZitadelError> {
        let url = format!("{}/management/v1/users/{}", self.cfg.issuer, id);
        self.delete(&url).await.map(|_| ())
    }
}
