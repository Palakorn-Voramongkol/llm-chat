//! The operator session model + a fail-closed axum extractor. Loads the
//! tower-sessions session and REJECTS 403 unless roles contains "chat.admin"
//! (design §4.2 "fails closed"). Reuses the Operator written by auth::callback.

use axum::{
    extract::FromRequestParts,
    http::{request::Parts, StatusCode},
};
use serde::{Deserialize, Serialize};
use tower_sessions::Session;

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
            Some(o) if o.has("chat.admin") => Ok(o),
            Some(_) => Err((StatusCode::FORBIDDEN, "operator lacks chat.admin")),
            None => Err((StatusCode::UNAUTHORIZED, "no operator session")),
        }
    }
}
