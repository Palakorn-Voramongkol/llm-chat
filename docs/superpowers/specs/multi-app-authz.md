# Multi-application authorization

**Status:** implemented. Historical design record; for the live system see
`docs/architecture.md`.

## Goal

Let an operator manage **multiple applications**, where each application defines
**its own roles**, and assign **which users can use which application and with
what role**. The Console answers and edits:

- *What applications exist?*
- *What roles does each application have?* (per-application role CRUD)
- *Who can use application X, and as what role?* (grants: user × app × roles)

## The model: application = Zitadel **Project**

In Zitadel, **roles and access grants belong to a Project, not to an OIDC App**
(an "App" is only a login client). So:

| User-facing concept | Zitadel object |
|---|---|
| **Application** (a product with its own roles) | **Project** |
| **Role in an application** | **Project role** (`roleKey`) |
| **"User U can use app A as role R"** | **User grant** `(userId, projectId, [roleKeys])` |
| **Login client** (web/native/API) under an app | **OIDC App** under that project |

The platform's own chat service stays bound to the existing **`llm-chat`**
project (the manager validates `chat.user` against that project's audience).
`cfg.project_id` is the *home project* (chat audience, the operator's own
grants); everything the Console *manages* is parameterized by `projectId`. New
projects authorize **other** services the operator builds; chat is one
application among them.

## Security — least privilege

The runtime admin SA (`chat-admin-api`) never holds standing org-wide project
rights (never `ORG_OWNER`). Its grants:

- Org level: `ORG_USER_MANAGER` (users + grants) and `ORG_SETTINGS_MANAGER`
  (org rename — minimal role granting `org.write`).
- Per project: `PROJECT_OWNER` on each managed project, which is what permits
  per-project role CRUD and grants. Zitadel returns 403 without it.

The SA cannot create projects or grant itself ownership it lacks. A **new
application is provisioned out of band** by `deploy/compose/provisioner/new_app.py`:
using the **bootstrap IAM_OWNER key**, it creates the project, assigns the SA
`PROJECT_OWNER` on it (same `assign_admin_project_member` path as the home
project in `provision.py`), and seeds optional initial roles. After that the
operator manages the app entirely from the Console (`/applications/<pid>`) with
no further privilege. This is the least-privilege runbook to add an application.

**Boundary invariants:** the operator gate (`chat.admin`) and the chat
service's `chat.user` audience are unchanged; every `/api/*` endpoint stays
behind the `Operator` extractor.

## Backend (admin-api)

Zitadel client methods that target the home project keep a thin alias and gain a
`*_for` / `*_in` / `*_to` variant taking `project_id` (`admin-api/src/zitadel/`):

- Projects (`project.rs`): `list_projects()` → POST `/projects/_search`;
  `get_project()` / `update_project()` on the home project.
- Roles (`grants.rs`): `list_roles_for(pid)`, `create_role_in(pid,…)`,
  `update_role_in(pid,…)` (renames displayName + group; `roleKey` is the
  immutable id), `delete_role_in(pid,…)` (cascades — strips the role from every
  grant on that project).
- Grants (`grants.rs`): `add_grant_to(uid, pid, roleKeys)`; `set_grant_roles`
  PUT **replaces** the whole `roleKeys` set (so "remove one role" is
  read-modify-write via `roles_without`); `remove_grant`; `list_user_grants`
  (all projects) and `list_project_grants(pid)` (one app's roster). Search
  returns the grant id as `id`/`userGrantId`; `normalize_grant_id` maps it to
  `grantId` so PUT/DELETE target it.

**HTTP routes** (`admin-api/src/api/mod.rs`, all `Operator`-gated):

- `GET /api/projects` — list applications. **Filters out** Zitadel's reserved
  internal `ZITADEL` project (not a user-facing app).
- `GET/POST /api/projects/{pid}/roles`, `PUT/DELETE /api/projects/{pid}/roles/{roleKey}`
- `GET /api/projects/{pid}/apps` — the app's login clients.
- `GET /api/projects/{pid}/grants` — the app's user roster.
- `POST /api/users/{id}/grants` accepts an optional `projectId` (omitted ⇒ home
  project); `PUT/DELETE /api/users/{id}/grants/{grantId}` unchanged (grantId is
  global).
- Home-project aliases kept: `/api/roles`, `/api/apps`, `/api/project`.

There is **no** create/delete-project endpoint on the Console — that path is the
provisioner's IAM_OWNER runbook above, by design (the SA can't own what it
didn't make).

## Frontend (admin-web) — surfaces

- **Applications** list: projects with name, #roles, #login clients, #users.
- **Application detail** (`/applications/{pid}`): Roles, Login clients, and
  Users (roster — who has access and their roles here).
- **Per-user access matrix**: the *Access (grants)* dialog spans all
  applications; powers the Users table **"App access & roles"** column (chips
  grouped by application, from the user's grants across projects).

## Phasing (delivered)

- **P1 — Read + assign.** Multi-project read (projects, their roles + login
  clients) and the assignment surfaces (per-user access matrix, per-app roster).
- **P2 — Manage applications.** Per-project role CRUD from the Console; new
  projects added via the `new_app.py` provisioner runbook (PROJECT_OWNER per
  project), not from the Console.
- **P3 — Polish.** "App access & roles" column, per-app roster view, edge states.

## Out of scope

- Cross-org / multi-tenant projects (single org assumed).
- Custom OIDC grant types beyond what `/apps` supports.
- Changing the chat service's binding to the `llm-chat` project.
