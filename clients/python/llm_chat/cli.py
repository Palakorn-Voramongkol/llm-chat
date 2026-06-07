"""Command-line entry point: ``llm-chat ask`` (one-shot) and ``llm-chat chat`` (REPL)."""

from __future__ import annotations

import argparse
import asyncio
import logging
import sys

from . import __version__
from .auth import Credentials, fetch_access_token, resolve_credentials
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
from .repl import run_repl

log = logging.getLogger("llm_chat.cli")

# Exit codes (documented in the README).
EXIT_OK = 0
EXIT_USAGE = 2
EXIT_AUTH = 3
EXIT_MANAGER = 4
EXIT_ANSWER = 5
EXIT_INTERRUPT = 130


def _token_provider(creds: Credentials):
    """Return an async callable that mints a fresh access token off-thread
    (so reconnects re-authenticate without blocking the event loop)."""
    return lambda: asyncio.to_thread(fetch_access_token, creds)


async def _run_ask(creds: Credentials, manager_url: str, send: str, timeout: float) -> int:
    async with ChatClient(manager_url, _token_provider(creds)) as client:
        answer = await client.ask(send, timeout=timeout)
        print(f"Q: {send}")
        print(f"A: {answer.text}")
    return EXIT_OK


async def _run_chat(creds: Credentials, manager_url: str, timeout: float) -> int:
    client = ChatClient(manager_url, _token_provider(creds))
    try:
        return await run_repl(client, timeout)
    finally:
        await client.close()


def _resolve(args: argparse.Namespace) -> tuple[Credentials, str]:
    creds = resolve_credentials(args.issuer, args.project, args.key_file)
    return creds, resolve_manager(args.manager)


def _dispatch(args: argparse.Namespace) -> int:
    configure_logging(args.verbose)
    try:
        creds, manager_url = _resolve(args)
    except CredentialError as e:
        print(f"error: {e}", file=sys.stderr)
        return EXIT_USAGE

    try:
        if args.command == "ask":
            return asyncio.run(_run_ask(creds, manager_url, args.send, args.timeout))
        return asyncio.run(_run_chat(creds, manager_url, args.timeout))
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
    except LlmChatError as e:  # any other client error
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

    ask = sub.add_parser("ask", help="send one question and print the answer")
    add_common_args(ask)
    ask.add_argument("--send", required=True, help="the question text")

    chat = sub.add_parser("chat", help="interactive multi-turn REPL (default)")
    add_common_args(chat)

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
    `--issuer/--project/--key-file/--manager/--send/--timeout`, no subcommand."""
    _force_utf8()
    p = argparse.ArgumentParser(prog="llm_chat_client.py",
                                description="One-shot llm-chat question (legacy interface).")
    add_common_args(p)
    p.add_argument("--send", default="hello", help="the question text")
    args = p.parse_args(argv)
    args.command = "ask"
    return _dispatch(args)


if __name__ == "__main__":
    raise SystemExit(main())
