"""Security tests for path validation and file access controls."""

import pytest
from pathlib import Path
from fastapi import HTTPException
import tempfile
import os


def test_path_traversal_blocked():
    """Test that parent directory traversal is blocked."""
    from latex_ai.server import validate_path, get_workspace_root
    
    # Try to access parent directory
    with pytest.raises(HTTPException) as exc_info:
        validate_path("../../etc/passwd")
    
    assert exc_info.value.status_code == 403
    assert "outside workspace" in exc_info.value.detail.lower()


def test_absolute_path_outside_workspace_blocked():
    """Test that absolute paths outside workspace are blocked."""
    from latex_ai.server import validate_path
    
    # Try to access system directory
    if os.name == 'nt':
        evil_path = "C:/Windows/System32/config"
    else:
        evil_path = "/etc/passwd"
    
    with pytest.raises(HTTPException) as exc_info:
        validate_path(evil_path)
    
    assert exc_info.value.status_code == 403
    assert "outside workspace" in exc_info.value.detail.lower()


def test_symlink_escape_blocked():
    """Test that symlink attacks escaping workspace are blocked."""
    from latex_ai.server import validate_path, get_workspace_root
    import platform
    
    workspace = get_workspace_root()
    
    # Skip on Windows if user lacks privilege (expected in non-admin environment)
    if platform.system() == 'Windows':
        pytest.skip("Requires admin privileges for symlink creation on Windows")
    
    # Create a symlink pointing outside workspace
    with tempfile.TemporaryDirectory() as tmpdir:
        symlink_path = workspace / "evil_link"
        target_path = Path(tmpdir) / "outside.txt"
        target_path.write_text("secret")
        
        try:
            # Create symlink
            if hasattr(os, 'symlink'):
                symlink_path.symlink_to(target_path)
                
                # Try to access through symlink
                with pytest.raises(HTTPException) as exc_info:
                    validate_path(str(symlink_path))
                
                assert exc_info.value.status_code == 403
                assert "outside workspace" in exc_info.value.detail.lower()
        finally:
            # Cleanup
            if symlink_path.exists():
                symlink_path.unlink()


def test_hidden_file_access_blocked():
    """Test that hidden files and directories are blocked."""
    from latex_ai.server import validate_path
    
    # Try to access .git directory
    with pytest.raises(HTTPException) as exc_info:
        validate_path(".git/config")
    
    assert exc_info.value.status_code == 403
    assert "hidden" in exc_info.value.detail.lower()
    
    # Try to access .env file
    with pytest.raises(HTTPException) as exc_info:
        validate_path(".env")
    
    assert exc_info.value.status_code == 403
    assert "hidden" in exc_info.value.detail.lower()


def test_valid_workspace_path_allowed():
    """Test that valid paths within workspace are allowed."""
    from latex_ai.server import validate_path, get_workspace_root
    
    workspace = get_workspace_root()
    
    # Create a test file
    test_file = workspace / "test_valid.tex"
    test_file.write_text("% Test LaTeX")
    
    try:
        # Should not raise exception
        result = validate_path("test_valid.tex")
        assert result == test_file
        assert result.is_relative_to(workspace)
    finally:
        # Cleanup
        if test_file.exists():
            test_file.unlink()


def test_nested_valid_path_allowed():
    """Test that nested paths within workspace are allowed."""
    from latex_ai.server import validate_path, get_workspace_root
    
    workspace = get_workspace_root()
    
    # Create nested directory structure
    nested_dir = workspace / "subdir" / "nested"
    nested_dir.mkdir(parents=True, exist_ok=True)
    test_file = nested_dir / "document.tex"
    test_file.write_text("% Nested document")
    
    try:
        # Should not raise exception
        result = validate_path("subdir/nested/document.tex")
        assert result == test_file
        assert result.is_relative_to(workspace)
    finally:
        # Cleanup
        if test_file.exists():
            test_file.unlink()
        if nested_dir.exists():
            nested_dir.rmdir()
        if (workspace / "subdir").exists():
            (workspace / "subdir").rmdir()


def test_special_characters_in_filename():
    """Test that files with special characters are handled correctly."""
    from latex_ai.server import validate_path, get_workspace_root
    
    workspace = get_workspace_root()
    
    # Files with spaces and special chars (but not hidden)
    test_file = workspace / "my document (v2).tex"
    test_file.write_text("% Document with spaces")
    
    try:
        # Should not raise exception
        result = validate_path("my document (v2).tex")
        assert result == test_file
    finally:
        # Cleanup
        if test_file.exists():
            test_file.unlink()


def test_invalid_path_characters():
    """Test that invalid path strings are rejected."""
    from latex_ai.server import validate_path
    
    # Null bytes
    with pytest.raises(HTTPException) as exc_info:
        validate_path("test\x00.tex")
    
    assert exc_info.value.status_code in (400, 403)


def test_etag_computation_consistency():
    """Test that ETag computation is consistent for same content."""
    from latex_ai.server import compute_etag
    
    content1 = "\\documentclass{article}\n\\begin{document}\nHello\\end{document}"
    content2 = "\\documentclass{article}\n\\begin{document}\nHello\\end{document}"
    content3 = "\\documentclass{article}\n\\begin{document}\nWorld\\end{document}"
    
    etag1 = compute_etag(content1)
    etag2 = compute_etag(content2)
    etag3 = compute_etag(content3)
    
    # Same content should produce same ETag
    assert etag1 == etag2
    
    # Different content should produce different ETag
    assert etag1 != etag3
    
    # ETag should be 16 characters (hex digest prefix)
    assert len(etag1) == 16
    assert all(c in '0123456789abcdef' for c in etag1)
