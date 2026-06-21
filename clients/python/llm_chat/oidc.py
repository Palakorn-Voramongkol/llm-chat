"""OAuth2 Authorization Code flow with PKCE — interactive human login.

A native/public client (no secret): we prove possession with PKCE (S256) and a
CSRF `state`, catch the redirect on a loopback HTTP server, and exchange the
code for access + refresh + id tokens. `offline_access` is requested so the
session survives the short access-token lifetime via refresh.
"""

from __future__ import annotations

import base64
import hashlib
import http.server
import logging
import secrets
import threading
import time
import urllib.parse
import webbrowser
from dataclasses import dataclass

import requests

from .errors import AuthError, ProtocolError

log = logging.getLogger("llm_chat.oidc")

REQUEST_TIMEOUT = 20


# ---------------- token model ----------------

@dataclass
class TokenSet:
    access_token: str
    refresh_token: str | None
    id_token: str | None
    expires_at: float  # epoch seconds

    def is_expired(self, skew: int = 30) -> bool:
        return time.time() >= (self.expires_at - skew)

    @classmethod
    def from_response(cls, body: dict, *, now: float | None = None) -> "TokenSet":
        now = time.time() if now is None else now
        return cls(
            access_token=body["access_token"],
            refresh_token=body.get("refresh_token"),
            id_token=body.get("id_token"),
            expires_at=now + int(body.get("expires_in", 300)),
        )


# ---------------- pure helpers (unit-tested, no network) ----------------

def _b64url(raw: bytes) -> str:
    return base64.urlsafe_b64encode(raw).rstrip(b"=").decode("ascii")


def make_pkce() -> tuple[str, str]:
    """Return (code_verifier, code_challenge) using S256."""
    verifier = _b64url(secrets.token_bytes(32))
    challenge = _b64url(hashlib.sha256(verifier.encode("ascii")).digest())
    return verifier, challenge


def make_state() -> str:
    return _b64url(secrets.token_bytes(16))


def build_scope(project: str) -> str:
    return (
        "openid profile email offline_access "
        f"urn:zitadel:iam:org:project:id:{project}:aud "
        "urn:zitadel:iam:org:projects:roles"
    )


def build_authorize_url(authorize_endpoint: str, *, client_id: str, redirect_uri: str,
                        scope: str, code_challenge: str, state: str) -> str:
    q = urllib.parse.urlencode({
        "client_id": client_id,
        "redirect_uri": redirect_uri,
        "response_type": "code",
        "scope": scope,
        "code_challenge": code_challenge,
        "code_challenge_method": "S256",
        "state": state,
        "prompt": "login",
    })
    return f"{authorize_endpoint}?{q}"


def parse_callback(path: str) -> dict:
    """Extract query params from the redirect path (e.g. '/callback?code=..&state=..')."""
    query = urllib.parse.urlparse(path).query
    flat = {k: v[0] for k, v in urllib.parse.parse_qs(query).items()}
    return flat


@dataclass
class Endpoints:
    authorize: str
    token: str
    revoke: str


def discover(issuer: str, *, timeout: int = REQUEST_TIMEOUT) -> Endpoints:
    """Read endpoints from the OIDC discovery document, with a Zitadel-shaped
    fallback if discovery is unreachable."""
    url = f"{issuer.rstrip('/')}/.well-known/openid-configuration"
    try:
        body = requests.get(url, timeout=timeout).json()
        return Endpoints(
            authorize=body["authorization_endpoint"],
            token=body["token_endpoint"],
            revoke=body.get("revocation_endpoint", f"{issuer.rstrip('/')}/oauth/v2/revoke"),
        )
    except (requests.RequestException, ValueError, KeyError) as e:
        log.debug("discovery failed (%s); using Zitadel default endpoints", e)
        base = issuer.rstrip("/")
        return Endpoints(
            authorize=f"{base}/oauth/v2/authorize",
            token=f"{base}/oauth/v2/token",
            revoke=f"{base}/oauth/v2/revoke",
        )


# ---------------- loopback redirect capture ----------------

class _CallbackHandler(http.server.BaseHTTPRequestHandler):
    captured: dict = {}

    def do_GET(self):  # noqa: N802 (http.server API)
        params = parse_callback(self.path)
        if "code" in params or "error" in params:
            type(self).captured = params
            self.send_response(200)
            self.send_header("Content-Type", "text/html; charset=utf-8")
            self.end_headers()
            ok = "error" not in params
            msg = ("Login complete — you can close this tab and return to the terminal."
                   if ok else f"Login failed: {params.get('error')}")
            self.wfile.write(f"<html><body><h3>{msg}</h3></body></html>".encode())
        else:
            self.send_response(404)
            self.end_headers()

    def log_message(self, *args):  # silence the default stderr logging
        pass


def _capture_redirect(port: int, *, timeout: int) -> dict:
    """Serve one request on 127.0.0.1:port and return the captured query params."""
    _CallbackHandler.captured = {}
    server = http.server.HTTPServer(("127.0.0.1", port), _CallbackHandler)
    server.timeout = timeout
    t = threading.Thread(target=server.handle_request, daemon=True)
    t.start()
    t.join(timeout)
    server.server_close()
    return dict(_CallbackHandler.captured)


# ---------------- flows ----------------

def login(issuer: str, client_id: str, project: str, *, redirect_port: int = 8477,
          open_browser: bool = True, timeout: int = 300) -> TokenSet:
    """Run the full Auth Code + PKCE login and return a `TokenSet`.

    Raises AuthError on any failure (user denial, state mismatch, token error).
    """
    endpoints = discover(issuer)
    verifier, challenge = make_pkce()
    state = make_state()
    # RFC 8252: 127.0.0.1 loopback literal (not "localhost", which a hosts-file
    # entry could hijack). The callback server binds 127.0.0.1.
    redirect_uri = f"http://127.0.0.1:{redirect_port}/callback"
    scope = build_scope(project)
    url = build_authorize_url(endpoints.authorize, client_id=client_id,
                              redirect_uri=redirect_uri, scope=scope,
                              code_challenge=challenge, state=state)

    print(f"Opening browser to sign in:\n  {url}")
    if open_browser:
        try:
            webbrowser.open(url)
        except Exception:  # noqa: BLE001 — headless boxes have no browser
            print("(could not open a browser automatically — open the URL above)")

    params = _capture_redirect(redirect_port, timeout=timeout)
    if not params:
        raise AuthError(f"timed out waiting for the login redirect on :{redirect_port}")
    if "error" in params:
        raise AuthError(f"login denied/failed: {params.get('error')} "
                        f"{params.get('error_description', '')}".strip())
    if params.get("state") != state:
        raise AuthError("state mismatch on the OAuth callback (possible CSRF) — aborting")
    code = params.get("code")
    if not code:
        raise AuthError("no authorization code in the callback")

    return exchange_code(endpoints.token, client_id=client_id, code=code,
                         redirect_uri=redirect_uri, code_verifier=verifier)


def exchange_code(token_endpoint: str, *, client_id: str, code: str,
                  redirect_uri: str, code_verifier: str,
                  timeout: int = REQUEST_TIMEOUT) -> TokenSet:
    resp = _post_token(token_endpoint, {
        "grant_type": "authorization_code",
        "client_id": client_id,
        "code": code,
        "redirect_uri": redirect_uri,
        "code_verifier": code_verifier,
    }, timeout=timeout)
    return TokenSet.from_response(resp)


def refresh(token_endpoint: str, *, client_id: str, refresh_token: str,
            timeout: int = REQUEST_TIMEOUT) -> TokenSet:
    resp = _post_token(token_endpoint, {
        "grant_type": "refresh_token",
        "client_id": client_id,
        "refresh_token": refresh_token,
    }, timeout=timeout)
    ts = TokenSet.from_response(resp)
    # Zitadel may not echo a new refresh token; keep the old one if so.
    if ts.refresh_token is None:
        ts.refresh_token = refresh_token
    return ts


def revoke(revoke_endpoint: str, *, client_id: str, token: str,
           timeout: int = REQUEST_TIMEOUT) -> None:
    """Best-effort refresh-token revocation at logout."""
    try:
        requests.post(revoke_endpoint,
                      data={"client_id": client_id, "token": token},
                      timeout=timeout)
    except requests.RequestException as e:
        log.debug("revoke failed (ignored): %s", e)


def _post_token(token_endpoint: str, data: dict, *, timeout: int) -> dict:
    try:
        resp = requests.post(token_endpoint, data=data, timeout=timeout)
    except requests.RequestException as e:
        raise AuthError(f"could not reach the token endpoint {token_endpoint}: {e}") from e
    if resp.status_code != 200:
        raise AuthError(f"token endpoint returned {resp.status_code}: {resp.text[:300]}")
    try:
        return resp.json()
    except ValueError as e:
        raise ProtocolError(f"token response was not JSON: {resp.text[:200]}") from e
