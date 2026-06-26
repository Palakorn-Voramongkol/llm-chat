# Backend-owned `/status` and `whoami` Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Move all identity resolution and `/status`/`whoami` rendering out of the chat clients and into the manager, so the clients send a request and print the server's finished text.

**Architecture:** Add a new chat.user-gated `/identity` WebSocket endpoint to the manager that spawns no worker and opens no chat session. The client sends only its own local facts (kind/version, render mode, timeout, manager URL, its `/chat` session id + message count); the manager resolves the human identity (display name via Zitadel `/userinfo` using the user's own token, roles + email from the verified JWT, project/issuer from manager config) and renders the full block, returning text the client prints verbatim. The box-drawing renderers and the JWT-decoding identity logic are deleted from both the rust and python clients.

**Tech Stack:** Rust (manager `llm-chat-manager`, client `llm-chat-client`), Python (`llm_chat` client), Zitadel OIDC, `reqwest` (already a manager dep), `tokio-tungstenite`, `websockets` (python).

## Global Constraints

- **Tests — Rust manager:** `cargo test -p llm-chat-manager` (run from `D:\projects\llm-chat`).
- **Tests — Rust client:** `cargo test -p llm-chat-client`.
- **Tests — Python client:** `python -m pytest` run from `D:\projects\llm-chat\clients\python` (the venv there; `pytest -q`).
- **Shared branch discipline:** the branch `feat/app-code` is shared with another concurrent session. `git add` EXPLICIT paths only — never `git add -A`/`.`. Never merge/discard the branch as part of a task.
- **Keep rust + python clients byte-identical in output.** The `/status` block and the `whoami` line(s) are now rendered by the manager (one renderer), so both clients print the SAME server text. Only the client `kind` field (`"rust"` vs `"python"`) differs, and it is supplied by the client in its request payload.
- **The `/status` block layout is byte-for-byte the existing layout** (see Task 1 — it is copied verbatim from the current `format_status`). Do not change spacing, the `·` separators, or the rule width.
- **Fail closed (security rule, CLAUDE.md):** `/identity` is gated by the SAME handshake as every manager endpoint (verified Zitadel JWT + `chat.user`). The user's access token is used ONLY for that user's own `/userinfo` call, within the request, and is never logged or retained past the connection. No admin API is touched. The client does NO identity decoding and NO fallback to client-side decode — if the backend is unreachable, it prints an error.
- **`/userinfo` is best-effort DISPLAY only** (never authz): on any HTTP/JSON failure the manager falls back to the verified-JWT email, then the `sub`. It never errors `/status`.
- **YAGNI — no identity cache in v1.** `/status` and `whoami` are rare, human-initiated commands; a single `/userinfo` GET per call is cheap. The `sub → name` cache mentioned in the spec is deliberately deferred. State this in the commit body for Task 2.
- **Scope:** only `/status` (REPL) + `whoami`/`login` (CLI) change. `/usage` and `/dir` rendering stay in the clients — they format already-resolved structured data (counts, sizes, tree), not identity, and are out of scope.

---

## File Structure

**Manager (`manager/src/main.rs`)** — one file (the manager is a single large binary; follow the existing pattern):
- New fields on `ManagerState`: `http: reqwest::Client`, `issuer: Option<String>`, `project_id: Option<String>`, `project_name: Option<String>`.
- Handshake captures the verified access token + email into holders (next to the existing user_id/roles holders).
- New `/identity` branch in the post-handshake path dispatch (placed FIRST, before `/control`).
- New `handle_identity(...)` async handler.
- New pure functions: `parse_status_client`, `format_status_block`, `format_whoami_line`, `user_label_from_userinfo`; one IO function `resolve_user_label`.

**Rust client:**
- `clients/rust/src/config.rs` — add `identity_url(manager_ws) -> String`.
- `clients/rust/src/protocol.rs` — add `request_identity(...)` free fn + `ChatClient::token_provider()` getter.
- `clients/rust/src/repl.rs` — `/status` calls `request_identity`, prints `block`. Delete `format_status` + its tests + the `identity_from_token` import. `ReplCtx` loses `issuer`/`project`, gains `identity_url`.
- `clients/rust/src/cli.rs` — `whoami`/`login` route through `/identity`; delete `identity_from_token`, `print_whoami`, `decode_claims` + their tests; update `ReplCtx` construction.

**Python client:**
- `clients/python/llm_chat/config.py` — add `identity_url(manager_ws) -> str`.
- `clients/python/llm_chat/protocol.py` — add `request_identity(...)` module fn.
- `clients/python/llm_chat/repl.py` — `/status` via `request_identity`; delete `format_status`; `ReplCtx` loses `issuer`/`project`, gains `identity_url`.
- `clients/python/llm_chat/cli.py` — `whoami`/`login` via `/identity`; delete `_identity_from_token`, `_print_whoami`, `_decode_claims`; update `ReplCtx` construction.
- `clients/python/tests/test_repl.py`, `clients/python/tests/test_cli_resolver.py` — drop deleted-function tests; add `identity_url` test.

**Config:**
- `.env.local.example` — add `MANAGER_PROJECT_NAME=llm-chat` under the manager section.

---

## Task 1: Manager — pure identity renderers + label shaper

**Files:**
- Modify: `manager/src/main.rs` (add pure functions + their `#[cfg(test)]` tests near the existing pure-function tests).

**Interfaces:**
- Produces (used by Task 2):
  - `struct StatusClient { kind: String, version: String, auth_label: String, render_mode: String, timeout_secs: u64, manager_url: String, connected: bool, session_id: Option<String>, msgs_this_session: u64 }`
  - `fn parse_status_client(v: &serde_json::Value) -> StatusClient`
  - `fn format_status_block(c: &StatusClient, who: &str, sub: &str, roles: &[String], issuer: &str, project: &str) -> String`
  - `fn format_whoami_line(who: &str, sub: &str, roles: &[String]) -> String`
  - `fn user_label_from_userinfo(v: &serde_json::Value, email_fallback: Option<&str>, sub: &str) -> String`

- [ ] **Step 1: Write the failing tests**

Add this module near the other `#[cfg(test)]` blocks in `manager/src/main.rs` (e.g. after the existing `compose_own_usage_reply` tests). It references functions that don't exist yet, so it won't compile — that's the failing state.

```rust
#[cfg(test)]
mod identity_tests {
    use super::*;

    fn sample_client() -> serde_json::Value {
        serde_json::json!({
            "type": "status",
            "client": {
                "kind": "rust", "version": "1.0.0",
                "authLabel": "human (browser login)",
                "renderMode": "auto", "timeoutSecs": 120,
                "managerUrl": "ws://m:7777/chat",
                "connected": true, "sessionId": "s1", "msgsThisSession": 2
            }
        })
    }

    #[test]
    fn parse_status_client_reads_all_fields() {
        let c = parse_status_client(&sample_client());
        assert_eq!(c.kind, "rust");
        assert_eq!(c.version, "1.0.0");
        assert_eq!(c.auth_label, "human (browser login)");
        assert_eq!(c.render_mode, "auto");
        assert_eq!(c.timeout_secs, 120);
        assert_eq!(c.manager_url, "ws://m:7777/chat");
        assert!(c.connected);
        assert_eq!(c.session_id.as_deref(), Some("s1"));
        assert_eq!(c.msgs_this_session, 2);
    }

    #[test]
    fn parse_status_client_defaults_missing_fields() {
        // Missing `client` object → display-safe defaults (not a security path).
        let c = parse_status_client(&serde_json::json!({"type": "status"}));
        assert_eq!(c.kind, "?");
        assert_eq!(c.render_mode, "auto");
        assert_eq!(c.timeout_secs, 0);
        assert!(!c.connected);
        assert_eq!(c.session_id, None);
        assert_eq!(c.msgs_this_session, 0);
    }

    #[test]
    fn format_status_block_matches_layout() {
        let c = parse_status_client(&sample_client());
        let roles = vec!["chat.admin".to_string(), "chat.user".to_string()];
        let s = format_status_block(&c, "admin@example.com", "U9", &roles,
                                    "http://iss:8080", "llm-chat");
        assert!(s.contains("client    llm-chat · rust · v1.0.0"));
        assert!(s.contains("auth      human (browser login)"));
        assert!(s.contains("user      admin@example.com"));
        assert!(s.contains("  sub     U9"));
        assert!(s.contains("  roles   chat.admin, chat.user"));
        assert!(s.contains("manager   ws://m:7777/chat · connected"));
        assert!(s.contains("session   s1 · 2 msgs this session"));
        assert!(s.contains("issuer    http://iss:8080"));
        assert!(s.contains("project   llm-chat"));
        assert!(s.contains("display   render=auto · timeout=120s"));
    }

    #[test]
    fn format_status_block_empty_roles_and_no_session() {
        let c = parse_status_client(&serde_json::json!({
            "client": {"kind":"python","version":"1.0.0","authLabel":"machine (kabytech key)",
                       "renderMode":"raw","timeoutSecs":60,"managerUrl":"ws://m:7777/chat",
                       "connected": false, "sessionId": null, "msgsThisSession": 0}
        }));
        let s = format_status_block(&c, "who", "sub", &[], "http://iss", "P123");
        assert!(s.contains("roles   —"));
        assert!(s.contains("session   — · 0 msgs"));
        assert!(s.contains("ws://m:7777/chat · disconnected"));
        assert!(s.contains("render=raw · timeout=60s"));
    }

    #[test]
    fn format_whoami_line_with_and_without_roles() {
        let with = format_whoami_line("a@b.c", "U1", &["chat.user".to_string()]);
        assert_eq!(with, "logged in as a@b.c (sub=U1)\n  roles: chat.user");
        let without = format_whoami_line("a@b.c", "U1", &[]);
        assert_eq!(without, "logged in as a@b.c (sub=U1)");
    }

    #[test]
    fn user_label_prefers_name_then_username_then_email_then_fallback() {
        let v = serde_json::json!({"name":"Jane Doe","preferred_username":"jane","email":"j@x.io"});
        assert_eq!(user_label_from_userinfo(&v, Some("e@x.io"), "U1"), "Jane Doe");
        let v = serde_json::json!({"preferred_username":"jane","email":"j@x.io"});
        assert_eq!(user_label_from_userinfo(&v, Some("e@x.io"), "U1"), "jane");
        let v = serde_json::json!({"email":"j@x.io"});
        assert_eq!(user_label_from_userinfo(&v, Some("e@x.io"), "U1"), "j@x.io");
        let v = serde_json::json!({});
        assert_eq!(user_label_from_userinfo(&v, Some("e@x.io"), "U1"), "e@x.io");
        assert_eq!(user_label_from_userinfo(&v, None, "U1"), "U1");
    }
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p llm-chat-manager identity_tests`
Expected: FAIL to COMPILE — `cannot find function parse_status_client` (and the others).

- [ ] **Step 3: Write the pure functions**

Add this block to `manager/src/main.rs` near the other pure helpers (e.g. just above the existing `compose_own_usage_reply`). The `format_status_block` string is copied verbatim from the deleted client `format_status` so the output is identical.

```rust
// ---------- /identity: pure renderers + label shaper ----------

const STATUS_RULE: &str = "─────────────────────────────────────────────";

/// The client-supplied half of a `/status` request: only facts the client alone
/// knows (its flags + its `/chat` connection). The identity half (who/sub/roles/
/// issuer/project) is server-resolved. Missing fields degrade to display-safe
/// defaults — this is a display surface, not a security boundary.
struct StatusClient {
    kind: String,
    version: String,
    auth_label: String,
    render_mode: String,
    timeout_secs: u64,
    manager_url: String,
    connected: bool,
    session_id: Option<String>,
    msgs_this_session: u64,
}

/// PURE: parse the `client` object of a `status` request into `StatusClient`.
fn parse_status_client(v: &serde_json::Value) -> StatusClient {
    let c = v.get("client").cloned().unwrap_or_else(|| serde_json::json!({}));
    let s = |k: &str| c.get(k).and_then(|x| x.as_str()).unwrap_or("").to_string();
    let str_or = |k: &str, d: &str| {
        let val = c.get(k).and_then(|x| x.as_str()).unwrap_or(d);
        if val.is_empty() { d.to_string() } else { val.to_string() }
    };
    StatusClient {
        kind: str_or("kind", "?"),
        version: str_or("version", "?"),
        auth_label: str_or("authLabel", "—"),
        render_mode: str_or("renderMode", "auto"),
        timeout_secs: c.get("timeoutSecs").and_then(|x| x.as_u64()).unwrap_or(0),
        manager_url: s("managerUrl"),
        connected: c.get("connected").and_then(|x| x.as_bool()).unwrap_or(false),
        session_id: c.get("sessionId").and_then(|x| x.as_str()).map(String::from),
        msgs_this_session: c.get("msgsThisSession").and_then(|x| x.as_u64()).unwrap_or(0),
    }
}

/// PURE: render the full `/status` block. Identity args are server-resolved;
/// `c` is the client-supplied context. Layout is byte-identical to the layout
/// the clients used to render — keep it stable.
fn format_status_block(
    c: &StatusClient,
    who: &str,
    sub: &str,
    roles: &[String],
    issuer: &str,
    project: &str,
) -> String {
    let roles_str = if roles.is_empty() { "—".to_string() } else { roles.join(", ") };
    let conn = if c.connected { "connected" } else { "disconnected" };
    let sid = c.session_id.as_deref().unwrap_or("—");
    format!(
        "─ status ───────────────────────────────────\n\
         \x20client    llm-chat · {kind} · v{version}\n\
         \x20auth      {auth}\n\
         \x20user      {who}\n\
         \x20  sub     {sub}\n\
         \x20  roles   {roles}\n\
         \x20manager   {manager} · {conn}\n\
         \x20session   {sid} · {msgs} msgs this session\n\
         \x20issuer    {issuer}\n\
         \x20project   {project}\n\
         \x20display   render={render} · timeout={timeout}s\n\
         {rule}",
        kind = c.kind, version = c.version, auth = c.auth_label,
        who = who, sub = sub, roles = roles_str, manager = c.manager_url,
        conn = conn, sid = sid, msgs = c.msgs_this_session, issuer = issuer,
        project = project, render = c.render_mode, timeout = c.timeout_secs, rule = STATUS_RULE,
    )
}

/// PURE: render the `whoami` line(s). Matches the old client `print_whoami`.
fn format_whoami_line(who: &str, sub: &str, roles: &[String]) -> String {
    let mut s = format!("logged in as {who} (sub={sub})");
    if !roles.is_empty() {
        s.push_str(&format!("\n  roles: {}", roles.join(", ")));
    }
    s
}

/// PURE: pick a DISPLAY label from a `/userinfo` body, falling back to the
/// verified-JWT email then the `sub`. Never errors (never authz).
fn user_label_from_userinfo(
    v: &serde_json::Value,
    email_fallback: Option<&str>,
    sub: &str,
) -> String {
    v.get("name")
        .and_then(|x| x.as_str())
        .or_else(|| v.get("preferred_username").and_then(|x| x.as_str()))
        .or_else(|| v.get("email").and_then(|x| x.as_str()))
        .map(|s| s.to_string())
        .or_else(|| email_fallback.map(|s| s.to_string()))
        .unwrap_or_else(|| sub.to_string())
}
```

> Note: if `STATUS_RULE` already exists elsewhere in `main.rs`, reuse it instead of redeclaring (the manager currently has no such const — grep `STATUS_RULE` first; declare it only if absent).

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p llm-chat-manager identity_tests`
Expected: PASS (6 tests).

- [ ] **Step 5: Commit**

```bash
cd /d/projects/llm-chat
git add manager/src/main.rs
git commit -m "feat(manager): pure /identity renderers + userinfo label shaper

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Task 2: Manager — `/identity` endpoint, state, handshake token capture

**Files:**
- Modify: `manager/src/main.rs`
  - `struct ManagerState` (around line 954) — add fields.
  - `main()` JWKS setup (around lines 1126-1173) — capture issuer/project_id, read `MANAGER_PROJECT_NAME`, populate new state fields.
  - Handshake holders + callback (around lines 1282-1357) — add token + email capture.
  - Post-handshake dispatch (around lines 1359-1427) — add `/identity` branch FIRST.
  - Add `handle_identity` + `resolve_user_label`.

**Interfaces:**
- Consumes (from Task 1): `parse_status_client`, `format_status_block`, `format_whoami_line`, `user_label_from_userinfo`, `StatusClient`.
- Produces: a live `ws /identity` endpoint returning `{type:"status", block}` and `{type:"whoami", line}`.

- [ ] **Step 1: Add the new `ManagerState` fields**

In `struct ManagerState` (after the `clients: HashMap<String, ClientInfo>,` field, around line 978), add:

```rust
    /// Outbound HTTP for the user's own Zitadel `/userinfo` (display-name
    /// resolution on `/identity`). Reused across requests.
    http: reqwest::Client,
    /// Zitadel issuer (for building the `/userinfo` URL). `None` when Zitadel
    /// auth isn't configured — `/identity` then rejects (no verified user).
    issuer: Option<String>,
    /// The manager's own Zitadel project id (shown on `/status` when no
    /// friendly name is configured).
    project_id: Option<String>,
    /// Friendly project name from `MANAGER_PROJECT_NAME` (else the id is shown).
    project_name: Option<String>,
```

- [ ] **Step 2: Populate the fields in `main()`**

In `main()`, change the JWKS block so the issuer + project id escape the match, and read `MANAGER_PROJECT_NAME`. Replace the `let jwks = match ... ;` assignment (around line 1126) so it is preceded by two `mut` bindings, and set them inside the `Ok(cfg)` arm BEFORE `cfg` is moved into `JwksCache::new`:

```rust
    // Captured from ZitadelConfig before it's consumed by JwksCache::new, so
    // /identity can build the userinfo URL and show the project.
    let mut manager_issuer: Option<String> = None;
    let mut manager_project_id: Option<String> = None;
    let jwks = match zitadel_auth::ZitadelConfig::from_env() {
        Ok(cfg) => {
            manager_issuer = Some(cfg.issuer.clone());
            manager_project_id = Some(cfg.project_id.clone());
            tracing::info!(target: "manager::auth",
                issuer = %cfg.issuer,
                audience = ?cfg.audience,
                project_id = %cfg.project_id,
                "Zitadel auth enabled");
            let cache = zitadel_auth::JwksCache::new(cfg);
            // ...UNCHANGED preload + background refresher + Some(cache)...
```

Leave the rest of the `Ok(cfg)` arm and the `Err(reason)` arm UNCHANGED. Then read the friendly name once (place it just after the `jwks` match, before the `ManagerState` construction):

```rust
    let manager_project_name = std::env::var("MANAGER_PROJECT_NAME")
        .ok()
        .filter(|s| !s.is_empty());
```

Then in the `ManagerState { ... }` literal (around line 1165), add the four fields:

```rust
        clients: HashMap::new(),
        http: reqwest::Client::new(),
        issuer: manager_issuer,
        project_id: manager_project_id,
        project_name: manager_project_name,
```

- [ ] **Step 3: Capture the verified token + email at the handshake**

Add two holders next to the existing ones (after `let roles_holder = ...` / `let roles_capture = ...`, around lines 1285-1289):

```rust
    let token_holder = Arc::new(std::sync::Mutex::new(None::<String>));
    let email_holder = Arc::new(std::sync::Mutex::new(None::<String>));
    let token_capture = token_holder.clone();
    let email_capture = email_holder.clone();
```

Inside the JWKS branch of the callback, right after the existing
`*roles_capture.lock().unwrap() = principal.roles.clone();` (around line 1337), add:

```rust
            *token_capture.lock().unwrap() = Some(token.clone());
            *email_capture.lock().unwrap() = principal.email.clone();
```

(`token` is the bearer string from `extract_bearer`; `principal.email` is `Option<String>`.)

- [ ] **Step 4: Add the `/identity` dispatch branch (placed FIRST)**

After the existing post-handshake bindings (around line 1362, just after the
`tracing::info!(... "post-handshake routing")` line), take the captured token/email
and add the `/identity` branch BEFORE the `if req_path == "/control"` block:

```rust
    let captured_token = token_holder.lock().unwrap().take();
    let captured_email = email_holder.lock().unwrap().take();

    if req_path == "/identity" {
        // Lightweight identity surface: resolve who-am-I + render /status or
        // whoami. No worker, no chat session. chat.user already enforced at the
        // handshake. The user's token is used only for their own /userinfo.
        let uid = match user_id {
            Some(u) => u,
            None => return reject_no_user(ws).await,
        };
        let roles = roles_holder.lock().unwrap().clone();
        let token = match captured_token {
            Some(t) => t,
            None => return reject_no_user(ws).await,
        };
        return handle_identity(ws, state, uid, roles, captured_email, token).await;
    }
    // Not /identity → do not retain the user's access token.
    drop(captured_token);
    drop(captured_email);
```

- [ ] **Step 5: Add the `handle_identity` + `resolve_user_label` functions**

Add after `handle_chat` (or near the other handlers). The handler reads ONE request
frame, resolves identity once, renders, replies, and closes.

```rust
// ---------- /identity ----------

/// IO: GET {issuer}/oidc/v1/userinfo with the user's OWN access token and shape
/// the display label. Best-effort: any failure → email_fallback → sub.
async fn resolve_user_label(
    http: &reqwest::Client,
    issuer: &str,
    access_token: &str,
    email_fallback: Option<&str>,
    sub: &str,
) -> String {
    let url = format!("{}/oidc/v1/userinfo", issuer);
    match http.get(&url).bearer_auth(access_token).send().await {
        Ok(resp) => match resp.json::<serde_json::Value>().await {
            Ok(v) => user_label_from_userinfo(&v, email_fallback, sub),
            Err(_) => email_fallback.unwrap_or(sub).to_string(),
        },
        Err(_) => email_fallback.unwrap_or(sub).to_string(),
    }
}

async fn handle_identity(
    ws: tokio_tungstenite::WebSocketStream<TcpStream>,
    state: SharedState,
    user_id: String,
    roles: Vec<String>,
    email: Option<String>,
    token: String,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let (mut sink, mut stream) = ws.split();

    // One request, then close.
    let text = match tokio::time::timeout(Duration::from_secs(30), stream.next()).await {
        Ok(Some(Ok(Message::Text(t)))) => t,
        Ok(Some(Ok(Message::Binary(b)))) => String::from_utf8_lossy(&b).into_owned(),
        _ => return Ok(()), // closed / no request
    };
    let v: serde_json::Value = match serde_json::from_str(&text) {
        Ok(v) => v,
        Err(e) => {
            let _ = sink
                .send(Message::Text(
                    serde_json::json!({"type":"err","text":format!("bad JSON: {e}")}).to_string(),
                ))
                .await;
            return Ok(());
        }
    };

    let (http, issuer, project_id, project_name) = {
        let st = state.lock().await;
        (st.http.clone(), st.issuer.clone(), st.project_id.clone(), st.project_name.clone())
    };
    let issuer = match issuer {
        Some(i) => i,
        None => {
            let _ = sink
                .send(Message::Text(
                    serde_json::json!({"type":"err","text":"identity unavailable: no issuer configured"}).to_string(),
                ))
                .await;
            return Ok(());
        }
    };

    let who = resolve_user_label(&http, &issuer, &token, email.as_deref(), &user_id).await;
    let mut sorted_roles = roles.clone();
    sorted_roles.sort();
    sorted_roles.dedup();

    let reply = match v.get("type").and_then(|t| t.as_str()) {
        Some("status") => {
            let c = parse_status_client(&v);
            let project = project_name
                .as_deref()
                .or(project_id.as_deref())
                .unwrap_or("—");
            let block = format_status_block(&c, &who, &user_id, &sorted_roles, &issuer, project);
            serde_json::json!({"type":"status","block": block})
        }
        Some("whoami") => {
            let line = format_whoami_line(&who, &user_id, &sorted_roles);
            serde_json::json!({"type":"whoami","line": line})
        }
        other => serde_json::json!({
            "type":"err",
            "text": format!("unknown identity request type {:?}; expected \"status\" or \"whoami\"", other),
        }),
    };

    let _ = sink.send(Message::Text(reply.to_string())).await;
    let _ = sink.send(Message::Close(None)).await;
    Ok(())
}
```

> `Duration`, `Message`, `StreamExt`/`SinkExt`, `serde_json`, `TcpStream`, `SharedState` are already imported/in scope in `main.rs` (used by `handle_chat`). If the compiler reports `Duration` not in scope at this position, add `use std::time::Duration;` at the top of the function (as `handle_chat` does).

- [ ] **Step 6: Build + run the manager tests**

Run: `cargo test -p llm-chat-manager`
Expected: PASS — Task 1's `identity_tests` plus all existing manager tests compile and pass. (The `/identity` round-trip itself is exercised end-to-end in Tasks 4-5 against a running stack; there is no Zitadel mock in the unit suite.)

- [ ] **Step 7: Commit**

```bash
cd /d/projects/llm-chat
git add manager/src/main.rs
git commit -m "feat(manager): /identity WS endpoint resolves + renders status/whoami

New chat.user-gated /identity endpoint: no worker, no chat session. Resolves
the display name via the user's own Zitadel /userinfo (email/sub fallback),
roles from the verified JWT, project/issuer from manager config, and renders
the full /status block + whoami line. Token captured at handshake, used only
for that request's userinfo, dropped on close. No identity cache in v1
(rare human-initiated commands; one GET each is cheap).

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Task 3: Config — `MANAGER_PROJECT_NAME`

**Files:**
- Modify: `.env.local.example` (manager section, around lines 25-28).

**Interfaces:**
- Consumes: the manager's `std::env::var("MANAGER_PROJECT_NAME")` read (Task 2).
- Produces: the friendly project name in the generated `.env.local` (injected into the manager via `env_file:` in `docker-compose.yml`).

- [ ] **Step 1: Add the variable to the template**

In `.env.local.example`, under the `# ── manager ──` section, add the line after `MANAGER_BACKEND_PORTS=7878`:

```
# Friendly project name shown on the clients' /status (else the numeric id):
MANAGER_PROJECT_NAME=llm-chat
```

- [ ] **Step 2: Verify the reset scripts pass it through**

The real `.env.local` is regenerated from this template by `reset.ps1`/`reset.sh`.
Confirm a static (non-generated) line carries through — grep how those scripts build
`.env.local`:

Run: `grep -n "env.local" reset.ps1 reset.sh`
Expected: the scripts copy the template and substitute only the generated IDs
(`PROJECT_ID`, `OIDC_CLIENT_ID`); a static line like `MANAGER_PROJECT_NAME` passes
through unchanged. If a script instead emits a curated allow-list of lines, add
`MANAGER_PROJECT_NAME` to that list. (If you already have a `.env.local`, add the
same line to it by hand so the running stack picks it up.)

- [ ] **Step 3: Commit**

```bash
cd /d/projects/llm-chat
git add .env.local.example
git commit -m "feat(config): MANAGER_PROJECT_NAME for friendly /status project label

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Task 4: Rust client — route `/status`, `whoami`, `login` through `/identity`

**Files:**
- Modify: `clients/rust/src/config.rs` — add `identity_url` + a unit test.
- Modify: `clients/rust/src/protocol.rs` — add `request_identity` + `ChatClient::token_provider`.
- Modify: `clients/rust/src/repl.rs` — `/status` via `/identity`; delete `format_status` + its tests; update `ReplCtx`.
- Modify: `clients/rust/src/cli.rs` — `whoami`/`login` via `/identity`; delete `identity_from_token`, `print_whoami`, `decode_claims` + their tests; update `ReplCtx` build.

**Interfaces:**
- Consumes (from Task 2): the `/identity` endpoint (`{type:"status"|"whoami"}` → `{block}`/`{line}`).
- Produces:
  - `config::identity_url(manager_ws: &str) -> String`
  - `protocol::request_identity(identity_url: &str, provider: &TokenProvider, request: serde_json::Value, timeout: Duration) -> Result<serde_json::Value>`
  - `protocol::ChatClient::token_provider(&self) -> TokenProvider`
  - `ReplCtx { kind, version, auth_label, manager_url, identity_url }` (no more `issuer`/`project`).

- [ ] **Step 1: Write the failing `identity_url` test**

In `clients/rust/src/config.rs`, inside the existing `#[cfg(test)] mod tests`, add:

```rust
    #[test]
    fn identity_url_swaps_path() {
        assert_eq!(identity_url("ws://127.0.0.1:7777/chat"), "ws://127.0.0.1:7777/identity");
        assert_eq!(identity_url("wss://host.example:443/chat"), "wss://host.example:443/identity");
        assert_eq!(identity_url("ws://h:7777"), "ws://h:7777/identity");
    }
```

- [ ] **Step 2: Run it to verify it fails**

Run: `cargo test -p llm-chat-client identity_url_swaps_path`
Expected: FAIL to COMPILE — `cannot find function identity_url`.

- [ ] **Step 3: Implement `identity_url`**

In `clients/rust/src/config.rs`, after `resolve_manager` (around line 99), add:

```rust
/// Derive the `/identity` URL from the manager `/chat` URL: same scheme +
/// host:port, path replaced with `/identity` (the manager serves both).
pub fn identity_url(manager_ws: &str) -> String {
    match manager_ws.split_once("://") {
        Some((scheme, rest)) => {
            let authority = rest.split('/').next().unwrap_or(rest);
            format!("{scheme}://{authority}/identity")
        }
        None => manager_ws.to_string(),
    }
}
```

- [ ] **Step 4: Run it to verify it passes**

Run: `cargo test -p llm-chat-client identity_url_swaps_path`
Expected: PASS.

- [ ] **Step 5: Add `request_identity` + the `token_provider` getter**

In `clients/rust/src/protocol.rs`, add the import for `connect_async` (the file
currently imports `connect_async_with_config`; add `connect_async` to the same
`use tokio_tungstenite::{...}` line). Then add the getter inside `impl ChatClient`
(next to `current_token`):

```rust
    /// Clone the token provider — used to authenticate the short-lived
    /// `/identity` connection that `/status` opens.
    pub fn token_provider(&self) -> TokenProvider {
        self.token_provider.clone()
    }
```

And add this free function at the end of the file (after `impl ChatClient`):

```rust
/// Open a short-lived `/identity` connection, send ONE request, and return the
/// single reply frame whose `type` matches the request's. The manager spawns no
/// worker; it resolves identity, renders, replies, and closes. Used by `/status`
/// (`{"type":"status","client":{…}}`) and `whoami` (`{"type":"whoami"}`). The
/// client prints the returned text verbatim — it does no identity logic.
pub async fn request_identity(
    identity_url: &str,
    provider: &TokenProvider,
    request: serde_json::Value,
    timeout: Duration,
) -> Result<serde_json::Value> {
    let want = request
        .get("type")
        .and_then(|t| t.as_str())
        .unwrap_or("")
        .to_string();

    let p = provider.clone();
    let token = tokio::task::spawn_blocking(move || p())
        .await
        .map_err(|e| Error::Auth(format!("token task failed: {e}")))??;

    let mut req = identity_url
        .into_client_request()
        .map_err(|e| Error::ManagerUnavailable(format!("bad identity URL {identity_url}: {e}")))?;
    req.headers_mut().insert(
        "Authorization",
        format!("Bearer {token}")
            .parse()
            .map_err(|e| Error::ManagerUnavailable(format!("bad auth header: {e}")))?,
    );

    let (mut ws, _) = match tokio::time::timeout(Duration::from_secs(15), connect_async(req)).await {
        Err(_) => {
            return Err(Error::ManagerUnavailable(format!(
                "could not connect to {identity_url}: open timed out"
            )))
        }
        Ok(Err(e)) => {
            return Err(Error::ManagerUnavailable(format!(
                "could not connect to {identity_url}: {e}"
            )))
        }
        Ok(Ok(pair)) => pair,
    };

    ws.send(Message::Text(request.to_string()))
        .await
        .map_err(|e| Error::ManagerUnavailable(format!("identity send failed: {e}")))?;

    let deadline = Instant::now() + timeout;
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(Error::AnswerTimeout("no identity reply within timeout".into()));
        }
        let frame = match tokio::time::timeout(remaining, ws.next()).await {
            Err(_) => return Err(Error::AnswerTimeout("no identity reply within timeout".into())),
            Ok(None) | Ok(Some(Ok(Message::Close(_)))) | Ok(Some(Err(_))) => {
                return Err(Error::ManagerUnavailable("identity connection closed".into()))
            }
            Ok(Some(Ok(Message::Text(t)))) => t,
            Ok(Some(Ok(Message::Binary(b)))) => String::from_utf8_lossy(&b).into_owned(),
            Ok(Some(Ok(_))) => continue,
        };
        let msg: serde_json::Value = serde_json::from_str(&frame)
            .map_err(|_| Error::Protocol(format!("manager sent non-JSON frame: {frame}")))?;
        match msg.get("type").and_then(|t| t.as_str()) {
            Some(t) if t == want => {
                let _ = ws.close(None).await;
                return Ok(msg);
            }
            Some("err") => {
                return Err(Error::Protocol(
                    msg.get("text").and_then(|v| v.as_str()).unwrap_or("error").to_string(),
                ))
            }
            _ => continue,
        }
    }
}
```

- [ ] **Step 6: Build to confirm protocol.rs compiles**

Run: `cargo build -p llm-chat-client`
Expected: builds (warnings about the not-yet-used `request_identity`/`token_provider`/`identity_url` are fine).

- [ ] **Step 7: Update `ReplCtx` and `/status` in `repl.rs`**

In `clients/rust/src/repl.rs`:

1. Change the imports at the top: drop `use crate::cli::identity_from_token;`, and add
   `use crate::protocol::{request_identity, ChatClient};` (replace the existing
   `use crate::protocol::ChatClient;`).

2. Replace the `ReplCtx` struct (lines 27-36) with:

```rust
/// Static context for the REPL's `/status` request. Everything here is a CLIENT
/// fact (its kind/version, auth mode, the manager URL it dialed, and the
/// `/identity` URL it posts to). Identity + project + issuer come from the
/// backend, which also renders the block.
pub struct ReplCtx {
    pub kind: &'static str,    // "rust"
    pub version: &'static str, // crate version
    pub auth_label: String,    // "human (browser login)" | "machine (kabytech key)"
    pub manager_url: String,
    pub identity_url: String,
}
```

3. DELETE the `STATUS_RULE` const (line 38) and the entire `format_status` function
   (lines 42-75). (Keep `human_int`, `human_bytes`, `format_usage`, `format_dir` — those
   stay; but note `format_usage`/`format_dir` reference `STATUS_RULE`. So do NOT delete
   `STATUS_RULE`; only delete `format_status`.) Re-read after editing: keep `STATUS_RULE`,
   delete `format_status` only.

4. Replace the `/status` handler (lines 341-372) with:

```rust
        if user == "/status" {
            let req = serde_json::json!({
                "type": "status",
                "client": {
                    "kind": ctx.kind,
                    "version": ctx.version,
                    "authLabel": ctx.auth_label,
                    "renderMode": mode_name(render_mode),
                    "timeoutSecs": timeout.as_secs(),
                    "managerUrl": ctx.manager_url,
                    "connected": client.connected(),
                    "sessionId": client.session_id,
                    "msgsThisSession": history.len(),
                }
            });
            match request_identity(&ctx.identity_url, &client.token_provider(), req, timeout).await {
                Ok(reply) => println!(
                    "{}",
                    c.dim(reply.get("block").and_then(|v| v.as_str()).unwrap_or("(no status)"))
                ),
                Err(e) => println!("{}", c.err(&format!("status unavailable: {e}"))),
            }
            println!();
            continue;
        }
```

5. DELETE the two `format_status_*` tests in the `#[cfg(test)] mod tests` block
   (`format_status_includes_all_fields`, `format_status_handles_empty_roles_and_no_session`,
   lines 485-508) and update the `ctx()` test helper (lines 474-483) to the new
   struct shape:

```rust
    fn ctx() -> ReplCtx {
        ReplCtx {
            kind: "rust",
            version: "1.0.0",
            auth_label: "machine (kabytech key)".to_string(),
            manager_url: "ws://m:7777/chat".to_string(),
            identity_url: "ws://m:7777/identity".to_string(),
        }
    }
```

   (`ctx()` is still used by no remaining test after deleting the two `format_status`
   tests — if `cargo test` warns it's unused, delete the `ctx()` helper too.)

- [ ] **Step 8: Update `whoami`/`login` in `cli.rs`**

In `clients/rust/src/cli.rs`:

1. Update imports: from `crate::config` add `identity_url`; from `crate::protocol` add
   `request_identity`. The line `use crate::config::{configure_logging, load_env_local, resolve_manager, AuthMode, CommonArgs};`
   becomes `...resolve_manager, identity_url, AuthMode, CommonArgs};`. The line
   `use crate::protocol::{ChatClient, TokenProvider};` becomes
   `use crate::protocol::{request_identity, ChatClient, TokenProvider};`.

2. DELETE `decode_claims` (lines 113-128), `identity_from_token` (lines 130-154), and
   `print_whoami` (lines 156-167). Also remove the now-unused `use base64::Engine;`
   (line 9) and the `use serde_json::{Map, Value};` import if `Map`/`Value` become
   unused (grep — `Value` may still be used elsewhere; only remove what's unused).

3. Add a shared helper (place it near `user_provider`):

```rust
/// Ask the backend `/identity` who we are and print its rendered line. The
/// client does NO token decoding — identity is the server's job. Fails loudly
/// if the manager is unreachable (no client-side fallback).
fn show_identity(manager_url: &str, provider: &TokenProvider, timeout: Duration) -> Result<u8> {
    let id_url = identity_url(manager_url);
    let rt = tokio::runtime::Runtime::new()
        .map_err(|e| Error::ManagerUnavailable(format!("could not start runtime: {e}")))?;
    let reply = rt.block_on(request_identity(
        &id_url,
        provider,
        serde_json::json!({"type": "whoami"}),
        timeout,
    ))?;
    println!("{}", reply.get("line").and_then(|v| v.as_str()).unwrap_or("(no identity)"));
    Ok(0)
}
```

4. Replace `cmd_login` (lines 171-176):

```rust
fn cmd_login(c: &CommonArgs) -> Result<u8> {
    let (issuer, client_id, project, store, endpoints) = user_session(c)?;
    login_and_store(&issuer, &client_id, &project, &store, c.oidc_port)?;
    let provider = user_provider(store, endpoints.token.clone(), client_id);
    let manager_url = resolve_manager(&c.manager)?;
    // Greeting is best-effort — the session is already cached.
    match show_identity(&manager_url, &provider, Duration::from_secs_f64(c.timeout)) {
        Ok(code) => Ok(code),
        Err(e) => {
            println!("logged in. (identity unavailable: {e})");
            Ok(0)
        }
    }
}
```

5. Replace `cmd_whoami` (lines 193-205):

```rust
fn cmd_whoami(c: &CommonArgs) -> Result<u8> {
    let (_issuer, client_id, _project, store, endpoints) = user_session(c)?;
    if store.load().is_none() {
        eprintln!("not logged in — run `llm-chat login`");
        return Ok(EXIT_AUTH);
    }
    let provider = user_provider(store, endpoints.token.clone(), client_id);
    let manager_url = resolve_manager(&c.manager)?;
    show_identity(&manager_url, &provider, Duration::from_secs_f64(c.timeout))
}
```

6. Update the `ReplCtx` construction in `cmd_chat_or_ask` (lines 289-299) to the new
   shape (drop `issuer`/`project`, add `identity_url`):

```rust
            let ctx = ReplCtx {
                kind: "rust",
                version: env!("CARGO_PKG_VERSION"),
                auth_label: match mode {
                    AuthMode::Machine => "machine (kabytech key)".to_string(),
                    AuthMode::User => "human (browser login)".to_string(),
                },
                manager_url: manager_url.clone(),
                identity_url: identity_url(&manager_url),
            };
```

7. DELETE the now-stale tests in `cli.rs`'s `#[cfg(test)] mod tests`:
   `decode_claims_reads_payload` and `decode_claims_bad_token_is_empty` (lines 377-392).
   Keep `cli_parses_subcommands`.

- [ ] **Step 9: Build + test the whole client**

Run: `cargo test -p llm-chat-client`
Expected: PASS. Then `cargo build -p llm-chat-client` clean (no unused-import / dead-code
warnings — if any appear for `Map`/`Value`/`base64`, remove those imports).

- [ ] **Step 10: Commit**

```bash
cd /d/projects/llm-chat
git add clients/rust/src/config.rs clients/rust/src/protocol.rs clients/rust/src/repl.rs clients/rust/src/cli.rs
git commit -m "feat(client/rust): /status + whoami + login via backend /identity

The rust client stops decoding JWTs and rendering the status block. /status,
whoami, and login now post to the manager's /identity endpoint and print the
server-rendered text. Deletes identity_from_token, format_status, print_whoami.

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Task 5: Python client — route `/status`, `whoami`, `login` through `/identity`

**Files:**
- Modify: `clients/python/llm_chat/config.py` — add `identity_url`.
- Modify: `clients/python/llm_chat/protocol.py` — add `request_identity`.
- Modify: `clients/python/llm_chat/repl.py` — `/status` via `/identity`; delete `format_status`; update `ReplCtx`.
- Modify: `clients/python/llm_chat/cli.py` — `whoami`/`login` via `/identity`; delete `_identity_from_token`, `_print_whoami`, `_decode_claims`; update `ReplCtx` build.
- Modify: `clients/python/tests/test_repl.py` — drop `format_status` tests; add `identity_url` test.
- Modify: `clients/python/tests/test_cli_resolver.py` — drop `_decode_claims` tests.

**Interfaces:**
- Consumes (from Task 2): the `/identity` endpoint.
- Produces:
  - `config.identity_url(manager_ws: str) -> str`
  - `protocol.request_identity(identity_url, provider, request: dict, *, timeout) -> dict`
  - `ReplCtx(kind, version, auth_label, manager_url, identity_url)` (no `issuer`/`project`).

- [ ] **Step 1: Write the failing `identity_url` test**

In `clients/python/tests/test_repl.py`, add (and add `identity_url` to the import from
`llm_chat.config`, or import it directly):

```python
def test_identity_url_swaps_path():
    from llm_chat.config import identity_url
    assert identity_url("ws://127.0.0.1:7777/chat") == "ws://127.0.0.1:7777/identity"
    assert identity_url("wss://host.example:443/chat") == "wss://host.example:443/identity"
    assert identity_url("ws://h:7777") == "ws://h:7777/identity"
```

- [ ] **Step 2: Run it to verify it fails**

Run (from `clients/python`): `python -m pytest tests/test_repl.py::test_identity_url_swaps_path -q`
Expected: FAIL — `ImportError: cannot import name 'identity_url'`.

- [ ] **Step 3: Implement `identity_url`**

In `clients/python/llm_chat/config.py`, after `resolve_manager` (around line 70), add:

```python
def identity_url(manager_ws: str) -> str:
    """Derive the `/identity` URL from the manager `/chat` URL: same scheme +
    host:port, path replaced with `/identity` (the manager serves both)."""
    scheme, sep, rest = manager_ws.partition("://")
    if not sep:
        return manager_ws
    authority = rest.split("/", 1)[0]
    return f"{scheme}://{authority}/identity"
```

- [ ] **Step 4: Run it to verify it passes**

Run: `python -m pytest tests/test_repl.py::test_identity_url_swaps_path -q`
Expected: PASS.

- [ ] **Step 5: Add `request_identity` to `protocol.py`**

In `clients/python/llm_chat/protocol.py`, add a module-level function (after the
`ChatClient` class, end of file):

```python
async def request_identity(
    identity_url: str,
    provider: TokenProvider,
    request: dict,
    *,
    timeout: float = 120.0,
) -> dict:
    """Open a short-lived `/identity` connection, send ONE request, and return
    the reply frame whose ``type`` matches. The manager spawns no worker; it
    resolves identity, renders, replies, and closes. Used by `/status`
    (``{"type":"status","client":{…}}``) and whoami (``{"type":"whoami"}``).
    The client prints the returned text verbatim — no identity logic here.

    Raises AnswerTimeout / ProtocolError / ManagerUnavailable.
    """
    want = request.get("type", "")
    token = await _call_token_provider(provider)
    try:
        ws = await websockets.connect(
            identity_url,
            additional_headers=[("Authorization", f"Bearer {token}")],
            max_size=None,
            open_timeout=15,
        )
    except (OSError, websockets.WebSocketException, asyncio.TimeoutError) as e:
        raise ManagerUnavailable(f"could not connect to {identity_url}: {e}") from e

    try:
        await ws.send(json.dumps(request))
        deadline = time.monotonic() + timeout
        while True:
            remaining = deadline - time.monotonic()
            if remaining <= 0:
                raise AnswerTimeout("no identity reply within the timeout")
            try:
                raw = await asyncio.wait_for(ws.recv(), timeout=remaining)
            except asyncio.TimeoutError:
                raise AnswerTimeout("no identity reply within the timeout") from None
            except websockets.ConnectionClosed as e:
                raise ManagerUnavailable(f"identity connection closed: {e}") from e
            try:
                msg = json.loads(raw)
            except ValueError as e:
                raise ProtocolError(f"manager sent non-JSON frame: {raw!r}") from e
            mtype = msg.get("type")
            if mtype == want:
                return msg
            if mtype == "err":
                raise ProtocolError(msg.get("text", "identity error"))
            log.debug("skip frame type=%s (awaiting %s)", mtype, want)
    finally:
        try:
            await ws.close()
        except Exception:  # noqa: BLE001 — close must never raise
            pass
```

- [ ] **Step 6: Update `ReplCtx` + `/status` in `repl.py`**

In `clients/python/llm_chat/repl.py`:

1. Add the import near the top (with the other `from .` imports):
   `from .config import identity_url` — actually `identity_url` is built in cli.py and
   passed in; the repl only needs `request_identity`. Add
   `from .protocol import Answer, ChatClient, request_identity` (extend the existing
   protocol import).

2. Replace the `ReplCtx` dataclass (lines 111-120) with:

```python
@dataclass(frozen=True)
class ReplCtx:
    """Static context for the REPL's /status request. All CLIENT facts; identity,
    project, and issuer come from the backend (which also renders the block)."""

    kind: str            # "python"
    version: str
    auth_label: str      # "human (browser login)" | "machine (kabytech key)"
    manager_url: str
    identity_url: str
```

3. DELETE the `format_status` function (lines 123-144). Keep `STATUS_RULE`,
   `human_int`, `human_bytes`, `format_usage`, `format_dir`.

4. Replace the `/status` handler (lines 273-289) with:

```python
        if user == "/status":
            req = {
                "type": "status",
                "client": {
                    "kind": ctx.kind,
                    "version": ctx.version,
                    "authLabel": ctx.auth_label,
                    "renderMode": render_mode,
                    "timeoutSecs": int(timeout),
                    "managerUrl": ctx.manager_url,
                    "connected": client.connected,
                    "sessionId": client.session_id,
                    "msgsThisSession": len(history),
                },
            }
            try:
                reply = await request_identity(
                    ctx.identity_url, client.token_provider, req, timeout=timeout)
                print(c.dim(reply.get("block") or "(no status)"))
            except (AnswerTimeout, ProtocolError, ManagerUnavailable) as e:
                print(c.err(f"status unavailable: {e}"))
            print()
            continue
```

   This needs `client.token_provider`. The `ChatClient` stores it as
   `self._token_provider`. Add a public accessor: in `protocol.py`'s `ChatClient`,
   add a property (next to `current_token`):

```python
    @property
    def token_provider(self) -> TokenProvider:
        """The token provider — used to auth the short-lived /identity connection."""
        return self._token_provider
```

- [ ] **Step 7: Update `whoami`/`login` in `cli.py`**

In `clients/python/llm_chat/cli.py`:

1. Update imports: add `identity_url` to the `from .config import (...)` line; add
   `request_identity` to `from .protocol import ChatClient` →
   `from .protocol import ChatClient, request_identity`.

2. DELETE `_decode_claims` (lines 108-116), `_identity_from_token` (lines 119-130),
   and `_print_whoami` (lines 133-137). Remove the now-unused `import base64` and
   `import json` if they are unused after deletion (grep the file — `json` is likely
   still used elsewhere; only remove `base64` if nothing else needs it).

3. Add a shared helper (place it near `_user_provider`):

```python
async def _show_identity(manager_url: str, provider, timeout: float) -> int:
    """Ask the backend /identity who we are and print its rendered line. No
    client-side token decoding; fails loudly if the manager is unreachable."""
    reply = await request_identity(
        identity_url(manager_url), provider, {"type": "whoami"}, timeout=timeout)
    print(reply.get("line") or "(no identity)")
    return EXIT_OK
```

4. Replace `_cmd_login` (lines 166-170):

```python
def _cmd_login(args) -> int:
    issuer, client_id, project, store, endpoints = _user_session(args)
    _login_and_store(issuer, client_id, project, store, args.oidc_port)
    provider = _user_provider(issuer, client_id, store, endpoints)
    manager_url = resolve_manager(args.manager)
    try:  # greeting is best-effort — the session is already cached
        return asyncio.run(_show_identity(manager_url, provider, args.timeout))
    except (ManagerUnavailable, ProtocolError, AnswerTimeout) as e:
        print(f"logged in. (identity unavailable: {e})")
        return EXIT_OK
```

5. Replace `_cmd_whoami` (lines 186-193):

```python
def _cmd_whoami(args) -> int:
    issuer, client_id, _project, store, endpoints = _user_session(args)
    if store.load() is None:
        print("not logged in — run `llm-chat login`", file=sys.stderr)
        return EXIT_AUTH
    provider = _user_provider(issuer, client_id, store, endpoints)
    manager_url = resolve_manager(args.manager)
    return asyncio.run(_show_identity(manager_url, provider, args.timeout))
```

6. Update the `ReplCtx` construction in `_cmd_chat_or_ask` (lines 213-221):

```python
    ctx = ReplCtx(
        kind="python",
        version=__version__,
        auth_label="machine (kabytech key)" if mode == "machine" else "human (browser login)",
        manager_url=manager_url,
        identity_url=identity_url(manager_url),
    )
```

- [ ] **Step 8: Update the Python tests**

1. In `clients/python/tests/test_repl.py`: DELETE `test_format_status_includes_all_fields`
   and `test_format_status_empty_roles_and_no_session` (they test the deleted
   `format_status`), and remove `format_status` + `ReplCtx` from the import on line 6
   if `ReplCtx`/`format_status` are no longer referenced (check the `_ctx()` helper —
   if it only fed `format_status`, delete it too). Keep the `format_usage`/`format_dir`/
   answer-formatting tests. The new `test_identity_url_swaps_path` (Step 1) stays.

2. In `clients/python/tests/test_cli_resolver.py`: DELETE `test_decode_claims_reads_payload`
   (lines 27-32) and any other test referencing `cli._decode_claims` / `_identity_from_token`.
   Keep the resolver tests for issuer/project/client-id/manager.

- [ ] **Step 9: Run the full Python suite**

Run (from `clients/python`): `python -m pytest -q`
Expected: PASS (deleted-function tests gone, `identity_url` test green, everything else green).

- [ ] **Step 10: Commit**

```bash
cd /d/projects/llm-chat
git add clients/python/llm_chat/config.py clients/python/llm_chat/protocol.py clients/python/llm_chat/repl.py clients/python/llm_chat/cli.py clients/python/tests/test_repl.py clients/python/tests/test_cli_resolver.py
git commit -m "feat(client/python): /status + whoami + login via backend /identity

Mirrors the rust client: the python client stops decoding JWTs and rendering
the status block. /status, whoami, and login post to the manager's /identity
endpoint and print the server-rendered text. Deletes _identity_from_token,
_decode_claims, format_status, _print_whoami.

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Task 6: End-to-end verification against the running stack

**Files:** none (manual verification).

**Interfaces:** Consumes everything above.

- [ ] **Step 1: Rebuild + restart the manager image**

The manager runs in Docker. Rebuild just the manager (the zitadel-init gotcha:
do NOT re-run deps — see config.md):

Run: `docker compose up -d --build --no-deps manager`
Expected: manager container rebuilt + restarted, no errors. (If `.env.local` lacks
`MANAGER_PROJECT_NAME`, add it per Task 3 Step 2 before this step so the project shows
`llm-chat`, not the numeric id.)

- [ ] **Step 2: `/status` shows human labels (rust)**

Start the rust REPL (human login), then type `/status`. Expected: the `user` line
shows your display name (or email), `roles` shows `chat.user` (+ `chat.admin` if
granted), `project` shows `llm-chat`, `session`/`msgs`/`render`/`timeout` reflect the
live client. NO numeric `sub` in the `user` line (the numeric id appears ONLY on the
`sub` line, which is intentional).

- [ ] **Step 3: `whoami` + `login` go through the backend**

Run `llm-chat whoami`. Expected: `logged in as <name> (sub=<id>)` + roles, produced by
the backend (verify by checking the manager logs show a `/identity` connection). Run
`llm-chat login` again — expected: the post-login greeting prints the same backend line.

- [ ] **Step 4: Python client parity**

Repeat Steps 2-3 with the python client (`llm-chat` from `clients/python`). The
`/status` block and `whoami` line must be byte-identical to the rust client except the
`client` line's `python` vs `rust` token.

- [ ] **Step 5: Fail-closed check**

Stop the manager (`docker compose stop manager`), then run `llm-chat whoami`. Expected:
a clear "manager unavailable / could not connect … /identity" error and a non-zero exit
— NOT a client-side-decoded identity. Restart: `docker compose start manager`.

- [ ] **Step 6: Finish the branch**

Announce and use the **superpowers:finishing-a-development-branch** skill:
verify `cargo test -p llm-chat-manager`, `cargo test -p llm-chat-client`, and
`python -m pytest -q` (from `clients/python`) all pass, then present the standard
options. (Remember: shared branch — explicit-path commits only; the merge-to-main
step, if chosen, uses the established throwaway-worktree + cherry-pick flow.)

---

## Self-Review

**Spec coverage:**
- New `/identity` WS endpoint (chat.user-gated, no chat session) → Task 2. ✓
- Token capture at handshake, used only for that request's userinfo, dropped on close → Task 2 Steps 3-4 (capture; `drop` on non-identity paths; handler closes after one reply). ✓
- `resolve_user_label` mirroring admin-api `fetch_display_name` → Task 2 Step 5. ✓
- Renderer moves into the manager (one renderer, rust+python deleted) → Tasks 1, 4, 5. ✓
- Client sends `{kind,version,renderMode,timeoutSecs,managerUrl,connected,sessionId,msgsThisSession}` (+ `authLabel`) → Tasks 4-5 `/status` handlers. ✓ (`authLabel` added: the client legitimately knows its own auth mode; the manager can't infer browser-vs-key from the token. Noted as a deliberate, non-identity client fact.)
- whoami `{type:"whoami"}` → `{line}` → Tasks 2, 4, 5. ✓
- `MANAGER_PROJECT_NAME` config → Task 3 + Task 2 read. ✓
- Fail-soft userinfo, fail-closed transport → Task 2 (`resolve_user_label` best-effort) + Tasks 4-5 (no client fallback) + Task 6 Step 5. ✓
- `sub → name` cache: deliberately deferred (YAGNI) — stated in Global Constraints + Task 2 commit body. Documented deviation from the spec's "in-memory cache only" line; the spec listed it as the only caching kept, but it adds shared-mutable-state complexity for a rare human command. Flag to the user at execution if they want it kept.

**Placeholder scan:** no TBD/TODO; every code step has complete code; every command has an expected result. ✓

**Type consistency:** `StatusClient`/`parse_status_client`/`format_status_block`/`format_whoami_line`/`user_label_from_userinfo` signatures match between Task 1 (definition) and Task 2 (call sites). `request_identity` signature matches between Task 4 (rust def) / Task 5 (python def) and their call sites. `ReplCtx` new shape (`kind,version,auth_label,manager_url,identity_url`) is consistent across repl.rs/cli.rs (Task 4) and repl.py/cli.py (Task 5). `token_provider()` getter (rust) / `token_provider` property (python) added and used. ✓

**One deviation to surface at execution:** the identity cache is dropped (YAGNI). If the user wants it, add a `tokio::sync::Mutex<HashMap<String,String>>` on `ManagerState` and short-circuit `resolve_user_label` — a ~10-line addition, not a redesign.
