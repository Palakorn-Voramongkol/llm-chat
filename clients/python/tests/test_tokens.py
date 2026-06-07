"""Unit tests for the keyring-backed TokenStore (with a fake keyring)."""

from __future__ import annotations

import sys
import time

import pytest

from llm_chat import tokens
from llm_chat.errors import AuthError
from llm_chat.oidc import TokenSet
from llm_chat.tokens import TokenStore


class _FakeKeyring:
    def __init__(self, fail=False):
        self.store = {}
        self.fail = fail

    def set_password(self, service, user, pw):
        if self.fail:
            raise RuntimeError("no backend")
        self.store[(service, user)] = pw

    def get_password(self, service, user):
        if self.fail:
            raise RuntimeError("no backend")
        return self.store.get((service, user))

    def delete_password(self, service, user):
        self.store.pop((service, user), None)


@pytest.fixture
def cfg(tmp_path, monkeypatch):
    monkeypatch.setenv("LLM_CHAT_CONFIG_DIR", str(tmp_path))
    return tmp_path


@pytest.fixture
def fake_keyring(monkeypatch):
    fk = _FakeKeyring()
    monkeypatch.setitem(sys.modules, "keyring", fk)
    return fk


def _ts(access="acc", refresh="ref", exp_in=300):
    return TokenSet(access_token=access, refresh_token=refresh, id_token="idt",
                    expires_at=time.time() + exp_in)


def test_save_load_roundtrip_keyring(cfg, fake_keyring):
    store = TokenStore("http://iss", "client-1")
    store.save(_ts())
    loaded = store.load()
    assert loaded.access_token == "acc"
    assert loaded.refresh_token == "ref"
    # refresh token lives in the keyring, NOT the file
    assert ("llm-chat", "refresh:http://iss") in fake_keyring.store


def test_refresh_token_not_in_plaintext_file_when_keyring_works(cfg, fake_keyring):
    TokenStore("http://iss", "c").save(_ts())
    text = (cfg / "tokens.json").read_text()
    assert "ref" not in text          # refresh token is in the keyring
    assert "acc" in text              # access token is in the file


def test_file_fallback_when_keyring_unavailable(cfg, monkeypatch):
    monkeypatch.setitem(sys.modules, "keyring", _FakeKeyring(fail=True))
    store = TokenStore("http://iss", "c")
    store.save(_ts())
    loaded = store.load()
    assert loaded.refresh_token == "ref"      # fell back to the file
    assert "ref" in (cfg / "tokens.json").read_text()


def test_clear_removes_keyring_and_file(cfg, fake_keyring):
    store = TokenStore("http://iss", "c")
    store.save(_ts())
    store.clear()
    assert store.load() is None
    assert ("llm-chat", "refresh:http://iss") not in fake_keyring.store


def test_valid_access_token_returns_cached_when_fresh(cfg, fake_keyring):
    store = TokenStore("http://iss", "c")
    store.save(_ts(exp_in=300))
    called = []
    tok = store.valid_access_token(lambda rt: called.append(rt) or _ts())
    assert tok == "acc"
    assert called == []  # not refreshed


def test_valid_access_token_refreshes_when_expired(cfg, fake_keyring):
    store = TokenStore("http://iss", "c")
    store.save(_ts(access="old", exp_in=-10))  # already expired
    new = _ts(access="new-access", refresh="new-ref")
    tok = store.valid_access_token(lambda rt: new)
    assert tok == "new-access"
    assert store.load().refresh_token == "new-ref"  # persisted


def test_valid_access_token_raises_when_not_logged_in(cfg, fake_keyring):
    with pytest.raises(AuthError, match="not logged in"):
        TokenStore("http://iss", "c").valid_access_token(lambda rt: _ts())
