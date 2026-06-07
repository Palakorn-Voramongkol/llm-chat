# Configuration knowledge base

A running log of non-obvious configuration problems we've hit on this project
and the fix that worked. New entries go at the top. Each entry should answer:
**what broke**, **why**, **the fix**, and **how to verify**. If you find a new
problem, add it here when you find the fix — future-you will thank you.

---

## Provisioning a new machine-user account in Zitadel

**Use case:** a new app/team needs its own credential to talk to the
manager (rather than sharing the kabytech one). Five Zitadel resources
need to line up:

  1. **Org** (e.g. `corridraw.com`) — its own tenant.
  2. **Machine user** in that org with `accessTokenType=ACCESS_TOKEN_TYPE_JWT`.
     **This step is the gotcha.** The default for a freshly-created machine
     user is `ACCESS_TOKEN_TYPE_BEARER`, which produces opaque/JWE access
     tokens. The manager validates incoming tokens locally against
     Zitadel's JWKS — that only works for real JWTs, so JWE tokens come
     back as 401. Symptoms include the access_token having 5 dot-separated
     segments instead of 3 and the Python decoder choking on UTF-8 in the
     middle segment.
  3. **JSON key** for the user, downloaded once and saved to
     `~/.config/llm-chat/<user>-key.json` mode 0600.
  4. **Project grant** — the `llm-chat` project (owned by `kabytech.com`)
     is granted to the new org with role `chat.user`. Without this, a
     UserGrant in the new org pointing at the cross-org project will be
     refused.
  5. **User grant** — the new machine user gets `chat.user` on the granted
     project. Tokens minted by this user then carry
     `urn:zitadel:iam:org:project:<id>:roles.chat.user.<orgid>` — the
     manager checks for `chat.user` and lets the WS upgrade through.

**Provisioning script:** `deploy/zitadel/provision_machine_user.py` does
all five steps idempotently (re-running rotates the key but reuses the
existing org/user/grants). Example:

```bash
python3 deploy/zitadel/provision_machine_user.py \
    --org  corridraw.com \
    --user corridraw \
    --out  ~/.config/llm-chat/corridraw-key.json
```

**Side note about `/v2beta/organizations`:** the org-creation path uses
`POST /v2beta/organizations`, not `/management/v1/orgs`. The latter
fails with `User could not be found (COMMAND-uXHNj)` when called as a
system user (sysadmin) because Zitadel tries to record the org's "human"
admin from the user table and sysadmin isn't a row there. The v2beta
endpoint accepts system-user callers without that lookup.

**Verify:** mint a token via the saved key and decode the middle JWT
segment. The claims should include
`urn:zitadel:iam:org:project:370627061150121985:roles` mapping
`chat.user` to the new org's id. Then `chat --account <user>` should
round-trip cleanly through the manager.

---

## Per-session working directory (`?cwd=…` on `/chat`)

**Feature**, not a bug-fix — but worth recording so it isn't reinvented.

A `/chat` connection accepts an optional `?cwd=<urlencoded-absolute-path>`
query parameter. The manager forwards it to the worker's `/control` "open"
command, which:

1. Canonicalizes the path (rejects non-existent or non-directory paths
   with a clear error to the client),
2. Auto-trusts it in `~/.claude.json` (sets
   `projects[<cwd>].hasTrustDialogAccepted = true`) so claude skips its
   first-launch trust dialog,
3. Spawns the claude PTY with `CommandBuilder::cwd(...)` (Unix) or
   `CreateProcessW(lpCurrentDirectory=...)` (Windows) so claude is
   *actually* running in that folder — relative file paths in the chat
   resolve correctly without `--add-dir` gymnastics.

The Python client passes it via positional arg: `chat ~/projects/foo`. The
arg is `expanduser()`'d, `.resolve()`'d, then percent-encoded into the URL.

**Security note:** auto-trusting any caller-supplied path looks scary, but
the credential gating the API (Zitadel JWT with `chat.user` role) is the
real access boundary. Anyone who can reach `/chat` can already do
everything claude can, so silently accepting "trust this folder for me"
adds no new attack surface.

**Verify:** create a file in some dir, run `chat <dir>`, ask claude to
read it. Then delete the file, send `/clear`, ask again — claude should
correctly report the file is gone.

---

## claude shows the "trust this folder" dialog and eats the user's question

**Symptom:** Client → manager round-trip succeeds (`initialized` + `ack` arrive)
but no `a` frame; the client times out. Manager logs say `queued + sent` but
nothing comes back from the worker `/qa/<sid>` stream. Watching the raw
`/s/<sid>` PTY output reveals claude is rendering a *"Yes, I trust this
folder / No, exit"* dialog instead of the chat prompt.

**Why:** `claude` 2.1.123 records per-cwd trust in `~/.claude.json` under
`projects.<cwd>.hasTrustDialogAccepted`. When the manager's systemd unit
sets `WorkingDirectory=/home/llm` but only `/home/llm/projects/llm-chat`
has been accepted (because that's where you usually run claude
interactively), the manager-spawned claude shows the trust dialog. The
manager then dutifully types the user's question into that dialog, where
it's discarded; the trailing `\r` confirms the trust prompt instead.

`--dangerously-skip-permissions` skips per-tool prompts (Read, Write, Bash)
but does **not** skip the folder-trust dialog.

**Fix:** Mark the manager's working dir as trusted in `~/.claude.json`:
```bash
python3 -c "import json,os
p='/home/llm/.claude.json'; d=json.load(open(p))
d.setdefault('projects',{}).setdefault('/home/llm',{})['hasTrustDialogAccepted']=True
tmp=p+'.tmp'; json.dump(d, open(tmp,'w'), indent=2); os.chmod(tmp,0o600); os.replace(tmp,p)"
```
Then `systemctl restart llm-chat-manager`.

**Verify:** Run the client; expect a real `a` frame. If you ever change the
manager's `WorkingDirectory` (or run claude as a different user), repeat
this for the new cwd.

---

## systemd unit `User=llm` + `ProtectHome=true` ⇒ chdir denied (status 200)

**Symptom:** `llm-chat-manager.service` restart-loops with
`code=exited, status=200/CHDIR` and `Changing to the requested working
directory failed: Permission denied`. `/home/llm` exists with mode 0750
owned by `llm:llm`, so the failure looks impossible at first glance.

**Why:** systemd's `ProtectHome=true` makes `/home`, `/root`, `/run/user`
inaccessible to the service — even for the home-directory owner. Combined
with `WorkingDirectory=/home/llm`, the service can't enter its own home.

**Fix:** Drop `ProtectHome=true` from the unit. The manager + worker need
write access to `~/.claude` (claude state) and `~/.local/share/com.llm-chat.app`
(SQLite); `ProtectHome=read-only` doesn't help either. Use `PrivateTmp=true`
and `ReadWritePaths=/home/llm` instead. The current unit at
`deploy/manager/llm-chat-manager.service` already reflects this.

**Verify:** `systemctl status llm-chat-manager` should be `active (running)`
and journal should show `manager listening addr=ws://127.0.0.1:7777`.

**Caveat:** `deploy.yml` only ships **binaries**, not the unit file. After
editing the unit in the repo, `scp` it onto the box and `daemon-reload`
manually — otherwise the deployed unit and the repo drift apart silently.

---

## Zitadel system-API user (`sysadmin`): which audience to sign with

**Symptom:** Calls to `/admin/v1/orgs/_search` etc. as `sysadmin` return
401 with `Errors.Token.Invalid` and the journal shows
`audience is not valid: Audience must contain client_id "http://id.palakorn.com:443"`.
Or — when going via `/oauth/v2/token` — `Errors.AuthNKey.NotFound`.

**Why:** System-API users (declared in `ZITADEL_SYSTEMAPIUSERS`) authenticate
**by signing a JWT and using it directly as the Bearer token**, NOT by
exchanging it at `/oauth/v2/token`. The token-exchange path looks for a
DB-stored AuthN key by `kid`, which a system user doesn't have. The audience
must be `http://id.palakorn.com:443` (note the literal `http://...:443`,
even though the instance is HTTPS — this is the form Zitadel publishes
internally as the API audience).

**Fix:** When acting as `sysadmin`:
```python
import jwt, time, uuid
priv = open("/root/.zitadel-bootstrap/sysadmin.key.pem","rb").read()
now = int(time.time())
token = jwt.encode(
    {"iss":"sysadmin","sub":"sysadmin","aud":"http://id.palakorn.com:443",
     "iat":now,"exp":now+3600,"jti":str(uuid.uuid4())},
    priv, algorithm="RS256", headers={"kid":"sysadmin"})
# use `token` directly as Bearer on /admin/v1/* and /management/v1/*
```
For org-scoped calls (e.g. `/management/v1/users/...`), also send
`x-zitadel-orgid: <org-id>`.

**Verify:** `POST /admin/v1/orgs/_search` with body `{}` should return
the org list (look up `kabytech.com` by `nameQuery`, *not* `domainQuery` —
its primary domain is the auto-generated `kabytechcom.id.palakorn.com`).

---

## Generating a fresh kabytech machine-user key (no UI)

**When:** You need a Python client to authenticate as the `kabytech`
service user but don't have the JSON key file (e.g. it only lives as the
`KABYTECH_KEY` GitHub Actions secret, which is write-only).

**How:** Use the sysadmin system-API key to call
`POST /management/v1/users/{userId}/keys` with body `{"type":"KEY_TYPE_JSON"}`
on `org_id = <kabytech.com org id>`. The response's `keyDetails` field is
base64-encoded JSON in exactly the format the Python client expects.
Decode and write to `~/.config/llm-chat/kabytech-key.json` with mode 0600.

The full script is at `/tmp/mint_kabytech_key.py` (it also deletes any
older keys for the user so only one stays live). When deleting old keys,
beware Zitadel's read model is eventually-consistent — the `_search` for
keys may not yet show the one you just created. Re-list a few seconds
later if you need to see all of them.

**Reminder:** Adding a key in Zitadel does **not** invalidate existing
keys. Both the new key and any prior ones keep working until you
explicitly delete them or they expire.

---

## Manager rejects auth with 401 "JWKS fetch failed: cache empty"

**Symptom:** Client gets `401 Unauthorized` at the WS handshake. Manager
logs show an old `JWKS preload failed — clients will be rejected until
refresh succeeds error=JWKS fetch failed: error decoding response body`
from when the manager first started. Curl-ing
`https://id.palakorn.com/oauth/v2/keys` works fine right now.

**Why:** The manager preloads JWKS at startup. If Zitadel was still
warming up at that moment (e.g. on a host reboot where the manager is
ordered `After=zitadel.service` but Zitadel takes longer to be HTTP-ready
than to be marked started), the preload fails and the cache stays empty
until the next hourly refresh. Until that fires, every client is rejected
with "cache empty".

**Fix (quick):** `systemctl restart llm-chat-manager` — fresh boot,
preload retries, JWKS is healthy. The handshake callback then accepts
tokens normally.

**Fix (durable, if it recurs):** Either tighten the unit ordering with a
health-check (e.g. `ExecStartPre=/usr/bin/curl -fsS https://id.palakorn.com/oauth/v2/keys`
with a retry loop) or shorten the JWKS refresh interval in
`auth_zitadel.rs` so a transient miss self-heals faster.

---

## Zitadel access token has no `roles` claim ⇒ manager 403s with `chat.user`

**Symptom:** Token mints fine via `/oauth/v2/token`, manager validates the
signature, but rejects the WS upgrade with
`missing role chat.user (principal ... has roles [])`.

**Why:** Zitadel only embeds project roles in the access token if the
client requests them via the right scope. The bare project-audience scope
(`urn:zitadel:iam:org:project:id:<pid>:aud`) only adds the project to the
audience — it does not bring roles along.

**Fix:** Add `urn:zitadel:iam:org:projects:roles` (note the **plural**
`projects`) to the scope list. Example:
```
scope = "openid profile urn:zitadel:iam:org:project:id:370627061150121985:aud urn:zitadel:iam:org:projects:roles"
```
The Python client at `clients/python/llm_chat_client.py` already does this.

**Verify:** Decode the access token; the claims should include
`urn:zitadel:iam:org:project:<pid>:roles` mapping `chat.user` to the
granted org. If the role mapping is empty even with the right scope, the
user grant probably doesn't exist — check
`POST /management/v1/users/grants/_search` with `{"queries":[{"userIdQuery":{"userId":"<id>"}}]}`.

---

## `/usr/local/bin/claude` vs `/home/llm/.local/bin/claude`

**Context:** The worker spawns `claude --dangerously-skip-permissions` via
PATH lookup (`find_claude_path()` in `worker/src/lib.rs`). Both binaries
exist on this box. Whichever one is found first wins, and they can drift
in version (currently 2.1.121 in `/usr/local/bin` vs 2.1.123 in
`~/.local/bin` if you don't keep them in sync).

**Convention on this server:** `/usr/local/bin/claude` is a symlink to
`/home/llm/.local/bin/claude`, which itself points to whatever version
the user-local install manages (e.g. `~/.local/share/claude/versions/2.1.123`).
That way, when the user upgrades claude via the npm/native installer, the
worker picks it up automatically.

**If you need to revert** to a pinned root-installed binary, copies of
each version live at `/home/llm/.local/share/claude/versions/<ver>` —
`install -m 0755 -o root -g root <that-file> /usr/local/bin/claude`
restores a byte-exact copy.

**Lesson learned:** If you replace `/usr/local/bin/claude` with a symlink,
do it with `ln -sfn` *only after* you have a backup somewhere else —
`ln -sfn` will silently delete the original 247MB binary it overwrites.
