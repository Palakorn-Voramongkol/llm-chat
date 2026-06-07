//! Secure token cache. Port of `tokens.py`.
//!
//! The refresh token (long-lived) goes into the OS keyring (Windows Credential
//! Manager / macOS Keychain / Linux Secret Service). If no keyring backend is
//! available it degrades to a 0600 file. Short-lived access/id tokens live in a
//! 0600 `tokens.json` sidecar.
//!
//! Byte-for-byte compatible with the Python client: same keyring service
//! ("llm-chat") + user ("refresh:<issuer>"), same config-dir path (matching
//! platformdirs.user_config_dir("llm-chat")), same JSON schema keyed by issuer —
//! so a login made by either client is reused by the other.

use std::path::PathBuf;

use serde_json::{json, Map, Value};

use crate::errors::{Error, Result};
use crate::oidc::TokenSet;

const SERVICE: &str = "llm-chat";

/// `platformdirs.user_config_dir("llm-chat")`, honoring the LLM_CHAT_CONFIG_DIR
/// override. Created if missing.
pub fn config_dir() -> PathBuf {
    if let Ok(o) = std::env::var("LLM_CHAT_CONFIG_DIR") {
        if !o.is_empty() {
            let p = PathBuf::from(o);
            let _ = std::fs::create_dir_all(&p);
            return p;
        }
    }
    let p = platform_config_dir();
    let _ = std::fs::create_dir_all(&p);
    p
}

#[cfg(windows)]
fn platform_config_dir() -> PathBuf {
    // platformdirs: %LOCALAPPDATA%\<author>\<app> with author defaulting to app
    // → llm-chat\llm-chat.
    let base = dirs::data_local_dir().unwrap_or_else(|| PathBuf::from("."));
    base.join("llm-chat").join("llm-chat")
}

#[cfg(target_os = "macos")]
fn platform_config_dir() -> PathBuf {
    // ~/Library/Application Support/llm-chat
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
    home.join("Library").join("Application Support").join("llm-chat")
}

#[cfg(all(unix, not(target_os = "macos")))]
fn platform_config_dir() -> PathBuf {
    // $XDG_CONFIG_HOME/llm-chat or ~/.config/llm-chat
    if let Ok(xdg) = std::env::var("XDG_CONFIG_HOME") {
        if !xdg.is_empty() {
            return PathBuf::from(xdg).join("llm-chat");
        }
    }
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
    home.join(".config").join("llm-chat")
}

fn tokens_file() -> PathBuf {
    config_dir().join("tokens.json")
}

fn load_file() -> Map<String, Value> {
    match std::fs::read_to_string(tokens_file()) {
        Ok(s) => serde_json::from_str::<Value>(&s)
            .ok()
            .and_then(|v| v.as_object().cloned())
            .unwrap_or_default(),
        Err(_) => Map::new(),
    }
}

fn save_file(data: &Map<String, Value>) {
    let path = tokens_file();
    if std::fs::write(&path, Value::Object(data.clone()).to_string()).is_ok() {
        set_owner_only(&path);
    }
}

#[cfg(unix)]
fn set_owner_only(path: &std::path::Path) {
    use std::os::unix::fs::PermissionsExt;
    let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
}
#[cfg(not(unix))]
fn set_owner_only(_path: &std::path::Path) {}

/// Per-issuer token cache: refresh token in the keyring, access in a file.
pub struct TokenStore {
    issuer: String,
    kr_user: String,
    #[allow(dead_code)]
    client_id: String,
}

impl TokenStore {
    pub fn new(issuer: &str, client_id: &str) -> Self {
        TokenStore {
            issuer: issuer.to_string(),
            kr_user: format!("refresh:{issuer}"),
            client_id: client_id.to_string(),
        }
    }

    // ---- keyring, with graceful degradation ----
    fn kr_set(&self, refresh_token: &str) -> bool {
        match keyring::Entry::new(SERVICE, &self.kr_user).and_then(|e| e.set_password(refresh_token)) {
            Ok(()) => true,
            Err(e) => {
                tracing::warn!("keyring unavailable ({e}) — storing refresh token in a 0600 file");
                false
            }
        }
    }
    fn kr_get(&self) -> Option<String> {
        keyring::Entry::new(SERVICE, &self.kr_user)
            .ok()?
            .get_password()
            .ok()
    }
    fn kr_del(&self) {
        if let Ok(e) = keyring::Entry::new(SERVICE, &self.kr_user) {
            let _ = e.delete_credential();
        }
    }

    // ---- public API ----
    pub fn save(&self, ts: &TokenSet) {
        let mut data = load_file();
        let mut entry = Map::new();
        entry.insert("access_token".into(), json!(ts.access_token));
        entry.insert("id_token".into(), json!(ts.id_token));
        entry.insert("expires_at".into(), json!(ts.expires_at));
        if let Some(rt) = &ts.refresh_token {
            if !self.kr_set(rt) {
                entry.insert("refresh_token".into(), json!(rt)); // file fallback
            }
        }
        data.insert(self.issuer.clone(), Value::Object(entry));
        save_file(&data);
    }

    pub fn load(&self) -> Option<TokenSet> {
        let data = load_file();
        let entry = data.get(&self.issuer)?.as_object()?;
        let refresh = self.kr_get().or_else(|| {
            entry
                .get("refresh_token")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
        });
        Some(TokenSet {
            access_token: entry
                .get("access_token")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
            refresh_token: refresh,
            id_token: entry
                .get("id_token")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string()),
            expires_at: entry.get("expires_at").and_then(|v| v.as_f64()).unwrap_or(0.0),
        })
    }

    pub fn clear(&self) {
        self.kr_del();
        let mut data = load_file();
        data.remove(&self.issuer);
        save_file(&data);
    }

    /// Return a non-expired access token, refreshing via `refresh_fn(refresh_token)`
    /// if needed. Errors when there's nothing usable.
    pub fn valid_access_token<F>(&self, refresh_fn: F) -> Result<String>
    where
        F: Fn(&str) -> Result<TokenSet>,
    {
        let ts = self.load();
        let ts = match ts {
            Some(t) if !t.access_token.is_empty() || t.refresh_token.is_some() => t,
            _ => return Err(Error::Auth("not logged in — run `llm-chat login`".into())),
        };
        if !ts.access_token.is_empty() && !ts.is_expired() {
            return Ok(ts.access_token);
        }
        if let Some(rt) = &ts.refresh_token {
            let new = refresh_fn(rt)?;
            self.save(&new);
            return Ok(new.access_token);
        }
        Err(Error::Auth("session expired — run `llm-chat login`".into()))
    }
}
