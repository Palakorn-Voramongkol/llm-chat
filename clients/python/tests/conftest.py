"""Make the sibling `llm_chat` package importable when running pytest."""

from __future__ import annotations

import os
import sys

# clients/python/ (parent of this tests/ dir) holds the llm_chat package.
sys.path.insert(0, os.path.abspath(os.path.join(os.path.dirname(__file__), "..")))
