"""Tests for undo/redo endpoints backed by the history database."""

import os
from pathlib import Path
from fastapi.testclient import TestClient


def test_undo_redo_workflow(tmp_path, monkeypatch):
    """Verify that /api/undo restores previous content and /api/redo re-applies it."""
    # Configure workspace root to temporary directory
    monkeypatch.setenv("WORKSPACE_ROOT", str(tmp_path))

    from latex_ai.server import app
    from latex_ai.state import state

    # Use context manager to ensure lifespan startup initializes history_db
    with TestClient(app) as client:
        # Create a test file within the workspace
        test_file = tmp_path / "undo_redo.tex"
        original = "\\documentclass{article}\n\\begin{document}\nOriginal\n\\end{document}\n"
        test_file.write_text(original)

        # Create a baseline history entry by saving slightly modified content
        baseline = original + "% baseline\n"
        save_resp = client.post("/api/file", json={"path": str(test_file), "content": baseline})
        assert save_resp.status_code in (200, 201)

        # Apply an agent edit (rewrite_file) to change content
        new_content = original.replace("Original", "Changed")
        agent_req = {
            "edits": [
                {
                    "file_path": str(test_file),
                    "op": "rewrite_file",
                    "payload": new_content,
                }
            ],
            "auto_compile": False,
        }

        resp = client.post("/api/agent", json=agent_req)
        assert resp.status_code == 200
        assert test_file.read_text() == new_content

        # Undo the last agent edit
        undo_resp = client.post("/api/undo", json={"file_path": str(test_file)})
        assert undo_resp.status_code == 200
        undo_data = undo_resp.json()
        assert undo_data.get("success") is True
        # Content should be restored to original
        assert test_file.read_text() == baseline

        # Determine the agent apply entry id to redo
        assert state.history_db is not None
        entries = state.history_db.get_history(str(test_file), 100)
        assert entries, "History should contain entries after agent apply and undo"
        # History is returned with most recent first; find the last agent apply entry
        agent_apply_id = None
        for e in entries:
            if e.command in ("ai-agent-apply", "agent-apply"):
                agent_apply_id = e.id
                break
        assert agent_apply_id is not None, "Should find an agent apply history entry"

        # Redo to the agent apply entry
        redo_resp = client.post("/api/redo", json={"file_path": str(test_file), "history_id": agent_apply_id})
        assert redo_resp.status_code == 200
        redo_data = redo_resp.json()
        assert redo_data.get("success") is True
        # Content should be restored to the changed version
        assert test_file.read_text() == new_content
