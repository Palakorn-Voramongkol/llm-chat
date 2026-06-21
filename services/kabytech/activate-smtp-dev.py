"""One-off DEV helper: activate the existing Zitadel SMTP config(s) in a running
stack (Zitadel v3 requires activation before it will send). Uses the bootstrap
key at secrets/_bootstrap-admin-sa.json. Run from the repo root."""
import os
import sys

os.environ["ADMIN_KEY_PATH"] = "secrets/_bootstrap-admin-sa.json"
os.environ.setdefault("PROVISION_ISSUER", "http://host.docker.internal:8080")
sys.path.insert(0, os.path.join("deploy", "compose", "provisioner"))
import provision  # noqa: E402

admin = provision.load_admin_key()
token = provision.mint_management_token(admin)
h = {"Authorization": f"Bearer {token}", "Content-Type": "application/json"}
s = provision.request_with_retry("POST", f"{provision.ISSUER}/admin/v1/smtp/_search", headers=h, json_body={})
configs = s.json().get("result") or []
print("existing SMTP configs:", [(c.get("id"), c.get("state")) for c in configs])
for c in configs:
    r = provision.request_with_retry(
        "POST", f"{provision.ISSUER}/admin/v1/smtp/{c['id']}/_activate", headers=h, json_body={})
    print("activate", c["id"], "->", r.status_code)
