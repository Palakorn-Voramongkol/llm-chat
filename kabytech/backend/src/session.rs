//! The end-user session model + a fail-closed axum extractor: loads the
//! tower-sessions session and REJECTS unless roles contains "chat.user".
//! Ported from admin-api/src/session.rs (gate chat.admin -> chat.user).

use axum::{
    extract::FromRequestParts,
    http::{request::Parts, StatusCode},
};
use serde::{Deserialize, Serialize};
use tower_sessions::Session;

/// Absolute max session lifetime regardless of activity (idle expiry is on the
/// layer). Fail closed: a session with no login_at is treated as expired.
const SESSION_ABSOLUTE_MAX_SECS: u64 = 12 * 3600;

fn now_unix_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EndUser {
    pub user_id: String,
    pub name: String,
    pub roles: Vec<String>,
}

impl EndUser {
    pub fn has(&self, role: &str) -> bool {
        self.roles.iter().any(|r| r == role)
    }
}

impl<S> FromRequestParts<S> for EndUser
where
    S: Send + Sync,
{
    type Rejection = (StatusCode, &'static str);

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let session = Session::from_request_parts(parts, state)
            .await
            .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, "session layer missing"))?;
        let user: Option<EndUser> = session
            .get("end_user")
            .await
            .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, "session read failed"))?;
        match user {
            Some(u) if u.has("chat.user") => {
                let login_at: Option<u64> = session
                    .get("login_at")
                    .await
                    .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, "session read failed"))?;
                match login_at {
                    Some(t) if now_unix_secs().saturating_sub(t) < SESSION_ABSOLUTE_MAX_SECS => Ok(u),
                    _ => {
                        let _ = session.delete().await;
                        Err((StatusCode::UNAUTHORIZED, "session expired; re-authenticate"))
                    }
                }
            }
            Some(_) => Err((StatusCode::FORBIDDEN, "user lacks chat.user")),
            None => Err((StatusCode::UNAUTHORIZED, "no session")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn has_checks_membership() {
        let u = EndUser { user_id: "u1".into(), name: "U".into(), roles: vec!["chat.user".into()] };
        assert!(u.has("chat.user"));
        assert!(!u.has("chat.admin"));
    }
}
