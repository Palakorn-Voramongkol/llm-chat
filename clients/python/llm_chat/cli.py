"""Command-line entry point.

Subcommands:
  chat    interactive REPL  — human login (browser Auth Code + PKCE)
  ask     one-shot question — machine key (kabytech) by default
  login   browser login, cache the user session
  logout  revoke + clear the cached session
  whoami  show the cached user identity

Credential model (override with --auth {user,machine}):
  chat → user (human, browser login)   ask/legacy → machine (kabytech key)
"""

from __future__ import annotations

import argparse
import asyncio
import base64
import json
import logging
import os
import sys

from . import __version__, oidc
from .auth import DEFAULT_ISSUER, _read_secret_file, fetch_access_token, resolve_credentials
from .config import add_common_args, configure_logging, resolve_manager
from .errors import (
    AnswerTimeout,
    AuthError,
    CredentialError,
    LlmChatError,
    ManagerUnavailable,
    ProtocolError,
)
from .protocol import ChatClient
from .render import render_markdown, resolve_mode
from .repl import run_repl
from .tokens import TokenStore

log = logging.getLogger("llm_chat.cli")

# Exit codes (documented in the README).
EXIT_OK = 0
EXIT_USAGE = 2
EXIT_AUTH = 3
EXIT_MANAGER = 4
EXIT_ANSWER = 5
EXIT_INTERRUPT = 130


# ---------------- credential resolution ----------------

def _auth_mode(args: argparse.Namespace) -> str:
    if getattr(args, "auth", None):
        return args.auth
    return "user" if args.command == "chat" else "machine"


def _resolve_issuer(args) -> str:
    return args.issuer or os.environ.get("ZITADEL_ISSUER") or DEFAULT_ISSUER


def _resolve_project(args) -> str:
    p = args.project or os.environ.get("PROJECT_ID") or _read_secret_file("project_id")
    if not p:
        raise CredentialError("no project id: pass --project or run the compose stack")
    return p


def _resolve_client_id(args) -> str:
    cid = (getattr(args, "oidc_client_id", None) or os.environ.get("OIDC_CLIENT_ID")
           or _read_secret_file("oidc_client_id"))
    if not cid:
        raise CredentialError(
            "no OIDC client id: run the compose stack so secrets/oidc_client_id exists, "
            "or pass --oidc-client-id")
    return cid


def _machine_provider(args):
    creds = resolve_credentials(args.issuer, args.project, args.key_file)
    return lambda: asyncio.to_thread(fetch_access_token, creds)


def _user_session(args):
    """Return (issuer, client_id, project, store, endpoints) for the user path."""
    issuer = _resolve_issuer(args)
    client_id = _resolve_client_id(args)
    project = _resolve_project(args)
    return issuer, client_id, project, TokenStore(issuer, client_id), oidc.discover(issuer)


def _user_provider(issuer, client_id, store, endpoints):
    def refresh_fn(rt):
        return oidc.refresh(endpoints.token, client_id=client_id, refresh_token=rt)
    return lambda: asyncio.to_thread(store.valid_access_token, refresh_fn)


def _login_and_store(issuer, client_id, project, store, port) -> oidc.TokenSet:
    ts = oidc.login(issuer, client_id, project, redirect_port=port)
    store.save(ts)
    return ts


# ---------------- jwt display (no verification) ----------------

def _decode_claims(token: str | None) -> dict:
    if not token:
        return {}
    try:
        payload = token.split(".")[1]
        payload += "=" * (-len(payload) % 4)
        return json.loads(base64.urlsafe_b64decode(payload))
    except Exception:  # noqa: BLE001 — display only
        return {}


def _print_whoami(ts: oidc.TokenSet) -> None:
    claims = _decode_claims(ts.id_token or ts.access_token)
    sub = claims.get("sub", "?")
    who = claims.get("email") or claims.get("preferred_username") or sub
    roles = []
    for k, v in claims.items():
        if k.endswith(":roles") and isinstance(v, dict):
            roles.extend(v.keys())
    print(f"logged in as {who} (sub={sub})")
    if roles:
        print(f"  roles: {', '.join(sorted(set(roles)))}")


# ---------------- run loops ----------------

async def _run_ask(provider, manager_url: str, send: str, timeout: float,
                   render_mode: str) -> int:
    async with ChatClient(manager_url, provider) as client:
        answer = await client.ask(send, timeout=timeout)
        print(f"Q: {send}")
        if render_mode == "raw":
            print(f"A: {answer.text}")
        else:
            print("A:")
            render_markdown(answer.text, render_mode)
    return EXIT_OK


async def _run_chat(provider, manager_url: str, timeout: float,
                    render_mode: str) -> int:
    client = ChatClient(manager_url, provider)
    try:
        return await run_repl(client, timeout, render_mode)
    finally:
        await client.close()


# ---------------- subcommands ----------------

def _cmd_login(args) -> int:
    issuer, client_id, project, store, _ = _user_session(args)
    ts = _login_and_store(issuer, client_id, project, store, args.oidc_port)
    _print_whoami(ts)
    return EXIT_OK


def _cmd_logout(args) -> int:
    issuer = _resolve_issuer(args)
    client_id = _resolve_client_id(args)
    store = TokenStore(issuer, client_id)
    ts = store.load()
    if ts and ts.refresh_token:
        endpoints = oidc.discover(issuer)
        oidc.revoke(endpoints.revoke, client_id=client_id, token=ts.refresh_token)
    store.clear()
    print("logged out.")
    return EXIT_OK


def _cmd_whoami(args) -> int:
    issuer, client_id, _, store, _ = _user_session(args)
    ts = store.load()
    if not ts:
        print("not logged in — run `llm-chat login`", file=sys.stderr)
        return EXIT_AUTH
    _print_whoami(ts)
    return EXIT_OK


def _cmd_chat_or_ask(args) -> int:
    mode = _auth_mode(args)
    manager_url = resolve_manager(args.manager)
    if mode == "machine":
        provider = _machine_provider(args)
    else:  # user — ensure logged in (browser) before connecting
        issuer, client_id, project, store, endpoints = _user_session(args)
        refresh_fn = lambda rt: oidc.refresh(endpoints.token, client_id=client_id, refresh_token=rt)  # noqa: E731
        try:
            store.valid_access_token(refresh_fn)
        except AuthError:
            print("Not logged in — starting browser login…")
            _login_and_store(issuer, client_id, project, store, args.oidc_port)
        provider = _user_provider(issuer, client_id, store, endpoints)
    render_mode = resolve_mode(plain=args.plain, raw=args.raw)
    if args.command == "ask":
        return asyncio.run(_run_ask(provider, manager_url, args.send, args.timeout, render_mode))
    return asyncio.run(_run_chat(provider, manager_url, args.timeout, render_mode))


# ---------------- dispatch ----------------

def _dispatch(args: argparse.Namespace) -> int:
    configure_logging(args.verbose)
    try:
        if args.command == "login":
            return _cmd_login(args)
        if args.command == "logout":
            return _cmd_logout(args)
        if args.command == "whoami":
            return _cmd_whoami(args)
        return _cmd_chat_or_ask(args)
    except CredentialError as e:
        print(f"error: {e}", file=sys.stderr)
        return EXIT_USAGE
    except AuthError as e:
        print(f"auth error: {e}", file=sys.stderr)
        return EXIT_AUTH
    except ManagerUnavailable as e:
        print(f"manager unavailable: {e}", file=sys.stderr)
        return EXIT_MANAGER
    except (ProtocolError, AnswerTimeout) as e:
        print(f"error: {e}", file=sys.stderr)
        return EXIT_ANSWER
    except KeyboardInterrupt:
        return EXIT_INTERRUPT
    except LlmChatError as e:
        print(f"error: {e}", file=sys.stderr)
        return EXIT_ANSWER


def _force_utf8() -> None:
    # Windows consoles default to cp1252 and choke on the spinner/emoji/colors.
    for stream in (sys.stdout, sys.stderr):
        try:
            stream.reconfigure(encoding="utf-8", errors="replace")  # type: ignore[attr-defined]
        except (AttributeError, ValueError):
            pass


def build_parser() -> argparse.ArgumentParser:
    p = argparse.ArgumentParser(
        prog="llm-chat",
        description="Client for the llm-chat manager's /chat WebSocket.",
    )
    p.add_argument("--version", action="version", version=f"llm-chat {__version__}")
    sub = p.add_subparsers(dest="command")

    ask = sub.add_parser("ask", help="send one question and print the answer (machine auth)")
    add_common_args(ask)
    ask.add_argument("--send", required=True, help="the question text")

    chat = sub.add_parser("chat", help="interactive multi-turn REPL (human login)")
    add_common_args(chat)

    for name, helptext in (("login", "browser sign-in; cache the session"),
                           ("logout", "revoke and clear the cached session"),
                           ("whoami", "show the cached user identity")):
        sp = sub.add_parser(name, help=helptext)
        add_common_args(sp)

    return p


def main(argv: list[str] | None = None) -> int:
    _force_utf8()
    parser = build_parser()
    args = parser.parse_args(argv)
    if args.command is None:           # bare `llm-chat` → interactive chat
        args = parser.parse_args(["chat", *(argv or [])])
    return _dispatch(args)


def ask_main(argv: list[str] | None = None) -> int:
    """Backward-compatible flat entry for the legacy llm_chat_client.py script:
    `--issuer/--project/--key-file/--manager/--send/--timeout`, no subcommand.
    Always machine auth (kabytech)."""
    _force_utf8()
    p = argparse.ArgumentParser(prog="llm_chat_client.py",
                                description="One-shot llm-chat question (legacy interface).")
    add_common_args(p)
    p.add_argument("--send", default="hello", help="the question text")
    args = p.parse_args(argv)
    args.command = "ask"
    if not args.auth:
        args.auth = "machine"
    return _dispatch(args)


if __name__ == "__main__":
    raise SystemExit(main())
