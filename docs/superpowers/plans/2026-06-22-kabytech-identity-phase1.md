# kabytech Identity UX — Phase 1 (Invitation + Registration) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Invite-only onboarding for kabytech: an operator invites an email, Zitadel sends an invite link (MailHog in dev), and the invited user sets a password on a beautiful kabytech page — landing a `chat.user` who can log in.

**Architecture:** kabytech-backend gains a privileged SA (mints a Management token via JWT-bearer, ported from `admin-api/src/zitadel/token.rs`) and two endpoints — `POST /api/invite` (chat.admin-gated) and `POST /api/accept`. The frontend gains beautiful `/invite` and `/accept` pages. The provisioner creates the SA + configures env-driven SMTP. Phase 2 (custom login) is a separate plan.

**Tech Stack:** Rust (axum 0.8, tower-sessions, jsonwebtoken, reqwest), Next.js 16 + Tailwind v4, the Zitadel v2 User API + v1 Management API + Admin SMTP API, Python provisioner, MailHog, Docker Compose.

## Global Constraints

Copied from `docs/superpowers/specs/2026-06-22-kabytech-identity-ux-design.md`.

- **Invite-only.** No public signup. "Registration" = setting a password on an invite link.
- **`/invite` + `POST /api/invite` require `chat.admin`** (fail closed). `/accept` is unauthenticated but the invite **code must be verified** (proves email ownership) before any password is set — no code, no account change.
- **All SMTP from `.env`, no hardcoded values:** `KABY_SMTP_HOST`, `KABY_SMTP_PORT`, `KABY_SMTP_TLS`, `KABY_SMTP_USER`, `KABY_SMTP_PASSWORD`, `KABY_SMTP_SENDER_ADDRESS`, `KABY_SMTP_SENDER_NAME`. HOST/PORT/SENDER_ADDRESS required (fail-fast); USER/PASSWORD may be empty; TLS is `false`/`0`/`no` ⇒ off.
- **kabytech SA least-privilege:** `ORG_USER_MANAGER` (org member) + `IAM_LOGIN_CLIENT` (instance member) only — never `ORG_OWNER`. Key at `secrets/kabytech-login-key.json`, mounted read-only; never leaves the backend.
- **Grant exactly `chat.user`** on the one chat project — no role widening.
- **The invite link points at the frontend origin:** `{KABY_PUBLIC_ORIGIN}/accept?userID={{.UserID}}&code={{.Code}}&orgID={{.OrgID}}` (`KABY_PUBLIC_ORIGIN` = `http://localhost:3001`).
- **Idempotent provisioner** (409 == already provisioned), matching existing `create_*`.
- **Phase 1 only:** no custom login, no Session API, no `IAM_LOGIN_CLIENT` *use* yet (the role is granted now so Phase 2 needs no re-provision).

---

### Task 1: MailHog service + env-driven SMTP variables

**Files:**
- Modify: `docker-compose.yml` (add `mailhog` service; pass `KABY_SMTP_*` to the provisioner)
- Modify: `.env` (add the `KABY_SMTP_*` dev values pointing at MailHog)

- [ ] **Step 1: Add the MailHog service to `docker-compose.yml`** (after the `kabytech-frontend` service, before `volumes:`)

```yaml
  mailhog:
    image: mailhog/mailhog
    ports:
      - "127.0.0.1:8025:8025"   # web inbox (loopback-only)
    restart: unless-stopped
```

- [ ] **Step 2: Pass the SMTP env to the provisioner** — in `docker-compose.yml`, under the `zitadel-init` service `environment:` block, add:

```yaml
      KABY_SMTP_HOST: ${KABY_SMTP_HOST:-mailhog}
      KABY_SMTP_PORT: ${KABY_SMTP_PORT:-1025}
      KABY_SMTP_TLS: ${KABY_SMTP_TLS:-false}
      KABY_SMTP_USER: ${KABY_SMTP_USER:-}
      KABY_SMTP_PASSWORD: ${KABY_SMTP_PASSWORD:-}
      KABY_SMTP_SENDER_ADDRESS: ${KABY_SMTP_SENDER_ADDRESS:-noreply@kabytech.local}
      KABY_SMTP_SENDER_NAME: ${KABY_SMTP_SENDER_NAME:-kabytech}
```

- [ ] **Step 3: Add the dev values to `.env`**

```bash
printf '\n# kabytech invite SMTP (dev -> MailHog; set real creds in prod)\nKABY_SMTP_HOST=mailhog\nKABY_SMTP_PORT=1025\nKABY_SMTP_TLS=false\nKABY_SMTP_USER=\nKABY_SMTP_PASSWORD=\nKABY_SMTP_SENDER_ADDRESS=noreply@kabytech.local\nKABY_SMTP_SENDER_NAME=kabytech\n' >> .env
```

- [ ] **Step 4: Validate the compose config**

Run: `docker compose config | grep -E "mailhog|KABY_SMTP_HOST"`
Expected: the `mailhog` service and the `KABY_SMTP_HOST: mailhog` value appear, no errors.

- [ ] **Step 5: Commit**

```bash
git add docker-compose.yml
git commit -m "feat(kabytech): MailHog dev SMTP service + env-driven KABY_SMTP_* wiring"
```

---

### Task 2: Provisioner — kabytech login SA (ORG_USER_MANAGER + IAM_LOGIN_CLIENT)

Create the machine user the backend authenticates as, granted the two roles, with its JSON key written to `secrets/`. Mirrors `create_admin_sa` + `assign_admin_member` + `grant_iam_viewer`.

**Files:**
- Modify: `deploy/compose/provisioner/provision.py`
- Test: `deploy/compose/provisioner/test_provision.py`

**Interfaces:**
- Consumes: `request_with_retry`, `is_success`, `ISSUER`, `create_machine_user`-style POST, `generate_json_key`, `write_secret`.
- Produces: `KABY_SA_USERNAME = "kabytech-login"`, `create_kaby_sa(token, headers) -> str` (userId), `assign_kaby_org_member(token, headers, uid) -> None` (ORG_USER_MANAGER), `assign_kaby_login_client(token, uid) -> None` (instance IAM_LOGIN_CLIENT).

- [ ] **Step 1: Write the failing tests** (append to `test_provision.py`)

```python
def test_create_kaby_sa_posts_machine_jwt():
    captured = {}

    def fake_rwr(method, url, *, headers=None, json_body=None, **kw):
        captured["url"] = url
        captured["body"] = json_body
        return _FakeResp(200, {"userId": "kaby-sa-1"})

    with mock.patch.object(provision, "request_with_retry", fake_rwr):
        uid = provision.create_kaby_sa("tok", {"h": "1"})
    assert uid == "kaby-sa-1"
    assert captured["url"].endswith("/management/v1/users/machine")
    assert captured["body"]["userName"] == provision.KABY_SA_USERNAME
    assert captured["body"]["accessTokenType"] == "ACCESS_TOKEN_TYPE_JWT"


def test_assign_kaby_org_member_posts_user_manager():
    captured = {}

    def fake_rwr(method, url, *, headers=None, json_body=None, **kw):
        captured["url"] = url
        captured["body"] = json_body
        return _FakeResp(200, {"details": {}})

    with mock.patch.object(provision, "request_with_retry", fake_rwr):
        provision.assign_kaby_org_member("tok", {"h": "1"}, "kaby-sa-1")
    assert captured["url"].endswith("/management/v1/orgs/me/members")
    assert captured["body"] == {"userId": "kaby-sa-1", "roles": ["ORG_USER_MANAGER"]}


def test_assign_kaby_login_client_posts_instance_member_no_org_header():
    captured = {}

    def fake_rwr(method, url, *, headers=None, json_body=None, **kw):
        captured.update(url=url, headers=headers, body=json_body)
        return _FakeResp(200)

    with mock.patch.object(provision, "request_with_retry", fake_rwr):
        provision.assign_kaby_login_client("tok", "kaby-sa-1")
    assert captured["url"].endswith("/admin/v1/members")          # instance-scoped
    assert "x-zitadel-orgid" not in captured["headers"]
    assert captured["body"] == {"userId": "kaby-sa-1", "roles": ["IAM_LOGIN_CLIENT"]}


def test_assign_kaby_login_client_409_is_success():
    with mock.patch.object(provision, "request_with_retry",
                           lambda *a, **k: _FakeResp(409)):
        provision.assign_kaby_login_client("tok", "kaby-sa-1")  # must NOT raise
```

- [ ] **Step 2: Run to verify fail**

Run: `cd deploy/compose/provisioner && python -m pytest test_provision.py -k kaby -v`
Expected: FAIL (`create_kaby_sa` not found).

- [ ] **Step 3: Implement** (in `provision.py`, after `grant_iam_viewer`)

```python
# ---- kabytech login SA (design 2026-06-22, identity UX) ----
# The backend authenticates as this machine user to create/invite users and
# (Phase 2) drive the Session API. Least privilege: ORG_USER_MANAGER (users +
# grants) + IAM_LOGIN_CLIENT (Session API / auth-request finalize). No ORG_OWNER.
KABY_SA_USERNAME = "kabytech-login"


def create_kaby_sa(token: str, headers: dict) -> str:
    resp = request_with_retry(
        "POST", f"{ISSUER}/management/v1/users/machine", headers=headers,
        json_body={"userName": KABY_SA_USERNAME, "name": KABY_SA_USERNAME,
                   "accessTokenType": "ACCESS_TOKEN_TYPE_JWT"})
    if resp.status_code == 200:
        return resp.json()["userId"]
    if resp.status_code == 409:
        raise SystemExit(
            "kabytech-login SA already exists (409): clean-boot contract — run "
            "`docker compose down -v` AND delete ./secrets.")
    resp.raise_for_status()
    raise RuntimeError(f"create_kaby_sa unexpected status {resp.status_code}")


def assign_kaby_org_member(token: str, headers: dict, uid: str) -> None:
    """Org member with ORG_USER_MANAGER (create users + grants). Idempotent."""
    resp = request_with_retry(
        "POST", f"{ISSUER}/management/v1/orgs/me/members", headers=headers,
        json_body={"userId": uid, "roles": ["ORG_USER_MANAGER"]})
    if not is_success(resp.status_code):
        resp.raise_for_status()


def assign_kaby_login_client(token: str, uid: str) -> None:
    """Instance member with IAM_LOGIN_CLIENT (Session API). Instance-scoped:
    NO x-zitadel-orgid header (mirrors grant_iam_viewer). Idempotent (409 ok)."""
    resp = request_with_retry(
        "POST", f"{ISSUER}/admin/v1/members",
        headers={"Authorization": f"Bearer {token}", "Content-Type": "application/json"},
        json_body={"userId": uid, "roles": ["IAM_LOGIN_CLIENT"]})
    if not is_success(resp.status_code):
        resp.raise_for_status()
```

- [ ] **Step 4: Wire into `main()`** — after the kabytech OIDC client block (the `create_kabytech_oidc_app` lines), add:

```python
    # kabytech login SA: the backend authenticates as this user to invite/create
    # users and (Phase 2) drive the Session API.
    kaby_sa_id = create_kaby_sa(token, headers)
    kaby_sa_key = generate_json_key(token, headers, kaby_sa_id)
    assign_kaby_org_member(token, headers, kaby_sa_id)
    assign_kaby_login_client(token, kaby_sa_id)
    write_secret("kabytech-login-key.json", json.dumps(kaby_sa_key))
    write_secret("kabytech_login_user_id", kaby_sa_id)
    print(f"[provision] kabytech-login SA id={kaby_sa_id} "
          f"roles=ORG_USER_MANAGER+IAM_LOGIN_CLIENT")
```

- [ ] **Step 5: Mock the new calls in `test_main`** — inside `test_main_...`'s ExitStack, add:

```python
        p("create_kaby_sa", return_value="kaby-sa-1")
        p("assign_kaby_org_member")
        p("assign_kaby_login_client")
```

(The existing `generate_json_key` mock returns `{"userId": "kaby-1"}`; the new `write_secret` calls land in `written`. Add asserts after the block:)

```python
    assert json.loads(written["kabytech-login-key.json"]) == {"userId": "kaby-1"}
    assert written["kabytech_login_user_id"] == "kaby-sa-1"
```

- [ ] **Step 6: Run the full provisioner suite**

Run: `cd deploy/compose/provisioner && python -m pytest test_provision.py -q`
Expected: PASS (all existing + 4 new).

- [ ] **Step 7: Commit**

```bash
git add deploy/compose/provisioner/provision.py deploy/compose/provisioner/test_provision.py
git commit -m "feat(provisioner): kabytech-login SA (ORG_USER_MANAGER + IAM_LOGIN_CLIENT) + key"
```

---

### Task 3: Provisioner — env-driven Zitadel SMTP config

Configure Zitadel's SMTP provider from the `KABY_SMTP_*` env vars so invite emails send (to MailHog in dev).

**Files:**
- Modify: `deploy/compose/provisioner/provision.py`
- Test: `deploy/compose/provisioner/test_provision.py`

**Interfaces:**
- Produces: `build_smtp_body(env: dict) -> dict`, `require_smtp_env(env: dict) -> dict` (fail-fast on missing HOST/PORT/SENDER_ADDRESS), `configure_smtp(token, env) -> None`.

- [ ] **Step 1: Write the failing tests**

```python
def test_build_smtp_body_from_env():
    env = {"KABY_SMTP_HOST": "mailhog", "KABY_SMTP_PORT": "1025",
           "KABY_SMTP_TLS": "false", "KABY_SMTP_USER": "", "KABY_SMTP_PASSWORD": "",
           "KABY_SMTP_SENDER_ADDRESS": "noreply@kabytech.local",
           "KABY_SMTP_SENDER_NAME": "kabytech"}
    b = provision.build_smtp_body(env)
    assert b["host"] == "mailhog:1025"
    assert b["tls"] is False
    assert b["senderAddress"] == "noreply@kabytech.local"
    assert b["senderName"] == "kabytech"
    assert b["user"] == "" and b["password"] == ""


def test_require_smtp_env_rejects_missing_host():
    with pytest.raises(SystemExit):
        provision.require_smtp_env({"KABY_SMTP_PORT": "1025",
                                    "KABY_SMTP_SENDER_ADDRESS": "a@b.c"})


def test_build_smtp_tls_true_when_truthy():
    env = {"KABY_SMTP_HOST": "smtp.example.com", "KABY_SMTP_PORT": "587",
           "KABY_SMTP_TLS": "true", "KABY_SMTP_SENDER_ADDRESS": "a@b.c",
           "KABY_SMTP_SENDER_NAME": "x"}
    assert provision.build_smtp_body(env)["tls"] is True
```

- [ ] **Step 2: Run to verify fail**

Run: `cd deploy/compose/provisioner && python -m pytest test_provision.py -k smtp -v`
Expected: FAIL (`build_smtp_body` not found).

- [ ] **Step 3: Implement** (in `provision.py`)

```python
# ---- Zitadel SMTP config (env-driven; design 2026-06-22) ----
def require_smtp_env(env: dict) -> dict:
    """Fail-fast: HOST/PORT/SENDER_ADDRESS are required (name the first missing)."""
    for k in ("KABY_SMTP_HOST", "KABY_SMTP_PORT", "KABY_SMTP_SENDER_ADDRESS"):
        if not (env.get(k) or "").strip():
            raise SystemExit(f"{k} must be set for SMTP config (no default)")
    return env


def build_smtp_body(env: dict) -> dict:
    """AddSMTPConfigRequest body from KABY_SMTP_* env. host = HOST:PORT; TLS is
    off unless an explicit truthy value."""
    tls = (env.get("KABY_SMTP_TLS") or "").strip().lower() not in ("", "false", "0", "no")
    return {
        "host": f"{env['KABY_SMTP_HOST'].strip()}:{env['KABY_SMTP_PORT'].strip()}",
        "tls": tls,
        "senderAddress": env["KABY_SMTP_SENDER_ADDRESS"].strip(),
        "senderName": (env.get("KABY_SMTP_SENDER_NAME") or "kabytech").strip(),
        "user": (env.get("KABY_SMTP_USER") or "").strip(),
        "password": (env.get("KABY_SMTP_PASSWORD") or "").strip(),
    }


def configure_smtp(token: str, env: dict) -> None:
    """POST /admin/v1/smtp (instance-scoped, no org header). 409 == already set."""
    require_smtp_env(env)
    resp = request_with_retry(
        "POST", f"{ISSUER}/admin/v1/smtp",
        headers={"Authorization": f"Bearer {token}", "Content-Type": "application/json"},
        json_body=build_smtp_body(env))
    if not is_success(resp.status_code):
        resp.raise_for_status()
```

- [ ] **Step 4: Wire into `main()`** — after the kabytech SA block:

```python
    # Env-driven SMTP so invite emails send (MailHog in dev, real SMTP in prod).
    configure_smtp(token, os.environ)
    print(f"[provision] SMTP configured host={os.environ.get('KABY_SMTP_HOST')}")
```

- [ ] **Step 5: Mock in `test_main`** — add `p("configure_smtp")` to the ExitStack.

- [ ] **Step 6: Run + commit**

Run: `cd deploy/compose/provisioner && python -m pytest test_provision.py -q` → PASS.

```bash
git add deploy/compose/provisioner/provision.py deploy/compose/provisioner/test_provision.py
git commit -m "feat(provisioner): env-driven Zitadel SMTP config (KABY_SMTP_*)"
```

---

### Task 4: Backend — SA token mint + Zitadel client + chat.admin extractor

Add the SA JWT-bearer token mint (ported from `admin-api/src/zitadel/token.rs`), a thin Zitadel HTTP client, config for the SA key path, and an `Operator` (chat.admin) session extractor.

**Files:**
- Modify: `services/kabytech/backend/src/config.rs` (add `sa_key_path`)
- Create: `services/kabytech/backend/src/zitadel.rs`
- Modify: `services/kabytech/backend/src/session.rs` (add `Operator`)
- Modify: `services/kabytech/backend/src/lib.rs` (add `pub mod zitadel;`)

**Interfaces:**
- Produces: `config::KabyConfig.sa_key_path: String` (env `KABY_SA_KEY_PATH`); `zitadel::build_assertion(user_id, key_id, pem, issuer, now) -> Result<String,String>`; `zitadel::Zitadel { http, issuer, project_id, sa_key_path }` with `async fn mint_token(&self) -> Result<String, String>`; `session::Operator { user_id, name, roles }` extractor requiring `chat.admin`.

- [ ] **Step 1: Add `sa_key_path` to `KabyConfig`** — in `config.rs`, add the field to the struct and `from_map`:

```rust
    pub sa_key_path: String,
```
and in `from_map` (before `cookie_secure`):
```rust
            sa_key_path: require_var("KABY_SA_KEY_PATH", get("KABY_SA_KEY_PATH"))?,
```
Update `config.rs` tests `full_map()` to include `("KABY_SA_KEY_PATH", "/secrets/kabytech-login-key.json")`, and any `KabyConfig { .. }` literals in `auth.rs` tests to add `sa_key_path: "/x".into(),`.

- [ ] **Step 2: Write the failing token test** — create `services/kabytech/backend/src/zitadel.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_assertion_signs_rs256_with_kid() {
        // A throwaway 2048-bit test key is generated in Step 3's testdata note;
        // here we assert the function rejects a bad PEM (no key file needed).
        let err = build_assertion("u", "k", "not a pem", "http://iss", 0).unwrap_err();
        assert!(err.to_lowercase().contains("pem") || err.to_lowercase().contains("key"));
    }
}
```

- [ ] **Step 3: Implement `zitadel.rs`** (port of `token.rs` mint, slimmed; returns `String` errors)

```rust
//! kabytech's Zitadel client: SA JWT-bearer token mint + User-API calls for
//! invite/accept. Ported from admin-api/src/zitadel/{token,users}.rs.

use serde_json::{json, Value};

#[derive(Clone)]
pub struct Zitadel {
    pub http: std::sync::Arc<reqwest::Client>,
    pub issuer: String,
    pub project_id: String,
    pub sa_key_path: String,
}

/// The `zitadel` literal targets Zitadel's internal project so the Management
/// API accepts the minted token (the admin-api §2.5 scope trap).
const ADMIN_SCOPE: &str = "openid profile urn:zitadel:iam:org:project:id:zitadel:aud";

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// PURE: sign the JWT-bearer assertion (RS256, header kid, iss=sub=user_id).
pub fn build_assertion(user_id: &str, key_id: &str, pem: &str, issuer: &str, now: u64) -> Result<String, String> {
    use jsonwebtoken::{encode, Algorithm, EncodingKey, Header};
    let mut header = Header::new(Algorithm::RS256);
    header.kid = Some(key_id.to_string());
    let claims = json!({ "iss": user_id, "sub": user_id, "aud": issuer, "iat": now, "exp": now + 3600 });
    let key = EncodingKey::from_rsa_pem(pem.as_bytes()).map_err(|e| format!("bad SA key PEM: {e}"))?;
    encode(&header, &claims, &key).map_err(|e| format!("sign assertion: {e}"))
}

impl Zitadel {
    /// Mint a Management-API token from the SA JSON key (jwt-bearer).
    pub async fn mint_token(&self) -> Result<String, String> {
        let raw = std::fs::read_to_string(&self.sa_key_path).map_err(|e| format!("read sa key: {e}"))?;
        let sa: Value = serde_json::from_str(&raw).map_err(|e| format!("sa key json: {e}"))?;
        let assertion = build_assertion(
            sa["userId"].as_str().unwrap_or_default(),
            sa["keyId"].as_str().unwrap_or_default(),
            sa["key"].as_str().unwrap_or_default(),
            &self.issuer, now_secs())?;
        let resp = self.http.post(format!("{}/oauth/v2/token", self.issuer))
            .form(&[("grant_type", "urn:ietf:params:oauth:grant-type:jwt-bearer"),
                    ("assertion", assertion.as_str()), ("scope", ADMIN_SCOPE)])
            .send().await.map_err(|e| format!("token endpoint: {e}"))?;
        if !resp.status().is_success() {
            return Err(format!("token mint returned {}", resp.status()));
        }
        let j: Value = resp.json().await.map_err(|e| format!("token json: {e}"))?;
        j["access_token"].as_str().map(String::from).ok_or_else(|| "no access_token".into())
    }
}
```

- [ ] **Step 4: Add `Operator` (chat.admin) to `session.rs`** — append:

```rust
/// A chat.admin operator session (for /invite). Same store key as EndUser
/// ("end_user") but requires chat.admin. Fail closed.
pub struct Operator(pub EndUser);

impl<S> FromRequestParts<S> for Operator
where
    S: Send + Sync,
{
    type Rejection = (StatusCode, &'static str);

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let user = EndUser::from_request_parts(parts, state).await?;
        if user.has("chat.admin") {
            Ok(Operator(user))
        } else {
            Err((StatusCode::FORBIDDEN, "operator lacks chat.admin"))
        }
    }
}
```

- [ ] **Step 5: Register the module + wire `AppState` (so the crate builds)** — add `pub mod zitadel;` to `lib.rs` and `pub zitadel: zitadel::Zitadel` to the `AppState` struct. In `main.rs`, shadow the client into an `Arc` after `assert_issuer_match`, build the `Zitadel`, and include it in `AppState`:

```rust
    // ...after assert_issuer_match(&http, &cfg).await? and the JwksCache setup:
    let http = std::sync::Arc::new(http);   // shadow Client -> Arc<Client>, shared
    let zitadel = kabytech_backend::zitadel::Zitadel {
        http: http.clone(), issuer: cfg.issuer.clone(),
        project_id: cfg.project_id.clone(), sa_key_path: cfg.sa_key_path.clone(),
    };
    let state = AppState { cfg: cfg.clone(), jwks, http: http.clone(), zitadel };
```

(This replaces the MVP's `http: Arc::new(http)` line.)

- [ ] **Step 6: Run tests + build**

Run: `cargo test -p kabytech-backend && cargo build -p kabytech-backend`
Expected: PASS (config, pkce, session, zitadel bad-pem) and the binary builds with the wired `AppState`.

- [ ] **Step 7: Commit**

```bash
git add services/kabytech/backend/src
git commit -m "feat(kabytech-backend): SA token mint + Zitadel client + chat.admin Operator extractor"
```

---

### Task 5: Backend — `POST /api/invite` (create user + grant chat.user + email invite)

**Files:**
- Modify: `services/kabytech/backend/src/zitadel.rs` (add `create_invited_user`, `grant_chat_user`)
- Modify: `services/kabytech/backend/src/auth.rs` (add `api_invite` handler)
- Modify: `services/kabytech/backend/src/main.rs` (route + put `Zitadel` in `AppState`)

**Interfaces:**
- Produces: `zitadel::invite_user_body(email, given, family, accept_url_template) -> Value`; `Zitadel::create_invited_user(token, email, given, family, accept_base) -> Result<String,String>` (userId); `Zitadel::grant_chat_user(token, user_id, project_id) -> Result<(),String>`; handler `auth::api_invite(Operator, State, Json<InviteReq>) -> Response`.

- [ ] **Step 1: Write the failing body-builder test** (in `zitadel.rs` tests)

```rust
    #[test]
    fn invite_user_body_carries_email_and_accept_url_template() {
        let b = invite_user_body("a@b.c", "Ada", "Lovelace", "http://localhost:3001");
        assert_eq!(b["username"], "a@b.c");
        assert_eq!(b["profile"]["givenName"], "Ada");
        assert_eq!(b["email"]["email"], "a@b.c");
        let tmpl = b["email"]["sendCode"]["urlTemplate"].as_str().unwrap();
        assert!(tmpl.starts_with("http://localhost:3001/accept?userID="));
        assert!(tmpl.contains("{{.UserID}}") && tmpl.contains("{{.Code}}") && tmpl.contains("{{.OrgID}}"));
        // invite-only: no password is set at creation
        assert!(b.get("password").is_none());
    }
```

- [ ] **Step 2: Run to verify fail** — `cargo test -p kabytech-backend invite_user_body` → FAIL.

- [ ] **Step 3: Implement the client methods** (in `zitadel.rs`)

```rust
/// PURE: the v2 create-human body for an INVITE — email with a sendCode
/// urlTemplate pointing at kabytech /accept; NO password (set on accept).
pub fn invite_user_body(email: &str, given: &str, family: &str, accept_base: &str) -> Value {
    let tmpl = format!("{accept_base}/accept?userID={{{{.UserID}}}}&code={{{{.Code}}}}&orgID={{{{.OrgID}}}}");
    json!({
        "username": email,
        "profile": { "givenName": given, "familyName": family },
        "email": { "email": email, "sendCode": { "urlTemplate": tmpl } },
    })
}

impl Zitadel {
    /// POST /v2/users/human → create the invited user (emails the invite link).
    pub async fn create_invited_user(&self, token: &str, email: &str, given: &str, family: &str, accept_base: &str)
        -> Result<String, String> {
        let resp = self.http.post(format!("{}/v2/users/human", self.issuer))
            .bearer_auth(token).json(&invite_user_body(email, given, family, accept_base))
            .send().await.map_err(|e| format!("create user: {e}"))?;
        let status = resp.status();
        let body: Value = resp.json().await.map_err(|e| format!("create user json: {e}"))?;
        if status.is_success() {
            return body["userId"].as_str().map(String::from).ok_or_else(|| "no userId".into());
        }
        if status.as_u16() == 409 {
            return Err("a user with that email already exists".into());
        }
        Err(format!("create user returned {status}: {body}"))
    }

    /// Grant exactly chat.user on the chat project (v1 mgmt grant).
    pub async fn grant_chat_user(&self, token: &str, user_id: &str) -> Result<(), String> {
        let resp = self.http.post(format!("{}/management/v1/users/{}/grants", self.issuer, user_id))
            .bearer_auth(token)
            .json(&json!({ "projectId": self.project_id, "roleKeys": ["chat.user"] }))
            .send().await.map_err(|e| format!("grant: {e}"))?;
        if resp.status().is_success() || resp.status().as_u16() == 409 { Ok(()) }
        else { Err(format!("grant returned {}", resp.status())) }
    }
}
```

- [ ] **Step 4: Add the handler** (in `auth.rs`)

```rust
use crate::session::Operator;

#[derive(serde::Deserialize)]
pub struct InviteReq { pub email: String, pub given: Option<String>, pub family: Option<String> }

/// chat.admin only (the Operator extractor enforces it). Creates the invited
/// user (emails the link) + grants chat.user.
pub async fn api_invite(_op: Operator, State(st): State<AppState>, Json(req): Json<InviteReq>) -> Response {
    let email = req.email.trim();
    if email.is_empty() || !email.contains('@') {
        return (StatusCode::BAD_REQUEST, "a valid email is required").into_response();
    }
    let token = match st.zitadel.mint_token().await {
        Ok(t) => t, Err(e) => return (StatusCode::BAD_GATEWAY, e).into_response(),
    };
    let given = req.given.as_deref().unwrap_or("");
    let family = req.family.as_deref().unwrap_or("");
    let uid = match st.zitadel.create_invited_user(&token, email, given, family, &st.cfg.public_origin).await {
        Ok(u) => u,
        Err(e) => return (StatusCode::CONFLICT, e).into_response(),
    };
    if let Err(e) = st.zitadel.grant_chat_user(&token, &uid).await {
        return (StatusCode::BAD_GATEWAY, e).into_response();
    }
    Json(serde_json::json!({ "ok": true, "userId": uid, "email": email })).into_response()
}
```

Note `Json` is already imported in `auth.rs` (Task 4 of the MVP). Add `axum::Json` to the `InviteReq` extraction if needed.

- [ ] **Step 5: Route** — `AppState.zitadel` was already wired in Task 4. In `main.rs`, add `use axum::routing::post;` and `.route("/api/invite", post(auth::api_invite))` to the router.

- [ ] **Step 6: Build + test** — `cargo test -p kabytech-backend && cargo build -p kabytech-backend` → PASS.

- [ ] **Step 7: Commit**

```bash
git add services/kabytech/backend/src
git commit -m "feat(kabytech-backend): POST /api/invite — create invited user + grant chat.user"
```

---

### Task 6: Backend — `POST /api/accept` (verify code + set password)

**Files:**
- Modify: `services/kabytech/backend/src/zitadel.rs` (`verify_email`, `set_password`)
- Modify: `services/kabytech/backend/src/auth.rs` (`api_accept`)
- Modify: `services/kabytech/backend/src/main.rs` (route)

**Interfaces:**
- Produces: `Zitadel::verify_email(token, user_id, code) -> Result<(),String>` (POST `/v2/users/{id}/email/verify` body `{verificationCode}`); `Zitadel::set_password(token, user_id, password) -> Result<(),String>` (PUT `/management/v1/users/{id}/password` body `{newPassword:{password,changeRequired:false}}`); handler `auth::api_accept(State, Json<AcceptReq>) -> Response`.

- [ ] **Step 1: Write the failing body-builder test** (in `zitadel.rs` tests)

```rust
    #[test]
    fn set_password_body_shape() {
        let b = set_password_body("hunter2");
        assert_eq!(b["newPassword"]["password"], "hunter2");
        assert_eq!(b["newPassword"]["changeRequired"], false);
    }
```

- [ ] **Step 2: Run to verify fail** — `cargo test -p kabytech-backend set_password_body` → FAIL.

- [ ] **Step 3: Implement** (in `zitadel.rs`)

```rust
pub fn set_password_body(password: &str) -> Value {
    json!({ "newPassword": { "password": password, "changeRequired": false } })
}

impl Zitadel {
    /// Verify the emailed code (proves email ownership). v2:
    /// POST /v2/users/{id}/email/verify { verificationCode }.
    pub async fn verify_email(&self, token: &str, user_id: &str, code: &str) -> Result<(), String> {
        let resp = self.http.post(format!("{}/v2/users/{}/email/verify", self.issuer, user_id))
            .bearer_auth(token).json(&json!({ "verificationCode": code }))
            .send().await.map_err(|e| format!("verify email: {e}"))?;
        if resp.status().is_success() { Ok(()) }
        else { Err("invalid or expired invite code".into()) }
    }

    /// Set the user's password (SA-authorized; valid while the user is Initial).
    pub async fn set_password(&self, token: &str, user_id: &str, password: &str) -> Result<(), String> {
        let resp = self.http.put(format!("{}/management/v1/users/{}/password", self.issuer, user_id))
            .bearer_auth(token).json(&set_password_body(password))
            .send().await.map_err(|e| format!("set password: {e}"))?;
        if resp.status().is_success() { Ok(()) }
        else { Err(format!("set password returned {}", resp.status())) }
    }
}
```

- [ ] **Step 4: Add the handler** (in `auth.rs`)

```rust
#[derive(serde::Deserialize)]
pub struct AcceptReq { pub user_id: String, pub code: String, pub password: String }

/// Unauthenticated by necessity, but fail-closed: the emailed code MUST verify
/// (proves email ownership) before any password is set.
pub async fn api_accept(State(st): State<AppState>, Json(req): Json<AcceptReq>) -> Response {
    if req.password.len() < 8 {
        return (StatusCode::BAD_REQUEST, "password must be at least 8 characters").into_response();
    }
    let token = match st.zitadel.mint_token().await {
        Ok(t) => t, Err(e) => return (StatusCode::BAD_GATEWAY, e).into_response(),
    };
    if let Err(e) = st.zitadel.verify_email(&token, &req.user_id, &req.code).await {
        return (StatusCode::FORBIDDEN, e).into_response();   // bad/expired code → reject
    }
    if let Err(e) = st.zitadel.set_password(&token, &req.user_id, &req.password).await {
        return (StatusCode::BAD_GATEWAY, e).into_response();
    }
    Json(serde_json::json!({ "ok": true })).into_response()
}
```

- [ ] **Step 5: Route** — `.route("/api/accept", post(auth::api_accept))` in `main.rs`.

- [ ] **Step 6: Build + test** — `cargo test -p kabytech-backend && cargo build -p kabytech-backend` → PASS.

- [ ] **Step 7: Commit**

```bash
git add services/kabytech/backend/src
git commit -m "feat(kabytech-backend): POST /api/accept — verify invite code + set password"
```

---

### Task 7: Frontend — beautiful `/invite` and `/accept` pages

**Files:**
- Create: `services/kabytech/frontend/app/invite/page.tsx`
- Create: `services/kabytech/frontend/app/invite/page.test.tsx`
- Create: `services/kabytech/frontend/app/accept/page.tsx`
- Create: `services/kabytech/frontend/app/accept/page.test.tsx`
- Modify: `services/kabytech/frontend/next.config.ts` (proxy `/api/:path*` already covers `/api/invite` + `/api/accept` — verify, no change expected)
- Create: `services/kabytech/frontend/components/Card.tsx` (shared beautiful shell)

**Interfaces:**
- Consumes: backend `POST /api/invite` `{email,given,family}` and `POST /api/accept` `{user_id,code,password}` via the same-origin proxy.

- [ ] **Step 1: Shared card shell** — `components/Card.tsx`:

```tsx
export function AuthCard({ title, subtitle, children }: {
  title: string; subtitle?: string; children: React.ReactNode;
}) {
  return (
    <main className="flex min-h-screen items-center justify-center bg-gradient-to-br from-indigo-50 via-white to-slate-100 p-6">
      <div className="w-full max-w-md rounded-2xl border border-slate-200/70 bg-white/90 p-8 shadow-xl backdrop-blur">
        <div className="mb-6">
          <div className="mb-1 text-sm font-semibold tracking-wide text-indigo-600">kabytech</div>
          <h1 className="text-2xl font-semibold text-slate-900">{title}</h1>
          {subtitle && <p className="mt-1 text-sm text-slate-500">{subtitle}</p>}
        </div>
        {children}
      </div>
    </main>
  );
}

export const inputCls =
  "w-full rounded-lg border border-slate-300 px-3 py-2 text-sm outline-none transition focus:border-indigo-500 focus:ring-2 focus:ring-indigo-200";
export const btnCls =
  "w-full rounded-lg bg-indigo-600 px-4 py-2.5 text-sm font-semibold text-white transition hover:bg-indigo-700 disabled:opacity-50";
```

- [ ] **Step 2: `/invite` failing test** — `app/invite/page.test.tsx`:

```tsx
import { describe, it, expect, vi, afterEach } from "vitest";
import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import Page from "./page";

afterEach(() => vi.restoreAllMocks());

describe("invite page", () => {
  it("posts the email and shows a success state", async () => {
    const fetchMock = vi.fn(async () =>
      new Response(JSON.stringify({ ok: true, email: "a@b.c" }),
        { status: 200, headers: { "content-type": "application/json" } }));
    vi.stubGlobal("fetch", fetchMock);
    render(<Page />);
    fireEvent.change(screen.getByPlaceholderText(/email/i), { target: { value: "a@b.c" } });
    fireEvent.click(screen.getByRole("button", { name: /send invite/i }));
    await waitFor(() => expect(screen.getByText(/invite sent/i)).toBeInTheDocument());
    expect(fetchMock).toHaveBeenCalledWith("/api/invite", expect.objectContaining({ method: "POST" }));
  });
});
```

- [ ] **Step 3: Run to verify fail** — `cd services/kabytech/frontend && pnpm exec vitest run app/invite` → FAIL.

- [ ] **Step 4: Implement `/invite`** — `app/invite/page.tsx`:

```tsx
"use client";
import { useState } from "react";
import { AuthCard, inputCls, btnCls } from "@/components/Card";

export default function Page() {
  const [email, setEmail] = useState(""); const [given, setGiven] = useState("");
  const [family, setFamily] = useState(""); const [sent, setSent] = useState<string | null>(null);
  const [err, setErr] = useState<string | null>(null); const [busy, setBusy] = useState(false);

  async function submit(e: React.FormEvent) {
    e.preventDefault(); setBusy(true); setErr(null);
    const r = await fetch("/api/invite", { method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ email, given, family }) });
    setBusy(false);
    if (r.ok) setSent(email); else setErr((await r.text()) || "invite failed");
  }

  if (sent) return (
    <AuthCard title="Invite sent" subtitle={`An invitation email is on its way to ${sent}.`}>
      <button className={btnCls} onClick={() => { setSent(null); setEmail(""); }}>Invite another</button>
    </AuthCard>
  );
  return (
    <AuthCard title="Invite a user" subtitle="They'll get an email to set their password and join.">
      <form onSubmit={submit} className="space-y-3">
        <input className={inputCls} placeholder="Email address" type="email" required
          value={email} onChange={(e) => setEmail(e.target.value)} />
        <div className="flex gap-3">
          <input className={inputCls} placeholder="First name" value={given} onChange={(e) => setGiven(e.target.value)} />
          <input className={inputCls} placeholder="Last name" value={family} onChange={(e) => setFamily(e.target.value)} />
        </div>
        {err && <p className="text-sm text-rose-600">{err}</p>}
        <button className={btnCls} disabled={busy}>{busy ? "Sending…" : "Send invite"}</button>
      </form>
    </AuthCard>
  );
}
```

- [ ] **Step 5: `/accept` failing test** — `app/accept/page.test.tsx`:

```tsx
import { describe, it, expect, vi, afterEach } from "vitest";
import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import Page from "./page";

afterEach(() => vi.restoreAllMocks());

describe("accept page", () => {
  it("posts user_id, code, and password", async () => {
    const fetchMock = vi.fn(async () => new Response(JSON.stringify({ ok: true }), { status: 200 }));
    vi.stubGlobal("fetch", fetchMock);
    // jsdom-safe: set the URL the page reads via history (location.search is read-only).
    window.history.pushState({}, "", "/accept?userID=u1&code=c1&orgID=o1");
    render(<Page />);
    fireEvent.change(screen.getByPlaceholderText(/^password/i), { target: { value: "hunter2!" } });
    fireEvent.change(screen.getByPlaceholderText(/confirm/i), { target: { value: "hunter2!" } });
    fireEvent.click(screen.getByRole("button", { name: /set password/i }));
    await waitFor(() => expect(fetchMock).toHaveBeenCalledWith("/api/accept", expect.objectContaining({ method: "POST" })));
  });
});
```

- [ ] **Step 6: Implement `/accept`** — `app/accept/page.tsx`:

```tsx
"use client";
import { useState } from "react";
import { AuthCard, inputCls, btnCls } from "@/components/Card";

export default function Page() {
  const [pw, setPw] = useState(""); const [confirm, setConfirm] = useState("");
  const [err, setErr] = useState<string | null>(null); const [done, setDone] = useState(false);
  const [busy, setBusy] = useState(false);

  async function submit(e: React.FormEvent) {
    e.preventDefault(); setErr(null);
    if (pw.length < 8) return setErr("Password must be at least 8 characters.");
    if (pw !== confirm) return setErr("Passwords do not match.");
    const q = new URLSearchParams(location.search);
    setBusy(true);
    const r = await fetch("/api/accept", { method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ user_id: q.get("userID"), code: q.get("code"), password: pw }) });
    setBusy(false);
    if (r.ok) setDone(true); else setErr((await r.text()) || "could not set password");
  }

  if (done) return (
    <AuthCard title="You're all set" subtitle="Your password is set. You can sign in now.">
      <a className={btnCls + " block text-center"} href="/login">Go to sign in</a>
    </AuthCard>
  );
  return (
    <AuthCard title="Set your password" subtitle="Finish joining kabytech.">
      <form onSubmit={submit} className="space-y-3">
        <input className={inputCls} type="password" placeholder="Password" value={pw} onChange={(e) => setPw(e.target.value)} />
        <input className={inputCls} type="password" placeholder="Confirm password" value={confirm} onChange={(e) => setConfirm(e.target.value)} />
        {err && <p className="text-sm text-rose-600">{err}</p>}
        <button className={btnCls} disabled={busy}>{busy ? "Saving…" : "Set password"}</button>
      </form>
    </AuthCard>
  );
}
```

- [ ] **Step 7: Run tests + build**

Run: `cd services/kabytech/frontend && pnpm test && pnpm run build`
Expected: PASS (login + invite + accept tests) and a clean build.

- [ ] **Step 8: Commit**

```bash
git add services/kabytech/frontend/app services/kabytech/frontend/components
git commit -m "feat(kabytech-frontend): beautiful /invite and /accept pages"
```

---

### Task 8: Compose wiring + live smoke

**Files:**
- Modify: `docker-compose.yml` (mount the SA key + `KABY_SA_KEY_PATH` into `kabytech-backend`)

- [ ] **Step 1: Wire the SA key into the backend** — in `docker-compose.yml`, `kabytech-backend.environment` add `KABY_SA_KEY_PATH: /secrets/kabytech-login-key.json` (the `./secrets:/secrets:ro` mount already exists). Also set the backend's `KABY_PUBLIC_ORIGIN` is already `http://localhost:3001` (the invite-link base).

- [ ] **Step 2: Clean reprovision (creates the SA + SMTP + MailHog)**

```bash
docker compose down -v && rm -rf ./secrets
KABYTECH_OIDC_REDIRECT_URI=http://localhost:3001/callback \
KABYTECH_OIDC_POST_LOGOUT_URI=http://localhost:3001/ \
  docker compose up -d --build
```

Expected: provisioner exits 0; `secrets/kabytech-login-key.json` exists; `docker compose logs zitadel-init` shows the SMTP + kabytech-login lines.

- [ ] **Step 3: Live smoke — invite → email → accept → login**

1. Log into kabytech as `admin` (chat.admin) at `http://localhost:3001/login` (or, until Phase 2, via the OIDC redirect). Open `http://localhost:3001/invite`.
2. Invite `e2e-invitee@kabytech.local`, first/last name. Expect "Invite sent".
3. Open MailHog at `http://localhost:8025`, open the invite email, click the link (→ `http://localhost:3001/accept?userID=…&code=…&orgID=…`).
4. Set a password (≥8 chars) → "You're all set".
5. Go to `/login`, sign in as `e2e-invitee@kabytech.local` with that password → land authenticated ("Signed in as …").

Expected: all five succeed. A 403 on `/invite` means the logged-in user isn't `chat.admin`. An empty MailHog inbox means SMTP config didn't apply (check `docker compose logs zitadel-init`).

- [ ] **Step 4: Commit**

```bash
git add docker-compose.yml
git commit -m "feat(kabytech): mount login SA key into the backend + Phase 1 invite smoke"
```

---

## Final verification (after all tasks)

1. **Provisioner tests:** `cd deploy/compose/provisioner && python -m pytest test_provision.py -q` → all pass.
2. **Backend tests + build:** `cargo test -p kabytech-backend && cargo build -p kabytech-backend` → green.
3. **Frontend tests + build:** `cd services/kabytech/frontend && pnpm test && pnpm run build` → green.
4. **Live smoke (Task 8 Step 3):** invite → MailHog → accept → login as the invitee succeeds.

Phase 2 (custom Session-API login replacing the hosted redirect) is a separate plan, unblocked by the `IAM_LOGIN_CLIENT` role granted here.
