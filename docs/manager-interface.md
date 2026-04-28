# llm-chat-manager — command reference

WS server: `ws://127.0.0.1:7777` (auth: `Authorization: Bearer <token>` or `?token=<token>`; the manager generates the token at startup and persists it to `~/.local/share/com.llm-chat.app/auth.token`, then propagates it to every backend it spawns via the `LLM_CHAT_AUTH_TOKEN` env var).

The manager spawns N llm-chat backends (default 2 on ports 7878-7879), routes session traffic to the owning backend, and persists a FIFO Q→A queue in SQLite or Postgres.

## Endpoints

| Path | Type | Description |
|---|---|---|
| `/control` | JSON RPC | Spawn / close / introspect sessions across every backend; query the manager's own queue and live client registry. One JSON request per line, one JSON reply per line. |
| `/chat` | typed Q→A protocol | Stateless chat channel: send `q`, get back `initialized`, `ack`, then `a` once Claude has answered. Auto-spawns a session per `/chat` connection. Supports image/PDF attachments. |
| `/s/<sid>` | bidirectional PTY bridge | Pass-through to the owning backend's `/s/<sid>`. Send raw bytes to Claude's stdin; receive raw terminal output. |
| `/s/new` | bidirectional PTY bridge | Auto-spawn a fresh session, send `{sid, port}` as the first frame, then bridge as `/s/<sid>`. Auto-closes the session when the WS disconnects. |
| `/qa/<sid>` | read-only stream | Pass-through to the owning backend's `/qa/<sid>` — parsed Q&A pairs (`{num, question, answer, ...}`). |
| `/` | one-shot JSON | List every session ID across every backend. |

## `/control` commands

| `cmd` | Payload | Returns | Description | Source |
|---|---|---|---|---|
| `instances` | — | `{ok, ports[], sessionsPerPort}` | List the backend ports the manager is currently driving and how many sessions each holds. | `main.rs:823` |
| `open` | — | `{ok, sessionId, backendPort}` | Pick the least-loaded backend, spawn a new session there, register `sid → port` so future calls route correctly. | `main.rs:837` |
| `close` | `{sessionId}` | `{ok, sessionId}` | Forward `close` to the owning backend and drop the session from the routing map. | `main.rs:841` |
| `list` / `info` | — | `{ok, count, sessions[], byBackend}` | Aggregate every backend's session list into one reply (plus the raw per-backend response). | `main.rs:852` |
| `history` | `{sessionId?}` | `{ok, history[]}` or `{ok, histories{sid:…}}` | One session's Q&A history (routed to the owning backend) or all sessions across all backends if `sessionId` is omitted. | `main.rs:881` |
| `clients` | — | `{ok, count, clients[]}` | Live in-memory registry of connections currently on `/chat` (connectionId, sid, backendPort, connectedAt, lastQAt, questionsSent). Sorted by connectedAt. | `main.rs:914` |
| `queue` | `{connectionId?, sid?, status?, limit?}` | `{ok, count, rows[]}` | Query the manager's persistent `chat_question` table — every question that came in via `/chat`, with status (`pending`/`sent`/`answered`/`confirmed`/`error`), times, and answer text. Default `limit=100`, max 1000. | `main.rs:923` |
| `fifo` | `{port? \| sessionId?, sid?, status?, limit?}` | rows or `{ok, byBackend}` | Forward to one backend's `pty_input` audit log (target by `port` or `sessionId`); aggregates across all backends if neither is given. | `main.rs:936` |
| `screenshot` | `{port? \| sessionId?}` | `{ok, byPort:{port:{ok,path}}}` | Ask one backend (or every backend) to capture its main window to PNG. *(Windows-only on the backend side.)* | `main.rs:970` |
| `switch` | `{sessionId}` | backend's `{ok}` | Forward to the owning backend — make that session the UI-active one there. | `main.rs:996` |
| `clear` | `{sessionId, what:"stream"\|"terminal"\|"all"}` | backend's `{ok}` | Forward to the owning backend. | `main.rs:996` |
| `current` | — | backend's `{ok, sessionId, index}` | Forward to the first backend (or the one resolved from `sessionId`). | `main.rs:996` |
| `log` | — | backend's `{ok, path}` | Forward to a backend; returns its control-log file path. | `main.rs:996` |

Banner sent on `/control` open: `{"ok":true,"hello":"manager-control"}`.

## `/chat` typed protocol

| Direction | Frame | Description |
|---|---|---|
| C → S | `{"type":"q","id":"<opaque>","text":"…","attachments":[{name,mime,data:base64}]?}` | A new question. `id` is your correlation token; `attachments` are optional image/PDF files which the manager saves to disk and rewrites into "Read the file at …" instructions before sending text to Claude. |
| S → C | `{"type":"initialized","sid":…,"backendPort":…,"connectionId":…,"timeOut":…}` | First frame after handshake. The manager spawned a session for you; subsequent frames refer to it. |
| S → C | `{"type":"ack","id":…,"seq":N,"timeIn":…}` | Sent **immediately after the DB insert, before** writing to the PTY. `seq` is the server-assigned FIFO index — also the row id in `chat_question`. |
| S → C | `{"type":"a","id":…,"seq":N,"text":"…","timeIn":…,"timeOut":…}` | Sent **after a 3 s parser settle** once Claude's answer is detected on `/qa`. `text` is the parsed answer body. |
| C → S | `{"type":"confirm","seq":N}` | Marks the row as `confirmed` in `chat_question` (audit closure). |
| S → C | `{"type":"err","text":…,"timeOut":…}` | Any error along the way — backend down, DB insert failed, PTY closed, attachment save failed, etc. |

Auth required on the WS upgrade. One `/chat` connection = one auto-spawned session for its lifetime.

## Persistence

- **Manager queue:** `chat_question` table — every `/chat` question with its status, attachment paths, and final answer. SQLite by default at `~/.local/share/com.llm-chat.app/manager.sqlite`; override with `MANAGER_DB_PATH` (SQLite file) or `MANAGER_DB_URL=postgres://…` for Postgres.
- **Backend FIFO** (per backend, separate file): `pty_input` table in `backend-<port>.sqlite` — every byte written to a PTY, with status `pending`/`written`/`error`. Queried via the manager's `fifo` cmd.

## Quick recipes

```js
const T = fs.readFileSync(`${os.homedir()}/.local/share/com.llm-chat.app/auth.token`,'utf8').trim();
const HDR = { headers: { Authorization: `Bearer ${T}` } };

// /control: spawn a session and list backends
const ctl = new WebSocket('ws://127.0.0.1:7777/control', HDR);
ctl.onopen = () => ctl.send(JSON.stringify({ cmd: 'instances' }));
ctl.onmessage = e => console.log(JSON.parse(e.data));

// /chat: ask "hello", read the answer
const chat = new WebSocket('ws://127.0.0.1:7777/chat', HDR);
chat.onopen = () => chat.send(JSON.stringify({ type: 'q', id: 'h1', text: 'hello' }));
chat.onmessage = e => {
  const f = JSON.parse(e.data);
  console.log(f);                                // initialized → ack → a
  if (f.type === 'a') chat.send(JSON.stringify({ type: 'confirm', seq: f.seq }));
};
```
