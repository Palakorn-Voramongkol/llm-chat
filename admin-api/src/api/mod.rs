//! The /api JSON surface (design §5). Every /api/* handler takes the Operator
//! extractor, so a missing/insufficient session is rejected before the body
//! runs. /login,/callback,/logout establish the session and are NOT gated.

pub mod error;

use axum::{
    extract::{Path, Query, State},
    routing::{delete, get, patch, post, put},
    Json, Router,
};
use serde::{Deserialize, Serialize};
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
        .route("/api/events", get(list_events))
        .route("/api/capabilities", get(list_capabilities))
        .route("/api/stats", get(stats))
        .route("/api/roles/{roleKey}", put(update_role).delete(delete_role))
        .route("/api/roles/{roleKey}/holders", get(list_role_holders))
        .route("/api/users/{id}/keys", get(list_keys).post(create_key))
        .route("/api/users/{id}/keys/{keyId}", delete(delete_key))
        .route("/api/users/{id}/secret", post(generate_secret).delete(delete_secret))
        .route("/api/users/{id}/files", get(user_files))
        .route("/api/apps", get(list_apps).post(create_oidc_app))
        .route("/api/apps/{appId}", get(get_app).put(update_oidc_config).delete(delete_app))
        .route("/api/apps/{appId}/secret", post(regenerate_app_secret))
        .route("/api/org", get(get_org).put(update_org))
        .route("/api/project", get(get_project).put(update_project))
        // Multi-application authorization (each project = one application).
        .route("/api/projects", get(list_projects))
        .route("/api/projects/{pid}/roles", get(list_project_roles).post(create_project_role))
        .route("/api/projects/{pid}/roles/{roleKey}", put(update_project_role).delete(delete_project_role))
        .route("/api/projects/{pid}/apps", get(list_project_apps).post(create_project_app))
        .route("/api/projects/{pid}/apps/{appId}", get(get_project_app).put(update_project_app).delete(delete_project_app))
        .route("/api/projects/{pid}/apps/{appId}/secret", post(regenerate_project_app_secret))
        .route("/api/projects/{pid}/grants", get(list_project_grants))
        .route("/api/org/policies/login", get(get_login_policy))
        .route("/api/org/policies/password-complexity", get(get_password_complexity_policy))
        .route("/api/org/policies/lockout", get(get_lockout_policy))
        .route("/api/status", get(status))
        .route("/api/chat-sessions", get(chat_sessions))
        .route("/api/session-apps", get(session_apps))
        .route("/api/usage", get(usage))
        .route("/api/usage-daily", get(usage_daily))
        .route("/api/signins", get(list_signins))
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
struct EventListQuery {
    editor: Option<String>,
    aggregate: Option<String>,
    from: Option<String>,
    #[serde(default)]
    asc: bool,
    #[serde(default = "default_event_limit")]
    limit: u32,
}
fn default_event_limit() -> u32 { 100 }

/// PURE: the /api/capabilities body. One field today (events); a fail-closed
/// boolean the audit page branches on (§11).
fn capabilities_json(events: bool) -> Value {
    json!({ "events": events })
}

async fn list_capabilities(_op: Operator, State(st): State<AppState>) -> Result<Json<Value>, ApiError> {
    let events = st.zitadel.can_read_events().await?;
    Ok(Json(capabilities_json(events)))
}

/// Sessions page (§Sessions): the operator's own session + platform health in
/// one read. Health values DEGRADE to false (this is a status view — a probe
/// failure is itself the signal); identity comes from the verified session.
async fn status(
    op: Operator,
    session: tower_sessions::Session,
    State(st): State<AppState>,
) -> Result<Json<Value>, ApiError> {
    let zitadel_ok = st.zitadel.valid_token().await.is_ok();
    let events_cap = st.zitadel.can_read_events().await.unwrap_or(false);
    let expires_at = session
        .expiry_date()
        .format(&time::format_description::well_known::Rfc3339)
        .ok();
    Ok(Json(json!({
        "operator": { "userId": op.user_id, "name": op.name, "roles": op.roles },
        "session": { "expiresAt": expires_at },
        "health": { "zitadel": zitadel_ok },
        "capabilities": {
            "events": events_cap,
            "chatSessions": !st.cfg.session_apps.is_empty(),
        },
    })))
}

/// Active chat sessions via the manager's /control (read-only "list" +
/// "instances"). Capability-gated on MANAGER_CONTROL_URL; each reply degrades
/// independently so one failing backend never blanks the panel.
#[derive(Deserialize)]
struct ChatSessionsQuery { app: Option<String> }

async fn chat_sessions(_op: Operator, State(st): State<AppState>, Query(qp): Query<ChatSessionsQuery>)
    -> Result<Json<Value>, ApiError> {
    let app = match qp.app.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        Some(key) => match crate::config::find_app(&st.cfg.session_apps, key) {
            Some(a) => a,
            None => return Err(ApiError::BadRequest(format!("unknown application: {key}"))),
        },
        None => match crate::config::default_app(&st.cfg.session_apps) {
            Some(a) => a,
            None => return Ok(Json(json!({ "configured": false }))),
        },
    };
    let token = st.zitadel.mint_chat_token(&app.project_id).await?;
    let list = crate::manager::control_query(&app.control_url, &token, "list")
        .await
        .unwrap_or_else(|e| json!({ "ok": false, "error": e }));
    let instances = crate::manager::control_query(&app.control_url, &token, "instances")
        .await
        .unwrap_or_else(|e| json!({ "ok": false, "error": e }));
    // Live /chat clients — carries each session's authenticated owner (userId).
    let clients = crate::manager::control_query(&app.control_url, &token, "clients")
        .await
        .unwrap_or_else(|e| json!({ "ok": false, "error": e }));
    Ok(Json(crate::manager::combine_control_replies(list, instances, clients)))
}

/// PURE: display-ready app list for the Sessions picker — only key + name leave
/// the server (control URLs / project ids stay internal).
fn session_apps_json(apps: &[crate::config::SessionApp]) -> Value {
    json!({
        "apps": apps.iter().map(|a| json!({ "key": a.key, "name": a.name })).collect::<Vec<_>>(),
    })
}

/// The chat-capable applications for the Sessions page picker.
async fn session_apps(_op: Operator, State(st): State<AppState>) -> Json<Value> {
    Json(session_apps_json(&st.cfg.session_apps))
}

/// Per-user token usage from the manager's /control "usage" (chat.admin-gated).
/// Capability-gated on MANAGER_CONTROL_URL, exactly like chat_sessions.
async fn usage(_op: Operator, State(st): State<AppState>) -> Result<Json<Value>, ApiError> {
    let Some(app) = crate::config::default_app(&st.cfg.session_apps) else {
        return Ok(Json(json!({ "configured": false, "users": [], "totals": {} })));
    };
    let token = st.zitadel.mint_chat_token(&app.project_id).await?;
    Ok(Json(
        crate::manager::control_query(&app.control_url, &token, "usage")
            .await
            .unwrap_or_else(|e| json!({ "ok": false, "error": e })),
    ))
}

/// Per-user per-day token usage (last 30 days) from /control "usage-daily".
/// chat.admin-gated, capability-gated on MANAGER_CONTROL_URL — mirrors usage().
async fn usage_daily(_op: Operator, State(st): State<AppState>) -> Result<Json<Value>, ApiError> {
    let Some(app) = crate::config::default_app(&st.cfg.session_apps) else {
        return Ok(Json(json!({ "configured": false, "days": [] })));
    };
    let token = st.zitadel.mint_chat_token(&app.project_id).await?;
    Ok(Json(
        crate::manager::control_query(&app.control_url, &token, "usage-daily")
            .await
            .unwrap_or_else(|e| json!({ "ok": false, "error": e })),
    ))
}

/// A user's claude sandbox tree (read-only) via the manager's /control
/// "user-box". chat.admin-gated; capability-gated on MANAGER_CONTROL_URL like
/// chat_sessions/usage. The worker confines the listing to {base}/{userId}.
async fn user_files(_op: Operator, State(st): State<AppState>, Path(id): Path<String>)
    -> Result<Json<Value>, ApiError> {
    let Some(app) = crate::config::default_app(&st.cfg.session_apps) else {
        return Ok(Json(json!({ "configured": false, "entries": [], "truncated": false })));
    };
    let token = st.zitadel.mint_chat_token(&app.project_id).await?;
    let reply = crate::manager::control_request(&app.control_url, &token, json!({ "cmd": "user-box", "userId": id }))
        .await
        .unwrap_or_else(|e| json!({ "ok": false, "error": e }));
    Ok(Json(json!({
        "configured": true,
        "ok": reply.get("ok").and_then(Value::as_bool).unwrap_or(false),
        "entries": reply.get("entries").cloned().unwrap_or_else(|| json!([])),
        "truncated": reply.get("truncated").and_then(Value::as_bool).unwrap_or(false),
        "error": reply.get("error").cloned(),
    })))
}

/// Recent sign-ins derived from the audit event log (the honest source on this
/// stack — the classic hosted login creates no v2 session-API sessions, which
/// was verified live to return an empty search). Same capability gate as Audit.
async fn list_signins(_op: Operator, State(st): State<AppState>) -> Result<Json<Value>, ApiError> {
    use crate::zitadel::events::{is_signin_event, EventQuery};
    if !st.zitadel.can_read_events().await.unwrap_or(false) {
        return Ok(Json(json!({ "available": false, "result": [] })));
    }
    let q = EventQuery {
        editor_user_id: None,
        aggregate_id: None,
        from: None,
        asc: false,
        limit: 100,
    };
    let events = st.zitadel.search_events(&q).await?;
    let signins: Vec<Value> = events
        .into_iter()
        .filter(|e| {
            e.get("type")
                .and_then(|t| t.get("type"))
                .and_then(Value::as_str)
                .map(is_signin_event)
                .unwrap_or(false)
        })
        .collect();
    Ok(Json(json!({ "available": true, "result": signins })))
}

async fn list_events(_op: Operator, State(st): State<AppState>, Query(qp): Query<EventListQuery>)
    -> Result<Json<Value>, ApiError> {
    let q = crate::zitadel::events::EventQuery {
        editor_user_id: qp.editor,
        aggregate_id: qp.aggregate,
        from: qp.from,
        asc: qp.asc,
        limit: qp.limit,
    };
    let events = st.zitadel.search_events(&q).await?;
    Ok(Json(json!({ "result": events })))
}

/// Dashboard counts (design §10). Each count is `Option<u64>` — `null` in JSON
/// when its own fan-out call failed, so the card shows an em-dash, never a false
/// `0` (§12). camelCase preserved for the frontend `Stats` type.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct StatsResponse {
    humans: Option<u64>,
    machines: Option<u64>,
    roles: Option<u64>,
    grants: Option<u64>,
    apps: Option<u64>,
    token_healthy: bool,
}

/// GET /api/stats — fan out the per-area `totalResult` counts + a SA-token
/// health self-check. Counts run concurrently; `valid_token()` proves the BFF
/// can still mint a Management token (no new Zitadel surface beyond apps search).
async fn stats(_op: Operator, State(st): State<AppState>) -> Json<Value> {
    let (humans, machines, roles, grants, apps) = tokio::join!(
        st.zitadel.count_humans(),
        st.zitadel.count_machines(),
        st.zitadel.count_roles(),
        st.zitadel.count_grants(),
        st.zitadel.count_apps(),
    );
    let token_healthy = st.zitadel.valid_token().await.is_ok();
    let body = StatsResponse { humans, machines, roles, grants, apps, token_healthy };
    Json(serde_json::to_value(body).unwrap_or_else(|_| json!({})))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CreateRole { role_key: String, display_name: String, #[serde(default)] group: String }
async fn create_role(_op: Operator, State(st): State<AppState>, Json(b): Json<CreateRole>)
    -> Result<Json<Value>, ApiError> {
    st.zitadel.create_role(&b.role_key, &b.display_name, &b.group).await?;
    Ok(Json(json!({ "ok": true })))
}

// Rename a role's display name + group (the key is immutable). Home project.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct UpdateRole { display_name: String, #[serde(default)] group: String }
async fn update_role(_op: Operator, State(st): State<AppState>, Path(role_key): Path<String>, Json(b): Json<UpdateRole>)
    -> Result<Json<Value>, ApiError> {
    st.zitadel.update_role(&role_key, &b.display_name, &b.group).await?;
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
struct AddGrant {
    role_keys: Vec<String>,
    // Which application (project) to grant on. Omitted => the home project (the
    // single-project Access dialog); the access matrix sends it explicitly.
    #[serde(default)]
    project_id: Option<String>,
}
async fn add_grant(_op: Operator, State(st): State<AppState>, Path(id): Path<String>, Json(b): Json<AddGrant>)
    -> Result<Json<Value>, ApiError> {
    let grant_id = match b.project_id.as_deref() {
        Some(pid) => st.zitadel.add_grant_to(&id, pid, &b.role_keys).await?,
        None => st.zitadel.add_grant(&id, &b.role_keys).await?,
    };
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

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct UpdateProject {
    name: String,
    #[serde(default)] project_role_assertion: bool,
    #[serde(default)] project_role_check: bool,
    #[serde(default)] has_project_check: bool,
}

// The organization (name + id). Renaming is allowed via ORG_SETTINGS_MANAGER on
// the SA (minimal org role; NOT ORG_OWNER).
async fn get_org(_op: Operator, State(st): State<AppState>) -> Result<Json<Value>, ApiError> {
    Ok(Json(st.zitadel.get_org().await?))
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct UpdateOrg { name: String }
async fn update_org(_op: Operator, State(st): State<AppState>, Json(b): Json<UpdateOrg>)
    -> Result<Json<Value>, ApiError> {
    st.zitadel.update_org(&b.name).await?;
    Ok(Json(json!({ "ok": true })))
}

async fn get_project(_op: Operator, State(st): State<AppState>) -> Result<Json<Value>, ApiError> {
    Ok(Json(st.zitadel.get_project().await?))
}

// ---- Multi-application authorization (each project = one application) ----
// All Operator-gated. Read surfaces for P1: list applications, an application's
// roles + login clients, and its user roster (who can use it, with which roles).
async fn list_projects(_op: Operator, State(st): State<AppState>) -> Result<Json<Value>, ApiError> {
    // Exclude Zitadel's reserved internal "ZITADEL" project: it is not a
    // user-facing application, and granting its IAM roles is out of scope for a
    // chat.admin operator. The Console manages user applications only.
    let apps: Vec<Value> = st.zitadel.list_projects().await?
        .into_iter()
        .filter(|p| p.get("name").and_then(Value::as_str) != Some("ZITADEL"))
        .collect();
    Ok(Json(json!({ "result": apps })))
}
async fn list_project_roles(_op: Operator, State(st): State<AppState>, Path(pid): Path<String>)
    -> Result<Json<Value>, ApiError> {
    Ok(Json(json!({ "result": st.zitadel.list_roles_for(&pid).await? })))
}
// Define a role IN an application (P2). Requires PROJECT_OWNER on pid — Zitadel
// returns 403 otherwise (the SA owns its home + provisioner-owned projects).
async fn create_project_role(_op: Operator, State(st): State<AppState>, Path(pid): Path<String>, Json(b): Json<CreateRole>)
    -> Result<Json<Value>, ApiError> {
    st.zitadel.create_role_in(&pid, &b.role_key, &b.display_name, &b.group).await?;
    Ok(Json(json!({ "ok": true })))
}
// Rename a role's display name + group on an application (key immutable).
async fn update_project_role(_op: Operator, State(st): State<AppState>, Path((pid, role_key)): Path<(String, String)>, Json(b): Json<UpdateRole>)
    -> Result<Json<Value>, ApiError> {
    st.zitadel.update_role_in(&pid, &role_key, &b.display_name, &b.group).await?;
    Ok(Json(json!({ "ok": true })))
}
// DELETE cascades — strips this role from every grant on the app (design §7).
async fn delete_project_role(_op: Operator, State(st): State<AppState>, Path((pid, role_key)): Path<(String, String)>)
    -> Result<Json<Value>, ApiError> {
    st.zitadel.delete_role_in(&pid, &role_key).await?;
    Ok(Json(json!({ "ok": true })))
}
async fn list_project_apps(_op: Operator, State(st): State<AppState>, Path(pid): Path<String>)
    -> Result<Json<Value>, ApiError> {
    Ok(Json(json!({ "result": st.zitadel.list_apps_for(&pid).await? })))
}

// Login-client (OIDC app) CRUD scoped to a project (the multi-app model).
// Requires PROJECT_OWNER on pid — Zitadel returns 403 otherwise (fail-closed,
// no fallback to the home project). clientSecret on create/regenerate is
// streamed straight through, never logged.
async fn create_project_app(_op: Operator, State(st): State<AppState>, Path(pid): Path<String>, Json(b): Json<CreateOidcApp>)
    -> Result<Json<Value>, ApiError> {
    Ok(Json(st.zitadel.create_oidc_app_in(
        &pid, &b.name, &b.redirect_uris, &b.response_types, &b.grant_types, &b.app_type, &b.auth_method_type,
    ).await?))
}

async fn get_project_app(_op: Operator, State(st): State<AppState>, Path((pid, app_id)): Path<(String, String)>)
    -> Result<Json<Value>, ApiError> {
    Ok(Json(st.zitadel.get_app_in(&pid, &app_id).await?))
}

async fn update_project_app(_op: Operator, State(st): State<AppState>, Path((pid, app_id)): Path<(String, String)>, Json(b): Json<UpdateOidcConfig>)
    -> Result<Json<Value>, ApiError> {
    st.zitadel.update_oidc_config_in(
        &pid, &app_id, &b.redirect_uris, &b.response_types, &b.grant_types, &b.app_type, &b.auth_method_type,
    ).await?;
    Ok(Json(json!({ "ok": true })))
}

async fn delete_project_app(_op: Operator, State(st): State<AppState>, Path((pid, app_id)): Path<(String, String)>)
    -> Result<Json<Value>, ApiError> {
    st.zitadel.delete_app_in(&pid, &app_id).await?;
    Ok(Json(json!({ "ok": true })))
}

async fn regenerate_project_app_secret(_op: Operator, State(st): State<AppState>, Path((pid, app_id)): Path<(String, String)>)
    -> Result<Json<Value>, ApiError> {
    Ok(Json(st.zitadel.regenerate_app_secret_in(&pid, &app_id).await?))
}

async fn list_project_grants(_op: Operator, State(st): State<AppState>, Path(pid): Path<String>)
    -> Result<Json<Value>, ApiError> {
    Ok(Json(json!({ "result": st.zitadel.list_project_grants(&pid).await? })))
}
async fn update_project(_op: Operator, State(st): State<AppState>, Json(b): Json<UpdateProject>) -> Result<Json<Value>, ApiError> {
    st.zitadel.update_project(&b.name, b.project_role_assertion, b.project_role_check, b.has_project_check).await?;
    Ok(Json(json!({ "ok": true })))
}

// Read-only policy handlers: Unavailable (degraded 403) surfaces as a 200
// envelope { available:false, policy:null }, never an HTTP error (design §9).
fn policy_envelope(p: crate::zitadel::policies::PolicyRead) -> Json<Value> {
    use crate::zitadel::policies::PolicyRead;
    match p {
        PolicyRead::Available(v) => Json(json!({ "available": true, "policy": v })),
        PolicyRead::Unavailable => Json(json!({ "available": false, "policy": Value::Null })),
    }
}
async fn get_login_policy(_op: Operator, State(st): State<AppState>) -> Result<Json<Value>, ApiError> {
    Ok(policy_envelope(st.zitadel.get_login_policy().await?))
}
async fn get_password_complexity_policy(_op: Operator, State(st): State<AppState>) -> Result<Json<Value>, ApiError> {
    Ok(policy_envelope(st.zitadel.get_password_complexity_policy().await?))
}
async fn get_lockout_policy(_op: Operator, State(st): State<AppState>) -> Result<Json<Value>, ApiError> {
    Ok(policy_envelope(st.zitadel.get_lockout_policy().await?))
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

    #[test]
    fn update_project_accepts_camelcase() {
        let b: UpdateProject = serde_json::from_value(json!({
            "name":"llm-chat","projectRoleAssertion":true,"projectRoleCheck":false,"hasProjectCheck":true
        })).expect("camelCase UpdateProject");
        assert_eq!(b.name, "llm-chat");
        assert!(b.project_role_assertion);
        assert!(!b.project_role_check);
        assert!(b.has_project_check);
    }

    #[test]
    fn event_list_query_parses_filters_with_defaults() {
        // editor/aggregate/from optional; asc + limit defaulted when absent.
        let q: EventListQuery = serde_urlencoded::from_str(
            "editor=u-9&aggregate=agg-7&from=2026-06-01T00:00:00Z"
        ).expect("parse event query");
        assert_eq!(q.editor.as_deref(), Some("u-9"));
        assert_eq!(q.aggregate.as_deref(), Some("agg-7"));
        assert_eq!(q.from.as_deref(), Some("2026-06-01T00:00:00Z"));
        assert!(!q.asc, "asc defaults to false (newest-first)");
        assert_eq!(q.limit, 100, "limit defaults to 100");
    }

    #[test]
    fn capabilities_payload_shape() {
        // The /api/capabilities body the audit page reads.
        let v = capabilities_json(false);
        assert_eq!(v.get("events").and_then(Value::as_bool), Some(false));
        assert_eq!(capabilities_json(true).get("events").and_then(Value::as_bool), Some(true));
    }

    #[test]
    fn session_apps_json_exposes_only_key_and_name() {
        let apps = vec![
            crate::config::SessionApp {
                key: "llm-chat".into(), name: "llm-chat".into(),
                control_url: "ws://secret/control".into(), project_id: "p1".into(),
            },
        ];
        let v = session_apps_json(&apps);
        assert_eq!(v["apps"][0]["key"], "llm-chat");
        assert_eq!(v["apps"][0]["name"], "llm-chat");
        // control_url / project_id must NOT leak to the client.
        assert!(v["apps"][0].get("controlUrl").is_none());
        assert!(v["apps"][0].get("projectId").is_none());
        assert!(v["apps"][0].get("control_url").is_none());
    }

    #[test]
    fn stats_response_serializes_camelcase() {
        let s = StatsResponse {
            humans: Some(18),
            machines: Some(6),
            roles: Some(3),
            grants: Some(40),
            apps: Some(3),
            token_healthy: true,
        };
        let v = serde_json::to_value(&s).expect("serialize StatsResponse");
        assert_eq!(v.get("humans").and_then(|x| x.as_u64()), Some(18));
        assert!(v.get("tokenHealthy").and_then(|x| x.as_bool()).unwrap(), "camelCase tokenHealthy: {v}");
        assert!(v.get("token_healthy").is_none(), "no snake_case: {v}");
        // A failed count must serialize as JSON null (em-dash on the card), not 0.
        let degraded = serde_json::to_value(StatsResponse {
            humans: None, machines: None, roles: None, grants: None, apps: None,
            token_healthy: false,
        }).unwrap();
        assert!(degraded.get("humans").unwrap().is_null(), "null count: {degraded}");
    }

    /// Build a test router with the session layer attached but NO session cookie.
    /// The Operator extractor finds no "operator" key in the session and returns
    /// 401. Used to gate-test every /api/* route without spinning up Zitadel.
    fn test_router_no_session() -> axum::Router {
        use std::sync::Arc;
        use tower_sessions::{MemoryStore, SessionManagerLayer};
        use zitadel_auth::{JwksCache, ZitadelConfig};

        let cfg = crate::config::AdminConfig::from_map(&|k| match k {
            "ZITADEL_ISSUER" => Some("http://localhost:8080".into()),
            "ZITADEL_PROJECT_ID" => Some("test-project".into()),
            "ZITADEL_AUDIENCE" => Some("test-audience".into()),
            "ADMIN_SA_KEY_PATH" => Some("/tmp/nonexistent.json".into()),
            "ADMIN_OIDC_CLIENT_ID" => Some("test-client-id".into()),
            "ADMIN_OIDC_CLIENT_SECRET" => Some("test-client-secret".into()),
            "ADMIN_BIND_ADDR" => Some("0.0.0.0:0".into()),
            "ADMIN_PUBLIC_ORIGIN" => Some("http://localhost:0".into()),
            "ADMIN_ALLOWED_ORIGIN" => Some("http://localhost:3000".into()),
            "ADMIN_SESSION_KEY" => Some("0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef".into()),
            _ => None,
        }).expect("test AdminConfig");

        let http = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .unwrap();
        let zitadel = Arc::new(crate::zitadel::ZitadelClient::new(cfg.clone(), http.clone()));
        let jwks = JwksCache::new(ZitadelConfig {
            issuer: cfg.issuer.clone(),
            audience: vec![cfg.audience.clone()],
            jwks_uri: format!("{}/oauth/v2/keys", cfg.issuer),
            project_id: cfg.project_id.clone(),
        });
        let state = crate::AppState { cfg, jwks, zitadel, http, app_codes: std::sync::Arc::new(vec![]) };
        let session_layer = SessionManagerLayer::new(MemoryStore::default())
            .with_name("id");
        crate::api::router(state).layer(session_layer)
    }

    #[tokio::test]
    async fn usage_route_requires_operator() {
        use tower::ServiceExt;
        // building the router and calling GET /api/usage without a session cookie
        // returns 401 (same harness the other gated routes use).
        let app = test_router_no_session();
        let res = app.oneshot(
            axum::http::Request::builder().uri("/api/usage").body(axum::body::Body::empty()).unwrap()
        ).await.unwrap();
        assert_eq!(res.status(), axum::http::StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn usage_daily_route_requires_operator() {
        use tower::ServiceExt;
        let app = test_router_no_session();
        let res = app.oneshot(
            axum::http::Request::builder().uri("/api/usage-daily").body(axum::body::Body::empty()).unwrap()
        ).await.unwrap();
        assert_eq!(res.status(), axum::http::StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn project_app_routes_require_operator() {
        use tower::ServiceExt;
        let cases: &[(&str, &str)] = &[
            ("POST", "/api/projects/p1/apps"),
            ("GET", "/api/projects/p1/apps/a1"),
            ("PUT", "/api/projects/p1/apps/a1"),
            ("DELETE", "/api/projects/p1/apps/a1"),
            ("POST", "/api/projects/p1/apps/a1/secret"),
        ];
        for (method, uri) in cases {
            let app = test_router_no_session();
            let res = app
                .oneshot(
                    axum::http::Request::builder()
                        .method(*method)
                        .uri(*uri)
                        .body(axum::body::Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(
                res.status(),
                axum::http::StatusCode::UNAUTHORIZED,
                "{method} {uri} must be Operator-gated (401), got {}",
                res.status()
            );
        }
    }

    #[tokio::test]
    async fn user_files_route_requires_operator() {
        use tower::ServiceExt;
        let app = test_router_no_session();
        let res = app
            .oneshot(
                axum::http::Request::builder()
                    .uri("/api/users/test-id/files")
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), axum::http::StatusCode::UNAUTHORIZED);
    }
}
