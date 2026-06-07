# Human login (OIDC Auth Code + PKCE) for the interactive client — design

**Date:** 2026-06-07
**Status:** approved (build without further check-ins, per user)

## 1. Goal & scope

Add **interactive human login** to the Python *interactive* chat client, using the
OAuth2 **Authorization Code flow with PKCE** (browser-based). Keep the existing
**machine-key (kabytech) flow** for **machine-to-machine** callers. The manager
is **unchanged** — it already validates any JWT (JWKS + issuer + audience +
expiry) and authorizes on the `chat.user` role.

### Two identities, one gate

| | kabytech (machine) | human user |
|---|---|---|
| Who | a microservice calling the chat service | a person at a terminal |
| Authn | machine key → JWT-bearer assertion → token | browser login (Auth Code + PKCE) |
| Authz | `chat.user` role granted to the SA | `chat.user` role granted to the user |
| Used by | `ask` one-shot, legacy shim, SDKs, CI | `llm-chat chat` (the REPL) |

Both obtain a **JWT access token** and pass the **same** manager check
(`verify_sync` + `principal.has("chat.user")`).

### Role-based credential selection (not "auto-pick")

- **`llm-chat chat`** (interactive REPL) → **requires a human user token**. If none
  is cached/valid, it runs the login flow. It must **never** fall back to the
  machine key (a human must not impersonate the microservice).
- **`llm-chat ask`** / legacy `llm_chat_client.py` → **machine key** (kabytech) by
  default; `--auth user` lets a human do a one-shot on their own identity.
- Explicit override everywhere: `--auth {user,machine}`.

## 2. Components

### 2.1 Provisioner (`deploy/compose/provisioner/provision.py`)

Adds two creations after the existing project/role/machine-user sequence:

1. **OIDC application** — `POST /management/v1/projects/{pid}/apps/oidc`:
   - `appType: OIDC_APP_TYPE_NATIVE` (public client, **no secret**)
   - `grantTypes: [OIDC_GRANT_TYPE_AUTHORIZATION_CODE, OIDC_GRANT_TYPE_REFRESH_TOKEN]`
   - `responseTypes: [OIDC_RESPONSE_TYPE_CODE]`
   - `authMethodType: OIDC_AUTH_METHOD_TYPE_NONE` (PKCE; no client secret)
   - `accessTokenType: OIDC_TOKEN_TYPE_JWT` (manager validates via JWKS — same
     lesson as the machine user; opaque tokens give 401)
   - `redirectUris: ["http://localhost:8477/callback"]`
   - `postLogoutRedirectUris: ["http://localhost:8477/"]`
   - `devMode: true` (allow http loopback redirect in local-dev)
   - → write `secrets/oidc_client_id`.
2. **Demo human user** — `POST /management/v1/users/human` with userName, profile,
   verified email, and an initial **permanent** password; then **grant `chat.user`**
   (same `users/{id}/grants` endpoint as the machine user).
   - → write `secrets/demo_user`, `secrets/demo_password`.

Exact request/response field names are **verified empirically against the running
Zitadel v3.4.10** during implementation (the running stack is the source of
truth); the design fixes intent, not the precise JSON. Idempotency: same
clean-boot contract as today (`down -v` + delete `secrets/`); 409s on re-run are
surfaced loudly, not guessed (matches the existing `_search`-UNVERIFIED stance).

### 2.2 Client (`clients/python/llm_chat/`)

- **`oidc.py`** — the Auth Code + PKCE flow:
  1. Resolve endpoints from `{issuer}/.well-known/openid-configuration`.
  2. Generate PKCE `code_verifier` + `code_challenge` (**S256**) and a random
     **`state`**.
  3. Start a one-shot loopback HTTP server on `127.0.0.1:8477`.
  4. Open the browser to `/authorize` (client_id, redirect_uri, scope, S256
     challenge, state).
  5. Receive the redirect; **verify `state`**; exchange `code` + `verifier` at
     `/token` → access + refresh + id tokens.
  6. Return a `TokenSet` (access, refresh, expiry).
  - `scope = "openid profile email offline_access "` +
    `"urn:zitadel:iam:org:project:id:<project>:aud urn:zitadel:iam:org:projects:roles"`.
- **`tokens.py`** — secure cache:
  - Refresh token stored in the **OS keyring** (`keyring`: Windows Credential
    Manager / macOS Keychain / libsecret); **fallback** to a `0600` file under the
    user config dir if keyring is unavailable.
  - Access token kept in memory; refreshed via the refresh token when within a
    skew window of expiry.
  - `logout()` **revokes** the refresh token at `{issuer}/oauth/v2/revoke` and
    clears the cache.
- **`cli.py`** — new subcommands + resolver:
  - `llm-chat login` (browser flow), `llm-chat logout` (revoke + clear),
    `llm-chat whoami` (show the cached principal from the id token).
  - Credential resolver per §1: `chat`→user (auto-login), `ask`→machine,
    `--auth` overrides.
- New dependency: `keyring>=24`.

### 2.3 Manager

No change. (Confirmed: `auth_zitadel.verify_sync` + `principal.has("chat.user")`
are principal-agnostic.)

## 3. Security checklist (the "highly secure / best practice" requirement)

- PKCE **S256** (public client, no secret in the CLI).
- **`state`** parameter (CSRF) on the callback; reject mismatch.
- **Loopback** redirect `127.0.0.1` only (RFC 8252 native-app best practice).
- Short-lived **access tokens** + **refresh tokens** via `offline_access` (no
  constant re-login).
- Refresh token in the **OS keyring**, `0600`-file fallback; access token never
  written to disk.
- **JWT** access tokens so the manager validates locally (no token leakage to an
  introspection endpoint).
- `logout` **revokes** the refresh token server-side.
- **MFA** is enforced/handled entirely in the browser by Zitadel — transparent to
  the CLI.
- **Caveat (local-dev only):** the stack issuer is **plain HTTP** (no TLS). This
  is the single non-production aspect, inherited from the compose stack's
  local-dev design. **Production requires HTTPS** for the issuer and redirect.

## 4. Error handling

- Not logged in (chat) → run `login`; if the browser can't open, print the URL.
- Access token expired → silent refresh; refresh failed/expired → prompt re-login.
- `state` mismatch / user denies → clear error, exit non-zero (auth exit code 3).
- Manager 403 (no `chat.user`) → explain the user lacks the role; exit 5.
- Keyring unavailable → fall back to file, warn once.

## 5. Testing

- **Unit:** PKCE verifier/challenge (S256 correctness), `state` generation +
  mismatch rejection, `TokenSet` expiry/refresh logic, keyring cache (mocked),
  credential resolver (chat→user, ask→machine, `--auth` override),
  OIDC-config discovery parsing.
- **Integration:** provisioner creates the OIDC app + demo user (assert via the
  Management API against the live Zitadel); `whoami`/token-exchange unit-level
  with a mocked token endpoint.
- **Manual:** full browser `login` → `chat` round-trip on the human identity
  (browser step can't be fully automated; documented in the README).

## 6. Files

**New:** `clients/python/llm_chat/oidc.py`, `clients/python/llm_chat/tokens.py`,
`clients/python/tests/test_oidc.py`, `clients/python/tests/test_tokens.py`.
**Changed:** `provision.py` (+ its tests), `llm_chat/cli.py`, `llm_chat/config.py`
(`--auth`, redirect port), `llm_chat/auth.py` (machine path stays; shared bits),
`pyproject.toml`/`requirements.txt` (`keyring`), `clients/python/README.md`,
`deploy/compose/README.md` (login steps + the demo user), the provisioner
Dockerfile (no new deps — `requests` already present).

## 7. Out of scope (YAGNI)

User self-registration, password reset UI, multiple concurrent cached users,
token introspection, a GUI. The demo user is for local testing; real deployments
manage users in the Zitadel console.
