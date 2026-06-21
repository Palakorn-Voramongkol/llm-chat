# Docker Compose stack

**Status:** implemented — see `docs/architecture.md` for the current, canonical stack. This file records the design decisions and gotchas behind the local-dev Compose stack.

## 1. Summary

A `docker compose up` that boots the **server side** of `llm-chat` on a developer's Docker Desktop for Windows. Containers: **postgres**, **zitadel** (self-hosted OIDC IdP), **zitadel-init** (one-shot provisioner), **manager** (Rust message router), **admin-api** (operator BFF), **admin-web** (Console UI). The **worker runs natively on the Windows host** so it executes the real `claude` binary against the user's own `~/.claude` and webview; it is reached from the manager container at `host.docker.internal:7878`. The Python reference client (`clients/python/llm_chat_client.py`) also runs on the host.

The linchpin is a single issuer string, `http://host.docker.internal:8080`, that resolves identically from the host (where the client fetches a token) and from inside containers (where the manager/admin-api validate `iss` and fetch JWKS).

**Local-dev playground only** — plain-HTTP issuer, non-`Secure` cookies, insecure placeholder defaults. Never expose to a real network.

The three **network-address** vars (`LLM_CHAT_WS_BIND`, `MANAGER_BIND`, `MANAGER_BACKEND_HOST`) are **required, env-driven, with no hardcoded default in code** — a missing/empty value makes the binary **fail fast** at startup naming the var. The two **mode-toggle** vars are presence-based and backward-compatible: `MANAGER_BACKEND_PORTS` unset = spawn local workers; `LLM_CHAT_AUTH_TOKEN` unset = random token.

## 2. Goals / Non-Goals

**Goals.** One `docker compose up` brings up the stack healthcheck-gated and in order. The provisioner auto-creates the Zitadel project, roles, machine users, grants, OIDC app, and downloadable keys with no manual console clicks. Host worker and containers share one auth token and one OIDC issuer for an end-to-end round-trip. Address vars are required/env-driven; mode toggles stay backward-compatible.

**Non-Goals.** Not production: issuer is `http://` (`ExternalSecure=false`), no TLS, insecure `.env.example` defaults. Not running the worker in a container (it needs a real display/webview and the user's `~/.claude`). Not multi-host / HA / reverse-proxy. Not changing the wire protocol, `/chat` Q→A semantics, or JWKS verification logic.

## 3. Architecture & Topology

The manager validates client JWTs against Zitadel's JWKS and requires project role `chat.user` (encoded under `urn:zitadel:iam:org:project:<project_id>:roles`). The admin-api requires `chat.admin` for operators. The Python client mints JWTs via the JWT-bearer flow using `kabytech`'s JSON key. For the `iss` claim to match on both sides of the trust boundary, host and containers must name the issuer with the **exact same literal string**.

**The linchpin:** `http://host.docker.internal:8080`.
- Under Docker Desktop for Windows, `host.docker.internal` resolves automatically inside Linux containers (no `extra_hosts` needed) **and** from the native Windows host (Docker maintains the Win32 hosts entry).
- Zitadel publishes `8080:8080` on all interfaces (not loopback-only), so both vantage points hit the same Zitadel. Zitadel is told `ExternalDomain=host.docker.internal`, `ExternalPort=8080`, `ExternalSecure=false`, so its discovery doc advertises exactly `http://host.docker.internal:8080`.

```
WINDOWS HOST (Docker Desktop)
  Python client  ── (1) token: POST host.docker.internal:8080/oauth/v2/token
                 ── (2) chat:  ws://localhost:7777/chat  (Bearer <JWT>)
  Browser        ── Console:   http://localhost:3000  (operator OIDC login)
  run-worker.ps1 → worker.exe (real claude + ~/.claude), WS listen 0.0.0.0:7878

  ╔════════════ docker compose network ════════════╗
  ║ postgres:5432 ◄ zitadel:8080 ◄ zitadel-init (1-shot) ║
  ║                         │ writes ./secrets/* + /out/manager.generated.env ║
  ║ manager:7777   (MANAGER_BIND=0.0.0.0)            ║
  ║ admin-api:7676 (ADMIN_BIND_ADDR=0.0.0.0:7676)    ║
  ║ admin-web:3000 (proxies /api,/login,... → admin-api) ║
  ╚══════════════════════════════════════════════════╝
  (manager → host worker) ws://host.docker.internal:7878/{control,s,qa,/}
```

`MANAGER_BACKEND_PORTS` is comma-separated by contract but here holds exactly `7878` — no fan-out; the single host worker serves all sessions.

## 4. Components

### 4.1 postgres
- **Image:** `postgres:17-alpine`. Backing store for Zitadel only — no `llm-chat` data.
- **Env:** `POSTGRES_USER=postgres`, `POSTGRES_PASSWORD=${POSTGRES_PASSWORD}`, `POSTGRES_DB=postgres` (admin/bootstrap DB Zitadel's init connects to).
- **Healthcheck:** `pg_isready -U postgres -d postgres` (5s/5s/20/10s). **Volume:** `pgdata` → `/var/lib/postgresql/data`.
- **Postgres wiring (B, shipped):** Zitadel's init uses the `postgres` admin connection to auto-create a `zitadel` database and an unprivileged `zitadel_user`, then runs as that user (least-privilege). The discrete admin/user split with per-role `SSL_MODE=disable` is the chosen wiring; the single-superuser alternative was rejected.

### 4.2 zitadel
- **Image:** **pinned** `ghcr.io/zitadel/zitadel:v3.4.10` (never `:latest` — the v1 Management-API endpoints the provisioner calls are deprecated and can drift between majors; re-verify the call surface on any bump). This tag is **distroless** (no shell, no `wget`/`curl`).
- **Command:** `start-from-init --masterkeyFromEnv --tlsMode disabled` (init → setup → serve; `--tlsMode disabled` is required to serve the plain-HTTP issuer).
- **`user: root`** (local-dev): the `FIRSTINSTANCE` setup writes the admin-SA key into the root-owned `machinekey` volume; the image's default non-root user can't create that file, so `03_default_instance` would fail with "permission denied" (then mask as "instance domain AlreadyExists" on restart).
- **Key env:**
  - `ZITADEL_MASTERKEY=${ZITADEL_MASTERKEY}` — **exactly 32 chars**; one-shot, irreversible (changing it after first init loses encrypted data).
  - `ZITADEL_EXTERNALDOMAIN=host.docker.internal`, `ZITADEL_EXTERNALPORT=8080`, `ZITADEL_EXTERNALSECURE=false`.
  - `ZITADEL_TLS_ENABLED=false` — **required by the healthcheck** (see below), separate from `EXTERNALSECURE`.
  - Postgres wiring (B): `ZITADEL_DATABASE_POSTGRES_HOST=postgres`, `_PORT=5432`, `_DATABASE=zitadel`, `_USER_USERNAME=zitadel_user`, `_USER_PASSWORD=${POSTGRES_PASSWORD}`, `_USER_SSL_MODE=disable`, `_ADMIN_USERNAME=postgres`, `_ADMIN_PASSWORD=${POSTGRES_PASSWORD}`, `_ADMIN_SSL_MODE=disable`.
  - Bootstrap admin SA (first init only, writes `IAM_OWNER` JSON key): `ZITADEL_FIRSTINSTANCE_ORG_MACHINE_MACHINE_USERNAME=zitadel-admin-sa`, `..._MACHINE_NAME=Admin`, `..._MACHINEKEY_TYPE=1` (1 = JSON), `ZITADEL_FIRSTINSTANCE_MACHINEKEYPATH=/machinekey/zitadel-admin-sa.json`. (Note: double `MACHINE_MACHINE` in user fields vs single `MACHINEKEY`; `MACHINEKEYPATH` is top-level under `FirstInstance`.)
- **Ports:** `8080:8080` (all interfaces — **not** `127.0.0.1:8080:8080`, or containers reaching `host.docker.internal` get connection-refused).
- **Healthcheck:** `["CMD", "/app/zitadel", "ready"]` (exec form — distroless has no shell, so an HTTP probe of `/debug/healthz` is impossible). Per upstream issue #9495 the `ready` subcommand probes HTTPS even when `EXTERNALSECURE=false`; **`ZITADEL_TLS_ENABLED=false` is what flips it to HTTP** and makes the check pass. Interval 5s/5s/30/30s.
- **Volume:** `machinekey` → `/machinekey` (shared read-only with the provisioner). **Restart:** `unless-stopped`.

### 4.3 zitadel-init (provisioner)
- **Image:** built from `deploy/compose/provisioner/Dockerfile`, base `python:3-slim`, deps `pyjwt[crypto]` (PyJWT + `cryptography` for the RS256 backend) and `requests`.
- **Purpose:** one-shot, **idempotent**. Using the bootstrap admin key it mints a Management-API token (JWT-bearer), then provisions the home `llm-chat` project and everything the manager, admin-api, and clients need. Scripts: `provision.py` (the main run), plus one-off siblings `new_app.py` (create a new app/project, grant the runtime SA `PROJECT_OWNER` on it via the bootstrap key) and `org_rename.py` (rename the org via the bootstrap key — the runtime SA is deliberately too least-privilege to do it).
- **Env:** `PROVISION_ISSUER=http://host.docker.internal:8080`; `PROVISION_ENABLE_AUDIT` (default `0`; `1` grants the admin SA instance-level `IAM_OWNER_VIEWER` so the Console's Audit page works — least-privilege, off by default).
- **Dependencies:** `zitadel` healthy. **Restart:** `"no"`. **Volumes:** `machinekey` (ro), `./secrets` → `/secrets`, `genenv` → `/out`.
- **Outputs (to `./secrets` bind mount unless noted):**
  - `kabytech-key.json` — the M2M client's `--key-file`.
  - `project_id`, `kabytech_user_id`.
  - `admin-api-key.json` — the runtime admin-api SA JSON key.
  - `admin_oidc_client_id`, `admin_oidc_client_secret` — the operator-login OIDC app creds.
  - `/out/manager.generated.env` (named volume `genenv`, shared with manager + admin-api) — `ZITADEL_PROJECT_ID=<id>` and `ZITADEL_AUDIENCE=<id>` (audience = project id).

**What it creates** (base path `/management/v1`; treat HTTP **409** = `ALREADY_EXISTS` as "already provisioned, continue"):
1. Project `llm-chat` → persist `<projectId>`.
2. Role `chat.user` (`{"roleKey":"chat.user","displayName":"Chat User","group":""}`).
3. Role `chat.admin` (`displayName "Chat Admin"`) — the role admin-api authorizes operators on.
4. Machine user `kabytech` (the reference M2M client) + its JSON key (returned **once**, base64 in `keyDetails` — decode and write immediately, it can't be retrieved later).
5. Runtime admin-api SA `chat-admin-api` + JSON key (`admin-api-key.json`), an OIDC app for operator login (`accessTokenRoleAssertion=true` so `chat.admin` rides in the access token), and its client id/secret.
6. Role grants: `kabytech` → `chat.user`; the admin SA → `chat.user`+`chat.admin` (the Console Sessions page reads the manager's `chat.admin`-only `/control`). Plus demo human users (`chatter` = `chat.user`; `admin` = `chat.user`+`chat.admin`) for the Console/CLI login flows.
7. Write all secrets and `/out/manager.generated.env`; exit 0.

**Robustness gotchas:**
- **Retry every HTTP call.** Even after the healthcheck passes, token/Management endpoints briefly return 5xx / 401 / 403 on a freshly-bootstrapped instance. Retry on connection errors, 5xx, and (briefly) 401/403; **do not** retry 409 (success signal) or 400/404 (deterministic bugs).
- **Token mint:** `POST /oauth/v2/token`, `grant_type=urn:ietf:params:oauth:grant-type:jwt-bearer`, scope `openid profile urn:zitadel:iam:org:project:id:zitadel:aud`. **Scope trap:** the literal `zitadel` targets Zitadel's own internal project so the Management API accepts the token — do **not** substitute the `llm-chat` project id (contrast the *client* scope in §7.2). Assertion JWT: header `{"alg":"RS256","kid":<admin keyId>}`, payload `iss=sub=<userId>`, `aud=http://host.docker.internal:8080`, `iat/exp` ≤ 1 h.
- **Org context:** send `x-zitadel-orgid: <orgId>` (the SA's own org, from `GET /auth/v1/users/me` → `user.details.resourceOwner`) on every Management call. (Omitting it falls back to the SA's org too.)
- **Key idempotency guard (survives `down -v`):** `./secrets` is a host bind mount that **survives `docker compose down -v`** while the Zitadel DB and `machinekey` volume are wiped. A plain "skip if file exists" would leave a stale key whose `userId` no longer exists → silent auth failure. So: if `kabytech-key.json` exists, compare its on-disk `userId` against the `kabytech` user resolved this run — match ⇒ skip (true re-run), mismatch ⇒ regenerate. The simpler operator path is to delete `./secrets` on a clean reset (§9).

### 4.4 manager
- **Image:** `deploy/compose/manager.Dockerfile` (multi-stage `rust:1-bookworm` build of `./manager` → `debian:bookworm-slim` + `ca-certificates`). Entrypoint `deploy/compose/entrypoint.sh` sources `/out/manager.generated.env`, **validates** the three Zitadel vars, then `exec`s the binary.
- **Purpose:** message router. Verifies client JWTs against Zitadel JWKS, requires `chat.user`, bridges `/chat`, `/control`, `/s/<sid>`, `/qa/<sid>`, `/` to the **host** worker.
- **Key env:** `ZITADEL_ISSUER=http://host.docker.internal:8080`, `LLM_CHAT_AUTH_TOKEN=${LLM_CHAT_AUTH_TOKEN}`, `MANAGER_BIND=0.0.0.0`, `MANAGER_BACKEND_HOST=host.docker.internal`, `MANAGER_BACKEND_PORTS=7878`, `RUST_LOG` (default `info`). `ZITADEL_PROJECT_ID`/`ZITADEL_AUDIENCE` come from `/out/manager.generated.env`.
- **Ports:** `7777:7777`. **Dependencies:** `zitadel-init` `service_completed_successfully`. **Restart:** `unless-stopped` (so the manager survives the transient window before the host worker is up; the startup probe is fatal — §5/§7.1). **No healthcheck** (nothing depends on it; `/dev/tcp` probes are unreliable under dash). **Volume:** `genenv` → `/out` (ro).
- **External-backend mode:** because `MANAGER_BACKEND_PORTS=7878` is set, the manager **does not spawn** a worker (it has no worker binary) and treats `7878` on `MANAGER_BACKEND_HOST` as an already-running backend.

**`entrypoint.sh` fails fast on incomplete Zitadel config (load-bearing).** `ZitadelConfig::from_env()` (`manager/src/auth_zitadel.rs`) is all-or-nothing — missing any of `ZITADEL_ISSUER`/`ZITADEL_AUDIENCE`/`ZITADEL_PROJECT_ID` makes the manager **silently fall back to shared-token auth** (warn-log only), after which JWT clients get a confusing 401. Since these vars are split across `.env` and `/out/manager.generated.env`, a half-written generated file would degrade invisibly. The entrypoint therefore asserts all three non-empty and `exit 1` otherwise:

```sh
#!/bin/sh
set -e; set -a; . /out/manager.generated.env; set +a
: "${ZITADEL_ISSUER:?missing — refusing to start in shared-token mode}"
: "${ZITADEL_PROJECT_ID:?missing from manager.generated.env}"
: "${ZITADEL_AUDIENCE:?missing from manager.generated.env}"
exec /usr/local/bin/llm-chat-manager
```

### 4.5 admin-api (operator BFF)
- **Image:** `deploy/compose/admin-api.Dockerfile` (Rust/axum). The **only** component that calls Zitadel's Management/Admin v1 APIs; admin-web never talks to Zitadel directly.
- **Auth:** OIDC operator login (browser `/login` → `/callback`); requires `chat.admin` in the operator access token (`admin-api/src/auth.rs`) or returns 403. To reach the manager's `chat.admin` `/control`, it mints its own SA token.
- **Key env:** `ZITADEL_ISSUER=http://host.docker.internal:8080` (fails fast if the discovery doc's issuer differs), `ADMIN_BIND_ADDR=0.0.0.0:7676`, `ADMIN_SA_KEY_PATH=/secrets/admin-api-key.json`, `ADMIN_OIDC_CLIENT_ID_FILE` / `ADMIN_OIDC_CLIENT_SECRET_FILE` (from `./secrets`), `ADMIN_PUBLIC_ORIGIN=http://localhost:3000`, `ADMIN_ALLOWED_ORIGIN=http://localhost:3000`, `ADMIN_SESSION_KEY=${ADMIN_SESSION_KEY}`, `ADMIN_COOKIE_SECURE=false`, `MANAGER_CONTROL_URL=ws://manager:7777/control`.
  - **`ADMIN_PUBLIC_ORIGIN` must be `:3000`, not the BFF's `:7676`:** admin-web same-origin-proxies the OIDC nav (`/login`,`/callback`,`/logout`), so the registered redirect_uri must stay on `:3000`. Otherwise Zitadel 302s the browser straight to `:7676`, bypassing the proxy and dropping the pre-auth cookie (scoped to `:3000`) → permanent "state mismatch" 403 at `/callback`.
  - **`ADMIN_COOKIE_SECURE=false`** for the plain-HTTP local stack — otherwise the browser never returns the session cookie and login silently fails (fail-closed default: omitting it ⇒ `Secure=true` for prod/TLS).
- **Ports:** `7676:7676`. **Dependencies:** `zitadel-init` completed. **Volumes:** `genenv` → `/out` (ro), `./secrets` → `/secrets` (ro). **Restart:** `unless-stopped`.

### 4.6 admin-web (Console UI)
- **Image:** `deploy/compose/admin-web.Dockerfile` (Next.js, `NODE_ENV=production`). `next.config` rewrites `/api`,`/login`,`/callback`,`/logout` to `ADMIN_API_ORIGIN=http://admin-api:7676` (same-origin proxy → no CORS, `SameSite=Lax` cookie). Talks only to admin-api.
- **Ports:** `3000:3000`. **Dependencies:** `admin-api` started. **Restart:** `unless-stopped`. **Start here** in a browser.

### 4.7 worker (host, not in compose)
- The existing Tauri build run natively on Windows via `deploy/compose/run-worker.ps1`; launches the real `claude` against `~/.claude` and uses the host webview for Q&A.
- **Launch env:** `LLM_CHAT_AUTH_TOKEN=<same as manager>`, `LLM_CHAT_WS_PORT=7878`, `LLM_CHAT_WS_BIND=0.0.0.0` (loopback-only would be refused from the container).
- **Listener:** `ws://0.0.0.0:7878/{control,s/<sid>,qa/<sid>,/}`, Bearer auth. Manager requests carry no browser `Origin`, so the worker's origin rejection doesn't block them.
- **Must start before the manager's startup TCP probe** runs (the probe is fatal — §5/§7.1).

## 5. Code Changes

Mode-toggle vars are additive and default to today's behavior when unset (`MANAGER_BACKEND_PORTS` unset = spawn; `LLM_CHAT_AUTH_TOKEN` unset = random). The three address vars are **required, no code default** — resolved through pure `Result`-returning helpers; a missing/empty value fails fast naming the var.

- **worker:** `worker_bind_addr(bind: Option<String>, port) -> Result<String, String>` (`worker/src/lib.rs`) — `Err` (naming `LLM_CHAT_WS_BIND`) when `bind` is `None`/empty; else `Ok("<host>:<port>")`. In-place signature change of the existing helper (a duplicate is a compile error). `start_ws_server` matches on it and `process::exit(1)` on `Err`. Already-correct (no change): `load_or_generate_auth_token()` honors `LLM_CHAT_AUTH_TOKEN`; the WS server accepts inbound Bearer + rejects only http(s) `Origin`; port is `LLM_CHAT_WS_PORT` (default 7878).
- **manager** (`manager/src/main.rs`): one reusable `require_addr(var_name, raw: Option<String>) -> Result<String,String>` (trims; `Err` on `None`/empty/whitespace naming the var). In `main()`, resolve `MANAGER_BIND` and `MANAGER_BACKEND_HOST` via `require_addr` and `?`-propagate (non-zero exit).
  - **`MANAGER_BACKEND_HOST`** replaces the hardcoded `127.0.0.1` at the five request-time dial sites (`call_backend` `/control`, `handle_chat` `/s` + `/qa`, `bridge_to_backend`, `handle_root` `/`) and in the `wait_for_tcp` readiness probe. Request-time sites use a thin `backend_host()` wrapper (`require_addr(...).expect("validated at startup")`) to avoid a `ManagerState` field; `wait_for_tcp` gets a `host: &str` parameter (don't read env inside the loop).
  - **`MANAGER_BIND`** replaces `127.0.0.1` in `TcpListener::bind`.
  - **`MANAGER_BACKEND_PORTS`** set ⇒ skip `spawn_instance`, parse the comma port list. The spawn path (production, no external backend) must forward the worker's required bind: thread `backend_host` into `spawn_instance` and set `cmd.env("LLM_CHAT_WS_BIND", &backend_host)` on each child.
  - **`LLM_CHAT_AUTH_TOKEN`**: `let auth_token = env::var("LLM_CHAT_AUTH_TOKEN").ok().filter(|t| !t.is_empty()).unwrap_or_else(random_token);` before the existing `fs::write(&token_path, &auth_token)`. `call_backend()` reads the token from the **on-disk file** every call, so writing it at startup is sufficient — no threading through `ManagerState`.

**Fatal startup probe (load-bearing).** In external-backend mode `wait_for_tcp(&backend_host, p, 90).await?` `?`-propagates a `TimedOut` out of `main()` (≈45 s ceiling) — the manager **exits** if `7878` isn't reachable. Resolved by the **ordering contract** (start the host worker before `docker compose up`, §7.1) + `restart: unless-stopped`. Keep the `?`-propagation — it is **not** non-fatal.

**Backward compatibility.** With both mode toggles unset, the manager spawns local workers and generates a random token — today's behavior. To reproduce today's loopback, set `MANAGER_BIND=MANAGER_BACKEND_HOST=127.0.0.1` (spawned workers then get `LLM_CHAT_WS_BIND=127.0.0.1`). Omitting either fails fast.

## 6. Supporting Files

- **docker-compose.yml** — the services, the `pgdata`/`machinekey`/`genenv` named volumes, the `./secrets` bind, healthchecks (`postgres`, `zitadel`), `depends_on` conditions, restart policies.
- **.gitignore** — must ignore `secrets/` (the repo's base `.gitignore` does not). `secrets/kabytech-key.json` + `admin-api-key.json` are live RSA private keys. `.env` is covered by `*.env`; `.env.example` is not (stays committable).
- **.dockerignore** — keeps the build context small and prevents leaking `secrets/` into images (separate from `.gitignore`).
- **manager.Dockerfile** / **entrypoint.sh** (§4.4), **admin-api.Dockerfile** / **admin-api-entrypoint.sh** (resolves `*_FILE` → value, sources `/out`), **admin-web.Dockerfile**.
- **provisioner/** — `Dockerfile`, `provision.py`, `new_app.py`, `org_rename.py` (§4.3).
- **run-worker.ps1** — sets the shared token + `LLM_CHAT_WS_PORT=7878` + `LLM_CHAT_WS_BIND=0.0.0.0`, launches the native worker, warns about the Windows Firewall prompt.
- **`worker/package.json`** — because `LLM_CHAT_WS_BIND` is now required, `npm run tauri dev` would fail fast. Add `cross-env` and scripts that supply the vars: `"dev": "cross-env LLM_CHAT_WS_BIND=127.0.0.1 LLM_CHAT_WS_PORT=7878 tauri dev"` (and `build`). Standalone dev becomes `npm run dev` / `npm run build`; `README.md` and `docs/architecture.md` references updated to match.
- **`deploy/manager/manager.env.example`** (production / non-Docker) — add `MANAGER_BIND=127.0.0.1` and `MANAGER_BACKEND_HOST=127.0.0.1`. Production deliberately omits `MANAGER_BACKEND_PORTS` so the manager spawns workers (each gets `LLM_CHAT_WS_BIND=backend_host` via the forwarding above).

## 7. Runtime Data Flow

### 7.1 Boot sequence
**Ordering contract:** the **host worker starts before `docker compose up`** — the manager's external-backend probe is fatal, so `7878` must already be listening; `restart: unless-stopped` covers any transient window.

1. **host worker** — `run-worker.ps1` (approve the Firewall prompt for `0.0.0.0:7878`).
2. **postgres** — healthy when `pg_isready` passes.
3. **zitadel** — waits for postgres. First run against a fresh DB: init (wiring B creates `zitadel` DB + `zitadel_user`), setup (`03_default_instance` creates the `IAM_OWNER` bootstrap SA and writes `/machinekey/zitadel-admin-sa.json` once), then serves `:8080`. Healthy when `zitadel ready` passes.
4. **zitadel-init** — waits for zitadel healthy; mints the Management token, runs the idempotent sequence (§4.3) with retries, writes `./secrets/*` + `/out/manager.generated.env`, exits 0.
5. **manager** — waits for zitadel-init completed; entrypoint validates the generated env; binary writes the token to disk, binds `0.0.0.0:7777`, preloads JWKS, probes the already-running host worker (succeeds), registers `7878` without spawning.
6. **admin-api** — waits for zitadel-init completed; binds `0.0.0.0:7676`. **admin-web** — waits for admin-api; binds `:3000`.

### 7.2 One `/chat` round-trip
1. **Token (host).** Client signs a JWT-bearer assertion with `secrets/kabytech-key.json`, `POST`s to `http://host.docker.internal:8080/oauth/v2/token`; gets an access token with `iss=http://host.docker.internal:8080` and `chat.user` under `urn:zitadel:iam:org:project:<project_id>:roles`.

   **Two distinct, load-bearing scopes — do not swap:**

   | Caller | Scope (verbatim) | Audience |
   |---|---|---|
   | **Provisioner** (admin token, Management API) | `openid profile urn:zitadel:iam:org:project:id:zitadel:aud` | literal `zitadel` = Zitadel's internal project |
   | **Client** (token the manager validates) | `openid profile urn:zitadel:iam:org:project:id:<project>:aud urn:zitadel:iam:org:projects:roles` | numeric `<project>` = the `llm-chat` project id; plural `projects:roles` requests role claims |

   The client scope is already fixed in `clients/python/llm_chat_client.py` and is **not** a deliverable here — only the provisioner scope is implemented.

2. **Connect.** Client opens `ws://localhost:7777/chat` with `Authorization: Bearer <JWT>`, sends `{"type":"q","id":...,"text":"hello"}`.
3. **Verify (manager).** Validates signature vs cached JWKS, `iss == ZITADEL_ISSUER`, `aud` contains `ZITADEL_AUDIENCE` (= project id), and `chat.user` present. `iss` matches because both sides use the same literal issuer.
4. **Bridge.** Manager dials `ws://host.docker.internal:7878/control` (+ `/s/<sid>`, `/qa/<sid>`) via `MANAGER_BACKEND_HOST`, authenticating with the shared `LLM_CHAT_AUTH_TOKEN` read from its token file.
5. **Answer + confirm.** Worker drives `claude`, the webview parses Q&A, the answer flows back over `/qa/<sid>` to the manager, which emits an `a` frame **including a `seq` field** — the client (`llm_chat_client.py`) reads `msg["seq"]` and replies `{"type":"confirm","seq":<seq>}` (a missing `seq` raises `KeyError`). Success = the client prints the answer **and exits 0**.

## 8. Single-issuer resolution

Both host (token fetch) and containers (JWKS fetch + `iss` validation) use the identical literal `http://host.docker.internal:8080`:
- **Containers:** `host.docker.internal` auto-resolves inside Linux containers under Docker Desktop for Windows — **no `extra_hosts`** added. (Bare-Linux Docker Engine would need `extra_hosts: ["host.docker.internal:host-gateway"]` — different, out-of-scope topology.)
- **Host:** Docker Desktop maintains the Win32 hosts entry, so the name resolves on Windows.
- **Same endpoint:** `8080:8080` published all-interfaces; both resolutions terminate at the same Zitadel, which advertises `http://host.docker.internal:8080`. Publishing `127.0.0.1:8080:8080` instead would connection-refuse the container side.

**Fallback** (if host-side resolution fails, e.g. WSL 2 engine with the Win32-hosts setting off): as Administrator append `127.0.0.1 host.docker.internal` to `C:\Windows\System32\drivers\etc\hosts` — correct on the host because `8080` is host-published. **Do not** add it if Docker already manages the entry (duplicates cause flaky resolution); verify first with `Resolve-DnsName host.docker.internal` and a `curl …/.well-known/openid-configuration` from both host and a throwaway container.

## 9. Run Instructions

```powershell
# From repo root (D:\projects\llm-chat) on Docker Desktop for Windows.

# 1. Pre-flight: confirm the host ports are free (we have hit a dual-listener 7777
#    collision before — native + Docker both binding). Any row => free that port.
Get-NetTCPConnection -LocalPort 3000,7676,7777,7878,8080 -State Listen -ErrorAction SilentlyContinue

# 2. Env file.
cp .env.example .env
#   ZITADEL_MASTERKEY   -> openssl rand -hex 16   (exactly 32 hex chars; one-shot)
#   POSTGRES_PASSWORD   -> any strong password
#   LLM_CHAT_AUTH_TOKEN -> openssl rand -hex 32   (shared by manager + host worker)
#   ADMIN_SESSION_KEY   -> openssl rand -hex 32   (admin-api session cookie)

# 3. Start the REAL worker FIRST (uses your ~/.claude), BEFORE compose, because the
#    manager's startup probe of :7878 is fatal. Approve the Firewall prompt if shown.
.\deploy\compose\run-worker.ps1

# 4. Bring up the server side.
docker compose up -d
#    Wait until `docker compose ps` shows zitadel healthy, zitadel-init Exited(0),
#    and .\secrets\kabytech-key.json + .\secrets\project_id exist.

# 5. Full round-trip via the Python reference client.
python clients/python/llm_chat_client.py `
  --issuer  http://host.docker.internal:8080 `
  --project (Get-Content -Raw .\secrets\project_id).Trim() `
  --key-file .\secrets\kabytech-key.json `
  --manager ws://localhost:7777/chat `
  --send "hello"      # expect an 'a' frame and exit 0

# Operator Console: open http://localhost:3000 and log in (demo `admin` user).

# CLEAN RESET: wipe Zitadel state AND host secrets together, or the stale key
#   won't match the fresh instance:
#     docker compose down -v ; Remove-Item -Recurse -Force .\secrets
```

**Client env-var names differ from the manager's** (from `clients/python/llm_chat_client.py`): `--issuer`=`ZITADEL_ISSUER`, `--project`=`PROJECT_ID` (**not** `ZITADEL_PROJECT_ID`), `--key-file`=`KABYTECH_KEY`, `--manager`=`MANAGER_WS`. The flag-based flow above avoids this footgun.

## 10. Testing & Verification

1. **Compose lint:** `docker compose config --quiet` exits 0; shows the six services, three named volumes, the `./secrets` bind, restart policies, `depends_on` conditions.
2. **Healthcheck-gated boot:** with the worker running, `docker compose up -d`, poll `docker compose ps` until postgres + zitadel `healthy` and zitadel-init `Exited (0)`; manager/admin-api don't start before zitadel-init completes.
3. **Manager in JWT mode, not shared-token fallback:** `docker compose logs manager` shows "Zitadel auth enabled" with resolved issuer/audience/project_id, **not** the fallback warning (catches a partial `manager.generated.env`).
4. **Provisioner artifacts:** `./secrets/kabytech-key.json` is valid JSON with `"type":"serviceaccount"` + `keyId`/`key`(PEM)/`userId`; `project_id`, `kabytech_user_id`, `admin-api-key.json`, `admin_oidc_client_id/secret` non-empty; `manager.generated.env` defines equal non-empty `ZITADEL_PROJECT_ID`/`ZITADEL_AUDIENCE`.
5. **Round-trip:** §9 step 5 returns an `a` frame carrying `seq`, client sends `confirm{seq}`, **process exits 0** (exit-0, not just "a frame appeared", is the real pass — it exercises the full handshake).
6. **Console login:** `http://localhost:3000`, operator OIDC login as `admin` succeeds; users/roles/sessions pages load.
7. **Idempotency:** `docker compose run --rm zitadel-init` against the same instance is a no-op exit 0 (creates → 409, key gen skipped because on-disk `kabytech_user_id` matches); `kabytech-key.json` not rewritten. After `down -v` **without** deleting `./secrets`, the next run regenerates (on-disk `userId` no longer exists) and the round-trip still works.
8. **Loopback parity + fail-fast:** (a) with the three address vars set to `127.0.0.1` and `MANAGER_BACKEND_PORTS` unset, the manager binds `127.0.0.1`, spawns local workers, loopback `/chat` works as today. (b) with any one of `MANAGER_BIND`/`MANAGER_BACKEND_HOST`/`LLM_CHAT_WS_BIND` unset, the binary fails fast (non-zero exit, message naming the var).

## 11. Risks & Gotchas

- **Management-API version drift.** The v1 `AddProject/...` endpoints are deprecated. Pin the tag (`v3.4.10`), never `:latest`; re-verify the provisioner's call surface on any bump.
- **Port collisions.** `3000`, `7676`, `7777`, `7878`, `8080` must be free. We have hit a dual-listener `7777` collision (native + Docker). Pre-flight with `Get-NetTCPConnection` (§9).
- **Clock skew → "token validation failed".** JWT-bearer assertions carry `iat/exp`; host vs container clock drift fails validation opaquely. Docker Desktop normally keeps time synced — flag skew as a candidate when config looks correct.
- **`host.docker.internal` resolution.** Container-side is automatic; host-side depends on Docker maintaining the Win32 entry (WSL 2 setting). Use the §8 fallback; never duplicate the entry. Publish `8080:8080` all-interfaces.
- **HTTP-only issuer + non-`Secure` cookies (security).** `ExternalSecure=false` / `ADMIN_COOKIE_SECURE=false` mean cleartext credentials — **local-dev only**, never expose beyond localhost.
- **Secrets must not be committed.** `kabytech-key.json` and `admin-api-key.json` are live RSA private keys; the `.gitignore` must ignore `secrets/`.
- **Windows Firewall prompt** on the worker's `0.0.0.0:7878` bind — approve it (private networks); if declined, the manager can't reach the worker and `/chat` fails at the bridge.
- **Manager startup probe is fatal** — `wait_for_tcp(7878, 90)` exits `main()` on timeout (~45 s). Start the worker before `docker compose up` (§7.1) + `restart: unless-stopped`.
- **Zitadel first-init is one-shot.** The bootstrap key is written only on first init against a fresh DB; forcing re-creation throws `duplicate key`. To start clean, `docker compose down -v` **and** delete `./secrets` together.
- **Masterkey irreversible** — exactly 32 chars, can't change after first init without losing encrypted data. Generate once (`openssl rand -hex 16`), never edit.
- **Zitadel healthcheck quirk.** The image is distroless (no shell/`wget`/`curl`), so the probe is `zitadel ready`; per issue #9495 it probes HTTPS unless `ZITADEL_TLS_ENABLED=false` is set (separate from `EXTERNALSECURE`).
- **admin-api OIDC redirect origin.** `ADMIN_PUBLIC_ORIGIN` must be the proxy origin `:3000`, not the BFF's `:7676`, or the pre-auth cookie is dropped → "state mismatch" 403 at `/callback` (§4.5).
