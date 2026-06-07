# Zitadel User-Management Admin (Rust BFF + Next.js) — design

**Date:** 2026-06-07
**Status:** approved (design); implementation to follow

> **Appendix (source of truth for API/integration detail):**
> [`2026-06-07-zitadel-admin-api-reference.md`](./2026-06-07-zitadel-admin-api-reference.md)
> — grounded Zitadel v3.4.10 Management-API endpoints, crate versions, and a
> consolidated "verify empirically against the running instance" checklist (§6).
> This design fixes **intent and architecture**; the appendix fixes the **facts**;
> the running Zitadel v3.4.10 is the **ultimate source of truth** and every
> low/medium-confidence item is discharged by an integration test, not a guess.

## 1. Summary

Add a **separate, Rust-based user-management admin** to the `llm-chat` stack. It lets
an operator manage **both machine clients (M2M service accounts) and human users**
through a browser, while **Zitadel keeps running as the identity provider** — the admin
drives Zitadel's **Management API** in the background; the operator never opens the
Zitadel console.

The admin is built as **Approach A — a Backend-For-Frontend (BFF)**:

- **`admin-api`** — a new Rust (`axum`) service that is the *only* holder of secrets.
  It runs the operator's OIDC login (Authorization Code + PKCE), keeps the operator's
  tokens **server-side**, hands the browser an **httpOnly session cookie**, authorizes
  every request on a new **`chat.admin`** project role, and calls the Zitadel
  Management API using a **dedicated, least-privilege admin service account**.
- **`admin-web`** — a Next.js (App Router, latest) UI with a modern component library
  (shadcn/ui). It is a pure client of `admin-api`: it never sees a token or a
  Management credential.

The `manager` and the live chat data path are **untouched** — this is a separate
service with its own blast radius. Auth logic is **shared, not duplicated**:
`manager/src/auth_zitadel.rs` is extracted verbatim into a `crates/zitadel-auth` lib
that both `manager` and `admin-api` depend on (this converts the repo into a Cargo
workspace).

## 2. Goals / Non-Goals

### Goals
- A browser admin to manage Zitadel users (machine + human) end-to-end, **API-driven**,
  with **no manual clicks in the Zitadel console**.
- Operators authenticate via **Zitadel OIDC** and are authorized on a new **`chat.admin`**
  role — real per-operator identity, no new credential system, RBAC reusing the
  stack's own IdP.
- **Credential containment:** the admin SA key, OIDC client secret, session key, and
  Management-API token live **only** in `admin-api`; the browser holds only an opaque
  session cookie.
- **Reuse, don't reimplement:** share `auth_zitadel.rs` via `crates/zitadel-auth`;
  mirror the proven `oidc.py` PKCE flow and `provision.py` Management-API patterns.
- v1 operation surface (below) is **complete enough to run day-to-day** without the
  console: list/view, create, edit, password lifecycle, roles, machine keys, lifecycle.
- Every uncertain Zitadel behavior is **verified against the running v3.4.10** by an
  integration test (appendix §6), starting with the highest-risk one.

### Non-Goals
- **Not removing Zitadel.** Zitadel remains the IdP; the admin *calls* its API.
- **Not production-hardened** as part of v1: the local stack uses a plain-HTTP issuer and
  non-Secure cookies (dev-only, documented). Prod TLS / cookie-domain hardening is noted
  but not delivered here.
- **Not changing** the manager, the chat wire protocol, the worker, or the existing
  machine-key (kabytech) and human-login (OIDC PKCE CLI) flows.
- **Not** deep frontend test coverage in v1 (a login→list→create Playwright smoke only).
- **Not** multi-instance/HA, audit-log storage, or bulk import in v1.

## 3. Architecture & Topology

Trust boundary (credentials live only in `admin-api`):

```
BROWSER ── cookie ──> admin-web (Next.js)  ──/api (credentials:include)──>  admin-api (Rust/axum)
                                                                              │  holds: operator session (server-side),
                                                                              │         admin SA key, OIDC client secret,
                                                                              │         cached Management-API token
   operator OIDC login (full-page nav):
   browser ─> admin-api /login ─> Zitadel /authorize ─> /callback ─(verify JWT, require chat.admin)─> set cookie ─> admin-web

   admin-api ──(JWT-bearer with SA key)──> Zitadel /oauth/v2/token ──(Bearer mgmt token)──> /management/v1 + /v2 user APIs
```

- **Single-issuer linchpin (inherited from the compose stack):** the `iss` claim must
  match byte-for-byte. The browser-facing issuer and the container-internal issuer must
  be reconciled to one literal string; `admin-api` asserts the discovery doc's `issuer`
  equals its configured `ZITADEL_ISSUER` at startup and fails fast otherwise.
- **Preferred topology: same-origin.** `admin-web` proxies `/api/*` to `admin-api`
  (Next.js rewrites), so the session cookie is `SameSite=Lax` with **no CORS layer**.
  If deployed cross-origin, `admin-api` uses exact-origin CORS with `allow_credentials`
  (never `*`); cross-registrable-domain prod then needs `SameSite=None; Secure` (HTTPS).
- **Repo becomes a Cargo workspace:** members `manager`, `worker`, `admin-api`,
  `crates/zitadel-auth`; one `Cargo.lock`, shared `target/`.

## 4. Components

### 4.1 `crates/zitadel-auth` (new shared lib)
Extracted **verbatim** from `manager/src/auth_zitadel.rs`: `JwksCache` (fetch/cache,
RS256 verify, `set_issuer`/`set_audience`), `Principal`, `Principal.has(role)`, role
extraction from `urn:zitadel:iam:org:project:{project_id}:roles`. Consumed by `manager`
(behavior-preserving) and `admin-api` (`principal.has("chat.admin")`). **The manager's
existing tests must stay green after the move** — the extraction adds no behavior.

### 4.2 `admin-api` (new Rust `axum` BFF)
Modules, each with one job:
- **`config`** — all env-driven, validated at startup with a `require_*` fail-fast
  contract naming any missing var (consistent with the manager/worker pattern):
  `ZITADEL_ISSUER`, `ZITADEL_PROJECT_ID`/audience, SA key path, OIDC `client_id` +
  `client_secret`, bind addr, session key, allowed origin.
- **`auth`** — `/login`, `/callback`, `/logout`. Hand-rolled Authorization Code + PKCE
  mirroring `clients/python/llm_chat/oidc.py` (`reqwest` to `/oauth/v2/authorize` and
  `/oauth/v2/token`), verifies the returned JWT via `zitadel-auth`, **requires
  `chat.admin`**, mints an **opaque server-side session** (`tower-sessions`), sets the
  httpOnly cookie. Logout revokes the refresh token (`/oauth/v2/revoke`) + `end_session`
  + clears the session. *(Hand-rolled rather than the `openidconnect` crate to avoid its
  HTTPS-issuer strictness against the plain-HTTP dev issuer — see appendix §6.7.)*
- **`session`/middleware** — an axum extractor that loads the session and **fails closed**
  (401/403) unless the operator is present and has `chat.admin`.
- **`zitadel` client** — the only module touching Zitadel write APIs. Holds the admin SA
  key; mints + **caches** the Management-API token (JWT-bearer, refresh before expiry);
  wraps the Management API grouped by resource: `users` (search/get/create/edit/
  lifecycle/password), `grants` (roles), `keys` (machine keys/secret). Maps gRPC→HTTP→
  clean JSON errors; retries transient `5xx` with bounded backoff (reusing
  `provision.py`'s policy).
- **`api`** — the `/api/*` JSON surface the frontend consumes (table in §5), each route
  behind the session extractor, delegating to the `zitadel` client.

### 4.3 `admin-web` (Next.js, App Router)
Pure UI: users **DataTable** (TanStack), **create/edit dialogs** (shadcn Form + zod),
destructive-action **AlertDialogs**, a thin fetch client (`credentials:'include'`),
login as a full-page navigation to `admin-api/login`. Never holds a token.

### 4.4 `provision.py` additions (one-time, bootstrap `IAM_OWNER`)
- Create the **`chat.admin`** project role (alongside `chat.user`). *(Keeping role
  creation in the one-time provisioner is what lets the runtime SA stay least-privilege —
  it needs no `project.role.write`.)*
- Create the **`admin-api` machine user** + JSON key → `secrets/admin-api-key.json`.
- Register the **OIDC WEB app** (`appType=OIDC_APP_TYPE_WEB`,
  `authMethodType=OIDC_AUTH_METHOD_TYPE_BASIC` + PKCE, code+refresh grants, JWT access
  token, `accessTokenRoleAssertion=true`); capture `clientId` →
  `secrets/admin_oidc_client_id`, `clientSecret` (shown once) →
  `secrets/admin_oidc_client_secret`.
- Grant the SA its org-manager role (`ORG_USER_MANAGER`; **bump to `ORG_OWNER` only if
  the empirical check shows `ORG_USER_MANAGER` cannot mint machine keys** — appendix §6.2).
- Optionally seed a first `chat.admin` operator (or document granting it to the demo
  human user) so the admin is reachable on first boot.

### 4.5 `docker-compose.yml` additions
Two services: `admin-api` (Rust, multi-stage build like `manager`) and `admin-web`
(Next.js `output: 'standalone'`, node runtime). Both env-driven; `admin-api` depends on
`zitadel-init` completed (needs the SA key + OIDC secret + project id).

## 5. API surface

Every `/api/*` route requires a valid session **and** `chat.admin`. v1 scope =
list/view, create (human+machine), **edit human + password lifecycle**, grant/revoke
roles, machine keys + full lifecycle.

| `admin-api` endpoint | Purpose | Zitadel call (appendix §3) |
|---|---|---|
| `GET /login` · `GET /callback` · `POST /logout` | operator OIDC (full-page nav) | `/oauth/v2/authorize`, `/token`, `/revoke`, `end_session` |
| `GET /api/me` | current operator (name, roles) | from session |
| `GET /api/users?q,type,state,page` | list/search | `POST /v2/users` (read) |
| `GET /api/users/{id}` | detail + grants | `GET /v2/users/{id}` + `users/grants/_search` |
| `POST /api/users/human` | create human | `POST /v2/users/human` |
| `POST /api/users/machine` | create machine | `POST /management/v1/users/machine` |
| `PATCH /api/users/{id}/profile` · `/email` | edit human | `PUT .../users/{id}/profile` · `/email` |
| `POST /api/users/{id}/password` · `/resend-init` | password lifecycle | `PUT .../password` · `POST .../_resend_initialization` |
| `POST /api/users/{id}/{deactivate\|reactivate\|lock\|unlock}` | state | `POST .../users/{id}/_deactivate` … |
| `DELETE /api/users/{id}` | delete (irreversible) | `DELETE /management/v1/users/{id}` |
| `GET/POST /api/users/{id}/grants` · `PUT/DELETE .../{grantId}` | roles | `users/{id}/grants` add/update(replace)/remove |
| `GET /api/roles` | list project roles | `projects/{pid}/roles/_search` |
| `GET/POST /api/users/{id}/keys` · `DELETE .../{keyId}` | machine keys | `users/{id}/keys` (+ `_search`) |
| `POST/DELETE /api/users/{id}/secret` | client secret | `PUT/DELETE .../users/{id}/secret` |

**v1 vs v2:** writes/lifecycle use the proven v1 `/management/v1` paths
(matching `provision.py`); reads use v2 `/v2/users` where field names are cleaner. The
`zitadel` client owns the v1↔v2 field mapping (`id`↔`userId`, `userName`↔`username`,
`isEmailVerified`↔`isVerified`, `firstName/lastName`↔`givenName/familyName`).

## 6. Data flow

1. **Operator login (full-page navigation).** browser → `GET admin-api/login`
   (PKCE+`state`+`nonce` stored in a pre-auth session) → 302 Zitadel `/authorize` →
   operator authenticates → 302 `GET admin-api/callback?code&state` → verify `state`,
   exchange `code`+`verifier`, **verify JWT via `zitadel-auth`, require `chat.admin`** →
   mint opaque session, set cookie → 302 to `admin-web`.
2. **Authenticated action** (e.g. create machine key). `admin-web`
   `fetch(POST /api/users/{id}/keys, credentials:'include')` → session extractor checks
   cookie + `chat.admin` → `zitadel` client uses its cached Management-API token →
   `POST .../users/{id}/keys` → the **`keyDetails` private key is returned once**, so
   `admin-api` **streams it to the browser as a one-time download and never persists it**.
3. **Management token lifecycle.** Minted lazily via JWT-bearer with the SA key, cached
   in memory, refreshed before expiry; never logged or exposed.

## 7. Behaviors pinned by the research

- **`409` is runtime, not clean-boot.** Unlike the one-shot provisioner (which exits on
  409), `admin-api` maps `409 ALREADY_EXISTS` to a friendly "already exists" error for
  the operator — it is a long-lived interactive service.
- **Revoke-one-role is read-modify-write.** Grant `PUT` *replaces* the whole role set, so
  "remove one role" = `users/grants/_search` (read current `roleKeys`) → `PUT` the
  reduced set. This makes `users/grants/_search` a load-bearing read.
- **Deactivate/delete is not instant logout.** The manager validates JWTs locally with no
  introspection, so a deactivated user's already-issued token stays valid until its TTL.
  The UI surfaces this; short token TTLs are the mitigation if instant revocation matters.
- **Two different `*_TOKEN_TYPE_JWT` enums.** `ACCESS_TOKEN_TYPE_JWT` (machine *user*)
  vs `OIDC_TOKEN_TYPE_JWT` (OIDC *app*) — must not be conflated (silent failure).

## 8. Error handling & security

- **Credential containment** (§4.2): SA key, client secret, session key from `secrets/`
  (gitignored) via env; Management token cached in memory; nothing secret reaches the
  browser or logs.
- **Session/transport:** opaque session id; `HttpOnly`, `SameSite=Lax`, `Secure` in prod
  (`__Host-` prefix); server-side store (`MemoryStore` dev / Postgres store compose).
  CSRF via `state` on login + same-origin proxy (preferred) or exact-origin credentialed
  CORS.
- **Two fail-fast guards at startup:** (a) required-config validation naming any missing
  var; (b) **issuer-string match** of the discovery doc vs `ZITADEL_ISSUER` (`exit 1`
  on mismatch) — pre-empts silent per-token 401s.
- **Error mapping:** gRPC→HTTP→`{code,message}` JSON; transient `5xx`/connection retry
  with bounded backoff; deterministic `4xx` never retried; no internal/secret leakage.
- **Irreversible actions** (user delete, key delete) require explicit UI confirm
  (AlertDialog) and are terminal server-side.
- **Dev honesty:** plain-HTTP issuer + non-Secure cookies are dev-only and documented.

## 9. Testing

- **Pure Rust unit tests (no network)**, pure-helper/thin-wrapper convention:
  config fail-fast, gRPC→HTTP→JSON error mapping, PKCE challenge/verifier, SA JWT-bearer
  assertion builder, the `chat.admin` session gate, revoke-one-role set math, each
  Management-API request-body builder.
- **`zitadel-auth` extraction is behavior-preserving:** `cargo test -p llm-chat-manager`
  stays green; add `chat.admin` extraction tests.
- **Integration tests vs the running Zitadel v3.4.10** (source of truth): a gated suite
  driving `create → grant → key → deactivate → delete`, **discharging the appendix §6
  checklist**, starting with the highest-risk item — *a human auth-code login actually
  carries `chat.admin` in the verifiable JWT* given the project's
  `projectRoleAssertion/roleCheck/hasProjectCheck=false` flags. Each test records which
  checklist items it closes (and whether an app/project flag had to be flipped).
- **End-to-end acceptance:** operator logs in → creates a machine user + key → that key
  mints a token that passes the **manager's** `chat.user` gate (full-loop with the
  existing stack).
- **Frontend:** a login → list → create Playwright smoke only (v1).
- TDD red→green, one commit per task (repo convention).

## 10. Key risks & verification gates

1. **Human role-claim in the JWT (highest risk).** Must be proven *first*; everything in
   the authorization model depends on it. Likely requires setting app-level
   `accessTokenRoleAssertion=true` and possibly flipping `projectRoleCheck`. Mitigation:
   the very first integration task proves/repairs this before any auth code is built on it.
2. **Issuer-string mismatch** (browser-facing vs container-internal). Mitigation:
   startup fail-fast guard (§8) + reuse the compose stack's single-issuer discipline.
3. **SA privilege sufficiency** for machine-key minting under `ORG_USER_MANAGER`.
   Mitigation: empirical check; bump to `ORG_OWNER` if needed (role creation already
   stays in the provisioner, so no `project.role.write` is required at runtime).
4. **`openidconnect`/plain-HTTP issuer.** Mitigation: hand-rolled PKCE flow mirroring the
   proven `oidc.py` instead of the crate.
5. **Cross-origin cookies.** Mitigation: same-origin proxy as the default topology.

## 11. Open decisions deferred to implementation (resolved by the running instance)
The appendix §6 checklist (50+ items). The plan turns each load-bearing item into an
explicit verification step against the running stack; none are hard-coded as certain.

## 12. New / changed files (overview)
```
llm-chat/
├── Cargo.toml                         # NEW: workspace root (members + shared deps)
├── crates/zitadel-auth/               # NEW: extracted from manager/src/auth_zitadel.rs
├── manager/                           # CHANGED: depend on zitadel-auth (behavior-preserving)
├── admin-api/                         # NEW: Rust axum BFF (config, auth, session, zitadel, api)
├── admin-web/                         # NEW: Next.js App Router admin UI
├── deploy/compose/provisioner/provision.py   # CHANGED: chat.admin role, admin SA+key, OIDC WEB app, SA grant
├── docker-compose.yml                 # CHANGED: admin-api + admin-web services
├── .gitignore / .env.example          # CHANGED: new secrets + env
└── docs/superpowers/specs/2026-06-07-zitadel-admin-api-reference.md  # the grounded appendix
```
