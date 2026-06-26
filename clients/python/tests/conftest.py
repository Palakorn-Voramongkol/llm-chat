"""Make the sibling `llm_chat` package importable when running pytest, and
isolate tests from the developer's real repo-root `.env.local`."""

from __future__ import annotations

import os
import sys

import pytest

# clients/python/ (parent of this tests/ dir) holds the llm_chat package.
sys.path.insert(0, os.path.abspath(os.path.join(os.path.dirname(__file__), "..")))


@pytest.fixture(autouse=True)
def _no_real_env_local(tmp_path, monkeypatch):
    """Point load_env_local() at a nonexistent path so the suite never loads the
    real .env.local. Tests set connection env vars explicitly via monkeypatch."""
    monkeypatch.setenv("LLM_CHAT_ENV_FILE", str(tmp_path / "nonexistent.env.local"))
