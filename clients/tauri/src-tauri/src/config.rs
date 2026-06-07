//! Runtime configuration. Env vars (LUMINA_*) override; a repo `secrets/` dir is
//! a dev fallback for project id / oidc client id (same files the other clients
//! read).

use std::path::PathBuf;

pub const APP_NAME: &str = "Lumina";

fn env_or(key: &str, default: &str) -> String {
    std::env::var(key)
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| default.to_string())
}
fn env_opt(key: &str) -> Option<String> {
    std::env::var(key).ok().filter(|s| !s.is_empty())
}

fn secrets_dir() -> PathBuf {
    if let Some(d) = env_opt("LUMINA_SECRETS_DIR") {
        return PathBuf::from(d);
    }
    // <repo>/clients/tauri/src-tauri/../../../secrets == <repo>/secrets
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("..")
        .join("secrets")
}
fn read_secret(name: &str) -> Option<String> {
    std::fs::read_to_string(secrets_dir().join(name))
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

#[derive(Clone, serde::Serialize)]
pub struct Config {
    pub app_name: String,
    pub issuer: String,
    pub manager_ws: String,
    pub project: Option<String>,
    pub client_id: Option<String>,
    pub plantuml_server: String,
    pub required_role: String,
}

impl Config {
    pub fn load() -> Self {
        Config {
            app_name: APP_NAME.to_string(),
            issuer: env_or("LUMINA_ISSUER", "http://host.docker.internal:8080"),
            manager_ws: env_or("LUMINA_MANAGER_WS", "ws://127.0.0.1:7777/chat"),
            project: env_opt("LUMINA_PROJECT").or_else(|| read_secret("project_id")),
            client_id: env_opt("LUMINA_OIDC_CLIENT_ID").or_else(|| read_secret("oidc_client_id")),
            plantuml_server: env_or("LUMINA_PLANTUML_SERVER", "https://www.plantuml.com/plantuml"),
            required_role: env_or("LUMINA_REQUIRED_ROLE", "chat.app"),
        }
    }
}
