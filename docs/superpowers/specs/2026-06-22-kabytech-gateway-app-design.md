# kabytech gateway app — auth/login MVP design

**Status:** designed 2026-06-22; not yet implemented. First slice of the kabytech
gateway (the [pass-through design](2026-06-22-gateway-identity-passthrough-design.md)).

**One-liner:** A standalone web app — `kabytech/backend` (Rust/axum OIDC Relying
Party) + `kabytech/frontend` (Next.js 16 + Tailwind v4) — that logs an end-user
in against Zitadel and establishes an authenticated session. It mirrors the
`admin-api` ↔ `admin-web` pattern, gating on `chat.user` instead of `chat.admin`.

## Scope

**In (this MVP):** a complete browser login round-trip — Sign in → Zitadel →
back to a logged-in home showing who you are → Logout. The authenticated session
holds the end-user's access **and** refresh tokens (for later phases).

**Out (later phases, explicitly not now):** forwarding chat to the manager, the
per-user `/chat` WebSocket, long-connection token refresh, and the upstream-IdP
federation wiring (that is Zitadel config; the app code does not change when it
is added).

## Architecture

```
Browser ─▶ kabytech/frontend (Next.js 16, :3001)
              │ same-origin proxy (next.config rewrites): /login /callback /logout /api/me
              ▼
          kabytech/backend (Rust axum, :7670) ── OIDC Auth Code + PKCE ─▶ Zitadel (:8080)
                                                  uses the kabytech-gateway OIDC client
```

Same shape as `admin-web` ↔ `admin-api`. The backend is the OIDC Relying Party
and the only holder of the client secret + session; the frontend is UI + a
same-origin proxy (no CORS, `SameSite=Lax` cookie).

## kabytech/backend (Rust axum) — mirrors `admin-api/src/auth.rs`

**Crate:** new Cargo **workspace member** `kabytech/backend` (added to the root
`Cargo.toml` `members`, so compose Dockerfiles build it in the full workspace).

**Files (one responsibility each):**
- `kabytech/backend/Cargo.toml` — deps: `axum`, `tokio`, `tower-sessions`,
  `serde`, `serde_json`, `reqwest`, `base64`, `sha2`, `rand`, `url`, `tracing`,
  and the workspace `zitadel-auth` crate (shared JWKS verify).
- `src/config.rs` — `KabyConfig` from env, **fail-fast** on any missing required
  value (no silent defaults): `issuer`, `project_id`, `oidc_client_id`,
  `oidc_client_secret`, `bind_addr`, `public_origin` (the backend's own origin,
  for the OIDC `redirect_uri`), `allowed_origin` (the frontend origin to 302 back
  to), `session_key`, `cookie_secure`.
- `src/auth.rs` — the OIDC flow, ported from `admin-api/src/auth.rs`: pure
  `pkce_pair(seed)` + `build_authorize_url(cfg, challenge, state, nonce)`
  (requesting `openid profile email offline_access`, the project-aud scope, and
  the projects-roles scope), and handlers `login`, `callback`, `logout`,
  `exchange_code`, `fetch_display_name`.
- `src/session.rs` — `EndUser { user_id, name, roles }` stored in a
  `tower_sessions` signed-cookie session, plus the access/refresh tokens and a
  `login_at` stamp; an `EndUser` extractor that rejects unauthenticated requests.
- `src/main.rs` — router + `AppState` (config, shared `reqwest::Client`,
  `zitadel-auth` JWKS cache, session layer). Routes:
  `GET /login`, `GET /callback`, `GET /logout`, `GET /api/me`.

**Auth flow (ported, gate changed to `chat.user`):**
1. `GET /login` — generate PKCE verifier/challenge + `state` + `nonce`, stash in
   the pre-auth session, 302 to Zitadel `/oauth/v2/authorize`.
2. `GET /callback` — verify `state` (CSRF), exchange `code`+`verifier` at
   `/oauth/v2/token` (HTTP Basic with the client id/secret), verify the returned
   access-token JWT via the **shared `zitadel-auth` JWKS cache**, **require
   `chat.user` — fail closed (403) otherwise**, resolve a display name via OIDC
   userinfo (best-effort, display-only), `cycle_id()` the session (fixation
   defense), store `EndUser` + tokens + `login_at`, 302 to the frontend origin.
3. `GET /logout` — clear the session, best-effort Zitadel `end_session`.
4. `GET /api/me` — return `{userId, name, roles}` for the session, or **401** if
   unauthenticated.

**Security posture (inherited from the platform's rules):** fail-fast on missing
config; gate strictly on `chat.user`; `cookie_secure` defaults to the secure
value (only set false for local plain-HTTP dev); identity + authz ride the
**verified JWT**, never client input; the client secret never leaves the backend.

## kabytech/frontend (Next.js 16.2.7 + Tailwind v4) — mirrors `admin-web`

**Files:**
- `kabytech/frontend/package.json` — same Next/React/Tailwind versions as
  `admin-web` (`next@16.2.7`, `react@19`, `tailwindcss@4`, `@tailwindcss/postcss`),
  trimmed to what the MVP needs (no shadcn/tanstack/recharts yet).
- `kabytech/frontend/next.config.ts` — security headers + `rewrites()` proxying
  `/login`, `/callback`, `/logout`, `/api/:path*` to `KABY_BACKEND_ORIGIN`.
- `app/layout.tsx`, `app/globals.css` (Tailwind v4), `app/page.tsx`.

**UI (one page, two states):** `app/page.tsx` fetches `/api/me`. Unauthenticated
→ a centered card with a **Sign in** button linking to `/login`. Authenticated →
a card showing the user's name/email + a **Logout** link to `/logout`. Plain
Tailwind primitives for the MVP (shadcn can be adopted later if the UI grows).

## Ports & compose

`kabytech-backend` :7670, `kabytech-frontend` :3001 — both **loopback-only**
(`127.0.0.1:`) in `docker-compose.yml`, like every other service. Two new compose
services build the crate (full-workspace context) and the Next app; the frontend
`depends_on` the backend. Env wires the kabytech-gateway OIDC client id/secret
(from `secrets/kabytech_oidc_client_id` / `_secret`) and a `KABY_SESSION_KEY`.

## Two decisions (confirmed during brainstorming)

1. **MVP authenticates against Zitadel directly** (Zitadel's own login screen,
   existing users like `chatter`), using the `kabytech-gateway` OIDC client.
   Upstream-IdP federation is layered on later as pure Zitadel config — no app
   change. This makes login work today.
2. **Depends on the `kabytech-gateway` OIDC client** (provisioned by the
   pass-through plan's Task 2). The backend reads its id/secret. For local dev the
   redirect URI must point at the backend's `public_origin` (`/callback`), so the
   provisioned client's redirect URIs must include the dev origin — set via
   `KABYTECH_OIDC_REDIRECT_URI` at provisioning time (default placeholder is the
   prod URL and must be overridden for local dev).

## Error handling

- Missing/invalid config → backend **refuses to start**, naming the missing key.
- `state` mismatch → 403 (CSRF). No PKCE verifier in session → 400. Code exchange
  failure → 502. JWT invalid → 401. Missing `chat.user` → **403** (fail closed).
- `/api/me` when unauthenticated → 401; the frontend renders the signed-out state.
- userinfo lookup failure → display name falls back to email/user_id (identity
  and authz are unaffected; they ride the verified JWT).

## Testing

- **Backend (unit, no network):** `pkce_pair` S256 + determinism; `build_authorize_url`
  carries PKCE/state/nonce + the project-aud and roles scopes + the backend
  `redirect_uri`; `config` fail-fast on a missing required key. The
  `callback` happy path (real code exchange + a real `chat.user` JWT) is a
  **gated live smoke** (needs the stack), like `admin-api`.
- **Frontend (component):** `app/page.tsx` renders the Sign-in state when
  `/api/me` is 401 and the user card when it returns a user (fetch mocked).

## Non-goals

- No chat forwarding / `/chat` WS / token refresh loop (next phase).
- No upstream-IdP federation wiring in app code (Zitadel config; separate).
- No user management, settings, or any admin surface — this app is end-user login
  only. (Operators use the existing Console.)
- No shared session store / horizontal scale yet — in-memory sessions like
  `admin-api` (a restart forces re-login; acceptable for the MVP).
