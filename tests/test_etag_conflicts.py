"""Tests for ETag-based conflict detection and resolution."""

import pytest
from pathlib import Path
from fastapi.testclient import TestClient
import json
import tempfile


@pytest.fixture
def test_file(tmp_path):
    """Create a temporary test file."""
    file_path = tmp_path / "test.tex"
    initial_content = "\\documentclass{article}\n\\begin{document}\nOriginal\\end{document}"
    file_path.write_text(initial_content)
    return file_path


@pytest.fixture
def client(monkeypatch, tmp_path):
    """Create a test client with temporary workspace."""
    from latex_ai.server import app, get_workspace_root
    
    # Mock workspace to use temporary directory
    monkeypatch.setenv('WORKSPACE_ROOT', str(tmp_path))
    
    return TestClient(app)


def test_etag_returned_on_file_read(client, test_file):
    """Test that GET /api/file returns ETag header."""
    response = client.get(f"/api/file?path={test_file}")
    
    assert response.status_code == 200
    assert "etag" in response.headers
    assert len(response.headers["etag"]) == 16
    
    data = response.json()
    assert "content" in data


def test_successful_save_with_matching_etag(client, test_file):
    """Test that save succeeds when ETag matches."""
    # Get file with ETag
    get_response = client.get(f"/api/file?path={test_file}")
    original_etag = get_response.headers["etag"]
    
    # Save with matching ETag
    new_content = "\\documentclass{article}\n\\begin{document}\nModified\\end{document}"
    save_response = client.post(
        "/api/file",
        json={"path": str(test_file), "content": new_content},
        headers={"If-Match": original_etag}
    )
    
    assert save_response.status_code == 200
    assert "etag" in save_response.headers
    
    # New ETag should be different
    new_etag = save_response.headers["etag"]
    assert new_etag != original_etag
    
    # Verify content was saved
    assert test_file.read_text() == new_content


def test_conflict_detected_with_stale_etag(client, test_file):
    """Test that 409 conflict is returned when ETag doesn't match."""
    # Get file with ETag
    get_response = client.get(f"/api/file?path={test_file}")
    original_etag = get_response.headers["etag"]
    
    # Simulate concurrent modification
    concurrent_content = "\\documentclass{article}\n\\begin{document}\nConcurrent\\end{document}"
    test_file.write_text(concurrent_content)
    
    # Try to save with stale ETag
    my_content = "\\documentclass{article}\n\\begin{document}\nMy changes\\end{document}"
    save_response = client.post(
        "/api/file",
        json={"path": str(test_file), "content": my_content},
        headers={"If-Match": original_etag}
    )
    
    # Should get conflict
    assert save_response.status_code == 409
    
    conflict_data = save_response.json()
    assert "current_content" in conflict_data
    assert "current_etag" in conflict_data
    assert conflict_data["current_content"] == concurrent_content
    
    # File should still have concurrent content (save rejected)
    assert test_file.read_text() == concurrent_content


def test_save_without_etag_succeeds():
    """Test that save without If-Match header still works (backward compatibility)."""
    from latex_ai.server import app
    from fastapi.testclient import TestClient
    
    client = TestClient(app)
    
    # Save without ETag header (first-time save or old client)
    content = "\\documentclass{article}\n\\begin{document}\nNo ETag\\end{document}"
    response = client.post(
        "/api/file",
        json={"path": "new_file.tex", "content": content}
    )
    
    # Should succeed without conflict check
    assert response.status_code in (200, 201)


def test_etag_changes_after_modification(client, test_file):
    """Test that ETag changes when file content is modified."""
    # Get initial ETag
    response1 = client.get(f"/api/file?path={test_file}")
    etag1 = response1.headers["etag"]
    
    # Modify file
    new_content = test_file.read_text() + "\n% Added comment"
    test_file.write_text(new_content)
    
    # Get new ETag
    response2 = client.get(f"/api/file?path={test_file}")
    etag2 = response2.headers["etag"]
    
    # ETags should differ
    assert etag1 != etag2


def test_identical_content_produces_same_etag(client, test_file):
    """Test that identical content produces the same ETag."""
    content = test_file.read_text()
    
    # Get ETag
    response1 = client.get(f"/api/file?path={test_file}")
    etag1 = response1.headers["etag"]
    
    # Rewrite with same content
    test_file.write_text(content)
    
    # Get ETag again
    response2 = client.get(f"/api/file?path={test_file}")
    etag2 = response2.headers["etag"]
    
    # Should be identical
    assert etag1 == etag2


def test_concurrent_save_workflow(client, test_file):
    """Test realistic concurrent editing scenario."""
    # User A and B both load the file
    response_a = client.get(f"/api/file?path={test_file}")
    response_b = client.get(f"/api/file?path={test_file}")
    
    etag_a = response_a.headers["etag"]
    etag_b = response_b.headers["etag"]
    
    assert etag_a == etag_b  # Same initial version
    
    # User A saves first
    content_a = "\\documentclass{article}\n\\begin{document}\nUser A\\end{document}"
    save_a = client.post(
        "/api/file",
        json={"path": str(test_file), "content": content_a},
        headers={"If-Match": etag_a}
    )
    
    assert save_a.status_code == 200
    new_etag_a = save_a.headers["etag"]
    
    # User B tries to save with stale ETag
    content_b = "\\documentclass{article}\n\\begin{document}\nUser B\\end{document}"
    save_b = client.post(
        "/api/file",
        json={"path": str(test_file), "content": content_b},
        headers={"If-Match": etag_b}  # Stale!
    )
    
    # User B should get conflict
    assert save_b.status_code == 409
    conflict = save_b.json()
    assert conflict["current_content"] == content_a
    assert conflict["current_etag"] == new_etag_a
    
    # User B can now resolve by using new ETag
    save_b_retry = client.post(
        "/api/file",
        json={"path": str(test_file), "content": content_b},
        headers={"If-Match": new_etag_a}  # Updated ETag
    )
    
    assert save_b_retry.status_code == 200


def test_invalid_etag_format():
    """Test that invalid ETag format is handled gracefully."""
    from latex_ai.server import app
    from fastapi.testclient import TestClient
    
    client = TestClient(app)
    
    # Try to save with malformed ETag
    response = client.post(
        "/api/file",
        json={"path": "test.tex", "content": "content"},
        headers={"If-Match": "invalid-format-12345"}
    )
    
    # Should either reject or ignore invalid ETag
    assert response.status_code in (200, 400, 409)


def test_etag_case_sensitivity():
    """Test that ETag comparison is case-sensitive."""
    from latex_ai.server import compute_etag
    
    content = "Test content"
    etag = compute_etag(content)
    
    # ETag should be lowercase hex
    assert etag == etag.lower()
    assert all(c in '0123456789abcdef' for c in etag)
