//! Credential resolution and Zitadel JWT-bearer token minting. Port of `auth.py`.
//!
//! Resolution precedence per credential: explicit value > environment var >
//! the compose stack's `secrets/` directory — so the CLI runs flagless when the
//! stack is up. HTTP is synchronous (`reqwest::blocking`).

use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use jsonwebtoken::{encode, Algorithm, EncodingKey, Header};
use serde::Serialize;

use crate::errors::{Error, Result};

#[derive(Debug, Clone)]
pub struct Credentials {
    pub issuer: String,
    pub project: String,
    pub key_file: String,
}

/// Resolve each connection credential from an explicit value or the process env
/// (the env is fed by `.env.local`). SOLE-SOURCED — no `secrets/` fallback and
/// no hardcoded default: a missing value fails closed with a message naming the
/// env var to set in `.env.local`. The key file's *contents* still live in
/// `secrets/`; `KABYTECH_KEY` is just the path to it.
pub fn resolve_credentials(
    issuer: Option<&str>,
    project: Option<&str>,
    key_file: Option<&str>,
) -> Result<Credentials> {
    let issuer = issuer
        .map(str::to_string)
        .or_else(|| std::env::var("ZITADEL_ISSUER").ok())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| {
            Error::Credential("no issuer: pass --issuer or set ZITADEL_ISSUER in .env.local".into())
        })?;

    let project = project
        .map(str::to_string)
        .or_else(|| std::env::var("PROJECT_ID").ok())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| {
            Error::Credential("no project id: pass --project or set PROJECT_ID in .env.local".into())
        })?;

    let key_file = key_file
        .map(str::to_string)
        .or_else(|| std::env::var("KABYTECH_KEY").ok())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| {
            Error::Credential(
                "no machine-user key: pass --key-file or set KABYTECH_KEY in .env.local".into(),
            )
        })?;
    if !Path::new(&key_file).exists() {
        return Err(Error::Credential(format!("key file not found: {key_file}")));
    }

    tracing::debug!(issuer = %issuer, project = %project, key_file = %key_file, "resolved credentials");
    Ok(Credentials { issuer, project, key_file })
}

#[derive(Serialize)]
struct Assertion<'a> {
    iss: &'a str,
    sub: &'a str,
    aud: &'a str,
    iat: u64,
    exp: u64,
}

/// Sign a JWT-bearer assertion with the machine key and exchange it for an
/// access token (a JWT the manager validates via JWKS).
pub fn fetch_access_token(creds: &Credentials) -> Result<String> {
    let ttl: u64 = 300;
    // Read the machine-user key file: {userId, keyId, key (PEM)}.
    let key: serde_json::Value = std::fs::read_to_string(&creds.key_file)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .ok_or_else(|| {
            Error::Credential(format!("invalid machine-user key file {}", creds.key_file))
        })?;
    let (user_id, key_id, private_key) = match (
        key.get("userId").and_then(|v| v.as_str()),
        key.get("keyId").and_then(|v| v.as_str()),
        key.get("key").and_then(|v| v.as_str()),
    ) {
        (Some(u), Some(k), Some(p)) => (u, k, p),
        _ => {
            return Err(Error::Credential(format!(
                "invalid machine-user key file {}: missing userId/keyId/key",
                creds.key_file
            )))
        }
    };

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let claims = Assertion {
        iss: user_id,
        sub: user_id,
        aud: &creds.issuer,
        iat: now,
        exp: now + ttl,
    };
    let mut header = Header::new(Algorithm::RS256);
    header.kid = Some(key_id.to_string());
    let enc_key = EncodingKey::from_rsa_pem(private_key.as_bytes())
        .map_err(|e| Error::Credential(format!("invalid machine-user key file {}: {e}", creds.key_file)))?;
    let assertion = encode(&header, &claims, &enc_key)
        .map_err(|e| Error::Credential(format!("could not sign assertion: {e}")))?;

    let token_url = format!("{}/oauth/v2/token", creds.issuer.trim_end_matches('/'));
    let scope = format!(
        "openid profile urn:zitadel:iam:org:project:id:{}:aud \
         urn:zitadel:iam:org:projects:roles",
        creds.project
    );
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .map_err(|e| Error::Auth(format!("http client error: {e}")))?;
    let resp = client
        .post(&token_url)
        .form(&[
            ("grant_type", "urn:ietf:params:oauth:grant-type:jwt-bearer"),
            ("scope", &scope),
            ("assertion", &assertion),
        ])
        .send()
        .map_err(|e| Error::Auth(format!("could not reach the token endpoint {token_url}: {e}")))?;
    let status = resp.status();
    let text = resp.text().unwrap_or_default();
    if status.as_u16() != 200 {
        let snippet: String = text.chars().take(300).collect();
        return Err(Error::Auth(format!(
            "token endpoint returned {}: {snippet} (issuer reachable? project/key correct?)",
            status.as_u16()
        )));
    }
    let token = serde_json::from_str::<serde_json::Value>(&text)
        .ok()
        .and_then(|v| v.get("access_token").and_then(|t| t.as_str()).map(String::from))
        .ok_or_else(|| {
            let snippet: String = text.chars().take(300).collect();
            Error::Auth(format!("token response had no access_token: {snippet}"))
        })?;
    tracing::debug!("minted access token len={}", token.len());
    Ok(token)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    // These env vars stand in for what `.env.local` loads into the process env
    // (the SOLE source) — they are NOT a fallback. The binary never sets them;
    // these tests only assert resolution reads the env and that an explicit
    // --flag overrides it. Serialize, since std::env is process-global.
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    fn clear_env() {
        for v in ["ZITADEL_ISSUER", "PROJECT_ID", "KABYTECH_KEY"] {
            std::env::remove_var(v);
        }
    }

    fn temp_key() -> std::path::PathBuf {
        let p = std::env::temp_dir().join("llm_chat_test_key.json");
        std::fs::write(&p, "{}").unwrap();
        p
    }

    #[test]
    fn explicit_args_win_over_env() {
        let _g = ENV_LOCK.lock().unwrap();
        clear_env();
        std::env::set_var("ZITADEL_ISSUER", "http://env:8080");
        let kf = temp_key();
        let creds = resolve_credentials(Some("http://x:8080"), Some("p1"), kf.to_str()).unwrap();
        assert_eq!(creds.issuer, "http://x:8080");
        assert_eq!(creds.project, "p1");
        assert_eq!(creds.key_file, kf.to_str().unwrap());
        clear_env();
    }

    #[test]
    fn reads_from_env_no_secrets_fallback() {
        let _g = ENV_LOCK.lock().unwrap();
        clear_env();
        let kf = temp_key();
        std::env::set_var("ZITADEL_ISSUER", "http://env:8080");
        std::env::set_var("PROJECT_ID", "penv");
        std::env::set_var("KABYTECH_KEY", kf.to_str().unwrap());
        let creds = resolve_credentials(None, None, None).unwrap();
        assert_eq!(creds.issuer, "http://env:8080");
        assert_eq!(creds.project, "penv");
        clear_env();
    }

    #[test]
    fn fails_closed_when_issuer_absent() {
        let _g = ENV_LOCK.lock().unwrap();
        clear_env();
        let kf = temp_key();
        let err = resolve_credentials(None, Some("p1"), kf.to_str()).unwrap_err();
        assert!(format!("{err}").contains("no issuer"));
        clear_env();
    }

    #[test]
    fn fails_closed_when_key_path_missing() {
        let _g = ENV_LOCK.lock().unwrap();
        clear_env();
        let err =
            resolve_credentials(Some("http://x:8080"), Some("p1"), Some("/no/such/key.json"))
                .unwrap_err();
        assert!(format!("{err}").contains("key file not found"));
        clear_env();
    }
}
