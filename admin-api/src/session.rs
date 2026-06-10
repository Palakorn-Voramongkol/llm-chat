//! The operator session model + a fail-closed axum extractor. Loads the
//! tower-sessions session and REJECTS 403 unless roles contains "chat.admin"
//! (design §4.2 "fails closed"). Reuses the Operator written by auth::callback.

use axum::{
    extract::FromRequestParts,
    http::{request::Parts, StatusCode},
};
use serde::{Deserialize, Serialize};
use tower_sessions::Session;

/// Absolute max session lifetime, regardless of activity. Idle expiry (8 h) is
/// set on the session layer; this is the hard ceiling on top of it, so a cookie
/// kept alive by continuous activity still forces periodic re-authentication —
/// capping the window a stolen session is usable. Fail closed: a session with
/// no recorded login_at is treated as expired.
const SESSION_ABSOLUTE_MAX_SECS: u64 = 12 * 3600;

fn now_unix_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Operator {
    pub user_id: String,
    pub name: String,
    pub roles: Vec<String>,
}

impl Operator {
    pub fn has(&self, role: &str) -> bool {
        self.roles.iter().any(|r| r == role)
    }
}

impl<S> FromRequestParts<S> for Operator
where
    S: Send + Sync,
{
    type Rejection = (StatusCode, &'static str);

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let session = Session::from_request_parts(parts, state)
            .await
            .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, "session layer missing"))?;
        let op: Option<Operator> = session
            .get("operator")
            .await
            .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, "session read failed"))?;
        match op {
            Some(o) if o.has("chat.admin") => {
                // Absolute-lifetime gate (defense in depth over idle expiry).
                let login_at: Option<u64> = session
                    .get("login_at")
                    .await
                    .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, "session read failed"))?;
                match login_at {
                    Some(t) if now_unix_secs().saturating_sub(t) < SESSION_ABSOLUTE_MAX_SECS => {
                        Ok(o)
                    }
                    _ => {
                        // Stale or unstamped → purge and force re-auth.
                        let _ = session.delete().await;
                        Err((StatusCode::UNAUTHORIZED, "session expired; re-authenticate"))
                    }
                }
            }
            Some(_) => Err((StatusCode::FORBIDDEN, "operator lacks chat.admin")),
            None => Err((StatusCode::UNAUTHORIZED, "no operator session")),
        }
    }
}
