#!/usr/bin/env python3
"""Minimal interactive chat client for the llm-chat manager.

Run it, type a question, press Enter, watch claude reply. Ctrl+C or Ctrl+D
exits. All auth + endpoint settings are baked in below — no CLI flags
needed beyond optional `--attach <path>` for image/PDF uploads.

Protocol it speaks (server-defined; this client is just a pipe):

    server → {"type":"initialized","sid":"…","backendPort":N,"connectionId":"…","timeOut":"…"}
    client → {"type":"q","id":"<opaque>","text":"<question>"}
    server → {"type":"ack","id":"<opaque>","seq":N,"timeIn":"…"}
    server → {"type":"a","id":"<opaque>","seq":N,"text":"<answer>","timeIn":"…","timeOut":"…"}
    client → {"type":"confirm","id":"<opaque>","seq":N}                              (auto)
    server → {"type":"err",...}
"""
from __future__ import annotations

import argparse
import asyncio
import base64
import json
import mimetypes
import os
import signal
import sys
import time
from pathlib import Path

import jwt as pyjwt
import requests
import websockets


# ─── Embedded configuration ────────────────────────────────────────────────
# At startup we probe the local manager port. If it's listening (you're on
# the same box as the manager) we use the plain-WS loopback URL; otherwise
# we transparently fall back to the public TLS endpoint via nginx.
#
# Multiple machine-user keys are supported. Add a (label, path) pair to
# ACCOUNTS when a new account is provisioned in Zitadel. If exactly one
# key file exists, we use it silently; if more than one, we present a
# menu at startup.
MANAGER_LOCAL   = "ws://127.0.0.1:7777/chat"
MANAGER_REMOTE  = "wss://id.palakorn.com/chat"
LOCAL_PROBE     = ("127.0.0.1", 7777)
ZITADEL_ISSUER  = "https://id.palakorn.com"
ZITADEL_PROJECT = "370627061150121985"
ACCOUNTS = [
    ("kabytech",  Path(os.path.expanduser("~/.config/llm-chat/kabytech-key.json"))),
    ("corridraw", Path(os.path.expanduser("~/.config/llm-chat/corridraw-key.json"))),
    ("palakorn",  Path(os.path.expanduser("~/.config/llm-chat/palakorn-key.json"))),
]
# ───────────────────────────────────────────────────────────────────────────


def pick_account(preselected: str | None,
                 force_default: bool = False) -> tuple[str, Path]:
    """Resolve which account/key to use. Order:
       1. --account <label> on CLI → exact match,
       2. exactly one configured key file present → silent pick,
       3. force_default=True (one-shot / non-TTY) → first available silently,
       4. multiple keys + interactive TTY → numbered menu on stderr.
    """
    available = [(label, path) for label, path in ACCOUNTS if path.is_file()]
    if not available:
        sys.exit("no machine-user key found in any of: "
                 + ", ".join(str(p) for _, p in ACCOUNTS))
    if preselected:
        for label, path in available:
            if label == preselected:
                return label, path
        sys.exit(f"--account {preselected!r} not found "
                 f"(available: {', '.join(l for l, _ in available)})")
    if len(available) == 1:
        return available[0]
    if force_default or not sys.stdin.isatty():
        # Non-interactive: pick the first available without prompting,
        # so `chat --send …` and CI pipelines don't hang on a menu.
        first = available[0]
        print(f"-- non-interactive run; using first account: {first[0]}",
              file=sys.stderr)
        return first
    # Interactive menu.
    print("Select account:", file=sys.stderr)
    for i, (label, path) in enumerate(available, 1):
        print(f"  {i}. {label}  ({path})", file=sys.stderr)
    while True:
        try:
            choice = input("> ").strip()
        except EOFError:
            sys.exit("no account chosen")
        if choice.isdigit():
            n = int(choice)
            if 1 <= n <= len(available):
                return available[n - 1]
        for label, path in available:
            if choice == label:
                return label, path
        print("invalid choice; type a number or label", file=sys.stderr)


def pick_manager_url() -> str:
    """Use the local loopback URL if 127.0.0.1:7777 accepts a TCP connection;
    otherwise use the public WSS endpoint."""
    import socket
    s = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
    s.settimeout(0.3)
    try:
        s.connect(LOCAL_PROBE)
        s.close()
        return MANAGER_LOCAL
    except (OSError, socket.timeout):
        return MANAGER_REMOTE


# Server-side whitelist mirrored here so we fail fast with a helpful error.
ALLOWED_ATTACH_MIME = {
    "image/png", "image/jpeg", "image/jpg", "image/gif", "image/webp",
    "application/pdf",
}


def load_attachment(path: str) -> dict:
    """Read a file from disk and return {name, mime, data:base64}."""
    p = Path(path)
    if not p.is_file():
        sys.exit(f"--attach: not a file: {path}")
    mime, _ = mimetypes.guess_type(p.name)
    if not mime:
        sys.exit(f"--attach: cannot infer MIME type from extension: {p.name}")
    if mime not in ALLOWED_ATTACH_MIME:
        sys.exit(f"--attach: MIME type not allowed: {mime} (allowed: {sorted(ALLOWED_ATTACH_MIME)})")
    raw = p.read_bytes()
    return {
        "name": p.name,
        "mime": mime,
        "data": base64.b64encode(raw).decode("ascii"),
    }


def mint_access_token(key_path: Path) -> str:
    """JWT-bearer flow against Zitadel using the given machine-user key."""
    if not key_path.is_file():
        sys.exit(f"key file not found: {key_path}")
    with key_path.open() as f:
        key = json.load(f)
    now = int(time.time())
    assertion = pyjwt.encode(
        {"iss": key["userId"], "sub": key["userId"], "aud": ZITADEL_ISSUER,
         "iat": now, "exp": now + 300},
        key["key"], algorithm="RS256",
        headers={"kid": key["keyId"]},
    )
    r = requests.post(
        f"{ZITADEL_ISSUER.rstrip('/')}/oauth/v2/token",
        data={
            "grant_type": "urn:ietf:params:oauth:grant-type:jwt-bearer",
            "scope": (f"openid profile "
                      f"urn:zitadel:iam:org:project:id:{ZITADEL_PROJECT}:aud "
                      f"urn:zitadel:iam:org:projects:roles"),
            "assertion": assertion,
        },
        timeout=15,
    )
    r.raise_for_status()
    return r.json()["access_token"]


async def make_stdin_reader() -> asyncio.StreamReader:
    """Wrap stdin (binary) in an asyncio StreamReader."""
    loop = asyncio.get_running_loop()
    reader = asyncio.StreamReader()
    protocol = asyncio.StreamReaderProtocol(reader)
    await loop.connect_read_pipe(lambda: protocol, sys.stdin.buffer)
    return reader


async def chat_oneshot(manager_url: str, token: str, text: str,
                       attachments: list[dict], timeout: float) -> int:
    """Send a single message, wait for the matching `a` frame, print just
    the answer text on stdout, exit. Used by CI / scripted callers via
    `--send`. Returns 0 on success, 1 on timeout, 2 on protocol error."""
    deadline = time.time() + timeout
    async with websockets.connect(
        manager_url,
        extra_headers=[("Authorization", f"Bearer {token}")],
        max_size=None,
    ) as ws:
        # Wait for `initialized` so we know the session is up before sending.
        first = await asyncio.wait_for(ws.recv(), timeout=15)
        try:
            init = json.loads(first)
        except json.JSONDecodeError:
            print(f"non-JSON first frame: {first!r}", file=sys.stderr)
            return 2
        if init.get("type") != "initialized":
            print(f"unexpected first frame: {init}", file=sys.stderr)
            return 2
        print(f"-- session sid={init.get('sid')}", file=sys.stderr)

        msg = {"type": "q", "id": "oneshot-1", "text": text}
        if attachments:
            msg["attachments"] = attachments
        await ws.send(json.dumps(msg))

        while True:
            remaining = deadline - time.time()
            if remaining <= 0:
                print("[ws] FAIL - no answer within timeout", file=sys.stderr)
                return 1
            try:
                raw = await asyncio.wait_for(ws.recv(), timeout=remaining)
            except asyncio.TimeoutError:
                continue
            try:
                m = json.loads(raw)
            except json.JSONDecodeError:
                continue
            t = m.get("type")
            if t == "ack":
                continue
            if t == "err":
                print(f"[err] {m.get('text')}", file=sys.stderr)
                return 2
            if t == "a":
                await ws.send(json.dumps({"type": "confirm",
                                          "id": m.get("id"),
                                          "seq": m.get("seq")}))
                print(m.get("text", ""))
                return 0


async def chat_loop(manager_url: str, token: str, attachments: list[dict]) -> None:
    stdin_reader = await make_stdin_reader()
    pending_attachments = list(attachments or [])
    async with websockets.connect(
        manager_url,
        extra_headers=[("Authorization", f"Bearer {token}")],
        max_size=None,
    ) as ws:
        loop = asyncio.get_running_loop()
        stop = asyncio.Event()
        next_id = 1

        async def reader() -> None:
            """Print every server message verbatim, one per line.

            `initialized` goes to STDERR so stdout stays pure. After each
            `a` we auto-send a `confirm` so the manager can mark the row
            as delivered.
            """
            try:
                async for msg in ws:
                    text = msg if isinstance(msg, str) else msg.decode("utf-8", "replace")
                    parsed = None
                    try:
                        parsed = json.loads(text)
                    except json.JSONDecodeError:
                        pass
                    msg_type = parsed.get("type") if isinstance(parsed, dict) else None
                    out = sys.stderr if msg_type == "initialized" else sys.stdout
                    out.write(text + "\n")
                    out.flush()
                    if msg_type == "a":
                        confirm = {
                            "type": "confirm",
                            "id": parsed.get("id"),
                            "seq": parsed.get("seq"),
                        }
                        try:
                            await ws.send(json.dumps(confirm))
                        except Exception:
                            pass
            except (websockets.exceptions.ConnectionClosed, asyncio.CancelledError):
                pass
            finally:
                stop.set()

        async def writer() -> None:
            nonlocal next_id, pending_attachments
            try:
                while not stop.is_set():
                    raw = await stdin_reader.readline()
                    if not raw:  # EOF (Ctrl+D)
                        break
                    text = raw.decode("utf-8", errors="replace").rstrip("\n")
                    if not text:
                        continue
                    msg = {"type": "q", "id": str(next_id), "text": text}
                    if pending_attachments:
                        msg["attachments"] = pending_attachments
                        pending_attachments = []  # only the first q gets them
                    next_id += 1
                    await ws.send(json.dumps(msg))
            finally:
                stop.set()
                try:
                    await asyncio.wait_for(ws.close(), timeout=2.0)
                except (asyncio.TimeoutError, Exception):
                    pass

        def _sigint(*_a):
            stop.set()
            try:
                loop.create_task(ws.close())
            except RuntimeError:
                pass

        loop.add_signal_handler(signal.SIGINT, _sigint)
        reader_task = asyncio.create_task(reader())
        writer_task = asyncio.create_task(writer())
        done, pending = await asyncio.wait(
            {reader_task, writer_task},
            return_when=asyncio.FIRST_COMPLETED,
        )
        for t in pending:
            t.cancel()
        await asyncio.gather(*pending, return_exceptions=True)


def main() -> None:
    p = argparse.ArgumentParser(description=__doc__.split("\n\n", 1)[0])
    p.add_argument(
        "cwd", nargs="?", default=None,
        help="optional working directory the worker should spawn claude in. "
             "Resolved to an absolute path on the client side and sent to "
             "the manager as `?cwd=...`. The worker auto-trusts it in "
             "~/.claude.json so claude skips its trust dialog.",
    )
    p.add_argument(
        "--account", default=None,
        help="which Zitadel machine-user key to authenticate with "
             f"({', '.join(l for l,_ in ACCOUNTS)}). "
             "Skipped if only one is installed; otherwise prompts.",
    )
    p.add_argument(
        "--send", default=None, metavar="TEXT",
        help="One-shot mode: send TEXT, print the answer on stdout, exit. "
             "Used by CI's e2e step. Without it, runs as an interactive REPL.",
    )
    p.add_argument(
        "--timeout", type=float, default=60.0,
        help="One-shot mode only: max seconds to wait for the answer "
             "(default 60).",
    )
    p.add_argument(
        "--attach", action="append", default=[],
        help="path to a file (image/PDF) to attach to the FIRST question. "
             "Repeatable.",
    )
    args = p.parse_args()

    label, key_path = pick_account(args.account,
                                   force_default=args.send is not None)

    manager_url = pick_manager_url()
    if args.cwd:
        from urllib.parse import quote
        absolute = str(Path(args.cwd).expanduser().resolve())
        manager_url = f"{manager_url}?cwd={quote(absolute, safe='')}"
    token = mint_access_token(key_path)
    print(f"-- auth:    {label} ({key_path})", file=sys.stderr)
    print(f"-- manager: {manager_url}", file=sys.stderr)

    attachments = [load_attachment(path) for path in args.attach]
    if attachments:
        print(f"-- {len(attachments)} attachment(s) queued for the first question",
              file=sys.stderr)

    if args.send is not None:
        try:
            sys.exit(asyncio.run(chat_oneshot(manager_url, token, args.send,
                                              attachments, args.timeout)))
        except KeyboardInterrupt:
            sys.exit(130)

    print("-- /chat connected; type a question + Enter; Ctrl+C or Ctrl+D to exit --",
          file=sys.stderr)
    try:
        asyncio.run(chat_loop(manager_url, token, attachments))
    except KeyboardInterrupt:
        pass


if __name__ == "__main__":
    main()
