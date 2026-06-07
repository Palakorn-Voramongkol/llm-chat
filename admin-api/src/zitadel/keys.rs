//! Machine-key (jwt-profile) + client-secret (client_credentials) wrappers.
//! Two independent credential lifecycles per machine user (appendix §3.5).
//! keyDetails (the private key) is returned ONLY by create_json_key.
//! Verify response keys (userId/keyDetails/clientSecret/result[]) against the
//! running Zitadel v3.4.10 — appendix §6.3/§6.4/§6.6 (Task 19), not asserted here.

use serde_json::{json, Value};

use super::error::ZitadelError;
use super::ZitadelClient;

impl ZitadelClient {
    /// Create a JSON key: POST /users/{id}/keys {type:KEY_TYPE_JSON}.
    /// Returns the FULL create response — keyDetails (base64 SA JSON) is here ONCE.
    pub async fn create_json_key(&self, user_id: &str) -> Result<Value, ZitadelError> {
        let url = format!("{}/management/v1/users/{}/keys", self.cfg.issuer, user_id);
        self.post_json(&url, &json!({ "type": "KEY_TYPE_JSON" })).await
    }

    /// List keys (metadata only, no private key): POST /users/{id}/keys/_search.
    pub async fn list_keys(&self, user_id: &str) -> Result<Vec<Value>, ZitadelError> {
        let url = format!("{}/management/v1/users/{}/keys/_search", self.cfg.issuer, user_id);
        let v = self.post_json(&url, &json!({})).await?;
        Ok(v.get("result").and_then(Value::as_array).cloned().unwrap_or_default())
    }

    /// Delete (= revoke) a key: DELETE /users/{id}/keys/{keyId}.
    pub async fn delete_key(&self, user_id: &str, key_id: &str) -> Result<(), ZitadelError> {
        let url = format!("{}/management/v1/users/{}/keys/{}", self.cfg.issuer, user_id, key_id);
        self.delete(&url).await.map(|_| ())
    }

    /// Generate a client secret: PUT /users/{id}/secret. clientSecret shown ONCE.
    pub async fn generate_secret(&self, user_id: &str) -> Result<Value, ZitadelError> {
        let url = format!("{}/management/v1/users/{}/secret", self.cfg.issuer, user_id);
        self.put_json(&url, &json!({})).await
    }

    /// Remove the client secret: DELETE /users/{id}/secret.
    pub async fn delete_secret(&self, user_id: &str) -> Result<(), ZitadelError> {
        let url = format!("{}/management/v1/users/{}/secret", self.cfg.issuer, user_id);
        self.delete(&url).await.map(|_| ())
    }
}
