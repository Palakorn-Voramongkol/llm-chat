"""Unit tests for credential resolution and JWT-bearer token minting."""

from __future__ import annotations

import json
import os

import pytest
from cryptography.hazmat.primitives import serialization
from cryptography.hazmat.primitives.asymmetric import rsa

from llm_chat import auth
from llm_chat.auth import Credentials, fetch_access_token, resolve_credentials
from llm_chat.errors import AuthError, CredentialError


def _write_machine_key(path: str) -> dict:
    """Create a real RSA key and a Zitadel-style machine-key JSON at `path`."""
    key = rsa.generate_private_key(public_exponent=65537, key_size=2048)
    pem = key.private_bytes(
        serialization.Encoding.PEM,
        serialization.PrivateFormat.TraditionalOpenSSL,
        serialization.NoEncryption(),
    ).decode()
    data = {"type": "serviceaccount", "keyId": "kid-123", "key": pem, "userId": "user-999"}
    with open(path, "w", encoding="utf-8") as f:
        json.dump(data, f)
    return data


@pytest.fixture
def secrets(tmp_path, monkeypatch):
    """A populated secrets dir wired in via LLM_CHAT_SECRETS_DIR."""
    (tmp_path / "project_id").write_text("376349317130092547", encoding="utf-8")
    _write_machine_key(str(tmp_path / "kabytech-key.json"))
    monkeypatch.setenv("LLM_CHAT_SECRETS_DIR", str(tmp_path))
    for var in ("ZITADEL_ISSUER", "PROJECT_ID", "KABYTECH_KEY"):
        monkeypatch.delenv(var, raising=False)
    return tmp_path


def test_resolve_defaults_from_secrets_dir(secrets):
    creds = resolve_credentials()
    assert creds.issuer == auth.DEFAULT_ISSUER
    assert creds.project == "376349317130092547"
    assert creds.key_file == os.path.join(str(secrets), "kabytech-key.json")


def test_explicit_args_win(secrets, tmp_path):
    other = tmp_path / "other-key.json"
    _write_machine_key(str(other))
    creds = resolve_credentials(issuer="http://x:8080", project="p1", key_file=str(other))
    assert (creds.issuer, creds.project, creds.key_file) == ("http://x:8080", "p1", str(other))


def test_missing_project_raises(tmp_path, monkeypatch):
    monkeypatch.setenv("LLM_CHAT_SECRETS_DIR", str(tmp_path))  # empty dir
    for var in ("PROJECT_ID", "KABYTECH_KEY"):
        monkeypatch.delenv(var, raising=False)
    with pytest.raises(CredentialError, match="project id"):
        resolve_credentials()


def test_missing_key_file_raises(tmp_path, monkeypatch):
    (tmp_path / "project_id").write_text("p1", encoding="utf-8")
    monkeypatch.setenv("LLM_CHAT_SECRETS_DIR", str(tmp_path))
    monkeypatch.delenv("KABYTECH_KEY", raising=False)
    with pytest.raises(CredentialError, match="machine-user key"):
        resolve_credentials()


def test_fetch_access_token_posts_assertion(secrets, monkeypatch):
    creds = resolve_credentials()
    captured = {}

    class _Resp:
        status_code = 200

        @staticmethod
        def json():
            return {"access_token": "the-token-abc"}

    def fake_post(url, data=None, timeout=None):
        captured["url"] = url
        captured["data"] = data
        return _Resp()

    monkeypatch.setattr(auth.requests, "post", fake_post)
    tok = fetch_access_token(creds)
    assert tok == "the-token-abc"
    assert captured["url"].endswith("/oauth/v2/token")
    assert captured["data"]["grant_type"].endswith("jwt-bearer")
    assert creds.project in captured["data"]["scope"]
    assert captured["data"]["assertion"]  # a signed JWT string


def test_fetch_access_token_non_200_raises(secrets, monkeypatch):
    creds = resolve_credentials()

    class _Resp:
        status_code = 401
        text = "unauthorized"

    monkeypatch.setattr(auth.requests, "post", lambda *a, **k: _Resp())
    with pytest.raises(AuthError, match="401"):
        fetch_access_token(creds)


def test_fetch_access_token_bad_key_file_raises(tmp_path):
    bad = tmp_path / "bad.json"
    bad.write_text("{not json", encoding="utf-8")
    creds = Credentials(issuer="http://x:8080", project="p1", key_file=str(bad))
    with pytest.raises(CredentialError, match="invalid machine-user key"):
        fetch_access_token(creds)
