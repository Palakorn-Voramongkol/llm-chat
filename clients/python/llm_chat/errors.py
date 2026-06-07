"""Exception hierarchy for the llm-chat client.

A single base (`LlmChatError`) so callers can catch everything, with specific
subclasses so the CLI can map each failure to a clear message and exit code.
"""

from __future__ import annotations


class LlmChatError(Exception):
    """Base class for all llm-chat client errors."""


class CredentialError(LlmChatError):
    """A required credential (issuer / project / key file) is missing or unreadable."""


class AuthError(LlmChatError):
    """The Zitadel token exchange failed (network, bad key, rejected assertion)."""


class ManagerUnavailable(LlmChatError):
    """Could not establish/maintain the WebSocket connection to the manager."""


class ProtocolError(LlmChatError):
    """The manager sent something we couldn't interpret, or returned an `err` frame."""


class AnswerTimeout(LlmChatError):
    """No answer arrived within the allotted time."""
