"""One-off DEV helper: provision the kabytech-login SA + Zitadel SMTP into an
ALREADY-running stack (avoids a destructive `docker compose down -v`).

Uses the bootstrap IAM_OWNER key (extracted from the machinekey volume to
secrets/_bootstrap-admin-sa.json) to create the SA with ORG_USER_MANAGER +
IAM_LOGIN_CLIENT, write its key to secrets/, and configure SMTP -> MailHog.
The canonical path is a clean reprovision; this is the surgical equivalent.

Run from the repo root:  python services/kabytech/provision-identity-dev.py
Delete secrets/_bootstrap-admin-sa.json afterwards (it is all-powerful).
"""
import json
import os
import sys

os.environ["ADMIN_KEY_PATH"] = "secrets/_bootstrap-admin-sa.json"
os.environ.setdefault("PROVISION_ISSUER", "http://host.docker.internal:8080")

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)),
                                "..", "..", "deploy", "compose", "provisioner"))
import provision  # noqa: E402

admin = provision.load_admin_key()
token = provision.mint_management_token(admin)
org_id = provision.fetch_org_id(token)
headers = provision.mgmt_headers(token, org_id)

uid = provision.create_kaby_sa(token, headers)
key = provision.generate_json_key(token, headers, uid)
provision.assign_kaby_org_member(token, headers, uid)
provision.assign_kaby_login_client(token, uid)
with open("secrets/kabytech-login-key.json", "w", encoding="utf-8") as f:
    f.write(json.dumps(key))
with open("secrets/kabytech_login_user_id", "w", encoding="utf-8") as f:
    f.write(uid)

smtp_env = {"KABY_SMTP_HOST": "mailhog", "KABY_SMTP_PORT": "1025",
            "KABY_SMTP_TLS": "false", "KABY_SMTP_USER": "", "KABY_SMTP_PASSWORD": "",
            "KABY_SMTP_SENDER_ADDRESS": "noreply@kabytech.local",
            "KABY_SMTP_SENDER_NAME": "kabytech"}
provision.configure_smtp(token, smtp_env)
print(f"kabytech-login SA id={uid} (ORG_USER_MANAGER + IAM_LOGIN_CLIENT) + SMTP -> mailhog configured")
