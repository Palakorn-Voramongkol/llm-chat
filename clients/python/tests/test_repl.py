"""Unit tests for REPL answer/usage/dir formatting + identity URL derivation."""

from __future__ import annotations

from llm_chat.config import identity_url
from llm_chat.repl import (
    format_answer, format_dir, format_usage, human_bytes, human_int,
)


def test_single_line_unchanged():
    assert format_answer("just one line", 8) == "just one line"


def test_multiline_continuation_is_indented():
    out = format_answer("1. Apple\n2. Banana\n3. Orange", len("Claude: "))
    assert out == "1. Apple\n        2. Banana\n        3. Orange"


def test_blank_line_not_indented():
    out = format_answer("para one\n\npara two", 4)
    assert out == "para one\n\n    para two"


def test_identity_url_swaps_path():
    assert identity_url("ws://127.0.0.1:7777/chat") == "ws://127.0.0.1:7777/identity"
    assert identity_url("wss://host.example:443/chat") == "wss://host.example:443/identity"
    assert identity_url("ws://h:7777") == "ws://h:7777/identity"


def test_human_int_groups_thousands():
    assert human_int(0) == "0"
    assert human_int(42) == "42"
    assert human_int(12345) == "12,345"
    assert human_int(1_000_000) == "1,000,000"


def test_human_bytes_scales():
    assert human_bytes(0) == "0 B"
    assert human_bytes(512) == "512 B"
    assert human_bytes(1024) == "1.0 KB"
    assert human_bytes(1024 * 1024) == "1.0 MB"


def test_format_usage_totals_and_daily():
    reply = {
        "type": "usage", "userId": "u9", "userName": "Jane Doe", "requests": 42,
        "charsIn": 12345, "charsOut": 67890, "files": 3, "fileBytes": 1048576,
        "lastUsed": "2026-06-26T17:30:00.000Z",
        "daily": [{"day": "2026-06-26", "requests": 12, "charsIn": 3456,
                   "charsOut": 12345, "files": 1, "fileBytes": 262144}],
    }
    s = format_usage(reply)
    assert "user       Jane Doe" in s  # server-resolved name, not the id
    assert "user       u9" not in s
    assert "requests   42" in s
    assert "chars in   12,345" in s
    assert "files      3 · 1.0 MB" in s
    assert "last used  2026-06-26T17:30:00.000Z" in s
    assert "2026-06-26   12 req · 3,456 in · 12,345 out · 1 files · 256.0 KB" in s


def test_format_usage_empty_daily():
    reply = {"userId": "u", "requests": 0, "charsIn": 0, "charsOut": 0,
             "files": 0, "fileBytes": 0, "daily": []}
    s = format_usage(reply)
    assert "(no usage in the last 7 days)" in s
    assert "files      0 · 0 B" in s


def test_format_dir_renders_tree():
    reply = {"type": "dir", "truncated": False, "entries": [
        {"path": "projects", "dir": True, "size": 0},
        {"path": "projects/main.rs", "dir": False, "size": 11},
        {"path": "todo.md", "dir": False, "size": 5},
    ]}
    s = format_dir(reply)
    assert "/ · 3 items" in s
    assert "\n  projects/" in s
    assert "\n    main.rs  11 B" in s
    assert "\n  todo.md  5 B" in s


def test_format_dir_empty_box():
    s = format_dir({"type": "dir", "truncated": False, "entries": []})
    assert "/ · 0 items" in s
    assert "(empty)" in s
