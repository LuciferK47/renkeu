"""Tests for agent history filtering endpoint."""

from fastapi.testclient import TestClient


def test_agent_history_endpoint_returns_agent_entries(tmp_path, monkeypatch):
    monkeypatch.setenv("WORKSPACE_ROOT", str(tmp_path))

    from latex_ai.server import app

    with TestClient(app) as client:
        target = tmp_path / "agent_history.tex"

        original = "\\documentclass{article}\n\\begin{document}\nOriginal\n\\end{document}\n"
        target.write_text(original)

        # Create a baseline history entry via file save (manual-save)
        baseline = original + "% baseline\n"
        resp_save = client.post("/api/file", json={"path": str(target), "content": baseline})
        assert resp_save.status_code in (200, 201)

        # Apply an agent edit to generate an ai-agent-apply entry
        new_content = baseline.replace("Original", "Changed")
        agent_req = {
            "edits": [
                {
                    "file_path": str(target),
                    "op": "rewrite_file",
                    "payload": new_content,
                }
            ],
            "auto_compile": False,
        }
        resp_agent = client.post("/api/agent", json=agent_req)
        assert resp_agent.status_code == 200

        # Fetch agent-filtered history; should include only agent commands
        resp_hist = client.get(f"/api/history/agent?path={target}")
        assert resp_hist.status_code == 200
        entries = resp_hist.json()
        assert entries, "Expected at least one agent history entry"
        for entry in entries:
            assert entry["command"] in ("ai-agent-apply", "agent-apply")

        # Ensure the latest entry corresponds to our agent change
        latest = entries[0]
        assert "Changed" in latest.get("diff", "")
