# llm-chat — stack architecture

## Overview

**llm-chat turns the interactive `claude` CLI into a programmable,
multi-tenant chat service.** There is no Anthropic API call anywhere in this
system. Instead, the real `claude` Code TUI runs inside a pseudo-terminal
(ConPTY on Windows, a PTY on unix); the system *types* a question into that
terminal as if a human were at the keyboard, watches the terminal output
scroll by, parses the assistant's answer back out, and returns it as
structured JSON. Everything a normal `claude` session can do — tools, file
reads, its full reasoning — happens for free, because it *is* a normal
`claude` session being driven by a robot.

Three tiers stack up to make that usable by more than one caller:

- **Worker** — owns the bottom of the stack: one Tauri app that can spawn
  several `claude` PTY sessions, relay their raw terminal I/O over WebSocket,
  and (in its webview) parse each session's scrolling output into clean
  question/answer pairs. Run on its own it's just a desktop terminal window
  with Claude in it.
- **Manager** — the multiplexer and front door. It spawns *N* workers, hands
  each incoming caller a fresh session on the least-loaded one, and hides all
  the raw-PTY plumbing behind a single typed `/chat` protocol. It also
  authenticates callers, and persists every question in a durable FIFO queue
  so an answer is never silently lost.
- **Client** — any consumer of `/chat`. The reference one is a small Python
  script; the `tests/` directory has JS equivalents.

**The lifecycle of one question, in a sentence:** a client opens an
authenticated WebSocket to the manager's `/chat`, the manager spins up a
private `claude` session on a worker, types the question into that session's
PTY, waits for the worker's parser to surface the finished answer, pairs it
back to the question in FIFO order, and streams it to the client as an `a`
frame — recording every step in SQLite along the way.

Three properties are worth holding onto while reading the rest of this page:

1. **WebSocket-only.** There is no REST surface; every endpoint on every
   component is a WebSocket. nginx exists purely to terminate TLS and proxy
   the upgrade.
2. **Two-layer auth.** Outside callers are gated by Zitadel-issued JWTs;
   the internal manager↔worker hops use a shared loopback token. (See
   [Auth model](#auth-model--two-layers).)
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
| **Worker** (`worker/`) | Rust + JS (Tauri 2) | `ws://127.0.0.1:7878` (loopback) | Spawns `claude` in a PTY, relays raw I/O, parses Q&A. Dual-mode: visible desktop app **or** hidden managed backend. |
| **Manager** (`manager/`) | Rust | `ws://127.0.0.1:7777` (loopback) | Spawns N workers, verifies client auth, routes `sid → backend`, persists a FIFO Q→A queue, exposes the typed `/chat` protocol. |
| **Client** (`clients/python/`) | Python | — | Reference client. Authenticates to Zitadel as a machine user, connects to the manager's `/chat`, does Q→A. (JS equivalents live in `tests/`.) |
| **Zitadel** (`deploy/zitadel/`) | — | `https://id.palakorn.com` | OIDC IdP. Issues machine-user JWTs and publishes the JWKS the manager verifies against. |
| **nginx** (`deploy/manager/`) | — | `https://api.palakorn.com:443` | TLS terminator. Proxies every WS path to the manager on `127.0.0.1:7777`. |

```
┌─────────────┐   wss + Zitadel JWT    ┌──────────────────────────────┐
│ Client      │ ─────────────────────► │ nginx (api.palakorn.com:443) │
│ (python ref │   /chat                │  TLS term → 127.0.0.1:7777   │
│  or JS test)│                        └───────────────┬──────────────┘
└─────────────┘                                        │ ws (loopback)
                                                       ▼
                                        ┌───────────────────────────────┐
                                        │ MANAGER  (:7777)              │
                                        │ • verifies JWT vs Zitadel JWKS│
                                        │   (role chat.user) OR shared  │
                                        │   token fallback              │
                                        │ • spawns N workers            │
                                        │ • routes sid → backend port   │
                                        │ • FIFO queue chat_question    │
                                        │   (manager.sqlite / Postgres) │
                                        └───────┬───────────────┬───────┘
                          ws + shared token     │               │
                          (loopback only)       ▼               ▼
                              ┌───────────────────────┐  ┌──────────────────────┐
                              │ WORKER backend :7878  │  │ WORKER backend :7879 │
                              │ Tauri app (window     │  │ … (MANAGER_INSTANCES)│
                              │ hidden in stealth)    │  └──────────────────────┘
                              │ /control /s/ /qa/     │
                              │ ConPTY → claude.exe   │
                              │ xterm + JS parser ────┼─► broadcast_qa → /qa/<sid>
                              └───────────────────────┘
```

---

## The worker is one binary with two modes

`worker/` builds a single Tauri app. **Environment variables decide how it
behaves** (`worker/src/lib.rs:run()`):

| Env var | Set by | Effect |
|---|---|---|
| `LLM_CHAT_WS_PORT` | manager (`spawn_instance`) | Which port the backend's WS server binds. Default `7878`. |
| `LLM_CHAT_AUTH_TOKEN` | manager | The shared loopback token. **Its presence = "managed" mode**; absent = standalone, token read/generated from the `auth.token` file. |
| `LLM_CHAT_STEALTH=1` | manager (when `MANAGER_STEALTH`) | Hide the window + skip the taskbar at startup. The WS server and webview keep running. |

- **Standalone (`npm run dev`):** a desktop terminal window you watch —
  xterm.js hosting `claude` live. This is what you get running the worker
  directly.
- **Managed (manager-spawned):** the same process with the window hidden,
  driven entirely over WebSocket by the manager.

> **The webview always runs — even hidden.** Q&A parsing happens in the
> *frontend JS* (`frontend/claude_cli_parser.js`), which reads the xterm
> buffer and calls the `broadcast_qa` Tauri command to fan out parsed answers
> on `/qa/<sid>`. So a "headless" backend still needs a webview/display. On
> Linux servers the manager transparently wraps the worker in `xvfb-run -a`
> when no `DISPLAY`/`WAYLAND_DISPLAY` is set (`manager/src/main.rs:spawn_instance`).

---

## Auth model — two layers

### External: client → manager

Decided at manager startup (`manager/src/main.rs`, `auth_zitadel.rs`):

- **Zitadel JWT** when `ZITADEL_ISSUER` is set. On every WS upgrade the manager
  extracts the `Bearer` token, verifies it against the cached **JWKS**
  (`/oauth/v2/keys`, refreshed hourly), checks `iss` + `aud`, and **requires
  the project role `chat.user`** (encoded under
  `urn:zitadel:iam:org:project:<project_id>:roles`). Missing/invalid → 401;
  wrong role → 403.
- **Shared-token fallback** when `ZITADEL_ISSUER` is *unset*. The manager
  compares the presented token against its own per-process token
  (constant-time). This is the **fully-offline local path** — no Zitadel
  needed.

The reference **Python client only implements the Zitadel JWT-bearer flow** —
it signs an assertion with a machine-user key and exchanges it for an access
token. It has no shared-token mode, so running it requires real Zitadel
credentials (issuer + project + `kabytech-key.json`).

### Internal: manager ↔ backend (and backend standalone)

A **random 32-byte per-process token**, generated by the manager at startup,
written to `auth.token`, and propagated to every spawned worker via
`LLM_CHAT_AUTH_TOKEN`. Used on the loopback hops only (`/s/`, `/qa/`,
`/control`). Browser-style `Origin` headers are rejected outright on both
manager and backend.

**Token file location** (same dir on both, so DBs sit beside it):
- Windows: `%LOCALAPPDATA%\com.llm-chat.app\auth.token`
- Linux/macOS: `$XDG_DATA_HOME/com.llm-chat.app/auth.token` (default `~/.local/share/...`)

ACL is tightened to the current user (`icacls` on Windows, `chmod 0600` on unix).

---

## Data flow — one `/chat` round-trip

`handle_chat` in `manager/src/main.rs` orchestrates this:

1. **Connect.** Client opens a WS to the manager `/chat` with its `Bearer` JWT.
   Manager verifies it (JWKS + `chat.user`).
2. **Open a session.** Manager picks the least-loaded backend, sends
   `{cmd:open}` over that backend's `/control`; the backend spawns a fresh
   `claude` PTY and returns a `sessionId`. Manager records `sid → port`.
3. **Initialized.** Manager → client: `{type:initialized, sid, backendPort, connectionId}`.
4. **Bridge.** Manager opens two loopback WS to the owning backend:
   `/s/<sid>` (PTY input) and `/qa/<sid>` (parsed answers).
5. **Warm up.** Sleeps `MANAGER_CHAT_WARMUP_SECS`. The backend reports its
   transport in the `{cmd:open}` reply, so the manager picks the default: **0
   for stream-json** (claude reads stdin from the start — no warmup needed) and
   **8 s for the legacy PTY transport** (where `claude` needs ~5–8 s to reach
   its prompt *and* the JS parser must hook `onData` before traffic starts). An
   explicit env value overrides both.
6. **Ask.** Client → `{type:q, id, text, attachments?}`.
   - Attachments are first saved via the backend's `save_attachment`; their
     on-disk paths are rewritten into `Read the file at <path>.` instructions
     prepended to the text.
   - Manager `INSERT`s a `pending` row (the autoincrement `seq` is both the
     FIFO key and the receipt id) → emits `{type:ack, seq}` **immediately,
     before touching the PTY**.
   - Manager writes the text to `/s/<sid>`, waits 150 ms, then writes `\r`
     (splitting body and Enter avoids Claude's TUI dropping multi-line
     submits) → marks the row `sent`.
7. **Answer.** The backend feeds the PTY → `claude` answers → xterm renders →
   `claude_cli_parser.js` parses it → `broadcast_qa` fans out
   `{num, answer}` on `/qa/<sid>`.
8. **Pair & deliver.** Each `/qa` event carries `final`. stream-json sends one
   complete `result` per question (`final:true`) → the manager delivers it
   **immediately**; the legacy PTY path streams partial repaints → the manager
   debounces `MANAGER_CHAT_SETTLE_MS` (default 3 s) to capture the *final* text.
   Either way it pops the **oldest `sent` row** for this connection (FIFO),
   marks it `answered`, and sends
   `{type:a, id, seq, text, timeIn, timeOut, latencyMs}` to the client
   (`latencyMs` = question-received → answer-forwarded).
9. **Confirm.** Client → `{type:confirm, seq}` → manager marks the row
   `confirmed` (audit closure).
10. **Teardown.** On client disconnect the manager closes the session
    (`{cmd:close}` → backend tears down the PTY) and drops it from the
    routing map.

Row status machine: `pending → sent → answered → confirmed` (happy path);
`any → error` on failure.

---

## Persistence

| Store | Owner | Location | Holds |
|---|---|---|---|
| `chat_question` | manager | `manager.sqlite` (default) or Postgres via `MANAGER_DB_URL` | Every `/chat` question: text, status, attachment paths, answer, timestamps. The durable FIFO. |
| `pty_input` | each backend | `backend-<port>.sqlite` | Every byte written to that PTY, status `pending`/`written`/`error`. Audited via the manager's `fifo` cmd. |
| `auth.token` | manager (or standalone backend) | `…/com.llm-chat.app/auth.token` | The shared loopback token. |
| QA logs | backend frontend | `…/com.llm-chat.app/` (rolling `qa_*.log`) | Human-readable Q&A transcript per session. |

Default SQLite files all live in `%LOCALAPPDATA%\com.llm-chat.app\` (Windows) /
`~/.local/share/com.llm-chat.app/` (unix).

---

## Endpoints at a glance

| Path | Manager | Backend | Type |
|---|:---:|:---:|---|
| `/chat` | ✓ | — | Typed Q→A protocol (auto-spawns a session per connection). |
| `/control` | ✓ | ✓ | JSON RPC: spawn/close/introspect sessions, query queue/clients. |
| `/s/<sid>` | ✓ | ✓ | Bidirectional raw PTY bridge. |
| `/s/new` | ✓ | — | Auto-spawn + bridge + auto-close. |
| `/qa/<sid>` | ✓ | ✓ | Read-only parsed Q&A stream. |
| `/` | ✓ | ✓ | One-shot JSON list of session ids. |

Full payloads: [`manager-interface.md`](manager-interface.md),
[`worker-interface.md`](worker-interface.md).

---

## Ports & config

| Port | Bind | What |
|---|---|---|
| `7777` | loopback | Manager (`MANAGER_PORT`). nginx fronts it on prod at `:443`. |
| `7878`, `7879`, … | loopback | Worker backends (`MANAGER_START_PORT` + i, one per `MANAGER_INSTANCES`). |
| `443` | public | nginx `api.palakorn.com` → manager. |
| `443` | public | Zitadel `id.palakorn.com`. |

**Manager env** (`deploy/manager/manager.env.example`): `MANAGER_PORT`,
`MANAGER_INSTANCES`, `MANAGER_START_PORT`, `MANAGER_STEALTH`, `LLM_CHAT_EXE`,
`MANAGER_CHAT_WARMUP_SECS` (default: 0 for stream-json, 8 for PTY),
`MANAGER_CHAT_SETTLE_MS` (PTY answer debounce, default 3000),
`ZITADEL_ISSUER` / `ZITADEL_AUDIENCE` /
`ZITADEL_PROJECT_ID`, `MANAGER_DB_URL` (optional Postgres).

**Client env / flags** (`clients/python/llm_chat_client.py`): `--issuer` /
`ZITADEL_ISSUER`, `--project` / `PROJECT_ID`, `--key-file` / `KABYTECH_KEY`,
`--manager` / `MANAGER_WS` (default `ws://127.0.0.1:7777/chat`).

---

## Production deployment

GitHub Actions (`.github/workflows/`) on push to `main`:

1. `cargo build --release` for `manager/` and `worker/` on the runner.
2. `scp` binaries to the server's deploy staging dir.
3. SSH (forced-command, allowlisted) `install-manager-binary` /
   `install-worker-binary`, then `restart-manager`.
4. Health probe Zitadel's OIDC discovery doc.
5. End-to-end probe: the Python client authenticates as the `kabytech`
   machine user and sends `hello` to `wss://api.palakorn.com/chat`, expecting
   an `a` frame.

The manager runs as the `llm-chat` systemd service; it spawns the worker
binaries itself (they are **not** separate services — `MANAGER_INSTANCES`
controls how many). See [`deploy/manager/README.md`](../deploy/manager/README.md)
and [`deploy/zitadel/README.md`](../deploy/zitadel/README.md).

---

## Running locally

### Worker alone (desktop terminal)

```bash
cd worker
npm install
npm run dev
```

A window opens with `claude` live in a PTY. No manager, no auth — you're the user.

### Full local stack (offline, no Zitadel)

Start the manager **without** `ZITADEL_ISSUER` so it uses shared-token auth; it
spawns the worker backend(s) itself.

```bash
# from manager/  (debug binary already at manager/target/debug/)
MANAGER_INSTANCES=1 \
LLM_CHAT_EXE=../worker/target/debug/llm-chat.exe \
  ./target/debug/llm-chat-manager
```

Then drive `/chat` or `/control` with a local client that reads the shared
token from `auth.token` and passes it as `?token=` (see the JS clients in
`tests/`, e.g. `tests/manager.js`). The reference Python client does **not**
work on this path — it requires Zitadel credentials.

### Reference Python client (needs Zitadel)

```bash
pip install PyJWT requests websockets
python clients/python/llm_chat_client.py \
  --issuer   https://id.palakorn.com \
  --project  370627061150121985 \
  --key-file /path/to/kabytech-key.json \
  --manager  wss://api.palakorn.com/chat \
  --send     "hello"
```
