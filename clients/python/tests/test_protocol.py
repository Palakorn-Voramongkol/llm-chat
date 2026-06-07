"""Unit tests for ChatClient frame handling, using a scripted fake WebSocket."""

from __future__ import annotations

import asyncio
import json
from collections import deque

import pytest

from llm_chat import protocol
from llm_chat.errors import AnswerTimeout, ProtocolError
from llm_chat.protocol import ChatClient


class FakeWS:
    """A websockets-like double: recv() pops scripted frames; send() records."""

    def __init__(self, incoming):
        self._incoming = deque(incoming)
        self.sent = []
        self.closed = False

    async def recv(self):
        if self._incoming:
            return self._incoming.popleft()
        # Out of frames: block so asyncio.wait_for can time out.
        await asyncio.Event().wait()

    async def send(self, data):
        self.sent.append(json.loads(data))

    async def close(self):
        self.closed = True


def _install_fake(monkeypatch, frames):
    fake = FakeWS([json.dumps(f) for f in frames])

    async def fake_connect(url, **kwargs):
        return fake

    monkeypatch.setattr(protocol.websockets, "connect", fake_connect)
    return fake


def _client():
    return ChatClient("ws://test/chat", token_provider=lambda: "tok")


async def test_ask_returns_answer_and_confirms(monkeypatch):
    fake = _install_fake(monkeypatch, [
        {"type": "initialized", "sid": "s-1", "backendPort": 7878},
        {"type": "ack", "id": "m1", "seq": 1},
        {"type": "a", "id": "m1", "seq": 1, "text": "4",
         "timeIn": "2026-06-07T08:00:00.000+00:00", "timeOut": "2026-06-07T08:00:06.000+00:00"},
    ])
    async with _client() as client:
        assert client.session_id == "s-1"
        answer = await client.ask("2+2?", timeout=5)
    assert answer.text == "4"
    assert answer.seq == 1
    assert answer.latency_s == pytest.approx(6.0, abs=0.01)
    # We sent the question and a confirm for seq 1.
    assert {"type": "q", "id": "m1", "text": "2+2?"} in fake.sent
    assert {"type": "confirm", "seq": 1} in fake.sent


async def test_ask_skips_unrelated_frames(monkeypatch):
    _install_fake(monkeypatch, [
        {"type": "initialized", "sid": "s-1"},
        {"type": "ack", "id": "m1", "seq": 1},
        {"type": "a", "id": "OTHER", "seq": 99, "text": "stale"},   # wrong id → skip
        {"type": "a", "id": "m1", "seq": 1, "text": "real"},
    ])
    async with _client() as client:
        answer = await client.ask("q", timeout=5)
    assert answer.text == "real"


async def test_ask_raises_on_err_frame(monkeypatch):
    _install_fake(monkeypatch, [
        {"type": "initialized", "sid": "s-1"},
        {"type": "err", "text": "empty question"},
    ])
    async with _client() as client:
        with pytest.raises(ProtocolError, match="empty question"):
            await client.ask("q", timeout=5)


async def test_ask_times_out(monkeypatch):
    _install_fake(monkeypatch, [
        {"type": "initialized", "sid": "s-1"},
        {"type": "ack", "id": "m1", "seq": 1},
        # no `a` frame ever arrives
    ])
    async with _client() as client:
        with pytest.raises(AnswerTimeout):
            await client.ask("q", timeout=0.2)
