import base64
import json
import time
from unittest import mock

import pytest

import provision


def test_decode_key_details_returns_serviceaccount_dict():
    sa = {"type": "serviceaccount", "keyId": "k1", "key": "-----PEM-----",
          "userId": "u1"}
    key_details_b64 = base64.b64encode(json.dumps(sa).encode()).decode()
    assert provision.decode_key_details(key_details_b64) == sa


def test_should_skip_keygen_when_userid_matches():
    assert provision.should_skip_keygen(existing_user_id="u1", current_user_id="u1") is True


def test_should_regenerate_when_userid_mismatch():
    assert provision.should_skip_keygen(existing_user_id="uOLD", current_user_id="uNEW") is False


def test_should_regenerate_when_no_existing_key():
    assert provision.should_skip_keygen(existing_user_id=None, current_user_id="u1") is False


@pytest.mark.parametrize("status", [500, 502, 503, 401, 403])
def test_retry_predicate_retries_on_transient(status):
    assert provision.should_retry(status=status, attempt=0) is True


def test_retry_predicate_retries_on_connection_error():
    assert provision.should_retry(status=None, attempt=0) is True


@pytest.mark.parametrize("status", [409, 400, 404])
def test_retry_predicate_does_not_retry_on_deterministic(status):
    assert provision.should_retry(status=status, attempt=0) is False


def test_retry_predicate_stops_401_after_initial_window():
    # 401/403 only retried during the initial window (attempt < INITIAL_AUTH_RETRY_ATTEMPTS)
    assert provision.should_retry(status=401, attempt=provision.INITIAL_AUTH_RETRY_ATTEMPTS) is False


def test_build_jwt_assertion_header_and_claims():
    admin = {"type": "serviceaccount", "keyId": "kid-123", "userId": "user-456",
             "key": "PEM"}
    issuer = "http://host.docker.internal:8080"
    with mock.patch.object(provision.pyjwt, "encode") as enc:
        enc.return_value = "signed"
        out = provision.build_jwt_assertion(admin, issuer, now=1000)
    assert out == "signed"
    claims, key = enc.call_args.args[0], enc.call_args.args[1]
    assert claims["iss"] == "user-456"
    assert claims["sub"] == "user-456"
    assert claims["aud"] == issuer
    assert claims["iat"] == 1000
    assert claims["exp"] == 1000 + 3600
    assert key == "PEM"
    assert enc.call_args.kwargs["algorithm"] == "RS256"
    assert enc.call_args.kwargs["headers"] == {"kid": "kid-123"}


def test_is_success_treats_409_as_already_provisioned():
    assert provision.is_success(200) is True
    assert provision.is_success(409) is True
    assert provision.is_success(400) is False
    assert provision.is_success(500) is False


def test_write_generated_env_writes_project_id_and_equal_audience(tmp_path):
    out = tmp_path / "sub" / "manager.generated.env"
    with mock.patch.object(provision, "OUT_ENV_PATH", str(out)):
        provision.write_generated_env("PROJ-999")
    # §10.4: both keys defined, equal, and non-empty; exact two-line content.
    assert out.read_text() == "ZITADEL_PROJECT_ID=PROJ-999\nZITADEL_AUDIENCE=PROJ-999\n"


# ---------- OIDC app + demo human user (human-login path) ----------

class _FakeResp:
    def __init__(self, status_code, body=None):
        self.status_code = status_code
        self._body = body or {}

    def json(self):
        return self._body

    def raise_for_status(self):
        if self.status_code >= 400:
            raise RuntimeError(f"HTTP {self.status_code}")


def test_create_oidc_app_posts_native_pkce_jwt():
    captured = {}

    def fake_rwr(method, url, *, headers=None, json_body=None, **kw):
        captured["url"] = url
        captured["body"] = json_body
        return _FakeResp(200, {"clientId": "client-abc"})

    with mock.patch.object(provision, "request_with_retry", fake_rwr):
        cid = provision.create_oidc_app("tok", {"h": "1"}, "proj-1")
    assert cid == "client-abc"
    assert captured["url"].endswith("/management/v1/projects/proj-1/apps/oidc")
    b = captured["body"]
    assert b["appType"] == "OIDC_APP_TYPE_NATIVE"
    assert b["authMethodType"] == "OIDC_AUTH_METHOD_TYPE_NONE"   # public client / PKCE
    assert b["accessTokenType"] == "OIDC_TOKEN_TYPE_JWT"         # JWKS-validatable
    assert provision.OIDC_REDIRECT_URI in b["redirectUris"]
    assert "OIDC_GRANT_TYPE_REFRESH_TOKEN" in b["grantTypes"]


def test_create_oidc_app_409_is_systemexit():
    with mock.patch.object(provision, "request_with_retry",
                           lambda *a, **k: _FakeResp(409)):
        with pytest.raises(SystemExit):
            provision.create_oidc_app("tok", {}, "p")


def test_create_human_user_posts_verified_permanent_password():
    captured = {}

    def fake_rwr(method, url, *, headers=None, json_body=None, **kw):
        captured["url"] = url
        captured["body"] = json_body
        return _FakeResp(200, {"userId": "user-xyz"})

    with mock.patch.object(provision, "request_with_retry", fake_rwr):
        uid = provision.create_human_user("tok", {}, "org-1")
    assert uid == "user-xyz"
    assert captured["url"].endswith("/v2/users/human")   # v2 API: real password object
    b = captured["body"]
    assert b["username"] == provision.DEMO_USERNAME
    assert b["email"]["isVerified"] is True
    assert b["password"]["changeRequired"] is False      # active immediately, no forced change
    assert b["password"]["password"] == provision.DEMO_PASSWORD
    assert b["organization"]["orgId"] == "org-1"


def test_create_human_user_409_is_systemexit():
    with mock.patch.object(provision, "request_with_retry",
                           lambda *a, **k: _FakeResp(409)):
        with pytest.raises(SystemExit):
            provision.create_human_user("tok", {}, "org-1")


# ---------- chat.admin role + admin SA + admin OIDC WEB app (admin-api path) ----------

def test_create_admin_role_posts_chat_admin_rolekey():
    captured = {}

    def fake_rwr(method, url, *, headers=None, json_body=None, **kw):
        captured["method"] = method
        captured["url"] = url
        captured["body"] = json_body
        return _FakeResp(200, {"details": {}})

    with mock.patch.object(provision, "request_with_retry", fake_rwr):
        provision.create_admin_role("tok", {"h": "1"}, "proj-1")
    assert captured["method"] == "POST"
    assert captured["url"].endswith("/management/v1/projects/proj-1/roles")
    b = captured["body"]
    assert b["roleKey"] == "chat.admin"
    assert b["displayName"] == "Chat Admin"


def test_create_admin_role_409_is_success_not_systemexit():
    with mock.patch.object(provision, "request_with_retry",
                           lambda *a, **k: _FakeResp(409)):
        provision.create_admin_role("tok", {}, "p")  # must NOT raise


def test_create_admin_role_raises_on_hard_error():
    with mock.patch.object(provision, "request_with_retry",
                           lambda *a, **k: _FakeResp(400)):
        with pytest.raises(RuntimeError):
            provision.create_admin_role("tok", {}, "p")
