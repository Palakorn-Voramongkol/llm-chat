"""One-off live verification for self-counted per-user usage.

Drives the REAL chat path as the kabytech machine user:
  1. a text-only question, and
  2. a question carrying a real 1x1 PNG attachment,
then prints the exact inputs so we can compare against the Console's
/api/usage aggregate (charsIn / charsOut / files / fileBytes).

Run from the repo root:  python verify_self_counted.py
"""
from __future__ import annotations

import asyncio
import base64
import json
import os
import sys
import time

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), "clients", "python"))

from llm_chat.auth import resolve_credentials, fetch_access_token  # noqa: E402
from llm_chat.protocol import ChatClient  # noqa: E402

ISSUER = os.environ.get("ZITADEL_ISSUER", "http://host.docker.internal:8080")
PROJECT = open("secrets/project_id", encoding="utf-8").read().strip()
KEY = "secrets/kabytech-key.json"
MANAGER = os.environ.get("MANAGER_WS", "ws://127.0.0.1:7777/chat")

# A genuine, minimal 1x1 PNG (decodes to a real image the worker can save).
PNG = base64.b64decode(
    "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAAC0lEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg=="
)


async def send(client: ChatClient, text: str, attachments, timeout: float = 150.0):
    """Send a raw `q` (optionally with attachments) and await its answer."""
    client._counter += 1
    mid = f"m{client._counter}"
    msg = {"type": "q", "id": mid, "text": text}
    if attachments:
        msg["attachments"] = attachments
    await client._ws.send(json.dumps(msg))
    return await client._await_answer(mid, time.monotonic() + timeout)


async def main() -> int:
    creds = resolve_credentials(ISSUER, PROJECT, KEY)
    provider = lambda: asyncio.to_thread(fetch_access_token, creds)  # noqa: E731

    q1 = "Reply with exactly the two characters: OK (nothing else)."
    q2 = "An image is attached. Reply with exactly: GOTIT (nothing else)."
    png_b64 = base64.b64encode(PNG).decode()

    async with ChatClient(MANAGER, provider) as client:
        print(f"connected sid={client.session_id}", file=sys.stderr)
        a1 = await send(client, q1, None)
        print(f"a1 received: {a1.text!r}", file=sys.stderr)
        a2 = await send(client, q2, [{"name": "pixel.png", "mime": "image/png", "data": png_b64}])
        print(f"a2 received: {a2.text!r}", file=sys.stderr)

    out = {
        "q1_text": q1,
        "q1_chars": len(q1),
        "a1_text": a1.text,
        "a1_chars": len(a1.text),
        "q2_text": q2,
        "q2_chars": len(q2),
        "a2_text": a2.text,
        "a2_chars": len(a2.text),
        "png_bytes": len(PNG),
        "EXPECTED_aggregate_for_kabytech": {
            "requests": 2,
            "charsIn": len(q1) + len(q2),
            "charsOut": len(a1.text) + len(a2.text),
            "files": 1,
            "fileBytes": len(PNG),
        },
    }
    print(json.dumps(out, indent=2))
    return 0


if __name__ == "__main__":
    raise SystemExit(asyncio.run(main()))
