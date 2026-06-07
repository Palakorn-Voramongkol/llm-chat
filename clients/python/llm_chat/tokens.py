"""Secure token cache.

The refresh token (the long-lived credential) goes into the OS keyring
(Windows Credential Manager / macOS Keychain / libsecret). If no keyring
backend is available it degrades to a `0600` file. Short-lived access/id tokens
live in a `0600` sidecar file and are refreshed on demand.
"""

from __future__ import annotations

import json
import logging
import os
import stat

from .errors import AuthError
from .oidc import TokenSet

log = logging.getLogger("llm_chat.tokens")

SERVICE = "llm-chat"


def _config_dir() -> str:
    override = os.environ.get("LLM_CHAT_CONFIG_DIR")
    if override:
        os.makedirs(override, exist_ok=True)
        return override
    try:
        import platformdirs
        d = platformdirs.user_config_dir("llm-chat")
    except Exception:  # noqa: BLE001 — platformdirs optional
        d = os.path.join(os.path.expanduser("~"), ".config", "llm-chat")
    os.makedirs(d, exist_ok=True)
    return d


def _tokens_file() -> str:
    return os.path.join(_config_dir(), "tokens.json")


def _load_file() -> dict:
    try:
        with open(_tokens_file(), encoding="utf-8") as f:
            return json.load(f)
    except (OSError, ValueError):
        return {}


def _save_file(data: dict) -> None:
    path = _tokens_file()
    with open(path, "w", encoding="utf-8") as f:
        json.dump(data, f)
    try:
        os.chmod(path, stat.S_IRUSR | stat.S_IWUSR)  # 0600
    except OSError:
        pass


class TokenStore:
    """Per-issuer token cache: refresh token in the keyring, access in a file."""

    def __init__(self, issuer: str, client_id: str) -> None:
        self.issuer = issuer
        self.client_id = client_id
        self._kr_user = f"refresh:{issuer}"

    # ---- keyring, with graceful degradation ----
    def _kr_set(self, refresh_token: str) -> bool:
        try:
            import keyring
            keyring.set_password(SERVICE, self._kr_user, refresh_token)
            return True
        except Exception as e:  # noqa: BLE001 — any backend failure → file fallback
            log.warning("keyring unavailable (%s) — storing refresh token in a 0600 file", e)
            return False

    def _kr_get(self) -> str | None:
        try:
            import keyring
            return keyring.get_password(SERVICE, self._kr_user)
        except Exception:  # noqa: BLE001
            return None

    def _kr_del(self) -> None:
        try:
            import keyring
            keyring.delete_password(SERVICE, self._kr_user)
        except Exception:  # noqa: BLE001 — not present / no backend
            pass

    # ---- public API ----
    def save(self, ts: TokenSet) -> None:
        data = _load_file()
        entry = {"access_token": ts.access_token, "id_token": ts.id_token,
                 "expires_at": ts.expires_at}
        if ts.refresh_token:
            if not self._kr_set(ts.refresh_token):
                entry["refresh_token"] = ts.refresh_token  # file fallback
        data[self.issuer] = entry
        _save_file(data)

    def load(self) -> TokenSet | None:
        entry = _load_file().get(self.issuer)
        if not entry:
            return None
        refresh = self._kr_get() or entry.get("refresh_token")
        return TokenSet(
            access_token=entry.get("access_token", ""),
            refresh_token=refresh,
            id_token=entry.get("id_token"),
            expires_at=entry.get("expires_at", 0),
        )

    def clear(self) -> None:
        self._kr_del()
        data = _load_file()
        data.pop(self.issuer, None)
        _save_file(data)

    def valid_access_token(self, refresh_fn) -> str:
        """Return a non-expired access token, refreshing via `refresh_fn(refresh_token)
        -> TokenSet` if needed. Raises AuthError when there's nothing usable."""
        ts = self.load()
        if ts is None or not (ts.access_token or ts.refresh_token):
            raise AuthError("not logged in — run `llm-chat login`")
        if ts.access_token and not ts.is_expired():
            return ts.access_token
        if ts.refresh_token:
            new = refresh_fn(ts.refresh_token)
            self.save(new)
            return new.access_token
        raise AuthError("session expired — run `llm-chat login`")
