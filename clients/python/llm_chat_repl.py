#!/usr/bin/env python3
"""Interactive REPL for the llm-chat manager.

Opens ONE persistent WebSocket session — claude warms up once, then every
prompt you type re-uses the same session. Type your message, press Enter,
read the answer. Exit with `/exit`, `/quit`, or Ctrl+D.

Usage:
    llm_chat_repl.py \\
        --issuer    https://id.palakorn.com \\
        --project   <project_id> \\
        --key-file  /path/to/kabytech-key.json \\
        --manager   ws://127.0.0.1:7777/chat
"""
from __future__ import annotations

import argparse
import asyncio
import json
import os
import sys
import time

import jwt as pyjwt
import requests
import websockets


def parse_args() -> argparse.Namespace:
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("--issuer",   default=os.environ.get("ZITADEL_ISSUER"))
    p.add_argument("--project",  default=os.environ.get("PROJECT_ID"))
    p.add_argument("--key-file", default=os.environ.get("KABYTECH_KEY"))
    p.add_argument("--manager",  default=os.environ.get("MANAGER_WS",
                                                        "ws://127.0.0.1:7777/chat"))
    p.add_argument("--timeout", type=int, default=120,
                   help="Per-turn timeout in seconds (default 120)")
    a = p.parse_args()
    for r in ("issuer", "project", "key_file"):
        if not getattr(a, r):
            p.error(f"--{r.replace('_','-')} (or its env var) is required")
    return a


def get_access_token(issuer: str, project: str, key_file: str) -> str:
    with open(key_file) as f:
        key = json.load(f)
    now = int(time.time())
    assertion = pyjwt.encode(
        {"iss": key["userId"], "sub": key["userId"], "aud": issuer,
         "iat": now, "exp": now + 300},
        key["key"], algorithm="RS256", headers={"kid": key["keyId"]})
    r = requests.post(
        f"{issuer.rstrip('/')}/oauth/v2/token",
        data={
            "grant_type": "urn:ietf:params:oauth:grant-type:jwt-bearer",
            "scope": (f"openid profile "
                      f"urn:zitadel:iam:org:project:id:{project}:aud "
                      f"urn:zitadel:iam:org:projects:roles"),
            "assertion": assertion},
        timeout=15)
    r.raise_for_status()
    return r.json()["access_token"]


async def read_line() -> str | None:
    """Prompt + read one line off stdin without blocking the event loop.
    Returns None on EOF (Ctrl+D)."""
    loop = asyncio.get_running_loop()
    try:
        return await loop.run_in_executor(None, input, "> ")
    except EOFError:
        return None


async def repl(manager_url: str, token: str, per_turn_timeout: int) -> int:
    async with websockets.connect(
        manager_url,
        additional_headers=[("Authorization", f"Bearer {token}")],
    ) as ws:
        # First frame from the manager is `initialized` — surface it once
        # so the user sees the session is up, then go quiet.
        try:
            init = json.loads(await asyncio.wait_for(ws.recv(), timeout=15))
        except asyncio.TimeoutError:
            print("manager did not send `initialized` within 15s", file=sys.stderr)
            return 1
        if init.get("type") != "initialized":
            print(f"unexpected first frame: {init}", file=sys.stderr)
            return 1
        print(f"[connected — sid={init.get('sid')}]  type /exit or Ctrl+D to quit\n",
              flush=True)

        turn = 0
        while True:
            text = await read_line()
            if text is None or text.strip() in ("/exit", "/quit"):
                print()
                return 0
            if not text.strip():
                continue

            turn += 1
            qid = f"r-{turn}"
            await ws.send(json.dumps({"type": "q", "id": qid, "text": text}))

            deadline = time.time() + per_turn_timeout
            answer: str | None = None
            while True:
                remaining = deadline - time.time()
                if remaining <= 0:
                    print(f"[timeout — no answer in {per_turn_timeout}s]\n",
                          file=sys.stderr, flush=True)
                    break
                try:
                    raw = await asyncio.wait_for(ws.recv(), timeout=remaining)
                except asyncio.TimeoutError:
                    continue
                msg = json.loads(raw)
                t = msg.get("type")
                if t == "ack":
                    continue                          # nothing to show
                if t == "err":
                    print(f"[error] {msg.get('text')}\n", file=sys.stderr, flush=True)
                    break
                if t == "a":
                    answer = msg.get("text", "")
                    await ws.send(json.dumps({"type": "confirm", "seq": msg["seq"]}))
                    break
                # ignore any other frame types

            if answer is not None:
                print(answer + "\n", flush=True)


def main() -> int:
    args = parse_args()
    tok = get_access_token(args.issuer, args.project, args.key_file)
    try:
        return asyncio.run(repl(args.manager, tok, args.timeout))
    except KeyboardInterrupt:
        print()
        return 0


if __name__ == "__main__":
    raise SystemExit(main())
