"""Shared types for LaTeX-AI."""
from __future__ import annotations
from dataclasses import dataclass, field
from pathlib import Path
from typing import Optional, Any


@dataclass
class EditResult:
    """Result of applying a LaTeX editing command."""
    changed: bool
    message: str
    new_text: Optional[str] = None
    backup_path: Optional[Path] = None


@dataclass
class ConversationMessage:
    """A single message in a conversation."""
    role: str  # 'user' or 'assistant'
    content: str
    timestamp: Optional[float] = None


@dataclass
class PlanRequest:
    """Request for AI planning with optional conversation context."""
    file_path: str
    request_text: str
    model: Optional[str] = None
    context: list[ConversationMessage] = field(default_factory=list)
    session_id: Optional[str] = None
