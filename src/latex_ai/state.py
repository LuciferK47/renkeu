"""Application-wide shared state for the LaTeX-AI server."""
from __future__ import annotations

import asyncio
from typing import Any, Dict, Optional

from latex_ai.history_db import HistoryDB


class AppState:
    """Container for mutable runtime state shared across routers."""

    def __init__(self) -> None:
        self.history_db: Optional[HistoryDB] = None
        self.lsp_clients: Dict[str, Any] = {}
        self.compilation_semaphore: Optional[asyncio.Semaphore] = None
        self.startup_time: float = 0.0


state = AppState()
