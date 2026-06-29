# Per-app Sandbox Templates (Sub-project 1: store + first-login provisioning) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Materialize an app's sandbox template into `{LLM_CHAT_USER_ENV_BASE}/{userId}/{app}/` the first time a user logs into the app (kabytech first), confined and idempotent.

**Architecture:** The manager owns a per-app template store (`chat_db`). On kabytech `/callback`, the backend best-effort calls a new `chat.user`-gated manager `/provision` WS endpoint; the manager loads the template, substitutes `{{name}}/{{userId}}/{{app}}/{{date}}`, and forwards files to a worker via `call_backend`; the worker writes them under the user's confined box (via `confine_path`), only if absent.

**Tech Stack:** Rust — manager (`llm-chat-manager`, sqlx, tokio-tungstenite, reqwest), worker (`llm_chat_lib`, std::fs), kabytech-backend (`kaby`, axum, tokio-tungstenite [new]).

## Global Constraints

- **Tests:** `cargo test -p llm-chat-manager`, `cargo test -p llm-chat --no-default-features` (worker lib), `cargo test -p kabytech-backend` — all run from `D:\projects\llm-chat`.
- **Branch:** work on `main` (single-branch workflow the user adopted); `git add` EXPLICIT paths only (the working tree has unrelated pre-existing uncommitted doc deletions — never `git add -A`).
- **Fail closed / single box owner (CLAUDE.md):** the worker is the SOLE owner of the box + confinement; every template path goes through `confine_path` (rejects `..`/absolute/`\`/`:`/NUL); `userId` is taken from the verified JWT at the manager, never from the request; `/provision` is `chat.user`-gated and self-scoped.
- **Idempotent:** dirs + files are written ONLY IF ABSENT — never clobber an existing file.
- **Best-effort login:** kabytech provisioning must NEVER block login — a failure/timeout/unreachable manager is logged and ignored.
- **Variables:** only `{{name}}`, `{{userId}}`, `{{app}}`, `{{date}}` are substituted (literal token replace); `date` is `YYYY-MM-DD`; unknown `{{x}}` left as-is.
- **Out of scope (Sub-project 2):** the admin-web Console template editor and the admin-api→manager relay. Here the kabytech template is SEEDED server-side.

---

## File Structure

- `worker/src/user_env.rs` — add `provision_entries` (pure-ish: confine + write-if-absent). Owns the confinement.
- `worker/src/lib.rs` — add the `provision-app-box` control-command arm (dispatch only).
- `manager/src/main.rs` — `app_sandbox_template` schema; `ChatDb::get_template`/`upsert_template`; `TemplateEntry`/`parse_template`/`TemplateVars`/`substitute_vars`; kabytech default + startup seed; `/provision` dispatch + `handle_provision`.
- `services/kabytech/backend/Cargo.toml` — add `tokio-tungstenite` + `futures-util`.
- `services/kabytech/backend/src/provision.rs` — new: WS provision client (mirror admin-api `manager.rs`).
- `services/kabytech/backend/src/config.rs` — optional `manager_provision_url` + `app_code`.
- `services/kabytech/backend/src/auth.rs` — fire the provision in `callback`.
- `services/kabytech/backend/src/{lib.rs,main.rs}` — declare `mod provision`; thread config.
- `.env.local.example`, `docker-compose.yml` — `KABY_MANAGER_PROVISION_URL`, `KABY_APP_CODE`.

---

## Task 1: Worker — `provision_entries` + `provision-app-box` command

**Files:**
- Modify: `worker/src/user_env.rs` (add `provision_entries` + tests)
- Modify: `worker/src/lib.rs:2458` (`match cmd` — add the `provision-app-box` arm)

**Interfaces:**
- Consumes: existing `confine_path(base, user_id, Some(subpath)) -> Result<PathBuf, ResolveError>`, `resolve_user_cwd`.
- Produces: `pub struct SeedEntry { pub path: String, pub dir: bool, pub content: String }` and
  `pub fn provision_entries(base: &Path, user_id: &str, app: &str, entries: &[SeedEntry]) -> Result<usize, ResolveError>`.

- [ ] **Step 1: Write the failing tests**

Add to the `#[cfg(test)] mod tests` in `worker/src/user_env.rs`:

```rust
    #[test]
    fn provision_entries_creates_files_and_dirs_confined() {
        let tmp = TempDir::new().unwrap();
        let base = tmp.path();
        let entries = vec![
            SeedEntry { path: "README.md".into(), dir: false, content: "hello".into() },
            SeedEntry { path: "sub".into(), dir: true, content: String::new() },
            SeedEntry { path: "sub/config.json".into(), dir: false, content: "{}".into() },
        ];
        let n = provision_entries(base, "u1", "kabytech", &entries).unwrap();
        assert_eq!(n, 3);
        assert_eq!(std::fs::read_to_string(base.join("u1/kabytech/README.md")).unwrap(), "hello");
        assert!(base.join("u1/kabytech/sub").is_dir());
        assert_eq!(std::fs::read_to_string(base.join("u1/kabytech/sub/config.json")).unwrap(), "{}");
    }

    #[test]
    fn provision_entries_writes_only_if_absent() {
        let tmp = TempDir::new().unwrap();
        let base = tmp.path();
        let e = vec![SeedEntry { path: "README.md".into(), dir: false, content: "v1".into() }];
        provision_entries(base, "u1", "kabytech", &e).unwrap();
        // a pre-existing file is NEVER overwritten
        let e2 = vec![SeedEntry { path: "README.md".into(), dir: false, content: "v2".into() }];
        let n = provision_entries(base, "u1", "kabytech", &e2).unwrap();
        assert_eq!(n, 0, "nothing newly written");
        assert_eq!(std::fs::read_to_string(base.join("u1/kabytech/README.md")).unwrap(), "v1");
    }

    #[test]
    fn provision_entries_rejects_traversal() {
        let tmp = TempDir::new().unwrap();
        let base = tmp.path();
        let e = vec![SeedEntry { path: "../escape".into(), dir: false, content: "x".into() }];
        assert!(provision_entries(base, "u1", "kabytech", &e).is_err());
        // a bad app segment is rejected too
        let e2 = vec![SeedEntry { path: "ok".into(), dir: false, content: "x".into() }];
        assert!(provision_entries(base, "u1", "..", &e2).is_err());
    }
```

> The test module already imports `super::*` and uses `TempDir` (see the existing `list_box_tree_*` tests). If `TempDir` isn't in scope, add `use tempfile::TempDir;` at the top of the test module (the crate already uses it in these tests).

- [ ] **Step 2: Run them to verify they fail**

Run: `cargo test -p llm-chat --no-default-features provision_entries`
Expected: FAIL to COMPILE — `cannot find type SeedEntry` / `function provision_entries`.

- [ ] **Step 3: Implement `provision_entries`**

In `worker/src/user_env.rs`, after `resolve_user_cwd` (around line 103), add:

```rust
/// One entry to materialize into a box: a file (`dir=false`, with `content`) or
/// an empty folder (`dir=true`). `path` is relative to `{base}/{user_id}/{app}/`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SeedEntry {
    pub path: String,
    pub dir: bool,
    pub content: String,
}

/// Materialize `entries` under the user's confined box `{base}/{user_id}/{app}/`.
/// Each entry's full box-relative path (`{app}/{path}`) is validated by
/// `confine_path` (rejects traversal/absolute/illegal chars — fail closed). The
/// app folder is ensured even when `entries` is empty. Files/dirs are written
/// ONLY IF ABSENT (never clobber). Returns the count of newly-created entries.
pub fn provision_entries(
    base: &Path,
    user_id: &str,
    app: &str,
    entries: &[SeedEntry],
) -> Result<usize, ResolveError> {
    // Ensure {base}/{user_id}/{app}/ exists + is confined (canonicalize-proven).
    resolve_user_cwd(base, user_id, Some(app))?;
    let mut created = 0usize;
    for e in entries {
        let rel = format!("{app}/{}", e.path);
        let full = confine_path(base, user_id, Some(&rel))?;
        if e.dir {
            if !full.exists() {
                std::fs::create_dir_all(&full)
                    .map_err(|err| ResolveError::Io(format!("create {}: {err}", full.display())))?;
                created += 1;
            }
        } else {
            if let Some(parent) = full.parent() {
                std::fs::create_dir_all(parent)
                    .map_err(|err| ResolveError::Io(format!("create {}: {err}", parent.display())))?;
            }
            if !full.exists() {
                std::fs::write(&full, e.content.as_bytes())
                    .map_err(|err| ResolveError::Io(format!("write {}: {err}", full.display())))?;
                created += 1;
            }
        }
    }
    Ok(created)
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p llm-chat --no-default-features provision_entries`
Expected: PASS (3 tests).

- [ ] **Step 5: Add the `provision-app-box` control command**

In `worker/src/lib.rs`, in the `match cmd` block (the `"dir" => {…}` arm is around line 2685), add a new arm right after the `"dir"` arm closes (before `"close"`):

```rust
                            "provision-app-box" => {
                                // Materialize an app's sandbox template into the
                                // caller's confined box. userId + app are validated;
                                // files are written only-if-absent. Fail closed.
                                let user_id = req.get("userId").and_then(|v| v.as_str()).unwrap_or("");
                                let app = req.get("app").and_then(|v| v.as_str()).unwrap_or("");
                                let entries: Vec<crate::user_env::SeedEntry> = req
                                    .get("files")
                                    .and_then(|v| v.as_array())
                                    .map(|arr| arr.iter().map(|f| crate::user_env::SeedEntry {
                                        path: f.get("path").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                                        dir: f.get("dir").and_then(|v| v.as_bool()).unwrap_or(false),
                                        content: f.get("content").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                                    }).collect())
                                    .unwrap_or_default();
                                tracing::info!(target: "backend::provision", user_id, app, n = entries.len(), "provision-app-box received");
                                let base = USER_ENV_BASE.get().expect("validated at startup");
                                match crate::user_env::provision_entries(base, user_id, app, &entries) {
                                    Ok(created) => serde_json::json!({"ok": true, "created": created}),
                                    Err(e) => {
                                        tracing::warn!(target: "backend::provision", error = %e, "provision REJECTED (fail closed)");
                                        serde_json::json!({"ok": false, "error": format!("env: {e}")})
                                    }
                                }
                            }
```

- [ ] **Step 6: Build the worker to confirm it compiles**

Run: `cargo build -p llm-chat --no-default-features`
Expected: builds clean.

- [ ] **Step 7: Commit**

```bash
cd /d/projects/llm-chat
git add worker/src/user_env.rs worker/src/lib.rs
git commit -m "feat(worker): provision-app-box — materialize a confined app template (if-absent)

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Task 2: Manager — template store + pure template functions

**Files:**
- Modify: `manager/src/main.rs` (schema in `init_schema_sqlite`+`init_schema_postgres`; `ChatDb::get_template`/`upsert_template`; pure `TemplateEntry`/`parse_template`/`TemplateVars`/`substitute_vars`; default + tests)

**Interfaces:**
- Produces:
  - `struct TemplateEntry { path: String, dir: bool, content: String }` (serde `Deserialize`+`Serialize`)
  - `fn parse_template(json: &str) -> Result<Vec<TemplateEntry>, String>`
  - `struct TemplateVars { name: String, user_id: String, app: String, date: String }`
  - `fn substitute_vars(content: &str, v: &TemplateVars) -> String`
  - `const KABYTECH_DEFAULT_TEMPLATE: &str` (the JSON array)
  - `ChatDb::get_template(&self, app_code: &str) -> Result<Option<String>, sqlx::Error>`
  - `ChatDb::upsert_template(&self, app_code: &str, template_json: &str, updated_at: &str) -> Result<(), sqlx::Error>`

- [ ] **Step 1: Write the failing pure-function tests**

Add a test module near the other `#[cfg(test)]` blocks in `manager/src/main.rs`:

```rust
#[cfg(test)]
mod template_tests {
    use super::*;

    #[test]
    fn parse_template_reads_entries() {
        let json = r#"[
            {"path":"README.md","dir":false,"content":"hi {{name}}"},
            {"path":"sub","dir":true,"content":""}
        ]"#;
        let t = parse_template(json).expect("ok");
        assert_eq!(t.len(), 2);
        assert_eq!(t[0].path, "README.md");
        assert!(!t[0].dir);
        assert_eq!(t[0].content, "hi {{name}}");
        assert!(t[1].dir);
    }

    #[test]
    fn parse_template_rejects_non_array() {
        assert!(parse_template("{}").is_err());
        assert!(parse_template("not json").is_err());
    }

    #[test]
    fn substitute_vars_replaces_known_tokens_only() {
        let v = TemplateVars {
            name: "Jane Doe".into(), user_id: "U9".into(),
            app: "kabytech".into(), date: "2026-06-29".into(),
        };
        let out = substitute_vars("{{name}} {{userId}} {{app}} {{date}} {{unknown}}", &v);
        assert_eq!(out, "Jane Doe U9 kabytech 2026-06-29 {{unknown}}");
    }

    #[test]
    fn kabytech_default_template_parses() {
        let t = parse_template(KABYTECH_DEFAULT_TEMPLATE).expect("default parses");
        assert!(t.iter().any(|e| e.path == "README.md"));
        assert!(t.iter().any(|e| e.path == "config.json"));
    }

    #[tokio::test]
    async fn template_table_roundtrips() {
        use sqlx::sqlite::SqlitePoolOptions;
        let pool = SqlitePoolOptions::new().connect("sqlite::memory:").await.unwrap();
        init_schema_sqlite(&pool).await.unwrap();
        let db = ChatDb::Sqlite(pool);
        assert!(db.get_template("kabytech").await.unwrap().is_none());
        db.upsert_template("kabytech", "[]", "2026-06-29T00:00:00Z").await.unwrap();
        assert_eq!(db.get_template("kabytech").await.unwrap().as_deref(), Some("[]"));
        // upsert overwrites
        db.upsert_template("kabytech", "[{\"path\":\"x\",\"dir\":true,\"content\":\"\"}]", "t2").await.unwrap();
        assert!(db.get_template("kabytech").await.unwrap().unwrap().contains("\"x\""));
    }
}
```

> `ChatDb::Sqlite(pool)` is the existing enum variant (see `enum ChatDb { Sqlite(SqlitePool), … }`). If the variant is private-constructed elsewhere, construct it the same way the existing `compose_*`/schema tests do.

- [ ] **Step 2: Run them to verify they fail**

Run: `cargo test -p llm-chat-manager template_tests`
Expected: FAIL to COMPILE — `parse_template` / `TemplateVars` / `get_template` not found.

- [ ] **Step 3: Add the schema (both dialects)**

In `init_schema_sqlite` (around line 593), after the `chat_question` `CREATE TABLE`, add another statement (use a second `sqlx::query(...).execute(pool).await?;` like the existing ones):

```rust
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS app_sandbox_template (
            app_code TEXT PRIMARY KEY,
            template_json TEXT NOT NULL,
            updated_at TEXT NOT NULL
        )",
    )
    .execute(pool)
    .await?;
```

In `init_schema_postgres` (around line 645), add the identical statement (Postgres accepts this DDL verbatim) against the `PgPool`.

- [ ] **Step 4: Add the `ChatDb` methods**

In `impl ChatDb` (around line 266), add (mirroring how existing methods `match self` over the dialect — keep the same pattern as e.g. `usage_for`):

```rust
    /// The stored template JSON for an app code, or None.
    pub async fn get_template(&self, app_code: &str) -> Result<Option<String>, sqlx::Error> {
        match self {
            ChatDb::Sqlite(p) => {
                let row: Option<(String,)> = sqlx::query_as(
                    "SELECT template_json FROM app_sandbox_template WHERE app_code = ?",
                ).bind(app_code).fetch_optional(p).await?;
                Ok(row.map(|r| r.0))
            }
            ChatDb::Postgres(p) => {
                let row: Option<(String,)> = sqlx::query_as(
                    "SELECT template_json FROM app_sandbox_template WHERE app_code = $1",
                ).bind(app_code).fetch_optional(p).await?;
                Ok(row.map(|r| r.0))
            }
        }
    }

    /// Insert or replace the template for an app code.
    pub async fn upsert_template(&self, app_code: &str, template_json: &str, updated_at: &str)
        -> Result<(), sqlx::Error> {
        match self {
            ChatDb::Sqlite(p) => {
                sqlx::query(
                    "INSERT INTO app_sandbox_template (app_code, template_json, updated_at)
                     VALUES (?, ?, ?)
                     ON CONFLICT(app_code) DO UPDATE SET template_json = excluded.template_json,
                       updated_at = excluded.updated_at",
                ).bind(app_code).bind(template_json).bind(updated_at).execute(p).await?;
            }
            ChatDb::Postgres(p) => {
                sqlx::query(
                    "INSERT INTO app_sandbox_template (app_code, template_json, updated_at)
                     VALUES ($1, $2, $3)
                     ON CONFLICT (app_code) DO UPDATE SET template_json = EXCLUDED.template_json,
                       updated_at = EXCLUDED.updated_at",
                ).bind(app_code).bind(template_json).bind(updated_at).execute(p).await?;
            }
        }
        Ok(())
    }
```

> Confirm the Postgres variant is named `ChatDb::Postgres(PgPool)` (grep `enum ChatDb`). If it differs, match the actual variant name.

- [ ] **Step 5: Add the pure functions + default template**

Add near the other pure helpers in `manager/src/main.rs` (e.g. just above `compose_own_usage_reply`):

```rust
/// One entry in a per-app sandbox template. `content` is empty for `dir:true`.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
struct TemplateEntry {
    path: String,
    #[serde(default)]
    dir: bool,
    #[serde(default)]
    content: String,
}

/// PURE: parse a template JSON array. Errors on non-array / bad shape.
fn parse_template(json: &str) -> Result<Vec<TemplateEntry>, String> {
    serde_json::from_str::<Vec<TemplateEntry>>(json)
        .map_err(|e| format!("invalid template JSON: {e}"))
}

/// The substitution context for a template materialization.
struct TemplateVars {
    name: String,
    user_id: String,
    app: String,
    date: String,
}

/// PURE: replace `{{name}}`/`{{userId}}`/`{{app}}`/`{{date}}` literally; leave
/// any other `{{…}}` untouched.
fn substitute_vars(content: &str, v: &TemplateVars) -> String {
    content
        .replace("{{name}}", &v.name)
        .replace("{{userId}}", &v.user_id)
        .replace("{{app}}", &v.app)
        .replace("{{date}}", &v.date)
}

/// The default kabytech sandbox template, seeded on startup (Sub-project 1 has
/// no Console editor yet). README + a starter config.json.
const KABYTECH_DEFAULT_TEMPLATE: &str = r#"[
  {"path":"README.md","dir":false,"content":"# kabytech workspace\n\nThis is {{name}}'s kabytech workspace ({{userId}}).\nCreated {{date}}.\n"},
  {"path":"config.json","dir":false,"content":"{\n  \"app\": \"{{app}}\",\n  \"version\": 1,\n  \"createdAt\": \"{{date}}\",\n  \"settings\": {}\n}\n"}
]"#;
```

- [ ] **Step 6: Run the tests to verify they pass**

Run: `cargo test -p llm-chat-manager template_tests`
Expected: PASS (5 tests).

- [ ] **Step 7: Commit**

```bash
cd /d/projects/llm-chat
git add manager/src/main.rs
git commit -m "feat(manager): app_sandbox_template store + template parse/substitute + kabytech default

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Task 3: Manager — `/provision` endpoint + startup seed

**Files:**
- Modify: `manager/src/main.rs` (startup seed after `chat_db` opens; `/provision` dispatch branch; `handle_provision`)

**Interfaces:**
- Consumes (Task 2): `ChatDb::get_template`/`upsert_template`, `parse_template`, `substitute_vars`, `TemplateVars`, `KABYTECH_DEFAULT_TEMPLATE`. Existing: `resolve_user_label`, `call_backend`, the captured-token handshake holder, `instance_ports`.
- Produces: `ws /provision` → `{type:"provision", ok, provisioned}`.

- [ ] **Step 1: Seed the kabytech default on startup**

In `main()`, just after the chat DB is opened (the `let (chat_db, db_descr) = open_chat_db().await?;` line, ~1116), add:

```rust
    // Seed the default kabytech sandbox template once (no-op if a row exists, so
    // a later Console edit survives restarts). Sub-project 1 has no editor yet.
    if chat_db.get_template("kabytech").await.ok().flatten().is_none() {
        let now = now_iso();
        if let Err(e) = chat_db.upsert_template("kabytech", KABYTECH_DEFAULT_TEMPLATE, &now).await {
            tracing::warn!(target: "manager", error = %e, "seeding kabytech template failed");
        } else {
            tracing::info!(target: "manager", "seeded default kabytech sandbox template");
        }
    }
```

- [ ] **Step 2: Add the `/provision` dispatch branch**

In `handle_client`, the captured-token section already routes `/identity` and `/chat` (both consume `captured_token`) before `drop(captured_token)`. Add a `/provision` branch right after the `/chat` branch and BEFORE the `drop`:

```rust
    if req_path == "/provision" {
        // Materialize the caller's app sandbox template. chat.user-gated; userId
        // from the verified token; self-scoped. Uses the user's own token only
        // for their /userinfo (name), then drops it — same posture as /identity.
        let uid = match user_id {
            Some(u) => u,
            None => return reject_no_user(ws).await,
        };
        let token = match captured_token {
            Some(t) => t,
            None => return reject_no_user(ws).await,
        };
        return handle_provision(ws, state, uid, captured_email, token).await;
    }
```

> This sits between the `/chat` branch and `drop(captured_token); drop(captured_email);`. Same move-on-diverging-path pattern as `/identity`/`/chat` (each branch returns).

- [ ] **Step 3: Add `handle_provision`**

Add after `handle_identity` in `manager/src/main.rs`:

```rust
async fn handle_provision(
    ws: tokio_tungstenite::WebSocketStream<TcpStream>,
    state: SharedState,
    user_id: String,
    email: Option<String>,
    token: String,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    use std::time::Duration;
    let (mut sink, mut stream) = ws.split();

    let text = match tokio::time::timeout(Duration::from_secs(30), stream.next()).await {
        Ok(Some(Ok(Message::Text(t)))) => t,
        Ok(Some(Ok(Message::Binary(b)))) => String::from_utf8_lossy(&b).into_owned(),
        _ => return Ok(()),
    };
    let v: serde_json::Value = match serde_json::from_str(&text) {
        Ok(v) => v,
        Err(e) => {
            let _ = sink.send(Message::Text(
                serde_json::json!({"type":"err","text":format!("bad JSON: {e}")}).to_string())).await;
            return Ok(());
        }
    };
    let app = v.get("app").and_then(|t| t.as_str()).unwrap_or("").to_string();
    if app.is_empty() {
        let _ = sink.send(Message::Text(
            serde_json::json!({"type":"err","text":"missing app"}).to_string())).await;
        return Ok(());
    }

    let (http, issuer, db, port) = {
        let st = state.lock().await;
        (st.http.clone(), st.issuer.clone(), st.chat_db.clone(), st.instance_ports.first().copied())
    };

    // Load the app's template; absent → no-op success.
    let raw = match db.get_template(&app).await {
        Ok(Some(j)) => j,
        Ok(None) => {
            let _ = sink.send(Message::Text(
                serde_json::json!({"type":"provision","ok":true,"provisioned":false}).to_string())).await;
            return Ok(());
        }
        Err(e) => {
            let _ = sink.send(Message::Text(
                serde_json::json!({"type":"err","text":format!("template load: {e}")}).to_string())).await;
            return Ok(());
        }
    };
    let entries = match parse_template(&raw) {
        Ok(t) => t,
        Err(e) => {
            let _ = sink.send(Message::Text(
                serde_json::json!({"type":"err","text":format!("template parse: {e}")}).to_string())).await;
            return Ok(());
        }
    };

    // Resolve the display name (best-effort) for {{name}}, then substitute.
    let name = match issuer.as_deref() {
        Some(iss) => resolve_user_label(&http, iss, &token, email.as_deref(), &user_id).await,
        None => user_id.clone(),
    };
    let vars = TemplateVars {
        name,
        user_id: user_id.clone(),
        app: app.clone(),
        date: chrono::Utc::now().format("%Y-%m-%d").to_string(),
    };
    let files: Vec<serde_json::Value> = entries.iter().map(|e| serde_json::json!({
        "path": e.path,
        "dir": e.dir,
        "content": substitute_vars(&e.content, &vars),
    })).collect();

    // Materialize via a worker (any worker — the box base is shared host fs).
    let Some(port) = port else {
        let _ = sink.send(Message::Text(
            serde_json::json!({"type":"err","text":"no worker available"}).to_string())).await;
        return Ok(());
    };
    let reply = call_backend(port, serde_json::json!({
        "cmd": "provision-app-box", "userId": user_id, "app": app, "files": files,
    })).await;
    match reply {
        Ok(r) if r.get("ok").and_then(|v| v.as_bool()) == Some(true) => {
            let _ = sink.send(Message::Text(
                serde_json::json!({"type":"provision","ok":true,"provisioned":true}).to_string())).await;
        }
        Ok(r) => {
            let _ = sink.send(Message::Text(serde_json::json!({"type":"err",
                "text": format!("provision: {}", r.get("error").and_then(|v| v.as_str()).unwrap_or("rejected"))}).to_string())).await;
        }
        Err(e) => {
            let _ = sink.send(Message::Text(
                serde_json::json!({"type":"err","text":format!("worker: {e}")}).to_string())).await;
        }
    }
    let _ = sink.send(Message::Close(None)).await;
    Ok(())
}
```

> `now_iso`, `Message`, `TcpStream`, `SharedState`, `StreamExt`/`SinkExt`, `chrono` are all already in scope/used in `main.rs`. `call_backend(port, req) -> Result<Value, …>` and `instance_ports: Vec<u16>` exist on `ManagerState`.

- [ ] **Step 4: Build + test the manager**

Run: `cargo test -p llm-chat-manager`
Expected: PASS — Task 2's `template_tests` plus all existing tests; clean build (no warnings).

- [ ] **Step 5: Commit**

```bash
cd /d/projects/llm-chat
git add manager/src/main.rs
git commit -m "feat(manager): /provision endpoint + startup seed of the kabytech template

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Task 4: kabytech-backend — provision client + `/callback` hook + config

**Files:**
- Modify: `services/kabytech/backend/Cargo.toml` (add `tokio-tungstenite`, `futures-util`)
- Create: `services/kabytech/backend/src/provision.rs`
- Modify: `services/kabytech/backend/src/config.rs` (optional `manager_provision_url` + `app_code`)
- Modify: `services/kabytech/backend/src/auth.rs` (`callback` fires the provision)
- Modify: `services/kabytech/backend/src/lib.rs` (declare `mod provision`; thread cfg into `AppState` if needed)

**Interfaces:**
- Consumes (Task 3): the manager `ws /provision` (`{type:"provision",app}` → `{type:"provision",ok}`).
- Produces: `provision::provision_app_box(url: &str, token: &str, app: &str) -> Result<(), String>`;
  `KabyConfig.manager_provision_url: Option<String>`, `KabyConfig.app_code: String`.

- [ ] **Step 1: Add the WS client dep**

In `services/kabytech/backend/Cargo.toml` `[dependencies]`, add:

```toml
tokio-tungstenite = "0.21"
futures-util = "0.3"
```

(`0.21` matches the manager + admin-api versions — keep them aligned.)

- [ ] **Step 2: Write the provision client (mirror admin-api/src/manager.rs)**

Create `services/kabytech/backend/src/provision.rs`:

```rust
//! Best-effort WS client that asks the manager to materialize THIS user's app
//! sandbox template at first login. Mirrors admin-api/src/manager.rs: the token
//! rides the Authorization: Bearer header (never the URL). The manager's
//! /provision verifies the JWT (chat.user) and self-scopes to the token's user.

use futures_util::{SinkExt, StreamExt};
use serde_json::json;
use tokio_tungstenite::tungstenite::Message;

/// Open the manager /provision WS, send one provision request, read one reply.
/// Best-effort: a transport/timeout/err reply is returned as Err for the caller
/// to log — it must NOT block login. 8s overall budget.
pub async fn provision_app_box(url: &str, token: &str, app: &str) -> Result<(), String> {
    use tokio_tungstenite::tungstenite::client::IntoClientRequest;
    let mut req = url.into_client_request().map_err(|e| format!("bad provision url {url}: {e}"))?;
    req.headers_mut().insert(
        "Authorization",
        format!("Bearer {token}").parse().map_err(|e| format!("bad auth header: {e}"))?,
    );
    let connect = tokio_tungstenite::connect_async(req);
    let (mut ws, _) = tokio::time::timeout(std::time::Duration::from_secs(8), connect)
        .await
        .map_err(|_| "provision connect timeout".to_string())?
        .map_err(|e| format!("provision connect: {e}"))?;
    ws.send(Message::Text(json!({"type":"provision","app":app}).to_string()))
        .await
        .map_err(|e| format!("provision send: {e}"))?;
    let reply = match tokio::time::timeout(std::time::Duration::from_secs(8), ws.next()).await {
        Ok(Some(Ok(Message::Text(t)))) => t,
        Ok(Some(Ok(Message::Binary(b)))) => String::from_utf8_lossy(&b).into_owned(),
        Ok(_) => return Err("provision: closed early".into()),
        Err(_) => return Err("provision reply timeout".into()),
    };
    let _ = ws.close(None).await;
    let v: serde_json::Value = serde_json::from_str(&reply).map_err(|e| format!("provision reply: {e}"))?;
    if v.get("type").and_then(|t| t.as_str()) == Some("provision")
        && v.get("ok").and_then(|t| t.as_bool()) == Some(true)
    {
        Ok(())
    } else {
        Err(format!("provision rejected: {reply}"))
    }
}
```

- [ ] **Step 3: Add the config fields**

In `services/kabytech/backend/src/config.rs`, add to `KabyConfig`:

```rust
    /// Optional manager /provision WS URL (e.g. ws://manager:7777/provision). When
    /// set, first-login provisions the user's app sandbox (best-effort). Absent →
    /// the feature is off and login is unaffected.
    pub manager_provision_url: Option<String>,
    /// The app code this gateway provisions under (default "kabytech").
    pub app_code: String,
```

and in `from_map` (inside the `Ok(KabyConfig { … })`), add:

```rust
            manager_provision_url: get("KABY_MANAGER_PROVISION_URL")
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty()),
            app_code: get("KABY_APP_CODE")
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| "kabytech".to_string()),
```

Then update the `full_map()` test helper in `config.rs` tests so existing tests still pass (these are optional vars, so `full_map` needs no change; but add a focused test):

```rust
    #[test]
    fn app_code_defaults_to_kabytech_and_provision_url_optional() {
        let cfg = KabyConfig::from_map(&getter(full_map())).expect("ok");
        assert_eq!(cfg.app_code, "kabytech");
        assert_eq!(cfg.manager_provision_url, None);
        let mut m = full_map();
        m.insert("KABY_APP_CODE", "other");
        m.insert("KABY_MANAGER_PROVISION_URL", "ws://manager:7777/provision");
        let cfg = KabyConfig::from_map(&getter(m)).expect("ok");
        assert_eq!(cfg.app_code, "other");
        assert_eq!(cfg.manager_provision_url.as_deref(), Some("ws://manager:7777/provision"));
    }
```

- [ ] **Step 4: Declare the module + fire the provision in `callback`**

In `services/kabytech/backend/src/lib.rs` (or wherever the other `mod` declarations live — check `main.rs` too), add:

```rust
pub mod provision;
```

In `services/kabytech/backend/src/auth.rs`, in `callback`, after the `login_at` is inserted and BEFORE the final `Redirect::to(...)`, add the best-effort provision:

```rust
    // Best-effort: materialize this user's app sandbox on first login. Never
    // blocks login — log and continue regardless.
    if let Some(url) = st.cfg.manager_provision_url.clone() {
        let app = st.cfg.app_code.clone();
        let tok = token.clone();
        tokio::spawn(async move {
            if let Err(e) = crate::provision::provision_app_box(&url, &tok, &app).await {
                tracing::warn!(target: "kaby::provision", error = %e, "first-login provision failed (ignored)");
            } else {
                tracing::info!(target: "kaby::provision", app = %app, "provisioned user sandbox");
            }
        });
    }
```

> `token` is the access token already obtained earlier in `callback` (the one passed to `verify_sync`/`fetch_display_name`). Confirm it's still in scope at that point (it is — it's a `let token = …` near the top of `callback`). `tracing` is already used in the backend.

- [ ] **Step 5: Build + test the kabytech backend**

Run: `cargo test -p kabytech-backend`
Expected: PASS (incl. the new config test); clean build.

> If the crate name isn't `kabytech-backend`, use the name from `services/kabytech/backend/Cargo.toml` `[package] name`.

- [ ] **Step 6: Commit**

```bash
cd /d/projects/llm-chat
git add services/kabytech/backend/Cargo.toml services/kabytech/backend/src/provision.rs services/kabytech/backend/src/config.rs services/kabytech/backend/src/auth.rs services/kabytech/backend/src/lib.rs
git commit -m "feat(kabytech): best-effort first-login sandbox provision via manager /provision

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Task 5: Config — wire `KABY_MANAGER_PROVISION_URL` + `KABY_APP_CODE`

**Files:**
- Modify: `.env.local.example` (kabytech-backend section)
- Modify: `docker-compose.yml` (kabytech-backend `environment:`)

**Interfaces:** Consumes Task 4's config reads.

- [ ] **Step 1: Document in `.env.local.example`**

In the `# ── kabytech-backend ──` block of `.env.local.example`, after `KABY_SA_KEY_PATH=...`, add:

```
# First-login sandbox provisioning (optional). When set, on a user's first
# kabytech login the manager materializes that user's {userId}/kabytech/ sandbox
# from the app template. Absent -> feature off, login unaffected.
KABY_MANAGER_PROVISION_URL=ws://manager:7777/provision
KABY_APP_CODE=kabytech
```

- [ ] **Step 2: Ensure the kabytech-backend container reads them**

`.env.local` is already an `env_file` for kabytech-backend (see `docker-compose.yml`), so adding the vars to `.env.local.example` (and the live `.env.local`) is sufficient — no `environment:` change needed. Confirm by grepping:

Run: `grep -nE "KABY_MANAGER_PROVISION_URL|KABY_APP_CODE" .env.local.example`
Expected: both lines present. (If you keep a live `.env.local`, add the same two lines there so the running container picks them up.)

- [ ] **Step 3: Commit**

```bash
cd /d/projects/llm-chat
git add .env.local.example
git commit -m "feat(config): KABY_MANAGER_PROVISION_URL + KABY_APP_CODE for first-login provisioning

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Task 6: End-to-end verification + finish

**Files:** none (build + manual verification).

- [ ] **Step 1: Rebuild manager + kabytech-backend**

Run: `docker compose up -d --build --no-deps manager kabytech-backend`
Expected: both rebuilt + restarted, no errors. (Ensure the live `.env.local` has the two `KABY_*` lines so the container enables provisioning.)

- [ ] **Step 2: Confirm the manager seeded the template + endpoint is live**

Run: `docker compose logs manager --since 60s 2>&1 | grep -iE "seeded default kabytech|listening"`
Expected: "seeded default kabytech sandbox template" (first boot) + "manager listening".

- [ ] **Step 3: Drive a fresh kabytech login and check the box**

Run the existing login flow for a user with no kabytech box yet (e.g. a freshly created user, or delete the box dir first):
```bash
# from repo root, using the scratchpad login flow used during this project:
python <scratchpad>/login_kaby.py kabyuser '<password>'
# then verify the materialized files (the worker writes under .user-envs on the host):
ls -la ".user-envs/379495528934670340/kabytech/"
cat ".user-envs/379495528934670340/kabytech/README.md"
cat ".user-envs/379495528934670340/kabytech/config.json"
```
Expected: `README.md` + `config.json` exist; `README.md` shows the resolved name + today's date (substituted); `config.json` has `"app": "kabytech"`.

- [ ] **Step 4: Idempotency check**

Edit `.user-envs/{userId}/kabytech/README.md` (append a line), log in again, and re-check:
```bash
python <scratchpad>/login_kaby.py kabyuser '<password>'
cat ".user-envs/<userId>/kabytech/README.md"
```
Expected: your edit is preserved (write-if-absent — the second login did not overwrite it).

- [ ] **Step 5: Fail-soft check**

Temporarily point `KABY_MANAGER_PROVISION_URL` at a dead port (or stop the manager), log in, and confirm **login still succeeds** (the provision error is only logged). Restore.

- [ ] **Step 6: Finish the branch**

Announce and use **superpowers:finishing-a-development-branch**: verify `cargo test -p llm-chat-manager`, `cargo test -p llm-chat --no-default-features`, and `cargo test -p kabytech-backend` pass, then present the standard options. Single-branch (`main`) workflow — explicit-path commits.

---

## Self-Review

**Spec coverage:**
- Manager template store (`app_sandbox_template` table + get/upsert) → Task 2. ✓
- Pure `parse_template` + `substitute_vars` ({{name}}/{{userId}}/{{app}}/{{date}}) → Task 2. ✓
- Startup seed of the kabytech default → Task 3 Step 1. ✓
- `/provision` chat.user-gated, userId-from-token, self-scoped → Task 3 (dispatch reuses captured token like `/identity`). ✓
- Worker `provision-app-box` reusing `confine_path`, write-if-absent → Task 1. ✓
- kabytech `/callback` best-effort hook + WS client + config → Task 4. ✓
- Config wiring (optional, feature-off when absent) → Tasks 4 & 5. ✓
- Idempotency, fail-closed confinement, fail-soft login, variables → enforced in Tasks 1/3/4 + verified in Task 6. ✓

**Placeholder scan:** none — every code step has complete code; every command an expected result.

**Type consistency:** `SeedEntry{path,dir,content}` (worker) and `TemplateEntry{path,dir,content}` (manager) carry the same fields; the manager serializes `{path,dir,content}` JSON the worker deserializes into `SeedEntry` — names align via JSON keys. `parse_template`/`substitute_vars`/`TemplateVars`/`KABYTECH_DEFAULT_TEMPLATE` defined in Task 2, consumed in Task 3. `ChatDb::get_template`/`upsert_template` defined Task 2, used in Task 3's seed + `handle_provision`. `provision_app_box(url,token,app)` defined Task 4, called in `callback`. `handle_provision(ws,state,user_id,email,token)` mirrors `handle_identity`'s signature.

**One thing to verify at implementation time (noted in Task 2/4):** the exact `ChatDb` Postgres variant name and the kabytech crate `[package] name` — grep before coding those two lines.
