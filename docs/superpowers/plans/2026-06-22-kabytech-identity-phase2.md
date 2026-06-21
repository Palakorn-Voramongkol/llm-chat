# kabytech Identity UX — Phase 2 (Custom Login) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace Zitadel's plain hosted login with kabytech's own beautiful `/login` page, driven by Zitadel's Session API + OIDC auth-request finalize, so end-users authenticate entirely on a kabytech-branded form.

**Architecture:** Configure the kabytech OIDC app to **Login V2** with `baseUri = {frontend origin}`, so an authorize request redirects the browser to `{frontend}/login?authRequest=<id>` instead of Zitadel's hosted UI. The kabytech `/login` page renders a custom form; `POST /api/login` creates a Zitadel **session** (password check), **finalizes** the auth request (with the `IAM_LOGIN_CLIENT` SA already provisioned in Phase 1), and returns the callback URL the browser follows to complete the existing `/callback` → tokens → cookie flow.

**Tech Stack:** Rust (axum, reqwest, the existing kabytech-backend `Zitadel` client), Next.js 16 + Tailwind v4, Zitadel v2 Session API (`/v2/sessions`) + OIDC auth-request API (`/v2/oidc/auth_requests/{id}`), the v1 `UpdateOIDCAppConfig`.

## Global Constraints

Copied from `docs/superpowers/specs/2026-06-22-kabytech-identity-ux-design.md` (Phase 2).

- **Gate `chat.user`, fail closed** — `/callback` already verifies the token's `chat.user` (unchanged); bad credentials → inline error.
- **The Login-V2 redirect path is fixed:** Zitadel appends `/login?authRequest=<id>` to the app's `baseUri`. So `baseUri = http://localhost:3001` ⇒ the browser lands on `http://localhost:3001/login?authRequest=<id>`. `/login` MUST therefore be a **frontend page** (not proxied to the backend).
- **The backend uses the `kabytech-login` SA** (already granted `IAM_LOGIN_CLIENT` in Phase 1) to read + finalize auth requests and create sessions. Its `mint_token()` (ADMIN_SCOPE) already exists.
- **Session API shape:** `POST /v2/sessions` body `{checks:{user:{loginName}, password:{password}}}` → `{sessionId, sessionToken}`. Finalize: `POST /v2/oidc/auth_requests/{id}` body `{session:{sessionId, sessionToken}}` → `{callbackUrl}`.
- **Passwords flow browser → backend → Zitadel only** (TLS in prod; local dev is plain HTTP). The frontend never talks to Zitadel directly.
- **No new SA / role** — Phase 1's `kabytech-login` SA already has everything.
- **Phase 2 only:** no MFA UI, no password reset (Zitadel handles reset out of band).

---

### Task 1: Login V2 app config (custom-UI delegation) — VERIFY FIRST

This is the linchpin and the riskiest unknown, so it is verified live before any UI is built. Configure the kabytech OIDC app to Login V2 with the frontend `baseUri`, and confirm an authorize request redirects to `/login?authRequest=…`.

**Files:**
- Modify: `deploy/compose/provisioner/provision.py` (add `loginVersion` to `build_kabytech_oidc_app_body`; new env `KABYTECH_LOGIN_BASE_URI`)
- Modify: `deploy/compose/provisioner/test_provision.py`
- Create: `services/kabytech/set-loginv2-dev.py` (apply Login V2 to a running app via `UpdateOIDCAppConfig`, no reprovision)

**Interfaces:**
- Produces: `KABYTECH_LOGIN_BASE_URI` (env, default `http://localhost:3001`); `build_kabytech_oidc_app_body` now emits `loginVersion: {loginV2: {baseUri}}`.

- [ ] **Step 1: Write the failing test** (append to `test_provision.py`)

```python
def test_kabytech_oidc_app_uses_login_v2_with_base_uri(monkeypatch):
    monkeypatch.setenv("KABYTECH_LOGIN_BASE_URI", "http://localhost:3001")
    import importlib
    importlib.reload(provision)  # re-read the env-derived module constant
    b = provision.build_kabytech_oidc_app_body(["https://gw.example/callback"], [])
    assert b["loginVersion"]["loginV2"]["baseUri"] == "http://localhost:3001"
    importlib.reload(provision)  # restore default for other tests
```

- [ ] **Step 2: Run to verify it fails**

Run: `cd deploy/compose/provisioner && python -m pytest test_provision.py -k login_v2 -v`
Expected: FAIL (`loginVersion` KeyError).

- [ ] **Step 3: Implement** — in `provision.py`, near the other kabytech OIDC constants add:

```python
KABYTECH_LOGIN_BASE_URI = os.environ.get("KABYTECH_LOGIN_BASE_URI", "http://localhost:3001")
```

and in `build_kabytech_oidc_app_body`, add this key to the returned dict (before `devMode`):

```python
        # Login V2: delegate the login UI to kabytech. Zitadel redirects an
        # authorize request to {baseUri}/login?authRequest=<id> (the /login path
        # is fixed/appended by Zitadel) instead of rendering its hosted page.
        "loginVersion": {"loginV2": {"baseUri": KABYTECH_LOGIN_BASE_URI}},
```

- [ ] **Step 4: Run to verify it passes**

Run: `cd deploy/compose/provisioner && python -m pytest test_provision.py -q`
Expected: PASS (all, incl. the new test).

- [ ] **Step 5: Dev helper to apply Login V2 to a running app**

Create `services/kabytech/set-loginv2-dev.py` (avoids a destructive reprovision):

```python
"""DEV: set the kabytech OIDC app to Login V2 (baseUri=frontend) on a running
stack via UpdateOIDCAppConfig, so authorize requests redirect to the custom
login page. Uses the bootstrap key. Run: python services/kabytech/set-loginv2-dev.py"""
import json, os, sys
os.environ["ADMIN_KEY_PATH"] = "secrets/_bootstrap-admin-sa.json"
os.environ.setdefault("PROVISION_ISSUER", "http://host.docker.internal:8080")
sys.path.insert(0, os.path.join("deploy", "compose", "provisioner"))
import provision  # noqa: E402

base_uri = os.environ.get("KABYTECH_LOGIN_BASE_URI", "http://localhost:3001")
admin = provision.load_admin_key()
token = provision.mint_management_token(admin)
org_id = provision.fetch_org_id(token)
h = provision.mgmt_headers(token, org_id)
project_id = open("secrets/project_id").read().strip()
client_id = open("secrets/kabytech_oidc_client_id").read().strip()

# find the app id by its clientId
s = provision.request_with_retry(
    "POST", f"{provision.ISSUER}/management/v1/projects/{project_id}/apps/_search",
    headers=h, json_body={})
app = next(a for a in s.json()["result"]
           if a.get("oidcConfig", {}).get("clientId") == client_id)
app_id = app["id"]

# UpdateOIDCAppConfig: PUT keeps existing fields, so re-send the current oidc
# config + loginVersion. Pull the current config and merge.
cfg = app["oidcConfig"]
body = {
    "redirectUris": cfg.get("redirectUris", []),
    "postLogoutRedirectUris": cfg.get("postLogoutRedirectUris", []),
    "responseTypes": cfg.get("responseTypes", ["OIDC_RESPONSE_TYPE_CODE"]),
    "grantTypes": cfg.get("grantTypes", ["OIDC_GRANT_TYPE_AUTHORIZATION_CODE", "OIDC_GRANT_TYPE_REFRESH_TOKEN"]),
    "appType": cfg.get("appType", "OIDC_APP_TYPE_WEB"),
    "authMethodType": cfg.get("authMethodType", "OIDC_AUTH_METHOD_TYPE_BASIC"),
    "accessTokenType": cfg.get("accessTokenType", "OIDC_TOKEN_TYPE_JWT"),
    "accessTokenRoleAssertion": True,
    "idTokenRoleAssertion": True,
    "loginVersion": {"loginV2": {"baseUri": base_uri}},
}
r = provision.request_with_retry(
    "PUT", f"{provision.ISSUER}/management/v1/projects/{project_id}/apps/{app_id}/oidc_config",
    headers=h, json_body=body)
print("UpdateOIDCAppConfig", r.status_code, json.dumps(r.json())[:200])
```

- [ ] **Step 6: VERIFY live (the gate)** — apply Login V2 to the running app and confirm the redirect

```bash
docker run --rm -v "llm-chat_machinekey:/mk:ro" alpine cat /mk/zitadel-admin-sa.json > secrets/_bootstrap-admin-sa.json
python services/kabytech/set-loginv2-dev.py            # expect 200
rm -f secrets/_bootstrap-admin-sa.json
# Hop 1: backend /login 302s to the Zitadel authorize URL. Hop 2: that authorize
# request 302s to the login UI — with Login V2 it must target the kabytech path.
AUTHZ=$(curl -sS -o /dev/null -w "%{redirect_url}" "http://localhost:7670/login")
echo "authorize: $AUTHZ"
curl -sS -o /dev/null -w "delegates to: %{redirect_url}\n" "$AUTHZ"
```

Expected: the second line contains `localhost:3001/login?authRequest=` (Zitadel delegated to kabytech). **If it instead points at Zitadel's own `/ui/...` login**, per-app Login V2 did not take — STOP and report. (Fallback: set the instance default `ZITADEL_OIDC_DEFAULTLOGINURLV2=http://localhost:3001/login?authRequest=` on the zitadel container + reprovision, which applies Login V2 instance-wide.)

- [ ] **Step 7: Commit**

```bash
git add deploy/compose/provisioner/provision.py deploy/compose/provisioner/test_provision.py services/kabytech/set-loginv2-dev.py
git commit -m "feat(kabytech): kabytech OIDC app uses Login V2 (custom login UI delegation)"
```

---

### Task 2: Backend Zitadel client — session + auth-request methods

Add the Session API + auth-request read/finalize calls to the existing `Zitadel` client.

**Files:**
- Modify: `services/kabytech/backend/src/zitadel.rs`

**Interfaces:**
- Consumes: `Zitadel::mint_token()` (exists).
- Produces: pure `session_check_body(login_name, password) -> Value`, `finalize_body(session_id, session_token) -> Value`; `Zitadel::create_session(token, login_name, password) -> Result<(String,String),String>` (sessionId, sessionToken); `Zitadel::finalize_auth_request(token, auth_request_id, session_id, session_token) -> Result<String,String>` (callbackUrl).

- [ ] **Step 1: Write the failing tests** (in `zitadel.rs` tests)

```rust
    #[test]
    fn session_check_body_has_user_and_password() {
        let b = session_check_body("alice@x.test", "pw");
        assert_eq!(b["checks"]["user"]["loginName"], "alice@x.test");
        assert_eq!(b["checks"]["password"]["password"], "pw");
    }

    #[test]
    fn finalize_body_carries_session() {
        let b = finalize_body("sid-1", "stok-1");
        assert_eq!(b["session"]["sessionId"], "sid-1");
        assert_eq!(b["session"]["sessionToken"], "stok-1");
    }
```

- [ ] **Step 2: Run to verify fail** — `cargo test -p kabytech-backend session_check_body` → FAIL.

- [ ] **Step 3: Implement** — add the pure builders + methods to `zitadel.rs`

```rust
/// PURE: a v2 session create with a username + password check.
pub fn session_check_body(login_name: &str, password: &str) -> Value {
    json!({ "checks": { "user": { "loginName": login_name }, "password": { "password": password } } })
}

/// PURE: the auth-request finalize body (resolve the request with a session).
pub fn finalize_body(session_id: &str, session_token: &str) -> Value {
    json!({ "session": { "sessionId": session_id, "sessionToken": session_token } })
}
```

and inside `impl Zitadel`:

```rust
    /// Create a Zitadel session checking the user's password. Returns
    /// (sessionId, sessionToken). A wrong password yields an Err.
    pub async fn create_session(&self, token: &str, login_name: &str, password: &str)
        -> Result<(String, String), String> {
        let resp = self.http.post(format!("{}/v2/sessions", self.issuer))
            .bearer_auth(token).json(&session_check_body(login_name, password))
            .send().await.map_err(|e| format!("create session: {e}"))?;
        if !resp.status().is_success() {
            return Err("invalid credentials".into());
        }
        let j: Value = resp.json().await.map_err(|e| format!("session json: {e}"))?;
        let sid = j["sessionId"].as_str().ok_or("no sessionId")?.to_string();
        let stok = j["sessionToken"].as_str().ok_or("no sessionToken")?.to_string();
        Ok((sid, stok))
    }

    /// Finalize the OIDC auth request with the session; returns the callback URL
    /// the browser must follow to complete the code flow (needs IAM_LOGIN_CLIENT).
    pub async fn finalize_auth_request(&self, token: &str, auth_request_id: &str,
        session_id: &str, session_token: &str) -> Result<String, String> {
        let resp = self.http.post(format!("{}/v2/oidc/auth_requests/{}", self.issuer, auth_request_id))
            .bearer_auth(token).json(&finalize_body(session_id, session_token))
            .send().await.map_err(|e| format!("finalize: {e}"))?;
        if !resp.status().is_success() {
            let s = resp.status();
            let b = resp.text().await.unwrap_or_default();
            return Err(format!("finalize returned {s}: {b}"));
        }
        let j: Value = resp.json().await.map_err(|e| format!("finalize json: {e}"))?;
        j["callbackUrl"].as_str().map(String::from).ok_or_else(|| "no callbackUrl".into())
    }
```

- [ ] **Step 4: Run to verify pass** — `cargo test -p kabytech-backend` → PASS.

- [ ] **Step 5: Commit**

```bash
git add services/kabytech/backend/src/zitadel.rs
git commit -m "feat(kabytech-backend): Zitadel session + auth-request finalize client methods"
```

---

### Task 3: Backend handlers — `/api/login/start` + `POST /api/login`

Rename the MVP authorize-start off `/login` (now a frontend page) to `/api/login/start`, and add the session-driven `POST /api/login`.

**Files:**
- Modify: `services/kabytech/backend/src/auth.rs` (rename `login` → `login_start`; add `api_login`)
- Modify: `services/kabytech/backend/src/main.rs` (routes)

**Interfaces:**
- Consumes: `auth::build_authorize_url`, `Zitadel::{mint_token, create_session, finalize_auth_request}`.
- Produces: `auth::login_start` (GET, 302 to Zitadel authorize), `auth::api_login` (POST `{authRequest, loginName, password}` → `{callbackUrl}` or error).

- [ ] **Step 1: Rename the start handler** — in `auth.rs`, rename `pub async fn login(` to `pub async fn login_start(` (body unchanged — it still builds the authorize URL + redirects; Zitadel's Login V2 will bounce the browser to the frontend `/login?authRequest=`).

- [ ] **Step 2: Add the login handler** — append to `auth.rs`:

```rust
#[derive(Deserialize)]
pub struct LoginReq { pub auth_request: String, pub login_name: String, pub password: String }

/// Custom-login: create a Zitadel session (password check), finalize the OIDC
/// auth request, and return the callback URL the browser follows to finish the
/// code flow (-> /callback -> token exchange -> chat.user gate -> cookie).
pub async fn api_login(State(st): State<AppState>, Json(req): Json<LoginReq>) -> Response {
    let token = match st.zitadel.mint_token().await {
        Ok(t) => t, Err(e) => return (StatusCode::BAD_GATEWAY, e).into_response(),
    };
    let (sid, stok) = match st.zitadel.create_session(&token, req.login_name.trim(), &req.password).await {
        Ok(s) => s,
        Err(_) => return (StatusCode::UNAUTHORIZED, "invalid credentials").into_response(),
    };
    match st.zitadel.finalize_auth_request(&token, &req.auth_request, &sid, &stok).await {
        Ok(callback) => Json(serde_json::json!({ "callbackUrl": callback })).into_response(),
        Err(e) => (StatusCode::BAD_GATEWAY, e).into_response(),
    }
}
```

- [ ] **Step 3: Wire the routes** — in `main.rs`, replace `.route("/login", get(auth::login))` with:

```rust
        .route("/api/login/start", get(auth::login_start))
        .route("/api/login", post(auth::api_login))
```

- [ ] **Step 4: Build + test** — `cargo test -p kabytech-backend && cargo build -p kabytech-backend` → PASS.

- [ ] **Step 5: Commit**

```bash
git add services/kabytech/backend/src/auth.rs services/kabytech/backend/src/main.rs
git commit -m "feat(kabytech-backend): /api/login/start + POST /api/login (custom session login)"
```

---

### Task 4: Frontend — custom `/login` page + proxy update

`/login` becomes a frontend page: with `?authRequest` it shows the form; without, it kicks off the OIDC start.

**Files:**
- Create: `services/kabytech/frontend/app/login/page.tsx`
- Create: `services/kabytech/frontend/app/login/page.test.tsx`
- Modify: `services/kabytech/frontend/next.config.ts` (remove the `/login` rewrite; add `/api/login/start` is covered by `/api/:path*`)

**Interfaces:**
- Consumes: backend `GET /api/login/start`, `POST /api/login`.

- [ ] **Step 1: Remove the `/login` proxy** — in `next.config.ts` `rewrites()`, DELETE the line `{ source: "/login", destination: \`${KABY_BACKEND_ORIGIN}/login\` },`. Keep `/api/:path*`, `/callback`, `/logout`. (`/login` is now a Next page; `/api/login/start` proxies via `/api/:path*`.)

- [ ] **Step 2: Write the failing test** — `app/login/page.test.tsx`:

```tsx
import { describe, it, expect, vi, afterEach } from "vitest";
import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import Page from "./page";

afterEach(() => vi.restoreAllMocks());

describe("custom login page", () => {
  it("with ?authRequest, posts credentials and follows the callback", async () => {
    const assign = vi.fn();
    vi.stubGlobal("location", { search: "?authRequest=AR_1", assign,
      set href(v: string) { assign(v); } } as unknown as Location);
    const fetchMock = vi.fn(async () =>
      new Response(JSON.stringify({ callbackUrl: "http://host/callback?code=x" }),
        { status: 200, headers: { "content-type": "application/json" } }));
    vi.stubGlobal("fetch", fetchMock);
    render(<Page />);
    fireEvent.change(screen.getByPlaceholderText(/email|username/i), { target: { value: "a@b.c" } });
    fireEvent.change(screen.getByPlaceholderText(/password/i), { target: { value: "pw123456" } });
    fireEvent.click(screen.getByRole("button", { name: /sign in/i }));
    await waitFor(() => expect(fetchMock).toHaveBeenCalledWith("/api/login", expect.objectContaining({ method: "POST" })));
  });
});
```

- [ ] **Step 3: Run to verify fail** — `cd services/kabytech/frontend && pnpm exec vitest run app/login` → FAIL.

- [ ] **Step 4: Implement** — `app/login/page.tsx`:

```tsx
"use client";
import { useEffect, useState } from "react";
import { AuthCard, inputCls, btnCls } from "@/components/Card";

export default function Page() {
  const [authRequest, setAuthRequest] = useState<string | null>(null);
  const [loginName, setLoginName] = useState(""); const [password, setPassword] = useState("");
  const [err, setErr] = useState<string | null>(null); const [busy, setBusy] = useState(false);

  useEffect(() => {
    const ar = new URLSearchParams(location.search).get("authRequest");
    if (ar) setAuthRequest(ar);
    else location.href = "/api/login/start"; // begin the OIDC flow (Zitadel bounces back here)
  }, []);

  async function submit(e: React.FormEvent) {
    e.preventDefault(); setErr(null); setBusy(true);
    const r = await fetch("/api/login", { method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ auth_request: authRequest, login_name: loginName, password }) });
    setBusy(false);
    if (r.ok) { const { callbackUrl } = await r.json(); location.href = callbackUrl; }
    else setErr((await r.text()) || "sign in failed");
  }

  if (!authRequest) return <AuthCard title="Signing in…"><p className="text-slate-500">Redirecting…</p></AuthCard>;
  return (
    <AuthCard title="Sign in to kabytech" subtitle="Welcome back.">
      <form onSubmit={submit} className="space-y-3">
        <input className={inputCls} placeholder="Email or username" autoComplete="username"
          value={loginName} onChange={(e) => setLoginName(e.target.value)} />
        <input className={inputCls} type="password" placeholder="Password" autoComplete="current-password"
          value={password} onChange={(e) => setPassword(e.target.value)} />
        {err && <p className="text-sm text-rose-600">{err}</p>}
        <button className={btnCls} disabled={busy}>{busy ? "Signing in…" : "Sign in"}</button>
      </form>
    </AuthCard>
  );
}
```

- [ ] **Step 5: Run tests + build** — `cd services/kabytech/frontend && pnpm test && pnpm run build` → PASS (login/invite/accept/home tests) + a `/login` route in the build output.

- [ ] **Step 6: Commit**

```bash
git add services/kabytech/frontend/app/login services/kabytech/frontend/next.config.ts
git commit -m "feat(kabytech-frontend): custom /login page (Session API), drop hosted-login proxy"
```

---

### Task 5: Rebuild, redeploy, and live smoke

**Files:** none (deploy + verify).

- [ ] **Step 1: Rebuild + restart the kabytech images**

```bash
docker compose build kabytech-backend kabytech-frontend
docker compose up -d --no-deps kabytech-backend kabytech-frontend
```

(If Task 1's live check used the dev helper on the running app, Login V2 is already applied. For a from-scratch stack it is set by the provisioner.)

- [ ] **Step 2: Live smoke — the custom login replaces the hosted page**

1. Open `http://localhost:3001/` → click **Sign in** (→ `/login`).
2. `/login` (no authRequest) redirects to `/api/login/start` → Zitadel → **bounces back to `http://localhost:3001/login?authRequest=…`** showing the **kabytech** form (NOT Zitadel's hosted page).
3. Enter a `chat.user`'s credentials (e.g. the `chatter` account, or an invited user from Phase 1) → submit.
4. The browser follows the returned callback URL → `/callback` → land authenticated ("Signed in as …").
5. A wrong password shows the inline "invalid credentials" error on the kabytech form (no Zitadel page).

Expected: all five succeed; at no point does Zitadel's plain hosted login render. If step 2 still shows the Zitadel hosted page, Login V2 was not applied (re-run Task 1 Step 5/6).

- [ ] **Step 3: Commit (if any deploy notes/scripts changed)** — otherwise nothing to commit.

---

## Final verification (after all tasks)

1. **Provisioner tests:** `python -m pytest deploy/compose/provisioner/test_provision.py -q` → all pass.
2. **Backend tests + build:** `cargo test -p kabytech-backend && cargo build -p kabytech-backend` → green.
3. **Frontend tests + build:** `cd services/kabytech/frontend && pnpm test && pnpm run build` → green; build lists `/login`.
4. **Live smoke (Task 5 Step 2):** Sign in renders the **kabytech** form, credentials authenticate, wrong password errors inline — Zitadel's hosted login never appears.

With Phase 2, kabytech owns the entire end-user identity surface: custom login, invite, and accept-password pages, all backed by Zitadel via the Session + User APIs.
