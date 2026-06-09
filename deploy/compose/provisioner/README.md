## Live ORG_OWNER bump (already-provisioned instance)

The provisioner grants the admin service account `ORG_OWNER` on a **clean**
boot (`assign_admin_member`). A bare re-run **no-ops** on an existing member
(Zitadel returns 409 == already a member, treated as success), so an instance
provisioned before the bump keeps its old `ORG_USER_MANAGER` role and the
admin Console's policy/project/role/app writes will 403.

To bump the live instance without re-provisioning, run the one-shot
`update-member` (`PUT /management/v1/orgs/me/members/{saUserId}`) with the
**bootstrap IAM_OWNER** key (it needs `org.member.write`; the runtime SA does
not). From the provisioner directory:

```bash
python - <<'PY'
import json, provision
boot = provision.load_admin_key()                       # bootstrap IAM_OWNER key
token = provision.mint_management_token(boot)
org_id = provision.fetch_org_id(token)
headers = provision.mgmt_headers(token, org_id)
sa_user_id = open(f"{provision.SECRETS_DIR}/admin_api_user_id").read().strip()
provision.update_admin_member(token, headers, sa_user_id)
print(f"bumped {sa_user_id} -> {provision.ADMIN_SA_ROLE}")
PY
```

Verify (expect `ORG_OWNER` in the member's roles):

```bash
curl -s -X POST "$PROVISION_ISSUER/management/v1/orgs/me/members/_search" \
  -H "Authorization: Bearer $BOOT_TOKEN" -H "Content-Type: application/json" \
  -d '{}' | python -c 'import sys,json; \
[print(m["userId"], m["roles"]) for m in json.load(sys.stdin).get("result", [])]'
```
