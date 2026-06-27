# Multi-application Sessions page — design

**Status:** designed 2026-06-27; not yet implemented.
**Area:** admin-api (app registry + endpoints + per-app token), admin-web (Sessions page selector), config.
**Rules it enforces:** *Business logic lives in the backend, not the client* (the registry + display-ready app list are server-owned). *Fail closed* (unknown app key is rejected, never substituted). *Don't guess* (sign-ins are kept platform-wide because Zitadel's audit log is verifiably not project-filterable).

## Problem / goal

The admin-web `/sessions` page monitors chat sessions through a **single** manager
(`MANAGER_CONTROL_URL`) — effectively the **llm-chat** application's manager. The
platform is multi-application (each application is a Zitadel project), but there is
no way to pick a different application, and the session data has no application
dimension. The "Recent sign-ins" panel is already platform-wide (instance audit),
which the page does not make clear.

**Goal:** a **future-ready application selector** on the Sessions page. A real app
picker, backed by a backend registry mapping *application → chat manager*, that
re-scopes the per-app panels to the chosen application. Today the registry holds a
single entry (llm-chat), so the picker shows one option; adding a second chat
manager later is **config-only**, no code change. No fake data, no fake apps.

## Decisions (from brainstorming)

- **Model:** future-ready selector — one app today, registry-backed, config-only to add more.
- **Scope on selection:** the chat panels (Active sessions / Users chatting / Workers / Live chat sessions) **and** the chat-manager health row re-scope to the selected app. Zitadel health stays global.
- **Recent sign-ins:** stays **platform-wide**, relabeled "Recent sign-ins — all applications". Zitadel's audit log is **not** project-filterable (verified: `admin/v1/events/_search` filters only by org / user / aggregate / date — `admin-api/src/zitadel/events.rs:28`; `password.check` sign-in events carry no application context at all). Per-app sign-in filtering is therefore out of scope.

## Architecture / data flow

```
admin-web /sessions
  ├─ GET /api/session-apps           → [{ key, name }]      (populate the picker)
  └─ GET /api/chat-sessions?app=<key> → { configured, ok, list, instances, clients }
                                          (queries THAT app's manager /control)

admin-api app registry (one entry per chat-capable application):
  { key, name, controlUrl, projectId }
    key        – stable id used in the ?app= param and the picker
    name       – display label
    controlUrl – ws://…/control for that application's manager
    projectId  – Zitadel project whose audience the SA token must target
                 (so that app's manager accepts the token) + whose roles it asserts

  Source:  MANAGER_CONTROL_APPS  (JSON array)            [new, optional]
  Fallback: if MANAGER_CONTROL_APPS is absent but the legacy MANAGER_CONTROL_URL is
            set → synthesize ONE entry { key:"llm-chat", name:"llm-chat",
            controlUrl:<MANAGER_CONTROL_URL>, projectId:<ZITADEL_PROJECT_ID> }.
  Default: the FIRST entry — used by the existing single-manager endpoints
           (usage, usage-daily, user files) which are NOT app-selectable here.
```

The selected application re-scopes only the chat panels + the chat-manager health
row. Recent sign-ins and Zitadel health remain instance-wide. The client sends only
the selected `app` key; the backend owns the registry, mints the right token, picks
the right manager, and returns display-ready data.

## admin-api

### App registry (`config` + a small pure module)

A pure type and parser, unit-testable, HTTP-agnostic:

- `struct SessionApp { key: String, name: String, control_url: String, project_id: String }`
- `parse_session_apps(manager_control_apps: Option<&str>, legacy_url: Option<&str>, legacy_project_id: &str) -> Vec<SessionApp>`
  - If `manager_control_apps` is present: parse JSON array of `{ key, name, controlUrl, projectId }`. Each field required; an entry missing a field is **dropped** (logged), never defaulted (no guessed URLs/audiences).
  - Else if `legacy_url` is present: return one synthesized llm-chat entry (above).
  - Else: return `[]` (nothing configured).
- Stored on `AdminConfig` as `session_apps: Vec<SessionApp>` (replaces the lone `manager_control_url` as the source of truth; the env var itself stays supported via the fallback).
- Helpers: `default_app(&[SessionApp]) -> Option<&SessionApp>` (first entry); `find_app<'a>(&'a [SessionApp], key: &str) -> Option<&'a SessionApp>`.

### Endpoints

- `GET /api/session-apps` (Operator-gated, like the other admin endpoints) →
  `{ "apps": [{ "key": "...", "name": "..." }] }` from `session_apps`. `[]` when
  nothing configured. Display-ready (no URLs/project ids leaked to the client).
- `GET /api/chat-sessions?app=<key>` — change the existing handler:
  - Resolve the entry: `app` present → `find_app` (unknown → **400 fail closed**, never substitute); `app` absent → `default_app` (none → `{ configured: false }`, today's behavior).
  - Mint a chat token for the entry's `projectId`, query the entry's `controlUrl` for `list` / `instances` / `clients`, combine as today (`combine_control_replies`).
- `GET /api/signins`, `/api/status`: **unchanged** (platform-wide).
- `GET /api/usage`, `/api/usage-daily`, `/api/users/{id}/files`: same behavior/scope, but their internal call switches from the removed `cfg.manager_control_url` to the **default** app's `control_url` + `mint_chat_token(default.project_id)`. Not app-selectable (out of scope).

### Token minting

- Generalize `mint_chat_token` → `mint_chat_token(project_id: &str)`: the scope's
  `…project:id:{project_id}:aud…` uses the passed project id instead of
  `self.cfg.project_id`. The default/llm-chat entry's `project_id` equals the
  existing `cfg.project_id`, so today's token is byte-for-byte the same.
- Existing callers (`usage`, `usage-daily`, `user files`) pass the **default** app's
  `project_id`. `chat-sessions` passes the **selected** entry's `project_id`.

## admin-web (Sessions page)

- New state: `apps: {key,name}[]` (from `/api/session-apps`), `selectedApp: string`
  (default = first app's key; optionally initialised from / synced to `?app=` for
  shareable links).
- A shadcn **`Select`** in the `PageHeader` actions, listing `apps`. Disabled with a
  "no chat applications configured" hint when `apps` is empty.
- `load()` fetches `/api/chat-sessions?app=${selectedApp}` (and refetches on change);
  the per-app stat strip + Live chat sessions card + the "Chat sessions" health row
  derive from that reply, exactly as today.
- The **Recent sign-ins** card heading becomes **"Recent sign-ins — all applications"**.
- One app today → a single-option Select still renders, making the scoping explicit.

## Config

- New optional env var **`MANAGER_CONTROL_APPS`** (admin-api), a JSON array of
  `{ key, name, controlUrl, projectId }`. Documented in `.env.local.example` next to
  `MANAGER_CONTROL_URL` with a one-entry example (commented out — the legacy var
  already covers today's single app via the fallback).
- `MANAGER_CONTROL_URL` is **retained** as the legacy single-app fallback; no
  existing deploy needs to change.

## Error handling

- No registry configured → `/api/session-apps` `{ apps: [] }`; the page disables the
  picker and shows "no chat applications configured" (mirrors today's `configured:false`).
- `?app=` unknown → **400** (fail closed); the page surfaces "unknown application".
- A selected app's manager unreachable → its chat panels show "manager unreachable"
  (the existing per-reply degrade in `combine_control_replies` is preserved).
- A malformed `MANAGER_CONTROL_APPS` entry is dropped at parse time with a log line;
  the app does not start with a half-built registry silently masking the bad entry.

## Security

- All new endpoints are `Operator`-gated like every other admin endpoint; no new
  auth surface.
- The registry's `controlUrl` / `projectId` are **never** sent to the client — only
  `{ key, name }`. The client passes back only the opaque `key`.
- Unknown `app` is rejected, never substituted (no cross-app data bleed).
- The SA still needs `chat.admin` on each app's project for that app's manager to
  accept `/control`; today it holds it on llm-chat. Granting it on future projects is
  a provisioning step, noted but out of scope.

## Testing

- **admin-api (pure units):** `parse_session_apps` — JSON path, legacy-fallback
  synthesis, empty case, and dropping an entry missing a field; `find_app` unknown →
  None; `session_apps_json` shaping (only key+name); `mint_chat_token` scope string
  for a given project id. The `/api/chat-sessions?app=` happy path + unknown-key 400
  are integration (gated like the existing admin IT).
- **admin-web (vitest):** the Sessions page renders the picker from a mocked
  `/api/session-apps`; changing the selection refetches `/api/chat-sessions` with the
  new `?app=`; the sign-ins heading reads "Recent sign-ins — all applications".

## Out of scope (YAGNI)

- Per-application filtering of Recent sign-ins (Zitadel audit log isn't project-scopable).
- Making usage / usage-daily / sandbox-tree endpoints app-selectable (default app only).
- Auto-provisioning the SA's `chat.admin` grant on additional projects.
- Discovering managers automatically from Zitadel projects (registry is explicit config).

## File-by-file change list

**admin-api**
- `src/config.rs` — add `SessionApp` + `parse_session_apps` + `session_apps: Vec<SessionApp>` on `AdminConfig`; read `MANAGER_CONTROL_APPS`, fall back to `MANAGER_CONTROL_URL` + `ZITADEL_PROJECT_ID`. **Remove** the standalone `manager_control_url` field — the registry's **default entry** (`default_app`) is now the single source for the non-selectable endpoints (its `control_url` + `project_id`).
- `src/zitadel/token.rs` — `mint_chat_token(project_id: &str)`.
- `src/api/mod.rs` — `GET /api/session-apps` (`session_apps_json` pure shaper); extend `chat_sessions` to resolve the entry from `?app=` (default/unknown handling) and mint per-entry; update `usage`/`usage-daily`/`user files` to pass the default entry's project id.
- `src/manager.rs` — unchanged (still `control_query` / `combine_control_replies`).

**admin-web**
- `app/(dash)/sessions/page.tsx` — fetch `/api/session-apps`, add the `Select`, thread `selectedApp` into the chat-sessions fetch, relabel the sign-ins card.
- `lib/types.ts` — `SessionAppList` (`{ apps: { key: string; name: string }[] }`).
- `components/ui/select.tsx` — reuse if present; add via shadcn CLI if missing.

**config**
- `.env.local.example` — document `MANAGER_CONTROL_APPS` (commented example) by `MANAGER_CONTROL_URL`.
