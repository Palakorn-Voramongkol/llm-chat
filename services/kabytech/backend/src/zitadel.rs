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

    /// POST /v2/users/human → create the invited user (emails the invite link).
    pub async fn create_invited_user(
        &self, token: &str, email: &str, given: &str, family: &str, accept_base: &str,
    ) -> Result<String, String> {
        let resp = self
            .http
            .post(format!("{}/v2/users/human", self.issuer))
            .bearer_auth(token)
            .json(&invite_user_body(email, given, family, accept_base))
            .send()
            .await
            .map_err(|e| format!("create user: {e}"))?;
        let status = resp.status();
        let body: Value = resp.json().await.map_err(|e| format!("create user json: {e}"))?;
        if status.is_success() {
            return body["userId"].as_str().map(String::from).ok_or_else(|| "no userId".into());
        }
        if status.as_u16() == 409 {
            return Err("a user with that email already exists".into());
        }
        Err(format!("create user returned {status}: {body}"))
    }

    /// Grant exactly chat.user on the chat project (v1 mgmt grant). 409 == ok.
    pub async fn grant_chat_user(&self, token: &str, user_id: &str) -> Result<(), String> {
        let resp = self
            .http
            .post(format!("{}/management/v1/users/{}/grants", self.issuer, user_id))
            .bearer_auth(token)
            .json(&json!({ "projectId": self.project_id, "roleKeys": ["chat.user"] }))
            .send()
            .await
            .map_err(|e| format!("grant: {e}"))?;
        if resp.status().is_success() || resp.status().as_u16() == 409 {
            Ok(())
        } else {
            Err(format!("grant returned {}", resp.status()))
        }
    }

    /// Verify the emailed code (proves email ownership). v2:
    /// POST /v2/users/{id}/email/verify { verificationCode }.
    pub async fn verify_email(&self, token: &str, user_id: &str, code: &str) -> Result<(), String> {
        let resp = self
            .http
            .post(format!("{}/v2/users/{}/email/verify", self.issuer, user_id))
            .bearer_auth(token)
            .json(&json!({ "verificationCode": code }))
            .send()
            .await
            .map_err(|e| format!("verify email: {e}"))?;
        if resp.status().is_success() {
            Ok(())
        } else {
            Err("invalid or expired invite code".into())
        }
    }

    /// Set the user's password (SA-authorized; valid while the user is Initial).
    /// v2: POST /v2/users/{id}/password (the v1 management PUT is gone in v3).
    pub async fn set_password(&self, token: &str, user_id: &str, password: &str) -> Result<(), String> {
        let resp = self
            .http
            .post(format!("{}/v2/users/{}/password", self.issuer, user_id))
            .bearer_auth(token)
            .json(&set_password_body(password))
            .send()
            .await
            .map_err(|e| format!("set password: {e}"))?;
        if resp.status().is_success() {
            Ok(())
        } else {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            Err(format!("set password returned {status}: {body}"))
        }
    }
}

/// PURE: the v2 create-human body for an INVITE — email with a sendCode
/// urlTemplate pointing at kabytech /accept; NO password (set on accept).
pub fn invite_user_body(email: &str, given: &str, family: &str, accept_base: &str) -> Value {
    let tmpl = format!(
        "{accept_base}/accept?userID={{{{.UserID}}}}&code={{{{.Code}}}}&orgID={{{{.OrgID}}}}"
    );
    json!({
        "username": email,
        "profile": { "givenName": given, "familyName": family },
        "email": { "email": email, "sendCode": { "urlTemplate": tmpl } },
    })
}

/// PURE: the v1 set-password body (no forced change).
pub fn set_password_body(password: &str) -> Value {
    json!({ "newPassword": { "password": password, "changeRequired": false } })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_assertion_rejects_bad_pem() {
        let err = build_assertion("u", "k", "not a pem", "http://iss", 0).unwrap_err();
        assert!(err.to_lowercase().contains("pem") || err.to_lowercase().contains("key"));
    }

    #[test]
    fn invite_user_body_carries_email_and_accept_url_template() {
        let b = invite_user_body("a@b.c", "Ada", "Lovelace", "http://localhost:3001");
        assert_eq!(b["username"], "a@b.c");
        assert_eq!(b["profile"]["givenName"], "Ada");
        assert_eq!(b["email"]["email"], "a@b.c");
        let tmpl = b["email"]["sendCode"]["urlTemplate"].as_str().unwrap();
        assert!(tmpl.starts_with("http://localhost:3001/accept?userID="));
        assert!(tmpl.contains("{{.UserID}}") && tmpl.contains("{{.Code}}") && tmpl.contains("{{.OrgID}}"));
        assert!(b.get("password").is_none()); // invite-only: no password at creation
    }

    #[test]
    fn set_password_body_shape() {
        let b = set_password_body("hunter2");
        assert_eq!(b["newPassword"]["password"], "hunter2");
        assert_eq!(b["newPassword"]["changeRequired"], false);
    }
}
