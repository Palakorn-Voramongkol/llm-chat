import base64
import contextlib
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
        uid = provision.create_human_user(
            "tok", {}, "org-1",
            provision.CHATTER_USERNAME, "Chatter", "User",
            provision.CHATTER_EMAIL, provision.CHATTER_PASSWORD)
    assert uid == "user-xyz"
    assert captured["url"].endswith("/v2/users/human")   # v2 API: real password object
    b = captured["body"]
    assert b["username"] == provision.CHATTER_USERNAME
    assert b["email"]["isVerified"] is True
    assert b["password"]["changeRequired"] is False      # active immediately, no forced change
    assert b["password"]["password"] == provision.CHATTER_PASSWORD
    assert b["organization"]["orgId"] == "org-1"


def test_create_human_user_409_is_systemexit():
    with mock.patch.object(provision, "request_with_retry",
                           lambda *a, **k: _FakeResp(409)):
        with pytest.raises(SystemExit):
            provision.create_human_user(
                "tok", {}, "org-1", "chatter", "Chatter", "User",
                "chatter@example.com", "pw")


# ---------- opt-in Audit grant (instance-level IAM_OWNER_VIEWER) ----------

def test_grant_iam_viewer_posts_instance_member_without_org_header():
    captured = {}

    def fake_rwr(method, url, *, headers=None, json_body=None, **kw):
        captured.update(method=method, url=url, headers=headers, body=json_body)
        return _FakeResp(200)

    with mock.patch.object(provision, "request_with_retry", fake_rwr):
        provision.grant_iam_viewer("tok", "sa-9")
    assert captured["method"] == "POST"
    assert captured["url"].endswith("/admin/v1/members")        # instance-scoped admin API
    assert "x-zitadel-orgid" not in captured["headers"]         # NOT org-scoped
    assert captured["body"] == {"userId": "sa-9", "roles": ["IAM_OWNER_VIEWER"]}


def test_grant_iam_viewer_409_is_success():
    with mock.patch.object(provision, "request_with_retry",
                           lambda *a, **k: _FakeResp(409)):
        provision.grant_iam_viewer("tok", "sa-9")  # 409 == already a member, no raise


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


def test_create_admin_sa_posts_machine_user_jwt_token_type():
    captured = {}

    def fake_rwr(method, url, *, headers=None, json_body=None, **kw):
        captured["url"] = url
        captured["body"] = json_body
        return _FakeResp(200, {"userId": "sa-123"})

    with mock.patch.object(provision, "request_with_retry", fake_rwr):
        uid = provision.create_admin_sa("tok", {"h": "1"})
    assert uid == "sa-123"
    assert captured["url"].endswith("/management/v1/users/machine")
    b = captured["body"]
    assert b["userName"] == "chat-admin-api"
    assert b["name"] == "chat-admin-api"
    assert b["accessTokenType"] == "ACCESS_TOKEN_TYPE_JWT"


def test_create_admin_sa_409_is_systemexit_clean_boot():
    with mock.patch.object(provision, "request_with_retry",
                           lambda *a, **k: _FakeResp(409)):
        with pytest.raises(SystemExit):
            provision.create_admin_sa("tok", {})


def test_generate_admin_key_decodes_keydetails():
    sa = {"type": "serviceaccount", "keyId": "k9", "key": "PEM", "userId": "sa-123"}
    kd_b64 = base64.b64encode(json.dumps(sa).encode()).decode()

    def fake_rwr(method, url, *, headers=None, json_body=None, **kw):
        assert url.endswith("/management/v1/users/sa-123/keys")
        assert json_body == {"type": "KEY_TYPE_JSON"}
        return _FakeResp(200, {"keyId": "k9", "keyDetails": kd_b64})

    with mock.patch.object(provision, "request_with_retry", fake_rwr):
        out = provision.generate_admin_key("tok", {}, "sa-123")
    assert out == sa


def test_create_admin_oidc_app_posts_web_basic_with_role_assertion():
    captured = {}

    def fake_rwr(method, url, *, headers=None, json_body=None, **kw):
        captured["url"] = url
        captured["body"] = json_body
        return _FakeResp(200, {"clientId": "cid-1", "clientSecret": "shh-1"})

    with mock.patch.object(provision, "request_with_retry", fake_rwr):
        cid, secret = provision.create_admin_oidc_app("tok", {"h": "1"}, "proj-1")
    assert (cid, secret) == ("cid-1", "shh-1")
    assert captured["url"].endswith("/management/v1/projects/proj-1/apps/oidc")
    b = captured["body"]
    assert b["appType"] == "OIDC_APP_TYPE_WEB"
    assert b["authMethodType"] == "OIDC_AUTH_METHOD_TYPE_BASIC"
    assert b["accessTokenType"] == "OIDC_TOKEN_TYPE_JWT"
    assert b["accessTokenRoleAssertion"] is True
    assert b["idTokenRoleAssertion"] is True
    assert b["devMode"] is True
    assert "OIDC_GRANT_TYPE_AUTHORIZATION_CODE" in b["grantTypes"]
    assert "OIDC_GRANT_TYPE_REFRESH_TOKEN" in b["grantTypes"]
    assert b["responseTypes"] == ["OIDC_RESPONSE_TYPE_CODE"]
    assert provision.ADMIN_OIDC_REDIRECT_URI in b["redirectUris"]


def test_create_admin_oidc_app_409_is_systemexit():
    with mock.patch.object(provision, "request_with_retry",
                           lambda *a, **k: _FakeResp(409)):
        with pytest.raises(SystemExit):
            provision.create_admin_oidc_app("tok", {}, "p")


def test_assign_admin_member_posts_user_manager():
    captured = {}

    def fake_rwr(method, url, *, headers=None, json_body=None, **kw):
        captured["url"] = url
        captured["body"] = json_body
        return _FakeResp(200, {"details": {}})

    with mock.patch.object(provision, "request_with_retry", fake_rwr):
        provision.assign_admin_member("boot-tok", {"h": "1"}, "sa-123")
    assert captured["url"].endswith("/management/v1/orgs/me/members")
    b = captured["body"]
    assert b["userId"] == "sa-123"
    # least privilege: user manager + settings manager (org rename), NOT ORG_OWNER
    assert b["roles"] == ["ORG_USER_MANAGER", "ORG_SETTINGS_MANAGER"]
    assert "ORG_OWNER" not in b["roles"]


def test_assign_admin_project_member_posts_project_owner():
    captured = {}

    def fake_rwr(method, url, *, headers=None, json_body=None, **kw):
        captured["url"] = url
        captured["body"] = json_body
        return _FakeResp(200, {"details": {}})

    with mock.patch.object(provision, "request_with_retry", fake_rwr):
        provision.assign_admin_project_member(
            "boot-tok", {"h": "1"}, "proj-1", "sa-123")
    assert captured["url"].endswith("/management/v1/projects/proj-1/members")
    b = captured["body"]
    assert b["userId"] == "sa-123"
    assert b["roles"] == ["PROJECT_OWNER"]  # scoped to one project, not the org


def test_assign_admin_member_409_is_success():
    with mock.patch.object(provision, "request_with_retry",
                           lambda *a, **k: _FakeResp(409)):
        provision.assign_admin_member("boot-tok", {}, "sa-123")  # must NOT raise


# ---------- live update-member one-shot (already-provisioned instance, §5) ----------

def test_update_admin_member_puts_user_manager_to_member_path():
    captured = {}

    def fake_rwr(method, url, *, headers=None, json_body=None, **kw):
        captured["method"] = method
        captured["url"] = url
        captured["body"] = json_body
        return _FakeResp(200, {"details": {}})

    with mock.patch.object(provision, "request_with_retry", fake_rwr):
        provision.update_admin_member("boot-tok", {"h": "1"}, "sa-123")
    assert captured["method"] == "PUT"
    assert captured["url"].endswith("/management/v1/orgs/me/members/sa-123")
    assert captured["body"]["roles"] == ["ORG_USER_MANAGER", "ORG_SETTINGS_MANAGER"]


def test_update_admin_member_raises_on_hard_error():
    with mock.patch.object(provision, "request_with_retry",
                           lambda *a, **k: _FakeResp(400)):
        with pytest.raises(RuntimeError):
            provision.update_admin_member("boot-tok", {}, "sa-123")


# ---------- Task 7: end-to-end main() integration ----------

def test_main_provisions_admin_role_sa_app_and_writes_secrets(tmp_path):
    written = {}
    calls = []

    def fake_write_secret(name, content):
        written[name] = content

    # ExitStack (flat) instead of a deeply-nested `with A, B, C, ...:` — the
    # latter exceeds CPython's 20-statically-nested-block limit once every
    # create_* is mocked.
    with contextlib.ExitStack() as es:
        def p(name, new=mock.DEFAULT, **kw):
            es.enter_context(mock.patch.object(provision, name, new, **kw))
        p("load_admin_key", return_value={"userId": "boot", "keyId": "k", "key": "PEM"})
        p("mint_management_token", return_value="boot-tok")
        p("fetch_org_id", return_value="org-1")
        p("create_project", return_value="proj-1")
        p("add_role")
        p("create_machine_user", return_value="kaby-1")
        p("read_existing_user_id", return_value=None)
        p("generate_json_key", return_value={"userId": "kaby-1"})
        p("grant_role")
        p("create_grant_action", return_value="act-1")
        p("bind_post_creation_trigger",
          side_effect=lambda t, h, aid: calls.append(("trigger", aid)))
        p("create_oidc_app", return_value="cli-cid")
        p("create_human_user", return_value="demo-1")
        p("create_admin_role", side_effect=lambda *a, **k: calls.append("role"))
        p("create_admin_sa", return_value="sa-9")
        p("generate_admin_key", return_value={"userId": "sa-9", "keyId": "ak"})
        p("create_admin_oidc_app", return_value=("admin-cid", "admin-secret"))
        p("create_kabytech_oidc_app", return_value=("kaby-cid", "kaby-secret"))
        p("create_kaby_sa", return_value="kaby-sa-1")
        p("assign_kaby_org_member")
        p("assign_kaby_login_client")
        p("assign_admin_member",
          side_effect=lambda t, h, uid: calls.append(("member", uid)))
        p("assign_admin_project_member",
          side_effect=lambda t, h, pid, uid: calls.append(("proj-member", pid, uid)))
        p("write_secret", fake_write_secret)
        p("write_generated_env")
        rc = provision.main()

    assert rc == 0
    assert json.loads(written["admin-api-key.json"]) == {"userId": "sa-9", "keyId": "ak"}
    assert written["admin_api_user_id"] == "sa-9"
    assert written["admin_oidc_client_id"] == "admin-cid"
    assert written["admin_oidc_client_secret"] == "admin-secret"
    assert ("member", "sa-9") in calls
    assert "role" in calls
    # gateway pass-through (design 2026-06-22): auto-grant action bound to trigger
    assert ("trigger", "act-1") in calls
    assert written["kabytech_oidc_client_id"] == "kaby-cid"
    assert written["kabytech_oidc_client_secret"] == "kaby-secret"
    # identity UX Phase 1: kabytech-login SA key + id written
    assert json.loads(written["kabytech-login-key.json"]) == {"userId": "kaby-1"}
    assert written["kabytech_login_user_id"] == "kaby-sa-1"


# ---------- kabytech login SA + env SMTP (identity UX Phase 1) ----------

def test_create_kaby_sa_posts_machine_jwt():
    captured = {}

    def fake_rwr(method, url, *, headers=None, json_body=None, **kw):
        captured["url"] = url
        captured["body"] = json_body
        return _FakeResp(200, {"userId": "kaby-sa-1"})

    with mock.patch.object(provision, "request_with_retry", fake_rwr):
        uid = provision.create_kaby_sa("tok", {"h": "1"})
    assert uid == "kaby-sa-1"
    assert captured["url"].endswith("/management/v1/users/machine")
    assert captured["body"]["userName"] == provision.KABY_SA_USERNAME
    assert captured["body"]["accessTokenType"] == "ACCESS_TOKEN_TYPE_JWT"


def test_assign_kaby_org_member_posts_user_manager():
    captured = {}

    def fake_rwr(method, url, *, headers=None, json_body=None, **kw):
        captured["url"] = url
        captured["body"] = json_body
        return _FakeResp(200, {"details": {}})

    with mock.patch.object(provision, "request_with_retry", fake_rwr):
        provision.assign_kaby_org_member("tok", {"h": "1"}, "kaby-sa-1")
    assert captured["url"].endswith("/management/v1/orgs/me/members")
    assert captured["body"] == {"userId": "kaby-sa-1", "roles": ["ORG_USER_MANAGER"]}


def test_assign_kaby_login_client_posts_instance_member_no_org_header():
    captured = {}

    def fake_rwr(method, url, *, headers=None, json_body=None, **kw):
        captured.update(url=url, headers=headers, body=json_body)
        return _FakeResp(200)

    with mock.patch.object(provision, "request_with_retry", fake_rwr):
        provision.assign_kaby_login_client("tok", "kaby-sa-1")
    assert captured["url"].endswith("/admin/v1/members")
    assert "x-zitadel-orgid" not in captured["headers"]
    assert captured["body"] == {"userId": "kaby-sa-1", "roles": ["IAM_LOGIN_CLIENT"]}


def test_assign_kaby_login_client_409_is_success():
    with mock.patch.object(provision, "request_with_retry",
                           lambda *a, **k: _FakeResp(409)):
        provision.assign_kaby_login_client("tok", "kaby-sa-1")  # must NOT raise


# ---------- Gateway identity pass-through: auto-grant action + trigger (Task 1) ----------

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
    s = provision.build_grant_action_script("p", "chat.user")
    assert "chat.admin" not in s


def test_trigger_constants_are_external_auth_post_creation():
    assert provision.FLOW_TYPE_EXTERNAL_AUTHENTICATION == "FLOW_TYPE_EXTERNAL_AUTHENTICATION"
    assert provision.TRIGGER_TYPE_POST_CREATION == "TRIGGER_TYPE_POST_CREATION"


def test_create_grant_action_searches_then_posts_when_absent():
    calls = []

    def fake_rwr(method, url, *, headers=None, json_body=None, **kw):
        calls.append((url, json_body))
        if url.endswith("/actions/_search"):
            return _FakeResp(200, {"result": []})
        return _FakeResp(200, {"id": "act-77"})

    with mock.patch.object(provision, "request_with_retry", fake_rwr):
        aid = provision.create_grant_action("tok", {"h": "1"}, "proj-1")
    assert aid == "act-77"
    assert calls[0][0].endswith("/management/v1/actions/_search")
    assert calls[1][0].endswith("/management/v1/actions")
    assert calls[1][1]["name"] == provision.GRANT_ACTION_NAME


def test_create_grant_action_reuses_existing_by_name_idempotent():
    def fake_rwr(method, url, *, headers=None, json_body=None, **kw):
        if url.endswith("/actions/_search"):
            return _FakeResp(200, {"result": [
                {"id": "act-existing", "name": provision.GRANT_ACTION_NAME}]})
        raise AssertionError("must not POST a new action when one exists")

    with mock.patch.object(provision, "request_with_retry", fake_rwr):
        aid = provision.create_grant_action("tok", {}, "proj-1")
    assert aid == "act-existing"


def test_bind_post_creation_trigger_sets_actionids_on_external_auth_flow():
    captured = {}

    def fake_rwr(method, url, *, headers=None, json_body=None, **kw):
        captured["url"] = url
        captured["body"] = json_body
        return _FakeResp(200, {})

    with mock.patch.object(provision, "request_with_retry", fake_rwr):
        provision.bind_post_creation_trigger("tok", {"h": "1"}, "act-77")
    assert captured["url"].endswith(
        "/management/v1/flows/FLOW_TYPE_EXTERNAL_AUTHENTICATION"
        "/trigger/TRIGGER_TYPE_POST_CREATION")
    assert captured["body"] == {"actionIds": ["act-77"]}


# ---------- Gateway identity pass-through: kabytech OIDC web client (Task 2) ----------

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


def test_kabytech_oidc_app_uses_login_v2_with_base_uri():
    # Login V2 delegates the login UI to kabytech (custom /login page).
    b = provision.build_kabytech_oidc_app_body(["https://gw.example/callback"], [])
    assert b["loginVersion"]["loginV2"]["baseUri"] == provision.KABYTECH_LOGIN_BASE_URI
    assert provision.KABYTECH_LOGIN_BASE_URI == "http://localhost:3001"


def test_create_kabytech_oidc_app_posts_web_basic_with_role_assertion():
    captured = {}

    def fake_rwr(method, url, *, headers=None, json_body=None, **kw):
        captured["url"] = url
        captured["body"] = json_body
        return _FakeResp(200, {"clientId": "kc-1", "clientSecret": "ks-1"})

    with mock.patch.object(provision, "request_with_retry", fake_rwr):
        cid, secret = provision.create_kabytech_oidc_app("tok", {"h": "1"}, "proj-1")
    assert (cid, secret) == ("kc-1", "ks-1")
    assert captured["url"].endswith("/management/v1/projects/proj-1/apps/oidc")
    b = captured["body"]
    assert b["appType"] == "OIDC_APP_TYPE_WEB"
    assert b["accessTokenRoleAssertion"] is True


def test_create_kabytech_oidc_app_409_is_systemexit():
    with mock.patch.object(provision, "request_with_retry",
                           lambda *a, **k: _FakeResp(409)):
        with pytest.raises(SystemExit):
            provision.create_kabytech_oidc_app("tok", {}, "p")
