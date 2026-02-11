import pytest
from pathlib import Path

from src.latex_ai.services.ai import PatchBlock, apply_patch_blocks


@pytest.mark.asyncio
async def test_apply_patch_blocks_success(tmp_path: Path):
    target = tmp_path / "file.txt"
    target.write_text("alpha\nbeta\ngamma\n")

    blocks = [PatchBlock(search="beta", replace="BETA")] 
    success, new_content = await apply_patch_blocks(target, blocks)

    assert success is True
    assert "BETA" in new_content
    assert "beta" not in new_content


@pytest.mark.asyncio
async def test_apply_patch_blocks_missing_search(tmp_path: Path):
    target = tmp_path / "file.txt"
    target.write_text("alpha\nbeta\ngamma\n")

    blocks = [PatchBlock(search="delta", replace="DELTA")]
    success, new_content = await apply_patch_blocks(target, blocks)

    assert success is False
    # Content should remain unchanged on failure
    assert new_content == target.read_text()
