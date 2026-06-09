//! The /api JSON surface (design §5). Every /api/* handler takes the Operator
//! extractor, so a missing/insufficient session is rejected before the body
//! runs. /login,/callback,/logout establish the session and are NOT gated.

pub mod error;

use axum::{
    extract::{Path, Query, State},
    routing::{delete, get, patch, post, put},
    Json, Router,
};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::auth;
use crate::session::Operator;
use crate::AppState;
use error::ApiError;

pub fn router(state: AppState) -> Router {
    Router::new()
        // operator OIDC (full-page nav) — no Operator extractor on these.
        .route("/login", get(auth::login))
        .route("/callback", get(auth::callback))
        .route("/logout", get(auth::logout))
        // gated /api surface
        .route("/api/me", get(me))
        .route("/api/users", get(list_users))
        .route("/api/users/{id}", get(get_user).delete(delete_user))
        .route("/api/users/human", post(create_human))
        .route("/api/users/machine", post(create_machine))
        .route("/api/users/{id}/profile", patch(edit_profile))
        .route("/api/users/{id}/email", patch(edit_email))
        .route("/api/users/{id}/password", post(set_password))
        .route("/api/users/{id}/resend-init", post(resend_init))
        .route("/api/users/{id}/deactivate", post(deactivate))
        .route("/api/users/{id}/reactivate", post(reactivate))
        .route("/api/users/{id}/lock", post(lock))
        .route("/api/users/{id}/unlock", post(unlock))
        .route("/api/users/{id}/grants", get(list_grants).post(add_grant))
        .route("/api/users/{id}/grants/{grantId}", put(set_grant).delete(remove_grant))
        .route("/api/roles", get(list_roles).post(create_role))
        .route("/api/roles/{roleKey}", delete(delete_role))
        .route("/api/roles/{roleKey}/holders", get(list_role_holders))
        .route("/api/users/{id}/keys", get(list_keys).post(create_key))
        .route("/api/users/{id}/keys/{keyId}", delete(delete_key))
        .route("/api/users/{id}/secret", post(generate_secret).delete(delete_secret))
        .route("/api/apps", get(list_apps).post(create_oidc_app))
        .route("/api/apps/{appId}", get(get_app).put(update_oidc_config).delete(delete_app))
        .route("/api/apps/{appId}/secret", post(regenerate_app_secret))
        .with_state(state)
}

async fn me(op: Operator) -> Json<Value> {
    Json(json!({ "userId": op.user_id, "name": op.name, "roles": op.roles }))
}

#[derive(Deserialize)]
struct UserListQuery { q: Option<String>, r#type: Option<String>, state: Option<String> }

async fn list_users(_op: Operator, State(st): State<AppState>, Query(qp): Query<UserListQuery>)
    -> Result<Json<Value>, ApiError> {
    // Map the optional filters to v2 SearchQuery[]. Exact query-field shapes are
    // verified against the running instance (appendix §6.3); unknown filters are
    // simply omitted (unfiltered list) rather than guessed.
    let mut queries: Vec<Value> = Vec::new();
    if let Some(q) = qp.q.filter(|s| !s.is_empty()) {
        queries.push(json!({ "userNameQuery": { "userName": q, "method": "TEXT_QUERY_METHOD_CONTAINS_IGNORE_CASE" } }));
    }
    if let Some(t) = qp.r#type.filter(|s| !s.is_empty()) {
        // "human" | "machine" -> v2 type query
        queries.push(json!({ "typeQuery": { "type": format!("TYPE_{}", t.to_uppercase()) } }));
    }
    if let Some(s) = qp.state.filter(|s| !s.is_empty()) {
        queries.push(json!({ "stateQuery": { "state": s } }));
    }
    let users = st.zitadel.search_users(queries).await?;
    Ok(Json(json!({ "result": users })))
}

async fn get_user(_op: Operator, State(st): State<AppState>, Path(id): Path<String>)
    -> Result<Json<Value>, ApiError> {
    let user = st.zitadel.get_user(&id).await?;
    let grants = st.zitadel.list_user_grants(&id).await?;
    Ok(Json(json!({ "user": user, "grants": grants })))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CreateHuman { user_name: String, given_name: String, family_name: String, email: String, password: String }
async fn create_human(_op: Operator, State(st): State<AppState>, Json(b): Json<CreateHuman>)
    -> Result<Json<Value>, ApiError> {
    let id = st.zitadel.create_human(&b.user_name, &b.given_name, &b.family_name, &b.email, &b.password).await?;
    Ok(Json(json!({ "userId": id })))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CreateMachine { user_name: String, name: String }
async fn create_machine(_op: Operator, State(st): State<AppState>, Json(b): Json<CreateMachine>)
    -> Result<Json<Value>, ApiError> {
    let id = st.zitadel.create_machine(&b.user_name, &b.name).await?;
    Ok(Json(json!({ "userId": id })))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct EditProfile { given_name: String, family_name: String }
async fn edit_profile(_op: Operator, State(st): State<AppState>, Path(id): Path<String>, Json(b): Json<EditProfile>)
    -> Result<Json<Value>, ApiError> {
    st.zitadel.edit_profile(&id, &b.given_name, &b.family_name).await?;
    Ok(Json(json!({ "ok": true })))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct EditEmail { email: String, #[serde(default)] is_verified: bool }
async fn edit_email(_op: Operator, State(st): State<AppState>, Path(id): Path<String>, Json(b): Json<EditEmail>)
    -> Result<Json<Value>, ApiError> {
    st.zitadel.edit_email(&id, &b.email, b.is_verified).await?;
    Ok(Json(json!({ "ok": true })))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SetPassword { password: String, #[serde(default)] change_required: bool }
async fn set_password(_op: Operator, State(st): State<AppState>, Path(id): Path<String>, Json(b): Json<SetPassword>)
    -> Result<Json<Value>, ApiError> {
    st.zitadel.set_password(&id, &b.password, b.change_required).await?;
    Ok(Json(json!({ "ok": true })))
}

async fn resend_init(_op: Operator, State(st): State<AppState>, Path(id): Path<String>) -> Result<Json<Value>, ApiError> {
    st.zitadel.resend_init(&id).await?;
    Ok(Json(json!({ "ok": true })))
}

macro_rules! lifecycle_handler {
    ($name:ident, $call:ident) => {
        async fn $name(_op: Operator, State(st): State<AppState>, Path(id): Path<String>) -> Result<Json<Value>, ApiError> {
            st.zitadel.$call(&id).await?;
            Ok(Json(json!({ "ok": true })))
        }
    };
}
lifecycle_handler!(deactivate, deactivate);
lifecycle_handler!(reactivate, reactivate);
lifecycle_handler!(lock, lock);
lifecycle_handler!(unlock, unlock);

async fn delete_user(_op: Operator, State(st): State<AppState>, Path(id): Path<String>) -> Result<Json<Value>, ApiError> {
    st.zitadel.delete_user(&id).await?;
    Ok(Json(json!({ "ok": true })))
}

async fn list_roles(_op: Operator, State(st): State<AppState>) -> Result<Json<Value>, ApiError> {
    Ok(Json(json!({ "result": st.zitadel.list_roles().await? })))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CreateRole { role_key: String, display_name: String, #[serde(default)] group: String }
async fn create_role(_op: Operator, State(st): State<AppState>, Json(b): Json<CreateRole>)
    -> Result<Json<Value>, ApiError> {
    st.zitadel.create_role(&b.role_key, &b.display_name, &b.group).await?;
    Ok(Json(json!({ "ok": true })))
}

// DELETE cascades — strips this role from every grant (design §7).
async fn delete_role(_op: Operator, State(st): State<AppState>, Path(role_key): Path<String>)
    -> Result<Json<Value>, ApiError> {
    st.zitadel.delete_role(&role_key).await?;
    Ok(Json(json!({ "ok": true })))
}

async fn list_role_holders(_op: Operator, State(st): State<AppState>, Path(role_key): Path<String>)
    -> Result<Json<Value>, ApiError> {
    Ok(Json(json!({ "result": st.zitadel.list_role_holders(&role_key).await? })))
}

async fn list_grants(_op: Operator, State(st): State<AppState>, Path(id): Path<String>) -> Result<Json<Value>, ApiError> {
    Ok(Json(json!({ "result": st.zitadel.list_user_grants(&id).await? })))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct AddGrant { role_keys: Vec<String> }
async fn add_grant(_op: Operator, State(st): State<AppState>, Path(id): Path<String>, Json(b): Json<AddGrant>)
    -> Result<Json<Value>, ApiError> {
    let grant_id = st.zitadel.add_grant(&id, &b.role_keys).await?;
    Ok(Json(json!({ "userGrantId": grant_id })))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SetGrant { role_keys: Vec<String> }
async fn set_grant(_op: Operator, State(st): State<AppState>, Path((id, grant_id)): Path<(String, String)>, Json(b): Json<SetGrant>)
    -> Result<Json<Value>, ApiError> {
    st.zitadel.set_grant_roles(&id, &grant_id, &b.role_keys).await?;
    Ok(Json(json!({ "ok": true })))
}

async fn remove_grant(_op: Operator, State(st): State<AppState>, Path((id, grant_id)): Path<(String, String)>)
    -> Result<Json<Value>, ApiError> {
    st.zitadel.remove_grant(&id, &grant_id).await?;
    Ok(Json(json!({ "ok": true })))
}

async fn list_keys(_op: Operator, State(st): State<AppState>, Path(id): Path<String>) -> Result<Json<Value>, ApiError> {
    Ok(Json(json!({ "result": st.zitadel.list_keys(&id).await? })))
}

// keyDetails (private key) returned ONCE; streamed straight to the operator,
// never persisted server-side (design §6 step 2).
async fn create_key(_op: Operator, State(st): State<AppState>, Path(id): Path<String>) -> Result<Json<Value>, ApiError> {
    Ok(Json(st.zitadel.create_json_key(&id).await?))
}

async fn delete_key(_op: Operator, State(st): State<AppState>, Path((id, key_id)): Path<(String, String)>)
    -> Result<Json<Value>, ApiError> {
    st.zitadel.delete_key(&id, &key_id).await?;
    Ok(Json(json!({ "ok": true })))
}

async fn generate_secret(_op: Operator, State(st): State<AppState>, Path(id): Path<String>) -> Result<Json<Value>, ApiError> {
    Ok(Json(st.zitadel.generate_secret(&id).await?))
}

async fn delete_secret(_op: Operator, State(st): State<AppState>, Path(id): Path<String>) -> Result<Json<Value>, ApiError> {
    st.zitadel.delete_secret(&id).await?;
    Ok(Json(json!({ "ok": true })))
}

async fn list_apps(_op: Operator, State(st): State<AppState>) -> Result<Json<Value>, ApiError> {
    Ok(Json(json!({ "result": st.zitadel.list_apps().await? })))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CreateOidcApp {
    name: String,
    redirect_uris: Vec<String>,
    response_types: Vec<String>,
    grant_types: Vec<String>,
    app_type: String,
    auth_method_type: String,
}
// clientSecret (WEB+BASIC) returned ONCE; streamed straight to the operator,
// never persisted/logged server-side (design §3 secret invariant).
async fn create_oidc_app(_op: Operator, State(st): State<AppState>, Json(b): Json<CreateOidcApp>)
    -> Result<Json<Value>, ApiError> {
    Ok(Json(st.zitadel.create_oidc_app(
        &b.name, &b.redirect_uris, &b.response_types, &b.grant_types, &b.app_type, &b.auth_method_type,
    ).await?))
}

async fn get_app(_op: Operator, State(st): State<AppState>, Path(app_id): Path<String>)
    -> Result<Json<Value>, ApiError> {
    Ok(Json(st.zitadel.get_app(&app_id).await?))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct UpdateOidcConfig {
    redirect_uris: Vec<String>,
    response_types: Vec<String>,
    grant_types: Vec<String>,
    app_type: String,
    auth_method_type: String,
}
async fn update_oidc_config(_op: Operator, State(st): State<AppState>, Path(app_id): Path<String>, Json(b): Json<UpdateOidcConfig>)
    -> Result<Json<Value>, ApiError> {
    st.zitadel.update_oidc_config(
        &app_id, &b.redirect_uris, &b.response_types, &b.grant_types, &b.app_type, &b.auth_method_type,
    ).await?;
    Ok(Json(json!({ "ok": true })))
}

// clientSecret returned ONCE on regenerate; streamed straight through (design §3).
async fn regenerate_app_secret(_op: Operator, State(st): State<AppState>, Path(app_id): Path<String>)
    -> Result<Json<Value>, ApiError> {
    Ok(Json(st.zitadel.regenerate_app_secret(&app_id).await?))
}

async fn delete_app(_op: Operator, State(st): State<AppState>, Path(app_id): Path<String>)
    -> Result<Json<Value>, ApiError> {
    st.zitadel.delete_app(&app_id).await?;
    Ok(Json(json!({ "ok": true })))
}

#[cfg(test)]
mod contract_tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn create_human_accepts_camelcase() {
        let b: CreateHuman = serde_json::from_value(json!({
            "userName": "alice", "givenName": "Alice", "familyName": "Stone",
            "email": "a@x.io", "password": "pw"
        })).expect("camelCase CreateHuman");
        assert_eq!(b.user_name, "alice");
        assert_eq!(b.given_name, "Alice");
        assert_eq!(b.family_name, "Stone");
    }

    #[test]
    fn create_machine_accepts_camelcase() {
        let b: CreateMachine = serde_json::from_value(json!({
            "userName": "bot", "name": "bot"
        })).expect("camelCase CreateMachine");
        assert_eq!(b.user_name, "bot");
    }

    #[test]
    fn add_grant_accepts_rolekeys() {
        let b: AddGrant = serde_json::from_value(json!({
            "roleKeys": ["chat.user"]
        })).expect("camelCase AddGrant");
        assert_eq!(b.role_keys, vec!["chat.user".to_string()]);
    }

    #[test]
    fn create_role_accepts_camelcase() {
        let b: CreateRole = serde_json::from_value(json!({
            "roleKey": "chat.viewer", "displayName": "Chat Viewer", "group": "chat"
        })).expect("camelCase CreateRole");
        assert_eq!(b.role_key, "chat.viewer");
        assert_eq!(b.display_name, "Chat Viewer");
        assert_eq!(b.group, "chat");
    }

    #[test]
    fn create_role_group_defaults_empty() {
        let b: CreateRole = serde_json::from_value(json!({
            "roleKey": "chat.viewer", "displayName": "Chat Viewer"
        })).expect("CreateRole without group");
        assert_eq!(b.group, "");
    }

    #[test]
    fn create_oidc_app_accepts_camelcase() {
        let b: CreateOidcApp = serde_json::from_value(json!({
            "name": "Chat",
            "redirectUris": ["https://x/cb"],
            "responseTypes": ["OIDC_RESPONSE_TYPE_CODE"],
            "grantTypes": ["OIDC_GRANT_TYPE_AUTHORIZATION_CODE"],
            "appType": "OIDC_APP_TYPE_WEB",
            "authMethodType": "OIDC_AUTH_METHOD_TYPE_BASIC"
        })).expect("camelCase CreateOidcApp");
        assert_eq!(b.name, "Chat");
        assert_eq!(b.redirect_uris, vec!["https://x/cb".to_string()]);
        assert_eq!(b.app_type, "OIDC_APP_TYPE_WEB");
        assert_eq!(b.auth_method_type, "OIDC_AUTH_METHOD_TYPE_BASIC");
    }

    #[test]
    fn update_oidc_config_accepts_camelcase() {
        let b: UpdateOidcConfig = serde_json::from_value(json!({
            "redirectUris": ["https://x/cb"],
            "responseTypes": ["OIDC_RESPONSE_TYPE_CODE"],
            "grantTypes": ["OIDC_GRANT_TYPE_AUTHORIZATION_CODE"],
            "appType": "OIDC_APP_TYPE_NATIVE",
            "authMethodType": "OIDC_AUTH_METHOD_TYPE_NONE"
        })).expect("camelCase UpdateOidcConfig");
        assert_eq!(b.app_type, "OIDC_APP_TYPE_NATIVE");
        assert_eq!(b.response_types, vec!["OIDC_RESPONSE_TYPE_CODE".to_string()]);
    }
}
