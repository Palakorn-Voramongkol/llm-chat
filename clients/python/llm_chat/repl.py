"""Interactive REPL: a colored, multi-turn chat over a persistent session."""

from __future__ import annotations

import asyncio
import itertools
import os
import sys
import time

from .errors import AnswerTimeout, ManagerUnavailable, ProtocolError
from .protocol import Answer, ChatClient


class _Ansi:
    """Minimal ANSI styling, disabled when output isn't a TTY or NO_COLOR is set."""

    def __init__(self, enabled: bool) -> None:
        self.enabled = enabled

    def _wrap(self, code: str, s: str) -> str:
        return f"\x1b[{code}m{s}\x1b[0m" if self.enabled else s

    def you(self, s: str) -> str:
        return self._wrap("1;36", s)   # bold cyan

    def claude(self, s: str) -> str:
        return self._wrap("1;33", s)   # bold yellow

    def dim(self, s: str) -> str:
        return self._wrap("2", s)

    def err(self, s: str) -> str:
        return self._wrap("1;31", s)


def _color_enabled() -> bool:
    return sys.stdout.isatty() and os.environ.get("NO_COLOR") is None


async def _spinner(stop: asyncio.Event, label: str) -> None:
    if not sys.stdout.isatty():
        await stop.wait()
        return
    frames = itertools.cycle("⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏")
    t0 = time.monotonic()
    try:
        while not stop.is_set():
            sys.stdout.write(f"\r{label} {next(frames)} thinking… ({time.monotonic()-t0:0.0f}s)   ")
            sys.stdout.flush()
            await asyncio.sleep(0.12)
    except asyncio.CancelledError:
        pass
    finally:
        sys.stdout.write("\r" + " " * 48 + "\r")
        sys.stdout.flush()


def _read_line(prompt: str) -> str | None:
    try:
        return input(prompt)
    except (EOFError, KeyboardInterrupt):
        return None


def format_answer(text: str, indent_len: int) -> str:
    """Indent continuation logical lines so a multi-line answer aligns under the
    first line (the text after a "Claude: "-style label). Newlines preserved by
    the worker parser are honored; the terminal soft-wraps any over-long line."""
    lines = text.split("\n")
    if len(lines) <= 1:
        return text
    indent = " " * indent_len
    out = lines[0]
    for ln in lines[1:]:
        out += "\n" + (indent + ln if ln else "")
    return out


async def _read_multiline(c: _Ansi) -> str | None:
    print(c.dim("(multi-line: end with a single '.' on its own line)"))
    lines: list[str] = []
    while True:
        line = await asyncio.to_thread(_read_line, c.dim("… "))
        if line is None:
            return None
        if line.strip() == ".":
            break
        lines.append(line)
    return "\n".join(lines)


HELP = """commands:
  /help            show this help
  /history         print this session's Q&A so far
  /session         show the backend session id
  /reset           drop the session and start a fresh one (clears claude context)
  /multi           enter a multi-line message (end with '.')
  /quit, /exit     leave
anything else is sent to claude on the same (context-preserving) session."""


async def run_repl(client: ChatClient, timeout: float) -> int:
    """Run the interactive loop until the user quits. Returns an exit code."""
    c = _Ansi(_color_enabled())
    try:
        await client.connect()
    except ManagerUnavailable as e:
        print(c.err(f"cannot connect: {e}"), file=sys.stderr)
        return 2

    print(c.dim(f"connected — session {client.session_id}"))
    print(c.dim("type a message, /help for commands. first reply includes warm-up.\n"))
    history: list[tuple[str, str]] = []

    while True:
        user = await asyncio.to_thread(_read_line, c.you("You: "))
        if user is None:
            break
        user = user.strip()
        if not user:
            continue

        if user in ("/quit", "/exit"):
            break
        if user == "/help":
            print(c.dim(HELP) + "\n")
            continue
        if user == "/session":
            print(c.dim(f"session {client.session_id}\n"))
            continue
        if user == "/history":
            if not history:
                print(c.dim("(no messages yet)\n"))
            for i, (q, a) in enumerate(history, 1):
                print(f"{c.you(f'You[{i}]:')} {q}")
                print(f"{c.claude(f'Claude[{i}]:')} {a}\n")
            continue
        if user == "/reset":
            await client.close()
            try:
                await client.connect()
            except ManagerUnavailable as e:
                print(c.err(f"reconnect failed: {e}"), file=sys.stderr)
                return 2
            history.clear()
            print(c.dim(f"fresh session — {client.session_id}\n"))
            continue
        if user == "/multi":
            user = await _read_multiline(c)
            if not user:
                continue

        stop = asyncio.Event()
        spin = asyncio.create_task(_spinner(stop, c.claude("Claude:")))
        try:
            answer: Answer = await client.ask(user, timeout=timeout)
        except AnswerTimeout:
            stop.set(); await asyncio.gather(spin, return_exceptions=True)
            print(c.err(f"Claude: [no answer within {timeout:g}s]") + "\n")
            continue
        except ProtocolError as e:
            stop.set(); await asyncio.gather(spin, return_exceptions=True)
            print(c.err(f"Claude: [error] {e}") + "\n")
            continue
        except ManagerUnavailable as e:
            stop.set(); await asyncio.gather(spin, return_exceptions=True)
            print(c.err(f"[connection lost] {e}"), file=sys.stderr)
            return 2
        stop.set()
        await asyncio.gather(spin, return_exceptions=True)

        history.append((user, answer.text))
        body = format_answer(answer.text, len("Claude: "))
        line = f"{c.claude('Claude:')} {body}"
        if answer.latency_s is not None:
            line += c.dim(f"  ({answer.latency_s:0.1f}s)")
        print(line + "\n")

    print(c.dim("bye"))
    return 0
