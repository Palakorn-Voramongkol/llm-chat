# Zitadel deployment for the llm-chat manager

This directory installs Zitadel as a **native systemd service** alongside the
existing palakorn.com nginx + PostgreSQL on the same VPS, exposed at
`https://id.palakorn.com`. No Docker.

> Zitadel must run on its own hostname. OIDC discovery is required by spec to
> live at `<issuer>/.well-known/openid-configuration`, so subpath deployment
> (`palakorn.com/zitadel`) breaks every OIDC client. Use the subdomain.

## Files

| File | Purpose |
|---|---|
| `install.sh` | Downloads the Zitadel binary, creates the `zitadel` user, lays out `/etc/zitadel` + `/var/lib/zitadel`, installs the systemd unit. Run once with `sudo`. |
| `zitadel.service` | systemd unit (Type=simple, hardened). Reads env from `/etc/zitadel/zitadel.env`. |
| `zitadel.env.example` | Copy to `/etc/zitadel/zitadel.env`; fill in secrets (mode 0600). |
| `nginx-id.palakorn.com.conf` | nginx vhost terminating TLS, proxying to `127.0.0.1:8080`. |

## Deploy steps (run on the server)

### 1. DNS

Add an A record:
```
id.palakorn.com    A    <your VPS IP>
```
Wait for it to resolve (`dig +short id.palakorn.com` returns your IP).

### 2. Postgres role

As the OS `postgres` user:
```bash
sudo -u postgres psql <<'SQL'
CREATE USER zitadel WITH PASSWORD '<choose a strong password>';
SQL
```
Zitadel will create the `zitadel` database itself on first boot using the
admin credentials in `zitadel.env` (`ZITADEL_DATABASE_POSTGRES_ADMIN_*`).

### 3. Run the installer

```bash
cd deploy/zitadel
sudo ./install.sh                    # latest release
# or pin: sudo ZITADEL_VERSION=v2.71.10 ./install.sh
```

The installer will:
- download the Zitadel binary to `/usr/local/bin/zitadel`
- create `zitadel:zitadel` system user (no shell, no login)
- create `/etc/zitadel/` (mode 0750, owner zitadel)
- create `/var/lib/zitadel/` (mode 0750, owner zitadel — Zitadel's working dir)
- copy the example env to `/etc/zitadel/zitadel.env` (mode 0600) if not already present
- install `/etc/systemd/system/zitadel.service` and `daemon-reload`

It does **not** start the service yet.

### 4. Fill in the env file

```bash
sudo $EDITOR /etc/zitadel/zitadel.env
```

Required fields:
- `ZITADEL_MASTERKEY` — generate once with `openssl rand -hex 16`. **Lose this and you lose access to encrypted data.** Back it up.
- `ZITADEL_DATABASE_POSTGRES_USER_PASSWORD` — the password from step 2.
- `ZITADEL_DATABASE_POSTGRES_ADMIN_PASSWORD` — your local `postgres` superuser password (only used on first boot to create the `zitadel` database; rotate or revoke afterward).
- `ZITADEL_FIRSTINSTANCE_ORG_HUMAN_PASSWORD` — bootstrap admin password.

### 5. nginx vhost + cert

```bash
sudo cp nginx-id.palakorn.com.conf /etc/nginx/sites-available/id.palakorn.com
sudo ln -sf /etc/nginx/sites-available/id.palakorn.com /etc/nginx/sites-enabled/
sudo certbot --nginx -d id.palakorn.com
sudo nginx -t && sudo systemctl reload nginx
```
certbot writes the `ssl_certificate` paths the vhost expects.

### 6. Start Zitadel

```bash
sudo systemctl enable --now zitadel
sudo journalctl -fu zitadel
```

Wait for the line `server is listening on [::]:8080`. First boot takes ~30 s
(it creates the schema). Subsequent boots are seconds.

If something fails, the journal has the full reason. Common issues:
- Postgres credentials wrong → `connection refused` / `password authentication failed`
- `ZITADEL_MASTERKEY` not exactly 32 hex chars → startup error
- `id.palakorn.com` not resolving → certbot fails earlier in step 5

### 7. First login

Open `https://id.palakorn.com/ui/console` and log in with the bootstrap admin
(`ZITADEL_FIRSTINSTANCE_ORG_HUMAN_*` from `zitadel.env`).

After confirming login works, you can remove the `ZITADEL_FIRSTINSTANCE_*`
lines from `/etc/zitadel/zitadel.env` — they're only consulted on the very
first boot.

### 8. Configure for the manager

In the Console:

1. **Organization** — defaults to one named after the bootstrap admin. Rename or create another (`palakorn`).
2. **Project** — create one named `llm-chat`.
3. **Roles** under that project — add `chat.user`, `chat.admin`.
4. **Application (API)** — under the project:
   - Name: `llm-chat-manager`
   - Auth method: **JWT**
   - Note the `client_id` — this is the JWT `aud` value the manager will validate.
5. **Application (Native or Web)** — for the Tauri / Python interactive client:
   - Type: Native (CLI) or Web (Tauri shell)
   - Auth method: **PKCE**
   - Redirect URIs: whatever your client uses
   - Note the `client_id`.
6. **Service User** (for non-interactive Python CLI):
   - Create user → tab **Keys** → generate a JSON key, download it.
   - Tab **Authorizations** → grant role `chat.user` in project `llm-chat`.
7. (Optional) **Identity Providers** → wire up Google / GitHub:
   - Paste `client_id` + `client_secret` from each provider's dev console.
   - Configure JIT-create + auto-link rules.

### 9. Manager-side change (separate task)

The manager needs to verify Zitadel JWTs. The scaffolding already lives at
`manager/src/auth_zitadel.rs` (compiles only after the Cargo deps + `mod`
declaration noted at the top of that file). To enable it:

1. Add to `manager/Cargo.toml`:
   ```toml
   jsonwebtoken = "9"
   reqwest      = { version = "0.12", default-features = false, features = ["json", "rustls-tls"] }
   ```
2. Add to `manager/src/main.rs`:
   ```rust
   mod auth_zitadel;
   ```
3. Set environment for the manager systemd unit (e.g. via a drop-in):
   ```
   ZITADEL_ISSUER=https://id.palakorn.com
   ZITADEL_AUDIENCE=<client_id of the API app from step 8.4>
   ZITADEL_PROJECT_ID=<project id of llm-chat>
   ```
4. Replace `extract_token` + `check_token_eq` in `handle_client` with
   `auth_zitadel::verify(...)`. See the usage sketch at the top of
   `auth_zitadel.rs`.
5. Drop `auth_token_path()` + `lock_token_acl()` + the file write at startup
   once you're confident the JWT path works.

### 10. Client examples

**Python service-user client** (using the JSON key from step 8.6):
```python
import json, time, requests, jwt as pyjwt

key = json.load(open("zitadel-key.json"))
now = int(time.time())
assertion = pyjwt.encode(
    {
        "iss": key["userId"], "sub": key["userId"],
        "aud": "https://id.palakorn.com",
        "iat": now, "exp": now + 3600,
    },
    key["key"], algorithm="RS256",
    headers={"kid": key["keyId"]},
)
tok = requests.post(
    "https://id.palakorn.com/oauth/v2/token",
    data={
        "grant_type": "urn:ietf:params:oauth:grant-type:jwt-bearer",
        "scope": "openid urn:zitadel:iam:org:project:id:<llm-chat-projectid>:aud",
        "assertion": assertion,
    },
).json()["access_token"]

# now use Bearer `tok` against wss://api.palakorn.com/...
```

## Rollback

```bash
sudo systemctl disable --now zitadel
sudo rm /etc/systemd/system/zitadel.service /usr/local/bin/zitadel
sudo systemctl daemon-reload
sudo rm /etc/nginx/sites-enabled/id.palakorn.com
sudo nginx -t && sudo systemctl reload nginx
# data persists in Postgres database `zitadel`; drop it for a clean slate:
#   sudo -u postgres psql -c 'DROP DATABASE zitadel;'
```

`/etc/zitadel/` and `/var/lib/zitadel/` are kept on uninstall — wipe them
manually if you don't want them around.
