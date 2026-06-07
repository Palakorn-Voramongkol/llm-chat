#!/usr/bin/env python3
"""Backward-compatible one-shot client (thin shim).

The implementation now lives in the `llm_chat` package. This script preserves
the original interface used in the docs/tests:

    llm_chat_client.py --issuer ... --project ... --key-file ... \\
                       --manager ws://127.0.0.1:7777/chat --send "hello"

Prefer the installed command for new use: `llm-chat ask --send "hello"`.
"""

from __future__ import annotations

import os
import sys

# Make the sibling `llm_chat` package importable when run as a loose script.
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

from llm_chat.cli import ask_main  # noqa: E402

if __name__ == "__main__":
    raise SystemExit(ask_main())
