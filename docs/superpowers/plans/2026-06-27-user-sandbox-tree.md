# User Sandbox Tree — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Show each user's confined `claude` sandbox (`{LLM_CHAT_USER_ENV_BASE}/{userId}`) as a folder+file tree in the Console Users detail panel.

**Architecture:** Reuse the worker's confined `list_box_tree`; add a non-creating `list_box_readonly`, expose it to operators through the existing `admin-api → manager /control → worker /control` proxy chain (a new `chat.admin`-gated `user-box` control command + a `GET /api/users/{id}/files` BFF endpoint), and render the flat `{path,dir,size}` entries as a collapsible tree in the Users panel.

**Tech Stack:** Rust (worker `llm-chat`, manager `llm-chat-manager`, admin-api `llm-chat-admin-api`); Next.js 16 / React 19 / TypeScript / vitest (admin-web). Spec: `docs/superpowers/specs/2026-06-27-user-sandbox-tree-design.md`.

## Global Constraints

- **Read-only / non-mutating admin view:** the new `list_box_readonly` must NOT create a box for a user who has none — it returns empty. The existing `list_box_tree` (create-on-open) is unchanged.
- **Worker `dir` `create` flag defaults `true`** so the existing `/chat` `/dir` self-view is byte-for-byte unchanged; the admin path sends `create:false`.
- **Confinement is fail-closed and unchanged:** mandatory user id, traversal rejected, symlinks listed but NEVER followed. Caps: `max_depth = 8`, `max_entries = 2000`; a `truncated` flag is surfaced end to end.
- **`chat.admin` gate twice:** the admin-api `Operator` extractor AND the manager `/control` surface (already gated). No new ungated surface.
- **`control_query(url, token, cmd)` must keep working** — add `control_request(url, token, req: Value)` and make `control_query` a thin wrapper.
- **Capability-gate** `/api/users/{id}/files` on `MANAGER_CONTROL_URL` exactly like `/api/chat-sessions` and `/api/usage` (unset → `{configured:false}`, never a 5xx).
- **shadcn-first (admin-web):** there is no shadcn "tree" primitive, so `<SandboxTree>` is a justified app-specific component built from minimal markup + `Button` toggles + lucide icons.
- **Cargo:** worker pkg is `llm-chat` — test it with `--no-default-features` (skips Tauri/GUI). Manager `llm-chat-manager`, admin-api `llm-chat-admin-api`. **admin-web:** `pnpm` from repo root with `-C admin-web`.
- **Commits:** Conventional Commits; end each body with `Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>`. Stage explicit paths only (shared branch — never `git add -A`).

---

## File Structure

**worker**
- `worker/src/user_env.rs` — *modify*: add `list_box_readonly` + tests.
- `worker/src/lib.rs` — *modify*: `dir` `/control` handler honors a `create` flag.

**manager**
- `manager/src/main.rs` — *modify*: add a `user-box` arm to `handle_control`.

**admin-api**
- `admin-api/src/manager.rs` — *modify*: add `control_request`; `control_query` becomes a wrapper.
- `admin-api/src/api/mod.rs` — *modify*: `GET /api/users/{id}/files` route + handler + gating test.

**admin-web**
- `admin-web/lib/types.ts` — *modify*: `SandboxEntry`, `SandboxFiles`.
- `admin-web/lib/sandbox-tree.ts` — *create*: `TreeNode`, `buildTree`.
- `admin-web/lib/sandbox-tree.test.ts` — *create*.
- `admin-web/components/users/sandbox-tree.tsx` — *create*: `<SandboxTree>`.
- `admin-web/components/users/sandbox-tree.test.tsx` — *create*.
- `admin-web/app/(dash)/users/page.tsx` — *modify*: Sandbox `PanelSection` + fetch.

---

### Task 1: Worker non-creating listing (`list_box_readonly`)

**Files:**
- Modify: `worker/src/user_env.rs`

**Interfaces:**
- Consumes: existing `valid_user_id`, `walk_box`, `DirEntry`, `ResolveError`.
- Produces: `pub fn list_box_readonly(base: &Path, user_id: Option<&str>, max_depth: usize, max_entries: usize) -> Result<(Vec<DirEntry>, bool), ResolveError>` — like `list_box_tree` but returns `(vec![], false)` for an absent box WITHOUT creating it.

- [ ] **Step 1: Write the failing tests.** Add to the `#[cfg(test)] mod tests` block in `worker/src/user_env.rs` (after the existing `list_box_tree_*` tests, before the closing `}`):

```rust
    #[test]
    fn list_box_readonly_lists_existing_box() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("u1");
        std::fs::create_dir_all(root.join("projects/sub")).unwrap();
        std::fs::write(root.join("todo.md"), b"hello").unwrap();
        let (entries, truncated) = list_box_readonly(tmp.path(), Some("u1"), 8, 1000).unwrap();
        let paths: Vec<&str> = entries.iter().map(|e| e.path.as_str()).collect();
        assert!(paths.contains(&"todo.md"));
        assert!(paths.contains(&"projects"));
        assert!(paths.contains(&"projects/sub"));
        assert!(!truncated);
        assert_eq!(entries.iter().find(|e| e.path == "todo.md").unwrap().size, 5);
    }

    #[test]
    fn list_box_readonly_absent_box_is_empty_and_not_created() {
        let tmp = tempfile::tempdir().unwrap();
        let (entries, truncated) = list_box_readonly(tmp.path(), Some("ghost"), 8, 1000).unwrap();
        assert!(entries.is_empty());
        assert!(!truncated);
        assert!(!tmp.path().join("ghost").exists(), "viewing must NOT create the box");
    }

    #[test]
    fn list_box_readonly_rejects_bad_user() {
        let tmp = tempfile::tempdir().unwrap();
        assert!(matches!(list_box_readonly(tmp.path(), None, 8, 1000), Err(ResolveError::BadUser(_))));
        assert!(matches!(list_box_readonly(tmp.path(), Some(""), 8, 1000), Err(ResolveError::BadUser(_))));
        assert!(matches!(list_box_readonly(tmp.path(), Some("a/b"), 8, 1000), Err(ResolveError::BadUser(_))));
    }

    #[cfg(unix)]
    #[test]
    fn list_box_readonly_does_not_follow_symlink_escape() {
        use std::os::unix::fs::symlink;
        let tmp = tempfile::tempdir().unwrap();
        let outside = tmp.path().join("outside");
        std::fs::create_dir_all(&outside).unwrap();
        std::fs::write(outside.join("secret.txt"), b"top secret").unwrap();
        let root = tmp.path().join("u1");
        std::fs::create_dir_all(&root).unwrap();
        symlink(&outside, root.join("link")).unwrap();
        let (entries, _) = list_box_readonly(tmp.path(), Some("u1"), 8, 1000).unwrap();
        let paths: Vec<&str> = entries.iter().map(|e| e.path.as_str()).collect();
        assert!(paths.contains(&"link"));
        assert!(!paths.iter().any(|p| p.contains("secret")));
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p llm-chat --no-default-features user_env::tests::list_box_readonly`
Expected: FAIL to compile — `cannot find function list_box_readonly in this scope`.

- [ ] **Step 3: Implement `list_box_readonly`.** Add it in `worker/src/user_env.rs` immediately after the `list_box_tree` function (before `fn walk_box`):

```rust
/// Read-only, NON-CREATING variant of `list_box_tree` for the admin Console
/// view: if the user's box does not exist yet, return an empty listing WITHOUT
/// creating it (viewing must never mutate the filesystem). Same confinement and
/// symlink-safety as `list_box_tree` (a symlink is listed but never descended,
/// so the walk can't escape the box). Fails closed on an invalid user id.
pub fn list_box_readonly(
    base: &Path,
    user_id: Option<&str>,
    max_depth: usize,
    max_entries: usize,
) -> Result<(Vec<DirEntry>, bool), ResolveError> {
    let uid = user_id.unwrap_or("").trim();
    if !valid_user_id(uid) {
        return Err(ResolveError::BadUser(format!("{uid:?}")));
    }
    let root_lexical = base.join(uid);
    // Non-creating: no box on disk → no sandbox yet (do NOT create one).
    if !root_lexical.exists() {
        return Ok((Vec::new(), false));
    }
    // Canonicalize the root to prove it resolves before walking (mirrors
    // resolve_user_cwd minus the create); the walk never follows symlinks.
    let root = root_lexical
        .canonicalize()
        .map_err(|e| ResolveError::Escape(format!("canonicalize root: {e}")))?;
    let mut out = Vec::new();
    let mut truncated = false;
    walk_box(&root, &root, 0, max_depth, max_entries, &mut out, &mut truncated);
    out.sort_by(|a, b| a.path.cmp(&b.path));
    Ok((out, truncated))
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p llm-chat --no-default-features user_env`
Expected: PASS — all `user_env` tests green (the `*_symlink_escape` test is `#[cfg(unix)]` and is simply not built on Windows).

- [ ] **Step 5: Commit**

```bash
git add worker/src/user_env.rs
git commit -F - <<'EOF'
feat(worker): non-creating list_box_readonly for the admin sandbox view

Lists a user's confined box like list_box_tree but returns empty (without
creating it) when the box does not exist — viewing must not mutate.

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
EOF
```

---

### Task 2: Worker `dir` `/control` honors a `create` flag

**Files:**
- Modify: `worker/src/lib.rs` (the `"dir" =>` arm of the `/control` handler, ~line 2573)

**Interfaces:**
- Consumes: `Task 1` `list_box_readonly`; existing `list_box_tree`, `USER_ENV_BASE`.
- Produces: the worker `/control` `dir` command accepts an optional `create` boolean (default `true`); `create:false` lists read-only/non-creating. Reply shape unchanged: `{ok:true, entries:[{path,dir,size}], truncated}` / `{ok:false, error}`.

- [ ] **Step 1: Replace the `"dir" =>` arm.** In `worker/src/lib.rs`, replace exactly this block:

```rust
                            "dir" => {
                                // List the caller's OWN box (recursive tree),
                                // reusing the same fail-closed confinement as
                                // `open`. user id is MANDATORY (no fallback).
                                let user_id = req.get("userId").and_then(|v| v.as_str());
                                tracing::info!(target: "backend::dir", user_id = ?user_id, "dir command received");
                                let base = USER_ENV_BASE.get().expect("validated at startup");
                                match crate::user_env::list_box_tree(base, user_id, 8, 2000) {
                                    Ok((entries, truncated)) => {
                                        let items: Vec<serde_json::Value> = entries.iter().map(|e| serde_json::json!({
                                            "path": e.path, "dir": e.dir, "size": e.size,
                                        })).collect();
                                        serde_json::json!({"ok": true, "entries": items, "truncated": truncated})
                                    }
                                    Err(e) => {
                                        tracing::warn!(target: "backend::dir", error = %e, "dir REJECTED (fail closed)");
                                        serde_json::json!({"ok": false, "error": format!("env: {e}")})
                                    }
                                }
                            }
```

with:

```rust
                            "dir" => {
                                // List a user's box (recursive tree). user id is
                                // MANDATORY (no fallback). `create` defaults true so
                                // the /chat self-view creates the box on first open;
                                // the admin read-only path sends create:false.
                                let user_id = req.get("userId").and_then(|v| v.as_str());
                                let create = req.get("create").and_then(|v| v.as_bool()).unwrap_or(true);
                                tracing::info!(target: "backend::dir", user_id = ?user_id, create, "dir command received");
                                let base = USER_ENV_BASE.get().expect("validated at startup");
                                let listed = if create {
                                    crate::user_env::list_box_tree(base, user_id, 8, 2000)
                                } else {
                                    crate::user_env::list_box_readonly(base, user_id, 8, 2000)
                                };
                                match listed {
                                    Ok((entries, truncated)) => {
                                        let items: Vec<serde_json::Value> = entries.iter().map(|e| serde_json::json!({
                                            "path": e.path, "dir": e.dir, "size": e.size,
                                        })).collect();
                                        serde_json::json!({"ok": true, "entries": items, "truncated": truncated})
                                    }
                                    Err(e) => {
                                        tracing::warn!(target: "backend::dir", error = %e, "dir REJECTED (fail closed)");
                                        serde_json::json!({"ok": false, "error": format!("env: {e}")})
                                    }
                                }
                            }
```

- [ ] **Step 2: Verify it compiles + worker tests pass**

Run: `cargo test -p llm-chat --no-default-features`
Expected: PASS — the crate compiles and all worker unit tests (incl. `user_env`) pass. (This arm lives inside `run_ws_server`; its behavior is covered by the `list_box_readonly`/`list_box_tree` unit tests + compilation. No new unit test.)

- [ ] **Step 3: Commit**

```bash
git add worker/src/lib.rs
git commit -F - <<'EOF'
feat(worker): dir /control honors a create flag (false = read-only view)

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
EOF
```

---

### Task 3: Manager `user-box` `/control` command

**Files:**
- Modify: `manager/src/main.rs` (insert an arm in `handle_control`'s `match cmd`, between the `"usage-daily"` and `"fifo"` arms, ~line 1609)

**Interfaces:**
- Consumes: existing `call_backend(port: u16, req: Value) -> Result<Value, _>`, `state.lock().await.instance_ports: Vec<u16>`.
- Produces: a `chat.admin`-gated `user-box` command — request `{cmd:"user-box", userId:"<id>"}`; reply is the worker `dir` reply (`{ok, entries, truncated}` / `{ok:false, error}`).

- [ ] **Step 1: Add the `user-box` arm.** In `manager/src/main.rs`, find this boundary between the `usage-daily` and `fifo` arms:

```rust
                match db.usage_daily(&cutoff).await {
                    Ok(rows) => compose_daily_reply(&rows),
                    Err(e) => serde_json::json!({"ok": false, "error": format!("usage-daily query: {e}")}),
                }
            }
            "fifo" => {
```

and insert the new arm between the closing `}` of `usage-daily` and `"fifo" =>`:

```rust
                match db.usage_daily(&cutoff).await {
                    Ok(rows) => compose_daily_reply(&rows),
                    Err(e) => serde_json::json!({"ok": false, "error": format!("usage-daily query: {e}")}),
                }
            }
            "user-box" => {
                // List ANY user's confined claude sandbox (read-only, non-creating)
                // for the admin Console. /control is already chat.admin-gated.
                match req.get("userId").and_then(|v| v.as_str()).filter(|s| !s.is_empty()) {
                    None => serde_json::json!({"ok": false, "error": "userId required"}),
                    Some(uid) => {
                        // Single shared env-base on the worker host → any worker can
                        // list any user's box; use the first instance.
                        let port = state.lock().await.instance_ports.first().copied();
                        match port {
                            None => serde_json::json!({"ok": false, "error": "no worker available"}),
                            Some(p) => match call_backend(
                                p,
                                serde_json::json!({"cmd": "dir", "userId": uid, "create": false}),
                            ).await {
                                Ok(v) => v,
                                Err(e) => serde_json::json!({"ok": false, "error": e.to_string()}),
                            },
                        }
                    }
                }
            }
            "fifo" => {
```

- [ ] **Step 2: Verify it compiles + manager tests pass**

Run: `cargo test -p llm-chat-manager`
Expected: PASS — compiles and existing manager tests stay green. (The arm is a read-through proxy; the listing logic is unit-tested in the worker. No new manager unit test.)

- [ ] **Step 3: Commit**

```bash
git add manager/src/main.rs
git commit -F - <<'EOF'
feat(manager): user-box /control command (admin sandbox listing)

chat.admin-gated; proxies the worker dir command with create:false for
an arbitrary userId.

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
EOF
```

---

### Task 4: admin-api `GET /api/users/{id}/files` (+ `control_request`)

**Files:**
- Modify: `admin-api/src/manager.rs`
- Modify: `admin-api/src/api/mod.rs` (route + handler + gating test)

**Interfaces:**
- Consumes: `Task 3` manager `user-box` command; existing `Operator` extractor, `mint_chat_token`, `cfg.manager_control_url`.
- Produces: `control_request(url, token, req: Value) -> Result<Value, String>`; `GET /api/users/{id}/files` returning `{configured, ok, entries, truncated, error}`.

- [ ] **Step 1: Generalize the manager proxy.** In `admin-api/src/manager.rs`, replace the `control_query` function:

```rust
pub async fn control_query(url: &str, token: &str, cmd: &str) -> Result<Value, String> {
```

…down to its closing `}` — with `control_request` (taking a request object) plus a thin `control_query` wrapper. Replace the whole function body. The new functions:

```rust
/// Open the manager /control WS, send one request object, read one reply.
/// The hello frame ({"ok":true,"hello":"manager-control"}) is consumed first.
/// The token rides the `Authorization: Bearer` header (never the URL).
pub async fn control_request(url: &str, token: &str, req: Value) -> Result<Value, String> {
    use tokio_tungstenite::tungstenite::client::IntoClientRequest;
    let mut request = url
        .into_client_request()
        .map_err(|e| format!("bad manager control URL {url}: {e}"))?;
    request.headers_mut().insert(
        "Authorization",
        format!("Bearer {token}")
            .parse()
            .map_err(|e| format!("bad auth header: {e}"))?,
    );
    let (mut ws, _) = tokio_tungstenite::connect_async(request)
        .await
        .map_err(|e| format!("manager connect: {e}"))?;

    // Consume the hello frame (or treat a missing one as the reply).
    let hello = read_text(&mut ws).await?;
    let hello_v: Value = serde_json::from_str(&hello).unwrap_or(json!({}));
    let is_hello = hello_v.get("hello").is_some();

    ws.send(Message::Text(req.to_string()))
        .await
        .map_err(|e| format!("manager send: {e}"))?;

    let reply = if is_hello { read_text(&mut ws).await? } else { hello };
    let _ = ws.close(None).await;
    serde_json::from_str(&reply).map_err(|e| format!("manager reply parse: {e}"))
}

/// Thin wrapper: send a bare `{"cmd": cmd}` request.
pub async fn control_query(url: &str, token: &str, cmd: &str) -> Result<Value, String> {
    control_request(url, token, json!({ "cmd": cmd })).await
}
```

(Leave `read_text`, `combine_control_replies`, and the existing tests unchanged.)

- [ ] **Step 2: Write the failing gating test.** In `admin-api/src/api/mod.rs`, add to `mod contract_tests` (after `usage_daily_route_requires_operator`):

```rust
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
```

- [ ] **Step 3: Run it to verify it fails**

Run: `cargo test -p llm-chat-admin-api --lib api::contract_tests::user_files_route_requires_operator`
Expected: FAIL — the route doesn't exist, so an unauthenticated request gets `404 NOT_FOUND`, not `401`.

- [ ] **Step 4: Add the route.** In `pub fn router`, after the line `.route("/api/users/{id}/secret", post(generate_secret).delete(delete_secret))`, add:

```rust
        .route("/api/users/{id}/files", get(user_files))
```

- [ ] **Step 5: Add the handler.** Place it next to `usage` (after the `usage_daily` handler):

```rust
/// A user's claude sandbox tree (read-only) via the manager's /control
/// "user-box". chat.admin-gated; capability-gated on MANAGER_CONTROL_URL like
/// chat_sessions/usage. The worker confines the listing to {base}/{userId}.
async fn user_files(_op: Operator, State(st): State<AppState>, Path(id): Path<String>)
    -> Result<Json<Value>, ApiError> {
    let Some(url) = st.cfg.manager_control_url.clone() else {
        return Ok(Json(json!({ "configured": false, "entries": [], "truncated": false })));
    };
    let token = st.zitadel.mint_chat_token().await?;
    let reply = crate::manager::control_request(&url, &token, json!({ "cmd": "user-box", "userId": id }))
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

- [ ] **Step 6: Run the gating test + full admin-api suite**

Run: `cargo test -p llm-chat-admin-api`
Expected: PASS — `user_files_route_requires_operator` now returns 401; all other tests still green.

- [ ] **Step 7: Commit**

```bash
git add admin-api/src/manager.rs admin-api/src/api/mod.rs
git commit -F - <<'EOF'
feat(admin-api): GET /api/users/{id}/files — proxy the sandbox listing

Adds control_request(req) (control_query becomes a wrapper) and an
Operator-gated, capability-gated endpoint returning the user's sandbox
tree from the manager user-box command.

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
EOF
```

---

### Task 5: admin-web types + `buildTree`

**Files:**
- Modify: `admin-web/lib/types.ts`
- Create: `admin-web/lib/sandbox-tree.ts`
- Test: `admin-web/lib/sandbox-tree.test.ts`

**Interfaces:**
- Produces: `SandboxEntry { path:string; dir:boolean; size:number }`, `SandboxFiles { configured:boolean; ok?:boolean; entries?:SandboxEntry[]; truncated?:boolean; error?:string }`; `TreeNode { name; path; dir; size; children:TreeNode[] }`; `buildTree(entries: SandboxEntry[]): TreeNode[]`.

- [ ] **Step 1: Add the types.** Append to `admin-web/lib/types.ts`:

```ts
// ---- Per-user sandbox listing (GET /api/users/{id}/files) ----
// The worker's confined box listing: flat, '/'-separated, sorted entries.
export interface SandboxEntry { path: string; dir: boolean; size: number }
export interface SandboxFiles {
  configured: boolean;        // false when MANAGER_CONTROL_URL is unset
  ok?: boolean;               // false on a manager/worker error
  entries?: SandboxEntry[];
  truncated?: boolean;        // depth/entry cap hit
  error?: string;
}
```

- [ ] **Step 2: Write the failing test.** Create `admin-web/lib/sandbox-tree.test.ts`:

```ts
import { describe, it, expect } from "vitest";
import { buildTree } from "./sandbox-tree";

describe("buildTree", () => {
  it("nests '/'-separated entries, folders before files, with sizes", () => {
    const tree = buildTree([
      { path: "todo.md", dir: false, size: 310 },
      { path: "projects", dir: true, size: 0 },
      { path: "projects/main.rs", dir: false, size: 842 },
      { path: "projects/sub", dir: true, size: 0 },
    ]);
    // folders before files at the top level
    expect(tree.map((n) => n.name)).toEqual(["projects", "todo.md"]);
    const projects = tree.find((n) => n.name === "projects")!;
    expect(projects.children.map((n) => n.name)).toEqual(["sub", "main.rs"]);
    expect(tree.find((n) => n.name === "todo.md")!.size).toBe(310);
    expect(projects.children.find((n) => n.name === "main.rs")!.size).toBe(842);
    expect(projects.path).toBe("projects");
    expect(projects.children.find((n) => n.name === "main.rs")!.path).toBe("projects/main.rs");
  });

  it("returns [] for no entries", () => {
    expect(buildTree([])).toEqual([]);
  });
});
```

- [ ] **Step 3: Run it to verify it fails**

Run: `pnpm -C admin-web exec vitest run lib/sandbox-tree.test.ts`
Expected: FAIL — `./sandbox-tree` cannot be resolved.

- [ ] **Step 4: Implement `buildTree`.** Create `admin-web/lib/sandbox-tree.ts`:

```ts
import type { SandboxEntry } from "@/lib/types";

export interface TreeNode {
  name: string;          // last path segment
  path: string;          // full relative path
  dir: boolean;
  size: number;          // 0 for directories
  children: TreeNode[];
}

// Turn the flat, '/'-separated entry list into a nested tree. Folders are
// placed before files at each level, then alphabetical. The worker already
// sorts entries so a parent dir precedes its children; we sort defensively too.
export function buildTree(entries: SandboxEntry[]): TreeNode[] {
  const roots: TreeNode[] = [];
  const byPath = new Map<string, TreeNode>();
  const sorted = [...entries].sort((a, b) => a.path.localeCompare(b.path));
  for (const e of sorted) {
    const segs = e.path.split("/");
    const node: TreeNode = {
      name: segs[segs.length - 1],
      path: e.path,
      dir: e.dir,
      size: e.dir ? 0 : e.size,
      children: [],
    };
    byPath.set(e.path, node);
    const parent = segs.length > 1 ? byPath.get(segs.slice(0, -1).join("/")) : undefined;
    if (parent) parent.children.push(node);
    else roots.push(node); // top-level, or an orphan whose parent wasn't listed
  }
  sortLevel(roots);
  return roots;
}

function sortLevel(nodes: TreeNode[]): void {
  nodes.sort((a, b) => (a.dir === b.dir ? a.name.localeCompare(b.name) : a.dir ? -1 : 1));
  for (const n of nodes) sortLevel(n.children);
}
```

- [ ] **Step 5: Run the test to verify it passes**

Run: `pnpm -C admin-web exec vitest run lib/sandbox-tree.test.ts`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add admin-web/lib/types.ts admin-web/lib/sandbox-tree.ts admin-web/lib/sandbox-tree.test.ts
git commit -F - <<'EOF'
feat(admin-web): sandbox types + buildTree (flat entries -> nested tree)

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
EOF
```

---

### Task 6: admin-web `<SandboxTree>` component

**Files:**
- Create: `admin-web/components/users/sandbox-tree.tsx`
- Test: `admin-web/components/users/sandbox-tree.test.tsx`

**Interfaces:**
- Consumes: `Task 5` `TreeNode`.
- Produces: `<SandboxTree nodes={TreeNode[]} />` — collapsible folders (expanded at the top level), files with sizes.

- [ ] **Step 1: Write the failing test.** Create `admin-web/components/users/sandbox-tree.test.tsx`:

```tsx
import { describe, it, expect } from "vitest";
import { render, screen } from "@testing-library/react";
import { SandboxTree } from "./sandbox-tree";
import { buildTree } from "@/lib/sandbox-tree";

describe("SandboxTree", () => {
  it("renders folders, files and sizes (top level expanded)", () => {
    const nodes = buildTree([
      { path: "projects", dir: true, size: 0 },
      { path: "projects/main.rs", dir: false, size: 842 },
      { path: "todo.md", dir: false, size: 310 },
    ]);
    render(<SandboxTree nodes={nodes} />);
    expect(screen.getByText("projects")).toBeInTheDocument();
    expect(screen.getByText("todo.md")).toBeInTheDocument();
    expect(screen.getByText("main.rs")).toBeInTheDocument(); // top-level folder expanded
    expect(screen.getByText("842 B")).toBeInTheDocument();
    expect(screen.getByText("310 B")).toBeInTheDocument();
  });
});
```

- [ ] **Step 2: Run it to verify it fails**

Run: `pnpm -C admin-web exec vitest run components/users/sandbox-tree.test.tsx`
Expected: FAIL — `./sandbox-tree` cannot be resolved.

- [ ] **Step 3: Implement the component.** Create `admin-web/components/users/sandbox-tree.tsx`:

```tsx
"use client";
import { useState } from "react";
import { ChevronRight, ChevronDown, Folder, File as FileIcon } from "lucide-react";
import type { TreeNode } from "@/lib/sandbox-tree";

function fmtSize(n: number): string {
  if (n < 1024) return `${n} B`;
  if (n < 1024 * 1024) return `${(n / 1024).toFixed(1)} KB`;
  return `${(n / 1024 / 1024).toFixed(1)} MB`;
}

function Node({ node, depth }: { node: TreeNode; depth: number }) {
  const [open, setOpen] = useState(depth < 1); // top level expanded
  if (node.dir) {
    return (
      <div>
        <button type="button" onClick={() => setOpen((o) => !o)}
          className="hover:bg-muted/50 flex w-full items-center gap-1.5 rounded px-1 py-0.5 text-left text-sm"
          style={{ paddingLeft: `${depth * 14}px` }}>
          {open ? <ChevronDown className="size-3.5 shrink-0" /> : <ChevronRight className="size-3.5 shrink-0" />}
          <Folder className="size-4 shrink-0 text-sky-600" />
          <span className="truncate">{node.name}</span>
        </button>
        {open && node.children.map((c) => <Node key={c.path} node={c} depth={depth + 1} />)}
      </div>
    );
  }
  return (
    <div className="flex items-center gap-1.5 px-1 py-0.5 text-sm"
      style={{ paddingLeft: `${depth * 14 + 19}px` }}>
      <FileIcon className="text-muted-foreground size-4 shrink-0" />
      <span className="truncate">{node.name}</span>
      <span className="text-muted-foreground ml-auto text-xs tabular-nums">{fmtSize(node.size)}</span>
    </div>
  );
}

export function SandboxTree({ nodes }: { nodes: TreeNode[] }) {
  return <div className="space-y-0.5">{nodes.map((n) => <Node key={n.path} node={n} depth={0} />)}</div>;
}
```

- [ ] **Step 4: Run the test to verify it passes**

Run: `pnpm -C admin-web exec vitest run components/users/sandbox-tree.test.tsx`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add admin-web/components/users/sandbox-tree.tsx admin-web/components/users/sandbox-tree.test.tsx
git commit -F - <<'EOF'
feat(admin-web): SandboxTree component (collapsible folders + file sizes)

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
EOF
```

---

### Task 7: admin-web Users panel Sandbox section

**Files:**
- Modify: `admin-web/app/(dash)/users/page.tsx`

**Interfaces:**
- Consumes: `Task 4` `GET /api/users/{id}/files`; `Task 5` `SandboxFiles`, `buildTree`; `Task 6` `SandboxTree`; existing `api`, `PanelSection`, `selected: User | null`, `Button`.

- [ ] **Step 1: Add imports.** In `admin-web/app/(dash)/users/page.tsx`, add to the imports:

```tsx
import { SandboxTree } from "@/components/users/sandbox-tree";
import { buildTree } from "@/lib/sandbox-tree";
```

and add `SandboxFiles` to the existing `@/lib/types` import list.

- [ ] **Step 2: Add state + fetch.** Inside the component, add state next to the other `useState`s:

```tsx
  const [sandbox, setSandbox] = useState<SandboxFiles | null>(null);
```

and add an effect that loads the sandbox when the selected user changes (place after the existing effects):

```tsx
  useEffect(() => {
    if (!selected) { setSandbox(null); return; }
    let alive = true;
    setSandbox(null); // loading
    api
      .get<SandboxFiles>(`/api/users/${selected.id}/files`)
      .then((s) => { if (alive) setSandbox(s); })
      .catch(() => { if (alive) setSandbox({ configured: true, ok: false, error: "Failed to load sandbox" }); });
    return () => { alive = false; };
  }, [selected]);
```

- [ ] **Step 3: Render the Sandbox section.** In the `<DetailPanel>`, after the `<PanelSection title="App access & roles">…</PanelSection>` (the last section), add:

```tsx
              <PanelSection title="Sandbox">
                {!sandbox ? (
                  <span className="text-muted-foreground text-sm">Loading…</span>
                ) : sandbox.configured === false ? (
                  <span className="text-muted-foreground text-sm">Sandbox view not configured (MANAGER_CONTROL_URL).</span>
                ) : sandbox.ok === false ? (
                  <span className="text-destructive text-sm">{sandbox.error || "Sandbox unavailable"}</span>
                ) : (sandbox.entries ?? []).length === 0 ? (
                  <span className="text-muted-foreground text-sm">No sandbox yet.</span>
                ) : (
                  <>
                    <SandboxTree nodes={buildTree(sandbox.entries ?? [])} />
                    {sandbox.truncated && (
                      <p className="text-muted-foreground mt-2 text-xs">Showing first 2000 entries (truncated).</p>
                    )}
                  </>
                )}
              </PanelSection>
```

- [ ] **Step 4: Typecheck + full vitest**

Run: `pnpm -C admin-web exec tsc --noEmit && pnpm -C admin-web exec vitest run`
Expected: PASS — no type errors; all unit tests (incl. `buildTree` and `SandboxTree`) green.

- [ ] **Step 5: Commit**

```bash
git add "admin-web/app/(dash)/users/page.tsx"
git commit -F - <<'EOF'
feat(admin-web): show the user's sandbox tree in the Users panel

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
EOF
```

---

## Final verification (after all tasks)

- [ ] `cargo test -p llm-chat --no-default-features` — worker tests pass.
- [ ] `cargo test -p llm-chat-manager` — compiles, tests pass.
- [ ] `cargo test -p llm-chat-admin-api` — incl. `user_files_route_requires_operator`.
- [ ] `pnpm -C admin-web exec tsc --noEmit` — clean.
- [ ] `pnpm -C admin-web exec vitest run` — all green.
- [ ] Manual (rebuild `admin-api` + `admin-web` + the native worker; `MANAGER_CONTROL_URL` set): open `/users`, select a user who has chatted → the **Sandbox** section shows their folder/file tree; select a user who never chatted → **No sandbox yet** (and no box dir is created on the worker host).

## Self-review notes (author)

- **Spec coverage:** non-creating `list_box_readonly` (Task 1) ✓; `create` flag on the worker `dir` command (Task 2) ✓; manager `user-box` (Task 3) ✓; admin-api endpoint + `control_request` + capability gate (Task 4) ✓; types + `buildTree` (Task 5) ✓; `<SandboxTree>` (Task 6) ✓; Users panel section + states (Task 7) ✓; security gates inherited (Operator + manager `/control`) ✓.
- **Deviation from spec:** the spec listed "a Users-panel render test"; this plan instead unit-tests `buildTree` (pure) and renders `<SandboxTree>` directly — equivalent coverage of the new UI logic without mocking the Users page's large multi-fetch fan-out. The page wiring is covered by tsc + manual/e2e.
- **Type consistency:** `SandboxEntry`/`SandboxFiles` (Task 5) are produced by Task 4's JSON envelope and consumed in Tasks 6–7; `TreeNode`/`buildTree` names match across Tasks 5–7; the worker `dir` reply shape (`entries:[{path,dir,size}], truncated`) is unchanged from Task 2 through the manager (Task 3) and admin-api (Task 4).
