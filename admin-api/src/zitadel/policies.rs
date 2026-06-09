//! Read-only org-policy getters (design §9). The least-privilege SA may not even
//! be able to READ org policies, so a 403 degrades to a typed "unavailable";
//! every OTHER error propagates. No write path by design.
use serde_json::Value;
use super::error::ZitadelError;
use super::ZitadelClient;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PolicyRead { Available(Value), Unavailable }

/// PURE: a 403 (Forbidden) → Unavailable; unwrap the { "policy": {...} }
/// envelope; every other error propagates.
fn classify_policy(res: Result<Value, ZitadelError>) -> Result<PolicyRead, ZitadelError> {
    match res {
        Ok(v) => Ok(PolicyRead::Available(v.get("policy").cloned().unwrap_or(v))),
        Err(ZitadelError::Forbidden) => Ok(PolicyRead::Unavailable),
        Err(e) => Err(e),
    }
}

impl ZitadelClient {
    /// Org login policy: GET /management/v1/policies/login (design §9).
    pub async fn get_login_policy(&self) -> Result<PolicyRead, ZitadelError> {
        let url = format!("{}/management/v1/policies/login", self.cfg.issuer);
        classify_policy(self.get_json(&url).await)
    }
    /// Org password-complexity policy: GET /management/v1/policies/password/complexity.
    pub async fn get_password_complexity_policy(&self) -> Result<PolicyRead, ZitadelError> {
        let url = format!("{}/management/v1/policies/password/complexity", self.cfg.issuer);
        classify_policy(self.get_json(&url).await)
    }
    /// Org lockout policy: GET /management/v1/policies/lockout.
    pub async fn get_lockout_policy(&self) -> Result<PolicyRead, ZitadelError> {
        let url = format!("{}/management/v1/policies/lockout", self.cfg.issuer);
        classify_policy(self.get_json(&url).await)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test] fn available_unwraps_envelope() {
        assert_eq!(classify_policy(Ok(json!({"policy":{"minLength":"8"}}))).unwrap(),
                   PolicyRead::Available(json!({"minLength":"8"})));
    }
    #[test] fn forbidden_degrades() {
        assert_eq!(classify_policy(Err(ZitadelError::Forbidden)).unwrap(), PolicyRead::Unavailable);
    }
    #[test] fn other_errors_propagate() {
        assert!(classify_policy(Err(ZitadelError::NotFound)).is_err());
    }
}
