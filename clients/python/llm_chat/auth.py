"""Credential resolution and Zitadel JWT-bearer token minting.

Resolution precedence for each credential: explicit value > environment var >
the compose stack's ``secrets/`` directory. This is what lets the CLI run with
no flags when the stack is up.
"""

from __future__ import annotations

import json
import logging
import os
import time
from dataclasses import dataclass

import jwt as pyjwt
import requests

from .errors import AuthError, CredentialError

log = logging.getLogger("llm_chat.auth")

DEFAULT_ISSUER = "http://host.docker.internal:8080"

# This file lives at <repo>/clients/python/llm_chat/auth.py, so the compose
# stack's secrets are three levels up.
_SECRETS_DIR = os.path.abspath(
    os.path.join(os.path.dirname(__file__), "..", "..", "..", "secrets")
)


def secrets_dir() -> str:
    """Absolute path to the stack's ``secrets/`` directory (may not exist)."""
    # Allow an override for non-standard layouts / tests.
    return os.environ.get("LLM_CHAT_SECRETS_DIR", _SECRETS_DIR)


def _read_secret_file(name: str) -> str | None:
    path = os.path.join(secrets_dir(), name)
    try:
        with open(path, encoding="utf-8") as f:
            return f.read().strip()
    except OSError:
        return None


@dataclass(frozen=True)
class Credentials:
    """The three values needed to mint a token, plus where they came from."""

    issuer: str
    project: str
    key_file: str


def resolve_credentials(
    issuer: str | None = None,
    project: str | None = None,
    key_file: str | None = None,
) -> Credentials:
    """Fill in any missing credential from env / the secrets dir.

    Raises:
        CredentialError: if a credential can't be resolved, or the key file is
            absent — with a message that names what to do about it.
    """
    issuer = issuer or os.environ.get("ZITADEL_ISSUER") or DEFAULT_ISSUER

    project = project or os.environ.get("PROJECT_ID") or _read_secret_file("project_id")
    if not project:
        raise CredentialError(
            "no project id: pass --project, set PROJECT_ID, or run the compose "
            f"stack so {os.path.join(secrets_dir(), 'project_id')} exists"
        )

    if not key_file:
        key_file = os.environ.get("KABYTECH_KEY")
    if not key_file:
        candidate = os.path.join(secrets_dir(), "kabytech-key.json")
        key_file = candidate if os.path.exists(candidate) else None
    if not key_file:
        raise CredentialError(
            "no machine-user key: pass --key-file, set KABYTECH_KEY, or run the "
            f"compose stack so {os.path.join(secrets_dir(), 'kabytech-key.json')} exists"
        )
    if not os.path.exists(key_file):
        raise CredentialError(f"key file not found: {key_file}")

    log.debug("resolved credentials issuer=%s project=%s key_file=%s", issuer, project, key_file)
    return Credentials(issuer=issuer, project=project, key_file=key_file)


def fetch_access_token(creds: Credentials, *, ttl: int = 300, timeout: int = 15) -> str:
    """Sign a JWT-bearer assertion with the machine key and exchange it for an
    access token (a JWT the manager validates via JWKS).

    Raises:
        CredentialError: the key file is malformed.
        AuthError: the token endpoint was unreachable or rejected the request.
    """
    try:
        with open(creds.key_file, encoding="utf-8") as f:
            key = json.load(f)
        user_id, key_id, private_key = key["userId"], key["keyId"], key["key"]
    except (OSError, json.JSONDecodeError, KeyError) as e:
        raise CredentialError(f"invalid machine-user key file {creds.key_file}: {e}") from e

    now = int(time.time())
    assertion = pyjwt.encode(
        {"iss": user_id, "sub": user_id, "aud": creds.issuer, "iat": now, "exp": now + ttl},
        private_key,
        algorithm="RS256",
        headers={"kid": key_id},
    )
    token_url = f"{creds.issuer.rstrip('/')}/oauth/v2/token"
    try:
        resp = requests.post(
            token_url,
            data={
                "grant_type": "urn:ietf:params:oauth:grant-type:jwt-bearer",
                "scope": (
                    f"openid profile "
                    f"urn:zitadel:iam:org:project:id:{creds.project}:aud "
                    f"urn:zitadel:iam:org:projects:roles"
                ),
                "assertion": assertion,
            },
            timeout=timeout,
        )
    except requests.RequestException as e:
        raise AuthError(f"could not reach the token endpoint {token_url}: {e}") from e

    if resp.status_code != 200:
        raise AuthError(
            f"token endpoint returned {resp.status_code}: {resp.text[:300]} "
            f"(issuer reachable? project/key correct?)"
        )
    try:
        token = resp.json()["access_token"]
    except (ValueError, KeyError) as e:
        raise AuthError(f"token response had no access_token: {resp.text[:300]}") from e

    log.debug("minted access token len=%d", len(token))
    return token
