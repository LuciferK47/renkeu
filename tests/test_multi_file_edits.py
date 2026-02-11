"""Tests for multi-file typed edits application logic."""

import os
from pathlib import Path
from fastapi.testclient import TestClient
import pytest


@pytest.mark.asyncio
async def test_multi_file_edits_apply_with_etag_conflict(tmp_path, monkeypatch):
    """Apply multi-file edits via shared helper and verify conflicts and diffs."""
    # Configure workspace root
    monkeypatch.setenv("WORKSPACE_ROOT", str(tmp_path))

    from latex_ai.server import AgentEdit
    from latex_ai.services.ai import apply_agent_edits
    from latex_ai.services.files import compute_etag
    from latex_ai.state import state

    # Ensure history DB is initialized by touching the server app
    from latex_ai.server import app
    # Ensure lifespan runs so history_db is initialized
    with TestClient(app) as _client:
        pass

    # Create two files in workspace
    file1 = tmp_path / "file1.tex"
    file2 = tmp_path / "file2.tex"
    file1_content = "\\documentclass{article}\n\\begin{document}\nHello\n\\end{document}\n"
    file2_content = "\\documentclass{article}\n\\begin{document}\nWorld\n\\end{document}\n"
    file1.write_text(file1_content)
    file2.write_text(file2_content)

    # Compute correct ETag for file1, and craft a stale one to force conflict
    correct_etag = compute_etag(file1_content)
    stale_etag = "deadbeefdeadbeef"  # invalid but should trigger mismatch

    # Prepare edits: insert into file1, rewrite file2
    edit1_conflict = AgentEdit(
        file_path=str(file1),
        op="insert",
        payload="",  # use typed fields
        line=2,
        text="NEW LINE",
        expected_etag=stale_etag,
    )
    edit2_ok = AgentEdit(
        file_path=str(file2),
        op="rewrite_file",
        payload=file2_content.replace("World", "Changed"),
    )

    # Apply edits with conflict on file1; auto_compile disabled for speed
    result = state.history_db and (await apply_agent_edits([edit1_conflict, edit2_ok], history_db=state.history_db, auto_compile=False))
    assert result is not None

    # One conflict expected, one applied
    assert result["applied"] == 1
    assert len(result["conflicts"]) == 1
    assert result["success"] is False

    # Verify file2 changed and file1 unchanged
    assert file2.read_text() != file2_content
    assert "Changed" in file2.read_text()
    assert file1.read_text() == file1_content

    # Now apply edit1 with correct ETag; should succeed
    edit1_ok = AgentEdit(
        file_path=str(file1),
        op="insert",
        payload="",
        line=2,
        text="NEW LINE",
        expected_etag=correct_etag,
    )

    result2 = state.history_db and (await apply_agent_edits([edit1_ok], history_db=state.history_db, auto_compile=False))
    assert result2 is not None
    assert result2["success"] is True
    assert result2["applied"] == 1
    assert len(result2["conflicts"]) == 0

    # Verify insertion at line 2 (1-based)
    lines = file1.read_text().splitlines()
    assert lines[1] == "NEW LINE"
