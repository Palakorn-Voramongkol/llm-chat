# User sandbox tree in the Users panel — design

**Status:** designed 2026-06-27; not yet implemented.
**Area:** worker (`user_env.rs` + `/control`), manager (`/control`), admin-api (BFF), admin-web (Console Users page).

## Problem / goal

On the operator Console Users page (`/users`), selecting a user opens a detail
panel (Identity / Activity / Usage / App access & roles). Operators have no way
to see **what is inside that user's Claude sandbox** — the per-user confined
working directory (`{LLM_CHAT_USER_ENV_BASE}/{userId}`) where their `claude`
sessions run. Goal: add a **Sandbox** section to that panel showing the user's
sandbox as a **collapsible tree of folders + files with sizes**.

## Reuse (the hard part already exists)

`worker/src/user_env.rs` already has `list_box_tree(base, user_id, max_depth,
max_entries) -> (Vec<DirEntry>, bool)` — a recursive, **confined**, symlink-safe
listing returning `DirEntry { path: String /* relative, '/'-sep */, dir: bool,
size: u64 }` plus a `truncated` flag. It fails closed (mandatory user id,
traversal rejected, symlinks listed but never descended). The worker `/control`
`dir` command (`worker/src/lib.rs`) already wraps it for an arbitrary `userId`.
The only thing missing is an **operator-scoped path** to it and a **non-creating**
variant for read-only viewing.

## Architecture / data flow

```
admin-web Users panel: select user
  → GET /api/users/{id}/files                     admin-api  (Operator = chat.admin)
     → manager /control { cmd:"user-box", userId:id }   (NEW, chat.admin-gated)
        → call_backend(worker, { cmd:"dir", userId:id, create:false })   (worker /control)
           → user_env::list_box_readonly(base, id, 8, 2000)
              → { ok, entries:[{path,dir,size}], truncated }
```

The worker owns the sandbox filesystem (it runs natively on the host and spawns
`claude`), so the manager must proxy to a worker — admin-api never reaches a
worker directly. This mirrors the existing `/api/chat-sessions` and `/api/usage`
read-only proxy pattern.

## Layer 1 — worker (`user_env.rs` + `/control`)

### `user_env.rs`: a non-creating read-only listing

Add `list_box_readonly(base, user_id, max_depth, max_entries) -> Result<(Vec<DirEntry>, bool), ResolveError>`:

- Validate `user_id` (`valid_user_id`); empty/invalid → `BadUser` (fail closed).
- Compute the lexical root `confine_path(base, uid, None)` = `{base}/{uid}`.
- **If the root does not exist** (`std::fs::symlink_metadata` errors / not a dir)
  → return `(vec![], false)` — **no box yet, and DO NOT create one**.
- Else canonicalize the root and walk it with the existing `walk_box` (which uses
  `symlink_metadata` and never descends symlinks, so the walk cannot escape).
  Entries are relative, '/'-separated, sorted; `truncated` set by depth/entry caps.

`list_box_tree` (create-then-list) is unchanged — it stays the `/chat` `/dir`
path's behavior. `walk_box` is reused as-is.

### `/control` `dir` command: optional `create` flag

The `dir` handler gains an optional `create` boolean (default **`true`**, so the
existing `/chat` self-view path is byte-for-byte unchanged). When `create:false`
it calls `list_box_readonly` instead of `list_box_tree`. Reply shape unchanged:
`{ ok:true, entries:[{path,dir,size}], truncated }` or `{ ok:false, error }`.

## Layer 2 — manager `/control`

Add a `user-box` command to `handle_control` (already `chat.admin`-gated):

- Read `userId` from the request; missing/empty → `{ ok:false, error:"userId required" }`.
- Pick a worker: the **first available instance port** (single shared env-base —
  see Assumptions). No worker available → `{ ok:false, error:"no worker" }`.
- `call_backend(port, { cmd:"dir", userId, create:false })`; return the worker's
  reply (`{ok, entries, truncated}` / `{ok:false,error}`) unchanged.

It does not touch session state — purely a read-through to the worker.

## Layer 3 — admin-api

### Route + handler

`GET /api/users/{id}/files` (the `Operator` extractor gates it on `chat.admin`):

- If `cfg.manager_control_url` is `None` → `{ configured:false, entries:[], truncated:false }`.
- Else mint a chat token (`mint_chat_token`) and call the manager `user-box`
  command, returning `{ configured:true, ok, entries, truncated, error? }`
  (degrade to `{ ok:false, error }` on transport failure — never a 5xx, mirroring
  `chat_sessions`/`usage`).

### `manager.rs`: pass a param to `/control`

Generalize the proxy to send an arbitrary request object:
`control_request(url, token, req: Value) -> Result<Value, String>` sends `req`
verbatim (the existing `control_query(url, token, cmd)` becomes a thin wrapper
`control_request(url, token, json!({"cmd": cmd}))`, so current callers are
unchanged). The files handler uses
`control_request(url, token, json!({ "cmd":"user-box", "userId": id }))`.

## Layer 4 — admin-web (Console Users page)

### Types (`lib/types.ts`)

```ts
export interface SandboxEntry { path: string; dir: boolean; size: number }
export interface SandboxFiles {
  configured: boolean;
  ok?: boolean;
  entries?: SandboxEntry[];
  truncated?: boolean;
  error?: string;
}
```

### Pure tree builder (`lib/sandbox-tree.ts`)

`buildTree(entries: SandboxEntry[]): TreeNode[]` — turn the flat, '/'-separated,
sorted entry list into nested `TreeNode { name, path, dir, size, children }`,
folders before files at each level. Pure and unit-tested (the flat→nested logic
is the only non-trivial UI logic).

### `<SandboxTree>` component (`components/users/sandbox-tree.tsx`)

Recursive render of `TreeNode[]`: each folder is a button that toggles expand
(lucide `ChevronRight`/`ChevronDown` + `Folder`), each file shows `File` + name +
`fmtBytes(size)`. There is no shadcn "tree" primitive, so this is a justified
app-specific component built from minimal markup + `Button` for the toggles +
lucide icons (shadcn-first respected: no primitive exists for a file tree).

### Users panel wiring (`app/(dash)/users/page.tsx`)

- Add `sandbox: SandboxFiles | null` state and a `loadSandbox(userId)` that
  fetches `GET /api/users/{id}/files` (best-effort, its own try/catch — a sandbox
  error never blanks the rest of the panel).
- Fetch on `selected` change (alongside the existing per-user fan-out), with an
  `alive` guard like the existing detail fetches.
- New `<PanelSection title="Sandbox">` rendering, by state:
  - **loading** → "Loading…"
  - **not configured** (`configured:false`) → "Sandbox view not configured (MANAGER_CONTROL_URL)."
  - **error** (`ok:false`) → the error string, inline.
  - **empty** (`entries:[]`) → "No sandbox yet."
  - **tree** → `<SandboxTree>`, plus a "Showing first 2000 entries (truncated)"
    note when `truncated`, and a small **Refresh** button (`Button variant="ghost" size="sm"`).

## Behavior decisions

- **Read-only / non-mutating view (chosen):** viewing a user who never chatted
  shows **"No sandbox yet"** and does **not** create a box directory for them
  (`list_box_readonly`). Reserved purely for the admin view; the `/chat` `/dir`
  self-view keeps its create-on-open behavior.
- **Caps:** depth 8, 2000 entries (unchanged from `list_box_tree`); `truncated`
  surfaced in the UI.
- **Load on select + manual Refresh** (no polling).

## Security

- `chat.admin` enforced twice: the admin-api `Operator` extractor and the manager
  `/control` gate. No new ungated surface.
- The worker confines to `{base}/{userId}` (validated id, no traversal, symlinks
  listed but never followed). `userId` is the selected Zitadel user id (numeric
  string — passes `valid_user_id`).
- Non-creating view does not mutate the filesystem.
- Token rides the `Authorization: Bearer` header to the manager (never the URL),
  as the existing proxy already does.

## Error handling (fail-soft, per-tile)

`not configured` / `unreachable` / `empty` / `truncated` are all distinct,
explicit states. A sandbox failure degrades only the Sandbox section — the rest
of the user panel is unaffected (the established per-tile best-effort pattern).

## Testing

- **worker:** unit tests for `list_box_readonly` — lists a populated box; returns
  empty for an **absent** box **without creating it**; does not follow a symlink
  out of the box; rejects an empty/invalid user id. (Mirror the `list_box_tree`
  tests.)
- **admin-api:** route-gating test (`GET /api/users/{id}/files` → 401 without an
  operator session, like `usage_route_requires_operator`); `control_request`
  sends the given request object verbatim.
- **admin-web:** vitest for `buildTree` (flat→nested, folders-first, sizes); a
  Users-panel render test (mock `/api/users/{id}/files`, select a user, assert the
  tree renders a known folder + file, and the "No sandbox yet" empty state).

## Assumptions

- **Single shared env-base on the worker host.** True today (one native worker;
  spawning-mode workers share the host filesystem), so the manager can ask any
  worker for any user's box. Workers on *separate* hosts with *separate* bases
  would need per-host routing — explicitly out of scope.

## Out of scope

- Viewing/downloading **file contents** (this is a listing only — path, dir, size).
- Editing/deleting sandbox files from the Console.
- Pagination beyond the existing depth/entry caps + `truncated` indicator.
- Live updates / polling.

## File-by-file change list

**worker**
- `worker/src/user_env.rs` — add `list_box_readonly` + tests.
- `worker/src/lib.rs` — `dir` `/control` handler honors an optional `create` flag.

**manager**
- `manager/src/main.rs` — `user-box` command in `handle_control` (pick a worker,
  proxy `dir` with `create:false`).

**admin-api**
- `admin-api/src/manager.rs` — generalize to `control_request(url, token, req)`.
- `admin-api/src/api/mod.rs` — `GET /api/users/{id}/files` route + handler.

**admin-web**
- `admin-web/lib/types.ts` — `SandboxEntry`, `SandboxFiles`.
- `admin-web/lib/sandbox-tree.ts` + test — `buildTree`.
- `admin-web/components/users/sandbox-tree.tsx` — `<SandboxTree>`.
- `admin-web/app/(dash)/users/page.tsx` — Sandbox `PanelSection` + fetch.
- a Users-panel render test for the Sandbox section.
