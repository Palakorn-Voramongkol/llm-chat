# llm-chat compose stack (local-dev only)

Server-side stack: **postgres + Zitadel + provisioner + manager** in Docker,
with the **worker running natively on Windows** (real `claude`, `~/.claude`,
webview). LOCAL DEV ONLY — the issuer is plain HTTP, cookies are non-Secure.
**Never expose this beyond your machine.**

The whole thing hinges on one literal issuer string,
`http://host.docker.internal:8080`, that resolves the same from the host
(Python client) and from inside containers (manager). Don't change it.

## Prerequisites
- Docker Desktop for Windows.
- The worker built: `cargo build --release` in `worker/` (produces
  `worker/target/release/llm-chat.exe`).
- Python 3 with `pyjwt[crypto]`, `requests`, `websockets` for the client.

## Run (§9)

```powershell
# 0. From repo root D:\projects\llm-chat.

# 1. Pre-flight: the three host ports must be FREE (a dual-listener 7777
#    collision has bitten this environment before).
Get-NetTCPConnection -LocalPort 7777,7878,8080 -State Listen -ErrorAction SilentlyContinue

# 2. Env file.
cp .env.example .env
#   ZITADEL_MASTERKEY   -> openssl rand -hex 16   (exactly 32 hex chars; one-shot)
#   POSTGRES_PASSWORD   -> a strong password
#   LLM_CHAT_AUTH_TOKEN -> openssl rand -hex 32   (shared by manager + worker)

# 3. Start the worker FIRST (before compose — the manager's :7878 probe is fatal).
#    Approve the Windows Firewall prompt for the 0.0.0.0 bind if it appears.
.\deploy\compose\run-worker.ps1

# 4. Bring up the server side.
docker compose up -d
#    Wait until `docker compose ps` shows zitadel healthy + zitadel-init Exited(0),
#    and .\secrets\kabytech-key.json + .\secrets\project_id exist.

# 5a. Machine round-trip (kabytech service account, no human).
python clients/python/llm_chat_client.py `
  --issuer  http://host.docker.internal:8080 `
  --project (Get-Content -Raw .\secrets\project_id).Trim() `
  --key-file .\secrets\kabytech-key.json `
  --manager ws://127.0.0.1:7777/chat `
  --send "hello"
#    Expect an 'a' frame and exit code 0.

# 5b. Human, interactive (browser login as the demo user).
cd clients\python; pip install .        # one-time: gives the `llm-chat` command
llm-chat login                          # browser opens; sign in with:
Get-Content ..\..\secrets\demo_user, ..\..\secrets\demo_password
llm-chat whoami                         # shows demo@llm-chat.local + chat.user
llm-chat chat                           # interactive REPL on the human identity
```

The provisioner registers an OIDC app (`secrets/oidc_client_id`) and a demo
human user (`secrets/demo_user` / `secrets/demo_password`) with the `chat.user`
role. The machine (kabytech) and human paths hit the same manager check
(JWKS-validated JWT + `chat.user`); only the principal differs.

### Clean reset
Wipe Zitadel state AND host secrets together, or the stale kabytech key won't
match the fresh instance:
```powershell
docker compose down -v
Remove-Item -Recurse -Force .\secrets
```

## Client env-var names (footgun)
The client reads DIFFERENT env names than the manager. If driving by env
instead of flags:

| Client flag | Client env var | NOT |
|---|---|---|
| `--issuer`   | `ZITADEL_ISSUER` | — |
| `--project`  | `PROJECT_ID`     | not `ZITADEL_PROJECT_ID` |
| `--key-file` | `KABYTECH_KEY`   | — |
| `--manager`  | `MANAGER_WS`     | — |

## Two scopes — do not swap (spec §7.2)
- Provisioner (Management API): `openid profile urn:zitadel:iam:org:project:id:zitadel:aud` (literal `zitadel`).
- Client (token the manager validates): `openid profile urn:zitadel:iam:org:project:id:<project>:aud urn:zitadel:iam:org:projects:roles` (numeric project id + plural `projects:roles`). This is already fixed in the Python client.

## host.docker.internal fallback (§8)
Container-side resolution is automatic under Docker Desktop (the manager reaches
the issuer fine). The **host-side** Python client is the one that can break,
because it must reach the SAME issuer URL `http://host.docker.internal:8080` to
fetch its token. Verify first:
```powershell
Resolve-DnsName host.docker.internal
curl http://host.docker.internal:8080/debug/healthz   # want 200
```
Two distinct failure modes have been seen:

1. **No entry at all** (e.g. WSL2 engine with the Win32-hosts setting off) —
   resolution fails outright. Append as Administrator to
   `C:\Windows\System32\drivers\etc\hosts`:
   ```
   127.0.0.1 host.docker.internal
   ```

2. **Docker manages the entry but points it at the host's LAN IP**
   (e.g. `192.168.1.106 host.docker.internal`). Resolution *succeeds* but the
   client still can't connect: Docker Desktop publishes the port reachable only
   on **loopback** (`127.0.0.1`/`::1` work, the LAN IP is firewall-blocked from
   the host). Confirm with `curl http://127.0.0.1:8080/debug/healthz` (200) vs
   the LAN IP (times out). Fix: repoint the Docker-managed line to loopback as
   Administrator (host-side only — containers use their own injected entry and
   are unaffected):
   ```
   127.0.0.1 host.docker.internal
   ```
   `ipconfig /flushdns` after editing. (Docker Desktop may rewrite its managed
   block on restart; re-apply if the client starts failing again.)

Do NOT leave two conflicting `host.docker.internal` lines (duplicates cause
flaky resolution) — edit the existing one in place. Always publish Zitadel
`8080:8080` (all interfaces), never `127.0.0.1:8080:8080`.

## Verification (§10)
- `docker compose config --quiet` exits 0.
- `docker compose logs manager` shows **"Zitadel auth enabled"**, NOT
  "Zitadel auth NOT configured — falling back to shared-token auth".
- `.\secrets\kabytech-key.json` is valid JSON with `"type":"serviceaccount"`,
  `keyId`, `key` (PEM), `userId`.
- The client round-trip returns `a` and exits 0.

## Risks (§11)
- **Management-API drift:** the v1 endpoints are deprecated; the Zitadel tag is
  pinned (to-confirm). Never `:latest`. Re-verify the provisioner call surface
  on any bump.
- **`_search` 409-recovery is UNVERIFIED:** provision.py raises on a 409 instead
  of guessing the endpoint. Clean reset (down -v + delete secrets) avoids it.
- **Port collisions:** free 7777/7878/8080 first (see pre-flight).
- **Clock skew:** JWT iat/exp windows fail if host/container clocks drift.
- **HTTP-only issuer:** no TLS, cleartext tokens — local-dev only.
- **Required address vars:** the manager and worker fail fast if MANAGER_BIND,
  MANAGER_BACKEND_HOST, or LLM_CHAT_WS_BIND is unset — compose/run-worker.ps1
  set them all, so this only bites if you launch a binary by hand without them.
- **Secrets:** `.\secrets\kabytech-key.json` is a live RSA key; `secrets/` is
  gitignored. Never commit it.
- **Firewall prompt:** approve the worker's 0.0.0.0 bind (private networks).
- **Manager probe is fatal:** start the worker before `docker compose up`;
  `restart: unless-stopped` heals transient windows.
- **Masterkey irreversible:** exactly 32 chars, never change after first init.
