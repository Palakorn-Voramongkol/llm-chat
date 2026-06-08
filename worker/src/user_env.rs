//! Per-user Claude working-environment path confinement (design
//! 2026-06-09). Fail closed: any missing/invalid/unprovable input rejects.
//! Pure helpers (no I/O) live here with the one filesystem resolver
//! (resolve_user_cwd) so the security logic is unit-testable.

use std::path::{Path, PathBuf};

/// Why a per-user cwd could not be resolved. Every variant means "reject,
/// do not spawn" — there is no fallback.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolveError {
    BadUser(String),
    BadPath(String),
    Escape(String),
    Io(String),
}

impl std::fmt::Display for ResolveError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ResolveError::BadUser(m) => write!(f, "bad user id: {m}"),
            ResolveError::BadPath(m) => write!(f, "bad path: {m}"),
            ResolveError::Escape(m) => write!(f, "path escapes user dir: {m}"),
            ResolveError::Io(m) => write!(f, "io: {m}"),
        }
    }
}

/// PURE: require the env base. REQUIRED — no default. Trims; Err naming the
/// var when None/empty/whitespace. Mirrors worker_bind_addr's contract.
pub fn require_user_env_base(raw: Option<String>) -> Result<PathBuf, String> {
    match raw.map(|s| s.trim().to_string()).filter(|s| !s.is_empty()) {
        Some(v) => Ok(PathBuf::from(v)),
        None => Err(
            "LLM_CHAT_USER_ENV_BASE must be set (no default) — the per-user \
             Claude environment root".to_string(),
        ),
    }
}

fn valid_user_id(user_id: &str) -> bool {
    !user_id.is_empty()
        && user_id.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

/// PURE (no I/O): validate `user_id` and the client `subpath`, and return the
/// LEXICAL candidate `base/user_id/<components…>`. Rejects an empty/illegal
/// user id, and any subpath that is absolute or contains `..`, `.`, an empty
/// component, `\`, `:`, or NUL. None/empty subpath → the user root.
pub fn confine_path(
    base: &Path,
    user_id: &str,
    subpath: Option<&str>,
) -> Result<PathBuf, ResolveError> {
    if !valid_user_id(user_id) {
        return Err(ResolveError::BadUser(format!("{user_id:?}")));
    }
    let mut out = base.join(user_id);
    let raw = subpath.unwrap_or("").trim();
    if raw.is_empty() {
        return Ok(out);
    }
    if raw.starts_with('/') {
        return Err(ResolveError::BadPath("absolute path not allowed".into()));
    }
    for comp in raw.split('/') {
        if comp.is_empty() || comp == "." || comp == ".." {
            return Err(ResolveError::BadPath(format!("illegal component {comp:?}")));
        }
        if comp.contains('\\') || comp.contains(':') || comp.contains('\0') {
            return Err(ResolveError::BadPath(format!("illegal char in {comp:?}")));
        }
        out.push(comp);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn require_base_rejects_missing() {
        assert!(require_user_env_base(None).unwrap_err().contains("LLM_CHAT_USER_ENV_BASE"));
        assert!(require_user_env_base(Some("   ".into())).unwrap_err().contains("LLM_CHAT_USER_ENV_BASE"));
    }
    #[test]
    fn require_base_trims_and_accepts() {
        assert_eq!(require_user_env_base(Some("  /srv/envs  ".into())).unwrap(), PathBuf::from("/srv/envs"));
    }

    fn base() -> PathBuf { PathBuf::from("/srv/envs") }

    #[test]
    fn confine_none_subpath_is_user_root() {
        assert_eq!(confine_path(&base(), "u1", None).unwrap(), PathBuf::from("/srv/envs/u1"));
        assert_eq!(confine_path(&base(), "u1", Some("")).unwrap(), PathBuf::from("/srv/envs/u1"));
    }
    #[test]
    fn confine_nested_service_subpath() {
        assert_eq!(
            confine_path(&base(), "311867081814147073", Some("crm/acct-42")).unwrap(),
            PathBuf::from("/srv/envs/311867081814147073/crm/acct-42"),
        );
    }
    #[test]
    fn confine_rejects_bad_user() {
        assert!(matches!(confine_path(&base(), "", None), Err(ResolveError::BadUser(_))));
        assert!(matches!(confine_path(&base(), "..", None), Err(ResolveError::BadUser(_))));
        assert!(matches!(confine_path(&base(), "a/b", None), Err(ResolveError::BadUser(_))));
        assert!(matches!(confine_path(&base(), "a b", None), Err(ResolveError::BadUser(_))));
    }
    #[test]
    fn confine_rejects_traversal_and_absolute() {
        assert!(matches!(confine_path(&base(), "u1", Some("../x")), Err(ResolveError::BadPath(_))));
        assert!(matches!(confine_path(&base(), "u1", Some("a/../../b")), Err(ResolveError::BadPath(_))));
        assert!(matches!(confine_path(&base(), "u1", Some("/etc")), Err(ResolveError::BadPath(_))));
        assert!(matches!(confine_path(&base(), "u1", Some("a/./b")), Err(ResolveError::BadPath(_))));
    }
    #[test]
    fn confine_rejects_windows_and_nul() {
        assert!(matches!(confine_path(&base(), "u1", Some("a\\b")), Err(ResolveError::BadPath(_))));
        assert!(matches!(confine_path(&base(), "u1", Some("C:")), Err(ResolveError::BadPath(_))));
        assert!(matches!(confine_path(&base(), "u1", Some("a\0b")), Err(ResolveError::BadPath(_))));
    }
}
