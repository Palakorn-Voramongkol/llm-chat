# LLM Chat

Minimal Tauri 2 desktop app that opens a single terminal window and runs the **Claude CLI** inside a Windows ConPTY session — the same behavior as clicking the *Claude* button in the parent [`onscreen-kbd`](../onscreen-kbd) project, with everything else stripped out.

> **Platform:** Windows only (uses ConPTY + Win32).

## What it does

On launch:
1. Looks for `claude` on `PATH`, in `%APPDATA%\npm`, then `%LOCALAPPDATA%\AnthropicClaude`.
2. Opens a single window hosting xterm.js.
3. Spawns a ConPTY child running `claude` and pipes stdin/stdout between the PTY and xterm.

That's it. No on-screen keyboard, no auth, no web server, no NATS — just a terminal pre-wired to Claude.

## Layout

```
.
├── manager/                  # Rust manager (auth, queue, routing) — listens 7777
│   ├── Cargo.toml
│   └── src/
│       ├── main.rs
│       └── auth_zitadel.rs   # Zitadel JWT verifier (JWKS-cached, sync verify)
│
├── worker/                   # Tauri PTY backend — spawns claude CLI in a PTY
│   ├── Cargo.toml
│   ├── src/                  # Rust: ConPTY (Win) + portable-pty (Unix)
│   ├── tauri.conf.json
│   ├── capabilities/   icons/   build.rs
│   ├── frontend/             # JS UI (xterm.js + parser)
│   │   ├── index.html  terminal.js  claude_cli_parser.js
│   │   └── lib/xterm/        # vendored xterm.js + fit addon
│   ├── package.json
│   └── package-lock.json
│
├── clients/python/           # Reference Python client (machine-user JWT auth)
├── tests/                    # Cross-component WS integration tests
├── deploy/
│   ├── zitadel/              # Zitadel + login UI deployment artifacts
│   ├── manager/              # (TODO) systemd unit + nginx vhost for the manager
│   └── worker/               # (TODO)
├── docs/                     # Endpoint reference per component
└── .github/workflows/ci.yml  # matrix CI: manager × worker, debug × release
```

## Run (worker only, dev)

```bash
cd worker
npm install
npm run tauri dev
```

Build a bundle:

```bash
npm run tauri build
```

If `claude` isn't found, the terminal will show an `echo` message instead of crashing — install Claude Code (`npm i -g @anthropic-ai/claude-code`) or place `claude.exe` somewhere on `PATH` and relaunch.
