# Zitadel admin Console (admin-api + admin-web)

**Status:** implemented as `admin-api` + `admin-web`; see
[`docs/architecture.md`](../../architecture.md) for the current system view.
Historical design record — the live implementation has since grown past this v1
scope (apps, projects, policies, audit events, org/project settings, multi-app
authz). API/integration facts live in
[`zitadel-api-reference.md`](./zitadel-api-reference.md) (grounded Zitadel
v3.4.10 endpoints + the "verify against the running instance" checklist §6).

## 1. Summary

A separate, Rust-based user-management Console for the `llm-chat` stack. An
operator manages both machine clients (M2M service accounts) and human users
through a browser while Zitadel keeps running as the IdP — the Console drives
Zitadel's Management API in the background; the operator never opens the Zitadel
console. Built as a Backend-For-Frontend (BFF):

- **`admin-api`** (Rust `axum`, `:7676`) — the *only* holder of secrets and the
  *only* component that calls Zitadel's admin APIs. Runs the operator's OIDC
  login (Authorization Code + PKCE), keeps tokens server-side, hands the browser
  an httpOnly session cookie, authorizes every request on the **`chat.admin`**
  project role, and calls the Management API via a dedicated least-privilege
  admin service account.
- **`admin-web`** (Next.js App Router, shadcn/ui, `:3000`) — a pure client of
  `admin-api`; never sees a token or a Management credential.

The `manager` and the live chat data path are untouched. JWT verification is
**shared, not duplicated:** the crate `crates/zitadel-auth` (`JwksCache`,
`Principal`, role extraction) is consumed by both `manager` and `admin-api`
(`st.jwks.verify_sync` → `principal.has("chat.admin")`). The repo is a Cargo
workspace (`manager`, `worker`, `admin-api`, `crates/zitadel-auth`).

**Non-goals:** not removing Zitadel (it stays the IdP); not production-hardened
in v1 (dev stack uses a plain-HTTP issuer + non-Secure cookies, documented); no
change to the manager, chat wire protocol, worker, or existing
machine-key / human-login flows; no multi-instance/HA or bulk import in v1.

## 2. Architecture & topology

Trust boundary — credentials live only in `admin-api`:

```
BROWSER ── cookie ──> admin-web (Next.js)  ──/api (credentials:include)──>  admin-api (Rust/axum)
                                                                              │  holds: operator session (server-side),
                                                                              │         admin SA key, OIDC client secret,
                                                                              │         cached Management-API token
   operator OIDC login (full-page nav):
   browser ─> admin-api /login ─> Zitadel /authorize ─> /callback ─(verify JWT, require chat.admin)─> set cookie ─> admin-web

   admin-api ──(JWT-bearer with SA key)──> Zitadel /oauth/v2/token ──(Bearer mgmt token)──> /management/v1 + /v2 user APIs
```

- **Single-issuer linchpin:** the `iss` claim must match byte-for-byte.
  `admin-api` asserts the discovery doc's `issuer` equals its configured
  `ZITADEL_ISSUER` at startup and fails fast otherwise.
- **Same-origin topology:** `admin-web` proxies `/api/*` to `admin-api` (Next.js
  rewrites), so the cookie is `SameSite=Lax` with no CORS layer. A cross-origin
  deployment uses exact-origin CORS with `allow_credentials` (never `*`); prod
  across registrable domains needs `SameSite=None; Secure` (HTTPS).

## 3. Components

**`crates/zitadel-auth`** — extracted verbatim from `manager/src/auth_zitadel.rs`:
`JwksCache` (fetch/cache, RS256 verify, `set_issuer`/`set_audience`),
`Principal`, `Principal.has(role)`, role extraction from
`urn:zitadel:iam:org:project:{project_id}:roles`. Behavior-preserving for
`manager`; `admin-api` uses `principal.has("chat.admin")`.

**`admin-api`** (`axum` BFF) modules:
- **`config`** — env-driven, validated at startup with a fail-fast contract
  naming any missing var (`ZITADEL_ISSUER`, project id/audience, SA key path,
  OIDC `client_id`+`client_secret`, `ADMIN_BIND_ADDR` `:7676`, origins, session
  key).
- **`auth`** — `/login`, `/callback`, `/logout`. Hand-rolled Authorization Code +
  PKCE mirroring `oidc.py`, verifies the returned JWT via `zitadel-auth`,
  **requires `chat.admin`**, mints an opaque server-side session
  (`tower-sessions`), sets the httpOnly cookie. Hand-rolled rather than the
  `openidconnect` crate, which rejects the plain-HTTP dev issuer (reference §6.7).
- **`session`** — an axum `Operator` extractor that loads the session and **fails
  closed** (401/403) unless `chat.admin` is present, plus an absolute
  session-lifetime ceiling over idle expiry.
- **`zitadel` client** — the only module touching Zitadel admin APIs. Holds the
  SA key; mints + caches the Management-API token (JWT-bearer, refreshed before
  expiry). Wrapped by resource (`users`, `grants`, `keys`, `project`, `apps`,
  `policies`, `events`, `stats`); maps gRPC→HTTP→clean JSON, retries transient
  `5xx` with bounded backoff.
- **`api`** — the `/api/*` JSON surface, each route behind the `Operator`
  extractor.

**`admin-web`** (Next.js) — users DataTable (TanStack), create/edit dialogs
(shadcn Form + zod), destructive-action AlertDialogs, a thin
`credentials:'include'` fetch client, login as a full-page nav to
`admin-api/login`. Never holds a token.

**`provision.py`** (one-time, bootstrap `IAM_OWNER`) — creates the `chat.admin`
project role (alongside `chat.user`), the `admin-api` machine user + JSON key
(`secrets/admin-api-key.json`), the OIDC WEB app (BASIC + PKCE, code+refresh, JWT
access token, `accessTokenRoleAssertion=true`), and grants the SA its
least-privilege org role. Keeping role creation in the provisioner is what lets
the runtime SA stay least-privilege.

## 4. API surface

Every `/api/*` route requires a valid session **and** `chat.admin`; the OIDC
endpoints (`/login`, `/callback`, `/logout`) establish the session and are not
gated. Implemented surface (`admin-api/src/api/mod.rs`):

- **OIDC:** `GET /login` · `/callback` · `/logout` (full-page nav).
- **Operator/health:** `GET /api/me` · `/api/status`.
- **Users:** `GET /api/users?q,type,state` · `GET/DELETE /api/users/{id}` ·
  `POST /api/users/human|machine` · `PATCH .../profile|email` ·
  `POST .../password|resend-init` · `POST .../{deactivate|reactivate|lock|unlock}`.
- **Grants/roles:** `GET/POST /api/users/{id}/grants` · `PUT/DELETE .../{grantId}` ·
  `GET/POST /api/roles` · `PUT/DELETE /api/roles/{roleKey}` · `.../holders`.
- **Keys/secret:** `GET/POST /api/users/{id}/keys` · `DELETE .../{keyId}` ·
  `POST/DELETE /api/users/{id}/secret`.
- **Apps/org/project:** `GET/POST /api/apps` · `GET/PUT/DELETE /api/apps/{appId}` ·
  `POST .../secret` · `GET/PUT /api/org|/api/project` · `/api/org/policies/*`.
- **Multi-app authz:** `GET/POST /api/projects` · `.../roles` · `.../apps` ·
  `.../grants`.
- **Audit/dashboard:** `GET /api/events` · `/api/signins` · `/api/capabilities` ·
  `/api/stats` · `/api/chat-sessions` (live sessions via the manager `/control`
  proxy).

**v1 vs v2:** writes/lifecycle use the v1 `/management/v1` paths (matching
`provision.py`); reads use v2 `/v2/users` where field names are cleaner. The
`zitadel` client owns the v1↔v2 field mapping (`id`↔`userId`,
`userName`↔`username`, `isEmailVerified`↔`isVerified`,
`firstName/lastName`↔`givenName/familyName`).

## 5. Data flow

1. **Operator login (full-page nav).** browser → `GET admin-api/login`
   (PKCE+`state`+`nonce` in a pre-auth session) → 302 Zitadel `/authorize` →
   operator authenticates → 302 `GET admin-api/callback?code&state` → verify
   `state`, exchange `code`+`verifier`, **verify JWT via `zitadel-auth`, require
   `chat.admin`** → mint opaque session, set cookie → 302 to `admin-web`.
2. **Authenticated action.** `admin-web` `fetch(..., credentials:'include')` →
   `Operator` extractor checks cookie + `chat.admin` → `zitadel` client uses its
   cached Management-API token. A returned private key / client secret is returned
   **once**, streamed straight to the operator, and **never persisted**.
3. **Management token lifecycle.** Minted lazily via JWT-bearer with the SA key,
   cached in memory, refreshed before expiry; never logged or exposed.

## 6. Behaviors & risks pinned by the research

- **`409` is runtime, not clean-boot.** `admin-api` maps `409 ALREADY_EXISTS` to
  a friendly "already exists" error — it is a long-lived interactive service.
- **Revoke-one-role is read-modify-write.** Grant `PUT` replaces the whole role
  set, so "remove one role" = read current `roleKeys` → `PUT` the reduced set.
- **Deactivate/delete is not instant logout.** The manager validates JWTs locally
  with no introspection, so an already-issued token stays valid until its TTL;
  short TTLs are the mitigation, surfaced in the UI.
- **Two distinct `*_TOKEN_TYPE_JWT` enums:** `ACCESS_TOKEN_TYPE_JWT` (machine
  user) vs `OIDC_TOKEN_TYPE_JWT` (OIDC app) — must not be conflated.
- **Highest risk — the human role-claim in the JWT.** The whole authorization
  model depends on it; proven first via integration test (needs app-level
  `accessTokenRoleAssertion=true`, possibly flipping `projectRoleCheck`). SA
  privilege for machine-key minting is similarly checked empirically.

## 7. Security

- **Credential containment:** SA key, client secret, session key from `secrets/`
  (gitignored) via env; Management token cached in memory; nothing secret reaches
  the browser or logs.
- **Session/transport:** opaque session id; `HttpOnly`, `SameSite=Lax`, `Secure`
  in prod; server-side store. CSRF via `state` on login + same-origin proxy.
- **Two fail-fast startup guards:** required-config validation naming any missing
  var; issuer-string match of the discovery doc vs `ZITADEL_ISSUER` (exit on
  mismatch) — pre-empts silent per-token 401s.
- **Error mapping:** gRPC→HTTP→`{code,message}` JSON; transient `5xx` retried with
  bounded backoff; deterministic `4xx` never retried; no secret leakage.
  Degraded-permission reads (policies, events) return a `{ available:false }`
  envelope, not an HTTP error.
- **Irreversible actions** (user/key delete) require explicit UI confirm and are
  terminal server-side.

## 8. Testing

- **Pure Rust unit tests (no network):** config fail-fast, gRPC→HTTP→JSON error
  mapping, PKCE challenge/verifier, SA JWT-bearer assertion builder, the
  `chat.admin` session gate, revoke-one-role set math, and camelCase `/api`
  request contracts.
- **`zitadel-auth` extraction is behavior-preserving:**
  `cargo test -p llm-chat-manager` stays green.
- **Integration vs the running Zitadel v3.4.10** (source of truth): a gated suite
  driving `create → grant → key → deactivate → delete`, discharging the reference
  §6 checklist; the human role-claim is proven first.
- **End-to-end:** operator login → create machine user + key → that key mints a
  token passing the manager's `chat.user` gate. **Frontend:** a login → list →
  create Playwright smoke (v1).
