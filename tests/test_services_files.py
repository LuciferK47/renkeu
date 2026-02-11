"""Tests for services.files helpers."""

import asyncio
from pathlib import Path

import pytest

from latex_ai.services.files import (
    atomic_write,
    compute_etag,
    save_with_etag_history_cache,
)


@pytest.mark.anyio
async def test_atomic_write(tmp_path: Path):
    target = tmp_path / "file.txt"
    await atomic_write(target, "hello")
    assert target.read_text() == "hello"


@pytest.mark.anyio
async def test_etag_conflict(tmp_path: Path):
    target = tmp_path / "doc.tex"
    await atomic_write(target, "one")
    etag = compute_etag("one")

    # Stale ETag should conflict
    result = await save_with_etag_history_cache(target, "two", if_match="stale")
    assert result["status"] == 409
    assert result["conflict"]["current_etag"] == etag
    assert target.read_text() == "one"  # unchanged


@pytest.mark.anyio
async def test_save_updates_etag(tmp_path: Path):
    target = tmp_path / "doc.tex"
    await atomic_write(target, "one")
    etag_old = compute_etag("one")

    result = await save_with_etag_history_cache(target, "two", if_match=etag_old)
    assert result["success"]
    assert result["etag"] != etag_old
    assert target.read_text() == "two"


@pytest.mark.anyio
async def test_save_creates_file_without_etag(tmp_path: Path):
    target = tmp_path / "new.tex"
    result = await save_with_etag_history_cache(target, "new content", if_match=None)
    assert result["success"]
    assert target.read_text() == "new content"
