# Manager deployment

systemd unit + nginx vhost for the llm-chat manager. Once this is in place,
GitHub Actions can ship new manager binaries via SSH and restart the service
without further server-side changes.

## Files

| File | Purpose |
|---|---|
| `llm-chat-manager.service` | systemd unit (Type=simple, hardened). Runs `/usr/local/bin/llm-chat-manager` as the `llm-chat:llm-chat` system user. |
| `manager.env.example` | Template for `/etc/llm-chat/manager.env` (mode 0600). Holds Zitadel issuer/audience/project + DB URL. |
| `nginx-api.palakorn.com.conf` | TLS-terminating vhost on `api.palakorn.com`. Proxies all WS endpoints (`/chat`, `/control`, `/qa/`, `/s/`) to the manager on `127.0.0.1:7777`. |
| `llm-chat-deploy.sh` | Forced-command script invoked by the deploy SSH key. Allowlists three actions: `restart-manager`, `restart-worker`, `install-{manager,worker}-binary`. |

## One-time server setup

Required before GH Actions deploy works.

### 1. DNS

Add A record at your DNS provider:
```
api.palakorn.com    A    <public IP>    TTL 300
```
Verify: `dig +short api.palakorn.com` returns your IP.

### 2. Runtime user + dirs (manager)

```bash
sudo groupadd --system llm-chat
sudo useradd --system --gid llm-chat \
    --home-dir /var/lib/llm-chat --shell /usr/sbin/nologin llm-chat
sudo install -d -m 0750 -o llm-chat -g llm-chat /var/lib/llm-chat /etc/llm-chat
```

### 3. Install systemd unit + env file

```bash
sudo cp llm-chat-manager.service /etc/systemd/system/
sudo cp manager.env.example      /etc/llm-chat/manager.env
sudo chown llm-chat:llm-chat     /etc/llm-chat/manager.env
sudo chmod 600                   /etc/llm-chat/manager.env
sudo $EDITOR /etc/llm-chat/manager.env       # fill in the blanks
sudo systemctl daemon-reload
```

The unit won't start until the binaries are at `/usr/local/bin/llm-chat-manager`
and `/usr/local/bin/llm-chat-worker` (the first deploy installs them).

### 4. nginx + TLS

```bash
sudo cp nginx-api.palakorn.com.conf /etc/nginx/sites-available/api.palakorn.com
sudo ln -sf /etc/nginx/sites-available/api.palakorn.com /etc/nginx/sites-enabled/
sudo certbot --nginx -d api.palakorn.com
sudo nginx -t && sudo systemctl reload nginx
```

### 5. Deploy user + SSH key + forced-command

```bash
# Deploy user (no shell, no password, only key auth)
sudo useradd --system --create-home --home-dir /var/lib/deploy \
    --shell /usr/sbin/nologin deploy
sudo install -d -m 0755 -o deploy -g deploy \
    /var/lib/deploy/llm-chat/manager /var/lib/deploy/llm-chat/worker
sudo install -d -m 0700 -o deploy -g deploy /var/lib/deploy/.ssh
sudo touch /var/lib/deploy/.ssh/authorized_keys
sudo chmod 600 /var/lib/deploy/.ssh/authorized_keys
sudo chown deploy:deploy /var/lib/deploy/.ssh/authorized_keys

sudo cp llm-chat-deploy.sh /usr/local/bin/llm-chat-deploy.sh
sudo chmod 0755 /usr/local/bin/llm-chat-deploy.sh

# Sudoers — deploy can only restart these specific services
sudo tee /etc/sudoers.d/llm-chat-deploy <<'SUDOERS' >/dev/null
deploy ALL=(root) NOPASSWD: /usr/bin/systemctl restart llm-chat-manager
deploy ALL=(root) NOPASSWD: /usr/bin/systemctl restart llm-chat-worker
deploy ALL=(root) NOPASSWD: /usr/bin/systemctl is-active llm-chat-manager
deploy ALL=(root) NOPASSWD: /usr/bin/systemctl is-active llm-chat-worker
SUDOERS
sudo chmod 0440 /etc/sudoers.d/llm-chat-deploy
sudo visudo -c -f /etc/sudoers.d/llm-chat-deploy

# Generate the key on the server, install the public half:
ssh-keygen -t ed25519 -f /tmp/deploy-key -N '' -C 'gh-actions-deploy@llm-chat'
sudo bash -c "echo 'command=\"/usr/local/bin/llm-chat-deploy.sh\",no-port-forwarding,no-X11-forwarding,no-agent-forwarding,no-pty $(cat /tmp/deploy-key.pub)' >> /var/lib/deploy/.ssh/authorized_keys"
sudo chown deploy:deploy /var/lib/deploy/.ssh/authorized_keys

# Add /tmp/deploy-key (PRIVATE half) as GitHub secret SSH_DEPLOY_KEY, then:
sudo rm /tmp/deploy-key /tmp/deploy-key.pub
```

## What GH Actions does on each push to main

1. `cargo build --release` for both `manager/` and `worker/` on the runner.
2. `scp` binaries → `/var/lib/deploy/llm-chat/{manager,worker}/`.
3. `ssh deploy@HOST install-manager-binary` and `install-worker-binary`.
4. `ssh deploy@HOST restart-manager` (and worker if its binary changed).
5. Health probe: `curl https://id.palakorn.com/.well-known/openid-configuration`.
6. End-to-end probe: Python client authenticates as `kabytech`, sends `hello`
   to `wss://api.palakorn.com/chat`, expects an `a` frame.

## Required GitHub Actions secrets

| Secret | Value |
|---|---|
| `SSH_DEPLOY_KEY` | The full private deploy key (`-----BEGIN OPENSSH PRIVATE KEY-----` … `-----END OPENSSH PRIVATE KEY-----`). |
| `SSH_HOST`       | The server's hostname or public IP (e.g. `64.176.85.75`). |
| `KABYTECH_KEY`   | The kabytech machine-user JSON key from Zitadel (the file Zitadel handed you when you created the key). |
