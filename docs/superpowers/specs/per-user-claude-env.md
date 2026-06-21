# Per-user Claude working environments

**Date:** 2026-06-09
**Status:** IMPLEMENTED. The confinement resolver, the worker fail-fast on
`LLM_CHAT_USER_ENV_BASE`, and the manager's reject-on-no-user-id gate all ship
in the codebase (`worker/src/user_env.rs`, `worker/src/lib.rs`,
`manager/src/main.rs`). This document records the final design.

## Summary

Every authenticated user (and each of their services) gets an isolated,
auto-created working directory that `claude` runs in. The layout is
**`{base}/{user_id}/{relative-subpath}`**:

- **`base`** — server-storage root from **`LLM_CHAT_USER_ENV_BASE`** on the
  **worker** (the component that runs `claude` and owns the filesystem).
- **`user_id`** — from the **verified Zitadel JWT** (`principal.user_id`).
  Never from the client, never anonymous.
- **`relative-subpath`** — the client's `?cwd=` value, reinterpreted as a path
  *relative* to the user's tree (first segment is the "service id", e.g.
  `crm/acct-42`; deeper segments are the client's choice).

Each `/chat` session is **confined** to its user's tree: absolute paths and
`../` traversal are rejected, and the resolved path is proven (by
canonicalization) to stay under `{base}/{user_id}`. The directory is created on
demand. This reuses the existing `?cwd` → `cmd_open` → worker-spawn plumbing;
the change is that the **manager** supplies the trusted `user_id` and the
**worker** resolves + confines + creates the path instead of trusting an
arbitrary client `cwd`.

### Goals / Non-Goals

- **Goal:** per-(user, service) isolation — a user can never read/write/run
  outside `{base}/{user_id}/`. `claude` starts in that dir, so each
  user/service has its own project context, files, and `claude` trust.
- **Non-goal:** not a quota system; not per-user OS users / chroot / containers
  (this is path confinement within the worker process, not kernel isolation).
  Not a wire-protocol change. A service is identified only by the path segment
  the client chooses — the **user** identity is the security boundary.

## Security principle (load-bearing) — fail closed, never relax

This is a security boundary and follows `CLAUDE.md` ("Fail closed — no
fallbacks, no silent defaults" / "Never relax security"). **Every**
uncertain/missing/invalid case **rejects** (no spawn). There is **no** fallback
to a shared/anonymous/default identity or directory, and no
sanitize-and-continue:

| Condition | Behavior (fail closed) |
|---|---|
| No authenticated `user_id` (shared-token mode / token without `sub`) | **Reject** `/chat` before spawn. No `_shared`, no anonymous. |
| `LLM_CHAT_USER_ENV_BASE` unset/empty | **Fail-fast at worker startup**, naming the var. |
| `user_id` not matching `^[A-Za-z0-9_-]+$` | **Reject** (`BadUser`). |
| Subpath has `..`, is absolute, or contains `\`, `:`, or NUL | **Reject** (`BadPath`) — never strip-and-continue. |
| Canonicalized path not provably under `{base}/{user_id}` (symlink/race) | **Reject** (`Escape`). |
| `canonicalize` itself errors | **Reject** — never use the un-canonicalized path. |

The only behavioral default that is **not** a security relaxation: a `/chat`
with **no** `?cwd` runs at the user's own root `{base}/{user_id}/` — still
fully confined.

**Consequence (intended):** because `user_id` is mandatory and unspoofable,
`/chat` effectively requires the Zitadel JWT path (which carries the user id).
The legacy shared-token-only mode — no per-user identity — is no longer valid
for chat; the worker refuses to spawn without a non-empty `userId`.

## Architecture & data flow

```
client ──/chat?cwd=<relative-subpath>──▶ manager
   manager: verify JWT → principal.user_id    [reject if no user_id]
            capture user_id; handle_chat(ws, state, user_id, subpath)
            cmd_open → worker: {"cmd":"open","userId":<id>,"cwd":<subpath?>}
   worker:  base = LLM_CHAT_USER_ENV_BASE (validated at startup)
            cwd = open_cwd(base, userId, subpath)   [reject on any error]
            create_dir_all(cwd); auto_trust(cwd); spawn claude with cwd
```

**Manager (`manager/src/main.rs`):** captures `principal.user_id` into a
`user_id_holder` (mirrors the existing path/query holders). After the
handshake, every authenticated path (`/chat`, `/control`, `/s/`, `/qa/`,
`/s/new`) rejects when `user_id` is `None`. `cmd_open` adds `userId` to the
`open` body; `cwd` carries the relative subpath unchanged.

**Worker (`worker/src/lib.rs`, `worker/src/user_env.rs`):** at startup resolves
`LLM_CHAT_USER_ENV_BASE` — **required, no default, fail-fast** (exits non-zero
naming the var, mirroring the bind-addr contract) and stores it in a
`OnceLock`. The `open` handler reads `userId` + `cwd` and calls `open_cwd`; any
error returns an `open` error so the manager emits an `err` frame — **no
spawn**.

## The confinement resolver (security core)

Split into a **pure** (no-I/O) part and an **I/O** part so the validation is
unit-testable without a filesystem and the canonical guard is
integration-tested.

**Pure — `confine_path(base, user_id, subpath) -> Result<PathBuf, ResolveError>`:**
- `user_id`: non-empty and `^[A-Za-z0-9_-]+$` → else `BadUser` (the regex also
  forbids `/`, `\`, `.`, `..`).
- `subpath`: `None`/`""` → user root. Otherwise split on `/`; each component
  must be non-empty, not `.`/`..`, and free of `\`, `:`, NUL; a leading `/` →
  `BadPath`. Any violation → `BadPath` (reject, never strip).
- Returns the lexical candidate `base/user_id/<components…>`. No I/O.

**I/O — `resolve_user_cwd(...)`:** `confine_path` → `create_dir_all(candidate)`
→ `canonicalize` both the candidate and `base/user_id`; if either errors →
`Escape`/reject (never use the un-canonicalized path) → assert
`real.starts_with(root)` else `Escape` (defends against symlinks/races the
lexical check can't see). Returns the lexical confined path (clean cwd for
claude); the canonical form is used only to prove confinement.

**Gate — `open_cwd(base, user_id: Option, subpath)`:** a user id is MANDATORY;
`None`/empty → `BadUser`. Otherwise `resolve_user_cwd`.

`ResolveError` is one enum (`BadUser`, `BadPath`, `Escape`, `Io(String)`) with
`Display`; the worker maps it into the `open` error surfaced to the client (no
internal paths leaked).

## Config

- **`LLM_CHAT_USER_ENV_BASE`** — worker env var, **required, fail-fast** (no
  default), via a pure unit-tested resolver (`require_user_env_base`). Set in
  the host worker launcher, the worker dev/build scripts, and the worker deploy
  env.
- **Default subpath:** absent `?cwd` → `{base}/{user_id}/` (user root, fully
  confined).

## Backward-compatibility & clients

- `?cwd` is now **relative**; absolute paths / `../` are rejected. Reference
  clients send a relative subpath (or omit it): `clients/python`,
  `clients/rust`, `clients/tauri` (see [lumina-client.md](lumina-client.md)).
- Shared-token-only chat is no longer supported (per the security principle).

## Testing (shipped in `worker/src/user_env.rs`)

- **Pure unit tests — `confine_path`:** valid nested `{user}/{service}/sub`;
  `..` traversal; absolute path; `\`/drive/NUL component; empty/`..`/bad-char
  user id; empty subpath → user root.
- **FS tests — `resolve_user_cwd`** (tempdir): dir is auto-created; a symlink
  inside the user tree pointing outside `{base}/{user_id}` is caught as
  `Escape` (unix).
- **Gate — `open_cwd`:** rejects missing/empty user id; succeeds for a valid one.
- **Config:** `require_user_env_base` rejects missing/whitespace, trims and
  accepts otherwise.
- **Manager:** `open` body carries `userId` and the relative `cwd`; `/chat`
  with no user id is rejected (no `open` sent).
