//! User-grant (authorization) wrappers + the revoke-one-role set math.
//! v1 Management API (appendix §3.4). The grant id is `userGrantId` on add and
//! `id` on search (same value). PUT REPLACES the whole roleKeys set, so
//! "remove one role" is read-modify-write via `roles_without` (design §7).

use serde_json::{json, Value};

use super::error::ZitadelError;
use super::ZitadelClient;

/// Return `current` with `drop` removed, order-preserving. Pure (design §7).
pub fn roles_without(current: &[String], drop: &str) -> Vec<String> {
    current.iter().filter(|r| *r != drop).cloned().collect()
}

/// Pure: the grants/_search body that finds every holder of `role` in
/// `pid`. Two queries AND-combine (reference §3.4): project + role.
pub fn holders_search_body(project_id: &str, role_key: &str) -> Value {
    json!({ "queries": [
        { "projectIdQuery": { "projectId": project_id } },
        { "roleKeyQuery": { "roleKey": role_key } },
    ] })
}

impl ZitadelClient {
    /// List project roles: POST /management/v1/projects/{pid}/roles/_search (§3.3).
    pub async fn list_roles(&self) -> Result<Vec<Value>, ZitadelError> {
        let pid = &self.cfg.project_id;
        let url = format!("{}/management/v1/projects/{}/roles/_search", self.cfg.issuer, pid);
        let v = self.post_json(&url, &json!({})).await?;
        Ok(v.get("result").and_then(Value::as_array).cloned().unwrap_or_default())
    }

    /// List a user's grants: POST /management/v1/users/grants/_search filtered by
    /// userId (§3.4). NOTE the path is /users/grants/_search, not nested per-user.
    pub async fn list_user_grants(&self, user_id: &str) -> Result<Vec<Value>, ZitadelError> {
        let url = format!("{}/management/v1/users/grants/_search", self.cfg.issuer);
        let body = json!({ "queries": [{ "userIdQuery": { "userId": user_id } }] });
        let v = self.post_json(&url, &body).await?;
        Ok(v.get("result").and_then(Value::as_array).cloned().unwrap_or_default())
    }

    /// Add a grant (one per user+project): POST /users/{id}/grants -> userGrantId.
    pub async fn add_grant(&self, user_id: &str, role_keys: &[String]) -> Result<String, ZitadelError> {
        let url = format!("{}/management/v1/users/{}/grants", self.cfg.issuer, user_id);
        let body = json!({ "projectId": self.cfg.project_id, "roleKeys": role_keys });
        let v = self.post_json(&url, &body).await?;
        Ok(v.get("userGrantId").and_then(Value::as_str).unwrap_or_default().to_string())
    }

    /// Replace the whole roleKeys set on a grant: PUT /users/{id}/grants/{grantId}.
    pub async fn set_grant_roles(&self, user_id: &str, grant_id: &str, role_keys: &[String]) -> Result<(), ZitadelError> {
        let url = format!("{}/management/v1/users/{}/grants/{}", self.cfg.issuer, user_id, grant_id);
        self.put_json(&url, &json!({ "roleKeys": role_keys })).await.map(|_| ())
    }

    /// Revoke an entire grant: DELETE /users/{id}/grants/{grantId}.
    pub async fn remove_grant(&self, user_id: &str, grant_id: &str) -> Result<(), ZitadelError> {
        let url = format!("{}/management/v1/users/{}/grants/{}", self.cfg.issuer, user_id, grant_id);
        self.delete(&url).await.map(|_| ())
    }

    /// Create a project role (§3.3): POST /projects/{pid}/roles {roleKey,
    /// displayName, group}. `roleKey` is the unique id (no separate id returned).
    pub async fn create_role(&self, role_key: &str, display_name: &str, group: &str) -> Result<(), ZitadelError> {
        let pid = &self.cfg.project_id;
        let url = format!("{}/management/v1/projects/{}/roles", self.cfg.issuer, pid);
        let body = json!({ "roleKey": role_key, "displayName": display_name, "group": group });
        self.post_json(&url, &body).await.map(|_| ())
    }

    /// Delete a project role (§3.3): DELETE /projects/{pid}/roles/{roleKey}.
    /// CASCADES — strips the role from every user grant (design §7 warning).
    pub async fn delete_role(&self, role_key: &str) -> Result<(), ZitadelError> {
        let pid = &self.cfg.project_id;
        let url = format!("{}/management/v1/projects/{}/roles/{}", self.cfg.issuer, pid, role_key);
        self.delete(&url).await.map(|_| ())
    }

    /// List holders of a role (§3.4): POST /users/grants/_search filtered by
    /// project + roleKey. Items carry {id, userId, roleKeys[], displayName,...}.
    pub async fn list_role_holders(&self, role_key: &str) -> Result<Vec<Value>, ZitadelError> {
        let url = format!("{}/management/v1/users/grants/_search", self.cfg.issuer);
        let body = holders_search_body(&self.cfg.project_id, role_key);
        let v = self.post_json(&url, &body).await?;
        Ok(v.get("result").and_then(Value::as_array).cloned().unwrap_or_default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roles_without_drops_only_the_named_role_preserving_order() {
        let cur = vec!["chat.user".to_string(), "chat.admin".to_string()];
        assert_eq!(roles_without(&cur, "chat.admin"), vec!["chat.user".to_string()]);
    }

    #[test]
    fn roles_without_is_noop_when_role_absent() {
        let cur = vec!["chat.user".to_string()];
        assert_eq!(roles_without(&cur, "chat.admin"), vec!["chat.user".to_string()]);
    }

    #[test]
    fn roles_without_can_empty_the_set() {
        let cur = vec!["chat.admin".to_string()];
        assert!(roles_without(&cur, "chat.admin").is_empty());
    }

    #[test]
    fn holders_query_filters_by_project_and_role_anded() {
        let body = super::holders_search_body("p1", "chat.admin");
        let queries = body.get("queries").and_then(Value::as_array).unwrap();
        assert_eq!(queries.len(), 2);
        assert_eq!(queries[0]["projectIdQuery"]["projectId"], "p1");
        assert_eq!(queries[1]["roleKeyQuery"]["roleKey"], "chat.admin");
    }
}
