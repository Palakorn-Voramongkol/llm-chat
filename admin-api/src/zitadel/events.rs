//! Admin-API event log (audit) — POST /admin/v1/events/_search (design §11).
//! CAPABILITY-GATED: the event log needs IAM_OWNER_VIEWER (instance), which the
//! ORG_OWNER service account does NOT have, so `can_read_events` probes it and
//! the UI fails closed (§3/§11). When readable, results are CONFINED by
//! `resourceOwner` to the SA's own org — the instance log is instance-wide and
//! must never leak other orgs' events (fail-closed confinement, §11).

use serde_json::{json, Value};

use super::error::ZitadelError;
use super::ZitadelClient;

/// One audit query (mapped from /api/events query params). Pure input to
/// `build_events_body`; HTTP-agnostic so it is unit-testable.
pub struct EventQuery {
    pub editor_user_id: Option<String>,
    pub aggregate_id: Option<String>,
    /// Lower-bound creationDate (RFC3339) — the sequence cursor for paging.
    pub from: Option<String>,
    pub asc: bool,
    pub limit: u32,
}

/// PURE: build the POST /admin/v1/events/_search body. `resourceOwner` is ALWAYS
/// set to the SA's org so the instance-wide log is confined to one org (§11);
/// absent filters are omitted (no guessed defaults), present ones mapped to the
/// exact admin-API field names.
pub fn build_events_body(org_id: &str, q: &EventQuery) -> Value {
    let mut body = json!({
        "limit": q.limit,
        "asc": q.asc,
        "resourceOwner": org_id,
    });
    let obj = body.as_object_mut().expect("object");
    if let Some(e) = q.editor_user_id.as_ref().filter(|s| !s.is_empty()) {
        obj.insert("editorUserId".into(), json!(e));
    }
    if let Some(a) = q.aggregate_id.as_ref().filter(|s| !s.is_empty()) {
        // aggregateId is a repeated field on the events search.
        obj.insert("aggregateId".into(), json!([a]));
    }
    if let Some(d) = q.from.as_ref().filter(|s| !s.is_empty()) {
        obj.insert("creationDate".into(), json!(d));
    }
    body
}

/// PURE: extract the SA's org id from a GET /auth/v1/users/me body. The org is
/// at user.details.resourceOwner (provision.py:fetch_org_id, §11).
pub fn org_id_from_me(body: &Value) -> Option<String> {
    body.get("user")
        .and_then(|u| u.get("details"))
        .and_then(|d| d.get("resourceOwner"))
        .and_then(Value::as_str)
        .map(String::from)
}

/// PURE: classify an events probe result into a capability boolean. Only the
/// permission errors (Forbidden = missing IAM_OWNER_VIEWER, NotFound = endpoint
/// unavailable) mean "no capability"; everything else is a genuine failure and
/// must propagate so we never report "unavailable" for a transient outage (§11).
pub fn capability_from(res: Result<(), ZitadelError>) -> Result<bool, ZitadelError> {
    match res {
        Ok(()) => Ok(true),
        Err(ZitadelError::Forbidden) | Err(ZitadelError::NotFound) => Ok(false),
        Err(other) => Err(other),
    }
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

    /// Search the audit log: POST /admin/v1/events/_search, CONFINED to the SA's
    /// org via resourceOwner (§11). Returns the `events` array passed through
    /// (camelCase preserved). Needs IAM_OWNER_VIEWER — gate via can_read_events.
    pub async fn search_events(&self, q: &EventQuery) -> Result<Vec<Value>, ZitadelError> {
        let org_id = self.sa_org_id().await?;
        let url = format!("{}/admin/v1/events/_search", self.cfg.issuer);
        let v = self.post_json(&url, &build_events_body(&org_id, q)).await?;
        Ok(v.get("events").and_then(Value::as_array).cloned().unwrap_or_default())
    }

    /// Probe whether the SA can read the event log (needs IAM_OWNER_VIEWER, §3).
    /// Does a minimal confined search and maps a 403/404 to `false`; other errors
    /// propagate (do not masquerade as "no capability").
    pub async fn can_read_events(&self) -> Result<bool, ZitadelError> {
        let probe = EventQuery {
            editor_user_id: None,
            aggregate_id: None,
            from: None,
            asc: false,
            limit: 1,
        };
        capability_from(self.search_events(&probe).await.map(|_| ()))
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

    #[test]
    fn events_body_confines_to_org_and_carries_filters() {
        let b = build_events_body(
            "org-123",
            &EventQuery {
                editor_user_id: Some("u-9".into()),
                aggregate_id: Some("agg-7".into()),
                from: Some("2026-06-01T00:00:00Z".into()),
                asc: false,
                limit: 50,
            },
        );
        assert_eq!(b["asc"], json!(false));
        assert_eq!(b["limit"], json!(50));
        // resourceOwner confinement is ALWAYS present (§11).
        assert_eq!(b["resourceOwner"], json!("org-123"));
        assert_eq!(b["editorUserId"], json!("u-9"));
        assert_eq!(b["aggregateId"], json!(["agg-7"]));
        assert_eq!(b["creationDate"], json!("2026-06-01T00:00:00Z"));
    }

    #[test]
    fn events_body_omits_absent_filters_but_keeps_confinement() {
        let b = build_events_body(
            "org-123",
            &EventQuery { editor_user_id: None, aggregate_id: None, from: None, asc: false, limit: 100 },
        );
        assert_eq!(b["resourceOwner"], json!("org-123"));
        assert!(b.get("editorUserId").is_none());
        assert!(b.get("aggregateId").is_none());
        assert!(b.get("creationDate").is_none());
    }

    #[test]
    fn forbidden_and_not_found_mean_no_capability() {
        assert_eq!(capability_from(Err(ZitadelError::Forbidden)), Ok(false));
        assert_eq!(capability_from(Err(ZitadelError::NotFound)), Ok(false));
    }

    #[test]
    fn ok_means_capability_present() {
        assert_eq!(capability_from(Ok(())), Ok(true));
    }

    #[test]
    fn other_errors_propagate_not_swallowed() {
        // A transport/upstream failure is NOT "no capability" — surface it.
        assert_eq!(
            capability_from(Err(ZitadelError::Upstream)),
            Err(ZitadelError::Upstream)
        );
    }
}
