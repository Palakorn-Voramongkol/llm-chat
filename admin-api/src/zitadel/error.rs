//! Zitadel client error type + pure HTTP-status mapping.
//! Mirrors the appendix §3 gRPC->HTTP table that provision.py relies on:
//!   ALREADY_EXISTS->409, NOT_FOUND->404, PERMISSION_DENIED->403,
//!   INVALID_ARGUMENT/FAILED_PRECONDITION->400, 5xx->Upstream.

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ZitadelError {
    Upstream,
    NotFound,
    Forbidden,
    AlreadyExists,
    Invalid(String),
    Transport(String),
}

impl std::fmt::Display for ZitadelError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ZitadelError::Upstream => write!(f, "upstream zitadel error"),
            ZitadelError::NotFound => write!(f, "not found"),
            ZitadelError::Forbidden => write!(f, "forbidden"),
            ZitadelError::AlreadyExists => write!(f, "already exists"),
            ZitadelError::Invalid(m) => write!(f, "invalid: {m}"),
            ZitadelError::Transport(m) => write!(f, "transport: {m}"),
        }
    }
}

impl std::error::Error for ZitadelError {}

/// PURE: map an upstream HTTP status (+ raw body for context) to a typed
/// error. `body` is carried into `Invalid` so 400s surface Zitadel's message.
pub fn map_status(status: u16, body: &str) -> ZitadelError {
    match status {
        409 => ZitadelError::AlreadyExists,
        404 => ZitadelError::NotFound,
        403 => ZitadelError::Forbidden,
        400 => ZitadelError::Invalid(body.to_string()),
        500..=599 => ZitadelError::Upstream,
        other => ZitadelError::Invalid(format!("unexpected status {other}: {body}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_known_statuses() {
        assert_eq!(map_status(409, ""), ZitadelError::AlreadyExists);
        assert_eq!(map_status(404, ""), ZitadelError::NotFound);
        assert_eq!(map_status(403, ""), ZitadelError::Forbidden);
        assert_eq!(map_status(500, ""), ZitadelError::Upstream);
        assert_eq!(map_status(503, ""), ZitadelError::Upstream);
    }

    #[test]
    fn maps_400_carries_body() {
        assert_eq!(
            map_status(400, "bad role key"),
            ZitadelError::Invalid("bad role key".into())
        );
    }

    #[test]
    fn unknown_status_is_invalid_with_body() {
        assert_eq!(
            map_status(418, "teapot"),
            ZitadelError::Invalid("unexpected status 418: teapot".into())
        );
    }
}
