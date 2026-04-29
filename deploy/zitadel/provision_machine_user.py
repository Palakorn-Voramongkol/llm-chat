#!/usr/bin/env python3
"""Provision a Zitadel org + machine user that can authenticate to the
llm-chat manager (the manager validates the user's `chat.user` role).

Creates five resources, all idempotent — re-running is safe:

  1. Organization              (e.g. "corridraw.com")
  2. Machine user              (e.g. "corridraw"), with accessTokenType=JWT
  3. JSON machine-user key     (saved to a file you specify)
  4. Project grant: the llm-chat project (owned by the kabytech.com org)
     is granted to the new org with the chat.user role.
  5. User grant: the new machine user gets the chat.user role on the
     granted project.

Why JWT and not the default? The manager verifies inbound access tokens
locally against Zitadel's JWKS — that only works if Zitadel issues real
JWTs. Default-machine-user `ACCESS_TOKEN_TYPE_BEARER` returns opaque/JWE
tokens that the manager cannot decode.

Usage:
    python3 provision_machine_user.py \\
        --org    corridraw.com \\
        --user   corridraw \\
        --out    ~/.config/llm-chat/corridraw-key.json
"""
import argparse
import base64
import json
import os
import sys
import time
import urllib.error
import urllib.request
import uuid

import jwt  # pyjwt


# ─── Defaults; usually no need to override unless this is a different deployment.
ZITADEL              = "https://id.palakorn.com"
SYS_AUDIENCE         = "http://id.palakorn.com:443"
SYS_USER             = "sysadmin"
SYS_KID              = "sysadmin"
PROJECT_ID           = "370627061150121985"        # llm-chat
PROJECT_OWNER_ORG_ID = "370627058616762369"        # kabytech.com (owns the project)
ROLE_KEY             = "chat.user"
SYSADMIN_KEY_CANDIDATES = [
    "/tmp/sysadmin.key.pem",
    "/root/.zitadel-bootstrap/sysadmin.key.pem",
]
# ───


def call(method, path, token, body=None, org_id=None, ignore_codes=None):
    """Call Zitadel; raise on HTTPError unless the response body's `code`
    field is in `ignore_codes` (e.g. {9} for `Errors.User.NotChanged`)."""
    headers = {"Authorization": f"Bearer {token}",
               "Content-Type":  "application/json"}
    if org_id:
        headers["x-zitadel-orgid"] = org_id
    data = json.dumps(body).encode() if body is not None else None
    req  = urllib.request.Request(f"{ZITADEL}{path}",
                                  data=data, method=method, headers=headers)
    try:
        with urllib.request.urlopen(req, timeout=20) as r:
            raw = r.read()
            return json.loads(raw) if raw else {}
    except urllib.error.HTTPError as e:
        body_text = e.read().decode(errors="replace")
        if ignore_codes:
            try:
                j = json.loads(body_text)
                if j.get("code") in ignore_codes or any(
                        d.get("id", "").startswith("COMMAND-") and
                        "NotChanged" in d.get("message", "")
                        for d in j.get("details", [])):
                    return {}
            except json.JSONDecodeError:
                pass
        raise SystemExit(f"{method} {path} -> {e.code}: {body_text}")


def find_sysadmin_key(override: str | None) -> str:
    if override:
        return override
    for p in SYSADMIN_KEY_CANDIDATES:
        if os.path.isfile(p):
            return p
    raise SystemExit(
        "no sysadmin key found in any of: "
        + ", ".join(SYSADMIN_KEY_CANDIDATES)
        + ". Pass --sysadmin-key /path/to/sysadmin.key.pem."
    )


def mint_sys_token(key_path: str) -> str:
    with open(key_path, "rb") as f:
        priv = f.read()
    now = int(time.time())
    return jwt.encode(
        {"iss": SYS_USER, "sub": SYS_USER, "aud": SYS_AUDIENCE,
         "iat": now, "exp": now + 3600, "jti": str(uuid.uuid4())},
        priv, algorithm="RS256", headers={"kid": SYS_KID})


def find_or_create_org(token, name):
    res = call("POST", "/admin/v1/orgs/_search", token,
               {"queries": [{"nameQuery":
                   {"name": name, "method": "TEXT_QUERY_METHOD_EQUALS"}}]})
    for o in res.get("result") or []:
        if o.get("name") == name:
            print(f"[ok ] org {name!r} already exists -> {o['id']}",
                  file=sys.stderr)
            return o["id"]
    print(f"[new] creating org {name!r}", file=sys.stderr)
    # /management/v1/orgs would refuse with "User could not be found" because
    # the system user (sysadmin) isn't a real user record. The v2beta org
    # endpoint accepts system-user callers.
    res = call("POST", "/v2beta/organizations", token, {"name": name})
    return res["id"]


def find_or_create_machine_user(token, org_id, username):
    res = call("POST", "/management/v1/users/_search", token,
               {"queries": [{"userNameQuery":
                   {"userName": username, "method": "TEXT_QUERY_METHOD_EQUALS"}}]},
               org_id=org_id)
    for u in res.get("result") or []:
        if u.get("userName") == username:
            print(f"[ok ] user {username!r} already exists -> {u['id']}",
                  file=sys.stderr)
            return u["id"]
    print(f"[new] creating machine user {username!r}", file=sys.stderr)
    res = call("POST", "/management/v1/users/machine", token, {
        "userName":    username,
        "name":        f"{username} machine user",
        "description": "API user for the llm-chat client",
        "accessTokenType": "ACCESS_TOKEN_TYPE_JWT",
    }, org_id=org_id)
    return res["userId"]


def ensure_jwt_token_type(token, org_id, user_id, username):
    """Force the user's accessTokenType to JWT. Idempotent — safe to run
    on a freshly-created user that's already JWT (the API just records the
    no-op as a new sequence)."""
    print(f"[ok ] ensuring user {username!r} accessTokenType=JWT",
          file=sys.stderr)
    # Zitadel rejects no-op PUTs with `Errors.User.NotChanged` (code 9).
    # Treat that as success: the field is already where we want it.
    call("PUT", f"/management/v1/users/{user_id}/machine", token,
         {"name":             f"{username} machine user",
          "description":      "API user for the llm-chat client",
          "accessTokenType":  "ACCESS_TOKEN_TYPE_JWT"},
         org_id=org_id, ignore_codes={9})


def mint_machine_key(token, org_id, user_id):
    """Add a fresh JSON key, then delete every other existing key on the
    user — so the file we write is always the only valid credential in
    Zitadel and re-running the script effectively rotates the key. The
    listing is eventually-consistent, so the just-minted key may not yet
    appear on the first list — guard against accidentally deleting it."""
    res = call("POST", f"/management/v1/users/{user_id}/keys", token,
               {"type": "KEY_TYPE_JSON"}, org_id=org_id)
    new_key_id = res["keyId"]
    print(f"[new] minted key {new_key_id}", file=sys.stderr)

    listing = call("POST", f"/management/v1/users/{user_id}/keys/_search",
                   token, {}, org_id=org_id)
    stale = [k["id"] for k in (listing.get("result") or [])
             if k["id"] != new_key_id]
    for kid in stale:
        call("DELETE", f"/management/v1/users/{user_id}/keys/{kid}",
             token, None, org_id=org_id)
        print(f"[ok ] rotated out stale key {kid}", file=sys.stderr)

    return new_key_id, base64.b64decode(res["keyDetails"])


def find_or_create_project_grant(token, owner_org_id, project_id,
                                 granted_org_id, role_keys):
    res = call("POST", f"/management/v1/projects/{project_id}/grants/_search",
               token, {}, org_id=owner_org_id)
    for g in res.get("result") or []:
        if g.get("grantedOrgId") == granted_org_id:
            gid = g.get("grantId") or g.get("id")
            print(f"[ok ] project grant exists -> {gid}", file=sys.stderr)
            return gid
    print(f"[new] creating project grant {project_id} -> {granted_org_id}",
          file=sys.stderr)
    res = call("POST", f"/management/v1/projects/{project_id}/grants", token,
               {"grantedOrgId": granted_org_id, "roleKeys": role_keys},
               org_id=owner_org_id)
    return res.get("grantId") or res.get("id")


def find_or_create_user_grant(token, org_id, user_id, project_id,
                              project_grant_id, role_keys):
    res = call("POST", "/management/v1/users/grants/_search", token,
               {"queries": [{"userIdQuery": {"userId": user_id}}]},
               org_id=org_id)
    for g in res.get("result") or []:
        if (g.get("projectId") == project_id
                and g.get("state") == "USER_GRANT_STATE_ACTIVE"):
            print(f"[ok ] user grant exists -> {g['id']} "
                  f"roles={g.get('roleKeys')}", file=sys.stderr)
            return g["id"]
    print(f"[new] creating user grant for {user_id} on project {project_id}",
          file=sys.stderr)
    body = {"projectId": project_id, "roleKeys": role_keys}
    if project_grant_id:
        body["projectGrantId"] = project_grant_id
    res = call("POST", f"/management/v1/users/{user_id}/grants",
               token, body, org_id=org_id)
    return res.get("userGrantId") or res.get("id")


def main():
    p = argparse.ArgumentParser(description=__doc__,
                                formatter_class=argparse.RawDescriptionHelpFormatter)
    p.add_argument("--org",  required=True,
                   help="Org name to create (e.g. 'corridraw.com').")
    p.add_argument("--user", required=True,
                   help="Machine user name (e.g. 'corridraw').")
    p.add_argument("--out",  required=True,
                   help="Where to write the JSON key (mode 0600).")
    p.add_argument("--sysadmin-key", default=None,
                   help=f"Path to the sysadmin private PEM. "
                        f"Default: first existing of {SYSADMIN_KEY_CANDIDATES}.")
    args = p.parse_args()

    out_path = os.path.expanduser(args.out)
    sys_key  = find_sysadmin_key(args.sysadmin_key)
    token    = mint_sys_token(sys_key)

    org_id  = find_or_create_org(token, args.org)
    user_id = find_or_create_machine_user(token, org_id, args.user)
    ensure_jwt_token_type(token, org_id, user_id, args.user)

    key_id, key_bytes = mint_machine_key(token, org_id, user_id)

    pg_id = find_or_create_project_grant(token, PROJECT_OWNER_ORG_ID,
                                         PROJECT_ID, org_id, [ROLE_KEY])
    ug_id = find_or_create_user_grant(token, org_id, user_id,
                                      PROJECT_ID, pg_id, [ROLE_KEY])

    os.makedirs(os.path.dirname(out_path) or ".", exist_ok=True)
    with open(out_path, "wb") as f:
        f.write(key_bytes)
    os.chmod(out_path, 0o600)
    parsed = json.loads(key_bytes)

    print()
    print("=== provisioning summary ===")
    print(f"  org           {args.org}    -> {org_id}")
    print(f"  user          {args.user}   -> {user_id}")
    print(f"  key id        {key_id}")
    print(f"  project_grant {pg_id} (llm-chat -> {args.org})")
    print(f"  user_grant    {ug_id}")
    print(f"  key file      {out_path} (mode 0600)")
    print(f"  type/keyId    {parsed.get('type')} / {parsed.get('keyId')}")


if __name__ == "__main__":
    main()
