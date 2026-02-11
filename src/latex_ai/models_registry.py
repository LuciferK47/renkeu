"""Centralized AI model registry for LaTeX-AI."""

from pydantic import BaseModel
from typing import Dict


class ModelInfo(BaseModel):
    """AI model information."""
    id: str
    label: str
    provider: str = "Gemini"
    tier: str = "balanced"
    supports: Dict[str, bool] = {}


# Single source of truth for available AI models
AVAILABLE_MODELS = [
    ModelInfo(
        id="gpt-5.1-codex-max",
        label="GPT-5.1 Codex Max (compat)",
        provider="Compat",
        tier="best",
        supports={"plan": True, "apply": False, "long_context": True, "code": True, "json": True}
    ),
    ModelInfo(
        id="gemini-pro",
        label="Gemini Pro",
        provider="Gemini",
        tier="balanced",
        supports={"plan": True, "apply": False, "long_context": True, "code": True, "json": True}
    ),
    ModelInfo(
        id="gemini-1.5-pro",
        label="Gemini 1.5 Pro",
        provider="Gemini",
        tier="balanced",
        supports={"plan": True, "apply": False, "long_context": True, "code": True, "json": True}
    ),
    ModelInfo(
        id="gemini-1.5-flash",
        label="Gemini 1.5 Flash",
        provider="Gemini",
        tier="fast/cheap",
        supports={"plan": True, "apply": False, "long_context": False, "code": True, "json": True}
    ),
]

# Quick lookup by ID
AVAILABLE_MODEL_IDS = {m.id for m in AVAILABLE_MODELS}
