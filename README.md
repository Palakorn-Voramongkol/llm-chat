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
├── src/
│   ├── index.html          # single window
│   ├── main.js             # xterm + PTY wiring + auto-start
│   ├── styles.css
│   └── lib/xterm/          # vendored xterm.js + fit addon
└── src-tauri/
    ├── src/
    │   ├── main.rs
    │   └── lib.rs          # ConPTY, find_claude_path, pty_write/resize/terminal_ready
    ├── capabilities/default.json
    ├── tauri.conf.json
    ├── Cargo.toml
    └── build.rs
```

## Run

```bash
npm install
npm run tauri dev
```

Build a bundle:

```bash
npm run tauri build
```

If `claude` isn't found, the terminal will show an `echo` message instead of crashing — install Claude Code (`npm i -g @anthropic-ai/claude-code`) or place `claude.exe` somewhere on `PATH` and relaunch.
