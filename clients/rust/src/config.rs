//! Shared CLI argument definitions, `.env.local` loading, and logging setup.
//!
//! Port of `config.py`. All credential flags default to None; the precedence
//! (explicit flag > env var, the env fed by `.env.local`) is resolved in
//! `auth`/`cli`. Connection settings are SOLE-SOURCED from `.env.local`: no
//! `secrets/` fallback and no hardcoded default — a missing value fails closed.

use std::path::{Path, PathBuf};

use clap::Args;

use crate::errors::{Error, Result};

/// Load connection settings from the repo-root `.env.local` into the process
/// environment. dotenvy does NOT override already-set vars, so an explicit
/// shell env var (or a `--flag`, resolved separately) still wins. A missing
/// file is fine — resolution then fails closed on whatever value is absent.
/// Override the path via `LLM_CHAT_ENV_FILE`.
pub fn load_env_local() {
    let path = std::env::var("LLM_CHAT_ENV_FILE")
        .ok()
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            // <repo>/clients/rust/../../.env.local == <repo>/.env.local
            Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..").join(".env.local")
        });
    let _ = dotenvy::from_path(&path);
}

/// Connection/auth/display flags shared by every subcommand (the Python
/// `add_common_args` "connection" + "display" groups plus -v/--verbose).
#[derive(Args, Debug, Clone)]
pub struct CommonArgs {
    /// Zitadel issuer URL (required: --issuer or ZITADEL_ISSUER in .env.local)
    #[arg(long, help_heading = "connection")]
    pub issuer: Option<String>,

    /// Zitadel project_id (required: --project or PROJECT_ID in .env.local)
    #[arg(long, help_heading = "connection")]
    pub project: Option<String>,

    /// machine-user JSON key (required: --key-file or KABYTECH_KEY in .env.local)
    #[arg(long = "key-file", help_heading = "connection")]
    pub key_file: Option<String>,

    /// manager /chat WebSocket URL (required: --manager or MANAGER_WS in .env.local)
    #[arg(long, help_heading = "connection")]
    pub manager: Option<String>,

    /// per-answer timeout in seconds (default: 120; high effort is slow)
    #[arg(long, default_value_t = 120.0, help_heading = "connection")]
    pub timeout: f64,

    /// credential type (default: chat->user, ask->machine)
    #[arg(long, value_enum, help_heading = "connection")]
    pub auth: Option<AuthMode>,

    /// OIDC client id (required: --oidc-client-id or OIDC_CLIENT_ID in .env.local)
    #[arg(long = "oidc-client-id", help_heading = "connection")]
    pub oidc_client_id: Option<String>,

    /// loopback port for the browser login redirect (default: 8477)
    #[arg(long = "oidc-port", default_value_t = 8477, help_heading = "connection")]
    pub oidc_port: u16,

    /// render markdown as plain text (no ANSI color/styling)
    #[arg(long, help_heading = "display")]
    pub plain: bool,

    /// print claude's literal markdown without rendering
    #[arg(long, help_heading = "display")]
    pub raw: bool,

    /// -v for INFO, -vv for DEBUG diagnostics on stderr
    #[arg(short, long, action = clap::ArgAction::Count)]
    pub verbose: u8,
}

#[derive(clap::ValueEnum, Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthMode {
    User,
    Machine,
}

/// `--manager` or `$MANAGER_WS` (from `.env.local`) — fail closed, no default.
pub fn resolve_manager(manager: &Option<String>) -> Result<String> {
    manager
        .clone()
        .or_else(|| std::env::var("MANAGER_WS").ok())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| {
            Error::Credential(
                "no manager URL: pass --manager or set MANAGER_WS in .env.local \
                 (e.g. ws://127.0.0.1:7777/chat)"
                    .into(),
            )
        })
}

/// Derive the `/identity` URL from the manager `/chat` URL: same scheme +
/// host:port, path replaced with `/identity` (the manager serves both).
pub fn identity_url(manager_ws: &str) -> String {
    match manager_ws.split_once("://") {
        Some((scheme, rest)) => {
            let authority = rest.split('/').next().unwrap_or(rest);
            format!("{scheme}://{authority}/identity")
        }
        None => manager_ws.to_string(),
    }
}

/// Map -v/-vv to a tracing filter. Diagnostics go to stderr so they don't mix
/// with the chat transcript on stdout. Idempotent across calls.
pub fn configure_logging(verbosity: u8) {
    use tracing_subscriber::EnvFilter;
    let level = match verbosity {
        0 => "warn",
        1 => "info",
        _ => "debug",
    };
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(level));
    let _ = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(std::io::stderr)
        .with_target(true)
        .try_init();
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    // resolve_manager reads $MANAGER_WS; serialize the env-touching tests.
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn resolve_manager_explicit_flag_wins() {
        let _g = ENV_LOCK.lock().unwrap();
        std::env::set_var("MANAGER_WS", "ws://env:7777/chat");
        assert_eq!(
            resolve_manager(&Some("ws://flag:7777/chat".to_string())).unwrap(),
            "ws://flag:7777/chat"
        );
        std::env::remove_var("MANAGER_WS");
    }

    #[test]
    fn resolve_manager_reads_env() {
        let _g = ENV_LOCK.lock().unwrap();
        std::env::set_var("MANAGER_WS", "ws://env:7777/chat");
        assert_eq!(resolve_manager(&None).unwrap(), "ws://env:7777/chat");
        std::env::remove_var("MANAGER_WS");
    }

    #[test]
    fn resolve_manager_fails_closed_when_absent() {
        let _g = ENV_LOCK.lock().unwrap();
        std::env::remove_var("MANAGER_WS");
        let err = resolve_manager(&None).unwrap_err();
        assert!(format!("{err}").contains("no manager URL"));
    }

    #[test]
    fn identity_url_swaps_path() {
        assert_eq!(identity_url("ws://127.0.0.1:7777/chat"), "ws://127.0.0.1:7777/identity");
        assert_eq!(identity_url("wss://host.example:443/chat"), "wss://host.example:443/identity");
        assert_eq!(identity_url("ws://h:7777"), "ws://h:7777/identity");
    }
}
