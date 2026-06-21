# llm-chat worker — command reference

Per-endpoint reference for the worker's WebSocket interface, verified against
`worker/src/lib.rs` (the worker crate is the source of truth; this doc tracks it).

WS server: `ws://<bind>:<port>` — `<port>` is `LLM_CHAT_WS_PORT` (default `7878`),
`<bind>` is `LLM_CHAT_WS_BIND` (**REQUIRED, no default** — `127.0.0.1` for
loopback, `0.0.0.0` to be reachable from a container; the worker exits at startup
if it's unset).

Auth: every endpoint requires the per-process token, presented as
`Authorization: Bearer <token>` **or** `?token=<token>` query string. Browser-style
`http(s)` `Origin` headers are rejected outright (403). The token is:
- `LLM_CHAT_AUTH_TOKEN` when set (managed mode — the manager passes it to the
  spawned worker), otherwise
- read from / generated into the auth-token file: `%LOCALAPPDATA%\com.llm-chat.app\auth.token`
  on Windows, `$XDG_DATA_HOME/com.llm-chat.app/auth.token` (default
  `~/.local/share/com.llm-chat.app/auth.token`) on Linux/macOS. The standalone
  worker tightens that file's ACL to the current user and also prints the token
  to stderr at startup.

## Binaries

| Binary | Build | Window | Notes |
|---|---|---|---|
| `llm-chat-worker` | default (`gui` feature) — `src/main.rs` | Tauri 2 window | Desktop build; same WS server runs in a background task. `LLM_CHAT_STEALTH=1` hides the window + skips the taskbar. |
| `llm-chat-headless` | `--no-default-features` — `src/bin/headless.rs` | none | Windowless server for CLI-only Linux (no X11/Wayland). Calls `run_headless()`; no WebKitGTK linked. |

## Transports

The worker drives `claude` via one of two transports, chosen by `LLM_CHAT_TRANSPORT`
(applied at session spawn in `do_spawn_session`):

| Transport | Default | How it drives claude | Webview |
|---|---|---|---|
| `stream-json` | **yes** (any value ≠ `pty`) | `claude -p --input-format stream-json --output-format stream-json --verbose --dangerously-skip-permissions` over raw pipes; reads claude's exact answer text from `result` events (`subtype:"success"` / `is_error:false`). No terminal scraping. | No |
| `pty` | `LLM_CHAT_TRANSPORT=pty` | Spawns `claude` in a PTY (ConPTY on Windows, `portable-pty` on Unix). Legacy TUI path; raw console bytes flow over `/s/`. Extra args from `LLM_CHAT_CLAUDE_ARGS` (default `--dangerously-skip-permissions`). | Yes (PTY/TUI needs the JS terminal in `worker/frontend/claude_cli_parser.js`) |

A given session lives in **either** the stream-json session map **or** the PTY
session map, never both.

## Endpoints

| Path | Type | Description |
|---|---|---|
| `/control` | JSON RPC | Manage sessions and query worker state — one JSON request per line, one JSON reply per line. |
| `/qa/<sid\|index>` | read-only stream | Subscribe to parsed Q&A pairs as Claude produces answers in that session. First frame is `{"type":"subscribed","sessionId":…}`; each answer is one JSON line `{num,question,answer,sessionId,isNew,final}`. `<index>` is the 1-based session number. |
| `/s/<sid\|index>` | bidirectional bridge | Send raw bytes to the session's stdin. For **stream-json** sessions: bytes are buffered until a `\r` submit, then sent as one stream-json `user` message (answers come back on `/qa/`, **not** here). For **PTY** sessions: bytes go straight to the PTY and raw terminal output bytes stream back as binary frames. Greeting frame on connect: `connected to session <sid>`. |
| `/` (or empty path) | one-shot | Replies with the JSON array of current session IDs, then closes. |

Every `/s/` write is recorded in the SQLite FIFO (`pty_input`) before the write is
attempted, then marked `written`/`error` after — see the `fifo` command.

## `/control` commands

One JSON object per line with a `cmd` field. Unknown `cmd` → `{"ok":false,"error":"unknown cmd: …"}`.
Source line numbers are into `worker/src/lib.rs`.

| `cmd` | Payload | Returns | Description | Source |
|---|---|---|---|---|
| `list` | — | `{ok, count, sessions[], active}` | All session IDs and the active one. | `lib.rs:2270` |
| `info` | — | `{ok, count, sessions[], active, activeIndex, logPath}` | `list` plus the active 1-based index and the control-log path. | `lib.rs:2281` |
| `current` | — | `{ok, active, activeIndex, sessions[]}` | The active session, its 1-based index, and the session list. | `lib.rs:2298` |
| `open` | `{userId, cwd?}` | `{ok, sessionId, transport}` | Spawn a new Claude session (transport per `LLM_CHAT_TRANSPORT`). `userId` is **mandatory** — the spawn is confined under `{LLM_CHAT_USER_ENV_BASE}/{userId}/{cwd}`; a missing/invalid user id or a path that escapes the base is rejected with no spawn (fail closed). The reply's `transport` lets the manager tune warmup. | `lib.rs:2398` |
| `close` | `{sessionId}` | `{ok, sessionId}` | Tear down the session (stream-json or PTY), its broadcast channels, Q&A history, and attachments. | `lib.rs:2464` |
| `switch` | `{sessionId}` | `{ok, sessionId}` (or `{ok:false,error}` if no such session) | Make this session the UI-visible/active one (emits an event for the GUI). | `lib.rs:2489` |
| `clear` | `{sessionId, what?:"stream"\|"terminal"\|"all"}` | `{ok, sessionId, what}` | Clear the parsed-Q&A history, the terminal buffer, or both (default `all`). `sessionId` may be an ID or a 1-based index. | `lib.rs:2313` |
| `history` | `{sessionId?}` | `{ok, sessionId, history[]}` for one session, or `{ok, histories:{<sid>:[…]}}` for all | In-memory Q&A history (`{num,question,answer}`). `sessionId` may be an ID or 1-based index; omit it for every session. | `lib.rs:2357` |
| `log` | — | `{ok, path}` | Path to the rolling control-log file (every command in/out is logged there). | `lib.rs:2507` |
| `screenshot` | — | `{ok, path, port}` *(Windows GUI only)* | Capture the main app window to PNG via GDI; non-Windows returns `{ok:false,error:"screenshot is windows-only"}`. | `lib.rs:2252` |
| `save_attachment` | `{sid, name, mime, data:base64}` | `{ok, path}` | Decode and save an image/PDF attachment under `attachments/<sid>/<uuid>-<name>` so Claude can read it. Only `image/png\|jpeg\|jpg\|gif\|webp` and `application/pdf` are accepted; other MIME types are rejected. `sid`+`mime`+`data` are required. | `lib.rs:2514` |
| `fifo` | `{sid?, status?, limit?}` | `{ok, rows[], count}` | Query the durable audit log of every byte written to a session (the `pty_input` table). Filter by `sid` and `status` (`pending`/`written`/`error`); `limit` defaults to 100, clamped to 1–1000; rows are newest-first. Each row: `{seq, sid, payload, payloadLen, timeIn, status, timeWritten}`. | `lib.rs:2548` |

Banner sent on `/control` open: `{"ok":true,"hello":"control"}`.

There is no `pty_write` / `pty_resize` / `introspect` control command — those are
Tauri commands internal to the GUI, not part of the WS `/control` protocol. Use
`/s/<sid>` to write to a session and `fifo` to inspect writes.

## `pty_input` SQLite table

One DB file per worker instance: `<auth-token-dir>/backend-<port>.sqlite` (override
with `LLM_CHAT_DB_PATH`), WAL mode. Every `/s/<sid>` write is recorded here in FIFO
order, surviving a restart for audit/replay:

```
pty_input(seq INTEGER PK, sid TEXT, payload BLOB, time_in TEXT,
          status TEXT DEFAULT 'pending', time_written TEXT)
```

`status` transitions `pending` → `written` (write succeeded) or `error` (write
failed). The DB is best-effort: if it can't be opened the worker still runs, just
without the durable queue.

## Environment variables

| Var | Required | Meaning |
|---|---|---|
| `LLM_CHAT_WS_BIND` | **yes** | WS bind host. No default — worker exits if unset. |
| `LLM_CHAT_WS_PORT` | no (default `7878`) | WS port; also names the SQLite file. |
| `LLM_CHAT_USER_ENV_BASE` | **yes** | Root under which every `open` confines `{userId}/{cwd}`. Validated once at startup; worker exits if unset/invalid. |
| `LLM_CHAT_AUTH_TOKEN` | no | When set (and non-empty) = managed mode; this exact token is required by every WS connection. When unset, the worker reads/generates the auth-token file. |
| `LLM_CHAT_TRANSPORT` | no (default `stream-json`) | `pty` selects the legacy PTY/TUI transport; any other value is treated as `stream-json`. |
| `LLM_CHAT_STEALTH` | no | `1` hides the GUI window + skips the taskbar (GUI build only; no effect on `llm-chat-headless`). |
| `LLM_CHAT_CLAUDE_ARGS` | no (default `--dangerously-skip-permissions`) | Extra args appended to `claude` in the **PTY** transport only. |
| `LLM_CHAT_DB_PATH` | no | Override the `pty_input` SQLite file path. |
| `RUST_LOG` | no (default `info`) | Tracing filter, e.g. `RUST_LOG=backend=debug,backend::pty=trace`. `LOG_JSON=1` switches to JSON log output. |

## Quick recipes

```js
const T = process.env.LLM_CHAT_AUTH_TOKEN
  || fs.readFileSync(`${os.homedir()}/.local/share/com.llm-chat.app/auth.token`,'utf8').trim();
const HDR = { headers: { Authorization: `Bearer ${T}` } };

// /control — open a session (userId is mandatory), then list
const ctl = new WebSocket('ws://127.0.0.1:7878/control', HDR);
ctl.onmessage = e => console.log(JSON.parse(e.data));
ctl.onopen = () => {
  ctl.send(JSON.stringify({ cmd: 'open', userId: 'alice', cwd: 'project' }));
  ctl.send(JSON.stringify({ cmd: 'list' }));
};

// send text + read parsed answers (sid from the open reply)
const qa  = new WebSocket(`ws://127.0.0.1:7878/qa/${sid}`, HDR);
const s   = new WebSocket(`ws://127.0.0.1:7878/s/${sid}`,  HDR);
s.onopen  = () => s.send('hello\r');                    // \r submits the message
qa.onmessage = e => console.log(JSON.parse(e.data));    // {num, question, answer, final, ...}
```

There is **no `/chat` endpoint** on the worker — `/chat` exists only on the manager
binary, which connects to the worker via the `/control` and `/s/` + `/qa/`
endpoints above.
