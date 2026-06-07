"""Unit tests for CLI auth-mode selection, JWT display, and parser wiring."""

from __future__ import annotations

import argparse
import base64
import json

from llm_chat import cli, oidc


def _fake_jwt(payload: dict) -> str:
    body = base64.urlsafe_b64encode(json.dumps(payload).encode()).rstrip(b"=").decode()
    return f"header.{body}.sig"


def test_auth_mode_is_role_based_by_default():
    assert cli._auth_mode(argparse.Namespace(command="chat", auth=None)) == "user"
    assert cli._auth_mode(argparse.Namespace(command="ask", auth=None)) == "machine"


def test_auth_mode_explicit_override_wins():
    assert cli._auth_mode(argparse.Namespace(command="chat", auth="machine")) == "machine"
    assert cli._auth_mode(argparse.Namespace(command="ask", auth="user")) == "user"


def test_decode_claims_reads_payload():
    tok = _fake_jwt({"sub": "u1", "email": "demo@llm-chat.local"})
    claims = cli._decode_claims(tok)
    assert claims["email"] == "demo@llm-chat.local"
    assert cli._decode_claims(None) == {}
    assert cli._decode_claims("not-a-jwt") == {}


def test_print_whoami_shows_email_and_roles(capsys):
    tok = _fake_jwt({
        "sub": "u1", "email": "demo@llm-chat.local",
        "urn:zitadel:iam:org:project:123:roles": {"chat.user": {"org": "o1"}},
    })
    cli._print_whoami(oidc.TokenSet("acc", None, tok, 9e9))
    out = capsys.readouterr().out
    assert "demo@llm-chat.local" in out
    assert "chat.user" in out


def test_parser_has_login_logout_whoami_and_auth_flag():
    p = cli.build_parser()
    assert p.parse_args(["login"]).command == "login"
    assert p.parse_args(["logout"]).command == "logout"
    assert p.parse_args(["whoami"]).command == "whoami"
    assert p.parse_args(["chat", "--auth", "machine"]).auth == "machine"
    assert p.parse_args(["chat"]).oidc_port == 8477


def test_legacy_ask_main_forces_machine(monkeypatch):
    captured = {}

    def fake_dispatch(args):
        captured["command"] = args.command
        captured["auth"] = args.auth
        return 0

    monkeypatch.setattr(cli, "_dispatch", fake_dispatch)
    cli.ask_main(["--send", "hi"])
    assert captured["command"] == "ask"
    assert captured["auth"] == "machine"
