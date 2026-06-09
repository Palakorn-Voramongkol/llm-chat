# Platform-Management Console — Design Spec

**Status:** Draft for review · **Date:** 2026-06-09 · **Branch:** feat/zitadel-admin

**Goal:** Turn the single-screen admin-web into a professional, easy-to-use
**platform-management Console** for the Zitadel-backed platform — managing
Users, Roles, Applications, Project & Org settings, a Dashboard, and (capability
permitting) an Audit log — behind the existing `chat.admin` operator gate.

**Architecture (one sentence):** A Next.js 16 / React 19 SPA under one `(dash)`
route group, fronted by the Rust **axum admin-api** that is the *only* caller of
Zitadel; every area reuses three fixed layers — a shared shell + thin pages, an
`Operator`-gated handler, and a `ZitadelClient` method — and never invents new
patterns.

**Visual baseline (approved):** VS Code-style **activity-bar ribbon** + side
panel + light, colorful theme. Rendered mockup:
`docs/superpowers/specs/mockups/console-shell.html` (→ `console-shell.png`).

---

## 1. Overview & Goals

v1 ships, in this order: **Shell**, **Users** (refactor), **Roles & Grants**,
**OIDC Applications**, **Project & Org settings**, **Dashboard**. **Audit** ships
last and **capability-gated** — see §3/§11; it cannot function under `ORG_OWNER`
alone.

**Authorization model (approved): an "App" = a Zitadel Project.** Each app owns
its own role catalog and its login clients; a user's access to an app is a
**user grant** = `(user, project, roles[])`; one user can hold grants across many
apps. The current single project (roles `chat.user`/`chat.admin`/`chat.app`,
clients CLI/Lumina/admin) becomes the first app ("Chat"). Roles are per-project,
never per-client; clients of one app share that app's roles.

Non-goals for v1: multi-org management, IdP/SSO federation config, Zitadel
Actions, SAML/API-app types beyond what already exists, and any change to the
chat data path.

## 2. Architecture — three reused layers

Every area adds code at the **same three layers**; it does not introduce new
shapes.

1. **Frontend (admin-web).** All pages live under `app/(dash)/` sharing the root
   `app/layout.tsx` (already mounts `<Toaster/>` + next-themes). The net-new
   foundation is **one** file, `app/(dash)/layout.tsx` — a `'use client'` shell
   (uses `usePathname` for active nav) rendering a persistent **Sidebar** +
   **Topbar** with `{children}` as the page slot. Each area page is a thin client
   component that mirrors `users/page.tsx` **exactly**: a `useCallback load()`
   doing `api.get` into `useState`, called on mount and after every mutation;
   mutations call `api.post/patch/put/del` then `toast.success` + reload; `catch
   => toast.error(e instanceof ApiError ? e.message : 'fallback')`; 401 is
   swallowed because `lib/api.ts` already full-page-redirects to `/login`. Lists
   render through the shared TanStack `components/ui/data-table.tsx`.
2. **Backend handler (admin-api).** A thin axum handler behind the `Operator`
   extractor (`src/session.rs` — fails closed: 401 no session, 403 lacking
   `chat.admin`), added to the router in `src/api/mod.rs`, returning the
   Zitadel JSON passed through (camelCase preserved).
3. **ZitadelClient method.** A `pub async fn` in a `src/zitadel/*.rs` impl module
   over the existing `post/put/get/delete` + `valid_token()` JWT-bearer helpers,
   wired into `zitadel/mod.rs`.

**Single source of nav truth:** a typed `NAV` array (icon, label, href, match)
consumed by the Sidebar; adding an area = append one `NAV` entry + one
`page.tsx`. Nav icons (lucide, already a dep): `LayoutDashboard, Users,
ShieldCheck, AppWindow, Building2, ScrollText`.

## 3. Security Model

- **Operator gate (unchanged, fail-closed):** every `/api/*` route runs behind
  the `Operator` extractor; `/callback` requires `chat.admin` or 403. No relaxation.
- **SA scope → least privilege (revised after security review; NOT `ORG_OWNER`).**
  The runtime admin-api SA is long-lived and its key is persisted to `./secrets`,
  so it must **not** hold standing `ORG_OWNER` (that key becomes an org-wide
  escalation target). Instead it gets exactly two scoped grants: `ORG_USER_MANAGER`
  at the org level (users + grants) and `PROJECT_OWNER` on the `llm-chat` project
  (apps + roles). **Org policies are NOT writable by the runtime SA** — they are
  written by the one-time provisioner with the bootstrap `IAM_OWNER` token (§9),
  and the Console's policy view is read-only. Still defense-in-depth: every action
  is also gated behind a human `chat.admin` operator session, and the SA key never
  leaves the BFF.
- **Audit needs MORE than ORG_OWNER (blocker — see §11).** The event log
  (`/admin/v1/events/_search`) requires **`IAM_OWNER_VIEWER`** (instance-level),
  which `ORG_OWNER` does not include. Audit is therefore **capability-gated**:
  shipped but shown as "unavailable — requires IAM_OWNER_VIEWER" until that
  instance grant is added. Enabling it is a separate, explicit decision (it is
  instance-wide and reads all orgs' events).
- **Secret one-time-reveal invariant:** OIDC client secrets and machine keys are
  returned **once** (on create / regenerate) and never readable again. Stream the
  Zitadel `Value` straight through the BFF untouched (like `create_key`); never
  log/trace it; reveal once in the UI with an explicit "copy now, won't be shown
  again" affordance.
- **Self-lockout danger:** deleting the `chat.admin` role or revoking one's own
  `chat.admin` grant locks the operator out of admin-web. Confirm dialogs for
  these must state the consequence; consider a guard that refuses to delete
  `chat.admin` while it is the operator's only admin grant.
- **MFA distinction:** the org login policy's MFA-*prompt* (the 2FA-setup nudge
  the demo login hit) is separate from *forcing* MFA. The Project & Org page
  exposes the prompt/`mfaInitSkipLifetime` knob explicitly (§9).

## 4. Shared Shell & Reusable Patterns (Phase 0)

- **`app/(dash)/layout.tsx`** (NEW, the only shell file) + **`components/shell/`**:
  `Sidebar.tsx`, `Topbar.tsx`, `NavLink.tsx` (active-aware `<Link>` via
  `usePathname`), `OperatorBadge.tsx` (fetches `/api/me`, shows name/avatar; on
  401 the api helper already redirects). Theme tokens already exist in
  `globals.css` (`--sidebar-*`). Render chrome immediately; never block on a fetch.
- **Canonical page shape:** page-title + toolbar (primary action) + `DataTable`
  + create/edit `Dialog` (react-hook-form + `zodResolver`) + lifecycle
  `DropdownMenu` + `AlertDialog` ConfirmDialog. Every new area copies this.
- **PREREQUISITE FIX:** `components/ui/data-table.tsx` hardcodes the empty-state
  string **"No users."** — parameterize it with an `emptyMessage` prop **before**
  Roles/Apps/Audit reuse the table, or every table will say "No users."
- **Three missing primitives to add** (shadcn): `card`, `switch`, `checkbox`
  (used by Dashboard cards, policy toggles, grant multiselect).
- **Next.js 16 grounding (mandatory):** `admin-web/AGENTS.md` requires reading
  `node_modules/next/dist/docs/` before routing/page code. Dynamic route params
  are now **async** (`params: Promise<…>`) and route-group layouts add no URL
  segment. Honor both.

## 5. Service-Account Permission Bump (provisioner)

- **`deploy/compose/provisioner/provision.py`**: keep `ADMIN_SA_ROLE =
  "ORG_USER_MANAGER"` in `assign_admin_member` (`POST .../orgs/me/members`), and
  add `assign_admin_project_member` granting `PROJECT_OWNER`
  (`POST /management/v1/projects/{pid}/members`, idempotent — 409 == already a
  member). Both run on a clean boot with the bootstrap token.
- **Live-instance application (no clean re-provision needed):** the SA already has
  `ORG_USER_MANAGER`; apply the missing **project** grant via a one-shot
  `POST /management/v1/projects/{pid}/members {userId, roles:[PROJECT_OWNER]}` with
  the bootstrap IAM_OWNER key (see `deploy/compose/provisioner/README.md`).
- Update `test_provision.py` to assert the scoped roles
  (`ORG_USER_MANAGER` + `PROJECT_OWNER`).

## 6. Users (refactor — reference implementation)

Strip the page-owned `<main className="container">`, the `<h1>` + operator line,
and the Sign-out button out of `app/(dash)/users/page.tsx` (all now the shell's
job). What remains is the canonical *list + create/edit + lifecycle + confirm*
page that every other area mirrors. Backend already complete. Add a per-user
**Access (grants)** affordance here that opens the Grants UI from §7.

## 7. Roles & Grants

**Backend gaps:** 3 new `ZitadelClient` methods + 3 routes (grants infra already
exists).

| Route | Zitadel call (verified) | Notes |
|---|---|---|
| `GET /api/roles` (exists) | `POST .../projects/{pid}/roles/_search` | list project roles |
| `POST /api/roles` | `POST /management/v1/projects/{pid}/roles` | create role (key, displayName, group) |
| `DELETE /api/roles/{roleKey}` | `DELETE /management/v1/projects/{pid}/roles/{roleKey}` | **cascades** — strips the role from every grant |
| `GET /api/roles/{roleKey}/holders` | `POST /management/v1/users/grants/_search` (filter by role) | who holds this role |
| grants (exist) | `users/grants/_search`, add/set/remove grant | per-user assign/revoke |

**UI:** a **Roles page** (DataTable + `CreateRoleDialog` + `HoldersDialog` +
cascade-warning ConfirmDialog) and a **Grants UI** in the user detail (checkbox
multiselect of `list_roles`, grouped by app/project).

**One-grant-per-project invariant (critical):** Zitadel allows one user-grant
per (user, project). Assign/revoke must branch: **POST** to create the grant if
none exists, **PUT** to replace the whole `roleKeys` set if it exists, **DELETE**
to revoke all. Getting this wrong creates duplicate or orphaned grants. `roleKey`
path params must be `encodeURIComponent`'d.

## 8. OIDC Applications (all net-new)

New `zitadel/apps.rs` (6 methods, `pub mod apps;`) + 6 routes. **Per the App=Project
model, an "App" page is the project's apps tab; an app's roles live on the
project (§7), its access on user grants (§7).** This section is the OIDC *client*
CRUD within an app.

| Route | Zitadel call | Verified |
|---|---|---|
| `GET /api/apps` | `POST .../projects/{pid}/apps/_search` | ✅ |
| `POST /api/apps/oidc` | `POST .../projects/{pid}/apps/oidc` | ✅ (provisioner-proven) |
| `GET /api/apps/{appId}` | `GET .../projects/{pid}/apps/{appId}` | ✅ |
| `PUT /api/apps/{appId}/oidc` | `PUT .../apps/{appId}/oidc_config` | ⚠️ **verify before coding** |
| `POST /api/apps/{appId}/secret` | `POST .../apps/{appId}/oidc_config/_generate_client_secret` | ⚠️ **verify before coding** |
| `DELETE /api/apps/{appId}` | `DELETE .../apps/{appId}` | ✅ |

**UI:** Applications list → app detail/edit dialog (redirectUris, grantTypes,
responseTypes, appType, authMethod), **create with one-time secret reveal**,
**rotate secret** (one-time reveal + "breaks clients on the old secret" confirm),
delete. **Edit is read-modify-write** the full `oidc_config`. Confirm dialogs:
"changing redirectUris can instantly break a live login."

## 9. Project & Org Settings

New `zitadel/project.rs` (get/update — the SA's `PROJECT_OWNER` covers this) +
`zitadel/policies.rs` (**read-only** get methods).

| Route | Zitadel call | Writable at runtime? |
|---|---|---|
| `GET/PUT /api/project` | `GET/PUT /management/v1/projects/{id}` | ✅ (PROJECT_OWNER) |
| `GET /api/org/policies/login` | `GET /management/v1/policies/login` | ❌ read-only |
| `GET /api/org/policies/password-complexity` | `GET /management/v1/policies/password/complexity` | ❌ read-only |
| `GET /api/org/policies/lockout` | `GET /management/v1/policies/lockout` | ❌ read-only |

**Why no policy writes at runtime (security review):** org policies are
org-level and would require `ORG_OWNER`, which the least-privilege runtime SA
deliberately does NOT have. So policy **reads** are best-effort (capability-probed
like Audit — if the SA can't even read a policy, the card shows "managed
out-of-band"), and policy **changes** are made by the one-time **provisioner**
with the bootstrap `IAM_OWNER` token, not the Console.

**Demo-login fix moves to the provisioner:** the login policy's
`mfaInitSkipLifetime` (the 2FA-setup nudge that interrupted the demo login) is set
by the provisioner on boot, not edited live. (**Protobuf `Duration` serializes as
a string**, e.g. `"0s"` — handle that where the value is read/displayed.)

**UI:** a Settings page of cards — **Project** (editable: name, settings) and the
three **policy cards read-only** (display current state + a note that policy
changes are provisioner-managed).

## 10. Dashboard

`GET /api/stats` fans out `totalResult` counts from existing `_search` endpoints
(users by type, roles, grants, apps) + a `valid_token()` health self-check; no
new Zitadel surface beyond `count_apps` (apps `_search`). **`details.totalResult`
may serialize as a JSON string** in some builds — the count helper must try
`as_u64` **and** `as_str().parse`, or counts silently read 0. **UI:** colorful
stat cards (mockup) that **deep-link** into their section.

## 11. Audit (capability-gated, conditional — last phase)

`GET /api/events` over `POST /admin/v1/events/_search` (sequence-cursor paging) +
`GET /api/capabilities` (probes whether the SA can read events). **Blocker:** the
event API needs `IAM_OWNER_VIEWER` (instance), not `ORG_OWNER`. So: build the
page + a **fail-closed capability banner** — if `can_read_events` is false, show
"Audit requires IAM_OWNER_VIEWER on the service account" instead of erroring. If
enabled, **confine by `resourceOwner` to the SA's org** (the instance log is
instance-wide; don't leak other orgs). Ships only after §1–§10; enabling the
grant is a follow-up decision.

## 12. Error Handling & Empty/Loading States

`ApiError` → toast with the structured Zitadel message; 403s carry the missing
scope/role. Each Dashboard card degrades to `null`/em-dash on its own failure
(one failing count never blanks the page). Central 401 → `/login` (existing).
Correct per-area empty messages via the new `emptyMessage` prop (§4).

## 13. Testing

- **Backend:** camelCase contract tests on each new handler's JSON; pure
  set-math tests for the grant POST/PUT/DELETE branch (`roles_without` exists);
  `ADMIN_IT`-gated live tests for the **two unknowns** — the policy-upsert error
  code (§9) and the two `verified:false` OIDC endpoints (§8).
- **Frontend:** extend `admin-web/e2e/smoke.spec.ts` per area — Roles
  create/delete (+cascade confirm), App create with **secret-reveal-once**
  assertion, a grant assign/revoke round-trip, a policy edit, Dashboard counts
  render. All reuse the existing `ADMIN_IT=1` operator login.

## 14. Open Risks & Verification Backlog

1. **Audit/ORG_OWNER contradiction (blocker)** — event log needs
   IAM_OWNER_VIEWER; Audit ships capability-gated (§11). *Decision pending:* grant
   the instance role or leave Audit disabled.
2. **Two `verified:false` OIDC endpoints** (`oidc_config` PUT, regenerate secret)
   — confirm the exact path/body before coding §8 (doc pages 404'd via fetch;
   URLs are live).
3. **Policy upsert error code** (404 vs 400 on default policy) — confirm via
   ADMIN_IT (§9).
4. **Self-lockout via Roles/grants** (§3) — guard + confirm.
5. **Live-bump idempotency** — must use *update-member* PUT, not a re-run (§5).
6. **`totalResult` string-or-number** parse (§10).
7. **v1 mgmt endpoints** are doc-deprecated (v2/v3 exist) but are what this
   codebase uses and work on Zitadel v3.4.10 — tech-debt, not v1 scope.
8. **Next.js 16 drift** — async `params`, route-group rules (§4).

## 15. Build Sequence & Dependencies

- **Phase 0 — Foundation:** `(dash)/layout.tsx` shell + `components/shell/*` +
  the 3 missing primitives + the `emptyMessage` DataTable fix + the
  **least-privilege SA grant** (ORG_USER_MANAGER + PROJECT_OWNER, applied live).
  Refactor Users into the shell. *Everything else
  depends on this.*
- **Phase 1 — Roles & Grants** (backend mostly exists → fast).
- **Phase 2 — OIDC Applications** (verify the 2 endpoints first).
- **Phase 3 — Project & Org settings** (needs the bump from Phase 0).
- **Phase 4 — Dashboard** (aggregates the above).
- **Phase 5 — Audit** (capability-gated; conditional on the IAM_OWNER_VIEWER
  decision).

Each phase is a working, navigable screen behind the ribbon before the next
begins.
