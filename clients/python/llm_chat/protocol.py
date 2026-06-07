"""Async client for the manager's typed ``/chat`` WebSocket protocol.

Protocol (per message):
    client → ``{"type":"q","id":<id>,"text":<text>}``
    manager → ``{"type":"ack","id","seq"}``           (receipt)
    manager → ``{"type":"a","id","seq","text",...}``  (the answer)
    client → ``{"type":"confirm","seq":<seq>}``       (we got it)
    manager → ``{"type":"err",...}``                  (on failure)

`ChatClient` keeps ONE connection (and therefore one backend session, so claude
retains conversation context across `ask()` calls). If the socket drops it
transparently re-authenticates and reconnects on the next `ask()`.
"""

from __future__ import annotations

import asyncio
import json
import logging
import time
from dataclasses import dataclass
from typing import Awaitable, Callable

import websockets

from .errors import AnswerTimeout, ManagerUnavailable, ProtocolError

log = logging.getLogger("llm_chat.protocol")

# A callable that returns a fresh access token (used for connect + reconnect).
TokenProvider = Callable[[], Awaitable[str]] | Callable[[], str]


@dataclass(frozen=True)
class Answer:
    """A settled answer for one question."""

    text: str
    seq: int
    id: str
    time_in: str | None = None
    time_out: str | None = None

    @property
    def latency_s(self) -> float | None:
        if not (self.time_in and self.time_out):
            return None
        try:
            from datetime import datetime

            fmt = "%Y-%m-%dT%H:%M:%S.%f%z"
            return (
                datetime.strptime(self.time_out, fmt) - datetime.strptime(self.time_in, fmt)
            ).total_seconds()
        except (ValueError, TypeError):
            return None


async def _call_token_provider(provider: TokenProvider) -> str:
    result = provider()
    if asyncio.iscoroutine(result):
        return await result
    return result  # type: ignore[return-value]


class ChatClient:
    """A persistent `/chat` session. Use as an async context manager."""

    def __init__(
        self,
        manager_url: str,
        token_provider: TokenProvider,
        *,
        max_reconnects: int = 1,
    ) -> None:
        self._url = manager_url
        self._token_provider = token_provider
        self._max_reconnects = max_reconnects
        self._ws: websockets.WebSocketClientProtocol | None = None
        self._counter = 0
        self.session_id: str | None = None

    async def __aenter__(self) -> "ChatClient":
        await self.connect()
        return self

    async def __aexit__(self, *exc) -> None:
        await self.close()

    async def connect(self) -> None:
        """Open the WebSocket and drain the manager's ``initialized`` frame.

        Raises:
            ManagerUnavailable: the connection could not be established.
        """
        token = await _call_token_provider(self._token_provider)
        try:
            self._ws = await websockets.connect(
                self._url,
                additional_headers=[("Authorization", f"Bearer {token}")],
                max_size=None,
                open_timeout=15,
            )
        except (OSError, websockets.WebSocketException, asyncio.TimeoutError) as e:
            raise ManagerUnavailable(f"could not connect to {self._url}: {e}") from e

        # The manager sends an `initialized` frame right away; capture the sid.
        try:
            raw = await asyncio.wait_for(self._ws.recv(), timeout=10)
            first = json.loads(raw)
            if first.get("type") == "initialized":
                self.session_id = first.get("sid")
                log.debug("connected sid=%s backendPort=%s", self.session_id, first.get("backendPort"))
            else:
                # Not fatal — push it back conceptually by handling in ask(); but
                # the protocol always leads with `initialized`, so just log.
                log.warning("expected 'initialized', got %r", first.get("type"))
        except (asyncio.TimeoutError, websockets.WebSocketException, ValueError) as e:
            await self.close()
            raise ManagerUnavailable(f"no 'initialized' frame from {self._url}: {e}") from e

    async def close(self) -> None:
        if self._ws is not None:
            try:
                await self._ws.close()
            except Exception:  # noqa: BLE001 — close must never raise
                pass
            self._ws = None

    @property
    def connected(self) -> bool:
        # We don't probe a library-specific `.closed`/`.state` (it moved across
        # websockets versions). `close()` and the drop handler null `_ws`, and
        # ask() catches ConnectionClosed and reconnects — so this is enough.
        return self._ws is not None

    async def ask(self, text: str, *, timeout: float = 120.0) -> Answer:
        """Send a question and return its settled `Answer`.

        Transparently reconnects (up to ``max_reconnects``) if the socket has
        dropped. Raises `AnswerTimeout`, `ProtocolError`, or `ManagerUnavailable`.
        """
        attempts = 0
        while True:
            if not self.connected:
                await self.connect()
            self._counter += 1
            msg_id = f"m{self._counter}"
            try:
                await self._ws.send(json.dumps({"type": "q", "id": msg_id, "text": text}))
                return await self._await_answer(msg_id, time.monotonic() + timeout)
            except websockets.ConnectionClosed as e:
                await self.close()
                attempts += 1
                if attempts > self._max_reconnects:
                    raise ManagerUnavailable(f"connection closed during ask(): {e}") from e
                log.warning("connection dropped, reconnecting (attempt %d)", attempts)
                # loop: reconnect and retry the same question

    async def _await_answer(self, msg_id: str, deadline: float) -> Answer:
        assert self._ws is not None
        while True:
            remaining = deadline - time.monotonic()
            if remaining <= 0:
                raise AnswerTimeout(f"no answer for {msg_id} within the timeout")
            try:
                raw = await asyncio.wait_for(self._ws.recv(), timeout=remaining)
            except asyncio.TimeoutError:
                raise AnswerTimeout(f"no answer for {msg_id} within the timeout") from None
            try:
                msg = json.loads(raw)
            except ValueError as e:
                raise ProtocolError(f"manager sent non-JSON frame: {raw!r}") from e
            mtype = msg.get("type")
            if mtype == "a" and msg.get("id") == msg_id:
                await self._ws.send(json.dumps({"type": "confirm", "seq": msg["seq"]}))
                return Answer(
                    text=msg.get("text", ""),
                    seq=msg.get("seq", -1),
                    id=msg.get("id", msg_id),
                    time_in=msg.get("timeIn"),
                    time_out=msg.get("timeOut"),
                )
            if mtype == "err":
                raise ProtocolError(msg.get("text", "manager returned an error"))
            # `initialized`, `ack`, or an unrelated `a` → keep waiting.
            log.debug("skip frame type=%s id=%s", mtype, msg.get("id"))
