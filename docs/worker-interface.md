# llm-chat backend — command reference

WS server: `ws://127.0.0.1:7878` (auth: `Authorization: Bearer <token>` or `?token=<token>`; token at `~/.local/share/com.llm-chat.app/auth.token`).

## Endpoints

| Path | Type | Description |
|---|---|---|
| `/control` | JSON RPC | Manage sessions and query backend state — one JSON request per line, one JSON reply per line. |
| `/qa/<sid\|index>` | read-only stream | Subscribe to parsed Q&A pairs (`{num,question,answer,...}`) as Claude produces answers in that session. |
| `/s/<sid\|index>` | bidirectional PTY bridge | Send raw bytes to Claude's stdin (e.g. `"hello\r"`); receive raw terminal output bytes back. |

## `/control` commands

| `cmd` | Payload | Returns | Description | Source |
|---|---|---|---|---|
| `list` | — | `{ok, count, sessions[], active}` | List every session ID on this backend and which one is active. | `lib.rs:1675` |
| `info` | — | `{ok, sessions[], active, activeIndex, logPath}` | Same as `list` plus the active index (1-based) and the path to the QA log file. | `lib.rs:1687` |
| `current` | — | `{ok, sessionId, index}` | Just the currently active session and its 1-based index. | `lib.rs:1705` |
| `open` | — | `{ok, sessionId}` | Spawn a new Claude PTY session (runs `claude --dangerously-skip-permissions`). | `lib.rs:1808` |
| `close` | `{sessionId}` | `{ok}` | Tear down the PTY, broadcast channels, QA history, and attachments for that session. | `lib.rs:1860` |
| `switch` | `{sessionId}` | `{ok}` | Make this session the UI-visible/active one. | `lib.rs:1888` |
| `clear` | `{sessionId, what:"stream"\|"terminal"\|"all"}` | `{ok}` | Clear the parsed-Q&A stream, the terminal buffer, or both — for that session. | `lib.rs:1721` |
| `history` | `{sessionId?}` | `{ok, history:[{num,question,answer,...}]}` | Return in-memory Q&A history for one session, or for all sessions if `sessionId` is omitted. | `lib.rs:1768` |
| `log` | — | `{ok, path}` | Path to the rolling control-log file (every command in/out is logged there). | `lib.rs:1908` |
| `screenshot` | — | `{ok, path}` *(Windows only)* | Capture the main app window to PNG via GDI; returns the saved file path. | `lib.rs:1657` |
| `save_attachment` | `{sid, name, mime, data:base64}` | `{ok, path}` | Decode and save an image/PDF attachment under `attachments/<sid>/` so Claude can read it via its Read tool. | `lib.rs:1915` |
| `fifo` | `{sid?, status?, limit?}` | rows from `pty_input` SQLite table | Query the durable audit log of every byte written to a PTY (status: `pending`/`written`/`error`). | `lib.rs:1931` |

Banner sent on `/control` open: `{"ok":true,"hello":"control"}`.

## Quick recipes

```js
const T = fs.readFileSync(`${os.homedir()}/.local/share/com.llm-chat.app/auth.token`,'utf8').trim();
const HDR = { headers: { Authorization: `Bearer ${T}` } };

// /control
const ctl = new WebSocket('ws://127.0.0.1:7878/control', HDR);
ctl.onopen = () => ctl.send(JSON.stringify({ cmd: 'list' }));
ctl.onmessage = e => console.log(JSON.parse(e.data));

// send text + read parsed answers
const qa  = new WebSocket(`ws://127.0.0.1:7878/qa/${sid}`, HDR);
const pty = new WebSocket(`ws://127.0.0.1:7878/s/${sid}`,  HDR);
pty.onopen = () => pty.send('hello\r');
qa.onmessage = e => console.log(JSON.parse(e.data));   // {num, question, answer, ...}
```

There is **no `/chat` endpoint** on this backend — `/chat` exists only on the manager binary.
