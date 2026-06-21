# Lumina — desktop chat client (`clients/tauri`)

> Status: **built** at `clients/tauri/` (Tauri 2 + React/TS/Vite/Tailwind).
> The shipped auth flow is **Authorization Code + PKCE via the system browser**,
> not the ROPC password grant this doc originally proposed (Zitadel's password
> grant is disabled for this client). Rendering uses **highlight.js**
> (rehype-highlight), not Shiki. Other major pieces match this record.
> Distinct from `clients/rust` (the `llm-chat` REPL) and `clients/python` (the
> reference client) — same `/chat` protocol, different front ends.

## Purpose

A polished desktop chat client for the llm-chat stack that renders claude's
answers richly: markdown, syntax-highlighted code, math, Mermaid + PlantUML
diagrams, inline images, sanitized HTML, and PDFs. It talks to the existing
manager `/chat` WebSocket (same protocol as the Python/Rust clients) and adds a
real **login + authorization** gate.

Product name **Lumina** lives in one constant `APP_NAME` (Rust `config.rs` +
frontend `config.ts`) so it is trivially renamable.

**In scope:** login, authorization gate (role check), multi-session chat UI,
rich rendering of the content types above, keyring token storage, dark/light
theme. **Out:** formats needing conversion (TIFF, RAW), offline PlantUML (we
call a render server), multi-window, auto-update.

## Architecture

```
Tauri app (Lumina): Rust shell (src-tauri) + React/TS frontend (src)
  ── ws://…/chat (Authorization: Bearer <jwt>) ──►  manager ──► worker ──► claude
```

The Rust shell owns secrets and the network: it runs the OIDC token calls
(reqwest), holds the refresh token in the OS keyring, and bridges the `/chat`
WebSocket, emitting Tauri events to the webview. The webview never sees the
refresh token or the WS auth header (CSP-restricted); it gets only a sanitized
`Identity { sub, email, name, roles }`.

### Rust modules (`src-tauri/src`)

- `auth.rs` — Auth Code + PKCE: `login()` builds the authorize URL, opens the
  system browser, captures the loopback redirect on `127.0.0.1:8477`, and
  exchanges the code at `/oauth/v2/token`. `restore()` does the refresh-token
  grant from the keyring; `logout()` revokes. Discovers endpoints via
  `.well-known/openid-configuration` (with fallback). Decodes the JWT (no
  verify) to extract roles from any `…:roles` claim. `get_config()` exposes
  config to the webview.
- `tokens.rs` — OS keyring wrapper for the refresh token (per issuer).
- `chat.rs` — `chat_connect()` opens the `/chat` WS with the bearer token and
  forwards every frame to the webview as a single Tauri event `chat://frame`
  (`chat://closed` on drop); `chat_send(id, text)` writes a `{type:q,…}` frame;
  `chat_close()`.
- `config.rs` — `Config::load()`: `APP_NAME`, issuer, `manager_ws`, project,
  OIDC client id, PlantUML server, `required_role` (default `chat.app`). Reads
  `LUMINA_*` env, falls back to repo `secrets/` for project/client id.
- `lib.rs` / `main.rs` — Tauri builder, command registration, single window.

### Frontend modules (`src`)

- `auth/` — `useAuth` (login/restore/logout, identity), `LoginScreen`,
  `AuthorizationGate` (mounts children only if identity holds `REQUIRED_ROLE`,
  else an access-denied screen).
- `chat/` — `useChat` (subscribes to `chat://frame`, session state), `ChatView`,
  `Composer`, `Message`, `DayDivider`.
- `render/` — `Markdown` (react-markdown + remark-gfm/-math + rehype-katex/-raw
  + rehype-sanitize), `CodeBlock` (highlight.js), `Mermaid`, `PlantUml`, `Pdf`
  (pdf.js), `Fallback`. A fence dispatcher routes ` ```mermaid ` / ` ```plantuml `
  to their renderers, everything else to highlight.js.
- `lib/tauri.ts`, `lib/url.ts` — typed `invoke()` wrappers + event listeners.

## Authentication & authorization

**Authentication:** standard browser OIDC. `login()` runs Authorization Code +
PKCE against Zitadel's hosted login (system browser, loopback redirect),
exchanges the code for access + refresh + id tokens. Refresh token → keyring;
access token in the Rust process only; the webview sees only the sanitized
`Identity`. (The original design proposed ROPC password grant; it was dropped
because the grant is disabled for this client and PKCE is the secure standard.)

**Authorization:** Lumina's own gate requires the project role
`LUMINA_REQUIRED_ROLE`, which **defaults to `chat.app`** (`config.rs`,
`config.ts`). `AuthorizationGate` checks `identity.roles` for it; if absent the
user is authenticated but shown a "ask an admin for the role" screen and the chat
UI never mounts. The manager independently still requires `chat.user` on `/chat`
(defense in depth; see [`../../../docs/architecture.md`](../../../docs/architecture.md)).

> **Known divergence (current code).** The provisioner
> (`deploy/compose/provisioner/provision.py`) only creates `chat.user` and
> `chat.admin` — it does **not** create or grant `chat.app`, nor register a
> dedicated Lumina OIDC app. So against the default provisioned stack Lumina
> rejects every user. To run it today, either set `LUMINA_REQUIRED_ROLE=chat.user`
> or add a `chat.app` role + grant in the provisioner. This was part of the
> original design that was never wired into `provision.py`.

## Rendering pipeline

claude answers in markdown; `Markdown` parses it once and renders prose/lists/
tables/blockquotes as styled HTML, `$…$`/`$$…$$` → KaTeX, and raw HTML via
rehype-raw then **sanitized** (allowlist — never unsanitized
`dangerouslySetInnerHTML`). Fenced code is dispatched by language: `mermaid` →
SVG; `plantuml` → deflate+base64 encode (pako) and load `<img>` from the
configured PlantUML server (default `https://www.plantuml.com/plantuml`);
anything else → highlight.js with a copy button. Inline images → click-to-zoom;
PDFs → a pdf.js viewer. Display-only: claude's markdown is the source of truth.

## Error handling

- Login: bad credentials / Zitadel error → inline form error; issuer unreachable
  → "cannot reach the sign-in server".
- Authorization: missing role → access-denied screen, not a toast.
- Chat: WS drop → `chat://closed` + reconnect; `err` frame → inline error bubble.
  Each renderer is in an error boundary (`Fallback`) so one bad block (bad
  mermaid/plantuml/math) shows its raw source with a "couldn't render" note
  rather than blanking the conversation.

## Build / run

- `npm install`, then `npm run tauri dev` / `npm run tauri build`. `npm test`
  runs Vitest.
- Frontend builds with Vite; the Rust shell is its own crate, **not** in the
  root Cargo workspace (Tauri apps carry their own build config / webview
  binary).
- Env: `LUMINA_ISSUER`, `LUMINA_MANAGER_WS`, `LUMINA_PROJECT`,
  `LUMINA_OIDC_CLIENT_ID`, `LUMINA_PLANTUML_SERVER`, `LUMINA_REQUIRED_ROLE`,
  `LUMINA_SECRETS_DIR`.
