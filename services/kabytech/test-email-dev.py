"""DEV: create a user with sendCode (triggers Zitadel to EMAIL the invite link)
to verify SMTP delivery to MailHog. Run: python services/kabytech/test-email-dev.py <email>"""
import os, sys
os.environ["ADMIN_KEY_PATH"] = "secrets/_bootstrap-admin-sa.json"
os.environ.setdefault("PROVISION_ISSUER", "http://host.docker.internal:8080")
sys.path.insert(0, os.path.join("deploy", "compose", "provisioner"))
import provision  # noqa: E402

email = sys.argv[1]
admin = provision.load_admin_key()
token = provision.mint_management_token(admin)
h = {"Authorization": f"Bearer {token}", "Content-Type": "application/json"}
body = {
    "username": email,
    "profile": {"givenName": "Mail", "familyName": "Test"},
    "email": {"email": email, "sendCode": {
        "urlTemplate": "http://localhost:3001/accept?userID={{.UserID}}&code={{.Code}}&orgID={{.OrgID}}"}},
}
r = provision.request_with_retry("POST", f"{provision.ISSUER}/v2/users/human", headers=h, json_body=body)
print("create status", r.status_code, "userId", r.json().get("userId"))
