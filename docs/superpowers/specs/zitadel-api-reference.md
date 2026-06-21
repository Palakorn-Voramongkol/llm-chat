# Zitadel admin — API & integration reference

> Working reference for the Rust (axum) admin-api + Next.js Console against Zitadel.
> **`admin-api/src/zitadel/` is the source of truth** for what we actually call —
> every endpoint/shape below is grounded there (and in `provision.py`,
> `auth_zitadel.rs`, `oidc.py`). Target: **Zitadel v3.4.10**, issuer
> `http://host.docker.internal:8080` (plain HTTP, local dev only). Items not yet
> confirmed against the running instance are in §6.
>
> **Approach A:** the BFF/admin-api owns the OIDC session; a dedicated
> least-privilege service account holds Management-API rights; operators are
> authorized via the `chat.admin` project role.

All `/management/v1` (v1) endpoints are officially deprecated for v2 resource APIs
but fully functional in v3.4.10 and are the **proven** path here: v1 for
write/lifecycle (matches `provision.py`); v2 for user reads (`/v2/users`) where
field names are cleaner. `409 ALREADY_EXISTS` is treated as success (idempotent).
gRPC→HTTP status: `ALREADY_EXISTS→409`, `INVALID_ARGUMENT/FAILED_PRECONDITION→400`,
`NOT_FOUND→404`, `PERMISSION_DENIED→403`, `UNAUTHENTICATED→401` (`zitadel/error.rs`).
All calls send `Authorization: Bearer <mgmt token>`, `Content-Type: application/json`,
optional `x-zitadel-orgid`.

---

## 1. Operator login (BFF OIDC web app)

The BFF is a **confidential server**, so it registers a different OIDC app than the
CLI's public NATIVE loopback client. The operator's browser never sees a token; the
BFF holds tokens server-side and issues an httpOnly session cookie.

### 1.1 App type & auth method

- **App type = `OIDC_APP_TYPE_WEB`** (server-side/confidential). Others:
  `OIDC_APP_TYPE_USER_AGENT` (SPA, public), `OIDC_APP_TYPE_NATIVE` (CLI/mobile
  loopback, public — what `provision.py:create_oidc_app` uses today).
- **Auth method = `OIDC_AUTH_METHOD_TYPE_BASIC`** (`client_secret_basic`,
  recommended) — yields a **client secret**. Others: `_POST` (secret in form body),
  `_NONE` (public/PKCE-only, the CLI pattern), `_PRIVATE_KEY_JWT` (strongest,
  key-pair — production upgrade).
- **Combine the secret WITH PKCE** — Zitadel recommends PKCE regardless of app type,
  so the BFF sends both `client_secret` and `code_verifier`.

### 1.2 Register the app

`POST /management/v1/projects/{projectId}/apps/oidc` (camelCase JSON). This is the
shape `apps.rs::create_oidc_app` / `oidc_create_body` send (also the
`provision.py:create_admin_oidc_app`-proven shape):

| Field | Value |
|---|---|
| `name` | e.g. `chat-admin-bff` |
| `redirectUris` | `["http://localhost:{bffPort}/callback"]` (dev) / https in prod |
| `postLogoutRedirectUris` | `["http://localhost:{bffPort}/"]` (or the Next.js app URL) |
| `responseTypes` | `["OIDC_RESPONSE_TYPE_CODE"]` |
| `grantTypes` | `["OIDC_GRANT_TYPE_AUTHORIZATION_CODE","OIDC_GRANT_TYPE_REFRESH_TOKEN"]` |
| `appType` | `"OIDC_APP_TYPE_WEB"` |
| `authMethodType` | `"OIDC_AUTH_METHOD_TYPE_BASIC"` |
| `accessTokenType` | `"OIDC_TOKEN_TYPE_JWT"` (roles ride in a verifiable JWT) |
| `accessTokenRoleAssertion` / `idTokenRoleAssertion` | `true` (roles in access + id token) |
| `devMode` | `true` (dev only — permits `http://localhost`) |

Response returns `clientId` **and** `clientSecret` — the secret is shown **once**;
`apps.rs` streams it straight through, **never logged**, and the caller persists it
(mirror `secrets/oidc_client_id`; add `secrets/oidc_client_secret`). Use only `CODE`
response + `AUTHORIZATION_CODE` grant; add `REFRESH_TOKEN` for `offline_access`. Do
**not** enable `IMPLICIT`.

**Enum trap:** the OIDC-app token enum is `OIDC_TOKEN_TYPE_JWT`, **NOT** the machine
`ACCESS_TOKEN_TYPE_JWT` (§3.2). Don't conflate them (`apps.rs` note).

Related app endpoints (`apps.rs`): list `POST /projects/{pid}/apps/_search`; get
`GET /projects/{pid}/apps/{appId}`; replace config `PUT
/projects/{pid}/apps/{appId}/oidc_config` (full read-modify-write, **no `name`** —
that's an app-level field); regenerate secret `POST
/projects/{pid}/apps/{appId}/oidc_config/_generate_client_secret` (returns
`clientSecret` once); delete `DELETE /projects/{pid}/apps/{appId}`.

### 1.3 Scopes

```
openid profile email offline_access
urn:zitadel:iam:org:project:id:{projectId}:aud
urn:zitadel:iam:org:projects:roles
```

- `...:project:id:{projectId}:aud` puts `projectId` in the token audience — required
  so `auth_zitadel.rs`'s `validation.set_audience(project_id)` passes.
- `...:org:projects:roles` asserts the roles claim for every project in the audience.
- `offline_access` requests a refresh token (code flow only).
- Do **not** use `...:project:id:zitadel:aud` — that targets Zitadel's internal
  project and is for the **management** token (§2), not the operator.

This is `build_scope()` in `clients/python/llm_chat/oidc.py` plus `email`.

### 1.4 Code exchange

- Discover at `{issuer}/.well-known/openid-configuration`. Endpoints: authorize
  `{issuer}/oauth/v2/authorize`, token `{issuer}/oauth/v2/token`, jwks
  `{issuer}/oauth/v2/keys`, revoke `{issuer}/oauth/v2/revoke`, end_session
  `{issuer}/oauth/v2/end_session` (a.k.a. `/oidc/v1/end_session`).
- BFF `POST {issuer}/oauth/v2/token` with `grant_type=authorization_code`, `code`,
  `redirect_uri`, `code_verifier`, authenticating with `client_id`+`client_secret`
  via HTTP Basic (`Basic base64(urlencode(id):urlencode(secret))`). With
  `authMethodType=POST`, put `client_id`+`client_secret` in the form body instead.
- The browser never sees the secret or tokens — only the backend `/callback` does.

### 1.5 Role-claim verification (reuse `auth_zitadel.rs` verbatim)

- `accessTokenType=OIDC_TOKEN_TYPE_JWT` makes the access token a self-contained
  RS256 JWT verifiable via JWKS at `{issuer}/oauth/v2/keys` — exactly what
  `auth_zitadel.rs::JwksCache` does (fetch JWKS, cache by kid, RS256 decode,
  `set_issuer`, `set_audience(project_id)`, read `sub`/`email`/`org`/roles).
  **BEARER (opaque) tokens cannot be verified locally** and force introspection
  (the repo's BEARER → manager 401 lesson).
- Granted roles land under claim `urn:zitadel:iam:org:project:{projectId}:roles` — a
  JSON **object whose KEYS are the role keys** (value `{orgId: orgDomain}`).
  `auth_zitadel.rs` collects `m.keys()` into `Principal.roles`; the BFF gates
  operators with `principal.has("chat.admin")` — **zero new parsing logic**.

### 1.6 Session & logout

- Store the operator's tokens server-side keyed by an opaque session id; set that id
  in an **httpOnly, Secure, SameSite=Lax** cookie (`__Host-` prefix in prod). Persist
  CSRF `state` + OIDC `nonce` across `/login → /callback`. `SameSite=Lax` survives
  the top-level GET redirect back from Zitadel.
- Logout: register `postLogoutRedirectUris`, hit `end_session_endpoint` with
  `post_logout_redirect_uri` (+ `id_token_hint`), revoke the refresh token at
  `{issuer}/oauth/v2/revoke` (as `oidc.py:revoke()`), then clear the server-side
  session + cookie.

---

## 2. Admin service account

The admin-api runs as **its own machine user**, distinct from the bootstrap
`IAM_OWNER` SA.

### 2.1 Least-privilege roles (verified — NOT `ORG_OWNER`)

The runtime SA gets exactly these scoped grants; it must **not** hold standing
`ORG_OWNER` (an org-wide escalation target):

- **`ORG_USER_MANAGER`** (org level) — users + grants (`user.*`, `user.grant.*`).
- **`ORG_SETTINGS_MANAGER`** (org level) — the **minimal** role for `org.write`,
  i.e. renaming the org (§3.7). NOT `ORG_OWNER`.
- **`PROJECT_OWNER`** on each managed project — owns that project's roles + apps
  (`project.role.write/read/delete`, app CRUD, `project.write`).

These grants are written by the **one-time provisioner with the bootstrap
`IAM_OWNER` token** (`assign_admin_member` for the org roles via
`POST .../orgs/me/members`; a `POST /management/v1/projects/{pid}/members
{userId, roles:["PROJECT_OWNER"]}` per project — the SA can't grant itself
ownership it lacks).

Role comparison: `IAM_USER_MANAGER` = users+grants across **all** orgs (broader than
needed, avoid); `IAM_OWNER` = everything (bootstrap SA only — never reuse at
runtime). For the multi-app model, the provisioner adds the SA as `PROJECT_OWNER` on
each project it creates (never blanket `ORG_OWNER`).

**Audit needs more — `IAM_OWNER_VIEWER` (instance-level)**, which none of the above
include. The event log (§3.8) is therefore **capability-gated**: shipped but shown
"unavailable — requires IAM_OWNER_VIEWER" until that grant is added.

### 2.2 Create the machine user

`POST {issuer}/management/v1/users/machine`
```json
{"userName":"chat-admin-api","name":"chat-admin-api",
 "description":"...","accessTokenType":"ACCESS_TOKEN_TYPE_JWT"}
```
Returns `{"userId":"..."}`. Permission `user.write`. Use `ACCESS_TOKEN_TYPE_JWT`
(machine-user enum) so the manager can verify the token locally via JWKS; an SA
token used **only to call the Management API** may be opaque BEARER (Zitadel
introspects it server-side). Proven in `provision.py:create_machine_user`.

### 2.3 Mint the SA's JSON key

`POST {issuer}/management/v1/users/{userId}/keys` body `{"type":"KEY_TYPE_JSON"}`.
Response `keyDetails` is **base64-encoded** serviceaccount JSON
(`{type, keyId, key:<PEM>, userId}`) — returned **once**. `AddMachineKey` permission
is `user.write` per proto (verify empirically — §6). Proven in
`provision.py:generate_json_key`.

### 2.4 Mint the Management-API token (JWT-bearer)

`POST {issuer}/oauth/v2/token`, `Content-Type: application/x-www-form-urlencoded`:
```
grant_type = urn:ietf:params:oauth:grant-type:jwt-bearer
assertion  = <signed JWT>
scope      = openid profile urn:zitadel:iam:org:project:id:zitadel:aud
```
The `assertion` is RS256-signed with the key's PEM, header `{kid: keyId}`, claims
`{iss=userId, sub=userId, aud=issuer, iat, exp=iat+3600}`. The literal token
**`zitadel`** in the scope targets Zitadel's internal project so the Management API
accepts the token (the "scope trap" — without it, 403). `token.rs` caches the token
and refreshes it before expiry; verbatim logic in `provision.py:mint_management_token`.

### 2.5 Org context header

Management calls set `x-zitadel-orgid: <orgId>` when the token's default org differs
from the org being administered; otherwise `orgs/me` and `users/*` resolve to the
token's resource owner. The org id comes from `GET /auth/v1/users/me` →
`user.details.resourceOwner` (`events.rs::org_id_from_me`; flagged UNVERIFIED in
`provision.py` — §6).

---

## 3. Management API surface

These are the methods `admin-api/src/zitadel/` actually calls.

### 3.1 Search / get users (`users.rs`, v2 reads)

- **`POST /v2/users`** (v2 UserService, **the repo's read path**) — list/search —
  body `{"queries":[<SearchQuery>...]}` (AND-combined), optional `query{offset,limit,
  asc}`, `sortingColumn`. Returns `result[]` with `userId`/`username`/
  `email.isVerified`, profile `givenName`/`familyName`. Mapped via
  `model::user_from_v2`.
- **`GET /v2/users/{userId}`** (v2) — get one — `{"user":{userId,state,username,
  human|machine}}`.
- v1 equivalents exist (`POST /management/v1/users/_search`, `GET
  /management/v1/users/{id}`) with field deltas: `id`↔`userId`,
  `userName`↔`username`, `email.isEmailVerified`↔`email.isVerified`,
  `firstName/lastName`↔`givenName/familyName`. Bind Rust structs to the v2 names.

`SearchQuery` (discriminated, one per entry): `userNameQuery {userName,method}`,
`typeQuery {type}`, `displayNameQuery`, `emailQuery {email|emailAddress,method}`,
`firstNameQuery`, `lastNameQuery`, `nickNameQuery`, `loginNameQuery`, `phoneQuery`,
`stateQuery {state}`, `inUserIdsQuery {userIds:[...]}`, `inUserEmailsQuery`,
`organizationIdQuery` (v2), `metadataKeyFilter`/`metadataValueFilter` (v2),
`andQuery`/`orQuery`/`notQuery`.

- `typeQuery.type`: `TYPE_UNSPECIFIED` | `TYPE_HUMAN` | `TYPE_MACHINE`.
- `*.method` (`TextQueryMethod`): `..._EQUALS(_IGNORE_CASE)`,
  `..._STARTS_WITH(_IGNORE_CASE)`, `..._CONTAINS(_IGNORE_CASE)`,
  `..._ENDS_WITH(_IGNORE_CASE)`.
- `stateQuery.state` (`USER_STATE_*`): `UNSPECIFIED`, `ACTIVE`, `INACTIVE`,
  `DELETED`, `LOCKED`, `INITIAL` (created but password/email not set — the "Activate
  User" trap).
- `sortingColumn` (`USER_FIELD_NAME_*`): `USER_NAME`, `FIRST_NAME`, `LAST_NAME`,
  `NICK_NAME`, `DISPLAY_NAME`, `EMAIL`, `STATE`, `TYPE`, `CREATION_DATE`.

### 3.2 Create human / machine (`users.rs`, writes)

- **`POST /v2/users/human`** (v2, **the repo's working path**) — `create_human`
  sends `{username, profile:{givenName,familyName}, email:{email,isVerified:true},
  password:{password,changeRequired:false}}`. `changeRequired:false` +
  `isVerified:true` = immediately-active, login-ready user. Returns `{userId}`. Full
  v2 fields also accept `nickName/displayName/preferredLanguage/gender`, `phone`,
  `organization:{orgId|orgDomain}`, `userId`, `hashedPassword:{hash}`, `metadata`,
  `idpLinks`, `totpSecret`; returns `{userId, details, emailCode?, phoneCode?}`.
- **`POST /management/v1/users/machine`** (v1) — `create_machine` sends `{userName,
  name, accessTokenType:"ACCESS_TOKEN_TYPE_JWT"}` → `{userId}`. Set JWT for any token
  the manager verifies via JWKS; default-when-omitted is undocumented (likely BEARER
  — always set explicitly). Permission `user.write`.

**The biggest gotcha — three password shapes, sending the wrong one is silently
ignored → user stuck in `initial`:** (1) v1 `/users/human`: `initialPassword`
(string, no change-required flag — this stranded the demo user, why the repo uses
v2); (2) v1 `_import`: top-level `password`/`hashedPassword` + separate
`passwordChangeRequired`; (3) v2 `/users/human`: nested
`password:{password,changeRequired}` or `hashedPassword:{hash}`. `hashedPassword`
must be PHC/Modular-Crypt (bcrypt default; argon2/scrypt/pbkdf2/md5 need instance
config). A unified `CreateUser` (`POST /v2/users`, response `{id}`) appears
**v4-era** — don't rely on it for v3.4.10 (§6).

Human edit endpoints (v1): `PUT .../users/{id}/profile {firstName,lastName}`,
`PUT .../users/{id}/email {email,isEmailVerified}`,
`PUT .../users/{id}/password {newPassword:{password,changeRequired}}`,
`POST .../users/{id}/_resend_initialization`.

### 3.3 Project roles (`grants.rs`)

- **`POST /management/v1/projects/{pid}/roles`** — create —
  `{roleKey, displayName, group}`. `roleKey` is the unique id (no separate role id
  returned). Permission `project.role.write` (held via `PROJECT_OWNER`).
- **`PUT /management/v1/projects/{pid}/roles/{roleKey}`** — rename (display name +
  group) — body `{roleKey, displayName, group}`. **The `roleKey` itself is IMMUTABLE**
  (Zitadel ignores a changed key in the path); only `displayName` + `group` change.
- **`POST /management/v1/projects/{pid}/roles/_search`** — list — body `{}` or
  `{queries:[{roleKeyQuery:{key}}]}`; result items `{key,displayName,group,details}`.
- **`DELETE /management/v1/projects/{pid}/roles/{roleKey}`** — delete. **CASCADES** —
  strips the role from every user grant on that project.

### 3.4 User grants / authorizations (`grants.rs`)

- **`POST /management/v1/users/{userId}/grants`** — add —
  `{projectId, roleKeys:[...]}` → `{userGrantId}`. **One grant per (user, project)**
  — a second POST for the same project → `409`; to add a role use PUT.
- **`PUT /management/v1/users/{userId}/grants/{grantId}`** — `{roleKeys:[...]}`
  **REPLACES** the whole set (not additive — resend all to keep). "Remove one role"
  is read-modify-write via `roles_without`.
- **`DELETE /management/v1/users/{userId}/grants/{grantId}`** — revoke entire grant.
- **`POST /management/v1/users/grants/_search`** — search grants (find grantId / list
  holders). **Path is `/users/grants/_search`, NOT nested per-user.** Queries
  AND-combine: `{queries:[{userIdQuery:{userId}}]}` for one user;
  `{queries:[{projectIdQuery:{projectId}},{roleKeyQuery:{roleKey}}]}` to list all
  holders of a role; `{queries:[{projectIdQuery:{projectId}}]}` for the whole project
  roster. Result items `{id, userId, projectId, roleKeys[], orgId, state, ...}`.
  Two `userIdQuery` entries return empty — one user per request. (Deprecated →
  Authorization Service v2 `ListAuthorizations` — verify, §6.)

**The grant-id field is `userGrantId` on add but `id` on search (same value).**
`grants.rs::normalize_grant_id` rewrites both to **`grantId`** so the Console
contract is uniform — without it the grant dialogs PUT to `/grants/undefined` and
silently fail.

### 3.5 Machine keys & secrets (`keys.rs`)

JSON keys (jwt-profile) and client secrets (client_credentials) are **two
independent credential lifecycles** per machine user.

- **`POST /management/v1/users/{userId}/keys`** — create JSON key —
  `{type:KEY_TYPE_JSON}` (optional `expirationDate` RFC3339, or `publicKey` to BYO).
  Returns `{keyId, keyDetails(base64), details}`. **`keyDetails` (the private key) is
  returned ONLY here** — store immediately, never log.
- **`POST /management/v1/users/{userId}/keys/_search`** — list — body `{}`. Returns
  `result[]` with `{id,type,expirationDate,details}` — **metadata only**.
- **`DELETE /management/v1/users/{userId}/keys/{keyId}`** — revoke (deletion IS
  revocation).
- **`PUT /management/v1/users/{userId}/secret`** — generate client secret — returns
  `{clientId, clientSecret, details}`; `clientSecret` returned **once**.
- **`DELETE /management/v1/users/{userId}/secret`** — remove client secret.

### 3.6 User lifecycle (`users.rs`)

- `POST /management/v1/users/{userId}/_deactivate` (reversible) / `_reactivate`.
- `POST /management/v1/users/{userId}/_lock` (forced lockout) / `_unlock`.
- **`DELETE /management/v1/users/{userId}`** — **IRREVERSIBLE** (state→deleted, tears
  down a machine user's keys). Hard-confirm in the UI.

**Stateless-JWT caveat:** the manager does local JWKS validation with **no
introspection**, so deactivate/lock/delete do **not** instantly kill live JWTs — an
issued token stays valid until its TTL. Surface this in UX; use short TTLs if instant
revocation matters.

### 3.7 Org & project (`project.rs`)

- **`GET /management/v1/orgs/me`** — the SA's org (name + id), unwrapped from its
  `{org:{...}}` envelope.
- **`PUT /management/v1/orgs/me {name}`** — **rename the org**. Needs `org.write`,
  granted via **`ORG_SETTINGS_MANAGER`** (minimal — NOT ORG_OWNER). Zitadel rejects a
  no-op rename with `400 "not changed"`.
- **`POST /management/v1/projects/_search`** — list all projects (each = one app).
- **`GET /management/v1/projects/{id}`** / **`PUT /management/v1/projects/{id}`** —
  get/update one project. PUT body is full read-modify-write `{name,
  projectRoleAssertion, projectRoleCheck, hasProjectCheck}`. `PROJECT_OWNER` covers
  both.

### 3.8 Audit / event log (`events.rs`) — capability-gated

- **`POST /admin/v1/events/_search`** — needs **`IAM_OWNER_VIEWER`** (instance), which
  the runtime SA does NOT have. `can_read_events` probes it (a minimal confined
  search; `403/404` → `false`, other errors propagate) and the UI **fails closed**.
- Body `build_events_body` ALWAYS sets **`resourceOwner = SA's org id`** so the
  instance-wide log is **confined to one org** — it must never leak other orgs'
  events (fail-closed confinement). Optional `editorUserId`, `aggregateId` (repeated
  field — wrapped in an array), `creationDate` (RFC3339 lower-bound cursor), `asc`,
  `limit`. Absent filters are omitted (no guessed defaults). Returns the `events`
  array, camelCase preserved.
- The SA's org id comes from `GET /auth/v1/users/me` → `user.details.resourceOwner`;
  if it can't be resolved, `search_events` returns `NotFound` rather than search
  unconfined (a leak).
- Recent sign-ins are derived from this log (`is_signin_event`): the classic hosted
  login creates NO v2 session-API sessions (`/v2/sessions/search` is empty), but
  sign-ins ARE visible as `oidc_session.*` / `user.token.*` / `password.check`
  events.

---

## 4. Rust stack

### 4.1 Workspace & shared auth crate

Extract `manager/src/auth_zitadel.rs` **verbatim** into a lib crate both `manager`
and `admin-api` depend on — it already does JWKS fetch, `DecodingKey::
from_rsa_components`, `jsonwebtoken::decode` with `Validation::new(Algorithm::RS256)`,
`set_issuer` + `set_audience(project_id)`, and role extraction →
`Principal.has(role)`. admin-api gates on `principal.has("chat.admin")`.
`verify_sync()` is synchronous (no I/O after JWKS cache warm), so call it directly in
an axum `FromRequestParts` extractor. **Do not re-implement this.**

Root `Cargo.toml` workspace members `["manager","worker","admin-api",
"crates/zitadel-auth"]`, shared `[workspace.dependencies]` (tokio, serde, reqwest
rustls-tls, jsonwebtoken 9) so manager and admin-api resolve identical versions.

### 4.2 Crates & versions

| Crate | Version | Role |
|---|---|---|
| `axum` | `0.8` | HTTP server (tokio + tower + hyper 1.x) |
| `tokio` | `1` (`rt-multi-thread`,`macros`) | runtime |
| `tower-http` | `0.6` (`cors`,`trace`) | CORS / tracing |
| `tower-sessions` | `0.15` | server-side sessions; cookie holds only session id |
| `tower-sessions-sqlx-store` | `0.15` (`PostgresStore`) | session store on sqlx 0.8 |
| `openidconnect` | `4` (re-exports `oauth2` `^5`) | Auth Code + PKCE + discovery |
| `reqwest` | `0.12` (`json`,`rustls-tls`) | Management-API calls |
| `jsonwebtoken` | `9` | RS256 — JWKS verify **and** SA JWT-bearer assertion |
| `sqlx` | `0.8` (postgres/sqlite, rustls) | reuse manager's pool |

- **axum 0.8:** path params are `/{id}` / `/{*rest}` (not `/:id`); `Option<T>`
  extractors use `OptionalFromRequestParts`.
- **tower-sessions 0.15:** **default is `SameSite::Strict` — set `Lax`** for the OIDC
  redirect (Strict drops the cookie on the cross-site redirect back).
  `PostgresStore::new(pool)` + `store.migrate()`.
- **openidconnect 4.x:** discover → `CoreClient::from_provider_metadata` →
  `PkceCodeChallenge::new_random_sha256` → `authorize_url` → `exchange_code(code)
  .set_pkce_verifier(...)`. **v4 breaking:** async calls take an explicit
  `&http_client` (reqwest adapter), not a closure — build one `reqwest::Client`
  (`redirect::Policy::none()` per SSRF guard) and pass it everywhere. Default auth =
  HTTP Basic = `authMethodType=BASIC` (`.set_auth_type(AuthType::RequestBody)` for
  POST). Persist `pkce_verifier`/`nonce`/`state` in the session. Read `chat.admin` by
  reusing the `zitadel-auth` `JwksCache`, not the crate's id_token_verifier.
- **SA JWT-bearer assertion** (Rust vs `provision.py`'s PyJWT):
  `jsonwebtoken::encode(&Header{alg:RS256, kid:Some(key_id),..}, &Claims{iss:userId,
  sub:userId, aud:issuer, iat, exp:iat+3600}, &EncodingKey::from_rsa_pem(pem)?)`.

### 4.3 TLS hygiene & topology

- Keep **one** TLS backend (rustls) across reqwest, openidconnect, sqlx;
  openidconnect's default reqwest can pull `native-tls`. **Confirm with `cargo tree
  -e features -i reqwest`** that no `native-tls`/`openssl` appears (§6).
- Prefer **same-origin** (Next.js rewrites `/api/* → axum`) so the cookie is
  `SameSite=Lax`, no CORS, JS never touches tokens. If separate origins:
  `CorsLayer::new().allow_origin(exact origin).allow_credentials(true)...` — wildcard
  `*` is rejected with credentials (correct guardrail); cookie then needs
  `SameSite=None; Secure` (HTTPS-only, blocked by the local plain-HTTP issuer).

---

## 5. Next.js frontend

### 5.1 Version & runtime

- **Next.js 16** (App Router, React 19.2, Node 20.9+, Turbopack default — opt out
  `next dev --webpack`). Breaking: `middleware.ts` → `proxy.ts`;
  `cookies()`/`headers()`/`params`/`searchParams` are now **async**. Pin 15.x if 16's
  async churn is unwelcome.
- **Needs a Node server at runtime** (authed dashboard — cookies/server actions/
  dynamic — is unsupported by static `output: 'export'`). Package with **`output:
  'standalone'`** in `node:20-alpine`, `node server.js`, port 3000.

### 5.2 Cross-origin cookie auth flow

- **`localhost:3000` (Next) ↔ `localhost:{bffPort}` (BFF) is SAME-SITE** — SameSite is
  computed at registrable-domain + scheme; **port is ignored**. So a `SameSite=Lax;
  HttpOnly; Path=/` cookie **is** delivered on credentialed cross-origin fetches in
  dev — no `SameSite=None;Secure` needed locally. In prod across different registrable
  domains → `SameSite=None; Secure` (HTTPS) mandatory; same registrable domain
  (sub-domains) → `Lax` works.
- **Same-site ≠ CORS-exempt** — different port = different **origin**. The BFF must
  echo the **exact** Next origin in `Access-Control-Allow-Origin` (never `*` with
  credentials), set `Access-Control-Allow-Credentials: true`, add `Vary: Origin`,
  list methods/headers, answer the OPTIONS preflight. Browser uses
  `fetch(url, {credentials:'include'})` on every call.
- **Login is a full-page navigation, not fetch:** (1) "Sign in" → top-level
  `GET {bff}/login`. (2) BFF makes PKCE+state, 302 → Zitadel `/oauth/v2/authorize`.
  (3) user authenticates (MFA at Zitadel). (4) 302 → `GET {bff}/callback?code&state`.
  (5) BFF verifies state, exchanges code+verifier, validates the JWT (same JWKS/
  issuer/audience), checks `chat.admin`, mints its **own opaque** session cookie
  (`HttpOnly; SameSite=Lax; Path=/`, +Secure prod), 302 → Next app. The browser only
  ever holds the opaque BFF session cookie (XSS-safe). The Next origin is **not** a
  Zitadel redirect target — no Zitadel-side config for it.

### 5.3 Components & packaging

- **shadcn/ui** (Radix + Tailwind v4, React 19; prefer **pnpm** to avoid the
  `--legacy-peer-deps` prompt; components copied into the repo). Users list =
  **DataTable on TanStack Table**; create-user = **Form** (react-hook-form + zod) in a
  **Dialog**; destructive confirm = **AlertDialog** (not plain Dialog). All fetches
  use `credentials:'include'`.
- Compose: `frontend` service, multi-stage Dockerfile, `output:'standalone'`,
  `node:20-alpine`, expose 3000. **`NEXT_PUBLIC_*` is inlined at BUILD time** — build
  per-env or use the same-origin proxy (Next `rewrites`/`proxy.ts`, CORS-free). Dev:
  the BFF's CORS allow-origin must include `http://localhost:3000`.

---

## 6. Must verify empirically against running Zitadel v3.4.10

Treat the **running stack as the source of truth**. Open items not already locked
down by `zitadel/` tests:

**OIDC WEB app & login** — WEB/BASIC app returns a non-empty `clientSecret` and the
exact key name; `/token` accepts `client_secret` + `code_verifier` together (and
ideally requires PKCE); `chat.admin` appears in the ACCESS token (not only id token)
for a HUMAN grant under `urn:zitadel:iam:org:project:{pid}:roles` even with project
`projectRoleAssertion:false`; `http://localhost` / `host.docker.internal` redirects
accepted at runtime with `devMode=true`; `end_session` path
(`/oidc/v1/end_session` vs `/oauth/v2/end_session`) and whether `id_token_hint` is
required; the BFF audience (same project as manager or its own); discovery-doc issuer
string matches `ZITADEL_ISSUER` exactly (else jsonwebtoken issuer validation fails).

**SA & permissions** — `AddMachineKey` succeeds with only `user.write`
(`ORG_USER_MANAGER`); whether this self-hosted instance overrides `defaults.yaml`
RolePermissionMappings; org-id resolution (`x-zitadel-orgid` needed?
`GET /auth/v1/users/me` → `resourceOwner`); whether to migrate deprecated
`AddOrgMember` → v2 `CreateAdministrator`.

**User read/create/grants** — v2 `emailQuery` key (`emailAddress` vs `email`);
exhaustive `TEXT_QUERY_METHOD_*` / `USER_STATE_*` spelling; v2 `ListUsers` without
`organizationIdQuery` returns token-org vs instance-wide; whether Authorization
Service v2 `ListAuthorizations` is enabled (else stay on v1
`/users/grants/_search`); pagination defaults/limits; re-POSTing an existing role /
grant returns **409**; that `grantId` (normalized from `id`/`userGrantId`) is
accepted by PUT/DELETE; whether `x-zitadel-orgid` is needed when the target user is
in the SA's org.

**User creation** — default `accessTokenType` when omitted on `/users/machine`
(likely BEARER — always set JWT); reproduce v1 `/users/human` `initialPassword`
stranding the user in `initial`; confirm v2 `/users/human` returns `{userId}` and
goes active; whether unified `CreateUser` exists on v3.4.10 (docs suggest v4-only);
v2 `hashedPassword` shape and enabled verifiers (bcrypt only by default); invite/
init-email behavior.

**Keys & lifecycle** — `expirationDate` accepted / rejects past dates; status codes
for deleting a missing key / already-deleted user; exact `_search` body shape and
pagination; how long until a deactivated/locked machine user is actually locked out
given local JWKS validation (token TTL) and whether a short-TTL/revocation strategy
is needed; machine-secret rotation invalidates the prior secret; if choosing v2 for
keys, re-verify every shape (v2 `AddKey` returns `keyContent` not `keyDetails`) — do
not mix v1 create with v2 list.

**Rust stack** — `openidconnect` 4.x / `oauth2` 5.x accepts the plain-HTTP issuer
`http://host.docker.internal:8080` (likely a local-dev blocker); `cargo tree`
shows rustls only; `SameSite=Lax` survives Zitadel's cross-site 302 back;
`tower-sessions` 0.15 + sqlx-store 0.15 compile against the repo's sqlx 0.8 (rustls);
exact `openidconnect` 4.0.x API surface.

**Frontend / CORS** — `tower-http` `allow_credentials(true)` with an exact origin +
the OPTIONS preflight for JSON POST/DELETE return the expected headers; BFF mints its
own opaque session cookie (or, if storing the JWT, it stays under ~4KB); production
domain topology for cookie attributes; `postLogoutRedirectUris` points at the Next
app and the Next origin needs no Zitadel-side config.
