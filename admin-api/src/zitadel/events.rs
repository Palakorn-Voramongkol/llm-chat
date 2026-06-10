//! Admin-API event log (audit) — POST /admin/v1/events/_search (design §11).
//! CAPABILITY-GATED: the event log needs IAM_OWNER_VIEWER (instance), which the
//! ORG_OWNER service account does NOT have, so `can_read_events` probes it and
//! the UI fails closed (§3/§11). When readable, results are CONFINED by
//! `resourceOwner` to the SA's own org — the instance log is instance-wide and
//! must never leak other orgs' events (fail-closed confinement, §11).

use serde_json::Value;

use super::error::ZitadelError;
use super::ZitadelClient;

/// PURE: extract the SA's org id from a GET /auth/v1/users/me body. The org is
/// at user.details.resourceOwner (provision.py:fetch_org_id, §11).
pub fn org_id_from_me(body: &Value) -> Option<String> {
    body.get("user")
        .and_then(|u| u.get("details"))
        .and_then(|d| d.get("resourceOwner"))
        .and_then(Value::as_str)
        .map(String::from)
}

impl ZitadelClient {
    /// Resolve the SA's own org id (the confinement anchor). FAIL CLOSED: if the
    /// org cannot be resolved we return NotFound rather than search unconfined,
    /// because an unconfined event search would leak every org (§11).
    pub async fn sa_org_id(&self) -> Result<String, ZitadelError> {
        let url = format!("{}/auth/v1/users/me", self.cfg.issuer);
        let v = self.get_json(&url).await?;
        org_id_from_me(&v).ok_or(ZitadelError::NotFound)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn org_id_from_me_reads_details_resource_owner() {
        // GET /auth/v1/users/me shape (provision.py:fetch_org_id, §11 confinement).
        let body = json!({
            "user": { "details": { "resourceOwner": "org-123" } }
        });
        assert_eq!(org_id_from_me(&body), Some("org-123".to_string()));
    }

    #[test]
    fn org_id_from_me_is_none_when_missing() {
        assert_eq!(org_id_from_me(&json!({ "user": { "details": {} } })), None);
    }
}
