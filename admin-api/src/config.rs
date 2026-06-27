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

/// PURE: parse the session-cookie `Secure` toggle. Secure BY DEFAULT — the
/// cookie carries an operator session, so the safe value wins unless explicitly
/// disabled. Only a plain-HTTP local dev stack opts out with
/// ADMIN_COOKIE_SECURE=false (else the cookie is never sent over http and login
/// silently breaks). Absent/unrecognized → true (fail closed).
pub fn parse_cookie_secure(raw: Option<String>) -> bool {
    !matches!(
        raw.map(|s| s.trim().to_ascii_lowercase()).as_deref(),
        Some("false") | Some("0") | Some("no")
    )
}

/// One chat-capable application in the Sessions registry. `project_id` is the
/// Zitadel project whose audience the SA token must target (so that app's
/// manager accepts it) and whose roles it asserts.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SessionApp {
    pub key: String,
    pub name: String,
    pub control_url: String,
    pub project_id: String,
}

/// PURE: build the chat-app registry. Prefers `MANAGER_CONTROL_APPS` (a JSON
/// array of `{key,name,controlUrl,projectId}`); an entry missing any field is
/// dropped (never defaulted). Malformed JSON is a hard error (fail fast). If the
/// var is absent/blank, falls back to ONE llm-chat entry synthesized from the
/// legacy `MANAGER_CONTROL_URL` + the admin project id. Absent both → empty.
pub fn parse_session_apps(
    manager_control_apps: Option<&str>,
    legacy_url: Option<&str>,
    legacy_project_id: &str,
) -> Result<Vec<SessionApp>, String> {
    #[derive(serde::Deserialize)]
    struct Raw {
        key: Option<String>,
        name: Option<String>,
        #[serde(rename = "controlUrl")]
        control_url: Option<String>,
        #[serde(rename = "projectId")]
        project_id: Option<String>,
    }
    let nonempty = |s: Option<String>| s.map(|x| x.trim().to_string()).filter(|x| !x.is_empty());
    if let Some(j) = manager_control_apps.map(str::trim).filter(|s| !s.is_empty()) {
        let raws: Vec<Raw> = serde_json::from_str(j)
            .map_err(|e| format!("MANAGER_CONTROL_APPS is not valid JSON: {e}"))?;
        return Ok(raws
            .into_iter()
            .filter_map(|r| {
                Some(SessionApp {
                    key: nonempty(r.key)?,
                    name: nonempty(r.name)?,
                    control_url: nonempty(r.control_url)?,
                    project_id: nonempty(r.project_id)?,
                })
            })
            .collect());
    }
    if let Some(url) = legacy_url.map(str::trim).filter(|s| !s.is_empty()) {
        return Ok(vec![SessionApp {
            key: "llm-chat".to_string(),
            name: "llm-chat".to_string(),
            control_url: url.to_string(),
            project_id: legacy_project_id.to_string(),
        }]);
    }
    Ok(Vec::new())
}

/// The default app (the first registry entry) — used by the non-selectable
/// endpoints (usage, usage-daily, user files, and chat-sessions with no `?app=`).
pub fn default_app(apps: &[SessionApp]) -> Option<&SessionApp> {
    apps.first()
}

/// Resolve a registry entry by its `key`.
pub fn find_app<'a>(apps: &'a [SessionApp], key: &str) -> Option<&'a SessionApp> {
    apps.iter().find(|a| a.key == key)
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
    /// Whether the session cookie carries the `Secure` attribute. Secure by
    /// default (see parse_cookie_secure); a plain-HTTP dev stack sets
    /// ADMIN_COOKIE_SECURE=false. NEVER false in a TLS production deploy.
    pub cookie_secure: bool,
    /// Optional manager /control WS URL (e.g. ws://manager:7777/control) for the
    /// Sessions page's chat-sessions panel. OPTIONAL BY DESIGN (a capability-gated
    /// feature toggle, not a security value): absent → the panel reports
    /// `configured:false` and stays dark. No default URL is invented.
    pub manager_control_url: Option<String>,
    /// Chat-capable applications for the Sessions page (registry). The first
    /// entry is the default used by the non-selectable endpoints.
    pub session_apps: Vec<SessionApp>,
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
        let project_id = require_var("ZITADEL_PROJECT_ID", get("ZITADEL_PROJECT_ID"))?;
        let session_apps = parse_session_apps(
            get("MANAGER_CONTROL_APPS").as_deref(),
            get("MANAGER_CONTROL_URL").as_deref(),
            &project_id,
        )?;
        Ok(AdminConfig {
            issuer,
            project_id: project_id.clone(),
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
            cookie_secure: parse_cookie_secure(get("ADMIN_COOKIE_SECURE")),
            manager_control_url: get("MANAGER_CONTROL_URL")
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty()),
            session_apps,
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
    fn cookie_secure_defaults_true_and_only_explicit_falsey_disables() {
        assert!(parse_cookie_secure(None)); // absent → secure (fail closed)
        assert!(parse_cookie_secure(Some("true".into())));
        assert!(parse_cookie_secure(Some("anything".into())));
        assert!(!parse_cookie_secure(Some("false".into())));
        assert!(!parse_cookie_secure(Some(" FALSE ".into())));
        assert!(!parse_cookie_secure(Some("0".into())));
        assert!(!parse_cookie_secure(Some("no".into())));
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

    #[test]
    fn parse_session_apps_reads_json_array() {
        let json = r#"[
            {"key":"llm-chat","name":"llm-chat","controlUrl":"ws://m:7777/control","projectId":"p1"},
            {"key":"app2","name":"App Two","controlUrl":"ws://m2:7777/control","projectId":"p2"}
        ]"#;
        let apps = parse_session_apps(Some(json), None, "ignored").expect("ok");
        assert_eq!(apps.len(), 2);
        assert_eq!(apps[0], SessionApp {
            key: "llm-chat".into(), name: "llm-chat".into(),
            control_url: "ws://m:7777/control".into(), project_id: "p1".into(),
        });
        assert_eq!(apps[1].key, "app2");
        assert_eq!(apps[1].project_id, "p2");
    }

    #[test]
    fn parse_session_apps_falls_back_to_legacy_single_entry() {
        let apps = parse_session_apps(None, Some("ws://m:7777/control"), "p1").expect("ok");
        assert_eq!(apps, vec![SessionApp {
            key: "llm-chat".into(), name: "llm-chat".into(),
            control_url: "ws://m:7777/control".into(), project_id: "p1".into(),
        }]);
    }

    #[test]
    fn parse_session_apps_empty_when_nothing_configured() {
        assert_eq!(parse_session_apps(None, None, "p1").expect("ok"), vec![]);
        assert_eq!(parse_session_apps(Some("   "), Some("  "), "p1").expect("ok"), vec![]);
    }

    #[test]
    fn parse_session_apps_drops_entries_missing_a_field() {
        // second entry has no projectId -> dropped (never defaulted).
        let json = r#"[
            {"key":"ok","name":"OK","controlUrl":"ws://m/control","projectId":"p1"},
            {"key":"bad","name":"Bad","controlUrl":"ws://m/control"}
        ]"#;
        let apps = parse_session_apps(Some(json), None, "p1").expect("ok");
        assert_eq!(apps.len(), 1);
        assert_eq!(apps[0].key, "ok");
    }

    #[test]
    fn parse_session_apps_errors_on_malformed_json() {
        let err = parse_session_apps(Some("not json"), None, "p1").unwrap_err();
        assert!(err.contains("MANAGER_CONTROL_APPS"));
    }

    #[test]
    fn default_and_find_app() {
        let apps = parse_session_apps(
            Some(r#"[{"key":"a","name":"A","controlUrl":"u","projectId":"p"}]"#), None, "x",
        ).expect("ok");
        assert_eq!(default_app(&apps).unwrap().key, "a");
        assert_eq!(find_app(&apps, "a").unwrap().key, "a");
        assert!(find_app(&apps, "nope").is_none());
        assert!(default_app(&[]).is_none());
    }
}
