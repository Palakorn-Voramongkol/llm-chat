# Per-app sandbox templates — store + first-login provisioning (Sub-project 1) — design

**Status:** designed 2026-06-29; not yet implemented.
**Area:** manager (template store + `/provision` endpoint + variable substitution), worker (provision-app-box command), kabytech-backend (first-login hook), config.
**Scope:** This is **Sub-project 1** of two. It builds the **backend** end-to-end: a per-app sandbox-template store, and the runtime that materializes a template into the user's box at first login (kabytech is the first consumer). **Sub-project 2** (the admin-web Console editor that authors templates) is OUT OF SCOPE here — in this sub-project the template is **seeded** server-side.

## Problem / goal

Each platform application should be able to define a **sandbox template** — a folder
structure + files (e.g. `README.md`, `config.json`) — that is materialized into a
user's per-user box at `{LLM_CHAT_USER_ENV_BASE}/{userId}/{app}/` the **first time**
that user logs into the app. The user's box is owned and confined by the **worker**
(`worker/src/user_env.rs`); the box lives host-side at `.user-envs/{userId}`. kabytech
is a Docker gateway with no box access. So provisioning must flow through the box's
owner without spreading the confinement boundary.

**Goal (Sub-project 1):** a manager-owned per-app template store, and a first-login
provisioning chain (kabytech `/callback` → manager `/provision` → worker materialize)
that creates `{userId}/{app}/` from the app's template, confined and idempotent. The
template is **seeded** for kabytech in this sub-project; authoring via the Console is
Sub-project 2.

## Decisions (from brainstorming)

- **What/when:** a folder named after the app (`{userId}/kabytech/`), created on first kabytech login (the `/callback` after the `chat.user` gate), create-if-absent (idempotent).
- **Seed content:** per-app **template** = folder structure + files (`README.md`, `config.json`, …) — not hardcoded; consumed from the store.
- **Mechanism:** the **worker** stays the SOLE box owner; kabytech triggers provisioning via a new `chat.user`-gated manager endpoint, self-scoped to the authenticated user's own box.
- **Store owner:** the **manager** (it has `chat_db` and owns the worker pool / box domain; admin-api is stateless). Provisioning is self-contained: manager reads its own store → worker materializes. (Sub-project 2's Console relays edits → admin-api → manager.)
- **Sequencing:** backend (this spec) first; admin-web editor second.

## Architecture / data flow

```
user logs into kabytech (browser, custom loginV2)
  → kabytech-backend /callback   (chat.user gate passes; EndUser session set)
      → best-effort: open WS  ws://manager:7777/provision   (Authorization: Bearer <user's own access token>)
              send { "type":"provision", "app":"kabytech" }
          → manager: verify JWT (chat.user); userId := principal.user_id (NEVER client-supplied)
          → manager: load template for app "kabytech" from chat_db (none → no-op, ok)
          → manager: resolve display name via the user's own /userinfo (reuse resolve_user_label)
          → manager: substitute {{name}} {{userId}} {{app}} {{date}} in each file's content
          → manager → worker (call_backend, loopback):
                { "cmd":"provision-app-box", "userId":<uid>, "app":"kabytech", "files":[ {path,dir,content}, … ] }
              → worker: for each entry, confine_path(base, userId, "{app}/{path}")  (rejects traversal),
                        create {base}/{userId}/kabytech/, write each file/dir ONLY IF ABSENT
              → { ok:true, created:<n> }
          → manager → { "type":"provision", "ok":true } ; close
  → login completes (unchanged whether or not provisioning succeeded)
```

The kabytech call is **best-effort**: login already succeeded at the gate, so a failure
(or the manager being unreachable) is logged and ignored; the next login retries
(idempotent). The user only ever provisions **their own** box (userId from the verified
token).

## Manager

### Template store (`chat_db`)

A new table, dialect-portable like the existing `chat_question` (SQLite + Postgres):

```sql
CREATE TABLE app_sandbox_template (
  app_code     TEXT PRIMARY KEY,   -- e.g. "kabytech" (matches secrets/app_codes.json keys)
  template_json TEXT NOT NULL,     -- JSON array of TemplateEntry (below)
  updated_at   TEXT NOT NULL
);
```

`ChatDb` gains:
- `get_template(app_code) -> Option<String>` (the raw `template_json`, or None).
- `upsert_template(app_code, template_json, updated_at)` (used by the seed; Sub-project 2 reuses it via admin-api→manager).

**Startup seed:** on boot, if `get_template("kabytech")` is None, `upsert_template` the
default kabytech template (below). Idempotent: never overwrites an existing row (so a
later Console edit survives a restart).

### Template format (pure types)

```
struct TemplateEntry { path: String, dir: bool, content: String }   // content empty when dir
fn parse_template(json: &str) -> Result<Vec<TemplateEntry>, String>  // serde; reject non-array / missing fields
```
`path` is box-relative (under `{userId}/{app}/`). A `dir:true` entry is an empty folder;
a `dir:false` entry is a file with `content`.

### Variable substitution (pure)

```
struct TemplateVars { name: String, user_id: String, app: String, date: String }
fn substitute_vars(content: &str, v: &TemplateVars) -> String
```
Replaces the literal tokens `{{name}}`, `{{userId}}`, `{{app}}`, `{{date}}` (no other
templating; unknown `{{…}}` left as-is). `date` is the current date `YYYY-MM-DD`.

### New `/provision` WS endpoint (chat.user-gated, no chat session)

Added to the post-handshake path dispatch next to `/identity`. Reuses the existing
handshake (verified JWT + `chat.user`) and the **captured access token** (the same
holder `/identity` uses) for the `/userinfo` name lookup. It opens **no** chat session.

- Request: `{ "type":"provision", "app":"<code>" }`.
- `userId` := the verified principal's user id (never from the request).
- Load `get_template(app)`. None → reply `{type:"provision", ok:true, provisioned:false}` (no-op).
- Resolve `name` via `resolve_user_label(http, issuer, token, email, userId)` (existing).
- Build `TemplateVars`, substitute each entry's content, and `call_backend` a worker with
  `{cmd:"provision-app-box", userId, app, files:[…]}`.
- Reply `{type:"provision", ok:true, provisioned:true}`; close. Worker/DB errors → `{type:"err", text}`.

The handshake dispatch change mirrors the `/chat` change already shipped: `/provision`,
like `/identity` and `/chat`, consumes the captured token (the other paths still drop it).

## Worker

### New control command `provision-app-box`

Handled where the existing `dir` / `user-box` control commands are dispatched. Input:
`{ cmd:"provision-app-box", userId, app, files:[{path,dir,content}] }`. For each entry:

1. `full = confine_path(base, userId, &format!("{app}/{path}"))` — reuses the existing
   confinement (rejects empty/`.`/`..`/absolute/`\\`/`:`/NUL components for **both** `app`
   and every `path` component). A rejected entry fails the command (fail closed).
2. `dir:true` → `create_dir_all(full)`.
3. `dir:false` → `create_dir_all(full.parent())`, then **write only if `!full.exists()`**
   (never clobber). 

The worker also ensures `{base}/{userId}/{app}/` exists (via `resolve_user_cwd(base, userId, Some(app))`,
which create+confines and canonicalize-proves containment). Returns `{ ok:true, created:<n> }`.
Pure helper `provision_entries(root, entries) -> io::Result<usize>` is unit-tested with a temp dir.

## kabytech-backend

In `/callback`, after the `EndUser` session is written, fire a **best-effort** provision:
open a WS to `KABY_MANAGER_PROVISION_URL` with the user's access token (the one already
obtained in `exchange_code`), send `{type:"provision", app: KABY_APP_CODE}`, await the
reply with a short timeout, log any failure, and continue regardless. A new config:

- `KABY_MANAGER_PROVISION_URL` (e.g. `ws://manager:7777/provision`) — **optional**: absent →
  provisioning is skipped (the feature is off), login unaffected.
- `KABY_APP_CODE` (default `"kabytech"`) — the app code this gateway provisions under.

## Config

- `KABY_MANAGER_PROVISION_URL` + `KABY_APP_CODE` in the kabytech-backend env (`.env.local.example`
  + compose); both optional (absent → feature off, no login impact).
- No new manager config (the template store is in the existing `chat_db`; the seed is built in).

## Default kabytech template (seeded)

```json
[
  { "path": "README.md", "dir": false,
    "content": "# kabytech workspace\n\nThis is {{name}}'s kabytech workspace ({{userId}}).\nCreated {{date}}.\n" },
  { "path": "config.json", "dir": false,
    "content": "{\n  \"app\": \"{{app}}\",\n  \"version\": 1,\n  \"createdAt\": \"{{date}}\",\n  \"settings\": {}\n}\n" }
]
```

## Error handling (fail-closed where it matters, fail-soft where it doesn't)

- kabytech `/callback` provision: **best-effort** — failure/timeout/unreachable manager is
  logged, never blocks login.
- Manager `/provision`: missing template → no-op `ok`. A malformed stored template, or a
  worker rejection (traversal), is an `err` reply (logged) — but since the call is best-effort
  at kabytech, it still doesn't block login.
- Worker `provision-app-box`: any path that fails `confine_path` fails the command (fail
  closed — never write outside the box). Existing files are never overwritten.

## Security

- The worker remains the **single owner** of the box and its confinement; kabytech gains no
  direct box access.
- Every template path (and the `app` segment) flows through `confine_path` — traversal,
  absolute paths, and illegal chars are rejected.
- `userId` is taken from the **verified** JWT at the manager; `/provision` is `chat.user`-gated
  and can only provision the **caller's own** box.
- The user's token is used only for the user's own `/userinfo` (name), within the request,
  then dropped — same posture as `/identity`.

## Testing

- **Worker (pure + IT):** `provision_entries` creates dirs/files under a temp root; writes
  only-if-absent (a pre-existing file is untouched); a template path with `..`/absolute is
  rejected by `confine_path` (fail closed). The control round-trip is the gated IT.
- **Manager (pure):** `parse_template` (valid array, reject non-array/missing fields);
  `substitute_vars` (all four tokens; unknown `{{x}}` left as-is). `get_template`/`upsert_template`
  schema test (SQLite memory). The `/provision` round-trip + token self-scoping is IT.
- **kabytech (pure/IT):** `/callback` fires the provision best-effort and login still
  completes when the manager is unreachable (provision URL pointed at a dead port).
- **e2e:** a fresh kabytech login (the `login_kaby.py`-style flow) results in
  `.user-envs/{userId}/kabytech/README.md` + `config.json` existing, with `{{name}}`/`{{date}}`
  substituted; a second login leaves them unchanged.

## Out of scope (Sub-project 2 and beyond)

- The **admin-web Console editor** to author per-app templates (folder tree + per-file content).
- The **admin-api → manager** relay that persists Console edits to the template store.
- Per-app templates for apps other than kabytech (the mechanism is general — `app` is a
  parameter — but only kabytech is wired + seeded here).
- Re-materializing/migrating an existing box when a template changes (templates apply at
  first login / create-if-absent only).

## File-by-file change list

**manager**
- `manager/src/main.rs` — `app_sandbox_template` schema (SQLite + Postgres) in the `ChatDb`
  init; `ChatDb::get_template`/`upsert_template`; startup seed of the kabytech default;
  `TemplateEntry`/`parse_template`/`TemplateVars`/`substitute_vars` (pure) + tests; the
  `/provision` path branch (consumes the captured token like `/identity`/`/chat`) and
  `handle_provision`.

**worker**
- `worker/src/user_env.rs` — `provision_entries(root, &[TemplateEntry-equiv])` pure helper
  + tests (reuses `confine_path`).
- the worker control dispatch (where `dir`/`user-box` are handled) — add `provision-app-box`.

**kabytech-backend**
- `services/kabytech/backend/src/config.rs` — optional `manager_provision_url` + `app_code`.
- `services/kabytech/backend/src/auth.rs` (`callback`) — best-effort provision call.
- `services/kabytech/backend/src/main.rs` / a small `provision.rs` — the WS provision client.

**config**
- `.env.local.example` + `docker-compose.yml` (kabytech-backend) — `KABY_MANAGER_PROVISION_URL`,
  `KABY_APP_CODE`.
