"""Shared CLI argument definitions and logging setup."""

from __future__ import annotations

import argparse
import logging


def add_common_args(p: argparse.ArgumentParser) -> None:
    """Add the connection/auth flags shared by the `ask` and `chat` subcommands.

    All default to None so `resolve_credentials()`/`resolve_manager()` can apply
    the flag > env > secrets-file precedence. There are no hardcoded fallbacks:
    a value absent from all three sources fails fast (fail-closed).
    """
    g = p.add_argument_group("connection")
    g.add_argument("--issuer", default=None,
                   help="Zitadel issuer URL (required: pass --issuer or set $ZITADEL_ISSUER)")
    g.add_argument("--project", default=None,
                   help="Zitadel project_id (default: $PROJECT_ID or secrets/project_id)")
    g.add_argument("--key-file", default=None,
                   help="machine-user JSON key (default: $KABYTECH_KEY or secrets/kabytech-key.json)")
    g.add_argument("--manager", default=None,
                   help="manager /chat WebSocket URL (required: pass --manager or set $MANAGER_WS)")
    g.add_argument("--timeout", type=float, default=120.0,
                   help="per-answer timeout in seconds (default: 120; high effort is slow)")
    g.add_argument("--auth", choices=["user", "machine"], default=None,
                   help="credential type (default: chat→user, ask→machine)")
    g.add_argument("--oidc-client-id", default=None,
                   help="OIDC client id (default: $OIDC_CLIENT_ID or secrets/oidc_client_id)")
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
