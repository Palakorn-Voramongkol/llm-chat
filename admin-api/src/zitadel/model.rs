//! Request/response models + v1<->v2 field mapping for the Zitadel user APIs.
//! Reads use v2 (`userId`/`username`/`isVerified`/`givenName/familyName`),
//! writes use v1 (appendix §3.1). One mapping site so the rest of the code is
//! version-agnostic.

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum UserKind {
    Human,
    Machine,
}

#[derive(Debug, Clone, Serialize)]
pub struct User {
    pub id: String,
    pub user_name: String,
    pub kind: UserKind,
    pub state: String,
    pub email: Option<String>,
    pub display_name: Option<String>,
    pub given_name: Option<String>,
    pub family_name: Option<String>,
}

fn str_at<'a>(v: &'a Value, key: &str) -> Option<&'a str> {
    v.get(key).and_then(Value::as_str)
}

/// Map a v2 user object (`/v2/users` list item or `/v2/users/{id}`.user) to `User`.
pub fn user_from_v2(v: &Value) -> User {
    let human = v.get("human");
    let machine = v.get("machine");
    let kind = if machine.is_some() {
        UserKind::Machine
    } else {
        UserKind::Human
    };

    let profile = human.and_then(|h| h.get("profile"));
    let given_name = profile.and_then(|p| str_at(p, "givenName")).map(String::from);
    let family_name = profile.and_then(|p| str_at(p, "familyName")).map(String::from);
    let email = human
        .and_then(|h| h.get("email"))
        .and_then(|e| str_at(e, "email"))
        .map(String::from);

    let display_name = profile
        .and_then(|p| str_at(p, "displayName"))
        .map(String::from)
        .or_else(|| machine.and_then(|m| str_at(m, "name")).map(String::from));

    User {
        id: str_at(v, "userId").unwrap_or_default().to_string(),
        user_name: str_at(v, "username").unwrap_or_default().to_string(),
        kind,
        // Normalize "USER_STATE_ACTIVE" -> "ACTIVE" so the single mapping site
        // matches the frontend UserState enum + the columns.tsx badge variants.
        state: str_at(v, "state")
            .unwrap_or_default()
            .trim_start_matches("USER_STATE_")
            .to_string(),
        email,
        display_name,
        given_name,
        family_name,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn user_from_v2_maps_human_fields() {
        let v = json!({
            "userId": "u-1", "username": "alice", "state": "USER_STATE_ACTIVE",
            "human": {
                "profile": { "givenName": "Alice", "familyName": "Stone",
                             "displayName": "Alice Stone" },
                "email": { "email": "alice@x.io", "isVerified": true }
            }
        });
        let u = user_from_v2(&v);
        assert_eq!(u.id, "u-1");
        assert_eq!(u.user_name, "alice");
        assert_eq!(u.kind, UserKind::Human);
        // state is normalized: the raw "USER_STATE_ACTIVE" loses its prefix so it
        // matches the frontend UserState enum + columns.tsx badge logic (Task 22).
        assert_eq!(u.state, "ACTIVE");
        assert_eq!(u.email.as_deref(), Some("alice@x.io"));
        assert_eq!(u.display_name.as_deref(), Some("Alice Stone"));
    }

    #[test]
    fn user_from_v2_maps_machine_with_no_email() {
        let v = json!({
            "userId": "m-9", "username": "chat-admin-api",
            "state": "USER_STATE_ACTIVE",
            "machine": { "name": "chat-admin-api", "description": "svc" }
        });
        let u = user_from_v2(&v);
        assert_eq!(u.kind, UserKind::Machine);
        assert_eq!(u.email, None);
        assert_eq!(u.display_name.as_deref(), Some("chat-admin-api"));
    }
}
