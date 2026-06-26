"""Shared CLI argument definitions, `.env.local` loading, and logging setup."""

from __future__ import annotations

import argparse
import logging
import os


def load_env_local() -> None:
    """Load connection settings from the repo-root ``.env.local`` into the
    process env (the SOLE source). ``override=False`` so a real env var / --flag
    still wins. A missing file is fine — resolution then fails closed on
    whatever is absent. Override the path via ``$LLM_CHAT_ENV_FILE``.
    """
    from dotenv import load_dotenv

    # <repo>/clients/python/llm_chat/config.py -> <repo>/.env.local
    path = os.environ.get("LLM_CHAT_ENV_FILE") or os.path.abspath(
        os.path.join(os.path.dirname(__file__), "..", "..", "..", ".env.local")
    )
    load_dotenv(path, override=False)


def add_common_args(p: argparse.ArgumentParser) -> None:
    """Add the connection/auth flags shared by the `ask` and `chat` subcommands.

    All default to None so `resolve_credentials()`/`resolve_manager()` apply the
    flag > env precedence (the env fed by `.env.local`). There is no secrets/
    fallback and no hardcoded default: a value absent from both the flag and the
    env fails fast (fail-closed).
    """
    g = p.add_argument_group("connection")
    g.add_argument("--issuer", default=None,
                   help="Zitadel issuer URL (required: --issuer or ZITADEL_ISSUER in .env.local)")
    g.add_argument("--project", default=None,
                   help="Zitadel project_id (required: --project or PROJECT_ID in .env.local)")
    g.add_argument("--key-file", default=None,
                   help="machine-user JSON key (required: --key-file or KABYTECH_KEY in .env.local)")
    g.add_argument("--manager", default=None,
                   help="manager /chat WebSocket URL (required: --manager or MANAGER_WS in .env.local)")
    g.add_argument("--timeout", type=float, default=120.0,
                   help="per-answer timeout in seconds (default: 120; high effort is slow)")
    g.add_argument("--auth", choices=["user", "machine"], default=None,
                   help="credential type (default: chat→user, ask→machine)")
    g.add_argument("--oidc-client-id", default=None,
                   help="OIDC client id (required: --oidc-client-id or OIDC_CLIENT_ID in .env.local)")
    g.add_argument("--oidc-port", type=int, default=8477,
                   help="loopback port for the browser login redirect (default: 8477)")

    d = p.add_argument_group("display")
    d.add_argument("--plain", action="store_true",
                   help="render markdown as plain text (no ANSI color/styling) — "
                        "good for dumb terminals, logs, copy-paste")
    d.add_argument("--raw", action="store_true",
                   help="print claude's literal markdown without rendering")

    p.add_argument("-v", "--verbose", action="count", default=0,
                   help="-v for INFO, -vv for DEBUG diagnostics on stderr")


def resolve_manager(manager: str | None) -> str:
    import os

    from .errors import CredentialError

    resolved = manager or os.environ.get("MANAGER_WS")
    if not resolved:
        raise CredentialError(
            "no manager URL: pass --manager or set MANAGER_WS "
            "(e.g. ws://127.0.0.1:7777/chat)"
        )
    return resolved


def identity_url(manager_ws: str) -> str:
    """Derive the `/identity` URL from the manager `/chat` URL: same scheme +
    host:port, path replaced with `/identity` (the manager serves both)."""
    scheme, sep, rest = manager_ws.partition("://")
    if not sep:
        return manager_ws
    authority = rest.split("/", 1)[0]
    return f"{scheme}://{authority}/identity"


def configure_logging(verbosity: int) -> None:
    """Map -v/-vv to logging levels. Diagnostics go to stderr so they don't mix
    with the chat transcript on stdout."""
    import sys

    level = logging.WARNING
    if verbosity == 1:
        level = logging.INFO
    elif verbosity >= 2:
        level = logging.DEBUG
    logging.basicConfig(
        level=level,
        stream=sys.stderr,
        format="%(asctime)s %(levelname)-5s %(name)s: %(message)s",
        datefmt="%H:%M:%S",
    )
