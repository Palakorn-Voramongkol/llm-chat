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

import provision  # reuse ISSUER, mint/header helpers, etc.


# ---------- pure helpers (unit-tested) ----------

def roles_claim_key(project_id: str) -> str:
    """The Zitadel roles claim name for a project (appendix §1.5)."""
    return f"urn:zitadel:iam:org:project:{project_id}:roles"


def has_admin_role(claims: dict, project_id: str) -> bool:
    """True iff the JWT claims carry chat.admin under the project roles claim.
    The claim is an object whose KEYS are role keys (appendix §1.5)."""
    roles = claims.get(roles_claim_key(project_id))
    return isinstance(roles, dict) and provision.ADMIN_ROLE_KEY in roles


def issuer_matches(discovery_iss: str, configured: str) -> bool:
    """Byte-for-byte issuer compare (design §8, §6.7)."""
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
    """Run all three checks; return a report dict. §6.1 requires an interactive
    auth-code login token, read from env ADMIN_GATE_HUMAN_ACCESS_TOKEN when
    available; absent that, reported None. Decoding is signature-unverified here
    (claim inspection only)."""
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
