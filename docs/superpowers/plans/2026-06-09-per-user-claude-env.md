# Per-user Claude Working Environment — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (- [ ]) syntax for tracking.

**Goal:** Confine every `claude` session the worker spawns to `{base}/{user_id}/{relative-subpath}` — `user_id` from the verified JWT (mandatory, no fallback), `base` from a required `LLM_CHAT_USER_ENV_BASE` worker env var, the subpath relative-only with `../`/absolute rejected and a canonical escape-guard — auto-created and fully confined.

**Architecture:** The manager (which verifies the JWT) captures `principal.user_id` and passes it plus the client's relative `?cwd` subpath to the worker in the `open` command. The worker resolves the confined absolute path with a pure-then-IO helper pair in a new focused module `worker/src/user_env.rs`, then spawns `claude` there. Fail closed at every point per `CLAUDE.md`: missing user id, missing base, traversal/absolute path, or an unprovable confinement all **reject** (no spawn) — no `_shared`, no defaults.

**Tech Stack:** Rust (worker `llm-chat` Tauri lib, manager `llm-chat-manager`), `serde_json`, `tempfile` (dev), PowerShell host launcher.

---

## Conventions

- Worker tests: `cargo test` from `D:\projects\llm-chat\worker`.
- Manager tests: `cargo test -p llm-chat-manager` from `D:\projects\llm-chat`.
- Pure helpers (no I/O) are unit-tested directly; the one filesystem helper is tested with a `tempfile::tempdir()`.
- One commit per task, conventional-commit message ending with the `Co-Authored-By` trailer.
- Spec: `docs/superpowers/specs/2026-06-09-per-user-claude-env-design.md`.

---

## Task 1: `worker/src/user_env.rs` — `ResolveError`, `require_user_env_base`, `confine_path` (pure)

**Files:**
- Create: `worker/src/user_env.rs`
- Modify: `worker/src/lib.rs` (add `mod user_env;` near the other `mod`/top declarations)
- Test: `worker/src/user_env.rs` (inline `#[cfg(test)]`)

- [ ] **Step 1: Write the failing tests** — create `worker/src/user_env.rs` with the types + stubs + tests:
  ```rust
  //! Per-user Claude working-environment path confinement (design
  //! 2026-06-09). Fail closed: any missing/invalid/unprovable input rejects.
  //! Pure helpers (no I/O) live here with the one filesystem resolver
  //! (resolve_user_cwd) so the security logic is unit-testable.

  use std::path::{Path, PathBuf};

  /// Why a per-user cwd could not be resolved. Every variant means "reject,
  /// do not spawn" — there is no fallback.
  #[derive(Debug, Clone, PartialEq, Eq)]
  pub enum ResolveError {
      /// No authenticated user id, or it fails the id charset.
      BadUser(String),
      /// The client subpath was absolute, contained `..`, or an illegal char.
      BadPath(String),
      /// The resolved path could not be proven to stay under {base}/{user_id}.
      Escape(String),
      /// A filesystem operation failed.
      Io(String),
  }

  impl std::fmt::Display for ResolveError {
      fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
          match self {
              ResolveError::BadUser(m) => write!(f, "bad user id: {m}"),
              ResolveError::BadPath(m) => write!(f, "bad path: {m}"),
              ResolveError::Escape(m) => write!(f, "path escapes user dir: {m}"),
              ResolveError::Io(m) => write!(f, "io: {m}"),
          }
      }
  }

  /// PURE: require the env base. REQUIRED — no default. Trims; Err naming the
  /// var when None/empty/whitespace. Mirrors worker_bind_addr's contract.
  pub fn require_user_env_base(raw: Option<String>) -> Result<PathBuf, String> {
      unimplemented!()
  }

  /// PURE (no I/O): validate `user_id` and the client `subpath`, and return the
  /// LEXICAL candidate `base/user_id/<components…>`. Rejects an empty/illegal
  /// user id, and any subpath that is absolute or contains `..`, `.`, an empty
  /// component, `\`, `:`, or NUL. None/empty subpath → the user root.
  pub fn confine_path(
      base: &Path,
      user_id: &str,
      subpath: Option<&str>,
  ) -> Result<PathBuf, ResolveError> {
      unimplemented!()
  }

  #[cfg(test)]
  mod tests {
      use super::*;

      #[test]
      fn require_base_rejects_missing() {
          assert!(require_user_env_base(None).unwrap_err().contains("LLM_CHAT_USER_ENV_BASE"));
          assert!(require_user_env_base(Some("   ".into())).unwrap_err().contains("LLM_CHAT_USER_ENV_BASE"));
      }
      #[test]
      fn require_base_trims_and_accepts() {
          assert_eq!(require_user_env_base(Some("  /srv/envs  ".into())).unwrap(), PathBuf::from("/srv/envs"));
      }

      fn base() -> PathBuf { PathBuf::from("/srv/envs") }

      #[test]
      fn confine_none_subpath_is_user_root() {
          assert_eq!(confine_path(&base(), "u1", None).unwrap(), PathBuf::from("/srv/envs/u1"));
          assert_eq!(confine_path(&base(), "u1", Some("")).unwrap(), PathBuf::from("/srv/envs/u1"));
      }
      #[test]
      fn confine_nested_service_subpath() {
          assert_eq!(
              confine_path(&base(), "311867081814147073", Some("crm/acct-42")).unwrap(),
              PathBuf::from("/srv/envs/311867081814147073/crm/acct-42"),
          );
      }
      #[test]
      fn confine_rejects_bad_user() {
          assert!(matches!(confine_path(&base(), "", None), Err(ResolveError::BadUser(_))));
          assert!(matches!(confine_path(&base(), "..", None), Err(ResolveError::BadUser(_))));
          assert!(matches!(confine_path(&base(), "a/b", None), Err(ResolveError::BadUser(_))));
          assert!(matches!(confine_path(&base(), "a b", None), Err(ResolveError::BadUser(_))));
      }
      #[test]
      fn confine_rejects_traversal_and_absolute() {
          assert!(matches!(confine_path(&base(), "u1", Some("../x")), Err(ResolveError::BadPath(_))));
          assert!(matches!(confine_path(&base(), "u1", Some("a/../../b")), Err(ResolveError::BadPath(_))));
          assert!(matches!(confine_path(&base(), "u1", Some("/etc")), Err(ResolveError::BadPath(_))));
          assert!(matches!(confine_path(&base(), "u1", Some("a/./b")), Err(ResolveError::BadPath(_))));
      }
      #[test]
      fn confine_rejects_windows_and_nul() {
          assert!(matches!(confine_path(&base(), "u1", Some("a\\b")), Err(ResolveError::BadPath(_))));
          assert!(matches!(confine_path(&base(), "u1", Some("C:")), Err(ResolveError::BadPath(_))));
          assert!(matches!(confine_path(&base(), "u1", Some("a\0b")), Err(ResolveError::BadPath(_))));
      }
  }
  ```
  Add `mod user_env;` to `worker/src/lib.rs` (next to the file's other top-level `mod`/`use` items).

- [ ] **Step 2: Run — expect FAIL** — `cd D:\projects\llm-chat\worker; cargo test user_env`
  Expected: the tests fail with `panicked at 'not implemented'` (both helpers are stubs).

- [ ] **Step 3: Implement** — replace the two stubs:
  ```rust
  pub fn require_user_env_base(raw: Option<String>) -> Result<PathBuf, String> {
      match raw.map(|s| s.trim().to_string()).filter(|s| !s.is_empty()) {
          Some(v) => Ok(PathBuf::from(v)),
          None => Err(
              "LLM_CHAT_USER_ENV_BASE must be set (no default) — the per-user \
               Claude environment root".to_string(),
          ),
      }
  }

  fn valid_user_id(user_id: &str) -> bool {
      !user_id.is_empty()
          && user_id.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
  }

  pub fn confine_path(
      base: &Path,
      user_id: &str,
      subpath: Option<&str>,
  ) -> Result<PathBuf, ResolveError> {
      if !valid_user_id(user_id) {
          return Err(ResolveError::BadUser(format!("{user_id:?}")));
      }
      let mut out = base.join(user_id);
      let raw = subpath.unwrap_or("").trim();
      if raw.is_empty() {
          return Ok(out);
      }
      if raw.starts_with('/') {
          return Err(ResolveError::BadPath("absolute path not allowed".into()));
      }
      for comp in raw.split('/') {
          if comp.is_empty() || comp == "." || comp == ".." {
              return Err(ResolveError::BadPath(format!("illegal component {comp:?}")));
          }
          if comp.contains('\\') || comp.contains(':') || comp.contains('\0') {
              return Err(ResolveError::BadPath(format!("illegal char in {comp:?}")));
          }
          out.push(comp);
      }
      Ok(out)
  }
  ```

- [ ] **Step 4: Run — expect PASS** — `cd D:\projects\llm-chat\worker; cargo test user_env`
  Expected: `test result: ok.` for all the new tests; `cargo build` clean.

- [ ] **Step 5: Commit**
  ```powershell
  git add worker/src/user_env.rs worker/src/lib.rs
  git commit -m @'
  feat(worker): per-user env path confinement core (require base + confine_path)

  New focused module worker/src/user_env.rs: require_user_env_base (required,
  no default) and the pure confine_path that validates the user id and the
  client subpath (rejects empty/illegal user, absolute, .., ., backslash,
  drive, NUL) and returns the lexical {base}/{user_id}/{subpath}. Fail closed.

  Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
  '@
  ```

---

## Task 2: `resolve_user_cwd` + `open_cwd` (filesystem confinement + the missing-user-id gate)

**Files:**
- Modify: `worker/src/user_env.rs` (add `resolve_user_cwd`, `open_cwd`, tests)
- Modify: `worker/Cargo.toml` (add `tempfile` to `[dev-dependencies]`)

- [ ] **Step 1: Write the failing tests** — append to `worker/src/user_env.rs`'s `#[cfg(test)] mod tests`:
  ```rust
      use std::fs;

      #[test]
      fn resolve_creates_and_returns_confined_dir() {
          let tmp = tempfile::tempdir().unwrap();
          let p = resolve_user_cwd(tmp.path(), "u1", Some("svc/a")).unwrap();
          assert!(p.is_dir(), "dir auto-created");
          assert!(p.ends_with("u1/svc/a") || p.ends_with("u1\\svc\\a"));
          // confined under {base}/u1
          assert!(p.starts_with(tmp.path().join("u1")));
      }

      #[test]
      fn resolve_rejects_traversal() {
          let tmp = tempfile::tempdir().unwrap();
          assert!(matches!(resolve_user_cwd(tmp.path(), "u1", Some("../escape")), Err(ResolveError::BadPath(_))));
      }

      #[cfg(unix)]
      #[test]
      fn resolve_rejects_symlink_escape() {
          use std::os::unix::fs::symlink;
          let tmp = tempfile::tempdir().unwrap();
          let outside = tmp.path().join("outside");
          fs::create_dir_all(&outside).unwrap();
          let user_root = tmp.path().join("u1");
          fs::create_dir_all(&user_root).unwrap();
          // a symlink INSIDE the user tree pointing OUT of it
          symlink(&outside, user_root.join("link")).unwrap();
          let err = resolve_user_cwd(tmp.path(), "u1", Some("link")).unwrap_err();
          assert!(matches!(err, ResolveError::Escape(_)), "got {err:?}");
      }

      #[test]
      fn open_cwd_rejects_missing_user() {
          let tmp = tempfile::tempdir().unwrap();
          assert!(matches!(open_cwd(tmp.path(), None, Some("svc")), Err(ResolveError::BadUser(_))));
          assert!(matches!(open_cwd(tmp.path(), Some(""), Some("svc")), Err(ResolveError::BadUser(_))));
      }

      #[test]
      fn open_cwd_ok_for_valid_user() {
          let tmp = tempfile::tempdir().unwrap();
          let p = open_cwd(tmp.path(), Some("u1"), None).unwrap();
          assert!(p.is_dir());
      }
  ```

- [ ] **Step 2: Run — expect FAIL** — `cd D:\projects\llm-chat\worker; cargo test user_env`
  Expected: compile error / `cannot find function 'resolve_user_cwd'`/`'open_cwd'` (and `tempfile` unresolved until Step 3 adds it).

- [ ] **Step 3: Implement** — add `tempfile` to `worker/Cargo.toml`:
  ```toml
  [dev-dependencies]
  tempfile = "3"
  ```
  (If `[dev-dependencies]` already exists, add the line; don't duplicate the header.)
  Then add to `worker/src/user_env.rs` (above the test module):
  ```rust
  /// Create + confine the per-user cwd, returning the LEXICAL confined path
  /// (not the verbatim canonical form, so claude gets a clean cwd). The
  /// canonical form is used only to PROVE confinement (defends against
  /// symlinks/races the lexical check can't see). Fail closed on any error.
  pub fn resolve_user_cwd(
      base: &Path,
      user_id: &str,
      subpath: Option<&str>,
  ) -> Result<PathBuf, ResolveError> {
      let candidate = confine_path(base, user_id, subpath)?;
      std::fs::create_dir_all(&candidate)
          .map_err(|e| ResolveError::Io(format!("create {}: {e}", candidate.display())))?;
      let real = candidate
          .canonicalize()
          .map_err(|e| ResolveError::Escape(format!("canonicalize candidate: {e}")))?;
      let root = base
          .join(user_id)
          .canonicalize()
          .map_err(|e| ResolveError::Escape(format!("canonicalize root: {e}")))?;
      if !real.starts_with(&root) {
          return Err(ResolveError::Escape(format!(
              "{} not under {}", real.display(), root.display()
          )));
      }
      Ok(candidate)
  }

  /// The open-command gate: a user id is MANDATORY (no fallback). None/empty →
  /// reject. Otherwise resolve + confine.
  pub fn open_cwd(
      base: &Path,
      user_id: Option<&str>,
      subpath: Option<&str>,
  ) -> Result<PathBuf, ResolveError> {
      let uid = user_id.unwrap_or("").trim();
      if uid.is_empty() {
          return Err(ResolveError::BadUser(
              "per-user environment requires an authenticated user id".into(),
          ));
      }
      resolve_user_cwd(base, uid, subpath)
  }
  ```

- [ ] **Step 4: Run — expect PASS** — `cd D:\projects\llm-chat\worker; cargo test user_env`
  Expected: all `user_env` tests pass (the `resolve_*`, `open_cwd_*`, and Task-1 tests). On Windows the `symlink_escape` test is `#[cfg(unix)]`-gated (skipped); the traversal + missing-user gates still run.

- [ ] **Step 5: Commit**
  ```powershell
  git add worker/src/user_env.rs worker/Cargo.toml
  git commit -m @'
  feat(worker): resolve_user_cwd (create+confine) + open_cwd user-id gate

  resolve_user_cwd create_dir_all's the confined path then canonicalizes to
  PROVE it stays under {base}/{user_id} (symlink/race guard), returning the
  clean lexical path. open_cwd makes the user id mandatory — None/empty is
  rejected, never a fallback. tempdir + symlink-escape tests.

  Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
  '@
  ```

---

## Task 3: Wire the worker — startup fail-fast on the base + `open` handler uses `open_cwd`

**Files:**
- Modify: `worker/src/lib.rs` (startup base validation + `open` command handler at ~2376)

- [ ] **Step 1: Add the startup fail-fast + OnceLock.** Near the top of `worker/src/lib.rs` (by the other `use`/statics), add a process-wide validated base:
  ```rust
  use std::sync::OnceLock;
  static USER_ENV_BASE: OnceLock<std::path::PathBuf> = OnceLock::new();
  ```
  In `start_ws_server` (where `worker_bind_addr` is validated with fail-fast at ~line 2127), add the same fail-fast for the base, BEFORE binding the listener:
  ```rust
      // Per-user Claude environment root — REQUIRED, no default (CLAUDE.md
      // "fail closed"). Validate once at startup; the open handler confines
      // every spawn under {base}/{user_id}.
      match crate::user_env::require_user_env_base(std::env::var("LLM_CHAT_USER_ENV_BASE").ok()) {
          Ok(base) => { let _ = USER_ENV_BASE.set(base); }
          Err(msg) => {
              tracing::error!(target: "worker", error = %msg, "cannot start");
              eprintln!("{msg}");
              std::process::exit(1);
          }
      };
  ```

- [ ] **Step 2: Change the `open` handler** at `worker/src/lib.rs:2414-2425` to resolve the confined cwd from `userId` + the (relative) `cwd` subpath. Replace:
  ```rust
                                  let cwd = req
                                      .get("cwd")
                                      .and_then(|v| v.as_str())
                                      .map(|s| s.to_string());
                                  let res = do_spawn_session(
                                      id.clone(),
                                      120,
                                      30,
                                      cwd,
                                      st_handle,
                                      &sink_ctrl,
                                  );
  ```
  with the following. The `open` arm is an expression that evaluates to a `serde_json::Value` (there is no early `return`), so the error path evaluates to an error value and the `Ok` path continues to `do_spawn_session`:
  ```rust
                                  let user_id = req.get("userId").and_then(|v| v.as_str());
                                  let subpath = req.get("cwd").and_then(|v| v.as_str());
                                  let base = USER_ENV_BASE.get().expect("validated at startup");
                                  match crate::user_env::open_cwd(base, user_id, subpath) {
                                      Err(e) => serde_json::json!({"ok":false,"error":format!("env: {e}")}),
                                      Ok(p) => {
                                          let cwd = Some(p.to_string_lossy().into_owned());
                                          match do_spawn_session(id.clone(), 120, 30, cwd, st_handle, &sink_ctrl) {
                                              Ok(_) => {
                                                  let _ = sink_ctrl.emit(
                                                      "external-session-added",
                                                      serde_json::json!({"sessionId": id}),
                                                  );
                                                  serde_json::json!({"ok":true,"sessionId":id,"transport":transport})
                                              }
                                              Err(e) => serde_json::json!({"ok":false,"error":e}),
                                          }
                                      }
                                  }
  ```
  (This replaces the original `let res = do_spawn_session(...); match res { ... }` block at lines ~2418-2435 entirely; the `open` arm now evaluates to the match above.)

- [ ] **Step 3: Build + run existing tests — expect PASS.**
  Run: `cd D:\projects\llm-chat\worker; cargo build; cargo test`
  Expected: clean build; the full worker suite (including the new `user_env` tests and the existing `ws_bind`/others) passes. Note: the open-handler wiring's logic is unit-covered by the `user_env::open_cwd` tests; this step verifies it compiles and integrates.

- [ ] **Step 4: Manual smoke (documented; not blocking the commit).** With a worker built and `LLM_CHAT_USER_ENV_BASE` set, an `open` command with `{"userId":"u1","cwd":"svc"}` creates `{base}/u1/svc` and spawns there; an `open` with no `userId` returns `{"ok":false,"error":"env: bad user id: ..."}`; starting the worker with `LLM_CHAT_USER_ENV_BASE` unset exits non-zero naming the var. (Real end-to-end is verified in Task 6's run.)

- [ ] **Step 5: Commit**
  ```powershell
  git add worker/src/lib.rs
  git commit -m @'
  feat(worker): confine every spawn under {base}/{userId} via open_cwd

  Validate LLM_CHAT_USER_ENV_BASE at startup (required, fail-fast) into a
  process OnceLock. The open command now reads userId + the relative cwd
  subpath and calls user_env::open_cwd to create+confine the dir; a missing
  user id or any resolve error replies with an error and does NOT spawn.

  Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
  '@
  ```

---

## Task 4: Manager — capture `user_id`, require it for spawns, pass it to the worker

**Files:**
- Modify: `manager/src/main.rs` (holder + capture ~1021-1070; routing ~1091-1109; `cmd_open` ~1383; the three callers ~1159/1494/1688; `handle_chat`/`handle_control`/`bridge_session_auto` signatures)
- Test: `manager/src/main.rs` (`#[cfg(test)] mod tests` — add `open_request_body`)

- [ ] **Step 1: Write the failing test** — extract the worker `open` body builder so it is unit-testable, and test it. Add to the existing `#[cfg(test)] mod tests` block in `manager/src/main.rs`:
  ```rust
      #[test]
      fn open_body_carries_user_id_and_relative_cwd() {
          let b = open_request_body("311867081814147073", Some("crm/acct-42"));
          assert_eq!(b["cmd"], "open");
          assert_eq!(b["userId"], "311867081814147073");
          assert_eq!(b["cwd"], "crm/acct-42");
      }
      #[test]
      fn open_body_omits_cwd_when_none() {
          let b = open_request_body("u1", None);
          assert_eq!(b["userId"], "u1");
          assert!(b.get("cwd").is_none());
      }
  ```

- [ ] **Step 2: Run — expect FAIL** — `cargo test -p llm-chat-manager open_body`
  Expected: `cannot find function 'open_request_body'`.

- [ ] **Step 3: Implement.**

  3a. Add the pure body builder near `cmd_open` (above line 1383):
  ```rust
  /// Build the worker `open` command body. The user id is REQUIRED (the worker
  /// confines every spawn under {base}/{userId}); the relative subpath is added
  /// only when present.
  fn open_request_body(user_id: &str, subpath: Option<&str>) -> serde_json::Value {
      let mut body = serde_json::json!({"cmd":"open","userId": user_id});
      if let Some(p) = subpath {
          body["cwd"] = serde_json::Value::String(p.to_string());
      }
      body
  }
  ```

  3b. Change `cmd_open` (lines 1383-1394) to require `user_id` and use the builder:
  ```rust
  async fn cmd_open(
      state: &SharedState,
      user_id: &str,
      subpath: Option<&str>,
  ) -> Result<(String, u16, String), Box<dyn std::error::Error + Send + Sync>> {
      let port = pick_least_loaded_port(state)
          .await
          .ok_or("no backends configured")?;
      let body = open_request_body(user_id, subpath);
      let resp = call_backend(port, body).await?;
      // … (rest unchanged: sid/transport extraction, session_to_port insert)
  ```

  3c. Capture the user id. At lines 1021-1024, add a third holder:
  ```rust
      let user_id_holder = Arc::new(std::sync::Mutex::new(None::<String>));
      let user_id_capture = user_id_holder.clone();
  ```
  Inside the handshake callback, in the Zitadel branch right before `return Ok(resp)` (after the `chat.user` check passes, ~line 1069), record the verified id:
  ```rust
              *user_id_capture.lock().unwrap() = Some(principal.user_id.clone());
              return Ok(resp);
  ```
  (The shared-token branch leaves it `None` — that is the fail-closed signal downstream.)

  3d. After the handshake (after line 1092), read it and thread it through. Replace the routing for the three SPAWNING endpoints so each rejects when there is no user id:
  ```rust
      let user_id = user_id_holder.lock().unwrap().clone();

      if req_path == "/control" {
          let uid = match user_id { Some(u) => u, None => return reject_no_user(ws).await };
          return handle_control(ws, state, uid).await;
      }
      if req_path == "/chat" {
          let uid = match user_id { Some(u) => u, None => return reject_no_user(ws).await };
          let cwd = parse_query_param(&req_query, "cwd");
          return handle_chat(ws, state, uid, cwd).await;
      }
      if req_path == "/s/new" {
          let uid = match user_id { Some(u) => u, None => return reject_no_user(ws).await };
          return bridge_session_auto(ws, state, uid).await;
      }
  ```
  Add the helper (near `handle_chat`):
  ```rust
  /// Reject a session that has no authenticated user id (fail closed — the
  /// per-user environment requires one; no fallback). Sends a typed err frame
  /// and closes.
  async fn reject_no_user(
      mut ws: tokio_tungstenite::WebSocketStream<TcpStream>,
  ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
      use futures_util::SinkExt;
      let _ = ws
          .send(tokio_tungstenite::tungstenite::Message::Text(
              serde_json::json!({"type":"err","text":"per-user environment requires an authenticated user id"}).to_string(),
          ))
          .await;
      let _ = ws.close(None).await;
      Ok(())
  }
  ```
  (Confirm the `SinkExt`/`Message` import path matches how the file already sends frames; reuse the existing pattern if different.)

  3e. Thread `user_id` into the three handlers + their `cmd_open` calls:
  - `handle_chat` (1677): add `user_id: String` param; change the `cmd_open` call (1688) to `cmd_open(&state, &user_id, cwd.as_deref())`.
  - `handle_control` (its signature): add `user_id: String`; change its `"open"` arm `cmd_open(&state, None)` (1159) to `cmd_open(&state, &user_id, None)`.
  - `bridge_session_auto` (its signature): add `user_id: String`; change its `cmd_open(&state, None)` (1494) to `cmd_open(&state, &user_id, None)`.

- [ ] **Step 4: Run — expect PASS** — from `D:\projects\llm-chat`:
  ```powershell
  cargo test -p llm-chat-manager open_body ; cargo build -p llm-chat-manager
  ```
  Expected: the two `open_body` tests pass; the manager builds clean (all three `cmd_open` callers now pass a `user_id`; the routing rejects spawns without one). Run the full `cargo test -p llm-chat-manager` to confirm the existing suite stays green.

- [ ] **Step 5: Commit**
  ```powershell
  git add manager/src/main.rs
  git commit -m @'
  feat(manager): require + pass authenticated user_id to every worker spawn

  Capture principal.user_id (a new holder, like path/query); the /chat,
  /control, and /s/new spawn routes REJECT when there is no user id (fail
  closed, no fallback). cmd_open now takes user_id + a relative subpath and
  sends them in the open body (unit-tested open_request_body). handle_chat/
  handle_control/bridge_session_auto thread the id through.

  Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
  '@
  ```

---

## Task 5: Wiring — set `LLM_CHAT_USER_ENV_BASE` for every worker launch path

**Files:**
- Modify: `deploy/compose/run-worker.ps1`
- Modify: `worker/package.json` (the `dev`/`build` cross-env scripts)
- Modify: `deploy/worker/README.md`

- [ ] **Step 1: `deploy/compose/run-worker.ps1`** — set the env before launching the worker. READ the file to find where it sets `LLM_CHAT_WS_BIND`/`LLM_CHAT_WS_PORT`, and add alongside (use a sensible host path, e.g. under the repo or a data dir):
  ```powershell
  $env:LLM_CHAT_USER_ENV_BASE = "$PSScriptRoot\..\..\.user-envs"
  ```
  (Pick the path that matches the file's existing style; the point is a writable host dir. Create the parent if the script creates other dirs.)

- [ ] **Step 2: `worker/package.json`** — the `dev`/`build` scripts already set `LLM_CHAT_WS_BIND`/`LLM_CHAT_WS_PORT` via `cross-env`. Add `LLM_CHAT_USER_ENV_BASE` so standalone runs start. READ the current scripts; change e.g.:
  ```json
  "dev":   "cross-env LLM_CHAT_WS_BIND=127.0.0.1 LLM_CHAT_WS_PORT=7878 LLM_CHAT_USER_ENV_BASE=./.user-envs tauri dev",
  "build": "cross-env LLM_CHAT_WS_BIND=127.0.0.1 LLM_CHAT_WS_PORT=7878 LLM_CHAT_USER_ENV_BASE=./.user-envs tauri build"
  ```
  (Match the existing exact values for the other two vars; only add the new one.)

- [ ] **Step 3: `deploy/worker/README.md`** — document the new required var and the relative-cwd contract: every worker process MUST set `LLM_CHAT_USER_ENV_BASE` (the per-user Claude environment root) or it exits at startup; `/chat?cwd=` is now a path RELATIVE to `{base}/{user_id}/` (absolute paths / `..` are rejected); chat now requires a Zitadel user identity (shared-token-only mode no longer spawns).

- [ ] **Step 4: Verify** — `cd D:\projects\llm-chat\worker; npm run build` (or the project's worker build) starts without the fail-fast error, i.e. the env var reaches the binary. (A full run is Task 6.) Confirm `.gitignore` ignores the chosen `.user-envs` dir (append `.user-envs/` to the repo `.gitignore` if not already covered).

- [ ] **Step 5: Commit**
  ```powershell
  git add deploy/compose/run-worker.ps1 worker/package.json deploy/worker/README.md .gitignore
  git commit -m @'
  build(worker): supply LLM_CHAT_USER_ENV_BASE on every launch path

  run-worker.ps1, the worker npm dev/build scripts, and the deploy doc now set
  the required per-user env base; document the relative-cwd contract and that
  chat requires a Zitadel user identity. Ignore the local .user-envs dir.

  Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
  '@
  ```

---

## Task 6: Audit + update the reference clients (relative `cwd` only) + end-to-end check

**Files:**
- Modify (as needed): `clients/python/llm_chat/*`, `clients/rust/*`, `clients/tauri/src/*`

- [ ] **Step 1: Audit.** Grep each client for where it sets the `/chat?cwd=` query (e.g. `cwd=` in the connect URL). For each, confirm it passes a RELATIVE subpath (or none). Commands:
  `Grep cwd in clients/python`, `clients/rust`, `clients/tauri`.
  - If a client hard-codes or forwards an ABSOLUTE path (leading `/` or a drive), change it to a relative subpath (e.g. a service name) or drop the param so the session lands at the user root.
  - If a client passes no `cwd`, no change is needed (it gets `{base}/{user_id}/`).

- [ ] **Step 2: Make the minimal edits** found in Step 1 (relative subpath or none). Keep each client's own tests green: `clients/python` → `cd D:\projects\llm-chat; python -m pytest clients/python -q`; `clients/rust` → `cargo test` in that crate; `clients/tauri` → `cd clients/tauri; npx --no-install tsc --noEmit` (npm, not pnpm).

- [ ] **Step 3: End-to-end smoke (documented runbook; gated on a running stack).** With Zitadel + manager + worker up and `LLM_CHAT_USER_ENV_BASE` set: log in as the demo user, send a `/chat` `q`, and confirm (a) `claude` answered, and (b) a directory `{base}/{demo_user_id}/…` was created on the worker host. Confirm a `?cwd=../x` is rejected with an `err` frame. (This discharges the design's confinement end-to-end; it requires the live stack so it is a runbook step, not an offline test.)

- [ ] **Step 4: Run the offline client suites** to confirm no regression (the commands in Step 2).

- [ ] **Step 5: Commit**
  ```powershell
  git add clients
  git commit -m @'
  fix(clients): send a relative /chat cwd subpath (per-user env confinement)

  Audit the python/rust/tauri reference clients so any ?cwd they send is a
  RELATIVE subpath under {base}/{user_id} (or omitted), matching the worker's
  new confinement. Absolute cwd is no longer accepted.

  Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
  '@
  ```
