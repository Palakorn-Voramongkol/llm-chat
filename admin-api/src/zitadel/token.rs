//! SA JWT-bearer assertion + Management-API token cache (appendix §2.5).
//! `build_assertion` is the Rust mirror of provision.py:build_jwt_assertion;
//! the token mint uses grant_type=jwt-bearer with the `zitadel` scope trap.

use crate::zitadel::error::{map_status, ZitadelError};
use crate::zitadel::ZitadelClient;

/// A minted Management-API token + its absolute expiry (epoch seconds).
#[derive(Clone, Debug)]
pub struct CachedToken {
    pub token: String,
    pub exp: u64,
}

/// PURE: sign the JWT-bearer assertion. Header `{kid}`, claims
/// `{iss=sub=user_id, aud=issuer, iat=now, exp=now+3600}`, RS256 over `pem`.
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
    let claims = serde_json::json!({
        "iss": user_id,
        "sub": user_id,
        "aud": issuer,
        "iat": now,
        "exp": now + 3600,
    });
    let key = EncodingKey::from_rsa_pem(pem.as_bytes())
        .map_err(|e| format!("bad SA key PEM: {e}"))?;
    encode(&header, &claims, &key).map_err(|e| format!("sign assertion: {e}"))
}

/// The `zitadel` literal targets Zitadel's own internal project so the
/// Management API accepts the minted token (appendix §2.5 scope trap).
pub const ADMIN_SCOPE: &str =
    "openid profile urn:zitadel:iam:org:project:id:zitadel:aud";

impl ZitadelClient {
    /// Return a valid Management token, minting (and caching) a fresh one if
    /// none is cached or the cached one expires within 60s.
    pub async fn valid_token(&self) -> Result<String, ZitadelError> {
        let now = now_secs();
        if let Some(t) = self.token.read().await.as_ref() {
            if t.exp > now + 60 {
                return Ok(t.token.clone());
            }
        }
        let fresh = self.mint_management_token().await?;
        let tok = fresh.token.clone();
        *self.token.write().await = Some(fresh);
        Ok(tok)
    }

    /// Mint a Management-API token via the JWT-bearer grant (appendix §2.5).
    pub async fn mint_management_token(&self) -> Result<CachedToken, ZitadelError> {
        let raw = std::fs::read_to_string(&self.cfg.sa_key_path)
            .map_err(|e| ZitadelError::Transport(format!("read sa key: {e}")))?;
        let sa: serde_json::Value = serde_json::from_str(&raw)
            .map_err(|e| ZitadelError::Invalid(format!("sa key json: {e}")))?;
        let user_id = sa["userId"].as_str().unwrap_or_default();
        let key_id = sa["keyId"].as_str().unwrap_or_default();
        let pem = sa["key"].as_str().unwrap_or_default();
        let now = now_secs();
        let assertion = build_assertion(user_id, key_id, pem, &self.cfg.issuer, now)
            .map_err(ZitadelError::Invalid)?;

        let url = format!("{}/oauth/v2/token", self.cfg.issuer);
        let resp = self
            .http
            .post(&url)
            .form(&[
                ("grant_type", "urn:ietf:params:oauth:grant-type:jwt-bearer"),
                ("assertion", assertion.as_str()),
                ("scope", ADMIN_SCOPE),
            ])
            .send()
            .await
            .map_err(|e| ZitadelError::Transport(e.to_string()))?;
        let status = resp.status().as_u16();
        let body = resp.text().await.unwrap_or_default();
        if status != 200 {
            return Err(map_status(status, &body));
        }
        let json: serde_json::Value = serde_json::from_str(&body)
            .map_err(|e| ZitadelError::Invalid(format!("token json: {e}")))?;
        let token = json["access_token"]
            .as_str()
            .ok_or_else(|| ZitadelError::Invalid("no access_token in mint response".into()))?
            .to_string();
        let ttl = json["expires_in"].as_u64().unwrap_or(3000);
        Ok(CachedToken { token, exp: now + ttl })
    }
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use jsonwebtoken::{decode, Algorithm, DecodingKey, EncodingKey, Validation};

    const TEST_PRIV_PEM: &str = include_str!("testdata/test_rsa_priv.pem");

    #[test]
    fn build_assertion_round_trips() {
        let now = 1_700_000_000u64;
        let jwt = build_assertion("user-1", "kid-9", TEST_PRIV_PEM, "http://iss", now)
            .expect("sign ok");

        let _enc = EncodingKey::from_rsa_pem(TEST_PRIV_PEM.as_bytes()).unwrap();
        let pub_pem = include_str!("testdata/test_rsa_pub.pem");
        let dk = DecodingKey::from_rsa_pem(pub_pem.as_bytes()).unwrap();
        let mut v = Validation::new(Algorithm::RS256);
        v.set_audience(&["http://iss"]);
        v.set_required_spec_claims(&["exp", "aud"]);
        // The assertion is signed with a fixed historical `now`, so its `exp` is
        // in the past relative to wall-clock time. This test verifies the RS256
        // signature + claim values (exp is asserted explicitly below), not that
        // the token is currently unexpired — disable wall-clock exp validation.
        v.validate_exp = false;
        let data = decode::<serde_json::Value>(&jwt, &dk, &v).expect("verify ok");
        assert_eq!(data.claims["iss"], "user-1");
        assert_eq!(data.claims["sub"], "user-1");
        assert_eq!(data.claims["aud"], "http://iss");
        assert_eq!(data.claims["iat"], now);
        assert_eq!(data.claims["exp"], now + 3600);

        let header = jsonwebtoken::decode_header(&jwt).unwrap();
        assert_eq!(header.kid.as_deref(), Some("kid-9"));
        assert_eq!(header.alg, Algorithm::RS256);
    }

    #[test]
    fn build_assertion_rejects_bad_pem() {
        let err = build_assertion("u", "k", "not a pem", "http://iss", 0).unwrap_err();
        assert!(err.to_lowercase().contains("pem") || err.to_lowercase().contains("key"));
    }
}
