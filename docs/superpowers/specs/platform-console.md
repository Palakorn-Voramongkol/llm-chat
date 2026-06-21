# Platform-management Console

> **Status:** Implemented as `admin-web` + `admin-api`. This is the historical
> design record; for the current system see [docs/architecture.md](../../architecture.md).

**Goal:** A professional operator Console for the Zitadel-backed platform —
Users, Roles & Grants, Applications, OIDC clients, Project & Org settings,
Sessions/workers monitoring, Audit, and a Dashboard — behind the existing
`chat.admin` operator gate.

**Architecture (one sentence):** A Next.js 16 / React 19 SPA (`admin-web`,
`:3000`) under one `(dash)` route group, fronted by the Rust **axum admin-api**
(`:7676`) that is the *only* caller of Zitadel; every area reuses three fixed
layers — shared shell + thin pages, an `Operator`-gated handler, and a
`ZitadelClient` method.

**Authorization model: an "App" = a Zitadel Project.** Each app owns its own
role catalog and login clients; a user's access to an app is a **user grant** =
`(user, project, roles[])`, and one user can hold grants across many apps. The
original single project (roles `chat.user`/`chat.admin`/`chat.app`, clients
CLI/Lumina/admin) is the first app ("Chat"). Roles are per-project, never
per-client.

---

## 1. Architecture — three reused layers

Every area adds code at the **same three layers**:

1. **Frontend (`admin-web`).** All pages live under `app/(dash)/`, sharing the
   one shell file `app/(dash)/layout.tsx` (a `'use client'` Sidebar + Topbar
   with `{children}` as the page slot). Each page mirrors `users/page.tsx`: a
   `useCallback load()` doing `api.get` into `useState`, called on mount and
   after every mutation; mutations call `api.post/patch/put/del` then
   `toast.success` + reload; `catch => toast.error(...)`; 401 is swallowed
   because `lib/api.ts` full-page-redirects to `/login`. Lists render through
   the shared TanStack `components/ui/data-table.tsx`.
2. **Backend handler (`admin-api`).** A thin axum handler behind the `Operator`
   extractor (`src/session.rs`, fail-closed: 401 no session, 403 lacking
   `chat.admin`), added to the router in `src/api/mod.rs`, returning Zitadel
   JSON passed through (camelCase preserved).
3. **`ZitadelClient` method.** A `pub async fn` in `src/zitadel/*.rs` over the
   existing `post/put/get/delete` + `valid_token()` JWT-bearer helpers.

**Single source of nav truth:** a typed `NAV` array (icon, label, href, match)
in `components/shell/nav.ts`, consumed by the Sidebar. Adding an area = append
one `NAV` entry + one `page.tsx`.

## 2. Security model

- **Operator gate (fail-closed):** every `/api/*` route runs behind the
  `Operator` extractor; the operator's JWT must carry `chat.admin` or 403. No
  relaxation. admin-web never talks to Zitadel directly. To reach the manager's
  `chat.admin` `/control` for live monitoring, admin-api mints its own
  project-audience `chat.admin` token.
- **SA least privilege (NOT `ORG_OWNER`).** The runtime admin-api SA key is
  long-lived and persisted to `./secrets`, so it must not hold standing
  `ORG_OWNER`. It gets two scoped grants: `ORG_USER_MANAGER` (org-level: users +
  grants) and `PROJECT_OWNER` on the `llm-chat` project (apps + roles).
- **Org policies are read-only at runtime.** Policy *writes* require `ORG_OWNER`,
  which the SA deliberately lacks; they are made by the one-time provisioner with
  the bootstrap `IAM_OWNER` token (§7). The Console's policy view is read-only,
  capability-probed (shows "managed out-of-band" if even reads fail).
- **Audit needs `IAM_OWNER_VIEWER` (instance-level)**, which `ORG_OWNER` does not
  include. Audit is **capability-gated** (§8): shipped but shown as unavailable
  until that instance grant is added — a separate, explicit decision, since it is
  instance-wide and reads all orgs' events.
- **Secret one-time-reveal invariant:** OIDC client secrets and machine keys are
  returned once (on create/regenerate) and never readable again. Stream the
  Zitadel `Value` straight through the BFF untouched; never log it; reveal once
  in the UI with an explicit "copy now, won't be shown again" affordance.
- **Self-lockout danger:** deleting the `chat.admin` role or revoking one's own
  `chat.admin` grant locks the operator out. Confirm dialogs must state the
  consequence; guard against deleting `chat.admin` while it is the operator's
  only admin grant.

## 3. Shared shell & reusable patterns

- **`app/(dash)/layout.tsx`** + **`components/shell/`**: `Sidebar.tsx`,
  `Topbar.tsx`, `NavLink.tsx` (active-aware via `usePathname`),
  `OperatorBadge.tsx` (fetches `/api/me`, shows name/avatar), plus
  `GlobalSearch.tsx`, `NotificationBell.tsx`, `ThemeToggle.tsx`, `PageHeader.tsx`.
  The Topbar shows a `Console / <area>` breadcrumb, global search, notification
  bell, theme toggle, and the operator badge. Render chrome immediately; never
  block on a fetch.
- **Canonical page shape:** page header + toolbar (primary action) + `DataTable`
  + create/edit `Dialog` (react-hook-form + `zodResolver`) + lifecycle
  `DropdownMenu` + `AlertDialog` ConfirmDialog. Every area copies this.
- **Next.js 16 grounding (mandatory):** `admin-web/AGENTS.md` requires reading
  `node_modules/next/dist/docs/` before routing code. Dynamic route params are
  **async** (`params: Promise<…>`); route-group layouts add no URL segment.

## 4. Pages

The shipped nav (`components/shell/nav.ts`), in order:

| Page | Route | Notes |
|---|---|---|
| **Dashboard** | `/dashboard` | landing page; `(dash)/page.tsx` redirects `/` here |
| **Users** | `/users` | canonical list + create/edit + lifecycle + per-user Access (grants) |
| **Roles** | `/roles` | project role catalog + holders + create/rename |
| **Applications** | `/applications` | apps as projects; detail at `/applications/[id]` |
| **OIDC Clients** | `/apps` | OIDC client CRUD within an app |
| **Sessions** | `/sessions` | live platform monitoring (proxies manager `/control`) |
| **Project & Org** | `/settings` | editable project card + read-only policy cards |
| **Audit** | `/audit` | capability-gated event log (§8) |

### Users
Canonical *list + create/edit + lifecycle + confirm* page that every other area
mirrors. Per-user **Access (grants)** affordance opens the Grants UI (§5).

### Roles & Grants
Routes: `GET/POST /api/roles`, `DELETE /api/roles/{roleKey}` (cascades — strips
the role from every grant), `GET /api/roles/{roleKey}/holders`, plus the grants
endpoints (`users/grants/_search`, add/set/remove). UI: a Roles DataTable with
create/rename + holders + cascade-warning confirm, and a Grants UI in the user
detail (checkbox multiselect of roles, grouped by app/project).

**One-grant-per-project invariant:** Zitadel allows one user-grant per
(user, project). Assign/revoke branches: **POST** to create if none exists,
**PUT** to replace the whole `roleKeys` set if it exists, **DELETE** to revoke
all. `roleKey` path params must be `encodeURIComponent`'d.

### Applications & OIDC clients
Per the App=Project model, an app's roles live on the project (Roles) and its
access on user grants. The **Applications** page lists/edits apps (projects); the
**OIDC Clients** page (`/apps`) is the OIDC *client* CRUD within an app. Routes:
`GET/POST /api/apps`, `GET/PUT /api/apps/{appId}`, `DELETE /api/apps/{appId}`,
`POST /api/apps/{appId}/secret`. UI: list → detail/edit dialog (redirectUris,
grantTypes, responseTypes, appType, authMethod), **create with one-time secret
reveal**, **rotate secret** (one-time reveal + "breaks clients on the old
secret" confirm). Edit is read-modify-write of the full `oidc_config`. Confirm:
"changing redirectUris can instantly break a live login."

### Project & Org settings
`GET/PUT /api/project` (editable, `PROJECT_OWNER`) plus read-only policy reads
(`login`, `password-complexity`, `lockout`). UI: a Settings page of cards — an
editable **Project** card and three **read-only policy cards** noting that policy
changes are provisioner-managed. The login policy's `mfaInitSkipLifetime` (the
2FA-setup nudge that interrupted the demo login) is set by the provisioner on
boot, not edited live. Note: protobuf `Duration` serializes as a string (e.g.
`"0s"`).

### Sessions
Live platform-wide monitoring (all users, not just the operator): active chat
sessions, users chatting now, workers online, recent sign-ins. Each panel
degrades on its own failure. Data comes from admin-api endpoints
(`/api/status`, `/api/chat-sessions`, `/api/signins`) that proxy the manager's
`chat.admin` `/control` surface.

### Dashboard
`GET /api/stats` fans out `totalResult` counts from existing `_search` endpoints
(users by type, roles, grants, apps) + a `valid_token()` health self-check. UI:
colorful stat cards that deep-link into their section. Note:
`details.totalResult` may serialize as a JSON string — the count helper tries
`as_u64` **and** `as_str().parse`, or counts silently read 0.

## 5. Service-account permission bump (provisioner)

`deploy/compose/provisioner/provision.py`: `assign_admin_member` grants
`ORG_USER_MANAGER` (`POST .../orgs/me/members`); `assign_admin_project_member`
grants `PROJECT_OWNER` (`POST /management/v1/projects/{pid}/members`, idempotent —
409 == already a member). For a live instance the project grant is applied via a
one-shot member POST/PUT with the bootstrap `IAM_OWNER` key. `test_provision.py`
asserts the scoped roles (`ORG_USER_MANAGER` + `PROJECT_OWNER`).

## 6. Error handling & states

`ApiError` → toast with the structured Zitadel message; 403s carry the missing
scope/role. Each Dashboard/Sessions card degrades to `null`/em-dash on its own
failure (one failing count never blanks the page). Central 401 → `/login`.
DataTable empty states use a parameterized `emptyMessage` prop (not a hardcoded
string).

## 7. Audit (capability-gated)

`GET /api/events` over `POST /admin/v1/events/_search` (sequence-cursor paging) +
a capabilities probe. The event API needs `IAM_OWNER_VIEWER` (instance), not
`ORG_OWNER`, so the page renders a **fail-closed capability banner** — if
`can_read_events` is false, it shows "Audit requires IAM_OWNER_VIEWER on the
service account" instead of erroring. When enabled, results are confined by
`resourceOwner` to the SA's org (the instance log is instance-wide; don't leak
other orgs). Enabling the grant remains a follow-up decision.

## 8. Testing

- **Backend:** camelCase contract tests on each handler's JSON; set-math tests
  for the grant POST/PUT/DELETE branch; `ADMIN_IT`-gated live tests for the
  policy-upsert error code and the OIDC `oidc_config`/secret endpoints.
- **Frontend:** `admin-web/e2e/smoke.spec.ts` per area — Roles create/delete
  (+cascade confirm), App create with secret-reveal-once assertion, a grant
  assign/revoke round-trip, Dashboard counts render — all reusing the existing
  `ADMIN_IT=1` operator login.
