"""Unit tests for the OIDC Auth Code + PKCE helpers and flow (no network)."""

from __future__ import annotations

import base64
import hashlib
import time

import pytest

from llm_chat import oidc
from llm_chat.errors import AuthError


def test_pkce_is_s256_and_unpadded():
    verifier, challenge = oidc.make_pkce()
    expected = base64.urlsafe_b64encode(
        hashlib.sha256(verifier.encode("ascii")).digest()
    ).rstrip(b"=").decode("ascii")
    assert challenge == expected
    assert "=" not in verifier and "=" not in challenge


def test_state_is_random():
    assert oidc.make_state() != oidc.make_state()


def test_build_scope_has_project_offline_and_openid():
    s = oidc.build_scope("PROJ-9")
    assert s.startswith("openid")
    assert "offline_access" in s
    assert "PROJ-9" in s


def test_authorize_url_has_pkce_state_and_code_response():
    u = oidc.build_authorize_url(
        "http://iss/authorize", client_id="c", redirect_uri="http://localhost:8477/callback",
        scope="openid", code_challenge="CH", state="ST")
    assert "code_challenge_method=S256" in u
    assert "code_challenge=CH" in u
    assert "state=ST" in u
    assert "response_type=code" in u


def test_parse_callback_extracts_code_and_state():
    assert oidc.parse_callback("/callback?code=abc&state=xyz") == {"code": "abc", "state": "xyz"}


def test_discover_uses_well_known(monkeypatch):
    class _R:
        @staticmethod
        def json():
            return {"authorization_endpoint": "AE", "token_endpoint": "TE",
                    "revocation_endpoint": "RE"}

    monkeypatch.setattr(oidc.requests, "get", lambda url, timeout=None: _R())
    ep = oidc.discover("http://iss")
    assert (ep.authorize, ep.token, ep.revoke) == ("AE", "TE", "RE")


def test_discover_falls_back_to_zitadel_paths(monkeypatch):
    def boom(url, timeout=None):
        raise oidc.requests.RequestException("down")

    monkeypatch.setattr(oidc.requests, "get", boom)
    ep = oidc.discover("http://iss")
    assert ep.token.endswith("/oauth/v2/token")
    assert ep.authorize.endswith("/oauth/v2/authorize")


def test_exchange_code_builds_tokenset(monkeypatch):
    class _R:
        status_code = 200

        @staticmethod
        def json():
            return {"access_token": "a", "refresh_token": "r", "id_token": "i", "expires_in": 300}

    monkeypatch.setattr(oidc.requests, "post", lambda url, data=None, timeout=None: _R())
    ts = oidc.exchange_code("http://t", client_id="c", code="x", redirect_uri="u", code_verifier="v")
    assert ts.access_token == "a" and ts.refresh_token == "r" and ts.id_token == "i"
    assert ts.expires_at > time.time()
    assert not ts.is_expired()


def test_refresh_keeps_old_refresh_token_when_not_echoed(monkeypatch):
    class _R:
        status_code = 200

        @staticmethod
        def json():
            return {"access_token": "a2", "expires_in": 300}  # no refresh_token

    monkeypatch.setattr(oidc.requests, "post", lambda url, data=None, timeout=None: _R())
    ts = oidc.refresh("http://t", client_id="c", refresh_token="OLD")
    assert ts.refresh_token == "OLD"
    assert ts.access_token == "a2"


def test_post_token_non_200_raises(monkeypatch):
    class _R:
        status_code = 401
        text = "bad request"

    monkeypatch.setattr(oidc.requests, "post", lambda *a, **k: _R())
    with pytest.raises(AuthError, match="401"):
        oidc.exchange_code("http://t", client_id="c", code="x", redirect_uri="u", code_verifier="v")


def test_login_happy_path(monkeypatch):
    monkeypatch.setattr(oidc, "discover", lambda issuer, **k: oidc.Endpoints("A", "T", "R"))
    monkeypatch.setattr(oidc, "make_state", lambda: "STATE123")
    monkeypatch.setattr(oidc, "make_pkce", lambda: ("VER", "CHAL"))
    monkeypatch.setattr(oidc.webbrowser, "open", lambda url: True)
    monkeypatch.setattr(oidc, "_capture_redirect",
                        lambda port, timeout: {"code": "CODE", "state": "STATE123"})
    seen = {}

    def fake_exchange(token_endpoint, *, client_id, code, redirect_uri, code_verifier, timeout=20):
        seen.update(code=code, verifier=code_verifier, redirect=redirect_uri)
        return oidc.TokenSet("acc", "ref", "idt", time.time() + 999)

    monkeypatch.setattr(oidc, "exchange_code", fake_exchange)
    ts = oidc.login("http://iss", "client-1", "proj-1", open_browser=True)
    assert ts.access_token == "acc"
    assert seen["code"] == "CODE" and seen["verifier"] == "VER"
    assert "8477" in seen["redirect"]


def test_login_state_mismatch_raises(monkeypatch):
    monkeypatch.setattr(oidc, "discover", lambda issuer, **k: oidc.Endpoints("A", "T", "R"))
    monkeypatch.setattr(oidc, "make_state", lambda: "GOOD")
    monkeypatch.setattr(oidc, "make_pkce", lambda: ("V", "C"))
    monkeypatch.setattr(oidc.webbrowser, "open", lambda url: True)
    monkeypatch.setattr(oidc, "_capture_redirect",
                        lambda port, timeout: {"code": "x", "state": "EVIL"})
    with pytest.raises(AuthError, match="state mismatch"):
        oidc.login("http://iss", "c", "p")


def test_login_timeout_raises(monkeypatch):
    monkeypatch.setattr(oidc, "discover", lambda issuer, **k: oidc.Endpoints("A", "T", "R"))
    monkeypatch.setattr(oidc.webbrowser, "open", lambda url: True)
    monkeypatch.setattr(oidc, "_capture_redirect", lambda port, timeout: {})
    with pytest.raises(AuthError, match="timed out"):
        oidc.login("http://iss", "c", "p")
