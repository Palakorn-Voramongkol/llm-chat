# llm-chat-manager — command reference

WS server: `ws://<MANAGER_BIND>:<MANAGER_PORT>` (default port `7777`).

The manager either spawns N llm-chat-worker backends (default 2, on ports
7878-7879) or attaches to pre-started workers (external-backend mode), routes
per-session traffic to the backend that owns each session, and persists a
durable FIFO Q→A queue in SQLite (default) or Postgres.

## Authentication & authorization

The manager has two inbound auth modes, chosen at startup by whether
`ZITADEL_ISSUER` is set:

- **Zitadel JWT (primary).** When `ZITADEL_ISSUER`/`ZITADEL_AUDIENCE`/
  `ZITADEL_PROJECT_ID` are all set, every WS upgrade must carry a Zitadel
  access token (`Authorization: Bearer <jwt>` or `?token=<jwt>`). The token is
  verified against Zitadel's JWKS (RS256, issuer + audience checked) using a
  cache preloaded at startup and refreshed hourly. Project roles are read from
  the `urn:zitadel:iam:org:project:<project_id>:roles` claim.
- **Shared-token fallback.** Only if `ZITADEL_ISSUER` is unset, the manager
  falls back to a single per-process shared token (same `Bearer`/`?token=`
  carriers). The token is `LLM_CHAT_AUTH_TOKEN` if set, otherwise randomly
  generated at startup, and is persisted to disk
  (`%LOCALAPPDATA%\com.llm-chat.app\auth.token` on Windows,
  `$XDG_DATA_HOME/com.llm-chat.app/auth.token` on unix) with an ACL/`chmod 0600`
  restricting it to the current user.

The shared token is **also** used internally for the manager↔backend hop
(loopback) in both modes — the manager propagates it to every backend it spawns
via the `LLM_CHAT_AUTH_TOKEN` env var.

Per-endpoint authorization (enforced after the JWT is verified — only meaningful
in JWT mode; shared-token mode has no role concept):

| Endpoint | Required |
|---|---|
| `/chat`, `/s/new`, `/` | role `chat.user` |
| `/control` | role `chat.admin` (it can list/close/inspect *any* user's session) |
| `/s/<sid>`, `/qa/<sid>` | the session's authenticated **owner**, or `chat.admin`. Fails closed: unknown session or one with no recorded owner is rejected |

Any WS that lacks a captured user id is rejected with a typed
`{"type":"err","text":…}` frame and closed. Cross-origin upgrades (an `Origin`
header starting `http://`/`https://`) are rejected with 403.

## Endpoints

| Path | Type | Description |
|---|---|---|
| `/control` | JSON RPC | Spawn / close / introspect sessions across every backend; query the manager's own queue and live client registry. One JSON request per line, one JSON reply per line. Requires `chat.admin`. |
| `/chat` | typed Q→A protocol | Stateless chat channel: send `q`, get back `initialized`, `ack`, then `a` once Claude has answered. Auto-spawns one session per `/chat` connection (closed on disconnect). Accepts optional `?cwd=<urlencoded-path>` to run Claude in a specific directory. Supports image/PDF attachments. |
| `/s/<sid>` | bidirectional PTY bridge | Pass-through to the owning backend's `/s/<sid>`. Send raw bytes to Claude's stdin; receive raw terminal output. Owner-or-admin only. |
| `/s/new` | bidirectional PTY bridge | Auto-spawn a fresh session, send `{sid, backendPort}` as the first text frame, then bridge as `/s/<sid>`. Auto-closes the session when the WS disconnects. |
| `/qa/<sid>` | read-only stream | Pass-through to the owning backend's `/qa/<sid>` — parsed Q&A pairs (`{num, question, answer, final, …}`). Owner-or-admin only. |
| `/` | one-shot JSON | List every session ID across every backend, as a JSON array. |

## `/control` commands

One JSON request per WS message, one JSON reply per request. Requires the
`chat.admin` role.

| `cmd` | Payload | Returns | Description |
|---|---|---|---|
| `instances` | — | `{ok, ports[], sessionsPerPort}` | List the backend ports the manager is driving and how many sessions each holds. |
| `open` | — | `{ok, sessionId, backendPort, transport}` | Pick the least-loaded backend, spawn a new session there under the caller's user id, register `sid → port` and `sid → owner`. `transport` is the backend's transport (`stream-json` or `pty`). |
| `close` | `{sessionId}` | `{ok, sessionId}` | Forward `close` to the owning backend and drop the session from the routing + owner maps. |
| `list` / `info` | — | `{ok, count, sessions[], byBackend}` | Aggregate every backend's session list into one reply (plus the raw per-backend response under `byBackend`). |
| `history` | `{sessionId?}` | `{ok, history[]}` or `{ok, histories{sid:…}}` | One session's Q&A history (routed to the owning backend) or all sessions across all backends if `sessionId` is omitted. |
| `clients` | — | `{ok, count, clients[]}` | Live in-memory registry of connections currently on `/chat`: `{connectionId, sid, userId, backendPort, connectedAt, lastQAt, questionsSent}`. Sorted by `connectedAt`. |
| `queue` | `{connectionId?, sid?, status?, limit?}` | `{ok, count, rows[]}` | Query the manager's persistent `chat_question` table — every `/chat` question with status (`pending`/`sent`/`answered`/`confirmed`/`error`), times, answer text, and attachment paths. Most-recent first. Default `limit=100`, clamped to 1–1000. |
| `fifo` | `{port? \| sessionId?, sid?, status?, limit?}` | rows or `{ok, byBackend}` | Forward to one backend's `pty_input` audit log (target by `port` or `sessionId`); aggregates across all backends if neither is given. `sid`/`status`/`limit` are forwarded verbatim. |
| `screenshot` | `{port? \| sessionId?}` | `{ok, byPort:{port:…}}` | Ask one backend (or every backend) to capture its main window to PNG. *(Windows-only on the backend side.)* |
| `switch` | `{sessionId}` | backend's reply | Forward to the owning backend — make that session the UI-active one there. |
| `clear` | `{sessionId, what:"stream"\|"terminal"\|"all"}` | backend's reply | Forward to the owning backend. |
| `current` | `{sessionId?}` | backend's `{ok, sessionId, index}` | Forward to the session's backend, or the first backend when no `sessionId`. |
| `log` | `{sessionId?}` | backend's `{ok, path}` | Forward to the session's backend (or the first backend); returns its control-log file path. |

`switch`/`clear`/`current`/`log` are forwarded verbatim to the resolved backend;
the manager just routes them. An unknown `cmd` returns
`{ok:false, error:"unknown cmd: …"}`.

Banner sent on `/control` open: `{"ok":true,"hello":"manager-control"}`.

## `/chat` typed protocol

One auto-spawned session per `/chat` connection, for its lifetime. All
timestamps are RFC 3339 / ISO 8601 in UTC with millisecond precision.

| Direction | Frame | Description |
|---|---|---|
| S → C | `{"type":"initialized","sid":…,"backendPort":…,"connectionId":…,"timeOut":…}` | First frame after the handshake. The manager has spawned a session for you; subsequent frames refer to it. |
| C → S | `{"type":"q","id":"<opaque>","text":"…","attachments":[{name,mime,data:base64}]?}` | A new question. `id` is your correlation token; `attachments` are optional image/PDF files which the manager forwards to the backend (`save_attachment`), then prepends `Read the file at <path>.` instructions to the question text before sending it to Claude. |
| S → C | `{"type":"ack","id":…,"seq":N,"timeIn":…}` | Sent **immediately after the DB insert, before** the question is written to the backend. `seq` is the server-assigned FIFO index — also the row PK in `chat_question`. `id` is your opaque value echoed back. |
| S → C | `{"type":"a","id":…,"seq":N,"text":"…","timeIn":…,"timeOut":…,"latencyMs":N}` | The answer, paired FIFO to the oldest outstanding `q` on this connection. Delivered **immediately** when the backend marks it final (stream-json emits one complete `result` per question, `final:true`); the legacy PTY path debounces partial repaints by `MANAGER_CHAT_SETTLE_MS` (default 3000 ms) first. `latencyMs` = q-received → a-forwarded. |
| C → S | `{"type":"confirm","seq":N}` | Marks the row `confirmed` (with `time_confirmed`) in `chat_question` — only if it is currently `answered` (stale/replayed confirms are ignored). Audit closure. |
| S → C | `{"type":"err","id?":…,"text":…,"timeIn?":…,"timeOut":…}` | Any error along the way — open failed, bad JSON, empty question, unknown frame type, backend `/s/` or `/qa/` connect failed, attachment save failed, DB insert failed, backend PTY closed, etc. |

Notes:
- `seq` (the server-assigned, monotonically increasing receipt id = the
  `chat_question` PK) appears in both `ack` and `a`, so a client can correlate
  by its own opaque `id` or by `seq`.
- A newly-spawned session may be held back by a cold-start grace period before
  the first `q` is processed: `MANAGER_CHAT_WARMUP_SECS` (default 8 s for the
  PTY transport, 0 for stream-json).
- On client disconnect the manager auto-closes the session.

## Persistence

- **Manager queue:** `chat_question` table — one row per `/chat` question.
  Columns: `seq` (FIFO PK), `connection_id`, `sid`, `q_id`, `text`, `time_in`,
  `status`, `answer_text`, `time_out`, `time_confirmed`, `attachment_paths`
  (JSON array of saved file paths). Status machine:
  `pending → sent → answered → confirmed` (happy path), `… → error` (PTY died,
  parser timeout, etc.). A row stuck at `answered` means the answer was sent but
  the client never confirmed receipt. SQLite by default at
  `…/com.llm-chat.app/manager.sqlite` (override with `MANAGER_DB_PATH`);
  set `MANAGER_DB_URL=postgres://…` for Postgres (or a sqlite path/URL).
  `queue` rows are returned with camelCase keys
  (`seq, connectionId, sid, qId, text, timeIn, status, answerText, timeOut, attachmentPaths`).
- **Backend FIFO** (per backend, separate file): `pty_input` table in the
  worker's own DB — every byte written to a PTY, with status. Queried via the
  manager's `fifo` cmd.

## Configuration (env vars)

| Var | Required | Default | Purpose |
|---|---|---|---|
| `MANAGER_PORT` | no | `7777` | Manager WS listen port. |
| `MANAGER_BIND` | **yes** | — (fail-fast) | Listen address (e.g. `127.0.0.1`, `0.0.0.0`). No code default. |
| `MANAGER_BACKEND_HOST` | **yes** | — (fail-fast) | Host the manager dials backends at (and binds spawned workers to). No code default. |
| `MANAGER_INSTANCES` | no | `2` | Number of workers to spawn (spawning mode only). |
| `MANAGER_START_PORT` | no | `7878` | First backend port; consecutive ports follow. |
| `LLM_CHAT_EXE` | no | sibling `worker/target/{release\|debug}/llm-chat-worker[.exe]` | Path to the worker executable (spawning mode only). |
| `MANAGER_BACKEND_PORTS` | no | — | If set (comma-separated port list), switches to **external-backend mode**: the manager attaches to pre-started workers on these ports instead of spawning any. Presence is the mode toggle. |
| `MANAGER_STEALTH` | no | off | Spawn workers with `LLM_CHAT_STEALTH=1` (`1`/`true`). |
| `LLM_CHAT_AUTH_TOKEN` | no | random 32-byte hex | Shared manager↔backend token (and inbound token in shared-token mode); persisted to the auth-token file. |
| `MANAGER_DB_URL` | no | — | `postgres://…` → Postgres; any other value → a sqlite path/URL. Unset → default sqlite file. |
| `MANAGER_DB_PATH` | no | `…/com.llm-chat.app/manager.sqlite` | Override the default sqlite file location. |
| `MANAGER_CHAT_WARMUP_SECS` | no | 8 (pty) / 0 (stream-json) | Cold-start grace before the first `q` on a `/chat` session. |
| `MANAGER_CHAT_SETTLE_MS` | no | `3000` | PTY-only debounce window for finalizing an answer (unused on stream-json). |
| `ZITADEL_ISSUER` | for JWT auth | — | Zitadel issuer URL; presence enables JWT auth. |
| `ZITADEL_AUDIENCE` | for JWT auth | — | Comma-separated audience (the project id). |
| `ZITADEL_PROJECT_ID` | for JWT auth | — | Project id used to locate the roles claim. |
| `RUST_LOG` / `LOG_JSON` | no | `info` / off | Tracing level filter; `LOG_JSON=1` switches to JSON log output. |

On startup `MANAGER_BIND` and `MANAGER_BACKEND_HOST` are resolved and validated
**before** any side effect (spawning workers, opening the DB); a missing one
aborts cleanly.

## Quick recipes

```js
const T = fs.readFileSync(`${os.homedir()}/.local/share/com.llm-chat.app/auth.token`,'utf8').trim();
// In JWT mode, T is instead a Zitadel access token obtained from your OIDC flow.
const HDR = { headers: { Authorization: `Bearer ${T}` } };

// /control: spawn a session and list backends (requires chat.admin)
const ctl = new WebSocket('ws://127.0.0.1:7777/control', HDR);
ctl.onopen = () => ctl.send(JSON.stringify({ cmd: 'instances' }));
ctl.onmessage = e => console.log(JSON.parse(e.data));

// /chat: ask "hello", read the answer (requires chat.user)
const chat = new WebSocket('ws://127.0.0.1:7777/chat', HDR);
chat.onopen = () => chat.send(JSON.stringify({ type: 'q', id: 'h1', text: 'hello' }));
chat.onmessage = e => {
  const f = JSON.parse(e.data);
  console.log(f);                                // initialized → ack → a
  if (f.type === 'a') chat.send(JSON.stringify({ type: 'confirm', seq: f.seq }));
};
```
