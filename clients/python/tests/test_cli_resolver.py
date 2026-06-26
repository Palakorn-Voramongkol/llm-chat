"""Unit tests for CLI auth-mode selection and parser wiring."""

from __future__ import annotations

import argparse

from llm_chat import cli


def test_auth_mode_is_role_based_by_default():
    assert cli._auth_mode(argparse.Namespace(command="chat", auth=None)) == "user"
    assert cli._auth_mode(argparse.Namespace(command="ask", auth=None)) == "machine"


def test_auth_mode_explicit_override_wins():
    assert cli._auth_mode(argparse.Namespace(command="chat", auth="machine")) == "machine"
    assert cli._auth_mode(argparse.Namespace(command="ask", auth="user")) == "user"


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
