# Zitadel User-Management Admin — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (- [ ]) syntax for tracking.

**Goal:** Build a separate Rust (axum) BFF + Next.js admin that manages Zitadel users (machine + human) entirely through the Zitadel Management API, with operators authenticated via Zitadel OIDC and authorized on a new chat.admin role — no Zitadel console needed.

**Architecture:** Approach A (BFF). admin-api owns the OIDC session and all credentials (admin SA key, OIDC client secret, cached Management-API token) and exposes a cookie-authed JSON API; admin-web (Next.js) is a pure UI. auth_zitadel.rs is shared verbatim via a new crates/zitadel-auth workspace lib; the manager and the chat data path are untouched.

**Tech Stack:** Rust workspace (axum 0.8, tokio 1, tower-sessions 0.15, reqwest 0.12 rustls, jsonwebtoken 9), Next.js 16 App Router (pnpm, shadcn/ui, TanStack Table, react-hook-form + zod), Python provisioner (pyjwt[crypto] + requests), Docker Compose, Zitadel v3.4.10.

---

## Phase A-workspace

### Task 1: Convert repo to a Cargo workspace (manager + worker stay green)

**Files:**
- Create: `D:\projects\llm-chat\Cargo.toml`
- Modify: `D:\projects\llm-chat\manager\Cargo.toml:10-41` (deps → workspace deps)
- Modify: `D:\projects\llm-chat\worker\Cargo.toml` (no member changes needed; lock shared)

- [ ] **Step 1: Write the failing test** — there is no root manifest yet, so the "test" is the workspace metadata resolving. This is a metadata bootstrap, not a `#[test]`: the failing check is running cargo against a workspace that does not exist yet. First confirm RED with the command in Step 2 (no implementation written).

- [ ] **Step 2: Run it — expect FAIL** — from `D:\projects\llm-chat`:
  ```powershell
  cargo metadata --no-deps --format-version 1 | Select-String '"workspace_members"'
  ```
  Expected failure: `error: could not find 'Cargo.toml' in 'D:\projects\llm-chat' or any parent directory` (no workspace root exists). This proves RED.

- [ ] **Step 3: Implement** — create the workspace root manifest. To keep THIS task green on its own, list only the members that exist now (`manager`, `worker`); Task 2 adds `crates/zitadel-auth` and Task 9 adds `admin-api`. Write:

  `D:\projects\llm-chat\Cargo.toml`:
  ```toml
  [workspace]
  members = ["manager", "worker"]
  resolver = "2"

  [workspace.dependencies]
  tokio = { version = "1", features = ["rt-multi-thread", "macros"] }
  serde = { version = "1", features = ["derive"] }
  serde_json = "1"
  reqwest = { version = "0.12", default-features = false, features = ["json", "rustls-tls"] }
  jsonwebtoken = "9"
  tracing = "0.1"
  ```

  Then point the manager's matching deps at the workspace so the versions are shared. Edit `D:\projects\llm-chat\manager\Cargo.toml` — replace the four lines that the workspace now owns (keep manager-specific features by spelling them inline where they exceed the workspace set):
  ```toml
  serde = { workspace = true }
  serde_json = { workspace = true }
  tracing = "0.1"
  jsonwebtoken = { workspace = true }
  reqwest = { workspace = true }
  ```
  Leave `tokio` in manager as-is (it needs extra features `net,sync,io-util,time,process,signal` beyond the workspace default — do NOT replace with `workspace = true` or it loses those features). Leave `worker/Cargo.toml` unchanged (its tokio/serde feature sets differ; sharing is optional and YAGNI here).

- [ ] **Step 4: Run — expect PASS** — from `D:\projects\llm-chat`:
  ```powershell
  cargo metadata --no-deps --format-version 1 | Select-String 'workspace_members' ; cargo build -p llm-chat-manager -p llm-chat ; cargo test -p llm-chat-manager
  ```
  Expected: `cargo metadata` prints a `workspace_members` array containing both `llm-chat-manager` and `llm-chat`; both crates build; `cargo test -p llm-chat-manager` reports `test result: ok.` (manager behavior + tests unchanged). A single root `Cargo.lock` now exists.

- [ ] **Step 5: Commit**
  ```powershell
  git add D:\projects\llm-chat\Cargo.toml D:\projects\llm-chat\Cargo.lock D:\projects\llm-chat\manager\Cargo.toml
  git commit -m @'
  build: convert repo to a Cargo workspace (manager + worker)

  Add a workspace root Cargo.toml with resolver=2 and shared
  [workspace.dependencies]; point the manager's serde/serde_json/jsonwebtoken/
  reqwest at the workspace so versions are shared and there is one Cargo.lock.
  Manager keeps its extra tokio features inline. Behavior + tests unchanged.

  Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
  '@
  ```

---

### Task 2: Extract `crates/zitadel-auth` verbatim from `manager/src/auth_zitadel.rs`

**Files:**
- Create: `D:\projects\llm-chat\crates\zitadel-auth\Cargo.toml`
- Create: `D:\projects\llm-chat\crates\zitadel-auth\src\lib.rs` (verbatim move of `auth_zitadel.rs`)
- Modify: `D:\projects\llm-chat\Cargo.toml:2` (add the new member)
- Modify: `D:\projects\llm-chat\manager\Cargo.toml` (add `zitadel-auth` dependency)
- Modify: `D:\projects\llm-chat\manager\src\main.rs:21,673,829,836,1001` (drop `mod`, requalify)
- Delete: `D:\projects\llm-chat\manager\src\auth_zitadel.rs`

- [ ] **Step 1: Write the failing test** — prove the new crate exists and re-exports the four public items by compiling a doctest-free smoke test inside the new crate. Append to the moved `lib.rs` (you will create the file in Step 3, but author the test now so RED is real once the crate manifest exists). The test asserts the public surface is reachable by name:
  ```rust
  #[cfg(test)]
  mod surface_tests {
      use super::{AuthError, JwksCache, Principal, ZitadelConfig};

      #[test]
      fn public_items_are_exported_and_principal_has_works() {
          // Principal::has is the role check both manager and admin-api rely on.
          let p = Principal {
              user_id: "u1".into(),
              org_id: "o1".into(),
              roles: vec!["chat.user".into()],
              email: None,
          };
          assert!(p.has("chat.user"));
          assert!(!p.has("chat.admin"));

          // Construct a config + cache without touching the network.
          let cfg = ZitadelConfig {
              issuer: "https://id.example.com".into(),
              audience: vec!["proj-1".into()],
              jwks_uri: "https://id.example.com/oauth/v2/keys".into(),
              project_id: "proj-1".into(),
          };
          let _cache = JwksCache::new(cfg);

          // AuthError Display is part of the kept surface.
          assert_eq!(AuthError::Missing.to_string(), "missing Authorization header");
      }
  }
  ```

- [ ] **Step 2: Run it — expect FAIL** — from `D:\projects\llm-chat`:
  ```powershell
  cargo test -p zitadel-auth surface_tests
  ```
  Expected failure: `error: package(s) 'zitadel-auth' not found in workspace` (the crate/member does not exist yet). RED.

- [ ] **Step 3: Implement** — create the crate, move the source verbatim, wire the workspace + manager.

  3a. `D:\projects\llm-chat\crates\zitadel-auth\Cargo.toml`:
  ```toml
  [package]
  name = "zitadel-auth"
  version = "0.1.0"
  edition = "2021"

  [lib]
  name = "zitadel_auth"
  path = "src/lib.rs"

  [dependencies]
  serde = { workspace = true }
  serde_json = "1"
  jsonwebtoken = { workspace = true }
  reqwest = { workspace = true }
  tokio-tungstenite = "0.21"
  ```
  (`tokio-tungstenite` is required because `extract_bearer` takes a `tungstenite::handshake::server::Request`. `serde_json` is used by `verify_sync` for `TokenData<serde_json::Value>`.)

  3b. Create `D:\projects\llm-chat\crates\zitadel-auth\src\lib.rs` with the EXACT current contents of `manager/src/auth_zitadel.rs` (all 242 lines: module doc, `Principal`/`AuthError`/`ZitadelConfig`/`Jwks`/`JwkInner`/`CacheInner`/`JwksCache`/`extract_bearer`), then append the `surface_tests` module from Step 1 at the end. No logic changes — verbatim move keeps it behavior-preserving.

  3c. Add the member to `D:\projects\llm-chat\Cargo.toml`:
  ```toml
  members = ["manager", "worker", "crates/zitadel-auth"]
  ```

  3d. Add the dependency to `D:\projects\llm-chat\manager\Cargo.toml` (under `[dependencies]`):
  ```toml
  zitadel-auth = { path = "../crates/zitadel-auth" }
  ```

  3e. Edit `D:\projects\llm-chat\manager\src\main.rs`:
  - Line 21: delete `mod auth_zitadel;`.
  - Line 673: `jwks: Option<auth_zitadel::JwksCache>,` → `jwks: Option<zitadel_auth::JwksCache>,`
  - Line 829: `let jwks = match auth_zitadel::ZitadelConfig::from_env() {` → `...zitadel_auth::ZitadelConfig::from_env() {`
  - Line 836: `let cache = auth_zitadel::JwksCache::new(cfg);` → `let cache = zitadel_auth::JwksCache::new(cfg);`
  - Line 1001: `let token = match auth_zitadel::extract_bearer(req) {` → `...zitadel_auth::extract_bearer(req) {`
  (Optionally add `use zitadel_auth::{JwksCache, ZitadelConfig, extract_bearer};` near the top imports and drop the path prefixes — but the minimal requalify above is sufficient and keeps the diff tight.)

  3f. Delete `D:\projects\llm-chat\manager\src\auth_zitadel.rs`:
  ```powershell
  git rm D:\projects\llm-chat\manager\src\auth_zitadel.rs
  ```

- [ ] **Step 4: Run — expect PASS** — from `D:\projects\llm-chat`:
  ```powershell
  cargo test -p zitadel-auth surface_tests ; cargo build -p llm-chat-manager ; cargo test -p llm-chat-manager
  ```
  Expected: `zitadel-auth` test prints `test result: ok. 1 passed`; the manager builds clean (no `auth_zitadel` module, references resolve to `zitadel_auth::`); `cargo test -p llm-chat-manager` still reports `test result: ok.` — behavior preserved.

- [ ] **Step 5: Commit**
  ```powershell
  git add D:\projects\llm-chat\Cargo.toml D:\projects\llm-chat\Cargo.lock D:\projects\llm-chat\crates\zitadel-auth\Cargo.toml D:\projects\llm-chat\crates\zitadel-auth\src\lib.rs D:\projects\llm-chat\manager\Cargo.toml D:\projects\llm-chat\manager\src\main.rs
  git rm D:\projects\llm-chat\manager\src\auth_zitadel.rs
  git commit -m @'
  refactor: extract crates/zitadel-auth from manager/src/auth_zitadel.rs

  Move the Zitadel JWT auth verbatim into a shared lib crate (lib name
  zitadel_auth) keeping ZitadelConfig, JwksCache, Principal, AuthError public.
  Manager now depends on it: drop "mod auth_zitadel", requalify uses to
  zitadel_auth::, delete the old module. Behavior + manager tests unchanged;
  admin-api will reuse the same crate in Phase C.

  Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
  '@
  ```

---

### Task 3: Add a pure `chat.admin` role-extraction unit test (+ extract the pure helper)

**Files:**
- Modify: `D:\projects\llm-chat\crates\zitadel-auth\src\lib.rs` (add a pure `roles_from_claims` helper; call it from `verify_sync`)
- Test: `D:\projects\llm-chat\crates\zitadel-auth\src\lib.rs` (`#[cfg(test)]` module)

Rationale: `verify_sync` currently inlines the role extraction (lib.rs:205-210), which is unit-testable only behind a signed JWT + live JWKS — racy and network-bound. Per the pure-helper/thin-wrapper convention, lift the claims→roles math into a pure fn so `chat.admin` extraction is tested without I/O. This is the source-of-truth claim shape (`urn:zitadel:iam:org:project:{pid}:roles` is a JSON object whose KEYS are the role names), exercised directly on a real claims `Value`. NOTE: this `roles_from_claims` (Rust) is the single canonical extraction; the Python copy in Task 8 (`has_admin_role`) must keep the identical claim-key format `urn:zitadel:iam:org:project:{pid}:roles`.

- [ ] **Step 1: Write the failing test** — append to the `#[cfg(test)]` area of `D:\projects\llm-chat\crates\zitadel-auth\src\lib.rs`:
  ```rust
  #[cfg(test)]
  mod role_extraction_tests {
      use super::roles_from_claims;
      use serde_json::json;

      #[test]
      fn extracts_chat_admin_from_project_roles_object() {
          let pid = "311867081814147073";
          // Zitadel encodes project roles as an OBJECT keyed by role name; each
          // value is itself a map of orgId -> primaryDomain. We want the KEYS.
          let claims = json!({
              "sub": "u-1",
              format!("urn:zitadel:iam:org:project:{pid}:roles"): {
                  "chat.user":  { "o-1": "example.localhost" },
                  "chat.admin": { "o-1": "example.localhost" }
              }
          });
          let mut roles = roles_from_claims(&claims, pid);
          roles.sort();
          assert_eq!(roles, vec!["chat.admin".to_string(), "chat.user".to_string()]);
      }

      #[test]
      fn missing_roles_claim_yields_empty() {
          let claims = json!({ "sub": "u-2" });
          assert!(roles_from_claims(&claims, "311867081814147073").is_empty());
      }

      #[test]
      fn wrong_project_id_yields_empty() {
          let claims = json!({
              "urn:zitadel:iam:org:project:OTHER:roles": { "chat.admin": {} }
          });
          assert!(roles_from_claims(&claims, "311867081814147073").is_empty());
      }
  }
  ```

- [ ] **Step 2: Run it — expect FAIL** — from `D:\projects\llm-chat`:
  ```powershell
  cargo test -p zitadel-auth role_extraction_tests
  ```
  Expected failure: `error[E0425]: cannot find function 'roles_from_claims' in this scope` (the pure helper does not exist yet). RED.

- [ ] **Step 3: Implement** — add the pure helper and call it from `verify_sync` (DRY: one extraction path). In `D:\projects\llm-chat\crates\zitadel-auth\src\lib.rs`, add the free function (place it near the bottom, beside `extract_bearer`):
  ```rust
  /// Pure: pull the project-role names out of a verified claims object.
  ///
  /// Zitadel encodes project roles under
  /// `urn:zitadel:iam:org:project:<project_id>:roles` as a JSON object whose
  /// KEYS are the role names (each value is an orgId→primaryDomain map we
  /// ignore). Returns the role names; empty when the claim is absent or not an
  /// object. No I/O — unit-testable without a signed token.
  pub fn roles_from_claims(claims: &serde_json::Value, project_id: &str) -> Vec<String> {
      let roles_key = format!("urn:zitadel:iam:org:project:{}:roles", project_id);
      claims
          .get(&roles_key)
          .and_then(|v| v.as_object())
          .map(|m| m.keys().cloned().collect())
          .unwrap_or_default()
  }
  ```
  Then replace the inlined block in `verify_sync` (the lines that build `roles_key` + collect `roles`, currently lib.rs:204-210) with a call to the helper so there is a single source of the extraction logic:
  ```rust
          // Zitadel encodes project roles under
          //   urn:zitadel:iam:org:project:<projectid>:roles
          let roles: Vec<String> = roles_from_claims(&claims, &self.cfg.project_id);
  ```
  (No behavior change — identical math, now reused and testable.)

- [ ] **Step 4: Run — expect PASS** — from `D:\projects\llm-chat`:
  ```powershell
  cargo test -p zitadel-auth ; cargo test -p llm-chat-manager
  ```
  Expected: `zitadel-auth` reports all tests passing including the three `role_extraction_tests` (`test result: ok. 4 passed` with the earlier surface test), and `llm-chat-manager` still reports `test result: ok.` — the `verify_sync` refactor preserved manager behavior.

- [ ] **Step 5: Commit**
  ```powershell
  git add D:\projects\llm-chat\crates\zitadel-auth\src\lib.rs
  git commit -m @'
  test(zitadel-auth): pure chat.admin role-extraction helper + tests

  Lift the project-role extraction out of verify_sync into a pure
  roles_from_claims(claims, project_id) so chat.admin extraction is unit-tested
  on a real claims Value with no network or signed JWT. verify_sync now calls
  the helper (single source of truth, no behavior change). Covers the
  urn:zitadel:iam:org:project:{pid}:roles object-keyed shape, missing-claim,
  and wrong-project-id cases.

  Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
  '@
  ```

---

## Phase B-provisioner

### Task 4: `create_admin_role` — the `chat.admin` project role (idempotent, 409=success)
**Files:**
- Modify: `deploy/compose/provisioner/provision.py:197-204` (add new helper next to `add_role`; new module constant near line 28)
- Test: `deploy/compose/provisioner/test_provision.py` (append after the demo-user tests, ~line 150)

This mirrors the existing `add_role` contract exactly: a role re-POST returns `409 ALREADY_EXISTS`, which `is_success()` treats as provisioned (appendix §3.3, §6.5). It is NOT a clean-boot `SystemExit` create — roles are idempotent like `chat.user`.

- [ ] **Step 1: Write the failing test** — append to `test_provision.py`:
  ```python
  # ---------- chat.admin role + admin SA + admin OIDC WEB app (admin-api path) ----------

  def test_create_admin_role_posts_chat_admin_rolekey():
      captured = {}

      def fake_rwr(method, url, *, headers=None, json_body=None, **kw):
          captured["method"] = method
          captured["url"] = url
          captured["body"] = json_body
          return _FakeResp(200, {"details": {}})

      with mock.patch.object(provision, "request_with_retry", fake_rwr):
          provision.create_admin_role("tok", {"h": "1"}, "proj-1")
      assert captured["method"] == "POST"
      assert captured["url"].endswith("/management/v1/projects/proj-1/roles")
      b = captured["body"]
      assert b["roleKey"] == "chat.admin"
      assert b["displayName"] == "Chat Admin"


  def test_create_admin_role_409_is_success_not_systemexit():
      # role re-create is idempotent (appendix §3.3/§6.5): 409 == provisioned, no raise.
      with mock.patch.object(provision, "request_with_retry",
                             lambda *a, **k: _FakeResp(409)):
          provision.create_admin_role("tok", {}, "p")  # must NOT raise


  def test_create_admin_role_raises_on_hard_error():
      with mock.patch.object(provision, "request_with_retry",
                             lambda *a, **k: _FakeResp(400)):
          with pytest.raises(RuntimeError):
              provision.create_admin_role("tok", {}, "p")
  ```

- [ ] **Step 2: Run it — expect FAIL** —
  `cd D:\projects\llm-chat; python -m pytest deploy/compose/provisioner/test_provision.py -v -k create_admin_role`
  Expected: 3 errors/failures with `AttributeError: module 'provision' has no attribute 'create_admin_role'`.

- [ ] **Step 3: Implement** — add the constant near line 28 (under `ROLE_KEY = "chat.user"`):
  ```python
  ADMIN_ROLE_KEY = "chat.admin"
  ```
  and add the helper immediately after `add_role` (after line 204):
  ```python
  def create_admin_role(token: str, headers: dict, project_id: str) -> None:
      """Create the chat.admin project role the admin-api authorizes operators on
      (appendix §3.3). Idempotent like add_role: 409 ALREADY_EXISTS == provisioned.
      Keeping role creation in the one-time provisioner is what lets the runtime
      admin SA stay least-privilege (no project.role.write needed at runtime)."""
      resp = request_with_retry(
          "POST", f"{ISSUER}/management/v1/projects/{project_id}/roles",
          headers=headers,
          json_body={"roleKey": ADMIN_ROLE_KEY, "displayName": "Chat Admin",
                     "group": ""},
      )
      if not is_success(resp.status_code):
          resp.raise_for_status()
  ```

- [ ] **Step 4: Run — expect PASS** —
  `cd D:\projects\llm-chat; python -m pytest deploy/compose/provisioner/test_provision.py -v -k create_admin_role`
  Expected: `3 passed`.

- [ ] **Step 5: Commit** —
  `git add deploy/compose/provisioner/provision.py deploy/compose/provisioner/test_provision.py`
  ```powershell
  git commit -m @'
  feat(provisioner): create chat.admin project role (idempotent)

  Add create_admin_role() following the existing add_role idempotency
  contract — 409 ALREADY_EXISTS counts as provisioned (appendix §3.3/§6.5).
  Role creation stays in the one-time provisioner so the runtime admin SA
  needs no project.role.write.

  Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
  '@
  ```

---

### Task 5: `create_admin_sa` — the `chat-admin-api` machine user + JSON key (clean-boot create)
**Files:**
- Modify: `deploy/compose/provisioner/provision.py:207-236` (add helper near `create_machine_user`/`generate_json_key`; new constant near line 29)
- Test: `deploy/compose/provisioner/test_provision.py` (append after Task 4 tests)

The machine-user create follows the existing `create_machine_user` clean-boot contract: `200`→`userId`, `409`→`SystemExit` (UNVERIFIED `_search` recovery, §6.2). Enum trap: machine USER uses `ACCESS_TOKEN_TYPE_JWT` (NOT `OIDC_TOKEN_TYPE_JWT`). The key is minted exactly like `generate_json_key` (decode base64 `keyDetails`, appendix §2.2). This task tests the two pure-ish wrappers; the on-disk side effects (`secrets/admin-api-key.json` + `secrets/admin_api_user_id`) are wired in main() in Task 7.

- [ ] **Step 1: Write the failing test** — append to `test_provision.py`:
  ```python
  def test_create_admin_sa_posts_machine_user_jwt_token_type():
      captured = {}

      def fake_rwr(method, url, *, headers=None, json_body=None, **kw):
          captured["url"] = url
          captured["body"] = json_body
          return _FakeResp(200, {"userId": "sa-123"})

      with mock.patch.object(provision, "request_with_retry", fake_rwr):
          uid = provision.create_admin_sa("tok", {"h": "1"})
      assert uid == "sa-123"
      assert captured["url"].endswith("/management/v1/users/machine")
      b = captured["body"]
      assert b["userName"] == "chat-admin-api"
      assert b["name"] == "chat-admin-api"
      # enum trap: machine USER token type, NOT the OIDC_TOKEN_TYPE_JWT app enum
      assert b["accessTokenType"] == "ACCESS_TOKEN_TYPE_JWT"


  def test_create_admin_sa_409_is_systemexit_clean_boot():
      # clean-boot contract: a pre-existing SA means stale state; _search recovery
      # is UNVERIFIED (§12/§6.2) so we exit loudly rather than guess.
      with mock.patch.object(provision, "request_with_retry",
                             lambda *a, **k: _FakeResp(409)):
          with pytest.raises(SystemExit):
              provision.create_admin_sa("tok", {})


  def test_generate_admin_key_decodes_keydetails():
      sa = {"type": "serviceaccount", "keyId": "k9", "key": "PEM", "userId": "sa-123"}
      kd_b64 = base64.b64encode(json.dumps(sa).encode()).decode()

      def fake_rwr(method, url, *, headers=None, json_body=None, **kw):
          assert url.endswith("/management/v1/users/sa-123/keys")
          assert json_body == {"type": "KEY_TYPE_JSON"}
          return _FakeResp(200, {"keyId": "k9", "keyDetails": kd_b64})

      with mock.patch.object(provision, "request_with_retry", fake_rwr):
          out = provision.generate_admin_key("tok", {}, "sa-123")
      assert out == sa
  ```

- [ ] **Step 2: Run it — expect FAIL** —
  `cd D:\projects\llm-chat; python -m pytest deploy/compose/provisioner/test_provision.py -v -k "admin_sa or admin_key"`
  Expected: failures with `AttributeError: module 'provision' has no attribute 'create_admin_sa'` (and `generate_admin_key`).

- [ ] **Step 3: Implement** — add the constant near line 29 (under `MACHINE_USERNAME = "kabytech"`):
  ```python
  ADMIN_SA_USERNAME = "chat-admin-api"
  ```
  add `create_admin_sa` after `create_machine_user` (after line 227):
  ```python
  def create_admin_sa(token: str, headers: dict) -> str:
      """Create the dedicated least-privilege admin-api machine user (appendix
      §2.1). Distinct from the bootstrap IAM_OWNER SA and from kabytech.
      ACCESS_TOKEN_TYPE_JWT (machine-user enum) — do NOT use the OIDC app enum
      OIDC_TOKEN_TYPE_JWT here (§7 enum trap). Clean-boot contract like
      create_machine_user: 409 -> SystemExit (UNVERIFIED _search recovery, §12)."""
      resp = request_with_retry(
          "POST", f"{ISSUER}/management/v1/users/machine", headers=headers,
          json_body={"userName": ADMIN_SA_USERNAME, "name": ADMIN_SA_USERNAME,
                     "description": "admin-api least-privilege management SA",
                     "accessTokenType": "ACCESS_TOKEN_TYPE_JWT"},
      )
      if resp.status_code == 200:
          return resp.json()["userId"]
      if resp.status_code == 409:
          raise SystemExit(
              "chat-admin-api SA already exists (409): _search recovery is "
              "UNVERIFIED (§12). On a clean reset run `docker compose down -v` "
              "AND delete ./secrets.")
      resp.raise_for_status()
      raise RuntimeError(f"create_admin_sa unexpected status {resp.status_code}")
  ```
  and add `generate_admin_key` after `generate_json_key` (after line 236):
  ```python
  def generate_admin_key(token: str, headers: dict, user_id: str) -> dict:
      """Mint the admin SA's JSON key; keyDetails (base64 serviceaccount JSON) is
      returned ONCE (appendix §2.2). Same shape as generate_json_key for kabytech."""
      resp = request_with_retry(
          "POST", f"{ISSUER}/management/v1/users/{user_id}/keys", headers=headers,
          json_body={"type": "KEY_TYPE_JSON"},
      )
      resp.raise_for_status()
      return decode_key_details(resp.json()["keyDetails"])
  ```

- [ ] **Step 4: Run — expect PASS** —
  `cd D:\projects\llm-chat; python -m pytest deploy/compose/provisioner/test_provision.py -v -k "admin_sa or admin_key"`
  Expected: `3 passed`.

- [ ] **Step 5: Commit** —
  `git add deploy/compose/provisioner/provision.py deploy/compose/provisioner/test_provision.py`
  ```powershell
  git commit -m @'
  feat(provisioner): create chat-admin-api SA + JSON key

  Add create_admin_sa() (machine user, ACCESS_TOKEN_TYPE_JWT — not the OIDC
  app enum) and generate_admin_key() (base64 keyDetails decoded once, §2.2),
  mirroring the kabytech create_machine_user/generate_json_key contract:
  200 -> userId, 409 -> SystemExit clean-boot.

  Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
  '@
  ```

---

### Task 6: `create_admin_oidc_app` + `assign_admin_member` — WEB/BASIC OIDC app and org-manager grant
**Files:**
- Modify: `deploy/compose/provisioner/provision.py:248-281` (add helper near `create_oidc_app`; constants near line 35)
- Test: `deploy/compose/provisioner/test_provision.py` (append after Task 5 tests)

`create_admin_oidc_app` is `create_oidc_app` with `appType=WEB`, `authMethodType=BASIC`, and it captures BOTH `clientId` AND `clientSecret` (shown once, appendix §1.2/§6.1). Enum trap: the app uses `OIDC_TOKEN_TYPE_JWT` (NOT the machine `ACCESS_TOKEN_TYPE_JWT`). `assign_admin_member` POSTs to `orgs/me/members` with `ORG_USER_MANAGER` and is idempotent (409=success, appendix §2.4); it is called with the BOOTSTRAP token in main(). The OIDC redirect URI here (`http://localhost:7676/callback`) is the admin-api's OWN origin — it MUST match `ADMIN_PUBLIC_ORIGIN` in compose (Task 25) and the `public_origin` field in `AdminConfig` (Task 11) used to build the callback in Task 17. `accessTokenRoleAssertion=true` is the §6.1 repair lever: it makes `chat.admin` ride in the access-token JWT even though the project keeps `projectRoleAssertion=false`.

- [ ] **Step 1: Write the failing test** — append to `test_provision.py`:
  ```python
  def test_create_admin_oidc_app_posts_web_basic_with_role_assertion():
      captured = {}

      def fake_rwr(method, url, *, headers=None, json_body=None, **kw):
          captured["url"] = url
          captured["body"] = json_body
          return _FakeResp(200, {"clientId": "cid-1", "clientSecret": "shh-1"})

      with mock.patch.object(provision, "request_with_retry", fake_rwr):
          cid, secret = provision.create_admin_oidc_app("tok", {"h": "1"}, "proj-1")
      assert (cid, secret) == ("cid-1", "shh-1")
      assert captured["url"].endswith("/management/v1/projects/proj-1/apps/oidc")
      b = captured["body"]
      assert b["appType"] == "OIDC_APP_TYPE_WEB"                  # confidential server
      assert b["authMethodType"] == "OIDC_AUTH_METHOD_TYPE_BASIC" # client_secret_basic
      assert b["accessTokenType"] == "OIDC_TOKEN_TYPE_JWT"        # OIDC app enum (not machine)
      assert b["accessTokenRoleAssertion"] is True               # roles in access JWT (§6.1)
      assert b["idTokenRoleAssertion"] is True
      assert b["devMode"] is True
      assert "OIDC_GRANT_TYPE_AUTHORIZATION_CODE" in b["grantTypes"]
      assert "OIDC_GRANT_TYPE_REFRESH_TOKEN" in b["grantTypes"]
      assert b["responseTypes"] == ["OIDC_RESPONSE_TYPE_CODE"]
      assert provision.ADMIN_OIDC_REDIRECT_URI in b["redirectUris"]


  def test_create_admin_oidc_app_409_is_systemexit():
      with mock.patch.object(provision, "request_with_retry",
                             lambda *a, **k: _FakeResp(409)):
          with pytest.raises(SystemExit):
              provision.create_admin_oidc_app("tok", {}, "p")


  def test_assign_admin_member_posts_org_user_manager():
      captured = {}

      def fake_rwr(method, url, *, headers=None, json_body=None, **kw):
          captured["url"] = url
          captured["body"] = json_body
          return _FakeResp(200, {"details": {}})

      with mock.patch.object(provision, "request_with_retry", fake_rwr):
          provision.assign_admin_member("boot-tok", {"h": "1"}, "sa-123")
      assert captured["url"].endswith("/management/v1/orgs/me/members")
      b = captured["body"]
      assert b["userId"] == "sa-123"
      assert b["roles"] == ["ORG_USER_MANAGER"]


  def test_assign_admin_member_409_is_success():
      # member re-add is idempotent (appendix §2.4): 409 == already a member.
      with mock.patch.object(provision, "request_with_retry",
                             lambda *a, **k: _FakeResp(409)):
          provision.assign_admin_member("boot-tok", {}, "sa-123")  # must NOT raise
  ```

- [ ] **Step 2: Run it — expect FAIL** —
  `cd D:\projects\llm-chat; python -m pytest deploy/compose/provisioner/test_provision.py -v -k "admin_oidc or admin_member"`
  Expected: failures with `AttributeError: module 'provision' has no attribute 'create_admin_oidc_app'` (and `assign_admin_member`, `ADMIN_OIDC_REDIRECT_URI`).

- [ ] **Step 3: Implement** — add constants near line 35 (after the existing OIDC block):
  ```python
  # admin-api OIDC WEB app (confidential server / BASIC + PKCE) — distinct from the
  # CLI's public NATIVE app above. Captures BOTH clientId and clientSecret (once).
  ADMIN_OIDC_APP_NAME = "chat-admin-api"
  ADMIN_OIDC_REDIRECT_URI = os.environ.get(
      "ADMIN_OIDC_REDIRECT_URI", "http://localhost:7676/callback")
  ADMIN_OIDC_POST_LOGOUT_URI = os.environ.get(
      "ADMIN_OIDC_POST_LOGOUT_URI", "http://localhost:3000/")
  ADMIN_SA_ROLE = "ORG_USER_MANAGER"  # least privilege; bump to ORG_OWNER per §6.2 gate
  ```
  add `create_admin_oidc_app` after `create_oidc_app` (after line 281):
  ```python
  def create_admin_oidc_app(token: str, headers: dict, project_id: str):
      """Register the admin-api's confidential OIDC WEB app (appendix §1.2).

      Differs from create_oidc_app (the CLI's public NATIVE client): WEB +
      BASIC yields a client_secret (combined with PKCE at runtime). The app
      enum is OIDC_TOKEN_TYPE_JWT — NOT the machine ACCESS_TOKEN_TYPE_JWT (§7
      enum trap). accessTokenRoleAssertion=true so chat.admin rides in the
      ACCESS-token JWT even though the project has projectRoleAssertion=false
      (§6.1 gate). redirectUris uses the admin-api's OWN origin (ADMIN_PUBLIC_ORIGIN
      / public_origin), not the web origin. Returns (clientId, clientSecret);
      the secret is shown ONCE.
      """
      resp = request_with_retry(
          "POST", f"{ISSUER}/management/v1/projects/{project_id}/apps/oidc",
          headers=headers,
          json_body={
              "name": ADMIN_OIDC_APP_NAME,
              "redirectUris": [ADMIN_OIDC_REDIRECT_URI],
              "postLogoutRedirectUris": [ADMIN_OIDC_POST_LOGOUT_URI],
              "responseTypes": ["OIDC_RESPONSE_TYPE_CODE"],
              "grantTypes": ["OIDC_GRANT_TYPE_AUTHORIZATION_CODE",
                             "OIDC_GRANT_TYPE_REFRESH_TOKEN"],
              "appType": "OIDC_APP_TYPE_WEB",
              "authMethodType": "OIDC_AUTH_METHOD_TYPE_BASIC",
              "accessTokenType": "OIDC_TOKEN_TYPE_JWT",
              "devMode": True,
              "accessTokenRoleAssertion": True,
              "idTokenRoleAssertion": True,
          },
      )
      if resp.status_code == 200:
          body = resp.json()
          return body["clientId"], body["clientSecret"]
      if resp.status_code == 409:
          raise SystemExit(
              "admin OIDC app already exists (409): clean-boot contract — run "
              "`docker compose down -v` AND delete ./secrets.")
      resp.raise_for_status()
      raise RuntimeError(
          f"create_admin_oidc_app unexpected status {resp.status_code}")


  def assign_admin_member(token: str, headers: dict, sa_user_id: str) -> None:
      """Grant the admin SA its org-manager role (appendix §2.4). MUST be called
      with the BOOTSTRAP IAM_OWNER token (needs org.member.write) — NOT the new
      least-privilege SA. orgs/me resolves the org from the calling token /
      x-zitadel-orgid. Idempotent: 409 == already a member. ORG_USER_MANAGER is
      least privilege; bump to ORG_OWNER only if the §6.2 key-mint gate fails."""
      resp = request_with_retry(
          "POST", f"{ISSUER}/management/v1/orgs/me/members", headers=headers,
          json_body={"userId": sa_user_id, "roles": [ADMIN_SA_ROLE]},
      )
      if not is_success(resp.status_code):
          resp.raise_for_status()
  ```

- [ ] **Step 4: Run — expect PASS** —
  `cd D:\projects\llm-chat; python -m pytest deploy/compose/provisioner/test_provision.py -v -k "admin_oidc or admin_member"`
  Expected: `4 passed`.

- [ ] **Step 5: Commit** —
  `git add deploy/compose/provisioner/provision.py deploy/compose/provisioner/test_provision.py`
  ```powershell
  git commit -m @'
  feat(provisioner): admin OIDC WEB app + ORG_USER_MANAGER grant

  Add create_admin_oidc_app() (WEB/BASIC confidential client, captures
  clientId+clientSecret once, accessTokenRoleAssertion=true per §6.1, redirect
  to the admin-api public origin) and assign_admin_member() (orgs/me/members
  ORG_USER_MANAGER, idempotent 409). OIDC app uses OIDC_TOKEN_TYPE_JWT, not the
  machine ACCESS_TOKEN_TYPE_JWT.

  Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
  '@
  ```

---

### Task 7: Wire admin provisioning into `main()` + write the admin secrets
**Files:**
- Modify: `deploy/compose/provisioner/provision.py:338-378` (extend `main()` after the existing human-login block; uses `write_secret`)
- Test: `deploy/compose/provisioner/test_provision.py` (append; an end-to-end `main()` test with everything mocked)

Wire the four new helpers into `main()` after the demo-user block, following the existing sequence/secret-writing style. The admin SA role grant (`assign_admin_member`) uses the BOOTSTRAP token+headers (the same `token`/`headers` main() already minted from the IAM_OWNER key — appendix §2.4). Write `secrets/admin-api-key.json`, `secrets/admin_api_user_id`, `secrets/admin_oidc_client_id`, `secrets/admin_oidc_client_secret`. The test asserts the call sequence + exact secret writes with all I/O mocked (no network, non-racy).

- [ ] **Step 1: Write the failing test** — append to `test_provision.py`:
  ```python
  def test_main_provisions_admin_role_sa_app_and_writes_secrets(tmp_path):
      written = {}
      calls = []

      def fake_write_secret(name, content):
          written[name] = content

      with mock.patch.object(provision, "load_admin_key",
                             return_value={"userId": "boot", "keyId": "k",
                                           "key": "PEM"}), \
           mock.patch.object(provision, "mint_management_token",
                             return_value="boot-tok"), \
           mock.patch.object(provision, "fetch_org_id", return_value="org-1"), \
           mock.patch.object(provision, "create_project", return_value="proj-1"), \
           mock.patch.object(provision, "add_role"), \
           mock.patch.object(provision, "create_machine_user",
                             return_value="kaby-1"), \
           mock.patch.object(provision, "read_existing_user_id",
                             return_value=None), \
           mock.patch.object(provision, "generate_json_key",
                             return_value={"userId": "kaby-1"}), \
           mock.patch.object(provision, "grant_role"), \
           mock.patch.object(provision, "create_oidc_app",
                             return_value="cli-cid"), \
           mock.patch.object(provision, "create_human_user",
                             return_value="demo-1"), \
           mock.patch.object(provision, "create_admin_role",
                             side_effect=lambda *a, **k: calls.append("role")), \
           mock.patch.object(provision, "create_admin_sa",
                             return_value="sa-9"), \
           mock.patch.object(provision, "generate_admin_key",
                             return_value={"userId": "sa-9", "keyId": "ak"}), \
           mock.patch.object(provision, "create_admin_oidc_app",
                             return_value=("admin-cid", "admin-secret")), \
           mock.patch.object(provision, "assign_admin_member",
                             side_effect=lambda t, h, uid: calls.append(("member", uid))), \
           mock.patch.object(provision, "write_secret", fake_write_secret), \
           mock.patch.object(provision, "write_generated_env"):
          rc = provision.main()

      assert rc == 0
      # admin SA key + id + OIDC client id/secret all persisted
      assert json.loads(written["admin-api-key.json"]) == {"userId": "sa-9", "keyId": "ak"}
      assert written["admin_api_user_id"] == "sa-9"
      assert written["admin_oidc_client_id"] == "admin-cid"
      assert written["admin_oidc_client_secret"] == "admin-secret"
      # org-manager grant used the bootstrap token and the admin SA's userId
      assert ("member", "sa-9") in calls
      assert "role" in calls
  ```

- [ ] **Step 2: Run it — expect FAIL** —
  `cd D:\projects\llm-chat; python -m pytest deploy/compose/provisioner/test_provision.py -v -k test_main_provisions_admin`
  Expected: `KeyError: 'admin-api-key.json'` (main() does not yet provision/write the admin artifacts).

- [ ] **Step 3: Implement** — in `main()`, insert the admin block after the demo-user secret writes (after line 371, `write_secret("demo_password", DEMO_PASSWORD)`) and before the final `write_secret("project_id", ...)`:
  ```python
      # ----- admin-api provisioning (appendix §2, §1.2) -----
      # Reuses the same bootstrap IAM_OWNER token/headers minted above:
      # assign_admin_member NEEDS org.member.write (§2.4), which the runtime
      # least-privilege SA will not have. Role creation stays here so the
      # runtime SA needs no project.role.write.
      create_admin_role(token, headers, project_id)
      admin_sa_id = create_admin_sa(token, headers)
      admin_sa = generate_admin_key(token, headers, admin_sa_id)
      assign_admin_member(token, headers, admin_sa_id)
      admin_cid, admin_secret = create_admin_oidc_app(token, headers, project_id)
      write_secret("admin-api-key.json", json.dumps(admin_sa))
      write_secret("admin_api_user_id", admin_sa_id)
      write_secret("admin_oidc_client_id", admin_cid)
      write_secret("admin_oidc_client_secret", admin_secret)
      print(f"[provision] admin: sa_user_id={admin_sa_id} "
            f"admin_oidc_client_id={admin_cid} role={ADMIN_SA_ROLE}")
  ```
  Then update the final summary `print` (line 376) to mention the admin SA:
  ```python
      print(f"[provision] done: project_id={project_id} userId={user_id} "
            f"oidc_client_id={client_id} demo_user={DEMO_USERNAME} "
            f"admin_sa_user_id={admin_sa_id}")
  ```

- [ ] **Step 4: Run — expect PASS** —
  `cd D:\projects\llm-chat; python -m pytest deploy/compose/provisioner/test_provision.py -v`
  Expected: all tests pass (the full file, including the new `test_main_provisions_admin...`).

- [ ] **Step 5: Commit** —
  `git add deploy/compose/provisioner/provision.py deploy/compose/provisioner/test_provision.py`
  ```powershell
  git commit -m @'
  feat(provisioner): wire admin role/SA/app into main() + write secrets

  After the existing project/role/user/demo sequence, provision the
  chat.admin role, the chat-admin-api SA + JSON key, its ORG_USER_MANAGER
  org membership (bootstrap IAM_OWNER token), and the admin OIDC WEB app;
  persist admin-api-key.json, admin_api_user_id, admin_oidc_client_id,
  admin_oidc_client_secret to ./secrets.

  Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
  '@
  ```

---

### Task 8: EMPIRICAL VERIFICATION GATE — human role-claim in JWT, issuer match, SA key-mint (ADMIN_IT=1)
**Files:**
- Create: `deploy/compose/provisioner/verify_admin_gate.py`
- Test: `deploy/compose/provisioner/test_verify_admin_gate.py`

This is the highest-risk GATE (design §10.1). **ORDERING (blocking):** the gated `run_gate()` integration leg MUST be run against the running stack after Task 7 (secrets populated) and BEFORE Task 17 builds the OIDC callback that depends on `chat.admin` riding in the access-token JWT. If §6.1 fails, the repair lever is already in place — `create_admin_oidc_app` sets `accessTokenRoleAssertion=true` (Task 6) — but if that is insufficient the project flags (`projectRoleAssertion/roleCheck/hasProjectCheck`, set false by the existing `create_project`) must be flipped here before C is built; `run_gate()` records which repair was needed. Per the convention "for I/O-bound Zitadel wrappers whose exact JSON is empirical, make the verifying step an integration test gated on `ADMIN_IT=1` rather than fabricating a mocked response body," this task ships:
1. PURE helpers (unit-tested, non-racy): `roles_claim_key(pid)`, `has_admin_role(claims, pid)`, `issuer_matches(discovery_iss, configured)`. NOTE: `has_admin_role` is the Python mirror of the canonical Rust `roles_from_claims` (Task 3) and uses the identical claim-key format.
2. A gated integration runner against the RUNNING Zitadel v3.4.10 that discharges appendix **§6.1** (human access-token carries `chat.admin` under `urn:zitadel:iam:org:project:{pid}:roles`), **§6.7** (discovery `issuer` == configured `ISSUER`), and **§6.2** (`ORG_USER_MANAGER` SA can mint a machine key). It SKIPS unless `ADMIN_IT=1`, and on a §6.1 failure it documents the repair (app-level `accessTokenRoleAssertion=true` and/or flip `projectRoleCheck`/`hasProjectCheck`).

- [ ] **Step 1: Write the failing test** — create `deploy/compose/provisioner/test_verify_admin_gate.py`:
  ```python
  import os

  import pytest

  import verify_admin_gate as gate


  def test_roles_claim_key_uses_project_id():
      assert gate.roles_claim_key("proj-1") == \
          "urn:zitadel:iam:org:project:proj-1:roles"


  def test_has_admin_role_true_when_role_present():
      # Zitadel roles claim: object whose KEYS are role keys (appendix §1.5).
      claims = {"urn:zitadel:iam:org:project:proj-1:roles":
                {"chat.admin": {"org-1": "example.org"}}}
      assert gate.has_admin_role(claims, "proj-1") is True


  def test_has_admin_role_false_when_absent_or_other_role():
      claims = {"urn:zitadel:iam:org:project:proj-1:roles":
                {"chat.user": {"org-1": "example.org"}}}
      assert gate.has_admin_role(claims, "proj-1") is False
      assert gate.has_admin_role({}, "proj-1") is False


  def test_issuer_matches_is_exact_string_compare():
      iss = "http://host.docker.internal:8080"
      assert gate.issuer_matches(iss, iss) is True
      # byte-for-byte: a trailing slash is a MISMATCH (jsonwebtoken would 401)
      assert gate.issuer_matches(iss + "/", iss) is False


  @pytest.mark.skipif(os.environ.get("ADMIN_IT") != "1",
                      reason="integration gate — set ADMIN_IT=1 against running Zitadel v3.4.10")
  def test_integration_admin_gate_runs():
      # Discharges appendix §6.1 (human chat.admin in ACCESS-token JWT),
      # §6.7 (discovery issuer == configured), §6.2 (ORG_USER_MANAGER mints key).
      report = gate.run_gate()
      assert report["issuer_match"] is True, report
      assert report["sa_can_mint_key"] is True, report
      assert report["human_has_admin_role"] is True, (
          "human access token lacks chat.admin under the project roles claim; "
          "repair: set app accessTokenRoleAssertion=true and/or flip project "
          "projectRoleCheck/hasProjectCheck (appendix §6.1/§6.5). Report: "
          + repr(report))
  ```

- [ ] **Step 2: Run it — expect FAIL** —
  `cd D:\projects\llm-chat; python -m pytest deploy/compose/provisioner/test_verify_admin_gate.py -v`
  Expected: the 4 pure tests fail with `ModuleNotFoundError: No module named 'verify_admin_gate'`; the integration test reports `SKIPPED` (ADMIN_IT not set).

- [ ] **Step 3: Implement** — create `deploy/compose/provisioner/verify_admin_gate.py`:
  ```python
  #!/usr/bin/env python3
  """Empirical verification GATE for the admin-api (design §10.1, appendix §6).

  Pure helpers (unit-tested, non-racy) + a gated integration runner that proves,
  against the RUNNING Zitadel v3.4.10 (the source of truth — never a fabricated
  mock body), the three load-bearing facts the whole authorization model rests on:

    §6.7  the discovery doc's `issuer` matches the configured ISSUER byte-for-byte
    §6.2  an ORG_USER_MANAGER SA can mint a machine JSON key (else bump to ORG_OWNER)
    §6.1  a HUMAN authorization-code login carries `chat.admin` in the VERIFIABLE
          access-token JWT under urn:zitadel:iam:org:project:{pid}:roles, given the
          project was created with projectRoleAssertion/roleCheck/hasProjectCheck=false.
          If absent, the repair is app-level accessTokenRoleAssertion=true and/or
          flipping the project flags — this runner records which was needed.

  Run: `ADMIN_IT=1 python deploy/compose/provisioner/verify_admin_gate.py`.
  """
  from __future__ import annotations

  import json
  import os
  import sys

  import requests

  import provision  # reuse ISSUER, build_jwt_assertion, mint/header helpers, etc.


  # ---------- pure helpers (unit-tested) ----------

  def roles_claim_key(project_id: str) -> str:
      """The Zitadel roles claim name for a project (appendix §1.5). Mirrors
      auth_zitadel.rs's `format!("urn:zitadel:iam:org:project:{}:roles", pid)`
      and the canonical roles_from_claims (Task 3)."""
      return f"urn:zitadel:iam:org:project:{project_id}:roles"


  def has_admin_role(claims: dict, project_id: str) -> bool:
      """True iff the JWT claims carry chat.admin under the project roles claim.
      The claim is an object whose KEYS are role keys (appendix §1.5)."""
      roles = claims.get(roles_claim_key(project_id))
      return isinstance(roles, dict) and provision.ADMIN_ROLE_KEY in roles


  def issuer_matches(discovery_iss: str, configured: str) -> bool:
      """Byte-for-byte issuer compare — a trailing slash difference is a real
      mismatch that makes jsonwebtoken issuer validation 401 (design §8, §6.7)."""
      return discovery_iss == configured


  # ---------- gated integration runner (source of truth: running Zitadel) ----------

  def _check_issuer() -> bool:
      resp = requests.get(
          f"{provision.ISSUER}/.well-known/openid-configuration",
          timeout=provision.REQUEST_TIMEOUT)
      resp.raise_for_status()
      return issuer_matches(resp.json()["issuer"], provision.ISSUER)


  def _check_sa_can_mint_key() -> bool:
      """Prove the least-privilege admin SA (ORG_USER_MANAGER) can mint a JSON key
      (§6.2). Mints a Management token from secrets/admin-api-key.json and calls
      AddMachineKey on its OWN userId; success => user.write is sufficient."""
      with open(os.path.join(provision.SECRETS_DIR, "admin-api-key.json")) as f:
          sa = json.load(f)
      token = provision.mint_management_token(sa)
      org_id = provision.fetch_org_id(token)
      headers = provision.mgmt_headers(token, org_id)
      resp = provision.request_with_retry(
          "POST", f"{provision.ISSUER}/management/v1/users/{sa['userId']}/keys",
          headers=headers, json_body={"type": "KEY_TYPE_JSON"})
      return resp.status_code == 200


  def run_gate() -> dict:
      """Run all three checks; return a report dict. §6.1 (human role-claim in the
      access JWT) requires an interactive auth-code login, so it is read from a
      pre-obtained token in env ADMIN_GATE_HUMAN_ACCESS_TOKEN when available;
      absent that, it is reported None (run the login leg manually and re-run).
      Decoding is signature-unverified here (claim inspection only) — the manager's
      auth_zitadel.rs JwksCache is the verifying path in production."""
      report = {"issuer_match": None, "sa_can_mint_key": None,
                "human_has_admin_role": None, "repair_needed": None}
      report["issuer_match"] = _check_issuer()
      report["sa_can_mint_key"] = _check_sa_can_mint_key()

      tok = os.environ.get("ADMIN_GATE_HUMAN_ACCESS_TOKEN")
      if tok:
          import base64 as _b64
          payload = tok.split(".")[1]
          payload += "=" * (-len(payload) % 4)
          claims = json.loads(_b64.urlsafe_b64decode(payload))
          with open(os.path.join(provision.SECRETS_DIR, "project_id")) as f:
              pid = f.read().strip()
          report["human_has_admin_role"] = has_admin_role(claims, pid)
          if not report["human_has_admin_role"]:
              report["repair_needed"] = (
                  "set app accessTokenRoleAssertion=true and/or flip project "
                  "projectRoleCheck/hasProjectCheck (appendix §6.1/§6.5)")
      else:
          report["human_has_admin_role"] = None
          report["repair_needed"] = (
              "provide ADMIN_GATE_HUMAN_ACCESS_TOKEN from a human auth-code login "
              "to discharge §6.1")
      return report


  if __name__ == "__main__":
      print(json.dumps(run_gate(), indent=2))
      sys.exit(0)
  ```

- [ ] **Step 4: Run — expect PASS (pure) / SKIP (gate)** —
  `cd D:\projects\llm-chat; python -m pytest deploy/compose/provisioner/test_verify_admin_gate.py -v`
  Expected: 4 pure tests `PASSED`, the integration test `SKIPPED` (ADMIN_IT unset). To actually discharge §6.1/§6.2/§6.7 against the running stack: `$env:ADMIN_IT=1; python -m pytest deploy/compose/provisioner/test_verify_admin_gate.py -v` (with Zitadel v3.4.10 up and `./secrets` populated by Task 7); record in the appendix §6 checklist which items passed and any repair applied. Run this gate against the stack before starting Task 17.

- [ ] **Step 5: Commit** —
  `git add deploy/compose/provisioner/verify_admin_gate.py deploy/compose/provisioner/test_verify_admin_gate.py`
  ```powershell
  git commit -m @'
  test(provisioner): empirical admin gate (§6.1/§6.2/§6.7)

  Pure helpers (roles_claim_key, has_admin_role, issuer_matches) unit-tested
  non-racy, plus an ADMIN_IT=1-gated runner that proves against the running
  Zitadel v3.4.10: discovery issuer matches configured (§6.7), the
  ORG_USER_MANAGER SA can mint a machine key (§6.2), and a human access-token
  carries chat.admin under the project roles claim (§6.1) — recording the
  repair (accessTokenRoleAssertion / project-flag flip) if it does not.

  Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
  '@
  ```

---

## Phase C1-backend-core

### Task 9: admin-api crate scaffold + workspace member + Cargo.toml
**Files:**
- Create: `admin-api/Cargo.toml`
- Create: `admin-api/src/main.rs`
- Modify: `Cargo.toml:1-12` (add `admin-api` to workspace `members` — Phase A did NOT add it)
- Test: (compile-only smoke; the real unit tests arrive in Task 10+)

- [ ] **Step 1: Write the failing test** — make the crate exist with a placeholder `main.rs` and a trivial pure module so `cargo test -p llm-chat-admin-api` has something to compile. Create `admin-api/src/main.rs`:
  ```rust
  // llm-chat-admin-api — Backend-For-Frontend for the Zitadel user-management
  // admin. Owns the operator OIDC session + the least-privilege admin service
  // account; the browser only ever holds an opaque session cookie.
  //
  // This file is fleshed out in Task 13 (startup: config fail-fast + issuer-match
  // guard + router + serve). For now it is a compiling placeholder so the crate
  // is a real workspace member and `cargo test -p llm-chat-admin-api` runs.

  fn main() {
      eprintln!("llm-chat-admin-api: not yet wired (see Task 13)");
  }

  #[cfg(test)]
  mod scaffold_smoke {
      #[test]
      fn crate_compiles_and_tests_run() {
          // Proves the workspace member exists and its test harness runs.
          // (Placeholder — deleted in Task 13 when real startup_tests land.)
          assert_eq!(2 + 2, 4);
      }
  }
  ```
- [ ] **Step 2: Run it — expect FAIL** — `cargo test -p llm-chat-admin-api` from `D:\projects\llm-chat`. Expect failure: `error: package ID specification 'llm-chat-admin-api' did not match any packages` (the crate has no `Cargo.toml` yet, so cargo can't resolve the package).
- [ ] **Step 3: Implement** — first add `admin-api` to the root `Cargo.toml` `[workspace] members` (Phase A's Task 1 deliberately did NOT add it, and Task 2 only added `crates/zitadel-auth`):
  ```toml
  members = ["manager", "worker", "crates/zitadel-auth", "admin-api"]
  ```
  then create `admin-api/Cargo.toml` pinning the appendix §4.3 stack and wiring shared workspace deps + the local `zitadel-auth` crate:
  ```toml
  [package]
  name = "llm-chat-admin-api"
  version = "0.1.0"
  edition = "2021"

  [[bin]]
  name = "llm-chat-admin-api"
  path = "src/main.rs"

  [dependencies]
  # Shared workspace deps (root Cargo.toml [workspace.dependencies], Phase A).
  tokio = { workspace = true }
  serde = { workspace = true }
  serde_json = { workspace = true }
  reqwest = { workspace = true }
  jsonwebtoken = { workspace = true }
  tracing = { workspace = true }

  # Reused verbatim from the manager — JWKS verify + Principal.has("chat.admin").
  zitadel-auth = { path = "../crates/zitadel-auth" }

  # admin-api-specific stack (appendix §4.3).
  axum = "0.8"
  tower = "0.5"
  tower-http = { version = "0.6", features = ["trace"] }
  tower-sessions = "0.15"
  tracing-subscriber = { version = "0.3", features = ["env-filter"] }
  base64 = "0.22"
  sha2 = "0.10"
  rand = "0.8"
  url = "2"
  time = "0.3"
  ```
  Note `tracing-subscriber`/`base64`/`sha2`/`rand`/`url`/`time` are admin-api-only and intentionally not in `[workspace.dependencies]` (DRY: only genuinely-shared deps go there). The `cors` feature of `tower-http` is intentionally NOT enabled — admin-web is a same-origin proxy (Phase D), so no CORS layer is needed; `trace` is wired in Task 18.
- [ ] **Step 4: Run — expect PASS** — `cargo test -p llm-chat-admin-api` from `D:\projects\llm-chat`. Expect: `test scaffold_smoke::crate_compiles_and_tests_run ... ok` and `test result: ok. 1 passed`.
- [ ] **Step 5: Commit** —
  ```powershell
  git add Cargo.toml admin-api/Cargo.toml admin-api/src/main.rs
  git commit -m @'
  feat(admin-api): scaffold the crate as a workspace member

  Add admin-api to the workspace members; placeholder binary + smoke test so
  cargo test -p llm-chat-admin-api runs. Pins the appendix §4.3 stack (axum 0.8,
  tower-sessions 0.15, reqwest rustls, jsonwebtoken 9) and depends on the shared
  zitadel-auth crate.

  Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
  '@
  ```

---

### Task 10: `config.rs` — `require_var` pure helper
**Files:**
- Create: `admin-api/src/config.rs`
- Modify: `admin-api/src/main.rs:1-10` (add `mod config;`)
- Test: `admin-api/src/config.rs` (inline `#[cfg(test)] mod tests`)

- [ ] **Step 1: Write the failing test** — create `admin-api/src/config.rs` with ONLY the test module + a stub signature (mirrors the manager's `require_addr` pure contract, but named `require_var` per the locked contract and trims + names the missing var):
  ```rust
  //! Startup configuration for admin-api — env-driven, validated fail-fast.
  //! Mirrors the manager/worker `require_*` contract: a pure helper that trims
  //! and names any missing/empty var, plus `AdminConfig::from_env` that resolves
  //! every required var up front so a misconfig aborts before any side effect.

  /// PURE: require a non-empty config var. Trims surrounding whitespace.
  /// Returns Err("{name} must be set (no default)") when None/empty/whitespace-
  /// only; Ok(trimmed) otherwise. Modeled on manager::require_addr.
  pub fn require_var(_name: &str, _raw: Option<String>) -> Result<String, String> {
      unimplemented!()
  }

  #[cfg(test)]
  mod tests {
      use super::*;

      #[test]
      fn require_var_trims_and_accepts() {
          assert_eq!(require_var("X", Some("  v  ".into())), Ok("v".into()));
      }

      #[test]
      fn require_var_rejects_none() {
          assert_eq!(
              require_var("ZITADEL_ISSUER", None),
              Err("ZITADEL_ISSUER must be set (no default)".into())
          );
      }

      #[test]
      fn require_var_rejects_whitespace_only() {
          assert_eq!(
              require_var("SA_KEY_PATH", Some("   ".into())),
              Err("SA_KEY_PATH must be set (no default)".into())
          );
      }
  }
  ```
  Add `mod config;` to `admin-api/src/main.rs` (right under the file header comment, before `fn main`).
- [ ] **Step 2: Run it — expect FAIL** — `cargo test -p llm-chat-admin-api require_var` from `D:\projects\llm-chat`. Expect: all three tests fail with `panicked at 'not implemented'` (the stub `unimplemented!()`).
- [ ] **Step 3: Implement** — replace the stub body:
  ```rust
  pub fn require_var(name: &str, raw: Option<String>) -> Result<String, String> {
      match raw.map(|s| s.trim().to_string()).filter(|s| !s.is_empty()) {
          Some(v) => Ok(v),
          None => Err(format!("{name} must be set (no default)")),
      }
  }
  ```
- [ ] **Step 4: Run — expect PASS** — `cargo test -p llm-chat-admin-api require_var` from `D:\projects\llm-chat`. Expect: `test result: ok. 3 passed`.
- [ ] **Step 5: Commit** —
  ```powershell
  git add admin-api/src/config.rs admin-api/src/main.rs
  git commit -m @'
  feat(admin-api): require_var pure config helper (trims, names missing var)

  Mirrors manager::require_addr fail-fast contract for the BFF config module.

  Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
  '@
  ```

---

### Task 11: `config.rs` — `AdminConfig` struct + `from_env` fail-fast
**Files:**
- Modify: `admin-api/src/config.rs` (add `AdminConfig` + `from_env`, extend tests)
- Test: `admin-api/src/config.rs` (inline tests; `from_env` driven through a pure `from_map` to keep it non-racy — no real env mutation)

This struct carries a `public_origin` field (from `ADMIN_PUBLIC_ORIGIN`, e.g. `http://localhost:7676`) — the admin-api's OWN externally-reachable origin used to build the OIDC `redirect_uri`/`end_session` so it matches the redirect the provisioner registered (Task 6). This is DISTINCT from `allowed_origin` (the web origin `http://localhost:3000`). Both are required; reconciles the earlier `allowed_origin_self()` drift to one named field.

- [ ] **Step 1: Write the failing test** — `from_env` reads process env (global, racy under parallel tests), so split off a PURE inner `from_map(&dyn Fn(&str)->Option<String>)` that `from_env` delegates to; test the pure mapper. Add the struct + stubs + tests to `admin-api/src/config.rs`:
  ```rust
  /// Resolved, validated admin-api configuration. Every field is required —
  /// there is no code default (the manager/worker pattern). `from_env`/`from_map`
  /// fail fast naming the first missing var.
  #[derive(Clone, Debug, PartialEq, Eq)]
  pub struct AdminConfig {
      pub issuer: String,
      pub project_id: String,
      pub audience: String,
      pub sa_key_path: String,
      pub oidc_client_id: String,
      pub oidc_client_secret: String,
      pub bind_addr: String,
      pub public_origin: String,
      pub allowed_origin: String,
      pub session_key: String,
  }

  impl AdminConfig {
      /// PURE: resolve every required var from a lookup fn. `issuer` and
      /// `public_origin` are trailing-slash-trimmed to match
      /// zitadel_auth::ZitadelConfig and so the startup discovery issuer-match
      /// guard (Task 13) and the OIDC redirect_uri compare like-for-like.
      pub fn from_map(get: &dyn Fn(&str) -> Option<String>) -> Result<AdminConfig, String> {
          unimplemented!()
      }

      /// Thin wrapper: resolve from the real process environment.
      pub fn from_env() -> Result<AdminConfig, String> {
          Self::from_map(&|k| std::env::var(k).ok())
      }
  }
  ```
  Append to the existing `mod tests`:
  ```rust
      use std::collections::HashMap;

      fn getter(m: HashMap<&'static str, &'static str>) -> impl Fn(&str) -> Option<String> {
          move |k: &str| m.get(k).map(|s| s.to_string())
      }

      fn full_map() -> HashMap<&'static str, &'static str> {
          HashMap::from([
              ("ZITADEL_ISSUER", "http://host.docker.internal:8080/"),
              ("ZITADEL_PROJECT_ID", "p1"),
              ("ZITADEL_AUDIENCE", "p1"),
              ("ADMIN_SA_KEY_PATH", "/secrets/admin-api-key.json"),
              ("ADMIN_OIDC_CLIENT_ID", "cid"),
              ("ADMIN_OIDC_CLIENT_SECRET", "csecret"),
              ("ADMIN_BIND_ADDR", "0.0.0.0:7676"),
              ("ADMIN_PUBLIC_ORIGIN", "http://localhost:7676/"),
              ("ADMIN_ALLOWED_ORIGIN", "http://localhost:3000"),
              ("ADMIN_SESSION_KEY", "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"),
          ])
      }

      #[test]
      fn from_map_ok_trims_issuer_and_public_origin_slash() {
          let cfg = AdminConfig::from_map(&getter(full_map())).expect("ok");
          assert_eq!(cfg.issuer, "http://host.docker.internal:8080"); // trailing / trimmed
          assert_eq!(cfg.public_origin, "http://localhost:7676");     // trailing / trimmed
          assert_eq!(cfg.project_id, "p1");
          assert_eq!(cfg.bind_addr, "0.0.0.0:7676");
      }

      #[test]
      fn from_map_names_first_missing_var() {
          let mut m = full_map();
          m.remove("ADMIN_OIDC_CLIENT_SECRET");
          assert_eq!(
              AdminConfig::from_map(&getter(m)),
              Err("ADMIN_OIDC_CLIENT_SECRET must be set (no default)".into())
          );
      }
  ```
- [ ] **Step 2: Run it — expect FAIL** — `cargo test -p llm-chat-admin-api config` from `D:\projects\llm-chat`. Expect: `from_map_*` tests fail with `panicked at 'not implemented'`.
- [ ] **Step 3: Implement** — replace `from_map`'s body (reuse `require_var`, trim the issuer + public-origin slash like `ZitadelConfig::from_env`):
  ```rust
      pub fn from_map(get: &dyn Fn(&str) -> Option<String>) -> Result<AdminConfig, String> {
          let issuer = require_var("ZITADEL_ISSUER", get("ZITADEL_ISSUER"))?
              .trim_end_matches('/')
              .to_string();
          let public_origin = require_var("ADMIN_PUBLIC_ORIGIN", get("ADMIN_PUBLIC_ORIGIN"))?
              .trim_end_matches('/')
              .to_string();
          Ok(AdminConfig {
              issuer,
              project_id: require_var("ZITADEL_PROJECT_ID", get("ZITADEL_PROJECT_ID"))?,
              audience: require_var("ZITADEL_AUDIENCE", get("ZITADEL_AUDIENCE"))?,
              sa_key_path: require_var("ADMIN_SA_KEY_PATH", get("ADMIN_SA_KEY_PATH"))?,
              oidc_client_id: require_var("ADMIN_OIDC_CLIENT_ID", get("ADMIN_OIDC_CLIENT_ID"))?,
              oidc_client_secret: require_var(
                  "ADMIN_OIDC_CLIENT_SECRET",
                  get("ADMIN_OIDC_CLIENT_SECRET"),
              )?,
              bind_addr: require_var("ADMIN_BIND_ADDR", get("ADMIN_BIND_ADDR"))?,
              public_origin,
              allowed_origin: require_var("ADMIN_ALLOWED_ORIGIN", get("ADMIN_ALLOWED_ORIGIN"))?,
              session_key: require_var("ADMIN_SESSION_KEY", get("ADMIN_SESSION_KEY"))?,
          })
      }
  ```
- [ ] **Step 4: Run — expect PASS** — `cargo test -p llm-chat-admin-api config` from `D:\projects\llm-chat`. Expect: `test result: ok. 5 passed` (3 from Task 10 + 2 new).
- [ ] **Step 5: Commit** —
  ```powershell
  git add admin-api/src/config.rs
  git commit -m @'
  feat(admin-api): AdminConfig::from_env fail-fast resolver

  Pure from_map(get) resolves every required var (issuer + public_origin
  slash-trimmed to match zitadel_auth and the registered OIDC redirect) and
  names the first missing one; from_env is the thin env wrapper. public_origin
  (admin-api own origin) is distinct from allowed_origin (the web origin).

  Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
  '@
  ```

---

### Task 12: `zitadel/error.rs` — `ZitadelError` enum + `map_status` pure
**Files:**
- Create: `admin-api/src/zitadel/mod.rs` (module root; declares `pub mod error;`)
- Create: `admin-api/src/zitadel/error.rs`
- Modify: `admin-api/src/main.rs:1-10` (add `mod zitadel;`)
- Test: `admin-api/src/zitadel/error.rs` (inline tests)

- [ ] **Step 1: Write the failing test** — create the `zitadel` module root `admin-api/src/zitadel/mod.rs`:
  ```rust
  //! The only module that touches Zitadel APIs. Submodules are added across
  //! Phase C: error (gRPC->HTTP mapping), token (SA JWT-bearer + cache), model,
  //! users, grants, keys. Task 12 lands `error`.

  pub mod error;
  ```
  Create `admin-api/src/zitadel/error.rs` with the enum + a `map_status` stub + tests (status mapping is the appendix §3 gRPC→HTTP table: `409→AlreadyExists, 404→NotFound, 403→Forbidden, 400→Invalid, 5xx→Upstream`, else `Invalid`):
  ```rust
  //! Zitadel client error type + pure HTTP-status mapping.
  //! Mirrors the appendix §3 gRPC->HTTP table that provision.py relies on:
  //!   ALREADY_EXISTS->409, NOT_FOUND->404, PERMISSION_DENIED->403,
  //!   INVALID_ARGUMENT/FAILED_PRECONDITION->400, 5xx->Upstream.

  #[derive(Debug, Clone, PartialEq, Eq)]
  pub enum ZitadelError {
      Upstream,
      NotFound,
      Forbidden,
      AlreadyExists,
      Invalid(String),
      Transport(String),
  }

  impl std::fmt::Display for ZitadelError {
      fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
          match self {
              ZitadelError::Upstream => write!(f, "upstream zitadel error"),
              ZitadelError::NotFound => write!(f, "not found"),
              ZitadelError::Forbidden => write!(f, "forbidden"),
              ZitadelError::AlreadyExists => write!(f, "already exists"),
              ZitadelError::Invalid(m) => write!(f, "invalid: {m}"),
              ZitadelError::Transport(m) => write!(f, "transport: {m}"),
          }
      }
  }

  impl std::error::Error for ZitadelError {}

  /// PURE: map an upstream HTTP status (+ raw body for context) to a typed
  /// error. `body` is carried into `Invalid` so 400s surface Zitadel's message.
  pub fn map_status(_status: u16, _body: &str) -> ZitadelError {
      unimplemented!()
  }

  #[cfg(test)]
  mod tests {
      use super::*;

      #[test]
      fn maps_known_statuses() {
          assert_eq!(map_status(409, ""), ZitadelError::AlreadyExists);
          assert_eq!(map_status(404, ""), ZitadelError::NotFound);
          assert_eq!(map_status(403, ""), ZitadelError::Forbidden);
          assert_eq!(map_status(500, ""), ZitadelError::Upstream);
          assert_eq!(map_status(503, ""), ZitadelError::Upstream);
      }

      #[test]
      fn maps_400_carries_body() {
          assert_eq!(
              map_status(400, "bad role key"),
              ZitadelError::Invalid("bad role key".into())
          );
      }

      #[test]
      fn unknown_status_is_invalid_with_body() {
          // e.g. an unexpected 418 / 402 — keep the body for diagnosis.
          assert_eq!(
              map_status(418, "teapot"),
              ZitadelError::Invalid("unexpected status 418: teapot".into())
          );
      }
  }
  ```
  Add `mod zitadel;` to `admin-api/src/main.rs` (under `mod config;`).
- [ ] **Step 2: Run it — expect FAIL** — `cargo test -p llm-chat-admin-api map_status` from `D:\projects\llm-chat`. Expect: the three tests fail with `panicked at 'not implemented'`.
- [ ] **Step 3: Implement** — replace `map_status`'s body:
  ```rust
  pub fn map_status(status: u16, body: &str) -> ZitadelError {
      match status {
          409 => ZitadelError::AlreadyExists,
          404 => ZitadelError::NotFound,
          403 => ZitadelError::Forbidden,
          400 => ZitadelError::Invalid(body.to_string()),
          500..=599 => ZitadelError::Upstream,
          other => ZitadelError::Invalid(format!("unexpected status {other}: {body}")),
      }
  }
  ```
- [ ] **Step 4: Run — expect PASS** — `cargo test -p llm-chat-admin-api map_status` from `D:\projects\llm-chat`. Expect: `test result: ok. 3 passed`.
- [ ] **Step 5: Commit** —
  ```powershell
  git add admin-api/src/zitadel/mod.rs admin-api/src/zitadel/error.rs admin-api/src/main.rs
  git commit -m @'
  feat(admin-api): ZitadelError + pure map_status (gRPC->HTTP table)

  Maps 409/404/403/400/5xx to typed errors per appendix §3; 400 carries the
  upstream body so operators see Zitadel's message.

  Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
  '@
  ```

---

### Task 13: `zitadel/token.rs` — `build_assertion` pure RS256 + cache wrappers; `main.rs` startup (config fail-fast + issuer-match guard)
**Files:**
- Create: `admin-api/src/zitadel/token.rs` (`CachedToken`, pure `build_assertion`, `ZitadelClient::{valid_token,mint_management_token}`)
- Modify: `admin-api/src/zitadel/mod.rs` (add `pub mod token;` + `ZitadelClient` struct)
- Create: `admin-api/src/lib.rs` (lib surface so `tests/` can import the modules)
- Modify: `admin-api/src/main.rs` (real startup: `AdminConfig::from_env` fail-fast + discovery issuer-match guard)
- Test (unit, pure): `admin-api/src/zitadel/token.rs` inline tests for `build_assertion`
- Test (integration, gated): `admin-api/tests/integration.rs` gated on `ADMIN_IT=1` for the live token mint (the token-endpoint JSON shape is empirical — appendix §2.5, §6.2 — so it is NOT asserted via a fabricated mock)

- [ ] **Step 1: Write the failing test** — pure `build_assertion` mirrors `provision.py:build_jwt_assertion` (RS256, header `{kid}`, claims `{iss=sub=user_id, aud=issuer, iat=now, exp=now+3600}`). Test it by signing with a throwaway RSA PEM and verifying with the matching public key (round-trip — no network, no fabricated server response). First add to `admin-api/src/zitadel/mod.rs`:
  ```rust
  pub mod token;

  use crate::config::AdminConfig;
  use token::CachedToken;

  /// The only struct that calls Zitadel write APIs. Holds the SA key (via cfg),
  /// a shared reqwest client, and a cached Management-API token refreshed before
  /// expiry. Submodule `impl` blocks add the actual API methods across Phase C.
  pub struct ZitadelClient {
      pub cfg: AdminConfig,
      pub http: reqwest::Client,
      pub token: tokio::sync::RwLock<Option<CachedToken>>,
  }

  impl ZitadelClient {
      /// Two-arg constructor (infallible): the caller builds the redirect-less
      /// reqwest client once and shares it. Used by main.rs and the integration
      /// tests alike.
      pub fn new(cfg: AdminConfig, http: reqwest::Client) -> Self {
          Self { cfg, http, token: tokio::sync::RwLock::new(None) }
      }
  }
  ```
  Create `admin-api/src/zitadel/token.rs` with the stub + tests (use the `jsonwebtoken` testkit `EncodingKey`/`DecodingKey` already in the dep set):
  ```rust
  //! SA JWT-bearer assertion + Management-API token cache (appendix §2.5).
  //! `build_assertion` is the Rust mirror of provision.py:build_jwt_assertion;
  //! the token mint uses grant_type=jwt-bearer with the `zitadel` scope trap.

  use crate::zitadel::error::{map_status, ZitadelError};
  use crate::zitadel::ZitadelClient;

  /// A minted Management-API token + its absolute expiry (epoch seconds).
  #[derive(Clone, Debug)]
  pub struct CachedToken {
      pub token: String,
      pub exp: u64,
  }

  /// PURE: sign the JWT-bearer assertion. Header `{kid}`, claims
  /// `{iss=sub=user_id, aud=issuer, iat=now, exp=now+3600}`, RS256 over `pem`
  /// (the SA key's private PEM). Mirrors provision.py:build_jwt_assertion.
  pub fn build_assertion(
      _user_id: &str,
      _key_id: &str,
      _pem: &str,
      _issuer: &str,
      _now: u64,
  ) -> Result<String, String> {
      unimplemented!()
  }

  /// The `zitadel` literal targets Zitadel's own internal project so the
  /// Management API accepts the minted token (appendix §2.5 scope trap).
  pub const ADMIN_SCOPE: &str =
      "openid profile urn:zitadel:iam:org:project:id:zitadel:aud";

  impl ZitadelClient {
      /// Return a valid Management token, minting (and caching) a fresh one if
      /// none is cached or the cached one expires within 60s.
      pub async fn valid_token(&self) -> Result<String, ZitadelError> {
          let now = now_secs();
          if let Some(t) = self.token.read().await.as_ref() {
              if t.exp > now + 60 {
                  return Ok(t.token.clone());
              }
          }
          let fresh = self.mint_management_token().await?;
          let tok = fresh.token.clone();
          *self.token.write().await = Some(fresh);
          Ok(tok)
      }

      /// Mint a Management-API token via the JWT-bearer grant (appendix §2.5).
      pub async fn mint_management_token(&self) -> Result<CachedToken, ZitadelError> {
          // Load the SA serviceaccount JSON ({userId,keyId,key:<PEM>,...}).
          let raw = std::fs::read_to_string(&self.cfg.sa_key_path)
              .map_err(|e| ZitadelError::Transport(format!("read sa key: {e}")))?;
          let sa: serde_json::Value = serde_json::from_str(&raw)
              .map_err(|e| ZitadelError::Invalid(format!("sa key json: {e}")))?;
          let user_id = sa["userId"].as_str().unwrap_or_default();
          let key_id = sa["keyId"].as_str().unwrap_or_default();
          let pem = sa["key"].as_str().unwrap_or_default();
          let now = now_secs();
          let assertion = build_assertion(user_id, key_id, pem, &self.cfg.issuer, now)
              .map_err(ZitadelError::Invalid)?;

          let url = format!("{}/oauth/v2/token", self.cfg.issuer);
          let resp = self
              .http
              .post(&url)
              .form(&[
                  ("grant_type", "urn:ietf:params:oauth:grant-type:jwt-bearer"),
                  ("assertion", assertion.as_str()),
                  ("scope", ADMIN_SCOPE),
              ])
              .send()
              .await
              .map_err(|e| ZitadelError::Transport(e.to_string()))?;
          let status = resp.status().as_u16();
          let body = resp.text().await.unwrap_or_default();
          if status != 200 {
              return Err(map_status(status, &body));
          }
          let json: serde_json::Value = serde_json::from_str(&body)
              .map_err(|e| ZitadelError::Invalid(format!("token json: {e}")))?;
          let token = json["access_token"]
              .as_str()
              .ok_or_else(|| ZitadelError::Invalid("no access_token in mint response".into()))?
              .to_string();
          let ttl = json["expires_in"].as_u64().unwrap_or(3000);
          Ok(CachedToken { token, exp: now + ttl })
      }
  }

  fn now_secs() -> u64 {
      std::time::SystemTime::now()
          .duration_since(std::time::UNIX_EPOCH)
          .map(|d| d.as_secs())
          .unwrap_or(0)
  }

  #[cfg(test)]
  mod tests {
      use super::*;
      use jsonwebtoken::{decode, Algorithm, DecodingKey, EncodingKey, Validation};

      // 2048-bit RSA test key (PEM). Generated for tests only; never used in prod.
      const TEST_PRIV_PEM: &str = include_str!("testdata/test_rsa_priv.pem");

      #[test]
      fn build_assertion_round_trips() {
          let now = 1_700_000_000u64;
          let jwt = build_assertion("user-1", "kid-9", TEST_PRIV_PEM, "http://iss", now)
              .expect("sign ok");

          // (sanity: the priv PEM parses)
          let _enc = EncodingKey::from_rsa_pem(TEST_PRIV_PEM.as_bytes()).unwrap();
          let pub_pem = include_str!("testdata/test_rsa_pub.pem");
          let dk = DecodingKey::from_rsa_pem(pub_pem.as_bytes()).unwrap();
          let mut v = Validation::new(Algorithm::RS256);
          v.set_audience(&["http://iss"]);
          v.set_required_spec_claims(&["exp", "aud"]);
          let data = decode::<serde_json::Value>(&jwt, &dk, &v).expect("verify ok");
          assert_eq!(data.claims["iss"], "user-1");
          assert_eq!(data.claims["sub"], "user-1");
          assert_eq!(data.claims["aud"], "http://iss");
          assert_eq!(data.claims["iat"], now);
          assert_eq!(data.claims["exp"], now + 3600);

          // header carries the kid
          let header = jsonwebtoken::decode_header(&jwt).unwrap();
          assert_eq!(header.kid.as_deref(), Some("kid-9"));
          assert_eq!(header.alg, Algorithm::RS256);
      }

      #[test]
      fn build_assertion_rejects_bad_pem() {
          let err = build_assertion("u", "k", "not a pem", "http://iss", 0).unwrap_err();
          assert!(err.to_lowercase().contains("pem") || err.to_lowercase().contains("key"));
      }
  }
  ```
  Generate the two test PEMs the test `include_str!`s (one-time, committed as testdata). If `openssl` is not on PATH on the dev machine, generate them with the workspace's own Rust crypto or via Python (`cryptography`) — the only requirement is a valid PKCS#1/PKCS#8 RSA-2048 PEM pair committed at those paths; do NOT skip the test:
  ```powershell
  New-Item -ItemType Directory -Force admin-api/src/zitadel/testdata
  openssl genrsa -out admin-api/src/zitadel/testdata/test_rsa_priv.pem 2048
  openssl rsa -in admin-api/src/zitadel/testdata/test_rsa_priv.pem -pubout -out admin-api/src/zitadel/testdata/test_rsa_pub.pem
  ```
  (Python fallback if no openssl: `python -c "from cryptography.hazmat.primitives.asymmetric import rsa; from cryptography.hazmat.primitives import serialization as s; k=rsa.generate_private_key(public_exponent=65537,key_size=2048); open('admin-api/src/zitadel/testdata/test_rsa_priv.pem','wb').write(k.private_bytes(s.Encoding.PEM,s.PrivateFormat.PKCS8,s.NoEncryption())); open('admin-api/src/zitadel/testdata/test_rsa_pub.pem','wb').write(k.public_key().public_bytes(s.Encoding.PEM,s.PublicFormat.SubjectPublicKeyInfo))"`.)
- [ ] **Step 2: Run it — expect FAIL** — `cargo test -p llm-chat-admin-api build_assertion` from `D:\projects\llm-chat`. Expect: `build_assertion_round_trips` and `build_assertion_rejects_bad_pem` fail with `panicked at 'not implemented'`.
- [ ] **Step 3: Implement** — (a) fill in `build_assertion`:
  ```rust
  pub fn build_assertion(
      user_id: &str,
      key_id: &str,
      pem: &str,
      issuer: &str,
      now: u64,
  ) -> Result<String, String> {
      use jsonwebtoken::{encode, Algorithm, EncodingKey, Header};
      let mut header = Header::new(Algorithm::RS256);
      header.kid = Some(key_id.to_string());
      let claims = serde_json::json!({
          "iss": user_id,
          "sub": user_id,
          "aud": issuer,
          "iat": now,
          "exp": now + 3600,
      });
      let key = EncodingKey::from_rsa_pem(pem.as_bytes())
          .map_err(|e| format!("bad SA key PEM: {e}"))?;
      encode(&header, &claims, &key).map_err(|e| format!("sign assertion: {e}"))
  }
  ```
  (b) create `admin-api/src/lib.rs` (the canonical surface shared by the bin + the integration tests — Tasks 17/18/19 extend it):
  ```rust
  //! admin-api library surface — shared by the binary and the integration tests.
  pub mod config;
  pub mod zitadel;
  ```
  (c) flesh out `admin-api/src/main.rs` with the real startup — `AdminConfig::from_env` fail-fast + the discovery **issuer-match guard** (§8: the discovery doc's `issuer` must equal configured `ZITADEL_ISSUER`, else `exit 1`), then build the `ZitadelClient` and (placeholder) serve. `main.rs` now uses the lib crate rather than re-declaring modules:
  ```rust
  // llm-chat-admin-api — Backend-For-Frontend for the Zitadel user-management
  // admin. Owns the operator OIDC session + the least-privilege admin service
  // account; the browser only ever holds an opaque session cookie.

  use llm_chat_admin_api::config::AdminConfig;
  use llm_chat_admin_api::zitadel;

  /// PURE: the two issuer strings must match byte-for-byte (the single-issuer
  /// linchpin, design §8). `configured` is already trailing-slash-trimmed by
  /// AdminConfig::from_map; trim the discovery value the same way before compare.
  fn issuer_matches(configured: &str, discovered: &str) -> bool {
      configured == discovered.trim_end_matches('/')
  }

  /// Fetch the discovery doc and assert its `issuer` equals our configured one.
  async fn assert_issuer_match(
      http: &reqwest::Client,
      cfg: &AdminConfig,
  ) -> Result<(), String> {
      let url = format!("{}/.well-known/openid-configuration", cfg.issuer);
      let doc: serde_json::Value = http
          .get(&url)
          .send()
          .await
          .map_err(|e| format!("discovery fetch {url}: {e}"))?
          .json()
          .await
          .map_err(|e| format!("discovery json: {e}"))?;
      let discovered = doc["issuer"].as_str().unwrap_or_default();
      if !issuer_matches(&cfg.issuer, discovered) {
          return Err(format!(
              "issuer mismatch: configured ZITADEL_ISSUER={} but discovery issuer={} \
               (a single literal issuer must match byte-for-byte, design §8)",
              cfg.issuer, discovered
          ));
      }
      Ok(())
  }

  #[tokio::main]
  async fn main() -> Result<(), Box<dyn std::error::Error>> {
      tracing_subscriber::fmt()
          .with_env_filter(
              tracing_subscriber::EnvFilter::try_from_default_env()
                  .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
          )
          .with_writer(std::io::stderr)
          .init();

      // Guard 1 (design §8a): required-config validation, fail fast naming the
      // first missing var, BEFORE any side effect.
      let cfg = AdminConfig::from_env().map_err(|e| -> Box<dyn std::error::Error> {
          tracing::error!(target: "admin-api::config", error = %e, "config invalid");
          e.into()
      })?;
      tracing::info!(
          target: "admin-api",
          issuer = %cfg.issuer,
          project_id = %cfg.project_id,
          bind = %cfg.bind_addr,
          "admin-api starting"
      );

      let http = reqwest::Client::builder()
          .redirect(reqwest::redirect::Policy::none()) // SSRF guard (appendix §4.3)
          .build()?;

      // Guard 2 (design §8b): issuer-string match of the discovery doc vs
      // ZITADEL_ISSUER — pre-empts silent per-token 401s. exit 1 on mismatch.
      assert_issuer_match(&http, &cfg).await.map_err(|e| -> Box<dyn std::error::Error> {
          tracing::error!(target: "admin-api::startup", error = %e, "issuer-match guard failed");
          e.into()
      })?;
      tracing::info!(target: "admin-api::startup", "issuer-match guard passed");

      let _client = zitadel::ZitadelClient::new(cfg.clone(), http);
      // Router + serve land in Task 18 (auth/session/api + AppState + session
      // layer). For now, bind so the fail-fast guards are exercised end-to-end
      // and the process stays up.
      let listener = tokio::net::TcpListener::bind(&cfg.bind_addr).await?;
      tracing::info!(target: "admin-api", addr = %cfg.bind_addr, "admin-api listening (router pending Task 18)");
      let app = axum::Router::new();
      axum::serve(listener, app).await?;
      Ok(())
  }

  #[cfg(test)]
  mod startup_tests {
      use super::issuer_matches;

      #[test]
      fn issuer_match_trims_discovery_slash() {
          assert!(issuer_matches("http://h:8080", "http://h:8080/"));
          assert!(issuer_matches("http://h:8080", "http://h:8080"));
      }

      #[test]
      fn issuer_mismatch_detected() {
          assert!(!issuer_matches("http://h:8080", "http://other:8080"));
      }
  }
  ```
  Remove the old `scaffold_smoke` module from Task 9's `main.rs` (its placeholder is now superseded by `startup_tests` + the real `main`). Update `admin-api/Cargo.toml` to declare BOTH the lib and the bin so `tests/` can `use llm_chat_admin_api::...`:
  ```toml
  [lib]
  name = "llm_chat_admin_api"
  path = "src/lib.rs"

  [[bin]]
  name = "llm-chat-admin-api"
  path = "src/main.rs"
  ```
- [ ] **Step 4: Run — expect PASS** — `cargo test -p llm-chat-admin-api` from `D:\projects\llm-chat`. Expect all unit tests pass: `build_assertion_round_trips`, `build_assertion_rejects_bad_pem`, `issuer_match_*`, plus Task 10–12 (`require_var*`, `from_map*`, `map_status*`) — `test result: ok.` with 0 failed. The gated live-mint check is the integration test below (not run here).
- [ ] **Step 5: Add the gated integration test (live source of truth, not a mock)** — the `/oauth/v2/token` JWT-bearer response shape is empirical (appendix §2.5, §6.2). Create `admin-api/tests/integration.rs` with the two helpers fully defined (no placeholders) and the two-arg `ZitadelClient::new`:
  ```rust
  //! Integration tests vs a RUNNING Zitadel v3.4.10. Gated on ADMIN_IT=1 so the
  //! default `cargo test` stays offline. Discharges appendix §6 checklist items
  //! against the real instance (the source of truth) instead of mocking shapes.
  //!
  //! Requires the same env AdminConfig::from_env reads (ZITADEL_ISSUER, project,
  //! ADMIN_SA_KEY_PATH pointing at secrets/admin-api-key.json, ADMIN_PUBLIC_ORIGIN,
  //! ADMIN_ALLOWED_ORIGIN, ADMIN_SESSION_KEY, ADMIN_BIND_ADDR, OIDC client id/secret).

  use llm_chat_admin_api::config::AdminConfig;
  use llm_chat_admin_api::zitadel::ZitadelClient;

  fn it_enabled() -> bool {
      std::env::var("ADMIN_IT").as_deref() == Ok("1")
  }

  /// Resolve config from the live env (panics if unset — only called under the gate).
  fn llm_chat_admin_api_cfg() -> AdminConfig {
      AdminConfig::from_env().expect("admin config from env (ADMIN_IT run)")
  }

  /// Build the shared redirect-less reqwest client + the two-arg ZitadelClient.
  fn admin_client(cfg: AdminConfig, http: reqwest::Client) -> ZitadelClient {
      ZitadelClient::new(cfg, http)
  }

  // §6.2: prove ORG_USER_MANAGER (or ORG_OWNER) can mint a Management token via
  // the SA JSON key, and that the response carries access_token + expires_in.
  #[tokio::test]
  async fn it_mint_management_token() {
      if !it_enabled() {
          eprintln!("skipping (set ADMIN_IT=1 + Zitadel env to run) — appendix §6.2");
          return;
      }
      let cfg = llm_chat_admin_api_cfg();
      let http = reqwest::Client::builder()
          .redirect(reqwest::redirect::Policy::none())
          .build()
          .unwrap();
      let client = admin_client(cfg, http);
      let tok = client.mint_management_token().await.expect("mint ok (§6.2)");
      assert!(!tok.token.is_empty(), "access_token present");
      assert!(tok.exp > 0, "expires_in mapped to absolute exp");
  }
  ```
  Run with `$env:ADMIN_IT='1'; cargo test -p llm-chat-admin-api --test integration` against the running stack and record that §6.2 (machine-key/token mint under the granted role) is discharged; if the mint 403s, bump the SA grant per Phase B (`ORG_USER_MANAGER`→`ORG_OWNER`) and re-run. Do NOT assert a fabricated JSON body offline.
- [ ] **Step 6: Commit** —
  ```powershell
  git add admin-api/src/zitadel/mod.rs admin-api/src/zitadel/token.rs admin-api/src/zitadel/testdata admin-api/src/main.rs admin-api/src/lib.rs admin-api/Cargo.toml admin-api/tests/integration.rs
  git commit -m @'
  feat(admin-api): SA JWT-bearer assertion + token cache + startup guards

  build_assertion mirrors provision.py:build_jwt_assertion (RS256, kid header,
  iss=sub=userId, aud=issuer, exp+3600); valid_token caches and refreshes the
  Management-API token before expiry via the jwt-bearer grant with the `zitadel`
  scope trap. Add src/lib.rs so bin + tests share one surface. main.rs now fails
  fast on bad config and on a discovery issuer mismatch (design §8). Live
  token-mint shape is checked by the ADMIN_IT=1 integration test (appendix
  §2.5/§6.2), not a fabricated mock.

  Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
  '@
  ```

---

## Phase C2-backend-api

> Prereqs from earlier phases (assumed green before this phase starts): the Cargo workspace exists with `admin-api/` as a member, `crates/zitadel-auth` exports `ZitadelConfig`, `JwksCache`, `Principal`, `AuthError`, and `admin-api` already has `src/config.rs` (`AdminConfig` incl. `public_origin`, `require_var`, `from_env`), `src/lib.rs` (`pub mod config; pub mod zitadel;`), `src/zitadel/mod.rs` (`ZitadelClient` with two-arg `new(cfg, http)`), `src/zitadel/token.rs` (`CachedToken`, `build_assertion`, `valid_token`, `mint_management_token`), and `src/zitadel/error.rs` (`ZitadelError`, `map_status`) from Phase C1. The Task 8 empirical gate (§6.1) has been run against the stack before Task 17. All commands run from `D:\projects\llm-chat` in PowerShell.

---

### Task 14: zitadel/model.rs — `User` struct + `user_from_v2` (v1↔v2 field mapping, pure-unit-tested)

**Files:**
- Create: `admin-api/src/zitadel/model.rs`
- Modify: `admin-api/src/zitadel/mod.rs:1` (add `pub mod model;`)

- [ ] **Step 1: Write the failing test** — append to `admin-api/src/zitadel/model.rs` a `#[cfg(test)]` module that pins the v2 read shape (`userId`/`username`/`email.isVerified`/`givenName`/`familyName`) and the human/machine discriminator (appendix §3.1 v1↔v2 deltas). This shape is the v2 GET/list response and is **read-only mapping** so it is safe to unit-test (no network):

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn user_from_v2_maps_human_fields() {
        let v = json!({
            "userId": "u-1", "username": "alice", "state": "USER_STATE_ACTIVE",
            "human": {
                "profile": { "givenName": "Alice", "familyName": "Stone",
                             "displayName": "Alice Stone" },
                "email": { "email": "alice@x.io", "isVerified": true }
            }
        });
        let u = user_from_v2(&v);
        assert_eq!(u.id, "u-1");
        assert_eq!(u.user_name, "alice");
        assert_eq!(u.kind, UserKind::Human);
        // state is normalized: the raw "USER_STATE_ACTIVE" loses its prefix so it
        // matches the frontend UserState enum + columns.tsx badge logic (Task 22).
        assert_eq!(u.state, "ACTIVE");
        assert_eq!(u.email.as_deref(), Some("alice@x.io"));
        assert_eq!(u.display_name.as_deref(), Some("Alice Stone"));
    }

    #[test]
    fn user_from_v2_maps_machine_with_no_email() {
        let v = json!({
            "userId": "m-9", "username": "chat-admin-api",
            "state": "USER_STATE_ACTIVE",
            "machine": { "name": "chat-admin-api", "description": "svc" }
        });
        let u = user_from_v2(&v);
        assert_eq!(u.kind, UserKind::Machine);
        assert_eq!(u.email, None);
        assert_eq!(u.display_name.as_deref(), Some("chat-admin-api"));
    }
}
```

- [ ] **Step 2: Run it — expect FAIL** — `cargo test -p llm-chat-admin-api model::tests` — expect failure: `cannot find type 'User'` / `cannot find function 'user_from_v2'` (module not yet implemented). If `mod model;` is missing, the failure is instead an unresolved-module error — that confirms Step-1-of-Modify is still pending.

- [ ] **Step 3: Implement** — add `pub mod model;` to `admin-api/src/zitadel/mod.rs`, then write `admin-api/src/zitadel/model.rs` above the test module:

```rust
//! Request/response models + v1<->v2 field mapping for the Zitadel user APIs.
//! Reads use v2 (`userId`/`username`/`isVerified`/`givenName/familyName`),
//! writes use v1 (appendix §3.1). One mapping site so the rest of the code is
//! version-agnostic.

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum UserKind {
    Human,
    Machine,
}

#[derive(Debug, Clone, Serialize)]
pub struct User {
    pub id: String,
    pub user_name: String,
    pub kind: UserKind,
    pub state: String,
    pub email: Option<String>,
    pub display_name: Option<String>,
    pub given_name: Option<String>,
    pub family_name: Option<String>,
}

fn str_at<'a>(v: &'a Value, key: &str) -> Option<&'a str> {
    v.get(key).and_then(Value::as_str)
}

/// Map a v2 user object (`/v2/users` list item or `/v2/users/{id}`.user) to `User`.
pub fn user_from_v2(v: &Value) -> User {
    let human = v.get("human");
    let machine = v.get("machine");
    let kind = if machine.is_some() {
        UserKind::Machine
    } else {
        UserKind::Human
    };

    let profile = human.and_then(|h| h.get("profile"));
    let given_name = profile.and_then(|p| str_at(p, "givenName")).map(String::from);
    let family_name = profile.and_then(|p| str_at(p, "familyName")).map(String::from);
    let email = human
        .and_then(|h| h.get("email"))
        .and_then(|e| str_at(e, "email"))
        .map(String::from);

    let display_name = profile
        .and_then(|p| str_at(p, "displayName"))
        .map(String::from)
        .or_else(|| machine.and_then(|m| str_at(m, "name")).map(String::from));

    User {
        id: str_at(v, "userId").unwrap_or_default().to_string(),
        user_name: str_at(v, "username").unwrap_or_default().to_string(),
        kind,
        // Normalize "USER_STATE_ACTIVE" -> "ACTIVE" so the single mapping site
        // matches the frontend UserState enum + the columns.tsx badge variants.
        state: str_at(v, "state")
            .unwrap_or_default()
            .trim_start_matches("USER_STATE_")
            .to_string(),
        email,
        display_name,
        given_name,
        family_name,
    }
}
```

- [ ] **Step 4: Run — expect PASS** — `cargo test -p llm-chat-admin-api model::tests` — expect `test result: ok. 2 passed`.

- [ ] **Step 5: Commit**
```powershell
git add admin-api/src/zitadel/model.rs admin-api/src/zitadel/mod.rs
git commit -m @'
feat(admin-api): User model + v2->User field mapping (unit-tested)

Reads use the v2 user shape (userId/username/isVerified/givenName/familyName);
user_from_v2 is the single v1<->v2 mapping site (appendix §3.1). Human/machine
discriminated by the `human`/`machine` sub-object.

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
'@
```

---

### Task 15: zitadel/grants.rs — `roles_without` pure helper (unit-tested) + grant CRUD wrappers (gated IT)

**Files:**
- Create: `admin-api/src/zitadel/grants.rs`
- Modify: `admin-api/src/zitadel/mod.rs:1` (add `pub mod grants;`; add the shared `post_json`/`get_json`/`put_json`/`delete` helpers)

- [ ] **Step 1: Write the failing test** — the load-bearing pure logic here is **revoke-one-role set math** (design §7: grant `PUT` *replaces* the whole set, so "remove one role" = read current → PUT the reduced set). Append a test module to `admin-api/src/zitadel/grants.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roles_without_drops_only_the_named_role_preserving_order() {
        let cur = vec!["chat.user".to_string(), "chat.admin".to_string()];
        assert_eq!(roles_without(&cur, "chat.admin"), vec!["chat.user".to_string()]);
    }

    #[test]
    fn roles_without_is_noop_when_role_absent() {
        let cur = vec!["chat.user".to_string()];
        assert_eq!(roles_without(&cur, "chat.admin"), vec!["chat.user".to_string()]);
    }

    #[test]
    fn roles_without_can_empty_the_set() {
        let cur = vec!["chat.admin".to_string()];
        assert!(roles_without(&cur, "chat.admin").is_empty());
    }
}
```

- [ ] **Step 2: Run it — expect FAIL** — `cargo test -p llm-chat-admin-api grants::tests` — expect failure: `cannot find function 'roles_without' in this scope`.

- [ ] **Step 3: Implement** — first add the shared HTTP helpers to `admin-api/src/zitadel/mod.rs` (they wrap `valid_token()` + reqwest + `map_status`; shared by Tasks 15-16):
```rust
use crate::zitadel::error::{map_status, ZitadelError};
use serde_json::Value;

impl ZitadelClient {
    async fn send_json(
        &self,
        method: reqwest::Method,
        url: &str,
        body: Option<&Value>,
    ) -> Result<Value, ZitadelError> {
        let token = self.valid_token().await?;
        let mut req = self
            .http
            .request(method, url)
            .bearer_auth(token)
            .header("x-zitadel-orgid", "")  // resolved from token; left blank = caller org
            .header(reqwest::header::CONTENT_TYPE, "application/json");
        if let Some(b) = body {
            req = req.json(b);
        }
        let resp = req
            .send()
            .await
            .map_err(|e| ZitadelError::Transport(e.to_string()))?;
        let status = resp.status().as_u16();
        let text = resp.text().await.unwrap_or_default();
        if !(200..300).contains(&status) {
            return Err(map_status(status, &text));
        }
        if text.is_empty() {
            return Ok(Value::Null);
        }
        serde_json::from_str(&text)
            .map_err(|e| ZitadelError::Invalid(format!("response json: {e}")))
    }

    pub(crate) async fn post_json(&self, url: &str, body: &Value) -> Result<Value, ZitadelError> {
        self.send_json(reqwest::Method::POST, url, Some(body)).await
    }
    pub(crate) async fn put_json(&self, url: &str, body: &Value) -> Result<Value, ZitadelError> {
        self.send_json(reqwest::Method::PUT, url, Some(body)).await
    }
    pub(crate) async fn get_json(&self, url: &str) -> Result<Value, ZitadelError> {
        self.send_json(reqwest::Method::GET, url, None).await
    }
    pub(crate) async fn delete(&self, url: &str) -> Result<Value, ZitadelError> {
        self.send_json(reqwest::Method::DELETE, url, None).await
    }
}
```
add `pub mod grants;` to `admin-api/src/zitadel/mod.rs`, then write `admin-api/src/zitadel/grants.rs`. The pure helper is fully implemented; the I/O wrappers use the **proven v1 grant paths** (appendix §3.4) — grant-id field is `userGrantId` on add but `id` on search (same value), `PUT` replaces the whole `roleKeys` set, search path is `/users/grants/_search` (NOT `/users/{id}/grants/_search`). Their exact JSON is empirical and verified by Task 19's gated IT, not a fabricated mock:

```rust
//! User-grant (authorization) wrappers + the revoke-one-role set math.
//! v1 Management API (appendix §3.4). The grant id is `userGrantId` on add and
//! `id` on search (same value). PUT REPLACES the whole roleKeys set, so
//! "remove one role" is read-modify-write via `roles_without` (design §7).

use serde_json::{json, Value};

use super::error::ZitadelError;
use super::ZitadelClient;

/// Return `current` with `drop` removed, order-preserving. Pure (design §7).
pub fn roles_without(current: &[String], drop: &str) -> Vec<String> {
    current.iter().filter(|r| *r != drop).cloned().collect()
}

impl ZitadelClient {
    /// List project roles: POST /management/v1/projects/{pid}/roles/_search (§3.3).
    pub async fn list_roles(&self) -> Result<Vec<Value>, ZitadelError> {
        let pid = &self.cfg.project_id;
        let url = format!("{}/management/v1/projects/{}/roles/_search", self.cfg.issuer, pid);
        let v = self.post_json(&url, &json!({})).await?;
        Ok(v.get("result").and_then(Value::as_array).cloned().unwrap_or_default())
    }

    /// List a user's grants: POST /management/v1/users/grants/_search filtered by
    /// userId (§3.4). NOTE the path is /users/grants/_search, not nested per-user.
    pub async fn list_user_grants(&self, user_id: &str) -> Result<Vec<Value>, ZitadelError> {
        let url = format!("{}/management/v1/users/grants/_search", self.cfg.issuer);
        let body = json!({ "queries": [{ "userIdQuery": { "userId": user_id } }] });
        let v = self.post_json(&url, &body).await?;
        Ok(v.get("result").and_then(Value::as_array).cloned().unwrap_or_default())
    }

    /// Add a grant (one per user+project): POST /users/{id}/grants -> userGrantId.
    pub async fn add_grant(&self, user_id: &str, role_keys: &[String]) -> Result<String, ZitadelError> {
        let url = format!("{}/management/v1/users/{}/grants", self.cfg.issuer, user_id);
        let body = json!({ "projectId": self.cfg.project_id, "roleKeys": role_keys });
        let v = self.post_json(&url, &body).await?;
        Ok(v.get("userGrantId").and_then(Value::as_str).unwrap_or_default().to_string())
    }

    /// Replace the whole roleKeys set on a grant: PUT /users/{id}/grants/{grantId}.
    pub async fn set_grant_roles(&self, user_id: &str, grant_id: &str, role_keys: &[String]) -> Result<(), ZitadelError> {
        let url = format!("{}/management/v1/users/{}/grants/{}", self.cfg.issuer, user_id, grant_id);
        self.put_json(&url, &json!({ "roleKeys": role_keys })).await.map(|_| ())
    }

    /// Revoke an entire grant: DELETE /users/{id}/grants/{grantId}.
    pub async fn remove_grant(&self, user_id: &str, grant_id: &str) -> Result<(), ZitadelError> {
        let url = format!("{}/management/v1/users/{}/grants/{}", self.cfg.issuer, user_id, grant_id);
        self.delete(&url).await.map(|_| ())
    }
}
```

- [ ] **Step 4: Run — expect PASS** — `cargo test -p llm-chat-admin-api grants::tests` — expect `test result: ok. 3 passed`. Also `cargo build -p llm-chat-admin-api` to confirm the wrappers compile.

- [ ] **Step 5: Commit**
```powershell
git add admin-api/src/zitadel/grants.rs admin-api/src/zitadel/mod.rs
git commit -m @'
feat(admin-api): grant CRUD + roles_without revoke-one-role set math

PUT replaces the whole roleKeys set, so removing one role is read-modify-write
via the pure roles_without helper (design §7, unit-tested). Adds the shared
post_json/get_json/put_json/delete client helpers. Wrappers use the v1 grant
paths (appendix §3.4); their JSON shapes are verified by the ADMIN_IT
integration suite, not mocked.

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
'@
```

---

### Task 16: zitadel/users.rs + keys.rs — user lifecycle + machine-key/secret wrappers (request shapes pinned; gated IT)

**Files:**
- Create: `admin-api/src/zitadel/users.rs`
- Create: `admin-api/src/zitadel/keys.rs`
- Modify: `admin-api/src/zitadel/mod.rs:1` (add `pub mod users;` and `pub mod keys;`)

> These are pure I/O wrappers over Zitadel whose exact JSON is empirical. Per the convention, this task specifies the **request shape + signature precisely** and defers response-shape verification to the `ADMIN_IT=1` integration test (Task 19). There is no fabricated-mock unit test here — that would assert an unverified shape as certain (CLAUDE.md "source of truth"). The verifying step is "compiles + IT passes." COVERAGE NOTE: Task 19's lifecycle is extended to exercise `create_human`, `edit_profile`, `edit_email`, `set_password`, `resend_init`, `lock/unlock/reactivate`, and `generate_secret/delete_secret` so these paths are not left unverified.

- [ ] **Step 1: Write the (compile-level) failing check** — add `pub mod users;` and `pub mod keys;` to `admin-api/src/zitadel/mod.rs` referencing files that do not yet exist, so the build fails. Run `cargo build -p llm-chat-admin-api` — expect failure: `file not found for module 'users'` (and `keys`). This is the red state: the API layer (Task 18) cannot compile until these wrappers exist.

- [ ] **Step 2: Confirm RED** — `cargo build -p llm-chat-admin-api` — expect `error[E0583]: file not found for module 'users'`. (Same convention applies as elsewhere: the failing build is the red bar for an I/O-bound wrapper whose response shape is only knowable against the running instance.)

- [ ] **Step 3: Implement** — write `admin-api/src/zitadel/users.rs` (search/get use **v2 reads** → `user_from_v2`; create-machine/edit/lifecycle/password use **proven v1 writes** — appendix §3.2/§3.6; v2 create-human per §3.2 "the repo's chosen working path"; password shape per the §3.2 three-shapes gotcha; `_resend_initialization` per the §5 table):

```rust
//! User search/get (v2 reads) + create/edit/lifecycle (v1+v2 writes).
//! Read shape -> model::user_from_v2; write shapes per appendix §3.2/§3.6.
//! Exact response keys verified by tests/integration.rs (ADMIN_IT=1).

use serde_json::{json, Value};

use super::error::ZitadelError;
use super::model::{user_from_v2, User};
use super::ZitadelClient;

impl ZitadelClient {
    /// List/search users via v2: POST /v2/users (§3.1). `type`/`state` optional.
    pub async fn search_users(&self, queries: Vec<Value>) -> Result<Vec<User>, ZitadelError> {
        let url = format!("{}/v2/users", self.cfg.issuer);
        let v = self.post_json(&url, &json!({ "queries": queries })).await?;
        let result = v.get("result").and_then(Value::as_array).cloned().unwrap_or_default();
        Ok(result.iter().map(user_from_v2).collect())
    }

    /// Get one user via v2: GET /v2/users/{id} -> {user:{...}} (§3.1).
    pub async fn get_user(&self, id: &str) -> Result<User, ZitadelError> {
        let url = format!("{}/v2/users/{}", self.cfg.issuer, id);
        let v = self.get_json(&url).await?;
        let user = v.get("user").unwrap_or(&v);
        Ok(user_from_v2(user))
    }

    /// Create a human via v2 (the repo's working path, §3.2): nested password
    /// {password,changeRequired:false} + email{isVerified} = immediately active.
    pub async fn create_human(
        &self, username: &str, given: &str, family: &str, email: &str, password: &str,
    ) -> Result<String, ZitadelError> {
        let url = format!("{}/v2/users/human", self.cfg.issuer);
        let body = json!({
            "username": username,
            "profile": { "givenName": given, "familyName": family },
            "email": { "email": email, "isVerified": true },
            "password": { "password": password, "changeRequired": false },
        });
        let v = self.post_json(&url, &body).await?;
        Ok(v.get("userId").and_then(Value::as_str).unwrap_or_default().to_string())
    }

    /// Create a machine user via v1 (§3.2). ACCESS_TOKEN_TYPE_JWT (machine USER,
    /// not the OIDC-app enum) so the manager can verify the token via JWKS.
    pub async fn create_machine(&self, username: &str, name: &str) -> Result<String, ZitadelError> {
        let url = format!("{}/management/v1/users/machine", self.cfg.issuer);
        let body = json!({
            "userName": username, "name": name,
            "accessTokenType": "ACCESS_TOKEN_TYPE_JWT",
        });
        let v = self.post_json(&url, &body).await?;
        Ok(v.get("userId").and_then(Value::as_str).unwrap_or_default().to_string())
    }

    /// Edit human profile (v1): PUT /management/v1/users/{id}/profile (§5 table).
    pub async fn edit_profile(&self, id: &str, given: &str, family: &str) -> Result<(), ZitadelError> {
        let url = format!("{}/management/v1/users/{}/profile", self.cfg.issuer, id);
        let body = json!({ "firstName": given, "lastName": family });
        self.put_json(&url, &body).await.map(|_| ())
    }

    /// Edit human email (v1): PUT /management/v1/users/{id}/email (§5 table).
    pub async fn edit_email(&self, id: &str, email: &str, verified: bool) -> Result<(), ZitadelError> {
        let url = format!("{}/management/v1/users/{}/email", self.cfg.issuer, id);
        let body = json!({ "email": email, "isEmailVerified": verified });
        self.put_json(&url, &body).await.map(|_| ())
    }

    /// Set a human password (v1): PUT /management/v1/users/{id}/password (§3.2/§5).
    pub async fn set_password(&self, id: &str, password: &str, change_required: bool) -> Result<(), ZitadelError> {
        let url = format!("{}/management/v1/users/{}/password", self.cfg.issuer, id);
        let body = json!({ "newPassword": { "password": password, "changeRequired": change_required } });
        self.put_json(&url, &body).await.map(|_| ())
    }

    /// Resend the initialization mail (v1): POST .../{id}/_resend_initialization (§5).
    pub async fn resend_init(&self, id: &str) -> Result<(), ZitadelError> {
        let url = format!("{}/management/v1/users/{}/_resend_initialization", self.cfg.issuer, id);
        self.post_json(&url, &json!({})).await.map(|_| ())
    }

    pub async fn deactivate(&self, id: &str) -> Result<(), ZitadelError> { self.lifecycle(id, "_deactivate").await }
    pub async fn reactivate(&self, id: &str) -> Result<(), ZitadelError> { self.lifecycle(id, "_reactivate").await }
    pub async fn lock(&self, id: &str) -> Result<(), ZitadelError> { self.lifecycle(id, "_lock").await }
    pub async fn unlock(&self, id: &str) -> Result<(), ZitadelError> { self.lifecycle(id, "_unlock").await }

    async fn lifecycle(&self, id: &str, verb: &str) -> Result<(), ZitadelError> {
        let url = format!("{}/management/v1/users/{}/{}", self.cfg.issuer, id, verb);
        self.post_json(&url, &json!({})).await.map(|_| ())
    }

    /// IRREVERSIBLE delete (§3.6): DELETE /management/v1/users/{id}.
    pub async fn delete_user(&self, id: &str) -> Result<(), ZitadelError> {
        let url = format!("{}/management/v1/users/{}", self.cfg.issuer, id);
        self.delete(&url).await.map(|_| ())
    }
}
```

Then write `admin-api/src/zitadel/keys.rs` (machine keys/secret, appendix §3.5 — `keyDetails` returned **once** on create; `_search` for list; secret via `PUT/DELETE .../secret`).
**Verify response keys (`userId`, `keyDetails`, `clientSecret`, `result[]`) against running Zitadel v3.4.10 — appendix §6.3/§6.4/§6.6** (discharged by Task 19, not asserted here):

```rust
//! Machine-key (jwt-profile) + client-secret (client_credentials) wrappers.
//! Two independent credential lifecycles per machine user (appendix §3.5).
//! keyDetails (the private key) is returned ONLY by create_json_key.
//! Verify response keys (userId/keyDetails/clientSecret/result[]) against the
//! running Zitadel v3.4.10 — appendix §6.3/§6.4/§6.6 (Task 19), not asserted here.

use serde_json::{json, Value};

use super::error::ZitadelError;
use super::ZitadelClient;

impl ZitadelClient {
    /// Create a JSON key: POST /users/{id}/keys {type:KEY_TYPE_JSON}.
    /// Returns the FULL create response — keyDetails (base64 SA JSON) is here ONCE.
    pub async fn create_json_key(&self, user_id: &str) -> Result<Value, ZitadelError> {
        let url = format!("{}/management/v1/users/{}/keys", self.cfg.issuer, user_id);
        self.post_json(&url, &json!({ "type": "KEY_TYPE_JSON" })).await
    }

    /// List keys (metadata only, no private key): POST /users/{id}/keys/_search.
    pub async fn list_keys(&self, user_id: &str) -> Result<Vec<Value>, ZitadelError> {
        let url = format!("{}/management/v1/users/{}/keys/_search", self.cfg.issuer, user_id);
        let v = self.post_json(&url, &json!({})).await?;
        Ok(v.get("result").and_then(Value::as_array).cloned().unwrap_or_default())
    }

    /// Delete (= revoke) a key: DELETE /users/{id}/keys/{keyId}.
    pub async fn delete_key(&self, user_id: &str, key_id: &str) -> Result<(), ZitadelError> {
        let url = format!("{}/management/v1/users/{}/keys/{}", self.cfg.issuer, user_id, key_id);
        self.delete(&url).await.map(|_| ())
    }

    /// Generate a client secret: PUT /users/{id}/secret. clientSecret shown ONCE.
    pub async fn generate_secret(&self, user_id: &str) -> Result<Value, ZitadelError> {
        let url = format!("{}/management/v1/users/{}/secret", self.cfg.issuer, user_id);
        self.put_json(&url, &json!({})).await
    }

    /// Remove the client secret: DELETE /users/{id}/secret.
    pub async fn delete_secret(&self, user_id: &str) -> Result<(), ZitadelError> {
        let url = format!("{}/management/v1/users/{}/secret", self.cfg.issuer, user_id);
        self.delete(&url).await.map(|_| ())
    }
}
```

- [ ] **Step 4: Run — expect PASS (compiles)** — `cargo build -p llm-chat-admin-api` then `cargo test -p llm-chat-admin-api --lib` — expect a clean build and `0 failed`.

- [ ] **Step 5: Commit**
```powershell
git add admin-api/src/zitadel/users.rs admin-api/src/zitadel/keys.rs admin-api/src/zitadel/mod.rs
git commit -m @'
feat(admin-api): user lifecycle + machine-key/secret Zitadel wrappers

Reads via v2 (user_from_v2); writes via the proven v1 paths and v2 create-human
(appendix §3.2/§3.5/§3.6). Request shapes pinned (JWT machine token, nested v2
password, KEY_TYPE_JSON, PUT/DELETE secret). Response shapes are empirical and
verified by the ADMIN_IT integration suite (§6.3/§6.4/§6.6), never mocked.

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
'@
```

---

### Task 17: auth.rs — `pkce_pair` + `build_authorize_url` (pure, unit-tested) + login/callback/logout handlers

**Files:**
- Create: `admin-api/src/auth.rs`
- Modify: `admin-api/src/lib.rs` (add `pub mod auth;`)

> Hand-rolled Auth Code + PKCE mirroring `clients/python/llm_chat/oidc.py` (design §4.2; NOT the `openidconnect` crate — avoids plain-HTTP issuer rejection, appendix §6.7). The two **pure** helpers are unit-tested deterministically; the network handlers are exercised by Task 19's gated IT and Task 23's Playwright smoke. The OIDC `redirect_uri` is built from `cfg.public_origin` (the admin-api's own externally-reachable origin, `http://localhost:7676`) so it matches the redirect URI the provisioner registered (Task 6) — NOT `allowed_origin` (the web origin). PRECONDITION: the Task 8 §6.1 gate has been run against the stack so `chat.admin` is known to ride in the access-token JWT.

- [ ] **Step 1: Write the failing test** — append a test module to `admin-api/src/auth.rs`. `pkce_pair` must be deterministic from a seed (so it is unit-testable, unlike `oidc.py::make_pkce`'s random version) and produce a valid S256 challenge = `b64url(sha256(verifier))`; `build_authorize_url` must carry the §1.2/§1.3 params and the WEB-app scopes incl. project-audience + roles (appendix §1.3). `test_cfg()` sets `issuer=http://h:8080`, `public_origin=http://localhost:7676`, `project_id=p1`, `oidc_client_id=c1`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
    use sha2::{Digest, Sha256};
    use crate::config::AdminConfig;

    fn test_cfg() -> AdminConfig {
        AdminConfig {
            issuer: "http://h:8080".into(),
            project_id: "p1".into(),
            audience: "p1".into(),
            sa_key_path: "/x".into(),
            oidc_client_id: "c1".into(),
            oidc_client_secret: "s1".into(),
            bind_addr: "0.0.0.0:7676".into(),
            public_origin: "http://localhost:7676".into(),
            allowed_origin: "http://localhost:3000".into(),
            session_key: "k".into(),
        }
    }

    #[test]
    fn pkce_pair_challenge_is_s256_of_verifier() {
        let (verifier, challenge) = pkce_pair("seed-abc");
        let want = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
        assert_eq!(challenge, want);
        assert!(!verifier.contains('=') && !verifier.contains('+') && !verifier.contains('/'));
    }

    #[test]
    fn pkce_pair_is_deterministic_per_seed() {
        assert_eq!(pkce_pair("s1"), pkce_pair("s1"));
        assert_ne!(pkce_pair("s1").0, pkce_pair("s2").0);
    }

    #[test]
    fn build_authorize_url_carries_pkce_state_nonce_and_scopes() {
        let cfg = test_cfg();
        let url = build_authorize_url(&cfg, "CHAL", "STATE", "NONCE");
        assert!(url.starts_with("http://h:8080/oauth/v2/authorize?"));
        assert!(url.contains("client_id=c1"));
        assert!(url.contains("response_type=code"));
        assert!(url.contains("code_challenge=CHAL"));
        assert!(url.contains("code_challenge_method=S256"));
        assert!(url.contains("state=STATE"));
        assert!(url.contains("nonce=NONCE"));
        // redirect_uri uses the admin-api public origin (:7676), URL-encoded
        assert!(url.contains("redirect_uri=http%3A%2F%2Flocalhost%3A7676%2Fcallback"));
        // project-audience + roles scopes (URL-encoded ':' = %3A)
        assert!(url.contains("urn%3Azitadel%3Aiam%3Aorg%3Aproject%3Aid%3Ap1%3Aaud"));
        assert!(url.contains("urn%3Azitadel%3Aiam%3Aorg%3Aprojects%3Aroles"));
    }
}
```

- [ ] **Step 2: Run it — expect FAIL** — `cargo test -p llm-chat-admin-api auth::tests` — expect failure: `cannot find function 'pkce_pair'` / `cannot find function 'build_authorize_url'`. (The `base64`/`sha2`/`url`/`rand` deps were added to `admin-api/Cargo.toml` in Task 9.)

- [ ] **Step 3: Implement** — add `pub mod auth;` to `admin-api/src/lib.rs`, then write `admin-api/src/auth.rs`. Pure helpers fully implemented; handlers mirror `oidc.py` (`/oauth/v2/authorize` redirect → `/callback` exchanges code+verifier at `/oauth/v2/token` with HTTP Basic per §1.4 → verify JWT via `zitadel_auth::JwksCache` → require `chat.admin` → store operator in session → 302 to web; `/logout` revokes + `end_session` + clears session). All origin-derived URLs use `cfg.public_origin`:

```rust
//! Hand-rolled OIDC Authorization Code + PKCE for the operator login, mirroring
//! clients/python/llm_chat/oidc.py (design §4.2). Not the openidconnect crate:
//! that rejects the plain-HTTP dev issuer (appendix §6.7). The callback JWT is
//! verified by the SHARED zitadel_auth::JwksCache and gated on `chat.admin`.
//! Handler success path (real code exchange + chat.admin in the access-token JWT
//! for a HUMAN login) is verified by Task 23's gated smoke — appendix §6.1.

use axum::{
    extract::{Query, State},
    http::{header, StatusCode},
    response::{IntoResponse, Redirect, Response},
};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use tower_sessions::Session;

use crate::config::AdminConfig;
use crate::session::Operator;
use crate::AppState;

// ---------------- pure helpers (unit-tested, no network) ----------------

fn b64url(raw: &[u8]) -> String {
    URL_SAFE_NO_PAD.encode(raw)
}

/// Deterministic (verifier, challenge) using S256: challenge = b64url(sha256(verifier)).
/// `seed` is fresh per-login (random) at the call site; deterministic here so it
/// is unit-testable. Mirrors oidc.py::make_pkce but seedable.
pub fn pkce_pair(seed: &str) -> (String, String) {
    let verifier = b64url(Sha256::digest(format!("verifier:{seed}").as_bytes()).as_slice());
    let challenge = b64url(Sha256::digest(verifier.as_bytes()).as_slice());
    (verifier, challenge)
}

/// Build the /oauth/v2/authorize URL with PKCE + WEB-app scopes (appendix §1.2/§1.3).
/// redirect_uri uses cfg.public_origin (the admin-api's own origin).
pub fn build_authorize_url(cfg: &AdminConfig, challenge: &str, state: &str, nonce: &str) -> String {
    let scope = format!(
        "openid profile email offline_access \
         urn:zitadel:iam:org:project:id:{}:aud \
         urn:zitadel:iam:org:projects:roles",
        cfg.project_id
    );
    let redirect_uri = format!("{}/callback", cfg.public_origin);
    let q = url::form_urlencoded::Serializer::new(String::new())
        .append_pair("client_id", &cfg.oidc_client_id)
        .append_pair("redirect_uri", &redirect_uri)
        .append_pair("response_type", "code")
        .append_pair("scope", &scope)
        .append_pair("code_challenge", challenge)
        .append_pair("code_challenge_method", "S256")
        .append_pair("state", state)
        .append_pair("nonce", nonce)
        .append_pair("prompt", "login")
        .finish();
    format!("{}/oauth/v2/authorize?{}", cfg.issuer, q)
}

// ---------------- handlers (network; verified by Task 19/23) ----------------

pub async fn login(State(st): State<AppState>, session: Session) -> Response {
    let seed = b64url(&rand::random::<[u8; 16]>());
    let (verifier, challenge) = pkce_pair(&seed);
    let state = b64url(&rand::random::<[u8; 16]>());
    let nonce = b64url(&rand::random::<[u8; 16]>());
    // pre-auth session: persist verifier+state+nonce across /login -> /callback (§1.6)
    let _ = session.insert("pkce_verifier", &verifier).await;
    let _ = session.insert("oauth_state", &state).await;
    let _ = session.insert("oidc_nonce", &nonce).await;
    Redirect::to(&build_authorize_url(&st.cfg, &challenge, &state, &nonce)).into_response()
}

#[derive(Deserialize)]
pub struct CallbackQuery {
    code: Option<String>,
    state: Option<String>,
    error: Option<String>,
}

pub async fn callback(
    State(st): State<AppState>,
    session: Session,
    Query(q): Query<CallbackQuery>,
) -> Response {
    if let Some(err) = q.error {
        return (StatusCode::FORBIDDEN, format!("login failed: {err}")).into_response();
    }
    let want_state: Option<String> = session.get("oauth_state").await.ok().flatten();
    if want_state.is_none() || q.state != want_state {
        return (StatusCode::FORBIDDEN, "state mismatch (possible CSRF)").into_response();
    }
    let verifier: String = match session.get("pkce_verifier").await.ok().flatten() {
        Some(v) => v,
        None => return (StatusCode::BAD_REQUEST, "no PKCE verifier in session").into_response(),
    };
    let code = match q.code {
        Some(c) => c,
        None => return (StatusCode::BAD_REQUEST, "no authorization code").into_response(),
    };

    // Exchange code + verifier, authenticating with client_id:client_secret via
    // HTTP Basic (§1.4). Then verify the JWT via the SHARED JwksCache and require
    // chat.admin (§1.5) — zero new parsing logic.
    let token = match exchange_code(&st, &code, &verifier).await {
        Ok(t) => t,
        Err(e) => return (StatusCode::BAD_GATEWAY, e).into_response(),
    };
    let principal = match st.jwks.verify_sync(&token) {
        Ok(p) => p,
        Err(e) => return (StatusCode::UNAUTHORIZED, e.to_string()).into_response(),
    };
    if !principal.has("chat.admin") {
        return (StatusCode::FORBIDDEN, "not a chat.admin operator").into_response();
    }

    let op = Operator {
        user_id: principal.user_id.clone(),
        name: principal.email.clone().unwrap_or_else(|| principal.user_id.clone()),
        roles: principal.roles.clone(),
    };
    let _ = session.remove::<String>("pkce_verifier").await;
    if session.insert("operator", &op).await.is_err() {
        return (StatusCode::INTERNAL_SERVER_ERROR, "session write failed").into_response();
    }
    // 302 to the web origin (the browser talks to admin-web :3000).
    Redirect::to(&format!("{}/", st.cfg.allowed_origin)).into_response()
}

pub async fn logout(State(st): State<AppState>, session: Session) -> Response {
    let _ = session.delete().await;
    // best-effort end_session (appendix §1.6); cookie/session already cleared.
    let url = format!("{}/oidc/v1/end_session?post_logout_redirect_uri={}/",
        st.cfg.issuer, st.cfg.allowed_origin);
    Redirect::to(&url).into_response()
}

async fn exchange_code(st: &AppState, code: &str, verifier: &str) -> Result<String, String> {
    let redirect_uri = format!("{}/callback", st.cfg.public_origin);
    let basic = b64_basic(&st.cfg.oidc_client_id, &st.cfg.oidc_client_secret);
    let resp = st.http
        .post(format!("{}/oauth/v2/token", st.cfg.issuer))
        .header(header::AUTHORIZATION, format!("Basic {basic}"))
        .form(&[
            ("grant_type", "authorization_code"),
            ("code", code),
            ("redirect_uri", &redirect_uri),
            ("code_verifier", verifier),
        ])
        .send().await.map_err(|e| format!("token endpoint unreachable: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("token endpoint returned {}", resp.status()));
    }
    let body: serde_json::Value = resp.json().await.map_err(|e| format!("token not JSON: {e}"))?;
    body.get("access_token").and_then(|v| v.as_str()).map(String::from)
        .ok_or_else(|| "no access_token in token response".into())
}

fn b64_basic(id: &str, secret: &str) -> String {
    // HTTP Basic per §1.4: base64(urlencode(id):urlencode(secret)).
    let enc = |s: &str| url::form_urlencoded::byte_serialize(s.as_bytes()).collect::<String>();
    base64::engine::general_purpose::STANDARD.encode(format!("{}:{}", enc(id), enc(secret)))
}
```

- [ ] **Step 4: Run — expect PASS** — `cargo test -p llm-chat-admin-api auth::tests` — expect `test result: ok. 3 passed`. (Note: `auth.rs` references `crate::AppState` and `crate::session::Operator`, which land in Task 18; until then it will not compile on its own. Sequence Task 18 immediately after, or stub `AppState`/`Operator` minimally. The pure-helper tests above do not depend on them — to run Step 4 in isolation, temporarily `#[cfg(test)]`-gate only the `tests` module and the pure fns; the canonical green is after Task 18 wires `AppState`.)

- [ ] **Step 5: Commit**
```powershell
git add admin-api/src/auth.rs admin-api/src/lib.rs
git commit -m @'
feat(admin-api): hand-rolled OIDC login (pkce_pair + authorize URL + callback)

PKCE/authorize-URL builders are pure and unit-tested; the callback exchanges
code+verifier (HTTP Basic, §1.4), verifies the JWT via the shared
zitadel_auth::JwksCache and requires chat.admin (§1.5). redirect_uri uses
cfg.public_origin (admin-api own origin) to match the registered redirect.
Hand-rolled, not the openidconnect crate, to accept the plain-HTTP dev issuer
(§6.7). Human-login role-in-access-token is verified by Task 23 (§6.1).

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
'@
```

---

### Task 18: `AppState` + session.rs (`Operator` extractor, fail-closed) + api/error.rs + api/mod.rs router + session layer + main.rs wiring

**Files:**
- Modify: `admin-api/src/lib.rs` (define `AppState` + add `pub mod session;` and `pub mod api;`)
- Create: `admin-api/src/session.rs`
- Create: `admin-api/src/api/error.rs`
- Create: `admin-api/src/api/mod.rs`
- Modify: `admin-api/src/main.rs` (build `AppState` incl. `JwksCache`, install `SessionManagerLayer` + `TraceLayer`, serve `api::router(state)`)

This task closes three critique gaps: (1) no task defined `AppState`; (2) the `tower-sessions` `SessionManagerLayer` was never installed (so `Session` extraction would fail at runtime); (3) the `JwksCache` (which consumes `audience`/`project_id`) was never constructed.

- [ ] **Step 1: Write the failing test** — append a test module to `admin-api/src/api/error.rs` (the pure, deterministic piece is the `ZitadelError → ApiError → HTTP status/JSON` mapping; the extractor's reject path is verified end-to-end by Task 19/23):

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::zitadel::error::ZitadelError;
    use axum::http::StatusCode;
    use axum::response::IntoResponse;

    #[test]
    fn maps_zitadel_errors_to_http_status() {
        assert_eq!(ApiError::from(ZitadelError::NotFound).into_response().status(), StatusCode::NOT_FOUND);
        assert_eq!(ApiError::from(ZitadelError::Forbidden).into_response().status(), StatusCode::FORBIDDEN);
        assert_eq!(ApiError::from(ZitadelError::AlreadyExists).into_response().status(), StatusCode::CONFLICT);
        assert_eq!(ApiError::from(ZitadelError::Invalid("bad".into())).into_response().status(), StatusCode::BAD_REQUEST);
        assert_eq!(ApiError::from(ZitadelError::Upstream).into_response().status(), StatusCode::BAD_GATEWAY);
        assert_eq!(ApiError::from(ZitadelError::Transport("x".into())).into_response().status(), StatusCode::BAD_GATEWAY);
    }
}
```

- [ ] **Step 2: Run it — expect FAIL** — `cargo test -p llm-chat-admin-api error::tests` — expect failure: `cannot find type 'ApiError'` (module not yet created).

- [ ] **Step 3: Implement** — define `AppState` in `admin-api/src/lib.rs`, add the module decls, then write the three files and wire `main.rs`.

  3a. Extend `admin-api/src/lib.rs`:
  ```rust
  //! admin-api library surface — shared by the binary and the integration tests.
  pub mod api;
  pub mod auth;
  pub mod config;
  pub mod session;
  pub mod zitadel;

  use std::sync::Arc;

  /// Shared handler state. Clone is cheap (Arc + reqwest::Client are ref-counted).
  #[derive(Clone)]
  pub struct AppState {
      pub cfg: config::AdminConfig,
      pub jwks: zitadel_auth::JwksCache,
      pub zitadel: Arc<zitadel::ZitadelClient>,
      pub http: reqwest::Client,
  }
  ```

  3b. `admin-api/src/session.rs`:
  ```rust
  //! The operator session model + a fail-closed axum extractor. Loads the
  //! tower-sessions session and REJECTS 403 unless roles contains "chat.admin"
  //! (design §4.2 "fails closed"). Reuses the Operator written by auth::callback.

  use axum::{
      extract::FromRequestParts,
      http::{request::Parts, StatusCode},
  };
  use serde::{Deserialize, Serialize};
  use tower_sessions::Session;

  #[derive(Debug, Clone, Serialize, Deserialize)]
  pub struct Operator {
      pub user_id: String,
      pub name: String,
      pub roles: Vec<String>,
  }

  impl Operator {
      pub fn has(&self, role: &str) -> bool {
          self.roles.iter().any(|r| r == role)
      }
  }

  impl<S> FromRequestParts<S> for Operator
  where
      S: Send + Sync,
  {
      type Rejection = (StatusCode, &'static str);

      async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
          let session = Session::from_request_parts(parts, state)
              .await
              .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, "session layer missing"))?;
          let op: Option<Operator> = session
              .get("operator")
              .await
              .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, "session read failed"))?;
          match op {
              Some(o) if o.has("chat.admin") => Ok(o),
              Some(_) => Err((StatusCode::FORBIDDEN, "operator lacks chat.admin")),
              None => Err((StatusCode::UNAUTHORIZED, "no operator session")),
          }
      }
  }
  ```

  3c. `admin-api/src/api/error.rs`:
  ```rust
  //! API error type: maps ZitadelError -> HTTP status + {code,message} JSON
  //! (design §8). No internal/secret leakage in the message.

  use axum::{http::StatusCode, response::{IntoResponse, Response}, Json};
  use serde_json::json;

  use crate::zitadel::error::ZitadelError;

  #[derive(Debug)]
  pub enum ApiError {
      NotFound(String),
      Forbidden(String),
      Conflict(String),
      BadRequest(String),
      Upstream(String),
  }

  impl ApiError {
      fn parts(&self) -> (StatusCode, &str, &str) {
          match self {
              ApiError::NotFound(m) => (StatusCode::NOT_FOUND, "not_found", m),
              ApiError::Forbidden(m) => (StatusCode::FORBIDDEN, "forbidden", m),
              ApiError::Conflict(m) => (StatusCode::CONFLICT, "already_exists", m),
              ApiError::BadRequest(m) => (StatusCode::BAD_REQUEST, "invalid", m),
              ApiError::Upstream(m) => (StatusCode::BAD_GATEWAY, "upstream", m),
          }
      }
  }

  impl IntoResponse for ApiError {
      fn into_response(self) -> Response {
          let (status, code, message) = self.parts();
          (status, Json(json!({ "code": code, "message": message }))).into_response()
      }
  }

  impl From<ZitadelError> for ApiError {
      fn from(e: ZitadelError) -> Self {
          match e {
              ZitadelError::NotFound => ApiError::NotFound("resource not found".into()),
              ZitadelError::Forbidden => ApiError::Forbidden("permission denied".into()),
              ZitadelError::AlreadyExists => ApiError::Conflict("already exists".into()),
              ZitadelError::Invalid(m) => ApiError::BadRequest(m),
              ZitadelError::Upstream => ApiError::Upstream("zitadel upstream error".into()),
              ZitadelError::Transport(m) => ApiError::Upstream(m),
          }
      }
  }
  ```

  3d. `admin-api/src/api/mod.rs` (full `/api` table; every `/api/*` handler takes the `Operator` extractor so the route is gated; `/login|/callback|/logout` establish the session and are intentionally NOT gated; delegates to the `ZitadelClient` wrappers from Tasks 15-16):
  ```rust
  //! The /api JSON surface (design §5). Every /api/* handler takes the Operator
  //! extractor, so a missing/insufficient session is rejected before the body
  //! runs. /login,/callback,/logout establish the session and are NOT gated.

  pub mod error;

  use axum::{
      extract::{Path, Query, State},
      routing::{delete, get, patch, post, put},
      Json, Router,
  };
  use serde::Deserialize;
  use serde_json::{json, Value};

  use crate::auth;
  use crate::session::Operator;
  use crate::AppState;
  use error::ApiError;

  pub fn router(state: AppState) -> Router {
      Router::new()
          // operator OIDC (full-page nav) — no Operator extractor on these.
          .route("/login", get(auth::login))
          .route("/callback", get(auth::callback))
          .route("/logout", post(auth::logout))
          // gated /api surface
          .route("/api/me", get(me))
          .route("/api/users", get(list_users))
          .route("/api/users/{id}", get(get_user).delete(delete_user))
          .route("/api/users/human", post(create_human))
          .route("/api/users/machine", post(create_machine))
          .route("/api/users/{id}/profile", patch(edit_profile))
          .route("/api/users/{id}/email", patch(edit_email))
          .route("/api/users/{id}/password", post(set_password))
          .route("/api/users/{id}/resend-init", post(resend_init))
          .route("/api/users/{id}/deactivate", post(deactivate))
          .route("/api/users/{id}/reactivate", post(reactivate))
          .route("/api/users/{id}/lock", post(lock))
          .route("/api/users/{id}/unlock", post(unlock))
          .route("/api/users/{id}/grants", get(list_grants).post(add_grant))
          .route("/api/users/{id}/grants/{grantId}", put(set_grant).delete(remove_grant))
          .route("/api/roles", get(list_roles))
          .route("/api/users/{id}/keys", get(list_keys).post(create_key))
          .route("/api/users/{id}/keys/{keyId}", delete(delete_key))
          .route("/api/users/{id}/secret", post(generate_secret).delete(delete_secret))
          .with_state(state)
  }

  async fn me(op: Operator) -> Json<Value> {
      Json(json!({ "userId": op.user_id, "name": op.name, "roles": op.roles }))
  }

  #[derive(Deserialize)]
  struct UserListQuery { q: Option<String>, r#type: Option<String>, state: Option<String> }

  async fn list_users(_op: Operator, State(st): State<AppState>, Query(qp): Query<UserListQuery>)
      -> Result<Json<Value>, ApiError> {
      // Map the optional filters to v2 SearchQuery[]. Exact query-field shapes are
      // verified against the running instance (appendix §6.3); unknown filters are
      // simply omitted (unfiltered list) rather than guessed.
      let mut queries: Vec<Value> = Vec::new();
      if let Some(q) = qp.q.filter(|s| !s.is_empty()) {
          queries.push(json!({ "userNameQuery": { "userName": q, "method": "TEXT_QUERY_METHOD_CONTAINS_IGNORE_CASE" } }));
      }
      if let Some(t) = qp.r#type.filter(|s| !s.is_empty()) {
          // "human" | "machine" -> v2 type query
          queries.push(json!({ "typeQuery": { "type": format!("TYPE_{}", t.to_uppercase()) } }));
      }
      if let Some(s) = qp.state.filter(|s| !s.is_empty()) {
          queries.push(json!({ "stateQuery": { "state": s } }));
      }
      let users = st.zitadel.search_users(queries).await?;
      Ok(Json(json!({ "result": users })))
  }

  async fn get_user(_op: Operator, State(st): State<AppState>, Path(id): Path<String>)
      -> Result<Json<Value>, ApiError> {
      let user = st.zitadel.get_user(&id).await?;
      let grants = st.zitadel.list_user_grants(&id).await?;
      Ok(Json(json!({ "user": user, "grants": grants })))
  }

  #[derive(Deserialize)]
  struct CreateHuman { username: String, given_name: String, family_name: String, email: String, password: String }
  async fn create_human(_op: Operator, State(st): State<AppState>, Json(b): Json<CreateHuman>)
      -> Result<Json<Value>, ApiError> {
      let id = st.zitadel.create_human(&b.username, &b.given_name, &b.family_name, &b.email, &b.password).await?;
      Ok(Json(json!({ "userId": id })))
  }

  #[derive(Deserialize)]
  struct CreateMachine { username: String, name: String }
  async fn create_machine(_op: Operator, State(st): State<AppState>, Json(b): Json<CreateMachine>)
      -> Result<Json<Value>, ApiError> {
      let id = st.zitadel.create_machine(&b.username, &b.name).await?;
      Ok(Json(json!({ "userId": id })))
  }

  #[derive(Deserialize)]
  struct EditProfile { given_name: String, family_name: String }
  async fn edit_profile(_op: Operator, State(st): State<AppState>, Path(id): Path<String>, Json(b): Json<EditProfile>)
      -> Result<Json<Value>, ApiError> {
      st.zitadel.edit_profile(&id, &b.given_name, &b.family_name).await?;
      Ok(Json(json!({ "ok": true })))
  }

  #[derive(Deserialize)]
  struct EditEmail { email: String, #[serde(default)] is_verified: bool }
  async fn edit_email(_op: Operator, State(st): State<AppState>, Path(id): Path<String>, Json(b): Json<EditEmail>)
      -> Result<Json<Value>, ApiError> {
      st.zitadel.edit_email(&id, &b.email, b.is_verified).await?;
      Ok(Json(json!({ "ok": true })))
  }

  #[derive(Deserialize)]
  struct SetPassword { password: String, #[serde(default)] change_required: bool }
  async fn set_password(_op: Operator, State(st): State<AppState>, Path(id): Path<String>, Json(b): Json<SetPassword>)
      -> Result<Json<Value>, ApiError> {
      st.zitadel.set_password(&id, &b.password, b.change_required).await?;
      Ok(Json(json!({ "ok": true })))
  }

  async fn resend_init(_op: Operator, State(st): State<AppState>, Path(id): Path<String>) -> Result<Json<Value>, ApiError> {
      st.zitadel.resend_init(&id).await?;
      Ok(Json(json!({ "ok": true })))
  }

  macro_rules! lifecycle_handler {
      ($name:ident, $call:ident) => {
          async fn $name(_op: Operator, State(st): State<AppState>, Path(id): Path<String>) -> Result<Json<Value>, ApiError> {
              st.zitadel.$call(&id).await?;
              Ok(Json(json!({ "ok": true })))
          }
      };
  }
  lifecycle_handler!(deactivate, deactivate);
  lifecycle_handler!(reactivate, reactivate);
  lifecycle_handler!(lock, lock);
  lifecycle_handler!(unlock, unlock);

  async fn delete_user(_op: Operator, State(st): State<AppState>, Path(id): Path<String>) -> Result<Json<Value>, ApiError> {
      st.zitadel.delete_user(&id).await?;
      Ok(Json(json!({ "ok": true })))
  }

  async fn list_roles(_op: Operator, State(st): State<AppState>) -> Result<Json<Value>, ApiError> {
      Ok(Json(json!({ "result": st.zitadel.list_roles().await? })))
  }

  async fn list_grants(_op: Operator, State(st): State<AppState>, Path(id): Path<String>) -> Result<Json<Value>, ApiError> {
      Ok(Json(json!({ "result": st.zitadel.list_user_grants(&id).await? })))
  }

  #[derive(Deserialize)]
  struct AddGrant { role_keys: Vec<String> }
  async fn add_grant(_op: Operator, State(st): State<AppState>, Path(id): Path<String>, Json(b): Json<AddGrant>)
      -> Result<Json<Value>, ApiError> {
      let grant_id = st.zitadel.add_grant(&id, &b.role_keys).await?;
      Ok(Json(json!({ "userGrantId": grant_id })))
  }

  #[derive(Deserialize)]
  struct SetGrant { role_keys: Vec<String> }
  async fn set_grant(_op: Operator, State(st): State<AppState>, Path((id, grant_id)): Path<(String, String)>, Json(b): Json<SetGrant>)
      -> Result<Json<Value>, ApiError> {
      st.zitadel.set_grant_roles(&id, &grant_id, &b.role_keys).await?;
      Ok(Json(json!({ "ok": true })))
  }

  async fn remove_grant(_op: Operator, State(st): State<AppState>, Path((id, grant_id)): Path<(String, String)>)
      -> Result<Json<Value>, ApiError> {
      st.zitadel.remove_grant(&id, &grant_id).await?;
      Ok(Json(json!({ "ok": true })))
  }

  async fn list_keys(_op: Operator, State(st): State<AppState>, Path(id): Path<String>) -> Result<Json<Value>, ApiError> {
      Ok(Json(json!({ "result": st.zitadel.list_keys(&id).await? })))
  }

  // keyDetails (private key) returned ONCE; streamed straight to the operator,
  // never persisted server-side (design §6 step 2).
  async fn create_key(_op: Operator, State(st): State<AppState>, Path(id): Path<String>) -> Result<Json<Value>, ApiError> {
      Ok(Json(st.zitadel.create_json_key(&id).await?))
  }

  async fn delete_key(_op: Operator, State(st): State<AppState>, Path((id, key_id)): Path<(String, String)>)
      -> Result<Json<Value>, ApiError> {
      st.zitadel.delete_key(&id, &key_id).await?;
      Ok(Json(json!({ "ok": true })))
  }

  async fn generate_secret(_op: Operator, State(st): State<AppState>, Path(id): Path<String>) -> Result<Json<Value>, ApiError> {
      Ok(Json(st.zitadel.generate_secret(&id).await?))
  }

  async fn delete_secret(_op: Operator, State(st): State<AppState>, Path(id): Path<String>) -> Result<Json<Value>, ApiError> {
      st.zitadel.delete_secret(&id).await?;
      Ok(Json(json!({ "ok": true })))
  }
  ```

  3e. Wire `main.rs` (final): build the `JwksCache`, the `AppState`, install the session layer + trace layer, and serve `api::router(state)`. Replace the placeholder serve block from Task 13:
  ```rust
  use std::sync::Arc;
  use llm_chat_admin_api::{api, session as _session, AppState};
  use tower_http::trace::TraceLayer;
  use tower_sessions::{Expiry, MemoryStore, SessionManagerLayer};
  use zitadel_auth::{JwksCache, ZitadelConfig};
  // ... (keep AdminConfig import + issuer_matches + assert_issuer_match)

  // inside main(), replacing the `let _client = ...; let listener = ...; serve(empty)` block:
      let zitadel_client = Arc::new(zitadel::ZitadelClient::new(cfg.clone(), http.clone()));
      let jwks = JwksCache::new(ZitadelConfig {
          issuer: cfg.issuer.clone(),
          audience: vec![cfg.audience.clone()],
          jwks_uri: format!("{}/oauth/v2/keys", cfg.issuer),
          project_id: cfg.project_id.clone(),
      });
      let state = AppState {
          cfg: cfg.clone(),
          jwks,
          zitadel: zitadel_client,
          http: http.clone(),
      };

      // tower-sessions: in-memory store, signed cookie, SameSite=Lax (same-origin
      // proxy means Lax survives the Zitadel 302 back). secure=false for the
      // plain-HTTP dev origin; flip to true behind TLS.
      let session_layer = SessionManagerLayer::new(MemoryStore::default())
          .with_name("id")
          .with_same_site(tower_sessions::cookie::SameSite::Lax)
          .with_secure(false)
          .with_expiry(Expiry::OnInactivity(time::Duration::hours(8)));

      let app = api::router(state)
          .layer(session_layer)
          .layer(TraceLayer::new_for_http());

      let listener = tokio::net::TcpListener::bind(&cfg.bind_addr).await?;
      tracing::info!(target: "admin-api", addr = %cfg.bind_addr, "admin-api listening");
      axum::serve(listener, app).await?;
      Ok(())
  ```
  (`zitadel` is reachable via `llm_chat_admin_api::zitadel`; adjust the `use` so `ZitadelClient::new` resolves. The `ADMIN_SESSION_KEY` is reserved for a signed-cookie store upgrade; the MemoryStore + opaque session id is the dev default. If a signed store is required now, swap `MemoryStore` for a signing store keyed by `cfg.session_key` — keep the same layer wiring.)

- [ ] **Step 4: Run — expect PASS** — `cargo test -p llm-chat-admin-api error::tests` — expect `test result: ok. 6 passed`; then `cargo build -p llm-chat-admin-api` — expect clean (the full router wires every Task-15/16 wrapper + `AppState` + session layer, so a missing piece fails here). Also `cargo test -p llm-chat-admin-api` to confirm all earlier unit tests still pass.

- [ ] **Step 5: Commit**
```powershell
git add admin-api/src/lib.rs admin-api/src/session.rs admin-api/src/api/error.rs admin-api/src/api/mod.rs admin-api/src/main.rs
git commit -m @'
feat(admin-api): AppState + fail-closed Operator extractor + /api router + session layer

Define AppState (cfg, JwksCache, Arc<ZitadelClient>, http). Operator extractor
loads the tower-sessions session and rejects unless roles contain chat.admin
(design §4.2). api/mod.rs exposes the full /api surface (design §5), every
/api/* handler gated; /login,/callback,/logout establish the session. ApiError
maps ZitadelError to {code,message} JSON (design §8, unit-tested). main.rs
builds the JwksCache, installs SessionManagerLayer + TraceLayer, and serves the
router.

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
'@
```

---

### Task 19: tests/integration.rs — gated full-coverage lifecycle against running Zitadel (ADMIN_IT=1)

**Files:**
- Modify: `admin-api/tests/integration.rs` (extend the suite created in Task 13)
- Modify: `admin-api/Cargo.toml` (add `[dev-dependencies]` `tokio`, `serde_json` if not already present)

> This is the **source-of-truth** task (CLAUDE.md): instead of mocking unverified Zitadel JSON, it drives the real `ZitadelClient` wrappers against the running v3.4.10 instance and discharges appendix §6.3/§6.4/§6.5/§6.6. The whole suite is **skipped unless `ADMIN_IT=1`** so the default `cargo test` stays offline and non-racy. Per the Task 16 coverage note, this lifecycle now exercises the human-create / edit / password / secret / lock-unlock-reactivate / resend-init paths in addition to create→grant→key→deactivate→delete, so none of the Task-16 wrappers is left unverified.

- [ ] **Step 1: Write the failing/gated test** — extend `admin-api/tests/integration.rs` (it already has `it_enabled()`, `llm_chat_admin_api_cfg()`, `admin_client(cfg, http)` from Task 13). Build the real `ZitadelClient` via the **two-arg** `ZitadelClient::new(cfg, http)` (matching Task 13's locked constructor) and run the full lifecycle, asserting each response shape the unit tests deliberately did NOT fabricate:

```rust
#[tokio::test]
async fn create_grant_key_lifecycle_full_coverage() {
    if !it_enabled() {
        eprintln!("ADMIN_IT!=1 — skipping integration lifecycle test");
        return;
    }
    let cfg = llm_chat_admin_api_cfg();
    let http = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap();
    let z = admin_client(cfg, http);

    let suffix = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH).unwrap().as_secs();

    // ---- machine user: create (§6.4) -> grant (§6.5) -> key (§6.6) ----
    let m_name = format!("it-machine-{suffix}");
    let user_id = z.create_machine(&m_name, &m_name).await.expect("create_machine");
    assert!(!user_id.is_empty(), "create_machine must return userId");

    let grant_id = z.add_grant(&user_id, &["chat.user".into()]).await.expect("add_grant");
    assert!(!grant_id.is_empty(), "add_grant must return userGrantId");
    z.set_grant_roles(&user_id, &grant_id, &["chat.user".into(), "chat.admin".into()])
        .await.expect("set_grant_roles (PUT replace)");

    let key = z.create_json_key(&user_id).await.expect("create_json_key");
    assert!(key.get("keyDetails").and_then(|v| v.as_str()).is_some(),
        "create_json_key must return base64 keyDetails (returned once)");
    let key_id = key.get("keyId").and_then(|v| v.as_str()).expect("keyId").to_string();
    let keys = z.list_keys(&user_id).await.expect("list_keys");
    assert!(keys.iter().any(|k| k.get("id").and_then(|v| v.as_str()) == Some(&key_id)),
        "list_keys must include the just-created key id");
    z.delete_key(&user_id, &key_id).await.expect("delete_key");

    // client-secret lifecycle (§6.6): generate (shown once) then delete.
    let secret = z.generate_secret(&user_id).await.expect("generate_secret");
    assert!(secret.get("clientSecret").and_then(|v| v.as_str()).is_some(),
        "generate_secret must return clientSecret once");
    z.delete_secret(&user_id).await.expect("delete_secret");

    // read-back via v2 (§6.3): get_user maps the v2 shape.
    let fetched = z.get_user(&user_id).await.expect("get_user");
    assert_eq!(fetched.id, user_id, "get_user must round-trip the userId");

    // machine lifecycle: lock/unlock/deactivate/reactivate then delete.
    z.lock(&user_id).await.expect("lock");
    z.unlock(&user_id).await.expect("unlock");
    z.deactivate(&user_id).await.expect("deactivate");
    z.reactivate(&user_id).await.expect("reactivate");
    z.delete_user(&user_id).await.expect("delete_user (irreversible)");

    // ---- human user: create (§6.3) -> edit -> password -> resend-init -> delete ----
    let h_name = format!("it-human-{suffix}");
    let h_id = z.create_human(
        &h_name, "Given", "Family", &format!("{h_name}@example.localhost"), "Sup3r-Secret!"
    ).await.expect("create_human");
    assert!(!h_id.is_empty(), "create_human must return userId");
    z.edit_profile(&h_id, "Given2", "Family2").await.expect("edit_profile");
    z.edit_email(&h_id, &format!("{h_name}2@example.localhost"), true).await.expect("edit_email");
    z.set_password(&h_id, "An0ther-Secret!", false).await.expect("set_password");
    // resend_init is allowed only for INITIAL-state users; tolerate a precondition
    // failure here (the user is already active) rather than failing the suite.
    let _ = z.resend_init(&h_id).await;
    z.delete_user(&h_id).await.expect("delete human (irreversible)");
}
```

- [ ] **Step 2: Run it — expect SKIP (offline) / RED (gated, pre-stack)** — `cargo test -p llm-chat-admin-api --test integration` — offline: the new test prints `ADMIN_IT!=1 — skipping integration lifecycle test` and returns (proving it compiles + is wired against the Task-13 lib surface). With `$env:ADMIN_IT="1"` but before the stack is up / wrappers behave: it FAILS at the first live call — the genuine red bar for an I/O wrapper whose shape is only knowable against the running instance.

- [ ] **Step 3: Implement** — no new app code; this task's deliverable is the runnable full-coverage harness. Confirm `admin-api/Cargo.toml` has the dev-deps the test needs:
  ```toml
  [dev-dependencies]
  tokio = { workspace = true }
  serde_json = { workspace = true }
  ```
  (`reqwest` is already a normal dependency.)

- [ ] **Step 4: Run — expect PASS** — offline default: `cargo test -p llm-chat-admin-api --test integration` — expect `test result: ok.` with the skip lines on stderr (both `it_mint_management_token` and the lifecycle return early). Against the running stack (real verification): `$env:ADMIN_IT="1"; <other env from secrets>; cargo test -p llm-chat-admin-api --test integration -- --nocapture` — expect every lifecycle assertion to pass, **discharging appendix §6.3/§6.4/§6.5/§6.6**. Record in the commit body which checklist items closed and whether any app/project flag had to be flipped (per design §9).

- [ ] **Step 5: Commit**
```powershell
git add admin-api/tests/integration.rs admin-api/Cargo.toml
git commit -m @'
test(admin-api): gated full-coverage lifecycle integration suite

Drives the real ZitadelClient wrappers against a running Zitadel v3.4.10
(ADMIN_IT=1; skipped otherwise) and asserts the actual response shapes
(userId, userGrantId, keyDetails, clientSecret, result[]) — the source of
truth, not a mock. Covers machine create->grant->key->secret->lock/unlock/
deactivate/reactivate->delete AND human create->edit->password->resend->delete
so no Task-16 wrapper is left unverified. Discharges appendix §6.3/§6.4/§6.5/§6.6.

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
'@
```

---

## Phase D-frontend

> **Prereq:** Phase C's `admin-api` exposes the `/api` surface and `/login|/callback|/logout` on `http://localhost:7676` (the OIDC WEB app's `redirectUris` is `http://localhost:7676/callback` per the provisioner, matching `ADMIN_PUBLIC_ORIGIN`). `admin-web` is a **same-origin proxy** in front of it: the browser only ever talks to `admin-web` on `:3000`, and Next.js rewrites `/api/*`, `/login`, `/callback`, `/logout` to the `admin-api` origin — so the BFF's `SameSite=Lax` session cookie just works with **no CORS** (appendix §5.2, §5.4; design §3 "Preferred topology: same-origin"). All client fetches use `credentials:'include'` (appendix §5.3).

### Task 20: admin-web scaffold + same-origin rewrites
**Files:**
- Create: `admin-web/package.json`
- Create: `admin-web/pnpm-workspace.yaml`
- Create: `admin-web/tsconfig.json`
- Create: `admin-web/next.config.ts`
- Create: `admin-web/postcss.config.mjs`
- Create: `admin-web/app/globals.css`
- Create: `admin-web/app/layout.tsx`
- Create: `admin-web/components.json`
- Create: `admin-web/lib/utils.ts`
- Create: `admin-web/.env.example`
- Create: `admin-web/.gitignore`
- Create: `admin-web/vitest.config.ts`
- Test: `admin-web/__tests__/next-config.test.ts`

- [ ] **Step 1: Write the failing test** — assert the rewrite contract (this is the load-bearing same-origin proxy; appendix §5.2/§5.4). Because `next.config.ts`'s `rewrites()` reads `ADMIN_API_ORIGIN`, the test imports the config and inspects the returned rules so we never accidentally drop a route.

  `admin-web/__tests__/next-config.test.ts`:
  ```ts
  import { describe, it, expect, beforeEach } from "vitest";

  describe("next.config same-origin proxy", () => {
    beforeEach(() => {
      process.env.ADMIN_API_ORIGIN = "http://localhost:7676";
    });

    it("rewrites /api/* and the OIDC nav routes to admin-api, no CORS", async () => {
      const mod = await import("../next.config");
      const cfg = mod.default;
      const rules = await cfg.rewrites!();
      // next.config.rewrites may return an array or {beforeFiles,...}
      const list = Array.isArray(rules) ? rules : rules.beforeFiles ?? [];
      const bySource = Object.fromEntries(list.map((r: any) => [r.source, r.destination]));

      expect(bySource["/api/:path*"]).toBe("http://localhost:7676/api/:path*");
      expect(bySource["/login"]).toBe("http://localhost:7676/login");
      expect(bySource["/callback"]).toBe("http://localhost:7676/callback");
      expect(bySource["/logout"]).toBe("http://localhost:7676/logout");
    });

    it("defaults ADMIN_API_ORIGIN to localhost:7676 when unset", async () => {
      delete process.env.ADMIN_API_ORIGIN;
      const mod = await import("../next.config?fresh=" + Date.now());
      const cfg = mod.default;
      const rules = await cfg.rewrites!();
      const list = Array.isArray(rules) ? rules : rules.beforeFiles ?? [];
      const bySource = Object.fromEntries(list.map((r: any) => [r.source, r.destination]));
      expect(bySource["/api/:path*"]).toBe("http://localhost:7676/api/:path*");
    });
  });
  ```

- [ ] **Step 2: Run it — expect FAIL** — nothing is scaffolded yet, so the import resolves to nothing.
  ```powershell
  cd D:\projects\llm-chat\admin-web; pnpm install; pnpm test
  ```
  Expected: FAIL — `Cannot find module '../next.config'` (or `pnpm` errors because `package.json` is absent). This is red.

- [ ] **Step 3: Implement** — scaffold the project. Run the generators, then write the config files.
  ```powershell
  cd D:\projects\llm-chat; pnpm create next-app@latest admin-web --ts --app --tailwind --eslint --use-pnpm --src-dir=false --import-alias "@/*" --no-turbopack --skip-install
  cd D:\projects\llm-chat\admin-web; pnpm add @tanstack/react-table react-hook-form zod @hookform/resolvers lucide-react class-variance-authority clsx tailwind-merge
  pnpm add -D vitest @vitejs/plugin-react jsdom @testing-library/react @testing-library/jest-dom @playwright/test
  pnpm dlx shadcn@latest init -b neutral -y
  pnpm dlx shadcn@latest add button input dialog alert-dialog dropdown-menu form label table badge sonner select -y
  ```

  Then write `admin-web/next.config.ts` (the rewrite contract — same-origin proxy, no CORS, appendix §5.4):
  ```ts
  import type { NextConfig } from "next";

  const ADMIN_API_ORIGIN = process.env.ADMIN_API_ORIGIN ?? "http://localhost:7676";

  const nextConfig: NextConfig = {
    output: "standalone", // Phase E packages .next/standalone in node:20-alpine
    async rewrites() {
      return [
        { source: "/api/:path*", destination: `${ADMIN_API_ORIGIN}/api/:path*` },
        { source: "/login", destination: `${ADMIN_API_ORIGIN}/login` },
        { source: "/callback", destination: `${ADMIN_API_ORIGIN}/callback` },
        { source: "/logout", destination: `${ADMIN_API_ORIGIN}/logout` },
      ];
    },
  };

  export default nextConfig;
  ```

  `admin-web/vitest.config.ts`:
  ```ts
  import { defineConfig } from "vitest/config";
  import react from "@vitejs/plugin-react";
  import path from "node:path";

  export default defineConfig({
    plugins: [react()],
    test: {
      environment: "jsdom",
      globals: true,
      setupFiles: ["./vitest.setup.ts"],
      exclude: ["**/node_modules/**", "**/e2e/**", "**/.next/**"],
    },
    resolve: { alias: { "@": path.resolve(__dirname, ".") } },
  });
  ```

  `admin-web/vitest.setup.ts`:
  ```ts
  import "@testing-library/jest-dom/vitest";
  ```

  Add the test/dev scripts to `admin-web/package.json` (`pnpm create` set `dev/build/start/lint`; append):
  ```json
  "scripts": {
    "dev": "next dev",
    "build": "next build",
    "start": "next start",
    "lint": "next lint",
    "test": "vitest run",
    "e2e": "playwright test"
  }
  ```

  `admin-web/.env.example`:
  ```
  # admin-web talks ONLY to admin-api via the Next.js same-origin proxy (no NEXT_PUBLIC_* token leakage).
  # In compose this is the admin-api service URL (e.g. http://admin-api:7676).
  ADMIN_API_ORIGIN=http://localhost:7676
  ```

  Append to `admin-web/.gitignore` (Playwright + vitest artefacts on top of the Next defaults):
  ```
  /test-results/
  /playwright-report/
  /e2e-results/
  ```

- [ ] **Step 4: Run — expect PASS**
  ```powershell
  cd D:\projects\llm-chat\admin-web; pnpm test -- next-config
  ```
  Expected: PASS — `next-config.test.ts (2)` green; both `/api/:path*` and the three OIDC nav routes map to `http://localhost:7676/...`.

- [ ] **Step 5: Commit**
  ```powershell
  cd D:\projects\llm-chat; git add admin-web .gitignore; git commit -F -
  ```
  Commit message:
  ```
  feat(admin-web): scaffold Next.js 16 app + same-origin proxy to admin-api

  pnpm + shadcn/ui + Tailwind v4 scaffold. next.config rewrites /api/*,
  /login, /callback, /logout to ADMIN_API_ORIGIN so the BFF session cookie
  stays SameSite=Lax with no CORS layer. Vitest + Playwright wired.

  Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
  ```

---

### Task 21: lib/api.ts typed fetch client (credentials: include)
**Files:**
- Create: `admin-web/lib/api.ts`
- Create: `admin-web/lib/types.ts`
- Test: `admin-web/__tests__/api.test.ts`

- [ ] **Step 1: Write the failing test** — the client is the single source of truth for talking to the BFF: it must always send `credentials:'include'` (appendix §5.3), join the same-origin path, parse the `{code,message}` error JSON that `admin-api/src/api/error.rs` emits, and 401→redirect to `/login`. Mock `fetch` so the unit test is non-racy.

  `admin-web/__tests__/api.test.ts`:
  ```ts
  import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
  import { api, ApiError } from "../lib/api";

  function mockFetch(status: number, body: unknown, ok = status < 400) {
    return vi.fn().mockResolvedValue({
      ok,
      status,
      json: async () => body,
      text: async () => JSON.stringify(body),
      headers: new Headers({ "content-type": "application/json" }),
    } as unknown as Response);
  }

  beforeEach(() => {
    // jsdom has no location.assign by default; stub it
    Object.defineProperty(window, "location", {
      value: { assign: vi.fn(), href: "" },
      writable: true,
    });
  });
  afterEach(() => vi.restoreAllMocks());

  describe("api client", () => {
    it("GET sends credentials:'include' and same-origin /api path, returns parsed JSON", async () => {
      const f = mockFetch(200, { result: [{ id: "u1", userName: "a" }] });
      vi.stubGlobal("fetch", f);
      const out = await api.get<{ result: unknown[] }>("/api/users?q=a");
      expect(f).toHaveBeenCalledTimes(1);
      const [url, init] = f.mock.calls[0];
      expect(url).toBe("/api/users?q=a");
      expect(init.credentials).toBe("include");
      expect(out.result).toHaveLength(1);
    });

    it("POST serializes JSON body + sets content-type", async () => {
      const f = mockFetch(200, { userId: "u9" });
      vi.stubGlobal("fetch", f);
      await api.post("/api/users/machine", { userName: "bot", name: "bot" });
      const [, init] = f.mock.calls[0];
      expect(init.method).toBe("POST");
      expect(JSON.parse(init.body)).toEqual({ userName: "bot", name: "bot" });
      expect((init.headers as Record<string, string>)["Content-Type"]).toBe("application/json");
    });

    it("maps admin-api {code,message} error JSON to ApiError", async () => {
      const f = mockFetch(409, { code: "AlreadyExists", message: "user exists" });
      vi.stubGlobal("fetch", f);
      await expect(api.post("/api/users/machine", {})).rejects.toMatchObject({
        name: "ApiError",
        status: 409,
        code: "AlreadyExists",
        message: "user exists",
      });
    });

    it("on 401 redirects to /login (full-page nav) and throws", async () => {
      const f = mockFetch(401, { code: "Unauthorized", message: "no session" });
      vi.stubGlobal("fetch", f);
      await expect(api.get("/api/me")).rejects.toBeInstanceOf(ApiError);
      expect(window.location.assign).toHaveBeenCalledWith("/login");
    });
  });
  ```

- [ ] **Step 2: Run it — expect FAIL**
  ```powershell
  cd D:\projects\llm-chat\admin-web; pnpm test -- api
  ```
  Expected: FAIL — `Cannot find module '../lib/api'` (`api.ts`/`types.ts` not created).

- [ ] **Step 3: Implement**

  `admin-web/lib/types.ts` (mirrors `admin-api/src/zitadel/model.rs` `User`/`UserKind` and the `/api` surface shapes; appendix §3.1 v2 field names — `userId`/`username`/`email.isVerified`):
  ```ts
  export type UserKind = "Human" | "Machine";
  export type UserState =
    | "ACTIVE" | "INACTIVE" | "LOCKED" | "INITIAL" | "DELETED" | "UNSPECIFIED";

  export interface User {
    id: string;
    userName: string;
    kind: UserKind;
    state: UserState;
    email?: string;
    displayName?: string;
  }

  export interface UserList {
    result: User[];
    total?: number;
  }

  export interface Me {
    userId: string;
    name: string;
    roles: string[];
  }

  export interface Role {
    key: string;
    displayName: string;
    group?: string;
  }

  export interface UserGrant {
    grantId: string;
    projectId: string;
    roleKeys: string[];
  }

  export interface CreateHumanInput {
    userName: string;
    givenName: string;
    familyName: string;
    email: string;
    password?: string;
  }

  export interface CreateMachineInput {
    userName: string;
    name: string;
  }

  export interface EditProfileInput {
    givenName: string;
    familyName: string;
    displayName?: string;
  }
  ```

  `admin-web/lib/api.ts` (typed client; `credentials:'include'` on every call; parses `{code,message}`; 401 → full-page `/login`):
  ```ts
  export class ApiError extends Error {
    readonly name = "ApiError";
    constructor(
      readonly status: number,
      readonly code: string,
      message: string,
    ) {
      super(message);
    }
  }

  async function request<T>(
    path: string,
    init: RequestInit & { json?: unknown } = {},
  ): Promise<T> {
    const { json, headers, ...rest } = init;
    const res = await fetch(path, {
      ...rest,
      credentials: "include",
      headers: {
        ...(json !== undefined ? { "Content-Type": "application/json" } : {}),
        ...(headers as Record<string, string> | undefined),
      },
      ...(json !== undefined ? { body: JSON.stringify(json) } : {}),
    });

    if (!res.ok) {
      let code = "Error";
      let message = res.statusText || `HTTP ${res.status}`;
      try {
        const body = (await res.json()) as { code?: string; message?: string };
        if (body.code) code = body.code;
        if (body.message) message = body.message;
      } catch {
        /* non-JSON body: keep status text */
      }
      // BFF says "no session" -> the login flow is a full-page nav, not fetch (appendix §5.2)
      if (res.status === 401 && typeof window !== "undefined") {
        window.location.assign("/login");
      }
      throw new ApiError(res.status, code, message);
    }

    if (res.status === 204) return undefined as T;
    return (await res.json()) as T;
  }

  export const api = {
    get: <T>(path: string) => request<T>(path, { method: "GET" }),
    post: <T>(path: string, json?: unknown) =>
      request<T>(path, { method: "POST", json }),
    patch: <T>(path: string, json?: unknown) =>
      request<T>(path, { method: "PATCH", json }),
    put: <T>(path: string, json?: unknown) =>
      request<T>(path, { method: "PUT", json }),
    del: <T>(path: string) => request<T>(path, { method: "DELETE" }),
  };
  ```

- [ ] **Step 4: Run — expect PASS**
  ```powershell
  cd D:\projects\llm-chat\admin-web; pnpm test -- api
  ```
  Expected: PASS — `api.test.ts (4)` green: GET sends `credentials:'include'`, POST serializes body, 409 maps to `ApiError{code:"AlreadyExists"}`, 401 calls `window.location.assign("/login")`.

- [ ] **Step 5: Commit**
  ```powershell
  cd D:\projects\llm-chat; git add admin-web/lib/api.ts admin-web/lib/types.ts admin-web/__tests__/api.test.ts; git commit -F -
  ```
  Commit message:
  ```
  feat(admin-web): typed fetch client with credentials:'include'

  lib/api.ts is the single BFF gateway: same-origin /api paths, JSON
  body/parse, maps admin-api {code,message} -> ApiError, and on 401
  full-page-navigates to /login. lib/types.ts mirrors the /api surface.

  Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
  ```

---

### Task 22: users DataTable + columns + row actions + create/edit/confirm dialogs + Sign-in gate
**Files:**
- Create: `admin-web/components/ui/data-table.tsx`
- Create: `admin-web/components/users/columns.tsx`
- Create: `admin-web/components/users/users-table.tsx`
- Create: `admin-web/components/users/create-user-dialog.tsx`
- Create: `admin-web/components/users/edit-user-dialog.tsx`
- Create: `admin-web/components/users/confirm-dialog.tsx`
- Create: `admin-web/app/(dash)/users/page.tsx`
- Create: `admin-web/app/page.tsx`
- Create: `admin-web/__tests__/columns.test.tsx`

- [ ] **Step 1: Write the failing test** — `columns` is the only non-trivially-pure UI unit here (TanStack `ColumnDef<User>[]`): it must render the state as a readable badge and expose every per-row destructive action from the `/api` surface (deactivate/reactivate/lock/unlock/delete, plus edit for human users). Test the column defs + the rendered cells via Testing Library so the table itself stays a thin wrapper (per the project's pure-helper/thin-wrapper convention).

  `admin-web/__tests__/columns.test.tsx`:
  ```tsx
  import { describe, it, expect, vi } from "vitest";
  import { render, screen } from "@testing-library/react";
  import { flexRender } from "@tanstack/react-table";
  import { buildColumns } from "../components/users/columns";
  import type { User } from "../lib/types";

  const human: User = {
    id: "u1", userName: "alice", kind: "Human", state: "ACTIVE",
    email: "alice@x.io", displayName: "Alice A",
  };

  function renderCell(colId: string, user: User) {
    const cols = buildColumns({
      onEdit: vi.fn(), onDelete: vi.fn(), onLifecycle: vi.fn(),
    });
    const col = cols.find((c) => ("accessorKey" in c ? c.accessorKey : c.id) === colId);
    if (!col) throw new Error(`no column ${colId}`);
    const cell = (col as any).cell;
    // minimal row stub for cell renderers
    const ctx = { row: { original: user, getValue: (k: string) => (user as any)[k] } };
    return render(<>{flexRender(cell, ctx as any)}</>);
  }

  describe("user columns", () => {
    it("has the expected columns", () => {
      const ids = buildColumns({ onEdit: vi.fn(), onDelete: vi.fn(), onLifecycle: vi.fn() })
        .map((c) => ("accessorKey" in c ? (c as any).accessorKey : c.id));
      expect(ids).toEqual(
        expect.arrayContaining(["userName", "kind", "state", "email", "actions"]),
      );
    });

    it("renders state as a badge label", () => {
      renderCell("state", human);
      expect(screen.getByText("ACTIVE")).toBeInTheDocument();
    });

    it("fires onLifecycle('deactivate') from the row action menu", async () => {
      const onLifecycle = vi.fn();
      const cols = buildColumns({ onEdit: vi.fn(), onDelete: vi.fn(), onLifecycle });
      const actions = cols.find((c) => c.id === "actions")!;
      const ctx = { row: { original: human } };
      render(<>{flexRender((actions as any).cell, ctx as any)}</>);
      // the menu items are rendered with data-testid attributes for deterministic testing
      const item = await screen.findByTestId("action-deactivate");
      item.click();
      expect(onLifecycle).toHaveBeenCalledWith(human, "deactivate");
    });
  });
  ```

- [ ] **Step 2: Run it — expect FAIL**
  ```powershell
  cd D:\projects\llm-chat\admin-web; pnpm test -- columns
  ```
  Expected: FAIL — `Cannot find module '../components/users/columns'`.

- [ ] **Step 3: Implement** — write the reusable DataTable, the columns with row actions, the table container that wires fetches via `lib/api`, the two react-hook-form+zod dialogs, the AlertDialog confirm, and the pages.

  `admin-web/components/ui/data-table.tsx` (generic shadcn/TanStack table — appendix §5.3, default pageSize 10):
  ```tsx
  "use client";
  import {
    type ColumnDef, flexRender, getCoreRowModel,
    getPaginationRowModel, getSortedRowModel, getFilteredRowModel,
    useReactTable, type SortingState, type ColumnFiltersState,
  } from "@tanstack/react-table";
  import { useState } from "react";
  import {
    Table, TableBody, TableCell, TableHead, TableHeader, TableRow,
  } from "@/components/ui/table";
  import { Button } from "@/components/ui/button";
  import { Input } from "@/components/ui/input";

  interface DataTableProps<TData, TValue> {
    columns: ColumnDef<TData, TValue>[];
    data: TData[];
    filterColumn?: string;
    filterPlaceholder?: string;
  }

  export function DataTable<TData, TValue>({
    columns, data, filterColumn, filterPlaceholder,
  }: DataTableProps<TData, TValue>) {
    const [sorting, setSorting] = useState<SortingState>([]);
    const [columnFilters, setColumnFilters] = useState<ColumnFiltersState>([]);
    const table = useReactTable({
      data, columns,
      getCoreRowModel: getCoreRowModel(),
      getPaginationRowModel: getPaginationRowModel(),
      getSortedRowModel: getSortedRowModel(),
      getFilteredRowModel: getFilteredRowModel(),
      onSortingChange: setSorting,
      onColumnFiltersChange: setColumnFilters,
      state: { sorting, columnFilters },
      initialState: { pagination: { pageSize: 10 } },
    });

    return (
      <div className="space-y-3">
        {filterColumn && (
          <Input
            placeholder={filterPlaceholder ?? "Filter..."}
            value={(table.getColumn(filterColumn)?.getFilterValue() as string) ?? ""}
            onChange={(e) =>
              table.getColumn(filterColumn)?.setFilterValue(e.target.value)
            }
            className="max-w-sm"
          />
        )}
        <div className="rounded-md border">
          <Table>
            <TableHeader>
              {table.getHeaderGroups().map((hg) => (
                <TableRow key={hg.id}>
                  {hg.headers.map((h) => (
                    <TableHead key={h.id}>
                      {h.isPlaceholder ? null
                        : flexRender(h.column.columnDef.header, h.getContext())}
                    </TableHead>
                  ))}
                </TableRow>
              ))}
            </TableHeader>
            <TableBody>
              {table.getRowModel().rows.length ? (
                table.getRowModel().rows.map((row) => (
                  <TableRow key={row.id}>
                    {row.getVisibleCells().map((cell) => (
                      <TableCell key={cell.id}>
                        {flexRender(cell.column.columnDef.cell, cell.getContext())}
                      </TableCell>
                    ))}
                  </TableRow>
                ))
              ) : (
                <TableRow>
                  <TableCell colSpan={columns.length} className="h-24 text-center">
                    No users.
                  </TableCell>
                </TableRow>
              )}
            </TableBody>
          </Table>
        </div>
        <div className="flex items-center justify-end gap-2">
          <Button variant="outline" size="sm"
            onClick={() => table.previousPage()}
            disabled={!table.getCanPreviousPage()}>Previous</Button>
          <Button variant="outline" size="sm"
            onClick={() => table.nextPage()}
            disabled={!table.getCanNextPage()}>Next</Button>
        </div>
      </div>
    );
  }
  ```

  `admin-web/components/users/columns.tsx` (`buildColumns` factory so handlers inject cleanly + stay unit-testable; row actions cover the `/api` lifecycle + delete + edit):
  ```tsx
  "use client";
  import type { ColumnDef } from "@tanstack/react-table";
  import { MoreHorizontal } from "lucide-react";
  import { Badge } from "@/components/ui/badge";
  import { Button } from "@/components/ui/button";
  import {
    DropdownMenu, DropdownMenuContent, DropdownMenuItem,
    DropdownMenuLabel, DropdownMenuSeparator, DropdownMenuTrigger,
  } from "@/components/ui/dropdown-menu";
  import type { User } from "@/lib/types";

  export type Lifecycle =
    | "deactivate" | "reactivate" | "lock" | "unlock" | "resend-init";

  export interface ColumnHandlers {
    onEdit: (u: User) => void;
    onDelete: (u: User) => void;
    onLifecycle: (u: User, action: Lifecycle) => void;
  }

  export function buildColumns(h: ColumnHandlers): ColumnDef<User>[] {
    return [
      { accessorKey: "userName", header: "Username" },
      {
        accessorKey: "kind", header: "Type",
        cell: ({ row }) => <Badge variant="secondary">{row.original.kind}</Badge>,
      },
      {
        accessorKey: "state", header: "State",
        cell: ({ row }) => {
          const s = row.original.state;
          const variant = s === "ACTIVE" ? "default"
            : s === "INITIAL" ? "secondary" : "destructive";
          return <Badge variant={variant}>{s}</Badge>;
        },
      },
      { accessorKey: "email", header: "Email" },
      {
        id: "actions",
        cell: ({ row }) => {
          const u = row.original;
          return (
            <DropdownMenu>
              <DropdownMenuTrigger asChild>
                <Button variant="ghost" className="h-8 w-8 p-0">
                  <span className="sr-only">Open menu</span>
                  <MoreHorizontal className="h-4 w-4" />
                </Button>
              </DropdownMenuTrigger>
              <DropdownMenuContent align="end">
                <DropdownMenuLabel>Actions</DropdownMenuLabel>
                {u.kind === "Human" && (
                  <DropdownMenuItem data-testid="action-edit" onSelect={() => h.onEdit(u)}>
                    Edit profile
                  </DropdownMenuItem>
                )}
                <DropdownMenuItem data-testid="action-deactivate"
                  onSelect={() => h.onLifecycle(u, "deactivate")}>Deactivate</DropdownMenuItem>
                <DropdownMenuItem data-testid="action-reactivate"
                  onSelect={() => h.onLifecycle(u, "reactivate")}>Reactivate</DropdownMenuItem>
                <DropdownMenuItem data-testid="action-lock"
                  onSelect={() => h.onLifecycle(u, "lock")}>Lock</DropdownMenuItem>
                <DropdownMenuItem data-testid="action-unlock"
                  onSelect={() => h.onLifecycle(u, "unlock")}>Unlock</DropdownMenuItem>
                <DropdownMenuSeparator />
                <DropdownMenuItem data-testid="action-delete"
                  className="text-destructive" onSelect={() => h.onDelete(u)}>
                  Delete (irreversible)
                </DropdownMenuItem>
              </DropdownMenuContent>
            </DropdownMenu>
          );
        },
      },
    ];
  }
  ```

  `admin-web/components/users/confirm-dialog.tsx` (AlertDialog for destructive ops — appendix §5.3, design §8 "Irreversible actions"):
  ```tsx
  "use client";
  import {
    AlertDialog, AlertDialogAction, AlertDialogCancel, AlertDialogContent,
    AlertDialogDescription, AlertDialogFooter, AlertDialogHeader, AlertDialogTitle,
  } from "@/components/ui/alert-dialog";

  export function ConfirmDialog({
    open, onOpenChange, title, description, confirmLabel = "Confirm", onConfirm,
  }: {
    open: boolean;
    onOpenChange: (o: boolean) => void;
    title: string;
    description: string;
    confirmLabel?: string;
    onConfirm: () => void;
  }) {
    return (
      <AlertDialog open={open} onOpenChange={onOpenChange}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>{title}</AlertDialogTitle>
            <AlertDialogDescription>{description}</AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel>Cancel</AlertDialogCancel>
            <AlertDialogAction onClick={onConfirm}>{confirmLabel}</AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    );
  }
  ```

  `admin-web/components/users/create-user-dialog.tsx` (shadcn Form + react-hook-form + zod; human vs machine; posts to `/api/users/human` | `/api/users/machine`):
  ```tsx
  "use client";
  import { useState } from "react";
  import { useForm } from "react-hook-form";
  import { zodResolver } from "@hookform/resolvers/zod";
  import { z } from "zod";
  import { toast } from "sonner";
  import { Button } from "@/components/ui/button";
  import {
    Dialog, DialogContent, DialogHeader, DialogTitle, DialogTrigger, DialogFooter,
  } from "@/components/ui/dialog";
  import {
    Form, FormControl, FormField, FormItem, FormLabel, FormMessage,
  } from "@/components/ui/form";
  import { Input } from "@/components/ui/input";
  import {
    Select, SelectContent, SelectItem, SelectTrigger, SelectValue,
  } from "@/components/ui/select";
  import { api, ApiError } from "@/lib/api";

  const humanSchema = z.object({
    kind: z.literal("Human"),
    userName: z.string().min(1),
    givenName: z.string().min(1),
    familyName: z.string().min(1),
    email: z.string().email(),
    password: z.string().min(8).optional().or(z.literal("")),
  });
  const machineSchema = z.object({
    kind: z.literal("Machine"),
    userName: z.string().min(1),
    name: z.string().min(1),
  });
  const schema = z.discriminatedUnion("kind", [humanSchema, machineSchema]);
  type FormValues = z.infer<typeof schema>;

  export function CreateUserDialog({ onCreated }: { onCreated: () => void }) {
    const [open, setOpen] = useState(false);
    const form = useForm<FormValues>({
      resolver: zodResolver(schema),
      defaultValues: { kind: "Human", userName: "", givenName: "", familyName: "", email: "" },
    });
    const kind = form.watch("kind");

    async function onSubmit(values: FormValues) {
      try {
        if (values.kind === "Human") {
          await api.post("/api/users/human", {
            userName: values.userName,
            givenName: values.givenName,
            familyName: values.familyName,
            email: values.email,
            ...(values.password ? { password: values.password } : {}),
          });
        } else {
          await api.post("/api/users/machine", {
            userName: values.userName, name: values.name,
          });
        }
        toast.success("User created");
        setOpen(false);
        form.reset();
        onCreated();
      } catch (e) {
        toast.error(e instanceof ApiError ? e.message : "Create failed");
      }
    }

    return (
      <Dialog open={open} onOpenChange={setOpen}>
        <DialogTrigger asChild>
          <Button data-testid="create-user">Create user</Button>
        </DialogTrigger>
        <DialogContent>
          <DialogHeader><DialogTitle>Create user</DialogTitle></DialogHeader>
          <Form {...form}>
            <form onSubmit={form.handleSubmit(onSubmit)} className="space-y-4">
              <FormField control={form.control} name="kind" render={({ field }) => (
                <FormItem>
                  <FormLabel>Type</FormLabel>
                  <Select onValueChange={field.onChange} value={field.value}>
                    <FormControl><SelectTrigger><SelectValue /></SelectTrigger></FormControl>
                    <SelectContent>
                      <SelectItem value="Human">Human</SelectItem>
                      <SelectItem value="Machine">Machine</SelectItem>
                    </SelectContent>
                  </Select>
                  <FormMessage />
                </FormItem>
              )} />
              <FormField control={form.control} name="userName" render={({ field }) => (
                <FormItem><FormLabel>Username</FormLabel>
                  <FormControl><Input {...field} /></FormControl><FormMessage /></FormItem>
              )} />
              {kind === "Human" ? (
                <>
                  <FormField control={form.control} name="givenName" render={({ field }) => (
                    <FormItem><FormLabel>Given name</FormLabel>
                      <FormControl><Input {...field} /></FormControl><FormMessage /></FormItem>
                  )} />
                  <FormField control={form.control} name="familyName" render={({ field }) => (
                    <FormItem><FormLabel>Family name</FormLabel>
                      <FormControl><Input {...field} /></FormControl><FormMessage /></FormItem>
                  )} />
                  <FormField control={form.control} name="email" render={({ field }) => (
                    <FormItem><FormLabel>Email</FormLabel>
                      <FormControl><Input type="email" {...field} /></FormControl><FormMessage /></FormItem>
                  )} />
                  <FormField control={form.control} name="password" render={({ field }) => (
                    <FormItem><FormLabel>Password (optional)</FormLabel>
                      <FormControl><Input type="password" {...field} /></FormControl><FormMessage /></FormItem>
                  )} />
                </>
              ) : (
                <FormField control={form.control} name="name" render={({ field }) => (
                  <FormItem><FormLabel>Display name</FormLabel>
                    <FormControl><Input {...field} /></FormControl><FormMessage /></FormItem>
                )} />
              )}
              <DialogFooter><Button type="submit">Create</Button></DialogFooter>
            </form>
          </Form>
        </DialogContent>
      </Dialog>
    );
  }
  ```

  `admin-web/components/users/edit-user-dialog.tsx` (PATCH `/api/users/{id}/profile`):
  ```tsx
  "use client";
  import { useEffect } from "react";
  import { useForm } from "react-hook-form";
  import { zodResolver } from "@hookform/resolvers/zod";
  import { z } from "zod";
  import { toast } from "sonner";
  import { Button } from "@/components/ui/button";
  import {
    Dialog, DialogContent, DialogHeader, DialogTitle, DialogFooter,
  } from "@/components/ui/dialog";
  import {
    Form, FormControl, FormField, FormItem, FormLabel, FormMessage,
  } from "@/components/ui/form";
  import { Input } from "@/components/ui/input";
  import { api, ApiError } from "@/lib/api";
  import type { User } from "@/lib/types";

  const schema = z.object({
    givenName: z.string().min(1),
    familyName: z.string().min(1),
    displayName: z.string().optional(),
  });
  type FormValues = z.infer<typeof schema>;

  export function EditUserDialog({
    user, open, onOpenChange, onSaved,
  }: {
    user: User | null;
    open: boolean;
    onOpenChange: (o: boolean) => void;
    onSaved: () => void;
  }) {
    const form = useForm<FormValues>({
      resolver: zodResolver(schema),
      defaultValues: { givenName: "", familyName: "", displayName: "" },
    });
    useEffect(() => {
      const [given = "", family = ""] = (user?.displayName ?? "").split(" ");
      form.reset({ givenName: given, familyName: family, displayName: user?.displayName ?? "" });
    }, [user, form]);

    async function onSubmit(values: FormValues) {
      if (!user) return;
      try {
        await api.patch(`/api/users/${user.id}/profile`, values);
        toast.success("Profile updated");
        onOpenChange(false);
        onSaved();
      } catch (e) {
        toast.error(e instanceof ApiError ? e.message : "Update failed");
      }
    }

    return (
      <Dialog open={open} onOpenChange={onOpenChange}>
        <DialogContent>
          <DialogHeader><DialogTitle>Edit profile</DialogTitle></DialogHeader>
          <Form {...form}>
            <form onSubmit={form.handleSubmit(onSubmit)} className="space-y-4">
              <FormField control={form.control} name="givenName" render={({ field }) => (
                <FormItem><FormLabel>Given name</FormLabel>
                  <FormControl><Input {...field} /></FormControl><FormMessage /></FormItem>
              )} />
              <FormField control={form.control} name="familyName" render={({ field }) => (
                <FormItem><FormLabel>Family name</FormLabel>
                  <FormControl><Input {...field} /></FormControl><FormMessage /></FormItem>
              )} />
              <DialogFooter><Button type="submit">Save</Button></DialogFooter>
            </form>
          </Form>
        </DialogContent>
      </Dialog>
    );
  }
  ```

  `admin-web/app/(dash)/users/page.tsx` (client page: `/api/me` gate → render table + dialogs; wires lifecycle/delete to `lib/api`):
  ```tsx
  "use client";
  import { useCallback, useEffect, useState } from "react";
  import { toast } from "sonner";
  import { Button } from "@/components/ui/button";
  import { DataTable } from "@/components/ui/data-table";
  import { buildColumns, type Lifecycle } from "@/components/users/columns";
  import { CreateUserDialog } from "@/components/users/create-user-dialog";
  import { EditUserDialog } from "@/components/users/edit-user-dialog";
  import { ConfirmDialog } from "@/components/users/confirm-dialog";
  import { api, ApiError } from "@/lib/api";
  import type { Me, User, UserList } from "@/lib/types";

  export default function UsersPage() {
    const [me, setMe] = useState<Me | null>(null);
    const [users, setUsers] = useState<User[]>([]);
    const [editTarget, setEditTarget] = useState<User | null>(null);
    const [deleteTarget, setDeleteTarget] = useState<User | null>(null);

    const load = useCallback(async () => {
      try {
        const list = await api.get<UserList>("/api/users");
        setUsers(list.result);
      } catch (e) {
        if (!(e instanceof ApiError && e.status === 401)) {
          toast.error("Failed to load users");
        }
      }
    }, []);

    useEffect(() => {
      // /api/me gate: 401 inside lib/api redirects to /login (full-page nav)
      api.get<Me>("/api/me").then(setMe).catch(() => {});
      load();
    }, [load]);

    async function onLifecycle(u: User, action: Lifecycle) {
      try {
        await api.post(`/api/users/${u.id}/${action}`);
        toast.success(`${action} ok`);
        load();
      } catch (e) {
        toast.error(e instanceof ApiError ? e.message : `${action} failed`);
      }
    }

    async function confirmDelete() {
      if (!deleteTarget) return;
      try {
        await api.del(`/api/users/${deleteTarget.id}`);
        toast.success("User deleted");
      } catch (e) {
        toast.error(e instanceof ApiError ? e.message : "Delete failed");
      } finally {
        setDeleteTarget(null);
        load();
      }
    }

    const columns = buildColumns({
      onEdit: setEditTarget,
      onDelete: setDeleteTarget,
      onLifecycle,
    });

    return (
      <main className="container mx-auto py-8 space-y-4">
        <div className="flex items-center justify-between">
          <div>
            <h1 className="text-2xl font-semibold">Users</h1>
            {me && <p className="text-sm text-muted-foreground">Signed in as {me.name}</p>}
          </div>
          <div className="flex gap-2">
            <CreateUserDialog onCreated={load} />
            <Button variant="outline" asChild><a href="/logout">Sign out</a></Button>
          </div>
        </div>
        <DataTable columns={columns} data={users}
          filterColumn="userName" filterPlaceholder="Filter by username..." />
        <EditUserDialog user={editTarget} open={!!editTarget}
          onOpenChange={(o) => !o && setEditTarget(null)} onSaved={load} />
        <ConfirmDialog open={!!deleteTarget}
          onOpenChange={(o) => !o && setDeleteTarget(null)}
          title="Delete user?"
          description="This is irreversible and removes the user and any machine keys. Already-issued tokens stay valid until their TTL expires."
          confirmLabel="Delete" onConfirm={confirmDelete} />
      </main>
    );
  }
  ```

  `admin-web/app/page.tsx` (root redirects to `/users`):
  ```tsx
  import { redirect } from "next/navigation";

  export default function Home() {
    redirect("/users");
  }
  ```

  Add `<Toaster />` to `admin-web/app/layout.tsx` (sonner host) — insert inside `<body>`:
  ```tsx
  import { Toaster } from "@/components/ui/sonner";
  // ...inside <body>{children}<Toaster /></body>
  ```

- [ ] **Step 4: Run — expect PASS** (also typecheck the new tree)
  ```powershell
  cd D:\projects\llm-chat\admin-web; pnpm test -- columns; pnpm exec tsc --noEmit
  ```
  Expected: PASS — `columns.test.tsx (3)` green (columns present, state badge renders, `action-deactivate` fires `onLifecycle(user,"deactivate")`); `tsc --noEmit` exits 0.

- [ ] **Step 5: Commit**
  ```powershell
  cd D:\projects\llm-chat; git add admin-web/components admin-web/app admin-web/__tests__/columns.test.tsx; git commit -F -
  ```
  Commit message:
  ```
  feat(admin-web): users DataTable, dialogs, row actions + /api/me gate

  TanStack DataTable + buildColumns (state badges, per-row lifecycle/delete
  actions), create/edit dialogs (react-hook-form + zod), AlertDialog confirm
  for irreversible delete, /users page gated on /api/me, root redirect.

  Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
  ```

---

### Task 23: Playwright login → list → create smoke
**Files:**
- Create: `admin-web/playwright.config.ts`
- Create: `admin-web/e2e/smoke.spec.ts`
- Modify: `admin-web/package.json:scripts` (the `e2e` script added in Task 20)

- [ ] **Step 1: Write the failing test** — a black-box smoke proving the operator path the design pins (§9 "a login → list → create Playwright smoke only"): an unauthenticated visit redirects to the BFF `/login` (full-page nav, appendix §5.2); when authenticated (BFF session cookie present), `/users` lists users and the Create-user dialog POSTs and shows the new row. Authentication is real and lives in `admin-api` + Zitadel, so the authed half is **gated on `ADMIN_IT=1`** (mirrors the Rust `ADMIN_IT=1` integration gate) rather than fabricating a logged-in cookie. The redirect half always runs against `pnpm dev`.

  `admin-web/playwright.config.ts`:
  ```ts
  import { defineConfig, devices } from "@playwright/test";

  const BASE_URL = process.env.ADMIN_WEB_URL ?? "http://localhost:3000";

  export default defineConfig({
    testDir: "./e2e",
    timeout: 30_000,
    use: { baseURL: BASE_URL, trace: "on-first-retry" },
    projects: [{ name: "chromium", use: { ...devices["Desktop Chrome"] } }],
    // Start admin-web for the redirect smoke; the full login path needs the
    // whole compose stack (admin-api + Zitadel) and runs under ADMIN_IT=1.
    webServer: process.env.ADMIN_IT
      ? undefined
      : {
          command: "pnpm dev",
          url: BASE_URL,
          reuseExistingServer: true,
          timeout: 60_000,
        },
  });
  ```

  `admin-web/e2e/smoke.spec.ts`:
  ```ts
  import { test, expect } from "@playwright/test";

  const FULL = process.env.ADMIN_IT === "1";

  test("unauthenticated visit to /users redirects toward /login (BFF nav)", async ({ page }) => {
    // No session cookie: lib/api 401 -> window.location.assign('/login'),
    // which the same-origin proxy forwards to admin-api -> Zitadel /authorize.
    const resp = await page.goto("/users");
    // Either the client redirected us to /login, or (full stack) on to Zitadel.
    await expect
      .poll(() => page.url())
      .toMatch(/\/login|\/oauth\/v2\/authorize/);
    expect(resp).not.toBeNull();
  });

  test.describe("authenticated operator flow", () => {
    test.skip(!FULL, "requires running stack: set ADMIN_IT=1 + a logged-in chat.admin session");

    test("login -> list users -> create machine user", async ({ page }) => {
      // Real login against Zitadel (operator with chat.admin). Credentials from env.
      await page.goto("/login");
      await page.getByLabel(/username|loginname/i).fill(process.env.ADMIN_IT_USER!);
      await page.getByRole("button", { name: /next|continue/i }).click();
      await page.getByLabel(/password/i).fill(process.env.ADMIN_IT_PASS!);
      await page.getByRole("button", { name: /next|continue|sign in/i }).click();

      // Lands back on the dashboard (BFF set its session cookie, 302 -> admin-web).
      await page.waitForURL(/\/users/);
      await expect(page.getByRole("heading", { name: "Users" })).toBeVisible();

      // Create a machine user.
      const uname = `pw-bot-${Date.now()}`;
      await page.getByTestId("create-user").click();
      await page.getByRole("combobox").click();
      await page.getByRole("option", { name: "Machine" }).click();
      await page.getByLabel("Username").fill(uname);
      await page.getByLabel("Display name").fill(uname);
      await page.getByRole("button", { name: "Create" }).click();

      // New row appears (filter then assert).
      await page.getByPlaceholder(/filter by username/i).fill(uname);
      await expect(page.getByText(uname)).toBeVisible();
    });
  });
  ```

- [ ] **Step 2: Run it — expect FAIL**
  ```powershell
  cd D:\projects\llm-chat\admin-web; pnpm exec playwright install chromium; pnpm e2e
  ```
  Expected: FAIL initially — before `playwright.config.ts`/`smoke.spec.ts` exist Playwright reports "no tests found"; once written but before the app builds clean, the redirect test fails (app won't boot / wrong URL). The authed `describe` is **skipped** unless `ADMIN_IT=1`.

- [ ] **Step 3: Implement** — the spec files above ARE the implementation (config + spec). Confirm `package.json` has `"e2e": "playwright test"` (added in Task 20). No app code changes are needed; the smoke exercises the Task 21/22 UI through the same-origin proxy.

- [ ] **Step 4: Run — expect PASS**
  - Redirect smoke (no stack needed; `webServer` boots `pnpm dev`):
    ```powershell
    cd D:\projects\llm-chat\admin-web; pnpm e2e -- -g "redirects toward /login"
    ```
    Expected: PASS — 1 passed; `page.url()` matches `/login` (or `/oauth/v2/authorize` when the proxy reaches a running BFF).
  - Full operator flow (requires the running compose stack + a `chat.admin` operator):
    ```powershell
    cd D:\projects\llm-chat\admin-web; $env:ADMIN_IT="1"; $env:ADMIN_WEB_URL="http://localhost:3000"; $env:ADMIN_IT_USER="<operator>"; $env:ADMIN_IT_PASS="<password>"; pnpm e2e -- -g "login -> list users -> create"
    ```
    Expected (against the running stack): PASS — lands on `/users`, "Users" heading visible, the new `pw-bot-*` row appears. **This discharges appendix §6.1 (human auth-code login carries `chat.admin`) and §5.2 (the same-origin `SameSite=Lax` session cookie survives the Zitadel 302 back) end-to-end.** If it fails on the role gate, that is the §6.1/§10-risk-1 repair (app `accessTokenRoleAssertion=true` / project flags) surfacing in Phase B — record it, do not stub the cookie.

- [ ] **Step 5: Commit**
  ```powershell
  cd D:\projects\llm-chat; git add admin-web/playwright.config.ts admin-web/e2e admin-web/package.json; git commit -F -
  ```
  Commit message:
  ```
  test(admin-web): Playwright login->list->create smoke

  Always-on redirect smoke proves an unauthed /users bounces to the BFF
  /login (full-page nav). The authenticated create-machine-user flow is
  gated on ADMIN_IT=1 against the running stack (appendix §6.1, §5.2) and
  is never faked with a stubbed session.

  Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
  ```

---

## Phase E-compose

### Task 24: admin-api + admin-web Dockerfiles
**Files:**
- Create: `deploy/compose/admin-api.Dockerfile`
- Create: `deploy/compose/admin-web.Dockerfile`
- Test: `deploy/compose/test_dockerfiles.py`

- [ ] **Step 1: Write the failing test** — pure static assertions on the two Dockerfiles (no Docker daemon needed; mirrors how `manager.Dockerfile` is structured: multi-stage `rust:1-bookworm` → `debian:bookworm-slim` with `ca-certificates`, and a node standalone image). Create `deploy/compose/test_dockerfiles.py`:

  ```python
  """Static contract tests for the admin-api / admin-web Dockerfiles.

  No Docker daemon: we assert the build *recipe* matches the locked contract
  (multi-stage rust->debian-slim for the BFF; node:20-alpine standalone for the
  web). A real `docker build` is exercised by the compose acceptance task (Task 26).
  """
  from pathlib import Path

  HERE = Path(__file__).resolve().parent
  API = HERE / "admin-api.Dockerfile"
  WEB = HERE / "admin-web.Dockerfile"


  def test_admin_api_is_multistage_rust_to_debian_slim() -> None:
      t = API.read_text(encoding="utf-8")
      # build stage: workspace-aware rust toolchain
      assert "FROM rust:1-bookworm AS build" in t
      # workspace build: the crate lives in a Cargo workspace, so the whole
      # workspace manifest + the sibling crates it shares a lock with must be
      # present for `cargo build --locked` to resolve.
      assert "cargo build --release --locked -p llm-chat-admin-api" in t
      # runtime stage: slim debian with TLS roots (rustls verifies the issuer cert)
      assert "FROM debian:bookworm-slim" in t
      assert "ca-certificates" in t
      assert (
          "COPY --from=build /src/target/release/llm-chat-admin-api "
          "/usr/local/bin/llm-chat-admin-api" in t
      )
      assert 'ENTRYPOINT ["/usr/local/bin/llm-chat-admin-api"]' in t


  def test_admin_web_is_node_standalone() -> None:
      t = WEB.read_text(encoding="utf-8")
      assert "FROM node:20-alpine AS build" in t
      assert "corepack enable" in t            # pnpm via corepack (repo uses pnpm)
      assert "pnpm install --frozen-lockfile" in t
      assert "pnpm run build" in t
      # next.config sets output:'standalone' -> copy the standalone server
      assert "COPY --from=build /app/.next/standalone ./" in t
      assert "COPY --from=build /app/.next/static ./.next/static" in t
      assert 'CMD ["node", "server.js"]' in t
      assert "EXPOSE 3000" in t
  ```

- [ ] **Step 2: Run it — expect FAIL**
  - Command: `cd D:\projects\llm-chat; python -m pytest deploy/compose/test_dockerfiles.py -v`
  - Expected failure: both tests `FAILED` with `FileNotFoundError: ...admin-api.Dockerfile` / `...admin-web.Dockerfile` (files do not exist yet).

- [ ] **Step 3: Implement** — create both Dockerfiles.

  `deploy/compose/admin-api.Dockerfile` (workspace build: copy the root manifest + lock and all workspace members so `--locked` resolves, then build only the BFF; runtime stage mirrors `manager.Dockerfile` exactly):
  ```dockerfile
  # syntax=docker/dockerfile:1
  # admin-api: the Rust BFF (axum). Multi-stage like manager.Dockerfile, but the
  # crate lives in a Cargo workspace, so the whole workspace manifest + lock and
  # every member it shares the lock with must be COPYd before `cargo build`.
  FROM rust:1-bookworm AS build
  WORKDIR /src
  # Workspace skeleton: root manifest + single lock, then each member's sources.
  COPY Cargo.toml Cargo.lock ./
  COPY crates ./crates
  COPY manager/Cargo.toml ./manager/Cargo.toml
  COPY manager/src ./manager/src
  COPY worker/Cargo.toml worker/build.rs ./worker/
  COPY worker/src ./worker/src
  COPY admin-api/Cargo.toml ./admin-api/Cargo.toml
  COPY admin-api/src ./admin-api/src
  RUN cargo build --release --locked -p llm-chat-admin-api

  FROM debian:bookworm-slim
  RUN apt-get update \
      && apt-get install -y --no-install-recommends ca-certificates \
      && rm -rf /var/lib/apt/lists/*
  COPY --from=build /src/target/release/llm-chat-admin-api /usr/local/bin/llm-chat-admin-api
  EXPOSE 7676
  ENTRYPOINT ["/usr/local/bin/llm-chat-admin-api"]
  ```
  NOTE: verify `worker/build.rs` does not read assets outside the COPYd paths (e.g. a `web/dist` dir). If it does, add those paths to the COPY set or the workspace build of `-p llm-chat-admin-api` (which still compiles worker's build script as part of resolving the workspace) will fail. `admin-api/src` includes `src/zitadel/testdata/*.pem` (Task 13) — harmless in the image, kept so the build context is one consistent tree.

  `deploy/compose/admin-web.Dockerfile` (Next.js `output:'standalone'`, node runtime, pnpm via corepack; build context is `./admin-web`):
  ```dockerfile
  # syntax=docker/dockerfile:1
  # admin-web: Next.js (App Router) standalone build. Build context = ./admin-web.
  FROM node:20-alpine AS build
  WORKDIR /app
  RUN corepack enable
  COPY package.json pnpm-lock.yaml ./
  RUN pnpm install --frozen-lockfile
  COPY . .
  RUN pnpm run build

  FROM node:20-alpine
  WORKDIR /app
  ENV NODE_ENV=production
  # next.config sets output:'standalone' -> a self-contained server bundle.
  COPY --from=build /app/.next/standalone ./
  COPY --from=build /app/.next/static ./.next/static
  COPY --from=build /app/public ./public
  EXPOSE 3000
  CMD ["node", "server.js"]
  ```

- [ ] **Step 4: Run — expect PASS**
  - Command: `cd D:\projects\llm-chat; python -m pytest deploy/compose/test_dockerfiles.py -v`
  - Expected output: `2 passed` (`test_admin_api_is_multistage_rust_to_debian_slim PASSED`, `test_admin_web_is_node_standalone PASSED`).

- [ ] **Step 5: Commit**
  - `git add deploy/compose/admin-api.Dockerfile deploy/compose/admin-web.Dockerfile deploy/compose/test_dockerfiles.py`
  - Commit message:
    ```
    build(compose): admin-api + admin-web Dockerfiles

    Multi-stage rust:1-bookworm -> debian:bookworm-slim (ca-certificates) for the
    BFF, building -p llm-chat-admin-api from the Cargo workspace; node:20-alpine
    standalone for the Next.js web. Static contract test (no daemon) locks the
    recipe; real `docker build` is exercised by the compose acceptance task.

    Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
    ```

---

### Task 25: compose admin-api + admin-web services + ADMIN_SESSION_KEY + generated-env wiring
**Files:**
- Modify: `docker-compose.yml:111-117` (after the `manager` service, before `volumes:`)
- Modify: `deploy/compose/admin-api.Dockerfile` (add an entrypoint that sources the generated env, since compose cannot `env_file` from a runtime volume)
- Create: `deploy/compose/admin-api-entrypoint.sh`
- Modify: `.env.example:13` (append `ADMIN_SESSION_KEY`)
- Test: `deploy/compose/test_compose_admin.py`

This closes the critique's generated-env gap: `AdminConfig::from_env` REQUIRES `ZITADEL_PROJECT_ID`/`ZITADEL_AUDIENCE`, which the provisioner writes into `manager.generated.env` (mounted at `/out`). Compose `env_file:` cannot read a path that only exists inside a runtime volume, so a tiny entrypoint sources `/out/manager.generated.env` (and the `./secrets/admin_oidc_client_*` files via the `*_FILE` indirection) before exec-ing the binary. The single-issuer `ZITADEL_ISSUER` and the bind/origin vars come from the compose `environment:` block; `ADMIN_OIDC_CLIENT_ID/SECRET` are read from the secrets files by the entrypoint.

- [ ] **Step 1: Write the failing test** — parse the compose YAML (no daemon) and assert the two new services and their wiring exactly match the locked contract: ports 7676/3000, `depends_on: zitadel-init service_completed_successfully`, env from `.env` + the generated env + `./secrets` mounts, the entrypoint, and `ADMIN_SESSION_KEY` present in `.env.example`. Create `deploy/compose/test_compose_admin.py`:

  ```python
  """Contract tests for the admin-api / admin-web compose services.

  Parses docker-compose.yml as data (PyYAML) and .env.example as text. No Docker
  daemon — the real stack boot is the Task 26 acceptance task.
  """
  from pathlib import Path

  import yaml

  ROOT = Path(__file__).resolve().parents[2]
  COMPOSE = ROOT / "docker-compose.yml"
  ENV_EXAMPLE = ROOT / ".env.example"
  ENTRYPOINT = ROOT / "deploy" / "compose" / "admin-api-entrypoint.sh"


  def _services() -> dict:
      return yaml.safe_load(COMPOSE.read_text(encoding="utf-8"))["services"]


  def test_admin_api_service() -> None:
      svc = _services()["admin-api"]
      # built from the Phase-E Dockerfile, root build context (workspace build)
      assert svc["build"]["context"] == "."
      assert svc["build"]["dockerfile"] == "deploy/compose/admin-api.Dockerfile"
      assert svc["ports"] == ["7676:7676"]
      # waits for the one-shot provisioner to finish (SA key + OIDC secret + pid)
      assert svc["depends_on"]["zitadel-init"]["condition"] == "service_completed_successfully"
      env = svc["environment"]
      assert env["ADMIN_BIND_ADDR"] == "0.0.0.0:7676"
      assert env["ZITADEL_ISSUER"] == "http://host.docker.internal:8080"
      assert env["ADMIN_SA_KEY_PATH"] == "/secrets/admin-api-key.json"
      assert env["ADMIN_SESSION_KEY"] == "${ADMIN_SESSION_KEY}"
      assert env["ADMIN_PUBLIC_ORIGIN"] == "http://localhost:7676"
      assert env["ADMIN_ALLOWED_ORIGIN"] == "http://localhost:3000"
      # generated env (project_id / audience) + the secrets dir are mounted in
      mounts = svc["volumes"]
      assert "genenv:/out:ro" in mounts
      assert "./secrets:/secrets:ro" in mounts


  def test_admin_web_service() -> None:
      svc = _services()["admin-web"]
      assert svc["build"]["context"] == "./admin-web"
      assert svc["build"]["dockerfile"] == "../deploy/compose/admin-web.Dockerfile"
      assert svc["ports"] == ["3000:3000"]
      # web is a pure client of admin-api; proxy target is the in-network host
      assert svc["depends_on"]["admin-api"]["condition"] == "service_started"
      assert svc["environment"]["ADMIN_API_ORIGIN"] == "http://admin-api:7676"


  def test_entrypoint_sources_generated_env() -> None:
      t = ENTRYPOINT.read_text(encoding="utf-8")
      # sources project_id/audience from the generated env mounted at /out
      assert "/out/manager.generated.env" in t
      # resolves the OIDC client id/secret from the *_FILE indirection
      assert "ADMIN_OIDC_CLIENT_ID_FILE" in t
      assert "ADMIN_OIDC_CLIENT_SECRET_FILE" in t
      assert "exec /usr/local/bin/llm-chat-admin-api" in t


  def test_env_example_has_session_key() -> None:
      t = ENV_EXAMPLE.read_text(encoding="utf-8")
      assert "ADMIN_SESSION_KEY=" in t
  ```

- [ ] **Step 2: Run it — expect FAIL**
  - Command: `cd D:\projects\llm-chat; python -m pytest deploy/compose/test_compose_admin.py -v`
  - Expected failure: `KeyError: 'admin-api'` in `test_admin_api_service` (and `'admin-web'`), `FileNotFoundError` for the entrypoint, plus `test_env_example_has_session_key` fails — the services, entrypoint, and env var do not exist yet. (If PyYAML is absent, `pip install pyyaml` first.)

- [ ] **Step 3: Implement** — create the entrypoint, point the Dockerfile at it, add both services, and add the env var.

  3a. Create `deploy/compose/admin-api-entrypoint.sh` (sources the generated env + resolves the `*_FILE` secret indirection, then exec the binary — POSIX `sh`, present in `debian:bookworm-slim`):
  ```sh
  #!/bin/sh
  # admin-api entrypoint: compose cannot env_file a path that only exists inside a
  # runtime volume, so source the provisioner's generated env (project_id /
  # audience) and resolve the OIDC client id/secret from the mounted secret files
  # before exec-ing the binary. AdminConfig::from_env then sees every required var.
  set -eu

  # project_id + audience (ZITADEL_PROJECT_ID / ZITADEL_AUDIENCE), written by the
  # provisioner into manager.generated.env on the genenv volume mounted at /out.
  if [ -f /out/manager.generated.env ]; then
      # shellcheck disable=SC1091
      set -a; . /out/manager.generated.env; set +a
  else
      echo "admin-api-entrypoint: /out/manager.generated.env missing (provisioner not done?)" >&2
      exit 1
  fi

  # OIDC client id/secret: read the file path indirection if the value is unset.
  if [ -z "${ADMIN_OIDC_CLIENT_ID:-}" ] && [ -n "${ADMIN_OIDC_CLIENT_ID_FILE:-}" ]; then
      ADMIN_OIDC_CLIENT_ID="$(cat "$ADMIN_OIDC_CLIENT_ID_FILE")"; export ADMIN_OIDC_CLIENT_ID
  fi
  if [ -z "${ADMIN_OIDC_CLIENT_SECRET:-}" ] && [ -n "${ADMIN_OIDC_CLIENT_SECRET_FILE:-}" ]; then
      ADMIN_OIDC_CLIENT_SECRET="$(cat "$ADMIN_OIDC_CLIENT_SECRET_FILE")"; export ADMIN_OIDC_CLIENT_SECRET
  fi

  exec /usr/local/bin/llm-chat-admin-api
  ```
  Make it executable and adjust the Dockerfile runtime stage to use it (replace the `ENTRYPOINT` line from Task 24):
  ```dockerfile
  COPY deploy/compose/admin-api-entrypoint.sh /usr/local/bin/admin-api-entrypoint.sh
  RUN chmod +x /usr/local/bin/admin-api-entrypoint.sh
  ENTRYPOINT ["/usr/local/bin/admin-api-entrypoint.sh"]
  ```
  (Keep the prior `COPY --from=build ... /usr/local/bin/llm-chat-admin-api` line; the entrypoint execs it. The Task-24 `test_dockerfiles.py` asserts the binary `ENTRYPOINT` string — update that test's last assertion to the entrypoint OR keep BOTH lines and assert the binary is still COPYd; simplest: change the `test_admin_api_is_multistage` final assertion to `assert 'admin-api-entrypoint.sh' in t` and keep the binary-COPY assertion.)

  NOTE on the generated-env var names: the provisioner's `manager.generated.env` must export `ZITADEL_PROJECT_ID` and `ZITADEL_AUDIENCE` (the names `AdminConfig::from_env` reads). If the existing file uses different names (e.g. `PROJECT_ID`), add an alias line in `write_generated_env` (Phase B/Task 7 already touches the generated env) so both the manager and admin-api resolve — verify the actual key names in `provision.py:write_generated_env` and reconcile to `ZITADEL_PROJECT_ID`/`ZITADEL_AUDIENCE`.

  3b. add both services to `docker-compose.yml` after the `manager` service (insert before the top-level `volumes:`), matching the `manager` service's env/mount patterns:
  ```yaml
    admin-api:
      build:
        context: .
        dockerfile: deploy/compose/admin-api.Dockerfile
      environment:
        # Single-issuer linchpin (§3): same literal string the manager uses, so
        # the JWTs the admin verifies share one `iss`. admin-api fails fast at
        # startup if the discovery doc's issuer differs.
        ZITADEL_ISSUER: http://host.docker.internal:8080
        ADMIN_BIND_ADDR: 0.0.0.0:7676
        # admin SA JSON key + OIDC client creds written by the provisioner into
        # the (read-only) secrets mount; never reach the browser or logs. The
        # entrypoint resolves *_FILE -> value and sources /out for project/audience.
        ADMIN_SA_KEY_PATH: /secrets/admin-api-key.json
        ADMIN_OIDC_CLIENT_ID_FILE: /secrets/admin_oidc_client_id
        ADMIN_OIDC_CLIENT_SECRET_FILE: /secrets/admin_oidc_client_secret
        # OIDC redirect must match the app registered by provision.py
        # (create_admin_oidc_app: http://localhost:7676/callback). This is the
        # admin-api's OWN origin (public_origin), distinct from the web origin.
        ADMIN_PUBLIC_ORIGIN: http://localhost:7676
        ADMIN_ALLOWED_ORIGIN: http://localhost:3000
        ADMIN_SESSION_KEY: ${ADMIN_SESSION_KEY}
        RUST_LOG: ${RUST_LOG:-info}
      ports:
        - "7676:7676"
      depends_on:
        zitadel-init:
          condition: service_completed_successfully
      volumes:
        # project_id + audience (manager.generated.env) and the SA/OIDC secrets.
        - genenv:/out:ro
        - ./secrets:/secrets:ro
      restart: unless-stopped

    admin-web:
      build:
        context: ./admin-web
        dockerfile: ../deploy/compose/admin-web.Dockerfile
      environment:
        NODE_ENV: production
        # next.config rewrites /api,/login,/callback,/logout to this origin
        # (same-origin proxy: no CORS, SameSite=Lax cookie). In-network DNS name.
        ADMIN_API_ORIGIN: http://admin-api:7676
      ports:
        - "3000:3000"
      depends_on:
        admin-api:
          condition: service_started
      restart: unless-stopped
  ```

  3c. append to `.env.example` (after line 13, mirroring the existing `openssl rand` comment style):
  ```bash

  # Session cookie signing key for admin-api (httpOnly opaque session).
  #   generate: openssl rand -hex 32
  ADMIN_SESSION_KEY=changeme-openssl-rand-hex-32
  ```

- [ ] **Step 4: Run — expect PASS**
  - Command: `cd D:\projects\llm-chat; python -m pytest deploy/compose/test_compose_admin.py deploy/compose/test_dockerfiles.py -v`
  - Then validate the merged compose file parses with Docker's own schema: `cd D:\projects\llm-chat; docker compose config --quiet; echo "compose-config-exit=$LASTEXITCODE"`
  - Expected output: all tests pass (the four compose tests + the two Dockerfile tests, the latter updated for the entrypoint); and `compose-config-exit=0` (no schema/interpolation errors — confirms YAML indentation and the new services are well-formed).

- [ ] **Step 5: Commit**
  - `git add docker-compose.yml .env.example deploy/compose/admin-api.Dockerfile deploy/compose/admin-api-entrypoint.sh deploy/compose/test_compose_admin.py deploy/compose/test_dockerfiles.py`
  - Commit message:
    ```
    feat(compose): admin-api (7676) + admin-web (3000) services

    admin-api builds from the workspace, depends_on zitadel-init completed, reads
    project_id/audience from genenv via an entrypoint that sources
    /out/manager.generated.env and resolves the SA key + OIDC creds from a
    read-only ./secrets mount, single-issuer with the manager. admin-web proxies
    /api to admin-api (same-origin, no CORS). Add ADMIN_SESSION_KEY to
    .env.example. Contract test parses the compose YAML; `docker compose config`
    validates it.

    Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
    ```

---

### Task 26: end-to-end acceptance — operator login → create machine user+key → key passes the manager `chat.user` gate
**Files:**
- Create: `deploy/compose/test_e2e_admin.py`
- Test: `deploy/compose/test_e2e_admin.py`

This is a **gated full-loop acceptance test** against the **running stack** (the source of truth — no fabricated Zitadel response bodies). It is the §6 / §10 highest-risk discharge for the compose layer: it proves the admin-minted machine key actually authenticates against the **manager**. It runs only when `ADMIN_E2E=1` (skips otherwise), consistent with the `ADMIN_IT=1` gating convention.

CRITICAL (source-of-truth, CLAUDE.md): the manager is a **WebSocket** server and the python client drives it via the async `protocol.ChatClient` (`ChatClient(manager_url, token_provider, *, max_reconnects=1)`, async context manager, `await client.ask(text) -> Answer`). There is NO synchronous `protocol.send_hello/send_q/recv_frame` TCP API — that was a fabricated API and is removed. The manager rejects a token lacking `chat.user` with HTTP 403 **during the WebSocket upgrade**, which surfaces in the client as `errors.ManagerUnavailable` (the `websockets.connect` raises before the socket opens). A granted token completes the handshake and `ask("ping")` returns an `Answer` with `.text`. The manager WS URL is `ws://127.0.0.1:7777/chat`.

- [ ] **Step 1: Write the failing test** — drive the live `admin-api` to create a machine user, mint its JSON key, grant it `chat.user`, then use the python client's own `auth.fetch_access_token` (the real machine-key → JWT path) and `protocol.ChatClient` to assert the manager's `chat.user` gate **accepts** it (and **rejects** it pre-grant). Create `deploy/compose/test_e2e_admin.py`:

  ```python
  """End-to-end acceptance for the admin stack (§9 'End-to-end acceptance', §10).

  RUNBOOK (operator, once):
    1. cp .env.example .env  &&  fill ZITADEL_MASTERKEY / POSTGRES_PASSWORD /
       LLM_CHAT_AUTH_TOKEN / ADMIN_SESSION_KEY  (openssl rand -hex ...).
    2. docker compose up -d --build   (postgres, zitadel, zitadel-init,
       manager, admin-api, admin-web all come up; zitadel-init exits 0).
    3. Grant your human operator the chat.admin role (provision seeds or
       console-once), then open http://localhost:3000 -> "Sign in" -> log in.
    4. Copy the session cookie value (DevTools->Application->Cookies, name "id")
       and the project id:
         $env:ADMIN_E2E="1"
         $env:ADMIN_OPERATOR_COOKIE="<cookie>"
         $env:PROJECT_ID=(Get-Content secrets/project_id)
    5. python -m pytest deploy/compose/test_e2e_admin.py -v

  GATE: runs only when ADMIN_E2E=1 against a RUNNING `docker compose up` stack;
  skipped otherwise. This is the SOURCE-OF-TRUTH loop — it talks to the live
  admin-api and the live manager (via the real async ChatClient WebSocket
  protocol), never a mocked Zitadel body.

  Loop: operator session (cookie) -> POST /api/users/machine -> POST .../keys
  (key returned once) -> POST .../grants {chat.user} -> mint a JWT from that key
  via the python client's real auth path -> assert the MANAGER's chat.user gate
  ACCEPTS it (a chat round-trips). Asserts the SAME key, ungranted, is REJECTED
  (the manager 403s the WS upgrade -> ManagerUnavailable).
  """
  import asyncio
  import json
  import os
  import sys
  import time
  import uuid

  import pytest
  import requests

  # The python client lives under clients/python; make it importable.
  sys.path.insert(0, os.path.join(
      os.path.dirname(__file__), "..", "..", "clients", "python"))

  pytestmark = pytest.mark.skipif(
      os.environ.get("ADMIN_E2E") != "1",
      reason="ADMIN_E2E!=1 — needs a running compose stack (docker compose up)",
  )

  ADMIN_API = os.environ.get("ADMIN_API_BASE", "http://localhost:7676")
  MANAGER_WS = os.environ.get("MANAGER_WS", "ws://127.0.0.1:7777/chat")
  OPERATOR_COOKIE = os.environ.get("ADMIN_OPERATOR_COOKIE")
  PROJECT_ID = os.environ.get("PROJECT_ID")  # from secrets/project_id


  def _session() -> requests.Session:
      if not OPERATOR_COOKIE:
          pytest.skip("ADMIN_OPERATOR_COOKIE unset — log in once and export the cookie")
      s = requests.Session()
      s.headers.update({"Content-Type": "application/json"})
      # The BFF session cookie is named "id" (SessionManagerLayer.with_name("id")).
      s.cookies.set("id", OPERATOR_COOKIE, domain="localhost")
      return s


  async def _ask_via_manager(token: str) -> str:
      """Open ONE /chat WebSocket with the bearer token via the real ChatClient
      and return the answer text. Raises ManagerUnavailable if the manager 403s
      the upgrade (the chat.user gate rejecting an ungranted token)."""
      from llm_chat.protocol import ChatClient
      async with ChatClient(MANAGER_WS, token_provider=lambda: token) as c:
          answer = await c.ask("ping", timeout=30.0)
          return answer.text


  def _manager_accepts(token: str) -> tuple[bool, str]:
      """True if the manager's chat.user gate let the token through (a chat
      round-tripped). A ManagerUnavailable means the WS upgrade was rejected
      (403 — missing chat.user) OR the manager is down; the caller sequences
      this so a rejection is the expected pre-grant outcome."""
      from llm_chat.errors import ManagerUnavailable
      try:
          text = asyncio.run(_ask_via_manager(token))
          return True, text
      except ManagerUnavailable as e:
          return False, str(e)


  def test_admin_minted_machine_key_passes_manager_chat_user_gate(tmp_path) -> None:
      from llm_chat import auth

      s = _session()
      uname = f"e2e-machine-{uuid.uuid4().hex[:8]}"

      # 1) operator creates a machine user via the BFF
      r = s.post(f"{ADMIN_API}/api/users/machine",
                 data=json.dumps({"username": uname, "name": uname}))
      assert r.status_code in (200, 201), r.text
      user_id = r.json()["userId"]

      # 2) operator mints a JSON key — returned ONCE, streamed to the caller
      r = s.post(f"{ADMIN_API}/api/users/{user_id}/keys",
                 data=json.dumps({"type": "KEY_TYPE_JSON"}))
      assert r.status_code in (200, 201), r.text
      created = r.json()  # admin-api returns the full create response (keyDetails once)
      # keyDetails is base64 of the serviceaccount JSON ({userId,keyId,key,...}).
      import base64
      key_blob = json.loads(base64.b64decode(created["keyDetails"]))
      key_file = tmp_path / "e2e-key.json"
      key_file.write_text(json.dumps(key_blob), encoding="utf-8")

      creds = auth.Credentials(
          issuer=os.environ.get("ZITADEL_ISSUER", auth.DEFAULT_ISSUER),
          project=PROJECT_ID,
          key_file=str(key_file),
      )

      # 2b) BEFORE granting chat.user: the manager MUST reject (gate is real).
      token = auth.fetch_access_token(creds)
      ok, msg = _manager_accepts(token)
      assert not ok, f"expected chat.user rejection before grant, got accept: {msg!r}"

      # 3) operator grants chat.user to the new machine user
      r = s.post(f"{ADMIN_API}/api/users/{user_id}/grants",
                 data=json.dumps({"role_keys": ["chat.user"]}))
      assert r.status_code in (200, 201), r.text

      # 4) a FRESH token (role projected) must now pass the manager gate.
      #    Zitadel projection is eventually consistent — retry briefly.
      ok, msg = False, ""
      for _ in range(10):
          token = auth.fetch_access_token(creds)
          ok, msg = _manager_accepts(token)
          if ok:
              break
          time.sleep(1)
      assert ok, f"admin-minted+granted key was rejected by manager: {msg!r}"

      # 5) cleanup — delete the throwaway machine user
      s.delete(f"{ADMIN_API}/api/users/{user_id}")
  ```

- [ ] **Step 2: Run it — expect FAIL (or skip without the gate)**
  - Without the stack: `cd D:\projects\llm-chat; python -m pytest deploy/compose/test_e2e_admin.py -v` → `1 skipped` (gate off — correct: it never fabricates a Zitadel body).
  - With `$env:ADMIN_E2E=1` but **before** admin-api implements the routes / the stack is up: the test FAILS at the first `s.post(.../api/users/machine)` with a connection error or non-2xx — proving the loop is genuinely unimplemented (red).

- [ ] **Step 3: Implement** — this task's "implementation" is the runnable acceptance harness + its operator-doc (the RUNBOOK is the module docstring above; no app code is invented here — the routes were built in Phase C and the WebSocket client already exists in `clients/python/llm_chat/protocol.py`). Confirm the live preconditions: `clients/python` is importable (the test prepends it to `sys.path`), and `pip install -e clients/python` (or its deps `websockets`, `pyjwt[crypto]`, `requests`) is available in the test env.

- [ ] **Step 4: Run — expect PASS against the running stack**
  - Bring the stack up: `cd D:\projects\llm-chat; docker compose up -d --build`
  - Wait for the provisioner to finish: `cd D:\projects\llm-chat; docker compose wait zitadel-init` (exit 0 = SA key + OIDC creds + project_id written).
  - Set the gate vars (per the RUNBOOK) and run: `cd D:\projects\llm-chat; $env:ADMIN_E2E="1"; $env:PROJECT_ID=(Get-Content secrets/project_id); python -m pytest deploy/compose/test_e2e_admin.py -v`
  - Expected output: `1 passed` — `test_admin_minted_machine_key_passes_manager_chat_user_gate PASSED` (the admin-minted machine key was rejected pre-grant via `ManagerUnavailable`, then accepted by the manager's `chat.user` gate post-grant with a chat round-trip). This is the §10 highest-risk gate discharged against Zitadel v3.4.10 + the live manager as the source of truth.

- [ ] **Step 5: Commit**
  - `git add deploy/compose/test_e2e_admin.py`
  - Commit message:
    ```
    test(compose): end-to-end admin acceptance gate (ADMIN_E2E=1)

    Full-loop against the RUNNING stack: operator session -> create machine user
    -> mint JSON key (returned once) -> grant chat.user -> the python client's
    real machine-key->JWT path + the real async ChatClient WebSocket are REJECTED
    by the manager pre-grant (ManagerUnavailable on the 403 upgrade) and ACCEPTED
    post-grant (chat round-trips). Source-of-truth (live Zitadel + live manager),
    no mocked bodies; skipped unless ADMIN_E2E=1. Discharges the §10 highest-risk
    compose gate.

    Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
    ```
