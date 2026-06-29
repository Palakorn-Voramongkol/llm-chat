//! Startup configuration for kabytech-backend — env-driven, validated fail-fast.
//! Ported from admin-api/src/config.rs (trimmed for the gateway login MVP).

/// PURE: require a non-empty config var (trims surrounding whitespace).
pub fn require_var(name: &str, raw: Option<String>) -> Result<String, String> {
    match raw.map(|s| s.trim().to_string()).filter(|s| !s.is_empty()) {
        Some(v) => Ok(v),
        None => Err(format!("{name} must be set (no default)")),
    }
}

/// PURE: parse the session-cookie `Secure` toggle. Secure BY DEFAULT; only an
/// explicit false/0/no (a plain-HTTP dev stack) disables it. Fail closed.
pub fn parse_cookie_secure(raw: Option<String>) -> bool {
    !matches!(
        raw.map(|s| s.trim().to_ascii_lowercase()).as_deref(),
        Some("false") | Some("0") | Some("no")
    )
}

/// Resolved, validated kabytech-backend config. Every field required.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KabyConfig {
    pub issuer: String,
    pub project_id: String,
    pub audience: String,
    pub oidc_client_id: String,
    pub oidc_client_secret: String,
    pub bind_addr: String,
    /// The FRONTEND origin (e.g. http://localhost:3001). The OIDC redirect_uri is
    /// {public_origin}/callback so the browser lands on the proxied frontend and
    /// keeps the SameSite=Lax cookie.
    pub public_origin: String,
    pub allowed_origin: String,
    pub session_key: String,
    /// Path to the kabytech-login SA JSON key (jwt-bearer → Management token for
    /// invite/accept; Phase 2 Session API). Required.
    pub sa_key_path: String,
    pub cookie_secure: bool,
    /// Optional manager /provision WS URL (e.g. ws://manager:7777/provision). When
    /// set, first-login provisions the user's app sandbox (best-effort). Absent →
    /// the feature is off and login is unaffected.
    pub manager_provision_url: Option<String>,
    /// The app code this gateway provisions under (default "kabytech").
    pub app_code: String,
}

impl KabyConfig {
    pub fn from_map(get: &dyn Fn(&str) -> Option<String>) -> Result<KabyConfig, String> {
        let issuer = require_var("ZITADEL_ISSUER", get("ZITADEL_ISSUER"))?
            .trim_end_matches('/')
            .to_string();
        let public_origin = require_var("KABY_PUBLIC_ORIGIN", get("KABY_PUBLIC_ORIGIN"))?
            .trim_end_matches('/')
            .to_string();
        Ok(KabyConfig {
            issuer,
            project_id: require_var("ZITADEL_PROJECT_ID", get("ZITADEL_PROJECT_ID"))?,
            audience: require_var("ZITADEL_AUDIENCE", get("ZITADEL_AUDIENCE"))?,
            oidc_client_id: require_var("KABY_OIDC_CLIENT_ID", get("KABY_OIDC_CLIENT_ID"))?,
            oidc_client_secret: require_var("KABY_OIDC_CLIENT_SECRET", get("KABY_OIDC_CLIENT_SECRET"))?,
            bind_addr: require_var("KABY_BIND_ADDR", get("KABY_BIND_ADDR"))?,
            public_origin,
            allowed_origin: require_var("KABY_ALLOWED_ORIGIN", get("KABY_ALLOWED_ORIGIN"))?,
            session_key: require_var("KABY_SESSION_KEY", get("KABY_SESSION_KEY"))?,
            sa_key_path: require_var("KABY_SA_KEY_PATH", get("KABY_SA_KEY_PATH"))?,
            cookie_secure: parse_cookie_secure(get("KABY_COOKIE_SECURE")),
            manager_provision_url: get("KABY_MANAGER_PROVISION_URL")
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty()),
            app_code: get("KABY_APP_CODE")
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| "kabytech".to_string()),
        })
    }

    pub fn from_env() -> Result<KabyConfig, String> {
        Self::from_map(&|k| std::env::var(k).ok())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn getter(m: HashMap<&'static str, &'static str>) -> impl Fn(&str) -> Option<String> {
        move |k: &str| m.get(k).map(|s| s.to_string())
    }

    fn full_map() -> HashMap<&'static str, &'static str> {
        HashMap::from([
            ("ZITADEL_ISSUER", "http://host.docker.internal:8080/"),
            ("ZITADEL_PROJECT_ID", "p1"),
            ("ZITADEL_AUDIENCE", "p1"),
            ("KABY_OIDC_CLIENT_ID", "cid"),
            ("KABY_OIDC_CLIENT_SECRET", "csecret"),
            ("KABY_BIND_ADDR", "0.0.0.0:7670"),
            ("KABY_PUBLIC_ORIGIN", "http://localhost:3001/"),
            ("KABY_ALLOWED_ORIGIN", "http://localhost:3001"),
            ("KABY_SESSION_KEY", "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"),
            ("KABY_SA_KEY_PATH", "/secrets/kabytech-login-key.json"),
        ])
    }

    #[test]
    fn require_var_trims_and_rejects_empty() {
        assert_eq!(require_var("X", Some("  v  ".into())), Ok("v".into()));
        assert_eq!(require_var("X", None), Err("X must be set (no default)".into()));
        assert_eq!(require_var("X", Some("  ".into())), Err("X must be set (no default)".into()));
    }

    #[test]
    fn cookie_secure_defaults_true_only_falsey_disables() {
        assert!(parse_cookie_secure(None));
        assert!(parse_cookie_secure(Some("anything".into())));
        assert!(!parse_cookie_secure(Some("false".into())));
        assert!(!parse_cookie_secure(Some("0".into())));
        assert!(!parse_cookie_secure(Some(" NO ".into())));
    }

    #[test]
    fn from_map_ok_trims_issuer_and_public_origin() {
        let cfg = KabyConfig::from_map(&getter(full_map())).expect("ok");
        assert_eq!(cfg.issuer, "http://host.docker.internal:8080");
        assert_eq!(cfg.public_origin, "http://localhost:3001");
        assert_eq!(cfg.project_id, "p1");
        assert_eq!(cfg.bind_addr, "0.0.0.0:7670");
    }

    #[test]
    fn from_map_names_first_missing_var() {
        let mut m = full_map();
        m.remove("KABY_OIDC_CLIENT_SECRET");
        assert_eq!(
            KabyConfig::from_map(&getter(m)),
            Err("KABY_OIDC_CLIENT_SECRET must be set (no default)".into())
        );
    }

    #[test]
    fn app_code_defaults_to_kabytech_and_provision_url_optional() {
        let cfg = KabyConfig::from_map(&getter(full_map())).expect("ok");
        assert_eq!(cfg.app_code, "kabytech");
        assert_eq!(cfg.manager_provision_url, None);
        let mut m = full_map();
        m.insert("KABY_APP_CODE", "other");
        m.insert("KABY_MANAGER_PROVISION_URL", "ws://manager:7777/provision");
        let cfg = KabyConfig::from_map(&getter(m)).expect("ok");
        assert_eq!(cfg.app_code, "other");
        assert_eq!(cfg.manager_provision_url.as_deref(), Some("ws://manager:7777/provision"));
    }
}
