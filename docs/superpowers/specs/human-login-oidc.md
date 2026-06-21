# Human login (OIDC Auth Code + PKCE)

**Date:** 2026-06-07 · **Status:** Implemented. Built first in the **Python** client (`clients/python/`), then ported to the **Rust** client (`clients/rust/src/oidc.rs` etc. — explicitly "Port of `oidc.py`/`cli.py`"), which is the fuller current client. Provisioner, both clients' `oidc`/`tokens`/`login`/`logout`/`whoami`, and the shared verifier all match this design; a few details evolved (noted inline). Note: `docs/architecture.md` is **stale** here — it still calls Python "the reference client" doing "JWT-bearer only" and omits the Rust client; the code contradicts that.

Interactive **human login** for the chat client via OAuth2 **Authorization Code + PKCE** (browser-based). The existing **machine-key (kabytech) flow** stays for machine-to-machine callers. The manager is **unchanged**: it already validates any JWT (JWKS + issuer + audience + expiry) and gates on the `chat.user` role.

## Two identities, one gate

| | kabytech (machine) | human user |
|---|---|---|
| Who | a microservice | a person at a terminal |
| Authn | machine key → JWT-bearer assertion | browser login (Auth Code + PKCE) |
| Authz | `chat.user` granted to the SA | `chat.user` granted to the user |
| Used by | `ask`, legacy shim, SDKs, CI | `llm-chat chat` (the REPL) |

Both obtain a **JWT access token** and pass the same manager check (`verify_sync` + `principal.has("chat.user")`; roles confirmed in `crates/zitadel-auth/src/lib.rs`).

**Credential selection** (override anywhere with `--auth {user,machine}`):
- `llm-chat chat` → **user token required**; auto-runs login if none cached. Never falls back to the machine key.
- `llm-chat ask` / legacy `llm_chat_client.py` → **machine key** by default.

## Components

### Provisioner (`deploy/compose/provisioner/provision.py`)

1. **OIDC public app** `llm-chat-cli` — `POST /management/v1/projects/{pid}/apps/oidc`:
   `appType: OIDC_APP_TYPE_NATIVE`, `authMethodType: OIDC_AUTH_METHOD_TYPE_NONE` (PKCE, no secret),
   `grantTypes: [AUTHORIZATION_CODE, REFRESH_TOKEN]`, `responseTypes: [CODE]`,
   `accessTokenType: OIDC_TOKEN_TYPE_JWT` (opaque tokens give the manager a 401 — same lesson as the machine user; note the OIDC-app enum differs from the machine-user `ACCESS_TOKEN_TYPE_JWT`),
   `redirectUris: ["http://localhost:8477/callback"]`, `postLogoutRedirectUris: ["http://localhost:8477/"]`.
   → writes `secrets/oidc_client_id`.
2. **Demo humans** (`POST /v2/users/human`, then grant `chat.user` via `users/{id}/grants`).
   *Evolved from the original single demo user:* now provisions **two** — `chatter` (chat.user only: `/chat`, not the Console) and `admin` (chat.user + chat.admin: also the Console). A separate confidential WEB OIDC app `chat-admin-api` (BASIC, redirect `:3000/callback`) serves the admin-api and is out of scope here.

Idempotent clean-boot contract (`down -v` + delete `secrets/`); 409 on re-run is surfaced loudly.

### Client (`clients/python/llm_chat/`)

- **`oidc.py`** — Auth Code + PKCE: discover endpoints from `{issuer}/.well-known/openid-configuration` (Zitadel `/oauth/v2/*` fallback if discovery fails); generate PKCE verifier/challenge (**S256**) + random `state`; one-shot loopback server on `127.0.0.1:8477`; open browser to `/authorize`; verify `state` on the callback; exchange `code`+`verifier` at `/token` → `TokenSet` (access/refresh/id). `revoke()` for logout. Scope:
  `openid profile email offline_access urn:zitadel:iam:org:project:id:<project>:aud urn:zitadel:iam:org:projects:roles`.
  (Redirect URI uses `localhost`, not `127.0.0.1`, to match the registered app.)
- **`tokens.py`** — `TokenStore`: refresh token in the **OS keyring** (Credential Manager / Keychain / libsecret), `0600`-file fallback if unavailable. Access/id tokens cached in a `0600` sidecar file and refreshed within a skew window. `clear()` on logout.
- **`cli.py`** — subcommands `login`, `logout` (revoke + clear), `whoami` (decode cached id token); credential resolver: `chat`→user (auto-login), `ask`→machine, `--auth` override.
- Deps: `keyring`, `platformdirs`.

### Manager — no change.

## Security

- PKCE **S256** (public client, no secret); `state` (CSRF) rejected on mismatch; **loopback** redirect only (RFC 8252).
- Short-lived access tokens + `offline_access` refresh; refresh token in OS keyring (`0600` fallback).
- **JWT** access tokens (manager validates locally, no introspection); `logout` revokes server-side; MFA handled entirely in the browser by Zitadel.
- **Local-dev caveat:** the stack issuer is plain HTTP. **Production requires HTTPS** for issuer and redirect.

## Error handling

Not logged in (chat) → run login (print URL if browser can't open). Access token expired → silent refresh; refresh failed → re-login. `state` mismatch / denial → clear error, exit non-zero (`EXIT_AUTH=3`). Keyring unavailable → file fallback, warn once.

## Out of scope (YAGNI)

Self-registration, password-reset UI, multiple concurrent cached users, token introspection, a GUI. Demo users are for local testing; real deployments manage users in the Zitadel console.
