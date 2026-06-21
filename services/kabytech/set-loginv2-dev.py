"""DEV: set the kabytech OIDC app to Login V2 (baseUri=frontend) on a running
stack via UpdateOIDCAppConfig, so authorize requests redirect to the custom
login page. Uses the bootstrap key. Run: python services/kabytech/set-loginv2-dev.py"""
import json, os, sys
os.environ["ADMIN_KEY_PATH"] = "secrets/_bootstrap-admin-sa.json"
os.environ.setdefault("PROVISION_ISSUER", "http://host.docker.internal:8080")
sys.path.insert(0, os.path.join("deploy", "compose", "provisioner"))
import provision  # noqa: E402

base_uri = os.environ.get("KABYTECH_LOGIN_BASE_URI", "http://localhost:3001")
admin = provision.load_admin_key()
token = provision.mint_management_token(admin)
org_id = provision.fetch_org_id(token)
h = provision.mgmt_headers(token, org_id)
project_id = open("secrets/project_id").read().strip()
client_id = open("secrets/kabytech_oidc_client_id").read().strip()

s = provision.request_with_retry(
    "POST", f"{provision.ISSUER}/management/v1/projects/{project_id}/apps/_search",
    headers=h, json_body={})
app = next(a for a in s.json()["result"]
           if a.get("oidcConfig", {}).get("clientId") == client_id)
app_id = app["id"]
cfg = app["oidcConfig"]
body = {
    "redirectUris": cfg.get("redirectUris", []),
    "postLogoutRedirectUris": cfg.get("postLogoutRedirectUris", []),
    "responseTypes": cfg.get("responseTypes", ["OIDC_RESPONSE_TYPE_CODE"]),
    "grantTypes": cfg.get("grantTypes", ["OIDC_GRANT_TYPE_AUTHORIZATION_CODE", "OIDC_GRANT_TYPE_REFRESH_TOKEN"]),
    "appType": cfg.get("appType", "OIDC_APP_TYPE_WEB"),
    "authMethodType": cfg.get("authMethodType", "OIDC_AUTH_METHOD_TYPE_BASIC"),
    "accessTokenType": cfg.get("accessTokenType", "OIDC_TOKEN_TYPE_JWT"),
    "accessTokenRoleAssertion": True,
    "idTokenRoleAssertion": True,
    "loginVersion": {"loginV2": {"baseUri": base_uri}},
}
r = provision.request_with_retry(
    "PUT", f"{provision.ISSUER}/management/v1/projects/{project_id}/apps/{app_id}/oidc_config",
    headers=h, json_body=body)
print("UpdateOIDCAppConfig", r.status_code, json.dumps(r.json())[:200])
