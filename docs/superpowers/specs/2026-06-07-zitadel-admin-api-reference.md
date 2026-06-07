# Zitadel Admin — Grounded API & Integration Reference

> Appendix for the Rust (axum) BFF + Next.js Zitadel user-management admin design doc.
> **Approach A:** the BFF owns the OIDC session; a dedicated least-privilege admin
> service account holds Management-API rights; operators are authorized via the
> `chat.admin` project role.
>
> Target runtime: **Zitadel v3.4.10**, issuer `http://host.docker.internal:8080`
> (plain HTTP, local dev only). Every fact below is grounded in the repo's proven
> code (`provision.py`, `auth_zitadel.rs`, `oidc.py`) or official Zitadel/crate
> docs; items not yet confirmed against the running instance are collected in §6.

---

## 1. Operator login (BFF OIDC web app)

The BFF is a **confidential server**, so it registers a different OIDC app than the
CLI's public NATIVE loopback client. The operator's browser never sees a token; the
BFF holds tokens server-side and issues an httpOnly session cookie.

### 1.1 App type & auth method

- **App type = `OIDC_APP_TYPE_WEB`** (proto enum `0`) — server-side/confidential.
  Contrast: `OIDC_APP_TYPE_USER_AGENT=1` (SPA, public), `OIDC_APP_TYPE_NATIVE=2`
  (CLI/mobile loopback, public — what `provision.py:create_oidc_app` uses today).
  (source: https://raw.githubusercontent.com/zitadel/zitadel/main/proto/zitadel/app.proto)
- **Auth method = `OIDC_AUTH_METHOD_TYPE_BASIC`** (`0`, `client_secret_basic`,
  default/recommended) — yields a **client secret**. Alternatives:
  `_POST=1` (secret in form body), `_NONE=2` (public/PKCE-only, the CLI pattern),
  `_PRIVATE_KEY_JWT=3` (strongest, key-pair, no shared secret — production upgrade).
  (source: https://zitadel.com/docs/apis/openidoauth/authn-methods)
- **Combine the secret WITH PKCE.** Zitadel recommends PKCE "regardless of the app
  type," so the BFF sends both `client_secret` and `code_verifier`.
  (source: https://zitadel.com/docs/guides/integrate/login/oidc/oauth-recommended-flows)

### 1.2 Register the app

`POST /management/v1/projects/{projectId}/apps/oidc` (camelCase JSON body):

| Field | Value |
|---|---|
| `name` | e.g. `chat-admin-bff` |
| `redirectUris` | `["http://localhost:{bffPort}/callback"]` (dev) / https in prod |
| `postLogoutRedirectUris` | `["http://localhost:{bffPort}/"]` (or the Next.js app URL) |
| `responseTypes` | `["OIDC_RESPONSE_TYPE_CODE"]` |
| `grantTypes` | `["OIDC_GRANT_TYPE_AUTHORIZATION_CODE","OIDC_GRANT_TYPE_REFRESH_TOKEN"]` |
| `appType` | `"OIDC_APP_TYPE_WEB"` |
| `authMethodType` | `"OIDC_AUTH_METHOD_TYPE_BASIC"` |
| `accessTokenType` | `"OIDC_TOKEN_TYPE_JWT"` |
| `accessTokenRoleAssertion` | `true` (roles ride in the access-token JWT) |
| `idTokenRoleAssertion` | `true` (roles in id token too) |
| `devMode` | `true` (dev only — permits `http://localhost`) |

Response returns `clientId` **and** `clientSecret` — the secret is shown **once**;
persist it immediately (mirror how the repo writes `secrets/oidc_client_id`; add
`secrets/oidc_client_secret`). This is `provision.py:create_oidc_app` except
`appType=WEB`, `authMethodType=BASIC`, and capturing the secret.
(source: https://zitadel.com/docs/apis/resources/mgmt/management-service-update-oidc-app-config;
source: D:\projects\llm-chat\deploy\compose\provisioner\provision.py)

Use only `CODE` response + `AUTHORIZATION_CODE` grant; add `REFRESH_TOKEN` for
`offline_access`. Do **not** enable `IMPLICIT` (deprecated/insecure).
(source: https://raw.githubusercontent.com/zitadel/zitadel/main/proto/zitadel/app.proto)

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
- Do **not** use `...:project:id:zitadel:aud` — that targets Zitadel's own internal
  project and is for the **admin/management** token (§2), not the operator.

This is exactly `build_scope()` in `clients/python/llm_chat/oidc.py` plus `email`.
(source: https://zitadel.com/docs/apis/openidoauth/scopes;
source: D:\projects\llm-chat\clients\python\llm_chat\oidc.py)

### 1.4 Code exchange

- Discover at `{issuer}/.well-known/openid-configuration`. Zitadel endpoints:
  authorize `{issuer}/oauth/v2/authorize`, token `{issuer}/oauth/v2/token`,
  jwks `{issuer}/oauth/v2/keys`, revoke `{issuer}/oauth/v2/revoke`,
  end_session `{issuer}/oauth/v2/end_session` (a.k.a. `/oidc/v1/end_session`).
- BFF `POST {issuer}/oauth/v2/token` with `grant_type=authorization_code`, `code`,
  `redirect_uri`, `code_verifier`, authenticating with `client_id`+`client_secret`
  via HTTP Basic header (`Basic base64(urlencode(id):urlencode(secret))`). With
  `authMethodType=POST`, put `client_id`+`client_secret` in the form body instead.
- The browser never sees the secret or tokens — only the backend `/callback` does.
  (source: https://zitadel.com/docs/apis/openidoauth/endpoints)

### 1.5 Role-claim verification (reuse `auth_zitadel.rs` verbatim)

- Set `accessTokenType=OIDC_TOKEN_TYPE_JWT` so the access token is a self-contained
  RS256 JWT verifiable via JWKS at `{issuer}/oauth/v2/keys` — exactly what
  `auth_zitadel.rs::JwksCache` does (fetch JWKS, cache by kid, RS256 decode,
  `set_issuer(issuer)`, `set_audience(project_id)`, read `sub`/`email`/`org`/roles).
  **BEARER (opaque) tokens cannot be verified locally** and force introspection;
  the repo learned this (BEARER → manager 401).
- Granted roles land under claim `urn:zitadel:iam:org:project:{projectId}:roles` — a
  JSON **object whose KEYS are the role keys** (value is `{orgId: orgDomain}`).
  `auth_zitadel.rs` builds `roles_key = format!("urn:zitadel:iam:org:project:{}:roles", project_id)`,
  collects `m.keys()` into `Principal.roles`, and `Principal.has("chat.user")` checks
  membership. The BFF reuses this with `principal.has("chat.admin")` — **zero new
  parsing logic**.
  (source: D:\projects\llm-chat\manager\src\auth_zitadel.rs;
  source: https://zitadel.com/docs/guides/integrate/retrieve-user-roles)

### 1.6 Session & logout

- After code exchange, store the operator's tokens server-side keyed by an opaque
  session id; set that id in an **httpOnly, Secure, SameSite=Lax** cookie (use the
  `__Host-` prefix in prod). The browser holds only the session id. Persist the CSRF
  `state` + OIDC `nonce` across `/login → /callback` (pre-auth session).
  `SameSite=Lax` survives the top-level GET redirect back from Zitadel.
- Logout: register `postLogoutRedirectUris`, hit the `end_session_endpoint` with
  `post_logout_redirect_uri` (+ `id_token_hint`), revoke the refresh token at
  `{issuer}/oauth/v2/revoke` (as `oidc.py:revoke()` does), then clear the
  server-side session + cookie.
  (source: D:\projects\llm-chat\clients\python\llm_chat\oidc.py)

---

## 2. Admin service account

The admin-api runs as **its own machine user**, distinct from the bootstrap
`IAM_OWNER` SA. Least privilege = grant **`ORG_USER_MANAGER`** (org-scoped) unless it
must create project roles at runtime, in which case `ORG_OWNER` is required.

### 2.1 Create the machine user

`POST {issuer}/management/v1/users/machine`
```json
{"userName":"chat-admin-api","name":"chat-admin-api",
 "description":"...","accessTokenType":"ACCESS_TOKEN_TYPE_JWT"}
```
Returns `{"userId":"..."}`. Permission: `user.write`. Use `ACCESS_TOKEN_TYPE_JWT` (not
BEARER) if the manager will verify the token locally; an SA token used **only to call
the Zitadel Management API** may be opaque BEARER (Zitadel introspects it
server-side — two validators, two requirements, don't conflate). Proven in
`provision.py:create_machine_user`.
(source: D:\projects\llm-chat\deploy\compose\provisioner\provision.py)

### 2.2 Mint the SA's JSON key

`POST {issuer}/management/v1/users/{userId}/keys`
```json
{"type":"KEY_TYPE_JSON"}
```
Response `keyDetails` is **base64-encoded** serviceaccount JSON
(`{type, keyId, key:<PEM>, userId}`) — returned **once**. Permission for
`AddMachineKey` is **`user.write`** (proto auth_option), so a user-manager can mint
keys (verify empirically — see §6). Proven in `provision.py:generate_json_key` /
`decode_key_details`.
(source: https://zitadel.com/docs/apis/resources/mgmt/management-service-add-machine-key)

### 2.3 Least-privilege role choice

`ORG_USER_MANAGER` permission set (verbatim, v3.4.10 `cmd/defaults.yaml`
RolePermissionMappings):
```
org.read, user.read, user.global.read, user.write, user.delete,
user.grant.read, user.grant.write, user.grant.delete, user.membership.read,
user.feature.read/write/delete, policy.read, project.read,
project.role.read, session.read, session.delete
```
This covers: create/update users (`user.write`), deactivate
(`POST /users/{id}/_deactivate`), delete (`DELETE /users/{id}` → `user.delete`),
machine-key creation (`user.write`), and user-grant CRUD (`user.grant.*`).

**It does NOT include `project.role.write`** — so `ORG_USER_MANAGER` cannot create
project roles (`AddProjectRole` requires `project.role.write`). If the admin-api
must create roles at runtime, use **`ORG_OWNER`** (org-scoped, no instance-level
membership needed; includes `project.role.write/read/delete`, `project.create`,
`project.member.*`, `org.member.*`, `user.*`, `user.grant.*`).

Role comparison: `ORG_USER_MANAGER` = users+grants only (true least privilege);
`ORG_OWNER` = full org incl. `project.role.write`; `IAM_USER_MANAGER` = users+grants
across **all** orgs (still no `project.role.write` — broader than needed, avoid);
`IAM_OWNER` = everything (bootstrap SA only — do **not** reuse at runtime).
(source: https://raw.githubusercontent.com/zitadel/zitadel/v3.4.10/cmd/defaults.yaml;
source: https://raw.githubusercontent.com/zitadel/zitadel/v3.4.10/proto/zitadel/management.proto)

### 2.4 Assign the manager role (one-time, by the bootstrap IAM_OWNER token)

`POST {issuer}/management/v1/orgs/me/members`
```json
{"userId":"<sa userId>","roles":["ORG_USER_MANAGER"]}
```
Permission: `org.member.write`. `orgs/me` resolves the org from the calling token /
`x-zitadel-orgid` header — set that header to the target org. **This call must use a
token that has `org.member.write`** (the bootstrap IAM_OWNER SA), NOT the new
least-privilege SA. `AddOrgMember` is marked deprecated in favor of a v2
`CreateAdministrator` API but is functional in v3.4.10.
(source: https://zitadel.com/docs/reference/api/management/zitadel.management.v1.ManagementService.AddOrgMember;
source: https://zitadel.com/docs/concepts/structure/managers)

Discover assignable role keys at runtime via `ListOrgMemberRoles` (path unconfirmed —
§6) instead of hardcoding, since self-hosted instances can override `defaults.yaml`.

### 2.5 Mint the Management-API token (JWT-bearer)

`POST {issuer}/oauth/v2/token`, `Content-Type: application/x-www-form-urlencoded`:
```
grant_type = urn:ietf:params:oauth:grant-type:jwt-bearer
assertion  = <signed JWT>
scope      = openid profile urn:zitadel:iam:org:project:id:zitadel:aud
```
The **`assertion`** is RS256-signed with the key's PEM, header `{kid: keyId}`, claims
`{iss=userId, sub=userId, aud=issuer, iat, exp=iat+3600}`. The literal token
**`zitadel`** in the scope targets Zitadel's own internal project so the Management
API accepts the token (the "scope trap" — without it you get 403). Verbatim in
`provision.py:build_jwt_assertion` / `mint_management_token`.
(source: https://zitadel.com/docs/guides/integrate/zitadel-apis/access-zitadel-apis;
source: D:\projects\llm-chat\deploy\compose\provisioner\provision.py)

### 2.6 Org context header

All management calls set `x-zitadel-orgid: <orgId>` when the token's default org
differs from the org being administered; otherwise `orgs/me` and `users/*` resolve to
the token's resource owner. `provision.py` derives the org id from
`GET /auth/v1/users/me` → `user.details.resourceOwner` (flagged UNVERIFIED — §6) and
omits the header if unavailable.

---

## 3. Management API surface

All calls: `Authorization: Bearer <mgmt token>`, `Content-Type: application/json`,
optional `x-zitadel-orgid`. Every `/management/v1` (v1) endpoint is officially
**deprecated** in favor of v2 resource APIs but is fully functional in v3.4.10 and is
the **proven** path in this repo. Recommendation: v1 for write/lifecycle (matches
`provision.py`); v2 for read paths (`/v2/users`) where field names are cleaner.
`409 ALREADY_EXISTS` is treated as success (idempotent) per `provision.py:is_success`.
gRPC→HTTP: `ALREADY_EXISTS→409`, `INVALID_ARGUMENT/FAILED_PRECONDITION→400`,
`NOT_FOUND→404`, `PERMISSION_DENIED→403`, `UNAUTHENTICATED→401`.

### 3.1 Search / get users

- **`POST /management/v1/users/_search`** (v1, org-scoped) — list users —
  body `{"query":{offset,limit,asc}, "sortingColumn":<USER_FIELD_NAME_*>, "queries":[<SearchQuery>...]}`;
  `queries` AND-combined. Returns `result[]` items with `id`/`userName`/
  `email.isEmailVerified`, discriminated by `human` vs `machine`.
- **`POST /v2/users`** (v2 UserService, **recommended**) — list users — same body
  shape; adds `organizationIdQuery` for cross-org scoping and `andQuery/orQuery/notQuery`.
  Returns `result[]` with `userId`/`username`/`email.isVerified`, profile
  `givenName`/`familyName`. (No `/_search` suffix.)
- **`GET /management/v1/users/{id}`** (v1) — get one — `{"user":{id,state,userName,human|machine}}`.
- **`GET /v2/users/{userId}`** (v2, **recommended**) — get one —
  `{"details",,"user":{userId,state,username,human|machine}}`.

`SearchQuery` discriminated union (one per entry): `userNameQuery {userName,method}`,
`typeQuery {type}`, `displayNameQuery`, `emailQuery {email|emailAddress,method}`,
`firstNameQuery`, `lastNameQuery`, `nickNameQuery`, `loginNameQuery`, `phoneQuery`,
`stateQuery {state}`, `inUserIdsQuery {userIds:[...]}`, `inUserEmailsQuery`,
`organizationIdQuery` (v2), `metadataKeyFilter`/`metadataValueFilter` (v2),
`andQuery`/`orQuery`/`notQuery`.

- **`typeQuery.type`**: `TYPE_UNSPECIFIED` | `TYPE_HUMAN` | `TYPE_MACHINE`.
- **`*.method`** (`TextQueryMethod`): `TEXT_QUERY_METHOD_EQUALS`,
  `..._EQUALS_IGNORE_CASE`, `..._STARTS_WITH(_IGNORE_CASE)`,
  `..._CONTAINS(_IGNORE_CASE)`, `..._ENDS_WITH(_IGNORE_CASE)`.
- **`stateQuery.state`** (`USER_STATE_*`): `UNSPECIFIED`, `ACTIVE`, `INACTIVE`,
  `DELETED`, `LOCKED`, `INITIAL` (created but password/email not set — the "Activate
  User" trap).
- **`sortingColumn`** (`USER_FIELD_NAME_*`): `USER_NAME`, `FIRST_NAME`, `LAST_NAME`,
  `NICK_NAME`, `DISPLAY_NAME`, `EMAIL`, `STATE`, `TYPE`, `CREATION_DATE`.

**v1↔v2 field deltas (bind Rust structs carefully):** `id`↔`userId`,
`userName`↔`username`, `email.isEmailVerified`↔`email.isVerified`,
profile `firstName/lastName`↔`givenName/familyName`.
(source: https://zitadel.com/docs/apis/resources/mgmt/management-service-list-users;
source: https://zitadel.com/docs/apis/resources/user_service_v2/user-service-list-users;
source: https://zitadel.com/docs/apis/resources/mgmt/management-service-get-user-by-id;
source: https://zitadel.com/docs/apis/resources/user_service_v2/user-service-get-user-by-id)

### 3.2 Create human / machine

- **`POST /management/v1/users/machine`** — create machine user — fields
  `userName`(req), `name`(req), `description`, `accessTokenType`, `userId`(optional
  set-your-own). Returns `{userId, details}`. `409`=ALREADY_EXISTS. Permission
  `user.write`. `accessTokenType` ∈ `ACCESS_TOKEN_TYPE_BEARER` (opaque ~104-char) |
  `ACCESS_TOKEN_TYPE_JWT`. **Set JWT** for any token the manager verifies via JWKS;
  default-when-omitted is undocumented (likely BEARER — always set explicitly).
- **`POST /v2/users/human`** (v2, **the repo's chosen working path**) — create human —
  fields `username`, `profile:{givenName,familyName,nickName?,displayName?,
  preferredLanguage?,gender?}`, `email:{email, isVerified:true}` (or `sendCode:{}` /
  `returnCode:{}`), `phone?`, `organization:{orgId}|{orgDomain}`, `userId?`,
  `password:{password, changeRequired:false}` OR `hashedPassword:{hash}`, `metadata?`,
  `idpLinks?`, `totpSecret?`. Returns `{userId, details, emailCode?, phoneCode?}`.
  `changeRequired:false` + `isVerified:true` = immediately-active, login-ready user.
  Proven in `provision.py:create_human_user`.
- **`POST /management/v1/users/human`** (v1, **avoid for new users**) — create human —
  password field is **`initialPassword`** (bare string, **no change-required flag**);
  this left the demo user stuck in `initial` state, which is why the repo migrated to
  v2. Returns `{userId, details}`.
- **`POST /management/v1/users/human/_import`** (v1, migration) — top-level
  `password` OR `hashedPassword` (strings) PLUS separate top-level
  `passwordChangeRequired` bool; supports `recoveryCodes`, `idps`, `otpCode`.

**The single biggest gotcha — three password shapes:** (1) v1 `/users/human`:
`initialPassword` (string, no flag); (2) v1 `_import`: top-level `password`/
`hashedPassword` + separate `passwordChangeRequired`; (3) v2 `/users/human`: nested
`password:{password,changeRequired}` or `hashedPassword:{hash}`. Sending the wrong
shape is **silently ignored** → user stuck in `initial`. Match shape to endpoint.
`hashedPassword` must be PHC/Modular-Crypt format (bcrypt enabled by default;
argon2/scrypt/pbkdf2/md5 need explicit instance config).

A unified `CreateUser` (`POST /v2/users`, response `{id}` not `{userId}`) exists in
docs but appears **v4-era** — do not rely on it for v3.4.10 (§6).
(source: https://zitadel.com/docs/reference/api/management/zitadel.management.v1.ManagementService.AddMachineUser;
source: https://zitadel.com/docs/reference/api/user/zitadel.user.v2.UserService.AddHumanUser;
source: D:\projects\llm-chat\deploy\compose\provisioner\provision.py)

### 3.3 Project roles

- **`POST /management/v1/projects/{projectId}/roles`** — create role —
  `{"roleKey":"chat.admin","displayName":"Chat Admin","group":""}`. `roleKey`+
  `displayName` required; `group` optional. Returns `{details}` (no role id —
  `roleKey` is the unique id). Permission `project.role.write`. Proven for `chat.user`
  in `provision.py:add_role`.
- **`POST /management/v1/projects/{projectId}/roles/_search`** — list/search roles —
  `{"query":{offset,limit,asc}, "queries":[{"roleKeyQuery":{"key":"chat.admin"}}]}`;
  result items `{key,displayName,group,details}`. Permission `project.role.read`.

(source: https://zitadel.com/docs/apis/resources/mgmt/management-service-add-project-role;
source: https://zitadel.com/docs/apis/resources/mgmt/management-service-list-project-roles)

### 3.4 User grants (authorizations)

- **`POST /management/v1/users/{userId}/grants`** — add grant —
  `{"projectId":"<pid>","roleKeys":["chat.admin"]}`. Returns `{"userGrantId","details"}`.
  Permission `user.grant.write`. **One grant per (user, project)** — a second POST for
  the same project → `409 ALREADY_EXISTS`; to add a role to an existing grant use PUT.
- **`PUT /management/v1/users/{userId}/grants/{grantId}`** — update grant —
  `{"roleKeys":["chat.user","chat.admin"]}`. **REPLACES** the whole roleKeys set (not
  additive — resend all to keep). Permission `user.grant.write`.
- **`DELETE /management/v1/users/{userId}/grants/{grantId}`** — revoke entire grant —
  no body, returns `{details}`. To drop one role of several, PUT a reduced set instead.
  Permission `user.grant.delete`.
- **`POST /management/v1/users/grants/_search`** — search grants (find grantId / list
  all `chat.admin` operators) — **path is `/users/grants/_search`, NOT
  `/users/{userId}/grants/_search`**. Body
  `{"query":{offset,limit,asc}, "queries":[{"userIdQuery":{"userId":"..."}},
  {"projectIdQuery":{"projectId":"..."}},{"roleKeyQuery":{"roleKey":"chat.admin"}}]}`.
  Result items: `{id (=userGrantId), userId, projectId, projectGrantId, roleKeys[],
  orgId, orgName, state, ...}`. **GOTCHA:** queries AND-combine — two `userIdQuery`
  entries return empty; query one user per request, or filter by
  `projectId`+`roleKey` to list all admins. (Itself deprecated → Authorization
  Service v2 `ListAuthorizations` — verify availability, §6.)

The grant-id field is `userGrantId` on add but `id` on search (same value).
Proven for both human and machine users via `provision.py:grant_role`.
(source: https://zitadel.com/docs/apis/resources/mgmt/management-service-add-user-grant;
source: https://zitadel.com/docs/apis/resources/mgmt/management-service-update-user-grant;
source: https://zitadel.com/docs/apis/resources/mgmt/management-service-remove-user-grant;
source: https://zitadel.com/docs/apis/resources/mgmt/management-service-list-user-grants)

### 3.5 Machine keys & secrets

JSON keys (jwt-profile) and client secrets (client_credentials) are **two independent
credential lifecycles** per machine user.

- **`POST /management/v1/users/{userId}/keys`** — create JSON key — body
  `{"type":"KEY_TYPE_JSON","expirationDate":"<RFC3339>"}` (both optional; or supply
  `publicKey` to bring your own). Returns `{keyId, keyDetails(base64), details}`.
  **`keyDetails` (the private key) is returned ONLY here** — store immediately.
- **`POST /management/v1/users/{userId}/keys/_search`** — list keys — body
  `{}` or `{"query":{offset,limit,asc}}`. Returns `result[]` with
  `{id,type,expirationDate,details}` — **metadata only, no private key**.
- **`GET /management/v1/users/{userId}/keys/{keyId}`** — get one key —
  `{"key":{id,type,expirationDate,details}}` (metadata only).
- **`DELETE /management/v1/users/{userId}/keys/{keyId}`** — revoke key (deletion IS
  revocation) — no body, returns `{details}`.
- **`PUT /management/v1/users/{userId}/secret`** — generate client secret — returns
  `{clientId, clientSecret, details}`; `clientSecret` returned **once**.
- **`DELETE /management/v1/users/{userId}/secret`** — remove client secret.

(source: https://zitadel.com/docs/apis/resources/mgmt/management-service-add-machine-key;
source: https://zitadel.com/docs/apis/resources/mgmt/management-service-list-machine-keys;
source: https://zitadel.com/docs/apis/resources/mgmt/management-service-remove-machine-key;
source: https://zitadel.com/docs/reference/api/management/zitadel.management.v1.ManagementService.RemoveMachineSecret)

### 3.6 User lifecycle

- **`POST /management/v1/users/{userId}/_deactivate`** — no body → `{details}`. Errors
  if already deactivated. Reversible.
- **`POST /management/v1/users/{userId}/_reactivate`** — undo deactivate.
- **`POST /management/v1/users/{userId}/_lock`** — temporary forced lockout. Errors if
  already locked.
- **`POST /management/v1/users/{userId}/_unlock`** — undo lock.
- **`DELETE /management/v1/users/{userId}`** — **IRREVERSIBLE** delete (state→deleted,
  tears down a machine user's keys). Surface a hard confirm in the UI.

**Critical stateless-JWT caveat:** the manager does local JWKS validation with **no
introspection**, so deactivate/lock/delete do **not** instantly kill live JWTs — an
already-issued token stays valid until its TTL expires. Surface this in admin-api UX;
consider short token TTLs if instant revocation matters.
(source: https://zitadel.com/docs/apis/resources/mgmt/management-service-deactivate-user;
source: https://zitadel.com/docs/apis/resources/mgmt/management-service-remove-user;
source: D:\projects\llm-chat\manager\src\auth_zitadel.rs)

---

## 4. Rust stack

The repo is **not yet a Cargo workspace**: `manager/` (`llm-chat-manager`) and
`worker/` (`llm-chat`, Tauri) are standalone crates with no root `Cargo.toml`.
(source: D:\projects\llm-chat\manager\Cargo.toml; D:\projects\llm-chat\worker\Cargo.toml)

### 4.1 Workspace conversion (step one)

Create a root `Cargo.toml`:
```toml
[workspace]
members  = ["manager", "worker", "admin-api", "crates/zitadel-auth"]
resolver = "2"            # "3" if you adopt edition 2024

[workspace.dependencies]
tokio        = { version = "1", features = ["rt-multi-thread", "macros"] }
serde        = { version = "1", features = ["derive"] }
reqwest      = { version = "0.12", default-features = false, features = ["json", "rustls-tls"] }
jsonwebtoken = "9"
```
Single `Cargo.lock` + shared `target/` ⇒ manager and admin-api resolve identical
tokio/reqwest/serde. Members reference shared crates with `dep = { workspace = true }`.

### 4.2 Shared `crates/zitadel-auth` (reuse `auth_zitadel.rs`)

Extract `manager/src/auth_zitadel.rs` **verbatim** into a lib crate that both
`manager` and `admin-api` depend on. It already does everything admin-api needs:
JWKS fetch (reqwest), `DecodingKey::from_rsa_components`, `jsonwebtoken::decode` with
`Validation::new(Algorithm::RS256)`, `set_issuer` + `set_audience(project_id)`, and
role extraction from `urn:zitadel:iam:org:project:{project_id}:roles` →
`Principal.has(role)`. admin-api gates operators on `principal.has("chat.admin")`.
`verify_sync()` is intentionally synchronous (no I/O after JWKS cache warm), so call
it directly in an axum `FromRequestParts` extractor or middleware. **Do not duplicate
or re-implement this** — it is the proven source of truth.
(source: D:\projects\llm-chat\manager\src\auth_zitadel.rs)

### 4.3 Concrete crates & versions

| Crate | Version | Role |
|---|---|---|
| `axum` | `0.8` (0.8.x; 0.8.8 reported) | HTTP server (tokio + tower + hyper 1.x) |
| `tokio` | `1` (`rt-multi-thread`,`macros`) | runtime (matches repo) |
| `tower-http` | `0.6` (`cors`,`trace`) | CORS / tracing middleware |
| `tower-sessions` | `0.15` | server-side sessions; cookie holds only session id |
| `tower-sessions-sqlx-store` | `0.15` (`PostgresStore`) | session store on the repo's sqlx 0.8 pool |
| `openidconnect` | `4` (4.0.x; re-exports `oauth2` `^5`) | Auth Code + PKCE + discovery + id-token verify |
| `reqwest` | `0.12` (`json`,`rustls-tls`) | Management-API calls (match repo) |
| `jsonwebtoken` | `9` | RS256 — JWKS verify (via zitadel-auth) **and** SA JWT-bearer assertion |
| `sqlx` | `0.8` (postgres/sqlite, rustls) | reuse the manager's pool/config |

**axum 0.8 breaking changes to note:** path params are `/{id}` and `/{*rest}` (not
`/:id`, `/*rest`); `Option<T>` extractors use `OptionalFromRequestParts`.

**`tower-sessions` 0.15:** `SessionManagerLayer` builder — `with_name`,
`with_secure(bool)`, `with_http_only(bool)`, `with_same_site(cookie::SameSite)`,
`with_domain`, `with_path`, `with_expiry(Expiry::OnInactivity(Duration) | OnSessionEnd
| AtDateTime)`, `with_signed(cookie::Key)` / `with_private(cookie::Key)`. **Default is
`SameSite::Strict` — set `Lax`** for the OIDC redirect (Strict can drop the cookie on
the cross-site redirect back). `PostgresStore::new(pool)` + `store.migrate().await?`
(creates `tower_sessions.session`, data as MessagePack). `MemoryStore` for first-pass dev.

**`openidconnect` 4.x flow:** `CoreProviderMetadata::discover_async(IssuerUrl, &http_client)`
→ `CoreClient::from_provider_metadata(meta, client_id, Some(client_secret)).set_redirect_uri(...)`;
`PkceCodeChallenge::new_random_sha256()`; `client.authorize_url(...)
.set_pkce_challenge(challenge).add_scope(...).url()`; `client.exchange_code(code)
.set_pkce_verifier(verifier).request_async(&http_client).await`; verify id_token via
`client.id_token_verifier()` + `claims(verifier, &nonce)`. **v4 breaking change:**
async calls take an explicit `&http_client` (reqwest adapter), not a closure — build
one `reqwest::Client` (with `redirect::Policy::none()` per SSRF guard) and pass it
everywhere. Default client auth is HTTP Basic = matches `authMethodType=BASIC`;
`.set_auth_type(AuthType::RequestBody)` switches to POST. Persist `pkce_verifier`,
`nonce`, `state` in the tower-session across `/login → /callback`.
Read the `chat.admin` role by reusing the `zitadel-auth` `JwksCache` (lower-risk,
proven) rather than the crate's id_token_verifier path.

**SA JWT-bearer assertion (Rust one-liner vs `provision.py`'s PyJWT):**
`jsonwebtoken::encode(&Header{alg:RS256, kid:Some(key_id), ..}, &Claims{iss:userId,
sub:userId, aud:issuer, iat, exp:iat+3600}, &EncodingKey::from_rsa_pem(pem)?)`.

### 4.4 TLS feature hygiene

Keep **one** TLS backend (rustls) across reqwest, openidconnect, and sqlx.
`openidconnect`'s default reqwest integration can pull `native-tls`; set
`openidconnect = { version="4", default-features=false, features=["reqwest","rustls-tls"] }`
(verify exact feature name) or disable its reqwest feature and pass your own
rustls-configured `reqwest::Client`. **Confirm with `cargo tree -e features -i reqwest`**
that no `native-tls`/`openssl` appears (§6).

### 4.5 Topology

Strongly prefer **same-origin** (Next.js rewrites/proxy `/api/* → axum BFF`) so the
session cookie is `SameSite=Lax`, no CORS layer needed, JS never touches tokens. Only
if separate origins: `tower-http CorsLayer::new().allow_origin(exact origin)
.allow_credentials(true).allow_methods([...]).allow_headers([CONTENT_TYPE])` — wildcard
`*` is rejected with credentials (correct guardrail); cookie then needs
`SameSite=None; Secure` (HTTPS-only, blocked by the local plain-HTTP issuer).
(source: https://docs.rs/axum/latest/axum/;
source: https://docs.rs/tower-sessions/latest/tower_sessions/;
source: https://docs.rs/openidconnect/latest/openidconnect/;
source: https://crates.io/crates/tower-sessions-sqlx-store)

---

## 5. Next.js frontend

### 5.1 Version & runtime

- **Next.js 16** (16.2.7 as of 2026-05; App Router default, **React 19.2**,
  **Node 20.9+**, TypeScript 5.1+, **Turbopack default** — opt out with
  `next dev --webpack`). Breaking: `middleware.ts` → `proxy.ts` (Node runtime);
  `cookies()`/`headers()`/`params`/`searchParams` are now **async** (`await`). Pin
  Next.js 15.x (also App Router + React 19) if 16's async churn is unwelcome.
- **Needs a Node server at runtime** — an authed dashboard (cookies/server
  actions/dynamic) is unsupported by static `output: 'export'`. Package with
  **`output: 'standalone'`** in a `node:20-alpine` (or 22) runner copying
  `.next/standalone` + `.next/static` + `public`, `node server.js`, port 3000.

### 5.2 Cross-origin cookie auth flow

- **`localhost:3000` (Next) ↔ `localhost:{bffPort}` (BFF) is SAME-SITE** — SameSite is
  computed at registrable-domain + scheme (eTLD+1); **port is ignored**. So a
  `SameSite=Lax; HttpOnly; Path=/` cookie **is** delivered on credentialed
  cross-origin fetches in local dev — no `SameSite=None;Secure` needed locally.
  In prod across different registrable domains → `SameSite=None; Secure` (HTTPS)
  mandatory; same registrable domain (sub-domains) → `Lax` still works.
- **Same-site ≠ CORS-exempt** — different port = different **origin**. The BFF must:
  echo the **exact** Next.js origin in `Access-Control-Allow-Origin` (never `*` with
  credentials), set `Access-Control-Allow-Credentials: true`, add `Vary: Origin`, list
  methods/headers, and answer the OPTIONS preflight (JSON body / custom headers trigger
  it). The browser uses `fetch(url, { credentials: 'include' })` on **every** call.
- **Login is a full-page navigation, not fetch.** (1) "Sign in" link → top-level
  `GET {bff}/login`. (2) BFF makes PKCE+state, 302 → Zitadel
  `/oauth/v2/authorize`. (3) user authenticates (MFA at Zitadel). (4) Zitadel 302 →
  `GET {bff}/callback?code&state`. (5) BFF verifies state, exchanges code+verifier at
  `/oauth/v2/token`, validates the JWT (same JWKS/issuer/audience path), checks
  `chat.admin`, mints its **own** opaque session, sets `Set-Cookie: session=...;
  HttpOnly; SameSite=Lax; Path=/` (+Secure prod), 302 → Next.js app. The BFF
  holds/refreshes Zitadel tokens server-side; the browser only ever has the opaque BFF
  session cookie (XSS-safe). The Next.js origin is **not** a Zitadel redirect target,
  so no Zitadel-side config is needed for it.

### 5.3 Component library & components

- **shadcn/ui** (Radix + Tailwind v4, full React 19 support). `npx shadcn@latest init`
  (App Router auto-detected). On npm with React 19 you'll be prompted for
  `--legacy-peer-deps`/`--force` — prefer **pnpm** to avoid. Tailwind v4 (`@theme`,
  OKLCH, `data-slot`); components are copied into the repo (not an npm dep) on Radix +
  lucide-react.
- **Users list = shadcn DataTable on TanStack Table** (`add table` +
  `@tanstack/react-table`; both DataTable and `columns` are `'use client'`).
  `ColumnDef<User>[]` with sorting/filtering/pagination (default pageSize 10)/row
  selection/per-row DropdownMenu actions. Fetch with `credentials:'include'`.
- **Create-user = shadcn Form** (react-hook-form + zod resolver:
  FormField/FormItem/FormControl/FormMessage) inside a **Dialog**; submit POSTs to the
  BFF with `credentials:'include'`.
- **Destructive confirm = AlertDialog** (not plain Dialog) —
  `AlertDialogAction`/`AlertDialogCancel`, `role=alertdialog`. On confirm → DELETE to
  BFF → `router.refresh()` / refetch / TanStack Query invalidation.

### 5.4 Packaging

- Dev: `npm run dev` (Turbopack) on :3000; `NEXT_PUBLIC_API_BASE_URL=http://localhost:{bffPort}`;
  the BFF's CORS allow-origin must include `http://localhost:3000`.
- Compose: `frontend` service, multi-stage Dockerfile (deps → builder → runner),
  `output: 'standalone'`, `node:20-alpine`, expose 3000, `NODE_ENV=production`.
  **`NEXT_PUBLIC_*` is inlined at BUILD time** — build per-env or proxy if the API URL
  differs per environment. (Same-origin proxy via Next `rewrites`/`proxy.ts` is the
  CORS-free alternative.)
(source: https://nextjs.org/blog/next-16;
source: https://nextjs.org/docs/app/getting-started/deploying;
source: https://ui.shadcn.com/docs/components/data-table;
source: https://web.dev/articles/schemeful-samesite)

---

## 6. MUST VERIFY EMPIRICALLY against running Zitadel v3.4.10

Consolidated checklist of every low/medium-confidence item and open question. Treat
the **running stack as the source of truth** — confirm each before building on it.

### 6.1 OIDC WEB app & operator login
- [ ] `POST /management/v1/projects/{pid}/apps/oidc` with `appType=OIDC_APP_TYPE_WEB` +
  `authMethodType=OIDC_AUTH_METHOD_TYPE_BASIC` returns a **non-empty `clientSecret`** —
  and confirm the exact JSON key (`clientSecret` vs `client_secret`).
- [ ] A WEB/BASIC app **accepts (and ideally requires) PKCE**: does `/token` reject a
  missing `code_verifier`, and does it accept `client_secret` + `code_verifier`
  together without erroring on a confidential client?
- [ ] With `urn:zitadel:iam:org:projects:roles` + `accessTokenRoleAssertion=true`, the
  **`chat.admin` role appears in the ACCESS token JWT** (not only id token) under
  `urn:zitadel:iam:org:project:{pid}:roles` **for a HUMAN user grant** (repo only
  proved this for a machine user; project created with `projectRoleAssertion:false`).
- [ ] `http://localhost` (and/or `http://host.docker.internal`) redirect URIs are
  **accepted at runtime** with `devMode=true` (GitHub #9384 shows console-vs-runtime
  validation drift); decide the exact dev redirect host the browser hits.
- [ ] Project audience: confirm whether the BFF needs the **same project as the
  manager** or its own, and that `ZITADEL_AUDIENCE` for admin-api = the project id so
  `validation.set_audience` passes.
- [ ] `end_session` endpoint path (`/oidc/v1/end_session` vs `/oauth/v2/end_session`)
  and whether `id_token_hint` is required for the post-logout redirect to be honored.

### 6.2 Service account & permissions
- [ ] `AddMachineKey` (`POST /users/{id}/keys`) **succeeds with only `user.write`** —
  i.e. an `ORG_USER_MANAGER` SA can mint machine keys (proto auth_option says
  `user.write`, but `defaults.yaml` shows `user.credential.write` only on
  `ORG_OWNER`/`IAM_OWNER`).
- [ ] Exact HTTP method/path of **`ListOrgMemberRoles`** (to enumerate valid org member
  role keys).
- [ ] Org-id resolution: whether the runtime SA needs `x-zitadel-orgid` set, and that
  `GET /auth/v1/users/me` returns `user.details.resourceOwner` (flagged UNVERIFIED in
  `provision.py`).
- [ ] Whether this self-hosted instance **overrides `defaults.yaml`
  RolePermissionMappings** (InternalAuthZ) — if customized, the `ORG_USER_MANAGER`
  permission set may differ from the upstream defaults quoted in §2.3.
- [ ] Policy decision: is **project-role creation** a runtime admin-api responsibility
  (needs `ORG_OWNER` / `project.role.write`) or a one-time provisioner (`IAM_OWNER`)
  responsibility? If runtime, `ORG_USER_MANAGER` is insufficient.
- [ ] Whether to migrate `AddOrgMember` (deprecated) → v2 `CreateAdministrator`, and
  whether that v2 API is fully wired in v3.4.10.

### 6.3 User read / search / grants
- [ ] v2 `emailQuery` field key: `emailAddress` (v2) vs `email` (v1).
- [ ] Exhaustive `TEXT_QUERY_METHOD_*` and `USER_STATE_*` spelling (docs didn't
  re-list them) — confirm via gRPC reflection / OpenAPI on the instance.
- [ ] v2 `POST /v2/users` result top-level uses `userId`/`username` and
  `email.isVerified` exactly (vs v1 `id`/`userName`/`isEmailVerified`).
- [ ] Whether **Authorization Service v2 `ListAuthorizations`**
  (`zitadel.authorization.v2`) is present/enabled in v3.4.10; if not, fall back to v1
  `POST /management/v1/users/grants/_search` for reading the `chat.admin` grant.
- [ ] Whether v2 `ListUsers` without `organizationIdQuery` returns only the token's org
  or instance-wide, and whether the admin token (zitadel-project audience) may list
  across orgs.
- [ ] `grants/_search` with `projectId`+`roleKey=chat.admin` returns the `chat.admin`
  roleKey across all users in the org once an operator is granted it.
- [ ] Pagination defaults/limits (default limit, max limit) and `query.asc` +
  `sortingColumn` behavior.
- [ ] Whether v1 read/search paths honor `x-zitadel-orgid` the same way the
  provisioner's create-calls do.

### 6.4 User creation
- [ ] **Default `accessTokenType`** when omitted on `POST /users/machine` (likely
  BEARER — always set `ACCESS_TOKEN_TYPE_JWT` explicitly).
- [ ] Reproduce that v1 `/users/human` with `initialPassword` **cannot** create an
  immediately-active user with a permanent password (leaves it `initial`).
- [ ] `POST /v2/users/human` returns `{userId}` and accepts
  `password:{password,changeRequired:false}` + `email:{isVerified:true}` → active,
  login-ready user.
- [ ] Whether the unified **`CreateUser`** (`POST /v2/users` or `/v2/users/new`) exists
  at all on v3.4.10 — exact path, machine-branch fields (does the machine oneof carry
  `accessTokenType`?), response key (`id` vs `userId`). Docs suggest **v4-only**.
- [ ] v2 `hashedPassword` shape (`{hash}` with algorithm embedded in the PHC string vs
  a separate algorithm field) and which verifiers are enabled (bcrypt only by default).
- [ ] Invite/init-email behavior: does omitting `password` + `email.sendCode:{}`
  trigger an init email, and is there a separate `CreateInviteCode` flow to use for
  operator onboarding?
- [ ] Whether `x-zitadel-orgid` is required for admin-api user-create calls or the SA's
  own org is used by default.

### 6.5 Roles, grants, idempotency
- [ ] Re-POSTing an existing **project role** and existing **user grant** actually
  return **HTTP 409** (not 200/412).
- [ ] Grant-id field name on add (`userGrantId`) vs search (`id`), and that PUT/DELETE
  accept that same value as `{grantId}`.
- [ ] App-level `accessTokenRoleAssertion:true` is sufficient to get `chat.admin` into
  the access token even though the project has `projectRoleAssertion:false`.
- [ ] Which manager role the admin-api SA needs for `project.role.write` +
  `user.grant.write/delete/read` (ORG_OWNER vs PROJECT_OWNER vs custom), and whether
  `x-zitadel-orgid` is needed when the target user is in the SA's org.
- [ ] `grants/_search` with `projectIdQuery`+`roleKeyQuery` returns grants across all
  users in the org, and how org scoping / `x-zitadel-orgid` affects the result set.
- [ ] Standardize on v1 (proven) vs migrate grants to v2 `AuthorizationService`
  `CreateAuthorization` — confirm v2 behaves identically on v3.4.10 if chosen.

### 6.6 Machine keys & lifecycle
- [ ] `POST .../keys` accepts `expirationDate` (RFC3339) and returns
  `400 INVALID_ARGUMENT` for a past/invalid date.
- [ ] Status code for `DELETE .../keys/{keyId}` on a missing key (likely 404), and for
  deleting an already-deleted user.
- [ ] Exact `List keys` (`_search`) request body shape v1 accepts
  (`{"query":{"offset":"0","limit":100,"asc":true}}` vs flat `{limit,offset}` vs empty
  `{}`) and pagination behavior.
- [ ] `GET` single-key response wrapper key name (`key` vs flat) — docs page
  intermittently 404'd.
- [ ] How long until a **deactivated/locked machine user is actually locked out** given
  local JWKS validation (no introspection) — i.e. token TTL — and whether admin-api
  needs a complementary short-TTL/revocation strategy.
- [ ] Machine-secret deletion verb/path (`DELETE .../users/{userId}/secret`) and that
  generating a new secret invalidates the prior one.
- [ ] If choosing **v2** for keys/lifecycle: re-verify every shape (v2 `AddKey` returns
  `keyContent` not `keyDetails`; paths `/v2/users/...`) — do **not** mix v1 create with
  v2 list.

### 6.7 Rust stack
- [ ] Does `openidconnect` 4.x / `oauth2` 5.x **accept the plain-HTTP issuer**
  `http://host.docker.internal:8080`, or does it reject non-HTTPS issuer/redirect URLs
  without an explicit opt-in? **Likely a local-dev blocker.**
- [ ] `cargo tree -e features -i reqwest` shows **rustls only** (no native-tls/openssl)
  after wiring openidconnect=4 + reqwest(rustls) + sqlx(rustls).
- [ ] `chat.admin` lands in the BFF's id_token/access_token under
  `urn:zitadel:iam:org:project:{pid}:roles` when the WEB app has
  id/accessTokenRoleAssertion=true (manager reads from access token; BFF reads from
  id_token after login).
- [ ] The discovery doc at `{issuer}/.well-known/openid-configuration` advertises
  authorization/token/jwks/end_session endpoints with the `host.docker.internal:8080`
  base, and the **issuer string inside the doc matches `ZITADEL_ISSUER` exactly**
  (else jsonwebtoken issuer validation fails).
- [ ] `tower-sessions` 0.15 + `tower-sessions-sqlx-store` 0.15 **compile against the
  repo's sqlx 0.8** with the rustls runtime (version skew could force a bump).
- [ ] `SameSite=Lax` (vs Strict) survives Zitadel's cross-site 302 back to the BFF
  `/callback`.
- [ ] Exact `openidconnect` 4.0.x API surface (`set_pkce_verifier`, `exchange_code`
  signature, `request_async` vs blocking) against the pinned version; decide reuse of
  `zitadel-auth` `JwksCache` vs the crate's `id_token_verifier`.

### 6.8 Frontend / CORS
- [ ] `tower-http` `allow_credentials(true)` with an exact origin works, and the OPTIONS
  preflight for JSON POST/DELETE from `localhost:3000` returns the expected
  `Access-Control-Allow-Origin` / `Access-Control-Allow-Credentials` headers.
- [ ] BFF session model: confirm the BFF mints its **own opaque** session cookie
  (recommended). If instead storing the Zitadel JWT in the cookie, confirm size stays
  under the ~4KB browser limit (role-claim JWTs can be large).
- [ ] Production domain topology to pin cookie attributes: different registrable
  domains → `SameSite=None; Secure` (HTTPS) mandatory; same registrable domain
  (sub-domains) → `Lax` suffices.
- [ ] `postLogoutRedirectUris` should point at the Next.js app (vs the BFF), and confirm
  the Next.js origin needs no Zitadel-side config (it is not a redirect target).
