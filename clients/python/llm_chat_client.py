#!/usr/bin/env python3
"""llm-chat reference client (Python).

Authenticates to Zitadel as a machine user using a JSON key, exchanges the
JWT-bearer assertion for an access token, then connects to the manager's
typed `/chat` WebSocket and exchanges Q→A frames.

Usage:
    llm_chat_client.py \\
        --issuer    https://id.palakorn.com \\
        --project   <project_id> \\
        --key-file  /path/to/kabytech-key.json \\
        --manager   wss://api.example.com/chat \\
        --send      "hello"

Environment variables override the defaults; CLI flags override env.
"""

from __future__ import annotations

import argparse
import asyncio
import json
import os
import sys
import time
from typing import Any

import jwt as pyjwt
import requests
import websockets


def parse_args() -> argparse.Namespace:
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("--issuer", default=os.environ.get("ZITADEL_ISSUER"),
                   help="Zitadel issuer URL, e.g. https://id.example.com")
    p.add_argument("--project", default=os.environ.get("PROJECT_ID"),
                   help="Zitadel project_id (used for OIDC audience scope)")
    p.add_argument("--key-file", default=os.environ.get("KABYTECH_KEY"),
                   help="Path to the machine-user JSON key from Zitadel")
    p.add_argument("--manager", default=os.environ.get("MANAGER_WS", "ws://127.0.0.1:7777/chat"),
                   help="Manager /chat WebSocket URL (ws:// or wss://)")
    p.add_argument("--send", default="hello", help="Question text to send")
    p.add_argument("--timeout", type=int, default=60, help="End-to-end timeout (s)")
    a = p.parse_args()
    for r in ("issuer", "project", "key_file"):
        if not getattr(a, r):
            p.error(f"--{r.replace('_','-')} (or its env var) is required")
    return a


def get_access_token(issuer: str, project: str, key_file: str) -> str:
    """JWT-bearer flow: sign assertion with the machine user's private key,
    exchange at the OIDC token endpoint for an access_token."""
    with open(key_file) as f:
        key = json.load(f)
    now = int(time.time())
    assertion = pyjwt.encode(
        {"iss": key["userId"], "sub": key["userId"], "aud": issuer,
         "iat": now, "exp": now + 300},
        key["key"], algorithm="RS256",
        headers={"kid": key["keyId"]},
    )
    r = requests.post(
        f"{issuer.rstrip('/')}/oauth/v2/token",
        data={
            "grant_type": "urn:ietf:params:oauth:grant-type:jwt-bearer",
            "scope": (
                f"openid profile "
                f"urn:zitadel:iam:org:project:id:{project}:aud "
                f"urn:zitadel:iam:org:projects:roles"
            ),
            "assertion": assertion,
        },
        timeout=15,
    )
    r.raise_for_status()
    return r.json()["access_token"]


async def chat_round_trip(manager_url: str, token: str, text: str, deadline: float) -> str:
    """Connect to /chat, send `q text=…`, return the answer text."""
    async with websockets.connect(
        manager_url,
        additional_headers=[("Authorization", f"Bearer {token}")],
    ) as ws:
        print(f"[ws] connected to {manager_url}")
        await ws.send(json.dumps({"type": "q", "id": "e2e-1", "text": text}))
        print(f"[ws] >>> q text={text!r}")

        while True:
            remaining = deadline - time.time()
            if remaining <= 0:
                raise TimeoutError("manager did not return an `a` frame in time")
            raw = await asyncio.wait_for(ws.recv(), timeout=remaining)
            msg: dict[str, Any] = json.loads(raw)
            print(f"[ws] <<< {json.dumps(msg)[:200]}")
            if msg.get("type") == "a":
                await ws.send(json.dumps({"type": "confirm", "seq": msg["seq"]}))
                return msg.get("text", "")


def main() -> int:
    args = parse_args()
    print(f"[auth] issuer={args.issuer}  project={args.project}  key={args.key_file}")
    tok = get_access_token(args.issuer, args.project, args.key_file)
    print(f"[auth] access_token len={len(tok)}")

    deadline = time.time() + args.timeout
    try:
        answer = asyncio.run(chat_round_trip(args.manager, tok, args.send, deadline))
    except (TimeoutError, asyncio.TimeoutError):
        print("[ws] FAIL - no answer within timeout", file=sys.stderr)
        return 1
    print()
    print("=== Q&A ===")
    print(f"Q: {args.send}")
    print(f"A: {answer}")
    print("===========")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
