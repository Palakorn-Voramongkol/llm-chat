"""End-to-end acceptance for the admin stack (§9 'End-to-end acceptance', §10).

RUNBOOK (operator, once):
  1. cp .env.example .env  &&  fill ZITADEL_MASTERKEY / POSTGRES_PASSWORD /
     LLM_CHAT_AUTH_TOKEN / ADMIN_SESSION_KEY  (openssl rand -hex ...).
  2. docker compose up -d --build   (postgres, zitadel, zitadel-init,
     manager, admin-api, admin-web all come up; zitadel-init exits 0).
  3. Grant your human operator the chat.admin role (provision seeds or
     console-once), then open http://localhost:3000 -> "Sign in" -> log in.
  4. Copy the session cookie value (DevTools->Application->Cookies, name "id")
     and the project id:
       $env:ADMIN_E2E="1"
       $env:ADMIN_OPERATOR_COOKIE="<cookie>"
       $env:PROJECT_ID=(Get-Content secrets/project_id)
  5. python -m pytest deploy/compose/test_e2e_admin.py -v

GATE: runs only when ADMIN_E2E=1 against a RUNNING `docker compose up` stack;
skipped otherwise. This is the SOURCE-OF-TRUTH loop — it talks to the live
admin-api and the live manager (via the real async ChatClient WebSocket
protocol), never a mocked Zitadel body.

Loop: operator session (cookie) -> POST /api/users/machine -> POST .../keys
(key returned once) -> POST .../grants {chat.user} -> mint a JWT from that key
via the python client's real auth path -> assert the MANAGER's chat.user gate
ACCEPTS it (a chat round-trips). Asserts the SAME key, ungranted, is REJECTED
(the manager 403s the WS upgrade -> ManagerUnavailable).
"""
import asyncio
import json
import os
import sys
import time
import uuid

import pytest
import requests

# The python client lives under clients/python; make it importable.
sys.path.insert(0, os.path.join(
    os.path.dirname(__file__), "..", "..", "clients", "python"))

pytestmark = pytest.mark.skipif(
    os.environ.get("ADMIN_E2E") != "1",
    reason="ADMIN_E2E!=1 — needs a running compose stack (docker compose up)",
)

ADMIN_API = os.environ.get("ADMIN_API_BASE", "http://localhost:7676")
MANAGER_WS = os.environ.get("MANAGER_WS", "ws://127.0.0.1:7777/chat")
OPERATOR_COOKIE = os.environ.get("ADMIN_OPERATOR_COOKIE")
PROJECT_ID = os.environ.get("PROJECT_ID")  # from secrets/project_id


def _session() -> requests.Session:
    if not OPERATOR_COOKIE:
        pytest.skip("ADMIN_OPERATOR_COOKIE unset — log in once and export the cookie")
    s = requests.Session()
    s.headers.update({"Content-Type": "application/json"})
    # The BFF session cookie is named "id" (SessionManagerLayer.with_name("id")).
    s.cookies.set("id", OPERATOR_COOKIE, domain="localhost")
    return s


async def _ask_via_manager(token: str) -> str:
    """Open ONE /chat WebSocket with the bearer token via the real ChatClient
    and return the answer text. Raises ManagerUnavailable if the manager 403s
    the upgrade (the chat.user gate rejecting an ungranted token)."""
    from llm_chat.protocol import ChatClient
    async with ChatClient(MANAGER_WS, token_provider=lambda: token) as c:
        answer = await c.ask("ping", timeout=30.0)
        return answer.text


def _manager_accepts(token: str) -> tuple[bool, str]:
    """True if the manager's chat.user gate let the token through (a chat
    round-tripped). A ManagerUnavailable means the WS upgrade was rejected
    (403 — missing chat.user) OR the manager is down; the caller sequences
    this so a rejection is the expected pre-grant outcome."""
    from llm_chat.errors import ManagerUnavailable
    try:
        text = asyncio.run(_ask_via_manager(token))
        return True, text
    except ManagerUnavailable as e:
        return False, str(e)


def test_admin_minted_machine_key_passes_manager_chat_user_gate(tmp_path) -> None:
    from llm_chat import auth

    s = _session()
    uname = f"e2e-machine-{uuid.uuid4().hex[:8]}"

    # 1) operator creates a machine user via the BFF
    r = s.post(f"{ADMIN_API}/api/users/machine",
               data=json.dumps({"username": uname, "name": uname}))
    assert r.status_code in (200, 201), r.text
    user_id = r.json()["userId"]

    # 2) operator mints a JSON key — returned ONCE, streamed to the caller
    r = s.post(f"{ADMIN_API}/api/users/{user_id}/keys",
               data=json.dumps({"type": "KEY_TYPE_JSON"}))
    assert r.status_code in (200, 201), r.text
    created = r.json()  # admin-api returns the full create response (keyDetails once)
    # keyDetails is base64 of the serviceaccount JSON ({userId,keyId,key,...}).
    import base64
    key_blob = json.loads(base64.b64decode(created["keyDetails"]))
    key_file = tmp_path / "e2e-key.json"
    key_file.write_text(json.dumps(key_blob), encoding="utf-8")

    creds = auth.Credentials(
        issuer=os.environ.get("ZITADEL_ISSUER", auth.DEFAULT_ISSUER),
        project=PROJECT_ID,
        key_file=str(key_file),
    )

    # 2b) BEFORE granting chat.user: the manager MUST reject (gate is real).
    token = auth.fetch_access_token(creds)
    ok, msg = _manager_accepts(token)
    assert not ok, f"expected chat.user rejection before grant, got accept: {msg!r}"

    # 3) operator grants chat.user to the new machine user
    r = s.post(f"{ADMIN_API}/api/users/{user_id}/grants",
               data=json.dumps({"role_keys": ["chat.user"]}))
    assert r.status_code in (200, 201), r.text

    # 4) a FRESH token (role projected) must now pass the manager gate.
    #    Zitadel projection is eventually consistent — retry briefly.
    ok, msg = False, ""
    for _ in range(10):
        token = auth.fetch_access_token(creds)
        ok, msg = _manager_accepts(token)
        if ok:
            break
        time.sleep(1)
    assert ok, f"admin-minted+granted key was rejected by manager: {msg!r}"

    # 5) cleanup — delete the throwaway machine user
    s.delete(f"{ADMIN_API}/api/users/{user_id}")
