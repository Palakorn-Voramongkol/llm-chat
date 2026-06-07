#!/usr/bin/env python3
"""Backward-compatible interactive REPL (thin shim).

The implementation now lives in the `llm_chat` package. Prefer the installed
command for new use: `llm-chat chat` (or just `llm-chat`).

    llm_chat_repl.py [--issuer ... --project ... --key-file ... --manager ...]
"""

from __future__ import annotations

import os
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

from llm_chat.cli import main  # noqa: E402

if __name__ == "__main__":
    # Force the `chat` subcommand so the bare script stays interactive.
    raise SystemExit(main(["chat", *sys.argv[1:]]))
