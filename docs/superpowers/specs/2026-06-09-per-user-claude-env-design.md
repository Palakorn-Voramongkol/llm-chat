# Per-user (and per-service) Claude working environments — design

**Date:** 2026-06-09
**Status:** design (brainstormed; pending user review → writing-plans)

## 1. Summary

Give every authenticated user (and each of their services) an isolated, auto-created
working directory that `claude` runs in as its base directory. The directory is laid
out as **`{base}/{user_id}/{relative-subpath}`**, where:

- **`base`** is a server-storage root, supplied by the **`LLM_CHAT_USER_ENV_BASE`** env
  var on the **worker** (the component that runs `claude` and owns the filesystem).
- **`user_id`** comes from the **verified Zitadel JWT** (`principal.user_id`) — never
  from the client, never anonymous.
- **`relative-subpath`** is the client's `?cwd=` value, reinterpreted as a *relative*
  path under the user's tree (the first segment is the "service id", e.g.
  `crm/acct-42`); deeper segments are the client's to choose.

Every `/chat` session is **confined** to its user's tree: absolute paths and `../`
traversal are rejected, and the resolved path is proven (by canonicalization) to stay
under `{base}/{user_id}`. The directory is created automatically if absent.

This reuses the existing `?cwd` → `cmd_open` → worker-spawn plumbing; the change is that
the **manager** now provides the trusted `user_id` and the **worker** resolves +
confines + creates the path instead of trusting an arbitrary client `cwd`.

## 2. Goals / Non-Goals

### Goals
- Per-(user, service) isolation: a user can never read/write/run outside `{base}/{user_id}/`.
- The base root is configured by one env var on the worker; the per-user/service tree is
  created on demand.
- `claude` starts in that directory (its base/cwd), so each user/service has its own
  project context, files, and `claude` trust.

### Non-Goals
- Not a quota / storage-accounting system, not per-user OS users or chroot/containers
  (this is path confinement within the worker process, not kernel isolation).
- Not changing the wire protocol shape (`q`/`a`/`confirm`) or the auth model — only the
  derivation of the spawn cwd.
- Not adding per-service *credentials* (a service is identified by the path segment the
  client chooses; the **user** identity is the security boundary).

## 3. Security principle (load-bearing) — fail closed, never relax

This feature is a security boundary and follows the repo rules **"Fail closed — no
fallbacks, no silent defaults"** and **"Never relax security, authentication, or
authorization"** (`CLAUDE.md`). Concretely, **every** uncertain/missing/invalid case
**rejects** (no spawn) — there is **no** fallback to a shared/anonymous/default
identity or directory, and no sanitize-and-continue:

| Condition | Behavior (fail closed) |
|---|---|
| No authenticated `user_id` (shared-token mode, or a token without `sub`) | **Reject** the `/chat` session before spawn. No `_shared`, no anonymous. |
| `LLM_CHAT_USER_ENV_BASE` unset/empty | **Fail-fast at worker startup** (the worker refuses to run), naming the var. |
| `user_id` not matching `^[A-Za-z0-9_-]+$` | **Reject** (`BadUser`). |
| Subpath contains `..`, is absolute, or has `\`, `:` (drive), or NUL | **Reject** (`BadPath`) — never strip-and-continue. |
| Canonicalized path not provably under `{base}/{user_id}` (symlink/race) | **Reject** (`Escape`). |
| `canonicalize` itself errors | **Reject** — never fall back to the unchecked path. |

The single behavioral default that is **not** a security relaxation: a `/chat` with **no**
`?cwd` runs at the user's own root `{base}/{user_id}/` — still fully confined to that
user.

**Consequence (intended):** because `user_id` is mandatory and unspoofable, `/chat`
effectively requires the **Zitadel JWT** path (which carries the user id). The legacy
shared-token-only mode — which has no per-user identity — is no longer valid for chat;
the worker refuses to spawn without a non-empty `userId`.

## 4. Architecture & data flow

```
client ──/chat?cwd=<relative-subpath>──▶ manager
   manager: verify JWT (existing) → principal.user_id   [reject if no user_id]
            capture user_id (Arc<Mutex> holder, like path/query holders)
            handle_chat(ws, state, user_id, subpath)
            cmd_open → worker `open` cmd: {"cmd":"open","userId":<id>,"cwd":<subpath?>}
   worker:  base = LLM_CHAT_USER_ENV_BASE (validated at startup)
            cwd = resolve_user_cwd(base, user_id, subpath)?   [reject on any error]
            create_dir_all(cwd); auto_trust(cwd); spawn claude with cwd
```

**Manager changes (`manager/src/main.rs`):**
- In the handshake callback, after the `chat.user` check, store `principal.user_id` into
  a `user_id_holder: Arc<Mutex<Option<String>>>` (mirrors the existing `path_holder` /
  `query_holder`).
- After `accept_hdr_async`, read the holder. For `/chat`: if `user_id` is `None`
  (shared-token mode / no identity) → send an `err` frame and close (**reject**, no
  spawn). Otherwise `handle_chat(ws, state, user_id, subpath)`.
- `cmd_open` adds `userId` to the `open` JSON body; `cwd` carries the (now relative)
  subpath unchanged.

**Worker changes (`worker/src/lib.rs`):**
- At startup, resolve `LLM_CHAT_USER_ENV_BASE` via a pure `require_env`-style helper —
  **required, no default, fail-fast** (matching `LLM_CHAT_WS_BIND`'s contract).
- On the `open` command, read `userId` + `cwd` from the body, call the resolver, create +
  trust + spawn. Any resolver error → return an error in the `open` response so the
  manager emits an `err` frame to the client; **do not spawn**.

## 5. The confinement resolver (the security core)

Split into a **pure** (no-I/O) part and an **I/O** part so the validation logic is
unit-testable without a filesystem, and the canonical guard is integration-tested.

**Pure (lexical) — `confine_path`:**
```
fn confine_path(base: &Path, user_id: &str, subpath: Option<&str>) -> Result<PathBuf, ResolveError>
```
- `user_id`: must match `^[A-Za-z0-9_-]+$` and be non-empty → else `BadUser`. (Zitadel ids
  are numeric; the regex also forbids `/`, `\`, `.`, `..`.)
- `subpath`: `None`/`""` → the user root. Otherwise split on `/`; every component must be
  non-empty and not `.`/`..` and must not contain `\`, `:`, or NUL; a leading `/`
  (absolute) → `BadPath`. Any violation → `BadPath` (reject; never strip).
- Returns the lexical candidate `base.join(user_id).join(components…)`. No I/O.

**I/O — `resolve_user_cwd`:**
```
fn resolve_user_cwd(base: &Path, user_id: &str, subpath: Option<&str>) -> Result<PathBuf, ResolveError>
```
1. `candidate = confine_path(base, user_id, subpath)?`
2. `create_dir_all(&candidate)` (so canonicalize can resolve it).
3. `real = canonicalize(&candidate)`; `root = canonicalize(base.join(user_id))`.
   If either errors → `Escape`/reject (never use the un-canonicalized path).
4. Assert `real.starts_with(&root)` → else `Escape` (defends against symlinks/races that
   the lexical check can't see).
5. Return `real`.

`ResolveError` is one enum (`BadUser`, `BadPath`, `Escape`, `Io(String)`) with a
`Display`; the worker maps it into the `open` error string surfaced to the client.

## 6. Config & defaults

- **`LLM_CHAT_USER_ENV_BASE`** — worker env var, **required, fail-fast** (no default), via
  a pure unit-tested resolver. Set in:
  - `deploy/compose/run-worker.ps1` (host worker launcher),
  - `worker/package.json` `dev`/`build` cross-env scripts (standalone runs),
  - `deploy/worker` deployment docs / service env.
- **Default subpath:** absent `?cwd` → `{base}/{user_id}/` (user root, fully confined).

## 7. Backward-compatibility & client changes

- `?cwd` is now a **relative** subpath; absolute paths / `../` are rejected. The reference
  clients are audited and updated to send a relative subpath (or omit it):
  `clients/python`, `clients/rust`, `clients/tauri` (Lumina).
- Shared-token-only chat is no longer supported (per §3) — documented in the deploy docs.

## 8. Error handling

- Resolver errors and "no user id" are surfaced to the client as a typed `err` frame with
  a clear message (no internal paths leaked). No partial spawn.
- Worker fails fast at startup if `LLM_CHAT_USER_ENV_BASE` is missing — the process exits
  non-zero naming the var.

## 9. Testing

- **Pure unit tests — `confine_path`** (no filesystem): valid nested `{user}/{service}/sub`,
  `..` traversal, absolute path, `\`/drive/NUL component, empty/`..`/bad-char user id,
  empty subpath → user root.
- **FS tests — `resolve_user_cwd`** (tempdir): the dir is `create_dir_all`'d; a symlink
  inside the user tree pointing outside `{base}/{user_id}` is caught as `Escape`.
- **Worker config:** fail-fast when `LLM_CHAT_USER_ENV_BASE` is unset (pure resolver test).
- **Manager:** `user_id` is captured and added to the `open` body; a `/chat` with no
  user id is rejected (no `open` sent).
- Existing manager + worker suites stay green.

## 10. Files touched (overview)

```
manager/src/main.rs   — user_id holder + capture; reject /chat with no user_id;
                        handle_chat(user_id, subpath); cmd_open adds "userId".
worker/src/lib.rs     — LLM_CHAT_USER_ENV_BASE required-resolver; confine_path (pure) +
                        resolve_user_cwd (I/O); open handler uses them; spawn at cwd.
deploy/compose/run-worker.ps1     — set LLM_CHAT_USER_ENV_BASE.
worker/package.json               — dev/build scripts set LLM_CHAT_USER_ENV_BASE.
deploy/worker/README.md           — document the env var + relative-cwd contract.
clients/python|rust|tauri         — send a relative subpath (or none); no absolute cwd.
CLAUDE.md                         — (added) the two security rules this design follows.
```
