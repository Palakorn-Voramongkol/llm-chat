# llm-chat — stack architecture

## Overview

**llm-chat turns the `claude` CLI into a programmable, multi-tenant chat
service.** There is no Anthropic API call anywhere in this system. Instead, a
real `claude` Code process runs per session and the system feeds it questions
and reads back its answers as structured JSON. Everything a normal `claude`
session can do — tools, file reads, its full reasoning — happens for free,
because it *is* a real `claude` session being driven programmatically.

**How the worker drives `claude` — the default is the source-of-truth
transport.** The worker runs `claude` in headless print mode over **raw pipes**:

```
claude -p --input-format stream-json --output-format stream-json \
       --verbose --dangerously-skip-permissions
```

One long-lived process per session; each question is written to its stdin as a
stream-json `user` message, and the worker reads Claude's **actual answer text**
from the `result` events on stdout — real newlines, real markdown, no terminal
scraping. A legacy **PTY transport** (`LLM_CHAT_TRANSPORT=pty`) still exists for
the standalone desktop app: it runs `claude` in a pseudo-terminal (ConPTY on
Windows, a PTY on unix) and a JS parser reads the scrolling xterm buffer. Stream-
json is the default for every managed/server path; PTY is the GUI-only fallback.

Four tiers stack up:

- **Worker** — owns the bottom of the stack: spawns and drives `claude`
  sessions and relays them over WebSocket. Two binaries: a Tauri desktop app
  (`llm-chat-worker`) and a windowless server (`llm-chat-headless`).
- **Manager** — the multiplexer and front door for chat. Routes each incoming
  caller to a session on the least-loaded worker, hides the worker plumbing
  behind a single typed `/chat` protocol, authenticates callers, and persists
  every question in a durable FIFO queue so an answer is never silently lost.
- **Admin Console** — an operator-only management plane: a Rust BFF
  (`admin-api`) that is the *only* component allowed to call Zitadel's admin
  APIs, fronted by a Next.js web UI (`admin-web`). It manages users, roles,
  apps and org policy, and monitors live chat sessions/workers via the
  manager's `/control`.
- **Client** — any consumer of `/chat`. The reference one is a small Python
  package (`clients/python`); JS equivalents live in `tests/`.

**The lifecycle of one question, in a sentence:** a client opens an
authenticated WebSocket to the manager's `/chat`, the manager opens a private
`claude` session on a worker, sends the question, waits for the worker to surface
the finished answer from Claude's `result` event, pairs it back to the question
in FIFO order, and streams it to the client as an `a` frame — recording every
step in SQLite/Postgres along the way.

Three properties are worth holding onto while reading the rest of this page:

1. **Two data planes.** The **chat plane** (client ↔ manager ↔ worker) is
   WebSocket-only — every endpoint is a WS upgrade. The **admin plane**
   (browser ↔ admin-web ↔ admin-api ↔ Zitadel/manager) is HTTP/REST + JSON.
2. **Layered auth.** Outside callers are gated by Zitadel-issued JWTs (role
   `chat.user` for chat, `chat.admin` for the admin/ops surfaces); the internal
   manager↔worker hops use a shared loopback token. (See
   [Auth model](#auth-model).)
3. **Durable and at-least-once.** The manager's queue survives a crash
   mid-question, and the client `confirm`s receipt so undelivered answers are
   auditable.

For the per-endpoint command reference, see
[`manager-interface.md`](manager-interface.md) and
[`worker-interface.md`](worker-interface.md). This page is the map that sits
above them.

---

## Components

| Component | Lang | Listens | Role |
|---|---|---|---|
| **Worker** (`worker/`) | Rust + JS (Tauri 2) | `ws://<bind>:7878` | Spawns `claude` and drives it (stream-json over pipes, or legacy PTY), relays I/O, surfaces Q&A. Two binaries: `llm-chat-worker` (desktop) / `llm-chat-headless` (no window). |
| **Manager** (`manager/`) | Rust | `ws://<bind>:7777` | Routes `sid → worker`, verifies client auth, persists a FIFO Q→A queue, exposes the typed `/chat` protocol + a `chat.admin`-gated `/control` ops surface. Spawns workers **or** connects to pre-started ones. |
| **admin-api** (`admin-api/`) | Rust (axum) | `http://<bind>:7676` | Operator BFF. The **only** Zitadel admin caller. OIDC operator login + a `chat.admin`-gated `/api/*` JSON surface; proxies the manager's `/control` for live-session monitoring. |
| **admin-web** (`admin-web/`) | TypeScript (Next.js 16) | `http://<bind>:3000` | The operator Console UI (users, roles, apps, project/org, audit, sessions/workers, dashboard). Talks only to admin-api. |
| **Client** (`clients/python/`) | Python | — | Reference chat client. Authenticates to Zitadel as a machine user, connects to the manager's `/chat`, does Q→A. (JS equivalents in `tests/`.) |
| **Zitadel** | — | `:8080` (local) / `https://id.palakorn.com` (prod) | OIDC IdP. Issues user/machine JWTs and publishes the JWKS the manager and admin-api verify against. |
| **Postgres** | — | `:5432` (internal) | Backing store for Zitadel and (optionally) the manager queue. |
| **nginx** (`deploy/manager/`) | — | `:443` (prod) | TLS terminator. Proxies the WS chat paths to the manager. |

```
 ADMIN PLANE (HTTP/REST)                         CHAT PLANE (WebSocket)
 ┌───────────┐  https + OIDC cookie    ┌───────────┐  wss + Zitadel JWT (chat.user)
 │ browser   │ ──────────────────────► │ client    │ ──────────────────────────────┐
 │ (operator)│   /api/*                │ (python)  │  /chat                          │
 └─────┬─────┘                         └───────────┘                                 ▼
       │                                                          ┌────────────────────────────────┐
       ▼                                                          │ MANAGER  (:7777)               │
 ┌───────────┐  http  ┌──────────────────────────┐               │ • verifies JWT vs Zitadel JWKS │
 │ admin-web │ ─────► │ admin-api (:7676)        │               │   (chat.user) OR shared token  │
 │  (:3000)  │ /api/* │ • OIDC operator login    │               │ • /chat → routes sid → worker  │
 └───────────┘        │ • chat.admin gate        │               │ • /control (chat.admin) ops    │
                      │ • ONLY Zitadel admin call│──┐            │ • FIFO queue chat_question     │
                      └─────────────┬────────────┘  │ ws /control └───────┬────────────────────────┘
                                    │ mgmt/admin v1  │ (chat.admin   shared │ token (loopback/host)
                                    ▼                │  JWT)         ┌──────▼───────────────────────┐
                              ┌───────────┐          └──────────────│ WORKER backend :7878         │
                              │  Zitadel  │ ◄─── JWKS ──────────────│ stream-json: claude -p (pipe)│
                              │  (:8080)  │     (manager+admin-api) │ or legacy PTY → claude.exe   │
                              └───────────┘                         │ /control /s/ /qa/            │
                                                                    └──────────────────────────────┘
```

---

## The worker: two binaries, two transports, two modes

`worker/` is one crate that builds two binaries:

- **`llm-chat-worker`** (`src/main.rs`, default `gui` feature) — a Tauri 2
  desktop app. Run standalone it's a terminal window with `claude` live in it;
  run managed (manager-spawned) the window is hidden.
- **`llm-chat-headless`** (`src/bin/headless.rs`) — the windowless server. Built
  `--no-default-features` it links **no Tauri/WebKitGTK at all**, so it runs on a
  CLI-only host with no display. This is the binary used as a pre-started backend
  (e.g. the native worker the Docker compose manager connects to).

**Transport** (`LLM_CHAT_TRANSPORT`, `worker/src/lib.rs`):

| Value | Default? | How it drives `claude` | Needs a display? |
|---|---|---|---|
| `stream-json` | **yes** | `claude -p --input-format stream-json --output-format stream-json` over raw pipes; reads `result` events. | **No** — no PTY, no xterm, no webview. |
| `pty` | legacy | ConPTY/PTY + xterm.js; `frontend/claude_cli_parser.js` scrapes the buffer and calls `broadcast_qa`. | Yes — needs a webview (xvfb on headless Linux for the GUI build). |

**Environment** decides port, bind, auth and visibility:

| Env var | Set by | Effect |
|---|---|---|
| `LLM_CHAT_WS_PORT` | manager / launcher | WS server port. Default `7878`. |
| `LLM_CHAT_WS_BIND` | launcher | Bind address. **REQUIRED — no default.** `127.0.0.1` for loopback; `0.0.0.0` when a container (e.g. the Dockerized manager via `host.docker.internal`) must reach it. |
| `LLM_CHAT_AUTH_TOKEN` | manager | The shared loopback token. **Presence = "managed" mode**; absent = standalone (token read/generated from `auth.token`). |
| `LLM_CHAT_TRANSPORT` | launcher | `stream-json` (default) or `pty`. |
| `LLM_CHAT_STEALTH=1` | manager (GUI build) | Hide the window + skip the taskbar. Irrelevant to `llm-chat-headless`. |

> **Only the legacy PTY/GUI path needs a webview.** With the default stream-json
> transport the answer comes straight from Claude's `result` event, so a headless
> backend needs no display, no xterm, no JS parser. The `xvfb-run` wrapping
> described in older docs applies only to the GUI build running the PTY transport.

---

## Auth model

### Chat plane: client → manager

Decided at manager startup (`manager/src/main.rs`):

- **Zitadel JWT** when `ZITADEL_ISSUER` is set. On every WS upgrade the manager
  extracts the `Bearer` token, verifies it against the cached **JWKS**
  (refreshed hourly), checks `iss` + `aud`, and **requires the project role
  `chat.user`** (under `urn:zitadel:iam:org:project:<project_id>:roles`).
  Missing/invalid → 401; wrong role → 403.
- **Shared-token fallback** when `ZITADEL_ISSUER` is *unset*. The manager
  compares the presented token against its own per-process token
  (constant-time). The **fully-offline local path** — no Zitadel needed.

The reference **Python client only implements the Zitadel JWT-bearer flow** — it
signs an assertion with a machine-user key and exchanges it for an access token.
Running it requires real Zitadel credentials (issuer + project + a machine key
such as `kabytech-key.json`).

### Ops surface: manager `/control` requires `chat.admin`

`/control` exposes **every** user's session ids and can close/inspect sessions,
so it is gated above `chat.user`: the manager rejects any caller whose JWT does
not carry **`chat.admin`** (`manager/src/main.rs`, fail-closed). This is the
surface the admin-api BFF uses to monitor live chat sessions and workers; a
plain `chat.user` token cannot reach it.

### Admin plane: operator → admin-api

The admin-api authenticates an **operator** via OIDC (browser `/login` →
`/callback` → session cookie) and gates every `/api/*` handler on the
**`chat.admin`** role in the operator's access-token JWT (`admin-api/src/auth.rs`;
not an operator → 403). The admin-api is the **only** component that calls
Zitadel's Management/Admin v1 APIs — admin-web never talks to Zitadel directly.
To reach the manager's `chat.admin` `/control`, the admin-api mints its own
project-audience `chat.admin` token.

### Internal: manager ↔ worker (and standalone worker)

A **shared token** used on the loopback/host hops only (`/s/`, `/qa/`, and the
worker's own `/control`). In managed mode the manager generates a random 32-byte
per-process token, writes it to `auth.token`, and propagates it to each worker
via `LLM_CHAT_AUTH_TOKEN`; in the Docker stack a fixed `LLM_CHAT_AUTH_TOKEN` is
set on both the manager and the pre-started worker so they share it. Browser-
style `Origin` headers are rejected outright on both manager and worker.

**Token file location** (same dir on both, so DBs sit beside it):
- Windows: `%LOCALAPPDATA%\com.llm-chat.app\auth.token`
- Linux/macOS: `$XDG_DATA_HOME/com.llm-chat.app/auth.token` (default `~/.local/share/...`)

ACL is tightened to the current user (`icacls` on Windows, `chmod 0600` on unix).

---

## Data flow — one `/chat` round-trip

`handle_chat` in `manager/src/main.rs` orchestrates this:

1. **Connect.** Client opens a WS to the manager `/chat` with its `Bearer` JWT.
   Manager verifies it (JWKS + `chat.user`).
2. **Open a session.** Manager picks the least-loaded worker, sends `{cmd:open}`
   over that worker's `/control`; the worker spawns a fresh `claude` session and
   returns a `sessionId`. Manager records `sid → port` **and the authenticated
   owner (JWT sub)** so `/control` can report whose session it is.
3. **Initialized.** Manager → client: `{type:initialized, sid, backendPort, connectionId}`.
4. **Bridge.** Manager opens two loopback/host WS to the owning worker:
   `/s/<sid>` (input) and `/qa/<sid>` (answers).
5. **Warm up.** Sleeps `MANAGER_CHAT_WARMUP_SECS`. The worker reports its
   transport in the `{cmd:open}` reply, so the manager picks the default: **0 for
   stream-json** (claude reads stdin from the start — no warmup) and **8 s for
   the legacy PTY transport** (claude needs ~5–8 s to reach its prompt *and* the
   JS parser must hook `onData` first). An explicit env value overrides both.
6. **Ask.** Client → `{type:q, id, text, attachments?}`.
   - Attachments are first saved via the worker's `save_attachment`; their
     on-disk paths are rewritten into `Read the file at <path>.` instructions
     prepended to the text.
   - Manager `INSERT`s a `pending` row (the autoincrement `seq` is both the FIFO
     key and the receipt id) → emits `{type:ack, seq}` **immediately**.
   - Manager writes the text to `/s/<sid>` (stream-json: one `user` message;
     PTY: body then a separate `\r`) → marks the row `sent`.
7. **Answer.** The worker feeds `claude` → Claude answers. stream-json: the
   `result` event's text is forwarded as one final answer. PTY: the JS parser
   reads the xterm buffer and `broadcast_qa` fans out `{num, answer}` repaints.
8. **Pair & deliver.** Each `/qa` event carries `final`. stream-json sends one
   complete `result` per question (`final:true`) → delivered **immediately**;
   the legacy PTY path streams partial repaints → debounced
   `MANAGER_CHAT_SETTLE_MS` (default 3 s) to capture the *final* text. Either way
   the manager pops the **oldest `sent` row** for this connection (FIFO), marks
   it `answered`, and sends `{type:a, id, seq, text, timeIn, timeOut, latencyMs}`.
9. **Confirm.** Client → `{type:confirm, seq}` → manager marks the row
   `confirmed` (audit closure).
10. **Teardown.** On client disconnect the manager closes the session
    (`{cmd:close}` → worker tears down the session) and drops it from the
    routing map.

Row status machine: `pending → sent → answered → confirmed` (happy path);
`any → error` on failure.

---

## Persistence

| Store | Owner | Location | Holds |
|---|---|---|---|
| `chat_question` | manager | `manager.sqlite` (default) or Postgres via `MANAGER_DB_URL` | Every `/chat` question: text, status, attachment paths, answer, timestamps. The durable FIFO. |
| `pty_input` | each worker | `backend-<port>.sqlite` | Every input byte written to a session, status `pending`/`written`/`error`. Audited via the manager's `fifo` cmd. |
| `auth.token` | manager (or standalone worker) | `…/com.llm-chat.app/auth.token` | The shared loopback token. |
| QA logs | worker | `…/com.llm-chat.app/` (rolling `qa_*.log`) | Human-readable Q&A transcript per session. |
| Zitadel DB | Zitadel | Postgres | Users, roles, grants, projects, apps, event log (the audit source). |

Default SQLite files live in `%LOCALAPPDATA%\com.llm-chat.app\` (Windows) /
`~/.local/share/com.llm-chat.app/` (unix). admin-web holds **no** state; admin-api
holds only **ephemeral operator sessions** (`tower-sessions`, in-memory store, 8 h
idle / 12 h absolute expiry). All domain state lives in Zitadel (identity) and the
manager (chat queue) — restart admin-api and operators simply re-authenticate.

---

## Endpoints at a glance

**Chat plane (WebSocket):**

| Path | Manager | Worker | Type |
|---|:---:|:---:|---|
| `/chat` | ✓ | — | Typed Q→A protocol (auto-spawns a session per connection). `chat.user`. |
| `/control` | ✓ (chat.admin JWT) | ✓ (shared token) | JSON RPC: spawn/close/introspect sessions, query queue/clients; manager adds `instances`. |
| `/s/<sid>` | ✓ | ✓ | Bidirectional raw input bridge. |
| `/s/new` | ✓ | — | Auto-spawn + bridge + auto-close. |
| `/qa/<sid>` | ✓ | ✓ | Read-only parsed Q&A stream. |
| `/` | ✓ | ✓ | One-shot JSON list of session ids. |

**Admin plane (HTTP, admin-api `:7676`):** `/login`, `/callback`, `/logout`
(OIDC, ungated) and the `chat.admin`-gated JSON surface `/api/*` —
`/api/me`, `/api/users…`, `/api/roles…`, `/api/apps…`, project/org policy,
`/api/events` + `/api/capabilities` (audit), and `/api/status`,
`/api/chat-sessions`, `/api/signins` (live monitoring, the last proxied from the
manager's `/control`).

Full payloads: [`manager-interface.md`](manager-interface.md),
[`worker-interface.md`](worker-interface.md).

---

## Ports & config

| Port | What |
|---|---|
| `3000` | admin-web (operator Console UI). **Start here.** |
| `7676` | admin-api (BFF). Auth-gated — `401` without an operator session. |
| `7777` | Manager (`MANAGER_PORT` / `MANAGER_BIND`). nginx fronts it on prod `:443`. |
| `7878`, `7879`, … | Worker backends. |
| `8080` | Zitadel (local). `id.palakorn.com:443` in prod. |
| `5432` | Postgres (internal). |

**Manager env:** `MANAGER_PORT`, `MANAGER_BIND` (**required**),
`MANAGER_STEALTH`, `MANAGER_CHAT_WARMUP_SECS` (default 0 stream-json / 8 PTY),
`MANAGER_CHAT_SETTLE_MS` (PTY debounce, default 3000), `MANAGER_DB_URL`
(optional Postgres), `ZITADEL_ISSUER` / `ZITADEL_AUDIENCE` /
`ZITADEL_PROJECT_ID`, and one of:
- **Spawning mode:** `MANAGER_INSTANCES` (default 2), `MANAGER_START_PORT`
  (default 7878), `LLM_CHAT_EXE` — the manager launches the workers itself.
- **External-backend mode:** `MANAGER_BACKEND_HOST` (**required**) +
  `MANAGER_BACKEND_PORTS` (comma-separated) — the manager does **not** spawn;
  it waits for pre-started workers at that host:port. (Used by the Docker stack:
  `host.docker.internal:7878`.)

**admin-api env:** `ZITADEL_ISSUER`, the admin OIDC client id/secret, the admin
service-account key, `ADMIN_API_BIND`, and `MANAGER_CONTROL_URL`
(e.g. `ws://manager:7777/control`) to enable the live-sessions panel.

**admin-web env:** `ADMIN_API_ORIGIN` (e.g. `http://admin-api:7676`).

**Client env / flags** (`clients/python`): `--issuer` / `ZITADEL_ISSUER`,
`--project` / `PROJECT_ID`, `--key-file` / `KABYTECH_KEY`, `--manager` /
`MANAGER_WS` (default `ws://127.0.0.1:7777/chat`).

---

## Deployment

### Docker Compose (self-host / local full stack)

`docker-compose.yml` brings up the containerized plane:

| Service | Notes |
|---|---|
| `postgres` | Backing store for Zitadel. |
| `zitadel` | OIDC IdP on `:8080`. |
| `zitadel-init` | One-shot provisioner (project, roles, apps, the admin + machine users, `./secrets/*`). Idempotent — a re-run on an already-provisioned volume exits non-zero on a benign `409 project already exists`. |
| `admin-api` | Operator BFF on `:7676`. |
| `admin-web` | Console UI on `:3000`. |
| `manager` | Chat front door on `:7777`, in **external-backend mode** (`MANAGER_BACKEND_HOST=host.docker.internal`, `MANAGER_BACKEND_PORTS=7878`). |

The **worker runs natively on the host** (it spawns `claude`), not as a compose
service. The manager reaches it via `host.docker.internal:7878`, so the worker
must bind `0.0.0.0` and share the manager's `LLM_CHAT_AUTH_TOKEN`:

```bash
docker compose up -d                      # postgres, zitadel, admin-api, admin-web, manager

# native worker the manager connects to (windowless, stream-json):
cargo build --bin llm-chat-headless --no-default-features
LLM_CHAT_WS_BIND=0.0.0.0 LLM_CHAT_WS_PORT=7878 \
LLM_CHAT_AUTH_TOKEN=<same token as the manager> \
  ./worker/target/debug/llm-chat-headless
```

Then open **http://localhost:3000** (operator Console) or drive `/chat`
(`ws://localhost:7777/chat`) with a Zitadel-authenticated client.

### Production (systemd + nginx)

GitHub Actions (`.github/workflows/`) on push to `main`:

1. `cargo build --release` for `manager/` and `worker/` on the runner.
2. `scp` binaries to the server's deploy staging dir.
3. SSH (forced-command, allowlisted) `install-manager-binary` /
   `install-worker-binary`, then `restart-manager`.
4. Health probe Zitadel's OIDC discovery doc.
5. End-to-end probe: the Python client authenticates as the `kabytech` machine
   user and sends `hello` to `wss://api.palakorn.com/chat`, expecting an `a` frame.

In this topology the manager runs as the `llm-chat` systemd service in
**spawning mode** — it launches the worker binaries itself
(`MANAGER_INSTANCES`), so they are **not** separate services. nginx terminates
TLS and proxies the WS chat paths. See
[`deploy/manager/README.md`](../deploy/manager/README.md) and
[`deploy/zitadel/README.md`](../deploy/zitadel/README.md).

---

## Running locally

### Worker alone (desktop terminal)

```bash
cd worker
npm install
npm run dev
```

A window opens with `claude` live. No manager, no auth — you're the user.

### Worker alone (headless, debug bottom-up)

```bash
cargo build --bin llm-chat-headless --no-default-features
LLM_CHAT_WS_BIND=127.0.0.1 LLM_CHAT_WS_PORT=7878 \
  ./worker/target/debug/llm-chat-headless
```

Drive its `/s/<sid>` / `/qa/<sid>` / `/control` directly to verify the lowest
layer before bringing the manager up on top.

### Full offline stack (no Zitadel)

Start the manager **without** `ZITADEL_ISSUER` (shared-token auth) in spawning
mode; it launches the worker(s) itself. Drive `/chat` or `/control` with a JS
client that reads the shared token from `auth.token` and passes it as `?token=`
(see `tests/`). The reference Python client does **not** work on this path — it
requires Zitadel credentials.

### Reference Python client (needs Zitadel)

```bash
pip install PyJWT requests websockets
PYTHONPATH=clients/python python -m llm_chat ask \
  --manager ws://127.0.0.1:7777/chat \
  --send "hello"
# credentials resolve from ./secrets (project_id, kabytech-key.json) or
# --issuer/--project/--key-file flags.
```
