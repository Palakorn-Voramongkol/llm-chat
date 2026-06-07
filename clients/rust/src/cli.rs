//! Command-line entry point. Port of `cli.py`.
//!
//! Subcommands: ask (one-shot, machine auth) / chat (REPL, human login) /
//! login / logout / whoami. Bare `llm-chat` → chat.

use std::sync::Arc;
use std::time::Duration;

use base64::Engine;
use clap::{Parser, Subcommand};
use serde_json::{Map, Value};

use crate::auth::{fetch_access_token, read_secret_file, resolve_credentials};
use crate::config::{configure_logging, resolve_manager, AuthMode, CommonArgs, DEFAULT_ISSUER};
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

// ---------------- credential resolution (precedence: explicit > env > secrets) ----------------

fn resolve_issuer(c: &CommonArgs) -> String {
    c.issuer
        .clone()
        .or_else(|| std::env::var("ZITADEL_ISSUER").ok())
        .unwrap_or_else(|| DEFAULT_ISSUER.to_string())
}

fn resolve_project(c: &CommonArgs) -> Result<String> {
    c.project
        .clone()
        .or_else(|| std::env::var("PROJECT_ID").ok())
        .or_else(|| read_secret_file("project_id"))
        .filter(|s| !s.is_empty())
        .ok_or_else(|| {
            Error::Credential("no project id: pass --project or run the compose stack".into())
        })
}

fn resolve_client_id(c: &CommonArgs) -> Result<String> {
    c.oidc_client_id
        .clone()
        .or_else(|| std::env::var("OIDC_CLIENT_ID").ok())
        .or_else(|| read_secret_file("oidc_client_id"))
        .filter(|s| !s.is_empty())
        .ok_or_else(|| {
            Error::Credential(
                "no OIDC client id: run the compose stack so secrets/oidc_client_id exists, \
                 or pass --oidc-client-id"
                    .into(),
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
    let issuer = resolve_issuer(c);
    let client_id = resolve_client_id(c)?;
    let project = resolve_project(c)?;
    let store = TokenStore::new(&issuer, &client_id);
    let ts = oidc::login(&issuer, &client_id, &project, c.oidc_port, true, Duration::from_secs(300))?;
    store.save(&ts);
    print_whoami(&ts);
    Ok(0)
}

fn cmd_logout(c: &CommonArgs) -> Result<u8> {
    let issuer = resolve_issuer(c);
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
    let issuer = resolve_issuer(c);
    let client_id = resolve_client_id(c)?;
    let store = TokenStore::new(&issuer, &client_id);
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

// ---------------- chat / ask ----------------

fn build_provider(c: &CommonArgs, mode: AuthMode) -> Result<TokenProvider> {
    match mode {
        AuthMode::Machine => {
            let creds = resolve_credentials(
                c.issuer.as_deref(),
                c.project.as_deref(),
                c.key_file.as_deref(),
            )?;
            Ok(Arc::new(move || fetch_access_token(&creds)))
        }
        AuthMode::User => {
            let issuer = resolve_issuer(c);
            let client_id = resolve_client_id(c)?;
            let project = resolve_project(c)?;
            let store = TokenStore::new(&issuer, &client_id);
            let endpoints = oidc::discover(&issuer);

            // Ensure logged in (browser) before connecting.
            let ep = endpoints.token.clone();
            let cid = client_id.clone();
            let ok = store
                .valid_access_token(|rt| oidc::refresh(&ep, &cid, rt))
                .is_ok();
            if !ok {
                println!("Not logged in — starting browser login…");
                let ts = oidc::login(
                    &issuer,
                    &client_id,
                    &project,
                    c.oidc_port,
                    true,
                    Duration::from_secs(300),
                )?;
                store.save(&ts);
            }

            let store = Arc::new(store);
            let token_ep = endpoints.token.clone();
            let cid2 = client_id.clone();
            Ok(Arc::new(move || {
                let ep = token_ep.clone();
                let cid = cid2.clone();
                store.valid_access_token(move |rt| oidc::refresh(&ep, &cid, rt))
            }))
        }
    }
}

fn run_session(c: &CommonArgs, send: Option<String>) -> Result<u8> {
    let is_chat = send.is_none();
    let mode = c
        .auth
        .unwrap_or(if is_chat { AuthMode::User } else { AuthMode::Machine });
    let manager_url = resolve_manager(&c.manager);
    let render_mode = resolve_mode(c.plain, c.raw);
    let timeout = Duration::from_secs_f64(c.timeout);

    let provider = build_provider(c, mode)?;

    let rt = tokio::runtime::Runtime::new()
        .map_err(|e| Error::ManagerUnavailable(format!("could not start runtime: {e}")))?;
    rt.block_on(async move {
        let mut client = ChatClient::new(&manager_url, provider);
        match send {
            Some(q) => {
                client.connect().await?;
                let answer = client.ask(&q, timeout).await?;
                println!("Q: {q}");
                if render_mode == RenderMode::Raw {
                    println!("A: {}", answer.text);
                } else {
                    println!("A:");
                    render_markdown(&answer.text, render_mode);
                }
                client.close().await;
                Ok(0u8)
            }
            None => {
                let code = run_repl(&mut client, timeout, render_mode).await;
                client.close().await;
                Ok(code as u8)
            }
        }
    })
}

// ---------------- dispatch ----------------

/// Parse args, run the command, return the process exit code.
pub fn run() -> u8 {
    let cli = Cli::parse();
    // Bare `llm-chat` → chat (matches cli.py main()).
    let command = match cli.command {
        Some(cmd) => cmd,
        None => Cli::parse_from(["llm-chat", "chat"]).command.expect("chat"),
    };

    configure_logging(command.common().verbose);

    let result: Result<u8> = match &command {
        Command::Login { common } => cmd_login(common),
        Command::Logout { common } => cmd_logout(common),
        Command::Whoami { common } => cmd_whoami(common),
        Command::Ask { common, send } => run_session(common, Some(send.clone())),
        Command::Chat { common } => run_session(common, None),
    };

    match result {
        Ok(code) => code,
        Err(e) => {
            eprintln!("{}: {e}", e.prefix());
            e.exit_code()
        }
    }
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
