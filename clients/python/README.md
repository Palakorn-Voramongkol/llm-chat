# llm-chat Python client

A small, dependency-light client for the llm-chat manager's `/chat` WebSocket.
It connects to the manager and exchanges typed `q`/`a` frames — one-shot or as
an interactive multi-turn REPL that keeps conversation context.

**Two identities, same manager gate (a JWT carrying the `chat.user` role):**

- **Human** — interactive **browser login** (OAuth2 Authorization Code + PKCE).
  Used by `llm-chat chat`. This is a person signing in as themselves.
- **Machine (kabytech)** — a service account using a **JSON key** (JWT-bearer).
  Used by `llm-chat ask` / the legacy script / CI. This is for microservices.

Override the default per-command choice with `--auth {user,machine}`.

## Install

```bash
# from this directory (clients/python)
pip install .            # installs the `llm-chat` command + deps
# or, without installing the package:
pip install -r requirements.txt
```

Requires Python 3.9+. Dependencies: `pyjwt[crypto]`, `requests`, `websockets`,
`keyring`, `platformdirs`.

## Quick start (zero config)

When the compose stack is running it writes credentials to `<repo>/secrets/`
(machine key, OIDC client id, demo user). The client auto-discovers them.

```bash
# Human, interactive: sign in once, then chat
llm-chat login                     # opens the browser (demo user: see secrets/)
llm-chat chat                      # REPL on your user identity (also: `llm-chat`)
llm-chat whoami                    # show who you're signed in as + roles
llm-chat logout                    # revoke + clear the session

# Machine (kabytech service account): no login, uses the key
llm-chat ask --send "what is 2+2?"
python -m llm_chat chat            # if not pip-installed
```

`llm-chat chat` auto-starts the browser login if you're not signed in.
The **demo** sign-in credentials are written to `secrets/demo_user` and
`secrets/demo_password` by the provisioner.

> First reply includes an ~8 s session warm-up; high-effort replies take ~10–20 s,
> so keep `--timeout` generous (default 120 s).

## Commands

| Command | Auth | Purpose |
|---|---|---|
| `llm-chat chat` | human | interactive multi-turn REPL (default subcommand) |
| `llm-chat ask --send TEXT` | machine | send one question, print the answer, exit |
| `llm-chat login` | human | browser sign-in; cache the session |
| `llm-chat logout` | human | revoke the refresh token + clear the cache |
| `llm-chat whoami` | human | show the cached identity + roles |

### Human login (Auth Code + PKCE)

`login` runs a standard, secure desktop flow: PKCE (S256), a CSRF `state`, a
loopback redirect on `127.0.0.1:8477`, and `offline_access` so the session is
refreshed silently instead of re-prompting every few minutes. The **refresh
token is stored in your OS keyring** (Windows Credential Manager / macOS
Keychain / libsecret), falling back to a `0600` file if no keyring is present;
access tokens live in a `0600` file. `logout` revokes the refresh token at
Zitadel. MFA, if enabled, happens in the browser.

> **Local-dev caveat:** the compose issuer is plain **HTTP** (no TLS). Fine for
> localhost; **production must use HTTPS** for the issuer and redirect.

### Connection / auth flags

Each value falls back to an env var, then the `secrets/` dir:

| Flag | Env var | Default |
|---|---|---|
| `--issuer` | `ZITADEL_ISSUER` | `http://host.docker.internal:8080` |
| `--project` | `PROJECT_ID` | `secrets/project_id` |
| `--key-file` | `KABYTECH_KEY` | `secrets/kabytech-key.json` (machine) |
| `--oidc-client-id` | `OIDC_CLIENT_ID` | `secrets/oidc_client_id` (human) |
| `--oidc-port` | — | `8477` (login redirect) |
| `--manager` | `MANAGER_WS` | `ws://127.0.0.1:7777/chat` |
| `--auth` | — | `chat`→`user`, `ask`→`machine` |
| `--timeout` | — | `120` (seconds per answer) |
| `-v` / `-vv` | — | INFO / DEBUG logs on stderr |

`LLM_CHAT_SECRETS_DIR` overrides where `secrets/` is looked up;
`LLM_CHAT_CONFIG_DIR` overrides where the token cache is stored.

### REPL slash-commands

```
/help      /history     /session
/reset     /multi       /quit (/exit)
```

`/reset` starts a fresh backend session (clears claude's context); `/multi`
sends a multi-line message (end with a single `.`).

## Exit codes

| Code | Meaning |
|---|---|
| 0 | success |
| 2 | usage / missing-or-bad credentials |
| 3 | authentication failed (Zitadel) |
| 4 | manager unavailable (connect/connection lost) |
| 5 | protocol error or answer timeout |
| 130 | interrupted (Ctrl-C) |

## Layout

```
llm_chat/
  auth.py       machine credential resolution + JWT-bearer token minting
  oidc.py       human login: Auth Code + PKCE flow, token refresh/revoke
  tokens.py     keyring-backed token store (refresh) + file fallback
  protocol.py   ChatClient: persistent session, ask(), reconnect/re-auth
  repl.py       interactive UI (colors, spinner, history, slash-commands)
  cli.py        `llm-chat` entry point (chat/ask/login/logout/whoami)
  config.py     shared args + logging
  errors.py     exception hierarchy
tests/          unit tests (auth, oidc, tokens, protocol, repl, cli)
llm_chat_client.py   legacy one-shot shim (kept for the docs/tests interface)
llm_chat_repl.py     legacy REPL shim
```

`llm_chat_client.py` keeps its original interface
(`--issuer/--project/--key-file/--manager/--send`) so existing docs and the
compose verification steps keep working.

## Tests

```bash
pip install -e ".[test]"
pytest -q          # 40 unit tests (auth/oidc/tokens/protocol/repl/cli), no stack required
```
