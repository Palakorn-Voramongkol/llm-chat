# LLM Chat

**llm-chat turns the `claude` CLI into a programmable, multi-tenant chat
service** — with an operator Console for managing who can use it.

There is no Anthropic API call anywhere in this system. A real `claude` Code
process runs per session and the platform feeds it questions and reads back its
answers as structured JSON. Everything a normal `claude` session can do — tools,
file reads, its full reasoning — happens for free, because it *is* a real
`claude` session being driven programmatically.

The worker drives `claude` in headless print mode over raw pipes:

```
claude -p --input-format stream-json --output-format stream-json \
       --verbose --dangerously-skip-permissions
```

One long-lived process per session; each question is written to its stdin as a
stream-json `user` message, and the worker reads Claude's **actual answer text**
from the `result` events on stdout — real newlines, real markdown, no terminal
scraping. (A legacy PTY transport still exists for the standalone desktop app.)

## Architecture overview

The system splits into **two independent data planes**:

- **Chat plane** (WebSocket): `client → manager → worker → claude`. Every
  endpoint is a WS upgrade. Outside callers are gated by Zitadel JWTs carrying
  the `chat.user` role; the internal manager↔worker hops use a shared loopback
  token.
- **Admin plane** (HTTP/REST + JSON): `browser → admin-web → admin-api →
  Zitadel`. An operator-only management Console for users, roles, apps and org
  policy, plus live monitoring of chat sessions and workers. Gated on the
  `chat.admin` role; **admin-api is the only component that ever calls Zitadel's
  admin APIs**.

```
ADMIN PLANE  (HTTP / REST + JSON)             CHAT PLANE  (WebSocket)

┌─────────────────────────────┐               ┌──────────────────────────────────────┐
│ browser   (operator)        │               │ client   (python)                    │
└──────────────┬──────────────┘               │ machine-user JWT (chat.user)         │
               │ https + OIDC                 └───────────────────┬──────────────────┘
               ▼                                                  │ wss + JWT  /chat
┌─────────────────────────────┐                                   ▼
│ admin-web   (:3000)         │               ┌──────────────────────────────────────┐
│ Console UI → admin-api only │               │ MANAGER   (:7777)                    │
└──────────────┬──────────────┘               │ • verify JWT vs Zitadel JWKS         │
               │ /api/*                       │   (chat.user)  OR shared token       │
               ▼                              │ • /chat → routes sid → worker        │
┌─────────────────────────────┐ ws /control   │ • /control (chat.admin) ops          │
│ admin-api   (:7676)         ├──────────────>│ • durable FIFO queue                 │
│ • OIDC operator login       │               └───────────────────┬──────────────────┘
│ • chat.admin gate           │                                   │ shared token
│ • ONLY Zitadel admin caller │                                   ▼
└──────────────┬──────────────┘               ┌──────────────────────────────────────┐
               │ mgmt/admin v1                │ WORKER backend   (:7878)             │
               ▼                              │ • stream-json: claude -p (pipe)      │
┌─────────────────────────────┐               │ • or legacy PTY → claude.exe         │
│ Zitadel   (:8080)           │               │ • /control   /s/<sid>   /qa/<sid>    │
│ OIDC IdP — issues JWTs,     │               └───────────────────┬──────────────────┘
│ publishes JWKS              │                                   │ stream-json
└─────────────────────────────┘                                   ▼
                                              ┌──────────────────────────────────────┐
                                              │ claude   (one real CLI / session)    │
                                              └──────────────────────────────────────┘
```

> Both the **manager** and **admin-api** verify incoming JWTs against Zitadel's
> published **JWKS** (refreshed hourly); only **admin-api** ever calls Zitadel's
> Management/Admin v1 APIs. The worker never sees a JWT — the manager↔worker hop
> is authenticated by the shared loopback token.

### The lifecycle of one question

A client opens an authenticated WebSocket to the manager's `/chat`, the manager
opens a private `claude` session on the least-loaded worker, sends the question,
waits for the worker to surface the finished answer from Claude's `result`
event, pairs it back to the question in FIFO order, and streams it to the client
as an `a` frame — recording every step in SQLite/Postgres along the way:

1. **Connect.** Client opens a WS to `/chat` with its `Bearer` JWT; manager
   verifies it (JWKS + `chat.user`).
2. **Open a session.** Manager picks the least-loaded worker, which spawns a
   fresh `claude` session and returns a `sessionId`. Manager records
   `sid → worker` **and the authenticated owner** (JWT sub).
3. **Ask.** Client sends `{type:q, id, text}`. Manager `INSERT`s a `pending`
   row (its autoincrement `seq` is both the FIFO key and the receipt id), acks
   immediately, then writes the text to the worker as a stream-json `user`
   message.
4. **Answer.** The worker feeds `claude`; Claude answers; the `result` event's
   text is forwarded as one complete answer.
5. **Pair & deliver.** Manager pops the oldest `sent` row for this connection
   (FIFO), marks it `answered`, and sends `{type:a, id, seq, text, latencyMs}`.
6. **Confirm.** Client confirms receipt → manager marks the row `confirmed`
   (audit closure). Row status machine: `pending → sent → answered → confirmed`.

Three properties hold throughout: **two data planes** (WS chat vs. REST admin),
**layered auth** (Zitadel JWTs outside, shared loopback token inside), and
**durable, at-least-once delivery** (the queue survives a crash mid-question and
the client confirms receipt). See [`docs/architecture.md`](docs/architecture.md)
for the full treatment.

## Components

| Component | Lang | Listens | Role |
|---|---|---|---|
| **worker** (`worker/`) | Rust + JS (Tauri 2) | `:7878` | Spawns and drives `claude` (stream-json over pipes, or legacy PTY); relays I/O over WS. Two binaries: `llm-chat-worker` (desktop) / `llm-chat-headless` (no window). |
| **manager** (`manager/`) | Rust | `:7777` | Routes `sid → worker`, verifies client auth, persists a durable FIFO Q→A queue, exposes the typed `/chat` protocol + a `chat.admin`-gated `/control` ops surface. |
| **admin-api** (`admin-api/`) | Rust (axum) | `:7676` | Operator BFF. The **only** component allowed to call Zitadel's admin APIs. OIDC operator login + a `chat.admin`-gated `/api/*` JSON surface. |
| **admin-web** (`admin-web/`) | TypeScript (Next.js 16) | `:3000` | The operator Console UI (users, roles, apps, project/org, audit, sessions/workers, dashboard). Talks only to admin-api. |
| **client** (`clients/python/`) | Python | — | Reference chat client. Authenticates to Zitadel as a machine user, connects to `/chat`, does Q→A. (JS equivalents in `tests/`.) |
| **Zitadel** | — | `:8080` | OIDC IdP. Issues user/machine JWTs and publishes the JWKS the manager and admin-api verify against. |
| **Postgres** | — | `:5432` | Backing store for Zitadel (and optionally the manager queue). |

> **Platform note:** the worker uses ConPTY/Win32 for its desktop GUI build, but
> the default headless `llm-chat-headless` binary links no Tauri/WebKitGTK and
> runs on any CLI-only host — no display required.

## Quick start (full stack)

The containerized plane (Zitadel, admin-api, admin-web, manager) comes up via
Docker Compose; the **worker runs natively on the host** because it spawns
`claude`. The manager reaches it via `host.docker.internal:7878`, so the worker
must bind `0.0.0.0` and share the manager's `LLM_CHAT_AUTH_TOKEN`.

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

## Running a single layer (debug bottom-up)

When an end-to-end flow fails, isolate the lowest layer first.

**Worker alone (desktop terminal)** — a window with `claude` live, no manager,
no auth:

```bash
cd worker && npm install && npm run dev
```

**Worker alone (headless)** — drive its `/s/<sid>` / `/qa/<sid>` / `/control`
directly to verify the bottom layer before bringing the manager up on top:

```bash
cargo build --bin llm-chat-headless --no-default-features
LLM_CHAT_WS_BIND=127.0.0.1 LLM_CHAT_WS_PORT=7878 \
  ./worker/target/debug/llm-chat-headless
```

**Reference Python client** (needs Zitadel credentials):

```bash
pip install PyJWT requests websockets
PYTHONPATH=clients/python python -m llm_chat ask \
  --manager ws://127.0.0.1:7777/chat --send "hello"
# credentials resolve from ./secrets (project_id, kabytech-key.json) or
# --issuer/--project/--key-file flags.
```

## Layout

```
.
├── manager/              # Rust manager — auth, routing, durable FIFO queue (:7777)
├── worker/               # Tauri PTY/pipe backend — spawns & drives claude (:7878)
├── admin-api/            # Rust axum BFF — the only Zitadel admin caller (:7676)
├── admin-web/            # Next.js operator Console UI (:3000)
├── crates/zitadel-auth/  # Shared JWT/JWKS verifier — used by manager + admin-api
├── clients/
│   ├── python/           # Reference chat client (machine-user JWT auth)
│   ├── rust/             # Rust REPL client (binary: llm-chat) — port of python
│   └── tauri/            # Desktop chat client
├── deploy/
│   ├── compose/          # Docker stack: Dockerfiles + Zitadel provisioner (python)
│   ├── manager/          # systemd unit + nginx vhost (production)
│   ├── worker/           # worker deploy artifacts
│   └── zitadel/          # Zitadel + login UI deployment artifacts
├── tests/                # Cross-component WS integration tests (JS)
├── testcases/e2e/        # End-to-end case harness (single-backend, multi-session)
├── docs/                 # architecture.md + per-component endpoint references
├── docker-compose.yml
├── Cargo.toml            # Rust workspace (manager, worker, admin-api, crates, rust client)
└── config.md             # running log of debugged config gotchas + fixes
```

## Documentation

- **[`docs/architecture.md`](docs/architecture.md)** — the full stack map: the
  two planes, the worker's two binaries/two transports/two modes, the layered
  auth model, the one-question data flow, persistence, and deployment.
- **[`docs/manager-interface.md`](docs/manager-interface.md)** /
  **[`docs/worker-interface.md`](docs/worker-interface.md)** — per-endpoint
  command reference.
- **[`config.md`](config.md)** — non-obvious configuration problems already
  debugged on this project (Zitadel quirks, systemd hardening, JWKS races,
  scope/roles claims) plus the fix that worked. Check it first when something
  looks like a config/deployment problem.
- **[`deploy/manager/README.md`](deploy/manager/README.md)** /
  **[`deploy/zitadel/README.md`](deploy/zitadel/README.md)** — production
  (systemd + nginx) deployment.
