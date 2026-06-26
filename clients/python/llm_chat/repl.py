"""Interactive REPL: a colored, multi-turn chat over a persistent session."""

from __future__ import annotations

import asyncio
import itertools
import os
import sys
import time
from dataclasses import dataclass

from .errors import AnswerTimeout, ManagerUnavailable, ProtocolError
from .protocol import Answer, ChatClient
from .render import MODES, render_markdown


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
  /status          show your identity + client/connection status
  /usage           show your own usage (totals + last 7 days)
  /render MODE     switch markdown display: auto | plain | raw
  /reset           drop the session and start a fresh one (clears claude context)
  /multi           enter a multi-line message (end with '.')
  /quit, /exit     leave
anything else is sent to claude on the same (context-preserving) session."""

STATUS_RULE = "─────────────────────────────────────────────"


@dataclass(frozen=True)
class ReplCtx:
    """Static context for the REPL's /status block (dynamic bits read live)."""

    kind: str            # "python"
    version: str
    auth_label: str      # "human (browser login)" | "machine (kabytech key)"
    issuer: str
    project: str
    manager_url: str


def format_status(ctx: ReplCtx, who: str, sub: str, roles: list[str],
                  connected: bool, session_id: str | None, msgs: int,
                  render: str, timeout_s: float) -> str:
    """PURE: render the /status block (roles already sorted/deduped). Matches
    the Rust client's layout (only `kind` differs) — keep the two in sync."""
    roles_str = ", ".join(roles) if roles else "—"
    conn = "connected" if connected else "disconnected"
    sid = session_id if session_id else "—"
    return (
        "─ status ───────────────────────────────────\n"
        f" client    llm-chat · {ctx.kind} · v{ctx.version}\n"
        f" auth      {ctx.auth_label}\n"
        f" user      {who}\n"
        f"   sub     {sub}\n"
        f"   roles   {roles_str}\n"
        f" manager   {ctx.manager_url} · {conn}\n"
        f" session   {sid} · {msgs} msgs this session\n"
        f" issuer    {ctx.issuer}\n"
        f" project   {ctx.project}\n"
        f" display   render={render} · timeout={int(timeout_s)}s\n"
        f"{STATUS_RULE}"
    )


def human_int(n: int) -> str:
    """PURE: integer with thousands separators (12345 -> '12,345')."""
    return f"{n:,}"


def human_bytes(n: int) -> str:
    """PURE: human-readable byte size (0 -> '0 B', 1024 -> '1.0 KB')."""
    if n < 1024:
        return f"{n} B"
    units = ("KB", "MB", "GB", "TB")
    v = n / 1024.0
    u = 0
    while v >= 1024.0 and u < len(units) - 1:
        v /= 1024.0
        u += 1
    return f"{v:.1f} {units[u]}"


def _as_int(obj: dict, k: str) -> int:
    v = obj.get(k, 0)
    return v if isinstance(v, int) else 0


def format_usage(reply: dict) -> str:
    """PURE: render the /usage block from the manager's `usage` reply. Matches
    the Rust client's layout — keep the two in sync."""
    user = reply.get("userId") or "—"
    last = reply.get("lastUsed") or "—"
    lines = [
        "─ usage ─────────────────────────────────────",
        f" user       {user}",
        f" requests   {human_int(_as_int(reply, 'requests'))}",
        f" chars in   {human_int(_as_int(reply, 'charsIn'))}",
        f" chars out  {human_int(_as_int(reply, 'charsOut'))}",
        f" files      {human_int(_as_int(reply, 'files'))} · {human_bytes(_as_int(reply, 'fileBytes'))}",
        f" last used  {last}",
        " ── last 7 days ──",
    ]
    daily = reply.get("daily") or []
    if daily:
        for d in daily:
            day = d.get("day") or "?"
            lines.append(
                f" {day}   {human_int(_as_int(d, 'requests'))} req · "
                f"{human_int(_as_int(d, 'charsIn'))} in · {human_int(_as_int(d, 'charsOut'))} out · "
                f"{human_int(_as_int(d, 'files'))} files · {human_bytes(_as_int(d, 'fileBytes'))}"
            )
    else:
        lines.append(" (no usage in the last 7 days)")
    lines.append(STATUS_RULE)
    return "\n".join(lines)


def _print_answer(c: _Ansi, text: str, render_mode: str, latency_s: float | None) -> None:
    """Print the 'Claude:' label, then render the answer body as a block.

    The body is rendered as markdown (display only — the text is claude's exact
    output); raw mode prints it verbatim. A label line keeps headings/tables
    left-aligned instead of starting awkwardly after an inline 'Claude: '."""
    print(c.claude("Claude:"))
    render_markdown(text, render_mode)
    if latency_s is not None:
        print(c.dim(f"({latency_s:0.1f}s)"))
    print()


async def run_repl(client: ChatClient, ctx: ReplCtx, timeout: float, render_mode: str = "auto") -> int:
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
        if user == "/status":
            # Re-mint/refresh the token and decode its identity live. Local
            # import avoids a cli<->repl import cycle.
            note = None
            try:
                tok = await client.current_token()
                from .cli import _identity_from_token
                who, sub, roles = _identity_from_token(tok)
            except Exception as e:  # noqa: BLE001 — display surface; never crash /status
                who, sub, roles, note = "(could not read token)", "—", [], str(e)
            print(c.dim(format_status(
                ctx, who, sub, roles, client.connected,
                client.session_id, len(history), render_mode, timeout)))
            if note:
                print(c.err(f"  token error: {note}"))
            print()
            continue
        if user == "/usage":
            try:
                reply = await client.usage(timeout=timeout)
                print(c.dim(format_usage(reply)) + "\n")
            except (AnswerTimeout, ProtocolError, ManagerUnavailable) as e:
                print(c.err(f"usage unavailable: {e}") + "\n")
            continue
        if user == "/history":
            if not history:
                print(c.dim("(no messages yet)\n"))
            for i, (q, a) in enumerate(history, 1):
                print(f"{c.you(f'You[{i}]:')} {q}")
                print(c.claude(f"Claude[{i}]:"))
                render_markdown(a, render_mode)
                print()
            continue
        if user.startswith("/render"):
            parts = user.split()
            if len(parts) == 2 and parts[1] in MODES:
                render_mode = parts[1]
                print(c.dim(f"render mode: {render_mode}\n"))
            else:
                print(c.dim(f"usage: /render {'|'.join(MODES)} (current: {render_mode})\n"))
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
        _print_answer(c, answer.text, render_mode, answer.latency_s)

    print(c.dim("bye"))
    return 0
