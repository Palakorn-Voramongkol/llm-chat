//! Error type for the llm-chat client.
//!
//! Mirrors the Python `errors.py` hierarchy: one enum the CLI maps to a clear
//! message + exit code. Exit codes and message prefixes match `cli.py`'s
//! dispatch exactly (CredentialError->2, AuthError->3, ManagerUnavailable->4,
//! ProtocolError/AnswerTimeout->5, KeyboardInterrupt->130).

use std::fmt;

#[derive(Debug)]
pub enum Error {
    /// A required credential (issuer / project / key file) is missing/unreadable.
    Credential(String),
    /// The Zitadel token exchange failed (network, bad key, rejected assertion).
    Auth(String),
    /// Could not establish/maintain the WebSocket connection to the manager.
    ManagerUnavailable(String),
    /// The manager sent something uninterpretable, or returned an `err` frame.
    Protocol(String),
    /// No answer arrived within the allotted time.
    AnswerTimeout(String),
}

// Exit codes (documented in the README; identical to the Python client).
pub const EXIT_OK: u8 = 0;
pub const EXIT_USAGE: u8 = 2;
pub const EXIT_AUTH: u8 = 3;
pub const EXIT_MANAGER: u8 = 4;
pub const EXIT_ANSWER: u8 = 5;
pub const EXIT_INTERRUPT: u8 = 130;

impl Error {
    pub fn exit_code(&self) -> u8 {
        match self {
            Error::Credential(_) => EXIT_USAGE,
            Error::Auth(_) => EXIT_AUTH,
            Error::ManagerUnavailable(_) => EXIT_MANAGER,
            Error::Protocol(_) | Error::AnswerTimeout(_) => EXIT_ANSWER,
        }
    }

    /// Stderr prefix, matching the Python dispatch's `print(f"...: {e}")`.
    pub fn prefix(&self) -> &'static str {
        match self {
            Error::Credential(_) => "error",
            Error::Auth(_) => "auth error",
            Error::ManagerUnavailable(_) => "manager unavailable",
            Error::Protocol(_) | Error::AnswerTimeout(_) => "error",
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let msg = match self {
            Error::Credential(m)
            | Error::Auth(m)
            | Error::ManagerUnavailable(m)
            | Error::Protocol(m)
            | Error::AnswerTimeout(m) => m,
        };
        f.write_str(msg)
    }
}

impl std::error::Error for Error {}

pub type Result<T> = std::result::Result<T, Error>;
