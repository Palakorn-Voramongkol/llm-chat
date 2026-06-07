import os

import pytest

import verify_admin_gate as gate


def test_roles_claim_key_uses_project_id():
    assert gate.roles_claim_key("proj-1") == \
        "urn:zitadel:iam:org:project:proj-1:roles"


def test_has_admin_role_true_when_role_present():
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
    assert gate.issuer_matches(iss + "/", iss) is False


@pytest.mark.skipif(os.environ.get("ADMIN_IT") != "1",
                    reason="integration gate — set ADMIN_IT=1 against running Zitadel v3.4.10")
def test_integration_admin_gate_runs():
    report = gate.run_gate()
    assert report["issuer_match"] is True, report
    assert report["sa_can_mint_key"] is True, report
    assert report["human_has_admin_role"] is True, (
        "human access token lacks chat.admin under the project roles claim; "
        "repair: set app accessTokenRoleAssertion=true and/or flip project "
        "projectRoleCheck/hasProjectCheck (appendix §6.1/§6.5). Report: "
        + repr(report))
