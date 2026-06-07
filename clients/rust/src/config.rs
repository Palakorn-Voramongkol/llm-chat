//! Shared CLI argument definitions and logging setup.
//!
//! Port of `config.py`. All credential flags default to None so the
//! precedence (explicit > env > secrets dir) is resolved in `auth`/`cli`, NOT
//! by clap's `env` (which would wrongly let env beat the secrets-file fallback).

use clap::Args;

/// Default Zitadel issuer when neither --issuer nor $ZITADEL_ISSUER is set.
/// Matches `auth.DEFAULT_ISSUER`.
pub const DEFAULT_ISSUER: &str = "http://host.docker.internal:8080";

/// Connection/auth/display flags shared by every subcommand (the Python
/// `add_common_args` "connection" + "display" groups plus -v/--verbose).
#[derive(Args, Debug, Clone)]
pub struct CommonArgs {
    /// Zitadel issuer URL (default: $ZITADEL_ISSUER or http://host.docker.internal:8080)
    #[arg(long, help_heading = "connection")]
    pub issuer: Option<String>,

    /// Zitadel project_id (default: $PROJECT_ID or secrets/project_id)
    #[arg(long, help_heading = "connection")]
    pub project: Option<String>,

    /// machine-user JSON key (default: $KABYTECH_KEY or secrets/kabytech-key.json)
    #[arg(long = "key-file", help_heading = "connection")]
    pub key_file: Option<String>,

    /// manager /chat WebSocket URL (default: $MANAGER_WS or ws://127.0.0.1:7777/chat)
    #[arg(long, help_heading = "connection")]
    pub manager: Option<String>,

    /// per-answer timeout in seconds (default: 120; high effort is slow)
    #[arg(long, default_value_t = 120.0, help_heading = "connection")]
    pub timeout: f64,

    /// credential type (default: chat->user, ask->machine)
    #[arg(long, value_enum, help_heading = "connection")]
    pub auth: Option<AuthMode>,

    /// OIDC client id (default: $OIDC_CLIENT_ID or secrets/oidc_client_id)
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

/// `manager or $MANAGER_WS or ws://127.0.0.1:7777/chat` — matches resolve_manager.
pub fn resolve_manager(manager: &Option<String>) -> String {
    manager
        .clone()
        .or_else(|| std::env::var("MANAGER_WS").ok())
        .unwrap_or_else(|| "ws://127.0.0.1:7777/chat".to_string())
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
