"""Credential resolution and Zitadel JWT-bearer token minting.

Connection settings are SOLE-SOURCED from ``.env.local`` (loaded into the
process env at CLI startup). Resolution precedence per credential: explicit
value > environment var. There is NO ``secrets/`` fallback and NO hardcoded
default — a missing value fails closed.
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
    """Resolve each credential from an explicit value or the process env (fed by
    ``.env.local``). SOLE-SOURCED — no ``secrets/`` fallback, no default: a
    missing value fails closed with a message naming the env var. The key file's
    *contents* still live in ``secrets/``; ``KABYTECH_KEY`` is just the path.

    Raises:
        CredentialError: if a credential is absent, or the key file is missing.
    """
    issuer = issuer or os.environ.get("ZITADEL_ISSUER")
    if not issuer:
        raise CredentialError("no issuer: pass --issuer or set ZITADEL_ISSUER in .env.local")

    project = project or os.environ.get("PROJECT_ID")
    if not project:
        raise CredentialError("no project id: pass --project or set PROJECT_ID in .env.local")

    key_file = key_file or os.environ.get("KABYTECH_KEY")
    if not key_file:
        raise CredentialError(
            "no machine-user key: pass --key-file or set KABYTECH_KEY in .env.local"
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
