# kabytech gateway — identity pass-through integration contract

kabytech forwards each end-user's **own** Zitadel token to the manager, so the
platform attributes usage and isolates sessions per end-user. kabytech holds no
impersonation credential.

## 1. OIDC client

Use the `kabytech-gateway` confidential OIDC web client provisioned for you:
`secrets/kabytech_oidc_client_id` + `secrets/kabytech_oidc_client_secret`.
Flow: Authorization Code + PKCE. Register your real redirect URI via
`KABYTECH_OIDC_REDIRECT_URI` at provisioning time.

## 2. Scopes (required — the token will be rejected without them)

Request, on every end-user login:

    openid profile email offline_access
    urn:zitadel:iam:org:project:id:<CHAT_PROJECT_ID>:aud
    urn:zitadel:iam:org:projects:roles

`<CHAT_PROJECT_ID>` = `secrets/project_id`. The project-aud scope puts the chat
project in the token `aud` (the manager validates it); the roles scope asserts
`chat.user` (the manager gates on it); `offline_access` returns the refresh
token you need for step 4.

## 3. Forwarding (one WS per end-user)

For each active end-user, open a dedicated manager `/chat` WebSocket with that
user's access token:

    Authorization: Bearer <end-user access token>

Do **not** multiplex two end-users over one WS — the `sub` is bound at the
handshake. One end-user = one connection = one isolated claude session.

## 4. Token lifecycle

The manager validates the token **once, at the handshake**; a live connection
survives token expiry. On reconnect, mint a fresh access token from the user's
refresh token and present it. Keep a per-user OIDC session for as long as the
user is active.

## 5. Fail-closed semantics

- A user without a `chat.user` grant is rejected with HTTP 403 at the handshake.
  This is expected on the very first login only if the JIT auto-grant action did
  not run; surface "access provisioning pending" and retry — never fall back to a
  shared identity.
- An expired/revoked refresh token => the user must re-login. Do not substitute
  another user's token.

## 6. Verifying it works

After a user chats, the Console (Users → that user → Usage) shows their own
chars/files counts and daily graph, attributed to their `sub` — not to
`kabytech`. Two different end-users appear as two different rows.
