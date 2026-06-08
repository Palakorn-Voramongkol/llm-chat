# Server-Side Docker Compose Stack — Implementation Plan
> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (- [ ]) syntax for tracking.

**Goal:** Ship a `docker compose up` stack (postgres + self-hosted Zitadel + one-shot provisioner + Rust manager) plus a host-native worker launcher and Python-client round-trip, all gated on a single shared OIDC issuer `http://host.docker.internal:8080`, so a client mints a `chat.user` JWT and gets an `a` answer frame back (exit 0). The three network-address env vars (`LLM_CHAT_WS_BIND`, `MANAGER_BIND`, `MANAGER_BACKEND_HOST`) are **required with no hardcoded default in the Rust code** — every address comes from env, and a missing/empty value makes the binary **fail fast** at startup with a clear error naming the var (nonzero exit). The two mode-toggle vars keep presence-based semantics: with `MANAGER_BACKEND_PORTS` unset the manager spawns local workers, and with `LLM_CHAT_AUTH_TOKEN` unset it generates a random token — byte-for-byte today's behavior.

**Architecture:** The manager validates client JWTs against Zitadel JWKS (role `chat.user`) and bridges `/chat`, `/control`, `/s/<sid>`, `/qa/<sid>`, `/` to a backend. In this stack the manager runs in a container and the **worker runs natively on Windows** (real `claude`, `~/.claude`, webview), so the manager dials the host worker at `host.docker.internal:7878` in **external-backend mode** (no spawn). The provisioner uses Zitadel's bootstrap admin key to create the `llm-chat` project, `chat.user` role, `kabytech` machine user, role grant, and JSON key, writing `./secrets/*` and `/out/manager.generated.env`. The single literal issuer `http://host.docker.internal:8080` resolves identically from host (Win32 hosts entry) and container (Docker Desktop auto-resolution), so the `iss` claim matches on both sides.

**Tech Stack:** Rust (manager `llm-chat-manager`, edition 2021; worker Tauri lib), Python 3 (`pyjwt[crypto]` + `requests` runtime; `pytest` dev/test), Docker Compose (postgres:17-alpine, zitadel pinned tag, python:3-slim, multi-stage rust:1-bookworm→debian:bookworm-slim), PowerShell host launcher.

---

## Conventions used across all tasks

- **Pure-vs-thin split (Rust):** every env read is decomposed into a *pure* helper that takes `Option<String>`/`&str` (unit-tested under `#[cfg(test)]` in the same file) plus a *thin wrapper* that calls `std::env::var(...)` and delegates to the pure helper (not unit-tested, to keep tests non-racy under cargo's parallel runner). For the three **required address** vars the pure helper returns `Result<String, String>` so the missing/empty case is a unit-testable `Err` (no hidden default); the binary turns that `Err` into a fail-fast at startup. **Helper names are fixed here and must not be renamed in later tasks. Both layers are enumerated explicitly so a worker reading only this list cannot misname a tested symbol:**
  - **Pure helpers (unit-tested):** `worker_bind_addr` (T1, `Result<String, String>` — note: an earlier `-> String` default-to-loopback version of this helper already exists in the repo and T1 *converts* it in place), `require_addr` (T2 — ONE reusable `fn require_addr(var_name: &str, raw: Option<String>) -> Result<String, String>` used by BOTH `MANAGER_BIND` (T3) and `MANAGER_BACKEND_HOST` (T2)), `parse_backend_ports` (T4, `Option<Vec<u16>>`), `resolve_auth_token` (T5, `String`).
  - **Thin wrappers (NOT unit-tested):** `backend_host` (T2 — reads `MANAGER_BACKEND_HOST`, unwraps `require_addr` via `.expect("validated at startup")`), `external_backend_ports` (T4).
  - The unit tests `use super::require_addr;` / `parse_backend_ports` / `resolve_auth_token` / `worker_bind_addr` — do **not** name a pure helper `backend_host` or the tests fail to compile.
- **`MANAGER_BACKEND_HOST` resolve-and-validate decision (spec §5.1(1)):** The spec's stated intent is that all dial/probe sites observe one consistent, *present* backend host. We satisfy this by resolving `backend_host` once in `main()` via `require_addr("MANAGER_BACKEND_HOST", …)` and **failing fast** if it is missing/empty, then threading that startup-resolved `String` into `wait_for_tcp`, `spawn_instance` (so the spawned worker inherits the required bind), and the spawn-skip log (the sites inside `main()`'s scope). The five request-time dial sites (`call_backend`, `/s/`, `/qa/`, `bridge_to_backend`, `handle_root`) are **not** in `main()`'s scope and would require threading a value through `ManagerState`; instead each calls the thin `backend_host()` wrapper inline, which reads `MANAGER_BACKEND_HOST` and unwraps `require_addr` via `.expect("validated at startup")`. **This is SAFE because `main()` already validated presence at startup** and `MANAGER_BACKEND_HOST` is immutable for the process lifetime (set by compose at launch, never mutated), so the `.expect` can never fire in a started process and every per-request read returns the identical value. We accept the per-request `std::env::var` cost (these are one-shot per WS connection, not hot loops) rather than expand scope by adding a field to `ManagerState` or changing `call_backend`'s signature. T2 records this; if the author prefers the `ManagerState` threading, that is a follow-up, not part of this plan.
- **Manager tests:** the manager currently has **no** `#[cfg(test)]` tests. Each Rust manager task adds a `#[cfg(test)] mod tests` block to `manager/src/main.rs` (append at end of file; later tasks add functions to the *same* block). Run with `cargo test -p llm-chat-manager` from `D:\projects\llm-chat`.
- **Worker tests:** run with `cargo test` from `D:\projects\llm-chat\worker`.
- **Repo is in an intermediate state (worker side):** an earlier "default-to-loopback" version of the worker bind change is already applied — `worker/src/lib.rs` already has a `fn worker_bind_addr(...) -> String` helper (line 1555) that defaults to `127.0.0.1`, the bind site at line 1660 already calls it, and a `#[cfg(test)] mod ws_bind_tests` block (line 2372) already asserts the default-to-loopback behavior. T1 therefore **converts** these in place to the required + fail-fast form rather than inserting them greenfield. Every T1 worker edit below is anchored to that actual current source.
- **Loopback literal:** all run/round-trip commands use `ws://127.0.0.1:7777/chat` (matches the client's own default at `clients/python/llm_chat_client.py:42` and the manager's `127.0.0.1` listen log, which now comes from `MANAGER_BIND=127.0.0.1` rather than a code default). `localhost` is avoided for consistency even though it resolves identically.
- **Commit discipline:** one commit per task, conventional-commit message, real `git add` of exactly the files touched. If on `main`, the executing skill creates a branch first.
- **§12 UNVERIFIED items** (the two `_search` 409-recovery endpoints, the `/auth/v1/users/me` org-id shape, the image tag, the healthcheck tool, the postgres wiring) are **never hard-coded as certain**. Each task that touches one includes an explicit verification step, and codes the clean-boot primary path (which does not exercise `_search`).

---

## Task T1 — worker `LLM_CHAT_WS_BIND` (convert existing default-loopback helper+test to required + fail-fast apply + standalone npm scripts)

**Files:** `D:\projects\llm-chat\worker\src\lib.rs`, `D:\projects\llm-chat\worker\package.json`, `D:\projects\llm-chat\README.md`, `D:\projects\llm-chat\docs\architecture.md`

- [ ] **Rewrite the existing failing test module first.** `worker/src/lib.rs` already contains a `#[cfg(test)] mod ws_bind_tests` block at line 2372 whose `defaults_to_loopback_when_none` / `defaults_to_loopback_when_empty` tests assert the OLD `None -> "127.0.0.1:7878"` default and directly contradict the new Result-returning behavior. **Do not append a second module** (that is a duplicate-module compile error). Instead REWRITE the existing block in place: delete the two `defaults_to_loopback_*` tests and replace them with `errors_when_none` / `errors_when_empty` (asserting `Err` naming `LLM_CHAT_WS_BIND`), keeping `honors_all_interfaces` and replacing `honors_specific_ip_and_port` with the `honors_loopback` case. Edit `worker/src/lib.rs` (lines 2372–2392):
  - BEFORE:
    ```rust
    #[cfg(test)]
    mod ws_bind_tests {
        use super::worker_bind_addr;

        #[test]
        fn defaults_to_loopback_when_none() {
            assert_eq!(worker_bind_addr(None, 7878), "127.0.0.1:7878");
        }
        #[test]
        fn defaults_to_loopback_when_empty() {
            assert_eq!(worker_bind_addr(Some(String::new()), 7878), "127.0.0.1:7878");
        }
        #[test]
        fn honors_all_interfaces() {
            assert_eq!(worker_bind_addr(Some("0.0.0.0".to_string()), 7878), "0.0.0.0:7878");
        }
        #[test]
        fn honors_specific_ip_and_port() {
            assert_eq!(worker_bind_addr(Some("10.0.0.5".to_string()), 9000), "10.0.0.5:9000");
        }
    }
    ```
  - AFTER:
    ```rust
    #[cfg(test)]
    mod ws_bind_tests {
        use super::worker_bind_addr;

        #[test]
        fn errors_when_none() {
            let err = worker_bind_addr(None, 7878).unwrap_err();
            assert!(err.contains("LLM_CHAT_WS_BIND"), "err names the var: {err}");
        }
        #[test]
        fn errors_when_empty() {
            let err = worker_bind_addr(Some(String::new()), 7878).unwrap_err();
            assert!(err.contains("LLM_CHAT_WS_BIND"), "err names the var: {err}");
        }
        #[test]
        fn honors_all_interfaces() {
            assert_eq!(worker_bind_addr(Some("0.0.0.0".to_string()), 7878).unwrap(),
                       "0.0.0.0:7878");
        }
        #[test]
        fn honors_loopback() {
            assert_eq!(worker_bind_addr(Some("127.0.0.1".to_string()), 7878).unwrap(),
                       "127.0.0.1:7878");
        }
    }
    ```
- [ ] **Run — expect FAIL** (the existing helper still returns `String`, so the rewritten tests do not compile):
  `cd D:\projects\llm-chat\worker; cargo test worker_bind_addr`
  Expected: a **type/compile error**, not "cannot find function" — `worker_bind_addr` already exists but returns `String`, so `worker_bind_addr(None, 7878).unwrap_err()` fails to compile (`no method named `unwrap_err` found for type `String``) and `.unwrap()` on the `Ok`-asserting tests likewise has no receiver. This confirms the helper must be converted to `Result`.
- [ ] **Convert the existing pure helper to `Result` + no default.** The helper already exists at `worker/src/lib.rs:1553–1558` with signature `-> String` and a loopback default. EDIT it in place — change the doc comment to the required/no-default wording, change the return type to `Result<String, String>`, and change the body to the None/empty -> `Err` / Some -> `Ok` form:
  - BEFORE:
    ```rust
    /// Pure: format the worker's WS bind address. `bind` is the raw value of
    /// LLM_CHAT_WS_BIND; None/empty -> loopback (today's behavior).
    fn worker_bind_addr(bind: Option<String>, port: u16) -> String {
        let host = bind.filter(|s| !s.is_empty()).unwrap_or_else(|| "127.0.0.1".to_string());
        format!("{}:{}", host, port)
    }
    ```
  - AFTER:
    ```rust
    /// Pure: format the worker's WS bind address. `bind` is the raw value of
    /// LLM_CHAT_WS_BIND. This var is REQUIRED — there is no default. None/empty
    /// yields an Err naming the var so the caller can fail fast; otherwise
    /// Ok("<host>:<port>").
    fn worker_bind_addr(bind: Option<String>, port: u16) -> Result<String, String> {
        match bind.filter(|s| !s.is_empty()) {
            Some(host) => Ok(format!("{}:{}", host, port)),
            None => Err(
                "LLM_CHAT_WS_BIND must be set (no default) — e.g. 0.0.0.0 or 127.0.0.1"
                    .to_string(),
            ),
        }
    }
    ```
  (Consistency note vs the manager's `require_addr`, which trims whitespace before the empty check: `worker_bind_addr` intentionally does NOT trim — the brief's worker test matrix is only None/empty/`0.0.0.0`/`127.0.0.1`, so trimming is out of scope here. A whitespace-only `LLM_CHAT_WS_BIND` would pass through and fail later at `TcpListener::bind`; this asymmetry is accepted and noted, not a blocker.)
- [ ] **Apply at the bind site with fail-fast.** The bind site at `worker/src/lib.rs:1660` already calls the helper but ignores the new `Result`. Convert it to a fail-fast match:
  - BEFORE (line 1660):
    ```rust
            let addr = worker_bind_addr(std::env::var("LLM_CHAT_WS_BIND").ok(), port);
    ```
  - AFTER:
    ```rust
            let addr = match worker_bind_addr(std::env::var("LLM_CHAT_WS_BIND").ok(), port) {
                Ok(addr) => addr,
                Err(msg) => {
                    // The WS server is core; there is no default bind. Fail fast.
                    tracing::error!(target: "worker", error = %msg, "cannot start WS server");
                    eprintln!("{}", msg);
                    std::process::exit(1);
                }
            };
    ```
  (The following `let listener = match TcpListener::bind(&addr).await { ... }` at line 1661 is unchanged. The pure `worker_bind_addr` stays pure — the `std::process::exit(1)` lives only in this thin call site.)
- [ ] **Run — expect PASS:** `cd D:\projects\llm-chat\worker; cargo test ws_bind` → `test result: ok. 4 passed`. (Filter by the test-module name `ws_bind`; `cargo test worker_bind_addr` matches 0 tests because the test names live under `ws_bind_tests::`, e.g. `ws_bind_tests::errors_when_none`.)
- [ ] **Full worker suite compiles and passes:** `cargo test` (from `worker/`).
- [ ] **Add standalone-dev npm scripts (so `tauri dev`/`tauri build` still get the now-required bind).** Edit `worker/package.json` to add a `cross-env` devDependency and `dev`/`build` scripts that supply `LLM_CHAT_WS_BIND` + `LLM_CHAT_WS_PORT`:
  - BEFORE:
    ```json
      "scripts": {
        "tauri": "tauri"
      },
      "devDependencies": {
        "@tauri-apps/cli": "^2"
      }
    ```
  - AFTER:
    ```json
      "scripts": {
        "tauri": "tauri",
        "dev": "cross-env LLM_CHAT_WS_BIND=127.0.0.1 LLM_CHAT_WS_PORT=7878 tauri dev",
        "build": "cross-env LLM_CHAT_WS_BIND=127.0.0.1 LLM_CHAT_WS_PORT=7878 tauri build"
      },
      "devDependencies": {
        "@tauri-apps/cli": "^2",
        "cross-env": "^7"
      }
    ```
- [ ] **Install the new devDependency:** `cd D:\projects\llm-chat\worker; npm install`
  Expected: `package-lock.json` updated, `node_modules/cross-env` present (`Test-Path .\node_modules\.bin\cross-env.cmd` → `True`).
- [ ] **Update the standalone run/build references in the top-level README.** The standalone worker is now run/built via `npm run dev` / `npm run build` (which set the required `LLM_CHAT_WS_BIND`), not bare `npm run tauri dev` / `npm run tauri build`. Edit `D:\projects\llm-chat\README.md`:
  - At line 52:
    - BEFORE: `npm run tauri dev`
    - AFTER:  `npm run dev`
  - At line 58:
    - BEFORE: `npm run tauri build`
    - AFTER:  `npm run build`
- [ ] **Update the standalone run references in the architecture doc.** `docs/architecture.md` also documents the standalone worker run with the now-stale `npm run tauri dev`. Edit `D:\projects\llm-chat\docs\architecture.md`:
  - At line 281 (the runnable code block):
    - BEFORE: `npm run tauri dev`
    - AFTER:  `npm run dev`
  - At line 108 (the prose `**Standalone (`npm run tauri dev`):**`):
    - BEFORE: `- **Standalone (`npm run tauri dev`):** a desktop terminal window you watch —`
    - AFTER:  `- **Standalone (`npm run dev`):** a desktop terminal window you watch —`
- [ ] **Confirm the binary name run-worker.ps1 depends on.** `worker/Cargo.toml` package name is `llm-chat` with no `[[bin]]` override, so `cargo build` yields `llm-chat.exe`. Verify: `cd D:\projects\llm-chat\worker; cargo build --release; Test-Path .\target\release\llm-chat.exe` → `True`. (T11's launcher hard-codes this name; this is the gate that confirms it.)
- [ ] **Commit:**
  `git add worker/src/lib.rs worker/package.json worker/package-lock.json README.md docs/architecture.md`
  `git commit` message:
  ```
  feat(worker): require LLM_CHAT_WS_BIND for the WS listener (no default)

  Convert the existing default-to-loopback worker_bind_addr(bind, port) to
  -> Result (unit-tested): it now returns Err naming LLM_CHAT_WS_BIND when
  unset/empty instead of defaulting to 127.0.0.1, and start_ws_server fails
  fast (tracing::error! + eprintln! + exit 1) because the WS server is core
  and there is no hardcoded default. Rewrite ws_bind_tests accordingly. Add
  npm `dev`/`build` scripts (cross-env) that supply LLM_CHAT_WS_BIND=127.0.0.1
  for standalone runs, and point the README + architecture-doc run/build
  steps at them.

  Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
  ```

---

## Task T2 — manager `MANAGER_BACKEND_HOST` (required addr via `require_addr` + main() fail-fast + thin wrapper + spawn_instance forwarding + host param on `wait_for_tcp` + 5 dial sites)

**File:** `D:\projects\llm-chat\manager\src\main.rs`

- [ ] **Write the failing test first.** Append (or create) at the end of `manager/src/main.rs`. These tests cover both vars that will use the shared `require_addr` helper (`MANAGER_BACKEND_HOST` here, `MANAGER_BIND` in T3):
  ```rust
  #[cfg(test)]
  mod tests {
      use super::*;

      #[test]
      fn require_addr_errors_when_none() {
          let err = require_addr("MANAGER_BACKEND_HOST", None).unwrap_err();
          assert!(err.contains("MANAGER_BACKEND_HOST"), "names the var: {err}");
      }
      #[test]
      fn require_addr_errors_when_empty() {
          let err = require_addr("MANAGER_BACKEND_HOST", Some(String::new())).unwrap_err();
          assert!(err.contains("MANAGER_BACKEND_HOST"), "names the var: {err}");
      }
      #[test]
      fn require_addr_errors_when_whitespace() {
          let err = require_addr("MANAGER_BACKEND_HOST", Some("  ".to_string())).unwrap_err();
          assert!(err.contains("MANAGER_BACKEND_HOST"), "names the var: {err}");
      }
      #[test]
      fn require_addr_honors_loopback() {
          assert_eq!(require_addr("MANAGER_BACKEND_HOST", Some("127.0.0.1".to_string())).unwrap(),
                     "127.0.0.1");
      }
      #[test]
      fn require_addr_honors_docker_host() {
          assert_eq!(require_addr("MANAGER_BACKEND_HOST",
                                  Some("host.docker.internal".to_string())).unwrap(),
                     "host.docker.internal");
      }
  }
  ```
- [ ] **Run — expect FAIL:** `cargo test -p llm-chat-manager require_addr`
  Expected: `cannot find function `require_addr` in this scope`.
- [ ] **Add the reusable pure helper + thin wrapper.** Immediately below `fn random_token()` (ends at `manager/src/main.rs:486`), insert:
  ```rust
  /// Pure, reusable: require a non-empty address env var. Trims surrounding
  /// whitespace. Returns Err(format!("{var_name} must be set (no default)")) when
  /// None/empty/whitespace-only; Ok(trimmed) otherwise. Shared by MANAGER_BIND
  /// and MANAGER_BACKEND_HOST — these are REQUIRED, there is no code default.
  fn require_addr(var_name: &str, raw: Option<String>) -> Result<String, String> {
      match raw.map(|s| s.trim().to_string()).filter(|s| !s.is_empty()) {
          Some(v) => Ok(v),
          None => Err(format!("{var_name} must be set (no default)")),
      }
  }

  /// Thin wrapper (not unit-tested): read MANAGER_BACKEND_HOST.
  ///
  /// SAFE to unwrap: main() resolves MANAGER_BACKEND_HOST via require_addr at
  /// startup and fails fast if it is missing, so by the time any request-time
  /// dial site runs the var is guaranteed present. The five request-time sites
  /// (call_backend, /s/, /qa/, bridge_to_backend, handle_root) are outside
  /// main()'s scope and call this wrapper inline per request; MANAGER_BACKEND_HOST
  /// is immutable for the process lifetime, so every read returns the same
  /// already-validated value. Accepted per-request read (one-shot, not a hot
  /// loop); see plan Conventions. No ManagerState field, no call_backend signature
  /// change.
  fn backend_host() -> String {
      require_addr("MANAGER_BACKEND_HOST", std::env::var("MANAGER_BACKEND_HOST").ok())
          .expect("validated at startup")
  }
  ```
- [ ] **Resolve once in `main()` with fail-fast.** After `manager/src/main.rs:702` (`lock_token_acl(&token_path);`) and before the `stealth` block at line 711, insert:
  ```rust
      // Resolve the backend dial host once, at startup, and FAIL FAST if it is
      // missing — there is no code default. This same String is threaded into
      // the spawn_instance forwarding, wait_for_tcp, and the spawn-skip log so
      // every site observes the one validated host.
      let backend_host = require_addr(
          "MANAGER_BACKEND_HOST",
          std::env::var("MANAGER_BACKEND_HOST").ok(),
      )?;
  ```
  (`main()` returns `Result<(), Box<dyn Error>>`, so `?` propagates the `String` error as a `Box<dyn Error>` and the process exits non-zero with the message naming the var. Cross-task note: T5 rewrites lines 699–702 *above* this insert; the two edits do not overlap — see T2-adjacency note in T5.)
- [ ] **Forward the bind to spawned workers (`spawn_instance`, `main.rs:810`).** Local-worker spawning is the NON-external/production path; the spawned worker now *requires* `LLM_CHAT_WS_BIND`, so the manager must give it the same host it dials. Thread `backend_host` into the signature and forward it:
  - BEFORE (signature, line 810):
    ```rust
  fn spawn_instance(exe: &str, port: u16, auth_token: &str, stealth: bool) -> std::io::Result<()> {
    ```
  - AFTER:
    ```rust
  fn spawn_instance(exe: &str, port: u16, auth_token: &str, backend_host: &str, stealth: bool) -> std::io::Result<()> {
    ```
  - BEFORE (env wiring, line 835):
    ```rust
      cmd.env("LLM_CHAT_WS_PORT", port.to_string())
          .env("LLM_CHAT_AUTH_TOKEN", auth_token);
    ```
  - AFTER:
    ```rust
      cmd.env("LLM_CHAT_WS_PORT", port.to_string())
          .env("LLM_CHAT_AUTH_TOKEN", auth_token)
          // The manager dials backends at backend_host, so the spawned worker
          // must bind that same host. This also supplies the worker's now-
          // required LLM_CHAT_WS_BIND with no hardcoded default.
          .env("LLM_CHAT_WS_BIND", backend_host);
    ```
  (The one caller at `main.rs:719` is updated in T4's spawn-loop rework — it passes `&backend_host`. Until T4 runs, if you build between tasks, the single existing caller at line 719 must temporarily pass `&backend_host`; the T4 rework keeps that argument.)
- [ ] **Update the existing `spawn_instance` caller to pass `backend_host` (`main.rs:719`).** (T4 reworks the surrounding loop; here only thread the host through so the build stays green between tasks.)
  - BEFORE: `        spawn_instance(&exe_path, port, &auth_token, stealth)?;`
  - AFTER:  `        spawn_instance(&exe_path, port, &auth_token, &backend_host, stealth)?;`
- [ ] **Apply at `call_backend()` (`main.rs:1294`).** No host in scope; calls the wrapper inline (validated at startup, see wrapper doc-comment):
  - BEFORE:
    ```rust
      let url = format!("ws://127.0.0.1:{}/control", port);
    ```
  - AFTER:
    ```rust
      let url = format!("ws://{}:{}/control", backend_host(), port);
    ```
- [ ] **Apply at `handle_chat()` `/s/` (`main.rs:1485`).**
  - BEFORE: `    let s_url = format!("ws://127.0.0.1:{}/s/{}", port, sid);`
  - AFTER:  `    let s_url = format!("ws://{}:{}/s/{}", backend_host(), port, sid);`
- [ ] **Apply at `handle_chat()` `/qa/` (`main.rs:1502`).**
  - BEFORE: `    let qa_url = format!("ws://127.0.0.1:{}/qa/{}", port, sid);`
  - AFTER:  `    let qa_url = format!("ws://{}:{}/qa/{}", backend_host(), port, sid);`
- [ ] **Apply at `bridge_to_backend()` (`main.rs:1946`).**
  - BEFORE: `    let url = format!("ws://127.0.0.1:{}{}{}", backend_port, base, sid);`
  - AFTER:  `    let url = format!("ws://{}:{}{}{}", backend_host(), backend_port, base, sid);`
- [ ] **Apply at `handle_root()` (`main.rs:2016`).**
  - BEFORE: `        let url = format!("ws://127.0.0.1:{}/", p);`
  - AFTER:  `        let url = format!("ws://{}:{}/", backend_host(), p);`
- [ ] **Add `host: &str` param to `wait_for_tcp` (`main.rs:868–879`).**
  - BEFORE:
    ```rust
  async fn wait_for_tcp(port: u16, retries: u32) -> Result<(), std::io::Error> {
      for _ in 0..retries {
          if TcpStream::connect(("127.0.0.1", port)).await.is_ok() {
              return Ok(());
          }
          tokio::time::sleep(std::time::Duration::from_millis(500)).await;
      }
      Err(std::io::Error::new(
          std::io::ErrorKind::TimedOut,
          format!("backend on port {} did not come up", port),
      ))
  }
    ```
  - AFTER:
    ```rust
  async fn wait_for_tcp(host: &str, port: u16, retries: u32) -> Result<(), std::io::Error> {
      for _ in 0..retries {
          if TcpStream::connect((host, port)).await.is_ok() {
              return Ok(());
          }
          tokio::time::sleep(std::time::Duration::from_millis(500)).await;
      }
      Err(std::io::Error::new(
          std::io::ErrorKind::TimedOut,
          format!("backend on {}:{} did not come up", host, port),
      ))
  }
    ```
- [ ] **Update the one existing caller (`main.rs:725`).** (T4 reworks the surrounding loop; here only thread the host through.)
  - BEFORE: `        wait_for_tcp(p, 90).await?;`
  - AFTER:  `        wait_for_tcp(&backend_host, p, 90).await?;`
- [ ] **Run — expect PASS:** `cargo test -p llm-chat-manager require_addr` → `5 passed`.
- [ ] **Build check:** `cargo build -p llm-chat-manager` succeeds with no warnings about unused `backend_host`.
- [ ] **Commit:**
  `git add manager/src/main.rs`
  `git commit` message:
  ```
  feat(manager): require MANAGER_BACKEND_HOST (no default) for backend dialing

  Reusable pure require_addr(var, raw) -> Result (unit-tested) trims and
  Errs (naming the var) on missing/empty; thin backend_host() unwraps it
  via .expect("validated at startup"). main() resolves MANAGER_BACKEND_HOST
  once and fails fast (?-propagated) if absent. wait_for_tcp now takes a
  host param; spawn_instance forwards LLM_CHAT_WS_BIND=backend_host to the
  spawned worker (so the worker's required bind matches the host the manager
  dials). The five request-time dial sites call backend_host() inline.

  Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
  ```

---

## Task T3 — manager `MANAGER_BIND` (required own listen socket + test + fail-fast apply + manager.env.example)

**Files:** `D:\projects\llm-chat\manager\src\main.rs`, `D:\projects\llm-chat\deploy\manager\manager.env.example`

- [ ] **Write the failing test first.** Add to the existing `#[cfg(test)] mod tests` block (from T2) — these exercise the SAME `require_addr` helper, now via the `MANAGER_BIND` var name:
  ```rust
      #[test]
      fn require_addr_bind_errors_when_none() {
          let err = require_addr("MANAGER_BIND", None).unwrap_err();
          assert!(err.contains("MANAGER_BIND"), "names the var: {err}");
      }
      #[test]
      fn require_addr_bind_errors_when_empty() {
          let err = require_addr("MANAGER_BIND", Some(String::new())).unwrap_err();
          assert!(err.contains("MANAGER_BIND"), "names the var: {err}");
      }
      #[test]
      fn require_addr_bind_honors_all_interfaces() {
          assert_eq!(require_addr("MANAGER_BIND", Some("0.0.0.0".to_string())).unwrap(),
                     "0.0.0.0");
      }
  ```
- [ ] **Run — expect FAIL the first time only if the helper were missing.** `require_addr` already exists from T2, so these tests should *compile and pass immediately* — that is expected (T3 reuses T2's helper, it does not add a new pure function). Verify they pass: `cargo test -p llm-chat-manager require_addr_bind` → `3 passed`. (No red phase for a new helper here; the new behavior added by T3 is the **apply + fail-fast at the listen bind**, exercised by T13.)
- [ ] **Apply at the listen bind (`main.rs:787`) + log (`main.rs:790`) with fail-fast.** Resolve `MANAGER_BIND` via `require_addr` near startup and propagate the error (fail fast — no default listen host):
  - BEFORE:
    ```rust
      let listener = TcpListener::bind(("127.0.0.1", manager_port)).await?;
      tracing::info!(
          target: "manager",
          addr = %format!("ws://127.0.0.1:{}", manager_port),
          "manager listening"
      );
    ```
  - AFTER:
    ```rust
      let bind_host = require_addr("MANAGER_BIND", std::env::var("MANAGER_BIND").ok())?;
      let listener = TcpListener::bind((bind_host.as_str(), manager_port)).await?;
      tracing::info!(
          target: "manager",
          addr = %format!("ws://{}:{}", bind_host, manager_port),
          "manager listening"
      );
    ```
  (`?` propagates the `require_addr` `String` error as `Box<dyn Error>`, so a missing/empty `MANAGER_BIND` fails fast at startup with the var-naming message and a non-zero exit.)
- [ ] **Add the required `MANAGER_BIND`/`MANAGER_BACKEND_HOST` lines to the production env example.** `deploy/manager/manager.env.example` is the non-Docker production env; it already has `MANAGER_PORT` etc. but not the now-required address vars. Production binds loopback (nginx fronts it) and dials its locally-spawned workers on loopback, so both are `127.0.0.1`. Production does **not** set `MANAGER_BACKEND_PORTS` (so the manager spawns workers), and those spawned workers receive `LLM_CHAT_WS_BIND` via the `spawn_instance` forwarding from T2.
  - BEFORE (lines 4–5):
    ```
    # Manager listen port (only nginx talks to it; bind loopback by default).
    MANAGER_PORT=7777
    ```
  - AFTER:
    ```
    # Manager listen port (only nginx talks to it).
    MANAGER_PORT=7777

    # Required network addresses (no code default — the manager fails fast if
    # either is unset). nginx fronts the manager and workers are local, so both
    # are loopback in production. MANAGER_BACKEND_PORTS is intentionally NOT set,
    # so the manager spawns local workers; each spawned worker inherits
    # LLM_CHAT_WS_BIND=MANAGER_BACKEND_HOST.
    MANAGER_BIND=127.0.0.1
    MANAGER_BACKEND_HOST=127.0.0.1
    ```
- [ ] **Run — expect PASS:** `cargo test -p llm-chat-manager require_addr` → all `require_addr`/`require_addr_bind` tests green (T2+T3). `cargo build -p llm-chat-manager` succeeds.
- [ ] **Commit:**
  `git add manager/src/main.rs deploy/manager/manager.env.example`
  `git commit` message:
  ```
  feat(manager): require MANAGER_BIND (no default) for the listen socket

  Reuse require_addr to resolve MANAGER_BIND at startup; the bind + log use
  it and main() fails fast (?-propagated, naming the var) when it is unset.
  Add MANAGER_BIND=127.0.0.1 and MANAGER_BACKEND_HOST=127.0.0.1 to the
  production manager.env.example (nginx-fronted, local-spawned workers; no
  MANAGER_BACKEND_PORTS, so workers are spawned and inherit the bind).

  Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
  ```

---

## Task T4 — manager `MANAGER_BACKEND_PORTS` external-backend mode (parse + test + skip spawn + reconcile probe)

**File:** `D:\projects\llm-chat\manager\src\main.rs`

- [ ] **Write the failing test first.** Add to the `#[cfg(test)] mod tests` block:
  ```rust
      #[test]
      fn parse_ports_none_is_none() {
          assert_eq!(parse_backend_ports(None), None);
      }
      #[test]
      fn parse_ports_empty_is_none() {
          assert_eq!(parse_backend_ports(Some(String::new())), None);
          assert_eq!(parse_backend_ports(Some("   ".to_string())), None);
      }
      #[test]
      fn parse_ports_single() {
          assert_eq!(parse_backend_ports(Some("7878".to_string())), Some(vec![7878]));
      }
      #[test]
      fn parse_ports_multi() {
          assert_eq!(parse_backend_ports(Some("7878,7879".to_string())),
                     Some(vec![7878, 7879]));
      }
      #[test]
      fn parse_ports_skips_blank_and_bad_keeps_good() {
          // " 7878 , bad , 7879 " -> trims, ignores unparseable tokens.
          assert_eq!(parse_backend_ports(Some(" 7878 , bad , 7879 ".to_string())),
                     Some(vec![7878, 7879]));
      }
      #[test]
      fn parse_ports_all_bad_is_none() {
          assert_eq!(parse_backend_ports(Some("bad,nope".to_string())), None);
      }
  ```
- [ ] **Run — expect FAIL:** `cargo test -p llm-chat-manager parse_backend_ports` → `cannot find function `parse_backend_ports``.
- [ ] **Add the pure parser + thin wrapper.** Below the `backend_host()` wrapper added in T2 (after `main.rs:486` block):
  ```rust
  /// Pure: parse a comma-separated MANAGER_BACKEND_PORTS list. Returns Some(ports)
  /// when at least one token parses to a u16; None when unset/empty/all-unparseable.
  /// PRESENCE is the mode toggle (unchanged semantics): None == "spawn local
  /// workers" (today's behavior); Some == external-backend mode. This is NOT an
  /// address value, so it is intentionally NOT made required.
  fn parse_backend_ports(raw: Option<String>) -> Option<Vec<u16>> {
      let raw = raw?;
      let ports: Vec<u16> = raw
          .split(',')
          .map(|s| s.trim())
          .filter(|s| !s.is_empty())
          .filter_map(|s| s.parse::<u16>().ok())
          .collect();
      if ports.is_empty() {
          None
      } else {
          Some(ports)
      }
  }

  /// Thin wrapper (not unit-tested): read MANAGER_BACKEND_PORTS.
  fn external_backend_ports() -> Option<Vec<u16>> {
      parse_backend_ports(std::env::var("MANAGER_BACKEND_PORTS").ok())
  }
  ```
- [ ] **Skip the spawn loop in external mode (`main.rs:716–721`).** Per spec §5.1(3) — when ports are provided, do not spawn; use the list verbatim. **Remove the entire BEFORE block (including its first line `let mut ports = Vec::new();`) and replace with the single immutable `let ports` binding below.** After this edit there is NO `let mut ports` anywhere — the outer `ports` is immutable, and downstream `for &p in &ports` (line 723–727) only reads it, so there is no leftover shadow or `unused mut` warning.
  - BEFORE:
    ```rust
      let mut ports = Vec::new();
      for i in 0..n_instances {
          let port = start_port + i as u16;
          spawn_instance(&exe_path, port, &auth_token, &backend_host, stealth)?;
          ports.push(port);
      }
    ```
  - AFTER:
    ```rust
      let ports: Vec<u16> = match external_backend_ports() {
          Some(external) => {
              tracing::info!(
                  target: "manager",
                  backend_host = %backend_host,
                  ports = ?external,
                  "external backend mode — waiting for pre-started worker(s), not spawning"
              );
              external
          }
          None => {
              let mut spawned = Vec::new();
              for i in 0..n_instances {
                  let port = start_port + i as u16;
                  spawn_instance(&exe_path, port, &auth_token, &backend_host, stealth)?;
                  spawned.push(port);
              }
              spawned
          }
      };
    ```
  (Note: `backend_host` is the startup-resolved, fail-fast-validated `String` introduced in T2; it is in scope here because the T2 insert sits between line 702 and the stealth block at line 711, i.e. *above* this loop. The `None` arm forwards it into `spawn_instance` (T2 forwarding) so each spawned worker gets its required `LLM_CHAT_WS_BIND`. The match arm's local accumulator is named `spawned` — the only `mut` — so the outer `ports` stays immutable.)
- [ ] **Reconcile the fatal probe (`main.rs:723–727`).** Keep the `?` fatal-propagation exactly as today (spec §5.1(3): ordering contract + `restart: unless-stopped` resolve it; do **not** make it non-fatal). Only thread the resolved host through (already done structurally in T2; confirm the final shape):
  - FINAL shape (after T2 + this task):
    ```rust
      tracing::info!(target: "manager", count = ports.len(), "waiting for backends");
      for &p in &ports {
          wait_for_tcp(&backend_host, p, 90).await?;
          tracing::info!(target: "manager", instance_port = p, "backend ready");
      }
    ```
- [ ] **Run — expect PASS:** `cargo test -p llm-chat-manager parse_ports` → `7 passed` (filter by the test-name prefix `parse_ports`; `parse_backend_ports` matches 0 because the tests are named `parse_ports_*`). Full `cargo test -p llm-chat-manager` green.
- [ ] **Build check:** `cargo build -p llm-chat-manager` — confirm no "unused variable `n_instances`/`start_port`/`exe_path`/`stealth`" warnings (they are still used in the `None` arm) and no "unused mut"/shadow warning on `ports`.
- [ ] **Commit:**
  `git add manager/src/main.rs`
  `git commit` message:
  ```
  feat(manager): external-backend mode via MANAGER_BACKEND_PORTS

  Pure parse_backend_ports(raw) (unit-tested: none/single/multi/whitespace/
  bad-token/all-bad). PRESENCE is the mode toggle (unchanged): unset spawns
  local workers (each forwarded the required LLM_CHAT_WS_BIND=backend_host);
  set skips spawn and treats the listed ports as pre-started backends. The
  startup wait_for_tcp probe stays fatal (resolved by the boot-ordering
  contract + restart:unless-stopped).

  Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
  ```

---

## Task T5 — manager `LLM_CHAT_AUTH_TOKEN` honoring (resolve helper + test + apply)

**File:** `D:\projects\llm-chat\manager\src\main.rs`

- [ ] **Write the failing test first.** Add to the `#[cfg(test)] mod tests` block:
  ```rust
      #[test]
      fn auth_token_uses_env_when_set() {
          let gen = || "GENERATED".to_string();
          assert_eq!(resolve_auth_token(Some("envtok".to_string()), &gen), "envtok");
      }
      #[test]
      fn auth_token_generates_when_none() {
          let gen = || "GENERATED".to_string();
          assert_eq!(resolve_auth_token(None, &gen), "GENERATED");
      }
      #[test]
      fn auth_token_generates_when_empty() {
          let gen = || "GENERATED".to_string();
          assert_eq!(resolve_auth_token(Some(String::new()), &gen), "GENERATED");
      }
  ```
- [ ] **Run — expect FAIL:** `cargo test -p llm-chat-manager resolve_auth_token` → `cannot find function `resolve_auth_token``.
- [ ] **Add the pure helper.** Below the `external_backend_ports()` wrapper:
  ```rust
  /// Pure: choose the auth token — env value if non-empty, else `gen()`.
  /// PRESENCE is the toggle (unchanged semantics, NOT an address value, NOT made
  /// required): absent/empty -> generate a random token (today's behavior);
  /// present -> use it. `gen` is injected so the parser is testable without RNG.
  fn resolve_auth_token(env: Option<String>, gen: &dyn Fn() -> String) -> String {
      env.filter(|t| !t.is_empty()).unwrap_or_else(gen)
  }
  ```
- [ ] **Apply at token generation (`main.rs:699–702`).** Keep the existing `fs::write` (line 701) and `lock_token_acl` (line 702) exactly — they persist the chosen token to disk, which is what `call_backend` reads back. **Cross-task adjacency note:** T2 has already inserted `let backend_host = require_addr("MANAGER_BACKEND_HOST", …)?;` immediately *after* `lock_token_acl(&token_path);`. This T5 edit rewrites only the four lines 699–702 *above* that insert, so the two edits do not overlap and both exact-match cleanly.
  - BEFORE:
    ```rust
      let auth_token = random_token();
      let token_path = auth_token_path();
      std::fs::write(&token_path, &auth_token)?;
      lock_token_acl(&token_path);
    ```
  - AFTER:
    ```rust
      let auth_token = resolve_auth_token(
          std::env::var("LLM_CHAT_AUTH_TOKEN").ok(),
          &random_token,
      );
      let token_path = auth_token_path();
      std::fs::write(&token_path, &auth_token)?;
      lock_token_acl(&token_path);
    ```
- [ ] **Document — no change at `call_backend` (`main.rs:1291`).** `call_backend` reads the token from the on-disk file every call (`std::fs::read_to_string(auth_token_path())?.trim()...`), **not** from `ManagerState::auth_token`. Because we honor `LLM_CHAT_AUTH_TOKEN` *before* the `fs::write` at line 701, the env value lands on disk at startup and `call_backend` reads it back on first use. No timing issue, no `ManagerState` threading needed. **Do not edit line 1291.** (Verified: `manager/src/main.rs:1291` = `let token = std::fs::read_to_string(auth_token_path())?` — leave as-is.)
- [ ] **Run — expect PASS:** `cargo test -p llm-chat-manager auth_token` → `3 passed` (filter by the test-name prefix `auth_token`; `resolve_auth_token` matches 0 because the tests are named `auth_token_*`). Full `cargo test -p llm-chat-manager` green (all T2–T5 tests).
- [ ] **Commit:**
  `git add manager/src/main.rs`
  `git commit` message:
  ```
  feat(manager): honor LLM_CHAT_AUTH_TOKEN env over a random token

  Pure resolve_auth_token(env, gen) (unit-tested) chooses env value when
  non-empty else random_token(). Presence is the toggle (unchanged): unset
  preserves today's random-token behavior. The chosen token is written to
  the same on-disk file call_backend already reads, so manager<->host-worker
  share one token.

  Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
  ```

---

## Task T6 — provisioner `provision.py` + unit tests (pure helpers + main; §4.3 sequence)

**Files (new):** `D:\projects\llm-chat\deploy\compose\provisioner\provision.py`, `D:\projects\llm-chat\deploy\compose\provisioner\test_provision.py`, `D:\projects\llm-chat\deploy\compose\provisioner\conftest.py`

- [ ] **Install test deps FIRST** (so the red phase isolates the missing `provision` symbol rather than a missing `jwt`/`requests` dependency; the host interpreter must be Python ≥3.7 — `provision.py` adds `from __future__ import annotations` so the PEP 604 `str | None` hint is a no-op string and does not require 3.10):
  `cd D:\projects\llm-chat; pip install "pyjwt[crypto]" requests pytest`
  Expected: `Successfully installed ...` (or `Requirement already satisfied`).
- [ ] **Add the conftest path shim** (test-only import shim; not enumerated in spec §6 — flag in the commit/PR as test infrastructure with no product behavior). Create `D:\projects\llm-chat\deploy\compose\provisioner\conftest.py`:
  ```python
  import os, sys
  sys.path.insert(0, os.path.dirname(__file__))
  ```
- [ ] **Write the failing tests first.** Create `D:\projects\llm-chat\deploy\compose\provisioner\test_provision.py`:
  ```python
  import base64
  import json
  import time
  from unittest import mock

  import pytest

  import provision


  def test_decode_key_details_returns_serviceaccount_dict():
      sa = {"type": "serviceaccount", "keyId": "k1", "key": "-----PEM-----",
            "userId": "u1"}
      key_details_b64 = base64.b64encode(json.dumps(sa).encode()).decode()
      assert provision.decode_key_details(key_details_b64) == sa


  def test_should_skip_keygen_when_userid_matches():
      assert provision.should_skip_keygen(existing_user_id="u1", current_user_id="u1") is True


  def test_should_regenerate_when_userid_mismatch():
      assert provision.should_skip_keygen(existing_user_id="uOLD", current_user_id="uNEW") is False


  def test_should_regenerate_when_no_existing_key():
      assert provision.should_skip_keygen(existing_user_id=None, current_user_id="u1") is False


  @pytest.mark.parametrize("status", [500, 502, 503, 401, 403])
  def test_retry_predicate_retries_on_transient(status):
      assert provision.should_retry(status=status, attempt=0) is True


  def test_retry_predicate_retries_on_connection_error():
      assert provision.should_retry(status=None, attempt=0) is True


  @pytest.mark.parametrize("status", [409, 400, 404])
  def test_retry_predicate_does_not_retry_on_deterministic(status):
      assert provision.should_retry(status=status, attempt=0) is False


  def test_retry_predicate_stops_401_after_initial_window():
      # 401/403 only retried during the initial window (attempt < INITIAL_AUTH_RETRY_ATTEMPTS)
      assert provision.should_retry(status=401, attempt=provision.INITIAL_AUTH_RETRY_ATTEMPTS) is False


  def test_build_jwt_assertion_header_and_claims():
      admin = {"type": "serviceaccount", "keyId": "kid-123", "userId": "user-456",
               "key": "PEM"}
      issuer = "http://host.docker.internal:8080"
      with mock.patch.object(provision.pyjwt, "encode") as enc:
          enc.return_value = "signed"
          out = provision.build_jwt_assertion(admin, issuer, now=1000)
      assert out == "signed"
      claims, key = enc.call_args.args[0], enc.call_args.args[1]
      assert claims["iss"] == "user-456"
      assert claims["sub"] == "user-456"
      assert claims["aud"] == issuer
      assert claims["iat"] == 1000
      assert claims["exp"] == 1000 + 3600
      assert key == "PEM"
      assert enc.call_args.kwargs["algorithm"] == "RS256"
      assert enc.call_args.kwargs["headers"] == {"kid": "kid-123"}


  def test_is_success_treats_409_as_already_provisioned():
      assert provision.is_success(200) is True
      assert provision.is_success(409) is True
      assert provision.is_success(400) is False
      assert provision.is_success(500) is False


  def test_write_generated_env_writes_project_id_and_equal_audience(tmp_path):
      out = tmp_path / "sub" / "manager.generated.env"
      with mock.patch.object(provision, "OUT_ENV_PATH", str(out)):
          provision.write_generated_env("PROJ-999")
      # §10.4: both keys defined, equal, and non-empty; exact two-line content.
      assert out.read_text() == "ZITADEL_PROJECT_ID=PROJ-999\nZITADEL_AUDIENCE=PROJ-999\n"
  ```
- [ ] **Run — expect FAIL:** `cd D:\projects\llm-chat; python -m pytest deploy/compose/provisioner/test_provision.py -v`
  Expected: collection/import error `ModuleNotFoundError: No module named 'provision'` (deps were installed in the first step, so the failure isolates the missing `provision` module — not a missing `jwt`/`requests`). The `conftest.py` makes `provision` importable once the module exists.
- [ ] **Write `provision.py`** as importable pure helpers + a `main()`. Create `D:\projects\llm-chat\deploy\compose\provisioner\provision.py`:
  ```python
  #!/usr/bin/env python3
  """Idempotent Zitadel provisioner for the llm-chat compose stack (§4.3).

  Reads the bootstrap admin key from /machinekey/zitadel-admin-sa.json, mints a
  Management-API token via the JWT-bearer flow, creates the llm-chat project,
  the chat.user role, the kabytech machine user, a JSON key, and a role grant,
  then writes ./secrets/* and /out/manager.generated.env.

  Pure helpers (unit-tested in test_provision.py) are separated from main().
  """
  from __future__ import annotations

  import base64
  import json
  import os
  import sys
  import time

  import jwt as pyjwt
  import requests

  ISSUER = os.environ.get("PROVISION_ISSUER", "http://host.docker.internal:8080")
  ADMIN_KEY_PATH = os.environ.get("ADMIN_KEY_PATH", "/machinekey/zitadel-admin-sa.json")
  SECRETS_DIR = os.environ.get("SECRETS_DIR", "/secrets")
  OUT_ENV_PATH = os.environ.get("OUT_ENV_PATH", "/out/manager.generated.env")

  PROJECT_NAME = "llm-chat"
  ROLE_KEY = "chat.user"
  MACHINE_USERNAME = "kabytech"

  MAX_ATTEMPTS = 10
  BACKOFF_SECONDS = 3
  REQUEST_TIMEOUT = 15
  INITIAL_AUTH_RETRY_ATTEMPTS = 3  # retry 401/403 only while attempt < this

  # Management-API admin scope. The literal word `zitadel` targets Zitadel's own
  # internal project so the Management API accepts the token (§4.3 scope trap).
  ADMIN_SCOPE = "openid profile urn:zitadel:iam:org:project:id:zitadel:aud"


  # ---------- pure helpers (unit-tested) ----------

  def decode_key_details(key_details_b64: str) -> dict:
      """Base64-decode the inline keyDetails -> the serviceaccount JSON dict."""
      return json.loads(base64.b64decode(key_details_b64).decode())


  def should_skip_keygen(existing_user_id, current_user_id: str) -> bool:
      """True only when an on-disk key exists AND its userId matches the user we
      just created/looked-up this run (true re-run against the same instance)."""
      return existing_user_id is not None and existing_user_id == current_user_id


  def should_retry(status, attempt: int) -> bool:
      """Retry on connection errors (status is None) and 5xx always; on 401/403
      only during the initial window. Never retry 409/400/404.

      Note: 401/403 exhaust their window at attempt == INITIAL_AUTH_RETRY_ATTEMPTS
      (3), i.e. a ~9s auth window — this is EARLIER than the 5xx path, which can
      run the full MAX_ATTEMPTS toward the ~30s ceiling. The two windows differ
      by design; do not conflate them."""
      if status is None:
          return True
      if 500 <= status < 600:
          return True
      if status in (401, 403):
          return attempt < INITIAL_AUTH_RETRY_ATTEMPTS
      return False


  def is_success(status: int) -> bool:
      """200 OK and 409 Conflict (ALREADY_EXISTS) are both 'provisioned'."""
      return status == 200 or status == 409


  def build_jwt_assertion(admin: dict, issuer: str, now: int) -> str:
      """Sign the JWT-bearer assertion with the admin key's PEM (§4.3)."""
      return pyjwt.encode(
          {"iss": admin["userId"], "sub": admin["userId"], "aud": issuer,
           "iat": now, "exp": now + 3600},
          admin["key"], algorithm="RS256",
          headers={"kid": admin["keyId"]},
      )


  # ---------- HTTP with retries ----------

  def request_with_retry(method: str, url: str, *, headers=None, data=None,
                         json_body=None) -> requests.Response:
      """Call an HTTP endpoint with the §4.3 retry policy. Returns the final
      Response; raises on exhausted retries or a non-retryable connection error.

      Wraps token mint AND each Management call (§4.3). 401/403 stop retrying
      after INITIAL_AUTH_RETRY_ATTEMPTS; 5xx/connection errors retry up to
      MAX_ATTEMPTS, after which the final Response is returned so the caller's
      raise_for_status() surfaces a persistent 5xx."""
      last_exc = None
      for attempt in range(MAX_ATTEMPTS):
          try:
              resp = requests.request(
                  method, url, headers=headers, data=data, json=json_body,
                  timeout=REQUEST_TIMEOUT,
              )
          except requests.RequestException as exc:
              last_exc = exc
              if should_retry(None, attempt) and attempt < MAX_ATTEMPTS - 1:
                  time.sleep(BACKOFF_SECONDS)
                  continue
              raise
          if should_retry(resp.status_code, attempt) and attempt < MAX_ATTEMPTS - 1:
              time.sleep(BACKOFF_SECONDS)
              continue
          return resp
      if last_exc is not None:
          raise last_exc
      raise RuntimeError(f"exhausted retries for {method} {url}")


  # ---------- impure orchestration ----------

  def load_admin_key() -> dict:
      with open(ADMIN_KEY_PATH) as f:
          return json.load(f)


  def mint_management_token(admin: dict) -> str:
      assertion = build_jwt_assertion(admin, ISSUER, int(time.time()))
      resp = request_with_retry(
          "POST", f"{ISSUER}/oauth/v2/token",
          headers={"Content-Type": "application/x-www-form-urlencoded"},
          data={
              "grant_type": "urn:ietf:params:oauth:grant-type:jwt-bearer",
              "assertion": assertion,
              "scope": ADMIN_SCOPE,
          },
      )
      resp.raise_for_status()
      return resp.json()["access_token"]


  def fetch_org_id(token: str):
      """Fetch the SA's org id via GET /auth/v1/users/me.
      UNVERIFIED (§12): the exact field is user.details.resourceOwner. If the
      shape differs against the pinned tag, return None and omit x-zitadel-orgid
      (documented SA-org fallback)."""
      try:
          resp = request_with_retry(
              "GET", f"{ISSUER}/auth/v1/users/me",
              headers={"Authorization": f"Bearer {token}"},
          )
          if resp.status_code != 200:
              return None
          body = resp.json()
          return body.get("user", {}).get("details", {}).get("resourceOwner")
      except Exception:
          return None


  def mgmt_headers(token: str, org_id):
      h = {"Authorization": f"Bearer {token}", "Content-Type": "application/json"}
      if org_id:
          h["x-zitadel-orgid"] = org_id
      return h


  def create_project(token: str, headers: dict) -> str:
      resp = request_with_retry(
          "POST", f"{ISSUER}/management/v1/projects", headers=headers,
          json_body={"name": PROJECT_NAME, "projectRoleAssertion": False,
                     "projectRoleCheck": False, "hasProjectCheck": False,
                     "privateLabelingSetting":
                         "PRIVATE_LABELING_SETTING_UNSPECIFIED"},
      )
      if resp.status_code == 200:
          return resp.json()["id"]
      if resp.status_code == 409:
          # 409 recovery via projects/_search is UNVERIFIED (§12). On the
          # clean-boot path Zitadel + ./secrets are wiped together, so this
          # branch is not exercised. Surface it loudly instead of guessing.
          raise SystemExit(
              "project already exists (409): _search recovery is UNVERIFIED "
              "(§12). On a clean reset run `docker compose down -v` AND delete "
              "./secrets so this branch is not hit.")
      resp.raise_for_status()
      raise RuntimeError(f"create_project unexpected status {resp.status_code}")


  def add_role(token: str, headers: dict, project_id: str) -> None:
      resp = request_with_retry(
          "POST", f"{ISSUER}/management/v1/projects/{project_id}/roles",
          headers=headers,
          json_body={"roleKey": ROLE_KEY, "displayName": "Chat User", "group": ""},
      )
      if not is_success(resp.status_code):
          resp.raise_for_status()


  def create_machine_user(token: str, headers: dict) -> str:
      resp = request_with_retry(
          "POST", f"{ISSUER}/management/v1/users/machine", headers=headers,
          json_body={"userName": MACHINE_USERNAME, "name": MACHINE_USERNAME,
                     "description": "llm-chat reference client",
                     "accessTokenType": "ACCESS_TOKEN_TYPE_BEARER"},
      )
      if resp.status_code == 200:
          return resp.json()["userId"]
      if resp.status_code == 409:
          # users/_search recovery is UNVERIFIED (§12); clean-boot does not hit it.
          raise SystemExit(
              "kabytech user already exists (409): _search recovery is "
              "UNVERIFIED (§12). On a clean reset run `docker compose down -v` "
              "AND delete ./secrets.")
      resp.raise_for_status()
      raise RuntimeError(f"create_machine_user unexpected status {resp.status_code}")


  def generate_json_key(token: str, headers: dict, user_id: str) -> dict:
      resp = request_with_retry(
          "POST", f"{ISSUER}/management/v1/users/{user_id}/keys", headers=headers,
          json_body={"type": "KEY_TYPE_JSON"},
      )
      resp.raise_for_status()
      return decode_key_details(resp.json()["keyDetails"])


  def grant_role(token: str, headers: dict, user_id: str, project_id: str) -> None:
      resp = request_with_retry(
          "POST", f"{ISSUER}/management/v1/users/{user_id}/grants", headers=headers,
          json_body={"projectId": project_id, "roleKeys": [ROLE_KEY]},
      )
      if not is_success(resp.status_code):
          resp.raise_for_status()


  def read_existing_user_id() -> str | None:
      path = os.path.join(SECRETS_DIR, "kabytech_user_id")
      if not os.path.exists(os.path.join(SECRETS_DIR, "kabytech-key.json")):
          return None
      if not os.path.exists(path):
          return None
      with open(path) as f:
          v = f.read().strip()
      return v or None


  def write_secret(name: str, content: str) -> None:
      os.makedirs(SECRETS_DIR, exist_ok=True)
      with open(os.path.join(SECRETS_DIR, name), "w") as f:
          f.write(content)


  def write_generated_env(project_id: str) -> None:
      os.makedirs(os.path.dirname(OUT_ENV_PATH), exist_ok=True)
      with open(OUT_ENV_PATH, "w") as f:
          f.write(f"ZITADEL_PROJECT_ID={project_id}\n")
          f.write(f"ZITADEL_AUDIENCE={project_id}\n")


  def main() -> int:
      # §4.3 strict sequence: mint token -> ensure org context -> create project,
      # role, machine user (steps 1-3) -> derive/skip key (step 4) -> grant role
      # (step 5) -> write project_id, kabytech_user_id, manager.generated.env
      # then exit 0 (step 6).
      admin = load_admin_key()
      token = mint_management_token(admin)
      org_id = fetch_org_id(token)
      headers = mgmt_headers(token, org_id)

      project_id = create_project(token, headers)
      add_role(token, headers, project_id)
      user_id = create_machine_user(token, headers)

      existing_user_id = read_existing_user_id()
      if should_skip_keygen(existing_user_id, user_id):
          print(f"[provision] key for userId={user_id} already on disk — skipping keygen")
      else:
          sa = generate_json_key(token, headers, user_id)
          write_secret("kabytech-key.json", json.dumps(sa))
          write_secret("kabytech_user_id", user_id)
          print(f"[provision] wrote kabytech-key.json for userId={user_id}")

      grant_role(token, headers, user_id, project_id)

      write_secret("project_id", project_id)
      write_secret("kabytech_user_id", user_id)
      write_generated_env(project_id)
      print(f"[provision] done: project_id={project_id} userId={user_id}")
      return 0


  if __name__ == "__main__":
      sys.exit(main())
  ```
- [ ] **Run — expect PASS:** `cd D:\projects\llm-chat; python -m pytest deploy/compose/provisioner/test_provision.py -v`
  Expected: all tests pass (`decode_key_details`, `should_skip_keygen` ×3, `should_retry` ×9 incl. parametrized, `is_success`, `build_jwt_assertion`, `write_generated_env`).
- [ ] **§12 verification note (do not skip).** The two `409` branches (`create_project`, `create_machine_user`) raise `SystemExit` rather than guess the UNVERIFIED `_search` endpoint/body. Before ever relying on re-provisioning the *same* Zitadel instance without wiping `./secrets`, **verify** `POST /management/v1/projects/_search` (`nameQuery`/`TEXT_QUERY_METHOD_EQUALS`) and `POST /management/v1/users/_search` (`userNameQuery`) against the pinned tag's API reference, and only then implement recovery. The clean-boot path (T13) wipes Zitadel + `./secrets` together and never hits these branches. Likewise `fetch_org_id` reads `user.details.resourceOwner` but **falls back to `None` (omit `x-zitadel-orgid`)** if the shape differs — the documented SA-org fallback.
- [ ] **Commit:**
  `git add deploy/compose/provisioner/provision.py deploy/compose/provisioner/test_provision.py deploy/compose/provisioner/conftest.py`
  `git commit` message:
  ```
  feat(provisioner): idempotent Zitadel provision.py + unit tests

  Pure helpers (decode_key_details, should_skip_keygen, should_retry,
  is_success, build_jwt_assertion, write_generated_env) unit-tested with
  pytest+mock. main() mints a Management-API token, creates project/role/
  machine-user/key/grant with the §4.3 retry policy and userId-aware key
  guard, writes ./secrets/* and manager.generated.env (PROJECT_ID==AUDIENCE).
  UNVERIFIED _search 409-recovery and the users/me org-id shape are flagged,
  not guessed. conftest.py is a test-only import shim (not in §6 file list).

  Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
  ```

---

## Task T7 — provisioner Dockerfile

**File (new):** `D:\projects\llm-chat\deploy\compose\provisioner\Dockerfile`

- [ ] **Write the Dockerfile.** Per §4.3: base `python:3-slim`, deps `pyjwt[crypto]` + `requests` (the `[crypto]` extra pulls `cryptography` for RS256 — the intentional, explained divergence from the approved "pyjwt/requests" list).
  ```dockerfile
  # syntax=docker/dockerfile:1
  FROM python:3-slim

  # pyjwt[crypto] = pyjwt + cryptography (required RS256 backend, see spec §4.3).
  RUN pip install --no-cache-dir "pyjwt[crypto]" requests

  WORKDIR /app
  COPY provision.py /app/provision.py

  ENTRYPOINT ["python", "/app/provision.py"]
  ```
- [ ] **Verify the image builds** (build context is the provisioner dir so `provision.py` is present; `test_provision.py`/`conftest.py` are not copied — runtime image stays lean):
  `cd D:\projects\llm-chat; docker build -f deploy/compose/provisioner/Dockerfile -t llm-chat-provisioner:test deploy/compose/provisioner`
  Expected: `naming to docker.io/library/llm-chat-provisioner:test` and exit 0.
- [ ] **Verify deps import in-image:**
  `docker run --rm --entrypoint python llm-chat-provisioner:test -c "import jwt, requests, cryptography; print('ok')"`
  Expected stdout: `ok`, exit 0.
- [ ] **Commit:**
  `git add deploy/compose/provisioner/Dockerfile`
  `git commit` message:
  ```
  build(provisioner): python:3-slim image with pyjwt[crypto]+requests

  Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
  ```

---

## Task T8 — manager.Dockerfile + entrypoint.sh (multi-stage; fail-fast assertions)

**Files (new):** `D:\projects\llm-chat\deploy\compose\manager.Dockerfile`, `D:\projects\llm-chat\deploy\compose\entrypoint.sh`

- [ ] **Write `entrypoint.sh`** exactly as §4.4 mandates (sources `/out/manager.generated.env`, asserts the three Zitadel vars non-empty, exec's the binary):
  ```sh
  #!/bin/sh
  set -e
  set -a
  . /out/manager.generated.env
  set +a
  : "${ZITADEL_ISSUER:?ZITADEL_ISSUER missing — refusing to start in shared-token mode}"
  : "${ZITADEL_PROJECT_ID:?ZITADEL_PROJECT_ID missing from manager.generated.env}"
  : "${ZITADEL_AUDIENCE:?ZITADEL_AUDIENCE missing from manager.generated.env}"
  exec /usr/local/bin/llm-chat-manager
  ```
  (LF line endings — it runs under `/bin/sh` in the Linux image. If editing on Windows, ensure no CRLF.)
- [ ] **Ensure a committed `manager/Cargo.lock` exists** (reproducible container build == local `cargo test` build). If absent, generate and commit it with this task: `cd D:\projects\llm-chat\manager; cargo generate-lockfile; Test-Path .\Cargo.lock` → `True`. The Dockerfile copies it **non-optionally** so the released binary resolves the identical dependency versions T2–T5 tested against.
- [ ] **Write `manager.Dockerfile`** (multi-stage per §4.4; build context = repo root so `./manager` is in scope):
  ```dockerfile
  # syntax=docker/dockerfile:1
  FROM rust:1-bookworm AS build
  WORKDIR /src
  COPY manager/Cargo.toml manager/Cargo.lock ./manager/
  COPY manager/src ./manager/src
  RUN cargo build --release --locked --manifest-path manager/Cargo.toml

  FROM debian:bookworm-slim
  RUN apt-get update \
      && apt-get install -y --no-install-recommends ca-certificates \
      && rm -rf /var/lib/apt/lists/*
  COPY --from=build /src/manager/target/release/llm-chat-manager /usr/local/bin/llm-chat-manager
  COPY deploy/compose/entrypoint.sh /usr/local/bin/entrypoint.sh
  RUN chmod +x /usr/local/bin/entrypoint.sh
  ENTRYPOINT ["/usr/local/bin/entrypoint.sh"]
  ```
  (`Cargo.lock` is copied non-optionally and the build uses `--locked` so the container build is byte-reproducible against the committed lockfile — no floating-dep divergence from the local `cargo test` runs.)
- [ ] **Verify the image builds:**
  `cd D:\projects\llm-chat; docker build -f deploy/compose/manager.Dockerfile -t llm-chat-manager:test .`
  Expected: build succeeds, `naming to docker.io/library/llm-chat-manager:test`, exit 0.
- [ ] **Verify fail-fast (no generated env → exit 1).** Run the image with an **empty** `/out` (no `manager.generated.env`):
  `New-Item -ItemType Directory -Force deploy/compose/_emptyout | Out-Null`
  `docker run --rm -v ${PWD}/deploy/compose/_emptyout:/out llm-chat-manager:test; echo "exit=$LASTEXITCODE"`
  Expected: the entrypoint's `. /out/manager.generated.env` fails under `set -e` (file absent) → **nonzero exit**; `exit=` shows a nonzero code. Clean up: `Remove-Item -Recurse -Force deploy/compose/_emptyout`.
- [ ] **Verify fail-fast (partial generated env → `ZITADEL_ISSUER missing`).** Create a generated env missing `ZITADEL_ISSUER`:
  - `New-Item -ItemType Directory -Force deploy/compose/_partialout | Out-Null`
  - Write `deploy/compose/_partialout/manager.generated.env` with only:
    ```
    ZITADEL_PROJECT_ID=123
    ZITADEL_AUDIENCE=123
    ```
  - `docker run --rm -v ${PWD}/deploy/compose/_partialout:/out llm-chat-manager:test; echo "exit=$LASTEXITCODE"`
  Expected stderr contains `ZITADEL_ISSUER missing — refusing to start in shared-token mode`, nonzero exit. Clean up: `Remove-Item -Recurse -Force deploy/compose/_partialout`.
  (`ZITADEL_ISSUER` comes from compose `.env`, not the generated file, so at runtime in compose it *is* present; this test isolates the entrypoint assertion by running the image standalone without it. NOTE: this standalone run does not set `MANAGER_BIND`/`MANAGER_BACKEND_HOST`, so even past the entrypoint asserts the Rust binary would fail fast on those required vars — the entrypoint assertion is what this step isolates, and it fires first.)
- [ ] **Commit:**
  `git add deploy/compose/manager.Dockerfile deploy/compose/entrypoint.sh manager/Cargo.lock`
  `git commit` message:
  ```
  build(manager): multi-stage Dockerfile + fail-fast entrypoint

  rust:1-bookworm build of ./manager into debian:bookworm-slim + ca-certs.
  Cargo.lock committed and copied non-optionally with --locked so the
  container build matches the locally tested dep versions. entrypoint.sh
  sources /out/manager.generated.env and asserts ZITADEL_ISSUER/PROJECT_ID
  /AUDIENCE are non-empty (exit 1) so a partial generated env fails visibly
  instead of silently degrading to shared-token auth.

  Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
  ```

---

## Task T9 — `.env.example` + `.dockerignore` + `.gitignore` edit

**Files:** new `D:\projects\llm-chat\.env.example`, new `D:\projects\llm-chat\.dockerignore`; edit `D:\projects\llm-chat\.gitignore`

- [ ] **Write `.env.example`** (insecure local-dev placeholders, per §2/§9):
  ```dotenv
  # llm-chat compose stack — LOCAL DEV ONLY. HTTP issuer, no TLS. Never expose.
  # Copy to .env and fill in. `.env` is gitignored by the existing *.env rule.

  # Exactly 32 chars; one-shot — cannot change after first Zitadel init.
  #   generate: openssl rand -hex 16
  ZITADEL_MASTERKEY=changeme-32-characters-exactly!!

  # Postgres password (Zitadel's backing DB only).
  POSTGRES_PASSWORD=changeme-strong-password

  # Shared token for manager<->host-worker auth.
  #   generate: openssl rand -hex 32
  LLM_CHAT_AUTH_TOKEN=changeme-openssl-rand-hex-32
  ```
- [ ] **Write `.dockerignore`** (keeps build context small; excludes secrets from any image build, §6). The `worker/` entries are inert defense-in-depth (the manager build context COPYs only `manager/*`, never `worker/`); kept so a future worker image build cannot leak its target/node_modules:
  ```gitignore
  .git/
  node_modules/
  worker/target/
  worker/node_modules/
  manager/target/
  secrets/
  *.log
  .env
  ```
- [ ] **Edit `.gitignore`.** The current file is exactly seven lines (`node_modules/`, `worker/target/`, `worker/node_modules/`, `manager/target/`, `*.log`, `*.env`, `.claude/`) and does **not** ignore `secrets/`. Append one line:
  - BEFORE (last line):
    ```
    .claude/
    ```
  - AFTER:
    ```
    .claude/
    secrets/
    ```
- [ ] **Verify `secrets/` is now ignored and `.env.example` is still tracked:**
  - `cd D:\projects\llm-chat; New-Item -ItemType Directory -Force secrets | Out-Null; New-Item -ItemType File -Force secrets/probe | Out-Null`
  - `git check-ignore secrets/probe` → prints `secrets/probe` (ignored), exit 0.
  - `git check-ignore .env.example` → prints nothing, exit 1 (NOT ignored — `*.env` does not match `.env.example`). Confirm with `git status --porcelain .env.example` showing it as untracked/added.
  - Clean up the probe: `Remove-Item secrets/probe`.
- [ ] **Commit:**
  `git add .env.example .dockerignore .gitignore`
  `git commit` message:
  ```
  chore: add .env.example/.dockerignore, gitignore secrets/

  Append secrets/ to .gitignore so the live kabytech RSA private key can
  never be committed; .env.example ships insecure local-dev placeholders
  (not matched by *.env, stays committable). .dockerignore keeps build
  context small and keeps secrets out of images.

  Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
  ```

---

## Task T10 — `docker-compose.yml` (4 services, 3 volumes, ./secrets bind, healthchecks, depends_on)

**File (new):** `D:\projects\llm-chat\docker-compose.yml`

- [ ] **Write `docker-compose.yml`.** Encodes §4.1–§4.4 + §7.1. Postgres wiring (B) per §4.1 (flagged as an implementation choice; fallback to (A) is acceptable — see verification step). Zitadel image tag and healthcheck tool are **to-be-confirmed pins** (§12), not asserted certain. The manager env sets all three required address vars explicitly (`MANAGER_BIND`, `MANAGER_BACKEND_HOST`) plus the external-mode toggle `MANAGER_BACKEND_PORTS`.
  ```yaml
  services:
    postgres:
      image: postgres:17-alpine
      environment:
        POSTGRES_USER: postgres
        POSTGRES_PASSWORD: ${POSTGRES_PASSWORD}
        POSTGRES_DB: postgres
      volumes:
        - pgdata:/var/lib/postgresql/data
      healthcheck:
        test: ["CMD-SHELL", "pg_isready -U postgres -d postgres"]
        interval: 5s
        timeout: 5s
        retries: 20
        start_period: 10s
      restart: unless-stopped

    zitadel:
      # PINNED, TO BE CONFIRMED (§4.2/§12): verify this tag exposes the v1
      # Management-API endpoints provision.py calls; never use :latest.
      image: ghcr.io/zitadel/zitadel:v3.4.10
      command: 'start-from-init --masterkeyFromEnv --tlsMode disabled'
      environment:
        ZITADEL_MASTERKEY: ${ZITADEL_MASTERKEY}
        ZITADEL_EXTERNALDOMAIN: host.docker.internal
        ZITADEL_EXTERNALPORT: 8080
        ZITADEL_EXTERNALSECURE: "false"
        # Postgres wiring (B) — discrete admin+user split (§4.1; fallback to (A)).
        ZITADEL_DATABASE_POSTGRES_HOST: postgres
        ZITADEL_DATABASE_POSTGRES_PORT: 5432
        ZITADEL_DATABASE_POSTGRES_DATABASE: zitadel
        ZITADEL_DATABASE_POSTGRES_USER_USERNAME: zitadel_user
        ZITADEL_DATABASE_POSTGRES_USER_PASSWORD: ${POSTGRES_PASSWORD}
        ZITADEL_DATABASE_POSTGRES_USER_SSL_MODE: disable
        ZITADEL_DATABASE_POSTGRES_ADMIN_USERNAME: postgres
        ZITADEL_DATABASE_POSTGRES_ADMIN_PASSWORD: ${POSTGRES_PASSWORD}
        ZITADEL_DATABASE_POSTGRES_ADMIN_SSL_MODE: disable
        # Bootstrap admin SA — JSON key written once to the machinekey volume.
        ZITADEL_FIRSTINSTANCE_ORG_MACHINE_MACHINE_USERNAME: zitadel-admin-sa
        ZITADEL_FIRSTINSTANCE_ORG_MACHINE_MACHINE_NAME: Admin
        ZITADEL_FIRSTINSTANCE_ORG_MACHINE_MACHINEKEY_TYPE: 1
        ZITADEL_FIRSTINSTANCE_MACHINEKEYPATH: /machinekey/zitadel-admin-sa.json
      ports:
        - "8080:8080"
      depends_on:
        postgres:
          condition: service_healthy
      healthcheck:
        # /debug/healthz is fixed by the approved design; the TOOL is the only
        # open variable (§4.2/§11/§12). If the pinned image ships neither wget
        # nor curl, fall back to ["CMD","/app/zitadel","ready"] and verify it
        # empirically (issue #9495: `ready` has attempted HTTPS even with TLS off).
        test: ["CMD-SHELL", "wget -qO- http://localhost:8080/debug/healthz || exit 1"]
        interval: 5s
        timeout: 5s
        retries: 30
        start_period: 30s
      volumes:
        - machinekey:/machinekey
      restart: unless-stopped

    zitadel-init:
      build:
        context: ./deploy/compose/provisioner
        dockerfile: Dockerfile
      environment:
        PROVISION_ISSUER: http://host.docker.internal:8080
      depends_on:
        zitadel:
          condition: service_healthy
      volumes:
        - machinekey:/machinekey:ro
        - ./secrets:/secrets
        - genenv:/out
      restart: "no"

    manager:
      build:
        context: .
        dockerfile: deploy/compose/manager.Dockerfile
      environment:
        ZITADEL_ISSUER: http://host.docker.internal:8080
        LLM_CHAT_AUTH_TOKEN: ${LLM_CHAT_AUTH_TOKEN}
        MANAGER_BIND: 0.0.0.0
        MANAGER_BACKEND_HOST: host.docker.internal
        MANAGER_BACKEND_PORTS: "7878"
      ports:
        - "7777:7777"
      depends_on:
        zitadel-init:
          condition: service_completed_successfully
      volumes:
        - genenv:/out:ro
      restart: unless-stopped

  volumes:
    pgdata:
    machinekey:
    genenv:
  ```
- [ ] **Verify compose parses:** `cd D:\projects\llm-chat; docker compose config --quiet; echo "exit=$LASTEXITCODE"`
  Expected: no output, `exit=0`. (Requires `.env` to exist; if only running the lint, a `.env` copied from `.env.example` satisfies variable interpolation.)
- [ ] **Verify the resolved model shows everything (§10.1):** `docker compose config`
  Confirm in the rendered output: the **4 services** (`postgres`, `zitadel`, `zitadel-init`, `manager`); the **3 named volumes** (`pgdata`, `machinekey`, `genenv`); the **`./secrets` bind** on `zitadel-init`; the **healthchecks** on `postgres` and `zitadel` (none on manager); the **`depends_on` conditions** (`postgres: service_healthy`, `zitadel: service_healthy`, `zitadel-init: service_completed_successfully`); the **ports** `8080:8080` and `7777:7777` (all-interfaces, NOT `127.0.0.1:`); the **restart policies** (`unless-stopped` on postgres/zitadel/manager, `"no"` on zitadel-init); and the manager `MANAGER_*` env (`MANAGER_BIND=0.0.0.0`, `MANAGER_BACKEND_HOST=host.docker.internal`, `MANAGER_BACKEND_PORTS=7878` — the three address vars are present so the manager will not fail fast).
- [ ] **§12 verification items (record, do not assume certain).** Each item below has a check AND a committed-fallback action under THIS task's scope (root-cause edits, not dirty fixes):
  - **Image tag:** before first `up`, run `docker manifest inspect ghcr.io/zitadel/zitadel:v3.4.10 > $null; echo $LASTEXITCODE` (0 = tag exists). At bring-up, confirm the v1 Management endpoints exist (provisioner round-trip in T13 is the empirical check). **If the tag is wrong/missing, edit `docker-compose.yml` to a different concrete version (never `:latest`), re-verify, and amend/extend this T10 commit.**
  - **Healthcheck tool:** after `zitadel` starts, `docker compose exec zitadel sh -c "command -v wget || command -v curl || echo NONE"`. **If `NONE`, edit `docker-compose.yml` to switch the healthcheck `test` to `["CMD","/app/zitadel","ready"]`, empirically confirm it reports healthy on this HTTP deployment (issue #9495 caveat), and commit that change under T10** — a real fix, not a manual one-off swap. (T13 re-checks this end-to-end.)
  - **TLS-disable flag:** if `start-from-init --masterkeyFromEnv --tlsMode disabled` is rejected by this tag, edit the `command`/env to the minimal form the tag honors (e.g. `ZITADEL_TLS_ENABLED=false`), re-verify, and commit under T10.
  - **Postgres wiring:** if wiring (B) (`zitadel_user` auto-create) errors at init, edit `docker-compose.yml` to fall back to wiring (A) — drop the `_USER_*` discrete fields and connect Zitadel as `postgres` into a pre-created `zitadel` DB — re-verify, and commit under T10. Client-facing behavior is unchanged.
- [ ] **Commit:**
  `git add docker-compose.yml`
  `git commit` message:
  ```
  build(compose): 4-service stack (postgres/zitadel/init/manager)

  Healthcheck-gated boot ordering via depends_on conditions; pgdata/
  machinekey/genenv volumes; ./secrets bind; 8080/7777 published all-
  interfaces (host.docker.internal must reach them); manager in external-
  backend mode with all three required address vars set explicitly
  (MANAGER_BIND=0.0.0.0, MANAGER_BACKEND_HOST=host.docker.internal,
  MANAGER_BACKEND_PORTS=7878). Image tag, healthcheck tool, TLS flag and
  postgres wiring are flagged to-confirm per §12, each with a committed
  fallback edit if the check fails.

  Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
  ```

---

## Task T11 — `run-worker.ps1` (host worker launcher)

**File (new):** `D:\projects\llm-chat\deploy\compose\run-worker.ps1`

- [ ] **Write `run-worker.ps1`.** Reads `LLM_CHAT_AUTH_TOKEN` from `.env` (or `-Token` param), sets `LLM_CHAT_WS_PORT=7878` and `LLM_CHAT_WS_BIND=0.0.0.0` (the worker now *requires* this — the script always provides it), launches the worker, warns about the firewall prompt (§4.5/§6). **The script is structured so its functions are dot-source-safe:** the launch tail runs only when the script is executed (`$MyInvocation.InvocationName -ne '.'`), so a test can dot-source it to call `Read-DotEnvValue` without launching the GUI worker.
  ```powershell
  #requires -Version 5.1
  <#
  .SYNOPSIS
    Launch the native Windows worker for the llm-chat compose stack.
    Binds 0.0.0.0:7878 with the shared token so the manager container can
    reach it via host.docker.internal:7878. Start this BEFORE `docker compose up`
    (the manager's startup probe of :7878 is fatal — spec §7.1).
  #>
  [CmdletBinding()]
  param(
      [string]$Token,
      [int]$Port = 7878,
      [string]$Bind = "0.0.0.0",
      [string]$EnvFile = (Join-Path $PSScriptRoot "..\..\.env"),
      [string]$WorkerExe
  )

  $ErrorActionPreference = "Stop"

  function Read-DotEnvValue([string]$Path, [string]$Key) {
      if (-not (Test-Path $Path)) { return $null }
      foreach ($line in Get-Content -LiteralPath $Path) {
          $t = $line.Trim()
          if ($t -eq "" -or $t.StartsWith("#")) { continue }
          $eq = $t.IndexOf("=")
          if ($eq -lt 1) { continue }
          $k = $t.Substring(0, $eq).Trim()
          if ($k -eq $Key) {
              return $t.Substring($eq + 1).Trim().Trim('"')
          }
      }
      return $null
  }

  function Resolve-WorkerExe([string]$RepoRoot, [string]$Override) {
      if ($Override) { return $Override }
      $release = Join-Path $RepoRoot "worker\target\release\llm-chat.exe"
      $debug   = Join-Path $RepoRoot "worker\target\debug\llm-chat.exe"
      if (Test-Path $release) { return $release }
      if (Test-Path $debug)   { return $debug }
      throw "worker binary not found. Build it first (cargo build --release in worker/) or pass -WorkerExe."
  }

  function Invoke-RunWorker {
      param([string]$Token, [int]$Port, [string]$Bind, [string]$EnvFile, [string]$WorkerExe)

      if (-not $Token) { $Token = Read-DotEnvValue -Path $EnvFile -Key "LLM_CHAT_AUTH_TOKEN" }
      if (-not $Token) {
          throw "LLM_CHAT_AUTH_TOKEN not provided (-Token) and not found in $EnvFile. " +
                "Copy .env.example to .env and set it (openssl rand -hex 32)."
      }

      $repoRoot = Resolve-Path (Join-Path $PSScriptRoot "..\..")
      $exe = Resolve-WorkerExe -RepoRoot $repoRoot -Override $WorkerExe

      Write-Host "[run-worker] worker  = $exe"
      Write-Host "[run-worker] bind    = ${Bind}:${Port}"
      Write-Host "[run-worker] token   = (len=$($Token.Length))"
      Write-Host "[run-worker] NOTE: Windows Defender Firewall may prompt for the 0.0.0.0 bind."
      Write-Host "[run-worker]       Approve it (PRIVATE networks only) or the manager cannot reach :$Port."

      $env:LLM_CHAT_AUTH_TOKEN = $Token
      $env:LLM_CHAT_WS_PORT     = "$Port"
      $env:LLM_CHAT_WS_BIND     = $Bind

      # Foreground/blocking by design: holds the session for the GUI worker's
      # lifetime so Ctrl-C stops the worker.
      & $exe
  }

  # Only run the launch tail when executed directly, NOT when dot-sourced
  # (`. .\run-worker.ps1`). Dot-sourcing exposes the functions for testing
  # without launching the windowless GUI worker.
  if ($MyInvocation.InvocationName -ne '.') {
      Invoke-RunWorker -Token $Token -Port $Port -Bind $Bind -EnvFile $EnvFile -WorkerExe $WorkerExe
  }
  ```
- [ ] **Verify it errors cleanly without a token (no `.env`, no `-Token`)** — exercises the launch path's param/dotenv plumbing without reaching `& $exe` (it throws first):
  `cd D:\projects\llm-chat; pwsh -NoProfile -Command "& ./deploy/compose/run-worker.ps1 -EnvFile ./does-not-exist.env"; echo \"exit=$LASTEXITCODE\""`
  Expected: throws `LLM_CHAT_AUTH_TOKEN not provided ...`, `exit=1`.
- [ ] **Verify `Read-DotEnvValue` in isolation by dot-sourcing (no launch).** Concrete, runnable harness — writes a temp `.env`, dot-sources the script (the `InvocationName -eq '.'` guard suppresses the launch tail), calls the function, asserts the value:
  `cd D:\projects\llm-chat; pwsh -NoProfile -Command "Set-Content -Path $env:TEMP\rw.env -Value 'LLM_CHAT_AUTH_TOKEN=abc123'; . ./deploy/compose/run-worker.ps1; if ((Read-DotEnvValue $env:TEMP\rw.env 'LLM_CHAT_AUTH_TOKEN') -ne 'abc123') { throw 'FAIL' }; 'ok'"`
  Expected stdout: `ok`, exit 0. (Confirms the dotenv read logic and that dot-sourcing does NOT launch the GUI worker.) Clean up: `Remove-Item $env:TEMP\rw.env`.
- [ ] **Commit:**
  `git add deploy/compose/run-worker.ps1`
  `git commit` message:
  ```
  feat(host): run-worker.ps1 launches native worker on 0.0.0.0:7878

  Reads LLM_CHAT_AUTH_TOKEN from .env (or -Token), sets LLM_CHAT_WS_PORT/
  LLM_CHAT_WS_BIND (the worker requires the bind — the script always
  supplies it), launches the worker (foreground/blocking by design),
  warns about the Windows Firewall prompt. Functions are dot-source-safe
  (launch tail guarded by InvocationName) so Read-DotEnvValue is unit-
  testable without launching the GUI. Start before `docker compose up`
  (manager probe of :7878 is fatal).

  Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
  ```

---

## Task T12 — `deploy/compose/README.md` (run steps §9, fallback §8, risks §11)

**File (new):** `D:\projects\llm-chat\deploy\compose\README.md`

- [ ] **Write the README** capturing §9 run steps, the §8 `host.docker.internal` fallback, the §10 verification, and §11 risks. Include the exact client env-var-name table (client uses `PROJECT_ID`/`KABYTECH_KEY`/`MANAGER_WS`/`ZITADEL_ISSUER`, NOT the manager's names) and the two-scope warning. Round-trip uses `ws://127.0.0.1:7777/chat` (the client default at `llm_chat_client.py:42` and the manager's `127.0.0.1` listen log).
  ```markdown
  # llm-chat compose stack (local-dev only)

  Server-side stack: **postgres + Zitadel + provisioner + manager** in Docker,
  with the **worker running natively on Windows** (real `claude`, `~/.claude`,
  webview). LOCAL DEV ONLY — the issuer is plain HTTP, cookies are non-Secure.
  **Never expose this beyond your machine.**

  The whole thing hinges on one literal issuer string,
  `http://host.docker.internal:8080`, that resolves the same from the host
  (Python client) and from inside containers (manager). Don't change it.

  ## Prerequisites
  - Docker Desktop for Windows.
  - The worker built: `cargo build --release` in `worker/` (produces
    `worker/target/release/llm-chat.exe`).
  - Python 3 with `pyjwt[crypto]`, `requests`, `websockets` for the client.

  ## Run (§9)

  ```powershell
  # 0. From repo root D:\projects\llm-chat.

  # 1. Pre-flight: the three host ports must be FREE (a dual-listener 7777
  #    collision has bitten this environment before).
  Get-NetTCPConnection -LocalPort 7777,7878,8080 -State Listen -ErrorAction SilentlyContinue

  # 2. Env file.
  cp .env.example .env
  #   ZITADEL_MASTERKEY   -> openssl rand -hex 16   (exactly 32 hex chars; one-shot)
  #   POSTGRES_PASSWORD   -> a strong password
  #   LLM_CHAT_AUTH_TOKEN -> openssl rand -hex 32   (shared by manager + worker)

  # 3. Start the worker FIRST (before compose — the manager's :7878 probe is fatal).
  #    Approve the Windows Firewall prompt for the 0.0.0.0 bind if it appears.
  .\deploy\compose\run-worker.ps1

  # 4. Bring up the server side.
  docker compose up -d
  #    Wait until `docker compose ps` shows zitadel healthy + zitadel-init Exited(0),
  #    and .\secrets\kabytech-key.json + .\secrets\project_id exist.

  # 5. Round-trip with the Python client.
  python clients/python/llm_chat_client.py `
    --issuer  http://host.docker.internal:8080 `
    --project (Get-Content -Raw .\secrets\project_id).Trim() `
    --key-file .\secrets\kabytech-key.json `
    --manager ws://127.0.0.1:7777/chat `
    --send "hello"
  #    Expect an 'a' frame and exit code 0.
  ```

  ### Clean reset
  Wipe Zitadel state AND host secrets together, or the stale kabytech key won't
  match the fresh instance:
  ```powershell
  docker compose down -v
  Remove-Item -Recurse -Force .\secrets
  ```

  ## Client env-var names (footgun)
  The client reads DIFFERENT env names than the manager. If driving by env
  instead of flags:

  | Client flag | Client env var | NOT |
  |---|---|---|
  | `--issuer`   | `ZITADEL_ISSUER` | — |
  | `--project`  | `PROJECT_ID`     | not `ZITADEL_PROJECT_ID` |
  | `--key-file` | `KABYTECH_KEY`   | — |
  | `--manager`  | `MANAGER_WS`     | — |

  ## Two scopes — do not swap (spec §7.2)
  - Provisioner (Management API): `openid profile urn:zitadel:iam:org:project:id:zitadel:aud` (literal `zitadel`).
  - Client (token the manager validates): `openid profile urn:zitadel:iam:org:project:id:<project>:aud urn:zitadel:iam:org:projects:roles` (numeric project id + plural `projects:roles`). This is already fixed in the Python client.

  ## host.docker.internal fallback (§8)
  Container-side resolution is automatic under Docker Desktop. If HOST-side
  resolution fails (e.g. WSL2 engine with the Win32-hosts setting off), verify
  first:
  ```powershell
  Resolve-DnsName host.docker.internal
  curl http://host.docker.internal:8080/.well-known/openid-configuration
  ```
  Only if Docker is NOT already managing the entry, append as Administrator to
  `C:\Windows\System32\drivers\etc\hosts`:
  ```
  127.0.0.1 host.docker.internal
  ```
  Do NOT duplicate it if Docker manages it (duplicates cause flaky resolution).
  Publish Zitadel `8080:8080` (all interfaces), never `127.0.0.1:8080:8080`.

  ## Verification (§10)
  - `docker compose config --quiet` exits 0.
  - `docker compose logs manager` shows **"Zitadel auth enabled"**, NOT
    "Zitadel auth NOT configured — falling back to shared-token auth".
  - `.\secrets\kabytech-key.json` is valid JSON with `"type":"serviceaccount"`,
    `keyId`, `key` (PEM), `userId`.
  - The client round-trip returns `a` and exits 0.

  ## Risks (§11)
  - **Management-API drift:** the v1 endpoints are deprecated; the Zitadel tag is
    pinned (to-confirm). Never `:latest`. Re-verify the provisioner call surface
    on any bump.
  - **`_search` 409-recovery is UNVERIFIED:** provision.py raises on a 409 instead
    of guessing the endpoint. Clean reset (down -v + delete secrets) avoids it.
  - **Port collisions:** free 7777/7878/8080 first (see pre-flight).
  - **Clock skew:** JWT iat/exp windows fail if host/container clocks drift.
  - **HTTP-only issuer:** no TLS, cleartext tokens — local-dev only.
  - **Required address vars:** the manager and worker fail fast if MANAGER_BIND,
    MANAGER_BACKEND_HOST, or LLM_CHAT_WS_BIND is unset — compose/run-worker.ps1
    set them all, so this only bites if you launch a binary by hand without them.
  - **Secrets:** `.\secrets\kabytech-key.json` is a live RSA key; `secrets/` is
    gitignored. Never commit it.
  - **Firewall prompt:** approve the worker's 0.0.0.0 bind (private networks).
  - **Manager probe is fatal:** start the worker before `docker compose up`;
    `restart: unless-stopped` heals transient windows.
  - **Masterkey irreversible:** exactly 32 chars, never change after first init.
  ```
- [ ] **Verify it renders** (no broken tables): `cd D:\projects\llm-chat; docker run --rm -v ${PWD}:/d -w /d python:3-slim python -c "open('deploy/compose/README.md').read(); print('ok')"` → `ok` (sanity that the file is readable; markdown has no build step).
- [ ] **Commit:**
  `git add deploy/compose/README.md`
  `git commit` message:
  ```
  docs(compose): README with run steps, fallback, and risks

  Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
  ```

---

## Task T13 — End-to-end verification (§10 full)

No new files; this task **runs** the stack and asserts the spec's §10 criteria. Record actual outputs; do not claim success without them.

- [ ] **Pre-flight ports free (§9.1 / §11):**
  `cd D:\projects\llm-chat; Get-NetTCPConnection -LocalPort 7777,7878,8080 -State Listen -ErrorAction SilentlyContinue`
  Expected: no rows. If any row → free that port before continuing (the dual-listener 7777 collision is a known hazard here).
- [ ] **Env file present:** `Test-Path .env` is `True` with `ZITADEL_MASTERKEY` (32 chars), `POSTGRES_PASSWORD`, `LLM_CHAT_AUTH_TOKEN` filled.
- [ ] **Confirm the worker binary exists (run-worker.ps1 prerequisite):** `cd D:\projects\llm-chat\worker; cargo build --release; Test-Path .\target\release\llm-chat.exe` → `True`.
- [ ] **Start the worker FIRST (§7.1 ordering contract):** `cd D:\projects\llm-chat; .\deploy\compose\run-worker.ps1` (approve the firewall prompt) — run in a separate terminal since it blocks. Confirm it is listening on all interfaces:
  `Get-NetTCPConnection -LocalPort 7878 -State Listen` → one row with `LocalAddress` `0.0.0.0`.
- [ ] **Compose lint (§10.1):** `docker compose config --quiet; echo "exit=$LASTEXITCODE"` → `exit=0`. `docker compose config` shows the 4 services + 3 volumes + `./secrets` bind + healthchecks + depends_on conditions.
- [ ] **Bring up + healthcheck-gated boot (§10.2):** `docker compose up -d`, then poll:
  `docker compose ps`
  Expected progression: `postgres` and `zitadel` reach `healthy`; `zitadel-init` becomes `Exited (0)`; `manager` is `Up` and started **after** `zitadel-init` completed. Verify manager did not start early via `docker compose logs manager` (first manager log line appears after init exits).
  - **§12 healthcheck-tool check:** if `zitadel` never goes healthy, run `docker compose exec zitadel sh -c "command -v wget || command -v curl || echo NONE"`; if `NONE`, swap the healthcheck to `["CMD","/app/zitadel","ready"]` (committed under T10), confirm empirically, and re-up.
- [ ] **Manager in JWT mode, NOT shared-token fallback (§10.3):**
  `docker compose logs manager | Select-String "Zitadel auth"`
  Expected: a line `Zitadel auth enabled` with `issuer/audience/project_id`; **must NOT** contain `Zitadel auth NOT configured — falling back to shared-token auth`. (If the fallback line appears, the generated env is partial — the entrypoint assertion should have already failed, so investigate `manager.generated.env`.)
- [ ] **Provisioner artifacts present + valid (§10.4):**
  - `Test-Path .\secrets\kabytech-key.json` = True; `Get-Content -Raw .\secrets\kabytech-key.json | ConvertFrom-Json` yields an object with `type` == `serviceaccount`, non-empty `keyId`, `key` (PEM string), `userId`.
  - `(Get-Content -Raw .\secrets\project_id).Trim()` non-empty; `(Get-Content -Raw .\secrets\kabytech_user_id).Trim()` non-empty.
  - Generated env: `docker compose exec manager cat /out/manager.generated.env` shows `ZITADEL_PROJECT_ID=<id>` and `ZITADEL_AUDIENCE=<id>` equal and non-empty. (The unit test `test_write_generated_env_writes_project_id_and_equal_audience` in T6 guards this content statically; this is the end-to-end confirmation.)
- [ ] **Full client round-trip returns `a` and exits 0 (§10.5):**
  ```powershell
  python clients/python/llm_chat_client.py `
    --issuer  http://host.docker.internal:8080 `
    --project (Get-Content -Raw .\secrets\project_id).Trim() `
    --key-file .\secrets\kabytech-key.json `
    --manager ws://127.0.0.1:7777/chat `
    --send "hello"
  echo "exit=$LASTEXITCODE"
  ```
  Expected: client prints an `a` frame for `"hello"`; the `a` frame carries `seq`; the client sends `{"type":"confirm","seq":<seq>}`; **`exit=0`** (the real pass criterion — a missing `seq` would raise `KeyError`, not exit 0).
- [ ] **Idempotency re-run is a no-op (§10.6):**
  - Capture the key hash: `$h1 = (Get-FileHash .\secrets\kabytech-key.json).Hash`
  - `docker compose run --rm zitadel-init; echo "exit=$LASTEXITCODE"` → `exit=0`; logs show create calls returning 409-as-success and `key for userId=... already on disk — skipping keygen`.
  - `$h2 = (Get-FileHash .\secrets\kabytech-key.json).Hash; $h1 -eq $h2` → `True` (key NOT rewritten).
  - **§12 note:** the `409` branches currently raise `SystemExit` for the project/user creates (the `_search` recovery is UNVERIFIED). If this no-op re-run hits those `SystemExit`s instead of the skip-keygen path, that confirms the `_search` recovery must be implemented and verified before relying on same-instance re-provisioning; the **clean-boot** path below is the supported flow.
- [ ] **Self-heal after `down -v` without deleting secrets (§10.6 second half):**
  - Capture the old user id: `$oldUser = (Get-Content -Raw .\secrets\kabytech_user_id).Trim()`
  - `docker compose down -v` (wipes Zitadel + volumes; leaves `./secrets`).
  - **Worker re-start gate (explicit condition):** `Get-NetTCPConnection -LocalPort 7878 -State Listen -ErrorAction SilentlyContinue` — if NO row, re-run `.\deploy\compose\run-worker.ps1` (approve the firewall prompt) in a separate terminal; if a row exists, leave it running.
  - `docker compose up -d`.
  - The provisioner detects the on-disk `userId` no longer matches the fresh instance and **regenerates**: `(Get-Content -Raw .\secrets\kabytech_user_id).Trim() -ne $oldUser` → `True`; re-run the §10.5 round-trip command above → returns `a`, `exit=0`.
  - (Note: on this fresh instance the project/user creates are 200, not 409, so the UNVERIFIED `_search` branches are not exercised — this is the supported clean path.)
- [ ] **Required-address + fail-fast smoke (§10.7) — replaces the old "no env vars set" test.** This task proves BOTH (a) the address vars SET to loopback reproduce today's behavior, AND (b) any one of them UNSET makes the binary fail fast at startup naming the missing var. The two mode toggles (`MANAGER_BACKEND_PORTS`, `LLM_CHAT_AUTH_TOKEN`) keep their unset == today semantics.
  - **Build both binaries:**
    `cd D:\projects\llm-chat; cargo build --release -p llm-chat-manager`
    `cd D:\projects\llm-chat\worker; cargo build --release`
    Expected: both `Finished `release` profile` lines, exit 0. (The worker release binary at `worker\target\release\llm-chat.exe` is what the manager will spawn.)
  - **(a) Address vars SET to 127.0.0.1 == today's loopback/spawn behavior.** Run the manager standalone with the three address vars set to loopback, the mode toggles unset (so it spawns a worker and generates a token), and `LLM_CHAT_EXE` pointed at the worker binary (the standalone manager is NOT next to the worker exe, so its `current_exe`-dir fallback would not find it — `LLM_CHAT_EXE` is the required prerequisite, see `manager/src/main.rs:660`). The spawned worker inherits `LLM_CHAT_WS_BIND` from `spawn_instance` (= `MANAGER_BACKEND_HOST` = `127.0.0.1`). **The manager persists its generated token to its on-disk token file; capture that path so the loopback client can authenticate deterministically.**
    ```powershell
    cd D:\projects\llm-chat
    Remove-Item Env:MANAGER_BACKEND_PORTS, Env:LLM_CHAT_AUTH_TOKEN, Env:ZITADEL_ISSUER -ErrorAction SilentlyContinue
    $env:MANAGER_BIND = "127.0.0.1"
    $env:MANAGER_BACKEND_HOST = "127.0.0.1"
    $env:LLM_CHAT_EXE = "$PWD\worker\target\release\llm-chat.exe"
    Start-Process -FilePath "$PWD\manager\target\release\llm-chat-manager.exe" -RedirectStandardError manager-bc.log -PassThru
    ```
    - **Assert loopback listen + spawn (NOT external-backend mode):**
      `Get-NetTCPConnection -LocalPort 7777,7878 -State Listen` → both rows have `LocalAddress` `127.0.0.1` (NOT `0.0.0.0`). (The 7878 row is the spawned worker, whose bind was forwarded as `127.0.0.1`.)
      `Get-Content manager-bc.log | Select-String "manager listening"` → contains `ws://127.0.0.1:7777`.
      `Get-Content manager-bc.log | Select-String "external backend mode"` → **no match** (it spawned, did not run external mode).
    - **Loopback round-trip still works (shared-token auth, no Zitadel) — deterministic auth.** With `ZITADEL_ISSUER` unset the manager is in shared-token fallback, so the client must present the SAME token the manager wrote to disk. Resolve the manager's token-file path and pass it to the client explicitly (do not rely on a re-generated value):
      ```powershell
      # The manager writes its chosen/generated token to auth_token_path()
      # (the same file call_backend reads). Read it back and drive the client
      # against ws://127.0.0.1:7777/chat in shared-token mode (Zitadel flags
      # omitted). Pass the token via the client's shared-token flag/env.
      $tok = (Get-Content -Raw $env:LOCALAPPDATA\llm-chat\auth.token -ErrorAction SilentlyContinue).Trim()
      python clients/python/llm_chat_client.py `
        --manager ws://127.0.0.1:7777/chat `
        --auth-token $tok `
        --send "hello"
      echo "exit=$LASTEXITCODE"
      ```
      Expected: an `a` frame and `exit=0`. (Pass criterion is the `a` frame + exit 0. If the manager's on-disk token path differs on this host, resolve it from the manager's startup log / `auth_token_path()` rather than guessing — the point is to feed the client the exact token the manager persisted, so shared-token auth is deterministic, not hand-waved.)
    - Stop the standalone manager (`Stop-Process` the returned PID) and its spawned worker.
  - **(b) Any one of the three address vars UNSET → fail fast at startup, naming the var.** Each sub-check launches the binary with exactly one required var removed and asserts a nonzero exit AND the var name in stderr. Run each, then read its log:
    - **Manager, `MANAGER_BIND` missing:**
      ```powershell
      cd D:\projects\llm-chat
      Remove-Item Env:MANAGER_BIND -ErrorAction SilentlyContinue
      $env:MANAGER_BACKEND_HOST = "127.0.0.1"
      & "$PWD\manager\target\release\llm-chat-manager.exe" 2> manager-bind-missing.log; echo "exit=$LASTEXITCODE"
      ```
      Expected: nonzero `exit`; `Get-Content manager-bind-missing.log | Select-String "MANAGER_BIND must be set"` matches.
    - **Manager, `MANAGER_BACKEND_HOST` missing:**
      ```powershell
      $env:MANAGER_BIND = "127.0.0.1"
      Remove-Item Env:MANAGER_BACKEND_HOST -ErrorAction SilentlyContinue
      & "$PWD\manager\target\release\llm-chat-manager.exe" 2> manager-host-missing.log; echo "exit=$LASTEXITCODE"
      ```
      Expected: nonzero `exit`; `Get-Content manager-host-missing.log | Select-String "MANAGER_BACKEND_HOST must be set"` matches. (`main()` validates `MANAGER_BACKEND_HOST` before the listen bind, so this fails fast regardless of `MANAGER_BIND`.)
    - **Worker, `LLM_CHAT_WS_BIND` missing:**
      ```powershell
      Remove-Item Env:LLM_CHAT_WS_BIND -ErrorAction SilentlyContinue
      & "$PWD\worker\target\release\llm-chat.exe" 2> worker-bind-missing.log; echo "exit=$LASTEXITCODE"
      Get-NetTCPConnection -LocalPort 7878 -State Listen -ErrorAction SilentlyContinue
      ```
      Expected: nonzero `exit`; `Get-Content worker-bind-missing.log | Select-String "LLM_CHAT_WS_BIND must be set"` matches; the `Get-NetTCPConnection` returns **no row** (the worker exited at startup rather than binding a default loopback socket).
  - This proves the three address vars are required + fail-fast (no hidden default), while the two mode toggles still default to today's behavior. Clean up: `Remove-Item manager-bc.log, manager-bind-missing.log, manager-host-missing.log, worker-bind-missing.log -ErrorAction SilentlyContinue`.
- [ ] **Tear down:** `docker compose down -v; Remove-Item -Recurse -Force .\secrets` (clean reset), stop the worker.
- [ ] **No code/file changes in this task** → **no commit.** (If the verification surfaced a real defect, fix it in the owning task T1–T12 under that task's commit, root-causing per CLAUDE.md — never a dirty fix.)

---

## Coverage map (every spec section is implemented)

- §4.1 postgres + wiring (B)/(A) fallback → **T10** (+ §12 verification with committed fallback).
- §4.2 zitadel image/command/healthcheck/firstinstance → **T10** (+ §12 tag/tool/TLS checks with committed fallback).
- §4.3 provisioner sequence, retries, org context, userId key-guard, base64 decode, outputs (incl. `write_generated_env` PROJECT_ID==AUDIENCE, unit-tested) → **T6** (+ §12 `_search`/`users/me` flags).
- §4.4 manager image/entrypoint/external-backend/restart → **T8** (image+entrypoint+Cargo.lock), **T2/T4** (external-backend), **T10** (restart/depends_on).
- §4.5 worker host launch env → **T11** (+ **T1** required bind code + standalone npm scripts; binary-name confirmed in T1/T13).
- §5.1 manager code (BACKEND_HOST required+resolved-in-main + documented per-request reads, BIND required, BACKEND_PORTS toggle, AUTH_TOKEN toggle, wait_for_tcp host param, spawn_instance bind forwarding, require_addr fail-fast) → **T2, T3, T4, T5**.
- §5.2 worker `LLM_CHAT_WS_BIND` required + fail-fast — implemented as an **in-place conversion** of the repo's already-applied default-to-loopback helper/test/bind-site (helper at lib.rs:1555 `-> String`, bind at lib.rs:1660 already using it, `ws_bind_tests` at lib.rs:2372 asserting the loopback default) to the required + Result + fail-fast form (and the documented no-change items) → **T1**.
- §6 new files (compose, .env.example, .dockerignore, .gitignore edit, Dockerfiles, entrypoint, provisioner, run-worker, README; manager.env.example edit; worker package.json scripts; README/architecture-doc run-step edits; provisioner `conftest.py` is a test-only shim flagged in T6) → **T1, T3, T6–T12**.
- §7 boot sequence + round-trip (incl. `seq`/confirm) → exercised in **T13**.
- §8 single-issuer resolution + hosts fallback → documented **T12**, verified **T13**.
- §9 run instructions → **T12** README, executed **T13**.
- §10 testing 1–7 → **T13** (plus per-task unit tests in T1–T6, incl. `write_generated_env`, and Dockerfile verifications in T7/T8/T10); §10.7 is now the required-address + fail-fast smoke: (a) loopback-set vars reproduce today's spawn/loopback round-trip with deterministic shared-token auth (client fed the manager's persisted token file), (b) any one address var unset fails fast naming it, with the `LLM_CHAT_EXE` spawn prerequisite.
- §11 risks → mitigations embedded across **T9** (gitignore), **T10** (ports/publish/pin), **T11** (firewall), **T12** (README incl. required-address footgun), **T13** (pre-flight).
- §12 open questions → explicit verification steps with committed fallbacks in **T6** (_search/org-id), **T10** (tag/tool/TLS/postgres); §12.6 optional manager fail-fast for the Zitadel/shared-token vars is **out of scope** per the spec's "Adopt only if approved" stance (§4.4 / §12.6) and is intentionally not implemented — the entrypoint assertion in **T8** covers the compose path. (Note: this is distinct from the now-required *address*-var fail-fast in §5, which IS implemented in T1–T3.) Confirm the user did not separately approve §12.6.
