"""E2E: two distinct subs => two distinct usage buckets + isolated sessions.

This proves the platform behaviour the gateway pass-through relies on: when two
different authenticated identities reach the manager, usage is attributed per
`sub` and their claude sessions are isolated. A real browser-federation login
can't run in CI, so two already-provisioned machine identities stand in for two
federated end-users:

  - user A = kabytech         (secrets/kabytech-key.json,    chat.user)
  - user B = admin SA         (secrets/admin-api-key.json,   chat.user + chat.admin)

Both hold chat.user (so both can drive /chat); B also holds chat.admin (so it can
read /control "usage"). Identity TYPE is irrelevant here — what we prove is
per-`sub` attribution + isolation, which is exactly what federated end-users get.

Requires the live compose stack + the native worker (same prerequisites as
deploy/compose/test_e2e_admin.py).

Run:  python -m pytest deploy/compose/test_e2e_gateway.py -v -s
"""
import asyncio
import json
import os
import sys
import time

import pytest

_REPO = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", ".."))
sys.path.insert(0, os.path.join(_REPO, "clients", "python"))

from llm_chat.auth import resolve_credentials, fetch_access_token  # noqa: E402
from llm_chat.protocol import ChatClient  # noqa: E402

ISSUER = os.environ.get("ZITADEL_ISSUER", "http://host.docker.internal:8080")
MANAGER_CHAT = os.environ.get("MANAGER_WS", "ws://127.0.0.1:7777/chat")
MANAGER_CONTROL = os.environ.get("MANAGER_CONTROL_WS", "ws://127.0.0.1:7777/control")


def _secret(name):
    with open(os.path.join(_REPO, "secrets", name), encoding="utf-8") as f:
        return f.read().strip()


PROJECT = _secret("project_id")
A_KEY = os.path.join(_REPO, "secrets", "kabytech-key.json")
B_KEY = os.path.join(_REPO, "secrets", "admin-api-key.json")
A_SUB = _secret("kabytech_user_id")
B_SUB = _secret("admin_api_user_id")


def _token(key_file):
    return fetch_access_token(resolve_credentials(ISSUER, PROJECT, key_file))


async def _ask(token, text):
    async with ChatClient(MANAGER_CHAT, lambda: token) as c:
        return (await c.ask(text, timeout=150)).text


async def _control_usage(admin_token):
    import websockets
    async with websockets.connect(
        MANAGER_CONTROL, additional_headers=[("Authorization", f"Bearer {admin_token}")],
        max_size=None, open_timeout=15) as ws:
        await ws.send(json.dumps({"cmd": "usage"}))
        # Read frames until the usage reply arrives (tolerates an optional
        # greeting frame /control may or may not send before the reply).
        deadline = time.monotonic() + 15
        while time.monotonic() < deadline:
            msg = json.loads(await asyncio.wait_for(ws.recv(), timeout=15))
            if "users" in msg or msg.get("ok") is False:
                return msg
        raise AssertionError("no usage reply from /control within timeout")


@pytest.fixture(scope="module")
def tokens():
    return {"a": _token(A_KEY), "b": _token(B_KEY)}


def test_two_subs_get_two_distinct_usage_buckets(tokens):
    asyncio.run(_ask(tokens["a"], "Reply with exactly: AAA"))
    asyncio.run(_ask(tokens["b"], "Reply with exactly: BBB"))
    usage = asyncio.run(_control_usage(tokens["b"]))  # B holds chat.admin
    by_user = {u["userId"]: u for u in usage["users"]}
    assert A_SUB in by_user, f"user A ({A_SUB}) must have its own usage row"
    assert B_SUB in by_user, f"user B ({B_SUB}) must have its own usage row"
    assert by_user[A_SUB]["charsIn"] > 0
    assert by_user[B_SUB]["charsIn"] > 0
    assert A_SUB != B_SUB


def test_sessions_are_isolated_between_subs(tokens):
    # A plants a codeword in its claude session; B (separate sub => separate
    # session) must not see it.
    asyncio.run(_ask(tokens["a"],
                     "Remember this codeword: BANANA. Reply with exactly: OK"))
    b_answer = asyncio.run(_ask(
        tokens["b"],
        "What codeword did I give you earlier? If I gave none, reply exactly: NONE"))
    assert "BANANA" not in b_answer.upper(), (
        f"isolation breach: user B saw user A's codeword (answer={b_answer!r})")
