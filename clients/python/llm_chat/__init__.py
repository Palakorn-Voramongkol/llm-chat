"""llm-chat Python client.

A small, dependency-light client for the llm-chat manager's `/chat` WebSocket:

- `llm_chat.auth`     — resolve credentials (flags / env / secrets dir) and mint
                        a Zitadel JWT-bearer access token.
- `llm_chat.protocol` — `ChatClient`, an async client that connects, asks
                        questions on a persistent session, and survives drops.
- `llm_chat.cli`      — the `llm-chat` command (`ask` one-shot, `chat` REPL).

Public API is re-exported here for convenience.
"""

from __future__ import annotations

from .auth import Credentials, fetch_access_token, resolve_credentials
from .errors import (
    AnswerTimeout,
    AuthError,
    CredentialError,
    LlmChatError,
    ManagerUnavailable,
    ProtocolError,
)
from .protocol import Answer, ChatClient

__version__ = "1.0.0"

__all__ = [
    "__version__",
    "Credentials",
    "resolve_credentials",
    "fetch_access_token",
    "ChatClient",
    "Answer",
    "LlmChatError",
    "CredentialError",
    "AuthError",
    "ManagerUnavailable",
    "ProtocolError",
    "AnswerTimeout",
]
