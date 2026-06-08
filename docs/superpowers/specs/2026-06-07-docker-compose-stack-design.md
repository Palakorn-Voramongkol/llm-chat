# Server-Side Docker Compose Stack for llm-chat (Full Zitadel + Worker-on-Host)

## 1. Summary

This spec delivers a Docker Compose stack that boots the **server side** of `llm-chat` so a client can play with it on a local developer machine running Docker Desktop for Windows. The stack runs four containers — **postgres**, **zitadel** (a full, self-hosted Zitadel identity provider), **zitadel-init** (a one-shot provisioner), and **manager** (the Rust message router) — while the **worker runs natively on the Windows host** so it executes the real `claude` binary against the user's own `~/.claude` credentials and webview. The client is the existing Python reference client (`clients/python/llm_chat_client.py`), also run on the host. The entire design hinges on a single issuer string, `http://host.docker.internal:8080`, that resolves identically from both the host (where the Python client fetches a token) and the manager container (where it validates the `iss` claim and fetches JWKS). This is a **local-development playground**: the issuer is plain HTTP and several defaults are deliberately insecure, so it must never be exposed to a real network. The code changes required are minimal and env-driven. The three **network-address** variables (`LLM_CHAT_WS_BIND`, `MANAGER_BIND`, `MANAGER_BACKEND_HOST`) are **required with no hardcoded default in the code** — every address comes from the environment, and a missing/empty address var makes the binary **fail fast** at startup with a clear error naming the var (non-zero exit, no silent default). The two **mode-toggle** variables remain presence-based and backward-compatible: `MANAGER_BACKEND_PORTS` unset and `LLM_CHAT_AUTH_TOKEN` unset behave exactly as today.

## 2. Goals / Non-Goals

### Goals
- Provide a single `docker compose up` that brings up Postgres, Zitadel, an automatic provisioner, and the manager, healthcheck-gated and in the correct order.
- Automatically provision a Zitadel project (`llm-chat`), a role (`chat.user`), a machine user (`kabytech`), a role grant, and a downloadable JSON machine key — with no manual clicks in the Zitadel console.
- Let the host-native worker and the containerized manager share one auth token and one OIDC issuer, so a real `claude` session is reachable end-to-end from the Python client.
- Make the three network-address variables (`LLM_CHAT_WS_BIND`, `MANAGER_BIND`, `MANAGER_BACKEND_HOST`) **required and env-driven with no hardcoded fallback in code**, so addresses are never silently defaulted; a missing one fails fast at startup. Keep the mode-toggle changes (`MANAGER_BACKEND_PORTS`, `LLM_CHAT_AUTH_TOKEN`) presence-based and backward-compatible so the existing standalone/loopback workflow is unchanged when those are unset.
- Provide the config wiring (compose env, `run-worker.ps1`, `deploy/manager/manager.env.example`, and `worker/package.json` `cross-env` scripts) so both dev and production supply the now-required address vars and nothing silently breaks.
- Deliver copy-pasteable run instructions and a verifiable round-trip (`--send "hello"` returns an `a` frame, client exits 0).

### Non-Goals
- **Not production.** The issuer is `http://` (no TLS), `ExternalSecure=false`, cookies are non-`Secure`, and the `.env.example` ships insecure placeholder defaults. This stack is explicitly local-dev-only.
- Not running the worker in a container. The worker's Q&A parsing runs in a webview (it needs a real display) and must use the user's `~/.claude` state; both are satisfied only on the host. Containerizing the worker (xvfb, credential mounting) is out of scope.
- Not multi-host, not HA, not a reverse-proxy / TLS-terminating deployment. We run only the Zitadel API container needed for the machine-to-machine (JWT-bearer) flows used here; the approved design specifies the API/IdP container, not a console-/login-UI deployment. Whether the chosen image tag bundles or omits a separate login UI is a property of the tag selected in §4.2, not a design decision made here.
- Not changing the wire protocol, the `/chat` typed Q→A semantics, or the JWKS verification logic in the manager.

## 3. Architecture & Topology

The manager validates client JWTs against Zitadel's JWKS and requires the project role `chat.user` (encoded under `urn:zitadel:iam:org:project:<project_id>:roles`). The Python client mints those JWTs via the JWT-bearer flow using `kabytech`'s JSON key. For the `iss` claim to match on **both** sides of the trust boundary, the host client and the manager container must name the issuer with the **exact same literal string**.

**The linchpin:** `http://host.docker.internal:8080`.
- On Docker Desktop for Windows, `host.docker.internal` resolves from **inside Linux containers** automatically (Docker Desktop provides it; no `extra_hosts` needed).
- It **also** resolves from the **native Windows host**, because Docker Desktop maintains a `host.docker.internal` entry in the Win32 hosts file (`C:\Windows\System32\drivers\etc\hosts`).
- Both resolutions terminate at the same host-published port `8080:8080` (published on all interfaces, not loopback-only), so both vantage points hit the same Zitadel and see the same advertised issuer. Zitadel is told `ExternalDomain=host.docker.internal`, `ExternalPort=8080`, `ExternalSecure=false` so its discovery document advertises exactly `http://host.docker.internal:8080`.

```
WINDOWS HOST  (Docker Desktop for Windows)
┌──────────────────────────────────────────────────────────────────────────────┐
│  Python client                             run-worker.ps1 → worker.exe       │
│  clients/python/llm_chat_client.py         (real claude + ~/.claude)         │
│                                            WS listen 0.0.0.0:7878            │
│                                                                              │
│  (1) get token:  POST http://host.docker.internal:8080/oauth/v2/token        │
│  (2) chat:       ws://localhost:7777/chat  (Authorization: Bearer <JWT>)     │
│                                                                              │
│  ┌─────────────────────────────────────────────────────────────────────┐     │
│  │ host.docker.internal resolves from BOTH the host and containers     │     │
│  │ published :8080 → zitadel        published :7777 → manager          │     │
│  └─────────────────────────────────────────────────────────────────────┘     │
│                                                                              │
│  ╔════════════════════════ docker compose network ═════════════════════════╗ │
│  ║  postgres:5432  ◄──  zitadel:8080  ◄──  zitadel-init (one-shot)         ║ │
│  ║                                         │ writes                        ║ │
│  ║                                         ▼ ./secrets/kabytech-key.json   ║ │
│  ║                                           ./secrets/project_id          ║ │
│  ║                                           /out/manager.generated.env    ║ │
│  ║                                                                         ║ │
│  ║  manager:7777   (MANAGER_BIND=0.0.0.0)                                  ║ │
│  ║  (3) verify JWT vs JWKS:                                                ║ │
│  ║      http://host.docker.internal:8080/oauth/v2/keys                     ║ │
│  ║  (4) dial host worker (no spawn):                                       ║ │
│  ║      MANAGER_BACKEND_HOST=host.docker.internal  PORTS=7878              ║ │
│  ╚═════════════════════════════════════════════════════════════════════════╝ │
│                                                                              │
│  (5) manager → host worker:  ws://host.docker.internal:7878/{control,s,qa}   │
└──────────────────────────────────────────────────────────────────────────────┘
```

`MANAGER_BACKEND_PORTS` is a comma-separated list by contract, but in this topology it holds exactly one port (`7878`); there is no fan-out — the single host worker serves all sessions.

## 4. Components

### 4.1 postgres

- **Image:** `postgres:17-alpine`.
- **Purpose:** Backing store for Zitadel only. No `llm-chat` data lives here.
- **Key config/env:**
  - `POSTGRES_USER=postgres`
  - `POSTGRES_PASSWORD=${POSTGRES_PASSWORD}` (from `.env`)
  - `POSTGRES_DB=postgres` (the admin/bootstrap database that Zitadel's init connects to in order to create its own DB).
- **Dependencies:** none.
- **Healthcheck:** `pg_isready -U postgres -d postgres` — `interval: 5s`, `timeout: 5s`, `retries: 20`, `start_period: 10s`.
- **Volumes:** named volume `pgdata` → `/var/lib/postgresql/data`.

**How Zitadel connects to this DB — implementation choice to confirm (beyond the approved minimal "Zitadel's DB").** The approved design specifies postgres only as "Zitadel's DB, healthcheck `pg_isready`, volume `pgdata`." It does **not** prescribe *how* Zitadel authenticates to Postgres. Two valid wirings exist (see the verified findings under "Running Zitadel … with external Postgres"):

- **(A) Single-superuser DSN/discrete connection** — Zitadel connects as `postgres` directly into a pre-existing `zitadel` database. Simplest; fewest env vars.
- **(B) Discrete-field admin+user split** — Zitadel's init uses the `postgres` admin connection to auto-create a dedicated `zitadel` database and an unprivileged `zitadel_user`, then runs as that user. More faithful to least-privilege.

This spec adopts **(B)** because it is the documented "known-good local HTTP" wiring and keeps Zitadel off the superuser at runtime, but **flags it explicitly as an implementation choice not in the approved scope**. If (B) causes friction at build time, falling back to (A) is acceptable and changes nothing client-facing. The §4.2 env block below reflects (B); see also Open Questions §12.

### 4.2 zitadel

- **Image:** **pinned** — the approved design mandates a pinned tag (RISKS: "pin Zitadel image tag (Management-API drift)") but deliberately leaves the version open. This spec uses `ghcr.io/zitadel/zitadel:v3.4.10` as a **concrete, to-be-confirmed pin**, not a settled design decision. **The chosen tag MUST be verified to expose the v1 Management-API endpoints the provisioner calls** (`AddProject`, `AddProjectRole`, `AddMachineUser`, `AddMachineKey`, `AddUserGrant`, and the corresponding `_search` lookups). **Never use `:latest`.** Pinning is mandatory because those v1 endpoints are deprecated and could drift between Zitadel major versions. (The v3-vs-v4 tradeoff and whether a given tag ships a separate login-UI image are properties of the tag, to confirm at bring-up — not asserted here as design fact.)
- **Purpose:** The OIDC identity provider. Issues JWTs to the Python client and serves JWKS to the manager.
- **Command:** `start-from-init` (per the approved design: runs init → setup → serve). The **mechanism** for disabling TLS — e.g. the `--masterkeyFromEnv` and `--tlsMode disabled` flags and/or the `ZITADEL_TLS_ENABLED=false` env var — is a **derived requirement of the approved `ExternalSecure=false`** (you cannot serve a plain-HTTP issuer while the binary insists on TLS), not a separate approved decision. Use the minimal combination that the pinned tag honors; the verified-findings "known-good local HTTP config" uses `start-from-init --masterkeyFromEnv --tlsMode disabled`. Confirm the exact flag/env form against the pinned tag at build time (Open Questions §12); do not treat the specific TLS-toggle spelling as load-bearing.
- **Key config/env:**
  - `ZITADEL_MASTERKEY=${ZITADEL_MASTERKEY}` — **exactly 32 characters**; one-shot, cannot be rotated after first init without data loss. (If using `--masterkeyFromEnv`, read from this var; otherwise pass `--masterkey "${ZITADEL_MASTERKEY}"`.)
  - `ZITADEL_EXTERNALDOMAIN=host.docker.internal`  *(approved)*
  - `ZITADEL_EXTERNALPORT=8080`  *(approved)*
  - `ZITADEL_EXTERNALSECURE=false`  *(approved)*
  - Postgres connection env per the §4.1 wiring choice. For wiring (B):
    - `ZITADEL_DATABASE_POSTGRES_HOST=postgres`
    - `ZITADEL_DATABASE_POSTGRES_PORT=5432`
    - `ZITADEL_DATABASE_POSTGRES_DATABASE=zitadel`
    - `ZITADEL_DATABASE_POSTGRES_USER_USERNAME=zitadel_user`
    - `ZITADEL_DATABASE_POSTGRES_USER_PASSWORD=${POSTGRES_PASSWORD}`
    - `ZITADEL_DATABASE_POSTGRES_USER_SSL_MODE=disable`
    - `ZITADEL_DATABASE_POSTGRES_ADMIN_USERNAME=postgres`
    - `ZITADEL_DATABASE_POSTGRES_ADMIN_PASSWORD=${POSTGRES_PASSWORD}`
    - `ZITADEL_DATABASE_POSTGRES_ADMIN_SSL_MODE=disable`
    - *(The `_USER_*`/`_ADMIN_*` SSL-mode split and the unprivileged-user separation are part of wiring (B); see §4.1 and §12.)*
  - **Bootstrap admin machine user** *(approved: "First-instance creates a BOOTSTRAP ADMIN machine user whose JSON key is written to a shared `machinekey` volume")*. This creates an `IAM_OWNER` service account and writes its JSON key on **first init only**. Mind the **double `MACHINE`** in the user-object fields versus the single-token `MACHINEKEY`, and that `MACHINEKEYPATH` is top-level under `FirstInstance` (no `ORG_MACHINE` infix):
    - `ZITADEL_FIRSTINSTANCE_ORG_MACHINE_MACHINE_USERNAME=zitadel-admin-sa`
    - `ZITADEL_FIRSTINSTANCE_ORG_MACHINE_MACHINE_NAME=Admin`
    - `ZITADEL_FIRSTINSTANCE_ORG_MACHINE_MACHINEKEY_TYPE=1`  (`1` = JSON; the only supported value)
    - `ZITADEL_FIRSTINSTANCE_MACHINEKEYPATH=/machinekey/zitadel-admin-sa.json`
- **Ports:** `8080:8080` (published on all interfaces — **not** `127.0.0.1:8080:8080`, or the container side of `host.docker.internal` would get connection-refused).
- **Dependencies:** `postgres` → `condition: service_healthy`.
- **Healthcheck (approved endpoint `/debug/healthz`):** the probe **endpoint is fixed by the approved design** as `GET /debug/healthz`. The only open variable is the **tool** used to hit it from inside the container, which depends on what the pinned image ships. Spec decision: use an HTTP probe of `/debug/healthz`, e.g. `["CMD-SHELL", "wget -qO- http://localhost:8080/debug/healthz || exit 1"]`, with `interval: 5s`, `timeout: 5s`, `retries: 30`, `start_period: 30s`. **Tool-availability is a single verification item, not a design unknown** (tracked in §11 and §12): if the pinned image ships neither `wget` nor `curl`, fall back to the binary's built-in readiness subcommand `["CMD", "/app/zitadel", "ready"]`. Do **not** invent flag forms for that subcommand. Note (per findings, issue #9495): the `ready` subcommand has historically attempted HTTPS **even when TLS is disabled** — so `ExternalSecure=false`/`ZITADEL_TLS_ENABLED=false` do **not** guarantee it works on an HTTP deployment; if you fall back to `ready`, verify it empirically against the pinned tag.
- **Volumes:** named volume `machinekey` → `/machinekey` (shared with `zitadel-init` so the bootstrap key is readable by the provisioner).
- **Restart policy:** `restart: unless-stopped`.

### 4.3 zitadel-init (provisioner)

- **Image:** built from `deploy/compose/provisioner/Dockerfile`. Base `python:3-slim`. Python deps: **`pyjwt[crypto]`** (i.e. `pyjwt` plus its `cryptography` RS256 backend) and `requests`. The approved dependency list is "pyjwt/requests"; **`cryptography` is added as a necessary derivation**, because PyJWT cannot sign/verify RS256 without a crypto backend. This is an intentional, explained divergence from the approved list, not a silent addition.
- **Purpose:** One-shot, **idempotent** provisioner. Using the bootstrap admin key, it mints a Management-API token (JWT-bearer flow), creates the `llm-chat` project, the `chat.user` role, the `kabytech` machine user, grants the role, and generates `kabytech`'s JSON key. It then writes the artifacts the host client and the manager need.
- **Inputs:** reads `/machinekey/zitadel-admin-sa.json` (bootstrap admin key, JSON `{type, keyId, key, userId}`); knows the issuer `http://host.docker.internal:8080`.
- **Outputs:**
  - `/secrets/kabytech-key.json` → bind-mounted to `./secrets/kabytech-key.json` on the host (the client's `--key-file`).
  - `/secrets/project_id` → `./secrets/project_id` (the runtime project id, for the client's `--project`).
  - `/secrets/kabytech_user_id` → `./secrets/kabytech_user_id` (the `kabytech` userId, used by the idempotency guard in step 4 to detect a stale key after a `down -v`).
  - `/out/manager.generated.env` → named volume `genenv` shared with the manager, containing `ZITADEL_PROJECT_ID=<id>` and `ZITADEL_AUDIENCE=<id>` (audience equals project id).
- **Dependencies:** `zitadel` → `condition: service_healthy`.
- **Healthcheck:** none (one-shot job; the manager depends on its **completed** state, see §7).
- **Restart policy:** `restart: "no"` (one-shot; must not loop).
- **Volumes:** `machinekey` (ro), bind `./secrets` → `/secrets`, named `genenv` → `/out`.

**Robustness against a "just-healthy" Zitadel.** Even after `/debug/healthz` returns 200, the token and Management endpoints can briefly return 5xx / "instance not ready." `provision.py` MUST wrap **every** HTTP call (token mint and each Management call) in a retry loop:
- Retry on connection errors and HTTP `5xx` (and HTTP `401/403` for a short initial window, since a freshly-bootstrapped instance may not have the SA's grants live for a beat).
- Policy: up to **10** attempts, **3 s** fixed backoff between attempts (~30 s ceiling), per-request **timeout 15 s**. Do **not** retry on `409` (that is the success/idempotency signal) or on `400/404` (those are deterministic bugs to surface).

**Token acquisition (JWT-bearer with the bootstrap admin key).**
- `POST http://host.docker.internal:8080/oauth/v2/token`, `Content-Type: application/x-www-form-urlencoded`.
- Form: `grant_type=urn:ietf:params:oauth:grant-type:jwt-bearer`, `assertion=<signed JWT>`, **`scope=openid profile urn:zitadel:iam:org:project:id:zitadel:aud`**.
  - **Scope trap:** the literal word `zitadel` in the scope is intentional — it targets Zitadel's own internal project so the **Management API** accepts the token. Do **not** substitute the `llm-chat` project id here. (Contrast with the **client** scope in §7.2, which uses the numeric project id; see the scope table in §7.2.)
- Assertion JWT header: `{"alg":"RS256","kid":"<keyId from admin key>"}`. Payload: `{"iss":"<userId>","sub":"<userId>","aud":"http://host.docker.internal:8080","iat":<now>,"exp":<now+3600>}` (`iat` no older than 1 h; sign with the admin key's PEM).
- Use the returned `access_token` as `Authorization: Bearer <token>` on every `/management/v1` call below.

**Org context.** All create/search calls must land in the SA's own organization. The provisioner sets `x-zitadel-orgid` explicitly to be deterministic: fetch the SA's org once via `GET /auth/v1/users/me` and read `user.details.resourceOwner` (the org id), then send `x-zitadel-orgid: <orgId>` on every `/management/v1` request. *(Per the findings, omitting the header falls back to the SA's own org and would also work for a single-org provisioner; setting it explicitly removes ambiguity and is the chosen approach. The `GET /auth/v1/users/me` shape is **unverified** against the pinned tag — see §12 — so if it does not return the org id as expected, fall back to omitting `x-zitadel-orgid` and relying on the documented SA-org fallback.)*

**Management API call sequence** (base path `/management/v1`; treat HTTP **409 Conflict** = gRPC `ALREADY_EXISTS` as "already provisioned, continue"):

1. **Create project** — `POST /management/v1/projects`
   Body: `{"name":"llm-chat","projectRoleAssertion":false,"projectRoleCheck":false,"hasProjectCheck":false,"privateLabelingSetting":"PRIVATE_LABELING_SETTING_UNSPECIFIED"}`
   Response `200`: `{"id":"<projectId>", ...}` → persist `<projectId>`.
   **Idempotency:** the verified findings state the provisioner should **attempt `AddProject` and treat HTTP 409 as already-provisioned**. Follow that: POST the create; on `409`, recover the existing project id.
   *Recovery lookup — UNVERIFIED:* recovering the existing id on a 409 (e.g. via `POST /management/v1/projects/_search` with `{"queries":[{"nameQuery":{"name":"llm-chat","method":"TEXT_QUERY_METHOD_EQUALS"}}]}`) uses an endpoint/body that is **NOT in the verified findings** and must be **confirmed against the pinned Zitadel reference before implementation** (§12). The premise that project `name` is non-unique is likewise unverified. Note: in the normal clean-boot path Zitadel state and `./secrets` are wiped together (see §11 / §9), so the 409-on-project branch is only exercised when the same Zitadel instance is re-provisioned — making the search-recovery path a secondary concern, not the primary flow.

2. **Add role** — `POST /management/v1/projects/{projectId}/roles`
   Body: `{"roleKey":"chat.user","displayName":"Chat User","group":""}`
   Response `200`: `{"details":{...}}` (the `roleKey` is the identifier). Duplicate → `409`, treat as success.

3. **Create machine user** — `POST /management/v1/users/machine`
   Body: `{"userName":"kabytech","name":"kabytech","description":"llm-chat reference client","accessTokenType":"ACCESS_TOKEN_TYPE_BEARER"}`
   Response `200`: `{"userId":"<userId>", ...}` → persist `<userId>`. Uniqueness is on `userName`; a duplicate returns `409`.
   *409 recovery — UNVERIFIED:* recovering the existing `userId` on a `409` (e.g. via `POST /management/v1/users/_search` with body `{"queries":[{"userNameQuery":{"userName":"kabytech","method":"TEXT_QUERY_METHOD_EQUALS"}}]}`) uses an endpoint/query-object shape that is **NOT in the verified findings** and must be **confirmed against the pinned Zitadel reference before implementation** (§12). As with step 1, the clean-boot path wipes Zitadel and `./secrets` together, so this branch is a secondary path.

4. **Generate JSON key** (key material returned **once**, inline) — `POST /management/v1/users/{userId}/keys`
   Body: `{"type":"KEY_TYPE_JSON"}` (omit `publicKey` so Zitadel generates the keypair and returns the private key; `expirationDate` is **omitted** — local-dev keys do not expire by default, the simplest choice).
   Response `200`: `{"keyId":"<id>","keyDetails":"<base64>", ...}`.
   **Critical:** base64-**decode** `keyDetails` to obtain `{"type":"serviceaccount","keyId":...,"key":"<PEM>","userId":...}`. Write that decoded JSON to `/secrets/kabytech-key.json` immediately — it cannot be retrieved later. Also write `<userId>` to `/secrets/kabytech_user_id`.
   **Idempotency guard (robust to a wiped Zitadel):** a plain "skip if `kabytech-key.json` exists" guard is unsafe, because `./secrets` is a host bind mount that **survives `docker compose down -v`** while the Zitadel DB and `machinekey` volume are wiped — leaving a stale key whose `userId` no longer exists, causing a silent auth failure. The guard therefore is:
     - If `/secrets/kabytech-key.json` does **not** exist → generate, write key + `kabytech_user_id`.
     - If it **does** exist → read the on-disk `userId`; if that `userId` matches the `kabytech` user just created/looked-up in this run (step 3), **skip** regeneration (true re-run, key still valid). If it does **not** match (stale key from a wiped instance) → regenerate and overwrite both files.
   - This makes re-runs against the *same* instance a no-op while self-healing after a `down -v`. (§9/§11 additionally document deleting `./secrets` on a clean reset as the simpler operator path.)

5. **Grant the role** — `POST /management/v1/users/{userId}/grants`
   Body: `{"projectId":"<projectId>","roleKeys":["chat.user"]}`
   Response `200`: `{"userGrantId":"<id>", ...}`. Duplicate grant → `409`, treat as success.

6. **Write outputs:** `/secrets/project_id` ← `<projectId>`; `/secrets/kabytech_user_id` ← `<userId>`; `/out/manager.generated.env` ← `ZITADEL_PROJECT_ID=<projectId>\nZITADEL_AUDIENCE=<projectId>\n`. Exit 0.

### 4.4 manager

- **Image:** built from `deploy/compose/manager.Dockerfile` (multi-stage: `rust:1-bookworm` build of `./manager` → runtime `debian:bookworm-slim` with `ca-certificates`). Entrypoint `deploy/compose/entrypoint.sh` sources `/out/manager.generated.env`, **validates** the three Zitadel vars, then `exec`s the manager binary.
- **Purpose:** The message router. Verifies client JWTs against Zitadel JWKS, requires role `chat.user`, and bridges `/chat`, `/control`, `/s/<sid>`, `/qa/<sid>`, `/` to the **host** worker.
- **Key config/env:**
  - From `.env`: `ZITADEL_ISSUER=http://host.docker.internal:8080`, `LLM_CHAT_AUTH_TOKEN=${LLM_CHAT_AUTH_TOKEN}`.
  - From `/out/manager.generated.env` (sourced by entrypoint): `ZITADEL_PROJECT_ID`, `ZITADEL_AUDIENCE`.
  - New env vars introduced by this work (see §5): `MANAGER_BIND=0.0.0.0`, `MANAGER_BACKEND_HOST=host.docker.internal`, `MANAGER_BACKEND_PORTS=7878`.
- **Ports:** `7777:7777` (published; the Python client connects to `ws://localhost:7777/chat`).
- **Dependencies:** `zitadel-init` → `condition: service_completed_successfully` (guarantees the generated env and key exist before the manager starts).
- **Restart policy:** `restart: unless-stopped` — required so the manager survives the transient window before the host worker is up (see §5.1(3) and the boot-ordering contract in §7.1). Without this, the `wait_for_tcp` startup probe failing once would leave the manager exited with no recovery.
- **Healthcheck:** **none** (dropped). The approved design specifies healthchecks only on `postgres` and `zitadel`; nothing in the stack depends on the manager's health, and a `/dev/tcp` redirection probe is unreliable because Docker's `CMD-SHELL` runs `/bin/sh` (dash on Debian), where `/dev/tcp` is unavailable. A manager healthcheck is therefore out of scope; readiness is observed via `docker compose logs manager` and the §10 round-trip.
- **Volumes:** named `genenv` → `/out` (ro).
- **External-backend mode:** because `MANAGER_BACKEND_PORTS=7878` is set, the manager **does not spawn** a worker (it has no worker binary in the container) and instead treats port `7878` on `MANAGER_BACKEND_HOST` as an already-running backend. `MANAGER_BACKEND_PORTS` is a comma-separated list by contract; here it is the single port `7878` — there is no multi-port fan-out, the one host worker serves every session.

**`entrypoint.sh` must fail-fast on incomplete Zitadel config.** This is load-bearing: the manager's `ZitadelConfig::from_env()` (`manager/src/auth_zitadel.rs:74`) is **all-or-nothing** — it requires `ZITADEL_ISSUER`, `ZITADEL_AUDIENCE`, **and** `ZITADEL_PROJECT_ID`; if **any** is missing it returns `Err`, and the manager (`main.rs:770-776`) **silently falls back to shared-token auth** (warn-log only), after which the JWT client gets a confusing `401`. Because this spec splits those vars across two sources (`ZITADEL_ISSUER` from `.env`; the other two from `/out/manager.generated.env`), a half-written or empty generated file would degrade to shared-token mode invisibly. The entrypoint MUST therefore, after sourcing, assert all three are non-empty and `exit 1` otherwise so the container is visibly broken instead of silently mis-authenticating:

```sh
#!/bin/sh
set -e
set -a
. /out/manager.generated.env
set +a
: "${ZITADEL_ISSUER:?ZITADEL_ISSUER missing — refusing to start in shared-token mode}"
: "${ZITADEL_PROJECT_ID:?ZITADEL_PROJECT_ID missing from manager.generated.env}"
: "${ZITADEL_AUDIENCE:?ZITADEL_AUDIENCE missing from manager.generated.env}"
exec /usr/local/bin/llm-chat-manager
```

*(Optional, recommended code hardening — flagged in §12, not in the approved scope: make the manager itself fail-fast when `ZITADEL_ISSUER` is set but `ZITADEL_AUDIENCE`/`ZITADEL_PROJECT_ID` are missing, rather than silently degrading. Out of scope for this change unless approved.)*

### 4.5 worker (host, not in compose)

- **Binary:** the existing Tauri build (`src-tauri` / `worker`), run natively on Windows via `deploy/compose/run-worker.ps1`. It launches the real `claude` against the user's `~/.claude` and uses the host webview for Q&A parsing.
- **Launch env (set by `run-worker.ps1`):**
  - `LLM_CHAT_AUTH_TOKEN=<same value as the manager's>` — so manager↔worker token auth matches.
  - `LLM_CHAT_WS_PORT=7878`
  - `LLM_CHAT_WS_BIND=0.0.0.0` — so the worker's WS listener is reachable from the manager container via `host.docker.internal:7878` (loopback-only would be refused from the container).
- **Listener:** `ws://0.0.0.0:7878/{control,s/<sid>,qa/<sid>,/}`, Bearer-token auth. Manager requests carry no browser `Origin` header, so the worker's http(s)-origin rejection does not block them.
- **Health/order:** the worker **must be started before the manager's startup TCP probe runs**, because that probe is fatal (see §5.1(3) and §7.1). The ordering contract is in §7.1 / §9.

## 5. Code Changes

The two **mode-toggle** variables remain additive and env-gated and **default to today's exact behavior when unset** (`MANAGER_BACKEND_PORTS` unset = spawn local workers; `LLM_CHAT_AUTH_TOKEN` unset = random token). The three **network-address** variables (`MANAGER_BIND`, `MANAGER_BACKEND_HOST`, `LLM_CHAT_WS_BIND`) are **required with no hardcoded default in the code**: each is resolved through a pure, `Result`-returning helper, and a missing/empty value makes the binary **fail fast** at startup with a clear error naming the var (non-zero exit). Verified change sites (file:line) below match the current source.

**Pure, `Result`-returning resolution helpers (unit-testable without mutating env):**
- **worker:** `fn worker_bind_addr(bind: Option<String>, port: u16) -> Result<String, String>` — returns `Err` (message naming `LLM_CHAT_WS_BIND`) when `bind` is `None`/empty; otherwise `Ok("<host>:<port>")`. This is a **change to the existing helper** (today it returns `String` with a loopback default), not a new function. Unit tests: `None -> Err`, `Some("") -> Err`, `Some("0.0.0.0") -> Ok("0.0.0.0:7878")`, `Some("127.0.0.1") -> Ok("127.0.0.1:7878")`.
- **manager:** ONE reusable helper `fn require_addr(var_name: &str, raw: Option<String>) -> Result<String, String>` — trims; returns `Err(format!("{var_name} must be set (no default)"))` when `None`/empty/whitespace-only; otherwise `Ok(trimmed)`. The same helper backs both `MANAGER_BIND` and `MANAGER_BACKEND_HOST`. Unit tests cover both usages: `None -> Err` (message contains the var name), `Some("") -> Err`, `Some("  ") -> Err`, `Some("127.0.0.1") -> Ok`, `Some("host.docker.internal") -> Ok`.

### 5.1 `manager/src/main.rs`

`main()` returns `Result<(), Box<dyn Error>>`. **Near startup, resolve BOTH required address vars via `require_addr` and fail fast** if either is missing — naming the offending var (return the `Err` / `eprintln!` + propagate, non-zero exit):
- `let manager_bind = require_addr("MANAGER_BIND", std::env::var("MANAGER_BIND").ok())?;`
- `let backend_host = require_addr("MANAGER_BACKEND_HOST", std::env::var("MANAGER_BACKEND_HOST").ok())?;`

The resolved `backend_host: String` is threaded into `wait_for_tcp(&host, port, retries)` and into `spawn_instance` (see (3)). This keeps the change minimal: **no `ManagerState` field, no `call_backend` signature change.**

**(1) `MANAGER_BACKEND_HOST`** (required; resolved once via `require_addr` in `main()`) — the backend-dial host at **five** request-time sites plus the readiness probe:
- **Line 1294** `call_backend()`: `format!("ws://127.0.0.1:{}/control", port)` → use `MANAGER_BACKEND_HOST`.
- **Line 1485** `handle_chat()`: `format!("ws://127.0.0.1:{}/s/{}", port, sid)` → use `MANAGER_BACKEND_HOST`.
- **Line 1502** `handle_chat()`: `format!("ws://127.0.0.1:{}/qa/{}", port, sid)` → use `MANAGER_BACKEND_HOST`.
- **Line 1946** `bridge_to_backend()`: `format!("ws://127.0.0.1:{}{}{}", backend_port, base, sid)` → use `MANAGER_BACKEND_HOST`.
- **Line 2016** `handle_root()`: `format!("ws://127.0.0.1:{}/", p)` → use `MANAGER_BACKEND_HOST`.
- **The five request-time dial sites keep using a thin `fn backend_host() -> String` wrapper** that reads `MANAGER_BACKEND_HOST` from env and unwraps via `require_addr` — i.e. `require_addr("MANAGER_BACKEND_HOST", std::env::var("MANAGER_BACKEND_HOST").ok()).expect("validated at startup")`. This is **safe** because `main()` already validated presence at startup (document this with the `.expect("validated at startup")` message). It avoids a `ManagerState` field and avoids changing `call_backend`'s signature.
- **Line 870** `wait_for_tcp()`: `TcpStream::connect(("127.0.0.1", port))` → connect to `(host.as_str(), port)`. **Specified mechanism (single, no implementer choice):** add a `host: &str` parameter to `wait_for_tcp` and pass the startup-resolved `backend_host` value in from the caller (the same value `backend_host()` returns at request time). Do **not** read the env var inside the loop.

**(2) `MANAGER_BIND`** (required; resolved via `require_addr` in `main()` and bound to `manager_bind`) — the manager's own listen socket:
- **Line 787**: `TcpListener::bind(("127.0.0.1", manager_port))` → `bind((manager_bind.as_str(), manager_port))`. Because `bind` is generic over `ToSocketAddrs`, pass `(&str, u16)` or format an `addr` string. If `MANAGER_BIND` is missing/empty the `require_addr` call in `main()` has already failed fast before this line is reached.
- **Line 790** (logging only, cosmetic): update the log to print the resolved `MANAGER_BIND` value instead of the literal `127.0.0.1` for accuracy.

**(3) External-backend mode: `MANAGER_BACKEND_PORTS`** (unset = current spawn behavior — presence-based, backward-compatible):
- **Lines 716–721** (spawn loop): if `MANAGER_BACKEND_PORTS` is set, **skip** `spawn_instance` entirely and parse the comma-separated port list into `ports` instead. When unset, behavior is unchanged (spawn `n_instances` workers). **`spawn_instance` (the non-external/production path) must FORWARD the worker's required bind:** thread `backend_host` into `spawn_instance`'s signature and have it pass `cmd.env("LLM_CHAT_WS_BIND", &backend_host)` to each spawned child. Rationale: the manager dials backends at `backend_host`, so a spawned worker must bind that same host — and this supplies the spawned worker its now-required `LLM_CHAT_WS_BIND` with no hardcoded default. (In external-backend mode no workers are spawned, so the forwarding is exercised only on the spawn path.)
- **Lines 723–727** (`wait_for_tcp` loop): **this probe is fatal and must be reconciled with boot ordering.** The verified code is `wait_for_tcp(p, 90).await?;` — the `?` propagates a `TimedOut` error out of `main()`, so the manager **process exits** if the backend on `7878` is not reachable within `90 × 500 ms = 45 s`. With the new `host` parameter the call becomes `wait_for_tcp(&backend_host, p, 90).await?`. This spec resolves the fatal probe by the **ordering contract** (preferred, no behavior change to the probe): the host worker is started **before** `docker compose up` (see §7.1 / §9), so port `7878` is already listening when the manager probes, and the probe succeeds promptly. The manager additionally carries `restart: unless-stopped` (§4.4) so any transient pre-worker window self-heals. Keep the existing `?`-propagation; **do not** describe the probe as "non-fatal." Optionally log "external backend mode — waiting for pre-started worker(s) at MANAGER_BACKEND_HOST:<port>" before the wait for operator clarity.
  - *Alternative (not chosen, recorded for completeness):* make `wait_for_tcp` non-fatal in external mode by logging a warning and continuing instead of `?`-propagating at line 725. This spec does **not** adopt it, to keep the code change minimal and avoid changing the loopback-mode contract; the ordering+restart approach above is sufficient.

**(4) `LLM_CHAT_AUTH_TOKEN`** (unset = current random-token behavior — presence-based, backward-compatible):
- **Lines 699–702** (token generation + file write): change `let auth_token = random_token();` to first check the env var — `let auth_token = std::env::var("LLM_CHAT_AUTH_TOKEN").ok().filter(|t| !t.is_empty()).unwrap_or_else(random_token);` — then keep the existing `std::fs::write(&token_path, &auth_token)?;` and `lock_token_acl(&token_path);`.
- **Call-out (verified, load-bearing):** `call_backend()` reads the token from the **on-disk file** every call — **Line 1291**: `let token = std::fs::read_to_string(auth_token_path())?.trim().to_string();`. It does **not** read `ManagerState::auth_token`. Therefore, honoring the env var **before** the existing `fs::write` at line 701 is sufficient and correct: the env value lands on disk at startup, and `call_backend` reads it back on first use. No timing issue, no need to thread the token through `ManagerState`. The in-memory copy (line 781) is used only for spawning workers (irrelevant in external-backend mode) and intra-process auth checks.

Backward compatibility (mode toggles only): with `MANAGER_BACKEND_PORTS` and `LLM_CHAT_AUTH_TOKEN` unset, the manager spawns local workers and generates a random token — today's behavior exactly. The address vars `MANAGER_BIND` and `MANAGER_BACKEND_HOST` are **required** and have **no** default; to reproduce today's loopback behavior set both to `127.0.0.1` (and the spawned workers then receive `LLM_CHAT_WS_BIND=127.0.0.1` via the forwarding in (3)). Omitting either makes the manager fail fast at startup naming the missing var.

### 5.2 `worker/src/lib.rs`

**(1) `LLM_CHAT_WS_BIND`** (required; no hardcoded default):
- **Change the existing `worker_bind_addr` helper at line 1555** from `fn worker_bind_addr(bind: Option<String>, port: u16) -> String` (which today filters out empty and defaults to `127.0.0.1`) to `fn worker_bind_addr(bind: Option<String>, port: u16) -> Result<String, String>`: return `Err` naming `LLM_CHAT_WS_BIND` when `bind` is `None`/empty; otherwise `Ok("<host>:<port>")`. Do **not** add a second definition — a duplicate `worker_bind_addr` is a compile error; this is an in-place signature/body replacement. The helper stays **pure** (no env access), so it is unit-tested without mutating the environment. Update the existing helper unit tests (currently at lines 2374–2390, which assert the loopback default, e.g. `worker_bind_addr(None, 7878) == "127.0.0.1:7878"` and `Some("")` → loopback) to the new `Result` contract: `None -> Err`, `Some("") -> Err`, `Some("0.0.0.0") -> Ok("0.0.0.0:7878")`, `Some("127.0.0.1") -> Ok("127.0.0.1:7878")`.
- **Line 1660** (`start_ws_server`): the call site already reads `let addr = worker_bind_addr(std::env::var("LLM_CHAT_WS_BIND").ok(), port);` (an earlier version of this change is partially applied — there is **no** literal `let addr = format!("127.0.0.1:{}", port);` line to replace). Convert this existing call into the match/fail-fast form so a missing/empty bind exits non-zero — the WS server is core, there is no default:
  ```rust
  let addr = match worker_bind_addr(std::env::var("LLM_CHAT_WS_BIND").ok(), port) {
      Ok(a) => a,
      Err(msg) => {
          tracing::error!("{msg}");
          eprintln!("{msg}");
          std::process::exit(1);
      }
  };
  ```
  The subsequent `TcpListener::bind(&addr)` (line 1661) is unchanged.

**(2) Already correct — no change needed (documented for confidence):**
- `load_or_generate_auth_token()` (lines 1560–1580) **already** honors `LLM_CHAT_AUTH_TOKEN` from env first, so the host worker and manager share the token with no code change.
- The WS server already accepts inbound Bearer-token connections, validates with constant-time compare, and rejects only http(s) `Origin` headers — manager requests have no `Origin` and pass.
- The port is already `LLM_CHAT_WS_PORT` (default 7878).

Required-var behavior: `LLM_CHAT_WS_BIND` has **no** default; the worker fails fast at startup naming it when unset/empty. To run the worker standalone in dev, supply it explicitly — `worker/package.json` does this via `cross-env` (see §6, §9). The compose path supplies `0.0.0.0` via `run-worker.ps1`; the manager-spawn path supplies it via the `spawn_instance` forwarding (§5.1(3)).

## 6. New Files

```
llm-chat/
├── docker-compose.yml                         # the 4-service stack (postgres, zitadel, zitadel-init, manager)
├── .env.example                               # ZITADEL_MASTERKEY, POSTGRES_PASSWORD, LLM_CHAT_AUTH_TOKEN (copy to .env)
├── .dockerignore                              # excludes target/, node_modules/, .git/, secrets/ from build context
├── .gitignore                                 # EDIT (not new): append `secrets/` so the live private key is never committable
├── secrets/                                   # created at runtime; ignored only AFTER the .gitignore edit below
│                                              #   holds kabytech-key.json, project_id, kabytech_user_id
├── README.md                                  # EDIT (not new): line 52 `npm run tauri dev`→`npm run dev`,
│                                              #   line 58 `npm run tauri build`→`npm run build`
├── docs/
│   └── architecture.md                        # EDIT (not new): line 108 + line 281 `npm run tauri dev`→`npm run dev`
├── worker/
│   └── package.json                           # EDIT (not new): add cross-env devDependency + dev/build scripts
│                                              #   that set the now-required LLM_CHAT_WS_BIND/LLM_CHAT_WS_PORT
├── deploy/
│   ├── manager/
│   │   └── manager.env.example                # EDIT (not new): add MANAGER_BIND=127.0.0.1, MANAGER_BACKEND_HOST=127.0.0.1
│   └── compose/
│       ├── manager.Dockerfile                 # multi-stage Rust→debian-slim build of ./manager
│       ├── entrypoint.sh                      # sources + validates manager.generated.env, exec's the manager binary
│       ├── provisioner/
│       │   ├── Dockerfile                     # python:3-slim + pyjwt[crypto] (pyjwt+cryptography) + requests
│       │   └── provision.py                   # the idempotent Zitadel provisioner (§4.3 call sequence)
│       ├── run-worker.ps1                     # host helper: sets token+port+bind, launches the native worker
│       └── README.md                          # the run instructions and troubleshooting (§9, §11)
```

- **docker-compose.yml** — defines the four services, the `pgdata` / `machinekey` / `genenv` named volumes, the `./secrets` bind mount, healthchecks (`postgres`, `zitadel`), `depends_on` conditions (§7), and the restart policies in §4.
- **`.gitignore` edit (REQUIRED, §6 task).** The repo's current `.gitignore` does **NOT** ignore `secrets/` — verified contents are exactly: `node_modules/`, `worker/target/`, `worker/node_modules/`, `manager/target/`, `*.log`, `*.env`, `.claude/`. A live RSA private key would land in an un-ignored directory and be trivially committable. **Append this line to `.gitignore`:**
  ```
  secrets/
  ```
  (`.env` is already covered by the existing `*.env`; `.env.example` is **not** matched by `*.env`, so it remains committable as intended. Do **not** rely on any pre-existing "gitignored" assumption — this edit is what makes `secrets/` ignored.)
- **.dockerignore** — keeps the build context small and prevents leaking `secrets/` into images (this is a *separate* mechanism from `.gitignore` and does not substitute for it).
- **manager.Dockerfile** — stage 1 builds `./manager` in release mode; stage 2 copies the binary into `debian:bookworm-slim`, installs `ca-certificates`, adds `entrypoint.sh`.
- **entrypoint.sh** — sources `/out/manager.generated.env`, **asserts the three Zitadel vars are non-empty (exit 1 otherwise)**, then `exec`s the manager binary (see §4.4 for the exact script).
- **provisioner/Dockerfile** + **provision.py** — implement §4.3 exactly; the script is re-runnable (idempotent), with the retry loop, explicit `x-zitadel-orgid`, and the userId-aware key guard.
- **run-worker.ps1** — reads `LLM_CHAT_AUTH_TOKEN` from `.env` (or a param), sets `LLM_CHAT_WS_PORT=7878`, `LLM_CHAT_WS_BIND=0.0.0.0` (the now-required worker bind, supplied here so the host worker reaches fail-fast satisfied), and launches the worker binary; prints a reminder about the Windows Firewall prompt.
- **README.md** — the §9 steps plus the §8 fallback and §11 risks.
- **`worker/package.json` edit (REQUIRED).** Because `LLM_CHAT_WS_BIND` is now required with no code default, `npm run tauri dev` would fail fast at worker startup. Add `cross-env` as a `devDependency` and add scripts that supply the required vars for standalone dev/build:
  ```json
  "dev":   "cross-env LLM_CHAT_WS_BIND=127.0.0.1 LLM_CHAT_WS_PORT=7878 tauri dev",
  "build": "cross-env LLM_CHAT_WS_BIND=127.0.0.1 LLM_CHAT_WS_PORT=7878 tauri build"
  ```
  Install the dependency (`npm install --save-dev cross-env` in `worker/`). Standalone-dev runs become `npm run dev` (and `npm run build`).
- **`README.md` edit (REQUIRED).** Two standalone-worker run references break once `LLM_CHAT_WS_BIND` is required. Update **both**:
  - **README.md:52** `npm run tauri dev` → `npm run dev`
  - **README.md:58** `npm run tauri build` → `npm run build`
- **`docs/architecture.md` edit (REQUIRED).** Two more standalone-worker run references (`npm run tauri dev`) become stale — after `LLM_CHAT_WS_BIND` is required they fail fast. Update **both** to `npm run dev`:
  - **docs/architecture.md:108** — the prose `Standalone (`npm run tauri dev`)` reference.
  - **docs/architecture.md:281** — the runnable code block `npm run tauri dev` under `### Worker alone`.
- **`deploy/manager/manager.env.example` edit (REQUIRED, production / non-Docker).** The file already exists (with `MANAGER_PORT` etc.). Add the now-required address vars so production supplies them:
  ```
  MANAGER_BIND=127.0.0.1
  MANAGER_BACKEND_HOST=127.0.0.1
  ```
  Production deliberately does **not** set `MANAGER_BACKEND_PORTS`, so the manager **spawns** workers; each spawned worker receives its required `LLM_CHAT_WS_BIND` via the `spawn_instance` forwarding (`= backend_host`, §5.1(3)), so no separate worker env entry is needed in production.

## 7. Runtime Data Flow

### 7.1 Boot sequence and ordering contract

**Ordering contract (resolves the fatal `wait_for_tcp` probe):** the **host worker is started before `docker compose up`**. The manager's external-backend startup probe (`wait_for_tcp`, §5.1(3)) is fatal — if `7878` is not reachable within ~45 s the manager process exits — so `7878` must already be listening when the manager comes up. Combined with the manager's `restart: unless-stopped` policy, this guarantees the manager either probes a live worker on first try or self-heals on restart.

Compose `depends_on` order:
1. **host worker** — start `run-worker.ps1` first (before compose). Binds `0.0.0.0:7878` with the shared token. *(If the worker shows a Windows Firewall prompt, approve it now.)*
2. **postgres** — becomes healthy when `pg_isready` passes.
3. **zitadel** — waits for `postgres` healthy. On the **first** run against a fresh DB it: runs init (under wiring (B), creates the `zitadel` DB + `zitadel_user`), runs setup (migration `03_default_instance` creates the `IAM_OWNER` bootstrap SA and writes `/machinekey/zitadel-admin-sa.json` — **once only**), then serves on `:8080`. Becomes healthy when `/debug/healthz` returns 200.
4. **zitadel-init** — waits for `zitadel` healthy. Mints a Management-API token, runs the idempotent sequence (§4.3 steps 1–6) with retries, writes `./secrets/{kabytech-key.json,project_id,kabytech_user_id}` and `/out/manager.generated.env`, then **exits 0**.
5. **manager** — waits for `zitadel-init` **completed successfully**. Entrypoint sources + validates the generated env; the binary writes `LLM_CHAT_AUTH_TOKEN` to its on-disk token file, binds `0.0.0.0:7777`, preloads JWKS, probes the **already-running** host worker at `host.docker.internal:7878` (succeeds promptly), and registers port `7878` without spawning.

### 7.2 One `/chat` round-trip (host + containers)

1. **Token (host).** The Python client signs a JWT-bearer assertion with `secrets/kabytech-key.json` and `POST`s to `http://host.docker.internal:8080/oauth/v2/token`. Zitadel returns an access token whose `iss` is `http://host.docker.internal:8080` and whose roles include `chat.user` under `urn:zitadel:iam:org:project:<project_id>:roles`.

   **Two distinct, load-bearing scope strings — do not swap them.** The shipped client and the provisioner use *different* scopes on purpose:

   | Caller | Scope (verbatim) | Audience meaning |
   |---|---|---|
   | **Provisioner** (§4.3, admin token for the **Management API**) | `openid profile urn:zitadel:iam:org:project:id:zitadel:aud` | literal `zitadel` = Zitadel's **internal** project |
   | **Client** (this step, token the **manager** validates) | `openid profile urn:zitadel:iam:org:project:id:<project>:aud urn:zitadel:iam:org:projects:roles` | numeric `<project>` = the **`llm-chat`** project id (what the manager checks as `aud`); plural `projects:roles` requests the role claims |

   The **client scope is already fixed in `clients/python/llm_chat_client.py` (lines 69–73)** and is **NOT a deliverable** of this spec — only the provisioner scope is implemented (in `provision.py`). The client form uses singular `project:id:<project>:aud` + plural `projects:roles`; the provisioner form uses the singular literal `project:id:zitadel:aud` with no roles scope. Mixing them breaks auth.

2. **Connect (host→manager).** The client opens `ws://localhost:7777/chat` with `Authorization: Bearer <JWT>` and sends a typed `q` frame (`{"type":"q","id":...,"text":"hello"}`).
3. **Verify (manager).** The manager validates the token signature against its cached JWKS, checks `iss == ZITADEL_ISSUER`, `aud` contains `ZITADEL_AUDIENCE` (= project id), and that `chat.user` is present. The `iss` matches because the host minted it from the **same literal** `http://host.docker.internal:8080` the manager validates against.
4. **Bridge (manager→host worker).** The manager dials `ws://host.docker.internal:7878/control` (and `/s/<sid>`, `/qa/<sid>`) using `MANAGER_BACKEND_HOST`, authenticating with the shared `LLM_CHAT_AUTH_TOKEN` it read from its token file.
5. **Answer + confirm (worker→manager→client).** The worker drives the real `claude` session, the webview parses the Q&A, and the answer flows back over `/qa/<sid>` to the manager, which emits an `a` frame to the client. **The manager's `a` frame MUST include a `seq` field**: the shipped client (`llm_chat_client.py:99-101`) reads `msg["seq"]` and replies with `{"type":"confirm","seq":<seq>}`; an `a` frame lacking `seq` makes the client raise `KeyError`, not exit cleanly. A successful round-trip is the client printing the answer **and exiting 0** after the confirm handshake.

## 8. Single-Issuer / host.docker.internal Resolution

Both the host client (token fetch) and the container manager (JWKS fetch + `iss` validation) use the **identical literal** issuer string `http://host.docker.internal:8080`. This works because:

- **Container side (manager).** Under Docker Desktop for Windows, `host.docker.internal` resolves automatically inside Linux containers to the host's internal IP. **No `extra_hosts` entry is required, and none is added** — the approved topology is Docker Desktop on Windows. *(Bare-Linux Docker Engine would need `extra_hosts: ["host.docker.internal:host-gateway"]`, but that is a different, out-of-scope topology; this stack does not include it.)*
- **Host side (Python client).** Docker Desktop maintains a `host.docker.internal` entry in the Win32 hosts file, so the name resolves on Windows too.
- **Same endpoint.** Zitadel's `8080` is published `8080:8080` (all interfaces), so both resolutions terminate at the same published port and the same Zitadel, which advertises issuer `http://host.docker.internal:8080`.

**Why publish must be all-interfaces:** `host.docker.internal` points to the host's internal IP, **not** the host's `127.0.0.1`. If Zitadel were published as `127.0.0.1:8080:8080`, the host client could still reach it but the **container would get connection-refused**. Publish as `8080:8080`.

**Documented fallback (the approved resolution mitigation — a hosts-file entry).** If host-side resolution is not working (e.g. WSL 2 engine with the Win32-hosts setting disabled), add a static hosts entry on the host. As Administrator, append to `C:\Windows\System32\drivers\etc\hosts`:

```
127.0.0.1 host.docker.internal
```

On the host this is correct because Zitadel's `8080` is published to the host, so `127.0.0.1:8080` reaches it. This host-side mapping need not equal the container-side IP — the container still resolves `host.docker.internal` to the host internal IP via Docker Desktop — because both terminate at the same published `8080`. **Caveat:** do **not** add this line if Docker Desktop is already managing the entry (duplicates cause flaky resolution). Verify first with `Resolve-DnsName host.docker.internal` and `curl http://host.docker.internal:8080/.well-known/openid-configuration` from both the host and a throwaway container; only add the static line when Docker is **not** managing it. *(The "127.0.0.1-only host service is unreachable via host.docker.internal" claim is high-confidence inference from how the name resolves plus loopback bind semantics, not a verbatim Docker doc quote — but the Win32-hosts and container-auto-resolution behaviors are documented.)*

## 9. Run Instructions

```powershell
# 0. From the repo root (D:\projects\llm-chat) on Docker Desktop for Windows.

# 1. Pre-flight: confirm the three host ports are free (the user's environment has hit
#    a dual-listener 7777 collision before — native + Docker both binding). If any line
#    below returns a row, stop and free that port first.
Get-NetTCPConnection -LocalPort 7777,7878,8080 -State Listen -ErrorAction SilentlyContinue

# 2. Create and fill the env file.
cp .env.example .env
# Edit .env:
#   ZITADEL_MASTERKEY   -> openssl rand -hex 16     (exactly 32 hex chars; one-shot, do not change later)
#   POSTGRES_PASSWORD   -> any strong password
#   LLM_CHAT_AUTH_TOKEN -> openssl rand -hex 32     (shared by manager + host worker)

# 3. Start the REAL worker FIRST (uses your ~/.claude), BEFORE compose, because the
#    manager's startup probe of :7878 is fatal. Approve the Windows Firewall prompt for
#    the 0.0.0.0 bind on port 7878 if it appears.
.\deploy\compose\run-worker.ps1

# 4. Bring up the server side. Compose orders: postgres -> zitadel -> zitadel-init -> manager.
docker compose up -d
# Wait until `docker compose ps` shows zitadel healthy and zitadel-init Exited(0),
# and `.\secrets\kabytech-key.json` and `.\secrets\project_id` exist.

# 5. Run the Python reference client (host) for a full round-trip.
python clients/python/llm_chat_client.py `
  --issuer  http://host.docker.internal:8080 `
  --project (Get-Content -Raw .\secrets\project_id).Trim() `
  --key-file .\secrets\kabytech-key.json `
  --manager ws://localhost:7777/chat `
  --send "hello"
# Expect an 'a' (answer) frame and exit code 0.

# CLEAN RESET (when starting over): wipe Zitadel state AND the host secrets together,
# or the stale kabytech key will not match the fresh Zitadel instance.
#   docker compose down -v
#   Remove-Item -Recurse -Force .\secrets
```

**Client env-var names (if driving the client via environment instead of flags).** The shipped client reads **different** env var names from the manager's; do not assume they match. Verified from `clients/python/llm_chat_client.py:36-43`:

| Client flag | Client env var | NOT |
|---|---|---|
| `--issuer` | `ZITADEL_ISSUER` | — |
| `--project` | `PROJECT_ID` | not `ZITADEL_PROJECT_ID` |
| `--key-file` | `KABYTECH_KEY` | — |
| `--manager` | `MANAGER_WS` | — |

Setting `ZITADEL_PROJECT_ID` (the manager's name) will **not** be picked up by the client. The documented flow above uses explicit CLI flags to avoid this footgun.

**Standalone worker dev (outside compose).** Because `LLM_CHAT_WS_BIND` is now required with no code default, run the standalone worker with `npm run dev` (from `worker/`), **not** `npm run tauri dev` — the `dev` script supplies `LLM_CHAT_WS_BIND=127.0.0.1` and `LLM_CHAT_WS_PORT=7878` via `cross-env` (see §6). Likewise use `npm run build` instead of `npm run tauri build`. Without these vars the worker fails fast at startup with an error naming `LLM_CHAT_WS_BIND`. The same `npm run tauri dev`/`npm run tauri build` references in the docs are updated to match: **README.md:52** (`tauri dev`→`dev`) and **README.md:58** (`tauri build`→`build`), and **docs/architecture.md:108** and **docs/architecture.md:281** (`tauri dev`→`dev`) (see §6).

## 10. Testing & Verification

1. **Compose lint:** `docker compose config` parses with no errors and shows the four services, three named volumes, the `./secrets` bind, the restart policies, and the `depends_on` conditions. `docker compose config --quiet` exits 0.
2. **Healthcheck-gated boot:** with the worker already running (§9 step 3), `docker compose up -d` then poll `docker compose ps` until `postgres` and `zitadel` are `healthy` and `zitadel-init` is `Exited (0)`. The manager must not start before `zitadel-init` completes (verify via `docker compose logs manager`).
3. **Manager is in JWT mode, not silent shared-token fallback:** `docker compose logs manager` shows "Zitadel auth enabled" with the resolved `issuer/audience/project_id`, **not** "Zitadel auth NOT configured — falling back to shared-token auth." (This catches a partial `manager.generated.env`.)
4. **Provisioner artifacts present:** assert `./secrets/kabytech-key.json` exists and is valid JSON containing `"type":"serviceaccount"`, `keyId`, `key` (PEM), `userId`; assert `./secrets/project_id` and `./secrets/kabytech_user_id` are non-empty; assert the `genenv` volume's `manager.generated.env` defines `ZITADEL_PROJECT_ID` and `ZITADEL_AUDIENCE` (equal, non-empty values).
5. **Full client round-trip returns `a` and exits 0:** run the §9 step-5 command with the worker running. Assert the client receives an `a` frame for `"hello"`, the `a` frame carries a `seq` field, the client sends `confirm{seq}`, and **the process exits 0**. (Exit-0 — not merely "an `a`-typed frame appeared" — is the real pass criterion, because it exercises the full `seq`/confirm handshake; a missing `seq` would make the client raise `KeyError`.)
6. **Provisioner idempotency:** re-running the provisioner against the **same** Zitadel instance (`docker compose run --rm zitadel-init`) must be a **no-op** that exits 0 — create calls return `409` (treated as success), and key generation is skipped because the on-disk `kabytech_user_id` matches the existing `kabytech` user. Confirm `kabytech-key.json` was **not** rewritten (compare mtime/hash). Separately, after a `docker compose down -v` **without** deleting `./secrets`, the next provisioner run must **regenerate** the key (because the on-disk `userId` no longer exists in the fresh instance) rather than ship a stale key — verify the new `kabytech_user_id` differs and the round-trip still returns `a`.
7. **Loopback parity + fail-fast on missing address vars:**
   - **(a) Loopback parity (address vars SET to `127.0.0.1`):** build and run the manager and worker with the three address vars set to `127.0.0.1` (`MANAGER_BIND=127.0.0.1`, `MANAGER_BACKEND_HOST=127.0.0.1`, and — for a spawned worker — `LLM_CHAT_WS_BIND=127.0.0.1` forwarded by `spawn_instance`), and with `MANAGER_BACKEND_PORTS` unset so the manager spawns local workers. Confirm they behave **exactly as today's loopback**: the manager binds `127.0.0.1`, spawns local workers dialing `127.0.0.1`, and a loopback `/chat` still works end-to-end.
   - **(b) Fail-fast (any one address var UNSET):** with **any one** of `MANAGER_BIND`, `MANAGER_BACKEND_HOST`, or `LLM_CHAT_WS_BIND` unset/empty, the corresponding binary **fails fast at startup** — assert a **non-zero exit** and a clear error message **naming the missing var**. (The mode toggles `MANAGER_BACKEND_PORTS` / `LLM_CHAT_AUTH_TOKEN` remain presence-based: unset still equals today's behavior and is not part of this fail-fast assertion.)

## 11. Risks & Mitigations

- **Management-API version drift / pinned tag.** The v1 `AddProject/AddProjectRole/AddMachineUser/AddUserGrant/AddMachineKey` endpoints are deprecated (superseded by v2 resource services). **Mitigation:** pin a concrete tag (this spec uses `v3.4.10`, to be confirmed per §4.2), never `:latest`, and **verify the pinned tag still exposes those v1 endpoints**; if upgrading Zitadel, re-verify the provisioner's call surface (and consider porting to v2 services) before bumping.
- **`_search` recovery endpoints unverified.** The 409-recovery lookups in §4.3 steps 1 and 3 (`projects/_search`, `users/_search`) are not in the verified findings. **Mitigation:** confirm their exact path/body against the pinned tag's reference before implementing; the clean-boot path (which wipes Zitadel + `./secrets` together) does not exercise them, so they are a secondary concern.
- **Port collisions on the host.** `7777` (manager), `7878` (worker), and `8080` (Zitadel) must be free. The user's environment has previously hit a **dual-listener 7777 collision** (native + Docker both binding 7777). **Mitigation:** the §9 pre-flight `Get-NetTCPConnection -LocalPort 7777,7878,8080 -State Listen` check before bring-up; free any occupied port first.
- **Clock skew → "token validation failed".** The JWT-bearer assertions carry `iat/exp` windows (client `exp = now+300`; provisioner `exp = now+3600`). If the Windows host clock and a container clock drift apart, token/issuer validation fails with an opaque error. **Mitigation:** Docker Desktop normally keeps container time synced; flag clock skew as a candidate cause when "token validation failed" appears despite correct config.
- **`host.docker.internal` resolution caveats.** Container-side resolution is automatic under Docker Desktop, but host-side resolution depends on Docker Desktop maintaining the Win32 hosts entry (governed by a WSL 2-engine setting). **Mitigation:** the §8 hosts-file fallback plus the verification commands; do not duplicate the entry if Docker already manages it. Publish Zitadel `8080:8080` (all interfaces), never `127.0.0.1:8080:8080`.
- **HTTP-only issuer (security).** `ExternalSecure=false` means no TLS and non-`Secure` cookies — credentials/tokens travel in cleartext. **Mitigation:** documented as **local-dev-only** in the README and §2; never bind/expose this stack beyond the local machine.
- **Secrets must not be committed.** `./secrets/kabytech-key.json` is a live RSA private key, and the repo's `.gitignore` does **not** currently ignore `secrets/`. **Mitigation:** the §6-mandated `.gitignore` edit appending `secrets/` (and the `.dockerignore` exclusion, which is a separate concern for image builds).
- **Windows Firewall prompt on the worker's `0.0.0.0` bind.** Starting the worker with `LLM_CHAT_WS_BIND=0.0.0.0` triggers a one-time Windows Defender Firewall prompt. **Mitigation:** `run-worker.ps1` warns the user to approve it (private networks only). If declined, the manager cannot reach `:7878` and `/chat` fails at the bridge step.
- **Manager startup probe is fatal.** In external-backend mode the manager's `wait_for_tcp(7878, 90)` **propagates a timeout out of `main()` and exits** if the worker is not up within ~45 s. **Mitigation:** the §7.1 ordering contract (start the worker before `docker compose up`) plus `restart: unless-stopped` on the manager so any transient window self-heals. This probe is **not** non-fatal — do not assume lazy dialing saves a missing worker at startup.
- **Zitadel first-init is one-shot.** The bootstrap admin key at `/machinekey/zitadel-admin-sa.json` is written **only** on the first init against a fresh DB; forcing re-creation can throw `duplicate key value violates unique constraint`. **Mitigation:** the `machinekey` volume persists the key; to start clean, `docker compose down -v` **and** `Remove-Item -Recurse -Force .\secrets` together (see §9) so init and the host key regenerate coherently.
- **Masterkey is irreversible.** `ZITADEL_MASTERKEY` must be exactly 32 chars and cannot change after first init without losing encrypted data. **Mitigation:** generate once with `openssl rand -hex 16`, store in `.env`, never edit afterward.
- **Zitadel healthcheck tool availability** (single verification item, not a design unknown). The `/debug/healthz` endpoint is fixed; the in-container tool (`wget`/`curl`) may be absent from the pinned image. **Mitigation:** verify once against the pinned tag at first bring-up; if both are absent, fall back to `["CMD", "/app/zitadel", "ready"]` and **empirically confirm** it works on this HTTP deployment, because (per issue #9495) the `ready` subcommand has historically attempted HTTPS even with TLS disabled — disabling TLS does **not** guarantee it is neutralized.

## 12. Open Questions

These are genuine items requiring confirmation (one-time empirical checks or upstream-reference lookups), not settled design:

1. **Zitadel `_search` recovery endpoints/bodies (§4.3 steps 1, 3).** `POST /management/v1/projects/_search` (`nameQuery`/`TEXT_QUERY_METHOD_EQUALS`) and `POST /management/v1/users/_search` (`userNameQuery`) and the premise that project `name` is non-unique are **not in the verified findings**; confirm against the pinned Zitadel API reference before implementing the 409-recovery branches.
2. **`GET /auth/v1/users/me` org-id shape (§4.3 org context).** The exact field exposing the SA's org id is unverified; if it differs, fall back to omitting `x-zitadel-orgid` (documented SA-org fallback).
3. **Zitadel image tag (§4.2).** `v3.4.10` is a concrete pin to be confirmed; verify the chosen tag exposes the v1 Management-API endpoints the provisioner uses and pick the minimal TLS-disable mechanism (`--tlsMode disabled` / `--masterkeyFromEnv` / `ZITADEL_TLS_ENABLED=false`) the tag honors.
4. **Zitadel healthcheck tooling (§4.2/§11).** Confirm whether the pinned image ships `wget`/`curl`; if not, validate the `/app/zitadel ready` fallback empirically on this HTTP deployment (the issue-#9495 HTTPS-on-disabled-TLS quirk is possibly still live).
5. **Postgres connection wiring (§4.1).** The discrete-field admin/user split + per-role SSL-mode (wiring B) is an implementation choice beyond the approved minimal "Zitadel's DB"; confirm at build time or fall back to the single-connection wiring (A).
6. **Optional manager fail-fast hardening (§4.4).** Making the manager itself error when `ZITADEL_ISSUER` is set but `ZITADEL_AUDIENCE`/`ZITADEL_PROJECT_ID` are missing (instead of silently degrading to shared-token auth) is a small, recommended code change **beyond the approved scope** — the `entrypoint.sh` assertion covers the compose path either way. Adopt only if approved.
