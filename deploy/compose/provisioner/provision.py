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
import secrets
import string
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
ADMIN_ROLE_KEY = "chat.admin"
MACHINE_USERNAME = "kabytech"
ADMIN_SA_USERNAME = "chat-admin-api"

# Interactive human-login path (OIDC Auth Code + PKCE). The OIDC public app the
# CLI logs in through, plus a ready demo human user with the same chat.user role.
OIDC_APP_NAME = "llm-chat-cli"
# RFC 8252 prefers the 127.0.0.1 loopback literal over "localhost" (which a
# hosts-file entry on a shared box could hijack). Default to 127.0.0.1 and
# register BOTH spellings (see _both_loopback_hosts) so either client form works.
OIDC_REDIRECT_URI = os.environ.get("OIDC_REDIRECT_URI", "http://127.0.0.1:8477/callback")
OIDC_POST_LOGOUT_URI = os.environ.get("OIDC_POST_LOGOUT_URI", "http://127.0.0.1:8477/")


def _both_loopback_hosts(uri: str) -> list:
    """Both 127.0.0.1 and localhost spellings of a loopback redirect URI, so a
    client using either host matches a registered redirect."""
    if "127.0.0.1" in uri:
        return [uri, uri.replace("127.0.0.1", "localhost")]
    if "localhost" in uri:
        return [uri, uri.replace("localhost", "127.0.0.1")]
    return [uri]
# loginName == username: the org has userLoginMustBeDomain=false (verified against
# the running instance — the demo user's loginNames were ["demo"]), so the login
# name is just "demo". The email is a separate field and is never itself a login
# name.
# Two browser-login humans (both sign in through the OIDC public app above):
#   chatter — a plain chat user (chat.user): can use /chat, NOT the Console.
#   admin   — the Console operator (chat.user + chat.admin): the only human who
#             can sign into admin-web (:3000).
CHATTER_USERNAME = "chatter"
CHATTER_EMAIL = "chatter@example.com"
ADMIN_USERNAME = "admin"
ADMIN_EMAIL = "admin@example.com"


def gen_demo_password() -> str:
    """A CSPRNG demo password satisfying Zitadel's default complexity policy
    (upper + lower + digit + symbol, min 8). Guarantees one char of each
    required class, fills the rest from a mixed alphabet, then CSPRNG-shuffles
    so class positions aren't fixed."""
    rng = secrets.SystemRandom()
    required = [
        secrets.choice(string.ascii_uppercase),
        secrets.choice(string.ascii_lowercase),
        secrets.choice(string.digits),
        secrets.choice("!@#$%^&*-_=+"),
    ]
    rest = [secrets.choice(string.ascii_letters + string.digits) for _ in range(16)]
    chars = required + rest
    rng.shuffle(chars)
    return "".join(chars)


# Human login passwords. NEVER ship a committed default: admin holds chat.admin,
# so a hardcoded value would be a publicly-known admin credential on every
# deployment. Override with $CHATTER_USER_PASSWORD / $ADMIN_USER_PASSWORD for
# fixed values; otherwise mint a fresh CSPRNG password each provision. The
# clean-boot contract (create_human_user 409 → hard fail + delete ./secrets)
# guarantees the minted value is what gets written to secrets/*, so it always
# matches the account that was created.
CHATTER_PASSWORD = os.environ.get("CHATTER_USER_PASSWORD") or gen_demo_password()
ADMIN_PASSWORD = os.environ.get("ADMIN_USER_PASSWORD") or gen_demo_password()

# admin-api OIDC WEB app (confidential server / BASIC + PKCE) — distinct from the
# CLI's public NATIVE app above. Captures BOTH clientId and clientSecret (once).
ADMIN_OIDC_APP_NAME = "chat-admin-api"
ADMIN_OIDC_REDIRECT_URI = os.environ.get(
    "ADMIN_OIDC_REDIRECT_URI", "http://localhost:3000/callback")
ADMIN_OIDC_POST_LOGOUT_URI = os.environ.get(
    "ADMIN_OIDC_POST_LOGOUT_URI", "http://localhost:3000/")
# Least privilege (security review, spec §3/§5): the runtime admin-api SA is a
# long-lived principal whose key is persisted to ./secrets, so it must NOT hold
# standing ORG_OWNER. At the org level it gets ORG_USER_MANAGER (users + grants)
# and ORG_SETTINGS_MANAGER (lets the Console rename the org — minimal role that
# grants org.write, verified; NOT ORG_OWNER), plus PROJECT_OWNER per project
# (apps + roles). Org POLICIES remain provisioner-managed (read-only in Console).
ADMIN_SA_ORG_ROLES = ["ORG_USER_MANAGER", "ORG_SETTINGS_MANAGER"]
ADMIN_SA_PROJECT_ROLE = "PROJECT_OWNER"

# Audit is OPT-IN (spec §3/§11). Reading the org event log needs the
# instance-level IAM_OWNER_VIEWER role — broader than the least-privilege org +
# project memberships above (it can read EVERY org on the instance; the Console
# still confines the Audit view to the SA's own org via resourceOwner). OFF by
# default; set $PROVISION_ENABLE_AUDIT=1 to grant it during provisioning so the
# Audit page works and SURVIVES a clean reset. The role is read-only (VIEWER).
ENABLE_AUDIT = os.environ.get("PROVISION_ENABLE_AUDIT", "").strip().lower() in (
    "1", "true", "yes", "on")
IAM_VIEWER_ROLE = "IAM_OWNER_VIEWER"

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


def create_machine_user(token: str, headers: dict) -> str:
    resp = request_with_retry(
        "POST", f"{ISSUER}/management/v1/users/machine", headers=headers,
        json_body={"userName": MACHINE_USERNAME, "name": MACHINE_USERNAME,
                   "description": "llm-chat reference client",
                   # JWT (not BEARER/opaque): the manager validates access
                   # tokens locally via JWKS, so Zitadel must issue self-
                   # contained JWTs. BEARER yields opaque 104-char tokens the
                   # manager cannot verify and rejects with 401.
                   "accessTokenType": "ACCESS_TOKEN_TYPE_JWT"},
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


def generate_json_key(token: str, headers: dict, user_id: str) -> dict:
    resp = request_with_retry(
        "POST", f"{ISSUER}/management/v1/users/{user_id}/keys", headers=headers,
        json_body={"type": "KEY_TYPE_JSON"},
    )
    resp.raise_for_status()
    return decode_key_details(resp.json()["keyDetails"])


def generate_admin_key(token: str, headers: dict, user_id: str) -> dict:
    """Mint the admin SA's JSON key; keyDetails (base64 serviceaccount JSON) is
    returned ONCE (appendix §2.2). Same shape as generate_json_key for kabytech."""
    resp = request_with_retry(
        "POST", f"{ISSUER}/management/v1/users/{user_id}/keys", headers=headers,
        json_body={"type": "KEY_TYPE_JSON"},
    )
    resp.raise_for_status()
    return decode_key_details(resp.json()["keyDetails"])


def grant_role(token: str, headers: dict, user_id: str, project_id: str,
               role_keys=None) -> None:
    keys = role_keys if role_keys is not None else [ROLE_KEY]
    resp = request_with_retry(
        "POST", f"{ISSUER}/management/v1/users/{user_id}/grants", headers=headers,
        json_body={"projectId": project_id, "roleKeys": keys},
    )
    if not is_success(resp.status_code):
        resp.raise_for_status()


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


def create_oidc_app(token: str, headers: dict, project_id: str) -> str:
    """Register the public OIDC native app the interactive CLI logs in through.

    NATIVE + auth method NONE = a public client using PKCE (no client secret).
    accessTokenType JWT so the manager validates tokens locally via JWKS — a
    BEARER/opaque token would be rejected 401 (same lesson as the machine user).
    devMode allows the http loopback redirect in this local-dev stack.
    """
    resp = request_with_retry(
        "POST", f"{ISSUER}/management/v1/projects/{project_id}/apps/oidc",
        headers=headers,
        json_body={
            "name": OIDC_APP_NAME,
            "redirectUris": _both_loopback_hosts(OIDC_REDIRECT_URI),
            "postLogoutRedirectUris": _both_loopback_hosts(OIDC_POST_LOGOUT_URI),
            "responseTypes": ["OIDC_RESPONSE_TYPE_CODE"],
            "grantTypes": ["OIDC_GRANT_TYPE_AUTHORIZATION_CODE",
                           "OIDC_GRANT_TYPE_REFRESH_TOKEN"],
            "appType": "OIDC_APP_TYPE_NATIVE",
            "authMethodType": "OIDC_AUTH_METHOD_TYPE_NONE",
            "accessTokenType": "OIDC_TOKEN_TYPE_JWT",
            "devMode": True,
            "accessTokenRoleAssertion": True,
            "idTokenRoleAssertion": True,
        },
    )
    if resp.status_code == 200:
        return resp.json()["clientId"]
    if resp.status_code == 409:
        raise SystemExit(
            "OIDC app already exists (409): clean-boot contract — run "
            "`docker compose down -v` AND delete ./secrets.")
    resp.raise_for_status()
    raise RuntimeError(f"create_oidc_app unexpected status {resp.status_code}")


def create_admin_oidc_app(token: str, headers: dict, project_id: str):
    """Register the admin-api's confidential OIDC WEB app (appendix §1.2).

    Differs from create_oidc_app (the CLI's public NATIVE client): WEB +
    BASIC yields a client_secret (combined with PKCE at runtime). The app
    enum is OIDC_TOKEN_TYPE_JWT — NOT the machine ACCESS_TOKEN_TYPE_JWT (§7
    enum trap). accessTokenRoleAssertion=true so chat.admin rides in the
    ACCESS-token JWT even though the project has projectRoleAssertion=false
    (§6.1 gate). redirectUris uses the same-origin proxy / web origin
    (http://localhost:3000/callback == ADMIN_PUBLIC_ORIGIN): admin-web proxies
    the OIDC nav to the BFF, so the redirect must land on :3000 (the BFF's own
    :7676 is never browser-facing — going there drops the :3000 pre-auth
    cookie). Returns (clientId, clientSecret); the secret is shown ONCE.
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
    """Add the admin SA as an ORG member with ORG_USER_MANAGER — users + grants
    only, NOT org ownership (least privilege; spec §3/§5). MUST be called with
    the BOOTSTRAP IAM_OWNER token (needs org.member.write) — NOT the new SA
    itself. orgs/me resolves the org from the calling token / x-zitadel-orgid.
    Idempotent: 409 == already a member. Project-level powers (apps/roles) come
    from assign_admin_project_member (PROJECT_OWNER), not from org ownership."""
    resp = request_with_retry(
        "POST", f"{ISSUER}/management/v1/orgs/me/members", headers=headers,
        json_body={"userId": sa_user_id, "roles": ADMIN_SA_ORG_ROLES},
    )
    if not is_success(resp.status_code):
        resp.raise_for_status()


def grant_iam_viewer(token: str, sa_user_id: str) -> None:
    """Add the admin SA as an INSTANCE member with IAM_OWNER_VIEWER (read-only),
    enabling the Console's Audit page (spec §11). This is the opt-in audit grant
    — only runs when $PROVISION_ENABLE_AUDIT is set. Instance-scoped admin API:
    NO x-zitadel-orgid header (unlike the org/project member calls). MUST use the
    BOOTSTRAP IAM_OWNER token. Idempotent: 409 == already a member."""
    resp = request_with_retry(
        "POST", f"{ISSUER}/admin/v1/members",
        headers={"Authorization": f"Bearer {token}",
                 "Content-Type": "application/json"},
        json_body={"userId": sa_user_id, "roles": [IAM_VIEWER_ROLE]},
    )
    if not is_success(resp.status_code):
        resp.raise_for_status()


def assign_admin_project_member(token: str, headers: dict, project_id: str,
                                sa_user_id: str) -> None:
    """Add the admin SA as a PROJECT member with PROJECT_OWNER on the llm-chat
    project (spec §3/§5). This is what lets the runtime admin-api manage this
    project's apps and roles — scoped to ONE project, not the whole org.
    Idempotent: 409 == already a project member. MUST be called with the
    BOOTSTRAP IAM_OWNER token (needs project.member.write)."""
    resp = request_with_retry(
        "POST", f"{ISSUER}/management/v1/projects/{project_id}/members",
        headers=headers,
        json_body={"userId": sa_user_id, "roles": [ADMIN_SA_PROJECT_ROLE]},
    )
    if not is_success(resp.status_code):
        resp.raise_for_status()


def update_admin_member(token: str, headers: dict, sa_user_id: str) -> None:
    """Idempotently set an EXISTING org member roles to ADMIN_SA_ORG_ROLES
    (ORG_USER_MANAGER). On the already-provisioned instance the SA is already an
    ORG_USER_MANAGER member, so this is a no-op there; it exists so a re-run can
    correct the org role if it ever drifted. The NEW least-privilege capability a
    live instance needs is the PROJECT member grant — apply
    assign_admin_project_member live with the bootstrap token. MUST use the
    BOOTSTRAP IAM_OWNER token (org.member.write)."""
    resp = request_with_retry(
        "PUT", f"{ISSUER}/management/v1/orgs/me/members/{sa_user_id}",
        headers=headers, json_body={"roles": ADMIN_SA_ORG_ROLES},
    )
    if not is_success(resp.status_code):
        resp.raise_for_status()


def create_human_user(
    token: str, headers: dict, org_id,
    username: str, given: str, family: str, email: str, password: str,
) -> str:
    """Create a human user via the v2 user API.

    The v1 management /users/human endpoint silently ignored the password and
    left the user in the "initial" state (Zitadel's "Activate User / set your
    password" screen blocks login). The v2 endpoint takes a real password object
    with changeRequired=False, and a verified email, so the user is immediately
    active and can sign in with the known password.
    """
    body = {
        "username": username,
        "profile": {"givenName": given, "familyName": family},
        "email": {"email": email, "isVerified": True},
        "password": {"password": password, "changeRequired": False},
    }
    if org_id:
        body["organization"] = {"orgId": org_id}
    resp = request_with_retry(
        "POST", f"{ISSUER}/v2/users/human", headers=headers, json_body=body,
    )
    if resp.status_code in (200, 201):
        return resp.json()["userId"]
    if resp.status_code == 409:
        raise SystemExit(
            f"{username} user already exists (409): clean-boot contract — run "
            "`docker compose down -v` AND delete ./secrets.")
    resp.raise_for_status()
    raise RuntimeError(f"create_human_user unexpected status {resp.status_code}")


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

    # Auto-grant chat.user to every externally-federated (JIT) user, so
    # kabytech's end-users reach the manager with chat.user on their first token.
    grant_action_id = create_grant_action(token, headers, project_id)
    bind_post_creation_trigger(token, headers, grant_action_id)
    print(f"[provision] external-auth post-creation auto-grant bound "
          f"action_id={grant_action_id} role={ROLE_KEY}")

    # Interactive human-login path: an OIDC public app (PKCE) + two human users.
    # The kabytech machine path above is for M2M callers; this is for people
    # logging in through the browser.
    client_id = create_oidc_app(token, headers, project_id)
    write_secret("oidc_client_id", client_id)

    # ----- admin-api provisioning (appendix §2, §1.2) -----
    # Reuses the same bootstrap IAM_OWNER token/headers minted above:
    # assign_admin_member NEEDS org.member.write (§2.4), which the runtime
    # least-privilege SA will not have. Role creation stays here so the
    # runtime SA needs no project.role.write. The chat.admin role must exist
    # before we grant it (to the admin human and the runtime SA), so create it
    # up front.
    create_admin_role(token, headers, project_id)

    # chatter — a plain chat user (chat.user only): can use /chat, NOT the Console.
    chatter_id = create_human_user(
        token, headers, org_id,
        CHATTER_USERNAME, "Chatter", "User", CHATTER_EMAIL, CHATTER_PASSWORD)
    grant_role(token, headers, chatter_id, project_id)  # chat.user
    write_secret("chatter_user", CHATTER_USERNAME)
    write_secret("chatter_password", CHATTER_PASSWORD)

    # admin — the Console operator: chat.user lets it use /chat, chat.admin lets
    # it sign into admin-web (:3000). One grant carries both roles.
    admin_human_id = create_human_user(
        token, headers, org_id,
        ADMIN_USERNAME, "Console", "Admin", ADMIN_EMAIL, ADMIN_PASSWORD)
    grant_role(token, headers, admin_human_id, project_id,
               role_keys=[ROLE_KEY, ADMIN_ROLE_KEY])
    write_secret("admin_user", ADMIN_USERNAME)
    write_secret("admin_password", ADMIN_PASSWORD)

    admin_sa_id = create_admin_sa(token, headers)
    admin_sa = generate_admin_key(token, headers, admin_sa_id)
    assign_admin_member(token, headers, admin_sa_id)
    assign_admin_project_member(token, headers, project_id, admin_sa_id)
    # The Console's Sessions page reads the manager's /control (chat.admin-only
    # ops surface; chat.user is the manager's handshake gate) — grant the SA
    # both project roles in its one user-grant.
    grant_role(token, headers, admin_sa_id, project_id,
               role_keys=[ROLE_KEY, ADMIN_ROLE_KEY])
    admin_cid, admin_secret = create_admin_oidc_app(token, headers, project_id)
    write_secret("admin-api-key.json", json.dumps(admin_sa))
    write_secret("admin_api_user_id", admin_sa_id)
    write_secret("admin_oidc_client_id", admin_cid)
    write_secret("admin_oidc_client_secret", admin_secret)
    print(f"[provision] admin: sa_user_id={admin_sa_id} "
          f"admin_oidc_client_id={admin_cid} "
          f"org_roles={ADMIN_SA_ORG_ROLES} project_role={ADMIN_SA_PROJECT_ROLE}")

    # Opt-in Audit (spec §11): grant the SA the instance-level IAM_OWNER_VIEWER
    # so the Console's Audit page works and survives a clean reset. Off unless
    # $PROVISION_ENABLE_AUDIT is set — keeps the SA least-privilege by default.
    if ENABLE_AUDIT:
        grant_iam_viewer(token, admin_sa_id)
        print(f"[provision] audit ENABLED: granted {IAM_VIEWER_ROLE} to SA {admin_sa_id}")
    else:
        print("[provision] audit disabled (default) — set PROVISION_ENABLE_AUDIT=1 to enable")

    write_secret("issuer", ISSUER)
    write_secret("project_id", project_id)
    write_secret("kabytech_user_id", user_id)
    write_generated_env(project_id)
    print(f"[provision] done: project_id={project_id} kabytech_id={user_id} "
          f"oidc_client_id={client_id} chatter={CHATTER_USERNAME} "
          f"admin={ADMIN_USERNAME} admin_human_id={admin_human_id} "
          f"admin_sa_id={admin_sa_id}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
