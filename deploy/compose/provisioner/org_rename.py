#!/usr/bin/env python3
"""Rename the organization.

Renaming the org needs `org.write` (ORG_OWNER / IAM_OWNER). The runtime admin-api
SA is deliberately least-privilege (ORG_USER_MANAGER + per-project PROJECT_OWNER)
and CANNOT do it — so the Console shows the org name read-only and this one-off
performs the rename with the BOOTSTRAP IAM_OWNER key. (Same posture as new_app.py.)

Usage (from the repo root, stack up):

  docker compose run --rm -e ORG_NAME="My Company" \
    --entrypoint python zitadel-init /app/org_rename.py
"""
import os
import sys

import requests

import provision as P


def main() -> int:
    name = os.environ.get("ORG_NAME", "").strip()
    if not name:
        print("ORG_NAME is required (e.g. -e ORG_NAME=\"My Company\")", file=sys.stderr)
        return 2

    token = P.mint_management_token(P.load_admin_key())  # bootstrap IAM_OWNER
    org = P.fetch_org_id(token)
    headers = P.mgmt_headers(token, org)

    r = requests.put(f"{P.ISSUER}/management/v1/orgs/me", headers=headers,
                     json={"name": name}, timeout=15)
    if r.status_code == 400 and "not changed" in r.text:
        print(f"[org-rename] org is already named '{name}' — nothing to do")
        return 0
    r.raise_for_status()
    print(f"[org-rename] DONE — organization renamed to '{name}'")
    return 0


if __name__ == "__main__":
    sys.exit(main())
