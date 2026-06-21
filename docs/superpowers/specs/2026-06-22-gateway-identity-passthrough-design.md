# Gateway identity pass-through (kabytech) — design

**Status:** designed 2026-06-22; not yet implemented.

**One-liner:** Let a gateway server (kabytech) serve many human end-users so
that *each end-user's own Zitadel identity* reaches the manager — giving
per-user usage graphs and per-user workspace/session isolation **for free**,
with near-zero platform change.

## Why (and why NOT token-exchange)

Today every request through kabytech authenticates with kabytech's single
machine key, so the manager sees one `sub` (`kabytech`) and collapses all
downstream users into one bucket (one usage graph, one shared workspace, one
shared claude conversation). The platform already attributes usage, confines
workspaces, and threads claude context **per authenticated JWT `sub`** — so the
fix is simply to make the end-user's *own* `sub` reach the manager.

This was first framed as OAuth **token-exchange** (RFC 8693). It is **not
needed and is explicitly dropped**: token-exchange exists for impersonation —
when the gateway must mint tokens for users who never authenticate themselves.
Here the end-users *do* authenticate themselves (browser login). So kabytech is
a **transparent identity-forwarding proxy**: it forwards each user's own bearer
token unchanged. That is strictly simpler and safer — kabytech never holds an
"act-as-anyone" credential, so there is no impersonation key to protect.

## Identity model (the decisions that shaped this)

- **End-users are humans**, signing in through a browser (OIDC Auth Code + PKCE).
- **End-users log in themselves** against an **upstream IdP** that Zitadel
  federates (e.g. Google, or kabytech's own corporate OIDC/SAML — the choice is
  config, not architecture).
- **Just-in-time (JIT) provisioning:** Zitadel auto-creates the local Zitadel
  user on first federated login. A Zitadel **action** then auto-grants
  `chat.user`. No manual per-user setup, no app-side admin-api provisioning path.

## Architecture & data flow

```
End-user (human) ──login──▶ kabytech web app (OIDC RP, Auth Code + PKCE)
        │                          │  requests scopes: openid profile email
        │                          │  offline_access + project-aud + projects-roles
        ▼                          ▼
  Upstream IdP  ◀─federates──  Zitadel ── JIT-creates local user on first login
  (Google / kabytech's own)         └─ External-Auth ▸ Post-Creation action: grant chat.user
                                   │
                                   │ access token: sub=<end-user>, aud=[chat project],
                                   │ roles={chat.user}, + refresh token
                                   ▼
kabytech gateway ── ONE manager /chat WS per end-user, forwarding THAT user's bearer ──▶ manager
                                                                                          │
                                              validates sig + aud + chat.user; attributes per sub;
                                              worker confines workspace + claude session per sub
```

**Invariant:** the `sub` reaching the manager is always the end-user's own,
because kabytech forwards the user's token verbatim. No shared/var identity.

## Components & responsibilities

### A. Zitadel (configuration only — no platform code)

1. **Upstream IdP** registered as an external login provider with automatic
   creation/linking (JIT) enabled.
2. **Auto-grant action.** A JavaScript action attached to the **External
   Authentication** flow, **Post Creation** trigger
   (`TRIGGER_TYPE_POST_CREATION`, id `3`), that adds a `chat.user` **user-grant**
   on the chat project. This is the documented `addGrant` pattern ("assign a
   role to the user upon registration via an external identity provider",
   Zitadel `guides/manage/customize/behavior` + `apis/actions/external-authentication`).
   Granting `chat.user` before token issuance means the very first token already
   carries the role.
3. **OIDC client** for kabytech's web app: Auth Code + PKCE, kabytech's own
   redirect URIs. Public or confidential per kabytech's deployment.

### B. kabytech gateway (integration code — kabytech's side, not this repo)

- Acts as the **OIDC Relying Party**: runs the browser login, holds each
  end-user's session including the **refresh token**.
- For each active end-user, opens a **dedicated** manager `/chat` WebSocket
  carrying *that user's* access token in the `Authorization: Bearer` header (the
  same handshake `clients/python/llm_chat/protocol.py` already performs).
- Supplies a **fresh** access token on every (re)connect via the user's refresh
  token — the `TokenProvider` callable pattern the existing client supports.

### C. Platform (manager, admin-api, admin-web) — UNCHANGED

- **manager** already gates every endpoint on `chat.user`
  (`manager/src/main.rs:1233`), reads the role + `sub` from the verified JWT,
  validates the token's `aud` against the configured chat project, attributes
  self-counted usage per `sub`, and confines the worker workspace + claude
  session per `sub`.
- **admin-api / admin-web** already render per-user usage + the daily graph for
  any user the manager has seen (Users table + detail panel). A new federated
  user simply appears as a new row with its own graph.

## How the token gets the right `aud` + role

kabytech's OIDC client requests the same scopes the existing human-login flow
uses (`clients/python/llm_chat/oidc.py:build_scope`):

```
openid profile email offline_access
urn:zitadel:iam:org:project:id:<CHAT_PROJECT_ID>:aud   # adds chat project to aud
urn:zitadel:iam:org:projects:roles                     # asserts granted roles (chat.user)
```

`<CHAT_PROJECT_ID>` is the platform's chat project (the manager's configured
audience; the same value in `secrets/project_id`). This is proven in-repo: it is
exactly how today's `chat` human login obtains a manager-acceptable token.

## Token lifecycle over the long-lived WS

The manager authenticates **once, at the WS handshake** — it does not re-check
mid-connection. So a live connection survives access-token expiry; the cost lands
only at **reconnect**, where kabytech must present a fresh access token minted
from the user's refresh token. kabytech therefore maintains a per-user OIDC
session for as long as that user is active. (No platform change: the manager's
handshake-time validation and the client's reconnect-with-fresh-token behavior
already exist.)

## Isolation

One end-user → one `sub` → one manager `/chat` connection → one worker workspace
+ one claude conversation. Two end-users can never see each other's files or
conversation context — identical to the confinement that already separates
`kabytech` from `admin`. **No multiplexing:** kabytech must not carry two
end-users over one authenticated WS, because the `sub` is fixed at handshake;
each end-user gets their own connection.

## Security posture (fail-closed)

- **Grant is the gate.** A user without the `chat.user` grant (e.g. the
  Post-Creation action failed) is rejected by the manager with 403 — no fallback,
  nothing leaks. The action is the only thing that opens access.
- **No impersonation credential.** kabytech forwards only tokens users obtained
  themselves; it holds no "act-as-anyone" key. A kabytech compromise exposes
  live sessions, not a master impersonation secret.
- **Least privilege.** The Post-Creation action grants exactly `chat.user` on
  exactly the one chat project. No admin scope, no role widening.
- **No client-supplied identity trust.** Identity comes only from a
  Zitadel-verified JWT; kabytech never asserts a `sub` of its own choosing.

## Error handling

- **Action fails to grant** → first `/chat` is 403. Surfaced to the end-user as
  "access not yet provisioned"; remedied by re-running the grant (action retry or
  one admin-api grant call). Fail-closed: never auto-fall-back to a shared
  identity.
- **Upstream IdP down** → login fails at Zitadel; kabytech shows a login error.
  No platform involvement.
- **Refresh token expired/revoked** → kabytech cannot mint a new access token →
  reconnect fails → end-user must re-login. The manager rejects an expired token
  at handshake (fail-closed).
- **`aud`/scope misconfigured** (kabytech client doesn't request the project-aud
  scope) → manager rejects on audience. Caught by the verification test below.

## Testing / verification

The platform code is unchanged, so testing is integration/E2E, not unit:

1. **Two federated users, two graphs.** Drive a question as federated user A and
   another as federated user B (each via the real OIDC + forward path). Assert the
   Console shows **two distinct user rows**, each with its **own** chars/files
   counts and daily graph — and that neither is attributed to `kabytech`.
2. **Isolation.** As user A, attach a file; as user B, ask claude to read A's
   path. Assert B cannot access it (separate confinement root + separate claude
   session).
3. **Auto-grant.** A brand-new external identity's first `/chat` succeeds (the
   Post-Creation action granted `chat.user` before the first token).
4. **Fail-closed.** A user whose grant was removed gets 403 at `/chat`; no
   fallback bucket appears.
5. **Refresh.** Force access-token expiry on a long session; assert reconnect
   mints a fresh token and the same `sub` is preserved (usage keeps accruing to
   the same user, not a new bucket).

## Non-goals

- **Token-exchange / impersonation (RFC 8693)** — dropped; see "Why NOT" above.
- **Self-service in-Zitadel registration** — out of scope; identity originates
  upstream and is JIT-federated.
- **Multiplexing multiple end-users over one manager WS** — incompatible with
  per-`sub` handshake auth and per-user isolation.
- **Platform code changes to manager/admin-api/Console** — none expected; if the
  verification surfaces a gap, that becomes its own scoped change, not part of
  this spec.
- **A new "kabytech → its end-users" Console view** — unnecessary, because
  federated end-users are real Zitadel users and already appear in the Users
  table with their own graphs.
