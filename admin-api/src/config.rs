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

/// Resolved, validated admin-api configuration. Every field is required —
/// there is no code default (the manager/worker pattern). `from_env`/`from_map`
/// fail fast naming the first missing var.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AdminConfig {
    pub issuer: String,
    pub project_id: String,
    pub audience: String,
    pub sa_key_path: String,
    pub oidc_client_id: String,
    pub oidc_client_secret: String,
    pub bind_addr: String,
    pub public_origin: String,
    pub allowed_origin: String,
    pub session_key: String,
    /// Optional manager /control WS URL (e.g. ws://manager:7777/control) for the
    /// Sessions page's chat-sessions panel. OPTIONAL BY DESIGN (a capability-gated
    /// feature toggle, not a security value): absent → the panel reports
    /// `configured:false` and stays dark. No default URL is invented.
    pub manager_control_url: Option<String>,
}

impl AdminConfig {
    /// PURE: resolve every required var from a lookup fn. `issuer` and
    /// `public_origin` are trailing-slash-trimmed to match
    /// zitadel_auth::ZitadelConfig and so the startup discovery issuer-match
    /// guard (Task 13) and the OIDC redirect_uri compare like-for-like.
    pub fn from_map(get: &dyn Fn(&str) -> Option<String>) -> Result<AdminConfig, String> {
        let issuer = require_var("ZITADEL_ISSUER", get("ZITADEL_ISSUER"))?
            .trim_end_matches('/')
            .to_string();
        let public_origin = require_var("ADMIN_PUBLIC_ORIGIN", get("ADMIN_PUBLIC_ORIGIN"))?
            .trim_end_matches('/')
            .to_string();
        Ok(AdminConfig {
            issuer,
            project_id: require_var("ZITADEL_PROJECT_ID", get("ZITADEL_PROJECT_ID"))?,
            audience: require_var("ZITADEL_AUDIENCE", get("ZITADEL_AUDIENCE"))?,
            sa_key_path: require_var("ADMIN_SA_KEY_PATH", get("ADMIN_SA_KEY_PATH"))?,
            oidc_client_id: require_var("ADMIN_OIDC_CLIENT_ID", get("ADMIN_OIDC_CLIENT_ID"))?,
            oidc_client_secret: require_var(
                "ADMIN_OIDC_CLIENT_SECRET",
                get("ADMIN_OIDC_CLIENT_SECRET"),
            )?,
            bind_addr: require_var("ADMIN_BIND_ADDR", get("ADMIN_BIND_ADDR"))?,
            public_origin,
            allowed_origin: require_var("ADMIN_ALLOWED_ORIGIN", get("ADMIN_ALLOWED_ORIGIN"))?,
            session_key: require_var("ADMIN_SESSION_KEY", get("ADMIN_SESSION_KEY"))?,
            manager_control_url: get("MANAGER_CONTROL_URL")
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty()),
        })
    }

    /// Thin wrapper: resolve from the real process environment.
    pub fn from_env() -> Result<AdminConfig, String> {
        Self::from_map(&|k| std::env::var(k).ok())
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

    use std::collections::HashMap;

    fn getter(m: HashMap<&'static str, &'static str>) -> impl Fn(&str) -> Option<String> {
        move |k: &str| m.get(k).map(|s| s.to_string())
    }

    fn full_map() -> HashMap<&'static str, &'static str> {
        HashMap::from([
            ("ZITADEL_ISSUER", "http://host.docker.internal:8080/"),
            ("ZITADEL_PROJECT_ID", "p1"),
            ("ZITADEL_AUDIENCE", "p1"),
            ("ADMIN_SA_KEY_PATH", "/secrets/admin-api-key.json"),
            ("ADMIN_OIDC_CLIENT_ID", "cid"),
            ("ADMIN_OIDC_CLIENT_SECRET", "csecret"),
            ("ADMIN_BIND_ADDR", "0.0.0.0:7676"),
            ("ADMIN_PUBLIC_ORIGIN", "http://localhost:7676/"),
            ("ADMIN_ALLOWED_ORIGIN", "http://localhost:3000"),
            ("ADMIN_SESSION_KEY", "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"),
        ])
    }

    #[test]
    fn from_map_ok_trims_issuer_and_public_origin_slash() {
        let cfg = AdminConfig::from_map(&getter(full_map())).expect("ok");
        assert_eq!(cfg.issuer, "http://host.docker.internal:8080");
        assert_eq!(cfg.public_origin, "http://localhost:7676");
        assert_eq!(cfg.project_id, "p1");
        assert_eq!(cfg.bind_addr, "0.0.0.0:7676");
    }

    #[test]
    fn from_map_names_first_missing_var() {
        let mut m = full_map();
        m.remove("ADMIN_OIDC_CLIENT_SECRET");
        assert_eq!(
            AdminConfig::from_map(&getter(m)),
            Err("ADMIN_OIDC_CLIENT_SECRET must be set (no default)".into())
        );
    }
}
