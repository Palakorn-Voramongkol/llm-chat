# kabytech identity UX — invitation, registration & custom login design

**Status:** designed 2026-06-22; not yet implemented. Builds on the
[kabytech gateway app MVP](2026-06-22-kabytech-gateway-app-design.md) (login-only,
OIDC-redirect). Replaces the plain Zitadel hosted UI with **kabytech's own
beautiful pages** and adds **invite-only registration**.

## Decisions (from brainstorming)

- **Approach A — full custom UI.** kabytech renders its own login / accept-invite
  / invite pages; the backend drives Zitadel's **Session API** (login) and
  **User API v2** (invite/register). Total control of the look.
- **Invite-only.** No public signup. Users join only by completing an invite an
  operator created. "Registration" = setting a password on the invite link.
- **Real email invites** via SMTP. Local dev uses a **MailHog** container (real
  SMTP, captured in a web inbox); production swaps in real SMTP creds.
- **`/invite` is a kabytech page gated to `chat.admin`** operators.
- **Two phases**, each its own implementation plan: Phase 1 = invitation +
  registration; Phase 2 = custom login.

## Architecture

kabytech becomes the **login UI** Zitadel delegates to for its OIDC app. The
backend holds a privileged service account and the client secret; the browser
holds only the opaque session cookie.

```
            ┌── /invite (chat.admin) ─▶ POST /api/invite ─▶ Zitadel User API: create user (no pw)
            │                                              + grant chat.user + email invite link
Browser ────┼── invite email (MailHog/SMTP) ─▶ /accept?userID&code
            │        └─▶ POST /api/accept ─▶ Zitadel: verify code + set password
            │
            └── /login (custom form) ─▶ POST /api/login ─▶ Zitadel Session API (password check)
                                          ─▶ finalize OIDC auth request ─▶ callback ─▶ tokens ─▶ cookie
```

## Components

- **MailHog** — new compose service (SMTP sink + web inbox at `:8025`); Zitadel's
  SMTP provider points at `mailhog:1025`.
- **kabytech-backend** — new endpoints:
  - `POST /api/invite` (`chat.admin`-gated): create a Zitadel human user (no
    password), grant `chat.user`, trigger the invite email.
  - `POST /api/accept`: verify the invite code + set the chosen password.
  - `POST /api/login` (Phase 2): create a Zitadel session (password check),
    finalize the OIDC auth request, follow the callback, set the session cookie.
- **kabytech-frontend** — new beautiful pages: `/login`, `/accept`, `/invite`.
- **provisioner** — a kabytech service account with `ORG_USER_MANAGER` (create
  users/grants/invites) + `IAM_LOGIN_CLIENT` (drive the Session API); SMTP config;
  point the OIDC app's login UI base URL at kabytech.

## Phase 1 — Invitation + registration

### SMTP (dev: MailHog)

- Add a `mailhog` service (`mailhog/mailhog`), ports `127.0.0.1:8025` (web UI) +
  internal `1025` (SMTP).
- Provisioner configures Zitadel SMTP via `POST /admin/v1/smtp`
  (`AddSMTPConfigRequest`): `host=mailhog:1025`, `tls=false`,
  `sender_address=noreply@kabytech.local`, `sender_name=kabytech`. Idempotent
  (a 409 / existing-config path treated as provisioned).
- Production: the same call with real SMTP host/user/password (operator-supplied
  env), no code change.

### Invite (`/invite`, `chat.admin`)

- A `chat.admin` operator opens `/invite` (gated by the session's role, like the
  existing `EndUser` extractor but requiring `chat.admin`), enters an email (+
  optional given/family name), submits.
- `POST /api/invite` → backend, using the kabytech SA token:
  1. `POST /v2/users/human` with `profile` + `email` and
     `email.sendCode.urlTemplate =
     {public_origin}/accept?userID={{.UserID}}&code={{.Code}}&orgID={{.OrgID}}`
     — Zitadel creates the user and emails the invite link (→ MailHog in dev).
  2. Grant `chat.user` (`POST /v2/users/{id}/grants` / mgmt grant, like the
     provisioner's `grant_role`).
- Returns the created `userId`; the page confirms "invite sent to <email>".

### Accept / register (`/accept`)

- The invite email link opens kabytech `/accept?userID&code&orgID` — a beautiful
  page with a "set your password" form (+ confirm).
- `POST /api/accept` → backend:
  1. Verify the code + mark email verified (`VerifyEmail`: `{user_id,
     verification_code}`).
  2. Set the chosen password (Zitadel set-password for the user, authenticated by
     the SA / the verified code).
- On success, redirect to `/login`. Invalid/expired code → a clear inline error
  (fail closed; no account state leaks).

## Phase 2 — Custom login (`/login`)

Replaces the OIDC-redirect-to-hosted-UI with a kabytech form, using the Session
API + auth-request finalize (the verified Zitadel "build your own login UI"
flow):

1. The OIDC app is configured so an authorize request lands on kabytech `/login`
   with an `authRequestId` (Zitadel delegates the login UI to kabytech).
2. `/login` renders the beautiful form. `POST /api/login` →
   `POST /v2/sessions` with a password check (`{checks:{user:{loginName},
   password:{password}}}`) → `sessionId` + `sessionToken`.
3. `POST /v2/oidc/auth_requests/{authRequestId}` with the session (backend SA must
   hold `IAM_LOGIN_CLIENT`) → returns the callback URL.
4. Backend follows the callback → gets the code → exchanges it for tokens (the
   existing `exchange_code`) → verifies `chat.user` → sets the cookie session.

Bad credentials → a clear inline error; `chat.user` missing → 403 (fail closed).

## Provisioning additions

- **kabytech SA**: a machine user `kabytech-login` with a JSON key, granted
  `ORG_USER_MANAGER` (org member) + `IAM_LOGIN_CLIENT` (instance member). Key
  written to `secrets/kabytech-login-key.json`; the backend mints a token from it
  for the User/Session/finalize calls. Least privilege — no `ORG_OWNER`.
- **SMTP**: `POST /admin/v1/smtp` as above.
- **OIDC app login UI**: set the kabytech-gateway app (or instance) login UI base
  URL to `{frontend_origin}` so Zitadel redirects authorize requests to kabytech
  `/login` (Phase 2). Phase 1 does not require this.

## Visual direction

Modern and professional, consistent across `/login`, `/accept`, `/invite`: a
centered card on a subtle branded gradient background, a kabytech wordmark, clean
type scale, generously-spaced fields with visible focus + error/validation
states, a primary action button, and concise helper text. Built with Tailwind v4
primitives (shadcn can be adopted if the surface grows). **Each page is rendered
and screenshotted for approval and iterated on** before it's considered done.

## Security

- `/invite` + `POST /api/invite` require `chat.admin` (a `chat.admin` session
  extractor); login + `/api/accept` are unauthenticated by necessity but
  fail-closed (invalid code → reject, no state leak).
- The kabytech SA is least-privilege (`ORG_USER_MANAGER` + `IAM_LOGIN_CLIENT`
  only); its key never leaves the backend; passwords flow browser→backend→Zitadel
  only (TLS in prod; the local stack is plain HTTP for dev only).
- Granting is exactly `chat.user` on the one chat project — no role widening.
- New users are created already granted `chat.user`, so a completed invite can
  immediately log in; an un-accepted invite cannot (no password set).

## Error handling

- SMTP unreachable at provision time → log + fail the SMTP step loudly (invites
  would silently not send otherwise).
- `POST /api/invite` duplicate email → surface Zitadel's "already exists" as a
  friendly "that user already exists" (no crash).
- `/accept` invalid/expired code → inline error + a "request a new invite" hint.
- `/api/login` (Phase 2) Session API failure / wrong password → inline "invalid
  credentials"; auth-request finalize failure → 502 with a generic message.

## Testing

- **Backend (unit):** the invite-user request body (profile + email + the
  `urlTemplate` pointing at `/accept`); the `chat.admin` extractor accepts only
  `chat.admin` sessions; the SMTP config body builder; (Phase 2) the
  session-create + finalize request shapes. Pure, no network.
- **Frontend (component):** `/accept` renders the set-password form and posts;
  `/invite` renders the email form and the success state; `/login` (Phase 2)
  renders + posts credentials. Fetch mocked.
- **Live smoke (gated):** invite `e2e-invitee@kabytech.local` from `/invite` →
  open the link from MailHog's inbox → set a password on `/accept` → log in via
  `/login` → land authenticated. Verified by driving the browser + reading
  MailHog.

## Non-goals

- No public open signup (invite-only).
- No social / upstream-IdP buttons on these pages (the federation design is
  separate).
- No password reset / MFA management UI in this iteration (Zitadel handles reset
  out of band; MFA stays optional as today).
- Production SMTP credentials are operator-supplied; this spec wires dev (MailHog)
  and the config call, not a real mail provider.
