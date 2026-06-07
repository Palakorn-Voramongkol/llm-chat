# Worker deployment (headless, CLI-only Linux)

The **worker** is the component that actually drives the `claude` CLI and exposes
the `/s/` + `/qa/` + `/control` WebSocket endpoints the manager talks to. It is a
Tauri app, but it does **not** need its GUI window to do its server job.

There are two binaries (same crate):

| Binary | Window | Use |
|---|---|---|
| `llm-chat` | opens a Tauri webview window | desktop / standalone |
| **`llm-chat-headless`** | **no window** | servers, CLI-only Linux |

## Does it run on a CLI-only Linux server?

**Yes** — use `llm-chat-headless`. It runs the WebSocket relay + stream-json
Claude sessions without ever creating a window, so **no X11/Wayland display
server is required**.

One caveat: the binary still *links* `libwebkit2gtk` (a Tauri dependency), so
that shared library must be **installed** — but it is never used to open a
window, so no display is needed:

```bash
sudo apt-get install -y libwebkit2gtk-4.1-0   # the lib, NOT a desktop
```

(Removing the WebKitGTK link entirely — for a smaller image with no GTK at all —
is a planned follow-up via Cargo feature-gating.)

## Prerequisites on the Linux box

- **Node.js + the `claude` CLI** on `PATH`, logged in (the worker shells out to
  `claude`). `~/.claude` holds the auth/session state — the worker uses the real
  CLI, there is no API key.
- `libwebkit2gtk-4.1-0` (see above).
- A Rust toolchain to build (or copy a prebuilt binary).

## Build

```bash
cd worker
cargo build --release --bin llm-chat-headless
# -> worker/target/release/llm-chat-headless
```

## Run

```bash
LLM_CHAT_WS_BIND=0.0.0.0 \
LLM_CHAT_WS_PORT=7878 \
LLM_CHAT_AUTH_TOKEN=<shared-token-the-manager-also-has> \
./target/release/llm-chat-headless
```

| Env var | Meaning |
|---|---|
| `LLM_CHAT_WS_BIND` | listen address (required; no default) |
| `LLM_CHAT_WS_PORT` | listen port (default 7878) |
| `LLM_CHAT_AUTH_TOKEN` | shared token for the manager↔worker WS auth |
| `LLM_CHAT_TRANSPORT` | `stream-json` (default) or `pty` (legacy TUI scrape) |
| `RUST_LOG` | e.g. `info,backend::qa=debug` |

The default **stream-json** transport reads claude's real structured output —
the worker needs no terminal/TUI and produces clean answers. (`pty` mode exists
for the legacy desktop TUI and is the only mode that needs the webview.)

## Connecting to the manager

The manager reaches the worker over `ws://<worker-host>:7878`. In the compose
dev stack the manager runs in a container and dials `host.docker.internal:7878`
(the worker on the host). For a Linux server deployment, point the manager's
`MANAGER_BACKEND_HOST` at the worker's address and set `MANAGER_BACKEND_PORTS`
to its port (external-backend mode), sharing the same `LLM_CHAT_AUTH_TOKEN`.

## systemd unit (example)

```ini
[Unit]
Description=llm-chat worker (headless)
After=network.target

[Service]
ExecStart=/opt/llm-chat/llm-chat-headless
Environment=LLM_CHAT_WS_BIND=0.0.0.0
Environment=LLM_CHAT_WS_PORT=7878
Environment=LLM_CHAT_AUTH_TOKEN=change-me
User=llm
Restart=on-failure

[Install]
WantedBy=multi-user.target
```

## Why not a Docker image here?

The worker drives the **real** `claude` CLI and relies on `~/.claude` auth
(no API key), so it's normally run natively on the host where claude is logged
in. Containerizing it means baking in node + claude + the logged-in session,
which is environment-specific; running `llm-chat-headless` under systemd is the
straightforward path. The manager/postgres/zitadel are what the compose stack
containerizes (see `deploy/compose/`).
