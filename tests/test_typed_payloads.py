import pytest
from pathlib import Path

from src.latex_ai.server import AgentEdit, AgentRequest, agent_edit_protocol_endpoint


@pytest.mark.asyncio
async def test_replace_range_typed(tmp_path: Path, monkeypatch):
    monkeypatch.setenv("WORKSPACE_ROOT", str(tmp_path))
    target = tmp_path / "file.tex"
    target.write_text("a\nb\nc\n")

    edit = AgentEdit(
        file_path=str(target),
        op="replace_range",
        start_line=2,
        end_line=2,
        text="B",
    )
    req = AgentRequest(edits=[edit], auto_compile=False)
    resp = await agent_edit_protocol_endpoint(req)

    assert resp.success is True
    assert "B" in target.read_text()
    assert "b" not in target.read_text()


@pytest.mark.asyncio
async def test_insert_typed(tmp_path: Path, monkeypatch):
    monkeypatch.setenv("WORKSPACE_ROOT", str(tmp_path))
    target = tmp_path / "file.tex"
    target.write_text("a\nc\n")

    edit = AgentEdit(
        file_path=str(target),
        op="insert",
        line=2,
        insert_text="b",
    )
    req = AgentRequest(edits=[edit], auto_compile=False)
    resp = await agent_edit_protocol_endpoint(req)

    assert resp.success is True
    content = target.read_text().splitlines()
    assert content == ["a", "b", "c"]
