"""Unit tests for REPL answer formatting + the /status block formatter."""

from __future__ import annotations

from llm_chat.repl import ReplCtx, format_answer, format_status


def test_single_line_unchanged():
    assert format_answer("just one line", 8) == "just one line"


def test_multiline_continuation_is_indented():
    out = format_answer("1. Apple\n2. Banana\n3. Orange", len("Claude: "))
    assert out == "1. Apple\n        2. Banana\n        3. Orange"


def test_blank_line_not_indented():
    out = format_answer("para one\n\npara two", 4)
    assert out == "para one\n\n    para two"


def _ctx() -> ReplCtx:
    return ReplCtx(
        kind="python", version="1.0.0", auth_label="machine (kabytech key)",
        issuer="http://iss:8080", project="P123", manager_url="ws://m:7777/chat",
    )


def test_format_status_includes_all_fields():
    s = format_status(_ctx(), "admin@example.com", "U9",
                      ["chat.admin", "chat.user"], True, "s1", 2, "auto", 120.0)
    assert "llm-chat · python · v1.0.0" in s
    assert "machine (kabytech key)" in s
    assert "user      admin@example.com" in s
    assert "sub     U9" in s
    assert "roles   chat.admin, chat.user" in s
    assert "ws://m:7777/chat · connected" in s
    assert "session   s1 · 2 msgs this session" in s
    assert "issuer    http://iss:8080" in s
    assert "project   P123" in s
    assert "render=auto · timeout=120s" in s


def test_format_status_empty_roles_and_no_session():
    s = format_status(_ctx(), "who", "sub", [], False, None, 0, "raw", 60.0)
    assert "roles   —" in s
    assert "session   — · 0 msgs" in s
    assert "ws://m:7777/chat · disconnected" in s
    assert "render=raw · timeout=60s" in s
