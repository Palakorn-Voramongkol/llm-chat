"""Unit tests for terminal markdown rendering (display modes).

These assert the contract that matters: 'raw' is byte-exact, 'plain' obeys
markdown without emitting any ANSI, 'auto' degrades to plain on a non-tty, and a
missing rich install falls back to raw instead of crashing.
"""

from __future__ import annotations

import io
import sys

from llm_chat.render import render_markdown, resolve_mode

SAMPLE = "## Heading\n\n- **bold** item\n- second\n\nplain line\n"


def test_resolve_mode():
    assert resolve_mode() == "auto"
    assert resolve_mode(plain=True) == "plain"
    assert resolve_mode(raw=True) == "raw"
    assert resolve_mode(plain=True, raw=True) == "raw"  # raw wins over plain


def test_raw_is_verbatim():
    buf = io.StringIO()
    render_markdown(SAMPLE, "raw", file=buf)
    assert buf.getvalue() == SAMPLE  # exact bytes — control chars intact


def test_raw_adds_trailing_newline_when_missing():
    buf = io.StringIO()
    render_markdown("no newline", "raw", file=buf)
    assert buf.getvalue() == "no newline\n"


def test_plain_has_no_ansi_and_obeys_markdown():
    buf = io.StringIO()
    render_markdown(SAMPLE, "plain", file=buf)
    out = buf.getvalue()
    assert "\x1b" not in out          # no ANSI escape codes at all
    assert "##" not in out            # heading marker rendered away
    assert "**" not in out            # bold markers rendered away
    assert "Heading" in out and "bold" in out  # content preserved
    assert "•" in out            # list item rendered as a bullet


def test_auto_to_non_tty_is_plain():
    # A StringIO is not a tty → rich auto-degrades to no-color plain text.
    buf = io.StringIO()
    render_markdown(SAMPLE, "auto", file=buf)
    out = buf.getvalue()
    assert "\x1b" not in out
    assert "##" not in out


def test_rich_missing_falls_back_to_raw(monkeypatch):
    # Simulate a broken/partial install where rich.markdown cannot import.
    monkeypatch.setitem(sys.modules, "rich.markdown", None)
    buf = io.StringIO()
    render_markdown(SAMPLE, "auto", file=buf)
    assert buf.getvalue() == SAMPLE  # degrades to raw, does not crash
