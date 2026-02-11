import pytest
from pathlib import Path

from src.latex_ai.document_bundle import DocumentBundle


@pytest.mark.asyncio
async def test_bundle_rejects_path_traversal(tmp_path: Path):
    # Arrange: workspace root and main file
    workspace = tmp_path / "workspace"
    workspace.mkdir()
    main_file = workspace / "main.tex"
    main_file.write_text(r"\input{../secret}")

    # A secret file outside workspace should never be read
    outside = tmp_path / "secret.tex"
    outside.write_text("TOP SECRET")

    bundle = DocumentBundle(main_file, workspace_root=workspace)

    # Act
    ok = await bundle.load_async()

    # Assert: load may return True, but must never include outside file
    for path_str in bundle.files.keys():
        assert Path(path_str).resolve().is_relative_to(workspace)

    # Dependencies should not include traversal target
    deps = []
    for v in bundle.dependencies.values():
        deps.extend(v)
    assert all((workspace in Path(d).resolve().parents) for d in deps)
    
    # Either it reports an error or simply skips the traversal reference
    assert ok is True or bundle.errors
