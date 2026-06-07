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
