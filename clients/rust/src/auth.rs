//! Credential resolution and Zitadel JWT-bearer token minting. Port of `auth.py`.
//!
//! Resolution precedence per credential: explicit value > environment var >
//! the compose stack's `secrets/` directory — so the CLI runs flagless when the
//! stack is up. HTTP is synchronous (`reqwest::blocking`).

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use jsonwebtoken::{encode, Algorithm, EncodingKey, Header};
use serde::Serialize;

use crate::config::DEFAULT_ISSUER;
use crate::errors::{Error, Result};

/// `secrets/` at the repo root (relative to this crate at compile time),
/// overridable for non-standard layouts / tests via LLM_CHAT_SECRETS_DIR.
/// Mirrors `auth._SECRETS_DIR` (computed from the source location) + the
/// `secrets_dir()` env override.
pub fn secrets_dir() -> PathBuf {
    if let Ok(o) = std::env::var("LLM_CHAT_SECRETS_DIR") {
        if !o.is_empty() {
            return PathBuf::from(o);
        }
    }
    // <repo>/clients/rust/../../secrets == <repo>/secrets
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("secrets")
}

pub fn read_secret_file(name: &str) -> Option<String> {
    std::fs::read_to_string(secrets_dir().join(name))
        .ok()
        .map(|s| s.trim().to_string())
}

#[derive(Debug, Clone)]
pub struct Credentials {
    pub issuer: String,
    pub project: String,
    pub key_file: String,
}

/// Fill in any missing credential from env / the secrets dir.
pub fn resolve_credentials(
    issuer: Option<&str>,
    project: Option<&str>,
    key_file: Option<&str>,
) -> Result<Credentials> {
    let issuer = issuer
        .map(|s| s.to_string())
        .or_else(|| std::env::var("ZITADEL_ISSUER").ok())
        .unwrap_or_else(|| DEFAULT_ISSUER.to_string());

    let project = project
        .map(|s| s.to_string())
        .or_else(|| std::env::var("PROJECT_ID").ok())
        .or_else(|| read_secret_file("project_id"))
        .filter(|s| !s.is_empty());
    let project = match project {
        Some(p) => p,
        None => {
            return Err(Error::Credential(format!(
                "no project id: pass --project, set PROJECT_ID, or run the compose \
                 stack so {} exists",
                secrets_dir().join("project_id").display()
            )))
        }
    };

    let mut key_file = key_file
        .map(|s| s.to_string())
        .or_else(|| std::env::var("KABYTECH_KEY").ok())
        .filter(|s| !s.is_empty());
    if key_file.is_none() {
        let candidate = secrets_dir().join("kabytech-key.json");
        if candidate.exists() {
            key_file = Some(candidate.to_string_lossy().into_owned());
        }
    }
    let key_file = match key_file {
        Some(k) => k,
        None => {
            return Err(Error::Credential(format!(
                "no machine-user key: pass --key-file, set KABYTECH_KEY, or run the \
                 compose stack so {} exists",
                secrets_dir().join("kabytech-key.json").display()
            )))
        }
    };
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
