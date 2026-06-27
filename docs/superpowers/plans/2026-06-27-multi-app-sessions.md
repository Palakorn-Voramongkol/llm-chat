# Multi-application Sessions page — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Give the admin-web `/sessions` page a registry-backed application selector so its chat panels re-scope to a chosen application, with one app (llm-chat) today and adding more being config-only.

**Architecture:** admin-api gains an app registry (`SessionApp` list) parsed from `MANAGER_CONTROL_APPS` JSON, falling back to a single synthesized llm-chat entry from the legacy `MANAGER_CONTROL_URL` + `ZITADEL_PROJECT_ID`. A new `GET /api/session-apps` feeds the picker; `GET /api/chat-sessions?app=<key>` queries the selected app's manager with a per-project SA token. The Sessions page adds a shadcn `Select` and relabels sign-ins "all applications".

**Tech Stack:** Rust (admin-api: axum, serde, reqwest, tokio-tungstenite), Next.js 16 / React 19 / TypeScript / shadcn-ui (admin-web), vitest.

## Global Constraints

- **Tests — admin-api:** `cargo test -p llm-chat-admin-api` (from `D:\projects\llm-chat`).
- **Tests — admin-web:** `npx vitest run`, `npx tsc --noEmit`, `npx eslint <file>` (run from `D:\projects\llm-chat\admin-web`).
- **Shared branch `feat/app-code`:** `git add` EXPLICIT paths only — never `-A`/`.`. Never merge/discard the branch in a task.
- **Backend owns the logic (CLAUDE.md):** the registry, the app list, and token/manager selection live in admin-api; the client sends only the opaque `app` key and renders `{key,name}`. `controlUrl`/`projectId` are NEVER sent to the client.
- **Fail closed:** an unknown `?app=` key is a 400, never substituted with another app. A malformed `MANAGER_CONTROL_APPS` JSON aborts startup (fail fast), naming the var.
- **Don't break existing endpoints:** `usage` / `usage-daily` / `user files` keep identical behavior, sourced from the registry's **default** (first) entry.
- **Sign-ins stay platform-wide** (Zitadel audit isn't project-filterable) — only relabeled.

---

## File Structure

**admin-api**
- `src/config.rs` — `SessionApp` type, `parse_session_apps`, `default_app`, `find_app`, `session_apps` on `AdminConfig`; remove the standalone `manager_control_url`.
- `src/zitadel/token.rs` — `chat_token_scope(project_id)` (pure) + `mint_chat_token(project_id)`.
- `src/api/mod.rs` — `/api/session-apps` route + `session_apps_json` shaper; `chat_sessions` gains `?app=`; `usage`/`usage_daily`/`user_files`/`status` use the default entry.
- `src/auth.rs` — update the `test_cfg()` fixture for the struct change.

**admin-web**
- `lib/session-apps.ts` + `lib/session-apps.test.ts` — pure `chatSessionsUrl(app)` helper (the unit-testable bit).
- `lib/types.ts` — `SessionApp` / `SessionAppList`.
- `app/(dash)/sessions/page.tsx` — picker + per-app fetch + sign-ins relabel.

**config**
- `.env.local.example` — document `MANAGER_CONTROL_APPS`.

---

## Task 1: admin-api — app registry in config (pure, additive)

**Files:**
- Modify: `admin-api/src/config.rs` (add types/functions/field + tests)
- Modify: `admin-api/src/auth.rs:216-231` (test fixture)

**Interfaces:**
- Produces:
  - `pub struct SessionApp { pub key: String, pub name: String, pub control_url: String, pub project_id: String }`
  - `pub fn parse_session_apps(manager_control_apps: Option<&str>, legacy_url: Option<&str>, legacy_project_id: &str) -> Result<Vec<SessionApp>, String>`
  - `pub fn default_app(apps: &[SessionApp]) -> Option<&SessionApp>`
  - `pub fn find_app<'a>(apps: &'a [SessionApp], key: &str) -> Option<&'a SessionApp>`
  - `AdminConfig.session_apps: Vec<SessionApp>`

- [ ] **Step 1: Write the failing tests**

Add to the `#[cfg(test)] mod tests` in `admin-api/src/config.rs` (before the closing `}`):

```rust
    #[test]
    fn parse_session_apps_reads_json_array() {
        let json = r#"[
            {"key":"llm-chat","name":"llm-chat","controlUrl":"ws://m:7777/control","projectId":"p1"},
            {"key":"app2","name":"App Two","controlUrl":"ws://m2:7777/control","projectId":"p2"}
        ]"#;
        let apps = parse_session_apps(Some(json), None, "ignored").expect("ok");
        assert_eq!(apps.len(), 2);
        assert_eq!(apps[0], SessionApp {
            key: "llm-chat".into(), name: "llm-chat".into(),
            control_url: "ws://m:7777/control".into(), project_id: "p1".into(),
        });
        assert_eq!(apps[1].key, "app2");
        assert_eq!(apps[1].project_id, "p2");
    }

    #[test]
    fn parse_session_apps_falls_back_to_legacy_single_entry() {
        let apps = parse_session_apps(None, Some("ws://m:7777/control"), "p1").expect("ok");
        assert_eq!(apps, vec![SessionApp {
            key: "llm-chat".into(), name: "llm-chat".into(),
            control_url: "ws://m:7777/control".into(), project_id: "p1".into(),
        }]);
    }

    #[test]
    fn parse_session_apps_empty_when_nothing_configured() {
        assert_eq!(parse_session_apps(None, None, "p1").expect("ok"), vec![]);
        assert_eq!(parse_session_apps(Some("   "), Some("  "), "p1").expect("ok"), vec![]);
    }

    #[test]
    fn parse_session_apps_drops_entries_missing_a_field() {
        // second entry has no projectId -> dropped (never defaulted).
        let json = r#"[
            {"key":"ok","name":"OK","controlUrl":"ws://m/control","projectId":"p1"},
            {"key":"bad","name":"Bad","controlUrl":"ws://m/control"}
        ]"#;
        let apps = parse_session_apps(Some(json), None, "p1").expect("ok");
        assert_eq!(apps.len(), 1);
        assert_eq!(apps[0].key, "ok");
    }

    #[test]
    fn parse_session_apps_errors_on_malformed_json() {
        let err = parse_session_apps(Some("not json"), None, "p1").unwrap_err();
        assert!(err.contains("MANAGER_CONTROL_APPS"));
    }

    #[test]
    fn default_and_find_app() {
        let apps = parse_session_apps(
            Some(r#"[{"key":"a","name":"A","controlUrl":"u","projectId":"p"}]"#), None, "x",
        ).expect("ok");
        assert_eq!(default_app(&apps).unwrap().key, "a");
        assert_eq!(find_app(&apps, "a").unwrap().key, "a");
        assert!(find_app(&apps, "nope").is_none());
        assert!(default_app(&[]).is_none());
    }
```

- [ ] **Step 2: Run them to verify they fail**

Run: `cargo test -p llm-chat-admin-api parse_session_apps`
Expected: FAIL to COMPILE — `cannot find function parse_session_apps` / `cannot find type SessionApp`.

- [ ] **Step 3: Add the types + functions**

In `admin-api/src/config.rs`, after the `parse_cookie_secure` function (around line 26), add:

```rust
/// One chat-capable application in the Sessions registry. `project_id` is the
/// Zitadel project whose audience the SA token must target (so that app's
/// manager accepts it) and whose roles it asserts.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SessionApp {
    pub key: String,
    pub name: String,
    pub control_url: String,
    pub project_id: String,
}

/// PURE: build the chat-app registry. Prefers `MANAGER_CONTROL_APPS` (a JSON
/// array of `{key,name,controlUrl,projectId}`); an entry missing any field is
/// dropped (never defaulted). Malformed JSON is a hard error (fail fast). If the
/// var is absent/blank, falls back to ONE llm-chat entry synthesized from the
/// legacy `MANAGER_CONTROL_URL` + the admin project id. Absent both → empty.
pub fn parse_session_apps(
    manager_control_apps: Option<&str>,
    legacy_url: Option<&str>,
    legacy_project_id: &str,
) -> Result<Vec<SessionApp>, String> {
    #[derive(serde::Deserialize)]
    struct Raw {
        key: Option<String>,
        name: Option<String>,
        #[serde(rename = "controlUrl")]
        control_url: Option<String>,
        #[serde(rename = "projectId")]
        project_id: Option<String>,
    }
    let nonempty = |s: Option<String>| s.map(|x| x.trim().to_string()).filter(|x| !x.is_empty());
    if let Some(j) = manager_control_apps.map(str::trim).filter(|s| !s.is_empty()) {
        let raws: Vec<Raw> = serde_json::from_str(j)
            .map_err(|e| format!("MANAGER_CONTROL_APPS is not valid JSON: {e}"))?;
        return Ok(raws
            .into_iter()
            .filter_map(|r| {
                Some(SessionApp {
                    key: nonempty(r.key)?,
                    name: nonempty(r.name)?,
                    control_url: nonempty(r.control_url)?,
                    project_id: nonempty(r.project_id)?,
                })
            })
            .collect());
    }
    if let Some(url) = legacy_url.map(str::trim).filter(|s| !s.is_empty()) {
        return Ok(vec![SessionApp {
            key: "llm-chat".to_string(),
            name: "llm-chat".to_string(),
            control_url: url.to_string(),
            project_id: legacy_project_id.to_string(),
        }]);
    }
    Ok(Vec::new())
}

/// The default app (the first registry entry) — used by the non-selectable
/// endpoints (usage, usage-daily, user files, and chat-sessions with no `?app=`).
pub fn default_app(apps: &[SessionApp]) -> Option<&SessionApp> {
    apps.first()
}

/// Resolve a registry entry by its `key`.
pub fn find_app<'a>(apps: &'a [SessionApp], key: &str) -> Option<&'a SessionApp> {
    apps.iter().find(|a| a.key == key)
}
```

- [ ] **Step 4: Add the `session_apps` field + populate it (keep `manager_control_url` for now)**

In the `AdminConfig` struct (after the `manager_control_url` field, around line 51) add:

```rust
    /// Chat-capable applications for the Sessions page (registry). The first
    /// entry is the default used by the non-selectable endpoints.
    pub session_apps: Vec<SessionApp>,
```

In `from_map` (around lines 59-85), compute the project id as a local BEFORE the struct literal and add the field. Replace the existing `project_id: require_var(...)?,` line by introducing a local and reusing it:

```rust
        let project_id = require_var("ZITADEL_PROJECT_ID", get("ZITADEL_PROJECT_ID"))?;
        let session_apps = parse_session_apps(
            get("MANAGER_CONTROL_APPS").as_deref(),
            get("MANAGER_CONTROL_URL").as_deref(),
            &project_id,
        )?;
        Ok(AdminConfig {
            issuer,
            project_id: project_id.clone(),
            audience: require_var("ZITADEL_AUDIENCE", get("ZITADEL_AUDIENCE"))?,
            sa_key_path: require_var("ADMIN_SA_KEY_PATH", get("ADMIN_SA_KEY_PATH"))?,
            oidc_client_id: require_var("ADMIN_OIDC_CLIENT_ID", get("ADMIN_OIDC_CLIENT_ID"))?,
            oidc_client_secret: require_var(
                "ADMIN_OIDC_CLIENT_SECRET",
                get("ADMIN_OIDC_CLIENT_SECRET"),
            )?,
            bind_addr: require_var("ADMIN_BIND_ADDR", get("ADMIN_BIND_ADDR"))?,
            public_origin,
            allowed_origin: require_var("ADMIN_ALLOWED_ORIGIN", get("ADMIN_ALLOWED_ORIGIN"))?,
            session_key: require_var("ADMIN_SESSION_KEY", get("ADMIN_SESSION_KEY"))?,
            cookie_secure: parse_cookie_secure(get("ADMIN_COOKIE_SECURE")),
            manager_control_url: get("MANAGER_CONTROL_URL")
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty()),
            session_apps,
        })
```

- [ ] **Step 5: Fix the `auth.rs` test fixture so it compiles**

In `admin-api/src/auth.rs` `test_cfg()` (around line 229), add the field after `manager_control_url: None,`:

```rust
            manager_control_url: None,
            session_apps: Vec::new(),
```

- [ ] **Step 6: Run the tests**

Run: `cargo test -p llm-chat-admin-api`
Expected: PASS — the 6 new `parse_session_apps`/`default_and_find_app` tests plus all existing admin-api tests.

- [ ] **Step 7: Commit**

```bash
cd /d/projects/llm-chat
git add admin-api/src/config.rs admin-api/src/auth.rs
git commit -m "feat(admin-api): chat-app registry (SessionApp + parse_session_apps)

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Task 2: admin-api — per-project token + route default endpoints through the registry

**Files:**
- Modify: `admin-api/src/zitadel/token.rs` (mint signature + pure scope helper)
- Modify: `admin-api/src/api/mod.rs:214-298` (status capability + usage/usage_daily/user_files)
- Modify: `admin-api/src/config.rs` (remove `manager_control_url` field + from_map line)
- Modify: `admin-api/src/auth.rs` (remove the field from `test_cfg`)

**Interfaces:**
- Consumes (Task 1): `default_app`, `SessionApp`.
- Produces:
  - `pub fn chat_token_scope(project_id: &str) -> String`
  - `pub async fn mint_chat_token(&self, project_id: &str) -> Result<String, ZitadelError>`

- [ ] **Step 1: Write the failing test for the pure scope helper**

In `admin-api/src/zitadel/token.rs`, in its `#[cfg(test)] mod tests`, add:

```rust
    #[test]
    fn chat_token_scope_targets_the_given_project() {
        let s = super::chat_token_scope("999");
        assert_eq!(
            s,
            "openid urn:zitadel:iam:org:project:id:999:aud urn:zitadel:iam:org:projects:roles"
        );
    }
```

- [ ] **Step 2: Run it to verify it fails**

Run: `cargo test -p llm-chat-admin-api chat_token_scope`
Expected: FAIL to COMPILE — `cannot find function chat_token_scope`.

- [ ] **Step 3: Extract the scope helper + parameterize `mint_chat_token`**

In `admin-api/src/zitadel/token.rs`, add the pure helper near the top of the file (after the imports / module doc):

```rust
/// PURE: the manager-token scope requesting `project_id`'s audience + project
/// roles (the jwt-bearer flow the python machine client uses).
pub fn chat_token_scope(project_id: &str) -> String {
    format!(
        "openid urn:zitadel:iam:org:project:id:{project_id}:aud urn:zitadel:iam:org:projects:roles"
    )
}
```

Change `mint_chat_token` (around line 107) to take `project_id` and use the helper. Replace its signature and the inline `scope` build:

```rust
    pub async fn mint_chat_token(&self, project_id: &str) -> Result<String, ZitadelError> {
        let raw = std::fs::read_to_string(&self.cfg.sa_key_path)
            .map_err(|e| ZitadelError::Transport(format!("read sa key: {e}")))?;
        let sa: serde_json::Value = serde_json::from_str(&raw)
            .map_err(|e| ZitadelError::Invalid(format!("sa key json: {e}")))?;
        let user_id = sa["userId"].as_str().unwrap_or_default();
        let key_id = sa["keyId"].as_str().unwrap_or_default();
        let pem = sa["key"].as_str().unwrap_or_default();
        let assertion = build_assertion(user_id, key_id, pem, &self.cfg.issuer, now_secs())
            .map_err(ZitadelError::Invalid)?;
        let scope = chat_token_scope(project_id);
        let url = format!("{}/oauth/v2/token", self.cfg.issuer);
```

(Leave the rest of the function body unchanged.)

- [ ] **Step 4: Route the non-selectable endpoints through the default app**

In `admin-api/src/api/mod.rs`:

`status` (around line 225) — change the capability flag:

```rust
            "chatSessions": !st.cfg.session_apps.is_empty(),
```

`usage` (around lines 253-263) — replace the body:

```rust
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
```

`usage_daily` (around lines 267-277) — replace the body:

```rust
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
```

`user_files` (around lines 282-298) — replace the prelude (the `let Some(url) = …` and the `mint_chat_token()` + `control_request(&url, …)`):

```rust
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
```

- [ ] **Step 5: Remove the now-unused `manager_control_url` field**

`admin-api/src/config.rs` — delete the field from the struct (around line 51):

```rust
    pub manager_control_url: Option<String>,
```

and delete its assignment in `from_map` (the `manager_control_url: get("MANAGER_CONTROL_URL")…filter(|s| !s.is_empty()),` block added/kept in Task 1 Step 4).

`admin-api/src/auth.rs` — delete `manager_control_url: None,` from `test_cfg()` (leaving `session_apps: Vec::new(),`).

- [ ] **Step 6: Build + test**

Run: `cargo test -p llm-chat-admin-api`
Expected: PASS, clean build. (`chat_token_scope` test green; no references to `manager_control_url` remain — verify with `rg -n "manager_control_url" admin-api/src` → no output.)

- [ ] **Step 7: Commit**

```bash
cd /d/projects/llm-chat
git add admin-api/src/zitadel/token.rs admin-api/src/api/mod.rs admin-api/src/config.rs admin-api/src/auth.rs
git commit -m "feat(admin-api): per-project chat token; default-app routing; drop manager_control_url

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Task 3: admin-api — `/api/session-apps` + `chat-sessions?app=`

**Files:**
- Modify: `admin-api/src/api/mod.rs` (route, `session_apps` handler, `session_apps_json`, `chat_sessions` query)

**Interfaces:**
- Consumes: `crate::config::{SessionApp, default_app, find_app}`, `crate::manager::{control_query, combine_control_replies}`, `ApiError::BadRequest`.
- Produces: `GET /api/session-apps` → `{ "apps": [{key,name}] }`; `GET /api/chat-sessions?app=<key>`.

- [ ] **Step 1: Write the failing test for the pure shaper**

In `admin-api/src/api/mod.rs`, add to its `#[cfg(test)] mod tests` (find the existing `mod tests` near the end; if none in this file, add one):

```rust
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
```

> If `admin-api/src/api/mod.rs` has no `#[cfg(test)] mod tests`, add at end of file:
> ```rust
> #[cfg(test)]
> mod tests {
>     use super::*;
>     // (test above goes here)
> }
> ```

- [ ] **Step 2: Run it to verify it fails**

Run: `cargo test -p llm-chat-admin-api session_apps_json`
Expected: FAIL to COMPILE — `cannot find function session_apps_json`.

- [ ] **Step 3: Add the shaper + handler + route**

In `admin-api/src/api/mod.rs`, add the pure shaper + handler near `chat_sessions` (after the `chat_sessions` function):

```rust
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
```

Register the route in `router()` (after the `/api/chat-sessions` line, ~69):

```rust
        .route("/api/session-apps", get(session_apps))
```

- [ ] **Step 4: Add `?app=` to `chat_sessions`**

Add the query struct (near the other `…Query` structs, e.g. above `chat_sessions`):

```rust
#[derive(Deserialize)]
struct ChatSessionsQuery { app: Option<String> }
```

Replace the `chat_sessions` function (around lines 233-249) with:

```rust
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
    let clients = crate::manager::control_query(&app.control_url, &token, "clients")
        .await
        .unwrap_or_else(|e| json!({ "ok": false, "error": e }));
    Ok(Json(crate::manager::combine_control_replies(list, instances, clients)))
}
```

- [ ] **Step 5: Build + test**

Run: `cargo test -p llm-chat-admin-api`
Expected: PASS (incl. `session_apps_json_exposes_only_key_and_name`); clean build.

- [ ] **Step 6: Commit**

```bash
cd /d/projects/llm-chat
git add admin-api/src/api/mod.rs
git commit -m "feat(admin-api): /api/session-apps + chat-sessions?app= (fail-closed unknown)

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Task 4: admin-web — Sessions page selector + sign-ins relabel

**Files:**
- Create: `admin-web/lib/session-apps.ts`, `admin-web/lib/session-apps.test.ts`
- Modify: `admin-web/lib/types.ts` (add `SessionApp` / `SessionAppList`)
- Modify: `admin-web/app/(dash)/sessions/page.tsx`

**Interfaces:**
- Consumes: `GET /api/session-apps` → `{ apps: {key,name}[] }`; `GET /api/chat-sessions?app=<key>`.
- Produces: `chatSessionsUrl(app: string): string`; `interface SessionApp { key: string; name: string }`, `interface SessionAppList { apps: SessionApp[] }`.

- [ ] **Step 1: Write the failing test for the URL helper**

Create `admin-web/lib/session-apps.test.ts`:

```ts
import { describe, it, expect } from "vitest";
import { chatSessionsUrl } from "./session-apps";

describe("chatSessionsUrl", () => {
  it("appends ?app= when an app is selected", () => {
    expect(chatSessionsUrl("llm-chat")).toBe("/api/chat-sessions?app=llm-chat");
  });
  it("url-encodes the app key", () => {
    expect(chatSessionsUrl("app two")).toBe("/api/chat-sessions?app=app%20two");
  });
  it("omits the param when no app is selected", () => {
    expect(chatSessionsUrl("")).toBe("/api/chat-sessions");
  });
});
```

- [ ] **Step 2: Run it to verify it fails**

Run (from `admin-web`): `npx vitest run lib/session-apps.test.ts`
Expected: FAIL — cannot resolve `./session-apps`.

- [ ] **Step 3: Implement the helper + types**

Create `admin-web/lib/session-apps.ts`:

```ts
/** Build the chat-sessions API URL, scoped to a selected application key.
 * Empty key → the backend's default app (no `?app=`). */
export function chatSessionsUrl(app: string): string {
  return app ? `/api/chat-sessions?app=${encodeURIComponent(app)}` : "/api/chat-sessions";
}
```

Add to `admin-web/lib/types.ts` (near the other list types):

```ts
export interface SessionApp {
  key: string;
  name: string;
}
export interface SessionAppList {
  apps: SessionApp[];
}
```

- [ ] **Step 4: Run it to verify it passes**

Run: `npx vitest run lib/session-apps.test.ts`
Expected: PASS (3 tests).

- [ ] **Step 5: Wire the picker into the Sessions page**

In `admin-web/app/(dash)/sessions/page.tsx`:

1. Add imports (after the existing `@/components` imports):

```tsx
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { chatSessionsUrl } from "@/lib/session-apps";
```

and extend the types import to include `SessionAppList`:

```tsx
import type { ChatSessions, SigninList, Status, UserList, SessionAppList } from "@/lib/types";
```

2. Add state inside `SessionsPage` (next to the other `useState`s):

```tsx
  const [apps, setApps] = useState<{ key: string; name: string }[]>([]);
  const [selectedApp, setSelectedApp] = useState("");
```

3. Fetch the app list once — add this effect ABOVE the existing `useEffect(() => { load(); }, [load])`:

```tsx
  useEffect(() => {
    api.get<SessionAppList>("/api/session-apps")
      .then((r) => {
        const list = r.apps ?? [];
        setApps(list);
        setSelectedApp((cur) => cur || (list[0]?.key ?? ""));
      })
      .catch(() => setApps([]));
  }, []);
```

4. In `load`, scope the chat-sessions fetch to the selected app and add `selectedApp` to the deps. Replace the chat fetch line:

```tsx
      setChat(await api.get<ChatSessions>(chatSessionsUrl(selectedApp)));
```

and change the `useCallback` dependency array from `[]` to `[selectedApp]`:

```tsx
  }, [selectedApp]);
```

5. Add the picker to the `PageHeader`. Replace the existing `<PageHeader … />` (the `title="Sessions"` one) with:

```tsx
      <PageHeader
        title="Sessions"
        description="Live activity across the platform — who's signed in, who's chatting, and worker health."
        actions={
          apps.length > 0 ? (
            <Select value={selectedApp} onValueChange={setSelectedApp}>
              <SelectTrigger className="h-8 w-44" aria-label="Select application">
                <SelectValue placeholder="Application" />
              </SelectTrigger>
              <SelectContent>
                {apps.map((a) => (
                  <SelectItem key={a.key} value={a.key}>{a.name}</SelectItem>
                ))}
              </SelectContent>
            </Select>
          ) : (
            <span className="text-muted-foreground text-xs">No chat applications configured</span>
          )
        }
      />
```

6. Relabel the sign-ins card heading. Find `<h2 className="text-sm font-semibold">Recent sign-ins</h2>` and change it to:

```tsx
          <h2 className="text-sm font-semibold">Recent sign-ins — all applications</h2>
```

- [ ] **Step 6: Typecheck, lint, test**

Run (from `admin-web`):
- `npx tsc --noEmit` → exit 0.
- `npx eslint "app/(dash)/sessions/page.tsx" lib/session-apps.ts` → no NEW errors (the page's pre-existing `react-hooks/set-state-in-effect` on `load()` may remain; introduce none beyond it).
- `npx vitest run` → all pass (incl. the 3 new `chatSessionsUrl` tests).

- [ ] **Step 7: Commit**

```bash
cd /d/projects/llm-chat
git add "admin-web/lib/session-apps.ts" "admin-web/lib/session-apps.test.ts" "admin-web/lib/types.ts" "admin-web/app/(dash)/sessions/page.tsx"
git commit -m "feat(admin-web): application selector on the Sessions page

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Task 5: config — document `MANAGER_CONTROL_APPS`

**Files:**
- Modify: `.env.local.example` (admin-api section, near `MANAGER_CONTROL_URL` ~line 39)

**Interfaces:** Consumes `parse_session_apps` (Task 1).

- [ ] **Step 1: Add the documented example**

In `.env.local.example`, just after the `MANAGER_CONTROL_URL=ws://manager:7777/control` line (in the `# ── admin-api ──` block), add:

```
# Multi-application Sessions page (optional). A JSON array of chat-capable apps;
# each app's manager /control + the Zitadel project whose audience the SA token
# targets. When unset, the single MANAGER_CONTROL_URL above is used as the lone
# "llm-chat" entry (today's behavior). Add entries here to populate the picker.
# MANAGER_CONTROL_APPS=[{"key":"llm-chat","name":"llm-chat","controlUrl":"ws://manager:7777/control","projectId":"<llm-chat project id>"}]
```

- [ ] **Step 2: Commit**

```bash
cd /d/projects/llm-chat
git add .env.local.example
git commit -m "docs(config): document MANAGER_CONTROL_APPS for the Sessions picker

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Task 6: End-to-end verification + finish

**Files:** none (build + manual verification).

- [ ] **Step 1: Rebuild admin-api + admin-web**

Run: `docker compose up -d --build --no-deps admin-api admin-web`
Expected: both images rebuilt + restarted, no errors. (`.env.local` already has `MANAGER_CONTROL_URL`, so the registry synthesizes the single llm-chat entry with zero new config.)

- [ ] **Step 2: Verify the endpoints**

Run:
```bash
curl -s -o /dev/null -w "%{http_code}\n" http://localhost:3000/api/session-apps     # expect 401 (unauth) or 200 if a session cookie is present
curl -s -o /dev/null -w "%{http_code}\n" http://localhost:3000/sessions             # expect 200
```
Expected: the routes resolve (401/200, not 404). A 404 means the route didn't register.

- [ ] **Step 3: Visual check (browser, logged in as an operator)**

Open `http://localhost:3000/sessions` (hard refresh). Expected: an **Application** selector in the header showing **llm-chat** (single option); the chat panels populate from that app's manager; the sign-ins card reads **"Recent sign-ins — all applications"**.

- [ ] **Step 4: Fail-closed check**

Run (logged-in session cookie required, or observe via the browser devtools network tab): request `GET /api/chat-sessions?app=bogus` → expect **400** (`unknown application: bogus`), NOT a fallback to llm-chat.

- [ ] **Step 5: Finish the branch**

Announce and use **superpowers:finishing-a-development-branch**: verify `cargo test -p llm-chat-admin-api` and (from `admin-web`) `npx vitest run` + `npx tsc --noEmit` pass, then present the standard options. Shared branch — explicit-path commits only; if merging to `main`, use the throwaway-worktree + cherry-pick flow.

---

## Self-Review

**Spec coverage:**
- Registry (`SessionApp`, `parse_session_apps`, JSON + legacy fallback, drop-bad-entry, malformed→error) → Task 1. ✓
- `default_app` for non-selectable endpoints + per-project `mint_chat_token` + remove `manager_control_url` → Task 2. ✓
- `GET /api/session-apps` (key+name only) + `chat-sessions?app=` (default + unknown→400) → Task 3. ✓
- Web picker + per-app fetch + sign-ins relabel + `{key,name}`-only client data → Task 4. ✓
- `MANAGER_CONTROL_APPS` documented; `MANAGER_CONTROL_URL` retained as fallback → Tasks 1 & 5. ✓
- Sign-ins/status unchanged & platform-wide → Tasks 2/3 (untouched signins) + Task 4 relabel. ✓
- Fail-closed unknown app; malformed-config fail-fast → Tasks 3 & 1. ✓
- Security: controlUrl/projectId never leave the server → Task 3 (`session_apps_json` test asserts it). ✓

**Deviation from spec (deliberate):** the spec mentioned a page-level vitest and an optional `?app=` URL sync. The plan tests the pure `chatSessionsUrl` helper instead of a brittle full-page test (matches this repo's lib-unit + manual-e2e style), and omits URL sync (YAGNI; component state only). Both are noted; neither changes behavior the user asked for.

**Placeholder scan:** none — every code step has complete code; every command has an expected result.

**Type consistency:** `SessionApp` fields (`key,name,control_url,project_id`) consistent across config/api; `mint_chat_token(project_id)` consistent in token.rs + all callers (usage/usage_daily/user_files/chat_sessions); `find_app`/`default_app` signatures match call sites; web `SessionAppList { apps: SessionApp{key,name}[] }` matches `session_apps_json` output and the page's `r.apps` read; `chatSessionsUrl` matches its test and the page call.
