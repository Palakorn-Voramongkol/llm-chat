"""Unit tests for REPL answer formatting (newline-preserving alignment)."""

from __future__ import annotations

from llm_chat.repl import format_answer


def test_single_line_unchanged():
    assert format_answer("just one line", 8) == "just one line"


def test_multiline_continuation_is_indented():
    out = format_answer("1. Apple\n2. Banana\n3. Orange", len("Claude: "))
    assert out == "1. Apple\n        2. Banana\n        3. Orange"


def test_blank_line_not_indented():
    out = format_answer("para one\n\npara two", 4)
    assert out == "para one\n\n    para two"
