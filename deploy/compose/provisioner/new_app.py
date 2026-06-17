#!/usr/bin/env python3
"""Create a new application (a Zitadel project) and grant the runtime admin SA
PROJECT_OWNER on it, so the Console can fully manage it afterwards.

Least-privilege by design (spec §3, design 2026-06-18): the runtime SA is NEVER
given org-wide project rights (ORG_OWNER). Instead this one-off uses the
BOOTSTRAP IAM_OWNER key to create the project and make the SA the owner of just
that project — exactly how the home `llm-chat` project is provisioned. After
this runs, the operator manages the app's roles / login clients / user grants in
the Console (/applications/<id>) with no further privilege.

Usage (from the repo root, stack up):

  docker compose run --rm \
    -e APP_NAME=lumina \
    -e APP_ROLES=lumina.viewer,lumina.editor \
    --entrypoint python zitadel-init /app/new_app.py

APP_NAME is required; APP_ROLES is an optional comma-separated list of initial
roles. Idempotent-ish: a duplicate project name creates a second project (names
are not unique in Zitadel) — pick a fresh name.
"""
import os
import sys

import requests

import provision as P


def main() -> int:
    name = os.environ.get("APP_NAME", "").strip()
    if not name:
        print("APP_NAME is required (e.g. -e APP_NAME=lumina)", file=sys.stderr)
        return 2
    roles = [r.strip() for r in os.environ.get("APP_ROLES", "").split(",") if r.strip()]

    token = P.mint_management_token(P.load_admin_key())  # bootstrap IAM_OWNER
    org = P.fetch_org_id(token)
    headers = P.mgmt_headers(token, org)
    sa_id = open(os.path.join(P.SECRETS_DIR, "admin_api_user_id")).read().strip()

    # 1) Create the project (the application).
    r = requests.post(f"{P.ISSUER}/management/v1/projects", headers=headers,
                      json={"name": name}, timeout=15)
    r.raise_for_status()
    pid = r.json()["id"]
    print(f"[new-app] created '{name}' projectId={pid}")

    # 2) Make the runtime SA PROJECT_OWNER so the Console can manage it (same
    #    least-privilege grant as the home project; assign is idempotent on 409).
    P.assign_admin_project_member(token, headers, pid, sa_id)
    print(f"[new-app] granted SA {sa_id} PROJECT_OWNER on {pid}")

    # 3) Optional initial roles.
    for rk in roles:
        rr = requests.post(f"{P.ISSUER}/management/v1/projects/{pid}/roles",
                           headers=headers,
                           json={"roleKey": rk, "displayName": rk, "group": ""},
                           timeout=15)
        ok = rr.status_code in (200, 409)
        print(f"[new-app] role {rk}: {rr.status_code}{'' if ok else ' ' + rr.text[:120]}")

    print(f"[new-app] DONE — manage '{name}' in the Console at /applications/{pid}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
