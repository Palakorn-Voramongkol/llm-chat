# Lumina — rich desktop client (Tauri 2 + React)

A professional desktop chat client for the llm-chat stack that renders claude's
answers richly: markdown, syntax-highlighted code, math, Mermaid + PlantUML
diagrams, inline images, sanitized HTML, and PDFs. It talks to the same manager
`/chat` WebSocket as the other clients, behind a real **login + authorization**
gate.

## What it does

- **Login** — Zitadel sign-in (Authorization Code + PKCE). The Rust shell opens
  the browser, captures the loopback redirect, and exchanges the code. The
  refresh token is stored in the OS keyring; the access token never leaves the
  shell; the webview only gets `{ sub, email, name, roles }`.

  > Zitadel's password grant (an in-app username/password form) is disabled for
  > this client, so the secure hosted-login flow is used. To use an in-app
  > password form instead, enable the password grant on the OIDC app and switch
  > `auth.rs` to the `password` grant.

- **Authorization** — beyond authentication, the account must hold the
  **`chat.app`** project role (configurable via `LUMINA_REQUIRED_ROLE`).
  Otherwise a clear "access denied" screen is shown and the chat never mounts.
  (The manager independently still requires `chat.user` on `/chat`.)

- **Rendering** — markdown (GFM tables, blockquotes, lists), KaTeX math,
  `highlight.js` code with copy buttons, ```mermaid → SVG, ```plantuml → a
  render-server image, ```pdf / `.pdf` links → an embedded viewer, sanitized raw
  HTML, and inline images. Each diagram has a fallback that shows the raw source
  if it can't render — one bad block never blanks a message.

## Run

```bash
cd clients/tauri
npm install
npm run tauri dev        # dev window (hot reload)
# or
npm run tauri build      # bundle an installer
```

Requires Node + the Rust toolchain (and a WebView runtime: WebView2 on Windows,
WebKitGTK on Linux).

## Config (env, read by the Rust shell)

| Var | Default |
|---|---|
| `LUMINA_ISSUER` | `http://host.docker.internal:8080` |
| `LUMINA_MANAGER_WS` | `ws://127.0.0.1:7777/chat` |
| `LUMINA_PROJECT` | `secrets/project_id` |
| `LUMINA_OIDC_CLIENT_ID` | `secrets/oidc_client_id` |
| `LUMINA_REQUIRED_ROLE` | `chat.app` |
| `LUMINA_PLANTUML_SERVER` | `https://www.plantuml.com/plantuml` |

## The `chat.app` role

The intended app role is `chat.app`. To grant it to the demo user, add it to the
provisioner (create the project role + grant it to the demo human user, next to
`chat.user`) and re-provision. For a quick try against an already-running stack
where the demo user only has `chat.user`, run with
`LUMINA_REQUIRED_ROLE=chat.user`.

## Layout

```
src/                     React frontend
  auth/                  useAuth, LoginScreen, AuthorizationGate
  chat/                  useChat, ChatView, Composer, Message
  render/                Markdown dispatcher + CodeBlock/Mermaid/PlantUml/Pdf
  lib/tauri.ts           typed IPC wrappers
src-tauri/               Rust shell
  src/auth.rs            PKCE login, refresh, logout, role decode
  src/chat.rs            /chat WebSocket bridge
  src/tokens.rs          keyring; src/config.rs  env/secrets
```
