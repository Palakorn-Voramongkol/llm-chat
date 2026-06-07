"""Render claude's markdown answers for terminal display.

DISPLAY ONLY. claude's exact markdown (the source of truth) is received and
stored unchanged; this module only controls how it is *printed* — the same way a
browser renders the same markdown bytes to HTML. We hand the whole markdown to a
real renderer (``rich``); we never strip characters or reverse-engineer
structure with regex.

Modes:
  auto  - styled (color / bold / tables) when the terminal supports it; ``rich``
          automatically falls back to plain text when output is piped, ``NO_COLOR``
          is set, or ``TERM=dumb``. ANSI is text, not a GUI, so this works on a
          headless Linux CLI and over SSH — no display server required.
  plain - markdown obeyed but ZERO ANSI color/styling (``color_system=None``):
          headings, bullets and tables still render, but as plain text. Good for
          dumb terminals, log files and copy-paste.
  raw   - the literal markdown exactly as received (escape hatch / debugging).
"""

from __future__ import annotations

import sys
from typing import TextIO

MODES = ("auto", "plain", "raw")


def resolve_mode(*, plain: bool = False, raw: bool = False) -> str:
    """Map the ``--plain`` / ``--raw`` flags to a mode string (raw wins)."""
    if raw:
        return "raw"
    if plain:
        return "plain"
    return "auto"


def _console(mode: str, file: TextIO):
    from rich.console import Console

    if mode == "plain":
        # No color system at all → no ANSI escapes, but markdown structure
        # (headings, bullets, tables) is still rendered as plain text.
        return Console(file=file, color_system=None)
    # auto: let rich detect tty / NO_COLOR / dumb terminal and degrade itself.
    return Console(file=file)


def _write_raw(text: str, out: TextIO) -> None:
    out.write(text if text.endswith("\n") else text + "\n")
    out.flush()


def render_markdown(text: str, mode: str = "auto", *, file: TextIO | None = None) -> None:
    """Print ``text`` (claude's markdown) to ``file`` (default stdout) per ``mode``."""
    out = file or sys.stdout
    if mode == "raw":
        _write_raw(text, out)
        return
    try:
        from rich.markdown import Markdown
    except ImportError:
        # rich is a declared dependency; this only bites a broken/partial
        # install. Degrade to raw rather than crash.
        _write_raw(text, out)
        return
    _console(mode, out).print(Markdown(text))
