"""Shared CLI argument definitions and logging setup."""

from __future__ import annotations

import argparse
import logging

from .auth import DEFAULT_ISSUER


def add_common_args(p: argparse.ArgumentParser) -> None:
    """Add the connection/auth flags shared by the `ask` and `chat` subcommands.

    All default to None so `resolve_credentials()` can apply the env / secrets
    precedence; only --issuer and --manager get literal fallbacks.
    """
    g = p.add_argument_group("connection")
    g.add_argument("--issuer", default=None,
                   help=f"Zitadel issuer URL (default: $ZITADEL_ISSUER or {DEFAULT_ISSUER})")
    g.add_argument("--project", default=None,
                   help="Zitadel project_id (default: $PROJECT_ID or secrets/project_id)")
    g.add_argument("--key-file", default=None,
                   help="machine-user JSON key (default: $KABYTECH_KEY or secrets/kabytech-key.json)")
    g.add_argument("--manager", default=None,
                   help="manager /chat WebSocket URL (default: $MANAGER_WS or ws://127.0.0.1:7777/chat)")
    g.add_argument("--timeout", type=float, default=120.0,
                   help="per-answer timeout in seconds (default: 120; high effort is slow)")
    p.add_argument("-v", "--verbose", action="count", default=0,
                   help="-v for INFO, -vv for DEBUG diagnostics on stderr")


def resolve_manager(manager: str | None) -> str:
    import os
    return manager or os.environ.get("MANAGER_WS", "ws://127.0.0.1:7777/chat")


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
