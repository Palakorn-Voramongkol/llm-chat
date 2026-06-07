//! Startup configuration for admin-api — env-driven, validated fail-fast.
//! Mirrors the manager/worker `require_*` contract: a pure helper that trims
//! and names any missing/empty var, plus `AdminConfig::from_env` that resolves
//! every required var up front so a misconfig aborts before any side effect.

/// PURE: require a non-empty config var. Trims surrounding whitespace.
/// Returns Err("{name} must be set (no default)") when None/empty/whitespace-
/// only; Ok(trimmed) otherwise. Modeled on manager::require_addr.
pub fn require_var(name: &str, raw: Option<String>) -> Result<String, String> {
    match raw.map(|s| s.trim().to_string()).filter(|s| !s.is_empty()) {
        Some(v) => Ok(v),
        None => Err(format!("{name} must be set (no default)")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn require_var_trims_and_accepts() {
        assert_eq!(require_var("X", Some("  v  ".into())), Ok("v".into()));
    }

    #[test]
    fn require_var_rejects_none() {
        assert_eq!(
            require_var("ZITADEL_ISSUER", None),
            Err("ZITADEL_ISSUER must be set (no default)".into())
        );
    }

    #[test]
    fn require_var_rejects_whitespace_only() {
        assert_eq!(
            require_var("SA_KEY_PATH", Some("   ".into())),
            Err("SA_KEY_PATH must be set (no default)".into())
        );
    }
}
