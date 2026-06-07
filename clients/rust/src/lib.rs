//! llm-chat client (Rust) — a faithful port of `clients/python`.
//!
//! - `auth`     — resolve credentials (flags/env/secrets) and mint a Zitadel
//!                JWT-bearer access token (machine user).
//! - `oidc`     — Authorization Code + PKCE human login (browser).
//! - `tokens`   — keyring + file token cache, interop with the Python client.
//! - `protocol` — `ChatClient`, the async `/chat` WebSocket session.
//! - `render`   — markdown → terminal (auto/plain/raw).
//! - `repl`     — interactive multi-turn REPL.
//! - `cli`      — the `llm-chat` command.

pub mod auth;
pub mod cli;
pub mod config;
pub mod errors;
pub mod oidc;
pub mod protocol;
pub mod render;
pub mod repl;
pub mod tokens;
