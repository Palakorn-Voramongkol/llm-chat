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

/// PURE: true if any `/`-delimited segment of `url` is a `.`/`..` dot-segment.
///
/// Operator-supplied ids (user id, app id, grant id, key id, role key) are
/// interpolated raw into Zitadel API paths. axum's `Path` extractor has
/// already percent-decoded them, so a smuggled `..%2F..` arrives here as a
/// literal `../..`. reqwest's URL parser would then normalize those
/// dot-segments away and re-point the privileged service-account request at a
/// different API object/path (path traversal / request smuggling against the
/// Zitadel Management API). We reject BEFORE that parse, centrally in
/// `send_json`, so the boundary holds for every call site — present and future
/// — without each one having to remember to validate. Fail closed.
pub fn path_has_traversal(url: &str) -> bool {
    if url.split('/').any(|seg| seg == "." || seg == "..") {
        return true;
    }
    // We only ever build clean resource PATHS (no query/fragment) against the
    // pinned issuer host. A '?' or '#' here can only have come from a path-param
    // id that decoded to one — appending a query string or truncating the path
    // on the privileged service-account request. Zitadel ids are numeric
    // snowflakes and role keys are dotted alphanumerics, so these chars never
    // appear legitimately. Reject (fail closed, defense in depth).
    url.contains('?') || url.contains('#')
}

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
    fn traversal_detected_in_smuggled_id_segment() {
        // A decoded `../..` id re-points users/{id} at another API path.
        assert!(path_has_traversal(
            "https://id.example.com/management/v1/users/../../admin/v1/instance"
        ));
        assert!(path_has_traversal("https://id.example.com/v2/users/.."));
        assert!(path_has_traversal("https://id.example.com/v2/users/."));
    }

    #[test]
    fn legitimate_ids_are_not_flagged() {
        // Real Zitadel ids are opaque snowflakes; role keys are dotted tokens.
        assert!(!path_has_traversal("https://id.example.com/v2/users/290527061160763393"));
        assert!(!path_has_traversal(
            "https://id.example.com/management/v1/projects/p1/roles/chat.admin"
        ));
        // A dotted value that is not a bare dot-segment must pass.
        assert!(!path_has_traversal("https://id.example.com/v2/users/a..b"));
    }

    #[test]
    fn query_or_fragment_in_id_is_rejected() {
        // An id that decoded to `?`/`#` would append a query / truncate the
        // privileged path — we never build those, so reject.
        assert!(path_has_traversal("https://id.example.com/v2/users/x?foo=bar"));
        assert!(path_has_traversal("https://id.example.com/v2/users/x#frag"));
    }

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
