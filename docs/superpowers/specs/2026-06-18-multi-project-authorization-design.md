# Multi-application authorization (per-app roles + per-user assignment)

**Status:** proposed — awaiting review before implementation.
**Approved direction (user):** multi-project model + full feature, accepting the SA privilege change.

## 1. Goal

Let an operator manage **multiple applications**, where each application defines
**its own roles**, and assign **which users can use which application and with
what role**. Concretely the Console must answer and edit:

- *What applications exist?* (create / edit / delete)
- *What roles does each application have?* (per-application role CRUD)
- *Who can use application X, and as what role?* (grants: user × app × roles)

## 2. The model: application = Zitadel **Project**

In Zitadel, **roles and access grants belong to a Project, not to an OIDC App**
(an "App" is only a login client). So:

| User-facing concept | Zitadel object |
|---|---|
| **Application** (a product with its own roles) | **Project** |
| **Role in an application** | **Project role** (`roleKey`) |
| **"User U can use app A as role R"** | **User grant** `(userId, projectId, [roleKeys])` |
| **Login client** (web/native/API) under an app | **OIDC App** under that project |

The platform's own chat service stays bound to the existing **`llm-chat`**
project (the manager validates `chat.user` against that project's audience). New
applications/projects authorize **other** services the operator builds; chat is
just one application among them.

`cfg.project_id` stays as the *platform's home project* (chat audience,
capabilities, the operator's own grants). Everything the Console *manages*
becomes parameterized by `projectId`.

## 3. Security — the SA privilege change (explicit, least-privilege)

Today the runtime admin SA (`chat-admin-api`) is **PROJECT_OWNER on `llm-chat`
only** + ORG_USER_MANAGER (users/grants). Managing *many* projects requires more:

- **To create projects:** an org-level project-creation right. Zitadel exposes
  `ORG_PROJECT_CREATOR` (create projects) — **exact role name to be verified
  against the running instance during implementation (do not assume).**
- **To own each managed project's roles + apps:** PROJECT_OWNER on each managed
  project. Two options:
  - **(a) Provisioner adds the SA as PROJECT_OWNER on each project it creates**
    (the SA owns what it makes). Stays per-project; preferred.
  - **(b) Grant the SA a broader org role** that owns all projects — simpler but
    broader standing privilege. **Not preferred.**
- **Decision:** go with **ORG_PROJECT_CREATOR + (a)** — the SA can create
  projects and owns each one it creates, never blanket ORG_OWNER. This is a real
  privilege increase over today and is surfaced here deliberately (CLAUDE.md:
  never widen silently). It is gated behind this approved design.

`grant_role` for the SA on newly-created projects is applied with the **bootstrap
IAM_OWNER token** at create time (the SA can't grant itself ownership it lacks),
mirroring `assign_admin_project_member` in `provision.py`.

**Boundary invariants kept:** the operator gate (`chat.admin`) is unchanged; the
chat service's `chat.user` audience is unchanged; every new endpoint stays
behind the `Operator` extractor.

## 4. Backend (admin-api) — generalize to multi-project

The Zitadel client methods that hardcode `self.cfg.project_id` gain a
`project_id: &str` parameter; new methods are added. Files: `project.rs`,
`grants.rs`, `apps.rs`, `users.rs` (grants).

**New / changed Zitadel client methods**
- `list_projects()` → POST `/management/v1/projects/_search`
- `create_project(name, settings)` → POST `/management/v1/projects`
- `get_project(pid)` / `update_project(pid, …)` / `delete_project(pid)`
- `list_roles(pid)` (was no-arg), `add_role(pid, key, name, group)`,
  `delete_role(pid, key)`
- `list_user_grants(uid)` already returns all projects' grants — keep; ensure it
  carries `projectId` per grant (it does). `add_grant(uid, pid, roles)`,
  `update_grant(uid, grantId, roles)`, `remove_grant(uid, grantId)` — add `pid`.
- `list_apps(pid)`, `create_oidc_app(pid, …)`, etc. — add `pid`.
- Project members: `list_project_members(pid)` / grant-side for the per-app
  roster (or derive the roster from user-grants search — see §5).

**New / changed HTTP routes** (all `Operator`-gated)
- `GET/POST /api/projects`, `GET/PUT/DELETE /api/projects/{pid}`
- `GET/POST /api/projects/{pid}/roles`, `DELETE /api/projects/{pid}/roles/{key}`
- `GET/POST /api/projects/{pid}/apps`, … (move today's `/api/apps` under a project)
- `GET /api/projects/{pid}/grants` (the app's user roster — who holds what)
- Grants on a user: `POST /api/users/{id}/grants` gains `projectId`;
  `PUT/DELETE /api/users/{id}/grants/{grantId}` unchanged (grantId is global).
- Keep `/api/roles`, `/api/apps` as thin aliases for the home project during a
  transition, or redirect callers — TBD to avoid breaking the current pages
  mid-migration.

**Config:** `project_id` stays (home project). No new required env. Multi-project
discovery is dynamic (list_projects).

## 5. Frontend (admin-web) — surfaces

1. **Applications page** (`/apps` reframed, or a new `/applications`): a table of
   **projects** — name, #roles, #apps(clients), #users. Create / edit / delete.
2. **Application detail** (`/applications/{pid}`): tabs/sections —
   - **Roles**: the project's roles (today's Roles page, scoped).
   - **Login clients**: the OIDC apps (today's Apps list, scoped).
   - **Users**: roster — who has access + their roles here (per-app assignment).
3. **Per-user access matrix**: generalize the existing *Access (grants)* dialog
   to span **all** applications — for each app the user can use, which roles.
   Powers the Users table **"App access & roles"** column (the mockup's column).
4. **Users table:** the `Roles` column → **"App access & roles"** (chips grouped
   by application), driven by the user's grants across projects.

Reuse: the grants plumbing, the role chips, `DataTable`, `DetailPanel`, the
create/secret-reveal dialogs.

## 6. Phasing (each phase ships + is verified independently)

- **P1 — Read + assign (the literal ask).** Multi-project *read* (list projects,
  their roles, their apps) + the **assignment** surface (per-user access matrix
  across apps, and the per-app user roster). No create/edit yet. Backend: add
  `project_id` params + list_projects; reuse existing grant write.
- **P2 — Manage applications.** Create/edit/delete projects + per-project role
  CRUD + move OIDC clients under a project. Backend: SA privilege change
  (provisioner: ORG_PROJECT_CREATOR + own-on-create) + the create endpoints.
- **P3 — Polish.** "App access & roles" column, per-app roster view, audit of
  grant changes, empty/edge states.

## 7. Risks / open decisions

- **Exact Zitadel role for project creation** (`ORG_PROJECT_CREATOR`?) — verify
  live before wiring the provisioner; fail closed if the grant can't be proven.
- **Migration of `/api/roles` & `/api/apps`** (single-project) without breaking
  the current Roles/Apps pages — alias to the home project during transition.
- **e2e/test debt:** the current suite already lags new UI (filter toggle, status
  badge). Multi-project routes need new tests; existing ones updated, not weakened.
- **Scope:** "full feature" is multi-day; delivered in the 3 reviewed phases above.

## 8. Out of scope (for now)
- Cross-org / multi-tenant projects (single org assumed).
- Custom OIDC grant types beyond what `/apps` already supports.
- Changing the chat service's binding to the `llm-chat` project.
