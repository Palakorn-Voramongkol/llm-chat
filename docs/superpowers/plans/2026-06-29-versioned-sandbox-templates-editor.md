# Versioned Sandbox Templates — Console Editor + LLM Migration Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Let operators author versioned per-app sandbox templates in the admin-web Console (two-pane tree + content editor) and have each user's sandbox be created on first login and LLM-migrated when a newer version is published.

**Architecture:** admin-web editor on the OIDC-client row → admin-api (resolves `clientId → app_code` from `secrets/app_codes.json`, mints SA token) → manager `/control` `get/set-template` (store gains `version` + `migrate_instructions`) → on login, manager `/provision` passes the version + instructions to the worker's `provision-app-box`, which reads a per-user stamp file and, when older, runs claude confined to `{userId}/{app}/` in a background task and bumps the stamp on success.

**Tech Stack:** Rust (manager `llm-chat-manager`, worker `llm-chat`/lib `llm_chat_lib`, admin-api `llm-chat-admin-api`), sqlx (sqlite + postgres), tokio, claude CLI (stream-json), Next.js 16 / React 19 / TypeScript / shadcn-ui / vitest (admin-web).

**Design doc:** `docs/superpowers/specs/2026-06-29-versioned-sandbox-templates-editor-design.md`

## Global Constraints

- **Fail closed (security-sensitive).** No fallbacks, no silent defaults on auth/identity/secrets/path boundaries. A required-but-missing/invalid value rejects loudly. (CLAUDE.md)
- **Backend authoritative.** Clients never map id→meaning or compute domain values; admin-api resolves `clientId → app_code`, the manager computes the version. (CLAUDE.md)
- **Source of truth, not scrape.** The migration drives claude with `--output-format stream-json` and reads its `result` event — never scrapes a TTY. (CLAUDE.md)
- **No dirty fixes.** Root-cause failures; never skip a check, hardcode around a bug, or catch-and-ignore. (CLAUDE.md)
- **Confinement reuse.** Every sandbox path goes through `worker/src/user_env.rs::confine_path` / `resolve_user_cwd` (rejects `..`/absolute/`\`/`:`/NUL; canonicalize-proven; symlink-safe). Never bypass it.
- **Version is a monotonic integer.** It increases ONLY on an explicit Publish; Publish REQUIRES non-empty migration instructions.
- **Per-user stamp** lives at `{userId}/{app}/.llm-chat/version`. Missing/unreadable/malformed → treated as v0 (full provision). Written by the worker only, never by the migration's claude run, and only on verified success.
- **Migration is async/best-effort.** Login is never blocked; a migration failure leaves the stamp unchanged (retried next login) and never destroys data.
- **shadcn/ui only** for admin-web UI primitives (no hand-rolled `<button>`/`<textarea>`); add a missing primitive with the shadcn CLI. Read `node_modules/next/dist/docs/` before writing Next.js code. (admin-web/AGENTS.md)
- **Test commands:** manager `cargo test -p llm-chat-manager <name>`; worker `cargo test -p llm-chat --no-default-features <name>`; admin-api `cargo test -p llm-chat-admin-api <name>`; admin-web (from `admin-web/`) `npx vitest run <file>`.
- **Commits:** explicit-path `git add` only (shared branch). Trailer `Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>`.

---

### Task 1: Manager — versioned template store

**Files:**
- Modify: `manager/src/main.rs` — `init_schema_sqlite` (~698-756), `init_schema_postgres` (~759-814), `ChatDb::get_template` (318-339), `ChatDb::upsert_template` (342-377), the startup seed (1364-1375), and the inline tests (~3880-3890).

**Interfaces:**
- Produces:
  - `struct TemplateRecord { pub template_json: String, pub version: i64, pub migrate_instructions: Option<String>, pub updated_at: String }`
  - `ChatDb::get_template(&self, app_code: &str) -> Result<Option<TemplateRecord>, sqlx::Error>`
  - `ChatDb::upsert_template(&self, app_code: &str, template_json: &str, version: i64, migrate_instructions: Option<&str>, updated_at: &str) -> Result<(), sqlx::Error>`

- [ ] **Step 1: Add the version + instructions columns to both schemas**

In `init_schema_sqlite`, the existing block creates `app_sandbox_template` (app_code, template_json, updated_at). Add the two columns to the `CREATE TABLE` and an idempotent ALTER pair right after it (SQLite has no `ADD COLUMN IF NOT EXISTS` — ignore the duplicate-column error, matching the existing `chat_question` migrations at 718-741):

```rust
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS app_sandbox_template (
            app_code TEXT PRIMARY KEY,
            template_json TEXT NOT NULL,
            version INTEGER NOT NULL DEFAULT 1,
            migrate_instructions TEXT,
            updated_at TEXT NOT NULL
        );",
    )
    .execute(pool)
    .await?;
    // Older DBs predate version/migrate_instructions — add them idempotently.
    let _ = sqlx::query("ALTER TABLE app_sandbox_template ADD COLUMN version INTEGER NOT NULL DEFAULT 1;")
        .execute(pool)
        .await;
    let _ = sqlx::query("ALTER TABLE app_sandbox_template ADD COLUMN migrate_instructions TEXT;")
        .execute(pool)
        .await;
```

In `init_schema_postgres`, mirror it with `ADD COLUMN IF NOT EXISTS` (Postgres supports it, matching 779-799):

```rust
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS app_sandbox_template (
            app_code TEXT PRIMARY KEY,
            template_json TEXT NOT NULL,
            version BIGINT NOT NULL DEFAULT 1,
            migrate_instructions TEXT,
            updated_at TEXT NOT NULL
        );",
    )
    .execute(pool)
    .await?;
    sqlx::query("ALTER TABLE app_sandbox_template ADD COLUMN IF NOT EXISTS version BIGINT NOT NULL DEFAULT 1;")
        .execute(pool)
        .await?;
    sqlx::query("ALTER TABLE app_sandbox_template ADD COLUMN IF NOT EXISTS migrate_instructions TEXT;")
        .execute(pool)
        .await?;
```

- [ ] **Step 2: Define `TemplateRecord` and rewrite `get_template`/`upsert_template`**

Replace the two methods (318-377). Add `TemplateRecord` just above `get_template` (inside `impl ChatDb` is fine as a free struct above the impl — place it next to `TemplateEntry` at ~268 or directly above the impl; keep it `#[derive(Debug, Clone, PartialEq, Eq)]`).

```rust
/// A stored sandbox-template row (Sub-project 2 added version + migrate_instructions).
#[derive(Debug, Clone, PartialEq, Eq)]
struct TemplateRecord {
    template_json: String,
    version: i64,
    migrate_instructions: Option<String>,
    updated_at: String,
}
```

```rust
    /// The stored sandbox-template row for an app code, or None.
    pub async fn get_template(&self, app_code: &str) -> Result<Option<TemplateRecord>, sqlx::Error> {
        match self {
            ChatDb::Sqlite(p) => {
                let row: Option<(String, i64, Option<String>, String)> = sqlx::query_as(
                    "SELECT template_json, version, migrate_instructions, updated_at
                     FROM app_sandbox_template WHERE app_code = ?",
                )
                .bind(app_code)
                .fetch_optional(p)
                .await?;
                Ok(row.map(|r| TemplateRecord {
                    template_json: r.0, version: r.1, migrate_instructions: r.2, updated_at: r.3,
                }))
            }
            ChatDb::Postgres(p) => {
                let row: Option<(String, i64, Option<String>, String)> = sqlx::query_as(
                    "SELECT template_json, version, migrate_instructions, updated_at
                     FROM app_sandbox_template WHERE app_code = $1",
                )
                .bind(app_code)
                .fetch_optional(p)
                .await?;
                Ok(row.map(|r| TemplateRecord {
                    template_json: r.0, version: r.1, migrate_instructions: r.2, updated_at: r.3,
                }))
            }
        }
    }

    /// Insert or replace the sandbox template for an app code.
    pub async fn upsert_template(
        &self,
        app_code: &str,
        template_json: &str,
        version: i64,
        migrate_instructions: Option<&str>,
        updated_at: &str,
    ) -> Result<(), sqlx::Error> {
        match self {
            ChatDb::Sqlite(p) => {
                sqlx::query(
                    "INSERT INTO app_sandbox_template (app_code, template_json, version, migrate_instructions, updated_at)
                     VALUES (?, ?, ?, ?, ?)
                     ON CONFLICT(app_code) DO UPDATE SET template_json = excluded.template_json,
                       version = excluded.version, migrate_instructions = excluded.migrate_instructions,
                       updated_at = excluded.updated_at",
                )
                .bind(app_code).bind(template_json).bind(version)
                .bind(migrate_instructions).bind(updated_at)
                .execute(p).await?;
            }
            ChatDb::Postgres(p) => {
                sqlx::query(
                    "INSERT INTO app_sandbox_template (app_code, template_json, version, migrate_instructions, updated_at)
                     VALUES ($1, $2, $3, $4, $5)
                     ON CONFLICT (app_code) DO UPDATE SET template_json = EXCLUDED.template_json,
                       version = EXCLUDED.version, migrate_instructions = EXCLUDED.migrate_instructions,
                       updated_at = EXCLUDED.updated_at",
                )
                .bind(app_code).bind(template_json).bind(version)
                .bind(migrate_instructions).bind(updated_at)
                .execute(p).await?;
            }
        }
        Ok(())
    }
```

- [ ] **Step 3: Update the startup seed call**

In the seed block (1364-1375), the `Ok(None)` arm calls `upsert_template`. Update it to pass the version + None instructions:

```rust
            if let Err(e) = chat_db
                .upsert_template("kabytech", KABYTECH_DEFAULT_TEMPLATE, 1, None, &now)
                .await
            {
```

- [ ] **Step 4: Update the existing inline tests to the new shapes**

Find the round-trip test (~3880-3890) that does `db.upsert_template("kabytech", "[]", "…")` and `get_template(...).as_deref()`. Replace its body with:

```rust
        assert!(db.get_template("kabytech").await.unwrap().is_none());
        db.upsert_template("kabytech", "[]", 1, None, "2026-06-29T00:00:00Z").await.unwrap();
        let rec = db.get_template("kabytech").await.unwrap().unwrap();
        assert_eq!(rec.template_json, "[]");
        assert_eq!(rec.version, 1);
        assert_eq!(rec.migrate_instructions, None);
        db.upsert_template("kabytech", "[{\"path\":\"x\",\"dir\":true,\"content\":\"\"}]", 2, Some("move things"), "t2").await.unwrap();
        let rec = db.get_template("kabytech").await.unwrap().unwrap();
        assert!(rec.template_json.contains("\"x\""));
        assert_eq!(rec.version, 2);
        assert_eq!(rec.migrate_instructions.as_deref(), Some("move things"));
```

- [ ] **Step 5: Run the manager tests**

Run: `cargo test -p llm-chat-manager template`
Expected: the round-trip test passes; the build compiles (other `get_template`/`upsert_template` callers may still fail to compile — Task 2/3 fix those; if so, complete Steps in this task by making the file compile, i.e. temporarily the `handle_provision` caller at ~1888 must be updated too — do it now as part of this task since it consumes `get_template`). Update `handle_provision`'s match (1888-1900) to the record shape minimally so the crate compiles:

```rust
    let rec = match db.get_template(&app).await {
        Ok(Some(r)) => r,
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
    let raw = rec.template_json.clone();
```

(Task 3 then uses `rec.version` / `rec.migrate_instructions`.) Re-run: `cargo test -p llm-chat-manager` → PASS.

- [ ] **Step 6: Commit**

```bash
git add manager/src/main.rs
git commit -m "feat(manager): version + migrate_instructions in app_sandbox_template store"
```

---

### Task 2: Manager — `resolve_set` + `get-template`/`set-template` control commands

**Files:**
- Modify: `manager/src/main.rs` — add `resolve_set` near the template helpers (~292), add two arms to `handle_control` (after the `user-box` arm, ~2147), add tests to the inline test module.

**Interfaces:**
- Consumes: `ChatDb::get_template`/`upsert_template`, `parse_template` (271), `now_iso`.
- Produces: `fn resolve_set(current_version: Option<i64>, current_instructions: Option<String>, publish: bool, new_instructions: Option<&str>) -> Result<(i64, Option<String>), String>`

- [ ] **Step 1: Write the failing test for `resolve_set`**

Add to the inline `#[cfg(test)] mod tests` in `manager/src/main.rs`:

```rust
    #[test]
    fn resolve_set_rules() {
        // brand-new app: establishes v1 regardless of publish; instructions ignored
        assert_eq!(resolve_set(None, None, false, None), Ok((1, None)));
        assert_eq!(resolve_set(None, None, true, Some("x")), Ok((1, Some("x".into()))));
        // content edit on existing row: keep version, preserve prior instructions
        assert_eq!(resolve_set(Some(3), Some("old".into()), false, None), Ok((3, Some("old".into()))));
        // publish on existing row: bump, require non-empty instructions
        assert_eq!(resolve_set(Some(3), Some("old".into()), true, Some("  do it  ")), Ok((4, Some("do it".into()))));
        assert!(resolve_set(Some(3), None, true, None).is_err());
        assert!(resolve_set(Some(3), None, true, Some("   ")).is_err());
    }
```

- [ ] **Step 2: Run it to verify it fails**

Run: `cargo test -p llm-chat-manager resolve_set_rules`
Expected: FAIL — `cannot find function resolve_set`.

- [ ] **Step 3: Implement `resolve_set`**

Add near `substitute_vars` (~292):

```rust
/// PURE: decide the stored (version, instructions) for a set-template request.
/// New app (no current row) → v1 (publish flag irrelevant; nothing to migrate
/// from). Existing row + content edit (publish=false) → keep the version and
/// PRESERVE the prior instructions. Existing row + publish=true → version+1 and
/// REQUIRE non-empty instructions (fail closed). Instructions are trimmed.
fn resolve_set(
    current_version: Option<i64>,
    current_instructions: Option<String>,
    publish: bool,
    new_instructions: Option<&str>,
) -> Result<(i64, Option<String>), String> {
    if publish {
        let instr = new_instructions
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .ok_or_else(|| "publishing a new version requires migration instructions".to_string())?;
        Ok((current_version.unwrap_or(0) + 1, Some(instr.to_string())))
    } else {
        Ok((current_version.unwrap_or(1), current_instructions))
    }
}
```

- [ ] **Step 4: Run the test → PASS**

Run: `cargo test -p llm-chat-manager resolve_set_rules`
Expected: PASS.

- [ ] **Step 5: Add the `get-template` and `set-template` control arms**

In `handle_control`, after the `"user-box"` arm closes (the `}` at ~2147, before `"fifo"`), insert:

```rust
            "get-template" => {
                match req.get("appCode").and_then(|v| v.as_str()).filter(|s| !s.is_empty()) {
                    None => serde_json::json!({"ok": false, "error": "appCode required"}),
                    Some(code) => {
                        let db = state.lock().await.chat_db.clone();
                        match db.get_template(code).await {
                            Ok(Some(rec)) => {
                                let template: serde_json::Value =
                                    serde_json::from_str(&rec.template_json)
                                        .unwrap_or_else(|_| serde_json::json!([]));
                                serde_json::json!({
                                    "ok": true, "appCode": code, "version": rec.version,
                                    "template": template, "migrateInstructions": rec.migrate_instructions,
                                    "updatedAt": rec.updated_at,
                                })
                            }
                            Ok(None) => serde_json::json!({
                                "ok": true, "appCode": code, "version": 0,
                                "template": [], "migrateInstructions": serde_json::Value::Null,
                                "updatedAt": serde_json::Value::Null,
                            }),
                            Err(e) => serde_json::json!({"ok": false, "error": format!("get-template: {e}")}),
                        }
                    }
                }
            }
            "set-template" => {
                let code = req.get("appCode").and_then(|v| v.as_str()).filter(|s| !s.is_empty());
                let template = req.get("template");
                match (code, template) {
                    (None, _) => serde_json::json!({"ok": false, "error": "appCode required"}),
                    (_, None) => serde_json::json!({"ok": false, "error": "template required"}),
                    (Some(code), Some(template)) => {
                        // Validate the entries shape (fail closed on bad template).
                        let template_json = template.to_string();
                        if let Err(e) = parse_template(&template_json) {
                            serde_json::json!({"ok": false, "error": format!("invalid template: {e}")})
                        } else {
                            let publish = req.get("publish").and_then(|v| v.as_bool()).unwrap_or(false);
                            let new_instr = req.get("migrateInstructions").and_then(|v| v.as_str());
                            let db = state.lock().await.chat_db.clone();
                            let current = match db.get_template(code).await {
                                Ok(c) => c,
                                Err(e) => return_err_inline(&mut sink, &format!("set-template load: {e}")).await,
                            };
                            let (cur_v, cur_instr) = match &current {
                                Some(r) => (Some(r.version), r.migrate_instructions.clone()),
                                None => (None, None),
                            };
                            match resolve_set(cur_v, cur_instr, publish, new_instr) {
                                Err(e) => serde_json::json!({"ok": false, "error": e}),
                                Ok((version, instr)) => {
                                    let now = now_iso();
                                    match db.upsert_template(code, &template_json, version, instr.as_deref(), &now).await {
                                        Ok(()) => serde_json::json!({"ok": true, "appCode": code, "version": version, "updatedAt": now}),
                                        Err(e) => serde_json::json!({"ok": false, "error": format!("set-template save: {e}")}),
                                    }
                                }
                            }
                        }
                    }
                }
            }
```

Note: the `return_err_inline` helper above is a shortcut that does not fit the `match` arm's `Value` type — DO NOT use it. Instead handle the load error inline by yielding a `Value`:

```rust
                            let current = match db.get_template(code).await {
                                Ok(c) => c,
                                Err(e) => {
                                    // emit the error as this arm's reply value
                                    return_value_on_err(e)
                                }
                            };
```

Simpler and correct — replace the whole `current`/`(cur_v, cur_instr)` lead-in with this exact form that keeps the arm returning a single `Value`:

```rust
                            match db.get_template(code).await {
                                Err(e) => serde_json::json!({"ok": false, "error": format!("set-template load: {e}")}),
                                Ok(current) => {
                                    let (cur_v, cur_instr) = match &current {
                                        Some(r) => (Some(r.version), r.migrate_instructions.clone()),
                                        None => (None, None),
                                    };
                                    match resolve_set(cur_v, cur_instr, publish, new_instr) {
                                        Err(e) => serde_json::json!({"ok": false, "error": e}),
                                        Ok((version, instr)) => {
                                            let now = now_iso();
                                            match db.upsert_template(code, &template_json, version, instr.as_deref(), &now).await {
                                                Ok(()) => serde_json::json!({"ok": true, "appCode": code, "version": version, "updatedAt": now}),
                                                Err(e) => serde_json::json!({"ok": false, "error": format!("set-template save: {e}")}),
                                            }
                                        }
                                    }
                                }
                            }
```

Use this corrected form; the `return_err_inline`/`return_value_on_err` names are illustrative only and must not appear in the code.

- [ ] **Step 6: Verify the crate compiles + tests pass**

Run: `cargo test -p llm-chat-manager`
Expected: PASS (build clean, `resolve_set_rules` green).

- [ ] **Step 7: Commit**

```bash
git add manager/src/main.rs
git commit -m "feat(manager): get-template/set-template control commands (version computed server-side)"
```

---

### Task 3: Manager — `handle_provision` passes version + instructions

**Files:**
- Modify: `manager/src/main.rs` — `handle_provision` (1853-1949); add a pure `build_provision_request` helper + test.

**Interfaces:**
- Consumes: `TemplateRecord` (Task 1), `substitute_vars`, `call_backend`.
- Produces: `fn build_provision_request(user_id: &str, app: &str, version: i64, files: Vec<serde_json::Value>, migrate_instructions: Option<&str>) -> serde_json::Value`

- [ ] **Step 1: Write the failing test**

Add to the inline test module:

```rust
    #[test]
    fn build_provision_request_shape() {
        let files = vec![serde_json::json!({"path":"README.md","dir":false,"content":"hi"})];
        let v = build_provision_request("u1", "kabytech", 3, files, Some("do the move"));
        assert_eq!(v["cmd"], "provision-app-box");
        assert_eq!(v["userId"], "u1");
        assert_eq!(v["app"], "kabytech");
        assert_eq!(v["version"], 3);
        assert_eq!(v["files"][0]["path"], "README.md");
        assert_eq!(v["migrateInstructions"], "do the move");
        // None instructions → JSON null
        let v2 = build_provision_request("u1", "kabytech", 1, vec![], None);
        assert!(v2["migrateInstructions"].is_null());
    }
```

- [ ] **Step 2: Run it → FAIL** (`cannot find function build_provision_request`)

Run: `cargo test -p llm-chat-manager build_provision_request_shape`

- [ ] **Step 3: Implement the helper**

Add near `handle_provision` (above it):

```rust
/// PURE: assemble the worker `provision-app-box` request. `files` are the
/// already-variable-substituted entries. `migrate_instructions` are operator
/// prose (NOT variable-substituted) used only when the worker must migrate.
fn build_provision_request(
    user_id: &str,
    app: &str,
    version: i64,
    files: Vec<serde_json::Value>,
    migrate_instructions: Option<&str>,
) -> serde_json::Value {
    serde_json::json!({
        "cmd": "provision-app-box",
        "userId": user_id,
        "app": app,
        "version": version,
        "files": files,
        "migrateInstructions": migrate_instructions,
    })
}
```

- [ ] **Step 4: Wire it into `handle_provision`**

Replace the `call_backend(...)` request construction (1931-1933) with:

```rust
    let reply = call_backend(
        port,
        build_provision_request(&user_id, &app, rec.version, files, rec.migrate_instructions.as_deref()),
    )
    .await;
```

(`rec` is the `TemplateRecord` from Task 1 Step 5; `files` is the substituted vec already built at 1920-1924.)

- [ ] **Step 5: Run tests → PASS**

Run: `cargo test -p llm-chat-manager`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add manager/src/main.rs
git commit -m "feat(manager): pass template version + migrate instructions to the worker on provision"
```

---

### Task 4: Worker — version stamp helpers + `decide_action`

**Files:**
- Modify: `worker/src/user_env.rs` — add helpers + tests at the end of the file (before the existing `#[cfg(test)]` or inside it for tests).

**Interfaces:**
- Consumes: `confine_path`, `ResolveError`.
- Produces:
  - `pub enum ProvisionAction { Provision, Current, Migrate }`
  - `pub fn decide_action(stamp: i64, target: i64) -> ProvisionAction`
  - `pub fn parse_stamp(raw: Option<&str>) -> i64`
  - `pub fn read_stamp(base: &Path, user_id: &str, app: &str) -> Result<i64, ResolveError>`
  - `pub fn write_stamp(base: &Path, user_id: &str, app: &str, version: i64) -> Result<(), ResolveError>`

- [ ] **Step 1: Write the failing tests**

Add to the `#[cfg(test)] mod tests` in `worker/src/user_env.rs`:

```rust
    #[test]
    fn parse_stamp_handles_missing_and_malformed() {
        assert_eq!(parse_stamp(None), 0);
        assert_eq!(parse_stamp(Some("")), 0);
        assert_eq!(parse_stamp(Some("  ")), 0);
        assert_eq!(parse_stamp(Some("notanint")), 0);
        assert_eq!(parse_stamp(Some("5")), 5);
        assert_eq!(parse_stamp(Some("  7\n")), 7);
    }

    #[test]
    fn decide_action_branches() {
        assert!(matches!(decide_action(0, 1), ProvisionAction::Provision));
        assert!(matches!(decide_action(-3, 2), ProvisionAction::Provision));
        assert!(matches!(decide_action(2, 2), ProvisionAction::Current));
        assert!(matches!(decide_action(3, 2), ProvisionAction::Current));
        assert!(matches!(decide_action(1, 2), ProvisionAction::Migrate));
    }

    #[test]
    fn stamp_round_trip_and_absent_is_zero() {
        let tmp = tempfile::tempdir().unwrap();
        let base = tmp.path();
        // absent marker → 0 (no box yet)
        assert_eq!(read_stamp(base, "u1", "kabytech").unwrap(), 0);
        write_stamp(base, "u1", "kabytech", 4).unwrap();
        assert_eq!(read_stamp(base, "u1", "kabytech").unwrap(), 4);
        // overwrite
        write_stamp(base, "u1", "kabytech", 5).unwrap();
        assert_eq!(read_stamp(base, "u1", "kabytech").unwrap(), 5);
    }

    #[test]
    fn stamp_rejects_bad_user() {
        let tmp = tempfile::tempdir().unwrap();
        assert!(matches!(read_stamp(tmp.path(), "", "kabytech"), Err(ResolveError::BadUser(_))));
        assert!(matches!(write_stamp(tmp.path(), "..", "kabytech", 1), Err(ResolveError::BadUser(_))));
    }
```

- [ ] **Step 2: Run → FAIL**

Run: `cargo test -p llm-chat --no-default-features parse_stamp_handles_missing_and_malformed`
Expected: FAIL — undefined.

- [ ] **Step 3: Implement the helpers**

Add to `worker/src/user_env.rs` (outside the test module, e.g. after `provision_entries`):

```rust
/// The version-stamp marker, relative to the app folder.
const STAMP_REL: &str = ".llm-chat/version";

/// What `provision-app-box` should do given the box's recorded version and the
/// template's current version.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProvisionAction { Provision, Current, Migrate }

/// PURE. `stamp` is the box's recorded version (0 = fresh/unprovable); `target`
/// is the template's current version.
pub fn decide_action(stamp: i64, target: i64) -> ProvisionAction {
    if stamp <= 0 { ProvisionAction::Provision }
    else if stamp >= target { ProvisionAction::Current }
    else { ProvisionAction::Migrate }
}

/// PURE: parse a stamp file's content. Missing/empty/non-integer → 0 (fail to
/// the safe full-provision path).
pub fn parse_stamp(raw: Option<&str>) -> i64 {
    raw.map(str::trim).and_then(|s| s.parse::<i64>().ok()).unwrap_or(0)
}

/// Read `{base}/{user_id}/{app}/.llm-chat/version`. Confinement errors REJECT
/// (Err); an absent or unreadable/malformed marker → Ok(0) (treat as fresh).
pub fn read_stamp(base: &Path, user_id: &str, app: &str) -> Result<i64, ResolveError> {
    let rel = format!("{app}/{STAMP_REL}");
    let full = confine_path(base, user_id, Some(&rel))?;
    match std::fs::read_to_string(&full) {
        Ok(s) => Ok(parse_stamp(Some(&s))),
        Err(_) => Ok(0),
    }
}

/// Write `{base}/{user_id}/{app}/.llm-chat/version`. Confined; creates the
/// `.llm-chat` parent. The worker owns this file (claude never writes it).
pub fn write_stamp(base: &Path, user_id: &str, app: &str, version: i64) -> Result<(), ResolveError> {
    let rel = format!("{app}/{STAMP_REL}");
    let full = confine_path(base, user_id, Some(&rel))?;
    if let Some(parent) = full.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| ResolveError::Io(format!("create {}: {e}", parent.display())))?;
    }
    std::fs::write(&full, format!("{version}\n").as_bytes())
        .map_err(|e| ResolveError::Io(format!("write {}: {e}", full.display())))
}
```

- [ ] **Step 4: Run → PASS**

Run: `cargo test -p llm-chat --no-default-features user_env`
Expected: PASS (all stamp + decide tests green).

- [ ] **Step 5: Commit**

```bash
git add worker/src/user_env.rs
git commit -m "feat(worker): per-box version stamp helpers + decide_action"
```

---

### Task 5: Worker — migration runner module (`worker/src/migrate.rs`)

**Files:**
- Create: `worker/src/migrate.rs`
- Modify: `worker/src/lib.rs` — add `mod migrate;` near the other `mod` declarations; change `fn ensure_claude_trusts` (1297) to `pub(crate) fn ensure_claude_trusts`.

**Interfaces:**
- Consumes: `crate::user_env::SeedEntry`, `crate::ensure_claude_trusts`.
- Produces:
  - `pub fn migration_prompt(instructions: &str, manifest: &str) -> String`
  - `pub fn render_manifest(entries: &[crate::user_env::SeedEntry]) -> String`
  - `pub fn migration_result_ok(line: &str) -> Option<bool>`
  - `pub fn run_box_migration(claude_path: &str, cwd: &std::path::Path, prompt: &str, timeout: std::time::Duration) -> Result<(), String>`

- [ ] **Step 1: Create the module with the pure helpers + tests**

Create `worker/src/migrate.rs`:

```rust
//! One-shot, confined LLM sandbox migration (Sub-project 2). Drives `claude`
//! in stream-json mode (source of truth — never scrapes a TTY) with cwd locked
//! to a single user's `{userId}/{app}/` folder. Pure helpers are unit-tested;
//! the spawn is exercised by the live e2e.

use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::Duration;

/// PURE: render the desired template as an LLM-readable manifest (target
/// layout). Files show their content; directories are marked.
pub fn render_manifest(entries: &[crate::user_env::SeedEntry]) -> String {
    let mut out = String::new();
    for e in entries {
        if e.dir {
            out.push_str(&format!("- DIR  {}\n", e.path));
        } else {
            out.push_str(&format!("- FILE {}\n  ----- desired content -----\n", e.path));
            for line in e.content.lines() {
                out.push_str(&format!("  {line}\n"));
            }
            out.push_str("  ----- end -----\n");
        }
    }
    if out.is_empty() { out.push_str("(empty template)\n"); }
    out
}

/// PURE: build the migration prompt. The instructions are operator-authored;
/// the manifest is the desired (already variable-substituted) layout.
pub fn migration_prompt(instructions: &str, manifest: &str) -> String {
    format!(
        "You are migrating the current working directory (a user's app sandbox) \
to a new template version. Apply ONLY the migration described below. Do not \
touch hidden files under .llm-chat/. Be idempotent: if the change is already \
applied, make no edits.\n\n\
=== MIGRATION INSTRUCTIONS ===\n{instructions}\n\n\
=== DESIRED TEMPLATE (target layout) ===\n{manifest}\n\
=== END ===\n\nPerform the migration now."
    )
}

/// PURE: classify a single claude stream-json stdout line. Some(true) on a
/// successful `result` event, Some(false) on a failed one, None otherwise.
/// Mirrors the JsonSession reader's success rule.
pub fn migration_result_ok(line: &str) -> Option<bool> {
    let v: serde_json::Value = serde_json::from_str(line).ok()?;
    if v.get("type").and_then(|x| x.as_str()) != Some("result") {
        return None;
    }
    let ok = v.get("subtype").and_then(|x| x.as_str()) == Some("success")
        || v.get("is_error").and_then(|x| x.as_bool()) == Some(false);
    Some(ok)
}

/// Run a one-shot claude migration in `cwd` (BLOCKING). Returns Ok(()) only on a
/// successful `result` event. Kills the child after `timeout`. Fail closed:
/// any spawn/read/timeout/failed-result → Err.
pub fn run_box_migration(
    claude_path: &str,
    cwd: &Path,
    prompt: &str,
    timeout: Duration,
) -> Result<(), String> {
    // Pre-trust the cwd so claude's TUI trust dialog never blocks the run.
    let cwd_str = cwd.to_string_lossy().to_string();
    let _ = crate::ensure_claude_trusts(&cwd_str);

    let args = [
        "-p",
        "--input-format", "stream-json",
        "--output-format", "stream-json",
        "--verbose",
        "--dangerously-skip-permissions",
    ];
    let lower = claude_path.to_ascii_lowercase();
    let mut cmd = if cfg!(windows) && (lower.ends_with(".cmd") || lower.ends_with(".bat")) {
        let mut c = Command::new("cmd.exe");
        c.arg("/c").arg(claude_path).args(args);
        c
    } else {
        let mut c = Command::new(claude_path);
        c.args(args);
        c
    };
    cmd.current_dir(cwd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW
    }
    let mut child = cmd.spawn().map_err(|e| format!("spawn claude (migrate): {e}"))?;

    // One stream-json user message, then close stdin so claude finishes the turn.
    {
        let mut stdin = child.stdin.take().ok_or("no child stdin")?;
        let msg = serde_json::json!({
            "type": "user",
            "message": {"role": "user", "content": [{"type": "text", "text": prompt}]}
        });
        let mut line = msg.to_string();
        line.push('\n');
        stdin.write_all(line.as_bytes()).map_err(|e| format!("write stdin: {e}"))?;
        stdin.flush().map_err(|e| format!("flush stdin: {e}"))?;
        // stdin dropped here → EOF.
    }

    // Watchdog: kill the child if it overruns the timeout.
    let killer = child.id();
    let kill_flag = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    {
        let kf = kill_flag.clone();
        std::thread::spawn(move || {
            std::thread::sleep(timeout);
            if !kf.load(std::sync::atomic::Ordering::SeqCst) {
                // Best-effort kill by pid (platform tools); the wait() below then returns.
                #[cfg(windows)]
                let _ = Command::new("taskkill").args(["/PID", &killer.to_string(), "/T", "/F"]).output();
                #[cfg(unix)]
                let _ = Command::new("kill").args(["-9", &killer.to_string()]).output();
            }
        });
    }

    let stdout = child.stdout.take().ok_or("no child stdout")?;
    let reader = BufReader::new(stdout);
    let mut result: Option<bool> = None;
    for line in reader.lines() {
        let line = match line { Ok(l) => l, Err(_) => break };
        if line.trim().is_empty() { continue; }
        if let Some(ok) = migration_result_ok(&line) { result = Some(ok); }
    }
    kill_flag.store(true, std::sync::atomic::Ordering::SeqCst);
    let _ = child.wait();
    match result {
        Some(true) => Ok(()),
        Some(false) => Err("claude reported a failed result".into()),
        None => Err("claude produced no result event (killed or crashed)".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::user_env::SeedEntry;

    #[test]
    fn manifest_lists_files_and_dirs() {
        let entries = vec![
            SeedEntry { path: "README.md".into(), dir: false, content: "# Hi\nLine 2".into() },
            SeedEntry { path: "notes".into(), dir: true, content: String::new() },
        ];
        let m = render_manifest(&entries);
        assert!(m.contains("FILE README.md"));
        assert!(m.contains("# Hi"));
        assert!(m.contains("DIR  notes"));
    }

    #[test]
    fn manifest_empty_template() {
        assert!(render_manifest(&[]).contains("empty template"));
    }

    #[test]
    fn prompt_embeds_instructions_and_manifest() {
        let p = migration_prompt("rename x to y", "- FILE y\n");
        assert!(p.contains("rename x to y"));
        assert!(p.contains("- FILE y"));
        assert!(p.contains(".llm-chat/")); // protects the stamp dir
    }

    #[test]
    fn result_ok_classifies_lines() {
        assert_eq!(migration_result_ok(r#"{"type":"result","subtype":"success"}"#), Some(true));
        assert_eq!(migration_result_ok(r#"{"type":"result","is_error":false}"#), Some(true));
        assert_eq!(migration_result_ok(r#"{"type":"result","subtype":"error_max_turns","is_error":true}"#), Some(false));
        assert_eq!(migration_result_ok(r#"{"type":"assistant"}"#), None);
        assert_eq!(migration_result_ok("not json"), None);
    }
}
```

- [ ] **Step 2: Register the module + export `ensure_claude_trusts`**

In `worker/src/lib.rs`: add `mod migrate;` beside the other top-level `mod` declarations, and change `fn ensure_claude_trusts(cwd: &str)` (1297) to `pub(crate) fn ensure_claude_trusts(cwd: &str)`.

- [ ] **Step 3: Run → PASS**

Run: `cargo test -p llm-chat --no-default-features migrate`
Expected: PASS (4 pure tests).

- [ ] **Step 4: Commit**

```bash
git add worker/src/migrate.rs worker/src/lib.rs
git commit -m "feat(worker): confined one-shot claude migration runner + pure helpers"
```

---

### Task 6: Worker — extend `provision-app-box` (decision + stamp + background migration)

**Files:**
- Modify: `worker/src/lib.rs` — the `"provision-app-box"` control arm (2712-2736); add a module-level in-flight guard near `USER_ENV_BASE` (12).

**Interfaces:**
- Consumes: `crate::user_env::{provision_entries, read_stamp, write_stamp, decide_action, ProvisionAction, SeedEntry}`, `crate::migrate::{run_box_migration, render_manifest, migration_prompt}`, `crate::user_env::resolve_user_cwd`, `find_claude_path` (1257).

- [ ] **Step 1: Add the in-flight guard**

Near the top of `worker/src/lib.rs` (after `static USER_ENV_BASE` at line 12), add:

```rust
use std::collections::HashSet;
static MIGRATIONS_IN_FLIGHT: std::sync::OnceLock<std::sync::Mutex<HashSet<String>>> =
    std::sync::OnceLock::new();
fn migrations_in_flight() -> &'static std::sync::Mutex<HashSet<String>> {
    MIGRATIONS_IN_FLIGHT.get_or_init(|| std::sync::Mutex::new(HashSet::new()))
}
```

(If `HashSet`/`Mutex` are already imported at the top, drop the redundant `use`.)

- [ ] **Step 2: Replace the `provision-app-box` arm body**

Replace lines 2712-2736 (the whole `"provision-app-box" => { … }` arm) with:

```rust
                            "provision-app-box" => {
                                // Materialize/reconcile an app's sandbox to the
                                // template version. userId + app validated; files
                                // written only-if-absent; migration (if older)
                                // runs claude confined to {userId}/{app}/ in the
                                // background. Fail closed; never blocks login.
                                let user_id = req.get("userId").and_then(|v| v.as_str()).unwrap_or("").to_string();
                                let app = req.get("app").and_then(|v| v.as_str()).unwrap_or("").to_string();
                                let version = req.get("version").and_then(|v| v.as_i64()).unwrap_or(0);
                                let instructions = req.get("migrateInstructions").and_then(|v| v.as_str()).map(|s| s.to_string());
                                let entries: Vec<crate::user_env::SeedEntry> = req
                                    .get("files")
                                    .and_then(|v| v.as_array())
                                    .map(|arr| arr.iter().map(|f| crate::user_env::SeedEntry {
                                        path: f.get("path").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                                        dir: f.get("dir").and_then(|v| v.as_bool()).unwrap_or(false),
                                        content: f.get("content").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                                    }).collect())
                                    .unwrap_or_default();
                                let base = USER_ENV_BASE.get().expect("validated at startup");
                                tracing::info!(target: "backend::provision", user_id = %user_id, app = %app, version, n = entries.len(), "provision-app-box received");

                                // Always create any new files write-if-absent first.
                                match crate::user_env::provision_entries(base, &user_id, &app, &entries) {
                                    Err(e) => {
                                        tracing::warn!(target: "backend::provision", error = %e, "provision REJECTED (fail closed)");
                                        serde_json::json!({"ok": false, "error": format!("env: {e}")})
                                    }
                                    Ok(created) => {
                                        let stamp = crate::user_env::read_stamp(base, &user_id, &app).unwrap_or(0);
                                        match crate::user_env::decide_action(stamp, version) {
                                            crate::user_env::ProvisionAction::Provision => {
                                                match crate::user_env::write_stamp(base, &user_id, &app, version.max(1)) {
                                                    Ok(()) => serde_json::json!({"ok": true, "action": "provisioned", "version": version, "created": created}),
                                                    Err(e) => serde_json::json!({"ok": false, "error": format!("stamp: {e}")}),
                                                }
                                            }
                                            crate::user_env::ProvisionAction::Current => {
                                                serde_json::json!({"ok": true, "action": "current", "version": version, "created": created})
                                            }
                                            crate::user_env::ProvisionAction::Migrate => {
                                                let key = format!("{user_id}/{app}");
                                                let started = {
                                                    let mut g = migrations_in_flight().lock().unwrap();
                                                    if g.contains(&key) { false } else { g.insert(key.clone()); true }
                                                };
                                                if !started {
                                                    serde_json::json!({"ok": true, "action": "migrating", "version": version, "note": "already in flight"})
                                                } else if let Some(instr) = instructions.clone() {
                                                    // Resolve the confined app cwd for claude.
                                                    match crate::user_env::resolve_user_cwd(base, &user_id, Some(&app)) {
                                                        Err(e) => {
                                                            migrations_in_flight().lock().unwrap().remove(&key);
                                                            serde_json::json!({"ok": false, "error": format!("cwd: {e}")})
                                                        }
                                                        Ok(cwd) => match find_claude_path() {
                                                            None => {
                                                                migrations_in_flight().lock().unwrap().remove(&key);
                                                                tracing::warn!(target: "backend::provision", "migration skipped: claude not found");
                                                                serde_json::json!({"ok": true, "action": "migrate-unavailable", "version": version})
                                                            }
                                                            Some(claude_path) => {
                                                                let base_owned = base.clone();
                                                                let (uid2, app2, key2) = (user_id.clone(), app.clone(), key.clone());
                                                                let manifest = crate::migrate::render_manifest(&entries);
                                                                let prompt = crate::migrate::migration_prompt(&instr, &manifest);
                                                                tokio::task::spawn_blocking(move || {
                                                                    let res = crate::migrate::run_box_migration(
                                                                        &claude_path, &cwd, &prompt,
                                                                        std::time::Duration::from_secs(600),
                                                                    );
                                                                    match res {
                                                                        Ok(()) => {
                                                                            if let Err(e) = crate::user_env::write_stamp(&base_owned, &uid2, &app2, version) {
                                                                                tracing::warn!(target: "backend::provision", error = %e, "migration ok but stamp write failed");
                                                                            } else {
                                                                                tracing::info!(target: "backend::provision", user_id = %uid2, app = %app2, version, "migration complete; stamp bumped");
                                                                            }
                                                                        }
                                                                        Err(e) => tracing::warn!(target: "backend::provision", error = %e, user_id = %uid2, app = %app2, "migration FAILED; stamp left unchanged (retry next login)"),
                                                                    }
                                                                    migrations_in_flight().lock().unwrap().remove(&key2);
                                                                });
                                                                serde_json::json!({"ok": true, "action": "migrating", "version": version})
                                                            }
                                                        },
                                                    }
                                                } else {
                                                    // Older stamp but no instructions supplied — cannot migrate; leave as-is.
                                                    migrations_in_flight().lock().unwrap().remove(&key);
                                                    serde_json::json!({"ok": true, "action": "migrate-skipped", "version": version, "note": "no instructions"})
                                                }
                                            }
                                        }
                                    }
                                }
                            }
```

- [ ] **Step 3: Build the worker**

Run: `cargo test -p llm-chat --no-default-features provision_entries`
Expected: PASS (the existing provision tests still green; the crate compiles with the new arm). If the worker binary is locked on Windows, stop the running process first (`Stop-Process`) — that is an environment step, not a code change.

- [ ] **Step 4: Commit**

```bash
git add worker/src/lib.rs
git commit -m "feat(worker): provision-app-box reconciles by version (provision/current/migrate)"
```

---

### Task 7: admin-api — app-code registry from `app_codes.json`

**Files:**
- Modify: `admin-api/src/config.rs` — add `AppCodeEntry`, `parse_app_codes`, a `client_to_app` map builder + tests.
- Modify: `admin-api/src/lib.rs` — add `pub app_codes: std::sync::Arc<Vec<config::AppCodeEntry>>` to `AppState`.
- Modify: `admin-api/src/main.rs` — load the file at startup (fail-fast) into `AppState`.
- Modify: `admin-api/src/api/mod.rs` test helper at 861 (constructs `AppState`) to include the new field.

**Interfaces:**
- Produces:
  - `#[derive(Clone, Debug, PartialEq, Eq)] pub struct AppCodeEntry { pub app_code: String, pub name: String, pub client_id: String, pub project_id: String }`
  - `pub fn parse_app_codes(json: &str) -> Result<Vec<AppCodeEntry>, String>`
  - `pub fn app_code_for_client<'a>(entries: &'a [AppCodeEntry], client_id: &str) -> Option<&'a AppCodeEntry>`

- [ ] **Step 1: Write failing tests** (in `admin-api/src/config.rs` test module)

```rust
    #[test]
    fn parse_app_codes_reads_map() {
        let json = r#"{
          "kabytech": {"name":"kabytech-gateway","clientId":"111","projectId":"222"}
        }"#;
        let v = parse_app_codes(json).expect("ok");
        assert_eq!(v.len(), 1);
        assert_eq!(v[0], AppCodeEntry {
            app_code: "kabytech".into(), name: "kabytech-gateway".into(),
            client_id: "111".into(), project_id: "222".into(),
        });
        assert_eq!(app_code_for_client(&v, "111").unwrap().app_code, "kabytech");
        assert!(app_code_for_client(&v, "999").is_none());
    }

    #[test]
    fn parse_app_codes_empty_object_ok() {
        assert_eq!(parse_app_codes("{}").expect("ok"), vec![]);
    }

    #[test]
    fn parse_app_codes_errors_on_malformed() {
        assert!(parse_app_codes("not json").is_err());
    }

    #[test]
    fn parse_app_codes_drops_entries_missing_fields() {
        // an entry with no clientId is dropped (never defaulted).
        let json = r#"{"ok":{"name":"n","clientId":"1","projectId":"2"},"bad":{"name":"n","projectId":"2"}}"#;
        let v = parse_app_codes(json).expect("ok");
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].app_code, "ok");
    }
```

- [ ] **Step 2: Run → FAIL**

Run: `cargo test -p llm-chat-admin-api parse_app_codes_reads_map`

- [ ] **Step 3: Implement** (in `admin-api/src/config.rs`)

```rust
/// One app-code → OIDC-client/project mapping (from secrets/app_codes.json,
/// written by the provisioner). Ties a sandbox template (keyed by app_code) to
/// the login client (clientId) operators see on the Application page.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AppCodeEntry {
    pub app_code: String,
    pub name: String,
    pub client_id: String,
    pub project_id: String,
}

/// PURE: parse app_codes.json ({ "<code>": {name, clientId, projectId} }). An
/// entry missing any field is dropped (never defaulted). Malformed JSON errors.
pub fn parse_app_codes(json: &str) -> Result<Vec<AppCodeEntry>, String> {
    #[derive(serde::Deserialize)]
    struct Raw { name: Option<String>, #[serde(rename = "clientId")] client_id: Option<String>, #[serde(rename = "projectId")] project_id: Option<String> }
    let map: std::collections::BTreeMap<String, Raw> =
        serde_json::from_str(json).map_err(|e| format!("app_codes.json is not valid JSON: {e}"))?;
    let nonempty = |s: Option<String>| s.map(|x| x.trim().to_string()).filter(|x| !x.is_empty());
    Ok(map.into_iter().filter_map(|(code, r)| {
        Some(AppCodeEntry {
            app_code: nonempty(Some(code))?,
            name: nonempty(r.name)?,
            client_id: nonempty(r.client_id)?,
            project_id: nonempty(r.project_id)?,
        })
    }).collect())
}

/// Find the entry whose OIDC clientId matches.
pub fn app_code_for_client<'a>(entries: &'a [AppCodeEntry], client_id: &str) -> Option<&'a AppCodeEntry> {
    entries.iter().find(|e| e.client_id == client_id)
}
```

- [ ] **Step 4: Run → PASS**

Run: `cargo test -p llm-chat-admin-api parse_app_codes`

- [ ] **Step 5: Wire into `AppState` + startup load**

In `admin-api/src/lib.rs`, add to `AppState`:

```rust
    pub app_codes: std::sync::Arc<Vec<config::AppCodeEntry>>,
```

In `admin-api/src/main.rs`, before constructing `state` (110), load the file fail-fast:

```rust
    // App-code registry (sandbox templates). Optional feature: absent path →
    // empty registry (editor simply hidden). Set-but-unreadable/malformed →
    // fail fast (no silent default — Global Constraints / fail-closed).
    let app_codes = match std::env::var("ADMIN_APP_CODES_PATH").ok().filter(|s| !s.trim().is_empty()) {
        None => Vec::new(),
        Some(path) => {
            let raw = std::fs::read_to_string(&path)
                .map_err(|e| format!("ADMIN_APP_CODES_PATH {path}: {e}"))?;
            crate::config::parse_app_codes(&raw)?
        }
    };
```

(Adapt the `?`/error type to `main`'s return — `main.rs` already returns a `Result`; match its error type, e.g. `.map_err(|e| anyhow::anyhow!(e))?` or the existing pattern. Inspect the top of `main` to use the same error path — do NOT introduce a new dependency.)

Then add `app_codes: std::sync::Arc::new(app_codes),` to the `AppState { … }` literal (110).

In `admin-api/src/api/mod.rs` at the test-helper `AppState` construction (861), add `app_codes: std::sync::Arc::new(vec![]),`.

- [ ] **Step 6: Build + test**

Run: `cargo test -p llm-chat-admin-api`
Expected: PASS (registry tests green; crate compiles).

- [ ] **Step 7: Commit**

```bash
git add admin-api/src/config.rs admin-api/src/lib.rs admin-api/src/main.rs admin-api/src/api/mod.rs
git commit -m "feat(admin-api): app-code registry from app_codes.json (clientId -> app_code)"
```

---

### Task 8: admin-api — annotate `list_project_apps` with `appCode`

**Files:**
- Modify: `admin-api/src/api/mod.rs` — `list_project_apps` (610-613); add a pure `annotate_apps` helper + test.

**Interfaces:**
- Consumes: `config::AppCodeEntry`, `config::app_code_for_client`.
- Produces: `fn annotate_apps(apps: Vec<Value>, entries: &[crate::config::AppCodeEntry]) -> Vec<Value>`

- [ ] **Step 1: Write failing test** (in the `admin-api/src/api/mod.rs` test module, near 921)

```rust
    #[test]
    fn annotate_apps_sets_app_code_by_client_id() {
        use crate::config::AppCodeEntry;
        let entries = vec![AppCodeEntry { app_code: "kabytech".into(), name: "kabytech-gateway".into(), client_id: "111".into(), project_id: "222".into() }];
        let apps = vec![
            serde_json::json!({"id":"a1","name":"Gateway","oidcConfig":{"clientId":"111"}}),
            serde_json::json!({"id":"a2","name":"Other","oidcConfig":{"clientId":"999"}}),
            serde_json::json!({"id":"a3","name":"NoOidc"}),
        ];
        let out = super::annotate_apps(apps, &entries);
        assert_eq!(out[0]["appCode"], "kabytech");
        assert!(out[1]["appCode"].is_null());
        assert!(out[2]["appCode"].is_null());
    }
```

- [ ] **Step 2: Run → FAIL**

Run: `cargo test -p llm-chat-admin-api annotate_apps_sets_app_code_by_client_id`

- [ ] **Step 3: Implement** (in `admin-api/src/api/mod.rs`)

```rust
/// PURE: tag each app with `appCode` (resolved from its oidcConfig.clientId via
/// the registry) or JSON null. The client uses this only to show/hide the
/// editor — the mapping itself stays server-side (Global Constraints).
fn annotate_apps(apps: Vec<Value>, entries: &[crate::config::AppCodeEntry]) -> Vec<Value> {
    apps.into_iter().map(|mut a| {
        let code = a.get("oidcConfig").and_then(|o| o.get("clientId")).and_then(|c| c.as_str())
            .and_then(|cid| crate::config::app_code_for_client(entries, cid))
            .map(|e| e.app_code.clone());
        a["appCode"] = match code { Some(c) => Value::String(c), None => Value::Null };
        a
    }).collect()
}
```

Update `list_project_apps`:

```rust
async fn list_project_apps(_op: Operator, State(st): State<AppState>, Path(pid): Path<String>)
    -> Result<Json<Value>, ApiError> {
    let apps = st.zitadel.list_apps_for(&pid).await?;
    let arr = apps.as_array().cloned().unwrap_or_default();
    Ok(Json(json!({ "result": annotate_apps(arr, &st.app_codes) })))
}
```

(`list_apps_for` returns a `Value`; if it returns `Vec<Value>` already, drop the `as_array` step and pass it directly. Inspect the return type and adapt.)

- [ ] **Step 4: Run → PASS**

Run: `cargo test -p llm-chat-admin-api annotate_apps`

- [ ] **Step 5: Commit**

```bash
git add admin-api/src/api/mod.rs
git commit -m "feat(admin-api): annotate project apps with appCode for the template editor"
```

---

### Task 9: admin-api — GET/PUT sandbox-template endpoints

**Files:**
- Modify: `admin-api/src/api/mod.rs` — add two routes (in `routes()`, near 62-63), two handlers, an `extract_client_id` pure helper + test.

**Interfaces:**
- Consumes: `st.zitadel.get_app_in(pid, app_id)`, `config::app_code_for_client`, `config::default_app`, `st.zitadel.mint_chat_token`, `crate::manager::control_request`.
- Produces: `fn extract_client_id(app: &Value) -> Option<String>`

- [ ] **Step 1: Write failing test for `extract_client_id`**

```rust
    #[test]
    fn extract_client_id_handles_both_shapes() {
        let wrapped = serde_json::json!({"app":{"oidcConfig":{"clientId":"111"}}});
        let bare = serde_json::json!({"oidcConfig":{"clientId":"222"}});
        assert_eq!(super::extract_client_id(&wrapped).as_deref(), Some("111"));
        assert_eq!(super::extract_client_id(&bare).as_deref(), Some("222"));
        assert!(super::extract_client_id(&serde_json::json!({"x":1})).is_none());
    }
```

- [ ] **Step 2: Run → FAIL**

Run: `cargo test -p llm-chat-admin-api extract_client_id_handles_both_shapes`

- [ ] **Step 3: Implement the helper + handlers + routes**

Helper:

```rust
/// PURE: pull oidcConfig.clientId from a Zitadel app payload, tolerating the
/// `{app:{…}}` wrapper the GET endpoint returns and the bare object the list
/// returns.
fn extract_client_id(app: &Value) -> Option<String> {
    let oidc = app.get("app").and_then(|a| a.get("oidcConfig"))
        .or_else(|| app.get("oidcConfig"))?;
    oidc.get("clientId").and_then(|c| c.as_str()).map(|s| s.to_string())
}
```

Shared resolver (handler-local): given `(pid, app_id)`, fetch the app, extract clientId, map to app_code, 404 if unmapped/unconfigured:

```rust
async fn resolve_app_code(st: &AppState, pid: &str, app_id: &str) -> Result<String, ApiError> {
    let app = st.zitadel.get_app_in(pid, app_id).await?;
    let client_id = extract_client_id(&app)
        .ok_or_else(|| ApiError::NotFound("app has no OIDC clientId".into()))?;
    crate::config::app_code_for_client(&st.app_codes, &client_id)
        .map(|e| e.app_code.clone())
        .ok_or_else(|| ApiError::NotFound("no sandbox template configured for this client".into()))
}
```

(Use whatever `ApiError` variant exists for 404 — inspect `ApiError`; if there is no `NotFound`, use the existing not-found/bad-request variant consistently. Do NOT invent a variant; match the enum.)

GET handler:

```rust
async fn get_sandbox_template(_op: Operator, State(st): State<AppState>, Path((pid, app_id)): Path<(String, String)>)
    -> Result<Json<Value>, ApiError> {
    let Some(app) = crate::config::default_app(&st.cfg.session_apps) else {
        return Ok(Json(json!({ "configured": false })));
    };
    let app_code = resolve_app_code(&st, &pid, &app_id).await?;
    let token = st.zitadel.mint_chat_token(&app.project_id).await?;
    let reply = crate::manager::control_request(&app.control_url, &token, json!({ "cmd": "get-template", "appCode": app_code }))
        .await
        .unwrap_or_else(|e| json!({ "ok": false, "error": e }));
    Ok(Json(json!({
        "configured": true,
        "ok": reply.get("ok").and_then(Value::as_bool).unwrap_or(false),
        "appCode": app_code,
        "version": reply.get("version").cloned().unwrap_or(json!(0)),
        "template": reply.get("template").cloned().unwrap_or_else(|| json!([])),
        "migrateInstructions": reply.get("migrateInstructions").cloned().unwrap_or(Value::Null),
        "updatedAt": reply.get("updatedAt").cloned().unwrap_or(Value::Null),
        "error": reply.get("error").cloned(),
    })))
}
```

PUT handler + body type:

```rust
#[derive(Deserialize)]
struct SaveTemplateBody { template: Value, #[serde(default)] publish: bool, #[serde(rename = "migrateInstructions")] migrate_instructions: Option<String> }

async fn put_sandbox_template(_op: Operator, State(st): State<AppState>, Path((pid, app_id)): Path<(String, String)>, Json(b): Json<SaveTemplateBody>)
    -> Result<Json<Value>, ApiError> {
    let Some(app) = crate::config::default_app(&st.cfg.session_apps) else {
        return Err(ApiError::BadRequest("chat backend not configured".into()));
    };
    let app_code = resolve_app_code(&st, &pid, &app_id).await?;
    let token = st.zitadel.mint_chat_token(&app.project_id).await?;
    let reply = crate::manager::control_request(&app.control_url, &token, json!({
        "cmd": "set-template", "appCode": app_code, "template": b.template,
        "publish": b.publish, "migrateInstructions": b.migrate_instructions,
    })).await.unwrap_or_else(|e| json!({ "ok": false, "error": e }));
    Ok(Json(json!({
        "ok": reply.get("ok").and_then(Value::as_bool).unwrap_or(false),
        "version": reply.get("version").cloned().unwrap_or(json!(0)),
        "updatedAt": reply.get("updatedAt").cloned().unwrap_or(Value::Null),
        "error": reply.get("error").cloned(),
    })))
}
```

Routes (add after line 63, the project-app routes):

```rust
        .route("/api/projects/{pid}/apps/{appId}/sandbox-template", get(get_sandbox_template).put(put_sandbox_template))
```

- [ ] **Step 4: Build + test**

Run: `cargo test -p llm-chat-admin-api`
Expected: PASS (extract_client_id green; crate compiles).

- [ ] **Step 5: Commit**

```bash
git add admin-api/src/api/mod.rs
git commit -m "feat(admin-api): GET/PUT sandbox-template endpoints relayed to the manager"
```

---

### Task 10: admin-web — types, API methods, template-tree utils

**Files:**
- Modify: `admin-web/lib/types.ts` — add `TemplateEntry`, `SandboxTemplate`, `SaveTemplateInput`; add `appCode?: string | null` to `OidcApp` (137-142).
- Modify: `admin-web/lib/api.ts` — add `getSandboxTemplate`/`saveSandboxTemplate`.
- Create: `admin-web/lib/sandbox-template.ts` — pure tree utils.
- Create: `admin-web/lib/sandbox-template.test.ts` — vitest.

**Interfaces:**
- Produces (in `lib/sandbox-template.ts`):
  - `export interface TemplateEntry { path: string; dir: boolean; content: string }`
  - `export interface TplNode { name: string; path: string; dir: boolean; content: string; children: TplNode[] }`
  - `export function entriesToTree(entries: TemplateEntry[]): TplNode[]`
  - `export function treeToEntries(roots: TplNode[]): TemplateEntry[]`
  - `export function isValidPath(path: string): boolean`

- [ ] **Step 1: Write failing tests** (`admin-web/lib/sandbox-template.test.ts`)

```ts
import { describe, it, expect } from "vitest";
import { entriesToTree, treeToEntries, isValidPath, type TemplateEntry } from "./sandbox-template";

describe("sandbox-template tree", () => {
  it("round-trips entries through the tree (files imply parent folders)", () => {
    const entries: TemplateEntry[] = [
      { path: "README.md", dir: false, content: "# hi" },
      { path: "notes/todo.md", dir: false, content: "do" },
      { path: "empty", dir: true, content: "" },
    ];
    const tree = entriesToTree(entries);
    const back = treeToEntries(tree);
    // README.md + notes/todo.md + empty/ survive; intermediate "notes" is NOT
    // emitted as a dir entry (it is implied by notes/todo.md).
    expect(back).toContainEqual({ path: "README.md", dir: false, content: "# hi" });
    expect(back).toContainEqual({ path: "notes/todo.md", dir: false, content: "do" });
    expect(back).toContainEqual({ path: "empty", dir: true, content: "" });
    expect(back.find((e) => e.path === "notes" && e.dir)).toBeUndefined();
  });

  it("validates paths the way the server confines them", () => {
    expect(isValidPath("a/b.txt")).toBe(true);
    expect(isValidPath("../x")).toBe(false);
    expect(isValidPath("/abs")).toBe(false);
    expect(isValidPath("a\\b")).toBe(false);
    expect(isValidPath("C:")).toBe(false);
    expect(isValidPath("a/./b")).toBe(false);
    expect(isValidPath("")).toBe(false);
  });
});
```

- [ ] **Step 2: Run → FAIL**

Run (from `admin-web/`): `npx vitest run lib/sandbox-template.test.ts`

- [ ] **Step 3: Implement `lib/sandbox-template.ts`**

```ts
export interface TemplateEntry {
  path: string;
  dir: boolean;
  content: string;
}

export interface TplNode {
  name: string;
  path: string;
  dir: boolean;
  content: string;
  children: TplNode[];
}

// Build a nested editable tree. Intermediate folders implied by a file path are
// materialized as structural dir nodes; an explicit dir entry maps to a dir
// node too. Folders sort before files, then alphabetical.
export function entriesToTree(entries: TemplateEntry[]): TplNode[] {
  const roots: TplNode[] = [];
  const byPath = new Map<string, TplNode>();
  const ensureDir = (path: string): TplNode => {
    const existing = byPath.get(path);
    if (existing) return existing;
    const segs = path.split("/");
    const node: TplNode = { name: segs[segs.length - 1], path, dir: true, content: "", children: [] };
    byPath.set(path, node);
    const parent = segs.length > 1 ? ensureDir(segs.slice(0, -1).join("/")) : undefined;
    if (parent) parent.children.push(node);
    else roots.push(node);
    return node;
  };
  const sorted = [...entries].sort((a, b) => a.path.localeCompare(b.path));
  for (const e of sorted) {
    const segs = e.path.split("/");
    if (e.dir) { ensureDir(e.path); continue; }
    const node: TplNode = { name: segs[segs.length - 1], path: e.path, dir: false, content: e.content, children: [] };
    byPath.set(e.path, node);
    const parent = segs.length > 1 ? ensureDir(segs.slice(0, -1).join("/")) : undefined;
    if (parent) parent.children.push(node);
    else roots.push(node);
  }
  sortLevel(roots);
  return roots;
}

// Flatten back to entries. Files always emit; a dir emits an explicit entry
// ONLY when it has no descendants (an empty folder) — intermediate folders are
// implied by their files (matches the worker's provision_entries).
export function treeToEntries(roots: TplNode[]): TemplateEntry[] {
  const out: TemplateEntry[] = [];
  const walk = (node: TplNode): void => {
    if (!node.dir) { out.push({ path: node.path, dir: false, content: node.content }); return; }
    if (node.children.length === 0) { out.push({ path: node.path, dir: true, content: "" }); return; }
    for (const c of node.children) walk(c);
  };
  for (const r of roots) walk(r);
  return out;
}

function sortLevel(nodes: TplNode[]): void {
  nodes.sort((a, b) => (a.dir === b.dir ? a.name.localeCompare(b.name) : a.dir ? -1 : 1));
  for (const n of nodes) sortLevel(n.children);
}

// Mirror the worker's confine_path rules (fast client feedback; the server is
// authoritative). Reject empty, absolute, traversal, '.', '\\', ':', NUL.
export function isValidPath(path: string): boolean {
  if (!path || path.startsWith("/")) return false;
  for (const seg of path.split("/")) {
    if (seg === "" || seg === "." || seg === "..") return false;
    if (seg.includes("\\") || seg.includes(":") || seg.includes("\0")) return false;
  }
  return true;
}
```

- [ ] **Step 4: Run → PASS**

Run (from `admin-web/`): `npx vitest run lib/sandbox-template.test.ts`

- [ ] **Step 5: Add types + API methods**

In `admin-web/lib/types.ts`, add `appCode?: string | null;` to the `OidcApp` interface, and add:

```ts
export interface SandboxTemplate {
  configured: boolean;
  ok: boolean;
  appCode: string;
  version: number;
  template: { path: string; dir: boolean; content: string }[];
  migrateInstructions: string | null;
  updatedAt: string | null;
  error?: string | null;
}

export interface SaveTemplateInput {
  template: { path: string; dir: boolean; content: string }[];
  publish: boolean;
  migrateInstructions?: string;
}

export interface SaveTemplateResult {
  ok: boolean;
  version: number;
  updatedAt: string | null;
  error?: string | null;
}
```

In `admin-web/lib/api.ts`, extend the `api` object:

```ts
  getSandboxTemplate: (pid: string, appId: string) =>
    request<import("./types").SandboxTemplate>(`/api/projects/${pid}/apps/${appId}/sandbox-template`, { method: "GET" }),
  saveSandboxTemplate: (pid: string, appId: string, body: import("./types").SaveTemplateInput) =>
    request<import("./types").SaveTemplateResult>(`/api/projects/${pid}/apps/${appId}/sandbox-template`, { method: "PUT", json: body }),
```

(Or add normal top-of-file imports for the types instead of inline `import(...)`, matching the file's existing import style — `lib/api.ts` currently has no imports, so either inline-import or add an `import type` line. Pick one consistently.)

- [ ] **Step 6: Typecheck + tests**

Run (from `admin-web/`): `npx vitest run lib/sandbox-template.test.ts && npx tsc --noEmit`
Expected: PASS, no type errors.

- [ ] **Step 7: Commit**

```bash
git add admin-web/lib/types.ts admin-web/lib/api.ts admin-web/lib/sandbox-template.ts admin-web/lib/sandbox-template.test.ts
git commit -m "feat(admin-web): sandbox-template types, API methods, tree utils"
```

---

### Task 11: admin-web — two-pane editor + wire into the client panel

**Files:**
- Create: `admin-web/components/applications/sandbox-template-editor.tsx`
- Modify: `admin-web/app/(dash)/applications/[id]/page.tsx` — render the editor in the client `DetailPanel` (184-222) when `selectedClient.appCode` is set.
- Add the shadcn `textarea` primitive: `npx shadcn@latest add textarea` (creates `components/ui/textarea.tsx`).
- Modify/extend: `admin-web/__tests__/application-detail-page.test.tsx` (or a new `components/applications/sandbox-template-editor.test.tsx`).

**Interfaces:**
- Consumes: `lib/sandbox-template` utils, `lib/api`, `lib/types`, shadcn `Dialog`, `Button`, `Input`, `Textarea`, `Label`, `Badge`.

- [ ] **Step 1: Add the textarea primitive**

Run (from `admin-web/`): `npx shadcn@latest add textarea`
Expected: `components/ui/textarea.tsx` created. (If the CLI prompts, accept defaults. This is a primitive, per AGENTS.md — do not hand-roll a `<textarea>`.)

- [ ] **Step 2: Write the failing component test** (`admin-web/components/applications/sandbox-template-editor.test.tsx`)

```tsx
import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { SandboxTemplateEditor } from "./sandbox-template-editor";
import { api } from "@/lib/api";

vi.mock("@/lib/api", () => ({
  api: { getSandboxTemplate: vi.fn(), saveSandboxTemplate: vi.fn() },
  ApiError: class extends Error {},
}));
vi.mock("sonner", () => ({ toast: { success: vi.fn(), error: vi.fn() } }));

describe("SandboxTemplateEditor", () => {
  beforeEach(() => {
    (api.getSandboxTemplate as any).mockResolvedValue({
      configured: true, ok: true, appCode: "kabytech", version: 2,
      template: [{ path: "README.md", dir: false, content: "# hi" }],
      migrateInstructions: null, updatedAt: "t",
    });
    (api.saveSandboxTemplate as any).mockResolvedValue({ ok: true, version: 2, updatedAt: "t2" });
  });

  it("loads and shows the version + a tree node", async () => {
    render(<SandboxTemplateEditor pid="p1" appId="a1" />);
    await waitFor(() => expect(screen.getByText(/v2/)).toBeInTheDocument());
    expect(screen.getByText("README.md")).toBeInTheDocument();
  });

  it("saves a content edit without publishing", async () => {
    render(<SandboxTemplateEditor pid="p1" appId="a1" />);
    await screen.findByText("README.md");
    await userEvent.click(screen.getByRole("button", { name: /save/i }));
    await waitFor(() => expect(api.saveSandboxTemplate).toHaveBeenCalledWith("p1", "a1",
      expect.objectContaining({ publish: false })));
  });
});
```

- [ ] **Step 3: Run → FAIL**

Run (from `admin-web/`): `npx vitest run components/applications/sandbox-template-editor.test.tsx`

- [ ] **Step 4: Implement the editor**

Create `admin-web/components/applications/sandbox-template-editor.tsx`. It must:
- on mount, `api.getSandboxTemplate(pid, appId)` → store `version`, build the tree with `entriesToTree`;
- render a two-pane layout: left = the tree (folders before files; click selects a node; new-file/new-folder/delete/rename controls), right = a `Textarea` bound to the selected file's `content` (disabled for dir nodes); show variable hints `{{name}} {{userId}} {{app}} {{date}}` and a `Badge` with `v{version}`;
- validate each node path with `isValidPath` (mark invalid, block save);
- **Save**: `treeToEntries` → `api.saveSandboxTemplate(pid, appId, { template, publish: false })`;
- **Publish new version**: open a `Dialog` requiring a non-empty migration-instructions `Textarea`; on confirm → `api.saveSandboxTemplate(pid, appId, { template, publish: true, migrateInstructions })`; disable confirm while empty;
- on success `toast.success` and refresh the version; on error `toast.error(e.message)`.

Build ONLY from shadcn primitives (Button, Input, Textarea, Label, Badge, Dialog) per AGENTS.md. The tree pane is an app-specific composition (a nested `<ul>` of buttons) — acceptable as composition, not a raw-HTML primitive. Signature:

```tsx
export function SandboxTemplateEditor({ pid, appId }: { pid: string; appId: string }) { /* ... */ }
```

Keep the file focused; if the tree rendering grows large, extract a `TemplateTree` subcomponent in the same file.

- [ ] **Step 5: Wire into the client panel**

In `admin-web/app/(dash)/applications/[id]/page.tsx`, inside the client `DetailPanel` (after the action buttons at 214-219, still within `{selectedClient && ( … )}`), add:

```tsx
              {selectedClient.appCode && (
                <PanelSection title="Sandbox template">
                  <SandboxTemplateEditor pid={id} appId={selectedClient.id} />
                </PanelSection>
              )}
```

Add the import at the top: `import { SandboxTemplateEditor } from "@/components/applications/sandbox-template-editor";`

- [ ] **Step 6: Run tests + typecheck**

Run (from `admin-web/`): `npx vitest run components/applications/sandbox-template-editor.test.tsx && npx tsc --noEmit`
Expected: PASS, no type errors. If the existing `__tests__/application-detail-page.test.tsx` mocks `api`, ensure it still passes (add the new methods to its mock if it asserts on the full api surface): `npx vitest run __tests__/application-detail-page.test.tsx`.

- [ ] **Step 7: Commit**

```bash
git add admin-web/components/applications/sandbox-template-editor.tsx admin-web/components/applications/sandbox-template-editor.test.tsx admin-web/components/ui/textarea.tsx "admin-web/app/(dash)/applications/[id]/page.tsx"
git commit -m "feat(admin-web): two-pane sandbox template editor on the OIDC client panel"
```

---

### Task 12: Wiring — env, compose verification, docs

**Files:**
- Modify: `.env.local.example` (admin-api section) and the live `.env.local`.
- Verify: `docker-compose.yml` admin-api `volumes` already mounts `./secrets:/secrets:ro` (it does — no change needed; confirm).
- Optional: `config.md` if a deployment gotcha surfaces.

- [ ] **Step 1: Add the env var to the template**

In `.env.local.example`, under `# ── admin-api ──`, after the `MANAGER_CONTROL_*` lines, add:

```
# Sandbox-template editor (optional). Path to the provisioner-written app-code
# map in the read-only /secrets mount. Maps each app's OIDC clientId -> app_code
# so the Console can author per-app sandbox templates. Absent -> editor hidden.
ADMIN_APP_CODES_PATH=/secrets/app_codes.json
```

- [ ] **Step 2: Mirror into the live `.env.local`**

Add the same `ADMIN_APP_CODES_PATH=/secrets/app_codes.json` line to the live `.env.local` (gitignored — not committed).

- [ ] **Step 3: Confirm the compose mount**

Verify `docker-compose.yml`'s `admin-api.volumes` contains `- ./secrets:/secrets:ro` (it does at the admin-api service). No change required. If it were missing, add it — but confirm first.

- [ ] **Step 4: Commit the template change**

```bash
git add .env.local.example
git commit -m "chore: ADMIN_APP_CODES_PATH for the sandbox-template editor"
```

---

## Self-Review

**Spec coverage:**
- Data model (version + migrate_instructions, ALTER) → Task 1. ✓
- TemplateRecord + get/upsert signatures → Task 1. ✓
- get-template/set-template, server-computed version, publish-requires-instructions → Task 2. ✓
- handle_provision passes version + instructions → Task 3. ✓
- Stamp helpers + decide_action (missing/malformed → v0) → Task 4. ✓
- LLM migration (stream-json, confined cwd, fail-closed, pure helpers) → Task 5. ✓
- provision-app-box provision/current/migrate + async background + in-flight guard + stamp-on-success → Task 6. ✓
- admin-api registry from app_codes.json (fail-fast) → Task 7. ✓
- list_project_apps annotation → Task 8. ✓
- GET/PUT endpoints, appId→clientId→appCode, Operator-gated → Task 9. ✓
- admin-web types/api/tree utils → Task 10. ✓
- two-pane editor + Publish dialog + wire-in → Task 11. ✓
- env/compose/docs → Task 12. ✓

**Placeholder scan:** Task 2 Step 5 deliberately flags illustrative names (`return_err_inline`/`return_value_on_err`) as NOT-to-be-used and gives the corrected code — the implementer uses the corrected `match db.get_template(...)` form. No `TODO`/`TBD` remain.

**Type consistency:** `TemplateRecord` fields (template_json/version/migrate_instructions/updated_at) used identically in Tasks 1–3. `decide_action`/`ProvisionAction` defined in Task 4, consumed in Task 6 via `crate::user_env::`. `AppCodeEntry`/`app_code_for_client` defined Task 7, consumed Tasks 8–9. `entriesToTree`/`treeToEntries`/`isValidPath`/`TemplateEntry`/`TplNode` defined Task 10, consumed Task 11. Endpoint shape `{version, template, migrateInstructions, updatedAt}` consistent across manager (Task 2), admin-api (Task 9), admin-web types (Task 10).

**Notes for the executor:**
- Subagents cannot run shell in this session — the controller runs all cargo/npx/git and commits; reviewers do read-only diff reviews.
- The worker binary may be locked on Windows; stop the running worker before `cargo build`/`cargo test` that links the worker bin.
- Adapt `ApiError` variant names and `list_apps_for`/`main` error-type details to the real enum/signatures (flagged inline) — do not invent variants or dependencies.
