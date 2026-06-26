# Applications ↔ OIDC login clients — nested IA redesign

**Status:** designed 2026-06-26; not yet implemented.
**Area:** admin-web (operator Console) + admin-api (BFF).
**Supersedes the relevant UI parts of:** `multi-app-authz.md` (read-only login
clients on the app detail) and the original `platform-console.md` `/apps` page.

## Problem

The Console presents OIDC login clients and the applications that own them as two
**disconnected, equally-named** top-level surfaces, so an operator cannot see the
parent→child relationship:

- **`/applications`** (nav: "Applications", `Boxes` icon) lists Zitadel
  **projects** (`GET /api/projects`). Each is an application with its own roles,
  users, and login clients. Its detail page shows login clients **read-only**.
- **`/apps`** (nav: "OIDC Clients", `AppWindow` icon) lists `GET /api/apps` —
  the OIDC clients of the **home/platform project only** — with full CRUD.

Concrete symptoms:

- Both page components are literally named `ApplicationsPage` and both render the
  heading **"Applications"**.
- The home project's clients appear in **both** places (CRUD in `/apps`,
  read-only in `/applications/<home-pid>`).
- There is **no link** from an application to its clients, or from a client back
  to its application. The relationship is invisible.
- The backend cements the split: per-project client CRUD is **hardcoded to the
  home project** — `create_oidc_app` / `get_app` / `update_oidc_config` /
  `regenerate_app_secret` / `delete_app` all use `self.cfg.project_id`. Only
  `list_apps_for(pid)` (and the route `GET /api/projects/{pid}/apps`) is
  project-parameterized, which is why other apps' clients are list-only.

## Goal

Make OIDC login clients a **managed child of each Application**. One mental model:
an Application (a Zitadel project) contains **roles + login clients + users**, and
you reach a client by opening its application. Retire the standalone clients page.

## Domain model (unchanged, from `multi-app-authz.md`)

| User-facing concept | Zitadel object |
|---|---|
| **Application** (a product with its own roles) | **Project** |
| **Login client** (web / native / SPA) under an app | **OIDC App** under that project |
| **Role in an application** | Project role (`roleKey`) |
| **"User U can use app A as role R"** | User grant `(userId, projectId, [roleKeys])` |

`cfg.project_id` stays the *home project* (the chat audience). The home project is
included in `GET /api/projects` (only the reserved internal `ZITADEL` project is
filtered out), so it becomes one Application among others, and its clients become
manageable on its detail page like any other app's.

## Decisions (validated with the operator)

1. **Nest clients under each Application.** Remove the standalone "OIDC Clients"
   nav item; clients are managed on the application detail page.
2. **Application detail layout = master–detail for clients + secondary cards.**
   Login clients are the primary surface (selectable list + right-hand detail
   panel, full CRUD); **Roles** and **Users** are compact cards below it.
3. **No project Edit/Delete in the app header.** Per-project create / rename /
   delete is not a Console power by design (the `new_app.py` IAM_OWNER runbook
   owns project lifecycle; the SA cannot own what it didn't make). Home-project
   rename stays on the **Project & Org** page. The header is identity-only.
4. **Surface, don't hide, missing ownership.** Client CRUD on a project the SA
   doesn't own returns Zitadel 403; the UI shows a clear error and does **not**
   fall back (fail-closed). Buttons stay visible — attempt then surface.

## Backend changes (admin-api)

### `src/zitadel/apps.rs` — project-scope the CRUD

Add `*_in(project_id, …)` variants mirroring the existing `list_apps_for`, and
turn the current home-project methods into thin aliases that delegate with
`self.cfg.project_id` (exactly the pattern `list_apps` → `list_apps_for` already
uses). The pure body builders (`oidc_create_body`, `oidc_update_body`) are
unchanged — only the URL gains the caller-supplied `pid`.

- `create_oidc_app_in(pid, name, redirect_uris, response_types, grant_types, app_type, auth_method)`
  → `POST /management/v1/projects/{pid}/apps/oidc`
- `get_app_in(pid, app_id)` → `GET …/projects/{pid}/apps/{app_id}`
- `update_oidc_config_in(pid, app_id, …)` → `PUT …/apps/{app_id}/oidc_config`
- `regenerate_app_secret_in(pid, app_id)` → `POST …/apps/{app_id}/oidc_config/_generate_client_secret`
- `delete_app_in(pid, app_id)` → `DELETE …/apps/{app_id}`
- Keep `create_oidc_app` / `get_app` / `update_oidc_config` /
  `regenerate_app_secret` / `delete_app` as aliases delegating to the `_in`
  variants with `&self.cfg.project_id`.

The `clientSecret` one-time-reveal invariant is preserved: create + regenerate
return the full response streamed straight through, never logged.

### `src/api/mod.rs` — project-scoped routes

Add, all `Operator`-gated, next to the existing `GET /api/projects/{pid}/apps`:

- `POST   /api/projects/{pid}/apps` → `create_project_app`
- `GET    /api/projects/{pid}/apps/{appId}` → `get_project_app`
- `PUT    /api/projects/{pid}/apps/{appId}` → `update_project_app` (oidc_config)
- `DELETE /api/projects/{pid}/apps/{appId}` → `delete_project_app`
- `POST   /api/projects/{pid}/apps/{appId}/secret` → `regenerate_project_app_secret`

Request bodies reuse the existing `CreateOidcApp` / `UpdateOidcConfig` structs.
Keep the home-project aliases (`/api/apps`, `/api/apps/{appId}`,
`/api/apps/{appId}/secret`) for back-compat; the frontend stops using them.

### Security / fail-closed (unchanged guarantees)

- Every new handler takes the `Operator` extractor (chat.admin) — same gate.
- `pid` and `appId` arrive via axum `Path` (decoded) and pass through the
  centralized `path_has_traversal` check in `send_json` before the privileged SA
  call (rejects `.`/`..`/`?`/`#`), so a smuggled segment can't redirect the SA
  request. No new bypass.
- Per-project CRUD requires `PROJECT_OWNER` on `pid`; Zitadel returns 403, mapped
  by `From<ZitadelError>` to HTTP 403. No fallback to the home project.

## Frontend changes (admin-web)

### `app/(dash)/applications/[id]/page.tsx` — rebuild as master–detail

- **Header:** keep the existing back-link/breadcrumb (`Applications`) and
  `PageHeader` (app name + id/state). No Edit/Delete on the project.
- **Primary — Login clients (master–detail):**
  - Left: a **selectable list** (not the full `DataTable` filter/group/density
    toolbar — match the approved mockup and the existing detail-page card style)
    of `OidcApp[]` from `GET /api/projects/{id}/apps`, with a `+ New client`
    action and per-row selection. Reuse `appTypeLabel` / the type chips from
    `components/apps/columns.tsx`.
  - Right: `DetailPanel` showing the selected client's OIDC config (clientId,
    app type, auth method, grant/response types, redirect URIs) — the same panel
    content currently in `app/(dash)/apps/page.tsx` — with actions **Edit**,
    **Rotate secret**, **Delete**.
  - Wiring (project-scoped):
    - New / Edit → `AppFormDialog` (see below) against
      `/api/projects/{id}/apps` and `/api/projects/{id}/apps/{appId}`.
    - Rotate → `POST /api/projects/{id}/apps/{appId}/secret` →
      `SecretRevealDialog` (one-time reveal).
    - Delete → `DELETE /api/projects/{id}/apps/{appId}` → `ConfirmDialog`.
- **Secondary — Roles & Users (cards below):** keep the existing manageable
  **Roles** card (add/edit/delete via the project-scoped role endpoints already
  in place) and the read-only **Users** roster card. No behavior change.
- **Data flow / error handling:** keep the established per-tile best-effort
  fan-out — clients, roles, grants, and the project name each load independently;
  a failure degrades that section only. After any client mutation, reload the
  clients list. A 403 (no `PROJECT_OWNER`) surfaces as a `toast.error` with a
  clear message; no silent fallback.

### `components/apps/app-form-dialog.tsx` — parametrize the endpoint

Add a required `projectId: string` prop and build the endpoint from it:
`POST /api/projects/{projectId}/apps` (create) and
`PUT /api/projects/{projectId}/apps/{app.id}` (edit). Update the dialog copy from
"application" to **"login client"** ("Register login client" / "Edit login
client") so the term matches the new hierarchy. No other logic change
(`lib/oidc.ts` form mappers are reused as-is).

### `app/(dash)/applications/page.tsx` + `components/applications/columns.tsx`

- Extend `AppMeta` with `clientCount: number`.
- In the page's per-app `Promise.all`, also call `GET /api/projects/{id}/apps`
  and set `clientCount = (apps.result ?? []).length` (best-effort, same try/catch
  that already wraps roles+grants).
- Add a **Clients** column to `buildApplicationColumns` (count, alongside Roles
  and Users), with a `meta.description`.

### `components/shell/nav.ts` — remove the standalone clients item

Delete the `{ label: "OIDC Clients", href: "/apps", … }` entry. `Applications`
stays. `GlobalSearch` iterates `NAV` for the Pages group and already routes
Applications/Roles to `/applications/{id}`, so the stale "OIDC Clients" page entry
disappears automatically — no GlobalSearch change required.

### `app/(dash)/apps/page.tsx` — convert to a redirect

Replace the page body with a small client component that fetches `GET /api/project`
(the home project, which includes `id`) and `router.replace('/applications/' +
id)`; on failure, `router.replace('/applications')`. Preserves existing bookmarks
and any in-app links to `/apps`. The duplicate `ApplicationsPage` component name
and "Applications" heading are removed in the process.

## Testing

**admin-api:**
- Contract tests: the new route handlers reuse `CreateOidcApp` / `UpdateOidcConfig`
  — covered by existing camelCase tests; add a `create_project_app` /
  `update_project_app` path-extraction test if a new struct is introduced.
- Gating: add `*_requires_operator` tests for the new project-scoped routes,
  mirroring `usage_route_requires_operator` (401 without a session).
- Optional integration (behind the existing `ADMIN_IT` gate): a project-scoped
  analog of `it_verify_oidc_config_put_and_secret_regen`.

**admin-web:**
- vitest: extend the applications columns test for the new **Clients** column;
  cover `clientCount` mapping.
- Playwright (gated suite): update `e2e/shell.spec.ts` for the removed nav item;
  add an app-detail clients happy-path (create → reveal secret → edit → delete)
  if the gated stack is available.

## Out of scope

- Per-project create / rename / delete of the Application (project) itself — stays
  the `new_app.py` provisioner runbook.
- Surfacing the human app-code (`app_codes.json`) in the Console — it is a
  provisioner secret mapping, not exposed by the projects API today.
- Changing the chat service's binding to the `llm-chat` project.
- A global cross-app "all clients" inventory view (the rejected option C-plus).

## Migration / compatibility

- `/apps` → redirect into the home application's detail; bookmarks keep working.
- Home-project app aliases (`/api/apps…`) retained server-side; only the frontend
  switches to the project-scoped routes.
- No data migration — purely routing + UI + additive backend endpoints.

## File-by-file change list

**admin-api**
- `src/zitadel/apps.rs` — add 5 `*_in` methods; make 5 existing methods aliases.
- `src/api/mod.rs` — add 5 project-scoped app routes + handlers; keep aliases.

**admin-web**
- `components/shell/nav.ts` — remove the OIDC Clients entry.
- `app/(dash)/applications/[id]/page.tsx` — rebuild (clients master–detail +
  Roles/Users cards + CRUD wiring).
- `components/apps/app-form-dialog.tsx` — add `projectId` prop; project-scoped
  endpoints; "login client" copy.
- `app/(dash)/applications/page.tsx` — fetch clients count into `AppMeta`.
- `components/applications/columns.tsx` — `clientCount` + Clients column.
- `app/(dash)/apps/page.tsx` — replace with a redirect to `/applications/<home-pid>`.
- Tests: applications columns vitest; `e2e/shell.spec.ts` nav; optional app-detail
  clients e2e.
