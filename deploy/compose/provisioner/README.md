## Live least-privilege grant (already-provisioned instance)

On a **clean** boot the provisioner gives the admin service account two *scoped*
memberships — `ORG_USER_MANAGER` at the org level (`assign_admin_member`: users +
grants) and `PROJECT_OWNER` on the llm-chat project
(`assign_admin_project_member`: apps + roles). It deliberately does **not** grant
`ORG_OWNER`: the runtime SA's key is persisted to `./secrets`, so standing org
ownership would be a privilege-escalation target (security review, spec §3/§5).
Org policies are written by this one-time provisioner with the bootstrap token,
never by the runtime SA.

A bare re-run **no-ops** on existing memberships (Zitadel returns 409 == already
a member, treated as success). An instance provisioned **before** the project
grant existed has `ORG_USER_MANAGER` but lacks the project `PROJECT_OWNER`, so the
Console's app/role writes will 403. Grant it live without re-provisioning by
running the one-shot below with the **bootstrap IAM_OWNER** key (it needs
`project.member.write`; the runtime SA does not). From the provisioner directory:

```bash
python - <<'PY'
import provision
boot = provision.load_admin_key()                       # bootstrap IAM_OWNER key
token = provision.mint_management_token(boot)
org_id = provision.fetch_org_id(token)
headers = provision.mgmt_headers(token, org_id)
sa = open(f"{provision.SECRETS_DIR}/admin_api_user_id").read().strip()
pid = open(f"{provision.SECRETS_DIR}/project_id").read().strip()
provision.update_admin_member(token, headers, sa)                 # ensure ORG_USER_MANAGER (no-op if set)
provision.assign_admin_project_member(token, headers, pid, sa)    # PROJECT_OWNER on the project
print(f"{sa}: org={provision.ADMIN_SA_ROLE} project={provision.ADMIN_SA_PROJECT_ROLE}")
PY
```

Verify the project membership (expect `PROJECT_OWNER`):

```bash
curl -s -X POST "$PROVISION_ISSUER/management/v1/projects/$PROJECT_ID/members/_search" \
  -H "Authorization: Bearer $BOOT_TOKEN" -H "Content-Type: application/json" \
  -d '{}' | python -c 'import sys,json; [print(m["userId"], m["roles"]) for m in json.load(sys.stdin).get("result", [])]'
```
