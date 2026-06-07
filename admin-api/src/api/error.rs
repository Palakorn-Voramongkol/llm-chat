//! API error type: maps ZitadelError -> HTTP status + {code,message} JSON
//! (design §8). No internal/secret leakage in the message.

use axum::{http::StatusCode, response::{IntoResponse, Response}, Json};
use serde_json::json;

use crate::zitadel::error::ZitadelError;

#[derive(Debug)]
pub enum ApiError {
    NotFound(String),
    Forbidden(String),
    Conflict(String),
    BadRequest(String),
    Upstream(String),
}

impl ApiError {
    fn parts(&self) -> (StatusCode, &str, &str) {
        match self {
            ApiError::NotFound(m) => (StatusCode::NOT_FOUND, "not_found", m),
            ApiError::Forbidden(m) => (StatusCode::FORBIDDEN, "forbidden", m),
            ApiError::Conflict(m) => (StatusCode::CONFLICT, "already_exists", m),
            ApiError::BadRequest(m) => (StatusCode::BAD_REQUEST, "invalid", m),
            ApiError::Upstream(m) => (StatusCode::BAD_GATEWAY, "upstream", m),
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, code, message) = self.parts();
        (status, Json(json!({ "code": code, "message": message }))).into_response()
    }
}

impl From<ZitadelError> for ApiError {
    fn from(e: ZitadelError) -> Self {
        match e {
            ZitadelError::NotFound => ApiError::NotFound("resource not found".into()),
            ZitadelError::Forbidden => ApiError::Forbidden("permission denied".into()),
            ZitadelError::AlreadyExists => ApiError::Conflict("already exists".into()),
            ZitadelError::Invalid(m) => ApiError::BadRequest(m),
            ZitadelError::Upstream => ApiError::Upstream("zitadel upstream error".into()),
            ZitadelError::Transport(m) => ApiError::Upstream(m),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::zitadel::error::ZitadelError;
    use axum::http::StatusCode;
    use axum::response::IntoResponse;

    #[test]
    fn maps_zitadel_errors_to_http_status() {
        assert_eq!(ApiError::from(ZitadelError::NotFound).into_response().status(), StatusCode::NOT_FOUND);
        assert_eq!(ApiError::from(ZitadelError::Forbidden).into_response().status(), StatusCode::FORBIDDEN);
        assert_eq!(ApiError::from(ZitadelError::AlreadyExists).into_response().status(), StatusCode::CONFLICT);
        assert_eq!(ApiError::from(ZitadelError::Invalid("bad".into())).into_response().status(), StatusCode::BAD_REQUEST);
        assert_eq!(ApiError::from(ZitadelError::Upstream).into_response().status(), StatusCode::BAD_GATEWAY);
        assert_eq!(ApiError::from(ZitadelError::Transport("x".into())).into_response().status(), StatusCode::BAD_GATEWAY);
    }
}
