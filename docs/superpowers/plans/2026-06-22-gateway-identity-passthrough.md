# Gateway Identity Pass-through (kabytech) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make each of kabytech's human end-users reach the manager under their *own* Zitadel `sub` (via upstream-IdP federation + JIT), so per-user usage graphs and per-user isolation work with no platform code change.

**Architecture:** The platform is unchanged — it already authenticates, attributes usage, and isolates per JWT `sub`. This plan delivers the *surrounding configuration*: (1) a Zitadel **auto-grant action** bound to the External-Authentication → Post-Creation trigger that grants `chat.user` to every JIT-federated user; (2) a Zitadel **OIDC web client** for kabytech's gateway; (3) a **runbook** for registering the upstream IdP (needs real, environment-specific secrets — operator-run, not automatable in CI); (4) an **in-repo E2E harness** proving two distinct `sub`s get two distinct graphs + isolated claude sessions; (5) the **integration contract** kabytech's off-repo gateway follows.

**Tech Stack:** Python (the `deploy/compose/provisioner` Zitadel provisioner + `pytest`), the Zitadel v1 Management REST API, the existing `clients/python/llm_chat` auth + WS client, the manager `/chat` and `/control` WebSocket endpoints.

## Global Constraints

Copied verbatim from `docs/superpowers/specs/2026-06-22-gateway-identity-passthrough-design.md`; every task's requirements implicitly include these.

- The manager gates **every** endpoint on the `chat.user` role (`manager/src/main.rs:1233`); a token's `aud` must contain the chat project id. The auto-grant must make `chat.user` present on a federated user's **first** token.
- An end-user token is obtained with these scopes (`clients/python/llm_chat/oidc.py:build_scope`): `openid profile email offline_access`, `urn:zitadel:iam:org:project:id:<CHAT_PROJECT_ID>:aud`, `urn:zitadel:iam:org:projects:roles`. `<CHAT_PROJECT_ID>` = the manager's configured audience = `secrets/project_id`.
- **One end-user = one `sub` = one manager `/chat` connection.** No multiplexing multiple end-users over one authenticated WS (the `sub` is fixed at handshake).
- **Fail-closed:** no `chat.user` grant → manager returns 403, no fallback bucket, no shared identity. kabytech holds **no** impersonation credential — it forwards only tokens users obtained themselves.
- **Least privilege:** the auto-grant action grants **exactly** `chat.user` on **exactly** the one chat project — never `chat.admin`, never role widening.
- **Provisioner idempotency:** every create is idempotent; a 409 `ALREADY_EXISTS` means "already provisioned", consistent with the existing `create_*` functions in `provision.py`.
- **No platform code change** to `manager` / `admin-api` / `admin-web` is expected. If the E2E harness (Task 4) surfaces a platform gap, stop and raise it — do not patch the platform inside this plan.
- **Testability boundary:** a real upstream IdP and kabytech's gateway are **off-repo** and need secrets unavailable in CI. Task 4 therefore proves the *platform behaviour* (per-`sub` attribution + isolation) using **machine stand-in identities** in place of federated humans — identical end-state (a distinct `sub` carrying `chat.user`). The federation/JIT/human-login path is verified by the Task 1 action + the Task 3 runbook, not by Task 4.

---

### Task 1: Auto-grant action + External-Auth Post-Creation trigger

Adds a Zitadel action that grants `chat.user` whenever a user is JIT-created via external authentication, and binds it to the correct flow/trigger. Idempotent. The pure body/script builders are unit-tested (matching the existing `test_provision.py` pure-helper pattern); the live POST helpers follow the established `create_*`/409 pattern and are exercised by provisioning, not unit tests.

**Files:**
- Modify: `deploy/compose/provisioner/provision.py` (add constants + 4 functions; call them from `main()`)
- Test: `deploy/compose/provisioner/test_provision.py` (add 4 pure tests)

**Interfaces:**
- Consumes: existing `request_with_retry(method, url, *, headers=None, json_body=None, ...) -> requests.Response`, `mgmt_headers(token, org_id) -> dict`, `ISSUER`, `ROLE_KEY` (`"chat.user"`), the `project_id` produced by `create_project`.
- Produces: `GRANT_ACTION_NAME: str`, `build_grant_action_script(project_id, role_key) -> str`, `build_grant_action_body(project_id, role_key) -> dict`, `find_action_id_by_name(token, headers, name) -> str | None`, `create_grant_action(token, headers, project_id) -> str` (action id), `bind_post_creation_trigger(token, headers, action_id) -> None`. Flow/trigger constants `FLOW_TYPE_EXTERNAL_AUTHENTICATION = "FLOW_TYPE_EXTERNAL_AUTHENTICATION"`, `TRIGGER_TYPE_POST_CREATION = "TRIGGER_TYPE_POST_CREATION"`.

- [ ] **Step 1: Write the failing tests**

Add to `deploy/compose/provisioner/test_provision.py`:

```python
def test_grant_action_script_embeds_project_and_role():
    s = provision.build_grant_action_script("proj-123", "chat.user")
    # The Zitadel v1 action runtime invokes the function whose name == the action name.
    assert f"function {provision.GRANT_ACTION_NAME}(ctx, api)" in s
    assert "api.userGrants.push(" in s
    assert "projectID: 'proj-123'" in s
    assert "roles: ['chat.user']" in s


def test_grant_action_body_shape():
    b = provision.build_grant_action_body("proj-123", "chat.user")
    assert b["name"] == provision.GRANT_ACTION_NAME
    assert b["timeout"] == "10s"
    assert b["allowedToFail"] is False
    assert "api.userGrants.push(" in b["script"]


def test_grant_action_grants_only_chat_user_least_privilege():
    # Least-privilege guard: the script must never grant chat.admin.
    s = provision.build_grant_action_script("p", "chat.user")
    assert "chat.admin" not in s


def test_trigger_constants_are_external_auth_post_creation():
    assert provision.FLOW_TYPE_EXTERNAL_AUTHENTICATION == "FLOW_TYPE_EXTERNAL_AUTHENTICATION"
    assert provision.TRIGGER_TYPE_POST_CREATION == "TRIGGER_TYPE_POST_CREATION"
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cd deploy/compose/provisioner && python -m pytest test_provision.py -k "grant_action or trigger_constants" -v`
Expected: FAIL with `AttributeError: module 'provision' has no attribute 'build_grant_action_script'`.

- [ ] **Step 3: Implement the constants + builders + live helpers**

Add to `deploy/compose/provisioner/provision.py` (near the other create_* helpers, after `grant_role`):

```python
# ---- Auto-grant on external-IdP JIT (design 2026-06-22) ----
# A Zitadel v1 action bound to the External-Authentication > Post-Creation
# trigger. It grants chat.user to every user JIT-created via a federated IdP,
# so the user's first token already carries the role the manager requires.
GRANT_ACTION_NAME = "grantChatUser"
# Flow/trigger enum identifiers (Zitadel mgmt v1, management.proto
# SetTriggerActionsRequest). The numeric forms "1"/"3" are accepted equivalents.
FLOW_TYPE_EXTERNAL_AUTHENTICATION = "FLOW_TYPE_EXTERNAL_AUTHENTICATION"
TRIGGER_TYPE_POST_CREATION = "TRIGGER_TYPE_POST_CREATION"


def build_grant_action_script(project_id: str, role_key: str) -> str:
    """The action body: a JS function (name MUST equal the action name) that
    pushes a single chat.user grant on the chat project. Least-privilege: it
    grants exactly role_key on exactly project_id."""
    return (
        f"function {GRANT_ACTION_NAME}(ctx, api) {{\n"
        f"  api.userGrants.push({{\n"
        f"    projectID: '{project_id}',\n"
        f"    roles: ['{role_key}']\n"
        f"  }});\n"
        f"}}\n"
    )


def build_grant_action_body(project_id: str, role_key: str) -> dict:
    return {
        "name": GRANT_ACTION_NAME,
        "script": build_grant_action_script(project_id, role_key),
        "timeout": "10s",
        "allowedToFail": False,
    }


def find_action_id_by_name(token: str, headers: dict, name: str):
    """Idempotency: actions are not deduplicated by name, so search first."""
    resp = request_with_retry(
        "POST", f"{ISSUER}/management/v1/actions/_search",
        headers=headers, json_body={})
    if not is_success(resp.status_code):
        raise RuntimeError(f"actions _search status {resp.status_code}")
    for a in resp.json().get("result", []):
        if a.get("name") == name:
            return a.get("id")
    return None


def create_grant_action(token: str, headers: dict, project_id: str) -> str:
    existing = find_action_id_by_name(token, headers, GRANT_ACTION_NAME)
    if existing:
        print(f"[provision] grant action already exists id={existing}")
        return existing
    resp = request_with_retry(
        "POST", f"{ISSUER}/management/v1/actions",
        headers=headers, json_body=build_grant_action_body(project_id, ROLE_KEY))
    if resp.status_code == 200:
        return resp.json()["id"]
    resp.raise_for_status()
    raise RuntimeError(f"create_grant_action unexpected status {resp.status_code}")


def bind_post_creation_trigger(token: str, headers: dict, action_id: str) -> None:
    """SetTriggerActions is a SET (idempotent): binds [action_id] to the
    External-Authentication Post-Creation trigger."""
    resp = request_with_retry(
        "POST",
        f"{ISSUER}/management/v1/flows/{FLOW_TYPE_EXTERNAL_AUTHENTICATION}"
        f"/trigger/{TRIGGER_TYPE_POST_CREATION}",
        headers=headers, json_body={"actionIds": [action_id]})
    if not is_success(resp.status_code):
        raise RuntimeError(f"bind_post_creation_trigger status {resp.status_code}")
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cd deploy/compose/provisioner && python -m pytest test_provision.py -k "grant_action or trigger_constants" -v`
Expected: PASS (4 passed).

- [ ] **Step 5: Wire into `main()`**

In `deploy/compose/provisioner/provision.py`, in `main()`, immediately after `grant_role(token, headers, user_id, project_id)` (the kabytech grant, ~line 603) add:

```python
    # Auto-grant chat.user to every externally-federated (JIT) user, so
    # kabytech's end-users reach the manager with chat.user on their first token.
    grant_action_id = create_grant_action(token, headers, project_id)
    bind_post_creation_trigger(token, headers, grant_action_id)
    print(f"[provision] external-auth post-creation auto-grant bound "
          f"action_id={grant_action_id} role={ROLE_KEY}")
```

- [ ] **Step 6: Commit**

```bash
git add deploy/compose/provisioner/provision.py deploy/compose/provisioner/test_provision.py
git commit -m "feat(provisioner): auto-grant chat.user on external-IdP JIT via post-creation action"
```

---

### Task 2: kabytech OIDC web client

Registers the confidential OIDC web app kabytech's gateway uses to run the Auth Code + PKCE login and obtain forwardable user tokens. Writes its client id + secret to `secrets/`. Mirrors the existing `create_admin_oidc_app` (WEB + BASIC) pattern.

**Files:**
- Modify: `deploy/compose/provisioner/provision.py` (constants, body builder, create fn, `main()` wiring + `write_secret`)
- Test: `deploy/compose/provisioner/test_provision.py` (add 2 pure tests)

**Interfaces:**
- Consumes: `request_with_retry`, `mgmt_headers`, `ISSUER`, `_both_loopback_hosts(uri) -> list`, `write_secret(name, content)`, `project_id`.
- Produces: `KABYTECH_OIDC_APP_NAME = "kabytech-gateway"`, `KABYTECH_OIDC_REDIRECT_URI`/`KABYTECH_OIDC_POST_LOGOUT_URI` (env-driven), `build_kabytech_oidc_app_body(redirect_uris, post_logout_uris) -> dict`, `create_kabytech_oidc_app(token, headers, project_id) -> tuple[str, str]` (clientId, clientSecret).

- [ ] **Step 1: Write the failing tests**

Add to `test_provision.py`:

```python
def test_kabytech_oidc_app_body_is_confidential_web_with_pkce_and_refresh():
    b = provision.build_kabytech_oidc_app_body(
        ["https://gw.example/callback"], ["https://gw.example/"])
    assert b["name"] == provision.KABYTECH_OIDC_APP_NAME
    assert b["appType"] == "OIDC_APP_TYPE_WEB"
    assert b["authMethodType"] == "OIDC_AUTH_METHOD_TYPE_BASIC"
    assert b["responseTypes"] == ["OIDC_RESPONSE_TYPE_CODE"]
    assert "OIDC_GRANT_TYPE_AUTHORIZATION_CODE" in b["grantTypes"]
    assert "OIDC_GRANT_TYPE_REFRESH_TOKEN" in b["grantTypes"]
    assert b["redirectUris"] == ["https://gw.example/callback"]


def test_kabytech_oidc_app_body_token_type_is_jwt():
    b = provision.build_kabytech_oidc_app_body(["https://gw.example/callback"], [])
    # JWT access tokens so the manager validates them via JWKS (not opaque).
    assert b["accessTokenType"] == "OIDC_TOKEN_TYPE_JWT"


def test_kabytech_oidc_app_asserts_roles_in_access_token():
    # The manager reads chat.user from the ACCESS token, and the chat project
    # has projectRoleAssertion=false, so the app must force role assertion or
    # chat.user never rides in the token and the manager 403s every end-user.
    b = provision.build_kabytech_oidc_app_body(["https://gw.example/callback"], [])
    assert b["accessTokenRoleAssertion"] is True
```

- [ ] **Step 2: Run to verify they fail**

Run: `cd deploy/compose/provisioner && python -m pytest test_provision.py -k kabytech -v`
Expected: FAIL with `AttributeError: ... 'build_kabytech_oidc_app_body'`.

- [ ] **Step 3: Implement**

Add to `provision.py` (after `create_admin_oidc_app`):

```python
# ---- kabytech gateway OIDC web client (design 2026-06-22) ----
# Confidential WEB app (BASIC + PKCE, code + refresh). kabytech's gateway runs
# the browser login through this client and forwards the resulting per-user
# access token to the manager. Redirect URIs are kabytech's, supplied by env;
# they default to a documented placeholder the operator overrides.
KABYTECH_OIDC_APP_NAME = "kabytech-gateway"
KABYTECH_OIDC_REDIRECT_URI = os.environ.get(
    "KABYTECH_OIDC_REDIRECT_URI", "https://gateway.kabytech.example/callback")
KABYTECH_OIDC_POST_LOGOUT_URI = os.environ.get(
    "KABYTECH_OIDC_POST_LOGOUT_URI", "https://gateway.kabytech.example/")


def build_kabytech_oidc_app_body(redirect_uris: list, post_logout_uris: list) -> dict:
    return {
        "name": KABYTECH_OIDC_APP_NAME,
        "redirectUris": redirect_uris,
        "postLogoutRedirectUris": post_logout_uris,
        "responseTypes": ["OIDC_RESPONSE_TYPE_CODE"],
        "grantTypes": ["OIDC_GRANT_TYPE_AUTHORIZATION_CODE",
                       "OIDC_GRANT_TYPE_REFRESH_TOKEN"],
        "appType": "OIDC_APP_TYPE_WEB",
        "authMethodType": "OIDC_AUTH_METHOD_TYPE_BASIC",
        "accessTokenType": "OIDC_TOKEN_TYPE_JWT",
        # Force chat.user into the ACCESS-token JWT (the chat project has
        # projectRoleAssertion=false); without this the manager 403s end-users.
        "accessTokenRoleAssertion": True,
        "idTokenRoleAssertion": True,
        "devMode": False,
    }


def create_kabytech_oidc_app(token: str, headers: dict, project_id: str):
    """Register the kabytech gateway's confidential OIDC web client. Returns
    (clientId, clientSecret); the secret is shown ONCE (clean-boot contract).
    Mirrors create_admin_oidc_app's status handling: 200 returns creds, 409 is
    the clean-boot contract (is_success() conflates 200/409, so check 200
    explicitly)."""
    body = build_kabytech_oidc_app_body(
        [KABYTECH_OIDC_REDIRECT_URI], [KABYTECH_OIDC_POST_LOGOUT_URI])
    resp = request_with_retry(
        "POST", f"{ISSUER}/management/v1/projects/{project_id}/apps/oidc",
        headers=headers, json_body=body)
    if resp.status_code == 200:
        b = resp.json()
        return b["clientId"], b["clientSecret"]
    if resp.status_code == 409:
        raise SystemExit(
            "kabytech OIDC app already exists (409): clean-boot contract — run "
            "`docker compose down -v` AND delete ./secrets.")
    resp.raise_for_status()
    raise RuntimeError(f"create_kabytech_oidc_app unexpected status {resp.status_code}")
```

- [ ] **Step 4: Run to verify they pass**

Run: `cd deploy/compose/provisioner && python -m pytest test_provision.py -k kabytech -v`
Expected: PASS (2 passed).

- [ ] **Step 5: Wire into `main()`**

In `provision.py` `main()`, after the admin-api block writes its secrets (~after line 651), add:

```python
    # kabytech gateway OIDC client — its client_id/secret let kabytech run the
    # browser login and forward per-user tokens. Written to ./secrets for the
    # operator to hand to kabytech's gateway config.
    kaby_cid, kaby_secret = create_kabytech_oidc_app(token, headers, project_id)
    write_secret("kabytech_oidc_client_id", kaby_cid)
    write_secret("kabytech_oidc_client_secret", kaby_secret)
    print(f"[provision] kabytech gateway OIDC client_id={kaby_cid}")
```

- [ ] **Step 6: Commit**

```bash
git add deploy/compose/provisioner/provision.py deploy/compose/provisioner/test_provision.py
git commit -m "feat(provisioner): register kabytech gateway confidential OIDC web client"
```

---

### Task 3: Upstream-IdP federation runbook

The real upstream IdP (Google, or kabytech's own OIDC/SAML) needs environment-specific secrets that do not exist in this repo or CI, so it is **operator-run, not provisioner-automated**. This task documents the exact Zitadel steps to register it with JIT (auto-creation) enabled so the Task 1 action fires, and how to confirm it. Deliverable is documentation + an operator verification checklist.

**Files:**
- Modify: `deploy/zitadel/README.md` (append a "Federating kabytech's upstream IdP" section)

- [ ] **Step 1: Write the runbook section**

Append to `deploy/zitadel/README.md`:

````markdown
## Federating kabytech's upstream IdP (JIT) for end-user pass-through

End-users authenticate against an upstream IdP that Zitadel federates and
JIT-provisions; the Task-1 `grantChatUser` action then grants `chat.user`
automatically. Registering the IdP needs that IdP's real client id/secret, so
it is an operator step (not run by the provisioner).

### 1. Register the IdP (generic OIDC example)

Using a bootstrap IAM_OWNER management token (`$TOKEN`) and the platform org id
(`$ORG`), against `$ISSUER` (e.g. `http://host.docker.internal:8080`):

```bash
curl -sS -X POST "$ISSUER/management/v1/idps/generic_oidc" \
  -H "Authorization: Bearer $TOKEN" -H "x-zitadel-orgid: $ORG" \
  -H "Content-Type: application/json" -d '{
    "name": "kabytech-upstream",
    "issuer": "https://upstream-idp.example",
    "clientId": "<UPSTREAM_CLIENT_ID>",
    "clientSecret": "<UPSTREAM_CLIENT_SECRET>",
    "scopes": ["openid", "profile", "email"],
    "isAutoCreation": true,
    "isAutoUpdate": true,
    "autoLinkingOption": "AUTO_LINKING_OPTION_EMAIL"
  }'
```

- `isAutoCreation: true` is the **JIT** switch — Zitadel creates the local user
  on first login.
- For Google use `/idps/google`; for SAML use `/idps/saml`. The JIT/auto-create
  field is the same `isAutoCreation`.

### 2. Add the IdP to the login policy

```bash
curl -sS -X POST "$ISSUER/management/v1/policies/login/idps" \
  -H "Authorization: Bearer $TOKEN" -H "x-zitadel-orgid: $ORG" \
  -H "Content-Type: application/json" -d '{"idpId":"<IDP_ID_FROM_STEP_1>","ownerType":"IDP_OWNER_TYPE_ORG"}'
```

### 3. Confirm the auto-grant fires

Log in once through the upstream IdP (a fresh external identity), then check the
new Zitadel user has a `chat.user` grant on the chat project:

```bash
curl -sS -X POST "$ISSUER/management/v1/users/grants/_search" \
  -H "Authorization: Bearer $TOKEN" -H "x-zitadel-orgid: $ORG" \
  -H "Content-Type: application/json" \
  -d '{"queries":[{"roleKeyQuery":{"roleKey":"chat.user"}}]}' | python -m json.tool
```

Expected: the newly federated user appears with `roleKeys: ["chat.user"]`. If it
does not, the Task-1 action/trigger binding is wrong — re-check
`/management/v1/flows/FLOW_TYPE_EXTERNAL_AUTHENTICATION/trigger/TRIGGER_TYPE_POST_CREATION`.
````

- [ ] **Step 2: Verify the doc renders and links resolve**

Run: `python -c "import pathlib; t=pathlib.Path('deploy/zitadel/README.md').read_text(encoding='utf-8'); assert 'Federating kabytech' in t and 'isAutoCreation' in t and 'grantChatUser' in t; print('runbook present')"`
Expected: prints `runbook present`.

- [ ] **Step 3: Commit**

```bash
git add deploy/zitadel/README.md
git commit -m "docs(zitadel): runbook to federate kabytech's upstream IdP with JIT"
```

---

### Task 4: End-to-end verification harness (per-`sub` graphs + isolation)

Proves the platform behaviour the whole feature relies on: two **distinct** `sub`s each carrying `chat.user`, driven through the real manager `/chat`, produce **two distinct usage buckets** and **isolated claude sessions**. Uses two machine stand-in users (provisioned at test setup) in place of federated humans — same end-state, but automatable without a browser. Requires the live compose stack + the native worker (same prerequisites as `deploy/compose/test_e2e_admin.py`).

**Files:**
- Create: `deploy/compose/test_e2e_gateway.py`

**Interfaces:**
- Consumes: `clients/python/llm_chat/auth.py::{resolve_credentials, fetch_access_token, Credentials}`, `clients/python/llm_chat/protocol.py::ChatClient`, the provisioner helpers `mint_management_token`, `load_admin_key`, `fetch_org_id`, `mgmt_headers`, `create_machine_user`-style management calls. The manager `/control` `{"cmd":"usage"}` reply shape `{ok, users:[{userId, charsIn, charsOut, files, fileBytes, ...}], totals}`.
- Produces: nothing imported elsewhere; the test IS the deliverable.

- [ ] **Step 1: Write the harness (the test)**

Create `deploy/compose/test_e2e_gateway.py`:

```python
"""E2E: two distinct subs => two distinct usage buckets + isolated sessions.

Stand-ins: two machine users (each its own key + chat.user grant) play the role
of two federated end-users. The platform attributes + isolates per sub, so this
proves the pass-through goal without a browser-federation login (which can't run
in CI). Requires the live compose stack + native worker.

Run: python -m pytest deploy/compose/test_e2e_gateway.py -v -s
"""
import asyncio
import json
import os
import sys
import time

import pytest

sys.path.insert(0, os.path.join(os.path.dirname(__file__), "..", "..", "clients", "python"))
sys.path.insert(0, os.path.join(os.path.dirname(__file__), "provisioner"))

from llm_chat.auth import Credentials, fetch_access_token  # noqa: E402
from llm_chat.protocol import ChatClient  # noqa: E402
import provision  # noqa: E402

ISSUER = os.environ.get("ZITADEL_ISSUER", "http://host.docker.internal:8080")
MANAGER_CHAT = os.environ.get("MANAGER_WS", "ws://127.0.0.1:7777/chat")
MANAGER_CONTROL = os.environ.get("MANAGER_CONTROL_WS", "ws://127.0.0.1:7777/control")
PROJECT = open(os.path.join(os.path.dirname(__file__), "..", "..", "secrets", "project_id")).read().strip()


def _mgmt():
    admin = provision.load_admin_key()
    token = provision.mint_management_token(admin)
    org_id = provision.fetch_org_id(token)
    return token, provision.mgmt_headers(token, org_id)


def _provision_standin(token, headers, username):
    """Create (idempotently) a machine user with chat.user + a JSON key.
    Returns the key dict usable as llm_chat Credentials.key_file content."""
    # create_machine_user uses a fixed name; create a uniquely-named one here.
    resp = provision.request_with_retry(
        "POST", f"{ISSUER}/management/v1/users/machine", headers=headers,
        json_body={"userName": username, "name": username,
                   "accessTokenType": "ACCESS_TOKEN_TYPE_JWT"})
    if resp.status_code == 409:
        sresp = provision.request_with_retry(
            "POST", f"{ISSUER}/management/v1/users/_search", headers=headers,
            json_body={"queries": [{"userNameQuery": {"userName": username}}]})
        uid = sresp.json()["result"][0]["id"]
    else:
        uid = resp.json()["userId"]
    provision.grant_role(token, headers, uid, PROJECT)  # chat.user (idempotent 409)
    kresp = provision.request_with_retry(
        "POST", f"{ISSUER}/management/v1/users/{uid}/keys", headers=headers,
        json_body={"type": "KEY_TYPE_JSON"})
    key_b64 = kresp.json()["keyDetails"]
    return provision.decode_key_details(key_b64)


def _token_for(key_dict, tmp_path, name):
    key_path = os.path.join(tmp_path, f"{name}.json")
    with open(key_path, "w") as f:
        json.dump(key_dict, f)
    creds = Credentials(issuer=ISSUER, project=PROJECT, key_file=key_path)
    return fetch_access_token(creds)


async def _ask(token, text):
    async with ChatClient(MANAGER_CHAT, lambda: token) as c:
        return (await c.ask(text, timeout=150)).text


async def _control_usage(admin_token):
    import websockets
    async with websockets.connect(
        MANAGER_CONTROL, additional_headers=[("Authorization", f"Bearer {admin_token}")],
        max_size=None, open_timeout=15) as ws:
        await ws.send(json.dumps({"cmd": "usage"}))
        # Read frames until the usage reply arrives (tolerates an optional
        # greeting frame /control may or may not send before the reply).
        deadline = time.monotonic() + 15
        while time.monotonic() < deadline:
            msg = json.loads(await asyncio.wait_for(ws.recv(), timeout=15))
            if "users" in msg or msg.get("ok") is False:
                return msg
        raise AssertionError("no usage reply from /control within timeout")


@pytest.fixture(scope="module")
def standins(tmp_path_factory):
    token, headers = _mgmt()
    tmp = tmp_path_factory.mktemp("keys")
    a = _provision_standin(token, headers, "e2e-enduser-a")
    b = _provision_standin(token, headers, "e2e-enduser-b")
    return {
        "a_token": _token_for(a, tmp, "a"), "a_sub": a["userId"],
        "b_token": _token_for(b, tmp, "b"), "b_sub": b["userId"],
    }


def test_two_subs_get_two_distinct_usage_buckets(standins):
    asyncio.run(_ask(standins["a_token"], "Reply with exactly: AAA"))
    asyncio.run(_ask(standins["b_token"], "Reply with exactly: BBB"))
    admin_token = fetch_access_token(Credentials(
        issuer=ISSUER, project=PROJECT,
        key_file=os.path.join(os.path.dirname(__file__), "..", "..", "secrets", "admin-api-key.json")))
    usage = asyncio.run(_control_usage(admin_token))
    by_user = {u["userId"]: u for u in usage["users"]}
    assert standins["a_sub"] in by_user, "user A must have its own usage row"
    assert standins["b_sub"] in by_user, "user B must have its own usage row"
    assert by_user[standins["a_sub"]]["charsIn"] > 0
    assert by_user[standins["b_sub"]]["charsIn"] > 0


def test_sessions_are_isolated_between_subs(standins):
    # A plants a codeword in its claude session; B (separate sub => separate
    # session) must not see it.
    asyncio.run(_ask(standins["a_token"],
                     "Remember this codeword: BANANA. Reply with exactly: OK"))
    b_answer = asyncio.run(_ask(
        standins["b_token"],
        "What codeword did I give you earlier? If I gave none, reply exactly: NONE"))
    assert "BANANA" not in b_answer.upper(), (
        f"isolation breach: user B saw user A's codeword (answer={b_answer!r})")
```

- [ ] **Step 2: Bring up the stack + native worker, then run the harness**

Run (PowerShell): ensure compose is up (`docker compose up -d`) and the native worker listens on `:7878`, then:
`python -m pytest deploy/compose/test_e2e_gateway.py -v -s`
Expected: BOTH tests PASS — two distinct `userId` rows with `charsIn > 0`, and user B's answer does not contain `BANANA`.

- [ ] **Step 3: If either test fails, STOP and raise it**

A failure means the platform does **not** attribute or isolate per `sub` as the spec assumes — that is a platform gap, not a harness bug. Per the Global Constraints, do not patch the platform inside this plan; report the failure with the harness output.

- [ ] **Step 4: Commit**

```bash
git add deploy/compose/test_e2e_gateway.py
git commit -m "test(e2e): two subs get distinct usage buckets + isolated sessions (gateway pass-through)"
```

---

### Task 5: kabytech gateway integration contract

The contract kabytech's off-repo gateway implements: which OIDC client + scopes to use, how to forward per-user tokens (one WS per end-user), how to refresh over a long connection, and the fail-closed semantics. This is the hand-off document for kabytech's team.

**Files:**
- Create: `docs/integration/kabytech-gateway.md`

- [ ] **Step 1: Write the contract**

Create `docs/integration/kabytech-gateway.md`:

```markdown
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
```

- [ ] **Step 2: Verify the doc is present and complete**

Run: `python -c "import pathlib; t=pathlib.Path('docs/integration/kabytech-gateway.md').read_text(encoding='utf-8'); assert all(k in t for k in ['offline_access','one WS per end-user','handshake','403']); print('contract present')"`
Expected: prints `contract present`.

- [ ] **Step 3: Commit**

```bash
git add docs/integration/kabytech-gateway.md
git commit -m "docs(integration): kabytech gateway identity pass-through contract"
```

---

## Final verification (after all tasks)

1. **Provisioner unit tests green:** `cd deploy/compose/provisioner && python -m pytest test_provision.py -v` → all pass (existing + the 6 new pure tests).
2. **Clean re-provision is idempotent:** `docker compose down -v` + delete `./secrets`, then `docker compose up -d` → provisioner exits 0; `secrets/kabytech_oidc_client_id` exists; the `grantChatUser` action is bound to the External-Auth Post-Creation trigger.
3. **Platform behaviour proven:** `python -m pytest deploy/compose/test_e2e_gateway.py -v -s` → both tests pass (two distinct usage buckets + session isolation).
4. **Operator path documented:** `deploy/zitadel/README.md` federation runbook and `docs/integration/kabytech-gateway.md` contract are present.

A real upstream-IdP login + kabytech's gateway code are exercised by the operator
following Tasks 3 and 5 in kabytech's own environment — outside this repo's
automated tests, by design.
```
