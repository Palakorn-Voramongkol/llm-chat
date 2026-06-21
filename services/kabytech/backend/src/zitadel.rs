//! kabytech's Zitadel client: SA JWT-bearer token mint + User-API calls for
//! invite/accept. Ported from admin-api/src/zitadel/{token,users}.rs.

use serde_json::{json, Value};

#[derive(Clone)]
pub struct Zitadel {
    pub http: std::sync::Arc<reqwest::Client>,
    pub issuer: String,
    pub project_id: String,
    pub sa_key_path: String,
}

/// The `zitadel` literal targets Zitadel's internal project so the Management
/// API accepts the minted token (the admin-api §2.5 scope trap).
const ADMIN_SCOPE: &str = "openid profile urn:zitadel:iam:org:project:id:zitadel:aud";

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// PURE: sign the JWT-bearer assertion (RS256, header kid, iss=sub=user_id).
pub fn build_assertion(
    user_id: &str,
    key_id: &str,
    pem: &str,
    issuer: &str,
    now: u64,
) -> Result<String, String> {
    use jsonwebtoken::{encode, Algorithm, EncodingKey, Header};
    let mut header = Header::new(Algorithm::RS256);
    header.kid = Some(key_id.to_string());
    let claims = json!({ "iss": user_id, "sub": user_id, "aud": issuer, "iat": now, "exp": now + 3600 });
    let key = EncodingKey::from_rsa_pem(pem.as_bytes()).map_err(|e| format!("bad SA key PEM: {e}"))?;
    encode(&header, &claims, &key).map_err(|e| format!("sign assertion: {e}"))
}

impl Zitadel {
    /// Mint a Management-API token from the SA JSON key (jwt-bearer).
    pub async fn mint_token(&self) -> Result<String, String> {
        let raw = std::fs::read_to_string(&self.sa_key_path).map_err(|e| format!("read sa key: {e}"))?;
        let sa: Value = serde_json::from_str(&raw).map_err(|e| format!("sa key json: {e}"))?;
        let assertion = build_assertion(
            sa["userId"].as_str().unwrap_or_default(),
            sa["keyId"].as_str().unwrap_or_default(),
            sa["key"].as_str().unwrap_or_default(),
            &self.issuer,
            now_secs(),
        )?;
        let resp = self
            .http
            .post(format!("{}/oauth/v2/token", self.issuer))
            .form(&[
                ("grant_type", "urn:ietf:params:oauth:grant-type:jwt-bearer"),
                ("assertion", assertion.as_str()),
                ("scope", ADMIN_SCOPE),
            ])
            .send()
            .await
            .map_err(|e| format!("token endpoint: {e}"))?;
        if !resp.status().is_success() {
            return Err(format!("token mint returned {}", resp.status()));
        }
        let j: Value = resp.json().await.map_err(|e| format!("token json: {e}"))?;
        j["access_token"].as_str().map(String::from).ok_or_else(|| "no access_token".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_assertion_rejects_bad_pem() {
        let err = build_assertion("u", "k", "not a pem", "http://iss", 0).unwrap_err();
        assert!(err.to_lowercase().contains("pem") || err.to_lowercase().contains("key"));
    }
}
