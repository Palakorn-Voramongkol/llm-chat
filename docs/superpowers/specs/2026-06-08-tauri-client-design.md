# Lumina — rich desktop client (`clients/tauri`) — design

## Purpose

A professional, beautiful desktop chat client for the llm-chat stack that
renders claude's answers richly: markdown, syntax-highlighted code, math,
Mermaid + PlantUML diagrams, inline images, sanitized HTML, and PDFs. It talks
to the existing manager `/chat` WebSocket (same protocol as the Python/Rust
clients) and adds a real **login + authorization** gate.

Product name: **Lumina** (held in one constant `APP_NAME` so it is trivially
renamable). Window identifier: `com.llm-chat.lumina`.

## Scope (in / out)

**In:** login screen (email+password), authorization gate (role check), a
multi-session chat UI, rich rendering of all the content types above, token
storage, dark/light theme.

**Out (noted, not built now):** non-web image formats requiring conversion
(TIFF, RAW); offline PlantUML rendering (we call a render server); multi-window;
auto-update.

## Architecture

```
┌──────────────────────────── Tauri app (Lumina) ───────────────────────────┐
│  Rust shell (src-tauri)                React + TS frontend (src)            │
│  ───────────────────────               ─────────────────────────           │
│  #[tauri::command] auth.*    ◀────────  Login screen  → useAuth()           │
│    login_password()  (ROPC)            Authorization gate (role check)      │
│    refresh(), logout(), whoami()       Chat view: sessions, composer,       │
│  #[tauri::command] store.*               message list                       │
│    keyring get/set/clear (refresh tok) Renderers: markdown, code (Shiki),   │
│  chat WS bridge (tokio-tungstenite):     math (KaTeX), mermaid, plantuml,   │
│    chat_connect/ chat_send /             html (sanitized), image, pdf.js    │
│    events: chat://answer, chat://ack   useChat() hook ◀ Tauri events        │
└──────────────────────────────────────────────────────────────────────────┘
                              │ ws://…/chat  (Authorization: Bearer <jwt>)
                              ▼
                         manager  ──►  worker  ──►  claude
```

The Rust shell owns secrets and the network: it performs the OIDC token calls
(reqwest), holds the refresh token in the OS keyring, and bridges the `/chat`
WebSocket — emitting Tauri events to the webview. The webview never sees the
refresh token and never opens the socket directly. This keeps the access token
and WS auth header out of arbitrary frontend JS (CSP-restricted).

### Modules (Rust, `src-tauri/src`)

- `auth.rs` — `login_password(email, password) -> Identity` (Zitadel ROPC at
  `/oauth/v2/token`, grant_type=password, scope incl. the roles scope);
  `refresh()`, `logout()` (revoke), `whoami()`. Decodes the JWT (no verify) to
  return `{ sub, email, roles[] }`. Stores the refresh token in keyring, access
  token in memory.
- `tokens.rs` — keyring wrapper (service `"lumina"`, user `refresh:<issuer>`),
  reused pattern from `clients/rust`.
- `chat.rs` — `chat_connect()` opens the `/chat` WS with the bearer token,
  reads the `initialized` frame, and forwards `ack`/`a`/`err` frames to the
  webview as Tauri events; `chat_send(text)` writes a `q` frame; reconnect.
- `config.rs` — `APP_NAME`, default issuer/manager URL/project/client id, the
  required app role (`REQUIRED_ROLE = "chat.app"`), env overrides.
- `lib.rs` — Tauri builder, command registration, single window.

### Modules (frontend, `src`)

- `auth/` — `useAuth` (login/logout/refresh, identity), `LoginScreen`,
  `AuthorizationGate` (renders children only if identity has `REQUIRED_ROLE`,
  else an "access denied" screen).
- `chat/` — `useChat` (subscribe to Tauri events, session state), `ChatView`,
  `Composer`, `MessageList`, `Message`.
- `render/` — `Markdown` (react-markdown + remark-gfm + remark-math +
  rehype-katex + rehype-raw + sanitize), `CodeBlock` (Shiki), `Mermaid`,
  `PlantUml`, `HtmlBlock`, `ImageBlock`, `PdfViewer` (pdf.js). A `fence`
  dispatcher maps ```` ```mermaid ````/```` ```plantuml ```` to their renderers
  and everything else to Shiki.
- `ui/` — theme (dark/light), layout (sidebar + main), Button, Spinner, Toast.
- `lib/tauri.ts` — typed wrappers over `invoke()` + event listeners.

## Authentication & authorization

**Authentication (login + password):** a custom email+password screen. On
submit, the Rust `login_password` command runs Zitadel's **Resource Owner
Password Credentials** grant and returns access + refresh + id tokens. Refresh
token → keyring; access token kept in the Rust process; the webview gets only a
sanitized `Identity { sub, email, roles }`.

> Security note: ROPC means the app handles the password and cannot enforce MFA.
> It is chosen because the request is explicitly an in-app "login and password"
> form. The provisioner enables the password grant on a dedicated OIDC app. The
> documented hardening path is Auth Code + PKCE in an embedded webview window
> (the `auth.rs` interface is shaped so this can replace ROPC without frontend
> changes).

**Authorization (beyond authentication):** the account must hold the
`chat.app` project role. After login, the `AuthorizationGate` checks
`identity.roles` for `REQUIRED_ROLE`; if absent, the user is authenticated but
shown a clear "You don't have access to Lumina — ask an admin for the `chat.app`
role" screen, and the chat UI never mounts. The manager independently still
requires `chat.user` on `/chat` (defense in depth). The demo user is granted
both `chat.user` and `chat.app`.

**Provisioner changes** (`deploy/compose/provisioner/provision.py`): create the
`chat.app` role (idempotent, like `chat.user`), grant it to the demo human user,
and register a dedicated OIDC app for Lumina with the password grant enabled.

## Rendering pipeline

claude answers in markdown. The `Markdown` component parses it once and renders:
- prose/lists/tables/blockquotes → styled HTML;
- `$…$` / `$$…$$` math → KaTeX;
- raw HTML in the markdown → rehype-raw, then **sanitized** (allowlist) before
  mounting — never `dangerouslySetInnerHTML` on unsanitized input;
- fenced code blocks dispatched by language:
  - `mermaid` → Mermaid renders to SVG;
  - `plantuml` → encode (deflate+base64) and load `<img>` from the configured
    PlantUML server (default `https://www.plantuml.com/plantuml`, overridable to
    a self-hosted one);
  - anything else → **Shiki** syntax highlighting (VS Code TextMate grammars +
    themes), with a copy button;
- images (`![](…)`, web formats) → inline `<img>` with click-to-zoom;
- PDFs (a link ending `.pdf`, or a `pdf` fence with a URL/data) → a pdf.js
  viewer panel.

Display-only: claude's exact markdown is the source of truth; we render it, we
never reconstruct it.

## Error handling

- Login: wrong credentials → inline form error (Zitadel 400/401 mapped); issuer
  unreachable → "cannot reach sign-in server".
- Authorization: missing role → access-denied screen (not an error toast).
- Chat: WS drop → auto-reconnect (token refreshed first); answer timeout → an
  inline "[no answer within Ns]" in the thread; `err` frame → inline error
  bubble. Per-renderer failures (bad mermaid/plantuml/math) → show the raw
  fenced source with a small "couldn't render" note, never crash the message.
- Each renderer is isolated in an error boundary so one bad block can't blank
  the conversation.

## Testing

- Rust: unit tests for the JWT claim/role decode, the keyring round-trip, and
  the PlantUML encoder (deflate+base64 against a known vector).
- Frontend: component tests (Vitest + Testing Library) for the fence dispatcher
  (routes mermaid/plantuml/code correctly), the markdown→KaTeX path, the HTML
  sanitizer (strips `<script>`/handlers), and `AuthorizationGate` (mounts only
  with the role).
- Manual: a checklist message containing one of each content type renders
  correctly.

## Build / run

- `npm install` then `npm run tauri dev` (dev) / `npm run tauri build` (bundle).
- Frontend builds with Vite; the Rust shell is a workspace-independent crate (it
  is **not** added to the root Cargo workspace — Tauri apps carry their own
  build config and a webview-linked binary, like a separate product).
- Env: `LUMINA_ISSUER`, `LUMINA_MANAGER_WS`, `LUMINA_PROJECT`,
  `LUMINA_OIDC_CLIENT_ID`, `LUMINA_PLANTUML_SERVER`.

## Milestones

1. Scaffold (Tauri 2 + React/TS/Vite/Tailwind), window, theme, app shell.
2. Auth: Rust ROPC + keyring; LoginScreen; AuthorizationGate; provisioner role.
3. Chat: Rust WS bridge + events; useChat; ChatView/Composer/MessageList.
4. Renderers: markdown+code+math first, then mermaid/html/image, then
   plantuml + pdf.
5. Polish: theming, copy buttons, zoom, empty/error states; tests; README.
