# Backend-owned `/status` and `/whoami` — design

**Status:** designed 2026-06-27; not yet implemented.
**Area:** manager (new `/identity` WS endpoint + the moved renderers), clients (rust + python), config.
**Rule it enforces:** *Business logic lives in the backend, not the client* (CLAUDE.md). The clients stop resolving identifiers, deciding which claim to trust, and formatting status — they send a request and print the backend's answer.

## Problem / goal

`/status` (and `whoami`) in the chat clients currently do **business logic in the
client**: they decode the JWT (`identity_from_token`), pick `email`/`preferred_username`/`sub`,
and render the status block (`format_status`) — duplicated across the rust and python
clients. The result also leaks raw numeric ids (the access token has no profile claims,
so it falls back to the numeric `sub`; the project shows its numeric id).

**Goal:** the **backend** resolves the identity (id → human labels) and renders the
whole block; the clients become pure relays — send a request with only the local facts
the backend can't know, receive the rendered text, print it. **Everything in `/status`
comes from the backend**; if the backend doesn't have a field, the request carries it.

## Architecture / data flow

```
client → connect  ws /identity   (Authorization: Bearer <user JWT>)
       → send     { type:"status", client:{ kind, version, renderMode, timeoutSecs,
                                             managerUrl, connected, sessionId, msgsThisSession } }
manager:  verify JWT (chat.user)  →  resolve identity ON DEMAND:
            displayName / email  ← Zitadel /userinfo  (the user's OWN access token; not an admin API)
            roles                ← the verified JWT principal
            sub / projectId      ← verified JWT / config
            projectName          ← MANAGER_PROJECT_NAME config (else the id)
            issuer               ← manager config
        →  render the FULL status block (the moved `format_status`)
       → send     { type:"status", block:"─ status ───…\n …\n────" }   → close
client → prints `block` verbatim.   (whoami: { type:"whoami" } → { type:"whoami", line })
```

The client supplies only what it alone knows (its kind/version, render mode, timeout, the
manager URL it dialed, and its current `/chat` connection's session id + msg count). The
manager supplies the identity + project + issuer and **renders the block**. No identifier
mapping, claim-picking, or formatting remains in the client.

## Manager

### New `/identity` WS endpoint (chat.user-gated, no chat session)

Added to the post-handshake path dispatch alongside `/chat`, `/control`, `/s/`, `/qa/`, `/`.
The handshake already verifies the JWT and requires `chat.user`; `/identity` reuses that. It
opens **no** chat session and spawns no worker. It handles two request types and closes:

- `{type:"status", client:{…}}` → `{type:"status", block:"<rendered>"}`
- `{type:"whoami"}` → `{type:"whoami", line:"<rendered>"}`

### Token capture for `/userinfo`

The sync handshake callback (which can't `await`) captures the **raw verified token** into a
holder next to the existing `user_id`/`roles` holders, and hands it to the async `/identity`
handler. The handler uses it **only** for the `/userinfo` call within that request, then the
connection closes — **no session-long token retention**. A `sub → resolved-name` cache in
`ManagerState` skips repeat `/userinfo` calls.

### Identity resolution

`resolve_user_label(http, issuer, access_token) -> UserLabel` mirrors admin-api's
`fetch_display_name` (auth.rs:164): `GET {issuer}/oidc/v1/userinfo` with the access token,
preferring `name` → `preferred_username` → `email`. **Best-effort:** on any failure it falls
back to the JWT `email`/`preferred_username`/`sub` so `/status` never errors.

### The renderer moves into the manager

`format_status(...)` and `format_whoami(...)` (the box-drawing) move **from the clients into
the manager** as **pure** functions taking the resolved identity + the client-supplied
context, returning the exact text block. One renderer (Rust) replaces the duplicated
rust+python versions. `MANAGER_PROJECT_NAME` (manager env) supplies the friendly project
name; unset → the project id (graceful).

## Clients (rust + python, kept identical)

Both become pure relays:

- **`whoami`** → open `/identity`, send `{type:"whoami"}`, print `line`. **Delete**
  `identity_from_token` / `print_whoami`'s JWT decoding.
- **`/status`** → open `/identity`, send `{type:"status", client:{kind,version,renderMode,
  timeoutSecs,managerUrl,connected,sessionId,msgsThisSession}}`, print `block`. **Delete** the
  client-side `format_status`.
- The client keeps only: gathering its own local context (its flags + its `/chat`
  connection's session id / msg count) and printing the returned text. No identifier
  mapping, claim-picking, or block formatting.

Auth for `/identity` reuses the existing token provider (the same Bearer the client already
mints for `/chat`).

## Config

- **`MANAGER_PROJECT_NAME`** (manager env): the friendly project name (`llm-chat`). The
  provisioner already knows it — populate it into the generated env (and `.env.local`); unset
  → the manager returns the project id. Resolving `id → name` via Zitadel would be an admin
  call (crosses the manager's boundary), so config is the clean backend source.

## Error handling (fail-soft)

- `/userinfo` failure → fall back to JWT `email`/`sub`; never errors `/status`.
- `MANAGER_PROJECT_NAME` unset → project id.
- `/identity` connect failure (client side) → the client prints a clear "could not reach
  manager for status" line (the only client-side branch, and it's transport, not logic).

## Security

- `/identity` is `chat.user`-gated by the same handshake as every manager endpoint; browser
  `Origin` is rejected as elsewhere.
- The user's token is used **only** for the user's **own** `/userinfo`, within the request,
  and never logged or retained past the connection. No admin API is touched (admin-api stays
  the only Zitadel-admin caller).

## Testing

- **Manager:** unit-test the pure `format_status` / `format_whoami` renderers (given a fixed
  identity + client context → exact block) and a pure `user_label_from_userinfo(value)` /
  fallback shaper. The `/userinfo` HTTP call + the `/identity` round-trip are integration
  (gated like the existing admin IT).
- **Clients:** the client logic reduces to "send request, print response" — covered by a
  small test that the request carries the expected fields and that the printed output is the
  server `block` verbatim (mock the WS).

## Out of scope

- Caching identity across processes (in-memory `sub → name` cache only).
- Changing `/chat`/`/control` protocols (this is a separate, additive endpoint).
- Zitadel-config changes to put profile claims in the access token.

## File-by-file change list

**manager**
- `manager/src/main.rs` — capture the verified token at the handshake; add the `/identity`
  path + `handle_identity`; `resolve_user_label` (userinfo) + `sub→name` cache;
  `format_status` / `format_whoami` pure renderers; read `MANAGER_PROJECT_NAME`.

**clients (rust)**
- `clients/rust/src/repl.rs` — `/status` calls `/identity`, prints `block`; remove
  `format_status` + `identity_from_token` usage.
- `clients/rust/src/cli.rs` — `whoami` calls `/identity`, prints `line`; remove
  `identity_from_token` / local decode. A small `/identity` request helper (protocol.rs).

**clients (python)**
- `clients/python/llm_chat/repl.py` / `cli.py` — same: `/status` + `whoami` via `/identity`,
  print the server text; delete the local renderers/decoders.

**config**
- `MANAGER_PROJECT_NAME` in the compose/manager env + `.env.local(.example)` + the provisioner
  output.
