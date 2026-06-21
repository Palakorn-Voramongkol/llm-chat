"""One-off DEV helper: register the kabytech-gateway OIDC client in the RUNNING
Zitadel with the local-dev redirect URI (http://localhost:3001/callback), and
write its id/secret to ./secrets so kabytech-backend can read them.

Non-destructive (no `docker compose down -v`). Use this when the running stack
was provisioned before the kabytech client existed. A clean boot
(`down -v` + reprovision with KABYTECH_OIDC_REDIRECT_URI set) is the canonical
path; this is the surgical equivalent for an already-running dev stack.

Run from the repo root:  python services/kabytech/register-dev-client.py
"""
import json
import os
import sys

os.environ["KABYTECH_OIDC_REDIRECT_URI"] = "http://localhost:3001/callback"
os.environ["KABYTECH_OIDC_POST_LOGOUT_URI"] = "http://localhost:3001/"

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)),
                                "..", "..", "deploy", "compose", "provisioner"))
import provision  # noqa: E402

admin = json.load(open("secrets/admin-api-key.json", encoding="utf-8"))
token = provision.mint_management_token(admin)
org_id = provision.fetch_org_id(token)
headers = provision.mgmt_headers(token, org_id)
project_id = open("secrets/project_id", encoding="utf-8").read().strip()

cid, secret = provision.create_kabytech_oidc_app(token, headers, project_id)
with open("secrets/kabytech_oidc_client_id", "w", encoding="utf-8") as f:
    f.write(cid)
with open("secrets/kabytech_oidc_client_secret", "w", encoding="utf-8") as f:
    f.write(secret)
print(f"registered kabytech-gateway client_id={cid} "
      f"redirect=http://localhost:3001/callback")
