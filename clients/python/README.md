# llm-chat Python client

A small, dependency-light client for the llm-chat manager's `/chat` WebSocket.
It authenticates to Zitadel with the machine-user key (JWT-bearer), connects to
the manager, and exchanges typed `q`/`a` frames — either one-shot or as an
interactive multi-turn REPL that keeps conversation context.

## Install

```bash
# from this directory (clients/python)
pip install .            # installs the `llm-chat` command + deps
# or, without installing the package:
pip install -r requirements.txt
```

Requires Python 3.9+. Dependencies: `pyjwt[crypto]`, `requests`, `websockets`.

## Quick start (zero config)

When the compose stack is running it writes credentials to `<repo>/secrets/`.
The client auto-discovers them, so no flags are needed:

```bash
llm-chat chat                      # interactive REPL (also: just `llm-chat`)
llm-chat ask --send "what is 2+2?" # one-shot
python -m llm_chat chat            # if not pip-installed
```

> First reply includes an ~8 s session warm-up; high-effort replies take ~10–20 s,
> so keep `--timeout` generous (default 120 s).

## Commands

| Command | Purpose |
|---|---|
| `llm-chat chat` | interactive multi-turn REPL (default when no subcommand) |
| `llm-chat ask --send TEXT` | send one question, print the answer, exit |

### Connection / auth flags (both commands)

Each value falls back to an env var, then the `secrets/` dir:

| Flag | Env var | Default |
|---|---|---|
| `--issuer` | `ZITADEL_ISSUER` | `http://host.docker.internal:8080` |
| `--project` | `PROJECT_ID` | `secrets/project_id` |
| `--key-file` | `KABYTECH_KEY` | `secrets/kabytech-key.json` |
| `--manager` | `MANAGER_WS` | `ws://127.0.0.1:7777/chat` |
| `--timeout` | — | `120` (seconds per answer) |
| `-v` / `-vv` | — | INFO / DEBUG logs on stderr |

`LLM_CHAT_SECRETS_DIR` overrides where `secrets/` is looked up.

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
  auth.py       credential resolution + JWT-bearer token minting
  protocol.py   ChatClient: persistent session, ask(), reconnect/re-auth
  repl.py       interactive UI (colors, spinner, history, slash-commands)
  cli.py        `llm-chat` entry point (ask / chat subcommands)
  config.py     shared args + logging
  errors.py     exception hierarchy
tests/          unit tests for auth + protocol
llm_chat_client.py   legacy one-shot shim (kept for the docs/tests interface)
llm_chat_repl.py     legacy REPL shim
```

`llm_chat_client.py` keeps its original interface
(`--issuer/--project/--key-file/--manager/--send`) so existing docs and the
compose verification steps keep working.

## Tests

```bash
pip install -e ".[test]"
pytest -q          # 11 unit tests (auth + protocol), no stack required
```
