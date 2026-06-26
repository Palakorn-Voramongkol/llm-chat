//! Command-line entry point. Port of `cli.py`.
//!
//! Subcommands: ask (one-shot, machine auth) / chat (REPL, human login) /
//! login / logout / whoami. Bare `llm-chat` → chat.

use std::sync::Arc;
use std::time::Duration;

use base64::Engine;
use clap::{Parser, Subcommand};
use serde_json::{Map, Value};

use crate::auth::{fetch_access_token, resolve_credentials};
use crate::config::{configure_logging, load_env_local, resolve_manager, AuthMode, CommonArgs};
use crate::errors::{Error, Result, EXIT_AUTH};
use crate::oidc::{self, TokenSet};
use crate::protocol::{ChatClient, TokenProvider};
use crate::render::{render_markdown, resolve_mode, RenderMode};
use crate::repl::run_repl;
use crate::tokens::TokenStore;

#[derive(Parser, Debug)]
#[command(
    name = "llm-chat",
    version = "1.0.0",
    about = "Client for the llm-chat manager's /chat WebSocket."
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// send one question and print the answer (machine auth)
    Ask {
        #[command(flatten)]
        common: CommonArgs,
        /// the question text
        #[arg(long)]
        send: String,
    },
    /// interactive multi-turn REPL (human login)
    Chat {
        #[command(flatten)]
        common: CommonArgs,
    },
    /// browser sign-in; cache the session
    Login {
        #[command(flatten)]
        common: CommonArgs,
    },
    /// revoke and clear the cached session
    Logout {
        #[command(flatten)]
        common: CommonArgs,
    },
    /// show the cached user identity
    Whoami {
        #[command(flatten)]
        common: CommonArgs,
    },
}

impl Command {
    fn common(&self) -> &CommonArgs {
        match self {
            Command::Ask { common, .. }
            | Command::Chat { common }
            | Command::Login { common }
            | Command::Logout { common }
            | Command::Whoami { common } => common,
        }
    }
}

// ------- credential resolution (sole source: explicit flag > env, from .env.local) -------

fn resolve_issuer(c: &CommonArgs) -> Result<String> {
    c.issuer
        .clone()
        .or_else(|| std::env::var("ZITADEL_ISSUER").ok())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| {
            Error::Credential("no issuer: pass --issuer or set ZITADEL_ISSUER in .env.local".into())
        })
}

fn resolve_project(c: &CommonArgs) -> Result<String> {
    c.project
        .clone()
        .or_else(|| std::env::var("PROJECT_ID").ok())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| {
            Error::Credential("no project id: pass --project or set PROJECT_ID in .env.local".into())
        })
}

fn resolve_client_id(c: &CommonArgs) -> Result<String> {
    c.oidc_client_id
        .clone()
        .or_else(|| std::env::var("OIDC_CLIENT_ID").ok())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| {
            Error::Credential(
                "no OIDC client id: pass --oidc-client-id or set OIDC_CLIENT_ID in .env.local".into(),
            )
        })
}

// ---------------- jwt display (no verification) ----------------

fn decode_claims(token: &str) -> Map<String, Value> {
    let parts: Vec<&str> = token.split('.').collect();
    if parts.len() < 2 {
        return Map::new();
    }
    let mut payload = parts[1].to_string();
    while payload.len() % 4 != 0 {
        payload.push('=');
    }
    base64::engine::general_purpose::URL_SAFE
        .decode(payload.as_bytes())
        .ok()
        .and_then(|b| serde_json::from_slice::<Value>(&b).ok())
        .and_then(|v| v.as_object().cloned())
        .unwrap_or_default()
}

fn print_whoami(ts: &TokenSet) {
    let token = ts
        .id_token
        .as_deref()
        .filter(|s| !s.is_empty())
        .unwrap_or(&ts.access_token);
    let claims = decode_claims(token);
    let sub = claims.get("sub").and_then(|v| v.as_str()).unwrap_or("?");
    let who = claims
        .get("email")
        .and_then(|v| v.as_str())
        .or_else(|| claims.get("preferred_username").and_then(|v| v.as_str()))
        .unwrap_or(sub);
    let mut roles: Vec<String> = Vec::new();
    for (k, v) in &claims {
        if k.ends_with(":roles") {
            if let Some(obj) = v.as_object() {
                roles.extend(obj.keys().cloned());
            }
        }
    }
    println!("logged in as {who} (sub={sub})");
    if !roles.is_empty() {
        roles.sort();
        roles.dedup();
        println!("  roles: {}", roles.join(", "));
    }
}

// ---------------- subcommands ----------------

fn cmd_login(c: &CommonArgs) -> Result<u8> {
    let (issuer, client_id, project, store, _endpoints) = user_session(c)?;
    let ts = login_and_store(&issuer, &client_id, &project, &store, c.oidc_port)?;
    print_whoami(&ts);
    Ok(0)
}

fn cmd_logout(c: &CommonArgs) -> Result<u8> {
    let issuer = resolve_issuer(c)?;
    let client_id = resolve_client_id(c)?;
    let store = TokenStore::new(&issuer, &client_id);
    if let Some(ts) = store.load() {
        if let Some(rt) = ts.refresh_token {
            let endpoints = oidc::discover(&issuer);
            oidc::revoke(&endpoints.revoke, &client_id, &rt);
        }
    }
    store.clear();
    println!("logged out.");
    Ok(0)
}

fn cmd_whoami(c: &CommonArgs) -> Result<u8> {
    let (_issuer, _client_id, _project, store, _endpoints) = user_session(c)?;
    match store.load() {
        Some(ts) => {
            print_whoami(&ts);
            Ok(0)
        }
        None => {
            eprintln!("not logged in — run `llm-chat login`");
            Ok(EXIT_AUTH)
        }
    }
}

// ---------------- credential mode + providers ----------------

fn auth_mode(c: &CommonArgs, is_chat: bool) -> AuthMode {
    c.auth
        .unwrap_or(if is_chat { AuthMode::User } else { AuthMode::Machine })
}

/// Machine (kabytech) token provider: mint a JWT-bearer access token per call.
fn machine_provider(c: &CommonArgs) -> Result<TokenProvider> {
    let creds = resolve_credentials(
        c.issuer.as_deref(),
        c.project.as_deref(),
        c.key_file.as_deref(),
    )?;
    Ok(Arc::new(move || fetch_access_token(&creds)))
}

/// (issuer, client_id, project, store, endpoints) for the human path.
fn user_session(c: &CommonArgs) -> Result<(String, String, String, TokenStore, oidc::Endpoints)> {
    let issuer = resolve_issuer(c)?;
    let client_id = resolve_client_id(c)?;
    let project = resolve_project(c)?;
    let endpoints = oidc::discover(&issuer);
    let store = TokenStore::new(&issuer, &client_id);
    Ok((issuer, client_id, project, store, endpoints))
}

/// Human token provider: a cached access token, refreshed on demand.
fn user_provider(store: TokenStore, token_endpoint: String, client_id: String) -> TokenProvider {
    let store = Arc::new(store);
    Arc::new(move || {
        let ep = token_endpoint.clone();
        let cid = client_id.clone();
        store.valid_access_token(move |rt| oidc::refresh(&ep, &cid, rt))
    })
}

fn login_and_store(
    issuer: &str,
    client_id: &str,
    project: &str,
    store: &TokenStore,
    port: u16,
) -> Result<TokenSet> {
    let ts = oidc::login(issuer, client_id, project, port, true, Duration::from_secs(300))?;
    store.save(&ts);
    Ok(ts)
}

// ---------------- run loops ----------------

fn cmd_chat_or_ask(c: &CommonArgs, send: Option<String>) -> Result<u8> {
    let mode = auth_mode(c, send.is_none());
    let manager_url = resolve_manager(&c.manager)?;
    let render_mode = resolve_mode(c.plain, c.raw);
    let timeout = Duration::from_secs_f64(c.timeout);

    let provider = match mode {
        AuthMode::Machine => machine_provider(c)?,
        AuthMode::User => {
            // ensure logged in (browser) before connecting
            let (issuer, client_id, project, store, endpoints) = user_session(c)?;
            let ep = endpoints.token.clone();
            let cid = client_id.clone();
            if store
                .valid_access_token(|rt| oidc::refresh(&ep, &cid, rt))
                .is_err()
            {
                println!("Not logged in — starting browser login…");
                login_and_store(&issuer, &client_id, &project, &store, c.oidc_port)?;
            }
            user_provider(store, endpoints.token.clone(), client_id)
        }
    };

    let rt = tokio::runtime::Runtime::new()
        .map_err(|e| Error::ManagerUnavailable(format!("could not start runtime: {e}")))?;
    match send {
        Some(q) => rt.block_on(run_ask(provider, &manager_url, &q, timeout, render_mode)),
        None => rt.block_on(run_chat(provider, &manager_url, timeout, render_mode)),
    }
}

async fn run_ask(
    provider: TokenProvider,
    manager_url: &str,
    send: &str,
    timeout: Duration,
    render_mode: RenderMode,
) -> Result<u8> {
    let mut client = ChatClient::new(manager_url, provider);
    client.connect().await?;
    let answer = client.ask(send, timeout).await?;
    println!("Q: {send}");
    if render_mode == RenderMode::Raw {
        println!("A: {}", answer.text);
    } else {
        println!("A:");
        render_markdown(&answer.text, render_mode);
    }
    client.close().await;
    Ok(0)
}

async fn run_chat(
    provider: TokenProvider,
    manager_url: &str,
    timeout: Duration,
    render_mode: RenderMode,
) -> Result<u8> {
    let mut client = ChatClient::new(manager_url, provider);
    let code = run_repl(&mut client, timeout, render_mode).await;
    client.close().await;
    Ok(code as u8)
}

// ---------------- dispatch ----------------

fn dispatch(command: Command) -> u8 {
    configure_logging(command.common().verbose);
    let result: Result<u8> = match &command {
        Command::Login { common } => cmd_login(common),
        Command::Logout { common } => cmd_logout(common),
        Command::Whoami { common } => cmd_whoami(common),
        Command::Ask { common, send } => cmd_chat_or_ask(common, Some(send.clone())),
        Command::Chat { common } => cmd_chat_or_ask(common, None),
    };
    match result {
        Ok(code) => code,
        Err(e) => {
            eprintln!("{}: {e}", e.prefix());
            e.exit_code()
        }
    }
}

/// Parse args (bare `llm-chat` → chat, like cli.py `main`) and run the chosen
/// command. Returns the process exit code.
pub fn main() -> u8 {
    // Sole-source connection settings from the repo-root .env.local (real env
    // vars and --flags still win; missing values fail closed during resolution).
    load_env_local();
    let cli = Cli::parse();
    let command = match cli.command {
        Some(cmd) => cmd,
        None => Cli::parse_from(["llm-chat", "chat"]).command.expect("chat"),
    };
    dispatch(command)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decode_claims_reads_payload() {
        // {"sub":"u1","email":"a@b.c"} as a fake JWT (header.payload.sig).
        let payload = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .encode(br#"{"sub":"u1","email":"a@b.c"}"#);
        let token = format!("h.{payload}.s");
        let claims = decode_claims(&token);
        assert_eq!(claims.get("sub").unwrap().as_str().unwrap(), "u1");
        assert_eq!(claims.get("email").unwrap().as_str().unwrap(), "a@b.c");
    }

    #[test]
    fn decode_claims_bad_token_is_empty() {
        assert!(decode_claims("not-a-jwt").is_empty());
        assert!(decode_claims("").is_empty());
    }

    #[test]
    fn cli_parses_subcommands() {
        // Smoke: the parser accepts each subcommand + the shared flags.
        for argv in [
            vec!["llm-chat", "whoami"],
            vec!["llm-chat", "login"],
            vec!["llm-chat", "logout"],
            vec!["llm-chat", "chat", "--plain"],
            vec!["llm-chat", "ask", "--send", "hi", "--raw"],
        ] {
            assert!(Cli::try_parse_from(argv).is_ok());
        }
        // ask requires --send
        assert!(Cli::try_parse_from(["llm-chat", "ask"]).is_err());
    }
}
