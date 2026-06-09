//! Project read + update within the llm-chat project (design §9). The SA holds
//! PROJECT_OWNER, so GET + PUT here are within least privilege. v1 Management
//! API. Mirrors zitadel/apps.rs method style.
use serde_json::{json, Value};
use super::error::ZitadelError;
use super::ZitadelClient;

/// PURE: the PUT /projects/{id} body (read-modify-write the whole settings set).
fn update_project_body(name: &str, role_assertion: bool, role_check: bool, has_project_check: bool) -> Value {
    json!({ "name": name, "projectRoleAssertion": role_assertion,
            "projectRoleCheck": role_check, "hasProjectCheck": has_project_check })
}

impl ZitadelClient {
    /// GET /management/v1/projects/{id} — returns the project entity (unwrapped
    /// from its { "project": {...} } envelope; falls back to the whole value).
    pub async fn get_project(&self) -> Result<Value, ZitadelError> {
        let pid = &self.cfg.project_id;
        let url = format!("{}/management/v1/projects/{}", self.cfg.issuer, pid);
        let v = self.get_json(&url).await?;
        Ok(v.get("project").cloned().unwrap_or(v))
    }
    /// PUT /management/v1/projects/{id} — PROJECT_OWNER covers this.
    pub async fn update_project(&self, name: &str, role_assertion: bool, role_check: bool, has_project_check: bool) -> Result<(), ZitadelError> {
        let pid = &self.cfg.project_id;
        let url = format!("{}/management/v1/projects/{}", self.cfg.issuer, pid);
        let body = update_project_body(name, role_assertion, role_check, has_project_check);
        self.put_json(&url, &body).await.map(|_| ())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn update_project_body_carries_name_and_settings() {
        let b = update_project_body("llm-chat", true, false, true);
        assert_eq!(b["name"], "llm-chat");
        assert_eq!(b["projectRoleAssertion"], true);
        assert_eq!(b["projectRoleCheck"], false);
        assert_eq!(b["hasProjectCheck"], true);
    }
}
