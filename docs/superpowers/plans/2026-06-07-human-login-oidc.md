# Human login (OIDC Auth Code + PKCE) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development or superpowers:executing-plans. Steps use `- [ ]`.

**Goal:** Add interactive browser-based human login (Auth Code + PKCE) to the Python interactive client, keep the kabytech machine-key flow for M2M, and have the provisioner register the OIDC app + a demo human user.

**Architecture:** Provisioner creates an OIDC native app (PKCE, JWT tokens) + a demo human user with `chat.user`. The client gains `oidc.py` (PKCE flow), `tokens.py` (keyring-cached refresh tokens), and `login/logout/whoami` CLI commands. Credential selection is role-based: `chat`→human, `ask`→machine. Manager unchanged.

**Tech Stack:** Python 3.9+, `requests`, `pyjwt[crypto]`, `keyring`, stdlib `http.server`/`webbrowser`/`hashlib`/`secrets`; Zitadel Management API v1.

**Source of truth for Zitadel API shapes:** the running Zitadel v3.4.10. Each provisioner task ends by running against the live stack and adjusting field names if the API differs.

---

## Task 1: Provisioner — register the OIDC app

**Files:** Modify `deploy/compose/provisioner/provision.py`; Test `deploy/compose/provisioner/test_provision.py`.

- [ ] **Add `create_oidc_app(token, headers, project_id) -> str`** posting to
  `{ISSUER}/management/v1/projects/{project_id}/apps/oidc` with:
  ```python
  {"name": "llm-chat-cli",
   "redirectUris": ["http://localhost:8477/callback"],
   "postLogoutRedirectUris": ["http://localhost:8477/"],
   "responseTypes": ["OIDC_RESPONSE_TYPE_CODE"],
   "grantTypes": ["OIDC_GRANT_TYPE_AUTHORIZATION_CODE", "OIDC_GRANT_TYPE_REFRESH_TOKEN"],
   "appType": "OIDC_APP_TYPE_NATIVE",
   "authMethodType": "OIDC_AUTH_METHOD_TYPE_NONE",
   "accessTokenType": "OIDC_TOKEN_TYPE_JWT",
   "devMode": True,
   "accessTokenRoleAssertion": True,
   "idTokenRoleAssertion": True}
  ```
  Return `resp.json()["clientId"]`. 200 == success; 409 → SystemExit (clean-boot contract, mirror `create_project`).
- [ ] **Unit test** `test_create_oidc_app_posts_native_pkce_jwt`: monkeypatch `request_with_retry` to assert the URL + the key fields (`appType`, `authMethodType==NONE`, `accessTokenType==JWT`, redirect uri) and that it returns the `clientId`.
- [ ] **Run** `python -m pytest deploy/compose/provisioner/test_provision.py -q`.
- [ ] **Commit** `feat(provisioner): register OIDC native app (PKCE, JWT tokens)`.

## Task 2: Provisioner — demo human user + role grant

**Files:** Modify `provision.py`; Test `test_provision.py`.

- [ ] **Add constants** `DEMO_USERNAME="demo"`, `DEMO_EMAIL="demo@llm-chat.local"`, and a password read from env `DEMO_USER_PASSWORD` (default `Demo-Passw0rd!` for local-dev).
- [ ] **Add `create_human_user(token, headers) -> str`** posting to
  `{ISSUER}/management/v1/users/human` with:
  ```python
  {"userName": DEMO_USERNAME,
   "profile": {"firstName": "Demo", "lastName": "User"},
   "email": {"email": DEMO_EMAIL, "isEmailVerified": True},
   "password": {"password": DEMO_PASSWORD, "changeRequired": False}}
  ```
  Return `userId`; 409 → SystemExit (clean-boot contract).
- [ ] **Reuse `grant_role`** to grant `chat.user` to the human user id.
- [ ] **Unit tests** `test_create_human_user_posts_verified_password` and that the role grant is invoked for the human id.
- [ ] **Run pytest. Commit** `feat(provisioner): create demo human user with chat.user`.

## Task 3: Provisioner — write secrets + wire into main()

**Files:** Modify `provision.py`; Test `test_provision.py`.

- [ ] **In `main()`**, after the existing sequence: call `create_oidc_app`,
  `create_human_user`, `grant_role`; then `write_secret("oidc_client_id", client_id)`,
  `write_secret("demo_user", DEMO_USERNAME)`, `write_secret("demo_password", DEMO_PASSWORD)`.
  Log a final line listing the new artifacts.
- [ ] **Idempotency:** these new creates also live behind the clean-boot contract;
  no `_search` recovery (consistent with today).
- [ ] **Unit test** `test_main_writes_oidc_and_demo_secrets` (monkeypatch the network
  helpers, assert the three new files are written).
- [ ] **Run pytest. Empirically verify against live Zitadel:**
  `docker compose down -v; Remove-Item -Recurse -Force .\secrets; docker compose build zitadel-init; docker compose up -d`; confirm `zitadel-init` exits 0 and `secrets/oidc_client_id`, `secrets/demo_user`, `secrets/demo_password` exist. **If any Zitadel field name is wrong, fix it here and re-run.**
- [ ] **Commit** `feat(provisioner): emit oidc_client_id + demo user creds to secrets`.

## Task 4: Client — `oidc.py` (Auth Code + PKCE)

**Files:** Create `clients/python/llm_chat/oidc.py`; Test `clients/python/tests/test_oidc.py`.

- [ ] **`@dataclass TokenSet`**: `access_token, refresh_token, id_token, expires_at(float)`; `is_expired(skew=30)`.
- [ ] **Pure helpers (unit-tested, no network):**
  - `make_pkce() -> (verifier, challenge)`: `verifier = base64url(secrets.token_bytes(32))`; `challenge = base64url(sha256(verifier))` (S256). No padding.
  - `make_state() -> str`: `base64url(secrets.token_bytes(16))`.
  - `build_authorize_url(endpoint, client_id, redirect_uri, scope, challenge, state)`.
  - `parse_callback(path) -> {code,state}` (from the redirect query).
  - `discover(issuer)` → endpoints from `/.well-known/openid-configuration` (with fallback to `{issuer}/oauth/v2/authorize|token|revoke`).
- [ ] **`login(issuer, client_id, project, *, port=8477, open_browser=True) -> TokenSet`**: start a one-shot loopback server, open browser, capture `code` (verify `state`), exchange at `/token` with `code_verifier`. Scope = `openid profile email offline_access urn:zitadel:iam:org:project:id:{project}:aud urn:zitadel:iam:org:projects:roles`.
- [ ] **`refresh(issuer, client_id, refresh_token) -> TokenSet`** (grant_type=refresh_token).
- [ ] **`revoke(issuer, client_id, token)`** (best-effort).
- [ ] **Unit tests:** S256 challenge matches a known vector; `parse_callback` extracts code+state; `build_authorize_url` contains `code_challenge_method=S256` + state; token exchange (mocked `requests.post`) returns a populated `TokenSet`; `state` mismatch raises.
- [ ] **Run pytest. Commit** `feat(client): OIDC Auth Code + PKCE flow (oidc.py)`.

## Task 5: Client — `tokens.py` (secure cache + refresh)

**Files:** Create `clients/python/llm_chat/tokens.py`; Test `clients/python/tests/test_tokens.py`.

- [ ] **Store** the refresh token via `keyring.set_password(SERVICE, issuer, refresh_token)`; fallback to a `0600` JSON file under `platformdirs`/`~/.config/llm-chat/` if keyring raises. Access/id tokens cached in a sidecar `0600` file (access tokens are short-lived; never the refresh token in plaintext when keyring works).
- [ ] **`TokenStore(issuer, client_id)`** with `load() -> TokenSet|None`, `save(TokenSet)`, `clear()`, and `valid_access_token(refresh_fn) -> str` (refresh when expired, persist the new set).
- [ ] **Unit tests** with a fake keyring backend (monkeypatch `keyring.set/get/delete_password`): save→load round-trip; expired access token triggers `refresh_fn`; `clear` removes both keyring + file; file fallback path when keyring raises.
- [ ] **Run pytest. Commit** `feat(client): keyring-backed token store with refresh`.

## Task 6: Client — CLI login/logout/whoami + credential resolver

**Files:** Modify `clients/python/llm_chat/cli.py`, `config.py`, `auth.py`; Test `clients/python/tests/test_cli_resolver.py`.

- [ ] **`config.py`:** add `--auth {user,machine}` (default None → role-based), `--oidc-port` (default 8477); read `oidc_client_id`/`demo_*` from secrets via `auth._read_secret_file`.
- [ ] **`cli.py`:** subcommands `login` (run `oidc.login`, `TokenStore.save`, print `whoami`), `logout` (`oidc.revoke` + `TokenStore.clear`), `whoami` (decode id token, print sub/email/roles).
- [ ] **Resolver `resolve_token_provider(args, command)`:**
  - `chat` (or `--auth user`): a provider returning `TokenStore.valid_access_token(refresh)`; if no cached token → run `login` first (or error with "run `llm-chat login`").
  - `ask`/legacy (or `--auth machine`): the existing machine-key provider.
- [ ] **Unit test** the resolver picks user-vs-machine per command/flag (mock TokenStore + machine token).
- [ ] **Run pytest. Commit** `feat(client): login/logout/whoami + role-based credential selection`.

## Task 7: Packaging + docs

**Files:** Modify `pyproject.toml`, `requirements.txt`, `clients/python/README.md`, `deploy/compose/README.md`.

- [ ] **Add `keyring>=24`** (and `platformdirs>=4`) to deps.
- [ ] **README (client):** document `llm-chat login` / `logout` / `whoami`, the `--auth` flag, the human-vs-machine model, and the keyring storage.
- [ ] **README (deploy/compose):** add the demo-user login steps and note `secrets/oidc_client_id` + `secrets/demo_user|demo_password`.
- [ ] **Commit** `docs: human login usage + keyring dep`.

## Task 8: End-to-end verification

- [ ] **Unit:** `python -m pytest` in `clients/python` and the provisioner — all green.
- [ ] **Provisioner integration:** clean boot; assert the OIDC app + demo user exist (Management API GET) and `chat.user` is granted to the demo user.
- [ ] **Manual browser login:** `llm-chat login` → sign in as `demo` → `whoami` shows the demo user + `chat.user` → `llm-chat chat` round-trips on the human token. (Browser step can't be automated; record the result.)
- [ ] **Machine path unchanged:** `llm-chat ask --send "hi"` still uses the kabytech key.
- [ ] **No commit** (verification only); fix any defect in its owning task.

---

## Self-review notes
- Spec §2.1 (OIDC app, demo user) → T1–T3. §2.2 (oidc, tokens, cli) → T4–T6. §3 security → T4 (PKCE/state), T5 (keyring), T6 (logout revoke). §5 testing → per-task unit + T8. §6 files → all covered. Manager unchanged (§2.3) — no task, correct.
- Zitadel field names are the main risk; T1–T3 verify empirically against the live instance and fix inline.
